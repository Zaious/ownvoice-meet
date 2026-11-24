// The desktop shell: the session state, the commands the windows call, and window lifetime.
// Listening = system audio (audio.rs) -> the speech-to-text option chosen in settings (gemini.rs,
// openai_rt.rs, stt_chunk.rs) -> captions; or Windows Live Captions read on the device
// (live_captions.rs, sentences.rs) -> captions. Captions are translated by the chosen language model
// (captions.rs, llm.rs), which also writes replies. Options and prices live in catalog.rs.
// Text size, opacity, click-through and capture exclusion: settings.rs (stored) and view.rs.

mod archive;
mod audio;
mod batch;
mod captions;
mod catalog;
mod chatgpt;
mod demo;
mod gemini;
mod gloss;
mod idle;
mod live_captions;
mod llm;
mod local;
mod openai_rt;
mod prompts;
mod recorder;
mod sentences;
mod session;
mod settings;
mod stt_chunk;
mod speakers;
mod summary;
mod usage;
mod vad;
mod view;
mod zh;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_window_state::StateFlags;
use tokio::sync::mpsc;

use session::{Reply, Session};
use settings::{Settings, ViewPatch, Views};

type Shared<'a> = State<'a, Mutex<Session>>;

const SCRIPT_MAX_BYTES: usize = 512 * 1024;
/// The catalog's "record only" option: capture and save, no speech-to-text.
const RECORD_ONLY: &str = "record-only";

/// What is listening, if anything. `generation` tells a finished pipeline whether it is still the
/// current one before it tidies up.
#[derive(Default)]
struct Listener {
    generation: u64,
    running: Option<Running>,
}

enum Running {
    /// System audio feeding a speech-to-text pipeline; stopping it closes the audio channel, which
    /// ends the pipeline.
    Audio(audio::Capture),
    LiveCaptions(live_captions::Listening),
}

impl Running {
    fn stop(self) {
        match self {
            Running::Audio(cap) => cap.stop(),
            Running::LiveCaptions(l) => l.stop(),
        }
    }
}

/// Stop listening and wait for it to wind down. Not called with the listener locked: Live Captions'
/// thread takes that lock as it ends.
fn stop_listening(app: &AppHandle) -> Option<PathBuf> {
    let running = app.state::<Mutex<Listener>>().lock().ok().and_then(|mut l| l.running.take());
    if let Some(r) = running {
        r.stop();
    }
    // The last lines go into the summary too; it comes back after the folder is closed and is
    // written there all the same (archive::set_summary).
    if settings::load(app).summary_every_min > 0.0 {
        batch::flush_now(app);
    }
    speakers::end(app);
    // After the capture has stopped, so the recording gets every chunk.
    archive::end(app)
}

/// A pipeline ended on its own (a rejected key, Live Captions not set up): tidy up if it is still
/// the current one.
fn pipeline_ended(app: &AppHandle, generation: u64) {
    usage::flush(app);
    // Recording in a room: losing the recognizer (a rejected key, say) must not lose the sound.
    let current = app.state::<Mutex<Listener>>().lock().ok().is_some_and(|l| l.generation == generation);
    if current && archive::recording(app) {
        let status = app.state::<Mutex<Session>>().lock().map(|s| s.status.clone()).unwrap_or_default();
        set_status(app, format!("{status}（辨識已停止；錄音繼續，按 ■ 停止才會結束）"));
        return;
    }
    let running = app.state::<Mutex<Listener>>().lock().ok().and_then(|mut l| if l.generation == generation { l.running.take() } else { None });
    match running {
        Some(Running::Audio(cap)) => cap.stop(),
        // Its thread is the one ending: nothing to wait for.
        Some(Running::LiveCaptions(_)) => {}
        None => return,
    }
    set_listening_flag(app, false);
}

/// The options this computer can use: Live Captions only where Windows has it.
fn offered() -> Vec<catalog::Model> {
    catalog::MODELS.iter().filter(|m| m.provider != "windows" || live_captions::installed()).copied().collect()
}

fn lock<'a>(state: &'a Shared<'_>) -> Result<std::sync::MutexGuard<'a, Session>, String> {
    state.lock().map_err(|_| "session state is poisoned".to_string())
}

fn set_status(app: &AppHandle, msg: impl Into<String>) {
    if let Ok(mut s) = app.state::<Mutex<Session>>().lock() {
        s.set_status(app, msg);
    }
}

fn set_listening_flag(app: &AppHandle, on: bool) {
    if let Ok(mut s) = app.state::<Mutex<Session>>().lock() {
        s.listening = on;
    }
    let _ = app.emit("listening", on);
}

/// Everything a window needs to draw itself when it opens.
#[tauri::command]
fn snapshot(state: Shared<'_>) -> Result<Session, String> {
    Ok(lock(&state)?.clone())
}

/// The bilingual script shown in the script window; also context for both translation directions.
#[tauri::command]
fn set_script(app: AppHandle, state: Shared<'_>, text: String) -> Result<(), String> {
    if text.len() > SCRIPT_MAX_BYTES {
        return Err("script must be under 512 KB".into());
    }
    lock(&state)?.script = text.clone();
    settings::save_script(&app, &text)?;
    app.emit("script", text).map_err(|e| e.to_string())
}

/// Fill in the script's missing halves (Chinese meanings, or English to read for Chinese lines) and
/// make the result the open script. Returns how many lines were filled.
#[tauri::command]
async fn gloss_script(app: AppHandle, text: String) -> Result<usize, String> {
    if text.len() > SCRIPT_MAX_BYTES {
        return Err("script must be under 512 KB".into());
    }
    let (filled, n) = gloss::fill_open_script(&app, text).await?;
    if n > 0 {
        app.emit("script", filled).map_err(|e| e.to_string())?;
    }
    Ok(n)
}

/// Settings "測試連線" for a self-hosted transcription server.
#[tauri::command]
async fn test_local_stt(app: AppHandle, url: String, model: String) -> Result<String, String> {
    let client = app.state::<llm::Http>().0.clone();
    local::check_stt(&client, &url, &model, settings::get_key("local")).await
}

/// Settings "測試連線" for a self-hosted chat server: the models it offers.
#[tauri::command]
async fn local_models(app: AppHandle, url: String) -> Result<Vec<String>, String> {
    let client = app.state::<llm::Http>().0.clone();
    local::list_models(&client, &url, settings::get_key("local")).await
}

/// Debug builds only: OWNVOICE_AUTOMATION, e.g. {"open": ["reply", "script"], "listen": true,
/// "script_tab": "Q&A"}, lets scripted demos and checks drive the app without the mouse, which
/// races with whoever is using the computer. Release builds always answer None.
#[tauri::command]
fn automation() -> Option<serde_json::Value> {
    if cfg!(debug_assertions) {
        std::env::var("OWNVOICE_AUTOMATION").ok().and_then(|s| serde_json::from_str(&s).ok())
    } else {
        None
    }
}

/// Chinese typed in the reply window, to become English to read aloud.
#[tauri::command]
fn compose_reply(app: AppHandle, state: Shared<'_>, zh: String) -> Result<u64, String> {
    let mut s = lock(&state)?;
    s.reply_seq += 1;
    let reply = Reply { id: s.reply_seq, zh: zh.clone(), en: String::new(), done: false };
    s.reply = Some(reply.clone());
    drop(s);
    app.emit("reply", &reply).map_err(|e| e.to_string())?;
    llm::spawn_reply(app, reply.id, zh);
    Ok(reply.id)
}

/// Start or stop capturing system audio and transcribing it.
#[tauri::command]
fn set_listening(app: AppHandle, listener: State<'_, Mutex<Listener>>, on: bool) -> Result<bool, String> {
    if !on {
        let saved = stop_listening(&app);
        set_listening_flag(&app, false);
        match saved {
            Some(dir) => set_status(&app, format!("已停止。存檔在 {}", dir.display())),
            None => set_status(&app, "已停止"),
        }
        return Ok(false);
    }
    let mut l = listener.lock().map_err(|_| "listener state is poisoned")?;
    if l.running.is_some() {
        return Ok(true);
    }
    let s = settings::load(&app);
    let Some(m) = offered().into_iter().find(|m| m.id == s.stt_id && m.available) else {
        set_status(&app, "請到「設定」選一個可用的語音辨識");
        return Ok(false);
    };
    let source = audio::Source::from_settings(&s);
    let translator = catalog::MODELS.iter().find(|x| x.id == s.llm_id).map_or("", |x| x.label);
    if m.transport == live_captions::TRANSPORT {
        if source != audio::Source::Loopback {
            set_status(&app, "Windows 即時輔助字幕只能聽電腦播放的聲音；用麥克風請改選雲端辨識");
            return Ok(false);
        }
        // First use needs the user's consent and the meeting language's speech files; Settings walks
        // through both and switches the language itself.
        if !live_captions::prepare(&s.stt_language).ready {
            set_status(&app, "即時輔助字幕還沒準備好：請開「設定」，照「Windows 即時輔助字幕」下面的步驟做完");
            return Ok(false);
        }
        // Live Captions gives text, not sound: only the transcript can be saved.
        let saving = archive::begin(&app, &s, archive::describe(&s, "Windows", m.label, translator), 16_000, false, false)?;
        l.generation += 1;
        let generation = l.generation;
        let end_app = app.clone();
        l.running = Some(Running::LiveCaptions(live_captions::start(app.clone(), s.stt_language.clone(), move || pipeline_ended(&end_app, generation))));
        drop(l);
        set_listening_flag(&app, true);
        if let Some((dir, _)) = saving {
            set_status(&app, format!("聆聽中（即時輔助字幕）。逐字稿存在 {}", dir.display()));
        }
        return Ok(true);
    }
    let record_only = m.transport == RECORD_ONLY;
    // Each option needs its own key (local servers may not need one).
    let key = settings::get_key(m.provider);
    if key.is_none() && m.provider != "local" && !record_only {
        set_status(&app, format!("請先到「設定」填 {} 的 API key", settings::display_name(m.provider)));
        return Ok(false);
    }
    let rate = match m.transport {
        "openai-live" => openai_rt::RATE,
        "gemini-live" => gemini::RATE,
        _ => stt_chunk::RATE,
    };
    let device_hint = match &source {
        audio::Source::Mic { device, .. } if !device.is_empty() => device.clone(),
        audio::Source::Mic { .. } => "預設麥克風".to_string(),
        audio::Source::Loopback => "預設喇叭".to_string(),
    };
    let about = archive::describe(&s, &device_hint, m.label, if record_only { "（不翻譯）" } else { translator });
    let (dir, tap) = match archive::begin(&app, &s, about, rate, true, record_only)? {
        Some((dir, tap)) => (Some(dir), tap),
        None => (None, None),
    };
    // Speaker labels hear the same sound as the recording.
    let tap = speakers::begin(&app, rate, tap);
    let (tx, rx) = mpsc::channel::<Vec<i16>>(64);
    let level_app = app.clone();
    let cap = match audio::start(rate, source, tx, tap, move |rms| {
        let _ = level_app.emit("level", rms);
    }) {
        Ok(cap) => cap,
        Err(e) => {
            speakers::end(&app);
            archive::end(&app);
            return Err(e);
        }
    };
    let saved = match (&dir, archive::recording(&app)) {
        (Some(d), true) => format!(" · 錄音中，存在 {}", d.display()),
        (Some(d), false) => format!(" · 逐字稿存在 {}", d.display()),
        _ => String::new(),
    };
    if record_only {
        set_status(&app, format!("只錄音，不辨識（{}）{saved}", cap.device));
    } else {
        set_status(&app, format!("連線中…（{} · {}）{saved}", m.label, cap.device));
    }
    l.generation += 1;
    let generation = l.generation;
    l.running = Some(Running::Audio(cap));
    drop(l);
    set_listening_flag(&app, true);
    // The speech service, started on a channel; idle.rs lets it go when it is quiet and starts it
    // again when someone speaks.
    let stt_app = app.clone();
    let stt_settings = s.clone();
    let spawn = (!record_only).then(move || {
        move |rx: mpsc::Receiver<Vec<i16>>| {
            let a = stt_app.clone();
            let key = key.clone();
            let s = stt_settings.clone();
            async move {
                let language = s.stt_language.clone();
                match m.transport {
                    "gemini-live" => gemini::run(a, key.unwrap_or_default(), m.api_model.into(), language, rx).await,
                    "openai-live" => openai_rt::run(a, m.id.into(), key.unwrap_or_default(), m.api_model.into(), captions::iso639(&language), rx).await,
                    _ => {
                        let (base_url, model) = if m.provider == "local" {
                            (s.local_stt_url.clone(), s.local_stt_model.clone())
                        } else {
                            ("https://api.openai.com/v1".to_string(), m.api_model.to_string())
                        };
                        let cfg = stt_chunk::Config { catalog_id: m.id.into(), base_url, key, model, language: captions::iso639(&language) };
                        stt_chunk::run(a, cfg, rx).await
                    }
                }
            }
        }
    });
    let (pause_after, end_after, end_min) = (idle::minutes(s.idle_pause_min), idle::minutes(s.idle_end_min), s.idle_end_min);
    let task_app = app.clone();
    tauri::async_runtime::spawn(async move {
        let rec_app = task_app.clone();
        match idle::supervise(task_app.clone(), rx, pause_after, end_after, spawn, move || archive::recording(&rec_app)).await {
            idle::Ended::Stopped => {}
            // The speech service ended on its own (a rejected key, say) with nothing being recorded.
            idle::Ended::Failed => pipeline_ended(&task_app, generation),
            idle::Ended::Idle => auto_end(&task_app, generation, end_min),
        }
    });
    Ok(true)
}

/// Quiet for longer than Settings allows: stop as ■ 停止 would, and say why.
fn auto_end(app: &AppHandle, generation: u64, minutes: f32) {
    let current = app.state::<Mutex<Listener>>().lock().ok().is_some_and(|l| l.generation == generation);
    if !current {
        return;
    }
    let saved = stop_listening(app);
    set_listening_flag(app, false);
    let tail = saved.map(|d| format!("。存檔在 {}", d.display())).unwrap_or_default();
    set_status(app, format!("已自動結束：安靜超過 {} 分鐘{tail}", idle::minutes_label(minutes)));
}

/// The microphones this computer offers.
#[tauri::command]
fn list_microphones() -> Vec<String> {
    audio::input_devices()
}

#[derive(serde::Serialize)]
struct MicCheck {
    device: String,
    peak: f32,
    average: f32,
}

/// Listen to a microphone for a few seconds (the check in Settings); "mic-level" events carry the
/// level as it goes.
#[tauri::command]
async fn mic_check(app: AppHandle, device: String, gain: f32) -> Result<MicCheck, String> {
    let source = audio::Source::Mic { device, gain: gain.clamp(1.0, 16.0) };
    tauri::async_runtime::spawn_blocking(move || {
        audio::measure(source, 6.0, move |rms| {
            let _ = app.emit("mic-level", rms);
        })
    })
    .await
    .map_err(|e| e.to_string())?
    .map(|(device, peak, average)| MicCheck { device, peak, average })
}

/// Start the next segment of the saved session.
#[tauri::command]
fn next_segment(app: AppHandle, name: Option<String>) -> Result<archive::SegmentInfo, String> {
    archive::next_segment(&app, name)
}

#[tauri::command]
fn rename_segment(app: AppHandle, name: String) -> Result<archive::SegmentInfo, String> {
    archive::rename_segment(&app, &name)
}

#[tauri::command]
fn current_segment(app: AppHandle) -> Option<archive::SegmentInfo> {
    archive::current(&app)
}

/// Open the saved sessions folder (the current session's, while one is being saved).
#[tauri::command]
fn open_archive_dir(app: AppHandle) -> Result<(), String> {
    let dir = PathBuf::from(archive::folder_for_display(&app)?);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let opener = if cfg!(target_os = "macos") { "open" } else { "explorer" };
    std::process::Command::new(opener).arg(&dir).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// The options shown in the settings switcher, with prices and descriptions.
#[tauri::command]
fn catalog() -> Vec<catalog::Model> {
    offered()
}

/// How far Windows Live Captions is set up for `language` (the meeting language in the settings
/// form). Switches its language when the speech files are already there.
#[tauri::command]
fn live_captions_setup(language: String) -> live_captions::Setup {
    live_captions::prepare(&language)
}

/// Open what a Live Captions setup step needs: "live-captions" or "language-settings".
#[tauri::command]
fn live_captions_open(what: String) -> Result<(), String> {
    live_captions::open_step(&what)
}

/// Estimated usage and cost: this run, today, this month.
#[tauri::command]
fn usage_report(app: AppHandle) -> usage::Report {
    usage::report(&app)
}

#[tauri::command]
fn get_settings(app: AppHandle) -> Settings {
    settings::load(&app)
}

#[tauri::command]
fn save_settings(app: AppHandle, value: Settings) -> Result<(), String> {
    settings::save(&app, &value)
}

/// Text size and opacity per window, capture exclusion, and the translation-only captions.
#[tauri::command]
fn get_view(app: AppHandle) -> Views {
    settings::load(&app).view
}

/// Change part of the view; every window redraws from the "view" event.
#[tauri::command]
fn set_view(app: AppHandle, patch: ViewPatch) -> Result<Views, String> {
    let v = settings::update_view(&app, &patch)?;
    if patch.hide_from_capture.is_some() {
        view::protect(&app, v.hide_from_capture);
    }
    app.emit("view", &v).map_err(|e| e.to_string())?;
    Ok(v)
}

#[tauri::command]
fn click_through(app: AppHandle) -> bool {
    view::click_through(&app)
}

/// Turn click-through on from the captions window; the tray or Ctrl+Shift+F8 turns it off.
#[tauri::command]
fn set_click_through(app: AppHandle, on: bool) {
    view::set_click_through(&app, on);
}

/// Store or (with an empty key) remove a provider's API key in the OS keychain.
#[tauri::command]
fn set_key(provider: String, key: String) -> Result<(), String> {
    settings::set_key(&provider, &key)
}

/// Which providers have a key. The keys themselves never leave the Rust side.
#[tauri::command]
fn key_status() -> HashMap<String, bool> {
    settings::PROVIDERS.iter().map(|p| (p.to_string(), settings::get_key(p).is_some())).collect()
}

/// Play the canned demo exchange through the normal caption path. It is all canned text: no audio,
/// no network, nothing billed.
#[tauri::command]
fn start_demo(app: AppHandle) {
    demo::start(app);
}

/// Empty the captions (and stop a demo that is playing).
#[tauri::command]
fn clear_captions(app: AppHandle, state: Shared<'_>) {
    if let Ok(mut s) = state.lock() {
        s.clear_captions(&app);
    }
    batch::clear(&app);
    summary::clear(&app);
}

/// The live summary as it stands.
#[tauri::command]
fn get_summary(app: AppHandle) -> summary::View {
    summary::current(&app)
}

/// Bring the summary up to date now with whatever lines are waiting (the 摘要 window's button).
#[tauri::command]
fn summarize_now(app: AppHandle) -> bool {
    batch::flush_now(&app)
}

/// How this copy was installed. "installed" (the Windows installer, which writes uninstall.exe next to
/// the app) may update itself; "portable" (the zip) and "other" (a dev build, other platforms for now)
/// only get a link to the download page.
#[tauri::command]
fn install_kind() -> &'static str {
    if cfg!(debug_assertions) {
        return "other";
    }
    let Ok(exe) = std::env::current_exe() else { return "other" };
    if cfg!(target_os = "macos") {
        // As Basesmall: an app bundle that can replace itself, not one running from the read-only disk
        // image or from where macOS runs a downloaded app it moved aside (App Translocation).
        let path = exe.to_string_lossy();
        let movable = path.contains(".app/Contents/MacOS/") && !path.starts_with("/Volumes/") && !path.contains("/AppTranslocation/");
        return if movable { "installed" } else { "other" };
    }
    if !cfg!(target_os = "windows") {
        return "other";
    }
    let installed = exe.parent().is_some_and(|d| d.join("uninstall.exe").is_file());
    if installed { "installed" } else { "portable" }
}

/// Speaker labels: whether the two models (about 31 MB) are on this computer.
#[tauri::command]
fn speaker_models_ready(app: AppHandle) -> bool {
    speakers::ready(&app)
}

/// Downloads the speaker models when they are missing; progress arrives as "speaker-models" events.
#[tauri::command]
async fn prepare_speakers(app: AppHandle) -> Result<(), String> {
    let client = app.state::<llm::Http>().0.clone();
    speakers::download(&app, &client).await
}

/// The user named speaker `who` (from 0) on a caption line.
#[tauri::command]
fn rename_speaker(app: AppHandle, who: usize, name: String) -> Result<(), String> {
    speakers::rename(&app, who, &name)
}

/// Sign in with ChatGPT: whether this computer is connected, and to which account.
#[tauri::command]
fn chatgpt_status() -> chatgpt::Status {
    chatgpt::status()
}

/// Opens the browser to sign in with ChatGPT and waits (up to 5 minutes) for it to come back.
#[tauri::command]
async fn chatgpt_sign_in(app: AppHandle) -> Result<chatgpt::Status, String> {
    let client = app.state::<llm::Http>().0.clone();
    let previous = chatgpt::load().map(|t| t.client_id);
    let t = chatgpt::sign_in(&client, previous.as_deref(), |url| {
        let _ = tauri_plugin_opener::open_url(url, None::<&str>);
    })
    .await?;
    chatgpt::save(&t)?;
    Ok(chatgpt::status())
}

/// Forgets the ChatGPT connection on this computer (the account itself stays connected in ChatGPT's
/// settings until the user disconnects it there).
#[tauri::command]
fn chatgpt_sign_out() {
    chatgpt::forget();
}

/// "windows", "macos" or "linux": what the settings window may offer (a Mac has no loopback).
#[tauri::command]
fn platform() -> &'static str {
    std::env::consts::OS
}

pub fn run() {
    // Development keys from the repo's git-ignored .env.local; release builds never read it.
    // OWNVOICE_NO_DEV_KEYS=1 skips it, so checks can see the app as a new user would.
    #[cfg(debug_assertions)]
    if std::env::var_os("OWNVOICE_NO_DEV_KEYS").is_none_or(|v| v != "1") {
        let _ = dotenvy::from_path(concat!(env!("CARGO_MANIFEST_DIR"), "/../.env.local"));
    }
    // One TLS crypto provider for both the WebSocket and the HTTP client.
    let _ = rustls::crypto::ring::default_provider().install_default();
    tauri::Builder::default()
        .plugin(tauri_plugin_window_state::Builder::default().with_state_flags(StateFlags::POSITION | StateFlags::SIZE).build())
        // The reply window's focus key works while Zoom or Teams has focus; the web view registers it.
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        // Opens the providers' key pages from Settings; capabilities/settings-links.json lists them.
        .plugin(tauri_plugin_opener::init())
        // The reply and script windows leave screen capture as they are made, not after they draw.
        .plugin(tauri::plugin::Builder::<tauri::Wry>::new("view").on_window_ready(view::on_window_ready).build())
        .manage(Mutex::new(Session::default()))
        .manage(Mutex::new(Listener::default()))
        .manage(Mutex::new(usage::Usage::default()))
        .manage(view::ClickThrough::default())
        .manage(captions::Lines::default())
        .manage(archive::Archive::default())
        .manage(idle::Activity::default())
        .manage(batch::Batches::default())
        .manage(summary::Summary::default())
        .manage(speakers::Speakers::default())
        .manage(llm::Http(reqwest::Client::new()))
        // Closing the captions window quits; the optional windows go with it.
        .on_window_event(|window, event| {
            if window.label() == "reply" && matches!(event, tauri::WindowEvent::CloseRequested { .. }) {
                view::remember_reply(window.app_handle(), false);
            }
            if window.label() == "main" && matches!(event, tauri::WindowEvent::Destroyed) {
                // Closes the Live Captions this app started, rather than leave it running hidden.
                stop_listening(window.app_handle());
                usage::flush(window.app_handle());
                window.app_handle().exit(0);
            }
        })
        .invoke_handler(tauri::generate_handler![
            speaker_models_ready,
            prepare_speakers,
            rename_speaker,
            chatgpt_status,
            chatgpt_sign_in,
            chatgpt_sign_out,
            snapshot,
            set_script,
            compose_reply,
            set_listening,
            get_settings,
            save_settings,
            set_key,
            key_status,
            catalog,
            usage_report,
            live_captions_setup,
            gloss_script,
            automation,
            test_local_stt,
            local_models,
            live_captions_open,
            start_demo,
            clear_captions,
            get_summary,
            summarize_now,
            list_microphones,
            mic_check,
            next_segment,
            rename_segment,
            current_segment,
            open_archive_dir,
            install_kind,
            platform,
            get_view,
            set_view,
            click_through,
            set_click_through
        ])
        // The restart after an in-app update.
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // The script open last time comes back (settings folder, script.md).
            let script = settings::load_script(app.handle());
            if !script.is_empty() {
                if let Ok(mut s) = app.state::<Mutex<Session>>().lock() {
                    s.script = script;
                }
            }
            // In-app updates (only when the user presses Update). The updater's key and address come
            // from tauri.updater.conf.json, which only release builds made with the update key merge
            // in; without it the plugin is left out and Settings offers the download page instead.
            if app.config().plugins.0.contains_key("updater") {
                app.handle().plugin(tauri_plugin_updater::Builder::new().build())?;
            }
            view::setup(app.handle())?;
            // Batches of finished lines every few minutes; the live summary is one taker.
            batch::register(app.handle(), summary::on_batch);
            batch::start(app.handle().clone());
            // Marks the next segment from anywhere (the meeting app or slides in front).
            {
                use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
                let registered = app.global_shortcut().on_shortcut(archive::SEGMENT_SHORTCUT, |app, _, e| {
                    if e.state == ShortcutState::Pressed {
                        match archive::next_segment(app, None) {
                            Ok(seg) => set_status(app, format!("新段落：{:02} · {}", seg.index, seg.name)),
                            Err(msg) => set_status(app, msg),
                        }
                    }
                });
                if let Err(e) = registered {
                    set_status(app.handle(), format!("新段落快捷鍵註冊失敗（可能被其他程式占用）：{e}"));
                }
            }
            // OWNVOICE_DEMO=1 starts the demo on launch (screenshots and smoke checks).
            if std::env::var_os("OWNVOICE_DEMO").is_some_and(|v| v == "1") {
                demo::start(app.handle().clone());
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running OwnVoice Meet");
}

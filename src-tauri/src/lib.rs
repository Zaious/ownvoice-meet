// The desktop shell: the session state, the commands the windows call, and window lifetime.
// Listening = system audio (audio.rs) -> the speech-to-text option chosen in settings (gemini.rs,
// openai_rt.rs, stt_chunk.rs) -> captions; or Windows Live Captions read on the device
// (live_captions.rs, sentences.rs) -> captions. Captions are translated by the chosen language model
// (captions.rs, llm.rs), which also writes replies. Options and prices live in catalog.rs.

mod audio;
mod captions;
mod catalog;
mod demo;
mod gemini;
mod live_captions;
mod llm;
mod openai_rt;
mod prompts;
mod sentences;
mod session;
mod settings;
mod stt_chunk;
mod usage;
mod vad;

use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_window_state::StateFlags;
use tokio::sync::mpsc;

use session::{Reply, Session};
use settings::Settings;

type Shared<'a> = State<'a, Mutex<Session>>;

const SCRIPT_MAX_BYTES: usize = 512 * 1024;

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
fn stop_listening(app: &AppHandle) {
    let running = app.state::<Mutex<Listener>>().lock().ok().and_then(|mut l| l.running.take());
    if let Some(r) = running {
        r.stop();
    }
}

/// A pipeline ended on its own (a rejected key, Live Captions not set up): tidy up if it is still
/// the current one.
fn pipeline_ended(app: &AppHandle, generation: u64) {
    usage::flush(app);
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
    app.emit("script", text).map_err(|e| e.to_string())
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
        stop_listening(&app);
        set_listening_flag(&app, false);
        set_status(&app, "已停止");
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
    if m.transport == live_captions::TRANSPORT {
        l.generation += 1;
        let generation = l.generation;
        let end_app = app.clone();
        l.running = Some(Running::LiveCaptions(live_captions::start(app.clone(), s.stt_language.clone(), move || pipeline_ended(&end_app, generation))));
        drop(l);
        set_listening_flag(&app, true);
        return Ok(true);
    }
    // Each option needs its own key (local servers may not need one).
    let key = settings::get_key(m.provider);
    if key.is_none() && m.provider != "local" {
        set_status(&app, format!("請先到「設定」填 {} 的 API key", settings::display_name(m.provider)));
        return Ok(false);
    }
    let rate = match m.transport {
        "openai-live" => openai_rt::RATE,
        "gemini-live" => gemini::RATE,
        _ => stt_chunk::RATE,
    };
    let (tx, rx) = mpsc::channel::<Vec<i16>>(64);
    let level_app = app.clone();
    let cap = audio::start(rate, tx, move |rms| {
        let _ = level_app.emit("level", rms);
    })?;
    set_status(&app, format!("連線中…（{} · {}）", m.label, cap.device));
    l.generation += 1;
    let generation = l.generation;
    l.running = Some(Running::Audio(cap));
    drop(l);
    set_listening_flag(&app, true);

    let task_app = app.clone();
    let language = s.stt_language.clone();
    tauri::async_runtime::spawn(async move {
        let a = task_app.clone();
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
        // The pipeline ended on its own (a rejected key, say): stop the capture it was fed by.
        pipeline_ended(&task_app, generation);
    });
    Ok(true)
}

/// The options shown in the settings switcher, with prices and descriptions.
#[tauri::command]
fn catalog() -> Vec<catalog::Model> {
    offered()
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

/// Play the canned demo exchange through the normal caption path.
#[tauri::command]
fn start_demo(app: AppHandle) {
    demo::start(app);
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
        .manage(Mutex::new(Session::default()))
        .manage(Mutex::new(Listener::default()))
        .manage(Mutex::new(usage::Usage::default()))
        .manage(captions::Lines::default())
        .manage(llm::Http(reqwest::Client::new()))
        // Closing the captions window quits; the optional windows go with it.
        .on_window_event(|window, event| {
            if window.label() == "main" && matches!(event, tauri::WindowEvent::Destroyed) {
                // Closes the Live Captions this app started, rather than leave it running hidden.
                stop_listening(window.app_handle());
                usage::flush(window.app_handle());
                window.app_handle().exit(0);
            }
        })
        .invoke_handler(tauri::generate_handler![
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
            start_demo
        ])
        .setup(|app| {
            // OWNVOICE_DEMO=1 starts the demo on launch (screenshots and smoke checks).
            if std::env::var_os("OWNVOICE_DEMO").is_some_and(|v| v == "1") {
                demo::start(app.handle().clone());
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running OwnVoice Meet");
}

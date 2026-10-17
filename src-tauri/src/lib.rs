// The desktop shell: the session state, the commands the windows call, and window lifetime.
// Listening = system audio (audio.rs) -> the speech-to-text option chosen in settings (gemini.rs,
// openai_rt.rs, stt_chunk.rs) -> captions; each finished caption is translated by the chosen
// language model (llm.rs), which also writes replies. Options and prices live in catalog.rs.
// Text size, opacity, click-through and capture exclusion: settings.rs (stored) and view.rs.

mod audio;
mod captions;
mod catalog;
mod demo;
mod gemini;
mod llm;
mod openai_rt;
mod prompts;
mod session;
mod settings;
mod stt_chunk;
mod usage;
mod vad;
mod view;

use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_window_state::StateFlags;
use tokio::sync::mpsc;

use session::{Reply, Session};
use settings::{Settings, ViewPatch, Views};

type Shared<'a> = State<'a, Mutex<Session>>;

const SCRIPT_MAX_BYTES: usize = 512 * 1024;

/// The running capture, if listening. `generation` tells a finished pipeline whether it is still
/// the current one before it tidies up.
#[derive(Default)]
struct Listener {
    generation: u64,
    capture: Option<audio::Capture>,
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
    let mut l = listener.lock().map_err(|_| "listener state is poisoned")?;
    if !on {
        if let Some(cap) = l.capture.take() {
            cap.stop(); // closes the audio channel; the Gemini session ends itself
        }
        set_listening_flag(&app, false);
        set_status(&app, "已停止");
        return Ok(false);
    }
    if l.capture.is_some() {
        return Ok(true);
    }
    let s = settings::load(&app);
    let Some(m) = catalog::find(&s.stt_id).filter(|m| m.available) else {
        set_status(&app, "請到「設定」選一個可用的語音辨識");
        return Ok(false);
    };
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
    l.capture = Some(cap);
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
        usage::flush(&task_app);
        // The pipeline ended on its own (a rejected key, say): stop the capture it was fed by.
        let slot = task_app.state::<Mutex<Listener>>();
        if let Ok(mut l) = slot.lock() {
            if l.generation == generation {
                if let Some(cap) = l.capture.take() {
                    cap.stop();
                    set_listening_flag(&task_app, false);
                }
            }
        };
    });
    Ok(true)
}

/// The options shown in the settings switcher, with prices and descriptions.
#[tauri::command]
fn catalog() -> &'static [catalog::Model] {
    catalog::MODELS
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

/// Play the canned demo exchange through the normal caption path.
#[tauri::command]
fn start_demo(app: AppHandle) {
    demo::start(app);
}

/// How this copy was installed. "installed" (the Windows installer, which writes uninstall.exe next to
/// the app) may update itself; "portable" (the zip) and "other" (a dev build, other platforms for now)
/// only get a link to the download page.
#[tauri::command]
fn install_kind() -> &'static str {
    if cfg!(debug_assertions) || !cfg!(target_os = "windows") {
        return "other";
    }
    let Ok(exe) = std::env::current_exe() else { return "other" };
    let installed = exe.parent().is_some_and(|d| d.join("uninstall.exe").is_file());
    if installed { "installed" } else { "portable" }
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
        .manage(llm::Http(reqwest::Client::new()))
        // Closing the captions window quits; the optional windows go with it.
        .on_window_event(|window, event| {
            if window.label() == "main" && matches!(event, tauri::WindowEvent::Destroyed) {
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
            start_demo,
            install_kind,
            get_view,
            set_view,
            click_through,
            set_click_through
        ])
        // The restart after an in-app update.
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            // In-app updates (only when the user presses Update). The updater's key and address come
            // from tauri.updater.conf.json, which only release builds made with the update key merge
            // in; without it the plugin is left out and Settings offers the download page instead.
            if app.config().plugins.0.contains_key("updater") {
                app.handle().plugin(tauri_plugin_updater::Builder::new().build())?;
            }
            view::setup(app.handle())?;
            // OWNVOICE_DEMO=1 starts the demo on launch (screenshots and smoke checks).
            if std::env::var_os("OWNVOICE_DEMO").is_some_and(|v| v == "1") {
                demo::start(app.handle().clone());
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running OwnVoice Meet");
}

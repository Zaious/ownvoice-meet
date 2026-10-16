// The desktop shell: the session state, the commands the windows call, and window lifetime.
// Listening = system audio (audio.rs) -> Gemini Live (gemini.rs) -> captions; each finished caption
// is translated by Claude (llm.rs). Replies go through Claude too.

mod audio;
mod demo;
mod gemini;
mod llm;
mod prompts;
mod session;
mod settings;

use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_window_state::StateFlags;
use tokio::sync::mpsc;

use session::{Reply, Session};
use settings::Settings;

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
    let Some(key) = settings::get_key("gemini") else {
        set_status(&app, "請先到「設定」填 Gemini API key");
        return Ok(false);
    };
    let s = settings::load(&app);
    let (tx, rx) = mpsc::channel::<Vec<i16>>(64);
    let level_app = app.clone();
    let cap = audio::start(tx, move |rms| {
        let _ = level_app.emit("level", rms);
    })?;
    set_status(&app, format!("連線中…（{}）", cap.device));
    l.generation += 1;
    let generation = l.generation;
    l.capture = Some(cap);
    drop(l);
    set_listening_flag(&app, true);

    let task_app = app.clone();
    tauri::async_runtime::spawn(async move {
        gemini::run(task_app.clone(), key, s.stt_model, s.stt_language, rx).await;
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
    // One TLS crypto provider for both the WebSocket and the HTTP client.
    let _ = rustls::crypto::ring::default_provider().install_default();
    tauri::Builder::default()
        .plugin(tauri_plugin_window_state::Builder::default().with_state_flags(StateFlags::POSITION | StateFlags::SIZE).build())
        // The reply window's focus key works while Zoom or Teams has focus; the web view registers it.
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(Mutex::new(Session::default()))
        .manage(Mutex::new(Listener::default()))
        .manage(llm::Http(reqwest::Client::new()))
        // Closing the captions window quits; the optional windows go with it.
        .on_window_event(|window, event| {
            if window.label() == "main" && matches!(event, tauri::WindowEvent::Destroyed) {
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
            start_demo
        ])
        .setup(|app| {
            // EZTRANS_DEMO=1 starts the demo on launch (screenshots and smoke checks).
            if std::env::var_os("EZTRANS_DEMO").is_some_and(|v| v == "1") {
                demo::start(app.handle().clone());
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running eztrans");
}

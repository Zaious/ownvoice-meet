// The desktop shell: the session state, the commands the three windows call, and window lifetime.
// Audio capture, speech-to-text and the language model plug in behind these commands.

mod demo;
mod session;

use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_window_state::StateFlags;

use session::{Reply, Session};

type Shared<'a> = State<'a, Mutex<Session>>;

const SCRIPT_MAX_BYTES: usize = 512 * 1024;

fn lock<'a>(state: &'a Shared<'_>) -> Result<std::sync::MutexGuard<'a, Session>, String> {
    state.lock().map_err(|_| "session state is poisoned".to_string())
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
    // Placeholder until the language-model provider is connected.
    let reply = Reply {
        id: s.reply_seq,
        zh,
        en: "(No language model connected yet. Add a provider in settings.)".into(),
        done: true,
    };
    s.reply = Some(reply.clone());
    drop(s);
    app.emit("reply", &reply).map_err(|e| e.to_string())?;
    Ok(reply.id)
}

/// Start or stop capturing system audio.
#[tauri::command]
fn set_listening(app: AppHandle, state: Shared<'_>, on: bool) -> Result<bool, String> {
    let mut s = lock(&state)?;
    // Placeholder until audio capture and speech-to-text are connected.
    s.listening = false;
    let msg = if on { "Audio capture is not connected yet. Try Demo." } else { "Stopped" };
    s.set_status(&app, msg);
    let _ = app.emit("listening", false);
    Ok(false)
}

/// Play the canned demo exchange through the normal caption path.
#[tauri::command]
fn start_demo(app: AppHandle) {
    demo::start(app);
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_window_state::Builder::default().with_state_flags(StateFlags::POSITION | StateFlags::SIZE).build())
        // The reply window's focus key works while Zoom or Teams has focus; the web view registers it.
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(Mutex::new(Session::default()))
        // Closing the captions window quits; the optional windows go with it.
        .on_window_event(|window, event| {
            if window.label() == "main" && matches!(event, tauri::WindowEvent::Destroyed) {
                window.app_handle().exit(0);
            }
        })
        .invoke_handler(tauri::generate_handler![snapshot, set_script, compose_reply, set_listening, start_demo])
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

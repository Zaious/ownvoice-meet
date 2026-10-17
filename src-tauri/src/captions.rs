// Caption bookkeeping shared by the speech-to-text providers: open a line, update its live text,
// finish it (which starts the translation) or drop it.

use std::sync::Mutex;

use tauri::{AppHandle, Manager};

use crate::llm;
use crate::session::{Segment, Session};

pub enum Failure {
    /// Retrying will not help (a rejected key, an unknown model): tell the user and stop.
    Fatal(String),
    /// A dropped connection or a timeout: try again.
    Retry(String),
}

pub fn status(app: &AppHandle, msg: impl Into<String>) {
    if let Ok(mut s) = app.state::<Mutex<Session>>().lock() {
        s.set_status(app, msg);
    }
}

pub fn open(app: &AppHandle) -> u64 {
    let state = app.state::<Mutex<Session>>();
    let Ok(mut s) = state.lock() else { return 0 };
    let id = s.next_segment_id();
    s.upsert_segment(app, Segment { id, en: "…".into(), zh: String::new(), is_final: false });
    id
}

pub fn interim(app: &AppHandle, id: u64, text: &str) {
    if let Ok(mut s) = app.state::<Mutex<Session>>().lock() {
        s.upsert_segment(app, Segment { id, en: text.to_string(), zh: String::new(), is_final: false });
    }
}

/// Final text for a line; empty text drops the line. Non-empty text is translated.
pub fn finish(app: &AppHandle, id: u64, text: &str) {
    let text = text.trim();
    {
        let state = app.state::<Mutex<Session>>();
        let Ok(mut s) = state.lock() else { return };
        if text.is_empty() {
            s.remove_segment(app, id);
            return;
        }
        s.upsert_segment(app, Segment { id, en: text.to_string(), zh: String::new(), is_final: true });
    }
    llm::spawn_translate(app.clone(), id, text.to_string());
}

pub fn drop_line(app: &AppHandle, id: u64) {
    if let Ok(mut s) = app.state::<Mutex<Session>>().lock() {
        s.remove_segment(app, id);
    }
}

/// Show an error in place of a line's text, without translating it.
pub fn fail(app: &AppHandle, id: u64, msg: &str) {
    if let Ok(mut s) = app.state::<Mutex<Session>>().lock() {
        s.upsert_segment(app, Segment { id, en: format!("〔{msg}〕"), zh: String::new(), is_final: true });
    }
}

/// "en-US" -> "en": the ISO 639-1 code most transcription APIs take. Empty stays empty (auto).
pub fn iso639(bcp47: &str) -> String {
    bcp47.split(['-', '_']).next().unwrap_or("").trim().to_lowercase()
}

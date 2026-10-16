// The one copy of the meeting state. Every window renders from it: a window that opens late asks
// for a snapshot, then follows the events. Audio, speech-to-text and the language model will all
// write here from the Rust side, so no window owns the state.

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// Captions kept in memory; older ones scroll away.
const MAX_SEGMENTS: usize = 500;

#[derive(Clone, Serialize, Default)]
pub struct Segment {
    pub id: u64,
    pub en: String,
    pub zh: String,
    #[serde(rename = "final")]
    pub is_final: bool,
}

#[derive(Clone, Serialize, Default)]
pub struct Reply {
    pub id: u64,
    pub zh: String,
    pub en: String,
    pub done: bool,
}

#[derive(Clone, Serialize, Default)]
pub struct Session {
    pub segments: Vec<Segment>,
    pub reply: Option<Reply>,
    pub script: String,
    pub listening: bool,
    pub status: String,
    #[serde(skip)]
    pub reply_seq: u64,
}

impl Session {
    /// Insert or update a caption, then tell every window.
    pub fn upsert_segment(&mut self, app: &AppHandle, seg: Segment) {
        match self.segments.iter_mut().rev().find(|s| s.id == seg.id) {
            Some(s) => *s = seg.clone(),
            None => {
                self.segments.push(seg.clone());
                if self.segments.len() > MAX_SEGMENTS {
                    self.segments.remove(0);
                }
            }
        }
        let _ = app.emit("caption", seg);
    }

    pub fn set_status(&mut self, app: &AppHandle, status: impl Into<String>) {
        self.status = status.into();
        let _ = app.emit("status", self.status.clone());
    }
}

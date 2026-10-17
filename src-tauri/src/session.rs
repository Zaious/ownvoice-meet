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
    #[serde(skip)]
    pub seg_seq: u64,
    /// English the user read aloud, placed after the caption that was last when they sent it.
    #[serde(skip)]
    pub my_lines: Vec<(u64, String)>,
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

    /// Insert or update a caption's English, keeping the Chinese it has (an early translation stays
    /// under a line that is still growing).
    pub fn set_en(&mut self, app: &AppHandle, id: u64, en: &str, is_final: bool) {
        let zh = self.segments.iter().rev().find(|s| s.id == id).map(|s| s.zh.clone()).unwrap_or_default();
        self.upsert_segment(app, Segment { id, en: en.to_string(), zh, is_final });
    }

    /// Drop a caption that came to nothing (a hypothesis with no final text).
    pub fn remove_segment(&mut self, app: &AppHandle, id: u64) {
        self.segments.retain(|s| s.id != id);
        let _ = app.emit("caption-remove", id);
    }

    pub fn next_segment_id(&mut self) -> u64 {
        self.seg_seq = self.seg_seq.max(self.segments.last().map_or(0, |s| s.id)) + 1;
        self.seg_seq
    }

    /// Replace a caption's Chinese (the translation streams in), keeping its English.
    pub fn set_zh(&mut self, app: &AppHandle, id: u64, zh: &str) {
        if let Some(s) = self.segments.iter_mut().rev().find(|s| s.id == id) {
            s.zh = zh.to_string();
            let _ = app.emit("caption", s.clone());
        }
    }

    /// The last `n` lines of the meeting before caption `before` (all of it when None), in order:
    /// what others said and what the user read aloud.
    pub fn context(&self, n: usize, before: Option<u64>) -> Vec<String> {
        let keep = |id: u64| before.is_none_or(|b| id < b);
        let mut lines: Vec<(u64, u8, String)> = self
            .segments
            .iter()
            .filter(|s| s.is_final && !s.en.is_empty() && keep(s.id))
            .map(|s| (s.id, 0, format!("Them: {}", s.en)))
            .collect();
        // A reply sent after caption k comes before caption k+1, so it counts when k < before.
        lines.extend(self.my_lines.iter().filter(|(id, _)| keep(*id)).map(|(id, en)| (*id, 1, format!("Me: {en}"))));
        lines.sort_by_key(|(id, order, _)| (*id, *order));
        let skip = lines.len().saturating_sub(n);
        lines.into_iter().skip(skip).map(|(_, _, t)| t).collect()
    }

    pub fn set_status(&mut self, app: &AppHandle, status: impl Into<String>) {
        self.status = status.into();
        let _ = app.emit("status", self.status.clone());
    }
}

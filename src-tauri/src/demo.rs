// Demo mode: plays a canned exchange through the same session path real captions will take, so the
// windows can be built and checked without audio, keys or a network. Also handy for screenshots.

use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Manager};

use crate::session::{Segment, Session};

const LINES: &[(&str, &str)] = &[
    ("Thanks everyone for joining. Let's start with the project update.", "謝謝大家參加。我們先從專案進度開始。"),
    ("Could you share your screen and walk us through the timeline?", "可以請你分享畫面，帶我們看一下時程嗎？"),
    ("We have about ten minutes for questions after the demo.", "示範之後大約有十分鐘可以提問。"),
    ("How is this different from the tools people already use?", "這跟大家已經在用的工具有什麼不同？"),
];

pub fn start(app: AppHandle) {
    thread::spawn(move || {
        let state = app.state::<Mutex<Session>>();
        let base = state.lock().map(|s| s.segments.last().map_or(0, |x| x.id)).unwrap_or(0);
        for (i, (en, zh)) in LINES.iter().enumerate() {
            let id = base + i as u64 + 1;
            // Interim words first (gray), then the final line, then the translation streaming in.
            let words: Vec<&str> = en.split(' ').collect();
            for n in (3..words.len()).step_by(3) {
                push(&app, Segment { id, en: words[..n].join(" "), ..Default::default() });
                thread::sleep(Duration::from_millis(350));
            }
            push(&app, Segment { id, en: en.to_string(), is_final: true, ..Default::default() });
            let chars: Vec<char> = zh.chars().collect();
            for n in (4..=chars.len()).step_by(4).chain([chars.len()]) {
                let zh: String = chars[..n].iter().collect();
                push(&app, Segment { id, en: en.to_string(), zh, is_final: true });
                thread::sleep(Duration::from_millis(60));
            }
            thread::sleep(Duration::from_millis(900));
        }
    });
}

fn push(app: &AppHandle, seg: Segment) {
    if let Ok(mut s) = app.state::<Mutex<Session>>().lock() {
        s.upsert_segment(app, seg);
    }
}

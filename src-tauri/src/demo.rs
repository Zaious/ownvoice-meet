// Demo mode: plays a canned exchange through the same session path real captions will take, so the
// windows can be built and checked without audio, keys or a network. It also fills the other two
// windows (the example script, unless one is open, and a sample reply), so a new user sees what each
// window is for before adding a key. Also handy for screenshots.

use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use crate::session::{Reply, Segment, Session};

const SCRIPT: &str = include_str!("../../examples/scripts/demo.md");
const REPLY_ZH: &str = "字幕工具幫你聽懂，這個工具幫你開口。我打中文，它給我可以照著唸的英文。";
const REPLY_EN: &str = "Caption tools help you understand. This one helps you speak. I type in Chinese, and it gives me English I can read aloud.";

const LINES: &[(&str, &str)] = &[
    ("Welcome back. Our next demo is OwnVoice Meet, from Zaious in Taiwan.", "歡迎回來。下一個示範是來自台灣的 Zaious 帶來的 OwnVoice Meet。"),
    ("Could you share your screen and show us the reply window?", "可以請你分享畫面，讓我們看看回覆視窗嗎？"),
    ("We have about five minutes for questions after the demo.", "示範之後大約有五分鐘可以提問。"),
    ("How is this different from the caption tools people already use?", "這跟大家已經在用的字幕工具有什麼不同？"),
];

pub fn start(app: AppHandle) {
    fill_other_windows(&app);
    // Starting again, or clearing the captions, stops this run (Session::demo_run).
    let (base, run) = {
        let state = app.state::<Mutex<Session>>();
        let Ok(mut s) = state.lock() else { return };
        s.demo_run += 1;
        (s.segments.last().map_or(0, |x| x.id), s.demo_run)
    };
    thread::spawn(move || {
        for (i, (en, zh)) in LINES.iter().enumerate() {
            let id = base + i as u64 + 1;
            // Interim words first (gray), then the final line, then the translation streaming in.
            let words: Vec<&str> = en.split(' ').collect();
            for n in (3..words.len()).step_by(3) {
                if !push(&app, run, Segment { id, en: words[..n].join(" "), ..Default::default() }) {
                    return;
                }
                thread::sleep(Duration::from_millis(350));
            }
            if !push(&app, run, Segment { id, en: en.to_string(), is_final: true, ..Default::default() }) {
                return;
            }
            let chars: Vec<char> = zh.chars().collect();
            for n in (4..=chars.len()).step_by(4).chain([chars.len()]) {
                let zh: String = chars[..n].iter().collect();
                if !push(&app, run, Segment { id, en: en.to_string(), zh, is_final: true }) {
                    return;
                }
                thread::sleep(Duration::from_millis(60));
            }
            thread::sleep(Duration::from_millis(900));
        }
    });
}

/// The example script (only when no script is open) and a sample reply.
fn fill_other_windows(app: &AppHandle) {
    let state = app.state::<Mutex<Session>>();
    let Ok(mut s) = state.lock() else { return };
    if s.script.trim().is_empty() {
        s.script = SCRIPT.to_string();
        let _ = app.emit("script", SCRIPT);
    }
    s.reply_seq += 1;
    let reply = Reply { id: s.reply_seq, zh: REPLY_ZH.into(), en: REPLY_EN.into(), done: true };
    s.reply = Some(reply.clone());
    let _ = app.emit("reply", reply);
}

/// Show one step of the demo, unless it has been stopped; false means stop.
fn push(app: &AppHandle, run: u64, seg: Segment) -> bool {
    let state = app.state::<Mutex<Session>>();
    let Ok(mut s) = state.lock() else { return false };
    if s.demo_run != run {
        return false;
    }
    s.upsert_segment(app, seg);
    true
}

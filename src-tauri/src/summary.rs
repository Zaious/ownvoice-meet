// The live summary: a short list of the main points so far, in Traditional Chinese, brought up to date
// every few minutes from the latest batch of finished lines (batch.rs) and the summary before it, by
// the translation model chosen in Settings. Shown in the 摘要 window and, when saving is on, written
// to summary.md in the session folder. It sums up what others said; it never answers for the user.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use chrono::{DateTime, Local};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::batch::{self, FinishedLine};
use crate::llm::{self, Http};
use crate::session::Session;
use crate::usage::{self, Tally};
use crate::{archive, prompts, settings, zh};

#[derive(Default)]
pub struct Summary {
    state: Mutex<View>,
    busy: AtomicBool,
}

#[derive(Clone, Default, Serialize)]
pub struct View {
    pub text: String,
    /// "14:05" when it was last brought up to date.
    pub updated: String,
    /// Lines it has taken in so far.
    pub lines: usize,
    pub working: bool,
    pub error: String,
}

pub fn current(app: &AppHandle) -> View {
    let s = app.state::<Summary>();
    let mut v = s.state.lock().map(|v| v.clone()).unwrap_or_default();
    v.working = s.busy.load(Ordering::SeqCst);
    v
}

/// The captions were cleared: start over.
pub fn clear(app: &AppHandle) {
    if let Ok(mut v) = app.state::<Summary>().state.lock() {
        *v = View::default();
    }
    let _ = app.emit("summary", current(app));
}

/// The batch hook (registered at startup).
pub fn on_batch(app: &AppHandle, lines: Vec<FinishedLine>) {
    let s = app.state::<Summary>();
    if s.busy.swap(true, Ordering::SeqCst) {
        // Still writing the last one: these lines go into the next.
        batch::requeue(app, lines);
        return;
    }
    let _ = app.emit("summary", current(app));
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = update(&app, &lines).await;
        let s = app.state::<Summary>();
        if let Ok(mut v) = s.state.lock() {
            match result {
                Ok(text) => {
                    v.text = text;
                    v.updated = Local::now().format("%H:%M").to_string();
                    v.lines += lines.len();
                    v.error.clear();
                }
                Err(e) => v.error = e,
            }
        }
        s.busy.store(false, Ordering::SeqCst);
        let view = current(&app);
        if view.error.is_empty() {
            archive::set_summary(&app, &view.text, &view.updated);
        } else {
            batch::requeue(&app, lines);
        }
        let _ = app.emit("summary", view);
    });
}

async fn update(app: &AppHandle, lines: &[FinishedLine]) -> Result<String, String> {
    let s = settings::load(app);
    let t = llm::target(&s)?;
    let script = app.state::<Mutex<Session>>().lock().map(|sess| sess.script.clone()).unwrap_or_default();
    let before = app.state::<Summary>().state.lock().map(|v| v.text.clone()).unwrap_or_default();
    let system = prompts::system_blocks(prompts::SUMMARY_SYSTEM, &prompts::parse_glossary(&s.glossary), &script);
    let client = app.state::<Http>().0.clone();
    let mut out = String::new();
    let tally = llm::stream(&client, &t, system, prompts::summary_user(&before, &transcript(lines)), |d| out.push_str(d))
        .await
        .map_err(|e| format!("摘要失敗：{e}"))?;
    usage::add_tokens(app, &t.catalog_id, Tally { ..tally });
    let text = zh::to_taiwan(out.trim());
    if text.is_empty() {
        return Err("摘要失敗：模型沒有回傳內容".into());
    }
    Ok(text)
}

/// The batch as the model reads it: time, speaker when known, English, then the Chinese.
pub fn transcript(lines: &[FinishedLine]) -> String {
    lines
        .iter()
        .filter(|l| !l.en.trim().is_empty())
        .map(|l| {
            let who = l.speaker.as_deref().map(|s| format!("{s}: ")).unwrap_or_default();
            let zh = if l.zh.trim().is_empty() { String::new() } else { format!("\n  （{}）", l.zh.trim()) };
            format!("[{}] {who}{}{zh}", fmt_time(&l.at), l.en.trim())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn fmt_time(t: &DateTime<Local>) -> String {
    t.format("%H:%M").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: u64, en: &str, zh: &str, speaker: Option<&str>) -> FinishedLine {
        let at = Local::now();
        FinishedLine { id, at, en: en.into(), zh: zh.into(), speaker: speaker.map(str::to_string) }
    }

    #[test]
    fn the_batch_reads_as_a_timed_transcript() {
        let t = transcript(&[
            line(1, "We moved the launch to March.", "我們把上線改到三月。", None),
            line(2, "  ", "", None),
            line(3, "Any questions?", "", Some("講者 2")),
        ]);
        let rows: Vec<&str> = t.lines().collect();
        assert_eq!(rows.len(), 3, "an empty line is left out; a translation adds its own row");
        assert!(rows[0].ends_with("] We moved the launch to March."));
        assert_eq!(rows[1], "  （我們把上線改到三月。）");
        assert!(rows[2].ends_with("] 講者 2: Any questions?"), "the speaker is named when known");
    }
}

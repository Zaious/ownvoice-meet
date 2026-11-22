// Finished caption lines, handed over a batch at a time: every few minutes (Settings: summary_every_min)
// the lines finished since the last batch go to every registered hook. The live summary (summary.rs)
// is one; anything else that wants the meeting a few minutes at a time registers the same way at
// startup. A hook gets its own copy and runs on its own; it never holds up the captions.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local};
use tauri::{AppHandle, Manager};

use crate::settings;

/// Fewer new sentences than this are not worth a batch (a quiet room, a short pause).
const MIN_SENTENCES: usize = 3;
const TICK: Duration = Duration::from_secs(15);

/// One finished caption line.
#[derive(Clone, Debug, PartialEq)]
pub struct FinishedLine {
    pub id: u64,
    pub at: DateTime<Local>,
    pub en: String,
    /// The final translation; empty while it is still on its way.
    pub zh: String,
    /// Who said it, when the speech service can tell (None until then).
    pub speaker: Option<String>,
}

pub type Hook = fn(&AppHandle, Vec<FinishedLine>);

pub struct Batches {
    pending: Mutex<Vec<FinishedLine>>,
    last: Mutex<Instant>,
    hooks: Mutex<Vec<Hook>>,
}

impl Default for Batches {
    fn default() -> Self {
        Batches { pending: Mutex::new(Vec::new()), last: Mutex::new(Instant::now()), hooks: Mutex::new(Vec::new()) }
    }
}

pub fn register(app: &AppHandle, hook: Hook) {
    if let Ok(mut h) = app.state::<Batches>().hooks.lock() {
        h.push(hook);
    }
}

/// A caption line's final English.
pub fn line_finished(app: &AppHandle, id: u64, en: &str) {
    if let Ok(mut p) = app.state::<Batches>().pending.lock() {
        match p.iter_mut().find(|l| l.id == id) {
            Some(l) => l.en = en.to_string(),
            None => p.push(FinishedLine { id, at: Local::now(), en: en.to_string(), zh: String::new(), speaker: None }),
        }
    }
}

/// A caption line's final translation.
pub fn line_translated(app: &AppHandle, id: u64, zh: &str) {
    if let Ok(mut p) = app.state::<Batches>().pending.lock() {
        if let Some(l) = p.iter_mut().find(|l| l.id == id) {
            l.zh = zh.to_string();
        }
    }
}

/// Lines a hook could not take this time (it was busy): back to the front of the next batch.
pub fn requeue(app: &AppHandle, mut lines: Vec<FinishedLine>) {
    if let Ok(mut p) = app.state::<Batches>().pending.lock() {
        lines.append(&mut p);
        *p = lines;
    }
}

/// The captions were cleared: so are the lines waiting for a batch.
pub fn clear(app: &AppHandle) {
    let b = app.state::<Batches>();
    if let Ok(mut p) = b.pending.lock() {
        p.clear();
    }
    if let Ok(mut l) = b.last.lock() {
        *l = Instant::now();
    };
}

/// Hand over what is waiting now, if anything (the summary window's button, the end of listening).
pub fn flush_now(app: &AppHandle) -> bool {
    flush(app, 1)
}

fn flush(app: &AppHandle, min_sentences: usize) -> bool {
    let b = app.state::<Batches>();
    let lines = {
        let Ok(mut p) = b.pending.lock() else { return false };
        if p.is_empty() || sentences(&p) < min_sentences {
            return false;
        }
        std::mem::take(&mut *p)
    };
    if let Ok(mut l) = b.last.lock() {
        *l = Instant::now();
    }
    let hooks = b.hooks.lock().map(|h| h.clone()).unwrap_or_default();
    for hook in hooks {
        hook(app, lines.clone());
    }
    true
}

/// Is a batch due: enough new lines, and the interval since the last one has passed?
fn due(pending: usize, since_last: Duration, every: Option<Duration>) -> bool {
    every.is_some_and(|e| since_last >= e) && pending >= MIN_SENTENCES
}

/// How many sentences the lines hold. A speaker who hardly pauses can come back as one long line
/// (Gemini through a Mac's microphone joined four sentences), so lines alone undercount.
fn sentences(lines: &[FinishedLine]) -> usize {
    lines
        .iter()
        .map(|l| l.en.split(['.', '?', '!']).filter(|s| s.split_whitespace().count() >= 2).count().max(1))
        .sum()
}

/// Checks every few seconds whether a batch is due.
pub fn start(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(TICK);
        let every = crate::idle::minutes(settings::load(&app).summary_every_min);
        let b = app.state::<Batches>();
        let pending = b.pending.lock().map(|p| sentences(&p)).unwrap_or(0);
        let since = b.last.lock().map(|l| l.elapsed()).unwrap_or_default();
        if due(pending, since, every) {
            flush(&app, MIN_SENTENCES);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: Duration = Duration::from_secs(60);

    #[test]
    fn a_batch_needs_the_interval_and_a_few_lines() {
        assert!(!due(10, 4 * M, Some(5 * M)), "not before the interval");
        assert!(due(10, 5 * M, Some(5 * M)));
        assert!(!due(2, 30 * M, Some(5 * M)), "a quiet room does not make a batch");
        assert!(!due(100, 300 * M, None), "0 minutes = no batches");
    }

    #[test]
    fn sentences_are_counted_inside_long_lines() {
        let line = |en: &str| FinishedLine { id: 0, at: Local::now(), en: en.into(), zh: String::new(), speaker: None };
        // As it came from Gemini through a Mac's microphone (2026-11-22): four sentences, one line.
        let joined = line("Welcome to the panel on digital libraries. I am the moderator.Our first panelist will talk about preserving web archives for 20 years.The library decided to move the whole collection to open formats by March next year.One open question is who pays for the storage after the grant ends.");
        assert_eq!(sentences(&[joined]), 5);
        assert_eq!(sentences(&[line("20 years"), line("Thanks.")]), 2, "a short line still counts once");
        assert_eq!(sentences(&[line("Moved to the U.S. last year.")]), 2);
    }
}

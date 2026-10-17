// Caption bookkeeping shared by the speech-to-text providers: open a line, update its live text,
// finish it (which starts the translation) or drop it.
//
// Early translation: a sentence that keeps growing past EARLY_AFTER without ending is translated as
// it stands, at most every EARLY_EVERY, so long unbroken speech shows Chinese before the speaker
// pauses. Each translation of a line is a numbered request. As in LiveCaptions-Translator's
// TranslationTaskQueue (SakiRinn, Apache-2.0, see THIRD_PARTY_NOTICES.md), a request that completes
// cancels the older ones still running, and an older request never overwrites a newer one. The final
// translation, started when the sentence ends, cancels the early ones and takes over the line once
// its text has caught up with theirs.
// OWNVOICE_DEBUG=1 appends each translation, and each sentence end, to captions-debug.log in the
// config folder, with times.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use crate::llm;
use crate::session::{Segment, Session};
use crate::settings;

/// A sentence still growing after this long is translated before it ends.
pub const EARLY_AFTER: Duration = Duration::from_secs(4);
/// At most one early translation of a line this often.
pub const EARLY_EVERY: Duration = Duration::from_secs(2);
/// Shorter interim text is not worth an early translation.
const EARLY_MIN_CHARS: usize = 20;

pub enum Failure {
    /// Retrying will not help (a rejected key, an unknown model): tell the user and stop.
    Fatal(String),
    /// A dropped connection or a timeout: try again.
    Retry(String),
}

/// Lines still being spoken or translated.
#[derive(Default)]
pub struct Lines(Mutex<HashMap<u64, Line>>);

struct Line {
    opened: Instant,
    early_at: Option<Instant>,
    early_text: String,
    /// The last request number given out.
    seq: u64,
    /// The request whose Chinese is on screen, and its length in characters.
    shown_seq: u64,
    shown_len: usize,
    running: Vec<(u64, Arc<AtomicBool>)>,
    finished: bool,
}

impl Line {
    fn new(now: Instant) -> Self {
        Line { opened: now, early_at: None, early_text: String::new(), seq: 0, shown_seq: 0, shown_len: 0, running: Vec::new(), finished: false }
    }

    fn wants_early(&self, text: &str, now: Instant) -> bool {
        !self.finished
            && now.duration_since(self.opened) >= EARLY_AFTER
            && self.early_at.is_none_or(|t| now.duration_since(t) >= EARLY_EVERY)
            && text != self.early_text
            && text.chars().count() >= EARLY_MIN_CHARS
    }

    /// Number a new request. The final one cancels every early one.
    fn start(&mut self, text: &str, early: bool, now: Instant) -> (u64, Arc<AtomicBool>) {
        if early {
            self.early_at = Some(now);
            self.early_text = text.to_string();
        } else {
            self.finished = true;
            self.cancel_below(u64::MAX);
        }
        self.seq += 1;
        let stop = Arc::new(AtomicBool::new(false));
        self.running.push((self.seq, stop.clone()));
        (self.seq, stop)
    }

    fn cancel_below(&mut self, seq: u64) {
        self.running.retain(|(s, stop)| {
            if *s < seq {
                stop.store(true, Ordering::SeqCst);
            }
            *s >= seq
        });
    }

    /// Whether request `seq`'s Chinese (`len` characters, complete or still streaming) goes on
    /// screen. A request still streaming waits until it is as long as the text it would replace.
    fn accept(&mut self, seq: u64, len: usize, complete: bool) -> bool {
        if seq < self.shown_seq || (!complete && seq != self.shown_seq && len < self.shown_len) {
            return false;
        }
        self.shown_seq = seq;
        self.shown_len = len;
        if complete {
            self.cancel_below(seq);
        }
        true
    }

    /// Request `seq` ended; true when nothing more will happen to the line.
    fn done(&mut self, seq: u64) -> bool {
        self.running.retain(|(s, _)| *s != seq);
        self.finished && self.running.is_empty()
    }
}

pub fn status(app: &AppHandle, msg: impl Into<String>) {
    if let Ok(mut s) = app.state::<Mutex<Session>>().lock() {
        s.set_status(app, msg);
    }
}

pub fn open(app: &AppHandle) -> u64 {
    let id = {
        let state = app.state::<Mutex<Session>>();
        let Ok(mut s) = state.lock() else { return 0 };
        let id = s.next_segment_id();
        s.upsert_segment(app, Segment { id, en: "…".into(), zh: String::new(), is_final: false });
        id
    };
    if let Ok(mut lines) = app.state::<Lines>().0.lock() {
        lines.insert(id, Line::new(Instant::now()));
    }
    id
}

/// The sentence so far (gray). Translated early when it keeps growing.
pub fn interim(app: &AppHandle, id: u64, text: &str) {
    if let Ok(mut s) = app.state::<Mutex<Session>>().lock() {
        s.set_en(app, id, text, false);
    }
    let now = Instant::now();
    let req = {
        let state = app.state::<Lines>();
        let Ok(mut lines) = state.0.lock() else { return };
        let line = lines.entry(id).or_insert_with(|| Line::new(now));
        line.wants_early(text, now).then(|| line.start(text, true, now))
    };
    if let Some((seq, stop)) = req {
        spawn_translate(app.clone(), id, text.to_string(), seq, stop, true);
    }
}

/// Final text for a line; empty text drops the line. Non-empty text is translated.
pub fn finish(app: &AppHandle, id: u64, text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return drop_line(app, id);
    }
    if let Ok(mut s) = app.state::<Mutex<Session>>().lock() {
        s.set_en(app, id, text, true);
    }
    debug_log(app, &format!("line {id} ended: {text}"));
    let now = Instant::now();
    let req = {
        let state = app.state::<Lines>();
        let Ok(mut lines) = state.0.lock() else { return };
        lines.entry(id).or_insert_with(|| Line::new(now)).start(text, false, now)
    };
    spawn_translate(app.clone(), id, text.to_string(), req.0, req.1, false);
}

pub fn drop_line(app: &AppHandle, id: u64) {
    if let Ok(mut s) = app.state::<Mutex<Session>>().lock() {
        s.remove_segment(app, id);
    }
    forget(app, id);
}

/// Show an error in place of a line's text, without translating it.
pub fn fail(app: &AppHandle, id: u64, msg: &str) {
    if let Ok(mut s) = app.state::<Mutex<Session>>().lock() {
        s.upsert_segment(app, Segment { id, en: format!("〔{msg}〕"), zh: String::new(), is_final: true });
    }
    forget(app, id);
}

fn forget(app: &AppHandle, id: u64) {
    if let Some(mut line) = app.state::<Lines>().0.lock().ok().and_then(|mut l| l.remove(&id)) {
        line.cancel_below(u64::MAX);
    }
}

fn spawn_translate(app: AppHandle, id: u64, en: String, seq: u64, stop: Arc<AtomicBool>, early: bool) {
    tauri::async_runtime::spawn(async move {
        let res = llm::translate(&app, id, &en, early, &stop, |zh| {
            show(&app, id, seq, &stop, zh, early, false);
        })
        .await;
        let (zh, shown) = match res {
            Ok(zh) => {
                let shown = show(&app, id, seq, &stop, &zh, early, true);
                (zh, shown)
            }
            Err(e) if !early => {
                let zh = format!("〔{e}〕");
                let shown = show(&app, id, seq, &stop, &zh, false, true);
                (zh, shown)
            }
            Err(e) => (e, false), // the final translation will come
        };
        let kind = if early { "early" } else { "final" };
        let outcome = if shown { "shown" } else if stop.load(Ordering::SeqCst) { "cancelled" } else { "not shown" };
        debug_log(&app, &format!("line {id} {kind} #{seq} {outcome}: {en} => {zh}"));
        if let Ok(mut lines) = app.state::<Lines>().0.lock() {
            if lines.get_mut(&id).is_some_and(|l| l.done(seq)) {
                lines.remove(&id);
            }
        }
    });
}

/// Put request `seq`'s Chinese under the line if it should be there; true if it was.
fn show(app: &AppHandle, id: u64, seq: u64, stop: &AtomicBool, zh: &str, early: bool, complete: bool) -> bool {
    if stop.load(Ordering::SeqCst) || (early && !reads_as_subtitle(zh)) {
        return false;
    }
    let state = app.state::<Lines>();
    let Ok(mut lines) = state.0.lock() else { return false };
    if !lines.get_mut(&id).is_some_and(|l| l.accept(seq, zh.chars().count(), complete)) {
        return false;
    }
    if let Ok(mut s) = app.state::<Mutex<Session>>().lock() {
        s.set_zh(app, id, zh);
    }
    true
}

/// An early translation is a guess at half a sentence; one that came back as notes or English
/// (more English words than Chinese characters, or several lines) is not shown, and the final one
/// will come.
fn reads_as_subtitle(zh: &str) -> bool {
    let han = zh.chars().filter(|c| ('\u{4e00}'..='\u{9fff}').contains(c)).count();
    let words = zh.split(|c: char| !c.is_ascii_alphabetic()).filter(|w| !w.is_empty()).count();
    !zh.trim().contains('\n') && han > 0 && han >= words
}

fn debug_log(app: &AppHandle, line: &str) {
    use std::io::Write as _;
    if std::env::var_os("OWNVOICE_DEBUG").is_none_or(|v| v != "1") {
        return;
    }
    let Ok(dir) = settings::config_dir(app) else { return };
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("captions-debug.log")) {
        let _ = writeln!(f, "{} {line}", chrono::Local::now().format("%H:%M:%S%.3f"));
    }
}

/// "en-US" -> "en": the ISO 639-1 code most transcription APIs take. Empty stays empty (auto).
pub fn iso639(bcp47: &str) -> String {
    bcp47.split(['-', '_']).next().unwrap_or("").trim().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    const LONG: &str = "so the plan for the next quarter is to move";

    #[test]
    fn early_translation_waits_then_throttles() {
        let t0 = Instant::now();
        let mut l = Line::new(t0);
        assert!(!l.wants_early(LONG, t0 + Duration::from_secs(3)), "not before 4 s");
        assert!(!l.wants_early("so the plan", t0 + EARLY_AFTER), "too short to be worth it");
        assert!(l.wants_early(LONG, t0 + EARLY_AFTER));
        l.start(LONG, true, t0 + EARLY_AFTER);
        let longer = "so the plan for the next quarter is to move the whole deployment";
        assert!(!l.wants_early(longer, t0 + EARLY_AFTER + Duration::from_secs(1)), "at most every 2 s");
        assert!(!l.wants_early(LONG, t0 + EARLY_AFTER + EARLY_EVERY), "same text again");
        assert!(l.wants_early(longer, t0 + EARLY_AFTER + EARLY_EVERY));
        l.start(longer, false, t0 + EARLY_AFTER + EARLY_EVERY);
        assert!(!l.wants_early(&format!("{longer} now"), t0 + Duration::from_secs(60)), "never after the sentence ended");
    }

    #[test]
    fn a_finished_request_cancels_older_ones_and_older_never_overwrite_newer() {
        let t0 = Instant::now();
        let mut l = Line::new(t0);
        let (a, stop_a) = l.start("first part of it", true, t0);
        let (b, stop_b) = l.start("first part of it and more", true, t0);
        // b finishes first: a is cancelled and can no longer show.
        assert!(l.accept(b, 12, true));
        assert!(stop_a.load(Ordering::SeqCst));
        assert!(!stop_b.load(Ordering::SeqCst));
        assert!(!l.accept(a, 30, true));
    }

    #[test]
    fn the_final_translation_replaces_the_early_one_once_it_catches_up() {
        let t0 = Instant::now();
        let mut l = Line::new(t0);
        let (e, stop_e) = l.start("an early part of the sentence", true, t0);
        assert!(l.accept(e, 10, false));
        assert!(l.accept(e, 20, true));
        let (f, _) = l.start("an early part of the sentence, finished.", false, t0);
        assert!(stop_e.load(Ordering::SeqCst), "the final request cancels early ones");
        assert!(!l.accept(f, 5, false), "keeps the fuller early text while the final one starts");
        assert!(l.accept(f, 21, false));
        assert!(l.accept(f, 22, false));
        assert!(!l.accept(e, 25, true));
        assert!(l.done(f));
    }

    #[test]
    fn notes_instead_of_a_translation_are_not_shown_early() {
        assert!(reads_as_subtitle("遷移到新叢集，然後在每個團隊都簽核之後"));
        assert!(reads_as_subtitle("把 Kubernetes 叢集升級"));
        assert!(!reads_as_subtitle("to the new cluster and then retire the old build servers once every"));
        assert!(!reads_as_subtitle("遷移到新的叢集\n\nWait, I must output only the translation."));
        assert!(!reads_as_subtitle(""));
    }

    #[test]
    fn a_short_final_translation_still_lands_when_complete() {
        let t0 = Instant::now();
        let mut l = Line::new(t0);
        let (e, _) = l.start("an early part of the sentence", true, t0);
        assert!(l.accept(e, 30, true));
        let (f, _) = l.start("done.", false, t0);
        assert!(!l.accept(f, 3, false));
        assert!(l.accept(f, 3, true));
    }
}

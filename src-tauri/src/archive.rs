// Saved sessions. While listening with saving on, each session gets a folder (under the user's
// Documents folder, "OwnVoice Meet", unless Settings names another) holding:
//   NN-<segment>.wav  the recording, one file per segment (save mode "audio"; recorder.rs)
//   NN-<segment>.md   that segment's transcript
//   transcript.md     the whole session, segment by segment
// A segment is a stretch the user marks: "新段落" (or Ctrl+Shift+F9) starts the next one, named
// from the agenda in Settings when there is one. Transcript lines are the finished caption lines
// with their final translation, timed from the start of the session; the files are rewritten a few
// seconds after a change and when listening stops.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::recorder::Recorder;
use crate::settings::{self, Settings};

const FLUSH_EVERY: Duration = Duration::from_secs(3);
pub const SEGMENT_SHORTCUT: &str = "CommandOrControl+Shift+F9";

#[derive(Default)]
pub struct Archive(Mutex<State>);

#[derive(Default)]
struct State {
    generation: u64,
    active: Option<Active>,
    /// The folder of the session that ended last: a summary that comes back after the end still lands there.
    last_dir: Option<PathBuf>,
}

struct Active {
    dir: PathBuf,
    started: Instant,
    started_at: DateTime<Local>,
    /// "收音：…" and "辨識：…" lines for the file headers.
    about: Vec<String>,
    agenda: Vec<String>,
    segments: Vec<Segment>,
    lines: Vec<Line>,
    by_id: HashMap<u64, usize>,
    /// Lines opened but not finished yet: id -> (start offset, segment).
    open: HashMap<u64, (Duration, usize)>,
    recorder: Option<Recorder>,
    /// The per-segment transcript files written last time, so a renamed segment's old file goes.
    written: HashMap<usize, PathBuf>,
    dirty: bool,
}

#[derive(Clone)]
struct Segment {
    name: String,
    at: Duration,
    wall: DateTime<Local>,
    /// The recording file as it was opened (renamed when the segment closes, if the name changed).
    wav: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq)]
struct Line {
    at: Duration,
    seg: usize,
    en: String,
    zh: String,
    /// Who said it, when the speech service can tell.
    speaker: Option<String>,
}

/// What the captions window shows about the current segment.
#[derive(Clone, Serialize)]
pub struct SegmentInfo {
    /// 1-based.
    pub index: usize,
    pub name: String,
    pub recording: bool,
    pub dir: String,
}

/// Where saved sessions go.
pub fn base_dir(app: &AppHandle, s: &Settings) -> Result<PathBuf, String> {
    if !s.archive_dir.trim().is_empty() {
        return Ok(PathBuf::from(s.archive_dir.trim()));
    }
    if let Some(dir) = std::env::var_os("OWNVOICE_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir).join("sessions"));
    }
    app.path().document_dir().map(|d| d.join("OwnVoice Meet")).map_err(|e| format!("找不到「文件」資料夾：{e}"))
}

/// Start saving a listening session, if Settings asks for it (or `force_audio`, the record-only
/// option). Returns the folder and, when recording, the sender audio.rs feeds.
pub fn begin(
    app: &AppHandle,
    s: &Settings,
    about: Vec<String>,
    rate: u32,
    has_audio: bool,
    force_audio: bool,
) -> Result<Option<(PathBuf, Option<crate::audio::Tap>)>, String> {
    let mode = if force_audio { "audio" } else { s.save_mode.as_str() };
    if mode != "text" && mode != "audio" {
        return Ok(None);
    }
    let now = Local::now();
    let dir = unique_dir(&base_dir(app, s)?, &now.format("%Y-%m-%d %H%M").to_string());
    std::fs::create_dir_all(&dir).map_err(|e| format!("無法建立存檔資料夾 {}：{e}", dir.display()))?;
    let agenda = parse_agenda(&s.agenda);
    let name = segment_name(&agenda, 0);
    let mut seg = Segment { name, at: Duration::ZERO, wall: now, wav: None };
    let mut tap = None;
    let mut recorder = None;
    if mode == "audio" && has_audio {
        let wav = dir.join(format!("{}.wav", stem(1, &seg.name)));
        let (rec, tx) = Recorder::start(wav.clone(), rate)?;
        seg.wav = Some(wav);
        recorder = Some(rec);
        tap = Some(tx);
    }
    let active = Active {
        dir: dir.clone(),
        started: Instant::now(),
        started_at: now,
        about,
        agenda,
        segments: vec![seg],
        lines: Vec::new(),
        by_id: HashMap::new(),
        open: HashMap::new(),
        recorder,
        written: HashMap::new(),
        dirty: true,
    };
    let generation = {
        let state = app.state::<Archive>();
        let mut st = state.0.lock().map_err(|_| "archive lock is poisoned")?;
        st.generation += 1;
        st.active = Some(active);
        st.generation
    };
    spawn_flusher(app.clone(), generation);
    emit_segment(app);
    Ok(Some((dir, tap)))
}

/// Stop saving: close the recording (the capture must have stopped already) and write the
/// transcript one last time. Returns the folder.
pub fn end(app: &AppHandle) -> Option<PathBuf> {
    let active = {
        let state = app.state::<Archive>();
        let mut st = state.0.lock().ok()?;
        st.generation += 1;
        let a = st.active.take();
        if let Some(a) = &a {
            st.last_dir = Some(a.dir.clone());
        }
        a
    };
    let mut a = active?;
    if let Some(rec) = a.recorder.take() {
        rec.stop(a.closing_path(a.segments.len() - 1));
    }
    let _ = a.write();
    let _ = app.emit("segment", None::<SegmentInfo>);
    Some(a.dir)
}

/// Is a recording running (so a failed recognizer should not stop the capture)?
pub fn recording(app: &AppHandle) -> bool {
    app.state::<Archive>().0.lock().ok().is_some_and(|st| st.active.as_ref().is_some_and(|a| a.recorder.is_some()))
}

pub fn current(app: &AppHandle) -> Option<SegmentInfo> {
    let state = app.state::<Archive>();
    let st = state.0.lock().ok()?;
    st.active.as_ref().map(Active::info)
}

/// Start the next segment, named `name` or from the agenda.
pub fn next_segment(app: &AppHandle, name: Option<String>) -> Result<SegmentInfo, String> {
    let info = {
        let state = app.state::<Archive>();
        let mut st = state.0.lock().map_err(|_| "archive lock is poisoned")?;
        let a = st.active.as_mut().ok_or("要先按「聆聽」並在設定開啟存檔，才能分段")?;
        let i = a.segments.len();
        let name = name.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| segment_name(&a.agenda, i));
        let mut seg = Segment { name, at: a.started.elapsed(), wall: Local::now(), wav: None };
        if let Some(rec) = &a.recorder {
            let next = a.dir.join(format!("{}.wav", stem(i + 1, &seg.name)));
            rec.split(a.closing_path(i - 1), next.clone());
            seg.wav = Some(next);
        }
        a.segments.push(seg);
        a.dirty = true;
        let _ = a.write();
        a.info()
    };
    emit_segment(app);
    Ok(info)
}

/// Rename the current segment (its files take the new name; the recording when it closes).
pub fn rename_segment(app: &AppHandle, name: &str) -> Result<SegmentInfo, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("名稱不能是空的".into());
    }
    let info = {
        let state = app.state::<Archive>();
        let mut st = state.0.lock().map_err(|_| "archive lock is poisoned")?;
        let a = st.active.as_mut().ok_or("目前沒有在存檔")?;
        if let Some(seg) = a.segments.last_mut() {
            seg.name = name.to_string();
        }
        a.dirty = true;
        let _ = a.write();
        a.info()
    };
    emit_segment(app);
    Ok(info)
}

/// A caption line started (speech began): remember when, and in which segment.
pub fn open_line(app: &AppHandle, id: u64) {
    with_active(app, |a| {
        let seg = a.segments.len() - 1;
        a.open.insert(id, (a.started.elapsed(), seg));
    });
}

/// A caption line's final English.
pub fn set_en(app: &AppHandle, id: u64, en: &str) {
    with_active(app, |a| {
        let (at, seg) = a.open.get(&id).copied().unwrap_or((a.started.elapsed(), a.segments.len() - 1));
        match a.by_id.get(&id) {
            Some(&i) => a.lines[i].en = en.to_string(),
            None => {
                a.by_id.insert(id, a.lines.len());
                a.lines.push(Line { at, seg, en: en.to_string(), zh: String::new(), speaker: None });
            }
        }
        a.dirty = true;
    });
}

/// A caption line's final translation.
pub fn set_zh(app: &AppHandle, id: u64, zh: &str) {
    with_active(app, |a| {
        if let Some(&i) = a.by_id.get(&id) {
            a.lines[i].zh = zh.to_string();
            a.dirty = true;
        }
    });
}

/// Who said a caption line (speakers.rs).
pub fn set_speaker(app: &AppHandle, id: u64, name: &str) {
    with_active(app, |a| {
        if let Some(&i) = a.by_id.get(&id) {
            a.lines[i].speaker = Some(name.to_string());
            a.dirty = true;
        }
    });
}

/// A speaker was named: every line of theirs in this session follows.
pub fn rename_speaker(app: &AppHandle, old: &str, name: &str) {
    with_active(app, |a| {
        for l in a.lines.iter_mut().filter(|l| l.speaker.as_deref() == Some(old)) {
            l.speaker = Some(name.to_string());
            a.dirty = true;
        }
    });
}

/// The live summary, kept in the session folder as summary.md.
pub fn set_summary(app: &AppHandle, text: &str, updated: &str) {
    let dir = {
        let state = app.state::<Archive>();
        let Ok(st) = state.0.lock() else { return };
        st.active.as_ref().map(|a| a.dir.clone()).or_else(|| st.last_dir.clone())
    };
    let Some(dir) = dir else { return };
    let body = format!("# 摘要\n\n- 更新：{updated}\n\n{}\n", text.trim());
    if let Err(e) = write_atomic(&dir.join("summary.md"), &body) {
        eprintln!("archive: {e}");
    }
}

/// A line that came to nothing.
pub fn drop_line(app: &AppHandle, id: u64) {
    with_active(app, |a| {
        a.open.remove(&id);
    });
}

fn with_active(app: &AppHandle, f: impl FnOnce(&mut Active)) {
    if let Ok(mut st) = app.state::<Archive>().0.lock() {
        if let Some(a) = st.active.as_mut() {
            f(a);
        }
    }
}

fn emit_segment(app: &AppHandle) {
    let _ = app.emit("segment", current(app));
}

/// Writes the transcript every few seconds while it changes, until the session ends.
fn spawn_flusher(app: AppHandle, generation: u64) {
    std::thread::spawn(move || loop {
        std::thread::sleep(FLUSH_EVERY);
        let state = app.state::<Archive>();
        let Ok(mut st) = state.0.lock() else { return };
        if st.generation != generation {
            return;
        }
        if let Some(a) = st.active.as_mut() {
            if a.dirty {
                if let Err(e) = a.write() {
                    eprintln!("archive: {e}");
                }
            }
        }
    });
}

impl Active {
    fn info(&self) -> SegmentInfo {
        let i = self.segments.len();
        SegmentInfo { index: i, name: self.segments[i - 1].name.clone(), recording: self.recorder.is_some(), dir: self.dir.display().to_string() }
    }

    /// The name segment `i` (0-based) closes under: its current name.
    fn closing_path(&self, i: usize) -> PathBuf {
        let seg = &self.segments[i];
        let wanted = self.dir.join(format!("{}.wav", stem(i + 1, &seg.name)));
        // Renaming onto an existing file fails on Windows; keep the opened name in that case.
        match &seg.wav {
            Some(opened) if *opened != wanted && wanted.exists() => opened.clone(),
            _ => wanted,
        }
    }

    fn write(&mut self) -> Result<(), String> {
        let header = render_header(&self.started_at, &self.about);
        let all = render(&header, &self.segments, &self.lines, None);
        write_atomic(&self.dir.join("transcript.md"), &all)?;
        for (i, seg) in self.segments.iter().enumerate() {
            let path = self.dir.join(format!("{}.md", stem(i + 1, &seg.name)));
            if let Some(old) = self.written.get(&i) {
                if *old != path {
                    let _ = std::fs::remove_file(old);
                }
            }
            write_atomic(&path, &render(&header, &self.segments, &self.lines, Some(i)))?;
            self.written.insert(i, path);
        }
        self.dirty = false;
        Ok(())
    }
}

fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, text).map_err(|e| format!("寫入 {} 失敗：{e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("寫入 {} 失敗：{e}", path.display()))
}

/// A folder named for the start time; "-2", "-3"… when two sessions start in the same minute.
fn unique_dir(base: &Path, name: &str) -> PathBuf {
    let first = base.join(name);
    if !first.exists() {
        return first;
    }
    (2..).map(|n| base.join(format!("{name}-{n}"))).find(|p| !p.exists()).unwrap_or(first)
}

pub fn parse_agenda(text: &str) -> Vec<String> {
    text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()
}

/// Segment `i` (0-based) takes the agenda's `i`-th name, or "段落 N".
fn segment_name(agenda: &[String], i: usize) -> String {
    agenda.get(i).cloned().unwrap_or_else(|| format!("段落 {}", i + 1))
}

/// "03-Session 2A Digital Libraries": a file name safe on Windows and macOS.
fn stem(index: usize, name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| if c.is_control() || r#"\/:*?"<>|"#.contains(c) { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let clean: String = clean.chars().take(60).collect();
    let clean = clean.trim_end_matches(['.', ' ']).to_string();
    if clean.is_empty() {
        format!("{index:02}")
    } else {
        format!("{index:02}-{clean}")
    }
}

fn hms(d: Duration) -> String {
    let s = d.as_secs();
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

fn render_header(started_at: &DateTime<Local>, about: &[String]) -> String {
    let mut h = format!("- 開始：{}\n", started_at.format("%Y-%m-%d %H:%M"));
    for line in about {
        h.push_str(&format!("- {line}\n"));
    }
    h
}

/// The transcript as Markdown: every segment, or only segment `only` (0-based).
fn render(header: &str, segments: &[Segment], lines: &[Line], only: Option<usize>) -> String {
    let title = match only {
        Some(i) => format!("# {:02} · {}", i + 1, segments[i].name),
        None => "# 逐字稿".to_string(),
    };
    let mut out = format!("{title}\n\n{header}");
    for (i, seg) in segments.iter().enumerate() {
        if only.is_some_and(|o| o != i) {
            continue;
        }
        out.push_str(&format!("\n## {:02} · {}（{} 開始，{}）\n\n", i + 1, seg.name, seg.wall.format("%H:%M"), hms(seg.at)));
        for l in lines.iter().filter(|l| l.seg == i && !l.en.trim().is_empty()) {
            let who = l.speaker.as_deref().map(|s| format!("{s}：")).unwrap_or_default();
            out.push_str(&format!("[{}] {who}{}\n", hms(l.at), l.en.trim()));
            if !l.zh.trim().is_empty() {
                out.push_str(&format!("　{}\n", l.zh.trim()));
            }
            out.push('\n');
        }
    }
    out
}

/// What the file header says about the session.
pub fn describe(s: &Settings, device: &str, recognizer: &str, translator: &str) -> Vec<String> {
    let source = if s.audio_source == "mic" { format!("收音：麥克風（{device}）") } else { format!("收音：電腦播放的聲音（{device}）") };
    vec![source, format!("辨識：{recognizer}"), format!("翻譯：{translator}")]
}

/// Settings' folder as shown to the user.
pub fn folder_for_display(app: &AppHandle) -> Result<String, String> {
    let s = settings::load(app);
    if let Some(info) = current(app) {
        return Ok(info.dir);
    }
    base_dir(app, &s).map(|d| d.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(name: &str, at: u64) -> Segment {
        Segment { name: name.into(), at: Duration::from_secs(at), wall: Local::now(), wav: None }
    }

    #[test]
    fn segment_names_come_from_the_agenda_then_count_on() {
        let agenda = parse_agenda("Keynote\n\n  Session 3A: Digital Libraries \n");
        assert_eq!(agenda, vec!["Keynote", "Session 3A: Digital Libraries"]);
        assert_eq!(segment_name(&agenda, 0), "Keynote");
        assert_eq!(segment_name(&agenda, 1), "Session 3A: Digital Libraries");
        assert_eq!(segment_name(&agenda, 2), "段落 3");
    }

    #[test]
    fn file_names_are_safe_on_windows() {
        assert_eq!(stem(3, "Session 3A: Digital Libraries"), "03-Session 3A Digital Libraries");
        assert_eq!(stem(1, "a/b\\c*?\"<>|"), "01-a b c");
        assert_eq!(stem(12, "  ...  "), "12");
        assert_eq!(stem(2, "段落 2."), "02-段落 2");
        assert_eq!(stem(1, &"x".repeat(100)).len(), 3 + 60);
    }

    #[test]
    fn transcript_is_grouped_by_segment_with_times() {
        let segments = vec![seg("Keynote", 0), seg("Q&A", 65)];
        let lines = vec![
            Line { at: Duration::from_secs(12), seg: 0, en: "Welcome to the conference.".into(), zh: "歡迎來到研討會。".into(), speaker: None },
            Line { at: Duration::from_secs(30), seg: 0, en: "  ".into(), zh: String::new(), speaker: None },
            Line { at: Duration::from_secs(3_725), seg: 1, en: "Any questions?".into(), zh: String::new(), speaker: Some("講者 2".into()) },
        ];
        let all = render("- 開始：x\n", &segments, &lines, None);
        assert!(all.starts_with("# 逐字稿\n\n- 開始：x\n"));
        assert!(all.contains("## 01 · Keynote（"));
        assert!(all.contains("[00:00:12] Welcome to the conference.\n　歡迎來到研討會。\n"));
        assert!(all.contains("## 02 · Q&A（") && all.contains("00:01:05）"));
        assert!(all.contains("[01:02:05] 講者 2：Any questions?\n\n"), "a line still waiting for its translation is kept, with its speaker");
        assert_eq!(all.matches('[').count(), 2, "an empty line is left out");

        let second = render("- 開始：x\n", &segments, &lines, Some(1));
        assert!(second.starts_with("# 02 · Q&A"));
        assert!(!second.contains("Welcome"), "a segment's own file holds only its lines");
    }

    #[test]
    fn a_second_session_in_the_same_minute_gets_its_own_folder() {
        let base = std::env::temp_dir().join(format!("ownvoice-archive-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let first = unique_dir(&base, "2026-11-21 1003");
        std::fs::create_dir_all(&first).unwrap();
        let second = unique_dir(&base, "2026-11-21 1003");
        assert_eq!(second, base.join("2026-11-21 1003-2"));
        let _ = std::fs::remove_dir_all(&base);
    }
}

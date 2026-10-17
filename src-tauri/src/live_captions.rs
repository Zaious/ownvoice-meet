// Listening through Windows 11's Live Captions (LiveCaptions.exe, 22H2 and later), which recognizes
// the system audio on the device for free. This reads its caption text with UI Automation on a
// thread of its own and turns it into caption lines (sentences.rs); its window is minimized and kept
// off the taskbar while listening. No audio is captured here and nothing leaves the computer until
// a finished sentence is translated.
//
// Ported from LiveCaptions-Translator by SakiRinn (Apache-2.0, see THIRD_PARTY_NOTICES.md):
// LiveCaptionsHandler.cs and the polling of Translator.SyncLoop. Start LiveCaptions.exe, find its
// window (class LiveCaptionsDesktopWindow), read the Name of the element with AutomationId
// CaptionsTextBlock every 25 ms (CaptionsScrollViewer in newer versions), move the window on
// screen if it is off screen or tiny, hide it
// (minimize + WS_EX_TOOLWINDOW), and start it again when the element is no longer available.
// Changed: a Live Captions window the user already had open is reused and given back when
// listening stops, instead of being killed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use tauri::AppHandle;

use crate::captions;
use crate::sentences::Event;

pub const TRANSPORT: &str = "windows-live-captions";

/// Whether this computer has Live Captions (Windows 11 22H2 or later).
pub fn installed() -> bool {
    imp::installed()
}

/// How far Live Captions is set up for the meeting language. Settings shows it as a checklist and
/// listening waits until `ready`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Setup {
    pub installed: bool,
    /// The user agreed in Live Captions' first-run prompt ("是，繼續").
    pub consented: bool,
    /// Installed speech files for the meeting language (a locale such as "en-US"), if any.
    pub speech: Option<String>,
    /// The language Live Captions is set to.
    pub language: Option<String>,
    /// The meeting language (settings; en-US when empty).
    pub meeting: String,
    pub ready: bool,
}

/// Judge the setup from what Windows reports. `packs` are the locales of the installed speech files.
fn evaluate(installed: bool, consented: bool, packs: &[String], language: Option<&str>, meeting: &str) -> Setup {
    let meeting = if meeting.trim().is_empty() { "en-US".to_string() } else { meeting.trim().to_string() };
    let same = |a: &str, b: &str| captions::iso639(a) == captions::iso639(b);
    // The exact locale if installed, else another variant of the same language (en-GB for en-US).
    let speech = packs.iter().find(|p| p.eq_ignore_ascii_case(&meeting)).or_else(|| packs.iter().find(|p| same(p, &meeting))).cloned();
    let language = language.map(str::to_string);
    // Its language must be one whose speech files are installed: set to en-US with only en-GB
    // installed, Live Captions asks to download en-US again.
    let usable = language.as_deref().is_some_and(|l| same(l, &meeting) && packs.iter().any(|p| p.eq_ignore_ascii_case(l)));
    let ready = installed && consented && speech.is_some() && usable;
    Setup { installed, consented, speech, language, meeting, ready }
}

pub fn setup(meeting: &str) -> Setup {
    evaluate(imp::installed(), imp::consented(), &imp::speech_packs(), imp::caption_language().as_deref(), meeting)
}

/// `setup`, after doing the one step the app can do itself: once the meeting language's speech
/// files are installed, switch Live Captions to it. Live Captions reads this setting when it starts.
pub fn prepare(meeting: &str) -> Setup {
    let s = setup(meeting);
    if let (true, true, Some(locale)) = (s.installed, s.consented, &s.speech) {
        if !s.ready && imp::set_caption_language(locale) {
            return setup(meeting);
        }
    }
    s
}

/// Open what a setup step needs: "live-captions" (its first-run prompt and language list) or
/// "language-settings" (Windows Settings, where speech files for a language are installed).
pub fn open_step(what: &str) -> Result<(), String> {
    imp::open_step(what)
}

pub struct Listening {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Listening {
    /// Stop, and wait until Live Captions is closed (or given back to the user).
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Start listening; `language` is the meeting's (settings, BCP-47). `ended` runs on the listening
/// thread when it ends, stopped or on its own.
pub fn start(app: AppHandle, language: String, ended: impl FnOnce() + Send + 'static) -> Listening {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let thread = std::thread::spawn(move || {
        let mut lines = Lines::default();
        if let Err(e) = imp::run(&app, &flag, &mut lines, &language) {
            captions::status(&app, format!("Windows 即時輔助字幕：{e}"));
        }
        lines.close(&app);
        ended();
    });
    Listening { stop, thread: Some(thread) }
}

/// The status line while listening. Live Captions recognizes the one language chosen in its own
/// settings (with that language's speech files installed); English heard by a Chinese model comes
/// out as nonsense, so a mismatch with the meeting language says so.
fn listening_status(live: Option<&str>, meeting: &str) -> String {
    match live {
        Some(live) if !captions::iso639(meeting).is_empty() && captions::iso639(live) != captions::iso639(meeting) => format!(
            "即時輔助字幕現在辨識 {live}，會議語言是 {meeting}：請在即時輔助字幕的 ⚙ ›「變更語言」改成會議的語言（第一次要下載語音檔），再重新按聆聽"
        ),
        _ => "聆聽中（Windows 即時輔助字幕）".into(),
    }
}

/// The caption line the sentence being spoken is shown in.
#[derive(Default)]
struct Lines {
    tracker: crate::sentences::Tracker,
    current: Option<u64>,
}

impl Lines {
    fn read(&mut self, app: &AppHandle, text: &str) {
        for ev in self.tracker.update(text, std::time::Instant::now()) {
            match ev {
                Event::Interim(t) => {
                    let id = *self.current.get_or_insert_with(|| captions::open(app));
                    captions::interim(app, id, &t);
                }
                Event::Final(t) => {
                    let id = self.current.take().unwrap_or_else(|| captions::open(app));
                    captions::finish(app, id, &t);
                }
                Event::Drop => {
                    if let Some(id) = self.current.take() {
                        captions::drop_line(app, id);
                    }
                }
            }
        }
    }

    /// Listening stopped: what was being said counts as said.
    fn close(&mut self, app: &AppHandle) {
        if let Some(id) = self.current.take() {
            captions::finish(app, id, self.tracker.open());
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use tauri::AppHandle;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{CloseHandle, HWND, RECT};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
    use windows::Win32::System::Registry::{RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_DWORD, RRF_RT_REG_SZ};
    use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE};
    use windows::Win32::System::Variant::VARIANT;
    use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation, IUIAutomationElement, TreeScope_Descendants, UIA_AutomationIdPropertyId};
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindow, MoveWindow, SetWindowLongPtrW, ShowWindow, GWL_EXSTYLE, SW_MINIMIZE, SW_RESTORE,
        WS_EX_TOOLWINDOW,
    };

    use super::{listening_status, Lines};
    use crate::captions;

    /// LiveCaptionsHandler reads the text in 10-20 ms; SyncLoop polls every 25 ms.
    const POLL: Duration = Duration::from_millis(25);
    const WINDOW_TIMEOUT: Duration = Duration::from_secs(15);
    /// On first use Live Captions asks to download its speech files before it shows any text.
    const TEXT_TIMEOUT: Duration = Duration::from_secs(15);
    /// Restarts without reading anything before giving up.
    const MAX_RESTARTS: u32 = 3;

    fn exe() -> Option<PathBuf> {
        std::env::var_os("SystemRoot").map(|r| PathBuf::from(r).join("System32").join("LiveCaptions.exe"))
    }

    pub fn installed() -> bool {
        exe().is_some_and(|p| p.is_file())
    }

    struct Com;
    impl Drop for Com {
        fn drop(&mut self) {
            unsafe { CoUninitialize() };
        }
    }

    const UI_KEY: PCWSTR = w!("Software\\Microsoft\\LiveCaptions\\UI");

    /// Whether the user agreed in the first-run prompt (UI key, HasUserConsented = 1).
    pub fn consented() -> bool {
        let mut v = 0u32;
        let mut len = 4u32;
        let err = unsafe { RegGetValueW(HKEY_CURRENT_USER, UI_KEY, w!("HasUserConsented"), RRF_RT_REG_DWORD, None, Some((&mut v as *mut u32).cast()), Some(&mut len)) };
        err.is_ok() && v == 1
    }

    /// Locales of the installed speech files: packages named MicrosoftWindows.Speech.<locale>.<n>.
    pub fn speech_packs() -> Vec<String> {
        let Some(dir) = std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("Packages")) else { return Vec::new() };
        let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
        entries
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .filter_map(|n| n.strip_prefix("MicrosoftWindows.Speech.").and_then(|r| r.split('.').next()).map(str::to_string))
            .collect()
    }

    pub fn set_caption_language(locale: &str) -> bool {
        let data: Vec<u16> = locale.encode_utf16().chain([0]).collect();
        let err = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, UI_KEY, w!("CaptionLanguage"), REG_SZ.0, Some(data.as_ptr().cast()), (data.len() * 2) as u32) };
        err.is_ok()
    }

    pub fn open_step(what: &str) -> Result<(), String> {
        let started = match what {
            "live-captions" => std::process::Command::new(exe().ok_or("找不到 Windows 資料夾")?).spawn(),
            "language-settings" => std::process::Command::new("explorer.exe").arg("ms-settings:regionlanguage").spawn(),
            other => return Err(format!("unknown step {other}")),
        };
        started.map(|_| ()).map_err(|e| format!("無法開啟：{e}"))
    }

    /// The language Live Captions recognizes (HKCU\Software\Microsoft\LiveCaptions\UI, CaptionLanguage).
    pub fn caption_language() -> Option<String> {
        let mut buf = [0u16; 64];
        let mut len = (buf.len() * 2) as u32;
        let err = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, UI_KEY, w!("CaptionLanguage"), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut len))
        };
        if err.is_err() {
            return None;
        }
        let n = (len as usize / 2).saturating_sub(1).min(buf.len());
        Some(String::from_utf16_lossy(&buf[..n])).filter(|l| !l.is_empty())
    }

    pub fn run(app: &AppHandle, stop: &AtomicBool, lines: &mut Lines, language: &str) -> Result<(), String> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.ok().map_err(|e| format!("COM: {e}"))?;
        let _com = Com;
        let uia: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }.map_err(|e| format!("UI Automation: {e}"))?;
        let mut first = true;
        let mut restarts = 0;
        while !stop.load(Ordering::SeqCst) {
            captions::status(app, if first { "啟動 Windows 即時輔助字幕…" } else { "即時輔助字幕關閉了，重新啟動中…" });
            let Some(win) = Window::open(stop)? else { break };
            let text = match find_text(&uia, win.hwnd, stop, TEXT_TIMEOUT) {
                Ok(Some(t)) => t,
                Ok(None) => {
                    win.close(false);
                    break;
                }
                Err(e) => {
                    // Probably first use: show Live Captions so its own setup can be finished.
                    win.close(true);
                    return Err(e);
                }
            };
            captions::status(app, listening_status(caption_language().as_deref(), language));
            if first && !win.started {
                // It was already running: what it showed before is not part of this meeting.
                lines.tracker.skip(&unsafe { text.CurrentName() }.map(|b| b.to_string()).unwrap_or_default());
            }
            first = false;
            let mut text = text;
            let mut read_any = false;
            while !stop.load(Ordering::SeqCst) {
                match unsafe { text.CurrentName() } {
                    Ok(name) => {
                        read_any = true;
                        lines.read(app, &name.to_string());
                    }
                    // ElementNotAvailable: Live Captions closed or rebuilt its view.
                    Err(_) => match unsafe { IsWindow(Some(win.hwnd)) }.as_bool().then(|| find_text(&uia, win.hwnd, stop, Duration::from_secs(2))) {
                        Some(Ok(Some(t))) => text = t,
                        _ => break,
                    },
                }
                std::thread::sleep(POLL);
            }
            if stop.load(Ordering::SeqCst) {
                win.close(false);
                break;
            }
            restarts = if read_any { 0 } else { restarts + 1 };
            if restarts >= MAX_RESTARTS {
                return Err("一直無法讀到字幕，請確認即時輔助字幕能正常開啟（Win+Ctrl+L）".into());
            }
        }
        Ok(())
    }

    /// The caption text element, waiting up to `timeout`; None when listening stopped meanwhile.
    fn find_text(uia: &IUIAutomation, hwnd: HWND, stop: &AtomicBool, timeout: Duration) -> Result<Option<IUIAutomationElement>, String> {
        let root = unsafe { uia.ElementFromHandle(hwnd) }.map_err(|e| format!("UI Automation: {e}"))?;
        // CaptionsTextBlock in the version LiveCaptions-Translator targets; newer Live Captions puts
        // the same text in the Name of CaptionsScrollViewer (seen on Windows 11 26200, 2026-10).
        let mut ids = Vec::new();
        for id in ["CaptionsTextBlock", "CaptionsScrollViewer"] {
            ids.push(unsafe { uia.CreatePropertyCondition(UIA_AutomationIdPropertyId, &VARIANT::from(id)) }.map_err(|e| format!("UI Automation: {e}"))?);
        }
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if stop.load(Ordering::SeqCst) {
                return Ok(None);
            }
            for by_id in &ids {
                if let Ok(el) = unsafe { root.FindFirst(TreeScope_Descendants, by_id) } {
                    return Ok(Some(el));
                }
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        Err("讀不到即時輔助字幕的文字。請到「設定」確認「Windows 即時輔助字幕」下面的步驟都打勾了".into())
    }

    struct Window {
        hwnd: HWND,
        /// Started for this session (closed when done), or the user's own (given back).
        started: bool,
        ex_style: isize,
    }

    impl Window {
        fn find() -> Option<HWND> {
            unsafe { FindWindowW(w!("LiveCaptionsDesktopWindow"), PCWSTR::null()) }.ok().filter(|h| !h.is_invalid())
        }

        /// Use the running Live Captions or start it; None when listening stopped meanwhile.
        fn open(stop: &AtomicBool) -> Result<Option<Window>, String> {
            if let Some(h) = Self::find() {
                return Ok(Some(Self::adopt(h, false)));
            }
            let exe = exe().ok_or("找不到 Windows 資料夾")?;
            std::process::Command::new(exe).spawn().map_err(|e| format!("無法啟動 LiveCaptions.exe：{e}"))?;
            let deadline = Instant::now() + WINDOW_TIMEOUT;
            while Instant::now() < deadline {
                if stop.load(Ordering::SeqCst) {
                    if let Some(h) = Self::find() {
                        Self::adopt(h, true).close(false);
                    }
                    return Ok(None);
                }
                if let Some(h) = Self::find() {
                    return Ok(Some(Self::adopt(h, true)));
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err("即時輔助字幕的視窗沒有出現".into())
        }

        /// LiveCaptionsHandler.FixLiveCaptions, then HideLiveCaptions.
        fn adopt(hwnd: HWND, started: bool) -> Window {
            unsafe {
                let mut r = RECT::default();
                if !IsIconic(hwnd).as_bool() && GetWindowRect(hwnd, &mut r).is_ok() && (r.left < 0 || r.top < 0 || r.right - r.left < 100 || r.bottom - r.top < 100) {
                    let _ = MoveWindow(hwnd, 800, 600, 600, 200, true);
                }
                let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex_style | WS_EX_TOOLWINDOW.0 as isize);
                Window { hwnd, started, ex_style }
            }
        }

        /// Close Live Captions if it was started for this session; otherwise (or with `show`) put
        /// its window back.
        fn close(self, show: bool) {
            unsafe {
                if !IsWindow(Some(self.hwnd)).as_bool() {
                    return;
                }
                if self.started && !show {
                    let mut pid = 0u32;
                    GetWindowThreadProcessId(self.hwnd, Some(&mut pid));
                    if let Ok(h) = OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, false, pid) {
                        let _ = TerminateProcess(h, 0);
                        let _ = WaitForSingleObject(h, 2000);
                        let _ = CloseHandle(h);
                    }
                    return;
                }
                SetWindowLongPtrW(self.hwnd, GWL_EXSTYLE, self.ex_style & !(WS_EX_TOOLWINDOW.0 as isize));
                let _ = ShowWindow(self.hwnd, SW_RESTORE);
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::sync::atomic::AtomicBool;

    use tauri::AppHandle;

    use super::Lines;

    pub fn installed() -> bool {
        false
    }

    pub fn consented() -> bool {
        false
    }

    pub fn speech_packs() -> Vec<String> {
        Vec::new()
    }

    pub fn caption_language() -> Option<String> {
        None
    }

    pub fn set_caption_language(_: &str) -> bool {
        false
    }

    pub fn open_step(_: &str) -> Result<(), String> {
        Err("只有 Windows 11 有即時輔助字幕".into())
    }

    pub fn run(_: &AppHandle, _: &AtomicBool, _: &mut Lines, _: &str) -> Result<(), String> {
        Err("只有 Windows 11 有即時輔助字幕".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_is_ready_only_with_consent_speech_files_and_the_meeting_language() {
        let zh = vec!["zh-TW".to_string()];
        let both = vec!["zh-TW".to_string(), "en-US".to_string()];
        // A Chinese Windows that only agreed: English speech files still missing.
        let s = evaluate(true, true, &zh, Some("zh-TW"), "en-US");
        assert_eq!((s.speech.as_deref(), s.ready), (None, false));
        // English installed but Live Captions still on Chinese: the app switches it (prepare).
        let s = evaluate(true, true, &both, Some("zh-TW"), "en-US");
        assert_eq!((s.speech.as_deref(), s.ready), (Some("en-US"), false));
        assert!(evaluate(true, true, &both, Some("en-US"), "en-US").ready);
        // Not agreed yet, or no Live Captions at all.
        assert!(!evaluate(true, false, &both, Some("en-US"), "en-US").ready);
        assert!(!evaluate(false, true, &both, Some("en-US"), "en-US").ready);
        // Another English variant counts; an empty meeting language means English.
        let gb = vec!["en-GB".to_string()];
        assert_eq!(evaluate(true, true, &gb, Some("en-GB"), "en-US").speech.as_deref(), Some("en-GB"));
        assert!(evaluate(true, true, &gb, Some("en-GB"), "").ready);
        assert_eq!(evaluate(true, true, &gb, None, " ").meeting, "en-US");
        // Set to an English whose speech files are missing (en-US, only en-GB installed): not ready,
        // so the app switches it to en-GB.
        let gb_zh = vec!["zh-TW".to_string(), "en-GB".to_string()];
        let s = evaluate(true, true, &gb_zh, Some("en-US"), "");
        assert_eq!((s.speech.as_deref(), s.ready), (Some("en-GB"), false));
        assert!(evaluate(true, true, &gb_zh, Some("en-GB"), "").ready);
    }

    #[test]
    fn a_live_captions_language_other_than_the_meetings_is_pointed_out() {
        assert_eq!(listening_status(Some("en-US"), "en-US"), "聆聽中（Windows 即時輔助字幕）");
        assert_eq!(listening_status(Some("en-GB"), "en-US"), "聆聽中（Windows 即時輔助字幕）");
        assert_eq!(listening_status(None, "en-US"), "聆聽中（Windows 即時輔助字幕）");
        assert_eq!(listening_status(Some("zh-TW"), ""), "聆聽中（Windows 即時輔助字幕）");
        let w = listening_status(Some("zh-TW"), "en-US");
        assert!(w.contains("zh-TW") && w.contains("en-US") && w.contains("變更語言"), "{w}");
    }
}

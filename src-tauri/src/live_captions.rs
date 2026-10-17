// Listening through Windows 11's Live Captions (LiveCaptions.exe, 22H2 and later), which recognizes
// the system audio on the device for free. This reads its caption text with UI Automation on a
// thread of its own and turns it into caption lines (sentences.rs); its window is minimized and kept
// off the taskbar while listening. No audio is captured here and nothing leaves the computer until
// a finished sentence is translated.
//
// Ported from LiveCaptions-Translator by SakiRinn (Apache-2.0, see THIRD_PARTY_NOTICES.md):
// LiveCaptionsHandler.cs and the polling of Translator.SyncLoop. Start LiveCaptions.exe, find its
// window (class LiveCaptionsDesktopWindow), read the Name of the element with AutomationId
// CaptionsTextBlock every 25 ms, move the window on screen if it is off screen or tiny, hide it
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
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
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

    /// The language Live Captions recognizes (HKCU\Software\Microsoft\LiveCaptions\UI, CaptionLanguage).
    fn caption_language() -> Option<String> {
        let mut buf = [0u16; 64];
        let mut len = (buf.len() * 2) as u32;
        let err = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, w!("Software\\Microsoft\\LiveCaptions\\UI"), w!("CaptionLanguage"), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut len))
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
        let by_id = unsafe { uia.CreatePropertyCondition(UIA_AutomationIdPropertyId, &VARIANT::from("CaptionsTextBlock")) }.map_err(|e| format!("UI Automation: {e}"))?;
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if stop.load(Ordering::SeqCst) {
                return Ok(None);
            }
            if let Ok(el) = unsafe { root.FindFirst(TreeScope_Descendants, &by_id) } {
                return Ok(Some(el));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        Err("還沒有字幕文字。第一次使用時，請在跳出的即時輔助字幕視窗選會議的語言（英文會議選 English）、按「繼續」下載語音檔，完成後再按聆聽".into())
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

    pub fn run(_: &AppHandle, _: &AtomicBool, _: &mut Lines, _: &str) -> Result<(), String> {
        Err("只有 Windows 11 有即時輔助字幕".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

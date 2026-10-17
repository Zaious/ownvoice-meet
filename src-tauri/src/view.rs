// Window behaviour the web views cannot do on their own: keeping the reply and script windows out
// of screen sharing, click-through for the captions window, and the tray menu that turns it off
// again (a window that ignores the mouse cannot be clicked back). The tray follows Basesmall's.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, Window, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::settings;

/// The windows that hold the user's own words. The captions window stays capturable: what others
/// said is already on everyone's screen.
pub const PRIVATE: [&str; 2] = ["reply", "script"];
/// Turns click-through on and off from anywhere, including while it is on.
pub const CLICK_THROUGH_SHORTCUT: &str = "CommandOrControl+Shift+F8";

#[derive(Default)]
pub struct ClickThrough {
    on: AtomicBool,
    item: Mutex<Option<CheckMenuItem<Wry>>>,
}

/// Exclude the window from capture (SetWindowDisplayAffinity WDA_EXCLUDEFROMCAPTURE on Windows 10
/// 2004 and later; older Windows ignores it).
pub fn protect(app: &AppHandle, on: bool) {
    for label in PRIVATE {
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.set_content_protected(on);
        }
    }
}

/// Called for every new window, before its page draws anything.
pub fn on_window_ready(w: Window) {
    if PRIVATE.contains(&w.label()) {
        let _ = w.set_content_protected(settings::load(w.app_handle()).view.hide_from_capture);
    }
}

pub fn click_through(app: &AppHandle) -> bool {
    app.state::<ClickThrough>().on.load(Ordering::SeqCst)
}

/// Let clicks fall through the captions window to the meeting underneath, or stop doing so.
pub fn set_click_through(app: &AppHandle, on: bool) {
    let Some(w) = app.get_webview_window("main") else { return };
    let _ = w.set_ignore_cursor_events(on);
    let state = app.state::<ClickThrough>();
    state.on.store(on, Ordering::SeqCst);
    if let Some(item) = state.item.lock().ok().and_then(|i| i.clone()) {
        let _ = item.set_checked(on);
    }
    let _ = app.emit("click-through", on);
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let through = CheckMenuItem::with_id(app, "click_through", "滑鼠穿透字幕視窗（Ctrl+Shift+F8）", true, false, None::<&str>)?;
    let on_top = CheckMenuItem::with_id(app, "on_top", "字幕視窗置頂", true, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "結束 OwnVoice Meet", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&through, &on_top, &PredefinedMenuItem::separator(app)?, &quit])?;
    if let Ok(mut slot) = app.state::<ClickThrough>().item.lock() {
        *slot = Some(through.clone());
    }
    let top = on_top.clone();
    let mut tray = TrayIconBuilder::with_id("main").tooltip("OwnVoice Meet").menu(&menu).show_menu_on_left_click(true);
    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }
    tray.on_menu_event(move |app, event| match event.id().as_ref() {
        // The item has already flipped its own check mark.
        "click_through" => set_click_through(app, !click_through(app)),
        "on_top" => {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_always_on_top(top.is_checked().unwrap_or(true));
            }
        }
        "quit" => app.exit(0),
        _ => {}
    })
    .build(app)?;

    // Registered here rather than in a web view: it must work while the captions window ignores input.
    let shortcut = app.global_shortcut().on_shortcut(CLICK_THROUGH_SHORTCUT, |app, _, e| {
        if e.state == ShortcutState::Pressed {
            set_click_through(app, !click_through(app));
        }
    });
    if let Err(e) = shortcut {
        // Another program holds the key; the tray still works.
        crate::set_status(app, format!("穿透快捷鍵註冊失敗（可能被其他程式占用）：{e}"));
    }
    Ok(())
}

//! **Send to {app}** (Wispr Flow's "Send in {app}", by behaviour;
//! 2026-10-29): the quick note's button that puts the note's text — or its
//! selection — into the app the person was in when the window came up, and
//! steps aside.
//!
//! - **Which app**: the window in front as the quick note opened
//!   ([`remember_before`], called by `quick_note::open` before it shows),
//!   found as "Paste last transcript" finds it (`text_injector::app_window`:
//!   the foreground window, or — the tray just had it — the app window on
//!   top). Never one of Yap's own windows: opened from Yap, there's nothing
//!   to send to and the button isn't shown.
//! - **Its name and icon** ([`quick_note_send_target`]): the process name
//!   (the page names it, `core/appNames.js`) and the exe's own icon (drawn
//!   as the call apps' are, `calls/app_icons.rs`), kept per exe.
//! - **The paste** ([`quick_note_send`]): the window put away, then the
//!   ordinary paste path (`text_injector::inject_text`, strict: back into that
//!   window or nowhere) — its paste keys, the checks that it landed, the
//!   clipboard left as it was. A paste that can't land leaves the text on the
//!   clipboard and says so once (`Left::alert`); a window that's gone too.
//!
//! A test run reads and pastes into no real window: a spec stages the app
//! it pretends was in front (`e2e_quick_note_before`) and reads what was
//! sent (`e2e_quick_note_sent`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use tauri::AppHandle;

/// The window that was in front when the quick note came up (0: none, or
/// one of Yap's).
static BEFORE: AtomicIsize = AtomicIsize::new(0);
/// Icons drawn, by exe path (data URLs; `None`: it has none).
static ICONS: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);

/// Test mode: the stand-in "window" a spec staged (no window has it) and
/// its app's name; what was sent last.
#[cfg(debug_assertions)]
const TEST_STAND_IN: isize = 0x7E57_0002;
#[cfg(debug_assertions)]
static TEST_APP: Mutex<Option<String>> = Mutex::new(None);
#[cfg(debug_assertions)]
static TEST_SENT: Mutex<Option<(String, isize)>> = Mutex::new(None);

#[cfg(debug_assertions)]
fn test_run() -> bool {
    crate::shell::e2e::active() && !crate::shell::realtest::on()
}

/// The app window the person is in now, if it isn't one of Yap's (pure
/// apart from the reads).
fn app_in_front() -> Option<isize> {
    #[cfg(debug_assertions)]
    if test_run() {
        return TEST_APP.lock().unwrap_or_else(|p| p.into_inner()).as_ref().map(|_| TEST_STAND_IN);
    }
    let own = std::process::id();
    crate::pasting::text_injector::app_window()
        .filter(|&h| crate::pasting::text_injector::window_process(h).is_some_and(|pid| pid != own))
}

/// Remember the window in front, for Send (`quick_note::open`, before the
/// quick note shows). One of Yap's own keeps what was remembered only when
/// it's the quick note itself (opened again while it's up).
pub fn remember_before(quick_note_in_front: bool) {
    if quick_note_in_front {
        return;
    }
    BEFORE.store(app_in_front().unwrap_or(0), Ordering::SeqCst);
}

/// The window Send goes to, while it's still there.
fn target() -> Option<isize> {
    let h = BEFORE.load(Ordering::SeqCst);
    if h == 0 {
        return None;
    }
    #[cfg(debug_assertions)]
    if test_run() {
        return (h == TEST_STAND_IN).then_some(h);
    }
    crate::pasting::text_injector::window_exists(h).then_some(h)
}

/// Where Send goes: the app's process name ("slack.exe") and its icon.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendTarget {
    pub app: String,
    pub icon: Option<String>,
}

fn target_info(h: isize) -> Option<SendTarget> {
    #[cfg(debug_assertions)]
    if test_run() {
        let app = TEST_APP.lock().unwrap_or_else(|p| p.into_inner()).clone()?;
        return Some(SendTarget { app, icon: None });
    }
    let app = crate::pasting::text_injector::app_name_for(Some(h))?;
    let icon = crate::pasting::text_injector::window_process(h)
        .and_then(crate::shell::procs::image_path)
        .and_then(|exe| icon_for(&exe));
    Some(SendTarget { app, icon })
}

/// The exe's own icon as a data URL, drawn once per exe.
fn icon_for(exe: &str) -> Option<String> {
    let key = exe.to_ascii_lowercase();
    if let Some(known) = ICONS.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(HashMap::new).get(&key) {
        return known.clone();
    }
    let icon = crate::calls::app_icons::exe_icon_url(std::path::Path::new(exe));
    ICONS.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(HashMap::new).insert(key, icon.clone());
    icon
}

/// The app Send would go to (`null`: none — the quick note came up from
/// Yap, the desktop, or that window has closed).
#[tauri::command]
pub async fn quick_note_send_target() -> Result<Option<SendTarget>, String> {
    tauri::async_runtime::spawn_blocking(|| target().and_then(target_info))
        .await
        .map_err(|e| e.to_string())
}

/// **Send to {app}**: the quick note steps aside and `text` (the note's, or
/// its selection) is pasted into the app the person was in, by the ordinary
/// paste path. Errors when there's nothing to send or nowhere to send it.
#[tauri::command]
pub async fn quick_note_send(app: AppHandle, text: String) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("There's nothing to send yet".into());
    }
    let Some(hwnd) = target() else {
        return Err("The app you were in has closed".into());
    };
    super::hide(&app);
    tracing::info!(chars = text.chars().count(), "Quick note: sending the note to the app it came from");
    #[cfg(debug_assertions)]
    if test_run() {
        *TEST_SENT.lock().unwrap_or_else(|p| p.into_inner()) = Some((text, hwnd));
        tracing::info!("e2e: not pasting the note");
        return Ok(());
    }
    let restore = crate::config::load().restore_clipboard;
    let kind = crate::pasting::app_kind::of_window(Some(hwnd));
    use crate::pasting::text_injector::Delivery;
    match crate::pasting::text_injector::inject_text(&text, restore, Some(hwnd), true, kind).await? {
        Delivery::Pasted | Delivery::Typed => {}
        Delivery::LeftOnClipboard(left) => crate::notices::alert::show_soon(&app, left.alert()),
        Delivery::WindowGone => crate::notices::alert::show_soon(&app, window_gone()),
    }
    Ok(())
}

/// The app Send was for has closed (or won't come to the front): the note
/// is on the clipboard instead.
fn window_gone() -> crate::notices::alert::Alert {
    crate::notices::alert::Alert {
        kind: "pasteBlocked",
        icon: "clipboard",
        title: "Your note is copied: press Ctrl+V".to_string(),
        body: "Yap couldn't get back to the app you were in, so your note is on the clipboard instead.".to_string(),
        action: None,
        timeout_ms: 12_000,
    }
}

/// Debug-only, test mode: the app a spec pretends was in front when the
/// quick note next opens (its process name; `null`: none).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_quick_note_before(app: Option<String>) -> Result<(), String> {
    if !test_run() {
        return Err("Only in e2e test mode".into());
    }
    *TEST_APP.lock().unwrap_or_else(|p| p.into_inner()) = app;
    *TEST_SENT.lock().unwrap_or_else(|p| p.into_inner()) = None;
    Ok(())
}

/// Debug-only, test mode: what Send sent last (`{text, app}`), or null.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_quick_note_sent() -> Result<Option<serde_json::Value>, String> {
    if !test_run() {
        return Err("Only in e2e test mode".into());
    }
    let sent = TEST_SENT.lock().unwrap_or_else(|p| p.into_inner()).clone();
    let app = TEST_APP.lock().unwrap_or_else(|p| p.into_inner()).clone();
    Ok(sent.map(|(text, h)| serde_json::json!({ "text": text, "app": app, "standIn": h == TEST_STAND_IN })))
}

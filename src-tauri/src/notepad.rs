//! The meeting notepad — Yap's meeting UI while a call runs (Wispr Flow's
//! Notetaker notepad, ported to Yap): a slim window docked to the right edge
//! of the screen, with the meeting's own notes ("My thoughts"), the live
//! transcript, the summary and "What did I miss?" (`src/lib/Notepad.svelte`).
//!
//! - **One window**, `notepad` in tauri.conf.json, created hidden and
//!   unfocused at startup like onboarding: it opens instantly, `capture.rs`
//!   always finds it by label, and opening it never takes the foreground
//!   (tao keeps the "don't focus" marker of a window created unfocused, so
//!   every `show()` is `SW_SHOWNOACTIVATE`; only a click on "Notepad" in Yap
//!   focuses it). Closing hides it; the recording carries on.
//! - **When**: a meeting recording starts from anywhere (`meeting_end`
//!   follows the recorder's `yap-meeting-state`) and "Open the notepad when a
//!   meeting starts" is on (`meeting_open_notepad`, default on). It shows the
//!   meeting being recorded; the Notes view reopens it on any meeting note
//!   ([`notepad_open`]).
//! - **Where**: docked to the right edge of the work area, full height, 30%
//!   of the width as Wispr Flow (768 px on a 2560 px screen; 400–800 px at
//!   100%, [`notepad_width`]), on the monitor with the call's window, else the
//!   one with the mouse cursor. Already on screen, it stays where the person
//!   put it.
//! - **"Split the screen when joining"** (`meeting_split_screen`, off by
//!   default): when a recording starts during a detected call, the call
//!   app's main window — the largest visible top-level window of its process,
//!   for a browser preferably the one whose title shows the meeting
//!   ([`pick_call_window`]) — is restored if maximised and moved to the rest
//!   of the work area, left of the notepad. The notepad's split button does
//!   the same at once ([`notepad_split`]); hovering it shows where the call
//!   will go as a glass outline (a small click-through window,
//!   [`notepad_split_preview`]). Never one of Yap's own windows, and never in
//!   a test run, which leaves other apps' windows alone.

use std::sync::atomic::{AtomicU64, Ordering};

use tauri::{AppHandle, Emitter, Manager};

/// The window's label (tauri.conf.json; `capture.rs` hides it from screen
/// capture while a meeting records).
pub const LABEL: &str = "notepad";
/// The note the notepad shows: `{ noteId }` (null: none).
const EVENT_NOTE: &str = "yap-notepad-note";
/// Yap showed or hid the notepad: `true` / `false`. (WebView2 reports the
/// page visible while its window is hidden, so the page can't tell.)
const EVENT_VISIBLE: &str = "yap-notepad-visible";

/// The note the notepad shows (0 = none).
static NOTE: AtomicU64 = AtomicU64::new(0);

/// A screen rectangle in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// The notepad's width on a work area `work_w` physical px wide at DPI
/// `scale`: 30% of it, as Wispr Flow (768 px on a 2560 px screen), between
/// 400 and 800 px at 100%, and never more than half the work area.
pub fn notepad_width(work_w: i32, scale: f64) -> i32 {
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let min = (400.0 * scale).round() as i32;
    let max = (800.0 * scale).round() as i32;
    let want = (f64::from(work_w) * 0.30).round() as i32;
    want.clamp(min, max).min(work_w / 2).max(1)
}

/// Docked to the right edge of `work`, full height.
pub fn dock_rect(work: Rect, width: i32) -> Rect {
    Rect {
        x: work.x + work.w - width,
        y: work.y,
        w: width,
        h: work.h,
    }
}

/// Split the screen: (the call's window on the left, the notepad on the right).
pub fn split_rects(work: Rect, width: i32) -> (Rect, Rect) {
    let call = Rect {
        x: work.x,
        y: work.y,
        w: work.w - width,
        h: work.h,
    };
    (call, dock_rect(work, width))
}

/// The window rect that puts a window's visible frame at `target`, when its
/// window rect reaches `(left, top, right, bottom)` px past the visible frame
/// (the invisible resize borders of a normal Windows 10/11 window).
pub fn outer_rect(target: Rect, (left, top, right, bottom): (i32, i32, i32, i32)) -> Rect {
    Rect {
        x: target.x - left,
        y: target.y - top,
        w: target.w + left + right,
        h: target.h + top + bottom,
    }
}

/// Room around the split preview's glass outline for its shadow (CSS px; the
/// page insets the outline by the same).
pub const PREVIEW_MARGIN: i32 = 24;

/// The split preview window over `slot` (where the call's window will go,
/// at DPI `scale`): the slot plus room for the outline's shadow all round.
pub fn preview_rect(slot: Rect, scale: f64) -> Rect {
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let m = (f64::from(PREVIEW_MARGIN) * scale).round() as i32;
    Rect {
        x: slot.x - m,
        y: slot.y - m,
        w: slot.w + 2 * m,
        h: slot.h + 2 * m,
    }
}

/// A top-level window, as [`pick_call_window`] sees it.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub hwnd: isize,
    pub pid: u32,
    /// The process's exe file name ("ms-teams.exe").
    pub exe: String,
    pub title: String,
    pub rect: Rect,
    pub minimized: bool,
    /// A tool window, an owned window (dialogs, pop-ups) or a cloaked one
    /// (a suspended UWP app's frame): never the call's main window.
    pub tool: bool,
    pub owned: bool,
    pub cloaked: bool,
}

/// The call app's main window among visible top-level `windows`: one of
/// `exes` (the call app's, or the browser showing the meeting), not Yap's
/// own process (`own_pid`), not minimised, a real window (titled, at least
/// 200×150, not a tool/owned/cloaked one); preferably one whose title shows
/// the call (`shows_call`), then the largest.
pub fn pick_call_window(
    windows: &[Candidate],
    exes: &[String],
    own_pid: u32,
    shows_call: impl Fn(&str) -> bool,
) -> Option<isize> {
    let fits: Vec<&Candidate> = windows
        .iter()
        .filter(|c| {
            c.pid != own_pid
                && !c.minimized
                && !c.tool
                && !c.owned
                && !c.cloaked
                && c.rect.w >= 200
                && c.rect.h >= 150
                && !c.title.trim().is_empty()
                && exes.iter().any(|e| e.eq_ignore_ascii_case(&c.exe))
        })
        .collect();
    let area = |c: &&&Candidate| i64::from(c.rect.w) * i64::from(c.rect.h);
    fits.iter()
        .filter(|c| shows_call(&c.title))
        .max_by_key(area)
        .or_else(|| fits.iter().max_by_key(area))
        .map(|c| c.hwnd)
}

/// The note the notepad shows (0 = none).
pub fn current_note() -> u64 {
    NOTE.load(Ordering::SeqCst)
}

fn set_note(app: &AppHandle, note_id: u64) {
    NOTE.store(note_id, Ordering::SeqCst);
    let id = (note_id != 0).then_some(note_id);
    let _ = app.emit(EVENT_NOTE, serde_json::json!({ "noteId": id }));
}

fn window(app: &AppHandle) -> Option<tauri::WebviewWindow> {
    app.get_webview_window(LABEL)
}

/// On screen (shown and not minimised).
fn on_screen(w: &tauri::WebviewWindow) -> bool {
    w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false)
}

/// Whether the notepad is on screen (it shows a meeting's summary as it's
/// written). Never call with a lock held: window getters wait on the main
/// thread.
pub fn shown(app: &AppHandle) -> bool {
    window(app).is_some_and(|w| on_screen(&w))
}

/// Closing the notepad hides it (app setup), like the main window.
pub fn init(app: &AppHandle) {
    if let Some(w) = window(app) {
        let hide = w.clone();
        w.on_window_event(move |event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = hide.hide();
                let _ = hide.emit(EVENT_VISIBLE, false);
            }
        });
    }
}

/// A meeting recording of `note_id` started (`meeting_end` saw its
/// `yap-meeting-state`): the notepad switches to it and, with "Open the
/// notepad when a meeting starts" on, comes up docked beside the call
/// without taking the focus (and splits the screen with it when asked).
pub fn on_meeting_started(app: &AppHandle, note_id: u64) {
    set_note(app, note_id);
    let cfg = crate::config::load();
    if !cfg.meeting_open_notepad {
        return;
    }
    let split = cfg.meeting_split_screen;
    let app = app.clone();
    // Off the recorder's thread: this queries and moves windows.
    std::thread::spawn(move || {
        let Some(w) = window(&app) else { return };
        let call = if crate::e2e::active() { None } else { platform::call_window() };
        if on_screen(&w) && !(split && call.is_some()) {
            return; // already up where the person put it
        }
        let placed = platform::place_docked(&w, call, split);
        if !placed {
            tracing::warn!("notepad: couldn't place it; showing it where it was");
        }
        show(&w, false);
        tracing::info!(note_id, split = split && call.is_some(), "notepad: opened for a meeting");
    });
}

/// Show the notepad on `note_id`, docked if it isn't on screen, and focus it:
/// a person asked for it (the Notes view's "Notepad", the Yap bar). Safe from
/// any thread.
pub fn open(app: &AppHandle, note_id: u64) {
    set_note(app, note_id);
    let Some(w) = window(app) else { return };
    if !w.is_visible().unwrap_or(false) {
        platform::place_docked(&w, None, false);
    }
    show(&w, true);
}

/// `focus`: also bring it to the front (never in test runs: tao's fallback
/// presses Alt in whichever app has focus).
fn show(w: &tauri::WebviewWindow, focus: bool) {
    if w.is_minimized().unwrap_or(false) {
        if focus {
            let _ = w.unminimize();
        } else {
            platform::restore_quietly(w);
        }
    }
    let _ = w.show();
    let _ = w.emit(EVENT_VISIBLE, true);
    if focus && !crate::e2e::active() {
        let _ = w.set_focus();
    }
}

/// A meeting note was discarded: if the notepad showed it, it goes away.
pub fn on_note_deleted(app: &AppHandle, note_id: u64) {
    if NOTE.compare_exchange(note_id, 0, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
        set_note(app, 0);
        if let Some(w) = window(app) {
            let _ = w.hide();
            let _ = w.emit(EVENT_VISIBLE, false);
        }
    }
}

// ---- splitting the screen from the notepad ------------------------------------------------

/// The split preview's label (an on-demand window, not in tauri.conf.json).
const PREVIEW_LABEL: &str = "split-preview";
/// Bumped by every show and hide of the preview: a show that finishes after
/// the pointer already left stays hidden.
static PREVIEW_GEN: AtomicU64 = AtomicU64::new(0);

/// The notepad's split button: put the live call's window left of the
/// notepad now, as "Split the screen when joining" does when a recording
/// starts. Not in test runs, which never move other apps' windows.
pub fn split_now(app: &AppHandle) -> Result<(), String> {
    hide_preview(app);
    if crate::e2e::active() {
        return Err("Test runs never move other apps' windows.".to_string());
    }
    let w = window(app).ok_or("The notepad isn't there")?;
    let call = platform::call_window().ok_or(
        "No call to split the screen with yet. Yap splits it with a call it noticed, in Teams, Zoom, Meet and the like.",
    )?;
    if !platform::place_docked(&w, Some(call), true) {
        return Err("Couldn't move the windows".to_string());
    }
    show(&w, false);
    tracing::info!("notepad: split the screen with the call (button)");
    Ok(())
}

/// Hovering the split button: a glass outline over the slot the call's
/// window would move to (Wispr Flow's split preview), in a small
/// borderless, click-through window that never takes the focus. Created on
/// first use. Returns whether it shows (there's a call to split with).
pub async fn show_preview(app: &AppHandle) -> Result<bool, String> {
    let gen = PREVIEW_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    if crate::e2e::active() {
        return Ok(false);
    }
    let Some((slot, scale)) = platform::call_slot() else {
        return Ok(false);
    };
    let frame = preview_rect(slot, scale);
    let w = match app.get_webview_window(PREVIEW_LABEL) {
        Some(w) => w,
        None => build_preview(app)?,
    };
    let current = || PREVIEW_GEN.load(Ordering::SeqCst) == gen;
    if current() {
        let _ = w.set_position(tauri::PhysicalPosition::new(frame.x, frame.y));
        let _ = w.set_size(tauri::PhysicalSize::new(frame.w.max(1) as u32, frame.h.max(1) as u32));
        let _ = w.show();
    }
    // The pointer left (or the split happened) while it was coming up.
    if !current() {
        let _ = w.hide();
        return Ok(false);
    }
    Ok(true)
}

/// Take the split preview down (the pointer left the button, or the split
/// happened).
pub fn hide_preview(app: &AppHandle) {
    PREVIEW_GEN.fetch_add(1, Ordering::SeqCst);
    if let Some(w) = app.get_webview_window(PREVIEW_LABEL) {
        let _ = w.hide();
    }
}

fn build_preview(app: &AppHandle) -> Result<tauri::WebviewWindow, String> {
    let w = tauri::WebviewWindowBuilder::new(
        app,
        PREVIEW_LABEL,
        tauri::WebviewUrl::App("index.html".into()),
    )
    .title("Yap")
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .focused(false)
    .focusable(false)
    .visible(false)
    .build()
    .map_err(|e| format!("Couldn't show the split preview: {e}"))?;
    let _ = w.set_ignore_cursor_events(true);
    // A meeting window: left out of screen shares while one records
    // (capture::MEETING_WINDOWS), from its first show on.
    crate::capture::sync_window(&w);
    Ok(w)
}

// ---- commands ------------------------------------------------------------------------------

/// The notepad's split button (see [`split_now`]).
#[tauri::command]
pub async fn notepad_split(app: AppHandle) -> Result<(), String> {
    split_now(&app)
}

/// The split button's hover preview: `show` on pointer enter, not on leave
/// (see [`show_preview`]). Async, as it may create a window.
#[tauri::command]
pub async fn notepad_split_preview(app: AppHandle, show: bool) -> Result<bool, String> {
    if show {
        show_preview(&app).await
    } else {
        hide_preview(&app);
        Ok(false)
    }
}

/// Show the notepad on `note_id` and focus it (the Notes view's "Notepad").
#[tauri::command]
pub fn notepad_open(app: AppHandle, note_id: u64) -> Result<(), String> {
    crate::notes::get(note_id).ok_or("Note not found")?;
    open(&app, note_id);
    Ok(())
}

/// `{ noteId }`: the note the notepad shows (null: none yet).
#[tauri::command]
pub fn notepad_state() -> serde_json::Value {
    let id = current_note();
    serde_json::json!({ "noteId": (id != 0).then_some(id) })
}

/// The consent message changed: `{ message }` (empty = Yap's default).
const EVENT_CONSENT: &str = "yap-consent-message-changed";

/// Save the consent message the notepad copies for the meeting chat, as the
/// person edited it (`meeting_consent_message`; empty = Yap's default).
/// Settings' copy of the config adopts it from `yap-consent-message-changed`,
/// or its next auto-save would put the old one back.
#[tauri::command]
pub fn notepad_consent_message(app: AppHandle, message: String) -> Result<(), String> {
    let message = message.trim().to_string();
    let mut cfg = crate::config::load();
    if cfg.meeting_consent_message == message {
        return Ok(());
    }
    cfg.meeting_consent_message = message.clone();
    crate::config::save(&cfg)?;
    let _ = app.emit(EVENT_CONSENT, serde_json::json!({ "message": message }));
    Ok(())
}

// ---- Windows ---------------------------------------------------------------------------------

#[cfg(windows)]
mod platform {
    use std::ffi::c_void;

    use super::{notepad_width, outer_rect, pick_call_window, split_rects, Candidate, Rect};

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct WinRect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    struct MonitorInfo {
        size: u32,
        monitor: WinRect,
        work: WinRect,
        flags: u32,
    }

    const GWL_EXSTYLE: i32 = -20;
    const WS_EX_TOOLWINDOW: i32 = 0x80;
    const GW_OWNER: u32 = 4;
    const MONITOR_DEFAULTTONEAREST: u32 = 2;
    const SW_SHOWNOACTIVATE: i32 = 4;
    const SW_RESTORE: i32 = 9;
    const SWP_NOZORDER: u32 = 0x0004;
    const SWP_NOACTIVATE: u32 = 0x0010;
    const SWP_NOOWNERZORDER: u32 = 0x0200;
    const SWP_ASYNCWINDOWPOS: u32 = 0x4000;
    const DWMWA_EXTENDED_FRAME_BOUNDS: u32 = 9;
    const DWMWA_CLOAKED: u32 = 14;
    const MDT_EFFECTIVE_DPI: u32 = 0;

    /// A window or monitor handle, declared as the other modules declare it
    /// (`clashing_extern_declarations`); `Candidate` keeps windows as `isize`.
    type Handle = *mut c_void;

    fn handle(hwnd: isize) -> Handle {
        hwnd as Handle
    }

    #[link(name = "user32")]
    extern "system" {
        fn EnumWindows(callback: unsafe extern "system" fn(*mut c_void, isize) -> i32, lparam: isize) -> i32;
        fn IsWindowVisible(hwnd: *mut c_void) -> i32;
        fn IsIconic(hwnd: *mut c_void) -> i32;
        fn IsZoomed(hwnd: *mut c_void) -> i32;
        fn GetWindowThreadProcessId(hwnd: *mut c_void, pid: *mut u32) -> u32;
        fn GetWindowTextW(hwnd: *mut c_void, text: *mut u16, max: i32) -> i32;
        fn GetWindowRect(hwnd: *mut c_void, rect: *mut WinRect) -> i32;
        fn GetWindowLongW(hwnd: *mut c_void, index: i32) -> i32;
        fn GetWindow(hwnd: *mut c_void, cmd: u32) -> *mut c_void;
        fn ShowWindowAsync(hwnd: *mut c_void, cmd: i32) -> i32;
        fn SetWindowPos(
            hwnd: *mut c_void,
            insert_after: *mut c_void,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            flags: u32,
        ) -> i32;
        fn MonitorFromWindow(hwnd: *mut c_void, flags: u32) -> *mut c_void;
        fn MonitorFromPoint(point: Point, flags: u32) -> *mut c_void;
        fn GetMonitorInfoW(monitor: *mut c_void, info: *mut MonitorInfo) -> i32;
        fn GetCursorPos(point: *mut Point) -> i32;
    }
    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmGetWindowAttribute(hwnd: *mut c_void, attribute: u32, value: *mut c_void, size: u32) -> i32;
    }
    #[link(name = "shcore")]
    extern "system" {
        fn GetDpiForMonitor(monitor: *mut c_void, kind: u32, x: *mut u32, y: *mut u32) -> i32;
    }

    fn rect_of(r: WinRect) -> Rect {
        Rect {
            x: r.left,
            y: r.top,
            w: r.right - r.left,
            h: r.bottom - r.top,
        }
    }

    unsafe extern "system" fn collect(hwnd: *mut c_void, lparam: isize) -> i32 {
        let list = &mut *(lparam as *mut Vec<isize>);
        if IsWindowVisible(hwnd) != 0 {
            list.push(hwnd as isize);
        }
        1
    }

    /// The visible top-level windows of processes running one of `exes`,
    /// with what [`pick_call_window`] needs. Other apps' window titles are
    /// never read; none are logged or kept.
    fn candidates(exes: &[String]) -> Vec<Candidate> {
        let mut hwnds: Vec<isize> = Vec::new();
        unsafe {
            EnumWindows(collect, &mut hwnds as *mut Vec<isize> as isize);
        }
        let mut names: std::collections::HashMap<u32, String> = Default::default();
        hwnds
            .into_iter()
            .filter_map(|id| unsafe {
                let hwnd = handle(id);
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, &mut pid);
                let exe = names
                    .entry(pid)
                    .or_insert_with(|| crate::text_injector::app_name_for(Some(id)).unwrap_or_default())
                    .clone();
                if !exes.iter().any(|e| e.eq_ignore_ascii_case(&exe)) {
                    return None;
                }
                let mut buf = [0u16; 256];
                let len = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
                let title = String::from_utf16_lossy(&buf[..len.max(0) as usize]);
                let mut r = WinRect::default();
                if GetWindowRect(hwnd, &mut r) == 0 {
                    return None;
                }
                let mut cloaked = 0u32;
                DwmGetWindowAttribute(
                    hwnd,
                    DWMWA_CLOAKED,
                    &mut cloaked as *mut u32 as *mut c_void,
                    std::mem::size_of::<u32>() as u32,
                );
                Some(Candidate {
                    hwnd: id,
                    pid,
                    exe,
                    title,
                    rect: rect_of(r),
                    minimized: IsIconic(hwnd) != 0,
                    tool: GetWindowLongW(hwnd, GWL_EXSTYLE) & WS_EX_TOOLWINDOW != 0,
                    owned: !GetWindow(hwnd, GW_OWNER).is_null(),
                    cloaked: cloaked != 0,
                })
            })
            .collect()
    }

    /// The live call's main window (call detection's latest call), if any.
    pub fn call_window() -> Option<isize> {
        let (app_id, exes) = crate::meeting_detect::call_window_exes()?;
        let hwnd = pick_call_window(&candidates(&exes), &exes, std::process::id(), |title| {
            crate::meeting_detect::title_shows(app_id, title)
        });
        tracing::info!(app = app_id, found = hwnd.is_some(), "notepad: the call's window");
        hwnd
    }

    /// Where the live call's window would go when the screen is split (its
    /// slot left of the notepad, physical px), and that monitor's DPI scale.
    pub fn call_slot() -> Option<(Rect, f64)> {
        let call = call_window()?;
        let (work, scale) = work_area(monitor_for(Some(call)))?;
        Some((split_rects(work, notepad_width(work.w, scale)).0, scale))
    }

    /// The work area and DPI scale of `monitor`.
    fn work_area(monitor: Handle) -> Option<(Rect, f64)> {
        let mut info = MonitorInfo {
            size: std::mem::size_of::<MonitorInfo>() as u32,
            monitor: WinRect::default(),
            work: WinRect::default(),
            flags: 0,
        };
        if monitor.is_null() || unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
            return None;
        }
        let (mut dx, mut dy) = (96u32, 96u32);
        let scale = if unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) } == 0 {
            f64::from(dx) / 96.0
        } else {
            1.0
        };
        Some((rect_of(info.work), scale))
    }

    /// The monitor with `hwnd`, else the one with the mouse cursor.
    fn monitor_for(hwnd: Option<isize>) -> Handle {
        if let Some(h) = hwnd {
            return unsafe { MonitorFromWindow(handle(h), MONITOR_DEFAULTTONEAREST) };
        }
        let mut p = Point::default();
        unsafe {
            GetCursorPos(&mut p);
            MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST)
        }
    }

    /// How far `hwnd`'s window rect reaches past its visible frame (left,
    /// top, right, bottom): the invisible resize borders.
    fn frame_margins(hwnd: Handle) -> (i32, i32, i32, i32) {
        let (mut outer, mut frame) = (WinRect::default(), WinRect::default());
        let ok = unsafe {
            GetWindowRect(hwnd, &mut outer) != 0
                && DwmGetWindowAttribute(
                    hwnd,
                    DWMWA_EXTENDED_FRAME_BOUNDS,
                    &mut frame as *mut WinRect as *mut c_void,
                    std::mem::size_of::<WinRect>() as u32,
                ) == 0
        };
        if !ok || frame.right <= frame.left {
            return (0, 0, 0, 0);
        }
        let m = (
            frame.left - outer.left,
            frame.top - outer.top,
            outer.right - frame.right,
            outer.bottom - frame.bottom,
        );
        // Anything bigger isn't a border (a window mid-animation).
        if [m.0, m.1, m.2, m.3].iter().all(|v| (0..=32).contains(v)) {
            m
        } else {
            (0, 0, 0, 0)
        }
    }

    fn move_to(hwnd: Handle, target: Rect, flags: u32) -> bool {
        let r = outer_rect(target, frame_margins(hwnd));
        unsafe { SetWindowPos(hwnd, std::ptr::null_mut(), r.x, r.y, r.w, r.h, flags) != 0 }
    }

    /// Dock the notepad `w` to the right edge of the work area of the call's
    /// monitor (or the cursor's); with `split`, move the call's window to the
    /// rest of it. Returns whether the notepad was placed.
    pub fn place_docked(w: &tauri::WebviewWindow, call: Option<isize>, split: bool) -> bool {
        let Ok(hwnd) = w.hwnd() else { return false };
        let own: Handle = hwnd.0;
        let Some((work, scale)) = work_area(monitor_for(call)) else {
            return false;
        };
        let width = notepad_width(work.w, scale);
        let (call_rect, pad) = split_rects(work, width);
        let flags = SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE;
        // Twice: moving to a monitor with another DPI resizes the window on
        // the way (WM_DPICHANGED); the second call sets the size it should be.
        let placed = move_to(own, pad, flags) && move_to(own, pad, flags);
        if let (true, Some(call)) = (split, call.map(handle)) {
            unsafe {
                if IsZoomed(call) != 0 {
                    // A maximised window ignores a new size: restore it first.
                    ShowWindowAsync(call, SW_RESTORE);
                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
            }
            // Another app's window: asynchronous, so a hung app can't hang Yap.
            let moved = move_to(call, call_rect, flags | SWP_ASYNCWINDOWPOS);
            tracing::info!(moved, "notepad: split the screen with the call");
        }
        placed
    }

    /// Un-minimise without activating (a meeting starting must not take the
    /// focus from the call).
    pub fn restore_quietly(w: &tauri::WebviewWindow) {
        if let Ok(hwnd) = w.hwnd() {
            unsafe {
                ShowWindowAsync(hwnd.0, SW_SHOWNOACTIVATE);
            }
        }
    }
}

#[cfg(not(windows))]
mod platform {
    /// No call windows off Windows (call detection is Windows-only).
    pub fn call_window() -> Option<isize> {
        None
    }

    pub fn call_slot() -> Option<(super::Rect, f64)> {
        None
    }

    /// Dock to the right edge of the primary monitor's work area.
    pub fn place_docked(w: &tauri::WebviewWindow, _call: Option<isize>, _split: bool) -> bool {
        let Ok(Some(m)) = w.primary_monitor() else { return false };
        let a = m.work_area();
        let work = super::Rect {
            x: a.position.x,
            y: a.position.y,
            w: a.size.width as i32,
            h: a.size.height as i32,
        };
        let r = super::dock_rect(work, super::notepad_width(work.w, m.scale_factor()));
        let _ = w.set_position(tauri::PhysicalPosition::new(r.x, r.y));
        let _ = w.set_size(tauri::PhysicalSize::new(r.w as u32, r.h as u32));
        true
    }

    pub fn restore_quietly(w: &tauri::WebviewWindow) {
        let _ = w.unminimize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect { x: 0, y: 0, w: 1920, h: 1032 };

    #[test]
    fn about_a_third_of_the_screen_within_bounds() {
        assert_eq!(notepad_width(1920, 1.0), 576);
        // Wispr Flow's, measured on a 2560 × 1440 screen.
        assert_eq!(notepad_width(2560, 1.0), 768);
        // Small screens: the minimum, but never more than half.
        assert_eq!(notepad_width(1366, 1.0), 410);
        assert_eq!(notepad_width(1024, 1.0), 400);
        assert_eq!(notepad_width(700, 1.0), 350);
        // Wide or high-DPI screens: the maximum, scaled.
        assert_eq!(notepad_width(3440, 1.0), 800);
        assert_eq!(notepad_width(3840, 1.5), 1152);
        assert_eq!(notepad_width(5120, 1.25), 1000);
        assert_eq!(notepad_width(1920, f64::NAN), 576);
    }

    #[test]
    fn the_preview_frames_the_calls_slot_with_room_for_its_shadow() {
        let (call, _) = split_rects(WORK, notepad_width(WORK.w, 1.0));
        let frame = preview_rect(call, 1.0);
        assert_eq!(frame, Rect { x: -24, y: -24, w: 1344 + 48, h: 1032 + 48 });
        // Scaled for the monitor's DPI.
        assert_eq!(preview_rect(call, 1.5).x, -36);
    }

    #[test]
    fn docked_right_and_split_side_by_side() {
        let pad = dock_rect(WORK, 576);
        assert_eq!(pad, Rect { x: 1344, y: 0, w: 576, h: 1032 });
        // A second monitor left of the primary, taskbar on top.
        let left = Rect { x: -2560, y: 48, w: 2560, h: 1392 };
        assert_eq!(dock_rect(left, 750), Rect { x: -750, y: 48, w: 750, h: 1392 });
        let (call, pad) = split_rects(WORK, 576);
        assert_eq!(call, Rect { x: 0, y: 0, w: 1344, h: 1032 });
        assert_eq!(call.x + call.w, pad.x); // touching, no gap
        assert_eq!(pad.x + pad.w, WORK.w);
    }

    #[test]
    fn invisible_borders_are_added_back() {
        let target = Rect { x: 0, y: 0, w: 1344, h: 1032 };
        // A normal window: 7 px of invisible border left, right and bottom.
        assert_eq!(
            outer_rect(target, (7, 0, 7, 7)),
            Rect { x: -7, y: 0, w: 1358, h: 1039 }
        );
        assert_eq!(outer_rect(target, (0, 0, 0, 0)), target);
    }

    fn win(hwnd: isize, pid: u32, exe: &str, title: &str, w: i32, h: i32) -> Candidate {
        Candidate {
            hwnd,
            pid,
            exe: exe.to_string(),
            title: title.to_string(),
            rect: Rect { x: 0, y: 0, w, h },
            minimized: false,
            tool: false,
            owned: false,
            cloaked: false,
        }
    }

    #[test]
    fn picks_the_call_apps_largest_real_window() {
        let teams = vec!["ms-teams.exe".to_string(), "teams.exe".to_string()];
        let windows = vec![
            win(1, 10, "MS-Teams.exe", "Chat | Microsoft Teams", 900, 700),
            win(2, 10, "ms-teams.exe", "Meeting with Priya | Microsoft Teams", 1400, 900),
            win(3, 10, "ms-teams.exe", "", 1920, 1080), // untitled helper
            win(4, 20, "chrome.exe", "Inbox", 1920, 1040),
            win(5, 99, "yap.exe", "Yap", 1920, 1080), // Yap's own
        ];
        let any = |_: &str| false;
        assert_eq!(pick_call_window(&windows, &teams, 99, any), Some(2));
        // Never Yap's own windows, even when it matches the exes.
        let yap = vec!["yap.exe".to_string()];
        assert_eq!(pick_call_window(&windows, &yap, 99, any), None);
        // Minimised, tool, owned, cloaked or tiny windows don't count.
        let mut hidden = windows.clone();
        hidden[1].minimized = true;
        hidden[0].tool = true;
        assert_eq!(pick_call_window(&hidden, &teams, 99, any), None);
        let mut small = windows.clone();
        small[1].rect = Rect { x: 0, y: 0, w: 180, h: 120 };
        assert_eq!(pick_call_window(&small, &teams, 99, any), Some(1));
        let mut cloaked = windows.clone();
        cloaked[1].cloaked = true;
        cloaked[0].owned = true;
        assert_eq!(pick_call_window(&cloaked, &teams, 99, any), None);
    }

    #[test]
    fn a_browser_call_prefers_the_window_that_shows_it() {
        let browser = vec!["chrome.exe".to_string()];
        let windows = vec![
            win(1, 20, "chrome.exe", "Inbox - Gmail", 1920, 1040),
            win(2, 20, "chrome.exe", "Meet - abc-defg-hij", 1200, 800),
        ];
        let meet = |t: &str| t.to_lowercase().starts_with("meet - ");
        assert_eq!(pick_call_window(&windows, &browser, 99, meet), Some(2));
        // No window shows it (another tab is in front): the largest.
        assert_eq!(pick_call_window(&windows, &browser, 99, |_| false), Some(1));
        assert_eq!(pick_call_window(&[], &browser, 99, meet), None);
    }
}

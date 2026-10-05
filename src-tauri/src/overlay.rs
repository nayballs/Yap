//! The `overlay` window — since 2026-10-05 the **Yap bar** (its behaviour is
//! in `bar.rs`): one fixed-size, transparent, always-on-top tool window that
//! never takes focus and lets clicks through, except while the cursor is on
//! something the page draws to be clicked (Wispr Flow's Flow Bar: a 512×614
//! "Status" window, WS_EX_TRANSPARENT cleared only while the pointer is on
//! the pill or a card). The label stays `overlay`: `yap-state` routing and
//! the meeting recorder's capture-hiding go by it.
//!
//! This module is the window plumbing and the pure maths behind it, all
//! unit-tested: where the bar sits on a monitor ([`place`]), whether the app
//! in front is fullscreen ([`fullscreen_on`]), which part of the page the
//! cursor is on ([`region_at`]), and the ex-styles the window always keeps
//! ([`enforced_ex_style`]). Coordinates are physical pixels: Tauri runs
//! per-monitor DPI aware, so `GetCursorPos`, monitor rects and window rects
//! share one space.

use serde::{Deserialize, Serialize};

/// The window's label in `tauri.conf.json`.
pub const LABEL: &str = "overlay";
/// The bar window's size in logical px (must match `tauri.conf.json`): room
/// for the pill, a tooltip or menu above it and two cards, with space inside
/// for soft shadows (a shadow reaching a transparent window's edge draws a
/// grey box).
pub const WIDTH: f64 = 520.0;
pub const HEIGHT: f64 = 560.0;

/// A rectangle in physical screen px (right/bottom exclusive, like Win32).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub const fn new(left: i32, top: i32, right: i32, bottom: i32) -> Self {
        Self { left, top, right, bottom }
    }
    pub fn width(&self) -> i32 {
        self.right - self.left
    }
    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
    /// Whether this rect reaches every edge of `other`.
    pub fn covers(&self, other: &Rect) -> bool {
        self.left <= other.left
            && self.top <= other.top
            && self.right >= other.right
            && self.bottom >= other.bottom
    }
    pub fn inflate(&self, by: i32) -> Rect {
        Rect::new(self.left - by, self.top - by, self.right + by, self.bottom + by)
    }
}

/// A monitor as the bar sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screen {
    /// The `HMONITOR` (stable while the monitor stays connected).
    pub id: isize,
    pub monitor: Rect,
    /// The monitor minus docked taskbars and app bars.
    pub work: Rect,
    /// DPI / 96.
    pub scale: f64,
    /// An auto-hidden taskbar on the bottom (top) edge reserves no work area
    /// but pops up over it: its thickness, kept clear (0 without one).
    pub autohide_bottom: i32,
    pub autohide_top: i32,
}

/// Which screen edge the bar sits on (`overlay_position`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Bottom,
    Top,
}

impl Edge {
    pub fn from_config(position: &str) -> Edge {
        if position == "top" {
            Edge::Top
        } else {
            Edge::Bottom
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Edge::Bottom => "bottom",
            Edge::Top => "top",
        }
    }
}

/// The bar window's rect on `screen` for a window of `size` (physical px):
/// centred on the work area, its bottom edge on the work area's bottom —
/// above a docked taskbar, flush with a monitor that has none (Wispr's Flow
/// Bar: 50 px up on the monitor with the taskbar) — or its top on the work
/// area's top for [`Edge::Top`]. An auto-hidden taskbar on that edge is kept
/// clear, so it can pop up without covering the bar.
pub fn place(screen: &Screen, size: (i32, i32), edge: Edge) -> Rect {
    let (w, h) = size;
    let left = screen.work.left + (screen.work.width() - w) / 2;
    let top = match edge {
        Edge::Bottom => screen.work.bottom - screen.autohide_bottom - h,
        Edge::Top => screen.work.top + screen.autohide_top,
    };
    Rect::new(left, top, left + w, top + h)
}

// ---- fullscreen ------------------------------------------------------------------

/// The window in front, as the fullscreen check needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct Front {
    pub rect: Rect,
    pub class: String,
    /// Maximised: it fills the work area (or the whole monitor when the
    /// taskbar is elsewhere or auto-hidden), but that's not fullscreen.
    pub zoomed: bool,
    /// One of Yap's own windows.
    pub ours: bool,
    /// The monitor most of it is on.
    pub monitor: isize,
}

/// Shell windows that can cover a monitor without being an app in
/// fullscreen: the desktop (Progman, WorkerW), the taskbars, and Windows
/// 11's Task View / Alt+Tab and Start hosts (which would otherwise hide the
/// bar for a blink on every Alt+Tab).
const SHELL_CLASSES: &[&str] = &[
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "XamlExplorerHostIslandWindow",
    "Windows.UI.Core.CoreWindow",
];

/// `SHQueryUserNotificationState`: an exclusive-mode Direct3D app is
/// running (a game) — fullscreen on every monitor as far as the bar cares.
pub const QUNS_RUNNING_D3D_FULL_SCREEN: i32 = 3;
/// …or Windows' presentation mode is on (a slideshow).
pub const QUNS_PRESENTATION_MODE: i32 = 4;

/// What's fullscreen on the bar's monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fullscreen {
    #[default]
    None,
    /// A window covering the whole monitor: a borderless game, a video, a
    /// browser in F11. The idle pill hides, but a card can still show over
    /// it (Wispr's "Meeting detected" card appears over a game).
    Borderless,
    /// An exclusive-mode Direct3D app or presentation mode: nothing shows
    /// over it (a topmost window would knock a game out of exclusive mode,
    /// or land on a projected slideshow); cards wait until it's over.
    Exclusive,
}

impl Fullscreen {
    pub fn as_str(self) -> &'static str {
        match self {
            Fullscreen::None => "none",
            Fullscreen::Borderless => "borderless",
            Fullscreen::Exclusive => "exclusive",
        }
    }
    pub fn from_name(name: &str) -> Fullscreen {
        match name {
            "borderless" => Fullscreen::Borderless,
            "exclusive" => Fullscreen::Exclusive,
            _ => Fullscreen::None,
        }
    }
}

/// What's fullscreen on `screen`'s monitor? Exclusive: an exclusive-mode
/// Direct3D app or presentation mode, anywhere (`quns`, from
/// `SHQueryUserNotificationState`, which can't say which monitor).
/// Borderless: a window in front that covers the whole monitor (not just its
/// work area) and isn't maximised, Yap's own or part of the shell. A
/// fullscreen window on another monitor leaves the bar alone.
pub fn fullscreen_on(front: Option<&Front>, quns: i32, screen: &Screen) -> Fullscreen {
    if quns == QUNS_RUNNING_D3D_FULL_SCREEN || quns == QUNS_PRESENTATION_MODE {
        return Fullscreen::Exclusive;
    }
    let Some(front) = front else { return Fullscreen::None };
    if front.ours || front.zoomed || SHELL_CLASSES.contains(&front.class.as_str()) {
        return Fullscreen::None;
    }
    if front.monitor == screen.id && front.rect.covers(&screen.monitor) {
        Fullscreen::Borderless
    } else {
        Fullscreen::None
    }
}

// ---- the pointer -------------------------------------------------------------------

/// A part of the page that takes the pointer — the pill, a card, the menu —
/// as the page reports it: CSS px, relative to the window's top-left.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// The region under the cursor (physical screen px) for a window whose
/// top-left is `origin` (physical) at `scale`. `slack` (CSS px) widens every
/// region — used while the pointer is already on one, so its edge doesn't
/// flicker between clickable and click-through.
pub fn region_at(
    regions: &[Region],
    cursor: (i32, i32),
    origin: (i32, i32),
    scale: f64,
    slack: f64,
) -> Option<&Region> {
    if scale <= 0.0 {
        return None;
    }
    let x = f64::from(cursor.0 - origin.0) / scale;
    let y = f64::from(cursor.1 - origin.1) / scale;
    regions.iter().find(|r| {
        x >= r.x - slack && x < r.x + r.w + slack && y >= r.y - slack && y < r.y + r.h + slack
    })
}

// ---- ex-styles ---------------------------------------------------------------------

pub const WS_EX_TOPMOST: u32 = 0x0000_0008;
pub const WS_EX_TRANSPARENT: u32 = 0x0000_0020;
pub const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
pub const WS_EX_APPWINDOW: u32 = 0x0004_0000;
pub const WS_EX_LAYERED: u32 = 0x0008_0000;
pub const WS_EX_NOACTIVATE: u32 = 0x0800_0000;

/// The ex-style the bar window keeps whatever else writes it (tao rewrites
/// the whole ex-style on every show, hide and flag change): a tool window
/// (never in the taskbar or Alt+Tab), never activated by a click, layered,
/// and click-through (`WS_EX_TRANSPARENT`) unless the cursor is on the pill
/// or a card. Applied from `WM_STYLECHANGING` (see `win::install`).
pub fn enforced_ex_style(style: u32, click_through: bool) -> u32 {
    let style = (style | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED) & !WS_EX_APPWINDOW;
    if click_through {
        style | WS_EX_TRANSPARENT
    } else {
        style & !WS_EX_TRANSPARENT
    }
}

/// Force a window to the very top of the native Z-order: the raw Win32
/// `SetWindowPos(HWND_TOPMOST, …)` (no move/resize, no focus steal), more
/// reliable than Tauri's `set_always_on_top`, which another topmost window
/// can override. (Approach borrowed from Handy.) Off Windows this falls back
/// to Tauri's wrapper.
pub fn force_topmost(window: &tauri::WebviewWindow) {
    #[cfg(windows)]
    if let Ok(hwnd) = window.hwnd() {
        win::raise(hwnd.0);
    }
    #[cfg(not(windows))]
    let _ = window.set_always_on_top(true);
}

// ---- Win32 ---------------------------------------------------------------------------

/// The bar window's Win32 side: ex-style enforcement and click-activation
/// refusal (a window subclass), click-through toggling, placement, and the
/// queries the bar polls (cursor, monitors, the window in front).
#[cfg(windows)]
#[allow(clippy::upper_case_acronyms)] // Win32 FFI names
pub mod win {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

    use super::{Front, Rect, Screen};

    pub type HWND = *mut c_void;

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct POINT {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct RECT {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[repr(C)]
    struct MONITORINFO {
        cb_size: u32,
        rc_monitor: RECT,
        rc_work: RECT,
        flags: u32,
    }

    #[repr(C)]
    struct STYLESTRUCT {
        style_old: u32,
        style_new: u32,
    }

    #[repr(C)]
    struct APPBARDATA {
        cb_size: u32,
        hwnd: HWND,
        callback_message: u32,
        edge: u32,
        rc: RECT,
        lparam: isize,
    }

    type SubclassProc = unsafe extern "system" fn(HWND, u32, usize, isize, usize, usize) -> isize;

    #[link(name = "user32")]
    extern "system" {
        fn GetCursorPos(point: *mut POINT) -> i32;
        fn MonitorFromPoint(point: POINT, flags: u32) -> isize;
        fn MonitorFromWindow(hwnd: HWND, flags: u32) -> isize;
        fn GetMonitorInfoW(monitor: isize, info: *mut MONITORINFO) -> i32;
        fn GetForegroundWindow() -> HWND;
        fn SetForegroundWindow(hwnd: HWND) -> i32;
        fn GetWindowRect(hwnd: HWND, rect: *mut RECT) -> i32;
        fn GetClassNameW(hwnd: HWND, buf: *mut u16, max: i32) -> i32;
        fn IsZoomed(hwnd: HWND) -> i32;
        fn IsWindow(hwnd: HWND) -> i32;
        fn GetWindowThreadProcessId(hwnd: HWND, pid: *mut u32) -> u32;
        fn GetWindowLongPtrW(hwnd: HWND, index: i32) -> isize;
        fn SetWindowLongPtrW(hwnd: HWND, index: i32, value: isize) -> isize;
        fn SetWindowPos(hwnd: HWND, after: HWND, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
        fn GetDpiForWindow(hwnd: HWND) -> u32;
        fn WindowFromPoint(point: POINT) -> HWND;
        fn GetAncestor(hwnd: HWND, flags: u32) -> HWND;
        fn AttachThreadInput(attach: u32, to: u32, on: i32) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcessId() -> u32;
        fn GetCurrentThreadId() -> u32;
    }
    #[link(name = "shcore")]
    extern "system" {
        fn GetDpiForMonitor(monitor: isize, kind: i32, dpi_x: *mut u32, dpi_y: *mut u32) -> i32;
    }
    #[link(name = "shell32")]
    extern "system" {
        fn SHQueryUserNotificationState(state: *mut i32) -> i32;
        fn SHAppBarMessage(message: u32, data: *mut APPBARDATA) -> usize;
    }
    #[link(name = "comctl32")]
    extern "system" {
        fn SetWindowSubclass(hwnd: HWND, proc_: SubclassProc, id: usize, data: usize) -> i32;
        fn DefSubclassProc(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> isize;
        fn RemoveWindowSubclass(hwnd: HWND, proc_: SubclassProc, id: usize) -> i32;
    }

    const GWL_EXSTYLE: i32 = -20;
    const MONITOR_DEFAULTTONULL: u32 = 0;
    const MONITOR_DEFAULTTONEAREST: u32 = 2;
    const MDT_EFFECTIVE_DPI: i32 = 0;
    const ABM_GETAUTOHIDEBAREX: u32 = 0x0000_000b;
    const ABE_TOP: u32 = 1;
    const ABE_BOTTOM: u32 = 3;
    const GA_ROOT: u32 = 2;
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOMOVE: u32 = 0x0002;
    const SWP_NOZORDER: u32 = 0x0004;
    const SWP_NOACTIVATE: u32 = 0x0010;
    const SWP_SHOWWINDOW: u32 = 0x0040;
    const WM_ACTIVATE: u32 = 0x0006;
    const WM_STYLECHANGING: u32 = 0x007C;
    const WM_NCDESTROY: u32 = 0x0082;
    const WM_MOUSEACTIVATE: u32 = 0x0021;
    const MA_NOACTIVATE: isize = 3;
    const WA_INACTIVE: usize = 0;
    const SUBCLASS_ID: usize = 0x59_41_50; // "YAP"

    /// Click-through right now: what `WM_STYLECHANGING` enforces.
    static CLICK_THROUGH: AtomicBool = AtomicBool::new(true);
    /// The bar window, once [`install`]ed (0 before).
    static HWND_BAR: AtomicIsize = AtomicIsize::new(0);
    /// Called when Windows activated the bar after all (`WM_ACTIVATE`).
    static ON_ACTIVATED: std::sync::OnceLock<fn()> = std::sync::OnceLock::new();

    fn rect(r: RECT) -> Rect {
        Rect::new(r.left, r.top, r.right, r.bottom)
    }

    unsafe extern "system" fn subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: usize,
        lparam: isize,
        _id: usize,
        _data: usize,
    ) -> isize {
        match msg {
            // A click never activates the bar (the app you're typing in
            // keeps the focus, so a dictation pastes into it).
            WM_MOUSEACTIVATE => return MA_NOACTIVATE,
            // Whatever writes the ex-style (tao rewrites all of it on every
            // show/hide), the bar keeps its own bits.
            WM_STYLECHANGING if wparam as i32 == GWL_EXSTYLE && lparam != 0 => {
                let styles = &mut *(lparam as *mut STYLESTRUCT);
                styles.style_new =
                    super::enforced_ex_style(styles.style_new, CLICK_THROUGH.load(Ordering::Relaxed));
            }
            // Activated anyway (WebView2 focusing itself): hand the focus back.
            WM_ACTIVATE if wparam & 0xFFFF != WA_INACTIVE => {
                if let Some(f) = ON_ACTIVATED.get() {
                    f();
                }
            }
            WM_NCDESTROY => {
                RemoveWindowSubclass(hwnd, subclass_proc, SUBCLASS_ID);
                HWND_BAR.store(0, Ordering::Relaxed);
            }
            _ => {}
        }
        DefSubclassProc(hwnd, msg, wparam, lparam)
    }

    /// Make `hwnd` the bar window: subclass it (ex-style enforcement, no
    /// click activation) and apply its ex-style now. Call on the window's
    /// thread (the main thread), before it's first shown.
    pub fn install(hwnd: HWND, on_activated: fn()) {
        let _ = ON_ACTIVATED.set(on_activated);
        HWND_BAR.store(hwnd as isize, Ordering::Relaxed);
        unsafe {
            SetWindowSubclass(hwnd, subclass_proc, SUBCLASS_ID, 0);
        }
        set_click_through(hwnd, true);
    }

    pub fn bar_hwnd() -> Option<HWND> {
        match HWND_BAR.load(Ordering::Relaxed) {
            0 => None,
            h => Some(h as HWND),
        }
    }

    /// Clickable or click-through. (Call on the main thread: the style
    /// change is a message to the window.)
    pub fn set_click_through(hwnd: HWND, on: bool) {
        CLICK_THROUGH.store(on, Ordering::Relaxed);
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, super::enforced_ex_style(style, on) as isize);
        }
    }

    pub fn ex_style(hwnd: HWND) -> u32 {
        unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 }
    }

    pub fn window_rect(hwnd: HWND) -> Option<Rect> {
        let mut r = RECT::default();
        (unsafe { GetWindowRect(hwnd, &mut r) } != 0).then(|| rect(r))
    }

    /// The window's scale (its DPI / 96).
    pub fn window_scale(hwnd: HWND) -> f64 {
        match unsafe { GetDpiForWindow(hwnd) } {
            0 => 1.0,
            dpi => f64::from(dpi) / 96.0,
        }
    }

    /// Move the window to `left`/`top` (no resize, no activation, no Z-order
    /// change). Call on the main thread.
    pub fn move_to(hwnd: HWND, left: i32, top: i32) {
        unsafe {
            SetWindowPos(hwnd, std::ptr::null_mut(), left, top, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
    }

    /// Top of the always-on-top band, shown, never activated.
    pub fn raise(hwnd: HWND) {
        let topmost = -1isize as HWND; // HWND_TOPMOST
        unsafe {
            SetWindowPos(hwnd, topmost, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
    }

    pub fn cursor() -> Option<(i32, i32)> {
        let mut p = POINT::default();
        (unsafe { GetCursorPos(&mut p) } != 0).then_some((p.x, p.y))
    }

    /// The monitor `id`, re-read (`None` once it's unplugged).
    pub fn screen(id: isize, autohide: bool) -> Option<Screen> {
        let mut info = MONITORINFO {
            cb_size: std::mem::size_of::<MONITORINFO>() as u32,
            rc_monitor: RECT::default(),
            rc_work: RECT::default(),
            flags: 0,
        };
        if id == 0 || unsafe { GetMonitorInfoW(id, &mut info) } == 0 {
            return None;
        }
        let (mut dx, mut dy) = (96u32, 96u32);
        let dpi = if unsafe { GetDpiForMonitor(id, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) } == 0 {
            dx
        } else {
            96
        };
        let monitor = rect(info.rc_monitor);
        let (autohide_bottom, autohide_top) = if autohide {
            (autohide_bar(info.rc_monitor, ABE_BOTTOM), autohide_bar(info.rc_monitor, ABE_TOP))
        } else {
            (0, 0)
        };
        Some(Screen {
            id,
            monitor,
            work: rect(info.rc_work),
            scale: f64::from(dpi) / 96.0,
            autohide_bottom,
            autohide_top,
        })
    }

    /// The monitor holding (or nearest to) a point.
    pub fn screen_at(point: (i32, i32), autohide: bool) -> Option<Screen> {
        let id = unsafe { MonitorFromPoint(POINT { x: point.0, y: point.1 }, MONITOR_DEFAULTTONEAREST) };
        screen(id, autohide)
    }

    /// The thickness of an auto-hidden taskbar on `edge` of the monitor at
    /// `monitor` (0 without one). A message to the shell: called only when
    /// the bar is placed, not on every poll.
    fn autohide_bar(monitor: RECT, edge: u32) -> i32 {
        let mut data = APPBARDATA {
            cb_size: std::mem::size_of::<APPBARDATA>() as u32,
            hwnd: std::ptr::null_mut(),
            callback_message: 0,
            edge,
            rc: monitor,
            lparam: 0,
        };
        let bar = unsafe { SHAppBarMessage(ABM_GETAUTOHIDEBAREX, &mut data) } as HWND;
        if bar.is_null() {
            return 0;
        }
        window_rect(bar).map_or(0, |r| r.height().clamp(0, 200))
    }

    fn ours(hwnd: HWND) -> bool {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        pid != 0 && pid == unsafe { GetCurrentProcessId() }
    }

    /// The window in front (for the fullscreen check).
    pub fn front() -> Option<Front> {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_null() {
            return None;
        }
        let mut buf = [0u16; 128];
        let len = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) }.max(0) as usize;
        Some(Front {
            rect: window_rect(hwnd)?,
            class: String::from_utf16_lossy(&buf[..len]),
            zoomed: unsafe { IsZoomed(hwnd) } != 0,
            ours: ours(hwnd),
            monitor: unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONULL) },
        })
    }

    /// `SHQueryUserNotificationState` (0 if it fails).
    pub fn notification_state() -> i32 {
        let mut state = 0i32;
        if unsafe { SHQueryUserNotificationState(&mut state) } == 0 {
            state
        } else {
            0
        }
    }

    /// The foreground window when it isn't Yap's (0 otherwise).
    pub fn foreign_foreground() -> isize {
        let hwnd = unsafe { GetForegroundWindow() };
        if hwnd.is_null() || ours(hwnd) {
            0
        } else {
            hwnd as isize
        }
    }

    /// The bar window itself is in front (it got activated after all).
    pub fn bar_in_front() -> bool {
        let fg = unsafe { GetForegroundWindow() };
        !fg.is_null() && bar_hwnd() == Some(fg)
    }

    /// Give the foreground back to `hwnd` (the app the person was in). The
    /// usual input-attach dance, as `text_injector::focus_window`.
    pub fn give_back_focus(hwnd: isize) {
        let target = hwnd as HWND;
        if target.is_null() || unsafe { IsWindow(target) } == 0 {
            return;
        }
        unsafe {
            let cur = GetCurrentThreadId();
            let other = GetWindowThreadProcessId(target, std::ptr::null_mut());
            let attached = other != 0 && AttachThreadInput(cur, other, 1) != 0;
            SetForegroundWindow(target);
            if attached {
                AttachThreadInput(cur, other, 0);
            }
        }
    }

    #[repr(C)]
    struct GUITHREADINFO {
        cb_size: u32,
        flags: u32,
        active: HWND,
        focus: HWND,
        capture: HWND,
        menu_owner: HWND,
        move_size: HWND,
        caret: HWND,
        caret_rect: RECT,
    }

    #[link(name = "user32")]
    extern "system" {
        fn GetGUIThreadInfo(thread: u32, info: *mut GUITHREADINFO) -> i32;
    }

    /// The active window of `hwnd`'s thread (Yap's main thread for its
    /// windows), whether or not it's the foreground: 0 for none.
    pub fn thread_active(hwnd: HWND) -> isize {
        let thread = unsafe { GetWindowThreadProcessId(hwnd, std::ptr::null_mut()) };
        let mut info: GUITHREADINFO = unsafe { std::mem::zeroed() };
        info.cb_size = std::mem::size_of::<GUITHREADINFO>() as u32;
        if thread == 0 || unsafe { GetGUIThreadInfo(thread, &mut info) } == 0 {
            return 0;
        }
        info.active as isize
    }

    /// Which top-level window a click at `point` would reach (hit-testing,
    /// no input sent): the bar while it's clickable there, the window under
    /// it while it's click-through.
    pub fn window_at(point: (i32, i32)) -> isize {
        unsafe {
            let hwnd = WindowFromPoint(POINT { x: point.0, y: point.1 });
            if hwnd.is_null() {
                0
            } else {
                GetAncestor(hwnd, GA_ROOT) as isize
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2560×1440 at 100 % with a 48 px taskbar at the bottom.
    fn primary() -> Screen {
        Screen {
            id: 1,
            monitor: Rect::new(0, 0, 2560, 1440),
            work: Rect::new(0, 0, 2560, 1392),
            scale: 1.0,
            autohide_bottom: 0,
            autohide_top: 0,
        }
    }

    /// A second 2560×1440 monitor to the right, no taskbar (Wispr's trace:
    /// "2560,2,5120,1442").
    fn right() -> Screen {
        Screen {
            id: 2,
            monitor: Rect::new(2560, 2, 5120, 1442),
            work: Rect::new(2560, 2, 5120, 1442),
            scale: 1.0,
            autohide_bottom: 0,
            autohide_top: 0,
        }
    }

    #[test]
    fn sits_bottom_centre_on_the_work_area() {
        // Above the taskbar on the primary monitor…
        let r = place(&primary(), (460, 520), Edge::Bottom);
        assert_eq!(r, Rect::new(1050, 872, 1510, 1392));
        assert_eq!(r.left + r.width() / 2, 1280, "centred");
        // …flush with the bottom of a monitor without one, centred on it
        // (Wispr's Flow Bar: 3584..4096 on 2560..5120, bottom 1440).
        let r = place(&right(), (512, 614), Edge::Bottom);
        assert_eq!(r, Rect::new(3584, 828, 4096, 1442));
    }

    #[test]
    fn follows_the_taskbar_to_any_edge() {
        let mut s = primary();
        // Taskbar on the left: centred on what's left, on the monitor's bottom.
        s.work = Rect::new(62, 0, 2560, 1440);
        let r = place(&s, (460, 520), Edge::Bottom);
        assert_eq!((r.left, r.bottom), (62 + (2498 - 460) / 2, 1440));
        // Taskbar at the top: the bottom is free.
        s.work = Rect::new(0, 48, 2560, 1440);
        assert_eq!(place(&s, (460, 520), Edge::Bottom).bottom, 1440);
        // Top edge (`overlay_position: "top"`): just under that taskbar.
        assert_eq!(place(&s, (460, 520), Edge::Top).top, 48);
    }

    #[test]
    fn keeps_clear_of_an_auto_hidden_taskbar() {
        let mut s = primary();
        // Auto-hide reserves no work area, but the taskbar pops up 48 px.
        s.work = s.monitor;
        s.autohide_bottom = 48;
        assert_eq!(place(&s, (460, 520), Edge::Bottom).bottom, 1392);
        s.autohide_bottom = 0;
        s.autohide_top = 40;
        assert_eq!(place(&s, (460, 520), Edge::Top).top, 40);
    }

    #[test]
    fn scales_with_the_monitor() {
        // A 4K laptop panel at 150 %, left of the primary, taskbar 72 px:
        // the window is 780×840 there and still centred above the taskbar.
        let s = Screen {
            id: 3,
            monitor: Rect::new(-3840, 0, 0, 2160),
            work: Rect::new(-3840, 0, 0, 2088),
            scale: 1.5,
            autohide_bottom: 0,
            autohide_top: 0,
        };
        let size = ((WIDTH * s.scale) as i32, (HEIGHT * s.scale) as i32);
        assert_eq!(size, (780, 840));
        let r = place(&s, size, Edge::Bottom);
        assert_eq!(r, Rect::new(-2310, 1248, -1530, 2088));
        assert!(s.monitor.covers(&r));
    }

    #[test]
    fn a_bar_wider_than_a_tiny_screen_stays_centred() {
        let mut s = primary();
        s.work = Rect::new(0, 0, 400, 300);
        let r = place(&s, (460, 520), Edge::Bottom);
        assert_eq!((r.left, r.right, r.bottom), (-30, 430, 300));
    }

    fn front(rect: Rect, class: &str) -> Front {
        Front { rect, class: class.into(), zoomed: false, ours: false, monitor: 1 }
    }

    #[test]
    fn fullscreen_means_the_whole_monitor() {
        let s = primary();
        let on = |f: Front| fullscreen_on(Some(&f), 5, &s);
        // A borderless game or an F11 browser: exactly the monitor.
        assert_eq!(on(front(s.monitor, "UnityWndClass")), Fullscreen::Borderless);
        // Bigger than it is fine too.
        assert_eq!(on(front(s.monitor.inflate(2), "Chrome_WidgetWin_1")), Fullscreen::Borderless);
        // Covering only the work area is an ordinary big window.
        assert_eq!(on(front(s.work, "Notepad")), Fullscreen::None);
        // Nothing in front.
        assert_eq!(fullscreen_on(None, 5, &s), Fullscreen::None);
    }

    #[test]
    fn maximised_shell_and_own_windows_are_not_fullscreen() {
        let s = primary();
        // Maximised on a monitor with an auto-hidden taskbar: it covers the
        // monitor (plus its invisible borders), but it isn't fullscreen.
        let mut max = front(s.monitor.inflate(8), "Chrome_WidgetWin_1");
        max.zoomed = true;
        assert_eq!(fullscreen_on(Some(&max), 5, &s), Fullscreen::None);
        // The desktop, the taskbar, Task View / Alt+Tab.
        for class in ["Progman", "WorkerW", "Shell_TrayWnd", "XamlExplorerHostIslandWindow"] {
            assert_eq!(fullscreen_on(Some(&front(s.monitor, class)), 5, &s), Fullscreen::None, "{class}");
        }
        // Yap's own windows.
        let mut own = front(s.monitor, "Yap");
        own.ours = true;
        assert_eq!(fullscreen_on(Some(&own), 5, &s), Fullscreen::None);
    }

    #[test]
    fn fullscreen_on_another_monitor_leaves_the_bar_alone() {
        // A game fullscreen on the primary while the cursor (and the bar)
        // is on the right-hand monitor.
        let game = front(primary().monitor, "UnityWndClass");
        assert_eq!(fullscreen_on(Some(&game), 2, &primary()), Fullscreen::Borderless);
        assert_eq!(fullscreen_on(Some(&game), 2, &right()), Fullscreen::None);
        // …but exclusive-mode Direct3D or presentation mode count anywhere.
        assert_eq!(fullscreen_on(None, QUNS_RUNNING_D3D_FULL_SCREEN, &right()), Fullscreen::Exclusive);
        let notepad = front(right().work, "Notepad");
        assert_eq!(fullscreen_on(Some(&notepad), QUNS_PRESENTATION_MODE, &right()), Fullscreen::Exclusive);
        assert_eq!(Fullscreen::from_name(Fullscreen::Borderless.as_str()), Fullscreen::Borderless);
    }

    fn region(id: &str, x: f64, y: f64, w: f64, h: f64) -> Region {
        Region { id: id.into(), x, y, w, h }
    }

    #[test]
    fn finds_the_region_under_the_cursor() {
        let regions = vec![region("pill", 170.0, 470.0, 120.0, 44.0), region("card:call", 60.0, 280.0, 340.0, 170.0)];
        let origin = (1050, 872);
        // On the pill.
        assert_eq!(region_at(&regions, (1050 + 200, 872 + 490), origin, 1.0, 0.0).map(|r| r.id.as_str()), Some("pill"));
        // On the card.
        assert_eq!(region_at(&regions, (1050 + 100, 872 + 300), origin, 1.0, 0.0).map(|r| r.id.as_str()), Some("card:call"));
        // On the transparent rest of the window, or off it.
        assert!(region_at(&regions, (1050 + 20, 872 + 20), origin, 1.0, 0.0).is_none());
        assert!(region_at(&regions, (10, 10), origin, 1.0, 0.0).is_none());
        // Right edges are exclusive.
        assert!(region_at(&regions, (1050 + 290, 872 + 490), origin, 1.0, 0.0).is_none());
    }

    #[test]
    fn hit_testing_scales_css_px_to_the_monitor() {
        // At 150 % a CSS px is 1.5 physical px.
        let regions = vec![region("pill", 100.0, 100.0, 40.0, 20.0)];
        assert!(region_at(&regions, (1000 + 150, 2000 + 150), (1000, 2000), 1.5, 0.0).is_some());
        assert!(region_at(&regions, (1000 + 140, 2000 + 140), (1000, 2000), 1.5, 0.0).is_none());
        assert!(region_at(&regions, (1000 + 209, 2000 + 179), (1000, 2000), 1.5, 0.0).is_some());
        assert!(region_at(&regions, (1000 + 211, 2000 + 179), (1000, 2000), 1.5, 0.0).is_none());
    }

    #[test]
    fn slack_keeps_the_pointer_on_a_region_near_its_edge() {
        let regions = vec![region("pill", 100.0, 100.0, 40.0, 20.0)];
        let just_outside = (144, 110);
        assert!(region_at(&regions, just_outside, (0, 0), 1.0, 0.0).is_none());
        assert!(region_at(&regions, just_outside, (0, 0), 1.0, 6.0).is_some());
        assert!(region_at(&regions, (150, 110), (0, 0), 1.0, 6.0).is_none());
    }

    #[test]
    fn the_window_keeps_its_ex_style() {
        // What tao writes on a show (topmost, layered + transparent from its
        // ignore-cursor flag, no-activate from `focusable: false`)…
        let tao = WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE;
        // …gains the tool-window bit and keeps the bar's click-through state.
        let clickable = enforced_ex_style(tao, false);
        assert_eq!(clickable & WS_EX_TRANSPARENT, 0);
        assert_ne!(clickable & WS_EX_TOOLWINDOW, 0);
        assert_ne!(clickable & WS_EX_NOACTIVATE, 0);
        assert_ne!(clickable & WS_EX_LAYERED, 0, "stays layered, as Electron's bar does");
        assert_ne!(clickable & WS_EX_TOPMOST, 0);
        let through = enforced_ex_style(WS_EX_APPWINDOW, true);
        assert_ne!(through & WS_EX_TRANSPARENT, 0);
        assert_eq!(through & WS_EX_APPWINDOW, 0, "never a taskbar button");
        let kept = WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED;
        assert_eq!(through & kept, kept);
    }
}

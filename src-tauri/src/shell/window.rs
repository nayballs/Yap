//! Yap's own windows: the main window and onboarding shown (fitted to
//! the screen, focused except in test runs) by the tray, the pages and
//! the first-run check; what the window-state plugin remembers; and the
//! hidden webviews' DWM-cloaked first show at start.

use std::sync::atomic::{AtomicU64, Ordering};

use tauri::{AppHandle, Manager};

/// When one of Yap's windows was last brought to the front (ms). A click on
/// the Yap bar that opened one (a card's Open in Chat) must not then have
/// the bar give the focus back to the app behind it (`bar.rs`
/// `give_focus_back`): over a game, Chat came up behind it.
static BROUGHT_FORWARD_MS: AtomicU64 = AtomicU64::new(0);

/// A Yap window was just brought to the front.
pub fn brought_forward() {
    BROUGHT_FORWARD_MS.store(crate::notices::notice::now_ms(), Ordering::Relaxed);
}

/// Whether one was within the last `ms`.
pub fn brought_forward_within(ms: u64) -> bool {
    crate::notices::notice::now_ms().saturating_sub(BROUGHT_FORWARD_MS.load(Ordering::Relaxed)) <= ms
}

/// Show the settings window (defined hidden in tauri.conf.json). Shared by the
/// `open_settings` command and the tray menu.
///
/// The `set_focus` is load-bearing here and in `show_onboarding`: both windows
/// are created unfocused (`"focus": false`) so launching Yap never pulls the
/// foreground onto a hidden window, which makes their first `show()` inactive.
pub fn show_settings(app: &AppHandle) -> Result<(), String> {
    let w = app
        .get_webview_window("settings")
        .ok_or("settings window not found")?;
    // `show()` leaves a minimized window on the taskbar; `set_focus` would
    // restore it, but e2e test runs skip that (below).
    if w.is_minimized().unwrap_or(false) {
        let _ = w.unminimize();
    }
    fit_to_screen(&w);
    let _ = w.show();
    focus(&w);
    behind_in_test_runs(&w);
    // The window is on screen now: a pending update announcement moves into
    // it. Not left to the Focused event alone: Windows can refuse a
    // background app the foreground, and e2e test runs never take it.
    crate::updates::on_main_window_focused(app);
    Ok(())
}

/// A window inside its monitor's work area. The main window's 1200 × 800
/// default is taller than a laptop at 150 % (1920 × 1080 → 1280 × 672 above
/// the taskbar) or a 1366 × 768 screen: centred, its title bar — the only
/// place to drag it — sat above the top of the screen and its bottom under
/// the taskbar; onboarding's 720 px put its buttons there. A window that
/// fits (or is maximised) is left alone.
fn fit_to_screen(w: &tauri::WebviewWindow) {
    if w.is_maximized().unwrap_or(false) {
        return;
    }
    let (Ok(Some(monitor)), Ok(size), Ok(pos)) = (w.current_monitor(), w.outer_size(), w.outer_position())
    else {
        return;
    };
    let area = monitor.work_area();
    let fitted = fit_rect(
        (pos.x, pos.y, size.width, size.height),
        (area.position.x, area.position.y, area.size.width, area.size.height),
    );
    if let Some((x, y, width, height)) = fitted {
        tracing::info!(width, height, "main window fitted to the screen");
        let _ = w.set_size(tauri::PhysicalSize::new(width, height));
        let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
    }
}

/// A window rect `(x, y, width, height)` moved and shrunk to lie inside the
/// work area, or `None` when it already does. Pure.
pub(crate) fn fit_rect(window: (i32, i32, u32, u32), area: (i32, i32, u32, u32)) -> Option<(i32, i32, u32, u32)> {
    let (x, y, width, height) = window;
    let (ax, ay, aw, ah) = area;
    if aw == 0 || ah == 0 {
        return None;
    }
    let width2 = width.min(aw);
    let height2 = height.min(ah);
    let x2 = x.clamp(ax, ax + (aw - width2) as i32);
    let y2 = y.clamp(ay, ay + (ah - height2) as i32);
    let fitted = (x2, y2, width2, height2);
    (fitted != window).then_some(fitted)
}

/// A test run's window, just shown: at the bottom of the z-order, behind the
/// apps of whoever works beside the suite (it still draws there: the suite
/// turns WebView2's occlusion tracking off, `e2e/support/yap.js`). Nothing
/// outside test runs, nor in robot mode, whose real windows are its point.
/// (25 Oct 2026: the main window kept popping up mid-screen during runs.)
pub fn behind_in_test_runs(w: &tauri::WebviewWindow) {
    if !crate::shell::e2e::active() || crate::shell::realtest::on() {
        return;
    }
    #[cfg(windows)]
    if let Ok(hwnd) = w.hwnd() {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, HWND_BOTTOM, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE};
        unsafe {
            let _ = SetWindowPos(HWND(hwnd.0), Some(HWND_BOTTOM), 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE);
        }
    }
}

/// **The person is in `w`**: it's Windows' foreground window (or owns the
/// one in front: a Save dialog it opened). Not `is_focused`: that's tao's
/// record of the window's own keyboard focus, and wry hands the keyboard
/// focus to WebView2's child window, so it can say no while the person
/// types in Chat (30 Oct 2026: the Voice Assistant's card came up over
/// Chat in front of the maintainer).
pub fn in_front(w: &tauri::WebviewWindow) -> bool {
    #[cfg(windows)]
    if let Ok(hwnd) = w.hwnd() {
        use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GetForegroundWindow, GA_ROOTOWNER};
        let fg = unsafe { GetForegroundWindow() };
        if fg.0.is_null() {
            return false;
        }
        return fg.0 == hwnd.0 || unsafe { GetAncestor(fg, GA_ROOTOWNER) }.0 == hwnd.0;
    }
    w.is_focused().unwrap_or(false)
}

/// A rectangle on the desktop, `(left, top, right, bottom)` in physical
/// pixels.
pub(crate) type Bounds = (i32, i32, i32, i32);

/// How much of `target` the windows above it leave in view, 0 to 1: the
/// part none of `above` covers (each clipped to it, overlaps counted once).
/// Pure.
pub(crate) fn uncovered_share(target: Bounds, above: &[Bounds]) -> f64 {
    let (l, t, r, b) = target;
    if r <= l || b <= t {
        return 0.0;
    }
    let over: Vec<Bounds> = above
        .iter()
        .map(|&(al, at, ar, ab)| (al.max(l), at.max(t), ar.min(r), ab.min(b)))
        .filter(|&(cl, ct, cr, cb)| cr > cl && cb > ct)
        .collect();
    let mut xs: Vec<i32> = vec![l, r];
    let mut ys: Vec<i32> = vec![t, b];
    for &(cl, ct, cr, cb) in &over {
        xs.extend([cl, cr]);
        ys.extend([ct, cb]);
    }
    xs.sort_unstable();
    xs.dedup();
    ys.sort_unstable();
    ys.dedup();
    let mut covered: i64 = 0;
    for x in xs.windows(2) {
        for y in ys.windows(2) {
            if over.iter().any(|&(cl, ct, cr, cb)| cl <= x[0] && x[0] < cr && ct <= y[0] && y[0] < cb) {
                covered += i64::from(x[1] - x[0]) * i64::from(y[1] - y[0]);
            }
        }
    }
    let area = i64::from(r - l) * i64::from(b - t);
    1.0 - covered as f64 / area as f64
}

/// How much of `w` is in view, 0 to 1: none while it's hidden, minimised or
/// cloaked (another virtual desktop), else what the windows above it leave
/// uncovered ([`uncovered_share`]) — every visible, uncloaked, unminimised
/// window higher in Windows' z-order, except click-through ones (overlays)
/// and the Yap bar. Never with a lock held (window getters).
pub fn in_view(w: &tauri::WebviewWindow) -> f64 {
    #[cfg(windows)]
    {
        let Ok(hwnd) = w.hwnd() else { return 0.0 };
        z::in_view(hwnd.0 as isize, crate::bar::overlay::win::bar_hwnd().map_or(0, |h| h as isize))
    }
    #[cfg(not(windows))]
    {
        f64::from(u8::from(w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false)))
    }
}

#[cfg(windows)]
mod z {
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindow, GetWindowLongPtrW, GetWindowRect, IsIconic, IsWindowVisible, GWL_EXSTYLE, GW_HWNDPREV, WS_EX_TRANSPARENT,
    };

    /// The most windows above it looked at (a desktop rarely has more).
    const MOST: usize = 400;

    fn cloaked(hwnd: HWND) -> bool {
        let mut v: u32 = 0;
        unsafe { DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut v as *mut u32 as *mut _, 4) }.is_ok() && v != 0
    }

    /// Its frame as drawn (without the invisible resize borders).
    fn frame(hwnd: HWND) -> Option<super::Bounds> {
        let mut r = RECT::default();
        let ok = unsafe { DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut RECT as *mut _, std::mem::size_of::<RECT>() as u32) }
            .is_ok()
            || unsafe { GetWindowRect(hwnd, &mut r) }.is_ok();
        ok.then_some((r.left, r.top, r.right, r.bottom))
    }

    fn drawn(hwnd: HWND) -> bool {
        unsafe { IsWindowVisible(hwnd) }.as_bool() && !unsafe { IsIconic(hwnd) }.as_bool() && !cloaked(hwnd)
    }

    pub fn in_view(target: isize, bar: isize) -> f64 {
        let target = HWND(target as *mut _);
        if !drawn(target) {
            return 0.0;
        }
        let Some(bounds) = frame(target) else { return 0.0 };
        let mut above = Vec::new();
        let mut h = target;
        for _ in 0..MOST {
            match unsafe { GetWindow(h, GW_HWNDPREV) } {
                Ok(prev) if !prev.0.is_null() => h = prev,
                _ => break,
            }
            if h.0 as isize == bar || !drawn(h) {
                continue;
            }
            let ex = unsafe { GetWindowLongPtrW(h, GWL_EXSTYLE) } as u32;
            if ex & WS_EX_TRANSPARENT.0 != 0 {
                continue;
            }
            if let Some(b) = frame(h) {
                above.push(b);
            }
        }
        super::uncovered_share(bounds, &above)
    }
}

/// `set_focus`, except in e2e test runs: when Windows won't hand a background
/// process the foreground, tao falls back to pressing Alt in whichever app has
/// focus — the developer's, while the suite runs beside them.
fn focus(w: &tauri::WebviewWindow) {
    brought_forward();
    if !crate::shell::e2e::active() {
        let _ = w.set_focus();
    }
}

/// Open the settings window.
#[tauri::command]
pub fn open_settings(app: AppHandle) -> Result<(), String> {
    show_settings(&app)
}

/// Show the first-run onboarding window (model picker). Shared by the
/// `open_onboarding` command and the first-run check in `setup`.
pub fn show_onboarding(app: &AppHandle) -> Result<(), String> {
    let w = app
        .get_webview_window("onboarding")
        .ok_or("onboarding window not found")?;
    // Show FIRST, then reload. Reloading while hidden left the webview with a
    // frozen visual surface when focused (JS state advanced — log-proven —
    // but pixels only updated after the window LOST focus). The reload itself
    // stays: it gives fresh event listeners and a fresh wizard every open.
    // (720 px tall: on a laptop at 150 % its buttons were under the taskbar.)
    fit_to_screen(&w);
    let _ = w.show();
    focus(&w);
    behind_in_test_runs(&w);
    // Tips and notices wait while the first-run setup is on screen (the
    // bar's first-use pointers too: it's told).
    crate::notices::notice::on_onboarding(true);
    crate::bar::refresh(app);
    let _ = w.eval("window.location.reload()");
    // Compositor nudge: a 1px resize round-trip forces DWM to recomposite the
    // webview surface, un-sticking any stale frame.
    if let Ok(size) = w.inner_size() {
        let _ = w.set_size(tauri::PhysicalSize::new(size.width + 1, size.height));
        let _ = w.set_size(size);
    }
    Ok(())
}

/// Open the onboarding / model-picker window.
#[tauri::command]
pub fn open_onboarding(app: AppHandle) -> Result<(), String> {
    show_onboarding(&app)
}

/// Hide the onboarding window (called when the user finishes first-run setup).
#[tauri::command]
pub fn close_onboarding(app: AppHandle) {
    if let Some(w) = app.get_webview_window("onboarding") {
        let _ = w.hide();
    }
    crate::notices::notice::on_onboarding(false);
    // The bar's first-use pointers may show now (`bar/pointer.rs`).
    crate::bar::refresh(&app);
}

/// What the window-state plugin remembers for the main window (see `run`).
/// Excludes VISIBLE so a start-hidden launch never un-hides itself.
pub(crate) fn window_state_flags() -> tauri_plugin_window_state::StateFlags {
    use tauri_plugin_window_state::StateFlags;
    StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED
}

#[cfg(target_os = "windows")]
mod dwm {
    use std::ffi::c_void;
    pub const DWMWA_TRANSITIONS_FORCEDISABLED: u32 = 3;
    pub const DWMWA_CLOAK: u32 = 13;
    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(
            hwnd: *mut c_void,
            attribute: u32,
            value: *const c_void,
            size: u32,
        ) -> i32;
        pub fn DwmFlush() -> i32;
    }

    /// Set a BOOL-valued DWM window attribute (best-effort).
    pub fn set_flag(hwnd: *mut c_void, attribute: u32, on: bool) {
        let value: i32 = on.into();
        unsafe {
            DwmSetWindowAttribute(
                hwnd,
                attribute,
                &value as *const i32 as *const c_void,
                std::mem::size_of::<i32>() as u32,
            );
        }
    }
}

/// One-shot show+hide of a still-hidden webview window so WebView2 finishes
/// initializing while VISIBLE (see the call site in `setup`), without anything
/// reaching the screen: the window is DWM-cloaked (composed, never drawn) with
/// its open/close animations disabled for the round-trip.
///
/// The previous version parked the window at (-32000,-32000) and moved it back
/// straight after, which flashed a blank "Welcome to Yap" window on every
/// launch — `hide()` starts DWM's ~200 ms close animation, and that animation
/// followed the window back to its real on-screen position. (Parking also
/// leaked -32000 into the window-state plugin's saved position.)
#[cfg(target_os = "windows")]
pub(crate) fn init_hidden_webview(window: &tauri::WebviewWindow) {
    // Already on screen (dev builds show Settings at launch): it initialized
    // visible anyway, and a show+hide here would just hide it again.
    if window.is_visible().unwrap_or(false) {
        return;
    }
    let Ok(hwnd) = window.hwnd() else { return };
    dwm::set_flag(hwnd.0, dwm::DWMWA_CLOAK, true);
    dwm::set_flag(hwnd.0, dwm::DWMWA_TRANSITIONS_FORCEDISABLED, true);
    let _ = window.show();
    let _ = window.hide();
    // Let DWM process the hide (one composition pass) while the window is
    // still cloaked, then restore normal behaviour for real opens.
    unsafe { dwm::DwmFlush() };
    dwm::set_flag(hwnd.0, dwm::DWMWA_TRANSITIONS_FORCEDISABLED, false);
    dwm::set_flag(hwnd.0, dwm::DWMWA_CLOAK, false);
}

#[cfg(test)]
mod tests {
    use super::{fit_rect, uncovered_share};

    #[test]
    fn how_much_of_a_window_the_ones_above_it_leave_in_view() {
        let chat = (0, 0, 1000, 800);
        // Nothing above it, or only on another monitor: all of it.
        assert_eq!(uncovered_share(chat, &[]), 1.0);
        assert_eq!(uncovered_share(chat, &[(2560, 0, 5120, 1400)]), 1.0);
        // A maximised app over it on the same monitor: none.
        assert_eq!(uncovered_share(chat, &[(-8, -8, 2568, 1400)]), 0.0);
        // A small window over a corner: most of it.
        assert_eq!(uncovered_share(chat, &[(500, 400, 1000, 800)]), 0.75);
        // Two overlapping windows are counted once.
        assert_eq!(uncovered_share(chat, &[(0, 0, 500, 800), (250, 0, 500, 800)]), 0.5);
        assert_eq!(uncovered_share(chat, &[(0, 0, 600, 800), (400, 0, 1000, 800)]), 0.0);
        // A window with no size shows nothing.
        assert_eq!(uncovered_share((10, 10, 10, 50), &[]), 0.0);
    }

    #[test]
    fn the_main_window_fits_a_small_screen() {
        // 1920 × 1080 at 150 %: 1920 × 1008 physical above the taskbar. The
        // 1200 × 800 window (1800 × 1200 physical), centred, started 60 px
        // above the screen and ran under the taskbar.
        assert_eq!(
            fit_rect((60, -60, 1800, 1200), (0, 0, 1920, 1008)),
            Some((60, 0, 1800, 1008))
        );
        // A 1366 × 768 screen at 100 % (728 above the taskbar).
        assert_eq!(fit_rect((83, -16, 1200, 800), (0, 0, 1366, 728)), Some((83, 0, 1200, 728)));
        // A window that fits is left alone, on any monitor.
        assert_eq!(fit_rect((660, 320, 1200, 800), (0, 0, 2560, 1400)), None);
        assert_eq!(fit_rect((2700, 100, 1200, 800), (2560, 0, 2560, 1400)), None);
        // Off the right edge of a second monitor: moved back on.
        assert_eq!(
            fit_rect((4500, 100, 1200, 800), (2560, 0, 2560, 1400)),
            Some((3920, 100, 1200, 800))
        );
        // No work area reported: nothing to fit to.
        assert_eq!(fit_rect((0, 0, 1200, 800), (0, 0, 0, 0)), None);
    }
}

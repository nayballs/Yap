//! **Yap's page fills its window, whatever resized it** (18 Nov 2026).
//!
//! The maintainer snapped the main window into a Snap layouts zone by its
//! title bar (dragged to the top of the screen), then maximised, minimised
//! and restored it: Yap's page went on drawing — clocks ticking, typing
//! showing — but at the snapped size, the rest of the window WebView2's
//! background, until Chat was left and opened again. Not reproduced by
//! programmatic snaps, Win+Left, maximise, minimise, restore or a resize
//! while cloaked; the picture is reproduced exactly by WebView2's own browser
//! window inside the webview left at an older size than the controller's
//! bounds (its page is laid out at that size), and **the controller's bounds
//! set again, even unchanged, put it right**.
//!
//! So the main window's webview is fitted to the window again, besides
//! wry's own resize on `WM_SIZE`:
//! - a moment after the window's size settles — a `WM_SIZE` that isn't a
//!   minimise, a size change in `WM_WINDOWPOSCHANGED`, the end of a drag of
//!   its edge or title bar (`WM_EXITSIZEMOVE`: a Snap layouts drop ends one)
//!   or a monitor's scale change (`WM_DPICHANGED`) — [`SETTLE_MS`] after the
//!   last, never while minimised or at 0 × 0;
//! - whenever the page finds itself laid out at another size than its window
//!   (`core/viewFit.js` asks [`main_view_fit`] on a resize, the window's focus
//!   and every few seconds while it has it; twice, a moment apart, before
//!   anything is done: a page mid-resize lags its window by a frame).
//!
//! Fitting is the controller's bounds set to the window's client area (and
//! wry's container window to the same), which WebView2 passes on to its own
//! windows: nothing changes when they're right.

use tauri::{AppHandle, Manager, WebviewWindow};

/// How long after the last size message the webview is fitted again.
pub const SETTLE_MS: u32 = 150;

/// Device pixels a page's size may differ from its window's by (CSS pixels
/// rounded at a fractional scale).
const SLACK: i32 = 3;

/// **Is the page laid out at another size than its window?** `client`: the
/// window's client area, `page`: the page's own size (its `innerWidth` and
/// `innerHeight` times its pixel ratio), both device pixels. Never for a
/// minimised window or a size of nothing (a window on its way down). Pure.
pub fn needs_fit(client: (i32, i32), page: (i32, i32), minimised: bool) -> bool {
    if minimised || client.0 <= 0 || client.1 <= 0 || page.0 <= 0 || page.1 <= 0 {
        return false;
    }
    (client.0 - page.0).abs() > SLACK || (client.1 - page.1).abs() > SLACK
}

/// What [`main_view_fit`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Fit {
    /// The page fills its window (or the window is minimised, hidden or
    /// mid-drag: nothing to tell).
    Fine,
    /// It doesn't: asked once, nothing done yet (`fit` false).
    Off,
    /// It didn't, and the webview has been fitted to the window again.
    Fitted,
}

/// Watch the main window: its webview fitted again once its size settles
/// (on the main thread, from setup).
pub fn watch(window: &WebviewWindow) {
    imp::watch(window);
}

/// **The page's own check**: it's laid out at `width` × `height` device
/// pixels. Off from the window's client area by more than a rounding, the
/// webview is fitted to the window again when `fit` (the page asks twice, a
/// moment apart, before it says so).
#[tauri::command]
pub async fn main_view_fit(app: AppHandle, width: i32, height: i32, fit: bool) -> Result<Fit, String> {
    let window = app.get_webview_window("settings").ok_or("Yap's window isn't there.")?;
    imp::check(&window, (width, height), fit).await
}

#[cfg(windows)]
mod imp {
    use std::cell::{Cell, RefCell};

    use tauri::WebviewWindow;
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClientRect, IsIconic, IsWindowVisible, KillTimer, SetTimer, SetWindowPos, SIZE_MINIMIZED, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
        WINDOWPOS, WM_DPICHANGED, WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE, WM_SIZE, WM_TIMER, WM_WINDOWPOSCHANGED,
    };

    use super::{needs_fit, Fit, SETTLE_MS};

    /// The subclass's id among Yap's window's, and its timer's.
    const SUBCLASS: usize = 0x59_6146;
    const TIMER: usize = 0x59_6146;

    thread_local! {
        /// The main window's webview controller (wry's), for fitting it.
        static CONTROLLER: RefCell<Option<ICoreWebView2Controller>> = const { RefCell::new(None) };
        /// Inside a drag of the window's edge or title bar.
        static MOVING: Cell<bool> = const { Cell::new(false) };
    }

    pub fn watch(window: &WebviewWindow) {
        let Ok(hwnd) = window.hwnd() else { return };
        let hwnd = hwnd.0 as isize;
        let set = window.with_webview(move |pw| {
            CONTROLLER.with(|c| *c.borrow_mut() = Some(pw.controller()));
            // (Never taken off: it lives as long as the window.)
            let _ = unsafe { SetWindowSubclass(HWND(hwnd as *mut _), Some(fit_proc), SUBCLASS, 0) };
        });
        if let Err(e) = set {
            tracing::warn!("Couldn't watch the main window's size: {e}");
        }
    }

    /// The window's client area, unless it's minimised or has none.
    fn client(hwnd: HWND) -> Option<(i32, i32)> {
        if unsafe { IsIconic(hwnd) }.as_bool() {
            return None;
        }
        let mut r = RECT::default();
        unsafe { GetClientRect(hwnd, &mut r) }.ok()?;
        let size = (r.right - r.left, r.bottom - r.top);
        (size.0 > 0 && size.1 > 0).then_some(size)
    }

    /// The webview fitted to the window's client area: the controller's
    /// bounds (WebView2 passes them on to its own windows, even unchanged)
    /// and wry's container window. (On the main thread.)
    fn fit(hwnd: HWND) -> Option<(i32, i32)> {
        let (w, h) = client(hwnd)?;
        let controller = CONTROLLER.with(|c| c.borrow().clone())?;
        unsafe {
            let _ = controller.SetBounds(RECT { left: 0, top: 0, right: w, bottom: h });
            let mut container = HWND::default();
            if controller.ParentWindow(&mut container).is_ok() && !container.0.is_null() && container != hwnd {
                let _ = SetWindowPos(container, None, 0, 0, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
            }
            let _ = controller.NotifyParentWindowPositionChanged();
        }
        Some((w, h))
    }

    /// The fit, again, [`SETTLE_MS`] after the last of these.
    fn settle_later(hwnd: HWND) {
        unsafe { SetTimer(Some(hwnd), TIMER, SETTLE_MS, None) };
    }

    unsafe extern "system" fn fit_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _: usize, _: usize) -> LRESULT {
        match msg {
            WM_SIZE if wp.0 != SIZE_MINIMIZED as usize => settle_later(hwnd),
            WM_WINDOWPOSCHANGED => {
                let pos = lp.0 as *const WINDOWPOS;
                if !pos.is_null() && unsafe { (*pos).flags } & SWP_NOSIZE != SWP_NOSIZE {
                    settle_later(hwnd);
                }
            }
            WM_DPICHANGED => settle_later(hwnd),
            WM_ENTERSIZEMOVE => MOVING.with(|m| m.set(true)),
            WM_EXITSIZEMOVE => {
                MOVING.with(|m| m.set(false));
                settle_later(hwnd);
            }
            WM_TIMER if wp.0 == TIMER => {
                unsafe {
                    let _ = KillTimer(Some(hwnd), TIMER);
                }
                fit(hwnd);
                return LRESULT(0);
            }
            _ => {}
        }
        unsafe { DefSubclassProc(hwnd, msg, wp, lp) }
    }

    pub async fn check(window: &WebviewWindow, page: (i32, i32), want_fit: bool) -> Result<Fit, String> {
        let hwnd = window.hwnd().map_err(|e| e.to_string())?.0 as isize;
        let (tx, rx) = tokio::sync::oneshot::channel();
        window
            .with_webview(move |_| {
                let hwnd = HWND(hwnd as *mut _);
                let shown = unsafe { IsWindowVisible(hwnd) }.as_bool();
                let found = match client(hwnd) {
                    Some(c) if shown && !MOVING.with(Cell::get) && needs_fit(c, page, false) => {
                        if want_fit {
                            fit(hwnd);
                            tracing::info!(page_w = page.0, page_h = page.1, window_w = c.0, window_h = c.1, "Yap's page was laid out at another size than its window: fitted again");
                            Fit::Fitted
                        } else {
                            Fit::Off
                        }
                    }
                    _ => Fit::Fine,
                };
                let _ = tx.send(found);
            })
            .map_err(|e| e.to_string())?;
        rx.await.map_err(|_| "Yap's window didn't answer.".to_string())
    }

    /// Test mode: **WebView2's own window inside the main webview left at
    /// `w` × `h`** while the controller's bounds stay the window's — the
    /// state the maintainer's snap left (its page laid out at that size,
    /// the rest of the window the webview's background). Its windows are
    /// WebView2's (`Chrome_WidgetWin_0` in Yap's process, `Chrome_WidgetWin_1`
    /// in its browser's under it): `Err` if they aren't there.
    #[cfg(debug_assertions)]
    pub async fn e2e_stale(window: &WebviewWindow, w: i32, h: i32) -> Result<(), String> {
        use windows::core::{w, PCWSTR};
        use windows::Win32::UI::WindowsAndMessaging::{FindWindowExW, SWP_ASYNCWINDOWPOS, SWP_NOMOVE};
        let (tx, rx) = tokio::sync::oneshot::channel();
        window
            .with_webview(move |pw| unsafe {
                let mut container = HWND::default();
                let _ = pw.controller().ParentWindow(&mut container);
                let inner = FindWindowExW(Some(container), None, w!("Chrome_WidgetWin_0"), PCWSTR::null())
                    .and_then(|w0| FindWindowExW(Some(w0), None, w!("Chrome_WidgetWin_1"), PCWSTR::null()));
                let _ = tx.send(match inner {
                    Ok(w1) => SetWindowPos(w1, None, 0, 0, w, h, SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOMOVE | SWP_ASYNCWINDOWPOS).map_err(|e| e.to_string()),
                    Err(_) => Err("WebView2's own window isn't there.".to_string()),
                });
            })
            .map_err(|e| e.to_string())?;
        rx.await.map_err(|_| "Yap's window didn't answer.".to_string())?
    }

    /// Test mode: the window as Windows would have it, without activating
    /// it: `minimise`, `restore`, or a drag of its title bar that ends
    /// (`dragged`: `WM_ENTERSIZEMOVE` then `WM_EXITSIZEMOVE`, as a Snap
    /// layouts drop ends one).
    #[cfg(debug_assertions)]
    pub async fn e2e_window(window: &WebviewWindow, action: &str) -> Result<(), String> {
        use windows::Win32::UI::WindowsAndMessaging::{SendMessageW, ShowWindow, SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE};
        let hwnd = HWND(window.hwnd().map_err(|e| e.to_string())?.0);
        unsafe {
            match action {
                "minimise" => {
                    let _ = ShowWindow(hwnd, SW_SHOWMINNOACTIVE);
                }
                "restore" => {
                    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                }
                "dragged" => {
                    SendMessageW(hwnd, WM_ENTERSIZEMOVE, None, None);
                    SendMessageW(hwnd, WM_EXITSIZEMOVE, None, None);
                }
                _ => return Err("minimise, restore or dragged".into()),
            }
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    use tauri::WebviewWindow;

    use super::Fit;

    pub fn watch(_: &WebviewWindow) {}

    pub async fn check(_: &WebviewWindow, _: (i32, i32), _: bool) -> Result<Fit, String> {
        Ok(Fit::Fine)
    }
}

/// Test mode: WebView2's own window inside the main webview left at an
/// older size, `width` × `height` device pixels (see `imp::e2e_stale`).
#[cfg(all(debug_assertions, windows))]
#[tauri::command]
pub async fn e2e_main_view_stale(app: AppHandle, width: i32, height: i32) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".into());
    }
    let window = app.get_webview_window("settings").ok_or("Yap's window isn't there.")?;
    imp::e2e_stale(&window, width, height).await
}

/// Test mode: the main window minimised, restored or dragged by its title
/// bar, without taking the focus (see `imp::e2e_window`).
#[cfg(all(debug_assertions, windows))]
#[tauri::command]
pub async fn e2e_main_window(app: AppHandle, action: String) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".into());
    }
    let window = app.get_webview_window("settings").ok_or("Yap's window isn't there.")?;
    imp::e2e_window(&window, &action).await
}

#[cfg(test)]
mod tests {
    use super::needs_fit;

    #[test]
    fn a_page_laid_out_at_another_size_than_its_window_needs_fitting() {
        // The maintainer's snap: the page at the snapped size, the window maximised.
        assert!(needs_fit((2560, 1392), (1498, 1392), false));
        assert!(needs_fit((2560, 1392), (2560, 1080), false));
        // Fits, or off by a rounding at 125 % / 150 %.
        assert!(!needs_fit((1200, 800), (1200, 800), false));
        assert!(!needs_fit((1279, 801), (1278, 800), false));
        assert!(!needs_fit((1279, 801), (1276, 798), false));
        // Never while minimised, nor for a size of nothing.
        assert!(!needs_fit((2560, 1392), (1498, 1392), true));
        assert!(!needs_fit((0, 0), (1200, 800), false));
        assert!(!needs_fit((1200, 800), (0, 0), false));
        assert!(!needs_fit((-32000, 19), (1200, 800), false));
    }
}

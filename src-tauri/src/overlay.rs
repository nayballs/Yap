//! The floating "transcribing" overlay — a small capsule that appears at the
//! bottom-center of the primary monitor while Yap is recording or processing,
//! and hides when it returns to idle.
//!
//! Driven entirely by the `yap-state` listener in `lib.rs` (decoupled from the
//! pipeline); this module just shows/hides + positions the `overlay` window.

use tauri::{AppHandle, LogicalPosition, Manager, Position};

/// Overlay window dimensions (must match `tauri.conf.json`).
const OVERLAY_WIDTH: f64 = 330.0;
const OVERLAY_HEIGHT: f64 = 48.0;
/// Gap between the overlay and the bottom edge of the screen (logical px).
const BOTTOM_MARGIN: f64 = 48.0;
/// Gap between the overlay and the top edge of the screen (logical px).
const TOP_MARGIN: f64 = 40.0;

#[cfg(target_os = "windows")]
mod win {
    use std::ffi::c_void;
    pub const SWP_NOSIZE: u32 = 0x0001;
    pub const SWP_NOMOVE: u32 = 0x0002;
    pub const SWP_NOACTIVATE: u32 = 0x0010;
    pub const SWP_SHOWWINDOW: u32 = 0x0040;
    #[link(name = "user32")]
    extern "system" {
        pub fn SetWindowPos(
            hwnd: *mut c_void,
            insert_after: *mut c_void,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            flags: u32,
        ) -> i32;
    }
}

/// Force a window to the very top of the native Z-order. Uses the raw Win32
/// `SetWindowPos(HWND_TOPMOST, …)` (no move/resize, no focus steal), which is
/// more reliable than Tauri's `set_always_on_top` — that one can be overridden
/// by other topmost windows. (Approach borrowed from Handy.) Off Windows this
/// falls back to Tauri's wrapper.
pub fn force_topmost(window: &tauri::WebviewWindow) {
    #[cfg(target_os = "windows")]
    {
        if let Ok(hwnd) = window.hwnd() {
            // HWND_TOPMOST == (HWND)-1
            let topmost = -1isize as *mut std::ffi::c_void;
            unsafe {
                win::SetWindowPos(
                    hwnd.0 as *mut std::ffi::c_void,
                    topmost,
                    0,
                    0,
                    0,
                    0,
                    win::SWP_NOMOVE | win::SWP_NOSIZE | win::SWP_NOACTIVATE | win::SWP_SHOWWINDOW,
                );
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window.set_always_on_top(true);
    }
}

/// Show the overlay, positioned at the bottom-center of the primary monitor.
///
/// Monitor position/size are physical pixels; we divide by the monitor's scale
/// factor to get logical coordinates and set a `LogicalPosition`, which Tauri
/// then maps correctly regardless of DPI.
pub fn show_overlay(app: &AppHandle) {
    let Some(win) = app.get_webview_window("overlay") else {
        tracing::warn!("overlay window not found");
        return;
    };

    // "top" or "bottom" (default) from the saved config.
    let at_top = crate::config::load().overlay_position == "top";

    match app.primary_monitor() {
        Ok(Some(monitor)) => {
            let scale = monitor.scale_factor();
            let mon_x = monitor.position().x as f64 / scale;
            let mon_y = monitor.position().y as f64 / scale;
            let mon_w = monitor.size().width as f64 / scale;
            let mon_h = monitor.size().height as f64 / scale;

            let x = mon_x + (mon_w - OVERLAY_WIDTH) / 2.0;
            let y = if at_top {
                mon_y + TOP_MARGIN
            } else {
                mon_y + mon_h - OVERLAY_HEIGHT - BOTTOM_MARGIN
            };

            let _ = win.set_position(Position::Logical(LogicalPosition { x, y }));
        }
        _ => tracing::warn!("no primary monitor; showing overlay at its default position"),
    }

    let _ = win.show();
    // Re-assert topmost after showing so it sits above the focused window.
    force_topmost(&win);
}

/// Hide the overlay window.
pub fn hide_overlay(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("overlay") {
        let _ = win.hide();
    }
}

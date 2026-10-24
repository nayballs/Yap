//! Yap's own windows: the main window and onboarding shown (fitted to
//! the screen, focused except in test runs) by the tray, the pages and
//! the first-run check.

use tauri::{AppHandle, Manager};

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

/// `set_focus`, except in e2e test runs: when Windows won't hand a background
/// process the foreground, tao falls back to pressing Alt in whichever app has
/// focus — the developer's, while the suite runs beside them.
fn focus(w: &tauri::WebviewWindow) {
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
    // Tips and notices wait while the first-run setup is on screen.
    crate::notices::notice::on_onboarding(true);
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
}

#[cfg(test)]
mod tests {
    use super::fit_rect;

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

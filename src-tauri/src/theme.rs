//! The theme of Yap's windows (Settings → General → Appearance → Theme:
//! System / Light / Dark, `config.theme`), on the Rust side.
//!
//! The pages draw the theme themselves (`src/lib/appearance.js` marks
//! `<html data-yap-theme>`, app.css has both palettes, and index.html's head
//! script puts the last theme on before the first paint). What a page can't
//! reach is the **webview's own background**: what WebView2 shows before the
//! page paints — a window shown after a while hidden (its renderer trimmed by
//! the low memory target, `webview_memory.rs`), the area a resize or
//! maximise uncovers. Left at WebView2's default (white), a dark Yap would
//! flash light there. So the opaque windows — the main window, onboarding,
//! the meeting notepad — get their page's background colour from here: at
//! startup (before their DWM-cloaked first show), after every config save,
//! and when Windows' app mode changes while the theme follows it
//! (`WindowEvent::ThemeChanged`). The Yap bar and the split preview are
//! transparent and left alone. Onboarding, the one window with Windows' own
//! title bar, gets it dark or light to match (DWM's immersive dark mode).
//!
//! "System" follows Windows' app mode (Settings → Personalisation → Colours →
//! "Choose your app mode"): `AppsUseLightTheme` in the user's
//! Themes\Personalize key, the same value WebView2's `prefers-color-scheme`
//! follows. Missing (older Windows) = light.

use tauri::{AppHandle, Manager};

/// A resolved theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

/// The theme for a `config.theme` choice: "light" and "dark" as written,
/// anything else (the default "system", an unknown value from another Yap)
/// Windows' app mode.
pub fn resolve(choice: &str, system_dark: bool) -> Theme {
    if choice.eq_ignore_ascii_case("dark") {
        Theme::Dark
    } else if choice.eq_ignore_ascii_case("light") {
        Theme::Light
    } else if system_dark {
        Theme::Dark
    } else {
        Theme::Light
    }
}

/// The opaque windows and their page's background in each theme (app.css:
/// the main window and onboarding on `--yap-bg`, the notepad on
/// `--yap-paper`).
const OPAQUE: &[(&str, [u8; 3], [u8; 3])] = &[
    ("settings", [0xf0, 0xed, 0xe7], [0x16, 0x14, 0x10]),
    ("onboarding", [0xf0, 0xed, 0xe7], [0x16, 0x14, 0x10]),
    ("notepad", [0xfa, 0xf9, 0xf6], [0x1c, 0x1a, 0x16]),
];

/// The background a window's webview gets in `theme` (None: a transparent
/// window, left alone).
pub fn background(label: &str, theme: Theme) -> Option<[u8; 3]> {
    OPAQUE
        .iter()
        .find(|(l, _, _)| *l == label)
        .map(|(_, light, dark)| if theme == Theme::Dark { *dark } else { *light })
}

/// Is Windows' app mode dark right now?
#[cfg(windows)]
pub fn system_dark() -> bool {
    use std::ffi::c_void;
    use windows::core::HSTRING;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let mut value: u32 = 1;
    let mut size = std::mem::size_of::<u32>() as u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            &HSTRING::from(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            &HSTRING::from("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut c_void),
            Some(&mut size),
        )
    };
    r == ERROR_SUCCESS && value == 0
}

#[cfg(not(windows))]
pub fn system_dark() -> bool {
    false
}

/// Give every opaque window's webview the background of the theme `cfg`
/// resolves to. Best-effort; cheap enough to run on every config save.
pub fn apply(app: &AppHandle, cfg: &crate::config::YapConfig) {
    let theme = resolve(&cfg.theme, system_dark());
    for (label, _, _) in OPAQUE {
        let Some(window) = app.get_webview_window(label) else { continue };
        let Some([r, g, b]) = background(label, theme) else { continue };
        if let Err(e) = window.set_background_color(Some(tauri::window::Color(r, g, b, 255))) {
            tracing::debug!(window = %label, "theme: couldn't set the background: {e}");
        }
        // Onboarding is the one window with Windows' own title bar: dark
        // with a dark Yap, even while Windows' app mode is light.
        #[cfg(windows)]
        if *label == "onboarding" {
            dark_title_bar(&window, theme == Theme::Dark);
        }
    }
    tracing::debug!(choice = %cfg.theme, dark = theme == Theme::Dark, "theme: window backgrounds set");
}

/// A decorated window's title bar in Windows' dark or light colours
/// (`DWMWA_USE_IMMERSIVE_DARK_MODE`, Windows 10 20H1 and later; older ones
/// ignore it). Best-effort.
#[cfg(windows)]
fn dark_title_bar(window: &tauri::WebviewWindow, dark: bool) {
    use std::ffi::c_void;
    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmSetWindowAttribute(hwnd: *mut c_void, attribute: u32, value: *const c_void, size: u32) -> i32;
    }
    const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
    let Ok(hwnd) = window.hwnd() else { return };
    let value: i32 = dark.into();
    unsafe {
        DwmSetWindowAttribute(
            hwnd.0,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &value as *const i32 as *const c_void,
            std::mem::size_of::<i32>() as u32,
        );
    }
}

/// Startup: the backgrounds from the saved config, and again whenever
/// Windows' app mode changes (tao reports it on every window; the main
/// window's event is enough).
pub fn init(app: &AppHandle) {
    apply(app, &crate::config::load());
    if let Some(main) = app.get_webview_window("settings") {
        let handle = app.clone();
        main.on_window_event(move |event| {
            if let tauri::WindowEvent::ThemeChanged(_) = event {
                apply(&handle, &crate::config::load());
            }
        });
    }
}

/// Test mode only: the background WebView2 shows under a window's page (its
/// controller's DefaultBackgroundColor), as "#rrggbb" — what a hidden window
/// shows in the moment before its page paints.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn e2e_webview_background(app: AppHandle, label: String) -> Result<String, String> {
    if !crate::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    let webview = app.get_webview_window(&label).ok_or_else(|| format!("no window {label}"))?;
    let (tx, rx) = std::sync::mpsc::channel();
    webview
        .with_webview(move |platform| {
            #[cfg(windows)]
            {
                use webview2_com::Microsoft::Web::WebView2::Win32::{
                    ICoreWebView2Controller2, COREWEBVIEW2_COLOR,
                };
                use windows::core::Interface;
                let read = || -> Result<String, String> {
                    let c2: ICoreWebView2Controller2 = platform.controller().cast().map_err(|e| e.to_string())?;
                    let mut c = COREWEBVIEW2_COLOR::default();
                    unsafe { c2.DefaultBackgroundColor(&mut c) }.map_err(|e| e.to_string())?;
                    Ok(format!("#{:02x}{:02x}{:02x}", c.R, c.G, c.B))
                };
                let _ = tx.send(read());
            }
            #[cfg(not(windows))]
            {
                let _ = platform;
                let _ = tx.send(Err("Windows only".to_string()));
            }
        })
        .map_err(|e| e.to_string())?;
    rx.recv_timeout(std::time::Duration::from_secs(5)).map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_choice_wins_and_system_follows_windows() {
        assert_eq!(resolve("dark", false), Theme::Dark);
        assert_eq!(resolve("Dark", false), Theme::Dark);
        assert_eq!(resolve("light", true), Theme::Light);
        assert_eq!(resolve("system", true), Theme::Dark);
        assert_eq!(resolve("system", false), Theme::Light);
        // Unknown (another Yap's value) or empty: as "system".
        assert_eq!(resolve("", true), Theme::Dark);
        assert_eq!(resolve("sepia", false), Theme::Light);
    }

    #[test]
    fn only_the_opaque_windows_get_a_background() {
        assert_eq!(background("settings", Theme::Light), Some([0xf0, 0xed, 0xe7]));
        assert_eq!(background("settings", Theme::Dark), Some([0x16, 0x14, 0x10]));
        assert_eq!(background("onboarding", Theme::Dark), Some([0x16, 0x14, 0x10]));
        assert_eq!(background("notepad", Theme::Dark), Some([0x1c, 0x1a, 0x16]));
        assert_eq!(background("notepad", Theme::Light), Some([0xfa, 0xf9, 0xf6]));
        // Transparent windows: never (an opaque background is the grey box).
        assert_eq!(background("overlay", Theme::Dark), None);
        assert_eq!(background("split-preview", Theme::Dark), None);
    }

    #[test]
    fn the_backgrounds_match_app_css() {
        // The page paints these same colours (app.css's --yap-bg and
        // --yap-paper, light and dark): a mismatch would show as a band
        // of another colour while the page catches up.
        let css = include_str!("../../src/app.css");
        let hex = |c: [u8; 3]| format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]);
        for (label, light, dark) in OPAQUE {
            assert!(css.contains(&hex(*light)), "{label}: light {} not in app.css", hex(*light));
            assert!(css.contains(&hex(*dark)), "{label}: dark {} not in app.css", hex(*dark));
        }
        assert!(css.contains("--yap-bg: #161410;"));
        assert!(css.contains("--yap-paper: #1c1a16;"));
    }
}

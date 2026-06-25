//! System-tray icon + right-click menu, modelled on Handy's tray.
//!
//! - A **state-aware icon** (a coloured dot: blue idle, red recording, amber
//!   processing, grey needs-model) generated at runtime — no image assets.
//! - A **right-click menu** that changes with state: when idle it offers a
//!   **model submenu** (switch the active model, checkmark on the current one);
//!   while recording/processing it offers **Cancel**. Always: Settings + Quit.
//! - **Left-click** opens Settings (handy when the pill is hidden).
//!
//! The tray is rebuilt on every `blip-state` change via [`update_tray`].

use crate::config;
use crate::stt;
use crate::AppState;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

fn tooltip() -> String {
    format!("Yap v{}", env!("CARGO_PKG_VERSION"))
}

/// Build a 32×32 RGBA "status dot" icon for the given state.
fn state_icon(state: &str) -> Image<'static> {
    let (r, g, b) = match state {
        "recording" => (239u8, 68u8, 68u8),    // red
        "processing" => (245u8, 158u8, 11u8),  // amber
        "needs-model" => (156u8, 163u8, 175u8), // grey
        _ => (96u8, 165u8, 250u8),             // blue (idle)
    };
    let size: u32 = 32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let center = (size as f32 - 1.0) / 2.0;
    let radius = size as f32 * 0.42;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let dist = (dx * dx + dy * dy).sqrt();
            // 1.5px soft edge for a less jagged dot.
            let alpha = if dist <= radius {
                255.0
            } else if dist <= radius + 1.5 {
                255.0 * (1.0 - (dist - radius) / 1.5)
            } else {
                0.0
            };
            let i = ((y * size + x) * 4) as usize;
            rgba[i] = r;
            rgba[i + 1] = g;
            rgba[i + 2] = b;
            rgba[i + 3] = alpha as u8;
        }
    }
    Image::new_owned(rgba, size, size)
}

/// Build the context menu for the given state.
fn build_menu(app: &AppHandle, state: &str) -> tauri::Result<Menu<Wry>> {
    #[cfg(target_os = "macos")]
    let (settings_accel, quit_accel) = (Some("Cmd+,"), Some("Cmd+Q"));
    #[cfg(not(target_os = "macos"))]
    let (settings_accel, quit_accel) = (Some("Ctrl+,"), Some("Ctrl+Q"));

    let version = MenuItem::with_id(app, "version", tooltip(), false, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, settings_accel)?;
    let check_updates =
        MenuItem::with_id(app, "check_updates", "Check for updates…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Yap", true, quit_accel)?;
    let sep = || PredefinedMenuItem::separator(app);

    let recording = matches!(state, "recording" | "processing");

    if recording {
        let cancel = MenuItem::with_id(app, "cancel", "Cancel", true, None::<&str>)?;
        return Menu::with_items(
            app,
            &[&version, &sep()?, &cancel, &sep()?, &settings, &sep()?, &quit],
        );
    }

    // Idle: a model submenu listing the installed models (checkmark = active).
    let data_dir = config::data_dir();
    let current = config::load().model_size;
    let installed: Vec<String> = stt::all_model_ids()
        .into_iter()
        .filter(|id| stt::is_model_installed(&data_dir, id))
        .map(|s| s.to_string())
        .collect();

    if installed.is_empty() {
        // No models yet — offer a way into the model picker.
        let download = MenuItem::with_id(app, "open_models", "Download a model…", true, None::<&str>)?;
        return Menu::with_items(
            app,
            &[
                &version,
                &sep()?,
                &download,
                &sep()?,
                &settings,
                &check_updates,
                &sep()?,
                &quit,
            ],
        );
    }

    let label = if installed.iter().any(|id| *id == current) {
        stt::model_name(&current)
    } else {
        "Model".to_string()
    };
    let submenu = Submenu::with_id(app, "model_submenu", label, true)?;
    for id in &installed {
        let item = CheckMenuItem::with_id(
            app,
            format!("model:{}", id),
            stt::model_name(id),
            true,
            *id == current,
            None::<&str>,
        )?;
        submenu.append(&item)?;
    }

    Menu::with_items(
        app,
        &[
            &version,
            &sep()?,
            &submenu,
            &sep()?,
            &settings,
            &check_updates,
            &sep()?,
            &quit,
        ],
    )
}

/// Switch the active model from the tray (installed models only). Runs the
/// (blocking) model load on a worker thread so the menu callback returns fast.
fn activate_model(app: &AppHandle, model_id: &str) {
    let app = app.clone();
    let model_id = model_id.to_string();
    std::thread::spawn(move || {
        let data_dir = config::data_dir();
        if !stt::is_model_installed(&data_dir, &model_id) {
            return;
        }
        let use_gpu = config::load().use_gpu;
        match stt::create_stt_engine(&data_dir, &model_id, use_gpu) {
            Ok(engine) => {
                let mut cfg = config::load();
                cfg.model_size = model_id.clone();
                let _ = config::save(&cfg);
                if let Some(st) = app.try_state::<AppState>() {
                    if let Ok(guard) = st.pipeline.lock() {
                        if let Some(p) = guard.as_ref() {
                            p.set_engine(engine);
                            p.update_config(cfg);
                        }
                    }
                }
                tracing::info!(model = %model_id, "Active model switched from tray");
                update_tray(&app, "idle"); // refresh the checkmark + label
            }
            Err(e) => tracing::warn!("Tray model switch failed: {}", e),
        }
    });
}

fn on_menu_event(app: &AppHandle, id: &str) {
    match id {
        "settings" => {
            let _ = crate::commands::show_settings(app);
        }
        "check_updates" => {
            // Open Settings (the update UI lives in About) and ask it to check.
            let _ = crate::commands::show_settings(app);
            let _ = app.emit("check-for-updates", ());
        }
        "open_models" => {
            let _ = crate::commands::show_onboarding(app);
        }
        "quit" => app.exit(0),
        "cancel" => {
            if let Some(st) = app.try_state::<AppState>() {
                if let Ok(guard) = st.pipeline.lock() {
                    if let Some(p) = guard.as_ref() {
                        p.cancel();
                    }
                }
            }
        }
        other if other.starts_with("model:") => {
            if let Some(model) = other.strip_prefix("model:") {
                activate_model(app, model);
            }
        }
        _ => {}
    }
}

/// Build the tray icon and install it. Call once at startup.
pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app, "idle")?;
    let tray = TrayIconBuilder::with_id("blip-tray")
        .icon(state_icon("idle"))
        .tooltip(tooltip())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| on_menu_event(app, event.id.as_ref()))
        .on_tray_icon_event(|tray, event| {
            // Left-click opens Settings (useful when the pill is hidden).
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = crate::commands::show_settings(tray.app_handle());
            }
        })
        .build(app)?;
    app.manage(tray);
    Ok(())
}

/// Update the tray icon + menu for a new state. No-op if the tray wasn't built.
pub fn update_tray(app: &AppHandle, state: &str) {
    if app.try_state::<TrayIcon>().is_none() {
        return;
    }
    let tray = app.state::<TrayIcon>();
    let _ = tray.set_icon(Some(state_icon(state)));
    if let Ok(menu) = build_menu(app, state) {
        let _ = tray.set_menu(Some(menu));
    }
    let _ = tray.set_tooltip(Some(tooltip()));
}

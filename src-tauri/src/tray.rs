//! System-tray icon + right-click menu, modelled on Handy's tray.
//!
//! - A **state-aware icon** (a coloured dot: blue idle, red recording, amber
//!   processing, grey needs-model) generated at runtime — no image assets.
//! - A **right-click menu** that changes with state: when idle it offers a
//!   **model submenu** (switch the active model, checkmark on the current one);
//!   while recording/processing it offers **Cancel**. Always: Settings + Quit.
//! - **Left-click** opens Settings (handy when the pill is hidden).
//!
//! The tray is rebuilt on every `yap-state` change via [`update_tray`].

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

/// Build the Yap "pac" tray icon (open-mouth circle + sound waves on a
/// transparent background) for the given state. The body colour signals state:
/// yellow idle, red recording, amber processing, grey needs-model. Drawn at
/// 128px so Windows can scale it down crisply.
fn state_icon(state: &str) -> Image<'static> {
    let (br, bg, bb) = match state {
        "recording" => (239.0f32, 68.0, 68.0),     // red
        "processing" => (245.0f32, 158.0, 11.0),   // amber
        "needs-model" => (156.0f32, 163.0, 175.0), // grey
        _ => (251.0f32, 191.0, 36.0),              // yellow (idle)
    };
    let size: i32 = 128;
    let mut rgba = vec![0u8; (size * size * 4) as usize];

    let (cx, cy, r) = (52.0f32, 64.0f32, 44.0f32);
    let mouth = 0.62f32; // half-angle of the mouth opening (radians), facing +x
    let (eye_x, eye_y, eye_r) = (50.0f32, 42.0f32, 7.0f32);
    let waves = [54.0f32, 66.0f32];
    let wave_hw = 4.0f32;
    let wave_ang = mouth * 0.8;
    let clamp01 = |v: f32| v.max(0.0).min(1.0);

    for y in 0..size {
        for x in 0..size {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let dx = px - cx;
            let dy = py - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let ang = dy.atan2(dx).abs();

            let (mut cr, mut cg, mut cb, mut ca) = (0.0f32, 0.0, 0.0, 0.0);
            if dist <= r + 1.0 {
                // Pac body = circle minus the mouth wedge.
                let circ = clamp01(r - dist + 0.5);
                let mouth_mask = clamp01((ang - mouth) / 0.10 + 0.5);
                ca = circ * mouth_mask;
                cr = br;
                cg = bg;
                cb = bb;
                // Eye, composited over the body only.
                let ed = ((px - eye_x).powi(2) + (py - eye_y).powi(2)).sqrt();
                let eye_a = clamp01(eye_r - ed + 0.5) * ca;
                cr = cr * (1.0 - eye_a) + 30.0 * eye_a;
                cg = cg * (1.0 - eye_a) + 58.0 * eye_a;
                cb = cb * (1.0 - eye_a) + 138.0 * eye_a;
            } else {
                // Sound-wave arcs radiating from the mouth.
                let mut wa = 0.0f32;
                for &wr in waves.iter() {
                    let d = (dist - wr).abs();
                    let radial = clamp01(wave_hw - d + 0.5);
                    let angular = clamp01((wave_ang - ang) / 0.12 + 0.5);
                    wa = wa.max(radial * angular);
                }
                if wa > 0.0 {
                    cr = 255.0;
                    cg = 255.0;
                    cb = 255.0;
                    ca = wa;
                }
            }

            let i = ((y * size + x) * 4) as usize;
            rgba[i] = cr as u8;
            rgba[i + 1] = cg as u8;
            rgba[i + 2] = cb as u8;
            rgba[i + 3] = (ca * 255.0) as u8;
        }
    }
    Image::new_owned(rgba, size as u32, size as u32)
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
    let tray = TrayIconBuilder::with_id("yap-tray")
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

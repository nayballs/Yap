//! System-tray icon + right-click menu, modelled on Handy's tray.
//!
//! - A **state-aware icon** (the "pac" mark: amber idle, red recording, amber
//!   processing, grey needs-model) generated at runtime — no image assets.
//! - A **right-click menu** that changes with state: when idle it offers a
//!   **model submenu** (switch the active model, checkmark on the current one);
//!   while recording/processing it offers **Cancel**. Always: Settings + Quit.
//! - **Updates** (updates.rs): a ready update adds "Restart to update to X"
//!   under the version line, a small green dot on the icon and a note in the
//!   tooltip — Windows Update's own "restart required" idiom.
//! - **Calls** (meeting_detect.rs): while a detected call is live and nothing
//!   records, "Record this Teams call" sits at the top of the idle menu.
//! - **Left-click** opens Settings.
//!
//! The tray is rebuilt on every `yap-state` change via [`update_tray`], and on
//! update-status and call-detection changes via [`refresh`].

use std::sync::Mutex;

use crate::config;
use crate::stt;
use crate::AppState;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

/// Stable tray id — the app-registry key used by `tray_by_id`/`remove_tray_by_id`.
const TRAY_ID: &str = "yap-tray";

/// Long-lived tray menus, swapped on state change instead of rebuilt.
/// Rebuilding native menus on EVERY `yap-state` event (the old behaviour)
/// churned thousands of Win32 menu/icon handles over a long session, which is
/// exactly what makes the Windows notification area glitch or drop the icon.
struct MenuCache {
    /// Idle menu (model submenu). Rebuilt only when `idle_key` changes.
    idle: Menu<Wry>,
    /// Recording/processing menu (Cancel).
    recording: Menu<Wry>,
    /// What the idle menu was built from: active model + installed set.
    idle_key: String,
    /// Which of the two menus is currently installed on the tray.
    showing_recording: bool,
}

static MENUS: Mutex<Option<MenuCache>> = Mutex::new(None);
/// What the icon was last drawn for (`state|badge`), so repeats are no-ops.
static LAST_ICON_STATE: Mutex<String> = Mutex::new(String::new());
/// The last pipeline state seen, so [`refresh`] can redraw for it.
static LAST_STATE: Mutex<String> = Mutex::new(String::new());

fn version_label() -> String {
    format!("Yap v{}", env!("CARGO_PKG_VERSION"))
}

fn tooltip() -> String {
    match crate::updates::tray_note() {
        Some(note) => format!("{} · {}", version_label(), note),
        None => version_label(),
    }
}

/// Build the Yap "pac" tray icon (open-mouth circle + sound waves on a
/// transparent background) for the given state. The body colour signals state:
/// brand amber idle (matches `src/assets/yap-logo.svg`), red recording, bright
/// amber processing, grey needs-model. `badge` adds a green dot top-right (an
/// update is ready). Drawn at 128px so Windows can scale it down crisply.
fn state_icon(state: &str, badge: bool) -> Image<'static> {
    let (br, bg, bb) = match state {
        "recording" => (239.0f32, 68.0, 68.0), // red
        "processing" | "processing-slow" => (245.0f32, 158.0, 11.0), // amber
        "needs-model" => (156.0f32, 163.0, 175.0), // grey
        _ => (240.0f32, 176.0, 74.0),           // brand amber (idle, logo #f0b04a)
    };
    let size: i32 = 128;
    let mut rgba = vec![0u8; (size * size * 4) as usize];

    let (cx, cy, r) = (52.0f32, 64.0f32, 44.0f32);
    let mouth = 0.62f32; // half-angle of the mouth opening (radians), facing +x
    let (eye_x, eye_y, eye_r) = (50.0f32, 42.0f32, 7.0f32);
    let waves = [54.0f32, 66.0f32];
    let wave_hw = 4.0f32;
    let wave_ang = mouth * 0.8;
    let clamp01 = |v: f32| v.clamp(0.0, 1.0);

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
                // Eye, composited over the body only (warm ink, logo #1f1c16).
                let ed = ((px - eye_x).powi(2) + (py - eye_y).powi(2)).sqrt();
                let eye_a = clamp01(eye_r - ed + 0.5) * ca;
                cr = cr * (1.0 - eye_a) + 31.0 * eye_a;
                cg = cg * (1.0 - eye_a) + 28.0 * eye_a;
                cb = cb * (1.0 - eye_a) + 22.0 * eye_a;
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

            // Update badge: a green dot in a warm-ink ring, composited on top
            // (the ring keeps it legible on light and dark taskbars alike).
            if badge {
                let bd = ((px - 106.0).powi(2) + (py - 22.0).powi(2)).sqrt();
                let ring = clamp01(21.0 - bd + 0.5);
                let dot = clamp01(15.0 - bd + 0.5);
                if ring > 0.0 {
                    // Ink ring (logo #1f1c16) → green dot (#22c55e).
                    let mix = |ink: f32, green: f32| ink * (1.0 - dot) + green * dot;
                    let (dr, dg, db) = (mix(31.0, 34.0), mix(28.0, 197.0), mix(22.0, 94.0));
                    cr = cr * (1.0 - ring) + dr * ring;
                    cg = cg * (1.0 - ring) + dg * ring;
                    cb = cb * (1.0 - ring) + db * ring;
                    ca = ca.max(ring);
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

    let version = MenuItem::with_id(app, "version", version_label(), false, None::<&str>)?;
    // Opens the main window (the control panel — Home feed + surfaces; the
    // Settings modal lives inside it).
    let settings = MenuItem::with_id(app, "settings", "Open Yap", true, settings_accel)?;
    let check_updates =
        MenuItem::with_id(app, "check_updates", "Check for updates…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Yap", true, quit_accel)?;
    let sep = || PredefinedMenuItem::separator(app);

    let recording = matches!(state, "recording" | "processing" | "processing-slow");

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

    let models: Box<dyn IsMenuItem<Wry>> = if installed.is_empty() {
        // No models yet — offer a way into the model picker.
        Box::new(MenuItem::with_id(app, "open_models", "Download a model…", true, None::<&str>)?)
    } else {
        let label = if installed.contains(&current) {
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
        Box::new(submenu)
    };

    // A call to record ("Record this Teams call") and an update the user can
    // act on ("Restart to update to 0.1.2") go right under the version line;
    // "Check for updates…" steps aside then.
    let call = crate::meeting_detect::tray_item()
        .map(|(id, label)| MenuItem::with_id(app, id, label, true, None::<&str>))
        .transpose()?;
    let update = crate::updates::tray_item()
        .map(|(id, label, enabled)| MenuItem::with_id(app, id, label, enabled, None::<&str>))
        .transpose()?;
    let (sep_update, sep_models, sep_quit) = (sep()?, sep()?, sep()?);
    let mut items: Vec<&dyn IsMenuItem<Wry>> = vec![&version, &sep_update];
    if let Some(call) = &call {
        items.push(call);
    }
    if let Some(update) = &update {
        items.push(update);
    }
    if call.is_some() || update.is_some() {
        items.push(&sep_models);
    }
    items.push(models.as_ref());
    let sep_settings = sep()?;
    items.push(&sep_settings);
    items.push(&settings);
    if update.is_none() {
        items.push(&check_updates);
    }
    items.push(&sep_quit);
    items.push(&quit);
    Menu::with_items(app, &items)
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
        "check_updates" => crate::updates::on_tray_check(app),
        "update_install" | "update_get" => {
            // Off the main thread: installing takes the pipeline lock (to
            // check for a dictation) and may open a window.
            let app = app.clone();
            let id = id.to_string();
            std::thread::spawn(move || crate::updates::on_tray_menu(&app, &id));
        }
        "open_models" => {
            let _ = crate::commands::show_onboarding(app);
        }
        other if other.starts_with("meeting_record:") => {
            // Off the main thread: starting the recorder blocks for a moment
            // and looks at the main window.
            if let Some(call_id) = other.strip_prefix("meeting_record:").and_then(|n| n.parse().ok()) {
                let app = app.clone();
                std::thread::spawn(move || crate::meeting_detect::on_tray_record(&app, call_id));
            }
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

/// The active model + installed set + update and call items the idle menu
/// depends on. When this changes (model switched/downloaded/deleted, an
/// update got ready, a call to record came or went) the idle menu is stale.
fn idle_menu_key() -> String {
    let data_dir = config::data_dir();
    let installed: Vec<&str> = stt::all_model_ids()
        .into_iter()
        .filter(|id| stt::is_model_installed(&data_dir, id))
        .collect();
    let update = crate::updates::tray_item()
        .map(|(id, label, _)| format!("{id}:{label}"))
        .unwrap_or_default();
    let call = crate::meeting_detect::tray_item()
        .map(|(id, label)| format!("{id}:{label}"))
        .unwrap_or_default();
    format!("{}|{}|{}|{}", config::load().model_size, installed.join(","), update, call)
}

/// Build the tray icon and install it. The app keeps the tray in its registry
/// (`tray_by_id(TRAY_ID)`); we deliberately do NOT also stash it in managed
/// state, so `ensure_tray` can remove and rebuild it at runtime.
pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app, "idle")?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(state_icon("idle", crate::updates::tray_badge()))
        .tooltip(tooltip())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| on_menu_event(app, event.id.as_ref()))
        .on_tray_icon_event(|tray, event| {
            // Left-click opens Settings.
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
    Ok(())
}

/// Reconcile the tray with the current config. Since the pill was retired
/// (2026-07-09) the tray is Yap's only persistent surface, so it is always
/// built regardless of `show_tray_icon` — without it Settings would be
/// unreachable. Called at startup AND from save_config.
pub fn ensure_tray(app: &AppHandle, cfg: &config::YapConfig) {
    let _ = cfg; // kept for signature stability (config no longer gates the tray)
    let desired = true;
    let exists = app.tray_by_id(TRAY_ID).is_some();
    if desired && !exists {
        // Fresh tray → the cached menus/icon-state belong to the old one.
        *MENUS.lock().unwrap_or_else(|p| p.into_inner()) = None;
        LAST_ICON_STATE
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
        if let Err(e) = build_tray(app) {
            tracing::warn!("Failed to build tray: {}", e);
        }
    } else if !desired && exists {
        app.remove_tray_by_id(TRAY_ID);
    }
}

/// Update the tray icon + menu for a new state. No-op if the tray isn't built.
/// Cheap by design: the icon is only re-rendered when the state actually
/// changed, and the two menus are cached and swapped — a full native-menu
/// rebuild happens only when the model list/selection or update item changes.
///
/// All tray work is posted to the main thread and runs there in order: tray
/// and menu calls made from other threads block until the main thread runs
/// them, so a worker holding the menu lock while the main thread waited on
/// that same lock (a sync command's `yap-state`, an update refresh) would
/// deadlock.
pub fn update_tray(app: &AppHandle, state: &str) {
    let handle = app.clone();
    let state = state.to_string();
    let _ = app.run_on_main_thread(move || {
        *LAST_STATE.lock().unwrap_or_else(|p| p.into_inner()) = state.clone();
        let mut guard = MENUS.lock().unwrap_or_else(|p| p.into_inner());
        apply(&handle, &mut guard, &state);
    });
}

/// Re-apply the update and call items, icon dot and tooltip after an
/// update-status change (updates.rs) or a call-detection change
/// (meeting_detect.rs), for the current pipeline state. Main thread, like
/// [`update_tray`].
pub fn refresh(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(tray) = handle.tray_by_id(TRAY_ID) {
            let _ = tray.set_tooltip(Some(tooltip()));
        }
        let state = LAST_STATE.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let mut guard = MENUS.lock().unwrap_or_else(|p| p.into_inner());
        apply(&handle, &mut guard, if state.is_empty() { "idle" } else { &state });
    });
}

/// The body of [`update_tray`] / [`refresh`]: main thread, under the menu lock.
fn apply(app: &AppHandle, guard: &mut Option<MenuCache>, state: &str) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };

    // Icon: `yap-state` repeats states; only touch the tray when it changed.
    {
        let badge = crate::updates::tray_badge();
        let key = format!("{state}|{badge}");
        let mut last = LAST_ICON_STATE.lock().unwrap_or_else(|p| p.into_inner());
        if *last != key {
            let _ = tray.set_icon(Some(state_icon(state, badge)));
            tracing::debug!(icon = %key, "tray: icon updated");
            *last = key;
        }
    }

    let recording = matches!(state, "recording" | "processing" | "processing-slow");

    if guard.is_none() {
        match (build_menu(app, "idle"), build_menu(app, "recording")) {
            (Ok(idle), Ok(rec)) => {
                *guard = Some(MenuCache {
                    idle,
                    recording: rec,
                    idle_key: idle_menu_key(),
                    // Force the install below by claiming the opposite menu.
                    showing_recording: !recording,
                });
            }
            _ => return,
        }
    }
    let cache = guard.as_mut().expect("just initialised");

    // Stale idle menu (model switched/downloaded/deleted, update item
    // changed) → rebuild it once.
    if !recording {
        let key = idle_menu_key();
        if cache.idle_key != key {
            if let Ok(menu) = build_menu(app, "idle") {
                tracing::debug!(menu = %key, "tray: idle menu rebuilt");
                cache.idle = menu;
                cache.idle_key = key;
                cache.showing_recording = true; // force reinstall below
            }
        }
    }

    if cache.showing_recording != recording {
        let menu = if recording {
            cache.recording.clone()
        } else {
            cache.idle.clone()
        };
        let _ = tray.set_menu(Some(menu));
        cache.showing_recording = recording;
    }
}

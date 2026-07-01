//! Yap — a tiny local voice dictation pill.
//!
//! Press a global hotkey, speak, press again: Yap transcribes locally with
//! Whisper and types the text into whatever window is focused. A chime marks
//! recording start/stop, and a correction dictionary fixes mis-heard jargon.
//!
//! The dictation engine (input hook, STT, text injection) is ported verbatim
//! from Voice Mirror; everything else here is the slim glue.

mod commands;
mod config;
mod history;
mod input_hook;
mod llm;
mod mute;
mod overlay;
mod pipeline;
mod portable;
mod selection;
mod sound;
mod stt;
mod text_injector;
mod tray;
mod usage;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Listener, Manager};
use tauri_plugin_autostart::ManagerExt;

/// Whether a recording/processing overlay is meant to be on screen. A background
/// thread re-asserts "always on top" while this is true, so the overlay (and the
/// pill, if shown) can't get buried behind another topmost/fullscreen window
/// mid-recording — which would leave the user unaware a recording is live.
static OVERLAY_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Bumped on every `yap-state` change so a scheduled auto-clear of the
/// transient "error" state is cancelled if a newer state arrives first.
static STATE_GEN: AtomicU64 = AtomicU64::new(0);

/// Shared app state: the running dictation pipeline.
pub struct AppState {
    pub pipeline: Mutex<Option<pipeline::Pipeline>>,
}

/// Enable or disable OS autostart via `tauri-plugin-autostart`.
/// Kept as a free function so `commands::set_autostart` can delegate here.
pub fn set_autostart_enabled(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let manager = app.autolaunch();
    let res = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
    res.map_err(|e| format!("Failed to set autostart: {}", e))
}

pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .try_init();

    // Decide portable-vs-installed once, before anything reads the data dir.
    portable::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|_app, _argv, _cwd| {}))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // Auto-update from GitHub Releases (driven from the frontend JS plugin)
        // + process for relaunch after install. Desktop-only.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(AppState {
            pipeline: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            commands::toggle_recording,
            commands::get_config,
            commands::save_config,
            commands::download_model,
            commands::download_model_size,
            commands::installed_models,
            commands::open_settings,
            commands::open_onboarding,
            commands::close_onboarding,
            commands::list_audio_devices,
            commands::list_output_devices,
            commands::model_language_info,
            commands::configure_hotkey,
            commands::configure_edit_hotkey,
            commands::set_pill_scale,
            commands::set_active_model,
            commands::delete_model,
            commands::cancel_recording,
            commands::set_autostart,
            commands::set_pill_visible,
            commands::is_portable,
            commands::test_post_process,
            commands::get_groq_usage,
            commands::get_history,
            commands::clear_history,
            commands::get_stats,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let cfg = config::load();

            // Register the app handle so the usage tracker can emit live
            // `groq-usage` updates after each AI-cleanup call.
            usage::set_app_handle(handle.clone());

            // Global input hook + dictation hotkey.
            input_hook::start_input_hook(handle.clone());
            if let Err(e) = input_hook::configure_dictation(&cfg.hotkey) {
                tracing::warn!("Failed to configure hotkey: {}", e);
            }
            // Optional edit/rewrite-mode hotkey (empty = unbound / opt-in).
            if let Err(e) = input_hook::configure_edit(&cfg.edit_hotkey) {
                tracing::warn!("Failed to configure edit hotkey: {}", e);
            }

            // Clear ort's 0-byte DirectML.dll stub so ONNX uses the real system
            // DirectML, then fix the transcribe-rs accelerator policy before any
            // model loads: whisper → CUDA (Auto), ONNX → DirectML. No-ops in stub.
            stt::fix_directml_stub();
            stt::apply_accelerator_settings(cfg.use_gpu);

            // Start the dictation pipeline (audio capture + STT engine).
            match pipeline::Pipeline::start(handle.clone(), cfg.clone()) {
                Ok(p) => {
                    if let Ok(mut guard) = app.state::<AppState>().pipeline.lock() {
                        *guard = Some(p);
                    }
                }
                Err(e) => tracing::error!("Failed to start pipeline: {}", e),
            }

            // Apply the saved pill size.
            commands::apply_pill_scale(&handle, cfg.pill_scale);

            // Make the overlay click-through + topmost so it floats above the
            // focused window without ever stealing the cursor.
            if let Some(w) = app.get_webview_window("overlay") {
                let _ = w.set_ignore_cursor_events(true);
                let _ = w.set_always_on_top(true);
            }

            // Honour the saved pill visibility (dictation still works hidden).
            // The pill is hidden by default — the bottom overlay gives on-speak
            // feedback and the tray opens Settings.
            if let Some(pill) = app.get_webview_window("pill") {
                if cfg.show_pill {
                    let _ = pill.show();
                } else {
                    let _ = pill.hide();
                }
                tracing::info!(show_pill = cfg.show_pill, "Applied initial pill visibility");
            }

            // Route hotkey press/release into the pipeline. Done Rust-side so the
            // core loop doesn't depend on the pill webview being ready. The
            // pipeline picks toggle vs push-to-talk live from its config, so
            // both events go through `on_key`.
            let press_handle = handle.clone();
            handle.listen("dictation-key-pressed", move |_event| {
                let state = press_handle.state::<AppState>();
                let pipeline = state.pipeline.lock();
                if let Ok(guard) = pipeline {
                    if let Some(p) = guard.as_ref() {
                        p.on_key(true);
                    }
                }
            });
            let release_handle = handle.clone();
            handle.listen("dictation-key-released", move |_event| {
                let state = release_handle.state::<AppState>();
                let pipeline = state.pipeline.lock();
                if let Ok(guard) = pipeline {
                    if let Some(p) = guard.as_ref() {
                        p.on_key(false);
                    }
                }
            });

            // Route the edit/rewrite hotkey the same way (its own event pair).
            let edit_press_handle = handle.clone();
            handle.listen("edit-key-pressed", move |_event| {
                let state = edit_press_handle.state::<AppState>();
                let pipeline = state.pipeline.lock();
                if let Ok(guard) = pipeline {
                    if let Some(p) = guard.as_ref() {
                        p.on_edit_key(true);
                    }
                }
            });
            let edit_release_handle = handle.clone();
            handle.listen("edit-key-released", move |_event| {
                let state = edit_release_handle.state::<AppState>();
                let pipeline = state.pipeline.lock();
                if let Ok(guard) = pipeline {
                    if let Some(p) = guard.as_ref() {
                        p.on_edit_key(false);
                    }
                }
            });

            // Drive the bottom-center overlay from the pipeline's `yap-state`
            // event (decoupled from the pipeline itself). Show it while
            // recording/processing if the user has the overlay enabled; hide it
            // otherwise. State changes are infrequent, so re-reading the saved
            // config here is fine.
            let overlay_handle = handle.clone();
            handle.listen("yap-state", move |event| {
                let state = event.payload().trim_matches('"'); // payload is a JSON string
                let generation = STATE_GEN.fetch_add(1, Ordering::Relaxed) + 1;
                // Show the overlay while recording/processing, and briefly on error.
                let show = matches!(state, "recording" | "processing" | "error");
                if show {
                    if config::load().show_overlay {
                        overlay::show_overlay(&overlay_handle);
                    }
                } else {
                    overlay::hide_overlay(&overlay_handle);
                }
                OVERLAY_ACTIVE.store(show, Ordering::Relaxed);
                // Keep the tray icon + menu in sync with the recording state.
                tray::update_tray(&overlay_handle, state);

                // The "error" state is transient: auto-clear it back to idle a
                // few seconds later, unless a newer state has arrived since.
                if state == "error" {
                    let h = overlay_handle.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_secs(4));
                        if STATE_GEN.load(Ordering::Relaxed) == generation {
                            let _ = h.emit("yap-state", "idle");
                        }
                    });
                }
            });

            // While recording/processing, keep the overlay (and the pill, if
            // shown) genuinely on top: re-assert "always on top" a few times a
            // second so another app's topmost or fullscreen window can't bury it
            // and leave the user unaware a recording is live.
            let topmost_handle = handle.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_millis(350));
                if !OVERLAY_ACTIVE.load(Ordering::Relaxed) {
                    continue;
                }
                for label in ["overlay", "pill"] {
                    if let Some(w) = topmost_handle.get_webview_window(label) {
                        if w.is_visible().unwrap_or(false) {
                            overlay::force_topmost(&w);
                        }
                    }
                }
            });

            // System tray (Handy-style: state-aware icon + model submenu).
            // Built when the user wants it, OR whenever the pill is hidden —
            // otherwise there'd be no way to reach Settings (no pill gear).
            if cfg.show_tray_icon || !cfg.show_pill {
                if let Err(e) = tray::build_tray(app.handle()) {
                    tracing::warn!("Failed to build tray: {}", e);
                }
            }

            // Reconcile OS autostart state with the saved config.
            // Only touch OS autostart when the desired state differs from the
            // current one. Avoids a spurious "disable" call that errors in dev
            // (the dev exe was never registered) and spams the log on launch.
            let autostart_now = app.autolaunch().is_enabled().unwrap_or(false);
            if autostart_now != cfg.autostart {
                if let Err(e) = set_autostart_enabled(&handle, cfg.autostart) {
                    tracing::debug!("autostart reconcile skipped: {}", e);
                }
            }

            // Closing the settings / onboarding windows hides them (so they can
            // reopen) instead of destroying them — the pill window stays the
            // app's lifetime.
            for label in ["settings", "onboarding"] {
                if let Some(win) = app.get_webview_window(label) {
                    let w = win.clone();
                    win.on_window_event(move |event| {
                        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                            api.prevent_close();
                            let _ = w.hide();
                        }
                    });
                }
            }

            // First run: if no model is downloaded yet, greet the user with the
            // onboarding model picker instead of a silent "needs-model" pill.
            // Suppressed when launched hidden (e.g. autostart at login).
            if !cfg.start_hidden && commands::installed_models().is_empty() {
                if let Err(e) = commands::show_onboarding(&handle) {
                    tracing::warn!("Could not show onboarding: {}", e);
                }
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Yap");
}

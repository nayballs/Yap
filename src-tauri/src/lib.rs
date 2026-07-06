//! Yap — a tiny local voice dictation pill.
//!
//! Press a global hotkey, speak, press again: Yap transcribes locally with
//! Whisper and types the text into whatever window is focused. A chime marks
//! recording start/stop, and a correction dictionary fixes mis-heard jargon.
//!
//! The dictation engine (input hook, STT, text injection) is ported verbatim
//! from Voice Mirror; everything else here is the slim glue.

mod agent_detect;
mod chats;
mod commands;
mod config;
mod media;
mod meeting;
mod notes;
mod tools;
mod history;
mod input_hook;
mod llm;
mod local_llm;
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

/// Reload handle for the global log filter — lets the Settings "Debug mode"
/// toggle (OpenWhispr's Debug Logging section) bump verbosity at runtime
/// without a restart. See `set_debug_logging`.
static LOG_RELOAD: std::sync::OnceLock<
    tracing_subscriber::reload::Handle<
        tracing_subscriber::EnvFilter,
        tracing_subscriber::Registry,
    >,
> = std::sync::OnceLock::new();

/// The filter directives for normal vs debug mode. Debug raises YAP's own
/// crate to `debug` while keeping dependencies at `info` (whisper/ort/reqwest
/// at debug would drown the file).
fn log_directives(debug: bool) -> &'static str {
    if debug {
        "info,yap_lib=debug"
    } else {
        "info"
    }
}

/// Live-switch the log verbosity (the Settings → Advanced → Debug Logging
/// toggle; persisted as `config.debug_logging` and re-applied on save/startup).
/// An explicit RUST_LOG env var always wins (it was applied at init and this
/// is only called from config paths when the value CHANGES).
pub(crate) fn set_debug_logging(enabled: bool) {
    if let Some(handle) = LOG_RELOAD.get() {
        let _ = handle.reload(tracing_subscriber::EnvFilter::new(log_directives(enabled)));
        // NB: the identifier `debug` must not appear inside tracing macros —
        // it resolves to the macro's own level shorthand and breaks the build.
        let state = if enabled { "enabled" } else { "disabled" };
        tracing::info!(debug_mode = enabled, "Debug logging {state}");
    }
}

/// Initialise tracing to BOTH stdout (for `tauri dev`) and a rolling file in the
/// data dir. Installed builds are windowed with no console, so stdout logs are
/// invisible — the file log is the only way to diagnose a shipped build (e.g.
/// the "stuck on transcribing" report was a CPU-only whisper build with no
/// visible logs). Must run AFTER `portable::init()` so the data dir resolves.
/// The filter sits behind a reload layer so "Debug mode" can raise it live.
fn init_logging() {
    use tracing_subscriber::prelude::*;

    // Startup level: RUST_LOG wins, else the persisted Debug-mode setting.
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            tracing_subscriber::EnvFilter::new(log_directives(config::load().debug_logging))
        });
    let (filter, reload_handle) = tracing_subscriber::reload::Layer::new(filter);
    let _ = LOG_RELOAD.set(reload_handle);

    let log_dir = config::data_dir().join("logs");
    let file_layer = match std::fs::create_dir_all(&log_dir) {
        Ok(_) => {
            let appender = tracing_appender::rolling::daily(&log_dir, "yap.log");
            let (nb, guard) = tracing_appender::non_blocking(appender);
            // The writer's flush guard must outlive the app; the process owns it
            // for its whole lifetime, so leaking it is intentional.
            std::mem::forget(guard);
            Some(
                tracing_subscriber::fmt::layer()
                    .with_ansi(false)
                    .with_writer(nb),
            )
        }
        Err(_) => None, // no data dir writable — fall back to stdout only
    };

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer()) // stdout (dev)
        .with(file_layer)
        .try_init();

    // Route panics into the log file too — a panicking thread otherwise dies
    // silently in a windowed build (and even in dev the console scrolls away).
    // A native access violation can't be caught here, but every Rust-level
    // panic now leaves a trace with its location.
    std::panic::set_hook(Box::new(|info| {
        let msg = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<non-string panic payload>".into());
        let loc = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown>".into());
        tracing::error!(location = %loc, "PANIC: {}", msg);
    }));
}

pub fn run() {
    // Decide portable-vs-installed once, before anything reads the data dir
    // (the file log lives under it).
    portable::init();
    init_logging();
    tracing::info!(
        engines = cfg!(feature = "engines"),
        version = env!("CARGO_PKG_VERSION"),
        "Yap starting — build capabilities (GPU whisper via Vulkan, ONNX via DirectML; falls back to CPU with no GPU)"
    );

    let builder = tauri::Builder::default();

    // Single-instance: RELEASE builds only. Dev builds must coexist with an
    // installed Yap — with the plugin on, a `tauri dev` instance pings the
    // running installed app and silently exits within seconds, which broke
    // every "run the dev build" workflow (incl. Voice Mirror's App Preview).
    // On a duplicate release launch, surface the app instead of doing nothing.
    #[cfg(not(debug_assertions))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
        use tauri::Manager;
        if let Some(settings) = app.get_webview_window("settings") {
            let _ = settings.show();
            let _ = settings.unminimize();
            let _ = settings.set_focus();
        }
    }));

    builder
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // Auto-update from GitHub Releases (driven from the frontend JS plugin)
        // + process for relaunch after install. Desktop-only.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        // External links ("Get your API key", GitHub, Learn more) open in the
        // default browser — target=_blank does nothing in a Tauri webview.
        .plugin(tauri_plugin_opener::init())
        // Native file-open dialog for the Upload surface's Browse button.
        .plugin(tauri_plugin_dialog::init())
        // Remember the MAIN window's size/position across launches. Only the
        // "settings" window is managed: the pill/overlay are positioned
        // programmatically, and onboarding is one-shot — restoring stale
        // bounds would misplace them. Flags exclude VISIBLE so the window
        // never un-hides itself on a start-hidden launch.
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::SIZE
                        | tauri_plugin_window_state::StateFlags::POSITION
                        | tauri_plugin_window_state::StateFlags::MAXIMIZED,
                )
                .with_denylist(&["pill", "overlay", "onboarding"])
                .build(),
        )
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
            commands::set_mic_test,
            commands::set_input_device,
            commands::frontend_log,
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
            commands::get_base_prompt,
            commands::get_edit_base_prompt,
            commands::local_llm_status,
            commands::local_llm_start,
            commands::local_llm_stop,
            commands::local_llm_install,
            commands::local_llm_delete,
            commands::open_llm_folder,
            commands::get_groq_usage,
            commands::get_history,
            commands::clear_history,
            commands::delete_history_entry,
            commands::get_stats,
            commands::transcribe_file,
            commands::cancel_file_transcription,
            commands::audio_file_info,
            commands::notes_list,
            commands::note_get,
            commands::note_create,
            commands::note_update,
            commands::note_delete,
            commands::note_enhance,
            commands::get_note_base_prompt,
            commands::notes_folders,
            commands::notes_folder_create,
            commands::notes_actions,
            commands::action_create,
            commands::action_update,
            commands::action_delete,
            commands::log_info,
            commands::open_logs_folder,
            commands::meeting_start,
            commands::meeting_stop,
            commands::meeting_state,
            commands::note_export,
            commands::note_ask,
            commands::chats_list,
            commands::chat_get,
            commands::chat_delete,
            commands::chat_send,
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

            // Clear any orphaned cleanup sidecar from a previous session (the
            // updater force-exits without running the Exit handler), then — if
            // on-device cleanup is selected + installed — warm up a fresh one
            // off-thread so the first dictation cleanup skips the cold load.
            local_llm::kill_orphans();
            {
                let cfg2 = cfg.clone();
                tauri::async_runtime::spawn(async move {
                    local_llm::autostart_if_configured(&cfg2).await;
                });
            }

            // Apply the saved pill size.
            commands::apply_pill_scale(&handle, cfg.pill_scale);

            // Make the overlay click-through + topmost so it floats above the
            // focused window without ever stealing the cursor.
            if let Some(w) = app.get_webview_window("overlay") {
                let _ = w.set_ignore_cursor_events(true);
                let _ = w.set_always_on_top(true);
            }

            // Dev builds: show Settings on launch. Every Yap window is hidden
            // at startup by design (tray-first UX), which leaves dev runs — and
            // anything trying to preview/capture the app, like Voice Mirror's
            // App Preview — with literally nothing on screen.
            #[cfg(debug_assertions)]
            if let Some(settings) = app.get_webview_window("settings") {
                let _ = settings.show();
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
            tray::ensure_tray(app.handle(), &cfg);

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

            // WebView2 subsystems initialized against a HIDDEN window can come
            // up permanently broken — most relevantly the Rust→JS event/eval
            // delivery channel (same created-hidden bug family as
            // tauri-apps/tauri#3654 and wry#1639; drag-drop got fixed in
            // wry#1638, the rest of the surface was never audited). One-shot
            // show+hide, parked off-screen so nothing flashes, forces those
            // windows to finish initialization while VISIBLE.
            for label in ["onboarding", "settings"] {
                if let Some(w) = app.get_webview_window(label) {
                    let orig = w.outer_position().ok();
                    let _ = w.set_position(tauri::PhysicalPosition::new(-32000, -32000));
                    let _ = w.show();
                    let _ = w.hide();
                    if let Some(p) = orig {
                        let _ = w.set_position(p);
                    } else {
                        let _ = w.center();
                    }
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
        .build(tauri::generate_context!())
        .expect("error while building Yap")
        .run(|_app_handle, event| {
            // Kill the on-device cleanup sidecar when the app exits so no
            // orphaned llamafile server is left running.
            if let tauri::RunEvent::Exit = event {
                local_llm::stop();
            }
        });
}

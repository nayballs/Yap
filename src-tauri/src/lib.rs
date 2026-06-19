//! Blip — a tiny local voice dictation pill.
//!
//! Press a global hotkey, speak, press again: Blip transcribes locally with
//! Whisper and types the text into whatever window is focused. A chime marks
//! recording start/stop, and a correction dictionary fixes mis-heard jargon.
//!
//! The dictation engine (input hook, STT, text injection) is ported verbatim
//! from Voice Mirror; everything else here is the slim glue.

mod commands;
mod config;
mod input_hook;
mod pipeline;
mod sound;
mod stt;
mod text_injector;

use std::sync::Mutex;
use tauri::{Listener, Manager};

/// Shared app state: the running dictation pipeline.
pub struct AppState {
    pub pipeline: Mutex<Option<pipeline::Pipeline>>,
}

pub fn run() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .try_init();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|_app, _argv, _cwd| {}))
        .manage(AppState {
            pipeline: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            commands::toggle_recording,
            commands::get_config,
            commands::save_config,
            commands::download_model,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let cfg = config::load();

            // Global input hook + dictation hotkey.
            input_hook::start_input_hook(handle.clone());
            if let Err(e) = input_hook::configure_dictation(&cfg.hotkey) {
                tracing::warn!("Failed to configure hotkey: {}", e);
            }

            // Start the dictation pipeline (audio capture + STT engine).
            match pipeline::Pipeline::start(handle.clone(), cfg.clone()) {
                Ok(p) => {
                    if let Ok(mut guard) = app.state::<AppState>().pipeline.lock() {
                        *guard = Some(p);
                    }
                }
                Err(e) => tracing::error!("Failed to start pipeline: {}", e),
            }

            // Toggle recording when the hotkey fires. Done Rust-side so the
            // core loop doesn't depend on the pill webview being ready.
            let toggle_handle = handle.clone();
            handle.listen("dictation-key-pressed", move |_event| {
                let state = toggle_handle.state::<AppState>();
                let pipeline = state.pipeline.lock();
                if let Ok(guard) = pipeline {
                    if let Some(p) = guard.as_ref() {
                        p.toggle();
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Blip");
}

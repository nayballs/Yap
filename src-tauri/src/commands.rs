//! Tauri commands exposed to the pill frontend.

use tauri::{AppHandle, State};

use crate::config::{self, BlipConfig};
use crate::stt;
use crate::AppState;

/// Toggle recording on/off (same action as the global hotkey).
#[tauri::command]
pub fn toggle_recording(state: State<'_, AppState>) {
    if let Ok(guard) = state.pipeline.lock() {
        if let Some(p) = guard.as_ref() {
            p.toggle();
        }
    }
}

/// Read the current config.
#[tauri::command]
pub fn get_config() -> BlipConfig {
    config::load()
}

/// Save config, re-apply the hotkey, and push it into the running pipeline.
#[tauri::command]
pub fn save_config(state: State<'_, AppState>, cfg: BlipConfig) -> Result<(), String> {
    config::save(&cfg)?;
    if let Err(e) = crate::input_hook::configure_dictation(&cfg.hotkey) {
        tracing::warn!("Failed to apply hotkey: {}", e);
    }
    if let Ok(guard) = state.pipeline.lock() {
        if let Some(p) = guard.as_ref() {
            p.update_config(cfg);
        }
    }
    Ok(())
}

/// Download the configured Whisper model, then load it into the pipeline.
#[tauri::command]
pub async fn download_model(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let cfg = config::load();
    let data_dir = config::data_dir();

    stt::ensure_model_exists(&data_dir, &cfg.model_size, Some(&app))
        .await
        .map_err(|e| e.to_string())?;

    let engine = stt::create_stt_engine("whisper-local", &data_dir, Some(&cfg.model_size), cfg.use_gpu)
        .map_err(|e| e.to_string())?;

    if let Ok(guard) = state.pipeline.lock() {
        if let Some(p) = guard.as_ref() {
            p.set_engine(engine);
        }
    }
    Ok(())
}

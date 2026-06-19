//! Tauri commands exposed to the pill frontend.

use tauri::{AppHandle, Emitter, LogicalSize, Manager, State};

use crate::config::{self, BlipConfig};
use crate::stt;
use crate::AppState;

/// Pill window dimensions at scale 1.0. Actual size = base * pill_scale.
pub const BASE_PILL_W: f64 = 210.0;
pub const BASE_PILL_H: f64 = 60.0;

/// Resize the pill window and tell its frontend to scale its content to match.
pub fn apply_pill_scale(app: &AppHandle, scale: f64) {
    let s = scale.clamp(0.6, 1.6);
    if let Some(pill) = app.get_webview_window("pill") {
        let _ = pill.set_size(LogicalSize::new(BASE_PILL_W * s, BASE_PILL_H * s));
    }
    let _ = app.emit("blip-scale", s);
}

/// Live pill resize (called from the settings slider).
#[tauri::command]
pub fn set_pill_scale(app: AppHandle, scale: f64) {
    apply_pill_scale(&app, scale);
}

/// Show the settings window (defined hidden in tauri.conf.json). Shared by the
/// `open_settings` command and the tray menu.
pub fn show_settings(app: &AppHandle) -> Result<(), String> {
    let w = app
        .get_webview_window("settings")
        .ok_or("settings window not found")?;
    let _ = w.show();
    let _ = w.set_focus();
    Ok(())
}

/// Open the settings window.
#[tauri::command]
pub fn open_settings(app: AppHandle) -> Result<(), String> {
    show_settings(&app)
}

/// List available microphone input device names.
#[tauri::command]
pub fn list_audio_devices() -> Vec<String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    host.input_devices()
        .map(|devs| devs.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

/// Apply a hotkey to the live input hook without persisting it.
/// Used by the settings key-recorder (pass "" to pause while choosing).
#[tauri::command]
pub fn configure_hotkey(spec: String) {
    let _ = crate::input_hook::configure_dictation(&spec);
}

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

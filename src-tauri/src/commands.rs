//! Tauri commands exposed to the pill frontend.

use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::config::{self, BlipConfig};
use crate::stt;
use crate::AppState;

/// Show (or create) the settings window. Shared by the `open_settings`
/// command and the tray menu.
pub fn show_settings(app: &AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("index.html".into()))
        .title("Blip Settings")
        .inner_size(470.0, 640.0)
        .min_inner_size(420.0, 480.0)
        .resizable(true)
        .center()
        .build()
        .map_err(|e| e.to_string())?;
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

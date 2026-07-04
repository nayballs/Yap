//! Tauri commands exposed to the pill frontend.

use tauri::{AppHandle, Emitter, LogicalSize, Manager, State};

use crate::config::{self, YapConfig};
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
    let _ = app.emit("yap-scale", s);
}

/// Live pill resize (called from the settings slider).
#[tauri::command]
pub fn set_pill_scale(app: AppHandle, scale: f64) {
    apply_pill_scale(&app, scale);
}

/// Show or hide the pill window live (called from the settings toggle).
/// Dictation keeps working when the pill is hidden. Persistence is handled
/// separately by `save_config`.
#[tauri::command]
pub fn set_pill_visible(app: AppHandle, visible: bool) {
    match app.get_webview_window("pill") {
        Some(pill) => {
            let res = if visible { pill.show() } else { pill.hide() };
            tracing::info!(visible, ok = res.is_ok(), "set_pill_visible");
        }
        None => tracing::warn!("set_pill_visible: pill window not found"),
    }
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

/// Show the first-run onboarding window (model picker). Shared by the
/// `open_onboarding` command and the first-run check in `setup`.
pub fn show_onboarding(app: &AppHandle) -> Result<(), String> {
    let w = app
        .get_webview_window("onboarding")
        .ok_or("onboarding window not found")?;
    let _ = w.show();
    let _ = w.set_focus();
    Ok(())
}

/// Open the onboarding / model-picker window.
#[tauri::command]
pub fn open_onboarding(app: AppHandle) -> Result<(), String> {
    show_onboarding(&app)
}

/// Hide the onboarding window (called when the user finishes first-run setup).
#[tauri::command]
pub fn close_onboarding(app: AppHandle) {
    if let Some(w) = app.get_webview_window("onboarding") {
        let _ = w.hide();
    }
}

/// Which registry models are already downloaded on disk (by model id).
#[tauri::command]
pub fn installed_models() -> Vec<String> {
    let data_dir = config::data_dir();
    stt::all_model_ids()
        .into_iter()
        .filter(|id| stt::is_model_installed(&data_dir, id))
        .map(|id| id.to_string())
        .collect()
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

/// List available audio output device names (for the chime).
#[tauri::command]
pub fn list_output_devices() -> Vec<String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    host.output_devices()
        .map(|devs| devs.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

/// Language/translation capability of a model, for the Settings UI. Lets the
/// frontend grey out the Language / Translate controls for models that don't
/// support them, and populate the language dropdown.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelLanguageInfo {
    pub supports_language: bool,
    pub supports_translate: bool,
    pub languages: Vec<String>,
}

/// Report whether a model supports language selection / translation and which
/// languages it exposes.
#[tauri::command]
pub fn model_language_info(model_size: String) -> ModelLanguageInfo {
    ModelLanguageInfo {
        supports_language: stt::model_supports_language(&model_size),
        supports_translate: stt::model_supports_translate(&model_size),
        languages: stt::model_supported_languages(&model_size)
            .into_iter()
            .map(|s| s.to_string())
            .collect(),
    }
}

/// Apply a hotkey to the live input hook without persisting it.
/// Used by the settings key-recorder (pass "" to pause while choosing).
#[tauri::command]
pub fn configure_hotkey(spec: String) {
    let _ = crate::input_hook::configure_dictation(&spec);
}

/// Live-apply the edit/rewrite-mode hotkey (used by the Settings recorder to
/// pause the binding while choosing a key, then re-apply it).
#[tauri::command]
pub fn configure_edit_hotkey(spec: String) {
    let _ = crate::input_hook::configure_edit(&spec);
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
pub fn get_config() -> YapConfig {
    config::load()
}

/// Save config, re-apply the hotkey, and push it into the running pipeline.
#[tauri::command]
pub fn save_config(
    app: AppHandle,
    state: State<'_, AppState>,
    cfg: YapConfig,
) -> Result<(), String> {
    config::save(&cfg)?;
    if let Err(e) = crate::input_hook::configure_dictation(&cfg.hotkey) {
        tracing::warn!("Failed to apply hotkey: {}", e);
    }
    if let Err(e) = crate::input_hook::configure_edit(&cfg.edit_hotkey) {
        tracing::warn!("Failed to apply edit hotkey: {}", e);
    }
    // Tray visibility can change with this save (show_tray_icon / show_pill) —
    // reconcile it live instead of waiting for the next app restart.
    crate::tray::ensure_tray(&app, &cfg);
    // This save may have newly selected on-device cleanup (globally or via a
    // per-profile override) — start the sidecar now rather than on next launch.
    let cfg_for_sidecar = cfg.clone();
    tauri::async_runtime::spawn(async move {
        crate::local_llm::autostart_if_configured(&cfg_for_sidecar).await;
    });
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
    download_and_activate(&app, &state, &cfg.model_size).await
}

/// Select a model size, download it (with progress events), and make it the
/// active engine. Used by the onboarding picker so the user can pick any model.
#[tauri::command]
pub async fn download_model_size(
    app: AppHandle,
    state: State<'_, AppState>,
    model_size: String,
) -> Result<(), String> {
    // Persist the choice so it survives restarts and the settings UI agrees.
    let mut cfg = config::load();
    cfg.model_size = model_size.clone();
    config::save(&cfg)?;

    download_and_activate(&app, &state, &model_size).await?;

    // Push the updated config into the running pipeline.
    if let Ok(guard) = state.pipeline.lock() {
        if let Some(p) = guard.as_ref() {
            p.update_config(cfg);
        }
    }
    Ok(())
}

/// Switch to an already-installed model: build the engine, install it into the
/// pipeline, persist the choice, and push the updated config. Does NOT download
/// — the frontend downloads first via `download_model_size`.
#[tauri::command]
pub fn set_active_model(state: State<'_, AppState>, model_size: String) -> Result<(), String> {
    let data_dir = config::data_dir();
    if !stt::is_model_installed(&data_dir, &model_size) {
        return Err(format!("Model '{}' is not installed", model_size));
    }

    let use_gpu = config::load().use_gpu;
    let engine = stt::create_stt_engine(&data_dir, &model_size, use_gpu)
        .map_err(|e| e.to_string())?;

    // Persist the choice.
    let mut cfg = config::load();
    cfg.model_size = model_size.clone();
    config::save(&cfg)?;

    if let Ok(guard) = state.pipeline.lock() {
        if let Some(p) = guard.as_ref() {
            p.set_engine(engine);
            p.update_config(cfg);
        }
    }
    tracing::info!(model = %model_size, "Active model switched");
    Ok(())
}

/// Delete an installed model's artifact from disk — the `.bin` file for
/// file-based models, or the extracted directory for ONNX models. Refuses to
/// delete the currently-active model (so the running engine keeps working).
#[tauri::command]
pub fn delete_model(model_size: String) -> Result<(), String> {
    let cfg = config::load();
    if cfg.model_size == model_size {
        return Err("Cannot delete the active model; switch to another model first".into());
    }

    let path = config::data_dir()
        .join("models")
        .join(stt::model_filename(&model_size));
    if !path.exists() {
        return Err(format!("Model '{}' is not installed", model_size));
    }
    let res = if path.is_dir() {
        std::fs::remove_dir_all(&path)
    } else {
        std::fs::remove_file(&path)
    };
    res.map_err(|e| format!("Failed to delete model: {}", e))?;
    tracing::info!(model = %model_size, path = %path.display(), "Model deleted");
    Ok(())
}

/// Stop recording and discard the audio (abort without transcribing).
#[tauri::command]
pub fn cancel_recording(state: State<'_, AppState>) {
    if let Ok(guard) = state.pipeline.lock() {
        if let Some(p) = guard.as_ref() {
            p.cancel();
        }
    }
}

/// Enable/disable launching Yap at OS login, and persist the choice.
#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    crate::set_autostart_enabled(&app, enabled)?;
    let mut cfg = config::load();
    cfg.autostart = enabled;
    config::save(&cfg)?;
    Ok(())
}

/// Test the AI cleanup settings: run a sample sentence through the saved
/// post-processing config and return the cleaned text (or the error). Lets the
/// user verify their base URL / key / model / prompt from the Settings UI.
/// Never logs the API key.
#[tauri::command]
pub async fn test_post_process(text: String) -> Result<String, String> {
    let cfg = config::load();
    // Route through the on-device sidecar when it's the selected provider + up.
    let (base_url, api_key, model, provider) = crate::local_llm::effective_endpoint(&cfg);
    crate::llm::cleanup(&text, &base_url, &api_key, &model, &provider, &cfg.pp_prompt).await
}

/// Status of the on-device cleanup sidecar: whether the runtime + model are
/// installed on disk, and whether the server is currently running.
#[tauri::command]
pub fn local_llm_status() -> serde_json::Value {
    let cfg = config::load();
    serde_json::json!({
        "installed": crate::local_llm::is_installed(&cfg),
        "running": crate::local_llm::is_running(),
        "modelFile": crate::local_llm::MODEL_FILENAME,
        "model": crate::local_llm::active_model_display(&cfg),
        "engine": crate::local_llm::ENGINE_DISPLAY,
        "activeModel": cfg.pp_local_model,
        "models": crate::local_llm::list_models(),
    })
}

/// Open the on-device models folder (`<data>/llm/`) in the file manager so the
/// user can drop in their own GGUF models.
#[tauri::command]
pub fn open_llm_folder() -> Result<(), String> {
    let dir = crate::local_llm::llm_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    let opener = "explorer";
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let opener = "xdg-open";
    std::process::Command::new(opener)
        .arg(&dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Start the on-device cleanup sidecar (runtime + model must be installed).
/// Resolves once the server's /health endpoint is ready.
#[tauri::command]
pub async fn local_llm_start() -> Result<(), String> {
    crate::local_llm::start().await.map(|_| ())
}

/// Stop the on-device cleanup sidecar.
#[tauri::command]
pub fn local_llm_stop() {
    crate::local_llm::stop();
}

/// Download the on-device cleanup runtime + model (each SHA-verified) on demand.
/// Emits `local-llm-download-progress` per stage. No-op for files already present.
#[tauri::command]
pub async fn local_llm_install(app: AppHandle) -> Result<(), String> {
    crate::local_llm::install(Some(&app)).await
}
/// Recent local transcription history, newest first (capped at `limit`).
/// Each item: `{ ts, raw, text, model, app, words }`.
#[tauri::command]
pub fn get_history(limit: Option<usize>) -> serde_json::Value {
    crate::history::list(limit.unwrap_or(100))
}

/// Delete all local transcription history.
#[tauri::command]
pub fn clear_history() {
    crate::history::clear();
}

/// Derived stats for the dashboard: totals, today, time saved, streak, and a
/// 30-day activity series. See `history::stats`.
#[tauri::command]
pub fn get_stats() -> serde_json::Value {
    crate::history::stats()
}

/// Today's Groq AI-cleanup usage snapshot for the Settings meter.
/// Shape: `{ day, tokens, tokenCap, requests, requestCap }`. Tokens are Yap's
/// own accumulated `usage.total_tokens`; the token cap is the free-tier estimate
/// (constant), while requests use Groq's exact daily header math. Resets at
/// midnight UTC.
#[tauri::command]
pub fn get_groq_usage() -> serde_json::Value {
    crate::usage::snapshot()
}

/// Whether Yap is running as a portable install (data lives next to the exe).
/// The update UI uses this to steer portable users to a manual download, since
/// the in-place updater can't safely replace a portable folder.
#[tauri::command]
pub fn is_portable() -> bool {
    crate::portable::is_portable()
}

/// Model ids whose download is currently in flight, so two concurrent downloads
/// of the same model can't interleave writes into the same `<name>.partial` file.
fn downloads_in_flight() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static SET: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    SET.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

/// Removes its id from the in-flight set on drop (covers every early return).
struct DownloadGuard(String);
impl Drop for DownloadGuard {
    fn drop(&mut self) {
        if let Ok(mut s) = downloads_in_flight().lock() {
            s.remove(&self.0);
        }
    }
}

/// Shared: ensure the model is on disk, build the engine, install it.
async fn download_and_activate(
    app: &AppHandle,
    state: &State<'_, AppState>,
    model_size: &str,
) -> Result<(), String> {
    // Reject a second concurrent download of the same model (double-click, or the
    // two download commands racing) — they'd corrupt a shared `.partial`.
    let _guard = {
        let mut inflight = downloads_in_flight()
            .lock()
            .map_err(|_| "download registry lock poisoned".to_string())?;
        if !inflight.insert(model_size.to_string()) {
            return Err(format!("{} is already downloading", model_size));
        }
        DownloadGuard(model_size.to_string())
    };

    let data_dir = config::data_dir();
    let use_gpu = config::load().use_gpu;

    stt::ensure_model_exists(&data_dir, model_size, Some(app))
        .await
        .map_err(|e| e.to_string())?;

    let engine = stt::create_stt_engine(&data_dir, model_size, use_gpu)
        .map_err(|e| e.to_string())?;

    if let Ok(guard) = state.pipeline.lock() {
        if let Some(p) = guard.as_ref() {
            p.set_engine(engine);
        }
    }
    Ok(())
}

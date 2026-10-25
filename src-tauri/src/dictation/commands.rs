//! The dictation's commands for Yap's windows: recording and the in-page
//! shortcut fallbacks, the microphone, the shortcuts re-bound live, the
//! Chat view's mic, Upload, the speech models, and history.

use tauri::{AppHandle, State};

use crate::config;
use crate::dictation::stt;
use crate::AppState;

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

/// List available microphone input device names (cpal's). Settings and the
/// first-run setup use `mics::list_mics` (endpoint ids, Windows' default).
#[tauri::command]
pub fn list_audio_devices() -> Vec<String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    host.input_devices()
        .map(|devs| devs.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

/// Toggle mic-test mode: while on, `yap-amp` levels are emitted even when idle
/// so onboarding's mic check can show a live meter without recording.
#[tauri::command]
pub fn set_mic_test(state: State<'_, AppState>, on: bool) {
    if let Ok(guard) = state.pipeline.lock() {
        if let Some(p) = guard.as_ref() {
            p.set_mic_test(on);
        }
    }
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
    let _ = crate::dictation::input_hook::configure_dictation(&spec);
}

/// Live-apply the edit/rewrite-mode hotkey (used by the Settings recorder to
/// pause the binding while choosing a key, then re-apply it).
#[tauri::command]
pub fn configure_edit_hotkey(spec: String) {
    let _ = crate::dictation::input_hook::configure_edit(&spec);
}

/// Live-apply the meeting shortcut (the Settings recorder pauses it while
/// choosing a key, as for the other two).
#[tauri::command]
pub fn configure_meeting_hotkey(spec: String) {
    let _ = crate::dictation::input_hook::configure_meeting(&spec);
}

/// Live-apply the Voice Assistant shortcut (the Settings recorder pauses it
/// while choosing a key, as for the others).
#[tauri::command]
pub fn configure_assistant_hotkey(spec: String) {
    let _ = crate::dictation::input_hook::configure_assistant(&spec);
}

/// Live-apply the Read aloud shortcut (the Settings recorder pauses it while
/// choosing a key, as for the others).
#[tauri::command]
pub fn configure_read_hotkey(spec: String) {
    let _ = crate::dictation::input_hook::configure_read(&spec);
}

/// Live-apply the "Use what I said" shortcut (`as_said.rs`; paused while
/// Settings picks a key, as for the others).
#[tauri::command]
pub fn configure_as_said_hotkey(spec: String) {
    let _ = crate::dictation::input_hook::configure_as_said(&spec);
}

/// Live-apply the Snip to Chat shortcut (`snip.rs`; paused while Settings
/// picks a key, as for the others).
#[tauri::command]
pub fn configure_snip_hotkey(spec: String) {
    let _ = crate::dictation::input_hook::configure_snip(&spec);
}

/// Live-apply the Paste last transcript shortcut (`pasting/last.rs`; paused
/// while Settings picks a key, as for the others).
#[tauri::command]
pub fn configure_paste_last_hotkey(spec: String) {
    let _ = crate::dictation::input_hook::configure_paste_last(&spec);
}

/// Live-apply the Copy last transcript shortcut (`pasting/last.rs`; paused
/// while Settings picks a key, as for the others).
#[tauri::command]
pub fn configure_copy_last_hotkey(spec: String) {
    let _ = crate::dictation::input_hook::configure_copy_last(&spec);
}

/// Debug-only, test mode: `text` as if a dictation had just heard it —
/// the dictation's own steps from cleanup on (nothing is pasted in a test
/// run), the bar's "Use what I said" offer included. Stands in for a
/// microphone.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn e2e_dictation(state: State<'_, AppState>, text: String) -> Result<crate::dictation::pipeline::Delivered, String> {
    if !crate::shell::e2e::active() {
        return Err("e2e_dictation is for test runs".into());
    }
    let robot = {
        let guard = state.pipeline.lock().map_err(|_| "the pipeline lock is poisoned")?;
        guard.as_ref().ok_or("no pipeline")?.robot()
    };
    robot.dictate_text(text).await
}

/// Debug-only, test mode: the rewrite shortcut with `text` as what was
/// said, in the window a dictation goes to (`e2e_target_window`'s stand-in,
/// or none). Nothing is selected (a test run reads no selection), so the
/// last dictation is rewritten when "Rewrite your last dictation" takes it,
/// else new text is written — the AI asked either way, nothing read or
/// pasted. Returns what it did.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn e2e_rewrite(state: State<'_, AppState>, text: String) -> Result<crate::dictation::pipeline::Rewritten, String> {
    if !crate::shell::e2e::active() {
        return Err("e2e_rewrite is for test runs".into());
    }
    let robot = {
        let guard = state.pipeline.lock().map_err(|_| "the pipeline lock is poisoned")?;
        guard.as_ref().ok_or("no pipeline")?.robot()
    };
    robot.rewrite_text(text).await
}

/// Debug-only, test mode: dictations and the rewrite shortcut go to a
/// stand-in window (`on`) instead of the one in front — never a real
/// window: nothing is read or pasted there (`pipeline::test_target`).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_target_window(on: bool) -> Result<(), String> {
    if !crate::shell::e2e::active() || crate::shell::realtest::on() {
        return Err("e2e_target_window is for test runs (not robot mode)".into());
    }
    crate::dictation::pipeline::set_test_target(on);
    tracing::info!(on, "e2e: dictations go to a stand-in window");
    Ok(())
}

/// The assistant shortcut went down (`pressed`) or up in one of Yap's own
/// windows (their in-page fallback: the global hook never sees keys while a
/// Yap webview has the focus). The hook's own path, so push-to-talk's hold
/// and double-tap work there too.
#[tauri::command]
pub fn assistant_key(state: State<'_, AppState>, pressed: bool) {
    if let Ok(guard) = state.pipeline.lock() {
        if let Some(p) = guard.as_ref() {
            p.on_assistant_key(pressed, None);
        }
    }
}

/// Toggle recording on/off (the bar's Dictate, a view's mic button).
#[tauri::command]
pub fn toggle_recording(state: State<'_, AppState>) {
    if let Ok(guard) = state.pipeline.lock() {
        if let Some(p) = guard.as_ref() {
            p.toggle();
        }
    }
}

/// The Chat view's mic: a dictation whose text goes into the chat box (the
/// page gets `yap-chat-dictation`, nothing is pasted), or stop the one
/// recording.
#[tauri::command]
pub fn chat_dictate(state: State<'_, AppState>) {
    if let Ok(guard) = state.pipeline.lock() {
        if let Some(p) = guard.as_ref() {
            p.toggle_for_chat();
        }
    }
}

/// The Chat view's box got (`true`) or lost the focus: a dictation started
/// while it has it (the hotkey in Yap's window, the bar's Dictate) is a
/// question for the chat.
#[tauri::command]
pub fn chat_input_focus(focused: bool) {
    crate::dictation::pipeline::set_chat_focus(focused);
}

/// Debug-only, test mode: `text` as if the chat's mic had just heard it —
/// the dictation's own steps from cleanup on, into the Chat view
/// (`yap-chat-dictation`). Stands in for a microphone.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn e2e_chat_dictation(state: State<'_, AppState>, text: String) -> Result<crate::dictation::pipeline::Delivered, String> {
    if !crate::shell::e2e::active() {
        return Err("e2e_chat_dictation is for test runs".into());
    }
    let robot = {
        let guard = state.pipeline.lock().map_err(|_| "the pipeline lock is poisoned")?;
        guard.as_ref().ok_or("no pipeline")?.robot()
    };
    robot.dictate_to_chat(text).await
}

/// The dictation hotkey went down (`pressed`) or up in one of Yap's own
/// windows — their in-page fallback, since the global hook never sees keys
/// while a Yap webview has the focus. Same path as the hook's events, so
/// push-to-talk's hold, tap and double-tap work there too.
#[tauri::command]
pub fn dictation_key(state: State<'_, AppState>, pressed: bool) {
    if let Ok(guard) = state.pipeline.lock() {
        if let Some(p) = guard.as_ref() {
            p.on_key(pressed, None);
        }
    }
}

/// Start transcribing an audio FILE (the Upload surface). Async: progress and
/// the result arrive via `yap-upload-progress` / `-done` / `-error` /
/// `-cancelled` events; this only validates + kicks the run off.
#[tauri::command]
pub fn transcribe_file(state: State<'_, AppState>, path: String) -> Result<(), String> {
    let guard = state.pipeline.lock().map_err(|_| "pipeline unavailable")?;
    let p = guard.as_ref().ok_or("pipeline not started")?;
    p.transcribe_file(path)
}

/// Cancel an in-flight Upload transcription (takes effect between chunks).
#[tauri::command]
pub fn cancel_file_transcription(state: State<'_, AppState>) {
    if let Ok(guard) = state.pipeline.lock() {
        if let Some(p) = guard.as_ref() {
            p.cancel_file_transcription();
        }
    }
}

/// Name + size of an audio file the user picked/dropped (Upload file card).
#[tauri::command]
pub fn audio_file_info(path: String) -> Result<serde_json::Value, String> {
    let meta = std::fs::metadata(&path).map_err(|e| format!("Can't read file: {e}"))?;
    let name = std::path::Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());
    Ok(serde_json::json!({ "name": name, "size": meta.len() }))
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
    let cfg = config::update(|c| c.model_size = model_size.clone())?;

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
///
/// The model already warm on the device it belongs on isn't loaded again
/// (2026-10-24): onboarding's download (`download_model_size`) loads it, then
/// calls this, which loaded it a second time — on the window's thread, so the
/// first-run window froze for the load (~1–3 s) right after the download.
/// The load now runs off it.
#[tauri::command]
pub async fn set_active_model(state: State<'_, AppState>, model_size: String) -> Result<(), String> {
    let data_dir = config::data_dir();
    if !stt::is_model_installed(&data_dir, &model_size) {
        return Err(format!("Model '{}' is not installed", model_size));
    }

    let use_gpu = config::load().use_gpu;
    let warm = state
        .pipeline
        .lock()
        .ok()
        .and_then(|g| g.as_ref().and_then(|p| p.engine_info()))
        .is_some_and(|(model, device)| model == model_size && device == crate::dictation::stt_speed::device_for(&model_size, use_gpu));
    let engine = if warm {
        None
    } else {
        let (dir, id) = (data_dir.clone(), model_size.clone());
        let loaded = tauri::async_runtime::spawn_blocking(move || stt::create_stt_engine(&dir, &id, use_gpu))
            .await
            .map_err(|e| e.to_string())?;
        Some(loaded.map_err(|e| e.to_string())?)
    };

    // Persist the choice.
    let cfg = config::update(|c| c.model_size = model_size.clone())?;

    if let Ok(guard) = state.pipeline.lock() {
        if let Some(p) = guard.as_ref() {
            if let Some(engine) = engine {
                p.set_engine(engine);
            }
            p.update_config(cfg);
        }
    }
    tracing::info!(model = %model_size, reloaded = !warm, "Active model switched");
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

/// Recent local transcription history, newest first (capped at `limit`).
/// Each item: `{ ts, raw, text, model, app, words }`.
#[tauri::command]
pub fn get_history(limit: Option<usize>) -> serde_json::Value {
    crate::dictation::history::list(limit.unwrap_or(100))
}

/// Delete all local transcription history. A save that failed is the error
/// (saving.rs): until history.json is saved, the history would come back.
#[tauri::command]
pub fn clear_history(app: AppHandle) -> Result<(), String> {
    use tauri::Emitter;
    crate::dictation::history::clear();
    let _ = app.emit("yap-history-changed", ());
    Ok(crate::dictation::history::saved()?)
}

/// Delete one history entry (matched by timestamp + final text) — the per-item
/// trash button in the Home feed. A save that failed is the error.
#[tauri::command]
pub fn delete_history_entry(app: AppHandle, ts: u64, text: String) -> Result<(), String> {
    use tauri::Emitter;
    crate::dictation::history::delete(ts, &text);
    let _ = app.emit("yap-history-changed", ());
    Ok(crate::dictation::history::saved()?)
}

/// Derived stats for the dashboard: totals, today, time saved, streak, and a
/// 30-day activity series. See `history::stats`.
#[tauri::command]
pub fn get_stats() -> serde_json::Value {
    crate::dictation::history::stats()
}

/// Everything Home needs to draw itself, in one round trip and off the
/// window's thread (2026-10-24): its history page and stats, the settings
/// it shows (history on, cleanup on, the model, the three shortcuts) and
/// the installed speech models. Before, Home asked for these in four calls,
/// sync ones each holding the main thread, behind the ~60 calls the rest of
/// the window makes as it starts (frontend.md, "Home's first paint").
#[tauri::command]
pub async fn home_snapshot(limit: Option<usize>) -> Result<serde_json::Value, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let cfg = config::load();
        serde_json::json!({
            "history": crate::dictation::history::list(limit.unwrap_or(200)),
            "stats": crate::dictation::history::stats(),
            "config": {
                "historyEnabled": cfg.history_enabled,
                "postProcessEnabled": cfg.post_process_enabled,
                "modelSize": cfg.model_size,
                "hotkey": cfg.hotkey,
                "editHotkey": cfg.edit_hotkey,
                "assistantHotkey": cfg.assistant_hotkey,
            },
            "installedModels": installed_models(),
        })
    })
    .await
    .map_err(|e| e.to_string())
}

/// Model ids whose download is currently in flight, so two concurrent downloads
/// of the same model can't interleave writes into the same `<name>.partial` file.
fn downloads_in_flight() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static SET: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    SET.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

/// A model download is running (the updater won't restart Yap under it).
pub(crate) fn model_download_in_flight() -> bool {
    downloads_in_flight().lock().is_ok_and(|s| !s.is_empty())
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

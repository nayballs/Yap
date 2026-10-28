//! Settings saved: the config read and written for the pages, and every
//! change applied live (the shortcuts, the pipeline, the bar, the tray…),
//! from Settings or from a page that changes one setting by itself.

use tauri::{AppHandle, State};

use crate::config::{self, YapConfig};
use crate::AppState;

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
    let prev = config::load();
    // "Runs on" is what's chosen; the old GPU switch follows it (as saved).
    let mut cfg = cfg;
    cfg.use_gpu = cfg.stt_device != "cpu";
    config::save(&cfg)?;
    apply_config(&app, state, &prev, cfg);
    Ok(())
}

/// Change one setting (or a few) from outside Settings — the Chat picker,
/// the voice's volume, Read aloud's speed, the on-device context — in one
/// step (`config::try_update`: never a load → change → save that could put
/// back what another thread saved meanwhile), then apply it as a Settings
/// save is. `Err` from `change`: nothing saved. Returns the config as saved.
pub(crate) fn change_config(
    app: &AppHandle,
    change: impl FnOnce(&mut YapConfig) -> Result<(), String>,
) -> Result<YapConfig, String> {
    use tauri::Manager;
    let mut prev = None;
    let cfg = config::try_update(|c| {
        prev = Some(c.clone());
        change(c)
    })?;
    let prev = prev.unwrap_or_else(|| cfg.clone());
    apply_config(app, app.state::<AppState>(), &prev, cfg.clone());
    Ok(cfg)
}

/// A config just saved (`cfg`, `prev` before it), applied: hotkeys, the
/// pipeline, the bar, the tray… (what [`save_config`] does after saving).
fn apply_config(app: &AppHandle, state: State<'_, AppState>, prev: &YapConfig, cfg: YapConfig) {
    let app = app.clone();
    // Live-apply a Debug-mode change (only on change, so an explicit RUST_LOG
    // env override isn't clobbered by unrelated saves).
    let prev_debug = prev.debug_logging;
    // Words taken out of the dictionary (from any surface) are never
    // suggested again (learn.rs; its own store, not config.json).
    crate::dictionary::learn::on_dictionary_saved(&prev.dictionary, &cfg.dictionary);
    // A provider key set or changed: check that provider's model list now.
    crate::ai::model_catalog::on_config_saved(&app, prev, &cfg);
    if cfg.debug_logging != prev_debug {
        crate::shell::logging::set_debug_logging(cfg.debug_logging);
    }
    if let Err(e) = crate::dictation::input_hook::configure_dictation(&cfg.hotkey) {
        tracing::warn!("Failed to apply hotkey: {}", e);
    }
    if let Err(e) = crate::dictation::input_hook::configure_edit(&cfg.edit_hotkey) {
        tracing::warn!("Failed to apply edit hotkey: {}", e);
    }
    if let Err(e) = crate::dictation::input_hook::configure_meeting(&cfg.meeting_hotkey) {
        tracing::warn!("Failed to apply meeting hotkey: {}", e);
    }
    crate::dictation::input_hook::configure_assistant_from(&cfg);
    crate::dictation::input_hook::configure_read_from(&cfg);
    crate::dictation::input_hook::configure_as_said_from(&cfg);
    crate::dictation::input_hook::configure_snip_from(&cfg);
    crate::dictation::input_hook::configure_paste_last_from(&cfg);
    crate::dictation::input_hook::configure_copy_last_from(&cfg);
    crate::dictation::input_hook::configure_quick_note_from(&cfg);
    crate::dictation::input_hook::configure_more_from(&cfg);
    crate::dictation::input_hook::configure_languages_from(&cfg);
    crate::dictation::input_hook::configure_transforms_from(&cfg);
    // The tray's Transforms menu follows the list.
    if cfg.transforms != prev.transforms {
        crate::shell::tray::refresh(&app);
    }
    // "Keep chat screenshots" shortened: the older ones go now.
    if cfg.chat_image_days != prev.chat_image_days {
        let days = cfg.chat_image_days;
        std::thread::spawn(move || crate::snip::images::sweep(days));
    }
    // "Delete meeting transcripts after" set or shortened: the older ones
    // go now (notes/retention.rs), and their audio with them.
    crate::notes::retention::on_config_saved(&app, prev, &cfg);
    if cfg.meeting_transcript_days != prev.meeting_transcript_days && cfg.meeting_transcript_days > 0 {
        std::thread::spawn(|| crate::meetings::meeting_audio::prune(None));
    }
    // Settings → Speech-to-Text → Runs on (stt_speed.rs): the warm engine
    // moves to the device chosen now, at a quiet moment.
    crate::dictation::stt_speed::sync(&cfg);
    if cfg.stt_device != prev.stt_device {
        crate::dictation::stt_speed::reload_if_needed(&app);
    }
    crate::voice::tts::set_volume(cfg.tts_volume);
    // The tray is reconciled on every save (always-on since the pill retired) —
    // reconcile it live instead of waiting for the next app restart.
    crate::shell::tray::ensure_tray(&app, &cfg);
    // This save may have newly selected on-device cleanup (globally or via a
    // per-profile override) — start the sidecar now rather than on next launch.
    let cfg_for_sidecar = cfg.clone();
    tauri::async_runtime::spawn(async move {
        crate::ai::local_llm::autostart_if_configured(&cfg_for_sidecar).await;
    });
    // The Integrations toggle may have flipped — start/stop the local API
    // bridge to match (idempotent when unchanged).
    crate::integrations::bridge::sync(&app, cfg.bridge_enabled);
    // …and the "Detect calls and offer to take notes" toggle.
    crate::calls::meeting_detect::sync(&app, cfg.meeting_detection);
    // Hiding meeting windows from screen shares may have flipped mid-meeting.
    crate::calls::capture::sync(&app);
    // …and "Notify before scheduled meetings start" applies at once.
    crate::calendar::on_config_saved();
    // …and the Yap bar (on/off, fullscreen, position, the tooltips' hotkeys).
    crate::bar::sync(&app, &cfg);
    // …and "Show tips" (the notice policy).
    crate::notices::notice::sync(&cfg);
    // …and "Search my meetings by meaning" (search.rs).
    crate::search::on_config_saved(prev, &cfg);
    // …and the theme: the opaque windows' webview backgrounds (theme.rs; the
    // pages follow `yap-config-saved`).
    crate::shell::theme::apply(&app, &cfg);
    if let Ok(guard) = state.pipeline.lock() {
        if let Some(p) = guard.as_ref() {
            p.update_config(cfg);
        }
    }
    // (Pages that show what the config resolves to — the Chat view's model
    // chip, the theme — read it again on `yap-config-saved`, which
    // `config::save` sends after every save, this one included.)
}

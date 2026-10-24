//! Tauri commands exposed to the frontend windows.

use tauri::{AppHandle, Manager, State};

use crate::config::{self, YapConfig};
use crate::dictation::stt;
use crate::AppState;

/// Show the settings window (defined hidden in tauri.conf.json). Shared by the
/// `open_settings` command and the tray menu.
///
/// The `set_focus` is load-bearing here and in `show_onboarding`: both windows
/// are created unfocused (`"focus": false`) so launching Yap never pulls the
/// foreground onto a hidden window, which makes their first `show()` inactive.
pub fn show_settings(app: &AppHandle) -> Result<(), String> {
    let w = app
        .get_webview_window("settings")
        .ok_or("settings window not found")?;
    // `show()` leaves a minimized window on the taskbar; `set_focus` would
    // restore it, but e2e test runs skip that (below).
    if w.is_minimized().unwrap_or(false) {
        let _ = w.unminimize();
    }
    fit_to_screen(&w);
    let _ = w.show();
    focus(&w);
    // The window is on screen now: a pending update announcement moves into
    // it. Not left to the Focused event alone: Windows can refuse a
    // background app the foreground, and e2e test runs never take it.
    crate::updates::on_main_window_focused(app);
    Ok(())
}

/// A window inside its monitor's work area. The main window's 1200 × 800
/// default is taller than a laptop at 150 % (1920 × 1080 → 1280 × 672 above
/// the taskbar) or a 1366 × 768 screen: centred, its title bar — the only
/// place to drag it — sat above the top of the screen and its bottom under
/// the taskbar; onboarding's 720 px put its buttons there. A window that
/// fits (or is maximised) is left alone.
fn fit_to_screen(w: &tauri::WebviewWindow) {
    if w.is_maximized().unwrap_or(false) {
        return;
    }
    let (Ok(Some(monitor)), Ok(size), Ok(pos)) = (w.current_monitor(), w.outer_size(), w.outer_position())
    else {
        return;
    };
    let area = monitor.work_area();
    let fitted = fit_rect(
        (pos.x, pos.y, size.width, size.height),
        (area.position.x, area.position.y, area.size.width, area.size.height),
    );
    if let Some((x, y, width, height)) = fitted {
        tracing::info!(width, height, "main window fitted to the screen");
        let _ = w.set_size(tauri::PhysicalSize::new(width, height));
        let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
    }
}

/// A window rect `(x, y, width, height)` moved and shrunk to lie inside the
/// work area, or `None` when it already does. Pure.
pub(crate) fn fit_rect(window: (i32, i32, u32, u32), area: (i32, i32, u32, u32)) -> Option<(i32, i32, u32, u32)> {
    let (x, y, width, height) = window;
    let (ax, ay, aw, ah) = area;
    if aw == 0 || ah == 0 {
        return None;
    }
    let width2 = width.min(aw);
    let height2 = height.min(ah);
    let x2 = x.clamp(ax, ax + (aw - width2) as i32);
    let y2 = y.clamp(ay, ay + (ah - height2) as i32);
    let fitted = (x2, y2, width2, height2);
    (fitted != window).then_some(fitted)
}

/// `set_focus`, except in e2e test runs: when Windows won't hand a background
/// process the foreground, tao falls back to pressing Alt in whichever app has
/// focus — the developer's, while the suite runs beside them.
fn focus(w: &tauri::WebviewWindow) {
    if !crate::e2e::active() {
        let _ = w.set_focus();
    }
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
    // Show FIRST, then reload. Reloading while hidden left the webview with a
    // frozen visual surface when focused (JS state advanced — log-proven —
    // but pixels only updated after the window LOST focus). The reload itself
    // stays: it gives fresh event listeners and a fresh wizard every open.
    // (720 px tall: on a laptop at 150 % its buttons were under the taskbar.)
    fit_to_screen(&w);
    let _ = w.show();
    focus(&w);
    // Tips and notices wait while the first-run setup is on screen.
    crate::notices::notice::on_onboarding(true);
    let _ = w.eval("window.location.reload()");
    // Compositor nudge: a 1px resize round-trip forces DWM to recomposite the
    // webview surface, un-sticking any stale frame.
    if let Ok(size) = w.inner_size() {
        let _ = w.set_size(tauri::PhysicalSize::new(size.width + 1, size.height));
        let _ = w.set_size(size);
    }
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
    crate::notices::notice::on_onboarding(false);
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

/// Switch the capture stream to a different input device live (no restart).
/// `device` = a name from `list_audio_devices`, or null for the system default.
#[tauri::command]
pub fn set_input_device(state: State<'_, AppState>, device: Option<String>) -> Result<(), String> {
    let guard = state
        .pipeline
        .lock()
        .map_err(|_| "pipeline lock poisoned".to_string())?;
    match guard.as_ref() {
        Some(p) => p.set_input_device(device.as_deref()),
        None => Err("pipeline not running".into()),
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

/// Debug-only, test mode: `text` as if a dictation had just heard it —
/// the dictation's own steps from cleanup on (nothing is pasted in a test
/// run), the bar's "Use what I said" offer included. Stands in for a
/// microphone.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn e2e_dictation(state: State<'_, AppState>, text: String) -> Result<crate::dictation::pipeline::Delivered, String> {
    if !crate::e2e::active() {
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
    if !crate::e2e::active() {
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
    if !crate::e2e::active() || crate::realtest::on() {
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
    if !crate::e2e::active() {
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

// ---- Notes (the AI Notepad surface) ----

/// Note summaries for the list pane, newest-updated first.
#[tauri::command]
pub fn notes_list() -> serde_json::Value {
    crate::notes::list()
}

/// One full note (raw + enhanced content).
#[tauri::command]
pub fn note_get(id: u64) -> Result<crate::notes::Note, String> {
    crate::notes::get(id).ok_or_else(|| "Note not found".to_string())
}

/// Create a note; returns it (the UI selects it immediately). A save that
/// failed is the error ("Couldn't save your note: …", saving.rs): the note
/// is kept in memory and saved with the next change, or Try again.
#[tauri::command]
pub fn note_create(
    title: Option<String>,
    content: Option<String>,
    source: Option<String>,
    folder: Option<String>,
) -> Result<crate::notes::Note, String> {
    let note = crate::notes::create(
        title.as_deref().unwrap_or(""),
        content.as_deref().unwrap_or(""),
        source.as_deref().unwrap_or("manual"),
        folder.as_deref().unwrap_or(""),
    );
    crate::notes::saved()?;
    Ok(note)
}

/// Update a note's title / raw content / folder / attendees (enhancement
/// fields untouched — an edit just marks the Enhanced tab stale). The other
/// windows showing the note follow it: `yap-note-changed` carries the note's
/// editable fields and `origin`, the label of the window that saved (the
/// Notes view and the meeting notepad keep each other in sync both ways).
/// A save that failed is the error, after the windows are told (the edit
/// is kept in memory: saving.rs).
#[tauri::command]
pub fn note_update(
    app: AppHandle,
    id: u64,
    title: Option<String>,
    content: Option<String>,
    folder: Option<String>,
    participants: Option<Vec<String>>,
    origin: Option<String>,
) -> Result<(), String> {
    crate::notes::update(id, title, content, folder, participants)?;
    note_changed(&app, id, origin.as_deref().unwrap_or(""));
    Ok(crate::notes::saved()?)
}

/// Tell every window that note `id`'s editable fields changed (`origin`:
/// who changed them, a window label or "ai"); see [`note_update`].
pub(crate) fn note_changed(app: &AppHandle, id: u64, origin: &str) {
    note_changed_with(app, id, origin, None);
}

/// [`note_changed`], with each transcript line's voice and lock
/// (`[speaker, locked]` per line) after a speaker edit (`speakers.rs`), so
/// the notepad and the Notes view relabel their lines too.
pub(crate) fn note_changed_with(app: &AppHandle, id: u64, origin: &str, line_speakers: Option<Vec<serde_json::Value>>) {
    use tauri::Emitter;
    if let Some(n) = crate::notes::get(id) {
        let mut payload = note_changed_payload(&n, origin);
        if let Some(lines) = line_speakers {
            payload["lineSpeakers"] = serde_json::Value::Array(lines);
        }
        let _ = app.emit("yap-note-changed", payload);
    }
}

/// `yap-note-changed`'s payload for note `n`: its editable fields and
/// speaker map, and who changed it (`origin`).
pub(crate) fn note_changed_payload(n: &crate::notes::Note, origin: &str) -> serde_json::Value {
    serde_json::json!({
        "id": n.id,
        "title": n.title,
        "titleAuto": n.title_auto,
        "content": n.content,
        "folder": n.folder,
        "participants": n.participants,
        "speakers": n.speakers,
        "origin": origin,
    })
}

/// Export a note as a markdown file at `path` (the editor's download button):
/// title heading + enhanced content (or raw) + the transcript, each line
/// under its speaker's name ("You", "Priya", "Priya?" for a guess, "Them").
#[tauri::command]
pub fn note_export(id: u64, path: String) -> Result<(), String> {
    let note = crate::notes::get(id).ok_or("Note not found")?;
    let mut out = String::new();
    let title = if note.title.trim().is_empty() {
        "Untitled note"
    } else {
        note.title.trim()
    };
    out.push_str(&format!("# {title}\n\n"));
    if !note.participants.is_empty() {
        out.push_str(&format!("Attendees: {}\n\n", note.participants.join(", ")));
    }
    if !note.enhanced_content.trim().is_empty() {
        out.push_str(note.enhanced_content.trim());
        out.push('\n');
        if !note.content.trim().is_empty() {
            out.push_str(&format!("\n---\n\n## My thoughts\n\n{}\n", note.content.trim()));
        }
    } else if !note.content.trim().is_empty() {
        out.push_str(note.content.trim());
        out.push('\n');
    }
    if !note.transcript.is_empty() {
        out.push_str("\n## Meeting Transcript\n\n");
        for seg in note.transcript.iter().filter(|s| !s.echo) {
            if seg.dictated {
                out.push_str("_You dictated here (left out of the notes)._\n\n");
                continue;
            }
            if seg.failed {
                let at = |ms: Option<u64>| crate::meetings::meeting_summary::clock(ms.unwrap_or(0) / 1000);
                out.push_str(&format!("_Couldn't transcribe {}–{}._\n\n", at(seg.start_ms), at(seg.end_ms)));
                continue;
            }
            let who = crate::speakers::display_line(&note.speakers, seg);
            out.push_str(&format!("**{who}:** {}\n\n", seg.text));
        }
    }
    std::fs::write(&path, out).map_err(|e| format!("Couldn't write file: {e}"))
}

/// Folder names (seeded with Personal + Meetings, OpenWhispr-style).
#[tauri::command]
pub fn notes_folders() -> Vec<String> {
    crate::notes::folders()
}

/// Note Actions — named prompt fragments (seeded with "Generate Notes").
#[tauri::command]
pub fn notes_actions() -> Vec<crate::notes::Action> {
    crate::notes::actions()
}

#[tauri::command]
pub fn action_create(
    name: String,
    description: Option<String>,
    prompt: String,
) -> Result<crate::notes::Action, String> {
    let action = crate::notes::action_create(&name, description.as_deref().unwrap_or(""), &prompt)?;
    crate::notes::saved()?;
    Ok(action)
}

#[tauri::command]
pub fn action_update(
    id: u64,
    name: String,
    description: Option<String>,
    prompt: String,
) -> Result<(), String> {
    crate::notes::action_update(id, &name, description.as_deref().unwrap_or(""), &prompt)?;
    Ok(crate::notes::saved()?)
}

#[tauri::command]
pub fn action_delete(id: u64) -> Result<(), String> {
    crate::notes::action_delete(id)?;
    Ok(crate::notes::saved()?)
}

/// Create a folder; returns the updated folder list.
#[tauri::command]
pub fn notes_folder_create(name: String) -> Result<Vec<String>, String> {
    let folders = crate::notes::folder_create(&name);
    crate::notes::saved()?;
    Ok(folders)
}

/// Rename a folder (the Notes view's folder menu): its notes go along.
/// Personal and Meetings can't be renamed, nor a folder given a name another
/// one has. Returns the folder list; every window follows (each moved note's
/// `yap-note-changed`, then `yap-notes-changed`).
#[tauri::command]
pub fn notes_folder_rename(app: AppHandle, from: String, to: String) -> Result<Vec<String>, String> {
    let change = crate::notes::folder_rename(&from, &to)?;
    folders_changed(&app, &change);
    crate::notes::saved()?;
    Ok(change.folders)
}

/// Delete a folder; its notes move to Personal. Personal and Meetings can't
/// be deleted. Returns the folder list; every window follows.
#[tauri::command]
pub fn notes_folder_delete(app: AppHandle, name: String) -> Result<Vec<String>, String> {
    let change = crate::notes::folder_delete(&name)?;
    folders_changed(&app, &change);
    crate::notes::saved()?;
    Ok(change.folders)
}

fn folders_changed(app: &AppHandle, change: &crate::notes::FolderChange) {
    use tauri::Emitter;
    for id in &change.moved {
        note_changed(app, *id, "folders");
    }
    let _ = app.emit("yap-notes-changed", ());
}

/// Delete a note. Never the one a meeting is recording into (that goes
/// through `meeting_delete`, which stops the recording first). Every window
/// lets go of it (`yap-note-deleted`; the notepad hides if it showed it).
#[tauri::command]
pub fn note_delete(app: AppHandle, id: u64) -> Result<(), String> {
    use tauri::Emitter;
    crate::meetings::meeting::delete_note(id)?;
    let _ = app.emit("yap-notes-changed", ());
    let _ = app.emit(crate::meetings::meeting_end::EVENT_DELETED, serde_json::json!({ "id": id }));
    crate::meetings::notepad::on_note_deleted(&app, id);
    // Gone from memory; until notes.json is saved it would come back.
    Ok(crate::notes::saved()?)
}

/// Run the note-formatting "action" on a note (OpenWhispr's Actions engine,
/// `runBackgroundAction`): resolve the **Note Formatting** scope's endpoint +
/// editable fragment — falling back to the global cleanup endpoint when the
/// scope is disabled (OpenWhispr `fallbackScope: dictationCleanup`) — call the
/// LLM at temp 0.3 under the immutable guardrails (NOTE_BASE_PROMPT, the
/// meeting variant, or ACTION_PLAN_BASE_PROMPT for the Action Plan), and
/// store the result in `enhanced_content` (raw content never touched).
/// Returns the enhanced markdown; the staleness hash is captured before the
/// call so edits made while the model runs correctly show as stale.
///
/// Meeting notes read their transcript through `meeting_summary`: whole when
/// it's short, else the rolling digests plus the latest stretch (finishing
/// any digest the meeting still owes first), so a long meeting fits the
/// model and comes back in seconds.
///
/// A summary the person edited is written over only with `replace` (the
/// window asked them first); else the run stops before the model with
/// `note_edits::SUMMARY_KEPT`. Ticked tasks stay ticked either way.
#[tauri::command]
pub async fn note_enhance(
    app: AppHandle,
    id: u64,
    action_id: Option<u64>,
    replace: Option<bool>,
) -> Result<String, String> {
    run_enhance(&app, id, action_id, replace.unwrap_or(false), |_| {}).await
}

/// The body of [`note_enhance`], with `step` told where it is, for the
/// notepad's "Step 2 of 3" (`meeting_end.rs` runs the action plan this way):
/// 1 reading the meeting (catching up on its digests), 2 writing, 3 checking
/// owners and deadlines and saving. `replace`: the person said a summary
/// they edited may go (`note_edits::write_summary`).
pub(crate) async fn run_enhance(
    app: &AppHandle,
    id: u64,
    action_id: Option<u64>,
    replace: bool,
    step: impl Fn(u8),
) -> Result<String, String> {
    step(1);
    let note = crate::notes::get(id).ok_or("Note not found")?;
    crate::notes::note_edits::keeps_summary(&note, replace)?;
    // When this run read the note: an edit to the summary after it wins.
    let read_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let heard_to = crate::notes::note_edits::newest_talk(&note);
    let hash = crate::notes::content_hash(&note.content);
    let cfg = config::load();
    let ep = crate::meetings::meeting_summary::resolve_endpoint(&cfg)?;
    // The on-device model is busy from the digests to the final call.
    let _sidecar = crate::local_llm::hold(&ep.base_url);

    // The fragment: the picked Action's prompt (OpenWhispr's Actions engine);
    // without one (older callers), the Note Formatting scope's editable
    // prompt, then the built-in default.
    let action = action_id.and_then(crate::notes::action_get);
    let action_plan = action
        .as_ref()
        .is_some_and(|a| a.kind == crate::notes::ACTION_PLAN);
    let fragment = action
        .map(|a| a.prompt)
        .unwrap_or_else(|| ep.fragment.clone());

    // Meeting notes: attendees + typed notes + the You:/Them: transcript
    // (OpenWhispr PersonalNotesView "assemble input"), long ones digested.
    let is_meeting = note.note_type == "meeting" && !note.transcript.is_empty();
    let (content, attendees, digest_actions) = if is_meeting {
        // A bigger on-device context reads more of the meeting raw.
        let budget = crate::meetings::meeting_summary::FinalBudget::for_endpoint(&ep);
        let note = crate::meetings::meeting_summary::prepare_final(app, id, &ep, budget).await?;
        let input = crate::meetings::meeting_summary::compose_meeting_input_within(&note, budget);
        // The owner checks know the speaker map's names too.
        (input.text, crate::speakers::known_people(&note), input.digest_actions)
    } else if note.participants.is_empty() {
        (note.content.clone(), Vec::new(), Vec::new())
    } else {
        (
            format!("Attendees: {}\n\n{}", note.participants.join(", "), note.content),
            note.participants.clone(),
            Vec::new(),
        )
    };

    let base = if action_plan {
        crate::llm::ACTION_PLAN_BASE_PROMPT
    } else if is_meeting {
        crate::llm::MEETING_NOTE_BASE_PROMPT
    } else {
        crate::llm::NOTE_BASE_PROMPT
    };
    step(2);
    let mut enhanced = crate::privacy::net::within(crate::privacy::net::Feature::Notes, crate::llm::enhance_note(
        &content,
        base,
        &fragment,
        &ep.base_url,
        &ep.api_key,
        &ep.model,
        &ep.provider,
        &ep.final_options(),
    ))
    .await?;
    step(3);
    if action_plan {
        enhanced = crate::meetings::meeting_summary::postcheck_action_plan(
            &enhanced,
            &attendees,
            &content,
            &digest_actions,
        );
    }
    crate::notes::note_edits::write_summary(id, &enhanced, &hash, read_ms, heard_to, replace)?;
    // What's stored: the tasks ticked in the summary before stay ticked.
    Ok(crate::notes::get(id).map(|n| n.enhanced_content).unwrap_or(enhanced))
}

// ---- Meeting recorder ----

/// Start recording a meeting into `note_id` (mic = "You" + system-audio
/// loopback = "Them"). Segments arrive via `yap-meeting-segment`; state via
/// `yap-meeting-state`.
#[tauri::command]
pub fn meeting_start(
    app: AppHandle,
    state: State<'_, AppState>,
    note_id: u64,
) -> Result<(), String> {
    crate::notes::get(note_id).ok_or("Note not found")?;
    let engine_slot = {
        let guard = state.pipeline.lock().map_err(|_| "pipeline unavailable")?;
        let p = guard.as_ref().ok_or("pipeline not started")?;
        p.engine_slot()
    };
    crate::meetings::meeting::start(app.clone(), engine_slot, note_id)?;
    // A recording makes it a meeting note, filed in Meetings as "Take
    // notes" files its own, unless the person chose its folder; the Notes
    // view says so (`yap-note-filed`, with a way back).
    match crate::notes::file_as_meeting(note_id) {
        Ok(Some(from)) => {
            use tauri::Emitter;
            let _ = app.emit(
                "yap-note-filed",
                serde_json::json!({ "noteId": note_id, "from": from, "to": crate::notes::MEETINGS }),
            );
            let _ = app.emit("yap-notes-changed", ());
        }
        Ok(None) => {}
        Err(e) => tracing::warn!(note_id, "Meeting note not filed: {e}"),
    }
    // During a meeting on the calendar, the note becomes that meeting's
    // (title, attendees, invite), whichever way the recording started.
    crate::calendar::on_meeting_started(&app, note_id);
    Ok(())
}

/// `{ recording, noteId?, elapsedSecs? }` for the Notes UI.
#[tauri::command]
pub fn meeting_state() -> serde_json::Value {
    crate::meetings::meeting::state()
}

/// The immutable note-enhancement guardrails, for the Note Formatting Prompt
/// Studio's View tab (same pattern as `get_base_prompt`/`get_edit_base_prompt`).
#[tauri::command]
pub fn get_note_base_prompt() -> String {
    crate::llm::NOTE_BASE_PROMPT.to_string()
}

/// Resolve the **Chat** scope's endpoint + persona (falling back to the global
/// cleanup endpoint when the scope is off) and fail fast on keyless cloud
/// providers. Shared by the embedded note chat and the AI Chat surface, and
/// by the Chat view's model chip (`ai_model::target`), which must name the
/// model these calls use.
pub(crate) fn resolve_chat_endpoint(
    cfg: &YapConfig,
) -> Result<(String, String, String, String, String, bool), String> {
    let scope = cfg.llm_scopes.get("chat");
    // A scope switched on from the chat's model chip may have no prompt yet.
    let persona = scope
        .map(|s| s.prompt.clone())
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| "You are a helpful assistant. Answer concisely.".to_string());
    let (base_url, api_key, model, provider, persona, disable_thinking) = match scope {
        Some(s) if s.enabled && !s.provider.is_empty() => {
            let key = cfg.provider_api_key(&s.provider, &s.api_key);
            let (b, k, m, p) =
                crate::local_llm::effective_endpoint_for(&s.provider, &s.base_url, &key, &s.model);
            (b, k, m, p, persona, s.disable_thinking)
        }
        _ => {
            let (b, k, m, p) = crate::local_llm::effective_endpoint(cfg);
            (b, k, m, p, persona, cfg.pp_disable_thinking)
        }
    };
    if base_url.is_empty() {
        return Err(
            "No AI model configured — set one in Settings → Language Models → Chat".to_string(),
        );
    }
    const KEYED_PROVIDERS: [&str; 5] = ["groq", "anthropic", "openai", "gemini", "openrouter"];
    if api_key.is_empty() && KEYED_PROVIDERS.contains(&provider.as_str()) {
        return Err(format!(
            "No {provider} API key — add one in Settings → Language Models"
        ));
    }
    Ok((base_url, api_key, model, provider, persona, disable_thinking))
}

/// One turn of the embedded note chat (the "Ask anything…" bar, OpenWhispr
/// `useEmbeddedChat`): the **Chat** scope's endpoint + prompt with the note's
/// content and transcript injected as context — falling back to the global
/// cleanup endpoint when the scope is off. `history` is the prior turns as
/// `[role, text]` pairs (the UI sends the last few).
#[tauri::command]
pub async fn note_ask(
    id: u64,
    question: String,
    history: Option<Vec<(String, String)>>,
) -> Result<String, String> {
    let note = crate::notes::get(id).ok_or("Note not found")?;
    let cfg = config::load();
    crate::ai_model::wake_on_device(&cfg).await?;
    let (base_url, api_key, model, provider, persona, disable_thinking) =
        resolve_chat_endpoint(&cfg)?;
    let _sidecar = crate::local_llm::hold(&base_url);

    // Inject the note as grounding context (content + transcript + attendees).
    // A long meeting's transcript comes as its digests + the latest stretch,
    // so the context fits an 8k local model.
    let mut context = String::new();
    if !note.participants.is_empty() {
        context.push_str(&format!("Attendees: {}\n", note.participants.join(", ")));
    }
    if let Some(line) = crate::speakers::speakers_line(&note) {
        context.push_str(&format!("{line}\n"));
    }
    if let Some(invite) = crate::calendar::invite_context(&note) {
        context.push_str(&invite);
    }
    if !note.content.trim().is_empty() {
        context.push_str(note.content.trim());
        context.push('\n');
    }
    if !note.transcript.is_empty() {
        const ASK_TRANSCRIPT_TOKENS: usize = 3_000;
        // A bigger on-device context reads more of it raw (×1–4).
        let budget = ASK_TRANSCRIPT_TOKENS * crate::local_llm::budget_factor_for(&provider, &base_url);
        context.push('\n');
        context.push_str(&crate::meetings::meeting_summary::ask_context(&note, budget));
    }
    if !note.enhanced_content.trim().is_empty() {
        context.push_str(&format!("\nEnhanced notes:\n{}\n", note.enhanced_content.trim()));
    }
    // The note and About you in the system prompt, the same for every
    // question about it; the clock with the question (`chats::with_context`):
    // a hybrid local model re-uses the note's prompt from one question to
    // the next instead of reading it again. A model without tools keeps the
    // clock in the system prompt too, as `chat_turn` does.
    let profile = crate::about_you::profile(&cfg);
    let now = chrono::Utc::now();
    let in_message = crate::chat::tools::supports_tools(&provider, &model);
    let mut system = format!(
        "{}\n\nThe user is asking about the following note. Ground your answers in it; if the answer isn't in the note, say so briefly.\n\n<note>\n{}\n</note>",
        persona.trim(),
        context.trim(),
    );
    let asked = if in_message {
        system.push_str("\n\n");
        system.push_str(crate::chat::chats::CONTEXT_NOTE);
        let profile_block = crate::about_you::profile_block(&profile);
        if !profile_block.is_empty() {
            system.push_str("\n\n");
            system.push_str(&profile_block);
        }
        crate::chat::chats::with_context(&[crate::about_you::clock_line(&profile, now)], &question)
    } else {
        system.push_str("\n\n");
        system.push_str(&crate::about_you::prompt_block(&profile, now));
        question.clone()
    };
    // Memory's "You" part, as every Chat answer gets it (memory.rs).
    let memory_you = crate::memory::you_block();
    if !memory_you.is_empty() {
        system.push_str("\n\n");
        system.push_str(&memory_you);
    }

    let history = history.unwrap_or_default();
    let answer = crate::llm::note_chat(
        &system,
        &history,
        &asked,
        &base_url,
        &api_key,
        &model,
        &provider,
        disable_thinking,
        None,
    );
    crate::privacy::net::within(crate::privacy::net::Feature::Notes, answer).await
}

// ---- Debug logging (Settings → Privacy & data, OpenWhispr Developer section) ----

/// Where logs live + the most recent log file (the one to attach to a bug
/// report). Yap always writes a daily-rolling file at `info`; Debug mode
/// raises Yap's own crate to `debug`.
#[tauri::command]
pub fn log_info() -> serde_json::Value {
    let dir = config::data_dir().join("logs");
    let newest = std::fs::read_dir(&dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_file())
        .max_by_key(|e| {
            e.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        })
        .map(|e| e.path().to_string_lossy().to_string());
    serde_json::json!({
        "dir": dir.to_string_lossy(),
        "file": newest,
    })
}

/// Open the logs folder in the system file manager.
#[tauri::command]
pub fn open_logs_folder() -> Result<(), String> {
    let dir = config::data_dir().join("logs");
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

// ---- AI Chat (the Chat surface; OpenWhispr chat/ChatView port) ----

/// What the notes the eager RAG found ([`rag_context`]) come under, in the
/// question's context (`chats::with_context`): OpenWhispr's framing line.
/// (Telling the model the notes "may not be what it's about" didn't keep
/// Qwen3.5-4B from answering a follow-up about a note that only shared
/// "time" with it — 0 of 6, `notes-long-chat`, 2026-10-22; the stricter
/// match, `tools::rag_notes`, did.)
const RAG_LEAD: &str = "Below are notes from the user's library that may be relevant. Reference them naturally if they help answer the question.";

/// Eager keyword-RAG (OpenWhispr `buildRAGContext`, keyword edition per the
/// ROADMAP's escalating plan): top-5 notes by the shared keyword scorer —
/// `strict`, only those that really match the question (`tools::rag_notes`:
/// for a question's context, since 22 Oct 2026, where a loose match pulled
/// follow-ups off the conversation); else the loose search, as before, in
/// the system prompt of a model without tools (Qwen2.5-1.5B answered less
/// well from one note than from five) — formatted exactly like theirs —
/// `<note id="N" title="T">\n{first 500 chars}\n</note>` joined by blank
/// lines.
fn rag_context(query: &str, strict: bool) -> String {
    const RAG_NOTE_LIMIT: usize = 5;
    const RAG_NOTE_SNIPPET_LENGTH: usize = 500;

    let notes = if strict {
        crate::chat::tools::rag_notes(query, RAG_NOTE_LIMIT)
    } else {
        crate::chat::tools::search_notes(query, RAG_NOTE_LIMIT)
    };
    notes
        .into_iter()
        .map(|(_, n)| {
            // Snippet from raw content (their choice); fall back to enhanced /
            // transcript for notes that are transcript-only.
            let source = if !n.content.trim().is_empty() {
                n.content.clone()
            } else if !n.enhanced_content.trim().is_empty() {
                n.enhanced_content.clone()
            } else {
                n.transcript
                    .iter()
                    .map(|s| s.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let snippet: String = source.chars().take(RAG_NOTE_SNIPPET_LENGTH).collect();
            let title = if n.title.trim().is_empty() {
                "Untitled".to_string()
            } else {
                n.title.clone()
            };
            format!("<note id=\"{}\" title=\"{}\">\n{}\n</note>", n.id, title, snippet)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Conversation summaries for the chat sidebar.
#[tauri::command]
pub fn chats_list() -> serde_json::Value {
    crate::chat::chats::list()
}

/// One conversation with its messages.
#[tauri::command]
pub fn chat_get(id: u64) -> Result<crate::chat::chats::Conversation, String> {
    crate::chat::chats::get(id).map(crate::chat::chats::for_page).ok_or_else(|| "Conversation not found".to_string())
}

/// Delete a conversation. A save that failed is the error (saving.rs):
/// until chats.json is saved, it would come back.
#[tauri::command]
pub fn chat_delete(id: u64) -> Result<(), String> {
    crate::chat::chats::delete(id);
    // A later conversation may get its id: nothing of this one carries over.
    crate::apps::moved_on(Some(id));
    Ok(crate::chat::chats::saved()?)
}

/// Several conversations at once (the Chat list's selection, "Delete all
/// chats…"): one write. Returns how many were deleted.
#[tauri::command]
pub fn chat_delete_many(ids: Vec<u64>) -> Result<usize, String> {
    let gone = crate::chat::chats::delete_many(&ids);
    for id in ids {
        crate::apps::moved_on(Some(id));
    }
    crate::chat::chats::saved()?;
    Ok(gone)
}

/// Rename a conversation (the Chat sidebar's Rename, in place): the title as
/// stored (one line, trimmed, ≤ 80 characters). Its place in the list stays.
#[tauri::command]
pub fn chat_rename(id: u64, title: String) -> Result<String, String> {
    let title = crate::chat::chats::rename(id, &title)?;
    crate::chat::chats::saved()?;
    Ok(title)
}

/// One chat turn, answered: what [`chat_turn`] returns.
pub(crate) struct ChatTurn {
    pub conversation_id: u64,
    /// The question's and the answer's indexes in the conversation.
    pub user_index: usize,
    pub index: usize,
    pub reply: String,
    pub tools_used: Vec<String>,
    /// Who answered; `None` when Yap did itself (an app opened, `apps.rs`).
    pub model: Option<crate::chat::chats::ChatModel>,
    /// The web pages the answer was written from (`web.rs`), numbered as
    /// it cites them.
    pub sources: Vec<crate::chat::chats::Source>,
    /// Stopped while it was being written (the page's ■): what came so far.
    pub stopped: bool,
}

impl ChatTurn {
    /// The answer's id, as the pages call it ("chat:<conversation>:<index>").
    pub fn message_id(&self) -> String {
        format!("chat:{}:{}", self.conversation_id, self.index)
    }
}

/// One chat turn, for the Chat view (`chat_send`) and the Voice Assistant
/// (`assistant.rs`). Creates the conversation on first message (title =
/// first 50 chars, OpenWhispr's rule), persists both sides, and answers via
/// the Chat scope with eager keyword-RAG over the notes library injected under
/// their exact framing line. `asked` runs once the question is saved (the
/// conversation's id and the question's index), before the model is called.
/// A note the tools create or change is announced as it happens, like any
/// other note edit (`yap-note-changed` with origin "chat", then
/// `yap-notes-changed`), so the notepad and the lists follow it live.
///
/// "Open Teams" and other plain requests to open an app are answered by Yap
/// itself, before any model (`apps::plan_for`): with no AI set up, with a
/// small local model, and without a model call at all — and so are the
/// answers to its own questions ("Did you mean Baldur's Gate 3?" → "yes").
/// `scope` "meetings": grounded in the meetings (the Meetings view's Ask
/// bar); "assistant": the Voice Assistant, which sends a shorter history.
///
/// `app`: the windows told of note changes; `None` in a model evaluation
/// (`eval.rs`), which runs this same turn with no windows.
///
/// **Streamed** (`stream.rs`) when someone watches (`watcher`: the Chat
/// view's channel, the Voice Assistant, an evaluation's clock): every model
/// call's text as it's written, each tool call as its chip; a model on this
/// PC steps aside for a dictation. The answer that comes back is the
/// **checked** one (below), which the page puts in place of what streamed.
/// `stop`: the page's ■ — the stream closes, and what was shown so far is
/// the answer, checked and saved as stopped (nothing shown: an error,
/// [`crate::chat::stream::STOPPED`]).
///
/// **A newer question replaces an older one** still being answered in the
/// same conversation: once this turn's question is saved, a turn still
/// answering there ends with nothing saved (`stream::Stoppable::answers`,
/// the error [`crate::chat::stream::SUPERSEDED`]) — its question stays, and the
/// model reads it with the newer one. A tester's voice chat (2026-10-23):
/// the Voice Assistant's first question, set aside for the barge-in that
/// asked the second, was asked again after the second's answer, and both
/// answers were saved under both questions.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn chat_turn(
    app: Option<&AppHandle>,
    conversation_id: Option<u64>,
    text: &str,
    scope: Option<&str>,
    by_voice: bool,
    asked: impl FnOnce(u64, usize) + Send,
    watcher: Option<&dyn crate::chat::stream::Sink>,
    stop: Option<&crate::chat::stream::Stoppable>,
) -> Result<ChatTurn, String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("Type a message first".to_string());
    }
    // No ■ from a page (the Voice Assistant, an evaluation): a switch of its
    // own, so a newer question can still end it.
    let own_stop;
    let stop = match stop {
        Some(s) => s,
        None => {
            own_stop = crate::chat::stream::Stoppable::new(None);
            &own_stop
        }
    };
    let cfg = config::load();
    if let Some(plan) = crate::apps::plan_for(&text, &cfg, conversation_id).await {
        return app_turn(conversation_id, &text, by_voice, asked, plan, stop).await;
    }
    crate::ai_model::wake_on_device(&cfg).await?;
    let (base_url, api_key, model, provider, persona, disable_thinking) =
        resolve_chat_endpoint(&cfg)?;
    // The on-device model is busy for the whole turn — between its tool
    // steps too, while a web search or a page read runs — so a context
    // restart or a runtime swap never cuts the answer off.
    let _sidecar = crate::local_llm::hold(&base_url);
    // Who answers, as the chip named it, kept with the answer.
    let answered_by = crate::ai_model::describe(
        &provider,
        &model,
        &base_url,
        &crate::local_llm::active_model_display(&cfg),
    )
    .chat_model();

    // Conversation bookkeeping (create on first message, OpenWhispr-style).
    let conv = turn_conversation(conversation_id, &text);
    // The model answers this one: Yap's own question about apps has lapsed.
    crate::apps::moved_on(Some(conv.id));
    // The last 19 (+ the new user turn ≈ their slice(-20)); a spoken
    // question, the last 8: every tool step sends them again. The window's
    // start moves a few turns at a time (`chats::history_start`: 14–19,
    // 5–8), so a long chat's prompt keeps its start from turn to turn.
    let (keep, step) = if scope == Some("assistant") { (8, 4) } else { (19, 6) };
    let earlier = &conv.messages[crate::chat::chats::history_start(conv.messages.len(), keep, step)..];
    let history: Vec<(String, String)> = earlier.iter().map(|m| (m.role.clone(), m.text.clone())).collect();
    // The question, with the screenshots it carries (Snip to Chat, the
    // turn's `snip::turn`): kept with it in chats.json as ids.
    let user_index = crate::chat::chats::append_question(conv.id, &text, by_voice, &crate::snip::turn::images())?;
    // This turn answers the conversation now: one still answering there
    // ends, with nothing saved.
    stop.answers(conv.id);
    asked(conv.id, user_index);

    // **Two parts, for the model's cache** (2026-10-22): the system prompt
    // holds what stays the same from turn to turn — the persona, the
    // instructions, About you without its clock — and everything that
    // changes with the question (the time, the notes the eager RAG found,
    // the meetings, earlier web answers, how to answer out loud) goes in
    // the question's own message (`chats::with_context`). A hybrid local
    // model (Qwen3.5) re-uses its cache only up to a checkpoint, so a
    // system prompt that changed every turn made every Chat request read
    // the whole conversation again (docs/model-evals.md); cloud providers'
    // prefix caches like a stable start too. That's for a model that calls
    // tools (Qwen3.5-4B, the cloud models); one that can't (Qwen2.5-1.5B,
    // dense: it re-reads only from the first change anyway) keeps it all in
    // the system prompt — with the notes next to the question it gave half
    // answers (4 of 10 turns, `notes-follow-ups`, 2026-10-22). OpenWhispr
    // runs BOTH eager RAG and the notes tools: the model gets relevant notes
    // up front AND can search for more.
    let tools_supported = crate::chat::tools::supports_tools(&provider, &model);
    let in_message = tools_supported;
    // "Remember that…" / "Forget that…" to a model without tools: Yap's
    // own memory pass makes the change (memory/learn.rs), and says so. A
    // model with tools does it with `remember` / `forget`.
    if !tools_supported {
        if let Some(done) = crate::memory::learn::spoken(&text).await {
            let (reply, chips) = done?;
            let index = crate::chat::chats::append(conv.id, "assistant", &reply, &chips, Some(answered_by.clone()), false)?;
            return Ok(ChatTurn {
                conversation_id: conv.id,
                user_index,
                index,
                reply,
                tools_used: chips,
                model: Some(answered_by),
                sources: Vec::new(),
                stopped: false,
            });
        }
    }
    // Memory (memory.rs): the "You" part in the system prompt with About
    // you (stable from turn to turn), what this question is about with it.
    let memory_you = crate::memory::you_block();
    let memory_here = crate::memory::relevant_block(&text);
    // "Search and reference past chats": a tool for a model with tools; for
    // one without, the chats a question points back to ("what did we say
    // about Lisbon?"), a couple of lines each.
    let past_chats = (cfg.memory_search_chats && !tools_supported && crate::chat::chats::refers_to_past(&text))
        .then(|| crate::chat::chats::past_chats_block(&text, conv.id))
        .flatten();
    // "Look things up on the web" (web.rs). Not for the Meetings view's Ask
    // bar: that's about the meetings.
    let web_on = cfg.web_search && scope != Some("meetings");
    // Which earlier answers came from a search: a tool-calling model with
    // the web gets each one's search replayed as the call it was
    // (`chats::history_messages`); any other model, a line in the
    // question's context (`chats::web_history_note`). Never a note in an
    // answer's text: a model copies it into answers that searched nothing.
    let (replay, history_note) = web_history_mode(tools_supported && web_on);
    // About you (about_you.rs): where the person is, their money and units,
    // name and note — last in the system prompt, below; their clock in the
    // question's context (it changes every minute).
    let profile = crate::about_you::profile(&cfg);
    let now = crate::eval::now();
    let about = crate::about_you::prompt_block(&profile, now);
    let zone = profile.region.time_zone_label.clone();
    let mut system = persona.trim().to_string();
    let mut context: Vec<String> = Vec::new();
    if in_message {
        context.push(crate::about_you::clock_line(&profile, now));
    }
    if tools_supported {
        system.push_str("\n\nYou have access to tools. ");
        system.push_str(&crate::chat::tools::TOOL_INSTRUCTIONS.join(" "));
        if cfg.open_apps {
            system.push(' ');
            system.push_str(&crate::chat::tools::APP_TOOL_INSTRUCTIONS.join(" "));
        }
        if web_on {
            system.push(' ');
            system.push_str(&crate::chat::tools::WEB_TOOL_INSTRUCTIONS.join(" "));
            system.push(' ');
            system.push_str(&crate::web::time_note(zone.as_deref()));
            // Outside the US: prices from where they are, or one plain line
            // saying there are none.
            if let Some(local) = crate::about_you::LocalPrices::for_region(&profile.region) {
                system.push(' ');
                system.push_str(&crate::web::price_note(&local));
            }
        }
        system.push(' ');
        system.push_str(&crate::chat::tools::MEMORY_TOOL_INSTRUCTIONS.join(" "));
        if cfg.memory_search_chats {
            system.push(' ');
            system.push_str(&crate::chat::tools::CHATS_TOOL_INSTRUCTIONS.join(" "));
        }
    } else {
        // No tools: it can't do anything on the PC, so it mustn't say so.
        system.push_str("\n\n");
        system.push_str(crate::apps::NO_TOOLS_NOTE);
        if cfg.open_apps {
            system.push(' ');
            system.push_str(crate::apps::NO_TOOLS_OPEN);
        }
    }
    if history_note && !replay {
        if let Some(note) = crate::chat::chats::web_history_note(earlier) {
            if in_message {
                context.push(note);
            } else {
                system.push_str("\n\n");
                system.push_str(&note);
            }
        }
    }
    // A model on this PC asked something that needs the web: its first step
    // must be a search (left to decide, Qwen3.5-4B answered "the best RPG
    // games coming out in 2026" from memory, a tester's chat, 2026-10-21).
    // Cloud models decide for themselves.
    let first = if tools_supported && web_on && crate::ai_model::runs_here(&provider, &base_url) && crate::web::needs_web(&text) {
        first_search(&text)
    } else {
        crate::chat::tools::FirstStep::Model
    };
    // Asked from the Meetings view: grounded in the latest meetings and the
    // meeting notes that match (calendar.rs).
    if scope == Some("meetings") {
        let meetings = crate::calendar::meetings_context(&text);
        if in_message {
            system.push_str("\n\nThe user is asking about their meetings: the context of their message gives their most recent meetings (newest first) and the meeting notes that match it. Answer from them, say which meeting an answer comes from, and say so when they don't cover the question.");
            context.push(if meetings.is_empty() {
                "No meetings or meeting notes match this question.".to_string()
            } else {
                format!("Their most recent meetings and the meeting notes that match:\n\n{meetings}")
            });
        } else if !meetings.is_empty() {
            system.push_str(&format!(
                "\n\nThe user is asking about their meetings. Below are their most recent meetings (newest first) and the meeting notes that match the question. Answer from them, say which meeting an answer comes from, and say so when they don't cover the question.\n\n{meetings}"
            ));
        }
    }
    let rag = if scope == Some("meetings") { String::new() } else { rag_context(&text, in_message) };
    if !rag.is_empty() {
        if in_message {
            context.push(format!("{RAG_LEAD}\n\n{rag}"));
        } else {
            system.push_str(&format!("\n\n{RAG_LEAD}\n\n{rag}"));
        }
    }
    for block in memory_here.into_iter().chain(past_chats) {
        if in_message {
            context.push(block);
        } else {
            system.push_str("\n\n");
            system.push_str(&block);
        }
    }
    // How the person wants answers, again last in the context, where a
    // small tool model heeds it (a model without tools has it at the end
    // of its system prompt, in memory's "You" block).
    if in_message {
        if let Some(note) = crate::memory::answer_note() {
            context.push(note);
        }
    }
    // Last in the system prompt, where small models heed it: what the
    // context block is and About you without its clock — or, the context
    // staying in the system prompt, the whole block with it.
    if in_message {
        system.push_str("\n\n");
        system.push_str(crate::chat::chats::CONTEXT_NOTE);
        let profile_block = crate::about_you::profile_block(&profile);
        if !profile_block.is_empty() {
            system.push_str("\n\n");
            system.push_str(&profile_block);
        }
    } else {
        system.push_str("\n\n");
        system.push_str(&about);
    }
    if !memory_you.is_empty() {
        system.push_str("\n\n");
        system.push_str(&memory_you);
    }
    // Asked out loud: a tool model with the web starts with the answer; one
    // read aloud whole — the Voice Assistant, or a question dictated into
    // Chat with spoken replies on (a web answer is said short anyway,
    // `assistant::spoken_answer`) — answers short. Last in the context,
    // just before the question, where small models heed it.
    if by_voice && tools_supported && web_on {
        context.push("This question was asked out loud: start your answer with the answer itself, in one or two short sentences, then any details.".to_string());
    }
    let spoken = by_voice && (scope == Some("assistant") || cfg.speak_replies);
    if spoken && !(tools_supported && web_on) {
        if in_message {
            context.push(crate::chat::assistant::SPOKEN_ANSWER_NOTE.to_string());
        } else {
            system.push_str("\n\n");
            system.push_str(crate::chat::assistant::SPOKEN_ANSWER_NOTE);
        }
    }

    // Streamed to whoever watches: what was shown is kept (a Stop keeps
    // it), and a model on this PC steps aside for a new dictation.
    let watch = watcher.map(crate::chat::stream::Watch::new);
    let beside = app
        .filter(|_| crate::ai_model::runs_here(&provider, &base_url))
        .map(|a| crate::chat::stream::NewDictations(a.clone()));
    let live = watch.as_ref().map(|w| crate::chat::stream::Live {
        sink: w,
        beside: beside.as_ref().map(|b| b as &dyn crate::chat::stream::Dictations),
    });

    let (reply, tools_used, sources, looked, pages, stopped) = if tools_supported {
        let notify = |id: u64| {
            use tauri::Emitter;
            if let Some(app) = app {
                note_changed(app, id, "chat");
                let _ = app.emit("yap-notes-changed", ());
            }
        };
        // The turn's web side: the person's links (this message and their
        // earlier ones), the sources found, the safety rule's flag.
        let mut web = web_on.then(|| {
            let earlier: Vec<&str> = history.iter().filter(|(r, _)| r == "user").map(|(_, t)| t.as_str()).collect();
            let settings = crate::web::Settings::for_chat(&cfg, &provider, &model, &base_url, &api_key);
            crate::web::Turn::new(settings, crate::web::Endpoints::current(), &text, &earlier)
                .with_earlier_sources(conv.messages.iter().flat_map(|m| m.sources.iter().map(|s| s.url.as_str())))
        });
        let mut messages: Vec<serde_json::Value> = if replay {
            crate::chat::chats::history_messages(earlier)
        } else {
            history
                .iter()
                .map(|(r, t)| serde_json::json!({ "role": if r == "assistant" { "assistant" } else { "user" }, "content": t }))
                .collect()
        };
        // Earlier screenshots: the newest few sent again, the rest a line
        // (snip/turn.rs).
        crate::snip::turn::with_history_images(&mut messages, earlier, &provider, &base_url).await;
        let extra = crate::chat::tools::Extra { memory: true, search_chats: cfg.memory_search_chats, conversation: Some(conv.id) };
        let looped = crate::chat::tools::run_tool_loop_with(
            &system,
            &messages,
            &text,
            &context,
            &base_url,
            &api_key,
            &model,
            &provider,
            disable_thinking,
            cfg.open_apps,
            &notify,
            web.as_mut(),
            first,
            extra,
            live,
        );
        // Stopped: what was shown so far is the answer (a step's text that
        // came before a tool call was never shown). Superseded: no answer.
        let (reply, tools_used, stopped) = match crate::chat::stream::unless_stopped(looped, Some(stop)).await {
            Ok(done) => {
                let (reply, tools_used) = done?;
                (reply, tools_used, false)
            }
            Err(crate::chat::stream::Halt::Superseded) => return Err(superseded()),
            Err(crate::chat::stream::Halt::Stopped) => {
                let seen = watch.as_ref().map(|w| w.seen()).unwrap_or_default();
                tracing::info!(shown = seen.text.len(), "Chat: the answer was stopped");
                (seen.text, seen.chips, true)
            }
        };
        // gpt-oss sometimes cites as "【1】" (its own browsing style): Yap's [1].
        let reply = if web.is_some() { crate::web::bracket_citations(&reply) } else { reply };
        let looked = web.as_ref().is_some_and(crate::web::Turn::looked);
        let sources = web.as_ref().map(|w| w.sources_for(&reply)).unwrap_or_default();
        let pages = web.map(|w| w.texts).unwrap_or_default();
        (reply, tools_used, sources, looked, pages, stopped)
    } else {
        // Small/local models: plain chat + eager RAG (their fallback), or
        // Yap's fixed web pipeline when the question needs the web.
        let endpoint = (base_url.as_str(), api_key.as_str(), model.as_str(), provider.as_str(), disable_thinking);
        // The web pipeline's own prompt ends as this one does: About you,
        // then memory's "You".
        let about_and_memory = if memory_you.is_empty() { about.clone() } else { format!("{about}\n\n{memory_you}") };
        let SmallAnswer { reply, chips, sources, looked, pages, stopped } = small_model_answer(
            &cfg,
            &persona,
            &system,
            &about_and_memory,
            &history,
            &text,
            endpoint,
            web_on,
            by_voice,
            live,
            Some(stop),
            watch.as_ref(),
        )
        .await?;
        // "Opening Baldur's Gate 3." from a model that can't open anything:
        // said as it is instead (apps.rs).
        let corrected = if crate::apps::claims_unasked(&reply, &text) {
            let (question, said, open_apps) = (text.clone(), reply.clone(), cfg.open_apps);
            tauri::async_runtime::spawn_blocking(move || {
                crate::apps::correct_claim(&said, &question, &crate::apps::apps(false), open_apps)
            })
            .await
            .ok()
            .flatten()
        } else {
            None
        };
        (corrected.unwrap_or(reply), chips, sources, looked, pages, stopped)
    };
    // Stopped before any of the answer showed: nothing to save.
    if stopped && reply.trim().is_empty() {
        return Err(crate::chat::stream::STOPPED.to_string());
    }
    // Every model's answer, checked before it's shown: no sources list or
    // "Yap searched…" note of its own, no search claimed that didn't
    // happen, no "[1]" with nothing (or another question's pages) behind it.
    let (mut reply, fixed) = crate::chat::chats::checked_answer(&reply, &text, looked, &conv.messages, &sources);
    for f in fixed {
        tracing::info!(fix = f, "Chat: Yap fixed the model's answer");
        crate::eval::post_fix(f);
    }
    // A price question that searched: no offer to look for prices where the
    // person is — Yap looked (and, finding only another country's, looked
    // again).
    if looked && crate::web::asks_about_prices(&text) {
        if let Some(stripped) = crate::about_you::LocalPrices::for_region(&profile.region)
            .and_then(|local| crate::about_you::strip_price_offer(&reply, &local))
        {
            tracing::info!("Chat: Yap took out the answer's offer to look for local prices");
            crate::eval::post_fix("price_offer");
            reply = stripped;
        }
    }
    // A price the web gave only in another money ("$8"), written in theirs
    // ("£8"): its page's sign back, and a note saying so.
    let mut restored_note = None;
    if let Some(local) = crate::about_you::LocalPrices::for_region(&profile.region).filter(|_| !pages.is_empty()) {
        if let Some((fixed, restored)) = crate::about_you::restore_price_signs(&reply, &pages, &local) {
            tracing::info!(prices = restored.len(), "Chat: Yap gave prices back the currency their pages gave them");
            crate::eval::post_fix("price_sign");
            reply = fixed;
            restored_note = Some(crate::about_you::restored_price_note(&restored, &local));
        }
    }
    // Prices in $ without saying whose, for someone who pays in £ (About
    // you): Yap says so under the answer (one note at most).
    if let Some(note) = crate::about_you::foreign_price_note(&reply, &profile.region) {
        tracing::info!("Chat: Yap noted the answer's prices are in dollars");
        crate::eval::post_fix("foreign_price_note");
        reply = format!("{}\n\n{note}", reply.trim_end());
    } else if let Some(note) = restored_note {
        reply = format!("{}\n\n{note}", reply.trim_end());
    }
    let index = crate::chat::chats::append_message(
        conv.id,
        "assistant",
        &reply,
        &tools_used,
        Some(answered_by.clone()),
        false,
        &sources,
        stopped,
    )?;
    // Read for lasting facts once the chat goes quiet ("Learn from my chats").
    crate::memory::learn::after_turn(conv.id);
    Ok(ChatTurn { conversation_id: conv.id, user_index, index, reply, tools_used, model: Some(answered_by), sources, stopped })
}

/// The fixed web pipeline's budgets for a model that can't call tools (an
/// 8k on-device model): the web passages, and the history kept beside them.
const SMALL_WEB_TOKENS: usize = 1_500;
const SMALL_HISTORY_TOKENS: usize = 2_000;

/// A turn for a model that can't call tools (the on-device Qwen2.5-1.5B, a
/// local model under 4B): plain chat over the notes — or, with web search
/// on and a question that needs it (`web::needs_web`), **Yap's fixed
/// pipeline**: it searches (the keyed service, else Wikipedia and game
/// wikis), puts the best few passages in the prompt, numbered, and asks the
/// model to answer only from them and cite them. It never lets the model
/// claim a search that didn't happen: it's told so (and `chat_turn`'s
/// checks flag an answer that claims one anyway). Also returns whether a
/// search ran. `about`: the About you block (`about_you::prompt_block`),
/// already last in `system`, kept last in the web pipeline's own prompt.
/// (These models keep the whole context in the system prompt, `chat_turn`.)
/// `live`: streamed to the page (its search as a chip, the answer as it's
/// written); `stop` ends the answer where it is (`watch` has what showed).
#[allow(clippy::too_many_arguments)]
async fn small_model_answer(
    cfg: &YapConfig,
    persona: &str,
    system: &str,
    about: &str,
    history: &[(String, String)],
    text: &str,
    (base_url, api_key, model, provider, disable_thinking): (&str, &str, &str, &str, bool),
    web_on: bool,
    by_voice: bool,
    live: Option<crate::chat::stream::Live<'_>>,
    stop: Option<&crate::chat::stream::Stoppable>,
    watch: Option<&crate::chat::stream::Watch<'_>>,
) -> Result<SmallAnswer, String> {
    let mut chips = Vec::new();
    let mut sources = Vec::new();
    let mut pages = Vec::new();
    let mut system = system.to_string();
    let mut history = history.to_vec();
    let looked = web_on && crate::web::needs_web(text);
    if looked {
        let settings = crate::web::Settings::for_chat(cfg, provider, model, base_url, api_key);
        // A price question's search names the person's country ("… UK").
        let query = crate::web::search_query(text);
        let query = crate::web::local_query(&query, text, settings.local.as_ref()).unwrap_or(query);
        let endpoints = crate::web::Endpoints::current();
        if let Some(live) = live {
            live.sink.send(crate::chat::stream::Event::Tool { name: "web_search".into(), doing: crate::chat::stream::doing("web_search").into() });
        }
        match crate::web::search(&settings, &endpoints, &query, text).await {
            Ok(mut found) if found.hits.iter().any(|h| !h.text.trim().is_empty()) => {
                chips.push(crate::web::search_chip(&query, &found, &settings));
                // Only another country's prices: once more for theirs.
                let price_line = match settings.local.as_ref() {
                    Some(local) if local.tells_apart && crate::web::asks_about_prices(text) => {
                        local_prices_again(&settings, &endpoints, local, &query, text, &mut found, &mut chips).await
                    }
                    _ => None,
                };
                let (found_sources, block) = crate::web::grounded_sources(&found.hits, SMALL_WEB_TOKENS);
                // Temperatures in the person's scale first, with a line
                // saying so (about_you.rs).
                let units = crate::about_you::temperature_units(settings.units, text);
                let temperature_line = crate::about_you::temperature_note(units, &block);
                let block = crate::about_you::convert_temperatures(units, &block).unwrap_or(block);
                system = crate::web::grounded_prompt(persona, &block, &crate::web::today(), by_voice);
                pages.push(block);
                system.push(' ');
                let zone = crate::about_you::profile(cfg).region.time_zone_label;
                system.push_str(&crate::web::time_note(zone.as_deref()));
                if let Some(line) = price_line {
                    system.push(' ');
                    system.push_str(&line);
                }
                if let Some(line) = temperature_line {
                    system.push(' ');
                    system.push_str(&line);
                }
                // Still only text: it can't open or change anything (apps.rs).
                system.push_str("\n\n");
                system.push_str(crate::apps::NO_TOOLS_NOTE);
                system.push_str("\n\n");
                system.push_str(about);
                history = crate::web::fit_history(&history, SMALL_HISTORY_TOKENS);
                chips.push(match found_sources.len() {
                    1 => "Answered from 1 web page".to_string(),
                    n => format!("Answered from {n} web pages"),
                });
                sources = found_sources;
            }
            Ok(found) => {
                chips.push(crate::web::search_chip(&query, &found, &settings));
                system.push_str(&format!("\n\n{}", crate::web::no_search_line(Some("the search found nothing for it"))));
            }
            Err(e) => {
                tracing::info!("Chat: the web search for a small model failed");
                chips.push(format!("Couldn't search the web: {}", crate::web::short(&e)));
                system.push_str(&format!("\n\n{}", crate::web::no_search_line(Some(&e))));
            }
        }
    } else if web_on {
        system.push_str(&format!("\n\n{}", crate::web::NOT_SEARCHED_LINE));
    } else {
        system.push_str(&format!("\n\n{}", crate::web::no_search_line(None)));
    }
    if let Some(live) = live {
        live.sink.send(crate::chat::stream::Event::Chips { chips: chips.clone(), sources: sources.clone(), web: looked });
    }
    let answered = crate::privacy::net::within(
        crate::privacy::net::Feature::Chat,
        crate::llm::note_chat(&system, &history, text, base_url, api_key, model, provider, disable_thinking, live),
    );
    let (reply, stopped) = match crate::chat::stream::unless_stopped(answered, stop).await {
        Ok(reply) => (reply?, false),
        Err(crate::chat::stream::Halt::Superseded) => return Err(superseded()),
        Err(crate::chat::stream::Halt::Stopped) => (watch.map(|w| w.seen().text).unwrap_or_default(), true),
    };
    Ok(SmallAnswer { reply, chips, sources, looked, pages, stopped })
}

/// A turn a newer question in its conversation replaced (`chat_turn`): no
/// answer saved, the question kept; [`crate::chat::stream::SUPERSEDED`].
fn superseded() -> String {
    tracing::info!("Chat: a newer question in the conversation replaced this turn; no answer saved");
    crate::chat::stream::SUPERSEDED.to_string()
}

/// A small model's answer ([`small_model_answer`]): the text, its chips and
/// sources, whether a search ran, and the web text it was given.
struct SmallAnswer {
    reply: String,
    chips: Vec<String>,
    sources: Vec<crate::chat::chats::Source>,
    looked: bool,
    pages: Vec<String>,
    /// Stopped while it was written (`reply` = what showed).
    stopped: bool,
}

/// The fixed web pipeline's second search for a price where the person is
/// (`about_you::LocalPrices`), when the first results only gave another
/// country's: "… UK £". Their prices found: those pages first. None again:
/// the others stay, and the answer is told to say so in one line. Returns
/// the prompt's line on prices, if any. (Its chip goes in `chips`.)
async fn local_prices_again(
    settings: &crate::web::Settings,
    endpoints: &crate::web::Endpoints,
    local: &crate::about_you::LocalPrices,
    query: &str,
    question: &str,
    found: &mut crate::web::Found,
    chips: &mut Vec<String>,
) -> Option<String> {
    let text_of = |f: &crate::web::Found| f.hits.iter().map(|h| format!("{}\n{}", h.title, h.text)).collect::<Vec<_>>().join("\n");
    let first = text_of(found);
    if local.local_in(&first) || !local.foreign_in(&first) {
        return None;
    }
    let (p, s) = (&local.place, local.sign());
    let none = format!(
        "The sources have no {p} prices (in {s}): start your answer with one short line saying you couldn't find {p} prices, then give the sources' prices as they are, saying which country they're for. Don't offer to look further."
    );
    let retry = crate::web::local_retry_query(query, local);
    crate::eval::post_fix("yap_search");
    let more = match crate::web::search(settings, endpoints, &retry, question).await {
        Ok(more) => more,
        Err(e) => {
            chips.push(format!("Couldn't search the web: {}", crate::web::short(&e)));
            return Some(none);
        }
    };
    chips.push(crate::web::search_chip(&retry, &more, settings));
    let theirs = local.local_in(&text_of(&more));
    // Their prices' pages first (the prompt keeps the first few), else the
    // first search's.
    let (mut hits, rest) = if theirs { (more.hits, std::mem::take(&mut found.hits)) } else { (std::mem::take(&mut found.hits), more.hits) };
    for h in rest {
        if !hits.iter().any(|x| crate::web::normal_url(&x.url) == crate::web::normal_url(&h.url)) {
            hits.push(h);
        }
    }
    found.hits = hits;
    Some(if theirs { format!("The first sources give {p} prices (in {s}): answer with those, as the sources give them.") } else { none })
}

/// How earlier web answers reach the model: `(replay, note)` — their
/// searches replayed as tool calls (a tool-calling model with the web on),
/// else the system prompt's line. A model evaluation may pick another way
/// (`YAP_EVAL_WEB_HISTORY` = replay | note | none) to compare them.
fn web_history_mode(tool_model_with_web: bool) -> (bool, bool) {
    if crate::eval::active() {
        match std::env::var("YAP_EVAL_WEB_HISTORY").as_deref() {
            Ok("note") => return (false, true),
            Ok("none") => return (false, false),
            Ok("replay") => return (tool_model_with_web, true),
            _ => {}
        }
    }
    (tool_model_with_web, true)
}

/// The first step for a model on this PC when the question needs the web:
/// it must search (`tools::FirstStep::MustSearch`: its own query, written
/// with the conversation in mind — "Arsenal match tonight 21 October 2026"
/// where the question as a query found nothing useful; Yap searches for it
/// when the server won't make it). A model evaluation may compare
/// (`YAP_EVAL_FIRST` = must | yap | model).
fn first_search(question: &str) -> crate::chat::tools::FirstStep {
    if crate::eval::active() {
        match std::env::var("YAP_EVAL_FIRST").as_deref() {
            Ok("yap") => return crate::chat::tools::FirstStep::YapSearches(crate::web::search_query(question)),
            Ok("model") => return crate::chat::tools::FirstStep::Model,
            _ => {}
        }
    }
    crate::chat::tools::FirstStep::MustSearch
}

/// The conversation a turn goes to: `conversation_id`'s, or a new one named
/// after the first message (first 50 chars, OpenWhispr's rule).
fn turn_conversation(conversation_id: Option<u64>, text: &str) -> crate::chat::chats::Conversation {
    match conversation_id.and_then(crate::chat::chats::get) {
        Some(c) => c,
        None => {
            let title: String = if text.chars().count() > 50 {
                format!("{}...", text.chars().take(50).collect::<String>())
            } else {
                text.to_string()
            };
            crate::chat::chats::create(&title)
        }
    }
}

/// A turn Yap answers itself (`apps.rs`): the request saved like any
/// question, the app opened (or why not) as the answer, with its chip. No
/// model, so none is named. Like any newer question, it replaces an answer
/// still coming in its conversation (`stop`, [`chat_turn`]).
async fn app_turn(
    conversation_id: Option<u64>,
    text: &str,
    by_voice: bool,
    asked: impl FnOnce(u64, usize) + Send,
    plan: crate::apps::Plan,
    stop: &crate::chat::stream::Stoppable,
) -> Result<ChatTurn, String> {
    let conv = turn_conversation(conversation_id, text);
    let user_index = crate::chat::chats::append(conv.id, "user", text, &[], None, by_voice)?;
    stop.answers(conv.id);
    asked(conv.id, user_index);
    // What its answer leaves open: "Did you mean …?", "Which one?", the app
    // "open it again" means.
    crate::apps::remember(conv.id, &plan);
    let answer = tauri::async_runtime::spawn_blocking(move || crate::apps::carry_out(&plan))
        .await
        .map_err(|e| e.to_string())?;
    let tools_used: Vec<String> = Some(answer.chip).filter(|c| !c.is_empty()).into_iter().collect();
    let index = crate::chat::chats::append(conv.id, "assistant", &answer.reply, &tools_used, None, false)?;
    Ok(ChatTurn {
        conversation_id: conv.id,
        user_index,
        index,
        reply: answer.reply,
        tools_used,
        model: None,
        sources: Vec::new(),
        stopped: false,
    })
}

/// Send one chat turn from the Chat view ([`chat_turn`]). Returns `{
/// conversationId, reply, toolsUsed, model, sources, messageId,
/// userMessageId, speak, stopped }`: the two messages' ids ("chat:<conversation>:<index>", what the page
/// calls them) and whether the answer is read out by itself
/// (`tts::auto_speak`: spoken replies on, and "When to speak" says so —
/// `voice`: the question was dictated, kept with it).
///
/// `onStream`: the page's channel — the answer as it's written (`stream.rs`
/// [`crate::chat::stream::Event`]: `{kind: "text", text}`, `{kind: "tool", name,
/// doing}`, `{kind: "chips", chips, sources, web}`, `{kind: "waiting",
/// waiting}`, plus `{kind: "turn", conversationId}` once the question is
/// saved); `reply` is then the checked answer to put in its place.
/// `turnId`: what `stream_stop` stops it by (the ■, or `superseded`: the page
/// sends a newer question next). The error [`crate::chat::stream::SUPERSEDED`]: a
/// newer question in the conversation replaced it — no answer saved, the
/// question kept.
///
/// `images`: the screenshots the question carries (Snip to Chat: ids from
/// `yap-snip`), saved with it and sent with every model call of the turn
/// as `image_url` parts fitted to the provider (`snip::turn`). A model
/// that can't read images is refused before anything is saved
/// (`snip::vision::CANT_READ`); one whose server turns the image down is
/// remembered as such and the error says so (`snip::vision::COULDNT_READ`).
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn chat_send(
    app: AppHandle,
    conversation_id: Option<u64>,
    text: String,
    scope: Option<String>,
    voice: Option<bool>,
    on_stream: crate::chat::stream::PageChannel,
    turn_id: Option<String>,
    images: Option<Vec<String>>,
) -> Result<serde_json::Value, String> {
    let by_voice = voice.unwrap_or(false);
    let stop = crate::chat::stream::Stoppable::new(turn_id.as_deref());
    let watched = on_stream.0.is_some();
    // Screenshots: the model must be able to read them, and their copies
    // for it are made (usually already, while the question was typed).
    let image_ids = images.unwrap_or_default();
    let (turn_images, target) = if image_ids.is_empty() {
        (crate::snip::turn::TurnImages::default(), None)
    } else {
        let cfg = config::load();
        let (base_url, _key, model, provider, _persona, _thinking) = resolve_chat_endpoint(&cfg)?;
        if crate::snip::vision::reads_images(&provider, &model) == Some(false) {
            return Err(format!("{} ({model})", crate::snip::vision::CANT_READ));
        }
        let prepared = crate::snip::turn::prepare(&image_ids, &provider, &base_url).await?;
        tracing::info!(images = prepared.images.len(), "Chat: a question with screenshots");
        (prepared, Some((provider, model)))
    };
    let turned = crate::snip::turn::with(
        turn_images,
        chat_turn(
            Some(&app),
            conversation_id,
            &text,
            scope.as_deref(),
            by_voice,
            |conv, index| {
                // The page learns its conversation (a new one) as soon as it's made.
                on_stream.send_json(serde_json::json!({
                    "kind": "turn",
                    "conversationId": conv,
                    "userMessageId": format!("chat:{conv}:{index}"),
                }));
            },
            watched.then_some(&on_stream as &dyn crate::chat::stream::Sink),
            Some(&stop),
        ),
    )
    .await;
    let turn = match (turned, &target) {
        (Ok(turn), _) => turn,
        // The server turned the image down: that model is a "no" from now
        // on, and the page offers a switch instead of a plain error.
        (Err(e), Some((provider, model))) if crate::snip::vision::is_image_refusal(&e) => {
            tracing::info!(provider, "Chat: the model refused the screenshot");
            crate::snip::vision::refused(provider, model);
            return Err(format!("{} ({model})", crate::snip::vision::COULDNT_READ));
        }
        (Err(e), _) => return Err(e),
    };
    // Read out by itself? (Asked at the answer: a dictation may have started
    // meanwhile, and the settings may have changed.)
    let now = config::load();
    let speak = crate::voice::tts::auto_speak(
        now.speak_replies,
        &now.speak_when,
        by_voice,
        crate::dictation::pipeline::dictation_state().1,
        // A meeting recording or a call: quiet (`tts::meeting_quiet`).
        crate::voice::tts::meeting_quiet().is_some(),
    );
    Ok(serde_json::json!({
        "conversationId": turn.conversation_id,
        "reply": turn.reply,
        "toolsUsed": turn.tools_used,
        "model": turn.model,
        "sources": turn.sources,
        "messageId": turn.message_id(),
        "userMessageId": format!("chat:{}:{}", turn.conversation_id, turn.user_index),
        "speak": speak && !turn.stopped,
        "stopped": turn.stopped,
    }))
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
    crate::model_catalog::on_config_saved(&app, prev, &cfg);
    if cfg.debug_logging != prev_debug {
        crate::set_debug_logging(cfg.debug_logging);
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
    crate::dictation::input_hook::configure_transforms_from(&cfg);
    // The tray's Transforms menu follows the list.
    if cfg.transforms != prev.transforms {
        crate::tray::refresh(&app);
    }
    // "Keep chat screenshots" shortened: the older ones go now.
    if cfg.chat_image_days != prev.chat_image_days {
        let days = cfg.chat_image_days;
        std::thread::spawn(move || crate::snip::images::sweep(days));
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
    crate::tray::ensure_tray(&app, &cfg);
    // This save may have newly selected on-device cleanup (globally or via a
    // per-profile override) — start the sidecar now rather than on next launch.
    let cfg_for_sidecar = cfg.clone();
    tauri::async_runtime::spawn(async move {
        crate::local_llm::autostart_if_configured(&cfg_for_sidecar).await;
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
    // …and the theme: the opaque windows' webview backgrounds (theme.rs; the
    // pages follow `yap-config-saved`).
    crate::theme::apply(&app, &cfg);
    if let Ok(guard) = state.pipeline.lock() {
        if let Some(p) = guard.as_ref() {
            p.update_config(cfg);
        }
    }
    // (Pages that show what the config resolves to — the Chat view's model
    // chip, the theme — read it again on `yap-config-saved`, which
    // `config::save` sends after every save, this one included.)
}

/// Local API bridge status for the Integrations view:
/// `{ running, port, bridgeFile }`.
#[tauri::command]
pub fn bridge_status() -> serde_json::Value {
    crate::integrations::bridge::status()
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

/// Links the pages asked to open during a test run (never opened), oldest
/// first, for the suite (`e2e_opened_urls`).
static E2E_OPENED_URLS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Is `url` a link Yap opens in the browser? Web and mail only: never
/// `file:`, `javascript:`, a custom scheme or a bare path.
fn is_web_or_mail_link(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    ["https://", "http://", "mailto:"].iter().any(|p| lower.starts_with(p))
}

/// Open a web or mail link in the default browser. Every external link in
/// Yap's pages comes through here (`lib/externalLinks.js`: settings links,
/// links in chat answers and notes, release notes, Share by email). A test
/// run never opens one: it keeps it for the suite (`e2e_opened_urls`), so a
/// test can't pop a browser up on the PC running it.
#[tauri::command]
pub fn open_external(app: AppHandle, url: String) -> Result<(), String> {
    if !is_web_or_mail_link(&url) {
        return Err("Yap only opens web and email links.".to_string());
    }
    let url = url.trim();
    if crate::e2e::active() {
        tracing::info!("e2e: would open a link in the browser");
        if let Ok(mut opened) = E2E_OPENED_URLS.lock() {
            opened.push(url.to_string());
        }
        return Ok(());
    }
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| format!("Couldn't open your browser: {e}"))
}

/// Test mode only: the links the pages asked to open (see `open_external`).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_opened_urls() -> Result<Vec<String>, String> {
    if !crate::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    Ok(E2E_OPENED_URLS.lock().map(|o| o.clone()).unwrap_or_default())
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
    config::update(|c| c.autostart = enabled)?;
    Ok(())
}

/// The immutable cleanup guardrail prompt, exposed so the Settings Prompt
/// Studio can display the full effective system prompt (guardrails + the
/// editable body) in its View tab.
#[tauri::command]
pub fn get_base_prompt() -> String {
    crate::llm::BASE_PROMPT.to_string()
}

/// The immutable **edit/rewrite (Commands)** guardrail prompt, exposed so the
/// Commands tab's Prompt Studio can show the full effective prompt (guardrails
/// + the editable agent body) in its View tab. See `llm::EDIT_BASE_PROMPT`.
#[tauri::command]
pub fn get_edit_base_prompt() -> String {
    crate::llm::EDIT_BASE_PROMPT.to_string()
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
    let spelling = crate::about_you::cleanup_spelling(&cfg);
    let cleanup = crate::llm::cleanup(&text, &base_url, &api_key, &model, &provider, &cfg.pp_prompt, spelling.as_deref(), &cfg.dictionary, cfg.pp_disable_thinking);
    crate::privacy::net::within(crate::privacy::net::Feature::Dictation, cleanup).await
}

/// Status of the on-device cleanup sidecar: whether the runtime + model are
/// installed on disk, and whether the server is currently running.
#[tauri::command]
pub fn local_llm_status() -> serde_json::Value {
    let cfg = config::load();
    let on_disk = crate::local_llm::list_models();
    // The graphics card picks the model recommended for this PC (gpu.rs).
    let gpu = crate::gpu::this_pc();
    let capable = crate::gpu::can_run_big_model(gpu.as_ref());
    let recommended = crate::local_llm::recommended_id(capable);
    // The curated one-click download set, with per-model installed state.
    let curated: Vec<serde_json::Value> = crate::local_llm::CURATED_MODELS
        .iter()
        .map(|m| {
            serde_json::json!({
                "id": m.id,
                "display": m.display,
                "blurb": m.blurb,
                "filename": m.filename,
                "sizeMb": m.size_mb,
                "url": m.url,
                // "Recommended for this PC"; `isDefault`: the model an empty
                // `ppLocalModel` means.
                "recommended": m.id == recommended,
                "isDefault": m.default,
                "needsGpu": m.needs_gpu,
                "tools": crate::chat::tools::supports_tools(
                    crate::local_llm::PROVIDER_ONDEVICE,
                    &crate::local_llm::model_id(std::path::Path::new(m.filename)),
                ),
                "family": m.family,
                "installed": on_disk.iter().any(|f| f == m.filename),
                // What it was trained for ("up to 256K"): the file's own
                // header once it's here, else the curated figure.
                "maxContext": crate::local_llm::trained_context(&crate::local_llm::llm_dir().join(m.filename)),
            })
        })
        .collect();
    // Where the running model went, and whether that makes it slow.
    let running = crate::local_llm::shown_running();
    let placement = running.then(crate::local_llm::placement).flatten();
    let serving = crate::local_llm::shown_model_file(&cfg);
    let serving_mb = std::fs::metadata(crate::local_llm::llm_dir().join(&serving)).map(|m| m.len() / 1_048_576).unwrap_or(0);
    let active_file = crate::local_llm::active_model_path(&cfg)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    // GGUFs of the person's own in the folder (not Yap's curated set), for
    // the Chat picker's "On this PC".
    let own: Vec<serde_json::Value> = on_disk
        .iter()
        .filter(|f| !crate::local_llm::CURATED_MODELS.iter().any(|m| m.filename == f.as_str()))
        .map(|f| {
            let id = crate::local_llm::model_id(std::path::Path::new(f));
            let mb = std::fs::metadata(crate::local_llm::llm_dir().join(f)).map(|m| m.len() / 1_048_576).unwrap_or(0);
            serde_json::json!({
                "file": f,
                "name": id,
                "sizeMb": mb,
                "tools": crate::chat::tools::supports_tools(crate::local_llm::PROVIDER_ONDEVICE, &id),
                "maxContext": crate::local_llm::trained_context(&crate::local_llm::llm_dir().join(f)),
            })
        })
        .collect();
    // Another feature than Chat on the on-device model (cleanup, a profile,
    // a scope): it shares the one sidecar, so switching it switches theirs.
    let shared_on_device = (cfg.post_process_enabled && cfg.pp_provider == crate::local_llm::PROVIDER_ONDEVICE)
        || cfg.cleanup_profiles.iter().any(|p| p.provider == crate::local_llm::PROVIDER_ONDEVICE)
        || cfg
            .llm_scopes
            .iter()
            .any(|(k, s)| k != "chat" && s.enabled && s.provider == crate::local_llm::PROVIDER_ONDEVICE);
    let upgrade = crate::local_llm::upgrade_offer(
        capable,
        crate::local_llm::in_use(&cfg),
        &active_file,
        cfg.local_llm_upgrade_dismissed,
    )
    .map(|m| {
        serde_json::json!({
            "id": m.id,
            "display": m.display,
            "sizeMb": m.size_mb,
            "installed": on_disk.iter().any(|f| f == m.filename),
        })
    });
    serde_json::json!({
        "installed": crate::local_llm::is_installed(&cfg),
        "running": running,
        "modelFile": crate::local_llm::MODEL_FILENAME,
        "model": crate::local_llm::active_model_display(&cfg),
        "engine": crate::local_llm::ENGINE_DISPLAY,
        // The llamafile on disk, the one Yap pins, the background upgrade
        // ("downloading" | "waiting" | ""), and the running server's MTP.
        "runtime": {
            "version": crate::local_llm::runtime_version().map(|v| v.to_string()),
            "pinned": crate::local_llm::RUNTIME_VERSION,
            "upgrade": crate::local_llm::upgrade_phase(),
            "mtp": crate::local_llm::serving_plan().map(|p| p.server().mtp),
        },
        "activeModel": cfg.pp_local_model,
        "activeFile": active_file,
        "models": on_disk,
        "own": own,
        "sharedOnDevice": shared_on_device,
        "curated": curated,
        "gpu": gpu,
        "gpuCapable": capable,
        "recommendedId": recommended,
        "placement": placement.map(|p| serde_json::json!({
            "kind": p.kind(),
            "gpuLayers": p.gpu_layers,
            "layers": p.layers,
        })),
        "slowOnThisPc": running && crate::local_llm::slow_on_this_pc(&serving, serving_mb, placement),
        "upgrade": upgrade,
        // The active model's context: the setting, its maximum, the
        // choices with what each takes (local_llm::context_view).
        "context": crate::local_llm::context_view(&cfg),
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

/// Download the on-device cleanup runtime + a model (each SHA-verified) on
/// demand. `model` = a curated id from `local_llm_status().curated` (None →
/// the bundled default). Returns the model's GGUF filename so the caller can
/// set `pp_local_model`. Emits `local-llm-download-progress` per stage.
/// No-op for files already present.
#[tauri::command]
pub async fn local_llm_install(app: AppHandle, model: Option<String>) -> Result<String, String> {
    match model {
        Some(id) => crate::local_llm::install_curated(&id, Some(&app))
            .await
            .map(|m| m.filename.to_string()),
        None => crate::local_llm::install(Some(&app))
            .await
            .map(|_| crate::local_llm::MODEL_FILENAME.to_string()),
    }
}

/// Delete a downloaded local-cleanup model GGUF from `<data>/llm/`. `filename`
/// is a bare name from `local_llm_status().models`/`curated`. Refuses path
/// traversal and non-`.gguf` names, and stops the sidecar first if it's serving
/// the file being removed (Windows locks an open file).
#[tauri::command]
pub fn local_llm_delete(filename: String) -> Result<(), String> {
    let name = std::path::Path::new(&filename)
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "invalid filename".to_string())?;
    if !name.to_ascii_lowercase().ends_with(".gguf") {
        return Err("not a .gguf model".into());
    }
    let path = crate::local_llm::llm_dir().join(name);
    if !path.is_file() {
        return Ok(());
    }
    if crate::local_llm::active_model_path(&config::load()) == path {
        crate::local_llm::stop();
    }
    std::fs::remove_file(&path).map_err(|e| e.to_string())
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

/// Today's Groq AI-cleanup usage snapshot for the Settings meter.
/// Shape: `{ day, tokens, tokenCap, requests, requestCap }`. Tokens are Yap's
/// own accumulated `usage.total_tokens`; the token cap is the free-tier estimate
/// (constant), while requests use Groq's exact daily header math. Resets at
/// midnight UTC.
#[tauri::command]
pub fn get_groq_usage() -> serde_json::Value {
    crate::usage::snapshot()
}

/// Frontend → log-file bridge: webviews call this to land diagnostics in the
/// same rolling `yap.log` as the backend (webview consoles are invisible in
/// normal runs, which made the onboarding event-delivery bug nearly
/// undebuggable — see 2026-07-05).
#[tauri::command]
pub fn frontend_log(msg: String) {
    tracing::info!("[web] {}", msg);
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

#[cfg(test)]
mod tests {
    use super::{fit_rect, is_web_or_mail_link};

    #[test]
    fn the_main_window_fits_a_small_screen() {
        // 1920 × 1080 at 150 %: 1920 × 1008 physical above the taskbar. The
        // 1200 × 800 window (1800 × 1200 physical), centred, started 60 px
        // above the screen and ran under the taskbar.
        assert_eq!(
            fit_rect((60, -60, 1800, 1200), (0, 0, 1920, 1008)),
            Some((60, 0, 1800, 1008))
        );
        // A 1366 × 768 screen at 100 % (728 above the taskbar).
        assert_eq!(fit_rect((83, -16, 1200, 800), (0, 0, 1366, 728)), Some((83, 0, 1200, 728)));
        // A window that fits is left alone, on any monitor.
        assert_eq!(fit_rect((660, 320, 1200, 800), (0, 0, 2560, 1400)), None);
        assert_eq!(fit_rect((2700, 100, 1200, 800), (2560, 0, 2560, 1400)), None);
        // Off the right edge of a second monitor: moved back on.
        assert_eq!(
            fit_rect((4500, 100, 1200, 800), (2560, 0, 2560, 1400)),
            Some((3920, 100, 1200, 800))
        );
        // No work area reported: nothing to fit to.
        assert_eq!(fit_rect((0, 0, 1200, 800), (0, 0, 0, 0)), None);
    }

    #[test]
    fn only_web_and_mail_links_open() {
        let links = ["https://docs.example.com/plan?a=1&b=2", "HTTP://x.io", " mailto:a@b.c", "https://github.com/nayballs/Yap"];
        for ok in links {
            assert!(is_web_or_mail_link(ok), "{ok}");
        }
        let no_links = [
            "javascript:alert(1)",
            "file:///C:/Windows/system32/calc.exe",
            r"C:\Windows",
            "ms-settings:privacy",
            "tauri://localhost",
            "",
        ];
        for no in no_links {
            assert!(!is_web_or_mail_link(no), "{no}");
        }
    }
}

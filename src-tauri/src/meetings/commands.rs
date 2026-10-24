//! Recording a meeting into a note, and the recording's state.

use tauri::{AppHandle, State};

use crate::AppState;

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

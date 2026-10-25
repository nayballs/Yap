//! The Notes view's commands: notes, folders and Actions, every window
//! told of a change, the AI's summary of a note ([`run_enhance`]), and a
//! note's Ask bar.

use tauri::AppHandle;

use crate::chat::turn::resolve_chat_endpoint;
use crate::config;

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
    std::fs::write(&path, export_markdown(&note)).map_err(|e| format!("Couldn't write file: {e}"))
}

/// What [`note_export`] writes (pure): the title, attendees, the summary
/// (then My thoughts) or the typed notes, and the transcript — talk only:
/// never an echo line (the call coming back through your mic) or a "your
/// mic changed" marker; a dictation or a failed stretch as a short note.
pub fn export_markdown(note: &crate::notes::Note) -> String {
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
            // Any other marker (a mic change) has no words to export.
            if !seg.is_talk() || seg.text.trim().is_empty() {
                continue;
            }
            let who = crate::speakers::display_line(&note.speakers, seg);
            out.push_str(&format!("**{who}:** {}\n\n", seg.text));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::TranscriptSegment;

    #[test]
    fn an_export_has_the_talk_never_echo_lines_or_markers() {
        let mut note: crate::notes::Note = serde_json::from_value(serde_json::json!({
            "id": 1, "title": "Launch sync", "content": "Ask about legal", "createdTs": 1, "updatedTs": 1,
            "participants": ["Priya"], "enhancedContent": "## Action plan\n### Priya\n- [ ] Book legal review",
        }))
        .unwrap();
        let line = |source: &str, text: &str, ts: u64| TranscriptSegment { source: source.into(), text: text.into(), ts, ..Default::default() };
        note.transcript = vec![
            line("you", "Morning.", 1),
            line("them", "Hi there, ECHO-CHECK.", 2),
            TranscriptSegment { echo: true, ..line("you", "Hi there, ECHO-CHECK.", 3) },
            TranscriptSegment::dictation_marker(4),
            line("them", "Legal needs a week.", 5),
        ];
        let md = export_markdown(&note);
        assert!(md.starts_with("# Launch sync\n\nAttendees: Priya\n\n## Action plan"), "{md}");
        assert!(md.contains("## My thoughts\n\nAsk about legal"));
        assert_eq!(md.matches("ECHO-CHECK").count(), 1, "the echo line left out: {md}");
        assert!(md.contains("**Them:** Legal needs a week."));
        assert!(md.contains("_You dictated here (left out of the notes)._"));
        assert!(!md.contains("**You:** \n"), "no empty line for a marker");
    }
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
    let _sidecar = crate::ai::local_llm::hold(&ep.base_url);

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
    // `content` is what the model is asked; `source`, what its reply is
    // checked against (the meeting without the language line).
    let (content, source, attendees, digest_actions) = if is_meeting {
        // A bigger on-device context reads more of the meeting raw.
        let budget = crate::meetings::meeting_summary::FinalBudget::for_endpoint(&ep);
        let note = crate::meetings::meeting_summary::prepare_final(app, id, &ep, budget).await?;
        // In the language the meeting was held in (meeting_summary/language.rs).
        let lang = crate::meetings::meeting_summary::language::of_note(&note);
        let (asked, input) = crate::meetings::meeting_summary::final_request(&note, budget, &lang, action_plan);
        // The owner checks know the speaker map's names too.
        (asked, input.text, crate::speakers::known_people(&note), input.digest_actions)
    } else if note.participants.is_empty() {
        (note.content.clone(), note.content.clone(), Vec::new(), Vec::new())
    } else {
        let content = format!("Attendees: {}\n\n{}", note.participants.join(", "), note.content);
        (content.clone(), content, note.participants.clone(), Vec::new())
    };

    let base = if action_plan {
        crate::ai::llm::ACTION_PLAN_BASE_PROMPT
    } else if is_meeting {
        crate::ai::llm::MEETING_NOTE_BASE_PROMPT
    } else {
        crate::ai::llm::NOTE_BASE_PROMPT
    };
    step(2);
    let mut enhanced = crate::privacy::net::within(crate::privacy::net::Feature::Notes, crate::ai::llm::enhance_note(
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
            &source,
            &digest_actions,
        );
    }
    crate::notes::note_edits::write_summary(id, &enhanced, &hash, read_ms, heard_to, replace)?;
    // Written, but not on disk (a full disk, the file held): said by the
    // caller ("Couldn't save your note: …", with Try again), never as if it
    // saved. The summary is kept in memory and saved with the next change.
    crate::notes::saved()?;
    // What's stored: the tasks ticked in the summary before stay ticked.
    Ok(crate::notes::get(id).map(|n| n.enhanced_content).unwrap_or(enhanced))
}

/// The immutable note-enhancement guardrails, for the Note Formatting Prompt
/// Studio's View tab (same pattern as `get_base_prompt`/`get_edit_base_prompt`).
#[tauri::command]
pub fn get_note_base_prompt() -> String {
    crate::ai::llm::NOTE_BASE_PROMPT.to_string()
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
    crate::ai::ai_model::wake_on_device(&cfg).await?;
    let (base_url, api_key, model, provider, persona, disable_thinking) =
        resolve_chat_endpoint(&cfg)?;
    let _sidecar = crate::ai::local_llm::hold(&base_url);

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
        let budget = ASK_TRANSCRIPT_TOKENS * crate::ai::local_llm::context::budget_factor_for(&provider, &base_url);
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
    let profile = crate::ai::about_you::profile(&cfg);
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
        let profile_block = crate::ai::about_you::profile_block(&profile);
        if !profile_block.is_empty() {
            system.push_str("\n\n");
            system.push_str(&profile_block);
        }
        crate::chat::chats::with_context(&[crate::ai::about_you::clock_line(&profile, now)], &question)
    } else {
        system.push_str("\n\n");
        system.push_str(&crate::ai::about_you::prompt_block(&profile, now));
        question.clone()
    };
    // Memory's "You" part, as every Chat answer gets it (memory.rs).
    let memory_you = crate::memory::you_block();
    if !memory_you.is_empty() {
        system.push_str("\n\n");
        system.push_str(&memory_you);
    }

    let history = history.unwrap_or_default();
    let answer = crate::ai::llm::note_chat(
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

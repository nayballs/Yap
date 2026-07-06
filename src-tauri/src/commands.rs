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
    // Show FIRST, then reload. Reloading while hidden left the webview with a
    // frozen visual surface when focused (JS state advanced — log-proven —
    // but pixels only updated after the window LOST focus). The reload itself
    // stays: it gives fresh event listeners and a fresh wizard every open.
    let _ = w.show();
    let _ = w.set_focus();
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
    let mut guard = state
        .pipeline
        .lock()
        .map_err(|_| "pipeline lock poisoned".to_string())?;
    match guard.as_mut() {
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

/// Create a note; returns it (the UI selects it immediately).
#[tauri::command]
pub fn note_create(
    title: Option<String>,
    content: Option<String>,
    source: Option<String>,
    folder: Option<String>,
) -> crate::notes::Note {
    crate::notes::create(
        title.as_deref().unwrap_or(""),
        content.as_deref().unwrap_or(""),
        source.as_deref().unwrap_or("manual"),
        folder.as_deref().unwrap_or(""),
    )
}

/// Update a note's title / raw content / folder / attendees (enhancement
/// fields untouched — an edit just marks the Enhanced tab stale).
#[tauri::command]
pub fn note_update(
    id: u64,
    title: Option<String>,
    content: Option<String>,
    folder: Option<String>,
    participants: Option<Vec<String>>,
) -> Result<(), String> {
    crate::notes::update(id, title, content, folder, participants)
}

/// Export a note as a markdown file at `path` (the editor's download button):
/// title heading + enhanced content (or raw) + the You/Them transcript.
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
            out.push_str(&format!("\n---\n\n## Raw notes\n\n{}\n", note.content.trim()));
        }
    } else if !note.content.trim().is_empty() {
        out.push_str(note.content.trim());
        out.push('\n');
    }
    if !note.transcript.is_empty() {
        out.push_str("\n## Meeting Transcript\n\n");
        for seg in &note.transcript {
            let who = if seg.source == "you" { "You" } else { "Them" };
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
    crate::notes::action_create(&name, description.as_deref().unwrap_or(""), &prompt)
}

#[tauri::command]
pub fn action_update(
    id: u64,
    name: String,
    description: Option<String>,
    prompt: String,
) -> Result<(), String> {
    crate::notes::action_update(id, &name, description.as_deref().unwrap_or(""), &prompt)
}

#[tauri::command]
pub fn action_delete(id: u64) -> Result<(), String> {
    crate::notes::action_delete(id)
}

/// Create a folder; returns the updated folder list.
#[tauri::command]
pub fn notes_folder_create(name: String) -> Vec<String> {
    crate::notes::folder_create(&name)
}

#[tauri::command]
pub fn note_delete(id: u64) {
    crate::notes::delete(id);
}

/// Run the note-formatting "action" on a note (OpenWhispr's Actions engine,
/// `runBackgroundAction`): resolve the **Note Formatting** scope's endpoint +
/// editable fragment — falling back to the global cleanup endpoint when the
/// scope is disabled (OpenWhispr `fallbackScope: dictationCleanup`) — call the
/// LLM at temp 0.3 under the immutable NOTE_BASE_PROMPT guardrails, and store
/// the result in `enhanced_content` (raw content never touched). Returns the
/// enhanced markdown; the staleness hash is captured before the call so edits
/// made while the model runs correctly show as stale.
#[tauri::command]
pub async fn note_enhance(id: u64, action_id: Option<u64>) -> Result<String, String> {
    let note = crate::notes::get(id).ok_or("Note not found")?;
    let hash = crate::notes::content_hash(&note.content);
    let cfg = config::load();

    // The fragment: the picked Action's prompt (OpenWhispr's Actions engine);
    // without one (older callers), fall back to the Note Formatting scope's
    // editable prompt, then the built-in default.
    let action_fragment = action_id
        .and_then(crate::notes::action_get)
        .map(|a| a.prompt);

    let (base_url, api_key, model, provider, scope_fragment, disable_thinking) =
        match cfg.llm_scopes.get("noteFormatting") {
            Some(s) if s.enabled && !s.provider.is_empty() => {
                let key = cfg.provider_api_key(&s.provider, &s.api_key);
                let (b, k, m, p) = crate::local_llm::effective_endpoint_for(
                    &s.provider,
                    &s.base_url,
                    &key,
                    &s.model,
                );
                let frag = if s.prompt.trim().is_empty() {
                    crate::llm::NOTE_DEFAULT_FRAGMENT.to_string()
                } else {
                    s.prompt.clone()
                };
                (b, k, m, p, frag, s.disable_thinking)
            }
            scope => {
                let (b, k, m, p) = crate::local_llm::effective_endpoint(&cfg);
                let frag = scope
                    .map(|s| s.prompt.clone())
                    .filter(|p| !p.trim().is_empty())
                    .unwrap_or_else(|| crate::llm::NOTE_DEFAULT_FRAGMENT.to_string());
                (b, k, m, p, frag, cfg.pp_disable_thinking)
            }
        };
    let fragment = action_fragment.unwrap_or(scope_fragment);

    if base_url.is_empty() {
        return Err(
            "No AI model configured — set one in Settings → Language Models → Note Formatting"
                .to_string(),
        );
    }
    const KEYED_PROVIDERS: [&str; 5] = ["groq", "anthropic", "openai", "gemini", "openrouter"];
    if api_key.is_empty() && KEYED_PROVIDERS.contains(&provider.as_str()) {
        return Err(format!(
            "No {provider} API key — add one in Settings → Language Models"
        ));
    }

    // Meeting notes: assemble typed content + the You:/Them: transcript
    // (OpenWhispr PersonalNotesView "assemble input") and use the meeting base
    // prompt. Attendee names are prepended so the model can attribute without
    // guessing (the base prompt forbids guessing names).
    let is_meeting = note.note_type == "meeting" && !note.transcript.is_empty();
    let attendees = if note.participants.is_empty() {
        String::new()
    } else {
        format!("Attendees: {}\n\n", note.participants.join(", "))
    };
    let content = if is_meeting {
        let mut lines = String::new();
        for seg in &note.transcript {
            let who = if seg.source == "you" { "You" } else { "Them" };
            lines.push_str(&format!("{who}: {}\n", seg.text));
        }
        if note.content.trim().is_empty() {
            format!("{attendees}## Meeting Transcript\n{lines}")
        } else {
            format!(
                "{attendees}{}\n\n## Meeting Transcript\n{lines}",
                note.content.trim()
            )
        }
    } else {
        format!("{attendees}{}", note.content)
    };

    let enhanced = crate::llm::enhance_note(
        &content,
        &fragment,
        is_meeting,
        &base_url,
        &api_key,
        &model,
        &provider,
        disable_thinking,
    )
    .await?;
    crate::notes::set_enhanced(id, &enhanced, &hash)?;
    Ok(enhanced)
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
    crate::meeting::start(app, engine_slot, note_id)
}

/// Stop the active meeting recording (final chunk is transcribed + persisted,
/// then the closing `yap-meeting-state` fires).
#[tauri::command]
pub fn meeting_stop() -> Result<(), String> {
    crate::meeting::stop()
}

/// `{ recording, noteId?, elapsedSecs? }` for the Notes UI.
#[tauri::command]
pub fn meeting_state() -> serde_json::Value {
    crate::meeting::state()
}

/// The immutable note-enhancement guardrails, for the Note Formatting Prompt
/// Studio's View tab (same pattern as `get_base_prompt`/`get_edit_base_prompt`).
#[tauri::command]
pub fn get_note_base_prompt() -> String {
    crate::llm::NOTE_BASE_PROMPT.to_string()
}

/// Resolve the **Chat** scope's endpoint + persona (falling back to the global
/// cleanup endpoint when the scope is off) and fail fast on keyless cloud
/// providers. Shared by the embedded note chat and the AI Chat surface.
fn resolve_chat_endpoint(
    cfg: &YapConfig,
) -> Result<(String, String, String, String, String, bool), String> {
    let (base_url, api_key, model, provider, persona, disable_thinking) =
        match cfg.llm_scopes.get("chat") {
            Some(s) if s.enabled && !s.provider.is_empty() => {
                let key = cfg.provider_api_key(&s.provider, &s.api_key);
                let (b, k, m, p) = crate::local_llm::effective_endpoint_for(
                    &s.provider,
                    &s.base_url,
                    &key,
                    &s.model,
                );
                (b, k, m, p, s.prompt.clone(), s.disable_thinking)
            }
            scope => {
                let (b, k, m, p) = crate::local_llm::effective_endpoint(cfg);
                let persona = scope
                    .map(|s| s.prompt.clone())
                    .filter(|p| !p.trim().is_empty())
                    .unwrap_or_else(|| {
                        "You are a helpful assistant. Answer concisely.".to_string()
                    });
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
    let (base_url, api_key, model, provider, persona, disable_thinking) =
        resolve_chat_endpoint(&cfg)?;

    // Inject the note as grounding context (content + transcript + attendees).
    let mut context = String::new();
    if !note.participants.is_empty() {
        context.push_str(&format!("Attendees: {}\n", note.participants.join(", ")));
    }
    if !note.content.trim().is_empty() {
        context.push_str(note.content.trim());
        context.push('\n');
    }
    if !note.transcript.is_empty() {
        context.push_str("\nMeeting transcript:\n");
        for seg in &note.transcript {
            let who = if seg.source == "you" { "You" } else { "Them" };
            context.push_str(&format!("{who}: {}\n", seg.text));
        }
    }
    if !note.enhanced_content.trim().is_empty() {
        context.push_str(&format!("\nEnhanced notes:\n{}\n", note.enhanced_content.trim()));
    }
    let system = format!(
        "{}\n\nThe user is asking about the following note. Ground your answers in it; if the answer isn't in the note, say so briefly.\n\n<note>\n{}\n</note>",
        persona.trim(),
        context.trim()
    );

    crate::llm::note_chat(
        &system,
        &history.unwrap_or_default(),
        &question,
        &base_url,
        &api_key,
        &model,
        &provider,
        disable_thinking,
    )
    .await
}

// ---- Debug logging (Settings → Advanced, OpenWhispr Developer section) ----

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

/// Eager keyword-RAG (OpenWhispr `buildRAGContext`, keyword edition per the
/// ROADMAP's escalating plan): score every note by query-word hits (title
/// weighted 3×), take the top 5, and format them exactly like theirs —
/// `<note id="N" title="T">\n{first 500 chars}\n</note>` joined by blank lines.
fn rag_context(query: &str) -> String {
    const RAG_NOTE_LIMIT: usize = 5;
    const RAG_NOTE_SNIPPET_LENGTH: usize = 500;

    let words: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_string()
        })
        .filter(|w| w.chars().count() > 2)
        .collect();
    if words.is_empty() {
        return String::new();
    }

    let mut scored: Vec<(usize, crate::notes::Note)> = crate::notes::all()
        .into_iter()
        .filter_map(|n| {
            let title = n.title.to_lowercase();
            let body = format!(
                "{}\n{}\n{}",
                n.content.to_lowercase(),
                n.enhanced_content.to_lowercase(),
                n.transcript
                    .iter()
                    .map(|s| s.text.to_lowercase())
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            let score: usize = words
                .iter()
                .map(|w| title.matches(w.as_str()).count() * 3 + body.matches(w.as_str()).count())
                .sum();
            (score > 0).then_some((score, n))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));

    scored
        .into_iter()
        .take(RAG_NOTE_LIMIT)
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
    crate::chats::list()
}

/// One conversation with its messages.
#[tauri::command]
pub fn chat_get(id: u64) -> Result<crate::chats::Conversation, String> {
    crate::chats::get(id).ok_or_else(|| "Conversation not found".to_string())
}

#[tauri::command]
pub fn chat_delete(id: u64) {
    crate::chats::delete(id);
}

/// Send one chat turn. Creates the conversation on first message (title =
/// first 50 chars, OpenWhispr's rule), persists both sides, and answers via
/// the Chat scope with eager keyword-RAG over the notes library injected under
/// their exact framing line. Returns `{ conversationId, reply }`.
#[tauri::command]
pub async fn chat_send(
    conversation_id: Option<u64>,
    text: String,
) -> Result<serde_json::Value, String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("Type a message first".to_string());
    }
    let cfg = config::load();
    let (base_url, api_key, model, provider, persona, disable_thinking) =
        resolve_chat_endpoint(&cfg)?;

    // Conversation bookkeeping (create on first message, OpenWhispr-style).
    let conv = match conversation_id.and_then(crate::chats::get) {
        Some(c) => c,
        None => {
            let title: String = if text.chars().count() > 50 {
                format!("{}...", text.chars().take(50).collect::<String>())
            } else {
                text.clone()
            };
            crate::chats::create(&title)
        }
    };
    let history: Vec<(String, String)> = conv
        .messages
        .iter()
        .rev()
        .take(19) // + the new user turn ≈ their slice(-20)
        .rev()
        .map(|m| (m.role.clone(), m.text.clone()))
        .collect();
    crate::chats::append(conv.id, "user", &text)?;

    // System prompt: persona + RAG notes under their exact framing.
    let rag = rag_context(&text);
    let system = if rag.is_empty() {
        persona.trim().to_string()
    } else {
        format!(
            "{}\n\nBelow are notes from the user's library that may be relevant. Reference them naturally if they help answer the question.\n\n{}",
            persona.trim(),
            rag
        )
    };

    let reply = crate::llm::note_chat(
        &system,
        &history,
        &text,
        &base_url,
        &api_key,
        &model,
        &provider,
        disable_thinking,
    )
    .await?;
    crate::chats::append(conv.id, "assistant", &reply)?;
    Ok(serde_json::json!({ "conversationId": conv.id, "reply": reply }))
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
    // Live-apply a Debug-mode change (only on change, so an explicit RUST_LOG
    // env override isn't clobbered by unrelated saves).
    let prev_debug = config::load().debug_logging;
    config::save(&cfg)?;
    if cfg.debug_logging != prev_debug {
        crate::set_debug_logging(cfg.debug_logging);
    }
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

/// The immutable cleanup guardrail prompt, exposed so the Settings Prompt
/// Studio can display the full effective system prompt (guardrails + the
/// editable body) in its View tab.
#[tauri::command]
pub fn get_base_prompt() -> String {
    crate::llm::BASE_PROMPT.to_string()
}

/// The immutable **edit/rewrite (Voice Agent)** guardrail prompt, exposed so the
/// Voice Agent tab's Prompt Studio can show the full effective prompt (guardrails
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
    crate::llm::cleanup(&text, &base_url, &api_key, &model, &provider, &cfg.pp_prompt, &cfg.dictionary, cfg.pp_disable_thinking).await
}

/// Status of the on-device cleanup sidecar: whether the runtime + model are
/// installed on disk, and whether the server is currently running.
#[tauri::command]
pub fn local_llm_status() -> serde_json::Value {
    let cfg = config::load();
    let on_disk = crate::local_llm::list_models();
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
                "recommended": m.recommended,
                "family": m.family,
                "installed": on_disk.iter().any(|f| f == m.filename),
            })
        })
        .collect();
    serde_json::json!({
        "installed": crate::local_llm::is_installed(&cfg),
        "running": crate::local_llm::is_running(),
        "modelFile": crate::local_llm::MODEL_FILENAME,
        "model": crate::local_llm::active_model_display(&cfg),
        "engine": crate::local_llm::ENGINE_DISPLAY,
        "activeModel": cfg.pp_local_model,
        "models": on_disk,
        "curated": curated,
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
    crate::history::list(limit.unwrap_or(100))
}

/// Delete all local transcription history.
#[tauri::command]
pub fn clear_history() {
    crate::history::clear();
}

/// Delete one history entry (matched by timestamp + final text) — the per-item
/// trash button in the Home feed.
#[tauri::command]
pub fn delete_history_entry(ts: u64, text: String) {
    crate::history::delete(ts, &text);
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

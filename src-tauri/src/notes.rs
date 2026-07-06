//! Local notes store — the AI Notepad's data layer (OpenWhispr `notes` table,
//! JSON-file edition).
//!
//! Mirrors the fields Yap needs from OpenWhispr's SQLite schema: raw markdown
//! `content` (never overwritten by AI), `enhanced_content` (the "Enhanced"
//! tab, written by the note-formatting Actions call), and
//! `enhanced_at_hash` — OpenWhispr's cheap `len + first-50-chars` staleness
//! marker, used only to show a "note changed since enhancement" dot. Stored as
//! `notes.json` in the data dir, same best-effort pattern as `history.rs`.
//! Deliberately skipped for v1 (documented in ROADMAP): folders, FTS, sync
//! columns, meeting transcript/participants (arrive with the Phase-6 recorder).

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// One You/Them transcript segment from the meeting recorder (OpenWhispr
/// `TranscriptSegment`, trimmed to what Yap uses: source "you"|"them", text,
/// unix-seconds timestamp).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegment {
    pub source: String,
    pub text: String,
    pub ts: u64,
}

/// One note. `note_type`: "personal" | "meeting" (set when a recording starts).
/// camelCase on the wire + on disk, like `YapConfig`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: u64,
    #[serde(default)]
    pub title: String,
    /// Raw markdown, exactly as typed/dictated. Never touched by enhancement.
    #[serde(default)]
    pub content: String,
    /// AI output (the Enhanced tab). Empty = never enhanced.
    #[serde(default)]
    pub enhanced_content: String,
    /// `content_hash` of `content` at the moment of the last enhancement.
    #[serde(default)]
    pub enhanced_at_hash: String,
    #[serde(default = "default_note_type")]
    pub note_type: String,
    /// Folder name (OpenWhispr seeds Personal + Meetings; user-creatable).
    #[serde(default = "default_folder")]
    pub folder: String,
    /// Meeting-recorder segments (You/Them), time-ordered. Empty for
    /// personal notes.
    #[serde(default)]
    pub transcript: Vec<TranscriptSegment>,
    /// Attendee names (OpenWhispr `participants`) — shown as chips and fed to
    /// the enhancement prompt so the model can attribute correctly.
    #[serde(default)]
    pub participants: Vec<String>,
    /// Where the note came from ("manual" | "upload" | later "meeting").
    #[serde(default)]
    pub source: String,
    pub created_ts: u64,
    pub updated_ts: u64,
}

fn default_note_type() -> String {
    "personal".to_string()
}

fn default_folder() -> String {
    "Personal".to_string()
}

fn default_folders() -> Vec<String> {
    vec!["Personal".to_string(), "Meetings".to_string()]
}

/// A note "Action" — a named, user-editable prompt fragment run under the
/// immutable NOTE_BASE_PROMPT guardrails (OpenWhispr `actions` table).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub prompt: String,
    /// Built-ins can be edited but not deleted (OpenWhispr semantics).
    #[serde(default)]
    pub builtin: bool,
}

/// Built-in action seeds: OpenWhispr's "Generate Notes" (database.js seed,
/// verbatim — the prompt is `llm::NOTE_DEFAULT_FRAGMENT`) plus two
/// meeting-focused templates (Yap additions; the meeting structure mirrors
/// OpenWhispr's MEETING_SYSTEM_PROMPT sections). `(name, description, prompt)`.
const BUILTIN_ACTIONS: [(&str, &str, &str); 3] = [
    (
        "Generate Notes",
        "Clean up, structure, and enhance your notes",
        crate::llm::NOTE_DEFAULT_FRAGMENT,
    ),
    (
        "Meeting Notes",
        "Turn rough meeting notes into structured minutes",
        "The content is rough notes taken during a meeting (possibly including fragments of transcript). Produce clean meeting notes. Start with a concise 1\u{2013}2 sentence summary of what the meeting was about. Then use these section headings, omitting any that have no content: ## Key Discussion Points, ## Decisions Made, ## Action Items, ## Follow-ups. Under Action Items use checkboxes (- [ ]) and attribute each item to a person where clear. Consolidate repeated points into coherent ones, preserve specific commitments and dates verbatim, and bias toward brevity.",
    ),
    (
        "Action Items",
        "Extract just the tasks, owners, and deadlines",
        "Extract ONLY the action items from the content. Output a markdown checkbox list (- [ ]) with one task per line. When the owner is clear, start the line with their name and a colon (e.g. - [ ] Dave: send the revised budget). Include deadlines in parentheses when mentioned. Do not add tasks that weren't stated or clearly implied. If there are genuinely no action items, output exactly: No action items.",
    ),
];

fn default_actions() -> Vec<Action> {
    BUILTIN_ACTIONS
        .iter()
        .enumerate()
        .map(|(i, (name, description, prompt))| Action {
            id: i as u64 + 1,
            name: name.to_string(),
            description: description.to_string(),
            prompt: prompt.to_string(),
            builtin: true,
        })
        .collect()
}

/// Additive migration: stores created before a built-in existed get it added
/// (matched by name so user edits to a built-in's prompt are never clobbered).
fn seed_missing_builtins(store: &mut Store) -> bool {
    let mut changed = false;
    for (name, description, prompt) in BUILTIN_ACTIONS {
        if !store
            .actions
            .iter()
            .any(|a| a.name.eq_ignore_ascii_case(name))
        {
            let id = store.actions.iter().map(|a| a.id).max().unwrap_or(0) + 1;
            store.actions.push(Action {
                id,
                name: name.to_string(),
                description: description.to_string(),
                prompt: prompt.to_string(),
                builtin: true,
            });
            changed = true;
        }
    }
    changed
}

/// On-disk shape: folders + notes (OpenWhispr seeds the Personal and Meetings
/// folders on first run; `database.js:191-198`).
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Store {
    #[serde(default = "default_folders")]
    folders: Vec<String>,
    #[serde(default = "default_actions")]
    actions: Vec<Action>,
    #[serde(default)]
    notes: Vec<Note>,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            folders: default_folders(),
            actions: default_actions(),
            notes: Vec::new(),
        }
    }
}

static STATE: Mutex<Option<Store>> = Mutex::new(None);

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn notes_path() -> PathBuf {
    crate::config::data_dir().join("notes.json")
}

fn load_from_disk() -> Store {
    let path = notes_path();
    let mut store = match std::fs::read_to_string(&path) {
        // Current shape first; fall back to the v1 bare-array format. A file
        // that exists but parses as NEITHER is quarantined (renamed aside),
        // never overwritten — see config::quarantine_corrupt.
        Ok(s) => match serde_json::from_str::<Store>(&s).or_else(|_| {
            serde_json::from_str::<Vec<Note>>(&s).map(|notes| Store {
                folders: default_folders(),
                actions: default_actions(),
                notes,
            })
        }) {
            Ok(store) => store,
            Err(e) => {
                tracing::error!("notes.json failed to parse: {}", e);
                crate::config::quarantine_corrupt(&path);
                Store::default()
            }
        },
        Err(_) => Store::default(),
    };
    if seed_missing_builtins(&mut store) {
        save_to_disk(&store);
    }
    store
}

fn save_to_disk(store: &Store) {
    match serde_json::to_string(store) {
        Ok(json) => {
            let _ = std::fs::create_dir_all(crate::config::data_dir());
            if let Err(e) = crate::config::atomic_write(&notes_path(), &json) {
                tracing::warn!("Failed to persist notes: {}", e);
            }
        }
        Err(e) => tracing::warn!("Failed to serialize notes: {}", e),
    }
}

fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let store = guard.get_or_insert_with(load_from_disk);
    f(store)
}

fn with_notes<R>(f: impl FnOnce(&mut Vec<Note>) -> R) -> R {
    with_store(|s| f(&mut s.notes))
}

/// Folder names, seeded order first.
pub fn folders() -> Vec<String> {
    with_store(|s| s.folders.clone())
}

// ---- Actions (named prompt fragments; OpenWhispr `actions` table) ----

pub fn actions() -> Vec<Action> {
    with_store(|s| s.actions.clone())
}

pub fn action_get(id: u64) -> Option<Action> {
    with_store(|s| s.actions.iter().find(|a| a.id == id).cloned())
}

pub fn action_create(name: &str, description: &str, prompt: &str) -> Result<Action, String> {
    let (name, prompt) = (name.trim(), prompt.trim());
    if name.is_empty() || prompt.is_empty() {
        return Err("An action needs a name and a prompt".to_string());
    }
    with_store(|s| {
        let id = s.actions.iter().map(|a| a.id).max().unwrap_or(0) + 1;
        let action = Action {
            id,
            name: name.to_string(),
            description: description.trim().to_string(),
            prompt: prompt.to_string(),
            builtin: false,
        };
        s.actions.push(action.clone());
        save_to_disk(s);
        Ok(action)
    })
}

pub fn action_update(
    id: u64,
    name: &str,
    description: &str,
    prompt: &str,
) -> Result<(), String> {
    let (name, prompt) = (name.trim(), prompt.trim());
    if name.is_empty() || prompt.is_empty() {
        return Err("An action needs a name and a prompt".to_string());
    }
    with_store(|s| {
        let action = s
            .actions
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or("Action not found")?;
        action.name = name.to_string();
        action.description = description.trim().to_string();
        action.prompt = prompt.to_string();
        save_to_disk(s);
        Ok(())
    })
}

/// Delete a custom action. Built-ins are protected (OpenWhispr semantics).
pub fn action_delete(id: u64) -> Result<(), String> {
    with_store(|s| {
        if s.actions.iter().any(|a| a.id == id && a.builtin) {
            return Err("Built-in actions can't be deleted".to_string());
        }
        s.actions.retain(|a| a.id != id);
        save_to_disk(s);
        Ok(())
    })
}

/// Add a folder (no-op if it already exists, case-insensitive).
pub fn folder_create(name: &str) -> Vec<String> {
    with_store(|s| {
        let trimmed = name.trim();
        if !trimmed.is_empty()
            && !s
                .folders
                .iter()
                .any(|f| f.eq_ignore_ascii_case(trimmed))
        {
            s.folders.push(trimmed.to_string());
            save_to_disk(s);
        }
        s.folders.clone()
    })
}

/// OpenWhispr's staleness marker: cheap, order-stable, good enough to answer
/// "did the raw content change since we enhanced it?".
pub fn content_hash(content: &str) -> String {
    let head: String = content.chars().take(50).collect();
    format!("{}:{}", content.len(), head)
}

/// Summaries for the notes list, newest-updated first:
/// `{ id, title, preview, updatedTs, hasEnhanced, stale, folder, source }`.
pub fn list() -> Value {
    with_notes(|notes| {
        let mut sorted: Vec<&Note> = notes.iter().collect();
        sorted.sort_by(|a, b| b.updated_ts.cmp(&a.updated_ts));
        let items: Vec<Value> = sorted
            .into_iter()
            .map(|n| {
                let preview: String = n.content.chars().take(120).collect();
                json!({
                    "id": n.id,
                    "title": n.title,
                    "preview": preview,
                    "updatedTs": n.updated_ts,
                    "hasEnhanced": !n.enhanced_content.is_empty(),
                    "stale": !n.enhanced_content.is_empty()
                        && n.enhanced_at_hash != content_hash(&n.content),
                    "folder": n.folder,
                    "source": n.source,
                    "noteType": n.note_type,
                })
            })
            .collect();
        json!(items)
    })
}

pub fn get(id: u64) -> Option<Note> {
    with_notes(|notes| notes.iter().find(|n| n.id == id).cloned())
}

/// All notes (cloned) — used by the AI Chat's keyword-RAG scorer.
pub fn all() -> Vec<Note> {
    with_notes(|notes| notes.clone())
}

pub fn create(title: &str, content: &str, source: &str, folder: &str) -> Note {
    with_store(|store| {
        let id = store.notes.iter().map(|n| n.id).max().unwrap_or(0) + 1;
        let now = now_secs();
        let folder = if folder.trim().is_empty() {
            default_folder()
        } else {
            folder.trim().to_string()
        };
        let note = Note {
            id,
            title: title.to_string(),
            content: content.to_string(),
            enhanced_content: String::new(),
            enhanced_at_hash: String::new(),
            note_type: default_note_type(),
            folder,
            transcript: Vec::new(),
            participants: Vec::new(),
            source: source.to_string(),
            created_ts: now,
            updated_ts: now,
        };
        store.notes.push(note.clone());
        save_to_disk(store);
        note
    })
}

/// Update title / raw content / folder; bumps `updated_ts`. Enhancement fields
/// are deliberately untouched — a content edit just makes the Enhanced tab stale.
pub fn update(
    id: u64,
    title: Option<String>,
    content: Option<String>,
    folder: Option<String>,
    participants: Option<Vec<String>>,
) -> Result<(), String> {
    with_store(|store| {
        let note = store
            .notes
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or("Note not found")?;
        if let Some(t) = title {
            note.title = t;
        }
        if let Some(c) = content {
            note.content = c;
        }
        if let Some(f) = folder {
            if !f.trim().is_empty() {
                note.folder = f.trim().to_string();
            }
        }
        if let Some(p) = participants {
            note.participants = p
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        note.updated_ts = now_secs();
        save_to_disk(store);
        Ok(())
    })
}

/// Append meeting-recorder segments to a note's transcript and mark it a
/// meeting note (the recorder persists every drain, so a crash loses at most
/// one chunk).
pub fn append_transcript(id: u64, segments: &[TranscriptSegment]) -> Result<(), String> {
    if segments.is_empty() {
        return Ok(());
    }
    with_store(|store| {
        let note = store
            .notes
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or("Note not found")?;
        note.transcript.extend_from_slice(segments);
        note.note_type = "meeting".to_string();
        note.updated_ts = now_secs();
        save_to_disk(store);
        Ok(())
    })
}

/// Store an enhancement result + the staleness hash of the content it was
/// computed from (pass the hash captured BEFORE the LLM call, so edits made
/// while the model ran correctly show as stale).
pub fn set_enhanced(id: u64, enhanced: &str, at_hash: &str) -> Result<(), String> {
    with_store(|store| {
        let note = store
            .notes
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or("Note not found")?;
        note.enhanced_content = enhanced.to_string();
        note.enhanced_at_hash = at_hash.to_string();
        note.updated_ts = now_secs();
        save_to_disk(store);
        Ok(())
    })
}

pub fn delete(id: u64) {
    with_store(|store| {
        store.notes.retain(|n| n.id != id);
        save_to_disk(store);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_tracks_length_and_head() {
        assert_eq!(content_hash("hello"), "5:hello");
        // Only the first 50 chars matter for the head…
        let long_a = format!("{}{}", "a".repeat(50), "tail-one");
        let long_b = format!("{}{}", "a".repeat(50), "tail-two");
        assert_eq!(content_hash(&long_a), content_hash(&long_b));
        // …but a length change is always caught.
        let longer = format!("{}{}", "a".repeat(50), "tail-longer");
        assert_ne!(content_hash(&long_a), content_hash(&longer));
    }
}

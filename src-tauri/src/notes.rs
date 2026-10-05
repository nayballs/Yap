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
/// unix-seconds timestamp of the audio it came from).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegment {
    pub source: String,
    pub text: String,
    pub ts: u64,
    /// A "you" segment that was the call audio leaking from the speakers into
    /// the mic (`meeting::is_echo`). Kept, but left out of the summary and
    /// hidden in the transcript view.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub echo: bool,
}

/// One action item in a meeting digest: who (a name, "You", or
/// "Unassigned"), what, and the deadline as it was said (empty = none).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DigestAction {
    pub owner: String,
    pub task: String,
    #[serde(default)]
    pub due: String,
}

/// A rolling summary of one stretch of a meeting (`meeting_summary.rs`),
/// written while the meeting is still going so the end-of-meeting summary
/// merges a few of these instead of re-reading hours of transcript.
/// Covers `transcript[from_seg..to_seg]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingDigest {
    pub from_seg: usize,
    pub to_seg: usize,
    /// Unix seconds of the first and last segment covered.
    pub start_ts: u64,
    pub end_ts: u64,
    #[serde(default)]
    pub key_points: Vec<String>,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub actions: Vec<DigestAction>,
    #[serde(default)]
    pub questions: Vec<String>,
    /// The model's reply, kept only when it had none of the expected sections.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub raw: String,
}

/// The calendar event a meeting note belongs to (`calendar.rs`): a copy taken
/// when the note was made from the calendar, or when a recording started
/// during the event, so the note keeps its context if the event changes or
/// the calendar is disconnected.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteEvent {
    /// The calendar's key for this occurrence.
    pub key: String,
    pub title: String,
    /// Unix seconds.
    pub start: u64,
    pub end: u64,
    /// The invite's description, cleaned (no links or dial-in details).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The meeting service ("teams", "meet", "zoom"…), if it had a link.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub service: String,
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
    /// Rolling meeting digests, in transcript order, each picking up where
    /// the last one stopped (`meeting_summary.rs`). Empty for short meetings
    /// and personal notes.
    #[serde(default)]
    pub digests: Vec<MeetingDigest>,
    /// Where the note came from ("manual" | "upload" | "meeting" | "calendar").
    #[serde(default)]
    pub source: String,
    pub created_ts: u64,
    pub updated_ts: u64,
    /// The calendar event this meeting note is for, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<NoteEvent>,
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
    /// What a built-in is for, when the app has to find it whatever the user
    /// renamed it to: [`ACTION_PLAN`] is the one "End meeting & summarise"
    /// runs (and runs under `llm::ACTION_PLAN_BASE_PROMPT`). Empty otherwise.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
}

/// `Action::kind` of the built-in "Action Plan".
pub const ACTION_PLAN: &str = "actionPlan";

/// Built-in action seeds: OpenWhispr's "Generate Notes" (database.js seed,
/// verbatim — the prompt is `llm::NOTE_DEFAULT_FRAGMENT`) plus three
/// meeting-focused templates (Yap additions; the meeting structure mirrors
/// OpenWhispr's MEETING_SYSTEM_PROMPT sections). `(name, description, prompt,
/// kind)`.
const BUILTIN_ACTIONS: [(&str, &str, &str, &str); 4] = [
    (
        "Generate Notes",
        "Clean up, structure, and enhance your notes",
        crate::llm::NOTE_DEFAULT_FRAGMENT,
        "",
    ),
    (
        "Meeting Notes",
        "Turn rough meeting notes into structured minutes",
        "The content is rough notes taken during a meeting (possibly including fragments of transcript). Produce clean meeting notes. Start with a concise 1\u{2013}2 sentence summary of what the meeting was about. Then use these section headings, omitting any that have no content: ## Key Discussion Points, ## Decisions Made, ## Action Items, ## Follow-ups. Under Action Items use checkboxes (- [ ]) and attribute each item to a person where clear. Consolidate repeated points into coherent ones, preserve specific commitments and dates verbatim, and bias toward brevity.",
        "",
    ),
    (
        "Action Items",
        "Extract just the tasks, owners, and deadlines",
        "Extract ONLY the action items from the content. Output a markdown checkbox list (- [ ]) with one task per line. When the owner is clear, start the line with their name and a colon (e.g. - [ ] Dave: send the revised budget). Include deadlines in parentheses when mentioned. Do not add tasks that weren't stated or clearly implied. If there are genuinely no action items, output exactly: No action items.",
        "",
    ),
    (
        "Action Plan",
        "Who does what by when, then decisions and open questions",
        crate::llm::ACTION_PLAN_DEFAULT_FRAGMENT,
        ACTION_PLAN,
    ),
];

fn default_actions() -> Vec<Action> {
    BUILTIN_ACTIONS
        .iter()
        .enumerate()
        .map(|(i, (name, description, prompt, kind))| Action {
            id: i as u64 + 1,
            name: name.to_string(),
            description: description.to_string(),
            prompt: prompt.to_string(),
            builtin: true,
            kind: kind.to_string(),
        })
        .collect()
}

/// Additive migration: stores created before a built-in existed get it added,
/// matched by kind for built-ins that have one, else by name, so user edits
/// to a built-in's name or prompt are never clobbered.
fn seed_missing_builtins(store: &mut Store) -> bool {
    let mut changed = false;
    for (name, description, prompt, kind) in BUILTIN_ACTIONS {
        let present = store.actions.iter().any(|a| {
            if kind.is_empty() {
                a.name.eq_ignore_ascii_case(name)
            } else {
                a.kind == kind
            }
        });
        if !present {
            let id = store.actions.iter().map(|a| a.id).max().unwrap_or(0) + 1;
            store.actions.push(Action {
                id,
                name: name.to_string(),
                description: description.to_string(),
                prompt: prompt.to_string(),
                builtin: true,
                kind: kind.to_string(),
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
            kind: String::new(),
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

/// Mark note `id` a meeting note before any transcript arrives (call
/// detection creates one and starts recording into it at once; see
/// `meeting_detect.rs`). Leaves `updated_ts` alone.
pub fn mark_meeting(id: u64) -> Result<(), String> {
    with_store(|store| {
        let note = store
            .notes
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or("Note not found")?;
        if note.note_type != "meeting" {
            note.note_type = "meeting".to_string();
            save_to_disk(store);
        }
        Ok(())
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
        sorted.sort_by_key(|n| std::cmp::Reverse(n.updated_ts));
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
            digests: Vec::new(),
            source: source.to_string(),
            created_ts: now,
            updated_ts: now,
            event: None,
        };
        store.notes.push(note.clone());
        save_to_disk(store);
        note
    })
}

/// Tie meeting note `id` to a calendar event (`calendar.rs`): keep the event,
/// take its `title` when given (the caller only passes one over a
/// placeholder), add its attendees after the ones already there, and, for a
/// note made ahead of the meeting, date it to the meeting (`date`).
pub fn link_event(
    id: u64,
    event: NoteEvent,
    title: Option<String>,
    attendees: &[String],
    date: Option<u64>,
) -> Result<(), String> {
    with_store(|store| {
        let note = store
            .notes
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or("Note not found")?;
        if let Some(title) = title.filter(|t| !t.trim().is_empty()) {
            note.title = title;
        }
        for name in attendees {
            let name = name.trim();
            if !name.is_empty() && !note.participants.iter().any(|p| p.eq_ignore_ascii_case(name)) {
                note.participants.push(name.to_string());
            }
        }
        if let Some(date) = date {
            note.created_ts = date;
        }
        note.note_type = "meeting".to_string();
        note.event = Some(event);
        note.updated_ts = now_secs();
        save_to_disk(store);
        Ok(())
    })
}

/// The note made for calendar event `key` (the most recently edited, if a
/// meeting was recorded twice).
pub fn find_by_event(key: &str) -> Option<u64> {
    with_notes(|notes| {
        notes
            .iter()
            .filter(|n| n.event.as_ref().is_some_and(|e| e.key == key))
            .max_by_key(|n| n.updated_ts)
            .map(|n| n.id)
    })
}

/// (note id, event key) for every note tied to an event (the Meetings view).
pub fn event_links() -> Vec<(u64, String)> {
    with_notes(|notes| {
        notes
            .iter()
            .filter_map(|n| n.event.as_ref().map(|e| (n.id, e.key.clone())))
            .collect()
    })
}

/// Update title / raw content / folder / attendees. Enhancement fields are
/// deliberately untouched — a content edit just makes the Enhanced tab stale.
/// Only a real change bumps `updated_ts` and hits the disk (see `apply_update`).
pub fn update(
    id: u64,
    title: Option<String>,
    content: Option<String>,
    folder: Option<String>,
    participants: Option<Vec<String>>,
) -> Result<(), String> {
    with_store(|store| {
        if apply_update(store, id, title, content, folder, participants)? {
            save_to_disk(store);
        }
        Ok(())
    })
}

/// Apply an edit to note `id`; returns whether anything actually changed.
/// Re-sending identical values (the editor flushing a note that was only
/// viewed, a no-op API PATCH) leaves `updated_ts` alone, so the list doesn't
/// float a note nobody edited to the top as "now".
fn apply_update(
    store: &mut Store,
    id: u64,
    title: Option<String>,
    content: Option<String>,
    folder: Option<String>,
    participants: Option<Vec<String>>,
) -> Result<bool, String> {
    let note = store
        .notes
        .iter_mut()
        .find(|n| n.id == id)
        .ok_or("Note not found")?;
    let mut changed = false;
    if let Some(t) = title {
        changed |= set_if_changed(&mut note.title, t);
    }
    if let Some(c) = content {
        changed |= set_if_changed(&mut note.content, c);
    }
    if let Some(f) = folder {
        if !f.trim().is_empty() {
            changed |= set_if_changed(&mut note.folder, f.trim().to_string());
        }
    }
    if let Some(p) = participants {
        let p: Vec<String> = p
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        changed |= set_if_changed(&mut note.participants, p);
    }
    if changed {
        note.updated_ts = now_secs();
    }
    Ok(changed)
}

fn set_if_changed<T: PartialEq>(slot: &mut T, value: T) -> bool {
    if *slot == value {
        return false;
    }
    *slot = value;
    true
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

/// How far into a note's transcript the meeting digests reach (the first
/// segment no digest covers yet).
pub fn digested_upto(note: &Note) -> usize {
    note.digests.last().map(|d| d.to_seg).unwrap_or(0)
}

/// Append a meeting digest. It must pick up exactly where the last one
/// stopped and stay inside the transcript, so a late or repeated result can
/// never leave a gap or cover the same stretch twice. Returns the new count.
pub fn add_digest(id: u64, digest: MeetingDigest) -> Result<usize, String> {
    with_store(|store| {
        let note = store
            .notes
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or("Note not found")?;
        let count = push_digest(note, digest)?;
        save_to_disk(store);
        Ok(count)
    })
}

fn push_digest(note: &mut Note, digest: MeetingDigest) -> Result<usize, String> {
    if digest.from_seg != digested_upto(note)
        || digest.to_seg <= digest.from_seg
        || digest.to_seg > note.transcript.len()
    {
        return Err("Digest doesn't continue the note's digests".to_string());
    }
    note.digests.push(digest);
    Ok(note.digests.len())
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

    fn store_with_note() -> Store {
        let note: Note = serde_json::from_value(json!({
            "id": 1,
            "title": "Standup",
            "content": "Ship the fix",
            "participants": ["Dave"],
            "createdTs": 1,
            "updatedTs": 1,
        }))
        .unwrap();
        Store {
            notes: vec![note],
            ..Store::default()
        }
    }

    #[test]
    fn saving_unchanged_values_keeps_updated_ts() {
        let mut store = store_with_note();
        // What the editor sends for a note that was only viewed, with the
        // folder/attendees spelled the way update() normalizes them anyway.
        let changed = apply_update(
            &mut store,
            1,
            Some("Standup".to_string()),
            Some("Ship the fix".to_string()),
            Some(" Personal ".to_string()),
            Some(vec![" Dave ".to_string(), String::new()]),
        )
        .unwrap();
        assert!(!changed);
        assert_eq!(store.notes[0].updated_ts, 1);
    }

    #[test]
    fn older_stores_load_and_get_the_action_plan() {
        // A store from before digests, echo flags and action kinds, whose
        // user edited the Meeting Notes prompt.
        let old = r#"{
            "folders": ["Personal", "Meetings"],
            "actions": [
                {"id": 1, "name": "Generate Notes", "prompt": "p1", "builtin": true},
                {"id": 2, "name": "Meeting Notes", "prompt": "my own minutes", "builtin": true},
                {"id": 3, "name": "Action Items", "prompt": "p3", "builtin": true}
            ],
            "notes": [{"id": 7, "title": "Sync", "noteType": "meeting",
                       "transcript": [{"source": "you", "text": "hi", "ts": 5}],
                       "createdTs": 1, "updatedTs": 1}]
        }"#;
        let mut store: Store = serde_json::from_str(old).unwrap();
        assert!(store.notes[0].digests.is_empty());
        assert!(!store.notes[0].transcript[0].echo);
        assert!(seed_missing_builtins(&mut store));
        let plan: Vec<&Action> = store.actions.iter().filter(|a| a.kind == ACTION_PLAN).collect();
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].id, 4);
        assert!(plan[0].builtin);
        // User edits survive; a second pass adds nothing.
        assert_eq!(store.actions[1].prompt, "my own minutes");
        assert!(!seed_missing_builtins(&mut store));
        // A renamed Action Plan is still found by kind (not re-added).
        store.actions[3].name = "Who does what".to_string();
        assert!(!seed_missing_builtins(&mut store));
        // An echo flag only reaches disk when set.
        let seg = serde_json::to_value(&store.notes[0].transcript[0]).unwrap();
        assert!(seg.get("echo").is_none());
    }

    #[test]
    fn digests_must_continue_each_other() {
        let mut note = store_with_note().notes.remove(0);
        note.transcript = (0..10)
            .map(|i| TranscriptSegment {
                source: "them".into(),
                text: format!("line {i}"),
                ts: i,
                echo: false,
            })
            .collect();
        let d = |from, to| MeetingDigest {
            from_seg: from,
            to_seg: to,
            start_ts: from as u64,
            end_ts: to as u64,
            key_points: vec![],
            decisions: vec![],
            actions: vec![],
            questions: vec![],
            raw: String::new(),
        };
        assert_eq!(push_digest(&mut note, d(0, 4)), Ok(1));
        assert!(push_digest(&mut note, d(0, 4)).is_err()); // the same stretch again
        assert!(push_digest(&mut note, d(5, 8)).is_err()); // a gap
        assert!(push_digest(&mut note, d(4, 11)).is_err()); // past the end
        assert_eq!(push_digest(&mut note, d(4, 10)), Ok(2));
        assert_eq!(digested_upto(&note), 10);
    }

    #[test]
    fn a_real_edit_bumps_updated_ts() {
        let mut store = store_with_note();
        let changed = apply_update(
            &mut store,
            1,
            Some("Standup".to_string()),
            Some("Ship the fix today".to_string()),
            None,
            None,
        )
        .unwrap();
        assert!(changed);
        assert_eq!(store.notes[0].content, "Ship the fix today");
        assert!(store.notes[0].updated_ts > 1);
        assert!(apply_update(&mut store, 2, None, None, None, None).is_err());
    }
}

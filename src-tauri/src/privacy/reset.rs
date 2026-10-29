//! **Reset app** (Settings → Privacy & data): "Clear everything and keep my
//! settings". What goes ([`GOES`]): the dictation history and its audio,
//! notes and meetings with their audio, chats and their screenshots, what
//! Yap remembers, tasks, the search index and the usage counts (and the
//! pre-meeting briefs, which quote notes and memory). What stays
//! ([`STAYS`]): config.json — every setting, the dictionary and snippets —,
//! the downloaded models, the account's sign-in, the calendars, and Yap's
//! own bookkeeping (tips shown, answers to offers, caches, the log).
//!
//! Each store is emptied **through its own module, under its own lock**
//! (`history::clear`, `notes::reset`, `chats::reset`, `memory::reset`…), so
//! what a store holds in memory goes with its file and no later save writes
//! the old data back; Undo's copies go too (memory's). Set-aside copies of
//! those stores (`<name>.corrupt-…`) go with them. Every window is told
//! (each note let go of, the stores' change events, `yap-data-reset`), so
//! Yap is clean without a restart. Refused while a dictation or a meeting
//! records (what they'd save next would come back).
//!
//! A unit test holds every file `docs/architecture/config-and-data.md`
//! names to a decision here: a new store has to be put in one list or the
//! other before the test passes.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// Every window's event once a reset is done: the main window shows its
/// views afresh.
pub const EVENT: &str = "yap-data-reset";

/// A part of what a reset deletes, as the dialog names it, and its files
/// in the data folder (a folder ends in `/`; one in brackets is part of a
/// file that stays).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Part {
    pub id: &'static str,
    pub label: &'static str,
    #[serde(skip)]
    pub files: &'static [&'static str],
}

/// What a reset deletes, in the order the dialog says it.
pub const GOES: &[Part] = &[
    Part { id: "history", label: "Your dictation history, and the last day of dictation audio", files: &["history.json", "recent-audio/"] },
    Part {
        id: "notes",
        label: "Every note and meeting, with their audio, earlier versions and pins (your folders and actions stay)",
        files: &["notes.json", "meeting-audio/", "note-versions/", "note_pins.json"],
    },
    Part { id: "chats", label: "Every chat, with its screenshots", files: &["chats.json", "chat-images/"] },
    Part { id: "memory", label: "What Yap remembers about you", files: &["memory.json"] },
    Part { id: "tasks", label: "Your tasks", files: &["tasks.json"] },
    Part { id: "search", label: "The search index", files: &["search-index/"] },
    Part { id: "usage", label: "Usage stats: AI usage and Groq's meter", files: &["ai_usage.json", "groq_usage.json"] },
    Part { id: "briefs", label: "Pre-meeting briefs (your calendars stay connected)", files: &["(calendar.json)"] },
];

/// What a reset keeps, as the dialog names it, and its files.
pub const STAYS: &[Part] = &[
    Part { id: "settings", label: "Your settings, dictionary and snippets", files: &["config.json", "dictionary_usage.json"] },
    Part { id: "models", label: "Downloaded speech, AI and voice models", files: &["models/", "llm/"] },
    Part { id: "account", label: "Your account sign-in, and your calendars", files: &["calendar.json"] },
    Part {
        id: "bookkeeping",
        label: "Yap's own records: tips shown, your answers to its offers, network receipts, caches and the log",
        files: &[
            "hints.json",
            "mic_tips.json",
            "notices.json",
            "word_suggestions.json",
            "snippet_suggestions.json",
            "language_nudges.json",
            "connections.json",
            "updates.json",
            "model_catalog.json",
            "stt_speed.json",
            "quieted_apps.json",
            "call_speakers.json",
            "icons/",
            "logs/",
        ],
    },
];

/// The stores whose set-aside copies (`<stem>.corrupt-…`, `config::set_aside`)
/// go with them: they hold the same content.
const SET_ASIDE: &[&str] = &["history", "notes", "note_pins", "chats", "memory", "tasks", "ai_usage", "groq_usage"];

/// Whether `file` (a name in the data folder) is a set-aside copy of a
/// store that goes. Pure.
pub fn set_aside_copy(file: &str) -> bool {
    SET_ASIDE.iter().any(|stem| file.strip_prefix(stem).and_then(|r| r.strip_prefix(".corrupt")).is_some())
}

/// The dialog's lists.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub goes: Vec<Line>,
    pub stays: Vec<&'static str>,
    /// Why it can't run now ("Finish the dictation first"), if it can't.
    pub blocked: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    pub id: &'static str,
    pub label: &'static str,
    /// How many there are now ("214 dictations"), when it's counted.
    pub count: Option<String>,
    /// Its files in the data folder (the dialog's small print).
    pub files: &'static [&'static str],
}

/// "214 dictations", "1 chat"; nothing for none. Pure.
pub fn count(n: usize, one: &str, many: &str) -> Option<String> {
    match n {
        0 => None,
        1 => Some(format!("1 {one}")),
        n => Some(format!("{n} {many}")),
    }
}

/// Why a reset can't run now, if it can't: a dictation or a meeting
/// recording (they'd save what they hold after it). Pure.
pub fn blocked(dictating: bool, meeting: bool) -> Option<String> {
    if meeting {
        Some("Stop the meeting first: Yap is recording it.".into())
    } else if dictating {
        Some("Finish the dictation first.".into())
    } else {
        None
    }
}

fn busy(app: &AppHandle) -> Option<String> {
    let dictating = app
        .try_state::<crate::AppState>()
        .and_then(|s| s.pipeline.lock().ok().map(|p| p.as_ref().is_some_and(|p| p.is_busy())))
        .unwrap_or(false);
    blocked(dictating, crate::meetings::meeting::is_recording())
}

/// Settings' dialog: what goes (with counts), what stays, and whether it
/// can run now.
#[tauri::command]
pub async fn data_reset_plan(app: AppHandle) -> Plan {
    let blocked = busy(&app);
    tauri::async_runtime::spawn_blocking(move || {
        let notes = crate::notes::all();
        let meetings = notes.iter().filter(|n| crate::integrations::mcp::is_meeting(n)).count();
        let counts = |id: &str| -> Option<String> {
            match id {
                "history" => count(crate::dictation::history::all().len(), "dictation", "dictations"),
                "notes" => {
                    let parts: Vec<String> = [count(notes.len() - meetings, "note", "notes"), count(meetings, "meeting", "meetings")]
                        .into_iter()
                        .flatten()
                        .collect();
                    (!parts.is_empty()).then(|| parts.join(", "))
                }
                "chats" => count(crate::chat::chats::with_all(|c| c.len()), "chat", "chats"),
                "memory" => count(crate::memory::items().len(), "thing", "things"),
                "tasks" => count(crate::notes::task_store::snapshot().tasks.len(), "task", "tasks"),
                _ => None,
            }
        };
        Plan {
            goes: GOES.iter().map(|p| Line { id: p.id, label: p.label, count: counts(p.id), files: p.files }).collect(),
            stays: STAYS.iter().map(|p| p.label).collect(),
            blocked,
        }
    })
    .await
    .unwrap_or(Plan { goes: Vec::new(), stays: Vec::new(), blocked: Some("Yap couldn't look at its data.".into()) })
}

/// What a reset did (counts only).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Done {
    pub notes: usize,
    pub chats: usize,
    pub memory: usize,
    pub tasks: usize,
    pub screenshots: usize,
    pub meeting_audio: usize,
    pub briefs: usize,
    pub set_aside: usize,
    /// Whether Yap needs a restart to finish (never so far: kept for the page).
    pub restart: bool,
}

/// Every store that goes, emptied in memory and on disk, store by store.
fn run(app: &AppHandle) -> Result<Done, String> {
    let mut done = Done::default();
    // History, and the day of dictation audio with it.
    crate::dictation::history::clear();
    // Notes and meetings: every window lets go of each.
    let ids = crate::notes::reset();
    done.notes = ids.len();
    // Their version history and pins (notes/versions.rs, notes/pins.rs).
    crate::notes::versions::reset();
    crate::notes::pins::reset();
    for id in &ids {
        let _ = app.emit(crate::meetings::meeting_end::EVENT_DELETED, serde_json::json!({ "id": id }));
        crate::meetings::notepad::on_note_deleted(app, *id);
    }
    done.meeting_audio = crate::meetings::meeting_audio::clear();
    let audio_dir = crate::config::data_dir().join("meeting-audio");
    if audio_dir.exists() && !crate::meetings::meeting::is_recording() {
        let _ = std::fs::remove_dir_all(&audio_dir);
    }
    // Chats, and every screenshot (the ones waiting in Chat's box too).
    done.chats = crate::chat::chats::reset();
    done.screenshots = crate::snip::chat_images_delete_all(app.clone());
    done.memory = crate::memory::reset();
    done.tasks = crate::notes::task_store::reset();
    crate::search::index::forget()?;
    crate::search::index::emit(app);
    crate::ai::usage::store::reset();
    crate::ai::usage::groq::reset();
    done.briefs = crate::calendar::forget_briefs();
    // Set-aside copies of those stores.
    let data = crate::config::data_dir();
    for e in std::fs::read_dir(&data).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if set_aside_copy(&name) && e.path().is_file() && std::fs::remove_file(e.path()).is_ok() {
            done.set_aside += 1;
        }
    }
    // Every window: the stores' own events, then the reset's.
    for event in ["yap-history-changed", "yap-notes-changed", "yap-tasks-changed"] {
        let _ = app.emit(event, ());
    }
    let _ = app.emit(EVENT, &done);
    // A save that failed is said (the store keeps it empty in memory and
    // tries again; saving.rs).
    crate::dictation::history::saved()?;
    crate::notes::saved()?;
    crate::notes::pins::SAVER.check()?;
    crate::chat::chats::saved()?;
    crate::memory::saved()?;
    crate::notes::task_store::saved()?;
    Ok(done)
}

/// Reset app: clear everything Yap keeps but the settings, the dictionary,
/// the models and the account (see [`GOES`] / [`STAYS`]). Refused while a
/// dictation or a meeting records.
#[tauri::command]
pub async fn data_reset(app: AppHandle) -> Result<Done, String> {
    if let Some(why) = busy(&app) {
        return Err(why);
    }
    let handle = app.clone();
    let done = tauri::async_runtime::spawn_blocking(move || run(&handle)).await.map_err(|e| e.to_string())??;
    tracing::info!(
        notes = done.notes,
        chats = done.chats,
        memory = done.memory,
        tasks = done.tasks,
        screenshots = done.screenshots,
        meeting_audio = done.meeting_audio,
        briefs = done.briefs,
        set_aside = done.set_aside,
        "Reset app: done"
    );
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every file and folder config-and-data.md names in the data folder is
    /// in one list or the other: a store added there has to be decided here
    /// (and the dialog's words with it).
    #[test]
    fn every_data_file_the_docs_name_is_cleared_or_kept() {
        let doc = include_str!("../../../docs/architecture/config-and-data.md");
        let re = regex::Regex::new(r"(?:%APPDATA%[/\\]yap[/\\]|<data>/)([A-Za-z0-9_.-]+)(/?)").unwrap();
        let named: std::collections::BTreeSet<String> =
            re.captures_iter(doc).map(|c| format!("{}{}", &c[1], if c[2].is_empty() && !c[1].contains('.') { "/" } else { &c[2] })).collect();
        assert!(named.len() >= 20, "{named:?}");
        let listed: Vec<&str> = GOES.iter().chain(STAYS).flat_map(|p| p.files.iter().copied()).collect();
        let decided = |f: &str| listed.iter().any(|l| *l == f || l.trim_end_matches('/') == f.trim_end_matches('/'));
        let undecided: Vec<&String> = named.iter().filter(|f| !decided(f)).collect();
        assert!(undecided.is_empty(), "Reset app: decide what happens to {undecided:?} (privacy/reset.rs GOES or STAYS)");
        // Nothing is both.
        for g in GOES.iter().flat_map(|p| p.files) {
            assert!(!STAYS.iter().flat_map(|p| p.files).any(|s| s == g), "{g} goes and stays");
        }
    }

    #[test]
    fn what_goes_is_what_was_asked_and_settings_stay() {
        let goes: Vec<&str> = GOES.iter().flat_map(|p| p.files.iter().copied()).collect();
        for f in ["history.json", "recent-audio/", "notes.json", "meeting-audio/", "note-versions/", "note_pins.json", "chats.json", "chat-images/", "memory.json", "tasks.json", "search-index/", "ai_usage.json"] {
            assert!(goes.contains(&f), "{f} goes");
        }
        let stays: Vec<&str> = STAYS.iter().flat_map(|p| p.files.iter().copied()).collect();
        for f in ["config.json", "models/", "llm/"] {
            assert!(stays.contains(&f), "{f} stays");
        }
        // Each line of the dialog says what it is in plain words.
        assert!(GOES.iter().all(|p| !p.label.is_empty() && !p.label.ends_with('.')));
    }

    #[test]
    fn set_aside_copies_of_what_goes_go_too() {
        assert!(set_aside_copy("notes.corrupt-1790000000"));
        assert!(set_aside_copy("history.corrupt-1790000000-2"));
        assert!(set_aside_copy("ai_usage.corrupt"));
        assert!(!set_aside_copy("config.corrupt-1790000000"), "the settings stay, set-aside ones too");
        assert!(!set_aside_copy("notes.json"));
        assert!(!set_aside_copy("notes_extra.corrupt-1"));
        assert!(!set_aside_copy("dictionary_usage.corrupt-1"));
    }

    #[test]
    fn a_reset_waits_for_a_dictation_or_a_meeting() {
        assert_eq!(blocked(false, false), None);
        assert!(blocked(true, false).unwrap().contains("dictation"));
        assert!(blocked(true, true).unwrap().contains("meeting"));
        assert_eq!(count(0, "chat", "chats"), None);
        assert_eq!(count(1, "chat", "chats").as_deref(), Some("1 chat"));
        assert_eq!(count(3, "chat", "chats").as_deref(), Some("3 chats"));
    }

    /// The stores empty in memory too: what a later save writes is nothing
    /// (unit tests run on in-memory stores of their own).
    #[test]
    fn the_stores_are_empty_in_memory_after_a_reset() {
        crate::chat::chats::create("A chat about leeks");
        crate::notes::create("Leeks", "by the shed", "manual", "");
        let _ = crate::notes::task_store::modify(|s| {
            s.tasks.push(serde_json::from_value(serde_json::json!({ "id": 1, "text": "Water the leeks", "createdMs": 1 })).unwrap());
            Ok(((), true))
        });
        assert_eq!(crate::chat::chats::reset(), 1);
        assert_eq!(crate::notes::reset().len(), 1);
        assert_eq!(crate::notes::task_store::reset(), 1);
        assert_eq!(crate::chat::chats::with_all(|c| c.len()), 0);
        assert!(crate::notes::all().is_empty());
        assert!(crate::notes::task_store::snapshot().tasks.is_empty());
        // The folders and actions stay.
        assert!(!crate::notes::folders().is_empty());
        assert!(!crate::notes::actions().is_empty());
    }
}

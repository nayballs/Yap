//! **Deleting your data** (Settings → Privacy & data → Your data): **Reset
//! app** ("Clear everything and keep my settings") and **Delete some of
//! your data…** (a tick box per kind). Both delete [`Kind`]s of data, the
//! one way: Reset app is every kind.
//!
//! - **The kinds** ([`KINDS`], the dialog's words): dictation history (and
//!   its audio), a day of dictation audio, the dictation stats; meetings
//!   (their transcripts, audio, summaries, tasks, Ask threads and earlier
//!   versions), the other notes, the tasks added on their own; chats (and
//!   their screenshots), what Yap remembers; the dictionary's suggestions;
//!   AI usage counts, network receipts, the search index and the logs. Each
//!   names its files in the data folder, whole or in part (`(notes.json)`).
//! - **What stays** ([`STAYS`]): config.json — every setting, the
//!   dictionary, snippets and transforms —, the person's own Skills, meeting
//!   folder rules and writing samples (set up on purpose), the downloaded
//!   models, the account's sign-in, the calendars, paired phones, and Yap's
//!   own bookkeeping (tips shown, answers to its offers, caches).
//! - **One way per store** ([`Choice`]): a store goes whole when every kind
//!   that owns part of it is chosen (notes.json when meetings and the other
//!   notes both go; tasks.json and chats.json when the tasks and the chats
//!   go too), else only the chosen kinds' part of it. Each is emptied
//!   **through its own module, under its own lock** (`history::clear`,
//!   `notes::remove_where`, `chats::delete_where`, `memory::reset`…), so what
//!   a store holds in memory goes with its file and no later save writes the
//!   old data back; Undo's copies go too (memory's). What goes with a note
//!   goes with it, as when it's deleted by hand: its tasks, Ask threads,
//!   versions, pin and audio. The search index drops the vectors of what
//!   went and memory forgets what it read of it (`memory::forget_sources`),
//!   the pre-meeting briefs (they quote notes and memory) are made again from
//!   what's left. Set-aside copies of a store (`<name>.corrupt-…`) go when
//!   the whole store does. Every window is told (each note let go of, the
//!   stores' change events, `yap-data-reset` with what went), so Yap is
//!   clean without a restart. Refused while a dictation or a meeting records
//!   (what they'd save next would come back), and while a store a kind
//!   changes couldn't be read.
//!
//! Unit tests hold every file `docs/architecture/config-and-data.md` names
//! to the kinds that own it (or to [`STAYS`]), and each kind to exactly the
//! files it names: a new store has to be decided here before they pass.

use std::collections::{BTreeSet, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::integrations::mcp::is_meeting;

/// Every window's event once a deletion is done (its [`Done`]): the main
/// window shows its views afresh.
pub const EVENT: &str = "yap-data-reset";

/// A kind of data the person can delete on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    History,
    DictationAudio,
    Stats,
    Meetings,
    Notes,
    Tasks,
    Chats,
    Memory,
    Suggestions,
    AiUsage,
    Receipts,
    SearchIndex,
    Logs,
}

/// A kind as the dialog names it, and what it changes.
#[derive(Debug, Clone, Copy)]
pub struct About {
    pub kind: Kind,
    /// Its heading in the dialog.
    pub group: &'static str,
    pub label: &'static str,
    /// What it removes, in plain words.
    pub says: &'static str,
    /// Its files in the data folder (a folder ends in `/`): whole, or in
    /// brackets for the part of one it changes.
    pub files: &'static [&'static str],
    /// The stores it changes (`saving::Saver` ids): one whose file couldn't
    /// be read holds it back.
    pub stores: &'static [&'static str],
}

const DICTATION: &str = "Dictation";
const NOTES: &str = "Notes and meetings";
const CHAT: &str = "Chat and memory";
const DICTIONARY: &str = "Dictionary";
const RECORDS: &str = "Yap's own records";

/// Every kind, in the dialog's order.
pub const KINDS: &[About] = &[
    About {
        kind: Kind::History,
        group: DICTATION,
        label: "Dictation history",
        says: "Every dictation in your history, with its audio. Your stats stay",
        files: &["history.json", "recent-audio/"],
        stores: &["history"],
    },
    About {
        kind: Kind::DictationAudio,
        group: DICTATION,
        label: "Dictation audio",
        says: "The last day's recordings of your dictations, and any you kept. Your history stays",
        files: &["recent-audio/"],
        stores: &[],
    },
    // They outlast clearing history on purpose ("Keep history"'s copy in
    // them stays: config.json has the choice).
    About {
        kind: Kind::Stats,
        group: DICTATION,
        label: "Dictation stats",
        says: "The numbers on Home and Insights: words, streaks, time saved and the apps you dictate into",
        files: &["dictation_stats.json"],
        stores: &["dictationStats"],
    },
    About {
        kind: Kind::Meetings,
        group: NOTES,
        label: "Meetings",
        says: "Every meeting, with its transcript, audio, summary, tasks, Ask threads and earlier versions",
        files: &[
            "(notes.json)",
            "meeting-audio/",
            "(note-versions/)",
            "(note_pins.json)",
            "(tasks.json)",
            "(chats.json)",
            "(memory.json)",
            "(search-index/)",
            "(calendar.json)",
        ],
        stores: &["notes", "note-versions", "note-pins", "tasks", "chats", "memory", "calendar"],
    },
    About {
        kind: Kind::Notes,
        group: NOTES,
        label: "Other notes",
        says: "Every note that isn't a meeting, quick notes and Chat's documents too, with their tasks, earlier versions and pins. Your folders and actions stay",
        files: &[
            "(notes.json)",
            "(meeting-audio/)",
            "(note-versions/)",
            "(note_pins.json)",
            "quick_note_tabs.json",
            "(tasks.json)",
            "(chats.json)",
            "(memory.json)",
            "(search-index/)",
            "(calendar.json)",
        ],
        stores: &["notes", "note-versions", "note-pins", "quickNoteTabs", "tasks", "chats", "memory", "calendar"],
    },
    About {
        kind: Kind::Tasks,
        group: NOTES,
        label: "Tasks you added",
        says: "Tasks you added in the Tasks view, not in a meeting. A meeting's tasks go with it",
        files: &["(tasks.json)"],
        stores: &["tasks"],
    },
    About {
        kind: Kind::Chats,
        group: CHAT,
        label: "Chats",
        says: "Every chat, archived ones too, with its screenshots. Documents Chat wrote stay in Notes",
        files: &["(chats.json)", "chat-images/", "(memory.json)", "(search-index/)"],
        stores: &["chats", "memory"],
    },
    About {
        kind: Kind::Memory,
        group: CHAT,
        label: "What Yap remembers",
        says: "Everything in Settings → Memory: the people, projects and topics Yap remembers about you",
        files: &["memory.json", "(search-index/)", "(calendar.json)"],
        stores: &["memory", "calendar"],
    },
    About {
        kind: Kind::Suggestions,
        group: DICTIONARY,
        label: "Dictionary suggestions",
        says: "The words Yap offered for your dictionary and your answers (it may offer them again), and the phrases it noted for snippets. Your dictionary and snippets stay",
        files: &["word_suggestions.json", "(snippet_suggestions.json)"],
        stores: &["wordSuggestions", "snippetSuggestions"],
    },
    About {
        kind: Kind::AiUsage,
        group: RECORDS,
        label: "AI usage counts",
        says: "The token counts in AI usage, and Groq's meter for today (its daily limit is kept)",
        files: &["ai_usage.json", "groq_usage.json"],
        stores: &["aiUsage", "usage"],
    },
    About {
        kind: Kind::Receipts,
        group: RECORDS,
        label: "Network receipts",
        says: "The last week's list in Network connections",
        files: &["connections.json"],
        stores: &["connections"],
    },
    About {
        kind: Kind::SearchIndex,
        group: RECORDS,
        label: "Search index",
        says: "Search by meaning's index of your notes, chats and memory. It's built again from what's left while search by meaning is on",
        files: &["search-index/"],
        stores: &[],
    },
    About {
        kind: Kind::Logs,
        group: RECORDS,
        label: "Logs",
        says: "Yap's log files, which Report a problem can include. Today's is emptied and carries on",
        files: &["logs/"],
        stores: &[],
    },
];

/// How kind `kind` is described.
pub fn about(kind: Kind) -> &'static About {
    KINDS.iter().find(|a| a.kind == kind).expect("every kind is in KINDS")
}

/// A part of what no deletion touches, as the dialog names it, and its
/// files in the data folder (a folder ends in `/`).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Part {
    pub id: &'static str,
    pub label: &'static str,
    /// Read by the tests that hold config-and-data.md to these lists.
    #[serde(skip)]
    #[cfg_attr(not(test), allow(dead_code))]
    pub files: &'static [&'static str],
}

/// What every deletion keeps, Reset app's too, as the dialog names it. (A
/// file here may still lose a part a kind names: calendar.json its briefs,
/// snippet_suggestions.json its noted phrases.)
pub const STAYS: &[Part] = &[
    // Skills and meeting folder rules are setup, like folders and actions: they stay.
    Part {
        id: "settings",
        label: "Your settings, dictionary, snippets and transforms, your Skills and meeting folder rules",
        files: &["config.json", "dictionary_usage.json", "skills.json", "meeting_folders.json"],
    },
    // Set up on purpose, like the dictionary (Settings → Commands).
    Part { id: "samples", label: "Your writing samples", files: &["writing_samples.json"] },
    Part { id: "models", label: "Downloaded speech, AI and voice models", files: &["models/", "llm/"] },
    // Paired phones are set up like an account sign-in (link.rs): they stay.
    Part { id: "account", label: "Your account sign-in, your calendars and paired phones", files: &["calendar.json", "link_devices.json", "link/"] },
    Part {
        id: "bookkeeping",
        label: "Yap's own records: tips shown, your answers to its offers, and caches",
        files: &[
            "hints.json",
            "mic_tips.json",
            "onboarding.json",
            "call_apps.json",
            "notices.json",
            // Its switch ("Suggest snippets…"); the phrases it noted go with
            // the dictionary's suggestions.
            "snippet_suggestions.json",
            "language_nudges.json",
            "updates.json",
            "model_catalog.json",
            "stt_speed.json",
            "quieted_apps.json",
            "call_speakers.json",
            "icons/",
        ],
    },
];

// ---- what a choice does, store by store (pure) ----------------------------------------------

/// What a deletion does to one store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cut {
    Keep,
    /// The chosen kinds' part of it.
    Part,
    Whole,
}

impl Cut {
    fn of(whole: bool, part: bool) -> Cut {
        if whole {
            Cut::Whole
        } else if part {
            Cut::Part
        } else {
            Cut::Keep
        }
    }
}

/// The kinds chosen, and what that does to each store. Pure.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Choice(BTreeSet<Kind>);

impl Choice {
    pub fn of(kinds: impl IntoIterator<Item = Kind>) -> Choice {
        Choice(kinds.into_iter().collect())
    }

    /// Reset app.
    pub fn every() -> Choice {
        Choice::of(KINDS.iter().map(|a| a.kind))
    }

    pub fn has(&self, kind: Kind) -> bool {
        self.0.contains(&kind)
    }

    fn any(&self, kinds: &[Kind]) -> bool {
        kinds.iter().any(|k| self.has(*k))
    }

    fn all(&self, kinds: &[Kind]) -> bool {
        kinds.iter().all(|k| self.has(*k))
    }

    pub fn kinds(&self) -> Vec<Kind> {
        self.0.iter().copied().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// history.json (`history::clear`, which takes its audio too).
    pub fn history(&self) -> Cut {
        Cut::of(self.has(Kind::History), false)
    }

    /// recent-audio/ (`recent_audio::clear`).
    pub fn dictation_audio(&self) -> Cut {
        Cut::of(self.any(&[Kind::History, Kind::DictationAudio]), false)
    }

    pub fn stats(&self) -> Cut {
        Cut::of(self.has(Kind::Stats), false)
    }

    /// notes.json's notes, and their versions and pins with them.
    pub fn notes(&self) -> Cut {
        const OWNERS: &[Kind] = &[Kind::Meetings, Kind::Notes];
        Cut::of(self.all(OWNERS), self.any(OWNERS))
    }

    /// meeting-audio/: every recording is a meeting's (an other note's own,
    /// should one have any, goes with it).
    pub fn meeting_audio(&self) -> Cut {
        Cut::of(self.has(Kind::Meetings), self.has(Kind::Notes))
    }

    /// The quick note's tabs: quick notes are other notes.
    pub fn quick_note_tabs(&self) -> Cut {
        Cut::of(self.has(Kind::Notes), false)
    }

    /// tasks.json: a meeting's tasks, a note's, and those added on their own.
    pub fn tasks(&self) -> Cut {
        const OWNERS: &[Kind] = &[Kind::Meetings, Kind::Notes, Kind::Tasks];
        Cut::of(self.all(OWNERS), self.any(OWNERS))
    }

    /// chats.json: Chat's conversations, and meetings' and notes' Ask threads.
    pub fn chats(&self) -> Cut {
        const OWNERS: &[Kind] = &[Kind::Meetings, Kind::Notes, Kind::Chats];
        Cut::of(self.all(OWNERS), self.any(OWNERS))
    }

    /// chat-images/: every screenshot is Chat's (Ask threads carry none).
    pub fn chat_images(&self) -> Cut {
        Cut::of(self.has(Kind::Chats), false)
    }

    /// memory.json: what's remembered, or what the passes read of chats and
    /// meetings that went.
    pub fn memory(&self) -> Cut {
        Cut::of(self.has(Kind::Memory), self.any(&[Kind::Meetings, Kind::Notes, Kind::Chats]))
    }

    /// search-index/: all of it, or the vectors of what went.
    pub fn search_index(&self) -> Cut {
        Cut::of(self.has(Kind::SearchIndex), self.any(&[Kind::Meetings, Kind::Notes, Kind::Chats, Kind::Memory]))
    }

    /// calendar.json's pre-meeting briefs: they quote notes and memory.
    pub fn briefs(&self) -> Cut {
        Cut::of(false, self.any(&[Kind::Meetings, Kind::Notes, Kind::Memory]))
    }

    pub fn word_suggestions(&self) -> Cut {
        Cut::of(self.has(Kind::Suggestions), false)
    }

    /// snippet_suggestions.json's noted phrases (its switch stays).
    pub fn snippet_suggestions(&self) -> Cut {
        Cut::of(false, self.has(Kind::Suggestions))
    }

    /// ai_usage.json and groq_usage.json.
    pub fn ai_usage(&self) -> Cut {
        Cut::of(self.has(Kind::AiUsage), false)
    }

    pub fn receipts(&self) -> Cut {
        Cut::of(self.has(Kind::Receipts), false)
    }

    pub fn logs(&self) -> Cut {
        Cut::of(self.has(Kind::Logs), false)
    }

    /// The files this choice changes: `notes.json` when it goes whole,
    /// `(notes.json)` for part of it. Pure.
    pub fn touches(&self) -> BTreeSet<String> {
        STORES
            .iter()
            .filter_map(|(file, cut)| match cut(self) {
                Cut::Keep => None,
                Cut::Part => Some(format!("({file})")),
                Cut::Whole => Some(file.to_string()),
            })
            .collect()
    }

    /// The stores whose set-aside copies go with them: those going whole.
    fn set_aside_stems(&self) -> Vec<&'static str> {
        SET_ASIDE.iter().filter(|(_, cut)| cut(self) == Cut::Whole).map(|(stem, _)| *stem).collect()
    }
}

/// Each file a deletion can change, and how a choice changes it.
type Rule = fn(&Choice) -> Cut;

const STORES: &[(&str, Rule)] = &[
    ("history.json", Choice::history),
    ("recent-audio/", Choice::dictation_audio),
    ("dictation_stats.json", Choice::stats),
    ("notes.json", Choice::notes),
    ("note-versions/", Choice::notes),
    ("note_pins.json", Choice::notes),
    ("meeting-audio/", Choice::meeting_audio),
    ("quick_note_tabs.json", Choice::quick_note_tabs),
    ("tasks.json", Choice::tasks),
    ("chats.json", Choice::chats),
    ("chat-images/", Choice::chat_images),
    ("memory.json", Choice::memory),
    ("search-index/", Choice::search_index),
    ("calendar.json", Choice::briefs),
    ("word_suggestions.json", Choice::word_suggestions),
    ("snippet_suggestions.json", Choice::snippet_suggestions),
    ("ai_usage.json", Choice::ai_usage),
    ("groq_usage.json", Choice::ai_usage),
    ("connections.json", Choice::receipts),
    ("logs/", Choice::logs),
];

/// The stores whose set-aside copies (`<stem>.corrupt-…`, `config::set_aside`;
/// `<stem>.unsaved-…`, what a store kept beside a file it couldn't read at
/// quit, `saving::flush`) go when the store goes whole: they hold the same
/// content. (One going in part keeps them: they may hold what stays.)
const SET_ASIDE: &[(&str, Rule)] = &[
    ("history", Choice::history),
    ("notes", Choice::notes),
    ("note_pins", Choice::notes),
    ("chats", Choice::chats),
    ("memory", Choice::memory),
    ("tasks", Choice::tasks),
    ("dictation_stats", Choice::stats),
    ("ai_usage", Choice::ai_usage),
    ("groq_usage", Choice::ai_usage),
    ("word_suggestions", Choice::word_suggestions),
    ("connections", Choice::receipts),
];

/// Whether `file` (a name in the data folder) is a set-aside copy of one of
/// the stores `stems`. Pure.
pub fn set_aside_copy(file: &str, stems: &[&str]) -> bool {
    stems.iter().any(|stem| {
        file.strip_prefix(stem).and_then(|r| r.strip_prefix(".corrupt").or_else(|| r.strip_prefix(".unsaved-"))).is_some()
    })
}

// ---- the dialog ---------------------------------------------------------------------------

/// The dialogs' lists: every kind with how much there is, what stays, and
/// whether a deletion can run now.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub goes: Vec<Line>,
    pub stays: Vec<&'static str>,
    /// Why nothing can be deleted now ("Finish the dictation first"), if so.
    pub blocked: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    pub kind: Kind,
    pub group: &'static str,
    pub label: &'static str,
    pub says: &'static str,
    /// How many there are now ("214 dictations"), when it's counted.
    pub count: Option<String>,
    /// How much of the disk it takes (its files, its audio and pictures).
    pub bytes: u64,
    /// Nothing kept of it now.
    pub empty: bool,
    /// Why this kind can't be deleted just now (a file Yap couldn't read).
    pub unread: Option<String>,
    /// Its files in the data folder (the dialog's small print).
    pub files: &'static [&'static str],
}

/// "214 dictations", "1 chat", "1,204 connections"; nothing for none. Pure.
pub fn count(n: usize, one: &str, many: &str) -> Option<String> {
    match n {
        0 => None,
        1 => Some(format!("1 {one}")),
        n => Some(format!("{} {many}", thousands(n))),
    }
}

/// "1,204". Pure.
fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// "3 chats, 4 screenshots" from the parts there are. Pure.
fn counts(parts: [Option<String>; 2]) -> Option<String> {
    let parts: Vec<String> = parts.into_iter().flatten().collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// Why a deletion can't run now, if it can't: a dictation or a meeting
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

/// A store kind `kind` changes whose file Yap couldn't read: what's in it
/// isn't in memory, so it can't be cleared (and would come back once read).
fn unread(kind: Kind) -> Option<String> {
    crate::shell::saving::unread_among(about(kind).stores)
        .map(|whole| format!("Yap couldn't read {whole} just now (another program may be using the file). Try again in a moment."))
}

/// Why choice `c` can't be deleted now, if it can't.
fn refused(app: &AppHandle, c: &Choice) -> Option<String> {
    busy(app).or_else(|| c.kinds().into_iter().find_map(unread))
}

fn file_bytes(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| if m.is_file() { m.len() } else { 0 }).unwrap_or(0)
}

/// A folder's files, all the way down: how many and their bytes.
fn dir_bytes(path: &Path) -> (usize, u64) {
    let mut out = (0, 0);
    for e in std::fs::read_dir(path).into_iter().flatten().flatten() {
        let Ok(kind) = e.file_type() else { continue };
        if kind.is_dir() {
            let (n, b) = dir_bytes(&e.path());
            out = (out.0 + n, out.1 + b);
        } else if kind.is_file() {
            out = (out.0 + 1, out.1 + e.metadata().map(|m| m.len()).unwrap_or(0));
        }
    }
    out
}

/// Each kind with how much there is of it now (counts and sizes only).
fn measure() -> Vec<Line> {
    let data = crate::config::data_dir();
    let notes = crate::notes::all();
    let (meetings, others): (Vec<_>, Vec<_>) = notes.iter().partition(|n| is_meeting(n));
    let versions = |list: &[&crate::notes::Note]| -> u64 {
        list.iter().filter_map(|n| crate::notes::versions::path(n.id)).map(|p| file_bytes(&p)).sum()
    };
    KINDS
        .iter()
        .map(|a| {
            let (count, bytes, empty) = match a.kind {
                Kind::History => {
                    let n = crate::dictation::history::all().len();
                    let (audio, audio_bytes) = dir_bytes(&crate::dictation::recent_audio::dir());
                    (count(n, "dictation", "dictations"), file_bytes(&data.join("history.json")) + audio_bytes, n == 0 && audio == 0)
                }
                Kind::DictationAudio => {
                    let s = crate::dictation::recent_audio::recent_audio_status();
                    (count(s.count, "recording", "recordings"), s.bytes, s.count == 0)
                }
                Kind::Stats => {
                    let days = crate::dictation::stats::snapshot()["days"].as_array().map_or(0, |d| d.len());
                    (count(days, "day", "days"), file_bytes(&data.join("dictation_stats.json")), days == 0)
                }
                Kind::Meetings => {
                    let (audio, audio_bytes) = dir_bytes(&crate::meetings::meeting_audio::root());
                    (count(meetings.len(), "meeting", "meetings"), audio_bytes + versions(&meetings), meetings.is_empty() && audio == 0)
                }
                Kind::Notes => (count(others.len(), "note", "notes"), versions(&others), others.is_empty()),
                Kind::Tasks => {
                    let n = crate::notes::task_store::snapshot().tasks.iter().filter(|t| t.note_id.is_none()).count();
                    (count(n, "task", "tasks"), 0, n == 0)
                }
                Kind::Chats => {
                    let n = crate::chat::chats::with_all(|c| c.iter().filter(|c| c.note.is_none()).count());
                    let (shots, shot_bytes) = crate::snip::images::usage();
                    (counts([count(n, "chat", "chats"), count(shots, "screenshot", "screenshots")]), shot_bytes, n == 0 && shots == 0)
                }
                Kind::Memory => {
                    let n = crate::memory::items().len();
                    (count(n, "thing", "things"), file_bytes(&data.join("memory.json")), n == 0)
                }
                Kind::Suggestions => {
                    let n = crate::dictionary::learn::offered();
                    let bytes = file_bytes(&data.join("word_suggestions.json"));
                    (count(n, "word offered", "words offered"), bytes, n == 0 && bytes == 0)
                }
                Kind::AiUsage => {
                    let bytes = file_bytes(&data.join("ai_usage.json")) + file_bytes(&data.join("groq_usage.json"));
                    (None, bytes, bytes == 0)
                }
                Kind::Receipts => {
                    let n: u64 = crate::privacy::net::entries().iter().map(|e| u64::from(e.count)).sum();
                    (count(n as usize, "connection", "connections"), file_bytes(&data.join("connections.json")), n == 0)
                }
                Kind::SearchIndex => {
                    let (_, bytes) = dir_bytes(&crate::search::index::dir());
                    (None, bytes, bytes == 0)
                }
                Kind::Logs => {
                    let (_, bytes) = dir_bytes(&data.join("logs"));
                    (None, bytes, bytes == 0)
                }
            };
            Line { kind: a.kind, group: a.group, label: a.label, says: a.says, count, bytes, empty, unread: unread(a.kind), files: a.files }
        })
        .collect()
}

/// Settings' dialogs (Reset app, Delete some of your data): every kind with
/// how much there is of it, what stays, and whether a deletion can run now.
#[tauri::command]
pub async fn data_reset_plan(app: AppHandle) -> Plan {
    let blocked = busy(&app);
    tauri::async_runtime::spawn_blocking(move || Plan { goes: measure(), stays: STAYS.iter().map(|p| p.label).collect(), blocked })
        .await
        .unwrap_or(Plan { goes: Vec::new(), stays: Vec::new(), blocked: Some("Yap couldn't look at its data.".into()) })
}

// ---- deleting -----------------------------------------------------------------------------

/// What a deletion did (counts only).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Done {
    /// The kinds deleted.
    pub kinds: Vec<Kind>,
    pub dictations: usize,
    /// Every note that went, meetings too.
    pub notes: usize,
    pub meetings: usize,
    /// Every conversation that went, Ask threads too.
    pub chats: usize,
    pub memory: usize,
    pub tasks: usize,
    pub screenshots: usize,
    pub meeting_audio: usize,
    pub briefs: usize,
    pub logs: usize,
    pub set_aside: usize,
    /// Whether Yap needs a restart to finish (never so far: kept for the page).
    pub restart: bool,
}

/// What went from the stores that keep their data in memory.
#[derive(Debug, Default)]
struct Gone {
    notes: Vec<u64>,
    chats: Vec<u64>,
}

/// The notes, their audio, tasks, chats and memory: the stores whose data
/// lives in memory (unit tests run this on in-memory stores of their own,
/// meeting audio in a folder of their own). Each through its own module.
fn clear_stores(c: &Choice, done: &mut Done) -> Gone {
    let mut gone = Gone::default();
    // Notes and meetings; what only the notes store keeps of them with them.
    match c.notes() {
        Cut::Whole => {
            done.meetings = crate::notes::with_all(|n| n.iter().filter(|n| is_meeting(n)).count());
            gone.notes = crate::notes::reset();
            // Their version history and pins (notes/versions.rs, notes/pins.rs),
            // their summary jobs.
            crate::notes::versions::reset();
            crate::notes::pins::reset();
            for id in &gone.notes {
                crate::meetings::meeting_end::forget(*id);
            }
        }
        Cut::Part => {
            let meetings = c.has(Kind::Meetings);
            gone.notes = crate::notes::remove_where(|n| is_meeting(n) == meetings);
            if meetings {
                done.meetings = gone.notes.len();
            }
            crate::notes::forget_own(&gone.notes);
        }
        Cut::Keep => {}
    }
    done.notes = gone.notes.len();
    // Meeting audio: every recording with the meetings (a recording going
    // on can't be: a deletion waits for it), else an other note's own.
    if c.meeting_audio() == Cut::Whole {
        done.meeting_audio = crate::meetings::meeting_audio::clear();
    }
    for id in &gone.notes {
        crate::meetings::meeting_audio::forget(*id);
    }
    if c.meeting_audio() == Cut::Whole && !crate::meetings::meeting::is_recording() {
        let dir = crate::meetings::meeting_audio::root();
        if dir.exists() {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
    // Notes there now: what names one that isn't is left over (an older
    // Yap deleted it) and goes with the kind it belonged to.
    let left: HashSet<u64> = if c.notes() == Cut::Keep { HashSet::new() } else { crate::notes::with_all(|n| n.iter().map(|n| n.id).collect()) };
    let went = |note: u64| c.notes() != Cut::Keep && (gone.notes.contains(&note) || !left.contains(&note));
    // Tasks: a note's go with it, those added on their own with Tasks.
    done.tasks = match c.tasks() {
        Cut::Whole => crate::notes::task_store::reset(),
        Cut::Part => crate::notes::task_store::forget_where(|note| match note {
            None => c.has(Kind::Tasks),
            Some(id) => went(id),
        }),
        Cut::Keep => 0,
    };
    // Conversations: Chat's own with Chats, a note's Ask threads with it.
    gone.chats = match c.chats() {
        Cut::Whole => {
            let ids = crate::chat::chats::with_all(|c| c.iter().map(|c| c.id).collect());
            crate::chat::chats::reset();
            ids
        }
        Cut::Part => crate::chat::chats::delete_where(|conv| match conv.note {
            None => c.has(Kind::Chats),
            Some(note) => went(note),
        }),
        Cut::Keep => Vec::new(),
    };
    done.chats = gone.chats.len();
    for id in &gone.chats {
        crate::apps::moved_on(Some(*id));
    }
    // Memory: all of it (Undo's copies too), or what it read of what went.
    match c.memory() {
        Cut::Whole => done.memory = crate::memory::reset(),
        Cut::Part => {
            crate::memory::forget_sources(&gone.chats, &gone.notes);
        }
        Cut::Keep => {}
    }
    // The vectors of what went (the index itself goes in `clear_files`).
    if c.search_index() == Cut::Part {
        crate::search::removed();
    }
    gone
}

/// Everything else, store by store, then the set-aside copies. The search
/// index's error (its folder held) is returned once the rest is done.
fn clear_files(app: &AppHandle, c: &Choice, done: &mut Done) -> Result<(), String> {
    // History, and the day of dictation audio with it.
    if c.history() == Cut::Whole {
        done.dictations = crate::dictation::history::all().len();
        crate::dictation::history::clear();
    } else if c.dictation_audio() == Cut::Whole {
        crate::dictation::recent_audio::clear();
    }
    if c.stats() == Cut::Whole {
        crate::dictation::stats::clear();
    }
    // The quick note's open tabs were other notes.
    if c.quick_note_tabs() == Cut::Whole {
        crate::notes::quick_note::tabs::reset();
    }
    // Every screenshot (the ones waiting in Chat's box too).
    if c.chat_images() == Cut::Whole {
        done.screenshots = crate::snip::chat_images_delete_all(app.clone());
    }
    // The index's folder held (an antivirus, the indexer at work): said
    // once the rest is done.
    let index = if c.search_index() == Cut::Whole {
        let r = crate::search::index::forget();
        crate::search::index::emit(app);
        r
    } else {
        Ok(())
    };
    if c.ai_usage() == Cut::Whole {
        crate::ai::usage::store::reset();
        crate::ai::usage::groq::reset();
    }
    if c.receipts() == Cut::Whole {
        crate::privacy::net::clear();
    }
    if c.word_suggestions() == Cut::Whole {
        crate::dictionary::learn::forget_all();
    }
    if c.snippet_suggestions() != Cut::Keep {
        crate::dictionary::snippet_offer::forget_noted();
    }
    if c.briefs() != Cut::Keep {
        done.briefs = crate::calendar::forget_briefs();
    }
    // Set-aside copies of the stores that went whole.
    let stems = c.set_aside_stems();
    if !stems.is_empty() {
        let data = crate::config::data_dir();
        for e in std::fs::read_dir(&data).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if set_aside_copy(&name, &stems) && e.path().is_file() && std::fs::remove_file(e.path()).is_ok() {
                done.set_aside += 1;
            }
        }
    }
    // The logs last: what's written after starts the emptied file.
    if c.logs() == Cut::Whole {
        done.logs = crate::shell::logging::clear();
    }
    index
}

/// Every window: each note let go of, the stores' own events, then the
/// deletion's.
fn tell(app: &AppHandle, c: &Choice, gone: &Gone, done: &Done) {
    for id in &gone.notes {
        let _ = app.emit(crate::meetings::meeting_end::EVENT_DELETED, serde_json::json!({ "id": id }));
        crate::meetings::notepad::on_note_deleted(app, *id);
    }
    let mut events: Vec<&str> = Vec::new();
    if c.dictation_audio() != Cut::Keep {
        events.push("yap-history-changed");
    }
    if c.stats() != Cut::Keep {
        events.push(crate::dictation::stats::EVENT);
    }
    if c.notes() != Cut::Keep {
        events.push("yap-notes-changed");
    }
    if c.tasks() != Cut::Keep {
        events.push("yap-tasks-changed");
    }
    // `memory::reset` says so itself.
    if c.memory() == Cut::Part {
        events.push(crate::memory::EVENT_CHANGED);
    }
    for event in events {
        let _ = app.emit(event, ());
    }
    if c.meeting_audio() != Cut::Keep {
        let _ = app.emit(crate::meetings::meeting_audio::EVENT_AUDIO, serde_json::json!({ "noteId": null }));
    }
    let _ = app.emit(EVENT, done);
}

/// Every kind in `c`, emptied in memory and on disk, store by store.
fn run(app: &AppHandle, c: &Choice) -> Result<Done, String> {
    let mut done = Done { kinds: c.kinds(), ..Done::default() };
    let gone = clear_stores(c, &mut done);
    let index = clear_files(app, c, &mut done);
    tell(app, c, &gone, &done);
    // A save that failed is said (the store keeps it empty in memory and
    // tries again; saving.rs).
    if c.history() != Cut::Keep {
        crate::dictation::history::saved()?;
    }
    if c.notes() != Cut::Keep {
        crate::notes::saved()?;
        crate::notes::pins::SAVER.check()?;
    }
    if c.chats() != Cut::Keep {
        crate::chat::chats::saved()?;
    }
    if c.memory() != Cut::Keep {
        crate::memory::saved()?;
    }
    if c.tasks() != Cut::Keep {
        crate::notes::task_store::saved()?;
    }
    if c.word_suggestions() != Cut::Keep {
        crate::dictionary::learn::SAVER.check()?;
    }
    if let Err(e) = index {
        tracing::warn!("Deleting your data: the search index's folder wasn't deleted ({e})");
        return Err(e);
    }
    Ok(done)
}

async fn delete(app: AppHandle, c: Choice) -> Result<Done, String> {
    if c.is_empty() {
        return Err("Pick what to delete first.".into());
    }
    if let Some(why) = refused(&app, &c) {
        return Err(why);
    }
    let handle = app.clone();
    let every = c == Choice::every();
    // The data folder's files it changes (names only).
    let files = c.touches().into_iter().collect::<Vec<_>>().join(" ");
    let done = tauri::async_runtime::spawn_blocking(move || run(&handle, &c)).await.map_err(|e| e.to_string())??;
    tracing::info!(
        kinds = done.kinds.len(),
        every,
        files = %files,
        dictations = done.dictations,
        notes = done.notes,
        meetings = done.meetings,
        chats = done.chats,
        memory = done.memory,
        tasks = done.tasks,
        screenshots = done.screenshots,
        meeting_audio = done.meeting_audio,
        briefs = done.briefs,
        logs = done.logs,
        set_aside = done.set_aside,
        "Deleting your data: done"
    );
    Ok(done)
}

/// Delete some of your data: the kinds ticked, each as Reset app clears it.
/// Refused while a dictation or a meeting records.
#[tauri::command]
pub async fn data_delete(app: AppHandle, kinds: Vec<Kind>) -> Result<Done, String> {
    delete(app, Choice::of(kinds)).await
}

/// Reset app: every kind (see [`KINDS`] / [`STAYS`]). Refused while a
/// dictation or a meeting records.
#[tauri::command]
pub async fn data_reset(app: AppHandle) -> Result<Done, String> {
    delete(app, Choice::every()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every file and folder config-and-data.md names in the data folder.
    fn named_in_the_docs() -> BTreeSet<String> {
        let doc = include_str!("../../../docs/architecture/config-and-data.md");
        let re = regex::Regex::new(r"(?:%APPDATA%[/\\]yap[/\\]|<data>/)([A-Za-z0-9_.-]+)(/?)").unwrap();
        let named: BTreeSet<String> =
            re.captures_iter(doc).map(|c| format!("{}{}", &c[1], if c[2].is_empty() && !c[1].contains('.') { "/" } else { &c[2] })).collect();
        assert!(named.len() >= 20, "{named:?}");
        named
    }

    fn stays(file: &str) -> bool {
        STAYS.iter().flat_map(|p| p.files).any(|f| *f == file)
    }

    /// Each kind on its own changes exactly the files it names: no more
    /// (nothing of another kind's is touched), no fewer.
    #[test]
    fn each_kind_changes_exactly_the_files_it_names() {
        for a in KINDS {
            let named: BTreeSet<String> = a.files.iter().map(|f| f.to_string()).collect();
            assert_eq!(Choice::of([a.kind]).touches(), named, "{:?}", a.kind);
        }
    }

    /// The per-file table: every file config-and-data.md names is changed by
    /// exactly the kinds that own it (whole or in part), and one no kind
    /// deletes whole stays. A store added there has to be decided here
    /// (and the dialog's words with it).
    #[test]
    fn every_data_file_the_docs_name_is_changed_by_exactly_the_kinds_that_own_it() {
        let named = named_in_the_docs();
        for f in &named {
            let part = format!("({f})");
            let whole: Vec<Kind> = KINDS.iter().filter(|a| a.files.contains(&f.as_str())).map(|a| a.kind).collect();
            let owners: Vec<Kind> = KINDS.iter().filter(|a| a.files.contains(&f.as_str()) || a.files.contains(&part.as_str())).map(|a| a.kind).collect();
            // Kept, or its kinds' between them: one or the other. A file
            // that stays loses at most a part a kind names; one that doesn't
            // goes whole with every kind that owns it (Reset app).
            if stays(f) {
                assert!(whole.is_empty(), "{f} stays, but {whole:?} delete it whole");
            } else {
                assert!(!owners.is_empty(), "{f}: decide which kinds delete it (privacy/reset.rs KINDS) or that it stays (STAYS)");
                assert!(Choice::of(owners.iter().copied()).touches().contains(f), "{f} goes whole with {owners:?} together");
            }
            for a in KINDS {
                let t = Choice::of([a.kind]).touches();
                let changes = t.contains(f) || t.contains(&part);
                assert_eq!(changes, owners.contains(&a.kind), "{f}: {:?} {}", a.kind, if changes { "changes it but doesn't name it" } else { "names it but doesn't change it" });
            }
        }
        // Every file a kind names is a real one (the docs name it).
        for a in KINDS {
            for f in a.files {
                assert!(named.contains(f.trim_start_matches('(').trim_end_matches(')')), "{:?} names {f}: say what it is in config-and-data.md", a.kind);
            }
        }
        // And every file a deletion can change is some kind's.
        for (f, _) in STORES {
            assert!(KINDS.iter().any(|a| a.files.contains(f) || a.files.contains(&format!("({f})").as_str())), "{f}");
        }
    }

    /// Reset app is every kind: each store goes whole (calendar.json and
    /// snippet_suggestions.json lose only their part: the calendars and the
    /// switch stay), and nothing that stays is deleted.
    #[test]
    fn reset_app_is_every_kind_and_every_store_goes_whole() {
        let every = Choice::every();
        for (f, cut) in STORES {
            let want = if matches!(*f, "calendar.json" | "snippet_suggestions.json") { Cut::Part } else { Cut::Whole };
            assert_eq!(cut(&every), want, "{f}");
        }
        for f in every.touches() {
            assert!(!stays(&f), "{f} stays: Reset app never deletes it");
        }
        for f in ["config.json", "dictionary_usage.json", "skills.json", "writing_samples.json", "models/", "llm/", "link_devices.json"] {
            assert!(stays(f), "{f} stays");
        }
        assert_eq!(every.set_aside_stems().len(), SET_ASIDE.len());
        // Each line says what it is in plain words.
        assert!(KINDS.iter().all(|a| !a.label.is_empty() && !a.says.is_empty() && !a.says.ends_with('.')));
        assert_eq!(KINDS.len(), BTreeSet::from_iter(KINDS.iter().map(|a| a.kind)).len());
    }

    /// A store shared by kinds goes whole only when they all go; else each
    /// keeps the others' part.
    #[test]
    fn a_shared_store_goes_whole_only_with_every_kind_that_owns_it() {
        use Kind::*;
        assert_eq!(Choice::of([Meetings]).notes(), Cut::Part);
        assert_eq!(Choice::of([Meetings, Notes]).notes(), Cut::Whole);
        assert_eq!(Choice::of([Meetings, Notes]).tasks(), Cut::Part);
        assert_eq!(Choice::of([Meetings, Notes, Tasks]).tasks(), Cut::Whole);
        assert_eq!(Choice::of([Meetings, Notes]).chats(), Cut::Part);
        assert_eq!(Choice::of([Meetings, Notes, Chats]).chats(), Cut::Whole);
        assert_eq!(Choice::of([Chats]).chats(), Cut::Part);
        assert_eq!(Choice::of([Chats]).memory(), Cut::Part);
        assert_eq!(Choice::of([History]).dictation_audio(), Cut::Whole);
        assert_eq!(Choice::of([DictationAudio]).history(), Cut::Keep);
        // Set-aside copies go only with a whole store.
        assert!(Choice::of([Meetings]).set_aside_stems().iter().all(|s| !["notes", "note_pins", "tasks", "chats"].contains(s)));
        assert!(Choice::of([Meetings, Notes]).set_aside_stems().contains(&"notes"));
        assert_eq!(Choice::of([History]).set_aside_stems(), ["history"]);
        assert!(Choice::of([Logs, SearchIndex, DictationAudio]).set_aside_stems().is_empty());
    }

    #[test]
    fn set_aside_copies_of_what_goes_go_too() {
        let every = Choice::every().set_aside_stems();
        assert!(set_aside_copy("notes.corrupt-1790000000", &every));
        assert!(set_aside_copy("history.corrupt-1790000000-2", &every));
        assert!(set_aside_copy("ai_usage.corrupt", &every));
        assert!(set_aside_copy("chats.unsaved-1790000000", &every));
        assert!(!set_aside_copy("config.corrupt-1790000000", &every), "the settings stay, set-aside ones too");
        assert!(!set_aside_copy("notes.json", &every));
        assert!(!set_aside_copy("notes_extra.corrupt-1", &every));
        assert!(!set_aside_copy("dictionary_usage.corrupt-1", &every));
        assert!(!set_aside_copy("notes.corrupt-1", &Choice::of([Kind::Meetings]).set_aside_stems()));
    }

    #[test]
    fn a_deletion_waits_for_a_dictation_or_a_meeting() {
        assert_eq!(blocked(false, false), None);
        assert!(blocked(true, false).unwrap().contains("dictation"));
        assert!(blocked(true, true).unwrap().contains("meeting"));
        assert_eq!(count(0, "chat", "chats"), None);
        assert_eq!(count(1, "chat", "chats").as_deref(), Some("1 chat"));
        assert_eq!(count(3, "chat", "chats").as_deref(), Some("3 chats"));
        assert_eq!(count(1_204, "connection", "connections").as_deref(), Some("1,204 connections"));
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(100), "100");
        assert_eq!(counts([count(2, "chat", "chats"), count(0, "screenshot", "screenshots")]).as_deref(), Some("2 chats"));
        assert_eq!(counts([None, None]), None);
    }

    /// The in-memory stores, seeded fresh: a note, a meeting, a chat and each
    /// note's Ask thread, a task of each and one on its own, a pin, what
    /// memory read and one thing it remembers. (Unit tests run on stores of
    /// their own, in memory.)
    struct Seeded {
        note: u64,
        meeting: u64,
        chat: u64,
        meeting_ask: u64,
        note_ask: u64,
    }

    fn seed() -> Seeded {
        clear_stores(&Choice::every(), &mut Done::default());
        let note = crate::notes::create("Allotment plans", "Leeks by the shed", "manual", "").id;
        let meeting = crate::notes::create("Weekly sync", "", "manual", "Meetings").id;
        crate::notes::mark_meeting(meeting).unwrap();
        let chat = crate::chat::chats::create("Leek soil").id;
        let meeting_ask = crate::chat::chats::create_for_note(meeting, "What was decided?").id;
        let note_ask = crate::chat::chats::create_for_note(note, "When do leeks go in?").id;
        let _ = crate::notes::task_store::modify(|s| {
            for (id, text, note_id) in [(1, "Ship the leek tracker", Some(meeting)), (2, "Buy seed", Some(note)), (3, "Water the leeks", None)] {
                s.tasks.push(serde_json::from_value(serde_json::json!({ "id": id, "text": text, "noteId": note_id, "createdMs": 1 })).unwrap());
            }
            s.archived.push(crate::notes::task_store::Archived { note_id: meeting, text: "Book the room".into() });
            Ok(((), true))
        });
        crate::notes::pins::set(note, crate::notes::get(note).unwrap().created_ts, true);
        crate::memory::mark_read(chat, 2);
        crate::memory::mark_read(note_ask, 1);
        crate::memory::meeting_read(meeting, true);
        crate::memory::save_item(serde_json::from_value(serde_json::json!({ "section": "people", "title": "Robin", "summary": "Neighbour" })).unwrap()).unwrap();
        Seeded { note, meeting, chat, meeting_ask, note_ask }
    }

    fn notes_left() -> Vec<u64> {
        crate::notes::all().iter().map(|n| n.id).collect()
    }
    fn chats_left() -> Vec<u64> {
        crate::chat::chats::with_all(|c| c.iter().map(|c| c.id).collect())
    }
    fn tasks_left() -> Vec<u64> {
        crate::notes::task_store::snapshot().tasks.iter().map(|t| t.id).collect()
    }
    fn delete(kinds: &[Kind]) -> Done {
        let mut done = Done::default();
        clear_stores(&Choice::of(kinds.iter().copied()), &mut done);
        done
    }

    #[test]
    fn chats_alone_leave_notes_meetings_and_their_ask_threads() {
        let s = seed();
        let done = delete(&[Kind::Chats]);
        assert_eq!(done.chats, 1);
        assert_eq!(notes_left(), [s.note, s.meeting]);
        assert_eq!(chats_left(), [s.meeting_ask, s.note_ask]);
        assert_eq!(tasks_left(), [1, 2, 3]);
        // Memory forgets what it read of the chat, and remembers the rest.
        assert_eq!(crate::memory::read_upto(s.chat), 0);
        assert_eq!(crate::memory::read_upto(s.note_ask), 1);
        assert!(crate::memory::meeting_read(s.meeting, false));
        assert_eq!(crate::memory::items().len(), 1);
        assert_eq!(crate::notes::pins::pins().len(), 1);
    }

    #[test]
    fn meetings_alone_take_their_tasks_and_ask_threads_and_leave_the_notes() {
        let s = seed();
        let done = delete(&[Kind::Meetings]);
        assert_eq!((done.notes, done.meetings, done.chats, done.tasks), (1, 1, 1, 1));
        assert_eq!(notes_left(), [s.note]);
        assert_eq!(chats_left(), [s.chat, s.note_ask]);
        assert_eq!(tasks_left(), [2, 3]);
        assert!(crate::notes::task_store::snapshot().archived.is_empty(), "the meeting's archive marks go with it");
        assert!(!crate::memory::meeting_read(s.meeting, false));
        assert_eq!(crate::memory::read_upto(s.chat), 2);
        assert_eq!(crate::notes::pins::pins().len(), 1, "the note's pin stays");
        // The folders and actions stay.
        assert!(!crate::notes::folders().is_empty());
        assert!(!crate::notes::actions().is_empty());
    }

    #[test]
    fn other_notes_alone_leave_the_meetings() {
        let s = seed();
        let done = delete(&[Kind::Notes]);
        assert_eq!((done.notes, done.meetings), (1, 0));
        assert_eq!(notes_left(), [s.meeting]);
        assert_eq!(chats_left(), [s.chat, s.meeting_ask]);
        assert_eq!(tasks_left(), [1, 3]);
        assert_eq!(crate::notes::task_store::snapshot().archived.len(), 1);
        assert_eq!(crate::memory::read_upto(s.note_ask), 0);
        assert!(crate::notes::pins::pins().is_empty());
    }

    #[test]
    fn tasks_and_memory_alone_touch_nothing_else() {
        let s = seed();
        assert_eq!(delete(&[Kind::Tasks]).tasks, 1);
        assert_eq!(tasks_left(), [1, 2]);
        assert_eq!(notes_left().len(), 2);
        assert_eq!(chats_left().len(), 3);
        assert_eq!(delete(&[Kind::Memory]).memory, 1);
        assert!(crate::memory::items().is_empty());
        assert_eq!(crate::memory::read_upto(s.chat), 0, "what the passes read goes with it");
        assert_eq!((notes_left().len(), chats_left().len(), tasks_left().len()), (2, 3, 2));
        // Kinds kept in files of their own leave every in-memory store alone.
        let _ = delete(&[Kind::History, Kind::DictationAudio, Kind::Stats, Kind::Suggestions, Kind::AiUsage, Kind::Receipts, Kind::SearchIndex, Kind::Logs]);
        assert_eq!((notes_left().len(), chats_left().len(), tasks_left().len()), (2, 3, 2));
    }

    /// The stores empty in memory too, every kind at once: what a later save
    /// writes is nothing.
    #[test]
    fn the_stores_are_empty_in_memory_after_a_reset() {
        let _ = seed();
        let done = delete(&KINDS.iter().map(|a| a.kind).collect::<Vec<_>>());
        assert_eq!((done.notes, done.meetings, done.chats, done.tasks, done.memory), (2, 1, 3, 3, 1));
        assert!(crate::notes::all().is_empty());
        assert!(chats_left().is_empty());
        assert!(crate::notes::task_store::snapshot() == crate::notes::task_store::TaskStore::default());
        assert!(crate::memory::items().is_empty());
        assert!(crate::notes::pins::pins().is_empty());
        // The folders and actions stay.
        assert!(!crate::notes::folders().is_empty());
        assert!(!crate::notes::actions().is_empty());
    }
}

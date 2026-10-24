//! **Memory** (Settings → Memory, 2026-10-22): what Yap remembers about the
//! person, kept on this PC, after Claude's memory — the maintainer's ask.
//!
//! - **The store** (`<data>/memory.json`, atomic writes, a corrupt file
//!   quarantined, never config.json): items in five sections — Profile and
//!   Preferences ("You", one item each), People, Projects, Topics — each a
//!   title, a one-line summary, short facts, where it came from and when
//!   ([`ops::Item`]). Small and bounded; changes go in as changes, never a
//!   rewrite (`memory/ops.rs`), and every batch can be undone.
//! - **Learning** (`memory/learn.rs`): a chat gone quiet (5 minutes, another
//!   chat started, or at the next start after Yap quit) is read by the
//!   person's Chat model for lasting facts ("Learn from my chats", on); a
//!   finished meeting's attendees and action plan too ("Learn from meetings",
//!   off). Never passing details, other people's private details, secrets
//!   (`memory/guard.rs`), nor — unless the person allows it — sensitive
//!   topics. A model on this PC never runs a pass during a dictation.
//!   With the main window on screen, a quiet toast says what Yap remembered
//!   (`yap-memory-learned`: Undo, Manage); never a Windows notification.
//! - **Use** (`memory/prompt.rs`): the "You" part goes with every Chat,
//!   Voice Assistant and notepad helper prompt (stable, so a model's cache
//!   keeps it); people, projects and topics only with a question about them
//!   (`commands::chat_turn`'s per-turn context). Tool-using models also get
//!   `remember` / `forget` and, with "Search and reference past chats",
//!   `search_chats` (`tools.rs`).
//! - **Dictation** (`memory/vocab.rs`): the names of people and projects
//!   become dictation vocabulary — the Whisper prompt, and for ONNX models a
//!   careful near-miss pass that never swallows an ordinary word.
//! - **Settings → Memory** (`MemorySection.svelte`): the switches, the
//!   sections, editing, "Tell Yap what to change", import, export (a .zip of
//!   Markdown), clear all.
//!
//! Kept here and sent only with questions to the AI model the person uses
//! (a cloud provider sees it, a model on this PC keeps it here); never sent
//! to Yap.

pub mod export;
pub mod guard;
pub mod learn;
pub mod ops;
pub mod prompt;
pub mod small;
pub mod vocab;

use std::collections::{BTreeMap, VecDeque};
#[cfg(not(test))]
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use ops::{Change, Done, Item, Origin, Rules, Section};

/// Every change to memory (Settings follows it).
pub const EVENT_CHANGED: &str = "yap-memory-changed";
/// Yap remembered something by itself (the main window's toast).
pub const EVENT_LEARNED: &str = "yap-memory-learned";

pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Store {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    items: Vec<Item>,
    #[serde(default)]
    next_id: u64,
    /// Conversation id → how many of its messages a pass has read.
    #[serde(default)]
    read: BTreeMap<u64, usize>,
    /// Conversations waiting for a pass (Yap quit before it ran).
    #[serde(default)]
    pending: Vec<u64>,
    /// Meeting notes a pass has read.
    #[serde(default)]
    meetings: Vec<u64>,
}

// ---- the store ---------------------------------------------------------------------------------

#[cfg(not(test))]
static STATE: Mutex<Option<Store>> = Mutex::new(None);

#[cfg(not(test))]
fn memory_path() -> PathBuf {
    crate::config::data_dir().join("memory.json")
}

#[cfg(not(test))]
fn load_from_disk() -> Store {
    if crate::eval::active() {
        return Store::default();
    }
    let path = memory_path();
    match std::fs::read_to_string(&path) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(store) => store,
            Err(e) => {
                tracing::error!("memory.json failed to parse: {}", e);
                crate::config::quarantine_corrupt(&path);
                Store::default()
            }
        },
        Err(_) => Store::default(),
    }
}

/// memory.json's saves (`saving.rs`): a failed one keeps what Yap
/// remembers in memory, is tried again with the next save, and is said once.
pub static SAVER: crate::saving::Saver = crate::saving::Saver::new("memory", "what Yap remembers about you", save_now);

/// `Ok` when all of it is on disk; else why not (the caller says it).
pub fn saved() -> Result<(), crate::saving::SaveError> {
    SAVER.check()
}

/// Write the whole store again (Try again).
fn save_now() -> Result<(), crate::saving::SaveError> {
    with_store(|s| save_to_disk(s));
    saved()
}

#[cfg(not(test))]
fn save_to_disk(store: &Store) {
    // A model evaluation keeps its memory in memory only.
    if crate::eval::active() {
        return;
    }
    let _ = SAVER.write_json(&memory_path(), store, false);
}

#[cfg(not(test))]
fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let store = guard.get_or_insert_with(load_from_disk);
    f(store)
}

// Unit tests never read or write the real memory.json: a store per test
// thread, in memory (as notes.rs and chats.rs do).
#[cfg(test)]
thread_local! {
    static TEST_STORE: std::cell::RefCell<Store> = std::cell::RefCell::new(Store::default());
}

#[cfg(test)]
fn save_to_disk(_store: &Store) {}

#[cfg(test)]
fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    TEST_STORE.with(|s| f(&mut s.borrow_mut()))
}

/// A model evaluation: each scenario starts with an empty memory (and the
/// scenario's seed), in memory only.
#[cfg(debug_assertions)]
pub fn eval_reset() {
    with_store(|s| *s = Store::default());
    clear_caches();
}

/// Everything Yap remembers, in the order kept.
pub fn items() -> Vec<Item> {
    with_store(|s| s.items.clone())
}

// ---- app handle, events ------------------------------------------------------------------------

static APP: OnceLock<AppHandle> = OnceLock::new();

/// The app, for events (none in a model evaluation or a unit test).
pub fn app() -> Option<&'static AppHandle> {
    APP.get()
}

fn emit_changed() {
    if let Some(app) = app() {
        let _ = app.emit(EVENT_CHANGED, ());
    }
}

/// Start memory with the app: events, and the learning scheduler (chats a
/// quit left waiting are read a minute after start).
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
    learn::start(app);
}

// ---- dictation vocabulary (cached) -------------------------------------------------------------

/// The names from memory (`vocab::entries`), cached until memory changes.
static VOCAB: Mutex<Option<Vec<crate::config::DictionaryEntry>>> = Mutex::new(None);

fn clear_caches() {
    if let Ok(mut v) = VOCAB.lock() {
        *v = None;
    }
}

/// "Is this lowercased word an ordinary word?": the everyday list, then
/// Windows' spell checker (opened only when a name or a heard word needs
/// it). **No spell checker = every word counts as ordinary**: Yap can't
/// tell "olive" from a name then, so memory's names are never near-miss
/// targets (only the Whisper prompt and a case fix).
pub struct Words {
    ordinary: std::cell::OnceCell<crate::dictionary::learn::Ordinary>,
}

impl Default for Words {
    fn default() -> Self {
        Words::new()
    }
}

impl Words {
    pub fn new() -> Words {
        Words { ordinary: std::cell::OnceCell::new() }
    }

    pub fn is(&self, w: &str) -> bool {
        if crate::dictionary::learn::is_frequent(w) {
            return true;
        }
        let o = self.ordinary.get_or_init(crate::dictionary::learn::Ordinary::new);
        !o.has_spell_checker() || o.is(w)
    }
}

/// The names memory adds to dictation (all of them for the Whisper prompt;
/// the safe ones also for the near-miss pass, `fuzzy` set). Excludes what
/// the dictionary already spells.
pub fn dictation_names(dictionary: &[crate::config::DictionaryEntry]) -> Vec<crate::config::DictionaryEntry> {
    let cached = VOCAB.lock().ok().and_then(|v| v.clone());
    let all = match cached {
        Some(v) => v,
        None => {
            let words = Words::new();
            let v = vocab::entries(&items(), &[], &|w| words.is(w));
            if let Ok(mut slot) = VOCAB.lock() {
                *slot = Some(v.clone());
            }
            v
        }
    };
    let taken: std::collections::HashSet<String> = dictionary
        .iter()
        .filter(|e| !e.snippet)
        .flat_map(|e| [e.to.trim().to_lowercase(), e.from.trim().to_lowercase()])
        .collect();
    all.into_iter().filter(|e| !taken.contains(&e.to.to_lowercase())).collect()
}

/// The dictionary a Whisper prompt is built from: the person's own words,
/// then memory's names.
pub fn with_names(dictionary: &[crate::config::DictionaryEntry]) -> Vec<crate::config::DictionaryEntry> {
    let mut all = dictionary.to_vec();
    all.extend(dictation_names(dictionary));
    all
}

// ---- changing memory ---------------------------------------------------------------------------

/// What one batch of changes did: its id (for Undo) and the changes.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    pub batch: u64,
    pub done: Vec<Done>,
}

struct UndoBatch {
    id: u64,
    /// Items the batch added.
    added: Vec<u64>,
    /// Items as they were before the batch (changed or removed).
    before: Vec<Item>,
}

static UNDO: Mutex<VecDeque<UndoBatch>> = Mutex::new(VecDeque::new());
static BATCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Keep a batch's undo (what it added, what it changed or removed).
fn keep_undo(before: &[Item], after: &[Item]) -> u64 {
    let id = BATCH.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    let added: Vec<u64> = after.iter().filter(|a| !before.iter().any(|b| b.id == a.id)).map(|a| a.id).collect();
    let changed: Vec<Item> = before.iter().filter(|b| after.iter().find(|a| a.id == b.id) != Some(b)).cloned().collect();
    if let Ok(mut u) = UNDO.lock() {
        u.push_back(UndoBatch { id, added, before: changed });
        while u.len() > 20 {
            u.pop_front();
        }
    }
    id
}

/// Apply `changes` (a pass's, a tool's, the person's) and save. Logs what
/// was left out (never the text).
pub fn change(changes: &[Change], rules: &Rules) -> Applied {
    let (out, batch) = with_store(|s| {
        let before = s.items.clone();
        let out = ops::apply(&mut s.items, &mut s.next_id, changes, rules);
        if s.items == before {
            return (out, 0);
        }
        let batch = keep_undo(&before, &s.items);
        save_to_disk(s);
        (out, batch)
    });
    for why in &out.dropped {
        tracing::info!(origin = %rules.origin.kind, "Memory: {why}");
    }
    if batch > 0 {
        tracing::info!(origin = %rules.origin.kind, changes = out.done.len(), "Memory changed");
        clear_caches();
        emit_changed();
    }
    Applied { batch, done: out.done }
}

/// Undo a batch: what it added goes, what it changed or removed comes back.
pub fn undo(batch: u64) -> Result<(), String> {
    let b = {
        let mut u = UNDO.lock().map_err(|_| "Undo isn't available")?;
        let at = u.iter().position(|b| b.id == batch).ok_or("That change can't be undone any more")?;
        u.remove(at).ok_or("That change can't be undone any more")?
    };
    with_store(|s| {
        s.items.retain(|i| !b.added.contains(&i.id));
        for old in &b.before {
            match s.items.iter_mut().find(|i| i.id == old.id) {
                Some(i) => *i = old.clone(),
                None => s.items.push(old.clone()),
            }
        }
        save_to_disk(s);
    });
    clear_caches();
    emit_changed();
    Ok(())
}

/// Tell the main window what Yap remembered by itself, while it's on
/// screen: a quiet toast with Undo and Manage. Never a Windows notification;
/// nothing at all with the window hidden (it's in Settings → Memory).
pub fn announce(applied: &Applied) {
    if applied.batch == 0 || applied.done.is_empty() {
        return;
    }
    let Some(app) = app() else {
        return;
    };
    let (visible, _) = crate::notices::notice::window_view(app);
    if !visible {
        return;
    }
    let _ = app.emit(EVENT_LEARNED, learned_payload(applied));
}

/// The toast's words: "Yap remembered: your sister is Rochelle", or for
/// several changes "Yap remembered 3 things" and each in the description.
pub fn learned_payload(applied: &Applied) -> Value {
    let verb = |d: &Done| match d.kind {
        "removed" => "forgot",
        _ if d.facts_added.is_empty() && !d.facts_removed.is_empty() && d.summary == d.summary_before => "forgot",
        "updated" if d.facts_added.is_empty() => "updated",
        _ => "remembered",
    };
    let says: Vec<String> = applied.done.iter().map(|d| d.say.clone()).filter(|s| !s.is_empty()).collect();
    let (title, description) = match applied.done.as_slice() {
        [d] if !d.say.is_empty() => (format!("Yap {}: {}", verb(d), d.say), String::new()),
        [d] => (format!("Yap {} something", verb(d)), String::new()),
        many => {
            let all_forgot = many.iter().all(|d| d.kind == "removed");
            (
                format!("Yap {} {} things", if all_forgot { "forgot" } else { "remembered" }, many.len()),
                says.join(" · "),
            )
        }
    };
    json!({ "batch": applied.batch, "title": title, "description": description, "count": applied.done.len() })
}

// ---- reading it --------------------------------------------------------------------------------

/// The "You" block for a system prompt (`prompt::you_block`): "" when Yap
/// remembers nothing about the person.
pub fn you_block() -> String {
    prompt::you_block(&items())
}

/// How the person wants answers, for the end of a question's context
/// (`prompt::answer_note`).
pub fn answer_note() -> Option<String> {
    prompt::answer_note(&items())
}

/// The people, projects and topics a question is about, as a block for its
/// context (`None`: nothing matches).
pub fn relevant_block(question: &str) -> Option<String> {
    prompt::relevant_block(&items(), question)
}

/// The block a notepad helper ends with: About you's name and note, then
/// what Yap remembers about the person. "" when there's nothing.
pub fn helper_block(cfg: &crate::config::YapConfig) -> String {
    let profile = crate::about_you::profile(cfg);
    let mut parts = Vec::new();
    if !profile.name.is_empty() {
        parts.push(format!("The user's name is {}.", profile.name));
    }
    if !profile.about.is_empty() {
        parts.push(format!("They told Yap: \"{}\"", profile.about.replace('"', "'")));
    }
    let you = you_block();
    if !you.is_empty() {
        parts.push(you);
    }
    parts.join("\n")
}

/// A notepad helper's messages with [`helper_block`] at the end of their
/// system prompt (its immutable start untouched). Unchanged when there's
/// nothing to add.
pub fn with_helper_block(mut messages: Value, cfg: &crate::config::YapConfig) -> Value {
    let block = helper_block(cfg);
    if block.is_empty() {
        return messages;
    }
    if let Some(system) = messages.as_array_mut().and_then(|m| m.iter_mut().find(|m| m["role"] == "system")) {
        let text = system["content"].as_str().unwrap_or("").to_string();
        system["content"] = json!(format!("{text}\n\n{block}"));
    }
    messages
}

// ---- the person's own changes (Settings) ---------------------------------------------------------

/// An item as Settings sends it (`id` none = a new one).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemInput {
    #[serde(default)]
    pub id: Option<u64>,
    pub section: Section,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub details: Vec<String>,
    #[serde(default = "yes")]
    pub dictation: bool,
}

fn yes() -> bool {
    true
}

/// Save an item as the person wrote it: cleaned and bounded like any
/// change, refused with the reason when it holds a secret (sensitive topics
/// are the person's to keep). Returns the item and the batch (Undo).
pub fn save_item(input: ItemInput) -> Result<(Item, u64), String> {
    let title = ops::one_line(&input.title, ops::MAX_NAME);
    let summary = ops::one_line(&input.summary, ops::MAX_ABOUT);
    let details: Vec<String> = input.details.iter().map(|d| ops::clean_fact(d)).filter(|d| !d.is_empty()).collect();
    if !input.section.is_you() && title.is_empty() {
        return Err("Give it a name".to_string());
    }
    for text in std::iter::once(&title).chain(std::iter::once(&summary)).chain(details.iter()) {
        if let Some(why) = guard::secret_in(text) {
            return Err(format!(
                "Yap doesn't keep {why} in memory: memory goes to the AI model with your questions. Take it out and save again."
            ));
        }
    }
    let now = now_secs();
    let saved = with_store(|s| {
        let before = s.items.clone();
        let max = input.section.max_facts();
        let mut details = details.clone();
        if details.len() > max {
            details.drain(..details.len() - max);
        }
        let existing = match input.id {
            Some(id) => Some(s.items.iter().position(|i| i.id == id).ok_or("That memory is gone")?),
            // A new Profile/Preferences line goes into the one item.
            None if input.section.is_you() => s.items.iter().position(|i| i.section == input.section),
            None => None,
        };
        let item = match existing {
            Some(at) => {
                let i = &mut s.items[at];
                if input.id.is_none() {
                    // Settings' "Add" on Profile/Preferences: more facts.
                    for d in details {
                        if !i.details.iter().any(|x| ops::norm(x) == ops::norm(&d)) {
                            i.details.push(d);
                        }
                    }
                    let extra = i.details.len().saturating_sub(max);
                    i.details.drain(..extra);
                } else {
                    if !i.section.is_you() {
                        i.title = title.clone();
                        i.summary = summary.clone();
                    }
                    i.details = details;
                    i.dictation = input.dictation;
                }
                i.updated_ts = now;
                i.source = Origin::new("manual", None, "");
                i.clone()
            }
            None => {
                if !input.section.is_you() && s.items.iter().filter(|i| i.section == input.section).count() >= input.section.max_items() {
                    return Err(format!("{} is full: delete one first", input.section.label()));
                }
                s.next_id += 1;
                let item = Item {
                    id: s.next_id,
                    section: input.section,
                    title: if input.section.is_you() { input.section.label().to_string() } else { title.clone() },
                    summary: if input.section.is_you() { String::new() } else { summary.clone() },
                    details,
                    source: Origin::new("manual", None, ""),
                    created_ts: now,
                    updated_ts: now,
                    dictation: input.dictation,
                };
                s.items.push(item.clone());
                item
            }
        };
        let batch = keep_undo(&before, &s.items);
        save_to_disk(s);
        Ok::<_, String>((item, batch))
    })?;
    clear_caches();
    emit_changed();
    Ok(saved)
}

/// Delete one item. Returns the batch (Undo).
pub fn delete_item(id: u64) -> Result<u64, String> {
    let batch = with_store(|s| {
        let before = s.items.clone();
        let at = s.items.iter().position(|i| i.id == id).ok_or("That memory is gone")?;
        s.items.remove(at);
        let batch = keep_undo(&before, &s.items);
        save_to_disk(s);
        Ok::<_, String>(batch)
    })?;
    clear_caches();
    emit_changed();
    Ok(batch)
}

/// "Spell this name in dictation" for a person or project.
pub fn set_dictation(id: u64, on: bool) -> Result<(), String> {
    with_store(|s| {
        let i = s.items.iter_mut().find(|i| i.id == id).ok_or("That memory is gone")?;
        i.dictation = on;
        save_to_disk(s);
        Ok::<_, String>(())
    })?;
    clear_caches();
    emit_changed();
    Ok(())
}

/// "Clear all memory": every item, and what the passes had read (so nothing
/// is learned again from old chats unless they go on). Returns the batch.
pub fn clear() -> u64 {
    let batch = with_store(|s| {
        let before = s.items.clone();
        s.items.clear();
        let batch = keep_undo(&before, &s.items);
        save_to_disk(s);
        batch
    });
    clear_caches();
    emit_changed();
    batch
}

// ---- bookkeeping for the passes (learn.rs) ----------------------------------------------------------

/// How many of a conversation's messages a pass has read.
pub fn read_upto(conversation: u64) -> usize {
    with_store(|s| s.read.get(&conversation).copied().unwrap_or(0))
}

/// A pass read a conversation up to `upto` messages.
pub fn mark_read(conversation: u64, upto: usize) {
    with_store(|s| {
        s.read.insert(conversation, upto);
        s.pending.retain(|c| *c != conversation);
        // Conversations deleted since: forget them (bounded).
        if s.read.len() > 500 {
            let keep: Vec<u64> = s.read.keys().rev().take(400).copied().collect();
            s.read.retain(|k, _| keep.contains(k));
        }
        save_to_disk(s);
    });
}

/// Conversations waiting for a pass, as kept across a quit.
pub fn pending() -> Vec<u64> {
    with_store(|s| s.pending.clone())
}

/// Keep `conversation` waiting for a pass across a quit.
pub fn set_pending(conversations: &[u64]) {
    with_store(|s| {
        if s.pending != conversations {
            s.pending = conversations.to_vec();
            save_to_disk(s);
        }
    });
}

/// Has a pass read this meeting? Marks it read when `mark`.
pub fn meeting_read(note: u64, mark: bool) -> bool {
    with_store(|s| {
        let was = s.meetings.contains(&note);
        if mark && !was {
            s.meetings.push(note);
            if s.meetings.len() > 300 {
                s.meetings.remove(0);
            }
            save_to_disk(s);
        }
        was
    })
}

// ---- Settings' commands ----------------------------------------------------------------------------

/// An item as Settings shows it: plus whether its name is fixed in
/// dictation's near-miss pass ("fixed") or only in the Whisper prompt
/// ("prompt").
fn item_view(i: &Item, words: &Words) -> Value {
    let mut v = serde_json::to_value(i).unwrap_or(Value::Null);
    if matches!(i.section, Section::People | Section::Projects) {
        v["dictationHint"] = json!(vocab::name_hint(&i.title, &|w| words.is(w)));
    }
    v
}

/// `memory_list`: every item (as Settings shows them), and whether an AI
/// model is set up to learn and change memory (`ai.ready`, else `ai.why`).
#[tauri::command]
pub async fn memory_list() -> Value {
    let cfg = crate::config::load();
    let ai = match learn::endpoint(&cfg) {
        // `small`: the Chat model can't call tools, so it learns by the
        // small path (Settings says what that changes).
        Ok(ep) => json!({ "ready": true, "learns": true, "small": learn::path(&ep) == learn::Path::Small }),
        Err(e) => json!({ "ready": false, "learns": false, "small": false, "why": e }),
    };
    let list = match tauri::async_runtime::spawn_blocking(move || {
        let words = Words::new();
        items().iter().map(|i| item_view(i, &words)).collect::<Vec<_>>()
    })
    .await
    {
        Ok(list) => list,
        // The dictation hints failed (the spell checker): the items without
        // them — never an empty list, which Settings would show as memory
        // gone.
        Err(e) => {
            tracing::warn!("Memory: listing without dictation hints: {e}");
            items().iter().map(|i| serde_json::to_value(i).unwrap_or(Value::Null)).collect()
        }
    };
    json!({ "items": list, "ai": ai })
}

// Each change below returns a save that failed as its error ("Couldn't save
// what Yap remembers about you: …", saving.rs); the change is kept in
// memory and saved with the next one, or Try again.

#[tauri::command]
pub fn memory_save_item(item: ItemInput) -> Result<Value, String> {
    let (item, batch) = save_item(item)?;
    saved()?;
    Ok(json!({ "item": item, "batch": batch }))
}

#[tauri::command]
pub fn memory_delete_item(id: u64) -> Result<u64, String> {
    let batch = delete_item(id)?;
    saved()?;
    Ok(batch)
}

#[tauri::command]
pub fn memory_set_dictation(id: u64, on: bool) -> Result<(), String> {
    set_dictation(id, on)?;
    Ok(saved()?)
}

#[tauri::command]
pub fn memory_clear() -> Result<u64, String> {
    let gone = clear();
    saved()?;
    Ok(gone)
}

#[tauri::command]
pub fn memory_undo(batch: u64) -> Result<(), String> {
    undo(batch)?;
    Ok(saved()?)
}

/// The prompt to paste into ChatGPT, Claude or Gemini (Import).
#[tauri::command]
pub fn memory_import_prompt() -> String {
    prompt::IMPORT_ASK.to_string()
}

/// "Tell Yap what to change": the model's changes, as a diff to confirm.
#[tauri::command]
pub async fn memory_tell(text: String) -> Result<learn::Proposal, String> {
    learn::propose(&text, learn::Kind::Tell).await
}

/// Import: what another assistant remembers, as changes to review.
#[tauri::command]
pub async fn memory_import(text: String) -> Result<learn::Proposal, String> {
    learn::propose(&text, learn::Kind::Import).await
}

/// Apply a proposal (all of it, or the changes `include` lists).
#[tauri::command]
pub fn memory_apply(proposal: u64, include: Option<Vec<usize>>) -> Result<Applied, String> {
    let applied = learn::apply_proposal(proposal, include.as_deref())?;
    saved()?;
    Ok(applied)
}

/// Write the export (.zip of Markdown) to `path`.
#[tauri::command]
pub fn memory_export(path: String) -> Result<(), String> {
    // About you's region, name and note head You.md: they're "You" too.
    let p = crate::about_you::profile(&crate::config::load());
    let about = [
        ("Name", p.name),
        ("Region", p.region.country_name.unwrap_or_default()),
        ("What you told Yap", if p.about.is_empty() { String::new() } else { format!("“{}”", p.about) }),
    ];
    let bytes = export::zip(&export::files(&items(), &about)).map_err(|e| e.to_string())?;
    std::fs::write(&path, bytes).map_err(|e| format!("Couldn't save the export: {e}"))
}

/// Debug-only (the e2e suite): run the learning pass over a conversation
/// now, as a quiet chat would, toast included.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn e2e_memory_learn(conversation_id: u64) -> Result<Value, String> {
    let applied = learn::learn_from_chat(conversation_id, true).await?;
    Ok(serde_json::to_value(applied).unwrap_or(Value::Null))
}

#[cfg(test)]
mod tests {
    use super::ops::{Action, Change};
    use super::*;

    fn add(section: Section, name: &str, facts: &[&str]) -> Change {
        Change {
            action: Action::Add,
            id: None,
            section: Some(section),
            name: name.into(),
            about: None,
            facts: facts.iter().map(|f| f.to_string()).collect(),
            remove_facts: vec![],
            say: String::new(),
        }
    }

    fn rules() -> Rules {
        Rules { sensitive_ok: false, origin: Origin::new("chat", Some(1), "Chat"), now: 10, max_changes: 5, grounding: None }
    }

    #[test]
    fn a_batch_can_be_undone() {
        let first = change(&[add(Section::People, "Rochelle", &["Lives in York"])], &rules());
        assert!(first.batch > 0);
        let second = change(&[add(Section::People, "Rochelle", &["Birthday 3 May"]), add(Section::Projects, "Allotment", &[])], &rules());
        assert_eq!(items().len(), 2);
        undo(second.batch).unwrap();
        let now = items();
        assert_eq!(now.len(), 1);
        assert_eq!(now[0].details, ["Lives in York"]);
        assert!(undo(second.batch).is_err(), "once");
        // Nothing changed: no batch.
        assert_eq!(change(&[add(Section::People, "Rochelle", &["Lives in York"])], &rules()).batch, 0);
    }

    #[test]
    fn the_person_can_edit_but_never_save_a_secret() {
        let (i, _) = save_item(ItemInput {
            id: None,
            section: Section::People,
            title: " Sam ".into(),
            summary: "Colleague".into(),
            details: vec!["Product manager".into(), "".into()],
            dictation: true,
        })
        .unwrap();
        assert_eq!((i.title.as_str(), i.details.len()), ("Sam", 1));
        let err = save_item(ItemInput {
            id: Some(i.id),
            section: Section::People,
            title: "Sam".into(),
            summary: String::new(),
            details: vec!["Phone 07700 900123".into()],
            dictation: true,
        })
        .unwrap_err();
        assert!(err.contains("a phone number"), "{err}");
        // Profile lines go into the one Profile item.
        for d in ["Works as a nurse", "Lives in Norwich"] {
            save_item(ItemInput { id: None, section: Section::Profile, title: String::new(), summary: String::new(), details: vec![d.into()], dictation: true }).unwrap();
        }
        let profile: Vec<Item> = items().into_iter().filter(|i| i.section == Section::Profile).collect();
        assert_eq!(profile.len(), 1);
        assert_eq!(profile[0].details, ["Works as a nurse", "Lives in Norwich"]);
        // Delete, clear, undo the clear.
        delete_item(i.id).unwrap();
        let cleared = clear();
        assert!(items().is_empty());
        undo(cleared).unwrap();
        assert_eq!(items().len(), 1);
    }

    #[test]
    fn the_toast_says_what_changed() {
        let a = change(&[add(Section::People, "Rochelle", &[])], &rules());
        let mut with_say = a.clone();
        with_say.done[0].say = "your sister is Rochelle".into();
        assert_eq!(learned_payload(&with_say)["title"], "Yap remembered: your sister is Rochelle");
        let b = change(&[add(Section::Projects, "Allotment", &[]), add(Section::Topics, "Running", &[])], &rules());
        let p = learned_payload(&b);
        assert_eq!(p["title"], "Yap remembered 2 things");
        assert_eq!(p["description"], "your project Allotment · Running");
    }
}

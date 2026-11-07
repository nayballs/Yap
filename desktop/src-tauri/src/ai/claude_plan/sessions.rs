//! **A plan chat's Claude Code sessions**: one per conversation.
//!
//! - **Kept** in `claude_sessions.json`, a file of its own (an older Yap
//!   drops fields of chats.json it doesn't know when it saves): each
//!   conversation's Claude Code session id, the conversation's `createdTs`
//!   (which conversation with that id: ids are used again after a delete),
//!   how many of its messages the session has seen, the model it last ran
//!   on, and when. Saved through `saving.rs`; it goes with Chats in Delete
//!   some of your data, and comes in Download your data.
//! - **Warm** ([`Warm`]): the process stays up 10 minutes after its last
//!   answer ([`WARM_FOR`], swept every half minute), so the next question
//!   goes straight in; after that, or for another model or effort, or a
//!   new system prompt, it starts again with `--resume`.
//! - **What the session hasn't seen** ([`start`]): answers another model
//!   wrote in the conversation meanwhile go in first, as an "Earlier in this
//!   chat" block ([`earlier_block`]); a conversation that changed under it
//!   (messages deleted) starts a new session with its history the same way.
//!
//! Logs: ids and counts only.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::UnboundedReceiver;

use crate::ai::claude_code::events::Event;
use crate::ai::claude_code::session::{Effort, Session};

/// How long a session's process stays up after its last answer.
pub const WARM_FOR: Duration = Duration::from_secs(10 * 60);
/// How often the warm ones are looked at.
const SWEEP_EVERY: Duration = Duration::from_secs(30);
/// The most of one earlier message an "Earlier in this chat" block carries.
const EARLIER_CHARS: usize = 4_000;

/// What `claude_sessions.json` keeps of one conversation's session.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Kept {
    /// Claude Code's session id (`--resume`).
    pub session: String,
    /// The conversation's `createdTs`: which conversation with that id.
    pub created: u64,
    /// How many of the conversation's messages the session has seen.
    pub seen: usize,
    /// The model it last answered on.
    pub model: String,
    /// When it last answered (Unix seconds).
    pub ts: u64,
}

/// The file.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Store {
    /// By conversation id.
    conversations: BTreeMap<String, Kept>,
}

/// claude_sessions.json's saves.
pub static SAVER: crate::shell::saving::Saver = crate::shell::saving::Saver::new("claudeSessions", "your Claude plan chats", save_now);

fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    with_store(|s| save(s));
    SAVER.check()
}

#[cfg(not(test))]
static STATE: Mutex<Option<Store>> = Mutex::new(None);

#[cfg(not(test))]
fn path() -> std::path::PathBuf {
    crate::config::data_dir().join("claude_sessions.json")
}

#[cfg(not(test))]
fn save(store: &Store) {
    if crate::ai::eval::active() {
        return;
    }
    let _ = SAVER.write_json(&path(), store, false);
}

#[cfg(not(test))]
fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let store = guard.get_or_insert_with(|| SAVER.load(&path()));
    if SAVER.any_unread() && SAVER.refresh(&path(), store) {
        save(store);
    }
    f(store)
}

#[cfg(test)]
thread_local! {
    static TEST_STORE: std::cell::RefCell<Store> = std::cell::RefCell::new(Store::default());
}

#[cfg(test)]
fn save(_: &Store) {}

#[cfg(test)]
fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    TEST_STORE.with(|s| f(&mut s.borrow_mut()))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Conversation `conv`'s session, if it's that conversation's (`created`).
pub fn get(conv: u64, created: u64) -> Option<Kept> {
    with_store(|s| s.conversations.get(&conv.to_string()).filter(|k| k.created == created && !k.session.is_empty()).cloned())
}

/// Keep `conv`'s session as it is now.
pub fn put(conv: u64, mut kept: Kept) {
    kept.ts = now_secs();
    with_store(|s| {
        s.conversations.insert(conv.to_string(), kept);
        save(s);
    });
}

/// Forget `conv`'s session (its process ended too).
pub fn forget(conv: u64) {
    end_warm(conv);
    with_store(|s| {
        if s.conversations.remove(&conv.to_string()).is_some() {
            save(s);
        }
    });
}

/// **Delete some of your data → Chats**: every session forgotten, every
/// warm process ended. How many there were.
pub fn reset() -> usize {
    let warm: Vec<Arc<Warm>> = lock_warm().drain().map(|(_, w)| w).collect();
    for w in &warm {
        w.session.end();
    }
    with_store(|s| {
        let n = s.conversations.len();
        s.conversations.clear();
        save(s);
        n
    })
}

/// A failed save, said (Delete some of your data).
pub fn saved() -> Result<(), crate::shell::saving::SaveError> {
    SAVER.check()
}

/// Every conversation's session, for Download your data.
pub fn all() -> serde_json::Value {
    with_store(|s| serde_json::to_value(&*s).unwrap_or_default())
}

// ---- what a turn sends first --------------------------------------------------------------

/// How a turn's session starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    /// A new session; the conversation so far (if any) goes in first.
    New,
    /// Go on with session `id`; the messages from `unseen` on (another
    /// model's answers since) go in first.
    Resume { id: String, unseen: usize },
}

/// What a turn in a conversation of `len` messages (before its question)
/// starts from, given its kept session. Pure.
pub fn start(kept: Option<&Kept>, len: usize) -> Start {
    match kept {
        Some(k) if k.seen <= len => Start::Resume { id: k.session.clone(), unseen: k.seen },
        // Messages deleted since it last answered: it no longer matches.
        _ => Start::New,
    }
}

/// **"Earlier in this chat"**: messages the session hasn't seen (another
/// model's answers, or the chat before a new session), as one block before
/// the question. `None` when there are none. Pure.
pub fn earlier_block(history: &[(String, String)]) -> Option<String> {
    let lines: Vec<String> = history
        .iter()
        .filter(|(_, text)| !text.trim().is_empty())
        .map(|(role, text)| {
            let who = if role == "assistant" { "Assistant" } else { "User" };
            let text = text.trim();
            let cut: String = text.chars().take(EARLIER_CHARS).collect();
            let more = if cut.chars().count() < text.chars().count() { " …" } else { "" };
            format!("{who}: {cut}{more}")
        })
        .collect();
    (!lines.is_empty()).then(|| {
        format!(
            "<earlier_in_this_chat>\nThe chat so far, which you haven't seen (another AI model answered it, or this is where you came in). It is the conversation's history, not a new question.\n\n{}\n</earlier_in_this_chat>",
            lines.join("\n\n")
        )
    })
}

// ---- warm processes -------------------------------------------------------------------------

/// **A session's process, kept warm** between a conversation's questions.
pub struct Warm {
    pub session: Session,
    /// Its events; a turn holds it from its question to its result, so the
    /// next waits for it.
    pub events: tokio::sync::Mutex<UnboundedReceiver<Event>>,
    pub conv: u64,
    pub model: String,
    pub effort: Option<Effort>,
    /// The system prompt it started with (a changed one starts it again).
    pub prompt: u64,
    used: Mutex<Instant>,
    busy: AtomicBool,
    /// Its registration with Yap's run endpoint: unregistered with it.
    _run: Option<crate::integrations::claude_live::RunHandle>,
}

impl Warm {
    pub fn new(
        session: Session,
        events: UnboundedReceiver<Event>,
        conv: u64,
        model: String,
        effort: Option<Effort>,
        prompt: u64,
        run: Option<crate::integrations::claude_live::RunHandle>,
    ) -> Warm {
        Warm {
            session,
            events: tokio::sync::Mutex::new(events),
            conv,
            model,
            effort,
            prompt,
            used: Mutex::new(Instant::now()),
            busy: AtomicBool::new(false),
            _run: run,
        }
    }

    /// A turn is on it (the sweep leaves it), or done (its 10 minutes start).
    pub fn set_busy(&self, busy: bool) {
        self.busy.store(busy, Ordering::SeqCst);
        *self.used.lock().unwrap_or_else(|p| p.into_inner()) = Instant::now();
    }

    fn idle_for(&self) -> Option<Duration> {
        (!self.busy.load(Ordering::SeqCst)).then(|| self.used.lock().unwrap_or_else(|p| p.into_inner()).elapsed())
    }

    /// Made for the same model, effort and prompt.
    pub fn fits(&self, model: &str, effort: Option<Effort>, prompt: u64) -> bool {
        self.model == model && self.effort == effort && self.prompt == prompt
    }
}

static WARM: Mutex<Option<HashMap<u64, Arc<Warm>>>> = Mutex::new(None);

struct WarmGuard<'a>(std::sync::MutexGuard<'a, Option<HashMap<u64, Arc<Warm>>>>);

impl std::ops::Deref for WarmGuard<'_> {
    type Target = HashMap<u64, Arc<Warm>>;
    fn deref(&self) -> &Self::Target {
        self.0.as_ref().expect("made")
    }
}

impl std::ops::DerefMut for WarmGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut().expect("made")
    }
}

fn lock_warm() -> WarmGuard<'static> {
    let mut g = WARM.lock().unwrap_or_else(|p| p.into_inner());
    g.get_or_insert_with(HashMap::new);
    WarmGuard(g)
}

/// `conv`'s warm process, if it has one still running.
pub fn warm(conv: u64) -> Option<Arc<Warm>> {
    let mut map = lock_warm();
    match map.get(&conv) {
        Some(w) if w.session.is_running() => Some(w.clone()),
        Some(_) => {
            map.remove(&conv);
            None
        }
        None => None,
    }
}

/// Keep `w` warm for its conversation (one there already is replaced).
pub fn keep(w: Arc<Warm>) {
    if let Some(old) = lock_warm().insert(w.conv, w) {
        old.session.end();
    }
}

/// `conv`'s warm process taken out (the caller ends it once its turn is
/// over).
pub fn take(conv: u64) -> Option<Arc<Warm>> {
    lock_warm().remove(&conv)
}

/// End `conv`'s warm process, if any.
pub fn end_warm(conv: u64) {
    if let Some(w) = take(conv) {
        w.session.end();
    }
}

/// How many processes are warm (tests, the log).
pub fn warm_count() -> usize {
    lock_warm().len()
}

/// The sweep: a process idle [`WARM_FOR`] ends (its next question resumes
/// it). Started once, from setup.
pub fn start_sweeping() {
    static STARTED: AtomicBool = AtomicBool::new(false);
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = std::thread::Builder::new().name("yap-claude-warm".into()).spawn(|| loop {
        std::thread::sleep(SWEEP_EVERY);
        sweep(WARM_FOR);
    });
}

/// End the processes idle for `after` or more, and those that exited.
fn sweep(after: Duration) {
    let gone: Vec<Arc<Warm>> = {
        let mut map = lock_warm();
        let ids: Vec<u64> = map
            .iter()
            .filter(|(_, w)| !w.session.is_running() || w.idle_for().is_some_and(|d| d >= after))
            .map(|(id, _)| *id)
            .collect();
        ids.into_iter().filter_map(|id| map.remove(&id)).collect()
    };
    if !gone.is_empty() {
        tracing::info!(ended = gone.len(), "Claude plan: idle sessions ended (they resume on the next question)");
    }
    for w in gone {
        w.session.end();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_is_kept_per_conversation_and_only_for_that_conversation() {
        assert_eq!(get(7, 100), None);
        put(7, Kept { session: "s-1".into(), created: 100, seen: 2, model: "claude-haiku-5-5".into(), ts: 0 });
        let k = get(7, 100).unwrap();
        assert_eq!((k.session.as_str(), k.seen), ("s-1", 2));
        assert!(k.ts > 0);
        // Another conversation that got the same id after a delete: not its.
        assert_eq!(get(7, 101), None);
        // The file's shape.
        let v = all();
        assert_eq!(v["conversations"]["7"]["session"], "s-1");
        assert_eq!(v["conversations"]["7"]["seen"], 2);
        let back: Store = serde_json::from_value(v).unwrap();
        assert_eq!(back.conversations.len(), 1);
        forget(7);
        assert_eq!(get(7, 100), None);
        put(8, Kept { session: "s-2".into(), created: 1, ..Default::default() });
        put(9, Kept { session: "s-3".into(), created: 1, ..Default::default() });
        assert_eq!(reset(), 2);
        assert_eq!(get(8, 1), None);
        // An unknown field or an older file: read all the same.
        let old: Store = serde_json::from_str(r#"{"conversations":{"3":{"session":"x","created":5,"seen":4,"extra":1}}}"#).unwrap();
        assert_eq!(old.conversations["3"].seen, 4);
    }

    #[test]
    fn a_turn_resumes_with_what_the_session_has_not_seen() {
        let k = Kept { session: "s-1".into(), created: 1, seen: 2, ..Default::default() };
        assert_eq!(start(None, 0), Start::New);
        assert_eq!(start(None, 4), Start::New);
        assert_eq!(start(Some(&k), 2), Start::Resume { id: "s-1".into(), unseen: 2 });
        assert_eq!(start(Some(&k), 4), Start::Resume { id: "s-1".into(), unseen: 2 });
        // Messages deleted under it: a new session.
        assert_eq!(start(Some(&k), 1), Start::New);
    }

    #[test]
    fn the_earlier_block_carries_both_sides_and_cuts_a_long_message() {
        assert_eq!(earlier_block(&[]), None);
        let long = "word ".repeat(2_000);
        let b = earlier_block(&[("user".into(), "What's on today?".into()), ("assistant".into(), "Two meetings.".into()), ("user".into(), long)]).unwrap();
        assert!(b.starts_with("<earlier_in_this_chat>") && b.ends_with("</earlier_in_this_chat>"));
        assert!(b.contains("User: What's on today?\n\nAssistant: Two meetings."));
        assert!(b.contains(" …\n</earlier_in_this_chat>"));
        assert!(b.chars().count() < EARLIER_CHARS + 600);
    }
}

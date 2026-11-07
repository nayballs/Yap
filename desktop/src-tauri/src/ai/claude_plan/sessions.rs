//! **A plan chat's Claude Code sessions**: one per conversation.
//!
//! - **Kept** in `claude_sessions.json`, a file of its own (an older Yap
//!   drops fields of chats.json it doesn't know when it saves): each
//!   conversation's Claude Code session id, the conversation's `createdTs`
//!   (which conversation with that id: ids are used again after a delete),
//!   how many of its messages the session has seen, the model it last ran
//!   on, and when. Saved through `saving.rs`; it goes with Chats in Delete
//!   some of your data, and comes in Download your data.
//! - **Claude Code's own copy**: Claude Code saves each session as a
//!   transcript in its own config folder (`claude_code/transcripts.rs`).
//!   Yap deletes a conversation's with it ([`forget`]: Chat's delete; [`reset`]:
//!   Delete some of your data → Chats, Reset app), and a session the
//!   conversation moved on from as soon as it's replaced ([`put`]): only the
//!   sessions kept here, only under Yap's own `claude/chat/` folder.
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

/// Keep `conv`'s session as it is now. A session it replaces (the
/// conversation moved on to a new one) is no use any more: Claude Code's
/// copy of it goes.
pub fn put(conv: u64, mut kept: Kept) {
    kept.ts = now_secs();
    let replaced = with_store(|s| {
        let old = s.conversations.insert(conv.to_string(), kept.clone());
        save(s);
        old.map(|o| o.session).filter(|o| !o.is_empty() && *o != kept.session)
    });
    if let Some(old) = replaced {
        in_background(move || remove_transcripts(vec![old]));
    }
}

/// Forget `conv`'s session, and Claude Code's copy of it (the conversation
/// is being deleted): its process ended first, so nothing more goes into
/// the transcript.
pub fn forget(conv: u64) {
    super::usage::forget_context(conv);
    let warm = take(conv);
    let gone = with_store(|s| {
        let gone = s.conversations.remove(&conv.to_string());
        if gone.is_some() {
            save(s);
        }
        gone
    });
    let ids: Vec<String> = gone.map(|k| k.session).into_iter().collect();
    in_background(move || {
        end_now(warm.as_slice());
        remove_transcripts(ids);
    });
}

/// **Delete some of your data → Chats**: every session forgotten, every
/// warm process ended, Claude Code's copies of them deleted. How many there
/// were.
pub fn reset() -> usize {
    super::usage::forget_contexts();
    let warm: Vec<Arc<Warm>> = lock_warm().drain().map(|(_, w)| w).collect();
    end_now(&warm);
    let (n, ids) = with_store(|s| {
        let ids: Vec<String> = s.conversations.values().map(|k| k.session.clone()).collect();
        let n = s.conversations.len();
        s.conversations.clear();
        save(s);
        (n, ids)
    });
    remove_transcripts(ids);
    n
}

/// How long a deleted chat's process gets to be gone before its transcript
/// is deleted anyway.
const END_WAIT: Duration = Duration::from_secs(5);

/// The processes of chats being deleted, ended now (their answers have
/// nowhere to go) and waited for, at most [`END_WAIT`]: then nothing more
/// goes into their transcripts.
fn end_now(warm: &[Arc<Warm>]) {
    for w in warm {
        w.session.kill();
    }
    let until = Instant::now() + END_WAIT;
    while warm.iter().any(|w| w.session.is_running()) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// `f` on a thread of its own (a unit test: now).
fn in_background(f: impl FnOnce() + Send + 'static) {
    if cfg!(test) {
        f();
    } else {
        let _ = std::thread::Builder::new().name("claude-transcripts".into()).spawn(f);
    }
}

/// **Claude Code's own copies of `sessions`** (its transcripts, filed under
/// Yap's `claude/chat/` folder, `claude_code::transcripts`) deleted: a chat
/// Yap deletes leaves no copy behind.
fn remove_transcripts(sessions: Vec<String>) {
    let sessions: Vec<String> = sessions.into_iter().filter(|s| !s.is_empty()).collect();
    if !sessions.is_empty() {
        crate::ai::claude_code::transcripts::remove_for(&super::oneshot::folder_path("chat"), &sessions);
    }
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

    /// Its registration with Yap's run endpoint (a turn's context goes on
    /// it: `RunHandle::begin_turn`).
    pub fn run(&self) -> Option<&crate::integrations::claude_live::RunHandle> {
        self._run.as_ref()
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

/// Every warm process still running (the plan's usage is asked on a free
/// one: `usage.rs`).
pub fn all_warm() -> Vec<Arc<Warm>> {
    lock_warm().values().filter(|w| w.session.is_running()).cloned().collect()
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
    fn claude_codes_copy_of_a_session_goes_with_its_chat() {
        use crate::ai::claude_code::transcripts::{project_dir_name, TEST_CONFIG};
        // A fake CLAUDE_CONFIG_DIR with the plan chats' transcripts in it.
        let config = std::env::temp_dir().join(format!("yap-plan-transcripts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&config);
        let chats = config.join("projects").join(project_dir_name(&super::super::oneshot::folder_path("chat").to_string_lossy()));
        let theirs = config.join("projects").join("E--Projects-garden-planner");
        std::fs::create_dir_all(&chats).unwrap();
        std::fs::create_dir_all(&theirs).unwrap();
        let ids = ["4d7c1d2e-8a51-4c3b-9f0e-2b6a7e1c5d90", "9f0e2b6a-7e1c-4d90-8a51-4c3b4d7c1d2e", "1b2c3d4e-0000-4000-8000-000000000001", "1b2c3d4e-0000-4000-8000-000000000002"];
        let transcript = |dir: &std::path::Path, id: &str| dir.join(format!("{id}.jsonl"));
        for id in ids {
            std::fs::write(transcript(&chats, id), "{}\n").unwrap();
            std::fs::write(transcript(&theirs, id), "{}\n").unwrap();
        }
        TEST_CONFIG.with(|c| *c.borrow_mut() = Some(config.clone()));

        // A conversation that moved on to a new session: the old copy goes.
        put(21, Kept { session: ids[0].into(), created: 5, ..Default::default() });
        put(21, Kept { session: ids[0].into(), created: 5, seen: 2, ..Default::default() });
        assert!(transcript(&chats, ids[0]).exists(), "the same session kept: nothing goes");
        put(21, Kept { session: ids[1].into(), created: 5, ..Default::default() });
        assert!(!transcript(&chats, ids[0]).exists(), "the session it replaced");
        // The conversation deleted: its session's copy goes.
        forget(21);
        assert!(!transcript(&chats, ids[1]).exists());
        // Delete some of your data → Chats: every kept session's.
        put(22, Kept { session: ids[2].into(), created: 1, ..Default::default() });
        put(23, Kept { session: ids[3].into(), created: 1, ..Default::default() });
        assert_eq!(reset(), 2);
        assert!(!transcript(&chats, ids[2]).exists() && !transcript(&chats, ids[3]).exists());
        // The person's own project, with the same file names: never touched.
        for id in ids {
            assert!(transcript(&theirs, id).exists(), "{id}");
        }
        TEST_CONFIG.with(|c| *c.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&config);
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

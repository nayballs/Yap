//! **The index, kept up to date in the background**: every passage of every
//! note, chat and memory item ([`chunks`](super::chunks); three [`Kind`]s
//! in one index) read by the embedding model once, its vector kept in the
//! store ([`store`](super::store)) by the passage's key.
//!
//! **When**: a while after Yap starts, then whenever something changes
//! (Yap's own events — `yap-note-changed`, `yap-notes-changed`, a deletion,
//! a recording's end, `yap-memory-changed`, a chat saved ([`changed`]) — or,
//! every two minutes, a cheap fingerprint of the notes and chats,
//! `notes::stamp` / `chats::stamp`, which also catches the local API's
//! edits). **Never in a dictation's way**: a pass waits while a dictation
//! or a meeting recording runs, and a batch under way is dropped the moment
//! one starts (`meeting_summary::unless_busy`: the connection closes) and
//! read again after. Passages already read are never read again (their key
//! is a hash of their words), so a pass cut off keeps what it did, and the
//! store drops the vectors of passages that are gone once they're over 30 %
//! of it — **at once when something was deleted** ([`removed`]: a note, a
//! chat, a memory item, a transcript's lines), even with search by meaning
//! off.
//!
//! The passages themselves are cut from the notes, chats and memory whenever
//! they're needed, kept per item while its fingerprint holds ([`prepared`],
//! [`prepared_chats`], [`prepared_memory`]).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Listener};

use super::chunks::{self, Chunk};
use super::embedder;
use super::rank;
use super::store::{Key, Store};

/// What a passage belongs to. Each searches apart (their ids are their own).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A note or a meeting.
    Note,
    /// A conversation in Chat.
    Chat,
    /// A person, project or topic Yap remembers.
    Memory,
}

/// A passage with what search needs of it, made once per version of its note.
pub struct Prepared {
    pub chunk: Chunk,
    /// What the embedding model reads ([`chunks::embed_text`]).
    pub embedded: String,
    pub key: Key,
    /// Its search words, and its note's title's.
    pub terms: Vec<String>,
    pub title_terms: Vec<String>,
    /// Each line's search words.
    pub line_terms: Vec<Vec<String>>,
    pub speakers: Vec<String>,
    /// Its words as written, in lower case: the Notes view's and the Chat
    /// sidebar's search by the words typed (`library.rs`).
    pub lower: String,
}

impl Prepared {
    pub fn of(chunk: Chunk) -> Prepared {
        let embedded = chunks::embed_text(&chunk);
        let key = chunks::key(embedder::model().tag, &embedded);
        let text = chunk.text();
        let terms = rank::terms(&text);
        let title_terms = rank::terms(&chunk.title);
        let line_terms = chunk.lines.iter().map(|l| rank::terms(&l.text)).collect();
        let speakers = chunk.speakers().into_iter().map(str::to_string).collect();
        let lower = text.to_lowercase();
        Prepared { chunk, embedded, key, terms, title_terms, line_terms, speakers, lower }
    }
}

fn hash_of(v: impl std::hash::Hash) -> u64 {
    use std::hash::Hasher;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

/// A note's version, for the passages kept of it.
fn fingerprint(n: &crate::notes::Note) -> u64 {
    hash_of((
        (n.updated_ts, &n.title, n.content.len(), n.enhanced_content.len(), n.transcript.len(), &n.note_type, &n.breaks),
        serde_json::to_string(&n.speakers).unwrap_or_default(),
        n.transcript.last().map(|s| (s.ts, s.text.len())),
    ))
}

/// A chat's version: its title and messages (what's searched of it).
fn chat_fingerprint(c: &crate::chat::chats::Conversation) -> u64 {
    hash_of((&c.title, c.messages.len(), c.messages.iter().map(|m| (m.ts, m.text.len())).collect::<Vec<_>>()))
}

/// A memory item's version: its name, who or what it is, its facts.
fn memory_fingerprint(i: &crate::memory::ops::Item) -> u64 {
    hash_of((&i.title, &i.summary, &i.details, i.section.is_you()))
}

type Cached = HashMap<(Kind, u64), (u64, Arc<Vec<Prepared>>)>;
#[cfg(not(test))]
static CACHE: Mutex<Option<Cached>> = Mutex::new(None);

#[cfg(not(test))]
fn with_cache<R>(f: impl FnOnce(&mut Cached) -> R) -> R {
    let mut g = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    f(g.get_or_insert_with(HashMap::new))
}

// Unit tests: a cache per test thread (as notes.rs keeps a store per thread).
#[cfg(test)]
thread_local! {
    static CACHE: std::cell::RefCell<Cached> = std::cell::RefCell::new(HashMap::new());
}

#[cfg(test)]
fn with_cache<R>(f: impl FnOnce(&mut Cached) -> R) -> R {
    CACHE.with(|c| f(&mut c.borrow_mut()))
}

/// Every note's passages, prepared (kept while the note doesn't change).
pub fn prepared(notes: &[crate::notes::Note]) -> Vec<Arc<Vec<Prepared>>> {
    with_cache(|cache| prepared_in(cache, Kind::Note, notes, |n| n.id, fingerprint, chunks::chunk_note))
}

/// Every chat's passages, prepared (archived ones too).
pub fn prepared_chats(convs: &[crate::chat::chats::Conversation]) -> Vec<Arc<Vec<Prepared>>> {
    with_cache(|cache| prepared_in(cache, Kind::Chat, convs, |c| c.id, chat_fingerprint, chunks::chunk_chat))
}

/// Every memory item's passage (people, projects and topics; none for
/// Profile and Preferences).
pub fn prepared_memory(items: &[crate::memory::ops::Item]) -> Vec<Arc<Vec<Prepared>>> {
    with_cache(|cache| prepared_in(cache, Kind::Memory, items, |i| i.id, memory_fingerprint, |i| chunks::chunk_memory(i).into_iter().collect()))
}

/// `items`' passages (one list per item, in order), from the cache while
/// an item's fingerprint holds; the cache forgets the items of `kind` that
/// are gone.
fn prepared_in<T>(
    cache: &mut Cached,
    kind: Kind,
    items: &[T],
    id: impl Fn(&T) -> u64,
    fingerprint: impl Fn(&T) -> u64,
    chunk: impl Fn(&T) -> Vec<Chunk>,
) -> Vec<Arc<Vec<Prepared>>> {
    let mut out = Vec::with_capacity(items.len());
    let mut seen = HashSet::with_capacity(items.len());
    for item in items {
        let key = (kind, id(item));
        seen.insert(key.1);
        let fp = fingerprint(item);
        match cache.get(&key) {
            Some((f, p)) if *f == fp => out.push(p.clone()),
            _ => {
                let p = Arc::new(chunk(item).into_iter().map(Prepared::of).collect::<Vec<_>>());
                cache.insert(key, (fp, p.clone()));
                out.push(p);
            }
        }
    }
    cache.retain(|(k, id), _| *k != kind || seen.contains(id));
    out
}

/// Every chat, its memory and its notes' passages, as the index reads them.
pub struct Library {
    pub notes: Vec<Arc<Vec<Prepared>>>,
    pub chats: Vec<Arc<Vec<Prepared>>>,
    pub memory: Vec<Arc<Vec<Prepared>>>,
    /// Each item, by kind and id (for the sweep).
    pub ids: Vec<(Kind, u64)>,
}

impl Library {
    /// Everything as it is now.
    pub fn now() -> Library {
        let (notes, mut ids) = crate::notes::with_all(|ns| (prepared(ns), ns.iter().map(|n| (Kind::Note, n.id)).collect::<Vec<_>>()));
        let (chats, chat_ids) = crate::chat::chats::with_all(|cs| (prepared_chats(cs), cs.iter().map(|c| (Kind::Chat, c.id)).collect::<Vec<_>>()));
        let items = crate::memory::items();
        let memory = prepared_memory(&items);
        ids.extend(chat_ids);
        ids.extend(items.iter().map(|i| (Kind::Memory, i.id)));
        Library { notes, chats, memory, ids }
    }

    /// Every passage: the notes', then memory's, then the chats'.
    pub fn passages(&self) -> impl Iterator<Item = &Prepared> {
        self.notes.iter().chain(&self.memory).chain(&self.chats).flat_map(|v| v.iter())
    }

    /// The items with at least one passage, of each kind: (notes, chats, memory).
    pub fn counts(&self) -> (usize, usize, usize) {
        let n = |v: &[Arc<Vec<Prepared>>]| v.iter().filter(|p| !p.is_empty()).count();
        (n(&self.notes), n(&self.chats), n(&self.memory))
    }
}

// ---- the store, shared with the searches ---------------------------------------

/// `<data>/search-index/`.
pub fn dir() -> std::path::PathBuf {
    crate::config::data_dir().join("search-index")
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);

/// The store, opened on first use.
pub fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut g = STORE.lock().unwrap_or_else(|p| p.into_inner());
    let m = embedder::model();
    let s = g.get_or_insert_with(|| Store::open(&dir(), m.tag, m.dims));
    f(s)
}

/// Forget the index (Rebuild, Delete): the file goes, the next pass starts
/// from nothing.
pub fn forget() -> Result<(), String> {
    let mut g = STORE.lock().unwrap_or_else(|p| p.into_inner());
    *g = None;
    let d = dir();
    if d.exists() {
        std::fs::remove_dir_all(&d).map_err(|e| format!("Couldn't delete the search index: {e}"))?;
    }
    set_counts(0, 0, Some(Counts::default()));
    Ok(())
}

/// The vectors of every passage that's gone, dropped now (a deletion: the
/// note's, chat's or memory item's vectors go with it). Works with search
/// by meaning off too; nothing to do without an index on disk.
fn sweep(library: &Library) {
    if !dir().join(super::store::FILE).is_file() {
        return;
    }
    // A store Yap couldn't read holds only what was added since: its
    // passages aren't gone, only not read yet.
    if crate::notes::unread() || crate::chat::chats::unread() || crate::memory::unread() {
        return;
    }
    let keep: HashSet<Key> = library.passages().map(|p| p.key).collect();
    let dead = with_store(|s| s.dead_share(&keep));
    if dead > 0.0 {
        match with_store(|s| s.compact(&keep)) {
            Ok(gone) => tracing::info!(gone, "Search: dropped the vectors of what was deleted"),
            Err(e) => tracing::warn!("Search: couldn't drop the vectors of what was deleted: {e}"),
        }
    }
}

// ---- status -------------------------------------------------------------------

/// What the index is doing, for Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    /// "Search by meaning" is off.
    Off,
    /// On, but the model isn't downloaded.
    NotInstalled,
    /// Waiting for a dictation or a meeting recording to finish.
    Waiting,
    Indexing,
    Ready,
    /// The model wouldn't start or answer (`error` says why); tried again later.
    Error,
}

/// What the index holds, by kind: the notes, chats and memory items it
/// read, and each kind's passages.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub notes: usize,
    pub chats: usize,
    pub memories: usize,
    pub note_passages: usize,
    pub chat_passages: usize,
    pub memory_passages: usize,
}

struct State {
    phase: Phase,
    error: Option<String>,
    passages: usize,
    indexed: usize,
    counts: Counts,
}

static STATE: Mutex<State> = Mutex::new(State {
    phase: Phase::Off,
    error: None,
    passages: 0,
    indexed: 0,
    counts: Counts { notes: 0, chats: 0, memories: 0, note_passages: 0, chat_passages: 0, memory_passages: 0 },
});

/// The index as Settings shows it.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub enabled: bool,
    pub installed: bool,
    /// The on-device AI engine is here (else a download brings it too).
    pub runtime_installed: bool,
    pub installing: bool,
    pub phase: Phase,
    pub error: Option<String>,
    /// Passages in every note, chat and memory item, and how many of them
    /// the index has read.
    pub passages: usize,
    pub indexed: usize,
    /// The notes, chats and memory items with passages.
    pub notes: usize,
    pub chats: usize,
    pub memories: usize,
    /// Each kind's passages.
    pub kinds: Counts,
    /// The index on disk (bytes).
    pub index_bytes: u64,
    /// The model on disk, and what a download would fetch now.
    pub model_bytes: u64,
    pub download_bytes: u64,
    /// "gpu" | "partial" | "cpu" (where it ran last).
    pub runs_on: Option<&'static str>,
    pub model_name: &'static str,
    pub license: &'static str,
    pub license_url: &'static str,
}

pub fn status() -> Status {
    let cfg = crate::config::load();
    let installed = embedder::installed();
    let (phase, error, passages, indexed, counts) = {
        let s = STATE.lock().unwrap_or_else(|p| p.into_inner());
        (s.phase, s.error.clone(), s.passages, s.indexed, s.counts)
    };
    let phase = match (cfg.search_by_meaning, installed) {
        (false, _) => Phase::Off,
        (true, false) => Phase::NotInstalled,
        _ => phase,
    };
    let m = embedder::model();
    Status {
        enabled: cfg.search_by_meaning,
        installed,
        runtime_installed: embedder::runtime_installed(),
        installing: embedder::installing(),
        phase,
        error,
        passages,
        indexed,
        notes: counts.notes,
        chats: counts.chats,
        memories: counts.memories,
        kinds: counts,
        index_bytes: std::fs::metadata(dir().join(super::store::FILE)).map(|m| m.len()).unwrap_or(0),
        model_bytes: std::fs::metadata(embedder::model_path()).map(|m| m.len()).unwrap_or(0),
        download_bytes: embedder::download_bytes(),
        runs_on: embedder::runs_on(),
        model_name: m.name,
        license: m.license,
        license_url: m.license_url,
    }
}

fn set_phase(phase: Phase, error: Option<String>) {
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    s.phase = phase;
    s.error = error;
}

fn set_counts(passages: usize, indexed: usize, counts: Option<Counts>) {
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    s.passages = passages;
    s.indexed = indexed;
    if let Some(c) = counts {
        s.counts = c;
    }
}

/// What `library` holds, by kind.
fn counts_of(library: &Library) -> Counts {
    let (notes, chats, memories) = library.counts();
    let n = |v: &[Arc<Vec<Prepared>>]| v.iter().map(|p| p.len()).sum();
    Counts { notes, chats, memories, note_passages: n(&library.notes), chat_passages: n(&library.chats), memory_passages: n(&library.memory) }
}

/// Tell the pages (`yap-search-status`).
pub fn emit(app: &AppHandle) {
    let _ = app.emit("yap-search-status", status());
}

// ---- the background pass -------------------------------------------------------

static DIRTY: AtomicBool = AtomicBool::new(true);
static WAKE: tokio::sync::Notify = tokio::sync::Notify::const_new();
static STARTED: AtomicBool = AtomicBool::new(false);

/// How long after Yap starts the first pass waits (the speech model and the
/// on-device model load first).
const FIRST_DELAY: Duration = Duration::from_secs(45);
/// The fingerprint's look, and the wait after a failure (doubling to 30 min).
const POLL: Duration = Duration::from_secs(120);
const MAX_BACKOFF: Duration = Duration::from_secs(30 * 60);
/// A burst of changes (a meeting's lines, typing) settles first.
const SETTLE: Duration = Duration::from_secs(8);
/// Passages per request: on the card a batch is a fraction of a second; on
/// the processor a few seconds at most, so a dictation never waits long.
const BATCH_GPU: usize = 16;
const BATCH_CPU: usize = 4;
/// Compact once this share of the store is passages that are gone (edits
/// leave their old passages' vectors behind until then)…
const DEAD_SHARE: f32 = 0.3;
/// …or at once when a note, a chat or a memory item is gone: the ones the
/// last pass saw.
static SEEN: Mutex<Option<HashSet<(Kind, u64)>>> = Mutex::new(None);
/// Something was deleted ([`removed`]): its vectors go at the next look,
/// search by meaning on or off. True at start: another Yap on the same data
/// folder (the installed nightly beside a dev build, an older version that
/// doesn't keep the index) may have deleted or changed notes and chats
/// since — the first look drops what's gone, and the first pass reads what's
/// new. (Searches never show what's gone either way: their passages are cut
/// from the stores as they are; a vector is only looked up by its key.)
static SWEEP: AtomicBool = AtomicBool::new(true);

/// Is an item the last pass saw gone now? The first pass of a run says yes
/// (one may have been deleted while Yap or the search was off). Remembers
/// `now` for the next.
fn item_gone<K: Copy + Eq + std::hash::Hash>(seen: &Mutex<Option<HashSet<K>>>, now: &[K]) -> bool {
    let now: HashSet<K> = now.iter().copied().collect();
    let mut seen = seen.lock().unwrap_or_else(|e| e.into_inner());
    let gone = seen.as_ref().is_none_or(|before| before.iter().any(|id| !now.contains(id)));
    *seen = Some(now);
    gone
}

/// Notes, chats or memory changed (or the setting): a pass soon.
pub fn poke() {
    DIRTY.store(true, Ordering::SeqCst);
    WAKE.notify_one();
}

/// Something was deleted — a note, a chat, a memory item, a transcript's
/// lines — and its vectors go too: at the next look (a few seconds), with
/// search by meaning on or off.
pub fn removed() {
    SWEEP.store(true, Ordering::SeqCst);
    poke();
}

fn first_delay() -> Duration {
    if crate::shell::e2e::active() {
        return Duration::from_millis(300);
    }
    FIRST_DELAY
}

fn settle() -> Duration {
    if crate::shell::e2e::active() {
        return Duration::from_millis(200);
    }
    SETTLE
}

/// Start the background index (once).
pub fn init(app: &AppHandle) {
    if STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    for event in ["yap-note-changed", "yap-notes-changed", "yap-meeting-state"] {
        app.listen(event, |_| poke());
    }
    // A deletion (memory's changes may be one: an item forgotten, Clear all).
    for event in ["yap-note-deleted", crate::memory::EVENT_CHANGED] {
        app.listen(event, |_| removed());
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(first_delay()).await;
        let mut last_stamp: Option<u64> = None;
        let mut wait = POLL;
        loop {
            let cfg = crate::config::load();
            let swept = SWEEP.load(Ordering::SeqCst);
            if !cfg.search_by_meaning || !embedder::installed() {
                set_phase(if cfg.search_by_meaning { Phase::NotInstalled } else { Phase::Off }, None);
                // Off, a deletion still takes its vectors with it.
                if SWEEP.swap(false, Ordering::SeqCst) {
                    sweep(&Library::now());
                    emit(&app);
                }
            } else {
                let stamp = crate::notes::stamp() ^ crate::chat::chats::stamp().rotate_left(1);
                let dirty = DIRTY.swap(false, Ordering::SeqCst);
                if dirty || swept || last_stamp != Some(stamp) {
                    match pass(&app).await {
                        Ok(true) => {
                            last_stamp = Some(stamp);
                            wait = POLL;
                        }
                        Ok(false) => {} // turned off meanwhile
                        Err(e) => {
                            tracing::warn!(retry_s = wait.as_secs(), "Meeting search: indexing stopped: {e}");
                            set_phase(Phase::Error, Some(e));
                            emit(&app);
                            wait = (wait * 2).min(MAX_BACKOFF);
                        }
                    }
                }
            }
            tokio::select! {
                _ = WAKE.notified() => tokio::time::sleep(settle()).await,
                _ = tokio::time::sleep(wait) => {}
            }
        }
    });
}

fn busy(app: &AppHandle) -> bool {
    crate::meetings::meeting_summary::dictation_busy(app) || crate::meetings::meeting::is_recording()
}

fn enabled() -> bool {
    crate::config::load().search_by_meaning
}

/// One pass: every passage not read yet, read. `Ok(false)`: turned off
/// meanwhile.
async fn pass(app: &AppHandle) -> Result<bool, String> {
    if busy(app) {
        set_phase(Phase::Waiting, None);
        emit(app);
        if !crate::meetings::meeting_summary::wait_idle(|| busy(app), enabled).await {
            return Ok(false);
        }
    }
    let library = Library::now();
    let all: Vec<&Prepared> = library.passages().collect();
    let keep: HashSet<Key> = all.iter().map(|p| p.key).collect();
    let mut queued = HashSet::new();
    let missing: Vec<&Prepared> = with_store(|s| all.iter().copied().filter(|p| !s.has(&p.key) && queued.insert(p.key)).collect());
    let total = keep.len();
    let counts = counts_of(&library);
    set_counts(total, total - missing.len(), Some(counts));
    if !missing.is_empty() {
        tracing::info!(passages = total, to_read = missing.len(), notes = counts.notes, chats = counts.chats, memories = counts.memories, "Search: indexing");
        set_phase(Phase::Indexing, None);
        emit(app);
        let t0 = Instant::now();
        let mut done = 0usize;
        let mut told = Instant::now();
        let mut i = 0;
        while i < missing.len() {
            if !enabled() {
                return Ok(false);
            }
            if busy(app) {
                set_phase(Phase::Waiting, None);
                emit(app);
                if !crate::meetings::meeting_summary::wait_idle(|| busy(app), enabled).await {
                    return Ok(false);
                }
                set_phase(Phase::Indexing, None);
            }
            let size = if embedder::runs_on() == Some("gpu") { BATCH_GPU } else { BATCH_CPU };
            let group = &missing[i..(i + size).min(missing.len())];
            let texts: Vec<String> = group.iter().map(|p| p.embedded.clone()).collect();
            // A dictation starting drops the batch (read again after it).
            let Some(result) = crate::meetings::meeting_summary::unless_busy(embedder::embed(&texts), || busy(app)).await else {
                continue;
            };
            let vectors = result?;
            let items: Vec<(Key, Vec<f32>)> = group.iter().map(|p| p.key).zip(vectors).collect();
            with_store(|s| s.add(&items)).map_err(|e| format!("couldn't save the search index: {e}"))?;
            done += group.len();
            i += group.len();
            set_counts(total, total - missing.len() + done, None);
            if told.elapsed() > Duration::from_millis(700) {
                told = Instant::now();
                emit(app);
            }
        }
        tracing::info!(read = done, ms = t0.elapsed().as_millis() as u64, on = embedder::runs_on().unwrap_or("?"), "Meeting search: indexed");
    }
    // The vectors of passages that are gone: a deleted note's, chat's or
    // memory item's at once (its index goes with it), and whatever else was
    // deleted (`removed`: a transcript's lines); an edit's once they're a
    // good part of it.
    let deleted = item_gone(&SEEN, &library.ids) | SWEEP.swap(false, Ordering::SeqCst);
    let dead = with_store(|s| s.dead_share(&keep));
    if dead > DEAD_SHARE || (deleted && dead > 0.0) {
        match with_store(|s| s.compact(&keep)) {
            Ok(gone) => tracing::info!(gone, "Meeting search: dropped the vectors of passages that are gone"),
            Err(e) => tracing::warn!("Meeting search: couldn't compact the index: {e}"),
        }
    }
    set_phase(Phase::Ready, None);
    emit(app);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::chunks::tests::{line, note};
    use serde_json::json;

    #[test]
    fn passages_are_kept_while_their_note_holds() {
        let mut n = note(json!({
            "id": 41,
            "noteType": "meeting",
            "title": "Launch review",
            "transcript": [line("them", "We agreed the launch moves to March third.", 100)],
        }));
        let first = prepared(std::slice::from_ref(&n));
        let again = prepared(std::slice::from_ref(&n));
        assert!(Arc::ptr_eq(&first[0], &again[0]), "the same note: the same passages");
        let p = &first[0][0];
        assert_eq!(p.embedded, "title: Launch review | text: Them: We agreed the launch moves to March third.");
        assert!(p.terms.contains(&"launch".to_string()) && !p.terms.contains(&"review".to_string()));
        assert_eq!(p.title_terms, ["launch", "review"]);
        assert_eq!(p.speakers, ["Them"]);
        // A change: made again, with a new key.
        n.transcript[0].text = "We agreed the launch moves to April.".into();
        n.updated_ts += 1;
        let changed = prepared(std::slice::from_ref(&n));
        assert!(!Arc::ptr_eq(&first[0], &changed[0]));
        assert_ne!(changed[0][0].key, p.key);
        // A note that's gone leaves the cache.
        prepared(&[]);
        assert!(with_cache(|c| !c.contains_key(&(Kind::Note, 41))));
    }

    #[test]
    fn a_deleted_note_sweeps_the_index_at_once() {
        let seen = Mutex::new(None);
        assert!(item_gone(&seen, &[1, 2, 3]), "a run's first pass sweeps (deleted while off)");
        assert!(!item_gone(&seen, &[1, 2, 3]));
        assert!(!item_gone(&seen, &[1, 2, 3, 4]), "a new note isn't a deletion");
        assert!(item_gone(&seen, &[1, 3, 4]), "note 2 is gone");
        assert!(!item_gone(&seen, &[1, 3, 4]));
        // Kinds apart: a chat 2 isn't note 2.
        let seen = Mutex::new(None);
        item_gone(&seen, &[(Kind::Note, 2), (Kind::Chat, 2)]);
        assert!(!item_gone(&seen, &[(Kind::Note, 2), (Kind::Chat, 2), (Kind::Memory, 2)]));
        assert!(item_gone(&seen, &[(Kind::Note, 2), (Kind::Memory, 2)]), "the chat is gone");
    }

    fn conversation(id: u64, title: &str, messages: &[(&str, &str)]) -> crate::chat::chats::Conversation {
        serde_json::from_value(json!({
            "id": id,
            "title": title,
            "createdTs": 100,
            "updatedTs": 200,
            "messages": messages.iter().enumerate().map(|(i, (role, text))| json!({ "role": role, "text": text, "ts": 100 + i as u64 })).collect::<Vec<_>>(),
        }))
        .expect("a conversation")
    }

    #[test]
    fn chats_and_memory_are_kinds_of_their_own_in_one_cache() {
        let note = note(json!({ "id": 7, "title": "Lisbon", "content": "Flights booked for May." }));
        let chat = conversation(7, "Trip ideas", &[("user", "Where should we eat in Lisbon?"), ("assistant", "Try the seafood places in Alfama.")]);
        let n = prepared(std::slice::from_ref(&note));
        let c = prepared_chats(std::slice::from_ref(&chat));
        assert_eq!(c[0][0].chunk.kind, "chat");
        assert_eq!(c[0][0].chunk.text(), "You: Where should we eat in Lisbon?\nYap: Try the seafood places in Alfama.");
        assert!(c[0][0].lower.contains("alfama"));
        // The same id in two kinds: two entries, neither replacing the other.
        assert!(Arc::ptr_eq(&n[0], &prepared(std::slice::from_ref(&note))[0]));
        assert!(Arc::ptr_eq(&c[0], &prepared_chats(std::slice::from_ref(&chat))[0]));
        // A new message: the chat's passages again (a new key), the note's kept.
        let mut more = chat.clone();
        more.messages.push(serde_json::from_value(json!({ "role": "user", "text": "And for breakfast?", "ts": 300 })).unwrap());
        let c2 = prepared_chats(std::slice::from_ref(&more));
        assert_ne!(c2[0][0].key, c[0][0].key);
        assert!(Arc::ptr_eq(&n[0], &prepared(std::slice::from_ref(&note))[0]));
        // Chats gone: only theirs leave the cache.
        prepared_chats(&[]);
        assert!(with_cache(|cache| !cache.contains_key(&(Kind::Chat, 7)) && cache.contains_key(&(Kind::Note, 7))));
    }

    #[test]
    fn a_deleted_chat_takes_its_vectors_with_it() {
        let dir = std::env::temp_dir().join(format!("yap-search-sweep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut store = Store::open(&dir, "test-model", 4);
        let a = conversation(1, "Trip", &[("user", "Where should we eat in Lisbon?")]);
        let b = conversation(2, "Bread", &[("user", "How long do I prove sourdough?")]);
        let both = prepared_chats(&[a.clone(), b.clone()]);
        let items: Vec<(Key, Vec<f32>)> = both.iter().flat_map(|v| v.iter()).map(|p| (p.key, vec![1.0, 0.0, 0.5, 0.0])).collect();
        store.add(&items).unwrap();
        // Chat 2 deleted: what's kept is what the library holds now (`sweep`).
        let left = prepared_chats(std::slice::from_ref(&a));
        let keep: HashSet<Key> = left.iter().flat_map(|v| v.iter()).map(|p| p.key).collect();
        assert!((store.dead_share(&keep) - 0.5).abs() < 1e-6);
        assert_eq!(store.compact(&keep).unwrap(), 1);
        assert!(store.has(&both[0][0].key) && !store.has(&both[1][0].key));
        // On disk too.
        let again = Store::open(&dir, "test-model", 4);
        assert_eq!(again.len(), 1);
        assert!(!again.has(&both[1][0].key));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

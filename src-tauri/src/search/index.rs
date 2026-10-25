//! **The index, kept up to date in the background**: every passage of every
//! note ([`chunks`](super::chunks)) read by the embedding model once, its
//! vector kept in the store ([`store`](super::store)) by the passage's key.
//!
//! **When**: a while after Yap starts, then whenever notes change (Yap's
//! own events — `yap-note-changed`, `yap-notes-changed`, a deletion, a
//! recording's end — or, every two minutes, a cheap fingerprint of the notes,
//! `notes::stamp`, which also catches the local API's and Chat's edits).
//! **Never in a dictation's way**: a pass waits while a dictation or a
//! meeting recording runs, and a batch under way is dropped the moment one
//! starts (`meeting_summary::unless_busy`: the connection closes) and read
//! again after. Passages already read are never read again (their key is a
//! hash of their words), so a pass cut off keeps what it did, and the store
//! drops the vectors of passages that are gone once they're over 30 % of it.
//!
//! The passages themselves are cut from the notes whenever they're needed,
//! kept per note while its fingerprint holds ([`prepared`]).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Listener};

use super::chunks::{self, Chunk};
use super::embedder;
use super::rank;
use super::store::{Key, Store};

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
}

impl Prepared {
    pub fn of(chunk: Chunk) -> Prepared {
        let embedded = chunks::embed_text(&chunk);
        let key = chunks::key(embedder::MODEL_TAG, &embedded);
        let terms = rank::terms(&chunk.text());
        let title_terms = rank::terms(&chunk.title);
        let line_terms = chunk.lines.iter().map(|l| rank::terms(&l.text)).collect();
        let speakers = chunk.speakers().into_iter().map(str::to_string).collect();
        Prepared { chunk, embedded, key, terms, title_terms, line_terms, speakers }
    }
}

/// A note's version, for the passages kept of it.
fn fingerprint(n: &crate::notes::Note) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (n.updated_ts, &n.title, n.content.len(), n.enhanced_content.len(), n.transcript.len(), &n.note_type, &n.breaks).hash(&mut h);
    serde_json::to_string(&n.speakers).unwrap_or_default().hash(&mut h);
    n.transcript.last().map(|s| (s.ts, s.text.len())).hash(&mut h);
    h.finish()
}

type Cached = HashMap<u64, (u64, Arc<Vec<Prepared>>)>;
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
    with_cache(|cache| prepared_in(cache, notes))
}

fn prepared_in(cache: &mut Cached, notes: &[crate::notes::Note]) -> Vec<Arc<Vec<Prepared>>> {
    let mut out = Vec::with_capacity(notes.len());
    let mut seen = HashSet::with_capacity(notes.len());
    for n in notes {
        seen.insert(n.id);
        let fp = fingerprint(n);
        match cache.get(&n.id) {
            Some((f, p)) if *f == fp => out.push(p.clone()),
            _ => {
                let p = Arc::new(chunks::chunk_note(n).into_iter().map(Prepared::of).collect::<Vec<_>>());
                cache.insert(n.id, (fp, p.clone()));
                out.push(p);
            }
        }
    }
    cache.retain(|id, _| seen.contains(id));
    out
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
    let s = g.get_or_insert_with(|| Store::open(&dir(), embedder::MODEL_TAG, embedder::DIMS));
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
    set_counts(0, 0, None);
    Ok(())
}

// ---- status -------------------------------------------------------------------

/// What the index is doing, for Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    /// "Search my meetings by meaning" is off.
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

struct State {
    phase: Phase,
    error: Option<String>,
    passages: usize,
    indexed: usize,
    notes: usize,
}

static STATE: Mutex<State> = Mutex::new(State { phase: Phase::Off, error: None, passages: 0, indexed: 0, notes: 0 });

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
    /// Passages in every note, and how many of them the index has read.
    pub passages: usize,
    pub indexed: usize,
    pub notes: usize,
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
    let (phase, error, passages, indexed, notes) = {
        let s = STATE.lock().unwrap_or_else(|p| p.into_inner());
        (s.phase, s.error.clone(), s.passages, s.indexed, s.notes)
    };
    let phase = match (cfg.search_by_meaning, installed) {
        (false, _) => Phase::Off,
        (true, false) => Phase::NotInstalled,
        _ => phase,
    };
    Status {
        enabled: cfg.search_by_meaning,
        installed,
        runtime_installed: embedder::runtime_installed(),
        installing: embedder::installing(),
        phase,
        error,
        passages,
        indexed,
        notes,
        index_bytes: std::fs::metadata(dir().join(super::store::FILE)).map(|m| m.len()).unwrap_or(0),
        model_bytes: std::fs::metadata(embedder::model_path()).map(|m| m.len()).unwrap_or(0),
        download_bytes: embedder::download_bytes(),
        runs_on: embedder::runs_on(),
        model_name: embedder::MODEL_NAME,
        license: embedder::LICENSE,
        license_url: embedder::LICENSE_URL,
    }
}

fn set_phase(phase: Phase, error: Option<String>) {
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    s.phase = phase;
    s.error = error;
}

fn set_counts(passages: usize, indexed: usize, notes: Option<usize>) {
    let mut s = STATE.lock().unwrap_or_else(|p| p.into_inner());
    s.passages = passages;
    s.indexed = indexed;
    if let Some(n) = notes {
        s.notes = n;
    }
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
/// …or at once when a note is gone: the notes the last pass saw.
static SEEN: Mutex<Option<HashSet<u64>>> = Mutex::new(None);

/// Is a note the last pass saw gone now? The first pass of a run says yes
/// (a note may have been deleted while Yap or the search was off).
/// Remembers `now` for the next.
fn note_gone(seen: &Mutex<Option<HashSet<u64>>>, now: &[u64]) -> bool {
    let now: HashSet<u64> = now.iter().copied().collect();
    let mut seen = seen.lock().unwrap_or_else(|e| e.into_inner());
    let gone = seen.as_ref().is_none_or(|before| before.iter().any(|id| !now.contains(id)));
    *seen = Some(now);
    gone
}

/// Notes changed (or the setting): a pass soon.
pub fn poke() {
    DIRTY.store(true, Ordering::SeqCst);
    WAKE.notify_one();
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
    for event in ["yap-note-changed", "yap-notes-changed", "yap-note-deleted", "yap-meeting-state"] {
        app.listen(event, |_| poke());
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(first_delay()).await;
        let mut last_stamp: Option<u64> = None;
        let mut wait = POLL;
        loop {
            let cfg = crate::config::load();
            if !cfg.search_by_meaning {
                set_phase(Phase::Off, None);
            } else if !embedder::installed() {
                set_phase(Phase::NotInstalled, None);
            } else {
                let stamp = crate::notes::stamp();
                let dirty = DIRTY.swap(false, Ordering::SeqCst);
                if dirty || last_stamp != Some(stamp) {
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
    let notes = crate::notes::all();
    let prepared = prepared(&notes);
    let all: Vec<&Prepared> = prepared.iter().flat_map(|v| v.iter()).collect();
    let keep: HashSet<Key> = all.iter().map(|p| p.key).collect();
    let mut queued = HashSet::new();
    let missing: Vec<&Prepared> = with_store(|s| all.iter().copied().filter(|p| !s.has(&p.key) && queued.insert(p.key)).collect());
    let total = keep.len();
    set_counts(total, total - missing.len(), Some(notes.len()));
    if !missing.is_empty() {
        tracing::info!(passages = total, to_read = missing.len(), notes = notes.len(), "Meeting search: indexing");
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
    // The vectors of passages that are gone: a deleted note's at once (its
    // index goes with it), an edit's once they're a good part of it.
    let ids: Vec<u64> = notes.iter().map(|n| n.id).collect();
    let deleted = note_gone(&SEEN, &ids);
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
        assert!(with_cache(|c| !c.contains_key(&41)));
    }

    #[test]
    fn a_deleted_note_sweeps_the_index_at_once() {
        let seen = Mutex::new(None);
        assert!(note_gone(&seen, &[1, 2, 3]), "a run's first pass sweeps (deleted while off)");
        assert!(!note_gone(&seen, &[1, 2, 3]));
        assert!(!note_gone(&seen, &[1, 2, 3, 4]), "a new note isn't a deletion");
        assert!(note_gone(&seen, &[1, 3, 4]), "note 2 is gone");
        assert!(!note_gone(&seen, &[1, 3, 4]));
    }
}

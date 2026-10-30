//! **Version history of each note** (2026-10-29; Wispr Flow's Scratchpad
//! keeps one, by behaviour): every note keeps its earlier texts — My
//! thoughts and the summary — each version marked with how it came, and any
//! of them comes back.
//!
//! - **Recorded where notes change.** `notes.rs`'s `modify`,
//!   `modify_quietly`, `modify_or_remove`, `update` and `set_enhanced` take
//!   the note's two texts before the change ([`Before`]) and tell
//!   [`noted`] after it, under the store's lock, so the versions follow the
//!   changes in order whoever made them. How a change came is the caller's
//!   ([`by`]: "the changes this thread makes now are the Polish transform"),
//!   else the entry point's own: [`kind::EDITED`] (a change Yap made some
//!   other way: the local API, a speaker named in the summary), or
//!   [`kind::TYPED`] for `modify_or_remove` (the quick note's typing). A
//!   change of ticks alone in the summary is no version.
//! - **Folding** ([`apply`], pure): a save of the same kind (typed, dictated,
//!   edited) and field as the newest version, within [`FOLD_MS`] of that
//!   version's first save, becomes that version. Yap's writing (a summary
//!   run, an Action), a transform, Chat and a restore are always versions of
//!   their own. A text history hasn't got — the note as it was before its
//!   first version, or a change Yap didn't see (an older Yap's) — is kept
//!   first as [`kind::EARLIER`]. [`KEEP`] versions per note, the oldest going
//!   first.
//! - **Stored** in `note-versions/<id>.json` ([`File`]: the note's id and
//!   `createdTs` with its versions), not in notes.json: an older Yap never
//!   opens these files, and notes.json stays the size it was. A file whose
//!   note isn't the one with its id now (a note an older Yap deleted, its id
//!   given to a new note) is ignored and written over. Loaded when a note
//!   first changes or its history is opened, kept for the notes used last
//!   ([`CACHED`]), and written once the store's lock is let go, in order per
//!   file ([`write_pending`]); a failed write is kept and written with the
//!   next one (`saving.rs`: "Couldn't save your notes' history: …").
//!   Deleted with the note ([`forget`]).
//! - **Restore** ([`restore`]): the version's text back in its field — a
//!   summary keeps the tasks ticked in it now (`tasks::carry_ticks`) and is
//!   the person's unless it's Yap's own (`note_edits::apply_summary_edit`) —
//!   as a new version ([`kind::RESTORED`]), so a restore can be undone like
//!   anything else ([`undo_target`]).
//! - **Transcript retention** (`retention.rs`): when a meeting's transcript
//!   goes after its days, so do its summary's earlier versions — Yap's
//!   writing from what was said, like the rolling digests that go with it;
//!   My thoughts' versions stay, as My thoughts does ([`drop_summaries`]).
//!
//! The log says which note, field and kind, and lengths: never the text.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use super::Note;

/// Saves of one kind within this long of a version's first save fold into it.
pub const FOLD_MS: u64 = 10 * 60 * 1000;
/// Versions kept per note.
pub const KEEP: usize = 50;
/// Notes whose history stays in memory once loaded (the ones used last).
const CACHED: usize = 16;

/// How a version came (`Version::kind`).
pub mod kind {
    /// Typed in Notes, the meeting notepad or the quick note.
    pub const TYPED: &str = "typed";
    /// A dictation pasted into the note.
    pub const DICTATED: &str = "dictated";
    /// Yap wrote the summary (the action plan, "Update").
    pub const SUMMARY: &str = "summary";
    /// An Action wrote the summary (`label`: its name).
    pub const ACTION: &str = "action";
    /// A saved transform (`label`: its name).
    pub const TRANSFORM: &str = "transform";
    /// Chat's note tools.
    pub const CHAT: &str = "chat";
    /// An earlier version brought back (`from`: which), or Yap's summary.
    pub const RESTORED: &str = "restored";
    /// Changed some other way (the local API, a speaker named in the summary).
    pub const EDITED: &str = "edited";
    /// What the note said before its history began, or a change Yap didn't see.
    pub const EARLIER: &str = "earlier";
}

/// Kinds whose saves fold together.
fn folds(kind: &str) -> bool {
    matches!(kind, kind::TYPED | kind::DICTATED | kind::EDITED)
}

/// Which of a note's texts a version is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Field {
    /// My thoughts (`content`).
    Notes,
    /// The summary (`enhanced_content`).
    Summary,
}

impl Field {
    /// "notes" | "summary" (the windows' words).
    pub fn parse(s: &str) -> Option<Field> {
        match s {
            "notes" => Some(Field::Notes),
            "summary" => Some(Field::Summary),
            _ => None,
        }
    }

    /// This field's text in `note`.
    pub fn of(self, note: &Note) -> &str {
        match self {
            Field::Notes => &note.content,
            Field::Summary => &note.enhanced_content,
        }
    }
}

/// How the changes a thread makes now came (see [`by`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct How {
    pub kind: &'static str,
    /// A transform's or an Action's name.
    pub label: String,
    /// A restore: the version it brought back.
    pub from: Option<u64>,
}

impl How {
    pub fn new(kind: &'static str) -> Self {
        How { kind, label: String::new(), from: None }
    }

    /// A kind with a name: a transform, an Action, Yap's summary restored.
    pub fn named(kind: &'static str, label: &str) -> Self {
        How { kind, label: label.trim().chars().take(80).collect(), from: None }
    }
}

/// One version of one of a note's texts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Version {
    /// Its number in the note's history (never used twice).
    pub n: u64,
    pub field: Field,
    /// How it came ([`kind`]).
    pub kind: String,
    /// A transform's or an Action's name.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    /// A restore: the version it brought back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<u64>,
    /// Its first save and its last (saves folded into it), unix ms.
    pub since_ms: u64,
    pub ms: u64,
    #[serde(default)]
    pub text: String,
}

/// A note's `note-versions/<id>.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct File {
    /// The note's id and `createdTs`: whose history this is.
    pub note: u64,
    pub created: u64,
    /// The next version's number.
    #[serde(default)]
    pub next: u64,
    #[serde(default)]
    pub versions: Vec<Version>,
}

impl File {
    fn new(note: u64, created: u64) -> Self {
        File { note, created, ..Default::default() }
    }
}

/// One change to `field` of a note, from `before` to `after` at `now` (unix
/// ms), made `how`, kept in `file` (pure; see the module notes). `before_ms`:
/// when `before` was written, for a text history hasn't got. Returns whether
/// the history changed.
pub fn apply(file: &mut File, field: Field, how: &How, before: &str, after: &str, before_ms: u64, now: u64) -> bool {
    if before == after || (field == Field::Summary && same_but_ticks(before, after)) {
        return false;
    }
    let same = |a: &str, b: &str| a == b || (field == Field::Summary && same_but_ticks(a, b));
    let known = file.versions.iter().rev().find(|v| v.field == field).is_some_and(|v| same(&v.text, before));
    if !known && !before.trim().is_empty() {
        let at = before_ms.min(now);
        push(file, Version { n: 0, field, kind: kind::EARLIER.into(), label: String::new(), from: None, since_ms: at, ms: at, text: before.to_string() });
    } else if let Some(last) = file.versions.last_mut() {
        if known
            && last.field == field
            && last.kind == how.kind
            && last.label == how.label
            && folds(how.kind)
            && now.saturating_sub(last.since_ms) < FOLD_MS
        {
            last.text = after.to_string();
            last.ms = now;
            return true;
        }
    }
    push(
        file,
        Version {
            n: 0,
            field,
            kind: how.kind.into(),
            label: how.label.clone(),
            from: how.from,
            since_ms: now,
            ms: now,
            text: after.to_string(),
        },
    );
    true
}

/// Add `v` as the newest version (numbered), the oldest going past [`KEEP`].
fn push(file: &mut File, mut v: Version) {
    let n = file.next.max(file.versions.last().map_or(0, |l| l.n) + 1).max(1);
    v.n = n;
    file.next = n + 1;
    file.versions.push(v);
    if file.versions.len() > KEEP {
        let extra = file.versions.len() - KEEP;
        file.versions.drain(..extra);
    }
}

fn same_but_ticks(a: &str, b: &str) -> bool {
    !a.trim().is_empty() && !b.trim().is_empty() && super::tasks::same_but_ticks(a, b)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---- how a change came ------------------------------------------------------------------

thread_local! {
    static HOW: RefCell<Option<How>> = const { RefCell::new(None) };
}

/// Run `f`; the note changes it makes are recorded as `how` (a transform,
/// an Action, Chat…). The call another part of Yap makes to have its change
/// marked: `versions::by(How::named(kind::ACTION, "Make it longer"), ||
/// note_edits::write_summary(…))`. Changes made outside it are marked by
/// the entry point (see the module notes).
pub fn by<R>(how: How, f: impl FnOnce() -> R) -> R {
    struct Reset(Option<How>);
    impl Drop for Reset {
        fn drop(&mut self) {
            let prev = self.0.take();
            HOW.with(|h| *h.borrow_mut() = prev);
        }
    }
    let _reset = Reset(HOW.with(|h| h.replace(Some(how))));
    f()
}

// ---- recorded under the notes store's lock ------------------------------------------

/// A note's texts before a change (`notes.rs` takes it under the store's lock).
#[derive(Debug, Clone)]
pub struct Before {
    content: String,
    summary: String,
    updated_ts: u64,
}

impl Before {
    pub fn of(note: &Note) -> Self {
        Before { content: note.content.clone(), summary: note.enhanced_content.clone(), updated_ts: note.updated_ts }
    }

    fn text(&self, field: Field) -> &str {
        match field {
            Field::Notes => &self.content,
            Field::Summary => &self.summary,
        }
    }
}

/// The histories loaded, the notes used last first (the notes store keeps
/// it, under its lock).
#[derive(Debug, Default)]
pub struct Cache {
    files: Vec<File>,
}

impl Cache {
    /// `note`'s history, loaded the first time.
    fn file(&mut self, note: &Note) -> &mut File {
        match self.files.iter().position(|f| f.note == note.id && f.created == note.created_ts) {
            Some(0) => {}
            Some(i) => {
                let f = self.files.remove(i);
                self.files.insert(0, f);
            }
            None => {
                self.files.retain(|f| f.note != note.id);
                self.files.insert(0, load(note.id, note.created_ts));
                // The ones used longest ago go, unless their writes are
                // still to come (then they'd load older than they are).
                while self.files.len() > CACHED {
                    match self.files.iter().rposition(|f| !busy(f.note)) {
                        Some(i) if i > 0 => {
                            self.files.remove(i);
                        }
                        _ => break,
                    }
                }
            }
        }
        let file = &mut self.files[0];
        refresh(file);
        file
    }

    /// Every history here whose file couldn't be read, read again
    /// ([`refresh`]). Their paths, read or not.
    fn refresh_unread(&mut self) -> Vec<PathBuf> {
        let mut seen = Vec::new();
        for file in &mut self.files {
            if let Some(path) = path(file.note).filter(|p| SAVER.unread(p)) {
                refresh(file);
                seen.push(path);
            }
        }
        seen
    }
}

/// A history whose file couldn't be read when it loaded (`load`: none in
/// memory, the file never written over): read again and, once it reads, its
/// versions and the ones made meanwhile kept together ([`take_in`]) and
/// queued to be written.
fn refresh(file: &mut File) {
    let Some(path) = path(file.note).filter(|p| SAVER.unread(p)) else { return };
    let Some(disk) = SAVER.reread(&path, crate::shell::saving::json::<File>) else { return };
    if disk.note == file.note && disk.created == file.created {
        take_in(file, disk);
    }
    queue_file(file);
}

/// `disk`'s versions, then the ones `file` has (made while it couldn't be
/// read), numbered on from the file's — a restore's `from` following — and
/// one that only repeats the file's newest of its field left out; the last
/// [`KEEP`] kept (pure).
pub fn take_in(file: &mut File, disk: File) {
    let made = std::mem::replace(file, disk);
    let mut renumbered: HashMap<u64, u64> = HashMap::new();
    for mut v in made.versions {
        let newest = file.versions.iter().rev().find(|d| d.field == v.field);
        if newest.is_some_and(|d| d.text == v.text) {
            if let Some(d) = newest {
                renumbered.insert(v.n, d.n);
            }
            continue;
        }
        let old = v.n;
        v.from = v.from.and_then(|f| renumbered.get(&f).copied());
        push(file, v);
        if let Some(last) = file.versions.last() {
            renumbered.insert(old, last.n);
        }
    }
}

/// Note `at` of `store` changed from `before`: its history follows (see the
/// module notes). `fallback`: how it came when the thread didn't say.
pub(super) fn noted(store: &mut super::Store, at: usize, before: &Before, fallback: &'static str) {
    let Some(note) = store.notes.get(at) else { return };
    let changed: Vec<Field> = [Field::Notes, Field::Summary].into_iter().filter(|f| before.text(*f) != f.of(note)).collect();
    if changed.is_empty() {
        return;
    }
    let how = HOW.with(|h| h.borrow().clone()).unwrap_or_else(|| How::new(fallback));
    let now = now_ms();
    let file = store.versions.file(note);
    let mut any = false;
    for field in changed {
        if apply(file, field, &how, before.text(field), field.of(note), before.updated_ts.saturating_mul(1000), now) {
            any = true;
            tracing::debug!(note_id = note.id, field = ?field, kind = how.kind, chars = field.of(note).chars().count(), "Note version");
        }
    }
    if any {
        queue_file(file);
    }
}

/// Note `id`'s history and the note, under the store's lock: `None` when
/// there's no such note.
fn with_file<R>(id: u64, f: impl FnOnce(&mut File, &Note) -> R) -> Option<R> {
    super::with_store(|s| {
        let at = s.notes.iter().position(|n| n.id == id)?;
        let note = &s.notes[at];
        let file = s.versions.file(note);
        Some(f(file, note))
    })
}

// ---- the files ------------------------------------------------------------------------

/// note-versions' saves (`saving.rs`).
pub static SAVER: crate::shell::saving::Saver =
    crate::shell::saving::Saver::new("note-versions", "your notes' history", save_now);

/// The order the snapshots were taken in (under the notes store's lock).
static SEQ: AtomicU64 = AtomicU64::new(0);

/// What waits to be written.
#[derive(Default)]
struct Queue {
    /// Snapshots taken since the last write (the newest per file; `None`:
    /// the file goes).
    files: HashMap<PathBuf, (u64, Option<String>)>,
    /// Snapshots that couldn't be written (a full disk, a file held):
    /// written with the next snapshot, or Try again.
    failed: HashMap<PathBuf, (u64, String)>,
    /// Files not to be dropped from memory yet: queued, being written, or
    /// failed (loaded from disk they'd be older than they are).
    busy: HashSet<PathBuf>,
    /// A writer is at work: a thread whose snapshot it took waits for it.
    writing: bool,
}

static QUEUE: Mutex<Option<Queue>> = Mutex::new(None);
/// Held while files are written (one writer at a time): the newest
/// snapshot on disk per file, never an older one over it.
static WRITTEN: Mutex<Option<HashMap<PathBuf, u64>>> = Mutex::new(None);

fn with_queue<R>(f: impl FnOnce(&mut Queue) -> R) -> R {
    let mut g = QUEUE.lock().unwrap_or_else(|p| p.into_inner());
    f(g.get_or_insert_with(Queue::default))
}

#[cfg(not(test))]
fn dir() -> Option<PathBuf> {
    // A model evaluation keeps everything in memory (`eval.rs`).
    if crate::ai::eval::active() {
        return None;
    }
    Some(crate::config::data_dir().join("note-versions"))
}

// Unit tests never touch the data folder: a folder a test names, or none.
#[cfg(test)]
thread_local! {
    static TEST_DIR: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

#[cfg(test)]
fn dir() -> Option<PathBuf> {
    TEST_DIR.with(|d| d.borrow().clone())
}

/// Where note `id`'s history is kept (`None`: nowhere — a model evaluation,
/// a unit test).
pub fn path(id: u64) -> Option<PathBuf> {
    dir().map(|d| d.join(format!("{id}.json")))
}

fn busy(id: u64) -> bool {
    path(id).is_some_and(|p| with_queue(|q| q.busy.contains(&p)))
}

/// Note `id`'s history as last written or queued (under the notes store's lock).
fn load(id: u64, created: u64) -> File {
    let Some(path) = path(id) else { return File::new(id, created) };
    let queued = with_queue(|q| {
        q.files
            .get(&path)
            .map(|(_, json)| json.clone())
            .or_else(|| q.failed.get(&path).map(|(_, json)| Some(json.clone())))
    });
    let file = match queued {
        Some(None) => return File::new(id, created),
        Some(Some(json)) => serde_json::from_str::<File>(&json).ok(),
        // Set aside if it isn't a history; one Yap couldn't read starts
        // empty here and is never written over until it reads ([`refresh`]).
        None => match SAVER.read(&path, crate::shell::saving::json::<File>) {
            crate::shell::saving::Found::Read(file) => Some(file),
            _ => None,
        },
    };
    match file {
        // Another note's (one deleted by an older Yap, its id given again): not this one's.
        Some(file) if file.note == id && file.created == created => file,
        _ => File::new(id, created),
    }
}

/// Queue `file` to be written once the store's lock is let go.
fn queue_file(file: &File) {
    let Some(path) = path(file.note) else { return };
    match serde_json::to_string(file) {
        Ok(json) => queue(path, Some(json)),
        Err(e) => tracing::error!(note_id = file.note, "couldn't serialise a note's history ({e})"),
    }
}

fn queue(path: PathBuf, json: Option<String>) {
    let seq = SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    with_queue(|q| {
        q.busy.insert(path.clone());
        q.files.insert(path, (seq, json));
    });
}

/// Write what's queued (`notes.rs` calls it once the store's lock is let go):
/// each file's newest snapshot, never one older than what's on disk, and
/// with them any that couldn't be written before (`retry`: those alone, Try
/// again). Returns once the snapshots this thread queued are written, by it
/// or by the writer at work.
pub(super) fn write_pending() -> Result<(), crate::shell::saving::SaveError> {
    write(false)
}

fn write(retry: bool) -> Result<(), crate::shell::saving::SaveError> {
    // Nearly every change to the notes store comes here: nothing to write, no wait.
    if with_queue(|q| q.files.is_empty() && !q.writing && !(retry && !q.failed.is_empty())) {
        return SAVER.status();
    }
    let mut written = WRITTEN.lock().unwrap_or_else(|p| p.into_inner());
    let written = written.get_or_insert_with(HashMap::new);
    let jobs: Vec<(PathBuf, u64, Option<String>)> = with_queue(|q| {
        if q.files.is_empty() && !retry {
            return Vec::new();
        }
        q.writing = true;
        let mut jobs: HashMap<PathBuf, (u64, Option<String>)> =
            q.failed.drain().map(|(p, (seq, json))| (p, (seq, Some(json)))).collect();
        for (p, (seq, json)) in q.files.drain() {
            if jobs.get(&p).is_none_or(|(s, _)| *s < seq) {
                jobs.insert(p, (seq, json));
            }
        }
        jobs.into_iter().map(|(p, (seq, json))| (p, seq, json)).collect()
    });
    if jobs.is_empty() {
        return SAVER.status();
    }
    let mut files = Vec::new();
    let mut seqs = Vec::new();
    let mut done: Vec<(PathBuf, u64)> = Vec::new();
    for (path, seq, json) in jobs {
        if written.get(&path).is_some_and(|&w| w >= seq) {
            done.push((path, seq));
            continue;
        }
        match json {
            Some(json) => {
                files.push((path, json));
                seqs.push(seq);
            }
            None => {
                if let Err(e) = std::fs::remove_file(&path) {
                    if e.kind() != std::io::ErrorKind::NotFound {
                        tracing::warn!("couldn't delete a note's history ({e})");
                    }
                }
                written.insert(path.clone(), seq);
                done.push((path, seq));
            }
        }
    }
    let result = if files.is_empty() {
        SAVER.status()
    } else {
        let (failed, result) = SAVER.write_each(&files);
        for (i, ((path, json), seq)) in files.into_iter().zip(seqs).enumerate() {
            if failed.contains(&i) {
                // Written with the next snapshot, or Try again.
                with_queue(|q| {
                    q.failed.insert(path, (seq, json));
                });
            } else {
                written.insert(path.clone(), seq);
                done.push((path, seq));
            }
        }
        result
    };
    with_queue(|q| {
        q.writing = false;
        for (path, _) in done {
            if !q.files.contains_key(&path) && !q.failed.contains_key(&path) {
                q.busy.remove(&path);
            }
        }
    });
    result
}

/// Try again (`saving.rs`): what couldn't be written, written now — the
/// files Yap couldn't read read again first (the histories in memory merged
/// with theirs; the others only read, nothing of them in memory).
fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    if SAVER.any_unread() {
        let cached = super::with_store(|s| s.versions.refresh_unread());
        for path in SAVER.unread_paths().into_iter().filter(|p| !cached.contains(p)) {
            let _ = SAVER.reread(&path, crate::shell::saving::json::<File>);
        }
    }
    let written = write(true);
    if written.is_err() && !SAVER.any_unread() && with_queue(|q| q.failed.is_empty() && q.files.is_empty()) {
        // Only files read again, nothing left to write.
        SAVER.nothing_to_save();
        return Ok(());
    }
    written
}

// ---- reading, restoring, forgetting ---------------------------------------------------

/// A version as the windows list it: whether it's the text its field holds now.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listed {
    #[serde(flatten)]
    pub version: Version,
    pub current: bool,
}

/// Note `id`'s versions, newest first.
pub fn list(id: u64) -> Result<Vec<Listed>, String> {
    with_file(id, |file, note| listed(file, note)).ok_or_else(|| "Note not found".to_string())
}

/// `file`'s versions, newest first, the newest of each field marked current
/// when it's what `note` holds (pure).
pub fn listed(file: &File, note: &Note) -> Vec<Listed> {
    let mut seen = HashSet::new();
    file.versions
        .iter()
        .rev()
        .map(|v| {
            let newest = seen.insert(v.field);
            let now = v.field.of(note);
            let current = newest && (v.text == now || (v.field == Field::Summary && same_but_ticks(&v.text, now)));
            Listed { version: v.clone(), current }
        })
        .collect()
}

/// The version to go back to after the newest change to `field` (the toast's
/// Undo after a transform or a restore): the one before the newest.
pub fn undo_target(id: u64, field: Field) -> Option<u64> {
    with_file(id, |file, _| file.versions.iter().rev().filter(|v| v.field == field).nth(1).map(|v| v.n)).flatten()
}

/// What [`restore`] did.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Restored {
    pub field: Field,
    /// The field's text now.
    pub text: String,
    /// The version the note had before (Undo brings it back).
    pub previous: Option<u64>,
}

/// Bring version `n` of note `id` back (see the module notes).
pub fn restore(id: u64, n: u64) -> Result<Restored, String> {
    let v = with_file(id, |file, _| file.versions.iter().find(|v| v.n == n).cloned())
        .ok_or("Note not found")?
        .ok_or("That version isn't kept any more")?;
    let now = now_ms();
    let how = How { kind: kind::RESTORED, label: String::new(), from: Some(n) };
    let text = by(how, || {
        super::modify(id, |note| {
            let changed = match v.field {
                Field::Notes => {
                    let changed = note.content != v.text;
                    note.content = v.text.clone();
                    changed
                }
                Field::Summary => {
                    let text = if v.text.trim().is_empty() || note.enhanced_content.trim().is_empty() {
                        v.text.clone()
                    } else {
                        super::tasks::carry_ticks(&note.enhanced_content, &v.text)
                    };
                    super::note_edits::apply_summary_edit(note, &text, now)
                }
            };
            Ok((v.field.of(note).to_string(), changed))
        })
    })?;
    tracing::info!(note_id = id, n, field = ?v.field, kind = %v.kind, chars = text.chars().count(), "Note version restored");
    Ok(Restored { field: v.field, text, previous: undo_target(id, v.field) })
}

/// Note `id` is gone: its history goes with it (in memory, and its file once
/// the store's lock is let go).
pub fn forget(id: u64) {
    super::with_store(|s| {
        s.versions.files.retain(|f| f.note != id);
        if let Some(path) = path(id) {
            queue(path, None);
        }
    });
}

/// Reset app (`privacy/reset.rs`): every note's history goes, in memory and
/// on disk (each file in `note-versions/`, and any set aside there). Returns
/// how many files went.
pub fn reset() -> usize {
    super::with_store(|s| {
        s.versions.files.clear();
        let Some(dir) = dir() else { return 0 };
        // Snapshots still to be written go too: each file's deletion comes after them.
        let queued: Vec<PathBuf> = with_queue(|q| q.files.keys().chain(q.failed.keys()).cloned().collect());
        with_queue(|q| q.failed.clear());
        let mut n = 0;
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            if p.extension().is_some_and(|x| x == "json") {
                queue(p, None);
                n += 1;
            } else {
                // A file set aside (`<id>.json.corrupt-…`): the same content.
                let _ = std::fs::remove_file(&p);
            }
        }
        for p in queued {
            queue(p, None);
        }
        n
    })
}

/// Every note's history for Download your data (`privacy/export.rs`):
/// `{note, title, versions}` per note that has one, its versions newest
/// first.
pub fn export_all() -> Vec<serde_json::Value> {
    super::all()
        .iter()
        .filter_map(|n| {
            let versions = list(n.id).ok().filter(|v| !v.is_empty())?;
            Some(serde_json::json!({ "note": n.id, "title": n.title, "versions": versions }))
        })
        .collect()
}

/// A meeting's transcript went after its days (`retention.rs`): its
/// summary's earlier versions go too (see the module notes). The summary
/// as it is now stays in the note.
pub fn drop_summaries(id: u64) {
    let dropped = with_file(id, |file, _| {
        let before = file.versions.len();
        file.versions.retain(|v| v.field != Field::Summary);
        let dropped = before - file.versions.len();
        if dropped > 0 {
            queue_file(file);
        }
        dropped
    })
    .unwrap_or(0);
    if dropped > 0 {
        tracing::info!(note_id = id, dropped, "Transcript retention: the summary's earlier versions deleted");
    }
}

// ---- commands ---------------------------------------------------------------------------

/// Note `id`'s version history, newest first (the Notes view's Version
/// history).
#[tauri::command]
pub fn note_versions(id: u64) -> Result<Vec<Listed>, String> {
    list(id)
}

/// Bring back version `n` of note `id` (Restore, and the toast's Undo).
/// Every window follows (`yap-note-changed`, `origin`: who asked); the
/// restore is a version itself.
#[tauri::command]
pub fn note_version_restore(app: AppHandle, id: u64, n: u64, origin: Option<String>) -> Result<Restored, String> {
    let restored = restore(id, n)?;
    let origin = origin.as_deref().unwrap_or("history");
    match restored.field {
        Field::Notes => super::commands::note_changed(&app, id, origin),
        Field::Summary => super::note_edits::summary_changed(&app, id, origin),
    }
    super::saved()?;
    Ok(restored)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: u64 = 60 * 1000;

    fn typed() -> How {
        How::new(kind::TYPED)
    }

    fn texts(file: &File) -> Vec<(String, String)> {
        file.versions.iter().map(|v| (v.kind.clone(), v.text.clone())).collect()
    }

    fn version(n: u64, kind: &str, from: Option<u64>, text: &str) -> Version {
        Version { n, field: Field::Notes, kind: kind.into(), label: String::new(), from, since_ms: n, ms: n, text: text.into() }
    }

    /// A history Yap couldn't read when the note was changed: the file's
    /// versions, then the ones made meanwhile, numbered on from the file's
    /// (a restore's `from` following), the "earlier" one that only repeats
    /// the file's newest left out.
    #[test]
    fn a_history_read_late_keeps_the_files_versions_and_the_new_ones() {
        let mut disk = File::new(7, 70);
        push(&mut disk, version(0, kind::TYPED, None, "Plan the beds"));
        push(&mut disk, version(0, kind::TYPED, None, "Plan the beds and paths"));
        let mut made = File::new(7, 70);
        push(&mut made, version(0, kind::EARLIER, None, "Plan the beds and paths"));
        push(&mut made, version(0, kind::TYPED, None, "Plan the beds, paths and a pond"));
        push(&mut made, version(0, kind::RESTORED, Some(1), "Plan the beds and paths"));
        take_in(&mut made, disk);
        let got: Vec<(u64, &str, Option<u64>)> = made.versions.iter().map(|v| (v.n, v.kind.as_str(), v.from)).collect();
        assert_eq!(got, [(1, kind::TYPED, None), (2, kind::TYPED, None), (3, kind::TYPED, None), (4, kind::RESTORED, Some(2))]);
        assert_eq!(made.next, 5);
        assert_eq!(made.versions[2].text, "Plan the beds, paths and a pond");
    }

    #[test]
    fn saves_of_one_kind_fold_for_ten_minutes_and_ai_writing_never_folds() {
        let mut f = File::new(1, 1);
        assert!(apply(&mut f, Field::Notes, &typed(), "", "Buy", 0, 0));
        assert!(apply(&mut f, Field::Notes, &typed(), "Buy", "Buy milk", 0, 3 * MIN));
        assert!(apply(&mut f, Field::Notes, &typed(), "Buy milk", "Buy milk and eggs", 0, 9 * MIN));
        assert_eq!(texts(&f), [("typed".into(), "Buy milk and eggs".into())]);
        assert_eq!((f.versions[0].since_ms, f.versions[0].ms), (0, 9 * MIN));
        // Ten minutes after its first save: a version of its own.
        apply(&mut f, Field::Notes, &typed(), "Buy milk and eggs", "Buy milk, eggs", 0, 10 * MIN);
        assert_eq!(f.versions.len(), 2);
        // A dictation is another kind; a transform never folds, even into
        // itself.
        apply(&mut f, Field::Notes, &How::new(kind::DICTATED), "Buy milk, eggs", "Buy milk, eggs and bread", 0, 11 * MIN);
        let polish = How::named(kind::TRANSFORM, "Polish");
        apply(&mut f, Field::Notes, &polish, "Buy milk, eggs and bread", "Buy milk, eggs and bread.", 0, 11 * MIN);
        apply(&mut f, Field::Notes, &polish, "Buy milk, eggs and bread.", "Milk, eggs, bread.", 0, 11 * MIN);
        let kinds: Vec<&str> = f.versions.iter().map(|v| v.kind.as_str()).collect();
        assert_eq!(kinds, ["typed", "typed", "dictated", "transform", "transform"]);
        assert_eq!(f.versions[4].label, "Polish");
        // Typing after them: a new version, never folded into an older one.
        apply(&mut f, Field::Notes, &typed(), "Milk, eggs, bread.", "Milk, eggs, bread. Soap", 0, 12 * MIN);
        assert_eq!(f.versions.len(), 6);
        // Numbers count up.
        assert_eq!(f.versions.iter().map(|v| v.n).collect::<Vec<_>>(), [1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn the_two_texts_keep_their_own_versions_and_a_tick_is_no_version() {
        let mut f = File::new(1, 1);
        apply(&mut f, Field::Notes, &typed(), "", "Agenda", 0, 0);
        apply(&mut f, Field::Summary, &How::new(kind::SUMMARY), "", "## Tasks\n- [ ] Book the room", 0, MIN);
        // Typing again within the ten minutes: the newest version is the
        // summary's, so it's a version of its own.
        apply(&mut f, Field::Notes, &typed(), "Agenda", "Agenda: budget", 0, 2 * MIN);
        assert_eq!(f.versions.len(), 3);
        // A tick changes nothing in the history.
        assert!(!apply(&mut f, Field::Summary, &typed(), "## Tasks\n- [ ] Book the room", "## Tasks\n- [x] Book the room", 0, 3 * MIN));
        // A summary edit after a tick: no "earlier" version for the tick.
        apply(&mut f, Field::Summary, &typed(), "## Tasks\n- [x] Book the room", "## Tasks\n- [x] Book the big room", 0, 4 * MIN);
        let kinds: Vec<&str> = f.versions.iter().map(|v| v.kind.as_str()).collect();
        assert_eq!(kinds, ["typed", "summary", "typed", "typed"]);
    }

    #[test]
    fn a_text_history_hasnt_got_is_kept_first_as_earlier() {
        // A note from before version history: what it said comes first.
        let mut f = File::new(1, 1);
        apply(&mut f, Field::Notes, &typed(), "Old words", "Old words, new", 5_000, MIN);
        assert_eq!(texts(&f), [("earlier".into(), "Old words".into()), ("typed".into(), "Old words, new".into())]);
        assert_eq!(f.versions[0].ms, 5_000);
        // Changed by an older Yap meanwhile: that text is kept before this
        // change, which doesn't fold into the version before it.
        apply(&mut f, Field::Notes, &typed(), "Changed elsewhere", "Changed elsewhere, then here", 2 * MIN, 2 * MIN);
        let kinds: Vec<&str> = f.versions.iter().map(|v| v.kind.as_str()).collect();
        assert_eq!(kinds, ["earlier", "typed", "earlier", "typed"]);
        // An empty note has nothing to keep.
        let mut g = File::new(2, 1);
        apply(&mut g, Field::Notes, &typed(), "", "First words", 0, 0);
        assert_eq!(g.versions.len(), 1);
    }

    #[test]
    fn fifty_versions_are_kept_the_oldest_going_first() {
        let mut f = File::new(1, 1);
        let mut text = String::new();
        for i in 0..(KEEP as u64 + 7) {
            let next = format!("{text} {i}");
            apply(&mut f, Field::Notes, &How::named(kind::TRANSFORM, "Polish"), &text, &next, 0, i);
            text = next;
        }
        assert_eq!(f.versions.len(), KEEP);
        assert_eq!(f.versions[0].n, 8, "the first seven went");
        assert_eq!(f.versions.last().unwrap().n, KEEP as u64 + 7);
        assert_eq!(f.versions.last().unwrap().text, text);
        // A number is never given twice, even once the newest went.
        f.versions.pop();
        apply(&mut f, Field::Notes, &typed(), "x", "y", 0, 0);
        assert_eq!(f.versions.last().unwrap().n, KEEP as u64 + 9);
    }

    /// A unit test's own folder for the files.
    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("yap-note-versions-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        TEST_DIR.with(|t| *t.borrow_mut() = Some(d.clone()));
        d
    }

    #[test]
    fn a_notes_history_is_recorded_restored_and_deleted_with_it() {
        let dir = scratch("restore");
        let id = super::super::create("Shopping", "", "manual", "").id;
        super::super::update(id, None, Some("milk".into()), None, None).unwrap();
        by(How::named(kind::TRANSFORM, "Polish"), || super::super::update(id, None, Some("Milk.".into()), None, None)).unwrap();
        let listed = list(id).unwrap();
        let kinds: Vec<(&str, bool)> = listed.iter().map(|l| (l.version.kind.as_str(), l.current)).collect();
        // `update` without a kind: changed some other way.
        assert_eq!(kinds, [("transform", true), ("edited", false)]);
        assert_eq!(undo_target(id, Field::Notes), Some(listed[1].version.n));
        let file = dir.join(format!("{id}.json"));
        let on_disk: File = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(on_disk.versions.len(), 2);
        assert_eq!(on_disk.created, super::super::get(id).unwrap().created_ts);
        // Never in notes.json: an older Yap saving it keeps the note and
        // never sees (or drops) its history.
        assert!(!super::super::store_json().contains("\"versions\""));

        // Restore the first: the note says it again, as a version of its own.
        let r = restore(id, listed[1].version.n).unwrap();
        assert_eq!((r.field, r.text.as_str()), (Field::Notes, "milk"));
        assert_eq!(super::super::get(id).unwrap().content, "milk");
        let after = list(id).unwrap();
        assert_eq!(after[0].version.kind, "restored");
        assert_eq!(after[0].version.from, Some(listed[1].version.n));
        // Undo puts back what it replaced.
        assert_eq!(r.previous, Some(listed[0].version.n));
        restore(id, r.previous.unwrap()).unwrap();
        assert_eq!(super::super::get(id).unwrap().content, "Milk.");

        // Deleted with the note: in memory and on disk.
        super::super::delete(id);
        assert!(!file.exists(), "its file goes");
        assert!(with_queue(|q| !q.busy.contains(&file)));
        assert!(list(id).is_err());
        // A new note given its id starts with no history.
        let again = super::super::create("Other", "", "manual", "").id;
        assert_eq!(again, id);
        assert!(list(again).unwrap().is_empty());
        TEST_DIR.with(|t| *t.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reset_app_takes_every_notes_history() {
        let dir = scratch("reset");
        let id = super::super::create("Plans", "", "manual", "").id;
        super::super::update(id, None, Some("Paint the shed".into()), None, None).unwrap();
        let file = dir.join(format!("{id}.json"));
        assert!(file.exists());
        let set_aside = dir.join("9.json.corrupt-1790000000");
        std::fs::write(&set_aside, "{").unwrap();
        assert_eq!(reset(), 1);
        assert!(!file.exists() && !set_aside.exists());
        assert!(list(id).unwrap().is_empty(), "nothing in memory either");
        TEST_DIR.with(|t| *t.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_left_by_another_note_with_the_id_is_not_this_ones() {
        let dir = scratch("stale");
        let id = super::super::create("Mine", "", "manual", "").id;
        // A deleted note's history an older Yap left behind (another createdTs).
        std::fs::create_dir_all(&dir).unwrap();
        let stale = File {
            note: id,
            created: 1,
            next: 2,
            versions: vec![Version { n: 1, field: Field::Notes, kind: "typed".into(), label: String::new(), from: None, since_ms: 1, ms: 1, text: "someone else's words".into() }],
        };
        std::fs::write(dir.join(format!("{id}.json")), serde_json::to_string(&stale).unwrap()).unwrap();
        assert!(list(id).unwrap().is_empty());
        super::super::update(id, None, Some("my words".into()), None, None).unwrap();
        let kept: File = serde_json::from_str(&std::fs::read_to_string(dir.join(format!("{id}.json"))).unwrap()).unwrap();
        assert_eq!(kept.versions.len(), 1);
        assert_eq!(kept.versions[0].text, "my words");
        TEST_DIR.with(|t| *t.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_summary_restored_keeps_its_ticks_and_retention_takes_the_summarys_versions() {
        let id = super::super::create("Sync", "notes", "manual", "").id;
        by(How::new(kind::SUMMARY), || super::super::note_edits::write_summary(id, "- [ ] Book the room\n- [ ] Send the deck", "h", 1, 0, false)).unwrap();
        super::super::modify(id, |n| Ok(((), super::super::note_edits::apply_summary_edit(n, "- [ ] Book the room", 2)))).unwrap();
        super::super::modify(id, |n| {
            n.enhanced_content = "- [x] Book the room".into();
            Ok(((), true))
        })
        .unwrap();
        let first = list(id).unwrap().last().unwrap().version.n;
        restore(id, first).unwrap();
        let n = super::super::get(id).unwrap();
        assert_eq!(n.enhanced_content, "- [x] Book the room\n- [ ] Send the deck", "Yap's words, the tick kept");
        assert_eq!(n.summary_edited_ms, 0, "Yap's own summary again");

        super::super::update(id, None, Some("notes, more".into()), None, None).unwrap();
        drop_summaries(id);
        let left = list(id).unwrap();
        assert!(left.iter().all(|l| l.version.field == Field::Notes), "My thoughts' versions stay");
        assert_eq!(left.len(), 2);
    }
}

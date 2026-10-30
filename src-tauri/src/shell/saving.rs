//! **Saving Yap's JSON stores, and saying so when a save fails** (code
//! review 2026-10-23 #7). Until then every store's write only logged a
//! warning, and the caller said it saved: a full disk, or an antivirus or a
//! backup tool holding `notes.json` during the rename, left everything after
//! it in memory only, and a crash lost notes, transcripts and chats the page
//! had shown as saved.
//!
//! Each store has a [`Saver`] (`notes::SAVER`, `history::SAVER`…) and writes
//! through it ([`Saver::write`]: `config::atomic_write`, its folder made
//! first). A write that fails:
//!
//! - **returns the error** ([`SaveError`]: "Couldn't save your note: the disk
//!   is full."), and the store's commands pass it on ([`Saver::check`], each
//!   store's `saved()`): the page shows it with **Try again**
//!   (`saves_retry` → [`retry_all`]; `src/lib/notices/saveErrors.js`);
//! - **never loses what the store holds**: it stays in memory, marked
//!   unsaved, and the next save of that store writes all of it — the retry
//!   (each store writes its whole state every time). Quitting tries once
//!   more ([`flush`], `lib.rs shutdown_cleanup`);
//! - **is said once** when no command said it (history after a dictation,
//!   memory learning, calendar sync, usage counts): [`TELL_AFTER`] later, one
//!   alert through the notice policy (`alert.rs` → `notice.rs`, kind
//!   [`KIND`]: never over a dictation, never dropped, not twice in ten
//!   minutes) names the first store and says if others failed too, with
//!   **Try again**. Not again for that store until a save of it has worked.
//!   A cache (the model lists, the speech engine's timings) is only logged
//!   ([`Saver::quiet`]).
//!
//! **Reading them, and never writing over one Yap couldn't read** (review
//! 2026-10-30). Until then every store took any read error at load — another
//! program holding the file (an antivirus, a backup tool, a sync client), a
//! cloud file offline, Windows refusing — as "no file" and started empty,
//! and its next save wrote that over the real file: notes.json lost every
//! note the moment notes seeded its folders. Every store now reads through
//! [`Saver::read`] / [`Saver::load`]:
//!
//! - **no file**: the store starts empty, as before;
//! - **not the store's shape** (torn, not JSON, not UTF-8): set aside as
//!   `<name>.corrupt-<secs>` (`config::quarantine_corrupt`), kept, and the
//!   store starts empty;
//! - **a file Yap couldn't read** (after a few tries over ~200 ms while
//!   another program holds it), or a corrupt one it couldn't set aside: the
//!   store starts empty **in memory** and keeps working there, but every
//!   write of that file is refused ([`Reason::Unread`]) — never written over.
//!   Said once, like a failed save ("Yap couldn't read your notes", with Try
//!   again). The file is read again a moment later — 2 s, 5 s, 15 s, 30 s,
//!   then every minute ([`reread_later`]: each store's own `save_now`, which
//!   reads it first) — and whenever the store is used ([`Saver::refresh`], at
//!   most every [`REREAD_GAP`]); once it reads, what's there and what was
//!   added in memory meanwhile are put together ([`merge_json`]: nothing in
//!   the file is lost) and saved. New items' ids while it couldn't be read
//!   start past the time it was found so ([`Saver::next_id`]), never one in
//!   the file. Quitting with it still unread keeps what was added beside it,
//!   as `<name>.unsaved-<secs>.json` ([`flush`]).
//!
//! The log says which store and why (the error's kind), never what it holds.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use tauri::AppHandle;

/// The alert's kind (its rule in `notice.rs`, the bar card's id).
pub const KIND: &str = "saveFailed";
/// The alert's **Try again** ([`crate::notices::alert::act`]).
pub const RETRY: &str = "retrySaves";
/// How long a failed save waits for a command to say it before the notice
/// does.
pub const TELL_AFTER: Duration = Duration::from_millis(1_500);

/// Why a save failed, as the person is told it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    DiskFull,
    /// Another program holds the file: a sharing or lock violation, or
    /// (what Windows says when a file another program has open is renamed
    /// over) access denied on a file that isn't read-only.
    Locked,
    /// The file is read-only.
    ReadOnly,
    /// Access denied otherwise (the folder).
    NotAllowed,
    /// Yap couldn't read the store's file when it loaded it, so it never
    /// writes over it until a read works ([`Saver::read`]).
    Unread,
    Other,
}

impl Reason {
    pub fn of(e: &io::Error) -> Self {
        // ERROR_SHARING_VIOLATION / ERROR_LOCK_VIOLATION, ERROR_HANDLE_DISK_FULL
        // / ERROR_DISK_FULL.
        #[cfg(windows)]
        match e.raw_os_error() {
            Some(32 | 33) => return Self::Locked,
            Some(39 | 112) => return Self::DiskFull,
            _ => {}
        }
        match e.kind() {
            io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded => Self::DiskFull,
            io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem => Self::NotAllowed,
            io::ErrorKind::ResourceBusy => Self::Locked,
            _ => Self::Other,
        }
    }

    /// [`Self::of`] a write of `path`: access denied is told apart by the
    /// file (read-only, there and held by another program, or not there:
    /// the folder).
    pub fn at(e: &io::Error, path: &Path) -> Self {
        match Self::of(e) {
            Self::NotAllowed => match std::fs::metadata(path) {
                Ok(m) if m.permissions().readonly() => Self::ReadOnly,
                Ok(_) => Self::Locked,
                Err(_) => Self::NotAllowed,
            },
            reason => reason,
        }
    }

    /// Plain words, after "Couldn't save your note: ".
    pub fn words(self) -> &'static str {
        match self {
            Self::DiskFull => "the disk is full",
            Self::Locked => "another program has the file locked",
            Self::ReadOnly => "the file is read-only",
            Self::NotAllowed => "Windows wouldn't let Yap write to its folder",
            Self::Unread => "Yap couldn't read the file (another program may be using it), so nothing was changed",
            Self::Other => "Windows couldn't write the file",
        }
    }
}

/// A save that didn't happen: what, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveError {
    /// "your note", "your dictation history"…
    pub what: &'static str,
    pub reason: Reason,
    /// The whole store, as a read of it is named ("your notes").
    pub whole: &'static str,
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Couldn't save {}: {}.", self.what, self.reason.words())
    }
}

impl std::error::Error for SaveError {}

/// A command's error, as the page shows it (`saveErrors.js` knows it by its
/// start, "Couldn't save ").
impl From<SaveError> for String {
    fn from(e: SaveError) -> Self {
        e.to_string()
    }
}

/// One store's saves.
#[derive(Debug)]
pub struct Saver {
    /// The store in the log and the test hook: "notes", "history"…
    pub id: &'static str,
    /// What the person calls it, after "Couldn't save ".
    pub what: &'static str,
    /// The whole store, after "Yap couldn't read " ([`Self::reads_as`]; else
    /// `what`).
    pub whole: &'static str,
    /// Said when a save fails by itself (a cache: only logged).
    tells: bool,
    /// Write everything the store holds again (the retry). A store whose
    /// file couldn't be read reads it again first ([`Saver::refresh`]).
    save_now: fn() -> Result<(), SaveError>,
}

/// A store whose last save failed.
#[derive(Debug, Clone, Copy)]
struct Failure {
    saver: &'static Saver,
    reason: Reason,
    /// Said already (by a command's error or the notice).
    told: bool,
}

// The stores whose last save failed. A unit test thread has a list of its
// own, so tests running side by side never see each other's failures.
#[cfg(not(test))]
static FAILED: std::sync::Mutex<Vec<Failure>> = std::sync::Mutex::new(Vec::new());

#[cfg(test)]
thread_local! {
    static FAILED: std::cell::RefCell<Vec<Failure>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn with_failed<R>(f: impl FnOnce(&mut Vec<Failure>) -> R) -> R {
    #[cfg(not(test))]
    {
        f(&mut FAILED.lock().unwrap_or_else(|p| p.into_inner()))
    }
    #[cfg(test)]
    {
        FAILED.with(|l| f(&mut l.borrow_mut()))
    }
}

impl Saver {
    /// A store of the person's (notes, history, memory…): a failed save is
    /// said.
    pub const fn new(id: &'static str, what: &'static str, save_now: fn() -> Result<(), SaveError>) -> Self {
        Self { id, what, whole: what, tells: true, save_now }
    }

    /// A cache Yap rebuilds by itself: a failed save is only logged (and
    /// still tried again with the next save).
    pub const fn quiet(id: &'static str, what: &'static str, save_now: fn() -> Result<(), SaveError>) -> Self {
        Self { id, what, whole: what, tells: false, save_now }
    }

    /// The whole store's name when it's other than `what` ("your notes" for
    /// "your note"): "Yap couldn't read your notes".
    pub const fn reads_as(self, whole: &'static str) -> Self {
        Self { whole, ..self }
    }

    fn error(&'static self, reason: Reason) -> SaveError {
        SaveError { what: self.what, reason, whole: self.whole }
    }

    /// Write `json` (everything the store holds) to `path`, atomically. A
    /// failure leaves the store unsaved, its data in memory, until a save
    /// of it works; said once ([`Self::check`], else the notice). Refused
    /// while `path` is a file Yap couldn't read ([`Self::read`]).
    pub fn write(&'static self, path: &Path, json: &str) -> Result<(), SaveError> {
        if self.refused(path, json) {
            return self.settle(Err((Reason::Unread, io::Error::other("the file couldn't be read"))));
        }
        let written = write_file(self.id, path, json);
        self.settle(written.map_err(|e| (Reason::at(&e, path), e)))
    }

    /// [`Self::write`] of `value` as JSON (`pretty`: indented).
    pub fn write_json<T: serde::Serialize + ?Sized>(&'static self, path: &Path, value: &T, pretty: bool) -> Result<(), SaveError> {
        let json = if pretty { serde_json::to_string_pretty(value) } else { serde_json::to_string(value) };
        match json {
            Ok(json) => self.write(path, &json),
            Err(e) => {
                // Yap's own types always serialise; say so in the log if not.
                tracing::error!(store = self.id, "couldn't serialise the store ({e})");
                self.settle(Err((Reason::Other, io::Error::other("serialise"))))
            }
        }
    }

    /// Record how a write went.
    fn settle(&'static self, written: Result<(), (Reason, io::Error)>) -> Result<(), SaveError> {
        match written {
            Ok(()) => {
                let was = with_failed(|list| {
                    let before = list.len();
                    list.retain(|f| !std::ptr::eq(f.saver, self));
                    before != list.len()
                });
                if was {
                    tracing::info!(store = self.id, "saved again after a failed save");
                }
                Ok(())
            }
            Err((reason, e)) => {
                let first = self.failed(reason);
                if first {
                    tracing::warn!(
                        store = self.id,
                        ?reason,
                        os_error = ?e.raw_os_error(),
                        "couldn't save; kept in memory and tried again with the next save ({e})"
                    );
                    if self.tells {
                        tell_later();
                    }
                } else {
                    tracing::debug!(store = self.id, ?reason, "still couldn't save");
                }
                Err(self.error(reason))
            }
        }
    }

    /// Mark the store unsaved for `reason`; whether it's new (to be said).
    fn failed(&'static self, reason: Reason) -> bool {
        with_failed(|list| match list.iter_mut().find(|f| std::ptr::eq(f.saver, self)) {
            Some(f) => {
                f.reason = reason;
                false
            }
            None => {
                list.push(Failure { saver: self, reason, told: false });
                true
            }
        })
    }

    /// `Ok` when everything the store holds is on disk; else why not. The
    /// caller says it (a command's error, which the page shows with Try
    /// again), so the notice for saves that failed by themselves doesn't.
    pub fn check(&'static self) -> Result<(), SaveError> {
        with_failed(|list| match list.iter_mut().find(|f| std::ptr::eq(f.saver, self)) {
            Some(f) => {
                f.told = true;
                Err(self.error(f.reason))
            }
            None => Ok(()),
        })
    }

    /// [`Self::check`] without saying it: for an answer to an AI model or an
    /// AI app (the chat's note tools, the local API), so it never claims a
    /// save that didn't happen, while Yap still says it itself.
    pub fn status(&'static self) -> Result<(), SaveError> {
        with_failed(|list| match list.iter().find(|f| std::ptr::eq(f.saver, self)) {
            Some(f) => Err(self.error(f.reason)),
            None => Ok(()),
        })
    }

    /// A store kept in several files (meeting audio's notes, one per
    /// recording): write each of `files` (path, JSON), and settle once — the
    /// store is unsaved while any of them couldn't be written (the first
    /// failure says why). The caller keeps the ones that failed for the next
    /// try. Returns which failed (their indexes).
    pub fn write_each(&'static self, files: &[(std::path::PathBuf, String)]) -> (Vec<usize>, Result<(), SaveError>) {
        let mut failed = Vec::new();
        let mut first: Option<(Reason, io::Error)> = None;
        for (i, (path, json)) in files.iter().enumerate() {
            let written = if self.refused(path, json) {
                Err((Reason::Unread, io::Error::other("the file couldn't be read")))
            } else {
                write_file(self.id, path, json).map_err(|e| (Reason::at(&e, path), e))
            };
            if let Err(e) = written {
                failed.push(i);
                if first.is_none() {
                    first = Some(e);
                }
            }
        }
        let result = self.settle(first.map_or(Ok(()), Err));
        (failed, result)
    }

    /// The store's file was removed on purpose (nothing left to keep): no
    /// longer unsaved.
    pub fn nothing_to_save(&'static self) {
        let _ = self.settle(Ok(()));
    }

    /// Whether the store holds changes that aren't on disk.
    #[cfg(test)]
    pub fn unsaved(&'static self) -> bool {
        with_failed(|list| list.iter().any(|f| std::ptr::eq(f.saver, self)))
    }
}

// ---- reading ---------------------------------------------------------------------------------

/// How long a store whose file couldn't be read waits before it's read
/// again on its own ([`Saver::refresh`]; the background reads, Try again and
/// quitting don't wait).
pub const REREAD_GAP: Duration = Duration::from_secs(2);
/// A read another program blocks is tried this many more times at load, 50
/// ms apart (it's usually gone in a moment).
const HELD_TRIES: u32 = 4;

/// A store's file as [`Saver::read`] found it.
#[derive(Debug)]
pub enum Found<T> {
    /// Read: what it holds.
    Read(T),
    /// No file (a first run, or deleted): the store starts empty.
    Missing,
    /// Not the store's shape (torn, not JSON, not UTF-8): set aside as
    /// `<name>.corrupt-<secs>`, never written over; the store starts empty.
    SetAside,
    /// There, but Yap couldn't read it (another program holding it, Windows
    /// refusing, a cloud file offline), or not the store's shape and it
    /// couldn't be set aside. The store starts empty in memory; the file is
    /// never written over until a read of it works ([`Saver::refresh`]).
    Unread,
}

impl<T: Default> Found<T> {
    /// What the store starts with.
    pub fn store(self) -> T {
        match self {
            Found::Read(t) => t,
            _ => T::default(),
        }
    }
}

/// `text` as JSON of `T` (`None`: not the store's shape). The usual parse
/// for [`Saver::read`].
pub fn json<T: DeserializeOwned>(text: &str) -> Option<T> {
    serde_json::from_str(text).ok()
}

/// A store's file Yap couldn't read.
#[derive(Debug)]
struct Unread {
    saver: &'static Saver,
    path: PathBuf,
    /// When it was found so (unix seconds): ids given meanwhile start past
    /// it ([`Saver::next_id`]).
    since: u64,
    /// Not read again on its own before this ([`REREAD_GAP`]).
    next_try: Instant,
    /// The last JSON a write of it was refused: kept beside it at quit if it
    /// never reads ([`flush`]).
    refused: Option<String>,
}

// The files Yap couldn't read. A unit test thread has a list of its own,
// like the failures.
#[cfg(not(test))]
static UNREAD: std::sync::Mutex<Vec<Unread>> = std::sync::Mutex::new(Vec::new());

#[cfg(test)]
thread_local! {
    static UNREAD: std::cell::RefCell<Vec<Unread>> = const { std::cell::RefCell::new(Vec::new()) };
}

fn with_unread<R>(f: impl FnOnce(&mut Vec<Unread>) -> R) -> R {
    #[cfg(not(test))]
    {
        f(&mut UNREAD.lock().unwrap_or_else(|p| p.into_inner()))
    }
    #[cfg(test)]
    {
        UNREAD.with(|l| f(&mut l.borrow_mut()))
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// A path's file name, for the log (never the folder).
fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

impl Saver {
    /// Read the store's file `path`, parsed by `parse` (`None`: not the
    /// store's shape; [`json`] for most). See [`Found`]: a file that couldn't
    /// be read marks the store unread — its writes of `path` refused, said
    /// once (Try again reads it again), read again in a moment.
    pub fn read<T>(&'static self, path: &Path, parse: impl Fn(&str) -> Option<T>) -> Found<T> {
        self.read_file(path, &parse, true)
    }

    /// [`Self::read`] as JSON, as the store starts: what the file holds, or
    /// empty (`T::default()`).
    pub fn load<T: DeserializeOwned + Default>(&'static self, path: &Path) -> T {
        self.read(path, json).store()
    }

    fn read_file<T>(&'static self, path: &Path, parse: &dyn Fn(&str) -> Option<T>, patient: bool) -> Found<T> {
        let mut tries = 0;
        let bytes = loop {
            match read_bytes(self.id, path) {
                Err(e) if patient && tries < HELD_TRIES && crate::config::held(&e) => {
                    tries += 1;
                    std::thread::sleep(Duration::from_millis(50));
                }
                read => break read,
            }
        };
        match bytes {
            Ok(bytes) => {
                let parsed = std::str::from_utf8(crate::config::without_bom(&bytes)).ok().and_then(parse);
                match parsed {
                    Some(t) => {
                        self.mark_read(path);
                        Found::Read(t)
                    }
                    // Set aside, never written over.
                    None if crate::config::quarantine_corrupt(path) => {
                        tracing::warn!(store = self.id, bytes = bytes.len(), "a store's file wasn't its shape: set aside, the store starts empty");
                        self.mark_read(path);
                        Found::SetAside
                    }
                    None => {
                        self.mark_unread(path, &io::Error::new(io::ErrorKind::InvalidData, "not the store's shape, and it couldn't be set aside"));
                        Found::Unread
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                self.mark_read(path);
                Found::Missing
            }
            Err(e) => {
                self.mark_unread(path, &e);
                Found::Unread
            }
        }
    }

    /// Whether `path` is a file of this store Yap couldn't read (its writes
    /// refused).
    pub fn unread(&'static self, path: &Path) -> bool {
        with_unread(|list| list.iter().any(|u| std::ptr::eq(u.saver, self) && u.path == path))
    }

    /// Whether any file of this store couldn't be read (a sweep that deletes
    /// what the store no longer names waits for it).
    pub fn any_unread(&'static self) -> bool {
        with_unread(|list| list.iter().any(|u| std::ptr::eq(u.saver, self)))
    }

    /// The files of this store Yap couldn't read (a store kept in several).
    pub fn unread_paths(&'static self) -> Vec<PathBuf> {
        with_unread(|list| list.iter().filter(|u| std::ptr::eq(u.saver, self)).map(|u| u.path.clone()).collect())
    }

    /// A new item's id in a store that numbers them (`max`: the highest it
    /// holds): `max + 1`, but while its file couldn't be read, past the time
    /// it was found so — never an id the file has (merged later).
    pub fn next_id(&'static self, max: u64) -> u64 {
        max.max(self.id_floor()) + 1
    }

    /// The lowest id a new item may have while the store's file couldn't be
    /// read (0 when it was read): see [`Self::next_id`].
    pub fn id_floor(&'static self) -> u64 {
        with_unread(|list| list.iter().filter(|u| std::ptr::eq(u.saver, self)).map(|u| u.since).max()).unwrap_or(0)
    }

    /// A store whose file `path` couldn't be read: read it again, unless it
    /// was tried less than [`REREAD_GAP`] ago. `Some`: it reads now (or it's
    /// gone, or set aside: `T::default()`), and writing is allowed again —
    /// the caller takes it in with what it holds and saves. `None`: nothing
    /// to do (the file was read, or still can't be).
    pub fn reread<T: Default>(&'static self, path: &Path, parse: impl Fn(&str) -> Option<T>) -> Option<T> {
        let now = Instant::now();
        let due = with_unread(|list| {
            let u = list.iter_mut().find(|u| std::ptr::eq(u.saver, self) && u.path == path)?;
            if u.next_try > now {
                return Some(false);
            }
            u.next_try = now + REREAD_GAP;
            Some(true)
        });
        if due != Some(true) {
            return None;
        }
        match self.read_file(path, &parse, false) {
            Found::Read(t) => Some(t),
            Found::Missing | Found::SetAside => Some(T::default()),
            Found::Unread => None,
        }
    }

    /// The store `store` (JSON in `path`), whose file couldn't be read: read
    /// again ([`Self::reread`]) and, once it reads, the file and what was
    /// added in memory meanwhile put together in `store` ([`merge_json`]).
    /// `true`: merged — the caller saves it.
    pub fn refresh<T: Serialize + DeserializeOwned + Default>(&'static self, path: &Path, store: &mut T) -> bool {
        self.refresh_with(path, store, json)
    }

    /// [`Self::refresh`] with the store's own parse.
    pub fn refresh_with<T: Serialize + DeserializeOwned + Default>(
        &'static self,
        path: &Path,
        store: &mut T,
        parse: impl Fn(&str) -> Option<T>,
    ) -> bool {
        let Some(disk) = self.reread(path, parse) else {
            return false;
        };
        merge_json(store, disk);
        tracing::info!(store = self.id, "read its file again: what was there and what was added since are kept together");
        true
    }

    /// A store kept in `slot`, read from `path` the first time
    /// ([`Self::load`]); while its file couldn't be read, read again and,
    /// once it reads, merged and saved (`pretty`: indented) —
    /// [`Self::refresh`]. The usual way a small store is used.
    pub fn loaded<'a, T: Serialize + DeserializeOwned + Default>(&'static self, slot: &'a mut Option<T>, path: &Path, pretty: bool) -> &'a mut T {
        let store = slot.get_or_insert_with(|| self.load(path));
        if self.any_unread() && self.refresh(path, store) {
            let _ = self.write_json(path, &*store, pretty);
        }
        store
    }

    /// `path` couldn't be read (`e`): its writes refused from now on, said
    /// once, read again in a moment.
    fn mark_unread(&'static self, path: &Path, e: &io::Error) {
        let new = with_unread(|list| {
            if list.iter().any(|u| std::ptr::eq(u.saver, self) && u.path == path) {
                return false;
            }
            list.push(Unread { saver: self, path: path.to_path_buf(), since: now_secs(), next_try: Instant::now() + REREAD_GAP, refused: None });
            true
        });
        if !new {
            tracing::debug!(store = self.id, kind = ?e.kind(), os_error = ?e.raw_os_error(), "still couldn't read the store's file");
            return;
        }
        tracing::warn!(
            store = self.id,
            file = %file_name(path),
            kind = ?e.kind(),
            os_error = ?e.raw_os_error(),
            "couldn't read the store's file: the store works in memory, never writes over it, and reads it again later"
        );
        // Said once, as a save that can't happen yet would be (a cache only
        // logged); Try again reads it again.
        if self.failed(Reason::Unread) && self.tells {
            tell_later();
        }
        reread_later();
    }

    /// `path` was read (or isn't there): writing it is fine again.
    fn mark_read(&'static self, path: &Path) {
        let was = with_unread(|list| {
            let before = list.len();
            list.retain(|u| !(std::ptr::eq(u.saver, self) && u.path == path));
            before != list.len()
        });
        if was {
            tracing::info!(store = self.id, file = %file_name(path), "the store's file reads again");
        }
    }

    /// Whether a write of `json` to `path` is refused (a file that couldn't
    /// be read); kept for quitting if so — a change's save, not quitting's
    /// own try (every change saves, so the last is all there is; nothing
    /// changed, nothing kept).
    fn refused(&'static self, path: &Path, json: &str) -> bool {
        let quitting = QUITTING.load(Ordering::SeqCst);
        with_unread(|list| match list.iter_mut().find(|u| std::ptr::eq(u.saver, self) && u.path == path) {
            Some(u) => {
                if !quitting {
                    u.refused = Some(json.to_string());
                }
                true
            }
            None => false,
        })
    }
}

/// The first store of the person's whose file couldn't be read, as it's
/// named ("your notes"): Reset app and Download your data wait for it.
pub fn unread_store() -> Option<&'static str> {
    with_unread(|list| list.iter().find(|u| u.saver.tells).map(|u| u.saver.whole))
}

/// `T` with `disk` (a store's file, read at last) and what `mem` holds — it
/// started empty (`T::default()`) when the file couldn't be read, and has
/// what was added since — put together in `mem` ([`merge_values`]). Yap's
/// own types always go to and from JSON; if one didn't, the file wins.
pub fn merge_json<T: Serialize + DeserializeOwned + Default>(mem: &mut T, disk: T) {
    let values = (serde_json::to_value(T::default()), serde_json::to_value(&*mem), serde_json::to_value(&disk));
    let (Ok(base), Ok(m), Ok(d)) = values else {
        tracing::error!("couldn't take in a store's file with what it held: the file's kept");
        *mem = disk;
        return;
    };
    match serde_json::from_value(merge_values(&base, d, m)) {
        Ok(t) => *mem = t,
        Err(e) => {
            tracing::error!("couldn't take in a store's file with what it held ({e}): the file's kept");
            *mem = disk;
        }
    }
}

/// A store's file (`disk`) with what was added in memory (`mem`) since it
/// started as `base`, so nothing in the file is lost (pure):
///
/// - unchanged in memory: the file's; unchanged in the file: memory's;
/// - objects: every key of both, each merged;
/// - lists: the file's items, then memory's that it hasn't (the same item:
///   the same `id`, else `key`, else equal), one in both merged;
/// - a number changed in both: the larger (counts, times, how far read);
/// - anything else changed in both: memory's (the newer).
pub fn merge_values(base: &Value, disk: Value, mem: Value) -> Value {
    if mem == *base || mem == disk {
        return disk;
    }
    if disk == *base {
        return mem;
    }
    match (disk, mem) {
        (Value::Object(mut d), Value::Object(m)) => {
            for (k, mv) in m {
                let merged = match d.remove(&k) {
                    Some(dv) => merge_values(base.get(&k).unwrap_or(&Value::Null), dv, mv),
                    None => mv,
                };
                d.insert(k, merged);
            }
            Value::Object(d)
        }
        (Value::Array(mut d), Value::Array(m)) => {
            for mv in m {
                match d.iter().position(|dv| same_item(dv, &mv)) {
                    Some(i) => {
                        let b = base.as_array().and_then(|b| b.iter().find(|bv| same_item(bv, &mv))).unwrap_or(&Value::Null);
                        let dv = std::mem::take(&mut d[i]);
                        d[i] = merge_values(b, dv, mv);
                    }
                    None => d.push(mv),
                }
            }
            Value::Array(d)
        }
        (Value::Number(d), Value::Number(m)) => {
            if m.as_f64().unwrap_or(0.0) > d.as_f64().unwrap_or(0.0) {
                Value::Number(m)
            } else {
                Value::Number(d)
            }
        }
        (_, m) => m,
    }
}

/// Whether two items of a list are the same one: by `id`, else `key`, when
/// both have it; else equal.
fn same_item(a: &Value, b: &Value) -> bool {
    for field in ["id", "key"] {
        if let (Some(x), Some(y)) = (a.get(field), b.get(field)) {
            return x == y;
        }
    }
    a == b
}

/// Read every file Yap couldn't, through its store's own save (which reads
/// it first and takes in what's there): now, not waiting for
/// [`REREAD_GAP`]. Returns how many stores still can't be read.
fn reread_all() -> usize {
    let savers: Vec<&'static Saver> = with_unread(|list| {
        let now = Instant::now();
        let mut savers: Vec<&'static Saver> = Vec::new();
        for u in list.iter_mut() {
            u.next_try = now;
            if !savers.iter().any(|s| std::ptr::eq(*s, u.saver)) {
                savers.push(u.saver);
            }
        }
        savers
    });
    // Reading again in the background says nothing: a store's `save_now`
    // ends with its check, which counts as said, so what wasn't said yet
    // is left to the notice.
    let told: Vec<(&'static Saver, bool)> = with_failed(|list| list.iter().map(|f| (f.saver, f.told)).collect());
    for saver in savers {
        let _ = (saver.save_now)();
    }
    with_failed(|list| {
        for f in list.iter_mut() {
            f.told = told.iter().find(|(s, _)| std::ptr::eq(*s, f.saver)).is_some_and(|(_, was)| *was);
        }
    });
    with_unread(|list| list.len())
}

/// A background read of every file Yap couldn't read is running.
static REREADING: AtomicBool = AtomicBool::new(false);

/// While any store's file couldn't be read: read again after 2 s, 5 s,
/// 15 s, 30 s, then every minute, until each reads (one thread, started
/// once). Never in a unit test.
fn reread_later() {
    if cfg!(test) || REREADING.swap(true, Ordering::SeqCst) {
        return;
    }
    let spawned = std::thread::Builder::new().name("store-reread".into()).spawn(|| {
        let mut waits = [2u64, 5, 15, 30].into_iter().chain(std::iter::repeat(60));
        loop {
            std::thread::sleep(Duration::from_secs(waits.next().unwrap_or(60)));
            if reread_all() > 0 {
                continue;
            }
            REREADING.store(false, Ordering::SeqCst);
            // One found unreadable meanwhile saw this thread running.
            if with_unread(|list| list.is_empty()) || REREADING.swap(true, Ordering::SeqCst) {
                return;
            }
        }
    });
    if let Err(e) = spawned {
        REREADING.store(false, Ordering::SeqCst);
        tracing::warn!("saves: couldn't start reading the stores' files again ({e})");
    }
}

/// Quitting with a store's file still unread: what the store last tried to
/// save is kept beside it as `<name>.unsaved-<secs>.json`, so nothing
/// added meanwhile is lost (the file itself never written over).
fn keep_beside() {
    let refused: Vec<(&'static Saver, PathBuf, String)> = with_unread(|list| {
        list.iter_mut().filter(|u| u.saver.tells).filter_map(|u| Some((u.saver, u.path.clone(), u.refused.take()?))).collect()
    });
    for (saver, path, json) in refused {
        let beside = path.with_extension(format!("unsaved-{}.json", now_secs()));
        match crate::config::atomic_write(&beside, &json) {
            Ok(()) => tracing::warn!(store = saver.id, file = %file_name(&beside), "quitting with the store's file unread: what it held is kept beside it"),
            Err(e) => tracing::warn!(store = saver.id, kind = ?e.kind(), "quitting with the store's file unread, and what it held couldn't be kept beside it"),
        }
    }
}

/// **Writes in order, outside the store's lock** (notes.json: its notes can
/// hold hours of transcript, and every recorder chunk, line edit and tick
/// used to wait for the disk under the lock). The store takes a snapshot
/// under its lock ([`Ordered::take`] numbers it, then the JSON), lets the
/// lock go, and writes it ([`Ordered::write`]): one write at a time, and a
/// snapshot older than one already written is never written (it would put
/// older notes back over newer ones). The newer snapshot holds every change
/// the older one did, so skipping it loses nothing; a write that fails
/// leaves the store unsaved ([`Saver`]) and the next one writes it all.
#[derive(Debug)]
pub struct Ordered {
    taken: std::sync::atomic::AtomicU64,
    written: std::sync::Mutex<u64>,
}

impl Default for Ordered {
    fn default() -> Self {
        Self::new()
    }
}

impl Ordered {
    pub const fn new() -> Self {
        Self { taken: std::sync::atomic::AtomicU64::new(0), written: std::sync::Mutex::new(0) }
    }

    /// The number of a snapshot taken now. Call it under the store's lock,
    /// so the numbers follow the changes.
    pub fn take(&self) -> u64 {
        self.taken.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Write snapshot `seq` (`write`), unless a later one was written
    /// already: `None` then. Writes wait for each other.
    pub fn write<R>(&self, seq: u64, write: impl FnOnce() -> R) -> Option<R> {
        let mut last = self.written.lock().unwrap_or_else(|p| p.into_inner());
        if seq <= *last {
            return None;
        }
        let out = write();
        *last = seq;
        Some(out)
    }
}

/// `config::atomic_write`, the folder made first (and, in a test run, a
/// failure staged by `e2e_fail_saves`).
fn write_file(id: &str, path: &Path, json: &str) -> io::Result<()> {
    #[cfg(debug_assertions)]
    if staged_failure(id) {
        return Err(io::Error::new(io::ErrorKind::StorageFull, "a failure staged by the test"));
    }
    #[cfg(not(debug_assertions))]
    let _ = id;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    crate::config::atomic_write(path, json)
}

/// `std::fs::read` (and, in a test run, a read staged to fail:
/// `e2e_fail_reads` — once each, not tried again as a held file is).
fn read_bytes(id: &str, path: &Path) -> io::Result<Vec<u8>> {
    #[cfg(debug_assertions)]
    if staged_read_failure(id) {
        return Err(io::Error::other("a read staged to fail by the test"));
    }
    #[cfg(not(debug_assertions))]
    let _ = id;
    std::fs::read(path)
}

/// Write every store whose last save failed again — a store whose file
/// couldn't be read reads it first, now. `Err`: the first that still can't
/// be saved (said by the caller: [`Saver::check`]).
pub fn retry_all() -> Result<(), SaveError> {
    with_unread(|list| {
        let now = Instant::now();
        for u in list.iter_mut() {
            u.next_try = now;
        }
    });
    let unsaved: Vec<&'static Saver> = with_failed(|list| list.iter().map(|f| f.saver).collect());
    let mut first = None;
    for saver in unsaved {
        if let Err(e) = (saver.save_now)().and_then(|()| saver.check()) {
            first.get_or_insert(e);
        }
    }
    first.map_or(Ok(()), Err)
}

/// Quitting ([`flush`]): its own tries are never what's kept beside a file
/// Yap couldn't read.
static QUITTING: AtomicBool = AtomicBool::new(false);

/// Quitting: one more try for anything unsaved (the log says how it went);
/// a store whose file still can't be read keeps what it held beside it.
pub fn flush() {
    if with_failed(|list| list.is_empty()) {
        return;
    }
    QUITTING.store(true, Ordering::SeqCst);
    match retry_all() {
        Ok(()) => tracing::info!("saves: everything unsaved is saved now"),
        Err(e) => tracing::warn!(reason = ?e.reason, "saves: some changes couldn't be saved before quitting"),
    }
    keep_beside();
}

// ---- telling the person ----------------------------------------------------------------------

static APP: OnceLock<AppHandle> = OnceLock::new();
/// A notice is on its way ([`tell_later`]).
static TELLING: AtomicBool = AtomicBool::new(false);

/// Start with the app: failed saves are said from now on — and one before
/// (a store's file Yap couldn't read as the app started) said now.
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
    if with_failed(|list| list.iter().any(|f| f.saver.tells && !f.told)) {
        tell_later();
    }
}

/// In [`TELL_AFTER`], say the failed saves no command said. Safe under any
/// store's lock: it only starts a thread.
fn tell_later() {
    let Some(app) = APP.get() else {
        return;
    };
    if TELLING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("save-failed".into()).spawn(move || {
        std::thread::sleep(TELL_AFTER);
        TELLING.store(false, Ordering::SeqCst);
        if let Some(alert) = untold_alert() {
            crate::notices::alert::show(&app, alert);
        }
    });
    if let Err(e) = spawned {
        TELLING.store(false, Ordering::SeqCst);
        tracing::warn!("saves: couldn't start the notice ({e})");
    }
}

/// The notice for the failed saves nobody said yet (marked said), if any.
fn untold_alert() -> Option<crate::notices::alert::Alert> {
    let untold: Vec<Failure> = with_failed(|list| {
        list.iter_mut()
            .filter(|f| f.saver.tells && !f.told)
            .map(|f| {
                f.told = true;
                *f
            })
            .collect()
    });
    let first = untold.first()?;
    Some(failed_alert(first.saver.error(first.reason), untold.len() - 1, false))
}

/// What the person is told about a failed save (`others`: how many more
/// stores failed with it; `again`: after Try again) — or a store whose file
/// couldn't be read ([`Reason::Unread`]).
pub fn failed_alert(e: SaveError, others: usize, again: bool) -> crate::notices::alert::Alert {
    let (title, mut body) = if e.reason == Reason::Unread {
        (
            format!("{} read {}", if again { "Still couldn't" } else { "Yap couldn't" }, e.whole),
            "Another program may be using the file. Nothing was changed, and Yap won't save over it: it'll try again in a moment, keeping anything new until then.".to_string(),
        )
    } else {
        (
            if again { format!("Still couldn't save {}", e.what) } else { format!("Couldn't save {}", e.what) },
            format!("{}. Yap is keeping it in memory and will try again with the next change.", capitalised(e.reason.words())),
        )
    };
    if others > 0 {
        body.push_str(" Some of Yap's other files couldn't be read or saved either.");
    }
    crate::notices::alert::Alert {
        kind: KIND,
        icon: "save",
        title,
        body,
        action: Some(crate::notices::alert::Action { id: RETRY, label: "Try again" }),
        timeout_ms: 30_000,
    }
}

fn capitalised(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// The alert's Try again (`alert::act`), on a thread of its own (file
/// writes, then the window queries of the answer): "Saved", or "Still
/// couldn't save…".
pub fn retry_soon(app: &AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("save-retry".into()).spawn(move || {
        let alert = match retry_all() {
            Ok(()) => crate::notices::alert::Alert {
                kind: KIND,
                icon: "save",
                title: "Saved".into(),
                body: "Everything Yap was keeping is saved now.".into(),
                action: None,
                timeout_ms: 4_000,
            },
            Err(e) => failed_alert(e, 0, true),
        };
        crate::notices::alert::show(&app, alert);
    });
    if let Err(e) = spawned {
        tracing::warn!("saves: couldn't try again ({e})");
    }
}

/// The page's Try again (`saveErrors.js`): every unsaved store written
/// again; the error of one that still can't be.
#[tauri::command]
pub fn saves_retry() -> Result<(), String> {
    retry_all().map_err(String::from)
}

// ---- test runs -------------------------------------------------------------------------------

/// Failures staged by `e2e_fail_saves`: store id → how many writes still
/// fail.
#[cfg(all(debug_assertions, not(test)))]
static STAGED: std::sync::Mutex<Vec<(String, u32)>> = std::sync::Mutex::new(Vec::new());

// Unit tests stage failures for their own thread's writes: tests running
// side by side never use up each other's.
#[cfg(test)]
thread_local! {
    static STAGED: std::cell::RefCell<Vec<(String, u32)>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(debug_assertions)]
fn with_staged<R>(f: impl FnOnce(&mut Vec<(String, u32)>) -> R) -> R {
    #[cfg(not(test))]
    {
        f(&mut STAGED.lock().unwrap_or_else(|p| p.into_inner()))
    }
    #[cfg(test)]
    {
        STAGED.with(|s| f(&mut s.borrow_mut()))
    }
}

/// The next `count` writes of store `id` fail as if the disk were full
/// (0: none).
#[cfg(debug_assertions)]
pub fn stage_failures(id: &str, count: u32) {
    with_staged(|staged| {
        staged.retain(|(s, _)| s != id);
        if count > 0 {
            staged.push((id.to_string(), count));
        }
    });
}

/// Whether this write of store `id` fails (one staged failure used up).
/// config.json's saves ask too, as "config" (`config::save_at`).
#[cfg(debug_assertions)]
pub(crate) fn staged_failure(id: &str) -> bool {
    with_staged(|staged| use_one(staged, id))
}

/// One staged failure of store `id` used up, if there's one.
#[cfg(debug_assertions)]
fn use_one(staged: &mut Vec<(String, u32)>, id: &str) -> bool {
    let Some(i) = staged.iter().position(|(s, _)| s == id) else {
        return false;
    };
    staged[i].1 -= 1;
    if staged[i].1 == 0 {
        staged.remove(i);
    }
    true
}

/// Reads staged to fail by `e2e_fail_reads` (or, from the start of a test
/// run, `YAP_E2E_FAIL_READS`: store ids, comma-separated, each failing
/// until cleared): store id → how many reads still fail.
#[cfg(all(debug_assertions, not(test)))]
static STAGED_READS: std::sync::Mutex<Vec<(String, u32)>> = std::sync::Mutex::new(Vec::new());

#[cfg(test)]
thread_local! {
    static STAGED_READS: std::cell::RefCell<Vec<(String, u32)>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(debug_assertions)]
fn with_staged_reads<R>(f: impl FnOnce(&mut Vec<(String, u32)>) -> R) -> R {
    #[cfg(not(test))]
    {
        static FROM_ENV: OnceLock<()> = OnceLock::new();
        let mut staged = STAGED_READS.lock().unwrap_or_else(|p| p.into_inner());
        FROM_ENV.get_or_init(|| {
            if crate::shell::e2e::active() {
                let ids = std::env::var("YAP_E2E_FAIL_READS").unwrap_or_default();
                for id in ids.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                    tracing::info!(store = id, "e2e: reads of this store fail on purpose from the start");
                    staged.push((id.to_string(), u32::MAX));
                }
            }
        });
        f(&mut staged)
    }
    #[cfg(test)]
    {
        STAGED_READS.with(|s| f(&mut s.borrow_mut()))
    }
}

/// The next `count` reads of store `id`'s file fail as if another program
/// held it (0: none).
#[cfg(debug_assertions)]
pub fn stage_read_failures(id: &str, count: u32) {
    with_staged_reads(|staged| {
        staged.retain(|(s, _)| s != id);
        if count > 0 {
            staged.push((id.to_string(), count));
        }
    });
}

#[cfg(debug_assertions)]
fn staged_read_failure(id: &str) -> bool {
    with_staged_reads(|staged| use_one(staged, id))
}

/// Read every store's file Yap couldn't read now (a test run's
/// `e2e_fail_reads` once it lets them read): how many still can't be.
#[cfg(debug_assertions)]
pub fn reread_now() -> usize {
    reread_all()
}

/// A unit test: every file this thread couldn't read is due to be read
/// again now (as the background reads would make it).
#[cfg(test)]
pub fn reread_due() {
    with_unread(|list| {
        for u in list.iter_mut() {
            u.next_try = Instant::now();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yap-saving-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn nothing_to_redo() -> Result<(), SaveError> {
        Ok(())
    }

    // Each test has savers of its own: tests run side by side, and a
    // failure staged for one store id is used up by whichever write comes
    // first.
    static NOTES: Saver = Saver::new("test-notes", "your note", nothing_to_redo);
    static CACHE: Saver = Saver::quiet("test-cache", "the model lists", nothing_to_redo);
    static LOCKED: Saver = Saver::new("test-locked", "your note", nothing_to_redo);
    static READ_ONLY: Saver = Saver::new("test-read-only", "your dictation history", nothing_to_redo);
    static FOLDER: Saver = Saver::new("test-folder", "your calendar", nothing_to_redo);

    #[test]
    fn the_person_reads_what_and_why_in_plain_words() {
        let e = SaveError { what: "your note", reason: Reason::DiskFull, whole: "your note" };
        assert_eq!(e.to_string(), "Couldn't save your note: the disk is full.");
        assert_eq!(String::from(SaveError { what: "your chat", reason: Reason::Locked, whole: "your chat" }), "Couldn't save your chat: another program has the file locked.");
        assert_eq!(Reason::of(&io::Error::from(io::ErrorKind::StorageFull)), Reason::DiskFull);
        assert_eq!(Reason::of(&io::Error::from(io::ErrorKind::PermissionDenied)), Reason::NotAllowed);
        let denied = io::Error::from(io::ErrorKind::PermissionDenied);
        assert_eq!(Reason::at(&denied, Path::new("Z:/no/such/folder/notes.json")), Reason::NotAllowed);
        assert_eq!(Reason::of(&io::Error::from(io::ErrorKind::InvalidData)), Reason::Other);
        #[cfg(windows)]
        {
            assert_eq!(Reason::of(&io::Error::from_raw_os_error(112)), Reason::DiskFull);
            assert_eq!(Reason::of(&io::Error::from_raw_os_error(32)), Reason::Locked);
        }
        let a = failed_alert(e, 2, false);
        assert_eq!(a.title, "Couldn't save your note");
        assert!(a.body.starts_with("The disk is full. Yap is keeping it"), "{}", a.body);
        assert!(a.body.contains("other files"), "{}", a.body);
        assert_eq!(a.action.map(|x| x.label), Some("Try again"));
        assert_eq!(failed_alert(e, 0, true).title, "Still couldn't save your note");
    }

    /// Another program holds the file (an antivirus, a backup tool): the
    /// save says so, the file on disk is left as it was, and the next save
    /// once it's free writes everything.
    #[cfg(windows)]
    #[test]
    fn a_save_to_a_locked_file_fails_and_the_next_one_writes_it_all() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = scratch("locked");
        let path = dir.join("notes.json");
        LOCKED.write(&path, r#"{"notes":["milk"]}"#).unwrap();
        let held = std::fs::OpenOptions::new().read(true).share_mode(0).open(&path).unwrap();
        let e = LOCKED.write(&path, r#"{"notes":["milk","eggs"]}"#).unwrap_err();
        assert_eq!(e, SaveError { what: "your note", reason: Reason::Locked, whole: "your note" });
        assert!(LOCKED.unsaved());
        drop(held);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{"notes":["milk"]}"#, "the old file, whole");
        // No temp file left behind.
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        // The next save writes what the store holds by then.
        LOCKED.write(&path, r#"{"notes":["milk","eggs","bread"]}"#).unwrap();
        assert!(!LOCKED.unsaved());
        assert!(LOCKED.check().is_ok());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{"notes":["milk","eggs","bread"]}"#);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn a_save_to_a_read_only_file_fails_until_it_can_be_written() {
        let dir = scratch("read-only");
        let path = dir.join("history.json");
        READ_ONLY.write(&path, "[1]").unwrap();
        let mut perms = std::fs::metadata(&path).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&path, perms.clone()).unwrap();
        let e = READ_ONLY.write(&path, "[1,2]").unwrap_err();
        assert_eq!(e, SaveError { what: "your dictation history", reason: Reason::ReadOnly, whole: "your dictation history" });
        assert!(READ_ONLY.unsaved());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[1]");
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        std::fs::set_permissions(&path, perms).unwrap();
        READ_ONLY.write(&path, "[1,2,3]").unwrap();
        assert!(!READ_ONLY.unsaved());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[1,2,3]");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_folder_is_made() {
        let dir = scratch("folder");
        let path = dir.join("deeper").join("calendar.json");
        FOLDER.write_json(&path, &serde_json::json!({ "events": [] }), false).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{"events":[]}"#);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A failed save is said once: by the command that hit it (`check`) or
    /// by the notice, never both, and not again until a save has worked.
    #[test]
    fn a_failure_is_said_once() {
        stage_failures("test-notes", 2);
        stage_failures("test-cache", 1);
        let dir = scratch("once");
        let path = dir.join("notes.json");
        assert!(NOTES.write(&path, "{}").is_err());
        assert!(CACHE.write(&dir.join("model_catalog.json"), "{}").is_err());
        // The notice names the store of the person's, not the cache.
        let alert = untold_alert().expect("said");
        assert_eq!(alert.title, "Couldn't save your note");
        assert!(!alert.body.contains("other files"), "a cache isn't counted: {}", alert.body);
        // Failing again: nothing more to say.
        assert!(NOTES.write(&path, "{}").is_err());
        assert!(untold_alert().is_none());
        // Saved: a later failure is new, and a command that says it (check)
        // leaves the notice nothing to say.
        NOTES.write(&path, "{}").unwrap();
        stage_failures("test-notes", 1);
        let e = NOTES.write(&path, "{}").unwrap_err();
        assert_eq!(NOTES.check(), Err(e));
        assert!(untold_alert().is_none());
        // Try again writes each unsaved store (here: nothing left to redo,
        // and the file is writable), and the cache's own next save clears it.
        stage_failures("test-notes", 0);
        NOTES.write(&path, "{}").unwrap();
        CACHE.write(&dir.join("model_catalog.json"), "{}").unwrap();
        assert!(retry_all().is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Snapshots taken under a store's lock and written after it, in any
    /// order the threads get there: the file ends as the last snapshot,
    /// never an older one written over a newer one.
    #[test]
    fn snapshots_written_outside_the_lock_never_go_back_in_time() {
        static ORDER: Ordered = Ordered::new();
        static NOTES_ORDERED: Saver = Saver::new("test-ordered", "your note", nothing_to_redo);
        let dir = scratch("ordered");
        let path = dir.join("notes.json");
        // Taken 1 then 2; 2 reaches the disk first: 1 is never written.
        let (a, b) = (ORDER.take(), ORDER.take());
        assert_eq!(ORDER.write(b, || NOTES_ORDERED.write(&path, "[1,2]")), Some(Ok(())));
        assert_eq!(ORDER.write(a, || NOTES_ORDERED.write(&path, "[1]")), None);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[1,2]");

        // Eight threads, each changing a shared store under its lock and
        // writing its snapshot after: the file is the store's last state.
        let store = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u32>::new()));
        let threads: Vec<_> = (0..8u32)
            .map(|t| {
                let (store, path) = (std::sync::Arc::clone(&store), path.clone());
                std::thread::spawn(move || {
                    for k in 0..25u32 {
                        let (seq, json) = {
                            let mut s = store.lock().unwrap();
                            s.push(t * 100 + k);
                            (ORDER.take(), serde_json::to_string(&*s).unwrap())
                        };
                        if k % 3 == 0 {
                            std::thread::yield_now();
                        }
                        let _ = ORDER.write(seq, || NOTES_ORDERED.write(&path, &json));
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let on_disk: Vec<u32> = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(on_disk, *store.lock().unwrap());
        assert_eq!(on_disk.len(), 200);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A store kept in several files: unsaved while any of them failed.
    #[test]
    fn a_store_in_several_files_is_unsaved_until_each_is_written() {
        static METAS: Saver = Saver::new("test-metas", "your meeting audio's notes", nothing_to_redo);
        let dir = scratch("each");
        let files = vec![(dir.join("1.json"), "{}".to_string()), (dir.join("2.json"), "{}".to_string())];
        stage_failures("test-metas", 1);
        let (failed, r) = METAS.write_each(&files);
        assert_eq!(failed, vec![0]);
        assert!(r.is_err() && METAS.unsaved());
        assert!(dir.join("2.json").is_file(), "the others still written");
        let (failed, r) = METAS.write_each(&files[..1]);
        assert!(failed.is_empty() && r.is_ok() && !METAS.unsaved());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn try_again_rewrites_each_unsaved_store_and_reports_one_that_still_fails() {
        fn still_failing() -> Result<(), SaveError> {
            Err(SaveError { what: "your chat", reason: Reason::DiskFull, whole: "your chat" })
        }
        static CHATS: Saver = Saver::new("test-chats", "your chat", still_failing);
        stage_failures("test-chats", 1);
        let dir = scratch("retry");
        assert!(CHATS.write(&dir.join("chats.json"), "{}").is_err());
        assert_eq!(retry_all(), Err(SaveError { what: "your chat", reason: Reason::DiskFull, whole: "your chat" }));
        CHATS.write(&dir.join("chats.json"), "{}").unwrap();
        assert_eq!(retry_all(), Ok(()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- reading -------------------------------------------------------------------------

    /// A store's file for the reading tests.
    #[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Shelf {
        #[serde(default)]
        items: Vec<Item>,
        #[serde(default)]
        next: u64,
        #[serde(default)]
        dismissed: bool,
    }

    #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Item {
        id: u64,
        text: String,
    }

    fn item(id: u64, text: &str) -> Item {
        Item { id, text: text.into() }
    }

    fn shelf_on_disk(path: &Path, items: &[Item]) -> String {
        let text = serde_json::to_string(&Shelf { items: items.to_vec(), next: items.len() as u64, dismissed: false }).unwrap();
        std::fs::write(path, &text).unwrap();
        text
    }

    #[test]
    fn no_file_starts_empty_and_saves_as_ever() {
        static MISSING: Saver = Saver::new("test-read-missing", "your shelf", nothing_to_redo);
        let dir = scratch("read-missing");
        let path = dir.join("shelf.json");
        assert!(matches!(MISSING.read(&path, json::<Shelf>), Found::Missing));
        assert!(!MISSING.unread(&path) && !MISSING.unsaved());
        assert_eq!(MISSING.load::<Shelf>(&path), Shelf::default());
        MISSING.write_json(&path, &Shelf { items: vec![item(1, "jam")], ..Shelf::default() }, false).unwrap();
        assert_eq!(MISSING.load::<Shelf>(&path).items, vec![item(1, "jam")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Not the store's shape — torn, not JSON, not UTF-8 — is set aside as
    /// it was (never written over) and the store starts empty; a UTF-8
    /// byte-order mark (Notepad) is read past.
    #[test]
    fn a_file_that_isnt_the_store_is_set_aside_and_the_store_starts_empty() {
        static TORN: Saver = Saver::new("test-read-torn", "your shelf", nothing_to_redo);
        let dir = scratch("read-torn");
        let path = dir.join("shelf.json");
        for bytes in [&b"{\"items\": [{\"id\": 1, \"te"[..], &[0xff, 0xfe, b'{', 0x00, b'}'][..]] {
            std::fs::write(&path, bytes).unwrap();
            assert!(matches!(TORN.read(&path, json::<Shelf>), Found::SetAside));
            assert!(!path.exists(), "moved aside");
            let aside: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("shelf.corrupt-")).collect();
            assert_eq!(std::fs::read(aside[0].path()).unwrap(), bytes, "kept as it was");
            std::fs::remove_file(aside[0].path()).unwrap();
            // Nothing left to protect: the store saves.
            assert!(!TORN.unread(&path));
            TORN.write(&path, "{}").unwrap();
            std::fs::remove_file(&path).unwrap();
        }
        let mut bom = b"\xEF\xBB\xBF".to_vec();
        bom.extend_from_slice(br#"{"items":[{"id":1,"text":"jam"}]}"#);
        std::fs::write(&path, bom).unwrap();
        assert_eq!(TORN.load::<Shelf>(&path).items, vec![item(1, "jam")]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Another program holds the file at load (an antivirus, a backup
    /// tool): the store starts empty in memory, every write of the file is
    /// refused and said once ("Yap couldn't read your shelf", Try again),
    /// ids given meanwhile are past the file's, and once it reads what's
    /// there and what was added are kept together — nothing in the file
    /// lost.
    #[cfg(windows)]
    #[test]
    fn a_file_another_program_holds_is_never_written_over_and_read_again_later() {
        use std::os::windows::fs::OpenOptionsExt;
        static HELD: Saver = Saver::new("test-read-held", "your shelf item", nothing_to_redo).reads_as("your shelf");
        let dir = scratch("read-held");
        let path = dir.join("shelf.json");
        let text = shelf_on_disk(&path, &[item(1, "jam"), item(2, "honey")]);
        let held = std::fs::OpenOptions::new().read(true).share_mode(0).open(&path).unwrap();

        let mut shelf: Shelf = HELD.load(&path);
        assert_eq!(shelf, Shelf::default(), "nothing read");
        assert!(HELD.unread(&path) && HELD.any_unread());
        assert_eq!(unread_store(), Some("your shelf"));
        // Said once, as a read.
        let alert = untold_alert().expect("said");
        assert_eq!(alert.title, "Yap couldn't read your shelf");
        assert!(alert.body.starts_with("Another program may be using the file. Nothing was changed"), "{}", alert.body);
        assert_eq!(alert.action.map(|a| a.label), Some("Try again"));
        assert!(untold_alert().is_none(), "once");
        // Working in memory: a new item's id is past anything the file has.
        let id = HELD.next_id(shelf.items.iter().map(|i| i.id).max().unwrap_or(0));
        assert!(id > 1_000_000_000, "{id}");
        shelf.items.push(item(id, "marmalade"));
        shelf.dismissed = true;
        // Never written over, even once it's free; a command says why.
        let e = HELD.write_json(&path, &shelf, false).unwrap_err();
        assert_eq!(e.reason, Reason::Unread);
        assert_eq!(
            e.to_string(),
            "Couldn't save your shelf item: Yap couldn't read the file (another program may be using it), so nothing was changed."
        );
        drop(held);
        assert!(HELD.write_json(&path, &shelf, false).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text, "the file as it was");
        // Not read again before its time…
        assert!(!HELD.refresh(&path, &mut shelf));
        // …then read, and kept together.
        reread_due();
        assert!(HELD.refresh(&path, &mut shelf));
        assert!(!HELD.unread(&path) && unread_store().is_none());
        assert_eq!(shelf.items, vec![item(1, "jam"), item(2, "honey"), item(id, "marmalade")]);
        assert!(shelf.dismissed);
        assert_eq!(HELD.next_id(shelf.items.iter().map(|i| i.id).max().unwrap_or(0)), id + 1);
        HELD.write_json(&path, &shelf, false).unwrap();
        assert!(!HELD.unsaved());
        assert_eq!(HELD.load::<Shelf>(&path), shelf);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A read staged to fail (as `e2e_fail_reads` does): the same, and Try
    /// again reads it again at once through the store's own save.
    #[test]
    fn try_again_reads_a_file_that_couldnt_be_read() {
        static STORE: std::sync::Mutex<Option<Shelf>> = std::sync::Mutex::new(None);
        thread_local! {
            static PATH: std::cell::RefCell<PathBuf> = const { std::cell::RefCell::new(PathBuf::new()) };
        }
        fn save_shelf() -> Result<(), SaveError> {
            let path = PATH.with(|p| p.borrow().clone());
            let mut guard = STORE.lock().unwrap();
            let shelf = SHELF.loaded(&mut guard, &path, false);
            SHELF.write_json(&path, &*shelf, false)?;
            SHELF.check()
        }
        static SHELF: Saver = Saver::new("test-read-staged", "your shelf", save_shelf);
        let dir = scratch("read-staged");
        let path = dir.join("shelf.json");
        PATH.with(|p| *p.borrow_mut() = path.clone());
        shelf_on_disk(&path, &[item(1, "jam")]);
        stage_read_failures("test-read-staged", 1);
        {
            let mut guard = STORE.lock().unwrap();
            let shelf = SHELF.loaded(&mut guard, &path, false);
            assert!(shelf.items.is_empty());
            let id = SHELF.next_id(0);
            shelf.items.push(item(id, "honey"));
        }
        assert!(SHELF.unread(&path));
        assert_eq!(SHELF.check().map_err(|e| e.reason), Err(Reason::Unread));
        // Try again: read at once (no wait), kept together, written.
        assert_eq!(retry_all(), Ok(()));
        assert!(!SHELF.unread(&path) && !SHELF.unsaved());
        let on_disk: Shelf = SHELF.load(&path);
        assert_eq!(on_disk.items.iter().map(|i| i.text.as_str()).collect::<Vec<_>>(), ["jam", "honey"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The background reads (each store's own save, which ends with its
    /// check) never count as telling the person: the notice still says it.
    #[test]
    fn reading_again_in_the_background_leaves_the_notice_to_say_it() {
        fn check_only() -> Result<(), SaveError> {
            BACKGROUND.check()
        }
        static BACKGROUND: Saver = Saver::new("test-read-background", "your shelf", check_only);
        let dir = scratch("read-background");
        let path = dir.join("shelf.json");
        shelf_on_disk(&path, &[item(1, "jam")]);
        stage_read_failures("test-read-background", u32::MAX);
        let _: Shelf = BACKGROUND.load(&path);
        assert_eq!(reread_all(), 1);
        assert_eq!(untold_alert().map(|a| a.title), Some("Yap couldn't read your shelf".to_string()));
        stage_read_failures("test-read-background", 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Quitting with the file still unread: what the store last tried to
    /// save is kept beside it, the file never touched.
    #[test]
    fn quitting_with_a_file_still_unread_keeps_what_was_added_beside_it() {
        static QUIT: Saver = Saver::new("test-read-quit", "your shelf", nothing_to_redo);
        let dir = scratch("read-quit");
        let path = dir.join("shelf.json");
        let text = shelf_on_disk(&path, &[item(1, "jam")]);
        stage_read_failures("test-read-quit", u32::MAX);
        let _: Shelf = QUIT.load(&path);
        assert!(QUIT.write(&path, r#"{"items":[{"id":9,"text":"honey"}]}"#).is_err());
        keep_beside();
        stage_read_failures("test-read-quit", 0);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        let beside: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("shelf.unsaved-") && n.ends_with(".json")).collect();
        assert_eq!(beside.len(), 1, "{beside:?}");
        assert!(std::fs::read_to_string(dir.join(&beside[0])).unwrap().contains("honey"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What a store held and its file, put together: everything in the file
    /// kept, what was added in memory added, a count the larger, a choice
    /// made meanwhile kept.
    #[test]
    fn a_file_and_what_was_added_meanwhile_are_kept_together() {
        use serde_json::json;
        let base = json!({ "items": [], "counts": {}, "offered": [], "dismissed": false, "lastAt": 0, "name": "" });
        let disk = json!({
            "items": [{ "id": 1, "text": "jam" }, { "id": 2, "text": "honey" }],
            "counts": { "teams": 3, "zoom": 1 },
            "offered": ["teams"],
            "dismissed": false,
            "lastAt": 500,
            "name": "kept",
        });
        let mem = json!({
            "items": [{ "id": 1_761_800_001u64, "text": "marmalade" }],
            "counts": { "teams": 1, "meet": 2 },
            "offered": ["meet"],
            "dismissed": true,
            "lastAt": 900,
            "name": "",
        });
        let merged = merge_values(&base, disk.clone(), mem);
        assert_eq!(
            merged,
            json!({
                "items": [{ "id": 1, "text": "jam" }, { "id": 2, "text": "honey" }, { "id": 1_761_800_001u64, "text": "marmalade" }],
                "counts": { "teams": 3, "zoom": 1, "meet": 2 },
                "offered": ["teams", "meet"],
                "dismissed": true,
                "lastAt": 900,
                "name": "kept",
            })
        );
        // Nothing added in memory: the file as it is.
        assert_eq!(merge_values(&base, disk.clone(), base.clone()), disk);
        // No file (or an empty one): memory's.
        let only = json!({ "items": [{ "id": 7, "text": "figs" }] });
        assert_eq!(merge_values(&json!({ "items": [] }), json!({ "items": [] }), only.clone()), only);
        // A list of plain items: the file's, then the new ones, once each.
        assert_eq!(merge_values(&json!([]), json!([1, 2, 3]), json!([3, 4])), json!([1, 2, 3, 4]));
        // Typed, as a store calls it.
        let mut shelf = Shelf { items: vec![item(1_761_800_001, "marmalade")], next: 1_761_800_002, dismissed: false };
        merge_json(&mut shelf, Shelf { items: vec![item(1, "jam")], next: 2, dismissed: true });
        assert_eq!(shelf, Shelf { items: vec![item(1, "jam"), item(1_761_800_001, "marmalade")], next: 1_761_800_002, dismissed: true });
    }
}

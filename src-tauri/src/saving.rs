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
//!   (`saves_retry` → [`retry_all`]; `src/lib/saveErrors.js`);
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
//! The log says which store and why (the error's kind), never what it holds.

use std::fmt;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use tauri::AppHandle;

/// The alert's kind (its rule in `notice.rs`, the bar card's id).
pub const KIND: &str = "saveFailed";
/// The alert's **Try again** ([`crate::alert::act`]).
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
    /// Said when a save fails by itself (a cache: only logged).
    tells: bool,
    /// Write everything the store holds again (the retry).
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
        Self { id, what, tells: true, save_now }
    }

    /// A cache Yap rebuilds by itself: a failed save is only logged (and
    /// still tried again with the next save).
    pub const fn quiet(id: &'static str, what: &'static str, save_now: fn() -> Result<(), SaveError>) -> Self {
        Self { id, what, tells: false, save_now }
    }

    /// Write `json` (everything the store holds) to `path`, atomically. A
    /// failure leaves the store unsaved, its data in memory, until a save
    /// of it works; said once ([`Self::check`], else the notice).
    pub fn write(&'static self, path: &Path, json: &str) -> Result<(), SaveError> {
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
                let first = with_failed(|list| match list.iter_mut().find(|f| std::ptr::eq(f.saver, self)) {
                    Some(f) => {
                        f.reason = reason;
                        false
                    }
                    None => {
                        list.push(Failure { saver: self, reason, told: false });
                        true
                    }
                });
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
                Err(SaveError { what: self.what, reason })
            }
        }
    }

    /// `Ok` when everything the store holds is on disk; else why not. The
    /// caller says it (a command's error, which the page shows with Try
    /// again), so the notice for saves that failed by themselves doesn't.
    pub fn check(&'static self) -> Result<(), SaveError> {
        with_failed(|list| match list.iter_mut().find(|f| std::ptr::eq(f.saver, self)) {
            Some(f) => {
                f.told = true;
                Err(SaveError { what: self.what, reason: f.reason })
            }
            None => Ok(()),
        })
    }

    /// [`Self::check`] without saying it: for an answer to an AI model or an
    /// AI app (the chat's note tools, the local API), so it never claims a
    /// save that didn't happen, while Yap still says it itself.
    pub fn status(&'static self) -> Result<(), SaveError> {
        with_failed(|list| match list.iter().find(|f| std::ptr::eq(f.saver, self)) {
            Some(f) => Err(SaveError { what: self.what, reason: f.reason }),
            None => Ok(()),
        })
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

/// Write every store whose last save failed again. `Err`: the first that
/// still can't be saved (said by the caller: [`Saver::check`]).
pub fn retry_all() -> Result<(), SaveError> {
    let unsaved: Vec<&'static Saver> = with_failed(|list| list.iter().map(|f| f.saver).collect());
    let mut first = None;
    for saver in unsaved {
        if let Err(e) = (saver.save_now)().and_then(|()| saver.check()) {
            first.get_or_insert(e);
        }
    }
    first.map_or(Ok(()), Err)
}

/// Quitting: one more try for anything unsaved (the log says how it went).
pub fn flush() {
    if with_failed(|list| list.is_empty()) {
        return;
    }
    match retry_all() {
        Ok(()) => tracing::info!("saves: everything unsaved is saved now"),
        Err(e) => tracing::warn!(reason = ?e.reason, "saves: some changes couldn't be saved before quitting"),
    }
}

// ---- telling the person ----------------------------------------------------------------------

static APP: OnceLock<AppHandle> = OnceLock::new();
/// A notice is on its way ([`tell_later`]).
static TELLING: AtomicBool = AtomicBool::new(false);

/// Start with the app: failed saves are said from now on.
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
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
            crate::alert::show(&app, alert);
        }
    });
    if let Err(e) = spawned {
        TELLING.store(false, Ordering::SeqCst);
        tracing::warn!("saves: couldn't start the notice ({e})");
    }
}

/// The notice for the failed saves nobody said yet (marked said), if any.
fn untold_alert() -> Option<crate::alert::Alert> {
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
    Some(failed_alert(SaveError { what: first.saver.what, reason: first.reason }, untold.len() - 1, false))
}

/// What the person is told about a failed save (`others`: how many more
/// stores failed with it; `again`: after Try again).
pub fn failed_alert(e: SaveError, others: usize, again: bool) -> crate::alert::Alert {
    let title = if again { format!("Still couldn't save {}", e.what) } else { format!("Couldn't save {}", e.what) };
    let mut body = format!("{}. Yap is keeping it in memory and will try again with the next change.", capitalised(e.reason.words()));
    if others > 0 {
        body.push_str(" Some of Yap's other files couldn't be saved either.");
    }
    crate::alert::Alert {
        kind: KIND,
        icon: "save",
        title,
        body,
        action: Some(crate::alert::Action { id: RETRY, label: "Try again" }),
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
            Ok(()) => crate::alert::Alert {
                kind: KIND,
                icon: "save",
                title: "Saved".into(),
                body: "Everything Yap was keeping is saved now.".into(),
                action: None,
                timeout_ms: 4_000,
            },
            Err(e) => failed_alert(e, 0, true),
        };
        crate::alert::show(&app, alert);
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
#[cfg(debug_assertions)]
static STAGED: std::sync::Mutex<Vec<(String, u32)>> = std::sync::Mutex::new(Vec::new());

/// The next `count` writes of store `id` fail as if the disk were full
/// (0: none).
#[cfg(debug_assertions)]
pub fn stage_failures(id: &str, count: u32) {
    let mut staged = STAGED.lock().unwrap_or_else(|p| p.into_inner());
    staged.retain(|(s, _)| s != id);
    if count > 0 {
        staged.push((id.to_string(), count));
    }
}

/// Whether this write of store `id` fails (one staged failure used up).
#[cfg(debug_assertions)]
fn staged_failure(id: &str) -> bool {
    let mut staged = STAGED.lock().unwrap_or_else(|p| p.into_inner());
    let Some(i) = staged.iter().position(|(s, _)| s == id) else {
        return false;
    };
    staged[i].1 -= 1;
    if staged[i].1 == 0 {
        staged.remove(i);
    }
    true
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
        let e = SaveError { what: "your note", reason: Reason::DiskFull };
        assert_eq!(e.to_string(), "Couldn't save your note: the disk is full.");
        assert_eq!(String::from(SaveError { what: "your chat", reason: Reason::Locked }), "Couldn't save your chat: another program has the file locked.");
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
        assert_eq!(e, SaveError { what: "your note", reason: Reason::Locked });
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
        assert_eq!(e, SaveError { what: "your dictation history", reason: Reason::ReadOnly });
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

    #[test]
    fn try_again_rewrites_each_unsaved_store_and_reports_one_that_still_fails() {
        fn still_failing() -> Result<(), SaveError> {
            Err(SaveError { what: "your chat", reason: Reason::DiskFull })
        }
        static CHATS: Saver = Saver::new("test-chats", "your chat", still_failing);
        stage_failures("test-chats", 1);
        let dir = scratch("retry");
        assert!(CHATS.write(&dir.join("chats.json"), "{}").is_err());
        assert_eq!(retry_all(), Err(SaveError { what: "your chat", reason: Reason::DiskFull }));
        CHATS.write(&dir.join("chats.json"), "{}").unwrap();
        assert_eq!(retry_all(), Ok(()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

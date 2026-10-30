//! **`tasks.json`: the tasks the person adds, and the ones they archived**
//! (`tasks.rs` lists and ticks them). A file of its own, not new fields in
//! notes.json: the installed Yap and a newer dev build share one data
//! folder, and an older Yap drops fields of notes.json it doesn't know the
//! next time it saves it — the person's own tasks and their archive would go
//! with them. An older Yap never opens this file.
//!
//! - `tasks`: [`OwnTask`]s — added in a meeting's Tasks tab (`noteId`) or in
//!   the Tasks view with no meeting; their own tick and archive mark.
//! - `archived`: a summary's tasks taken out of the Tasks view, by their note
//!   and their words as written (a summary's tick stays in the summary).
//!
//! Saved through `saving.rs` (atomically; a failed save kept in memory,
//! written with the next change or Try again, said once); a file that
//! doesn't read is set aside (`config::quarantine_corrupt`), never written
//! over. A note's added tasks and archive marks go with the note
//! ([`forget_note`]). Unit tests keep a store per test thread.

#[cfg(not(test))]
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use super::tasks::OwnTask;

/// A summary task archived from the Tasks view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Archived {
    pub note_id: u64,
    /// The task's words as written in the summary (`Task::text`).
    pub text: String,
}

/// What tasks.json holds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStore {
    #[serde(default)]
    pub tasks: Vec<OwnTask>,
    #[serde(default)]
    pub archived: Vec<Archived>,
}

impl TaskStore {
    /// Whether note `note_id`'s summary task with these words is archived.
    pub fn is_archived(&self, note_id: u64, text: &str) -> bool {
        self.archived.iter().any(|a| a.note_id == note_id && a.text == text)
    }
}

/// tasks.json's saves.
pub static SAVER: crate::shell::saving::Saver = crate::shell::saving::Saver::new("tasks", "your tasks", save_now);

/// `Ok` when every task is on disk; else why not (the caller says it).
pub fn saved() -> Result<(), crate::shell::saving::SaveError> {
    SAVER.check()
}

fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    with_store(|s| save(s));
    saved()
}

#[cfg(not(test))]
static STATE: Mutex<Option<TaskStore>> = Mutex::new(None);

#[cfg(not(test))]
fn path() -> std::path::PathBuf {
    crate::config::data_dir().join("tasks.json")
}

/// tasks.json as it loads (`saving::Saver::load`): a file that isn't tasks
/// is set aside; one Yap couldn't read leaves the store empty in memory,
/// never written over until it reads (read again in [`with_store`]).
#[cfg(not(test))]
fn load() -> TaskStore {
    SAVER.load(&path())
}

#[cfg(not(test))]
fn save(store: &TaskStore) {
    // A model evaluation keeps everything in memory.
    if crate::ai::eval::active() {
        return;
    }
    let _ = SAVER.write_json(&path(), store, false);
}

#[cfg(not(test))]
fn with_store<R>(f: impl FnOnce(&mut TaskStore) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let store = guard.get_or_insert_with(load);
    if SAVER.any_unread() && SAVER.refresh(&path(), store) {
        save(store);
    }
    f(store)
}

#[cfg(test)]
thread_local! {
    static TEST_STORE: std::cell::RefCell<TaskStore> = std::cell::RefCell::new(TaskStore::default());
}

#[cfg(test)]
fn save(_: &TaskStore) {}

#[cfg(test)]
fn with_store<R>(f: impl FnOnce(&mut TaskStore) -> R) -> R {
    TEST_STORE.with(|s| f(&mut s.borrow_mut()))
}

/// The store as it is now.
pub fn snapshot() -> TaskStore {
    with_store(|s| s.clone())
}

/// Reset app (`privacy/reset.rs`): every task added and every archive mark
/// gone. How many tasks went.
pub fn reset() -> usize {
    with_store(|s| {
        let gone = s.tasks.len();
        *s = TaskStore::default();
        save(s);
        gone
    })
}

/// Change the store under its lock: `f` returns its result and whether it
/// changed anything (then it's saved). An error changes nothing.
pub fn modify<R>(f: impl FnOnce(&mut TaskStore) -> Result<(R, bool), String>) -> Result<R, String> {
    with_store(|s| {
        let (out, changed) = f(s)?;
        if changed {
            save(s);
        }
        Ok(out)
    })
}

/// Note `note_id` was deleted: its added tasks and archive marks go too
/// (note ids come round again).
pub fn forget_note(note_id: u64) {
    let _ = modify(|s| {
        let before = (s.tasks.len(), s.archived.len());
        s.tasks.retain(|t| t.note_id != Some(note_id));
        s.archived.retain(|a| a.note_id != note_id);
        Ok(((), before != (s.tasks.len(), s.archived.len())))
    });
}

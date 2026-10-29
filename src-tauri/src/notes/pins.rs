//! **Pinned notes** (2026-10-29): pinned from a note's menu or its row in
//! Notes, they come first in Notes and in their folder's list, with a pin
//! mark ([`arrange`]).
//!
//! Kept in `note_pins.json`, a file of its own, not a field in notes.json:
//! the installed Yap and a newer dev build share one data folder, and an
//! older Yap drops fields of notes.json it doesn't know when it saves — a
//! pin there would come and go. An older Yap never opens this file: its
//! notes list unpinned, and the pins are there again in this Yap. Each pin
//! names the note's id and `createdTs`, so a pin left by a note an older Yap
//! deleted never passes to a new note given its id. A note's pin goes with
//! it ([`forget`]). Saved through `saving.rs` (a failed save kept in
//! memory and said once); a file that doesn't read is set aside. Unit tests
//! keep a store per test thread.

#[cfg(not(test))]
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter};

/// One pinned note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pin {
    pub id: u64,
    /// The note's `createdTs`: which note with that id.
    pub created: u64,
    /// When it was pinned (unix ms).
    pub ms: u64,
}

/// What note_pins.json holds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pins {
    #[serde(default)]
    pins: Vec<Pin>,
}

/// note_pins.json's saves.
pub static SAVER: crate::shell::saving::Saver =
    crate::shell::saving::Saver::new("note-pins", "your pinned notes", save_now);

fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    with_store(|s| save(s));
    SAVER.check()
}

#[cfg(not(test))]
static STATE: Mutex<Option<Pins>> = Mutex::new(None);

#[cfg(not(test))]
fn path() -> std::path::PathBuf {
    crate::config::data_dir().join("note_pins.json")
}

#[cfg(not(test))]
fn load() -> Pins {
    let path = path();
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| {
            tracing::error!("note_pins.json failed to parse; set aside");
            crate::config::quarantine_corrupt(&path);
            Pins::default()
        }),
        Err(_) => Pins::default(),
    }
}

#[cfg(not(test))]
fn save(store: &Pins) {
    // A model evaluation keeps everything in memory.
    if crate::ai::eval::active() {
        return;
    }
    let _ = SAVER.write_json(&path(), store, false);
}

#[cfg(not(test))]
fn with_store<R>(f: impl FnOnce(&mut Pins) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    f(guard.get_or_insert_with(load))
}

#[cfg(test)]
thread_local! {
    static TEST_STORE: std::cell::RefCell<Pins> = std::cell::RefCell::new(Pins::default());
}

#[cfg(test)]
fn save(_: &Pins) {}

#[cfg(test)]
fn with_store<R>(f: impl FnOnce(&mut Pins) -> R) -> R {
    TEST_STORE.with(|s| f(&mut s.borrow_mut()))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Pin or unpin the note `id` created at `created`. Returns whether it changed.
pub fn set(id: u64, created: u64, pinned: bool) -> bool {
    with_store(|s| {
        let at = s.pins.iter().position(|p| p.id == id);
        let changed = match (at, pinned) {
            (Some(i), true) if s.pins[i].created == created => false,
            (Some(i), true) => {
                s.pins[i] = Pin { id, created, ms: now_ms() };
                true
            }
            (None, true) => {
                s.pins.push(Pin { id, created, ms: now_ms() });
                true
            }
            (Some(i), false) => {
                s.pins.remove(i);
                true
            }
            (None, false) => false,
        };
        if changed {
            save(s);
        }
        changed
    })
}

/// The pins as they are.
pub fn pins() -> Vec<Pin> {
    with_store(|s| s.pins.clone())
}

/// Note `id` was deleted: its pin goes too (note ids come round again).
pub fn forget(id: u64) {
    with_store(|s| {
        let before = s.pins.len();
        s.pins.retain(|p| p.id != id);
        if s.pins.len() != before {
            save(s);
        }
    });
}

/// The notes list (`notes::list`, newest-updated first), each item marked
/// `pinned` and the pinned ones first, each part in the order it had
/// (pure). A pin counts only for the note it was made on (its id and
/// `createdTs`).
pub fn order(items: Vec<Value>, pins: &[Pin]) -> Vec<Value> {
    let pinned = |v: &Value| {
        let id = v["id"].as_u64();
        let created = v["createdTs"].as_u64();
        pins.iter().any(|p| Some(p.id) == id && Some(p.created) == created)
    };
    let (mut first, rest): (Vec<Value>, Vec<Value>) = items
        .into_iter()
        .map(|mut v| {
            let on = pinned(&v);
            v["pinned"] = Value::Bool(on);
            v
        })
        .partition(|v| v["pinned"] == Value::Bool(true));
    first.extend(rest);
    first
}

/// The notes list with its pins ([`order`]).
pub fn arrange(list: Value) -> Value {
    match list {
        Value::Array(items) => Value::Array(order(items, &pins())),
        other => other,
    }
}

/// Pin or unpin a note (its menu, its row in Notes). Every window's list
/// follows (`yap-notes-changed`). Returns whether it's pinned now.
#[tauri::command]
pub fn note_pin(app: AppHandle, id: u64, pinned: bool) -> Result<bool, String> {
    let note = crate::notes::get(id).ok_or("Note not found")?;
    if set(id, note.created_ts, pinned) {
        tracing::info!(note_id = id, pinned, "Note pinned");
        let _ = app.emit("yap-notes-changed", ());
    }
    SAVER.check()?;
    Ok(pinned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn item(id: u64, created: u64) -> Value {
        json!({ "id": id, "createdTs": created, "title": format!("Note {id}") })
    }

    fn ids(items: &[Value]) -> Vec<u64> {
        items.iter().map(|v| v["id"].as_u64().unwrap()).collect()
    }

    #[test]
    fn pinned_notes_come_first_each_part_in_its_order() {
        // Newest-updated first, as notes::list gives them.
        let list = vec![item(5, 50), item(3, 30), item(9, 90), item(1, 10), item(7, 70)];
        let pins = [Pin { id: 1, created: 10, ms: 2 }, Pin { id: 9, created: 90, ms: 1 }];
        let out = order(list.clone(), &pins);
        assert_eq!(ids(&out), [9, 1, 5, 3, 7]);
        assert_eq!(out.iter().filter(|v| v["pinned"] == json!(true)).count(), 2);
        assert_eq!(out[2]["pinned"], json!(false));
        // A pin on another note with that id (one deleted by an older Yap): none.
        let stale = [Pin { id: 3, created: 1, ms: 1 }];
        assert_eq!(ids(&order(list.clone(), &stale)), [5, 3, 9, 1, 7]);
        assert_eq!(ids(&order(list, &[])), [5, 3, 9, 1, 7]);
    }

    #[test]
    fn a_pin_is_set_once_taken_back_and_goes_with_its_note() {
        assert!(set(4, 40, true));
        assert!(!set(4, 40, true), "pinned already");
        assert!(set(6, 60, true));
        assert!(set(4, 40, false));
        assert!(!set(4, 40, false));
        assert_eq!(pins().iter().map(|p| p.id).collect::<Vec<_>>(), [6]);
        // Another note given id 6 pins afresh.
        assert!(set(6, 61, true));
        assert_eq!(pins()[0].created, 61);
        forget(6);
        assert!(pins().is_empty());
        // A note deleted: its pin goes with it; the list marks the rest.
        let n = crate::notes::create("Keep", "x", "manual", "");
        let other = crate::notes::create("Top", "y", "manual", "");
        assert!(set(other.id, other.created_ts, true));
        let listed = arrange(crate::notes::list());
        assert_eq!((listed[0]["id"].as_u64(), listed[0]["pinned"].as_bool()), (Some(other.id), Some(true)));
        assert_eq!(listed[1]["pinned"], json!(false));
        crate::notes::delete(other.id);
        assert!(pins().is_empty());
        assert!(!crate::notes::store_json().contains("pinned"), "never in notes.json");
        crate::notes::delete(n.id);
        // The file's shape: what an older Yap never opens.
        let json = serde_json::to_value(Pins { pins: vec![Pin { id: 1, created: 2, ms: 3 }] }).unwrap();
        assert_eq!(json, json!({ "pins": [{ "id": 1, "created": 2, "ms": 3 }] }));
    }
}

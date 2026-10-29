//! **Folder rules for recurring meetings**: "Always file Weekly sync in
//! Design" — every note of that series goes to that folder: the one it was
//! asked from, the earlier ones when the person says so, and each new one as
//! it's made from the calendar ([`folder_for`], `calendar::note_for`) or tied
//! to its meeting when a recording starts ([`file_linked`],
//! `calendar::on_meeting_started`). "File this meeting in…" moves one note.
//!
//! **The series** is the calendar's recurring id, carried in a meeting's
//! key (`brief::series_of`): an iCal event's UID with its RRULE (a
//! recurring occurrence's key is `<connection>:<uid>:<original start>`) or
//! Google's `recurringEventId` (an occurrence's id is that id and its
//! original start, `abc123_20261005T140000Z`). Without the connection, so a
//! calendar connected again keeps its rules. A one-off meeting has none.
//!
//! **`meeting_folders.json`** (`{ rules: { <series>: { folder, title,
//! createdTs } } }`): a file of its own, not fields in notes.json or
//! calendar.json, which an older Yap sharing the data folder would drop on
//! its next save. An older Yap never reads it: notes it makes for the
//! series go to Meetings, and stay there (a rule never moves a note again
//! once it's been filed, nor one the person moved). Saved through
//! `saving.rs`; a file that doesn't read is set aside, never written over.
//! A folder renamed takes its rules along; a folder deleted drops them
//! (`notes::commands`). Reset app keeps it, like the folders themselves.
//! Logs ids and counts, never a title.

use std::collections::BTreeMap;
#[cfg(not(test))]
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::notes::Note;

/// One series' folder.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub folder: String,
    /// The meeting's name when the rule was made ("Weekly sync"), to show.
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub created_ts: u64,
}

/// What meeting_folders.json holds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rules {
    /// Series id → its folder.
    #[serde(default)]
    pub rules: BTreeMap<String, Rule>,
}

pub static SAVER: crate::shell::saving::Saver =
    crate::shell::saving::Saver::new("meetingFolders", "your meeting folder rules", save_now);

fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    with_rules(|r| save(r));
    SAVER.check()
}

#[cfg(not(test))]
static STATE: Mutex<Option<Rules>> = Mutex::new(None);

#[cfg(not(test))]
fn path() -> std::path::PathBuf {
    crate::config::data_dir().join("meeting_folders.json")
}

#[cfg(not(test))]
fn load() -> Rules {
    let path = path();
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| {
            tracing::error!("meeting_folders.json failed to parse; set aside");
            crate::config::quarantine_corrupt(&path);
            Rules::default()
        }),
        Err(_) => Rules::default(),
    }
}

#[cfg(not(test))]
fn save(rules: &Rules) {
    if crate::ai::eval::active() {
        return;
    }
    let _ = SAVER.write_json(&path(), rules, false);
}

#[cfg(not(test))]
fn with_rules<R>(f: impl FnOnce(&mut Rules) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    f(guard.get_or_insert_with(load))
}

#[cfg(test)]
thread_local! {
    static TEST_RULES: std::cell::RefCell<Rules> = std::cell::RefCell::new(Rules::default());
}

#[cfg(test)]
fn save(_: &Rules) {}

#[cfg(test)]
fn with_rules<R>(f: impl FnOnce(&mut Rules) -> R) -> R {
    TEST_RULES.with(|r| f(&mut r.borrow_mut()))
}

/// The series a meeting note belongs to (its calendar event's), if any.
pub fn series_of_note(note: &Note) -> Option<String> {
    note.event.as_ref().and_then(|e| super::brief::series_of(&e.key))
}

/// The folder new notes of the meeting with calendar key `key` go to, when
/// its series has a rule and the folder is still there (as the folder list
/// spells it).
pub fn folder_for(key: &str) -> Option<String> {
    let series = super::brief::series_of(key)?;
    let folder = with_rules(|r| r.rules.get(&series).map(|rule| rule.folder.clone()))?;
    crate::notes::find_folder(&folder)
}

/// Whether a rule may move `note` into `folder`: it isn't there already,
/// and it sits where Yap put it (Meetings, or Personal before a recording
/// filed it) — never one the person moved (`folder_chosen`) or put in a
/// folder of their own. Pure.
pub fn may_file(note: &Note, folder: &str) -> bool {
    let here = note.folder.trim();
    !here.eq_ignore_ascii_case(folder)
        && !note.folder_chosen
        && (here.is_empty() || here.eq_ignore_ascii_case(crate::notes::MEETINGS) || here.eq_ignore_ascii_case(crate::notes::PERSONAL))
}

/// Note `note_id` was just tied to calendar meeting `key` (a recording
/// started during it): into its series' folder when there's a rule and
/// [`may_file`]. Returns the folder it went to.
pub fn file_linked(note_id: u64, key: &str) -> Option<String> {
    let folder = folder_for(key)?;
    let note = crate::notes::get(note_id)?;
    if !may_file(&note, &folder) {
        return None;
    }
    crate::notes::update(note_id, None, None, Some(folder.clone()), None).ok()?;
    tracing::info!(note_id, "calendar: a meeting note filed by its series' rule");
    Some(folder)
}

/// The other notes of series `series` (besides `except`) that aren't in
/// `folder` yet, newest first. Pure.
pub fn others_outside(notes: &[Note], series: &str, folder: &str, except: u64) -> Vec<u64> {
    let mut found: Vec<&Note> = notes
        .iter()
        .filter(|n| n.id != except && !n.folder.trim().eq_ignore_ascii_case(folder))
        .filter(|n| series_of_note(n).as_deref() == Some(series))
        .collect();
    found.sort_by_key(|n| std::cmp::Reverse(n.event.as_ref().map_or(n.created_ts, |e| e.start)));
    found.iter().map(|n| n.id).collect()
}

/// A folder renamed (the Notes view): its rules follow.
pub fn folder_renamed(from: &str, to: &str) {
    with_rules(|r| {
        let mut changed = false;
        for rule in r.rules.values_mut().filter(|rule| rule.folder.eq_ignore_ascii_case(from)) {
            rule.folder = to.trim().to_string();
            changed = true;
        }
        if changed {
            save(r);
        }
    });
}

/// A folder deleted: its rules go with it (its notes went to Personal).
pub fn folder_deleted(name: &str) {
    with_rules(|r| {
        let before = r.rules.len();
        r.rules.retain(|_, rule| !rule.folder.eq_ignore_ascii_case(name));
        if r.rules.len() != before {
            save(r);
        }
    });
}

/// What a meeting note's menu offers (the Meetings view): its folder, its
/// series and that series' rule, if any.
pub fn view_of(note: &Note) -> Value {
    let series = series_of_note(note);
    let rule = series.as_ref().and_then(|s| with_rules(|r| r.rules.get(s).cloned()));
    json!({
        "folder": note.folder,
        "series": series.map(|_| note.event.as_ref().map(|e| e.title.clone()).filter(|t| !t.trim().is_empty()).unwrap_or_else(|| note.title.clone())),
        "rule": rule.map(|r| r.folder),
    })
}

fn notes_changed(app: &AppHandle, moved: &[u64]) {
    for id in moved {
        crate::notes::commands::note_changed(app, *id, "folders");
    }
    let _ = app.emit("yap-notes-changed", ());
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// "File this meeting in…" (`series` false) or "Always file {series}
/// in…" (`series` true) from meeting note `note_id`'s menu: the note goes
/// to `folder` (made if need be); with `series`, the rule is kept for every
/// note of the series to come. Returns `{ moved, series, others }`:
/// `others`, how many earlier notes of the series aren't in the folder yet
/// (the page asks before moving them, [`meeting_file_past`]).
#[tauri::command]
pub fn meeting_file(app: AppHandle, note_id: u64, folder: String, series: bool) -> Result<Value, String> {
    let folder = folder.trim().to_string();
    if folder.is_empty() {
        return Err("Pick a folder".to_string());
    }
    let note = crate::notes::get(note_id).ok_or("Note not found")?;
    let folder = crate::notes::find_folder(&folder).unwrap_or_else(|| {
        crate::notes::folder_create(&folder);
        folder.clone()
    });
    let moved = !note.folder.trim().eq_ignore_ascii_case(&folder);
    if moved {
        crate::notes::update(note_id, None, None, Some(folder.clone()), None)?;
    }
    let mut others = 0;
    let mut title = String::new();
    if series {
        let id = series_of_note(&note).ok_or("This meeting doesn't repeat: file it on its own instead.")?;
        title = note.event.as_ref().map(|e| e.title.clone()).filter(|t| !t.trim().is_empty()).unwrap_or_else(|| note.title.clone());
        let rule = Rule { folder: folder.clone(), title: title.clone(), created_ts: now_secs() };
        with_rules(|r| {
            r.rules.insert(id.clone(), rule);
            save(r);
        });
        SAVER.check()?;
        others = others_outside(&crate::notes::all(), &id, &folder, note_id).len();
        tracing::info!(note_id, others, "calendar: a meeting series filed in a folder");
    } else {
        tracing::info!(note_id, "calendar: a meeting note filed in a folder");
    }
    let changed: Vec<u64> = if moved { vec![note_id] } else { Vec::new() };
    notes_changed(&app, &changed);
    crate::notes::saved()?;
    Ok(json!({ "moved": moved, "folder": folder, "series": title, "others": others }))
}

/// The earlier notes of `note_id`'s series into its rule's folder too
/// (asked after "Always file…"). Returns how many moved.
#[tauri::command]
pub fn meeting_file_past(app: AppHandle, note_id: u64) -> Result<usize, String> {
    let note = crate::notes::get(note_id).ok_or("Note not found")?;
    let series = series_of_note(&note).ok_or("This meeting doesn't repeat.")?;
    let folder = with_rules(|r| r.rules.get(&series).map(|rule| rule.folder.clone())).ok_or("This meeting isn't filed in a folder any more.")?;
    let folder = crate::notes::find_folder(&folder).ok_or("That folder isn't there any more.")?;
    let ids = others_outside(&crate::notes::all(), &series, &folder, note_id);
    let mut moved = Vec::new();
    for id in ids {
        if crate::notes::update(id, None, None, Some(folder.clone()), None).is_ok() {
            moved.push(id);
        }
    }
    tracing::info!(note_id, moved = moved.len(), "calendar: earlier notes of a series filed");
    notes_changed(&app, &moved);
    crate::notes::saved()?;
    Ok(moved.len())
}

/// "Stop filing {series} automatically": the rule goes; the notes stay
/// where they are.
#[tauri::command]
pub fn meeting_unfile(app: AppHandle, note_id: u64) -> Result<(), String> {
    let note = crate::notes::get(note_id).ok_or("Note not found")?;
    let series = series_of_note(&note).ok_or("This meeting doesn't repeat.")?;
    let gone = with_rules(|r| {
        let gone = r.rules.remove(&series).is_some();
        if gone {
            save(r);
        }
        gone
    });
    if gone {
        tracing::info!(note_id, "calendar: a meeting series' folder rule removed");
        let _ = app.emit("yap-notes-changed", ());
    }
    Ok(SAVER.check()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(id: u64, folder: &str, key: Option<&str>, start: u64) -> Note {
        let mut n: Note = serde_json::from_value(json!({
            "id": id, "title": format!("Note {id}"), "createdTs": start, "updatedTs": start, "folder": folder,
        }))
        .unwrap();
        n.event = key.map(|k| crate::notes::NoteEvent {
            key: k.into(),
            title: "Weekly sync".into(),
            start,
            end: start + 1_800,
            description: String::new(),
            service: String::new(),
        });
        n
    }

    #[test]
    fn a_notes_series_is_its_calendar_series() {
        // iCal: the UID of a recurring occurrence; Google: the recurring
        // event's id before the original start; a one-off: none.
        assert_eq!(series_of_note(&note(1, "Meetings", Some("2:weekly@example.com:1791800000"), 1)).as_deref(), Some("weekly@example.com"));
        assert_eq!(series_of_note(&note(2, "Meetings", Some("1:abc123_20261005T140000Z"), 1)).as_deref(), Some("abc123"));
        assert_eq!(series_of_note(&note(3, "Meetings", Some("2:design-review@example.com"), 1)), None);
        assert_eq!(series_of_note(&note(4, "Meetings", None, 1)), None);
        // The same series connected again (another connection slot) matches.
        assert_eq!(
            series_of_note(&note(5, "Meetings", Some("7:abc123_20261012T140000Z"), 1)),
            series_of_note(&note(2, "Meetings", Some("1:abc123_20261005T140000Z"), 1))
        );
    }

    #[test]
    fn a_rule_files_only_notes_yap_placed() {
        let mut n = note(1, "Meetings", Some("1:abc_20261005T140000Z"), 1);
        assert!(may_file(&n, "Design"));
        assert!(!may_file(&n, "meetings")); // there already, any case
        n.folder = "Personal".into();
        assert!(may_file(&n, "Design"));
        // Moved by the person, or in a folder of their own: left alone.
        n.folder_chosen = true;
        assert!(!may_file(&n, "Design"));
        let mut theirs = note(2, "Hiring", Some("1:abc_20261005T140000Z"), 1);
        assert!(!may_file(&theirs, "Design"));
        theirs.folder = "Design".into();
        assert!(!may_file(&theirs, "Design"));
    }

    #[test]
    fn earlier_notes_of_the_series_outside_the_folder() {
        let notes = [
            note(1, "Meetings", Some("1:abc_20261005T140000Z"), 300),
            note(2, "Design", Some("1:abc_20260928T140000Z"), 200),
            note(3, "Meetings", Some("1:abc_20260921T140000Z"), 100),
            note(4, "Meetings", Some("1:other_20261005T140000Z"), 300),
            note(5, "Meetings", Some("2:abc"), 300),
            note(6, "Personal", None, 300),
        ];
        // Asked from note 1: note 3 (note 2 is in Design already; 4 is another
        // series, 5 a one-off, 6 no meeting).
        assert_eq!(others_outside(&notes, "abc", "Design", 1), [3]);
        assert_eq!(others_outside(&notes, "abc", "design", 3), [1]);
    }

    #[test]
    fn rules_follow_their_folder() {
        with_rules(|r| {
            r.rules.insert("abc".into(), Rule { folder: "Design".into(), title: "Weekly sync".into(), created_ts: 1 });
            r.rules.insert("xyz".into(), Rule { folder: "Hiring".into(), title: "Interviews".into(), created_ts: 1 });
        });
        folder_renamed("design", "Product design");
        assert_eq!(with_rules(|r| r.rules["abc"].folder.clone()), "Product design");
        folder_deleted("Hiring");
        assert_eq!(with_rules(|r| r.rules.keys().cloned().collect::<Vec<_>>()), ["abc"]);
        // On disk as an older Yap never reads it, the main value plain.
        let json = with_rules(|r| serde_json::to_value(&*r).unwrap());
        assert_eq!(json["rules"]["abc"]["folder"], "Product design");
    }
}

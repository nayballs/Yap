//! **First-meeting coach marks** (2026-10-29, Wispr Flow's notepad
//! pointers): in the person's first recorded meeting, two small pointers in
//! the notepad — "Take notes here" over My thoughts and "Try What did I
//! miss?" by the Ask bar's chip. Each shows once (counted when it shows,
//! whether it's dismissed or not), and either can be dismissed.
//!
//! "First recorded meeting": no other meeting note has talk in it (so
//! someone who recorded meetings before this Yap had the marks never sees
//! them). Counted in `hints.json` beside the push-to-talk hints
//! (`dictation/coaching.rs`), not config.json: Settings auto-saves its own
//! copy of the config and would write an old count back. "Show tips" off
//! (Settings → General → Notifications): none. Test runs: none, unless the
//! instance was started with `YAP_E2E_COACH_MARKS=1` (a mark over My
//! thoughts in every other spec's first meeting would get in their way).

use serde::Serialize;

use crate::dictation::coaching::Record;
use crate::notes::Note;

/// Which marks the notepad shows for a meeting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Marks {
    /// "Take notes here", over My thoughts.
    pub notes: bool,
    /// "Try What did I miss?", by the Ask bar's chip.
    pub catch_up: bool,
}

/// Whether meeting `note_id` is the person's first recorded one: no other
/// note has talk in its transcript. Pure.
pub fn first_meeting(notes: &[Note], note_id: u64) -> bool {
    !notes.iter().any(|n| n.id != note_id && n.transcript.iter().any(|s| s.is_talk()))
}

/// The marks due (pure): in a first meeting, each one not shown before.
pub fn due(rec: &Record, first: bool, tips: bool) -> Marks {
    if !first || !tips {
        return Marks::default();
    }
    Marks { notes: rec.notepad_notes == 0, catch_up: rec.notepad_catch_up == 0 }
}

/// `rec` after mark `kind` ("notes" | "catchUp") showed.
pub fn shown(mut rec: Record, kind: &str) -> Record {
    match kind {
        "notes" => rec.notepad_notes += 1,
        "catchUp" => rec.notepad_catch_up += 1,
        _ => {}
    }
    rec
}

/// Test runs show the marks only when asked to.
fn allowed() -> bool {
    !crate::shell::e2e::active() || std::env::var("YAP_E2E_COACH_MARKS").is_ok_and(|v| v.trim() == "1")
}

/// The coach marks due in the notepad for meeting `noteId`.
#[tauri::command]
pub fn notepad_coach_marks(note_id: u64) -> Marks {
    if !allowed() {
        return Marks::default();
    }
    let tips = crate::config::load().show_tips;
    let first = crate::notes::with_all(|notes| first_meeting(notes, note_id));
    due(&crate::dictation::coaching::record(), first, tips)
}

/// Coach mark `kind` ("notes" | "catchUp") showed: counted, so it never
/// shows again.
#[tauri::command]
pub fn notepad_coach_mark_shown(kind: String) {
    if !matches!(kind.as_str(), "notes" | "catchUp") {
        return;
    }
    tracing::info!(kind = %kind, "Notepad: a first-meeting coach mark showed");
    crate::dictation::coaching::update_record(|rec| shown(rec, &kind));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::TranscriptSegment;

    fn note(id: u64, talk: bool) -> Note {
        let mut n: Note = serde_json::from_value(serde_json::json!({ "id": id, "title": "", "content": "", "createdTs": 1, "updatedTs": 1, "noteType": "meeting" })).unwrap();
        if talk {
            n.transcript = vec![TranscriptSegment { source: "you".into(), text: "Hello everyone.".into(), ts: 5, ..Default::default() }];
        }
        n
    }

    #[test]
    fn the_marks_show_once_each_in_the_first_meeting_only() {
        let rec = Record::default();
        // The first meeting: both, the one recording having talk or not.
        assert!(first_meeting(&[note(1, true), note(2, false)], 1));
        assert_eq!(due(&rec, true, true), Marks { notes: true, catch_up: true });
        // Another meeting already had talk: never.
        assert!(!first_meeting(&[note(1, true), note(2, true)], 2));
        assert_eq!(due(&rec, false, true), Marks::default());
        // Tips off: none.
        assert_eq!(due(&rec, true, false), Marks::default());
        // Each counted once it showed, so never again.
        let rec = shown(rec, "notes");
        assert_eq!(due(&rec, true, true), Marks { notes: false, catch_up: true });
        let rec = shown(rec, "catchUp");
        assert_eq!(due(&rec, true, true), Marks::default());
        assert_eq!(shown(rec.clone(), "other"), rec);
    }

    #[test]
    fn an_older_yaps_hints_json_keeps_reading() {
        // Fields an older Yap doesn't know: it reads the file as before (and
        // drops them when it saves: a mark may then show once more).
        let rec: Record = serde_json::from_value(serde_json::json!({ "tap": 1, "notepadNotes": 1, "notepadCatchUp": 1 })).unwrap();
        assert_eq!((rec.tap, rec.notepad_notes, rec.notepad_catch_up), (1, 1, 1));
        let old: Record = serde_json::from_value(serde_json::json!({ "tap": 1 })).unwrap();
        assert_eq!((old.notepad_notes, old.notepad_catch_up), (0, 0));
    }
}

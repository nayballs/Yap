//! Meeting notes you can fix by hand: a transcript line's words, the
//! summary's words, and the rule that Yap never writes over either without
//! asking (see docs/architecture/notes.md, "Notes you can fix"; the tasks in
//! a summary are `tasks.rs`).
//!
//! - **A line** ([`transcript_line_edit`], the notepad and the Notes view:
//!   double-click, or Edit line in its menu). Only its words change: it
//!   keeps its side, voice, date and place on the recording's clock, so ▶
//!   still plays what was said. It's marked `edited`, with what Yap heard
//!   kept in `original` (an edit back to that clears both). Transcribe again
//!   keeps it (`meeting_audio/redo.rs`) and the voice separation never splits
//!   it (`speakers::split_line`). When the edit fixed a misheard word or name
//!   ("cube and eighties" → "Kubernetes"), "Suggest words from your
//!   corrections" offers it for the dictionary, by its own rules
//!   (`learn::offer_for_line`), in the window that made the edit.
//! - **The summary** ([`note_summary_update`]): its markdown, edited in
//!   place. A change of words marks it the person's (`summary_edited_ms`);
//!   ticking a task doesn't (`tasks::same_but_ticks`).
//! - **Writing a summary** ([`write_summary`], every run of the Actions
//!   engine: `commands::run_enhance`). Over a summary the person edited only
//!   when the run was told to replace it (the window asked first) and the
//!   edit came before the run read the note; else it's kept
//!   ([`SUMMARY_KEPT`]). The end of a meeting writes no summary over an
//!   edited one: the windows say what's new since it instead, with Update.
//!   Tasks ticked in the old summary stay ticked in the new one
//!   (`tasks::carry_ticks`). Whatever model writes it, on this PC or in the
//!   cloud, the person's edits are local data and never go silently.
//!
//! Logs: ids, indexes and counts only — never a line's or a summary's words.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::notes::{Note, TranscriptSegment};

/// What a summary run says when it left the person's edited summary alone
/// (`meeting_end.rs` reports it as the job's "kept" state).
pub const SUMMARY_KEPT: &str = "Your edited summary was kept: Yap didn't write over it.";

/// A line is one line of talk: a paste of a whole document isn't.
const MAX_LINE_CHARS: usize = 4_000;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `text` as one line: line breaks and runs of spaces become one space.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Change `seg`'s words to `text` (pure). Returns whether anything changed.
/// The first edit keeps what Yap heard in `original`; an edit back to it
/// makes the line Yap's again.
pub fn apply_line_edit(seg: &mut TranscriptSegment, text: &str) -> Result<bool, String> {
    if seg.dictated {
        return Err("A “You dictated here” marker has no words to edit".to_string());
    }
    let text = one_line(text);
    if text.is_empty() {
        return Err("A line needs some words".to_string());
    }
    if text.chars().count() > MAX_LINE_CHARS {
        return Err("That's too long for one line of a transcript".to_string());
    }
    if text == seg.text {
        return Ok(false);
    }
    if seg.edited {
        seg.text = text;
        if seg.text == seg.original {
            seg.edited = false;
            seg.original.clear();
        }
    } else {
        seg.original = std::mem::replace(&mut seg.text, text);
        seg.edited = true;
    }
    Ok(true)
}

/// The line a window means: where it showed it, and what it said there.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineRef {
    pub index: usize,
    pub source: String,
    pub ts: u64,
    #[serde(default)]
    pub start_ms: Option<u64>,
    /// Its words as the window shows them.
    pub text: String,
}

/// Where `line` is now: at its index if it's still there, else wherever the
/// same line went (the voice separation splits lines while a meeting
/// records, moving the ones after). `None`: it changed or went meanwhile.
pub fn find_line(transcript: &[TranscriptSegment], line: &LineRef) -> Option<usize> {
    let same = |s: &TranscriptSegment| {
        s.source == line.source && s.ts == line.ts && s.start_ms == line.start_ms && s.text == line.text && !s.dictated
    };
    if transcript.get(line.index).is_some_and(same) {
        return Some(line.index);
    }
    transcript.iter().position(same)
}

/// A line edited: where it is, how it is now, and the dictionary offer
/// for the window to show (`learn::offer_for_line`; answered with
/// `word_suggestion_respond`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineEdited {
    pub index: usize,
    pub line: TranscriptSegment,
    pub changed: bool,
    pub offer: Option<crate::learn::Prompt>,
}

/// Edit a transcript line's words (the notepad, the Notes view). Every
/// window follows it (`yap-note-changed` with `lineEdit {index, line}`).
#[tauri::command]
pub async fn transcript_line_edit(
    app: AppHandle,
    note_id: u64,
    line: LineRef,
    text: String,
    origin: Option<String>,
) -> Result<LineEdited, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (index, before, seg, changed) = crate::notes::modify(note_id, |note| {
            let at = find_line(&note.transcript, &line)
                .ok_or("That line changed meanwhile (it may have been transcribed again). Try again.")?;
            let before = note.transcript[at].text.clone();
            let mut seg = note.transcript[at].clone();
            let changed = apply_line_edit(&mut seg, &text)?;
            if changed {
                note.transcript[at] = seg.clone();
            }
            Ok(((at, before, seg, changed), changed))
        })?;
        if !changed {
            return Ok(LineEdited { index, line: seg, changed, offer: None });
        }
        tracing::info!(note_id, index, edited = seg.edited, "Transcript line edited");
        if let Some(n) = crate::notes::get(note_id) {
            let mut payload = crate::commands::note_changed_payload(&n, origin.as_deref().unwrap_or(""));
            payload["lineEdit"] = serde_json::json!({ "index": index, "line": seg });
            let _ = app.emit("yap-note-changed", payload);
        }
        let offer = crate::learn::offer_for_line(&before, &seg.text);
        Ok(LineEdited { index, line: seg, changed, offer })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The summary's markdown changed to `text` by the person (pure): a change
/// of words makes it theirs (`now` = when, unix ms), a change of ticks
/// alone doesn't; emptied, the note has no summary. Returns whether
/// anything changed.
pub fn apply_summary_edit(note: &mut Note, text: &str, now: u64) -> bool {
    if text == note.enhanced_content {
        return false;
    }
    if text.trim().is_empty() {
        note.enhanced_content.clear();
        note.enhanced_at_hash.clear();
        note.summary_edited_ms = 0;
        note.summary_ts = 0;
        return true;
    }
    let was_empty = note.enhanced_content.trim().is_empty();
    let ticks_only = !was_empty && crate::tasks::same_but_ticks(&note.enhanced_content, text);
    note.enhanced_content = text.to_string();
    if !ticks_only {
        note.summary_edited_ms = now.max(1);
    }
    if was_empty {
        // A summary the person wrote from scratch: made from the notes as they are.
        note.enhanced_at_hash = crate::notes::content_hash(&note.content);
    }
    true
}

/// What the windows know of a note's summary after a change: its words,
/// whether the person edited them, and when its input was read.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryState {
    pub enhanced_content: String,
    pub summary_edited_ms: u64,
    pub summary_ts: u64,
}

impl SummaryState {
    fn of(note: &Note) -> Self {
        SummaryState {
            enhanced_content: note.enhanced_content.clone(),
            summary_edited_ms: note.summary_edited_ms,
            summary_ts: note.summary_ts,
        }
    }
}

/// Tell every window note `id`'s summary changed (`yap-note-changed` with
/// the summary's fields; `origin` = the window that changed it).
pub(crate) fn summary_changed(app: &AppHandle, id: u64, origin: &str) {
    if let Some(n) = crate::notes::get(id) {
        let mut payload = crate::commands::note_changed_payload(&n, origin);
        payload["enhancedContent"] = serde_json::json!(n.enhanced_content);
        payload["summaryEditedMs"] = serde_json::json!(n.summary_edited_ms);
        payload["summaryTs"] = serde_json::json!(n.summary_ts);
        let _ = app.emit("yap-note-changed", payload);
    }
}

/// Edit a note's summary (its markdown, from the Summary tab's editor).
#[tauri::command]
pub fn note_summary_update(app: AppHandle, id: u64, text: String, origin: Option<String>) -> Result<SummaryState, String> {
    let state = crate::notes::modify(id, |note| {
        let changed = apply_summary_edit(note, &text, now_ms());
        Ok((SummaryState::of(note), changed))
    })?;
    tracing::info!(note_id = id, edited = state.summary_edited_ms != 0, len = state.enhanced_content.len(), "Summary edited");
    summary_changed(&app, id, origin.as_deref().unwrap_or(""));
    Ok(state)
}

/// May a summary whose input was read at `read_ms` go over the one there
/// now (edited by the person at `edited_ms`, 0 = never)? Yes over Yap's
/// own; over the person's only when the run was told to `replace` it and
/// the edit came before the run read the note.
pub fn may_replace(edited_ms: u64, read_ms: u64, replace: bool) -> bool {
    edited_ms == 0 || (replace && edited_ms < read_ms)
}

/// Before a summary run spends a model call: `Err(SUMMARY_KEPT)` when the
/// note's summary is the person's and the run wasn't told to replace it.
pub fn keeps_summary(note: &Note, replace: bool) -> Result<(), String> {
    if note.summary_edited_ms != 0 && !replace {
        return Err(SUMMARY_KEPT.to_string());
    }
    Ok(())
}

/// The date (unix seconds) of the newest talk line in `note`: what a
/// summary read now has heard up to.
pub fn newest_talk(note: &Note) -> u64 {
    note.transcript
        .iter()
        .filter(|s| s.is_talk() && !s.text.trim().is_empty())
        .map(|s| s.ts)
        .max()
        .unwrap_or(0)
}

/// Store a summary Yap wrote (see the module notes): `at_hash` is the typed
/// notes' staleness mark, `read_ms` when the run read the note and
/// `heard_to` its newest talk line's date ([`newest_talk`]), all taken
/// before the model was called. "New since this summary" counts lines dated
/// after both (a line it read is never new, whatever its date). Tasks
/// ticked in the summary there now stay ticked. `Err(SUMMARY_KEPT)` when the
/// person's summary stays.
pub fn write_summary(id: u64, text: &str, at_hash: &str, read_ms: u64, heard_to: u64, replace: bool) -> Result<(), String> {
    crate::notes::modify(id, |note| {
        if !may_replace(note.summary_edited_ms, read_ms, replace) {
            return Err(SUMMARY_KEPT.to_string());
        }
        note.enhanced_content = crate::tasks::carry_ticks(&note.enhanced_content, text);
        note.enhanced_at_hash = at_hash.to_string();
        note.summary_edited_ms = 0;
        note.summary_ts = (read_ms / 1000).max(heard_to);
        Ok(((), true))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn seg(source: &str, text: &str, ts: u64) -> TranscriptSegment {
        TranscriptSegment {
            source: source.into(),
            text: text.into(),
            ts,
            start_ms: Some(ts * 1_000),
            end_ms: Some(ts * 1_000 + 900),
            ..Default::default()
        }
    }

    #[test]
    fn a_line_edit_keeps_what_was_heard_and_its_place() {
        let mut s = seg("them", "we deploy on cube and eighties", 10);
        s.speaker = "them-1".into();
        s.words = vec![[0, 100], [100, 200], [200, 300], [300, 400], [400, 500], [500, 600]];
        assert_eq!(apply_line_edit(&mut s, "  we deploy on\nKubernetes "), Ok(true));
        assert_eq!(s.text, "we deploy on Kubernetes");
        assert!(s.edited);
        assert_eq!(s.original, "we deploy on cube and eighties");
        // Its time, voice and audio stay: ▶ plays what was said.
        assert_eq!((s.ts, s.start_ms, s.end_ms, s.speaker.as_str()), (10, Some(10_000), Some(10_900), "them-1"));
        assert_eq!(s.words.len(), 6);
        // A second edit keeps the first original.
        assert_eq!(apply_line_edit(&mut s, "We deploy on Kubernetes."), Ok(true));
        assert_eq!(s.original, "we deploy on cube and eighties");
        // The same words again: nothing to do.
        assert_eq!(apply_line_edit(&mut s, "We deploy on Kubernetes."), Ok(false));
        // Back to what Yap heard: Yap's line again.
        assert_eq!(apply_line_edit(&mut s, "we deploy on cube and eighties"), Ok(true));
        assert!(!s.edited && s.original.is_empty());
        // No words, a marker: refused, the line untouched.
        assert!(apply_line_edit(&mut s, " \n ").is_err());
        assert_eq!(s.text, "we deploy on cube and eighties");
        let mut marker = TranscriptSegment::dictation_marker(5);
        assert!(apply_line_edit(&mut marker, "hello").is_err());
        assert!(apply_line_edit(&mut s, &"word ".repeat(1_000)).is_err());
    }

    #[test]
    fn an_edited_line_reaches_disk_only_when_edited() {
        let mut s = seg("you", "hello", 1);
        let plain = serde_json::to_value(&s).unwrap();
        assert!(plain.get("edited").is_none() && plain.get("original").is_none());
        apply_line_edit(&mut s, "Hello, Priya").unwrap();
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["edited"], json!(true));
        assert_eq!(v["original"], json!("hello"));
        let back: TranscriptSegment = serde_json::from_value(v).unwrap();
        assert!(back.edited && back.is_talk());
    }

    #[test]
    fn older_notes_load_unchanged() {
        // A note from before edited lines, edited summaries and tasks.
        let old = json!({
            "id": 7, "title": "Supplier call", "content": "ask about prices",
            "enhancedContent": "## Action plan\n### You\n- [ ] send the quote",
            "enhancedAtHash": "16:ask about prices", "noteType": "meeting", "folder": "Meetings",
            "transcript": [{"source": "them", "text": "hi", "ts": 5, "startMs": 0, "endMs": 900}],
            "participants": ["Priya"], "digests": [], "source": "meeting",
            "createdTs": 1, "updatedTs": 2
        });
        let note: Note = serde_json::from_value(old.clone()).unwrap();
        assert_eq!((note.summary_edited_ms, note.summary_ts), (0, 0));
        assert!(!note.transcript[0].edited && note.transcript[0].original.is_empty());
        let back = serde_json::to_value(&note).unwrap();
        for key in ["summaryEditedMs", "summaryTs"] {
            assert!(back.get(key).is_none(), "{key}");
        }
        assert!(back["transcript"][0].get("edited").is_none());
        // Every field it had reads back as it was.
        for (k, v) in old.as_object().unwrap() {
            assert_eq!(&back[k], v, "{k}");
        }
    }

    #[test]
    fn the_window_finds_its_line_even_when_lines_moved() {
        let mut t = vec![seg("you", "a", 1), seg("them", "b", 2), seg("them", "c", 3)];
        let line = |index, s: &TranscriptSegment| LineRef {
            index,
            source: s.source.clone(),
            ts: s.ts,
            start_ms: s.start_ms,
            text: s.text.clone(),
        };
        let c = line(2, &t[2]);
        assert_eq!(find_line(&t, &c), Some(2));
        // The voice separation split line 1 in two: "c" moved on.
        t.insert(2, seg("them", "b2", 2));
        assert_eq!(find_line(&t, &c), Some(3));
        // Its words changed meanwhile (another window): not found.
        t[3].text = "see".into();
        assert_eq!(find_line(&t, &c), None);
        // A marker is never the line.
        t.push(TranscriptSegment::dictation_marker(9));
        let m = LineRef { index: 4, source: "you".into(), ts: 9, start_ms: None, text: String::new() };
        assert_eq!(find_line(&t, &m), None);
    }

    fn summarised(text: &str) -> Note {
        let mut n: Note = serde_json::from_value(json!({
            "id": 1, "title": "Sync", "content": "notes", "createdTs": 1, "updatedTs": 1
        }))
        .unwrap();
        n.enhanced_content = text.into();
        n.summary_ts = 100;
        n
    }

    #[test]
    fn a_summary_edit_is_the_persons_but_a_tick_is_not() {
        let plan = "## Action plan\n### You\n- [ ] send the quote\n- [ ] book the room";
        let mut n = summarised(plan);
        assert!(!apply_summary_edit(&mut n, plan, 5));
        // Ticking in the editor: not an edit.
        assert!(apply_summary_edit(&mut n, "## Action plan\n### You\n- [x] send the quote\n- [ ] book the room", 5));
        assert_eq!(n.summary_edited_ms, 0);
        // Words changed: the person's now.
        assert!(apply_summary_edit(&mut n, "## Action plan\n### You\n- [x] send the quote to Priya\n- [ ] book the room", 6));
        assert_eq!(n.summary_edited_ms, 6);
        // Emptied: no summary at all.
        assert!(apply_summary_edit(&mut n, "  \n", 7));
        assert!(n.enhanced_content.is_empty() && n.enhanced_at_hash.is_empty());
        assert_eq!((n.summary_edited_ms, n.summary_ts), (0, 0));
        // Written from nothing: the person's, made from the notes as they are.
        assert!(apply_summary_edit(&mut n, "My own summary", 8));
        assert_eq!(n.summary_edited_ms, 8);
        assert_eq!(n.enhanced_at_hash, crate::notes::content_hash("notes"));
    }

    #[test]
    fn an_edited_summary_is_replaced_only_when_asked_and_not_edited_since() {
        assert!(may_replace(0, 10, false), "Yap's own summary");
        assert!(!may_replace(5, 10, false), "the person's, not asked");
        assert!(may_replace(5, 10, true), "asked, edited before the run read it");
        assert!(!may_replace(10, 10, true), "edited as the run read it");
        assert!(!may_replace(12, 10, true), "edited while the run went on");
        let mut n = summarised("x");
        assert!(keeps_summary(&n, false).is_ok());
        n.summary_edited_ms = 3;
        assert_eq!(keeps_summary(&n, false), Err(SUMMARY_KEPT.to_string()));
        assert!(keeps_summary(&n, true).is_ok());
    }

    #[test]
    fn writing_a_summary_keeps_the_persons_and_their_ticks() {
        let id = crate::notes::create("Sync", "notes", "manual", "").id;
        write_summary(id, "## Action plan\n### You\n- [ ] send the quote\n- [ ] book the room", "h1", 50_000, 0, false).unwrap();
        let n = crate::notes::get(id).unwrap();
        assert_eq!((n.summary_ts, n.enhanced_at_hash.as_str(), n.summary_edited_ms), (50, "h1", 0));
        // A line it read is never "new", whatever its date (a clock that's off).
        write_summary(id, "## Action plan\n### You\n- [ ] send the quote\n- [ ] book the room", "h1", 50_000, 64, false).unwrap();
        assert_eq!(crate::notes::get(id).unwrap().summary_ts, 64);
        // A tick, then Yap writes it again: still ticked.
        crate::notes::modify(id, |n| {
            n.enhanced_content = crate::tasks::set_done(&n.enhanced_content, 2, "send the quote", true)?;
            Ok(((), true))
        })
        .unwrap();
        write_summary(id, "Summary.\n\n## Action plan\n### You\n- [ ] Send the quote\n- [ ] book the room", "h2", 60_000, 0, false).unwrap();
        let n = crate::notes::get(id).unwrap();
        assert!(n.enhanced_content.contains("- [x] Send the quote"));
        assert!(n.enhanced_content.contains("- [ ] book the room"));
        // The person edits it: an automatic run leaves it.
        crate::notes::modify(id, |n| Ok(((), apply_summary_edit(n, "Mine.", 70_000)))).unwrap();
        assert_eq!(write_summary(id, "Yap's", "h3", 80_000, 0, false), Err(SUMMARY_KEPT.to_string()));
        // Asked to replace it (read after the edit): replaced, no longer theirs.
        write_summary(id, "Yap's", "h3", 80_000, 0, true).unwrap();
        let n = crate::notes::get(id).unwrap();
        assert_eq!((n.enhanced_content.as_str(), n.summary_edited_ms, n.summary_ts), ("Yap's", 0, 80));
        // Edited while a replacing run went on: the edit wins.
        crate::notes::modify(id, |n| Ok(((), apply_summary_edit(n, "Mine again.", 90_000)))).unwrap();
        assert_eq!(write_summary(id, "Late", "h4", 85_000, 0, true), Err(SUMMARY_KEPT.to_string()));
        assert_eq!(crate::notes::get(id).unwrap().enhanced_content, "Mine again.");
    }
}

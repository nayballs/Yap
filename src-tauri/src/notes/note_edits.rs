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
//!   engine: `notes::commands::run_enhance`). Over a summary the person edited only
//!   when the run was told to replace it (the window asked first) and the
//!   edit came before the run read the note; else it's kept
//!   ([`SUMMARY_KEPT`]). The end of a meeting writes no summary over an
//!   edited one: the windows say what's new since it instead, with Update.
//!   Tasks ticked in the old summary stay ticked in the new one
//!   (`tasks::carry_ticks`). Whatever model writes it, on this PC or in the
//!   cloud, the person's edits are local data and never go silently.
//! - **Back to Yap's summary** ([`note_summary_restore`], 2026-10-25): Yap's
//!   version is kept beside the person's (`summary_ai`, set by
//!   [`write_summary`]), so an edited summary can go back to it — the
//!   person's ticks carried over — with Undo (the window saves the person's
//!   version back). Editing the words back to Yap's makes it Yap's again.
//! - **Rating it** ([`note_summary_rate`]): 👍 / 👎 and, after a 👎, "What
//!   was wrong?" — in the note, on this PC only, never sent; a new summary
//!   clears it. For the model evaluations later.
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
    pub offer: Option<crate::dictionary::learn::Prompt>,
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
            let mut payload = crate::notes::commands::note_changed_payload(&n, origin.as_deref().unwrap_or(""));
            payload["lineEdit"] = serde_json::json!({ "index": index, "line": seg });
            let _ = app.emit("yap-note-changed", payload);
        }
        let offer = crate::dictionary::learn::offer_for_line(&before, &seg.text);
        Ok(LineEdited { index, line: seg, changed, offer })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The summary's markdown changed to `text` by the person (pure): a change
/// of words makes it theirs (`now` = when, unix ms), a change of ticks
/// alone doesn't, and words back to Yap's own (`summary_ai`) make it Yap's
/// again; emptied, the note has no summary. Returns whether anything
/// changed.
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
    let ticks_only = !was_empty && crate::notes::tasks::same_but_ticks(&note.enhanced_content, text);
    note.enhanced_content = text.to_string();
    if !note.summary_ai.is_empty() && crate::notes::tasks::same_but_ticks(&note.summary_ai, text) {
        note.summary_edited_ms = 0;
    } else if !ticks_only {
        note.summary_edited_ms = now.max(1);
    }
    if was_empty {
        // A summary the person wrote from scratch: made from the notes as they are.
        note.enhanced_at_hash = crate::notes::content_hash(&note.content);
    }
    true
}

/// What the windows know of a note's summary after a change: its words,
/// whether the person edited them, when its input was read, Yap's own
/// version (for "Back to Yap's summary") and the person's rating.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryState {
    pub enhanced_content: String,
    pub summary_edited_ms: u64,
    pub summary_ts: u64,
    pub summary_ai: String,
    pub summary_rating: Option<SummaryRating>,
}

impl SummaryState {
    fn of(note: &Note) -> Self {
        SummaryState {
            enhanced_content: note.enhanced_content.clone(),
            summary_edited_ms: note.summary_edited_ms,
            summary_ts: note.summary_ts,
            summary_ai: note.summary_ai.clone(),
            summary_rating: note.summary_rating.clone(),
        }
    }
}

/// Tell every window note `id`'s summary changed (`yap-note-changed` with
/// the summary's fields; `origin` = the window that changed it).
pub(crate) fn summary_changed(app: &AppHandle, id: u64, origin: &str) {
    if let Some(n) = crate::notes::get(id) {
        let mut payload = crate::notes::commands::note_changed_payload(&n, origin);
        payload["enhancedContent"] = serde_json::json!(n.enhanced_content);
        payload["summaryEditedMs"] = serde_json::json!(n.summary_edited_ms);
        payload["summaryTs"] = serde_json::json!(n.summary_ts);
        payload["summaryAi"] = serde_json::json!(n.summary_ai);
        payload["summaryRating"] = serde_json::json!(n.summary_rating);
        let _ = app.emit("yap-note-changed", payload);
    }
}

// ---- Back to Yap's summary ---------------------------------------------------------------

/// Whether note `note` offers "Back to Yap's summary": the person edited
/// Yap's summary, and Yap's is kept and says something else. Pure.
pub fn can_restore(note: &Note) -> bool {
    note.summary_edited_ms != 0
        && !note.summary_ai.trim().is_empty()
        && !crate::notes::tasks::same_but_ticks(&note.summary_ai, &note.enhanced_content)
}

/// "Back to Yap's summary" (pure): Yap's version in place of the person's,
/// the tasks they ticked still ticked (`tasks::carry_ticks`), Yap's again
/// (not edited). Returns the person's version for Undo (the window saves it
/// back through [`note_summary_update`]), or `None` when there's nothing to
/// go back to.
pub fn restore_ai_summary(note: &mut Note) -> Option<String> {
    if !can_restore(note) {
        return None;
    }
    let theirs = note.enhanced_content.clone();
    note.enhanced_content = crate::notes::tasks::carry_ticks(&theirs, &note.summary_ai);
    note.summary_edited_ms = 0;
    Some(theirs)
}

/// What "Back to Yap's summary" did: the summary now, and the person's
/// version it replaced (Undo puts it back).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Restored {
    pub state: SummaryState,
    pub previous: String,
}

/// Put Yap's summary back over the person's edited one (the notepad's and
/// the Notes view's "Back to Yap's summary"); every window follows. Undo is
/// [`note_summary_update`] with `previous`.
#[tauri::command]
pub fn note_summary_restore(app: AppHandle, id: u64, origin: Option<String>) -> Result<Restored, String> {
    use crate::notes::versions::{by, kind, How};
    let restored = by(How::named(kind::RESTORED, "Yap's summary"), || {
        crate::notes::modify(id, |note| match restore_ai_summary(note) {
            Some(previous) => Ok((Restored { state: SummaryState::of(note), previous }, true)),
            None => Err("There's no summary of Yap's to go back to".to_string()),
        })
    })?;
    tracing::info!(note_id = id, len = restored.state.enhanced_content.len(), "Summary: back to Yap's");
    summary_changed(&app, id, origin.as_deref().unwrap_or(""));
    crate::notes::saved()?;
    Ok(restored)
}

// ---- Rating a summary --------------------------------------------------------------------

/// The person's rating of Yap's summary: 👍 (`up`) or 👎 with, optionally,
/// what was wrong (`why`, ≤ [`WHY_MAX_CHARS`]). On this PC only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryRating {
    pub up: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub why: String,
    /// When (unix ms).
    pub ms: u64,
}

/// "What was wrong?" is a short note.
pub const WHY_MAX_CHARS: usize = 500;

/// Rate note `note`'s summary (pure): `rating` "up" | "down", or `None` to
/// take the rating back; `why` only with a 👎 (one paragraph, cut to
/// [`WHY_MAX_CHARS`]). No summary, nothing to rate.
pub fn apply_rating(note: &mut Note, rating: Option<&str>, why: &str, now: u64) -> Result<bool, String> {
    let next = match rating {
        None => None,
        Some(r @ ("up" | "down")) => {
            if note.enhanced_content.trim().is_empty() {
                return Err("There's no summary to rate yet".to_string());
            }
            let up = r == "up";
            let why = if up { String::new() } else { why.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(WHY_MAX_CHARS).collect() };
            Some(SummaryRating { up, why, ms: now.max(1) })
        }
        Some(other) => return Err(format!("A rating is up or down, not {other}")),
    };
    let same = match (&note.summary_rating, &next) {
        (Some(a), Some(b)) => a.up == b.up && a.why == b.why,
        (None, None) => true,
        _ => false,
    };
    if same {
        return Ok(false);
    }
    note.summary_rating = next;
    Ok(true)
}

/// 👍 / 👎 on a note's summary, and "What was wrong?" after a 👎 (the
/// notepad and the Notes view): kept in the note on this PC, never sent.
#[tauri::command]
pub fn note_summary_rate(
    app: AppHandle,
    id: u64,
    rating: Option<String>,
    why: Option<String>,
    origin: Option<String>,
) -> Result<SummaryState, String> {
    let state = crate::notes::modify(id, |note| {
        let changed = apply_rating(note, rating.as_deref(), why.as_deref().unwrap_or(""), now_ms())?;
        Ok((SummaryState::of(note), changed))
    })?;
    tracing::info!(
        note_id = id,
        rating = state.summary_rating.as_ref().map(|r| if r.up { "up" } else { "down" }).unwrap_or("none"),
        why_len = state.summary_rating.as_ref().map(|r| r.why.len()).unwrap_or(0),
        "Summary rated"
    );
    summary_changed(&app, id, origin.as_deref().unwrap_or(""));
    crate::notes::saved()?;
    Ok(state)
}

/// Edit a note's summary (its markdown, from the Summary tab's editor).
#[tauri::command]
pub fn note_summary_update(app: AppHandle, id: u64, text: String, origin: Option<String>) -> Result<SummaryState, String> {
    use crate::notes::versions::{by, kind, How};
    let state = by(How::new(kind::TYPED), || {
        crate::notes::modify(id, |note| {
            let changed = apply_summary_edit(note, &text, now_ms());
            Ok((SummaryState::of(note), changed))
        })
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
        note.enhanced_content = crate::notes::tasks::carry_ticks(&note.enhanced_content, text);
        note.enhanced_at_hash = at_hash.to_string();
        note.summary_edited_ms = 0;
        note.summary_ts = (read_ms / 1000).max(heard_to);
        // Yap's own, kept should the person edit it ("Back to Yap's
        // summary"); a rating was of the summary before.
        note.summary_ai = note.enhanced_content.clone();
        note.summary_rating = None;
        Ok(((), true))
    })
}

// ---- A summary rewritten, and its Undo (2026-10-29) -------------------------------------
//
// "Make notes longer" (`meeting_assist::meeting_longer`) writes Yap's summary
// again through `write_summary`; what was there before goes back with the
// window's Undo (`note_summary_put_back`): its words, Yap's own version
// beside them, and whether they were the person's — the ticks made since
// carried over. (When note version history lands, the summary before a
// rewrite is a version of its own, and Undo restores that version.)

/// A note's summary as it was before Yap rewrote it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SummaryBefore {
    pub enhanced_content: String,
    #[serde(default)]
    pub summary_ai: String,
    #[serde(default)]
    pub summary_edited_ms: u64,
    #[serde(default)]
    pub summary_ts: u64,
}

impl SummaryBefore {
    pub fn of(note: &Note) -> Self {
        SummaryBefore {
            enhanced_content: note.enhanced_content.clone(),
            summary_ai: note.summary_ai.clone(),
            summary_edited_ms: note.summary_edited_ms,
            summary_ts: note.summary_ts,
        }
    }
}

/// A note's summary state now (every window's copy follows it).
pub fn state_of(note: &Note) -> SummaryState {
    SummaryState::of(note)
}

/// The summary before a rewrite put back (pure): its words with the tasks
/// ticked since still ticked (`tasks::carry_ticks`), Yap's version and the
/// person's mark as they were. Returns whether anything changed.
pub fn put_back(note: &mut Note, before: &SummaryBefore) -> bool {
    let text = crate::notes::tasks::carry_ticks(&note.enhanced_content, &before.enhanced_content);
    let changed = text != note.enhanced_content || before.summary_ai != note.summary_ai || before.summary_edited_ms != note.summary_edited_ms;
    note.enhanced_content = text;
    note.summary_ai = before.summary_ai.clone();
    note.summary_edited_ms = before.summary_edited_ms;
    note.summary_ts = before.summary_ts;
    changed
}

/// Undo a rewrite of the summary ("Make notes longer"'s toast): the summary
/// as it was ([`SummaryBefore`]); every window follows.
#[tauri::command]
pub fn note_summary_put_back(app: AppHandle, id: u64, before: SummaryBefore, origin: Option<String>) -> Result<SummaryState, String> {
    let state = crate::notes::modify(id, |note| {
        let changed = put_back(note, &before);
        Ok((SummaryState::of(note), changed))
    })?;
    tracing::info!(note_id = id, len = state.enhanced_content.len(), "Summary: put back as it was");
    summary_changed(&app, id, origin.as_deref().unwrap_or(""));
    crate::notes::saved()?;
    Ok(state)
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
            n.enhanced_content = crate::notes::tasks::set_done(&n.enhanced_content, 2, "send the quote", true)?;
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

    #[test]
    fn back_to_yaps_summary_keeps_the_ticks_and_undo_puts_yours_back() {
        let id = crate::notes::create("Sync", "notes", "manual", "").id;
        let yaps = "## Recap\nThe launch.\n\n## Action plan\n### You\n- [ ] send the quote\n- [ ] book the room";
        write_summary(id, yaps, "h1", 50_000, 0, false).unwrap();
        let n = crate::notes::get(id).unwrap();
        assert_eq!(n.summary_ai, yaps);
        assert!(!can_restore(&n), "Yap's own: nothing to go back to");
        // The person rewrites it and ticks a task.
        let theirs = "## Recap\nThe launch, moved to May.\n\n## Action plan\n### You\n- [x] send the quote\n- [ ] book the room";
        crate::notes::modify(id, |n| Ok(((), apply_summary_edit(n, theirs, 60_000)))).unwrap();
        let mut n = crate::notes::get(id).unwrap();
        assert!(n.summary_edited_ms != 0 && can_restore(&n));
        // Back to Yap's: its words, their tick, Yap's again.
        assert_eq!(restore_ai_summary(&mut n).as_deref(), Some(theirs));
        assert!(n.enhanced_content.starts_with("## Recap\nThe launch.\n"), "{}", n.enhanced_content);
        assert!(n.enhanced_content.contains("- [x] send the quote"));
        assert_eq!(n.summary_edited_ms, 0);
        assert!(restore_ai_summary(&mut n).is_none(), "once");
        // Undo: their version saved back, theirs again.
        assert!(apply_summary_edit(&mut n, theirs, 70_000));
        assert_eq!((n.enhanced_content.as_str(), n.summary_edited_ms), (theirs, 70_000));
        // Words back to Yap's by hand: Yap's again, ticks aside.
        assert!(apply_summary_edit(&mut n, &yaps.replace("- [ ] send", "- [x] send"), 80_000));
        assert_eq!(n.summary_edited_ms, 0);
        // A summary from before Yap kept its own: nothing to go back to.
        let mut old = summarised("Mine.");
        old.summary_edited_ms = 5;
        assert!(!can_restore(&old) && restore_ai_summary(&mut old).is_none());
        // An older Yap on the same data saves the note without the fields it
        // doesn't know: Yap's version and the rating go, the person's edited
        // summary stays where every Yap reads it.
        crate::notes::modify(id, |n| Ok(((), apply_summary_edit(n, theirs, 90_000) || apply_rating(n, Some("down"), "x", 1)?))).unwrap();
        let mut v = serde_json::to_value(crate::notes::get(id).unwrap()).unwrap();
        assert!(v.get("summaryAi").is_some());
        v.as_object_mut().unwrap().remove("summaryAi");
        v.as_object_mut().unwrap().remove("summaryRating");
        let back: Note = serde_json::from_value(v).unwrap();
        assert_eq!(back.enhanced_content, theirs);
        assert!(back.summary_ai.is_empty() && back.summary_rating.is_none() && !can_restore(&back));
    }

    #[test]
    fn a_rating_stays_on_this_pc_with_its_reason_and_a_new_summary_clears_it() {
        let mut n = summarised("## Action plan\n### You\n- [ ] send the quote");
        assert_eq!(apply_rating(&mut n, Some("up"), "ignored for a 👍", 5), Ok(true));
        assert_eq!(n.summary_rating, Some(SummaryRating { up: true, why: String::new(), ms: 5 }));
        assert_eq!(apply_rating(&mut n, Some("up"), "", 6), Ok(false), "the same again");
        assert_eq!(apply_rating(&mut n, Some("down"), "  Missed   Priya's task\n", 7), Ok(true));
        assert_eq!(n.summary_rating.as_ref().unwrap().why, "Missed Priya's task");
        assert_eq!(apply_rating(&mut n, Some("down"), &"x".repeat(2_000), 8), Ok(true));
        assert_eq!(n.summary_rating.as_ref().unwrap().why.chars().count(), WHY_MAX_CHARS);
        assert!(apply_rating(&mut n, Some("meh"), "", 9).is_err());
        assert_eq!(apply_rating(&mut n, None, "", 9), Ok(true));
        assert!(n.summary_rating.is_none());
        // Nothing to rate without a summary.
        assert!(apply_rating(&mut summarised(""), Some("down"), "", 1).is_err());
        // Stored with the note; a new summary from Yap clears it.
        let id = crate::notes::create("Sync", "notes", "manual", "").id;
        write_summary(id, "Summary one.", "h", 1_000, 0, false).unwrap();
        crate::notes::modify(id, |n| Ok(((), apply_rating(n, Some("down"), "Too long", 2)?))).unwrap();
        let v = serde_json::to_value(crate::notes::get(id).unwrap()).unwrap();
        assert_eq!(v["summaryRating"], json!({ "up": false, "why": "Too long", "ms": 2 }));
        write_summary(id, "Summary two.", "h", 3_000, 0, false).unwrap();
        assert!(crate::notes::get(id).unwrap().summary_rating.is_none());
    }

    #[test]
    fn a_rewritten_summary_goes_back_as_it_was_with_the_ticks_made_since() {
        let id = crate::notes::create("Sync", "notes", "manual", "").id;
        let yaps = "## Recap
The launch.

## Action plan
### You
- [ ] send the quote
- [ ] book the room";
        write_summary(id, yaps, "h1", 50_000, 0, false).unwrap();
        let before = SummaryBefore::of(&crate::notes::get(id).unwrap());
        // "Make notes longer" writes it again; the person ticks a task in it.
        let longer = "## Recap
The launch, with the venue and the budget.

## Action plan
### You
- [ ] send the quote
- [ ] book the room";
        write_summary(id, longer, "h1", 60_000, 0, false).unwrap();
        crate::notes::modify(id, |n| Ok(((), apply_summary_edit(n, &longer.replace("- [ ] book", "- [x] book"), 61_000)))).unwrap();
        let mut n = crate::notes::get(id).unwrap();
        assert_eq!(n.summary_ai, longer);
        // Undo: the words before, Yap's own as before, the tick kept.
        assert!(put_back(&mut n, &before));
        assert!(n.enhanced_content.starts_with("## Recap
The launch.
"));
        assert!(n.enhanced_content.contains("- [x] book the room"));
        assert_eq!((n.summary_ai.as_str(), n.summary_edited_ms), (yaps, 0));
        assert!(!can_restore(&n), "Yap's own again, ticks aside");
        // Again: nothing changes.
        assert!(!put_back(&mut n, &before));
    }
}

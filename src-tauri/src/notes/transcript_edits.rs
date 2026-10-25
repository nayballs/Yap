//! **A fixed transcript line, the rest** (after `note_edits.rs`, which fixes
//! a line's words): put a line back to what Yap heard, delete one (with
//! Undo), or split one in two at the cursor. The meeting notepad and the
//! Notes view, in a line's menu (and Shift+Enter while editing one: split
//! there).
//!
//! - **Revert** ([`revert_line`]): an edited line's words go back to
//!   `original`; it's Yap's line again (Transcribe again may replace it).
//! - **Delete** ([`remove_line`]): its words go — from the note, every view,
//!   the summaries' inputs and search — but its side and place on the
//!   recording's clock stay as a mark with no text (`removed`), so
//!   Transcribe again and the refine after a meeting (`meeting_audio/
//!   redo.rs`) never bring them back from the audio. The rolling digests
//!   from the one that read it on are dropped (`drop_digests_from`): they
//!   were written with its words, and the next summary writes them again
//!   from what's left. **Undo** ([`restore_line`]) puts the line back as it
//!   was, from this run's memory of it (never from disk: once Yap closes,
//!   the words are gone for good).
//! - **Split** ([`split_line`]): the line becomes two at a word (the one
//!   the cursor is in or before starts the second), each keeping its side,
//!   voice and lock, and its own stretch of the recording's clock (by the
//!   words' times when the speech model gave them, else shared out by
//!   length), so ▶ plays each part. Both parts are the person's (`edited`,
//!   each with its share of what Yap heard as `original`): Transcribe again
//!   keeps them as they are and the voice separation never splits them
//!   again. Stop marks and digests after it move along (`notes::
//!   replace_line`; a digest that read the line reads both parts). Not
//!   while the meeting records, nor while its summary or a transcription
//!   of it runs (they hold places in its transcript).
//!
//! Logs: ids, indexes and counts — never a line's words.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::notes::note_edits::{find_line, LineRef};
use crate::notes::{Note, TranscriptSegment};

/// A marker (a dictation, a failed stretch, the mic disconnected) or a
/// deleted line: none of these edits apply.
fn is_marker(seg: &TranscriptSegment) -> bool {
    seg.dictated || seg.failed || seg.mic.is_some() || seg.removed
}

// ---- revert ----

/// `seg`'s words back to what Yap heard (pure). `Ok(false)`: nothing to
/// put back.
pub fn revert_line(seg: &mut TranscriptSegment) -> Result<bool, String> {
    if is_marker(seg) {
        return Err("That isn't a line of the transcript".to_string());
    }
    if !seg.edited {
        return Ok(false);
    }
    if !seg.original.trim().is_empty() {
        seg.text = std::mem::take(&mut seg.original);
    }
    seg.original.clear();
    seg.edited = false;
    Ok(true)
}

// ---- delete, and Undo ----

/// The rolling digests from the one that read line `at` on: they go (the
/// digests stay one run from the start; the next summary writes the rest
/// again from the lines as they are).
pub fn drop_digests_from(note: &mut Note, at: usize) -> usize {
    let before = note.digests.len();
    note.digests.retain(|d| d.to_seg <= at);
    before - note.digests.len()
}

/// Delete line `at` of `note` (pure): its words go, its side and place stay
/// as a mark (see the module notes). Returns the line as it was, for Undo.
pub fn remove_line(note: &mut Note, at: usize) -> Result<TranscriptSegment, String> {
    let seg = note.transcript.get(at).ok_or("That line isn't there any more")?;
    if is_marker(seg) {
        return Err("That isn't a line of the transcript".to_string());
    }
    let old = seg.clone();
    note.transcript[at] = TranscriptSegment {
        source: old.source.clone(),
        ts: old.ts,
        start_ms: old.start_ms,
        end_ms: old.end_ms,
        removed: true,
        ..Default::default()
    };
    drop_digests_from(note, at);
    Ok(old)
}

/// Put `line` back where it was deleted (pure): the mark at `at`, else the
/// mark of the same side, date and place wherever it moved.
pub fn restore_line(note: &mut Note, at: usize, line: &TranscriptSegment) -> Result<usize, String> {
    let same = |s: &TranscriptSegment| s.removed && s.source == line.source && s.ts == line.ts && s.start_ms == line.start_ms;
    let at = if note.transcript.get(at).is_some_and(same) {
        at
    } else {
        note.transcript.iter().position(same).ok_or("That line can't be put back any more")?
    };
    note.transcript[at] = line.clone();
    // Digests written while it was gone don't have its words.
    drop_digests_from(note, at);
    Ok(at)
}

/// Lines deleted in this run, for Undo: (note, the line as it was). The
/// newest last; a few dozen at most.
fn deleted() -> std::sync::MutexGuard<'static, Vec<(u64, TranscriptSegment)>> {
    static DELETED: Mutex<Vec<(u64, TranscriptSegment)>> = Mutex::new(Vec::new());
    DELETED.lock().unwrap_or_else(|p| p.into_inner())
}

const UNDO_KEEP: usize = 32;

// ---- split ----

/// Each word of `text` with its place, `(start, end)` ms on the recording's
/// clock: `times` (ms from `start`) when there's one per word, else the
/// line's stretch shared out by length.
fn places(words: &[&str], times: &[[u32; 2]], (start, end): (u64, u64)) -> Vec<(u64, u64)> {
    if times.len() == words.len() {
        return times.iter().map(|[s, e]| (start + *s as u64, start + *e as u64)).collect();
    }
    let total: u64 = words.iter().map(|w| w.chars().count() as u64 + 1).sum::<u64>().max(1);
    let span = end.saturating_sub(start);
    let mut at = 0;
    words
        .iter()
        .map(|w| {
            let from = start + span * at / total;
            at += w.chars().count() as u64 + 1;
            (from, start + span * at / total)
        })
        .collect()
}

/// `seg` split before its word `k` (pure): two lines, each keeping its side,
/// voice and lock, and its own stretch (see the module notes). Refused for a
/// marker, and for a split with no words on one side.
pub fn split_line(seg: &TranscriptSegment, k: usize) -> Result<[TranscriptSegment; 2], String> {
    if is_marker(seg) {
        return Err("That isn't a line of the transcript".to_string());
    }
    let words: Vec<&str> = seg.text.split_whitespace().collect();
    if k == 0 || k >= words.len() {
        return Err("Put the cursor between two words to split the line there".to_string());
    }
    let mut first = seg.clone();
    let mut second = seg.clone();
    first.text = words[..k].join(" ");
    second.text = words[k..].join(" ");
    // What Yap heard, shared the same way: the line's own words (unedited),
    // or its original's, split where the cut falls on the clock.
    let heard_text = if seg.edited && !seg.original.trim().is_empty() { seg.original.as_str() } else { seg.text.as_str() };
    let heard: Vec<&str> = heard_text.split_whitespace().collect();
    match (seg.start_ms, seg.end_ms) {
        (Some(start), Some(end)) => {
            let end = end.max(start);
            // Word times belong to what Yap heard (an edit keeps them).
            let heard_at = places(&heard, &seg.words, (start, end));
            let (cut, first_end) = if !seg.edited {
                (heard_at[k].0, heard_at[k - 1].1)
            } else {
                // The edited words' places, shared out by length.
                let shown = places(&words, &[], (start, end));
                (shown[k].0, shown[k - 1].1)
            };
            let cut = cut.clamp(start, end);
            let first_end = first_end.clamp(start, cut.max(start));
            // The heard words before the cut go with the first part.
            let n = if seg.edited { heard_at.iter().take_while(|&&(s, e)| (s + e) / 2 < cut).count() } else { k };
            let n = n.min(heard.len());
            first.original = heard[..n].join(" ");
            second.original = heard[n..].join(" ");
            first.start_ms = Some(start);
            first.end_ms = Some(first_end.max(start));
            second.start_ms = Some(cut);
            second.end_ms = Some(end.max(cut));
            second.ts = seg.ts + cut.saturating_sub(start) / 1000;
            let aligned = seg.words.len() == heard.len();
            first.words = if aligned { seg.words[..n].to_vec() } else { Vec::new() };
            second.words = if aligned {
                seg.words[n..]
                    .iter()
                    .map(|[s, e]| {
                        let rebase = |t: u32| (start + t as u64).saturating_sub(cut) as u32;
                        [rebase(*s), rebase(*e)]
                    })
                    .collect()
            } else {
                Vec::new()
            };
        }
        _ => {
            // A line from before lines had places: both keep its date.
            let n = (heard.len() * k).div_ceil(words.len()).min(heard.len());
            first.original = heard[..n].join(" ");
            second.original = heard[n..].join(" ");
            first.words.clear();
            second.words.clear();
        }
    }
    for part in [&mut first, &mut second] {
        part.edited = true;
        if part.original.is_empty() {
            part.original = part.text.clone();
        }
    }
    Ok([first, second])
}

/// Why line edits that move lines can't run on note `note_id` now (they
/// hold places in its transcript), if so.
fn busy(note_id: u64) -> Option<&'static str> {
    if crate::meetings::meeting::recording_note() == Some(note_id) {
        Some("Split lines once the meeting stops recording")
    } else if crate::meetings::meeting_end::summary_running_for(note_id) {
        Some("Yap is writing this meeting's summary: try again in a moment")
    } else if crate::meetings::meeting_audio::running_job(note_id).is_some() {
        Some("This meeting is being transcribed: try again when it's done")
    } else {
        None
    }
}

// ---- commands ----

/// A line changed in place: where it is and how it is now.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineChanged {
    pub index: usize,
    pub line: TranscriptSegment,
    pub changed: bool,
}

/// Tell every window line `index` of note `note_id` is `line` now
/// (`yap-note-changed`'s `lineEdit`, as a fixed line's words).
fn line_changed(app: &AppHandle, note_id: u64, index: usize, line: &TranscriptSegment, origin: &str) {
    if let Some(n) = crate::notes::get(note_id) {
        let mut payload = crate::notes::commands::note_changed_payload(&n, origin);
        payload["lineEdit"] = serde_json::json!({ "index": index, "line": line });
        let _ = app.emit("yap-note-changed", payload);
    }
}

/// "Revert to what Yap heard" on an edited line.
#[tauri::command]
pub fn transcript_line_revert(app: AppHandle, note_id: u64, line: LineRef, origin: Option<String>) -> Result<LineChanged, String> {
    let (index, seg, changed) = crate::notes::modify(note_id, |note| {
        let at = find_line(&note.transcript, &line).ok_or("That line changed meanwhile. Try again.")?;
        let mut seg = note.transcript[at].clone();
        let changed = revert_line(&mut seg)?;
        if changed {
            note.transcript[at] = seg.clone();
        }
        Ok(((at, seg, changed), changed))
    })?;
    if changed {
        tracing::info!(note_id, index, "Transcript line put back to what Yap heard");
        line_changed(&app, note_id, index, &seg, origin.as_deref().unwrap_or(""));
    }
    crate::notes::saved()?;
    Ok(LineChanged { index, line: seg, changed })
}

/// "Delete line": its words go (Undo puts them back: [`transcript_line_restore`]).
#[tauri::command]
pub fn transcript_line_delete(app: AppHandle, note_id: u64, line: LineRef, origin: Option<String>) -> Result<LineChanged, String> {
    let (index, mark, old, dropped) = crate::notes::modify(note_id, |note| {
        let at = find_line(&note.transcript, &line).ok_or("That line changed meanwhile. Try again.")?;
        let digests = note.digests.len();
        let old = remove_line(note, at)?;
        let dropped = digests - note.digests.len();
        Ok(((at, note.transcript[at].clone(), old, dropped), true))
    })?;
    {
        let mut kept = deleted();
        kept.push((note_id, old));
        let over = kept.len().saturating_sub(UNDO_KEEP);
        kept.drain(..over);
    }
    tracing::info!(note_id, index, digests_dropped = dropped, "Transcript line deleted");
    line_changed(&app, note_id, index, &mark, origin.as_deref().unwrap_or(""));
    crate::notes::saved()?;
    Ok(LineChanged { index, line: mark, changed: true })
}

/// Undo a "Delete line": the line deleted at `index` (its side, date and
/// place) put back as it was.
#[tauri::command]
pub fn transcript_line_restore(
    app: AppHandle,
    note_id: u64,
    index: usize,
    source: String,
    ts: u64,
    start_ms: Option<u64>,
    origin: Option<String>,
) -> Result<LineChanged, String> {
    let line = {
        let kept = deleted();
        kept.iter()
            .rev()
            .find(|(n, s)| *n == note_id && s.source == source && s.ts == ts && s.start_ms == start_ms)
            .map(|(_, s)| s.clone())
            .ok_or("That line can't be put back any more")?
    };
    let index = crate::notes::modify(note_id, |note| Ok((restore_line(note, index, &line)?, true)))?;
    deleted().retain(|(n, s)| !(*n == note_id && s.source == source && s.ts == ts && s.start_ms == start_ms));
    tracing::info!(note_id, index, "Transcript line put back");
    line_changed(&app, note_id, index, &line, origin.as_deref().unwrap_or(""));
    crate::notes::saved()?;
    Ok(LineChanged { index, line, changed: true })
}

/// A line split in two: where, and the two parts.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineSplit {
    pub index: usize,
    pub lines: Vec<TranscriptSegment>,
}

/// "Split line" at word `at` (the second part starts with it).
#[tauri::command]
pub fn transcript_line_split(app: AppHandle, note_id: u64, line: LineRef, at: usize, origin: Option<String>) -> Result<LineSplit, String> {
    if let Some(why) = busy(note_id) {
        return Err(why.to_string());
    }
    let (index, parts) = crate::notes::modify(note_id, |note| {
        let i = find_line(&note.transcript, &line).ok_or("That line changed meanwhile. Try again.")?;
        let parts = split_line(&note.transcript[i], at)?;
        crate::notes::replace_line(note, i, parts.to_vec());
        Ok(((i, parts.to_vec()), true))
    })?;
    tracing::info!(note_id, index, "Transcript line split");
    if let Some(n) = crate::notes::get(note_id) {
        let mut payload = crate::notes::commands::note_changed_payload(&n, origin.as_deref().unwrap_or(""));
        payload["lineSpeakers"] = serde_json::json!(crate::speakers::line_speakers(&n));
        payload["lineSplice"] = serde_json::json!({ "at": index, "remove": 1, "lines": parts });
        payload["breaks"] = serde_json::json!(n.breaks);
        let _ = app.emit("yap-note-changed", payload);
    }
    crate::notes::saved()?;
    Ok(LineSplit { index, lines: parts })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn seg(source: &str, text: &str, ts: u64, start: u64, end: u64) -> TranscriptSegment {
        TranscriptSegment { source: source.into(), text: text.into(), ts, start_ms: Some(start), end_ms: Some(end), ..Default::default() }
    }

    fn note_with(transcript: Vec<TranscriptSegment>, digests: &[(usize, usize)], breaks: Vec<usize>) -> Note {
        let mut n: Note = serde_json::from_value(json!({ "id": 1, "title": "Call", "createdTs": 1, "updatedTs": 1 })).unwrap();
        n.transcript = transcript;
        n.breaks = breaks;
        n.digests = digests
            .iter()
            .map(|&(f, t)| serde_json::from_value(json!({ "fromSeg": f, "toSeg": t, "startTs": 0, "endTs": 0 })).unwrap())
            .collect();
        n
    }

    #[test]
    fn a_reverted_line_says_what_yap_heard_again() {
        let mut s = seg("them", "we deploy on cube and eighties", 10, 0, 3_000);
        crate::notes::note_edits::apply_line_edit(&mut s, "We deploy on Kubernetes.").unwrap();
        assert_eq!(revert_line(&mut s), Ok(true));
        assert_eq!(s.text, "we deploy on cube and eighties");
        assert!(!s.edited && s.original.is_empty());
        // Nothing to put back now.
        assert_eq!(revert_line(&mut s), Ok(false));
        // Never a marker.
        assert!(revert_line(&mut TranscriptSegment::dictation_marker(3)).is_err());
    }

    #[test]
    fn a_deleted_line_leaves_its_place_and_comes_back_with_undo() {
        let lines = vec![
            seg("you", "hello all", 10, 0, 2_000),
            seg("them", "the password is hunter two", 12, 2_000, 5_000),
            seg("you", "let's start", 15, 5_000, 7_000),
        ];
        // Digests [0,1) and [1,3): the second read the line.
        let mut n = note_with(lines.clone(), &[(0, 1), (1, 3)], vec![3]);
        let old = remove_line(&mut n, 1).unwrap();
        assert_eq!(old.text, "the password is hunter two");
        let mark = &n.transcript[1];
        assert!(mark.removed && mark.text.is_empty() && mark.words.is_empty());
        assert_eq!((mark.source.as_str(), mark.ts, mark.start_ms, mark.end_ms), ("them", 12, Some(2_000), Some(5_000)));
        assert!(!mark.is_talk(), "no summary, view or search reads it");
        // The digest written with its words goes; the one before stays.
        assert_eq!(n.digests.iter().map(|d| (d.from_seg, d.to_seg)).collect::<Vec<_>>(), [(0, 1)]);
        // Indexes and stop marks don't move.
        assert_eq!((n.transcript.len(), n.breaks.clone()), (3, vec![3]));
        // Twice: refused (it's a mark now).
        assert!(remove_line(&mut n, 1).is_err());
        // Undo: back as it was, also after lines moved.
        n.transcript.insert(0, seg("you", "early", 9, 0, 500));
        assert_eq!(restore_line(&mut n, 1, &old), Ok(2));
        assert_eq!(n.transcript[2].text, "the password is hunter two");
        assert!(!n.transcript[2].removed);
        assert!(restore_line(&mut n, 2, &old).is_err(), "nothing left to put back");
    }

    #[test]
    fn a_split_line_keeps_its_time_and_speaker_in_both_parts() {
        let mut s = seg("them", "budget is due Friday okay thanks", 100, 10_000, 16_000);
        s.speaker = "them-2".into();
        s.speaker_locked = true;
        s.words = vec![[0, 400], [500, 800], [900, 1_200], [1_300, 2_000], [3_000, 3_600], [3_700, 4_500]];
        let [a, b] = split_line(&s, 4).unwrap();
        assert_eq!((a.text.as_str(), b.text.as_str()), ("budget is due Friday", "okay thanks"));
        // Each its own stretch, by the words' times; ▶ plays each.
        assert_eq!((a.start_ms, a.end_ms), (Some(10_000), Some(12_000)));
        assert_eq!((b.start_ms, b.end_ms), (Some(13_000), Some(16_000)));
        assert_eq!((a.ts, b.ts), (100, 103));
        assert_eq!(a.words, s.words[..4].to_vec());
        assert_eq!(b.words, vec![[0, 600], [700, 1_500]]);
        // Side, voice and lock kept; both the person's, kept by Transcribe again.
        for p in [&a, &b] {
            assert_eq!((p.source.as_str(), p.speaker.as_str(), p.speaker_locked), ("them", "them-2", true));
            assert!(p.edited && p.is_talk());
        }
        assert_eq!((a.original.as_str(), b.original.as_str()), ("budget is due Friday", "okay thanks"));
        // At the edges, or on a marker: refused.
        assert!(split_line(&s, 0).is_err() && split_line(&s, 6).is_err());
        assert!(split_line(&TranscriptSegment::dictation_marker(5), 1).is_err());

        // No word times: shared out by length.
        let plain = seg("you", "one two three four", 50, 0, 4_000);
        let [a, b] = split_line(&plain, 2).unwrap();
        assert_eq!((a.text.as_str(), b.text.as_str()), ("one two", "three four"));
        assert!(a.end_ms.unwrap() <= b.start_ms.unwrap());
        assert!(b.start_ms.unwrap() > 1_000 && b.start_ms.unwrap() < 3_000);
        assert_eq!(b.end_ms, Some(4_000));
        assert!(a.words.is_empty() && b.words.is_empty());

        // An edited line: what Yap heard is shared where the cut falls.
        let mut fixed = seg("them", "we deploy on cube and eighties after lunch", 70, 0, 8_000);
        fixed.words = vec![[0, 500], [600, 1_000], [1_100, 1_500], [1_600, 2_400], [2_500, 2_800], [2_900, 4_000], [5_000, 6_000], [6_100, 8_000]];
        crate::notes::note_edits::apply_line_edit(&mut fixed, "We deploy on Kubernetes after lunch").unwrap();
        let [a, b] = split_line(&fixed, 4).unwrap();
        assert_eq!((a.text.as_str(), b.text.as_str()), ("We deploy on Kubernetes", "after lunch"));
        assert!(a.original.starts_with("we deploy on") && b.original.ends_with("lunch"));
        assert_eq!(format!("{} {}", a.original, b.original), "we deploy on cube and eighties after lunch");
        // Revert on a part: its share of what Yap heard.
        let mut b2 = b.clone();
        revert_line(&mut b2).unwrap();
        assert!(b2.text.ends_with("lunch") && !b2.edited);
    }

    #[test]
    fn a_split_moves_the_lines_digests_and_stops_after_it() {
        let lines = vec![seg("you", "a b", 1, 0, 1_000), seg("them", "c d e", 2, 1_000, 3_000), seg("you", "f", 4, 3_000, 4_000)];
        let mut n = note_with(lines, &[(0, 2), (2, 3)], vec![2, 3]);
        let parts = split_line(&n.transcript[1], 1).unwrap();
        crate::notes::replace_line(&mut n, 1, parts.to_vec());
        let texts: Vec<&str> = n.transcript.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["a b", "c", "d e", "f"]);
        // A digest that read the line reads both parts; the rest move on.
        assert_eq!(n.digests.iter().map(|d| (d.from_seg, d.to_seg)).collect::<Vec<_>>(), [(0, 3), (3, 4)]);
        assert_eq!(n.breaks, vec![3, 4]);
    }

    /// Transcribe again (and the refine after a meeting) keep the person's
    /// changes: a deleted line's words never come back, a split stays split.
    #[test]
    fn transcribing_again_keeps_deletions_and_splits() {
        use crate::meetings::meeting_audio::redo::{rebuild, Redone};
        const T0: u64 = 1_761_000_000_000;
        let line = |source: &str, text: &str, s: u64, e: u64| TranscriptSegment {
            ts: (T0 + s) / 1000,
            ..seg(source, text, 0, s, e)
        };
        let mut n = note_with(
            vec![line("you", "hello", 0, 2_000), line("them", "my card number is", 3_000, 6_000), line("you", "ok one two", 7_000, 10_000)],
            &[],
            vec![3],
        );
        remove_line(&mut n, 1).unwrap();
        let parts = split_line(&n.transcript[2], 1).unwrap();
        crate::notes::replace_line(&mut n, 2, parts.to_vec());
        let redone = vec![Redone {
            started_ms: T0,
            end_ms: T0 + 11_000,
            lines: vec![line("you", "Hello.", 0, 2_000), line("them", "My card number is.", 3_100, 5_900), line("you", "OK, one two.", 7_000, 10_000)],
        }];
        let r = rebuild(&n.transcript, &n.breaks, &redone);
        let texts: Vec<&str> = r.transcript.iter().filter(|s| !s.removed).map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["Hello.", "ok", "one two"]);
        assert!(!r.transcript.iter().any(|s| s.text.contains("card")), "a deleted line's words never come back");
        assert_eq!(r.transcript.iter().filter(|s| s.removed).count(), 1, "its mark stays");
    }
}

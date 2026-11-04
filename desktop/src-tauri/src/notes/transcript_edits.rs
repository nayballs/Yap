//! **A fixed transcript line, the rest** (after `note_edits.rs`, which fixes
//! a line's words): put a line back to what Yap heard, delete one (with
//! Undo), or split one in two at the cursor. The meeting notepad and the
//! Notes view, in a line's menu (and Shift+Enter while editing one: split
//! there).
//!
//! - **Revert** ([`revert_line`]): an edited line's words go back to
//!   `original`; it's Yap's line again (Transcribe again may replace it).
//! - **Delete** ([`remove_line`]): the line goes from the note — every
//!   view, the summaries' inputs and search — so an older Yap sharing the
//!   data folder keeps it gone too. Its side and place on the recording's
//!   clock are kept beside the meeting's audio (`meeting_audio::Cut`, in
//!   `cuts.json`), so Transcribe again and the refine after a meeting
//!   (`meeting_audio/redo.rs`) never bring its words back. Stop marks after
//!   it move up; the rolling digests from the one that read it on are
//!   dropped (`drop_digests_from`): they were written with its words, and
//!   the next summary writes them again from what's left. **Undo**
//!   ([`restore_line`]) puts the line back where it was, from this run's
//!   memory of it (never from disk: once Yap closes, the words are gone for
//!   good). Not while the meeting records (the recorder and the voice
//!   separation hold places in its transcript).
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

/// A marker (a dictation, a failed stretch, the mic disconnected, the PC
/// asleep): none of these edits apply.
fn is_marker(seg: &TranscriptSegment) -> bool {
    seg.dictated || seg.failed || seg.mic.is_some() || seg.asleep_ms.is_some()
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

/// A line deleted, for Undo: the line as it was, where it was, and the stop
/// marks and length of the transcript before.
#[derive(Debug, Clone)]
pub struct Removed {
    pub line: TranscriptSegment,
    pub index: usize,
    pub breaks: Vec<usize>,
    pub len: usize,
}

/// Delete line `at` of `note` (pure; see the module notes): stop marks after
/// it move up one, digests from the one that read it go.
pub fn remove_line(note: &mut Note, at: usize) -> Result<Removed, String> {
    let seg = note.transcript.get(at).ok_or("That line isn't there any more")?;
    if is_marker(seg) {
        return Err("That isn't a line of the transcript".to_string());
    }
    let removed = Removed { line: seg.clone(), index: at, breaks: note.breaks.clone(), len: note.transcript.len() };
    note.transcript.remove(at);
    drop_digests_from(note, at);
    let mut breaks: Vec<usize> = note.breaks.iter().map(|&b| if b > at { b - 1 } else { b }).filter(|&b| b > 0).collect();
    breaks.dedup();
    note.breaks = breaks;
    Ok(removed)
}

/// Put a deleted line back (pure): where it was when nothing else changed
/// since (its stop marks as they were), else where it belongs by time.
/// Returns its index.
pub fn restore_line(note: &mut Note, removed: &Removed) -> Result<usize, String> {
    let line = &removed.line;
    if note.transcript.iter().any(|s| s.source == line.source && s.ts == line.ts && s.start_ms == line.start_ms && s.text == line.text) {
        return Err("That line is back already".to_string());
    }
    let unchanged = note.transcript.len() + 1 == removed.len && removed.index <= note.transcript.len();
    let at = if unchanged {
        removed.index
    } else {
        note.transcript.partition_point(|s| (s.ts, s.start_ms.unwrap_or(0)) <= (line.ts, line.start_ms.unwrap_or(0)))
    };
    note.transcript.insert(at, line.clone());
    if unchanged {
        note.breaks = removed.breaks.clone();
    } else {
        for b in &mut note.breaks {
            if *b > at {
                *b += 1;
            }
        }
    }
    // Digests written while it was gone don't have its words.
    drop_digests_from(note, at);
    Ok(at)
}

/// Lines deleted in this run, for Undo: (note, how it was). The newest
/// last; a few dozen at most.
fn deleted() -> std::sync::MutexGuard<'static, Vec<(u64, Removed)>> {
    static DELETED: Mutex<Vec<(u64, Removed)>> = Mutex::new(Vec::new());
    DELETED.lock().unwrap_or_else(|p| p.into_inner())
}

const UNDO_KEEP: usize = 32;

/// A deleted line's place, kept beside the meeting's audio.
fn cut_of(line: &TranscriptSegment) -> Option<crate::meetings::meeting_audio::Cut> {
    Some(crate::meetings::meeting_audio::Cut {
        source: line.source.clone(),
        ts: line.ts,
        start_ms: line.start_ms?,
        end_ms: line.end_ms?,
    })
}

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
fn busy(note_id: u64, split: bool) -> Option<&'static str> {
    if crate::meetings::meeting::recording_note() == Some(note_id) {
        Some(if split { "Split lines once the meeting stops recording" } else { "Delete lines once the meeting stops recording" })
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

/// Tell every window lines of note `note_id` were taken out or put in at
/// `at` (`yap-note-changed`'s `lineSplice`, as the voice separation's: the
/// lines after it moved), with every line's voice and the stop marks.
fn lines_spliced(app: &AppHandle, note_id: u64, at: usize, remove: usize, lines: &[TranscriptSegment], origin: &str) {
    if let Some(n) = crate::notes::get(note_id) {
        let mut payload = crate::notes::commands::note_changed_payload(&n, origin);
        payload["lineSpeakers"] = serde_json::json!(crate::speakers::line_speakers(&n));
        payload["lineSplice"] = serde_json::json!({ "at": at, "remove": remove, "lines": lines });
        payload["breaks"] = serde_json::json!(n.breaks);
        let _ = app.emit("yap-note-changed", payload);
    }
}

/// Lines taken out or put back: where, which, and the stop marks now.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineMoved {
    pub index: usize,
    pub line: TranscriptSegment,
    pub breaks: Vec<usize>,
}

/// "Delete line" (Undo puts it back: [`transcript_line_restore`]).
#[tauri::command]
pub fn transcript_line_delete(app: AppHandle, note_id: u64, line: LineRef, origin: Option<String>) -> Result<LineMoved, String> {
    if let Some(why) = busy(note_id, false) {
        return Err(why.to_string());
    }
    let (removed, breaks, dropped) = crate::notes::modify(note_id, |note| {
        let at = find_line(&note.transcript, &line).ok_or("That line changed meanwhile. Try again.")?;
        let digests = note.digests.len();
        let removed = remove_line(note, at)?;
        let dropped = digests - note.digests.len();
        Ok(((removed, note.breaks.clone(), dropped), true))
    })?;
    // Its place, so the audio never brings it back.
    if let Some(cut) = cut_of(&removed.line) {
        crate::meetings::meeting_audio::set_cut(note_id, cut, true);
    }
    let (index, gone) = (removed.index, removed.line.clone());
    {
        let mut kept = deleted();
        kept.push((note_id, removed));
        let over = kept.len().saturating_sub(UNDO_KEEP);
        kept.drain(..over);
    }
    tracing::info!(note_id, index, digests_dropped = dropped, "Transcript line deleted");
    lines_spliced(&app, note_id, index, 1, &[], origin.as_deref().unwrap_or(""));
    crate::notes::saved()?;
    Ok(LineMoved { index, line: gone, breaks })
}

/// Undo a "Delete line": the line deleted from where it was (`index`, its
/// side, date and place) put back as it was.
#[tauri::command]
pub fn transcript_line_restore(
    app: AppHandle,
    note_id: u64,
    index: usize,
    source: String,
    ts: u64,
    start_ms: Option<u64>,
    origin: Option<String>,
) -> Result<LineMoved, String> {
    if let Some(why) = busy(note_id, false) {
        return Err(why.to_string());
    }
    let is_it = |n: &u64, r: &Removed| *n == note_id && r.index == index && r.line.source == source && r.line.ts == ts && r.line.start_ms == start_ms;
    let removed = deleted()
        .iter()
        .rev()
        .find(|(n, r)| is_it(n, r))
        .map(|(_, r)| r.clone())
        .ok_or("That line can't be put back any more")?;
    let (at, breaks) = crate::notes::modify(note_id, |note| {
        let at = restore_line(note, &removed)?;
        Ok(((at, note.breaks.clone()), true))
    })?;
    deleted().retain(|(n, r)| !is_it(n, r));
    if let Some(cut) = cut_of(&removed.line) {
        crate::meetings::meeting_audio::set_cut(note_id, cut, false);
    }
    tracing::info!(note_id, index = at, "Transcript line put back");
    lines_spliced(&app, note_id, at, 0, std::slice::from_ref(&removed.line), origin.as_deref().unwrap_or(""));
    crate::notes::saved()?;
    Ok(LineMoved { index: at, line: removed.line, breaks })
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
    if let Some(why) = busy(note_id, true) {
        return Err(why.to_string());
    }
    let (index, parts) = crate::notes::modify(note_id, |note| {
        let i = find_line(&note.transcript, &line).ok_or("That line changed meanwhile. Try again.")?;
        let parts = split_line(&note.transcript[i], at)?;
        crate::notes::replace_line(note, i, parts.to_vec());
        Ok(((i, parts.to_vec()), true))
    })?;
    tracing::info!(note_id, index, "Transcript line split");
    lines_spliced(&app, note_id, index, 1, &parts, origin.as_deref().unwrap_or(""));
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
    fn a_deleted_line_goes_and_comes_back_with_undo() {
        let lines = vec![
            seg("you", "hello all", 10, 0, 2_000),
            seg("them", "the password is hunter two", 12, 2_000, 5_000),
            seg("you", "let's start", 15, 5_000, 7_000),
        ];
        // Digests [0,1) and [1,3): the second read the line. Stops after
        // the line and at the end.
        let mut n = note_with(lines.clone(), &[(0, 1), (1, 3)], vec![2, 3]);
        let removed = remove_line(&mut n, 1).unwrap();
        assert_eq!(removed.line.text, "the password is hunter two");
        // Gone from the note (an older Yap keeps it gone too).
        assert_eq!(n.transcript.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), ["hello all", "let's start"]);
        // The digest written with its words goes; the one before stays; the
        // stop marks after it move up.
        assert_eq!(n.digests.iter().map(|d| (d.from_seg, d.to_seg)).collect::<Vec<_>>(), [(0, 1)]);
        assert_eq!(n.breaks, vec![1, 2]);
        // A marker isn't a line to delete.
        n.transcript.push(TranscriptSegment::dictation_marker(20));
        assert!(remove_line(&mut n, 2).is_err());
        n.transcript.pop();
        // Undo, nothing else changed: back where it was, the stops as they were.
        assert_eq!(restore_line(&mut n, &removed), Ok(1));
        let texts = |t: &[TranscriptSegment]| t.iter().map(|s| (s.text.clone(), s.ts, s.start_ms, s.end_ms)).collect::<Vec<_>>();
        assert_eq!(texts(&n.transcript), texts(&lines));
        assert_eq!(n.breaks, vec![2, 3]);
        assert!(restore_line(&mut n, &removed).is_err(), "back already");
        // Undo after lines moved: where it belongs by time.
        let removed = remove_line(&mut n, 1).unwrap();
        n.transcript.insert(0, seg("you", "early", 9, 0, 500));
        assert_eq!(restore_line(&mut n, &removed), Ok(2));
        assert_eq!(n.transcript[2].text, "the password is hunter two");
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
        use crate::meetings::meeting_audio::redo::{rebuild_with_cuts, Redone};
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
        let removed = remove_line(&mut n, 1).unwrap();
        let cuts = [cut_of(&removed.line).unwrap()];
        let parts = split_line(&n.transcript[1], 1).unwrap();
        crate::notes::replace_line(&mut n, 1, parts.to_vec());
        let redone = vec![Redone {
            started_ms: T0,
            end_ms: T0 + 11_000,
            lines: vec![line("you", "Hello.", 0, 2_000), line("them", "My card number is.", 3_100, 5_900), line("you", "OK, one two.", 7_000, 10_000)],
        }];
        let r = rebuild_with_cuts(&n.transcript, &n.breaks, &redone, &cuts);
        let texts: Vec<&str> = r.transcript.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["Hello.", "ok", "one two"]);
        assert!(!r.transcript.iter().any(|s| s.text.contains("card")), "a deleted line's words never come back");
        assert_eq!(r.breaks, vec![3]);
    }
}

//! **Transcribe again**: a meeting's new lines put in place of the old ones
//! (pure, unit-tested; `meeting_audio::transcribe_again` runs it under the
//! notes store's lock).
//!
//! What it keeps:
//! - **lines it has no audio for**: a recording from before audio was kept,
//!   or whose audio has gone (each recording's lines are those dated within
//!   it), and every "You dictated here" marker (the audio has silence there;
//!   the marker stays where it was);
//! - **lines the person edited** (`TranscriptSegment::edited`,
//!   `note_edits.rs`), as they wrote them: a new line of the same side over
//!   one keeps only its words outside it (by its word times; without them
//!   by its share of the line's place, and none when that's under half of
//!   it) — whoever calls this (Transcribe again, a refined transcript)
//!   never writes over a fix;
//! - **who said what, where the lines still match by time**: a new line
//!   takes the voice (and the "I said who this is" lock) of the old line of
//!   its side it overlaps most — at least half of the shorter of the two;
//!   one that spans two old voices is split at its words where they change
//!   (`speakers::split_line`), as the speaker engine's labels split lines.
//!   The speaker map itself (the names) isn't touched;
//! - **where each recording stopped** (`Note::breaks`): moved to the same
//!   place among the new lines.
//!
//! The note's own text ("My thoughts") and its summary are fields this
//! never sees. The rolling digests are dropped by the caller: they were
//! written from the old lines, and "Generate summary" writes them again
//! from the new ones.

use crate::notes::TranscriptSegment;
use crate::speakers::Turn;

/// One recording of the meeting, transcribed again.
#[derive(Debug, Clone, Default)]
pub struct Redone {
    /// When it started and when its audio ends (unix ms).
    pub started_ms: u64,
    pub end_ms: u64,
    /// Its new lines (and markers for dictations no old line marked), in
    /// time order.
    pub lines: Vec<TranscriptSegment>,
}

/// The note's new transcript.
#[derive(Debug, Default)]
pub struct Rebuilt {
    pub transcript: Vec<TranscriptSegment>,
    pub breaks: Vec<usize>,
    /// Old lines replaced, and new lines put in.
    pub replaced: usize,
    pub added: usize,
    /// New lines that kept a voice other than their side's own.
    pub voiced: usize,
}

/// Which recording a line dated `ts` (unix seconds, rounded down) belongs
/// to: the latest to start that it falls within.
fn recording_of(redone: &[Redone], ts: u64) -> Option<usize> {
    let ms = ts * 1000;
    redone.iter().rposition(|r| ms + 999 >= r.started_ms && ms <= r.end_ms + 1000)
}

/// How long two stretches overlap (ms).
fn overlap(a: (u64, u64), b: (u64, u64)) -> u64 {
    a.1.min(b.1).saturating_sub(a.0.max(b.0))
}

fn place(seg: &TranscriptSegment) -> Option<(u64, u64)> {
    Some((seg.start_ms?, seg.end_ms?))
}

/// `line` with the voices of the old lines it matches by time (see above).
fn with_voices(line: TranscriptSegment, olds: &[&TranscriptSegment]) -> Vec<TranscriptSegment> {
    let Some(at) = place(&line).filter(|_| line.is_talk()) else {
        return vec![line];
    };
    // A match: at least half the shorter of the two.
    let matches = |o: &TranscriptSegment, at: (u64, u64)| {
        place(o).is_some_and(|p| {
            let shared = overlap(p, at);
            shared > 0 && shared * 2 >= (p.1 - p.0).min(at.1 - at.0).max(1)
        })
    };
    let near: Vec<&TranscriptSegment> = olds
        .iter()
        .copied()
        .filter(|o| o.source == line.source && o.is_talk() && matches(o, at))
        .collect();
    if near.is_empty() {
        return vec![line];
    }
    let parts = if near.iter().any(|o| o.speaker_id() != near[0].speaker_id()) {
        let turns: Vec<Turn> = near
            .iter()
            .filter_map(|o| {
                let (start_ms, end_ms) = place(o)?;
                Some(Turn { start_ms, end_ms, speaker: o.speaker_id().to_string() })
            })
            .collect();
        crate::speakers::split_line(&line, &turns)
    } else {
        vec![line]
    };
    parts
        .into_iter()
        .map(|mut part| {
            let at = place(&part).unwrap_or(at);
            let best = near
                .iter()
                .filter(|o| matches(o, at))
                .max_by_key(|o| place(o).map_or(0, |p| overlap(p, at)));
            if let Some(best) = best {
                let id = best.speaker_id();
                part.speaker = if id == part.source { String::new() } else { id.to_string() };
                part.speaker_locked = best.speaker_locked;
            }
            part
        })
        .collect()
}

/// Each word of `line` with its place on the recording's clock: its word
/// times when it has them, else the line's place shared out by length.
fn word_places(line: &TranscriptSegment, (start, end): (u64, u64)) -> Vec<(u64, u64)> {
    let words: Vec<&str> = line.text.split_whitespace().collect();
    if line.words.len() == words.len() {
        return line.words.iter().map(|[s, e]| (start + *s as u64, start + *e as u64)).collect();
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

/// `line` without what a line the person edited already says: the runs of
/// its words whose middles fall outside `fixed` (the edited lines of its
/// side and recording), each a line of its own. Without word times (the
/// words' places only guessed), none when what's left is under half of it:
/// it was mostly the edited line.
fn outside_edited(line: TranscriptSegment, fixed: &[(u64, u64)]) -> Vec<TranscriptSegment> {
    let Some(at) = place(&line).filter(|_| line.is_talk()) else {
        return vec![line];
    };
    if !fixed.iter().any(|&f| overlap(f, at) > 0) {
        return vec![line];
    }
    let words: Vec<&str> = line.text.split_whitespace().collect();
    let times = word_places(&line, at);
    let free: Vec<bool> = times
        .iter()
        .map(|&(s, e)| {
            let mid = (s + e) / 2;
            !fixed.iter().any(|&(fs, fe)| mid >= fs && mid < fe)
        })
        .collect();
    let aligned = line.words.len() == words.len();
    let kept = free.iter().filter(|f| **f).count();
    if !aligned && kept * 2 < words.len() {
        return Vec::new();
    }
    let mut parts = Vec::new();
    let mut k = 0;
    while k < words.len() {
        if !free[k] {
            k += 1;
            continue;
        }
        let from = k;
        while k < words.len() && free[k] {
            k += 1;
        }
        if from == 0 && k == words.len() {
            return vec![line];
        }
        let (s, e) = (times[from].0, times[k - 1].1.max(times[from].0));
        let mut part = line.clone();
        part.text = words[from..k].join(" ");
        part.start_ms = Some(s);
        part.end_ms = Some(e);
        part.words = if aligned {
            times[from..k].iter().map(|&(ws, we)| [(ws - s) as u32, (we - s) as u32]).collect()
        } else {
            Vec::new()
        };
        part.ts = line.ts + s.saturating_sub(at.0) / 1000;
        parts.push(part);
    }
    parts
}

/// The note's transcript with each of `redone`'s recordings' lines put in
/// place of the old ones (see the module notes). `old_breaks` are the old
/// transcript's stop marks.
pub fn rebuild(old: &[TranscriptSegment], old_breaks: &[usize], redone: &[Redone]) -> Rebuilt {
    let mut kept: Vec<TranscriptSegment> = Vec::new();
    let mut by_recording: Vec<Vec<&TranscriptSegment>> = vec![Vec::new(); redone.len()];
    // Per recording: the places of its lines the person edited, by side.
    let mut fixed: Vec<Vec<(&str, (u64, u64))>> = vec![Vec::new(); redone.len()];
    let mut replaced = 0;
    for seg in old {
        match recording_of(redone, seg.ts).filter(|_| !seg.dictated) {
            Some(i) if seg.edited => {
                if let Some(p) = place(seg) {
                    fixed[i].push((seg.source.as_str(), p));
                }
                kept.push(seg.clone());
            }
            Some(i) => {
                by_recording[i].push(seg);
                replaced += 1;
            }
            None => kept.push(seg.clone()),
        }
    }
    let mut added = 0;
    let mut fresh: Vec<TranscriptSegment> = Vec::new();
    for (i, r) in redone.iter().enumerate() {
        for line in &r.lines {
            // A marker the old transcript has already (same place): once.
            if line.dictated && kept.iter().any(|k| k.dictated && k.ts == line.ts) {
                continue;
            }
            let mine: Vec<(u64, u64)> = fixed[i].iter().filter(|(side, _)| *side == line.source).map(|(_, p)| *p).collect();
            for piece in outside_edited(line.clone(), &mine) {
                let parts = with_voices(piece, &by_recording[i]);
                added += parts.len();
                fresh.extend(parts);
            }
        }
    }
    let voiced = fresh
        .iter()
        .filter(|s| s.is_talk() && !s.speaker.is_empty() && s.speaker != s.source)
        .count();
    let mut transcript = kept;
    transcript.extend(fresh);
    // In time order (stable: lines from one second keep their order, and a
    // line the person edited sits among the new ones by its place).
    transcript.sort_by_key(|s| (s.ts, s.start_ms.unwrap_or(0)));

    // Each stop where it was: after the recording it ended (a line of a
    // recording done again), else after the line it followed.
    let mut breaks: Vec<usize> = old_breaks
        .iter()
        .filter(|&&b| b > 0 && b <= old.len())
        .map(|&b| {
            let before = &old[b - 1];
            let upto = match recording_of(redone, before.ts).filter(|_| !before.dictated) {
                Some(i) => redone[i].end_ms / 1000,
                None => before.ts,
            };
            transcript.partition_point(|s| s.ts <= upto)
        })
        .filter(|&b| b > 0)
        .collect();
    breaks.sort_unstable();
    breaks.dedup();
    Rebuilt { transcript, breaks, replaced, added, voiced }
}

/// **Try again** on one failed chunk: its marker (line `at`) gives way to
/// `lines` — what that stretch says now (none: it was quiet after all; a
/// marker again: it failed again). Not [`rebuild`]: that replaces every
/// line of a recording, this fills one hole in one side. Stop marks and
/// digests after it move along; a digest that covered the hole covers its
/// lines.
pub fn fill(note: &mut crate::notes::Note, at: usize, lines: Vec<TranscriptSegment>) {
    if at >= note.transcript.len() {
        return;
    }
    let n = lines.len();
    note.transcript.splice(at..at + 1, lines);
    let moved = |i: usize| if i > at { i + n - 1 } else { i };
    for d in &mut note.digests {
        d.from_seg = moved(d.from_seg);
        d.to_seg = moved(d.to_seg);
    }
    for b in &mut note.breaks {
        *b = moved(*b);
    }
    note.breaks.retain(|&b| b > 0);
    note.breaks.dedup();
    note.digests.retain(|d| d.to_seg > d.from_seg);
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: u64 = 1_761_000_000_000; // a recording's start, unix ms

    fn line(source: &str, text: &str, start_ms: u64, end_ms: u64) -> TranscriptSegment {
        TranscriptSegment {
            source: source.into(),
            text: text.into(),
            ts: (T0 + start_ms) / 1000,
            start_ms: Some(start_ms),
            end_ms: Some(end_ms),
            ..Default::default()
        }
    }

    fn voiced(mut seg: TranscriptSegment, speaker: &str, locked: bool) -> TranscriptSegment {
        seg.speaker = speaker.into();
        seg.speaker_locked = locked;
        seg
    }

    fn redone(lines: Vec<TranscriptSegment>, secs: u64) -> Redone {
        Redone { started_ms: T0, end_ms: T0 + secs * 1000, lines }
    }

    #[test]
    fn new_lines_replace_the_old_and_keep_their_voices() {
        let old = vec![
            voiced(line("them", "hello all", 1_000, 4_000), "them-1", false),
            line("you", "hi there", 5_000, 7_000),
            voiced(line("them", "budget is due", 9_000, 14_000), "p-1", true),
        ];
        let new = vec![
            line("them", "Hello, all.", 1_200, 4_100),
            line("you", "Hi there.", 5_100, 6_900),
            line("them", "The budget is due Friday.", 8_800, 14_200),
        ];
        let r = rebuild(&old, &[3], &[redone(new, 20)]);
        assert_eq!(r.transcript.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), [
            "Hello, all.",
            "Hi there.",
            "The budget is due Friday."
        ]);
        // The engine's voice, "You", and a name the person put on (locked).
        assert_eq!(r.transcript[0].speaker, "them-1");
        assert!(!r.transcript[0].speaker_locked);
        assert_eq!(r.transcript[1].speaker, "");
        assert_eq!(r.transcript[2].speaker, "p-1");
        assert!(r.transcript[2].speaker_locked);
        assert_eq!((r.replaced, r.added, r.voiced), (3, 3, 2));
        assert_eq!(r.breaks, vec![3]);
    }

    #[test]
    fn a_line_over_two_old_voices_is_split_where_they_change() {
        let old = vec![
            voiced(line("them", "one two three", 0, 3_000), "them-1", false),
            voiced(line("them", "four five six", 3_000, 6_000), "them-2", false),
        ];
        let mut long = line("them", "one two three four five six", 0, 6_000);
        long.words = vec![[0, 900], [1_000, 1_900], [2_000, 2_900], [3_100, 3_900], [4_100, 4_900], [5_100, 5_900]];
        let r = rebuild(&old, &[], &[redone(vec![long], 6)]);
        let got: Vec<(&str, &str)> = r.transcript.iter().map(|s| (s.speaker.as_str(), s.text.as_str())).collect();
        assert_eq!(got, [("them-1", "one two three"), ("them-2", "four five six")]);
    }

    #[test]
    fn a_line_that_barely_touches_an_old_one_keeps_its_own_side() {
        let old = vec![voiced(line("them", "earlier", 0, 2_000), "them-3", false)];
        // 300 ms of overlap on a 6 s line, and 15% of the old one's 2 s.
        let new = vec![line("them", "Something else entirely.", 1_700, 7_700)];
        let r = rebuild(&old, &[], &[redone(new, 10)]);
        assert_eq!(r.transcript[0].speaker, "");
        // And "You" never takes the call's voice.
        let old = vec![voiced(line("them", "theirs", 0, 4_000), "them-1", false)];
        let r = rebuild(&old, &[], &[redone(vec![line("you", "mine", 0, 4_000)], 5)]);
        assert_eq!(r.transcript[0].speaker, "");
    }

    #[test]
    fn markers_and_lines_without_audio_stay_where_they_were() {
        let earlier = TranscriptSegment {
            source: "them".into(),
            text: "from a recording with no audio".into(),
            ts: T0 / 1000 - 3_600,
            ..Default::default()
        };
        let marker = TranscriptSegment::dictation_marker((T0 + 6_000) / 1000);
        let old = vec![earlier.clone(), line("you", "before", 1_000, 5_000), marker.clone(), line("you", "after", 9_000, 12_000)];
        // Breaks: after the old recording, and at the end of this one.
        let new = vec![line("you", "Before.", 1_000, 5_000), line("you", "After.", 9_000, 12_000)];
        let r = rebuild(&old, &[1, 4], &[redone(new, 13)]);
        let texts: Vec<&str> = r.transcript.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["from a recording with no audio", "Before.", "", "After."]);
        assert!(r.transcript[2].dictated);
        assert_eq!(r.breaks, vec![1, 4]);
        assert_eq!((r.replaced, r.added), (2, 2));
        // A marker the replay found again isn't doubled.
        let again = vec![line("you", "Before.", 1_000, 5_000), marker, line("you", "After.", 9_000, 12_000)];
        let r = rebuild(&old, &[1, 4], &[redone(again, 13)]);
        assert_eq!(r.transcript.iter().filter(|s| s.dictated).count(), 1);
    }

    fn edited(mut seg: TranscriptSegment, text: &str) -> TranscriptSegment {
        seg.original = std::mem::replace(&mut seg.text, text.into());
        seg.edited = true;
        seg
    }

    #[test]
    fn a_line_the_person_edited_is_never_written_over() {
        let fixed = voiced(edited(line("them", "we deploy on cube and eighties", 5_000, 9_000), "We deploy on Kubernetes."), "p-1", true);
        let old = vec![line("you", "hello", 1_000, 3_000), fixed.clone(), line("you", "great", 10_000, 11_000)];
        // The same audio heard again: the edited line's own new line goes.
        let new = vec![
            line("you", "Hello.", 1_000, 3_000),
            line("them", "We deploy on cube and eighties.", 5_100, 8_900),
            line("you", "Great.", 10_000, 11_000),
        ];
        let r = rebuild(&old, &[3], &[redone(new, 12)]);
        let texts: Vec<&str> = r.transcript.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["Hello.", "We deploy on Kubernetes.", "Great."]);
        let kept = &r.transcript[1];
        assert!(kept.edited && kept.speaker_locked);
        assert_eq!(kept.original, "we deploy on cube and eighties");
        assert_eq!((kept.start_ms, kept.end_ms), (Some(5_000), Some(9_000)), "▶ still plays what was said");
        assert_eq!((r.replaced, r.added), (2, 2));
        assert_eq!(r.transcript.iter().filter(|s| s.edited).count(), 1);
        assert_eq!(r.breaks, vec![3]);

        // A longer new line over it, with word times: only its words outside
        // the edited line stay, as lines of their own.
        let mut long = line("them", "right so we deploy on cube and eighties after lunch today", 3_000, 12_000);
        long.words = vec![
            [0, 400], [500, 900], [2_100, 2_400], [2_500, 3_000], [3_100, 3_400], [3_500, 4_000], [4_100, 4_500],
            [4_600, 5_500], [6_100, 6_600], [6_700, 7_200], [7_300, 9_000],
        ];
        let r = rebuild(&old, &[], &[redone(vec![long], 12)]);
        let them: Vec<(&str, Option<u64>)> =
            r.transcript.iter().filter(|s| s.source == "them").map(|s| (s.text.as_str(), s.start_ms)).collect();
        assert_eq!(them, [("right so", Some(3_000)), ("We deploy on Kubernetes.", Some(5_000)), ("after lunch today", Some(9_100))]);
        // The other side's lines never give way to it.
        let r = rebuild(&old, &[], &[redone(vec![line("you", "Mm-hm, sounds good.", 5_000, 9_000)], 12)]);
        assert!(r.transcript.iter().any(|s| s.text == "Mm-hm, sounds good."));

        // No word times: shared out by length; mostly the edited line → gone.
        let r = rebuild(&old, &[], &[redone(vec![line("them", "we deploy on cube and eighties ok", 4_800, 9_400)], 12)]);
        assert_eq!(r.transcript.iter().filter(|s| s.source == "them").count(), 1);
        assert!(r.transcript.iter().all(|s| s.source != "them" || s.edited));
    }

    #[test]
    fn each_recording_takes_back_its_own_lines_and_stop() {
        // Two recordings of one meeting (a pause between), 30 s apart.
        let second = T0 + 30_000;
        let late = |text: &str, s: u64, e: u64| TranscriptSegment { ts: (second + s) / 1000, ..line("them", text, s, e) };
        let old = vec![line("them", "first a", 0, 4_000), line("them", "first b", 5_000, 9_000), late("second a", 0, 3_000)];
        let fresh_one = redone(vec![line("them", "First A.", 0, 4_000), line("them", "First B, longer.", 5_000, 9_500), line("them", "First C.", 10_000, 11_000)], 12);
        let fresh_two = Redone { started_ms: second, end_ms: second + 5_000, lines: vec![late("Second A.", 0, 3_000)] };
        let r = rebuild(&old, &[2, 3], &[fresh_one, fresh_two]);
        let texts: Vec<&str> = r.transcript.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["First A.", "First B, longer.", "First C.", "Second A."]);
        // The pause now falls after the first recording's three lines.
        assert_eq!(r.breaks, vec![3, 4]);
    }

    #[test]
    fn a_failed_chunk_is_replaced_by_its_lines_when_the_meeting_is_done_again() {
        // The live pass couldn't transcribe 15–30 s of "Them": a marker.
        let marker = TranscriptSegment::failed_marker("them", (T0 + 15_000) / 1000, 15_000, 30_000);
        assert!(!marker.is_talk(), "summaries skip it");
        let old = vec![line("them", "before", 1_000, 14_000), marker, line("them", "after", 31_000, 40_000)];
        let new = vec![
            line("them", "Before.", 1_000, 14_000),
            line("them", "The missing part.", 15_500, 29_000),
            line("them", "After.", 31_000, 40_000),
        ];
        let r = rebuild(&old, &[3], &[redone(new, 41)]);
        let texts: Vec<&str> = r.transcript.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["Before.", "The missing part.", "After."]);
        assert!(!r.transcript.iter().any(|s| s.failed));
        assert_eq!(r.breaks, vec![3]);
    }

    fn note_with(transcript: Vec<TranscriptSegment>, breaks: Vec<usize>, digests: Vec<(usize, usize)>) -> crate::notes::Note {
        let mut n: crate::notes::Note = serde_json::from_value(serde_json::json!({
            "id": 1, "title": "Call", "content": "", "createdTs": 1, "updatedTs": 1,
        }))
        .unwrap();
        n.transcript = transcript;
        n.breaks = breaks;
        n.digests = digests
            .into_iter()
            .map(|(from_seg, to_seg)| {
                serde_json::from_value(serde_json::json!({ "fromSeg": from_seg, "toSeg": to_seg, "startTs": 0, "endTs": 0 })).unwrap()
            })
            .collect();
        n
    }

    #[test]
    fn try_again_fills_the_hole_and_moves_what_follows() {
        let marker = TranscriptSegment::failed_marker("you", (T0 + 15_000) / 1000, 15_000, 30_000);
        let base = vec![line("them", "a", 0, 1_000), marker.clone(), line("them", "b", 31_000, 32_000)];
        // Two lines in its place: the stop after "b" and the digest move on.
        let mut n = note_with(base.clone(), vec![3], vec![(0, 2), (2, 3)]);
        fill(&mut n, 1, vec![line("you", "one", 15_000, 20_000), line("you", "two", 21_000, 29_000)]);
        let texts: Vec<&str> = n.transcript.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["a", "one", "two", "b"]);
        assert_eq!(n.breaks, vec![4]);
        assert_eq!(n.digests.iter().map(|d| (d.from_seg, d.to_seg)).collect::<Vec<_>>(), [(0, 3), (3, 4)]);
        // Quiet after all: the marker just goes.
        let mut n = note_with(base, vec![2, 3], vec![(0, 2)]);
        fill(&mut n, 1, vec![]);
        assert_eq!(n.transcript.len(), 2);
        assert_eq!(n.breaks, vec![1, 2]);
        assert_eq!(n.digests.iter().map(|d| (d.from_seg, d.to_seg)).collect::<Vec<_>>(), [(0, 1)]);
    }
}

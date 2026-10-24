//! The speaker timeline of one recording's call side, put together from what
//! the `yap.exe diarize` child(ren) report. Plain bookkeeping, unit-tested.

use serde::Serialize;

use super::protocol::RawSegment;

/// A stretch of one voice on a recording's call side ("Them").
///
/// Times are milliseconds on that recording's **session clock**, the one
/// `meeting.rs` counts its "Them" samples on (a position in samples / 16):
/// 0 is the recording's first sample, silence the recorder filled in counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    /// 1…8 within its `stream`, in the order the voices were first heard.
    pub speaker: u8,
    /// Which engine process heard it: 0, or 1… after a crash and restart. A
    /// restarted stream can't tell whether its Speaker 1 is the earlier one,
    /// so a voice is `(stream, speaker)`.
    pub stream: u32,
}

/// Who spoke when on a recording's call side, so far or (`done`) in full.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Timeline {
    /// The meeting note being recorded.
    pub note_id: u64,
    /// This recording's id (a note recorded, paused and resumed has one per
    /// recording, each with its own session clock and speaker numbers).
    pub session: u64,
    /// Sorted by start; the tail can still change until `done`.
    pub segments: Vec<Segment>,
    /// Call-side audio handed to the engine.
    pub audio_ms: u64,
    /// Labels are settled up to here (all of `audio_ms` once `done`). On the
    /// CPU's 30 s preset this trails the audio by ~10 s (at most ~21 s).
    pub labelled_ms: u64,
    /// The recording stopped and the engine finished.
    pub done: bool,
    /// Engine processes that died and were started again.
    pub restarts: u32,
    /// Stretches no engine labelled (between a crash and its restart).
    pub gaps: Vec<[u64; 2]>,
    /// Why labels stopped early, if they did (the meeting itself records on).
    pub error: Option<String>,
}

impl Timeline {
    /// The voices heard, as `(stream, speaker)`, in the order they first spoke.
    #[cfg_attr(not(test), allow(dead_code))] // for the speaker map (diarize.rs)
    pub fn voices(&self) -> Vec<(u32, u8)> {
        let mut out: Vec<(u32, u8)> = Vec::new();
        for s in &self.segments {
            if !out.contains(&(s.stream, s.speaker)) {
                out.push((s.stream, s.speaker));
            }
        }
        out
    }
}

/// The engine's speaker ids → 1…8 in the order they were first heard,
/// stable for the whole stream: an id keeps its number even when a later
/// update moves its first segment.
#[derive(Debug, Default)]
pub struct SpeakerOrder {
    seen: Vec<i32>,
}

impl SpeakerOrder {
    /// The engine's segments (seconds, its ids) as ours: milliseconds, sorted
    /// by start, empty ones and invalid ids dropped.
    pub fn convert(&mut self, raw: &[(f64, f64, i32)]) -> Vec<RawSegment> {
        let mut sorted: Vec<(u64, u64, i32)> = raw
            .iter()
            .filter(|(a, b, id)| *id > 0 && a.is_finite() && b.is_finite() && b > a)
            .map(|&(a, b, id)| (secs_to_ms(a), secs_to_ms(b), id))
            .filter(|(a, b, _)| b > a)
            .collect();
        sorted.sort_by_key(|&(a, b, _)| (a, b));
        sorted
            .into_iter()
            .map(|(start_ms, end_ms, id)| {
                let n = match self.seen.iter().position(|&s| s == id) {
                    Some(i) => i,
                    None => {
                        self.seen.push(id);
                        self.seen.len() - 1
                    }
                };
                RawSegment {
                    start_ms,
                    end_ms,
                    speaker: (n + 1).min(u8::MAX as usize) as u8,
                }
            })
            .collect()
    }
}

fn secs_to_ms(s: f64) -> u64 {
    (s.max(0.0) * 1000.0).round() as u64
}

/// One engine process's view of the recording.
#[derive(Debug)]
struct Run {
    stream: u32,
    /// Where on the session clock its audio started.
    offset_ms: u64,
    segments: Vec<RawSegment>,
    labelled_ms: u64,
}

/// The runs (one per engine process) of a recording.
#[derive(Debug, Default)]
pub struct Runs {
    runs: Vec<Run>,
}

impl Runs {
    /// An engine process started, fed from `offset_ms` of the session clock.
    pub fn begin(&mut self, stream: u32, offset_ms: u64) {
        self.runs.push(Run {
            stream,
            offset_ms,
            segments: Vec::new(),
            labelled_ms: 0,
        });
    }

    /// Its latest list (times from its own first sample).
    pub fn update(&mut self, stream: u32, segments: Vec<RawSegment>, labelled_ms: u64) {
        if let Some(run) = self.runs.iter_mut().find(|r| r.stream == stream) {
            run.segments = segments;
            run.labelled_ms = labelled_ms;
        }
    }

    /// Every run's segments on the session clock, how far labels are settled
    /// and the gaps between runs.
    pub fn merged(&self) -> (Vec<Segment>, u64, Vec<[u64; 2]>) {
        let mut segments = Vec::new();
        let mut gaps = Vec::new();
        for (i, run) in self.runs.iter().enumerate() {
            // A run can't speak for audio its successor got.
            let until = self.runs.get(i + 1).map(|next| next.offset_ms);
            for s in &run.segments {
                let start_ms = run.offset_ms + s.start_ms;
                let mut end_ms = run.offset_ms + s.end_ms;
                if let Some(until) = until {
                    if start_ms >= until {
                        continue;
                    }
                    end_ms = end_ms.min(until);
                }
                segments.push(Segment {
                    start_ms,
                    end_ms,
                    speaker: s.speaker,
                    stream: run.stream,
                });
            }
            if let Some(until) = until {
                let labelled_to = run.offset_ms + run.labelled_ms;
                if labelled_to < until {
                    gaps.push([labelled_to, until]);
                }
            }
        }
        segments.sort_by_key(|s| (s.start_ms, s.end_ms));
        let labelled = self.runs.last().map(|r| r.offset_ms + r.labelled_ms).unwrap_or(0);
        (segments, labelled, gaps)
    }
}

/// At most `max` voices: the ones who talked most stay, each other voice's
/// segments go to whoever kept talks nearest in time, and touching segments
/// of one voice are joined. For the debug `diarize_wav { speakers }` (the
/// engine takes no count; the spike found forcing one costs accuracy, so
/// meetings never use this).
#[cfg_attr(not(debug_assertions), allow(dead_code))] // diarize_wav (debug builds)
pub fn cap_speakers(segments: &[RawSegment], max: u8) -> Vec<RawSegment> {
    let mut talk: Vec<(u8, u64)> = Vec::new();
    for s in segments {
        match talk.iter_mut().find(|(id, _)| *id == s.speaker) {
            Some((_, t)) => *t += s.end_ms - s.start_ms,
            None => talk.push((s.speaker, s.end_ms - s.start_ms)),
        }
    }
    if max == 0 || talk.len() <= max as usize {
        return segments.to_vec();
    }
    talk.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let kept: Vec<u8> = talk.iter().take(max as usize).map(|(id, _)| *id).collect();
    let anchors: Vec<RawSegment> = segments.iter().copied().filter(|s| kept.contains(&s.speaker)).collect();
    let distance = |a: &RawSegment, b: &RawSegment| {
        if a.end_ms <= b.start_ms {
            b.start_ms - a.end_ms
        } else {
            a.start_ms.saturating_sub(b.end_ms)
        }
    };
    let mut out: Vec<RawSegment> = segments
        .iter()
        .map(|s| {
            if kept.contains(&s.speaker) {
                return *s;
            }
            let nearest = anchors.iter().min_by_key(|a| distance(a, s)).map(|a| a.speaker);
            RawSegment {
                speaker: nearest.unwrap_or(s.speaker),
                ..*s
            }
        })
        .collect();
    out.sort_by_key(|s| (s.start_ms, s.end_ms));
    // Join a voice's touching segments.
    let mut joined: Vec<RawSegment> = Vec::new();
    for s in out {
        match joined.iter_mut().rev().find(|j| j.speaker == s.speaker) {
            Some(j) if s.start_ms <= j.end_ms => j.end_ms = j.end_ms.max(s.end_ms),
            _ => joined.push(s),
        }
    }
    // Renumber 1… by first appearance again.
    let mut order = Vec::new();
    for s in &mut joined {
        let i = match order.iter().position(|&o| o == s.speaker) {
            Some(i) => i,
            None => {
                order.push(s.speaker);
                order.len() - 1
            }
        };
        s.speaker = i as u8 + 1;
    }
    joined
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start_ms: u64, end_ms: u64, speaker: u8) -> RawSegment {
        RawSegment { start_ms, end_ms, speaker }
    }

    #[test]
    fn speakers_are_numbered_by_first_appearance_and_keep_their_numbers() {
        let mut order = SpeakerOrder::default();
        // The engine's ids are its own; the earliest voice is Speaker 1.
        let first = order.convert(&[(5.0, 6.0, 1), (0.2, 4.0, 3), (7.0, 7.5, 3)]);
        assert_eq!(first, vec![seg(200, 4_000, 1), seg(5_000, 6_000, 2), seg(7_000, 7_500, 1)]);
        // A later update moves engine id 1 earlier: numbers don't change.
        let later = order.convert(&[(0.0, 0.1, 1), (0.2, 4.0, 3), (9.0, 9.5, 2)]);
        assert_eq!(later, vec![seg(0, 100, 2), seg(200, 4_000, 1), seg(9_000, 9_500, 3)]);
    }

    #[test]
    fn empty_and_invalid_segments_are_dropped() {
        let mut order = SpeakerOrder::default();
        let out = order.convert(&[
            (1.0, 1.0, 1),
            (2.0, 1.0, 1),
            (1.0, 2.0, 0),
            (1.0, 2.0, -1),
            (f64::NAN, 2.0, 1),
            (3.0, f64::INFINITY, 1),
            (4.0, 4.0004, 1), // rounds to nothing
            (-0.5, 0.25, 2),
        ]);
        assert_eq!(out, vec![seg(0, 250, 1)]);
    }

    #[test]
    fn one_run_is_the_timeline() {
        let mut runs = Runs::default();
        runs.begin(0, 0);
        runs.update(0, vec![seg(0, 1_000, 1), seg(1_200, 2_000, 2)], 1_800);
        let (segments, labelled, gaps) = runs.merged();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[1], Segment { start_ms: 1_200, end_ms: 2_000, speaker: 2, stream: 0 });
        assert_eq!(labelled, 1_800);
        assert!(gaps.is_empty());
    }

    #[test]
    fn a_restart_starts_a_new_stream_at_its_place_on_the_clock() {
        let mut runs = Runs::default();
        runs.begin(0, 0);
        // The first engine labelled 0–40 s, then died at 52 s of audio.
        runs.update(0, vec![seg(0, 30_000, 1), seg(31_000, 41_000, 2)], 40_000);
        runs.begin(1, 52_000);
        runs.update(1, vec![seg(500, 9_000, 1)], 9_500);
        let (segments, labelled, gaps) = runs.merged();
        assert_eq!(
            segments,
            vec![
                Segment { start_ms: 0, end_ms: 30_000, speaker: 1, stream: 0 },
                Segment { start_ms: 31_000, end_ms: 41_000, speaker: 2, stream: 0 },
                Segment { start_ms: 52_500, end_ms: 61_000, speaker: 1, stream: 1 },
            ]
        );
        assert_eq!(labelled, 61_500);
        assert_eq!(gaps, vec![[40_000, 52_000]]);
        let tl = Timeline { segments, ..Default::default() };
        assert_eq!(tl.voices(), vec![(0, 1), (0, 2), (1, 1)]);
    }

    #[test]
    fn a_dead_run_never_speaks_past_its_successor() {
        let mut runs = Runs::default();
        runs.begin(0, 0);
        runs.update(0, vec![seg(0, 12_000, 1), seg(13_000, 14_000, 2)], 14_000);
        runs.begin(1, 10_000);
        let (segments, _, gaps) = runs.merged();
        assert_eq!(segments, vec![Segment { start_ms: 0, end_ms: 10_000, speaker: 1, stream: 0 }]);
        assert!(gaps.is_empty());
        // Updates for a stream that never began are ignored.
        runs.update(7, vec![seg(0, 1, 1)], 1);
        assert_eq!(runs.merged().0.len(), 1);
    }

    #[test]
    fn capping_folds_the_quietest_voices_into_their_neighbours() {
        let segs = vec![
            seg(0, 10_000, 1),
            seg(10_000, 10_400, 3), // a short one right after Speaker 1
            seg(10_400, 20_000, 1),
            seg(21_000, 30_000, 2),
            seg(40_000, 40_300, 4), // nearest to Speaker 2's end
        ];
        let capped = cap_speakers(&segs, 2);
        assert_eq!(capped, vec![seg(0, 20_000, 1), seg(21_000, 30_000, 2), seg(40_000, 40_300, 2)]);
        // A cap the timeline already fits changes nothing; 0 means none.
        assert_eq!(cap_speakers(&segs, 4), segs);
        assert_eq!(cap_speakers(&segs, 0), segs);
    }

    #[test]
    fn capping_renumbers_by_first_appearance() {
        // Speaker 1 barely talks: Speakers 2 and 3 stay, as 1 and 2 (a gap
        // between two of one voice's segments stays a gap).
        let segs = vec![seg(0, 500, 1), seg(1_000, 9_000, 2), seg(9_500, 20_000, 3)];
        assert_eq!(
            cap_speakers(&segs, 2),
            vec![seg(0, 500, 1), seg(1_000, 9_000, 1), seg(9_500, 20_000, 2)]
        );
    }
}

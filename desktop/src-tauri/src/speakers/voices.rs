//! **Deciding about a meeting's voices** (pure, unit-tested; the prints
//! come from `voiceprint.rs`, the audio from the meeting on disk):
//!
//! - [`match_voices`] — which of a resumed recording's voices (or a
//!   restarted engine's) are voices heard before the pause, so "Speaker 2"
//!   before and after is one voice when it is ([`SAME`] and [`MARGIN`]:
//!   left apart when unsure);
//! - [`map_by_overlap`] — the speaker engine run again over a meeting
//!   (Find speakers, `rerun.rs`): which new voice is which old one, by the
//!   lines they share, so names stay;
//! - [`alike_pairs`] and [`split`] — two labelled voices that may be one
//!   person ("These may be one voice · Merge"), one label whose lines fall
//!   into two clear groups ("Split"): offered, never done by themselves;
//! - [`choose_clip`] — which few seconds of a voice to play from its menu
//!   or its name card: its clearest speech.
//!
//! The thresholds are cosine similarities of ERes2Net prints, measured on
//! the spike's test meetings (`voices::tests::real_*`, see speakers.md).

use super::voiceprint::{centroid, similarity, Print};

// ---- One voice, heard again ----

/// Two voices whose prints (each from at least [`MATCH_MIN_MS`] of
/// speech) are this alike are one voice heard again…
pub const SAME: f32 = 0.70;
/// …when that's also this far ahead of each one's next best: otherwise
/// they're left apart (a new "Speaker 4" is better than two people as one).
pub const MARGIN: f32 = 0.15;
/// Speech a voice needs before it's matched (or a print is trusted).
pub const MATCH_MIN_MS: u64 = 6_000;
/// A new voice this unlike every earlier one, with [`SETTLED_MS`] of
/// speech, is new for good: not asked about again.
pub const NEW_BELOW: f32 = 0.45;
pub const SETTLED_MS: u64 = 20_000;

/// One voice with its print: how much speech it was made from.
#[derive(Debug, Clone)]
pub struct Voice<K> {
    pub key: K,
    pub print: Print,
    pub ms: u64,
}

/// Which of `new`'s voices are `old`'s (new key, old key, how alike), one
/// to one: a pair is taken when they're at least [`SAME`] alike, both from
/// [`MATCH_MIN_MS`] of speech, and that's at least [`MARGIN`] ahead of
/// what either is to anyone else. The rest are left apart.
pub fn match_voices<A: Clone, B: Clone>(old: &[Voice<A>], new: &[Voice<B>]) -> Vec<(B, A, f32)> {
    let sim: Vec<Vec<f32>> = new.iter().map(|n| old.iter().map(|o| similarity(&n.print, &o.print)).collect()).collect();
    let mut pairs: Vec<(usize, usize, f32)> = Vec::new();
    for (i, n) in new.iter().enumerate() {
        for (j, o) in old.iter().enumerate() {
            let s = sim[i][j];
            if s < SAME || n.ms < MATCH_MIN_MS || o.ms < MATCH_MIN_MS {
                continue;
            }
            let row_next = (0..old.len()).filter(|&k| k != j).map(|k| sim[i][k]).fold(f32::MIN, f32::max);
            let col_next = (0..new.len()).filter(|&k| k != i).map(|k| sim[k][j]).fold(f32::MIN, f32::max);
            if s - row_next.max(-1.0) >= MARGIN && s - col_next.max(-1.0) >= MARGIN {
                pairs.push((i, j, s));
            }
        }
    }
    pairs.sort_by(|a, b| b.2.total_cmp(&a.2));
    let (mut used_new, mut used_old) = (vec![false; new.len()], vec![false; old.len()]);
    let mut out = Vec::new();
    for (i, j, s) in pairs {
        if !used_new[i] && !used_old[j] {
            used_new[i] = true;
            used_old[j] = true;
            out.push((new[i].key.clone(), old[j].key.clone(), s));
        }
    }
    out
}

/// What a new voice is so far: one of the earlier ones (`Some`), new for
/// good (`None` in `Some(None)`), or not known yet (`None`: more speech).
pub fn verdict<A: Clone>(old: &[Voice<A>], new: &Voice<()>) -> Option<Option<(A, f32)>> {
    if let Some((_, key, s)) = match_voices(old, std::slice::from_ref(new)).into_iter().next() {
        return Some(Some((key, s)));
    }
    let best = old
        .iter()
        .filter(|o| o.ms >= MATCH_MIN_MS)
        .map(|o| similarity(&new.print, &o.print))
        .fold(f32::MIN, f32::max);
    (new.ms >= SETTLED_MS && best < NEW_BELOW).then_some(None)
}

// ---- The engine run again: new voices onto old ones ----

/// How a new voice (the engine run again) and an old voice (on the lines)
/// overlap: `ms` of the old voice's lines the new voice's turns cover,
/// `assigned` of them on lines the person said who said.
#[derive(Debug, Clone, PartialEq)]
pub struct Overlap<N, O> {
    pub new: N,
    pub old: O,
    pub ms: u64,
    pub assigned: u64,
}

/// An old voice: its lines' length (ms) and how far its name is trusted
/// (0 unnamed … 4 the person's own).
#[derive(Debug, Clone, PartialEq)]
pub struct OldVoice<O> {
    pub id: O,
    pub ms: u64,
    pub rank: u8,
}

/// Which new voice takes which old voice's id (and so its name): a pair
/// qualifies when the new voice holds at least half of the old one's lines,
/// or at least half of what the new voice said on labelled lines was the
/// old one's. One to one, the best-named old voice first (a name the
/// person gave is never lost to an unnamed voice the engine now says is the
/// same person), then by overlap (lines the person assigned count three
/// times). A new voice left over is new; an old one left over loses its
/// lines.
pub fn map_by_overlap<N: Clone + PartialEq, O: Clone + PartialEq>(overlaps: &[Overlap<N, O>], old: &[OldVoice<O>]) -> Vec<(N, O)> {
    let weight = |o: &Overlap<N, O>| o.ms + 2 * o.assigned;
    let new_total = |n: &N| overlaps.iter().filter(|o| &o.new == n).map(&weight).sum::<u64>();
    let mut pairs: Vec<(&Overlap<N, O>, u8, u64)> = overlaps
        .iter()
        .filter_map(|o| {
            let v = old.iter().find(|v| v.id == o.old)?;
            let w = weight(o);
            let qualifies = w > 0 && (2 * o.ms >= v.ms || 2 * w >= new_total(&o.new));
            qualifies.then_some((o, v.rank, w))
        })
        .collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)));
    let mut out: Vec<(N, O)> = Vec::new();
    for (o, _, _) in pairs {
        if !out.iter().any(|(n, v)| *n == o.new || *v == o.old) {
            out.push((o.new.clone(), o.old.clone()));
        }
    }
    out
}

// ---- Flags: two voices that may be one, one that may be two ----

/// Two labelled voices this alike may be one person the engine split.
pub const ALIKE: f32 = 0.70;
/// Speech each needs before it's compared.
pub const FLAG_MIN_MS: u64 = 10_000;

/// Pairs of `voices` that may be one person (a, b, how alike), the most
/// alike first, each voice in one pair at most.
pub fn alike_pairs<K: Clone + PartialEq>(voices: &[Voice<K>]) -> Vec<(K, K, f32)> {
    let mut pairs: Vec<(usize, usize, f32)> = Vec::new();
    for i in 0..voices.len() {
        for j in i + 1..voices.len() {
            let (a, b) = (&voices[i], &voices[j]);
            if a.ms < FLAG_MIN_MS || b.ms < FLAG_MIN_MS || a.key == b.key {
                continue;
            }
            let s = similarity(&a.print, &b.print);
            if s >= ALIKE {
                pairs.push((i, j, s));
            }
        }
    }
    pairs.sort_by(|a, b| b.2.total_cmp(&a.2));
    let mut used = vec![false; voices.len()];
    let mut out = Vec::new();
    for (i, j, s) in pairs {
        if !used[i] && !used[j] {
            used[i] = true;
            used[j] = true;
            out.push((voices[i].key.clone(), voices[j].key.clone(), s));
        }
    }
    out
}

/// One line of a voice, with its print: what [`split`] groups.
#[derive(Debug, Clone)]
pub struct Line<K> {
    pub key: K,
    pub print: Print,
    pub ms: u64,
    /// The person said who said it: its group keeps the voice.
    pub assigned: bool,
}

/// A voice's lines in two groups: `moved` would become a new voice.
#[derive(Debug, Clone, PartialEq)]
pub struct Split<K> {
    pub moved: Vec<K>,
    pub kept: Vec<K>,
    /// How alike the two groups sound (lower: more clearly two).
    pub alike: f32,
    /// How much nearer each line is to its own group than to the other,
    /// on average.
    pub gap: f32,
    /// The smaller group's share of the voice: of its speech or of its
    /// lines, whichever is less.
    pub share: f32,
}

/// Two groups this unalike, at most (one voice's lines split in two score
/// 0.93–0.97 on the test meetings; two similar women heard as one, 0.65)…
pub const SPLIT_APART: f32 = 0.75;
/// …each line this much nearer its own group on average (one voice:
/// 0.02–0.06; two: 0.32)…
pub const SPLIT_GAP: f32 = 0.15;
/// …each group at least this much speech, in this many lines…
pub const SPLIT_MIN_MS: u64 = 15_000;
pub const SPLIT_MIN_LINES: usize = 3;
/// …and the smaller a fifth of the voice at least (a few lines over
/// crosstalk, where two people talk at once, aren't a second person).
pub const SPLIT_MIN_SHARE: f32 = 0.2;

/// Whether `lines` (one label's) are clearly two voices: two-means on their
/// prints (cosine, weighted by length, started from the two least alike),
/// then kept only when each group has [`SPLIT_MIN_MS`] in
/// [`SPLIT_MIN_LINES`] lines and the smaller [`SPLIT_MIN_SHARE`] of it,
/// the groups are at most [`SPLIT_APART`] alike, and lines sit
/// [`SPLIT_GAP`] nearer their own group. The group with the lines the
/// person assigned (else the bigger) keeps the voice.
pub fn split<K: Clone>(lines: &[Line<K>]) -> Option<Split<K>> {
    two_groups(lines).filter(|s| s.alike <= SPLIT_APART && s.gap >= SPLIT_GAP && s.share >= SPLIT_MIN_SHARE)
}

/// [`split`]'s two groups before its last tests (how alike, how clear, how
/// big): what the real-meeting checks print.
pub fn two_groups<K: Clone>(lines: &[Line<K>]) -> Option<Split<K>> {
    let n = lines.len();
    if n < 2 * SPLIT_MIN_LINES || lines.iter().map(|l| l.ms).sum::<u64>() < 2 * SPLIT_MIN_MS {
        return None;
    }
    let weighted = |idx: &[usize]| centroid(idx.iter().map(|&i| (lines[i].print.as_slice(), lines[i].ms as f32)));
    let all: Vec<usize> = (0..n).collect();
    let whole = weighted(&all)?;
    // Start from the line least like the whole, and the one least like it.
    let a0 = (0..n).min_by(|&i, &j| similarity(&lines[i].print, &whole).total_cmp(&similarity(&lines[j].print, &whole)))?;
    let b0 = (0..n).min_by(|&i, &j| similarity(&lines[i].print, &lines[a0].print).total_cmp(&similarity(&lines[j].print, &lines[a0].print)))?;
    let (mut ca, mut cb) = (lines[a0].print.clone(), lines[b0].print.clone());
    let mut group = vec![false; n]; // true: b
    for _ in 0..25 {
        let next: Vec<bool> = lines.iter().map(|l| similarity(&l.print, &cb) > similarity(&l.print, &ca)).collect();
        let (ia, ib): (Vec<usize>, Vec<usize>) = (0..n).partition(|&i| !next[i]);
        if ia.is_empty() || ib.is_empty() {
            return None;
        }
        ca = weighted(&ia)?;
        cb = weighted(&ib)?;
        if next == group {
            break;
        }
        group = next;
    }
    let (ia, ib): (Vec<usize>, Vec<usize>) = (0..n).partition(|&i| !group[i]);
    let ms = |idx: &[usize]| idx.iter().map(|&i| lines[i].ms).sum::<u64>();
    if ia.len() < SPLIT_MIN_LINES || ib.len() < SPLIT_MIN_LINES || ms(&ia) < SPLIT_MIN_MS || ms(&ib) < SPLIT_MIN_MS {
        return None;
    }
    let alike = similarity(&ca, &cb);
    let total = ms(&all).max(1) as f32;
    let gap = (0..n)
        .map(|i| {
            let (own, other) = if group[i] { (&cb, &ca) } else { (&ca, &cb) };
            (similarity(&lines[i].print, own) - similarity(&lines[i].print, other)) * lines[i].ms as f32
        })
        .sum::<f32>()
        / total;
    let share = (ms(&ia).min(ms(&ib)) as f32 / total).min(ia.len().min(ib.len()) as f32 / n as f32);
    let assigned = |idx: &[usize]| idx.iter().filter(|&&i| lines[i].assigned).map(|&i| lines[i].ms).sum::<u64>();
    let a_keeps = match assigned(&ia).cmp(&assigned(&ib)) {
        std::cmp::Ordering::Equal => ms(&ia) >= ms(&ib),
        o => o == std::cmp::Ordering::Greater,
    };
    let keys = |idx: &[usize]| idx.iter().map(|&i| lines[i].key.clone()).collect::<Vec<K>>();
    let (kept, moved) = if a_keeps { (keys(&ia), keys(&ib)) } else { (keys(&ib), keys(&ia)) };
    Some(Split { moved, kept, alike, gap, share })
}

// ---- A voice's clip ----

/// A line a voice's clip could come from.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClipLine {
    pub start_ms: u64,
    pub end_ms: u64,
    /// Its words' ends (ms on the same clock), when known: where a long
    /// line is cut.
    pub word_ends: Vec<u64>,
    pub words: usize,
    /// Another voice talks within [`CROSSTALK_MS`] of it (either side of
    /// the call): not clear.
    pub crosstalk: bool,
    /// The person said who said it: surely this voice.
    pub assigned: bool,
    /// Its loudness (RMS), once read: near-silent lines aren't a voice.
    pub level: Option<f32>,
}

/// The clip's longest.
pub const CLIP_MAX_MS: u64 = 8_000;
/// A line shorter than this says too little (unless nothing else does).
pub const CLIP_MIN_MS: u64 = 1_500;
/// Another voice this near a line makes it crosstalk.
pub const CROSSTALK_MS: u64 = 500;
/// Quieter than this (RMS) is hardly speech.
pub const QUIET: f32 = 0.004;

/// How good a line is to play as its voice (higher is better).
pub fn clip_score(l: &ClipLine) -> f32 {
    let dur = l.end_ms.saturating_sub(l.start_ms);
    if dur == 0 {
        return 0.0;
    }
    // Longer is better, up to the clip's length.
    let mut score = dur.min(CLIP_MAX_MS) as f32 / CLIP_MAX_MS as f32;
    if dur < CLIP_MIN_MS {
        score *= 0.2;
    }
    if l.crosstalk {
        score *= 0.3;
    }
    // Mostly pause, few words: a poor sample.
    let rate = l.words as f32 / (dur as f32 / 1000.0);
    if l.words > 0 && rate < 1.2 {
        score *= 0.6;
    }
    if l.assigned {
        score *= 1.25;
    }
    match l.level {
        Some(level) if level < QUIET => score *= 0.1,
        _ => {}
    }
    score
}

/// The clip to play for a voice: which of `lines`, and its stretch (cut at
/// a word's end to at most [`CLIP_MAX_MS`]). `None`: no line.
pub fn choose_clip(lines: &[ClipLine]) -> Option<(usize, u64, u64)> {
    let best = (0..lines.len()).filter(|&i| clip_score(&lines[i]) > 0.0).max_by(|&a, &b| {
        clip_score(&lines[a])
            .total_cmp(&clip_score(&lines[b]))
            // A tie: the earlier line (the voice as first heard).
            .then(lines[b].start_ms.cmp(&lines[a].start_ms))
    })?;
    let l = &lines[best];
    let mut end = l.end_ms;
    if end - l.start_ms > CLIP_MAX_MS {
        let limit = l.start_ms + CLIP_MAX_MS;
        end = l
            .word_ends
            .iter()
            .copied()
            .filter(|&e| e <= limit && e > l.start_ms + CLIP_MIN_MS)
            .max()
            .unwrap_or(limit);
    }
    Some((best, l.start_ms, end))
}

#[cfg(all(test, feature = "engines"))]
mod real;

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit print near axis `axis` of 8, tilted by `tilt` towards axis 7.
    fn print(axis: usize, tilt: f32) -> Print {
        let mut v = vec![0.0f32; 8];
        v[axis] = 1.0;
        v[7] += tilt;
        super::super::voiceprint::normalized(v).unwrap()
    }

    fn voice<K>(key: K, p: Print, ms: u64) -> Voice<K> {
        Voice { key, print: p, ms }
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn the_thresholds_hang_together() {
        assert!(SAME > NEW_BELOW && SAME + MARGIN <= 1.0);
        assert!(ALIKE >= SAME - 0.1, "flagging alike is no looser than matching");
        assert!(SPLIT_APART < 0.9, "well under what one voice's lines in two groups score");
        assert!(SPLIT_MIN_SHARE > 0.0 && SPLIT_MIN_SHARE < 0.5);
        assert!(SETTLED_MS > MATCH_MIN_MS);
    }

    #[test]
    fn voices_after_a_pause_match_the_ones_before_and_unsure_ones_stay_apart() {
        // Before the pause: Speakers 1–3. After: three voices, two of them
        // heard before (a bit different, as a voice is), one new.
        let old = vec![
            voice("them-1", print(0, 0.0), 40_000),
            voice("them-2", print(1, 0.0), 40_000),
            voice("them-3", print(2, 0.0), 40_000),
        ];
        let new = vec![
            voice((0u32, 1u8), print(1, 0.3), 20_000), // Speaker 2 again
            voice((0, 2), print(4, 0.0), 20_000),      // someone new
            voice((0, 3), print(0, 0.4), 20_000),      // Speaker 1 again
        ];
        let mut got = match_voices(&old, &new);
        got.sort_by_key(|(k, _, _)| *k);
        let got: Vec<((u32, u8), &str)> = got.iter().map(|(k, v, _)| (*k, *v)).collect();
        assert_eq!(got, vec![((0, 1), "them-2"), ((0, 3), "them-1")]);

        // Not enough speech yet: no answer either way.
        let short = vec![voice((0u32, 1u8), print(1, 0.0), MATCH_MIN_MS - 1)];
        assert!(match_voices(&old, &short).is_empty());

        // A voice as like two earlier ones as each other (similar voices):
        // left apart rather than guessed.
        let twins = vec![voice("a", print(0, 1.0), 30_000), voice("b", print(1, 1.0), 30_000)];
        let between = super::super::voiceprint::normalized(vec![0.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]).unwrap();
        assert!(similarity(&between, &twins[0].print) >= SAME, "alike enough to match either…");
        assert!(match_voices(&twins, &[voice((), between.clone(), 30_000)]).is_empty(), "…so it matches neither");

        // Two new voices both like one earlier voice: neither takes it
        // (the engine heard them as two people).
        let two = vec![voice(1, print(0, 0.1), 20_000), voice(2, print(0, 0.2), 20_000)];
        assert!(match_voices(&old[..1], &two).is_empty());
    }

    #[test]
    fn the_threshold_is_where_it_says() {
        // Exactly at SAME with a clear margin: matched; just under: apart.
        let at = |s: f32| {
            let p = vec![s, (1.0 - s * s).sqrt(), 0.0];
            match_voices(&[voice("old", vec![1.0, 0.0, 0.0], 20_000)], &[voice("new", p, 20_000)])
        };
        assert_eq!(at(SAME + 0.001).len(), 1);
        assert!(at(SAME - 0.01).is_empty());
    }

    #[test]
    fn a_new_voice_is_new_for_good_only_when_it_is_clearly_nobody_heard_before() {
        let old = vec![voice("them-1", print(0, 0.0), 40_000), voice("them-2", print(1, 0.0), 40_000)];
        assert_eq!(verdict(&old, &voice((), print(1, 0.2), 10_000)).map(|v| v.map(|(k, _)| k)), Some(Some("them-2")));
        assert_eq!(verdict(&old, &voice((), print(4, 0.0), SETTLED_MS)).map(|v| v.is_none()), Some(true));
        assert!(verdict(&old, &voice((), print(4, 0.0), SETTLED_MS - 1)).is_none(), "not enough speech yet");
        let unsure = super::super::voiceprint::normalized(vec![0.6, 0.0, 0.0, 0.0, 0.8, 0.0, 0.0, 0.0]).unwrap();
        assert!(verdict(&old, &voice((), unsure, 60_000)).is_none(), "neither: ask again with more");
    }

    #[test]
    fn a_run_again_keeps_the_old_ids_by_overlap_and_names_win() {
        let old = vec![
            OldVoice { id: "them-1", ms: 60_000, rank: 0 },
            OldVoice { id: "them-2", ms: 30_000, rank: 0 },
            OldVoice { id: "them-3", ms: 20_000, rank: 4 }, // "Sam", the person's name
        ];
        let ov = |new: u8, old: &'static str, ms: u64| Overlap { new, old, ms, assigned: 0 };
        // The same voices, renumbered: 1 ↔ them-2, 2 ↔ them-1, 3 ↔ them-3.
        let mapped = map_by_overlap(&[ov(1, "them-2", 29_000), ov(2, "them-1", 58_000), ov(3, "them-3", 20_000), ov(2, "them-2", 1_000)], &old);
        let mut m = mapped.clone();
        m.sort();
        assert_eq!(m, vec![(1, "them-2"), (2, "them-1"), (3, "them-3")]);

        // The engine now hears them-1 and Sam as one voice: it's Sam (the
        // person's name isn't lost to an unnamed voice).
        let one = map_by_overlap(&[ov(1, "them-1", 60_000), ov(1, "them-3", 20_000), ov(2, "them-2", 30_000)], &old);
        assert!(one.contains(&(1, "them-3")) && one.contains(&(2, "them-2")) && one.len() == 2);

        // A voice it couldn't tell apart before, now two: the bigger part
        // keeps the id, the other is new.
        let two = map_by_overlap(&[ov(1, "them-1", 40_000), ov(4, "them-1", 20_000)], &old[..1]);
        assert_eq!(two, vec![(1, "them-1")]);
        // Lines the person assigned weigh more.
        let assigned = map_by_overlap(
            &[ov(1, "them-1", 40_000), Overlap { new: 4, old: "them-1", ms: 20_000, assigned: 12_000 }],
            &old[..1],
        );
        assert_eq!(assigned, vec![(4, "them-1")]);
        // A sliver of overlap claims nothing.
        assert!(map_by_overlap(&[ov(1, "them-1", 2_000), ov(1, "them-2", 28_000)], &old[..1]).is_empty());
    }

    #[test]
    fn two_voices_that_sound_alike_are_flagged_once() {
        let voices = vec![
            voice("them-1", print(0, 0.0), 30_000),
            voice("them-2", print(1, 0.0), 30_000),
            voice("them-4", print(0, 0.2), 30_000), // them-1 again, split off
            voice("them-5", print(0, 0.25), FLAG_MIN_MS - 1),
        ];
        let flags = alike_pairs(&voices);
        assert_eq!(flags.len(), 1);
        assert_eq!((flags[0].0, flags[0].1), ("them-1", "them-4"));
        assert!(flags[0].2 >= ALIKE);
        assert!(alike_pairs(&voices[..2]).is_empty(), "different voices: nothing to offer");
    }

    fn line(key: usize, p: Print, ms: u64) -> Line<usize> {
        Line { key, print: p, ms, assigned: false }
    }

    #[test]
    fn a_label_holding_two_people_splits_and_one_person_does_not() {
        // Lines 0–4 one voice, 5–9 another, as one label.
        let mut lines: Vec<Line<usize>> = (0..5).map(|i| line(i, print(0, 0.1 * i as f32 / 5.0), 5_000)).collect();
        lines.extend((5..10).map(|i| line(i, print(1, 0.1 * (i - 5) as f32 / 5.0), 4_000)));
        let s = split(&lines).expect("two clear groups");
        assert_eq!(s.kept, vec![0, 1, 2, 3, 4], "the bigger group keeps the voice");
        assert_eq!(s.moved, vec![5, 6, 7, 8, 9]);
        assert!(s.alike <= SPLIT_APART && s.gap >= SPLIT_GAP && s.share >= SPLIT_MIN_SHARE, "{s:?}");

        // Lines the person assigned keep their group the voice, even the smaller one.
        for l in lines.iter_mut().skip(5) {
            l.assigned = true;
        }
        assert_eq!(split(&lines).unwrap().kept, vec![5, 6, 7, 8, 9]);

        // One person (prints scattered round one voice): no split.
        let one: Vec<Line<usize>> = (0..12).map(|i| line(i, print(0, 0.3 * ((i % 4) as f32 - 1.5)), 5_000)).collect();
        assert_eq!(split(&one), None);

        // Two groups, but one too small to say: no split.
        let mut few: Vec<Line<usize>> = (0..8).map(|i| line(i, print(0, 0.0), 5_000)).collect();
        few.extend((8..10).map(|i| line(i, print(1, 0.0), 10_000)));
        assert_eq!(split(&few), None);

        // Two groups that sound alike (one voice's lines, a little apart)
        // aren't "clearly two".
        let mut close: Vec<Line<usize>> = (0..5).map(|i| line(i, print(0, 2.0), 5_000)).collect();
        close.extend((5..10).map(|i| line(i, print(1, 2.0), 5_000)));
        assert_eq!(split(&close), None);

        // A few lines apart from the rest (crosstalk: two people at once):
        // too small a part of the voice to be a second person.
        let mut crosstalk: Vec<Line<usize>> = (0..17).map(|i| line(i, print(0, 0.0), 5_000)).collect();
        crosstalk.extend((17..20).map(|i| line(i, print(1, 0.0), 6_000)));
        let g = two_groups(&crosstalk).expect("two groups");
        assert!(g.alike <= SPLIT_APART && g.gap >= SPLIT_GAP && g.share < SPLIT_MIN_SHARE, "{g:?}");
        assert_eq!(split(&crosstalk), None);
    }

    #[test]
    fn the_clip_is_the_clearest_few_seconds() {
        let l = |start_ms: u64, end_ms: u64, words: usize| ClipLine { start_ms, end_ms, words, ..Default::default() };
        // A long clean line beats a short one and a long one over crosstalk.
        let lines = vec![
            l(0, 2_000, 5),
            ClipLine { crosstalk: true, ..l(10_000, 19_000, 25) },
            l(30_000, 36_000, 16),
            l(40_000, 41_000, 3),
        ];
        assert_eq!(choose_clip(&lines), Some((2, 30_000, 36_000)));
        // A line that's mostly pause loses to a dense one of the same length.
        let sparse = vec![l(0, 6_000, 3), l(10_000, 15_000, 14)];
        assert_eq!(choose_clip(&sparse).map(|c| c.0), Some(1));
        // Too quiet to be speech: passed over.
        let quiet = vec![ClipLine { level: Some(0.001), ..l(0, 8_000, 20) }, l(10_000, 13_000, 8)];
        assert_eq!(choose_clip(&quiet).map(|c| c.0), Some(1));
        // A long line is cut at a word's end within the clip's length.
        let long = vec![ClipLine { word_ends: vec![2_000, 5_000, 7_600, 9_000, 12_000], ..l(0, 12_000, 30) }];
        assert_eq!(choose_clip(&long), Some((0, 0, 7_600)));
        let no_words = vec![l(0, 12_000, 30)];
        assert_eq!(choose_clip(&no_words), Some((0, 0, CLIP_MAX_MS)));
        // Only short lines: the best of them still plays.
        assert_eq!(choose_clip(&[l(0, 900, 2), l(5_000, 6_200, 4)]).map(|c| c.0), Some(1));
        assert_eq!(choose_clip(&[]), None);
        // A line the person assigned is surely theirs.
        let assigned = vec![l(0, 6_000, 16), ClipLine { assigned: true, ..l(10_000, 15_500, 15) }];
        assert_eq!(choose_clip(&assigned).map(|c| c.0), Some(1));
    }
}

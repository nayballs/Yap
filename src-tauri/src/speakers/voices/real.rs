//! The voice features on real audio (ignored: they need the models and the
//! spike's test meetings on disk, and an `engines` build for the prints):
//! the speaker engine (the real `yap.exe diarize` child) and the
//! voice-matching model over the spike's meetings, scored against their
//! reference turns (frame error, 0.25 s collar, the best speaker mapping,
//! overlap scored — `tests/diarize_child.rs`'s `der`):
//!
//! - **one stream** (Find speakers; a meeting recorded without a pause);
//! - **a pause**: the meeting cut in two at its middle, an engine for each
//!   half (a resumed meeting's new engine) — its voices left apart (before)
//!   and matched by voiceprint ([`match_voices`], after), each match
//!   checked against the reference;
//! - **the flags** on the one stream's voices: pairs that may be one
//!   ([`alike_pairs`]) and voices that may be two ([`split`]), with the
//!   frame error before and after the split is taken.
//!
//! ```text
//! YAP_DIARIZATION_MODELS=<folder: nemotron-3-diarization/, nemo-speech-0.2.0-cpu/, eres2net-voxceleb-en/>
//! YAP_DIARIZE_EXE=<a yap.exe (the child)>
//! YAP_DIARIZATION_AUDIO=<folders of <name>.wav + <name>.rttm, ';' between>
//! YAP_VOICES_FILES=k3_similar,s3_clean   (optional; the default set below)
//! cargo test --features engines --lib speakers::voices::real -- --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};
use std::time::Instant;

use super::*;
use crate::speakers::diarize::{registry, run_saved_on, Piece, Segment};
use crate::speakers::voiceprint::Model;

type Turns = Vec<(f64, f64, String)>;

fn load_rttm(path: &Path) -> Turns {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter_map(|l| {
            let p: Vec<&str> = l.split_whitespace().collect();
            (p.first() == Some(&"SPEAKER")).then(|| {
                let s: f64 = p[3].parse().unwrap();
                let d: f64 = p[4].parse().unwrap();
                (s, s + d, p[7].to_string())
            })
        })
        .collect()
}

/// Frame-level diarization error rate with a collar (the spike's der.py).
fn der(reference: &Turns, hyp: &Turns, collar: f64) -> f64 {
    const STEP: f64 = 0.01;
    let end = reference.iter().chain(hyp).map(|s| s.1).fold(0.0, f64::max) + 1.0;
    let n = (end / STEP) as usize + 1;
    let matrix = |segs: &Turns| {
        let mut names: Vec<String> = segs.iter().map(|s| s.2.clone()).collect();
        names.sort();
        names.dedup();
        let mut m = vec![vec![false; names.len()]; n];
        for (a, b, k) in segs {
            let col = names.iter().position(|x| x == k).unwrap();
            let (from, to) = (((a / STEP).round() as usize).min(n), ((b / STEP).round() as usize).min(n));
            for row in m.iter_mut().take(to).skip(from) {
                row[col] = true;
            }
        }
        (m, names.len())
    };
    let (r, nr) = matrix(reference);
    let (h, nh) = matrix(hyp);
    let mut scored = vec![true; n];
    let c = (collar / STEP).round() as usize;
    for (a, b, _) in reference {
        for t in [a, b] {
            let i = (t / STEP).round() as usize;
            for s in scored.iter_mut().take((i + c).min(n)).skip(i.saturating_sub(c)) {
                *s = false;
            }
        }
    }
    let frames: Vec<usize> = (0..n).filter(|&i| scored[i]).collect();
    let mut co = vec![vec![0u64; nh]; nr];
    for &f in &frames {
        for i in 0..nr {
            for j in 0..nh {
                if r[f][i] && h[f][j] {
                    co[i][j] += 1;
                }
            }
        }
    }
    fn best(i: usize, co: &[Vec<u64>], used: &mut Vec<bool>, map: &mut Vec<Option<usize>>) -> (u64, Vec<Option<usize>>) {
        if i == co.len() {
            return (0, map.clone());
        }
        let mut top = best(i + 1, co, used, map);
        for j in 0..used.len() {
            if !used[j] && co[i][j] > 0 {
                used[j] = true;
                map[i] = Some(j);
                let (score, m) = best(i + 1, co, used, map);
                if score + co[i][j] > top.0 {
                    top = (score + co[i][j], m);
                }
                map[i] = None;
                used[j] = false;
            }
        }
        top
    }
    let (_, mapping) = best(0, &co, &mut vec![false; nh], &mut vec![None; nr]);
    let (mut total, mut miss, mut fa, mut conf) = (0u64, 0u64, 0u64, 0u64);
    for &f in &frames {
        let refs = r[f].iter().filter(|&&x| x).count() as u64;
        let hyps = h[f].iter().filter(|&&x| x).count() as u64;
        let correct = (0..nr).filter(|&i| r[f][i] && mapping[i].is_some_and(|j| h[f][j])).count() as u64;
        total += refs;
        miss += refs.saturating_sub(hyps);
        fa += hyps.saturating_sub(refs);
        conf += refs.min(hyps) - correct;
    }
    (miss + fa + conf) as f64 / total.max(1) as f64
}

/// The real engine over `audio`, one stream, as Find speakers runs it.
fn engine(audio: &[f32]) -> (Vec<Segment>, f64) {
    let files = registry::engine_files(crate::speakers::diarize::DEFAULT_MODEL).expect("the speaker model in YAP_DIARIZATION_MODELS");
    let t = Instant::now();
    let mut pieces = audio.chunks(16_000);
    let segs = run_saved_on(&files, || pieces.next().map(|p| Piece::Audio(p.to_vec())), &|_| {}, &|| false, "cancelled").unwrap();
    (segs, t.elapsed().as_secs_f64())
}

fn turns(segs: &[Segment], offset_s: f64, prefix: &str) -> Turns {
    segs.iter()
        .map(|s| (offset_s + s.start_ms as f64 / 1000.0, offset_s + s.end_ms as f64 / 1000.0, format!("{prefix}{}", s.speaker)))
        .collect()
}

/// The reference speaker who said most of `spans` (seconds), and their share.
fn majority(truth: &Turns, spans: &[(f64, f64)]) -> (String, f64) {
    let mut by: Vec<(String, f64)> = Vec::new();
    for &(a, b) in spans {
        for (s, e, k) in truth {
            let o = b.min(*e) - a.max(*s);
            if o > 0.0 {
                match by.iter_mut().find(|(x, _)| x == k) {
                    Some((_, t)) => *t += o,
                    None => by.push((k.clone(), o)),
                }
            }
        }
    }
    let total: f64 = by.iter().map(|(_, t)| t).sum();
    by.sort_by(|a, b| b.1.total_cmp(&a.1));
    by.first().map(|(k, t)| (k.clone(), t / total.max(1e-9))).unwrap_or_default()
}

/// Each engine voice with its lines' prints.
type VoiceLines = Vec<(u8, Vec<Line<(u64, u64)>>)>;

/// Each engine voice's lines (its segments ≥ 1.5 s, the longest 60) with
/// their prints, from `audio`.
fn voice_lines(model: &Model, audio: &[f32], segs: &[Segment]) -> VoiceLines {
    let mut by: Vec<(u8, Vec<&Segment>)> = Vec::new();
    for s in segs.iter().filter(|s| s.end_ms - s.start_ms >= 1_500) {
        match by.iter_mut().find(|(k, _)| *k == s.speaker) {
            Some((_, v)) => v.push(s),
            None => by.push((s.speaker, vec![s])),
        }
    }
    by.into_iter()
        .map(|(k, mut v)| {
            v.sort_by_key(|s| std::cmp::Reverse(s.end_ms - s.start_ms));
            v.truncate(60);
            let lines = v
                .iter()
                .filter_map(|s| {
                    let (a, b) = ((s.start_ms * 16) as usize, ((s.end_ms * 16) as usize).min(audio.len()));
                    let print = model.embed(&audio[a..b])?;
                    Some(Line { key: (s.start_ms, s.end_ms), print, ms: (s.end_ms - s.start_ms).min(10_000), assigned: false })
                })
                .collect();
            (k, lines)
        })
        .collect()
}

/// A voice from its lines, as Yap makes one: the longest first, up to
/// `budget_ms`.
fn voice_of(k: u8, lines: &[Line<(u64, u64)>], budget_ms: u64) -> Option<Voice<u8>> {
    let mut used = 0;
    let mut picked = Vec::new();
    for l in lines {
        if used >= budget_ms {
            break;
        }
        used += l.ms;
        picked.push((l.print.as_slice(), l.ms as f32));
    }
    Some(Voice { key: k, print: centroid(picked)?, ms: used })
}

fn find_wav(dirs: &[PathBuf], name: &str) -> Option<PathBuf> {
    dirs.iter().map(|d| d.join(format!("{name}.wav"))).find(|p| p.is_file())
}

#[test]
#[ignore]
fn real_meetings() {
    let (Some(_), Some(_), Some(audio_dirs)) = (
        std::env::var_os("YAP_DIARIZATION_MODELS"),
        std::env::var_os("YAP_DIARIZE_EXE"),
        std::env::var("YAP_DIARIZATION_AUDIO").ok(),
    ) else {
        eprintln!("set YAP_DIARIZATION_MODELS, YAP_DIARIZE_EXE and YAP_DIARIZATION_AUDIO");
        return;
    };
    let dirs: Vec<PathBuf> = audio_dirs.split(';').map(PathBuf::from).collect();
    let model = Model::open().expect("the voice-matching model in YAP_DIARIZATION_MODELS");
    let files = std::env::var("YAP_VOICES_FILES").unwrap_or_else(|_| {
        "k2_clean,k3_clean,k4_clean,k5_clean,k6_clean,k4_noisy,k6_noisy,k3_crosstalk,k4_crosstalk,kw5_mixed,k3_similar,s3_clean,s4_hard,s5_clean,s6_clean".into()
    });
    let (mut pairs_right, mut pairs_wrong, mut pairs_apart_should, mut pairs_apart_ok) = (0, 0, 0, 0);
    // The case built to merge: each half's voices as one meeting's.
    let (mut merge_right, mut merge_wrong, mut merge_possible, mut merge_mixed) = (0, 0, 0, 0);
    let (mut splits_right, mut splits_wrong) = (0, 0);
    for name in files.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let Some(wav) = find_wav(&dirs, name) else {
            eprintln!("{name}: not found");
            continue;
        };
        let audio = crate::dictation::media::decode_to_16k_mono(&wav).unwrap();
        let truth = load_rttm(&wav.with_extension("rttm"));
        let secs = audio.len() as f64 / 16_000.0;
        let n_true = {
            let mut k: Vec<&String> = truth.iter().map(|t| &t.2).collect();
            k.sort();
            k.dedup();
            k.len()
        };

        // One stream.
        let (whole, t_whole) = engine(&audio);
        let whole_turns = turns(&whole, 0.0, "w");
        let der_whole = der(&truth, &whole_turns, 0.25);
        let found = {
            let mut k: Vec<u8> = whole.iter().map(|s| s.speaker).collect();
            k.sort();
            k.dedup();
            k.len()
        };

        // A pause in the middle: two engines.
        let mid = (audio.len() / 2) / 16_000 * 16_000;
        let mid_s = mid as f64 / 16_000.0;
        let (h1, _) = engine(&audio[..mid]);
        let (h2, _) = engine(&audio[mid..]);
        let (t1, t2) = (turns(&h1, 0.0, "a"), turns(&h2, mid_s, "b"));
        let apart: Turns = t1.iter().chain(&t2).cloned().collect();
        let der_apart = der(&truth, &apart, 0.25);
        let t = Instant::now();
        let l1 = voice_lines(&model, &audio[..mid], &h1);
        let l2 = voice_lines(&model, &audio[mid..], &h2);
        let t_prints = t.elapsed().as_secs_f64();
        let old: Vec<Voice<u8>> = l1.iter().filter_map(|(k, l)| voice_of(*k, l, 45_000)).collect();
        let new: Vec<Voice<u8>> = l2.iter().filter_map(|(k, l)| voice_of(*k, l, 30_000)).collect();
        let matched = match_voices(&old, &new);
        let who = |segs: &[Segment], k: u8, off: f64| {
            let spans: Vec<(f64, f64)> =
                segs.iter().filter(|s| s.speaker == k).map(|s| (off + s.start_ms as f64 / 1000.0, off + s.end_ms as f64 / 1000.0)).collect();
            majority(&truth, &spans)
        };
        let mut notes = Vec::new();
        for o in &old {
            let (wo, _) = who(&h1, o.key, 0.0);
            for n in &new {
                let (wn, _) = who(&h2, n.key, mid_s);
                let s = similarity(&o.print, &n.print);
                let same = wo == wn;
                let m = matched.iter().any(|(nk, ok, _)| *nk == n.key && *ok == o.key);
                match (same, m) {
                    (true, true) => pairs_right += 1,
                    (false, true) => {
                        pairs_wrong += 1;
                        notes.push(format!("WRONG a{}({wo})~b{}({wn}) {s:.2}", o.key, n.key));
                    }
                    (true, false) => {
                        pairs_apart_should += 1;
                        notes.push(format!("apart a{}~b{} ({wo}) {s:.2}", o.key, n.key));
                    }
                    (false, false) => pairs_apart_ok += 1,
                }
            }
        }
        let renamed: Turns = t2
            .iter()
            .map(|(a, b, k)| {
                let n: u8 = k[1..].parse().unwrap();
                let to = matched.iter().find(|(nk, _, _)| *nk == n).map(|(_, ok, _)| format!("a{ok}")).unwrap_or(k.clone());
                (*a, *b, to)
            })
            .collect();
        let after: Turns = t1.iter().chain(&renamed).cloned().collect();
        let der_matched = der(&truth, &after, 0.25);

        // Built to merge: the two halves' voices left apart, as one meeting
        // whose engine restarted (or whose resume wasn't matched): the
        // pairs "may be one voice" offers.
        let both: Vec<Voice<(u8, u8)>> = l1
            .iter()
            .map(|(k, l)| ((1u8, *k), l))
            .chain(l2.iter().map(|(k, l)| ((2u8, *k), l)))
            .filter_map(|(key, l)| voice_of(0, l, 600_000).map(|v| Voice { key, print: v.print, ms: v.ms }))
            .collect();
        let who_half = |(half, k): (u8, u8)| if half == 1 { who(&h1, k, 0.0).0 } else { who(&h2, k, mid_s).0 };
        let share_half = |(half, k): (u8, u8)| if half == 1 { who(&h1, k, 0.0).1 } else { who(&h2, k, mid_s).1 };
        // As flags.rs offers them: a voice that may be two people is
        // offered a split, not a merge.
        let mixed_half: Vec<(u8, u8)> = l1
            .iter()
            .map(|(k, l)| ((1u8, *k), l))
            .chain(l2.iter().map(|(k, l)| ((2u8, *k), l)))
            .filter(|(_, l)| split(l).is_some())
            .map(|(key, _)| key)
            .collect();
        for a in both.iter().filter(|v| v.key.0 == 1) {
            if both.iter().any(|b| b.key.0 == 2 && who_half(b.key) == who_half(a.key)) {
                merge_possible += 1;
            }
        }
        for (a, b, s) in alike_pairs(&both).into_iter().filter(|(a, b, _)| !mixed_half.contains(a) && !mixed_half.contains(b)) {
            let (wa, wb) = (who_half(a), who_half(b));
            if wa == wb && a.0 != b.0 {
                merge_right += 1;
            } else if share_half(a) < 0.6 || share_half(b) < 0.6 {
                // One of them is two people the engine heard as one (its
                // split too small to offer): partly right, never done by
                // itself — the person listens to both first.
                merge_mixed += 1;
                notes.push(format!("merge offer with a mixed voice h{}s{}({wa} {:.0}%)~h{}s{}({wb} {:.0}%) {s:.2}", a.0, a.1, share_half(a) * 100.0, b.0, b.1, share_half(b) * 100.0));
            } else {
                merge_wrong += 1;
                notes.push(format!(
                    "MERGE OFFER WRONG h{}s{}({wa} {:.0}%)~h{}s{}({wb} {:.0}%) {s:.2}",
                    a.0,
                    a.1,
                    share_half(a) * 100.0,
                    b.0,
                    b.1,
                    share_half(b) * 100.0
                ));
            }
        }

        // The flags on the one stream's voices.
        let lw = voice_lines(&model, &audio, &whole);
        let voices: Vec<Voice<u8>> = lw.iter().filter_map(|(k, l)| voice_of(*k, l, 600_000)).collect();
        // As flags.rs offers them: a voice that may be two is split first.
        let mixed: Vec<u8> = lw.iter().filter(|(_, l)| split(l).is_some()).map(|(k, _)| *k).collect();
        let alike: Vec<(u8, u8, f32)> =
            alike_pairs(&voices).into_iter().filter(|(a, b, _)| !mixed.contains(a) && !mixed.contains(b)).collect();
        let mut split_der = der_whole;
        let mut flags = Vec::new();
        for (a, b, s) in &alike {
            let (wa, sa) = who(&whole, *a, 0.0);
            let (wb, sb) = who(&whole, *b, 0.0);
            flags.push(format!("ALIKE w{a}({wa} {:.0}%)~w{b}({wb} {:.0}%) {s:.2}", sa * 100.0, sb * 100.0));
        }
        let mut split_turns = whole_turns.clone();
        let verbose = std::env::var_os("YAP_VOICES_VERBOSE").is_some();
        if verbose {
            for (i, a) in voices.iter().enumerate() {
                for b in voices.iter().skip(i + 1) {
                    let (wa, _) = who(&whole, a.key, 0.0);
                    let (wb, _) = who(&whole, b.key, 0.0);
                    flags.push(format!("  pair w{}({wa})~w{}({wb}) {:.2}", a.key, b.key, similarity(&a.print, &b.print)));
                }
            }
            for o in &old {
                for n in &new {
                    let (wo, _) = who(&h1, o.key, 0.0);
                    let (wn, _) = who(&h2, n.key, mid_s);
                    flags.push(format!("  pause a{}({wo})~b{}({wn}) {:.2}", o.key, n.key, similarity(&o.print, &n.print)));
                }
            }
        }
        for (k, lines) in &lw {
            let (wk, share) = who(&whole, *k, 0.0);
            if verbose {
                if let Some(g) = two_groups(lines) {
                    let span = |keys: &[(u64, u64)]| keys.iter().map(|&(a, b)| (a as f64 / 1000.0, b as f64 / 1000.0)).collect::<Vec<_>>();
                    let (mk, ms) = majority(&truth, &span(&g.moved));
                    let (kk, ks) = majority(&truth, &span(&g.kept));
                    flags.push(format!(
                        "  groups w{k}({wk} {:.0}%): alike {:.2} gap {:.2} share {:.2}; moved {} ({mk} {:.0}%), kept {} ({kk} {:.0}%)",
                        share * 100.0,
                        g.alike,
                        g.gap,
                        g.share,
                        g.moved.len(),
                        ms * 100.0,
                        g.kept.len(),
                        ks * 100.0
                    ));
                }
            }
            match split(lines) {
                Some(sp) => {
                    flags.push(format!("SPLIT w{k}({wk} {:.0}%) alike {:.2} gap {:.2} moved {}", share * 100.0, sp.alike, sp.gap, sp.moved.len()));
                    if share < 0.8 {
                        splits_right += 1;
                    } else {
                        splits_wrong += 1;
                    }
                    // The person takes it: the moved lines become a new voice.
                    for t in split_turns.iter_mut() {
                        let key = (((t.0 * 1000.0).round()) as u64, ((t.1 * 1000.0).round()) as u64);
                        if t.2 == format!("w{k}") && sp.moved.iter().any(|m| m.0.abs_diff(key.0) <= 1 && m.1.abs_diff(key.1) <= 1) {
                            t.2 = format!("x{k}");
                        }
                    }
                }
                None if share < 0.8 => flags.push(format!("(no split) w{k}({wk} {:.0}%)", share * 100.0)),
                None => {}
            }
        }
        if flags.iter().any(|f| f.starts_with("SPLIT")) {
            split_der = der(&truth, &split_turns, 0.25);
        }
        eprintln!(
            "{name:<13} {secs:>5.0}s  people {n_true} found {found}  one stream {:>5.1}%  pause apart {:>5.1}% -> matched {:>5.1}%  split -> {:>5.1}%  engine {t_whole:.1}s prints {t_prints:.1}s",
            der_whole * 100.0,
            der_apart * 100.0,
            der_matched * 100.0,
            split_der * 100.0
        );
        for n in notes.iter().chain(&flags) {
            eprintln!("    {n}");
        }
    }
    eprintln!("pause pairs: matched right {pairs_right}, matched WRONG {pairs_wrong}, left apart but one voice {pairs_apart_should}, apart and different {pairs_apart_ok}");
    eprintln!(
        "built to merge (halves left apart): offered right {merge_right} of {merge_possible}, with a voice holding two people {merge_mixed}, offered WRONG {merge_wrong}"
    );
    eprintln!("splits offered: on a voice holding two people {splits_right}, on one person {splits_wrong}");
    assert_eq!(pairs_wrong, 0, "a voice matched to someone else");
    assert_eq!(merge_wrong, 0, "two clear people offered as one");
}

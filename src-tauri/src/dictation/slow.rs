//! **"Taking longer than usual…"** (Wispr Flow's, by behaviour): when a
//! dictation's transcription or its AI cleanup runs well past what's usual
//! on this PC, the bar's capsule says so quietly — and, when it's the
//! cleanup (the on-device model or a cloud one, whichever is set up),
//! offers **Skip cleanup**: the words as transcribed, pasted now. The watch
//! itself is the pipeline's (`pipeline/slow.rs`); what's usual, and when
//! that's well past, is here — pure, unit-tested:
//!
//! - **Transcription** ([`usual_stt_ms`]): a straight line (a fixed cost
//!   plus a cost per second of audio) through the speech engine's measured
//!   timings on this PC for the model and device in use (`stt_speed.rs`:
//!   its warm runs at ~3, ~11 and ~36 s of speech), scaled by how this
//!   session's own dictations compared with it (their median ratio,
//!   0.5–3×); not measured yet, the line through this session's own.
//! - **Cleanup** ([`usual_cleanup_ms`]): the line through this session's
//!   cleanups by the same model ([`cleanup_key`]: on this PC or in the
//!   cloud, and which), by the transcript's length.
//! - **Well past** ([`slow_after_ms`]): 2.5× the usual plus a second, and
//!   never before 4 s (Wispr's). Nothing known yet: 4 s plus half the
//!   audio's length for a transcription, plus 6 ms a character for a
//!   cleanup.
//!
//! The recent timings are this session's last 20 of each, in memory (a
//! restart starts again from the benchmark, which is kept). The log says
//! times and lengths, never the words.

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::Serialize;

use crate::dictation::stt::Device;

/// Never "longer than usual" before this.
pub const MIN_SLOW_MS: u64 = 4_000;
/// "Well past": this many times the usual…
pub const FACTOR: f64 = 2.5;
/// …plus this.
pub const SLACK_MS: u64 = 1_000;
/// Recent timings kept, of each kind.
pub const KEEP: usize = 20;
/// A transcription with nothing known yet: the floor plus this share of
/// the audio's length.
pub const UNKNOWN_STT_SHARE: f64 = 0.5;
/// A cleanup with nothing known yet: the floor plus this much a character.
pub const UNKNOWN_CLEANUP_MS_PER_CHAR: u64 = 6;
/// The recent dictations may scale the benchmark's line this much at most.
pub const SCALE_RANGE: (f64, f64) = (0.5, 3.0);

/// The bar's notice: `{stage, skip}` while it's slow, `null` once it isn't.
pub const EVENT: &str = "yap-slow";

/// Which part of a dictation is slow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    Transcribing,
    Cleanup,
}

/// What the bar gets ([`EVENT`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    pub stage: Stage,
    /// Skip cleanup is on offer.
    pub skip: bool,
}

/// A line `base + per · x` through `points` (x, ms) by least squares,
/// never sloping down or starting below zero. One length only (or one
/// point): flat at their median. `None` with no points.
pub fn fit(points: &[(u64, u64)]) -> Option<(f64, f64)> {
    if points.is_empty() {
        return None;
    }
    let n = points.len() as f64;
    let mean_x = points.iter().map(|p| p.0 as f64).sum::<f64>() / n;
    let mean_y = points.iter().map(|p| p.1 as f64).sum::<f64>() / n;
    let sxx: f64 = points.iter().map(|p| (p.0 as f64 - mean_x).powi(2)).sum();
    if sxx < 1.0 {
        let mut ys: Vec<u64> = points.iter().map(|p| p.1).collect();
        ys.sort_unstable();
        return Some((ys[ys.len() / 2] as f64, 0.0));
    }
    let sxy: f64 = points.iter().map(|p| (p.0 as f64 - mean_x) * (p.1 as f64 - mean_y)).sum();
    let per = sxy / sxx;
    if per <= 0.0 {
        return Some((mean_y, 0.0));
    }
    let base = mean_y - per * mean_x;
    if base < 0.0 {
        // Through the origin instead.
        let sxx0: f64 = points.iter().map(|p| (p.0 as f64).powi(2)).sum();
        let sxy0: f64 = points.iter().map(|p| p.0 as f64 * p.1 as f64).sum();
        return Some((0.0, (sxy0 / sxx0.max(1.0)).max(0.0)));
    }
    Some((base, per))
}

fn at((base, per): (f64, f64), x: u64) -> f64 {
    base + per * x as f64
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    Some(v[v.len() / 2])
}

/// The usual time (ms) to transcribe `audio_ms` of audio, from the
/// benchmark's warm timings (`bench`: (clip ms, ms)) and this session's
/// dictations (`recent`: (audio ms, ms)) with the same model and device.
/// `None`: nothing known.
pub fn usual_stt_ms(audio_ms: u64, bench: &[(u64, u64)], recent: &[(u64, u64)]) -> Option<u64> {
    let Some(line) = fit(bench) else {
        return fit(recent).map(|l| at(l, audio_ms).round() as u64);
    };
    // How this session's dictations compared with the benchmark's line.
    let ratios: Vec<f64> = recent
        .iter()
        .filter_map(|&(x, ms)| {
            let expected = at(line, x);
            (expected >= 1.0).then(|| ms as f64 / expected)
        })
        .collect();
    let scale = median(ratios).unwrap_or(1.0).clamp(SCALE_RANGE.0, SCALE_RANGE.1);
    Some((at(line, audio_ms) * scale).round() as u64)
}

/// The usual time (ms) of a cleanup of `chars` characters, from this
/// session's cleanups by the same model (`recent`: (chars, ms)).
pub fn usual_cleanup_ms(chars: u64, recent: &[(u64, u64)]) -> Option<u64> {
    fit(recent).map(|l| at(l, chars).round() as u64)
}

/// When a stage whose usual time is `usual` counts as taking longer than
/// usual (ms from its start): [`FACTOR`] × the usual + [`SLACK_MS`], never
/// before `floor`; `unknown` when nothing is known.
pub fn slow_after_ms(usual: Option<u64>, unknown: u64, floor: u64) -> u64 {
    match usual {
        Some(u) => ((u as f64 * FACTOR) as u64 + SLACK_MS).max(floor),
        None => unknown.max(floor),
    }
}

/// A transcription with nothing known: the floor plus half the audio.
pub fn unknown_stt_ms(audio_ms: u64, floor: u64) -> u64 {
    floor + (audio_ms as f64 * UNKNOWN_STT_SHARE) as u64
}

/// A cleanup with nothing known: the floor plus 6 ms a character.
pub fn unknown_cleanup_ms(chars: u64, floor: u64) -> u64 {
    floor + chars * UNKNOWN_CLEANUP_MS_PER_CHAR
}

/// Which model cleaned up, for its timings: on this PC (Yap's own model, a
/// self-hosted server on a loopback address) or in the cloud, and which.
pub fn cleanup_key(provider: &str, base_url: &str, model: &str) -> String {
    let here = crate::ai::ai_model::runs_here(provider, base_url);
    format!("{}|{provider}|{model}", if here { "here" } else { "cloud" })
}

/// A transcription's model and device, for its timings.
pub fn stt_key(model: &str, device: Device) -> String {
    format!("{model}|{device:?}")
}

// ---- this session's timings ----------------------------------------------------------------

#[derive(Default)]
struct Recent {
    /// (key, audio ms, ms)
    stt: VecDeque<(String, u64, u64)>,
    /// (key, chars, ms)
    cleanup: VecDeque<(String, u64, u64)>,
}

static RECENT: Mutex<Option<Recent>> = Mutex::new(None);

fn with_recent<T>(f: impl FnOnce(&mut Recent) -> T) -> T {
    let mut g = RECENT.lock().unwrap_or_else(|p| p.into_inner());
    f(g.get_or_insert_with(Recent::default))
}

fn push(list: &mut VecDeque<(String, u64, u64)>, key: &str, x: u64, ms: u64) {
    list.push_back((key.to_string(), x, ms));
    while list.len() > KEEP {
        list.pop_front();
    }
}

fn points(list: &VecDeque<(String, u64, u64)>, key: &str) -> Vec<(u64, u64)> {
    list.iter().filter(|(k, ..)| k == key).map(|&(_, x, ms)| (x, ms)).collect()
}

/// A transcription finished: `audio_ms` of audio took `ms`.
pub fn record_stt(key: &str, audio_ms: u64, ms: u64) {
    with_recent(|r| push(&mut r.stt, key, audio_ms, ms));
}

/// A cleanup finished: `chars` characters took `ms`.
pub fn record_cleanup(key: &str, chars: u64, ms: u64) {
    with_recent(|r| push(&mut r.cleanup, key, chars, ms));
}

/// When a transcription of `audio_ms` by `model` on `device` counts as
/// slow (ms), and its usual time if known.
pub fn stt_slow_after(model: &str, device: Device, audio_ms: u64) -> (u64, Option<u64>) {
    let bench = crate::dictation::stt_speed::warm_points(model, device);
    let recent = with_recent(|r| points(&r.stt, &stt_key(model, device)));
    let usual = usual_stt_ms(audio_ms, &bench, &recent);
    let floor = floor_ms();
    (slow_after_ms(usual, unknown_stt_ms(audio_ms, floor), floor), usual)
}

/// When a cleanup of `chars` by `key`'s model counts as slow (ms), and its
/// usual time if known.
pub fn cleanup_slow_after(key: &str, chars: u64) -> (u64, Option<u64>) {
    let recent = with_recent(|r| points(&r.cleanup, key));
    let usual = usual_cleanup_ms(chars, &recent);
    let floor = floor_ms();
    (slow_after_ms(usual, unknown_cleanup_ms(chars, floor), floor), usual)
}

// ---- test runs ------------------------------------------------------------------------------

/// A test run's floor instead of [`MIN_SLOW_MS`] (0: none), and how long
/// its stub transcription takes (`e2e_dictation_timing`).
#[cfg(debug_assertions)]
static TEST_FLOOR_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
#[cfg(debug_assertions)]
static TEST_STT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The floor in force ([`MIN_SLOW_MS`], or a test run's).
pub fn floor_ms() -> u64 {
    #[cfg(debug_assertions)]
    {
        let test = TEST_FLOOR_MS.load(std::sync::atomic::Ordering::SeqCst);
        if test > 0 && crate::shell::e2e::active() {
            return test;
        }
    }
    MIN_SLOW_MS
}

/// Test mode only: the floor (`floor_ms`, 0 = the real one), the stub's
/// transcription time (0 = at once), and forget this session's timings.
#[cfg(debug_assertions)]
pub fn set_test(floor_ms: u64, stt_ms: u64, forget: bool) {
    use std::sync::atomic::Ordering;
    TEST_FLOOR_MS.store(floor_ms, Ordering::SeqCst);
    TEST_STT_MS.store(stt_ms, Ordering::SeqCst);
    if forget {
        with_recent(|r| *r = Recent::default());
    }
}

/// How long a test run's transcription waits first (`None`: not at all).
#[cfg(debug_assertions)]
pub fn test_stt_delay() -> Option<std::time::Duration> {
    let ms = TEST_STT_MS.load(std::sync::atomic::Ordering::SeqCst);
    (ms > 0 && crate::shell::e2e::active()).then(|| std::time::Duration::from_millis(ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parakeet on a graphics card, as `stt_speed` measured it: ~3, ~11 and
    /// ~36 s of speech.
    const BENCH: [(u64, u64); 3] = [(3_000, 130), (11_000, 160), (36_000, 580)];

    #[test]
    fn a_line_through_the_timings() {
        assert_eq!(fit(&[]), None);
        // One point, or one length: flat at the median.
        assert_eq!(fit(&[(5_000, 900)]), Some((900.0, 0.0)));
        assert_eq!(fit(&[(5_000, 900), (5_000, 1_100), (5_000, 1_000)]), Some((1_000.0, 0.0)));
        // A real line.
        let (base, per) = fit(&[(0, 100), (10_000, 1_100)]).unwrap();
        assert!((base - 100.0).abs() < 1e-6 && (per - 0.1).abs() < 1e-9);
        // Never sloping down: flat at the mean.
        assert_eq!(fit(&[(1_000, 900), (9_000, 300)]), Some((600.0, 0.0)));
        // Never below zero at the start: through the origin.
        let (base, per) = fit(&[(1_000, 10), (2_000, 200), (3_000, 400)]).unwrap();
        assert_eq!(base, 0.0);
        assert!(per > 0.0);
    }

    #[test]
    fn the_usual_transcription_follows_the_benchmark_and_this_pcs_dictations() {
        // Nothing known.
        assert_eq!(usual_stt_ms(10_000, &[], &[]), None);
        // The benchmark alone: ~160 ms for 11 s, more for longer.
        let typical = usual_stt_ms(11_000, &BENCH, &[]).unwrap();
        assert!((100..300).contains(&typical), "{typical}");
        let long = usual_stt_ms(120_000, &BENCH, &[]).unwrap();
        assert!(long > 1_000, "{long}");
        // This session's dictations took twice the benchmark's time (a busy
        // PC): the usual doubles.
        let busy: Vec<(u64, u64)> = [3_000u64, 11_000, 20_000].iter().map(|&x| (x, (at(fit(&BENCH).unwrap(), x) * 2.0) as u64)).collect();
        let doubled = usual_stt_ms(11_000, &BENCH, &busy).unwrap();
        assert!((doubled as f64 / typical as f64 - 2.0).abs() < 0.05, "{doubled} vs {typical}");
        // One odd dictation (the first after a load) doesn't move it much:
        // the median.
        let mut odd = busy.clone();
        odd.push((11_000, 20_000));
        assert!((usual_stt_ms(11_000, &BENCH, &odd).unwrap() as f64 / doubled as f64 - 1.0).abs() < 0.3);
        // …and however odd, never more than 3×.
        let slow: Vec<(u64, u64)> = vec![(11_000, 60_000); 5];
        assert!(usual_stt_ms(11_000, &BENCH, &slow).unwrap() as f64 <= typical as f64 * 3.0 * 1.01);
        // No benchmark: the line through the dictations.
        assert_eq!(usual_stt_ms(10_000, &[], &[(5_000, 500), (15_000, 1_500)]), Some(1_000));
    }

    #[test]
    fn well_past_usual_and_never_before_four_seconds() {
        // A quick transcription: the four-second floor.
        assert_eq!(slow_after_ms(Some(160), 9_999, MIN_SLOW_MS), MIN_SLOW_MS);
        // A long one: 2.5× + 1 s.
        assert_eq!(slow_after_ms(Some(4_000), 9_999, MIN_SLOW_MS), 11_000);
        // Nothing known: 4 s + half the audio for a transcription…
        assert_eq!(slow_after_ms(None, unknown_stt_ms(10_000, MIN_SLOW_MS), MIN_SLOW_MS), 9_000);
        // …and 4 s + 6 ms a character for a cleanup.
        assert_eq!(slow_after_ms(None, unknown_cleanup_ms(200, MIN_SLOW_MS), MIN_SLOW_MS), 5_200);
        // Cloud cleanups that take ~1.2 s: slow after 4 s; a local model's
        // ~3 s for a long dictation: after 8.5 s.
        let cloud = [(80, 1_100), (200, 1_200), (400, 1_300)];
        assert_eq!(cleanup_after(&cloud, 200), MIN_SLOW_MS);
        let local = [(100, 1_500), (400, 2_400), (800, 3_000)];
        let after = cleanup_after(&local, 800);
        assert!((8_000..9_500).contains(&after), "{after}");
    }

    fn cleanup_after(recent: &[(u64, u64)], chars: u64) -> u64 {
        slow_after_ms(usual_cleanup_ms(chars, recent), unknown_cleanup_ms(chars, MIN_SLOW_MS), MIN_SLOW_MS)
    }

    #[test]
    fn timings_are_kept_per_model_twenty_at_most() {
        set_test(0, 0, true);
        for i in 0..30 {
            record_cleanup("cloud|groq|a", 100, 1_000 + i);
        }
        record_cleanup("here|ondevice|b", 100, 9_000);
        let a = with_recent(|r| points(&r.cleanup, "cloud|groq|a"));
        assert_eq!(a.len(), KEEP - 1, "the other model's took a place");
        assert_eq!(a.last(), Some(&(100, 1_029)));
        assert_eq!(with_recent(|r| points(&r.cleanup, "here|ondevice|b")), vec![(100, 9_000)]);
        // Unknown model: the unknown time.
        let (after, usual) = cleanup_slow_after("cloud|groq|new", 100);
        assert_eq!(usual, None);
        assert_eq!(after, MIN_SLOW_MS + 600);
        set_test(0, 0, true);
    }

    #[test]
    fn keys_tell_this_pc_from_the_cloud() {
        assert!(cleanup_key("ondevice", "", "qwen").starts_with("here|"));
        assert!(cleanup_key("custom", "http://127.0.0.1:8080/v1", "x").starts_with("here|"));
        assert!(cleanup_key("groq", "https://api.groq.com/openai/v1", "y").starts_with("cloud|"));
    }
}

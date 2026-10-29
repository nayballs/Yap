//! **The mic quiz** (2026-10-29; Wispr Flow's, by behaviour): which of your
//! microphones hears you best? You read a line aloud once, and Yap listens
//! on every connected microphone **together** for [`LISTEN_MS`] — shared
//! mode, so each opens beside whatever else has it (the dictation's own
//! mic, a call), and nothing is ever kept: the audio is measured and let
//! go. In the first-run setup's microphone step and in Settings → Dictation
//! → Microphone (the Help menu's Microphone help opens it there).
//!
//! - **Each mic's sound** (`mic_tips::measure`, the same measure as the
//!   "didn't hear you clearly" tips): the voice (the loud frames) and the
//!   room (the quiet ones) in dBFS, and how much hit full scale.
//! - **Its verdict** ([`judge`], pure): clear, quiet, noisy, too loud or
//!   silent, and a score — how far the voice stands above the room, less
//!   what's wrong. A **Bluetooth headset** in its call profile (8 or 16 kHz,
//!   `mic_tips::hands_free`) gets its honest note: it sounds worse while
//!   its mic is on, and scores less for it.
//! - **The best** ([`best`], pure): the highest score that heard you; on a
//!   tie, the one earlier in your order. Picking one (the page) puts it
//!   first in the microphones' order (`mics::offer::put_first`).
//!
//! Live levels go to the page as it listens (`yap-mic-quiz`: each mic's
//! level on the bar's scale, `meter::Floor`). One quiz at a time; Cancel
//! stops it. The log says counts and verdicts, never names at `info`.
//! Software microphones (a streaming app's) are listened to only when
//! they're in your order. Test mode's made-up microphones
//! (`mics::fake`) sound as a spec says (`e2e_mic_quiz`).

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::dictation::mic_tips::{self, Signal};
use crate::dictation::mics::Endpoint;

/// How long Yap listens: long enough to read the line.
pub const LISTEN_MS: u64 = 6_000;
/// The live levels' event.
pub const EVENT: &str = "yap-mic-quiz";
/// At most this many microphones at once.
const MAX_MICS: usize = 6;
/// No voice at all under this (dBFS): it didn't hear you.
pub const SILENT_DB: f32 = -62.0;
/// A score's ceiling: the voice this far over the room is as good as it gets.
const BEST_GAP_DB: f32 = 45.0;
const TICK: Duration = Duration::from_millis(100);
const RATE: u32 = crate::dictation::pipeline::TARGET_SAMPLE_RATE;

/// What a microphone made of the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Quality {
    Clear,
    Quiet,
    Noisy,
    /// So loud it distorts.
    Loud,
    /// It heard nothing (muted, out of reach).
    Silent,
    /// It wouldn't open.
    Failed,
}

/// A microphone's verdict: its quality and score (higher is better; only
/// one that heard you can be the best). Pure.
pub fn judge(signal: Option<&Signal>, hands_free: bool) -> (Quality, f32) {
    let Some(s) = signal else { return (Quality::Silent, 0.0) };
    if s.voice_db < SILENT_DB {
        return (Quality::Silent, 0.0);
    }
    let gap = (s.voice_db - s.floor_db).clamp(0.0, BEST_GAP_DB);
    let (quality, mut score) = if s.clipped >= mic_tips::CLIPPED_SHARE {
        (Quality::Loud, gap - 25.0)
    } else if s.voice_db < mic_tips::QUIET_DB {
        (Quality::Quiet, gap - 15.0 - (mic_tips::QUIET_DB - s.voice_db))
    } else if s.floor_db > mic_tips::NOISY_FLOOR_DB && s.voice_db - s.floor_db < mic_tips::NOISY_GAP_DB {
        (Quality::Noisy, gap - 10.0)
    } else {
        (Quality::Clear, gap)
    };
    if hands_free {
        // A headset's call profile: 8 or 16 kHz, narrow and muffled.
        score -= 15.0;
    }
    (quality, score)
}

/// Which of `verdicts` (in the person's order) is the best: the highest
/// score of those that heard them; the earlier on a tie. Pure.
pub fn best(verdicts: &[(Quality, f32)]) -> Option<usize> {
    let mut pick: Option<usize> = None;
    for (i, (q, score)) in verdicts.iter().enumerate() {
        if matches!(q, Quality::Silent | Quality::Failed) {
            continue;
        }
        if pick.is_none_or(|p| *score > verdicts[p].1) {
            pick = Some(i);
        }
    }
    pick
}

/// A verdict in a sentence, with the Bluetooth note. Pure.
pub fn note(quality: Quality, hands_free: bool) -> String {
    let mut out = match quality {
        Quality::Clear => "Hears you clearly.",
        Quality::Quiet => "Hears you only faintly: move closer, or turn its level up in Windows.",
        Quality::Noisy => "Hears a lot of noise around your voice.",
        Quality::Loud => "So loud your words distort: turn its level down in Windows.",
        Quality::Silent => "Didn't hear you. Is it muted, or out of reach?",
        Quality::Failed => "Couldn't listen to it.",
    }
    .to_string();
    if hands_free && !matches!(quality, Quality::Failed) {
        out.push_str(" Bluetooth headsets sound worse while their mic is on: Windows switches them to call quality.");
    }
    out
}

/// One microphone's result.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Heard {
    pub id: String,
    pub name: String,
    /// "Contoso H2" for "Headset Microphone (Contoso H2)".
    pub short: String,
    pub bluetooth: bool,
    pub built_in: bool,
    /// A Bluetooth headset in its call profile.
    pub hands_free: bool,
    pub quality: Quality,
    pub score: f32,
    /// The voice on the bar's scale (0..0.92), for its meter.
    pub level: f32,
    pub voice_db: Option<f32>,
    pub floor_db: Option<f32>,
    pub note: String,
}

/// The quiz's answer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Quiz {
    /// In the order Yap tries them.
    pub mics: Vec<Heard>,
    /// The best one's id.
    pub best: Option<String>,
    /// Cancelled before the end: no verdicts.
    pub cancelled: bool,
}

/// The microphones to listen to: connected, in the order Yap tries them
/// (the person's, then Windows' default, then the rest); a software one
/// only when it's in their order. Pure.
pub fn to_listen(present: &[Endpoint], order: &[crate::dictation::mics::MicChoice]) -> Vec<Endpoint> {
    let mut list: Vec<Endpoint> = present
        .iter()
        .filter(|e| !e.is_virtual || crate::dictation::mics::rank_of(order, e, present).is_some())
        .cloned()
        .collect();
    list.sort_by_key(|e| (crate::dictation::mics::rank_of(order, e, present).unwrap_or(usize::MAX), !e.default));
    list.truncate(MAX_MICS);
    list
}

// ---- listening ----------------------------------------------------------------------------------

static RUNNING: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);

/// One microphone being listened to.
struct Ear {
    ep: Endpoint,
    samples: Arc<Mutex<Vec<f32>>>,
    /// The loudest sample since the last tick (f32 bits).
    peak: Arc<AtomicU32>,
    floor: crate::dictation::meter::Floor,
    rate: u32,
    failed: bool,
    #[cfg(windows)]
    _stream: Option<cpal::Stream>,
    #[cfg(debug_assertions)]
    fake: Option<(f32, f32)>,
}

impl Ear {
    fn new(ep: Endpoint) -> Ear {
        Ear {
            ep,
            samples: Arc::new(Mutex::new(Vec::new())),
            peak: Arc::new(AtomicU32::new(0)),
            floor: crate::dictation::meter::Floor::default(),
            rate: 0,
            failed: false,
            #[cfg(windows)]
            _stream: None,
            #[cfg(debug_assertions)]
            fake: None,
        }
    }

    /// The level since the last tick, on the bar's scale.
    fn level(&mut self) -> f32 {
        let peak = f32::from_bits(self.peak.swap(0, Ordering::Relaxed));
        self.floor.level(peak)
    }
}

/// Open `ep` for listening (shared mode, downmixed to 16 kHz mono).
#[cfg(windows)]
fn open(ear: &mut Ear) {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let host = cpal::default_host();
    let Some(device) = host.input_devices().ok().and_then(|mut d| d.find(|d| d.name().is_ok_and(|n| n == ear.ep.name))) else {
        ear.failed = true;
        return;
    };
    let Ok(cfg) = device.default_input_config() else {
        ear.failed = true;
        return;
    };
    let (rate, channels) = (cfg.sample_rate().0, cfg.channels() as usize);
    ear.rate = rate;
    let (samples, peak) = (Arc::clone(&ear.samples), Arc::clone(&ear.peak));
    let stream = device.build_input_stream(
        &cpal::StreamConfig { channels: channels as u16, sample_rate: cpal::SampleRate(rate), buffer_size: cpal::BufferSize::Default },
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            let mono: Vec<f32> = if channels > 1 {
                data.chunks_exact(channels).map(|f| f.iter().sum::<f32>() / channels as f32).collect()
            } else {
                data.to_vec()
            };
            let loudest = mono.iter().fold(0f32, |m, s| m.max(s.abs()));
            peak.fetch_max(loudest.to_bits(), Ordering::Relaxed);
            let mono = if rate == RATE { mono } else { crate::dictation::pipeline::resample_linear(&mono, rate, RATE) };
            if let Ok(mut s) = samples.lock() {
                s.extend_from_slice(&mono);
            }
        },
        |e| tracing::warn!("Mic quiz: a stream stopped ({e})"),
        None,
    );
    match stream.map_err(|e| e.to_string()).and_then(|s| s.play().map(|_| s).map_err(|e| e.to_string())) {
        Ok(s) => ear._stream = Some(s),
        Err(e) => {
            tracing::warn!("Mic quiz: couldn't open a microphone ({e})");
            ear.failed = true;
        }
    }
}

#[cfg(not(windows))]
fn open(ear: &mut Ear) {
    ear.failed = true;
}

/// A made-up voice: `voice` (peak) in syllables (240 ms on, 160 ms off)
/// over a room at `room` (a steady hiss), `n` samples from sample `at`.
#[cfg(any(test, debug_assertions))]
pub(crate) fn synth(at: usize, n: usize, voice: f32, room: f32) -> Vec<f32> {
    let mut seed = (at as u32).wrapping_mul(2_654_435_761).wrapping_add(12_345);
    (at..at + n)
        .map(|i| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let hiss = ((seed >> 9) as f32 / (1u32 << 23) as f32 - 0.5) * 2.0 * room;
            let on = i % 6_400 < 3_840;
            let tone = if on { voice * (i as f32 * 2.0 * std::f32::consts::PI * 220.0 / 16_000.0).sin() } else { 0.0 };
            (tone + hiss).clamp(-1.0, 1.0)
        })
        .collect()
}

/// How a test run's made-up microphones sound (`e2e_mic_quiz`): id →
/// (voice, room) peaks.
#[cfg(debug_assertions)]
static PROFILES: Mutex<Option<std::collections::HashMap<String, (f32, f32)>>> = Mutex::new(None);

/// Listen on `eps` together for `ms` (live levels to the page), then judge
/// each. Blocking: a thread of its own.
fn listen(app: &AppHandle, eps: Vec<Endpoint>, ms: u64) -> Quiz {
    let mut ears: Vec<Ear> = eps.into_iter().map(Ear::new).collect();
    for ear in ears.iter_mut() {
        #[cfg(debug_assertions)]
        if crate::dictation::mics::fake::is_fake(&ear.ep.id) {
            let p = PROFILES.lock().ok().and_then(|p| p.as_ref().and_then(|m| m.get(&ear.ep.id).copied()));
            ear.fake = Some(p.unwrap_or((0.25, 0.001)));
            ear.rate = if ear.ep.bluetooth { 16_000 } else { 48_000 };
            continue;
        }
        open(ear);
    }
    tracing::info!(mics = ears.len(), failed = ears.iter().filter(|e| e.failed).count(), "Mic quiz: listening");
    let started = Instant::now();
    #[cfg(debug_assertions)]
    let mut fed = 0usize;
    loop {
        std::thread::sleep(TICK);
        let elapsed = started.elapsed().as_millis() as u64;
        #[cfg(debug_assertions)]
        {
            let due = (elapsed.min(ms) * RATE as u64 / 1_000) as usize;
            for ear in ears.iter_mut() {
                if let Some((voice, room)) = ear.fake {
                    let chunk = synth(fed, due.saturating_sub(fed), voice, room);
                    let loudest = chunk.iter().fold(0f32, |m, s| m.max(s.abs()));
                    ear.peak.fetch_max(loudest.to_bits(), Ordering::Relaxed);
                    if let Ok(mut s) = ear.samples.lock() {
                        s.extend_from_slice(&chunk);
                    }
                }
            }
            fed = due;
        }
        let levels: serde_json::Map<String, serde_json::Value> =
            ears.iter_mut().map(|e| (e.ep.id.clone(), serde_json::json!(if e.failed { 0.0 } else { e.level() }))).collect();
        let _ = app.emit(EVENT, serde_json::json!({ "elapsedMs": elapsed.min(ms), "totalMs": ms, "levels": levels }));
        if CANCEL.load(Ordering::SeqCst) || elapsed >= ms {
            break;
        }
    }
    let cancelled = CANCEL.load(Ordering::SeqCst);
    // The streams go first: nothing is listened to while it's judged.
    let taken: Vec<(Endpoint, Vec<f32>, u32, bool)> = ears
        .into_iter()
        .map(|e| {
            let samples = e.samples.lock().map(|mut s| std::mem::take(&mut *s)).unwrap_or_default();
            (e.ep, samples, e.rate, e.failed)
        })
        .collect();
    if cancelled {
        tracing::info!("Mic quiz: cancelled");
        return Quiz { mics: Vec::new(), best: None, cancelled: true };
    }
    let mut mics: Vec<Heard> = taken
        .into_iter()
        .map(|(ep, samples, rate, failed)| {
            let signal = (!failed).then(|| mic_tips::measure(&samples)).flatten();
            let hands_free = mic_tips::hands_free(&mic_tips::MicInfo { name: ep.name.clone(), endpoint: Some(ep.id.clone()), bluetooth: ep.bluetooth, rate });
            let (quality, score) = if failed { (Quality::Failed, 0.0) } else { judge(signal.as_ref(), hands_free) };
            let level = signal.map(|s| crate::dictation::meter::level(s.voice_db, crate::dictation::meter::FLOOR_MIN_DB)).unwrap_or(0.0);
            Heard {
                short: crate::dictation::mics::short_name(&ep.name),
                id: ep.id,
                name: ep.name,
                bluetooth: ep.bluetooth,
                built_in: ep.built_in,
                hands_free,
                quality,
                score,
                level,
                voice_db: signal.map(|s| s.voice_db.round()),
                floor_db: signal.map(|s| s.floor_db.round()),
                note: note(quality, hands_free),
            }
        })
        .collect();
    let pick = best(&mics.iter().map(|m| (m.quality, m.score)).collect::<Vec<_>>());
    let best = pick.map(|i| mics[i].id.clone());
    for m in mics.iter_mut() {
        m.score = m.score.round();
    }
    tracing::info!(
        mics = mics.len(),
        clear = mics.iter().filter(|m| m.quality == Quality::Clear).count(),
        silent = mics.iter().filter(|m| m.quality == Quality::Silent).count(),
        best = pick.is_some(),
        "Mic quiz: done"
    );
    Quiz { mics, best, cancelled: false }
}

/// Settings and the first-run setup: listen on every connected microphone
/// at once while the person reads the line (`ms`, default [`LISTEN_MS`]),
/// then say how each heard them and which is best.
#[tauri::command]
pub async fn mic_quiz_run(app: AppHandle, ms: Option<u64>) -> Result<Quiz, String> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err("The mic quiz is already listening".to_string());
    }
    CANCEL.store(false, Ordering::SeqCst);
    let ms = ms.unwrap_or(LISTEN_MS).clamp(1_000, 15_000);
    let order = crate::config::load().mic_order;
    let out = tauri::async_runtime::spawn_blocking(move || {
        let present = crate::dictation::mics::refresh().or_else(crate::dictation::mics::present).unwrap_or_default();
        let eps = to_listen(&present, &order);
        if eps.is_empty() {
            return Err("No microphone found. Plug one in or connect a headset.".to_string());
        }
        Ok(listen(&app, eps, ms))
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    RUNNING.store(false, Ordering::SeqCst);
    out
}

/// Stop the quiz listening (no verdicts).
#[tauri::command]
pub fn mic_quiz_cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// Test mode only: how each made-up microphone sounds to the quiz (`id` →
/// `[voice, room]` peaks; one not named: a clear voice).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_mic_quiz(profiles: std::collections::HashMap<String, [f32; 2]>) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    let map = profiles
        .into_iter()
        .map(|(id, [voice, room])| (if id.starts_with("e2e-") { id } else { format!("e2e-{id}") }, (voice, room)))
        .collect();
    *PROFILES.lock().unwrap_or_else(|p| p.into_inner()) = Some(map);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heard(voice: f32, room: f32) -> Option<Signal> {
        mic_tips::measure(&synth(0, 16_000 * 4, voice, room))
    }

    #[test]
    fn a_clear_mic_beats_a_quiet_a_noisy_and_a_distorting_one() {
        let clear = judge(heard(0.25, 0.001).as_ref(), false);
        let quiet = judge(heard(0.004, 0.0002).as_ref(), false);
        let noisy = judge(heard(0.05, 0.06).as_ref(), false);
        let loud = judge(heard(1.4, 0.001).as_ref(), false);
        assert_eq!(clear.0, Quality::Clear);
        assert_eq!(quiet.0, Quality::Quiet);
        assert_eq!(noisy.0, Quality::Noisy);
        assert_eq!(loud.0, Quality::Loud);
        assert!(clear.1 > quiet.1 && clear.1 > noisy.1 && clear.1 > loud.1, "{clear:?} {quiet:?} {noisy:?} {loud:?}");
        assert_eq!(best(&[quiet, noisy, clear, loud]), Some(2));
    }

    #[test]
    fn a_silent_mic_is_never_the_best_and_nothing_heard_means_none() {
        let silent = judge(Some(&Signal { secs: 4.0, voice_db: -90.0, floor_db: -95.0, clipped: 0.0 }), false);
        assert_eq!(silent.0, Quality::Silent);
        assert_eq!(judge(None, false).0, Quality::Silent);
        let quiet = judge(heard(0.004, 0.0002).as_ref(), false);
        assert_eq!(best(&[silent, quiet]), Some(1));
        assert_eq!(best(&[silent, (Quality::Failed, 99.0)]), None);
        assert_eq!(best(&[]), None);
    }

    #[test]
    fn a_bluetooth_headset_in_its_call_profile_loses_to_an_equal_wired_mic_and_says_why() {
        let wired = judge(heard(0.2, 0.002).as_ref(), false);
        let headset = judge(heard(0.2, 0.002).as_ref(), true);
        assert_eq!(headset.0, Quality::Clear);
        assert!(headset.1 < wired.1);
        assert_eq!(best(&[headset, wired]), Some(1));
        assert!(note(Quality::Clear, true).contains("Bluetooth headsets sound worse while their mic is on"));
        assert!(!note(Quality::Clear, false).contains("Bluetooth"));
    }

    #[test]
    fn a_tie_goes_to_the_one_earlier_in_the_order() {
        let a = judge(heard(0.25, 0.001).as_ref(), false);
        assert_eq!(best(&[a, a, a]), Some(0));
    }

    #[test]
    fn it_listens_to_connected_mics_in_the_order_yap_tries_them() {
        use crate::dictation::mics::MicChoice;
        let ep = |id: &str, default, is_virtual| Endpoint { id: id.into(), name: format!("Microphone ({id})"), default, is_virtual, ..Default::default() };
        let present = vec![ep("array", true, false), ep("usb", false, false), ep("stream", false, true), ep("bt", false, false)];
        let order = vec![MicChoice { id: "usb".into(), name: "Microphone (usb)".into() }];
        let ids: Vec<String> = to_listen(&present, &order).into_iter().map(|e| e.id).collect();
        // Yours first, then Windows' default, then the rest; no software mic.
        assert_eq!(ids, ["usb", "array", "bt"]);
        // A software mic in your order is listened to.
        let order = vec![MicChoice { id: "stream".into(), name: "Microphone (stream)".into() }];
        assert_eq!(to_listen(&present, &order)[0].id, "stream");
    }
}

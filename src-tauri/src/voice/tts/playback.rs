//! Where the audio goes: rodio on the output device, or a simulated
//! clock in test mode; a read-aloud's sentences tagged with their place,
//! and the voice's volume.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::Pcm;

/// Where the audio goes: rodio's player, or a simulated clock in test mode.
pub trait Playback: Send + Sync {
    fn append(&self, pcm: Pcm);
    /// Append audio of a read-aloud's sentence (`tag`): the playback then
    /// knows which sentence is playing, and how far in ([`position`]).
    ///
    /// [`position`]: Playback::position
    fn append_tagged(&self, pcm: Pcm, tag: Tag) {
        let _ = tag;
        self.append(pcm);
    }
    /// Empty the queue and go quiet, now.
    fn stop(&self);
    /// Played to the end (or stopped). Never while paused mid-way.
    fn done(&self) -> bool;
    /// Hold where it is (a read-aloud's ⏸), and go on from there.
    fn pause(&self) {}
    fn resume(&self) {}
    fn paused(&self) -> bool {
        false
    }
    /// The tagged sentence playing now (or last), and how far into its
    /// audio. `None` before any tagged audio has played.
    fn position(&self) -> Option<(usize, Duration)> {
        None
    }
}

/// Which read-aloud sentence a piece of audio is (`Track`'s index), and
/// where it starts in that sentence's audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tag {
    pub sentence: usize,
    pub offset: Duration,
}

/// A position as one atomic word: the sentence + 1 above bit 40, the
/// nanoseconds into it below (≤ 18 minutes a sentence). 0 = none yet.
const POS_SHIFT: u32 = 40;
const POS_NS: u64 = (1 << POS_SHIFT) - 1;

fn pack_position(sentence: usize, ns: u64) -> u64 {
    ((sentence as u64 + 1) << POS_SHIFT) | ns.min(POS_NS)
}

fn unpack_position(v: u64) -> Option<(usize, Duration)> {
    (v != 0).then(|| (((v >> POS_SHIFT) - 1) as usize, Duration::from_nanos(v & POS_NS)))
}

/// A sentence's audio on its way to the output, saying how far it's got:
/// every sample the output takes moves [`RodioPlayback`]'s position on
/// (paused, the output takes none).
struct Tagged<S> {
    inner: S,
    pos: Arc<AtomicU64>,
    sentence: usize,
    base_ns: f64,
    ns_per_sample: f64,
    taken: u64,
}

impl<S: Iterator<Item = f32>> Iterator for Tagged<S> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let s = self.inner.next()?;
        self.taken += 1;
        let ns = self.base_ns + self.taken as f64 * self.ns_per_sample;
        self.pos.store(pack_position(self.sentence, ns as u64), Ordering::Relaxed);
        Some(s)
    }
}

impl<S: rodio::Source> rodio::Source for Tagged<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }
    fn channels(&self) -> rodio::ChannelCount {
        self.inner.channels()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}

/// `pcm` as rodio plays it. `None` for audio without channels or a rate:
/// nothing to play (rodio 0.19's buffer panicked on it).
fn buffer(pcm: Pcm) -> Option<rodio::buffer::SamplesBuffer> {
    let channels = std::num::NonZero::new(pcm.channels)?;
    let rate = std::num::NonZero::new(pcm.sample_rate)?;
    Some(rodio::buffer::SamplesBuffer::new(channels, rate, pcm.samples))
}

/// A playback, plus whatever must stay alive (on the worker's thread) for it
/// to play: rodio's output (its device sink).
pub struct Opened {
    pub play: Arc<dyn Playback>,
    pub keep: Option<Box<dyn std::any::Any>>,
}

/// Opens the output, on the worker's thread.
pub type Opener = Box<dyn FnOnce() -> Result<Opened, String> + Send>;

// ---- The voice's volume (the maintainer, 2026-10-20: "so loud, even at 1% on the
// volume mixer"). Yap scales its own voice, so it's quiet enough whatever
// the Windows mixer says; a change applies at once, mid-sentence too.

/// The volume the voice plays at (0.0–1.0, `config.tts_volume`), as f32 bits.
static VOLUME: AtomicU32 = AtomicU32::new(0x3F00_0000); // 0.5
/// The player playing now, to change its volume while it speaks.
static LIVE_PLAYER: Mutex<Option<std::sync::Weak<rodio::Player>>> = Mutex::new(None);

/// A volume setting as the gain the samples get: squared, so the low end is
/// really quiet — half is a quarter of the voice's full loudness (−12 dB),
/// 10 % a hundredth (−40 dB), 100 % the voice as it comes. Pure.
pub fn gain(volume: f32) -> f32 {
    let v = if volume.is_finite() { volume.clamp(0.0, 1.0) } else { 0.5 };
    v * v
}

/// The volume setting now (0.0–1.0).
pub fn volume() -> f32 {
    f32::from_bits(VOLUME.load(Ordering::Relaxed))
}

/// Set the voice's volume (0.0–1.0): the sentence playing now included.
/// From every config save (`shell::settings::save_config`), startup, and the
/// Chat view's slider while it moves (`tts_volume_set`).
pub fn set_volume(volume: f32) {
    let v = if volume.is_finite() { volume.clamp(0.0, 1.0) } else { 0.5 };
    VOLUME.store(v.to_bits(), Ordering::Relaxed);
    let player = LIVE_PLAYER.lock().unwrap_or_else(|p| p.into_inner()).as_ref().and_then(std::sync::Weak::upgrade);
    if let Some(player) = player {
        player.set_volume(gain(v));
    }
}

struct RodioPlayback {
    player: Arc<rodio::Player>,
    /// [`pack_position`] of the tagged sample the output took last.
    pos: Arc<AtomicU64>,
    /// The other apps turned down while this plays ("Turn other apps down
    /// while Yap speaks", `voice/duck.rs`): held from the first audio, let
    /// go on a pause, a stop and when this output closes. This PC's
    /// speakers only: a phone's playback (`link/voice.rs`) has none.
    duck: crate::voice::duck::Duck,
}

impl Playback for RodioPlayback {
    fn append(&self, pcm: Pcm) {
        let Some(audio) = buffer(pcm) else { return };
        self.duck.hold();
        self.player.append(audio);
    }
    fn append_tagged(&self, pcm: Pcm, tag: Tag) {
        let ns_per_sample = 1e9 / (f64::from(pcm.sample_rate.max(1)) * f64::from(pcm.channels.max(1)));
        let Some(audio) = buffer(pcm) else { return };
        self.duck.hold();
        self.player.append(Tagged {
            inner: audio,
            pos: Arc::clone(&self.pos),
            sentence: tag.sentence,
            base_ns: tag.offset.as_nanos() as f64,
            ns_per_sample,
            taken: 0,
        });
    }
    fn stop(&self) {
        self.player.stop();
        self.duck.let_go();
    }
    fn done(&self) -> bool {
        self.player.empty()
    }
    fn pause(&self) {
        self.player.pause();
        self.duck.let_go();
    }
    fn resume(&self) {
        self.player.play();
        if !self.player.empty() {
            self.duck.hold();
        }
    }
    fn paused(&self) -> bool {
        self.player.is_paused()
    }
    fn position(&self) -> Option<(usize, Duration)> {
        unpack_position(self.pos.load(Ordering::Relaxed))
    }
}

/// The configured output device (`config.output_device`, else the default),
/// as the chimes use it.
pub(super) fn rodio_opener(device: Option<String>) -> Opener {
    Box::new(move || {
        let out = crate::dictation::sound::open_output(device.as_deref()).ok_or("No sound output device to speak on")?;
        let player = Arc::new(rodio::Player::connect_new(out.mixer()));
        player.set_volume(gain(volume()));
        *LIVE_PLAYER.lock().unwrap_or_else(|p| p.into_inner()) = Some(Arc::downgrade(&player));
        Ok(Opened {
            play: Arc::new(RodioPlayback { player, pos: Arc::new(AtomicU64::new(0)), duck: crate::voice::duck::Duck::new() }),
            keep: Some(Box::new(out)),
        })
    })
}

/// Plays nothing: each sentence takes its length on the clock — a clock
/// that stands still while paused. (Test mode and the unit tests.)
#[derive(Default)]
pub struct SimPlayback {
    clock: Mutex<SimClock>,
    stopped: std::sync::atomic::AtomicBool,
    pub(super) appended: AtomicU64,
}

/// [`SimPlayback`]'s clock: time played so far ("virtual" time: the pauses
/// left out), when the queue runs out, and what's queued (the last few).
#[derive(Default)]
struct SimClock {
    origin: Option<Instant>,
    paused_at: Option<Instant>,
    /// All the time spent paused.
    held: Duration,
    ends: Option<Duration>,
    queued: std::collections::VecDeque<(Duration, Duration, Option<Tag>)>,
}

impl SimClock {
    fn now(&mut self) -> Duration {
        let origin = *self.origin.get_or_insert_with(Instant::now);
        let at = self.paused_at.unwrap_or_else(Instant::now);
        at.saturating_duration_since(origin).saturating_sub(self.held)
    }
}

impl SimPlayback {
    fn clock(&self) -> std::sync::MutexGuard<'_, SimClock> {
        self.clock.lock().unwrap_or_else(|p| p.into_inner())
    }
    fn push(&self, length: Duration, tag: Option<Tag>) {
        let mut c = self.clock();
        let now = c.now();
        let from = c.ends.filter(|e| *e > now).unwrap_or(now);
        c.ends = Some(from + length);
        c.queued.push_back((from, length, tag));
        while c.queued.len() > 64 {
            c.queued.pop_front();
        }
        drop(c);
        self.appended.fetch_add(1, Ordering::SeqCst);
    }
}

impl Playback for SimPlayback {
    fn append(&self, pcm: Pcm) {
        self.push(pcm.duration(), None);
    }
    fn append_tagged(&self, pcm: Pcm, tag: Tag) {
        self.push(pcm.duration(), Some(tag));
    }
    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
    }
    fn done(&self) -> bool {
        if self.stopped.load(Ordering::SeqCst) {
            return true;
        }
        let mut c = self.clock();
        if c.paused_at.is_some() {
            return false;
        }
        let now = c.now();
        c.ends.is_none_or(|e| now >= e)
    }
    fn pause(&self) {
        let mut c = self.clock();
        if c.paused_at.is_none() {
            c.now();
            c.paused_at = Some(Instant::now());
        }
    }
    fn resume(&self) {
        let mut c = self.clock();
        if let Some(p) = c.paused_at.take() {
            c.held += p.elapsed();
        }
    }
    fn paused(&self) -> bool {
        self.clock().paused_at.is_some()
    }
    fn position(&self) -> Option<(usize, Duration)> {
        let mut c = self.clock();
        let now = c.now();
        c.queued
            .iter()
            .rev()
            .filter_map(|&(from, len, tag)| tag.map(|t| (from, len, t)))
            .find(|&(from, _, _)| from <= now)
            .map(|(from, len, t)| (t.sentence, t.offset + (now - from).min(len)))
    }
}

pub(super) fn sim_opener() -> Opener {
    Box::new(|| Ok(Opened { play: Arc::new(SimPlayback::default()), keep: None }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sentence reaches rodio as it was made (every sample, its channels
    /// and rate), and its tag moves the position on as the output takes it:
    /// half its samples taken, half its length in.
    #[test]
    fn a_tagged_sentence_plays_as_made_and_says_how_far_it_has_got() {
        use rodio::Source;
        let samples: Vec<f32> = (0..48_000).map(|i| (i as f32 / 48_000.0) - 0.5).collect();
        let pcm = || Pcm { sample_rate: 24_000, channels: 2, samples: samples.clone() };
        let pos = Arc::new(AtomicU64::new(0));
        let mut tagged = Tagged {
            inner: buffer(pcm()).unwrap(),
            pos: Arc::clone(&pos),
            sentence: 3,
            base_ns: Duration::from_millis(250).as_nanos() as f64,
            ns_per_sample: 1e9 / (24_000.0 * 2.0),
            taken: 0,
        };
        assert_eq!((tagged.channels().get(), tagged.sample_rate().get()), (2, 24_000));
        assert_eq!(tagged.total_duration(), Some(Duration::from_secs(1)));
        let first: Vec<f32> = tagged.by_ref().take(24_000).collect();
        let (sentence, at) = unpack_position(pos.load(Ordering::Relaxed)).unwrap();
        assert_eq!(sentence, 3);
        assert!(at.abs_diff(Duration::from_millis(750)) < Duration::from_micros(50), "{at:?}");
        let rest: Vec<f32> = tagged.collect();
        assert_eq!([first, rest].concat(), samples);
        // Audio without channels or a rate is nothing to play.
        assert!(buffer(Pcm { sample_rate: 0, ..pcm() }).is_none());
        assert!(buffer(Pcm { channels: 0, ..pcm() }).is_none());
    }

    /// The volume, a pause and a stop reach what the output takes: half
    /// volume is a quarter of each sample, paused gives silence, stopped
    /// ends the queue.
    #[test]
    fn the_volume_a_pause_and_a_stop_reach_the_output() {
        let (player, mut output) = rodio::Player::new();
        player.set_volume(gain(0.5));
        player.append(buffer(Pcm { sample_rate: 24_000, channels: 1, samples: vec![0.8; 24_000] }).unwrap());
        // rodio applies its controls every 5 ms of audio: past the first.
        let heard: Vec<f32> = output.by_ref().take(2_400).collect();
        assert!(heard[240..].iter().all(|s| (s - 0.2).abs() < 1e-6), "a quarter of 0.8");
        player.pause();
        let held: Vec<f32> = output.by_ref().take(2_400).collect();
        assert!(held[240..].iter().all(|s| *s == 0.0), "paused: silence");
        assert!(!player.empty() && player.is_paused());
        player.play();
        player.stop();
        let _: Vec<f32> = output.by_ref().take(2_400).collect();
        assert!(player.empty(), "stopped: nothing left");
    }

    #[test]
    fn the_volume_curve_is_quiet_at_the_low_end() {
        assert_eq!(gain(1.0), 1.0);
        assert_eq!(gain(0.5), 0.25);
        assert!((gain(0.1) - 0.01).abs() < 1e-6);
        assert_eq!(gain(0.0), 0.0);
        // Out of range or not a number: clamped, else the default half.
        assert_eq!(gain(3.0), 1.0);
        assert_eq!(gain(-1.0), 0.0);
        assert_eq!(gain(f32::NAN), 0.25);
    }
}

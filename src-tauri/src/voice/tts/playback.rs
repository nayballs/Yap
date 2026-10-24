//! Where the audio goes: rodio on the output device, or a simulated
//! clock in test mode; a read-aloud's sentences tagged with their place,
//! and the voice's volume.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::Pcm;

/// Where the audio goes: rodio's sink, or a simulated clock in test mode.
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

impl<S: rodio::Source<Item = f32>> rodio::Source for Tagged<S> {
    fn current_frame_len(&self) -> Option<usize> {
        self.inner.current_frame_len()
    }
    fn channels(&self) -> u16 {
        self.inner.channels()
    }
    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}

/// A playback, plus whatever must stay alive (on the worker's thread) for it
/// to play: rodio's output stream.
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
/// The sink playing now, to change its volume while it speaks.
static LIVE_SINK: Mutex<Option<std::sync::Weak<rodio::Sink>>> = Mutex::new(None);

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
    let sink = LIVE_SINK.lock().unwrap_or_else(|p| p.into_inner()).as_ref().and_then(std::sync::Weak::upgrade);
    if let Some(sink) = sink {
        sink.set_volume(gain(v));
    }
}

struct RodioPlayback {
    sink: Arc<rodio::Sink>,
    /// [`pack_position`] of the tagged sample the output took last.
    pos: Arc<AtomicU64>,
}

impl Playback for RodioPlayback {
    fn append(&self, pcm: Pcm) {
        self.sink.append(rodio::buffer::SamplesBuffer::new(pcm.channels, pcm.sample_rate, pcm.samples));
    }
    fn append_tagged(&self, pcm: Pcm, tag: Tag) {
        let ns_per_sample = 1e9 / (f64::from(pcm.sample_rate.max(1)) * f64::from(pcm.channels.max(1)));
        self.sink.append(Tagged {
            inner: rodio::buffer::SamplesBuffer::new(pcm.channels, pcm.sample_rate, pcm.samples),
            pos: Arc::clone(&self.pos),
            sentence: tag.sentence,
            base_ns: tag.offset.as_nanos() as f64,
            ns_per_sample,
            taken: 0,
        });
    }
    fn stop(&self) {
        self.sink.stop();
    }
    fn done(&self) -> bool {
        self.sink.empty()
    }
    fn pause(&self) {
        self.sink.pause();
    }
    fn resume(&self) {
        self.sink.play();
    }
    fn paused(&self) -> bool {
        self.sink.is_paused()
    }
    fn position(&self) -> Option<(usize, Duration)> {
        unpack_position(self.pos.load(Ordering::Relaxed))
    }
}

/// The configured output device (`config.output_device`, else the default),
/// as the chimes use it.
pub(super) fn rodio_opener(device: Option<String>) -> Opener {
    Box::new(move || {
        let (stream, handle) =
            crate::dictation::sound::open_output(device.as_deref()).ok_or("No sound output device to speak on")?;
        let sink = Arc::new(rodio::Sink::try_new(&handle).map_err(|e| format!("Couldn't play the voice: {e}"))?);
        sink.set_volume(gain(volume()));
        *LIVE_SINK.lock().unwrap_or_else(|p| p.into_inner()) = Some(Arc::downgrade(&sink));
        Ok(Opened {
            play: Arc::new(RodioPlayback { sink, pos: Arc::new(AtomicU64::new(0)) }),
            keep: Some(Box::new(stream)),
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

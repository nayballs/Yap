//! Tiny audio cues so you can *hear* when Yap starts/stops listening.
//!
//! Two-note sine chimes synthesized on the fly (no asset files needed),
//! ported from OpenWhispr's `utils/dictationCues.js`: a rising C5→E5 when
//! recording starts, a falling D5→A4 when it stops. Each note gets a 15 ms
//! attack ramp and an exponential fade-out — no hard edges, no clicks.

use std::thread;
use std::time::{Duration, Instant};

const SAMPLE_RATE: u32 = 44_100;
// OpenWhispr dictationCues.js constants, verbatim.
const START_NOTES: [f32; 2] = [523.25, 659.25]; // C5 → E5
const STOP_NOTES: [f32; 2] = [587.33, 440.0]; // D5 → A4
const NOTE_DURATION: f32 = 0.09;
const NOTE_GAP: f32 = 0.025;
const NOTE_ATTACK: f32 = 0.015;
const MAX_GAIN: f32 = 0.2;
const MIN_GAIN: f32 = 0.0001;

/// Run once a chime has played (on the chime's thread), or straight away
/// when it can't play: the other apps go quiet after the start chime and
/// come back after the stop chime (`mute.rs`).
pub type Played = Box<dyn FnOnce() + Send + 'static>;

/// Rising chime — recording started. `volume` scales loudness (0.0–1.0).
/// `device` selects the output device by name (None = system default).
pub fn play_start(volume: f32, device: Option<&str>, played: Option<Played>) {
    play_cue(&START_NOTES, volume, device, played);
}

/// Falling chime — recording stopped / processing. `volume` scales loudness.
/// `device` selects the output device by name (None = system default).
pub fn play_stop(volume: f32, device: Option<&str>, played: Option<Played>) {
    play_cue(&STOP_NOTES, volume, device, played);
}

/// Render the note sequence into a mono f32 buffer: per note, a linear
/// attack to `peak` then an exponential decay to MIN_GAIN (matching Web
/// Audio's linearRampToValueAtTime + exponentialRampToValueAtTime).
fn render_cue(notes: &[f32], peak: f32) -> Vec<f32> {
    let note_step = NOTE_DURATION + NOTE_GAP;
    let total = note_step * notes.len() as f32 + 0.02; // small silent tail
    let mut samples = vec![0.0f32; (total * SAMPLE_RATE as f32) as usize];

    for (i, &freq) in notes.iter().enumerate() {
        let note_start = i as f32 * note_step;
        let first = (note_start * SAMPLE_RATE as f32) as usize;
        let count = (NOTE_DURATION * SAMPLE_RATE as f32) as usize;
        let decay_len = NOTE_DURATION - NOTE_ATTACK;
        for j in 0..count {
            let t = j as f32 / SAMPLE_RATE as f32; // time within the note
            let gain = if t < NOTE_ATTACK {
                peak * (t / NOTE_ATTACK)
            } else {
                peak * (MIN_GAIN / MAX_GAIN).powf((t - NOTE_ATTACK) / decay_len)
            };
            let phase = std::f32::consts::TAU * freq * t;
            if let Some(s) = samples.get_mut(first + j) {
                *s += gain * phase.sin();
            }
        }
    }
    samples
}

/// Open an output stream on the named device, falling back to the system
/// default when no name is given or the device can't be found. Uses rodio's
/// re-exported cpal so the device type matches `OutputStream::try_from_device`.
pub(crate) fn open_output(device: Option<&str>) -> Option<(rodio::OutputStream, rodio::OutputStreamHandle)> {
    use rodio::cpal::traits::{DeviceTrait, HostTrait};

    if let Some(name) = device {
        let host = rodio::cpal::default_host();
        if let Ok(mut devs) = host.output_devices() {
            if let Some(dev) = devs.find(|d| d.name().map(|n| n == name).unwrap_or(false)) {
                if let Ok(pair) = rodio::OutputStream::try_from_device(&dev) {
                    return Some(pair);
                }
                // Its name at debug only: a headset's can carry its owner's.
                tracing::warn!("Failed to open chime output device; using default");
                tracing::debug!(device = name, "Chime output device that wouldn't open");
            }
        }
    }
    rodio::OutputStream::try_default().ok()
}

/// The longest a chime is waited for: it lasts a quarter of a second, so
/// past this its output has stopped taking audio.
const PLAY_LIMIT: Duration = Duration::from_secs(3);

/// Wait until `sink` has played what it holds, or `limit` at most. An
/// output device that goes while a chime plays (headphones unplugged, a
/// Bluetooth headset switched off with its mic, mid-dictation) stops its
/// stream, and nothing drains the sink again: rodio's `sleep_until_end`
/// waited for ever then, and what runs after the stop chime — the
/// dictation's mic let go, the other apps' sound back — never did.
fn wait_played(sink: &rodio::Sink, limit: Duration) {
    let started = Instant::now();
    while !sink.empty() {
        if started.elapsed() >= limit {
            tracing::warn!("Chime: its output stopped taking audio; going on without it");
            return;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn play_cue(notes: &'static [f32], volume: f32, device: Option<&str>, played: Option<Played>) {
    let peak = MAX_GAIN * volume.clamp(0.0, 1.0);
    if peak <= 0.0 && played.is_none() {
        return;
    }
    let device = device.map(str::to_string);
    // Play on a detached thread; OutputStream must stay alive until the
    // sink drains ([`wait_played`] blocks, for a moment at most). `played`
    // runs after it (and after a chime that couldn't play), on the same
    // thread.
    thread::spawn(move || {
        use rodio::buffer::SamplesBuffer;
        use rodio::Sink;

        if peak > 0.0 {
            if let Some((_stream, handle)) = open_output(device.as_deref()) {
                if let Ok(sink) = Sink::try_new(&handle) {
                    let samples = render_cue(notes, peak);
                    sink.append(SamplesBuffer::new(1, SAMPLE_RATE, samples));
                    wait_played(&sink, PLAY_LIMIT);
                }
            }
        }
        if let Some(f) = played {
            f();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::buffer::SamplesBuffer;

    /// The chime's output stopped taking audio (its device unplugged while
    /// it played): nothing drains the sink again. `sleep_until_end` waited
    /// for ever, so the stop chime's `played` — the mic let go, the other
    /// apps' sound back — never ran. Now the wait gives up.
    #[test]
    fn a_chime_whose_output_stopped_is_waited_for_a_moment_only() {
        // A sink nothing plays from: its output gone.
        let (sink, _stopped_output) = rodio::Sink::new_idle();
        sink.append(SamplesBuffer::new(1, SAMPLE_RATE, render_cue(&STOP_NOTES, MAX_GAIN)));
        let (tx, rx) = std::sync::mpsc::channel();
        thread::spawn(move || {
            wait_played(&sink, Duration::from_millis(300));
            let _ = tx.send(());
        });
        assert!(rx.recv_timeout(Duration::from_secs(5)).is_ok(), "the wait gave up");

        // An output that plays it: done as soon as it has.
        let (sink, mut output) = rodio::Sink::new_idle();
        sink.append(SamplesBuffer::new(1, SAMPLE_RATE, render_cue(&START_NOTES, MAX_GAIN)));
        let player = thread::spawn(move || while output.next().is_some() {});
        let started = Instant::now();
        wait_played(&sink, Duration::from_secs(10));
        assert!(started.elapsed() < Duration::from_secs(5), "played, not timed out");
        drop(sink);
        let _ = player.join();
    }
}

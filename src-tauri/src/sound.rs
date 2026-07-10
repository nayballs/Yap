//! Tiny audio cues so you can *hear* when Yap starts/stops listening.
//!
//! Two-note sine chimes synthesized on the fly (no asset files needed),
//! ported from OpenWhispr's `utils/dictationCues.js`: a rising C5→E5 when
//! recording starts, a falling D5→A4 when it stops. Each note gets a 15 ms
//! attack ramp and an exponential fade-out — no hard edges, no clicks.

use std::thread;

const SAMPLE_RATE: u32 = 44_100;
// OpenWhispr dictationCues.js constants, verbatim.
const START_NOTES: [f32; 2] = [523.25, 659.25]; // C5 → E5
const STOP_NOTES: [f32; 2] = [587.33, 440.0]; // D5 → A4
const NOTE_DURATION: f32 = 0.09;
const NOTE_GAP: f32 = 0.025;
const NOTE_ATTACK: f32 = 0.015;
const MAX_GAIN: f32 = 0.2;
const MIN_GAIN: f32 = 0.0001;

/// Rising chime — recording started. `volume` scales loudness (0.0–1.0).
/// `device` selects the output device by name (None = system default).
pub fn play_start(volume: f32, device: Option<&str>) {
    play_cue(&START_NOTES, volume, device);
}

/// Falling chime — recording stopped / processing. `volume` scales loudness.
/// `device` selects the output device by name (None = system default).
pub fn play_stop(volume: f32, device: Option<&str>) {
    play_cue(&STOP_NOTES, volume, device);
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
fn open_output(device: Option<&str>) -> Option<(rodio::OutputStream, rodio::OutputStreamHandle)> {
    use rodio::cpal::traits::{DeviceTrait, HostTrait};

    if let Some(name) = device {
        let host = rodio::cpal::default_host();
        if let Ok(mut devs) = host.output_devices() {
            if let Some(dev) = devs.find(|d| d.name().map(|n| n == name).unwrap_or(false)) {
                if let Ok(pair) = rodio::OutputStream::try_from_device(&dev) {
                    return Some(pair);
                }
                tracing::warn!(device = name, "Failed to open chime output device; using default");
            }
        }
    }
    rodio::OutputStream::try_default().ok()
}

fn play_cue(notes: &'static [f32], volume: f32, device: Option<&str>) {
    let peak = MAX_GAIN * volume.clamp(0.0, 1.0);
    if peak <= 0.0 {
        return;
    }
    let device = device.map(str::to_string);
    // Play on a detached thread; OutputStream must stay alive until the
    // sink drains, and sleep_until_end() blocks.
    thread::spawn(move || {
        use rodio::buffer::SamplesBuffer;
        use rodio::Sink;

        let Some((_stream, handle)) = open_output(device.as_deref()) else {
            return;
        };
        let Ok(sink) = Sink::try_new(&handle) else {
            return;
        };
        let samples = render_cue(notes, peak);
        sink.append(SamplesBuffer::new(1, SAMPLE_RATE, samples));
        sink.sleep_until_end();
    });
}

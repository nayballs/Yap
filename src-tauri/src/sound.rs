//! Tiny audio cues so you can *hear* when Blip starts/stops listening.
//!
//! Generates short sine beeps on the fly (no asset files needed): a higher
//! pitch when recording starts, a lower one when it stops.

use std::thread;
use std::time::Duration;

/// Rising chime — recording started. `volume` scales loudness (0.0–1.0).
/// `device` selects the output device by name (None = system default).
pub fn play_start(volume: f32, device: Option<&str>) {
    play_beep(880.0, 90, volume, device);
}

/// Falling chime — recording stopped / processing. `volume` scales loudness.
/// `device` selects the output device by name (None = system default).
pub fn play_stop(volume: f32, device: Option<&str>) {
    play_beep(523.0, 110, volume, device);
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

fn play_beep(freq: f32, ms: u64, volume: f32, device: Option<&str>) {
    let amplitude = 0.15 * volume.clamp(0.0, 1.0);
    if amplitude <= 0.0 {
        return;
    }
    let device = device.map(str::to_string);
    // Play on a detached thread; OutputStream must stay alive until the
    // sink drains, and sleep_until_end() blocks.
    thread::spawn(move || {
        use rodio::source::{SineWave, Source};
        use rodio::Sink;

        let Some((_stream, handle)) = open_output(device.as_deref()) else {
            return;
        };
        let Ok(sink) = Sink::try_new(&handle) else {
            return;
        };
        let source = SineWave::new(freq)
            .take_duration(Duration::from_millis(ms))
            .amplify(amplitude);
        sink.append(source);
        sink.sleep_until_end();
    });
}

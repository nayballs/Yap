//! Tiny audio cues so you can *hear* when Blip starts/stops listening.
//!
//! Generates short sine beeps on the fly (no asset files needed): a higher
//! pitch when recording starts, a lower one when it stops.

use std::thread;
use std::time::Duration;

/// Rising chime — recording started.
pub fn play_start() {
    play_beep(880.0, 90);
}

/// Falling chime — recording stopped / processing.
pub fn play_stop() {
    play_beep(523.0, 110);
}

fn play_beep(freq: f32, ms: u64) {
    // Play on a detached thread; OutputStream must stay alive until the
    // sink drains, and sleep_until_end() blocks.
    thread::spawn(move || {
        use rodio::source::{SineWave, Source};
        use rodio::{OutputStream, Sink};

        let Ok((_stream, handle)) = OutputStream::try_default() else {
            return;
        };
        let Ok(sink) = Sink::try_new(&handle) else {
            return;
        };
        let source = SineWave::new(freq)
            .take_duration(Duration::from_millis(ms))
            .amplify(0.15);
        sink.append(source);
        sink.sleep_until_end();
    });
}

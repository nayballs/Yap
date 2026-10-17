//! The dictation's **dead-mic watchdog** (Wispr Flow's, by behaviour — its
//! `reopenDictationMic`). A mic that's muted (in Windows or on the device),
//! blocked by Windows' privacy switch, or broken delivers **digital
//! silence** — samples that are exactly 0.0 — or no audio at all. A quiet
//! room is never all zeros (a working mic's own noise sees to that), so only
//! exact zeros count, and a merely quiet mic never trips it.
//!
//! The audio callback tallies each recording's live audio ([`MicTally`]:
//! how much arrived, whether any of it wasn't zero); a watcher thread polls
//! [`DeadMicWatch`] every [`POLL_MS`] while the recording runs:
//!
//! - no audio for [`NO_AUDIO_MS`], or [`ZEROS_MS`] of nothing but zeros →
//!   **reopen** the input stream, once, and keep recording;
//! - still nothing at [`REPORT_MS`] → **say so** ("No sound from <mic>…");
//!   the recording carries on, since the mic may come back (unmuted, plugged
//!   in), and the warning goes away when it does.
//!
//! Only the start of a recording is watched: once a sample that isn't zero
//! has arrived the mic works, and the watcher stops. Dictation only: a
//! meeting records through its own capture and has its own quiet-side
//! watch (`meeting.rs`).

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Audio rate of the tallied chunks (the pipeline's 16 kHz mono).
const RATE: u64 = crate::pipeline::TARGET_SAMPLE_RATE as u64;
/// No audio at all for this long: the stream has stalled (or never started).
pub const NO_AUDIO_MS: u64 = 2_000;
/// This much audio, all of it zeros: digital silence.
pub const ZEROS_MS: u64 = 3_000;
/// Still nothing at this point of the recording: tell the user.
pub const REPORT_MS: u64 = 5_000;
/// How often the watcher looks.
pub const POLL_MS: u64 = 250;

/// Anything but digital silence (`-0.0` is silence too).
pub fn has_sound(chunk: &[f32]) -> bool {
    chunk.iter().any(|&s| s != 0.0)
}

/// One recording's live audio as the callback saw it. Lock-free: written
/// from the audio callback, read by the watcher.
#[derive(Debug, Default)]
pub struct MicTally {
    samples: AtomicU64,
    heard: AtomicBool,
}

impl MicTally {
    /// A recording is starting.
    pub fn reset(&self) {
        self.samples.store(0, Ordering::Relaxed);
        self.heard.store(false, Ordering::Relaxed);
    }

    /// One callback's audio (16 kHz mono) while recording. Cheap: real
    /// audio is found non-zero within its first samples, and once heard
    /// nothing is scanned again.
    pub fn note(&self, chunk: &[f32]) {
        self.samples.fetch_add(chunk.len() as u64, Ordering::Relaxed);
        if !self.heard.load(Ordering::Relaxed) && has_sound(chunk) {
            self.heard.store(true, Ordering::Relaxed);
        }
    }

    /// Any sample that wasn't zero so far.
    pub fn heard(&self) -> bool {
        self.heard.load(Ordering::Relaxed)
    }

    /// `(samples so far, heard)`.
    pub fn read(&self) -> (u64, bool) {
        (self.samples.load(Ordering::Relaxed), self.heard())
    }
}

/// What the watcher should do now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicStep {
    /// Keep watching.
    Wait,
    /// Reopen the input stream (once per recording).
    Reopen,
    /// Tell the user the mic gives nothing (once per recording).
    Report,
    /// Sound arrived after the report: take the warning back. Done.
    Recovered,
    /// Sound arrived: the mic works. Done.
    Fine,
}

/// The watchdog's decisions for one recording. Pure: the recording's age
/// and its [`MicTally`] in, a [`MicStep`] out.
#[derive(Debug, Default)]
pub struct DeadMicWatch {
    reopened: bool,
    reported: bool,
    /// The tally's sample count at the last poll, and when it last grew.
    last_samples: u64,
    last_growth_ms: u64,
}

impl DeadMicWatch {
    /// `elapsed_ms` into the recording, `samples` of live audio so far,
    /// `heard` any that wasn't zero.
    pub fn poll(&mut self, elapsed_ms: u64, samples: u64, heard: bool) -> MicStep {
        if samples != self.last_samples {
            self.last_samples = samples;
            self.last_growth_ms = elapsed_ms;
        }
        if heard {
            return if self.reported { MicStep::Recovered } else { MicStep::Fine };
        }
        let stalled = elapsed_ms.saturating_sub(self.last_growth_ms) >= NO_AUDIO_MS;
        let zeros_ms = samples * 1_000 / RATE;
        if !stalled && zeros_ms < ZEROS_MS {
            return MicStep::Wait;
        }
        if !self.reopened {
            self.reopened = true;
            return MicStep::Reopen;
        }
        if !self.reported && elapsed_ms >= REPORT_MS {
            self.reported = true;
            return MicStep::Report;
        }
        MicStep::Wait
    }

    /// The user has been told.
    #[cfg(test)]
    pub fn reported(&self) -> bool {
        self.reported
    }
}

/// "No sound from Microphone (Realtek Audio) — check it isn't muted or
/// unplugged": the error (the main window's toast, the log).
pub fn message(mic: &str) -> String {
    let mic = mic.trim();
    if mic.is_empty() || mic == "unknown" {
        "No sound from your microphone — check it isn't muted or unplugged".to_string()
    } else {
        format!("No sound from {mic} — check it isn't muted or unplugged")
    }
}

/// The same, short enough for the dictation overlay's one line: "No sound
/// from Realtek Audio — is it muted?" (Windows calls its inputs
/// "Microphone (<the device>)": the device is the telling part).
pub fn short_message(mic: &str) -> String {
    let mic = mic.trim();
    let device = mic
        .strip_suffix(')')
        .and_then(|m| m.split_once(" ("))
        .map(|(_, inner)| inner.trim())
        .filter(|d| !d.is_empty())
        .unwrap_or(mic);
    if device.is_empty() || device == "unknown" {
        "No sound from your mic — is it muted?".to_string()
    } else {
        format!("No sound from {device} — is it muted?")
    }
}

/// What the overlay gets (`yap-mic-silent`; `null` takes it back).
#[derive(Debug, Clone, serde::Serialize)]
pub struct MicSilent {
    /// [`message`].
    pub message: String,
    /// [`short_message`], for its one line.
    pub short: String,
}

impl MicSilent {
    pub fn for_mic(mic: &str) -> Self {
        MicSilent { message: message(mic), short: short_message(mic) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A callback's worth of audio: 10 ms at 16 kHz.
    const CHUNK: usize = 160;

    /// What the watcher saw in a simulated recording: the audio callback
    /// delivers `chunk(ms)` every 10 ms (`None`: no callback then), the
    /// watcher polls every `POLL_MS`; `on_reopen` runs at a reopen (the new
    /// stream's audio from then on). Returns every step that wasn't `Wait`,
    /// with its time, up to `until_ms` or the watcher's end.
    fn simulate(
        until_ms: u64,
        mut chunk: impl FnMut(u64, bool) -> Option<Vec<f32>>,
    ) -> Vec<(u64, MicStep)> {
        let tally = MicTally::default();
        tally.reset();
        let mut watch = DeadMicWatch::default();
        let mut reopened = false;
        let mut out = Vec::new();
        let mut t = 0;
        while t <= until_ms {
            if let Some(c) = chunk(t, reopened) {
                tally.note(&c);
            }
            t += 10;
            if t.is_multiple_of(POLL_MS) {
                let (samples, heard) = tally.read();
                match watch.poll(t, samples, heard) {
                    MicStep::Wait => {}
                    step => {
                        out.push((t, step));
                        match step {
                            MicStep::Reopen => reopened = true,
                            MicStep::Fine | MicStep::Recovered => break,
                            _ => {}
                        }
                    }
                }
            }
        }
        out
    }

    fn zeros() -> Option<Vec<f32>> {
        Some(vec![0.0; CHUNK])
    }

    /// A quiet room through a working mic: about -70 dBFS of noise.
    fn room(t: u64) -> Option<Vec<f32>> {
        Some((0..CHUNK).map(|i| if (t as usize + i).is_multiple_of(2) { 3e-4 } else { -3e-4 }).collect())
    }

    #[test]
    fn digital_silence_reopens_at_three_seconds_and_reports_at_five() {
        let steps = simulate(8_000, |_, _| zeros());
        assert_eq!(steps, vec![(ZEROS_MS, MicStep::Reopen), (REPORT_MS, MicStep::Report)]);
    }

    #[test]
    fn no_audio_at_all_reopens_at_two_seconds_and_reports_at_five() {
        let steps = simulate(8_000, |_, _| None);
        assert_eq!(steps, vec![(NO_AUDIO_MS, MicStep::Reopen), (REPORT_MS, MicStep::Report)]);
    }

    #[test]
    fn a_quiet_mic_never_trips_it() {
        // The faintest noise isn't digital silence.
        assert_eq!(simulate(10_000, |t, _| room(t)), vec![(POLL_MS, MicStep::Fine)]);
        let one_bit: Vec<f32> = vec![0.0, 0.0, 1.0 / 32_768.0, 0.0];
        assert!(has_sound(&one_bit));
        // Silence is silence whatever its sign.
        assert!(!has_sound(&[0.0, -0.0, 0.0]));
        assert!(!has_sound(&[]));
    }

    #[test]
    fn a_pause_before_speaking_is_fine() {
        // Two and a half seconds of zeros (a noise-gated mic before you
        // speak), then a voice: nothing happens.
        let steps = simulate(8_000, |t, _| if t < 2_500 { zeros() } else { room(t) });
        assert_eq!(steps, vec![(2_750, MicStep::Fine)]);
    }

    #[test]
    fn a_reopen_that_brings_the_mic_back_reports_nothing() {
        // The old stream was stuck on zeros; the reopened one hears the room.
        let steps = simulate(8_000, |t, reopened| if reopened { room(t) } else { zeros() });
        assert_eq!(steps, vec![(ZEROS_MS, MicStep::Reopen), (ZEROS_MS + POLL_MS, MicStep::Fine)]);
    }

    #[test]
    fn a_reopen_that_brings_audio_back_after_a_stall_reports_nothing() {
        let steps = simulate(8_000, |t, reopened| if reopened { room(t) } else { None });
        assert_eq!(steps, vec![(NO_AUDIO_MS, MicStep::Reopen), (NO_AUDIO_MS + POLL_MS, MicStep::Fine)]);
    }

    #[test]
    fn unmuting_after_the_report_takes_it_back() {
        let steps = simulate(10_000, |t, _| if t < 7_000 { zeros() } else { room(t) });
        assert_eq!(
            steps,
            vec![(ZEROS_MS, MicStep::Reopen), (REPORT_MS, MicStep::Report), (7_250, MicStep::Recovered)]
        );
    }

    #[test]
    fn a_stream_that_stalls_after_a_moment_of_zeros_is_caught() {
        // Half a second of zeros, then the callbacks stop (unplugged).
        let steps = simulate(8_000, |t, _| if t < 500 { zeros() } else { None });
        assert_eq!(steps, vec![(2_500, MicStep::Reopen), (REPORT_MS, MicStep::Report)]);
    }

    #[test]
    fn reopen_and_report_happen_once() {
        let mut w = DeadMicWatch::default();
        assert_eq!(w.poll(3_000, 48_000, false), MicStep::Reopen);
        assert_eq!(w.poll(3_250, 52_000, false), MicStep::Wait);
        assert_eq!(w.poll(5_000, 80_000, false), MicStep::Report);
        assert!(w.reported());
        assert_eq!(w.poll(6_000, 96_000, false), MicStep::Wait);
        assert_eq!(w.poll(60_000, 960_000, false), MicStep::Wait);
    }

    #[test]
    fn the_message_names_the_mic() {
        assert_eq!(
            message("Microphone (Realtek(R) Audio)"),
            "No sound from Microphone (Realtek(R) Audio) — check it isn't muted or unplugged"
        );
        assert_eq!(message(""), "No sound from your microphone — check it isn't muted or unplugged");
    }

    #[test]
    fn the_short_message_names_the_device() {
        assert_eq!(short_message("Microphone (Razer Seiren Mini)"), "No sound from Razer Seiren Mini — is it muted?");
        assert_eq!(short_message("Microphone (Realtek(R) Audio)"), "No sound from Realtek(R) Audio — is it muted?");
        assert_eq!(short_message("Yeti Stereo Microphone"), "No sound from Yeti Stereo Microphone — is it muted?");
        assert_eq!(short_message("Headset ()"), "No sound from Headset () — is it muted?");
        assert_eq!(short_message(""), "No sound from your mic — is it muted?");
    }
}

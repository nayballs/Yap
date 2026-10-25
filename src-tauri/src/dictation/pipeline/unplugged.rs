//! **The microphone disconnected mid-dictation** (2026-10-25, `mics.rs`):
//! a USB headset pulled out, a Bluetooth one switched off or out of range.
//! Until then the recording went on hearing nothing: its stream had
//! stopped, the dead-mic watchdog (`mic_watch.rs`) had stopped watching once
//! the mic first worked, and the person talked on into nothing.
//!
//! Now the mic's thread (`mic.rs`) tells the dictation recording from it
//! ([`mic::Sink::lost`](crate::dictation::mic::Sink::lost)), and:
//!
//! - it **ends there**, as a press of the shortcut would (the stop chime,
//!   the other apps' sound back);
//! - what it heard is **transcribed and kept**, like any dictation — its
//!   audio a day (`recent_audio.rs`), the text in history — and pasted
//!   **only into the window it was for** (the Undo's strict paste): that
//!   window gone, the text is left on the clipboard and Home lists it as
//!   not pasted, to copy or run again;
//! - nothing heard yet (unplugged at the start): it ends without a
//!   transcription, the bar saying why;
//! - either way **one** alert says so — "Contoso H2 disconnected" — with
//!   what became of the words ([`Outcome`]: pasted, on the clipboard, kept)
//!   and which microphone the next dictation uses: the next one in the order
//!   that's connected, or Windows' default (`mics::candidates`). The paste
//!   says it when it knows ([`Shared::tell_unplugged`]); a dictation that
//!   pastes nothing (the Chat box, the Voice Assistant, no words heard) is
//!   told "kept what it heard" once it's done.
//!
//! What happens ([`on_lost`]) and what's said ([`lost_alert`]) are pure and
//! unit-tested.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::Emitter;

use super::{test_feed_on, Shared, DICTATION_ERROR, TARGET_SAMPLE_RATE};

/// Less than this (a quarter of a second) heard before the mic went: too
/// little to hold words.
const MIN_SAMPLES: u64 = TARGET_SAMPLE_RATE as u64 / 4;
/// A transcription this long hasn't said anything: the alert goes anyway.
const TELL_WITHIN: Duration = Duration::from_secs(180);

/// What a dictation does when its microphone is disconnected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OnLost {
    /// Not this dictation's (an older one's lease, or none recording), or a
    /// robot's WAV standing in for the mic: nothing.
    Ignore,
    /// It heard something: it ends, and what it heard is transcribed.
    Transcribe,
    /// Nothing heard yet: it ends without a transcription.
    EndEmpty,
}

/// See [`OnLost`]. `recording`: a dictation records; `ours`: from the mic
/// that went (its lease); `robot`: a WAV plays in the mic's place;
/// `heard`: any sample that wasn't digital silence; `samples`: how much it
/// captured (16 kHz).
pub(crate) fn on_lost(recording: bool, ours: bool, robot: bool, heard: bool, samples: u64) -> OnLost {
    if !recording || !ours || robot {
        OnLost::Ignore
    } else if heard && samples >= MIN_SAMPLES {
        OnLost::Transcribe
    } else {
        OnLost::EndEmpty
    }
}

/// What became of a dictation whose mic was disconnected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Pasted into the window it was for.
    Pasted,
    /// Its window was gone (or the app runs as administrator): on the
    /// clipboard.
    OnClipboard,
    /// Transcribed and kept (history; or the Chat box, the Voice
    /// Assistant), nothing pasted.
    Kept,
    /// It heard nothing: no transcription.
    Empty,
}

/// The mic a dictation lost, until the person is told.
#[derive(Debug, Clone)]
pub(crate) struct Lost {
    device: String,
    /// The microphone the next dictation uses (its name).
    next: Option<String>,
}

/// "Contoso H2 disconnected" — what became of the dictation, and which
/// microphone the next one uses (`next`: its name; `None`: none is
/// connected).
pub(crate) fn lost_alert(device: &str, next: Option<&str>, outcome: Outcome) -> crate::notices::alert::Alert {
    let did = match outcome {
        Outcome::Pasted => "Yap stopped the dictation and pasted what it heard.",
        Outcome::OnClipboard => "Yap stopped the dictation. What it heard is on the clipboard: press Ctrl+V.",
        Outcome::Kept => "Yap stopped the dictation and kept what it heard.",
        Outcome::Empty => "Yap stopped the dictation before it heard anything.",
    };
    let then = match next {
        Some(n) => format!("The next one uses {}.", crate::dictation::mics::short_name(n)),
        None => "Connect a microphone for the next one.".to_string(),
    };
    crate::notices::alert::Alert {
        kind: "micLost",
        icon: "mic",
        title: format!("{} disconnected", crate::dictation::mics::short_name(device)),
        body: format!("{did} {then}"),
        action: Some(crate::notices::alert::Action { id: crate::notices::alert::OPEN_MIC_ORDER, label: "Microphones" }),
        timeout_ms: 10_000,
    }
}

/// Home's reason for a dictation whose mic was disconnected and whose window
/// was gone by the paste.
pub(crate) const NOT_PASTED: &str = "Your microphone was disconnected and its window was gone";

impl Shared {
    /// The microphone `device` (its name) of dictation `lease` was
    /// disconnected (`mic.rs`; on a thread of its own).
    pub(super) fn mic_lost(self: &Arc<Self>, lease: u64, device: &str) {
        let ours = self.mic_lease.load(Ordering::SeqCst) == lease;
        let recording = self.recording.load(Ordering::SeqCst);
        let (samples, heard) = self.mic_tally.read();
        let next = self.next_mic(device);
        match on_lost(recording, ours, test_feed_on(), heard, samples) {
            OnLost::Ignore => {}
            OnLost::Transcribe => {
                tracing::warn!(samples, "Dictation: the microphone was disconnected; transcribing what it heard");
                if let Ok(mut u) = self.unplugged.lock() {
                    *u = Some(Lost { device: device.to_string(), next });
                }
                self.stop_and_transcribe();
                // The paste says it (`tell_unplugged`); anything else, once
                // the transcription is done.
                let shared = Arc::clone(self);
                let spawned = std::thread::Builder::new().name("dictation-mic-lost-told".into()).spawn(move || {
                    let since = Instant::now();
                    while shared.processing.load(Ordering::SeqCst) && since.elapsed() < TELL_WITHIN {
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    shared.tell_unplugged(Outcome::Kept);
                });
                if let Err(e) = spawned {
                    tracing::warn!("Dictation: no thread to say the mic was disconnected ({e})");
                    self.tell_unplugged(Outcome::Kept);
                }
            }
            OnLost::EndEmpty => {
                if !self.end_now() {
                    return;
                }
                tracing::warn!(samples, "Dictation: the microphone was disconnected before it heard anything");
                let short = crate::dictation::mics::short_name(device);
                let _ = self.app.emit(DICTATION_ERROR, format!("{short} was disconnected"));
                let _ = self.app.emit("yap-state", "error");
                crate::notices::alert::show_soon(&self.app, lost_alert(device, next.as_deref(), Outcome::Empty));
            }
        }
    }

    /// The microphone the next dictation uses, `device` gone (Windows may
    /// still list it a moment): its name.
    fn next_mic(&self, device: &str) -> Option<String> {
        let mut present = crate::dictation::mics::present()?;
        present.retain(|e| e.name != device);
        let order = self.config.read().map(|c| c.mic_order.clone()).unwrap_or_default();
        crate::dictation::mics::in_use(&order, &present, &[]).map(|e| e.name)
    }

    /// This dictation ended because its microphone was disconnected: its
    /// paste goes to its own window only.
    pub(super) fn unplugged(&self) -> bool {
        self.unplugged.lock().map(|u| u.is_some()).unwrap_or(false)
    }

    /// Say, once, that this dictation's mic was disconnected and what became
    /// of its words. `false`: it wasn't (or it's been said).
    pub(super) fn tell_unplugged(&self, outcome: Outcome) -> bool {
        let Some(lost) = self.unplugged.lock().ok().and_then(|mut u| u.take()) else {
            return false;
        };
        crate::notices::alert::show_soon(&self.app, lost_alert(&lost.device, lost.next.as_deref(), outcome));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dictation_that_heard_something_is_transcribed_one_that_didnt_just_ends() {
        let second = TARGET_SAMPLE_RATE as u64;
        assert_eq!(on_lost(true, true, false, true, 3 * second), OnLost::Transcribe);
        // Unplugged at the start: a moment of audio, or only silence.
        assert_eq!(on_lost(true, true, false, true, second / 10), OnLost::EndEmpty);
        assert_eq!(on_lost(true, true, false, false, 3 * second), OnLost::EndEmpty);
        assert_eq!(on_lost(true, true, false, false, 0), OnLost::EndEmpty);
    }

    #[test]
    fn a_mic_lost_with_no_dictation_from_it_changes_nothing() {
        // Warm after a dictation, an older dictation's lease, a robot's WAV.
        assert_eq!(on_lost(false, true, false, true, 48_000), OnLost::Ignore);
        assert_eq!(on_lost(true, false, false, true, 48_000), OnLost::Ignore);
        assert_eq!(on_lost(true, true, true, true, 48_000), OnLost::Ignore);
    }

    #[test]
    fn the_alert_names_the_mic_what_became_of_the_words_and_the_next_mic() {
        let usb = Some("Microphone (Fabrikam USB Mic)");
        let a = lost_alert("Headset Microphone (Contoso H2)", usb, Outcome::Pasted);
        assert_eq!(a.title, "Contoso H2 disconnected");
        assert_eq!(a.body, "Yap stopped the dictation and pasted what it heard. The next one uses Fabrikam USB Mic.");
        assert_eq!(a.kind, "micLost");
        let clip = lost_alert("Headset Microphone (Contoso H2)", usb, Outcome::OnClipboard);
        assert_eq!(clip.body, "Yap stopped the dictation. What it heard is on the clipboard: press Ctrl+V. The next one uses Fabrikam USB Mic.");
        let kept = lost_alert("Headset Microphone (Contoso H2)", usb, Outcome::Kept);
        assert!(kept.body.starts_with("Yap stopped the dictation and kept what it heard."));
        let none = lost_alert("Headset Microphone (Contoso H2)", None, Outcome::Empty);
        assert_eq!(none.body, "Yap stopped the dictation before it heard anything. Connect a microphone for the next one.");
    }
}

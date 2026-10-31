//! **A phone as the dictation's microphone** (Yap Link: `link/session.rs`,
//! `link/dictate.rs`). The phone's audio (16 kHz mono, over the network)
//! goes into the recording in this PC's mic's place, between a start and
//! a stop the phone sends, and from there it's a dictation like any other:
//! the window in front captured at the start (`capture_target`), the
//! `processing` guard, the warm engine (`run_stt`), AI cleanup in the
//! style of that app, the dictionary, snippets and smart insert, the paste
//! with its read check, history (marked as the phone's), Undo. Nothing of
//! the pipeline is forked: only where the audio comes from, and that the
//! phone is told what became of it ([`PhoneSink`]).
//!
//! What a phone's recording leaves out: this PC's microphone (never held;
//! its audio dropped while the phone's records), the chimes and quieting
//! other apps, the dead-mic watchdog and the mic tips, Commands by name
//! and "snip" (the words are typed as said), the bar's first-use pointers'
//! hint. This PC's own shortcut, the bar's ✓ and ✕ and the length limit
//! stop or cancel it as they do any recording; the phone hears so.
//!
//! **Dictate to phone** ([`PhoneTarget::Phone`]): the same, but the text
//! goes back to the phone ([`super::Goes::Phone`]): nothing pasted here,
//! kept in history as the phone's (`history::PHONE_APP`).
//!
//! The paste of a phone's dictation goes into its own window or onto the
//! clipboard, never into whatever has the focus now (`own_window_only`):
//! its person may be in another room.

use std::sync::atomic::Ordering;
use std::sync::Arc;

use tauri::Emitter;

use super::{dictation_began, AmpMeter, Delivered, Shared, PHONE_ARMED};

/// Where a phone's dictation goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PhoneTarget {
    /// Pasted into the window in front on this PC ("Dictate to PC").
    Pc,
    /// Back to the phone, which copies it ("Dictate to phone").
    Phone,
}

/// What became of a phone's dictation, for the phone.
#[derive(Debug, Clone)]
pub(crate) enum PhoneNews {
    /// It stopped recording (the phone's stop, or this PC's shortcut, the
    /// bar's ✓, the length limit): being transcribed.
    Transcribing,
    /// Done: what went to the paste, or back to the phone.
    Delivered(Box<Delivered>),
    /// It ended without text: why, in plain words.
    Failed(&'static str),
    /// Cancelled on this PC (the bar's ✕, Esc): kept there for Undo.
    Cancelled,
}

/// The phone a dictation came from (`link/dictate.rs`'s `Dictating`).
pub(crate) trait PhoneSink: Send + Sync {
    /// Tell the phone (an old dictation's news is the sink's to drop).
    fn tell(&self, news: PhoneNews);
    /// The phone called it off after it stopped talking (its Cancel):
    /// nothing is pasted.
    fn called_off(&self) -> bool;
}

/// Why a phone's dictation can't start now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PhoneBusy {
    /// A dictation records on this PC (its own, or another phone's).
    Dictating,
    /// One is still being transcribed or pasted (or an Upload, a Run
    /// again, a transform).
    Finishing,
}

/// The phone dictating now.
pub(super) struct PhoneJob {
    /// Its recording (`rec_session`).
    session: u64,
    sink: Arc<dyn PhoneSink>,
}

/// Words the phone shows when a dictation ends without text.
pub(super) const NOTHING_HEARD: &str = "Yap didn't catch anything. Try again.";
pub(super) const NO_ENGINE: &str = "Yap's speech engine was busy on the PC. Try again in a moment.";
pub(super) const NO_MODEL: &str = "Yap's speech model isn't ready on the PC.";
pub(super) const STT_FAILED: &str = "Yap couldn't transcribe that on the PC. Try again.";
const CRASHED: &str = "Yap hit an error on the PC while typing that. Try again.";

/// **Yap Link's way into the pipeline** for a phone's dictation
/// ([`super::Pipeline::phone_mic`]).
pub(crate) struct PhoneMic(pub(super) Arc<Shared>);

impl PhoneMic {
    /// A dictation records or transcribes now: a phone's can't start.
    pub(crate) fn busy(&self) -> Option<PhoneBusy> {
        busy_now(self.0.recording.load(Ordering::SeqCst), self.0.processing.load(Ordering::SeqCst))
    }

    /// **Start recording** a phone's dictation going to `target`, `sink`
    /// told what becomes of it. Its recording session (for
    /// [`Self::audio`], [`Self::stop`], [`Self::cancel`]), or why not.
    pub(crate) fn start(&self, target: PhoneTarget, sink: Arc<dyn PhoneSink>) -> Result<u64, PhoneBusy> {
        self.0.start_from_phone(target, sink)
    }

    /// The phone's audio (16 kHz mono) for recording `session`; dropped
    /// once that recording is over.
    pub(crate) fn audio(&self, session: u64, samples: &[f32]) {
        let s = &self.0;
        if !s.from_phone.load(Ordering::SeqCst) || !s.recording.load(Ordering::SeqCst) || s.rec_session.load(Ordering::SeqCst) != session {
            return;
        }
        if let Ok(mut amp) = s.amp.lock() {
            s.take_audio(samples, &mut amp);
        }
    }

    /// The phone's stop: recording `session` is transcribed and delivered,
    /// as a second press of the shortcut does. Nothing if it's over.
    pub(crate) fn stop(&self, session: u64) {
        let s = &self.0;
        let _g = s.gesture.lock().unwrap_or_else(|p| p.into_inner());
        if s.from_phone.load(Ordering::SeqCst) && s.recording.load(Ordering::SeqCst) && s.rec_session.load(Ordering::SeqCst) == session {
            s.stop_and_transcribe();
        }
    }

    /// The phone's Cancel (or it left) while recording `session`: cancelled
    /// as the bar's ✕ does — "Dictate to PC" kept a moment for Undo on this
    /// PC. `false`: it isn't recording any more (it's being transcribed:
    /// the sink's `called_off` stops its paste).
    pub(crate) fn cancel(&self, session: u64) -> bool {
        let s = &self.0;
        let _g = s.gesture.lock().unwrap_or_else(|p| p.into_inner());
        if s.from_phone.load(Ordering::SeqCst) && s.recording.load(Ordering::SeqCst) && s.rec_session.load(Ordering::SeqCst) == session {
            s.cancel(true);
            return true;
        }
        false
    }
}

/// Whether a phone's dictation may start: nothing recording, nothing being
/// transcribed. Pure.
pub(crate) fn busy_now(recording: bool, processing: bool) -> Option<PhoneBusy> {
    if recording {
        Some(PhoneBusy::Dictating)
    } else if processing {
        Some(PhoneBusy::Finishing)
    } else {
        None
    }
}

impl Shared {
    /// A phone's recording is under way now (its audio, not this PC's
    /// mic's, goes into it).
    pub(super) fn phone_recording(&self) -> bool {
        self.from_phone.load(Ordering::SeqCst) && self.recording.load(Ordering::SeqCst)
    }

    /// [`PhoneMic::start`]: what `start_recording` does, without this PC's
    /// microphone — the window in front captured (or none: back to the
    /// phone), the recording on, the bar's capsule showing it (and its
    /// waveform, from the phone's audio).
    fn start_from_phone(self: &Arc<Self>, target: PhoneTarget, sink: Arc<dyn PhoneSink>) -> Result<u64, PhoneBusy> {
        let _g = self.gesture.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(busy) = busy_now(self.recording.load(Ordering::SeqCst), self.processing.load(Ordering::SeqCst)) {
            tracing::info!(?busy, "A phone's dictation can't start: a dictation is under way");
            return Err(busy);
        }
        // A new recording, of the phone's: no limit reached, this PC's mic
        // left out of it from its first audio.
        self.capped.store(false, Ordering::SeqCst);
        self.from_phone.store(true, Ordering::SeqCst);
        if let Ok(mut buf) = self.buffer.lock() {
            buf.clear();
            self.mic_tally.reset();
        }
        if let Ok(mut amp) = self.amp.lock() {
            *amp = AmpMeter::default();
        }
        // Where it goes: the window in front (as this PC's own dictation),
        // or back to the phone.
        PHONE_ARMED.store(target == PhoneTarget::Phone, Ordering::SeqCst);
        self.capture_target(false);
        PHONE_ARMED.store(false, Ordering::SeqCst);
        self.mic_dead.store(false, Ordering::SeqCst);
        if let Ok(mut u) = self.unplugged.lock() {
            *u = None;
        }
        let session = self.rec_session.fetch_add(1, Ordering::SeqCst) + 1;
        if let Ok(mut p) = self.phone.lock() {
            *p = Some(PhoneJob { session, sink });
        }
        // A meeting recording keeps this dictation out of its "You" side.
        dictation_began();
        self.recording.store(true, Ordering::SeqCst);
        self.touch_activity();
        let _ = self.app.emit("yap-state", "recording");
        // No chime to wait for: the waveform shows at once.
        let _ = self.app.emit("yap-mic-ready", true);
        self.maybe_start_streaming(session);
        tracing::info!(to = ?target, "Recording started from a phone");
        Ok(session)
    }

    /// The phone dictating now, if any.
    fn phone_sink(&self) -> Option<Arc<dyn PhoneSink>> {
        self.phone.lock().ok().and_then(|p| p.as_ref().map(|j| Arc::clone(&j.sink)))
    }

    /// Taken: the phone has been (or is being) told how it ended.
    fn take_phone(&self) -> Option<PhoneJob> {
        self.phone.lock().ok().and_then(|mut p| p.take())
    }

    /// Stopped (whoever stopped it): the phone hears it's transcribing.
    pub(super) fn phone_transcribing(&self) {
        if let Some(sink) = self.phone_sink() {
            sink.tell(PhoneNews::Transcribing);
        }
    }

    /// Cancelled (the bar's ✕, Esc, the tray's Cancel — or the phone's
    /// own, whose sink drops this): the phone hears so.
    pub(super) fn phone_cancelled(&self) {
        if let Some(job) = self.take_phone() {
            tracing::info!(session = job.session, "A phone's dictation was cancelled");
            job.sink.tell(PhoneNews::Cancelled);
        }
    }

    /// Delivered (pasted, left on the clipboard, back to the phone…): the
    /// phone hears what became of it.
    pub(super) fn phone_delivered(&self, report: Delivered) {
        if let Some(job) = self.take_phone() {
            job.sink.tell(PhoneNews::Delivered(Box::new(report)));
        }
    }

    /// It ended without text (no speech model, the transcription failed).
    pub(super) fn phone_failed(&self, why: &'static str) {
        if let Some(job) = self.take_phone() {
            job.sink.tell(PhoneNews::Failed(why));
        }
    }

    /// The phone called its dictation off after it stopped talking.
    pub(super) fn phone_called_off(&self) -> bool {
        self.from_phone.load(Ordering::SeqCst) && self.phone_sink().is_some_and(|s| s.called_off())
    }
}

/// Held for a dictation's transcription (`run_stt`): a phone's dictation
/// that ended without its phone being told — nothing heard, or a crash —
/// tells it now.
pub(super) struct PhoneEnd(pub(super) Arc<Shared>);

impl Drop for PhoneEnd {
    fn drop(&mut self) {
        if let Some(job) = self.0.take_phone() {
            job.sink.tell(PhoneNews::Failed(if std::thread::panicking() { CRASHED } else { NOTHING_HEARD }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phone_waits_for_a_dictation_under_way() {
        assert_eq!(busy_now(false, false), None);
        assert_eq!(busy_now(true, false), Some(PhoneBusy::Dictating));
        assert_eq!(busy_now(false, true), Some(PhoneBusy::Finishing));
        // Recording beats finishing: it's the one to wait for.
        assert_eq!(busy_now(true, true), Some(PhoneBusy::Dictating));
    }
}

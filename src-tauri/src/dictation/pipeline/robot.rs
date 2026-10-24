//! Robot mode's dictations (`realtest.rs`) and the e2e suite's, debug
//! builds only: without a hotkey, from a transcript or a WAV played in
//! the mic's place.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::Emitter;

use super::busy::{Busy, Job};
use super::rewrite::{Rewritten, LAST_REWRITE};
use super::{
    arm_chat_dictation, dictation_began, dictation_ended, AmpMeter, Delivered, Shared, ASSISTANT_ARMED, LAST_DELIVERED,
    TARGET_SAMPLE_RATE, TEST_FEED,
};

/// Robot mode's dictations (`realtest.rs`, debug builds only): without a
/// hotkey, and from a transcript or a WAV instead of the microphone.
/// Where they paste is up to the guard in `realtest.rs`, as for any
/// dictation in robot mode.
#[cfg(debug_assertions)]
pub(crate) struct Robot(pub(super) Arc<Shared>);

/// How [`Robot::ask_assistant_audio`] presses the assistant shortcut.
#[cfg(debug_assertions)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Press {
    /// Pressed once to start (and again at the end, if it's still on).
    Toggle,
    /// Held down for the whole audio, let go at its end (push-to-talk).
    Hold,
    /// A double-tap: locked hands-free (push-to-talk), pressed again at the
    /// end if it's still on.
    HandsFree,
}

/// What [`Robot::ask_assistant_audio`] saw.
#[cfg(debug_assertions)]
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AskedAloud {
    /// The recording ended before its audio did, no key pressed: a pause
    /// ended it.
    pub ended_by_itself: bool,
    /// How much of the audio was played before it ended (ms).
    pub listened_ms: u64,
}

/// The robot's WAV plays in the mic's place until this drops.
#[cfg(debug_assertions)]
struct FeedOn;

#[cfg(debug_assertions)]
impl FeedOn {
    fn start() -> FeedOn {
        TEST_FEED.store(true, Ordering::SeqCst);
        FeedOn
    }
}

#[cfg(debug_assertions)]
impl Drop for FeedOn {
    fn drop(&mut self) {
        TEST_FEED.store(false, Ordering::SeqCst);
    }
}

#[cfg(debug_assertions)]
impl Robot {
    const BUSY: &'static str = "Yap is busy with another dictation";

    /// The `processing` flag for a robot dictation (`None`: one records or
    /// transcribes already), let go however it ends ([`Busy`]).
    fn take_busy(&self) -> Option<Busy<Shared>> {
        let shared = &self.0;
        (!shared.recording.load(Ordering::SeqCst)).then(|| Busy::take(shared, Job::Dictation)).flatten()
    }

    /// The report of the latest dictation that went to the paste (an Undo's,
    /// say), taken; `None` while none has since this was last asked.
    pub(crate) fn take_delivered(&self) -> Option<Delivered> {
        LAST_DELIVERED.lock().ok().and_then(|mut last| last.take())
    }

    /// `text` as if the STT had just heard it: the window in front becomes
    /// the target and the text around its cursor is read, as at a
    /// recording's start (waiting for that read, as a few seconds of talk
    /// would), then the dictation's own steps from cleanup to the paste.
    pub(crate) async fn dictate_text(&self, text: String) -> Result<Delivered, String> {
        let shared = &self.0;
        let Some(busy) = self.take_busy() else {
            return Err(Self::BUSY.into());
        };
        shared.capture_target(false);
        crate::pasting::selection::wait_for_context(Duration::from_millis(1_500));
        crate::dictionary::screen_terms::wait_for_terms(Duration::from_millis(3_000));
        let _ = shared.app.emit("yap-state", "processing");
        tracing::info!(len = text.len(), "e2e: robot dictation from a transcript");
        let report = shared.deliver(text).await;
        busy.done();
        shared.end_state(report.offer);
        Ok(report)
    }

    /// The rewrite shortcut with `instruction` as what was said: the window
    /// in front becomes the target and its selection is read, as at the
    /// key press — nothing selected right after a paste there, and that
    /// paste (the last dictation) is what's rewritten — then the AI and the
    /// paste. Returns what it did.
    pub(crate) async fn rewrite_text(&self, instruction: String) -> Result<Rewritten, String> {
        let shared = &self.0;
        let Some(busy) = self.take_busy() else {
            return Err(Self::BUSY.into());
        };
        if let Ok(mut r) = LAST_REWRITE.lock() {
            *r = None;
        }
        shared.capture_target(true);
        let _ = shared.app.emit("yap-state", "processing");
        tracing::info!(len = instruction.len(), "e2e: robot rewrite from a transcript");
        shared.run_rewrite(instruction).await;
        shared.edit_mode.store(false, Ordering::SeqCst);
        busy.done();
        Ok(LAST_REWRITE.lock().ok().and_then(|mut r| r.take()).unwrap_or_default())
    }

    /// [`Robot::dictate_text`] as the Chat view's mic would start it: into
    /// the chat box (`yap-chat-dictation`), never pasted. (Test mode's
    /// `dictation::commands::e2e_chat_dictation`.)
    pub(crate) async fn dictate_to_chat(&self, text: String) -> Result<Delivered, String> {
        arm_chat_dictation(true);
        let out = self.dictate_text(text).await;
        arm_chat_dictation(false);
        out
    }

    /// A question for the **Voice Assistant** without a microphone (test
    /// mode's `assistant::e2e_assistant_ask`): what a press of its shortcut
    /// does — the barge-in, the target, "listening" on the capsule for
    /// `listen_ms` (a dictation as the meeting recorder and the speaker see
    /// it) — then `text` as the transcript, from the dictionary on.
    pub(crate) async fn ask_assistant(&self, text: String, listen_ms: u64) -> Result<(), String> {
        let shared = &self.0;
        // As the shortcut: off during meetings and calls.
        if !shared.recording.load(Ordering::SeqCst) && crate::chat::assistant::off_for_meeting(&shared.app) {
            return Err("The Voice Assistant is off during meetings".into());
        }
        let Some(busy) = self.take_busy() else {
            return Err(Self::BUSY.into());
        };
        crate::voice::tts::stop_for_dictation();
        ASSISTANT_ARMED.store(true, Ordering::SeqCst);
        shared.capture_target(false);
        ASSISTANT_ARMED.store(false, Ordering::SeqCst);
        dictation_began();
        let _ = shared.app.emit("yap-state", "recording");
        tokio::time::sleep(Duration::from_millis(listen_ms)).await;
        dictation_ended();
        let _ = shared.app.emit("yap-state", "processing");
        tracing::info!(len = text.len(), "e2e: a question for the Voice Assistant from a transcript");
        let asked = shared.assistant.swap(false, Ordering::SeqCst) && shared.ask_assistant(text.trim());
        busy.done();
        let _ = shared.app.emit("yap-state", "idle");
        if asked {
            Ok(())
        } else {
            crate::chat::assistant::nothing(&shared.app);
            Err("Nothing to ask".into())
        }
    }

    /// A question to the **Voice Assistant** from `samples` (16 kHz mono)
    /// played in the mic's place in real time, its shortcut pressed as
    /// `press` says through the real key path (the recording mode and
    /// push-to-talk's gestures as configured): the start, "speak now", the
    /// live preview, the pause watch, and at the end of the audio the
    /// shortcut again if the recording is still on — then the STT and the
    /// question to Chat, as ever. Test mode's `e2e::e2e_assistant_audio`.
    pub(crate) async fn ask_assistant_audio(&self, samples: Vec<f32>, press: Press) -> Result<AskedAloud, String> {
        let shared = Arc::clone(&self.0);
        if shared.recording.load(Ordering::SeqCst) || shared.processing.load(Ordering::SeqCst) {
            return Err(Self::BUSY.into());
        }
        if crate::chat::assistant::off_for_meeting(&shared.app) {
            return Err("The Voice Assistant is off during meetings".into());
        }
        let _feed = FeedOn::start();
        if let Ok(mut pre) = shared.preroll.lock() {
            pre.clear();
        }
        let key = |pressed: bool| shared.assistant_key(pressed, None);
        key(true);
        if !shared.recording.load(Ordering::SeqCst) {
            return Err("The recording didn't start (see the log)".into());
        }
        let session = shared.rec_session.load(Ordering::SeqCst);
        match press {
            // A toggle's release does nothing; a hold stays down.
            Press::Toggle => key(false),
            Press::Hold => {}
            // A quick tap, then a second press: locked hands-free.
            Press::HandsFree => {
                tokio::time::sleep(Duration::from_millis(80)).await;
                key(false);
                tokio::time::sleep(Duration::from_millis(120)).await;
                key(true);
                tokio::time::sleep(Duration::from_millis(80)).await;
                key(false);
            }
        }
        tracing::info!(secs = samples.len() / TARGET_SAMPLE_RATE as usize, ?press, "e2e: a question for the Voice Assistant from audio");
        let feeder = Arc::clone(&shared);
        let played = tokio::task::spawn_blocking(move || {
            let mut amp = AmpMeter::default();
            let started = Instant::now();
            let chunk = TARGET_SAMPLE_RATE as usize / 10;
            for (i, part) in samples.chunks(chunk).enumerate() {
                let due = Duration::from_millis(100 * i as u64);
                if let Some(wait) = due.checked_sub(started.elapsed()) {
                    std::thread::sleep(wait);
                }
                if !feeder.recording.load(Ordering::SeqCst) || feeder.rec_session.load(Ordering::SeqCst) != session {
                    return 100 * i as u64;
                }
                feeder.take_audio(part, &mut amp);
                if i == 0 {
                    feeder.fire_pending_start();
                }
            }
            (samples.len() / (TARGET_SAMPLE_RATE as usize / 1000)) as u64
        })
        .await
        .map_err(|e| format!("The audio player failed: {e}"))?;
        // Still listening at the end of the audio: the shortcut ends it.
        let ended_by_itself = !(shared.recording.load(Ordering::SeqCst) && shared.rec_session.load(Ordering::SeqCst) == session);
        if !ended_by_itself {
            match press {
                Press::Hold => key(false),
                Press::Toggle | Press::HandsFree => {
                    key(true);
                    key(false);
                }
            }
        }
        let deadline = Instant::now() + Duration::from_secs(120);
        while shared.processing.load(Ordering::SeqCst) || shared.recording.load(Ordering::SeqCst) {
            if Instant::now() > deadline {
                return Err("The question wasn't transcribed within 2 minutes".into());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Ok(AskedAloud { ended_by_itself, listened_ms: played })
    }

    /// `samples` (16 kHz mono) played into the dictation in real time, in
    /// the mic's place, between a real start and a real stop: the target
    /// captured at the start, the live preview, the dead-mic watchdog, the
    /// STT, then the dictation's own steps to the paste. `None`: nothing was
    /// transcribed (a silent "mic", say). `cancel`: the recording is
    /// cancelled instead of stopped, as the bar's ✕ does (kept for Undo).
    pub(crate) async fn dictate_audio(&self, samples: Vec<f32>, cancel: bool) -> Result<Option<Delivered>, String> {
        let shared = Arc::clone(&self.0);
        if shared.recording.load(Ordering::SeqCst) || shared.processing.load(Ordering::SeqCst) {
            return Err(Self::BUSY.into());
        }
        if let Ok(mut last) = LAST_DELIVERED.lock() {
            *last = None;
        }
        let _feed = FeedOn::start();
        // No room audio from before the WAV in the recording.
        if let Ok(mut pre) = shared.preroll.lock() {
            pre.clear();
        }
        tracing::info!(secs = samples.len() / TARGET_SAMPLE_RATE as usize, "e2e: robot dictation from a WAV");
        shared.start_recording(false);
        if !shared.recording.load(Ordering::SeqCst) {
            return Err("The recording didn't start (see the log)".into());
        }
        let session = shared.rec_session.load(Ordering::SeqCst);
        let feeder = Arc::clone(&shared);
        tokio::task::spawn_blocking(move || {
            let mut amp = AmpMeter::default();
            let started = Instant::now();
            // 100 ms of audio every 100 ms, as a mic delivers it.
            for (i, chunk) in samples.chunks(TARGET_SAMPLE_RATE as usize / 10).enumerate() {
                let due = Duration::from_millis(100 * i as u64);
                if let Some(wait) = due.checked_sub(started.elapsed()) {
                    std::thread::sleep(wait);
                }
                if !feeder.recording.load(Ordering::SeqCst) || feeder.rec_session.load(Ordering::SeqCst) != session {
                    break;
                }
                feeder.take_audio(chunk, &mut amp);
                // The WAV is the mic here: its first audio says "speak now".
                if i == 0 {
                    feeder.fire_pending_start();
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        })
        .await
        .map_err(|e| format!("The WAV player failed: {e}"))?;
        // Stop as the hotkey would (or cancel, as the bar's ✕), if it's
        // still this recording.
        if shared.recording.load(Ordering::SeqCst) && shared.rec_session.load(Ordering::SeqCst) == session {
            if cancel {
                shared.cancel(true);
                return Ok(None);
            }
            shared.stop_and_transcribe();
        }
        // The transcription and the paste.
        let deadline = Instant::now() + Duration::from_secs(300);
        while shared.processing.load(Ordering::SeqCst) || shared.recording.load(Ordering::SeqCst) {
            if Instant::now() > deadline {
                return Err("The dictation didn't finish within 5 minutes".into());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Ok(LAST_DELIVERED.lock().ok().and_then(|mut last| last.take()))
    }
}

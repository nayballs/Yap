//! Recording: the shortcuts and push-to-talk's gestures; a recording's
//! start (the window it's for, the mic, the start chime once the mic
//! hears), its dead-mic watchdog and a Voice Assistant question's pause
//! watch; cancel and Undo; and the stop that hands the audio to
//! `run_stt`. The mic's audio comes in here (`mic::Sink`).

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::Emitter;

use crate::dictation::gesture::Action;
use crate::dictation::mic_watch::{DeadMicWatch, MicSilent, MicStep};

use super::busy::{spawn_busy, Busy, Job};
#[cfg(debug_assertions)]
use super::{test_target, TEST_SILENT_MIC};
use super::{
    destination, dictation_began, dictation_ended, test_feed_on, AmpMeter, GestureSlot, Goes, Kept, PauseSlot,
    PendingStart, Shared, ASSISTANT_ARMED, CHAT_ARMED, CHAT_FOCUSED, DICTATION_ERROR, PREROLL_SAMPLES, UNDO_MS,
};

/// A recording that started while the mic was still opening waits this long
/// at most for its first real audio (`mic::Sink::live`) before the start
/// chime plays anyway (a stream that never delivers: the dead-mic watchdog
/// takes it from there).
const LIVE_FALLBACK_MS: u64 = 3_000;
/// The dead-mic watchdog counts from when the stream is running; a stream
/// still being opened after this long is watched anyway.
const OPEN_PATIENCE_MS: u64 = 6_000;

/// The bar's cue that a Voice Assistant question is about to end on a pause
/// (`pause.rs`): `{leftMs}` as it starts running out, `{leftMs: null}` when
/// you speak again.
const PAUSE_CUE: &str = "yap-assistant-pause";
/// How often the pause watch looks at its verdict.
const PAUSE_POLL_MS: u64 = 50;

/// A cancelled recording shorter than this (0.5 s at 16 kHz), or one that
/// never heard a sound, isn't offered back: nothing worth transcribing.
const UNDO_MIN_SAMPLES: usize = 8_000;

/// The shortcut was pressed while the last dictation is still being
/// transcribed: the bar says "Hold on, still transcribing…" (the press is
/// ignored, as ever).
const HOLD_ON: &str = "yap-dictation-busy";

impl Shared {
    /// The stop chime (if it's on), then the recording lets go of the mic
    /// (`mic.rs`: it stays warm a while, or a Bluetooth headset's closes now,
    /// once the chime has played on it) and the other apps' sound comes
    /// back (`mute.rs`), in that order; `token` from `mute::end`.
    fn stop_chime_then_restore(self: &Arc<Self>, token: u64) {
        let shared = Arc::clone(self);
        let lease = self.mic_lease.load(Ordering::SeqCst);
        let restore: crate::dictation::sound::Played = Box::new(move || {
            let closed_bluetooth = shared.mic.release(lease);
            crate::dictation::mute::after_stop_chime_mic(token, closed_bluetooth);
        });
        if self.sound_enabled() {
            crate::dictation::sound::play_stop(self.audio_feedback_volume(), self.output_device().as_deref(), Some(restore));
        } else {
            std::thread::spawn(restore);
        }
    }

    /// The recording is over without a stop chime (cancelled, no mic): the
    /// mic and the other apps' sound go back at once.
    fn release_now(self: &Arc<Self>, token: u64) {
        let shared = Arc::clone(self);
        let lease = self.mic_lease.load(Ordering::SeqCst);
        std::thread::spawn(move || {
            let closed_bluetooth = shared.mic.release(lease);
            crate::dictation::mute::after_stop_chime_mic(token, closed_bluetooth);
        });
    }

    /// The start chime of the recording waiting for its first real audio
    /// (`pending_start`), once — from the mic's first real audio
    /// (`mic::Sink::live`), at once for a warm mic, or after
    /// [`LIVE_FALLBACK_MS`]. Then the other apps go quiet (`mute.rs`) and
    /// the bar's capsule shows the waveform ("speak now",
    /// `yap-mic-ready`).
    pub(super) fn fire_pending_start(&self) {
        let session = self.pending_session.swap(0, Ordering::SeqCst);
        if session == 0 {
            return;
        }
        let Some(start) = self.pending_start.lock().ok().and_then(|mut p| p.take()) else {
            return;
        };
        if start.session != session
            || !self.recording.load(Ordering::SeqCst)
            || self.rec_session.load(Ordering::SeqCst) != session
        {
            return; // that recording is over
        }
        // "Speak now": a Voice Assistant question's pause watch counts
        // speech from here (the chime itself left out, `pause.rs`).
        if let Ok(mut p) = self.pause.lock() {
            if let Some(slot) = p.as_mut().filter(|s| s.session == session) {
                slot.pause.speak_now();
            }
        }
        let token = start.token;
        let quiet: Option<crate::dictation::sound::Played> =
            start.plan.any().then(|| Box::new(move || crate::dictation::mute::after_start_chime(token)) as crate::dictation::sound::Played);
        if self.sound_enabled() {
            crate::dictation::sound::play_start(self.audio_feedback_volume(), self.output_device().as_deref(), quiet);
        } else if let Some(quiet) = quiet {
            std::thread::spawn(quiet);
        }
        let _ = self.app.emit("yap-mic-ready", true);
    }

    /// No start chime after all (the recording ended before its mic was on).
    fn drop_pending_start(&self) {
        self.pending_session.store(0, Ordering::SeqCst);
        if let Ok(mut p) = self.pending_start.lock() {
            *p = None;
        }
    }

    /// The mic couldn't be opened for `lease`: a recording holding it that
    /// has heard nothing ends now, saying why — Windows' privacy switches,
    /// or no microphone at all. (The robot's WAV stands in for a mic.)
    fn mic_failed(self: &Arc<Self>, lease: u64) {
        // A recording the mic already gave audio (a reopen by the dead-mic
        // watchdog that failed: unplugged mid-dictation) carries on with what
        // it has; the watchdog says what's wrong.
        if test_feed_on() {
            return;
        }
        self.mic_failed_lease.store(lease, Ordering::SeqCst);
        if self.mic_lease.load(Ordering::SeqCst) == lease
            && self.recording.load(Ordering::SeqCst)
            && self.mic_tally.read().0 == 0
        {
            self.end_without_mic();
        }
    }

    /// End the recording now with nothing to transcribe (no microphone, or
    /// it was unplugged before it heard anything): the mic and the other
    /// apps' sound go back at once, its audio and edit-mode state go.
    /// `false`: nothing was recording.
    pub(super) fn end_now(self: &Arc<Self>) -> bool {
        if !self.recording.swap(false, Ordering::SeqCst) {
            return false;
        }
        dictation_ended();
        self.drop_pending_start();
        let token = crate::dictation::mute::end();
        self.release_now(token);
        if let Ok(mut buf) = self.buffer.lock() {
            buf.clear();
        }
        if let Ok(mut g) = self.selection.lock() {
            *g = None;
        }
        if let Ok(mut g) = self.rewrite_last.lock() {
            *g = None;
        }
        self.edit_mode.store(false, Ordering::SeqCst);
        crate::dictionary::screen_terms::forget();
        if self.assistant.swap(false, Ordering::SeqCst) {
            crate::chat::assistant::nothing(&self.app);
        }
        true
    }

    /// End the recording that has no microphone (see [`Self::mic_failed`]).
    fn end_without_mic(self: &Arc<Self>) {
        if !self.end_now() {
            return;
        }
        // Windows' privacy switches, if they're why: that's what to fix
        // (unless the recording's start already said so).
        if let Some(blocked) = crate::dictation::mic_privacy::blocked() {
            tracing::warn!(?blocked, "Can't record — Windows' privacy settings block the microphone");
            if !self.mic_dead.swap(true, Ordering::SeqCst) {
                self.say_mic_blocked(blocked);
            }
        } else {
            tracing::warn!("Can't record — no microphone");
            let _ = self
                .app
                .emit(DICTATION_ERROR, "No microphone found — connect one and try again");
        }
        let _ = self.app.emit("yap-state", "error");
    }
    /// Mic audio (16 kHz mono) as it arrives: while recording — or while one
    /// is starting (`gather`) — into the buffer (and the dead-mic watchdog's
    /// tally); while idle, into the short pre-roll ring the next recording
    /// starts with. The waveform's level goes out while recording, or idle
    /// during a mic test (onboarding).
    pub(super) fn take_audio(&self, samples: &[f32], amp: &mut AmpMeter) {
        if !self.recording.load(Ordering::Relaxed) && !self.gather.load(Ordering::Relaxed) {
            if let Ok(mut pr) = self.preroll.lock() {
                pr.extend(samples.iter().copied());
                while pr.len() > PREROLL_SAMPLES {
                    pr.pop_front();
                }
            }
            if !self.mic_test.load(Ordering::Relaxed) {
                return;
            }
        } else {
            // The dead-mic watchdog's tally: how much arrived, and
            // whether any of it wasn't digital silence.
            self.mic_tally.note(samples);
            // The length limit (`cap.rs`): the buffer never grows past it (a
            // stuck key can't fill the memory); a minute before, the bar
            // warns; at it, the recording stops and is transcribed.
            let cap = crate::dictation::cap::current();
            let mut crossed = None;
            if let Ok(mut buf) = self.buffer.lock() {
                let before = buf.len();
                let room = cap.limit_samples().saturating_sub(before);
                buf.extend_from_slice(&samples[..samples.len().min(room)]);
                crossed = crate::dictation::cap::crossed(&cap, before, buf.len());
            }
            match crossed {
                Some(crate::dictation::cap::Crossed::Warn) => {
                    tracing::info!(left_ms = cap.left_ms(), "Dictation: nearing its length limit");
                    // Suspiciously long: is a key stuck down? (Never stops it.)
                    crate::dictation::input_hook::sweep_now("a dictation near its length limit");
                    let _ = self.app.emit(
                        crate::dictation::cap::EVENT,
                        serde_json::json!({ "leftMs": cap.left_ms(), "text": crate::dictation::cap::warning(&cap) }),
                    );
                }
                Some(crate::dictation::cap::Crossed::Stop) => {
                    // Off the mic's thread: the stop plays a chime and
                    // hands the audio on.
                    let session = self.rec_session.load(Ordering::SeqCst);
                    if let Some(shared) = self.me.upgrade() {
                        let _ = std::thread::Builder::new().name("dictation-cap".into()).spawn(move || shared.stop_at_cap(session));
                    }
                }
                None => {}
            }
            // A Voice Assistant question listens for its pause (`pause.rs`).
            if let Ok(mut p) = self.pause.lock() {
                if let Some(slot) = p.as_mut() {
                    slot.pause.audio(samples);
                }
            }
        }
        // The next scrolling-waveform bar (recording, or an active mic
        // test). The frontend shapes (gain/curve) and scrolls it.
        if let Some(peak) = amp.add(samples) {
            let _ = self.app.emit("yap-amp", peak);
        }
    }
    pub(super) fn toggle(self: &Arc<Self>, edit: bool) {
        if self.recording.load(Ordering::SeqCst) {
            self.stop_and_transcribe();
        } else {
            self.start_recording(edit);
        }
    }

    /// Route a hotkey press/release through the configured recording mode.
    ///
    /// - `toggle` mode: act on press only (flip recording on/off). Ignore release.
    /// - `pushToTalk` mode: the gestures of `gesture.rs` — hold to talk
    ///   (release stops + transcribes), a quick tap then a second press locks
    ///   it hands-free until the next press, a lone tap is thrown away with a
    ///   coaching hint.
    ///
    /// `edit` selects edit/rewrite mode (the dictation hotkey passes `false`, the
    /// edit hotkey passes `true`). `at`: when the key moved (`gesture::now_ms`,
    /// stamped by the input hook); `None` = now.
    pub(super) fn on_key(self: &Arc<Self>, pressed: bool, edit: bool, at: Option<u64>) {
        let mut g = self.gesture.lock().unwrap_or_else(|p| p.into_inner());
        let now = at.unwrap_or_else(crate::dictation::gesture::now_ms);
        if self.recording_mode() != "pushToTalk" {
            g.machine.reset();
            drop(g);
            if pressed {
                let was = self.recording.load(Ordering::SeqCst);
                self.toggle(edit);
                // The press that started this recording (a chord joining it
                // within a second drops it: `key_chord`).
                let started = !was && self.recording.load(Ordering::SeqCst);
                let session = self.rec_session.load(Ordering::SeqCst);
                self.gesture.lock().unwrap_or_else(|p| p.into_inner()).toggle_press = started.then_some((now, session));
            }
            return;
        }
        let triple = self.config.read().map(|c| c.triple_tap_meeting).unwrap_or(false);
        g.machine.set_triple_tap(triple);
        let recording = self.recording.load(Ordering::SeqCst);
        let step = if pressed {
            g.machine.press(now, recording)
        } else {
            g.machine.release(now, recording)
        };
        self.apply_gesture(&mut g, step, edit);
    }

    /// The Voice Assistant's shortcut moved ([`Pipeline::on_assistant_key`]).
    pub(super) fn assistant_key(self: &Arc<Self>, pressed: bool, at: Option<u64>) {
        // Off during meetings and calls: a press doesn't start listening (a
        // press while something records still stops it, below).
        if pressed && !self.recording.load(Ordering::SeqCst) && crate::chat::assistant::off_for_meeting(&self.app) {
            return;
        }
        ASSISTANT_ARMED.store(pressed, Ordering::SeqCst);
        self.on_key(pressed, false, at);
        // Taken by the recording if one started; never left for another.
        ASSISTANT_ARMED.store(false, Ordering::SeqCst);
    }

    /// The tap timer: a tap whose second press didn't come is thrown away.
    fn gesture_tick(self: &Arc<Self>) {
        let mut g = self.gesture.lock().unwrap_or_else(|p| p.into_inner());
        let step = g.machine.tick(crate::dictation::gesture::now_ms(), self.recording.load(Ordering::SeqCst));
        let edit = g.edit;
        self.apply_gesture(&mut g, step, edit);
    }

    /// Another key went down at `at` while a hold-to-talk key was held,
    /// under a second after its press (the input hook): the person meant a
    /// shortcut that shares the key (AltGr+E, Ctrl+Win+D). A recording that
    /// press started is thrown away silently — nothing transcribed or
    /// pasted, no stop chime, no Undo — with the "hold only {key}" tip.
    /// Push-to-talk: the gesture decides (`gesture::chord`); toggle mode:
    /// the press that started this recording, under a second before.
    pub(super) fn key_chord(self: &Arc<Self>, at: u64) {
        let mut g = self.gesture.lock().unwrap_or_else(|p| p.into_inner());
        let recording = self.recording.load(Ordering::SeqCst);
        if self.recording_mode() == "pushToTalk" {
            let step = g.machine.chord(at, recording);
            let edit = g.edit;
            self.apply_gesture(&mut g, step, edit);
            return;
        }
        let session = self.rec_session.load(Ordering::SeqCst);
        let ours = g
            .toggle_press
            .is_some_and(|(pressed, s)| s == session && at.saturating_sub(pressed) < crate::dictation::gesture::CHORD_MS);
        if !(recording && ours) {
            return;
        }
        g.toggle_press = None;
        let quiet = self.edit_mode.load(Ordering::SeqCst) || self.assistant.load(Ordering::SeqCst);
        tracing::info!("Toggle: the press grew into a chord within a second, so it was a shortcut: dropped");
        self.cancel(false);
        if !quiet {
            self.offer_hint(crate::dictation::gesture::Hint::ChordPress);
        }
    }

    /// The input hook's stuck-key sweep let go of the key holding a
    /// push-to-talk recording (its key-up was lost: the lock screen, a UAC
    /// prompt, sleep): the recording goes on **hands-free** — never stopped
    /// here — and the bar shows it can be stopped (`yap-dictation-locked`).
    pub(super) fn key_lost(self: &Arc<Self>) {
        let mut g = self.gesture.lock().unwrap_or_else(|p| p.into_inner());
        if self.recording_mode() != "pushToTalk" {
            return;
        }
        let step = g.machine.key_lost(self.recording.load(Ordering::SeqCst));
        let edit = g.edit;
        self.apply_gesture(&mut g, step, edit);
    }

    /// A coaching hint about the dictation hotkey, off the locks (showing it
    /// asks the main thread whether the window is on screen).
    fn offer_hint(&self, hint: crate::dictation::gesture::Hint) {
        let hotkey = self.config.read().map(|c| c.hotkey.clone()).unwrap_or_default();
        let app = self.app.clone();
        std::thread::spawn(move || crate::dictation::coaching::offer(&app, hint, &hotkey));
    }

    /// Do what a gesture step says (with the gesture lock held, so a key
    /// event and the tap timer never act at once). `edit`: the hotkey that
    /// starts a recording here.
    fn apply_gesture(self: &Arc<Self>, g: &mut GestureSlot, step: crate::dictation::gesture::Step, edit: bool) {
        match step.action {
            Action::Start => {
                g.edit = edit;
                g.assistant = ASSISTANT_ARMED.load(Ordering::SeqCst);
                // Held: the bar's capsule shows no ✕ / ✓ until it locks
                // hands-free (`yap-dictation-locked`) — the tap's wait for a
                // second press included, so nothing flashes. Said before the
                // recording starts, so its first frame already knows; the
                // page forgets it with the recording.
                let _ = self.app.emit("yap-dictation-held", true);
                self.start_recording(edit);
                // Refused (still transcribing the last one, no mic): nothing
                // to hold, tap or lock.
                if !self.recording.load(Ordering::SeqCst) {
                    g.machine.reset();
                } else {
                    self.ask_sweep_after_a_long_hold();
                }
            }
            Action::Stop => self.stop_and_transcribe(),
            Action::Discard if step.hint == Some(crate::dictation::gesture::Hint::ChordHold) => {
                tracing::info!("Push-to-talk: the press grew into a chord within a second, so it was a shortcut: dropped");
                self.cancel(false);
            }
            Action::Discard => {
                tracing::info!("Push-to-talk: a lone tap, nothing to transcribe");
                self.cancel(false);
            }
            Action::Lock => {
                tracing::info!("Push-to-talk: double-tap, recording hands-free");
                let _ = self.app.emit("yap-dictation-locked", true);
                std::thread::spawn(crate::dictation::coaching::hands_free_used);
            }
            Action::Unstick => {
                tracing::info!("Push-to-talk: the key holding the recording was stuck down; it goes on hands-free");
                let _ = self.app.emit("yap-dictation-locked", true);
            }
            Action::TripleTap => {
                // A meeting being recorded already: the press stops the
                // dictation as ever (never the meeting).
                if crate::meetings::meeting::is_recording() {
                    tracing::info!("Push-to-talk: a triple-tap during a meeting stops the dictation");
                    self.stop_and_transcribe();
                } else {
                    tracing::info!("Push-to-talk: triple-tap, meeting notes instead of a dictation");
                    self.cancel(false);
                    let app = self.app.clone();
                    std::thread::spawn(move || crate::calls::meeting_guard::start_or_stop(&app));
                }
            }
            Action::None => {}
        }
        if let Some(ms) = step.tick_in {
            let shared = Arc::clone(self);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(ms));
                shared.gesture_tick();
            });
        }
        // Coaching is about the dictation hotkey.
        if let (Some(hint), false, false) = (step.hint, g.edit, g.assistant) {
            self.offer_hint(hint);
        }
    }

    /// "Long dictation? Try meeting notes" (`coaching.rs`, once ever), naming
    /// the meeting shortcut — once this dictation is done (transcribed and
    /// pasted; a tip never lands mid-dictation), within a minute.
    fn suggest_meeting_notes(&self) {
        let app = self.app.clone();
        let hotkey = self.config.read().map(|c| c.meeting_hotkey.clone()).unwrap_or_default();
        let _ = std::thread::Builder::new().name("dictation-meeting-tip".into()).spawn(move || {
            let started = Instant::now();
            std::thread::sleep(Duration::from_millis(500));
            while crate::notices::notice::moment().dictating && started.elapsed() < Duration::from_secs(60) {
                std::thread::sleep(Duration::from_millis(250));
            }
            tracing::info!("Dictation: a long one; meeting notes suggested if it's due");
            crate::dictation::coaching::offer(&app, crate::dictation::gesture::Hint::LongDictation, &hotkey);
        });
    }

    /// A push-to-talk hold of a minute is suspiciously long: the input hook
    /// checks its keys then (`input_hook::sweep_now`; it never stops the
    /// recording — a stuck one goes on hands-free).
    fn ask_sweep_after_a_long_hold(self: &Arc<Self>) {
        let shared = Arc::clone(self);
        let session = self.rec_session.load(Ordering::SeqCst);
        let _ = std::thread::Builder::new().name("dictation-long-hold".into()).spawn(move || {
            std::thread::sleep(Duration::from_millis(crate::dictation::gesture::LONG_HOLD_MS));
            if shared.recording.load(Ordering::SeqCst) && shared.rec_session.load(Ordering::SeqCst) == session {
                crate::dictation::input_hook::sweep_now("a push-to-talk hold of a minute");
            }
        });
    }

    pub(super) fn start_recording(self: &Arc<Self>, edit: bool) {
        // Barge-in: a dictation starting (the hotkey, the chat's mic, the
        // bar) silences Yap's spoken reply at once, before anything records
        // — Yap's own voice must never be the next question. If it was
        // audible just now, the pre-roll (which would hold it) is left out.
        let heard_yap = crate::voice::tts::stop_for_dictation();
        // A previous transcription is still running — ignore the start so we don't
        // spawn an overlapping `run_stt` (which would rebuild a duplicate model).
        // Processing is normally sub-second on a GPU, so this rarely bites.
        if self.processing.load(Ordering::SeqCst) {
            tracing::info!("Ignoring start — a transcription is still processing");
            // "Hold on, still transcribing…" on the bar's capsule, which
            // is showing the last one.
            let _ = self.app.emit(HOLD_ON, ());
            return;
        }
        // A new recording hasn't reached any limit (`cap.rs`).
        self.capped.store(false, Ordering::SeqCst);
        // The mic: still warm from a moment ago (the stream delivers, with
        // the pre-roll), or opened now in the background — the recording
        // doesn't wait for it, it keeps everything the new stream delivers.
        // A mic that can't be opened ends the recording (`mic_failed`).
        let warm = self.mic.is_live();
        let lease = self.mic.hold();
        self.mic_lease.store(lease, Ordering::SeqCst);
        // Seed the buffer with the pre-roll ring (the ~300 ms before the keypress)
        // so a word already in flight isn't clipped — unless Yap was speaking
        // in it, or the mic wasn't running (the ring is old audio then).
        let pre: Vec<f32> = if heard_yap || !warm {
            Vec::new()
        } else {
            self.preroll
                .lock()
                .map(|p| p.iter().copied().collect())
                .unwrap_or_default()
        };
        if !warm {
            if let Ok(mut p) = self.preroll.lock() {
                p.clear();
            }
        }
        if let Ok(mut buf) = self.buffer.lock() {
            buf.clear();
            buf.extend_from_slice(&pre);
            // The dead-mic watchdog's tally starts afresh with each
            // recording; from here on the mic's audio is this recording's.
            self.mic_tally.reset();
            self.gather.store(true, Ordering::SeqCst);
        }
        self.capture_target(edit);

        self.mic_dead.store(false, Ordering::SeqCst);
        if let Ok(mut u) = self.unplugged.lock() {
            *u = None;
        }
        let session = self.rec_session.fetch_add(1, Ordering::SeqCst) + 1;

        // A meeting recording keeps this dictation out of its "You" side.
        dictation_began();
        self.recording.store(true, Ordering::SeqCst);
        self.gather.store(false, Ordering::SeqCst);
        self.touch_activity();
        let _ = self.app.emit("yap-state", "recording");
        // The mic couldn't be opened before the recording even started.
        if self.mic_failed_lease.load(Ordering::SeqCst) == lease && self.mic_tally.read().0 == 0 {
            self.end_without_mic();
            return;
        }
        // A question for the Voice Assistant ends on a pause (before the
        // start chime: the watch counts speech from it).
        let pause_watch = self.arm_pause(session);
        // The start chime first (Yap's own sound is never silenced), then
        // the other apps go quiet / pause once it has played (`mute.rs`) —
        // once the mic is on: at once when it's warm, else at its first
        // real audio (`fire_pending_start`), so the chime says "speak now"
        // only when Yap hears you.
        let plan = self.other_apps();
        let token = crate::dictation::mute::begin(plan);
        if let Ok(mut p) = self.pending_start.lock() {
            *p = Some(PendingStart { session, token, plan });
        }
        self.pending_session.store(session, Ordering::SeqCst);
        if self.mic.is_live() {
            self.fire_pending_start();
        } else {
            let _ = self.app.emit("yap-mic-ready", false);
            let shared = Arc::clone(self);
            let _ = std::thread::Builder::new().name("dictation-mic-ready".into()).spawn(move || {
                std::thread::sleep(Duration::from_millis(LIVE_FALLBACK_MS));
                if shared.pending_session.load(Ordering::SeqCst) == session {
                    tracing::info!("Microphone: no real audio yet, starting anyway");
                    shared.fire_pending_start();
                }
            });
        }
        // Windows' privacy switches keep desktop apps off the mic: this
        // recording will hear digital silence. Said now, not after the
        // watchdog's five seconds; the recording carries on (switching it on
        // mid-dictation works), and if it ends without a sound it isn't
        // transcribed, as for a dead mic.
        // (Said once: a mic that then fails to open says no more.)
        if !test_feed_on() {
            if let Some(blocked) = crate::dictation::mic_privacy::blocked() {
                tracing::warn!(?blocked, "Recording, but Windows' privacy settings block the microphone");
                if self.recording.load(Ordering::SeqCst) && !self.mic_dead.swap(true, Ordering::SeqCst) {
                    self.say_mic_blocked(blocked);
                }
            }
        }
        // Live partials: spawn a worker that re-transcribes a bounded sliding
        // window of the buffer while recording (cost independent of recording
        // length — see `partials::PartialSession`).
        self.maybe_start_streaming();
        self.spawn_mic_watch(session);
        if pause_watch {
            self.spawn_pause_watch(session);
        }
        tracing::info!("Recording started");
    }

    /// "Stop listening when I pause" (`pause.rs`): a fresh watch for
    /// recording `session` if it's a Voice Assistant question and the
    /// setting is on, else none. Its clock starts where the buffer is (the
    /// pre-roll), as the live preview's does. `true`: watch it.
    fn arm_pause(&self, session: u64) -> bool {
        let on = self.assistant.load(Ordering::SeqCst)
            && self.config.read().map(|c| c.assistant_stop_on_pause).unwrap_or(false);
        let at = if on { self.buffer.lock().map(|b| b.len() as u64).unwrap_or(0) } else { 0 };
        if let Ok(mut p) = self.pause.lock() {
            *p = on.then(|| PauseSlot { session, pause: crate::dictation::pause::Pause::new(at) });
        }
        on
    }

    /// May a pause end the recording now? Toggle mode, or push-to-talk
    /// locked hands-free — never under a held key (letting go ends it).
    /// `g`: the gesture lock, held.
    fn hands_free(&self, g: &GestureSlot) -> bool {
        self.recording_mode() != "pushToTalk" || g.machine.hands_free()
    }

    /// The pause watch for Voice Assistant recording `session`: ends the
    /// question when `pause.rs` says so, as a press of the shortcut would
    /// (the stop chime, the transcription, the answer), and tells the bar
    /// while a pause runs out. Until the recording ends, however it ends.
    fn spawn_pause_watch(self: &Arc<Self>, session: u64) {
        let shared = Arc::clone(self);
        let spawned = std::thread::Builder::new().name("assistant-pause".into()).spawn(move || {
            use crate::dictation::pause::Verdict;
            let live = || shared.recording.load(Ordering::SeqCst) && shared.rec_session.load(Ordering::SeqCst) == session;
            let mut cue = false;
            loop {
                std::thread::sleep(Duration::from_millis(PAUSE_POLL_MS));
                if !live() {
                    break;
                }
                let verdict = shared
                    .pause
                    .lock()
                    .ok()
                    .and_then(|p| p.as_ref().filter(|s| s.session == session).map(|s| (s.pause.verdict(), s.pause.heard_ms())));
                let Some((verdict, heard_ms)) = verdict else { break };
                // Under the gesture lock, as a key's stop is: a press at
                // this very moment finds it stopped, or stops it itself.
                let g = shared.gesture.lock().unwrap_or_else(|p| p.into_inner());
                let verdict = if shared.hands_free(&g) { verdict } else { Verdict::Listen };
                match verdict {
                    Verdict::End(why) => {
                        if live() {
                            tracing::info!(?why, heard_ms, "Voice Assistant: you paused, so the question ends");
                            shared.stop_and_transcribe();
                        }
                        break;
                    }
                    Verdict::Ending { left_ms } if !cue => {
                        cue = true;
                        let _ = shared.app.emit(PAUSE_CUE, serde_json::json!({ "leftMs": left_ms }));
                    }
                    Verdict::Listen if cue => {
                        cue = false;
                        let _ = shared.app.emit(PAUSE_CUE, serde_json::json!({ "leftMs": null }));
                    }
                    _ => {}
                }
                drop(g);
            }
            if cue {
                let _ = shared.app.emit(PAUSE_CUE, serde_json::json!({ "leftMs": null }));
            }
            if let Ok(mut p) = shared.pause.lock() {
                if p.as_ref().is_some_and(|s| s.session == session) {
                    *p = None;
                }
            }
        });
        if let Err(e) = spawned {
            tracing::warn!("Voice Assistant: no pause watch ({})", e);
        }
    }

    /// The live preview read the recording's first `covered` samples as
    /// `text`: the pause watch's words (`pause.rs`).
    pub(super) fn pause_words(&self, covered: usize, text: &str) {
        if let Ok(mut p) = self.pause.lock() {
            if let Some(slot) = p.as_mut() {
                slot.pause.words(covered as u64, text);
            }
        }
    }

    /// "Windows is blocking microphone access for desktop apps": in the
    /// overlay's line (the dead-mic watchdog's place), and as an alert with
    /// **Open settings** (a toast, a bar card or a Windows notification).
    fn say_mic_blocked(&self, blocked: crate::dictation::mic_privacy::Blocked) {
        let _ = self.app.emit("yap-mic-silent", Some(crate::dictation::mic_privacy::mic_silent()));
        crate::notices::alert::show_soon(&self.app, crate::dictation::mic_privacy::alert(blocked));
    }

    /// Where the dictation starting now goes: the window in front (captured
    /// before the overlay or anything else can take the focus; the paste
    /// goes back into it), what kind of app it is, edit mode's selection,
    /// and — in the background — the text around its cursor.
    pub(super) fn capture_target(self: &Arc<Self>, edit: bool) {
        // A new dictation: a cancelled one waiting for Undo is gone for good.
        if self.kept.lock().map(|mut k| k.take().is_some()).unwrap_or(false) {
            tracing::info!("A new dictation: the cancelled one can't be undone any more");
        }
        // Capture the window the user is dictating into, before the overlay
        // (or anything else) can steal focus. Restored at paste time.
        let hwnd = crate::pasting::text_injector::current_foreground().unwrap_or(0);
        // A test run's stand-in window, when a spec asked for one.
        #[cfg(debug_assertions)]
        let hwnd = test_target().unwrap_or(hwnd);
        self.target_hwnd.store(hwnd, Ordering::Relaxed);
        // A question for the Voice Assistant (its shortcut: answered out
        // loud, nothing pasted), or for the Chat view (its mic, or the hotkey
        // while its box has the focus: the page gets the text).
        let armed = CHAT_ARMED.swap(false, Ordering::SeqCst);
        let goes = destination(
            edit,
            ASSISTANT_ARMED.swap(false, Ordering::SeqCst),
            armed,
            CHAT_FOCUSED.load(Ordering::SeqCst),
            hwnd == 0,
        );
        let assistant = goes == Goes::Assistant;
        // A language shortcut's press: this dictation is in its language
        // (My languages, languages.rs) — not a rewrite or a question.
        crate::dictation::languages::begin_dictation(!edit && !assistant);
        self.to_chat.store(goes == Goes::ChatBox, Ordering::SeqCst);
        self.assistant.store(assistant, Ordering::SeqCst);
        match goes {
            Goes::ChatBox => tracing::info!("Dictation: a question for the Chat view"),
            // Said before the "recording" state, so the capsule's first
            // frame is the assistant's.
            Goes::Assistant => crate::chat::assistant::listening(&self.app),
            Goes::Paste => {}
        }
        // And what kind of app it is (a terminal, a chat app…): its process,
        // class and title only, for fitting the paste.
        let kind = crate::pasting::app_kind::of_window((hwnd != 0).then_some(hwnd));
        if let Ok(mut k) = self.target_kind.lock() {
            *k = kind;
        }
        let (smart_spacing, in_terminal, screen_names) = self
            .config
            .read()
            .map(|c| (c.smart_spacing, kind.with_settings(c.terminal_mode, true).is_terminal(), c.screen_names))
            .unwrap_or((false, false, false));

        // Edit/rewrite mode: grab the current selection NOW, while the target app
        // still has focus (before recording), so the spoken instruction can be
        // applied to it. Empty selection → write mode. In a terminal (terminal
        // mode on) never by Ctrl+C: with nothing selected it interrupts the
        // program running there.
        self.edit_mode.store(edit, Ordering::SeqCst);
        let mut rewrite_last = None;
        if edit {
            let target = if hwnd == 0 { None } else { Some(hwnd) };
            let sel = crate::pasting::selection::capture_selection(target, !in_terminal);
            // Nothing selected right after a paste into this window: the
            // instruction is for that paste — the last dictation — not new
            // text (`as_said.rs`), unless "Rewrite your last dictation" is
            // off (then write mode, as ever).
            if sel.is_none() {
                let on = self.config.read().map(|c| c.rewrite_last_dictation).unwrap_or(true);
                rewrite_last = crate::pasting::as_said::rewrite_target(on, target);
                if let Some(p) = &rewrite_last {
                    tracing::info!(
                        chars = p.text().chars().count(),
                        secs = p.age().as_secs(),
                        "Rewrite shortcut with nothing selected: rewriting the last dictation"
                    );
                }
            }
            if let Ok(mut g) = self.selection.lock() {
                *g = sel;
            }
        }
        if let Ok(mut g) = self.rewrite_last.lock() {
            *g = rewrite_last;
        }
        // "Match the text around the cursor": read it in the background now,
        // for the paste (`text_injector::prepare_dictation`). Not for edit
        // mode, whose rewrite replaces the selection as the AI wrote it, nor
        // in a terminal with terminal mode on: its "field" is the screen —
        // nor for the assistant, which pastes nothing.
        let smart = !edit && !assistant && smart_spacing && !in_terminal;
        crate::pasting::selection::begin_dictation((hwnd != 0).then_some(hwnd), smart);
        // "Spell names the way they're written on screen": the window's
        // visible text, read in the background for its names
        // (`screen_terms.rs`) — for a dictation that's pasted, not a rewrite.
        crate::dictionary::screen_terms::begin_dictation((hwnd != 0).then_some(hwnd), goes == Goes::Paste && !edit && screen_names);
    }

    /// The dead-mic watchdog for recording `session` (`mic_watch.rs`): polls
    /// the callback's tally until the mic proves itself or the recording
    /// ends; reopens the stream once if the start is all digital silence (or
    /// no audio comes at all), and says so if that didn't help.
    fn spawn_mic_watch(self: &Arc<Self>, session: u64) {
        let shared = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("dictation-mic-watch".into())
            .spawn(move || {
                let asked = Instant::now();
                let mut started = Instant::now();
                let mut watch = DeadMicWatch::default();
                loop {
                    std::thread::sleep(Duration::from_millis(crate::dictation::mic_watch::POLL_MS));
                    let live = shared.recording.load(Ordering::SeqCst)
                        && shared.rec_session.load(Ordering::SeqCst) == session;
                    if !live {
                        break;
                    }
                    // The mic opens on demand (`mic.rs`), a Bluetooth
                    // headset taking a second or two: its watch counts from
                    // when the stream runs (a robot's WAV needs none).
                    if !shared.mic.is_open() && !test_feed_on() && asked.elapsed() < Duration::from_millis(OPEN_PATIENCE_MS) {
                        started = Instant::now();
                        continue;
                    }
                    let (samples, heard) = shared.mic_tally.read();
                    match watch.poll(started.elapsed().as_millis() as u64, samples, heard) {
                        MicStep::Wait => {}
                        MicStep::Fine => {
                            // Sound after all, though the privacy switches
                            // said no at the start: take the warning back.
                            if shared.mic_dead.swap(false, Ordering::SeqCst) {
                                let _ = shared.app.emit("yap-mic-silent", None::<MicSilent>);
                            }
                            break;
                        }
                        MicStep::Reopen => {
                            tracing::warn!(samples, "Dictation mic: nothing but silence — reopening it");
                            if let Err(e) = shared.reopen_mic() {
                                tracing::warn!("Dictation mic: couldn't reopen it: {}", e);
                            }
                        }
                        MicStep::Report => {
                            // Already said when the recording started:
                            // Windows' privacy switches block the mic.
                            if shared.mic_dead.swap(true, Ordering::SeqCst) {
                                continue;
                            }
                            match crate::dictation::mic_privacy::blocked() {
                                // Not the mic's fault: Windows' privacy switches.
                                Some(blocked) => {
                                    tracing::warn!(?blocked, "Dictation mic: silent, and Windows' privacy settings block it");
                                    shared.say_mic_blocked(blocked);
                                }
                                None => {
                                    let silent = shared.mic_silent();
                                    tracing::warn!("Dictation mic: {}", silent.message);
                                    // The overlay says so in the waveform's place (the
                                    // recording carries on), and only there.
                                    let _ = shared.app.emit("yap-mic-silent", Some(&silent));
                                    let _ = shared.app.emit(DICTATION_ERROR, silent.message);
                                }
                            }
                        }
                        MicStep::Recovered => {
                            tracing::info!("Dictation mic: sound again");
                            shared.mic_dead.store(false, Ordering::SeqCst);
                            let _ = shared.app.emit("yap-mic-silent", None::<MicSilent>);
                            break;
                        }
                    }
                }
            });
        if let Err(e) = spawned {
            tracing::warn!("Dictation mic: no watchdog ({})", e);
        }
    }

    /// "No sound from <the mic> — check it isn't muted or unplugged", and
    /// the overlay's short form — or, when Windows' privacy switches block
    /// the mic, "Windows is blocking microphone access for desktop apps".
    fn mic_silent(&self) -> MicSilent {
        if crate::dictation::mic_privacy::blocked().is_some() {
            return crate::dictation::mic_privacy::mic_silent();
        }
        let name = self.mic_name.lock().map(|n| n.clone()).unwrap_or_default();
        MicSilent::for_mic(&name)
    }

    /// Recording `session` reached its length limit (`cap.rs`): stopped as
    /// the hotkey's second press would — under the gesture lock, as the
    /// pause watch's stop is — and marked `capped`, so its paste goes back
    /// to its own window or onto the clipboard and an alert offers Recover
    /// text. Nothing said is lost.
    fn stop_at_cap(self: &Arc<Self>, session: u64) {
        let _g = self.gesture.lock().unwrap_or_else(|p| p.into_inner());
        if !self.recording.load(Ordering::SeqCst) || self.rec_session.load(Ordering::SeqCst) != session {
            return;
        }
        tracing::info!(limit_ms = crate::dictation::cap::current().limit_ms, "Dictation: reached its length limit, stopping and transcribing");
        self.capped.store(true, Ordering::SeqCst);
        self.stop_and_transcribe();
    }

    /// Reopen the mic stream on the configured device, mid-recording (the
    /// recording carries on into the new stream). The dead stream stops
    /// first: two running at once would interleave their audio.
    fn reopen_mic(self: &Arc<Self>) -> Result<(), String> {
        self.mic.reopen()
    }
    /// Stop recording without transcribing. `undo`: the person cancelled it
    /// (the bar's ✕, the tray's Cancel), so its audio is kept for
    /// [`UNDO_MS`] — the bar shows "Dictation cancelled · Undo" (`yap-state`
    /// "cancelled") — then dropped; a push-to-talk lone tap is just thrown
    /// away. A recording too short to hold words, or one that never heard a
    /// sound, isn't offered back.
    pub(super) fn cancel(self: &Arc<Self>, undo: bool) {
        if !self.recording.swap(false, Ordering::SeqCst) {
            return;
        }
        dictation_ended();
        self.drop_pending_start();
        // No stop chime for a cancel: the mic and the other apps' sound go
        // back now.
        let token = crate::dictation::mute::end();
        self.release_now(token);
        let audio = self.buffer.lock().map(|mut b| std::mem::take(&mut *b)).unwrap_or_default();
        // The edit-mode selection/state goes with it (into the kept
        // recording, or nowhere), so it can't leak into a later session.
        let selection = self.selection.lock().ok().and_then(|mut g| g.take());
        let rewrite_last = self.rewrite_last.lock().ok().and_then(|mut g| g.take());
        let edit = self.edit_mode.swap(false, Ordering::SeqCst);
        // The names read off the screen are this dictation's only (an Undo
        // reads them again).
        crate::dictionary::screen_terms::forget();
        // A question for the assistant that's called off isn't kept for
        // Undo (Undo pastes; the assistant never does).
        let assistant = self.assistant.swap(false, Ordering::SeqCst);
        if assistant {
            crate::chat::assistant::nothing(&self.app);
        }
        if assistant || !(undo && audio.len() >= UNDO_MIN_SAMPLES && self.mic_tally.heard()) {
            let _ = self.app.emit("yap-state", "idle");
            tracing::info!("Recording cancelled (audio discarded)");
            return;
        }
        let id = self.kept_seq.fetch_add(1, Ordering::SeqCst) + 1;
        let target_hwnd = self.target_hwnd.load(Ordering::Relaxed);
        // Never lost: kept a day too, in Home as "Cancelled" with Transcribe
        // (not a rewrite's spoken instruction, which isn't a dictation).
        let audio_id = if edit { None } else { self.keep_cancelled(&audio, target_hwnd) };
        let kept = Kept {
            id,
            audio,
            target_hwnd,
            target_kind: self.target_kind.lock().map(|k| *k).unwrap_or_default(),
            edit,
            selection,
            rewrite_last,
            audio_id,
        };
        if let Ok(mut k) = self.kept.lock() {
            *k = Some(kept);
        }
        let _ = self.app.emit("yap-dictation-cancelled", serde_json::json!({ "undoMs": UNDO_MS }));
        let _ = self.app.emit("yap-state", "cancelled");
        tracing::info!("Recording cancelled (kept {} s for Undo)", UNDO_MS / 1000);
        let shared = Arc::clone(self);
        let spawned = std::thread::Builder::new().name("dictation-undo".into()).spawn(move || {
            std::thread::sleep(Duration::from_millis(UNDO_MS));
            let expired = shared
                .kept
                .lock()
                .map(|mut k| {
                    let ours = k.as_ref().is_some_and(|k| k.id == id);
                    if ours {
                        *k = None;
                    }
                    ours
                })
                .unwrap_or(false);
            if expired {
                tracing::info!("The cancelled dictation's audio is gone (Undo timed out)");
                let _ = shared.app.emit("yap-state", "idle");
            }
        });
        if spawned.is_err() {
            if let Ok(mut k) = self.kept.lock() {
                *k = None;
            }
            let _ = self.app.emit("yap-state", "idle");
        }
    }

    /// A cancelled dictation's audio kept a day (`recent_audio.rs`) and
    /// listed in history as "Cancelled" — Home's **Transcribe** runs it
    /// again — when recent audio and history are on. Its audio's id.
    fn keep_cancelled(&self, audio: &[f32], target_hwnd: isize) -> Option<String> {
        let (id, model) = {
            let cfg = self.config.read().ok()?;
            (crate::dictation::recent_audio::keep(audio, &cfg)?, cfg.model_size.clone())
        };
        let app_name = crate::pasting::text_injector::app_name_for((target_hwnd != 0).then_some(target_hwnd)).unwrap_or_default();
        crate::dictation::history::record_cancelled(&model, &app_name, &id);
        tracing::info!(id = %id, "A cancelled dictation is in history with its audio (Transcribe)");
        let _ = self.app.emit("yap-history-changed", ());
        // Its WAV is being written on its own thread: done pending once it is.
        let finished = id.clone();
        std::thread::spawn(move || crate::dictation::recent_audio::finished(&finished));
        Some(id)
    }

    /// **Undo** a cancel: the kept recording is transcribed and pasted
    /// exactly as if it had been stopped — into the window it was meant for
    /// (never the one in front now, if that's another), with its own kind of
    /// app, edit mode and selection. The text around that window's cursor
    /// is read afresh: it may have changed since.
    pub(super) fn undo(self: &Arc<Self>) -> Result<(), String> {
        if self.recording.load(Ordering::SeqCst) {
            return Err("A dictation is recording".into());
        }
        let Some(busy) = Busy::take(self, Job::Dictation) else {
            return Err("Yap is busy transcribing — try again in a moment".into());
        };
        let Some(k) = self.kept.lock().ok().and_then(|mut k| k.take()) else {
            busy.done();
            return Err("Nothing to undo: the cancelled dictation is gone".into());
        };
        // Taken back: no longer "Cancelled" in Home (it's transcribed now,
        // as a dictation of its own, its audio kept afresh).
        if let Some(id) = &k.audio_id {
            crate::dictation::history::forget_audio(id);
            crate::dictation::recent_audio::discard(id);
            let _ = self.app.emit("yap-history-changed", ());
        }
        self.target_hwnd.store(k.target_hwnd, Ordering::Relaxed);
        if let Ok(mut kind) = self.target_kind.lock() {
            *kind = k.target_kind;
        }
        self.edit_mode.store(k.edit, Ordering::SeqCst);
        if let Ok(mut g) = self.selection.lock() {
            *g = k.selection;
        }
        if let Ok(mut g) = self.rewrite_last.lock() {
            *g = k.rewrite_last;
        }
        let (smart_spacing, in_terminal, screen_names) = self
            .config
            .read()
            .map(|c| (c.smart_spacing, k.target_kind.with_settings(c.terminal_mode, true).is_terminal(), c.screen_names))
            .unwrap_or((false, false, false));
        let target = (k.target_hwnd != 0).then_some(k.target_hwnd);
        crate::pasting::selection::begin_dictation(target, smart_spacing && !k.edit && !in_terminal);
        crate::dictionary::screen_terms::begin_dictation(target, !k.edit && screen_names);
        let _ = self.app.emit("yap-state", "processing");
        tracing::info!(samples = k.audio.len(), "Undo: transcribing the cancelled dictation for its own window");
        self.undoing.store(true, Ordering::SeqCst);
        let shared = Arc::clone(self);
        spawn_busy(busy, async move {
            // `run_stt` consumes its Arc; keep a second handle for the flag.
            let flag = Arc::clone(&shared);
            shared.run_stt(k.audio).await;
            flag.undoing.store(false, Ordering::SeqCst);
        });
        Ok(())
    }

    pub(super) fn stop_and_transcribe(self: &Arc<Self>) {
        // Stopped once: a second stop racing the first (the shortcut pressed
        // as the Voice Assistant's pause ends it) does nothing.
        if !self.recording.swap(false, Ordering::SeqCst) {
            return;
        }
        dictation_ended();
        // Stopped before the mic was on: no start chime after the stop's.
        self.drop_pending_start();
        // The other apps' sound comes back once the stop chime has played.
        let quiet_token = crate::dictation::mute::end();
        // The dead-mic watchdog reported this recording and it never heard a
        // thing: it's all digital silence, nothing to transcribe (and Whisper
        // can make words up from silence). End on the reason instead.
        if self.mic_dead.load(Ordering::SeqCst) && !self.mic_tally.heard() {
            if let Ok(mut buf) = self.buffer.lock() {
                buf.clear();
            }
            if let Ok(mut g) = self.selection.lock() {
                *g = None;
            }
            if let Ok(mut g) = self.rewrite_last.lock() {
                *g = None;
            }
            self.edit_mode.store(false, Ordering::SeqCst);
            if self.assistant.swap(false, Ordering::SeqCst) {
                crate::chat::assistant::nothing(&self.app);
            }
            tracing::info!("Recording stopped: the mic gave nothing but silence, not transcribed");
            let _ = self.app.emit("yap-mic-silent", Some(self.mic_silent()));
            let _ = self.app.emit("yap-state", "error");
            self.stop_chime_then_restore(quiet_token);
            return;
        }
        let _ = self.app.emit("yap-state", "processing");
        self.stop_chime_then_restore(quiet_token);
        let audio = self
            .buffer
            .lock()
            .map(|mut b| std::mem::take(&mut *b))
            .unwrap_or_default();
        tracing::info!(samples = audio.len(), "Recording stopped, transcribing");
        // A dictation of ten minutes: meeting notes would have gone on past
        // the limit and summarised (a quiet tip, once ever; not for a
        // rewrite's instruction or a question).
        let ms = audio.len() as u64 * 1000 / super::TARGET_SAMPLE_RATE as u64;
        if ms >= crate::dictation::cap::meeting_hint_ms()
            && !self.edit_mode.load(Ordering::SeqCst)
            && !self.assistant.load(Ordering::SeqCst)
        {
            self.suggest_meeting_notes();
        }

        // Mark "processing" so a rapid re-toggle can't start an overlapping
        // transcription. Cleared when the task ends, however it ends (`Busy`).
        let busy = Busy::hold(self, Job::Dictation);
        let shared = Arc::clone(self);
        spawn_busy(busy, async move { shared.run_stt(audio).await });
    }
}

/// The mic's audio and news (`mic.rs`) into the pipeline.
impl crate::dictation::mic::Sink for Shared {
    fn audio(&self, samples: &[f32]) {
        // The e2e suite's "muted mic", and the robot's WAV playing in the
        // mic's place (debug builds only).
        #[cfg(debug_assertions)]
        let zeros;
        #[cfg(debug_assertions)]
        let samples = if TEST_SILENT_MIC.load(Ordering::Relaxed) {
            zeros = vec![0.0; samples.len()];
            &zeros[..]
        } else {
            samples
        };
        if test_feed_on() {
            return;
        }
        if let Ok(mut amp) = self.amp.lock() {
            self.take_audio(samples, &mut amp);
        }
    }

    fn live(&self) {
        self.fire_pending_start();
    }

    fn opened(&self, device: &str) {
        if let Ok(mut name) = self.mic_name.lock() {
            *name = device.to_string();
        }
        // The pre-roll is this stream's from now on, never a minute-old
        // stream's.
        if !self.recording.load(Ordering::SeqCst) && !self.gather.load(Ordering::SeqCst) {
            if let Ok(mut p) = self.preroll.lock() {
                p.clear();
            }
        }
    }

    fn failed(&self, _error: &str, dictation: Option<u64>) {
        // (The mic's thread: `Weak` gave it `&Shared`; the recording's end
        // needs the `Arc`.)
        if let Some(lease) = dictation {
            if let Some(shared) = self.me.upgrade() {
                shared.mic_failed(lease);
            }
        }
    }

    fn lost(&self, device: &str, dictation: Option<u64>) {
        // Off the mic's thread: the dictation's end releases the mic, which
        // waits for that thread (`unplugged.rs`).
        let (Some(shared), Some(lease)) = (self.me.upgrade(), dictation) else { return };
        let device = device.to_string();
        let spawned = std::thread::Builder::new()
            .name("dictation-mic-lost".into())
            .spawn(move || shared.mic_lost(lease, &device));
        if let Err(e) = spawned {
            tracing::warn!("Microphone: no thread to end the dictation on ({e})");
        }
    }
}

//! Other apps' sound while you dictate (Windows): **"Silence other apps
//! while you dictate"** (`config.quiet_other_apps`) and **"Pause music and
//! videos while you dictate"** (`config.pause_media`, `media_pause.rs`).
//!
//! The order, as Wispr Flow does it: the start chime plays first (Yap's own
//! sound is never touched, so it's always heard), and once it has played
//! ([`after_start_chime`], the chime's callback in `sound.rs`) the other
//! apps go quiet; after the dictation the stop chime plays, and once it has
//! played ([`after_stop_chime`]) everything comes back.
//!
//! - **Per app, not the whole device**: each other app's audio session on
//!   every output device is turned down to silence — the sessions, the
//!   rules of what's never touched (Yap's own session, system sounds, a call
//!   app's, the browser a meeting is in) and the exact restore are
//!   `other_apps.rs`'s, shared with Yap's voice turning them down while it
//!   speaks (`voice/duck.rs`). A dictation started while Yap speaks takes
//!   the voice's sessions over at the level they're at ([`takes_over`]): no
//!   jump up before its start sound, and its stop sound brings them back to
//!   their level from before Yap spoke.
//! - **Only what Yap changed comes back, to exactly its level**: a session
//!   whose level moved meanwhile (the app or you changed it) is left alone.
//!   A dictation that starts before the last one's sound came back keeps it
//!   down and brings it all back at its own end; a dictation that ended
//!   before its chime finished never quiets anything.
//! - **Bluetooth**: when the dictation itself put a headset into its call
//!   profile (the default output dropped below 44.1 kHz since the start, or
//!   its Bluetooth mic — opened on demand, `mic.rs` — was just closed), the
//!   restore waits for it to come back, polling every 50 ms for up to 5 s,
//!   plus 500 ms, so the music doesn't return through the call codec
//!   ([`codec_wait`]).
//! - **A crash mid-dictation**: what's down is written to
//!   `<data>/quieted_apps.json` while it is, and the next start
//!   ([`recover`]) brings it back (`other_apps.rs`).
//!
//! Configs before 2026-10-17 had `muteWhileRecording`, which muted the
//! whole output device before the start chime (the chime and any call
//! went silent too); `quiet_other_apps` reads it (`config.rs`).
//!
//! In a test run nothing is touched; the real-app robot touches only the
//! sound of processes it started (`realtest::audio_permits`,
//! `realtest::media_permits`), never the developer's own music.

use std::sync::Mutex;

use crate::dictation::media_pause::{self, Paused};
use crate::dictation::other_apps::{self, Holder};
use crate::shell::realtest::OtherApps;

/// What Yap does to other apps while you dictate (from Settings).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Plan {
    /// "Silence other apps while you dictate".
    pub quiet: bool,
    /// "Pause music and videos while you dictate".
    pub pause: bool,
}

impl Plan {
    pub fn any(self) -> bool {
        self.quiet || self.pause
    }
}

/// Below this rate the default output is in a headset's call profile.
const CALL_PROFILE_RATE: u32 = 44_100;
/// The Bluetooth wait: poll this often, at most this many times, then this
/// much more.
const CODEC_POLL_MS: u64 = 50;
const CODEC_POLLS: u32 = 100;
const CODEC_SETTLE_MS: u64 = 500;

// ---- the rules (pure) --------------------------------------------------------------------

/// A headset's call profile: the default output's rate dropped below CD
/// quality (8/16/32 kHz).
fn in_call_profile(rate: Option<u32>) -> bool {
    rate.is_some_and(|r| r < CALL_PROFILE_RATE)
}

/// Whether the restore should wait for the output to leave its call
/// profile: it went into one during the dictation (a call on, or "Keep the
/// microphone ready" on a headset: it was in one already, no wait) — or the
/// dictation's Bluetooth mic was just closed (`mic.rs` opens it on demand,
/// so the headset dropped to its call profile before the start chime and
/// leaves it now).
fn codec_wait(rate_at_start: Option<u32>, rate_now: Option<u32>, closed_bluetooth: bool) -> bool {
    if closed_bluetooth {
        return in_call_profile(rate_now);
    }
    rate_at_start.is_some() && !in_call_profile(rate_at_start) && in_call_profile(rate_now)
}

// ---- the pipeline's side ------------------------------------------------------------------

#[derive(Default)]
struct State {
    /// Bumped by every [`begin`] and [`end`].
    gen: u64,
    recording: bool,
    plan: Plan,
    /// The default output's rate when Yap quieted things.
    rate_at_start: Option<u32>,
    paused: Vec<Paused>,
    /// The token of the last stop whose restore ran (`after_stop_chime_mic`,
    /// or Yap quitting): media paused after it, still being paused when it
    /// ran, is played again by its own pause ([`late_pause_plays_now`]).
    restored: u64,
}

static STATE: Mutex<State> = Mutex::new(State {
    gen: 0,
    recording: false,
    plan: Plan { quiet: false, pause: false },
    rate_at_start: None,
    paused: Vec::new(),
    restored: 0,
});

/// The dictation's state: never held across COM (the pipeline's start and
/// stop take it, and mustn't wait on it). Turning down and bringing back
/// happen under `other_apps::ledger`, one at a time, taken first. Media is
/// paused and played with neither held (WinRT's waits, bounded in
/// `media_pause.rs`, never hold up the ledger: Yap's voice, the next
/// dictation's sound).
fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}

/// Media paused while its dictation's stop already brought the rest back
/// (a quick tap): nobody is left to play it again but the pause itself.
/// Pure.
fn late_pause_plays_now(s: &State) -> bool {
    !s.recording && s.restored == s.gen
}

/// A dictation starts (`start_recording`): its token, for
/// [`after_start_chime`].
pub fn begin(plan: Plan) -> u64 {
    let mut s = state();
    s.gen += 1;
    s.recording = true;
    s.plan = plan;
    s.gen
}

/// Whether a dictation recording now will silence the other apps: Yap's
/// voice letting go then hands its sessions to it instead of bringing them
/// up under it (`voice/duck.rs`, `other_apps::Ledger::hand_over`).
pub fn takes_over() -> bool {
    let s = state();
    s.recording && s.plan.quiet
}

/// The start chime has played (or there is none): quiet and pause the
/// other apps for dictation `token`, if it's still the one recording.
pub fn after_start_chime(token: u64) {
    let who = {
        let mut ledger = other_apps::ledger();
        let plan = {
            let s = state();
            if s.gen != token || !s.recording {
                return; // over before its chime was
            }
            s.plan
        };
        let who = crate::shell::realtest::other_apps();
        if !plan.any() {
            return;
        }
        if who == OtherApps::Nobody {
            tracing::info!("other apps: left alone in a test run");
            return;
        }
        let first = !ledger.holds(Holder::Dictation);
        let rate = other_apps::default_output_rate();
        if plan.quiet {
            other_apps::lower(&mut ledger, Holder::Dictation);
        }
        if first {
            state().rate_at_start = rate;
        }
        if !plan.pause {
            return;
        }
        who
    };
    // The media, with no lock held (its waits are WinRT's).
    let calls = other_apps::calls_now();
    let keep = media_pause::Keep {
        call_names: calls.names.clone(),
        browsers: calls.meeting || !calls.live_browsers.is_empty(),
    };
    let robot: Option<&dyn Fn(&str) -> bool> = match who {
        OtherApps::TestsOwn => Some(&crate::shell::realtest::media_permits),
        _ => None,
    };
    let paused = media_pause::pause(&keep, robot);
    let late = {
        let mut s = state();
        s.paused.extend(paused);
        if late_pause_plays_now(&s) {
            std::mem::take(&mut s.paused)
        } else {
            Vec::new()
        }
    };
    if !late.is_empty() {
        tracing::info!(n = late.len(), "other apps: the dictation was over by the time its media paused; playing it again");
        media_pause::resume(late);
    }
}

/// The dictation ended (stopped or cancelled): its token, for
/// [`after_stop_chime`].
pub fn end() -> u64 {
    let mut s = state();
    s.gen += 1;
    s.recording = false;
    s.gen
}

/// The stop chime has played (or there is none): bring back what Yap
/// quieted and paused, unless another dictation began meanwhile (it keeps
/// them down and brings them back at its end). Waits for a Bluetooth output
/// to leave the call profile first, if the dictation put it there.
#[cfg(test)]
pub fn after_stop_chime(token: u64) {
    after_stop_chime_mic(token, false)
}

/// [`after_stop_chime`], told whether the dictation's Bluetooth mic was
/// just closed (`mic::Mic::release`): its headset is leaving the call
/// profile, which the restore waits for.
pub fn after_stop_chime_mic(token: u64, closed_bluetooth: bool) {
    let rate_at_start = {
        // Once a quieting that's under way has finished (it's in the ledger).
        let ledger = other_apps::ledger();
        let mut s = state();
        if !ledger.holds(Holder::Dictation) && s.paused.is_empty() {
            // Nothing to bring back: media still being paused plays again
            // by itself once it is (`after_start_chime`).
            if s.gen == token {
                s.restored = token;
            }
            return;
        }
        s.rate_at_start
    };
    if codec_wait(rate_at_start, other_apps::default_output_rate(), closed_bluetooth) {
        tracing::info!("other apps: waiting for the headset to leave its call profile");
        for _ in 0..CODEC_POLLS {
            std::thread::sleep(std::time::Duration::from_millis(CODEC_POLL_MS));
            if !in_call_profile(other_apps::default_output_rate()) {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(CODEC_SETTLE_MS));
    }
    let paused = {
        let mut ledger = other_apps::ledger();
        let paused = {
            let mut s = state();
            if s.gen != token {
                return; // a new dictation keeps them down
            }
            s.rate_at_start = None;
            s.restored = token;
            std::mem::take(&mut s.paused)
        };
        other_apps::release(&mut ledger, Holder::Dictation);
        paused
    };
    // The media after the ledger is let go (WinRT's waits).
    media_pause::resume(paused);
}

/// Yap is quitting (`shutdown_cleanup`): everything back now, no waiting —
/// a dictation's and Yap's voice's alike (`other_apps::restore_all`).
pub fn restore_all() {
    let paused = {
        let mut ledger = other_apps::ledger();
        let paused = {
            let mut s = state();
            s.gen += 1;
            s.recording = false;
            s.restored = s.gen;
            std::mem::take(&mut s.paused)
        };
        other_apps::restore_all(&mut ledger);
        paused
    };
    media_pause::resume(paused);
}

/// At start: what a crash left quiet comes back (off the main thread).
pub fn recover() {
    other_apps::recover();
}

/// The robot's view (debug builds): the test's own sessions and media as
/// Windows has them now, and what Yap holds down.
#[cfg(debug_assertions)]
pub fn robot_view() -> serde_json::Value {
    let (sessions, held) = other_apps::robot_view(&crate::shell::realtest::audio_permits);
    // Asked before the state is taken: never held across WinRT.
    let media = media_list();
    let s = state();
    serde_json::json!({
        "sessions": sessions,
        "media": media,
        "quieted": held,
        "paused": s.paused.iter().map(|p| p.app.clone()).collect::<Vec<_>>(),
        "recording": s.recording,
    })
}

#[cfg(all(debug_assertions, windows))]
fn media_list() -> Vec<serde_json::Value> {
    media_pause::list(&crate::shell::realtest::media_permits)
}

#[cfg(all(debug_assertions, not(windows)))]
fn media_list() -> Vec<serde_json::Value> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bluetooth_wait_is_only_for_a_profile_the_dictation_caused() {
        // A headset dropped to its call profile during the dictation.
        assert!(codec_wait(Some(48_000), Some(16_000), false));
        // Already in it at the start (a call, a mic kept ready): no wait.
        assert!(!codec_wait(Some(16_000), Some(16_000), false));
        // Back to normal, or a normal device all along.
        assert!(!codec_wait(Some(48_000), Some(48_000), false));
        assert!(!codec_wait(Some(44_100), Some(44_100), false));
        assert!(!codec_wait(None, None, false));
        // The start unknown: no waiting on a guess.
        assert!(!codec_wait(None, Some(16_000), false));
        // The dictation's own Bluetooth mic was just closed (opened on
        // demand: the headset was in its call profile before the chime):
        // wait while it's still there, not once it has left.
        assert!(codec_wait(Some(16_000), Some(16_000), true));
        assert!(codec_wait(None, Some(16_000), true));
        assert!(!codec_wait(Some(16_000), Some(48_000), true));
        assert!(in_call_profile(Some(8_000)) && in_call_profile(Some(32_000)));
    }

    #[test]
    fn a_dictation_over_before_its_chime_quiets_nothing_and_a_new_one_keeps_them_down() {
        // (The token logic alone: no session is touched in `cargo test`'s
        // plan with nothing to do.)
        let a = begin(Plan::default());
        let ended = end();
        assert_ne!(a, ended);
        after_start_chime(a); // over: nothing
        let b = begin(Plan { quiet: true, pause: false });
        assert!(state().recording);
        // Recording with "Silence other apps" on: Yap's voice hands over.
        assert!(takes_over());
        after_stop_chime(ended); // a new dictation began: nothing restored
        assert_eq!(state().gen, b);
        end();
        assert!(!state().recording);
        assert!(!takes_over());
    }

    /// Media is paused with no lock held now, so a stop can come while it's
    /// being paused: what's paused after the stop's restore ran plays again
    /// at once; while recording, or before that restore, it waits for it;
    /// a new dictation keeps it for its own end.
    #[test]
    fn media_paused_after_its_dictation_ended_plays_again_by_itself() {
        let s = |recording: bool, gen: u64, restored: u64| State { gen, recording, restored, ..State::default() };
        // Still recording: the stop brings it back.
        assert!(!late_pause_plays_now(&s(true, 3, 0)));
        // Stopped, its stop chime still playing: the restore takes it.
        assert!(!late_pause_plays_now(&s(false, 4, 2)));
        // Stopped and restored already (a tap): played again by the pause.
        assert!(late_pause_plays_now(&s(false, 4, 4)));
        // A new dictation since: kept down for its end.
        assert!(!late_pause_plays_now(&s(true, 5, 4)));
    }
}

//! **Other apps turned down while Yap speaks** (Settings → Voice Assistant →
//! Voice: "Turn other apps down while Yap speaks", `config.duck_other_apps`,
//! on by default). While Yap's voice plays on this PC — spoken replies, the
//! Voice Assistant, Read aloud, ▶ — every other app's audio session goes
//! down to about 30 % of its own level, and back to exactly that level
//! after.
//!
//! - **Only this PC's speakers**: the output `tts/playback.rs`'s rodio
//!   opener makes holds a [`Duck`]; Yap Link's phone playback
//!   (`link/voice.rs`) never does, so an answer said on a phone turns
//!   nothing down here.
//! - **From the first sound, while it plays**: a [`Duck`] takes hold with
//!   the first audio handed to the output (a streamed answer waiting on its
//!   first sentence has nothing to hear yet), lets go on a pause (⏸ on the
//!   reading card) and takes hold again on ⏯, and lets go for good when the
//!   output closes.
//! - **Let go [`LET_GO_AFTER`] after the voice ends**: answers in a row, or
//!   a new one replacing one, don't bounce the music up and down; and a
//!   dictation started mid-speech (`stop_for_dictation`, then
//!   `mute::begin`) finds the sessions still down. With "Silence other apps
//!   while you dictate" on, the dictation takes them over at their 30 %
//!   (`mute::takes_over` → `Ledger::hand_over`), silences them after its
//!   start sound and brings them back to their own level after its stop
//!   sound; with it off they come back as the voice lets go.
//! - **The same sessions, rules and restore as a dictation's**
//!   (`dictation/other_apps.rs`): never Yap's own session, a call app's or
//!   system sounds; only a session still at Yap's level comes back; what's
//!   down is in `quieted_apps.json` while it is, so a crash's leftovers come
//!   back at the next start; quitting brings everything back
//!   (`mute::restore_all`).
//!
//! In a test run nothing is touched (`realtest::other_apps`).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use crate::dictation::other_apps::{self, Holder};

/// How long after the voice ends the other apps come back.
pub const LET_GO_AFTER: Duration = Duration::from_millis(600);

/// "Turn other apps down while Yap speaks" (from every config save, [`sync`]).
static ON: AtomicBool = AtomicBool::new(true);
/// Outputs playing Yap's voice now ([`Duck`]s holding).
static HOLDING: AtomicUsize = AtomicUsize::new(0);
/// The voice has turned the other apps down for this stretch of speech
/// (read and written with `other_apps::ledger` held): turned down once,
/// so a level the person moves meanwhile stays where they put it.
static DOWN: AtomicBool = AtomicBool::new(false);

/// The setting, from a config save (`shell/settings.rs`) and at start
/// (`tts::init`): switched off mid-answer, the other apps come back now;
/// on, they go down if Yap is speaking.
pub fn sync(cfg: &crate::config::YapConfig) {
    let was = ON.swap(cfg.duck_other_apps, Ordering::SeqCst);
    if was != cfg.duck_other_apps {
        kick(Duration::ZERO);
    }
}

/// One output's hold on the other apps (the PC's speakers, `playback.rs`):
/// [`Duck::hold`] when its audio plays, [`Duck::let_go`] when it pauses or
/// stops; dropped, it lets go.
#[derive(Default)]
pub struct Duck {
    held: AtomicBool,
}

impl Duck {
    pub fn new() -> Duck {
        Duck::default()
    }

    /// Yap's voice is playing on this output.
    pub fn hold(&self) {
        if !self.held.swap(true, Ordering::SeqCst) && HOLDING.fetch_add(1, Ordering::SeqCst) == 0 {
            kick(Duration::ZERO);
        }
    }

    /// It's paused, stopped or done.
    pub fn let_go(&self) {
        if self.held.swap(false, Ordering::SeqCst) && HOLDING.fetch_sub(1, Ordering::SeqCst) == 1 {
            kick(LET_GO_AFTER);
        }
    }
}

impl Drop for Duck {
    fn drop(&mut self) {
        self.let_go();
    }
}

/// Bring the other apps to where they should be, `after` from now, on a
/// thread of its own (the COM work never runs on the speaker's).
fn kick(after: Duration) {
    // A unit test never touches the developer's own music.
    if cfg!(test) || crate::shell::realtest::other_apps() == crate::shell::realtest::OtherApps::Nobody {
        return;
    }
    let _ = std::thread::Builder::new().name("yap-duck".into()).spawn(move || {
        if !after.is_zero() {
            std::thread::sleep(after);
        }
        settle();
    });
}

/// Down while Yap speaks (and the setting is on), else up: one at a time,
/// under the ledger's lock, so any order of kicks ends right.
fn settle() {
    let mut ledger = other_apps::ledger();
    match next_step(ON.load(Ordering::SeqCst), HOLDING.load(Ordering::SeqCst), DOWN.load(Ordering::SeqCst), ledger.holds(Holder::Voice)) {
        Step::Lower => {
            let n = other_apps::lower(&mut ledger, Holder::Voice);
            tracing::info!(count = n, "Spoken replies: other apps turned down while Yap speaks");
            DOWN.store(true, Ordering::SeqCst);
        }
        Step::LetGo => {
            if crate::dictation::mute::takes_over() {
                // A dictation is silencing them: at the level they're at
                // until its start sound, never back up under it.
                ledger.hand_over(Holder::Voice, Holder::Dictation);
                tracing::info!("Spoken replies: a dictation takes the other apps over");
            } else {
                other_apps::release(&mut ledger, Holder::Voice);
            }
            DOWN.store(false, Ordering::SeqCst);
        }
        Step::Stay => {}
    }
}

/// What [`settle`] does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Lower,
    LetGo,
    Stay,
}

/// The next step, from the setting, how many outputs play Yap's voice,
/// whether this stretch has turned them down, and whether the ledger still
/// holds any for the voice. Pure.
fn next_step(on: bool, holding: usize, down: bool, held: bool) -> Step {
    if on && holding > 0 {
        if down {
            Step::Stay
        } else {
            Step::Lower
        }
    } else if down || held {
        Step::LetGo
    } else {
        Step::Stay
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn down_once_while_yap_speaks_and_up_once_it_stops_or_the_setting_goes_off() {
        // Speaking, not down yet: down.
        assert_eq!(next_step(true, 1, false, false), Step::Lower);
        // Down already (a level moved since stays where the person put it).
        assert_eq!(next_step(true, 2, true, true), Step::Stay);
        // Nothing playing anymore: up.
        assert_eq!(next_step(true, 0, true, true), Step::LetGo);
        // The setting off mid-answer: up at once.
        assert_eq!(next_step(false, 1, true, true), Step::LetGo);
        // Off, nothing down: nothing to do.
        assert_eq!(next_step(false, 1, false, false), Step::Stay);
        assert_eq!(next_step(true, 0, false, false), Step::Stay);
        // Handed over to a dictation meanwhile: let go of nothing, but done.
        assert_eq!(next_step(true, 0, true, false), Step::LetGo);
    }

    #[test]
    fn a_duck_holds_once_and_lets_go_once() {
        // (`cargo test` never touches a session: `kick` does nothing in
        // one, so this checks the count alone.)
        let before = HOLDING.load(Ordering::SeqCst);
        let d = Duck::new();
        d.hold();
        d.hold();
        assert_eq!(HOLDING.load(Ordering::SeqCst), before + 1);
        d.let_go();
        d.let_go();
        assert_eq!(HOLDING.load(Ordering::SeqCst), before);
        d.hold();
        drop(d);
        assert_eq!(HOLDING.load(Ordering::SeqCst), before);
    }
}

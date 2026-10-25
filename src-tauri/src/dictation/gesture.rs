//! Push-to-talk gestures on the dictation hotkey (Wispr Flow's, by
//! behaviour — `references/wispr-flow/rea-teardown/04-dictation-and-other.md`):
//!
//! - **hold** the key to talk; letting go stops and transcribes;
//! - a **quick tap** (let go within [`TAP_MAX_MS`]) followed by a **second
//!   press** within [`DOUBLE_TAP_GAP_MS`] locks the recording **hands-free**:
//!   it carries on after the key comes up, until the next press stops it;
//! - a **lone tap** (no second press in time) is thrown away — a fraction of
//!   a second of audio is never a dictation — and earns a coaching hint
//!   ([`Hint::Tap`]); holding the key a minute or more earns the
//!   "double-tap is hands-free" hint ([`Hint::LongHold`]);
//! - a hold **under [`CHORD_MS`] that grows into a chord** (another key goes
//!   down while it's held: the person meant a shortcut that shares the key,
//!   AltGr+E, Ctrl+Win+D…) is thrown away silently ([`Gesture::chord`]),
//!   with the "hold only {key}" hint ([`Hint::ChordHold`]);
//! - a key the input hook finds **stuck "down"** (its key-up was lost to the
//!   lock screen, a UAC prompt, sleep: `input_hook.rs`'s sweep) leaves its
//!   recording going **hands-free** ([`Gesture::key_lost`]): never stopped
//!   or started by the sweep, the next press stops it;
//! - optionally (`triple_tap_meeting`, off), a **triple-tap** — a
//!   double-tap's second press let go quickly, and a third press soon after
//!   — throws the dictation away and starts meeting notes
//!   ([`Action::TripleTap`]).
//!
//! Pure: presses, releases and the clock (monotonic ms) go in, what to do
//! comes out ([`Step`]). `pipeline.rs` keeps one [`Gesture`] for both
//! dictation hotkeys, acts on its steps and schedules its [`Gesture::tick`].
//! Every call is told whether a dictation is recording right now, since one
//! can start or stop elsewhere (the Yap bar's Dictate, the tray's Cancel, a
//! start refused while the last one is still transcribing): the machine
//! follows that first. Toggle mode doesn't use any of this.

/// The gestures' clock: monotonic ms since Yap started (never jumps with
/// the wall clock). The input hook stamps each key event with it the moment
/// the key moves, so a slow start (the edit hotkey copying the selection)
/// can't make a tap look like a hold.
pub fn now_ms() -> u64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64
}

/// A press let go sooner than this is a tap, not a hold.
pub const TAP_MAX_MS: u64 = 300;
/// A second press this soon after a tap was let go locks the recording.
pub const DOUBLE_TAP_GAP_MS: u64 = 500;
/// Holding the key this long earns the "double-tap is hands-free" hint.
pub const LONG_HOLD_MS: u64 = 60_000;
/// A hold younger than this that another key joins was meant as a shortcut
/// (Wispr Flow's: under a second, not locked).
pub const CHORD_MS: u64 = 1_000;

/// Where the gesture is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum State {
    /// Nothing recording (or a recording this machine doesn't own yet).
    #[default]
    Idle,
    /// The key went down at `since` and started a recording; it records
    /// while the key is held.
    Held { since: u64 },
    /// A tap was let go at `at`. Still recording, waiting for a second press
    /// (a double-tap) until `at + DOUBLE_TAP_GAP_MS`.
    TapPending { at: u64 },
    /// The second press of a double-tap is still down: recording, locked.
    LockHeld,
    /// Hands-free: recording until the next press.
    Locked,
}

/// What the pipeline should do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    /// Start a recording.
    Start,
    /// Stop and transcribe.
    Stop,
    /// Stop and throw the audio away (a lone tap, a hold that grew into a
    /// chord).
    Discard,
    /// The recording just went hands-free (tell the overlay).
    Lock,
    /// The key holding the recording isn't down any more (the input hook's
    /// stuck-key sweep): it goes on hands-free — tell the overlay, as for a
    /// lock, but it's not a double-tap.
    Unstick,
    /// A triple-tap (`triple_tap_meeting` on): throw the dictation away and
    /// start meeting notes.
    TripleTap,
}

/// A coaching hint the gesture earned (`coaching.rs` decides whether it
/// shows: each a few times at most, never two close together).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hint {
    /// A lone tap: "Hold the key while you talk — or double-tap it".
    Tap,
    /// A hold of [`LONG_HOLD_MS`] or more: "double-tap for hands-free".
    LongHold,
    /// A push-to-talk hold that grew into a chord: "Hold only {key} to
    /// dictate".
    ChordHold,
    /// The same in toggle mode: "Press only {key} to dictate".
    ChordPress,
    /// A dictation of ten minutes or more: meeting notes go on past the
    /// limit and summarise (`pipeline`, at its stop; once).
    LongDictation,
}

/// The outcome of one event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub action: Action,
    pub hint: Option<Hint>,
    /// Call [`Gesture::tick`] this many ms from now (a tap waits for its
    /// second press).
    pub tick_in: Option<u64>,
}

impl Step {
    const NOTHING: Step = Step { action: Action::None, hint: None, tick_in: None };

    fn act(action: Action) -> Step {
        Step { action, ..Step::NOTHING }
    }
}

/// The gesture state machine (see the module docs).
#[derive(Debug, Default)]
pub struct Gesture {
    state: State,
    /// "Triple-tap for meeting notes" (`triple_tap_meeting`; off).
    triple_tap: bool,
    /// When the second press of the double-tap that locked went down.
    lock_press_at: u64,
    /// A third press until then is a triple-tap: the second press was a
    /// tap too (let go within [`TAP_MAX_MS`]).
    third_tap_until: Option<u64>,
}

impl Gesture {
    #[cfg(test)]
    pub fn state(&self) -> State {
        self.state
    }

    /// "Triple-tap for meeting notes" on or off (the pipeline sets it from
    /// the config before each event).
    pub fn set_triple_tap(&mut self, on: bool) {
        self.triple_tap = on;
    }

    /// The recording is locked hands-free (a double-tap, or one started
    /// elsewhere and followed): nobody's holding it with the key. A hold, or
    /// a tap still waiting for its second press, isn't. (The tests' word for
    /// the state.)
    #[cfg(test)]
    pub fn hands_free(&self) -> bool {
        matches!(self.state, State::LockHeld | State::Locked)
    }

    /// Back to idle (toggle mode, or a start that was refused).
    pub fn reset(&mut self) {
        self.state = State::Idle;
        self.third_tap_until = None;
    }

    /// Follow a recording that started or stopped somewhere else.
    fn sync(&mut self, recording: bool) {
        match (self.state, recording) {
            // Stopped (or never started) behind our back.
            (s, false) if s != State::Idle => self.reset(),
            // Started elsewhere (the bar's Dictate): it's hands-free already,
            // so the next press stops it.
            (State::Idle, true) => {
                self.state = State::Locked;
                self.third_tap_until = None;
            }
            _ => {}
        }
    }

    /// The hotkey went down at `now`.
    pub fn press(&mut self, now: u64, recording: bool) -> Step {
        self.sync(recording);
        match self.state {
            State::Idle => {
                self.state = State::Held { since: now };
                Step::act(Action::Start)
            }
            State::TapPending { at } if now.saturating_sub(at) <= DOUBLE_TAP_GAP_MS => {
                self.state = State::LockHeld;
                self.lock_press_at = now;
                self.third_tap_until = None;
                Step::act(Action::Lock)
            }
            // Too late for a double-tap (the tick that would have thrown the
            // tap away hasn't run yet): a fresh hold, the recording goes on.
            State::TapPending { .. } => {
                self.state = State::Held { since: now };
                Step::NOTHING
            }
            State::Locked => {
                // A third quick tap, when that's for meeting notes.
                let third = self.triple_tap && self.third_tap_until.is_some_and(|until| now <= until);
                self.reset();
                Step::act(if third { Action::TripleTap } else { Action::Stop })
            }
            // Already down (the other hotkey, or a repeat): nothing new.
            State::Held { .. } | State::LockHeld => Step::NOTHING,
        }
    }

    /// The hotkey came up at `now`.
    pub fn release(&mut self, now: u64, recording: bool) -> Step {
        self.sync(recording);
        match self.state {
            State::Held { since } => {
                let held = now.saturating_sub(since);
                if held < TAP_MAX_MS {
                    self.state = State::TapPending { at: now };
                    Step { tick_in: Some(DOUBLE_TAP_GAP_MS), ..Step::NOTHING }
                } else {
                    self.reset();
                    Step { hint: (held >= LONG_HOLD_MS).then_some(Hint::LongHold), ..Step::act(Action::Stop) }
                }
            }
            State::LockHeld => {
                self.state = State::Locked;
                // The second press was a tap too: a third soon after is a
                // triple-tap.
                let tapped = now.saturating_sub(self.lock_press_at) < TAP_MAX_MS;
                self.third_tap_until = tapped.then_some(now + DOUBLE_TAP_GAP_MS);
                Step::NOTHING
            }
            // Idle (the press that stopped it, or one whose start was
            // refused), Locked (hands-free ignores the key coming up),
            // TapPending (a stray release).
            _ => Step::NOTHING,
        }
    }

    /// Time passed (scheduled by a step's `tick_in`): a tap whose second
    /// press never came is thrown away.
    pub fn tick(&mut self, now: u64, recording: bool) -> Step {
        self.sync(recording);
        match self.state {
            State::TapPending { at } if now.saturating_sub(at) >= DOUBLE_TAP_GAP_MS => {
                self.reset();
                Step { hint: Some(Hint::Tap), ..Step::act(Action::Discard) }
            }
            _ => Step::NOTHING,
        }
    }

    /// Another key went down at `now` while the hotkey was held: a hold
    /// under [`CHORD_MS`] was meant as a shortcut that shares the key, so
    /// it's thrown away, silently, with the "hold only {key}" hint. A locked
    /// recording, an older hold or a tap's wait are left alone.
    pub fn chord(&mut self, now: u64, recording: bool) -> Step {
        self.sync(recording);
        match self.state {
            State::Held { since } if now.saturating_sub(since) < CHORD_MS => {
                self.reset();
                Step { hint: Some(Hint::ChordHold), ..Step::act(Action::Discard) }
            }
            _ => Step::NOTHING,
        }
    }

    /// The input hook found the hotkey isn't down any more though no key-up
    /// came (its stuck-key sweep: the lock screen, a UAC prompt, sleep took
    /// the key-up). A recording it held goes on **hands-free**, never
    /// stopped here: the next press stops it, and nothing said is lost.
    pub fn key_lost(&mut self, recording: bool) -> Step {
        self.sync(recording);
        match self.state {
            State::Held { .. } | State::LockHeld => {
                self.state = State::Locked;
                self.third_tap_until = None;
                Step::act(Action::Unstick)
            }
            // A tap's key is up already; idle or hands-free, nothing held.
            _ => Step::NOTHING,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drives a [`Gesture`] the way the pipeline does: `recording` follows
    /// the actions (a start that `refuse` stops never records).
    struct Rig {
        g: Gesture,
        recording: bool,
        refuse: bool,
    }

    impl Rig {
        fn new() -> Self {
            Rig { g: Gesture::default(), recording: false, refuse: false }
        }
        fn apply(&mut self, step: Step) -> Step {
            match step.action {
                Action::Start => {
                    self.recording = !self.refuse;
                    if self.refuse {
                        self.g.reset();
                    }
                }
                Action::Stop | Action::Discard | Action::TripleTap => self.recording = false,
                Action::Lock | Action::Unstick | Action::None => {}
            }
            step
        }
        fn chord(&mut self, t: u64) -> Step {
            let s = self.g.chord(t, self.recording);
            self.apply(s)
        }
        fn key_lost(&mut self) -> Step {
            let s = self.g.key_lost(self.recording);
            self.apply(s)
        }
        fn press(&mut self, t: u64) -> Step {
            let s = self.g.press(t, self.recording);
            self.apply(s)
        }
        fn release(&mut self, t: u64) -> Step {
            let s = self.g.release(t, self.recording);
            self.apply(s)
        }
        fn tick(&mut self, t: u64) -> Step {
            let s = self.g.tick(t, self.recording);
            self.apply(s)
        }
    }

    fn act(a: Action) -> Step {
        Step::act(a)
    }

    #[test]
    fn a_hold_records_while_the_key_is_down() {
        let mut r = Rig::new();
        assert_eq!(r.press(0), act(Action::Start));
        assert!(r.recording);
        assert_eq!(r.release(2_000), act(Action::Stop));
        assert!(!r.recording);
        assert_eq!(r.g.state(), State::Idle);
    }

    #[test]
    fn a_press_held_exactly_the_tap_limit_is_a_hold() {
        let mut r = Rig::new();
        r.press(1_000);
        assert_eq!(r.release(1_000 + TAP_MAX_MS), act(Action::Stop));
        r.press(5_000);
        // Just under: a tap.
        assert_eq!(r.release(5_000 + TAP_MAX_MS - 1).action, Action::None);
    }

    #[test]
    fn a_lone_tap_keeps_recording_for_the_gap_then_is_thrown_away_with_a_hint() {
        let mut r = Rig::new();
        assert_eq!(r.press(0), act(Action::Start));
        let up = r.release(120);
        assert_eq!(up, Step { tick_in: Some(DOUBLE_TAP_GAP_MS), ..Step::NOTHING });
        // Still recording, in case a second press comes.
        assert!(r.recording);
        assert_eq!(r.g.state(), State::TapPending { at: 120 });
        // A tick before the gap is up does nothing.
        assert_eq!(r.tick(400), Step::NOTHING);
        assert!(r.recording);
        // The gap is up: discard, and the hint.
        assert_eq!(r.tick(120 + DOUBLE_TAP_GAP_MS), Step { hint: Some(Hint::Tap), ..act(Action::Discard) });
        assert!(!r.recording);
        assert_eq!(r.g.state(), State::Idle);
        // A second tick (a stale timer) does nothing.
        assert_eq!(r.tick(5_000), Step::NOTHING);
    }

    #[test]
    fn a_double_tap_locks_until_the_next_press() {
        let mut r = Rig::new();
        assert_eq!(r.press(0), act(Action::Start));
        r.release(90);
        assert_eq!(r.press(90 + DOUBLE_TAP_GAP_MS), act(Action::Lock));
        assert_eq!(r.g.state(), State::LockHeld);
        // Letting go keeps it recording.
        assert_eq!(r.release(700), Step::NOTHING);
        assert_eq!(r.g.state(), State::Locked);
        assert!(r.recording);
        // The tap's timer finds a locked recording: nothing to throw away.
        assert_eq!(r.tick(90 + DOUBLE_TAP_GAP_MS), Step::NOTHING);
        assert!(r.recording);
        // Minutes later, one press stops it; its release does nothing.
        assert_eq!(r.press(300_000), act(Action::Stop));
        assert!(!r.recording);
        assert_eq!(r.release(300_100), Step::NOTHING);
        assert_eq!(r.g.state(), State::Idle);
        // And the next press is a fresh hold.
        assert_eq!(r.press(400_000), act(Action::Start));
    }

    #[test]
    fn only_a_locked_recording_is_hands_free() {
        // What the Voice Assistant's pause asks before it ends a question:
        // never under a held key, nor a tap that may still become a lock.
        let mut r = Rig::new();
        assert!(!r.g.hands_free());
        r.press(0);
        assert!(!r.g.hands_free(), "held");
        r.release(100);
        assert!(!r.g.hands_free(), "a tap waiting for its second press");
        r.press(300);
        assert!(r.g.hands_free(), "the second press, still down");
        r.release(400);
        assert!(r.g.hands_free(), "locked");
        r.press(5_000);
        assert!(!r.g.hands_free(), "stopped");
    }

    #[test]
    fn a_long_second_press_still_locks() {
        // The second press of a double-tap may be held a while: still locked.
        let mut r = Rig::new();
        r.press(0);
        r.release(100);
        assert_eq!(r.press(200), act(Action::Lock));
        assert_eq!(r.release(90_000), Step::NOTHING);
        assert_eq!(r.g.state(), State::Locked);
    }

    #[test]
    fn a_second_press_after_the_gap_is_a_fresh_hold() {
        // The discard tick hasn't run yet (a slow thread): a late second press
        // carries the recording on as an ordinary hold, no lock, no hint.
        let mut r = Rig::new();
        r.press(0);
        r.release(100);
        assert_eq!(r.press(100 + DOUBLE_TAP_GAP_MS + 1), Step::NOTHING);
        assert_eq!(r.g.state(), State::Held { since: 601 });
        assert!(r.recording);
        assert_eq!(r.tick(700), Step::NOTHING);
        assert_eq!(r.release(2_000), act(Action::Stop));
    }

    #[test]
    fn a_hold_of_a_minute_or_more_earns_the_hands_free_hint() {
        let mut r = Rig::new();
        r.press(0);
        assert_eq!(r.release(LONG_HOLD_MS - 1), act(Action::Stop));
        r.press(100_000);
        assert_eq!(
            r.release(100_000 + LONG_HOLD_MS),
            Step { hint: Some(Hint::LongHold), ..act(Action::Stop) }
        );
    }

    #[test]
    fn a_recording_started_elsewhere_stops_on_the_next_press() {
        // The bar's Dictate started one: push-to-talk treats it as hands-free.
        let mut r = Rig::new();
        r.recording = true;
        assert_eq!(r.press(0), act(Action::Stop));
        assert_eq!(r.release(80), Step::NOTHING);
        assert_eq!(r.g.state(), State::Idle);
    }

    #[test]
    fn a_recording_stopped_elsewhere_is_followed() {
        // Cancelled from the tray while a tap waited for its second press:
        // the timer finds nothing to discard, and the next press starts anew.
        let mut r = Rig::new();
        r.press(0);
        r.release(100);
        r.recording = false;
        assert_eq!(r.tick(600), Step::NOTHING);
        assert_eq!(r.press(700), act(Action::Start));

        // Stopped elsewhere while locked: the next press starts a new one.
        let mut r = Rig::new();
        r.press(0);
        r.release(100);
        r.press(200);
        r.release(300);
        r.recording = false;
        assert_eq!(r.press(10_000), act(Action::Start));

        // Stopped elsewhere while held: the release does nothing.
        let mut r = Rig::new();
        r.press(0);
        r.recording = false;
        assert_eq!(r.release(5_000), Step::NOTHING);
    }

    #[test]
    fn a_refused_start_leaves_nothing_behind() {
        // Still transcribing the last one, or no microphone: the start is
        // refused, so the release has nothing to stop and no tap to time.
        let mut r = Rig::new();
        r.refuse = true;
        assert_eq!(r.press(0), act(Action::Start));
        assert!(!r.recording);
        assert_eq!(r.release(100), Step::NOTHING);
        assert_eq!(r.g.state(), State::Idle);
    }

    #[test]
    fn repeated_presses_while_down_change_nothing() {
        let mut r = Rig::new();
        r.press(0);
        assert_eq!(r.press(50), Step::NOTHING);
        assert_eq!(r.g.state(), State::Held { since: 0 });
        r.release(100);
        r.press(200);
        assert_eq!(r.press(250), Step::NOTHING);
        assert_eq!(r.g.state(), State::LockHeld);
    }

    #[test]
    fn a_triple_tap_stops_on_the_third_press() {
        // Tap, tap (locked), tap: the third press is "the next press".
        let mut r = Rig::new();
        r.press(0);
        r.release(80);
        r.press(200);
        r.release(280);
        assert_eq!(r.press(400), act(Action::Stop));
        assert_eq!(r.release(480), Step::NOTHING);
        assert!(!r.recording);
    }

    #[test]
    fn a_triple_tap_starts_meeting_notes_only_when_switched_on_and_quick() {
        let mut r = Rig::new();
        r.g.set_triple_tap(true);
        r.press(0);
        r.release(80);
        r.press(200);
        r.release(280);
        assert_eq!(r.press(400), act(Action::TripleTap));
        assert!(!r.recording);
        assert_eq!(r.g.state(), State::Idle);
        assert_eq!(r.release(450), Step::NOTHING);

        // Too late for a third tap: the press just stops, as ever.
        let mut r = Rig::new();
        r.g.set_triple_tap(true);
        r.press(0);
        r.release(80);
        r.press(200);
        r.release(280);
        assert_eq!(r.press(280 + DOUBLE_TAP_GAP_MS + 1), act(Action::Stop));

        // The second press held (a lock held down, not a tap): a quick
        // press after it stops, never a triple-tap.
        let mut r = Rig::new();
        r.g.set_triple_tap(true);
        r.press(0);
        r.release(80);
        r.press(200);
        r.release(200 + TAP_MAX_MS + 50);
        assert_eq!(r.press(200 + TAP_MAX_MS + 150), act(Action::Stop));

        // Switched off: tap, tap, tap stops (the test above).
    }

    #[test]
    fn a_short_hold_that_grows_into_a_chord_is_thrown_away_with_a_hint() {
        // AltGr+E, Ctrl+Win+D: another key within a second of the press.
        let mut r = Rig::new();
        assert_eq!(r.press(1_000), act(Action::Start));
        assert_eq!(r.chord(1_000 + CHORD_MS - 1), Step { hint: Some(Hint::ChordHold), ..act(Action::Discard) });
        assert!(!r.recording);
        assert_eq!(r.g.state(), State::Idle);
        // Its key coming up afterwards does nothing; the next press starts anew.
        assert_eq!(r.release(1_300), Step::NOTHING);
        assert_eq!(r.press(5_000), act(Action::Start));
    }

    #[test]
    fn a_chord_after_a_second_or_on_a_locked_recording_changes_nothing() {
        // Held a second already: dictating, and a key pressed meanwhile.
        let mut r = Rig::new();
        r.press(0);
        assert_eq!(r.chord(CHORD_MS), Step::NOTHING);
        assert!(r.recording);
        assert_eq!(r.release(4_000), act(Action::Stop));

        // Locked hands-free (the second press still down): not a shortcut.
        let mut r = Rig::new();
        r.press(0);
        r.release(100);
        r.press(200);
        assert_eq!(r.chord(300), Step::NOTHING);
        r.release(350);
        assert_eq!(r.chord(400), Step::NOTHING);
        assert!(r.recording);

        // A tap waiting for its second press: the key is up, no chord.
        let mut r = Rig::new();
        r.press(0);
        r.release(100);
        assert_eq!(r.chord(200), Step::NOTHING);
        assert!(r.recording);

        // Nothing recording (a start refused): nothing to drop.
        let mut r = Rig::new();
        assert_eq!(r.chord(10), Step::NOTHING);
    }

    #[test]
    fn a_stuck_key_leaves_its_recording_going_hands_free() {
        // The key-up was lost (the lock screen): never stopped by the sweep.
        let mut r = Rig::new();
        r.press(0);
        assert_eq!(r.key_lost(), act(Action::Unstick));
        assert!(r.recording);
        assert_eq!(r.g.state(), State::Locked);
        assert!(r.g.hands_free());
        // A sweep again finds nothing to do.
        assert_eq!(r.key_lost(), Step::NOTHING);
        // The next press stops it, and nothing said is lost.
        assert_eq!(r.press(90_000), act(Action::Stop));
        assert!(!r.recording);

        // The second press of a double-tap, stuck: locked anyway.
        let mut r = Rig::new();
        r.press(0);
        r.release(100);
        r.press(200);
        assert_eq!(r.key_lost(), act(Action::Unstick));
        assert_eq!(r.g.state(), State::Locked);

        // Idle, a tap's wait, hands-free already: nothing.
        let mut r = Rig::new();
        assert_eq!(r.key_lost(), Step::NOTHING);
        r.press(0);
        r.release(100);
        assert_eq!(r.key_lost(), Step::NOTHING);
        assert_eq!(r.g.state(), State::TapPending { at: 100 });

        // A key held when nothing records (stopped from the tray): idle.
        let mut r = Rig::new();
        r.press(0);
        r.recording = false;
        assert_eq!(r.key_lost(), Step::NOTHING);
        assert_eq!(r.g.state(), State::Idle);
    }
}

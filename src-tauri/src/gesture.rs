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
//!   "double-tap is hands-free" hint ([`Hint::LongHold`]).
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
    /// Stop and throw the audio away (a lone tap).
    Discard,
    /// The recording just went hands-free (tell the overlay).
    Lock,
}

/// A coaching hint the gesture earned (`coaching.rs` decides whether it
/// shows: each a few times at most, never two close together).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hint {
    /// A lone tap: "Hold the key while you talk — or double-tap it".
    Tap,
    /// A hold of [`LONG_HOLD_MS`] or more: "double-tap for hands-free".
    LongHold,
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
}

impl Gesture {
    #[cfg(test)]
    pub fn state(&self) -> State {
        self.state
    }

    /// The recording is locked hands-free (a double-tap, or one started
    /// elsewhere and followed): nobody's holding it with the key, so the
    /// Voice Assistant's pause may end it (`pause.rs`). A hold, or a tap
    /// still waiting for its second press, isn't.
    pub fn hands_free(&self) -> bool {
        matches!(self.state, State::LockHeld | State::Locked)
    }

    /// Back to idle (toggle mode, or a start that was refused).
    pub fn reset(&mut self) {
        self.state = State::Idle;
    }

    /// Follow a recording that started or stopped somewhere else.
    fn sync(&mut self, recording: bool) {
        match (self.state, recording) {
            // Stopped (or never started) behind our back.
            (s, false) if s != State::Idle => self.state = State::Idle,
            // Started elsewhere (the bar's Dictate): it's hands-free already,
            // so the next press stops it.
            (State::Idle, true) => self.state = State::Locked,
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
                Step::act(Action::Lock)
            }
            // Too late for a double-tap (the tick that would have thrown the
            // tap away hasn't run yet): a fresh hold, the recording goes on.
            State::TapPending { .. } => {
                self.state = State::Held { since: now };
                Step::NOTHING
            }
            State::Locked => {
                self.state = State::Idle;
                Step::act(Action::Stop)
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
                    self.state = State::Idle;
                    Step { hint: (held >= LONG_HOLD_MS).then_some(Hint::LongHold), ..Step::act(Action::Stop) }
                }
            }
            State::LockHeld => {
                self.state = State::Locked;
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
                self.state = State::Idle;
                Step { hint: Some(Hint::Tap), ..Step::act(Action::Discard) }
            }
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
                Action::Stop | Action::Discard => self.recording = false,
                Action::Lock | Action::None => {}
            }
            step
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
}

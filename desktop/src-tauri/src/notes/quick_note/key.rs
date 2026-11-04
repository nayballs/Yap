//! The **Quick note shortcut's tap and hold** (Wispr Flow's Scratchpad key,
//! by behaviour; 2026-10-29):
//!
//! - **a tap** opens the window, or puts it away while it's in front — as
//!   the shortcut always did;
//! - **a hold** past [`HOLD_MS`] opens it and starts a push-to-talk
//!   dictation **into the note** (`quick_note::arm` → `Goes::QuickNote`,
//!   never pasted), which ends when the key comes up.
//!
//! The dictation starts only once the hold is a hold, so a tap never plays
//! a chime or flashes the bar's capsule. [`HOLD_MS`] is push-to-talk's own
//! line between a tap and a hold (`gesture::TAP_MAX_MS`, 300 ms): a press
//! let go sooner is a tap everywhere in Yap. Wispr's is 350 ms; Yap's is a
//! little quicker, and the window is up long before it (on the press).
//!
//! A hold under `gesture::CHORD_MS` that another key joins was meant as a
//! shortcut sharing the key: no dictation, or the one it started dropped
//! silently ([`Key::chord`]). A key-up the input hook lost (the lock
//! screen, a UAC prompt) leaves the dictation going hands-free until the
//! next press ([`Key::key_lost`]), as push-to-talk's does.
//!
//! Where the key's moves come from: the input hook (`quick-note-key-pressed`
//! / `-released`, any app in front), or a Yap window's in-page fallback
//! (`quick_note_key`: the hook never sees keys while a Yap webview has the
//! focus — and the quick note takes the focus on the press, so its own page
//! usually hears the release). Both come here; a move heard twice does
//! nothing the second time. The window's handle on the hook's binding is
//! let go when a page hears the release (`input_hook::quick_note_key_up`),
//! so the next press from another app isn't taken for the key repeating.
//!
//! [`Key`] is pure (presses, releases and the clock in, a [`Step`] out); the
//! rest drives it: the window on the main thread, the dictation through the
//! pipeline (`Pipeline::start_for_quick_note`).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::dictation::gesture::{now_ms, CHORD_MS, TAP_MAX_MS};

/// Held this long, the press is a hold: a dictation into the note starts.
pub const HOLD_MS: u64 = TAP_MAX_MS;

/// Where the key is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum State {
    /// Up (or down for nothing Yap knows of).
    #[default]
    Idle,
    /// Down since `since`, not a hold yet. Let go now it's a tap: `hide`
    /// puts the window away (it was in front when pressed), else it stays up.
    Pending { since: u64, hide: bool },
    /// Held past [`HOLD_MS`]: the note's dictation records until it's let go.
    Talking { since: u64 },
    /// Down, with nothing more to do (a dictation was already under way
    /// when it became a hold, or the hold grew into a chord).
    Spent,
    /// The key-up was lost (`key_lost`): the dictation goes on hands-free
    /// until the next press.
    Free,
}

/// What happens to the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Window {
    Leave,
    /// Up, on the note it opens on, in front.
    Open,
    /// Put away.
    Hide,
}

/// What happens to the note's dictation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Talk {
    Leave,
    /// Start one, held (push-to-talk).
    Start,
    /// Stop and transcribe it into the note.
    Stop,
    /// Stop and throw it away, silently (a hold that grew into a chord).
    Discard,
    /// It goes on hands-free (its key-up was lost): tell the bar.
    Unstick,
}

/// One event's outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub window: Window,
    pub talk: Talk,
    /// Call [`Key::tick`] this many ms from now (a press waits to be a hold).
    pub tick_in: Option<u64>,
}

impl Step {
    const NOTHING: Step = Step { window: Window::Leave, talk: Talk::Leave, tick_in: None };

    fn window(window: Window) -> Step {
        Step { window, ..Step::NOTHING }
    }

    fn talk(talk: Talk) -> Step {
        Step { talk, ..Step::NOTHING }
    }
}

/// The key's state machine (see the module docs). Every call is told
/// whether the note's own dictation still records (`ours`): one stopped
/// elsewhere (the bar's ✓, Esc, its length limit) is followed first.
#[derive(Debug, Default)]
pub struct Key {
    state: State,
}

impl Key {
    #[cfg(test)]
    pub fn state(&self) -> State {
        self.state
    }

    /// Follow a dictation of ours that ended elsewhere.
    fn sync(&mut self, ours: bool) {
        if matches!(self.state, State::Talking { .. } | State::Free) && !ours {
            self.state = match self.state {
                State::Talking { .. } => State::Spent,
                _ => State::Idle,
            };
        }
    }

    /// The key went down at `now`; `in_front`: the quick note is the window
    /// in front.
    pub fn press(&mut self, now: u64, in_front: bool, ours: bool) -> Step {
        self.sync(ours);
        match self.state {
            // Hands-free since its key-up was lost: this press stops it.
            State::Free => {
                self.state = State::Spent;
                Step::talk(Talk::Stop)
            }
            // Down already (the key repeating, or heard by the hook and a
            // page both): nothing new.
            State::Pending { .. } | State::Talking { .. } | State::Spent => Step::NOTHING,
            State::Idle => {
                self.state = State::Pending { since: now, hide: in_front };
                Step {
                    window: if in_front { Window::Leave } else { Window::Open },
                    talk: Talk::Leave,
                    tick_in: Some(HOLD_MS),
                }
            }
        }
    }

    /// The key came up at `now`.
    pub fn release(&mut self, now: u64, ours: bool) -> Step {
        self.sync(ours);
        match self.state {
            State::Pending { since, hide } => {
                self.state = State::Idle;
                // A tap: away if it was in front. (A press held past the
                // hold whose tick hasn't run yet is neither: nothing.)
                if hide && now.saturating_sub(since) < HOLD_MS {
                    Step::window(Window::Hide)
                } else {
                    Step::NOTHING
                }
            }
            State::Talking { .. } => {
                self.state = State::Idle;
                Step::talk(Talk::Stop)
            }
            State::Spent => {
                self.state = State::Idle;
                Step::NOTHING
            }
            // Hands-free ignores a key coming up; idle, a stray release.
            State::Free | State::Idle => Step::NOTHING,
        }
    }

    /// Time passed (a press's `tick_in`): still down past [`HOLD_MS`], it's a
    /// hold — the note's dictation starts, unless one is under way already
    /// (`busy`: recording or transcribing).
    pub fn tick(&mut self, now: u64, busy: bool) -> Step {
        match self.state {
            State::Pending { since, .. } if now.saturating_sub(since) >= HOLD_MS => {
                if busy {
                    self.state = State::Spent;
                    Step::NOTHING
                } else {
                    self.state = State::Talking { since };
                    Step::talk(Talk::Start)
                }
            }
            _ => Step::NOTHING,
        }
    }

    /// The dictation a [`Talk::Start`] asked for didn't start (no
    /// microphone, one still transcribing): the key does nothing more.
    pub fn refused(&mut self) {
        if matches!(self.state, State::Talking { .. }) {
            self.state = State::Spent;
        }
    }

    /// Another key went down at `now` while this one was held: under
    /// `CHORD_MS` it was meant as a shortcut sharing the key — no dictation
    /// (or the one it started dropped). Older holds are left alone.
    pub fn chord(&mut self, now: u64, ours: bool) -> Step {
        self.sync(ours);
        match self.state {
            State::Pending { since, .. } if now.saturating_sub(since) < CHORD_MS => {
                self.state = State::Spent;
                Step::NOTHING
            }
            State::Talking { since } if now.saturating_sub(since) < CHORD_MS => {
                self.state = State::Spent;
                Step::talk(Talk::Discard)
            }
            _ => Step::NOTHING,
        }
    }

    /// The input hook let go of the key without its key-up (its stuck-key
    /// sweep): a dictation it held goes on hands-free, never stopped here.
    pub fn key_lost(&mut self, ours: bool) -> Step {
        self.sync(ours);
        match self.state {
            State::Talking { .. } => {
                self.state = State::Free;
                Step::talk(Talk::Unstick)
            }
            State::Pending { .. } | State::Spent => {
                self.state = State::Idle;
                Step::NOTHING
            }
            State::Free | State::Idle => Step::NOTHING,
        }
    }
}

// ---- driving it --------------------------------------------------------------------------------

/// The machine; locked while a step is acted on, so a key event and the
/// hold's tick take turns (as push-to-talk's gesture lock does).
static KEY: Mutex<Key> = Mutex::new(Key { state: State::Idle });
/// The note's dictation the key started (the pipeline's recording session;
/// 0: none).
static OURS: AtomicU64 = AtomicU64::new(0);

fn key() -> std::sync::MutexGuard<'static, Key> {
    KEY.lock().unwrap_or_else(|p| p.into_inner())
}

/// The running pipeline, if there is one.
fn with_pipeline<T>(app: &AppHandle, f: impl FnOnce(&crate::dictation::pipeline::Pipeline) -> T) -> Option<T> {
    let state = app.try_state::<crate::AppState>()?;
    let guard = state.pipeline.lock().ok()?;
    guard.as_ref().map(f)
}

/// The key's dictation still records.
fn ours(app: &AppHandle) -> bool {
    let session = OURS.load(Ordering::SeqCst);
    session != 0 && with_pipeline(app, |p| p.recording_session()).flatten() == Some(session)
}

/// The Quick note shortcut moved: `pressed`, at `at` (the hook's
/// timestamp, `gesture::now_ms`; `None` = now).
pub fn on_key(app: &AppHandle, pressed: bool, at: Option<u64>) {
    let now = at.unwrap_or_else(now_ms);
    let mut k = key();
    let ours = ours(app);
    let step = if pressed {
        k.press(now, super::in_front(), ours)
    } else {
        k.release(now, ours)
    };
    tracing::debug!(pressed, ?step, "Quick note key");
    act(app, &mut k, step);
}

/// The hold's tick.
fn tick(app: &AppHandle) {
    let mut k = key();
    let busy = with_pipeline(app, |p| p.is_busy()).unwrap_or(true);
    let step = k.tick(now_ms(), busy);
    act(app, &mut k, step);
}

/// The input hook: another key joined a hold-to-talk key at `at`.
pub fn key_chord(app: &AppHandle, at: u64) {
    let mut k = key();
    let ours = ours(app);
    let step = k.chord(at, ours);
    act(app, &mut k, step);
}

/// The input hook's sweep let go of a hold-to-talk key without its key-up.
pub fn key_stuck(app: &AppHandle) {
    let mut k = key();
    let ours = ours(app);
    let step = k.key_lost(ours);
    act(app, &mut k, step);
}

/// Do what a step says, with the machine locked.
fn act(app: &AppHandle, k: &mut Key, step: Step) {
    match step.window {
        Window::Leave => {}
        Window::Open => super::open_soon(app),
        Window::Hide => super::hide_soon(app),
    }
    match step.talk {
        Talk::Leave => {}
        Talk::Start => {
            let session = with_pipeline(app, |p| p.start_for_quick_note()).flatten();
            match session {
                Some(s) => {
                    OURS.store(s, Ordering::SeqCst);
                    tracing::info!("Quick note: the shortcut held, dictating into the note");
                }
                None => {
                    tracing::info!("Quick note: the shortcut held, but no dictation started");
                    k.refused();
                }
            }
        }
        Talk::Stop => {
            let session = OURS.swap(0, Ordering::SeqCst);
            with_pipeline(app, |p| p.stop_session(session));
        }
        Talk::Discard => {
            tracing::info!("Quick note: the hold grew into a chord, so it was a shortcut: dropped");
            let session = OURS.swap(0, Ordering::SeqCst);
            with_pipeline(app, |p| p.discard_session(session));
        }
        Talk::Unstick => {
            tracing::info!("Quick note: the key holding the dictation was stuck down; it goes on hands-free");
            let _ = app.emit("yap-dictation-locked", true);
        }
    }
    if let Some(ms) = step.tick_in {
        let app = app.clone();
        let _ = std::thread::Builder::new().name("quick-note-hold".into()).spawn(move || {
            std::thread::sleep(Duration::from_millis(ms));
            tick(&app);
        });
    }
}

/// The pages' key moves, handed to one thread in the order they came: the
/// command returns at once (it runs on the main thread, which a dictation
/// starting may need), and a release never overtakes its press.
static PAGE_KEYS: std::sync::OnceLock<Mutex<std::sync::mpsc::Sender<(bool, u64)>>> = std::sync::OnceLock::new();

fn page_keys(app: &AppHandle) -> Option<std::sync::mpsc::Sender<(bool, u64)>> {
    let tx = PAGE_KEYS.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<(bool, u64)>();
        let app = app.clone();
        let _ = std::thread::Builder::new().name("quick-note-key".into()).spawn(move || {
            while let Ok((pressed, at)) = rx.recv() {
                on_key(&app, pressed, Some(at));
            }
        });
        Mutex::new(tx)
    });
    tx.lock().ok().map(|t| t.clone())
}

/// The Quick note shortcut from one of Yap's own windows (their in-page
/// fallback: the hook never sees keys while a Yap webview has the focus):
/// its press and its release. A release heard here lets go of the hook's
/// hold on the key too.
#[tauri::command]
pub fn quick_note_key(app: AppHandle, pressed: bool) {
    if !pressed {
        crate::dictation::input_hook::quick_note_key_up();
    }
    let at = now_ms();
    match page_keys(&app) {
        Some(tx) if tx.send((pressed, at)).is_ok() => {}
        _ => on_key(&app, pressed, Some(at)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drives a [`Key`] as the driver does: `recording` follows its talk.
    struct Rig {
        k: Key,
        recording: bool,
    }

    impl Rig {
        fn new() -> Self {
            Rig { k: Key::default(), recording: false }
        }
        fn apply(&mut self, step: Step) -> Step {
            match step.talk {
                Talk::Start => self.recording = true,
                Talk::Stop | Talk::Discard => self.recording = false,
                Talk::Leave | Talk::Unstick => {}
            }
            step
        }
        fn press(&mut self, t: u64, in_front: bool) -> Step {
            let s = self.k.press(t, in_front, self.recording);
            self.apply(s)
        }
        fn release(&mut self, t: u64) -> Step {
            let s = self.k.release(t, self.recording);
            self.apply(s)
        }
        fn tick(&mut self, t: u64) -> Step {
            let s = self.k.tick(t, false);
            self.apply(s)
        }
    }

    fn opened() -> Step {
        Step { window: Window::Open, talk: Talk::Leave, tick_in: Some(HOLD_MS) }
    }

    #[test]
    fn a_tap_opens_it_and_a_tap_while_its_in_front_puts_it_away() {
        let mut r = Rig::new();
        // Not in front: up, and a wait to see if it's a hold.
        assert_eq!(r.press(0, false), opened());
        // Let go before the hold: a tap; it stays up, nothing recorded.
        assert_eq!(r.release(120), Step::NOTHING);
        assert_eq!(r.tick(HOLD_MS), Step::NOTHING, "the tick after a tap does nothing");
        assert!(!r.recording);
        assert_eq!(r.k.state(), State::Idle);
        // In front: a tap puts it away (on the key-up, not the press).
        assert_eq!(r.press(1_000, true), Step { window: Window::Leave, talk: Talk::Leave, tick_in: Some(HOLD_MS) });
        assert_eq!(r.release(1_150), Step::window(Window::Hide));
        assert!(!r.recording);
    }

    #[test]
    fn a_hold_opens_it_and_dictates_into_the_note_until_it_comes_up() {
        let mut r = Rig::new();
        assert_eq!(r.press(0, false), opened());
        // Not a hold yet.
        assert_eq!(r.tick(HOLD_MS - 1), Step::NOTHING);
        // A hold: the note's dictation starts…
        assert_eq!(r.tick(HOLD_MS), Step::talk(Talk::Start));
        assert!(r.recording);
        assert_eq!(r.k.state(), State::Talking { since: 0 });
        // …the key repeating changes nothing…
        assert_eq!(r.press(800, true), Step::NOTHING);
        // …and it stops when the key comes up (never hiding the window).
        assert_eq!(r.release(4_000), Step::talk(Talk::Stop));
        assert!(!r.recording);
        assert_eq!(r.k.state(), State::Idle);
    }

    #[test]
    fn a_hold_with_the_window_in_front_dictates_and_keeps_it_up() {
        let mut r = Rig::new();
        r.press(0, true);
        assert_eq!(r.tick(HOLD_MS + 5), Step::talk(Talk::Start));
        assert_eq!(r.release(2_000), Step::talk(Talk::Stop), "not hidden");
    }

    #[test]
    fn exactly_the_hold_is_a_hold_and_just_under_it_a_tap() {
        let mut r = Rig::new();
        r.press(10_000, true);
        assert_eq!(r.release(10_000 + HOLD_MS - 1), Step::window(Window::Hide));
        // Held to the hold with its tick still to run: neither a tap nor a
        // dictation.
        r.press(20_000, true);
        assert_eq!(r.release(20_000 + HOLD_MS), Step::NOTHING);
        assert_eq!(r.tick(20_000 + HOLD_MS + 2), Step::NOTHING);
        assert!(!r.recording);
    }

    #[test]
    fn a_hold_while_another_dictation_runs_only_opens_the_window() {
        let mut r = Rig::new();
        r.press(0, false);
        assert_eq!(r.k.tick(HOLD_MS, true), Step::NOTHING);
        assert_eq!(r.k.state(), State::Spent);
        assert_eq!(r.release(2_000), Step::NOTHING, "not a tap either: it stays up");
        assert_eq!(r.k.state(), State::Idle);
    }

    #[test]
    fn a_dictation_that_didnt_start_leaves_the_key_spent() {
        let mut r = Rig::new();
        r.press(0, false);
        assert_eq!(r.k.tick(HOLD_MS, false), Step::talk(Talk::Start));
        r.k.refused();
        assert_eq!(r.k.state(), State::Spent);
        assert_eq!(r.k.release(1_000, false), Step::NOTHING);
    }

    #[test]
    fn a_dictation_stopped_elsewhere_is_followed() {
        let mut r = Rig::new();
        r.press(0, false);
        r.tick(HOLD_MS);
        // The bar's ✓ (or Esc) stopped it while the key was down.
        r.recording = false;
        assert_eq!(r.release(3_000), Step::NOTHING, "nothing left to stop");
        assert_eq!(r.k.state(), State::Idle);
    }

    #[test]
    fn a_hold_that_grows_into_a_chord_is_a_shortcut() {
        // Before the hold: no dictation, the window left up.
        let mut r = Rig::new();
        r.press(0, false);
        assert_eq!(r.k.chord(150, false), Step::NOTHING);
        assert_eq!(r.k.state(), State::Spent);
        assert_eq!(r.tick(HOLD_MS), Step::NOTHING);
        assert_eq!(r.release(600), Step::NOTHING);
        assert!(!r.recording);
        // Recording already, under a second: dropped.
        let mut r = Rig::new();
        r.press(0, false);
        r.tick(HOLD_MS);
        let s = r.k.chord(700, true);
        assert_eq!(r.apply(s), Step::talk(Talk::Discard));
        assert!(!r.recording);
        assert_eq!(r.release(900), Step::NOTHING);
        // An older hold: another key is just another key.
        let mut r = Rig::new();
        r.press(0, false);
        r.tick(HOLD_MS);
        assert_eq!(r.k.chord(CHORD_MS + 1, true), Step::NOTHING);
        assert!(r.recording);
    }

    #[test]
    fn a_lost_key_up_leaves_the_dictation_hands_free_until_the_next_press() {
        let mut r = Rig::new();
        r.press(0, false);
        r.tick(HOLD_MS);
        assert_eq!(r.k.key_lost(true), Step::talk(Talk::Unstick));
        assert_eq!(r.k.state(), State::Free);
        assert!(r.recording);
        // A stray release changes nothing; the next press stops it.
        assert_eq!(r.release(9_000), Step::NOTHING);
        assert_eq!(r.press(10_000, true), Step::talk(Talk::Stop));
        assert!(!r.recording);
        // That press's own release is spent.
        assert_eq!(r.release(10_100), Step::NOTHING);
        assert_eq!(r.k.state(), State::Idle);
        // A pending press whose key-up was lost: nothing to keep going.
        let mut r = Rig::new();
        r.press(0, false);
        assert_eq!(r.k.key_lost(false), Step::NOTHING);
        assert_eq!(r.tick(HOLD_MS), Step::NOTHING);
        assert_eq!(r.k.state(), State::Idle);
    }

    #[test]
    fn the_hold_is_push_to_talks_tap_line() {
        assert_eq!(HOLD_MS, TAP_MAX_MS);
        assert_eq!(HOLD_MS, 300);
    }
}

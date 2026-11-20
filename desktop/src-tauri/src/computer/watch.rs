//! **Who's in control** (phase 2): one chat drives the PC at a time, and
//! the person takes it back at once.
//!
//! - **One controller**: a chat's input actions hold the PC for
//!   [`HOLD`] after its last one; another chat's are refused meanwhile ("Another
//!   chat is using your PC").
//! - **The person's own input stops it** (robot mode's thresholds,
//!   `desktop/e2e-real/support/inputwatch.cs`): any real key — Esc above all
//!   — or button or wheel, or the pointer moved more than [`MOVE_PX`] within
//!   one burst of movement. Yap's input hook reports it
//!   (`dictation/input_hook.rs`: never an injected event, never Yap's own
//!   marked ones), atomics only in its callback. The action in progress
//!   stops between two of its steps, what it held is let go, and the rest
//!   of that turn's input actions are refused: Claude waits for the
//!   person's next message.
//! - Only while Yap is driving ([`ARMED`]): the rest of the time the hook
//!   does nothing more than it did.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a chat keeps the PC after its last input action.
pub const HOLD: Duration = Duration::from_secs(10);
/// The pointer moved this far within one burst is the person's.
pub const MOVE_PX: i32 = 10;
/// Moves further apart than this start a new burst.
const BURST_MS: u64 = 500;

/// Yap is driving: the hook reports the person's input.
static ARMED: AtomicBool = AtomicBool::new(false);
/// A guided tour (teach mode): the person's hand is on the mouse, so a small
/// move doesn't stop a step's actions (a key, a button or the wheel still does).
static LENIENT: AtomicBool = AtomicBool::new(false);
/// The person's last input while armed (ms on the hook's clock) and what it was.
static TOUCH_MS: AtomicU64 = AtomicU64::new(0);
static TOUCH_KIND: AtomicU8 = AtomicU8::new(0);
/// The pointer's burst: where it started, and the last move's time.
static BURST_X: AtomicI32 = AtomicI32::new(0);
static BURST_Y: AtomicI32 = AtomicI32::new(0);
static LAST_MOVE_MS: AtomicU64 = AtomicU64::new(0);

/// What the person did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Touch {
    Esc,
    Key,
    Button,
    Wheel,
    Move,
}

impl Touch {
    fn code(self) -> u8 {
        match self {
            Touch::Esc => 1,
            Touch::Key => 2,
            Touch::Button => 3,
            Touch::Wheel => 4,
            Touch::Move => 5,
        }
    }
    fn of(code: u8) -> Option<Touch> {
        Some(match code {
            1 => Touch::Esc,
            2 => Touch::Key,
            3 => Touch::Button,
            4 => Touch::Wheel,
            5 => Touch::Move,
            _ => return None,
        })
    }
    /// For the model and the chip.
    pub fn words(self) -> &'static str {
        match self {
            Touch::Esc => "pressed Esc",
            Touch::Key => "pressed a key",
            Touch::Button => "clicked",
            Touch::Wheel => "scrolled",
            Touch::Move => "moved the mouse",
        }
    }
}

fn now_ms() -> u64 {
    crate::dictation::gesture::now_ms()
}

fn touched(kind: Touch) {
    // Esc wins over whatever came just before it.
    if TOUCH_KIND.load(Ordering::Relaxed) != Touch::Esc.code() || kind == Touch::Esc || TOUCH_MS.load(Ordering::Relaxed) + 1000 < now_ms() {
        TOUCH_KIND.store(kind.code(), Ordering::Relaxed);
    }
    TOUCH_MS.store(now_ms(), Ordering::Relaxed);
}

/// Moves of the mouse don't stop a guided tour's actions (`on`), or do again.
pub fn lenient(on: bool) {
    LENIENT.store(on, Ordering::Relaxed);
}

/// **The hook heard a real key** (not injected, not Yap's). Atomics only —
/// and Esc answers a guided tour's tooltip waiting (its Exit).
pub fn person_key(vkey: u32, down: bool) {
    if down && vkey == 0x1B && super::teach::tip_up() {
        super::teach::press(super::teach::Pressed::Exit);
    }
    if !down || !ARMED.load(Ordering::Relaxed) {
        return;
    }
    touched(if vkey == 0x1B { Touch::Esc } else { Touch::Key });
}

/// Whether a burst of moves from `start` to `at` is the person's. Pure.
pub fn moved_far(start: (i32, i32), at: (i32, i32)) -> bool {
    (at.0 - start.0).abs().max((at.1 - start.1).abs()) > MOVE_PX
}

/// **The hook heard a real mouse event** (`msg`: its window message; the
/// pointer at `x`, `y`). Atomics only.
pub fn person_mouse(msg: u32, x: i32, y: i32) {
    if !ARMED.load(Ordering::Relaxed) {
        return;
    }
    match msg {
        // Moves: a new burst after a pause; far enough within one, the person's.
        0x0200 => {
            let now = now_ms();
            if now.saturating_sub(LAST_MOVE_MS.load(Ordering::Relaxed)) > BURST_MS {
                BURST_X.store(x, Ordering::Relaxed);
                BURST_Y.store(y, Ordering::Relaxed);
            }
            LAST_MOVE_MS.store(now, Ordering::Relaxed);
            if moved_far((BURST_X.load(Ordering::Relaxed), BURST_Y.load(Ordering::Relaxed)), (x, y)) {
                touched(Touch::Move);
            }
        }
        // Any button going down.
        0x0201 | 0x0204 | 0x0207 | 0x020B => touched(Touch::Button),
        0x020A | 0x020E => touched(Touch::Wheel),
        _ => {}
    }
}

/// **Test mode: the person's input, pretended** ("esc", "key", "button",
/// "wheel", "move"): as the hook would report it, only while Yap drives.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_computer_touch(kind: String) -> Result<bool, String> {
    if !crate::shell::e2e::active() {
        return Err("e2e_computer_touch is for test runs".into());
    }
    let t = match kind.as_str() {
        "esc" => Touch::Esc,
        "key" => Touch::Key,
        "button" => Touch::Button,
        "wheel" => Touch::Wheel,
        _ => Touch::Move,
    };
    // Esc on a guided tour's tooltip: its Exit, as the hook's Esc is.
    if t == Touch::Esc && super::teach::tip_up() {
        return Ok(super::teach::press(super::teach::Pressed::Exit));
    }
    let armed = ARMED.load(Ordering::Relaxed);
    if armed {
        touched(t);
    }
    Ok(armed)
}

/// Who holds the PC: the chat, and when its last action ended.
struct Holder {
    conversation: Option<u64>,
    last: Instant,
}

static HOLDER: Mutex<Option<Holder>> = Mutex::new(None);
/// The turns the person stopped (the latest few): their input actions are refused.
static STOPPED: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// **An input action under way**: the PC held, the watch armed, and its
/// stop — the person's input since it began, or the turn ending (dropped).
pub struct Driving {
    began: u64,
    turn: String,
    halted: Arc<AtomicBool>,
}

/// **The person's input since an action began**, for the blocking steps
/// to read between them.
#[derive(Clone, Copy)]
pub struct Probe {
    began: u64,
}

impl Probe {
    pub fn stopped(self) -> bool {
        TOUCH_MS.load(Ordering::Relaxed) > self.began
    }
}

impl Driving {
    /// The person took over since this began: what they did.
    pub fn touched(&self) -> Option<Touch> {
        (TOUCH_MS.load(Ordering::Relaxed) > self.began).then(|| Touch::of(TOUCH_KIND.load(Ordering::Relaxed))).flatten()
    }

    /// What the blocking steps read between them.
    pub fn probe(&self) -> Probe {
        Probe { began: self.began }
    }

    /// The flag the blocking steps read: set when the turn's future goes
    /// (■ Stop, a newer question) or the person takes over.
    pub fn halted(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.halted)
    }

    /// Stopped by the person: this turn's input actions end here.
    pub fn stopped_by_person(&self) {
        let mut s = STOPPED.lock().unwrap_or_else(|p| p.into_inner());
        if !s.contains(&self.turn) {
            s.push(self.turn.clone());
            let n = s.len();
            if n > 32 {
                s.drain(..n - 32);
            }
        }
    }
}

impl Drop for Driving {
    fn drop(&mut self) {
        // A turn's future dropped mid-action (■): the blocking steps stop too.
        self.halted.store(true, Ordering::Relaxed);
        if let Some(h) = HOLDER.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
            h.last = Instant::now();
        }
    }
}

/// What may refuse an input action before anything's sent. Pure.
pub fn refused(holder: Option<(Option<u64>, Duration)>, conversation: Option<u64>, stopped: bool) -> Option<(&'static str, &'static str)> {
    if stopped {
        return Some((
            "The user took over the mouse or keyboard during this turn, so Yap stopped. Don't use their apps again until they ask: say what's left to do.",
            "Didn't act: you took over",
        ));
    }
    if let Some((other, since)) = holder {
        if other != conversation && since < HOLD {
            return Some(("Another chat is using the user's PC right now. Say so, and try again once it's done.", "Didn't act: another chat is using your PC"));
        }
    }
    None
}

/// **Takes the PC for an input action** of `conversation`'s turn `turn`:
/// refused when another chat holds it or the person stopped this turn.
pub fn begin(conversation: Option<u64>, turn: &str) -> Result<Driving, (&'static str, &'static str)> {
    let stopped = STOPPED.lock().unwrap_or_else(|p| p.into_inner()).iter().any(|t| t == turn);
    let mut holder = HOLDER.lock().unwrap_or_else(|p| p.into_inner());
    let held = holder.as_ref().map(|h| (h.conversation, h.last.elapsed()));
    if let Some(no) = refused(held, conversation, stopped) {
        return Err(no);
    }
    *holder = Some(Holder { conversation, last: Instant::now() });
    ARMED.store(true, Ordering::Relaxed);
    // (Movement before this action began is never its stop.)
    LAST_MOVE_MS.store(0, Ordering::Relaxed);
    Ok(Driving { began: now_ms(), turn: turn.to_string(), halted: Arc::new(AtomicBool::new(false)) })
}

/// The PC isn't driven any more (the indicator went): the hook stops
/// listening for the person.
pub fn disarm() {
    ARMED.store(false, Ordering::Relaxed);
}

/// Forgets who holds the PC and the stopped turns (a test run's new
/// stand-in desktop, and unit tests).
#[cfg(any(test, debug_assertions))]
pub fn reset() {
    *HOLDER.lock().unwrap_or_else(|p| p.into_inner()) = None;
    STOPPED.lock().unwrap_or_else(|p| p.into_inner()).clear();
    TOUCH_MS.store(0, Ordering::Relaxed);
    disarm();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_chat_holds_the_pc_and_a_stopped_turn_stays_stopped() {
        assert!(refused(None, Some(1), false).is_none());
        assert!(refused(Some((Some(1), Duration::from_secs(1))), Some(1), false).is_none(), "its own chat");
        assert!(refused(Some((Some(2), Duration::from_secs(1))), Some(1), false).unwrap().0.contains("Another chat"));
        assert!(refused(Some((Some(2), HOLD + Duration::from_secs(1))), Some(1), false).is_none(), "let go after a while");
        assert!(refused(None, Some(1), true).unwrap().0.contains("took over"));
    }

    #[test]
    fn the_person_moving_the_mouse_far_enough_takes_over() {
        assert!(!moved_far((100, 100), (108, 95)));
        assert!(moved_far((100, 100), (100, 112)));
        assert!(moved_far((100, 100), (89, 100)));
    }

    /// Esc after an action began stops it; input before it never does; a
    /// stopped turn's next action is refused.
    #[test]
    fn esc_stops_what_began_before_it() {
        let _one = crate::computer::tests::LOCK.lock().unwrap_or_else(|p| p.into_inner());
        reset();
        touched(Touch::Key);
        std::thread::sleep(Duration::from_millis(5));
        let d = begin(Some(5), "turn-a").unwrap();
        assert_eq!(d.touched(), None, "a key before it began");
        std::thread::sleep(Duration::from_millis(5));
        person_key(0x1B, true);
        assert_eq!(d.touched(), Some(Touch::Esc));
        d.stopped_by_person();
        drop(d);
        assert!(begin(Some(5), "turn-a").is_err());
        assert!(begin(Some(5), "turn-b").is_ok(), "the next message's turn");
        reset();
    }
}

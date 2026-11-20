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
//!   does nothing more than it did. That's from an action's start until
//!   the bar's "Yap is using your PC" goes, so **between two actions too**:
//!   Esc pressed while Claude thinks about its next step stops that step
//!   (and the bar's line goes at once). (A real-API run on 20 Nov 2026
//!   found an Esc between actions lost: only one during an action counted.)
//! - **Never Yap's own cards**: while one waits for the person (the access
//!   card, an ask-first card, a question: `chat/pages/act/confirm.rs`),
//!   their clicks and keys answer it, and none of it before the card went
//!   ever stops what comes after. Without that, the mouse moving to a Send
//!   card's Allow stopped the very action it allowed.
//! - **A move of the mouse** counts only while an action runs (when Yap's
//!   pointer and theirs would fight), never on a guided tour (the hand on
//!   the mouse after Next) nor as the hand leaves a card just answered.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicU8, Ordering};
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
/// Input actions running now (from `begin` until their [`Driving`] goes).
static ACTIONS: AtomicU32 = AtomicU32::new(0);
/// The hand leaving a card just answered: moves this soon after aren't the person taking over.
const CARD_SETTLE_MS: u64 = 1_500;

/// **Yap's own cards waiting for the person** ([`card`]) and when the last
/// went (ms on the hook's clock; 0: none yet). The app's, for every thread;
/// a unit test's own thread's, so another test's card never reaches it.
mod cards {
    pub use imp::*;

    #[cfg(not(test))]
    mod imp {
        use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
        static WAITING: AtomicU32 = AtomicU32::new(0);
        static GONE: AtomicU64 = AtomicU64::new(0);
        pub fn waiting() -> u32 {
            WAITING.load(Ordering::Relaxed)
        }
        pub fn up() {
            WAITING.fetch_add(1, Ordering::Relaxed);
        }
        pub fn down(now: u64) {
            // (Each one up, then down: never below none.)
            WAITING.fetch_sub(1, Ordering::Relaxed);
            GONE.store(now, Ordering::Relaxed);
        }
        pub fn gone() -> u64 {
            GONE.load(Ordering::Relaxed)
        }
        #[cfg(debug_assertions)]
        pub fn reset() {
            GONE.store(0, Ordering::Relaxed);
        }
    }
    #[cfg(test)]
    mod imp {
        use std::cell::Cell;
        thread_local! {
            static WAITING: Cell<u32> = const { Cell::new(0) };
            static GONE: Cell<u64> = const { Cell::new(0) };
        }
        pub fn waiting() -> u32 {
            WAITING.with(Cell::get)
        }
        pub fn up() {
            WAITING.with(|w| w.set(w.get() + 1));
        }
        pub fn down(now: u64) {
            WAITING.with(|w| w.set(w.get().saturating_sub(1)));
            GONE.with(|g| g.set(now));
        }
        pub fn gone() -> u64 {
            GONE.with(Cell::get)
        }
        pub fn reset() {
            GONE.with(|g| g.set(0));
        }
    }
}

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

/// **Whether the person's `t` counts now**: Yap drives, no card of Yap's
/// waits, and a move only while an action runs — never on a guided tour,
/// nor as the hand leaves a card just answered. Atomics only.
fn counts(t: Touch, now: u64) -> bool {
    if !ARMED.load(Ordering::Relaxed) || cards::waiting() > 0 {
        return false;
    }
    let gone = cards::gone();
    t != Touch::Move || (ACTIONS.load(Ordering::Relaxed) > 0 && !LENIENT.load(Ordering::Relaxed) && (gone == 0 || now >= gone + CARD_SETTLE_MS))
}

/// **The person's input after `ms`** (never before a card of Yap's went),
/// and what it was.
fn touched_after(ms: u64) -> Option<Touch> {
    let since = ms.max(cards::gone());
    (TOUCH_MS.load(Ordering::Relaxed) > since).then(|| Touch::of(TOUCH_KIND.load(Ordering::Relaxed))).flatten()
}

/// The person took over after `ms` (the indicator's linger reads it: Esc
/// between two actions takes the bar's line away at once).
pub fn taken_over_since(ms: u64) -> bool {
    touched_after(ms).is_some()
}

/// The watch's clock (ms), for [`taken_over_since`].
pub fn clock() -> u64 {
    now_ms()
}

/// **A card of Yap's waiting for the person** (`chat/pages/act/confirm.rs`:
/// every card that asks): while one is up, the person's clicks and keys
/// answer it, never take over; dropped, it's gone.
pub struct CardUp(());

impl Drop for CardUp {
    fn drop(&mut self) {
        cards::down(now_ms().max(1));
        // The hand on its way back starts a burst of its own.
        LAST_MOVE_MS.store(0, Ordering::Relaxed);
    }
}

/// A card of Yap's shown: [`CardUp`] while it waits.
pub fn card() -> CardUp {
    cards::up();
    CardUp(())
}

/// **The hook heard a real key** (not injected, not Yap's). Atomics only —
/// and Esc answers a guided tour's tooltip waiting (its Exit).
pub fn person_key(vkey: u32, down: bool) {
    if down && vkey == 0x1B && super::teach::tip_up() {
        super::teach::press(super::teach::Pressed::Exit);
    }
    let t = if vkey == 0x1B { Touch::Esc } else { Touch::Key };
    if down && counts(t, now_ms()) {
        touched(t);
    }
}

/// Whether a burst of moves from `start` to `at` is the person's. Pure.
pub fn moved_far(start: (i32, i32), at: (i32, i32)) -> bool {
    (at.0 - start.0).abs().max((at.1 - start.1).abs()) > MOVE_PX
}

/// **The hook heard a real mouse event** (`msg`: its window message; the
/// pointer at `x`, `y`). Atomics only.
pub fn person_mouse(msg: u32, x: i32, y: i32) {
    let now = now_ms();
    match msg {
        // Moves: a new burst after a pause; far enough within one, the person's.
        0x0200 => {
            if !counts(Touch::Move, now) {
                return;
            }
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
        0x0201 | 0x0204 | 0x0207 | 0x020B if counts(Touch::Button, now) => touched(Touch::Button),
        0x020A | 0x020E if counts(Touch::Wheel, now) => touched(Touch::Wheel),
        _ => {}
    }
}

/// **Test mode: the person's input, pretended** ("esc", "key", "button",
/// "wheel", "move"): as the hook would report it, only while Yap drives
/// and no card of Yap's waits (a move only while an action runs). Whether
/// it counted.
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
    let counted = counts(t, now_ms());
    if counted {
        touched(t);
    }
    Ok(counted)
}

/// Who holds the PC: the chat and its turn, when its last action began
/// (ms) and when it ended.
struct Holder {
    conversation: Option<u64>,
    turn: String,
    began: u64,
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
        touched_after(self.began).is_some()
    }
}

impl Driving {
    /// The person took over since this began (never by answering a card of
    /// Yap's meanwhile, an ask-first card's Allow): what they did.
    pub fn touched(&self) -> Option<Touch> {
        touched_after(self.began)
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
        mark_stopped(&self.turn);
    }
}

/// `turn`'s input actions end here (the person took over).
fn mark_stopped(turn: &str) {
    let mut s = STOPPED.lock().unwrap_or_else(|p| p.into_inner());
    if !s.iter().any(|t| t == turn) {
        s.push(turn.to_string());
        let n = s.len();
        if n > 32 {
            s.drain(..n - 32);
        }
    }
}

impl Drop for Driving {
    fn drop(&mut self) {
        // A turn's future dropped mid-action (■): the blocking steps stop too.
        self.halted.store(true, Ordering::Relaxed);
        // (Each one `begin` counted.)
        ACTIONS.fetch_sub(1, Ordering::Relaxed);
        if let Some(h) = HOLDER.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
            h.last = Instant::now();
        }
    }
}

/// What the model and the chip are told when the person took over between
/// two of the turn's actions (the same words as during one, `act.rs`).
fn took_over(t: Touch) -> (&'static str, &'static str) {
    macro_rules! said {
        ($w:literal) => {
            (
                concat!("The user ", $w, " while Yap was working, so Yap stopped (nothing more was sent). Don't use their apps again until they ask: say what's left to do."),
                concat!("Stopped: you ", $w),
            )
        };
    }
    match t {
        Touch::Esc => said!("pressed Esc"),
        Touch::Key => said!("pressed a key"),
        Touch::Button => said!("clicked"),
        Touch::Wheel => said!("scrolled"),
        Touch::Move => said!("moved the mouse"),
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
/// refused when another chat holds it or the person stopped this turn —
/// during one of its actions, or **between two** (Esc while Claude thought
/// about this one, the bar still saying "Yap is using your PC").
pub fn begin(conversation: Option<u64>, turn: &str) -> Result<Driving, (&'static str, &'static str)> {
    let stopped = STOPPED.lock().unwrap_or_else(|p| p.into_inner()).iter().any(|t| t == turn);
    let mut holder = HOLDER.lock().unwrap_or_else(|p| p.into_inner());
    let held = holder.as_ref().map(|h| (h.conversation, h.last.elapsed()));
    if let Some(no) = refused(held, conversation, stopped) {
        return Err(no);
    }
    // The person's input since this turn's last action began, while the watch listened: theirs to stop.
    let between = holder.as_ref().filter(|h| h.conversation == conversation && h.turn == turn).and_then(|h| touched_after(h.began));
    if let Some(t) = between {
        mark_stopped(turn);
        disarm();
        return Err(took_over(t));
    }
    let began = now_ms();
    *holder = Some(Holder { conversation, turn: turn.to_string(), began, last: Instant::now() });
    ACTIONS.fetch_add(1, Ordering::Relaxed);
    ARMED.store(true, Ordering::Relaxed);
    // (Movement before this action began is never its stop.)
    LAST_MOVE_MS.store(0, Ordering::Relaxed);
    Ok(Driving { began, turn: turn.to_string(), halted: Arc::new(AtomicBool::new(false)) })
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
    TOUCH_KIND.store(0, Ordering::Relaxed);
    cards::reset();
    LENIENT.store(false, Ordering::Relaxed);
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

    /// Esc while Claude thinks between two actions (the watch still
    /// listening): the turn's next action is refused, in the same words as
    /// one stopped mid-action. (A real-API run, 20 Nov 2026, found it lost.)
    #[test]
    fn esc_between_two_actions_stops_the_next_one() {
        let _one = crate::computer::tests::LOCK.lock().unwrap_or_else(|p| p.into_inner());
        reset();
        drop(begin(Some(6), "turn-c").unwrap());
        std::thread::sleep(Duration::from_millis(5));
        person_key(0x1B, true);
        assert!(taken_over_since(0));
        let no = begin(Some(6), "turn-c").err().expect("refused");
        assert_eq!(no.1, "Stopped: you pressed Esc");
        assert!(no.0.contains("pressed Esc while Yap was working"));
        assert!(!ARMED.load(Ordering::Relaxed), "the watch stops listening");
        assert!(begin(Some(6), "turn-c").is_err(), "and the rest of the turn too");
        assert!(begin(Some(6), "turn-d").is_ok(), "the next message's turn");
        reset();
    }

    /// The person answering a card of Yap's (an ask-first card's Allow, the
    /// access card, a question) never takes over: not while it waits, not
    /// the action it allowed, not the turn's next.
    #[test]
    fn answering_a_card_of_yaps_never_takes_over() {
        let _one = crate::computer::tests::LOCK.lock().unwrap_or_else(|p| p.into_inner());
        reset();
        let d = begin(Some(7), "turn-e").unwrap();
        std::thread::sleep(Duration::from_millis(5));
        let card = card();
        person_mouse(0x0200, 100, 100);
        person_mouse(0x0200, 400, 300);
        person_mouse(0x0201, 400, 300);
        person_key(0x0D, true);
        assert_eq!(d.touched(), None, "the card's clicks and keys");
        drop(card);
        assert_eq!(d.touched(), None, "nor once it's gone");
        // The hand leaving the card: its moves don't count either.
        person_mouse(0x0200, 400, 300);
        person_mouse(0x0200, 800, 600);
        assert_eq!(d.touched(), None, "the hand leaving the card");
        drop(d);
        assert!(begin(Some(7), "turn-e").is_ok(), "the turn's next action");
        reset();
    }

    /// A move of the mouse counts while an action runs, never between two
    /// (Claude thinking) nor on a guided tour; a click between two does.
    #[test]
    fn moves_count_only_while_an_action_runs_and_never_on_a_tour() {
        let _one = crate::computer::tests::LOCK.lock().unwrap_or_else(|p| p.into_inner());
        reset();
        let d = begin(Some(8), "turn-f").unwrap();
        std::thread::sleep(Duration::from_millis(5));
        person_mouse(0x0200, 10, 10);
        person_mouse(0x0200, 200, 10);
        assert_eq!(d.touched(), Some(Touch::Move), "during the action");
        drop(d);
        reset();
        drop(begin(Some(8), "turn-g").unwrap());
        std::thread::sleep(Duration::from_millis(5));
        person_mouse(0x0200, 10, 10);
        person_mouse(0x0200, 200, 10);
        assert!(begin(Some(8), "turn-g").is_ok(), "a move between two actions");
        reset();
        lenient(true);
        let d = begin(Some(8), "turn-h").unwrap();
        std::thread::sleep(Duration::from_millis(5));
        person_mouse(0x0200, 10, 10);
        person_mouse(0x0200, 200, 10);
        assert_eq!(d.touched(), None, "a tour's step: the hand on the mouse after Next");
        person_mouse(0x0201, 200, 10);
        assert_eq!(d.touched(), Some(Touch::Button), "a click still stops it");
        drop(d);
        reset();
    }
}

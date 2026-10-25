//! What the input hook thinks is held, checked (pure, unit-tested): the
//! **stuck-key sweep**'s verdict ([`stuck`]) and the **chord** check
//! ([`another_key`]).
//!
//! **Keys stuck "down"** (Wispr Flow's stale-key checks, by behaviour): the
//! hook knows a key is down from its key-down, and up from its key-up. When
//! the key-up never reaches it — the lock screen or a UAC prompt took it
//! (another desktop: the hook hears nothing there), the PC went to sleep
//! with the key down, Windows dropped the hook for a moment — the binding
//! stays "active": push-to-talk keeps recording, and the key's next press
//! looks like a repeat, so a toggle or a chord never fires. A sweep on the
//! hook's own thread (every [`SWEEP_MS`], at once on a desktop switch or
//! after a sleep, and when the pipeline asks — a push-to-talk hold of a
//! minute, a dictation near its length limit) compares what the hook
//! thinks with what Windows says:
//!
//! - a key the hook doesn't swallow (a modifier-only chord, a modifier bound
//!   alone — RightAlt —, a combo's modifiers) is down for Windows or not
//!   (`GetAsyncKeyState`);
//! - a key the hook swallows (F9, a combo's main key, a mouse button) never
//!   shows in `GetAsyncKeyState` (measured: Windows updates the key state
//!   after the hooks, and a swallowed key never gets there), so only
//!   evidence counts: the keyboard repeating it while held stopped, with no
//!   other key pressed since (the keyboard repeats only the last key down;
//!   a key that never repeated — a mouse button, a key a mouse's software
//!   sends — is never judged by this); or [`Evidence::Away`]: the hook
//!   wasn't hearing (another desktop, a sleep, input Windows saw and the
//!   hook didn't).
//!
//! A stuck binding is let go **without its release event**: the sweep never
//! starts or stops a dictation. The pipeline is told (`key_lost`) and a
//! recording the key held goes on hands-free, the next press stopping it.

use super::{vk_family, KEY_TYPE_KEYBOARD, KEY_TYPE_MODS, KEY_TYPE_MOUSE};

/// How often the sweep runs on the hook's thread.
pub const SWEEP_MS: u32 = 5_000;
/// A press younger than this is never judged (Windows' key state may not
/// have caught up with the hook yet).
pub const GRACE_MS: u64 = 1_000;
/// A held key whose repeats stopped this long ago (none since, no other key
/// pressed) was let go: keyboards repeat every 33–400 ms once they start.
pub const REPEAT_GONE_MS: u64 = 2_000;
/// Windows saw input this much newer than anything the hook heard: the hook
/// isn't hearing (another desktop, or a hook Windows took away).
pub const DEAF_MS: u32 = 1_000;
/// The PC slept between two sweeps when the clock that counts sleep ran
/// this much further than the one that doesn't.
pub const SLEEP_MS: u64 = 2_000;

/// One active binding, as the sweep sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Held {
    /// Its key spec, parsed (`KEY_TYPE_*`, vkey or button, `MOD_*`).
    pub kind: u8,
    pub code: u32,
    pub mods: u8,
    /// How long ago its press fired (ms).
    pub age_ms: u64,
    /// How long ago its main key last repeated, if it repeated during this
    /// press.
    pub repeat_ago_ms: Option<u64>,
    /// Another key went down after that repeat (which stops the keyboard
    /// repeating this one, held or not).
    pub other_key_since: bool,
}

/// What Windows says now (`GetAsyncKeyState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Windows {
    /// The modifier families down (`MOD_*`).
    pub mods_down: u8,
    /// The binding's own key is down — asked only for a modifier bound
    /// alone (RightAlt), the one main key the hook never swallows.
    pub key_down: bool,
}

/// Why the sweep runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Evidence {
    /// The timer, or the pipeline asking: only what Windows' key state or
    /// the keyboard's repeats prove.
    Regular,
    /// The hook wasn't hearing for a while (a desktop switch — the lock
    /// screen, a UAC prompt —, a sleep, input it missed): any key-up may
    /// have been lost, so a key Windows can't vouch for counts as up.
    Away,
}

/// Why a binding was found stuck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// A modifier it needs is up for Windows.
    ModifiersUp,
    /// The modifier bound alone is up for Windows.
    KeyUp,
    /// The keyboard stopped repeating it, with no other key pressed.
    StoppedRepeating,
    /// The hook wasn't hearing, and nothing says it's still down.
    Away,
}

impl Why {
    pub fn describe(self) -> &'static str {
        match self {
            Why::ModifiersUp => "its modifiers are up",
            Why::KeyUp => "its key is up",
            Why::StoppedRepeating => "the keyboard stopped repeating it",
            Why::Away => "the hook missed input (another desktop, a sleep, or a lost hook)",
        }
    }
}

/// Is the active binding `h` stuck — let go without the hook hearing it?
/// `None`: held, or nothing proves otherwise.
pub fn stuck(h: &Held, windows: &Windows, evidence: Evidence) -> Option<Why> {
    if evidence == Evidence::Regular && h.age_ms < GRACE_MS {
        return None;
    }
    let mods_held = windows.mods_down & h.mods == h.mods;
    match h.kind {
        // Made of modifiers, never swallowed: Windows knows.
        KEY_TYPE_MODS => (!mods_held).then_some(Why::ModifiersUp),
        KEY_TYPE_KEYBOARD if h.mods == 0 && vk_family(h.code) != 0 => (!windows.key_down).then_some(Why::KeyUp),
        KEY_TYPE_KEYBOARD if !mods_held => Some(Why::ModifiersUp),
        KEY_TYPE_KEYBOARD | KEY_TYPE_MOUSE => swallowed(h, evidence),
        _ => None,
    }
}

/// A key the hook swallows: never in Windows' key state, so only evidence.
fn swallowed(h: &Held, evidence: Evidence) -> Option<Why> {
    if evidence == Evidence::Away {
        return Some(Why::Away);
    }
    match h.repeat_ago_ms {
        Some(ago) if h.kind == KEY_TYPE_KEYBOARD && ago >= REPEAT_GONE_MS && !h.other_key_since => {
            Some(Why::StoppedRepeating)
        }
        _ => None,
    }
}

/// Windows' last input (`GetLastInputInfo`, a tick count) is newer than the
/// last event the hook heard by more than [`DEAF_MS`]: the hook missed
/// input. Tick counts wrap after 49.7 days: compared as such.
pub fn missed_input(last_input: u32, heard: u32) -> bool {
    let ahead = last_input.wrapping_sub(heard);
    // A last input "behind" what the hook heard (a wrapped difference past
    // half the range) is the hook being ahead: never missed.
    ahead > DEAF_MS && ahead < u32::MAX / 2
}

/// Between two sweeps, the tick count (which counts sleep) ran `ticks` ms and
/// the unbiased interrupt time (which doesn't) `awake` ms: the PC slept.
pub fn slept(ticks: u64, awake: u64) -> bool {
    ticks.saturating_sub(awake) > SLEEP_MS
}

/// Is a key-down of `vk` (scan code `scan`) **another key** for an active
/// binding — one that makes it a chord the person meant as a shortcut? Not
/// its own main key (a repeat), not one of its modifiers (either side), not
/// AltGr's own left Ctrl (Windows sends a made-up one, scan code 0x21D,
/// with RightAlt on many layouts), not the menu-mask key Yap taps itself.
pub fn another_key(kind: u8, code: u32, mods: u8, vk: u32, scan: u32) -> bool {
    const VK_MENU_MASK: u32 = 0xE8;
    const ALTGR_CTRL_SCAN: u32 = 0x21D;
    if vk == VK_MENU_MASK || scan == ALTGR_CTRL_SCAN {
        return false;
    }
    let family = vk_family(vk);
    match kind {
        KEY_TYPE_KEYBOARD => {
            let own_family = vk_family(code);
            vk != code && (family == 0 || (mods & family == 0 && own_family != family))
        }
        KEY_TYPE_MODS => family == 0 || mods & family == 0,
        KEY_TYPE_MOUSE => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{MOD_ALT, MOD_CTRL, MOD_SHIFT, MOD_WIN};
    use super::*;

    fn held(kind: u8, code: u32, mods: u8) -> Held {
        Held { kind, code, mods, age_ms: 30_000, repeat_ago_ms: None, other_key_since: false }
    }
    const NOTHING: Windows = Windows { mods_down: 0, key_down: false };

    #[test]
    fn a_chord_or_a_modifier_bound_alone_follows_windows() {
        let chord = held(KEY_TYPE_MODS, 0, MOD_CTRL | MOD_WIN);
        assert_eq!(stuck(&chord, &NOTHING, Evidence::Regular), Some(Why::ModifiersUp));
        let ctrl_only = Windows { mods_down: MOD_CTRL, key_down: false };
        assert_eq!(stuck(&chord, &ctrl_only, Evidence::Regular), Some(Why::ModifiersUp));
        let both = Windows { mods_down: MOD_CTRL | MOD_WIN | MOD_SHIFT, key_down: false };
        assert_eq!(stuck(&chord, &both, Evidence::Regular), None);
        // Even when the hook was away: Windows vouches for it.
        assert_eq!(stuck(&chord, &both, Evidence::Away), None);

        // RightAlt (AltGr) bound alone: never swallowed, Windows knows it.
        let ralt = held(KEY_TYPE_KEYBOARD, 0xA5, 0);
        assert_eq!(stuck(&ralt, &Windows { mods_down: MOD_ALT, key_down: false }, Evidence::Regular), Some(Why::KeyUp));
        assert_eq!(stuck(&ralt, &Windows { mods_down: MOD_ALT, key_down: true }, Evidence::Regular), None);
    }

    #[test]
    fn a_young_press_is_never_judged_by_the_timer() {
        // Windows' key state may lag the hook by a moment.
        let chord = Held { age_ms: GRACE_MS - 1, ..held(KEY_TYPE_MODS, 0, MOD_CTRL | MOD_ALT) };
        assert_eq!(stuck(&chord, &NOTHING, Evidence::Regular), None);
        // …but a desktop switch is a desktop switch.
        assert_eq!(stuck(&chord, &NOTHING, Evidence::Away), Some(Why::ModifiersUp));
    }

    #[test]
    fn a_combo_whose_modifiers_are_up_is_stuck_its_main_key_needs_evidence() {
        // Ctrl+Shift+Space: Space is swallowed, Ctrl and Shift aren't.
        let combo = held(KEY_TYPE_KEYBOARD, 0x20, MOD_CTRL | MOD_SHIFT);
        assert_eq!(stuck(&combo, &Windows { mods_down: MOD_CTRL, key_down: false }, Evidence::Regular), Some(Why::ModifiersUp));
        let mods = Windows { mods_down: MOD_CTRL | MOD_SHIFT, key_down: false };
        assert_eq!(stuck(&combo, &mods, Evidence::Regular), None, "the modifiers are down; Space can't be asked");
        // The hook was away: Space's key-up may have been lost.
        assert_eq!(stuck(&combo, &mods, Evidence::Away), Some(Why::Away));
    }

    #[test]
    fn a_swallowed_key_is_stuck_when_its_repeats_stopped_with_no_other_key() {
        let f9 = held(KEY_TYPE_KEYBOARD, 0x78, 0);
        // Never repeated (a key a mouse's software sends, held): no verdict.
        assert_eq!(stuck(&f9, &NOTHING, Evidence::Regular), None);
        // Repeating a moment ago: held.
        let repeating = Held { repeat_ago_ms: Some(40), ..f9 };
        assert_eq!(stuck(&repeating, &NOTHING, Evidence::Regular), None);
        // Repeats stopped two seconds ago, nothing else pressed: let go.
        let gone = Held { repeat_ago_ms: Some(REPEAT_GONE_MS), ..f9 };
        assert_eq!(stuck(&gone, &NOTHING, Evidence::Regular), Some(Why::StoppedRepeating));
        // Another key went down since: that's why it stopped repeating.
        let typed = Held { other_key_since: true, ..gone };
        assert_eq!(stuck(&typed, &NOTHING, Evidence::Regular), None);
        // The hook was away: let go, repeats or not.
        assert_eq!(stuck(&f9, &NOTHING, Evidence::Away), Some(Why::Away));
    }

    #[test]
    fn a_mouse_button_is_only_ever_let_go_after_the_hook_was_away() {
        let x1 = held(KEY_TYPE_MOUSE, 4, 0);
        assert_eq!(stuck(&x1, &NOTHING, Evidence::Regular), None);
        // (A mouse button never repeats, whatever the field says.)
        let odd = Held { repeat_ago_ms: Some(60_000), ..x1 };
        assert_eq!(stuck(&odd, &NOTHING, Evidence::Regular), None);
        assert_eq!(stuck(&x1, &NOTHING, Evidence::Away), Some(Why::Away));
    }

    #[test]
    fn the_hook_missed_input_when_windows_saw_newer() {
        assert!(!missed_input(10_000, 10_000));
        assert!(!missed_input(10_000 + DEAF_MS, 10_000));
        assert!(missed_input(10_000 + DEAF_MS + 1, 10_000));
        // The hook a moment ahead of Windows' figure: fine.
        assert!(!missed_input(10_000, 10_050));
        // Across the tick count's wrap.
        assert!(missed_input(500, u32::MAX - 1_000));
        assert!(!missed_input(10, u32::MAX - 10));
    }

    #[test]
    fn a_sleep_shows_as_the_two_clocks_parting() {
        assert!(!slept(5_000, 5_000));
        assert!(!slept(5_000, 4_900));
        assert!(slept(65_000, 5_000));
        assert!(!slept(4_000, 5_000));
    }

    #[test]
    fn another_key_is_one_the_binding_does_not_own() {
        // F9 alone: any other key, modifiers included, is another key.
        assert!(another_key(KEY_TYPE_KEYBOARD, 0x78, 0, 0x41, 0x1E));
        assert!(another_key(KEY_TYPE_KEYBOARD, 0x78, 0, 0xA2, 0x1D));
        assert!(!another_key(KEY_TYPE_KEYBOARD, 0x78, 0, 0x78, 0x43), "its own repeat");
        // Ctrl+Win chord: D is another key (Ctrl+Win+D), either Ctrl isn't.
        let cw = MOD_CTRL | MOD_WIN;
        assert!(another_key(KEY_TYPE_MODS, 0, cw, 0x44, 0x20));
        assert!(!another_key(KEY_TYPE_MODS, 0, cw, 0xA3, 0x11D));
        assert!(!another_key(KEY_TYPE_MODS, 0, cw, 0x5C, 0x5C));
        assert!(another_key(KEY_TYPE_MODS, 0, cw, 0xA0, 0x2A), "Shift joining makes a bigger shortcut");
        // RightAlt (AltGr) alone: E is another key (AltGr+E = €); its own
        // family and the made-up left Ctrl that comes with it aren't.
        assert!(another_key(KEY_TYPE_KEYBOARD, 0xA5, 0, 0x45, 0x12));
        assert!(!another_key(KEY_TYPE_KEYBOARD, 0xA5, 0, 0xA4, 0x38));
        assert!(!another_key(KEY_TYPE_KEYBOARD, 0xA5, 0, 0xA2, 0x21D));
        // Ctrl+Shift+Space: its modifiers aren't, Alt is.
        let cs = MOD_CTRL | MOD_SHIFT;
        assert!(!another_key(KEY_TYPE_KEYBOARD, 0x20, cs, 0xA1, 0x36));
        assert!(another_key(KEY_TYPE_KEYBOARD, 0x20, cs, 0xA4, 0x38));
        // A mouse button held: any key.
        assert!(another_key(KEY_TYPE_MOUSE, 4, 0, 0x41, 0x1E));
        // The menu-mask key Yap taps itself never counts.
        assert!(!another_key(KEY_TYPE_KEYBOARD, 0x78, 0, 0xE8, 0));
    }
}

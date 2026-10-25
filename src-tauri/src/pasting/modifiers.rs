//! **Modifier keys around the keys Yap sends.** A dictation ended by a
//! chord (Ctrl+Alt+Space, a mouse button with Shift…) can leave a modifier
//! down as far as Windows knows — a key-up the hook swallowed, Win+L in
//! between, Sticky Keys — and then Yap's Ctrl+V reaches the app as
//! Ctrl+Alt+V (another shortcut, or nothing), Win+V opens clipboard history,
//! and typed text turns into Alt+letter menu keys.
//!
//! So every key sequence Yap sends (the paste keys, typing, the submit key,
//! edit mode's Ctrl+C) goes out with the modifiers sorted first ([`plan`],
//! [`around`], pure and unit-tested):
//!
//! - a modifier Windows says is down but the person isn't holding (stuck)
//!   is let go first, and stays up;
//! - a modifier the person **is** holding is let go for the sequence and
//!   pressed again straight after it, in the same `SendInput` call — which
//!   Windows never interleaves with other input — so no app ever sees it up
//!   while their finger is on it, and their own release later works as
//!   ever;
//! - letting go of Alt or Win taps the menu mask key first (an unassigned
//!   key, as `input_hook.rs` does for its combos), so that release opens
//!   neither the app's menu bar nor Start; pressing one again taps it
//!   after, for the person's own release later.
//!
//! What the person holds comes from the keyboard hook (`input_hook.rs`
//! calls [`note_key`] for every key event): each event that isn't injected
//! (`LLKHF_INJECTED`: Yap's own keys, another app's `SendInput`) sets or
//! clears its key's bit. The hook's word only counts for a key Windows also
//! says is down: one it saw go down but never come up (it was let go while
//! one of Yap's own windows had the focus, where the hook doesn't run) is
//! up. Without the hook (a test run) nothing counts as held; robot mode
//! can say what is ([`e2e_held_keys`]).

use std::sync::atomic::{AtomicU8, Ordering};

/// One bit per modifier key, left and right apart.
pub const L_SHIFT: u8 = 1 << 0;
pub const R_SHIFT: u8 = 1 << 1;
pub const L_CTRL: u8 = 1 << 2;
pub const R_CTRL: u8 = 1 << 3;
pub const L_ALT: u8 = 1 << 4;
pub const R_ALT: u8 = 1 << 5;
pub const L_WIN: u8 = 1 << 6;
pub const R_WIN: u8 = 1 << 7;
pub const ALT: u8 = L_ALT | R_ALT;
pub const WIN: u8 = L_WIN | R_WIN;

/// Each modifier's bit, its side-specific virtual key and whether it's an
/// extended key (the right Ctrl and Alt, both Win keys).
pub const KEYS: [(u8, u16, bool); 8] = [
    (L_SHIFT, 0xA0, false),
    (R_SHIFT, 0xA1, false),
    (L_CTRL, 0xA2, false),
    (R_CTRL, 0xA3, true),
    (L_ALT, 0xA4, false),
    (R_ALT, 0xA5, true),
    (L_WIN, 0x5B, true),
    (R_WIN, 0x5C, true),
];

/// The menu mask key: unassigned (AutoHotkey's vkE8; `input_hook.rs` taps
/// the same one). Pressed while Alt or Win is down, it makes their release
/// a combo's, not a lone tap that opens the menu bar or Start.
pub const MASK_VK: u16 = 0xE8;

/// The modifiers the person is holding, by the keyboard hook.
static HELD: AtomicU8 = AtomicU8::new(0);

/// A modifier's bit for a virtual key (the hook reports side-specific
/// codes for real keys); 0 for any other key.
pub fn bit_of(vk: u32) -> u8 {
    KEYS.iter().find(|(_, k, _)| u32::from(*k) == vk).map_or(0, |(bit, _, _)| *bit)
}

/// From the keyboard hook, for every key event: a real key (not
/// `injected`) going down or up. Cheap: one atomic operation.
pub fn note_key(vk: u32, down: bool, injected: bool) {
    let bit = bit_of(vk);
    if bit == 0 || injected {
        return;
    }
    if down {
        HELD.fetch_or(bit, Ordering::Relaxed);
    } else {
        HELD.fetch_and(!bit, Ordering::Relaxed);
    }
}

/// The modifiers the person is holding, by the hook (or robot mode's say).
pub fn held() -> u8 {
    #[cfg(debug_assertions)]
    if let Some(keys) = test_held() {
        return keys;
    }
    HELD.load(Ordering::Relaxed)
}

/// What to do with the modifiers around a sequence of keys.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// Let go before the keys: every modifier Windows says is down.
    pub release: u8,
    /// Pressed again after them: those the person is holding.
    pub restore: u8,
}

impl Plan {
    pub fn is_empty(self) -> bool {
        self.release == 0 && self.restore == 0
    }
}

/// The plan for modifiers Windows says are `down` while the person holds
/// `held` (the hook's word, only for keys that are down): everything down
/// goes up before Yap's keys — so Ctrl+V is Ctrl+V and typed letters are
/// letters — and what the person holds comes back down after them.
pub fn plan(down: u8, held: u8) -> Plan {
    Plan {
        release: down,
        restore: held & down,
    }
}

/// A key event Yap adds around its own: a virtual key, up or down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub vk: u16,
    pub up: bool,
    pub extended: bool,
}

/// The events before and after Yap's keys for `plan`: the mask key tapped,
/// then the releases (Alt or Win among them: the mask first); after, the
/// presses again, then the mask (Alt or Win among them).
pub fn around(plan: Plan) -> (Vec<Key>, Vec<Key>) {
    let mask = |out: &mut Vec<Key>| {
        out.push(Key { vk: MASK_VK, up: false, extended: false });
        out.push(Key { vk: MASK_VK, up: true, extended: false });
    };
    let mut before = Vec::new();
    if plan.release & (ALT | WIN) != 0 {
        mask(&mut before);
    }
    for (bit, vk, extended) in KEYS {
        if plan.release & bit != 0 {
            before.push(Key { vk, up: true, extended });
        }
    }
    let mut after = Vec::new();
    for (bit, vk, extended) in KEYS {
        if plan.restore & bit != 0 {
            after.push(Key { vk, up: false, extended });
        }
    }
    if plan.restore & (ALT | WIN) != 0 {
        mask(&mut after);
    }
    (before, after)
}

/// For the log: "Alt, Win".
pub fn names(bits: u8) -> String {
    let mut out: Vec<&str> = Vec::new();
    for (mask, name) in [(L_SHIFT | R_SHIFT, "Shift"), (L_CTRL | R_CTRL, "Ctrl"), (ALT, "Alt"), (WIN, "Win")] {
        if bits & mask != 0 {
            out.push(name);
        }
    }
    out.join(", ")
}

/// Wait (at most `max`) until the person has let go of every modifier —
/// a shortcut's own keys, before what it does sends keys. A key that's only
/// stuck isn't waited for: the keys Yap sends let go of it ([`plan`]).
/// Without the hook (a test run) there's nothing to wait for.
pub fn wait_until_let_go(max: std::time::Duration) {
    let started = std::time::Instant::now();
    while started.elapsed() < max {
        #[cfg(windows)]
        let held_now = held() & down_now();
        #[cfg(not(windows))]
        let held_now = 0u8;
        if held_now == 0 {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// The modifiers Windows says are down now (`GetAsyncKeyState`).
#[cfg(windows)]
pub fn down_now() -> u8 {
    #[link(name = "user32")]
    extern "system" {
        fn GetAsyncKeyState(vkey: i32) -> i16;
    }
    KEYS.iter()
        .filter(|(_, vk, _)| unsafe { GetAsyncKeyState(i32::from(*vk)) } < 0)
        .fold(0, |bits, (bit, _, _)| bits | bit)
}

// ---- Robot mode: what the person holds ----

/// Robot mode's say, `0x100 | keys` when set.
#[cfg(debug_assertions)]
static TEST_HELD: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);

#[cfg(debug_assertions)]
fn test_held() -> Option<u8> {
    let v = TEST_HELD.load(Ordering::SeqCst);
    (v & 0x100 != 0).then_some((v & 0xFF) as u8)
}

/// Test mode only (there's no keyboard hook): the modifiers the person is
/// holding, by name ("shift", "ctrl", "alt", "win": the left keys), or none
/// (`[]`); `null` hands it back to the hook.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_held_keys(keys: Option<Vec<String>>) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    let value = match &keys {
        None => 0,
        Some(list) => {
            let mut bits = 0u8;
            for k in list {
                bits |= match k.to_ascii_lowercase().as_str() {
                    "shift" => L_SHIFT,
                    "ctrl" => L_CTRL,
                    "alt" => L_ALT,
                    "win" => L_WIN,
                    other => return Err(format!("Unknown key {other}")),
                };
            }
            0x100 | u16::from(bits)
        }
    };
    tracing::info!(keys = ?keys, "e2e: the keys the person holds");
    TEST_HELD.store(value, Ordering::SeqCst);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stuck_modifier_is_let_go_and_stays_up() {
        // Alt down by Windows' word, nobody holding it.
        let p = plan(L_ALT, 0);
        assert_eq!(p, Plan { release: L_ALT, restore: 0 });
        let (before, after) = around(p);
        // The mask key first (the release opens no menu), then Alt up.
        assert_eq!(
            before,
            vec![
                Key { vk: MASK_VK, up: false, extended: false },
                Key { vk: MASK_VK, up: true, extended: false },
                Key { vk: 0xA4, up: true, extended: false },
            ]
        );
        assert!(after.is_empty());
    }

    #[test]
    fn a_held_modifier_comes_back_down_after_the_keys() {
        // The person's finger is on Win: up for Ctrl+V, down again after,
        // then the mask so their own release doesn't open Start.
        let p = plan(L_WIN, L_WIN);
        assert_eq!(p, Plan { release: L_WIN, restore: L_WIN });
        let (before, after) = around(p);
        assert_eq!(before.last(), Some(&Key { vk: 0x5B, up: true, extended: true }));
        assert_eq!(
            after,
            vec![
                Key { vk: 0x5B, up: false, extended: true },
                Key { vk: MASK_VK, up: false, extended: false },
                Key { vk: MASK_VK, up: true, extended: false },
            ]
        );
    }

    #[test]
    fn never_left_up_under_a_finger_and_never_pressed_for_nobody() {
        // Every combination of 8 keys down and held: whatever is held and
        // down is pressed again; nothing that isn't down is touched; nothing
        // held-but-up (the hook missed its release) is pressed.
        for down in 0..=255u8 {
            for held in [0u8, L_SHIFT, R_CTRL, L_ALT | R_WIN, 0xFF] {
                let p = plan(down, held);
                assert_eq!(p.release, down);
                assert_eq!(p.restore, held & down, "down {down:#x} held {held:#x}");
                let (before, after) = around(p);
                let ups = before.iter().filter(|k| k.vk != MASK_VK).count();
                let downs = after.iter().filter(|k| k.vk != MASK_VK).count();
                assert_eq!(ups, down.count_ones() as usize);
                assert_eq!(downs, (held & down).count_ones() as usize);
                assert!(before.iter().all(|k| k.up || k.vk == MASK_VK));
                assert!(after.iter().all(|k| !k.up || k.vk == MASK_VK));
            }
        }
    }

    #[test]
    fn shift_and_ctrl_alone_need_no_mask() {
        let (before, after) = around(plan(L_SHIFT | R_CTRL, R_CTRL));
        assert!(before.iter().all(|k| k.vk != MASK_VK));
        assert!(after.iter().all(|k| k.vk != MASK_VK));
        assert_eq!(before, vec![Key { vk: 0xA0, up: true, extended: false }, Key { vk: 0xA3, up: true, extended: true }]);
        assert_eq!(after, vec![Key { vk: 0xA3, up: false, extended: true }]);
        // Nothing down: nothing added.
        assert!(plan(0, 0xFF).is_empty());
        assert_eq!(around(plan(0, 0xFF)), (vec![], vec![]));
    }

    #[test]
    fn the_hook_tracks_real_keys_only() {
        // (The one test that uses the hook's table.)
        HELD.store(0, Ordering::Relaxed);
        note_key(0xA4, true, false); // the left Alt, by a finger
        note_key(0x5B, true, true); // Win, injected (another app's SendInput)
        note_key(0x41, true, false); // A: not a modifier
        assert_eq!(HELD.load(Ordering::Relaxed), L_ALT);
        note_key(0xA4, false, true); // an injected Alt up doesn't lift the finger
        assert_eq!(HELD.load(Ordering::Relaxed), L_ALT);
        note_key(0xA4, false, false);
        assert_eq!(HELD.load(Ordering::Relaxed), 0);
        assert_eq!(bit_of(0x12), 0, "the generic VK_MENU is no side's");
        assert_eq!(names(L_ALT | R_WIN | L_SHIFT), "Shift, Alt, Win");
    }
}

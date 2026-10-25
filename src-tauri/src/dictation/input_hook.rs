//! Unified input hook for the dictation, edit (Commands' rewrite), meeting,
//! Voice Assistant, Read aloud, "Use what I said", Snip to Chat, Paste last
//! transcript and Copy last transcript key bindings (the last four act on
//! their press only), up to `languages::MAX_SHORTCUTS` **language shortcuts**
//! (My languages: a slot each, [`configure_languages_from`]; press and
//! release, as `language-key` with the slot) and up to `transforms::MAX`
//! **saved transforms**' shortcuts (a slot each, [`configure_transforms_from`];
//! only their press, as `transform-key-pressed` with the slot). The snip shortcut (Win+Alt+S by
//! default) only acts on its press (`snip.rs`); while a snip's overlay is
//! up, Esc cancels it here too and goes no further ([`snip_escape`]).
//!
//! Installs both WH_KEYBOARD_LL and WH_MOUSE_LL hooks to capture:
//! - Keyboard keys (including those sent by mouse side buttons via driver software)
//! - Mouse extra buttons (XBUTTON1/2, middle)
//!
//! Configured keys are **suppressed** at the OS level (keyboard hooks only),
//! preventing "44444" in text fields when holding a bound key. Push-to-talk is
//! NOT a separate binding: the dictation key emits both press and release, and
//! the pipeline's `recording_mode` decides toggle vs hold. The meeting
//! shortcut (Win+Alt+M by default) only acts on its press (`meeting_guard`).
//! The assistant shortcut (unbound by default) records like the dictation
//! key, for a question to Yap's Chat (`assistant.rs`). The Read aloud
//! shortcut (unbound by default) only acts on its press: the selection in
//! the app you're in is read aloud (`read_aloud.rs`). No two bindings may
//! fire on one press ([`clashes`]: Settings refuses such a key, and setup /
//! `save_config` leave a clashing assistant or read-aloud shortcut unbound).
//!
//! A combo holding **Alt or Win** whose main key is swallowed taps an
//! unassigned "menu mask" key while the modifiers are still down
//! (AutoHotkey's `#MenuMaskKey`): otherwise letting go of Alt would open the
//! focused app's menu bar (Office shows its KeyTips) and Win the Start menu,
//! as neither saw another key in between.
//!
//! **Several shortcuts for one action** (2026-10-25, Wispr Flow's "Add
//! another"): each of the nine fixed actions has its first shortcut (the
//! config's `hotkey`, `editHotkey`…, as ever) and up to [`MORE`] more
//! (`moreHotkeys`, [`configure_more_from`]). They act as one key: the
//! action's press goes out when the first of them goes down and its release
//! when the last comes up, so holding F9 and a mouse button together is one
//! hold. The extras give way to every first shortcut and to earlier extras
//! ([`clashes`]); language shortcuts and transforms give way to them all.
//!
//! **What's held** ([`is_held`], [`held_keys`]): the hook keeps every key it
//! has seen go down and not up — swallowed ones too, which Windows' own key
//! state never shows. Two things read it: a key going down that wasn't held
//! is **another key**, which makes a hold-to-talk press under a second a
//! chord ([`held::another_key`] → the pipeline's `key_chord`: dropped
//! silently, the "hold only {key}" tip); and the **stuck-key sweep**
//! (`held.rs`): every [`held::SWEEP_MS`], at once on a desktop switch (the
//! lock screen, a UAC prompt) or after a sleep, and when the pipeline asks
//! ([`sweep_now`]), active bindings are checked against `GetAsyncKeyState`
//! and the evidence; a stuck one is let go **without** its release (the
//! pipeline's `key_stuck`: a recording it held goes on hands-free — the
//! sweep never starts or stops a dictation).
//!
//! **Robot mode** (`realtest.rs`, debug builds): the hook runs too, but acts
//! only on keys the robot marks as its own ([`ROBOT_MARK`] in the event's
//! extra info); everything else passes by untouched, as if no hook were
//! there. [`UNHEARD_MARK`] is a key event the hook pretends it never heard
//! (a key-up lost to the lock screen, staged).

// FFI type names match Win32 API conventions (HHOOK, POINT, MSG, etc.)
#![allow(clippy::upper_case_acronyms)]

mod held;

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use tauri::AppHandle;
#[cfg(target_os = "windows")]
use tauri::Emitter;
use tracing::{info, trace};
#[cfg(target_os = "windows")]
use tracing::{error, warn};

// ---- Key binding types ----

const KEY_TYPE_NONE: u8 = 0;
const KEY_TYPE_KEYBOARD: u8 = 1;
const KEY_TYPE_MOUSE: u8 = 2;
/// Modifier-only chord (e.g. hold Ctrl+Alt) — no main key. Ported from
/// OpenWhispr's `windows-key-listener.c` `g_useModifiersOnly` mode.
const KEY_TYPE_MODS: u8 = 3;

// Modifier bitmask (matches the required-modifier flags in OpenWhispr's C
// listener: Ctrl / Alt / Shift / Win).
const MOD_CTRL: u8 = 1;
const MOD_ALT: u8 = 2;
const MOD_SHIFT: u8 = 4;
const MOD_WIN: u8 = 8;

/// A configurable key binding (dictation, edit, meeting, assistant, read).
struct KeyBinding {
    /// 0=none, 1=keyboard vkey, 2=mouse button, 3=modifier-only chord
    key_type: AtomicU8,
    /// Virtual key code (for keyboard) or button ID (for mouse); 0 for chords
    key_code: AtomicU32,
    /// Required modifiers (MOD_* bitmask). "At least these held" — extra
    /// modifiers are tolerated, same as OpenWhispr's listener.
    mods: AtomicU8,
    /// Whether the key is currently pressed (for repeat suppression)
    active: AtomicBool,
    /// When its press fired (`gesture::now_ms`).
    since: AtomicU64,
    /// When its main key last went down again while held (the keyboard
    /// repeating it); 0: not yet in this press. The stuck-key sweep's
    /// evidence (`held.rs`).
    repeat_at: AtomicU64,
    /// This press already grew into a chord (said once).
    chorded: AtomicBool,
}

impl KeyBinding {
    const fn new() -> Self {
        Self {
            key_type: AtomicU8::new(KEY_TYPE_NONE),
            key_code: AtomicU32::new(0),
            mods: AtomicU8::new(0),
            active: AtomicBool::new(false),
            since: AtomicU64::new(0),
            repeat_at: AtomicU64::new(0),
            chorded: AtomicBool::new(false),
        }
    }

    fn configure(&self, key_type: u8, key_code: u32, mods: u8) {
        // The same key again (every save applies them all): a press held
        // through the save keeps its release.
        if self.kind_code_mods() == (key_type, key_code, mods) {
            return;
        }
        self.active.store(false, Ordering::Relaxed);
        self.key_type.store(key_type, Ordering::Release);
        self.key_code.store(key_code, Ordering::Release);
        self.mods.store(mods, Ordering::Release);
    }

    fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    fn matches_mouse(&self, button_id: u32) -> bool {
        self.key_type.load(Ordering::Acquire) == KEY_TYPE_MOUSE
            && self.key_code.load(Ordering::Acquire) == button_id
    }

    fn kind_code_mods(&self) -> (u8, u32, u8) {
        (
            self.key_type.load(Ordering::Acquire),
            self.key_code.load(Ordering::Acquire),
            self.mods.load(Ordering::Acquire),
        )
    }

    fn type_and_code(&self) -> (u8, u32) {
        (
            self.key_type.load(Ordering::Acquire),
            self.key_code.load(Ordering::Acquire),
        )
    }
}

// ---- Shared state ----

/// The nine fixed actions, in the order they give way to each other: each
/// one's index into [`FIXED`], [`FIXED_KEYS`], [`FIXED_NAMES`] and
/// [`FIXED_EVENTS`].
pub(crate) const DICTATION: usize = 0;
const EDIT: usize = 1;
const MEETING: usize = 2;
const ASSISTANT: usize = 3;
const READ: usize = 4;
/// "Use what I said" (`as_said.rs`): swap the last cleaned dictation's
/// paste for the words as said. Unbound by default.
const AS_SAID: usize = 5;
/// Snip to Chat (`snip.rs`): drag a box over any screen. Win+Alt+S by
/// default; its press only.
const SNIP: usize = 6;
/// Paste last transcript (`pasting/last.rs`): the latest dictation again,
/// where you're typing. Unbound by default; its press only.
const PASTE_LAST: usize = 7;
/// Copy last transcript (`pasting/last.rs`): the latest dictation onto the
/// clipboard. Unbound by default; its press only.
const COPY_LAST: usize = 8;
/// How many fixed actions there are.
pub const FIXED_COUNT: usize = 9;
/// Each fixed action's config key — its first shortcut's field, as
/// Settings and `moreHotkeys` name it.
pub const FIXED_KEYS: [&str; FIXED_COUNT] = [
    "hotkey",
    "editHotkey",
    "meetingHotkey",
    "assistantHotkey",
    "readHotkey",
    "asSaidHotkey",
    "snipHotkey",
    "pasteLastHotkey",
    "copyLastHotkey",
];
/// …its name in messages ("clashes with the dictation hotkey").
pub const FIXED_NAMES: [&str; FIXED_COUNT] = [
    "dictation hotkey",
    "rewrite shortcut",
    "meeting shortcut",
    "assistant shortcut",
    "read-aloud shortcut",
    "Use-what-I-said shortcut",
    "snip shortcut",
    "Paste-last shortcut",
    "Copy-last shortcut",
];
/// …and its events (pressed, released).
const FIXED_EVENTS: [(&str, &str); FIXED_COUNT] = [
    ("dictation-key-pressed", "dictation-key-released"),
    ("edit-key-pressed", "edit-key-released"),
    ("meeting-key-pressed", "meeting-key-released"),
    ("assistant-key-pressed", "assistant-key-released"),
    ("read-key-pressed", "read-key-released"),
    ("as-said-key-pressed", "as-said-key-released"),
    ("snip-key-pressed", "snip-key-released"),
    ("paste-last-key-pressed", "paste-last-key-released"),
    ("copy-last-key-pressed", "copy-last-key-released"),
];
/// The actions a key is held for (push-to-talk): a chord or a stuck key
/// matters to them.
const HOLDS: [usize; 3] = [DICTATION, EDIT, ASSISTANT];
/// Each fixed action's first shortcut.
static FIXED: [KeyBinding; FIXED_COUNT] = [const { KeyBinding::new() }; FIXED_COUNT];
/// More shortcuts for one action, beside its first ("+ Add another").
pub const MORE: usize = 3;
/// Each fixed action's more shortcuts ([`configure_more_from`]).
static MORE_BINDINGS: [[KeyBinding; MORE]; FIXED_COUNT] = [const { [const { KeyBinding::new() }; MORE] }; FIXED_COUNT];

/// Fixed action `i`'s shortcuts: its first, then its more.
fn group(i: usize) -> impl Iterator<Item = &'static KeyBinding> {
    std::iter::once(&FIXED[i]).chain(MORE_BINDINGS[i].iter())
}
/// Saved transforms' shortcuts (`transforms.rs`): a slot each, in the
/// list's order (`transforms::assign`); only the press is used. Beside the
/// nine fixed bindings, always giving way to them.
static TRANSFORM_BINDINGS: [KeyBinding; crate::dictation::transforms::MAX] = [const { KeyBinding::new() }; crate::dictation::transforms::MAX];
/// Which transform each slot runs (its id). Read by the forwarder thread,
/// never the hook callback.
static TRANSFORM_IDS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
/// The slots' events (pressed, released). The forwarder turns a press into
/// `transform-key-pressed` with its slot; releases go nowhere.
const TRANSFORM_EVENTS: [(&str, &str); crate::dictation::transforms::MAX] = [
    ("transform-key-0", "transform-key-0-up"),
    ("transform-key-1", "transform-key-1-up"),
    ("transform-key-2", "transform-key-2-up"),
    ("transform-key-3", "transform-key-3-up"),
    ("transform-key-4", "transform-key-4-up"),
    ("transform-key-5", "transform-key-5-up"),
    ("transform-key-6", "transform-key-6-up"),
    ("transform-key-7", "transform-key-7-up"),
    ("transform-key-8", "transform-key-8-up"),
    ("transform-key-9", "transform-key-9-up"),
    ("transform-key-10", "transform-key-10-up"),
    ("transform-key-11", "transform-key-11-up"),
];
/// Language shortcuts' keys (`languages.rs`, My languages): a slot each, in
/// the list's order (`languages::assign`); press and release, as the
/// dictation key's (push-to-talk holds them). Beside the nine fixed
/// bindings, always giving way to them; saved transforms give way to these.
static LANGUAGE_BINDINGS: [KeyBinding; crate::dictation::languages::MAX_SHORTCUTS] =
    [const { KeyBinding::new() }; crate::dictation::languages::MAX_SHORTCUTS];
/// Which language shortcut each slot is (its id).
static LANGUAGE_IDS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
/// The slots' events (pressed, released); the forwarder turns both into one
/// `language-key` event with the slot.
const LANGUAGE_EVENTS: [(&str, &str); crate::dictation::languages::MAX_SHORTCUTS] = [
    ("language-key-0", "language-key-0-up"),
    ("language-key-1", "language-key-1-up"),
    ("language-key-2", "language-key-2-up"),
    ("language-key-3", "language-key-3-up"),
];
/// A snip's overlay is up: Esc cancels it here too, in case Windows didn't
/// let the overlay take the focus (`snip::cancel`).
static SNIP_ESC: AtomicBool = AtomicBool::new(false);
/// Not an event: Esc while a snip is up (the forwarder cancels it).
#[cfg(target_os = "windows")]
const SNIP_ESCAPE: &str = "input-hook:snip-escape";

/// Channel from the hook callback to the emit-forwarder thread. The callback
/// must NEVER do slow work (like `app.emit`, which can block on a busy webview):
/// Windows silently REMOVES a low-level hook whose callback exceeds the
/// LowLevelHooksTimeout (~300 ms) — the hotkey then goes dead with no error.
/// So the callback only does atomics + a non-blocking channel send. Each
/// event goes with the moment the key moved (`gesture::now_ms`), its payload:
/// push-to-talk times taps and double-taps from it.
#[cfg(target_os = "windows")]
static EVENT_TX: std::sync::OnceLock<std::sync::mpsc::Sender<(&'static str, u64)>> =
    std::sync::OnceLock::new();

/// Not an event: asks the forwarder thread to tap the menu mask key (see
/// `send_menu_mask`) — SendInput is kept out of the hook callback too.
#[cfg(target_os = "windows")]
const MENU_MASK: &str = "input-hook:menu-mask";
/// Not an event: a hold-to-talk key's press under a second grew into a
/// chord (its press's time with it); the forwarder tells the pipeline.
#[cfg(target_os = "windows")]
const CHORD: &str = "input-hook:chord";
/// Not an event: the sweep let go of a hold-to-talk key that was stuck
/// "down"; the forwarder tells the pipeline.
#[cfg(target_os = "windows")]
const STUCK: &str = "input-hook:stuck";

/// Every key the hook has seen go down and not up (a bit per virtual key):
/// swallowed ones too, which Windows' key state never shows.
static HELD: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];
/// When a key last went down that wasn't held already (`gesture::now_ms`):
/// the keyboard stops repeating an older key then.
static LAST_NEW_KEY_AT: AtomicU64 = AtomicU64::new(0);
/// The tick count (`GetTickCount`) of the last input the hook heard, to
/// compare with Windows' own last input (`held::missed_input`).
static HEARD_TICK: AtomicU32 = AtomicU32::new(0);
/// The hook's thread (its id), for [`sweep_now`].
static HOOK_THREAD: AtomicU32 = AtomicU32::new(0);
/// Robot mode: act only on the robot's own keys ([`ROBOT_MARK`]).
static ROBOT_ONLY: AtomicBool = AtomicBool::new(false);
/// A key event the robot sent for the hook to act on (its extra info).
pub const ROBOT_MARK: usize = 0x5941_5052;
/// A key event the robot sent for the hook to miss (a key-up the lock
/// screen took): it passes by unheard.
pub const UNHEARD_MARK: usize = 0x5941_5055;

/// Whether the hook has `vk` down (pure bookkeeping, any thread).
pub fn is_held(vk: u32) -> bool {
    vk < 256 && HELD[(vk / 64) as usize].load(Ordering::Relaxed) & (1u64 << (vk % 64)) != 0
}

/// The keys the hook has down now (virtual keys), swallowed ones included.
/// Kept honest by the stuck-key sweep: a key Windows says is up, that no
/// binding is holding, is forgotten there.
pub fn held_keys() -> Vec<u32> {
    (0..256u32).filter(|&vk| is_held(vk)).collect()
}

/// Mark `vk` down or up; whether it was down before.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn set_held(vk: u32, down: bool) -> bool {
    if vk >= 256 {
        return false;
    }
    let bit = 1u64 << (vk % 64);
    let word = &HELD[(vk / 64) as usize];
    let before = if down { word.fetch_or(bit, Ordering::Relaxed) } else { word.fetch_and(!bit, Ordering::Relaxed) };
    before & bit != 0
}

// ---- Public API ----

/// Parse a key spec and set fixed action `i`'s first shortcut.
fn configure_fixed(i: usize, what: &str, key_spec: &str) -> Result<String, String> {
    let (key_type, key_code, mods) = parse_key_spec(key_spec)?;
    FIXED[i].configure(key_type, key_code, mods);
    let desc = format_binding(key_type, key_code, mods);
    info!("{} key configured: {} (spec: {:?})", what, desc, key_spec);
    Ok(desc)
}

/// Fixed action `i`'s first shortcut in `cfg` (its old single field).
pub fn first_spec(cfg: &crate::config::YapConfig, i: usize) -> &str {
    match i {
        DICTATION => &cfg.hotkey,
        EDIT => &cfg.edit_hotkey,
        MEETING => &cfg.meeting_hotkey,
        ASSISTANT => &cfg.assistant_hotkey,
        READ => &cfg.read_hotkey,
        AS_SAID => &cfg.as_said_hotkey,
        SNIP => &cfg.snip_hotkey,
        PASTE_LAST => &cfg.paste_last_hotkey,
        COPY_LAST => &cfg.copy_last_hotkey,
        _ => "",
    }
}

/// Set fixed action `i`'s first shortcut in `cfg`.
fn set_first_spec(cfg: &mut crate::config::YapConfig, i: usize, spec: String) {
    let field = match i {
        DICTATION => &mut cfg.hotkey,
        EDIT => &mut cfg.edit_hotkey,
        MEETING => &mut cfg.meeting_hotkey,
        ASSISTANT => &mut cfg.assistant_hotkey,
        READ => &mut cfg.read_hotkey,
        AS_SAID => &mut cfg.as_said_hotkey,
        SNIP => &mut cfg.snip_hotkey,
        PASTE_LAST => &mut cfg.paste_last_hotkey,
        COPY_LAST => &mut cfg.copy_last_hotkey,
        _ => return,
    };
    *field = spec;
}

/// `moreHotkeys` tidied at load (idempotent, so not a one-time migration:
/// the first shortcut never moved, and a config from before has none): only
/// the nine actions' keys, no blanks, no repeats of the action's own keys,
/// at most [`MORE`] each — and an action whose first shortcut was taken away
/// by hand gets its next one moved up into it.
pub fn tidy_more(cfg: &mut crate::config::YapConfig) {
    let map = std::mem::take(&mut cfg.more_hotkeys);
    for (i, key) in FIXED_KEYS.iter().enumerate() {
        let Some(list) = map.get(*key) else { continue };
        let mut first = first_spec(cfg, i).trim().to_string();
        let mut kept: Vec<String> = Vec::new();
        for spec in list.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
            if first.is_empty() {
                first = spec.to_string();
                set_first_spec(cfg, i, first.clone());
            } else if spec != first && !kept.iter().any(|k| k == spec) && kept.len() < MORE {
                kept.push(spec.to_string());
            }
        }
        if !kept.is_empty() {
            cfg.more_hotkeys.insert((*key).to_string(), kept);
        }
    }
}

/// Fixed action `i`'s more shortcuts in `cfg` (`moreHotkeys`), as saved.
pub fn more_specs(cfg: &crate::config::YapConfig, i: usize) -> Vec<String> {
    FIXED_KEYS
        .get(i)
        .and_then(|key| cfg.more_hotkeys.get(*key))
        .map(|list| list.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).take(MORE).collect())
        .unwrap_or_default()
}

/// One more shortcut's place: its action, its slot, the key it gets (""
/// when it's left unbound) and why not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoreSlot {
    pub action: usize,
    pub slot: usize,
    pub spec: String,
    pub clash: Option<String>,
}

/// Which more shortcuts get their keys (pure): each gives way to every
/// first shortcut (`firsts`, by name) and to the more shortcuts before it —
/// its own action's included (two keys of one action firing on one press
/// would be one press twice). Settings never saves a clash; a hand-edited
/// config.json gets its clashing ones left unbound.
pub fn assign_more(firsts: &[(&str, &str)], more: &[Vec<String>]) -> Vec<MoreSlot> {
    let mut taken: Vec<(String, String)> = firsts
        .iter()
        .filter(|(_, spec)| !spec.trim().is_empty())
        .map(|(name, spec)| ((*name).to_string(), spec.trim().to_string()))
        .collect();
    let mut out = Vec::new();
    for (action, specs) in more.iter().enumerate().take(FIXED_COUNT) {
        for (slot, spec) in specs.iter().take(MORE).enumerate() {
            let spec = spec.trim();
            let clash = taken.iter().find(|(_, other)| clashes(spec, other)).map(|(name, _)| format!("the {name}"));
            if clash.is_none() && !spec.is_empty() {
                taken.push((FIXED_NAMES[action].to_string(), spec.to_string()));
            }
            out.push(MoreSlot {
                action,
                slot,
                spec: if clash.is_some() { String::new() } else { spec.to_string() },
                clash,
            });
        }
    }
    out
}

/// Every fixed action's more shortcuts from config ([`assign_more`]), at
/// setup and every save; the slots past an action's list unbound.
pub fn configure_more_from(cfg: &crate::config::YapConfig) {
    let firsts: Vec<(&str, &str)> = (0..FIXED_COUNT).map(|i| (FIXED_NAMES[i], first_spec(cfg, i))).collect();
    let more: Vec<Vec<String>> = (0..FIXED_COUNT).map(|i| more_specs(cfg, i)).collect();
    for row in MORE_BINDINGS.iter() {
        for b in row {
            b.configure(KEY_TYPE_NONE, 0, 0);
        }
    }
    let mut bound = 0usize;
    for slot in assign_more(&firsts, &more) {
        if let Some(clash) = &slot.clash {
            tracing::warn!("Another {} clashes with {clash}: left unbound", FIXED_NAMES[slot.action]);
        }
        match parse_key_spec(&slot.spec) {
            Ok((kind, code, mods)) => {
                MORE_BINDINGS[slot.action][slot.slot].configure(kind, code, mods);
                bound += usize::from(kind != KEY_TYPE_NONE);
            }
            Err(e) => tracing::warn!("Another {} can't be read: {e}", FIXED_NAMES[slot.action]),
        }
    }
    info!(bound, "More shortcuts configured");
}

/// Live-set fixed action `key`'s more shortcuts (Settings pauses one with
/// "" while a new key is picked, then puts the list back). Settings checks
/// clashes before it saves; the save applies [`configure_more_from`].
pub fn configure_more(key: &str, specs: &[String]) -> Result<(), String> {
    let i = FIXED_KEYS.iter().position(|k| *k == key).ok_or_else(|| format!("No such shortcut: {key}"))?;
    for (slot, b) in MORE_BINDINGS[i].iter().enumerate() {
        let spec = specs.get(slot).map(|s| s.trim()).unwrap_or("");
        let (kind, code, mods) = parse_key_spec(spec)?;
        b.configure(kind, code, mods);
    }
    Ok(())
}

/// Every fixed action's shortcuts as set now, in words, and whether each is
/// held — the e2e specs' and the robot's check (debug builds).
#[cfg(debug_assertions)]
pub fn binding_states() -> serde_json::Value {
    let row = |b: &KeyBinding| {
        let (kind, code, mods) = b.kind_code_mods();
        serde_json::json!({ "key": format_binding(kind, code, mods), "active": b.is_active() })
    };
    let actions: serde_json::Map<String, serde_json::Value> = (0..FIXED_COUNT)
        .map(|i| (FIXED_KEYS[i].to_string(), serde_json::Value::Array(group(i).map(row).collect())))
        .collect();
    serde_json::json!({ "actions": actions, "held": held_keys() })
}

/// Parse a key spec string and configure the dictation binding.
///
/// Formats:
/// - `"kb:52"` — keyboard virtual key code 52 (the "4" key)
/// - `"kb:ctrl+shift+32"` — modifier combo: Ctrl+Shift+Space
/// - `"kb:165"` — a single right-side modifier (VK_RMENU = RightAlt)
/// - `"mods:ctrl+alt"` — modifier-only chord (2+ modifiers, no main key)
/// - `"mouse:4"` — mouse button 4 (XBUTTON1 / back)
/// - `"MouseButton4"` — legacy format, equivalent to `"mouse:4"`
pub fn configure_dictation(key_spec: &str) -> Result<String, String> {
    configure_fixed(DICTATION, "Dictation", key_spec)
}

/// Parse a key spec string and configure the edit/rewrite-mode binding. An empty
/// spec unbinds it (edit mode is opt-in).
pub fn configure_edit(key_spec: &str) -> Result<String, String> {
    configure_fixed(EDIT, "Edit", key_spec)
}

/// Parse a key spec string and configure the meeting shortcut (start taking
/// notes / stop and summarise — `meeting_guard`). An empty spec unbinds it.
pub fn configure_meeting(key_spec: &str) -> Result<String, String> {
    configure_fixed(MEETING, "Meeting", key_spec)
}

/// Parse a key spec string and configure the **Voice Assistant** shortcut
/// (`assistant.rs`: a question to Yap's Chat, answered out loud). An empty
/// spec unbinds it (the default).
pub fn configure_assistant(key_spec: &str) -> Result<String, String> {
    configure_fixed(ASSISTANT, "Assistant", key_spec)
}

/// Parse a key spec string and configure the **Read aloud** shortcut
/// (`read_aloud.rs`: the selection in the app you're in, read aloud). An
/// empty spec unbinds it (the default).
pub fn configure_read(key_spec: &str) -> Result<String, String> {
    configure_fixed(READ, "Read-aloud", key_spec)
}

/// The read-aloud shortcut from config, unless it clashes with one of the
/// others (a hand-edited config.json: Settings never saves one), in which
/// case it stays unbound and the log says why.
pub fn configure_read_from(cfg: &crate::config::YapConfig) {
    let spec = cfg.read_hotkey.trim();
    let others = [
        ("dictation hotkey", cfg.hotkey.as_str()),
        ("rewrite shortcut", cfg.edit_hotkey.as_str()),
        ("meeting shortcut", cfg.meeting_hotkey.as_str()),
        ("assistant shortcut", cfg.assistant_hotkey.as_str()),
    ];
    let spec = match others.iter().find(|(_, other)| clashes(spec, other)) {
        Some((name, _)) => {
            tracing::warn!("The read-aloud shortcut {spec:?} clashes with the {name}: left unbound");
            ""
        }
        None => spec,
    };
    if let Err(e) = configure_read(spec) {
        tracing::warn!("Failed to apply the read-aloud shortcut: {}", e);
    }
}

/// The assistant shortcut from config, unless it clashes with one of the
/// other three (a hand-edited config.json: Settings never saves one), in
/// which case it stays unbound and the log says why.
pub fn configure_assistant_from(cfg: &crate::config::YapConfig) {
    let spec = cfg.assistant_hotkey.trim();
    let others = [
        ("dictation hotkey", cfg.hotkey.as_str()),
        ("rewrite shortcut", cfg.edit_hotkey.as_str()),
        ("meeting shortcut", cfg.meeting_hotkey.as_str()),
    ];
    let spec = match others.iter().find(|(_, other)| clashes(spec, other)) {
        Some((name, _)) => {
            tracing::warn!("The assistant shortcut {spec:?} clashes with the {name}: left unbound");
            ""
        }
        None => spec,
    };
    if let Err(e) = configure_assistant(spec) {
        tracing::warn!("Failed to apply the assistant shortcut: {}", e);
    }
}

/// Parse a key spec string and configure the **"Use what I said"**
/// shortcut (`as_said.rs`). An empty spec unbinds it (the default).
pub fn configure_as_said(key_spec: &str) -> Result<String, String> {
    configure_fixed(AS_SAID, "Use-what-I-said", key_spec)
}

/// The "Use what I said" shortcut from config, unless it clashes with one
/// of the other five (a hand-edited config.json: Settings never saves one;
/// it gives way to Read aloud's, the older one).
pub fn configure_as_said_from(cfg: &crate::config::YapConfig) {
    let spec = cfg.as_said_hotkey.trim();
    let others = [
        ("dictation hotkey", cfg.hotkey.as_str()),
        ("rewrite shortcut", cfg.edit_hotkey.as_str()),
        ("meeting shortcut", cfg.meeting_hotkey.as_str()),
        ("assistant shortcut", cfg.assistant_hotkey.as_str()),
        ("read-aloud shortcut", cfg.read_hotkey.as_str()),
    ];
    let spec = match others.iter().find(|(_, other)| clashes(spec, other)) {
        Some((name, _)) => {
            tracing::warn!("The Use-what-I-said shortcut {spec:?} clashes with the {name}: left unbound");
            ""
        }
        None => spec,
    };
    if let Err(e) = configure_as_said(spec) {
        tracing::warn!("Failed to apply the Use-what-I-said shortcut: {}", e);
    }
}

/// Parse a key spec string and configure the **Snip to Chat** shortcut
/// (`snip.rs`). An empty spec unbinds it.
pub fn configure_snip(key_spec: &str) -> Result<String, String> {
    configure_fixed(SNIP, "Snip", key_spec)
}

/// The snip shortcut from config, unless it clashes with one of the other
/// six (a hand-edited config.json, or a shortcut someone had on Win+Alt+S
/// before snips existed: theirs stays, the snip's is left unbound and the
/// log says why).
pub fn configure_snip_from(cfg: &crate::config::YapConfig) {
    let spec = cfg.snip_hotkey.trim();
    let others = [
        ("dictation hotkey", cfg.hotkey.as_str()),
        ("rewrite shortcut", cfg.edit_hotkey.as_str()),
        ("meeting shortcut", cfg.meeting_hotkey.as_str()),
        ("assistant shortcut", cfg.assistant_hotkey.as_str()),
        ("read-aloud shortcut", cfg.read_hotkey.as_str()),
        ("Use-what-I-said shortcut", cfg.as_said_hotkey.as_str()),
    ];
    let spec = match others.iter().find(|(_, other)| clashes(spec, other)) {
        Some((name, _)) => {
            tracing::warn!("The snip shortcut {spec:?} clashes with the {name}: left unbound");
            ""
        }
        None => spec,
    };
    if let Err(e) = configure_snip(spec) {
        tracing::warn!("Failed to apply the snip shortcut: {}", e);
    }
}

/// Parse a key spec string and configure the **Paste last transcript**
/// shortcut (`pasting/last.rs`). An empty spec unbinds it (the default).
pub fn configure_paste_last(key_spec: &str) -> Result<String, String> {
    configure_fixed(PASTE_LAST, "Paste-last", key_spec)
}

/// Parse a key spec string and configure the **Copy last transcript**
/// shortcut (`pasting/last.rs`). An empty spec unbinds it (the default).
pub fn configure_copy_last(key_spec: &str) -> Result<String, String> {
    configure_fixed(COPY_LAST, "Copy-last", key_spec)
}

/// The shortcuts Paste last and Copy last give way to, by name, oldest
/// first: the seven before them, then Paste last's (Copy last's only).
fn older_shortcuts(cfg: &crate::config::YapConfig) -> [(&'static str, &str); 8] {
    [
        ("dictation hotkey", cfg.hotkey.as_str()),
        ("rewrite shortcut", cfg.edit_hotkey.as_str()),
        ("meeting shortcut", cfg.meeting_hotkey.as_str()),
        ("assistant shortcut", cfg.assistant_hotkey.as_str()),
        ("read-aloud shortcut", cfg.read_hotkey.as_str()),
        ("Use-what-I-said shortcut", cfg.as_said_hotkey.as_str()),
        ("snip shortcut", cfg.snip_hotkey.as_str()),
        ("Paste-last shortcut", cfg.paste_last_hotkey.as_str()),
    ]
}

/// `spec`, unless it clashes with one of `others` (a hand-edited
/// config.json: Settings never saves a clash) — then "" and the log says
/// which.
fn unless_clash<'a>(what: &str, spec: &'a str, others: &[(&str, &str)]) -> &'a str {
    match others.iter().find(|(_, other)| clashes(spec, other)) {
        Some((name, _)) => {
            tracing::warn!("The {what} {spec:?} clashes with the {name}: left unbound");
            ""
        }
        None => spec,
    }
}

/// The Paste last shortcut from config, unless it clashes with one of the
/// seven older ones (it gives way to them all).
pub fn configure_paste_last_from(cfg: &crate::config::YapConfig) {
    let older = older_shortcuts(cfg);
    let spec = unless_clash("Paste-last shortcut", cfg.paste_last_hotkey.trim(), &older[..7]);
    if let Err(e) = configure_paste_last(spec) {
        tracing::warn!("Failed to apply the Paste-last shortcut: {}", e);
    }
}

/// The Copy last shortcut from config, unless it clashes with one of the
/// eight before it (Paste last's included).
pub fn configure_copy_last_from(cfg: &crate::config::YapConfig) {
    let older = older_shortcuts(cfg);
    let spec = unless_clash("Copy-last shortcut", cfg.copy_last_hotkey.trim(), &older);
    if let Err(e) = configure_copy_last(spec) {
        tracing::warn!("Failed to apply the Copy-last shortcut: {}", e);
    }
}

/// While a snip's overlay is up, Esc goes to it (`snip.rs`).
pub fn snip_escape(on: bool) {
    SNIP_ESC.store(on, Ordering::SeqCst);
}

fn transform_ids() -> std::sync::MutexGuard<'static, Vec<String>> {
    TRANSFORM_IDS.lock().unwrap_or_else(|p| p.into_inner())
}

/// The saved transforms' shortcuts from config (`transforms::assign`): a
/// slot for each transform with a key, in list order; one whose key would
/// fire with one of the nine fixed shortcuts, or an earlier transform's,
/// is left unbound and the log says why (Settings never saves one).
pub fn configure_transforms_from(cfg: &crate::config::YapConfig) {
    let list = crate::dictation::transforms::list(cfg);
    let fixed = crate::dictation::transforms::fixed_shortcuts(cfg);
    let fixed: Vec<(&str, &str)> = fixed.iter().map(|(name, spec)| (*name, spec.as_str())).collect();
    let slots = crate::dictation::transforms::assign(&fixed, &list);
    let mut ids = transform_ids();
    ids.clear();
    let mut bound = 0usize;
    for (i, binding) in TRANSFORM_BINDINGS.iter().enumerate() {
        let Some(slot) = slots.get(i) else {
            binding.configure(KEY_TYPE_NONE, 0, 0);
            continue;
        };
        if let Some(clash) = &slot.clash {
            tracing::warn!(transform = %slot.id, "A transform's shortcut clashes with {clash}: left unbound");
        }
        let (kind, code, mods) = parse_key_spec(&slot.spec).unwrap_or_else(|e| {
            tracing::warn!(transform = %slot.id, "A transform's shortcut can't be read: {e}");
            (KEY_TYPE_NONE, 0, 0)
        });
        binding.configure(kind, code, mods);
        bound += usize::from(kind != KEY_TYPE_NONE);
        ids.push(slot.id.clone());
    }
    info!(bound, "Transform shortcuts configured");
}

/// Live-apply transform `id`'s shortcut (Settings pauses it with "" while
/// a new key is picked): its slot, when it has one — a transform's first
/// shortcut gets its slot at the save.
pub fn configure_transform(id: &str, spec: &str) {
    let Some(i) = transform_ids().iter().position(|x| x == id) else { return };
    match parse_key_spec(spec.trim()) {
        Ok((kind, code, mods)) => TRANSFORM_BINDINGS[i].configure(kind, code, mods),
        Err(e) => tracing::warn!(transform = %id, "A transform's shortcut can't be read: {e}"),
    }
}

/// The transform input-hook slot `slot` runs.
pub fn transform_at(slot: usize) -> Option<String> {
    transform_ids().get(slot).cloned()
}

/// The transform slots as set now: each one's transform and key, in words
/// ("keyboard vkey 133", "none") — the e2e specs' check (debug builds).
#[cfg(debug_assertions)]
pub fn transform_slots() -> Vec<(String, String)> {
    transform_ids()
        .iter()
        .zip(TRANSFORM_BINDINGS.iter())
        .map(|(id, b)| {
            let (kind, code, mods) = b.kind_code_mods();
            (id.clone(), format_binding(kind, code, mods))
        })
        .collect()
}

/// A transform slot's press event → its slot.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn transform_slot(event: &str) -> Option<usize> {
    TRANSFORM_EVENTS.iter().position(|(pressed, _)| *pressed == event)
}

fn language_ids() -> std::sync::MutexGuard<'static, Vec<String>> {
    LANGUAGE_IDS.lock().unwrap_or_else(|p| p.into_inner())
}

/// The language shortcuts' keys from config (`languages::assign`): a slot
/// for each with a key, in list order; one whose key would fire with one of
/// the nine fixed shortcuts (Paste last's and Copy last's included), or an
/// earlier language shortcut's, is left unbound and the log says why
/// (Settings never saves one).
pub fn configure_languages_from(cfg: &crate::config::YapConfig) {
    let list = crate::dictation::languages::shortcuts(cfg);
    let nine = crate::dictation::transforms::nine_shortcuts(cfg);
    let fixed: Vec<(&str, &str)> = nine.iter().map(|(name, spec)| (*name, spec.as_str())).collect();
    let slots = crate::dictation::languages::assign(&fixed, &list);
    let mut ids = language_ids();
    ids.clear();
    let mut bound = 0usize;
    for (i, binding) in LANGUAGE_BINDINGS.iter().enumerate() {
        let Some(slot) = slots.get(i) else {
            binding.configure(KEY_TYPE_NONE, 0, 0);
            continue;
        };
        if let Some(clash) = &slot.clash {
            tracing::warn!("A language shortcut's key clashes with {clash}: left unbound");
        }
        let (kind, code, mods) = parse_key_spec(&slot.spec).unwrap_or_else(|e| {
            tracing::warn!("A language shortcut's key can't be read: {e}");
            (KEY_TYPE_NONE, 0, 0)
        });
        binding.configure(kind, code, mods);
        bound += usize::from(kind != KEY_TYPE_NONE);
        ids.push(slot.id.clone());
    }
    info!(bound, "Language shortcuts configured");
}

/// Live-apply language shortcut `id`'s key (Settings pauses it with "" while
/// a new key is picked): its slot, when it has one — a shortcut's first key
/// gets its slot at the save.
pub fn configure_language(id: &str, spec: &str) {
    let Some(i) = language_ids().iter().position(|x| x == id) else { return };
    match parse_key_spec(spec.trim()) {
        Ok((kind, code, mods)) => LANGUAGE_BINDINGS[i].configure(kind, code, mods),
        Err(e) => tracing::warn!("A language shortcut's key can't be read: {e}"),
    }
}

/// The language shortcut input-hook slot `slot` is.
pub fn language_at(slot: usize) -> Option<String> {
    language_ids().get(slot).cloned()
}

/// The language slots as set now: each one's shortcut and key, in words —
/// the e2e specs' check (debug builds).
#[cfg(debug_assertions)]
pub fn language_slots() -> Vec<(String, String)> {
    language_ids()
        .iter()
        .zip(LANGUAGE_BINDINGS.iter())
        .map(|(id, b)| {
            let (kind, code, mods) = b.kind_code_mods();
            (id.clone(), format_binding(kind, code, mods))
        })
        .collect()
}

/// A language slot's event → its slot and whether it's the press.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn language_slot(event: &str) -> Option<(usize, bool)> {
    LANGUAGE_EVENTS.iter().enumerate().find_map(|(i, (down, up))| {
        if *down == event {
            Some((i, true))
        } else if *up == event {
            Some((i, false))
        } else {
            None
        }
    })
}

/// The modifier family of a virtual key (any platform; the hook's own
/// `modifier_family` is Windows-only).
fn vk_family(vk: u32) -> u8 {
    match vk {
        0x11 | 0xA2 | 0xA3 => MOD_CTRL,
        0x12 | 0xA4 | 0xA5 => MOD_ALT,
        0x10 | 0xA0 | 0xA1 => MOD_SHIFT,
        0x5B | 0x5C => MOD_WIN,
        _ => 0,
    }
}

/// Whether two key specs would fire on **one press**: the hook checks every
/// binding against every key event, so both would start. Pure (the same
/// rule as `hotkeysClash` in `lib/core/hotkeys.js`, which Settings checks with).
///
/// - The same key, button or chord, however it's written.
/// - A bare key fires only while no modifier is held (`process_keyboard_event`),
///   so F9 and Ctrl+F9 are two keys — unless the bare key is itself a
///   modifier (RightAlt), which fires whatever else is held.
/// - Combos on one main key fire with *at least* their modifiers held:
///   Ctrl+Space also fires inside Ctrl+Shift+Space.
/// - A modifier-only chord completes while a combo's modifiers go down
///   (Ctrl+Alt inside Ctrl+Alt+K), and inside a bigger chord.
///
/// An empty or unreadable spec clashes with nothing.
pub fn clashes(a: &str, b: &str) -> bool {
    let (Ok(a), Ok(b)) = (parse_key_spec(a.trim()), parse_key_spec(b.trim())) else {
        return false;
    };
    let within = |x: u8, y: u8| x & y == x;
    let nested = |x: u8, y: u8| within(x, y) || within(y, x);
    match (a, b) {
        ((KEY_TYPE_NONE, ..), _) | (_, (KEY_TYPE_NONE, ..)) => false,
        ((KEY_TYPE_MOUSE, x, _), (KEY_TYPE_MOUSE, y, _)) => x == y,
        ((KEY_TYPE_KEYBOARD, x, m), (KEY_TYPE_KEYBOARD, y, n)) => {
            if x != y {
                false
            } else if m == 0 || n == 0 {
                m == n || vk_family(x) != 0
            } else {
                nested(m, n)
            }
        }
        ((KEY_TYPE_MODS, _, m), (KEY_TYPE_MODS, _, n)) => nested(m, n),
        ((KEY_TYPE_MODS, _, chord), (KEY_TYPE_KEYBOARD, vk, m))
        | ((KEY_TYPE_KEYBOARD, vk, m), (KEY_TYPE_MODS, _, chord)) => {
            if m == 0 {
                // A modifier key bound alone (RightAlt) goes down inside a
                // chord holding its family.
                chord & vk_family(vk) != 0
            } else {
                within(chord, m)
            }
        }
        _ => false,
    }
}

/// Whether pressing a combo should tap the menu mask key: it holds Alt or
/// Win, and its main key (swallowed by the hook) isn't itself a modifier.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn needs_menu_mask(required: u8, bound_is_modifier: bool) -> bool {
    required & (MOD_ALT | MOD_WIN) != 0 && !bound_is_modifier
}

/// Parse one modifier token ("ctrl", "alt", …) to its MOD_* bit.
fn parse_mod_token(token: &str) -> Option<u8> {
    match token.to_ascii_lowercase().as_str() {
        "ctrl" | "control" => Some(MOD_CTRL),
        "alt" | "option" => Some(MOD_ALT),
        "shift" => Some(MOD_SHIFT),
        "win" | "super" | "meta" | "cmd" | "command" => Some(MOD_WIN),
        _ => None,
    }
}

fn parse_key_spec(spec: &str) -> Result<(u8, u32, u8), String> {
    if spec.is_empty() {
        return Ok((KEY_TYPE_NONE, 0, 0));
    }
    // "kb:52" or "kb:ctrl+shift+52" — modifier tokens then the main vkey.
    if let Some(rest) = spec.strip_prefix("kb:") {
        let mut mods: u8 = 0;
        let tokens: Vec<&str> = rest.split('+').collect();
        let (mod_tokens, key_token) = tokens.split_at(tokens.len() - 1);
        for t in mod_tokens {
            mods |= parse_mod_token(t).ok_or_else(|| format!("Unknown modifier: '{}'", t))?;
        }
        let code = key_token[0]
            .parse::<u32>()
            .map_err(|_| format!("Invalid vkey: {}", key_token[0]))?;
        return Ok((KEY_TYPE_KEYBOARD, code, mods));
    }
    // "mods:ctrl+alt" — modifier-only chord. Require 2+ so a lone "mods:ctrl"
    // can't hijack every Ctrl press (single right-side modifiers are plain
    // "kb:<vk>" bindings instead — VK_RCONTROL etc.).
    if let Some(rest) = spec.strip_prefix("mods:") {
        let mut mods: u8 = 0;
        let mut count = 0usize;
        for t in rest.split('+') {
            mods |= parse_mod_token(t).ok_or_else(|| format!("Unknown modifier: '{}'", t))?;
            count += 1;
        }
        if count < 2 || mods.count_ones() < 2 {
            return Err("A modifier-only chord needs at least 2 distinct modifiers".into());
        }
        return Ok((KEY_TYPE_MODS, 0, mods));
    }
    if let Some(btn) = spec.strip_prefix("mouse:") {
        let code = btn
            .parse::<u32>()
            .map_err(|_| format!("Invalid button: {}", btn))?;
        return Ok((KEY_TYPE_MOUSE, code, 0));
    }
    // Legacy format: "MouseButton4" → mouse button 4
    if let Some(rest) = spec.strip_prefix("MouseButton") {
        let id = rest
            .parse::<u32>()
            .map_err(|_| format!("Invalid MouseButton: {}", rest))?;
        return Ok((KEY_TYPE_MOUSE, id, 0));
    }
    Err(format!(
        "Unknown key spec: '{}'. Use 'kb:CODE', 'kb:MODS+CODE', 'mods:MODS' or 'mouse:ID'.",
        spec
    ))
}

fn format_mods(mods: u8) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if mods & MOD_CTRL != 0 {
        parts.push("ctrl");
    }
    if mods & MOD_ALT != 0 {
        parts.push("alt");
    }
    if mods & MOD_SHIFT != 0 {
        parts.push("shift");
    }
    if mods & MOD_WIN != 0 {
        parts.push("win");
    }
    parts.join("+")
}

fn format_binding(key_type: u8, key_code: u32, mods: u8) -> String {
    match key_type {
        KEY_TYPE_KEYBOARD if mods != 0 => {
            format!("keyboard {}+vkey {}", format_mods(mods), key_code)
        }
        KEY_TYPE_KEYBOARD => format!("keyboard vkey {}", key_code),
        KEY_TYPE_MOUSE => format!("mouse button {}", key_code),
        KEY_TYPE_MODS => format!("modifier chord {}", format_mods(mods)),
        _ => "none".into(),
    }
}

// ---- Windows-specific hooks ----

#[cfg(target_os = "windows")]
mod win32 {
    use std::ffi::c_int;

    pub type HHOOK = isize;

    pub const WH_KEYBOARD_LL: c_int = 13;
    pub const WH_MOUSE_LL: c_int = 14;

    pub const WM_KEYDOWN: u32 = 0x0100;
    pub const WM_SYSKEYDOWN: u32 = 0x0104;

    pub const WM_MBUTTONDOWN: u32 = 0x0207;
    pub const WM_MBUTTONUP: u32 = 0x0208;
    pub const WM_XBUTTONDOWN: u32 = 0x020B;
    pub const WM_XBUTTONUP: u32 = 0x020C;
    pub const WM_TIMER: u32 = 0x0113;

    pub const VK_LWIN: i32 = 0x5B;
    pub const VK_RWIN: i32 = 0x5C;
    pub const VK_LSHIFT: i32 = 0xA0;
    pub const VK_RSHIFT: i32 = 0xA1;
    pub const VK_LCONTROL: i32 = 0xA2;
    pub const VK_RCONTROL: i32 = 0xA3;
    pub const VK_LMENU: i32 = 0xA4;
    pub const VK_RMENU: i32 = 0xA5;
    /// An unassigned virtual key: AutoHotkey's default menu mask key (vkE8).
    pub const VK_MENU_MASK: u16 = 0xE8;

    pub const INPUT_KEYBOARD: u32 = 1;
    pub const KEYEVENTF_KEYUP: u32 = 0x0002;

    // SendInput's INPUT: the union includes MOUSEINPUT (its largest member) so
    // that sizeof(INPUT) is 40 on 64-bit, as SendInput's cbSize expects.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct MOUSEINPUT {
        pub dx: i32,
        pub dy: i32,
        pub mouse_data: u32,
        pub flags: u32,
        pub time: u32,
        pub extra_info: usize,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct KEYBDINPUT {
        pub vk: u16,
        pub scan: u16,
        pub flags: u32,
        pub time: u32,
        pub extra_info: usize,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub union INPUT_UNION {
        pub mi: MOUSEINPUT,
        pub ki: KEYBDINPUT,
    }

    #[repr(C)]
    pub struct INPUT {
        pub kind: u32,
        pub u: INPUT_UNION,
    }

    #[repr(C)]
    pub struct POINT {
        pub x: i32,
        pub y: i32,
    }

    #[repr(C)]
    pub struct KBDLLHOOKSTRUCT {
        pub vk_code: u32,
        pub scan_code: u32,
        pub flags: u32,
        pub time: u32,
        pub dw_extra_info: usize,
    }

    #[repr(C)]
    pub struct MSLLHOOKSTRUCT {
        pub pt: POINT,
        pub mouse_data: u32,
        pub flags: u32,
        pub time: u32,
        pub dw_extra_info: usize,
    }

    #[repr(C)]
    pub struct MSG {
        pub hwnd: isize,
        pub message: u32,
        pub wparam: usize,
        pub lparam: isize,
        pub time: u32,
        pub pt: POINT,
    }

    extern "system" {
        pub fn SetWindowsHookExW(
            id_hook: c_int,
            lpfn: unsafe extern "system" fn(c_int, usize, isize) -> isize,
            hmod: isize,
            dw_thread_id: u32,
        ) -> HHOOK;
        pub fn CallNextHookEx(
            hhk: HHOOK,
            n_code: c_int,
            wparam: usize,
            lparam: isize,
        ) -> isize;
        pub fn UnhookWindowsHookEx(hhk: HHOOK) -> i32;
        pub fn GetMessageW(
            msg: *mut MSG,
            hwnd: isize,
            msg_filter_min: u32,
            msg_filter_max: u32,
        ) -> i32;
        pub fn TranslateMessage(msg: *const MSG) -> i32;
        pub fn DispatchMessageW(msg: *const MSG) -> isize;
        pub fn SetTimer(
            hwnd: isize,
            id_event: usize,
            elapse: u32,
            lpfn: Option<unsafe extern "system" fn(isize, u32, usize, u32)>,
        ) -> usize;
        pub fn GetModuleHandleW(module_name: *const u16) -> isize;
        pub fn GetAsyncKeyState(vkey: i32) -> i16;
        pub fn SendInput(count: u32, inputs: *const INPUT, size: i32) -> u32;
        pub fn GetTickCount() -> u32;
        pub fn GetTickCount64() -> u64;
        pub fn QueryUnbiasedInterruptTime(time: *mut u64) -> i32;
        pub fn GetLastInputInfo(info: *mut LASTINPUTINFO) -> i32;
        pub fn GetCurrentThreadId() -> u32;
        pub fn PostThreadMessageW(thread: u32, msg: u32, wparam: usize, lparam: isize) -> i32;
        pub fn SetWinEventHook(
            event_min: u32,
            event_max: u32,
            hmod: isize,
            callback: unsafe extern "system" fn(isize, u32, isize, i32, i32, u32, u32),
            process: u32,
            thread: u32,
            flags: u32,
        ) -> isize;
    }

    #[repr(C)]
    pub struct LASTINPUTINFO {
        pub size: u32,
        pub time: u32,
    }

    /// The input desktop changed (the lock screen, a UAC prompt, and back).
    pub const EVENT_SYSTEM_DESKTOPSWITCH: u32 = 0x0020;
    pub const WINEVENT_OUTOFCONTEXT: u32 = 0;
    /// [`super::sweep_now`]'s thread message.
    pub const WM_APP_SWEEP: u32 = 0x8000 + 0x59;
}

/// Tap the menu mask key (down + up) while a combo's Alt / Win is still
/// held, so their release opens neither the focused app's menu bar nor the
/// Start menu (see the module docs). From the forwarder thread, never the
/// hook callback. Our own hook sees the injected key and ignores it.
#[cfg(target_os = "windows")]
fn send_menu_mask() {
    let key = |flags: u32| win32::INPUT {
        kind: win32::INPUT_KEYBOARD,
        u: win32::INPUT_UNION {
            ki: win32::KEYBDINPUT {
                vk: win32::VK_MENU_MASK,
                scan: 0,
                flags,
                time: 0,
                extra_info: 0,
            },
        },
    };
    let inputs = [key(0), key(win32::KEYEVENTF_KEYUP)];
    let sent = unsafe {
        win32::SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<win32::INPUT>() as i32,
        )
    };
    trace!(sent, "Input hook: menu mask key");
}

/// Which modifier family (MOD_* bit) a virtual key belongs to; 0 for
/// non-modifier keys. The LL hook reports side-specific codes (VK_LCONTROL…),
/// but the generic codes are included for injected input.
#[cfg(target_os = "windows")]
fn modifier_family(vk: u32) -> u8 {
    match vk {
        0x11 | 0xA2 | 0xA3 => MOD_CTRL,  // VK_CONTROL / L / R
        0x12 | 0xA4 | 0xA5 => MOD_ALT,   // VK_MENU / L / R
        0x10 | 0xA0 | 0xA1 => MOD_SHIFT, // VK_SHIFT / L / R
        0x5B | 0x5C => MOD_WIN,          // VK_LWIN / VK_RWIN
        _ => 0,
    }
}

/// Is any key of `family` down, ignoring `exclude_vk`? `GetAsyncKeyState` is
/// unreliable for the key that triggered the *current* hook callback (the state
/// isn't updated yet), so callers exclude it and supply its event state — the
/// same trick as OpenWhispr's `SyncModifierState`.
#[cfg(target_os = "windows")]
fn family_down_excluding(family: u8, exclude_vk: u32) -> bool {
    let vks: &[i32] = match family {
        MOD_CTRL => &[win32::VK_LCONTROL, win32::VK_RCONTROL],
        MOD_ALT => &[win32::VK_LMENU, win32::VK_RMENU],
        MOD_SHIFT => &[win32::VK_LSHIFT, win32::VK_RSHIFT],
        MOD_WIN => &[win32::VK_LWIN, win32::VK_RWIN],
        _ => &[],
    };
    vks.iter()
        .any(|&vk| vk as u32 != exclude_vk && unsafe { win32::GetAsyncKeyState(vk) } < 0)
}

/// Current modifier state as a MOD_* bitmask. When the in-flight hook event is
/// itself a modifier, its own family is decided by the event (`is_down`) OR'd
/// with the *other* side's async state (releasing LCtrl while RCtrl is held
/// keeps Ctrl "down").
#[cfg(target_os = "windows")]
fn mods_now(current_vk: u32, is_down: bool) -> u8 {
    let current_family = modifier_family(current_vk);
    let mut mods = 0u8;
    for family in [MOD_CTRL, MOD_ALT, MOD_SHIFT, MOD_WIN] {
        let held = if family == current_family {
            is_down || family_down_excluding(family, current_vk)
        } else {
            family_down_excluding(family, 0)
        };
        if held {
            mods |= family;
        }
    }
    mods
}

/// Handle a press/release for a key binding. Emits Tauri events and
/// suppresses key repeats (only emits on first press, not held repeats).
///
/// Returns whether this binding is "ours" for suppression purposes: a press is
/// always ours (we swallow the raw key so it doesn't land in the field); a
/// release is only ours if the binding was actually `active` (so the keyup of a
/// pass-through combo isn't swallowed).
///
/// `action`: the fixed action the binding is one of the shortcuts of — its
/// press goes out only when none of its other shortcuts is down already,
/// its release only when none is still down (they act as one key).
#[cfg(target_os = "windows")]
fn handle_binding_event(
    binding: &KeyBinding,
    action: Option<usize>,
    event_pressed: &'static str,
    event_released: &'static str,
    is_press: bool,
) -> bool {
    let now = crate::dictation::gesture::now_ms();
    let others_down = || action.is_some_and(|i| group(i).any(|b| !std::ptr::eq(b, binding) && b.is_active()));
    if is_press {
        // Only emit on first press (not repeats)
        if !binding.active.swap(true, Ordering::Relaxed) {
            binding.since.store(now, Ordering::Relaxed);
            binding.repeat_at.store(0, Ordering::Relaxed);
            binding.chorded.store(false, Ordering::Relaxed);
            if !others_down() {
                if let Some(tx) = EVENT_TX.get() {
                    info!("Input hook: queueing {}", event_pressed);
                    let _ = tx.send((event_pressed, now));
                }
            }
        } else {
            // Down again while held: the keyboard repeating it.
            binding.repeat_at.store(now, Ordering::Relaxed);
        }
        true
    } else if binding.active.swap(false, Ordering::Relaxed) {
        if !others_down() {
            if let Some(tx) = EVENT_TX.get() {
                info!("Input hook: queueing {}", event_released);
                let _ = tx.send((event_released, now));
            }
        }
        true
    } else {
        false
    }
}

// ---- Keyboard hook callback ----

/// Process one keyboard event against one binding. Returns whether the event
/// should be **suppressed** (swallowed before it reaches the focused app).
///
/// Semantics (ported from OpenWhispr's `windows-key-listener.c`, keeping Yap's
/// original bare-key behaviour):
/// - **Bare key** (`kb:120`): press only starts when NO modifier is held, so
///   Ctrl+F9 etc. pass through as normal shortcuts (Yap's original rule). The
///   release is honoured regardless of modifier state. Suppressed both ways.
/// - **Combo** (`kb:ctrl+shift+32`): press fires when the main key goes down
///   with at least the required modifiers held (extras tolerated); the main
///   key is suppressed, the modifiers are not. Releasing the main key OR any
///   required modifier ends the press (so push-to-talk can't get stuck).
/// - **Right-side single modifier** (`kb:165` = RightAlt): a plain vkey
///   binding whose key IS a modifier — never suppressed (RightAlt is AltGr on
///   international layouts) and exempt from the bare-key "no modifiers held"
///   guard (its own family is held by definition).
/// - **Modifier-only chord** (`mods:ctrl+alt`): press fires the instant the
///   chord completes; release when any chord modifier lifts. Never suppressed.
/// - Self-heal: while active, any other key event verifies the main key is
///   still physically down (`GetAsyncKeyState` is reliable for keys other than
///   the in-flight one) — a keyup eaten by Win+L etc. can't strand the binding.
#[cfg(target_os = "windows")]
fn process_keyboard_event(
    binding: &KeyBinding,
    action: Option<usize>,
    pressed: &'static str,
    released: &'static str,
    vkey: u32,
    is_keydown: bool,
) -> bool {
    let (kind, code, required) = binding.kind_code_mods();
    match kind {
        KEY_TYPE_KEYBOARD => {
            let bound_is_modifier = modifier_family(code) != 0;
            if vkey == code {
                if is_keydown {
                    let held = mods_now(vkey, true);
                    if required == 0 {
                        // Bare key: keep Yap's guard — modifier combos like
                        // Ctrl+F9 pass through — unless the bound key is
                        // itself a modifier (RightAlt holds Alt by nature).
                        if !bound_is_modifier && held != 0 {
                            return false;
                        }
                    } else if held & required != required {
                        return false; // required modifiers absent → passthrough
                    }
                    // Alt / Win held and the main key swallowed: mask their
                    // release (first press only, not the key's repeats).
                    if needs_menu_mask(required, bound_is_modifier)
                        && !binding.active.load(Ordering::Relaxed)
                    {
                        if let Some(tx) = EVENT_TX.get() {
                            let _ = tx.send((MENU_MASK, 0));
                        }
                    }
                    handle_binding_event(binding, action, pressed, released, true);
                    return !bound_is_modifier; // never suppress modifier keys
                }
                let ours = handle_binding_event(binding, action, pressed, released, false);
                return ours && !bound_is_modifier;
            }
            // Another key's event while our press is active: two safety rules.
            if binding.active.load(Ordering::Relaxed) {
                let family = modifier_family(vkey);
                if required != 0 && !is_keydown && family != 0 && required & family != 0 {
                    // 1. A required modifier was released → end the press.
                    if mods_now(vkey, false) & required != required {
                        handle_binding_event(binding, action, pressed, released, false);
                    }
                } else if bound_is_modifier
                    && unsafe { win32::GetAsyncKeyState(code as i32) } >= 0
                {
                    // 2. Self-heal a missed main-key keyup — ONLY for
                    // unsuppressed (modifier) bindings: a suppressed key never
                    // registers in GetAsyncKeyState (the hook eats the event
                    // before the key-state table updates), so an async check
                    // on it would "heal" a perfectly live press.
                    handle_binding_event(binding, action, pressed, released, false);
                }
            }
            false
        }
        KEY_TYPE_MODS => {
            let held = mods_now(vkey, is_keydown);
            if is_keydown {
                if held & required == required {
                    handle_binding_event(binding, action, pressed, released, true);
                }
            } else if binding.active.load(Ordering::Relaxed) && held & required != required {
                handle_binding_event(binding, action, pressed, released, false);
            }
            false // chords are made of modifiers — never suppressed
        }
        _ => false,
    }
}

/// The bindings a key is held for (push-to-talk): the dictation key's, the
/// rewrite shortcut's and the assistant's (all their shortcuts), and the
/// language shortcuts'.
fn hold_bindings() -> impl Iterator<Item = &'static KeyBinding> {
    HOLDS.iter().flat_map(|&i| group(i)).chain(LANGUAGE_BINDINGS.iter())
}

/// A key went down that wasn't held (`vkey`, scan code `scan`, at `now`): a
/// hold-to-talk press under a second it isn't part of grew into a chord —
/// the person meant a shortcut sharing the key. Said once per press, with
/// the moment the other key went down.
#[cfg(target_os = "windows")]
fn note_new_key(vkey: u32, scan: u32, now: u64) {
    LAST_NEW_KEY_AT.store(now, Ordering::Relaxed);
    for b in hold_bindings() {
        if !b.is_active() {
            continue;
        }
        let (kind, code, mods) = b.kind_code_mods();
        let since = b.since.load(Ordering::Relaxed);
        if now.saturating_sub(since) < crate::dictation::gesture::CHORD_MS
            && held::another_key(kind, code, mods, vkey, scan)
            && !b.chorded.swap(true, Ordering::Relaxed)
        {
            if let Some(tx) = EVENT_TX.get() {
                info!("Input hook: a hold-to-talk press grew into a chord");
                let _ = tx.send((CHORD, now));
            }
        }
    }
}

/// Robot mode (debug builds): whether the hook acts on this event — only the
/// robot's own ([`ROBOT_MARK`]); everything else passes by as if no hook
/// were there (`Some(true)`: heard, `Some(false)`: an [`UNHEARD_MARK`]
/// event, not even heard). `None`: act on it.
#[cfg(target_os = "windows")]
fn robot_skip(extra: usize) -> Option<bool> {
    if ROBOT_ONLY.load(Ordering::Relaxed) {
        return match extra {
            ROBOT_MARK => None,
            UNHEARD_MARK => Some(false),
            _ => Some(true),
        };
    }
    None
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn low_level_keyboard_proc(
    code: i32,
    wparam: usize,
    lparam: isize,
) -> isize {
    if code >= 0 {
        let info = &*(lparam as *const win32::KBDLLHOOKSTRUCT);
        let vkey = info.vk_code;
        let msg = wparam as u32;

        // Robot mode: only the robot's own keys (`heard`: still counted as
        // input the hook heard).
        if let Some(heard) = robot_skip(info.dw_extra_info) {
            if heard {
                HEARD_TICK.store(win32::GetTickCount(), Ordering::Relaxed);
            }
            return win32::CallNextHookEx(0, code, wparam, lparam);
        }
        HEARD_TICK.store(win32::GetTickCount(), Ordering::Relaxed);

        let is_keydown = msg == win32::WM_KEYDOWN || msg == win32::WM_SYSKEYDOWN;

        // What's held: a key that wasn't is a new key — maybe a chord.
        let was_held = set_held(vkey, is_keydown);
        if is_keydown && !was_held {
            note_new_key(vkey, info.scan_code, crate::dictation::gesture::now_ms());
        }

        // Esc while a snip's overlay is up: it cancels the snip, and goes no
        // further (the app behind the overlay mustn't get it).
        if vkey == 0x1B && SNIP_ESC.load(Ordering::Relaxed) {
            if is_keydown {
                if let Some(tx) = EVENT_TX.get() {
                    let _ = tx.send((SNIP_ESCAPE, 0));
                }
            }
            return 1;
        }

        let mut suppress = false;
        // The nine fixed actions, each with its first and more shortcuts.
        for (i, (pressed, released)) in FIXED_EVENTS.iter().enumerate() {
            for binding in group(i) {
                suppress |= process_keyboard_event(binding, Some(i), pressed, released, vkey, is_keydown);
            }
        }
        // Language shortcuts' keys, then saved transforms' (an unbound
        // slot is never matched).
        for (binding, (pressed, released)) in LANGUAGE_BINDINGS.iter().zip(LANGUAGE_EVENTS) {
            suppress |= process_keyboard_event(binding, None, pressed, released, vkey, is_keydown);
        }
        for (binding, (pressed, released)) in TRANSFORM_BINDINGS.iter().zip(TRANSFORM_EVENTS) {
            suppress |= process_keyboard_event(binding, None, pressed, released, vkey, is_keydown);
        }
        if suppress {
            return 1;
        }
    }

    win32::CallNextHookEx(0, code, wparam, lparam)
}

// ---- Mouse hook callback ----

#[cfg(target_os = "windows")]
unsafe extern "system" fn low_level_mouse_proc(
    code: i32,
    wparam: usize,
    lparam: isize,
) -> isize {
    if code >= 0 {
        let msg = wparam as u32;
        let hook_info = &*(lparam as *const win32::MSLLHOOKSTRUCT);

        if let Some(heard) = robot_skip(hook_info.dw_extra_info) {
            if heard {
                HEARD_TICK.store(win32::GetTickCount(), Ordering::Relaxed);
            }
            return win32::CallNextHookEx(0, code, wparam, lparam);
        }
        HEARD_TICK.store(win32::GetTickCount(), Ordering::Relaxed);

        let (is_press, button_id) = match msg {
            win32::WM_MBUTTONDOWN => (true, Some(3u32)),
            win32::WM_MBUTTONUP => (false, Some(3u32)),
            win32::WM_XBUTTONDOWN | win32::WM_XBUTTONUP => {
                let xbutton = (hook_info.mouse_data >> 16) & 0xFFFF;
                let id = match xbutton {
                    1 => Some(4u32), // XBUTTON1 (back)
                    2 => Some(5u32), // XBUTTON2 (forward)
                    _ => None,
                };
                (msg == win32::WM_XBUTTONDOWN, id)
            }
            _ => (false, None),
        };

        if let Some(id) = button_id {
            // The fixed actions' shortcuts (first and more), then the
            // language shortcuts', then the saved transforms': the first
            // binding on this button takes it, and the button is swallowed.
            for (i, (pressed, released)) in FIXED_EVENTS.iter().enumerate() {
                for binding in group(i) {
                    if binding.matches_mouse(id) {
                        handle_binding_event(binding, Some(i), pressed, released, is_press);
                        return 1;
                    }
                }
            }
            for (binding, (pressed, released)) in LANGUAGE_BINDINGS.iter().zip(LANGUAGE_EVENTS) {
                if binding.matches_mouse(id) {
                    handle_binding_event(binding, None, pressed, released, is_press);
                    return 1;
                }
            }
            for (binding, (pressed, released)) in TRANSFORM_BINDINGS.iter().zip(TRANSFORM_EVENTS) {
                if binding.matches_mouse(id) {
                    handle_binding_event(binding, None, pressed, released, is_press);
                    return 1;
                }
            }
        }
    }

    win32::CallNextHookEx(0, code, wparam, lparam)
}

// ---- The stuck-key sweep ----

/// The clocks at the last sweep (the hook's thread only): the tick count,
/// which counts sleep, and the unbiased interrupt time, which doesn't (ms).
#[cfg(target_os = "windows")]
static SWEEP_TICKS: AtomicU64 = AtomicU64::new(0);
#[cfg(target_os = "windows")]
static SWEEP_AWAKE: AtomicU64 = AtomicU64::new(0);

/// Ask the hook's thread to sweep now (`held.rs`): a push-to-talk hold of a
/// minute, a dictation near its length limit. Any thread; nothing waits.
pub fn sweep_now(why: &str) {
    #[cfg(target_os = "windows")]
    {
        let thread = HOOK_THREAD.load(Ordering::SeqCst);
        if thread != 0 {
            tracing::debug!(why, "Input hook: a sweep asked for");
            unsafe { win32::PostThreadMessageW(thread, win32::WM_APP_SWEEP, 0, 0) };
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = why;
}

/// Check every active binding against Windows' key state and the evidence
/// (`held::stuck`), on the hook's own thread (between hook callbacks). A
/// stuck one is let go **without** its release — never a dictation started
/// or stopped here — and when it held a recording (push-to-talk), the
/// pipeline is told (`STUCK`: it goes on hands-free). `away`: why the hook
/// may have missed input (a desktop switch), else found here (a sleep since
/// the last sweep, input Windows saw and the hook didn't).
#[cfg(target_os = "windows")]
fn sweep(away: Option<&'static str>) {
    let now = crate::dictation::gesture::now_ms();
    let ticks = unsafe { win32::GetTickCount64() };
    let mut unbiased = 0u64;
    unsafe { win32::QueryUnbiasedInterruptTime(&mut unbiased) };
    let awake = unbiased / 10_000;
    let (last_ticks, last_awake) = (SWEEP_TICKS.swap(ticks, Ordering::Relaxed), SWEEP_AWAKE.swap(awake, Ordering::Relaxed));
    let slept = last_ticks != 0 && held::slept(ticks.saturating_sub(last_ticks), awake.saturating_sub(last_awake));
    let mut input = win32::LASTINPUTINFO { size: std::mem::size_of::<win32::LASTINPUTINFO>() as u32, time: 0 };
    let missed = unsafe { win32::GetLastInputInfo(&mut input) } != 0
        && held::missed_input(input.time, HEARD_TICK.load(Ordering::Relaxed));
    let away = away.or(slept.then_some("a sleep")).or(missed.then_some("input the hook didn't hear"));
    let evidence = if away.is_some() { held::Evidence::Away } else { held::Evidence::Regular };
    let windows_mods = mods_now(0, false);
    let last_new = LAST_NEW_KEY_AT.load(Ordering::Relaxed);

    // One binding: let go if it's stuck. `true`: it was.
    let check = |b: &KeyBinding, name: &str| -> bool {
        if !b.is_active() {
            return false;
        }
        let (kind, code, mods) = b.kind_code_mods();
        let since = b.since.load(Ordering::Relaxed);
        let repeat_at = b.repeat_at.load(Ordering::Relaxed);
        let h = held::Held {
            kind,
            code,
            mods,
            age_ms: now.saturating_sub(since),
            repeat_ago_ms: (repeat_at != 0).then(|| now.saturating_sub(repeat_at)),
            other_key_since: last_new > repeat_at.max(since),
        };
        let alone_modifier = kind == KEY_TYPE_KEYBOARD && mods == 0 && modifier_family(code) != 0;
        let key_down = alone_modifier && unsafe { win32::GetAsyncKeyState(code as i32) } < 0;
        let Some(why) = held::stuck(&h, &held::Windows { mods_down: windows_mods, key_down }, evidence) else {
            return false;
        };
        b.active.store(false, Ordering::Relaxed);
        if kind == KEY_TYPE_KEYBOARD {
            set_held(code, false);
        }
        warn!(away = away.unwrap_or("no"), "Input hook: the {name} was stuck down ({}): let go, without its release", why.describe());
        true
    };
    let mut hold_lost = false;
    for (i, name) in FIXED_NAMES.iter().enumerate() {
        let mut any = false;
        for b in group(i) {
            any |= check(b, name);
        }
        // A recording key, none of its shortcuts down any more.
        if any && HOLDS.contains(&i) && !group(i).any(KeyBinding::is_active) {
            hold_lost = true;
        }
    }
    for b in LANGUAGE_BINDINGS.iter() {
        hold_lost |= check(b, "language shortcut");
    }
    for b in TRANSFORM_BINDINGS.iter() {
        check(b, "transform shortcut");
    }
    // What's held, kept honest: a key no binding holds that Windows says is
    // up (a key-up lost on another desktop) is forgotten. (A key a binding
    // swallows never shows in Windows' state: it's kept while held.)
    for vk in held_keys() {
        let swallowed = group_all().any(|b| {
            let (kind, code, _) = b.kind_code_mods();
            b.is_active() && kind == KEY_TYPE_KEYBOARD && code == vk && modifier_family(code) == 0
        });
        if !swallowed && unsafe { win32::GetAsyncKeyState(vk as i32) } >= 0 {
            set_held(vk, false);
        }
    }
    if hold_lost {
        if let Some(tx) = EVENT_TX.get() {
            let _ = tx.send((STUCK, now));
        }
    }
    if let Some(away) = away {
        tracing::debug!(away, "Input hook: swept after the hook may have missed input");
    }
}

/// Every binding: the fixed actions' (first and more), the language
/// shortcuts', the transforms'.
#[cfg(target_os = "windows")]
fn group_all() -> impl Iterator<Item = &'static KeyBinding> {
    (0..FIXED_COUNT).flat_map(group).chain(LANGUAGE_BINDINGS.iter()).chain(TRANSFORM_BINDINGS.iter())
}

/// The input desktop switched — the lock screen, a UAC prompt, or back
/// (`EVENT_SYSTEM_DESKTOPSWITCH`, on the hook's thread): the hook hears
/// nothing on another desktop, so any key-up may have been lost there.
#[cfg(target_os = "windows")]
unsafe extern "system" fn on_desktop_switch(_hook: isize, _event: u32, _hwnd: isize, _object: i32, _child: i32, _thread: u32, _time: u32) {
    info!("Input hook: the input desktop switched (the lock screen, a UAC prompt, or back): sweeping");
    sweep(Some("a desktop switch"));
}

// ---- Start the unified hook ----

/// Start the unified input hook on a background thread.
///
/// Installs both WH_KEYBOARD_LL and WH_MOUSE_LL hooks. The hooks check
/// configured key bindings (set via `configure_dictation`/`configure_edit`)
/// and emit Tauri events when matched. Keyboard keys are suppressed at the
/// OS level to prevent them from reaching other applications. In robot mode
/// (debug builds) it acts only on the robot's own keys ([`ROBOT_MARK`]).
#[cfg(target_os = "windows")]
pub fn start_input_hook(app_handle: AppHandle) {
    #[cfg(debug_assertions)]
    if crate::shell::realtest::on() {
        info!("Input hook: robot mode, only the robot's own keys");
        ROBOT_ONLY.store(true, Ordering::SeqCst);
    }
    // Emit-forwarder: the hook callback queues event names here and this thread
    // does the (potentially slow) `app.emit`, keeping the callback fast enough
    // that Windows never times it out (see EVENT_TX).
    let (tx, rx) = std::sync::mpsc::channel::<(&'static str, u64)>();
    EVENT_TX.set(tx).expect("Input hook event channel already set");
    std::thread::Builder::new()
        .name("input-hook-emit".into())
        .spawn(move || {
            while let Ok((event, at)) = rx.recv() {
                if event == MENU_MASK {
                    send_menu_mask();
                    continue;
                }
                if event == SNIP_ESCAPE {
                    crate::snip::cancel("escape");
                    continue;
                }
                // A hold-to-talk press grew into a chord (`at`: its press),
                // or the sweep let go of a stuck one: the pipeline decides.
                if event == CHORD {
                    crate::dictation::pipeline::key_chord(&app_handle, at);
                    continue;
                }
                if event == STUCK {
                    crate::dictation::pipeline::key_stuck(&app_handle);
                    continue;
                }
                // A saved transform's press: one event, with its slot.
                if let Some(slot) = transform_slot(event) {
                    info!(slot, "Input hook: emitting transform-key-pressed");
                    if let Err(e) = app_handle.emit("transform-key-pressed", slot) {
                        warn!("Input hook: failed to emit transform-key-pressed: {}", e);
                    }
                    continue;
                }
                if event.starts_with("transform-key-") {
                    continue;
                }
                // A language shortcut's press or release: one event, with
                // its slot (`languages::init` hands it to the pipeline).
                if let Some((slot, pressed)) = language_slot(event) {
                    info!(slot, pressed, "Input hook: emitting language-key");
                    let payload = serde_json::json!({ "slot": slot, "pressed": pressed, "at": at });
                    if let Err(e) = app_handle.emit(crate::dictation::languages::KEY_EVENT, payload) {
                        warn!("Input hook: failed to emit language-key: {}", e);
                    }
                    continue;
                }
                info!("Input hook: emitting {}", event);
                if let Err(e) = app_handle.emit(event, at) {
                    warn!("Input hook: failed to emit {}: {}", event, e);
                }
            }
        })
        .expect("Failed to spawn input hook emit thread");

    std::thread::Builder::new()
        .name("input-hook".into())
        .spawn(|| {
            info!("Starting unified input hook (WH_KEYBOARD_LL + WH_MOUSE_LL)");

            unsafe {
                HOOK_THREAD.store(win32::GetCurrentThreadId(), Ordering::SeqCst);
                let hmod = win32::GetModuleHandleW(std::ptr::null());
                type HookProc = unsafe extern "system" fn(std::ffi::c_int, usize, isize) -> isize;
                let install = |id: std::ffi::c_int, proc_: HookProc| {
                    win32::SetWindowsHookExW(id, proc_, hmod, 0)
                };

                let mut kb_hook = install(win32::WH_KEYBOARD_LL, low_level_keyboard_proc);
                if kb_hook == 0 {
                    error!("Failed to install WH_KEYBOARD_LL hook");
                    return;
                }
                info!("WH_KEYBOARD_LL installed (handle: {})", kb_hook);

                let mut mouse_hook = install(win32::WH_MOUSE_LL, low_level_mouse_proc);
                if mouse_hook == 0 {
                    error!("Failed to install WH_MOUSE_LL hook");
                    win32::UnhookWindowsHookEx(kb_hook);
                    return;
                }
                info!("WH_MOUSE_LL installed (handle: {})", mouse_hook);

                // Heartbeat timer: logs, and re-installs the hooks (below);
                // the stuck-key sweep's (held.rs).
                let rehook_timer = win32::SetTimer(0, 0, 30_000, None);
                let sweep_timer = win32::SetTimer(0, 0, held::SWEEP_MS, None);
                // The lock screen, a UAC prompt: the input desktop switching.
                let desktop_hook = win32::SetWinEventHook(
                    win32::EVENT_SYSTEM_DESKTOPSWITCH,
                    win32::EVENT_SYSTEM_DESKTOPSWITCH,
                    0,
                    on_desktop_switch,
                    0,
                    0,
                    win32::WINEVENT_OUTOFCONTEXT,
                );
                if desktop_hook == 0 {
                    warn!("Input hook: no desktop-switch events (the timer's sweep still runs)");
                }

                info!("Input hooks ready — configure bindings via configure_dictation/configure_edit");

                // Message loop — REQUIRED for low-level hooks to fire
                let mut msg: win32::MSG = std::mem::zeroed();
                loop {
                    let ret = win32::GetMessageW(&mut msg, 0, 0, 0);
                    if ret <= 0 {
                        info!("Input hook: GetMessageW returned {}, exiting", ret);
                        break;
                    }

                    if msg.message == win32::WM_TIMER && msg.wparam == sweep_timer {
                        sweep(None);
                        continue;
                    }
                    if msg.message == win32::WM_APP_SWEEP {
                        sweep(None);
                        continue;
                    }
                    if msg.message == win32::WM_TIMER && msg.wparam == rehook_timer {
                        // Self-heal: Windows silently REMOVES a low-level hook
                        // whose callback ever exceeds LowLevelHooksTimeout —
                        // the hotkey would stay dead forever with no error. Cheap
                        // insurance: re-install both hooks every heartbeat, so a
                        // silently-removed hook revives within 30 s. (Same trick
                        // AutoHotkey uses.)
                        win32::UnhookWindowsHookEx(kb_hook);
                        win32::UnhookWindowsHookEx(mouse_hook);
                        kb_hook = install(win32::WH_KEYBOARD_LL, low_level_keyboard_proc);
                        mouse_hook = install(win32::WH_MOUSE_LL, low_level_mouse_proc);
                        if kb_hook == 0 || mouse_hook == 0 {
                            error!(
                                "Input hook re-install failed (kb={}, mouse={}) — retrying next heartbeat",
                                kb_hook, mouse_hook
                            );
                        }
                        let (dict_t, dict_c) = FIXED[DICTATION].type_and_code();
                        let (edit_t, edit_c) = FIXED[EDIT].type_and_code();
                        trace!(
                            "Input hook heartbeat (re-hooked): dict=({},{}) edit=({},{})",
                            dict_t, dict_c, edit_t, edit_c
                        );
                    }

                    win32::TranslateMessage(&msg);
                    win32::DispatchMessageW(&msg);
                }

                win32::UnhookWindowsHookEx(kb_hook);
                win32::UnhookWindowsHookEx(mouse_hook);
                info!("Input hooks removed, thread exiting");
            }
        })
        .expect("Failed to spawn input hook thread");
}

/// No-op on non-Windows platforms.
#[cfg(not(target_os = "windows"))]
pub fn start_input_hook(_app_handle: AppHandle) {
    info!("Input hook not available on this platform");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_keyboard_spec() {
        assert_eq!(parse_key_spec("kb:52").unwrap(), (KEY_TYPE_KEYBOARD, 52, 0));
        assert_eq!(
            parse_key_spec("kb:100").unwrap(),
            (KEY_TYPE_KEYBOARD, 100, 0)
        );
    }

    #[test]
    fn parse_combo_spec() {
        assert_eq!(
            parse_key_spec("kb:ctrl+shift+32").unwrap(),
            (KEY_TYPE_KEYBOARD, 32, MOD_CTRL | MOD_SHIFT)
        );
        assert_eq!(
            parse_key_spec("kb:alt+120").unwrap(),
            (KEY_TYPE_KEYBOARD, 120, MOD_ALT)
        );
        // modifier names are case-insensitive; win/super/meta are aliases
        assert_eq!(
            parse_key_spec("kb:Ctrl+Win+65").unwrap(),
            (KEY_TYPE_KEYBOARD, 65, MOD_CTRL | MOD_WIN)
        );
        // a right-side modifier as the main key is a plain vkey binding
        assert_eq!(
            parse_key_spec("kb:165").unwrap(),
            (KEY_TYPE_KEYBOARD, 165, 0)
        );
        assert!(parse_key_spec("kb:bogus+32").is_err());
    }

    #[test]
    fn parse_win_combos() {
        // The meeting shortcut's default, Win+Alt+M, as the recorder writes it
        // (ctrl, alt, shift, win order)…
        assert_eq!(
            parse_key_spec("kb:alt+win+77").unwrap(),
            (KEY_TYPE_KEYBOARD, 77, MOD_ALT | MOD_WIN)
        );
        // …or in any other order.
        assert_eq!(
            parse_key_spec("kb:win+alt+77").unwrap(),
            (KEY_TYPE_KEYBOARD, 77, MOD_ALT | MOD_WIN)
        );
        assert_eq!(
            parse_key_spec("kb:super+shift+77").unwrap(),
            (KEY_TYPE_KEYBOARD, 77, MOD_SHIFT | MOD_WIN)
        );
        assert_eq!(
            parse_key_spec(&crate::config::YapConfig::default().meeting_hotkey).unwrap(),
            (KEY_TYPE_KEYBOARD, 77, MOD_ALT | MOD_WIN)
        );
        assert_eq!(
            format_binding(KEY_TYPE_KEYBOARD, 77, MOD_ALT | MOD_WIN),
            "keyboard alt+win+vkey 77"
        );
        // A chord of Win + Alt alone is fine too.
        assert_eq!(
            parse_key_spec("mods:win+alt").unwrap(),
            (KEY_TYPE_MODS, 0, MOD_ALT | MOD_WIN)
        );
    }

    #[test]
    fn alt_and_win_combos_mask_their_release() {
        // The main key is swallowed, so Alt / Win would be released "alone".
        assert!(needs_menu_mask(MOD_ALT | MOD_WIN, false)); // Win+Alt+M
        assert!(needs_menu_mask(MOD_ALT, false)); // Alt+F9
        assert!(needs_menu_mask(MOD_CTRL | MOD_WIN, false));
        // Ctrl/Shift combos open nothing on release, bare keys need no
        // modifiers, and a modifier main key is never swallowed.
        assert!(!needs_menu_mask(MOD_CTRL | MOD_SHIFT, false));
        assert!(!needs_menu_mask(0, false));
        assert!(!needs_menu_mask(MOD_ALT, true));
    }

    #[test]
    fn parse_mods_only_spec() {
        assert_eq!(
            parse_key_spec("mods:ctrl+alt").unwrap(),
            (KEY_TYPE_MODS, 0, MOD_CTRL | MOD_ALT)
        );
        assert_eq!(
            parse_key_spec("mods:ctrl+alt+shift").unwrap(),
            (KEY_TYPE_MODS, 0, MOD_CTRL | MOD_ALT | MOD_SHIFT)
        );
        // a single modifier (or the same one twice) can't be a chord — it
        // would hijack every plain Ctrl press
        assert!(parse_key_spec("mods:ctrl").is_err());
        assert!(parse_key_spec("mods:ctrl+ctrl").is_err());
        assert!(parse_key_spec("mods:ctrl+bogus").is_err());
    }

    #[test]
    fn parse_mouse_spec() {
        assert_eq!(parse_key_spec("mouse:4").unwrap(), (KEY_TYPE_MOUSE, 4, 0));
        assert_eq!(parse_key_spec("mouse:5").unwrap(), (KEY_TYPE_MOUSE, 5, 0));
    }

    #[test]
    fn parse_legacy_spec() {
        assert_eq!(
            parse_key_spec("MouseButton4").unwrap(),
            (KEY_TYPE_MOUSE, 4, 0)
        );
        assert_eq!(
            parse_key_spec("MouseButton3").unwrap(),
            (KEY_TYPE_MOUSE, 3, 0)
        );
    }

    #[test]
    fn parse_empty_spec() {
        assert_eq!(parse_key_spec("").unwrap(), (KEY_TYPE_NONE, 0, 0));
    }

    /// Snip to Chat's Win+Alt+S: its own key, clashing with none of Yap's
    /// defaults; a hand-made binding on it wins (the snip's is unbound).
    #[test]
    fn the_snip_shortcut_is_win_alt_s_and_its_own_key() {
        let cfg = crate::config::YapConfig::default();
        assert_eq!(cfg.snip_hotkey, "kb:alt+win+83");
        assert_eq!(parse_key_spec(&cfg.snip_hotkey), Ok((KEY_TYPE_KEYBOARD, 83, MOD_ALT | MOD_WIN)));
        assert_eq!(format_binding(KEY_TYPE_KEYBOARD, 83, MOD_ALT | MOD_WIN), format_binding(KEY_TYPE_KEYBOARD, 83, MOD_WIN | MOD_ALT));
        for other in [&cfg.hotkey, &cfg.meeting_hotkey, &cfg.edit_hotkey, &cfg.assistant_hotkey, &cfg.read_hotkey, &cfg.as_said_hotkey] {
            assert!(!clashes(&cfg.snip_hotkey, other), "{other}");
        }
        // Someone's read-aloud shortcut on Win+Alt+S, or Alt+Win+S written
        // the other way round: a clash.
        assert!(clashes("kb:alt+win+83", "kb:win+alt+83"));
        assert!(clashes("kb:alt+win+83", "mods:alt+win"));
        assert!(!clashes("kb:alt+win+83", "kb:alt+win+77"));
        // The menu mask runs for it (Win and Alt held, S swallowed).
        assert!(needs_menu_mask(MOD_ALT | MOD_WIN, false));
    }

    /// Paste last and Copy last transcript: unbound until set; a hand-made
    /// key one of the older shortcuts has is left to it (and Copy last gives
    /// way to Paste last). (The one test that sets their bindings.)
    #[test]
    fn paste_and_copy_last_are_unbound_and_give_way_to_the_older_shortcuts() {
        let mut cfg = crate::config::YapConfig::default();
        assert_eq!(cfg.paste_last_hotkey, "");
        assert_eq!(cfg.copy_last_hotkey, "");
        configure_paste_last_from(&cfg);
        configure_copy_last_from(&cfg);
        assert_eq!(FIXED[PASTE_LAST].kind_code_mods().0, KEY_TYPE_NONE);
        assert_eq!(FIXED[COPY_LAST].kind_code_mods().0, KEY_TYPE_NONE);
        // Their own keys: bound.
        cfg.paste_last_hotkey = "kb:ctrl+alt+86".into();
        cfg.copy_last_hotkey = "mouse:4".into();
        configure_paste_last_from(&cfg);
        configure_copy_last_from(&cfg);
        assert_eq!(FIXED[PASTE_LAST].kind_code_mods(), (KEY_TYPE_KEYBOARD, 86, MOD_CTRL | MOD_ALT));
        assert!(FIXED[COPY_LAST].matches_mouse(4));
        // The snip's Win+Alt+S, and Copy last on Paste last's key: unbound.
        cfg.paste_last_hotkey = "kb:win+alt+83".into();
        configure_paste_last_from(&cfg);
        assert_eq!(FIXED[PASTE_LAST].kind_code_mods().0, KEY_TYPE_NONE);
        cfg.paste_last_hotkey = "kb:ctrl+alt+86".into();
        cfg.copy_last_hotkey = "kb:alt+ctrl+86".into();
        configure_paste_last_from(&cfg);
        configure_copy_last_from(&cfg);
        assert_eq!(FIXED[PASTE_LAST].kind_code_mods().1, 86);
        assert_eq!(FIXED[COPY_LAST].kind_code_mods().0, KEY_TYPE_NONE);
        // The transforms give way to both.
        let fixed = crate::dictation::transforms::fixed_shortcuts(&cfg);
        assert!(fixed.iter().any(|(name, spec)| *name == "Paste-last shortcut" && spec == "kb:ctrl+alt+86"));
        assert!(fixed.iter().any(|(name, _)| *name == "Copy-last shortcut"));
        configure_paste_last("").unwrap();
        configure_copy_last("").unwrap();
    }

    /// (The one test that sets the transform slots: tests run at once.)
    #[test]
    fn transform_shortcuts_get_slots_in_list_order_giving_way_to_the_fixed_seven() {
        use crate::dictation::transforms::Transform;
        let mut cfg = crate::config::YapConfig::default();
        let key = |id: &str, hotkey: &str| {
            let mut t = cfg.transforms.iter().find(|t| t.id == id).cloned().unwrap();
            t.hotkey = hotkey.into();
            t
        };
        let mine = Transform { id: "t-1".into(), name: "Mine".into(), instruction: "Do it.".into(), hotkey: "mouse:5".into(), ..Default::default() };
        let list = vec![
            key("polish", "kb:ctrl+alt+80"),
            key("shorter", ""),
            // The meeting shortcut's key: left for it.
            key("translate", "kb:win+alt+77"),
            mine,
        ];
        cfg.transforms = list;
        configure_transforms_from(&cfg);
        assert_eq!(transform_at(0).as_deref(), Some("polish"));
        assert_eq!(transform_at(1).as_deref(), Some("translate"));
        assert_eq!(transform_at(2).as_deref(), Some("t-1"));
        assert_eq!(transform_at(3), None);
        assert_eq!(TRANSFORM_BINDINGS[0].kind_code_mods(), (KEY_TYPE_KEYBOARD, 80, MOD_CTRL | MOD_ALT));
        assert_eq!(TRANSFORM_BINDINGS[1].kind_code_mods().0, KEY_TYPE_NONE, "clashes: unbound");
        assert!(TRANSFORM_BINDINGS[2].matches_mouse(5));
        // Settings pausing one while a key is picked, then putting it back.
        configure_transform("polish", "");
        assert_eq!(TRANSFORM_BINDINGS[0].kind_code_mods().0, KEY_TYPE_NONE);
        configure_transform("polish", "kb:ctrl+alt+80");
        assert_eq!(TRANSFORM_BINDINGS[0].kind_code_mods().1, 80);
        // One with no slot yet waits for the save.
        configure_transform("shorter", "kb:ctrl+alt+83");
        assert_eq!(TRANSFORM_BINDINGS[3].kind_code_mods().0, KEY_TYPE_NONE);
        // A slot's events: its press is a transform press, its release isn't.
        assert_eq!(transform_slot("transform-key-2"), Some(2));
        assert_eq!(transform_slot("transform-key-2-up"), None);
        assert_eq!(transform_slot("edit-key-pressed"), None);
        assert_eq!(TRANSFORM_EVENTS.len(), crate::dictation::transforms::MAX);
        // Nothing listed: every slot unbound.
        cfg.transforms.clear();
        configure_transforms_from(&cfg);
        assert!(TRANSFORM_BINDINGS.iter().all(|b| b.kind_code_mods().0 == KEY_TYPE_NONE));
        assert_eq!(transform_at(0), None);
    }

    #[test]
    fn language_shortcuts_get_slots_giving_way_to_the_fixed_nine_and_transforms_to_them() {
        use crate::dictation::languages::LanguageShortcut;
        let sc = |id: &str, language: &str, to: &str, hotkey: &str| LanguageShortcut {
            id: id.into(),
            language: language.into(),
            translate_to: to.into(),
            hotkey: hotkey.into(),
        };
        let mut cfg = crate::config::YapConfig {
            language_shortcuts: vec![
                sc("l-fr", "fr", "", "kb:ctrl+alt+70"),
                // The dictation hotkey's key: left for it.
                sc("l-de", "de", "", &crate::config::YapConfig::default().hotkey),
                sc("l-es", "en", "es", "mouse:4"),
                // Copy last's key (one of the nine): left for it too.
                sc("l-it", "it", "", "kb:ctrl+alt+67"),
            ],
            copy_last_hotkey: "kb:ctrl+alt+67".into(),
            ..Default::default()
        };
        // A transform on the French shortcut's key gives way to it (the
        // slots' rule, pure: another test owns the transform slots).
        cfg.transforms[0].hotkey = "kb:ctrl+alt+70".into();
        let fixed = crate::dictation::transforms::fixed_shortcuts(&cfg);
        let fixed: Vec<(&str, &str)> = fixed.iter().map(|(n, s)| (*n, s.as_str())).collect();
        let slots = crate::dictation::transforms::assign(&fixed, &crate::dictation::transforms::list(&cfg));
        assert_eq!(slots[0].clash.as_deref(), Some("the language shortcut"));
        configure_languages_from(&cfg);
        assert_eq!(language_at(0).as_deref(), Some("l-fr"));
        assert_eq!(language_at(1).as_deref(), Some("l-de"));
        assert_eq!(language_at(2).as_deref(), Some("l-es"));
        assert_eq!(LANGUAGE_BINDINGS[0].kind_code_mods(), (KEY_TYPE_KEYBOARD, 70, MOD_CTRL | MOD_ALT));
        assert_eq!(LANGUAGE_BINDINGS[1].kind_code_mods().0, KEY_TYPE_NONE, "clashes: unbound");
        assert!(LANGUAGE_BINDINGS[2].matches_mouse(4));
        assert_eq!(language_at(3).as_deref(), Some("l-it"));
        assert_eq!(LANGUAGE_BINDINGS[3].kind_code_mods().0, KEY_TYPE_NONE, "Copy last's key: unbound");
        // Settings pausing one while a key is picked, then putting it back.
        configure_language("l-fr", "");
        assert_eq!(LANGUAGE_BINDINGS[0].kind_code_mods().0, KEY_TYPE_NONE);
        configure_language("l-fr", "kb:ctrl+alt+70");
        assert_eq!(LANGUAGE_BINDINGS[0].kind_code_mods().1, 70);
        // Both of a slot's events go out, the press and the release.
        assert_eq!(language_slot("language-key-2"), Some((2, true)));
        assert_eq!(language_slot("language-key-2-up"), Some((2, false)));
        assert_eq!(language_slot("transform-key-2"), None);
        assert_eq!(LANGUAGE_EVENTS.len(), crate::dictation::languages::MAX_SHORTCUTS);
        cfg.language_shortcuts.clear();
        configure_languages_from(&cfg);
        assert!(LANGUAGE_BINDINGS.iter().all(|b| b.kind_code_mods().0 == KEY_TYPE_NONE));
        assert_eq!(language_at(0), None);
    }

    /// More shortcuts for one action: each gives way to every first
    /// shortcut and to the more shortcuts before it, its own action's
    /// included; the language shortcuts and transforms give way to them.
    #[test]
    fn more_shortcuts_give_way_to_every_first_one_and_earlier_ones() {
        let firsts = [
            ("dictation hotkey", "kb:120"),
            ("rewrite shortcut", ""),
            ("meeting shortcut", "kb:alt+win+77"),
        ];
        let none = Vec::<String>::new;
        let more = vec![
            // Dictation: a mouse button (fine), F9 again (its own first
            // one: a clash), Ctrl+Alt (fine).
            vec!["mouse:4".to_string(), "kb:120".to_string(), "mods:ctrl+alt".to_string()],
            // Rewrite: the dictation's mouse button (taken), Ctrl+Alt+K
            // (Ctrl+Alt completes inside it: taken).
            vec!["mouse:4".to_string(), "kb:ctrl+alt+75".to_string()],
            // Meeting: Win+Alt+M written the other way (its own first one).
            vec!["kb:win+alt+77".to_string()],
            none(),
            none(),
            none(),
            none(),
            none(),
            none(),
        ];
        let slots = assign_more(&firsts, &more);
        let got: Vec<(usize, usize, &str, Option<&str>)> =
            slots.iter().map(|s| (s.action, s.slot, s.spec.as_str(), s.clash.as_deref())).collect();
        assert_eq!(
            got,
            vec![
                (0, 0, "mouse:4", None),
                (0, 1, "", Some("the dictation hotkey")),
                (0, 2, "mods:ctrl+alt", None),
                (1, 0, "", Some("the dictation hotkey")),
                (1, 1, "", Some("the dictation hotkey")),
                (2, 0, "", Some("the meeting shortcut")),
            ]
        );
        // Never more than MORE for one action.
        let lots = vec![vec!["mouse:3".into(), "mouse:4".into(), "mouse:5".into(), "kb:121".into()]];
        assert_eq!(assign_more(&[], &lots).len(), MORE);
    }

    /// (The one test that sets the more shortcuts' bindings.)
    #[test]
    fn more_shortcuts_are_read_from_the_config_and_feed_the_slots_rule() {
        let mut cfg = crate::config::YapConfig::default();
        cfg.more_hotkeys.insert("hotkey".into(), vec!["mouse:5".into(), "  ".into(), "kb:ctrl+alt+68".into()]);
        cfg.more_hotkeys.insert("snipHotkey".into(), vec!["kb:121".into()]);
        cfg.more_hotkeys.insert("nonsense".into(), vec!["kb:122".into()]);
        assert_eq!(more_specs(&cfg, DICTATION), vec!["mouse:5".to_string(), "kb:ctrl+alt+68".to_string()]);
        assert_eq!(first_spec(&cfg, SNIP), cfg.snip_hotkey);
        configure_more_from(&cfg);
        assert!(MORE_BINDINGS[DICTATION][0].matches_mouse(5));
        assert_eq!(MORE_BINDINGS[DICTATION][1].kind_code_mods(), (KEY_TYPE_KEYBOARD, 68, MOD_CTRL | MOD_ALT));
        assert_eq!(MORE_BINDINGS[DICTATION][2].kind_code_mods().0, KEY_TYPE_NONE);
        assert_eq!(MORE_BINDINGS[SNIP][0].kind_code_mods(), (KEY_TYPE_KEYBOARD, 121, 0));
        // Language shortcuts and transforms give way to them.
        let nine = crate::dictation::transforms::nine_shortcuts(&cfg);
        assert!(nine.iter().any(|(name, spec)| *name == "dictation hotkey" && spec == "mouse:5"));
        assert!(nine.iter().any(|(name, spec)| *name == "snip shortcut" && spec == "kb:121"));
        // Settings pausing one while a key is picked, then putting them back.
        configure_more("hotkey", &["".into(), "kb:ctrl+alt+68".into()]).unwrap();
        assert_eq!(MORE_BINDINGS[DICTATION][0].kind_code_mods().0, KEY_TYPE_NONE);
        assert!(configure_more("nonsense", &[]).is_err());
        // An empty list unbinds them all.
        cfg.more_hotkeys.clear();
        configure_more_from(&cfg);
        assert!(MORE_BINDINGS.iter().flatten().all(|b| b.kind_code_mods().0 == KEY_TYPE_NONE));
    }

    /// A config from before (one key per action) reads as ever: its keys
    /// stay the first, and no more. A hand-edited list is tidied.
    #[test]
    fn old_configs_read_correctly_and_hand_edited_lists_are_tidied() {
        let old: crate::config::YapConfig =
            serde_json::from_str(r#"{"hotkey":"kb:121","editHotkey":"mouse:4","meetingHotkey":""}"#).unwrap();
        let mut cfg = old.clone();
        tidy_more(&mut cfg);
        assert_eq!((cfg.hotkey.as_str(), cfg.edit_hotkey.as_str()), ("kb:121", "mouse:4"));
        assert!(cfg.more_hotkeys.is_empty());
        assert!((0..FIXED_COUNT).all(|i| more_specs(&cfg, i).is_empty()));

        let mut cfg = old;
        cfg.more_hotkeys.insert("hotkey".into(), vec![" mouse:5 ".into(), "kb:121".into(), "".into(), "mouse:5".into(), "kb:122".into(), "kb:123".into(), "kb:124".into()]);
        // The meeting shortcut taken away by hand, one more left: it moves up.
        cfg.more_hotkeys.insert("meetingHotkey".into(), vec!["kb:ctrl+alt+77".into()]);
        cfg.more_hotkeys.insert("bogus".into(), vec!["kb:125".into()]);
        cfg.more_hotkeys.insert("readHotkey".into(), vec!["  ".into()]);
        tidy_more(&mut cfg);
        assert_eq!(cfg.more_hotkeys.get("hotkey").unwrap(), &vec!["mouse:5".to_string(), "kb:122".into(), "kb:123".into()]);
        assert_eq!(cfg.meeting_hotkey, "kb:ctrl+alt+77");
        assert!(!cfg.more_hotkeys.contains_key("meetingHotkey"));
        assert!(!cfg.more_hotkeys.contains_key("bogus"));
        assert!(!cfg.more_hotkeys.contains_key("readHotkey"));
        // Tidy twice: the same.
        let again = cfg.more_hotkeys.clone();
        tidy_more(&mut cfg);
        assert_eq!(cfg.more_hotkeys, again);
        // And it saves under its camelCase name.
        let json = serde_json::to_value(&cfg).unwrap();
        assert_eq!(json["moreHotkeys"]["hotkey"][0], "mouse:5");
    }

    /// The data folder is shared with an older Yap (the installed nightly
    /// beside a dev build): one that doesn't know `moreHotkeys` reads a
    /// config that has them and saves it as it knows it. Every action's
    /// first shortcut survives, in the shape it always had; only the more
    /// ones (and triple-tap) are dropped.
    #[test]
    fn an_older_yap_saving_this_config_keeps_every_first_shortcut() {
        // An older Yap's config, as far as shortcuts go: the nine fields it
        // knows (serde ignores the rest, as YapConfig always has).
        #[derive(serde::Deserialize, serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Older {
            hotkey: String,
            edit_hotkey: String,
            meeting_hotkey: String,
            assistant_hotkey: String,
            read_hotkey: String,
            as_said_hotkey: String,
            snip_hotkey: String,
            paste_last_hotkey: String,
            copy_last_hotkey: String,
        }
        let mut cfg = crate::config::YapConfig {
            hotkey: "kb:121".into(),
            edit_hotkey: "mouse:5".into(),
            triple_tap_meeting: true,
            ..Default::default()
        };
        cfg.more_hotkeys.insert("hotkey".into(), vec!["mouse:4".into(), "kb:ctrl+alt+68".into()]);
        let saved = serde_json::to_value(&cfg).unwrap();
        // This Yap writes the first shortcut as it always was: one spec.
        assert_eq!(saved["hotkey"], "kb:121");
        let older: Older = serde_json::from_value(saved).unwrap();
        let resaved = serde_json::to_string(&older).unwrap();
        // …and reads the older Yap's save back.
        let mut back: crate::config::YapConfig = serde_json::from_str(&resaved).unwrap();
        tidy_more(&mut back);
        let firsts: Vec<&str> = (0..FIXED_COUNT).map(|i| first_spec(&back, i)).collect();
        let want: Vec<&str> = (0..FIXED_COUNT).map(|i| first_spec(&cfg, i)).collect();
        assert_eq!(firsts, want, "every first shortcut survives an older Yap's save");
        assert_eq!(back.hotkey, "kb:121");
        assert_eq!(back.meeting_hotkey, crate::config::YapConfig::default().meeting_hotkey);
        // What the older save drops: the more shortcuts and triple-tap.
        assert!(back.more_hotkeys.is_empty());
        assert!(!back.triple_tap_meeting);
        // A config with no more shortcuts is the old shape plus an empty map.
        let plain = serde_json::to_value(crate::config::YapConfig::default()).unwrap();
        assert_eq!(plain["moreHotkeys"], serde_json::json!({}));
        assert!(plain["hotkey"].is_string());
    }

    #[test]
    fn the_held_table_keeps_each_key_down_until_it_comes_up() {
        // (Virtual keys no other test or person touches: 0xFC, 0xFD.)
        assert!(!is_held(0xFC));
        assert!(!set_held(0xFC, true), "it wasn't down: a new key");
        assert!(set_held(0xFC, true), "down again: a repeat");
        assert!(is_held(0xFC));
        assert!(held_keys().contains(&0xFC));
        assert!(!is_held(0xFD));
        assert!(set_held(0xFC, false));
        assert!(!is_held(0xFC));
        // Out of range: never held.
        assert!(!set_held(300, true));
        assert!(!is_held(300));
    }

    #[test]
    fn the_same_key_clashes_however_it_is_written() {
        assert!(clashes("kb:120", "kb:120"));
        assert!(clashes("kb:alt+win+77", "kb:win+alt+77"));
        assert!(clashes("mouse:4", "MouseButton4"));
        assert!(clashes("mods:ctrl+alt", "mods:alt+ctrl"));
        // Different keys and buttons never do.
        assert!(!clashes("kb:120", "kb:121"));
        assert!(!clashes("mouse:4", "mouse:5"));
        assert!(!clashes("kb:4", "mouse:4"));
    }

    #[test]
    fn a_bare_key_and_its_combos_are_different_keys() {
        // F9 fires only with no modifier held (Ctrl+F9 passes through).
        assert!(!clashes("kb:120", "kb:ctrl+120"));
        assert!(!clashes("kb:alt+win+77", "kb:77"));
        // …unless the bare key is a modifier itself: RightAlt fires inside
        // a RightAlt combo, and inside a chord holding Alt.
        assert!(clashes("kb:165", "kb:ctrl+165"));
        assert!(clashes("kb:165", "mods:ctrl+alt"));
        assert!(!clashes("kb:165", "mods:ctrl+shift"));
        assert!(!clashes("kb:120", "mods:ctrl+alt"));
    }

    #[test]
    fn a_combo_inside_a_bigger_one_clashes() {
        // Ctrl+Space fires inside Ctrl+Shift+Space (extra modifiers are
        // tolerated): one press would start both.
        assert!(clashes("kb:ctrl+32", "kb:ctrl+shift+32"));
        assert!(clashes("kb:ctrl+shift+32", "kb:ctrl+32"));
        // Different modifiers on one key don't nest.
        assert!(!clashes("kb:ctrl+32", "kb:alt+32"));
        assert!(!clashes("kb:ctrl+alt+32", "kb:ctrl+shift+32"));
    }

    #[test]
    fn a_chord_clashes_with_what_completes_it() {
        // Ctrl+Alt completes on the way to Ctrl+Alt+K…
        assert!(clashes("mods:ctrl+alt", "kb:ctrl+alt+75"));
        assert!(clashes("kb:ctrl+alt+shift+75", "mods:ctrl+alt"));
        // …and inside a bigger chord.
        assert!(clashes("mods:ctrl+alt", "mods:ctrl+alt+shift"));
        // A combo not holding all of it is fine.
        assert!(!clashes("mods:ctrl+alt", "kb:ctrl+75"));
        assert!(!clashes("mods:ctrl+alt", "mods:ctrl+shift"));
        // The meeting shortcut's default against a Win+Alt chord.
        assert!(clashes("mods:win+alt", &crate::config::YapConfig::default().meeting_hotkey));
    }

    #[test]
    fn nothing_clashes_with_an_unbound_or_unreadable_key() {
        assert!(!clashes("", ""));
        assert!(!clashes("", "kb:120"));
        assert!(!clashes("kb:120", "  "));
        assert!(!clashes("garbage", "garbage"));
    }

    #[test]
    fn parse_invalid_spec() {
        assert!(parse_key_spec("garbage").is_err());
        assert!(parse_key_spec("kb:notanumber").is_err());
    }

    #[test]
    fn format_binding_describes_combos() {
        assert_eq!(
            format_binding(KEY_TYPE_KEYBOARD, 32, MOD_CTRL | MOD_SHIFT),
            "keyboard ctrl+shift+vkey 32"
        );
        assert_eq!(
            format_binding(KEY_TYPE_MODS, 0, MOD_CTRL | MOD_ALT),
            "modifier chord ctrl+alt"
        );
        assert_eq!(format_binding(KEY_TYPE_KEYBOARD, 120, 0), "keyboard vkey 120");
    }
}

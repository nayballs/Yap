//! Unified input hook for the dictation, edit (Commands' rewrite), meeting,
//! Voice Assistant, Read aloud, "Use what I said", Snip to Chat, Paste last
//! transcript and Copy last transcript key bindings (the last four act on
//! their press only), and up to `transforms::MAX` **saved transforms**' shortcuts
//! (a slot each, [`configure_transforms_from`]; only their press, as
//! `transform-key-pressed` with the slot). The snip shortcut (Win+Alt+S by
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

// FFI type names match Win32 API conventions (HHOOK, POINT, MSG, etc.)
#![allow(clippy::upper_case_acronyms)]

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
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
}

impl KeyBinding {
    const fn new() -> Self {
        Self {
            key_type: AtomicU8::new(KEY_TYPE_NONE),
            key_code: AtomicU32::new(0),
            mods: AtomicU8::new(0),
            active: AtomicBool::new(false),
        }
    }

    fn configure(&self, key_type: u8, key_code: u32, mods: u8) {
        self.active.store(false, Ordering::Relaxed);
        self.key_type.store(key_type, Ordering::Release);
        self.key_code.store(key_code, Ordering::Release);
        self.mods.store(mods, Ordering::Release);
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

static DICTATION_BINDING: KeyBinding = KeyBinding::new();
static EDIT_BINDING: KeyBinding = KeyBinding::new();
static MEETING_BINDING: KeyBinding = KeyBinding::new();
static ASSISTANT_BINDING: KeyBinding = KeyBinding::new();
static READ_BINDING: KeyBinding = KeyBinding::new();
/// "Use what I said" (`as_said.rs`): swap the last cleaned dictation's
/// paste for the words as said. Unbound by default.
static AS_SAID_BINDING: KeyBinding = KeyBinding::new();
/// Snip to Chat (`snip.rs`): drag a box over any screen. Win+Alt+S by
/// default; its press only.
static SNIP_BINDING: KeyBinding = KeyBinding::new();
/// Paste last transcript (`pasting/last.rs`): the latest dictation again,
/// where you're typing. Unbound by default; its press only.
static PASTE_LAST_BINDING: KeyBinding = KeyBinding::new();
/// Copy last transcript (`pasting/last.rs`): the latest dictation onto the
/// clipboard. Unbound by default; its press only.
static COPY_LAST_BINDING: KeyBinding = KeyBinding::new();
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

// ---- Public API ----

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
    let (key_type, key_code, mods) = parse_key_spec(key_spec)?;
    DICTATION_BINDING.configure(key_type, key_code, mods);
    let desc = format_binding(key_type, key_code, mods);
    info!("Dictation key configured: {} (spec: {:?})", desc, key_spec);
    Ok(desc)
}

/// Parse a key spec string and configure the edit/rewrite-mode binding. An empty
/// spec unbinds it (edit mode is opt-in).
pub fn configure_edit(key_spec: &str) -> Result<String, String> {
    let (key_type, key_code, mods) = parse_key_spec(key_spec)?;
    EDIT_BINDING.configure(key_type, key_code, mods);
    let desc = format_binding(key_type, key_code, mods);
    info!("Edit key configured: {} (spec: {:?})", desc, key_spec);
    Ok(desc)
}

/// Parse a key spec string and configure the meeting shortcut (start taking
/// notes / stop and summarise — `meeting_guard`). An empty spec unbinds it.
pub fn configure_meeting(key_spec: &str) -> Result<String, String> {
    let (key_type, key_code, mods) = parse_key_spec(key_spec)?;
    MEETING_BINDING.configure(key_type, key_code, mods);
    let desc = format_binding(key_type, key_code, mods);
    info!("Meeting key configured: {} (spec: {:?})", desc, key_spec);
    Ok(desc)
}

/// Parse a key spec string and configure the **Voice Assistant** shortcut
/// (`assistant.rs`: a question to Yap's Chat, answered out loud). An empty
/// spec unbinds it (the default).
pub fn configure_assistant(key_spec: &str) -> Result<String, String> {
    let (key_type, key_code, mods) = parse_key_spec(key_spec)?;
    ASSISTANT_BINDING.configure(key_type, key_code, mods);
    let desc = format_binding(key_type, key_code, mods);
    info!("Assistant key configured: {} (spec: {:?})", desc, key_spec);
    Ok(desc)
}

/// Parse a key spec string and configure the **Read aloud** shortcut
/// (`read_aloud.rs`: the selection in the app you're in, read aloud). An
/// empty spec unbinds it (the default).
pub fn configure_read(key_spec: &str) -> Result<String, String> {
    let (key_type, key_code, mods) = parse_key_spec(key_spec)?;
    READ_BINDING.configure(key_type, key_code, mods);
    let desc = format_binding(key_type, key_code, mods);
    info!("Read-aloud key configured: {} (spec: {:?})", desc, key_spec);
    Ok(desc)
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
    let (key_type, key_code, mods) = parse_key_spec(key_spec)?;
    AS_SAID_BINDING.configure(key_type, key_code, mods);
    let desc = format_binding(key_type, key_code, mods);
    info!("Use-what-I-said key configured: {} (spec: {:?})", desc, key_spec);
    Ok(desc)
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
    let (key_type, key_code, mods) = parse_key_spec(key_spec)?;
    SNIP_BINDING.configure(key_type, key_code, mods);
    let desc = format_binding(key_type, key_code, mods);
    info!("Snip key configured: {} (spec: {:?})", desc, key_spec);
    Ok(desc)
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
    let (key_type, key_code, mods) = parse_key_spec(key_spec)?;
    PASTE_LAST_BINDING.configure(key_type, key_code, mods);
    let desc = format_binding(key_type, key_code, mods);
    info!("Paste-last key configured: {} (spec: {:?})", desc, key_spec);
    Ok(desc)
}

/// Parse a key spec string and configure the **Copy last transcript**
/// shortcut (`pasting/last.rs`). An empty spec unbinds it (the default).
pub fn configure_copy_last(key_spec: &str) -> Result<String, String> {
    let (key_type, key_code, mods) = parse_key_spec(key_spec)?;
    COPY_LAST_BINDING.configure(key_type, key_code, mods);
    let desc = format_binding(key_type, key_code, mods);
    info!("Copy-last key configured: {} (spec: {:?})", desc, key_spec);
    Ok(desc)
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
    }
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
#[cfg(target_os = "windows")]
fn handle_binding_event(
    binding: &KeyBinding,
    event_pressed: &'static str,
    event_released: &'static str,
    is_press: bool,
) -> bool {
    if is_press {
        if let Some(tx) = EVENT_TX.get() {
            // Only emit on first press (not repeats)
            if !binding.active.swap(true, Ordering::Relaxed) {
                info!("Input hook: queueing {}", event_pressed);
                let _ = tx.send((event_pressed, crate::dictation::gesture::now_ms()));
            }
        }
        true
    } else if binding.active.swap(false, Ordering::Relaxed) {
        if let Some(tx) = EVENT_TX.get() {
            info!("Input hook: queueing {}", event_released);
            let _ = tx.send((event_released, crate::dictation::gesture::now_ms()));
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
                    handle_binding_event(binding, pressed, released, true);
                    return !bound_is_modifier; // never suppress modifier keys
                }
                let ours = handle_binding_event(binding, pressed, released, false);
                return ours && !bound_is_modifier;
            }
            // Another key's event while our press is active: two safety rules.
            if binding.active.load(Ordering::Relaxed) {
                let family = modifier_family(vkey);
                if required != 0 && !is_keydown && family != 0 && required & family != 0 {
                    // 1. A required modifier was released → end the press.
                    if mods_now(vkey, false) & required != required {
                        handle_binding_event(binding, pressed, released, false);
                    }
                } else if bound_is_modifier
                    && unsafe { win32::GetAsyncKeyState(code as i32) } >= 0
                {
                    // 2. Self-heal a missed main-key keyup — ONLY for
                    // unsuppressed (modifier) bindings: a suppressed key never
                    // registers in GetAsyncKeyState (the hook eats the event
                    // before the key-state table updates), so an async check
                    // on it would "heal" a perfectly live press.
                    handle_binding_event(binding, pressed, released, false);
                }
            }
            false
        }
        KEY_TYPE_MODS => {
            let held = mods_now(vkey, is_keydown);
            if is_keydown {
                if held & required == required {
                    handle_binding_event(binding, pressed, released, true);
                }
            } else if binding.active.load(Ordering::Relaxed) && held & required != required {
                handle_binding_event(binding, pressed, released, false);
            }
            false // chords are made of modifiers — never suppressed
        }
        _ => false,
    }
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

        let is_keydown = msg == win32::WM_KEYDOWN || msg == win32::WM_SYSKEYDOWN;

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
        for (binding, pressed, released) in [
            (
                &DICTATION_BINDING,
                "dictation-key-pressed",
                "dictation-key-released",
            ),
            (&EDIT_BINDING, "edit-key-pressed", "edit-key-released"),
            (&MEETING_BINDING, "meeting-key-pressed", "meeting-key-released"),
            (&ASSISTANT_BINDING, "assistant-key-pressed", "assistant-key-released"),
            (&READ_BINDING, "read-key-pressed", "read-key-released"),
            (&AS_SAID_BINDING, "as-said-key-pressed", "as-said-key-released"),
            (&SNIP_BINDING, "snip-key-pressed", "snip-key-released"),
            (&PASTE_LAST_BINDING, "paste-last-key-pressed", "paste-last-key-released"),
            (&COPY_LAST_BINDING, "copy-last-key-pressed", "copy-last-key-released"),
        ] {
            suppress |= process_keyboard_event(binding, pressed, released, vkey, is_keydown);
        }
        // Saved transforms' shortcuts (an unbound slot is never matched).
        for (binding, (pressed, released)) in TRANSFORM_BINDINGS.iter().zip(TRANSFORM_EVENTS) {
            suppress |= process_keyboard_event(binding, pressed, released, vkey, is_keydown);
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

        let (is_press, button_id) = match msg {
            win32::WM_MBUTTONDOWN => (true, Some(3u32)),
            win32::WM_MBUTTONUP => (false, Some(3u32)),
            win32::WM_XBUTTONDOWN | win32::WM_XBUTTONUP => {
                let hook_info = &*(lparam as *const win32::MSLLHOOKSTRUCT);
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
            // Check dictation binding — suppress the mouse button at OS level
            if DICTATION_BINDING.matches_mouse(id) {
                handle_binding_event(
                    &DICTATION_BINDING,
                    "dictation-key-pressed",
                    "dictation-key-released",
                    is_press,
                );
                return 1;
            }

            // Check edit/rewrite binding — suppress the mouse button at OS level
            if EDIT_BINDING.matches_mouse(id) {
                handle_binding_event(
                    &EDIT_BINDING,
                    "edit-key-pressed",
                    "edit-key-released",
                    is_press,
                );
                return 1;
            }

            // …and the meeting shortcut.
            if MEETING_BINDING.matches_mouse(id) {
                handle_binding_event(
                    &MEETING_BINDING,
                    "meeting-key-pressed",
                    "meeting-key-released",
                    is_press,
                );
                return 1;
            }

            // …and the Voice Assistant's.
            if ASSISTANT_BINDING.matches_mouse(id) {
                handle_binding_event(
                    &ASSISTANT_BINDING,
                    "assistant-key-pressed",
                    "assistant-key-released",
                    is_press,
                );
                return 1;
            }

            // …and Read aloud's.
            if READ_BINDING.matches_mouse(id) {
                handle_binding_event(&READ_BINDING, "read-key-pressed", "read-key-released", is_press);
                return 1;
            }

            // …and "Use what I said".
            if AS_SAID_BINDING.matches_mouse(id) {
                handle_binding_event(&AS_SAID_BINDING, "as-said-key-pressed", "as-said-key-released", is_press);
                return 1;
            }

            // …and Snip to Chat's.
            if SNIP_BINDING.matches_mouse(id) {
                handle_binding_event(&SNIP_BINDING, "snip-key-pressed", "snip-key-released", is_press);
                return 1;
            }

            // …and Paste last / Copy last transcript's.
            for (binding, pressed, released) in [
                (&PASTE_LAST_BINDING, "paste-last-key-pressed", "paste-last-key-released"),
                (&COPY_LAST_BINDING, "copy-last-key-pressed", "copy-last-key-released"),
            ] {
                if binding.matches_mouse(id) {
                    handle_binding_event(binding, pressed, released, is_press);
                    return 1;
                }
            }

            // …and the saved transforms'.
            for (binding, (pressed, released)) in TRANSFORM_BINDINGS.iter().zip(TRANSFORM_EVENTS) {
                if binding.matches_mouse(id) {
                    handle_binding_event(binding, pressed, released, is_press);
                    return 1;
                }
            }
        }
    }

    win32::CallNextHookEx(0, code, wparam, lparam)
}

// ---- Start the unified hook ----

/// Start the unified input hook on a background thread.
///
/// Installs both WH_KEYBOARD_LL and WH_MOUSE_LL hooks. The hooks check
/// configured key bindings (set via `configure_dictation`/`configure_edit`)
/// and emit Tauri events when matched. Keyboard keys are suppressed at the
/// OS level to prevent them from reaching other applications.
#[cfg(target_os = "windows")]
pub fn start_input_hook(app_handle: AppHandle) {
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

                // Heartbeat timer: logs, and re-installs the hooks (below).
                win32::SetTimer(0, 1, 30_000, None);

                info!("Input hooks ready — configure bindings via configure_dictation/configure_edit");

                // Message loop — REQUIRED for low-level hooks to fire
                let mut msg: win32::MSG = std::mem::zeroed();
                loop {
                    let ret = win32::GetMessageW(&mut msg, 0, 0, 0);
                    if ret <= 0 {
                        info!("Input hook: GetMessageW returned {}, exiting", ret);
                        break;
                    }

                    if msg.message == win32::WM_TIMER {
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
                        let (dict_t, dict_c) = DICTATION_BINDING.type_and_code();
                        let (edit_t, edit_c) = EDIT_BINDING.type_and_code();
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
        assert_eq!(PASTE_LAST_BINDING.kind_code_mods().0, KEY_TYPE_NONE);
        assert_eq!(COPY_LAST_BINDING.kind_code_mods().0, KEY_TYPE_NONE);
        // Their own keys: bound.
        cfg.paste_last_hotkey = "kb:ctrl+alt+86".into();
        cfg.copy_last_hotkey = "mouse:4".into();
        configure_paste_last_from(&cfg);
        configure_copy_last_from(&cfg);
        assert_eq!(PASTE_LAST_BINDING.kind_code_mods(), (KEY_TYPE_KEYBOARD, 86, MOD_CTRL | MOD_ALT));
        assert!(COPY_LAST_BINDING.matches_mouse(4));
        // The snip's Win+Alt+S, and Copy last on Paste last's key: unbound.
        cfg.paste_last_hotkey = "kb:win+alt+83".into();
        configure_paste_last_from(&cfg);
        assert_eq!(PASTE_LAST_BINDING.kind_code_mods().0, KEY_TYPE_NONE);
        cfg.paste_last_hotkey = "kb:ctrl+alt+86".into();
        cfg.copy_last_hotkey = "kb:alt+ctrl+86".into();
        configure_paste_last_from(&cfg);
        configure_copy_last_from(&cfg);
        assert_eq!(PASTE_LAST_BINDING.kind_code_mods().1, 86);
        assert_eq!(COPY_LAST_BINDING.kind_code_mods().0, KEY_TYPE_NONE);
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

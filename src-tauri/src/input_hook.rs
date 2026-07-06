//! Unified input hook for the dictation + edit key bindings.
//!
//! Installs both WH_KEYBOARD_LL and WH_MOUSE_LL hooks to capture:
//! - Keyboard keys (including those sent by mouse side buttons via driver software)
//! - Mouse extra buttons (XBUTTON1/2, middle)
//!
//! Configured keys are **suppressed** at the OS level (keyboard hooks only),
//! preventing "44444" in text fields when holding a bound key. Push-to-talk is
//! NOT a separate binding: the dictation key emits both press and release, and
//! the pipeline's `recording_mode` decides toggle vs hold.

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

/// A configurable key binding (dictation key, edit key).
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

/// Channel from the hook callback to the emit-forwarder thread. The callback
/// must NEVER do slow work (like `app.emit`, which can block on a busy webview):
/// Windows silently REMOVES a low-level hook whose callback exceeds the
/// LowLevelHooksTimeout (~300 ms) — the hotkey then goes dead with no error.
/// So the callback only does atomics + a non-blocking channel send.
#[cfg(target_os = "windows")]
static EVENT_TX: std::sync::OnceLock<std::sync::mpsc::Sender<&'static str>> =
    std::sync::OnceLock::new();

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
    }
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
                let _ = tx.send(event_pressed);
            }
        }
        true
    } else if binding.active.swap(false, Ordering::Relaxed) {
        if let Some(tx) = EVENT_TX.get() {
            info!("Input hook: queueing {}", event_released);
            let _ = tx.send(event_released);
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

        let mut suppress = false;
        for (binding, pressed, released) in [
            (
                &DICTATION_BINDING,
                "dictation-key-pressed",
                "dictation-key-released",
            ),
            (&EDIT_BINDING, "edit-key-pressed", "edit-key-released"),
        ] {
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
    let (tx, rx) = std::sync::mpsc::channel::<&'static str>();
    EVENT_TX.set(tx).expect("Input hook event channel already set");
    std::thread::Builder::new()
        .name("input-hook-emit".into())
        .spawn(move || {
            while let Ok(event) = rx.recv() {
                info!("Input hook: emitting {}", event);
                if let Err(e) = app_handle.emit(event, ()) {
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

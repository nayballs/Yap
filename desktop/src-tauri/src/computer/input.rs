//! **Clicks, keys and typing** (phase 2): each of the desktop contract's
//! input actions as the low-level steps that make it ([`steps_for`], pure),
//! and the steps sent ([`Hands`]: this PC's `SendInput`, or the stand-in
//! desktop's).
//!
//! - **Every event is Yap's own** (`dwExtraInfo` = `input_hook::OWN_MARK`):
//!   Yap's own hook lets it by, so a model typing Win+Alt+S never fires Yap's
//!   shortcuts, and the watch for the person's input ([`super::watch`])
//!   never takes it for theirs.
//! - **The pointer** goes in absolute virtual-desktop coordinates
//!   (`MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK`: without
//!   `VIRTUALDESK` the range is the primary display only, and displays can
//!   start at negative coordinates).
//! - **Keys** go by virtual key and scan code, extended where Windows wants
//!   it (arrows, Home, Delete…); a combo's keys go down in order and up in
//!   reverse. What it pressed itself is what it lets go: never
//!   `pasting/modifiers.rs`' plan, which would let the model's own held
//!   Ctrl go as "stuck".
//! - **Typing** is Unicode keystrokes in chunks (any text, any layout).
//! - **A stop** (Esc, the person's own input, ■) is checked between steps:
//!   whatever is down is let go, and nothing more is sent.

use std::time::Duration;

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

/// One key, by name ("ctrl", "a", "f5", "return"): the stand-in desktop
/// keeps names; this PC turns them into virtual keys ([`vk_of`]).
pub type Key = String;

/// **One low-level step.**
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// The pointer to a physical point on the virtual desktop.
    Move(i32, i32),
    Down(Button),
    Up(Button),
    /// The wheel: `ticks` notches, positive up (or right, `across`).
    Wheel { ticks: i32, across: bool },
    KeyDown(Key),
    KeyUp(Key),
    /// Text as Unicode keystrokes.
    Text(String),
    /// A pause (a held key, a double click's gap, `wait`).
    Pause(Duration),
}

/// Modifier names a click or a scroll may hold, as the contract spells them.
const MODIFIERS: &[&str] = &["ctrl", "control", "shift", "alt", "win", "super", "cmd", "meta", "windows"];

/// **A combo's keys** ("ctrl+shift+A", "Return", "alt+F4"): lower case,
/// each once, `+` between them (a lone "+" is the plus key). Pure.
pub fn keys_of(combo: &str) -> Vec<Key> {
    let s = combo.trim();
    if s == "+" {
        return vec!["plus".into()];
    }
    let mut out: Vec<Key> = Vec::new();
    for part in s.split('+') {
        let k = normalise(part);
        if !k.is_empty() && !out.contains(&k) {
            out.push(k);
        }
    }
    // "ctrl++": the plus key after the modifiers.
    if s.ends_with("++") {
        out.push("plus".into());
    }
    out
}

/// One key's name as Yap knows it. Pure.
fn normalise(name: &str) -> Key {
    let n = name.trim().to_lowercase();
    match n.as_str() {
        "control" | "ctl" => "ctrl",
        "super" | "cmd" | "command" | "meta" | "windows" | "lwin" | "rwin" | "os" => "win",
        "option" | "menu" => "alt",
        "enter" | "kp_enter" => "return",
        "esc" => "escape",
        "del" => "delete",
        "ins" => "insert",
        "bksp" | "back_space" | "backspace" => "backspace",
        "pgup" | "page_up" | "prior" => "pageup",
        "pgdn" | "page_down" | "next" => "pagedown",
        "arrowup" | "up_arrow" => "up",
        "arrowdown" | "down_arrow" => "down",
        "arrowleft" | "left_arrow" => "left",
        "arrowright" | "right_arrow" => "right",
        "spacebar" => "space",
        "caps_lock" => "capslock",
        "print" | "prtsc" | "print_screen" => "printscreen",
        other => return other.to_string(),
    }
    .to_string()
}

/// Whether `k` is a modifier. Pure.
pub fn is_modifier(k: &str) -> bool {
    matches!(k, "ctrl" | "shift" | "alt" | "win")
}

/// **A system combo** — one the `systemKeyCombos` grant must allow: the
/// Windows key in any combo or alone, Alt+F4 (quit), Alt+Tab and Alt+Esc
/// (switch), Ctrl+Esc (Start), Ctrl+Shift+Esc (Task Manager). Pure.
pub fn system_combo(keys: &[Key]) -> bool {
    let has = |k: &str| keys.iter().any(|x| x == k);
    has("win") || (has("alt") && (has("f4") || has("tab") || has("escape"))) || (has("ctrl") && has("escape"))
}

/// **Holds `mods` round `body`**: down in order, `body`, up in reverse. Pure.
fn holding(mods: &[Key], body: Vec<Step>) -> Vec<Step> {
    let mut out: Vec<Step> = mods.iter().cloned().map(Step::KeyDown).collect();
    out.extend(body);
    out.extend(mods.iter().rev().cloned().map(Step::KeyUp));
    out
}

/// The modifiers a click's or a scroll's `text` names (others ignored). Pure.
pub fn modifiers_of(text: &str) -> Vec<Key> {
    keys_of(text).into_iter().filter(|k| MODIFIERS.contains(&k.as_str()) || is_modifier(k)).collect()
}

/// A gap between a double click's clicks (well under Windows' double-click time).
const CLICK_GAP: Duration = Duration::from_millis(40);
/// A drag's moves between its ends.
const DRAG_STEPS: i32 = 8;
/// How long a typed chunk is (Unicode keystrokes are two events a character).
pub const TEXT_CHUNK: usize = 200;

/// **What one action does, as steps.** `at`: its point (physical); `from`:
/// a drag's start; `button`/`clicks` for the clicks; `mods` held round a
/// click or a scroll. Pure.
pub fn click(at: (i32, i32), button: Button, clicks: u32, mods: &[Key]) -> Vec<Step> {
    let mut body = vec![Step::Move(at.0, at.1)];
    for i in 0..clicks.max(1) {
        if i > 0 {
            body.push(Step::Pause(CLICK_GAP));
        }
        body.push(Step::Down(button));
        body.push(Step::Up(button));
    }
    // The pointer moves first, then the modifiers go down: a hover state isn't held-key's.
    let first = body.remove(0);
    let mut out = vec![first];
    out.extend(holding(mods, body));
    out
}

/// A drag from `from` to `to`, the left button held, through
/// [`DRAG_STEPS`] moves (an app that waits for movement sees some). Pure.
pub fn drag(from: (i32, i32), to: (i32, i32), mods: &[Key]) -> Vec<Step> {
    let mut body = vec![Step::Down(Button::Left)];
    for i in 1..=DRAG_STEPS {
        let x = from.0 + (to.0 - from.0) * i / DRAG_STEPS;
        let y = from.1 + (to.1 - from.1) * i / DRAG_STEPS;
        body.push(Step::Pause(Duration::from_millis(12)));
        body.push(Step::Move(x, y));
    }
    body.push(Step::Up(Button::Left));
    let mut out = vec![Step::Move(from.0, from.1)];
    out.extend(holding(mods, body));
    out
}

/// A scroll of `ticks` notches at `at`, `direction` "up" | "down" | "left" |
/// "right". Pure.
pub fn scroll(at: (i32, i32), direction: &str, ticks: u32, mods: &[Key]) -> Vec<Step> {
    let n = ticks.min(100) as i32;
    let (ticks, across) = match direction {
        "down" => (-n, false),
        "left" => (-n, true),
        "right" => (n, true),
        _ => (n, false),
    };
    let mut out = vec![Step::Move(at.0, at.1)];
    out.extend(holding(mods, vec![Step::Wheel { ticks, across }]));
    out
}

/// A combo pressed `repeat` times. Pure.
pub fn key(keys: &[Key], repeat: u32) -> Vec<Step> {
    let mut out = Vec::new();
    for _ in 0..repeat.clamp(1, 100) {
        out.extend(keys.iter().cloned().map(Step::KeyDown));
        out.extend(keys.iter().rev().cloned().map(Step::KeyUp));
    }
    out
}

/// A combo held `secs` seconds. Pure.
pub fn hold(keys: &[Key], secs: f64) -> Vec<Step> {
    let mut out: Vec<Step> = keys.iter().cloned().map(Step::KeyDown).collect();
    out.push(Step::Pause(Duration::from_millis((secs.clamp(0.0, 100.0) * 1000.0) as u64)));
    out.extend(keys.iter().rev().cloned().map(Step::KeyUp));
    out
}

/// Text in chunks of [`TEXT_CHUNK`] characters. Pure.
pub fn text(s: &str) -> Vec<Step> {
    let chars: Vec<char> = s.chars().collect();
    chars.chunks(TEXT_CHUNK).map(|c| Step::Text(c.iter().collect())).collect()
}

/// **What's down after `done` steps** of a sequence: the keys and buttons
/// to let go, last pressed first, if it stops there. Pure.
pub fn left_down(done: &[Step]) -> Vec<Step> {
    let mut down: Vec<Step> = Vec::new();
    for s in done {
        match s {
            Step::KeyDown(k) => down.push(Step::KeyUp(k.clone())),
            Step::KeyUp(k) => down.retain(|d| d != &Step::KeyUp(k.clone())),
            Step::Down(b) => down.push(Step::Up(*b)),
            Step::Up(b) => down.retain(|d| d != &Step::Up(*b)),
            _ => {}
        }
    }
    down.reverse();
    down
}

/// **Where steps go**: this PC, or the stand-in desktop — and the rest of
/// what changes things there (phase 3): the clipboard, an app opened.
pub trait Hands: Send + Sync {
    /// One step (blocking): an error when Windows took none of it.
    fn send(&self, step: &Step) -> Result<(), String>;
    /// The clipboard's text (`None`: nothing on it as text). Blocking.
    fn clipboard(&self) -> Result<Option<String>, String> {
        Err("the clipboard isn't reachable here".into())
    }
    /// The clipboard set to `text` (checked it took). Blocking.
    fn set_clipboard(&self, _text: &str) -> Result<(), String> {
        Err("the clipboard isn't reachable here".into())
    }
    /// **An allowed app to the front** (`g`; `app`: its entry in the Start
    /// menu's index): a window of it raised, else the app started. Blocking.
    fn open(&self, _g: &super::grants::Granted, _app: Option<&crate::apps::App>) -> Result<super::open::Opened, String> {
        Err("apps can't be opened here".into())
    }
    /// **A window back in front** (after the person answered a card in
    /// Yap's own window): whether it is. Blocking.
    fn raise(&self, _hwnd: isize) -> bool {
        false
    }
}

/// **Sends `steps`**, checking `stop` before each: stopped, whatever is
/// down is let go and the reason comes back (`Err(None)`: stopped;
/// `Err(Some(e))`: a step failed). Blocking.
pub fn run(hands: &dyn Hands, steps: &[Step], stop: &dyn Fn() -> bool) -> Result<(), Option<String>> {
    for (i, s) in steps.iter().enumerate() {
        if stop() {
            for up in left_down(&steps[..i]) {
                let _ = hands.send(&up);
            }
            return Err(None);
        }
        if let Step::Pause(d) = s {
            // A long pause (a held key, `wait`) still hears a stop.
            let until = std::time::Instant::now() + *d;
            while std::time::Instant::now() < until {
                if stop() {
                    for up in left_down(&steps[..i]) {
                        let _ = hands.send(&up);
                    }
                    return Err(None);
                }
                std::thread::sleep(Duration::from_millis(15).min(until.saturating_duration_since(std::time::Instant::now())));
            }
            continue;
        }
        if let Err(e) = hands.send(s) {
            for up in left_down(&steps[..i]) {
                let _ = hands.send(&up);
            }
            return Err(Some(e));
        }
        // A beat between events, as hands have: apps that read the queue lazily miss none.
        std::thread::sleep(Duration::from_millis(4));
    }
    Ok(())
}

/// **A key's virtual key** and whether it's extended, on Windows (`None`:
/// a name Yap doesn't know). Single characters go through the keyboard
/// layout (`VkKeyScanW`) on this PC; this table is the rest. Pure.
pub fn vk_of(k: &str) -> Option<(u16, bool)> {
    let named = match k {
        "ctrl" => (0x11, false),
        "shift" => (0x10, false),
        "alt" => (0x12, false),
        "win" => (0x5B, true),
        "return" => (0x0D, false),
        "tab" => (0x09, false),
        "escape" => (0x1B, false),
        "space" => (0x20, false),
        "backspace" => (0x08, false),
        "delete" => (0x2E, true),
        "insert" => (0x2D, true),
        "home" => (0x24, true),
        "end" => (0x23, true),
        "pageup" => (0x21, true),
        "pagedown" => (0x22, true),
        "left" => (0x25, true),
        "up" => (0x26, true),
        "right" => (0x27, true),
        "down" => (0x28, true),
        "capslock" => (0x14, false),
        "printscreen" => (0x2C, true),
        "plus" => (0xBB, false),
        "minus" => (0xBD, false),
        "comma" => (0xBC, false),
        "period" => (0xBE, false),
        "apps" | "contextmenu" => (0x5D, true),
        _ => {
            if let Some(n) = k.strip_prefix('f').and_then(|n| n.parse::<u16>().ok()).filter(|n| (1..=24).contains(n)) {
                return Some((0x70 + n - 1, false));
            }
            let mut cs = k.chars();
            return match (cs.next(), cs.next()) {
                (Some(c), None) if c.is_ascii_alphanumeric() => Some((c.to_ascii_uppercase() as u16, false)),
                _ => None,
            };
        }
    };
    Some(named)
}

/// **This PC's hands**: `SendInput`, every event marked as Yap's own.
#[cfg(windows)]
pub struct ThisPc;

#[cfg(windows)]
impl Hands for ThisPc {
    fn send(&self, step: &Step) -> Result<(), String> {
        win::send(step)
    }

    fn clipboard(&self) -> Result<Option<String>, String> {
        Ok(crate::pasting::text_injector::clip::read_text())
    }

    fn set_clipboard(&self, text: &str) -> Result<(), String> {
        if crate::pasting::text_injector::clip::put_text_verified(text, false) {
            Ok(())
        } else {
            Err("the clipboard stayed busy".into())
        }
    }

    fn open(&self, g: &super::grants::Granted, app: Option<&crate::apps::App>) -> Result<super::open::Opened, String> {
        super::open::on_this_pc(g, app)
    }

    fn raise(&self, hwnd: isize) -> bool {
        super::windows::raise(hwnd)
    }
}

#[cfg(windows)]
mod win {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        MapVirtualKeyW, SendInput, VkKeyScanW, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MAPVK_VK_TO_VSC, MOUSEEVENTF_ABSOLUTE,
        MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL,
        MOUSEINPUT, MOUSE_EVENT_FLAGS, VIRTUAL_KEY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN};

    use super::{vk_of, Button, Step};

    const MARK: usize = crate::dictation::input_hook::OWN_MARK;

    fn mouse(flags: MOUSE_EVENT_FLAGS, dx: i32, dy: i32, data: i32) -> INPUT {
        INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi: MOUSEINPUT { dx, dy, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: MARK } } }
    }

    fn keyboard(vk: u16, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
        INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(vk), wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: MARK } } }
    }

    fn checked(inputs: &[INPUT], what: &str) -> Result<(), String> {
        let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
        if sent as usize == inputs.len() {
            Ok(())
        } else {
            Err(format!("Windows took {sent} of {} {what} events", inputs.len()))
        }
    }

    /// A physical point → `SendInput`'s 0–65535 across the virtual desktop.
    fn absolute(x: i32, y: i32) -> (i32, i32) {
        let (vx, vy, vw, vh) = unsafe { (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN), GetSystemMetrics(SM_CXVIRTUALSCREEN).max(2), GetSystemMetrics(SM_CYVIRTUALSCREEN).max(2)) };
        let nx = (((x - vx) as i64 * 65535) / (vw as i64 - 1)) as i32;
        let ny = (((y - vy) as i64 * 65535) / (vh as i64 - 1)) as i32;
        (nx.clamp(0, 65535), ny.clamp(0, 65535))
    }

    /// A key's virtual key, scan code and extended flag (a character through the layout).
    fn key_of(name: &str) -> Option<(u16, u16, bool)> {
        let (vk, ext) = match vk_of(name) {
            Some(v) => v,
            None => {
                let mut cs = name.chars();
                let c = cs.next()?;
                if cs.next().is_some() {
                    return None;
                }
                let r = unsafe { VkKeyScanW(c as u16) };
                if r == -1 {
                    return None;
                }
                ((r as u16) & 0xFF, false)
            }
        };
        let scan = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
        Some((vk, scan, ext))
    }

    pub fn send(step: &Step) -> Result<(), String> {
        match step {
            Step::Move(x, y) => {
                let (nx, ny) = absolute(*x, *y);
                checked(&[mouse(MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK, nx, ny, 0)], "move")
            }
            Step::Down(b) | Step::Up(b) => {
                let down = matches!(step, Step::Down(_));
                let flags = match (b, down) {
                    (Button::Left, true) => MOUSEEVENTF_LEFTDOWN,
                    (Button::Left, false) => MOUSEEVENTF_LEFTUP,
                    (Button::Right, true) => MOUSEEVENTF_RIGHTDOWN,
                    (Button::Right, false) => MOUSEEVENTF_RIGHTUP,
                    (Button::Middle, true) => MOUSEEVENTF_MIDDLEDOWN,
                    (Button::Middle, false) => MOUSEEVENTF_MIDDLEUP,
                };
                checked(&[mouse(flags, 0, 0, 0)], "button")
            }
            Step::Wheel { ticks, across } => checked(&[mouse(if *across { MOUSEEVENTF_HWHEEL } else { MOUSEEVENTF_WHEEL }, 0, 0, ticks * 120)], "wheel"),
            Step::KeyDown(k) | Step::KeyUp(k) => {
                let (vk, scan, ext) = key_of(k).ok_or_else(|| format!("Yap doesn't know the key \"{k}\""))?;
                let mut flags = if ext { KEYEVENTF_EXTENDEDKEY } else { KEYBD_EVENT_FLAGS(0) };
                if matches!(step, Step::KeyUp(_)) {
                    flags |= KEYEVENTF_KEYUP;
                }
                checked(&[keyboard(vk, scan, flags)], "key")
            }
            Step::Text(t) => {
                let mut inputs = Vec::with_capacity(t.len() * 4);
                for unit in t.replace("\r\n", "\n").encode_utf16() {
                    // A new line as Enter: a text field's own Unicode newline is ignored by many apps.
                    if unit == '\n' as u16 {
                        inputs.push(keyboard(0x0D, 0x1C, KEYBD_EVENT_FLAGS(0)));
                        inputs.push(keyboard(0x0D, 0x1C, KEYEVENTF_KEYUP));
                        continue;
                    }
                    inputs.push(keyboard(0, unit, KEYEVENTF_UNICODE));
                    inputs.push(keyboard(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
                }
                checked(&inputs, "typing")
            }
            Step::Pause(_) => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combos_are_their_keys_in_order() {
        assert_eq!(keys_of("ctrl+shift+A"), ["ctrl", "shift", "a"]);
        assert_eq!(keys_of("Return"), ["return"]);
        assert_eq!(keys_of("cmd+c"), ["win", "c"], "a Mac's command is Windows' key");
        assert_eq!(keys_of("Control+Enter"), ["ctrl", "return"]);
        assert_eq!(keys_of("+"), ["plus"]);
        assert_eq!(keys_of("ctrl++"), ["ctrl", "plus"]);
        assert_eq!(keys_of("Page_Down"), ["pagedown"]);
        assert!(system_combo(&keys_of("alt+F4")) && system_combo(&keys_of("alt+tab")) && system_combo(&keys_of("win+l")) && system_combo(&keys_of("super")));
        assert!(system_combo(&keys_of("ctrl+shift+esc")) && system_combo(&keys_of("ctrl+escape")));
        assert!(!system_combo(&keys_of("ctrl+a")) && !system_combo(&keys_of("alt+f")) && !system_combo(&keys_of("shift+tab")));
        assert_eq!(modifiers_of("ctrl+shift"), ["ctrl", "shift"]);
        assert_eq!(modifiers_of("x"), Vec::<Key>::new());
        assert_eq!(vk_of("f5"), Some((0x74, false)));
        assert_eq!(vk_of("delete"), Some((0x2E, true)));
        assert_eq!(vk_of("a"), Some((0x41, false)));
        assert_eq!(vk_of("7"), Some((0x37, false)));
        assert_eq!(vk_of("f25"), None);
        assert_eq!(vk_of("nonsense"), None);
    }

    #[test]
    fn a_click_moves_first_then_holds_its_modifiers_round_the_button() {
        assert_eq!(
            click((10, 20), Button::Left, 1, &["shift".into()]),
            vec![Step::Move(10, 20), Step::KeyDown("shift".into()), Step::Down(Button::Left), Step::Up(Button::Left), Step::KeyUp("shift".into())]
        );
        let triple = click((1, 1), Button::Left, 3, &[]);
        assert_eq!(triple.iter().filter(|s| **s == Step::Down(Button::Left)).count(), 3);
        let d = drag((0, 0), (80, 40), &[]);
        assert_eq!(d.first(), Some(&Step::Move(0, 0)));
        assert_eq!(d.iter().rev().nth(1), Some(&Step::Move(80, 40)));
        assert_eq!(d.last(), Some(&Step::Up(Button::Left)));
        assert_eq!(scroll((5, 5), "down", 3, &[]), vec![Step::Move(5, 5), Step::Wheel { ticks: -3, across: false }]);
        assert_eq!(scroll((5, 5), "right", 500, &[])[1], Step::Wheel { ticks: 100, across: true });
        assert_eq!(key(&keys_of("ctrl+s"), 2).len(), 8);
        assert_eq!(text(&"x".repeat(450)).len(), 3);
    }

    /// Stopped half-way: what's down is let go, last first, and nothing more goes.
    #[test]
    fn a_stop_lets_go_of_what_it_pressed_and_sends_nothing_more() {
        struct Log(std::sync::Mutex<Vec<Step>>);
        impl Hands for Log {
            fn send(&self, s: &Step) -> Result<(), String> {
                self.0.lock().unwrap().push(s.clone());
                Ok(())
            }
        }
        let log = Log(std::sync::Mutex::new(Vec::new()));
        let steps = holding(&["ctrl".into(), "shift".into()], vec![Step::Down(Button::Left), Step::Move(5, 5), Step::Up(Button::Left)]);
        let count = std::sync::atomic::AtomicUsize::new(0);
        let r = run(&log, &steps, &|| count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) >= 3);
        assert_eq!(r, Err(None));
        let sent = log.0.lock().unwrap().clone();
        assert_eq!(sent, vec![Step::KeyDown("ctrl".into()), Step::KeyDown("shift".into()), Step::Down(Button::Left), Step::Up(Button::Left), Step::KeyUp("shift".into()), Step::KeyUp("ctrl".into())]);
        assert_eq!(left_down(&[Step::KeyDown("a".into()), Step::KeyUp("a".into())]), vec![]);
    }
}

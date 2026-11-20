//! **The input tools** (phase 2): the desktop contract's clicks, pointer,
//! scroll, keys, typing and `wait`, each run the same way:
//!
//! 1. its coordinates read in the last screenshot's frame (none yet: take
//!    one first) and mapped to the display's own pixels;
//! 2. the PC taken for this chat (`watch::begin`: one chat at a time, never
//!    after the person took over this turn);
//! 3. what's there read — the window in front, the one under the point, the
//!    element under it, the focus — and checked (`checks::decide`): refused,
//!    asked first on the card (`act/confirm.rs`, outside Bypass), or on;
//! 4. the indicator up ("Yap is using your PC · Esc to stop"), the steps
//!    sent (`input::run`) with the person's input and ■ Stop checked between
//!    them;
//! 5. stopped by the person: what was held let go, the rest of the turn's
//!    input refused, the indicator off at once.
//!
//! The result is a line for the model and a chip with the action's
//! `action_summary` (the desktop contract's: what it does, never why, never
//! a secret); a screenshot after it is the model's to take.

use serde_json::{json, Value};

use super::checks::{self, Act, Facts, Target, Verdict};
use super::input::{self, Button};
use super::{failed, grants, shot, Ctx, Run};

pub const LEFT_CLICK: &str = "left_click";
pub const RIGHT_CLICK: &str = "right_click";
pub const MIDDLE_CLICK: &str = "middle_click";
pub const DOUBLE_CLICK: &str = "double_click";
pub const TRIPLE_CLICK: &str = "triple_click";
pub const MOUSE_MOVE: &str = "mouse_move";
pub const DRAG: &str = "left_click_drag";
pub const MOUSE_DOWN: &str = "left_mouse_down";
pub const MOUSE_UP: &str = "left_mouse_up";
pub const SCROLL: &str = "scroll";
pub const TYPE: &str = "type";
pub const KEY: &str = "key";
pub const HOLD_KEY: &str = "hold_key";
pub const WAIT: &str = "wait";

/// Every input tool, in the order they're listed.
pub const NAMES: &[&str] = &[LEFT_CLICK, RIGHT_CLICK, MIDDLE_CLICK, DOUBLE_CLICK, TRIPLE_CLICK, MOUSE_MOVE, DRAG, MOUSE_DOWN, MOUSE_UP, SCROLL, TYPE, KEY, HOLD_KEY, WAIT];

/// The most a typed text may be, in characters.
pub const TYPE_MAX: usize = 5_000;

const SUMMARY: &str = "A few words saying what this action does and to what, for example 'Opens the Filters menu' or 'Types the note into Notepad'. Set it on every call. The effect only: no reasons, never a password or another secret.";
const COORDINATE: &str = "[x, y] in the last screenshot's pixels.";

fn point() -> Value {
    json!({ "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2, "description": COORDINATE })
}

/// The input tools' definitions, as the tool loop takes them. Pure.
pub fn definitions() -> Vec<Value> {
    let tool = |name: &str, description: &str, properties: Value, required: &[&str]| {
        json!({ "type": "function", "function": { "name": name, "description": description, "parameters": { "type": "object", "properties": properties, "required": required, "additionalProperties": false } } })
    };
    let click = |name: &str, what: &str| {
        tool(
            name,
            &format!("{what} at a point of the last screenshot. The window there must be one of the allowed apps'."),
            json!({ "coordinate": point(), "text": { "type": "string", "description": "Modifier keys to hold during the click: \"shift\", \"ctrl+shift\"…" }, "action_summary": { "type": "string", "description": SUMMARY } }),
            &["coordinate"],
        )
    };
    vec![
        click(LEFT_CLICK, "Left-click"),
        click(RIGHT_CLICK, "Right-click (opens a context menu in most apps)"),
        click(MIDDLE_CLICK, "Middle-click (the wheel's click)"),
        click(DOUBLE_CLICK, "Double-click (selects a word in most text)"),
        click(TRIPLE_CLICK, "Triple-click (selects a line in most text)"),
        tool(MOUSE_MOVE, "Move the pointer without clicking (for hover states).", json!({ "coordinate": point() }), &["coordinate"]),
        tool(
            DRAG,
            "Press the left button, move to a point and let go. From start_coordinate, else from where the pointer is.",
            json!({ "coordinate": point(), "start_coordinate": point(), "action_summary": { "type": "string", "description": SUMMARY } }),
            &["coordinate"],
        ),
        tool(MOUSE_DOWN, "Press the left button where the pointer is and keep it held (mouse_move first; left_mouse_up lets go).", json!({ "action_summary": { "type": "string", "description": SUMMARY } }), &[]),
        tool(MOUSE_UP, "Let the left button go where the pointer is.", json!({ "action_summary": { "type": "string", "description": SUMMARY } }), &[]),
        tool(
            SCROLL,
            "Scroll at a point of the last screenshot.",
            json!({
                "coordinate": point(),
                "scroll_direction": { "type": "string", "enum": ["up", "down", "left", "right"] },
                "scroll_amount": { "type": "number", "description": "Wheel notches, 0 to 100." },
                "text": { "type": "string", "description": "Modifier keys to hold while scrolling." }
            }),
            &["coordinate", "scroll_direction", "scroll_amount"],
        ),
        tool(TYPE, "Type text into whatever has the keyboard focus in the app in front. New lines are Enter.", json!({ "text": { "type": "string" }, "action_summary": { "type": "string", "description": SUMMARY } }), &["text"]),
        tool(
            KEY,
            "Press a key or a combo (\"return\", \"ctrl+s\", \"ctrl+shift+tab\"). System combos (switching or closing apps, the Windows key, locking the screen) need the systemKeyCombos grant.",
            json!({ "text": { "type": "string", "description": "Keys joined with +." }, "repeat": { "type": "number", "description": "1 to 100 (default 1)." }, "action_summary": { "type": "string", "description": SUMMARY } }),
            &["text"],
        ),
        tool(
            HOLD_KEY,
            "Hold a key or a combo down for a while, then let go.",
            json!({ "text": { "type": "string" }, "duration": { "type": "number", "description": "Seconds, 0 to 100." }, "action_summary": { "type": "string", "description": SUMMARY } }),
            &["text", "duration"],
        ),
        tool(WAIT, "Wait a while (for an app to load or animate).", json!({ "duration": { "type": "number", "description": "Seconds, 0 to 100." } }), &["duration"]),
    ]
}

/// A point the model gave, `[x, y]`.
fn xy(v: &Value) -> Option<(f64, f64)> {
    let a = v.as_array()?;
    (a.len() == 2).then(|| Some((a[0].as_f64()?, a[1].as_f64()?))).flatten()
}

/// What one call does: its checks' act, its steps' builder.
struct Plan {
    act: Act,
    /// The point it acts at (physical), for the window under it.
    at: Option<(i32, i32)>,
    /// A drag's end (physical).
    end: Option<(i32, i32)>,
    steps: Vec<input::Step>,
    /// Typed text (for the checks).
    text: String,
    chip: String,
}

fn summary_or(args: &Value, otherwise: &str) -> String {
    let s: String = args["action_summary"].as_str().unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
    if s.is_empty() {
        otherwise.to_string()
    } else {
        s.chars().take(100).collect()
    }
}

/// **The call read**: its act, where, its steps. `frame`: the last
/// screenshot's (only a call with coordinates needs one: keys and typing
/// don't, as in the desktop app); `cursor`: the pointer now (physical). Pure.
fn plan(name: &str, args: &Value, frame: Option<&super::frame::Frame>, cursor: Option<(i32, i32)>) -> Result<Plan, (String, String)> {
    let map = |key: &str| -> Result<Option<(i32, i32)>, (String, String)> {
        match &args[key] {
            Value::Null => Ok(None),
            v => {
                let Some(frame) = frame else {
                    return Err(("Take a screenshot first: coordinates are its pixels.".to_string(), "Didn't act: no screenshot yet".to_string()));
                };
                let (x, y) = xy(v).ok_or_else(|| (format!("{key} must be [x, y]."), "Didn't act: no point given".to_string()))?;
                frame.on_screen(x, y).map(Some).ok_or_else(|| {
                    (format!("[{x}, {y}] isn't on the last screenshot ({}×{}): coordinates are its pixels.", frame.w, frame.h), "Didn't act: that point isn't on the screenshot".to_string())
                })
            }
        }
    };
    let mods = input::modifiers_of(args["text"].as_str().unwrap_or(""));
    let need = |p: Option<(i32, i32)>| p.ok_or_else(|| ("coordinate is needed: [x, y] in the last screenshot's pixels.".to_string(), "Didn't act: no point given".to_string()));
    let clicks = |button: Button, n: u32, what: &str| -> Result<Plan, (String, String)> {
        let at = need(map("coordinate")?)?;
        Ok(Plan { act: Act::Click { button, mods: !mods.is_empty() }, at: Some(at), end: None, steps: input::click(at, button, n, &mods), text: String::new(), chip: summary_or(args, what) })
    };
    match name {
        LEFT_CLICK => clicks(Button::Left, 1, "Clicked"),
        RIGHT_CLICK => clicks(Button::Right, 1, "Right-clicked"),
        MIDDLE_CLICK => clicks(Button::Middle, 1, "Middle-clicked"),
        DOUBLE_CLICK => clicks(Button::Left, 2, "Double-clicked"),
        TRIPLE_CLICK => clicks(Button::Left, 3, "Triple-clicked"),
        MOUSE_MOVE => {
            let at = need(map("coordinate")?)?;
            Ok(Plan { act: Act::Move, at: Some(at), end: None, steps: vec![input::Step::Move(at.0, at.1)], text: String::new(), chip: "Moved the pointer".into() })
        }
        DRAG => {
            let to = need(map("coordinate")?)?;
            let from = map("start_coordinate")?.or(cursor).ok_or_else(|| ("start_coordinate is needed (the pointer's place isn't known).".to_string(), "Didn't drag: no start".to_string()))?;
            Ok(Plan { act: Act::Drag { mods: !mods.is_empty() }, at: Some(from), end: Some(to), steps: input::drag(from, to, &mods), text: String::new(), chip: summary_or(args, "Dragged") })
        }
        MOUSE_DOWN | MOUSE_UP => {
            let at = cursor.ok_or_else(|| ("The pointer's place isn't known: mouse_move first.".to_string(), "Didn't act: no pointer".to_string()))?;
            let step = if name == MOUSE_DOWN { input::Step::Down(Button::Left) } else { input::Step::Up(Button::Left) };
            Ok(Plan { act: Act::Press, at: Some(at), end: None, steps: vec![step], text: String::new(), chip: summary_or(args, if name == MOUSE_DOWN { "Pressed the button" } else { "Let the button go" }) })
        }
        SCROLL => {
            let at = map("coordinate")?.or(cursor).ok_or_else(|| ("coordinate is needed.".to_string(), "Didn't scroll: no point".to_string()))?;
            let dir = args["scroll_direction"].as_str().unwrap_or("down");
            if !matches!(dir, "up" | "down" | "left" | "right") {
                return Err(("scroll_direction is up, down, left or right.".into(), "Didn't scroll".into()));
            }
            let n = args["scroll_amount"].as_f64().unwrap_or(3.0).clamp(0.0, 100.0) as u32;
            Ok(Plan { act: Act::Scroll { mods: !mods.is_empty() }, at: Some(at), end: None, steps: input::scroll(at, dir, n, &mods), text: String::new(), chip: format!("Scrolled {dir}") })
        }
        TYPE => {
            let t = args["text"].as_str().unwrap_or("");
            if t.is_empty() {
                return Err(("text is needed.".into(), "Didn't type: no text".into()));
            }
            if t.chars().count() > TYPE_MAX {
                return Err((format!("That's over {TYPE_MAX} characters: type it in parts."), "Didn't type: too long".into()));
            }
            Ok(Plan { act: Act::Type, at: None, end: None, steps: input::text(t), text: t.to_string(), chip: summary_or(args, "Typed") })
        }
        KEY | HOLD_KEY => {
            let combo = args["text"].as_str().unwrap_or("");
            let keys = input::keys_of(combo);
            if keys.is_empty() {
                return Err(("text is needed: a key or a combo like \"ctrl+s\".".into(), "Didn't press a key".into()));
            }
            if let Some(k) = keys.iter().find(|k| input::vk_of(k).is_none() && k.chars().count() != 1) {
                return Err((format!("Yap doesn't know the key \"{k}\"."), "Didn't press a key: unknown key".into()));
            }
            let system = input::system_combo(&keys);
            if name == HOLD_KEY {
                let secs = args["duration"].as_f64().unwrap_or(1.0);
                return Ok(Plan { act: Act::Hold { system }, at: None, end: None, steps: input::hold(&keys, secs), text: String::new(), chip: summary_or(args, &format!("Held {combo}")) });
            }
            let repeat = args["repeat"].as_f64().unwrap_or(1.0).clamp(1.0, 100.0) as u32;
            let enter = keys.last().is_some_and(|k| k == "return");
            Ok(Plan { act: Act::Key { system, enter }, at: None, end: None, steps: input::key(&keys, repeat), text: String::new(), chip: summary_or(args, &format!("Pressed {combo}")) })
        }
        _ => Err((format!("{name} isn't one of Yap's input tools."), format!("Unknown tool: {name}"))),
    }
}

/// A window as the checks see it: the allowed app it's of and its tier.
pub(super) fn target(w: Option<super::windows::Win>, apps: &[grants::Granted], desk: &dyn shot::Desk) -> Option<Target> {
    let w = w?;
    let app = (!w.own).then(|| apps.iter().find(|g| g.owns(&w)).map(|g| (g.name.clone(), g.tier))).flatten();
    let messaging = crate::pasting::app_kind::classify(Some(&w.exe), &w.class, "") == crate::pasting::app_kind::AppKind::Messaging;
    let elevated = app.is_some() && desk.elevated(&w);
    Some(Target { yap: w.own, app, elevated, messaging })
}

/// **One input call** for the chat `ctx` says.
pub async fn execute(name: &str, args: &Value, ctx: &Ctx<'_>) -> Run {
    if name == WAIT {
        let secs = args["duration"].as_f64().unwrap_or(1.0).clamp(0.0, 100.0);
        tokio::time::sleep(std::time::Duration::from_millis((secs * 1000.0) as u64)).await;
        return json!({ "success": true, "data": { "waited": secs }, "displayText": format!("Waited {secs:.0} s") }).into();
    }
    let (apps, last, system_keys) = grants::with(ctx.conversation, |s| (s.apps.clone(), s.last.clone(), s.system_keys));
    if apps.is_empty() {
        return failed("No apps are allowed in this chat yet: call request_access first.", "Didn't act: no apps allowed").into();
    }
    let desk = match shot::desk() {
        Ok(d) => d,
        Err(e) => return failed(&format!("Yap can't act here ({e})."), "Didn't act").into(),
    };
    let cursor = desk.cursor();
    let plan = match plan(name, args, last.as_ref().map(|l| &l.frame), cursor) {
        Ok(p) => p,
        Err((error, chip)) => return failed(&error, &chip).into(),
    };
    // The PC, for this chat.
    let driving = match super::watch::begin(ctx.conversation, ctx.turn) {
        Ok(d) => d,
        Err((error, chip)) => return failed(error, chip).into(),
    };
    // What's there now, and the checks.
    let facts_desk = desk;
    let (verdict, desk) = {
        let apps = apps.clone();
        let p = (plan.act, plan.at, plan.end, plan.text.clone());
        let unattended = ctx.unattended;
        let read = move || {
            let d = facts_desk;
            let (act, at, end, text) = p;
            let under = at.map(|(x, y)| d.window_at(x, y));
            let element = match (act, at) {
                (Act::Click { .. }, Some((x, y))) => d.element_at(x, y),
                _ => None,
            };
            let focus = if act == Act::Type { d.focused() } else { None };
            let front = d.front();
            // Robot mode: only the windows the test opened, whatever's allowed (shell/realtest.rs).
            if crate::shell::realtest::on() {
                let pointer = matches!(act, Act::Click { .. } | Act::Drag { .. } | Act::Press | Act::Scroll { .. });
                let landing = if pointer { under.clone().flatten() } else { front.clone() };
                if let crate::shell::realtest::Gate::Refused(why) = crate::shell::realtest::gate(landing.map(|w| w.hwnd), "use an app (computer use)") {
                    return (Verdict::Refuse { error: why, chip: "Didn't act: robot mode".into() }, d);
                }
            }
            let f = Facts {
                front: target(front, &apps, d.as_ref()),
                under: under.and_then(|w| target(w, &apps, d.as_ref())),
                end: end.and_then(|(x, y)| target(d.window_at(x, y), &apps, d.as_ref())),
                element,
                focus,
                system_keys,
                text: &text,
                unattended,
            };
            (checks::decide(act, &f), d)
        };
        match tauri::async_runtime::spawn_blocking(read).await {
            Ok(r) => r,
            Err(_) => return failed("Yap couldn't read what's on screen.", "Didn't act").into(),
        }
    };
    let mut asked = false;
    match verdict {
        Verdict::Go => {}
        Verdict::Refuse { error, chip } => return failed(&error, &chip).into(),
        Verdict::Ask { act, label, app, does } => {
            use crate::chat::pages::act::confirm::{ask, Answer, Ask};
            let answer = ask(ctx.live, Ask { act, label: &label, site: &app, does, ..Default::default() }).await;
            if !matches!(answer, Answer::Allowed | Answer::AlwaysAllowed) {
                let error = match answer {
                    Answer::TimedOut => "The user didn't answer in time, so Yap didn't do it. Don't try again unless they ask.",
                    Answer::NoOne => "Nobody could be asked about this, so Yap didn't do it. Ask the user in words.",
                    _ => "The user said no on the card. Don't try it again: say what you'd have done.",
                };
                return failed(error, &format!("Didn't {does}: not allowed")).into();
            }
            asked = true;
        }
    }
    drop(desk);
    // The steps, the indicator up; the person's input and ■ stop them.
    super::indicator::driving(&plan.chip, last.as_ref().map(|l| l.frame.display));
    let hands = match shot::hands() {
        Ok(h) => h,
        Err(e) => {
            super::indicator::done();
            return failed(&format!("Yap can't act here ({e})."), "Didn't act").into();
        }
    };
    let halted = driving.halted();
    let steps = plan.steps.clone();
    // The person's input, read between steps (the hook's atomics).
    let probe = driving.probe();
    let ran = tauri::async_runtime::spawn_blocking(move || {
        let stop = || probe.stopped() || halted.load(std::sync::atomic::Ordering::Relaxed);
        input::run(hands.as_ref(), &steps, &stop)
    })
    .await;
    if let Some(who) = driving.touched() {
        driving.stopped_by_person();
        super::indicator::off();
        tracing::info!(tool = name, "Computer use: the user took over");
        return failed(
            &format!("The user {} while Yap was working, so Yap stopped (nothing more was sent). Don't use their apps again until they ask: say what's left to do.", who.words()),
            &format!("Stopped: you {}", who.words()),
        )
        .into();
    }
    super::indicator::done();
    match ran {
        Ok(Ok(())) => {
            let note = if asked { "The user allowed it on Yap's card, and it's done. Take a screenshot to see what it did." } else { "Done. Take a screenshot to see what it did." };
            json!({ "success": true, "data": { "done": name, "askedFirst": asked }, "note": note, "displayText": plan.chip }).into()
        }
        Ok(Err(None)) => failed("Stopped before it finished (the answer was stopped).", "Stopped").into(),
        Ok(Err(Some(e))) => {
            tracing::info!(tool = name, "Computer use: Windows didn't take the input");
            failed(&format!("Windows didn't take it ({e}). Take a screenshot and try another way, or ask the user."), "Didn't act: Windows refused it").into()
        }
        Err(_) => failed("Yap couldn't send it.", "Didn't act").into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snip::geom::Rect;

    #[test]
    fn a_calls_point_is_read_in_the_screenshots_frame() {
        let frame = super::super::frame::Frame::fit(Rect::new(0, 0, 3840, 2160), 1920);
        let p = plan(LEFT_CLICK, &json!({ "coordinate": [100, 50], "action_summary": "Opens the File menu" }), Some(&frame), None).ok().unwrap();
        assert_eq!(p.at, Some((201, 101)));
        assert_eq!(p.chip, "Opens the File menu");
        assert_eq!(p.act, Act::Click { button: Button::Left, mods: false });
        assert!(plan(LEFT_CLICK, &json!({ "coordinate": [5000, 50] }), Some(&frame), None).is_err());
        assert!(plan(LEFT_CLICK, &json!({}), Some(&frame), None).is_err());
        let shift = plan(LEFT_CLICK, &json!({ "coordinate": [1, 1], "text": "shift" }), Some(&frame), None).ok().unwrap();
        assert_eq!(shift.act, Act::Click { button: Button::Left, mods: true });
        let drag = plan(DRAG, &json!({ "coordinate": [10, 10] }), Some(&frame), Some((4, 4))).ok().unwrap();
        assert_eq!((drag.at, drag.end), (Some((4, 4)), Some((21, 21))));
        assert!(plan(DRAG, &json!({ "coordinate": [10, 10] }), Some(&frame), None).is_err(), "no start, no pointer");
        let k = plan(KEY, &json!({ "text": "alt+F4" }), Some(&frame), None).ok().unwrap();
        assert_eq!(k.act, Act::Key { system: true, enter: false });
        let enter = plan(KEY, &json!({ "text": "ctrl+Return" }), Some(&frame), None).ok().unwrap();
        assert_eq!(enter.act, Act::Key { system: false, enter: true });
        assert!(plan(KEY, &json!({ "text": "" }), Some(&frame), None).is_err());
        assert!(plan(TYPE, &json!({ "text": "x".repeat(TYPE_MAX + 1) }), Some(&frame), None).is_err());
        assert_eq!(definitions().len(), NAMES.len());
        // Keys and typing need no screenshot; a point does.
        assert!(plan(KEY, &json!({ "text": "ctrl+s" }), None, None).is_ok());
        assert!(plan(TYPE, &json!({ "text": "hi" }), None, None).is_ok());
        assert!(plan(LEFT_CLICK, &json!({ "coordinate": [1, 1] }), None, None).err().is_some_and(|e| e.0.contains("screenshot first")));
    }

    /// Phase 2 whole on the stand-in desktop: a click focuses, typing fills,
    /// Send asks first (no: nothing pressed), a password and a link are
    /// refused, a browser is read only, a masked app can't be clicked, and
    /// the person taking over ends the turn's input.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn clicks_and_typing_land_only_where_allowed() {
        use crate::chat::pages::act::confirm;
        use crate::computer::sim::{self, Scene, SimDisplay, SimWindow, Widget};
        let _one = crate::computer::tests::LOCK.lock().unwrap_or_else(|p| p.into_inner());
        grants::forget_all();
        crate::computer::watch::reset();
        let w = |kind: &str, name: &str, x: i32, y: i32| Widget { kind: kind.into(), name: name.into(), x, y, w: 160, h: 30, ..Default::default() };
        sim::set(Some(Scene {
            displays: vec![SimDisplay { x: 0, y: 0, w: 1200, h: 800, primary: true }],
            windows: vec![
                SimWindow { x: 700, y: 500, w: 300, h: 200, exe: r"C:\Apps\other.exe".into(), title: "Other".into(), ..Default::default() },
                SimWindow { x: 650, y: 50, w: 400, h: 300, exe: r"C:\Program Files\Microsoft\Edge\msedge.exe".into(), title: "Browser".into(), ..Default::default() },
                SimWindow {
                    x: 50,
                    y: 50,
                    w: 560,
                    h: 400,
                    exe: r"C:\Apps\mail.exe".into(),
                    title: "Mail".into(),
                    widgets: vec![w("field", "Message", 20, 40), w("button", "Send", 20, 90), w("password", "PIN", 20, 140), w("link", "Track parcel", 20, 190)],
                    ..Default::default()
                },
            ],
            cursor: Some((5, 5)),
            ..Default::default()
        }));
        let mail = grants::Granted::of_exe(r"C:\Apps\mail.exe");
        let edge = grants::Granted::of_exe(r"C:\Program Files\Microsoft\Edge\msedge.exe");
        grants::with(Some(90), |s| s.grant(vec![mail, edge], false, false, false));
        let ctx = Ctx { conversation: Some(90), live: None, provider: "anthropic", base_url: "https://api.anthropic.com/v1", unattended: false, turn: "turn-1" };
        let shot = crate::computer::look(crate::computer::SCREENSHOT, &json!({}), &ctx).await;
        assert_eq!(shot.result["success"], true, "{}", shot.result);
        let run = |name: &'static str, args: Value| async move { execute(name, &args, &ctx).await.result };
        let ok = |v: &Value| v["success"] == true;
        // The frame is the display's own (1200 px fits): a point is a pixel.
        let first = run(LEFT_CLICK, json!({ "coordinate": [100, 105], "action_summary": "Clicks into the message" })).await;
        assert!(ok(&first), "{first}");
        assert_eq!(first["displayText"], "Clicks into the message");
        let typed = run(TYPE, json!({ "text": "Running late" })).await;
        assert!(ok(&typed), "{typed}");
        assert_eq!(typed["displayText"], "Typed");
        // Send asks first: no on the card, nothing pressed.
        confirm::answer_in_test(Some(false));
        let send = run(LEFT_CLICK, json!({ "coordinate": [100, 155] })).await;
        confirm::answer_in_test(None);
        assert!(!ok(&send) && send["error"].as_str().unwrap().contains("said no"), "{send}");
        // A password field: focused, then typing refused.
        assert!(ok(&run(LEFT_CLICK, json!({ "coordinate": [100, 205] })).await));
        assert!(run(TYPE, json!({ "text": "1234" })).await["error"].as_str().unwrap().contains("password"));
        // A link, the browser, the app not allowed.
        assert!(run(LEFT_CLICK, json!({ "coordinate": [100, 255] })).await["error"].as_str().unwrap().contains("links"));
        assert!(run(LEFT_CLICK, json!({ "coordinate": [800, 100] })).await["error"].as_str().unwrap().contains("read only"));
        assert!(run(LEFT_CLICK, json!({ "coordinate": [900, 600] })).await["error"].as_str().unwrap().contains("hasn't allowed"));
        let scene = sim::scene().unwrap();
        let mail = scene.windows.iter().find(|w| w.title == "Mail").unwrap();
        assert_eq!(mail.widgets[0].text, "Running late");
        assert_eq!(mail.widgets[2].text, "", "never typed into the PIN");
        assert!(!scene.log.iter().any(|l| l.contains("pressed Send") || l.contains("opened")), "{:?}", scene.log);
        // The person takes over: this turn's input is refused from then on.
        let begun = crate::computer::watch::begin(Some(90), "turn-1").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(3));
        crate::computer::watch::person_key(0x1B, true);
        assert!(begun.touched().is_some());
        begun.stopped_by_person();
        drop(begun);
        assert!(run(LEFT_CLICK, json!({ "coordinate": [100, 105] })).await["error"].as_str().unwrap().contains("took over"));
        sim::set(None);
        grants::forget_all();
        crate::computer::watch::reset();
    }
}

//! **Teach mode** (phase 4, the desktop contract's): Claude guides the
//! person through a task step by step with a tooltip on screen, instead of
//! doing it all itself.
//!
//! - **`request_teach_access`**: the access card as `request_access`'s (the
//!   same apps and tiers, no clipboard or system-key extras), worded as a
//!   guided tour. Allowed: teach mode is on for the chat, and Yap's main
//!   window steps aside (hidden, never closed) until the turn ends.
//! - **`teach_step`**: one tooltip ([`tooltip`]: a small native window near
//!   the step's anchor, a ring on the spot, kept out of every capture and
//!   never taking the focus) with the explanation, what Next will do, and
//!   **Next** / **Exit**. Next runs the step's actions as a batch
//!   (`batch.rs`: the same checks; coordinates from the screenshot before
//!   it) and answers with a fresh screenshot; Exit, Esc, or no answer in
//!   [`WAIT`] answers `{exited: true}`, and teach mode is over for the turn.
//! - **`teach_batch`**: several steps in one call, every one checked before
//!   any tooltip shows; anchors and coordinates all from the screenshot
//!   before the call.
//! - **Teach mode ends with the turn** (`chat/turn.rs` calls
//!   [`turn_ended`]): the tooltip goes, Yap's window comes back.
//!
//! While a tooltip waits, the watch is off (the person's click on Next is
//! theirs to make); while a step's actions run, a key, a button or the
//! wheel stops them as ever, but a small move of the mouse doesn't (the
//! person's hand is on it).
//!
//! Logs: counts and outcomes only, never the tooltip's words.

pub mod tooltip;

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};

use super::{batch, failed, grants, Ctx, Run, SCREENSHOT};
use crate::snip::geom::Rect;

pub const REQUEST_TEACH_ACCESS: &str = "request_teach_access";
pub const TEACH_STEP: &str = "teach_step";
pub const TEACH_BATCH: &str = "teach_batch";

/// How long a tooltip waits for Next before the tour ends.
pub const WAIT: Duration = Duration::from_secs(15 * 60);
/// The most steps one `teach_batch` holds.
pub const STEPS_MAX: usize = 20;
/// The longest explanation, in characters.
const EXPLANATION_MAX: usize = 1_200;

/// Its definitions, as the tool loop takes them. Pure.
pub fn definitions() -> Vec<Value> {
    let tool = |name: &str, description: &str, properties: Value, required: &[&str]| {
        json!({ "type": "function", "function": { "name": name, "description": description, "parameters": { "type": "object", "properties": properties, "required": required, "additionalProperties": false } } })
    };
    let step = json!({
        "explanation": { "type": "string", "description": "The tooltip's text: what the user is looking at and why it matters. The only place they see your words during the tour: complete but short." },
        "next_preview": { "type": "string", "description": "One line on exactly what happens when they press Next (\"Next: I'll click Save.\")." },
        "actions": { "type": "array", "items": batch::action_schema(), "description": "What runs when they press Next: the same actions as computer_batch (empty for a step that only explains). They stop at the first error." },
        "anchor": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2, "description": "[x, y] in the last screenshot's pixels: where the tooltip points. Leave it out to centre the tooltip." }
    });
    vec![
        tool(
            REQUEST_TEACH_ACCESS,
            "Ask to guide the user through a task step by step with tooltips on screen: use it INSTEAD of request_access when they want to learn how (\"teach me\", \"walk me through\", \"show me how\"). Name the apps as the Start menu does, and what you'll teach. The same card as request_access, without the clipboard or system-shortcut extras. Allowed, Yap's window steps aside; call teach_step (or teach_batch) next, after a screenshot to anchor the first step. Teach mode ends with your turn.",
            json!({
                "apps": { "type": "array", "items": { "type": "string" }, "description": "The apps, as the Start menu names them." },
                "reason": { "type": "string", "description": "What you'll teach: shown as \"Yap wants to guide you through …\"." }
            }),
            &["apps", "reason"],
        ),
        tool(
            TEACH_STEP,
            "Show one tooltip and wait for the user to press Next; then run the step's actions, take a fresh screenshot and return both (no separate screenshot needed). During the tour the user only sees the tooltip: put all your words in explanation. Pack each step's actions: one step that fills a form beats five that fill a field each. Returns exited: true when the user presses Exit: don't call it again then.",
            step.clone(),
            &["explanation", "next_preview", "actions"],
        ),
        tool(
            TEACH_BATCH,
            "Queue several teach steps in one call: each still shows its tooltip and waits for Next, without a round trip between them. Every step is checked before the first tooltip shows. Within a batch every anchor and click refers to the screenshot taken BEFORE the call: leave out later steps' anchors unless the spot won't have moved. Returns stepsCompleted and each step's results with a final screenshot; exited: true with stepsCompleted when the user presses Exit; stepFailed when an action fails.",
            json!({ "steps": { "type": "array", "items": { "type": "object", "properties": step, "required": ["explanation", "next_preview", "actions"] }, "description": "The steps, in order." } }),
            &["steps"],
        ),
    ]
}

/// **One step as the model gave it**, checked.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub explanation: String,
    pub next_preview: String,
    pub actions: Vec<(String, Value)>,
    /// In the screenshot's pixels.
    pub anchor: Option<(f64, f64)>,
}

/// **A step read and checked** (`at`: its place, for the error). Pure.
pub fn read_step(v: &Value, at: &str) -> Result<Step, String> {
    let explanation = v["explanation"].as_str().unwrap_or("").trim().to_string();
    if explanation.is_empty() {
        return Err(format!("{at} needs an explanation."));
    }
    let explanation: String = explanation.chars().take(EXPLANATION_MAX).collect();
    let next_preview = v["next_preview"].as_str().unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ");
    let actions = match &v["actions"] {
        Value::Array(a) if a.is_empty() => Vec::new(),
        Value::Null => Vec::new(),
        list => batch::read(list).map_err(|e| format!("{at}: {e}"))?,
    };
    let anchor = match &v["anchor"] {
        Value::Null => None,
        Value::Array(p) if p.len() == 2 => Some((p[0].as_f64().ok_or(format!("{at}: anchor is [x, y]."))?, p[1].as_f64().ok_or(format!("{at}: anchor is [x, y]."))?)),
        _ => return Err(format!("{at}: anchor is [x, y].")),
    };
    Ok(Step { explanation, next_preview, actions, anchor })
}

/// **What a tooltip shows**: the words, the step's place in the tour, where
/// it points (physical pixels; `None`: centred) and the display it's on.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tip {
    pub explanation: String,
    pub next_preview: String,
    /// 1-based; `of`: the steps in this call (1 for `teach_step`).
    pub step: usize,
    pub of: usize,
    pub anchor: Option<(i32, i32)>,
    pub display: Rect,
}

/// What the person pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pressed {
    Next,
    Exit,
}

/// **The tour on now**: its chat, whether Yap's main window stepped aside
/// for it (and so comes back), and whether the person left it.
#[derive(Debug, Clone, Default)]
struct Tour {
    conversation: Option<u64>,
    main_hidden: bool,
    exited: bool,
}

static TOUR: Mutex<Option<Tour>> = Mutex::new(None);
/// The tooltip waiting: its id and where its answer goes.
static WAITING: Mutex<Option<(u64, tokio::sync::oneshot::Sender<Pressed>)>> = Mutex::new(None);
/// The tooltip shown now (test runs read it: `e2e_computer_teach`).
static SHOWN: Mutex<Option<Tip>> = Mutex::new(None);
static TIPS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
/// A tooltip is up: the input hook's Esc answers it (an atomic, for the hook).
static TIP_UP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether a tooltip waits now (the input hook asks: atomics only).
pub fn tip_up() -> bool {
    TIP_UP.load(std::sync::atomic::Ordering::Relaxed)
}

fn tour() -> std::sync::MutexGuard<'static, Option<Tour>> {
    TOUR.lock().unwrap_or_else(|p| p.into_inner())
}

/// Whether a tour is on in `conversation`.
pub fn on(conversation: Option<u64>) -> bool {
    tour().as_ref().is_some_and(|t| t.conversation == conversation)
}

/// **The person pressed Next or Exit** (the tooltip's buttons, Esc, a
/// test): the waiting tooltip answered. Whether one waited.
pub fn press(pressed: Pressed) -> bool {
    let w = WAITING.lock().unwrap_or_else(|p| p.into_inner()).take();
    match w {
        Some((_, tx)) => tx.send(pressed).is_ok(),
        None => false,
    }
}

/// **A tour begins** in `conversation`: Yap's main window steps aside
/// (hidden, if it showed), the watch lets small moves of the mouse be.
fn begin(conversation: Option<u64>) {
    let main_hidden = step_aside();
    super::watch::lenient(true);
    *tour() = Some(Tour { conversation, main_hidden, exited: false });
    tracing::info!(main_hidden, "Computer use: a guided tour began");
}

/// A tour begun with no card (test and robot runs: `e2e_computer_do`).
#[cfg(debug_assertions)]
pub fn begin_for_test(conversation: Option<u64>) {
    begin(conversation);
}

/// **The turn ended** (`chat/turn.rs`, every Chat turn, a plan chat's and
/// the Voice Assistant's too): the tour on in `conversation` (`None`: any)
/// is over — its tooltip gone, Yap's window back.
pub fn turn_ended(conversation: Option<u64>) {
    let ended = {
        let mut t = tour();
        if t.as_ref().is_some_and(|t| conversation.is_none() || t.conversation == conversation) {
            t.take()
        } else {
            None
        }
    };
    let Some(ended) = ended else { return };
    press(Pressed::Exit);
    tooltip::hide();
    *SHOWN.lock().unwrap_or_else(|p| p.into_inner()) = None;
    super::watch::lenient(false);
    if ended.main_hidden {
        come_back();
    }
    tracing::info!(exited = ended.exited, "Computer use: a guided tour ended with its turn");
}

/// Yap's main window out of the way: hidden if it showed (a test run only
/// notes it). Whether it was.
fn step_aside() -> bool {
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() && !crate::shell::realtest::on() {
        return true;
    }
    let Some(app) = crate::chat::pages::view::app() else { return false };
    use tauri::Manager;
    match app.get_webview_window("settings") {
        Some(w) if w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false) => w.hide().is_ok(),
        _ => false,
    }
}

/// Yap's main window back, without taking the focus.
fn come_back() {
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() && !crate::shell::realtest::on() {
        return;
    }
    let Some(app) = crate::chat::pages::view::app() else { return };
    use tauri::Manager;
    if let Some(w) = app.get_webview_window("settings") {
        crate::shell::window::show_quietly(&w);
    }
}

/// **A tooltip shown, its answer awaited** (the turn ending, ■ Stop or
/// [`WAIT`] passing: Exit).
async fn show_and_wait(tip: Tip) -> Pressed {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let id = TIPS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    *WAITING.lock().unwrap_or_else(|p| p.into_inner()) = Some((id, tx));
    *SHOWN.lock().unwrap_or_else(|p| p.into_inner()) = Some(tip.clone());
    // The person's click on Next is theirs: no actions run while it waits.
    super::watch::disarm();
    TIP_UP.store(true, std::sync::atomic::Ordering::Relaxed);
    tooltip::show(&tip);
    let pressed = match tokio::time::timeout(WAIT, rx).await {
        Ok(Ok(p)) => p,
        _ => Pressed::Exit,
    };
    {
        let mut w = WAITING.lock().unwrap_or_else(|p| p.into_inner());
        if w.as_ref().is_some_and(|(i, _)| *i == id) {
            *w = None;
        }
    }
    TIP_UP.store(false, std::sync::atomic::Ordering::Relaxed);
    tooltip::hide();
    *SHOWN.lock().unwrap_or_else(|p| p.into_inner()) = None;
    pressed
}

/// The person left the tour: said once, the same way for a step and a batch.
fn exited(completed: Option<usize>) -> Run {
    if let Some(t) = tour().as_mut() {
        t.exited = true;
    }
    tracing::info!("Computer use: the person left the guided tour");
    let mut data = json!({ "exited": true });
    if let Some(n) = completed {
        data["stepsCompleted"] = json!(n);
    }
    json!({ "success": true, "data": data, "note": "The user pressed Exit: the tour is over. Don't call teach_step or teach_batch again; answer them in words.", "displayText": "You left the tour" }).into()
}

/// What refuses a teach call now: no tour, or the person left it. Pure-ish.
fn not_now(ctx: &Ctx<'_>) -> Option<Run> {
    let t = tour().clone();
    match t {
        Some(t) if t.conversation == ctx.conversation && t.exited => Some(exited(None)),
        Some(t) if t.conversation == ctx.conversation => None,
        _ => Some(failed("No guided tour is on in this chat: call request_teach_access first (then take a screenshot to anchor the first step).", "Didn't guide you: no tour on").into()),
    }
}

/// The tooltip for `step` (`n` of `of`), its anchor read in `last`'s frame.
fn tip_of(step: &Step, n: usize, of: usize, last: Option<&grants::Last>) -> Result<Tip, String> {
    let display = match last {
        Some(l) => l.frame.display,
        None => return Err("Take a screenshot first: a step's anchor and actions use its pixels.".into()),
    };
    let anchor = match (step.anchor, last) {
        (Some((x, y)), Some(l)) => Some(l.frame.on_screen(x, y).ok_or(format!("anchor [{x}, {y}] isn't on the last screenshot ({}×{}).", l.frame.w, l.frame.h))?),
        _ => None,
    };
    Ok(Tip { explanation: step.explanation.clone(), next_preview: step.next_preview.clone(), step: n, of, anchor, display })
}

/// **`request_teach_access`**: the access card, worded as a tour; allowed,
/// the tour begins.
pub async fn request(args: &Value, ctx: &Ctx<'_>) -> Run {
    let mut asked = args.clone();
    // No extras in teach mode (the desktop contract's).
    if let Some(o) = asked.as_object_mut() {
        o.remove("clipboardRead");
        o.remove("clipboardWrite");
        o.remove("systemKeyCombos");
    }
    let result = super::access::request_for(&asked, ctx, true).await;
    let allowed = result["success"] == json!(true);
    if allowed {
        begin(ctx.conversation);
    }
    let mut result = result;
    if allowed {
        result["note"] = json!("Allowed: the tour is on, and Yap's window has stepped aside. Take a screenshot, then call teach_step (or teach_batch) with the first step anchored on it.");
    }
    result.into()
}

/// **`teach_step`**.
pub async fn step(args: &Value, ctx: &Ctx<'_>) -> Run {
    if let Some(no) = not_now(ctx) {
        return no;
    }
    let step = match read_step(args, "The step") {
        Ok(s) => s,
        Err(e) => return failed(&format!("{e} Nothing was shown."), "Didn't show the step").into(),
    };
    let last = grants::with(ctx.conversation, |s| s.last.clone());
    let tip = match tip_of(&step, 1, 1, last.as_deref()) {
        Ok(t) => t,
        Err(e) => return failed(&e, "Didn't show the step").into(),
    };
    if show_and_wait(tip).await == Pressed::Exit {
        return exited(None);
    }
    let (mut out, stopped) = batch::run(&step.actions, ctx).await;
    let shot = super::look(SCREENSHOT, &json!({}), ctx).await;
    out.looks.extend(shot.looks);
    out.images.extend(shot.images);
    out.seen |= shot.seen;
    let results = out.result["data"]["results"].clone();
    out.result = match stopped {
        Some(error) => json!({ "success": false, "error": error, "data": { "results": results, "screenshot": shot.result["data"] }, "displayText": "A step's action failed" }),
        None => json!({
            "success": true,
            "data": { "results": results, "screenshot": shot.result["data"] },
            "note": "The user pressed Next and the step's actions ran. The screenshot after them comes with this result: anchor the next step on it.",
            "displayText": "Showed you a step",
        }),
    };
    out
}

/// **`teach_batch`**.
pub async fn batch(args: &Value, ctx: &Ctx<'_>) -> Run {
    if let Some(no) = not_now(ctx) {
        return no;
    }
    let steps: Result<Vec<Step>, String> = match args["steps"].as_array() {
        Some(list) if list.is_empty() => Err("steps is empty: give at least one.".into()),
        Some(list) if list.len() > STEPS_MAX => Err(format!("That's {} steps: at most {STEPS_MAX} in one batch.", list.len())),
        Some(list) => list.iter().enumerate().map(|(i, s)| read_step(s, &format!("steps[{i}]"))).collect(),
        None => Err("steps must be a list of steps.".into()),
    };
    let steps = match steps {
        Ok(s) => s,
        Err(e) => return failed(&format!("{e} Nothing was shown."), "Didn't show the steps").into(),
    };
    let n = steps.len();
    // Every anchor and click: the screenshot before the batch.
    let before = grants::with(ctx.conversation, |s| s.last.clone());
    let tips: Result<Vec<Tip>, String> = steps.iter().enumerate().map(|(i, s)| tip_of(s, i + 1, n, before.as_deref())).collect();
    let tips = match tips {
        Ok(t) => t,
        Err(e) => return failed(&format!("{e} Nothing was shown."), "Didn't show the steps").into(),
    };
    let mut out = Run::default();
    let mut results: Vec<Value> = Vec::new();
    for (i, (step, tip)) in steps.iter().zip(tips).enumerate() {
        if show_and_wait(tip).await == Pressed::Exit {
            let mut left = exited(Some(i));
            left.looks = std::mem::take(&mut out.looks);
            left.images = std::mem::take(&mut out.images);
            left.seen = out.seen;
            left.result["data"]["results"] = json!(results);
            return left;
        }
        grants::with(ctx.conversation, |s| s.last = before.clone());
        let (ran, stopped) = batch::run(&step.actions, ctx).await;
        out.looks.extend(ran.looks);
        out.images.extend(ran.images);
        out.seen |= ran.seen;
        results.push(ran.result["data"]["results"].clone());
        if let Some(error) = stopped {
            let shot = super::look(SCREENSHOT, &json!({}), ctx).await;
            out.looks.extend(shot.looks);
            out.images.extend(shot.images);
            out.seen |= shot.seen;
            out.result = json!({ "success": false, "error": error, "data": { "stepsCompleted": i, "stepFailed": i, "results": results, "screenshot": shot.result["data"] }, "displayText": "A step's action failed" });
            return out;
        }
    }
    let shot = super::look(SCREENSHOT, &json!({}), ctx).await;
    out.looks.extend(shot.looks);
    out.images.extend(shot.images);
    out.seen |= shot.seen;
    out.result = json!({
        "success": true,
        "data": { "stepsCompleted": n, "results": results, "screenshot": shot.result["data"] },
        "note": "Every step was shown and its actions ran. The final screenshot comes with this result: anchor the next batch on it.",
        "displayText": format!("Showed you {n} step{}", if n == 1 { "" } else { "s" }),
    });
    out
}

/// **Test mode: the tooltip shown now**, and Next or Exit pressed on it
/// (`press`: "next" | "exit"); the tour's state.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_computer_teach(press: Option<String>) -> Result<Value, String> {
    if !crate::shell::e2e::active() {
        return Err("e2e_computer_teach is for test runs".into());
    }
    let shown = SHOWN.lock().unwrap_or_else(|p| p.into_inner()).clone();
    let pressed = match press.as_deref() {
        Some("next") => self::press(Pressed::Next),
        Some("exit") => self::press(Pressed::Exit),
        Some(other) => return Err(format!("press is next or exit, not {other}")),
        None => false,
    };
    let t = tour().clone();
    Ok(json!({ "tip": shown, "pressed": pressed, "placed": tooltip::placed(), "tour": t.map(|t| json!({ "conversation": t.conversation, "mainHidden": t.main_hidden, "exited": t.exited })) }))
}

/// Forgets the tour (tests).
#[cfg(any(test, debug_assertions))]
pub fn reset() {
    *tour() = None;
    *WAITING.lock().unwrap_or_else(|p| p.into_inner()) = None;
    *SHOWN.lock().unwrap_or_else(|p| p.into_inner()) = None;
    super::watch::lenient(false);
}

/// The tooltip shown now (tests).
#[cfg(test)]
pub fn shown() -> Option<Tip> {
    SHOWN.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_is_checked_before_anything_shows() {
        let s = read_step(&json!({ "explanation": "This is the Save button.", "next_preview": "Next: I'll click it.", "actions": [{ "action": "left_click", "coordinate": [10, 20] }], "anchor": [10, 20] }), "The step").unwrap();
        assert_eq!(s.actions.len(), 1);
        assert_eq!(s.anchor, Some((10.0, 20.0)));
        let explains = read_step(&json!({ "explanation": "Here's the menu.", "next_preview": "", "actions": [] }), "The step").unwrap();
        assert!(explains.actions.is_empty() && explains.anchor.is_none());
        assert!(read_step(&json!({ "explanation": " ", "actions": [] }), "The step").unwrap_err().contains("explanation"));
        assert!(read_step(&json!({ "explanation": "x", "actions": [{ "action": "teleport" }] }), "steps[2]").unwrap_err().starts_with("steps[2]"));
        assert!(read_step(&json!({ "explanation": "x", "actions": [], "anchor": [1] }), "The step").unwrap_err().contains("anchor"));
        assert_eq!(definitions().len(), 3);
    }

    /// A tour on the stand-in desktop: refused before one begins; a step's
    /// tooltip, Next running its actions and a screenshot after; a batch
    /// left with Exit on its second step; nothing more once left; the
    /// turn's end closing it.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn a_tour_shows_each_step_and_next_runs_it() {
        use crate::computer::sim::{self, Scene, SimDisplay, SimWindow, Widget};
        let _one = crate::computer::tests::LOCK.lock().unwrap_or_else(|p| p.into_inner());
        grants::forget_all();
        crate::computer::watch::reset();
        reset();
        sim::set(Some(Scene {
            displays: vec![SimDisplay { x: 0, y: 0, w: 1200, h: 800, primary: true }],
            windows: vec![SimWindow {
                x: 50,
                y: 50,
                w: 560,
                h: 400,
                exe: r"C:\Apps\mail.exe".into(),
                title: "Mail".into(),
                widgets: vec![Widget { kind: "field".into(), name: "Message".into(), x: 20, y: 40, w: 200, h: 30, ..Default::default() }],
                ..Default::default()
            }],
            cursor: Some((5, 5)),
            ..Default::default()
        }));
        grants::with(Some(95), |s| s.grant(vec![grants::Granted::of_exe(r"C:\Apps\mail.exe")], false, false, false));
        let ctx = Ctx { conversation: Some(95), live: None, provider: "anthropic", base_url: "https://api.anthropic.com/v1", unattended: false, turn: "turn-t" };
        let one = json!({ "explanation": "This is where your message goes.", "next_preview": "Next: I'll type Hi in it.", "actions": [{ "action": "left_click", "coordinate": [170, 105] }, { "action": "type", "text": "Hi" }], "anchor": [170, 105] });
        assert!(step(&one, &ctx).await.result["error"].as_str().unwrap().contains("request_teach_access"), "no tour yet");
        begin(Some(95));
        assert!(on(Some(95)) && !on(Some(96)));
        let _ = crate::computer::look(SCREENSHOT, &json!({}), &ctx).await;
        // The person: Next on each tooltip as it shows, then Exit on the batch's second.
        let presses = std::sync::Arc::new(std::sync::Mutex::new(vec![Pressed::Next, Pressed::Next, Pressed::Exit]));
        let person = {
            let presses = presses.clone();
            tokio::spawn(async move {
                let mut seen: Vec<Tip> = Vec::new();
                for _ in 0..4000 {
                    if let Some(t) = shown() {
                        if seen.last() != Some(&t) || tip_up() {
                            let next = presses.lock().unwrap().first().copied();
                            if let Some(p) = next {
                                if press(p) {
                                    presses.lock().unwrap().remove(0);
                                    seen.push(t);
                                }
                            }
                        }
                    }
                    if presses.lock().unwrap().is_empty() {
                        return seen;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(2)).await;
                }
                seen
            })
        };
        let first = step(&one, &ctx).await;
        assert_eq!(first.result["success"], true, "{}", first.result);
        assert_eq!(first.looks.len(), 1, "the screenshot after the step");
        let text = || sim::scene().unwrap().windows[0].widgets[0].text.clone();
        assert_eq!(text(), "Hi");
        let two = json!({ "steps": [
            { "explanation": "Now the rest.", "next_preview": "Next: I'll add more.", "actions": [{ "action": "type", "text": " there" }] },
            { "explanation": "And sign it.", "next_preview": "Next: I'll sign it.", "actions": [{ "action": "type", "text": " - Sam" }] }
        ] });
        let left = batch(&two, &ctx).await;
        assert_eq!(left.result["data"]["exited"], true, "{}", left.result);
        assert_eq!(left.result["data"]["stepsCompleted"], 1);
        assert_eq!(text(), "Hi there", "the second step's actions never ran");
        let seen = person.await.unwrap();
        assert_eq!(seen.len(), 3);
        assert_eq!((seen[0].step, seen[0].of, seen[0].anchor.is_some()), (1, 1, true));
        assert_eq!((seen[2].step, seen[2].of, seen[2].explanation.as_str()), (2, 2, "And sign it."));
        // Left: nothing more shows.
        assert_eq!(step(&one, &ctx).await.result["data"]["exited"], true);
        // A batch with a typo shows nothing.
        turn_ended(Some(95));
        assert!(!on(Some(95)));
        begin(Some(95));
        assert!(batch(&json!({ "steps": [{ "explanation": "x", "actions": [] }, { "explanation": "", "actions": [] }] }), &ctx).await.result["error"].as_str().unwrap().contains("steps[1]"));
        assert!(shown().is_none());
        turn_ended(None);
        sim::set(None);
        grants::forget_all();
        crate::computer::watch::reset();
    }
}

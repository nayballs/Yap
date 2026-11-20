//! **`computer_batch`** (phase 3, the desktop contract's): several actions
//! in one call, so a predictable sequence (click a field, type, press
//! Return) costs one round trip to the model instead of one each.
//!
//! - Every action is read and checked before any runs (an unknown action or
//!   a missing field refuses the whole batch, nothing done).
//! - They run in order, each through the same executor and checks as on its
//!   own (`act.rs`: the window under the point, the tier, the ask-first
//!   card, the person taking over), and the batch **stops at the first
//!   error**: "Batch stopped at actions[2] (left_click): … 2 completed,
//!   3 remaining."
//! - **Coordinates — clicks and zoom regions alike — refer to the
//!   screenshot taken before the batch**, never one taken inside it; after
//!   the batch, the last screenshot it took is the reference for the next
//!   call. A screenshot or zoom inside it comes back in place, in order.

use serde_json::{json, Value};

use super::{act, failed, grants, Ctx, Run, CURSOR, SCREENSHOT, ZOOM};

pub const COMPUTER_BATCH: &str = "computer_batch";

/// The actions a batch may hold (the desktop contract's list).
pub const ACTIONS: &[&str] = &[
    act::KEY,
    act::TYPE,
    act::MOUSE_MOVE,
    act::LEFT_CLICK,
    act::DRAG,
    act::RIGHT_CLICK,
    act::MIDDLE_CLICK,
    act::DOUBLE_CLICK,
    act::TRIPLE_CLICK,
    act::SCROLL,
    act::HOLD_KEY,
    SCREENSHOT,
    ZOOM,
    CURSOR,
    act::MOUSE_DOWN,
    act::MOUSE_UP,
    act::WAIT,
];

/// The most actions one batch holds.
pub const MAX_ACTIONS: usize = 50;

/// The schema of one action (the desktop contract's), shared with teach
/// mode's steps. Pure.
pub fn action_schema() -> Value {
    let point = |description: &str| json!({ "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2, "description": description });
    json!({
        "type": "object",
        "properties": {
            "action": { "type": "string", "enum": ACTIONS, "description": "The action to perform." },
            "coordinate": point("[x, y] for a click, mouse_move, scroll or a drag's end: the pixels of the screenshot taken before this call."),
            "start_coordinate": point("[x, y]: a drag's start (left_click_drag only); without it, from where the pointer is."),
            "text": { "type": "string", "description": "For type: the text. For key and hold_key: the keys (\"ctrl+s\"). For a click or scroll: modifier keys to hold." },
            "repeat": { "type": "number", "minimum": 1, "maximum": 100, "description": "For key: how many times." },
            "scroll_direction": { "type": "string", "enum": ["up", "down", "left", "right"] },
            "scroll_amount": { "type": "number", "minimum": 0, "maximum": 100 },
            "duration": { "type": "number", "description": "Seconds (0 to 100), for hold_key and wait." },
            "region": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4, "description": "[x0, y0, x1, y1] for zoom, in the screenshot taken before this call." },
            "scale": { "type": "number", "description": "For screenshot and zoom: 0.1 to 1 (default 1); coordinates stay in the full frame." },
            "action_summary": { "type": "string", "description": "A few words saying what this action does and to what ('Opens the Filters menu'). Set it on every action but mouse_move, scroll, screenshot, zoom, cursor_position and wait. The effect only: no reasons, never a password or another secret." }
        },
        "required": ["action"]
    })
}

/// Its definition, as the tool loop takes it. Pure.
pub fn definition() -> Value {
    let item = action_schema();
    json!({
        "type": "function",
        "function": {
            "name": COMPUTER_BATCH,
            "description": "Run several actions in ONE call: each separate call costs a round trip, so batch a sequence you can predict (click a field, type into it, press Return). They run in order and stop at the first error, each checked as on its own (the window under the point must be an allowed app's). Coordinates you write in a batch — clicks and zoom regions — always refer to the screenshot taken BEFORE this call, never one taken inside it. A screenshot or zoom inside it comes back in order; after the batch, the last screenshot it took is the reference for your next call.",
            "parameters": {
                "type": "object",
                "properties": { "actions": { "type": "array", "items": item, "description": "The actions, in order: [{\"action\":\"left_click\",\"coordinate\":[100,200]},{\"action\":\"type\",\"text\":\"hello\"},{\"action\":\"key\",\"text\":\"return\"},{\"action\":\"screenshot\"}]" } },
                "required": ["actions"],
                "additionalProperties": false
            }
        }
    })
}

/// **The actions read and checked**, before any runs: each its name and
/// its own arguments. Pure.
pub fn read(actions: &Value) -> Result<Vec<(String, Value)>, String> {
    let list = actions.as_array().ok_or("actions must be a list of actions.")?;
    if list.is_empty() {
        return Err("actions is empty: give at least one.".into());
    }
    if list.len() > MAX_ACTIONS {
        return Err(format!("That's {} actions: at most {MAX_ACTIONS} in one batch.", list.len()));
    }
    let mut out = Vec::new();
    for (i, a) in list.iter().enumerate() {
        let name = a["action"].as_str().unwrap_or("");
        if !ACTIONS.contains(&name) {
            return Err(if name.is_empty() { format!("actions[{i}] has no action.") } else { format!("actions[{i}]: \"{name}\" isn't an action a batch can hold ({}).", ACTIONS.join(", ")) });
        }
        let mut args = a.as_object().cloned().unwrap_or_default();
        args.remove("action");
        let needs: &[&str] = match name {
            act::LEFT_CLICK | act::RIGHT_CLICK | act::MIDDLE_CLICK | act::DOUBLE_CLICK | act::TRIPLE_CLICK | act::MOUSE_MOVE | act::DRAG => &["coordinate"],
            act::TYPE | act::KEY | act::HOLD_KEY => &["text"],
            ZOOM => &["region"],
            _ => &[],
        };
        if let Some(missing) = needs.iter().find(|k| args.get(**k).is_none_or(Value::is_null)) {
            return Err(format!("actions[{i}] ({name}) needs {missing}."));
        }
        out.push((name.to_string(), Value::Object(args)));
    }
    Ok(out)
}

/// **`computer_batch`** for the chat `ctx` says.
pub async fn execute(args: &Value, ctx: &Ctx<'_>) -> Run {
    let actions = match read(&args["actions"]) {
        Ok(a) => a,
        Err(e) => return failed(&format!("{e} Nothing was done."), "Didn't act: the batch wasn't right").into(),
    };
    run(&actions, ctx).await.0
}

/// **Actions read with [`read`], run in order** (a batch's, a teach step's
/// after Next): what they gave, and the error a stop says (`None`: every
/// one ran).
pub async fn run(actions: &[(String, Value)], ctx: &Ctx<'_>) -> (Run, Option<String>) {
    let n = actions.len();
    let conversation = ctx.conversation;
    // Every action's coordinates: the screenshot before the batch.
    let before = grants::with(conversation, |s| s.last.clone());
    let mut newest = None;
    let mut out = Run::default();
    let mut results: Vec<Value> = Vec::new();
    let mut chips: Vec<String> = Vec::new();
    for (i, (name, a)) in actions.iter().enumerate() {
        grants::with(conversation, |s| s.last = before.clone());
        let ran = Box::pin(super::one(name, a, ctx)).await;
        if name == SCREENSHOT && ran.result["success"] == json!(true) {
            newest = grants::with(conversation, |s| s.last.clone());
        }
        out.looks.extend(ran.looks);
        out.images.extend(ran.images);
        out.seen |= ran.seen;
        let chip = ran.result["displayText"].as_str().filter(|c| !c.is_empty()).unwrap_or(name).to_string();
        chips.push(chip.clone());
        if ran.result["success"] != json!(true) {
            let error = ran.result["error"].as_str().unwrap_or("it failed.").to_string();
            results.push(json!({ "action": name, "ok": false, "error": error }));
            grants::with(conversation, |s| s.last = newest.clone().or_else(|| before.clone()));
            let remaining = n - i - 1;
            tracing::info!(at = i, of = n, "Computer use: a batch stopped");
            let stopped = format!("Batch stopped at actions[{i}] ({name}): {error} {i} completed, {remaining} remaining.");
            out.result = json!({
                "success": false,
                "error": stopped,
                "data": { "results": results, "completed": i, "remaining": remaining },
                "displayText": "",
                "displayTexts": chips,
            });
            return (out, Some(stopped));
        }
        results.push(json!({ "action": name, "ok": true, "displayText": chip, "data": ran.result["data"].clone() }));
    }
    let took = newest.is_some();
    grants::with(conversation, |s| s.last = newest.or(before));
    let note = match (took, out.looks.is_empty()) {
        (true, _) => "All done. The pictures come in order; the last screenshot this batch took is the reference for your next call's coordinates.",
        (false, false) => "All done. The pictures come in order; coordinates still refer to the screenshot before this batch.",
        (false, true) => "All done. Take a screenshot to see what it did.",
    };
    out.result = json!({ "success": true, "data": { "results": results, "completed": n }, "note": note, "displayText": "", "displayTexts": chips });
    (out, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_batch_is_checked_whole_before_anything_runs() {
        let ok = read(&json!([{ "action": "left_click", "coordinate": [10, 20], "action_summary": "Clicks the box" }, { "action": "type", "text": "hi" }, { "action": "screenshot" }])).unwrap();
        assert_eq!(ok.len(), 3);
        assert_eq!(ok[0], ("left_click".to_string(), json!({ "coordinate": [10, 20], "action_summary": "Clicks the box" })));
        assert!(read(&json!([])).is_err());
        assert!(read(&json!("left_click")).is_err());
        assert!(read(&json!([{ "action": "type", "text": "a" }, { "action": "request_access" }])).unwrap_err().contains("actions[1]"));
        assert!(read(&json!([{ "action": "left_click" }])).unwrap_err().contains("coordinate"));
        assert!(read(&json!([{ "action": "zoom", "region": null }])).unwrap_err().contains("region"));
        assert!(read(&json!([{ "text": "a" }])).unwrap_err().contains("no action"));
        assert!(read(&Value::Array(vec![json!({ "action": "wait", "duration": 0 }); MAX_ACTIONS + 1])).is_err());
        let d = definition();
        assert_eq!(d["function"]["name"], COMPUTER_BATCH);
        assert_eq!(d["function"]["parameters"]["properties"]["actions"]["items"]["properties"]["coordinate"]["type"], "array");
        assert_eq!(d["function"]["parameters"]["properties"]["actions"]["items"]["properties"]["action"]["enum"].as_array().unwrap().len(), ACTIONS.len());
    }

    /// Phase 3 on the stand-in desktop: a batch runs in order from the
    /// screenshot before it and stops at the first error; the clipboard
    /// with its grants; an allowed app brought forward or opened.
    #[allow(clippy::await_holding_lock)]
    #[tokio::test]
    async fn a_batch_runs_in_order_and_stops_at_the_first_error() {
        use crate::computer::sim::{self, Scene, SimDisplay, SimWindow, Widget};
        let _one = crate::computer::tests::LOCK.lock().unwrap_or_else(|p| p.into_inner());
        grants::forget_all();
        crate::computer::watch::reset();
        let w = |kind: &str, name: &str, x: i32, y: i32| Widget { kind: kind.into(), name: name.into(), x, y, w: 160, h: 30, ..Default::default() };
        sim::set(Some(Scene {
            displays: vec![SimDisplay { x: 0, y: 0, w: 1200, h: 800, primary: true }],
            windows: vec![
                SimWindow { x: 700, y: 500, w: 300, h: 200, exe: r"C:\Apps\other.exe".into(), title: "Other".into(), ..Default::default() },
                SimWindow { x: 50, y: 50, w: 560, h: 400, exe: r"C:\Apps\mail.exe".into(), title: "Mail".into(), widgets: vec![w("field", "Message", 20, 40), w("field", "Subject", 20, 90)], ..Default::default() },
            ],
            cursor: Some((5, 5)),
            ..Default::default()
        }));
        let mail = grants::Granted::of_exe(r"C:\Apps\mail.exe");
        grants::with(Some(91), |s| s.grant(vec![mail], true, true, false));
        let ctx = Ctx { conversation: Some(91), live: None, provider: "anthropic", base_url: "https://api.anthropic.com/v1", unattended: false, turn: "turn-b" };
        let batch = |actions: Value| async move { execute(&json!({ "actions": actions }), &ctx).await };
        // No screenshot before it: its first click has no frame, and nothing else runs.
        let none = batch(json!([{ "action": "left_click", "coordinate": [100, 105] }, { "action": "type", "text": "x" }])).await.result;
        assert!(none["error"].as_str().unwrap().starts_with("Batch stopped at actions[0] (left_click)"), "{none}");
        assert!(none["error"].as_str().unwrap().ends_with("0 completed, 1 remaining."), "{none}");
        let shot = crate::computer::look(SCREENSHOT, &json!({}), &ctx).await;
        assert_eq!(shot.result["success"], true, "{}", shot.result);
        let did = batch(json!([
            { "action": "left_click", "coordinate": [100, 105], "action_summary": "Clicks into the message" },
            { "action": "type", "text": "Running late" },
            { "action": "screenshot", "scale": 0.5 },
            { "action": "left_click", "coordinate": [100, 155] },
            { "action": "type", "text": "Today" },
            { "action": "zoom", "region": [50, 50, 610, 450] }
        ]))
        .await;
        assert_eq!(did.result["success"], true, "{}", did.result);
        assert_eq!(did.result["data"]["completed"], 6);
        assert_eq!(did.looks.len(), 2, "the screenshot and the zoom, in order");
        assert!(did.seen);
        assert_eq!(did.result["displayTexts"][0], "Clicks into the message");
        let text = |name: &str| sim::scene().unwrap().windows.iter().find(|w| w.title == "Mail").unwrap().widgets.iter().find(|g| g.name == name).unwrap().text.clone();
        assert_eq!((text("Message"), text("Subject")), ("Running late".to_string(), "Today".to_string()));
        // A click on the app not allowed stops it: the typing after it never runs.
        let stop = batch(json!([{ "action": "left_click", "coordinate": [100, 105] }, { "action": "left_click", "coordinate": [800, 600] }, { "action": "type", "text": "never" }])).await.result;
        assert_eq!(stop["success"], false);
        let why = stop["error"].as_str().unwrap();
        assert!(why.starts_with("Batch stopped at actions[1] (left_click)") && why.ends_with("1 completed, 1 remaining."), "{why}");
        assert_eq!(text("Message"), "Running late");
        assert!(batch(json!([{ "action": "request_access" }])).await.result["error"].as_str().unwrap().contains("Nothing was done"));
        // The clipboard, with its grants (another chat has none).
        assert_eq!(crate::computer::clipboard::write(&json!({ "text": "Hello" }), &ctx).await.result["success"], true);
        assert_eq!(sim::scene().unwrap().clipboard.as_deref(), Some("Hello"));
        let read = crate::computer::clipboard::read(&ctx).await;
        assert_eq!(read.result["data"]["text"], "Hello");
        assert!(read.seen, "the clipboard is outside content");
        let other = Ctx { conversation: Some(92), ..ctx };
        assert!(crate::computer::clipboard::read(&other).await.result["error"].as_str().unwrap().contains("clipboardRead"));
        // open_application: an allowed app's window to the front, one not on screen opened, one not allowed refused.
        let open = |app: &'static str| async move { crate::computer::open::execute(&json!({ "app": app }), &ctx).await.result };
        assert_eq!(open("mail").await["displayText"], "Brought mail to the front");
        assert_eq!(sim::scene().unwrap().windows[0].title, "Mail");
        grants::with(Some(91), |s| s.grant(vec![grants::Granted::of_exe(r"C:\Apps\notes.exe")], true, true, false));
        assert_eq!(open("notes").await["displayText"], "Opened notes");
        let scene = sim::scene().unwrap();
        assert_eq!(scene.windows[0].title, "notes");
        assert!(scene.log.iter().any(|l| l == "opened notes"), "{:?}", scene.log);
        assert!(open("Paint").await["error"].as_str().unwrap().contains("isn't one of the apps allowed"));
        sim::set(None);
        grants::forget_all();
        crate::computer::watch::reset();
    }
}

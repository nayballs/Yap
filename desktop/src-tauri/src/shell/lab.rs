//! **Yap Lab's side of a test Yap** (debug builds only; `docs/lab.md`).
//!
//! Yap Lab (`npm run lab`, `scripts/lab/`) starts a test Yap in test mode
//! (`e2e.rs`) for the Claude Code session that maintains Yap, puts questions
//! to its Chat — typed in Chat's box, or asked out loud through
//! `e2e_assistant_ask` — and reads back what each turn did. A lab run is
//! test mode with `YAP_LAB=1`; everything test mode keeps out (the keyboard
//! hook, pastes into other apps, the focus, the real screen: `look_at_screen`
//! sees test mode's made-up monitors) stays out.
//!
//! **Live** (`YAP_LAB_LIVE=1`, every lab run but `--fake`): the two things
//! test mode swaps for stand-ins are real again, because finding real-world
//! problems is the point —
//! - **the web**: search, page reads, Yap's free sources and the pages
//!   beside the chat go to the internet (`web::Endpoints::current`), with
//!   every one of Yap's usual safety rules;
//! - **Claude Code**: the person's own, found as Yap finds it
//!   (`claude_code::cli::find`), on their own sign-in. Yap never signs in
//!   for them (test mode never opens the sign-in) and never reads its
//!   credentials: `claude auth status` says whether it's signed in, as in
//!   the app.
//!
//! **The record** (`YAP_LAB_RECORD`: a JSON-lines file in the lab's own
//! folder): what each turn did, as it went, for the lab to read back —
//! - `stream`: every event the Chat view's channel got (`chat_send`'s
//!   `onStream`: the steps, chips, cards, page tabs, pictures; the answer's
//!   text itself only as its length);
//! - `sent`: what `chat_send` answered (the reply, or the error);
//! - `tool`: each call of Yap's tool loop with its arguments and result
//!   (`eval::tool_called`'s hook, which the loop already calls);
//! - `claude`: Claude Code's own stream-json lines for a run Yap started (a
//!   plan chat, an agent): its tool calls (Yap's MCP tools and its own) with
//!   their inputs, their results, its turn's end — never the deltas;
//! - `event`: the app-wide events a spoken question and an agent go by
//!   (`yap-assistant-turn`, `yap-assistant-stream`, `yap-chat-answering`,
//!   `yap-claude-approve`, `yap-agent`, `yap-alert`…).
//!
//! Long strings are cut (their length kept) and pictures' base64 replaced
//! by their size. The record holds the lab's own questions and the pages
//! they read, in the lab's own folder: never Yap's log, never a normal run
//! (release builds compile it all out: [`on`] is always `false`).
//!
//! **The main window's size** (`YAP_LAB_WINDOW`, "1280x860": `lab start
//! --size`): set once at the end of setup, so the lab's screenshots of one
//! run compare with another's.

use serde_json::Value;
use tauri::AppHandle;

/// `true` in a lab run: test mode (`e2e::active`) started with `YAP_LAB=1`.
pub fn on() -> bool {
    #[cfg(debug_assertions)]
    {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ON.get_or_init(|| super::e2e::active() && std::env::var("YAP_LAB").is_ok_and(|v| v.trim() == "1"))
    }
    #[cfg(not(debug_assertions))]
    false
}

/// A lab run on the real web and the person's own Claude Code (`YAP_LAB_LIVE=1`:
/// every lab run but `--fake`). Never in a release build ([`on`]).
pub fn live() -> bool {
    static LIVE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *LIVE.get_or_init(|| on() && std::env::var("YAP_LAB_LIVE").is_ok_and(|v| v.trim() == "1"))
}

/// An event the Chat view's channel got (`stream::PageChannel`).
pub fn stream(event: &Value) {
    #[cfg(debug_assertions)]
    if on() {
        imp::record("stream", serde_json::json!({ "event": imp::without_text(event) }));
    }
    let _ = event;
}

/// What `chat_send` answered: the reply (`conversation`: the one it was
/// sent to, `None` for a new chat), or why it failed.
pub fn sent(conversation: Option<u64>, out: &Result<Value, String>) {
    #[cfg(debug_assertions)]
    if on() {
        let data = match out {
            Ok(reply) => serde_json::json!({ "to": conversation, "ok": true, "reply": reply }),
            Err(e) => serde_json::json!({ "to": conversation, "ok": false, "error": e }),
        };
        imp::record("sent", data);
    }
    let _ = (conversation, out);
}

/// One call of Yap's tool loop, with what it answered.
pub fn tool(name: &str, args: &Value, result: &Value) {
    #[cfg(debug_assertions)]
    if on() {
        imp::record("tool", serde_json::json!({ "via": "yap", "name": name, "args": args, "result": result }));
    }
    let _ = (name, args, result);
}

/// A line Claude Code wrote on its stdout (a run Yap started, process
/// `pid`; `agent`: an agent's run, not a plan chat's or a one-off
/// question's): its whole messages, results and turn ends are kept, never
/// the deltas.
pub fn claude_line(pid: u32, agent: bool, line: &str) {
    #[cfg(debug_assertions)]
    if on() {
        imp::claude_line(pid, agent, line);
    }
    let _ = (pid, agent, line);
}

/// At the end of setup, in a lab run: follow the app-wide events a turn
/// goes by.
pub fn start(app: &AppHandle) {
    #[cfg(debug_assertions)]
    if on() {
        imp::start(app);
    }
    let _ = app;
}

#[cfg(debug_assertions)]
mod imp {
    use std::io::Write;
    use std::sync::Mutex;

    use serde_json::{json, Map, Value};
    use tauri::{AppHandle, Listener, Manager};

    /// The app-wide events a lab run keeps.
    const EVENTS: &[&str] = &[
        "yap-assistant-turn",
        "yap-assistant-stream",
        "yap-chat-answering",
        "yap-claude-approve",
        "yap-claude-approve-done",
        "yap-agent",
        "yap-agent-report",
        "yap-alert",
    ];
    /// The stream events whose words aren't kept (the answer is in
    /// chats.json): only how long they were.
    const WORDS: &[&str] = &["text", "thinking", "section"];
    /// Claude Code's lines a lab run keeps (by `type`).
    const CLAUDE_TYPES: &[&str] = &["assistant", "user", "result", "system", "control_request"];
    /// The longest string kept whole.
    const MAX_STRING: usize = 20_000;

    struct Record {
        next: u64,
        file: Option<std::fs::File>,
    }

    static RECORD: Mutex<Option<Record>> = Mutex::new(None);

    fn now_ms() -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
    }

    /// One item: `{seq, ts, kind, …data}`, a line of the record file.
    pub(super) fn record(kind: &str, data: Value) {
        let mut guard = RECORD.lock().unwrap_or_else(|p| p.into_inner());
        let rec = guard.get_or_insert_with(|| Record {
            next: 1,
            file: std::env::var_os("YAP_LAB_RECORD")
                .filter(|p| !p.is_empty())
                .and_then(|p| std::fs::OpenOptions::new().create(true).append(true).open(p).ok()),
        });
        let mut item = Map::new();
        item.insert("seq".into(), json!(rec.next));
        item.insert("ts".into(), json!(now_ms()));
        item.insert("kind".into(), json!(kind));
        if let Value::Object(fields) = shrink(data) {
            item.extend(fields);
        }
        rec.next += 1;
        if let Some(f) = rec.file.as_mut() {
            let _ = writeln!(f, "{}", Value::Object(item));
        }
    }

    /// A stream event without its words (`text`, `thinking`, `section`):
    /// their length instead.
    pub(super) fn without_text(event: &Value) -> Value {
        let kind = event.get("kind").and_then(Value::as_str).unwrap_or("");
        if WORDS.contains(&kind) {
            let chars = event.get("text").and_then(Value::as_str).map_or(0, |t| t.chars().count());
            return json!({ "kind": kind, "chars": chars });
        }
        event.clone()
    }

    pub(super) fn claude_line(pid: u32, agent: bool, line: &str) {
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else { return };
        let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
        if !CLAUDE_TYPES.contains(&kind) {
            return;
        }
        // Its hooks' chatter and status pings say nothing about the turn.
        if kind == "system" && !matches!(value.get("subtype").and_then(Value::as_str), Some("init" | "permission_denied" | "api_retry")) {
            return;
        }
        record("claude", json!({ "pid": pid, "agent": agent, "line": value }));
    }

    pub(super) fn start(app: &AppHandle) {
        for name in EVENTS {
            app.listen(*name, move |e| {
                let mut payload = serde_json::from_str::<Value>(e.payload()).unwrap_or(Value::Null);
                // A spoken answer's stream: its words' length only, as the Chat view's.
                if let Some(ev) = payload.get("event").cloned() {
                    payload["event"] = without_text(&ev);
                }
                record("event", json!({ "name": name, "payload": payload }));
            });
        }
        size_main_window(app);
        tracing::info!("lab: the record is on");
    }

    /// The main window at `YAP_LAB_WINDOW`'s size (logical pixels: the
    /// page's CSS pixels), when the lab asked for one.
    fn size_main_window(app: &AppHandle) {
        let Ok(spec) = std::env::var("YAP_LAB_WINDOW") else { return };
        let Some((width, height)) = window_size(&spec) else {
            tracing::warn!("lab: YAP_LAB_WINDOW isn't WIDTHxHEIGHT within 860x600 to 7680x4320");
            return;
        };
        let Some(w) = app.get_webview_window("settings") else { return };
        match w.set_size(tauri::LogicalSize::new(width, height)) {
            Ok(()) => tracing::info!(width, height, "lab: the main window sized"),
            Err(e) => tracing::warn!("lab: couldn't size the main window: {e}"),
        }
    }

    /// "1280x860" → its width and height, within the main window's minimum
    /// (860 × 600) and an 8K screen. Pure.
    pub(super) fn window_size(spec: &str) -> Option<(f64, f64)> {
        let (w, h) = spec.trim().split_once(['x', 'X', '×'])?;
        let (w, h) = (w.trim().parse::<u32>().ok()?, h.trim().parse::<u32>().ok()?);
        ((860..=7680).contains(&w) && (600..=4320).contains(&h)).then_some((f64::from(w), f64::from(h)))
    }

    /// `v` with long strings cut (their length said) and pictures' base64
    /// replaced by its size. Pure.
    pub(super) fn shrink(v: Value) -> Value {
        match v {
            Value::String(s) => Value::String(cut(s)),
            Value::Array(items) => Value::Array(items.into_iter().map(shrink).collect()),
            Value::Object(fields) => Value::Object(
                fields
                    .into_iter()
                    .map(|(k, v)| match v {
                        Value::String(s) if is_base64(&k, &s) => (k, Value::String(format!("<base64: {} chars>", s.len()))),
                        v => (k, shrink(v)),
                    })
                    .collect(),
            ),
            v => v,
        }
    }

    fn cut(s: String) -> String {
        let chars = s.chars().count();
        if chars <= MAX_STRING {
            return s;
        }
        let kept: String = s.chars().take(MAX_STRING).collect();
        format!("{kept}…[cut: {chars} chars]")
    }

    /// A picture's data: base64 under `data` (MCP image content, the
    /// Messages API's `source`), or a `data:` URL anywhere.
    fn is_base64(key: &str, s: &str) -> bool {
        (s.len() > 512 && s.starts_with("data:image/"))
            || (key == "data" && s.len() > 512 && !s.contains(' ') && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"+/=\r\n".contains(&b)))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn long_strings_are_cut_and_pictures_left_out() {
            let long = "a".repeat(MAX_STRING + 5);
            let picture = "QUJD".repeat(300);
            let v = shrink(json!({ "text": long, "content": [{ "type": "image", "data": picture.clone() }], "url": format!("data:image/png;base64,{picture}"), "n": 3 }));
            assert!(v["text"].as_str().unwrap().ends_with(&format!("…[cut: {} chars]", MAX_STRING + 5)));
            assert_eq!(v["content"][0]["data"], json!("<base64: 1200 chars>"));
            assert!(v["url"].as_str().unwrap().starts_with("<base64: "));
            assert_eq!(v["n"], json!(3));
            // A short `data` field or one with words is kept.
            assert_eq!(shrink(json!({ "data": "hello there" }))["data"], json!("hello there"));
        }

        #[test]
        fn the_window_size_is_width_by_height_within_bounds() {
            assert_eq!(window_size("1280x860"), Some((1280.0, 860.0)));
            assert_eq!(window_size(" 1600 X 900 "), Some((1600.0, 900.0)));
            assert_eq!(window_size("1920×1080"), Some((1920.0, 1080.0)));
            assert_eq!(window_size("800x600"), None);
            assert_eq!(window_size("1280"), None);
            assert_eq!(window_size("wide x tall"), None);
        }

        #[test]
        fn a_stream_events_words_are_only_their_length() {
            assert_eq!(without_text(&json!({ "kind": "text", "text": "Hello, Sam." })), json!({ "kind": "text", "chars": 11 }));
            let chips = json!({ "kind": "chips", "chips": ["Searched the web"], "sources": [], "web": true });
            assert_eq!(without_text(&chips), chips);
        }
    }
}

//! **A Code chat's own slash commands** (17 Nov 2026, the Claude Code app's
//! `/clear` and `/compact`, picked from the + menu's Slash commands or typed;
//! `/model` is the page's: it switches the chat's model chip). Only in a Code
//! chat that works directly (`chat/turn.rs` `goes_to_session`); anywhere else
//! the words go as typed, as every other "/" does.
//!
//! - **`/clear`** ([`code_chat_clear`]): Claude starts afresh. The chat's
//!   agents then are set aside (`menu::add_clear`: never carried on again),
//!   so its next message starts a new session, as the app's `/clear` does.
//!   Nothing is deleted: the thread keeps every message, under a divider
//!   above the next one ("Context cleared"), and their own session stays on
//!   this PC (Fork from here above it still works; Rewind there doesn't). Not
//!   while a turn works: the page asks first and stops it.
//!   (Sent to Claude Code itself, 2.1.296 answers `conversation_reset` and a
//!   new session's init, then an empty result: Yap does the same on its side
//!   without a process.)
//! - **`/compact [instructions]`** ([`code_chat_compact`]): Claude Code's own
//!   compaction of the chat's session, run headless on it as `/context` is
//!   (`claude -p "/compact …" --resume <session>` in the workspace, on the
//!   chat's model). Checked on 2.1.296 (17 Nov 2026): `system/status`
//!   `compacting`, then `compact_result: "success"`, a `compact_boundary`
//!   with `compact_metadata` (`pre_tokens`, `post_tokens`, `duration_ms`,
//!   `trigger: "manual"`), the summary as a synthetic message and an empty
//!   result (`num_turns` 0); the session keeps its id, and its next turn
//!   starts from the summary. It is saved into the session (no
//!   `--no-session-persistence`: that would throw the compaction away). The
//!   person's own hooks run, as for any compaction. After it, the turn's
//!   context is the compacted size, for the chat's rings.
//!
//! The log has ids and counts, never the chat's words.

use std::ffi::OsString;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use super::store::{self, Agent, Status};
use crate::ai::claude_code::cli::{self, Place};
use crate::ai::claude_code::session::{self, Pay};

/// How long a compaction may take (it starts the person's MCP servers, and
/// the model summarises the whole conversation).
const COMPACT_WAIT: Duration = Duration::from_secs(5 * 60);

/// Every window hears of a compaction starting and ending
/// (`{conversation, phase: "start"|"done"|"failed", before?, after?}`).
pub const EVENT: &str = "yap-code-compact";

/// What a command did.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Done {
    /// It was this Code chat's to do (it works directly); `false`: send the
    /// words as typed.
    pub handled: bool,
    /// `/compact`: the context before and after, in tokens.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<u64>,
}

/// Whether Code chat `conversation` works directly now (its words go
/// straight to its own session).
fn direct(conversation: u64) -> bool {
    let cfg = crate::config::load();
    crate::chat::turn::goes_to_session(&cfg, Some(conversation), false, false)
}

/// **`/clear`** in Code chat `conversation`: its agents then are never
/// carried on again, its next message starts afresh. Refused while one works
/// (the page stops it first, asked).
#[tauri::command]
pub fn code_chat_clear(conversation: u64) -> Result<Done, String> {
    if !direct(conversation) {
        return Ok(Done::default());
    }
    let agents: Vec<Agent> = store::of_conversation(conversation).into_iter().filter(|a| !super::menu::cleared(conversation, &a.id)).collect();
    if agents.iter().any(|a| a.status.live() || super::runner::active(&a.id)) || super::runner::queue_state(conversation) != "idle" {
        return Err("Claude is working: stop it first, then clear.".into());
    }
    let at = crate::chat::chats::get(conversation).map_or(0, |c| c.messages.len());
    if at == 0 {
        return Err("There's nothing to clear yet.".into());
    }
    let ids: Vec<String> = agents.iter().map(|a| a.id.clone()).collect();
    super::menu::add_clear(conversation, at, ids.clone());
    super::menu::saved().map_err(|e| e.to_string())?;
    // The plan's ring for this chat: nothing in its context now.
    crate::ai::claude_plan::usage::forget_context_now(conversation);
    tracing::info!(conversation, agents = ids.len(), messages = at, "Code chat: cleared");
    Ok(Done { handled: true, ..Default::default() })
}

/// What a compaction said: its sizes, or why it didn't.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Compaction {
    pub ok: bool,
    pub before: u64,
    pub after: u64,
    pub ms: u64,
    /// Claude Code's own words when it didn't (an error result's).
    pub said: String,
}

/// **A compaction's output read** (stream-json lines). Pure.
pub fn parse_compaction(stdout: &str) -> Compaction {
    let mut out = Compaction::default();
    for line in stdout.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else { continue };
        let sub = v.get("subtype").and_then(Value::as_str).unwrap_or_default();
        match (v.get("type").and_then(Value::as_str).unwrap_or_default(), sub) {
            ("system", "compact_boundary") => {
                let m = v.get("compact_metadata").cloned().unwrap_or(Value::Null);
                let n = |k: &str| m.get(k).and_then(Value::as_u64).unwrap_or(0);
                out.ok = true;
                out.before = n("pre_tokens");
                out.after = n("post_tokens");
                out.ms = n("duration_ms");
            }
            ("system", "status") => {
                if let Some(r) = v.get("compact_result").and_then(Value::as_str) {
                    if r != "success" {
                        out.ok = false;
                        let why = v.get("compact_error").and_then(Value::as_str).unwrap_or(r);
                        out.said = why.chars().take(300).collect();
                    }
                }
            }
            ("result", _) if v.get("is_error").and_then(Value::as_bool) == Some(true) => {
                out.ok = false;
                let r = v.get("result").and_then(Value::as_str).unwrap_or_default();
                if !r.is_empty() {
                    out.said = r.chars().take(300).collect();
                }
            }
            _ => {}
        }
    }
    out
}

/// **`/compact`'s arguments** for `session` on `model`, with the person's
/// own `instructions` for the summary. Saved into the session. Pure.
pub fn compact_args(session: &str, model: Option<&str>, instructions: &str) -> Vec<String> {
    let instructions = instructions.split_whitespace().collect::<Vec<_>>().join(" ");
    let prompt = if instructions.is_empty() { "/compact".to_string() } else { format!("/compact {instructions}") };
    let mut a: Vec<String> = vec!["-p".into(), prompt, "--resume".into(), session.into(), "--output-format".into(), "stream-json".into(), "--verbose".into()];
    if let Some(m) = model.map(str::trim).filter(|m| !m.is_empty()) {
        a.push("--model".into());
        a.push(m.to_string());
    }
    a
}

/// Chats compacting now.
static COMPACTING: std::sync::Mutex<Vec<u64>> = std::sync::Mutex::new(Vec::new());

fn emit(payload: Value) {
    if let Some(app) = super::runner::app() {
        use tauri::Emitter;
        let _ = app.emit(EVENT, payload);
    }
}

/// **`/compact [instructions]`** in Code chat `conversation`: Claude Code's
/// own compaction of its session (the module's notes). Refused while it
/// works, before it has a session, or while one runs already.
#[tauri::command]
pub async fn code_chat_compact(conversation: u64, instructions: Option<String>) -> Result<Done, String> {
    if !direct(conversation) {
        return Ok(Done::default());
    }
    let folder = super::code_chat::folder_of(Some(conversation)).ok_or("That isn't a Code chat any more.")?;
    let agent = crate::chat::tools::agents::chats_agent_in(Some(conversation), &folder).filter(|a| a.started_ts > 0 && !a.session.is_empty()).ok_or("There's nothing to compact yet: Claude hasn't answered here since it last started afresh.")?;
    if matches!(agent.status, Status::Queued | Status::Running | Status::Asking) || super::runner::active(&agent.id) {
        return Err("Claude is working: compact once this turn is done.".into());
    }
    if agent.branch.is_some() {
        return Err("This chat was rewound: its next message picks the conversation up first, then it can be compacted.".into());
    }
    {
        let mut going = COMPACTING.lock().unwrap_or_else(|p| p.into_inner());
        if going.contains(&conversation) {
            return Err("It's compacting already.".into());
        }
        going.push(conversation);
    }
    emit(serde_json::json!({ "conversation": conversation, "phase": "start" }));
    let program = cli::find();
    let run = tauri::async_runtime::spawn_blocking({
        let agent = agent.clone();
        let instructions = instructions.unwrap_or_default();
        move || -> Result<Compaction, String> {
            let program = program.ok_or("Claude Code isn't installed on this PC.")?;
            let cfg = crate::config::load();
            let key = cfg.provider_api_key("anthropic", "");
            let pay = if agent.pay == "key" && !key.trim().is_empty() { Pay::ApiKey(session::ApiKey::new(key)) } else { Pay::Plan };
            let extra: Vec<(String, String)> = super::runner::TODO_ENV.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
            let env: Vec<(OsString, OsString)> = session::environment(std::env::vars_os(), &pay, &extra)?;
            let args = compact_args(&agent.session, Some(&agent.model), &instructions);
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            let out = cli::run_captured_at(&program, &args, Place { dir: agent.path.clone().into(), env, what: "Claude Code" }, COMPACT_WAIT)?;
            Ok(parse_compaction(&out.stdout))
        }
    })
    .await
    .map_err(|e| e.to_string())
    .and_then(|r| r);
    COMPACTING.lock().unwrap_or_else(|p| p.into_inner()).retain(|c| *c != conversation);
    let done = match run {
        Ok(c) if c.ok => c,
        Ok(c) => {
            tracing::info!(conversation, "Code chat: Claude Code didn't compact");
            emit(serde_json::json!({ "conversation": conversation, "phase": "failed" }));
            return Err(if c.said.is_empty() { "Claude Code didn't compact the conversation.".into() } else { format!("Claude Code didn't compact the conversation: {}", c.said) });
        }
        Err(e) => {
            tracing::info!(conversation, "Code chat: couldn't compact ({e})");
            emit(serde_json::json!({ "conversation": conversation, "phase": "failed" }));
            return Err(e);
        }
    };
    // Its context now: the compacted size, on its latest turn (the summary's and the Agent panel's numbers) and
    // the plan's ring for the chat.
    let window = agent.turn_list.iter().rev().find_map(|t| t.usage.as_ref().map(|u| u.window)).unwrap_or(0);
    store::touch(&agent.id, |j| {
        if let Some(u) = j.turn_list.iter_mut().rev().find_map(|t| t.usage.as_mut()) {
            u.context = done.after;
        }
    });
    if window > 0 {
        crate::ai::claude_plan::usage::note_context_now(conversation, crate::ai::claude_plan::usage::Fill { used: done.after, window, known: true });
    }
    tracing::info!(conversation, before = done.before, after = done.after, ms = done.ms, "Code chat: compacted");
    emit(serde_json::json!({ "conversation": conversation, "phase": "done", "before": done.before, "after": done.after }));
    Ok(Done { handled: true, before: Some(done.before), after: Some(done.after) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_compactions_sizes_are_read_from_its_boundary_and_a_failure_says_why() {
        // As 2.1.296 prints it (made-up ids).
        let ok = [
            r#"{"type":"system","subtype":"status","status":"compacting","session_id":"s-1"}"#,
            r#"{"type":"system","subtype":"status","status":null,"compact_result":"success","session_id":"s-1"}"#,
            r#"{"type":"system","subtype":"init","session_id":"s-1","tools":[]}"#,
            r#"{"type":"system","subtype":"compact_boundary","session_id":"s-1","compact_metadata":{"trigger":"manual","pre_tokens":54135,"post_tokens":2922,"cumulative_dropped_tokens":51213,"duration_ms":4276}}"#,
            r#"{"type":"user","message":{"role":"user","content":"This session is being continued…"},"isSynthetic":true}"#,
            r#"{"type":"result","subtype":"success","is_error":false,"num_turns":0,"result":"","session_id":"s-1","total_cost_usd":0.4}"#,
        ]
        .join("\n");
        assert_eq!(parse_compaction(&ok), Compaction { ok: true, before: 54135, after: 2922, ms: 4276, said: String::new() });
        let failed = [
            r#"{"type":"system","subtype":"status","status":null,"compact_result":"failed","compact_error":"Not enough messages to compact."}"#,
            r#"{"type":"result","subtype":"success","is_error":false,"num_turns":0,"result":""}"#,
        ]
        .join("\n");
        let f = parse_compaction(&failed);
        assert!(!f.ok);
        assert_eq!(f.said, "Not enough messages to compact.");
        let err = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"result":"Not logged in"}"#;
        assert_eq!(parse_compaction(err).said, "Not logged in");
        assert!(!parse_compaction("not json").ok);
    }

    #[test]
    fn compact_runs_on_the_chats_session_and_model_and_is_saved_into_it() {
        let a = compact_args("3f6c2a10-0000-4000-8000-00000000c0de", Some("claude-opus-5-5"), "  keep   the test plan ");
        assert_eq!(a[..3], ["-p", "/compact keep the test plan", "--resume"]);
        assert_eq!(a[3], "3f6c2a10-0000-4000-8000-00000000c0de");
        assert!(a.windows(2).any(|w| w == ["--model", "claude-opus-5-5"]));
        assert!(a.windows(2).any(|w| w == ["--output-format", "stream-json"]));
        assert!(!a.iter().any(|x| x == "--no-session-persistence"), "the compaction is kept in the session");
        assert_eq!(compact_args("s", None, "")[1], "/compact");
    }
}

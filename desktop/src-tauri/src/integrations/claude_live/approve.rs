//! **`approve`, an agent's permission prompt tool** (`--permission-prompt-tool
//! mcp__yap__approve`). When nothing else decides a tool call (the
//! permission mode, the rules), Claude Code calls it with `{tool_name,
//! input, tool_use_id}` (2.1.296's own call, read on this PC) and waits for
//! one text block holding JSON: `{"behavior":"allow","updatedInput":{…}}`
//! or `{"behavior":"deny","message":"…"}` (code.claude.com's Agent SDK
//! "Handle approvals and user input"; `updatedInput` is the input as asked,
//! which Claude Code before 2.1.207 required). The tool is listed, as
//! Claude Code needs, but a run started with the flag keeps it out of the
//! model's own tools (2.1.296, checked: `system/init` lists it without the
//! flag, not with it), so only Claude Code calls it.
//!
//! The person decides, never the model: a card ([`Pending`], the app event
//! `yap-claude-approve`: `{id, run, conversation, origin, tool, summary,
//! detail, label}`) in plain words ("Run a command: npm test", "Edit file:
//! src/app.ts") with the details trimmed, answered by
//! `claude_approve_answer` (Allow once / Deny). No answer in
//! [`super::APPROVE_WAIT`], or the run ended first, or Claude Code cancelled
//! the call: a no, said to Claude in words it can put in its report. Every
//! card's end goes out as `yap-claude-approve-done` `{id, run, outcome}`
//! (`allowed`, `denied`, `timedOut`, `ended`, `cancelled`).
//!
//! **A plan** (2026-11-07): Plan mode's `ExitPlanMode` ([`EXIT_PLAN`]) is
//! the agent asking to go ahead with its plan, so its card says so — "The
//! plan is ready", the plan itself (its input's `plan`, markdown, up to
//! [`PLAN_MAX`]), **Go ahead** / **Not yet**. Allowed, the agent carries the
//! plan out once its turn ends (`agents::runner::plan_allowed`); Not yet, it
//! stays a plan and Claude is told to change nothing.
//!
//! The log has the card's id, the tool's name and the outcome; never the
//! command, the file or the text.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use serde_json::{json, Value};

use super::server::{Endpoint, Run};
use super::Origin;

/// The tool's name on Yap's server.
pub const NAME: &str = "approve";
/// The card's event.
pub const EVENT: &str = "yap-claude-approve";
/// A card's end.
pub const EVENT_DONE: &str = "yap-claude-approve-done";
/// How much of a command, an edit or a file's text the card shows.
const DETAIL_MAX: usize = 2_000;
/// How much of it the summary line shows.
const SUMMARY_MAX: usize = 80;
/// Plan mode's tool for "here's the plan, may I go ahead?".
pub const EXIT_PLAN: &str = "ExitPlanMode";
/// How much of a plan its card shows (folded in the thread).
const PLAN_MAX: usize = 20_000;

/// **A permission prompt waiting for the person**, as the card shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    pub id: u64,
    /// The agent's id ([`super::Approval::run`]), else the run's own.
    pub run: String,
    pub conversation: Option<u64>,
    pub origin: Origin,
    /// Claude Code's tool ("Bash", "Edit", "mcp__github__create_issue").
    pub tool: String,
    /// What it would do, in plain words.
    pub summary: String,
    /// The command, the edit or the text, trimmed.
    pub detail: String,
    /// The run's label.
    pub label: String,
    /// When it was asked (unix seconds).
    pub asked_ts: u64,
}

/// How a prompt ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Allowed,
    Denied,
    TimedOut,
    /// The run ended (its handle dropped) before an answer.
    Ended,
    /// Claude Code cancelled the call (`notifications/cancelled`).
    Cancelled,
}

impl Outcome {
    pub fn word(self) -> &'static str {
        match self {
            Outcome::Allowed => "allowed",
            Outcome::Denied => "denied",
            Outcome::TimedOut => "timedOut",
            Outcome::Ended => "ended",
            Outcome::Cancelled => "cancelled",
        }
    }
}

struct Waiting {
    card: Pending,
    /// The live run's id (its handle's), whatever the card's `run` says.
    live: String,
    /// The JSON-RPC request's id, for a cancel.
    rpc: String,
    tx: mpsc::Sender<Outcome>,
}

/// **The prompts waiting**, one book per endpoint.
#[derive(Default)]
pub struct Book {
    waiting: Mutex<HashMap<u64, Waiting>>,
    next: AtomicU64,
}

impl Book {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<u64, Waiting>> {
        self.waiting.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The person's answer: `true` if that prompt was still waiting.
    pub fn answer(&self, id: u64, allow: bool) -> bool {
        let Some(w) = self.lock().remove(&id) else { return false };
        w.tx.send(if allow { Outcome::Allowed } else { Outcome::Denied }).is_ok()
    }

    /// Deny every prompt of a run that ended (each one's own thread says
    /// so to Claude Code and the UI); how many.
    pub fn end_run(&self, live: &str) -> usize {
        self.close(|w| w.live == live, Outcome::Ended)
    }

    /// Claude Code cancelled request `rpc` of run `live`.
    pub fn cancel(&self, live: &str, rpc: &str) -> usize {
        self.close(|w| w.live == live && w.rpc == rpc, Outcome::Cancelled)
    }

    fn close(&self, which: impl Fn(&Waiting) -> bool, outcome: Outcome) -> usize {
        let closed: Vec<Waiting> = {
            let mut map = self.lock();
            let ids: Vec<u64> = map.iter().filter(|(_, w)| which(w)).map(|(id, _)| *id).collect();
            ids.into_iter().filter_map(|id| map.remove(&id)).collect()
        };
        for w in &closed {
            let _ = w.tx.send(outcome);
        }
        closed.len()
    }

    /// The cards waiting, oldest first.
    pub fn pending(&self) -> Vec<Pending> {
        let mut out: Vec<Pending> = self.lock().values().map(|w| w.card.clone()).collect();
        out.sort_by_key(|p| p.id);
        out
    }
}

// ---- answering in test mode ----

/// Test mode (`e2e_claude_approve_auto`): 0 asks as in the app, 1 allows,
/// 2 denies, at once.
#[cfg(debug_assertions)]
static AUTO: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Test mode: answer every permission prompt at once (`Some(true)` allow,
/// `Some(false)` deny), or ask as in the app (`None`). The cards and their
/// ends still go out, so a spec sees them.
#[cfg(debug_assertions)]
pub fn set_auto_answer(answer: Option<bool>) {
    AUTO.store(
        match answer {
            None => 0,
            Some(true) => 1,
            Some(false) => 2,
        },
        Ordering::SeqCst,
    );
}

fn auto_answer() -> Option<bool> {
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() {
        return match AUTO.load(Ordering::SeqCst) {
            1 => Some(true),
            2 => Some(false),
            _ => None,
        };
    }
    None
}

// ---- the tool ----

/// The tool's definition: Claude Code needs it listed to use it.
pub fn definition() -> Value {
    json!({
        "name": NAME,
        "title": "Ask the user's permission in Yap",
        "description": "Claude Code's permission prompt for this run, answered by the user in Yap. Claude Code calls it by itself; never call it yourself.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "tool_name": { "type": "string", "description": "The tool that needs permission." },
                "input": { "type": "object", "description": "The tool's input." },
                "tool_use_id": { "type": "string" }
            },
            "required": ["tool_name", "input"]
        },
        "annotations": { "title": "Ask the user's permission in Yap", "openWorldHint": false }
    })
}

/// The answer Claude Code reads for `tool`.
fn decision(tool: &str, outcome: Outcome, input: &Value, wait: Duration) -> String {
    let deny = |message: String| json!({ "behavior": "deny", "message": message }).to_string();
    match outcome {
        Outcome::Allowed => json!({ "behavior": "allow", "updatedInput": input }).to_string(),
        // A plan's Not yet: still a plan, nothing changed.
        Outcome::Denied if tool == EXIT_PLAN => deny("Not yet: the user hasn't approved the plan. Stay in plan mode and change nothing: end your turn with the plan as it is, and they'll say when to go ahead.".into()),
        Outcome::Denied => deny("The user said no to this in Yap. Don't try to do the same thing another way: carry on without it, and say in your final summary what you couldn't do.".into()),
        Outcome::TimedOut => deny(format!("Nobody answered this permission request in Yap within {}, so it wasn't allowed. Carry on without it if you can, and say in your final summary that it needed the user's permission.", span(wait))),
        Outcome::Ended => deny("The agent was stopped in Yap before the user answered.".into()),
        Outcome::Cancelled => deny("This permission request was cancelled.".into()),
    }
}

/// "10 minutes", "1 minute", "30 seconds".
fn span(d: Duration) -> String {
    let secs = d.as_secs().max(1);
    let (n, unit) = if secs.is_multiple_of(60) { (secs / 60, "minute") } else { (secs, "second") };
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// **Ask the person**, and wait (on this request's own thread): the text
/// for Claude Code, and whether it's a tool error (a call that isn't a
/// permission prompt).
pub fn ask(ep: &Endpoint, run: &Run, args: &serde_json::Map<String, Value>, rpc: &str) -> (String, bool) {
    let Some(tool) = args.get("tool_name").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()) else {
        return ("approve is Claude Code's permission prompt: it takes tool_name and input.".into(), true);
    };
    let input = args.get("input").cloned().unwrap_or_else(|| json!({}));
    // A question for the person isn't a permission: Yap has no card for
    // one yet, so Claude decides and says so (the report asks the person).
    if tool == "AskUserQuestion" {
        let text = json!({ "behavior": "deny", "message": "Yap can't show questions while an agent runs. Make the most sensible choice yourself, carry on, and say in your final summary what you chose and what you'd have asked." }).to_string();
        return (text, false);
    }
    let (summary, detail) = describe(tool, &input, run.spec.folder.as_deref());
    let id = ep.approvals.next.fetch_add(1, Ordering::Relaxed) + 1;
    let approval = run.spec.approval.clone().unwrap_or_default();
    let card = Pending {
        id,
        run: if approval.run.is_empty() { run.id.clone() } else { approval.run.clone() },
        conversation: approval.conversation,
        origin: approval.origin,
        tool: tool.to_string(),
        summary,
        detail,
        label: run.spec.label.clone(),
        asked_ts: now_secs(),
    };
    let (tx, rx) = mpsc::channel();
    ep.approvals.lock().insert(id, Waiting { card: card.clone(), live: run.id.clone(), rpc: rpc.to_string(), tx });
    (ep.emit)(EVENT, serde_json::to_value(&card).unwrap_or(Value::Null));
    tracing::info!(id, run = %run.id, tool = %tool_kind(tool), "Claude Code: a permission prompt waits for the user");
    if let Some(allow) = auto_answer() {
        ep.approvals.answer(id, allow);
    }
    let outcome = match rx.recv_timeout(ep.wait) {
        Ok(o) => o,
        Err(_) => {
            // Taken out unless an answer got there first.
            let gone = ep.approvals.lock().remove(&id).is_none();
            if gone {
                rx.try_recv().unwrap_or(Outcome::TimedOut)
            } else {
                Outcome::TimedOut
            }
        }
    };
    // A plan allowed: the agent carries it out once this turn ends.
    if tool == EXIT_PLAN && outcome == Outcome::Allowed {
        crate::agents::runner::plan_allowed(&card.run);
    }
    (ep.emit)(EVENT_DONE, json!({ "id": id, "run": card.run, "outcome": outcome.word() }));
    tracing::info!(id, outcome = outcome.word(), "Claude Code: a permission prompt answered");
    (decision(tool, outcome, &input, ep.wait), false)
}

/// A tool's name for the log: Claude Code's own, or "mcp" for another
/// server's (whose name could say something about the person's setup).
fn tool_kind(tool: &str) -> &str {
    if tool.starts_with("mcp__") {
        "mcp"
    } else {
        tool
    }
}

// ---- in plain words ----

/// `s` on one line, at most `max` characters.
fn one_line(s: &str, max: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    clip(&flat, max)
}

/// At most `max` characters, "…" when cut.
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

/// A file as the card names it: relative to the run's folder when it's in
/// it.
fn shown(path: &str, folder: Option<&Path>) -> String {
    let p = Path::new(path);
    if let Some(rel) = folder.and_then(|f| super::backup::relative_to(p, f)) {
        return rel;
    }
    path.to_string()
}

/// **What a tool call would do**, as the card says it: a summary line and
/// the details (trimmed). Pure.
pub fn describe(tool: &str, input: &Value, folder: Option<&Path>) -> (String, String) {
    let s = |k: &str| input.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let file = |k: &str| shown(&s(k), folder);
    match tool {
        "Bash" | "PowerShell" => {
            // The line as the agent's steps say a command (its `cd` into the
            // run's folder gone, its paths relative; never the model's
            // description: a prompt says what would run), the command whole
            // below it.
            let cmd = s("command");
            let line = match folder {
                Some(f) => crate::agents::steps::command::relative(&crate::agents::steps::command::strip_cd(&cmd, f).0, f),
                None => cmd.clone(),
            };
            (format!("Run a command: {}", one_line(&line, SUMMARY_MAX)), clip(&cmd, DETAIL_MAX))
        }
        "Edit" => (
            format!("Edit file: {}", file("file_path")),
            clip(&format!("Replace:\n{}\n\nWith:\n{}", s("old_string"), s("new_string")), DETAIL_MAX),
        ),
        "MultiEdit" => {
            let edits = input.get("edits").and_then(Value::as_array).cloned().unwrap_or_default();
            let first = edits.first().map(|e| {
                format!(
                    "Replace:\n{}\n\nWith:\n{}",
                    e.get("old_string").and_then(Value::as_str).unwrap_or(""),
                    e.get("new_string").and_then(Value::as_str).unwrap_or("")
                )
            });
            let head = format!("{} change{}", edits.len(), if edits.len() == 1 { "" } else { "s" });
            (
                format!("Edit file: {}", file("file_path")),
                clip(&first.map_or(head.clone(), |f| format!("{head}. The first:\n{f}")), DETAIL_MAX),
            )
        }
        "Write" => (format!("Write file: {}", file("file_path")), clip(&s("content"), DETAIL_MAX)),
        "NotebookEdit" => (format!("Edit notebook: {}", file("notebook_path")), clip(&s("new_source"), DETAIL_MAX)),
        "Read" => (format!("Read file: {}", file("file_path")), String::new()),
        "Glob" | "Grep" => {
            let where_ = s("path");
            (
                format!("Search files: {}", one_line(&s("pattern"), SUMMARY_MAX)),
                if where_.is_empty() { String::new() } else { format!("In {}", shown(&where_, folder)) },
            )
        }
        "WebFetch" => {
            let url = s("url");
            let site = url::Url::parse(&url).ok().and_then(|u| u.host_str().map(str::to_string)).unwrap_or_else(|| one_line(&url, SUMMARY_MAX));
            (format!("Open a web page: {site}"), clip(&url, DETAIL_MAX))
        }
        "WebSearch" => (format!("Search the web: {}", one_line(&s("query"), SUMMARY_MAX)), String::new()),
        "Task" | "Agent" => (
            format!("Start a helper agent: {}", one_line(&s("description"), SUMMARY_MAX)),
            clip(&s("prompt"), DETAIL_MAX),
        ),
        "Skill" => (format!("Use a skill: {}", one_line(&s("skill"), SUMMARY_MAX)), String::new()),
        // Plan mode's "may I go ahead?": the plan itself, as Claude wrote it.
        EXIT_PLAN => ("The plan is ready".into(), clip(&s("plan"), PLAN_MAX)),
        t if t.starts_with("mcp__") => {
            let mut parts = t.splitn(3, "__").skip(1);
            let server = parts.next().unwrap_or("");
            let name = parts.next().unwrap_or("");
            (
                format!("Use {} from {}", if name.is_empty() { t } else { name }, if server.is_empty() { "an MCP server" } else { server }),
                clip(&pretty(input), DETAIL_MAX),
            )
        }
        other => (format!("Use {other}"), clip(&pretty(input), DETAIL_MAX)),
    }
}

fn pretty(v: &Value) -> String {
    match v {
        Value::Object(m) if m.is_empty() => String::new(),
        _ => serde_json::to_string_pretty(v).unwrap_or_default(),
    }
}

/// For [`super::rpc`]: a cancel's prompts closed.
pub fn cancelled(ep: &Endpoint, live: &str, rpc: &str) {
    let n = ep.approvals.cancel(live, rpc);
    if n > 0 {
        tracing::info!(run = %live, prompts = n, "Claude Code: a permission prompt cancelled");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wait_is_said_in_words() {
        assert_eq!(span(super::super::APPROVE_WAIT), "10 minutes");
        assert_eq!(span(Duration::from_secs(60)), "1 minute");
        assert_eq!(span(Duration::from_secs(90)), "90 seconds");
        assert_eq!(span(Duration::from_millis(300)), "1 second");
        let input = json!({ "command": "ls" });
        let allow: Value = serde_json::from_str(&decision("Bash", Outcome::Allowed, &input, super::super::APPROVE_WAIT)).unwrap();
        assert_eq!(allow, json!({ "behavior": "allow", "updatedInput": { "command": "ls" } }));
        let late: Value = serde_json::from_str(&decision("Bash", Outcome::TimedOut, &input, super::super::APPROVE_WAIT)).unwrap();
        assert!(late["message"].as_str().unwrap().contains("within 10 minutes"));
        let no: Value = serde_json::from_str(&decision("Bash", Outcome::Denied, &input, super::super::APPROVE_WAIT)).unwrap();
        assert!(no["message"].as_str().unwrap().starts_with("The user said no to this in Yap."));
        assert_eq!(tool_kind("mcp__tracker__create_issue"), "mcp", "another server's name stays out of the log");
        assert_eq!(tool_kind("Bash"), "Bash");
    }

    #[test]
    fn a_plan_asks_to_go_ahead_and_not_yet_keeps_it_a_plan() {
        let plan = "## Watering schedule\n\n1. Add a `watering` section to notes.md\n2. Run the tests";
        let input = json!({ "plan": plan });
        let (summary, detail) = describe(EXIT_PLAN, &input, None);
        assert_eq!(summary, "The plan is ready");
        assert_eq!(detail, plan, "the plan itself, as markdown");
        // A long plan is kept far past a command's 2,000 characters.
        let long = "Step. ".repeat(1_000);
        assert!(describe(EXIT_PLAN, &json!({ "plan": long }), None).1.chars().count() > DETAIL_MAX);
        // Go ahead: allowed as asked. Not yet: still a plan, nothing changed.
        let allow: Value = serde_json::from_str(&decision(EXIT_PLAN, Outcome::Allowed, &input, super::super::APPROVE_WAIT)).unwrap();
        assert_eq!(allow, json!({ "behavior": "allow", "updatedInput": { "plan": plan } }));
        let not_yet: Value = serde_json::from_str(&decision(EXIT_PLAN, Outcome::Denied, &input, super::super::APPROVE_WAIT)).unwrap();
        assert_eq!(not_yet["behavior"], "deny");
        assert!(not_yet["message"].as_str().unwrap().contains("Stay in plan mode and change nothing"));
    }
}

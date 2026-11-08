//! **Connectors in plan chats** (Connectors wave 7, behind
//! `connectors_preview`): a chat on the person's Claude plan runs their own
//! Claude Code lean, with `--strict-mcp-config` and Yap's endpoint only, and
//! Yap holds the connectors' tokens — so the endpoint is the proxy. For a
//! Chat run registered with them ([`super::RunSpec::connectors`]):
//!
//! - **The list** ([`Listed`]): every tool the turn may use that isn't Off
//!   (`chat/tools/connectors.rs` `offered_for_plan`: the preview on, the
//!   connector on, `where.plan` and `where.chat` — or `where.voice` asked
//!   out loud; a phone's question only `where.phone`'s reads set to Always
//!   allow (wave 6) —, on in the chat (its + menu's Connectors ▸), during a
//!   meeting only those allowed then), by Yap's own names (`<slug>__<tool>`, so Claude Code
//!   shows `mcp__yap__notion__search`; one too long for a model's 64
//!   characters with that in front is cut with a tag of its name), with
//!   `readOnlyHint` for the reads and Yap's words for what each does
//!   ("(Yap asks the user first.)"). Every one of them goes in the run's
//!   `--allowedTools`, so `dontAsk` never denies them: **Yap decides**.
//!   Claude Code's own tool search defers them past its budget.
//! - **A call** ([`call`]): wave 4's own `connectors::Turn::execute` for
//!   the turn it belongs to (`turn.rs`) — the consent (`consent.rs`: Always
//!   allow runs; Ask first, a tool that can delete, a write after outside
//!   content or a risky name asks; Off is refused), its card in the plan
//!   turn's thread (`ConnectorConfirm.svelte`, through the turn's stream)
//!   — asked out loud, also Yap's spoken line, the Yap bar's Allow once /
//!   Don't and a spoken yes or no (wave 6: the Voice Assistant's sink hears
//!   the card, `chat/assistant/spoken_ok.rs`) —
//!   while **the HTTP call is held** for the person's answer, as `approve`
//!   holds an agent's prompt (the card's two minutes and the call, within
//!   [`CALL_LIMIT`], inside the run's 11-minute tool timeout); a no, no
//!   answer or nobody watching (an evaluation) the tool's refusal in words
//!   the model can read. Its chip (`displayText`, `claude_plan::map`) and
//!   activity line ("Asking Home lights · List lights") are wave 4's.
//! - **The result** as MCP content: the tool loop's result JSON first (its
//!   chip), then each picture (at most two) as image content after the line
//!   saying what it is (`eyes::mcp_result`): a Claude reads images.
//! - **Outside content**: a call waits for its own tool call to show in the
//!   turn's events first (`turn::Slot::seen`), so a web read before it is
//!   counted — not seen in time, it's after outside content, on doubt (a
//!   write asks). A server's answer marks the turn (`Slot::set_connector`):
//!   a later write asks, Always allow or not; Yap's own action tools are
//!   refused ("not after a connector's result": `claude_live/agents.rs`), and
//!   the page tools' typed-words check holds (the web side's `mark_seen`).
//! - **Receipts**: the turn's part of Yap (`turn::Context::feature`: Chat,
//!   or the Voice Assistant's) around every request (`net::within`), so the
//!   proxy's calls are counted as Chat's are, with the connectors' purposes.
//! - **Stop**: the turn ending (■ Stop, a newer question) or Claude Code's
//!   own `notifications/cancelled` for the request ([`cancelled`]) drops the
//!   call — a card still waiting goes, and a call already sent is cancelled
//!   on the server (`connectors/call.rs`).
//!
//! **Who gets them**: the plan's Chat runs only — never a Code chat's
//! session, an agent's run (`Tools::Agent`: Yap Code agents keep the
//! person's own Claude Code setup) nor a one-shot scope call (cleanup,
//! Commands, notes: they never register with any). The session's list is
//! read once (`tools/list`), so its hash is in the session's fingerprint
//! (`chat/turn/plan.rs`): a connector turned on or off, a tool's setting
//! changed or a meeting starting starts a new session.
//!
//! The log has the run's id, the connector's id, the tool's name, outcomes
//! and counts — never arguments, a result or a server's words.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::agents::ToTurn;
use super::server::Run;
use super::turn::{Call, Slot, SEEN_WAIT};
use crate::chat::pictures::Look;
use crate::chat::stream::Live;
use crate::chat::tools::connectors::{self as tools, Offered, Turn};
use crate::integrations::connectors::cache::Kind;

/// The longest name a model takes for a tool (Anthropic's, OpenAI's): what
/// Claude Code sends is `mcp__yap__<name>`.
const NAME_MAX: usize = 64;

/// **How long one call may take** here, its card's wait included: inside
/// the run's tool timeout (`TOOL_TIMEOUT_MS`), so Yap answers before Claude
/// Code gives up on it.
pub const CALL_LIMIT: Duration = Duration::from_millis(super::TOOL_TIMEOUT_MS - 30_000);

/// The room for a tool's own name after Claude Code's `mcp__yap__`.
fn room() -> usize {
    NAME_MAX - format!("mcp__{}__", super::SERVER).len()
}

/// **What a session lists**: the turn's connector tools when it began, by
/// the names Claude Code knows them by.
#[derive(Debug, Clone)]
pub struct Listed {
    /// The tools, as `chat/tools/connectors.rs` offered them.
    pub turn: Arc<Turn>,
    /// Each one's name here and its wire name (`names.rs`), in its order.
    names: Vec<(String, String)>,
}

impl Listed {
    /// The list for `turn`'s tools. Pure.
    pub fn of(turn: Arc<Turn>) -> Listed {
        let names = names_for(turn.tools(), room());
        Listed { turn, names }
    }

    /// The names here, in order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(|(n, _)| n.as_str())
    }

    /// The wire name `name` stands for.
    pub fn wire(&self, name: &str) -> Option<&str> {
        self.names.iter().find(|(n, _)| n == name).map(|(_, w)| w.as_str())
    }

    /// **The tools as an MCP server lists them**: Yap's name, the connector
    /// and the tool's title, what it does in Yap's words (wave 4's), its
    /// schema made safe, `readOnlyHint` for a read. Pure.
    pub fn definitions(&self) -> Vec<Value> {
        self.names
            .iter()
            .filter_map(|(name, wire)| self.turn.tools().iter().find(|t| t.wire == *wire).map(|t| definition(name, t)))
            .collect()
    }

    /// **The session's fingerprint of it**: what Claude Code was told (the
    /// names, the words, the schemas, the hints), so any change to one —
    /// a connector on or off, a tool's setting — is another session. Pure.
    pub fn hash(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        Value::Array(self.definitions()).to_string().hash(&mut h);
        h.finish()
    }
}

/// The fingerprint of a session's connectors (`None`: none listed: 0).
pub fn hash_of(listed: Option<&Listed>) -> u64 {
    listed.map_or(0, Listed::hash)
}

/// One tool's MCP definition. Pure.
fn definition(name: &str, t: &Offered) -> Value {
    json!({
        "name": name,
        "title": t.label(),
        "description": t.about(),
        "inputSchema": t.schema,
        "annotations": {
            "title": t.label(),
            "readOnlyHint": t.kind == Kind::Reads,
            "destructiveHint": t.kind == Kind::Deletes,
            "openWorldHint": true,
        },
    })
}

/// Four hex characters of `tool`'s hash: what tells cut names apart.
fn tag(tool: &str) -> String {
    let h = Sha256::digest(tool.as_bytes());
    format!("{:02x}{:02x}", h[0], h[1])
}

/// **The names here** of `tools`: each one's wire name when it fits in
/// `room`, else cut with a tag of its own name; never two alike. (Wire names
/// are ASCII: `names.rs`.) Pure.
fn names_for(tools: &[Offered], room: usize) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for t in tools {
        let mut name = if t.wire.len() <= room {
            t.wire.clone()
        } else {
            let mut head = t.wire.clone();
            head.truncate(room - 5);
            format!("{head}_{}", tag(&t.tool))
        };
        let mut n = 2;
        while out.iter().any(|(m, _)| *m == name) {
            let suffix = n.to_string();
            name.truncate(room.min(name.len()).saturating_sub(suffix.len()));
            name.push_str(&suffix);
            n += 1;
        }
        out.push((name, t.wire.clone()));
    }
    out
}

/// Whether `name` is one of the run's connector tools.
pub(super) fn owns(run: &Run, name: &str) -> bool {
    run.spec.connectors.as_ref().is_some_and(|l| l.wire(name).is_some())
}

/// The tool loop's failure shape (`tools::tool_error`).
fn failed(error: &str, chip: &str) -> Value {
    json!({ "success": false, "error": error, "data": Value::Null, "displayText": chip })
}

// ---- Claude Code's own cancel ----

/// The calls under way, by (run, request id), each with its stop.
type Calls = HashMap<(String, String), Arc<tokio::sync::Notify>>;

/// The calls under way: Claude Code's `notifications/cancelled` ends one
/// ([`cancelled`]).
static CALLS: Mutex<Option<Calls>> = Mutex::new(None);

/// A call under way, listed until it ends.
struct Under {
    key: (String, String),
    stop: Arc<tokio::sync::Notify>,
}

impl Under {
    fn new(run: &str, rpc: &str) -> Under {
        let key = (run.to_string(), rpc.to_string());
        let stop = Arc::new(tokio::sync::Notify::new());
        CALLS.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(HashMap::new).insert(key.clone(), Arc::clone(&stop));
        Under { key, stop }
    }
}

impl Drop for Under {
    fn drop(&mut self) {
        if let Some(m) = CALLS.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
            m.remove(&self.key);
        }
    }
}

/// **Claude Code cancelled request `rpc`** of run `run` (its interrupt, its
/// own timeout): the call ends — a card still waiting goes, a call already
/// sent is cancelled on the server. Nothing when there's no such call.
pub(super) fn cancelled(run: &str, rpc: &str) {
    let stop = CALLS.lock().unwrap_or_else(|p| p.into_inner()).as_ref().and_then(|m| m.get(&(run.to_string(), rpc.to_string())).cloned());
    if let Some(stop) = stop {
        tracing::info!(run, "Claude Code: a connector's call was cancelled by Claude Code");
        stop.notify_one();
    }
}

// ---- a call ----

/// **One call** of a connector's tool `name` (Yap's name for it) on `run`,
/// request `rpc`: its MCP result. Runs on this request's own thread (a card
/// waits here for the person's answer); the turn ending, Claude Code's
/// cancel or [`CALL_LIMIT`] ends it.
pub(super) fn call(run: &Run, name: &str, args: &Map<String, Value>, rpc: &str) -> Value {
    let wire = run.spec.connectors.as_ref().and_then(|l| l.wire(name)).unwrap_or(name).to_string();
    let Some(turn) = run.turn.call() else {
        tracing::info!(run = %run.id, "Claude Code: a connector's tool called outside a chat turn; nothing done");
        return super::eyes::mcp_result(&failed("Nothing was done: this isn't part of a chat turn. Answer the user.", "Didn't use a connector: not part of a chat turn"), &[]);
    };
    let under = Under::new(&run.id, rpc);
    let started = Instant::now();
    let args = Value::Object(args.clone());
    let events = turn.events.clone();
    let feature = turn.ctx.feature.unwrap_or(crate::privacy::net::Feature::Chat);
    // Every request it makes is the turn's in the network receipts.
    let ran = tauri::async_runtime::block_on(crate::privacy::net::within(feature, async {
        tokio::select! {
            ran = run_for(&run.turn, &turn, name, &wire, &args, SEEN_WAIT) => Ok(ran),
            () = events.closed() => Err(Ended::Turn),
            () = under.stop.notified() => Err(Ended::Cancelled),
            () = tokio::time::sleep(CALL_LIMIT) => Err(Ended::TooLong),
        }
    }));
    drop(under);
    let (result, looks) = match ran {
        Ok(r) => r,
        Err(why) => {
            tracing::info!(run = %run.id, tool = name, why = why.word(), "Claude Code: a connector's call ended before it did");
            return super::eyes::mcp_result(&why.result(), &[]);
        }
    };
    let out = super::eyes::mcp_result(&result, &looks);
    let pictures = out["content"].as_array().map_or(0, |c| c.iter().filter(|b| b["type"] == "image").count());
    tracing::info!(run = %run.id, tool = name, ok = result["success"] == json!(true), pictures, ms = started.elapsed().as_millis() as u64, "Claude Code: a connector's tool answered");
    out
}

/// Why a call ended before it did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ended {
    /// The chat turn ended (■ Stop, a newer question).
    Turn,
    /// Claude Code cancelled the request.
    Cancelled,
    /// Past [`CALL_LIMIT`].
    TooLong,
}

impl Ended {
    fn word(self) -> &'static str {
        match self {
            Ended::Turn => "turn",
            Ended::Cancelled => "cancelled",
            Ended::TooLong => "too long",
        }
    }

    /// What the model reads. Pure.
    fn result(self) -> Value {
        match self {
            Ended::Turn => failed("Nothing more was done: the chat turn ended first. Don't try again unless the user asks.", "Didn't finish: the answer stopped"),
            Ended::Cancelled => failed("The call was cancelled before it finished.", "Didn't finish: cancelled"),
            Ended::TooLong => failed(
                "The connector took too long, so Yap stopped waiting. It may or may not have done it: say so, and don't try again unless the user asks.",
                "Didn't finish: the connector took too long",
            ),
        }
    }
}

/// [`call`] for `turn` of the run's `slot`: the tool `name` (its wire name
/// `wire`) run as Chat's tool loop runs it, after its own tool call showed
/// in the turn's events (at most `wait`; else outside content, on doubt), the
/// turn marked when a server answered.
pub(super) async fn run_for(slot: &Slot, turn: &Call, name: &str, wire: &str, args: &Value, wait: Duration) -> (Value, Vec<Look>) {
    // This turn's own (where it was asked, a meeting): never more than it may use.
    let Some(tools) = turn.ctx.connectors.as_ref().filter(|c| c.owns(wire)) else {
        tracing::info!(tool = name, "Claude Code: a connector's tool isn't this turn's; refused");
        return (
            failed(
                "Yap's connectors aren't offered for this question (it was asked from a phone, or the connector isn't allowed here now). Answer in words.",
                "Didn't use a connector: not for this question",
            ),
            Vec::new(),
        );
    };
    // Claude Code shows a tool call before it runs it: every web read, look
    // or connector result before this call is counted by then.
    let counted = slot.seen(turn.n, name, wait);
    if !counted {
        tracing::info!(tool = name, "Claude Code: a connector's call didn't show in the turn first; outside content, on doubt");
    }
    let outside = !counted || slot.web(turn.n);
    let own: Vec<&str> = turn.ctx.said.iter().map(String::as_str).collect();
    let sink = ToTurn(turn.events.clone());
    let live = turn.ctx.watched.then_some(Live { sink: &sink, beside: None });
    // The line, as the tool loop says it ("Asking Home lights · List lights").
    if let Some(l) = live {
        crate::chat::activity::tell(l.sink, tools.activity(wire, args));
    }
    let ctx = tools::Ctx { question: &turn.ctx.question, own: &own, outside, live };
    let ran = tools.execute(wire, args, ctx).await;
    // A server's answer: outside content for the rest of the turn.
    if ran.reached {
        slot.set_connector(turn.n);
        if let Some(side) = turn.web.as_ref() {
            side.lock().await.mark_seen();
        }
    }
    (ran.result, ran.looks)
}

#[cfg(test)]
mod tests;

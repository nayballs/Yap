//! **The endpoint Claude Code runs talk back to** (2026-11-07). Yap starts
//! the person's own Claude Code CLI for Chat on their Claude plan and for
//! Yap Code (`claude -p … --mcp-config <file> --settings <file>
//! --permission-prompt-tool mcp__yap__approve`); this is what those runs
//! reach: a loopback HTTP listener inside the running Yap. It needs no Local
//! API (`bridge.rs` can be off) and it reaches the live UI.
//!
//! - **The listener** ([`server`]): `127.0.0.1` on a port the system picks,
//!   started the first time a run is registered, then kept while Yap runs.
//!   Every request is refused unless it comes from this PC, names this
//!   PC in its `Host` (a web page on a name that resolves here can't reach
//!   it), carries no other site's `Origin`, and names a registered run with
//!   that run's key: 32 random bytes per run, sent as `Authorization: Bearer`
//!   (403 / 404 / 401). No CORS headers, ever. The key is only ever in
//!   memory and in the run's environment ([`RunHandle::env`], as
//!   `YAP_RUN_KEY`): its MCP config and hook settings say `${YAP_RUN_KEY}`,
//!   which Claude Code fills in itself, so the key is never in a file, on a
//!   command line or in a log. (The run's own commands inherit its
//!   environment; they already run with the person's own rights.)
//! - **A run** ([`register_run`] → [`RunHandle`]): its label, its tools
//!   ([`Tools::Chat`]: read only, plus Yap Code's when offered; [`Tools::Agent`]:
//!   `approve`), where its
//!   backups go, and who to ask ([`Approval`]). The handle gives the run's
//!   `--mcp-config` and `--settings` JSON and the environment they name;
//!   dropping it unregisters the run, and a permission prompt still waiting
//!   is denied.
//! - **MCP over HTTP** at `POST /mcp/<run>` ([`rpc`]): the Streamable HTTP
//!   transport's request/response subset, JSON answers only, no sessions,
//!   no SSE stream (`GET` is 405, which the transport allows). Claude Code
//!   2.1.296 (checked on this PC, 7 Nov 2026) first probes with
//!   `server/discover` (protocol 2026-07-28); Yap answers "method not found"
//!   and it falls back to `initialize` with 2025-11-25, which Yap echoes.
//!   Each request runs on a thread of its own, so a permission prompt
//!   waiting for the person never holds up a read.
//! - **Chat's tools** ([`reads`]): `yap.exe mcp`'s meetings, notes, tasks and
//!   folders (the same views, `mcp::run_tool`, read in process), Yap's memory
//!   (`search_memory`; never a Code chat's run, [`RunSpec::memory`]) and the
//!   calendar (`check_calendar`, offered while one is connected). Read only;
//!   never the dictation history.
//! - **Yap Code's tools** ([`agents`]), for a Chat run offered them
//!   ([`RunSpec::agents`]): `start_agent`, `agent_status`, `message_agent` and
//!   `stop_agent`, the tool loop's own code and rules, for the chat turn the
//!   call belongs to ([`turn`]: [`RunHandle::begin_turn`] gives a run its
//!   turn's question, the answer before it, the conversation, where it was
//!   asked, and its stream; the web flag; the order of Claude Code's events
//!   and its calls).
//! - **The screen and the pages beside the chat** ([`eyes`]), for a Chat run
//!   offered them ([`RunSpec::eyes`]): `look_at_screen`, the `browser_*`
//!   tools (going, reading, scrolling, finding, shots and zooms, tabs,
//!   waiting, batches; for a question typed in Chat clicking, typing, keys,
//!   hovering and fields too, each buy, send or sign-in on the person's
//!   card), `find_pictures`, `look_at_picture` and `show_picture` — the tool
//!   loop's own code and rules for the turn the call belongs to; what they
//!   take of the screen or a page goes back to the model as MCP image
//!   content, and what they show to the turn's stream (the screenshots with
//!   the question, a page's tab, a shot in the steps, a card, a picture).
//! - **The person's connectors** ([`connectors`], Connectors wave 7), for a
//!   Chat run on their plan offered them ([`RunSpec::connectors`]): every
//!   connector tool the turn may use that isn't Off, by Yap's names
//!   (`mcp__yap__notion__search`), all in `--allowedTools` so Yap's own
//!   settings decide — Always allow runs, Ask first holds the HTTP call
//!   while Yap's card asks in Chat, a no is a refusal the model reads —, a
//!   server's answer outside content for the rest of the turn.
//! - **`approve`** ([`approve`]), an agent's permission prompt tool: a card for
//!   the person (`yap-claude-approve`), Allow once / Deny
//!   ([`claude_approve_answer`]), no answer in 10 minutes a no. Claude Code
//!   keeps it out of the model's own tools. Test mode can answer every card
//!   at once (`e2e_claude_approve_auto`).
//! - **The backup hook** ([`backup`]): before Claude Code edits or writes a
//!   file, an http `PreToolUse` hook (`POST /hook/<run>/PreToolUse`) copies
//!   it into the run's backup folder once, with a `manifest.json`; a file
//!   that didn't exist yet is noted as created. [`backup::restore_all`]
//!   puts them back (Undo: an agent's turn, or all of them, each its own run).
//!
//! **The log** has ids, counts, tool names and outcomes, never a command,
//! a file's path or contents, a prompt or a key.

mod approve;
pub mod backup;
pub mod connectors;
pub mod eyes;
mod agents;
mod reads;
mod rpc;
mod server;
pub mod turn;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

pub use approve::{By, Pending, ASK, EVENT as APPROVE_EVENT, EVENT_DONE as APPROVE_DONE_EVENT, EXIT_PLAN};
#[cfg(test)]
pub use approve::{Choice, Question};
#[cfg(debug_assertions)]
pub use approve::set_auto_answer;

use server::Endpoint;

use crate::ai::claude_code::session::RUN_KEY_VAR;

/// The MCP server's name in a run's config: its tools are
/// `mcp__yap__<tool>`.
pub const SERVER: &str = "yap";
/// The permission prompt tool, for `--permission-prompt-tool`.
pub const PERMISSION_TOOL: &str = "mcp__yap__approve";
/// How long a permission prompt waits for the person.
pub const APPROVE_WAIT: Duration = Duration::from_secs(10 * 60);
/// The run's MCP server `timeout` (ms): Claude Code's hard limit per tool
/// call, and the floor of its idle limit (5 minutes for an HTTP server
/// otherwise), so a prompt can wait its 10 minutes.
pub const TOOL_TIMEOUT_MS: u64 = 11 * 60 * 1000;
/// The backup hook's `timeout` (s): a copy of at most 50 MB.
pub const HOOK_TIMEOUT_SECS: u64 = 60;
/// The tools the backup hook runs before.
pub const BACKUP_MATCHER: &str = "Edit|Write|MultiEdit|NotebookEdit";

/// Which tools a run gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Tools {
    /// Chat on the person's Claude plan: meetings, notes, tasks, folders,
    /// memory and the calendar, read only.
    Chat,
    /// An agent: `approve` (its permission prompts), for now.
    Agent,
}

/// How the person asked for the run (an agent's report is spoken when asked by
/// voice).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Origin {
    #[default]
    Typed,
    Voice,
}

/// Who a run's permission prompts are for: the card names the agent and the
/// conversation that started it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Approval {
    /// The agent's (or run's) id, as Yap's own UI knows it.
    pub run: String,
    pub conversation: Option<u64>,
    pub origin: Origin,
    /// **Nobody is asked** (an agent's turn in Bypass permissions, 8 Nov
    /// 2026): a prompt Claude Code still sends (its own safety checks, the
    /// person's own `ask` rules) is answered no at once, with no card
    /// (`approve::UNATTENDED`).
    pub unattended: bool,
}

/// **A run to register.**
#[derive(Debug, Clone)]
pub struct RunSpec {
    /// A short name for the card and the UI ("Shadow talents"); never logged.
    pub label: String,
    pub tools: Tools,
    /// Where the backup hook copies files before they're edited (an agent's
    /// `agents/<id>/backup/`). `None`: no hook in the run's settings.
    pub backup_dir: Option<PathBuf>,
    /// The run's working folder: approve's cards name files relative to it,
    /// and the hook resolves a relative path against it when Claude Code
    /// gives no `cwd`.
    pub folder: Option<PathBuf>,
    pub approval: Option<Approval>,
    /// A Chat run offered Yap Code's tools too ([`agents`]): agents on, Claude Code
    /// on this PC, not across the meetings. Listed for the whole session
    /// (its `tools/list` is read once), so a session made without them is
    /// started again to get them (`chat/turn/plan.rs`' fingerprint).
    pub agents: bool,
    /// A Chat run reads Yap's memory (`search_memory`): never a Code chat's
    /// (`agents/code_chat.rs`: Yap's memory is personal, the project keeps
    /// its own). Meetings, notes, tasks, folders and the calendar stay.
    pub memory: bool,
    /// A Chat run sees the screen and the pages beside the chat ([`eyes`]),
    /// as far as these gates say. Listed for the whole session, as the agent
    /// tools are; each turn says what it may use of them (`turn::Context`).
    pub eyes: eyes::Eyes,
    /// A plan chat's connector tools ([`connectors`]): listed for the whole
    /// session (its hash in the session's fingerprint, `chat/turn/plan.rs`),
    /// each call for the turn it belongs to. Never an agent's run.
    pub connectors: Option<connectors::Listed>,
    /// **An agent's run that reads Yap's own data** (12 Nov 2026): the
    /// session of a Code chat that works directly (`agents/direct.rs`) —
    /// meetings, notes, tasks, folders and the calendar, read only, as a
    /// plan Code chat's own session always had them ("build what we decided
    /// in yesterday's meeting"). Never Yap's memory (a Code chat's never),
    /// the screen, the pages or connectors; never an agent Chat sent out
    /// (Chat's model reads for its brief).
    pub reads: bool,
}

impl RunSpec {
    pub fn new(label: impl Into<String>, tools: Tools) -> RunSpec {
        RunSpec { label: label.into(), tools, backup_dir: None, folder: None, approval: None, agents: false, memory: true, eyes: eyes::Eyes::default(), connectors: None, reads: false }
    }

    /// The same, offered Yap Code's tools (`on`).
    pub fn with_agents(mut self, on: bool) -> RunSpec {
        self.agents = on;
        self
    }

    /// The same, with Yap's memory read (`on`) or not (a Code chat's).
    pub fn with_memory(mut self, on: bool) -> RunSpec {
        self.memory = on;
        self
    }

    /// The same, seeing the screen and the pages as `eyes` lets it.
    pub fn with_eyes(mut self, eyes: eyes::Eyes) -> RunSpec {
        self.eyes = eyes;
        self
    }

    /// The same, with the person's connectors' tools (`None`: none).
    pub fn with_connectors(mut self, listed: Option<connectors::Listed>) -> RunSpec {
        self.connectors = listed;
        self
    }
}

/// **A registered run.** Dropping it unregisters the run: its URL answers
/// 404 from then on, and a permission prompt still waiting is denied. Its
/// key reaches the run only through [`RunHandle::env`].
pub struct RunHandle {
    endpoint: Arc<Endpoint>,
    id: String,
    key: String,
    tools: Tools,
    /// Offered Yap Code's tools ([`RunSpec::agents`], a Chat run's).
    agents: bool,
    /// Reads Yap's memory ([`RunSpec::memory`]).
    memory: bool,
    /// Sees the screen and the pages ([`RunSpec::eyes`], a Chat run's).
    eyes: eyes::Eyes,
    /// Its connector tools' names ([`RunSpec::connectors`], a Chat run's).
    connectors: Vec<String>,
    /// Reads Yap's own data ([`RunSpec::reads`], an agent's run).
    reads: bool,
    has_backup: bool,
    /// The chat turn its calls belong to ([`RunHandle::begin_turn`]).
    turn: Arc<turn::Slot>,
}

impl std::fmt::Debug for RunHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunHandle").field("id", &self.id).field("port", &self.endpoint.port).finish_non_exhaustive()
    }
}

impl RunHandle {
    /// The run's id (in its URLs; not secret, not the key).
    #[cfg(test)]
    pub fn id(&self) -> &str {
        &self.id
    }

    #[cfg(test)]
    pub fn port(&self) -> u16 {
        self.endpoint.port
    }

    /// The run's MCP endpoint.
    pub fn mcp_url(&self) -> String {
        format!("http://127.0.0.1:{}/mcp/{}", self.endpoint.port, self.id)
    }

    /// `Authorization`'s value for this run, as it's sent (tests: what
    /// Claude Code makes of [`authorization`]).
    #[cfg(test)]
    fn bearer(&self) -> String {
        format!("Bearer {}", self.key)
    }

    /// **The run's environment**: its key under [`RUN_KEY_VAR`]
    /// (`Spec::env`; never `CLAUDE…` or `ANTHROPIC_…`, so the scrub keeps
    /// it).
    pub fn env(&self) -> Vec<(String, String)> {
        vec![(RUN_KEY_VAR.to_string(), self.key.clone())]
    }

    /// **`--mcp-config`** (with `--strict-mcp-config`): Yap's server only,
    /// its key named, never written ([`RunHandle::env`]).
    pub fn mcp_config(&self) -> Value {
        json!({
            "mcpServers": {
                SERVER: {
                    "type": "http",
                    "url": self.mcp_url(),
                    "headers": { "Authorization": authorization() },
                    "timeout": TOOL_TIMEOUT_MS,
                }
            }
        })
    }

    /// **`--settings`**: the backup hook for a run with a backup folder,
    /// else nothing (`{}`). An http hook's headers take only the variables
    /// it lists in `allowedEnvVars`.
    pub fn settings(&self) -> Value {
        if !self.has_backup {
            return json!({});
        }
        json!({
            "hooks": {
                "PreToolUse": [{
                    "matcher": BACKUP_MATCHER,
                    "hooks": [{
                        "type": "http",
                        "url": format!("http://127.0.0.1:{}/hook/{}/PreToolUse", self.endpoint.port, self.id),
                        "headers": { "Authorization": authorization() },
                        "allowedEnvVars": [RUN_KEY_VAR],
                        "timeout": HOOK_TIMEOUT_SECS,
                    }]
                }]
            }
        })
    }

    /// The run's own tools as `--allowedTools` names (`mcp__yap__list_meetings`
    /// …), so a Chat run in `dontAsk` mode can use them — its connectors'
    /// too, whose calls Yap's own settings decide ([`connectors`]). `approve`
    /// is never one: it's the permission prompt tool.
    pub fn allowed_tools(&self) -> Vec<String> {
        rpc::tool_names(self.tools, true, self.agents, self.memory, self.eyes, self.reads)
            .into_iter()
            .filter(|n| *n != approve::NAME)
            .map(str::to_string)
            .chain(self.connectors.iter().cloned())
            .map(|n| format!("mcp__{SERVER}__{n}"))
            .collect()
    }

    /// **A chat turn begins on this run** (`chat/turn/plan.rs`, before its
    /// question goes in, on every turn of a warm session): the calls Claude
    /// Code makes from now on belong to it — its question, the answer
    /// before it, the conversation, where it was asked, web content from its
    /// start ([`turn::Context`]) — and what they show comes out of the
    /// returned [`turn::Turn`]'s `events`. `web`: its web side, for the
    /// screen and page tools ([`eyes`]). Dropping it ends the turn.
    pub fn begin_turn(&self, ctx: turn::Context, web: Option<crate::web::Turn>) -> turn::Turn {
        self.turn.begin(ctx, web)
    }
}

/// `Authorization` as a run's files name it, `Bearer ${YAP_RUN_KEY}`:
/// Claude Code fills the key in from the run's environment
/// ([`RunHandle::env`]).
fn authorization() -> String {
    format!("Bearer ${{{RUN_KEY_VAR}}}")
}

impl Drop for RunHandle {
    fn drop(&mut self) {
        self.endpoint.unregister(&self.id);
    }
}

// ---- the endpoint Yap runs ----

static APP: OnceLock<AppHandle> = OnceLock::new();
static GLOBAL: Mutex<Option<Arc<Endpoint>>> = Mutex::new(None);

/// Keep the app's handle for the events (lib.rs setup). The listener itself
/// starts with the first run.
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// The app's events, as the endpoint sends them.
fn app_emit() -> server::Emit {
    Arc::new(|name: &str, payload: Value| {
        if let Some(app) = APP.get() {
            let _ = app.emit(name, payload);
        }
    })
}

/// Yap's endpoint, started if it isn't yet.
fn global() -> Result<Arc<Endpoint>, String> {
    let mut slot = GLOBAL.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(ep) = slot.as_ref() {
        return Ok(Arc::clone(ep));
    }
    let ep = Endpoint::start(Arc::new(reads::InApp), app_emit(), APPROVE_WAIT)?;
    *slot = Some(Arc::clone(&ep));
    Ok(ep)
}

/// The endpoint if it has started (nothing to answer otherwise).
fn started() -> Option<Arc<Endpoint>> {
    GLOBAL.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

/// **Register a run** with Yap's endpoint (started on first use).
pub fn register_run(spec: RunSpec) -> Result<RunHandle, String> {
    global()?.register(spec)
}

/// The permission prompts waiting for the person, oldest first (for the UI
/// a later wave builds).
pub fn pending() -> Vec<Pending> {
    started().map(|ep| ep.approvals.pending()).unwrap_or_default()
}

/// **A permission prompt's answer** (the card's Allow once / Deny): `true`
/// if that prompt was still waiting.
#[tauri::command]
pub fn claude_approve_answer(id: u64, allow: bool) -> bool {
    let answered = started().is_some_and(|ep| ep.approvals.answer(id, allow));
    tracing::info!(id, allow, answered, "Claude Code: the user answered a permission prompt");
    answered
}

/// **A question's answers** (`AskUserQuestion`'s card: `answers`, each
/// question's text → the option picked, the person's own words, or a list
/// of options): `true` if that question was still waiting and something of
/// it was answered. "Let it decide" is [`claude_approve_answer`]'s no.
#[tauri::command]
pub fn claude_approve_reply(id: u64, answers: serde_json::Value) -> bool {
    let answered = started().is_some_and(|ep| ep.approvals.reply(id, &answers, By::Pc));
    tracing::info!(id, answered, "Claude Code: the user answered a question");
    answered
}

/// **An answer from somewhere other than this PC's card** (Remote Control:
/// a paired phone, `link/code.rs`, once its own checks passed): `true` if
/// that prompt was still waiting — the first answer wins.
pub fn answer_by(id: u64, allow: bool, by: By) -> bool {
    started().is_some_and(|ep| ep.approvals.answer_by(id, allow, by))
}

/// Prompt `id`, while it's waiting.
pub fn waiting(id: u64) -> Option<Pending> {
    started().and_then(|ep| ep.approvals.waiting(id))
}

/// The permission prompts waiting, oldest first (a window opened later
/// shows the cards it missed).
#[tauri::command]
pub fn claude_approve_pending() -> Vec<Pending> {
    pending()
}

#[cfg(test)]
mod tests;

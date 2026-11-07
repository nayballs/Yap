//! **The endpoint Claude Code runs talk back to** (2026-11-07). Yap starts
//! the person's own Claude Code CLI for Chat on their Claude plan and for
//! Jobs (`claude -p … --mcp-config <json> --settings <json>
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
//!   (403 / 404 / 401). No CORS headers, ever. The key is only ever in the
//!   run's command-line JSON and in memory, never in a log or a file.
//! - **A run** ([`register_run`] → [`RunHandle`]): its label, its tools
//!   ([`Tools::Chat`]: read only, plus Jobs' when offered; [`Tools::Job`]:
//!   `approve`), where its
//!   backups go, and who to ask ([`Approval`]). The handle gives the run's
//!   `--mcp-config` and `--settings` JSON; dropping it (or [`RunHandle::end`])
//!   unregisters the run, and a permission prompt still waiting is denied.
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
//!   (`search_memory`) and the calendar (`check_calendar`, offered while one
//!   is connected). Read only; never the dictation history.
//! - **Jobs' tools** ([`jobs`]), for a Chat run offered them
//!   ([`RunSpec::jobs`]): `start_job`, `job_status`, `message_job` and
//!   `stop_job`, the tool loop's own code and rules, for the chat turn the
//!   call belongs to ([`turn`]: [`RunHandle::begin_turn`] gives a run its
//!   turn's question, the answer before it, the conversation, where it was
//!   asked, and its stream; the web flag; the order of Claude Code's events
//!   and its calls).
//! - **`approve`** ([`approve`]), a Job's permission prompt tool: a card for
//!   the person (`yap-claude-approve`), Allow once / Deny
//!   ([`claude_approve_answer`]), no answer in 10 minutes a no. Claude Code
//!   keeps it out of the model's own tools. Test mode can answer every card
//!   at once (`e2e_claude_approve_auto`).
//! - **The backup hook** ([`backup`]): before Claude Code edits or writes a
//!   file, an http `PreToolUse` hook (`POST /hook/<run>/PreToolUse`) copies
//!   it into the run's backup folder once, with a `manifest.json`; a file
//!   that didn't exist yet is noted as created. [`backup::restore`] puts
//!   them back (Undo job), [`backup::restore_only`] some of them.
//!
//! **The log** has ids, counts, tool names and outcomes, never a command,
//! a file's path or contents, a prompt or a key.

mod approve;
pub mod backup;
mod jobs;
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

pub use approve::Pending;
#[cfg(debug_assertions)]
pub use approve::set_auto_answer;

use server::Endpoint;

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
    /// A Job: `approve` (its permission prompts), for now.
    Job,
}

/// How the person asked for the run (a Job's report is spoken when asked by
/// voice).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Origin {
    #[default]
    Typed,
    Voice,
}

/// Who a run's permission prompts are for: the card names the job and the
/// conversation that started it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Approval {
    /// The job's (or run's) id, as Yap's own UI knows it.
    pub run: String,
    pub conversation: Option<u64>,
    pub origin: Origin,
}

/// **A run to register.**
#[derive(Debug, Clone)]
pub struct RunSpec {
    /// A short name for the card and the UI ("Shadow talents"); never logged.
    pub label: String,
    pub tools: Tools,
    /// Where the backup hook copies files before they're edited (a Job's
    /// `jobs/<id>/backup/`). `None`: no hook in the run's settings.
    pub backup_dir: Option<PathBuf>,
    /// The run's working folder: approve's cards name files relative to it,
    /// and the hook resolves a relative path against it when Claude Code
    /// gives no `cwd`.
    pub folder: Option<PathBuf>,
    pub approval: Option<Approval>,
    /// A Chat run offered Jobs' tools too ([`jobs`]): jobs on, Claude Code
    /// on this PC, not across the meetings. Listed for the whole session
    /// (its `tools/list` is read once), so a session made without them is
    /// started again to get them (`chat/turn/plan.rs`' fingerprint).
    pub jobs: bool,
}

impl RunSpec {
    pub fn new(label: impl Into<String>, tools: Tools) -> RunSpec {
        RunSpec { label: label.into(), tools, backup_dir: None, folder: None, approval: None, jobs: false }
    }

    /// The same, offered Jobs' tools (`on`).
    pub fn with_jobs(mut self, on: bool) -> RunSpec {
        self.jobs = on;
        self
    }
}

/// **A registered run.** Dropping it unregisters the run: its URL answers
/// 404 from then on, and a permission prompt still waiting is denied.
pub struct RunHandle {
    endpoint: Arc<Endpoint>,
    id: String,
    key: String,
    tools: Tools,
    /// Offered Jobs' tools ([`RunSpec::jobs`], a Chat run's).
    jobs: bool,
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
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn port(&self) -> u16 {
        self.endpoint.port
    }

    /// The run's MCP endpoint.
    pub fn mcp_url(&self) -> String {
        format!("http://127.0.0.1:{}/mcp/{}", self.endpoint.port, self.id)
    }

    /// `Authorization`'s value for this run.
    fn bearer(&self) -> String {
        format!("Bearer {}", self.key)
    }

    /// **`--mcp-config`** (with `--strict-mcp-config`): Yap's server only.
    pub fn mcp_config(&self) -> Value {
        json!({
            "mcpServers": {
                SERVER: {
                    "type": "http",
                    "url": self.mcp_url(),
                    "headers": { "Authorization": self.bearer() },
                    "timeout": TOOL_TIMEOUT_MS,
                }
            }
        })
    }

    pub fn mcp_config_json(&self) -> String {
        self.mcp_config().to_string()
    }

    /// **`--settings`**: the backup hook for a run with a backup folder,
    /// else nothing (`{}`).
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
                        "headers": { "Authorization": self.bearer() },
                        "timeout": HOOK_TIMEOUT_SECS,
                    }]
                }]
            }
        })
    }

    pub fn settings_json(&self) -> String {
        self.settings().to_string()
    }

    /// The run's own tools as `--allowedTools` names (`mcp__yap__list_meetings`
    /// …), so a Chat run in `dontAsk` mode can use them. `approve` is never
    /// one: it's the permission prompt tool.
    pub fn allowed_tools(&self) -> Vec<String> {
        rpc::tool_names(self.tools, true, self.jobs)
            .into_iter()
            .filter(|n| *n != approve::NAME)
            .map(|n| format!("mcp__{SERVER}__{n}"))
            .collect()
    }

    /// **A chat turn begins on this run** (`chat/turn/plan.rs`, before its
    /// question goes in, on every turn of a warm session): the calls Claude
    /// Code makes from now on belong to it — its question, the answer
    /// before it, the conversation, where it was asked, web content from its
    /// start ([`turn::Context`]) — and what they show comes out of the
    /// returned [`turn::Turn`]'s `events`. Dropping it ends the turn.
    pub fn begin_turn(&self, ctx: turn::Context) -> turn::Turn {
        self.turn.begin(ctx)
    }

    /// Unregister the run now (as dropping the handle does).
    pub fn end(self) {}
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

/// The permission prompts waiting, oldest first (a window opened later
/// shows the cards it missed).
#[tauri::command]
pub fn claude_approve_pending() -> Vec<Pending> {
    pending()
}

#[cfg(test)]
mod tests;

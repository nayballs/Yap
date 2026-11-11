//! Claude Code's `stream-json` output, read tolerantly: one [`Event`] per
//! line, any line Yap doesn't know kept whole as [`Event::Other`], a line
//! that isn't JSON at all counted as [`Event::NotJson`] (never its text),
//! and nothing ever fails the stream.
//!
//! Shapes are the Agent SDK's `SDKMessage` types (code.claude.com/docs/en/
//! agent-sdk/typescript, read 7 Nov 2026) and the headless guide
//! (code.claude.com/docs/en/headless), checked against Claude Code 2.1.296
//! signed out (`fixtures/2.1.296/real-*.jsonl`):
//!
//! - `system/init` comes before **every turn**, not once: the session's
//!   model, tools, MCP servers, `apiKeySource` and `capabilities` (2.1.296:
//!   `interrupt_receipt_v1`, `interrupt_cancel_queued_v1`, …).
//! - A turn that couldn't reach Claude still ends with a `result`:
//!   `subtype: "success"` but `is_error: true`, `terminal_reason:
//!   "api_error"`, and the words as `result` ("Not logged in · Please run
//!   /login" with no login, "Invalid API key · Fix external API key" with a
//!   bad key, 401). The `assistant` message before it carries `error:
//!   "authentication_failed"`. [`Classifier`] turns those into
//!   [`Problem::NotSignedIn`], never an answer.
//! - `control_response` answers a control request Yap sent on stdin
//!   (`{"type":"control_response","response":{"subtype":"success",
//!   "request_id":…,"response":{"still_queued":[]}}}` for an interrupt,
//!   seen on 2.1.296; the plan's usage for `get_usage`, `usage.rs`).
//! - **The context a call used**: `message_start`'s `usage` (the prompt:
//!   `input_tokens` and the cache's) and `message_delta`'s `output_tokens`,
//!   per model call ([`StreamKind::MessageStart`], [`StreamKind::MessageDelta`]),
//!   where a `result`'s `usage` is summed over the turn's calls; the
//!   `result`'s `modelUsage` says each model's context window.

use serde::Serialize;
use serde_json::Value;

/// One line of Claude Code's output, or what Yap made of it.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// `system/init`: before every turn.
    Init(Init),
    /// `system/status` (`requesting`, `compacting`, or none).
    Status(SystemStatus),
    /// `system/api_retry`: a request failed and Claude Code tries again.
    ApiRetry(ApiRetry),
    /// `system/permission_denied`: a tool call nobody allowed.
    PermissionDenied(PermissionDenied),
    /// `system/hook_started` / `hook_progress` / `hook_response`.
    Hook(Hook),
    /// `stream_event`: the model's words as they're written.
    Stream(Stream),
    /// `assistant`: a whole message (text, thinking, tool calls).
    Assistant(Assistant),
    /// `user`: tool results going back to the model (and replays).
    User(User),
    /// `result`: the turn is over.
    Result(TurnResult),
    /// `rate_limit_event`: where the plan's limits stand.
    RateLimit(RateLimit),
    /// `tool_progress`: a tool still running (every 30 s with `heartbeat`).
    ToolProgress(ToolProgress),
    /// `control_response`: Claude Code's answer to a control request.
    ControlResponse(ControlResponse),
    /// `control_request` from Claude Code (a permission request when the
    /// host answers them on stdin; Yap uses an MCP tool instead).
    ControlRequest(ControlRequest),
    /// Derived, never parsed: what went wrong, once per turn ([`Classifier`]).
    Problem(Problem),
    /// Derived: the process ended (its exit code when it had one). The
    /// last event a session sends.
    Exited(Option<i32>),
    /// A line that isn't a JSON object: only its length is kept.
    NotJson { chars: usize },
    /// Anything else, whole.
    Other(Value),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Init {
    pub session_id: String,
    pub model: String,
    pub cwd: String,
    pub tools: Vec<String>,
    pub mcp_servers: Vec<McpServer>,
    pub permission_mode: String,
    /// `"none"` (a claude.ai login, or no login), `"ANTHROPIC_API_KEY"`,
    /// `"apiKeyHelper"`, `"/login managed key"` (a Console login).
    pub api_key_source: String,
    pub version: String,
    pub capabilities: Vec<String>,
    pub effort: Option<String>,
}


#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct McpServer {
    pub name: String,
    /// `connected`, `pending`, `failed`, …
    pub status: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SystemStatus {
    pub status: Option<String>,
    pub permission_mode: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ApiRetry {
    pub attempt: u64,
    pub max_retries: u64,
    pub retry_delay_ms: u64,
    pub error_status: Option<u16>,
    /// `authentication_failed`, `oauth_org_not_allowed`, `account_on_hold`,
    /// `billing_error`, `rate_limit`, `overloaded`, `invalid_request`,
    /// `model_not_found`, `server_error`, `max_output_tokens`,
    /// `cloud_credential_error` or `unknown` (or a newer one).
    pub error: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PermissionDenied {
    pub tool_name: String,
    pub tool_use_id: String,
    pub message: String,
    pub decision_reason: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookPhase {
    Started,
    Progress,
    Response,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hook {
    pub phase: HookPhase,
    pub hook_id: String,
    pub hook_name: String,
    pub hook_event: String,
    /// On a response: `success`, `error` or `cancelled`.
    pub outcome: Option<String>,
    pub exit_code: Option<i64>,
    pub output: String,
}

/// A `stream_event` (the raw Messages API stream event inside it).
#[derive(Clone, Debug, PartialEq)]
pub struct Stream {
    /// Always `None` today: stream events come for the main conversation only.
    pub parent_tool_use_id: Option<String>,
    pub kind: StreamKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StreamKind {
    /// A model call began. `prompt`: what it read (`input_tokens` and the
    /// cache's, from its `usage`), when it said.
    MessageStart { id: Option<String>, model: Option<String>, prompt: Option<u64> },
    BlockStart { index: u64, block: BlockStart },
    /// `content_block_delta` / `text_delta`.
    Text { index: u64, text: String },
    /// `content_block_delta` / `thinking_delta`.
    Thinking { index: u64, thinking: String },
    /// `content_block_delta` / `input_json_delta`: a tool call's input, a
    /// piece at a time.
    ToolInput { index: u64, partial_json: String },
    BlockStop { index: u64 },
    /// `output_tokens`: what the call has written so far, when it said.
    MessageDelta { stop_reason: Option<String>, output_tokens: Option<u64> },
    MessageStop,
    /// `ping`, `signature_delta`, `citations_delta`, …
    Other(Value),
}

#[derive(Clone, Debug, PartialEq)]
pub enum BlockStart {
    Text,
    Thinking,
    ToolUse { id: String, name: String },
    Other(String),
}

/// A content block of an `assistant` or `user` message.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Text(String),
    Thinking(String),
    ToolUse { id: String, name: String, input: Value },
    /// A tool call's answer: its text, and the pictures it carries (a picture
    /// file read, a screenshot a tool took: at most [`RESULT_PICTURES`]).
    ToolResult { tool_use_id: String, text: String, is_error: bool, pictures: Vec<Picture> },
    /// An image (its type only: the data is never kept here).
    Image { media_type: String },
    Other(Value),
}

/// The pictures of one tool result kept, at most.
pub const RESULT_PICTURES: usize = 4;
/// A picture's most base64 characters (a bigger one is left out).
pub const PICTURE_CHARS: usize = 16 * 1024 * 1024;

/// **A picture in a tool's result**, as Claude Code hands it to the model:
/// its type and its base64 data. Never printed (its length only).
#[derive(Clone, Default, PartialEq)]
pub struct Picture {
    pub media_type: String,
    pub data: String,
}

impl std::fmt::Debug for Picture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Picture").field("media_type", &self.media_type).field("chars", &self.data.len()).finish()
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Assistant {
    pub message_id: Option<String>,
    pub model: Option<String>,
    pub blocks: Vec<Block>,
    /// `Some` for a subagent's message: the Agent (or Skill) call it runs under.
    pub parent_tool_use_id: Option<String>,
    /// Set when the message is Claude Code's own report of a failed request
    /// (`authentication_failed`, `rate_limit`, `billing_error`, …).
    pub error: Option<String>,
    pub stop_reason: Option<String>,
    /// Its `message.usage`: what this model call read (new, from the cache,
    /// into it) and wrote so far. Claude Code sends a message a block at a
    /// time, each with the call's usage as it stands (`None`: it didn't say).
    pub usage: Option<Usage>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct User {
    pub blocks: Vec<Block>,
    pub parent_tool_use_id: Option<String>,
    /// A user message Claude Code echoed back (`--replay-user-messages`).
    pub is_replay: bool,
    /// Claude Code's structured result of the tool (WebSearch's results,
    /// a file edit's patch, …), as it sent it.
    pub tool_use_result: Option<Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub web_search_requests: u64,
    pub web_fetch_requests: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Denial {
    pub tool_name: String,
    pub tool_use_id: String,
    pub tool_input: Value,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TurnResult {
    /// `success`, `error_max_turns`, `error_during_execution`,
    /// `error_max_budget_usd`, …
    pub subtype: String,
    pub is_error: bool,
    pub session_id: String,
    /// The answer (the `result` field): on an API error, Claude Code's own
    /// words about it.
    pub text: String,
    /// `completed`, `api_error`, `aborted_streaming`, `aborted_tools`,
    /// `max_turns`, `budget_exhausted`, … (`TerminalReason`).
    pub terminal_reason: Option<String>,
    pub stop_reason: Option<String>,
    pub api_error_status: Option<u16>,
    /// Claude Code's estimate of the API's price (`total_cost_usd`), even
    /// on a plan (where it's what the plan saved, not a charge) — **of its
    /// session so far, not of this turn**: every result of a process adds
    /// to it, and a process started with `--resume` goes on from what the
    /// session had cost before. A turn's own: [`TurnResult::cost_since`].
    pub cost_usd: f64,
    pub usage: Usage,
    pub num_turns: u64,
    pub duration_ms: u64,
    pub permission_denials: Vec<Denial>,
    /// The error subtypes' messages.
    pub errors: Vec<String>,
    /// `modelUsage`'s `contextWindow` by model: how big each model's
    /// context is, as Claude Code runs it.
    pub context_windows: Vec<(String, u64)>,
    /// `modelUsage`: what each model has used and cost **in the session so
    /// far** (the span `total_cost_usd` covers), while `usage` is this
    /// turn's own.
    pub model_usage: Vec<ModelUse>,
}

/// One model's line of a result's `modelUsage`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelUse {
    pub model: String,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub cost_usd: f64,
}

impl ModelUse {
    fn tokens(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_write
    }
}

/// Tokens weighed as the API prices them against each other: an output
/// token five times an input one, a cache read a tenth, a cache write (an
/// hour's, as Claude Code writes them) twice.
fn weighed(input: u64, output: u64, cache_read: u64, cache_write: u64) -> f64 {
    input as f64 + 5.0 * output as f64 + 0.1 * cache_read as f64 + 2.0 * cache_write as f64
}

impl TurnResult {
    /// Stopped by an interrupt, not finished.
    pub fn interrupted(&self) -> bool {
        self.terminal_reason.as_deref().is_some_and(|r| r.starts_with("aborted"))
    }

    /// Its `modelUsage` covers more than this turn: earlier turns of the
    /// session are in it (well beyond a helper model's few calls).
    pub fn covers_earlier_turns(&self) -> bool {
        let all: u64 = self.model_usage.iter().map(ModelUse::tokens).sum();
        let own = self.usage.input_tokens + self.usage.output_tokens + self.usage.cache_read_input_tokens + self.usage.cache_creation_input_tokens;
        all > own + own / 20 + 1_000
    }

    /// **What the session has cost since `before`**: this result's own
    /// cost, when `before` is the running total as its last result said it.
    ///
    /// `total_cost_usd` is Claude Code's running total for its session
    /// (checked on 2.1.296, 11 Nov 2026: a second result of one process said
    /// more than the first, and the first result of a process started with
    /// `--resume` listed the earlier turn's model, tokens and cost in its
    /// `modelUsage`). Read as one turn's cost, a 21-second follow-up showed
    /// the whole session's dollars, an agent's total counted every earlier
    /// turn again with each new one, and so did a plan chat's answers.
    ///
    /// - `before` known and the total went on from it: the difference;
    /// - `before` nothing (a new session), or the total started again
    ///   (nothing of earlier turns in `modelUsage`): all of it;
    /// - `before` not known (`None`: a resumed session nothing was kept
    ///   of) and earlier turns are in it: this turn's share of `model`'s
    ///   running cost, by its tokens.
    ///
    /// Pure.
    pub fn cost_since(&self, before: Option<f64>, model: &str) -> f64 {
        let total = self.cost_usd.max(0.0);
        let more = self.covers_earlier_turns();
        match before {
            Some(b) if b > 0.0 && more && total + 1e-9 >= b => total - b,
            Some(_) => total,
            None if more => self.own_share(model).unwrap_or(total),
            None => total,
        }
    }

    /// This turn's share of `model`'s running cost: its own tokens against
    /// all of that model's, weighed as the API prices them (the entry whose
    /// id starts as `model` does, else the busiest). Pure.
    fn own_share(&self, model: &str) -> Option<f64> {
        let named = |m: &&ModelUse| !model.is_empty() && (m.model.starts_with(model) || model.starts_with(m.model.as_str()));
        let entry = self.model_usage.iter().find(named).or_else(|| self.model_usage.iter().max_by_key(|m| m.tokens()))?;
        let all = weighed(entry.input, entry.output, entry.cache_read, entry.cache_write);
        let own = weighed(self.usage.input_tokens, self.usage.output_tokens, self.usage.cache_read_input_tokens, self.usage.cache_creation_input_tokens);
        (all > 0.0).then(|| entry.cost_usd.max(0.0) * (own / all).min(1.0))
    }

    /// `model`'s context window, as `modelUsage` says it.
    pub fn context_window(&self, model: &str) -> Option<u64> {
        self.context_windows.iter().find(|(m, _)| m == model).map(|(_, n)| *n).filter(|n| *n > 0)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RateLimit {
    /// `allowed`, `allowed_warning` or `rejected`.
    pub status: String,
    /// When the limit resets, Unix seconds (a value in milliseconds is
    /// turned into seconds).
    pub resets_at: Option<i64>,
    /// How much of it is used, as Claude Code says it (a fraction, 1.0 at
    /// the limit, in the recordings).
    pub utilization: Option<f64>,
    /// `credits_required`: a claude.ai plan whose included usage is spent.
    pub error_code: Option<String>,
    /// `rateLimitType`: the window it's about (`five_hour`, `seven_day`,
    /// `seven_day_opus`, `seven_day_sonnet`, `overage`, …).
    pub kind: Option<String>,
    /// `unifiedWindows`, when it comes: every window's numbers (read
    /// tolerantly: an object by window, or a list naming each).
    pub windows: Vec<RateWindow>,
}

/// One window of a `rate_limit_event`'s `unifiedWindows`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RateWindow {
    pub kind: String,
    pub utilization: Option<f64>,
    pub resets_at: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolProgress {
    pub tool_use_id: String,
    pub tool_name: String,
    pub parent_tool_use_id: Option<String>,
    pub elapsed_time_seconds: f64,
    pub heartbeat: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ControlResponse {
    pub request_id: String,
    /// `subtype: "success"`.
    pub ok: bool,
    /// The answer's body (`{"still_queued":[…]}` for an interrupt).
    pub response: Value,
    pub error: Option<String>,
    /// The whole `response` object (a body some versions may put there
    /// rather than under `response.response`).
    pub whole: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ControlRequest {
    pub request_id: String,
    pub subtype: String,
    pub request: Value,
}

/// What went wrong in a turn, in Yap's terms. Derived by [`Classifier`].
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Problem {
    /// Claude Code isn't signed in, its sign-in expired, or the API key
    /// was refused.
    NotSignedIn,
    /// **Its sign-in couldn't be renewed just now**: another Claude Code
    /// process on this PC was renewing it (or ended half way). Claude Code's
    /// own words: "Failed to refresh OAuth token: another Claude Code
    /// process is refreshing it or exited mid-refresh. This is usually
    /// transient; retry in a minute…". Never "isn't signed in" (11 Nov 2026,
    /// a Yap Lab run: the person was signed in, and was told to sign in).
    SignInBusy,
    /// The plan's limit (or the API's rate limit) was reached.
    #[serde(rename_all = "camelCase")]
    RateLimited {
        /// Unix seconds, when Claude Code said.
        resets_at: Option<i64>,
        /// The plan's included usage is spent; only bought credits help.
        credits_required: bool,
    },
    /// A billing problem on the account (`billing_error`).
    Billing,
    /// The account can't be used (`oauth_org_not_allowed`, `account_on_hold`).
    Account { reason: String },
    /// The run isn't paying the way Yap asked: a plan run that Claude Code
    /// bills to an API key (from its own settings), or the other way round.
    /// `source` is Claude Code's `apiKeySource`.
    PaysAnotherWay { source: String },
}

impl Problem {
    /// Plain words for the person. Chat's page knows them by their start
    /// ("Claude Code isn't signed in", "You've reached your Claude plan's
    /// limit", "Claude Code on this PC is set to pay"): the buttons that
    /// help (Sign in, Manage usage) go with them (`core/aiErrors.js`).
    pub fn message(&self) -> String {
        match self {
            Problem::NotSignedIn => "Claude Code isn't signed in on this PC. Sign in to Claude Code, then try again.".into(),
            Problem::SignInBusy => SIGN_IN_BUSY.into(),
            Problem::RateLimited { credits_required: true, .. } => {
                "You've reached your Claude plan's limit: its included usage is used up. It continues once you add usage credits.".into()
            }
            Problem::RateLimited { resets_at: Some(at), .. } => {
                format!("You've reached your Claude plan's limit (resets at {}).", resets_words(*at, &chrono::Local::now()))
            }
            Problem::RateLimited { .. } => "You've reached your Claude plan's limit.".into(),
            Problem::Billing => "Claude couldn't bill this account. Check your Claude billing, then try again.".into(),
            Problem::Account { .. } => "This Claude account can't be used with Claude Code right now.".into(),
            Problem::PaysAnotherWay { source } if matches!(source.as_str(), "ANTHROPIC_API_KEY" | "apiKeyHelper" | "/login managed key") => {
                "Claude Code on this PC is set to pay another way (an API key or a key helper in its own settings), so this wouldn't use your Claude plan. Yap stopped it before it asked anything.".into()
            }
            Problem::PaysAnotherWay { .. } => {
                "Claude Code on this PC is set to pay a different way than Yap asked, so Yap stopped it before it asked anything.".into()
            }
        }
    }
}

/// [`Problem::SignInBusy`]'s words (the page knows them by their start:
/// `core/aiErrors.js`).
pub const SIGN_IN_BUSY: &str = "Claude Code couldn't renew its sign-in just now: another Claude Code on this PC was doing the same. You're still signed in. Try again in a minute; if it keeps happening, close your other Claude Code windows.";

/// When a limit resets (Unix seconds), as the person reads it: "4:00 pm"
/// today, "Sun 9:00 am" within the week, else "20 Nov, 4:00 pm". Pure but
/// for `now`.
pub fn resets_words<Tz: chrono::TimeZone>(at: i64, now: &chrono::DateTime<Tz>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    let Some(when) = chrono::DateTime::from_timestamp(at, 0).map(|t| t.with_timezone(&now.timezone())) else {
        return "a later time".into();
    };
    let time = when.format("%-I:%M %P").to_string();
    match (when.date_naive() - now.date_naive()).num_days() {
        0 => time,
        1..=6 => format!("{} {time}", when.format("%a")),
        _ => format!("{}, {time}", when.format("%-d %b")),
    }
}

// ---- Parsing ----

/// One output line → its event. `None` for a blank line.
pub fn parse_line(line: &str) -> Option<Event> {
    let line = line.trim_start_matches('\u{feff}').trim();
    if line.is_empty() {
        return None;
    }
    match serde_json::from_str::<Value>(line) {
        Ok(v) if v.is_object() => Some(from_value(v)),
        _ => Some(Event::NotJson { chars: line.chars().count() }),
    }
}

/// A parsed JSON object → its event (anything unknown kept as `Other`).
pub fn from_value(v: Value) -> Event {
    let parsed = match str_of(&v, "type") {
        "system" => match str_of(&v, "subtype") {
            "init" => Some(Event::Init(init(&v))),
            "status" => Some(Event::Status(SystemStatus {
                status: opt_str(&v, "status"),
                permission_mode: opt_str(&v, "permissionMode"),
            })),
            "api_retry" => Some(Event::ApiRetry(ApiRetry {
                attempt: u64_of(&v, "attempt"),
                max_retries: u64_of(&v, "max_retries"),
                retry_delay_ms: u64_of(&v, "retry_delay_ms"),
                error_status: v.get("error_status").and_then(Value::as_u64).and_then(|s| u16::try_from(s).ok()),
                error: str_of(&v, "error").to_string(),
            })),
            "permission_denied" => Some(Event::PermissionDenied(PermissionDenied {
                tool_name: str_of(&v, "tool_name").to_string(),
                tool_use_id: str_of(&v, "tool_use_id").to_string(),
                message: str_of(&v, "message").to_string(),
                decision_reason: opt_str(&v, "decision_reason"),
            })),
            sub @ ("hook_started" | "hook_progress" | "hook_response") => Some(Event::Hook(Hook {
                phase: match sub {
                    "hook_started" => HookPhase::Started,
                    "hook_progress" => HookPhase::Progress,
                    _ => HookPhase::Response,
                },
                hook_id: str_of(&v, "hook_id").to_string(),
                hook_name: str_of(&v, "hook_name").to_string(),
                hook_event: str_of(&v, "hook_event").to_string(),
                outcome: opt_str(&v, "outcome"),
                exit_code: v.get("exit_code").and_then(Value::as_i64),
                output: str_of(&v, "output").to_string(),
            })),
            _ => None,
        },
        "stream_event" => v.get("event").filter(|e| e.is_object()).map(|e| {
            Event::Stream(Stream { parent_tool_use_id: opt_str(&v, "parent_tool_use_id"), kind: stream_kind(e) })
        }),
        "assistant" => v.get("message").filter(|m| m.is_object()).map(|m| {
            Event::Assistant(Assistant {
                message_id: opt_str(m, "id"),
                model: opt_str(m, "model"),
                blocks: blocks(m.get("content")),
                parent_tool_use_id: opt_str(&v, "parent_tool_use_id"),
                error: opt_str(&v, "error"),
                stop_reason: opt_str(m, "stop_reason"),
                usage: m.get("usage").filter(|u| u.is_object()).map(usage_of),
            })
        }),
        "user" => v.get("message").filter(|m| m.is_object()).map(|m| {
            Event::User(User {
                blocks: blocks(m.get("content")),
                parent_tool_use_id: opt_str(&v, "parent_tool_use_id"),
                is_replay: v.get("isReplay").and_then(Value::as_bool).unwrap_or(false),
                tool_use_result: v.get("tool_use_result").filter(|r| !r.is_null()).cloned(),
            })
        }),
        "result" => Some(Event::Result(result(&v))),
        "rate_limit_event" => v.get("rate_limit_info").filter(|i| i.is_object()).map(|i| {
            Event::RateLimit(RateLimit {
                status: str_of(i, "status").to_string(),
                resets_at: i.get("resetsAt").and_then(Value::as_f64).map(unix_seconds),
                utilization: i.get("utilization").and_then(Value::as_f64),
                error_code: opt_str(i, "errorCode"),
                kind: opt_str(i, "rateLimitType"),
                windows: rate_windows(i.get("unifiedWindows")),
            })
        }),
        "tool_progress" => Some(Event::ToolProgress(ToolProgress {
            tool_use_id: str_of(&v, "tool_use_id").to_string(),
            tool_name: str_of(&v, "tool_name").to_string(),
            parent_tool_use_id: opt_str(&v, "parent_tool_use_id"),
            elapsed_time_seconds: v.get("elapsed_time_seconds").and_then(Value::as_f64).unwrap_or(0.0),
            heartbeat: v.get("heartbeat").and_then(Value::as_bool).unwrap_or(false),
        })),
        "control_response" => v.get("response").filter(|r| r.is_object()).map(|r| {
            Event::ControlResponse(ControlResponse {
                request_id: str_of(r, "request_id").to_string(),
                ok: str_of(r, "subtype") == "success",
                response: r.get("response").cloned().unwrap_or(Value::Null),
                error: r.get("error").map(|e| e.as_str().map(str::to_string).unwrap_or_else(|| e.to_string())),
                whole: r.clone(),
            })
        }),
        "control_request" => v.get("request").filter(|r| r.is_object()).map(|r| {
            Event::ControlRequest(ControlRequest {
                request_id: str_of(&v, "request_id").to_string(),
                subtype: str_of(r, "subtype").to_string(),
                request: r.clone(),
            })
        }),
        _ => None,
    };
    parsed.unwrap_or(Event::Other(v))
}

fn init(v: &Value) -> Init {
    Init {
        session_id: str_of(v, "session_id").to_string(),
        model: str_of(v, "model").to_string(),
        cwd: str_of(v, "cwd").to_string(),
        tools: strings(v, "tools"),
        mcp_servers: v
            .get("mcp_servers")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|s| McpServer { name: str_of(s, "name").to_string(), status: str_of(s, "status").to_string() })
                    .collect()
            })
            .unwrap_or_default(),
        permission_mode: str_of(v, "permissionMode").to_string(),
        api_key_source: str_of(v, "apiKeySource").to_string(),
        version: str_of(v, "claude_code_version").to_string(),
        capabilities: strings(v, "capabilities"),
        effort: opt_str(v, "effort"),
    }
}

/// A `usage` object's counts (missing ones 0).
fn usage_of(usage: &Value) -> Usage {
    let server = usage.get("server_tool_use").cloned().unwrap_or(Value::Null);
    Usage {
        input_tokens: u64_of(usage, "input_tokens"),
        output_tokens: u64_of(usage, "output_tokens"),
        cache_creation_input_tokens: u64_of(usage, "cache_creation_input_tokens"),
        cache_read_input_tokens: u64_of(usage, "cache_read_input_tokens"),
        web_search_requests: u64_of(&server, "web_search_requests"),
        web_fetch_requests: u64_of(&server, "web_fetch_requests"),
    }
}

fn result(v: &Value) -> TurnResult {
    let usage = v.get("usage").cloned().unwrap_or(Value::Null);
    TurnResult {
        subtype: str_of(v, "subtype").to_string(),
        is_error: v.get("is_error").and_then(Value::as_bool).unwrap_or(false),
        session_id: str_of(v, "session_id").to_string(),
        text: str_of(v, "result").to_string(),
        terminal_reason: opt_str(v, "terminal_reason"),
        stop_reason: opt_str(v, "stop_reason"),
        api_error_status: v.get("api_error_status").and_then(Value::as_u64).and_then(|s| u16::try_from(s).ok()),
        cost_usd: v.get("total_cost_usd").and_then(Value::as_f64).unwrap_or(0.0),
        usage: usage_of(&usage),
        num_turns: u64_of(v, "num_turns"),
        duration_ms: u64_of(v, "duration_ms"),
        permission_denials: v
            .get("permission_denials")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .map(|d| Denial {
                        tool_name: str_of(d, "tool_name").to_string(),
                        tool_use_id: str_of(d, "tool_use_id").to_string(),
                        tool_input: d.get("tool_input").cloned().unwrap_or(Value::Null),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        errors: strings(v, "errors"),
        context_windows: v
            .get("modelUsage")
            .and_then(Value::as_object)
            .map(|m| m.iter().filter_map(|(model, u)| Some((model.clone(), u.get("contextWindow").and_then(Value::as_u64)?))).collect())
            .unwrap_or_default(),
        model_usage: v
            .get("modelUsage")
            .and_then(Value::as_object)
            .map(|m| {
                m.iter()
                    .map(|(model, u)| ModelUse {
                        model: model.clone(),
                        input: u64_of(u, "inputTokens"),
                        output: u64_of(u, "outputTokens"),
                        cache_read: u64_of(u, "cacheReadInputTokens"),
                        cache_write: u64_of(u, "cacheCreationInputTokens"),
                        cost_usd: u.get("costUSD").and_then(Value::as_f64).unwrap_or(0.0),
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// `unifiedWindows`, whichever way it comes: `{"seven_day": {utilization,
/// resetsAt}, …}` or `[{rateLimitType|type|window, utilization, resetsAt},
/// …]`. Anything else: none.
fn rate_windows(v: Option<&Value>) -> Vec<RateWindow> {
    let one = |kind: &str, w: &Value| -> Option<RateWindow> {
        let w = w.as_object()?;
        let at = w.get("resetsAt").or_else(|| w.get("resets_at"));
        Some(RateWindow {
            kind: kind.to_string(),
            utilization: w.get("utilization").and_then(Value::as_f64),
            resets_at: at.and_then(Value::as_f64).map(unix_seconds),
        })
    };
    let all: Vec<RateWindow> = match v {
        Some(Value::Object(m)) => m.iter().filter_map(|(k, w)| one(k, w)).collect(),
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(|w| {
                let kind = ["rateLimitType", "type", "window"].iter().find_map(|k| w.get(*k).and_then(Value::as_str))?;
                one(kind, w)
            })
            .collect(),
        _ => Vec::new(),
    };
    all.into_iter().filter(|w| !w.kind.is_empty()).collect()
}

/// A call's prompt, from its `usage`: what it read new, from the cache
/// and into it. `None` when it says nothing.
fn prompt_of(usage: Option<&Value>) -> Option<u64> {
    let u = usage.filter(|u| u.is_object())?;
    let keys = ["input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"];
    keys.iter().any(|k| u.get(*k).is_some_and(Value::is_number)).then(|| keys.iter().map(|k| u64_of(u, k)).sum())
}

fn stream_kind(e: &Value) -> StreamKind {
    let index = u64_of(e, "index");
    match str_of(e, "type") {
        "message_start" => {
            let m = e.get("message").cloned().unwrap_or(Value::Null);
            StreamKind::MessageStart { id: opt_str(&m, "id"), model: opt_str(&m, "model"), prompt: prompt_of(m.get("usage")) }
        }
        "content_block_start" => {
            let b = e.get("content_block").cloned().unwrap_or(Value::Null);
            let block = match str_of(&b, "type") {
                "text" => BlockStart::Text,
                "thinking" => BlockStart::Thinking,
                "tool_use" => BlockStart::ToolUse { id: str_of(&b, "id").to_string(), name: str_of(&b, "name").to_string() },
                other => BlockStart::Other(other.to_string()),
            };
            StreamKind::BlockStart { index, block }
        }
        "content_block_delta" => {
            let d = e.get("delta").cloned().unwrap_or(Value::Null);
            match str_of(&d, "type") {
                "text_delta" => StreamKind::Text { index, text: str_of(&d, "text").to_string() },
                "thinking_delta" => StreamKind::Thinking { index, thinking: str_of(&d, "thinking").to_string() },
                "input_json_delta" => {
                    StreamKind::ToolInput { index, partial_json: str_of(&d, "partial_json").to_string() }
                }
                _ => StreamKind::Other(e.clone()),
            }
        }
        "content_block_stop" => StreamKind::BlockStop { index },
        "message_delta" => StreamKind::MessageDelta {
            stop_reason: e.get("delta").and_then(|d| d.get("stop_reason")).and_then(Value::as_str).map(str::to_string),
            output_tokens: e.get("usage").and_then(|u| u.get("output_tokens")).and_then(Value::as_u64),
        },
        "message_stop" => StreamKind::MessageStop,
        _ => StreamKind::Other(e.clone()),
    }
}

fn blocks(content: Option<&Value>) -> Vec<Block> {
    match content {
        // A user message whose content is a plain string.
        Some(Value::String(s)) => vec![Block::Text(s.clone())],
        Some(Value::Array(a)) => a.iter().map(block).collect(),
        _ => Vec::new(),
    }
}

fn block(b: &Value) -> Block {
    match str_of(b, "type") {
        "text" => Block::Text(str_of(b, "text").to_string()),
        "thinking" => Block::Thinking(str_of(b, "thinking").to_string()),
        "tool_use" => Block::ToolUse {
            id: str_of(b, "id").to_string(),
            name: str_of(b, "name").to_string(),
            input: b.get("input").cloned().unwrap_or(Value::Null),
        },
        "tool_result" => Block::ToolResult {
            tool_use_id: str_of(b, "tool_use_id").to_string(),
            text: match b.get("content") {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Array(a)) => join_text(&a.iter().map(block).collect::<Vec<_>>()),
                _ => String::new(),
            },
            is_error: b.get("is_error").and_then(Value::as_bool).unwrap_or(false),
            pictures: match b.get("content") {
                Some(Value::Array(a)) => a.iter().filter_map(picture).take(RESULT_PICTURES).collect(),
                _ => Vec::new(),
            },
        },
        "image" => Block::Image {
            media_type: b.get("source").map(|s| str_of(s, "media_type").to_string()).unwrap_or_default(),
        },
        _ => Block::Other(b.clone()),
    }
}

/// An `image` block's picture (base64, within [`PICTURE_CHARS`]); anything
/// else, or a picture by its address, is none.
fn picture(b: &Value) -> Option<Picture> {
    if str_of(b, "type") != "image" {
        return None;
    }
    let source = b.get("source")?;
    let data = str_of(source, "data");
    (str_of(source, "type") == "base64" && !data.is_empty() && data.len() <= PICTURE_CHARS).then(|| Picture { media_type: str_of(source, "media_type").to_string(), data: data.to_string() })
}

fn join_text(blocks: &[Block]) -> String {
    blocks
        .iter()
        .filter_map(|b| match b {
            Block::Text(t) => Some(t.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

/// Unix seconds from a value that may be seconds or milliseconds.
fn unix_seconds(t: f64) -> i64 {
    if t > 1e12 {
        (t / 1000.0) as i64
    } else {
        t as i64
    }
}

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn opt_str(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

fn u64_of(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(|n| n.as_u64().or_else(|| n.as_f64().map(|f| f.max(0.0) as u64))).unwrap_or(0)
}

fn strings(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

// ---- Problems ----

/// How the run was asked to pay, for [`Classifier::expect_pay`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paying {
    Plan,
    ApiKey,
}

/// Turns a turn's events into [`Problem`]s: each one said once per turn
/// (the same problem again in that turn isn't), the slate wiped at the
/// turn's `result`.
#[derive(Debug, Default)]
pub struct Classifier {
    paying: Option<Paying>,
    said: Vec<Problem>,
}

impl Classifier {
    /// Check `system/init`'s `apiKeySource` against how Yap asked the run
    /// to pay ([`Problem::PaysAnotherWay`]).
    pub fn expect_pay(paying: Paying) -> Classifier {
        Classifier { paying: Some(paying), said: Vec::new() }
    }

    /// The problem `event` shows, if it's one not already said this turn.
    pub fn feed(&mut self, event: &Event) -> Option<Problem> {
        let found = problem_in(event).or_else(|| match (event, self.paying) {
            (Event::Init(init), Some(paying)) => pays_another_way(&init.api_key_source, paying),
            _ => None,
        });
        let new = found.filter(|p| !self.said.contains(p));
        if let Some(p) = &new {
            self.said.push(p.clone());
        }
        if matches!(event, Event::Result(_)) {
            self.said.clear();
        }
        new
    }
}

/// `apiKeySource` that doesn't match how the run was asked to pay. A plan
/// run must never be billed to a key (`ANTHROPIC_API_KEY`, an
/// `apiKeyHelper` from the person's settings, a Console login's managed
/// key); a key run must be on the key Yap gave it.
pub fn pays_another_way(source: &str, paying: Paying) -> Option<Problem> {
    let wrong = match paying {
        Paying::Plan => matches!(source, "ANTHROPIC_API_KEY" | "apiKeyHelper" | "/login managed key"),
        Paying::ApiKey => !source.is_empty() && source != "ANTHROPIC_API_KEY",
    };
    wrong.then(|| Problem::PaysAnotherWay { source: source.to_string() })
}

/// The problem one event shows on its own.
pub fn problem_in(event: &Event) -> Option<Problem> {
    match event {
        Event::ApiRetry(r) => by_error_kind(&r.error),
        Event::Assistant(a) => a.error.as_deref().and_then(by_error_kind),
        Event::RateLimit(r) if r.status == "rejected" => Some(Problem::RateLimited {
            resets_at: r.resets_at,
            credits_required: r.error_code.as_deref() == Some("credits_required"),
        }),
        Event::Result(r) => in_result(r),
        _ => None,
    }
}

/// Claude Code's error categories (`api_retry`'s `error`, an assistant
/// message's `error`).
fn by_error_kind(kind: &str) -> Option<Problem> {
    match kind {
        "authentication_failed" => Some(Problem::NotSignedIn),
        "rate_limit" => Some(Problem::RateLimited { resets_at: None, credits_required: false }),
        "billing_error" => Some(Problem::Billing),
        "oauth_org_not_allowed" | "account_on_hold" => Some(Problem::Account { reason: kind.to_string() }),
        _ => None,
    }
}

/// A result that failed on the API: signed out or refused (401/403, "Not
/// logged in", "Failed to authenticate", "Invalid API key"), or limited
/// (429, "limit").
fn in_result(r: &TurnResult) -> Option<Problem> {
    let api_failed = r.terminal_reason.as_deref() == Some("api_error") || r.api_error_status.is_some();
    if !r.is_error || !api_failed {
        return None;
    }
    let text = r.text.trim().to_lowercase();
    // Its sign-in couldn't be renewed for a moment: not signed out.
    if text.contains("failed to refresh oauth token") && ["another claude code process", "mid-refresh", "transient", "retry in a minute"].iter().any(|w| text.contains(w)) {
        return Some(Problem::SignInBusy);
    }
    let auth_words = ["not logged in", "failed to authenticate", "invalid api key", "please run /login", "oauth token"];
    if matches!(r.api_error_status, Some(401 | 403)) || auth_words.iter().any(|w| text.contains(w)) {
        return Some(Problem::NotSignedIn);
    }
    let limit_words = ["rate limit", "usage limit", "hit your limit", "limit reached", "reached your limit"];
    if r.api_error_status == Some(429) || limit_words.iter().any(|w| text.contains(w)) {
        return Some(Problem::RateLimited { resets_at: None, credits_required: false });
    }
    if text.contains("credit balance") || text.contains("billing") {
        return Some(Problem::Billing);
    }
    None
}

// ---- Tests ----

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    /// Every fixture: (Claude Code version, file name, its output lines with
    /// `{{session_id}}` filled in). Control lines (`_…`) and `//` comments
    /// are the fake's, not Claude Code's, and left out.
    pub(crate) fn fixtures() -> Vec<(String, String, Vec<String>)> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ai/claude_code/fixtures");
        let mut out = Vec::new();
        let mut versions: Vec<PathBuf> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.path()).collect();
        versions.sort();
        // A folder per Claude Code version ("2.1.296"); `fake/` holds the
        // fake's example scripts.
        let is_version = |p: &PathBuf| p.is_dir() && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(|c: char| c.is_ascii_digit()));
        for dir in versions.into_iter().filter(is_version) {
            let version = dir.file_name().unwrap().to_string_lossy().to_string();
            let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).collect();
            files.sort();
            for file in files.into_iter().filter(|p| p.extension().is_some_and(|e| e == "jsonl")) {
                let name = file.file_name().unwrap().to_string_lossy().to_string();
                let lines = std::fs::read_to_string(&file)
                    .unwrap()
                    .lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| !l.is_empty() && !l.starts_with("//") && !l.starts_with("{\"_"))
                    .map(|l| l.replace("{{session_id}}", "00000000-0000-4000-8000-000000000001").replace("{{user_text}}", "hello"))
                    .collect();
                out.push((version.clone(), name, lines));
            }
        }
        out
    }

    fn fixture(name: &str) -> Vec<Event> {
        let (_, _, lines) = fixtures().into_iter().find(|(_, n, _)| n == name).unwrap_or_else(|| panic!("no fixture {name}"));
        lines.iter().filter_map(|l| parse_line(l)).collect()
    }

    fn problems(events: &[Event], paying: Paying) -> Vec<Problem> {
        let mut c = Classifier::expect_pay(paying);
        events.iter().filter_map(|e| c.feed(e)).collect()
    }

    #[test]
    fn every_fixture_parses_every_line() {
        let all = fixtures();
        assert!(all.len() >= 8, "found the fixtures");
        for (version, name, lines) in &all {
            assert!(!lines.is_empty(), "{version}/{name} has lines");
            for (i, line) in lines.iter().enumerate() {
                let event = parse_line(line).unwrap_or_else(|| panic!("{version}/{name}:{i} blank"));
                assert!(!matches!(event, Event::NotJson { .. }), "{version}/{name}:{i} isn't JSON");
                // Claude Code's own types are all known; only the fake's
                // reports (`fake_*`) and Claude Code's extra system
                // messages may be `Other`.
                if let Event::Other(v) = &event {
                    let ty = v["type"].as_str().unwrap_or("");
                    assert_eq!(ty, "system", "{version}/{name}:{i} unknown {ty}");
                }
            }
            let last = parse_line(lines.last().unwrap()).unwrap();
            assert!(matches!(last, Event::Result(_)), "{version}/{name} ends with a result");
        }
    }

    // The numbers of real results (2.1.296, a Yap Lab run, 11 Nov 2026):
    // `total_cost_usd` is the session's running total, `modelUsage` what
    // each model has used in it so far, `usage` this turn's own.
    fn real_result(total: f64, usage: [u64; 4], models: &[(&str, [u64; 4], f64)]) -> TurnResult {
        let line = serde_json::json!({
            "type": "result", "subtype": "success", "is_error": false, "result": "Done.", "session_id": "5c6d7e8f-9a0b-4c1d-8e2f-3a4b5c6d7e8f",
            "total_cost_usd": total,
            "usage": { "input_tokens": usage[0], "output_tokens": usage[1], "cache_creation_input_tokens": usage[2], "cache_read_input_tokens": usage[3] },
            "modelUsage": models.iter().map(|(m, t, cost)| (m.to_string(), serde_json::json!({ "inputTokens": t[0], "outputTokens": t[1], "cacheCreationInputTokens": t[2], "cacheReadInputTokens": t[3], "costUSD": cost, "contextWindow": 1_000_000 }))).collect::<serde_json::Map<String, Value>>(),
        });
        let Some(Event::Result(r)) = parse_line(&line.to_string()) else { panic!("a result") };
        r
    }

    #[test]
    fn a_turns_own_cost_is_what_the_sessions_running_total_went_up_by() {
        let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
        // A plan chat's process, three answers: each result says the total so far.
        let first = real_result(0.1294579, [4, 549, 29_516, 58_959], &[("claude-sonnet-5-5", [4, 549, 29_516, 58_959], 0.1294579)]);
        let second = real_result(0.1437812, [6, 178, 891, 89_673], &[("claude-sonnet-5-5", [10, 727, 30_407, 148_632], 0.1437812)]);
        let third = real_result(0.1624425, [6, 280, 1_659, 92_133], &[("claude-sonnet-5-5", [16, 1_007, 32_066, 240_765], 0.1624425)]);
        assert_eq!(second.model_usage, [ModelUse { model: "claude-sonnet-5-5".into(), input: 10, output: 727, cache_read: 148_632, cache_write: 30_407, cost_usd: 0.1437812 }]);
        assert!(!first.covers_earlier_turns() && second.covers_earlier_turns() && third.covers_earlier_turns());
        // A new session starts from nothing: its first answer's cost is the total.
        assert!(near(first.cost_since(Some(0.0), "claude-sonnet-5-5"), 0.1294579));
        // The next two: what the total went up by — a cent or two, never the
        // 14 and 16 cents the answers were counted at.
        assert!(near(second.cost_since(Some(first.cost_usd), "claude-sonnet-5-5"), 0.0143233));
        assert!(near(third.cost_since(Some(second.cost_usd), "claude-sonnet-5-5"), 0.0186613));

        // An agent's second turn, a new process on `--resume`, on another
        // model: its first result's total goes on from the first turn's, and
        // `modelUsage` still lists that turn's model with its tokens and cost.
        let turn_one = real_result(0.01395236, [12, 1_977, 47_728, 341_706], &[("claude-haiku-5-5", [12, 1_977, 47_728, 341_706], 0.01395236)]);
        let resumed = real_result(0.23419546, [6, 832, 49_153, 152_991], &[("claude-haiku-5-5", [12, 1_977, 47_728, 341_706], 0.01395236), ("claude-sonnet-5-5", [6, 832, 49_153, 152_991], 0.2202431)]);
        assert!(resumed.covers_earlier_turns());
        assert!(near(resumed.cost_since(Some(turn_one.cost_usd), "claude-sonnet-5-5"), 0.2202431));
        // Nothing kept of the session (an agent from before, a file an older
        // Yap saved): this turn's share of its model's running cost.
        assert!(near(resumed.cost_since(None, "claude-sonnet-5-5"), 0.2202431));
        let share = second.cost_since(None, "claude-sonnet-5-5");
        assert!(share > 0.005 && share < 0.04, "about its own {share}, never the whole 0.14");
        // The model's id with a date or a window after it; one not listed: the busiest entry's.
        assert!(near(resumed.cost_since(None, "claude-sonnet-5-5[1m]"), 0.2202431));
        assert!(resumed.cost_since(None, "claude-opus-5-5") < 0.02);

        // A total that started again (a Claude Code that keeps none across
        // `--resume`: nothing of earlier turns in it): all of it is this turn's.
        let again = real_result(0.05, [10, 400, 2_000, 60_000], &[("claude-sonnet-5-5", [10, 400, 2_000, 60_000], 0.05)]);
        assert!(near(again.cost_since(Some(0.30), "claude-sonnet-5-5"), 0.05));
        assert!(near(again.cost_since(None, "claude-sonnet-5-5"), 0.05));
        // No `modelUsage` at all (the stand-in's results): as it says.
        let plain = TurnResult { cost_usd: 0.03, ..Default::default() };
        assert!(near(plain.cost_since(Some(0.02), ""), 0.03) && near(plain.cost_since(None, ""), 0.03));
        // Never below nothing.
        assert_eq!(TurnResult { cost_usd: -1.0, ..Default::default() }.cost_since(Some(0.0), ""), 0.0);
    }

    #[test]
    fn a_sign_in_that_couldnt_be_renewed_for_a_moment_is_never_not_signed_in() {
        // Claude Code 2.1.296's own result, as a Yap Lab run got it.
        let said = "Failed to refresh OAuth token: another Claude Code process is refreshing it or exited mid-refresh. This is usually transient; retry in a minute, and if it persists close other Claude Code processes or sign in again";
        let r = TurnResult { is_error: true, terminal_reason: Some("api_error".into()), text: said.into(), ..Default::default() };
        assert_eq!(problem_in(&Event::Result(r)), Some(Problem::SignInBusy));
        let words = Problem::SignInBusy.message();
        assert!(words.starts_with("Claude Code couldn't renew its sign-in just now") && words.contains("still signed in") && words.contains("Try again in a minute"), "{words}");
        assert_eq!(serde_json::to_value(Problem::SignInBusy).unwrap(), serde_json::json!({"kind":"signInBusy"}));
        // A token that was refused is still a sign-in problem.
        let refused = TurnResult { is_error: true, terminal_reason: Some("api_error".into()), text: "OAuth token has expired. Please run /login".into(), ..Default::default() };
        assert_eq!(problem_in(&Event::Result(refused)), Some(Problem::NotSignedIn));
    }

    #[test]
    fn a_real_signed_out_turn_is_not_signed_in_never_an_answer() {
        let events = fixture("real-not-logged-in.jsonl");
        let Event::Init(init) = &events[0] else { panic!("init first") };
        assert_eq!(init.version, "2.1.296");
        assert_eq!(init.api_key_source, "none");
        assert!(init.capabilities.iter().any(|c| c == "interrupt_receipt_v1"));
        assert_eq!(init.permission_mode, "dontAsk");
        let Some(Event::Result(r)) = events.last() else { panic!("result last") };
        assert!(r.is_error);
        assert_eq!(r.terminal_reason.as_deref(), Some("api_error"));
        assert_eq!(r.text, "Not logged in · Please run /login");
        assert_eq!(r.cost_usd, 0.0);
        assert_eq!(problems(&events, Paying::Plan), [Problem::NotSignedIn], "said once, not twice");
    }

    #[test]
    fn a_refused_key_is_not_signed_in_and_pays_by_key() {
        let events = fixture("real-invalid-api-key.jsonl");
        let Some(Event::Result(r)) = events.last() else { panic!() };
        assert_eq!(r.api_error_status, Some(401));
        assert_eq!(problems(&events, Paying::ApiKey), [Problem::NotSignedIn]);
        // The same run asked to pay by plan: Claude Code billed a key.
        let p = problems(&events, Paying::Plan);
        assert_eq!(p[0], Problem::PaysAnotherWay { source: "ANTHROPIC_API_KEY".into() });
    }

    #[test]
    fn a_failed_to_authenticate_result_alone_is_not_signed_in() {
        let r = TurnResult {
            is_error: true,
            terminal_reason: Some("api_error".into()),
            text: "Failed to authenticate. API Error: 401 {\"type\":\"error\"}".into(),
            ..Default::default()
        };
        assert_eq!(problem_in(&Event::Result(r)), Some(Problem::NotSignedIn));
        // An ordinary answer that happens to mention logging in isn't.
        let fine = TurnResult { text: "Run /login, then try.".into(), terminal_reason: Some("completed".into()), ..Default::default() };
        assert_eq!(problem_in(&Event::Result(fine)), None);
    }

    #[test]
    fn rate_limits_are_found_with_when_they_reset() {
        let events = fixture("synthetic-rate-limited.jsonl");
        let p = problems(&events, Paying::Plan);
        assert_eq!(
            p,
            [
                Problem::RateLimited { resets_at: None, credits_required: false },
                Problem::RateLimited { resets_at: Some(1_794_067_200), credits_required: false },
            ],
            "the retry first, then the limit with its reset time; the result adds nothing new"
        );
        let retry = events.iter().find_map(|e| if let Event::ApiRetry(r) = e { Some(r.clone()) } else { None }).unwrap();
        assert_eq!((retry.attempt, retry.max_retries, retry.error_status, retry.error.as_str()), (1, 10, Some(429), "rate_limit"));
        // A 429 result on its own.
        let r = TurnResult { is_error: true, api_error_status: Some(429), ..Default::default() };
        assert!(matches!(problem_in(&Event::Result(r)), Some(Problem::RateLimited { .. })));
        // A plan's spent usage, in milliseconds.
        let e = from_value(serde_json::json!({"type":"rate_limit_event","rate_limit_info":{"status":"rejected","resetsAt":1_794_067_200_000_i64,"errorCode":"credits_required"}}));
        assert_eq!(problem_in(&e), Some(Problem::RateLimited { resets_at: Some(1_794_067_200), credits_required: true }));
        let warn = from_value(serde_json::json!({"type":"rate_limit_event","rate_limit_info":{"status":"allowed_warning","utilization":0.9}}));
        assert_eq!(problem_in(&warn), None, "a warning isn't a stop");
    }

    #[test]
    fn auth_retries_and_account_errors() {
        for (kind, want) in [
            ("authentication_failed", Some(Problem::NotSignedIn)),
            ("billing_error", Some(Problem::Billing)),
            ("account_on_hold", Some(Problem::Account { reason: "account_on_hold".into() })),
            ("overloaded", None),
            ("something_new", None),
        ] {
            let e = from_value(serde_json::json!({"type":"system","subtype":"api_retry","attempt":1,"max_retries":10,"retry_delay_ms":500,"error_status":null,"error":kind}));
            assert_eq!(problem_in(&e), want, "{kind}");
        }
    }

    #[test]
    fn partial_text_and_thinking_stream_in_order() {
        let events = fixture("synthetic-text-and-thinking.jsonl");
        let text: String = events
            .iter()
            .filter_map(|e| match e {
                Event::Stream(Stream { kind: StreamKind::Text { text, .. }, .. }) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "Hello, Imogen. The meeting is at 3 pm.");
        let thinking: String = events
            .iter()
            .filter_map(|e| match e {
                Event::Stream(Stream { kind: StreamKind::Thinking { thinking, .. }, .. }) => Some(thinking.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(thinking, "They want a greeting.");
        let a = events.iter().find_map(|e| if let Event::Assistant(a) = e { Some(a.clone()) } else { None }).unwrap();
        assert_eq!(join_text(&a.blocks), "Hello, Imogen. The meeting is at 3 pm.");
        assert!(matches!(&a.blocks[0], Block::Thinking(t) if t == "They want a greeting."));
        let Some(Event::Result(r)) = events.last() else { panic!() };
        assert!(!r.is_error);
        assert_eq!(r.text, "Hello, Imogen. The meeting is at 3 pm.");
        assert_eq!((r.usage.input_tokens, r.usage.output_tokens, r.usage.cache_read_input_tokens), (1200, 42, 1000));
        assert!((r.cost_usd - 0.0021).abs() < 1e-9);
        assert!(problems(&events, Paying::Plan).is_empty());
        // Each call's own prompt and answer (the result's are the turn's
        // sum), and the model's context window.
        let starts: Vec<Option<u64>> = events
            .iter()
            .filter_map(|e| match e {
                Event::Stream(Stream { kind: StreamKind::MessageStart { prompt, .. }, .. }) => Some(*prompt),
                _ => None,
            })
            .collect();
        assert_eq!(starts, [Some(1200)]);
        assert!(events.iter().any(|e| matches!(e, Event::Stream(Stream { kind: StreamKind::MessageDelta { output_tokens: Some(42), .. }, .. }))));
        assert_eq!(r.context_window("claude-haiku-5-5"), Some(200_000));
        assert_eq!(r.context_window("claude-opus-5-5"), None);
    }

    #[test]
    fn a_calls_prompt_counts_the_cache_and_rate_limits_say_their_window() {
        let start = |usage: Value| match from_value(serde_json::json!({"type":"stream_event","event":{"type":"message_start","message":{"id":"m","usage":usage}}})) {
            Event::Stream(Stream { kind: StreamKind::MessageStart { prompt, .. }, .. }) => prompt,
            e => panic!("{e:?}"),
        };
        assert_eq!(start(serde_json::json!({"input_tokens":12,"cache_creation_input_tokens":300,"cache_read_input_tokens":41_000,"output_tokens":1})), Some(41_312));
        assert_eq!(start(serde_json::json!({"output_tokens":1})), None, "nothing about the prompt");
        assert_eq!(start(Value::Null), None);
        let e = from_value(serde_json::json!({"type":"rate_limit_event","rate_limit_info":{"status":"allowed_warning","rateLimitType":"seven_day","utilization":0.86,"resetsAt":1_794_067_200,
            "unifiedWindows":{"five_hour":{"utilization":0.4,"resetsAt":1_794_000_000},"seven_day":{"utilization":0.86},"odd":7}}}));
        let Event::RateLimit(r) = e else { panic!() };
        assert_eq!(r.kind.as_deref(), Some("seven_day"));
        assert_eq!(r.windows.len(), 2, "{:?}", r.windows);
        assert!(r.windows.contains(&RateWindow { kind: "five_hour".into(), utilization: Some(0.4), resets_at: Some(1_794_000_000) }));
        let listed = from_value(serde_json::json!({"type":"rate_limit_event","rate_limit_info":{"status":"allowed","unifiedWindows":[{"rateLimitType":"five_hour","utilization":0.1},{"utilization":0.2}]}}));
        let Event::RateLimit(r) = listed else { panic!() };
        assert_eq!(r.windows, [RateWindow { kind: "five_hour".into(), utilization: Some(0.1), resets_at: None }], "one with no name left out");
    }

    #[test]
    fn tool_use_and_its_result_with_a_subagent() {
        let events = fixture("synthetic-tool-use.jsonl");
        let calls: Vec<(String, String, Option<String>)> = events
            .iter()
            .filter_map(|e| match e {
                Event::Assistant(a) => a.blocks.iter().find_map(|b| match b {
                    Block::ToolUse { name, input, .. } => {
                        Some((name.clone(), input["query"].as_str().unwrap_or("").to_string(), a.parent_tool_use_id.clone()))
                    }
                    _ => None,
                }),
                _ => None,
            })
            .collect();
        assert_eq!(calls[0], ("WebSearch".to_string(), "tide times example bay".to_string(), None));
        assert_eq!(calls[1].0, "Task");
        assert_eq!(
            calls.iter().filter_map(|c| c.2.as_deref()).collect::<Vec<_>>(),
            ["toolu_agent_1"],
            "the subagent's call names its parent"
        );
        let starts: Vec<&BlockStart> = events
            .iter()
            .filter_map(|e| match e {
                Event::Stream(Stream { kind: StreamKind::BlockStart { block, .. }, .. }) => Some(block),
                _ => None,
            })
            .collect();
        assert!(starts.iter().any(|b| matches!(b, BlockStart::ToolUse { name, .. } if name == "WebSearch")));
        let input: String = events
            .iter()
            .filter_map(|e| match e {
                Event::Stream(Stream { kind: StreamKind::ToolInput { partial_json, .. }, .. }) => Some(partial_json.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(serde_json::from_str::<Value>(&input).unwrap()["query"], "tide times example bay");
        let u = events.iter().find_map(|e| if let Event::User(u) = e { Some(u.clone()) } else { None }).unwrap();
        assert!(matches!(&u.blocks[0], Block::ToolResult { tool_use_id, text, is_error: false, pictures } if tool_use_id == "toolu_search_1" && text.contains("example.com") && pictures.is_empty()));
        assert!(u.tool_use_result.is_some());
        let progress = events.iter().find_map(|e| if let Event::ToolProgress(p) = e { Some(p.clone()) } else { None }).unwrap();
        assert!(progress.heartbeat);
        assert_eq!(progress.tool_name, "WebSearch");
    }

    #[test]
    fn a_permission_denial_is_an_event_and_in_the_result() {
        let events = fixture("synthetic-permission-denied.jsonl");
        let d = events.iter().find_map(|e| if let Event::PermissionDenied(d) = e { Some(d.clone()) } else { None }).unwrap();
        assert_eq!(d.tool_name, "Bash");
        assert!(!d.message.is_empty());
        let Some(Event::Result(r)) = events.last() else { panic!() };
        assert_eq!(r.permission_denials.len(), 1);
        assert_eq!(r.permission_denials[0].tool_input["command"], "rm -rf build");
        let hooks: Vec<HookPhase> = events.iter().filter_map(|e| if let Event::Hook(h) = e { Some(h.phase) } else { None }).collect();
        assert_eq!(hooks, [HookPhase::Started, HookPhase::Response]);
    }

    #[test]
    fn an_interrupt_answers_and_the_turn_ends_aborted() {
        let events = fixture("real-interrupt-receipt.jsonl");
        let Event::ControlResponse(c) = &events[0] else { panic!("the receipt comes first") };
        assert!(c.ok);
        assert_eq!(c.request_id, "req_1_a1b2");
        assert_eq!(c.response["still_queued"], serde_json::json!([]));
        let r = TurnResult { terminal_reason: Some("aborted_streaming".into()), ..Default::default() };
        assert!(r.interrupted());
    }

    #[test]
    fn nothing_breaks_the_stream() {
        assert_eq!(parse_line(""), None);
        assert_eq!(parse_line("   \r"), None);
        assert_eq!(parse_line("running 1 test"), Some(Event::NotJson { chars: 14 }));
        assert_eq!(parse_line("[1,2]"), Some(Event::NotJson { chars: 5 }));
        assert_eq!(parse_line("{\"type\":\"result\""), Some(Event::NotJson { chars: 16 }));
        assert!(matches!(parse_line("{\"type\":\"brand_new\",\"x\":1}"), Some(Event::Other(_))));
        assert!(matches!(parse_line("{\"type\":\"assistant\"}"), Some(Event::Other(_))), "no message: kept whole");
        assert!(matches!(parse_line("\u{feff}{\"type\":\"system\",\"subtype\":\"status\",\"status\":null}"), Some(Event::Status(_))));
        // Fields of the wrong type read as empty, never a panic.
        let e = parse_line("{\"type\":\"result\",\"is_error\":\"yes\",\"usage\":7,\"permission_denials\":{}}").unwrap();
        let Event::Result(r) = e else { panic!() };
        assert!(!r.is_error && r.permission_denials.is_empty() && r.usage == Usage::default());
        // A user message whose content is a plain string.
        let Event::User(u) = parse_line("{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"hi\"}}").unwrap() else { panic!() };
        assert_eq!(u.blocks, [Block::Text("hi".into())]);
    }

    #[test]
    fn plain_words_for_every_problem() {
        for p in [
            Problem::NotSignedIn,
            Problem::RateLimited { resets_at: None, credits_required: false },
            Problem::RateLimited { resets_at: None, credits_required: true },
            Problem::Billing,
            Problem::Account { reason: "account_on_hold".into() },
            Problem::PaysAnotherWay { source: "apiKeyHelper".into() },
        ] {
            assert!(p.message().ends_with('.'));
        }
        assert!(Problem::NotSignedIn.message().contains("Sign in to Claude Code"));
        assert!(Problem::RateLimited { resets_at: None, credits_required: false }.message().starts_with("You've reached your Claude plan's limit"));
        assert!(Problem::RateLimited { resets_at: Some(1_794_067_200), credits_required: false }.message().contains("(resets at "));
        assert!(Problem::PaysAnotherWay { source: "apiKeyHelper".into() }.message().contains("wouldn't use your Claude plan"));
        // When a limit resets, in the person's words.
        use chrono::TimeZone;
        let now = chrono::Utc.with_ymd_and_hms(2026, 11, 7, 10, 0, 0).unwrap();
        let at = |d: u32, h: u32| chrono::Utc.with_ymd_and_hms(2026, 11, d, h, 0, 0).unwrap().timestamp();
        assert_eq!(resets_words(at(7, 16), &now), "4:00 pm");
        assert_eq!(resets_words(at(8, 9), &now), "Sun 9:00 am");
        assert_eq!(resets_words(at(20, 16), &now), "20 Nov, 4:00 pm");
        assert_eq!(serde_json::to_value(Problem::NotSignedIn).unwrap(), serde_json::json!({"kind":"notSignedIn"}));
        assert_eq!(
            serde_json::to_value(Problem::RateLimited { resets_at: Some(5), credits_required: false }).unwrap(),
            serde_json::json!({"kind":"rateLimited","resetsAt":5,"creditsRequired":false})
        );
        assert_eq!(pays_another_way("none", Paying::Plan), None);
        assert_eq!(pays_another_way("ANTHROPIC_API_KEY", Paying::ApiKey), None);
        assert!(pays_another_way("/login managed key", Paying::Plan).is_some());
        assert!(pays_another_way("apiKeyHelper", Paying::ApiKey).is_some());
    }

    #[test]
    fn an_assistant_message_says_its_calls_usage() {
        let e = parse_line(
            r#"{"type":"assistant","message":{"id":"m1","model":"claude-sonnet-5-5","content":[{"type":"text","text":"Hi"}],"usage":{"input_tokens":12,"cache_creation_input_tokens":300,"cache_read_input_tokens":41000,"output_tokens":9}},"parent_tool_use_id":null}"#,
        )
        .unwrap();
        let Event::Assistant(a) = e else { panic!("{e:?}") };
        let u = a.usage.unwrap();
        assert_eq!((u.input_tokens, u.cache_creation_input_tokens, u.cache_read_input_tokens, u.output_tokens), (12, 300, 41_000, 9));
        // None said: none (never zeros taken for a call that read nothing).
        let Event::Assistant(a) = parse_line(r#"{"type":"assistant","message":{"id":"m2","content":[]}}"#).unwrap() else { panic!() };
        assert_eq!(a.usage, None);
        let Event::Assistant(a) = parse_line(r#"{"type":"assistant","message":{"id":"m3","content":[],"usage":7}}"#).unwrap() else { panic!() };
        assert_eq!(a.usage, None);
    }

    /// As Claude Code 2.1.296 hands back a picture file it read: an image
    /// block in the tool result's content (checked on the real one, 11 Nov
    /// 2026).
    #[test]
    fn a_tool_result_keeps_the_pictures_it_carries() {
        let line = r#"{"type":"user","message":{"role":"user","content":[{"tool_use_id":"toolu_read_1","type":"tool_result","content":[{"type":"text","text":"Read it."},{"type":"image","source":{"type":"base64","data":"iVBORw0KGgo=","media_type":"image/png"}},{"type":"image","source":{"type":"url","url":"https://example.com/a.png"}},{"type":"image","source":{"type":"base64","data":"","media_type":"image/png"}}]}]},"parent_tool_use_id":null}"#;
        let Event::User(u) = parse_line(line).unwrap() else { panic!() };
        let Block::ToolResult { tool_use_id, text, is_error, pictures } = &u.blocks[0] else { panic!("{:?}", u.blocks) };
        assert_eq!((tool_use_id.as_str(), text.as_str(), *is_error), ("toolu_read_1", "Read it.", false));
        // The one with data: one by its address, or empty, is none.
        assert_eq!(pictures, &[Picture { media_type: "image/png".into(), data: "iVBORw0KGgo=".into() }]);
        // Its data is never printed.
        let shown = format!("{:?}", u.blocks[0]);
        assert!(shown.contains("chars: 12") && !shown.contains("iVBOR"), "{shown}");
        // No more than a few a result, none past the size kept.
        let many: Vec<Value> = (0..RESULT_PICTURES + 3).map(|_| serde_json::json!({ "type": "image", "source": { "type": "base64", "data": "AAAA", "media_type": "image/jpeg" } })).collect();
        let Block::ToolResult { pictures, .. } = block(&serde_json::json!({ "type": "tool_result", "tool_use_id": "t", "content": many })) else { panic!() };
        assert_eq!(pictures.len(), RESULT_PICTURES);
        let big = serde_json::json!({ "type": "image", "source": { "type": "base64", "data": "A".repeat(PICTURE_CHARS + 1), "media_type": "image/png" } });
        assert_eq!(picture(&big), None);
        // A result that is plain words has none.
        let Block::ToolResult { pictures, .. } = block(&serde_json::json!({ "type": "tool_result", "tool_use_id": "t", "content": "Done" })) else { panic!() };
        assert!(pictures.is_empty());
    }
}

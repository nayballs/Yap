//! **A Code chat's context, broken down** (15 Nov 2026, after the Claude
//! app's right column): what fills its session's context, by category
//! (Messages, Memory files, System tools, MCP tools…), with the MCP tools,
//! memory files, custom agents and skills behind them — Claude Code's own
//! `/context`, run for the chat's session. The summary panel draws it
//! (`SummaryTab.svelte`); anything that goes wrong leaves the panel's one
//! number as before ([`code_chat_context`] says `None`), never an error.
//!
//! - **The run** ([`args`]): `claude -p /context --resume <session>
//!   --no-session-persistence`, in the workspace (Claude Code files a
//!   session by its folder), on the chat's model (its window), with the
//!   to-do list switch every turn has (`runner::TODO_ENV`: the same tools),
//!   the person's hooks off (`disableAllHooks`, a settings file: nothing of
//!   theirs runs because the summary opened) and the Claude Code
//!   `ai/claude_code/cli.rs` finds. Checked on 2.1.296 (15 Nov 2026): the
//!   command answers locally (`num_turns: 0`, `total_cost_usd: 0`), and
//!   with `--no-session-persistence` the session's transcript is left
//!   byte for byte as it was. Without it, `/context` and its whole answer
//!   are written into the session (about 5k tokens the model would read on
//!   every later turn).
//! - **When**: never while the chat's Claude works (a turn queued, running
//!   or asking: the last breakdown is kept), never two at once, and again
//!   only once a turn has ended or 5 minutes went by ([`FRESH_FOR`]).
//! - **What's read** ([`parse_output`]): the local command's answer carries
//!   both a `context_usage` object (exact counts) on its assistant message
//!   and the same as markdown (the `result`: `## Context Usage`,
//!   `**Tokens:** 31k / 1m (3%)`, then tables). Neither is a documented
//!   contract, so both are read tolerantly: the object first, the markdown
//!   if it's missing or unreadable ([`parse_markdown`]), nothing if neither.
//! - **Never a cost**: an answer that took a model turn or cost anything
//!   (a Claude Code that sent `/context` to the model) is thrown away, and
//!   that program and version is never asked again while Yap runs.
//! - **Not counted**: Yap's own tools for the session (its browser, its
//!   cards: a turn's endpoint, `claude_live`), which only a turn has. A
//!   breakdown is of the chat's own Claude only: a plain chat, or a Code
//!   chat Chat's model answers, keeps the one number.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use super::store::{Agent, Status};
use crate::ai::claude_code::cli::{self, Place, Program};
use crate::ai::claude_code::session::{self, Conversation, Pay, RunFiles, Spec};

/// How long one `/context` may take (it starts the person's MCP servers).
const TIMEOUT: Duration = Duration::from_secs(45);
/// How long a breakdown stands while the chat's session hasn't moved.
const FRESH_FOR: Duration = Duration::from_secs(5 * 60);

// ---- What it says -------------------------------------------------------------------------------

/// **The chat's context, by category**, as Claude Code's `/context` gives it.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Breakdown {
    /// The model it's measured against (`claude-opus-5-5`).
    pub model: String,
    /// Tokens in use (every category that's `used`).
    pub used: u64,
    /// The model's context window.
    pub window: u64,
    /// How full, as Claude Code rounds it.
    pub percent: f64,
    pub categories: Vec<Category>,
    pub mcp_tools: Vec<McpTool>,
    pub memory_files: Vec<MemoryFile>,
    pub agents: Vec<CustomAgent>,
    pub skills: Vec<Skill>,
}

/// One row of "Estimated usage by category".
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub name: String,
    pub tokens: u64,
    pub kind: Kind,
}

/// What a category's tokens are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// In the context now.
    Used,
    /// Loaded only when looked for (tool search): no share of the window.
    Deferred,
    /// Kept free for compacting.
    Buffer,
    /// Free space.
    Free,
}

impl Kind {
    /// Claude Code's word for it, else what the category's name says.
    fn of(word: Option<&str>, name: &str) -> Kind {
        match word.map(str::to_ascii_lowercase).as_deref() {
            Some("used") => Kind::Used,
            Some("deferred") => Kind::Deferred,
            Some("buffer") => Kind::Buffer,
            Some("free") => Kind::Free,
            _ => {
                let n = name.to_ascii_lowercase();
                if n.contains("(deferred)") {
                    Kind::Deferred
                } else if n.starts_with("free space") {
                    Kind::Free
                } else if n.contains("buffer") {
                    Kind::Buffer
                } else {
                    Kind::Used
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpTool {
    pub name: String,
    pub server: String,
    pub tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryFile {
    /// "User", "Project", "Local"…
    pub kind: String,
    pub path: String,
    pub tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomAgent {
    pub name: String,
    pub source: String,
    pub tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub name: String,
    pub source: String,
    pub tokens: u64,
}

/// What one `/context` run printed, read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Answer {
    pub breakdown: Option<Breakdown>,
    /// Answered by Claude Code itself: a result with no model turn and no
    /// cost. `false` also when there was no result at all.
    pub local: bool,
}

/// **`/context`'s stream-json output, read**: the assistant message's
/// `context_usage` first, else the markdown (the result's, or that
/// message's text). Lines that aren't JSON are skipped. Pure.
pub fn parse_output(stdout: &str) -> Answer {
    let mut usage = None;
    let mut text = None;
    let mut local = false;
    for line in stdout.lines() {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else { continue };
        match v.get("type").and_then(Value::as_str) {
            Some("assistant") => {
                if let Some(b) = v.get("context_usage").and_then(from_usage) {
                    usage = Some(b);
                }
                if text.is_none() {
                    text = v.pointer("/message/content").and_then(Value::as_array).and_then(|c| c.iter().find_map(|b| b.get("text").and_then(Value::as_str))).map(str::to_string);
                }
            }
            Some("result") => {
                let turns = v.get("num_turns").and_then(Value::as_u64).unwrap_or(u64::MAX);
                let cost = v.get("total_cost_usd").and_then(Value::as_f64).unwrap_or(f64::MAX);
                local = turns == 0 && cost == 0.0 && !v.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                if let Some(r) = v.get("result").and_then(Value::as_str).filter(|r| !r.trim().is_empty()) {
                    text = Some(r.to_string());
                }
            }
            _ => {}
        }
    }
    let breakdown = usage.or_else(|| text.as_deref().and_then(parse_markdown));
    Answer { breakdown, local }
}

fn str_of<'a>(v: &'a Value, keys: &[&str]) -> &'a str {
    keys.iter().find_map(|k| v.get(*k).and_then(Value::as_str)).unwrap_or_default()
}

fn tokens_of(v: &Value) -> u64 {
    match v.get("tokens") {
        Some(Value::Number(n)) => n.as_u64().or_else(|| n.as_f64().map(|f| f.max(0.0).round() as u64)).unwrap_or(0),
        Some(Value::String(s)) => count(s).unwrap_or(0),
        _ => 0,
    }
}

fn list_of<'a>(v: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    v.get(key).and_then(Value::as_array).into_iter().flatten()
}

/// **The `context_usage` object** of `/context`'s answer (2.1.296:
/// `model`, `total_tokens`, `raw_max_tokens`, `percentage`, `categories`
/// [{name, tokens, kind}], `mcp_tools` [{name, server_name, tokens}],
/// `memory_files` [{path, type, tokens}], `agents` [{agent_type, source,
/// tokens}], `skills` [{name, source, tokens}]). `None` without a window or
/// categories. Pure.
pub fn from_usage(v: &Value) -> Option<Breakdown> {
    let window = v.get("raw_max_tokens").or_else(|| v.get("max_tokens")).and_then(Value::as_u64).filter(|w| *w > 0)?;
    let categories: Vec<Category> = list_of(v, "categories")
        .filter_map(|c| {
            let name = str_of(c, &["name"]).trim();
            (!name.is_empty()).then(|| Category { name: name.to_string(), tokens: tokens_of(c), kind: Kind::of(c.get("kind").and_then(Value::as_str), name) })
        })
        .collect();
    if categories.is_empty() {
        return None;
    }
    let summed: u64 = categories.iter().filter(|c| c.kind == Kind::Used).map(|c| c.tokens).sum();
    let used = v.get("total_tokens").and_then(Value::as_u64).unwrap_or(summed);
    let percent = v.get("percentage").and_then(Value::as_f64).unwrap_or_else(|| share(used, window));
    Some(Breakdown {
        model: str_of(v, &["model"]).to_string(),
        used,
        window,
        percent,
        categories,
        mcp_tools: list_of(v, "mcp_tools").map(|t| McpTool { name: str_of(t, &["name"]).to_string(), server: str_of(t, &["server_name", "server"]).to_string(), tokens: tokens_of(t) }).filter(|t| !t.name.is_empty()).collect(),
        memory_files: list_of(v, "memory_files").map(|f| MemoryFile { kind: str_of(f, &["type", "kind"]).to_string(), path: str_of(f, &["path"]).to_string(), tokens: tokens_of(f) }).filter(|f| !f.path.is_empty()).collect(),
        agents: list_of(v, "agents").map(|a| CustomAgent { name: str_of(a, &["agent_type", "name"]).to_string(), source: source_word(str_of(a, &["source"])), tokens: tokens_of(a) }).filter(|a| !a.name.is_empty()).collect(),
        skills: list_of(v, "skills").map(|s| Skill { name: str_of(s, &["name"]).to_string(), source: source_word(str_of(s, &["source"])), tokens: tokens_of(s) }).filter(|s| !s.name.is_empty()).collect(),
    })
}

/// A source as the markdown words it ("userSettings" → "User", "plugin" → "Plugin").
fn source_word(raw: &str) -> String {
    match raw {
        "userSettings" => "User".into(),
        "projectSettings" => "Project".into(),
        "localSettings" => "Local".into(),
        "policySettings" => "Managed".into(),
        "builtin" | "built-in" => "Built-in".into(),
        other => {
            let mut c = other.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
        }
    }
}

/// `part` of `whole` in percent, to one decimal place.
fn share(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    (part as f64 * 1000.0 / whole as f64).round() / 10.0
}

/// **A token count as the markdown writes it**: `4,324`, `628`, `15k`,
/// `32.2k`, `1m`, `~240`, `< 20`. Pure.
pub fn count(raw: &str) -> Option<u64> {
    let t: String = raw.trim().trim_start_matches(['~', '<', '≈', ' ']).chars().filter(|c| *c != ',' && !c.is_whitespace()).collect();
    let lower = t.to_ascii_lowercase();
    let (num, scale) = if let Some(n) = lower.strip_suffix('k') {
        (n, 1_000.0)
    } else if let Some(n) = lower.strip_suffix('m') {
        (n, 1_000_000.0)
    } else {
        (lower.as_str(), 1.0)
    };
    let n: f64 = num.parse().ok()?;
    (n.is_finite() && n >= 0.0).then(|| (n * scale).round() as u64)
}

/// A markdown table under a `###` heading: its header's cells (lower case)
/// and its rows' cells.
struct Table {
    head: Vec<String>,
    rows: Vec<Vec<String>>,
}

impl Table {
    /// The column whose header starts with one of `names`.
    fn col(&self, names: &[&str]) -> Option<usize> {
        self.head.iter().position(|h| names.iter().any(|n| h.starts_with(n)))
    }

    fn cell(row: &[String], at: Option<usize>) -> &str {
        at.and_then(|i| row.get(i)).map_or("", String::as_str)
    }
}

fn cells(line: &str) -> Vec<String> {
    let inner = line.trim().trim_start_matches('|').trim_end_matches('|');
    inner.split('|').map(|c| c.trim().to_string()).collect()
}

/// Every `### Heading` and the first table under it (headings lower case).
fn tables(md: &str) -> HashMap<String, Table> {
    fn done(heading: &Option<String>, table: &mut Option<Table>, out: &mut HashMap<String, Table>) {
        if let (Some(h), Some(t)) = (heading, table.take()) {
            out.entry(h.clone()).or_insert(t);
        }
    }
    let mut out = HashMap::new();
    let mut heading: Option<String> = None;
    let mut table: Option<Table> = None;
    for line in md.lines() {
        let l = line.trim();
        if let Some(h) = l.strip_prefix("###") {
            done(&heading, &mut table, &mut out);
            heading = Some(h.trim().trim_start_matches('#').trim().to_lowercase());
            continue;
        }
        if l.starts_with("## ") {
            done(&heading, &mut table, &mut out);
            heading = None;
            continue;
        }
        if !l.starts_with('|') {
            if table.as_ref().is_some_and(|t| !t.rows.is_empty() || !t.head.is_empty()) && !l.is_empty() {
                done(&heading, &mut table, &mut out);
            }
            continue;
        }
        let row = cells(l);
        if row.iter().all(|c| !c.is_empty() && c.chars().all(|ch| matches!(ch, '-' | ':' | ' '))) {
            continue;
        }
        match &mut table {
            None => table = Some(Table { head: row.iter().map(|c| c.to_lowercase()).collect(), rows: Vec::new() }),
            Some(t) => t.rows.push(row),
        }
    }
    done(&heading, &mut table, &mut out);
    out
}

/// **`/context`'s markdown** (2.1.296's result: `## Context Usage`, a
/// `**Model:**` line, `**Tokens:** 32.2k / 1m (3%)`, then `### Estimated
/// usage by category` | Category | Tokens | Percentage |, `### MCP Tools`
/// | Tool | Server | Tokens |, `### Custom Agents` | Agent Type | Source |
/// Tokens |, `### Memory Files` | Type | Path | Tokens |, `### Skills` |
/// Skill | Source | Tokens |). Columns are found by their headers. `None`
/// without the tokens line or the categories. Pure.
pub fn parse_markdown(md: &str) -> Option<Breakdown> {
    let mut model = String::new();
    let mut totals = None;
    for line in md.lines() {
        let l = line.trim().replace("**", "");
        if let Some(m) = l.strip_prefix("Model:") {
            model = m.trim().to_string();
        } else if let Some(t) = l.strip_prefix("Tokens:") {
            // "32.2k / 1m (3%)"
            let (pair, pct) = t.split_once('(').map_or((t, None), |(a, b)| (a, Some(b)));
            let (used, window) = pair.split_once('/')?;
            let percent = pct.and_then(|p| p.split('%').next()).and_then(|p| p.trim().parse::<f64>().ok());
            totals = Some((count(used)?, count(window)?, percent));
        }
    }
    let (used, window, percent) = totals?;
    if window == 0 {
        return None;
    }
    let all = tables(md);
    let cats = all.iter().find(|(h, _)| h.contains("by category")).map(|(_, t)| t)?;
    let (name_at, tokens_at) = (cats.col(&["category"]).or(Some(0)), cats.col(&["tokens"]).or(Some(1)));
    let categories: Vec<Category> = cats
        .rows
        .iter()
        .filter_map(|r| {
            let name = Table::cell(r, name_at);
            let tokens = count(Table::cell(r, tokens_at))?;
            (!name.is_empty()).then(|| Category { name: name.to_string(), tokens, kind: Kind::of(None, name) })
        })
        .collect();
    if categories.is_empty() {
        return None;
    }
    let rows = |heading: &str, cols: [&[&str]; 3]| -> Vec<(String, String, u64)> {
        let Some(t) = all.get(heading) else { return Vec::new() };
        let at = cols.map(|c| t.col(c));
        t.rows.iter().filter(|r| !Table::cell(r, at[0]).is_empty()).map(|r| (Table::cell(r, at[0]).to_string(), Table::cell(r, at[1]).to_string(), count(Table::cell(r, at[2])).unwrap_or(0))).collect()
    };
    Some(Breakdown {
        model,
        used,
        window,
        percent: percent.unwrap_or_else(|| share(used, window)),
        categories,
        mcp_tools: rows("mcp tools", [&["tool"], &["server"], &["tokens"]]).into_iter().map(|(name, server, tokens)| McpTool { name, server, tokens }).collect(),
        // (Its first column is the type, its second the path.)
        memory_files: rows("memory files", [&["path"], &["type"], &["tokens"]]).into_iter().map(|(path, kind, tokens)| MemoryFile { kind, path, tokens }).collect(),
        agents: rows("custom agents", [&["agent"], &["source"], &["tokens"]]).into_iter().map(|(name, source, tokens)| CustomAgent { name, source, tokens }).collect(),
        skills: rows("skills", [&["skill"], &["source"], &["tokens"]]).into_iter().map(|(name, source, tokens)| Skill { name, source, tokens }).collect(),
    })
}

// ---- Running it ---------------------------------------------------------------------------------

/// The person's hooks off for this run: nothing of theirs runs because the summary opened.
fn settings() -> Value {
    serde_json::json!({ "disableAllHooks": true })
}

/// **`/context`'s arguments** for `session` (`settings`: the file holding
/// [`settings`]), on `model` when the chat has one. Never persisted. Pure.
pub fn args(session: &str, model: Option<&str>, settings: Option<&Path>) -> Vec<String> {
    let mut a: Vec<String> = ["-p", "/context", "--resume", session, "--no-session-persistence", "--output-format", "stream-json", "--verbose"].iter().map(|s| s.to_string()).collect();
    if let Some(m) = model.map(str::trim).filter(|m| !m.is_empty()) {
        a.push("--model".into());
        a.push(m.to_string());
    }
    if let Some(f) = settings {
        a.push("--settings".into());
        a.push(f.display().to_string());
    }
    a
}

/// The chat's own Claude: its agent in the chat's workspace whose turns go
/// straight to its session, once it has one Claude Code still keeps.
fn chats_session(conversation: u64) -> Option<Agent> {
    let folder = super::code_chat::folder_of(Some(conversation))?;
    let agent = crate::chat::tools::agents::chats_agent_in(Some(conversation), &folder)?;
    let direct = agent.turn_list.iter().any(|t| t.direct);
    // (A Rewind waiting for its next turn: the session as it stands isn't the chat's.)
    let ready = direct && agent.started_ts > 0 && !agent.session.is_empty() && agent.branch.is_none();
    (ready && crate::ai::claude_code::transcripts::session_kept(Path::new(&agent.path), &agent.session) != Some(false)).then_some(agent)
}

/// At work now: a turn queued, running or asking.
fn busy(agent: &Agent) -> bool {
    matches!(agent.status, Status::Queued | Status::Running | Status::Asking)
}

/// Where a breakdown came from: its session and turns, so a turn's end asks again.
fn key_of(agent: &Agent) -> String {
    format!("{}|{}|{}|{}", agent.session, agent.turns, agent.ended_ts, agent.model)
}

struct Kept {
    key: String,
    at: Instant,
    breakdown: Breakdown,
}

static KEPT: Mutex<Option<HashMap<u64, Kept>>> = Mutex::new(None);
/// Claude Codes (program and version) that sent `/context` to the model.
static NOT_LOCAL: Mutex<Option<HashSet<String>>> = Mutex::new(None);
/// One `/context` at a time.
static ONE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn kept(conversation: u64, key: Option<&str>) -> Option<Breakdown> {
    let g = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    let k = g.as_ref()?.get(&conversation)?;
    match key {
        Some(key) if k.key != key || k.at.elapsed() > FRESH_FOR => None,
        _ => Some(k.breakdown.clone()),
    }
}

/// Run `/context` for `agent`'s session with `program`: its breakdown, or why not.
fn run(program: &Program, agent: &Agent) -> Result<Breakdown, String> {
    let version = cli::status().version.map(|v| v.to_string()).unwrap_or_default();
    let who = format!("{}|{version}", program.path().display());
    if NOT_LOCAL.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|s| s.contains(&who)) {
        return Err("this Claude Code doesn't answer /context itself".into());
    }
    // The chat's own way of paying (no model is called), its key only for a chat on one.
    let cfg = crate::config::load();
    let key = cfg.provider_api_key("anthropic", "");
    let pay = if agent.pay == "key" && !key.trim().is_empty() { Pay::ApiKey(session::ApiKey::new(key)) } else { Pay::Plan };
    let mut spec = Spec::new(&agent.path, Pay::Plan);
    spec.settings = Some(settings());
    spec.conversation = Conversation::Resume(agent.session.clone());
    let files = RunFiles::write(&spec)?;
    let extra: Vec<(String, String)> = super::runner::TODO_ENV.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let env: Vec<(OsString, OsString)> = session::environment(std::env::vars_os(), &pay, &extra)?;
    let model = Some(agent.model.as_str());
    let a = args(&agent.session, model, files.files().settings.as_deref());
    let a: Vec<&str> = a.iter().map(String::as_str).collect();
    let out = cli::run_captured_at(program, &a, Place { dir: agent.path.clone().into(), env, what: "Claude Code" }, TIMEOUT)?;
    drop(files);
    let answer = parse_output(&out.stdout);
    if !answer.local {
        if out.stdout.contains("\"type\":\"result\"") {
            tracing::warn!("Code chat: /context wasn't answered by Claude Code itself: not asked again");
            NOT_LOCAL.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashSet::new).insert(who);
        }
        return Err(format!("no local answer (exit {:?})", out.code));
    }
    answer.breakdown.ok_or_else(|| "its answer couldn't be read".to_string())
}

/// **The summary's Context, broken down**: Claude Code's `/context` for
/// Code chat `conversation`'s own session — the last one while its Claude
/// works, a new one once a turn has ended (at most every 5 minutes
/// otherwise). `None` (the panel keeps its one number): no such session, no
/// Claude Code, or an answer that couldn't be read. Never an error.
#[tauri::command]
pub async fn code_chat_context(conversation: u64) -> Option<Breakdown> {
    let agent = tauri::async_runtime::spawn_blocking(move || chats_session(conversation)).await.ok().flatten()?;
    if busy(&agent) {
        return kept(conversation, None);
    }
    let key = key_of(&agent);
    if let Some(b) = kept(conversation, Some(&key)) {
        return Some(b);
    }
    let _one = ONE.lock().await;
    // (Another ask may have run it meanwhile.)
    if let Some(b) = kept(conversation, Some(&key)) {
        return Some(b);
    }
    let program = cli::find()?;
    let id = agent.id.clone();
    let got = tauri::async_runtime::spawn_blocking(move || run(&program, &agent)).await.ok()?;
    match got {
        Ok(b) => {
            tracing::info!(agent = %id, categories = b.categories.len(), mcp_tools = b.mcp_tools.len(), skills = b.skills.len(), "Code chat: its context, broken down");
            KEPT.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(conversation, Kept { key, at: Instant::now(), breakdown: b.clone() });
            Some(b)
        }
        Err(e) => {
            tracing::info!(agent = %id, "Code chat: no breakdown of its context ({e})");
            kept(conversation, None)
        }
    }
}

#[cfg(test)]
#[path = "context/tests.rs"]
mod tests;

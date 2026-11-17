//! **A Code chat's + menu** (17 Nov 2026, after the Claude Code app's): what
//! the chat's Claude Code offers in its workspace — its slash commands and
//! skills, its MCP servers and connectors, its plugins — and the chat's own
//! choices: **more folders** it may use (`--add-dir`), and **servers
//! switched off** for it (`--disallowedTools mcp__<server>`).
//!
//! - **What it offers, per workspace** (`code_menu.json`'s `workspaces`):
//!   - read from every run's `system/init` ([`learn_init`]: `slash_commands`
//!     less `terminal_slash_commands`, `skills`, `plugins`, `mcp_servers`,
//!     as 2.1.296 sends them) and the `system/commands_changed` after it
//!     ([`learn_commands`]: each command's description, argument hint and
//!     whether it's Claude Code's own);
//!   - **before a workspace's first run, a look** ([`probe`]): Claude Code
//!     started there with no message, asked the Agent SDK's documented
//!     `initialize` (`supportedCommands()`: its commands) and `mcp_status`
//!     (`mcpServerStatus()`: its servers) control requests, then ended — no
//!     model call. Only `commands` and `mcpServers` are read: the rest
//!     (the account, the models) never is. Its plugins:
//!     `claude plugin list --json` there (`plugins.rs`). Looked at again a
//!     day later, or once a plugin is switched in Settings ([`stale`]);
//!   - a plugin's one line: its own `.claude-plugin/plugin.json`'s
//!     `description` (the plugin's documented manifest).
//! - **The chat's choices** (`code_menu.json`'s `chats`, never agents.json or
//!   chats.json: an older Yap sharing the data folder drops fields it
//!   doesn't know):
//!   - **its folders** ([`add_folder`]): Claude Code may read and change them
//!     as it does the workspace, in every run of the chat (its own session
//!     and the agents sent from it). Never a whole drive, Windows' own
//!     folders, the folder of every account, nor one that holds Yap's own
//!     data or is inside it ([`refused`]);
//!   - **its servers off** ([`set_server`]): their tools disallowed in its
//!     runs (`mcp__<name>`, the documented rule for a whole server; checked
//!     on 2.1.296: the session's tools leave out the server's). Yap's own
//!     endpoint (`claude_live::SERVER`) is never listed;
//!   - a new code chat's (no conversation yet) are kept for its workspace
//!     until its first question makes the conversation ([`adopt_new`]); a
//!     chat deleted loses them, a Fork from here copies them.
//! - Every change goes to every window as **`yap-code-menu`**
//!   (`{workspace?, conversation?}`).
//!
//! The log has counts, never a folder's path, a command's or a server's name.

#[cfg(not(test))]
use std::sync::Mutex;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::ai::claude_code::events::{Event, Init, McpServer};
use crate::ai::claude_code::session::{Pay, Session, Spec};

use super::workspaces::{self, norm};

/// Every window hears of a workspace's menu or a chat's choices changing.
pub const EVENT: &str = "yap-code-menu";

/// **A look is a probe** (Yap's own variable, which Claude Code ignores):
/// Yap's stand-in for Claude Code answers its control requests and plays no
/// script (`claude_code/fake.rs`).
pub const PROBE_VAR: &str = "YAP_CLAUDE_PROBE";

/// How long a look waits for Claude Code's answers.
#[cfg_attr(test, allow(dead_code))]
const PROBE_WAIT: Duration = Duration::from_secs(30);
/// Servers still starting: asked about again this long later, at most this
/// many times.
#[cfg_attr(test, allow(dead_code))]
const PROBE_GAP: Duration = Duration::from_millis(1500);
#[cfg_attr(test, allow(dead_code))]
const PROBE_AGAIN: u32 = 6;
/// What a workspace's menu is good for before it's looked at again (a run's
/// init keeps it fresh meanwhile).
const KEEP_SECS: u64 = 24 * 60 * 60;
/// A command's description, cut.
const LINE_MAX: usize = 300;

/// **The commands Claude Code 2.1.296 runs only in its own terminal**, until
/// a run's init says which (`terminal_slash_commands`): a look's list has no
/// such field.
const TERMINAL_ONLY: &[&str] = &["doctor", "color", "focus", "reload-plugins"];

/// **One command or skill after "/"**.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Command {
    /// As typed after "/": `review`, `garden-tools:water`.
    pub name: String,
    /// Claude Code's one line (a plugin's "(garden-tools) " taken off).
    pub description: String,
    /// What it takes after its name (`argumentHint`).
    pub hint: String,
    /// Claude Code's own (`builtin`).
    pub builtin: bool,
    /// One of the session's skills.
    pub skill: bool,
    /// The plugin it comes with (empty: none).
    pub plugin: String,
    /// Its other names (`aliases`; one list's name in another's: a run's
    /// init calls `docs` `anthropic-skills:docs`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
}

/// **An MCP server or connector** the session has.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Server {
    /// Claude Code's name for it ("claude.ai Example Docs", "plugin:p:s").
    pub name: String,
    /// As shown: "Example Docs", "s".
    pub label: String,
    /// `connected`, `pending`, `failed`, `needs-auth`, `disabled`, …
    pub status: String,
    /// `user`, `project`, `local`, `plugin`, `claudeai`, `dynamic`, …
    pub source: String,
}

/// **A plugin the session loaded.**
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Plugin {
    pub name: String,
    /// `name@marketplace`.
    pub id: String,
    pub version: String,
    /// Its manifest's one line (`.claude-plugin/plugin.json`).
    pub description: String,
    /// Where it's installed (never sent to the page).
    #[serde(skip_serializing)]
    pub path: String,
}

/// **What Claude Code offers in a workspace.**
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Offered {
    pub commands: Vec<Command>,
    pub servers: Vec<Server>,
    pub plugins: Vec<Plugin>,
    /// The commands that work only in Claude Code's terminal (left out).
    pub terminal: Vec<String>,
    /// Unix seconds it was read; 0: look again at the next menu.
    pub at: u64,
    /// `init` (a run's), `probe` (a look).
    pub from: String,
}

/// **A Code chat's own choices.**
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Choices {
    /// More folders its runs may use (full paths, as picked).
    pub folders: Vec<String>,
    /// Servers switched off for it (Claude Code's names).
    pub servers_off: Vec<String>,
    /// **Its `/clear`s** (`slash.rs`, 17 Nov 2026), oldest first.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub clears: Vec<Clear>,
}

/// **A `/clear` in a Code chat**: Claude starts afresh from message `at`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Clear {
    /// The thread's divider goes above this message (how many there were).
    pub at: usize,
    /// Unix seconds.
    pub ts: u64,
    /// The chat's agents then (their sessions are never carried on again).
    pub agents: Vec<String>,
}

impl Choices {
    fn is_empty(&self) -> bool {
        self.folders.is_empty() && self.servers_off.is_empty() && self.clears.is_empty()
    }
}

/// **Agent `agent` of chat `conversation` was cleared** (`/clear`): the chat's
/// next message starts a new one.
pub fn cleared(conversation: u64, agent: &str) -> bool {
    with_store(|s| s.chats.get(&conversation).is_some_and(|c| c.clears.iter().any(|cl| cl.agents.iter().any(|a| a == agent))))
}

/// **A `/clear` kept** for chat `conversation` (`slash.rs`): `agents` never
/// carried on again, the divider above message `at`.
pub fn add_clear(conversation: u64, at: usize, agents: Vec<String>) {
    with_store(|s| {
        let ch = s.chats.entry(conversation).or_default();
        ch.clears.push(Clear { at, ts: super::store::now_secs(), agents });
        save(s);
    });
    emit(None, Some(conversation));
}

/// The folders chat `conversation` added that are still on this PC (the
/// plan's read-only session in a Code chat that doesn't work directly:
/// `chat/turn/plan.rs`).
pub fn folders_for_run(conversation: u64) -> Vec<PathBuf> {
    choices(Some(conversation), None).folders.iter().map(PathBuf::from).filter(|p| p.is_dir()).collect()
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Store {
    /// Normalised workspace path → what's offered there.
    workspaces: BTreeMap<String, Offered>,
    /// Conversation id → its choices.
    chats: BTreeMap<u64, Choices>,
}

/// code_menu.json's saves (a cache, mostly: a failed save is only logged).
pub static SAVER: crate::shell::saving::Saver = crate::shell::saving::Saver::quiet("code_menu", "your Code chats' folders and servers", save_now);

fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    with_store(|s| save(s));
    SAVER.check()
}

#[cfg(not(test))]
fn path() -> PathBuf {
    super::store::data().join("code_menu.json")
}

#[cfg(not(test))]
static STATE: Mutex<Option<Store>> = Mutex::new(None);

#[cfg(not(test))]
fn save(store: &Store) {
    if crate::ai::eval::active() {
        return;
    }
    let _ = SAVER.write_json(&path(), store, false);
}

#[cfg(not(test))]
fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let store = guard.get_or_insert_with(|| SAVER.load(&path()));
    if SAVER.any_unread() && SAVER.refresh(&path(), store) {
        save(store);
    }
    f(store)
}

#[cfg(test)]
thread_local! {
    static TEST_STORE: std::cell::RefCell<Store> = std::cell::RefCell::new(Store::default());
}

#[cfg(test)]
fn save(_: &Store) {}

#[cfg(test)]
fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    TEST_STORE.with(|s| f(&mut s.borrow_mut()))
}

/// A new code chat's choices, by its workspace (until its first question
/// makes its conversation: [`adopt_new`]). Not saved.
#[cfg(not(test))]
static PENDING: Mutex<BTreeMap<String, Choices>> = Mutex::new(BTreeMap::new());

#[cfg(test)]
thread_local! {
    static TEST_PENDING: std::cell::RefCell<BTreeMap<String, Choices>> = const { std::cell::RefCell::new(BTreeMap::new()) };
}

fn with_pending<R>(f: impl FnOnce(&mut BTreeMap<String, Choices>) -> R) -> R {
    #[cfg(not(test))]
    {
        f(&mut PENDING.lock().unwrap_or_else(|p| p.into_inner()))
    }
    #[cfg(test)]
    {
        TEST_PENDING.with(|p| f(&mut p.borrow_mut()))
    }
}

fn emit(workspace: Option<&str>, conversation: Option<u64>) {
    if let Some(app) = super::runner::app() {
        use tauri::Emitter;
        let _ = app.emit(EVENT, json!({ "workspace": workspace, "conversation": conversation }));
    }
}

// ---- what's offered ----

/// **A server's name as shown**: a claude.ai connector without "claude.ai ",
/// a plugin's (`plugin:<plugin>:<server>`) by its own name. Pure.
pub fn label_of(name: &str) -> String {
    if let Some(rest) = name.strip_prefix("claude.ai ") {
        return rest.trim().to_string();
    }
    if let Some(rest) = name.strip_prefix("plugin:") {
        if let Some(server) = rest.rsplit(':').next().filter(|s| !s.is_empty()) {
            return server.to_string();
        }
    }
    name.to_string()
}

/// **The rule that disallows a whole server's tools** (`--disallowedTools`):
/// `mcp__` and its name with every character but letters, digits, `_` and
/// `-` made `_`, as Claude Code names its tools ("claude.ai Example Docs" →
/// `mcp__claude_ai_Example_Docs`, checked on 2.1.296). Pure.
pub fn tool_rule(server: &str) -> String {
    let name: String = server.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    format!("mcp__{name}")
}

/// The session's servers, Yap's own endpoint left out. Pure.
pub fn servers_of(list: &[McpServer]) -> Vec<Server> {
    list.iter()
        .filter(|s| !s.name.trim().is_empty() && s.name != crate::integrations::claude_live::SERVER)
        .map(|s| Server { name: s.name.clone(), label: label_of(&s.name), status: s.status.clone(), source: s.source.clone() })
        .collect()
}

/// A command's one line, tidied: its plugin's "(name) " off, one line, cut. Pure.
fn line_of(description: &str, plugin: &str) -> String {
    let mut d = description.trim();
    if !plugin.is_empty() {
        if let Some(rest) = d.strip_prefix(&format!("({plugin})")) {
            d = rest.trim_start();
        }
    }
    let one: String = d.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= LINE_MAX {
        return one;
    }
    let cut: String = one.chars().take(LINE_MAX).collect();
    format!("{}…", cut.trim_end())
}

/// The plugin `name` comes with (`plugin:skill`, a plugin the session has). Pure.
fn plugin_of(name: &str, plugins: &[String]) -> String {
    match name.split_once(':') {
        Some((p, _)) if plugins.iter().any(|q| q == p) => p.to_string(),
        _ => String::new(),
    }
}

/// Whether a command is one to keep: not one of Claude Code's own plumbing
/// (`__…`), not a terminal's. Pure.
fn listed(name: &str, terminal: &[String]) -> bool {
    !name.is_empty() && !name.starts_with("__") && !terminal.iter().any(|t| t == name)
}

/// One Claude Code says it removed ("(removed) …"): kept, so a later init's
/// bare name is still known, but never shown. Pure.
fn removed(c: &Command) -> bool {
    c.description.trim_start().starts_with("(removed)")
}

/// `c` answers to `name`: its own, or one of its aliases (2.1.296's init
/// names a skill `anthropic-skills:docs` that `initialize` calls `docs`). Pure.
fn answers_to(c: &Command, name: &str) -> bool {
    c.name == name || c.aliases.iter().any(|a| a == name)
}

/// **What the page shows** of `offered`: the removed ones left out.
pub fn shown(mut offered: Offered) -> Offered {
    offered.commands.retain(|c| !removed(c));
    offered
}

/// **What a run's `system/init` says is offered**, with what `old` knew of
/// each command (its line, its hint) kept. Pure.
pub fn from_init(init: &Init, old: Option<&Offered>, now: u64) -> Offered {
    let plugins: Vec<Plugin> = init
        .plugins
        .iter()
        .filter(|p| !p.source.ends_with("@builtin"))
        .map(|p| {
            let known = old.and_then(|o| o.plugins.iter().find(|q| q.name == p.name && q.path == p.path));
            Plugin { name: p.name.clone(), id: p.source.clone(), version: p.version.clone(), description: known.map(|k| k.description.clone()).unwrap_or_default(), path: p.path.clone() }
        })
        .collect();
    let names: Vec<String> = init.plugins.iter().map(|p| p.name.clone()).collect();
    let terminal = if init.terminal_slash_commands.is_empty() { old.map(|o| o.terminal.clone()).unwrap_or_default() } else { init.terminal_slash_commands.clone() };
    let commands = if init.slash_commands.is_empty() {
        // (A Claude Code whose init lists none: what was known stays.)
        old.map(|o| o.commands.clone()).unwrap_or_default()
    } else {
        init.slash_commands
            .iter()
            .filter(|n| listed(n, &terminal))
            .map(|n| {
                let was = old.and_then(|o| o.commands.iter().find(|c| answers_to(c, n)));
                Command {
                    name: n.clone(),
                    description: was.map(|c| c.description.clone()).unwrap_or_default(),
                    hint: was.map(|c| c.hint.clone()).unwrap_or_default(),
                    builtin: was.is_some_and(|c| c.builtin),
                    skill: init.skills.iter().any(|s| s == n),
                    plugin: plugin_of(n, &names),
                    aliases: was.map(|c| c.aliases.iter().chain(std::iter::once(&c.name)).filter(|a| *a != n).cloned().collect()).unwrap_or_default(),
                }
            })
            .collect()
    };
    Offered { commands, servers: servers_of(&init.mcp_servers), plugins, terminal, at: now, from: "init".into() }
}

/// **Commands with their details** (`commands_changed`'s, or `initialize`'s
/// answer: `{name, description, argumentHint, builtin}` each) laid over
/// `offered`: each one's line, hint and whether it's Claude Code's own; the
/// list itself taken from them when `offered` has none, or came from a look.
/// Pure.
pub fn with_details(mut offered: Offered, list: &[Value]) -> Offered {
    let terminal = if offered.terminal.is_empty() { TERMINAL_ONLY.iter().map(|s| s.to_string()).collect() } else { offered.terminal.clone() };
    let names: Vec<String> = offered.plugins.iter().map(|p| p.name.clone()).collect();
    let detail = |v: &Value| -> Option<Command> {
        let name = v.get("name").and_then(Value::as_str)?.trim().to_string();
        let raw = v.get("description").and_then(Value::as_str).unwrap_or_default();
        // A plugin's command, named for one the session hasn't said it has yet: by its "(plugin) " line.
        let mut plugin = plugin_of(&name, &names);
        if plugin.is_empty() {
            if let Some((p, _)) = name.split_once(':') {
                if raw.trim_start().starts_with(&format!("({p})")) {
                    plugin = p.to_string();
                }
            }
        }
        Some(Command {
            description: line_of(raw, &plugin),
            hint: v.get("argumentHint").and_then(Value::as_str).unwrap_or_default().trim().to_string(),
            builtin: v.get("builtin").and_then(Value::as_bool).unwrap_or(false),
            skill: false,
            plugin,
            aliases: v.get("aliases").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default(),
            name,
        })
    };
    let details: Vec<Command> = list.iter().filter_map(detail).collect();
    if offered.commands.is_empty() || offered.from == "probe" {
        let skills: Vec<(String, bool)> = offered.commands.iter().map(|c| (c.name.clone(), c.skill)).collect();
        offered.commands = details
            .into_iter()
            .filter(|c| listed(&c.name, &terminal))
            .map(|mut c| {
                c.skill = skills.iter().any(|(n, s)| n == &c.name && *s);
                c
            })
            .collect();
    } else {
        for c in offered.commands.iter_mut() {
            // By its name, or one of either's aliases (`anthropic-skills:docs` is `docs`).
            if let Some(d) = details.iter().find(|d| answers_to(d, &c.name) || c.aliases.iter().any(|a| answers_to(d, a))) {
                c.description = d.description.clone();
                c.hint = d.hint.clone();
                c.builtin = d.builtin;
                if c.plugin.is_empty() {
                    c.plugin = d.plugin.clone();
                }
                for a in d.aliases.iter().chain(std::iter::once(&d.name)) {
                    if *a != c.name && !c.aliases.contains(a) {
                        c.aliases.push(a.clone());
                    }
                }
            }
        }
    }
    offered
}

/// **A look's answers** (`initialize`'s `commands`, `mcp_status`'s
/// `mcpServers`) and the plugins `claude plugin list` gave. Pure.
pub fn from_probe(commands: &[Value], servers: &[Value], plugins: Vec<Plugin>, now: u64) -> Offered {
    let servers: Vec<McpServer> = servers
        .iter()
        .map(|s| McpServer {
            name: s.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
            status: s.get("status").and_then(Value::as_str).unwrap_or_default().to_string(),
            source: s.get("source").and_then(Value::as_str).or_else(|| s.get("scope").and_then(Value::as_str)).unwrap_or_default().to_string(),
        })
        .collect();
    let base = Offered { servers: servers_of(&servers), plugins, terminal: TERMINAL_ONLY.iter().map(|s| s.to_string()).collect(), at: now, from: "probe".into(), ..Default::default() };
    with_details(base, commands)
}

/// A plugin's one line from its manifest (`.claude-plugin/plugin.json`).
pub fn manifest_line(path: &str) -> String {
    if path.trim().is_empty() {
        return String::new();
    }
    let file = Path::new(path).join(".claude-plugin").join("plugin.json");
    let Ok(meta) = std::fs::metadata(&file) else { return String::new() };
    if meta.len() > 256 * 1024 {
        return String::new();
    }
    std::fs::read_to_string(&file)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get("description").and_then(Value::as_str).map(|d| line_of(d, "")))
        .unwrap_or_default()
}

/// Each plugin's line read where it's not known yet.
fn describe(plugins: &mut [Plugin]) {
    for p in plugins.iter_mut().filter(|p| p.description.is_empty()) {
        p.description = manifest_line(&p.path);
    }
}

/// **A run's `system/init`** in `workspace`: what it offers there, kept.
pub fn learn_init(workspace: &str, init: &Init) {
    let key = norm(workspace);
    if key.is_empty() {
        return;
    }
    let now = super::store::now_secs();
    let changed = with_store(|s| {
        let old = s.workspaces.get(&key);
        let mut fresh = from_init(init, old, now);
        // (An init that names no commands, and none known: still looked at, so the menu isn't empty for a day.)
        if fresh.commands.is_empty() {
            fresh.at = 0;
        }
        describe(&mut fresh.plugins);
        let same = old.is_some_and(|o| o.commands == fresh.commands && o.servers == fresh.servers && o.plugins == fresh.plugins && o.terminal == fresh.terminal);
        s.workspaces.insert(key.clone(), fresh);
        if !same {
            save(s);
        }
        !same
    });
    if changed {
        tracing::debug!(commands = init.slash_commands.len(), servers = init.mcp_servers.len(), plugins = init.plugins.len(), "Code menu: a run's init read");
        emit(Some(workspace), None);
    }
}

/// **A run's `system/commands_changed`** in `workspace` (its `commands`):
/// each command's line and hint, kept.
pub fn learn_commands(workspace: &str, event: &Value) {
    let Some(list) = event.get("commands").and_then(Value::as_array) else { return };
    let key = norm(workspace);
    if key.is_empty() {
        return;
    }
    let changed = with_store(|s| {
        let old = s.workspaces.get(&key).cloned().unwrap_or_default();
        let fresh = with_details(old.clone(), list);
        if fresh == old {
            return false;
        }
        s.workspaces.insert(key.clone(), fresh);
        save(s);
        true
    });
    if changed {
        emit(Some(workspace), None);
    }
}

/// **Every workspace's menu looked at again** at its next opening (a plugin
/// switched in Settings → Yap Code).
pub fn stale() {
    with_store(|s| {
        for o in s.workspaces.values_mut() {
            o.at = 0;
        }
        save(s);
    });
}

/// What's known for `workspace`, and whether to look again.
fn known(workspace: &str, now: u64) -> (Option<Offered>, bool) {
    let key = norm(workspace);
    let found = with_store(|s| s.workspaces.get(&key).cloned());
    let again = match &found {
        None => true,
        Some(o) => o.at == 0 || now.saturating_sub(o.at) > KEEP_SECS,
    };
    (found, again)
}

/// **A look at what Claude Code offers in `folder`** (see the module's
/// notes): no message, so no model call; the answers' commands and servers
/// only. (Unit tests never start one.)
#[cfg_attr(test, allow(dead_code))]
pub async fn probe(folder: PathBuf) -> Result<Offered, String> {
    let program = crate::ai::claude_code::cli::find().ok_or("Claude Code isn't installed on this PC.")?;
    if !folder.is_dir() {
        return Err("The workspace's folder isn't there.".into());
    }
    let mut spec = Spec::new(&folder, Pay::Plan);
    spec.persist = false;
    spec.env = vec![(PROBE_VAR.to_string(), "1".to_string())];
    let (session, mut rx) = Session::start_with(&program, spec)?;
    let asked_commands = session.control(json!({"subtype": "initialize"}))?;
    let mut asked_servers = session.control(json!({"subtype": "mcp_status"}))?;
    let mut commands: Option<Vec<Value>> = None;
    let mut servers: Option<Vec<Value>> = None;
    // Servers still connecting ("pending") are asked about again a moment later, a few times: the look's
    // process is young, and "Starting" would be all it said of most of them.
    let mut again = 0;
    let mut settled = false;
    let until = tokio::time::Instant::now() + PROBE_WAIT;
    while commands.is_none() || !settled {
        match tokio::time::timeout_at(until, rx.recv()).await {
            Ok(Some(Event::ControlResponse(r))) if r.request_id == asked_commands => {
                // (Only its commands: never the account or anything else it says.)
                commands = Some(if r.ok { r.response.get("commands").and_then(Value::as_array).cloned().unwrap_or_default() } else { Vec::new() });
            }
            Ok(Some(Event::ControlResponse(r))) if r.request_id == asked_servers => {
                let list = if r.ok { r.response.get("mcpServers").and_then(Value::as_array).cloned().unwrap_or_default() } else { Vec::new() };
                let pending = list.iter().any(|s| s.get("status").and_then(Value::as_str) == Some("pending"));
                servers = Some(list);
                if pending && again < PROBE_AGAIN && tokio::time::Instant::now() + PROBE_GAP < until {
                    again += 1;
                    tokio::time::sleep(PROBE_GAP).await;
                    match session.control(json!({"subtype": "mcp_status"})) {
                        Ok(id) => asked_servers = id,
                        Err(_) => settled = true,
                    }
                } else {
                    settled = true;
                }
            }
            Ok(Some(Event::Exited(_))) | Ok(None) | Err(_) => break,
            Ok(Some(_)) => {}
        }
    }
    session.kill();
    if commands.is_none() && servers.is_none() {
        return Err("Claude Code didn't say what it offers.".into());
    }
    let dir = folder.clone();
    let mut plugins = tauri::async_runtime::spawn_blocking(move || super::plugins::enabled_in(&program, &dir)).await.unwrap_or_default();
    describe(&mut plugins);
    let offered = from_probe(&commands.unwrap_or_default(), &servers.unwrap_or_default(), plugins, super::store::now_secs());
    tracing::info!(commands = offered.commands.len(), servers = offered.servers.len(), plugins = offered.plugins.len(), "Code menu: a workspace looked at");
    Ok(offered)
}

/// Looks in progress (normalised workspace paths): one at a time each.
#[cfg(not(test))]
static LOOKING: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// **A look at `workspace` started** in the background, unless one is
/// going; what it finds kept, every window told. Whether one is going now.
#[cfg(not(test))]
fn look(workspace: &str) -> bool {
    let key = norm(workspace);
    {
        let mut going = LOOKING.lock().unwrap_or_else(|p| p.into_inner());
        if going.contains(&key) {
            return true;
        }
        going.push(key.clone());
    }
    let path = workspace.to_string();
    tauri::async_runtime::spawn(async move {
        let found = probe(PathBuf::from(&path)).await;
        LOOKING.lock().unwrap_or_else(|p| p.into_inner()).retain(|k| k != &key);
        match found {
            Ok(offered) => {
                with_store(|s| {
                    // (A run's init since is fresher than a look.)
                    let newer = s.workspaces.get(&key).is_some_and(|o| o.from == "init" && o.at > 0 && super::store::now_secs().saturating_sub(o.at) < 60);
                    if !newer {
                        s.workspaces.insert(key.clone(), offered);
                        save(s);
                    }
                });
            }
            Err(e) => {
                tracing::info!("Code menu: couldn't look at a workspace ({e})");
                // (Not again at once: marked read, so the next opening doesn't ask again straight away.)
                with_store(|s| {
                    let entry = s.workspaces.entry(key.clone()).or_default();
                    if entry.at == 0 {
                        entry.at = super::store::now_secs().saturating_sub(KEEP_SECS - 10 * 60);
                    }
                });
            }
        }
        emit(Some(&path), None);
    });
    true
}

#[cfg(test)]
fn look(_: &str) -> bool {
    false
}

// ---- the chat's choices ----

/// Code chat `conversation`'s choices (none: a new chat's, its workspace's).
pub fn choices(conversation: Option<u64>, workspace: Option<&str>) -> Choices {
    match conversation {
        Some(c) => with_store(|s| s.chats.get(&c).cloned().unwrap_or_default()),
        None => workspace.map(|w| with_pending(|p| p.get(&norm(w)).cloned().unwrap_or_default())).unwrap_or_default(),
    }
}

/// Change a chat's choices (a new chat's: its workspace's, until it's made).
fn change(conversation: Option<u64>, workspace: Option<&str>, f: impl FnOnce(&mut Choices)) -> Result<Choices, String> {
    let out = match conversation {
        Some(c) => with_store(|s| {
            let mut ch = s.chats.get(&c).cloned().unwrap_or_default();
            f(&mut ch);
            if ch.is_empty() {
                s.chats.remove(&c);
            } else {
                s.chats.insert(c, ch.clone());
            }
            save(s);
            ch
        }),
        None => {
            let key = norm(workspace.ok_or("That isn't a Code chat.")?);
            with_pending(|p| {
                let mut ch = p.get(&key).cloned().unwrap_or_default();
                f(&mut ch);
                if ch.is_empty() {
                    p.remove(&key);
                } else {
                    p.insert(key.clone(), ch.clone());
                }
                ch
            })
        }
    };
    emit(workspace, conversation);
    Ok(out)
}

/// **A server on or off for a chat.**
pub fn set_server(conversation: Option<u64>, workspace: Option<&str>, server: &str, on: bool) -> Result<Choices, String> {
    let server = server.trim().to_string();
    if server.is_empty() || server == crate::integrations::claude_live::SERVER {
        return Err("That server can't be switched here.".into());
    }
    let out = change(conversation, workspace, |c| {
        c.servers_off.retain(|s| s != &server);
        if !on {
            c.servers_off.push(server.clone());
        }
    })?;
    tracing::info!(conversation = ?conversation, on, off = out.servers_off.len(), "Code menu: a server switched for a chat");
    Ok(out)
}

/// What can't be added as a chat's folder, and the folders that say so.
#[derive(Debug, Clone, Default)]
pub struct Guard {
    /// Windows' own folders (normalised): the Windows folder, Program Files,
    /// ProgramData.
    pub system: Vec<String>,
    /// Yap's own data folder (normalised).
    pub data: String,
    /// The folder of every account (`C:\Users`, normalised).
    pub users: Option<String>,
    /// The chat's workspace (normalised).
    pub workspace: String,
}

/// `inner` is `outer` or inside it (both normalised). Pure.
fn within(inner: &str, outer: &str) -> bool {
    !outer.is_empty() && (inner == outer || inner.strip_prefix(outer).is_some_and(|rest| rest.starts_with('\\')))
}

/// **Why `path` can't be a chat's folder** (workspace rules): never a whole
/// drive or share, Windows' own folders, the folder of every account, one
/// that holds Yap's own data or is inside it, or the chat's own workspace.
/// `None`: it can. Pure (on paths already resolved).
pub fn refused(path: &str, guard: &Guard) -> Option<&'static str> {
    if !workspaces::is_absolute(path) {
        return Some("Pick a folder by its full path.");
    }
    let p = norm(path);
    let drive_root = p.len() <= 2 && p.ends_with(':');
    let share_root = p.starts_with("\\\\") && p.trim_start_matches('\\').split('\\').filter(|s| !s.is_empty()).count() <= 2;
    if drive_root || share_root {
        return Some("A whole drive can't be added: pick a folder on it.");
    }
    if guard.system.iter().any(|s| within(&p, s)) {
        return Some("That's one of Windows' own folders: Claude Code is never given it.");
    }
    if within(&p, &guard.data) || within(&guard.data, &p) {
        return Some("That holds Yap's own data: Claude Code is never given it.");
    }
    if guard.users.as_deref().is_some_and(|u| u == p) {
        return Some("That's the folder of every account on this PC: pick a folder inside yours.");
    }
    if within(&p, &guard.workspace) {
        return Some("That's in this chat's workspace already.");
    }
    None
}

/// A path as Windows resolves it (links followed), `\\?\` off; else as given.
fn resolved(path: &Path) -> String {
    let real = std::fs::canonicalize(path).map(|p| p.display().to_string()).unwrap_or_else(|_| path.display().to_string());
    match real.strip_prefix(r"\\?\UNC\") {
        Some(rest) => format!(r"\\{rest}"),
        None => real.strip_prefix(r"\\?\").unwrap_or(&real).to_string(),
    }
}

/// This PC's guard for a chat in `workspace`.
fn guard_for(workspace: &str) -> Guard {
    let mut system: Vec<String> = ["SystemRoot", "windir", "ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "ProgramData"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|p| norm(&resolved(Path::new(&p))))
        .filter(|p| !p.is_empty())
        .collect();
    system.sort();
    system.dedup();
    let users = std::env::var_os("USERPROFILE").and_then(|p| Path::new(&p).parent().map(|u| norm(&resolved(u))));
    Guard { system, data: norm(&resolved(&crate::config::data_dir())), users, workspace: norm(&resolved(Path::new(workspace))) }
}

/// **A folder added to a chat** (its workspace `workspace`): checked
/// ([`refused`]), there, not added twice.
pub fn add_folder(conversation: Option<u64>, workspace: &str, picked: &Path) -> Result<Choices, String> {
    if !picked.is_dir() {
        return Err("That folder isn't there.".into());
    }
    let real = resolved(picked);
    if let Some(why) = refused(&real, &guard_for(workspace)) {
        return Err(why.into());
    }
    let shown = picked.display().to_string();
    let out = change(conversation, Some(workspace), |c| {
        if !c.folders.iter().any(|f| norm(&resolved(Path::new(f))) == norm(&real)) {
            c.folders.push(shown.clone());
        }
    })?;
    tracing::info!(conversation = ?conversation, folders = out.folders.len(), "Code menu: a folder added to a chat");
    Ok(out)
}

/// **A folder taken off a chat.**
pub fn remove_folder(conversation: Option<u64>, workspace: Option<&str>, path: &str) -> Result<Choices, String> {
    let want = norm(path);
    let out = change(conversation, workspace, |c| c.folders.retain(|f| norm(f) != want))?;
    tracing::info!(conversation = ?conversation, folders = out.folders.len(), "Code menu: a folder taken off a chat");
    Ok(out)
}

/// **What a chat's choices add to a run** (`spec`): its folders that are
/// still there (`--add-dir`), its servers off (`--disallowedTools`, after
/// the Mode's own). Pure but for `is_dir`.
pub fn apply_choices(spec: &mut Spec, choices: &Choices, is_dir: &dyn Fn(&Path) -> bool) {
    for f in &choices.folders {
        let p = PathBuf::from(f);
        if is_dir(&p) && !spec.add_dirs.contains(&p) {
            spec.add_dirs.push(p);
        }
    }
    for s in &choices.servers_off {
        let rule = tool_rule(s);
        if !spec.disallowed_tools.contains(&rule) {
            spec.disallowed_tools.push(rule);
        }
    }
}

/// **Chat `conversation`'s choices on a run** of it (its own session's or an
/// agent's sent from it).
pub fn apply(spec: &mut Spec, conversation: Option<u64>) {
    let Some(c) = conversation else { return };
    let choices = choices(Some(c), None);
    if choices.is_empty() {
        return;
    }
    apply_choices(spec, &choices, &|p| p.is_dir());
    tracing::info!(conversation = c, folders = spec.add_dirs.len(), off = choices.servers_off.len(), "Code menu: the chat's folders and servers on its run");
}

/// **A new code chat's first question made `conversation`** in `workspace`:
/// what its + menu chose meanwhile is its own now.
pub fn adopt_new(conversation: u64, workspace: &str) {
    let Some(ch) = with_pending(|p| p.remove(&norm(workspace))) else { return };
    if ch.is_empty() {
        return;
    }
    with_store(|s| {
        s.chats.insert(conversation, ch);
        save(s);
    });
    emit(Some(workspace), Some(conversation));
}

/// **A chat deleted**: its choices go (a later chat may get its id).
pub fn chat_deleted(conversation: u64) {
    let had = with_store(|s| {
        let had = s.chats.remove(&conversation).is_some();
        if had {
            save(s);
        }
        had
    });
    if had {
        emit(None, Some(conversation));
    }
}

/// **A Fork from here**: the new chat starts with the original's choices.
pub fn forked(from: u64, to: u64) {
    // (Never its /clears: they're the original's thread and agents.)
    let ch = Choices { clears: Vec::new(), ..choices(Some(from), None) };
    if ch.is_empty() {
        return;
    }
    with_store(|s| {
        s.chats.insert(to, ch);
        save(s);
    });
}

/// Reset (`privacy/reset.rs`): everything goes.
pub fn reset() {
    with_store(|s| {
        *s = Store::default();
        save(s);
    });
    with_pending(|p| p.clear());
}

pub fn saved() -> Result<(), crate::shell::saving::SaveError> {
    SAVER.check()
}

/// Download your data (`privacy/export.rs`): each chat's choices (its added
/// folders, its servers off), by conversation id; never the workspaces' lists.
pub fn export() -> Value {
    with_store(|s| serde_json::to_value(&s.chats).unwrap_or(Value::Null))
}

// ---- the page ----

/// One of the chat's folders, as the page shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderRow {
    pub path: String,
    pub name: String,
    /// Still on this PC (a gone one is skipped on its runs).
    pub exists: bool,
}

/// **The + menu's lists** for a chat (`code_menu`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Menu {
    pub workspace: String,
    /// What's known (none yet: being looked at, or Claude Code isn't there).
    pub offered: Option<Offered>,
    /// A look is going: the lists come with `yap-code-menu`.
    pub looking: bool,
    pub folders: Vec<FolderRow>,
    pub servers_off: Vec<String>,
}

fn rows(choices: &Choices) -> Vec<FolderRow> {
    choices.folders.iter().map(|f| FolderRow { path: f.clone(), name: workspaces::name_of(f), exists: Path::new(f).is_dir() }).collect()
}

/// **The folders chat `conversation` added** (a new chat's: its workspace's,
/// at `workspace`): the summary panel's Environment.
pub fn folder_rows(conversation: Option<u64>, workspace: &str) -> Vec<FolderRow> {
    rows(&choices(conversation, Some(workspace)))
}

/// The workspace a request means: chat `conversation`'s, else `workspace`
/// (a new chat's), while it's one of Yap Code's.
fn workspace_of(conversation: Option<u64>, workspace: Option<&str>) -> Result<String, String> {
    conversation
        .and_then(|c| super::code_chat::folder_of(Some(c)))
        .or_else(|| workspace.and_then(super::code_chat::folder_at))
        .map(|f| f.path)
        .ok_or_else(|| "That isn't one of Yap Code's workspaces.".to_string())
}

/// **A Code chat's + menu**: what its Claude Code offers in its workspace
/// (looked at in the background when nothing's known, or it's old), and the
/// chat's own folders and servers off.
#[tauri::command]
pub async fn code_menu(conversation: Option<u64>, workspace: Option<String>) -> Result<Menu, String> {
    let ws = workspace_of(conversation, workspace.as_deref())?;
    let (offered, again) = known(&ws, super::store::now_secs());
    let looking = again && crate::ai::claude_code::cli::find().is_some() && look(&ws);
    let ch = choices(conversation, Some(&ws));
    Ok(Menu { workspace: ws, offered: offered.map(shown), looking, folders: rows(&ch), servers_off: ch.servers_off })
}

/// **A server on or off** for a chat (`conversation`, or a new chat's
/// `workspace`).
#[tauri::command]
pub fn code_menu_server(conversation: Option<u64>, workspace: Option<String>, server: String, on: bool) -> Result<Vec<String>, String> {
    let ws = workspace_of(conversation, workspace.as_deref())?;
    let out = set_server(conversation, Some(&ws), &server, on)?;
    if conversation.is_some() {
        saved().map_err(|e| e.to_string())?;
    }
    Ok(out.servers_off)
}

/// **Add folder**: Windows' folder picker (a test run: the folder
/// `e2e_workspace` named), the folder checked and added to the chat. `None`:
/// cancelled.
#[tauri::command]
pub async fn code_chat_folder_add(app: tauri::AppHandle, window: tauri::WebviewWindow, conversation: Option<u64>, workspace: Option<String>) -> Result<Option<Vec<FolderRow>>, String> {
    let ws = workspace_of(conversation, workspace.as_deref())?;
    let Some(picked) = super::commands::pick_folder(&app, window, "Choose a folder Claude Code may use too").await? else { return Ok(None) };
    let out = tauri::async_runtime::spawn_blocking(move || add_folder(conversation, &ws, &picked)).await.map_err(|e| e.to_string())??;
    if conversation.is_some() {
        saved().map_err(|e| e.to_string())?;
    }
    Ok(Some(rows(&out)))
}

/// What a chat chose, read without looking at what Claude Code offers: the
/// Code edge's "+N folders", the thread's `/clear` dividers.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Chosen {
    pub folders: Vec<FolderRow>,
    pub servers_off: Vec<String>,
    pub clears: Vec<Clear>,
}

/// **A chat's own choices** (`conversation`, or a new chat's `workspace`).
#[tauri::command]
pub fn code_chat_choices(conversation: Option<u64>, workspace: Option<String>) -> Chosen {
    let ws = workspace_of(conversation, workspace.as_deref()).ok();
    let ch = choices(conversation, ws.as_deref());
    Chosen { folders: rows(&ch), servers_off: ch.servers_off, clears: ch.clears }
}

/// **A folder taken off a chat** (the summary panel's ×).
#[tauri::command]
pub fn code_chat_folder_remove(conversation: Option<u64>, workspace: Option<String>, path: String) -> Result<Vec<FolderRow>, String> {
    let ws = workspace_of(conversation, workspace.as_deref()).ok();
    let out = remove_folder(conversation, ws.as_deref(), &path)?;
    if conversation.is_some() {
        saved().map_err(|e| e.to_string())?;
    }
    Ok(rows(&out))
}

#[cfg(test)]
#[path = "menu/tests.rs"]
mod tests;

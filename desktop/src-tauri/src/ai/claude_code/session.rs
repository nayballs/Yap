//! One Claude Code process holding one conversation: `claude -p` in
//! stream-json both ways. Every user message Yap writes to its stdin is a
//! turn; every turn ends with a `result` (checked on 2.1.296: two messages,
//! two results, one `session_id`).
//!
//! - **The command line** ([`args`]): always `-p --input-format stream-json
//!   --output-format stream-json --verbose --include-partial-messages`, then
//!   what the [`Spec`] asks for. Never the prompt, and never JSON or a system
//!   prompt: the settings, the MCP config and the appended system prompt are
//!   written to files in a folder of the run's own under the temp folder
//!   (`--settings`, `--mcp-config`, `--append-system-prompt-file`, all
//!   checked on 2.1.296), removed when the session goes. So nothing needs
//!   escaping for an npm shim, Windows' 32 K command-line limit never bites,
//!   and nothing secret is on a command line every program can read.
//! - **A run's secrets** (the key of Yap's run endpoint) go in [`Spec::env`]
//!   and are named in the MCP config as `${VAR}`: Claude Code fills them in
//!   from its environment (checked on 2.1.296 for an http server's
//!   `headers`, inline and from a file; http hooks' `headers` take `$VAR` /
//!   `${VAR}` when the hook lists the variable in `allowedEnvVars`).
//! - **Paying is never ambiguous** ([`environment`]): the environment is
//!   Yap's with every `CLAUDE…` / `ANTHROPIC_…` variable taken off
//!   (`cli::clean_env`; `CLAUDE_CONFIG_DIR` stays), then `ANTHROPIC_API_KEY`
//!   set for a key run and absent for a plan run. A `Spec::env` entry can't
//!   put any of them back. And `system/init`'s `apiKeySource` is checked
//!   against it ([`Problem::PaysAnotherWay`]).
//! - **The prompt goes over stdin** ([`Session::send_user`]) as a stream-json
//!   user message, pictures first as base64 image blocks.
//! - **Stopping** ([`Session::interrupt`]): the Agent SDK's control request,
//!   `{"type":"control_request","request_id":…,"request":{"subtype":
//!   "interrupt"}}`, when `system/init` lists `interrupt_receipt_v1`
//!   (2.1.296 answers `{"type":"control_response","response":{"subtype":
//!   "success","request_id":…,"response":{"still_queued":[]}}}` before the
//!   turn's `result`); otherwise stdin is closed and the process tree ended.
//!   Windows has no SIGINT; [`Session::kill`] ends the tree (`procs`).
//! - **Spawned like the speaker engine**: piped stdio, no window, tied to Yap
//!   (`procs::end_with_yap`), a reader thread turning stdout into [`Event`]s
//!   on a channel, stderr to the log at debug.
//!
//! Logs at `info` carry ids, counts and outcomes, never the prompt or an
//! answer.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

use super::cli::{self, Program};
use super::events::{self, Classifier, Event, Paying, Problem, Usage};

/// How long a dropped session gets to exit by itself before it's ended.
const DROP_GRACE: Duration = Duration::from_secs(3);
/// How long the fallback interrupt waits after closing stdin.
const INTERRUPT_GRACE: Duration = Duration::from_secs(2);
/// How long the reader waits for the process once its stdout closed.
const EXIT_WAIT: Duration = Duration::from_secs(10);

// ---- The spec ----

/// `--effort`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effort {
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl Effort {
    pub fn as_str(self) -> &'static str {
        match self {
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
            Effort::XHigh => "xhigh",
            Effort::Max => "max",
        }
    }

    pub fn parse(s: &str) -> Option<Effort> {
        [Effort::Low, Effort::Medium, Effort::High, Effort::XHigh, Effort::Max]
            .into_iter()
            .find(|e| e.as_str().eq_ignore_ascii_case(s.trim()))
    }
}

/// `--permission-mode`. Yap always sets one (from 2.1.302 a `-p` run starts
/// in manual mode when nothing does).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PermissionMode {
    /// Asks before anything that needs approval.
    Manual,
    AcceptEdits,
    /// A classifier approves safe steps.
    Auto,
    /// Denies anything that would ask (the default: nothing runs unasked).
    #[default]
    DontAsk,
    Plan,
    BypassPermissions,
}

impl PermissionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            PermissionMode::Manual => "manual",
            PermissionMode::AcceptEdits => "acceptEdits",
            PermissionMode::Auto => "auto",
            PermissionMode::DontAsk => "dontAsk",
            PermissionMode::Plan => "plan",
            PermissionMode::BypassPermissions => "bypassPermissions",
        }
    }

    pub fn parse(s: &str) -> Option<PermissionMode> {
        use PermissionMode::*;
        [Manual, AcceptEdits, Auto, DontAsk, Plan, BypassPermissions].into_iter().find(|m| m.as_str() == s.trim())
    }
}

/// `--permission-prompts`: who answers a prompt in a `-p` run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionPrompts {
    /// The host: `--permission-prompt-tool` (Yap's `approve`).
    Host,
    /// Nobody: anything that would ask is denied (unattended jobs).
    Off,
}

impl PermissionPrompts {
    pub fn as_str(self) -> &'static str {
        match self {
            PermissionPrompts::Host => "host",
            PermissionPrompts::Off => "none",
        }
    }
}

/// An Anthropic API key for a key run. Its `Debug` never shows it.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    pub fn new(key: impl Into<String>) -> ApiKey {
        ApiKey(key.into())
    }
}

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApiKey(…)")
    }
}

/// How a run pays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pay {
    /// The person's Claude plan, through Claude Code's own sign-in.
    Plan,
    /// Their Anthropic API key, set as `ANTHROPIC_API_KEY` for this run only.
    ApiKey(ApiKey),
}

impl Pay {
    pub fn paying(&self) -> Paying {
        match self {
            Pay::Plan => Paying::Plan,
            Pay::ApiKey(_) => Paying::ApiKey,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Pay::Plan => "plan",
            Pay::ApiKey(_) => "api key",
        }
    }
}

/// A new conversation with an id Yap chose, or one to continue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Conversation {
    /// `--session-id <uuid>`.
    New(String),
    /// `--resume <id>`.
    Resume(String),
}

impl Conversation {
    /// A new conversation with a fresh id.
    pub fn fresh() -> Conversation {
        Conversation::New(new_session_id())
    }

    pub fn id(&self) -> &str {
        match self {
            Conversation::New(id) | Conversation::Resume(id) => id,
        }
    }
}

/// A random (version 4) UUID, as `--session-id` takes.
pub fn new_session_id() -> String {
    let mut b = [0u8; 16];
    if getrandom::fill(&mut b).is_err() {
        // No OS randomness (never seen): time and a counter keep it unique.
        static N: AtomicU64 = AtomicU64::new(0);
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        let mix = t ^ (u128::from(N.fetch_add(1, Ordering::Relaxed)) << 64);
        b = mix.to_le_bytes();
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

/// Everything about one Claude Code run. [`Spec::new`] gives the safe
/// defaults: `dontAsk`, a new conversation, no tools named (Claude Code's
/// own default set), no MCP config, the person's own settings. Its `Debug`
/// shows no secret and no prompt (the env's names, the prompt's length).
#[derive(Clone)]
pub struct Spec {
    /// The folder it works in.
    pub cwd: PathBuf,
    /// A full model id (`claude-opus-5-5`), or Claude Code's default.
    pub model: Option<String>,
    pub effort: Option<Effort>,
    pub permission_mode: PermissionMode,
    pub permission_prompts: Option<PermissionPrompts>,
    /// `--permission-prompt-tool` (`mcp__yap__approve`).
    pub permission_prompt_tool: Option<String>,
    /// `--tools`: exactly these built-in tools. `Some(vec![])` is none at
    /// all (`--tools ""`); `None`, Claude Code's default set.
    pub tools: Option<Vec<String>>,
    /// `--allowedTools`: allowed without asking (permission-rule syntax).
    pub allowed_tools: Vec<String>,
    /// `--disallowedTools`.
    pub disallowed_tools: Vec<String>,
    /// `--mcp-config`: `{"mcpServers":{…}}`. Name secrets as `${VAR}` and
    /// put them in [`Spec::env`].
    pub mcp_config: Option<Value>,
    /// `--strict-mcp-config`: only [`Spec::mcp_config`]'s servers.
    pub strict_mcp_config: bool,
    /// `--setting-sources`: of `user`, `project`, `local`. `Some(vec![])`
    /// loads none of the person's settings files; `None`, all of them.
    pub setting_sources: Option<Vec<String>>,
    /// `--settings`: this run's own settings (its hooks).
    pub settings: Option<Value>,
    /// `--append-system-prompt-file`: added to Claude Code's own prompt
    /// (never `--system-prompt`, which replaces it).
    pub append_system_prompt: Option<String>,
    /// `--add-dir`: more folders its tools may use.
    pub add_dirs: Vec<PathBuf>,
    pub conversation: Conversation,
    pub pay: Pay,
    /// More environment variables for this run (a run endpoint's key). A
    /// `CLAUDE…` / `ANTHROPIC_…` name is refused (`CLAUDE_CONFIG_DIR` is
    /// allowed).
    pub env: Vec<(String, String)>,
    /// `--max-budget-usd`: API key runs only (ignored on a plan).
    pub max_budget_usd: Option<f64>,
    /// `--fallback-model`.
    pub fallback_model: Option<String>,
}

impl Spec {
    pub fn new(cwd: impl Into<PathBuf>, pay: Pay) -> Spec {
        Spec {
            cwd: cwd.into(),
            model: None,
            effort: None,
            permission_mode: PermissionMode::default(),
            permission_prompts: None,
            permission_prompt_tool: None,
            tools: None,
            allowed_tools: Vec::new(),
            disallowed_tools: Vec::new(),
            mcp_config: None,
            strict_mcp_config: false,
            setting_sources: None,
            settings: None,
            append_system_prompt: None,
            add_dirs: Vec::new(),
            conversation: Conversation::fresh(),
            pay,
            env: Vec::new(),
            max_budget_usd: None,
            fallback_model: None,
        }
    }
}

impl std::fmt::Debug for Spec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Spec")
            .field("model", &self.model)
            .field("effort", &self.effort)
            .field("permission_mode", &self.permission_mode)
            .field("permission_prompts", &self.permission_prompts)
            .field("permission_prompt_tool", &self.permission_prompt_tool)
            .field("tools", &self.tools)
            .field("allowed_tools", &self.allowed_tools)
            .field("disallowed_tools", &self.disallowed_tools)
            .field("mcp_config", &self.mcp_config.is_some())
            .field("strict_mcp_config", &self.strict_mcp_config)
            .field("setting_sources", &self.setting_sources)
            .field("settings", &self.settings.is_some())
            .field("append_system_prompt_chars", &self.append_system_prompt.as_ref().map(|p| p.chars().count()))
            .field("add_dirs", &self.add_dirs.len())
            .field("conversation", &self.conversation)
            .field("pay", &self.pay)
            .field("env", &self.env.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>())
            .field("max_budget_usd", &self.max_budget_usd)
            .field("fallback_model", &self.fallback_model)
            .finish_non_exhaustive()
    }
}

/// The files a run's flags name (see [`RunFiles`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Files {
    pub settings: Option<PathBuf>,
    pub mcp_config: Option<PathBuf>,
    pub system_prompt: Option<PathBuf>,
}

/// Claude Code's arguments for `spec`, with its JSON and prompt in `files`.
pub fn args(spec: &Spec, files: &Files) -> Vec<String> {
    let mut a: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let mut push = |flag: &str, value: String| {
        a.push(flag.to_string());
        a.push(value);
    };
    match &spec.conversation {
        Conversation::New(id) => push("--session-id", id.clone()),
        Conversation::Resume(id) => push("--resume", id.clone()),
    }
    if let Some(m) = spec.model.as_deref().filter(|m| !m.is_empty()) {
        push("--model", m.to_string());
    }
    if let Some(e) = spec.effort {
        push("--effort", e.as_str().to_string());
    }
    push("--permission-mode", spec.permission_mode.as_str().to_string());
    if let Some(p) = spec.permission_prompts {
        push("--permission-prompts", p.as_str().to_string());
    }
    if let Some(t) = spec.permission_prompt_tool.as_deref().filter(|t| !t.is_empty()) {
        push("--permission-prompt-tool", t.to_string());
    }
    // Lists as one comma-separated value each: the variadic flags then
    // never swallow the next argument.
    if let Some(tools) = &spec.tools {
        push("--tools", tools.join(","));
    }
    if !spec.allowed_tools.is_empty() {
        push("--allowedTools", spec.allowed_tools.join(","));
    }
    if !spec.disallowed_tools.is_empty() {
        push("--disallowedTools", spec.disallowed_tools.join(","));
    }
    if let Some(f) = &files.mcp_config {
        push("--mcp-config", f.display().to_string());
    }
    if spec.strict_mcp_config {
        a.push("--strict-mcp-config".into());
    }
    if let Some(s) = &spec.setting_sources {
        a.push("--setting-sources".into());
        a.push(s.join(","));
    }
    if let Some(f) = &files.settings {
        a.push("--settings".into());
        a.push(f.display().to_string());
    }
    if let Some(f) = &files.system_prompt {
        a.push("--append-system-prompt-file".into());
        a.push(f.display().to_string());
    }
    if !spec.add_dirs.is_empty() {
        a.push("--add-dir".into());
        a.extend(spec.add_dirs.iter().map(|d| d.display().to_string()));
    }
    if let (Some(usd), Pay::ApiKey(_)) = (spec.max_budget_usd, &spec.pay) {
        a.push("--max-budget-usd".into());
        a.push(format!("{usd}"));
    }
    if let Some(m) = spec.fallback_model.as_deref().filter(|m| !m.is_empty()) {
        a.push("--fallback-model".into());
        a.push(m.to_string());
    }
    a
}

/// The run's environment: `inherited` cleaned (`cli::clean_env`), `extra`
/// added, and `ANTHROPIC_API_KEY` for a key run (never for a plan run).
pub fn environment(
    inherited: impl IntoIterator<Item = (OsString, OsString)>,
    pay: &Pay,
    extra: &[(String, String)],
) -> Result<Vec<(OsString, OsString)>, String> {
    let mut env = cli::clean_env(inherited);
    for (name, value) in extra {
        if name.is_empty() || name.contains('=') || !cli::keeps_env(name) {
            return Err(format!("{name} can't be set for a Claude Code run: it would change how Claude Code signs in or pays."));
        }
        env.retain(|(k, _)| !k.to_string_lossy().eq_ignore_ascii_case(name));
        env.push((name.into(), value.into()));
    }
    if let Pay::ApiKey(ApiKey(key)) = pay {
        env.push(("ANTHROPIC_API_KEY".into(), key.into()));
    }
    Ok(env)
}

/// The run's own folder of files (settings, MCP config, system prompt),
/// under `<temp>/yap-claude/`, removed with it.
struct RunFiles {
    dir: Option<PathBuf>,
    files: Files,
}

impl RunFiles {
    fn write(spec: &Spec) -> Result<RunFiles, String> {
        let wanted = spec.settings.is_some() || spec.mcp_config.is_some() || spec.append_system_prompt.is_some();
        if !wanted {
            return Ok(RunFiles { dir: None, files: Files::default() });
        }
        let mut raw = [0u8; 6];
        let _ = getrandom::fill(&mut raw);
        let tag: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        let short: String = spec.conversation.id().chars().filter(char::is_ascii_alphanumeric).take(8).collect();
        let dir = std::env::temp_dir().join("yap-claude").join(format!("{short}-{tag}"));
        std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't make a folder for Claude Code's settings: {e}"))?;
        let mut out = RunFiles { dir: Some(dir.clone()), files: Files::default() };
        let put = |name: &str, body: String| -> Result<PathBuf, String> {
            let path = dir.join(name);
            std::fs::write(&path, body).map_err(|e| format!("Couldn't write Claude Code's settings: {e}"))?;
            Ok(path)
        };
        if let Some(s) = &spec.settings {
            out.files.settings = Some(put("settings.json", s.to_string())?);
        }
        if let Some(m) = &spec.mcp_config {
            out.files.mcp_config = Some(put("mcp.json", m.to_string())?);
        }
        if let Some(p) = &spec.append_system_prompt {
            out.files.system_prompt = Some(put("system-prompt.txt", p.clone())?);
        }
        Ok(out)
    }
}

impl Drop for RunFiles {
    fn drop(&mut self) {
        if let Some(dir) = self.dir.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

// ---- Messages ----

/// A picture for a user message.
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    /// `image/png`, `image/jpeg`, `image/gif` or `image/webp`.
    pub media_type: String,
    /// The bytes, base64.
    pub base64: String,
}

impl Image {
    pub fn from_bytes(media_type: &str, bytes: &[u8]) -> Image {
        use base64::Engine as _;
        Image { media_type: media_type.to_string(), base64: base64::engine::general_purpose::STANDARD.encode(bytes) }
    }
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Image({}, {} base64 chars)", self.media_type, self.base64.len())
    }
}

/// A stream-json user message: the pictures, then the words.
pub fn user_message(text: &str, images: &[Image]) -> String {
    let mut content: Vec<Value> = images
        .iter()
        .map(|i| json!({"type": "image", "source": {"type": "base64", "media_type": i.media_type, "data": i.base64}}))
        .collect();
    if !text.is_empty() || content.is_empty() {
        content.push(json!({"type": "text", "text": text}));
    }
    json!({"type": "user", "message": {"role": "user", "content": content}}).to_string()
}

/// A control request line, and its id.
pub fn control_request(request: Value) -> (String, String) {
    static N: AtomicU64 = AtomicU64::new(1);
    let mut raw = [0u8; 4];
    let _ = getrandom::fill(&mut raw);
    let hex: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    let id = format!("yap_{}_{hex}", N.fetch_add(1, Ordering::Relaxed));
    let line = json!({"type": "control_request", "request_id": id, "request": request}).to_string();
    (id, line)
}

// ---- The session ----

/// What [`Session::interrupt`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Interrupt {
    /// The control request went: a `ControlResponse` with this id, then the
    /// turn's `Result` (`interrupted()`), follow.
    Requested { request_id: String },
    /// This Claude Code can't be interrupted: stdin was closed and the
    /// process is being ended (`Exited` follows).
    Ended,
}

struct State {
    session_id: String,
    capabilities: Vec<String>,
    init_seen: bool,
    exit: Option<Option<i32>>,
}

struct Shared {
    child: Mutex<Option<Child>>,
    stdin: Mutex<Option<ChildStdin>>,
    state: Mutex<State>,
    pid: u32,
    /// When it started (a FILETIME): its family is what started after.
    #[cfg_attr(not(windows), allow(dead_code))]
    since: u64,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl Shared {
    /// Wait up to `timeout` for the process to end: its exit code, `None`
    /// when it's still running (`Some(None)`: ended without one).
    fn wait_exit(&self, timeout: Duration) -> Option<Option<i32>> {
        let until = Instant::now() + timeout;
        loop {
            {
                let mut slot = lock(&self.child);
                let Some(child) = slot.as_mut() else { return Some(None) };
                match child.try_wait() {
                    Ok(Some(status)) => return Some(status.code()),
                    Ok(None) => {}
                    Err(_) => return Some(None),
                }
            }
            if Instant::now() >= until {
                return None;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// End the process and everything it started.
    fn kill_tree(&self) {
        #[cfg(windows)]
        {
            let family = crate::shell::procs::family(self.pid, self.since);
            // Its own children first (deepest last found), then it.
            for pid in family.iter().rev().filter(|&&p| p != self.pid) {
                crate::shell::procs::kill(*pid);
            }
        }
        if let Some(child) = lock(&self.child).as_mut() {
            let _ = child.kill();
        }
    }
}

/// One running Claude Code. Its events come on the channel [`Session::start`]
/// returns, ending with [`Event::Exited`]. Dropping it closes stdin and ends
/// the process after a moment (keep it until `Exited` to let a turn finish).
pub struct Session {
    shared: Arc<Shared>,
    files: Option<RunFiles>,
}

impl Session {
    /// Start the person's Claude Code ([`cli::find`]) on `spec`.
    pub fn start(spec: Spec) -> Result<(Session, UnboundedReceiver<Event>), String> {
        let program = cli::find().ok_or_else(|| "Claude Code isn't installed on this PC.".to_string())?;
        Session::start_with(&program, spec)
    }

    /// Start `program` (Claude Code, or a stand-in) on `spec`.
    pub fn start_with(program: &Program, spec: Spec) -> Result<(Session, UnboundedReceiver<Event>), String> {
        if !spec.cwd.is_dir() {
            return Err("The folder for Claude Code isn't there.".into());
        }
        let files = RunFiles::write(&spec)?;
        let mut cmd = program.command(&args(&spec, &files.files))?;
        let env = environment(std::env::vars_os(), &spec.pay, &spec.env)?;
        cmd.env_clear().envs(env).current_dir(&spec.cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)]
        let since = crate::shell::procs::now_filetime();
        #[cfg(not(windows))]
        let since = 0;
        let mut child = cmd.spawn().map_err(|e| format!("Couldn't start Claude Code: {e}"))?;
        crate::shell::procs::end_with_yap(&child);
        let pid = child.id();
        let Some(stdout) = child.stdout.take() else {
            let _ = child.kill();
            return Err("Couldn't read Claude Code's output.".into());
        };
        let stdin = child.stdin.take();
        forward_stderr(&mut child);
        tracing::info!(
            pid,
            pay = spec.pay.label(),
            mode = spec.permission_mode.as_str(),
            resume = matches!(spec.conversation, Conversation::Resume(_)),
            "claude code: session started"
        );
        let shared = Arc::new(Shared {
            child: Mutex::new(Some(child)),
            stdin: Mutex::new(stdin),
            state: Mutex::new(State {
                session_id: spec.conversation.id().to_string(),
                capabilities: Vec::new(),
                init_seen: false,
                exit: None,
            }),
            pid,
            since,
        });
        let (tx, rx) = unbounded_channel();
        let classifier = Classifier::expect_pay(spec.pay.paying());
        let reader_shared = shared.clone();
        std::thread::Builder::new()
            .name("claude-code-out".into())
            .spawn(move || read_events(reader_shared, stdout, tx, classifier))
            .map_err(|e| format!("Couldn't read Claude Code's output: {e}"))?;
        Ok((Session { shared, files: Some(files) }, rx))
    }

    /// The conversation's id: the one Yap gave, or the one Claude Code's
    /// `system/init` says (a resumed session it had to copy gets a new one).
    pub fn session_id(&self) -> String {
        lock(&self.shared.state).session_id.clone()
    }

    pub fn pid(&self) -> u32 {
        self.shared.pid
    }

    /// The latest `system/init`'s capabilities (empty before the first turn).
    pub fn capabilities(&self) -> Vec<String> {
        lock(&self.shared.state).capabilities.clone()
    }

    /// Whether the process is still running.
    pub fn is_running(&self) -> bool {
        lock(&self.shared.state).exit.is_none() && self.shared.wait_exit(Duration::ZERO).is_none()
    }

    /// A user message: a new turn (or, mid-turn, the next one queued).
    pub fn send_user(&self, text: &str, images: &[Image]) -> Result<(), String> {
        tracing::debug!(chars = text.chars().count(), images = images.len(), "claude code: user message");
        self.write_line(&user_message(text, images))
    }

    /// Send a control request (`{"subtype":"interrupt"}`, …): its id, which
    /// the `ControlResponse` carries. Only `interrupt` is checked against a
    /// real Claude Code (2.1.296); others are the Agent SDK's.
    pub fn control(&self, request: Value) -> Result<String, String> {
        let (id, line) = control_request(request);
        self.write_line(&line)?;
        Ok(id)
    }

    /// Stop the turn running now. With `interrupt_receipt_v1` (every
    /// version Yap starts lists it; before the first `system/init` that's
    /// assumed) the control request; otherwise stdin closed and the process
    /// ended after a moment.
    pub fn interrupt(&self) -> Result<Interrupt, String> {
        let can = {
            let s = lock(&self.shared.state);
            !s.init_seen || s.capabilities.iter().any(|c| c == "interrupt_receipt_v1")
        };
        if can {
            let request_id = self.control(json!({"subtype": "interrupt"}))?;
            tracing::info!(pid = self.shared.pid, "claude code: interrupt sent");
            return Ok(Interrupt::Requested { request_id });
        }
        self.end();
        let shared = self.shared.clone();
        std::thread::spawn(move || {
            if shared.wait_exit(INTERRUPT_GRACE).is_none() {
                shared.kill_tree();
            }
        });
        tracing::info!(pid = self.shared.pid, "claude code: no interrupt here; ending it");
        Ok(Interrupt::Ended)
    }

    /// Close stdin: Claude Code finishes the turn it's on, then exits.
    pub fn end(&self) {
        lock(&self.shared.stdin).take();
    }

    /// End the process and everything it started, now.
    pub fn kill(&self) {
        self.end();
        self.shared.kill_tree();
    }

    /// Wait (blocking) up to `timeout` for the process to end; its exit
    /// code (`Some(None)`: none), or `None` if it's still running.
    pub fn wait(&self, timeout: Duration) -> Option<Option<i32>> {
        self.shared.wait_exit(timeout)
    }

    fn write_line(&self, line: &str) -> Result<(), String> {
        let mut slot = lock(&self.shared.stdin);
        let Some(stdin) = slot.as_mut() else { return Err("This Claude Code session has ended.".into()) };
        let wrote = stdin.write_all(line.as_bytes()).and_then(|_| stdin.write_all(b"\n")).and_then(|_| stdin.flush());
        wrote.map_err(|_| {
            slot.take();
            "This Claude Code session has ended.".to_string()
        })
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.end();
        let files = self.files.take();
        if self.shared.wait_exit(Duration::ZERO).is_some() {
            // Already gone: its files go now.
            drop(files);
            return;
        }
        let shared = self.shared.clone();
        let _ = std::thread::Builder::new().name("claude-code-end".into()).spawn(move || {
            if shared.wait_exit(DROP_GRACE).is_none() {
                shared.kill_tree();
                let _ = shared.wait_exit(DROP_GRACE);
            }
            drop(files);
        });
    }
}

/// stdout → events, until it closes; then [`Event::Exited`].
fn read_events(shared: Arc<Shared>, stdout: std::process::ChildStdout, tx: UnboundedSender<Event>, mut classifier: Classifier) {
    let mut reader = BufReader::new(stdout);
    let mut buf = Vec::new();
    let mut lines = 0u64;
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let line = String::from_utf8_lossy(&buf);
        let Some(event) = events::parse_line(&line) else { continue };
        lines += 1;
        match &event {
            Event::Init(init) => {
                let mut s = lock(&shared.state);
                s.init_seen = true;
                s.capabilities = init.capabilities.clone();
                if !init.session_id.is_empty() {
                    s.session_id = init.session_id.clone();
                }
            }
            Event::NotJson { chars } => tracing::debug!(chars, "claude code: a line that isn't JSON"),
            Event::Result(r) => tracing::info!(
                pid = shared.pid,
                is_error = r.is_error,
                reason = r.terminal_reason.as_deref().unwrap_or(""),
                turns = r.num_turns,
                denials = r.permission_denials.len(),
                "claude code: turn done"
            ),
            _ => {}
        }
        // A problem comes just before the event that showed it, so a
        // `Result` that's only an error is never read as an answer first.
        if let Some(problem) = classifier.feed(&event) {
            tracing::info!(pid = shared.pid, problem = problem_kind(&problem), "claude code: problem");
            let _ = tx.send(Event::Problem(problem));
        }
        // Read on even with nobody listening: a full pipe would stall it.
        let _ = tx.send(event);
    }
    let code = shared.wait_exit(EXIT_WAIT).flatten();
    lock(&shared.state).exit = Some(code);
    tracing::info!(pid = shared.pid, code = ?code, lines, "claude code: session ended");
    let _ = tx.send(Event::Exited(code));
}

fn problem_kind(p: &Problem) -> &'static str {
    match p {
        Problem::NotSignedIn => "not signed in",
        Problem::RateLimited { .. } => "rate limited",
        Problem::Billing => "billing",
        Problem::Account { .. } => "account",
        Problem::PaysAnotherWay { .. } => "pays another way",
    }
}

/// The child's stderr into the log at debug level.
fn forward_stderr(child: &mut Child) {
    if let Some(err) = child.stderr.take() {
        let _ = std::thread::Builder::new().name("claude-code-err".into()).spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                let line = line.trim();
                if !line.is_empty() {
                    let cut: String = line.chars().take(300).collect();
                    tracing::debug!("claude code stderr: {cut}");
                }
            }
        });
    }
}

// ---- One question, one answer ----

/// A one-shot answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub text: String,
    pub usage: Usage,
    /// Claude Code's estimate of the API price (on a plan: what it saved).
    pub cost_usd: f64,
    pub session_id: String,
    /// The model that answered (`system/init`'s).
    pub model: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum OneshotError {
    /// Signed out, at the limit, paying the wrong way…
    Problem(Problem),
    /// Anything else, in plain words.
    Failed(String),
}

impl std::fmt::Display for OneshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OneshotError::Problem(p) => f.write_str(&p.message()),
            OneshotError::Failed(s) => f.write_str(s),
        }
    }
}

/// Ask once (memory learning, a title, a summary): start Claude Code on
/// `spec`, send `prompt`, close stdin and wait for the turn's `result` (at
/// most `timeout`). A [`Problem`] ends it at once. The process is ended when
/// this returns.
pub async fn oneshot(spec: Spec, prompt: &str, timeout: Duration) -> Result<Reply, OneshotError> {
    let program = cli::find().ok_or_else(|| OneshotError::Failed("Claude Code isn't installed on this PC.".into()))?;
    oneshot_with(&program, spec, prompt, timeout).await
}

/// [`oneshot`] on `program` (Claude Code, or a stand-in).
pub async fn oneshot_with(program: &Program, spec: Spec, prompt: &str, timeout: Duration) -> Result<Reply, OneshotError> {
    let (session, mut rx) = Session::start_with(program, spec).map_err(OneshotError::Failed)?;
    session.send_user(prompt, &[]).map_err(OneshotError::Failed)?;
    session.end();
    let mut model = String::new();
    let waited = tokio::time::timeout(timeout, async {
        while let Some(event) = rx.recv().await {
            match event {
                Event::Init(init) => model = init.model,
                Event::Problem(p) => return Err(OneshotError::Problem(p)),
                Event::Result(r) if r.is_error => {
                    let why = r.errors.first().cloned().filter(|e| !e.is_empty()).unwrap_or_else(|| r.text.clone());
                    return Err(OneshotError::Failed(if why.is_empty() { "Claude Code couldn't answer.".into() } else { why }));
                }
                Event::Result(r) => {
                    return Ok(Reply { text: r.text, usage: r.usage, cost_usd: r.cost_usd, session_id: r.session_id, model: model.clone() })
                }
                Event::Exited(_) => break,
                _ => {}
            }
        }
        Err(OneshotError::Failed("Claude Code stopped before it answered.".into()))
    })
    .await;
    match waited {
        Ok(result) => {
            if result.is_err() {
                session.kill();
            }
            result
        }
        Err(_) => {
            session.kill();
            Err(OneshotError::Failed("Claude Code took too long to answer.".into()))
        }
    }
}

/// The folder a one-shot call runs in when nothing else fits: an empty one
/// of Yap's own under the temp folder (no project's settings, CLAUDE.md or
/// MCP servers in it).
pub fn empty_folder() -> Result<PathBuf, String> {
    let dir = std::env::temp_dir().join("yap-claude").join("empty");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't make a folder for Claude Code: {e}"))?;
    Ok(dir)
}

/// `true` when `dir` is Yap's own (a run's files, [`empty_folder`]).
pub fn is_yaps_folder(dir: &Path) -> bool {
    crate::shell::procs::is_under(&std::env::temp_dir().join("yap-claude"), &dir.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::claude_code::events::{StreamKind, TurnResult};

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yap-cc-session-{tag}-{}", &new_session_id()[..8]));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Remove a test's folder once the stand-in that worked in it has gone
    /// (Windows keeps a running process's folder).
    fn remove_soon(dir: &Path) {
        let until = Instant::now() + Duration::from_secs(10);
        while std::fs::remove_dir_all(dir).is_err() && dir.exists() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn session_ids_are_v4_uuids() {
        let a = new_session_id();
        let b = new_session_id();
        assert_ne!(a, b);
        assert_eq!(a.len(), 36);
        let parts: Vec<&str> = a.split('-').collect();
        assert_eq!(parts.iter().map(|p| p.len()).collect::<Vec<_>>(), [8, 4, 4, 4, 12]);
        assert!(parts[2].starts_with('4'));
        assert!(matches!(parts[3].chars().next(), Some('8' | '9' | 'a' | 'b')));
    }

    #[test]
    fn a_spec_becomes_claude_codes_arguments() {
        let mut spec = Spec::new(std::env::temp_dir(), Pay::Plan);
        spec.conversation = Conversation::New("4d7c1d2e-8a51-4c3b-9f0e-2b6a7e1c5d90".into());
        let plain = args(&spec, &Files::default());
        assert_eq!(
            plain,
            [
                "-p",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
                "--session-id",
                "4d7c1d2e-8a51-4c3b-9f0e-2b6a7e1c5d90",
                "--permission-mode",
                "dontAsk",
            ],
            "the defaults: a new session, nothing runs unasked"
        );

        spec.conversation = Conversation::Resume("prev-id".into());
        spec.model = Some("claude-opus-5-5".into());
        spec.effort = Some(Effort::XHigh);
        spec.permission_mode = PermissionMode::Auto;
        spec.permission_prompts = Some(PermissionPrompts::Off);
        spec.permission_prompt_tool = Some("mcp__yap__approve".into());
        spec.tools = Some(vec!["WebSearch".into(), "WebFetch".into()]);
        spec.allowed_tools = vec!["mcp__yap__search_notes".into(), "Bash(git diff *)".into()];
        spec.disallowed_tools = vec!["Bash(rm *)".into()];
        spec.strict_mcp_config = true;
        spec.setting_sources = Some(vec![]);
        spec.add_dirs = vec![PathBuf::from(r"C:\Users\imogen\Notes"), PathBuf::from(r"D:\Shared")];
        spec.max_budget_usd = Some(0.5);
        spec.fallback_model = Some("claude-sonnet-5-5".into());
        let files = Files {
            settings: Some(PathBuf::from("s.json")),
            mcp_config: Some(PathBuf::from("m.json")),
            system_prompt: Some(PathBuf::from("p.txt")),
        };
        let a = args(&spec, &files);
        let after = |flag: &str| a.iter().position(|x| x == flag).map(|i| a[i + 1].as_str());
        assert_eq!(after("--resume"), Some("prev-id"));
        assert!(!a.contains(&"--session-id".to_string()));
        assert_eq!(after("--model"), Some("claude-opus-5-5"));
        assert_eq!(after("--effort"), Some("xhigh"));
        assert_eq!(after("--permission-mode"), Some("auto"));
        assert_eq!(after("--permission-prompts"), Some("none"));
        assert_eq!(after("--permission-prompt-tool"), Some("mcp__yap__approve"));
        assert_eq!(after("--tools"), Some("WebSearch,WebFetch"));
        assert_eq!(after("--allowedTools"), Some("mcp__yap__search_notes,Bash(git diff *)"));
        assert_eq!(after("--disallowedTools"), Some("Bash(rm *)"));
        assert_eq!(after("--mcp-config"), Some("m.json"));
        assert!(a.contains(&"--strict-mcp-config".to_string()));
        assert_eq!(after("--setting-sources"), Some(""), "none of the person's settings files");
        assert_eq!(after("--settings"), Some("s.json"));
        assert_eq!(after("--append-system-prompt-file"), Some("p.txt"));
        let dirs = a.iter().position(|x| x == "--add-dir").unwrap();
        assert_eq!(&a[dirs + 1..dirs + 3], [r"C:\Users\imogen\Notes", r"D:\Shared"]);
        assert_eq!(after("--max-budget-usd"), None, "a plan has no budget flag");
        assert_eq!(after("--fallback-model"), Some("claude-sonnet-5-5"));
        assert!(!a.iter().any(|x| x.contains('{')), "no JSON on the command line");

        spec.pay = Pay::ApiKey(ApiKey::new("sk-ant-example"));
        spec.tools = Some(vec![]);
        let a = args(&spec, &files);
        let after = |flag: &str| a.iter().position(|x| x == flag).map(|i| a[i + 1].as_str());
        assert_eq!(after("--max-budget-usd"), Some("0.5"));
        assert_eq!(after("--tools"), Some(""), "no tools at all");
        assert!(!a.iter().any(|x| x.contains("sk-ant")), "the key is never an argument");
        assert_eq!(format!("{:?}", spec.pay), "ApiKey(ApiKey(…))");
    }

    #[test]
    fn billing_is_never_ambiguous() {
        let inherited: Vec<(OsString, OsString)> = [
            ("PATH", "C:\\Windows"),
            ("ANTHROPIC_API_KEY", "sk-ant-from-the-environment"),
            ("ANTHROPIC_BASE_URL", "http://127.0.0.1:9"),
            ("CLAUDE_CODE_OAUTH_TOKEN", "not-yours"),
            ("CLAUDECODE", "1"),
            ("CLAUDE_CONFIG_DIR", "D:\\cc"),
        ]
        .iter()
        .map(|(k, v)| (OsString::from(k), OsString::from(v)))
        .collect();
        let get = |env: &[(OsString, OsString)], name: &str| {
            env.iter().find(|(k, _)| k.to_string_lossy().eq_ignore_ascii_case(name)).map(|(_, v)| v.to_string_lossy().into_owned())
        };
        let plan = environment(inherited.clone(), &Pay::Plan, &[("YAP_RUN_KEY".into(), "k1".into())]).unwrap();
        assert_eq!(get(&plan, "ANTHROPIC_API_KEY"), None, "a plan run has no key");
        assert_eq!(get(&plan, "ANTHROPIC_BASE_URL"), None);
        assert_eq!(get(&plan, "CLAUDE_CODE_OAUTH_TOKEN"), None);
        assert_eq!(get(&plan, "CLAUDECODE"), None);
        assert_eq!(get(&plan, "CLAUDE_CONFIG_DIR").as_deref(), Some("D:\\cc"));
        assert_eq!(get(&plan, "YAP_RUN_KEY").as_deref(), Some("k1"));
        assert_eq!(get(&plan, "PATH").as_deref(), Some("C:\\Windows"));

        let key = environment(inherited.clone(), &Pay::ApiKey(ApiKey::new("sk-ant-yaps")), &[]).unwrap();
        assert_eq!(get(&key, "ANTHROPIC_API_KEY").as_deref(), Some("sk-ant-yaps"), "Yap's key, not the inherited one");
        assert_eq!(key.iter().filter(|(k, _)| k == "ANTHROPIC_API_KEY").count(), 1);

        for sneaky in ["ANTHROPIC_API_KEY", "anthropic_auth_token", "CLAUDE_CODE_USE_BEDROCK", "CLAUDECODE", "", "A=B"] {
            assert!(environment(inherited.clone(), &Pay::Plan, &[(sneaky.into(), "x".into())]).is_err(), "{sneaky}");
        }
        let moved = environment(inherited, &Pay::Plan, &[("CLAUDE_CONFIG_DIR".into(), "E:\\other".into())]).unwrap();
        assert_eq!(get(&moved, "CLAUDE_CONFIG_DIR").as_deref(), Some("E:\\other"));
        assert_eq!(moved.iter().filter(|(k, _)| k.to_string_lossy().eq_ignore_ascii_case("CLAUDE_CONFIG_DIR")).count(), 1);
    }

    #[test]
    fn user_messages_carry_words_and_pictures() {
        let v: Value = serde_json::from_str(&user_message("What's \"this\"?\nLine two", &[])).unwrap();
        assert_eq!(v, json!({"type":"user","message":{"role":"user","content":[{"type":"text","text":"What's \"this\"?\nLine two"}]}}));
        let png = Image::from_bytes("image/png", &[0x89, b'P', b'N', b'G']);
        assert_eq!(format!("{png:?}"), "Image(image/png, 8 base64 chars)");
        let v: Value = serde_json::from_str(&user_message("And this?", std::slice::from_ref(&png))).unwrap();
        let content = v["message"]["content"].as_array().unwrap();
        assert_eq!(content[0], json!({"type":"image","source":{"type":"base64","media_type":"image/png","data":"iVBORw=="}}));
        assert_eq!(content[1]["text"], "And this?");
        let v: Value = serde_json::from_str(&user_message("", &[png])).unwrap();
        assert_eq!(v["message"]["content"].as_array().unwrap().len(), 1, "a picture alone");
        let (id, line) = control_request(json!({"subtype":"interrupt"}));
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v, json!({"type":"control_request","request_id":id,"request":{"subtype":"interrupt"}}));
        assert!(id.starts_with("yap_"));
        assert_ne!(control_request(json!({})).0, id);
    }

    #[test]
    fn effort_and_modes_read_back() {
        for e in [Effort::Low, Effort::Medium, Effort::High, Effort::XHigh, Effort::Max] {
            assert_eq!(Effort::parse(e.as_str()), Some(e));
        }
        assert_eq!(Effort::parse("XHIGH"), Some(Effort::XHigh));
        assert_eq!(Effort::parse("extreme"), None);
        use PermissionMode::*;
        for m in [Manual, AcceptEdits, Auto, DontAsk, Plan, BypassPermissions] {
            assert_eq!(PermissionMode::parse(m.as_str()), Some(m));
        }
        assert_eq!(PermissionMode::default(), DontAsk);
        assert_eq!(PermissionPrompts::Off.as_str(), "none");
    }

    #[test]
    fn a_runs_files_go_with_it() {
        let mut spec = Spec::new(std::env::temp_dir(), Pay::Plan);
        spec.settings = Some(json!({"hooks": {}}));
        spec.mcp_config = Some(json!({"mcpServers": {"yap": {"type": "http", "url": "http://127.0.0.1:1/mcp", "headers": {"Authorization": "Bearer ${YAP_RUN_KEY}"}}}}));
        spec.append_system_prompt = Some("About you: Priya, at example.com.".into());
        spec.env = vec![("YAP_RUN_KEY".into(), "secret-run-key".into())];
        spec.pay = Pay::ApiKey(ApiKey::new("sk-ant-example"));
        let shown = format!("{spec:?}");
        for hidden in ["Priya", "secret-run-key", "sk-ant", "Bearer"] {
            assert!(!shown.contains(hidden), "{hidden} in {shown}");
        }
        assert!(shown.contains("YAP_RUN_KEY") && shown.contains("append_system_prompt_chars: Some(33)"), "{shown}");
        let files = RunFiles::write(&spec).unwrap();
        let dir = files.dir.clone().unwrap();
        assert!(is_yaps_folder(&dir));
        let mcp: Value = serde_json::from_str(&std::fs::read_to_string(files.files.mcp_config.as_ref().unwrap()).unwrap()).unwrap();
        assert_eq!(mcp["mcpServers"]["yap"]["headers"]["Authorization"], "Bearer ${YAP_RUN_KEY}");
        assert_eq!(std::fs::read_to_string(files.files.system_prompt.as_ref().unwrap()).unwrap(), "About you: Priya, at example.com.");
        drop(files);
        assert!(!dir.exists(), "removed with the run");
        let none = RunFiles::write(&Spec::new(std::env::temp_dir(), Pay::Plan)).unwrap();
        assert!(none.dir.is_none() && none.files == Files::default());
    }

    // ---- Round trips against the fake ----

    /// This test binary as `fake-claude`: libtest stops reading options at
    /// `--`, so Claude Code's arguments after it are only filters that match
    /// no test, and `--exact` runs just [`fake_claude_stand_in`].
    fn stand_in(script: &Path) -> (Program, Vec<(String, String)>) {
        let name = format!("{}::fake_claude_stand_in", module_path!().split_once("::").map_or("", |(_, m)| m));
        let prefix = vec![name, "--exact".into(), "--nocapture".into(), "--quiet".into(), "--test-threads=1".into(), "--".into()];
        let program = Program::new(std::env::current_exe().unwrap()).with_prefix(prefix);
        (program, vec![("YAP_FAKE_CLAUDE_STAND_IN".into(), script.display().to_string())])
    }

    /// The stand-in itself (a no-op in a normal test run).
    #[test]
    fn fake_claude_stand_in() {
        let Some(script) = std::env::var_os("YAP_FAKE_CLAUDE_STAND_IN") else { return };
        // libtest may have started a line ("test … ... "): end it, so the
        // fake's first line is a line of its own.
        {
            use std::io::Write as _;
            let mut out = std::io::stdout();
            let _ = out.write_all(b"\n").and_then(|_| out.flush());
        }
        let mut args = vec![script.to_string_lossy().into_owned()];
        args.extend(std::env::args().skip_while(|a| a != "--").skip(1));
        std::process::exit(crate::ai::claude_code::fake::main(args));
    }

    async fn next_where(rx: &mut UnboundedReceiver<Event>, what: &str, pick: impl Fn(&Event) -> bool) -> Event {
        let found = tokio::time::timeout(Duration::from_secs(30), async {
            while let Some(e) = rx.recv().await {
                if pick(&e) {
                    return Some(e);
                }
            }
            None
        })
        .await;
        found.ok().flatten().unwrap_or_else(|| panic!("no {what}"))
    }

    fn other_subtype(e: &Event, subtype: &str) -> bool {
        matches!(e, Event::Other(v) if v["subtype"] == subtype)
    }

    const ROUND_TRIP: &str = r#"// Two turns: the first answered with what was said, the second interrupted.
{"_dump_args":true}
{"_dump_env":["ANTHROPIC_API_KEY","CLAUDECODE","CLAUDE_CODE_ENTRYPOINT","ANTHROPIC_BASE_URL","YAP_RUN_KEY"]}
{"_wait_user":true}
{"type":"system","subtype":"init","session_id":"{{session_id}}","cwd":"{{cwd}}","tools":[],"mcp_servers":[],"model":"claude-haiku-5-5","permissionMode":"dontAsk","apiKeySource":"none","claude_code_version":"2.1.296","capabilities":["interrupt_receipt_v1"]}
{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"You said: {{user_text}}"}},"parent_tool_use_id":null,"session_id":"{{session_id}}"}
{"type":"result","subtype":"success","is_error":false,"result":"You said: {{user_text}}","session_id":"{{session_id}}","total_cost_usd":0.001,"usage":{"input_tokens":10,"output_tokens":5},"permission_denials":[],"terminal_reason":"completed","num_turns":1}
{"_wait_user":true}
{"type":"system","subtype":"init","session_id":"{{session_id}}","cwd":"{{cwd}}","tools":[],"mcp_servers":[],"model":"claude-haiku-5-5","permissionMode":"dontAsk","apiKeySource":"none","claude_code_version":"2.1.296","capabilities":["interrupt_receipt_v1"]}
{"_sleep_ms":20000}
{"type":"result","subtype":"success","is_error":false,"result":"never: interrupted first","session_id":"{{session_id}}","total_cost_usd":0,"usage":{},"permission_denials":[],"terminal_reason":"completed"}
{"_wait_user":true}
"#;

    #[tokio::test]
    async fn a_session_round_trip_against_the_fake() {
        let dir = temp_dir("round-trip");
        let script = dir.join("round-trip.jsonl");
        std::fs::write(&script, ROUND_TRIP).unwrap();
        let (program, mut env) = stand_in(&script);
        env.push(("YAP_RUN_KEY".into(), "run-key-1".into()));
        let mut spec = Spec::new(&dir, Pay::Plan);
        spec.env = env;
        spec.mcp_config = Some(json!({"mcpServers": {}}));
        let id = spec.conversation.id().to_string();
        let (session, mut rx) = Session::start_with(&program, spec).unwrap();

        let args = next_where(&mut rx, "args", |e| other_subtype(e, "fake_args")).await;
        let Event::Other(args) = args else { unreachable!() };
        let argv: Vec<String> = serde_json::from_value(args["args"].clone()).unwrap();
        let after = |flag: &str| argv.iter().position(|x| x == flag).map(|i| argv[i + 1].clone());
        assert_eq!(after("--session-id"), Some(id.clone()));
        assert_eq!(after("--input-format").as_deref(), Some("stream-json"));
        assert!(after("--mcp-config").is_some_and(|f| f.ends_with("mcp.json")));
        let env = next_where(&mut rx, "env", |e| other_subtype(e, "fake_env")).await;
        let Event::Other(env) = env else { unreachable!() };
        // The test runs inside whatever started it (a Claude Code session,
        // a terminal): none of its Claude variables reach the run.
        assert_eq!(
            env["present"],
            json!({"ANTHROPIC_API_KEY":false,"CLAUDECODE":false,"CLAUDE_CODE_ENTRYPOINT":false,"ANTHROPIC_BASE_URL":false,"YAP_RUN_KEY":true})
        );

        session.send_user("Hello \"Priya\"", &[]).unwrap();
        let text = next_where(&mut rx, "text", |e| matches!(e, Event::Stream(_))).await;
        assert!(matches!(text, Event::Stream(s) if s.kind == StreamKind::Text { index: 0, text: "You said: Hello \"Priya\"".into() }));
        let Event::Result(r) = next_where(&mut rx, "result", |e| matches!(e, Event::Result(_))).await else { unreachable!() };
        assert_eq!((r.text.as_str(), r.session_id.as_str(), r.is_error), ("You said: Hello \"Priya\"", id.as_str(), false));
        assert_eq!(session.session_id(), id);
        assert_eq!(session.capabilities(), ["interrupt_receipt_v1"]);

        session.send_user("second", &[]).unwrap();
        next_where(&mut rx, "second init", |e| matches!(e, Event::Init(_))).await;
        let Interrupt::Requested { request_id } = session.interrupt().unwrap() else { panic!("asked, not killed") };
        let Event::ControlResponse(c) = next_where(&mut rx, "receipt", |e| matches!(e, Event::ControlResponse(_))).await else { unreachable!() };
        assert!(c.ok);
        assert_eq!(c.request_id, request_id);
        let Event::Result(r) = next_where(&mut rx, "aborted result", |e| matches!(e, Event::Result(_))).await else { unreachable!() };
        assert!(r.interrupted(), "{r:?}");

        session.end();
        let exited = next_where(&mut rx, "exit", |e| matches!(e, Event::Exited(_))).await;
        assert_eq!(exited, Event::Exited(Some(0)), "stdin closed: it exits");
        assert!(!session.is_running());
        assert!(session.send_user("too late", &[]).is_err());
        remove_soon(&dir);
    }

    #[tokio::test]
    async fn a_key_run_has_the_key_and_kill_ends_it() {
        let dir = temp_dir("key");
        let script = dir.join("key.jsonl");
        std::fs::write(&script, "{\"_dump_env\":[\"ANTHROPIC_API_KEY\"]}\n{\"_sleep_ms\":60000}\n").unwrap();
        let (program, env) = stand_in(&script);
        let mut spec = Spec::new(&dir, Pay::ApiKey(ApiKey::new("sk-ant-test-only")));
        spec.env = env;
        let (session, mut rx) = Session::start_with(&program, spec).unwrap();
        let Event::Other(env) = next_where(&mut rx, "env", |e| other_subtype(e, "fake_env")).await else { unreachable!() };
        assert_eq!(env["present"]["ANTHROPIC_API_KEY"], true);
        assert!(session.is_running());
        session.kill();
        assert!(matches!(next_where(&mut rx, "exit", |e| matches!(e, Event::Exited(_))).await, Event::Exited(_)));
        assert!(!session.is_running());
        remove_soon(&dir);
    }

    #[tokio::test]
    async fn oneshot_answers_or_says_what_went_wrong() {
        let dir = temp_dir("oneshot");
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ai/claude_code/fixtures/2.1.296");
        let (program, env) = stand_in(&fixtures.join("synthetic-text-and-thinking.jsonl"));
        let mut spec = Spec::new(&dir, Pay::Plan);
        spec.env = env;
        let reply = oneshot_with(&program, spec, "Say hello", Duration::from_secs(30)).await.unwrap();
        assert_eq!(reply.text, "Hello, Imogen. The meeting is at 3 pm.");
        assert_eq!(reply.model, "claude-haiku-5-5");
        assert_eq!(reply.usage.output_tokens, 42);
        assert!((reply.cost_usd - 0.0021).abs() < 1e-9);

        // The real signed-out recording, replayed: not an answer.
        let (program, env) = stand_in(&fixtures.join("real-not-logged-in.jsonl"));
        let mut spec = Spec::new(&dir, Pay::Plan);
        spec.env = env;
        let err = oneshot_with(&program, spec, "Say hello", Duration::from_secs(30)).await.unwrap_err();
        assert_eq!(err, OneshotError::Problem(Problem::NotSignedIn));
        assert!(err.to_string().contains("isn't signed in"));

        // A plan run that Claude Code would bill to a key is stopped.
        let (program, env) = stand_in(&fixtures.join("real-invalid-api-key.jsonl"));
        let mut spec = Spec::new(&dir, Pay::Plan);
        spec.env = env;
        let err = oneshot_with(&program, spec, "Say hello", Duration::from_secs(30)).await.unwrap_err();
        assert_eq!(err, OneshotError::Problem(Problem::PaysAnotherWay { source: "ANTHROPIC_API_KEY".into() }));

        // Too slow.
        let slow = dir.join("slow.jsonl");
        std::fs::write(&slow, "{\"_wait_user\":true}\n{\"_sleep_ms\":60000}\n").unwrap();
        let (program, env) = stand_in(&slow);
        let mut spec = Spec::new(&dir, Pay::Plan);
        spec.env = env;
        let started = Instant::now();
        let err = oneshot_with(&program, spec, "Say hello", Duration::from_millis(1500)).await.unwrap_err();
        assert_eq!(err, OneshotError::Failed("Claude Code took too long to answer.".into()));
        assert!(started.elapsed() < Duration::from_secs(20));
        remove_soon(&dir);
    }

    #[test]
    fn a_missing_folder_or_program_is_said_plainly() {
        let spec = Spec::new(PathBuf::from(r"Z:\no\such\folder\yap"), Pay::Plan);
        let program = Program::new(std::env::current_exe().unwrap());
        assert_eq!(Session::start_with(&program, spec).err().as_deref(), Some("The folder for Claude Code isn't there."));
        let spec = Spec::new(std::env::temp_dir(), Pay::Plan);
        let nothing = Program::new(PathBuf::from(r"Z:\no\such\claude.exe"));
        assert!(Session::start_with(&nothing, spec).err().unwrap().starts_with("Couldn't start Claude Code"));
    }

    /// The person's real Claude Code, signed out on purpose: an empty
    /// CLAUDE_CONFIG_DIR of its own, so it never sees their login, and the
    /// API never reached. Run by hand: `cargo test claude_code::session --
    /// --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn the_real_claude_code_signed_out_says_so() {
        let Some(program) = cli::find_in(dirs::home_dir().as_deref(), std::env::var_os("PATH").as_deref(), std::env::var_os("PATHEXT").as_deref())
        else {
            println!("no Claude Code on this PC");
            return;
        };
        let dir = temp_dir("real");
        let config = dir.join("empty-config");
        std::fs::create_dir_all(&config).unwrap();
        let mut spec = Spec::new(&dir, Pay::Plan);
        spec.env = vec![("CLAUDE_CONFIG_DIR".into(), config.display().to_string())];
        spec.tools = Some(vec![]);
        spec.strict_mcp_config = true;
        spec.setting_sources = Some(vec![]);
        spec.append_system_prompt = Some("Answer in one word.".into());
        spec.effort = Some(Effort::Low);
        let id = spec.conversation.id().to_string();
        let (session, mut rx) = Session::start_with(&program, spec).unwrap();
        session.send_user("Hello", &[]).unwrap();
        let mut seen: Vec<Event> = Vec::new();
        let done = tokio::time::timeout(Duration::from_secs(60), async {
            while let Some(e) = rx.recv().await {
                let stop = matches!(e, Event::Result(_));
                seen.push(e);
                if stop {
                    break;
                }
            }
        })
        .await;
        assert!(done.is_ok(), "a result in time");
        let init = seen.iter().find_map(|e| if let Event::Init(i) = e { Some(i.clone()) } else { None }).expect("init");
        println!("version {}, capabilities {:?}", init.version, init.capabilities);
        assert_eq!(init.session_id, id);
        assert_eq!(init.api_key_source, "none");
        assert!(init.can("interrupt_receipt_v1"));
        assert!(seen.contains(&Event::Problem(Problem::NotSignedIn)), "{seen:?}");
        let r: Vec<&TurnResult> = seen.iter().filter_map(|e| if let Event::Result(r) = e { Some(r) } else { None }).collect();
        assert!(r[0].is_error);
        let Interrupt::Requested { .. } = session.interrupt().unwrap() else { panic!() };
        let receipt = next_where(&mut rx, "receipt", |e| matches!(e, Event::ControlResponse(_))).await;
        println!("{receipt:?}");
        session.end();
        let exited = next_where(&mut rx, "exit", |e| matches!(e, Event::Exited(_))).await;
        println!("{exited:?}");
        remove_soon(&dir);
    }
}

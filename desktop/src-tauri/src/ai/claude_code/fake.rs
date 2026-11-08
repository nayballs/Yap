//! `yap.exe fake-claude <script.jsonl> [Claude Code's arguments…]` (debug
//! builds): a stand-in for Claude Code that replays a script, for tests.
//! Point `YAP_CLAUDE_EXE` at `yap.exe` and set `YAP_CLAUDE_ARGS_PREFIX` to
//! `["fake-claude","<script.jsonl>"]`, and Yap drives it as it would the
//! real one (`cli.rs`).
//!
//! **The script**, one JSON object a line (blank lines and `//` comments
//! skipped). A line is printed as it is, with `{{session_id}}` (from
//! `--session-id` / `--resume`, else a new one), `{{user_text}}` (the last
//! user message's words) and `{{cwd}}` filled in, JSON-escaped. A line whose
//! first key starts with `_` is an instruction:
//!
//! - `{"_wait_user":true}`: wait for the next user message on stdin (one
//!   sent earlier counts). stdin closed: the fake exits 0, as Claude Code
//!   does.
//! - `{"_sleep_ms":N}`: wait N ms (an interrupt still answered).
//! - `{"_hook":{"event":"PreToolUse","input":{…}}}`: POST `input` (with
//!   `session_id`, `hook_event_name` and `cwd` added when missing) to every
//!   http hook for that event in the `--settings` it was given whose matcher
//!   fits `input.tool_name`, with the hook's headers (`$VAR` / `${VAR}` for
//!   the variables in its `allowedEnvVars`). Prints a `system/hook_response`
//!   per hook, `output` the reply's body.
//! - `{"_mcp":{"tool":"approve","arguments":{…},"server":"yap"}}`: call a
//!   tool of the http MCP server named in `--mcp-config` (`server`, else the
//!   first): `initialize`, `notifications/initialized`, `tools/call`, with
//!   its headers (`${VAR}` / `${VAR:-default}` from the environment, as
//!   Claude Code does). Prints `{"type":"system","subtype":"fake_mcp",
//!   "server":…,"tool":…,"response":<the JSON-RPC response>|null,"error":…}`.
//!   With `"result_for":"<tool_use id>"`, then also the call's answer as
//!   Claude Code hands it to the model: a `user` message with that id's
//!   `tool_result` (the answer's text blocks, its image content as image
//!   blocks, its `isError`). With `"dump":"<file>"`, a line appended to that
//!   file with what the call gave (its content's blocks: text whole, an
//!   image's type and size, never its data), for an e2e spec.
//! - `{"_for":"chat"}` (or `"agent"`, `"ask"`, `"all"`): the lines up to the
//!   next `_for` play only in a process of that kind — `agent` started with
//!   `--permission-prompt-tool` (an agent's run), `chat` with an
//!   `--mcp-config` but no prompt tool (a plan chat's session), `ask` with
//!   neither (one question, no tools) — so one script plays a plan chat,
//!   the agent it starts and the report's question. `all` (as from the top):
//!   every kind.
//! - `{"_dump_args":true}`: prints `{"type":"system","subtype":"fake_args",
//!   "args":[…],"cwd":…,"pid":…,"user_text":…}` (its process id, and the
//!   last user message's words: after a `_wait_user`, which process served
//!   which call). `{"_dump_args":"<file>"}`: and appends that
//!   line to the file (an e2e spec reads a plan chat's launch there, or an
//!   agent's turns' arguments: `--session-id`, then `--resume`).
//! - `{"_dump_env":["ANTHROPIC_API_KEY",…]}`: prints `{"type":"system",
//!   "subtype":"fake_env","present":{"ANTHROPIC_API_KEY":false,…}}` (never a
//!   value).
//! - `{"_stderr":"text"}`: a line on stderr.
//! - `{"_write_file":{"path":"notes.md","text":"…"}}`: write a file inside
//!   the working folder (an agent's edit, after its backup hook).
//! - `{"_exit":N}`: exit with code N.
//! - `{"_usage":{…}}`: from now on, a `get_usage` control request is
//!   answered with this body (`control_response` / `success` / `response`:
//!   it, as 2.1.296 answers; `claude_code/usage.rs`). `{"_usage":null}`:
//!   refused again, as before any `_usage` (an older Claude Code).
//!
//! **stdin** is read the whole time: user messages queue for `_wait_user`;
//! an interrupt control request is answered at once
//! (`control_response` / `success` / `{"still_queued":[]}`, as 2.1.296
//! does) and, mid-turn, ends the turn with the fake's own aborted result
//! (`subtype: "error_during_execution"`, `terminal_reason:
//! "aborted_streaming"`), the script skipping on to the next `_wait_user`.
//! `get_usage` is answered with the script's `_usage`, if it set one. Any
//! other control request gets an error response.
//!
//! **Its transcript**, as Claude Code saves a session: with
//! `YAP_FAKE_CLAUDE_TRANSCRIPTS=1` and a `CLAUDE_CONFIG_DIR` (a test's own
//! folder: never without both, so never the person's own `~/.claude`), each
//! user message is appended to
//! `<CLAUDE_CONFIG_DIR>/projects/<cwd, encoded>/<session>.jsonl`
//! (`transcripts::project_dir_name`), unless it was started with
//! `--no-session-persistence`.
//!
//! **Start-up**: `YAP_FAKE_CLAUDE_START_MS` waits that long before reading
//! or printing anything, as Claude Code's own start does (a plan scope's
//! warm spares are measured against it).
//!
//! **`--version`** prints `2.1.296 (Claude Code)` (`YAP_FAKE_CLAUDE_VERSION`
//! for another); **`auth status`** prints a signed-in status
//! (`YAP_FAKE_CLAUDE_LOGGED_IN=0`: signed out, exit 1); **`auth login`**
//! says it signed in. Example scripts: `fixtures/fake/`; every recording in
//! `fixtures/<version>/` replays too.

mod http;

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, Write};
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

/// `yap.exe fake-claude …`'s entry: `args` are what follows `fake-claude`.
pub fn main(args: Vec<String>) -> i32 {
    let Some((script_path, claude_args)) = args.split_first() else {
        eprintln!("usage: yap.exe fake-claude <script.jsonl> [Claude Code's arguments…]");
        return 2;
    };
    if claude_args.iter().any(|a| a == "--version" || a == "-v") {
        let version = std::env::var("YAP_FAKE_CLAUDE_VERSION").unwrap_or_else(|_| "2.1.296".into());
        println!("{version} (Claude Code)");
        return 0;
    }
    if claude_args.first().is_some_and(|a| a == "auth") {
        return match claude_args.get(1).map(String::as_str) {
            Some("status") => {
                let signed_in = std::env::var("YAP_FAKE_CLAUDE_LOGGED_IN").map_or(true, |v| v.trim() != "0");
                let method = if signed_in { "claude.ai" } else { "none" };
                println!("{}", json!({"loggedIn": signed_in, "authMethod": method, "apiProvider": "firstParty"}));
                if signed_in {
                    0
                } else {
                    1
                }
            }
            Some("login") => {
                println!("fake-claude: signed in");
                0
            }
            _ => {
                eprintln!("fake-claude: unknown auth command");
                2
            }
        };
    }
    let script = match std::fs::read_to_string(script_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("fake-claude: can't read the script: {e}");
            return 2;
        }
    };
    let ctx = match Ctx::from_args(claude_args) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("fake-claude: {e}");
            return 2;
        }
    };
    // Claude Code's own start-up, simulated (`YAP_FAKE_CLAUDE_START_MS`):
    // nothing is read or printed until it's over (claude_plan/spare.rs'
    // measurements).
    if let Some(ms) = std::env::var("YAP_FAKE_CLAUDE_START_MS").ok().and_then(|v| v.trim().parse::<u64>().ok()).filter(|ms| *ms > 0) {
        std::thread::sleep(Duration::from_millis(ms.min(60_000)));
    }
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    match play(&script, &ctx, rx, &mut out) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("fake-claude: {e}");
            1
        }
    }
}

/// What the fake was started with.
#[derive(Clone, Debug, Default)]
pub struct Ctx {
    pub session_id: String,
    pub cwd: String,
    /// Claude Code's arguments, as given.
    pub args: Vec<String>,
    /// `--settings` (a file, or JSON).
    pub settings: Option<Value>,
    /// `--mcp-config`'s servers, merged (`{name: config}`).
    pub mcp_servers: Map<String, Value>,
    /// Variables to read instead of the environment (tests).
    pub vars: Option<HashMap<String, String>>,
    /// Started without `--no-session-persistence`: a transcript is kept
    /// (when the test asks for one: [`Ctx::transcript`]).
    pub persist: bool,
}

impl Ctx {
    pub fn from_args(args: &[String]) -> Result<Ctx, String> {
        let mut ctx = Ctx {
            args: args.to_vec(),
            cwd: std::env::current_dir().map(|d| d.display().to_string()).unwrap_or_default(),
            persist: !args.iter().any(|a| a == "--no-session-persistence"),
            ..Default::default()
        };
        let mut i = 0;
        while i < args.len() {
            let flag = args[i].as_str();
            let value = args.get(i + 1);
            match (flag, value) {
                ("--session-id" | "--resume" | "-r", Some(v)) => {
                    ctx.session_id = v.clone();
                    i += 1;
                }
                ("--settings", Some(v)) => {
                    ctx.settings = Some(json_or_file(v)?);
                    i += 1;
                }
                ("--mcp-config", _) => {
                    // Variadic: every value up to the next flag.
                    while let Some(v) = args.get(i + 1).filter(|v| !v.starts_with('-')) {
                        let config = json_or_file(v)?;
                        if let Some(servers) = config.get("mcpServers").and_then(Value::as_object) {
                            ctx.mcp_servers.extend(servers.clone());
                        }
                        i += 1;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        if ctx.session_id.is_empty() {
            ctx.session_id = super::session::new_session_id();
        }
        Ok(ctx)
    }

    fn var(&self, name: &str) -> Option<String> {
        match &self.vars {
            Some(vars) => vars.get(name).cloned(),
            None => std::env::var(name).ok(),
        }
    }

    /// Where this session's transcript goes, as Claude Code files it: only
    /// with `YAP_FAKE_CLAUDE_TRANSCRIPTS=1` and a `CLAUDE_CONFIG_DIR`, and
    /// never under `--no-session-persistence`.
    pub fn transcript(&self) -> Option<std::path::PathBuf> {
        if !self.persist || self.var("YAP_FAKE_CLAUDE_TRANSCRIPTS").is_none_or(|v| v.trim() != "1") {
            return None;
        }
        let config = self.var("CLAUDE_CONFIG_DIR").filter(|v| !v.trim().is_empty())?;
        Some(std::path::Path::new(&config).join("projects").join(super::transcripts::project_dir_name(&self.cwd)).join(format!("{}.jsonl", self.session_id)))
    }

    /// What kind of run this is, for `_for`: "agent" (a permission prompt
    /// tool), "chat" (an MCP server, no prompt tool) or "ask" (neither).
    pub fn kind(&self) -> &'static str {
        if self.args.iter().any(|a| a == "--permission-prompt-tool") {
            "agent"
        } else if self.args.iter().any(|a| a == "--mcp-config") {
            "chat"
        } else {
            "ask"
        }
    }
}

fn json_or_file(v: &str) -> Result<Value, String> {
    let text = if v.trim_start().starts_with('{') {
        v.to_string()
    } else {
        std::fs::read_to_string(v).map_err(|e| format!("can't read {v}: {e}"))?
    };
    serde_json::from_str(&text).map_err(|e| format!("not JSON ({v}): {e}"))
}

/// Play `script`: `input` is stdin's lines (closed: stdin closed). The exit
/// code.
pub fn play(script: &str, ctx: &Ctx, input: Receiver<String>, out: &mut dyn Write) -> Result<i32, String> {
    let mut p = Player { ctx, input, out, queue: VecDeque::new(), user_text: String::new(), in_turn: false, skipping: false, closed: false, hooks: 0, usage: None };
    // `_for`: the kind of run the lines play in (`None`: every kind).
    let kind = ctx.kind();
    let mut part: Option<String> = None;
    for raw in script.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        p.drain()?;
        let v: Value = serde_json::from_str(line).map_err(|e| format!("a script line isn't JSON: {e}"))?;
        let control = v.as_object().and_then(|o| o.iter().next()).filter(|(k, _)| k.starts_with('_'));
        if let Some(("_for", arg)) = control.map(|(k, a)| (k.as_str(), a)) {
            part = arg.as_str().filter(|s| *s != "all").map(str::to_string);
            continue;
        }
        if part.as_deref().is_some_and(|p| p != kind) {
            continue;
        }
        if p.skipping && control.is_none_or(|(k, _)| k != "_wait_user") {
            continue;
        }
        match control {
            Some((key, arg)) => {
                if let Some(code) = p.instruction(key, arg)? {
                    return Ok(code);
                }
            }
            None => {
                let text = p.fill(line);
                p.print(&text)?;
                if v["type"] == "result" {
                    p.in_turn = false;
                }
            }
        }
    }
    Ok(0)
}

struct Player<'a> {
    ctx: &'a Ctx,
    input: Receiver<String>,
    out: &'a mut dyn Write,
    /// User messages not yet waited for.
    queue: VecDeque<String>,
    user_text: String,
    in_turn: bool,
    /// Interrupted: skip to the next `_wait_user`.
    skipping: bool,
    closed: bool,
    hooks: u32,
    /// What `get_usage` answers (`_usage`); none: refused.
    usage: Option<Value>,
}

impl Player<'_> {
    fn print(&mut self, line: &str) -> Result<(), String> {
        writeln!(self.out, "{line}").and_then(|_| self.out.flush()).map_err(|e| format!("stdout closed: {e}"))
    }

    fn fill(&self, line: &str) -> String {
        line.replace("{{session_id}}", &self.ctx.session_id)
            .replace("{{user_text}}", &json_inner(&self.user_text))
            .replace("{{cwd}}", &json_inner(&self.ctx.cwd))
    }

    /// Answer whatever stdin has sent, without waiting.
    fn drain(&mut self) -> Result<(), String> {
        loop {
            match self.input.try_recv() {
                Ok(line) => self.take(&line)?,
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) => {
                    self.closed = true;
                    return Ok(());
                }
            }
        }
    }

    /// One stdin line: a user message queued, a control request answered.
    fn take(&mut self, line: &str) -> Result<(), String> {
        let Ok(v) = serde_json::from_str::<Value>(line) else { return Ok(()) };
        match v["type"].as_str() {
            Some("user") => self.queue.push_back(user_words(&v)),
            Some("control_request") => {
                let id = v["request_id"].clone();
                if v["request"]["subtype"] == "interrupt" {
                    let receipt = json!({"type":"control_response","response":{"subtype":"success","request_id":id,"response":{"still_queued":[]}}});
                    self.print(&receipt.to_string())?;
                    if self.in_turn {
                        self.in_turn = false;
                        self.skipping = true;
                        let aborted = json!({
                            "type": "result", "subtype": "error_during_execution", "is_error": true,
                            "terminal_reason": "aborted_streaming", "result": "", "errors": [],
                            "session_id": self.ctx.session_id, "total_cost_usd": 0, "num_turns": 1, "duration_ms": 0,
                            "usage": {"input_tokens": 0, "output_tokens": 0, "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0},
                            "permission_denials": []
                        });
                        self.print(&aborted.to_string())?;
                    }
                } else if let (Some("get_usage"), Some(body)) = (v["request"]["subtype"].as_str(), self.usage.as_ref()) {
                    let answer = json!({"type":"control_response","response":{"subtype":"success","request_id":id,"response":body}});
                    self.print(&answer.to_string())?;
                } else {
                    let refused = json!({"type":"control_response","response":{"subtype":"error","request_id":id,"error":"fake-claude doesn't do this control request"}});
                    self.print(&refused.to_string())?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Run an instruction; `Some(code)` to exit.
    fn instruction(&mut self, key: &str, arg: &Value) -> Result<Option<i32>, String> {
        match key {
            "_wait_user" => {
                self.skipping = false;
                loop {
                    if let Some(text) = self.queue.pop_front() {
                        self.record(&text);
                        self.user_text = text;
                        self.in_turn = true;
                        return Ok(None);
                    }
                    if self.closed {
                        return Ok(Some(0));
                    }
                    match self.input.recv() {
                        Ok(line) => self.take(&line)?,
                        Err(_) => self.closed = true,
                    }
                }
            }
            "_sleep_ms" => {
                let until = Instant::now() + Duration::from_millis(arg.as_u64().unwrap_or(0));
                while !self.skipping {
                    let left = until.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        break;
                    }
                    if self.closed {
                        std::thread::sleep(left);
                        break;
                    }
                    match self.input.recv_timeout(left) {
                        Ok(line) => self.take(&line)?,
                        Err(RecvTimeoutError::Timeout) => break,
                        Err(RecvTimeoutError::Disconnected) => self.closed = true,
                    }
                }
            }
            "_hook" => {
                let event = arg["event"].as_str().unwrap_or("").to_string();
                for line in self.hooks(&event, &arg["input"]) {
                    self.print(&line)?;
                }
            }
            "_mcp" => {
                for line in self.mcp(arg) {
                    self.print(&line)?;
                }
            }
            "_dump_args" => {
                // Its process id too: which process served which call (a
                // plan scope's spares, one call each).
                let line = json!({"type":"system","subtype":"fake_args","args":self.ctx.args,"cwd":self.ctx.cwd,"session_id":self.ctx.session_id,"pid":std::process::id(),"user_text":self.user_text});
                // A path: the line appended to that file too, for a test
                // that can't read the run's stdout (an e2e spec).
                if let Some(file) = arg.as_str().filter(|f| !f.trim().is_empty()) {
                    let written = std::fs::OpenOptions::new().create(true).append(true).open(file).and_then(|mut f| writeln!(f, "{line}"));
                    if let Err(e) = written {
                        eprintln!("fake-claude: couldn't write the arguments: {e}");
                    }
                }
                self.print(&line.to_string())?;
            }
            "_dump_env" => {
                let present: Map<String, Value> = arg
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .map(|name| (name.to_string(), Value::Bool(self.ctx.var(name).is_some_and(|v| !v.is_empty()))))
                    .collect();
                let line = json!({"type":"system","subtype":"fake_env","present":present,"session_id":self.ctx.session_id});
                self.print(&line.to_string())?;
            }
            "_stderr" => eprintln!("{}", arg.as_str().unwrap_or("")),
            "_write_file" => {
                // An agent's edit, as Claude Code's Edit tool makes it: `path`
                // relative to the working folder (never outside it).
                let rel = arg["path"].as_str().unwrap_or("");
                let safe = !rel.is_empty() && std::path::Path::new(rel).components().all(|c| matches!(c, std::path::Component::Normal(_)));
                if safe {
                    let path = std::path::Path::new(&self.ctx.cwd).join(rel);
                    if let Err(e) = std::fs::write(&path, arg["text"].as_str().unwrap_or("")) {
                        eprintln!("fake-claude: couldn't write {rel}: {e}");
                    }
                } else {
                    eprintln!("fake-claude: _write_file takes a path inside the working folder");
                }
            }
            "_exit" => return Ok(Some(arg.as_i64().unwrap_or(0) as i32)),
            "_usage" => self.usage = Some(arg.clone()).filter(|a| !a.is_null()),
            other => eprintln!("fake-claude: unknown instruction {other}"),
        }
        Ok(None)
    }

    /// A user message into the session's transcript, as Claude Code keeps
    /// one ([`Ctx::transcript`]).
    fn record(&self, text: &str) {
        let Some(path) = self.ctx.transcript() else { return };
        let line = json!({"type":"user","sessionId":self.ctx.session_id,"cwd":self.ctx.cwd,"message":{"role":"user","content":text}}).to_string();
        let wrote = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| {
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
            writeln!(f, "{line}")
        });
        if let Err(e) = wrote {
            eprintln!("fake-claude: couldn't keep the transcript: {e}");
        }
    }

    /// POST to the matching http hooks; a `hook_response` line each.
    fn hooks(&mut self, event: &str, input: &Value) -> Vec<String> {
        let ctx = self.ctx;
        let mut body = input.as_object().cloned().unwrap_or_default();
        body.entry("session_id").or_insert_with(|| json!(self.ctx.session_id));
        body.entry("hook_event_name").or_insert_with(|| json!(event));
        body.entry("cwd").or_insert_with(|| json!(self.ctx.cwd));
        let body = Value::Object(body).to_string();
        let tool = input["tool_name"].as_str().unwrap_or("");
        let groups = self.ctx.settings.as_ref().and_then(|s| s["hooks"][event].as_array().cloned()).unwrap_or_default();
        let mut lines = Vec::new();
        for group in groups.iter().filter(|g| matcher_fits(g["matcher"].as_str().unwrap_or(""), tool)) {
            for hook in group["hooks"].as_array().into_iter().flatten().filter(|h| h["type"] == "http") {
                self.hooks += 1;
                let allowed: Vec<String> =
                    hook["allowedEnvVars"].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
                let headers = header_list(&hook["headers"], |s| expand(s, |name| allowed.iter().any(|a| a == name).then(|| ctx.var(name)).flatten(), false));
                let url = hook["url"].as_str().unwrap_or("");
                let (outcome, output, status) = match http::post(url, &headers, body.as_bytes(), |_| false) {
                    Ok(r) => (if (200..300).contains(&r.status) { "success" } else { "error" }, String::from_utf8_lossy(&r.body).into_owned(), r.status),
                    Err(e) => ("error", e, 0),
                };
                lines.push(
                    json!({
                        "type": "system", "subtype": "hook_response", "hook_id": format!("fake-hook-{}", self.hooks),
                        "hook_name": event, "hook_event": event, "output": output, "stdout": "", "stderr": "",
                        "exit_code": 0, "outcome": outcome, "http_status": status, "session_id": self.ctx.session_id
                    })
                    .to_string(),
                );
            }
        }
        lines
    }

    /// A tool call on the run's MCP server: the `fake_mcp` line, and with
    /// `result_for` the `tool_result` the model would get. With `dump`, a
    /// line appended to that file: the result's content blocks (an image's
    /// type and size, never its data; [`dumped`]).
    fn mcp(&self, arg: &Value) -> Vec<String> {
        let line = self.mcp_call(arg);
        let v: Value = serde_json::from_str(&line).unwrap_or(Value::Null);
        if let Some(file) = arg["dump"].as_str().filter(|f| !f.trim().is_empty()) {
            let written = std::fs::OpenOptions::new().create(true).append(true).open(file).and_then(|mut f| writeln!(f, "{}", dumped(&v)));
            if let Err(e) = written {
                eprintln!("fake-claude: couldn't write the MCP call: {e}");
            }
        }
        let Some(id) = arg["result_for"].as_str() else { return vec![line] };
        let content = match (&v["response"]["result"], &v["response"]["error"], &v["error"]) {
            (r, _, _) if r.is_object() => Ok((tool_result_content(&r["content"]), r["isError"].as_bool().unwrap_or(false))),
            (_, e, _) if e.is_object() => Err(format!("MCP error {}: {}", e["code"], e["message"].as_str().unwrap_or(""))),
            (_, _, e) => Err(e.as_str().unwrap_or("The MCP call failed.").to_string()),
        };
        let (content, is_error) = content.unwrap_or_else(|text| (vec![json!({ "type": "text", "text": text })], true));
        let result = json!({
            "type": "user",
            "message": { "role": "user", "content": [{ "type": "tool_result", "tool_use_id": id, "content": content, "is_error": is_error }] },
            "parent_tool_use_id": null,
            "session_id": self.ctx.session_id,
        });
        vec![line, result.to_string()]
    }

    /// The call itself; the `fake_mcp` line.
    fn mcp_call(&self, arg: &Value) -> String {
        let tool = arg["tool"].as_str().unwrap_or("");
        let named = arg["server"].as_str();
        let server = match named {
            Some(n) => self.ctx.mcp_servers.get(n).map(|s| (n.to_string(), s.clone())),
            None => self.ctx.mcp_servers.iter().next().map(|(n, s)| (n.clone(), s.clone())),
        };
        let (name, result) = match server {
            Some((name, config)) => {
                let r = mcp_call(&config, tool, arg.get("arguments").cloned().unwrap_or(json!({})), |v| self.ctx.var(v));
                (name, r)
            }
            None => (String::new(), Err("no such MCP server in --mcp-config".to_string())),
        };
        let (response, error) = match result {
            Ok(r) => (r, Value::Null),
            Err(e) => (Value::Null, Value::String(e)),
        };
        json!({"type":"system","subtype":"fake_mcp","server":name,"tool":tool,"response":response,"error":error,"session_id":self.ctx.session_id}).to_string()
    }
}

/// **An MCP result's content as Claude Code hands it to the model** (2.1.296,
/// its MCP client): the text blocks' words joined into one text block, and
/// each image content (`{"type":"image","data","mimeType"}`) as an image
/// block (`{"type":"image","source":{"type":"base64","media_type","data"}}`)
/// in its place. Pure.
fn tool_result_content(content: &Value) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let mut words = String::new();
    for b in content.as_array().into_iter().flatten() {
        match b["type"].as_str() {
            Some("text") => words.push_str(b["text"].as_str().unwrap_or("")),
            Some("image") => {
                if !words.is_empty() {
                    out.push(json!({ "type": "text", "text": std::mem::take(&mut words) }));
                }
                out.push(json!({ "type": "image", "source": { "type": "base64", "media_type": b["mimeType"], "data": b["data"] } }));
            }
            _ => {}
        }
    }
    if !words.is_empty() || out.is_empty() {
        out.push(json!({ "type": "text", "text": words }));
    }
    out
}

/// **What an MCP call gave, for a test to read** (`_mcp`'s `dump`): the
/// tool, whether it failed, and its content — text whole, an image's type
/// and size in bytes (never its data). Pure.
fn dumped(v: &Value) -> Value {
    let result = &v["response"]["result"];
    let content: Vec<Value> = result["content"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|b| match b["type"].as_str() {
            Some("image") => {
                let data = b["data"].as_str().unwrap_or("");
                let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data).map_or(0, |d| d.len());
                json!({ "type": "image", "mimeType": b["mimeType"], "bytes": bytes })
            }
            _ => json!({ "type": b["type"], "text": b["text"] }),
        })
        .collect();
    json!({ "tool": v["tool"], "isError": result["isError"].as_bool().unwrap_or(v["error"].is_string()), "content": content, "error": v["error"] })
}

/// A user message's words (its text blocks, joined).
fn user_words(v: &Value) -> String {
    match &v["message"]["content"] {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join(""),
        _ => String::new(),
    }
}

/// `s` escaped for the inside of a JSON string.
fn json_inner(s: &str) -> String {
    let quoted = Value::String(s.to_string()).to_string();
    quoted[1..quoted.len() - 1].to_string()
}

/// A hook group's matcher against a tool name: none, `""` or `*` fits
/// everything; otherwise a regex (`Edit|Write`) matched whole.
fn matcher_fits(matcher: &str, tool: &str) -> bool {
    let m = matcher.trim();
    if m.is_empty() || m == "*" {
        return true;
    }
    regex::Regex::new(&format!("^(?:{m})$")).is_ok_and(|re| re.is_match(tool))
}

fn header_list(headers: &Value, fill: impl Fn(&str) -> String) -> Vec<(String, String)> {
    headers
        .as_object()
        .map(|h| h.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), fill(v)))).collect())
        .unwrap_or_default()
}

/// `$VAR`, `${VAR}` (and with `defaults`, `${VAR:-default}`) filled in by
/// `lookup` (a variable it doesn't give: empty, or the default).
fn expand(s: &str, lookup: impl Fn(&str) -> Option<String>, defaults: bool) -> String {
    let re = regex::Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)(?::-([^}]*))?\}|\$([A-Za-z_][A-Za-z0-9_]*)").expect("a fixed pattern");
    re.replace_all(s, |c: &regex::Captures| {
        let name = c.get(1).or_else(|| c.get(3)).map_or("", |m| m.as_str());
        let default = if defaults { c.get(2).map(|m| m.as_str().to_string()) } else { None };
        lookup(name).filter(|v| !v.is_empty()).or(default).unwrap_or_default()
    })
    .into_owned()
}

/// `initialize`, `notifications/initialized`, then `tools/call` on an http
/// MCP server: the call's JSON-RPC response.
fn mcp_call(config: &Value, tool: &str, arguments: Value, var: impl Fn(&str) -> Option<String>) -> Result<Value, String> {
    let url = config["url"].as_str().ok_or("the MCP server has no url")?;
    let mut headers = header_list(&config["headers"], |s| expand(s, &var, true));
    let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"fake-claude","version":"0.0.0"}}});
    let (reply, session) = rpc(url, &headers, &init, 1)?;
    if let Some(error) = reply.get("error") {
        return Err(format!("initialize failed: {error}"));
    }
    if let Some(s) = session {
        headers.push(("Mcp-Session-Id".into(), s));
    }
    let version = reply["result"]["protocolVersion"].as_str().unwrap_or("2025-11-25").to_string();
    headers.push(("MCP-Protocol-Version".into(), version));
    let note = json!({"jsonrpc":"2.0","method":"notifications/initialized"}).to_string();
    let r = http::post(url, &headers, note.as_bytes(), |_| false)?;
    if !(200..300).contains(&r.status) {
        return Err(format!("notifications/initialized: HTTP {}", r.status));
    }
    let call = json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":tool,"arguments":arguments}});
    Ok(rpc(url, &headers, &call, 2)?.0)
}

/// One JSON-RPC request: its response (a JSON body, or the SSE event that
/// carries it) and the server's session id.
fn rpc(url: &str, headers: &[(String, String)], request: &Value, id: u64) -> Result<(Value, Option<String>), String> {
    let body = request.to_string();
    let found = |bytes: &[u8]| sse_response(bytes, id).is_some();
    let r = http::post(url, headers, body.as_bytes(), found)?;
    if !(200..300).contains(&r.status) {
        return Err(format!("HTTP {}", r.status));
    }
    let session = r.header("mcp-session-id").map(str::to_string);
    let reply = if r.header("content-type").is_some_and(|t| t.contains("text/event-stream")) {
        sse_response(&r.body, id).ok_or("no response in the event stream")?
    } else {
        serde_json::from_slice(&r.body).map_err(|e| format!("the response isn't JSON: {e}"))?
    };
    Ok((reply, session))
}

/// The JSON-RPC response with `id` in an SSE body, once it's all there.
fn sse_response(body: &[u8], id: u64) -> Option<Value> {
    let text = String::from_utf8_lossy(body);
    for event in text.replace("\r\n", "\n").split("\n\n") {
        let data: Vec<&str> = event.lines().filter_map(|l| l.strip_prefix("data:")).map(|d| d.strip_prefix(' ').unwrap_or(d)).collect();
        if data.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(&data.join("\n")) {
            if v["id"] == json!(id) && (v.get("result").is_some() || v.get("error").is_some()) {
                return Some(v);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn ctx(args: &[&str]) -> Ctx {
        let mut c = Ctx::from_args(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>()).unwrap();
        c.cwd = r"C:\Users\imogen\Projects\example".into();
        c
    }

    /// Play `script` with `input` lines on stdin (then stdin closed when
    /// `close`): the exit code and the output lines.
    fn run(script: &str, c: &Ctx, input: &[&str], close: bool) -> (i32, Vec<Value>) {
        let (tx, rx) = std::sync::mpsc::channel();
        for line in input {
            tx.send(line.to_string()).unwrap();
        }
        let keep = (!close).then_some(tx);
        let mut out = Vec::new();
        let code = play(script, c, rx, &mut out).unwrap();
        drop(keep);
        (code, lines(&out))
    }

    /// Play `script` while stdin's lines arrive, each after its delay (ms);
    /// stdin closes after the last.
    fn run_timed(script: &str, c: &Ctx, input: Vec<(u64, &str)>) -> (i32, Vec<Value>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let input: Vec<(u64, String)> = input.into_iter().map(|(ms, l)| (ms, l.to_string())).collect();
        std::thread::spawn(move || {
            for (ms, line) in input {
                std::thread::sleep(Duration::from_millis(ms));
                let _ = tx.send(line);
            }
        });
        let mut out = Vec::new();
        let code = play(script, c, rx, &mut out).unwrap();
        (code, lines(&out))
    }

    fn lines(out: &[u8]) -> Vec<Value> {
        String::from_utf8_lossy(out).lines().map(|l| serde_json::from_str(l).unwrap()).collect()
    }

    const USER: &str = r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"Is \"Priya\" in?"}]}}"#;

    #[test]
    fn it_fills_in_the_session_and_what_was_said() {
        let c = ctx(&["-p", "--session-id", "5c6d7e8f-9a0b-4c1d-8e2f-3a4b5c6d7e8f"]);
        let script = "// a comment\n\n{\"_wait_user\":true}\n{\"type\":\"result\",\"result\":\"{{user_text}} in {{cwd}}\",\"session_id\":\"{{session_id}}\"}\n{\"_wait_user\":true}\n{\"type\":\"never\"}\n";
        let (code, out) = run(script, &c, &[USER], true);
        assert_eq!(code, 0, "stdin closed at the second wait: exits 0");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["result"], r#"Is "Priya" in? in C:\Users\imogen\Projects\example"#);
        assert_eq!(out[0]["session_id"], "5c6d7e8f-9a0b-4c1d-8e2f-3a4b5c6d7e8f");
        assert_eq!(ctx(&["--resume", "abc"]).session_id, "abc");
        assert_eq!(ctx(&["-p"]).session_id.len(), 36, "a new id when given none");
    }

    #[test]
    fn an_interrupt_is_answered_and_ends_the_turn() {
        let c = ctx(&["--session-id", "s1"]);
        let script = "{\"_wait_user\":true}\n{\"type\":\"system\",\"subtype\":\"init\"}\n{\"_sleep_ms\":30000}\n{\"type\":\"result\",\"result\":\"never\"}\n{\"_wait_user\":true}\n{\"type\":\"result\",\"result\":\"second: {{user_text}}\"}\n";
        let interrupt = r#"{"type":"control_request","request_id":"req_9","request":{"subtype":"interrupt"}}"#;
        let other = r#"{"type":"control_request","request_id":"req_10","request":{"subtype":"set_model","model":"x"}}"#;
        let second = r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"again"}]}}"#;
        let started = Instant::now();
        let (code, out) = run_timed(script, &c, vec![(0, USER), (300, interrupt), (0, other), (0, second)]);
        assert!(started.elapsed() < Duration::from_secs(10), "the sleep was cut short");
        assert_eq!(code, 0);
        let types: Vec<&str> = out.iter().map(|v| v["type"].as_str().unwrap()).collect();
        assert_eq!(types, ["system", "control_response", "result", "control_response", "result"]);
        assert_eq!(out[1]["response"], json!({"subtype":"success","request_id":"req_9","response":{"still_queued":[]}}));
        assert_eq!(out[2]["terminal_reason"], "aborted_streaming");
        assert_eq!(out[2]["session_id"], "s1");
        assert_eq!(out[3]["response"]["subtype"], "error");
        assert_eq!(out[4]["result"], "second: again");
    }

    #[test]
    fn get_usage_is_answered_with_the_scripts_usage_else_refused() {
        let c = ctx(&["-p", "--tools", ""]);
        let ask = |id: &str| format!(r#"{{"type":"control_request","request_id":"{id}","request":{{"subtype":"get_usage","skip_behaviors":true}}}}"#);
        let (a, b, d) = (ask("u1"), ask("u2"), ask("u3"));
        // Refused until the script sets an answer; that answer after; refused again after null.
        let script = "{\"_wait_user\":true}\n{\"_usage\":{\"rate_limits_available\":true,\"rate_limits\":{\"five_hour\":{\"utilization\":12}}}}\n{\"_wait_user\":true}\n{\"_usage\":null}\n{\"_wait_user\":true}\n";
        let (code, out) = run_timed(script, &c, vec![(0, a.as_str()), (0, USER), (200, b.as_str()), (0, USER), (200, d.as_str())]);
        assert_eq!(code, 0);
        let answers: Vec<(&str, &str)> = out.iter().map(|v| (v["response"]["request_id"].as_str().unwrap(), v["response"]["subtype"].as_str().unwrap())).collect();
        assert_eq!(answers, [("u1", "error"), ("u2", "success"), ("u3", "error")]);
        assert_eq!(out[1]["response"]["response"]["rate_limits"]["five_hour"]["utilization"], 12);
        let Some(crate::ai::claude_code::events::Event::ControlResponse(r)) = crate::ai::claude_code::events::parse_line(&out[1].to_string()) else { panic!() };
        let crate::ai::claude_code::usage::Asked::Answered(Some(u)) = crate::ai::claude_code::usage::from_response(&r) else { panic!() };
        assert_eq!(u.limits[0].title, "Current session");
    }

    #[test]
    fn one_script_plays_a_chat_its_agent_and_a_question_each_its_own_part() {
        let script = "{\"type\":\"result\",\"result\":\"every kind\"}\n{\"_for\":\"chat\"}\n{\"type\":\"result\",\"result\":\"chat\"}\n{\"_for\":\"agent\"}\n{\"type\":\"result\",\"result\":\"agent\"}\n{\"_for\":\"ask\"}\n{\"type\":\"result\",\"result\":\"ask\"}\n{\"_for\":\"all\"}\n{\"type\":\"result\",\"result\":\"all again\"}\n";
        let said = |args: &[&str]| -> Vec<String> {
            let c = ctx(args);
            run(script, &c, &[], true).1.iter().map(|v| v["result"].as_str().unwrap().to_string()).collect()
        };
        assert_eq!(said(&["-p", "--mcp-config", "{\"mcpServers\":{}}", "--strict-mcp-config"]), ["every kind", "chat", "all again"]);
        assert_eq!(said(&["-p", "--mcp-config", "{\"mcpServers\":{}}", "--permission-prompt-tool", "mcp__yap__approve"]), ["every kind", "agent", "all again"]);
        assert_eq!(said(&["-p", "--tools", "", "--strict-mcp-config"]), ["every kind", "ask", "all again"]);
    }

    #[test]
    fn it_reports_args_env_and_exit_codes() {
        let mut c = ctx(&["-p", "--model", "claude-haiku-5-5"]);
        c.vars = Some(HashMap::from([("YAP_RUN_KEY".to_string(), "k".to_string()), ("EMPTY".to_string(), String::new())]));
        let script = "{\"_dump_args\":true}\n{\"_dump_env\":[\"YAP_RUN_KEY\",\"ANTHROPIC_API_KEY\",\"EMPTY\"]}\n{\"_exit\":3}\n{\"type\":\"never\"}\n";
        let (code, out) = run(script, &c, &[], false);
        assert_eq!(code, 3);
        assert_eq!(out[0]["args"], json!(["-p", "--model", "claude-haiku-5-5"]));
        assert_eq!(out[1]["present"], json!({"YAP_RUN_KEY":true,"ANTHROPIC_API_KEY":false,"EMPTY":false}));
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn it_keeps_a_transcript_as_claude_code_does_only_when_asked() {
        let config = std::env::temp_dir().join(format!("yap-cc-fake-transcript-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&config);
        let vars = HashMap::from([("YAP_FAKE_CLAUDE_TRANSCRIPTS".to_string(), "1".to_string()), ("CLAUDE_CONFIG_DIR".to_string(), config.display().to_string())]);
        let script = "{\"_wait_user\":true}\n{\"type\":\"result\",\"result\":\"ok\"}\n{\"_wait_user\":true}\n";
        let mut c = ctx(&["-p", "--session-id", "4d7c1d2e-8a51-4c3b-9f0e-2b6a7e1c5d90"]);
        c.vars = Some(vars.clone());
        let path = c.transcript().unwrap();
        assert_eq!(path, config.join("projects").join("C--Users-imogen-Projects-example").join("4d7c1d2e-8a51-4c3b-9f0e-2b6a7e1c5d90.jsonl"));
        run(script, &c, &[USER, USER], true);
        let kept: Vec<Value> = std::fs::read_to_string(&path).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
        assert_eq!(kept.len(), 2, "each user message");
        assert_eq!(kept[0]["message"]["content"], "Is \"Priya\" in?");
        assert_eq!(kept[0]["sessionId"], "4d7c1d2e-8a51-4c3b-9f0e-2b6a7e1c5d90");
        // Not with --no-session-persistence, nor without both variables.
        let mut once = ctx(&["-p", "--no-session-persistence"]);
        once.vars = Some(vars);
        assert_eq!(once.transcript(), None);
        let mut unasked = ctx(&["-p"]);
        unasked.vars = Some(HashMap::from([("CLAUDE_CONFIG_DIR".to_string(), config.display().to_string())]));
        assert_eq!(unasked.transcript(), None);
        unasked.vars = Some(HashMap::from([("YAP_FAKE_CLAUDE_TRANSCRIPTS".to_string(), "1".to_string())]));
        assert_eq!(unasked.transcript(), None, "never the person's own ~/.claude");
        let _ = std::fs::remove_dir_all(&config);
    }

    #[test]
    fn variables_expand_as_claude_code_does() {
        let vars = HashMap::from([("KEY".to_string(), "abc".to_string())]);
        let get = |n: &str| vars.get(n).cloned();
        assert_eq!(expand("Bearer ${KEY}", get, true), "Bearer abc");
        assert_eq!(expand("Bearer $KEY!", get, false), "Bearer abc!");
        assert_eq!(expand("${MISSING:-fallback}", get, true), "fallback");
        assert_eq!(expand("${MISSING:-fallback}", get, false), "");
        assert_eq!(expand("no vars", get, true), "no vars");
        assert!(matcher_fits("Edit|Write", "Write"));
        assert!(!matcher_fits("Edit|Write", "WriteMore"));
        assert!(matcher_fits("", "Bash") && matcher_fits("*", "Bash"));
        assert!(!matcher_fits("Edit", "Bash"));
        assert!(!matcher_fits("(", "Bash"), "a bad pattern fits nothing");
        let sse = b"event: message\ndata: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\"}\n\nevent: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"ok\":true}}\n\n";
        assert_eq!(sse_response(sse, 2).unwrap()["result"]["ok"], true);
        assert_eq!(sse_response(sse, 1), None);
        assert_eq!(sse_response(b"data: {\"id\":2", 2), None, "not all there yet");
    }

    /// A stand-in for Yap's run endpoint: an http hook and an MCP server
    /// (JSON for initialize, SSE for the tool call), keyed.
    /// What the endpoint was sent: (path, Authorization, Mcp-Session-Id, body).
    type Seen = Arc<Mutex<Vec<(String, String, String, String)>>>;

    fn endpoint() -> (String, Seen) {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let seen: Seen = Arc::default();
        let log = seen.clone();
        std::thread::spawn(move || {
            for mut req in server.incoming_requests() {
                let mut body = String::new();
                let _ = req.as_reader().read_to_string(&mut body);
                let header = |name: &str| {
                    req.headers().iter().find(|h| h.field.to_string().eq_ignore_ascii_case(name)).map(|h| h.value.to_string()).unwrap_or_default()
                };
                let auth = header("Authorization");
                let session = header("Mcp-Session-Id");
                log.lock().unwrap().push((req.url().to_string(), auth.clone(), session.clone(), body.clone()));
                let ok = auth == "Bearer run-key-1";
                let msg: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
                let (status, ctype, text, sid) = if !ok {
                    (401, "application/json", "{}".to_string(), None)
                } else if req.url().starts_with("/hook") {
                    (200, "application/json", r#"{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}}"#.to_string(), None)
                } else if msg["method"] == "initialize" {
                    let r = json!({"jsonrpc":"2.0","id":msg["id"],"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"yap","version":"1"}}});
                    (200, "application/json", r.to_string(), Some("mcp-session-7"))
                } else if msg.get("id").is_none() {
                    (202, "application/json", String::new(), None)
                } else {
                    let r = json!({"jsonrpc":"2.0","id":msg["id"],"result":{"content":[{"type":"text","text":"{\"behavior\":\"allow\"}"}]}});
                    (200, "text/event-stream", format!("event: message\ndata: {r}\n\n"), None)
                };
                // `from_data`: no Content-Type of its own beside ours.
                let mut resp = tiny_http::Response::from_data(text.into_bytes())
                    .with_status_code(status)
                    .with_header(tiny_http::Header::from_bytes("Content-Type", ctype).unwrap());
                if let Some(sid) = sid {
                    resp = resp.with_header(tiny_http::Header::from_bytes("Mcp-Session-Id", sid).unwrap());
                }
                let _ = req.respond(resp);
            }
        });
        (format!("http://127.0.0.1:{port}"), seen)
    }

    #[test]
    fn hooks_and_the_approve_tool_reach_the_run_endpoint() {
        let (base, seen) = endpoint();
        let settings = json!({"hooks":{"PreToolUse":[
            {"matcher":"Edit|Write","hooks":[{"type":"http","url":format!("{base}/hook/backup"),"headers":{"Authorization":"Bearer $YAP_RUN_KEY"},"allowedEnvVars":["YAP_RUN_KEY"]}]},
            {"matcher":"Bash","hooks":[{"type":"http","url":format!("{base}/hook/never")}]}
        ]}});
        let mcp = json!({"mcpServers":{"yap":{"type":"http","url":format!("{base}/mcp/run1"),"headers":{"Authorization":"Bearer ${YAP_RUN_KEY}"}}}});
        let dir = std::env::temp_dir().join(format!("yap-cc-fake-{}", &super::super::session::new_session_id()[..8]));
        std::fs::create_dir_all(&dir).unwrap();
        let mcp_file = dir.join("mcp.json");
        std::fs::write(&mcp_file, mcp.to_string()).unwrap();
        let args = vec!["-p".to_string(), "--settings".into(), settings.to_string(), "--mcp-config".into(), mcp_file.display().to_string(), "--strict-mcp-config".into(), "--session-id".into(), "s-42".into()];
        let mut c = Ctx::from_args(&args).unwrap();
        assert_eq!(c.mcp_servers.len(), 1);
        c.vars = Some(HashMap::from([("YAP_RUN_KEY".to_string(), "run-key-1".to_string())]));
        let script = r#"{"_hook":{"event":"PreToolUse","input":{"tool_name":"Edit","tool_input":{"file_path":"C:\\Users\\imogen\\notes.md"}}}}
{"_hook":{"event":"PostToolUse","input":{"tool_name":"Edit"}}}
{"_mcp":{"tool":"approve","arguments":{"tool_name":"Bash","input":{"command":"ls"}}}}
{"_mcp":{"tool":"approve","server":"missing"}}
{"_mcp":{"tool":"approve","server":"missing","result_for":"toolu_9"}}
"#;
        let (code, out) = run(script, &c, &[], false);
        assert_eq!(code, 0);
        assert_eq!(out.len(), 5, "one hook fitted; three MCP lines, the last with its tool result");
        assert_eq!(out[0]["subtype"], "hook_response");
        assert_eq!(out[0]["outcome"], "success");
        assert!(out[0]["output"].as_str().unwrap().contains("permissionDecision"));
        assert_eq!(out[1]["subtype"], "fake_mcp");
        assert_eq!(out[1]["server"], "yap");
        assert_eq!(out[1]["response"]["result"]["content"][0]["text"], "{\"behavior\":\"allow\"}");
        assert_eq!(out[2]["error"], "no such MCP server in --mcp-config");
        // A failed call's tool result, as the model would get it.
        assert_eq!(out[4]["type"], "user");
        let block = &out[4]["message"]["content"][0];
        assert_eq!((block["tool_use_id"].as_str(), block["is_error"].as_bool()), (Some("toolu_9"), Some(true)));
        assert_eq!(block["content"][0]["text"], "no such MCP server in --mcp-config");
        let seen = seen.lock().unwrap().clone();
        let urls: Vec<&str> = seen.iter().map(|s| s.0.as_str()).collect();
        assert_eq!(urls, ["/hook/backup", "/mcp/run1", "/mcp/run1", "/mcp/run1"]);
        assert!(seen.iter().all(|s| s.1 == "Bearer run-key-1"), "the key from the environment, never the config");
        let hook: Value = serde_json::from_str(&seen[0].3).unwrap();
        assert_eq!((hook["session_id"].as_str(), hook["hook_event_name"].as_str()), (Some("s-42"), Some("PreToolUse")));
        assert_eq!(hook["tool_input"]["file_path"], r"C:\Users\imogen\notes.md");
        assert_eq!(seen[1].2, "", "initialize has no session yet");
        assert_eq!((seen[2].2.as_str(), seen[3].2.as_str()), ("mcp-session-7", "mcp-session-7"));
        let call: Value = serde_json::from_str(&seen[3].3).unwrap();
        assert_eq!(call["method"], "tools/call");
        assert_eq!(call["params"]["arguments"]["input"]["command"], "ls");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_mcp_results_pictures_reach_the_model_as_image_blocks_and_a_dump_never_has_their_data() {
        let content = json!([
            { "type": "text", "text": "{\"success\":true}" },
            { "type": "text", "text": "Monitor 1." },
            { "type": "image", "data": "iVBORw0KGgo=", "mimeType": "image/png" },
        ]);
        assert_eq!(
            tool_result_content(&content),
            [json!({ "type": "text", "text": "{\"success\":true}Monitor 1." }), json!({ "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "iVBORw0KGgo=" } })]
        );
        // Text only: one block, as before.
        assert_eq!(tool_result_content(&json!([{ "type": "text", "text": "a" }, { "type": "text", "text": "b" }])), [json!({ "type": "text", "text": "ab" })]);
        let dump = dumped(&json!({ "tool": "look_at_screen", "response": { "result": { "content": content, "isError": false } }, "error": null }));
        assert_eq!(dump["tool"], "look_at_screen");
        assert_eq!(dump["content"][2], json!({ "type": "image", "mimeType": "image/png", "bytes": 8 }));
        assert!(!dump.to_string().contains("iVBOR"));
    }

    #[test]
    fn every_recording_replays() {
        for (version, name, _) in super::super::events::tests::fixtures() {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ai/claude_code/fixtures").join(&version).join(&name);
            let script = std::fs::read_to_string(&path).unwrap();
            let c = ctx(&["--session-id", "s-replay"]);
            let (code, out) = run(&script, &c, &[USER, USER, USER], true);
            assert_eq!(code, 0, "{name}");
            assert!(out.iter().any(|v| v["type"] == "result"), "{name}");
            assert!(out.iter().filter(|v| v.get("session_id").is_some()).all(|v| v["session_id"] == "s-replay"), "{name}");
        }
        for entry in std::fs::read_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ai/claude_code/fixtures/fake")).unwrap().flatten() {
            let script = std::fs::read_to_string(entry.path()).unwrap();
            for line in script.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with("//")) {
                serde_json::from_str::<Value>(line).unwrap_or_else(|e| panic!("{}: {e}", entry.path().display()));
            }
        }
    }
}

//! **A workspace's dev servers** (14 Nov 2026; the maintainer, on the Claude
//! Code app's Dev servers menu in its browser pane: "Can we add this to Yap,
//! please? And an ability to do this"). A Code chat's workspace declares
//! them in **`.claude/launch.json`** — the same file, in the same format, as
//! Claude Code's desktop app reads (its public docs, "Configure preview
//! servers"), so one file serves both:
//!
//! ```json
//! { "version": "0.0.1",
//!   "configurations": [ { "name": "my-app", "runtimeExecutable": "npm",
//!                         "runtimeArgs": ["run", "dev"], "port": 3000 } ] }
//! ```
//!
//! - **Fields**: `name`; `runtimeExecutable` and `runtimeArgs` (the
//!   command), or `program` and `args` (a script run with `node`); `port`
//!   (3000 by default); `cwd` (from the workspace's root); `env`;
//!   `autoPort`; `url` (where the page opens instead of
//!   `http://localhost:<port>` — alone, no command: a server already
//!   running, attached to). Comments and trailing commas are allowed
//!   ([`strip_jsonc`]).
//! - **A port in use** ([`port_plan`]): `autoPort: true`, the next free one
//!   (`PORT` set, a `--port` / `-p` in the arguments rewritten);
//!   `false`, an error; not set, the next free one when the arguments say
//!   the port or the command is no package manager's script, else it stays
//!   stopped and says why (a script whose port Yap can't move).
//! - **Started** ([`start`]): in the workspace, no window, `PORT` and
//!   `BROWSER=none` set (a dev server never opens the person's own
//!   browser), its output kept ([`LOG_KEEP`] lines, in memory), tied to
//!   Yap's kill-on-close job (`shell/procs.rs`: it never outlives Yap);
//!   ready once its port answers ([`READY_WAIT`] at most), failed when it
//!   exits first. Started twice: the running one ([`start`] reuses it).
//! - **Stopped** ([`stop`]): the process and those it started.
//! - **Its pages** ([`running_origin`]): `http://localhost:<port>` (or
//!   `127.0.0.1`, `[::1]`, a `*.localhost` name) of a server running now is
//!   the one exception to the browser's "never this PC" rule
//!   (`chat/pages/view.rs`, `pages::checked`) — never another port of this
//!   PC.
//!
//! **Who starts one**: the person, from the Dev servers menu in the browser
//! panel of a Code chat (`ServersMenu.svelte`), and the chat's own Claude
//! with `preview_start` (`chat/pages/preview.rs`: asked first on a card
//! outside Bypass permissions — it runs a command). Only in a workspace Yap
//! knows (`workspaces::list`).
//!
//! **Privacy**: the log has names, ports and counts, never a command line,
//! an address's path or a line of output.

use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};

/// Where a workspace keeps them.
pub const LAUNCH: &str = ".claude/launch.json";
/// A server's port when its entry gives none (Claude Code's own default).
pub const DEFAULT_PORT: u16 = 3000;
/// Output lines kept per server (the newest).
pub const LOG_KEEP: usize = 1000;
/// How long a server may take to answer on its port.
pub const READY_WAIT: Duration = Duration::from_secs(90);
/// A line of output, at most.
const LINE_MAX: usize = 2_000;
/// The event the Dev servers menu hears.
pub const EVENT: &str = "yap-dev-servers";

// ---- launch.json ----------------------------------------------------------------------------

/// One entry of `launch.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub name: String,
    /// The command (`None`: attach to `url`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    pub args: Vec<String>,
    pub port: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_port: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl Config {
    /// Its command line as the person reads it (`npm run dev`).
    pub fn line(&self) -> String {
        match &self.command {
            Some(c) => std::iter::once(c.as_str()).chain(self.args.iter().map(String::as_str)).map(quoted).collect::<Vec<_>>().join(" "),
            None => format!("(attach to {})", self.url.as_deref().unwrap_or("")),
        }
    }

    /// Where its page opens.
    pub fn page(&self, port: u16) -> String {
        match &self.url {
            Some(u) if port == self.port => u.clone(),
            Some(u) => reqwest::Url::parse(u)
                .ok()
                .and_then(|mut x| x.set_port(Some(port)).ok().map(|_| x.to_string()))
                .unwrap_or_else(|| u.clone()),
            None => format!("http://localhost:{port}/"),
        }
    }
}

fn quoted(s: &str) -> String {
    if s.is_empty() || s.chars().any(char::is_whitespace) {
        format!("\"{s}\"")
    } else {
        s.to_string()
    }
}

/// A workspace's `launch.json`, read: its servers, the entries it couldn't
/// use (and why), whether the file is there.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Launch {
    pub configs: Vec<Config>,
    pub skipped: Vec<String>,
    pub exists: bool,
    /// The file has comments: Yap adds to it only through Claude (it would drop them).
    pub has_comments: bool,
}

/// **JSON with comments**, as plain JSON: `//` and `/* */` comments and a
/// comma before `}` or `]` gone, strings left as they are. Pure.
pub fn strip_jsonc(s: &str) -> String {
    let b: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            out.push(c);
            if c == '\\' && i + 1 < b.len() {
                out.push(b[i + 1]);
                i += 2;
                continue;
            }
            if c == '"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push(c);
                i += 1;
            }
            '/' if b.get(i + 1) == Some(&'/') => {
                while i < b.len() && b[i] != '\n' {
                    i += 1;
                }
            }
            '/' if b.get(i + 1) == Some(&'*') => {
                i += 2;
                while i + 1 < b.len() && !(b[i] == '*' && b[i + 1] == '/') {
                    i += 1;
                }
                i += 2;
            }
            ',' => {
                // A trailing comma: the next thing that isn't white space closes.
                let mut j = i + 1;
                while j < b.len() && b[j].is_whitespace() {
                    j += 1;
                }
                if !matches!(b.get(j), Some('}') | Some(']')) {
                    out.push(c);
                }
                i += 1;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// Whether `s` has a comment outside its strings. Pure.
fn has_comments(s: &str) -> bool {
    let b: Vec<char> = s.chars().collect();
    let mut in_str = false;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            '\\' if in_str => i += 1,
            '"' => in_str = !in_str,
            '/' if !in_str && matches!(b.get(i + 1), Some('/') | Some('*')) => return true,
            _ => {}
        }
        i += 1;
    }
    false
}

/// Is `host` this PC by name or address (`localhost`, `*.localhost`,
/// `127.x`, `::1`)? Pure.
pub fn is_loopback_host(host: &str) -> bool {
    let h = host.trim_matches(|c| c == '[' || c == ']').trim_end_matches('.').to_ascii_lowercase();
    h == "localhost" || h.ends_with(".localhost") || h.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// **`launch.json`'s text, read**: each usable entry, the rest said why.
/// Pure.
pub fn parse(text: &str) -> Result<Launch, String> {
    let v: Value = serde_json::from_str(&strip_jsonc(text)).map_err(|e| format!("launch.json isn't valid JSON ({e})."))?;
    let list = v["configurations"].as_array().ok_or_else(|| "launch.json has no \"configurations\" list.".to_string())?;
    let mut out = Launch { exists: true, has_comments: has_comments(text), ..Default::default() };
    for (i, c) in list.iter().enumerate() {
        match config_of(c) {
            Ok(cfg) if out.configs.iter().any(|x| x.name == cfg.name) => out.skipped.push(format!("“{}”: the name is used twice (the first one counts)", cfg.name)),
            Ok(cfg) => out.configs.push(cfg),
            Err(why) => out.skipped.push(format!("entry {}: {why}", i + 1)),
        }
    }
    Ok(out)
}

fn config_of(c: &Value) -> Result<Config, String> {
    let name = c["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).ok_or("it has no \"name\"")?.chars().take(80).collect::<String>();
    let strings = |k: &str| -> Result<Vec<String>, String> {
        match &c[k] {
            Value::Null => Ok(Vec::new()),
            Value::Array(a) => a.iter().map(|x| x.as_str().map(str::to_string).ok_or(format!("\"{k}\" must be a list of strings"))).collect(),
            _ => Err(format!("\"{k}\" must be a list of strings")),
        }
    };
    let (command, args) = match (c["runtimeExecutable"].as_str().map(str::trim).filter(|s| !s.is_empty()), c["program"].as_str().map(str::trim).filter(|s| !s.is_empty())) {
        (Some(exe), program) => {
            let mut a = strings("runtimeArgs")?;
            a.extend(program.map(str::to_string));
            a.extend(strings("args")?);
            (Some(exe.to_string()), a)
        }
        (None, Some(program)) => {
            let mut a = vec![program.to_string()];
            a.extend(strings("args")?);
            (Some("node".to_string()), a)
        }
        (None, None) => (None, Vec::new()),
    };
    let url = c["url"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    if command.is_none() && url.is_none() {
        return Err("it needs \"runtimeExecutable\" (with \"runtimeArgs\"), \"program\", or a \"url\" to attach to".into());
    }
    let mut port = match &c["port"] {
        Value::Null => None,
        p => Some(p.as_u64().filter(|p| (1..=65535).contains(p)).ok_or("\"port\" must be a number from 1 to 65535")? as u16),
    };
    if let Some(u) = &url {
        let parsed = reqwest::Url::parse(u).map_err(|_| "\"url\" isn't a web address")?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err("\"url\" must be http or https".into());
        }
        if parsed.host_str().is_some_and(is_loopback_host) {
            // A localhost url is the server's origin alone, its port the entry's.
            if parsed.path() != "/" || parsed.query().is_some() || parsed.fragment().is_some() {
                return Err("a localhost \"url\" must be the server's origin alone (no path or query)".into());
            }
            let up = parsed.port_or_known_default().unwrap_or(80);
            match port {
                Some(p) if p != up => return Err(format!("its \"url\" is on port {up} but its \"port\" is {p}")),
                None => port = Some(up),
                _ => {}
            }
        } else if command.is_some() {
            return Err("a \"url\" that isn't on this PC can only be attached to (no command)".into());
        }
    }
    let env = match &c["env"] {
        Value::Null => BTreeMap::new(),
        Value::Object(o) => o.iter().map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())).ok_or(format!("\"env\" value {k} must be a string"))).collect::<Result<_, _>>()?,
        _ => return Err("\"env\" must be an object of strings".into()),
    };
    let auto_port = match &c["autoPort"] {
        Value::Null => None,
        Value::Bool(b) => Some(*b),
        _ => return Err("\"autoPort\" must be true or false".into()),
    };
    let cwd = c["cwd"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    Ok(Config { name, command, args, port: port.unwrap_or(DEFAULT_PORT), cwd, env, auto_port, url })
}

/// **A workspace's `launch.json`** (none there: no servers, `exists` false).
pub fn read(workspace: &Path) -> Result<Launch, String> {
    let file = workspace.join(LAUNCH);
    match std::fs::read_to_string(&file) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Launch::default()),
        Err(e) => Err(format!("Yap couldn't read .claude/launch.json ({e}).")),
    }
}

/// **A server added to a workspace's `launch.json`** (the Dev servers
/// menu's Add server): made when there's none; refused when the file has
/// comments (rewriting it would drop them: Claude adds it instead) or the
/// name is taken.
pub fn add(workspace: &Path, name: &str, command: &str, args: &[String], port: u16) -> Result<Config, String> {
    let name = name.trim();
    let command = command.trim();
    if name.is_empty() || command.is_empty() {
        return Err("A server needs a name and a command.".into());
    }
    if port == 0 {
        return Err("A server needs its port.".into());
    }
    let file = workspace.join(LAUNCH);
    let entry = json!({ "name": name, "runtimeExecutable": command, "runtimeArgs": args, "port": port });
    let mut doc = match std::fs::read_to_string(&file) {
        Ok(text) => {
            let launch = parse(&text)?;
            if launch.has_comments {
                return Err("launch.json has comments that rewriting it would lose: ask Claude in the chat to add the server.".into());
            }
            if launch.configs.iter().any(|c| c.name.eq_ignore_ascii_case(name)) {
                return Err(format!("There's already a server called “{name}”."));
            }
            serde_json::from_str::<Value>(&strip_jsonc(&text)).map_err(|e| e.to_string())?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => json!({ "version": "0.0.1", "configurations": [] }),
        Err(e) => return Err(format!("Yap couldn't read .claude/launch.json ({e}).")),
    };
    doc["configurations"].as_array_mut().ok_or("launch.json has no \"configurations\" list.")?.push(entry.clone());
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Yap couldn't make the .claude folder ({e})."))?;
    }
    let text = serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())? + "\n";
    std::fs::write(&file, text).map_err(|e| format!("Yap couldn't write .claude/launch.json ({e})."))?;
    tracing::info!(port, "Dev servers: one added to a workspace's launch.json");
    config_of(&entry)
}

// ---- ports ------------------------------------------------------------------------------------

/// Is something on this PC listening on `port` (IPv4 or IPv6 loopback)?
pub fn in_use(port: u16) -> bool {
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
    let t = Duration::from_millis(300);
    TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, port)), t).is_ok() || TcpStream::connect_timeout(&SocketAddr::from((Ipv6Addr::LOCALHOST, port)), t).is_ok()
}

/// The first port after `from` nothing listens on (and Yap could bind).
pub fn free_port(from: u16) -> Option<u16> {
    (from.saturating_add(1)..=from.saturating_add(200)).find(|p| !in_use(*p) && std::net::TcpListener::bind(("127.0.0.1", *p)).is_ok())
}

/// A `--port N` / `-p N` / `--port=N` / `-p=N` in `args` set to `port`:
/// whether there was one. Pure.
pub fn rewrite_port(args: &mut [String], port: u16) -> bool {
    let mut found = false;
    for i in 0..args.len() {
        let a = args[i].clone();
        if (a == "--port" || a == "-p") && args.get(i + 1).is_some_and(|n| n.parse::<u16>().is_ok()) {
            args[i + 1] = port.to_string();
            found = true;
        } else if let Some(flag) = ["--port=", "-p="].iter().find(|f| a.starts_with(**f) && a[f.len()..].parse::<u16>().is_ok()) {
            args[i] = format!("{flag}{port}");
            found = true;
        }
    }
    found
}

/// **Where a server goes when its port is taken** (`busy`): its port and
/// arguments, or why it stays stopped. Pure.
pub fn port_plan(cfg: &Config, busy: bool, next_free: Option<u16>) -> Result<(u16, Vec<String>), String> {
    let mut args = cfg.args.clone();
    if !busy {
        return Ok((cfg.port, args));
    }
    if cfg.auto_port == Some(false) {
        return Err(format!("Port {} is in use (and \"autoPort\" is false): stop what's on it, or change its \"port\".", cfg.port));
    }
    let Some(free) = next_free else { return Err(format!("Port {} is in use and Yap found no free port after it.", cfg.port)) };
    let moved = rewrite_port(&mut args, free);
    let script = cfg.command.as_deref().map(|c| {
        let c = c.rsplit(['/', '\\']).next().unwrap_or(c).to_ascii_lowercase();
        let c = c.trim_end_matches(".cmd").trim_end_matches(".exe");
        matches!(c, "npm" | "yarn" | "pnpm" | "bun")
    });
    if !moved && cfg.auto_port.is_none() && script == Some(true) {
        return Err(format!(
            "Port {} is in use, and its script's port can't be moved from here: stop what's on it, pass the port in \"runtimeArgs\" (--port), set \"autoPort\": true if the script reads PORT, or change \"port\".",
            cfg.port
        ));
    }
    Ok((free, args))
}

// ---- running servers ----------------------------------------------------------------------------

/// A server's state, as the menu and Claude see it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub id: String,
    pub name: String,
    pub workspace: String,
    pub port: u16,
    pub url: String,
    /// starting, ready, stopped or failed.
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
    /// Attached to one already running (not Yap's to stop).
    pub attached: bool,
    /// Its port moved from the entry's (`autoPort`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moved_from: Option<u16>,
}

struct Server {
    info: Info,
    pid: Option<u32>,
    #[cfg_attr(not(windows), allow(dead_code))]
    since: u64,
    logs: VecDeque<(bool, String)>,
    stopping: bool,
    at: Instant,
}

static SERVERS: Mutex<Vec<Server>> = Mutex::new(Vec::new());
static NEXT: AtomicU32 = AtomicU32::new(1);

fn servers() -> std::sync::MutexGuard<'static, Vec<Server>> {
    SERVERS.lock().unwrap_or_else(|e| e.into_inner())
}

/// Every server Yap knows this run (a stopped or failed one kept, to read
/// its output), newest last.
pub fn list() -> Vec<Info> {
    servers().iter().map(|s| s.info.clone()).collect()
}

/// [`list`] for one workspace.
pub fn list_in(workspace: &Path) -> Vec<Info> {
    let w = norm(workspace);
    servers().iter().filter(|s| s.info.workspace == w).map(|s| s.info.clone()).collect()
}

/// One server by id.
pub fn get(id: &str) -> Option<Info> {
    servers().iter().find(|s| s.info.id == id).map(|s| s.info.clone())
}

fn norm(p: &Path) -> String {
    crate::agents::workspaces::norm(&p.to_string_lossy())
}

fn tell() {
    if let Some(app) = crate::memory::app() {
        use tauri::Emitter;
        let _ = app.emit(EVENT, list());
    }
}

/// **Is `url` a page of a dev server running now** (its loopback origin, or
/// an attached one's), the one exception to "never this PC"? Not just
/// any port of this PC: only one Yap started or attached to, while it runs.
pub fn running_origin(url: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(url.trim()) else { return false };
    if !matches!(u.scheme(), "http" | "https" | "ws" | "wss") {
        return false;
    }
    let Some(host) = u.host_str() else { return false };
    if !is_loopback_host(host) {
        return false;
    }
    let port = u.port_or_known_default().unwrap_or(0);
    servers().iter().filter(|s| matches!(s.info.state.as_str(), "starting" | "ready")).any(|s| {
        s.info.port == port
            && reqwest::Url::parse(&s.info.url).ok().and_then(|x| x.host_str().map(str::to_ascii_lowercase)).is_some_and(|h| {
                let host = host.trim_matches(|c| c == '[' || c == ']').to_ascii_lowercase();
                // Its own name, or this PC's other names for it.
                h == host || (is_loopback_host(&h) && matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1"))
            })
    })
}

/// A workspace Yap knows (its Code chats' folders): the only places a
/// server starts.
pub fn known_workspace(path: &str) -> Option<PathBuf> {
    let p = crate::agents::workspaces::norm(path);
    crate::agents::workspaces::list().into_iter().find(|f| crate::agents::workspaces::norm(&f.path) == p).map(|f| PathBuf::from(f.path))
}

/// **Start** workspace `workspace`'s server `name` (running already: that
/// one), and wait until its port answers.
pub async fn start(workspace: &Path, name: &str) -> Result<Info, String> {
    let launch = read(workspace)?;
    let cfg = launch.configs.iter().find(|c| c.name == name).or_else(|| launch.configs.iter().find(|c| c.name.eq_ignore_ascii_case(name))).cloned().ok_or_else(|| {
        if !launch.exists {
            "This workspace has no .claude/launch.json yet: add a server to it first.".to_string()
        } else if launch.configs.is_empty() {
            format!("launch.json has no server Yap can use{}.", if launch.skipped.is_empty() { String::new() } else { format!(" ({})", launch.skipped.join("; ")) })
        } else {
            format!("There's no server called “{name}”: launch.json has {}.", launch.configs.iter().map(|c| format!("“{}”", c.name)).collect::<Vec<_>>().join(", "))
        }
    })?;
    let w = norm(workspace);
    // Running already: that one.
    let running = servers().iter().find(|s| s.info.workspace == w && s.info.name == cfg.name && matches!(s.info.state.as_str(), "starting" | "ready")).map(|s| s.info.id.clone());
    if let Some(id) = running {
        return wait_ready(&id).await;
    }
    let id = format!("srv-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    // Attached: one already running (no command).
    let Some(command) = cfg.command.clone() else {
        let url = cfg.url.clone().unwrap_or_default();
        let parsed = reqwest::Url::parse(&url).map_err(|_| "Its \"url\" isn't a web address.".to_string())?;
        let local = parsed.host_str().is_some_and(is_loopback_host);
        if local && !in_use(cfg.port) {
            return Err(format!("Nothing is running on port {} to attach to: start the server first.", cfg.port));
        }
        if !local {
            crate::web::check_url(&url)?;
        }
        let info = Info { id, name: cfg.name.clone(), workspace: w, port: cfg.port, url, state: "ready".into(), why: None, attached: true, moved_from: None };
        servers().push(Server { info: info.clone(), pid: None, since: 0, logs: VecDeque::new(), stopping: false, at: Instant::now() });
        tracing::info!(port = cfg.port, "Dev servers: attached to one already running");
        tell();
        return Ok(info);
    };
    let busy = in_use(cfg.port);
    let (port, args) = port_plan(&cfg, busy, if busy { free_port(cfg.port) } else { None })?;
    let dir = match &cfg.cwd {
        Some(c) => workspace.join(c),
        None => workspace.to_path_buf(),
    };
    if !dir.is_dir() {
        return Err(format!("Its folder ({}) isn't there: check \"cwd\" in .claude/launch.json.", cfg.cwd.as_deref().unwrap_or(".")));
    }
    let exe = resolve(&command, &dir).ok_or_else(|| format!("Yap couldn't find the command “{command}”: is it installed, and on PATH?"))?;
    let mut cmd = tokio::process::Command::new(&exe);
    cmd.args(&args)
        .current_dir(&dir)
        .envs(&cfg.env)
        .env("PORT", port.to_string())
        .env("BROWSER", "none")
        .env("FORCE_COLOR", "0")
        .env("NO_COLOR", "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(false);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    #[cfg(windows)]
    let since = crate::shell::procs::now_filetime();
    #[cfg(not(windows))]
    let since = 0u64;
    let mut child = cmd.spawn().map_err(|e| format!("Yap couldn't start “{command}” ({e})."))?;
    crate::shell::procs::end_with_yap_tokio(&child);
    let pid = child.id();
    let url = cfg.page(port);
    let info = Info { id: id.clone(), name: cfg.name.clone(), workspace: w, port, url, state: "starting".into(), why: None, attached: false, moved_from: (port != cfg.port).then_some(cfg.port) };
    servers().push(Server { info, pid, since, logs: VecDeque::new(), stopping: false, at: Instant::now() });
    tracing::info!(port, moved = port != cfg.port, "Dev servers: one started");
    tell();
    // Its output, kept.
    for (err, pipe) in [(false, child.stdout.take().map(|p| Box::new(p) as Box<dyn tokio::io::AsyncRead + Unpin + Send>)), (true, child.stderr.take().map(|p| Box::new(p) as Box<dyn tokio::io::AsyncRead + Unpin + Send>))] {
        let Some(pipe) = pipe else { continue };
        let id = id.clone();
        tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut lines = tokio::io::BufReader::new(pipe).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                record(&id, err, &line);
            }
        });
    }
    // Its end.
    let watch = id.clone();
    tokio::spawn(async move {
        let status = child.wait().await;
        let mut all = servers();
        if let Some(s) = all.iter_mut().find(|s| s.info.id == watch) {
            let code = status.ok().and_then(|s| s.code());
            if s.stopping {
                s.info.state = "stopped".into();
            } else {
                s.info.state = "failed".into();
                s.info.why = Some(match code {
                    Some(0) => "it ended by itself".to_string(),
                    Some(c) => format!("it ended with code {c}"),
                    None => "it was ended".to_string(),
                });
            }
            tracing::info!(code, stopped = s.stopping, "Dev servers: one ended");
        }
        drop(all);
        tell();
    });
    wait_ready(&id).await
}

/// Until server `id` answers on its port (ready), ends (failed: why, and
/// its last lines), or [`READY_WAIT`] passes.
async fn wait_ready(id: &str) -> Result<Info, String> {
    loop {
        let (info, at) = match servers().iter().find(|s| s.info.id == id) {
            Some(s) => (s.info.clone(), s.at),
            None => return Err("That server is gone.".into()),
        };
        match info.state.as_str() {
            "ready" => return Ok(info),
            "failed" | "stopped" => {
                let tail = logs(id, false, 12, None).unwrap_or_default().join("\n");
                return Err(format!("“{}” didn't start: {}.{}", info.name, info.why.unwrap_or_else(|| "it stopped".into()), if tail.is_empty() { String::new() } else { format!(" Its last output:\n{tail}") }));
            }
            _ => {}
        }
        let port = info.port;
        if tokio::task::spawn_blocking(move || in_use(port)).await.unwrap_or(false) {
            if let Some(s) = servers().iter_mut().find(|s| s.info.id == id) {
                s.info.state = "ready".into();
            }
            tell();
            continue;
        }
        if at.elapsed() > READY_WAIT {
            return Err(format!("“{}” started but nothing answered on port {port} in {} s: its output (preview_logs) says why.", info.name, READY_WAIT.as_secs()));
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

/// A line of server `id`'s output kept (colour codes gone, clipped).
fn record(id: &str, err: bool, line: &str) {
    let clean: String = strip_ansi(line).chars().take(LINE_MAX).collect();
    if let Some(s) = servers().iter_mut().find(|s| s.info.id == id) {
        if s.logs.len() >= LOG_KEEP {
            s.logs.pop_front();
        }
        s.logs.push_back((err, clean));
    }
}

/// A terminal's colour and cursor codes taken out. Pure.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\u{1b}' {
            if it.peek() == Some(&'[') {
                it.next();
                for d in it.by_ref() {
                    if ('@'..='~').contains(&d) {
                        break;
                    }
                }
            }
            continue;
        }
        if c != '\r' {
            out.push(c);
        }
    }
    out
}

/// **Server `id`'s output**: the newest `lines` (errors only: lines on
/// stderr, or that say error / failed / exception / fatal), those with
/// `search` in them.
pub fn logs(id: &str, errors: bool, lines: usize, search: Option<&str>) -> Result<Vec<String>, String> {
    let all = servers();
    let s = all.iter().find(|s| s.info.id == id).ok_or_else(|| format!("There's no server “{id}”: preview_list lists them."))?;
    let search = search.map(str::to_lowercase).filter(|s| !s.is_empty());
    let mut out: Vec<String> = s
        .logs
        .iter()
        .filter(|(err, l)| !errors || *err || ["error", "failed", "exception", "fatal"].iter().any(|w| l.to_lowercase().contains(w)))
        .filter(|(_, l)| search.as_ref().is_none_or(|q| l.to_lowercase().contains(q)))
        .map(|(_, l)| l.clone())
        .collect();
    let keep = lines.clamp(1, LOG_KEEP);
    if out.len() > keep {
        out.drain(..out.len() - keep);
    }
    Ok(out)
}

/// **Stop** server `id`: the process and those it started (an attached one
/// is only let go).
pub fn stop(id: &str) -> Result<Info, String> {
    let (pid, since, attached) = {
        let mut all = servers();
        let s = all.iter_mut().find(|s| s.info.id == id).ok_or_else(|| format!("There's no server “{id}”."))?;
        if !matches!(s.info.state.as_str(), "starting" | "ready") {
            return Ok(s.info.clone());
        }
        s.stopping = true;
        if s.info.attached {
            s.info.state = "stopped".into();
        }
        (s.pid, s.since, s.info.attached)
    };
    if !attached {
        if let Some(pid) = pid {
            kill_tree(pid, since);
        }
        // Ended now (its end, heard a moment later, says the same): the answer says stopped (Yap Lab, 14 Nov
        // 2026: preview_stop said "ready").
        if let Some(s) = servers().iter_mut().find(|s| s.info.id == id) {
            s.info.state = "stopped".into();
        }
    }
    tracing::info!(attached, "Dev servers: one stopped");
    tell();
    get(id).ok_or_else(|| "That server is gone.".to_string())
}

/// Every running one stopped (in `workspace`, or everywhere).
pub fn stop_all(workspace: Option<&Path>) -> usize {
    let w = workspace.map(norm);
    let ids: Vec<String> = servers()
        .iter()
        .filter(|s| matches!(s.info.state.as_str(), "starting" | "ready") && w.as_ref().is_none_or(|w| &s.info.workspace == w))
        .map(|s| s.info.id.clone())
        .collect();
    ids.iter().filter(|id| stop(id).is_ok()).count()
}

#[cfg(windows)]
fn kill_tree(pid: u32, since: u64) {
    // Children first, so none is left to be adopted.
    let mut family = crate::shell::procs::family(pid, since);
    family.reverse();
    for p in family {
        crate::shell::procs::kill(p);
    }
}

#[cfg(not(windows))]
fn kill_tree(pid: u32, _since: u64) {
    let _ = std::process::Command::new("kill").arg("-TERM").arg(format!("-{pid}")).status();
    let _ = std::process::Command::new("kill").arg("-TERM").arg(pid.to_string()).status();
}

/// **A command as Windows finds it**: a path as it is; a bare name in the
/// folder or on PATH, with each of PATHEXT's endings (`npm` is `npm.cmd`).
fn resolve(command: &str, dir: &Path) -> Option<PathBuf> {
    let given = Path::new(command);
    if given.components().count() > 1 || given.is_absolute() {
        let p = if given.is_absolute() { given.to_path_buf() } else { dir.join(given) };
        return Some(p);
    }
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into()).split(';').filter(|e| !e.is_empty()).map(|e| e.to_ascii_lowercase()).collect()
    } else {
        vec![String::new()]
    };
    let has_ext = given.extension().is_some();
    let dirs = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default();
    for d in dirs {
        if has_ext || !cfg!(windows) {
            let p = d.join(command);
            if p.is_file() {
                return Some(p);
            }
        }
        if !has_ext {
            for e in &exts {
                let p = d.join(format!("{command}{e}"));
                if p.is_file() {
                    return Some(p);
                }
            }
        }
    }
    None
}

// ---- the menu's commands ------------------------------------------------------------------------

/// A workspace's servers for the Dev servers menu: what `launch.json` says,
/// each with its server's state this run.
#[tauri::command]
pub fn dev_servers(workspace: String) -> Result<Value, String> {
    let w = known_workspace(&workspace).ok_or("That isn't one of Yap Code's workspaces.")?;
    let launch = read(&w)?;
    let running = list_in(&w);
    let configs: Vec<Value> = launch
        .configs
        .iter()
        .map(|c| {
            let server = running.iter().rev().find(|s| s.name == c.name);
            json!({ "name": c.name, "port": c.port, "line": c.line(), "attach": c.command.is_none(), "server": server })
        })
        .collect();
    Ok(json!({ "configs": configs, "skipped": launch.skipped, "exists": launch.exists, "hasComments": launch.has_comments }))
}

/// The menu's ▶: started (no card: the person's own click).
#[tauri::command]
pub async fn dev_server_start(workspace: String, name: String) -> Result<Info, String> {
    let w = known_workspace(&workspace).ok_or("That isn't one of Yap Code's workspaces.")?;
    start(&w, &name).await
}

/// The menu's ■.
#[tauri::command]
pub fn dev_server_stop(id: String) -> Result<Info, String> {
    stop(&id)
}

/// The menu's Stop all (in that workspace).
#[tauri::command]
pub fn dev_servers_stop_all(workspace: Option<String>) -> usize {
    let w = workspace.as_deref().and_then(known_workspace);
    stop_all(w.as_deref())
}

/// The menu's Add server.
#[tauri::command]
pub fn dev_server_add(workspace: String, name: String, command: String, args: Vec<String>, port: u16) -> Result<Config, String> {
    let w = known_workspace(&workspace).ok_or("That isn't one of Yap Code's workspaces.")?;
    let out = add(&w, &name, &command, &args, port)?;
    tell();
    Ok(out)
}

#[cfg(test)]
mod tests;

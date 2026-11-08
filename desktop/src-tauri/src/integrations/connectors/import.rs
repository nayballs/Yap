//! **Adding connectors from a setup that exists already** (Settings →
//! Connectors → + Add): **Import from Claude Desktop** (its
//! `claude_desktop_config.json`'s `mcpServers`, both builds' copies, read
//! only: never written) and **Paste a setup** (an `mcpServers` block, a
//! server's own JSON, a link, or an `npx …` / `uvx …` line).
//!
//! Two steps, as every program is added: [`scan`] reads the setup and shows
//! each server whole — a program's exact command line with its warnings and
//! fingerprint, a link's host — with no secret in it (variables and
//! headers by name only); then [`import`] reads it again and adds the ones
//! the person ticked, each program only with the fingerprint they saw. The
//! secrets (a variable's or a header's value, a bearer token) go from the
//! setup straight to the vault: the page never holds them. Yap's own entry
//! (`yap mcp`) is never offered.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::{preview, secret_name, Pair, PreviewView, Spec};

/// The servers a setup holds: each one's name in it, and what adding it takes.
pub type Servers = Vec<(String, Spec)>;

/// Where a setup comes from.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "from", rename_all = "kebab-case")]
pub enum Source {
    /// Claude Desktop's settings file(s) on this PC.
    ClaudeDesktop,
    /// Text the person pasted.
    Paste { text: String },
}

impl Source {
    /// The origin's word for it (`origin.from`).
    pub fn word(&self) -> &'static str {
        match self {
            Source::ClaudeDesktop => "claude-desktop",
            Source::Paste { .. } => "paste",
        }
    }
}

/// One server found in a setup, as the person sees it before it's added.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    /// Its name in the setup (what [`import`]'s picks name).
    pub key: String,
    pub name: String,
    /// "remote" | "local".
    pub kind: &'static str,
    /// A link, or the whole command line.
    pub address: String,
    /// What adding it would run or reach (`None` with a `problem`).
    pub preview: Option<PreviewView>,
    /// A program's variables: names, a plain one's value, which are secret.
    pub env: Vec<FoundVar>,
    /// A link's request headers, by name.
    pub headers: Vec<String>,
    /// A link sends a bearer token.
    pub token: bool,
    /// One of the person's connectors already reaches it.
    pub added: bool,
    /// Why it can't be added.
    pub problem: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundVar {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub secret: bool,
}

/// [`scan`]'s answer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanView {
    pub servers: Vec<Found>,
    /// Where it was read from (a file's folder for Claude Desktop), for the
    /// page's line.
    pub read_from: Vec<String>,
    /// Entries that aren't servers Yap can add (Yap's own, a server
    /// without a command or a link).
    pub skipped: usize,
}

/// One server the person ticked: its key, and a program's fingerprint as
/// they saw it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pick {
    pub key: String,
    #[serde(default)]
    pub approved: Option<String>,
}

/// Words split as a command line splits them: spaces, with "double" or
/// 'single' quotes keeping an argument together. Pure.
pub fn split_line(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut any = false;
    for ch in line.chars() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => cur.push(ch),
            None if ch == '"' || ch == '\'' => {
                quote = Some(ch);
                any = true;
            }
            None if ch.is_whitespace() => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                any = false;
            }
            None => cur.push(ch),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Yap's own entry (`yap mcp`, under any name): never offered. Pure.
fn is_yap(name: &str, command: &str, args: &[String]) -> bool {
    let stem = std::path::Path::new(command).file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let mcp = args.first().is_some_and(|a| a.eq_ignore_ascii_case(super::super::mcp_clients::SERVER_ARG));
    mcp && (stem.starts_with("yap") || stem.starts_with("kiku") || name.eq_ignore_ascii_case(super::super::mcp_clients::SERVER_NAME))
}

/// One server's JSON (Claude's, Cursor's, VS Code's shapes) as a spec, or
/// `None` when it isn't one Yap can add. Pure.
fn server_spec(name: &str, v: &Value) -> Option<Spec> {
    let o = v.as_object()?;
    let text = |k: &str| o.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let pairs = |k: &str| -> Vec<(String, String)> {
        o.get(k).and_then(Value::as_object).map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_string()))).collect()).unwrap_or_default()
    };
    if let Some(command) = text("command") {
        let args: Vec<String> = o.get("args").and_then(Value::as_array).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
        if is_yap(name, &command, &args) {
            return None;
        }
        let env = pairs("env").into_iter().map(|(name, value)| Pair { name, value, secret: None }).collect();
        return Some(Spec { name: name.to_string(), command: Some(command), args, env, ..Default::default() });
    }
    let url = text("url").or_else(|| text("serverUrl")).or_else(|| text("httpUrl"))?;
    let kind = text("type").or_else(|| text("transport")).unwrap_or_default().to_ascii_lowercase();
    let mut token = None;
    let mut headers = Vec::new();
    for (k, v) in pairs("headers") {
        match v.trim().strip_prefix("Bearer ").filter(|_| k.eq_ignore_ascii_case("authorization")) {
            Some(t) if token.is_none() => token = Some(t.trim().to_string()),
            _ => headers.push(Pair { name: k, value: v, secret: Some(true) }),
        }
    }
    Some(Spec { name: name.to_string(), url: Some(url), token, headers, sse: kind == "sse", ..Default::default() })
}

/// A name for a link or a program pasted on its own. Pure.
fn pasted_name(spec: &Spec) -> String {
    if let Some(u) = spec.url.as_deref().and_then(|u| url::Url::parse(u).ok()) {
        let host = u.host_str().unwrap_or("Pasted server");
        return host.trim_start_matches("mcp.").trim_start_matches("api.").split('.').next().unwrap_or(host).to_string();
    }
    // The package an npx / uvx line runs, else the program's own name.
    let pkg = spec.args.iter().find(|a| !a.starts_with('-'));
    let base = pkg.cloned().or_else(|| spec.command.clone()).unwrap_or_else(|| "Pasted server".into());
    let base = base.rsplit(['/', '\\']).next().unwrap_or(&base).to_string();
    base.trim_end_matches(".exe").trim_start_matches("server-").trim_start_matches("mcp-server-").to_string()
}

/// The servers in a setup's text, in its order: `{"mcpServers": {…}}`
/// (Claude, Cursor), `{"servers": {…}}` (VS Code), a bare map of servers, a
/// single server's object, a link, or a command line. `Err`: none found.
/// Pure.
pub fn servers_in(text: &str) -> Result<(Servers, usize), String> {
    let text = text.trim().trim_start_matches('\u{feff}').trim();
    if text.is_empty() {
        return Err("Paste a setup first.".into());
    }
    if text.starts_with('{') {
        let v: Value = serde_json::from_str(text).map_err(|_| "That isn't JSON Yap can read (check its brackets and commas).".to_string())?;
        let map: Map<String, Value> = match v.get("mcpServers").or_else(|| v.get("servers")).and_then(Value::as_object) {
            Some(m) => m.clone(),
            None if v.get("command").is_some() || v.get("url").is_some() || v.get("serverUrl").is_some() => {
                let mut m = Map::new();
                m.insert("Pasted server".into(), v.clone());
                m
            }
            None => v.as_object().cloned().unwrap_or_default(),
        };
        let mut out = Vec::new();
        let mut skipped = 0;
        for (name, server) in &map {
            match server_spec(name, server) {
                Some(s) => out.push((name.clone(), s)),
                None => skipped += 1,
            }
        }
        if out.is_empty() {
            return Err(if skipped > 0 { "There's no server in it Yap can add.".into() } else { "Yap found no MCP servers in that.".into() });
        }
        return Ok((out, skipped));
    }
    if text.contains('\n') {
        return Err("Paste one link or one command line, or a JSON setup.".into());
    }
    let mut spec = if text.starts_with("https://") || text.starts_with("http://") {
        Spec { url: Some(text.to_string()), ..Default::default() }
    } else {
        let mut words = split_line(text);
        if words.is_empty() {
            return Err("Paste a setup first.".into());
        }
        let command = words.remove(0);
        if is_yap("", &command, &words) {
            return Err("There's no server in it Yap can add.".into());
        }
        Spec { command: Some(command), args: words, ..Default::default() }
    };
    spec.name = pasted_name(&spec);
    let mut name = spec.name.clone();
    if let Some(first) = name.get(..1) {
        name = first.to_uppercase() + &name[1..];
    }
    spec.name = name;
    Ok((vec![("1".into(), spec)], 0))
}

/// Claude Desktop's settings files on this PC (both builds' copies that
/// exist), through `mcp_clients.rs`'s places (a test run's stand-in profile).
fn claude_files() -> Vec<PathBuf> {
    super::super::mcp_clients::claude_desktop_files()
}

/// The setup `source` holds: its servers, where it was read from, how many
/// were skipped. Reads Claude Desktop's files (never writes them).
pub fn read(source: &Source) -> Result<(Servers, Vec<String>, usize), String> {
    match source {
        Source::Paste { text } => servers_in(text).map(|(s, n)| (s, Vec::new(), n)),
        Source::ClaudeDesktop => {
            let files = claude_files();
            if files.is_empty() {
                return Err("Yap didn't find Claude Desktop's settings on this PC.".into());
            }
            let mut out: Servers = Vec::new();
            let mut skipped = 0;
            let mut from = Vec::new();
            let mut problem = None;
            for f in &files {
                let text = match std::fs::read_to_string(f) {
                    Ok(t) => t,
                    Err(_) => continue,
                };
                from.push(f.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default());
                match servers_in(&text) {
                    Ok((servers, n)) => {
                        skipped += n;
                        for (k, s) in servers {
                            if !out.iter().any(|(have, _)| *have == k) {
                                out.push((k, s));
                            }
                        }
                    }
                    Err(e) if e.starts_with("That isn't JSON") => problem = Some("Claude Desktop's settings file isn't plain JSON, so Yap left it alone.".to_string()),
                    Err(_) => {}
                }
            }
            if out.is_empty() {
                return Err(problem.unwrap_or_else(|| "Claude Desktop has no MCP servers Yap can add.".into()));
            }
            Ok((out, from, skipped))
        }
    }
}

/// Whether one of the person's connectors reaches `spec` already (the same
/// link, or the same command line). Pure.
fn already(spec: &Spec, preview: Option<&PreviewView>, have: &[super::store::Connector]) -> bool {
    have.iter().any(|c| match &c.transport {
        super::store::Transport::Remote { url, .. } => spec.url.as_deref().and_then(|u| url::Url::parse(u.trim()).ok()).is_some_and(|u| u.as_str() == url),
        super::store::Transport::Local { resolved, args, .. } => {
            preview.and_then(|p| p.program.as_ref()).is_some_and(|p| p.command_line == super::local::command_line(std::path::Path::new(resolved), args))
        }
    })
}

/// [`ScanView`] of `source`: each server shown whole, no secret in it.
pub fn scan(source: &Source) -> Result<ScanView, String> {
    let (servers, read_from, skipped) = read(source)?;
    let have = super::store::list();
    let out = servers
        .into_iter()
        .map(|(key, spec)| {
            let pv = preview(&spec);
            let (preview, problem) = match pv {
                Ok(p) => (Some(p), None),
                Err(e) => (None, Some(e)),
            };
            let address = match (&spec.command, &preview) {
                (Some(_), Some(p)) => p.program.as_ref().map(|p| p.command_line.clone()).unwrap_or_default(),
                (Some(c), None) => std::iter::once(c.clone()).chain(spec.args.iter().cloned()).collect::<Vec<_>>().join(" "),
                (None, _) => spec.url.clone().unwrap_or_default(),
            };
            let env = spec
                .env
                .iter()
                .map(|p| {
                    let secret = p.secret.unwrap_or_else(|| secret_name(&p.name));
                    FoundVar { name: p.name.clone(), value: (!secret).then(|| p.value.clone()), secret }
                })
                .collect();
            Found {
                added: already(&spec, preview.as_ref(), &have),
                key,
                name: spec.name.chars().take(60).collect(),
                kind: if spec.command.is_some() { "local" } else { "remote" },
                address,
                preview,
                env,
                headers: spec.headers.iter().map(|h| h.name.clone()).collect(),
                token: spec.token.is_some(),
                problem,
            }
        })
        .collect();
    Ok(ScanView { servers: out, read_from, skipped })
}

/// The specs to add for `picks` (read again from `source`, each program
/// with the fingerprint the person saw): the ones found, and the keys that
/// weren't.
pub fn specs_for(source: &Source, picks: &[Pick]) -> Result<(Vec<Spec>, Vec<String>), String> {
    let (servers, _, _) = read(source)?;
    let mut out = Vec::new();
    let mut missing = Vec::new();
    for p in picks {
        match servers.iter().find(|(k, _)| *k == p.key) {
            Some((_, s)) => {
                let mut s = s.clone();
                s.approved = p.approved.clone();
                out.push(s);
            }
            None => missing.push(p.key.clone()),
        }
    }
    Ok((out, missing))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLAUDE: &str = r#"{
      "mcpServers": {
        "filesystem": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "C:\\Users\\Sam\\Documents"] },
        "github": { "command": "docker", "args": ["run", "-i", "--rm", "ghcr.io/github/github-mcp-server"], "env": { "GITHUB_PERSONAL_ACCESS_TOKEN": "ghp_FAKE123", "LOG_LEVEL": "info" } },
        "linear": { "type": "http", "url": "https://mcp.example.com/mcp", "headers": { "Authorization": "Bearer lin_FAKE", "X-Team": "eng" } },
        "yap": { "command": "C:\\Program Files\\Yap\\yap.exe", "args": ["mcp"] },
        "broken": { "note": "no command" }
      },
      "globalShortcut": "Ctrl+Space"
    }"#;

    #[test]
    fn claudes_setup_is_read_server_by_server_and_yaps_own_is_left_out() {
        let (servers, skipped) = servers_in(CLAUDE).unwrap();
        assert_eq!(skipped, 2, "Yap's own and one with no command");
        let names: Vec<&str> = servers.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, ["filesystem", "github", "linear"]);
        let gh = &servers[1].1;
        assert_eq!(gh.command.as_deref(), Some("docker"));
        assert_eq!(gh.env.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["GITHUB_PERSONAL_ACCESS_TOKEN", "LOG_LEVEL"]);
        let lin = &servers[2].1;
        assert_eq!(lin.url.as_deref(), Some("https://mcp.example.com/mcp"));
        assert_eq!(lin.token.as_deref(), Some("lin_FAKE"), "an Authorization: Bearer header is the token");
        assert_eq!(lin.headers.len(), 1);
        assert_eq!(lin.headers[0].name, "X-Team");
    }

    #[test]
    fn a_paste_can_be_a_block_a_server_a_link_or_a_command_line() {
        let (s, _) = servers_in(r#"{ "servers": { "files": { "type": "stdio", "command": "uvx", "args": ["mcp-server-time"] } } }"#).unwrap();
        assert_eq!(s[0].1.command.as_deref(), Some("uvx"));
        let (s, _) = servers_in(r#"{ "url": "https://mcp.example.com/sse", "type": "sse" }"#).unwrap();
        assert!(s[0].1.sse);
        assert_eq!(s[0].0, "Pasted server");
        let (s, _) = servers_in("https://mcp.example.com/mcp").unwrap();
        assert_eq!(s[0].1.name, "Example");
        let (s, _) = servers_in(r#"npx -y @modelcontextprotocol/server-filesystem "C:\Users\Sam\My Documents""#).unwrap();
        let spec = &s[0].1;
        assert_eq!(spec.command.as_deref(), Some("npx"));
        assert_eq!(spec.args, ["-y", "@modelcontextprotocol/server-filesystem", "C:\\Users\\Sam\\My Documents"]);
        assert_eq!(spec.name, "Filesystem");
        assert!(servers_in("").is_err());
        assert!(servers_in("{ not json").unwrap_err().contains("isn't JSON"));
        assert!(servers_in(r#"{ "mcpServers": {} }"#).unwrap_err().contains("no MCP servers"));
        assert!(servers_in("yap.exe mcp").is_err(), "Yap's own, pasted");
    }

    #[test]
    fn quotes_keep_an_argument_together() {
        assert_eq!(split_line(r#"a "b c" 'd e' f"#), ["a", "b c", "d e", "f"]);
        assert_eq!(split_line(r#"x "" y"#), ["x", "", "y"]);
        assert!(split_line("   ").is_empty());
    }
}

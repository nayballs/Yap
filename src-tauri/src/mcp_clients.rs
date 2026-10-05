//! "Add to Claude / ChatGPT / Gemini / Cursor…" (Settings → MCP):
//! one click adds Yap's MCP server (`yap.exe mcp`, see `mcp.rs`) to an AI
//! app by editing that app's own config file, the way its docs say to add a
//! local server; Remove takes it out again.
//!
//! Where each app keeps its servers on Windows (checked 2026-10-05):
//! - **Claude** (desktop): `claude_desktop_config.json` → `mcpServers`. The
//!   MSIX build (today's installer) reads it from its package's LocalCache
//!   (`%LOCALAPPDATA%\Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\Claude`)
//!   rather than `%APPDATA%\Claude` (anthropics/claude-code#26073), so every
//!   copy that exists is written.
//! - **Claude Code**: `~/.claude.json` → `mcpServers` (user scope, what
//!   `claude mcp add --scope user` writes), edited under Claude Code's own
//!   lock (`~/.claude.json.lock`, a proper-lockfile directory): Claude Code
//!   re-reads the file under that lock before each save, so a running session
//!   keeps Yap's entry.
//! - **ChatGPT**: the desktop app shares Codex's `~/.codex/config.toml` →
//!   `[mcp_servers.yap]` with the Codex CLI and IDE extension. ChatGPT on
//!   the web only reaches servers on the internet, so it can't use Yap.
//! - **Gemini CLI**: `~/.gemini/settings.json` → `mcpServers`.
//! - **Cursor**: `~/.cursor/mcp.json` → `mcpServers`, with `type: "stdio"`.
//! - **VS Code** (and Insiders): `%APPDATA%\Code\User\mcp.json` → `servers`,
//!   with `type: "stdio"`.
//! - **Windsurf**, renamed Devin Desktop in June 2026:
//!   `%APPDATA%\devin\mcp_config.json`, and the older
//!   `~/.codeium/windsurf/mcp_config.json` → `mcpServers`.
//!
//! The editing rules are the ones a careful person follows by hand. Only the
//! `yap` entry changes: every other byte stays as it was (other servers,
//! settings, key order and indentation; comments in TOML). A JSON file that
//! isn't plain JSON (comments, a typo) is left alone, with the reason. The
//! first edit keeps the original as `<file>.bak`. Writes go to a temp file
//! that is renamed over the old one, so an app never reads half a file.
//!
//! Tests never touch a real AI app: debug builds take `YAP_MCP_CLIENT_ROOT`
//! as a stand-in user profile, and a test-mode instance (`e2e.rs`) never uses
//! the real one.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde_json::value::RawValue;
use serde_json::{json, Value};

/// The entry's name in every app's server list.
pub const SERVER_NAME: &str = "yap";
/// `yap.exe <ARG>` starts the MCP server.
pub const SERVER_ARG: &str = "mcp";

/// How an app lists its MCP servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// `{"mcpServers": {"yap": {"command", "args"}}}` (Claude, Gemini CLI, Windsurf).
    McpServers,
    /// The same with `"type": "stdio"` (Cursor).
    McpServersTyped,
    /// Claude Code's `~/.claude.json`: typed, an empty `env`, under its lock.
    ClaudeCode,
    /// VS Code: `{"servers": {"yap": {"type": "stdio", …}}}`.
    VsCode,
    /// Codex and the ChatGPT desktop app: `[mcp_servers.yap]` in TOML.
    CodexToml,
}

struct ClientDef {
    id: &'static str,
    name: &'static str,
    detail: &'static str,
    /// What to do after adding, for the toast.
    restart: &'static str,
    format: Format,
}

/// In Wispr Flow's order (Claude, ChatGPT, Gemini, Cursor), then the rest.
const CLIENTS: [ClientDef; 7] = [
    ClientDef {
        id: "claude",
        name: "Claude",
        detail: "Claude desktop app",
        restart: "Quit Claude (right-click its tray icon → Quit) and open it again.",
        format: Format::McpServers,
    },
    ClientDef {
        id: "chatgpt",
        name: "ChatGPT",
        detail: "ChatGPT desktop app and Codex (ChatGPT on the web can't reach your PC)",
        restart: "Restart ChatGPT: Yap then shows under Settings → MCP servers. Codex picks it up on its next start.",
        format: Format::CodexToml,
    },
    ClientDef {
        id: "gemini",
        name: "Gemini",
        detail: "Gemini CLI",
        restart: "Restart Gemini CLI, then check with /mcp.",
        format: Format::McpServers,
    },
    ClientDef {
        id: "cursor",
        name: "Cursor",
        detail: "Cursor editor",
        restart: "Cursor lists it under Settings → MCP; switch it on there if it asks.",
        format: Format::McpServersTyped,
    },
    ClientDef {
        id: "claude-code",
        name: "Claude Code",
        detail: "Claude in your terminal",
        restart: "New Claude Code sessions pick it up; check with /mcp.",
        format: Format::ClaudeCode,
    },
    ClientDef {
        id: "vscode",
        name: "VS Code",
        detail: "Copilot agent mode in VS Code (and Insiders)",
        restart: "VS Code picks it up from mcp.json; start it from the MCP servers list if it asks.",
        format: Format::VsCode,
    },
    ClientDef {
        id: "windsurf",
        name: "Windsurf",
        detail: "Windsurf, now Devin Desktop",
        restart: "Refresh the MCP servers in Windsurf's settings, or restart it.",
        format: Format::McpServers,
    },
];

fn client(id: &str) -> Result<&'static ClientDef, String> {
    CLIENTS
        .iter()
        .find(|c| c.id == id)
        .ok_or_else(|| format!("Unknown AI app: {id}"))
}

// ---- where things are ----

/// The user-profile folders the apps' files live under.
#[derive(Debug, Clone)]
pub struct Bases {
    pub home: PathBuf,
    /// `%APPDATA%` (Roaming).
    pub appdata: PathBuf,
    /// `%LOCALAPPDATA%`.
    pub local: PathBuf,
    /// The person's real profile (so apps' own env overrides apply).
    pub real: bool,
}

impl Bases {
    /// A stand-in profile laid out like a Windows one (tests).
    pub fn under(root: PathBuf) -> Self {
        Self {
            appdata: root.join("AppData").join("Roaming"),
            local: root.join("AppData").join("Local"),
            home: root,
            real: false,
        }
    }
}

fn bases() -> Result<Bases, String> {
    #[cfg(debug_assertions)]
    if let Some(root) = std::env::var_os("YAP_MCP_CLIENT_ROOT").filter(|r| !r.is_empty()) {
        return Ok(Bases::under(PathBuf::from(root)));
    }
    // A test instance that wasn't given a stand-in still never writes to the
    // developer's own AI apps.
    if crate::e2e::active() {
        return Ok(Bases::under(crate::config::data_dir().join("mcp-clients")));
    }
    let missing = || "Couldn't find your user folders".to_string();
    Ok(Bases {
        home: dirs::home_dir().ok_or_else(missing)?,
        appdata: dirs::config_dir().ok_or_else(missing)?,
        local: dirs::data_local_dir().ok_or_else(missing)?,
        real: true,
    })
}

/// An app's env override of its own folder (only for the real profile).
fn env_dir(bases: &Bases, var: &str) -> Option<PathBuf> {
    if !bases.real {
        return None;
    }
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Installed MSIX packages whose folder name starts with `prefix`.
fn packages(bases: &Bases, prefix: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(bases.local.join("Packages"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .to_lowercase()
                .starts_with(&prefix.to_lowercase())
        })
        .map(|e| e.path())
        .collect();
    found.sort();
    found
}

/// One config file an app reads, and whether that copy of the app is here.
#[derive(Debug, Clone)]
pub struct Target {
    pub path: PathBuf,
    pub present: bool,
}

fn targets(id: &str, b: &Bases) -> Vec<Target> {
    let t = |path: PathBuf, present: bool| Target { path, present };
    match id {
        "claude" => {
            let mut out: Vec<Target> = packages(b, "Claude_")
                .into_iter()
                .map(|pkg| {
                    let dir = pkg.join("LocalCache").join("Roaming").join("Claude");
                    t(dir.join("claude_desktop_config.json"), true)
                })
                .collect();
            let classic = b.appdata.join("Claude");
            let present = classic.is_dir() || b.local.join("AnthropicClaude").is_dir();
            out.push(t(classic.join("claude_desktop_config.json"), present));
            out
        }
        "claude-code" => {
            let file = env_dir(b, "CLAUDE_CONFIG_DIR")
                .map(|d| d.join(".claude.json"))
                .unwrap_or_else(|| b.home.join(".claude.json"));
            let present = file.is_file() || b.home.join(".claude").is_dir();
            vec![t(file, present)]
        }
        "chatgpt" => {
            let dir = env_dir(b, "CODEX_HOME").unwrap_or_else(|| b.home.join(".codex"));
            let present = dir.is_dir() || !packages(b, "OpenAI.ChatGPT").is_empty();
            vec![t(dir.join("config.toml"), present)]
        }
        "gemini" => {
            let dir = b.home.join(".gemini");
            vec![t(dir.join("settings.json"), dir.is_dir())]
        }
        "cursor" => {
            let dir = b.home.join(".cursor");
            vec![t(dir.join("mcp.json"), dir.is_dir())]
        }
        "vscode" => ["Code", "Code - Insiders"]
            .iter()
            .map(|app| {
                let dir = b.appdata.join(app);
                t(dir.join("User").join("mcp.json"), dir.is_dir())
            })
            .collect(),
        "windsurf" => {
            let devin = b.appdata.join("devin");
            let legacy = b.home.join(".codeium").join("windsurf");
            vec![
                t(devin.join("mcp_config.json"), devin.is_dir()),
                t(legacy.join("mcp_config.json"), legacy.is_dir()),
            ]
        }
        _ => Vec::new(),
    }
}

// ---- the entry ----

/// Yap's entry for an app, as JSON (TOML is built from the same fields).
pub fn entry(format: Format, exe: &str) -> Value {
    match format {
        Format::McpServers => json!({ "command": exe, "args": [SERVER_ARG] }),
        Format::McpServersTyped | Format::VsCode => {
            json!({ "type": "stdio", "command": exe, "args": [SERVER_ARG] })
        }
        Format::ClaudeCode => {
            json!({ "type": "stdio", "command": exe, "args": [SERVER_ARG], "env": {} })
        }
        Format::CodexToml => json!({ "command": exe, "args": [SERVER_ARG] }),
    }
}

fn servers_key(format: Format) -> &'static str {
    if format == Format::VsCode {
        "servers"
    } else {
        "mcpServers"
    }
}

/// A JSON object's entries, in file order, each value as its original text.
struct Entries<'a>(Vec<(String, &'a RawValue)>);

impl<'de> Deserialize<'de> for Entries<'de> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Object;
        impl<'de> Visitor<'de> for Object {
            type Value = Entries<'de>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a JSON object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut out = Vec::new();
                while let Some((k, v)) = map.next_entry::<String, &'de RawValue>()? {
                    out.push((k, v));
                }
                Ok(Entries(out))
            }
        }
        d.deserialize_map(Object)
    }
}

fn owned(entries: Entries) -> Vec<(String, String)> {
    entries
        .0
        .into_iter()
        .map(|(k, v)| (k, v.get().to_string()))
        .collect()
}

/// Why Yap won't edit a JSON file.
fn json_problem(e: &serde_json::Error) -> String {
    if e.is_data() {
        "isn't a JSON object".to_string()
    } else {
        format!(
            "isn't plain JSON (line {}, column {}), so Yap left it alone",
            e.line(),
            e.column()
        )
    }
}

/// The file's indent unit (its first indented line's), else two spaces.
fn indent_of(text: &str) -> String {
    text.lines()
        .skip(1)
        .find_map(|l| {
            let rest = l.trim_start_matches([' ', '\t']);
            let ws = &l[..l.len() - rest.len()];
            (!ws.is_empty() && !rest.is_empty()).then(|| {
                if ws.starts_with('\t') {
                    "\t".to_string()
                } else {
                    ws.to_string()
                }
            })
        })
        .unwrap_or_else(|| "  ".to_string())
}

/// An object at `depth`, its values written as given.
fn emit(entries: &[(String, String)], indent: &str, depth: usize, nl: &str) -> String {
    if entries.is_empty() {
        return "{}".to_string();
    }
    let inner = indent.repeat(depth + 1);
    let body: Vec<String> = entries
        .iter()
        .map(|(k, v)| format!("{inner}{}: {v}", Value::String(k.clone())))
        .collect();
    format!(
        "{{{nl}{}{nl}{}}}",
        body.join(&format!(",{nl}")),
        indent.repeat(depth)
    )
}

/// Yap's entry written the way people write these by hand (`command` first,
/// `args` on one line), in the file's style, for a place at `depth`.
fn entry_text(format: Format, exe: &str, indent: &str, depth: usize, nl: &str) -> String {
    let mut fields = Vec::new();
    if matches!(format, Format::McpServersTyped | Format::VsCode | Format::ClaudeCode) {
        fields.push("\"type\": \"stdio\"".to_string());
    }
    fields.push(format!("\"command\": {}", Value::String(exe.to_string())));
    fields.push(format!("\"args\": [{}]", Value::String(SERVER_ARG.to_string())));
    if format == Format::ClaudeCode {
        fields.push("\"env\": {}".to_string());
    }
    let inner = indent.repeat(depth + 1);
    let body: Vec<String> = fields.iter().map(|f| format!("{inner}{f}")).collect();
    format!(
        "{{{nl}{}{nl}{}}}",
        body.join(&format!(",{nl}")),
        indent.repeat(depth)
    )
}

/// The text of a JSON config with Yap's entry added (`add`) or removed.
/// `None` when nothing needs to change (already there / already gone).
pub fn edit_json(original: Option<&str>, format: Format, exe: &str, add: bool) -> Result<Option<String>, String> {
    let text = original.unwrap_or("");
    let bom = text.starts_with('\u{feff}');
    let body = text.trim_start_matches('\u{feff}');
    let nl = if body.contains("\r\n") { "\r\n" } else { "\n" };
    let indent = indent_of(body);
    let key = servers_key(format);

    let mut root: Vec<(String, String)> = if body.trim().is_empty() {
        Vec::new()
    } else {
        owned(serde_json::from_str::<Entries>(body).map_err(|e| json_problem(&e))?)
    };
    let at = root.iter().position(|(k, _)| k == key);
    let mut servers: Vec<(String, String)> = match at {
        Some(i) => owned(
            serde_json::from_str::<Entries>(&root[i].1)
                .map_err(|_| format!("has a \"{key}\" that isn't an object, so Yap left it alone"))?,
        ),
        None if add => Vec::new(),
        None => return Ok(None),
    };
    let mine = servers.iter().position(|(k, _)| k == SERVER_NAME);
    let copies = servers.iter().filter(|(k, _)| k == SERVER_NAME).count();
    if add {
        let want = entry(format, exe);
        if let Some(j) = mine {
            let same = serde_json::from_str::<Value>(&servers[j].1).ok() == Some(want.clone());
            if same && copies == 1 {
                return Ok(None);
            }
            servers[j].1 = entry_text(format, exe, &indent, 2, nl);
            let mut seen = false;
            servers.retain(|(k, _)| k != SERVER_NAME || !std::mem::replace(&mut seen, true));
        } else {
            servers.push((SERVER_NAME.to_string(), entry_text(format, exe, &indent, 2, nl)));
        }
    } else {
        if mine.is_none() {
            return Ok(None);
        }
        servers.retain(|(k, _)| k != SERVER_NAME);
    }
    let list = emit(&servers, &indent, 1, nl);
    match at {
        Some(i) => root[i].1 = list,
        None => root.push((key.to_string(), list)),
    }
    let mut out = emit(&root, &indent, 0, nl);
    if body.trim().is_empty() || body.ends_with('\n') {
        out.push_str(nl);
    }
    if bom {
        out.insert(0, '\u{feff}');
    }
    Ok(Some(out))
}

/// The same for Codex's TOML (`[mcp_servers.yap]`), keeping comments and
/// formatting.
pub fn edit_toml(original: Option<&str>, exe: &str, add: bool) -> Result<Option<String>, String> {
    use toml_edit::{Array, DocumentMut, Item, Table};
    let text = original.unwrap_or("");
    let bom = text.starts_with('\u{feff}');
    let body = text.trim_start_matches('\u{feff}');
    let mut doc: DocumentMut = body.parse().map_err(|e: toml_edit::TomlError| {
        let first = e.to_string().lines().next().unwrap_or_default().to_string();
        format!("isn't valid TOML ({first}), so Yap left it alone")
    })?;
    if add {
        let servers = doc.entry("mcp_servers").or_insert_with(|| {
            let mut t = Table::new();
            t.set_implicit(true);
            Item::Table(t)
        });
        let Some(servers) = servers.as_table_like_mut() else {
            return Err("has an \"mcp_servers\" that isn't a table, so Yap left it alone".to_string());
        };
        if servers.get(SERVER_NAME).is_some_and(|cur| toml_matches(cur, exe)) {
            return Ok(None);
        }
        let mut t = Table::new();
        t.insert("command", toml_edit::value(exe));
        t.insert("args", toml_edit::value(Array::from_iter([SERVER_ARG])));
        servers.insert(SERVER_NAME, Item::Table(t));
    } else {
        let Some(servers) = doc.get_mut("mcp_servers").and_then(Item::as_table_like_mut) else {
            return Ok(None);
        };
        if servers.remove(SERVER_NAME).is_none() {
            return Ok(None);
        }
    }
    let mut out = doc.to_string();
    if body.contains("\r\n") {
        out = out.replace("\r\n", "\n").replace('\n', "\r\n");
    }
    if bom {
        out.insert(0, '\u{feff}');
    }
    Ok(Some(out))
}

fn toml_matches(item: &toml_edit::Item, exe: &str) -> bool {
    let args: Vec<&str> = item
        .get("args")
        .and_then(toml_edit::Item::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    item.get("command").and_then(toml_edit::Item::as_str) == Some(exe) && args == [SERVER_ARG]
}

/// Yap's entry in a config's text, if there is one (and the file reads).
fn read_entry(text: &str, format: Format) -> Result<Option<Value>, String> {
    let body = text.trim_start_matches('\u{feff}');
    if body.trim().is_empty() {
        return Ok(None);
    }
    if format == Format::CodexToml {
        let doc: toml_edit::DocumentMut = body
            .parse()
            .map_err(|_| "isn't valid TOML, so Yap can't edit it".to_string())?;
        let Some(item) = doc.get("mcp_servers").and_then(|s| s.get(SERVER_NAME)) else {
            return Ok(None);
        };
        let args: Vec<Value> = item
            .get("args")
            .and_then(toml_edit::Item::as_array)
            .map(|a| a.iter().filter_map(|v| v.as_str()).map(|s| json!(s)).collect())
            .unwrap_or_default();
        return Ok(Some(json!({
            "command": item.get("command").and_then(toml_edit::Item::as_str),
            "args": args,
        })));
    }
    let root = serde_json::from_str::<Entries>(body).map_err(|e| json_problem(&e))?;
    let Some((_, servers)) = root.0.iter().find(|(k, _)| k == servers_key(format)) else {
        return Ok(None);
    };
    let Ok(servers) = serde_json::from_str::<Entries>(servers.get()) else {
        return Ok(None);
    };
    Ok(servers
        .0
        .iter()
        .find(|(k, _)| k == SERVER_NAME)
        .and_then(|(_, v)| serde_json::from_str(v.get()).ok()))
}

// ---- files ----

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    path.with_file_name(name)
}

/// Write `text` over `path` atomically, keeping the original as `.bak` the
/// first time Yap changes the file (an existing `.bak` is never replaced).
fn write_with_backup(path: &Path, original: Option<&[u8]>, text: &str) -> Result<(), String> {
    let shown = path.display();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("Couldn't create {}: {e}", dir.display()))?;
    }
    if let Some(bytes) = original {
        let bak = backup_path(path);
        if !bak.exists() {
            std::fs::write(&bak, bytes).map_err(|e| format!("Couldn't back up {shown}: {e}"))?;
        }
    }
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".yap-tmp");
    let tmp = path.with_file_name(tmp_name);
    std::fs::write(&tmp, text).map_err(|e| format!("Couldn't write {shown}: {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Couldn't save {shown}: {e}")
    })
}

/// Claude Code's lock on `~/.claude.json`: a `.lock` directory
/// (proper-lockfile). Its holder refreshes it every few seconds and treats
/// one untouched for 10 s as abandoned; so does Yap.
fn with_lock<R>(
    config: &Path,
    wait: Duration,
    stale_after: Duration,
    f: impl FnOnce() -> Result<R, String>,
) -> Result<R, String> {
    let mut lock = config.as_os_str().to_os_string();
    lock.push(".lock");
    let lock = PathBuf::from(lock);
    if let Some(dir) = config.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let deadline = Instant::now() + wait;
    loop {
        match std::fs::create_dir(&lock) {
            Ok(()) => break,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let abandoned = std::fs::metadata(&lock)
                    .and_then(|m| m.modified())
                    .is_ok_and(|t| t.elapsed().unwrap_or_default() >= stale_after);
                if abandoned {
                    let _ = std::fs::remove_dir(&lock);
                    continue;
                }
                if Instant::now() >= deadline {
                    return Err("Claude Code is saving its settings right now. Try again in a moment.".to_string());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(format!("Couldn't lock {}: {e}", config.display())),
        }
    }
    let result = f();
    let _ = std::fs::remove_dir(&lock);
    result
}

/// Add or remove Yap's entry in one file. Returns whether it changed.
fn edit_file(path: &Path, format: Format, exe: &str, add: bool) -> Result<bool, String> {
    let run = || {
        let original = match std::fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("Couldn't read {}: {e}", path.display())),
        };
        let text = match &original {
            Some(bytes) => Some(
                std::str::from_utf8(bytes)
                    .map_err(|_| format!("{} isn't UTF-8 text, so Yap left it alone", path.display()))?,
            ),
            None => None,
        };
        let edited = match format {
            Format::CodexToml => edit_toml(text, exe, add),
            _ => edit_json(text, format, exe, add),
        }
        .map_err(|why| format!("{} {why}", path.display()))?;
        match edited {
            Some(new) => write_with_backup(path, original.as_deref(), &new).map(|()| true),
            None => Ok(false),
        }
    };
    if format == Format::ClaudeCode {
        with_lock(path, Duration::from_secs(5), Duration::from_secs(10), run)
    } else {
        run()
    }
}

// ---- status, add, remove ----

/// This exe, as the apps should launch it.
fn exe() -> Result<String, String> {
    std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| format!("Couldn't find Yap's own program file: {e}"))
}

/// One app's card or row in Settings → MCP.
fn status(def: &ClientDef, b: &Bases, exe: &str) -> Value {
    let all = targets(def.id, b);
    let present: Vec<&Target> = all.iter().filter(|t| t.present).collect();
    let mut entries = 0;
    let (mut runnable, mut elsewhere) = (true, false);
    let mut problem: Option<String> = None;
    for t in &present {
        let Ok(text) = std::fs::read_to_string(&t.path) else {
            continue;
        };
        match read_entry(&text, def.format) {
            Ok(Some(e)) => {
                entries += 1;
                let cmd = e["command"].as_str().unwrap_or_default();
                runnable &= Path::new(cmd).is_file();
                elsewhere |= !cmd.eq_ignore_ascii_case(exe);
            }
            Ok(None) => {}
            // The file's name is enough on the row; `paths` has the rest.
            Err(why) => {
                let name = t.path.file_name().unwrap_or_default().to_string_lossy();
                problem = Some(format!("{name} {why}"));
            }
        }
    }
    let state = if present.is_empty() {
        "missing"
    } else if entries == present.len() && runnable {
        "added"
    } else if entries > 0 && !runnable {
        "outdated"
    } else {
        "available"
    };
    json!({
        "id": def.id,
        "name": def.name,
        "detail": def.detail,
        "restart": def.restart,
        "installed": !present.is_empty(),
        "status": state,
        // Added by another copy of Yap (say the installed app, seen from a
        // dev build): it works, it just isn't this exe.
        "elsewhere": state == "added" && elsewhere,
        "problem": problem,
        "paths": present.iter().map(|t| t.path.to_string_lossy()).collect::<Vec<_>>(),
    })
}

/// The "All other apps" JSON: the `mcpServers` block most apps take.
fn snippet(exe: &str) -> String {
    let yap = entry_text(Format::McpServers, exe, "  ", 2, "\n");
    let servers = emit(&[(SERVER_NAME.to_string(), yap)], "  ", 1, "\n");
    emit(&[("mcpServers".to_string(), servers)], "  ", 0, "\n")
}

/// Everything Settings → MCP shows.
pub fn overview_in(b: &Bases, exe: &str) -> Value {
    json!({
        "command": exe,
        "args": [SERVER_ARG],
        "commandLine": format!("\"{exe}\" {SERVER_ARG}"),
        "snippet": snippet(exe),
        "devBuild": cfg!(debug_assertions),
        "clients": CLIENTS.iter().map(|c| status(c, b, exe)).collect::<Vec<_>>(),
    })
}

/// Add (or, `add == false`, remove) Yap in one app; returns its new row.
pub fn change_in(id: &str, add: bool, b: &Bases, exe: &str) -> Result<Value, String> {
    let def = client(id)?;
    let all = targets(def.id, b);
    let present: Vec<&Target> = all.iter().filter(|t| t.present).collect();
    if present.is_empty() {
        return Err(format!("{} isn't installed on this PC.", def.name));
    }
    for t in present {
        if add || t.path.exists() {
            edit_file(&t.path, def.format, exe, add)?;
        }
    }
    let row = status(def, b, exe);
    tracing::info!(app = def.id, add, "mcp: AI app config updated");
    Ok(row)
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}

/// Settings → MCP: Yap's command, a JSON snippet, and one row per app.
#[tauri::command]
pub async fn mcp_clients_status() -> Result<Value, String> {
    blocking(|| Ok(overview_in(&bases()?, &exe()?))).await
}

/// "Add to …": put Yap in that app's config. Returns the app's new row.
#[tauri::command]
pub async fn mcp_client_add(id: String) -> Result<Value, String> {
    blocking(move || change_in(&id, true, &bases()?, &exe()?)).await
}

/// "Remove": take Yap out of that app's config. Returns the app's new row.
#[tauri::command]
pub async fn mcp_client_remove(id: String) -> Result<Value, String> {
    blocking(move || change_in(&id, false, &bases()?, &exe()?)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = r"C:\Program Files\Yap\yap.exe";

    fn add(text: Option<&str>, format: Format) -> String {
        edit_json(text, format, EXE, true).unwrap().expect("a change")
    }

    fn parsed(text: &str) -> Value {
        serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap()
    }

    const CLAUDE: &str = r#"{
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "C:\\Users\\me\\Desktop"]
    }
  },
  "preferences": {"quickEntryShortcut": "off", "menuBarEnabled": false},
  "zeta": 1,
  "alpha": [1, 2, 3]
}
"#;

    #[test]
    fn adding_keeps_every_other_server_and_setting_as_it_was() {
        let out = add(Some(CLAUDE), Format::McpServers);
        // The other server's text and the other settings are byte for byte
        // the same, in the same (unsorted) order.
        assert!(out.contains(r#""args": ["-y", "@modelcontextprotocol/server-filesystem", "C:\\Users\\me\\Desktop"]"#));
        assert!(out.contains(r#""preferences": {"quickEntryShortcut": "off", "menuBarEnabled": false},"#));
        let order: Vec<usize> = ["\"mcpServers\"", "\"preferences\"", "\"zeta\"", "\"alpha\""]
            .iter()
            .map(|k| out.find(k).unwrap())
            .collect();
        assert!(order.windows(2).all(|w| w[0] < w[1]));
        let v = parsed(&out);
        assert_eq!(v["mcpServers"]["yap"], json!({ "command": EXE, "args": ["mcp"] }));
        assert_eq!(v["mcpServers"]["filesystem"]["command"], "npx");
        assert_eq!(v["alpha"], json!([1, 2, 3]));
        assert!(out.ends_with("}\n"));
        assert!(
            out.contains("\n    \"yap\": {\n      \"command\": \"C:\\\\Program Files\\\\Yap\\\\yap.exe\",\n      \"args\": [\"mcp\"]\n    }\n  },"),
            "written in the file's own indent:\n{out}"
        );
    }

    #[test]
    fn adding_twice_changes_nothing_and_removing_restores_the_file() {
        let once = add(Some(CLAUDE), Format::McpServers);
        assert_eq!(edit_json(Some(&once), Format::McpServers, EXE, true).unwrap(), None);
        let removed = edit_json(Some(&once), Format::McpServers, EXE, false)
            .unwrap()
            .expect("a change");
        assert_eq!(removed, CLAUDE, "add + remove gives back the same file");
        assert_eq!(edit_json(Some(&removed), Format::McpServers, EXE, false).unwrap(), None);
        // A Yap entry pointing at another copy is replaced, not duplicated.
        let moved = once.replace(r"C:\\Program Files\\Yap\\yap.exe", r"D:\\Old\\yap.exe");
        let fixed = add(Some(&moved), Format::McpServers);
        assert_eq!(fixed.matches("\"yap\"").count(), 1);
        assert_eq!(parsed(&fixed)["mcpServers"]["yap"]["command"], EXE);
    }

    #[test]
    fn missing_and_empty_files_get_a_fresh_list() {
        for text in [None, Some(""), Some("  \n")] {
            let out = add(text, Format::McpServers);
            assert_eq!(parsed(&out), json!({ "mcpServers": { "yap": { "command": EXE, "args": ["mcp"] } } }));
            assert!(out.ends_with("}\n"));
        }
        assert_eq!(edit_json(None, Format::McpServers, EXE, false).unwrap(), None);
        // A file with no server list yet gets one at the end.
        let out = add(Some("{\n  \"theme\": \"dark\"\n}"), Format::McpServers);
        assert_eq!(parsed(&out)["theme"], "dark");
        assert!(out.find("theme").unwrap() < out.find("mcpServers").unwrap());
        assert!(!out.ends_with('\n'), "no newline at the end before, none after");
    }

    #[test]
    fn files_yap_cannot_read_are_left_alone() {
        let commented = "{\n  // my servers\n  \"mcpServers\": {}\n}";
        let e = edit_json(Some(commented), Format::McpServers, EXE, true).unwrap_err();
        assert!(e.contains("isn't plain JSON (line 2"), "{e}");
        let trailing = "{\"mcpServers\": {\"a\": {},},}";
        assert!(edit_json(Some(trailing), Format::McpServers, EXE, true).is_err());
        assert!(edit_json(Some("[1, 2]"), Format::McpServers, EXE, true).unwrap_err().contains("isn't a JSON object"));
        assert!(edit_json(Some("{\"mcpServers\": []}"), Format::McpServers, EXE, true).unwrap_err().contains("isn't an object"));
        assert!(edit_json(Some("{\"mcpServers\": [}"), Format::McpServers, EXE, false).is_err());
    }

    #[test]
    fn bom_crlf_and_tabs_survive() {
        let original = "\u{feff}{\r\n\t\"mcpServers\": {\r\n\t\t\"a\": {\"command\": \"x\"}\r\n\t}\r\n}\r\n";
        let out = add(Some(original), Format::McpServers);
        assert!(out.starts_with('\u{feff}'));
        assert!(!out.replace("\r\n", "").contains('\n'), "only CRLF line ends: {out:?}");
        assert!(out.contains("\r\n\t\t\"yap\": {\r\n\t\t\t\"command\""));
        let back = edit_json(Some(&out), Format::McpServers, EXE, false).unwrap().unwrap();
        assert_eq!(back, original);
    }

    #[test]
    fn each_app_gets_its_own_shape() {
        let vscode = parsed(&add(Some("{\n\t\"servers\": {},\n\t\"inputs\": []\n}\n"), Format::VsCode));
        assert_eq!(vscode["servers"]["yap"], json!({ "type": "stdio", "command": EXE, "args": ["mcp"] }));
        assert_eq!(vscode["inputs"], json!([]));
        assert!(vscode.get("mcpServers").is_none());
        let cursor = parsed(&add(None, Format::McpServersTyped));
        assert_eq!(cursor["mcpServers"]["yap"]["type"], "stdio");
        let code = parsed(&add(Some(r#"{"numStartups": 3, "projects": {"C:/x": {"mcpServers": {}}}}"#), Format::ClaudeCode));
        assert_eq!(code["mcpServers"]["yap"]["env"], json!({}));
        assert_eq!(code["projects"]["C:/x"], json!({ "mcpServers": {} }), "project scopes untouched");
    }

    const CODEX: &str = "# my Codex settings\nmodel = \"gpt-5-codex\"\n\n[mcp_servers.context7]\ncommand = \"npx\" # docs\nargs = [\"-y\", \"@upstash/context7-mcp\"]\n";

    #[test]
    fn codex_toml_keeps_comments_and_other_servers() {
        let out = edit_toml(Some(CODEX), EXE, true).unwrap().expect("a change");
        assert!(out.starts_with(CODEX), "everything before stays as it was:\n{out}");
        assert!(out.contains("[mcp_servers.yap]"));
        let doc: toml_edit::DocumentMut = out.parse().unwrap();
        assert_eq!(doc["mcp_servers"]["yap"]["command"].as_str(), Some(EXE));
        assert_eq!(doc["mcp_servers"]["context7"]["command"].as_str(), Some("npx"));
        assert_eq!(edit_toml(Some(&out), EXE, true).unwrap(), None, "idempotent");
        let back = edit_toml(Some(&out), EXE, false).unwrap().unwrap();
        assert_eq!(back, CODEX);
        assert_eq!(edit_toml(Some(CODEX), EXE, false).unwrap(), None);
        // A fresh file gets just Yap's table; a broken one is refused.
        let fresh = edit_toml(None, EXE, true).unwrap().unwrap();
        assert!(fresh.starts_with("[mcp_servers.yap]\n"), "{fresh}");
        assert!(edit_toml(Some("model = \n["), EXE, true).unwrap_err().contains("isn't valid TOML"));
        assert_eq!(read_entry(&out, Format::CodexToml).unwrap().unwrap()["args"], json!(["mcp"]));
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yap-mcp-clients-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_first_edit_keeps_a_backup_and_a_bad_file_is_untouched() {
        let dir = scratch("files");
        let file = dir.join("claude_desktop_config.json");
        std::fs::write(&file, CLAUDE).unwrap();
        assert!(edit_file(&file, Format::McpServers, EXE, true).unwrap());
        assert_eq!(std::fs::read_to_string(backup_path(&file)).unwrap(), CLAUDE);
        assert!(!edit_file(&file, Format::McpServers, EXE, true).unwrap(), "nothing to do twice");
        assert!(edit_file(&file, Format::McpServers, EXE, false).unwrap());
        assert_eq!(std::fs::read_to_string(&file).unwrap(), CLAUDE);
        assert_eq!(std::fs::read_to_string(backup_path(&file)).unwrap(), CLAUDE, "the first backup stays");
        assert!(!dir.join("claude_desktop_config.json.yap-tmp").exists());

        let bad = dir.join("settings.json");
        std::fs::write(&bad, "{ \"mcpServers\": { oops } }").unwrap();
        let e = edit_file(&bad, Format::McpServers, EXE, true).unwrap_err();
        assert!(e.contains("settings.json isn't plain JSON"), "{e}");
        assert_eq!(std::fs::read_to_string(&bad).unwrap(), "{ \"mcpServers\": { oops } }");
        assert!(!backup_path(&bad).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn claude_codes_lock_is_respected() {
        let dir = scratch("lock");
        let file = dir.join(".claude.json");
        std::fs::write(&file, "{\"numStartups\": 1}").unwrap();
        let lock = dir.join(".claude.json.lock");
        std::fs::create_dir(&lock).unwrap();
        // Held by a live Claude Code: Yap waits, then gives up untouched.
        let busy = with_lock(&file, Duration::from_millis(300), Duration::from_secs(60), || Ok(()));
        assert!(busy.unwrap_err().contains("saving its settings"));
        assert!(lock.is_dir());
        // Abandoned (older than the stale limit): taken, used, released.
        let out = with_lock(&file, Duration::from_secs(2), Duration::ZERO, || Ok(42));
        assert_eq!(out, Ok(42));
        assert!(!lock.exists());
        assert!(edit_file(&file, Format::ClaudeCode, EXE, true).unwrap());
        assert!(!lock.exists());
        assert_eq!(parsed(&std::fs::read_to_string(&file).unwrap())["numStartups"], 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apps_are_found_added_and_removed_in_a_stand_in_profile() {
        let root = scratch("profile");
        let b = Bases::under(root.clone());
        // Claude: the MSIX build and a classic folder both here.
        let msix = b.local.join("Packages").join("Claude_pzs8sxrjxfjjc");
        std::fs::create_dir_all(msix.join("LocalCache").join("Roaming").join("Claude")).unwrap();
        std::fs::create_dir_all(b.appdata.join("Claude")).unwrap();
        std::fs::write(b.appdata.join("Claude").join("claude_desktop_config.json"), CLAUDE).unwrap();
        // Cursor installed with no config yet; VS Code not installed.
        std::fs::create_dir_all(root.join(".cursor")).unwrap();
        // Gemini with a file Yap can't read.
        std::fs::create_dir_all(root.join(".gemini")).unwrap();
        std::fs::write(root.join(".gemini").join("settings.json"), "{ // hi\n}").unwrap();
        // Yap's exe has to exist for an entry to count as working.
        let exe_path = root.join("yap.exe");
        std::fs::write(&exe_path, "").unwrap();
        let exe = exe_path.to_string_lossy().into_owned();

        let rows = overview_in(&b, &exe);
        let row = |rows: &Value, id: &str| rows["clients"].as_array().unwrap().iter().find(|c| c["id"] == id).unwrap().clone();
        assert_eq!(row(&rows, "claude")["status"], "available");
        assert_eq!(row(&rows, "claude")["paths"].as_array().unwrap().len(), 2);
        assert_eq!(row(&rows, "cursor")["status"], "available");
        assert_eq!(row(&rows, "vscode")["status"], "missing");
        assert!(row(&rows, "gemini")["problem"].as_str().unwrap().contains("isn't plain JSON"));
        assert!(rows["commandLine"].as_str().unwrap().ends_with("yap.exe\" mcp"));
        assert_eq!(parsed(rows["snippet"].as_str().unwrap())["mcpServers"]["yap"]["args"], json!(["mcp"]));

        let claude = change_in("claude", true, &b, &exe).unwrap();
        assert_eq!(claude["status"], "added");
        assert_eq!(claude["elsewhere"], false);
        let classic = std::fs::read_to_string(b.appdata.join("Claude").join("claude_desktop_config.json")).unwrap();
        assert_eq!(parsed(&classic)["mcpServers"]["filesystem"]["command"], "npx");
        let packaged = msix.join("LocalCache").join("Roaming").join("Claude").join("claude_desktop_config.json");
        assert_eq!(parsed(&std::fs::read_to_string(&packaged).unwrap())["mcpServers"]["yap"]["command"], exe.as_str());

        assert_eq!(change_in("cursor", true, &b, &exe).unwrap()["status"], "added");
        assert!(change_in("gemini", true, &b, &exe).unwrap_err().contains("isn't plain JSON"));
        assert!(change_in("vscode", true, &b, &exe).unwrap_err().contains("isn't installed"));
        assert!(change_in("nope", true, &b, &exe).is_err());

        // Yap moved away: the entry no longer runs, and the row says so.
        std::fs::remove_file(&exe_path).unwrap();
        assert_eq!(row(&overview_in(&b, &exe), "claude")["status"], "outdated");
        std::fs::write(&exe_path, "").unwrap();

        let removed = change_in("claude", false, &b, &exe).unwrap();
        assert_eq!(removed["status"], "available");
        assert_eq!(std::fs::read_to_string(b.appdata.join("Claude").join("claude_desktop_config.json")).unwrap(), CLAUDE);
        let _ = std::fs::remove_dir_all(&root);
    }
}

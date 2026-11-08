//! **Connectors (preview)**: Yap as an MCP client. The person adds a server
//! at a link (Streamable HTTP, or the older HTTP+SSE, with a token or
//! headers) or a program on this PC (stdio, its exact command line shown
//! first), and Yap keeps its tools, each Always allow / Ask first / Off,
//! for Chat and the Voice Assistant to use (wave 4; nothing offers them to
//! a model yet). How it works: docs/architecture/integrations.md,
//! "Connectors (preview)".
//!
//! - `store.rs` connectors.json · `cache.rs` connector_tools.json ·
//!   `vault.rs` the secrets, in Credential Manager;
//! - `manager.rs` one connection each: start, health, idle stop;
//! - `local.rs` programs (their own kill-on-close job) · `remote.rs` links
//!   (through `net.rs`, every request a receipt);
//! - `names.rs` / `schema.rs` / `results.rs` what a model would see ·
//!   `call.rs` one call (timeout, progress, stop);
//! - `fake.rs` fake servers for the tests.
//!
//! **The switch**: everything here is behind `connectors_preview` (config,
//! off by default; a debug build also takes `YAP_CONNECTORS=1`, as the e2e
//! suite does). Off, no command reads or writes a file, starts a program
//! or makes a request: [`connectors_list`] answers `enabled: false` and the
//! rest refuse.
//!
//! **Never another way in**: only these commands add a connector (Settings,
//! wave 5); Chat has no tool for it, a page beside the chat has no Yap IPC,
//! and the deep link takes none.

pub mod cache;
pub mod call;
#[cfg(test)]
mod fake;
pub mod local;
pub mod manager;
pub mod names;
pub mod remote;
pub mod results;
pub mod schema;
pub mod store;
#[cfg(test)]
mod tests;
pub mod vault;

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use rmcp::model::PaginatedRequestParams;
use rmcp::service::{Peer, RoleClient};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter};

use cache::{Diff, Kind, Listing, Server};
use manager::{Manager, Status};
use store::{AuthKind, Connector, EnvVar, Header, Setting, Transport};

/// Any change to the connectors (every window).
pub const CHANGED: &str = "yap-connectors-changed";
/// A connector's status changed: `{id, state, line, …}`.
pub const STATUS: &str = "yap-connector-status";
/// A listing waits at most this long for each page.
const LIST_TIMEOUT: Duration = Duration::from_secs(30);

/// Whether the connectors preview is on: `connectors_preview` in
/// config.json, or (a debug build) `YAP_CONNECTORS=1`.
pub fn enabled() -> bool {
    #[cfg(test)]
    {
        tests::switch()
    }
    #[cfg(not(test))]
    {
        env_on() || crate::config::load().connectors_preview
    }
}

/// `YAP_CONNECTORS=1` in a debug build (the e2e suite, a developer).
#[cfg_attr(test, allow(dead_code))]
fn env_on() -> bool {
    cfg!(debug_assertions) && std::env::var("YAP_CONNECTORS").is_ok_and(|v| v.trim() == "1")
}

const OFF: &str = "Connectors aren't switched on in this Yap yet.";

fn require() -> Result<(), String> {
    if enabled() {
        Ok(())
    } else {
        Err(OFF.into())
    }
}

static APP: OnceLock<AppHandle> = OnceLock::new();

/// Remember the app for events (setup). Nothing is read or started.
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

fn changed() {
    if let Some(app) = APP.get() {
        let _ = app.emit(CHANGED, ());
    }
}

/// The status event's payload.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusView {
    pub id: String,
    #[serde(flatten)]
    pub status: Status,
    pub line: String,
}

impl StatusView {
    fn of(id: &str, status: Status) -> StatusView {
        StatusView { id: id.to_string(), line: status.line(), status }
    }
}

/// A manager with real connections and the real clock, its status changes
/// told to every window.
fn new_manager() -> Manager {
    let notify: manager::Notify = Arc::new(|id: &str, s: &Status| {
        if let Some(app) = APP.get() {
            let _ = app.emit(STATUS, StatusView::of(id, s.clone()));
        }
    });
    let limits = manager::Limits { start_remote: start_limit(manager::Limits::default().start_remote), ..Default::default() };
    Manager::new(Arc::new(manager::SystemClock), limits, manager::connect_real(), notify)
}

#[cfg(not(test))]
static MANAGER: OnceLock<Manager> = OnceLock::new();

/// This run's manager. Its idle sweep starts with it. (Each unit test
/// thread has its own, as it has its own store.)
pub fn manager() -> &'static Manager {
    #[cfg(test)]
    {
        thread_local! {
            static M: &'static Manager = Box::leak(Box::new(new_manager()));
        }
        M.with(|m| *m)
    }
    #[cfg(not(test))]
    {
        MANAGER.get_or_init(|| {
            tauri::async_runtime::spawn(async {
                loop {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    manager().sweep().await;
                }
            });
            new_manager()
        })
    }
}

/// A test run's shorter start limit for links (`YAP_E2E_CONNECTOR_START_MS`,
/// debug builds in test mode), else `normal`.
fn start_limit(normal: Duration) -> Duration {
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() {
        if let Some(ms) = std::env::var("YAP_E2E_CONNECTOR_START_MS").ok().and_then(|v| v.trim().parse::<u64>().ok()) {
            return Duration::from_millis(ms.max(100));
        }
    }
    normal
}

/// Quit: every connection stopped, within 3 s (the jobs end programs
/// anyway). Nothing if nothing ever connected.
pub fn shutdown() {
    #[cfg(not(test))]
    if let Some(m) = MANAGER.get() {
        tauri::async_runtime::block_on(async {
            let _ = tokio::time::timeout(Duration::from_secs(3), m.stop_all()).await;
        });
    }
}

// ---- what the page gets ----------------------------------------------------------------------

/// One tool as listed, with its setting.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolView {
    pub name: String,
    /// What a model would call it (`names.rs`).
    pub wire: String,
    pub title: Option<String>,
    pub description: String,
    pub kind: Kind,
    pub setting: Setting,
    pub tokens: usize,
    pub flag: Option<String>,
    pub skipped: Option<String>,
}

/// A connector as the page shows it (never a secret: which there are).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub enabled: bool,
    /// "remote" | "local".
    pub kind: &'static str,
    /// The link, or the command line.
    pub address: String,
    pub auth: AuthKind,
    pub headers: Vec<String>,
    pub env: Vec<EnvVar>,
    /// A token, a header's value or a secret variable is kept.
    pub has_secrets: bool,
    pub status: StatusView,
    pub tools: Vec<ToolView>,
    /// What its offered tools cost a model each step.
    pub tokens: usize,
    pub server: Option<Server>,
    #[serde(rename = "where")]
    pub where_: store::Where,
    pub added: u64,
}

fn view(c: &Connector) -> View {
    let listing = cache::get(&c.id);
    let mut map = names::Map::default();
    if let Some(l) = &listing {
        map.add(&c.id, &c.slug, l.tools.iter().map(|t| t.name.as_str()));
    }
    let tools: Vec<ToolView> = listing
        .iter()
        .flat_map(|l| l.tools.iter())
        .map(|t| ToolView {
            wire: map.wire_name(&c.id, &t.name).unwrap_or_default().to_string(),
            name: t.name.clone(),
            title: t.title.clone(),
            description: t.description.clone(),
            kind: t.kind,
            setting: c.tools.get(&t.name).map_or_else(|| cache::default_setting(t), |s| s.setting),
            tokens: t.tokens,
            flag: t.flag.clone(),
            skipped: t.skipped.clone(),
        })
        .collect();
    let (kind, address, headers, env) = match &c.transport {
        Transport::Remote { url, headers, .. } => ("remote", url.clone(), headers.iter().map(|h| h.name.clone()).collect(), Vec::new()),
        Transport::Local { resolved, args, env, .. } => ("local", local::command_line(std::path::Path::new(resolved), args), Vec::new(), env.clone()),
    };
    let status = if c.enabled { manager().status(&c.id) } else { Status::Off };
    View {
        id: c.id.clone(),
        name: c.name.clone(),
        slug: c.slug.clone(),
        enabled: c.enabled,
        kind,
        address,
        auth: c.auth.kind,
        headers,
        env,
        has_secrets: !vault::load(&c.id).is_empty(),
        status: StatusView::of(&c.id, match status {
            // The tool count is the listing's.
            Status::Connected { .. } => Status::Connected { tools: tools.iter().filter(|t| t.skipped.is_none()).count() },
            s => s,
        }),
        tokens: listing.as_ref().map_or(0, |l| l.tokens(&c.tools)),
        server: listing.map(|l| l.server),
        tools,
        where_: c.where_,
        added: c.added,
    }
}

/// `connectors_list`'s answer.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListView {
    pub enabled: bool,
    pub connectors: Vec<View>,
}

// ---- adding one --------------------------------------------------------------------------------

/// A header or a variable as given.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pair {
    pub name: String,
    #[serde(default)]
    pub value: String,
    /// A variable kept in the vault (unset: by its name).
    #[serde(default)]
    pub secret: Option<bool>,
}

/// A connector to add: a link (`url`) or a program (`command`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spec {
    pub name: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub headers: Vec<Pair>,
    /// The link is an older HTTP+SSE stream.
    #[serde(default)]
    pub sse: bool,
    #[serde(default)]
    pub fallback_sse: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<Pair>,
    /// The fingerprint of the command line the person saw ([`connector_preview`]).
    #[serde(default)]
    pub approved: Option<String>,
    /// A call's timeout (seconds).
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

/// What the person is shown before adding: a program's command line and
/// warnings, or what a link will see.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewView {
    pub kind: &'static str,
    /// A program's (`local::Preview`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program: Option<local::Preview>,
    /// A link's host, and where it is ("pc" | "network" | "internet").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<crate::privacy::net::Place>,
    pub warnings: Vec<String>,
}

/// A variable whose name says it's a secret (TOKEN, KEY, SECRET, PASSWORD,
/// PAT). Pure.
pub fn secret_name(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    ["TOKEN", "KEY", "SECRET", "PASSWORD", "PASSWD", "PAT", "CREDENTIAL", "AUTH"].iter().any(|w| n.split(['_', '-']).any(|part| part == *w) || n.ends_with(w))
}

fn env_pairs(env: &[Pair]) -> Result<Vec<(String, Option<String>, bool)>, String> {
    let mut out = Vec::new();
    for p in env {
        let name = p.name.trim();
        if name.is_empty() || name.contains(['=', '\0']) {
            return Err(format!("{name:?} isn't a variable's name."));
        }
        if name.to_ascii_uppercase().starts_with("YAP_") {
            return Err("Variables starting YAP_ are Yap's own.".into());
        }
        let secret = p.secret.unwrap_or_else(|| secret_name(name));
        out.push((name.to_string(), (!secret).then(|| p.value.clone()), secret));
    }
    Ok(out)
}

fn check_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Give the connector a name.".into());
    }
    Ok(name.chars().take(60).collect())
}

/// What adding `spec` would do ([`PreviewView`]).
pub fn preview(spec: &Spec) -> Result<PreviewView, String> {
    if let Some(command) = spec.command.as_deref().filter(|c| !c.trim().is_empty()) {
        let env: Vec<(String, Option<String>)> = env_pairs(&spec.env)?.into_iter().map(|(n, v, _)| (n, v)).collect();
        let p = local::preview(command, &spec.args, &env)?;
        return Ok(PreviewView { kind: "local", warnings: p.warnings.clone(), program: Some(p), host: None, place: None });
    }
    let url = spec.url.as_deref().ok_or_else(|| "Give the server's address, or a program to run.".to_string())?;
    let (u, place) = remote::check_url(url)?;
    let host = crate::privacy::net::host_of(&u);
    let mut warnings = vec![format!("{host} sees what Yap asks it, and what it returns goes to the model you chat with.")];
    if spec.sse {
        warnings.push("This uses MCP's older HTTP+SSE transport.".into());
    }
    Ok(PreviewView { kind: "remote", program: None, host: Some(host), place: Some(place), warnings })
}

/// Add `spec` (a program only with the fingerprint its preview gave), its
/// secrets to the vault. Not connected yet.
pub fn add(spec: &Spec) -> Result<Connector, String> {
    let name = check_name(&spec.name)?;
    let mut secrets = vault::Secrets::default();
    let (transport, auth) = if let Some(command) = spec.command.as_deref().filter(|c| !c.trim().is_empty()) {
        let env = env_pairs(&spec.env)?;
        let shown: Vec<(String, Option<String>)> = env.iter().map(|(n, v, _)| (n.clone(), v.clone())).collect();
        let p = local::preview(command, &spec.args, &shown)?;
        if spec.approved.as_deref() != Some(p.fingerprint.as_str()) {
            return Err("Yap shows a program's command before it first runs: look at it again, then add it.".into());
        }
        for (name, _, secret) in &env {
            if *secret {
                let value = spec.env.iter().find(|e| e.name.trim() == name).map(|e| e.value.clone()).unwrap_or_default();
                secrets.env.insert(name.clone(), value);
            }
        }
        let env = env.into_iter().map(|(name, value, secret)| EnvVar { name, value, secret }).collect();
        (Transport::Local { command: command.trim().to_string(), resolved: p.resolved, args: spec.args.clone(), env, approved: p.fingerprint }, AuthKind::None)
    } else {
        let url = spec.url.as_deref().ok_or_else(|| "Give the server's address, or a program to run.".to_string())?;
        let (u, _) = remote::check_url(url)?;
        if let Some(f) = &spec.fallback_sse {
            let (fu, _) = remote::check_url(f)?;
            if fu.origin() != u.origin() {
                return Err("The older transport's address has to be on the same server.".into());
            }
        }
        let mut headers = Vec::new();
        for h in &spec.headers {
            let n = h.name.trim();
            if n.is_empty() {
                continue;
            }
            if n.eq_ignore_ascii_case("authorization") && spec.token.as_deref().is_some_and(|t| !t.trim().is_empty()) {
                return Err("Give the token or an Authorization header, not both.".into());
            }
            secrets.headers.insert(n.to_string(), h.value.trim().to_string());
            headers.push(Header { name: n.to_string() });
        }
        // The headers are checked now, not at the first start.
        let pairs: Vec<(String, String)> = secrets.headers.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        remote::Http::new(&u, crate::privacy::net::Place::Internet, "A connector".into(), None, &pairs)?;
        let token = spec.token.as_deref().map(str::trim).filter(|t| !t.is_empty());
        if let Some(t) = token {
            if t.contains(['\r', '\n']) {
                return Err("That token has characters a header can't carry.".into());
            }
            secrets.token = Some(t.to_string());
        }
        let auth = if token.is_some() {
            AuthKind::Bearer
        } else if !headers.is_empty() {
            AuthKind::Header
        } else {
            AuthKind::None
        };
        (Transport::Remote { url: u.to_string(), fallback_sse: spec.fallback_sse.clone(), sse: spec.sse, headers }, auth)
    };
    let mut c = store::add(store::New { name, transport, auth })?;
    if let Some(t) = spec.timeout_secs {
        c = store::update(&c.id, |c| {
            c.timeout_secs = Some(t.clamp(1, call::MAX_TIMEOUT.as_secs()));
            c.clone()
        })?;
    }
    if let Err(e) = vault::save(&c.id, &secrets) {
        let _ = store::remove(&c.id);
        return Err(e);
    }
    tracing::info!(connector = %c.id, kind = if c.transport.is_local() { "local" } else { "remote" }, auth = ?c.auth.kind, "connectors: added");
    Ok(c)
}

/// Connector `c`'s tools listed now (all pages).
async fn list(peer: &Peer<RoleClient>, server: Server) -> Result<Listing, String> {
    let mut all: Vec<Value> = Vec::new();
    let mut cursor: Option<String> = None;
    let mut ttl = None;
    for _ in 0..50 {
        let page = tokio::time::timeout(LIST_TIMEOUT, peer.list_tools(Some(PaginatedRequestParams::default().with_cursor(cursor.clone()))))
            .await
            .map_err(|_| "It didn't list its tools in time.".to_string())?
            .map_err(|_| "It wouldn't list its tools.".to_string())?;
        let v = serde_json::to_value(&page).unwrap_or(Value::Null);
        if ttl.is_none() {
            ttl = v.get("ttlMs").and_then(Value::as_u64);
        }
        all.extend(v.get("tools").and_then(Value::as_array).cloned().unwrap_or_default());
        cursor = v.get("nextCursor").and_then(Value::as_str).map(str::to_string);
        if cursor.is_none() || all.len() > cache::MAX_TOOLS {
            break;
        }
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    Ok(cache::listing_of(&all, server, ttl, now))
}

/// Connector `id`'s tools listed and its settings brought up to them
/// (`fresh`: a new connection first, as Try again).
pub async fn refresh(id: &str, fresh: bool) -> Result<Diff, String> {
    let c = store::get(id).ok_or("There's no such connector.")?;
    let m = manager();
    if fresh {
        m.reset(id).await;
    }
    let (peer, server) = m.peer(&c).await?;
    let listing = match list(&peer, server).await {
        Ok(l) => l,
        Err(e) => {
            m.broke(id).await;
            return Err(e);
        }
    };
    let offered = listing.tools.iter().filter(|t| t.skipped.is_none()).count();
    let diff = store::update(id, |c| cache::apply(&mut c.tools, &listing))?;
    cache::put(id, listing);
    m.listed(id, offered).await;
    tracing::info!(connector = id, tools = offered, added = diff.added.len(), changed = diff.changed.len(), removed = diff.removed.len(), "connectors: tools listed");
    changed();
    Ok(diff)
}

/// Connector `c`'s connection for a call: up already, or started — and
/// then listed again first (a changed tool drops back to Ask first before
/// any call: the rug-pull check). (Only a debug build calls tools until
/// Chat's loop does, wave 4.)
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub async fn ensure(c: &Connector) -> Result<Peer<RoleClient>, String> {
    let m = manager();
    if m.is_up(&c.id) {
        let (peer, _) = m.peer(c).await?;
        return Ok(peer);
    }
    refresh(&c.id, false).await?;
    let (peer, _) = m.peer(c).await?;
    Ok(peer)
}

// ---- commands ------------------------------------------------------------------------------------

/// Slow work (Credential Manager, file checks) off the async runtime —
/// inline in a unit test, whose stores are the test thread's own.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    #[cfg(test)]
    {
        f()
    }
    #[cfg(not(test))]
    {
        tauri::async_runtime::spawn_blocking(f).await.map_err(|_| "Yap couldn't do that just now.".to_string())?
    }
}

/// Every connector, with its status and tools (`enabled: false`, and
/// nothing read, while the preview is off).
#[tauri::command]
pub async fn connectors_list() -> ListView {
    if !enabled() {
        return ListView { enabled: false, connectors: Vec::new() };
    }
    ListView { enabled: true, connectors: store::list().iter().map(view).collect() }
}

/// What adding `spec` would run or reach, before anything is saved.
#[tauri::command]
pub async fn connector_preview(spec: Spec) -> Result<PreviewView, String> {
    require()?;
    blocking(move || preview(&spec)).await
}

/// Add a connector (a program only with its preview's fingerprint), then
/// connect and list its tools. A connector that doesn't connect is still
/// added, its status saying why.
#[tauri::command]
pub async fn connector_add(spec: Spec) -> Result<View, String> {
    require()?;
    let c = blocking(move || add(&spec)).await?;
    changed();
    let _ = refresh(&c.id, true).await;
    Ok(view(&store::get(&c.id).unwrap_or(c)))
}

/// Remove a connector: its connection stopped, its tools and secrets gone.
#[tauri::command]
pub async fn connector_remove(id: String) -> Result<(), String> {
    require()?;
    manager().forget(&id).await;
    if store::remove(&id)?.is_none() {
        return Err("There's no such connector.".into());
    }
    cache::forget(&id);
    vault::delete(&id);
    tracing::info!(connector = %id, "connectors: removed");
    changed();
    Ok(())
}

/// Turn a connector on or off everywhere (off: its connection stopped).
#[tauri::command]
pub async fn connector_set_enabled(id: String, enabled: bool) -> Result<View, String> {
    require()?;
    let c = store::update(&id, |c| {
        c.enabled = enabled;
        c.clone()
    })?;
    if enabled {
        manager().reset(&id).await;
    } else {
        manager().stop(&id, true).await;
    }
    changed();
    Ok(view(&c))
}

/// Try again: a fresh connection, its tools listed again; what changed.
#[tauri::command]
pub async fn connector_refresh_tools(id: String) -> Result<RefreshView, String> {
    require()?;
    let diff = refresh(&id, true).await;
    let c = store::get(&id).ok_or("There's no such connector.")?;
    match diff {
        Ok(diff) => Ok(RefreshView { connector: view(&c), diff }),
        Err(e) => Err(e),
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshView {
    pub connector: View,
    pub diff: Diff,
}

/// Set one tool's setting: "allow" | "ask" | "off" (a tool that can delete
/// is never Always allow).
#[tauri::command]
pub async fn connector_set_tool(id: String, tool: String, setting: String) -> Result<View, String> {
    require()?;
    let setting = Setting::parse(&setting).ok_or("That isn't a setting.")?;
    let listing = cache::get(&id).ok_or("Its tools haven't been listed yet.")?;
    let t = listing.tools.iter().find(|t| t.name == tool).ok_or("It has no such tool.")?;
    if setting == Setting::Allow && t.kind == Kind::Deletes {
        return Err("A tool that can delete always asks first.".into());
    }
    if setting != Setting::Off && t.skipped.is_some() {
        return Err("Yap can't offer that tool: its inputs couldn't be made safe for the models.".into());
    }
    let hash = t.hash.clone();
    let c = store::update(&id, |c| {
        c.tools.insert(tool.clone(), store::ToolSetting { setting, hash });
        c.clone()
    })?;
    changed();
    Ok(view(&c))
}

/// Show details: the server, the address, a program's last stderr lines
/// (memory only).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Details {
    pub server: Option<Server>,
    pub address: String,
    pub stderr: Vec<String>,
    pub listed_at: Option<u64>,
}

#[tauri::command]
pub async fn connector_details(id: String) -> Result<Details, String> {
    require()?;
    let c = store::get(&id).ok_or("There's no such connector.")?;
    let v = view(&c);
    let listing = cache::get(&id);
    Ok(Details { server: v.server, address: v.address, stderr: manager().stderr_tail(&id, 40), listed_at: listing.map(|l| l.listed_at) })
}

/// A tool called directly, for the tests and a debug check (debug builds
/// only: in a release build only Chat calls tools, through its consent).
/// Yap's tool-result JSON.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn connector_call_tool(id: String, tool: String, args: Option<Value>, timeout_ms: Option<u64>) -> Result<Value, String> {
    require()?;
    call_tool(&id, &tool, &args.unwrap_or(Value::Null), timeout_ms.map(Duration::from_millis)).await
}

/// One call to connector `id`'s `tool` (Off and unknown tools refused),
/// through the receipts as the connector's. Wave 4's tool loop puts its
/// consent before this.
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub async fn call_tool(id: &str, tool: &str, args: &Value, timeout: Option<Duration>) -> Result<Value, String> {
    let c = store::get(id).ok_or("There's no such connector.")?;
    let peer = ensure(&c).await?;
    let c = store::get(id).ok_or("There's no such connector.")?;
    match c.tools.get(tool) {
        None => return Err("It has no such tool.".into()),
        Some(s) if s.setting == Setting::Off => return Err("That tool is off.".into()),
        Some(_) => {}
    }
    let timeout = timeout.unwrap_or_else(|| call::timeout_for(c.timeout_secs));
    let started = std::time::Instant::now();
    let display = format!("{} · {tool}", c.kind());
    let out = call::call(&peer, tool, args, timeout, &call::Stop::new()).await;
    let ms = started.elapsed().as_millis() as u64;
    match out {
        Ok(shaped) => {
            tracing::info!(connector = id, tool, ms, success = shaped.success, chars = shaped.text.len(), "connectors: tool called");
            Ok(shaped.to_value(&display))
        }
        Err(e) => {
            tracing::info!(connector = id, tool, ms, outcome = e.word(), "connectors: tool call failed");
            if e == call::CallError::Lost {
                manager().broke(id).await;
            }
            Ok(serde_json::json!({ "success": false, "error": e.words(&c.kind()), "data": Value::Null, "displayText": display }))
        }
    }
}

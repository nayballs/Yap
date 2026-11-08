//! **Connectors (preview)**: Yap as an MCP client. The person adds a server
//! at a link (Streamable HTTP, or the older HTTP+SSE, with a token or
//! headers) or a program on this PC (stdio, its exact command line shown
//! first), and Yap keeps its tools, each Always allow / Ask first / Off,
//! for Chat and the Voice Assistant to use (`chat/tools/connectors.rs`:
//! offered to the strong cloud models, asked about by `consent.rs`, sized
//! by `budget.rs`). How it works: docs/architecture/integrations.md,
//! "Connectors (preview)" and "Connectors in Chat".
//!
//! - `store.rs` connectors.json · `cache.rs` connector_tools.json ·
//!   `vault.rs` the secrets, in Credential Manager;
//! - `manager.rs` one connection each: start, health, idle stop;
//! - `local.rs` programs (their own kill-on-close job) · `remote.rs` links
//!   (through `net.rs`, every request a receipt);
//! - `oauth.rs` signing in to a link in the browser (the MCP authorization
//!   spec: discovery, the client, PKCE on the loopback, tokens in the
//!   vault) · `refresh.rs` one refresh at a time across Yaps;
//! - `names.rs` / `schema.rs` / `results.rs` what a model sees ·
//!   `call.rs` one call (timeout, progress, stop);
//! - `consent.rs` whether a call runs, asks first or is refused ·
//!   `budget.rs` how many tools a model gets, and tool search past that;
//! - `catalog.rs` the bundled list Settings' Discover shows · `import.rs`
//!   a setup read from Claude Desktop or pasted;
//! - `chats.rs` each chat's own switches (the composer's Connectors ▸,
//!   `connector_chats.json`);
//! - `privacy.rs` what Delete some of your data, Download your data and
//!   Report a problem do with them (wave 8);
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

pub mod budget;
pub mod cache;
pub mod call;
pub mod catalog;
pub mod chats;
pub mod consent;
#[cfg(test)]
pub(crate) mod fake;
pub mod import;
pub mod local;
pub mod manager;
pub mod names;
pub mod oauth;
pub mod privacy;
pub mod refresh;
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
use store::{AuthKind, Connector, EnvVar, Header, Origin, Setting, Transport};

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

/// The status event's payload: the connection's state, its line, and for a
/// link that signs in in the browser its sign-in (`signIn`:
/// "needs-sign-in" | "signing-in" | "signed-in" | "expired").
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusView {
    pub id: String,
    #[serde(flatten)]
    pub status: Status,
    pub line: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sign_in: Option<oauth::SignInState>,
}

impl StatusView {
    fn of(id: &str, status: Status) -> StatusView {
        let c = store::get(id);
        StatusView::with(id, c.as_ref(), status)
    }

    fn with(id: &str, c: Option<&Connector>, status: Status) -> StatusView {
        let sign_in = c.and_then(|c| oauth::state_of(c, &status));
        let line = c.zip(sign_in).and_then(|(c, s)| oauth::line_of(c, s)).unwrap_or_else(|| status.line());
        StatusView { id: id.to_string(), line, status, sign_in }
    }
}

/// Tell every window connector `id`'s status now (its sign-in changed).
pub(crate) fn tell(id: &str) {
    if let Some(app) = APP.get() {
        let _ = app.emit(STATUS, StatusView::of(id, status_if_started(id)));
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

/// Connector `id`'s status, without starting the manager when nothing has
/// connected yet in this run (then it waits to start: Idle). For Report a
/// problem's counts and the status event.
pub(crate) fn status_if_started(id: &str) -> Status {
    #[cfg(test)]
    {
        manager().status(id)
    }
    #[cfg(not(test))]
    {
        MANAGER.get().map_or(Status::Idle, |m| m.status(id))
    }
}

/// Stop connector `id`'s connection, if this run's manager has started at
/// all (with the preview off it never has: nothing to stop, nothing started).
pub(crate) async fn stop_if_started(id: &str) {
    #[cfg(test)]
    {
        manager().stop(id, false).await;
    }
    #[cfg(not(test))]
    {
        if let Some(m) = MANAGER.get() {
            m.stop(id, false).await;
        }
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
    /// When one of its tools was last called (unix seconds).
    pub last_used: Option<u64>,
    /// Where it came from: "custom" | "catalog" | "import".
    pub origin: String,
    /// The catalogue entry it was added from (Discover's ✓, its logo).
    pub catalog: Option<String>,
    /// A link's browser sign-in (`None`: it doesn't sign in that way).
    pub sign_in: Option<oauth::SignInInfo>,
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
    let secrets = vault::load(&c.id);
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
        has_secrets: !secrets.is_empty(),
        sign_in: oauth::info(c, &status, &secrets),
        status: StatusView::with(&c.id, Some(c), match status {
            // The tool count is the listing's.
            Status::Connected { .. } => Status::Connected { tools: tools.iter().filter(|t| t.skipped.is_none()).count() },
            s => s,
        }),
        tokens: listing.as_ref().map_or(0, |l| l.tokens(&c.tools)),
        server: listing.map(|l| l.server),
        tools,
        where_: c.where_,
        added: c.added,
        last_used: c.last_used,
        origin: c.origin.kind.clone(),
        catalog: catalog_id(&c.origin).map(str::to_string),
    }
}

/// The catalogue entry's id an origin names (`{kind: "catalog", catalog}`).
fn catalog_id(origin: &Origin) -> Option<&str> {
    (origin.kind == "catalog").then(|| origin.extra.get("catalog").and_then(Value::as_str)).flatten()
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
    /// Added from this catalogue entry (Discover): its address and transport
    /// are the entry's (the person gives only the token, the name, and the
    /// address where the entry lets them), and it's named by the entry in
    /// the receipts.
    #[serde(default)]
    pub catalog: Option<String>,
}

/// `spec` as the catalogue entry it names adds it (the entry's address,
/// transport and name; the person's token, their address only where the
/// entry says so, a program's arguments as they edited them), or as it is.
fn resolved(spec: &Spec) -> Result<(Spec, Option<&'static catalog::Entry>), String> {
    let Some(id) = spec.catalog.as_deref() else { return Ok((spec.clone(), None)) };
    let e = catalog::get().get(id).ok_or("That connector isn't in Yap's list any more.")?;
    let name = Some(spec.name.trim()).filter(|n| !n.is_empty()).map_or_else(|| e.name.clone(), str::to_string);
    let out = match e.sign_in {
        catalog::SignIn::Local => Spec {
            name,
            command: Some(spec.command.clone().filter(|c| !c.trim().is_empty()).or_else(|| e.command.clone()).unwrap_or_default()),
            args: if spec.args.is_empty() { e.args.clone() } else { spec.args.clone() },
            env: spec.env.clone(),
            approved: spec.approved.clone(),
            catalog: spec.catalog.clone(),
            ..Default::default()
        },
        catalog::SignIn::Oauth | catalog::SignIn::Token => {
            let url = if e.url_editable { spec.url.clone().filter(|u| !u.trim().is_empty()).or_else(|| e.url.clone()) } else { e.url.clone() };
            let token = spec.token.clone().filter(|t| !t.trim().is_empty());
            if e.sign_in == catalog::SignIn::Token && token.is_none() {
                return Err(format!("Paste your {} token first.", e.name));
            }
            Spec { name, url, token, fallback_sse: e.fallback_sse.clone(), catalog: spec.catalog.clone(), ..Default::default() }
        }
    };
    Ok((out, Some(e)))
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
    let (spec, _) = resolved(spec)?;
    let spec = &spec;
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
    add_as(spec, None)
}

/// [`add`], marked as coming from `origin` (an import); one from the
/// catalogue is marked so by itself.
pub fn add_as(spec: &Spec, origin: Option<Origin>) -> Result<Connector, String> {
    let (spec, entry) = resolved(spec)?;
    let spec = &spec;
    let origin = match entry {
        Some(e) => {
            let mut extra = serde_json::Map::new();
            extra.insert("catalog".into(), Value::from(e.id.clone()));
            extra.insert("catalogVersion".into(), Value::from(catalog::get().version));
            Some(Origin { kind: "catalog".into(), extra })
        }
        None => origin,
    };
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
    if let Some(o) = origin {
        c = store::update(&c.id, |c| {
            c.origin = o;
            c.clone()
        })?;
    }
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
    let diff = store::update(id, |c| {
        let diff = cache::apply(&mut c.tools, &listing);
        // A catalogue entry's new tools may start stricter (Stripe's money
        // tools Off).
        if let Some(e) = catalog::of(&c.origin) {
            catalog::start_settings(e, &diff.added, &mut c.tools);
        }
        diff
    })?;
    cache::put(id, listing);
    m.listed(id, offered).await;
    tracing::info!(connector = id, tools = offered, added = diff.added.len(), changed = diff.changed.len(), removed = diff.removed.len(), "connectors: tools listed");
    changed();
    Ok(diff)
}

/// Connector `c`'s connection for a call: up already, or started — and
/// then listed again first (a changed tool drops back to Ask first before
/// any call: the rug-pull check).
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

/// Remove a connector: its connection stopped, a browser sign-in revoked
/// (as signing out does, for at most 10 s), its tools and secrets gone.
#[tauri::command]
pub async fn connector_remove(id: String) -> Result<(), String> {
    require()?;
    oauth::cancel(&id);
    manager().forget(&id).await;
    if vault::load(&id).grant.is_some() {
        let _ = tokio::time::timeout(Duration::from_secs(10), oauth::sign_out(&id)).await;
    }
    if store::remove(&id)?.is_none() {
        return Err("There's no such connector.".into());
    }
    cache::forget(&id);
    vault::delete(&id);
    // No chat keeps it switched off: a later connector may get its id.
    chats::connector_removed(&id);
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
        oauth::cancel(&id);
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

/// Where a connector's tools may be used, as changed on its page: Chat, the
/// Voice Assistant, during meetings (also behind `connectors_in_meetings`).
/// The phone stays as it is (read only, wave 6), and plan chats too.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WherePatch {
    #[serde(default)]
    pub chat: Option<bool>,
    #[serde(default)]
    pub voice: Option<bool>,
    #[serde(default)]
    pub meetings: Option<bool>,
    /// From the phone (Yap Link): its reads only (wave 6).
    #[serde(default)]
    pub phone: Option<bool>,
}

impl WherePatch {
    /// `w` with this patch's changes. Pure.
    pub fn apply(self, mut w: store::Where) -> store::Where {
        if let Some(v) = self.chat {
            w.chat = v;
        }
        if let Some(v) = self.voice {
            w.voice = v;
        }
        if let Some(v) = self.meetings {
            w.meetings = v;
        }
        if let Some(v) = self.phone {
            w.phone = v;
        }
        w
    }
}

/// **Where Yap uses it** (a connector's page): Chat, the Voice Assistant,
/// during meetings, and from the phone (its reads only, wave 6).
#[tauri::command]
pub async fn connector_set_where(id: String, places: WherePatch) -> Result<View, String> {
    require()?;
    let c = store::update(&id, |c| {
        c.where_ = places.apply(c.where_);
        c.clone()
    })?;
    tracing::info!(connector = %id, chat = c.where_.chat, voice = c.where_.voice, meetings = c.where_.meetings, phone = c.where_.phone, "connectors: where it's used changed");
    changed();
    Ok(view(&c))
}

/// How the Chat model gets the connectors' tools (Settings' line): every
/// tool as it is, or tool search past its share of the context.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetView {
    /// The Chat model (none set up: `None`).
    pub model: Option<crate::chat::chats::ChatModel>,
    /// That model gets connectors at all (the strong cloud models).
    pub gets: bool,
    /// What the tools Chat may use cost each step, and the model's share.
    pub tokens: u64,
    pub cap: u64,
    /// "all" | "search" (`find_tools` / `use_tool`) | "deferred"
    /// (Anthropic's own tool search) | "none".
    pub mode: &'static str,
}

/// The tokens of the tools Chat may use: each enabled connector's in Chat
/// (`where.chat`), as its view counts them. Pure.
pub fn chat_tokens(views: &[View]) -> u64 {
    views.iter().filter(|v| v.enabled && v.where_.chat).map(|v| v.tokens as u64).sum()
}

/// **Which way the Chat model gets them**: its share (`budget::cap`, its
/// context and a free plan's rate) against what the tools cost
/// (`budget::mode`), as a Chat turn decides it.
#[tauri::command]
pub async fn connectors_budget() -> Result<BudgetView, String> {
    require()?;
    let cfg = crate::config::load();
    let tokens = chat_tokens(&store::list().iter().map(view).collect::<Vec<_>>());
    let on_device = crate::ai::ai_model::OnDevice::here(&cfg);
    let Ok(target) = crate::ai::ai_model::target(&cfg, crate::ai::ai_model::Scope::Chat, &on_device) else {
        return Ok(BudgetView { model: None, gets: false, tokens, cap: 0, mode: "none" });
    };
    let strong = target.tools && crate::chat::documents::rich::strong(&target.provider, &target.model);
    let gets = crate::chat::tools::connectors::may(&target.provider, strong);
    let (context, _) = crate::chat::budget::context_window(&cfg, &target.provider, &target.model, &target.base_url).await;
    let cap = budget::cap(context, crate::chat::budget::rate_cap(&target.provider));
    let messages_api = crate::ai::llm::anthropic::speaks_messages(&target.provider, &target.model);
    let mode = if !gets {
        "none"
    } else {
        match budget::mode(tokens, cap, messages_api) {
            budget::Mode::All => "all",
            budget::Mode::Search => "search",
            budget::Mode::Deferred => "deferred",
        }
    };
    Ok(BudgetView { model: Some(target.chat_model()), gets, tokens, cap, mode })
}

/// **One connector in a chat's + menu** (Connectors ▸, wave 6): its name,
/// its mark (the catalogue entry's logo and colour; none: its initial) and
/// whether it's on in that chat.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatConnector {
    pub id: String,
    pub name: String,
    pub on: bool,
    pub logo: Option<String>,
    pub color: Option<String>,
}

/// A chat's connectors, for its + menu.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatConnectors {
    /// Connectors ▸ shows: the preview on, Chat's model gets connectors
    /// (in its tool loop, or on your Claude plan through Yap's endpoint),
    /// and one at least is enabled and allowed in Chat (on here or not).
    pub shown: bool,
    pub connectors: Vec<ChatConnector>,
}

/// The connectors a chat's + menu lists: each enabled one allowed in Chat
/// (`where.chat`), in the order they were added, on unless `off` says.
/// Pure.
pub fn chat_menu(list: &[Connector], off: &std::collections::BTreeSet<String>, gets: bool) -> ChatConnectors {
    let connectors: Vec<ChatConnector> = list
        .iter()
        .filter(|c| c.enabled && c.where_.chat)
        .map(|c| {
            let entry = catalog::of(&c.origin);
            ChatConnector {
                id: c.id.clone(),
                name: c.name.clone(),
                on: !off.contains(&c.id),
                logo: entry.and_then(|e| e.logo.clone()),
                color: entry.and_then(|e| e.color.clone()),
            }
        })
        .collect();
    ChatConnectors { shown: gets && !connectors.is_empty(), connectors }
}

/// **A chat's connectors** (its + menu's Connectors ▸): `conversationId`'s
/// switches (none: a new chat, every one on — its page keeps what it
/// switches until its first question). With the preview off, nothing is
/// read and nothing shows.
#[tauri::command]
pub async fn connector_chat_list(conversation_id: Option<u64>) -> ChatConnectors {
    if !enabled() {
        return ChatConnectors::default();
    }
    let cfg = crate::config::load();
    let on_device = crate::ai::ai_model::OnDevice::here(&cfg);
    // (Your Claude plan gets them through Yap's run endpoint, wave 7: its
    // chats honour these switches too.)
    let gets = crate::ai::ai_model::target(&cfg, crate::ai::ai_model::Scope::Chat, &on_device).is_ok_and(|t| {
        let strong = t.tools && crate::chat::documents::rich::strong(&t.provider, &t.model);
        crate::chat::tools::connectors::may(&t.provider, strong) || t.provider == crate::ai::claude_plan::PROVIDER
    });
    chat_menu(&store::list(), &chats::off(conversation_id), gets)
}

/// **Switch a connector on or off in one chat** (its + menu): kept in
/// connector_chats.json by the conversation's id. A new chat's switches go
/// with its first question instead (`chat_send`'s `connectorsOff`).
#[tauri::command]
pub async fn connector_chat_set(conversation_id: u64, id: String, on: bool) -> Result<ChatConnectors, String> {
    require()?;
    if store::get(&id).is_none() {
        return Err("There's no such connector.".into());
    }
    chats::set(conversation_id, &id, on)?;
    tracing::info!(conversation = conversation_id, connector = %id, on, "connectors: switched in one chat");
    Ok(connector_chat_list(Some(conversation_id)).await)
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

// ---- the catalogue and imports ------------------------------------------------------------------

/// A catalogue entry for Discover, and the connector added from it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntryView {
    #[serde(flatten)]
    pub entry: catalog::Entry,
    /// The person's connector added from it (Discover's ✓).
    pub added: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogView {
    pub version: u32,
    pub connectors: Vec<CatalogEntryView>,
    pub later: Vec<catalog::Later>,
}

/// **Discover's list**: the bundled catalogue (nothing fetched), each entry
/// with the connector added from it, and the "Coming later" cards.
#[tauri::command]
pub async fn connector_catalog() -> Result<CatalogView, String> {
    require()?;
    let c = catalog::get();
    let have = store::list();
    Ok(CatalogView {
        version: c.version,
        connectors: c
            .connectors
            .iter()
            .map(|e| CatalogEntryView { added: have.iter().find(|x| catalog_id(&x.origin) == Some(e.id.as_str())).map(|x| x.id.clone()), entry: e.clone() })
            .collect(),
        later: c.later.clone(),
    })
}

/// **Import from Claude Desktop / Paste a setup**, step one: each server in
/// it shown whole (a program's command line and warnings, a link's host),
/// no secret in it. Claude Desktop's files are only read.
#[tauri::command]
pub async fn connectors_import_scan(source: import::Source) -> Result<import::ScanView, String> {
    require()?;
    blocking(move || import::scan(&source)).await
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportFailed {
    pub name: String,
    pub why: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportView {
    pub added: Vec<View>,
    pub failed: Vec<ImportFailed>,
}

/// Step two: the servers the person ticked, read again from the setup and
/// added (each program only with the fingerprint they saw; secrets from
/// the setup straight to the vault), then connected and listed.
#[tauri::command]
pub async fn connectors_import(source: import::Source, picks: Vec<import::Pick>) -> Result<ImportView, String> {
    require()?;
    let from = source.word();
    let (specs, missing) = blocking(move || import::specs_for(&source, &picks)).await?;
    let mut added = Vec::new();
    let mut failed: Vec<ImportFailed> = missing.into_iter().map(|name| ImportFailed { name, why: "It isn't in that setup any more.".into() }).collect();
    for spec in specs {
        let name = spec.name.clone();
        let mut extra = serde_json::Map::new();
        extra.insert("from".into(), Value::from(from));
        let origin = Origin { kind: "import".into(), extra };
        match blocking(move || add_as(&spec, Some(origin))).await {
            Ok(c) => {
                changed();
                let _ = refresh(&c.id, true).await;
                added.push(view(&store::get(&c.id).unwrap_or(c)));
            }
            Err(why) => failed.push(ImportFailed { name, why }),
        }
    }
    tracing::info!(added = added.len(), failed = failed.len(), from, "connectors: imported");
    changed();
    Ok(ImportView { added, failed })
}

// ---- signing in ------------------------------------------------------------------------------------

/// How `connector_sign_in` ended (a failure is its `Err`, in plain words).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    SignedIn,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInView {
    pub outcome: Outcome,
    pub connector: View,
}

/// The person's browser on `link` (`shell/browsers.rs`, as every link
/// button: a test run keeps it for the suite instead).
fn open_in_browser(link: &str) -> Result<(), String> {
    let app = APP.get().ok_or("Yap isn't ready to open your browser yet.")?;
    crate::shell::commands::open_link(app, link)
}

/// Sign connector `id` in, opening the browser with `open`: the states told
/// as it goes; signed in, a fresh connection and its tools listed.
pub async fn sign_in_with(id: &str, open: oauth::Open<'_>) -> Result<SignInView, String> {
    let c = store::get(id).ok_or("There's no such connector.")?;
    oauth::can_sign_in(&c)?;
    let (ticket, before) = oauth::begin(id);
    if let Some(other) = before.filter(|o| o != id) {
        tell(&other);
    }
    tell(id);
    let out = oauth::sign_in(&c, &ticket, open).await;
    oauth::end(&ticket);
    let outcome = match out {
        Ok(()) => {
            manager().reset(id).await;
            if refresh(id, true).await.is_err() {
                tracing::info!(connector = id, "connectors: signed in, but its tools didn't list yet");
            }
            Outcome::SignedIn
        }
        Err(oauth::Stop::Cancelled) => {
            tracing::info!(connector = id, "connectors: sign-in cancelled");
            Outcome::Cancelled
        }
        Err(oauth::Stop::Failed(why)) => {
            tracing::info!(connector = id, "connectors: sign-in didn't finish");
            tell(id);
            changed();
            return Err(why);
        }
    };
    tell(id);
    changed();
    let c = store::get(id).ok_or("There's no such connector.")?;
    Ok(SignInView { outcome, connector: view(&c) })
}

/// **Sign in in the browser**: the server's sign-in found, Yap's client
/// picked (or registered), the person's browser opened on it; answers once
/// it's done (`signed-in`), cancelled (`cancelled`) or failed (`Err`, in
/// plain words). `yap-connector-status` says "signing-in" meanwhile.
#[tauri::command]
pub async fn connector_sign_in(id: String) -> Result<SignInView, String> {
    require()?;
    sign_in_with(&id, &open_in_browser).await
}

/// Stop waiting for connector `id`'s sign-in in the browser.
#[tauri::command]
pub async fn connector_sign_in_cancel(id: String) -> Result<bool, String> {
    require()?;
    let was = oauth::cancel(&id);
    tell(&id);
    Ok(was)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedOutView {
    /// The server confirmed the grant is revoked (else: disconnect Yap on
    /// the service's own page too).
    pub confirmed: bool,
    pub connector: View,
}

/// **Sign out**: the connection stopped, the grant revoked where the server
/// offers it, the tokens gone; "needs-sign-in" after.
#[tauri::command]
pub async fn connector_sign_out(id: String) -> Result<SignedOutView, String> {
    require()?;
    let c = store::get(&id).ok_or("There's no such connector.")?;
    if c.auth.kind != AuthKind::Oauth {
        return Err("This connector isn't signed in through the browser.".into());
    }
    oauth::cancel(&id);
    manager().stop(&id, false).await;
    let confirmed = oauth::sign_out(&id).await;
    tell(&id);
    changed();
    let c = store::get(&id).ok_or("There's no such connector.")?;
    Ok(SignedOutView { confirmed, connector: view(&c) })
}

/// **The person's own client** for a server Yap has no registration with:
/// its id (in connectors.json) and secret (in the vault); an empty id
/// forgets it. Used at the next sign-in, first.
#[tauri::command]
pub async fn connector_set_client(id: String, client_id: String, secret: Option<String>) -> Result<View, String> {
    require()?;
    blocking({
        let id = id.clone();
        move || oauth::set_client(&id, &client_id, secret.as_deref())
    })
    .await?;
    changed();
    let c = store::get(&id).ok_or("There's no such connector.")?;
    Ok(view(&c))
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
/// through the receipts as the connector's: the debug command's way.
/// Chat's tool loop has its own ([`call_for_chat`], after its consent).
#[cfg(debug_assertions)]
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
    let display = format!("{} · {tool}", c.kind());
    match call_logged(&c, &peer, tool, args, timeout).await {
        Ok(shaped) => Ok(shaped.to_value(&display)),
        Err(e) => Ok(serde_json::json!({ "success": false, "error": e, "data": Value::Null, "displayText": display })),
    }
}

/// **A call from Chat's tool loop** (`chat/tools/connectors.rs`, after its
/// consent): connector `c`'s `tool` on `peer`, within the connector's own
/// timeout. Its answer shaped, or why there isn't one in plain words.
pub async fn call_for_chat(c: &Connector, peer: &Peer<RoleClient>, tool: &str, args: &Value) -> Result<results::Shaped, String> {
    call_logged(c, peer, tool, args, call::timeout_for(c.timeout_secs)).await
}

/// A call under way, for the log: dropped before it ends (■ Stop, a newer
/// question) it says the call was stopped — the server is told by
/// `call.rs` (`notifications/cancelled`).
struct Calling<'a> {
    id: &'a str,
    tool: &'a str,
    started: std::time::Instant,
    done: bool,
}

impl Drop for Calling<'_> {
    fn drop(&mut self) {
        if !self.done {
            let ms = self.started.elapsed().as_millis() as u64;
            tracing::info!(connector = self.id, tool = self.tool, ms, outcome = "cancelled", "connectors: tool call stopped");
        }
    }
}

/// One call, logged ([`Calling`]): the connector's id, the tool's name, the
/// outcome, the time and the answer's length — never its arguments or its
/// answer. A lost connection is the manager's to start again.
async fn call_logged(c: &Connector, peer: &Peer<RoleClient>, tool: &str, args: &Value, timeout: Duration) -> Result<results::Shaped, String> {
    let signed = oauth::state_of(c, &manager().status(&c.id));
    let mut calling = Calling { id: &c.id, tool, started: std::time::Instant::now(), done: false };
    let out = call::call(peer, tool, args, timeout, &call::Stop::new()).await;
    calling.done = true;
    let ms = calling.started.elapsed().as_millis() as u64;
    // Settings' "Last used" (told when it changed: at most once a minute).
    if store::used(&c.id) {
        changed();
    }
    match out {
        Ok(shaped) => {
            tracing::info!(connector = %c.id, tool, ms, success = shaped.success, chars = shaped.text.len(), "connectors: tool called");
            Ok(shaped)
        }
        Err(e) => {
            tracing::info!(connector = %c.id, tool, ms, outcome = e.word(), "connectors: tool call failed");
            // The server refused the sign-in on this call (it ended, or it
            // wants more access): said so; not the connection's failure.
            let now = store::get(&c.id);
            if let Some(words) = now.as_ref().and_then(|n| sign_in_trouble(n, signed)) {
                if now.as_ref().is_some_and(|n| n.auth.expired) {
                    manager().stop(&c.id, false).await;
                }
                return Err(words);
            }
            if e == call::CallError::Lost {
                manager().broke(&c.id).await;
            }
            Err(e.words(&c.kind()))
        }
    }
}

/// **Always allow, from Chat's card**: connector `id`'s `tool` set to
/// Always allow, pinned to the definition it was shown for (`hash`) — never
/// a tool that can delete, nor one whose definition changed since. Every
/// window hears of it.
pub fn allow_always(id: &str, tool: &str, hash: &str) -> Result<(), String> {
    let listing = cache::get(id).ok_or("Its tools haven't been listed yet.")?;
    let t = listing.tools.iter().find(|t| t.name == tool).ok_or("It has no such tool.")?;
    if t.kind == Kind::Deletes {
        return Err("A tool that can delete always asks first.".into());
    }
    if t.hash != hash {
        return Err("The tool changed since Yap asked.".into());
    }
    store::update(id, |c| {
        c.tools.insert(tool.to_string(), store::ToolSetting { setting: Setting::Allow, hash: hash.to_string() });
    })?;
    tracing::info!(connector = id, tool, "connectors: a tool set to Always allow from Chat");
    changed();
    Ok(())
}

/// The preview's switch for a unit test on this thread (and a fresh store).
#[cfg(test)]
pub(crate) fn switch_for_test(on: bool) {
    tests::set_switch(on);
    store::reset_for_test();
}

/// A call failed because of its sign-in, judged by the state before it
/// (`was`) and now: what to say. Pure but for the one sign-in in flight.
fn sign_in_trouble(now: &Connector, was: Option<oauth::SignInState>) -> Option<String> {
    let state = oauth::state_of(now, &Status::Idle)?;
    if Some(state) == was || state == oauth::SignInState::SignedIn {
        return None;
    }
    oauth::line_of(now, state)
}

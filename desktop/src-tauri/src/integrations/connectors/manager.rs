//! **One connection per connector**, made when it's needed (a call, a
//! listing, Settings opening its page) and let go when it isn't:
//!
//! - **Started** within [`Limits::start_local`] (30 s: `npx`'s first
//!   download) or [`Limits::start_remote`] (15 s), both MCP eras through
//!   rmcp's Auto lifecycle (`server/discover`, else `initialize`). Two
//!   callers at once wait for the one start.
//! - **Stopped when idle**: [`Limits::idle`] (10 min) after its last use, a
//!   program's stdin closed, 3 s, then its job ended; a link just dropped
//!   (MCP is stateless now). Every connection stops at Quit
//!   ([`Manager::stop_all`], `shutdown_cleanup`); the jobs end programs
//!   however Yap ends.
//! - **Failures**: a program that died starts again at its next use; three
//!   failures (a start that failed, a connection that broke) within
//!   [`Limits::window`] (10 min) mark it [`Status::Stopped`] until Try
//!   again ([`Manager::reset`]).
//! - **Statuses** in plain words ([`Status::line`]), each change told
//!   (`yap-connector-status`).
//!
//! Its clock is a [`Clock`] (a fake one in tests: the lifecycle without
//! waiting) and its way to connect a [`Connect`] (fake servers in tests).
//! The log has the connector's id, outcomes and timings; never its name, an
//! address's path, a command line, stderr or a secret.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::future::BoxFuture;
use rmcp::model::{ClientCapabilities, ClientConfig, Implementation, ProtocolVersion};
use rmcp::service::{ClientInitializeError, ClientLifecycleMode, Peer, RoleClient, RunningService};
use rmcp::ClientHandler;
use serde::Serialize;

use super::cache::Server;
use super::local::{self, Program, Ring};
use super::remote::{self, Http, HttpError};
use super::store::{AuthKind, Connector, Transport};
use super::vault;

/// Yap as an MCP client: its name, and none of what it can't do yet
/// (sampling, roots, elicitation forms).
#[derive(Debug, Clone, Default)]
pub struct Handler;

impl ClientHandler for Handler {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::new(ClientCapabilities::default(), Implementation::new("yap", env!("CARGO_PKG_VERSION")).with_title("Yap"))
    }
}

pub type Service = RunningService<RoleClient, Handler>;

/// Both MCP eras: `server/discover` (2026-07-28), else `initialize`.
pub fn lifecycle() -> ClientLifecycleMode {
    ClientLifecycleMode::Auto { preferred_versions: vec![ProtocolVersion::V_2026_07_28], legacy_version: Some(ProtocolVersion::LATEST_WITH_INITIALIZE) }
}

/// rmcp's client over `transport`.
pub async fn serve<T, E, A>(transport: T) -> Result<Service, Box<ClientInitializeError>>
where
    T: rmcp::transport::IntoTransport<RoleClient, E, A>,
    E: std::error::Error + Send + Sync + 'static,
{
    rmcp::service::serve_client_with_lifecycle(Handler, transport, lifecycle()).await.map_err(Box::new)
}

/// A connection that's up.
pub struct Live {
    service: Service,
    pub server: Server,
}

impl Live {
    pub fn new(service: Service) -> Live {
        let info = service.peer().peer_info();
        let server = Server {
            name: info.as_ref().and_then(|i| i.server_info.as_ref()).map(|s| s.name.chars().take(80).collect()).unwrap_or_default(),
            version: info.as_ref().and_then(|i| i.server_info.as_ref()).map(|s| s.version.chars().take(40).collect()).unwrap_or_default(),
            protocol: info.as_ref().map(|i| i.protocol_version.as_str().to_string()).unwrap_or_default(),
        };
        Live { service, server }
    }

    pub fn peer(&self) -> Peer<RoleClient> {
        self.service.peer().clone()
    }

    /// The program ended, or the link broke.
    pub fn closed(&self) -> bool {
        self.service.peer().is_transport_closed()
    }

    /// Stop it: a program's stdin closed, 3 s, then its job ended.
    pub async fn stop(mut self) {
        let _ = self.service.close_with_timeout(Duration::from_secs(5)).await;
    }
}

/// Why a connection didn't start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartError {
    /// The server wants a browser sign-in (`oauth.rs`), or the one there was
    /// can't be used any more.
    NeedsSignIn,
    /// Anything else, in plain words.
    Failed(String),
}

/// How the manager connects (the real servers, or fakes in tests).
pub type Connect = Arc<dyn Fn(Connector, Arc<Mutex<Ring>>, Duration) -> BoxFuture<'static, Result<Live, StartError>> + Send + Sync>;

/// The manager's time, in milliseconds.
pub trait Clock: Send + Sync {
    fn now(&self) -> u64;
}

/// The real one: milliseconds since Yap started, never going back.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> u64 {
        static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
        START.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64
    }
}

/// The limits of a connection's life.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub start_local: Duration,
    pub start_remote: Duration,
    pub idle: Duration,
    pub failures: usize,
    pub window: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            start_local: Duration::from_secs(30),
            start_remote: Duration::from_secs(15),
            idle: Duration::from_secs(10 * 60),
            failures: 3,
            window: Duration::from_secs(10 * 60),
        }
    }
}

/// Where a connection is, as the person sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Status {
    /// Turned off.
    Off,
    /// Not running: it starts when it's needed.
    Idle,
    Starting,
    Connected { tools: usize },
    /// The server wants a browser sign-in.
    NeedsSignIn,
    /// The last start failed (it's tried again when next needed).
    Failed { why: String },
    /// Failed [`Limits::failures`] times in [`Limits::window`]: not tried
    /// again until Try again.
    Stopped,
}

impl Status {
    /// The status line.
    pub fn line(&self) -> String {
        match self {
            Status::Off => "Off".into(),
            Status::Idle => "Starts when Chat needs it".into(),
            Status::Starting => "Starting…".into(),
            Status::Connected { tools: 1 } => "Connected · 1 tool".into(),
            Status::Connected { tools } => format!("Connected · {tools} tools"),
            Status::NeedsSignIn => "Needs sign-in. Sign in in your browser, or use a token if it offers one.".into(),
            Status::Failed { why } => why.clone(),
            Status::Stopped => "Stopped by itself 3 times in 10 minutes. Try again when it's fixed.".into(),
        }
    }

    fn word(&self) -> &'static str {
        match self {
            Status::Off => "off",
            Status::Idle => "idle",
            Status::Starting => "starting",
            Status::Connected { .. } => "connected",
            Status::NeedsSignIn => "needs-sign-in",
            Status::Failed { .. } => "failed",
            Status::Stopped => "stopped",
        }
    }
}

/// Failures within the window, and whether there are too many. Pure.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Health {
    failures: Vec<u64>,
}

impl Health {
    /// A failure at `now`; whether that's one too many (`limits`).
    pub fn failed(&mut self, now: u64, limits: &Limits) -> bool {
        self.failures.push(now);
        self.trim(now, limits);
        self.failures.len() >= limits.failures
    }

    /// Too many failures in the window as of `now`.
    pub fn stopped(&mut self, now: u64, limits: &Limits) -> bool {
        self.trim(now, limits);
        self.failures.len() >= limits.failures
    }

    fn trim(&mut self, now: u64, limits: &Limits) {
        let window = limits.window.as_millis() as u64;
        self.failures.retain(|&t| now.saturating_sub(t) < window);
    }

    pub fn clear(&mut self) {
        self.failures.clear();
    }
}

/// Whether a connection used last at `last_used` is idle at `now`. Pure.
pub fn idle(last_used: u64, now: u64, limits: &Limits) -> bool {
    now.saturating_sub(last_used) >= limits.idle.as_millis() as u64
}

/// One connector's connection and its state.
struct Slot {
    live: Option<Live>,
    status: Status,
    health: Health,
    last_used: u64,
    stderr: Arc<Mutex<Ring>>,
}

/// Told of every status change: (connector id, status).
pub type Notify = Arc<dyn Fn(&str, &Status) + Send + Sync>;

/// Every connector's connection.
pub struct Manager {
    slots: Mutex<HashMap<String, Arc<tokio::sync::Mutex<Slot>>>>,
    /// The last status of each, readable without waiting for a start.
    statuses: Mutex<HashMap<String, Status>>,
    clock: Arc<dyn Clock>,
    limits: Limits,
    connect: Connect,
    notify: Notify,
}

impl Manager {
    pub fn new(clock: Arc<dyn Clock>, limits: Limits, connect: Connect, notify: Notify) -> Manager {
        Manager { slots: Mutex::new(HashMap::new()), statuses: Mutex::new(HashMap::new()), clock, limits, connect, notify }
    }

    fn slot(&self, id: &str) -> Arc<tokio::sync::Mutex<Slot>> {
        self.slots
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .entry(id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(Slot { live: None, status: Status::Idle, health: Health::default(), last_used: 0, stderr: Arc::default() })))
            .clone()
    }

    fn set(&self, id: &str, slot: &mut Slot, status: Status) {
        if slot.status == status {
            return;
        }
        tracing::info!(connector = id, status = status.word(), "connectors: status");
        slot.status = status.clone();
        self.statuses.lock().unwrap_or_else(|p| p.into_inner()).insert(id.to_string(), status.clone());
        (self.notify)(id, &status);
    }

    /// Connector `id`'s status now (a connector never started: idle).
    pub fn status(&self, id: &str) -> Status {
        self.statuses.lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned().unwrap_or(Status::Idle)
    }

    /// Connector `c`'s connection, started if it isn't up ([module
    /// docs](self)); `Err`: why it isn't, in plain words.
    pub async fn peer(&self, c: &Connector) -> Result<(Peer<RoleClient>, Server), String> {
        let slot = self.slot(&c.id);
        let mut s = slot.lock().await;
        if !c.enabled {
            self.set(&c.id, &mut s, Status::Off);
            return Err("This connector is turned off.".into());
        }
        let now = self.clock.now();
        if s.live.as_ref().is_some_and(Live::closed) {
            // It ended by itself (a crash, the link gone): a failure.
            let gone = s.live.take();
            drop(gone);
            tracing::warn!(connector = %c.id, "connectors: a connection ended by itself");
            if s.health.failed(now, &self.limits) {
                self.set(&c.id, &mut s, Status::Stopped);
            }
        }
        if s.health.stopped(now, &self.limits) {
            self.set(&c.id, &mut s, Status::Stopped);
            return Err(Status::Stopped.line());
        }
        if let Some(up) = s.live.as_ref().map(|live| (live.peer(), live.server.clone())) {
            s.last_used = now;
            return Ok(up);
        }
        self.set(&c.id, &mut s, Status::Starting);
        let limit = if c.transport.is_local() { self.limits.start_local } else { self.limits.start_remote };
        let started = std::time::Instant::now();
        s.stderr.lock().unwrap_or_else(|p| p.into_inner()).clear();
        match (self.connect)(c.clone(), s.stderr.clone(), limit).await {
            Ok(live) => {
                tracing::info!(connector = %c.id, ms = started.elapsed().as_millis() as u64, protocol = %live.server.protocol, "connectors: connected");
                let out = (live.peer(), live.server.clone());
                s.live = Some(live);
                s.last_used = self.clock.now();
                let tools = super::cache::get(&c.id).map_or(0, |l| l.tools.iter().filter(|t| t.skipped.is_none()).count());
                self.set(&c.id, &mut s, Status::Connected { tools });
                Ok(out)
            }
            Err(e) => {
                let now = self.clock.now();
                let too_many = s.health.failed(now, &self.limits);
                let (status, why) = match e {
                    StartError::NeedsSignIn => (Status::NeedsSignIn, Status::NeedsSignIn.line()),
                    StartError::Failed(why) => (Status::Failed { why: why.clone() }, why),
                };
                tracing::warn!(connector = %c.id, ms = started.elapsed().as_millis() as u64, status = status.word(), "connectors: couldn't connect");
                self.set(&c.id, &mut s, if too_many { Status::Stopped } else { status });
                Err(why)
            }
        }
    }

    /// The number of tools the connection's status says (after a listing).
    pub async fn listed(&self, id: &str, tools: usize) {
        let slot = self.slot(id);
        let mut s = slot.lock().await;
        if s.live.is_some() {
            self.set(id, &mut s, Status::Connected { tools });
        }
    }

    /// A call's connection broke: counted as a failure, let go.
    pub async fn broke(&self, id: &str) {
        let slot = self.slot(id);
        let mut s = slot.lock().await;
        if let Some(live) = s.live.take() {
            live.stop().await;
        }
        let now = self.clock.now();
        let status = if s.health.failed(now, &self.limits) { Status::Stopped } else { Status::Idle };
        self.set(id, &mut s, status);
    }

    /// Try again: failures forgotten, the connection let go (the next use
    /// starts it afresh).
    pub async fn reset(&self, id: &str) {
        let slot = self.slot(id);
        let mut s = slot.lock().await;
        s.health.clear();
        if let Some(live) = s.live.take() {
            live.stop().await;
        }
        self.set(id, &mut s, Status::Idle);
    }

    /// Stop connector `id`'s connection (turned off, removed); `off`: say Off.
    pub async fn stop(&self, id: &str, off: bool) {
        let slot = self.slot(id);
        let mut s = slot.lock().await;
        if let Some(live) = s.live.take() {
            live.stop().await;
            tracing::info!(connector = id, "connectors: stopped");
        }
        self.set(id, &mut s, if off { Status::Off } else { Status::Idle });
    }

    /// A removed connector is forgotten.
    pub async fn forget(&self, id: &str) {
        self.stop(id, true).await;
        self.slots.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
        self.statuses.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
    }

    /// Let go of connections idle for [`Limits::idle`], and of ones that
    /// ended by themselves (each a failure). A slot in use is left for the
    /// next sweep.
    pub async fn sweep(&self) {
        let slots: Vec<(String, Arc<tokio::sync::Mutex<Slot>>)> = self.slots.lock().unwrap_or_else(|p| p.into_inner()).iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        for (id, slot) in slots {
            let Ok(mut s) = slot.try_lock() else { continue };
            let now = self.clock.now();
            let Some(live) = &s.live else { continue };
            if live.closed() {
                s.live = None;
                tracing::warn!(connector = %id, "connectors: a connection ended by itself");
                let status = if s.health.failed(now, &self.limits) { Status::Stopped } else { Status::Idle };
                self.set(&id, &mut s, status);
            } else if idle(s.last_used, now, &self.limits) {
                if let Some(live) = s.live.take() {
                    live.stop().await;
                }
                tracing::info!(connector = %id, "connectors: idle, stopped");
                self.set(&id, &mut s, Status::Idle);
            }
        }
    }

    /// Stop every connection (Quit).
    pub async fn stop_all(&self) {
        let slots: Vec<Arc<tokio::sync::Mutex<Slot>>> = self.slots.lock().unwrap_or_else(|p| p.into_inner()).values().cloned().collect();
        let stops = slots.into_iter().map(|slot| async move {
            let live = slot.lock().await.live.take();
            if let Some(live) = live {
                live.stop().await;
            }
        });
        futures_util::future::join_all(stops).await;
    }

    /// Connector `id`'s last stderr lines (a program's; in memory only).
    pub fn stderr_tail(&self, id: &str, lines: usize) -> Vec<String> {
        let slot = self.slots.lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned();
        match slot {
            Some(slot) => match slot.try_lock() {
                Ok(s) => s.stderr.lock().unwrap_or_else(|p| p.into_inner()).tail(lines),
                Err(_) => Vec::new(),
            },
            None => Vec::new(),
        }
    }

    /// Whether connector `id` has a connection up now.
    pub fn is_up(&self, id: &str) -> bool {
        matches!(self.status(id), Status::Connected { .. })
    }
}

// ---- connecting for real --------------------------------------------------------------------

/// The errors inside an initialize error (its transport's, both eras').
fn transport_errors(e: &ClientInitializeError) -> Vec<&(dyn std::error::Error + 'static)> {
    match e {
        ClientInitializeError::TransportError { error, .. } => vec![error.error.as_ref() as &(dyn std::error::Error + 'static)],
        ClientInitializeError::LegacyFallbackFailed { discover, fallback } => {
            let mut v = transport_errors(fallback);
            v.extend(transport_errors(discover));
            v
        }
        _ => Vec::new(),
    }
}

/// The first [`HttpError`] in `e`.
pub(crate) fn http_error_of(e: &ClientInitializeError) -> Option<HttpError> {
    transport_errors(e).into_iter().find_map(|t| remote::http_error_in(t).cloned())
}

/// A remote start's error, as the person is told.
fn remote_error(e: &ClientInitializeError, http: &Http) -> StartError {
    if e.auth_challenge().is_some() && !http.sends_credentials() {
        return StartError::NeedsSignIn;
    }
    if let Some(h) = http_error_of(e) {
        return StartError::Failed(format!("Couldn't connect: {h}"));
    }
    match e {
        ClientInitializeError::JsonRpcError(data) => StartError::Failed(format!("Couldn't connect: {} refused the connection ({}).", http.host(), data.message.chars().take(200).collect::<String>())),
        ClientInitializeError::NoCompatibleProtocolVersion { .. } => StartError::Failed(format!("Couldn't connect: {} speaks a version of MCP Yap doesn't.", http.host())),
        _ => StartError::Failed(format!("Couldn't connect: {} didn't answer as an MCP server does.", http.host())),
    }
}

/// A local start's error.
fn local_error(e: &ClientInitializeError, name: &str) -> StartError {
    match e {
        ClientInitializeError::JsonRpcError(data) => StartError::Failed(format!("Couldn't start: {name} refused ({}).", data.message.chars().take(200).collect::<String>())),
        ClientInitializeError::NoCompatibleProtocolVersion { .. } => StartError::Failed(format!("Couldn't start: {name} speaks a version of MCP Yap doesn't.")),
        _ => StartError::Failed(format!("Couldn't start: {name} stopped before it answered. Show details has what it said.")),
    }
}

/// Whether Streamable HTTP isn't spoken here (try the older transport).
fn not_streamable(e: &ClientInitializeError) -> bool {
    matches!(http_error_of(e), Some(HttpError::NotStreamable { .. }))
}

/// The real way to connect: a program on this PC, or a link (`limit`: the
/// most the whole start may take).
pub fn connect_real() -> Connect {
    Arc::new(|c: Connector, stderr: Arc<Mutex<Ring>>, limit: Duration| Box::pin(connect(c, stderr, limit)))
}

async fn connect(c: Connector, stderr: Arc<Mutex<Ring>>, limit: Duration) -> Result<Live, StartError> {
    let what = match &c.transport {
        Transport::Local { resolved, .. } => std::path::Path::new(resolved).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "the program".into()),
        Transport::Remote { url, .. } => url::Url::parse(url).ok().map(|u| crate::privacy::net::host_of(&u)).unwrap_or_else(|| "the server".into()),
    };
    let local = c.transport.is_local();
    match tokio::time::timeout(limit, start(c, stderr)).await {
        Ok(r) => r.map(Live::new),
        Err(_) if local => Err(StartError::Failed(format!("Couldn't start: {what} didn't answer in {} s.", limit.as_secs()))),
        Err(_) => Err(StartError::Failed(format!("Couldn't connect: no answer from {what} in {} s.", limit.as_secs()))),
    }
}

async fn start(c: Connector, stderr: Arc<Mutex<Ring>>) -> Result<Service, StartError> {
    let secrets = vault::load(&c.id);
    match &c.transport {
        Transport::Local { resolved, args, env, approved, .. } => {
            let names: Vec<(String, Option<String>)> = env.iter().map(|e| (e.name.clone(), if e.secret { None } else { e.value.clone() })).collect();
            let path = Program::check(resolved, args, &names, approved).map_err(StartError::Failed)?;
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let vars: Vec<(String, String)> = env
                .iter()
                .filter_map(|e| if e.secret { secrets.env.get(&e.name).cloned() } else { e.value.clone() }.map(|v| (e.name.clone(), v)))
                .collect();
            let program = Program { path, args: args.clone(), env: vars, cwd: crate::config::data_dir().join("connectors").join(&c.id) };
            let transport = local::spawn(&program, stderr).map_err(StartError::Failed)?;
            serve(transport).await.map_err(|e| local_error(&e, &name))
        }
        Transport::Remote { url, fallback_sse, sse, headers } => {
            let (u, place) = remote::check_url(url).map_err(StartError::Failed)?;
            let token = if c.auth.kind == AuthKind::Bearer { secrets.token.clone() } else { None };
            let sent: Vec<(String, String)> = headers.iter().filter_map(|h| secrets.headers.get(&h.name).map(|v| (h.name.clone(), v.clone()))).collect();
            let mut http = Http::new(&u, place, c.kind(), token, &sent).map_err(StartError::Failed)?;
            if c.auth.kind == AuthKind::Oauth {
                // A browser sign-in's token, refreshed first if it's nearly
                // out: none usable, no start ("Needs sign-in").
                let source = Arc::new(super::oauth::Source::new(&c.id));
                source.bearer().await.map_err(|t| match t {
                    super::oauth::Trouble::SignIn => StartError::NeedsSignIn,
                    super::oauth::Trouble::Failed(why) => StartError::Failed(format!("Couldn't connect: {why}")),
                })?;
                http = http.with_sign_in(source);
            }
            let older = |to: url::Url| {
                let http = http.clone();
                async move {
                    let t = remote::legacy::Legacy::connect(http.clone(), &to, Duration::from_secs(15)).await.map_err(|h| StartError::Failed(format!("Couldn't connect: {h}")))?;
                    rmcp::service::serve_client_with_lifecycle(Handler, t, ClientLifecycleMode::Initialize).await.map_err(|e| remote_error(&e, &http))
                }
            };
            if *sse {
                return older(u).await;
            }
            match serve(remote::transport(http.clone(), &u)).await {
                Ok(s) => Ok(s),
                Err(e) if not_streamable(&e) => {
                    let to = match fallback_sse.as_deref() {
                        Some(f) => remote::check_url(f).map_err(StartError::Failed)?.0,
                        None => u.clone(),
                    };
                    tracing::info!(connector = %c.id, "connectors: no Streamable HTTP here; trying the older HTTP+SSE");
                    older(to).await
                }
                Err(e) => Err(remote_error(&e, &http)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    #[derive(Default)]
    struct FakeClock(AtomicU64);
    impl Clock for FakeClock {
        fn now(&self) -> u64 {
            self.0.load(Ordering::SeqCst)
        }
    }
    impl FakeClock {
        fn advance(&self, d: Duration) {
            self.0.fetch_add(d.as_millis() as u64, Ordering::SeqCst);
        }
    }

    #[test]
    fn three_failures_in_ten_minutes_stop_it_and_older_ones_fall_away() {
        let l = Limits::default();
        let mut h = Health::default();
        let min = 60_000;
        assert!(!h.failed(0, &l));
        assert!(!h.failed(4 * min, &l));
        // The first is 10 minutes old by now: still two.
        assert!(!h.failed(10 * min, &l));
        assert!(h.failed(11 * min, &l), "three within ten minutes");
        assert!(h.stopped(11 * min, &l));
        assert!(!h.stopped(25 * min, &l), "the window moves on");
        h.clear();
        assert!(!h.stopped(25 * min, &l));
        assert!(!idle(0, 9 * min, &l));
        assert!(idle(0, 10 * min, &l));
    }

    #[test]
    fn every_status_has_its_line() {
        assert_eq!(Status::Connected { tools: 12 }.line(), "Connected · 12 tools");
        assert_eq!(Status::Connected { tools: 1 }.line(), "Connected · 1 tool");
        assert_eq!(Status::Idle.line(), "Starts when Chat needs it");
        assert!(Status::Stopped.line().starts_with("Stopped by itself 3 times in 10 minutes"));
        assert!(Status::NeedsSignIn.line().starts_with("Needs sign-in"));
        let v = serde_json::to_value(Status::Failed { why: "Couldn't start: x".into() }).unwrap();
        assert_eq!(v, serde_json::json!({ "state": "failed", "why": "Couldn't start: x" }));
    }

    /// A manager on a fake clock, connecting to `fake.rs`'s servers.
    /// What the manager told, in order.
    type Heard = Arc<Mutex<Vec<(String, Status)>>>;

    fn manager(clock: Arc<FakeClock>, fail: Arc<AtomicU64>) -> (Manager, Heard) {
        let heard = Arc::new(Mutex::new(Vec::new()));
        let h = heard.clone();
        let connect: Connect = Arc::new(move |_c: Connector, _stderr, _limit| {
            let fail = fail.clone();
            Box::pin(async move {
                if fail.load(Ordering::SeqCst) > 0 {
                    fail.fetch_sub(1, Ordering::SeqCst);
                    return Err(StartError::Failed("Couldn't start: fake.exe didn't answer in 30 s.".into()));
                }
                let (service, _fake) = super::super::fake::connected(super::super::fake::Fake::default()).await;
                Ok(Live::new(service))
            })
        });
        let notify: Notify = Arc::new(move |id: &str, s: &Status| h.lock().unwrap().push((id.to_string(), s.clone())));
        (Manager::new(clock, Limits::default(), connect, notify), heard)
    }

    fn connector(id: &str) -> Connector {
        let mut c = serde_json::from_value::<Connector>(serde_json::json!({
            "id": id, "slug": "fake", "name": "Fake",
            "transport": { "kind": "local", "command": "fake", "resolved": "C:\\fake.exe", "args": [] }
        }))
        .unwrap();
        c.enabled = true;
        c
    }

    #[tokio::test]
    async fn the_lifecycle_on_a_fake_clock() {
        let clock = Arc::new(FakeClock::default());
        let fail = Arc::new(AtomicU64::new(0));
        let (m, heard) = manager(clock.clone(), fail.clone());
        let c = connector("c1");
        assert_eq!(m.status("c1"), Status::Idle);

        // Started on first use; the second use shares it.
        let (peer, server) = m.peer(&c).await.unwrap();
        assert_eq!(server.name, "fake");
        assert!(matches!(m.status("c1"), Status::Connected { .. }));
        let (again, _) = m.peer(&c).await.unwrap();
        assert!(!peer.is_transport_closed() && !again.is_transport_closed());

        // Used 9 minutes ago: kept. 10 minutes: let go.
        clock.advance(Duration::from_secs(9 * 60));
        m.sweep().await;
        assert!(m.is_up("c1"));
        clock.advance(Duration::from_secs(60));
        m.sweep().await;
        assert_eq!(m.status("c1"), Status::Idle);
        assert!(peer.is_transport_closed(), "the idle connection is closed");

        // Next use starts it again.
        m.peer(&c).await.unwrap();
        assert!(m.is_up("c1"));

        // Three failed starts in ten minutes: stopped until Try again.
        m.stop("c1", false).await;
        fail.store(3, Ordering::SeqCst);
        for _ in 0..2 {
            let err = m.peer(&c).await.unwrap_err();
            assert!(err.contains("didn't answer"), "{err}");
            assert!(matches!(m.status("c1"), Status::Failed { .. }));
            clock.advance(Duration::from_secs(60));
        }
        m.peer(&c).await.unwrap_err();
        assert_eq!(m.status("c1"), Status::Stopped);
        // Not tried again (the fake would connect now).
        assert!(m.peer(&c).await.unwrap_err().starts_with("Stopped by itself"));
        m.reset("c1").await;
        m.peer(&c).await.unwrap();
        assert!(m.is_up("c1"));

        // Turned off: stopped, and refused.
        let mut off = c.clone();
        off.enabled = false;
        m.stop("c1", true).await;
        assert!(m.peer(&off).await.unwrap_err().contains("turned off"));
        assert_eq!(m.status("c1"), Status::Off);

        // Every change was told.
        let told: Vec<&'static str> = heard.lock().unwrap().iter().map(|(_, s)| s.word()).collect();
        assert_eq!(told[..3], ["starting", "connected", "idle"]);
        assert!(told.contains(&"stopped") && told.ends_with(&["off"]));
    }

    #[tokio::test]
    async fn a_connection_that_ends_by_itself_is_a_failure_and_starts_again() {
        let clock = Arc::new(FakeClock::default());
        let (m, _) = manager(clock.clone(), Arc::new(AtomicU64::new(0)));
        let c = connector("c2");
        let (peer, _) = m.peer(&c).await.unwrap();
        // The fake's `crash` ends its server.
        let r = super::super::call::call(&peer, "crash", &serde_json::json!({}), Duration::from_secs(5), &Default::default()).await;
        assert_eq!(r.unwrap_err(), super::super::call::CallError::Lost);
        for _ in 0..50 {
            if peer.is_transport_closed() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        m.sweep().await;
        assert_eq!(m.status("c2"), Status::Idle);
        let (fresh, _) = m.peer(&c).await.unwrap();
        assert!(!fresh.is_transport_closed());
        m.stop_all().await;
        assert!(fresh.is_transport_closed());
    }
}

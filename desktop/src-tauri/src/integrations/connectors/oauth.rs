//! **Signing in to a connector in the browser** (the MCP authorization
//! spec, revision 2026-07-28; OAuth 2.1 for a native app). Yap's own code on
//! its own reqwest, not rmcp's `auth` feature (which brings another reqwest
//! and its own HTTP), so every request goes through `net.rs` as
//! "Connector sign-in · A connector" and every rule below is Yap's:
//!
//! 1. **Discovery** (`discover.rs`): the server's `401` challenge → its
//!    protected resource metadata (RFC 9728; its `resource` must be this
//!    server) → the authorization server it names → that server's metadata
//!    (RFC 8414 or OpenID; its `issuer` the one named; PKCE with S256, or
//!    Yap refuses).
//! 2. **The client** (`client.rs`), in the spec's order: a pre-registered
//!    one (the person's own, else Yap's from the catalogue) → Yap's client
//!    ID metadata document → dynamic registration (once per issuer, kept) →
//!    ask for a client id.
//! 3. **The browser** (`loopback.rs`): the authorize address with PKCE,
//!    `state`, the resource (RFC 8707) and the scopes (the challenge's, else
//!    the resource's, with `offline_access` when the server lists it, and on
//!    a step-up the ones before and the ones it asked for since), opened in
//!    the person's browser (`shell/browsers.rs`, never a Yap webview); the
//!    answer on the loopback (`127.0.0.1:51439`), its `state` and `iss`
//!    (RFC 9207) checked before the code is used.
//! 4. **Tokens** (`tokens.rs`): the code swapped with the verifier and the
//!    resource; the tokens in the vault ([`Grant`]: bound to their issuer,
//!    resource and client), never in a file or a log. [`access_token`]
//!    refreshes 30 s before expiry (half the life of a token that lasts
//!    under a minute), one refresh at a time across Yaps
//!    (`refresh.rs`, the vault read again under the guard); a refresh
//!    turned down ends the sign-in ("expired"). A `403 insufficient_scope`
//!    records what the server wants ([`step_up`]): "needs more access, sign
//!    in again", once per scope.
//! 5. **Sign-out** ([`sign_out`]): the grant revoked (RFC 7009) where the
//!    server offers it, the tokens gone from the vault; the registration
//!    and the person's own client stay, so signing in again is one click.
//!
//! **The states** ([`SignInState`], on `yap-connector-status` as `signIn`):
//! `needs-sign-in`, `signing-in`, `signed-in`, `expired`. One sign-in at a
//! time ([`begin`]: a new one cancels the one before).
//!
//! **Safety**: the authorization server is one the resource names, its
//! issuer checked twice (its metadata, the answer's `iss`); the loopback
//! takes only `127.0.0.1:<its port>` answers; the access token goes only to
//! the connector's own server (`remote.rs`, same origin; a grant for
//! another resource is never sent), the refresh token only to its issuer's
//! token endpoint. No token, code, verifier, secret or authorize address is
//! logged at any level (a test reads these files for it).

pub mod client;
pub mod discover;
pub mod loopback;
pub mod tokens;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::manager::Status;
use super::refresh;
use super::remote;
use super::store::{self, AuthKind, ClientKind, ClientRef, Connector, Endpoints, OwnClient, Transport};
use super::vault::{self, Grant, Secrets};
use crate::privacy::net::{self, Place};
use client::Pick;
use discover::{same_issuer, split_scopes, Net};
use loopback::{Done, Handback, Listener};
use tokens::{Failure, Method};

/// A token with less than this left is refreshed first (seconds).
pub const MARGIN: i64 = 30;

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

// ---- the states --------------------------------------------------------------------------------

/// Where a connector's browser sign-in is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SignInState {
    /// The server wants a sign-in (or more access: a step-up).
    NeedsSignIn,
    /// The browser is open on it.
    SigningIn,
    SignedIn,
    /// It ended (a refresh turned down, the tokens refused): sign in again.
    Expired,
}

/// Connector `c`'s sign-in state (`status`: its connection's), `None` for
/// one that doesn't sign in in the browser. Pure but for the one sign-in
/// in flight.
pub fn state_of(c: &Connector, status: &Status) -> Option<SignInState> {
    if !matches!(c.transport, Transport::Remote { .. }) {
        return None;
    }
    if signing_in(&c.id) {
        return Some(SignInState::SigningIn);
    }
    match c.auth.kind {
        AuthKind::Oauth if c.auth.expired => Some(SignInState::Expired),
        AuthKind::Oauth if c.auth.signed_in && c.auth.wants.is_empty() => Some(SignInState::SignedIn),
        AuthKind::Oauth => Some(SignInState::NeedsSignIn),
        AuthKind::None if *status == Status::NeedsSignIn => Some(SignInState::NeedsSignIn),
        _ => None,
    }
}

/// The status line a sign-in state says instead of the connection's
/// (`None`: the connection's own). Pure.
pub fn line_of(c: &Connector, state: SignInState) -> Option<String> {
    let host = host_of(c);
    match state {
        SignInState::SigningIn => Some("Signing in… Finish in your browser.".into()),
        SignInState::Expired => Some(format!("Your sign-in to {host} has ended. Sign in again.")),
        SignInState::NeedsSignIn if !c.auth.wants.is_empty() => Some(format!("{host} needs more access for this. Sign in again.")),
        SignInState::NeedsSignIn => Some(Status::NeedsSignIn.line()),
        SignInState::SignedIn => None,
    }
}

/// The connector's server's host ("the server" when it has none).
fn host_of(c: &Connector) -> String {
    match &c.transport {
        Transport::Remote { url, .. } => url::Url::parse(url).ok().map(|u| net::host_of(&u)).unwrap_or_else(|| "the server".into()),
        Transport::Local { .. } => "the program".into(),
    }
}

/// Where the connector's server is, and its address.
fn server_of(c: &Connector) -> Result<(url::Url, Place), String> {
    match &c.transport {
        Transport::Remote { url, .. } => remote::check_url(url),
        Transport::Local { .. } => Err(LOCAL.into()),
    }
}

/// Something changed that the page shows.
fn tell(id: &str) {
    super::tell(id);
}

const LOCAL: &str = "A program on this PC doesn't sign in through the browser: it takes its keys from its variables.";

// ---- one sign-in at a time -------------------------------------------------------------------------

/// A sign-in in flight: which connector, which attempt, and its Cancel.
#[derive(Clone)]
pub struct Ticket {
    pub id: String,
    pub attempt: u64,
    cancelled: Arc<AtomicBool>,
}

impl Ticket {
    pub fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

#[derive(Default)]
struct Flight {
    attempts: u64,
    now: Option<Ticket>,
}

#[cfg(not(test))]
static FLIGHT: Mutex<Option<Flight>> = Mutex::new(None);

#[cfg(not(test))]
fn with_flight<R>(f: impl FnOnce(&mut Flight) -> R) -> R {
    f(FLIGHT.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(Flight::default))
}

#[cfg(test)]
thread_local! {
    static FLIGHT: std::cell::RefCell<Flight> = std::cell::RefCell::new(Flight::default());
}

#[cfg(test)]
fn with_flight<R>(f: impl FnOnce(&mut Flight) -> R) -> R {
    FLIGHT.with(|fl| f(&mut fl.borrow_mut()))
}

/// A new sign-in for connector `id`; the one before (any connector's) is
/// cancelled: which connector that was.
pub fn begin(id: &str) -> (Ticket, Option<String>) {
    with_flight(|f| {
        let before = f.now.take().map(|t| {
            t.cancelled.store(true, Ordering::SeqCst);
            t.id
        });
        f.attempts += 1;
        let t = Ticket { id: id.to_string(), attempt: f.attempts, cancelled: Arc::default() };
        f.now = Some(t.clone());
        (t, before)
    })
}

/// That sign-in is over.
pub fn end(t: &Ticket) {
    with_flight(|f| {
        if f.now.as_ref().is_some_and(|n| n.attempt == t.attempt) {
            f.now = None;
        }
    })
}

/// Cancel connector `id`'s sign-in; whether there was one.
pub fn cancel(id: &str) -> bool {
    with_flight(|f| match f.now.take_if(|n| n.id == id) {
        Some(t) => {
            t.cancelled.store(true, Ordering::SeqCst);
            true
        }
        None => false,
    })
}

/// Whether connector `id`'s sign-in is in flight.
pub fn signing_in(id: &str) -> bool {
    with_flight(|f| f.now.as_ref().is_some_and(|n| n.id == id && !n.cancelled()))
}

// ---- the access token --------------------------------------------------------------------------------

/// Why a connection can't have a token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trouble {
    /// No sign-in, or it ended: sign in (again).
    SignIn,
    /// It couldn't be refreshed now (no answer…): Yap's words.
    Failed(String),
}

/// What to do with the kept grant.
#[derive(Debug, PartialEq, Eq)]
enum Decision {
    Use,
    Refresh,
    /// Unusable and nothing to refresh it with.
    Ended,
}

/// When grant `g`'s access token should be refreshed: [`MARGIN`] before it
/// runs out, or half its life for one that lasts under a minute (so a
/// short-lived token isn't refreshed at every use); 0: never (the server
/// didn't say when it runs out). Pure.
fn use_until(g: &Grant) -> i64 {
    if g.expires == 0 {
        return 0;
    }
    let life = g.expires - g.received;
    let margin = if life > 0 { MARGIN.min(life / 2) } else { MARGIN };
    g.expires - margin
}

/// The kept grant at `now`, the server having just refused `refused` (if
/// it did). A token the server didn't refuse (another Yap's newer one) is
/// used while it lasts. Pure.
fn decide(g: &Grant, now: i64, refused: Option<&str>) -> Decision {
    let refused_this = refused.is_some_and(|r| r == g.access);
    let until = use_until(g);
    if !refused_this && (until == 0 || until > now) {
        Decision::Use
    } else if g.refresh.is_some() {
        Decision::Refresh
    } else {
        Decision::Ended
    }
}

/// The kept grant, if it may be sent to connector `c`'s server (it was
/// issued for that resource).
fn bound_grant(c: &Connector) -> Option<Grant> {
    let grant = vault::load(&c.id).grant?;
    let (server, _) = server_of(c).ok()?;
    let resource = url::Url::parse(&grant.resource).ok()?;
    discover::resource_matches(&resource, &server).then_some(grant)
}

/// A client with its secret and how it authenticates.
struct Owned {
    id: String,
    secret: Option<String>,
    method: Method,
}

impl Owned {
    fn as_client(&self) -> tokens::Client<'_> {
        tokens::Client { id: &self.id, secret: self.secret.as_deref(), method: self.method }
    }
}

/// The client `client_id` is, for connector `c`: its secret (a
/// registration's, the person's own, the catalogue's) and method.
fn client_of(c: &Connector, s: &Secrets, client_id: &str, e: &Endpoints) -> Owned {
    let registration = c.auth.registration.as_ref().filter(|r| r.client_id == client_id && same_issuer(&r.issuer, &e.issuer));
    let (secret, registered) = if let Some(r) = registration {
        (s.registered_secret.clone(), Some(r.auth_method.clone()))
    } else if c.auth.own.as_ref().is_some_and(|o| o.client_id == client_id) {
        (s.own_secret.clone(), None)
    } else if let Some(cc) = client::catalog_client(&e.issuer).filter(|cc| cc.client_id == client_id) {
        (cc.secret.map(str::to_string), None)
    } else {
        // Yap's metadata document: a public client.
        (None, None)
    };
    let method = tokens::method_for(secret.is_some(), &e.auth_methods, registered.as_deref());
    Owned { id: client_id.to_string(), secret, method }
}

/// **An access token for connector `id`**: the kept one while it lasts,
/// else refreshed — one refresh at a time across Yaps, the vault read again
/// once the guard is held. `refused`: the token its server just turned down
/// (a 401): refreshed unless another Yap already has. A refresh turned down
/// ends the sign-in. The token and when to refresh it ([`use_until`]; 0:
/// only once it's refused).
pub async fn access_token(id: &str, refused: Option<&str>) -> Result<(String, i64), Trouble> {
    let c = store::get(id).ok_or(Trouble::SignIn)?;
    let Some(grant) = bound_grant(&c) else {
        no_grant(id);
        return Err(Trouble::SignIn);
    };
    if decide(&grant, now(), refused) == Decision::Use {
        let until = use_until(&grant);
        return Ok((grant.access, until));
    }
    let _guard = refresh::acquire(id).await.map_err(Trouble::Failed)?;
    // Another Yap may have refreshed while this one waited: its tokens.
    let Some(grant) = bound_grant(&c) else {
        no_grant(id);
        return Err(Trouble::SignIn);
    };
    match decide(&grant, now(), refused) {
        Decision::Use => {
            let until = use_until(&grant);
            return Ok((grant.access, until));
        }
        Decision::Ended => {
            ended(id);
            return Err(Trouble::SignIn);
        }
        Decision::Refresh => {}
    }
    let Some(endpoints) = c.auth.server.clone().filter(|e| same_issuer(&e.issuer, &grant.issuer)) else {
        ended(id);
        return Err(Trouble::SignIn);
    };
    let (_, place) = server_of(&c).map_err(Trouble::Failed)?;
    let host = host_of(&c);
    let net = Net::new(place, c.kind()).map_err(Trouble::Failed)?;
    let owned = client_of(&c, &vault::load(id), &grant.client_id, &endpoints);
    let Some(spend) = grant.refresh.clone() else {
        ended(id);
        return Err(Trouble::SignIn);
    };
    match tokens::refresh(&net, &endpoints, &owned.as_client(), &spend, &grant.resource).await {
        Ok(t) => {
            let at = now();
            let rotated = t.refresh.is_some();
            let next = Grant {
                access: t.access,
                refresh: t.refresh.or(grant.refresh.clone()),
                expires: t.expires_in.map_or(0, |s| at + s),
                received: at,
                scopes: t.scope.as_deref().map(split_scopes).unwrap_or_else(|| grant.scopes.clone()),
                ..grant
            };
            // Kept at once: a rotated refresh token is spent.
            if let Err(e) = vault::update(id, |s| s.grant = Some(next.clone())) {
                tracing::error!(connector = id, "connectors: a refreshed sign-in wasn't kept: {e}");
            }
            tracing::info!(connector = id, rotated, "connectors: sign-in refreshed");
            let until = use_until(&next);
            Ok((next.access, until))
        }
        Err(Failure::Ended | Failure::Client) => {
            tracing::info!(connector = id, "connectors: the sign-in's refresh was turned down");
            ended(id);
            Err(Trouble::SignIn)
        }
        Err(f) => Err(Trouble::Failed(f.words(&host))),
    }
}

/// The vault has no grant for a connector that says it's signed in (a
/// credential removed): needs a sign-in.
fn no_grant(id: &str) {
    let changed = store::update(id, |c| std::mem::take(&mut c.auth.signed_in)).unwrap_or(false);
    if changed {
        tell(id);
    }
}

/// **The sign-in can't be used any more**: its tokens gone from the vault
/// (the registration and a client's secret stay), "expired" until the
/// person signs in again.
pub fn ended(id: &str) {
    let _ = vault::update(id, |s| s.grant = None);
    let changed = store::update(id, |c| {
        let was = (c.auth.signed_in, c.auth.expired);
        c.auth.signed_in = false;
        c.auth.expired = c.auth.kind == AuthKind::Oauth;
        was != (c.auth.signed_in, c.auth.expired)
    })
    .unwrap_or(false);
    if changed {
        tracing::info!(connector = id, "connectors: the sign-in ended");
        tell(id);
    }
}

/// **A server asked for more access** (`403 insufficient_scope` with
/// `scope`): those scopes kept for the next sign-in, the state "needs
/// sign-in" — unless Yap asked for them all at the last sign-in already
/// (given up: the call just fails).
pub fn step_up(id: &str, scope: Option<&str>) {
    let wanted = scope.map(split_scopes).unwrap_or_default();
    let changed = store::update(id, |c| {
        if c.auth.kind != AuthKind::Oauth {
            return false;
        }
        let new: Vec<String> = wanted.iter().filter(|s| !c.auth.scopes.contains(s) && !c.auth.wants.contains(s)).cloned().collect();
        c.auth.wants.extend(new.iter().cloned());
        !new.is_empty()
    })
    .unwrap_or(false);
    if changed {
        tracing::info!(connector = id, scopes = wanted.len(), "connectors: the server wants more access (step-up)");
        tell(id);
    }
}

/// A connection's tokens (`remote::Http::with_sign_in`): the access token
/// held while it lasts, refreshed through [`access_token`].
pub struct Source {
    id: String,
    held: Mutex<Option<(String, i64)>>,
}

impl Source {
    pub fn new(id: &str) -> Source {
        Source { id: id.to_string(), held: Mutex::new(None) }
    }

    fn hold(&self, t: &(String, i64)) {
        *self.held.lock().unwrap_or_else(|p| p.into_inner()) = Some(t.clone());
    }

    /// The token for the next request.
    pub async fn bearer(&self) -> Result<String, Trouble> {
        let held = self.held.lock().unwrap_or_else(|p| p.into_inner()).clone();
        if let Some((t, _)) = held.filter(|(_, until)| *until == 0 || *until > now()) {
            return Ok(t);
        }
        let t = access_token(&self.id, None).await?;
        self.hold(&t);
        Ok(t.0)
    }

    /// The server refused `used` (a 401): a fresher one, or why there's none.
    pub async fn refused(&self, used: &str) -> Result<String, Trouble> {
        {
            let mut h = self.held.lock().unwrap_or_else(|p| p.into_inner());
            if h.as_ref().is_some_and(|(t, _)| t == used) {
                *h = None;
            }
        }
        let t = access_token(&self.id, Some(used)).await?;
        self.hold(&t);
        Ok(t.0)
    }

    /// The fresh token was refused too: the sign-in is over.
    pub async fn ended(&self) {
        *self.held.lock().unwrap_or_else(|p| p.into_inner()) = None;
        ended(&self.id);
    }

    /// A `403 insufficient_scope` ([`step_up`]).
    pub fn step_up(&self, scope: Option<&str>) {
        step_up(&self.id, scope);
    }
}

// ---- signing in --------------------------------------------------------------------------------------

/// How a sign-in stopped short.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stop {
    Cancelled,
    Failed(String),
}

/// How the browser is opened (the person's, through `open_external`; a
/// test's stand-in).
pub type Open<'a> = &'a (dyn Fn(&str) -> Result<(), String> + Send + Sync);

/// Whether connector `c` signs in in the browser.
pub fn can_sign_in(c: &Connector) -> Result<(), String> {
    if c.transport.is_local() {
        return Err(LOCAL.into());
    }
    if !c.enabled {
        return Err("This connector is turned off.".into());
    }
    match c.auth.kind {
        AuthKind::Bearer => Err("This connector uses a token. Remove the token to sign in in the browser instead.".into()),
        AuthKind::Header => Err("This connector sends its own headers, so it doesn't sign in in the browser.".into()),
        AuthKind::None | AuthKind::Oauth => Ok(()),
    }
}

/// The scopes to ask for: the challenge's, else the resource's; then on a
/// step-up the ones asked for before and the ones the server asked for
/// since; `offline_access` (a refresh token) when the server lists it and
/// there's anything to ask for. Each once. Pure.
pub fn scopes(challenge: Option<&str>, resource: &[String], server: &[String], before: &[String], wants: &[String]) -> Vec<String> {
    let mut out = challenge.map(split_scopes).filter(|s| !s.is_empty()).unwrap_or_else(|| resource.to_vec());
    for s in before.iter().chain(wants) {
        if !out.contains(s) {
            out.push(s.clone());
        }
    }
    if !out.is_empty() && server.iter().any(|s| s == "offline_access") && !out.iter().any(|s| s == "offline_access") {
        out.push("offline_access".into());
    }
    out
}

fn check(t: &Ticket) -> Result<(), Stop> {
    if t.cancelled() {
        Err(Stop::Cancelled)
    } else {
        Ok(())
    }
}

/// The server's "no", in Yap's words (its error code only, never its text).
fn denied_words(error: &str, host: &str) -> String {
    match error {
        "access_denied" => format!("You didn't allow Yap to use {host}."),
        "invalid_scope" => format!("{host} turned down the access Yap asked for."),
        e if !e.is_empty() && e.len() <= 40 && e.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') => format!("{host}'s sign-in said no ({e})."),
        _ => format!("{host}'s sign-in said no."),
    }
}

const MIXED_UP: &str = "The browser's answer didn't come from the sign-in server Yap asked, so Yap didn't use it. Try again.";

/// The client a sign-in uses.
struct Chosen {
    kind: ClientKind,
    owned: Owned,
}

/// **Sign connector `c` in through the browser** ([module docs](self)):
/// `Ok` once its tokens are kept and connectors.json says so.
pub async fn sign_in(c: &Connector, ticket: &Ticket, open: Open<'_>) -> Result<(), Stop> {
    can_sign_in(c).map_err(Stop::Failed)?;
    let (server, place) = server_of(c).map_err(Stop::Failed)?;
    let host = net::host_of(&server);
    let net = Net::new(place, c.kind()).map_err(Stop::Failed)?;

    // 1. The challenge (a 401 without one: the well-known address still).
    let probe = net.probe(&server).await.map_err(Stop::Failed)?;
    let challenge = match probe.status {
        401 => probe.challenge.as_deref().map(discover::challenge).unwrap_or_default(),
        s if (200..300).contains(&s) => return Err(Stop::Failed(format!("{host} doesn't ask for a sign-in: Yap can use it as it is."))),
        _ => discover::Challenge::default(),
    };
    check(ticket)?;

    // 2. The protected resource, and 3. the authorization server it names.
    let resource = discover::resource(&net, &server, challenge.resource_metadata.as_deref()).await.map_err(Stop::Failed)?;
    check(ticket)?;
    let mut found = Err(format!("Couldn't find {host}'s sign-in details."));
    for issuer in &resource.authorization_servers {
        found = discover::metadata(&net, issuer).await;
        if found.is_ok() {
            break;
        }
    }
    let meta = found.map_err(Stop::Failed)?;
    let issuer = meta.endpoints.issuer.clone();
    check(ticket)?;

    // Another issuer than last time: what was made with the old one goes.
    let before = store::get(&c.id).ok_or_else(|| Stop::Failed("There's no such connector.".into()))?;
    if before.auth.issuer.as_deref().is_some_and(|old| !same_issuer(old, &issuer)) {
        tracing::info!(connector = %c.id, "connectors: the server's sign-in server changed; its old registration dropped");
        let _ = store::update(&c.id, |c| {
            // The new one from now, so a sign-in that fails after
            // registering with it doesn't drop that registration again.
            c.auth.issuer = Some(issuer.clone());
            c.auth.registration = None;
            c.auth.scopes.clear();
            c.auth.wants.clear();
            c.auth.signed_in = false;
        });
        let _ = vault::update(&c.id, |s| {
            s.grant = None;
            s.registered_secret = None;
        });
    }
    let latest = store::get(&c.id).ok_or_else(|| Stop::Failed("There's no such connector.".into()))?;
    let asked = scopes(challenge.scope.as_deref(), &resource.scopes, &meta.scopes, &latest.auth.scopes, &latest.auth.wants);

    // 4. The listener (its port goes into a registration), 5. the client.
    let listener = loopback::listen().map_err(Stop::Failed)?;
    let secrets = vault::load(&c.id);
    let cimd = client::metadata_url();
    let pick = client::pick(latest.auth.own.as_ref(), client::catalog_client(&issuer), cimd.as_deref(), &meta, latest.auth.registration.as_ref());
    let chosen = match pick {
        Pick::Own(id) => Chosen { kind: ClientKind::Own, owned: client_of(&latest, &secrets, &id, &meta.endpoints) },
        Pick::Catalog(cc) => Chosen { kind: ClientKind::Catalog, owned: client_of(&latest, &secrets, cc.client_id, &meta.endpoints) },
        Pick::Cimd(url) => Chosen { kind: ClientKind::Cimd, owned: Owned { id: url, secret: None, method: Method::None } },
        Pick::Registered(r) => Chosen { kind: ClientKind::Dcr, owned: client_of(&latest, &secrets, &r.client_id, &meta.endpoints) },
        Pick::Register(endpoint) => {
            let redirects = client::redirect_uris(listener.port);
            let body = client::registration_request(&redirects, &asked);
            let answer = tokens::register(&net, &endpoint, &body).await.map_err(Stop::Failed)?;
            let (registration, secret) = client::registration_of(&answer, &issuer, &redirects, now() as u64)
                .ok_or_else(|| Stop::Failed(format!("{host}'s sign-in server registered Yap but didn't say as what. Try again.")))?;
            // Kept before the sign-in goes on: Yap never registers twice.
            store::update(&c.id, |c| c.auth.registration = Some(registration.clone())).map_err(Stop::Failed)?;
            vault::update(&c.id, |s| s.registered_secret = secret).map_err(Stop::Failed)?;
            tracing::info!(connector = %c.id, "connectors: registered Yap with the sign-in server");
            let now_kept = store::get(&c.id).unwrap_or_else(|| latest.clone());
            Chosen { kind: ClientKind::Dcr, owned: client_of(&now_kept, &vault::load(&c.id), &registration.client_id, &meta.endpoints) }
        }
        Pick::Ask => return Err(Stop::Failed(client::ask_words(&host))),
    };
    check(ticket)?;

    // 6. The browser.
    let (verifier, pkce) = loopback::pkce().map_err(Stop::Failed)?;
    let ask = loopback::Ask {
        authorize: &meta.endpoints.authorization_endpoint,
        client_id: &chosen.owned.id,
        redirect_uri: &listener.redirect_uri,
        state: &listener.state,
        challenge: &pkce,
        resource: &resource.resource,
        scopes: &asked,
    };
    let page = loopback::authorize_url(&ask).map_err(Stop::Failed)?;
    open(&page).map_err(Stop::Failed)?;
    tracing::info!(connector = %c.id, client = ?chosen.kind, scopes = asked.len(), "connectors: sign-in opened in the browser");

    // 7. Its answer: this attempt's (the listener checked `state`), from this
    // issuer (`iss`), else nothing from it is used or shown.
    let (handback, listener) = wait(listener, ticket.clone()).await?;
    let redirect_uri = listener.redirect_uri.clone();
    let (code, reply) = match handback {
        Handback::Denied { error, iss, reply } => {
            let why = if loopback::iss_ok(iss.as_deref(), &issuer, meta.endpoints.iss_supported) { denied_words(&error, &host) } else { MIXED_UP.to_string() };
            tracing::info!(connector = %c.id, "connectors: the sign-in was turned down in the browser");
            let _ = reply.send(Done::Failed(why.clone()));
            return Err(Stop::Failed(why));
        }
        Handback::Code { iss, reply, .. } if !loopback::iss_ok(iss.as_deref(), &issuer, meta.endpoints.iss_supported) => {
            tracing::warn!(connector = %c.id, "connectors: a sign-in's answer named another issuer; not used");
            let _ = reply.send(Done::Failed(MIXED_UP.into()));
            return Err(Stop::Failed(MIXED_UP.into()));
        }
        Handback::Code { code, reply, .. } => (code, reply),
    };

    // 8. The code swapped, the tokens kept.
    let out = finish(&latest, &net, &meta, &resource.resource, &chosen, &asked, &code, &verifier, &redirect_uri).await;
    drop(code);
    match out {
        Ok(()) => {
            let _ = reply.send(Done::SignedIn(c.name.clone()));
            Ok(())
        }
        Err(why) => {
            let _ = reply.send(Done::Failed(why.clone()));
            Err(Stop::Failed(why))
        }
    }
}

/// Wait for the browser (until a Cancel or [`loopback::TTL`]).
async fn wait(listener: Listener, ticket: Ticket) -> Result<(Handback, Listener), Stop> {
    tokio::task::spawn_blocking(move || {
        let deadline = Instant::now() + loopback::TTL;
        loop {
            match listener.handback.recv_timeout(Duration::from_millis(200)) {
                Ok(h) => return Ok((h, listener)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) if ticket.cancelled() => return Err(Stop::Cancelled),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) if Instant::now() >= deadline => {
                    return Err(Stop::Failed("Yap waited 10 minutes for the sign-in in your browser. Try again.".into()))
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Err(Stop::Failed("Yap stopped listening for the browser. Try again.".into())),
            }
        }
    })
    .await
    .map_err(|_| Stop::Failed("Yap couldn't finish the sign-in. Try again.".into()))?
}

/// The code swapped for tokens, which are kept (the vault) and the sign-in
/// recorded (connectors.json).
#[allow(clippy::too_many_arguments)]
async fn finish(c: &Connector, net: &Net, meta: &discover::Meta, resource: &str, chosen: &Chosen, asked: &[String], code: &str, verifier: &str, redirect_uri: &str) -> Result<(), String> {
    let host = host_of(c);
    let t = match tokens::exchange(net, &meta.endpoints, &chosen.owned.as_client(), code, verifier, redirect_uri, resource).await {
        Ok(t) => t,
        Err(Failure::Client) if chosen.kind == ClientKind::Dcr => {
            // The server forgot Yap's registration: made again next time.
            let _ = store::update(&c.id, |c| c.auth.registration = None);
            let _ = vault::update(&c.id, |s| s.registered_secret = None);
            return Err(format!("{host} didn't know Yap's app registration any more. Sign in again."));
        }
        Err(f) => return Err(f.words(&host)),
    };
    let at = now();
    let granted = t.scope.as_deref().map(split_scopes).unwrap_or_else(|| asked.to_vec());
    let mut kept: Vec<String> = asked.to_vec();
    for s in &granted {
        if !kept.contains(s) {
            kept.push(s.clone());
        }
    }
    let renews = t.refresh.is_some();
    let grant = Grant {
        access: t.access,
        refresh: t.refresh,
        expires: t.expires_in.map_or(0, |s| at + s),
        received: at,
        scopes: granted,
        issuer: meta.endpoints.issuer.clone(),
        resource: resource.to_string(),
        client_id: chosen.owned.id.clone(),
    };
    vault::update(&c.id, |s| s.grant = Some(grant))?;
    store::update(&c.id, |c| {
        c.auth.kind = AuthKind::Oauth;
        c.auth.issuer = Some(meta.endpoints.issuer.clone());
        c.auth.resource = Some(resource.to_string());
        c.auth.client = Some(ClientRef { kind: chosen.kind, client_id: chosen.owned.id.clone() });
        c.auth.scopes = kept;
        c.auth.signed_in = true;
        c.auth.expired = false;
        c.auth.wants.clear();
        c.auth.server = Some(meta.endpoints.clone());
    })?;
    tracing::info!(connector = %c.id, renews, lasts = t.expires_in.unwrap_or(0), "connectors: signed in");
    Ok(())
}

/// **Sign connector `id` out**: its grant revoked where the server offers
/// it (the refresh token, which takes its access tokens with it; else the
/// access token), the tokens gone here either way. Whether the server
/// confirmed it (`false`: no revocation offered, or refused — the person is
/// told to disconnect Yap on the service's own page).
pub async fn sign_out(id: &str) -> bool {
    let Some(c) = store::get(id) else { return true };
    // No refresh while this runs; another Yap holding it 15 s isn't a reason
    // to keep the tokens.
    let _guard = refresh::acquire(id).await.ok();
    let secrets = vault::load(id);
    let mut confirmed = true;
    if let Some(grant) = &secrets.grant {
        let endpoints = c.auth.server.as_ref().filter(|e| same_issuer(&e.issuer, &grant.issuer));
        match (endpoints, endpoints.and_then(|e| e.revocation_endpoint.clone()), server_of(&c)) {
            (Some(e), Some(endpoint), Ok((_, place))) => {
                let owned = client_of(&c, &secrets, &grant.client_id, e);
                confirmed = match Net::new(place, c.kind()) {
                    Ok(net) => match &grant.refresh {
                        Some(r) => tokens::revoke(&net, &endpoint, &owned.as_client(), r, "refresh_token").await,
                        None => tokens::revoke(&net, &endpoint, &owned.as_client(), &grant.access, "access_token").await,
                    },
                    Err(_) => false,
                };
            }
            _ => confirmed = false,
        }
    }
    let _ = vault::update(id, |s| s.grant = None);
    let _ = store::update(id, |c| {
        c.auth.signed_in = false;
        c.auth.expired = false;
        c.auth.wants.clear();
        c.auth.scopes.clear();
    });
    tracing::info!(connector = id, confirmed, "connectors: signed out");
    confirmed
}

/// **The person's own client** for connector `id` (`client_id` empty:
/// forgotten, with its secret): the id in connectors.json, the secret in
/// the vault.
pub fn set_client(id: &str, client_id: &str, secret: Option<&str>) -> Result<(), String> {
    let c = store::get(id).ok_or("There's no such connector.")?;
    if c.transport.is_local() {
        return Err(LOCAL.into());
    }
    let client_id = client_id.trim();
    let secret = secret.map(str::trim).filter(|s| !s.is_empty());
    if client_id.is_empty() {
        store::update(id, |c| c.auth.own = None)?;
        vault::update(id, |s| s.own_secret = None)?;
        return Ok(());
    }
    if client_id.len() > 255 || client_id.chars().any(|ch| ch.is_whitespace() || ch.is_control()) {
        return Err("That doesn't look like a client ID.".into());
    }
    if secret.is_some_and(|s| s.len() > 512 || s.chars().any(char::is_control)) {
        return Err("That client secret has characters Yap can't send.".into());
    }
    let has_secret = secret.is_some();
    store::update(id, |c| c.auth.own = Some(OwnClient { client_id: client_id.to_string() }))?;
    vault::update(id, |s| s.own_secret = secret.map(str::to_string))?;
    tracing::info!(connector = id, has_secret, "connectors: the person's own client set");
    Ok(())
}

/// What the page shows about a connector's sign-in.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignInInfo {
    pub state: SignInState,
    /// The sign-in server's host (the one the browser opens).
    pub host: Option<String>,
    /// The client the last sign-in used.
    pub client: Option<ClientKind>,
    /// The person's own client id, when they entered one.
    pub own_client_id: Option<String>,
    pub has_own_secret: bool,
    pub scopes: Vec<String>,
    /// The redirect a hand-entered client registers.
    pub redirect_uri: String,
}

/// Connector `c`'s sign-in for the page (`None`: it doesn't sign in in the
/// browser).
pub fn info(c: &Connector, status: &Status, secrets: &Secrets) -> Option<SignInInfo> {
    let state = state_of(c, status)?;
    let host = c.auth.server.as_ref().and_then(|e| url::Url::parse(&e.authorization_endpoint).ok()).map(|u| net::host_of(&u));
    Some(SignInInfo {
        state,
        host,
        client: c.auth.client.as_ref().map(|r| r.kind),
        own_client_id: c.auth.own.as_ref().map(|o| o.client_id.clone()),
        has_own_secret: secrets.own_secret.is_some(),
        scopes: c.auth.scopes.clone(),
        redirect_uri: loopback::redirect_uri(loopback::PORT),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant(access: &str, refresh: Option<&str>, expires: i64) -> Grant {
        Grant {
            access: access.into(),
            refresh: refresh.map(str::to_string),
            expires,
            received: 0,
            scopes: vec![],
            issuer: "https://auth.example.com".into(),
            resource: "https://mcp.example.com/mcp".into(),
            client_id: "c".into(),
        }
    }

    #[test]
    fn a_token_is_refreshed_30_s_before_it_runs_out_and_another_yaps_is_used() {
        assert_eq!(decide(&grant("a", Some("r"), 1_000), 900, None), Decision::Use);
        assert_eq!(decide(&grant("a", Some("r"), 1_000), 971, None), Decision::Refresh, "within 30 s");
        // A token that lasts 10 s is refreshed at half its life, not at every use.
        let short = Grant { received: 990, ..grant("a", Some("r"), 1_000) };
        assert_eq!((decide(&short, 994, None), decide(&short, 996, None)), (Decision::Use, Decision::Refresh));
        assert_eq!(use_until(&short), 995);
        assert_eq!(use_until(&grant("a", None, 0)), 0);
        assert_eq!(decide(&grant("a", None, 1_000), 971, None), Decision::Ended, "nothing to refresh with");
        assert_eq!(decide(&grant("a", None, 0), 1_000_000, None), Decision::Use, "no expiry said: used until refused");
        assert_eq!(decide(&grant("a", Some("r"), 0), 5, Some("a")), Decision::Refresh, "refused");
        assert_eq!(decide(&grant("b", Some("r"), 1_000), 5, Some("a")), Decision::Use, "another Yap's newer token");
    }

    #[test]
    fn the_scopes_asked_for() {
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(scopes(Some("read"), &v(&["read", "write"]), &v(&["offline_access"]), &[], &[]), ["read", "offline_access"]);
        assert_eq!(scopes(None, &v(&["read", "write"]), &[], &[], &[]), ["read", "write"]);
        assert!(scopes(None, &[], &v(&["offline_access"]), &[], &[]).is_empty(), "nothing to ask for: not offline_access alone");
        // A step-up: the scopes before and the new ones.
        assert_eq!(scopes(Some("read"), &[], &[], &v(&["read", "offline_access"]), &v(&["write"])), ["read", "offline_access", "write"]);
    }

    #[test]
    fn one_sign_in_at_a_time() {
        let (a, before) = begin("c1");
        assert_eq!(before, None);
        assert!(signing_in("c1"));
        let (b, before) = begin("c2");
        assert_eq!(before.as_deref(), Some("c1"));
        assert!(a.cancelled() && !b.cancelled() && !signing_in("c1") && signing_in("c2"));
        end(&a);
        assert!(signing_in("c2"), "an older attempt's end doesn't end the newer");
        assert!(!cancel("c1"));
        assert!(cancel("c2") && b.cancelled() && !signing_in("c2"));
        end(&b);
    }

    #[test]
    fn a_servers_no_is_said_in_yaps_words() {
        assert_eq!(denied_words("access_denied", "mcp.example.com"), "You didn't allow Yap to use mcp.example.com.");
        assert_eq!(denied_words("temporarily_unavailable", "mcp.example.com"), "mcp.example.com's sign-in said no (temporarily_unavailable).");
        assert_eq!(denied_words("Visit evil.example.com", "mcp.example.com"), "mcp.example.com's sign-in said no.");
    }

    /// No token, code, verifier, secret or authorize address is ever
    /// formatted into a log line, at any level, in the sign-in's code.
    #[test]
    fn the_sign_in_never_logs_a_secret() {
        let files = [
            include_str!("oauth.rs"),
            include_str!("oauth/discover.rs"),
            include_str!("oauth/client.rs"),
            include_str!("oauth/loopback.rs"),
            include_str!("oauth/tokens.rs"),
            include_str!("refresh.rs"),
        ];
        let banned = regex::Regex::new(r"\b(token|tokens|access|refresh_token|spend|verifier|pkce|secret|secrets|code|grant|page|link|state|query|body|answer|form)\b").unwrap();
        let call = regex::Regex::new(r"(?:tracing::)?(?:trace|debug|info|warn|error)!\s*\(").unwrap();
        let strings = regex::Regex::new(r#""(?:[^"\\]|\\.)*""#).unwrap();
        let named = regex::Regex::new(r"\{(\w+)").unwrap();
        for src in files {
            for m in call.find_iter(src) {
                // The call's text, to its closing parenthesis.
                let rest = &src[m.end()..];
                let mut depth = 1;
                let mut end = 0;
                let mut in_str = false;
                let mut prev = ' ';
                for (i, ch) in rest.char_indices() {
                    match ch {
                        '"' if prev != '\\' => in_str = !in_str,
                        '(' if !in_str => depth += 1,
                        ')' if !in_str => {
                            depth -= 1;
                            if depth == 0 {
                                end = i;
                                break;
                            }
                        }
                        _ => {}
                    }
                    prev = ch;
                }
                let text = &rest[..end];
                // Inside the message, only `{e}`-style names count.
                let outside = strings.replace_all(text, "\"\"");
                let inside: Vec<String> = strings.find_iter(text).flat_map(|s| named.captures_iter(s.as_str()).map(|c| c[1].to_string()).collect::<Vec<_>>()).collect();
                assert!(!banned.is_match(&outside), "a log call formats something secret: {text}");
                for name in inside {
                    assert!(!banned.is_match(&name), "a log message formats {name}: {text}");
                }
            }
        }
    }
}

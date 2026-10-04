//! Yap accounts: optional sign-in against the Yap account service (`cloud/`,
//! Better Auth on Cloudflare at auth.contextmirror.com). Nothing in dictation
//! depends on it.
//!
//! Every route ends with a Better Auth session token, kept in Windows
//! Credential Manager (never in config.json, the logs, or the webview) and
//! sent as `Authorization: Bearer <token>`:
//! - **Email code**: Yap calls the API directly (send a code, verify it).
//! - **Google / Microsoft / GitHub** (RFC 8252): the system browser does the
//!   provider sign-in, then hands Yap a one-time code that only redeems with
//!   the PKCE verifier Yap kept back. The code comes back by whichever channel
//!   this build has:
//!   - installed builds: the `com.contextmirror.yap://auth/callback#token=…`
//!     deep link (the installer registers the scheme; single-instance
//!     forwards it into the running app);
//!   - dev/portable builds, where the scheme isn't registered to this exe: a
//!     one-shot loopback listener on 127.0.0.1, whose port rides at the end
//!     of `state` for the account page to redirect to;
//!   - always: the account page shows the code to paste into Yap.
//!
//! Server endpoints and the handoff protocol: `cloud/README.md`.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};
use tiny_http::{Header, Response, Server};

/// Private-use URI scheme (RFC 8252 §7.1: reverse of a domain we control).
const SCHEME: &str = "com.contextmirror.yap";
/// Matches `DESKTOP_CLIENT_ID` in cloud/src/auth.ts.
const CLIENT_ID: &str = "yap-desktop";
const PROVIDERS: &[&str] = &["google", "microsoft", "github"];
/// How long a browser sign-in may take (provider 2FA etc.) before Yap stops
/// waiting. The server's handoff code itself lives 5 minutes.
const PENDING_TTL: Duration = Duration::from_secs(15 * 60);
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);
/// Re-validate the session this often while Yap runs, which also keeps the
/// server's 30-day sliding expiry rolling for people who never restart.
const REFRESH_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

const EXPIRED: &str = "That sign-in has expired. Start again from Settings → Account.";
const UNREACHABLE: &str =
    "Couldn't reach Yap's account service. Check your connection and try again.";

/// The account service. `YAP_AUTH_URL` overrides it; debug builds default to
/// the local `wrangler dev` server (cloud/README.md).
pub fn base_url() -> String {
    if let Ok(url) = std::env::var("YAP_AUTH_URL") {
        let url = url.trim().trim_end_matches('/');
        if !url.is_empty() {
            return url.to_string();
        }
    }
    if cfg!(debug_assertions) {
        "http://localhost:8787".into()
    } else {
        PRODUCTION_URL.into()
    }
}

const PRODUCTION_URL: &str = "https://auth.contextmirror.com";

/// Credential Manager user name for the session kept for `base`. Production
/// keeps the plain name; any other service (a dev build's local server) gets
/// its own entry, so it can never read, or sign out, the installed app's
/// real session.
#[cfg_attr(not(windows), allow(dead_code))]
fn credential_user_for(base: &str) -> String {
    if base == PRODUCTION_URL {
        "yap-account".into()
    } else {
        let host = base.split_once("://").map_or(base, |(_, rest)| rest);
        format!("yap-account@{}", host)
    }
}

fn api(path: &str) -> String {
    format!("{}/api/auth{}", base_url(), path)
}

static CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        // Recorded on the server's session row (which device is signed in).
        .user_agent(format!("Yap/{} (Windows)", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
});

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub email: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub image: Option<String>,
}

/// What Credential Manager holds: the token plus a profile copy, so an
/// offline start still shows who's signed in.
#[derive(Clone, Serialize, Deserialize)]
struct Stored {
    token: String,
    user: Profile,
    /// Unix seconds when this session was created by signing in.
    #[serde(default)]
    signed_in_at: u64,
}

/// Snapshot for the UI (`auth_status` + the `yap-auth-changed` event).
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    signed_in: bool,
    user: Option<Profile>,
    /// Linked sign-in providers ("google", "microsoft", "github"). Email codes
    /// work for every account, so they aren't listed.
    providers: Vec<String>,
    /// Provider of a browser sign-in in progress.
    pending: Option<String>,
    /// Signed in from the stored copy, not yet confirmed with the server.
    offline: bool,
    /// When the current session began (unix seconds). Changes on every fresh
    /// sign-in, which the delete-account flow uses to spot a re-sign-in.
    signed_in_at: Option<u64>,
    /// What the service offers right now (`/api/providers`); `None` until
    /// it has been reached, and then the UI offers everything.
    methods: Option<Methods>,
    service_url: String,
}

/// The service's sign-in methods: configured providers, and whether email
/// codes can be sent.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Methods {
    providers: Vec<String>,
    email: bool,
}

struct Pending {
    provider: String,
    state: String,
    verifier: String,
    loopback: Option<Arc<Server>>,
}

impl Drop for Pending {
    fn drop(&mut self) {
        // Ends the loopback thread's accept loop (no-op if already finished).
        if let Some(server) = &self.loopback {
            server.unblock();
        }
    }
}

struct Inner {
    pending: Option<Pending>,
    session: Option<Stored>,
    providers: Vec<String>,
    offline: bool,
    methods: Option<Methods>,
}

static STATE: Mutex<Inner> = Mutex::new(Inner {
    pending: None,
    session: None,
    providers: Vec::new(),
    offline: false,
    methods: None,
});

fn lock() -> MutexGuard<'static, Inner> {
    match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

pub fn status() -> Status {
    let inner = lock();
    Status {
        signed_in: inner.session.is_some(),
        user: inner.session.as_ref().map(|s| s.user.clone()),
        providers: inner.providers.clone(),
        pending: inner.pending.as_ref().map(|p| p.provider.clone()),
        offline: inner.offline,
        signed_in_at: inner.session.as_ref().map(|s| s.signed_in_at),
        methods: inner.methods.clone(),
        service_url: base_url(),
    }
}

fn emit_status(app: &AppHandle) {
    let _ = app.emit("yap-auth-changed", status());
}

/// Async failures the UI didn't directly ask for (deep link / loopback).
fn emit_error(app: &AppHandle, message: &str) {
    let _ = app.emit("yap-auth-error", message);
}

// ---- startup ----

/// Wire the deep-link handler and restore the stored session. Call from setup.
pub fn init(app: &AppHandle) {
    #[cfg(desktop)]
    {
        use tauri_plugin_deep_link::DeepLinkExt;
        let handle = app.clone();
        app.deep_link().on_open_url(move |event| {
            for url in event.urls() {
                handle_deep_link(&handle, &url);
            }
        });
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            refresh(&app).await;
            // Signed out, Yap never contacts the account service on its own:
            // the Account page fetches the sign-in methods when it opens.
            let signed_in = lock().session.is_some();
            if signed_in {
                refresh_methods(&app).await;
            }
            tokio::time::sleep(REFRESH_EVERY).await;
        }
    });
}

/// Re-read the service's sign-in methods. The Account page calls this when it
/// opens: the only time a signed-out Yap talks to the account service.
#[tauri::command]
pub async fn auth_check_methods(app: AppHandle) {
    refresh_methods(&app).await;
}

async fn refresh_methods(app: &AppHandle) {
    let fetched = async {
        let resp = CLIENT
            .get(format!("{}/api/providers", base_url()))
            .timeout(Duration::from_secs(8))
            .send()
            .await
            .ok()?;
        resp.status().is_success().then_some(())?;
        resp.json::<Methods>().await.ok()
    }
    .await;
    // Unreachable: keep what we last knew (or "unknown", which offers all).
    let Some(mut methods) = fetched else {
        return;
    };
    methods.providers.retain(|p| PROVIDERS.contains(&p.as_str()));
    let changed = {
        let mut inner = lock();
        let changed = inner.methods.as_ref() != Some(&methods);
        inner.methods = Some(methods);
        changed
    };
    if changed {
        emit_status(app);
    }
}

/// Load the stored session (if any) and confirm it with the server: a session
/// the server no longer knows (expired, revoked, account deleted) signs Yap
/// out; an unreachable server keeps the stored copy, flagged offline.
async fn refresh(app: &AppHandle) {
    let token = {
        let mut inner = lock();
        if inner.session.is_none() {
            inner.session = vault::load();
            inner.offline = inner.session.is_some();
        }
        inner.session.as_ref().map(|s| s.token.clone())
    };
    let Some(token) = token else {
        return;
    };
    emit_status(app);

    match get_session(&token).await {
        Ok(Some(user)) => {
            let providers = list_providers(&token).await.unwrap_or_default();
            let mut inner = lock();
            // Skip if the user signed out / switched while the request ran.
            if let Some(s) = inner.session.as_mut().filter(|s| s.token == token) {
                if s.user != user {
                    s.user = user;
                    let _ = vault::save(s);
                }
                inner.offline = false;
                inner.providers = providers;
            }
        }
        Ok(None) => {
            tracing::info!("auth: stored session is no longer valid; signing out");
            let mut inner = lock();
            if inner.session.as_ref().is_some_and(|s| s.token == token) {
                inner.session = None;
                inner.providers.clear();
                inner.offline = false;
                vault::clear();
            }
        }
        Err(e) => tracing::info!("auth: session check failed (keeping stored session): {}", e.code),
    }
    emit_status(app);
}

// ---- browser sign-in (Google / Microsoft / GitHub) ----

/// Open the system browser on a provider sign-in.
#[tauri::command]
pub async fn auth_start(app: AppHandle, provider: String) -> Result<(), String> {
    let provider = provider.to_ascii_lowercase();
    if !PROVIDERS.contains(&provider.as_str()) {
        return Err(format!("Unknown sign-in provider: {}", provider));
    }
    // Fail here, not on a dead browser tab, when the service is unreachable.
    let health = CLIENT
        .get(format!("{}/api/health", base_url()))
        .timeout(Duration::from_secs(6))
        .send()
        .await;
    if !health.is_ok_and(|r| r.status().is_success()) {
        return Err(UNREACHABLE.into());
    }
    let nonce = random_alnum(16)?;
    let (verifier, challenge) = pkce_pair()?;

    let (state, loopback) = if scheme_registered(&app) {
        (nonce, None)
    } else {
        let (server, port) = start_loopback(&app)?;
        (format!("{}-{}", nonce, port), Some(server))
    };

    let mut url = url::Url::parse(&api("/electron/init-oauth-proxy")).map_err(|e| e.to_string())?;
    url.query_pairs_mut()
        .append_pair("provider", &provider)
        .append_pair("client_id", CLIENT_ID)
        .append_pair("code_challenge", &challenge)
        .append_pair("state", &state);

    // Replacing an older attempt drops it, which stops its loopback listener.
    lock().pending = Some(Pending {
        provider: provider.clone(),
        state: state.clone(),
        verifier,
        loopback,
    });
    tracing::info!("auth: browser sign-in started ({})", provider);

    {
        use tauri_plugin_opener::OpenerExt;
        if let Err(e) = app.opener().open_url(url.as_str(), None::<&str>) {
            lock().pending = None;
            emit_status(&app);
            return Err(format!("Couldn't open your browser: {}", e));
        }
    }
    emit_status(&app);

    // Give up after a while so a forgotten tab doesn't leave Yap waiting.
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(PENDING_TTL).await;
        let expired = {
            let mut inner = lock();
            let stale = inner.pending.as_ref().is_some_and(|p| p.state == state);
            if stale {
                inner.pending = None;
            }
            stale
        };
        if expired {
            emit_status(&app2);
            emit_error(&app2, "Sign-in timed out. Start again when you're ready.");
        }
    });
    Ok(())
}

/// Stop waiting for a browser sign-in.
#[tauri::command]
pub fn auth_cancel(app: AppHandle) {
    if lock().pending.take().is_some() {
        tracing::info!("auth: browser sign-in cancelled");
    }
    emit_status(&app);
}

/// The code from the account page, pasted by hand (the fallback channel).
#[tauri::command]
pub async fn auth_submit_code(app: AppHandle, code: String) -> Result<(), String> {
    redeem(&app, &code).await
}

/// Installed builds only: `true` when the installer registered our scheme to
/// this very exe, so a deep link would reach this app (and not, say, an
/// installed copy while running a dev build).
fn scheme_registered(app: &AppHandle) -> bool {
    #[cfg(windows)]
    {
        use tauri_plugin_deep_link::DeepLinkExt;
        !crate::portable::is_portable() && app.deep_link().is_registered(SCHEME).unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        false
    }
}

fn handle_deep_link(app: &AppHandle, url: &url::Url) {
    if url.scheme() != SCHEME || url.host_str() != Some("auth") || url.path() != "/callback" {
        tracing::warn!("auth: ignoring unrecognised deep link");
        return;
    }
    let _ = crate::commands::show_settings(app);
    let Some(code) = url.fragment().and_then(|f| f.strip_prefix("token=")) else {
        emit_error(app, EXPIRED);
        return;
    };
    let code = code.to_string();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = redeem(&app, &code).await {
            emit_error(&app, &e);
        }
    });
}

/// The one-time code the account page hands back: base64url JSON
/// `{identifier, state}`, possibly %-escaped, possibly inside the full
/// callback URL (pasted from the address bar).
#[derive(Deserialize)]
struct Handoff {
    identifier: String,
    state: String,
}

fn parse_handoff(code: &str) -> Option<Handoff> {
    let code = code.trim();
    // A whole callback URL? '#', '?' and '&' never occur in base64url.
    let code = match ["#token=", "?token=", "&token="].iter().find_map(|k| code.find(k).map(|i| i + k.len())) {
        Some(start) => code[start..].split(['&', '#']).next().unwrap_or_default(),
        None => code,
    };
    // %-decode via the form decoder ('+' never occurs in base64url).
    let decoded: String = url::form_urlencoded::parse(format!("t={}", code).as_bytes())
        .next()
        .map(|(_, v)| v.into_owned())?;
    let bytes = URL_SAFE_NO_PAD.decode(decoded.trim().trim_end_matches('=')).ok()?;
    let handoff: Handoff = serde_json::from_slice(&bytes).ok()?;
    let sane = |s: &str, max: usize| !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_graphic());
    (sane(&handoff.identifier, 128) && sane(&handoff.state, 128)).then_some(handoff)
}

/// Swap a handoff code for a session, if it belongs to the sign-in in flight.
async fn redeem(app: &AppHandle, code: &str) -> Result<(), String> {
    let handoff = parse_handoff(code)
        .ok_or("That doesn't look like a Yap sign-in code. Copy the whole code and try again.")?;

    let (state, verifier) = {
        let inner = lock();
        let Some(pending) = inner.pending.as_ref() else {
            return Err(EXPIRED.into());
        };
        if !same_secret(&pending.state, &handoff.state) {
            // Not ours (an older attempt, or someone else's link): leave the
            // current sign-in waiting.
            return Err("That code is from a different sign-in. Use the newest one, or start again.".into());
        }
        (pending.state.clone(), pending.verifier.clone())
    };

    let resp = CLIENT
        .post(api("/electron/token"))
        .json(&json!({ "token": handoff.identifier, "state": state, "code_verifier": verifier }))
        .send()
        .await
        // Never reached the server, so the code is still good: keep waiting.
        .map_err(|_| UNREACHABLE.to_string())?;
    let result = read_api(resp).await;

    // The server has answered, so the code is spent either way.
    {
        let mut inner = lock();
        if inner.pending.as_ref().is_some_and(|p| p.state == state) {
            inner.pending = None;
        }
    }
    // Success emits once, from finish_sign_in (pending → signed in in one step).
    let session = result
        .map_err(|e| e.friendly())
        .and_then(|body| session_from(&body).ok_or_else(|| "The account service sent an unexpected reply.".to_string()));
    let (token, user) = match session {
        Ok(s) => s,
        Err(e) => {
            emit_status(app);
            return Err(e);
        }
    };
    finish_sign_in(app, token, user).await;
    tracing::info!("auth: signed in (browser)");
    Ok(())
}

// ---- loopback channel (dev / portable) ----

fn start_loopback(app: &AppHandle) -> Result<(Arc<Server>, u16), String> {
    let server = Server::http("127.0.0.1:0")
        .map(Arc::new)
        .map_err(|e| format!("Couldn't start the sign-in listener: {}", e))?;
    let port = server
        .server_addr()
        .to_ip()
        .map(|a| a.port())
        .ok_or("Couldn't start the sign-in listener.")?;
    let srv = server.clone();
    let app = app.clone();
    std::thread::Builder::new()
        .name("yap-auth-loopback".into())
        .spawn(move || {
            for request in srv.incoming_requests() {
                if handle_loopback(&app, request) {
                    break;
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok((server, port))
}

/// One request to the loopback listener. Returns `true` once the sign-in this
/// listener served is over (signed in, or its code was spent).
fn handle_loopback(app: &AppHandle, request: tiny_http::Request) -> bool {
    let from_loopback = request.remote_addr().is_some_and(|a| a.ip().is_loopback());
    let url = request.url().to_string();
    let code = url
        .strip_prefix("/callback?")
        .and_then(|q| url::form_urlencoded::parse(q.as_bytes()).find(|(k, _)| k == "token"))
        .map(|(_, v)| v.into_owned());
    let (Some(code), true) = (code, from_loopback) else {
        let _ = request.respond(Response::empty(404));
        return false;
    };

    let result = tauri::async_runtime::block_on(redeem(app, &code));
    // Back to the account service's pages for the human-facing result.
    let location = match &result {
        Ok(()) => format!("{}/#done", base_url()),
        Err(_) => format!("{}/error?error=handoff_failed", base_url()),
    };
    let redirect = Response::empty(302)
        .with_header(Header::from_bytes(&b"Location"[..], location.as_bytes()).expect("ascii header"))
        .with_header(Header::from_bytes(&b"Cache-Control"[..], &b"no-store"[..]).expect("static header"));
    let _ = request.respond(redirect);

    match result {
        Ok(()) => {
            let _ = crate::commands::show_settings(app);
            true
        }
        Err(e) => {
            emit_error(app, &e);
            // Done unless the code was someone else's and ours still waits.
            lock().pending.is_none()
        }
    }
}

// ---- email code ----

/// Email a 6-digit sign-in code (creates the account on first use).
#[tauri::command]
pub async fn auth_email_send(email: String) -> Result<(), String> {
    let email = normalize_email(&email)?;
    let resp = CLIENT
        .post(api("/email-otp/send-verification-otp"))
        .json(&json!({ "email": email, "type": "sign-in" }))
        .send()
        .await
        .map_err(|_| UNREACHABLE.to_string())?;
    read_api(resp).await.map_err(|e| e.friendly())?;
    tracing::info!("auth: email code sent");
    Ok(())
}

#[tauri::command]
pub async fn auth_email_verify(app: AppHandle, email: String, code: String) -> Result<(), String> {
    let email = normalize_email(&email)?;
    let code: String = code.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
    if code.len() != 6 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return Err("Enter the 6-digit code from the email.".into());
    }
    let resp = CLIENT
        .post(api("/sign-in/email-otp"))
        .json(&json!({ "email": email, "otp": code }))
        .send()
        .await
        .map_err(|_| UNREACHABLE.to_string())?;
    let body = read_api(resp).await.map_err(|e| e.friendly())?;
    let (token, user) = session_from(&body).ok_or("The account service sent an unexpected reply.")?;
    // A browser sign-in left waiting is moot now.
    lock().pending = None;
    finish_sign_in(&app, token, user).await;
    tracing::info!("auth: signed in (email code)");
    Ok(())
}

fn normalize_email(email: &str) -> Result<String, String> {
    let email = email.trim().to_lowercase();
    let valid = email.len() <= 254
        && !email.contains(char::is_whitespace)
        && email
            .split_once('@')
            .is_some_and(|(local, domain)| !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.'));
    if valid {
        Ok(email)
    } else {
        Err("Enter a valid email address.".into())
    }
}

// ---- session ----

async fn finish_sign_in(app: &AppHandle, token: String, user: Profile) {
    let signed_in_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let stored = Stored { token, user, signed_in_at };
    if let Err(e) = vault::save(&stored) {
        // Still signed in for this run; it just won't survive a restart.
        tracing::error!("auth: couldn't store the session: {}", e);
    }
    let providers = list_providers(&stored.token).await.unwrap_or_default();
    {
        let mut inner = lock();
        inner.session = Some(stored);
        inner.providers = providers;
        inner.offline = false;
    }
    emit_status(app);
}

/// Sign out: revoke the session server-side (best effort, so it works
/// offline too) and forget it locally.
#[tauri::command]
pub async fn auth_sign_out(app: AppHandle) -> Result<(), String> {
    let token = lock().session.as_ref().map(|s| s.token.clone());
    if let Some(token) = token {
        let revoked = CLIENT
            .post(api("/sign-out"))
            .bearer_auth(&token)
            .json(&json!({}))
            .send()
            .await;
        if revoked.is_err() {
            tracing::info!("auth: couldn't reach the server to revoke the session; signing out locally");
        }
    }
    forget_session(&app);
    tracing::info!("auth: signed out");
    Ok(())
}

/// Permanently delete the account. Errors with `"reauth"` when the server
/// wants a fresh sign-in first (sessions older than a day).
#[tauri::command]
pub async fn auth_delete_account(app: AppHandle) -> Result<(), String> {
    let token = lock()
        .session
        .as_ref()
        .map(|s| s.token.clone())
        .ok_or("You're not signed in.")?;
    let resp = CLIENT
        .post(api("/delete-user"))
        .bearer_auth(&token)
        .json(&json!({}))
        .send()
        .await
        .map_err(|_| UNREACHABLE.to_string())?;
    match read_api(resp).await {
        Ok(_) => {
            forget_session(&app);
            tracing::info!("auth: account deleted");
            Ok(())
        }
        Err(e) if e.code == "SESSION_EXPIRED" => Err("reauth".into()),
        Err(e) => Err(e.friendly()),
    }
}

fn forget_session(app: &AppHandle) {
    {
        let mut inner = lock();
        inner.session = None;
        inner.providers.clear();
        inner.offline = false;
    }
    vault::clear();
    emit_status(app);
}

#[tauri::command]
pub fn auth_status() -> Status {
    status()
}

/// `Ok(None)`: the server answered but doesn't recognise the token.
async fn get_session(token: &str) -> Result<Option<Profile>, ApiError> {
    let resp = CLIENT
        .get(api("/get-session"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|_| ApiError::network())?;
    let body = read_api(resp).await?;
    Ok(body.get("user").and_then(|u| serde_json::from_value(u.clone()).ok()))
}

async fn list_providers(token: &str) -> Result<Vec<String>, ApiError> {
    let resp = CLIENT
        .get(api("/list-accounts"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|_| ApiError::network())?;
    let body = read_api(resp).await?;
    let mut ids: Vec<String> = body
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.get("providerId")?.as_str().map(String::from)).collect())
        .unwrap_or_default();
    ids.retain(|p| PROVIDERS.contains(&p.as_str()));
    ids.sort();
    ids.dedup();
    Ok(ids)
}

/// `{token, user}` from a sign-in response.
fn session_from(body: &Value) -> Option<(String, Profile)> {
    let token = body.get("token")?.as_str()?.to_string();
    let user = serde_json::from_value(body.get("user")?.clone()).ok()?;
    (!token.is_empty()).then_some((token, user))
}

// ---- API errors ----

#[derive(Debug)]
struct ApiError {
    status: u16,
    /// Better Auth's error code (`INVALID_OTP`, …), or `NETWORK`.
    code: String,
    retry_after: Option<u64>,
}

impl ApiError {
    fn network() -> Self {
        ApiError { status: 0, code: "NETWORK".into(), retry_after: None }
    }

    fn friendly(&self) -> String {
        match self.code.as_str() {
            "NETWORK" => UNREACHABLE.into(),
            "INVALID_OTP" => "That code isn't right. Check it and try again.".into(),
            "OTP_EXPIRED" => "That code has expired. Send a new one.".into(),
            "TOO_MANY_ATTEMPTS" => "Too many wrong codes. Send a new one.".into(),
            "INVALID_EMAIL" => "Enter a valid email address.".into(),
            "INVALID_TOKEN" => "That sign-in code has expired or was already used. Start again.".into(),
            "STATE_MISMATCH" | "INVALID_CODE_VERIFIER" => {
                "That code belongs to a different sign-in. Start again.".into()
            }
            _ if self.status == 429 => match self.retry_after {
                Some(s) if s > 1 => format!("Too many tries. Wait {} seconds, then try again.", s),
                _ => "Too many tries. Wait a minute, then try again.".into(),
            },
            _ => format!(
                "Yap's account service had a problem (error {}). Try again in a moment.",
                self.status
            ),
        }
    }
}

/// JSON body of a 2xx reply, or the error Better Auth describes. Bodies are
/// never logged: they can carry tokens.
async fn read_api(resp: reqwest::Response) -> Result<Value, ApiError> {
    let status = resp.status().as_u16();
    let retry_after = resp
        .headers()
        .get("x-retry-after")
        .or_else(|| resp.headers().get("retry-after"))
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok());
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    if (200..300).contains(&status) {
        return Ok(body);
    }
    let code = body
        .get("code")
        .and_then(|c| c.as_str())
        .unwrap_or_default()
        .to_string();
    tracing::warn!("auth: service replied {} {}", status, code);
    Err(ApiError { status, code, retry_after })
}

// ---- crypto helpers ----

/// Uniform random `[A-Za-z0-9]` (rejection sampling: no modulo bias).
fn random_alnum(len: usize) -> Result<String, String> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut out = String::with_capacity(len);
    while out.len() < len {
        let mut buf = [0u8; 32];
        getrandom::fill(&mut buf).map_err(|e| e.to_string())?;
        for b in buf {
            if b < 248 && out.len() < len {
                out.push(ALPHABET[(b % 62) as usize] as char);
            }
        }
    }
    Ok(out)
}

/// RFC 7636 S256 pair: 32 random bytes as the verifier, its hash as the
/// challenge (both base64url, unpadded).
fn pkce_pair() -> Result<(String, String), String> {
    let mut raw = [0u8; 32];
    getrandom::fill(&mut raw).map_err(|e| e.to_string())?;
    let verifier = URL_SAFE_NO_PAD.encode(raw);
    let challenge = pkce_challenge(&verifier);
    Ok((verifier, challenge))
}

fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// Compare secrets without leaking length/prefix timing: hash both sides.
fn same_secret(a: &str, b: &str) -> bool {
    Sha256::digest(a.as_bytes()) == Sha256::digest(b.as_bytes())
}

// ---- credential store ----

#[cfg(windows)]
mod vault {
    use super::Stored;
    use std::collections::HashMap;
    use std::sync::Once;

    const SERVICE: &str = "com.yap.dictation";
    /// Credential Manager blobs top out at 2560 bytes (UTF-16, so ~1280 chars).
    const MAX_CHARS: usize = 1200;

    fn entry() -> Result<keyring_core::Entry, keyring_core::Error> {
        static STORE: Once = Once::new();
        STORE.call_once(|| match windows_native_keyring_store::Store::new() {
            Ok(store) => keyring_core::set_default_store(store),
            Err(e) => tracing::error!("auth: credential store unavailable: {}", e),
        });
        let user = super::credential_user_for(&super::base_url());
        // Local: a session token belongs to this PC. The default (Enterprise)
        // would roam it with a domain user's profile.
        keyring_core::Entry::new_with_modifiers(SERVICE, &user, &HashMap::from([("persistence", "Local")]))
    }

    pub fn load() -> Option<Stored> {
        match entry().and_then(|e| e.get_password()) {
            Ok(json) => serde_json::from_str(&json).ok(),
            Err(keyring_core::Error::NoEntry) => None,
            Err(e) => {
                tracing::error!("auth: couldn't read the stored session: {}", e);
                None
            }
        }
    }

    pub fn save(stored: &Stored) -> Result<(), String> {
        let mut json = serde_json::to_string(stored).map_err(|e| e.to_string())?;
        if json.chars().count() > MAX_CHARS {
            // The avatar URL is the only unbounded field; it's refetched online.
            let mut slim = stored.clone();
            slim.user.image = None;
            json = serde_json::to_string(&slim).map_err(|e| e.to_string())?;
        }
        entry().and_then(|e| e.set_password(&json)).map_err(|e| e.to_string())
    }

    pub fn clear() {
        match entry().and_then(|e| e.delete_credential()) {
            Ok(()) | Err(keyring_core::Error::NoEntry) => {}
            Err(e) => tracing::error!("auth: couldn't remove the stored session: {}", e),
        }
    }
}

/// No credential store wired up off Windows yet: sessions last one run.
#[cfg(not(windows))]
mod vault {
    use super::Stored;

    pub fn load() -> Option<Stored> {
        None
    }
    pub fn save(_: &Stored) -> Result<(), String> {
        Ok(())
    }
    pub fn clear() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code_for(identifier: &str, state: &str) -> String {
        URL_SAFE_NO_PAD.encode(json!({ "identifier": identifier, "state": state }).to_string())
    }

    #[test]
    fn pkce_matches_rfc7636_vector() {
        // RFC 7636 Appendix B.
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let (v, c) = pkce_pair().unwrap();
        assert_eq!(v.len(), 43);
        assert_eq!(pkce_challenge(&v), c);
    }

    #[test]
    fn random_alnum_is_alnum_and_sized() {
        let s = random_alnum(16).unwrap();
        assert_eq!(s.len(), 16);
        assert!(s.bytes().all(|b| b.is_ascii_alphanumeric()));
        assert_ne!(s, random_alnum(16).unwrap());
    }

    #[test]
    fn parse_handoff_accepts_every_shape() {
        let code = code_for("abc123", "Nonce-51234");
        let check = |input: &str| {
            let h = parse_handoff(input).expect(input);
            assert_eq!((h.identifier.as_str(), h.state.as_str()), ("abc123", "Nonce-51234"));
        };
        check(&code);
        check(&format!("  {}\n", code));
        check(&format!("{}==", code)); // padded
        check(&code.replace('-', "%2D")); // %-escaped
        check(&format!("com.contextmirror.yap://auth/callback#token={}", code));
        check(&format!("http://127.0.0.1:51234/callback?token={}&x=1", code));
    }

    #[test]
    fn parse_handoff_rejects_junk() {
        assert!(parse_handoff("").is_none());
        assert!(parse_handoff("not base64 at all!").is_none());
        assert!(parse_handoff(&URL_SAFE_NO_PAD.encode("{\"identifier\":\"\",\"state\":\"x\"}")).is_none());
        assert!(parse_handoff(&code_for("id with space", "s")).is_none());
    }

    #[test]
    fn credentials_are_kept_per_service() {
        // Production keeps the original entry, so existing sign-ins survive.
        assert_eq!(credential_user_for("https://auth.contextmirror.com"), "yap-account");
        assert_eq!(credential_user_for("http://localhost:8787"), "yap-account@localhost:8787");
        assert_ne!(credential_user_for("https://staging.example.com"), "yap-account");
    }

    #[test]
    fn email_normalisation() {
        assert_eq!(normalize_email("  Me@Example.COM ").unwrap(), "me@example.com");
        for bad in ["", "me", "me@", "@example.com", "me@example", "me @example.com", "me@.com"] {
            assert!(normalize_email(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn friendly_errors() {
        let e = |status, code: &str, retry_after| ApiError { status, code: code.into(), retry_after };
        assert!(e(400, "INVALID_OTP", None).friendly().contains("isn't right"));
        assert!(e(429, "", Some(42)).friendly().contains("42 seconds"));
        assert!(e(429, "", None).friendly().contains("a minute"));
        assert!(e(500, "", None).friendly().contains("error 500"));
        assert_eq!(ApiError::network().friendly(), UNREACHABLE);
    }

    #[test]
    fn session_from_requires_token_and_user() {
        let ok = json!({ "token": "t", "user": { "id": "u", "email": "e@x.io", "name": "", "image": null } });
        assert_eq!(session_from(&ok).unwrap().1.email, "e@x.io");
        assert!(session_from(&json!({ "token": "", "user": ok["user"] })).is_none());
        assert!(session_from(&json!({ "user": ok["user"] })).is_none());
    }
}

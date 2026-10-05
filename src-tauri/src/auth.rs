//! Yap accounts: optional sign-in against the Yap account service (`cloud/`,
//! Better Auth on Cloudflare at auth.contextmirror.com). Nothing in dictation
//! depends on it.
//!
//! Every route ends with a Better Auth session token, kept in Windows
//! Credential Manager (never in config.json, the logs, or the webview) and
//! sent as `Authorization: Bearer <token>`:
//! - **Email code**: Yap calls the API directly (send a code, verify it).
//! - **Google / GitHub / Discord** (RFC 8252): the system browser does the
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
//! - **Phone** (RFC 8628 device authorization): Yap shows a QR code for the
//!   service's `/device` page; the person signs in on their phone, checks the
//!   code matches and approves, while Yap polls for the session. A code that
//!   lapses stays on the page, dimmed, until the page asks for a new one.
//!
//! Signed in, Settings → Account also lists where the account is signed in
//! (each Yap install, and any browser) and can sign the others out.
//!
//! Server endpoints and the handoff protocol: `cloud/README.md`.

use std::sync::atomic::{AtomicU64, Ordering};
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
const PROVIDERS: &[&str] = &["google", "github", "discord"];
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
const NOT_SIGNED_IN: &str = "You're not signed in.";
const SIGNED_OUT: &str = "This PC has been signed out of your Yap account. Sign in again to carry on.";
const NEW_PHONE_SESSION: &str = "For your security, a sign-in made with your phone can't do this during its first day. \
     Sign in another way (Google, GitHub, Discord or an email code) to carry on.";

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
    /// Linked sign-in providers ("google", "github", "discord"). Email codes
    /// work for every account, so they aren't listed.
    providers: Vec<String>,
    /// Provider of a browser sign-in in progress.
    pending: Option<String>,
    /// A phone sign-in in progress: the QR code and code to show. Its code
    /// lapsing doesn't end it: the view stays, `expired`, for the page to
    /// offer a new one.
    device: Option<DeviceView>,
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
    // Read before taking STATE: the two locks are never held together.
    let device = device_lock().as_ref().map(|flow| flow.view.clone());
    let inner = lock();
    Status {
        signed_in: inner.session.is_some(),
        user: inner.session.as_ref().map(|s| s.user.clone()),
        providers: inner.providers.clone(),
        pending: inner.pending.as_ref().map(|p| p.provider.clone()),
        device,
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

// ---- browser sign-in (Google / GitHub / Discord) ----

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
    // `com.contextmirror.yap://account` (the "Open Yap" button on
    // auth.contextmirror.com/security, linked from the new-sign-in email):
    // show Settings → Account, where the device list is.
    if url.scheme() == SCHEME && url.host_str() == Some("account") {
        let _ = crate::commands::show_settings(app);
        let _ = app.emit("yap-open-settings", "account");
        return;
    }
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
    // On success finish_sign_in emits (pending → signed in in one step).
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

// ---- phone sign-in (OAuth 2.0 device authorization, RFC 8628) ----
//
// Yap asks the service for a code and shows it as a QR code for
// `…/device?user_code=…` (cloud/public/device.html). The person signs in on
// their phone, any way the service offers, and approves the code there,
// while Yap polls `/device/token`; once approved, that answers with a
// session token, kept like any other.

/// RFC 8628 §3.4: the grant type Yap polls with.
const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
/// RFC 8628 §3.5: every `slow_down` adds this to the polling interval.
const SLOW_DOWN_STEP: Duration = Duration::from_secs(5);
const PHONE_DENIED: &str = "The sign-in was denied on your phone.";
const PHONE_CODE_GONE: &str = "That sign-in code is no longer valid. Start again.";
const UNEXPECTED: &str = "The account service sent an unexpected reply.";

/// What the Account page shows while Yap waits for the phone (`Status::device`).
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    /// The code to check on the phone, grouped for reading: "WDJB-MJHT".
    user_code: String,
    /// Where to type the code by hand (`…/device`).
    verification_uri: String,
    /// What the QR code opens (`…/device?user_code=…`).
    verification_uri_complete: String,
    /// The code's lifetime as granted, in seconds.
    expires_in: u64,
    /// When the code lapses (unix seconds): what the page counts down to.
    expires_at: u64,
    /// The service has let the code lapse (or Yap's own deadline passed with
    /// the service still answering). Yap has stopped polling it; the page
    /// hides it and offers a new one (`auth_device_start` with `renew`).
    expired: bool,
    qr: Qr,
}

/// A QR code as SVG path data, in modules: `size` square, quiet zone not
/// included (the page adds the standard 4 modules around it).
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Qr {
    size: usize,
    path: String,
}

/// `/device/code`'s answer (RFC 8628 §3.2), checked. The device code is
/// Yap's polling secret: it never reaches the UI or the logs.
struct DeviceGrant {
    device_code: String,
    interval: Duration,
    lifetime: Duration,
    view: DeviceView,
}

impl DeviceGrant {
    /// `None` unless every field is sane and both links lead to the account
    /// service itself (`base`): the QR code must never send a phone elsewhere.
    fn parse(body: &Value, base: &str, now: u64) -> Option<Self> {
        let text = |key: &str| body.get(key).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty());
        let sane = |s: &str, max: usize| s.len() <= max && s.bytes().all(|b| b.is_ascii_graphic());
        let device_code = text("device_code").filter(|s| sane(s, 191))?;
        let user_code = text("user_code").filter(|s| sane(s, 32))?;
        let verification_uri = text("verification_uri")?;
        // Optional in RFC 8628 (Better Auth always sends it).
        let complete = match text("verification_uri_complete") {
            Some(link) => link.to_string(),
            None => {
                let mut link = url::Url::parse(verification_uri).ok()?;
                link.query_pairs_mut().append_pair("user_code", user_code);
                link.to_string()
            }
        };
        if !same_origin(verification_uri, base) || !same_origin(&complete, base) {
            return None;
        }
        let expires_in = body.get("expires_in")?.as_u64().filter(|&s| s > 0)?.min(30 * 60);
        let interval = body.get("interval").and_then(Value::as_u64).unwrap_or(5).clamp(1, 60);
        Some(DeviceGrant {
            device_code: device_code.to_string(),
            interval: Duration::from_secs(interval),
            lifetime: Duration::from_secs(expires_in),
            view: DeviceView {
                user_code: group_user_code(user_code),
                verification_uri: verification_uri.to_string(),
                qr: qr_for(&complete)?,
                verification_uri_complete: complete,
                expires_in,
                expires_at: now + expires_in,
                expired: false,
            },
        })
    }
}

/// `true` when `link` is an http(s) URL on `base`'s origin.
fn same_origin(link: &str, base: &str) -> bool {
    match (url::Url::parse(link), url::Url::parse(base)) {
        (Ok(a), Ok(b)) => matches!(a.scheme(), "https" | "http") && a.origin() == b.origin(),
        _ => false,
    }
}

/// "WDJBMJHT" → "WDJB-MJHT" (RFC 8628 §6.1); other shapes stay as they are.
fn group_user_code(code: &str) -> String {
    if code.len() == 8 && code.bytes().all(|b| b.is_ascii_alphanumeric()) {
        format!("{}-{}", &code[..4], &code[4..])
    } else {
        code.to_string()
    }
}

/// The QR code for `text` (error correction M) as one SVG path: each run of
/// dark modules on a row is a `M{x} {y}h{n}v1h-{n}z` rectangle.
fn qr_for(text: &str) -> Option<Qr> {
    use qrcode::{Color, EcLevel, QrCode};
    use std::fmt::Write;
    let code = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M).ok()?;
    let size = code.width();
    let mut path = String::new();
    for (y, row) in code.to_colors().chunks(size).enumerate() {
        let mut x = 0;
        while x < size {
            if row[x] != Color::Dark {
                x += 1;
                continue;
            }
            let start = x;
            while x < size && row[x] == Color::Dark {
                x += 1;
            }
            let _ = write!(path, "M{} {}h{n}v1h-{n}z", start, y, n = x - start);
        }
    }
    Some(Qr { size, path })
}

/// The phone sign-in in progress (at most one): `id` tells a stale poller
/// that it was cancelled or replaced.
struct DeviceFlow {
    id: u64,
    view: DeviceView,
    /// Approved on the phone: Yap is signing in with it now, so no new code
    /// may replace it (that would throw the approval away). Cancel still can.
    approved: bool,
}

impl DeviceFlow {
    fn new(id: u64, view: DeviceView) -> Self {
        DeviceFlow { id, view, approved: false }
    }
}

static DEVICE: Mutex<Option<DeviceFlow>> = Mutex::new(None);
static DEVICE_IDS: AtomicU64 = AtomicU64::new(1);

fn device_lock() -> MutexGuard<'static, Option<DeviceFlow>> {
    match DEVICE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn device_is_current(id: u64) -> bool {
    device_lock().as_ref().is_some_and(|flow| flow.id == id)
}

/// Makes `flow` the phone sign-in in progress. A renewal (`renew`) only
/// replaces a code still on the page, not one cancelled meanwhile, and
/// nothing replaces an approved one. `false` if `flow` wasn't taken: its
/// code then simply lapses, unused.
fn device_begin(flow: DeviceFlow, renew: bool) -> bool {
    let mut slot = device_lock();
    let free = match slot.as_ref() {
        Some(current) => !current.approved,
        None => !renew,
    };
    if free {
        *slot = Some(flow);
    }
    free
}

/// Changes flow `id`; `false` if it had already ended (cancelled, replaced).
fn device_update(id: u64, change: impl FnOnce(&mut DeviceFlow)) -> bool {
    match device_lock().as_mut().filter(|flow| flow.id == id) {
        Some(flow) => {
            change(flow);
            true
        }
        None => false,
    }
}

/// Ends flow `id`; `false` if it had already ended (cancelled, replaced).
fn device_end(id: u64) -> bool {
    let mut flow = device_lock();
    let current = flow.as_ref().is_some_and(|f| f.id == id);
    if current {
        *flow = None;
    }
    current
}

/// One answer from `/device/token`.
#[derive(Debug, PartialEq)]
enum PollReply {
    /// The session token: approved on the phone.
    Approved(String),
    /// Not approved yet: ask again after the interval.
    Pending,
    /// Asked too often (RFC 8628 §3.5): poll more slowly from now on.
    SlowDown,
    /// Rate-limited: wait this many seconds once, then carry on.
    Wait(u64),
    /// A hiccup on the way (server error, no connection): ask again.
    Retry,
    Denied,
    Expired,
    /// Nothing more to wait for; the message says why.
    Failed(String),
}

fn classify_poll(status: u16, body: &Value, retry_after: Option<u64>) -> PollReply {
    if (200..300).contains(&status) {
        return match body.get("access_token").and_then(Value::as_str) {
            Some(token) if !token.is_empty() && token.bytes().all(|b| b.is_ascii_graphic()) => {
                PollReply::Approved(token.to_string())
            }
            _ => PollReply::Failed(UNEXPECTED.into()),
        };
    }
    match body.get("error").and_then(Value::as_str).unwrap_or_default() {
        "authorization_pending" => PollReply::Pending,
        "slow_down" => PollReply::SlowDown,
        "access_denied" => PollReply::Denied,
        "expired_token" => PollReply::Expired,
        // Unknown or already-used code (or another client's).
        "invalid_grant" => PollReply::Failed(PHONE_CODE_GONE.into()),
        _ if status == 429 => PollReply::Wait(retry_after.unwrap_or(10).clamp(1, 60)),
        // Includes the 503 the service gives a call that got stuck.
        _ if status >= 500 => PollReply::Retry,
        _ => PollReply::Failed(format!(
            "Yap's account service had a problem (error {}). Try again in a moment.",
            status
        )),
    }
}

/// How a phone sign-in that won't be approved stops.
#[derive(Debug, PartialEq)]
enum PhoneEnd {
    /// The code lapsed. Not an error (codes lapse all the time): the page
    /// dims it and offers a new one.
    Expired,
    /// The attempt is over, for the reason given.
    Failed(String),
}

/// Whether `reply` stops the wait (`None`: keep polling; approvals are
/// handled before this). Past the code's lifetime the service should already
/// have said "expired": still answering, the code has lapsed all the same;
/// not answering, Yap can't tell what happened on the phone.
fn phone_end(reply: &PollReply, past_deadline: bool) -> Option<PhoneEnd> {
    match reply {
        PollReply::Approved(_) => None,
        PollReply::Denied => Some(PhoneEnd::Failed(PHONE_DENIED.into())),
        PollReply::Expired => Some(PhoneEnd::Expired),
        PollReply::Failed(message) => Some(PhoneEnd::Failed(message.clone())),
        PollReply::Retry if past_deadline => Some(PhoneEnd::Failed(UNREACHABLE.into())),
        PollReply::Pending | PollReply::SlowDown | PollReply::Wait(_) if past_deadline => Some(PhoneEnd::Expired),
        PollReply::Pending | PollReply::SlowDown | PollReply::Wait(_) | PollReply::Retry => None,
    }
}

async fn poll_once(device_code: &str) -> PollReply {
    let sent = CLIENT
        .post(api("/device/token"))
        .json(&json!({ "grant_type": DEVICE_GRANT, "device_code": device_code, "client_id": CLIENT_ID }))
        .send()
        .await;
    let Ok(resp) = sent else {
        return PollReply::Retry;
    };
    let status = resp.status().as_u16();
    let retry_after = resp
        .headers()
        .get("x-retry-after")
        .or_else(|| resp.headers().get("retry-after"))
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok());
    // Not read_api: "pending" every few seconds is no warning, and the body
    // carries the token.
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    classify_poll(status, &body, retry_after)
}

/// Sign in with your phone: get a code from the service and wait for the
/// phone to approve it. The QR code and code show up in the status
/// (`device`); a newer call replaces an attempt still waiting, or lapsed.
/// `renew` asks for a new code for the attempt on the page, so it does
/// nothing (`Ok(None)`) if that attempt ended meanwhile (cancelled, signed
/// in) or the phone has just approved it.
#[tauri::command]
pub async fn auth_device_start(app: AppHandle, renew: Option<bool>) -> Result<Option<DeviceView>, String> {
    let renew = renew.unwrap_or(false);
    let resp = CLIENT
        .post(api("/device/code"))
        .json(&json!({ "client_id": CLIENT_ID }))
        .send()
        .await
        .map_err(|_| UNREACHABLE.to_string())?;
    let body = read_api(resp).await.map_err(|e| e.friendly())?;
    let grant = DeviceGrant::parse(&body, &base_url(), unix_now()).ok_or(UNEXPECTED)?;
    let id = DEVICE_IDS.fetch_add(1, Ordering::Relaxed);
    let view = grant.view.clone();
    if !device_begin(DeviceFlow::new(id, view.clone()), renew) {
        return Ok(None);
    }
    tracing::info!("auth: phone sign-in {}", if renew { "has a new code" } else { "started" });
    emit_status(&app);
    tauri::async_runtime::spawn(wait_for_phone(app, id, grant));
    Ok(Some(view))
}

/// Stop waiting for the phone. The service can't withdraw a code, so it
/// simply lapses: approving it on the phone afterwards signs nothing in.
#[tauri::command]
pub fn auth_device_cancel(app: AppHandle) {
    if device_lock().take().is_some() {
        tracing::info!("auth: phone sign-in cancelled");
    }
    emit_status(&app);
}

/// Polls until the phone decides, the code lapses, or the attempt is
/// cancelled or replaced. The service has the last word on expiry; Yap's own
/// deadline only ends a wait it can no longer confirm (e.g. offline).
async fn wait_for_phone(app: AppHandle, id: u64, grant: DeviceGrant) {
    let deadline = tokio::time::Instant::now() + grant.lifetime;
    let mut interval = grant.interval;
    loop {
        tokio::time::sleep(interval).await;
        if !device_is_current(id) {
            return;
        }
        let reply = poll_once(&grant.device_code).await;
        if let PollReply::Approved(token) = reply {
            return finish_phone(&app, id, token).await;
        }
        match phone_end(&reply, tokio::time::Instant::now() >= deadline) {
            // Kept, dimmed, for the page to renew: nothing to report.
            Some(PhoneEnd::Expired) => {
                if device_update(id, |flow| flow.view.expired = true) {
                    tracing::info!("auth: phone sign-in code expired");
                    emit_status(&app);
                }
                return;
            }
            Some(PhoneEnd::Failed(message)) => {
                if device_end(id) {
                    tracing::info!("auth: phone sign-in ended without signing in");
                    emit_status(&app);
                    emit_error(&app, &message);
                }
                return;
            }
            None => {}
        }
        match reply {
            PollReply::SlowDown => interval += SLOW_DOWN_STEP,
            PollReply::Wait(secs) => tokio::time::sleep(Duration::from_secs(secs)).await,
            _ => {}
        }
    }
}

/// Approved: find out whose session it is, then sign in like the other
/// routes. Cancelled meanwhile? Then the new session is revoked instead.
async fn finish_phone(app: &AppHandle, id: u64, token: String) {
    // From here on a new code can't replace this attempt; Cancel still can.
    let mut user = None;
    if device_update(id, |flow| flow.approved = true) {
        // The token answer doesn't say who signed in; get-session does.
        for attempt in 0..3 {
            match get_session(&token).await {
                Ok(found) => {
                    user = found;
                    break;
                }
                Err(_) if attempt < 2 => tokio::time::sleep(Duration::from_secs(2)).await,
                Err(_) => {}
            }
        }
    }
    if !device_is_current(id) {
        end_session(&token).await;
        tracing::info!("auth: phone approved a cancelled sign-in; session revoked");
        return;
    }
    let Some(user) = user else {
        // A session Yap can't put a name to is no use, and left alone it would
        // sit in "Where you're signed in" for 30 days: revoke it (best effort).
        if !end_session(&token).await {
            tracing::info!("auth: couldn't revoke the unconfirmed phone session");
        }
        if device_end(id) {
            emit_status(app);
            emit_error(app, "Couldn't finish signing in. Start again from Settings → Account.");
        }
        return;
    };
    // Ends this attempt too, in the same update that shows the sign-in: the
    // page never flashes the signed-out view in between.
    finish_sign_in(app, token, user).await;
    tracing::info!("auth: signed in (phone)");
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod phone_tests {
    use super::*;

    const BASE: &str = "https://auth.contextmirror.com";

    /// What the service answers at /device/code (shape checked on wrangler dev).
    fn grant_body() -> Value {
        json!({
            "device_code": "WcFjcSVVttGsfrrmdAua891bzFIKHnt2QVjiuNhO",
            "user_code": "WDJBMJHT",
            "verification_uri": "https://auth.contextmirror.com/device",
            "verification_uri_complete": "https://auth.contextmirror.com/device?user_code=WDJBMJHT",
            "expires_in": 600,
            "interval": 5
        })
    }

    /// Redraws a QR path as a module grid (true = dark).
    fn grid_from_path(qr: &Qr) -> Vec<bool> {
        let mut grid = vec![false; qr.size * qr.size];
        for rect in qr.path.split('M').filter(|s| !s.is_empty()) {
            let (xy, rest) = rect.split_once('h').expect("h");
            let (x, y) = xy.split_once(' ').expect("x y");
            let (x, y): (usize, usize) = (x.parse().unwrap(), y.parse().unwrap());
            let n: usize = rest.split_once('v').expect("v").0.parse().unwrap();
            assert_eq!(rest, format!("{n}v1h-{n}z"), "{rect}");
            for dx in 0..n {
                assert!(!grid[y * qr.size + x + dx], "overlapping runs");
                grid[y * qr.size + x + dx] = true;
            }
        }
        grid
    }

    #[test]
    fn grant_parses_into_what_the_page_shows() {
        let g = DeviceGrant::parse(&grant_body(), BASE, 1_000).expect("valid grant");
        assert_eq!(g.device_code, "WcFjcSVVttGsfrrmdAua891bzFIKHnt2QVjiuNhO");
        assert_eq!((g.interval, g.lifetime), (Duration::from_secs(5), Duration::from_secs(600)));
        assert_eq!(g.view.user_code, "WDJB-MJHT");
        assert_eq!(g.view.verification_uri, "https://auth.contextmirror.com/device");
        assert_eq!(g.view.verification_uri_complete, "https://auth.contextmirror.com/device?user_code=WDJBMJHT");
        assert_eq!((g.view.expires_in, g.view.expires_at), (600, 1_600));
        assert_eq!(g.view.qr, qr_for(&g.view.verification_uri_complete).unwrap());
    }

    #[test]
    fn the_device_code_never_reaches_the_ui() {
        let g = DeviceGrant::parse(&grant_body(), BASE, 0).unwrap();
        let view = serde_json::to_value(&g.view).unwrap();
        let keys: Vec<&str> = view.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys.len(), 7);
        for key in ["userCode", "verificationUri", "verificationUriComplete", "expiresIn", "expiresAt", "expired", "qr"] {
            assert!(keys.contains(&key), "{key}");
        }
        assert_eq!(view["expired"], json!(false), "a new code is live");
        assert!(!view.to_string().contains(&g.device_code));
    }

    #[test]
    fn grant_links_must_stay_on_the_account_service() {
        let with = |key: &str, value: Value| {
            let mut body = grant_body();
            body[key] = value;
            DeviceGrant::parse(&body, BASE, 0)
        };
        assert!(with("verification_uri_complete", json!("https://evil.example/device?user_code=WDJBMJHT")).is_none());
        assert!(with("verification_uri", json!("https://auth.contextmirror.com.evil.example/device")).is_none());
        assert!(with("verification_uri", json!("http://auth.contextmirror.com/device")).is_none()); // scheme
        assert!(with("verification_uri", json!("https://auth.contextmirror.com:444/device")).is_none()); // port
        assert!(with("verification_uri_complete", json!("javascript:alert(1)")).is_none());
        // Missing complete link: built from the plain one.
        let mut body = grant_body();
        body.as_object_mut().unwrap().remove("verification_uri_complete");
        let g = DeviceGrant::parse(&body, BASE, 0).unwrap();
        assert_eq!(g.view.verification_uri_complete, "https://auth.contextmirror.com/device?user_code=WDJBMJHT");
        // Dev: the local service.
        let local = json!({
            "device_code": "abc", "user_code": "WDJBMJHT", "expires_in": 600, "interval": 5,
            "verification_uri": "http://localhost:8787/device",
            "verification_uri_complete": "http://localhost:8787/device?user_code=WDJBMJHT",
        });
        assert!(DeviceGrant::parse(&local, "http://localhost:8787", 0).is_some());
        assert!(DeviceGrant::parse(&local, BASE, 0).is_none());
    }

    #[test]
    fn grant_rejects_junk_and_clamps_timing() {
        let with = |key: &str, value: Value| {
            let mut body = grant_body();
            body[key] = value;
            DeviceGrant::parse(&body, BASE, 0)
        };
        assert!(with("device_code", json!("")).is_none());
        assert!(with("device_code", json!("has space")).is_none());
        assert!(with("device_code", json!("x".repeat(192))).is_none());
        assert!(with("user_code", json!(null)).is_none());
        assert!(with("expires_in", json!(0)).is_none());
        assert!(with("expires_in", json!("600")).is_none());
        assert_eq!(with("expires_in", json!(86_400)).unwrap().lifetime, Duration::from_secs(1800));
        assert_eq!(with("interval", json!(null)).unwrap().interval, Duration::from_secs(5));
        assert_eq!(with("interval", json!(0)).unwrap().interval, Duration::from_secs(1));
        assert_eq!(with("interval", json!(999)).unwrap().interval, Duration::from_secs(60));
        assert!(DeviceGrant::parse(&json!(null), BASE, 0).is_none());
    }

    #[test]
    fn user_codes_are_grouped_for_reading() {
        assert_eq!(group_user_code("WDJBMJHT"), "WDJB-MJHT");
        assert_eq!(group_user_code("ABC123"), "ABC123");
        assert_eq!(group_user_code("WDJB-MJH"), "WDJB-MJH");
    }

    #[test]
    fn same_origin_is_strict() {
        assert!(same_origin("https://auth.contextmirror.com/device?user_code=X", BASE));
        assert!(same_origin("https://AUTH.contextmirror.com/device", BASE));
        assert!(!same_origin("https://auth.contextmirror.com.evil.example/device", BASE));
        assert!(!same_origin("ftp://auth.contextmirror.com/device", "ftp://auth.contextmirror.com"));
        assert!(!same_origin("not a url", BASE));
        assert!(!same_origin("https://auth.contextmirror.com/device", "not a url"));
    }

    #[test]
    fn qr_path_redraws_the_exact_code() {
        let url = "https://auth.contextmirror.com/device?user_code=WDJBMJHT";
        let qr = qr_for(url).unwrap();
        assert_eq!(qr.size, 33); // version 4: small enough to scan off a screen easily
        let expected: Vec<bool> = qrcode::QrCode::with_error_correction_level(url, qrcode::EcLevel::M)
            .unwrap()
            .to_colors()
            .into_iter()
            .map(|c| c == qrcode::Color::Dark)
            .collect();
        assert_eq!(grid_from_path(&qr), expected);
        // Finder patterns in three corners, separators around them.
        let dark = |x: usize, y: usize| expected[y * qr.size + x];
        let last = qr.size - 1;
        assert!(dark(0, 0) && dark(6, 6) && dark(last, 0) && dark(0, last));
        assert!(!dark(7, 7) && !dark(last - 7, 7) && !dark(7, last - 7));
    }

    #[test]
    fn poll_replies() {
        let p = |status, body: Value, retry| classify_poll(status, &body, retry);
        assert_eq!(p(200, json!({ "access_token": "tok", "token_type": "Bearer" }), None), PollReply::Approved("tok".into()));
        assert_eq!(p(200, json!({ "access_token": "" }), None), PollReply::Failed(UNEXPECTED.into()));
        assert_eq!(p(200, json!({ "access_token": "a b" }), None), PollReply::Failed(UNEXPECTED.into()));
        assert_eq!(p(200, Value::Null, None), PollReply::Failed(UNEXPECTED.into()));
        assert_eq!(p(400, json!({ "error": "authorization_pending" }), None), PollReply::Pending);
        assert_eq!(p(400, json!({ "error": "slow_down" }), None), PollReply::SlowDown);
        assert_eq!(p(400, json!({ "error": "access_denied" }), None), PollReply::Denied);
        assert_eq!(p(400, json!({ "error": "expired_token" }), None), PollReply::Expired);
        assert_eq!(p(400, json!({ "error": "invalid_grant" }), None), PollReply::Failed(PHONE_CODE_GONE.into()));
        assert_eq!(p(429, json!({ "message": "Too many requests." }), Some(7)), PollReply::Wait(7));
        assert_eq!(p(429, Value::Null, None), PollReply::Wait(10));
        assert_eq!(p(429, Value::Null, Some(3600)), PollReply::Wait(60));
        assert_eq!(p(503, json!({ "code": "SERVICE_UNAVAILABLE" }), Some(1)), PollReply::Retry);
        assert_eq!(p(500, Value::Null, None), PollReply::Retry);
        assert!(matches!(p(400, json!({ "error": "invalid_request" }), None), PollReply::Failed(m) if m.contains("error 400")));
    }

    #[test]
    fn how_a_wait_ends() {
        let end = |reply: PollReply, past_deadline| phone_end(&reply, past_deadline);
        // In time: the in-between replies keep Yap polling.
        for reply in [PollReply::Pending, PollReply::SlowDown, PollReply::Wait(7), PollReply::Retry] {
            assert_eq!(end(reply, false), None);
        }
        // The service let the code lapse: no error, the page renews it.
        assert_eq!(end(PollReply::Expired, false), Some(PhoneEnd::Expired));
        // Past Yap's deadline with the service still answering: lapsed too.
        for reply in [PollReply::Pending, PollReply::SlowDown, PollReply::Wait(7)] {
            assert_eq!(end(reply, true), Some(PhoneEnd::Expired));
        }
        // Past it with no answer at all: an error, not a quiet new code.
        assert_eq!(end(PollReply::Retry, true), Some(PhoneEnd::Failed(UNREACHABLE.into())));
        assert_eq!(end(PollReply::Denied, false), Some(PhoneEnd::Failed(PHONE_DENIED.into())));
        assert_eq!(
            end(PollReply::Failed(PHONE_CODE_GONE.into()), false),
            Some(PhoneEnd::Failed(PHONE_CODE_GONE.into()))
        );
        // Approvals are handled before this, even a late one.
        assert_eq!(end(PollReply::Approved("tok".into()), true), None);
    }

    /// The tests that use the one DEVICE slot take turns.
    fn one_at_a_time() -> MutexGuard<'static, ()> {
        static TURN: Mutex<()> = Mutex::new(());
        TURN.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn one_phone_sign_in_at_a_time() {
        let _turn = one_at_a_time();
        let g = DeviceGrant::parse(&grant_body(), BASE, 0).unwrap();
        *device_lock() = Some(DeviceFlow::new(41, g.view.clone()));
        assert_eq!(status().device.as_ref(), Some(&g.view));
        // A newer attempt replaces it: the old poller sees it's no longer current.
        assert!(device_begin(DeviceFlow::new(42, g.view.clone()), false));
        assert!(!device_is_current(41) && device_is_current(42));
        assert!(!device_end(41), "a stale poller can't end the newer attempt");
        assert!(device_end(42));
        assert!(!device_end(42) && status().device.is_none());
    }

    #[test]
    fn an_expired_code_stays_on_the_page_for_a_new_one() {
        let _turn = one_at_a_time();
        let view = DeviceGrant::parse(&grant_body(), BASE, 0).unwrap().view;
        *device_lock() = Some(DeviceFlow::new(51, view.clone()));
        assert!(!device_update(50, |flow| flow.view.expired = true), "a stale poller can't mark it");
        assert!(device_update(51, |flow| flow.view.expired = true));
        // Still the attempt in progress, now flagged for the page.
        assert!(device_is_current(51));
        let shown = status().device.expect("kept");
        let json = serde_json::to_value(&shown).unwrap();
        assert_eq!((json["expired"].clone(), json["userCode"].clone()), (json!(true), json!("WDJB-MJHT")));
        // A new code takes its place, live.
        assert!(device_begin(DeviceFlow::new(52, view), true));
        assert!(!device_is_current(51));
        assert_eq!(status().device.map(|d| d.expired), Some(false));
        assert!(device_end(52));
    }

    #[test]
    fn a_new_code_never_revives_a_cancelled_attempt_or_drops_an_approved_one() {
        let _turn = one_at_a_time();
        let view = DeviceGrant::parse(&grant_body(), BASE, 0).unwrap().view;
        *device_lock() = None;
        // Cancelled while the new code was on its way: stays cancelled.
        assert!(!device_begin(DeviceFlow::new(61, view.clone()), true));
        assert!(status().device.is_none());
        // Starting afresh always goes ahead.
        assert!(device_begin(DeviceFlow::new(62, view.clone()), false));
        // Approved on the phone: Yap is signing in with it, so nothing
        // replaces it ...
        assert!(device_update(62, |flow| flow.approved = true));
        assert!(!device_begin(DeviceFlow::new(63, view.clone()), true));
        assert!(!device_begin(DeviceFlow::new(64, view), false));
        assert!(device_is_current(62));
        // ... but Cancel still ends it (auth_device_cancel), and then
        // finish_phone revokes the session.
        assert!(device_lock().take().is_some());
        assert!(!device_is_current(62));
    }
}

// ---- session ----

/// Where every route ends: keep the session and show it, then fill in the
/// linked providers.
async fn finish_sign_in(app: &AppHandle, token: String, user: Profile) {
    let stored = Stored { token: token.clone(), user, signed_in_at: unix_now() };
    if let Err(e) = vault::save(&stored) {
        // Still signed in for this run; it just won't survive a restart.
        tracing::error!("auth: couldn't store the session: {}", e);
    }
    let replaced = {
        let mut inner = lock();
        // Any other sign-in left waiting is moot now.
        inner.pending = None;
        let same_account = inner.session.as_ref().is_some_and(|s| s.user.id == stored.user.id);
        let old = inner.session.replace(stored);
        if !same_account {
            inner.providers.clear();
        }
        inner.offline = false;
        old.map(|s| s.token).filter(|old| *old != token)
    };
    // A phone sign-in too (the two locks are never held together). Its
    // poller, if any, stops at its next turn.
    *device_lock() = None;
    // Signed in: say so now, and look the providers up after. Meanwhile the
    // page reads "Signs in with an email code", which holds for every account.
    emit_status(app);
    let providers = list_providers(&token).await.unwrap_or_default();
    let changed = {
        let mut inner = lock();
        // Skip if the user signed out / switched while the request ran.
        let current = inner.session.as_ref().is_some_and(|s| s.token == token);
        let changed = current && inner.providers != providers;
        if changed {
            inner.providers = providers;
        }
        changed
    };
    if changed {
        emit_status(app);
    }
    // Yap keeps one session per PC. One this sign-in replaced (signing in
    // again to confirm it's you) would otherwise stay live on the server,
    // unused, for up to 30 days, and show up as another device. The new
    // session is already in place, so Yap no longer uses the old one.
    if let Some(old) = replaced {
        if !end_session(&old).await {
            tracing::info!("auth: couldn't revoke the replaced session");
        }
    }
}

/// Revoke one of Yap's own sessions server-side. Best effort: `false` when
/// the server couldn't be reached.
async fn end_session(token: &str) -> bool {
    CLIENT
        .post(api("/sign-out"))
        .bearer_auth(token)
        .json(&json!({}))
        .send()
        .await
        .is_ok()
}

fn current_token() -> Result<String, String> {
    lock()
        .session
        .as_ref()
        .map(|s| s.token.clone())
        .ok_or_else(|| NOT_SIGNED_IN.to_string())
}

/// Sign out: revoke the session server-side (best effort, so it works
/// offline too) and forget it locally.
#[tauri::command]
pub async fn auth_sign_out(app: AppHandle) -> Result<(), String> {
    let token = lock().session.as_ref().map(|s| s.token.clone());
    if let Some(token) = token {
        if !end_session(&token).await {
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
    let token = current_token()?;
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

// ---- where you're signed in ----

/// One place the account is signed in, as Settings → Account lists it.
/// Carries neither the session's token nor its IP address: the account
/// service never sends them (cloud/src/index.ts, `/api/account/sessions`).
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceSession {
    id: String,
    /// This PC's own session.
    current: bool,
    /// "Yap 0.1.1 on Windows", "Chrome on macOS", …
    label: String,
    /// Unix seconds, like the rest of the IPC; `None` when the server didn't say.
    created_at: Option<u64>,
    /// When the server last renewed the session. That happens about once a
    /// day while the session is used, so it's only good to the day.
    last_active_at: Option<u64>,
    expires_at: Option<u64>,
}

/// The account service's own routes (outside Better Auth's /api/auth).
fn account_api(path: &str) -> String {
    format!("{}/api/account{}", base_url(), path)
}

/// Where this account is signed in: this PC first, then the most recently
/// active. Works with a session of any age (Better Auth's own /list-sessions
/// wants a sign-in from the last day and hands out every session's token, so
/// the service has its own endpoint that returns ids only).
#[tauri::command]
pub async fn auth_list_sessions(app: AppHandle) -> Result<Vec<DeviceSession>, String> {
    let token = current_token()?;
    let resp = CLIENT
        .get(account_api("/sessions"))
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|_| UNREACHABLE.to_string())?;
    match read_api(resp).await {
        Ok(body) => Ok(parse_sessions(&body)),
        Err(e) => Err(session_call_failed(&app, e).await),
    }
}

/// Sign out everywhere but this PC (Better Auth's revoke-other-sessions,
/// which works with a session of any age).
#[tauri::command]
pub async fn auth_revoke_other_sessions(app: AppHandle) -> Result<(), String> {
    let token = current_token()?;
    let resp = CLIENT
        .post(api("/revoke-other-sessions"))
        .bearer_auth(&token)
        .json(&json!({}))
        .send()
        .await
        .map_err(|_| UNREACHABLE.to_string())?;
    if let Err(e) = read_api(resp).await {
        return Err(session_call_failed(&app, e).await);
    }
    tracing::info!("auth: signed out of the other sessions");
    Ok(())
}

/// Sign out one other device, by its id from the list. This PC signs out
/// with `auth_sign_out` instead (the service refuses its own session here).
#[tauri::command]
pub async fn auth_revoke_session(app: AppHandle, id: String) -> Result<(), String> {
    let token = current_token()?;
    let resp = CLIENT
        .post(account_api("/sessions/revoke"))
        .bearer_auth(&token)
        .json(&json!({ "id": id }))
        .send()
        .await
        .map_err(|_| UNREACHABLE.to_string())?;
    if let Err(e) = read_api(resp).await {
        if e.code == "CURRENT_SESSION" {
            return Err("That's this PC. Use Sign out to sign out here.".into());
        }
        return Err(session_call_failed(&app, e).await);
    }
    tracing::info!("auth: signed out one other session");
    Ok(())
}

/// What a refused list/revoke call tells the UI. A 401 means the server no
/// longer takes this PC's session, or that its session lookup hiccuped, so
/// re-check the way startup does: get-session answers definitively, and only
/// a session that's really gone signs Yap out.
async fn session_call_failed(app: &AppHandle, e: ApiError) -> String {
    if e.status == 401 {
        refresh(app).await;
        if lock().session.is_none() {
            return SIGNED_OUT.into();
        }
    }
    e.friendly()
}

/// The service's `{ "sessions": [{ id, current, createdAt, updatedAt,
/// expiresAt, userAgent }] }` → the rows the UI shows, this PC first.
fn parse_sessions(body: &Value) -> Vec<DeviceSession> {
    let rows = body.get("sessions").and_then(Value::as_array).into_iter().flatten();
    let mut sessions: Vec<DeviceSession> = rows
        .filter_map(|row| {
            let text = |key: &str| row.get(key).and_then(Value::as_str).unwrap_or_default();
            let time = |key: &str| row.get(key).and_then(unix_secs);
            let id = text("id");
            if id.is_empty() {
                return None;
            }
            let created_at = time("createdAt");
            Some(DeviceSession {
                id: id.to_string(),
                current: row.get("current").and_then(Value::as_bool).unwrap_or(false),
                label: device_label(text("userAgent")),
                created_at,
                last_active_at: time("updatedAt").or(created_at),
                expires_at: time("expiresAt"),
            })
        })
        .collect();
    sessions.sort_by(|a, b| {
        b.current
            .cmp(&a.current)
            .then(b.last_active_at.cmp(&a.last_active_at))
            .then(b.created_at.cmp(&a.created_at))
    });
    sessions
}

/// A session's user agent as people name the device: "Yap 0.1.1 on Windows"
/// for Yap itself (`CLIENT` sends `Yap/<version> (Windows)`), "Chrome on
/// macOS" for a browser.
fn device_label(user_agent: &str) -> String {
    // First match wins, so the browsers whose user agents also claim to be
    // Chrome (Edge, Opera, Samsung) or Safari (all of them) come first.
    const BROWSERS: &[(&str, &str)] = &[
        ("Edg/", "Edge"),
        ("EdgA/", "Edge"),
        ("EdgiOS/", "Edge"),
        ("Edge/", "Edge"),
        ("OPR/", "Opera"),
        ("OPiOS/", "Opera"),
        ("SamsungBrowser/", "Samsung Internet"),
        ("Vivaldi/", "Vivaldi"),
        ("Firefox/", "Firefox"),
        ("FxiOS/", "Firefox"),
        ("CriOS/", "Chrome"),
        ("Chrome/", "Chrome"),
        ("Safari/", "Safari"),
    ];
    // iPhone/iPad say "like Mac OS X"; Android says "Linux".
    const SYSTEMS: &[(&str, &str)] = &[
        ("Windows", "Windows"),
        ("iPhone", "iPhone"),
        ("iPad", "iPad"),
        ("Android", "Android"),
        ("CrOS", "ChromeOS"),
        ("Mac OS X", "macOS"),
        ("Macintosh", "macOS"),
        ("Linux", "Linux"),
    ];

    let ua = user_agent.trim();
    if ua.is_empty() {
        return "Unknown device".into();
    }
    if let Some(rest) = ua.strip_prefix("Yap/") {
        let (version, rest) = rest.split_once(' ').unwrap_or((rest, ""));
        let os = rest.trim_start().strip_prefix('(').and_then(|r| r.split_once(')')).map(|(os, _)| os.trim());
        // Anyone can send any user agent: only show plain, short text.
        let plain = |s: &str, extra: &str| {
            !s.is_empty() && s.len() <= 32 && s.chars().all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
        };
        let app = if plain(version, ".-+") { format!("Yap {}", version) } else { "Yap".into() };
        return match os.filter(|os| plain(os, " .")) {
            Some(os) => format!("{} on {}", app, os),
            None => app,
        };
    }
    let find = |table: &[(&str, &'static str)]| table.iter().find(|(needle, _)| ua.contains(needle)).map(|(_, name)| *name);
    match (find(BROWSERS), find(SYSTEMS)) {
        (Some(browser), Some(os)) => format!("{} on {}", browser, os),
        (Some(browser), None) => browser.into(),
        (None, Some(os)) => format!("Web browser on {}", os),
        (None, None) => "Web browser".into(),
    }
}

/// A server timestamp as unix seconds. JSON carries JS dates as ISO 8601
/// strings (`2026-10-04T20:25:17.000Z`); a bare number is epoch
/// milliseconds, the JS convention.
fn unix_secs(value: &Value) -> Option<u64> {
    match value {
        Value::String(s) => parse_rfc3339(s),
        Value::Number(n) => n.as_f64().filter(|ms| ms.is_finite() && *ms >= 0.0).map(|ms| (ms / 1000.0) as u64),
        _ => None,
    }
}

/// RFC 3339 date-time → unix seconds: `2026-10-04T20:25:17Z`, with an
/// optional fraction and a `Z` or `±hh:mm` zone (none counts as UTC).
fn parse_rfc3339(s: &str) -> Option<u64> {
    let b = s.trim().as_bytes();
    let num = |at: usize, len: usize| -> Option<i64> {
        let part = b.get(at..at + len)?;
        part.iter()
            .all(u8::is_ascii_digit)
            .then(|| part.iter().fold(0, |n, d| n * 10 + i64::from(d - b'0')))
    };
    let sep = |at: usize, ok: &[u8]| b.get(at).is_some_and(|c| ok.contains(c));
    if !(sep(4, b"-") && sep(7, b"-") && sep(10, b"Tt ") && sep(13, b":") && sep(16, b":")) {
        return None;
    }
    let (year, month, day) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let (hour, minute, second) = (num(11, 2)?, num(14, 2)?, num(17, 2)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut zone = &b[19..];
    if let Some(fraction) = zone.strip_prefix(b".") {
        let digits = fraction.iter().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 {
            return None;
        }
        zone = &fraction[digits..];
    }
    let offset = match zone {
        b"" | b"Z" | b"z" => 0,
        [sign @ (b'+' | b'-'), h1, h2, b':', m1, m2] if [h1, h2, m1, m2].iter().all(|c| c.is_ascii_digit()) => {
            let two = |hi: u8, lo: u8| i64::from(hi - b'0') * 10 + i64::from(lo - b'0');
            let secs = two(*h1, *h2) * 3600 + two(*m1, *m2) * 60;
            if *sign == b'-' {
                -secs
            } else {
                secs
            }
        }
        _ => return None,
    };
    let secs = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset;
    u64::try_from(secs).ok()
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's
/// `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
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
            // The service won't let a phone sign-in under a day old delete the
            // account or sign devices out (403).
            "NEW_PHONE_SESSION" => NEW_PHONE_SESSION.into(),
            _ if self.status == 429 => match self.retry_after {
                Some(s) if s > 1 => format!("Too many tries. Wait {}, then try again.", wait_text(s)),
                _ => "Too many tries. Wait a minute, then try again.".into(),
            },
            _ => format!(
                "Yap's account service had a problem (error {}). Try again in a moment.",
                self.status
            ),
        }
    }
}

/// A server-sent wait as people say it: "42 seconds", "3 minutes", "2 hours".
/// Rounded up, so waiting that long is always enough.
fn wait_text(secs: u64) -> String {
    match secs {
        0..=89 => format!("{} seconds", secs),
        90..=5399 => format!("{} minutes", secs.div_ceil(60)),
        _ => format!("{} hours", secs.div_ceil(3600)),
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
        assert!(e(429, "", Some(42)).friendly().contains("Wait 42 seconds,"));
        assert!(e(429, "", Some(90)).friendly().contains("Wait 2 minutes,"));
        assert!(e(429, "", Some(3540)).friendly().contains("Wait 59 minutes,"));
        assert!(e(429, "", Some(5400)).friendly().contains("Wait 2 hours,"));
        assert!(e(429, "", Some(86_000)).friendly().contains("Wait 24 hours,"));
        assert!(e(429, "", None).friendly().contains("a minute"));
        assert!(e(500, "", None).friendly().contains("error 500"));
        assert_eq!(ApiError::network().friendly(), UNREACHABLE);
        // A phone sign-in under a day old deleting the account / signing
        // devices out.
        assert_eq!(
            e(403, "NEW_PHONE_SESSION", None).friendly(),
            "For your security, a sign-in made with your phone can't do this during its first day. Sign in another way (Google, GitHub, Discord or an email code) to carry on."
        );
    }

    #[test]
    fn session_from_requires_token_and_user() {
        let ok = json!({ "token": "t", "user": { "id": "u", "email": "e@x.io", "name": "", "image": null } });
        assert_eq!(session_from(&ok).unwrap().1.email, "e@x.io");
        assert!(session_from(&json!({ "token": "", "user": ok["user"] })).is_none());
        assert!(session_from(&json!({ "user": ok["user"] })).is_none());
    }

    const STRAY_TOKEN: &str = "strayTokenAAAAAAAAAAAAAAAAAAAAAA";

    /// `GET /api/account/sessions` (cloud/src/index.ts) in server order. The
    /// service sends no tokens or IPs; the stray ones here prove the UI
    /// payload drops them anyway.
    fn list_reply() -> Value {
        json!({ "sessions": [
            {
                "id": "sessLaptop", "current": false,
                "expiresAt": "2026-10-12T08:00:00.000Z", "createdAt": "2026-09-12T08:00:00.000Z",
                "updatedAt": "2026-10-02T09:30:00.000Z", "userAgent": "Yap/0.1.0 (Windows)"
            },
            {
                "id": "sessThisPc", "current": true,
                "expiresAt": "2026-11-03T20:25:17.000Z", "createdAt": "2026-10-04T20:25:17.000Z",
                "updatedAt": "2026-10-04T20:25:17.000Z", "userAgent": "Yap/0.1.1 (Windows)",
                "token": STRAY_TOKEN, "ipAddress": "198.51.100.4"
            },
            {
                "id": "sessBrowser", "current": false,
                "expiresAt": "2026-11-02T10:00:00.000Z", "createdAt": "2026-10-03T10:00:00.000Z",
                "updatedAt": "2026-10-03T10:00:00.000Z",
                "userAgent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36"
            },
            // Malformed rows are skipped, not fatal.
            { "id": "" },
            "junk"
        ]})
    }

    #[test]
    fn device_labels() {
        let cases = [
            ("Yap/0.1.1 (Windows)", "Yap 0.1.1 on Windows"),
            ("Yap/0.1.1-nightly.103 (Windows)", "Yap 0.1.1-nightly.103 on Windows"),
            ("Yap/0.2.0", "Yap 0.2.0"),
            ("Yap/<b>x</b> (Windows)", "Yap on Windows"),
            ("Yap/0.3.0 (Windows <script>)", "Yap 0.3.0"),
            (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36",
                "Chrome on Windows",
            ),
            (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36 Edg/129.0.0.0",
                "Edge on Windows",
            ),
            (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:131.0) Gecko/20100101 Firefox/131.0",
                "Firefox on Windows",
            ),
            ("Mozilla/5.0 (X11; Linux x86_64; rv:131.0) Gecko/20100101 Firefox/131.0", "Firefox on Linux"),
            (
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15",
                "Safari on macOS",
            ),
            (
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36",
                "Chrome on macOS",
            ),
            (
                "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1",
                "Safari on iPhone",
            ),
            (
                "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) CriOS/129.0.6668.69 Mobile/15E148 Safari/604.1",
                "Chrome on iPhone",
            ),
            (
                "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Mobile Safari/537.36",
                "Chrome on Android",
            ),
            (
                "Mozilla/5.0 (Linux; Android 14; SM-S918B) AppleWebKit/537.36 (KHTML, like Gecko) SamsungBrowser/26.0 Chrome/122.0.0.0 Mobile Safari/537.36",
                "Samsung Internet on Android",
            ),
            (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36 OPR/114.0.0.0",
                "Opera on Windows",
            ),
            (
                "Mozilla/5.0 (X11; CrOS x86_64 14541.0.0) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36",
                "Chrome on ChromeOS",
            ),
            ("Mozilla/5.0 (X11; Linux x86_64) SomeNewBrowser/1.0", "Web browser on Linux"),
            ("curl/8.9.1", "Web browser"),
            ("", "Unknown device"),
            ("   ", "Unknown device"),
        ];
        for (ua, label) in cases {
            assert_eq!(device_label(ua), label, "{ua}");
        }
    }

    #[test]
    fn parse_sessions_marks_this_pc_and_orders_by_activity() {
        let sessions = parse_sessions(&list_reply());
        let ids: Vec<&str> = sessions.iter().map(|s| s.id.as_str()).collect();
        // This PC first, then the most recently active.
        assert_eq!(ids, ["sessThisPc", "sessBrowser", "sessLaptop"]);
        assert_eq!(sessions.iter().filter(|s| s.current).count(), 1);
        assert_eq!(
            sessions[0],
            DeviceSession {
                id: "sessThisPc".into(),
                current: true,
                label: "Yap 0.1.1 on Windows".into(),
                created_at: Some(1_791_145_517),
                last_active_at: Some(1_791_145_517),
                expires_at: Some(1_793_737_517),
            }
        );
        assert_eq!(sessions[1].label, "Chrome on macOS");
        assert_eq!(sessions[2].label, "Yap 0.1.0 on Windows");
        assert_eq!(sessions[2].created_at, Some(1_789_200_000));

        // Anything but the service's object: no sessions.
        assert!(parse_sessions(&json!({ "code": "UNAUTHORIZED" })).is_empty());
        assert!(parse_sessions(&json!([{ "id": "bare-array" }])).is_empty());
        assert!(parse_sessions(&Value::Null).is_empty());
    }

    #[test]
    fn session_list_for_the_ui_has_no_tokens_or_ips() {
        let ui = serde_json::to_value(parse_sessions(&list_reply())).unwrap();
        let text = ui.to_string();
        for secret in [STRAY_TOKEN, "198.51.100.4"] {
            assert!(!text.contains(secret), "{secret} leaked: {text}");
        }
        assert!(!text.to_lowercase().contains("token") && !text.contains("ipAddress"), "{text}");
        for row in ui.as_array().unwrap() {
            let mut keys: Vec<&str> = row.as_object().unwrap().keys().map(String::as_str).collect();
            keys.sort_unstable();
            assert_eq!(keys, ["createdAt", "current", "expiresAt", "id", "label", "lastActiveAt"]);
        }
    }

    #[test]
    fn server_timestamps() {
        let at = |s: &str| unix_secs(&json!(s));
        assert_eq!(at("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(at("2026-10-04T20:25:17Z"), Some(1_791_145_517));
        assert_eq!(at("2026-10-04T20:25:17.123Z"), Some(1_791_145_517));
        assert_eq!(at("2026-10-04T20:25:17.123456z"), Some(1_791_145_517));
        assert_eq!(at("2026-10-04 20:25:17"), Some(1_791_145_517));
        assert_eq!(at("2026-10-04T21:25:17+01:00"), Some(1_791_145_517));
        assert_eq!(at("2026-10-04T15:55:17-04:30"), Some(1_791_145_517));
        assert_eq!(at("2000-02-29T23:59:59Z"), Some(951_868_799));
        assert_eq!(at("2024-03-01T00:00:00.000Z"), Some(1_709_251_200));
        assert_eq!(unix_secs(&json!(1_791_145_517_123u64)), Some(1_791_145_517));
        for bad in [
            "",
            "2026-10-04",
            "2026-13-04T20:25:17Z",
            "2026-10-04T24:00:00Z",
            "2026-10-04T20:25:17.Z",
            "2026-10-04T20:25:17+0100",
            "2026-10-04T20:25:17 junk",
            "1969-12-31T23:59:59Z",
            "２０２６-10-04T20:25:17Z",
        ] {
            assert_eq!(at(bad), None, "{bad}");
        }
        assert_eq!(unix_secs(&json!(-5)), None);
        assert_eq!(unix_secs(&Value::Null), None);
    }
}

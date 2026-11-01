//! **Sign in with ChatGPT** (2026-11-01): Yap's AI on the person's own
//! ChatGPT plan instead of an API key, through OpenAI's flow for open-source
//! apps that run on the person's PC (developers.openai.com/siwc/
//! token-sharing-open-source; feasibility notes in the private repo).
//!
//! - **Signing in** (`chatgpt_sign_in`): the system browser at OpenAI's
//!   consent page, the code back on a loopback listener, exchanged and its
//!   ID token checked (`oauth.rs`, `jwt.rs`). Yap registers itself with the
//!   account the first time (the issued client id kept in `chatgpt.json`
//!   before the code is used) and reuses that registration after.
//! - **The provider** "chatgpt": a keyed provider whose key store holds
//!   [`PLAN_KEY`] while signed in with the plan's permission — not a secret,
//!   a marker, so every "has this provider a key?" check works as for the
//!   others. Its calls go to OpenAI's Responses API with the access token
//!   (`ai/llm/responses.rs`, from `post_chat_request`), never the marker.
//! - **Tokens**: the refresh token, the access token and its expiry in
//!   Windows Credential Manager (`vault.rs`), never config.json, the logs or
//!   the webview. The access token lasts an hour; the refresh token 30 days,
//!   replaced on every refresh. Two Yaps share the sign-in (the dev build
//!   beside the installed app, a model evaluation): refreshes happen one at a
//!   time across processes (a lock file) and each reads the newest tokens
//!   back first, so a rotated refresh token is never used twice.
//! - **The account** (`chatgpt.json`): this PC's host id (`urn:uuid:…`, made
//!   once), the issued client id, the account's opaque id, email and name
//!   (Settings' "Connected as …"), the scopes granted, and whether the
//!   one-time "You're using your ChatGPT plan" notice was seen.
//! - **Signing out** (`chatgpt_sign_out`): the grant revoked at OpenAI, the
//!   tokens removed here; a revocation OpenAI didn't confirm is said, with
//!   where to disconnect Yap in ChatGPT's settings.
//!
//! How it works: docs/architecture/ai-models.md, "Sign in with ChatGPT".

pub mod jwt;
pub mod oauth;
pub mod vault;

use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

/// The provider id (config.json, the key store, Chat's picker).
pub const PROVIDER: &str = "chatgpt";
/// What the key store holds for [`PROVIDER`] while signed in with the
/// plan's permission: a marker, never a credential.
pub const PLAN_KEY: &str = "chatgpt-plan";
/// The tokens are for this API (and the calls go there).
pub const BASE_URL: &str = "https://api.openai.com/v1";
/// Where the person manages their plan's usage (OpenAI's UI rules: next to
/// "Using ChatGPT plan" and the usage summary).
pub const USAGE_URL: &str = "https://chatgpt.com/settings/usage";

/// **The plan's models are OpenAI's**: every rule about a model (effort,
/// images, context, the strong models) reads "chatgpt" as "openai". Pure.
pub fn as_openai(provider: &str) -> &str {
    if provider == PROVIDER {
        "openai"
    } else {
        provider
    }
}

/// A token with less than this left is refreshed first (seconds).
const MARGIN: i64 = 120;

const NOT_SIGNED_IN: &str = "Yap isn't signed in to ChatGPT. Sign in in Settings → Language Models → ChatGPT.";

/// Who is signed in (`chatgpt.json`): nothing secret.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Account {
    /// This PC's `ext_agent_host_id`, made once.
    pub host_id: String,
    /// The registration OpenAI issued Yap for this account (`oaiapp_…`).
    pub client_id: Option<String>,
    /// The account's opaque id (`sub`): a later sign-in must match it.
    pub sub: Option<String>,
    pub email: Option<String>,
    pub name: Option<String>,
    pub scopes: Vec<String>,
    pub signed_in: bool,
    /// "You're using your ChatGPT plan" was seen (shown once).
    pub notice_seen: bool,
    pub connected_ts: i64,
}

impl Account {
    /// The plan's permission was granted.
    pub fn plan(&self) -> bool {
        self.scopes.iter().any(|s| s == oauth::PLAN_SCOPE)
    }
}

/// What Credential Manager holds, as one JSON value.
#[derive(Clone, Serialize, Deserialize)]
struct Creds {
    client: String,
    sub: String,
    refresh: String,
    access: String,
    /// When the access token stops working (Unix seconds).
    expires: i64,
}

fn load_creds() -> Option<Creds> {
    vault::load().and_then(|s| serde_json::from_str(&s).ok())
}

fn save_creds(c: &Creds) -> Result<(), String> {
    vault::save(&serde_json::to_string(c).map_err(|e| e.to_string())?)
}

// ---- state ------------------------------------------------------------------------------

#[derive(Default)]
struct State {
    waiting: bool,
    attempt: u64,
    error: Option<String>,
    /// The access token this process holds, and when it stops working.
    access: Option<(String, i64)>,
    /// Test runs: the links Yap would have opened.
    opened: Vec<String>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static ACCOUNT: Mutex<Option<Account>> = Mutex::new(None);
static APP: OnceLock<AppHandle> = OnceLock::new();
/// One refresh at a time in this process (the lock file does it across).
static REFRESHING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    f(STATE.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(State::default))
}

/// chatgpt.json's saves.
pub static SAVER: crate::shell::saving::Saver = crate::shell::saving::Saver::new("chatgpt", "your ChatGPT sign-in", save_now);

fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    with_account(|_| ());
    SAVER.check()
}

fn account_path() -> std::path::PathBuf {
    crate::config::data_dir().join("chatgpt.json")
}

/// The account, made with this PC's host id the first time; `f`'s changes
/// saved.
fn with_account<R>(f: impl FnOnce(&mut Account) -> R) -> R {
    let mut guard = ACCOUNT.lock().unwrap_or_else(|p| p.into_inner());
    let account = guard.get_or_insert_with(|| SAVER.load(&account_path()));
    if account.host_id.is_empty() {
        account.host_id = host_id().unwrap_or_default();
    }
    let before = serde_json::to_string(account).unwrap_or_default();
    let out = f(account);
    if serde_json::to_string(account).unwrap_or_default() != before && !crate::ai::eval::active() {
        let _ = SAVER.write_json(&account_path(), account, true);
    }
    out
}

/// A new host id: `urn:uuid:` and a random (version 4) UUID.
fn host_id() -> Result<String, String> {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(|e| e.to_string())?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    Ok(format!("urn:uuid:{}-{}-{}-{}-{}", &h[..8], &h[8..12], &h[12..16], &h[16..20], &h[20..]))
}

fn account() -> Account {
    with_account(|a| a.clone())
}

pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// What Settings and Chat show.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub signed_in: bool,
    /// The plan's permission granted: Yap can use the plan.
    pub plan: bool,
    pub waiting: bool,
    pub email: Option<String>,
    pub name: Option<String>,
    pub notice_seen: bool,
    pub error: Option<String>,
    pub usage_url: &'static str,
}

pub fn status() -> Status {
    let a = account();
    let (waiting, error) = state(|s| (s.waiting, s.error.clone()));
    Status {
        signed_in: a.signed_in,
        plan: a.signed_in && a.plan(),
        waiting,
        email: a.email.clone(),
        name: a.name.clone(),
        notice_seen: a.notice_seen,
        error,
        usage_url: USAGE_URL,
    }
}

fn emit() {
    if let Some(app) = APP.get() {
        let _ = app.emit("yap-chatgpt", status());
    }
}

/// The key store's marker on (signed in with the plan) or off: every
/// window's copy told (`yap-chatgpt` carries the status; Settings adopts it).
fn mark_key(on: bool) {
    let Some(app) = APP.get() else { return };
    let _ = crate::shell::settings::change_config(app, |cfg| {
        if on {
            crate::ai::ai_model::set_provider_key(cfg, PROVIDER, PLAN_KEY)
        } else {
            crate::ai::ai_model::clear_provider_key(cfg, PROVIDER);
            Ok(())
        }
    });
}

// ---- signing in -------------------------------------------------------------------------

fn open_link(app: &AppHandle, link: &str) -> Result<(), String> {
    if crate::shell::e2e::active() {
        tracing::info!("e2e: would open ChatGPT's sign-in");
        state(|s| s.opened.push(link.to_string()));
        return Ok(());
    }
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_url(link, None::<&str>).map_err(|e| format!("Couldn't open your browser: {e}"))
}

/// **Continue with ChatGPT**: OpenAI's consent page in the browser; the
/// rest when it hands back (`yap-chatgpt`). `reconsent`: ask the plan's
/// permission again (after a decline).
#[tauri::command]
pub async fn chatgpt_sign_in(app: AppHandle, reconsent: Option<bool>) -> Result<(), String> {
    let d = oauth::discovery().await?;
    let a = account();
    let sign_in = oauth::begin(
        &d,
        &oauth::Ask { client_id: a.client_id.as_deref(), host_id: &a.host_id, login_hint: a.email.as_deref(), reconsent: reconsent.unwrap_or(false) },
    )?;
    let attempt = state(|s| {
        s.attempt += 1;
        s.waiting = true;
        s.error = None;
        s.attempt
    });
    if let Err(e) = open_link(&app, &sign_in.url) {
        state(|s| s.waiting = false);
        emit();
        return Err(e);
    }
    tracing::info!("chatgpt: sign-in started");
    emit();
    tauri::async_runtime::spawn(async move { finish(d, sign_in, attempt, a.client_id).await });
    Ok(())
}

/// Stop waiting for the browser.
#[tauri::command]
pub fn chatgpt_cancel_sign_in() {
    state(|s| {
        s.attempt += 1;
        s.waiting = false;
    });
    emit();
}

async fn finish(d: oauth::Discovery, sign_in: oauth::SignIn, attempt: u64, saved_client: Option<String>) {
    let current = move || state(|s| s.attempt == attempt);
    let waited = tauri::async_runtime::spawn_blocking(move || {
        let deadline = std::time::Instant::now() + oauth::SIGN_IN_TTL;
        loop {
            match sign_in.handback.recv_timeout(Duration::from_millis(500)) {
                Ok(h) => return (Some(h), sign_in),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) if current() && std::time::Instant::now() < deadline => {}
                Err(_) => return (None, sign_in),
            }
        }
    })
    .await;
    let Ok((handback, sign_in)) = waited else { return };
    let outcome = match handback {
        Some(oauth::Handback::Code { code, client_id, reply }) => {
            let attempt = Attempt { verifier: sign_in.verifier.clone(), nonce: sign_in.nonce.clone(), redirect_uri: sign_in.redirect_uri.clone() };
            let result = connect(&d, &attempt, &code, client_id, saved_client).await;
            let _ = reply.send(result.clone());
            result.map(|_| ())
        }
        Some(oauth::Handback::Denied(code)) => {
            tracing::info!(code = %code, "chatgpt: sign-in declined");
            Err(if code == "access_denied" { "You didn't allow Yap to use ChatGPT.".to_string() } else { format!("ChatGPT's sign-in said no ({code}).") })
        }
        None => Ok(()),
    };
    state(|s| {
        if s.attempt == attempt {
            s.waiting = false;
            s.error = outcome.err();
        }
    });
    emit();
}

/// What one attempt sent that its code exchange and checks need.
struct Attempt {
    verifier: String,
    nonce: String,
    redirect_uri: String,
}

/// The code exchanged and checked: the account signed in. Its label (the
/// email) for the browser's page.
async fn connect(d: &oauth::Discovery, sign_in: &Attempt, code: &str, issued: Option<String>, saved: Option<String>) -> Result<String, String> {
    let client = match (issued, saved) {
        (Some(i), Some(s)) if i != s => return Err("ChatGPT answered for a different registration of Yap. Try signing in again.".into()),
        (Some(i), _) => i,
        (None, Some(s)) => s,
        (None, None) => return Err("ChatGPT didn't finish registering Yap. Try signing in again.".into()),
    };
    // Kept before the code is used: a failed exchange never registers Yap
    // a second time.
    with_account(|a| a.client_id = Some(client.clone()));
    let t = oauth::exchange(d, &client, code, &sign_in.verifier, &sign_in.redirect_uri).await.map_err(|f| f.message())?;
    let id_token = t.id_token.as_deref().ok_or("ChatGPT didn't say who signed in. Try again.")?;
    let who = oauth::check_id_token(d, id_token, &client, Some(&sign_in.nonce)).await?;
    // Another account than the one this registration is for: start afresh
    // next time (a new registration for it).
    if account().sub.as_deref().is_some_and(|s| s != who.sub) {
        with_account(|a| {
            a.client_id = None;
            a.sub = None;
        });
        return Err("That's a different ChatGPT account from the one Yap was set up with. Click Continue with ChatGPT again to add it.".into());
    }
    let refresh = t.refresh.clone().ok_or("ChatGPT didn't let Yap stay signed in. Try again.")?;
    let scopes: Vec<String> = t.scope.as_deref().unwrap_or("").split_whitespace().map(str::to_string).collect();
    let expires = now() + t.expires_in;
    save_creds(&Creds { client: client.clone(), sub: who.sub.clone(), refresh, access: t.access.clone(), expires })?;
    state(|s| s.access = Some((t.access, expires)));
    let a = with_account(|a| {
        a.sub = Some(who.sub);
        a.email = who.email;
        a.name = who.name;
        a.scopes = scopes;
        a.signed_in = true;
        a.connected_ts = now();
        a.clone()
    });
    mark_key(a.plan());
    tracing::info!(plan = a.plan(), "chatgpt: signed in");
    Ok(a.email.or(a.name).unwrap_or_else(|| "your account".into()))
}

/// How signing out went.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedOut {
    /// OpenAI confirmed the grant is revoked; else the person is told to
    /// disconnect Yap in ChatGPT's settings.
    pub confirmed: bool,
}

/// **Sign out**: the grant revoked at OpenAI (tried twice), the tokens
/// removed here either way. The registration and this PC's host id stay,
/// so signing in again is one click.
#[tauri::command]
pub async fn chatgpt_sign_out() -> Result<SignedOut, String> {
    let _one = REFRESHING.lock().await;
    let confirmed = match load_creds() {
        Some(c) => oauth::revoke(&c.client, &c.refresh).await,
        None => true,
    };
    forget("");
    tracing::info!(confirmed, "chatgpt: signed out");
    Ok(SignedOut { confirmed })
}

/// The tokens gone, the account signed out (`why`: said in Settings, ""
/// for a sign-out).
fn forget(why: &str) {
    vault::delete();
    state(|s| {
        s.access = None;
        s.error = Some(why.to_string()).filter(|w| !w.is_empty());
    });
    with_account(|a| {
        a.signed_in = false;
        a.scopes.clear();
    });
    mark_key(false);
    emit();
}

/// "You're using your ChatGPT plan" seen (Got it).
#[tauri::command]
pub fn chatgpt_notice_seen() {
    with_account(|a| a.notice_seen = true);
    emit();
}

#[tauri::command]
pub fn chatgpt_status() -> Status {
    status()
}

/// Test mode only: the links Yap would have opened.
#[tauri::command]
pub fn chatgpt_e2e_opened() -> Result<Vec<String>, String> {
    if !crate::shell::e2e::active() {
        return Err("test mode only".into());
    }
    Ok(state(|s| s.opened.clone()))
}

// ---- the access token -------------------------------------------------------------------

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn usable(access: &str, expires: i64) -> Option<String> {
    (expires - MARGIN > now() && !access.is_empty()).then(|| access.to_string())
}

/// **The access token for a call on the plan**, refreshed first when it's
/// nearly out (one refresh at a time, across Yaps too: see the module docs).
/// A grant OpenAI says is over signs Yap out, said in Settings.
pub async fn access_token() -> Result<String, String> {
    if let Some(t) = state(|s| s.access.as_ref().and_then(|(a, e)| usable(a, *e))) {
        return Ok(t);
    }
    let _one = REFRESHING.lock().await;
    let fresh = |c: &Creds| usable(&c.access, c.expires).inspect(|t| state(|s| s.access = Some((t.clone(), c.expires))));
    let creds = load_creds().ok_or(NOT_SIGNED_IN)?;
    if let Some(t) = fresh(&creds) {
        return Ok(t);
    }
    let _across = across_yaps().await?;
    // Another Yap may have refreshed while this one waited: its tokens.
    let creds = load_creds().ok_or(NOT_SIGNED_IN)?;
    if let Some(t) = fresh(&creds) {
        return Ok(t);
    }
    let d = oauth::discovery().await?;
    match oauth::refresh(&d, &creds.client, &creds.refresh).await {
        Ok(t) => {
            let expires = now() + t.expires_in;
            let next = Creds { refresh: t.refresh.clone().unwrap_or_else(|| creds.refresh.clone()), access: t.access.clone(), expires, ..creds.clone() };
            // Kept at once: the old refresh token is spent.
            if let Err(e) = save_creds(&next) {
                tracing::error!("chatgpt: the refreshed sign-in wasn't kept: {e}");
            }
            state(|s| s.access = Some((t.access.clone(), expires)));
            // The account behind it is still the one that signed in.
            if let Some(id) = &t.id_token {
                match oauth::check_id_token(&d, id, &creds.client, None).await {
                    Ok(who) if who.sub != creds.sub => {
                        forget("ChatGPT answered for another account. Sign in again.");
                        return Err(NOT_SIGNED_IN.into());
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!("chatgpt: a refreshed identity couldn't be checked now: {e}"),
                }
            }
            if let Some(scope) = t.scope.as_deref() {
                let scopes: Vec<String> = scope.split_whitespace().map(str::to_string).collect();
                let plan = with_account(|a| {
                    a.scopes = scopes;
                    a.plan()
                });
                if !plan {
                    mark_key(false);
                    emit();
                }
            }
            Ok(t.access)
        }
        Err(oauth::Failure::Ended) => {
            let why = oauth::Failure::Ended.message();
            forget(&why);
            Err(why)
        }
        Err(f) => Err(f.message()),
    }
}

/// **A fresh access token after OpenAI refused the one Yap had** (a 401):
/// the one held here and the one kept for other Yaps set aside, then
/// [`access_token`] refreshes.
pub async fn refreshed_token() -> Result<String, String> {
    {
        let _one = REFRESHING.lock().await;
        state(|s| s.access = None);
        if let Some(mut c) = load_creds() {
            c.expires = 0;
            let _ = save_creds(&c);
        }
    }
    access_token().await
}

/// The cross-process refresh lock (a lock file beside the temp folder's
/// other Yap files, named for the credential): waited for up to 15 s.
async fn across_yaps() -> Result<crate::ai::local_llm::runtime::UpgradeLock, String> {
    let dir = std::env::temp_dir().join("yap");
    let name = format!("{}.refresh.lock", vault::user(0));
    for _ in 0..150 {
        if let Some(lock) = crate::ai::local_llm::runtime::UpgradeLock::take_named(&dir, &name) {
            return Ok(lock);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err("Another Yap is refreshing the ChatGPT sign-in. Try again in a moment.".into())
}

/// The account's models (`slug`, display name), for Chat's picker and
/// Settings.
pub async fn models() -> Result<Vec<(String, String)>, String> {
    let token = access_token().await?;
    oauth::models(&token).await
}

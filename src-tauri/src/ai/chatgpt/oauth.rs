//! Sign in with ChatGPT's OAuth calls, as OpenAI's open-source flow has
//! them (developers.openai.com/siwc/token-sharing-open-source, read
//! 1 Nov 2026): a public client registered on the fly
//! (`client_id=dynamic_agent_client` the first time, the issued
//! `oaiapp_…` id after), PKCE (S256) in the system browser, the code handed
//! back to a loopback listener on `127.0.0.1` at `/auth/callback` (port 1455,
//! else any free one: only the port may change), the code exchanged without
//! a secret; refreshes (the refresh token rotates); revocation; the
//! account's model list. The endpoints come from the issuer's discovery
//! document, each checked to be on the issuer.
//!
//! Debug builds read `YAP_CHATGPT_ISSUER` / `YAP_CHATGPT_API` to point at a
//! local fake (the e2e suite's).

use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use once_cell::sync::Lazy;
use serde_json::Value;
use tiny_http::Server;

use crate::privacy::net::{Purpose, Recorded as _};

const ISSUER: &str = "https://auth.openai.com";
/// The API the tokens are for (`resource`), and where the calls go.
const RESOURCE: &str = "https://api.openai.com/v1";
/// Identity, and using the person's ChatGPT plan.
pub const SCOPES: &str = "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
/// The scope that lets Yap use the plan: without it, signed in but not
/// usable (the person declined, or a workspace's policy).
pub const PLAN_SCOPE: &str = "chatgpt.tokens.use.direct";
/// The name the person sees (and may edit) on the consent screen.
const APP_NAME: &str = "Yap";
const CALLBACK: &str = "/auth/callback";
const PORT: u16 = 1455;
/// How long Yap waits for the browser sign-in.
pub const SIGN_IN_TTL: Duration = Duration::from_secs(10 * 60);

/// The issuer (a fake in a debug build's tests).
pub fn issuer() -> String {
    pick("YAP_CHATGPT_ISSUER", ISSUER)
}

/// The API's base (`https://api.openai.com/v1`, or the fake's).
pub fn api_base() -> String {
    pick("YAP_CHATGPT_API", RESOURCE)
}

fn pick(var: &str, default: &str) -> String {
    #[cfg(debug_assertions)]
    if let Ok(v) = std::env::var(var) {
        let v = v.trim().trim_end_matches('/');
        if !v.is_empty() {
            return v.to_string();
        }
    }
    let _ = var;
    default.to_string()
}

static HTTP: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        // A redirect could carry a token's form elsewhere.
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(format!("Yap/{} (Windows)", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
});

// ---- discovery and keys ---------------------------------------------------------------

/// The issuer's endpoints.
#[derive(Debug, Clone)]
pub struct Discovery {
    pub authorize: String,
    pub token: String,
    pub revoke: Option<String>,
    pub jwks: String,
}

static DISCOVERY: Mutex<Option<Discovery>> = Mutex::new(None);
static KEYS: Mutex<Option<Value>> = Mutex::new(None);

/// The discovery document's endpoints, each on the issuer. Pure.
fn endpoints_of(doc: &Value, issuer: &str) -> Option<Discovery> {
    if doc["issuer"] != issuer {
        return None;
    }
    let on_issuer = |k: &str| -> Option<String> {
        let v = doc[k].as_str()?;
        let (a, b) = (url::Url::parse(v).ok()?, url::Url::parse(issuer).ok()?);
        (a.origin() == b.origin()).then(|| v.to_string())
    };
    Some(Discovery {
        authorize: on_issuer("authorization_endpoint")?,
        token: on_issuer("token_endpoint")?,
        revoke: doc.get("revocation_endpoint").and_then(|_| on_issuer("revocation_endpoint")),
        jwks: on_issuer("jwks_uri")?,
    })
}

pub async fn discovery() -> Result<Discovery, String> {
    if let Some(d) = DISCOVERY.lock().unwrap_or_else(|p| p.into_inner()).clone() {
        return Ok(d);
    }
    let issuer = issuer();
    let doc: Value = HTTP
        .get(format!("{issuer}/.well-known/openid-configuration"))
        .send_for(Purpose::ChatGptSignIn)
        .await
        .map_err(|_| OFFLINE.to_string())?
        .json()
        .await
        .map_err(|_| UNVERIFIED.to_string())?;
    let d = endpoints_of(&doc, &issuer).ok_or(UNVERIFIED)?;
    *DISCOVERY.lock().unwrap_or_else(|p| p.into_inner()) = Some(d.clone());
    Ok(d)
}

const OFFLINE: &str = "Couldn't reach ChatGPT. Check your connection and try again.";
const UNVERIFIED: &str = "ChatGPT's sign-in couldn't be checked. Try again in a moment.";

async fn keys(d: &Discovery, fresh: bool) -> Result<Value, String> {
    if !fresh {
        if let Some(k) = KEYS.lock().unwrap_or_else(|p| p.into_inner()).clone() {
            return Ok(k);
        }
    }
    let set: Value = HTTP
        .get(&d.jwks)
        .send_for(Purpose::ChatGptSignIn)
        .await
        .map_err(|_| OFFLINE.to_string())?
        .json()
        .await
        .map_err(|_| UNVERIFIED.to_string())?;
    if !set["keys"].is_array() {
        return Err(UNVERIFIED.into());
    }
    *KEYS.lock().unwrap_or_else(|p| p.into_inner()) = Some(set.clone());
    Ok(set)
}

/// **Check an ID token** for `client_id` (and this attempt's `nonce`):
/// the key set fetched again once when it lacks the token's key.
pub async fn check_id_token(d: &Discovery, token: &str, client_id: &str, nonce: Option<&str>) -> Result<super::jwt::Identity, String> {
    let now = chrono::Utc::now().timestamp();
    let issuer = issuer();
    let mut set = keys(d, false).await?;
    for fresh in [false, true] {
        if fresh {
            set = keys(d, true).await?;
        }
        match super::jwt::verify(token, &set, &issuer, client_id, nonce, now) {
            Ok(id) => return Ok(id),
            Err(super::jwt::Bad::NoKey) if !fresh => continue,
            Err(e) => {
                tracing::warn!(?e, "chatgpt: an ID token wasn't accepted");
                return Err("ChatGPT's answer couldn't be checked. Sign in again.".into());
            }
        }
    }
    Err(UNVERIFIED.into())
}

// ---- the browser sign-in ----------------------------------------------------------------

/// What the browser handed back to the loopback listener.
pub enum Handback {
    /// A code to exchange (with the issued client id on a new
    /// registration), and where to tell the browser how it went.
    Code { code: String, client_id: Option<String>, reply: mpsc::Sender<Result<String, String>> },
    /// The person said no (or OpenAI refused): its error code.
    Denied(String),
}

/// A sign-in in flight. Dropping it stops the listener.
pub struct SignIn {
    pub url: String,
    pub verifier: String,
    pub nonce: String,
    pub redirect_uri: String,
    pub handback: mpsc::Receiver<Handback>,
    server: Arc<Server>,
}

impl Drop for SignIn {
    fn drop(&mut self) {
        self.server.unblock();
    }
}

/// How a sign-in starts: a new registration, or the account's issued id.
pub struct Ask<'a> {
    pub client_id: Option<&'a str>,
    pub host_id: &'a str,
    pub login_hint: Option<&'a str>,
    /// Ask the plan's permission again (after a decline).
    pub reconsent: bool,
}

/// The authorize address for one attempt. Pure.
pub fn authorize_url(d: &Discovery, ask: &Ask, redirect_uri: &str, state: &str, nonce: &str, challenge: &str) -> Result<String, String> {
    let mut url = url::Url::parse(&d.authorize).map_err(|e| e.to_string())?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("client_id", ask.client_id.unwrap_or("dynamic_agent_client"))
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("scope", SCOPES)
            .append_pair("resource", RESOURCE)
            .append_pair("state", state)
            .append_pair("nonce", nonce)
            .append_pair("code_challenge_method", "S256")
            .append_pair("code_challenge", challenge)
            .append_pair("ext_agent_host_id", ask.host_id);
        // The app's name on the first registration only.
        if ask.client_id.is_none() {
            q.append_pair("agent_name_hint", APP_NAME);
        }
        if let Some(email) = ask.login_hint {
            q.append_pair("login_hint", email);
        }
        if ask.reconsent {
            q.append_pair("prompt", "consent");
        }
    }
    Ok(url.to_string())
}

/// Start a sign-in: the listener (port 1455, else a free one) and the
/// address for the browser.
pub fn begin(d: &Discovery, ask: &Ask) -> Result<SignIn, String> {
    let server = Server::http(("127.0.0.1", PORT))
        .or_else(|_| Server::http("127.0.0.1:0"))
        .map(Arc::new)
        .map_err(|e| format!("Couldn't start the sign-in listener: {e}"))?;
    let port = server.server_addr().to_ip().map(|a| a.port()).ok_or("Couldn't start the sign-in listener.")?;
    let redirect_uri = format!("http://127.0.0.1:{port}{CALLBACK}");
    tracing::info!(port, "chatgpt: waiting for the browser on the loopback");
    let state = crate::auth::random_alnum(32)?;
    let nonce = crate::auth::random_alnum(32)?;
    let (verifier, challenge) = crate::auth::pkce_pair()?;
    let url = authorize_url(d, ask, &redirect_uri, &state, &nonce, &challenge)?;

    let (tx, rx) = mpsc::channel();
    let srv = server.clone();
    let host = format!("127.0.0.1:{port}");
    std::thread::Builder::new()
        .name("yap-chatgpt-loopback".into())
        .spawn(move || {
            for request in srv.incoming_requests() {
                if handle(request, &host, &state, &tx) {
                    break;
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(SignIn { url, verifier, nonce, redirect_uri, handback: rx, server })
}

/// The callback's parameters, if they're this attempt's: one `state`,
/// matching. Pure.
fn ours(query: &str, state: &str) -> Option<HashMap<String, String>> {
    let pairs: Vec<(String, String)> = url::form_urlencoded::parse(query.as_bytes()).into_owned().collect();
    let states: Vec<&String> = pairs.iter().filter(|(k, _)| k == "state").map(|(_, v)| v).collect();
    if states.len() != 1 || !crate::auth::same_secret(states[0], state) {
        return None;
    }
    // A repeated code or client id isn't a callback Yap trusts.
    for k in ["code", "client_id"] {
        if pairs.iter().filter(|(n, _)| n == k).count() > 1 {
            return None;
        }
    }
    Some(pairs.into_iter().collect())
}

/// One request to the listener; `true` once the sign-in is over.
fn handle(request: tiny_http::Request, host: &str, state: &str, tx: &mpsc::Sender<Handback>) -> bool {
    let from_loopback = request.remote_addr().is_some_and(|a| a.ip().is_loopback());
    let right_host = request.headers().iter().any(|h| h.field.equiv("Host") && h.value.as_str() == host);
    let (path, query) = request.url().split_once('?').map(|(p, q)| (p.to_string(), q.to_string())).unwrap_or((request.url().to_string(), String::new()));
    let get = *request.method() == tiny_http::Method::Get;
    let Some(params) = (from_loopback && right_host && get && path == CALLBACK).then(|| ours(&query, state)).flatten() else {
        reply(request, 404, None);
        return false;
    };
    if let Some(error) = params.get("error") {
        reply(request, 200, Some(page(false, "ChatGPT wasn't connected. You can close this tab and go back to Yap.")));
        let _ = tx.send(Handback::Denied(error.clone()));
        return true;
    }
    let Some(code) = params.get("code").filter(|c| !c.is_empty()).cloned() else {
        reply(request, 400, None);
        return false;
    };
    let client_id = params.get("client_id").filter(|c| valid_client_id(c)).cloned();
    let (done_tx, done_rx) = mpsc::channel();
    if tx.send(Handback::Code { code, client_id, reply: done_tx }).is_err() {
        reply(request, 200, Some(page(false, "Yap stopped waiting for this sign-in. Start again from Yap.")));
        return true;
    }
    let html = match done_rx.recv_timeout(Duration::from_secs(45)) {
        Ok(Ok(account)) => page(true, &format!("Yap can now use the ChatGPT plan of {account}. You can close this tab.")),
        Ok(Err(e)) => page(false, &e),
        Err(_) => page(true, "Yap is finishing the connection. You can close this tab."),
    };
    reply(request, 200, Some(html));
    true
}

/// **The listener's answer, and the connection closed** (`Connection:
/// close`): a browser keeps a connection to `127.0.0.1:1455` open, and the
/// next attempt's callback would go down it to this attempt's listener,
/// gone by then — the sign-in after a declined one never arrived (1 Nov
/// 2026). tiny_http ignores a `Connection` header a response sets, so it's
/// written whole here.
fn reply(request: tiny_http::Request, status: u16, html: Option<String>) {
    use std::io::Write as _;
    let body = html.unwrap_or_default();
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        _ => "Not Found",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let mut w = request.into_writer();
    let _ = w.write_all(head.as_bytes()).and_then(|_| w.write_all(body.as_bytes())).and_then(|_| w.flush());
}

/// An issued client id as OpenAI's look (never the registration entry
/// point itself). Pure.
pub fn valid_client_id(id: &str) -> bool {
    (1..=200).contains(&id.len()) && id != "dynamic_agent_client" && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The little page the browser shows after the consent screen.
fn page(ok: bool, message: &str) -> String {
    let title = if ok { "ChatGPT connected" } else { "Not connected" };
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Yap · {title}</title>\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"></head>\
         <body style=\"margin:0;min-height:100vh;display:flex;align-items:center;justify-content:center;\
         background:#f6f3ee;color:#26231c;font:16px/1.5 'Segoe UI',system-ui,sans-serif\">\
         <main style=\"max-width:420px;padding:32px;border-radius:18px;background:#fff;\
         box-shadow:0 8px 30px rgba(38,35,28,.08);text-align:center\">\
         <h1 style=\"margin:0 0 8px;font:600 26px Georgia,serif\">{title}</h1><p style=\"margin:0;color:#6b6457\">{}</p>\
         </main></body></html>",
        esc(message),
    )
}

// ---- tokens -----------------------------------------------------------------------------

/// What the token endpoint returned.
#[derive(Debug)]
pub struct Tokens {
    pub access: String,
    pub expires_in: i64,
    pub refresh: Option<String>,
    pub id_token: Option<String>,
    /// The scopes granted (`None`: unchanged, on a refresh).
    pub scope: Option<String>,
}

/// Why a token call failed.
#[derive(Debug, PartialEq)]
pub enum Failure {
    Offline,
    /// The grant can't be used again (expired, revoked, reused): sign in
    /// again.
    Ended,
    /// Anything else: the status and OpenAI's error code.
    Http(u16, String),
}

impl Failure {
    pub fn message(&self) -> String {
        match self {
            Failure::Offline => OFFLINE.into(),
            Failure::Ended => "Your ChatGPT sign-in has ended. Sign in again in Settings → Language Models.".into(),
            Failure::Http(s, code) if code.is_empty() => format!("ChatGPT's sign-in had a problem (error {s}). Try again."),
            Failure::Http(s, code) => format!("ChatGPT's sign-in had a problem ({code}, error {s}). Try again."),
        }
    }
}

/// The error codes that end a grant (OpenAI's "Refresh errors"). Pure.
fn ends_grant(code: &str) -> bool {
    matches!(
        code,
        "invalid_grant" | "invalid_refresh_token" | "token_expired" | "refresh_token_expired" | "refresh_token_invalidated" | "refresh_token_reused"
    )
}

/// A token response, checked (OpenAI's DevKit's checks). Pure.
pub fn tokens_of(body: &Value, want_refresh: bool) -> Option<Tokens> {
    let text = |k: &str| body[k].as_str().filter(|s| !s.is_empty()).map(str::to_string);
    let access = text("access_token")?;
    if !body["token_type"].as_str().is_some_and(|t| t.eq_ignore_ascii_case("bearer")) {
        return None;
    }
    let expires_in = body["expires_in"].as_i64().filter(|n| *n > 0)?;
    let scope = text("scope");
    let refresh = text("refresh_token");
    let offline = scope.as_deref().is_some_and(|s| s.split_whitespace().any(|x| x == "offline_access"));
    if (want_refresh || offline) && refresh.is_none() {
        return None;
    }
    Some(Tokens { access, expires_in, refresh, id_token: text("id_token"), scope })
}

async fn token_call(d: &Discovery, form: &[(&str, &str)], want_refresh: bool) -> Result<Tokens, Failure> {
    let resp = HTTP.post(&d.token).form(form).send_for(Purpose::ChatGptSignIn).await.map_err(|_| Failure::Offline)?;
    let status = resp.status().as_u16();
    // Bodies hold tokens: never logged.
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    if status != 200 {
        let code = body["error"].as_str().or(body["error"]["code"].as_str()).unwrap_or("").to_string();
        tracing::warn!(status, code = %code, "chatgpt: the token endpoint said no");
        return Err(if ends_grant(&code) { Failure::Ended } else { Failure::Http(status, code) });
    }
    tokens_of(&body, want_refresh).ok_or_else(|| Failure::Http(status, "incomplete".into()))
}

/// Swap the code for tokens (the PKCE verifier, the same redirect).
pub async fn exchange(d: &Discovery, client_id: &str, code: &str, verifier: &str, redirect_uri: &str) -> Result<Tokens, Failure> {
    token_call(
        d,
        &[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", redirect_uri),
            ("resource", RESOURCE),
        ],
        false,
    )
    .await
}

/// A fresh access token (and the next refresh token).
pub async fn refresh(d: &Discovery, client_id: &str, refresh_token: &str) -> Result<Tokens, Failure> {
    token_call(d, &[("grant_type", "refresh_token"), ("client_id", client_id), ("refresh_token", refresh_token), ("resource", RESOURCE)], true).await
}

/// Let go of the grant (signing out): `true` once OpenAI confirmed it
/// (an empty 200, an already-dead token too); tried twice on a 5xx or the
/// network.
pub async fn revoke(client_id: &str, refresh_token: &str) -> bool {
    let Ok(d) = discovery().await else { return false };
    let Some(endpoint) = d.revoke else { return false };
    for attempt in 0..2 {
        let sent = HTTP
            .post(&endpoint)
            .form(&[("token", refresh_token), ("token_type_hint", "refresh_token"), ("client_id", client_id)])
            .send_for(Purpose::ChatGptSignIn)
            .await;
        match sent {
            Ok(r) if r.status().as_u16() == 200 => return true,
            Ok(r) if r.status().as_u16() < 500 => return false,
            _ if attempt == 0 => tokio::time::sleep(Duration::from_millis(300)).await,
            _ => {}
        }
    }
    false
}

// ---- the account's models -------------------------------------------------------------

/// The account's models to show (`visibility: "list"`, the server's
/// order): `(slug, display name)`. Pure.
pub fn models_of(body: &Value) -> Option<Vec<(String, String)>> {
    let list = body["models"].as_array()?;
    Some(
        list.iter()
            .filter(|m| m["visibility"] == "list")
            .filter_map(|m| {
                let slug = m["slug"].as_str().map(str::trim).filter(|s| !s.is_empty() && s.len() <= 200)?;
                let name = m["display_name"].as_str().map(str::trim).filter(|s| !s.is_empty() && s.len() <= 200).unwrap_or(slug);
                Some((slug.to_string(), name.to_string()))
            })
            .collect(),
    )
}

pub async fn models(access: &str) -> Result<Vec<(String, String)>, String> {
    let resp = HTTP
        .get(format!("{}/models", api_base()))
        .bearer_auth(access)
        .send_for(Purpose::ModelList("ChatGPT".into()))
        .await
        .map_err(|_| OFFLINE.to_string())?;
    let status = resp.status().as_u16();
    let body: Value = resp.json().await.unwrap_or(Value::Null);
    if status != 200 {
        return Err(format!("ChatGPT didn't list its models (error {status})."));
    }
    models_of(&body).ok_or_else(|| "ChatGPT's model list didn't look right.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn d() -> Discovery {
        Discovery {
            authorize: "https://auth.openai.com/api/accounts/authorize".into(),
            token: "https://auth.openai.com/api/accounts/oauth/token".into(),
            revoke: Some("https://auth.openai.com/api/accounts/oauth/revoke".into()),
            jwks: "https://auth.openai.com/.well-known/jwks.json".into(),
        }
    }

    #[test]
    fn the_first_sign_in_registers_yap_and_a_later_one_reuses_its_id() {
        let ask = Ask { client_id: None, host_id: "urn:uuid:00000000-0000-4000-8000-000000000000", login_hint: None, reconsent: false };
        let url = url::Url::parse(&authorize_url(&d(), &ask, "http://127.0.0.1:1455/auth/callback", "s", "n", "c").unwrap()).unwrap();
        let q: HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(q["client_id"], "dynamic_agent_client");
        assert_eq!(q["agent_name_hint"], "Yap");
        assert_eq!(q["scope"], SCOPES);
        assert_eq!(q["resource"], "https://api.openai.com/v1");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:1455/auth/callback");
        assert_eq!((q["code_challenge_method"].as_str(), q["code_challenge"].as_str()), ("S256", "c"));
        assert_eq!(q["ext_agent_host_id"], "urn:uuid:00000000-0000-4000-8000-000000000000");
        assert!(!q.contains_key("prompt") && !q.contains_key("login_hint"));

        let again = Ask { client_id: Some("oaiapp_abc"), login_hint: Some("sam@example.com"), reconsent: true, ..ask };
        let url = url::Url::parse(&authorize_url(&d(), &again, "http://127.0.0.1:50000/auth/callback", "s", "n", "c").unwrap()).unwrap();
        let q: HashMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(q["client_id"], "oaiapp_abc");
        assert!(!q.contains_key("agent_name_hint"));
        assert_eq!((q["login_hint"].as_str(), q["prompt"].as_str()), ("sam@example.com", "consent"));
    }

    #[test]
    fn only_this_attempts_callback_is_taken() {
        assert!(ours("code=abc&state=s1&client_id=oaiapp_x", "s1").is_some());
        assert!(ours("code=abc&state=s2", "s1").is_none());
        assert!(ours("code=abc", "s1").is_none());
        assert!(ours("code=abc&state=s1&state=s1", "s1").is_none());
        assert!(ours("code=a&code=b&state=s1", "s1").is_none());
        assert!(valid_client_id("oaiapp_Ab-1") && !valid_client_id("dynamic_agent_client") && !valid_client_id("a b") && !valid_client_id(""));
    }

    #[test]
    fn the_discovery_documents_endpoints_must_be_the_issuers() {
        let doc = json!({
            "issuer": "https://auth.openai.com",
            "authorization_endpoint": "https://auth.openai.com/api/accounts/authorize",
            "token_endpoint": "https://auth.openai.com/api/accounts/oauth/token",
            "jwks_uri": "https://auth.openai.com/.well-known/jwks.json",
        });
        let e = endpoints_of(&doc, "https://auth.openai.com").unwrap();
        assert!(e.revoke.is_none());
        let mut elsewhere = doc.clone();
        elsewhere["token_endpoint"] = json!("https://evil.example.com/token");
        assert!(endpoints_of(&elsewhere, "https://auth.openai.com").is_none());
        assert!(endpoints_of(&doc, "https://auth.example.com").is_none());
    }

    #[test]
    fn a_token_response_is_checked_and_a_dead_grant_said() {
        let ok = json!({ "access_token": "a", "token_type": "Bearer", "expires_in": 3600, "refresh_token": "r", "id_token": "i", "scope": "openid offline_access chatgpt.tokens.use.direct" });
        let t = tokens_of(&ok, false).unwrap();
        assert_eq!((t.access.as_str(), t.refresh.as_deref(), t.expires_in), ("a", Some("r"), 3600));
        let mut no_refresh = ok.clone();
        no_refresh.as_object_mut().unwrap().remove("refresh_token");
        assert!(tokens_of(&no_refresh, false).is_none(), "offline_access without a refresh token");
        let mut mac = ok.clone();
        mac["token_type"] = json!("mac");
        assert!(tokens_of(&mac, false).is_none());
        // A refresh may leave the scope out (unchanged).
        let refreshed = json!({ "access_token": "a2", "token_type": "bearer", "expires_in": 3600, "refresh_token": "r2" });
        assert!(tokens_of(&refreshed, true).unwrap().scope.is_none());
        assert!(ends_grant("refresh_token_reused") && ends_grant("invalid_grant") && !ends_grant("invalid_client"));
    }

    /// The browser's GET to the loopback listener, as raw HTTP: its status
    /// line and body.
    fn browser_get(url: &str) -> String {
        use std::io::{Read, Write};
        let u = url::Url::parse(url).unwrap();
        let host = format!("{}:{}", u.host_str().unwrap(), u.port().unwrap());
        let mut s = std::net::TcpStream::connect(&host).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let path = format!("{}?{}", u.path(), u.query().unwrap_or(""));
        // Kept alive, as a browser asks: the listener closes it.
        write!(s, "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: keep-alive\r\n\r\n").unwrap();
        // Read to the end of the page, as a browser told "close" does.
        let mut out = Vec::new();
        let mut buf = [0u8; 4096];
        while let Ok(n) = s.read(&mut buf) {
            if n == 0 {
                break;
            }
            out.extend_from_slice(&buf[..n]);
            if String::from_utf8_lossy(&out).contains("</html>") {
                break;
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    #[test]
    fn a_sign_in_after_a_declined_one_still_gets_its_callback() {
        let ask = Ask { client_id: None, host_id: "urn:uuid:00000000-0000-4000-8000-000000000000", login_hint: None, reconsent: false };
        for round in 0..3 {
            let s = begin(&d(), &ask).unwrap();
            let state: String = url::Url::parse(&s.url).unwrap().query_pairs().find(|(k, _)| k == "state").unwrap().1.into_owned();
            let back = if round == 0 { format!("{}?error=access_denied&state={state}", s.redirect_uri) } else { format!("{}?code=c{round}&state={state}&client_id=oaiapp_x", s.redirect_uri) };
            let browser = std::thread::spawn(move || browser_get(&back));
            match s.handback.recv_timeout(Duration::from_secs(5)).expect("the callback reached Yap") {
                Handback::Code { code, client_id, reply } => {
                    assert_eq!((code.as_str(), client_id.as_deref()), (format!("c{round}").as_str(), Some("oaiapp_x")));
                    reply.send(Ok("sam@example.com".into())).unwrap();
                }
                Handback::Denied(e) => assert_eq!(e, "access_denied"),
            }
            let page = browser.join().unwrap();
            assert!(page.starts_with("HTTP/1.1 200") && page.contains("Connection: close"), "{page}");
            assert!(page.contains(if round == 0 { "Not connected" } else { "ChatGPT connected" }), "{page}");
            drop(s);
        }
    }

    #[test]
    fn the_model_list_keeps_the_listed_ones_in_order() {
        let body = json!({ "models": [
            { "slug": "gpt-6.1-sol", "display_name": "GPT-6.1 Sol", "visibility": "list" },
            { "slug": "internal", "display_name": "Hidden", "visibility": "hide" },
            { "slug": "gpt-6-mini", "display_name": "", "visibility": "list" },
        ]});
        assert_eq!(models_of(&body).unwrap(), vec![("gpt-6.1-sol".into(), "GPT-6.1 Sol".into()), ("gpt-6-mini".into(), "gpt-6-mini".into())]);
        assert!(models_of(&json!({ "data": [] })).is_none());
    }
}

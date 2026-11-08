//! **The token endpoint, registration and revocation**: the code swapped
//! with its PKCE verifier and the resource (RFC 8707), refreshes (the
//! resource again; a refresh token that rotates is replaced at once),
//! dynamic client registration (RFC 7591) and revocation (RFC 7009). The
//! client authenticates as it was registered ([`Method`]): a public client
//! sends its id; one with a secret sends it in the form or as HTTP Basic,
//! as the server lists. Bodies hold tokens: never logged, only their
//! status and the server's error code.

use base64::Engine as _;
use serde_json::Value;
use url::Url;

use super::super::store::Endpoints;
use super::discover::Net;

/// What a token endpoint returned.
pub struct Tokens {
    pub access: String,
    pub refresh: Option<String>,
    /// Seconds the access token lasts, when the server says.
    pub expires_in: Option<i64>,
    /// The scopes granted (`None`: those asked for, RFC 6749 §5.1).
    pub scope: Option<String>,
}

/// Why a token call failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// No answer, or no connection: Yap's words.
    Offline(String),
    /// The grant can't be used again (`invalid_grant`: expired, revoked,
    /// reused): sign in again.
    Ended,
    /// The server doesn't know the client (`invalid_client`,
    /// `unauthorized_client`): a registration to make again.
    Client,
    /// Anything else: the status and the server's error code.
    Http(u16, String),
}

impl Failure {
    pub fn words(&self, host: &str) -> String {
        match self {
            Failure::Offline(why) => why.clone(),
            Failure::Ended => format!("Your sign-in to {host} has ended. Sign in again."),
            Failure::Client => format!("{host} doesn't know Yap's app registration any more. Sign in again."),
            Failure::Http(s, code) if code.is_empty() => format!("{host}'s sign-in had a problem (error {s}). Try again."),
            Failure::Http(s, code) => format!("{host}'s sign-in had a problem ({code}, error {s}). Try again."),
        }
    }
}

/// A token response, checked: an access token of type Bearer (DPoP and
/// others aren't something Yap sends). Pure.
pub fn tokens_of(body: &Value) -> Option<Tokens> {
    let text = |k: &str| body[k].as_str().filter(|s| !s.is_empty()).map(str::to_string);
    let access = text("access_token")?;
    if access.contains(['\r', '\n']) || !body["token_type"].as_str().is_some_and(|t| t.eq_ignore_ascii_case("bearer")) {
        return None;
    }
    let expires_in = body["expires_in"].as_i64().or_else(|| body["expires_in"].as_str().and_then(|s| s.parse().ok())).filter(|n| *n > 0);
    Some(Tokens { access, refresh: text("refresh_token"), expires_in, scope: text("scope") })
}

/// An error code a server sent, if it looks like one (`[a-z_]`, short):
/// never its description, which is the server's text. Pure.
pub fn error_code(body: &Value) -> String {
    body["error"].as_str().filter(|c| c.len() <= 40 && c.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')).unwrap_or("").to_string()
}

/// How the client authenticates at the token endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// A public client: its id in the form.
    None,
    /// `client_secret_post`.
    Post,
    /// `client_secret_basic` (HTTP Basic, the default RFC 8414 assumes).
    Basic,
}

/// The method for a client with or without a secret: the one it was
/// registered with, else the first the server lists that fits. Pure.
pub fn method_for(has_secret: bool, supported: &[String], registered: Option<&str>) -> Method {
    if !has_secret {
        return Method::None;
    }
    match registered {
        Some("client_secret_post") => return Method::Post,
        Some("client_secret_basic") => return Method::Basic,
        _ => {}
    }
    for m in supported {
        match m.as_str() {
            "client_secret_post" => return Method::Post,
            "client_secret_basic" => return Method::Basic,
            _ => {}
        }
    }
    Method::Basic
}

/// A client as the token endpoint sees it.
pub struct Client<'a> {
    pub id: &'a str,
    pub secret: Option<&'a str>,
    pub method: Method,
}

/// `form` with the client's authentication, POSTed to `url`.
async fn post_form(net: &Net, url: &str, client: &Client<'_>, form: &[(&str, &str)]) -> Result<super::discover::Answer, Failure> {
    let u = Url::parse(url).map_err(|_| Failure::Http(0, String::new()))?;
    let mut fields: Vec<(&str, &str)> = form.to_vec();
    let mut req = net.post(&u);
    match (client.method, client.secret) {
        (Method::Basic, Some(secret)) => {
            // RFC 6749 §2.3.1: each form-encoded, then joined.
            let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
            let pair = base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", enc(client.id), enc(secret)));
            let mut v = reqwest::header::HeaderValue::from_str(&format!("Basic {pair}")).map_err(|_| Failure::Http(0, String::new()))?;
            v.set_sensitive(true);
            req = req.header(reqwest::header::AUTHORIZATION, v);
        }
        (Method::Post, Some(secret)) => {
            fields.push(("client_id", client.id));
            fields.push(("client_secret", secret));
        }
        _ => fields.push(("client_id", client.id)),
    }
    net.send(&u, req.form(&fields)).await.map_err(Failure::Offline)
}

async fn token_call(net: &Net, endpoints: &Endpoints, client: &Client<'_>, form: &[(&str, &str)]) -> Result<Tokens, Failure> {
    let a = post_form(net, &endpoints.token_endpoint, client, form).await?;
    if a.status != 200 {
        let refusal = error_code(&a.body);
        tracing::warn!(status = a.status, refusal = %refusal, "connectors: the token endpoint said no");
        return Err(match refusal.as_str() {
            "invalid_grant" => Failure::Ended,
            "invalid_client" | "unauthorized_client" => Failure::Client,
            _ => Failure::Http(a.status, refusal),
        });
    }
    tokens_of(&a.body).ok_or_else(|| Failure::Http(200, "incomplete".into()))
}

/// Swap the code for tokens: the PKCE verifier, the same redirect, the
/// resource.
pub async fn exchange(net: &Net, endpoints: &Endpoints, client: &Client<'_>, code: &str, verifier: &str, redirect_uri: &str, resource: &str) -> Result<Tokens, Failure> {
    let form = [("grant_type", "authorization_code"), ("code", code), ("code_verifier", verifier), ("redirect_uri", redirect_uri), ("resource", resource)];
    token_call(net, endpoints, client, &form).await
}

/// A fresh access token (and, where the server rotates them, the next
/// refresh token).
pub async fn refresh(net: &Net, endpoints: &Endpoints, client: &Client<'_>, refresh_token: &str, resource: &str) -> Result<Tokens, Failure> {
    let form = [("grant_type", "refresh_token"), ("refresh_token", refresh_token), ("resource", resource)];
    token_call(net, endpoints, client, &form).await
}

/// Let go of `token` (RFC 7009; `hint`: `refresh_token` | `access_token`):
/// `true` once the server confirmed it (a 200, a token it no longer knows
/// too), tried twice on a 5xx or no answer.
pub async fn revoke(net: &Net, endpoint: &str, client: &Client<'_>, token: &str, hint: &str) -> bool {
    for attempt in 0..2 {
        match post_form(net, endpoint, client, &[("token", token), ("token_type_hint", hint)]).await {
            Ok(a) if a.status == 200 => return true,
            Ok(a) if a.status < 500 => {
                let refusal = error_code(&a.body);
                tracing::warn!(status = a.status, refusal = %refusal, "connectors: a revocation was refused");
                return false;
            }
            _ if attempt == 0 => tokio::time::sleep(std::time::Duration::from_millis(300)).await,
            _ => {}
        }
    }
    false
}

/// Register Yap at `endpoint` (RFC 7591): the server's answer.
pub async fn register(net: &Net, endpoint: &str, body: &Value) -> Result<Value, String> {
    let u = Url::parse(endpoint).map_err(|_| "The sign-in server's registration address isn't one Yap can use.".to_string())?;
    let a = net.send(&u, net.post(&u).json(body)).await?;
    if a.status != 200 && a.status != 201 {
        let refusal = error_code(&a.body);
        tracing::warn!(status = a.status, refusal = %refusal, "connectors: a client registration was refused");
        let host = crate::privacy::net::host_of(&u);
        return Err(if refusal.is_empty() { format!("{host} wouldn't register Yap (error {}).", a.status) } else { format!("{host} wouldn't register Yap ({refusal}).") });
    }
    Ok(a.body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_token_response_is_a_bearer_token_and_a_server_text_is_never_kept() {
        let t = tokens_of(&json!({ "access_token": "at-1", "token_type": "Bearer", "expires_in": 3600, "refresh_token": "rt-1", "scope": "read" })).unwrap();
        assert_eq!((t.access.as_str(), t.refresh.as_deref(), t.expires_in, t.scope.as_deref()), ("at-1", Some("rt-1"), Some(3600), Some("read")));
        let t = tokens_of(&json!({ "access_token": "at-1", "token_type": "bearer", "expires_in": "60" })).unwrap();
        assert_eq!((t.refresh, t.expires_in, t.scope), (None, Some(60), None), "no refresh token, the scope as asked");
        assert!(tokens_of(&json!({ "access_token": "at-1", "token_type": "DPoP" })).is_none());
        assert!(tokens_of(&json!({ "token_type": "Bearer" })).is_none());
        assert!(tokens_of(&json!({ "access_token": "a\nb", "token_type": "Bearer" })).is_none());
        assert_eq!(error_code(&json!({ "error": "invalid_grant", "error_description": "Go to evil.example.com to fix this" })), "invalid_grant");
        assert_eq!(error_code(&json!({ "error": "<script>alert(1)</script>" })), "");
        assert!(Failure::Ended.words("mcp.example.com").contains("Sign in again"));
    }

    #[test]
    fn a_client_authenticates_as_it_was_registered() {
        let listed = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(method_for(false, &listed(&["client_secret_basic"]), None), Method::None);
        assert_eq!(method_for(true, &[], None), Method::Basic, "RFC 8414's default");
        assert_eq!(method_for(true, &listed(&["none", "client_secret_post"]), None), Method::Post);
        assert_eq!(method_for(true, &listed(&["client_secret_post"]), Some("client_secret_basic")), Method::Basic);
    }
}

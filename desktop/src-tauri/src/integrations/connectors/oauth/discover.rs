//! **How a server signs in**, found the MCP authorization spec's way
//! (revision 2026-07-28), every request through `net.rs` as the
//! connector's sign-in ([`Net`]):
//!
//! 1. **The challenge**: the server's `401` and its `WWW-Authenticate:
//!    Bearer resource_metadata="…", scope="…"` ([`challenge`]).
//! 2. **Its protected resource metadata** (RFC 9728): the challenge's
//!    address, else `/.well-known/oauth-protected-resource/<path>`, then
//!    `/.well-known/oauth-protected-resource` ([`resource_urls`]). Its
//!    `resource` must be this server ([`resource_matches`]: the same origin,
//!    a path that is the server's or a parent of it), or Yap stops: a
//!    sign-in for another server is never used.
//! 3. **The authorization server's metadata** (RFC 8414, then OpenID
//!    Connect), for an issuer the resource names ([`metadata_urls`]: with
//!    a path, RFC 8414 with the path inserted, OpenID with it inserted, then
//!    OpenID with it appended). Its `issuer` must be the one Yap asked for
//!    ([`metadata_of`]), it must offer PKCE with S256 (without
//!    `code_challenge_methods_supported`, Yap refuses, as the spec says), and
//!    every endpoint must be one Yap may send a sign-in to ([`endpoint`]).
//!
//! **Where requests may go**: as the server's own link (`remote.rs`): for
//! a server on the internet, `https://` only, never an address on this PC
//! or the network (a name resolving to one refused as the connection is
//! made); for a server on this PC or the network, `http://` there too. No
//! redirects are followed. Bodies are read to 256 KB.

use std::time::Duration;

use serde_json::Value;
use url::Url;

use super::super::remote::{self, HttpError};
use super::super::store::Endpoints;
use crate::privacy::net::{self, Place, Purpose, Recorded as _};

/// Each sign-in request's limit.
const TIMEOUT: Duration = Duration::from_secs(20);
/// The most of an answer read.
const MAX_BODY: usize = 256 * 1024;

// ---- the challenge -------------------------------------------------------------------------------

/// A `WWW-Authenticate` challenge's Bearer parameters.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Challenge {
    pub resource_metadata: Option<String>,
    pub scope: Option<String>,
    pub error: Option<String>,
}

/// The Bearer challenge in a `WWW-Authenticate` header (one of several
/// schemes, or the only one; quoted or bare values). Pure.
pub fn challenge(header: &str) -> Challenge {
    let mut out = Challenge::default();
    let mut scheme = String::new();
    let b = header.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i] == b' ' || b[i] == b',' || b[i] == b'\t') {
            i += 1;
        }
        let start = i;
        while i < b.len() && !matches!(b[i], b' ' | b'=' | b',' | b'\t') {
            i += 1;
        }
        let token = &header[start..i];
        let mut j = i;
        while j < b.len() && b[j] == b' ' {
            j += 1;
        }
        if j < b.len() && b[j] == b'=' && !token.is_empty() {
            // key=value (quoted or a token)
            j += 1;
            while j < b.len() && b[j] == b' ' {
                j += 1;
            }
            let mut value = String::new();
            if j < b.len() && b[j] == b'"' {
                j += 1;
                while j < b.len() && b[j] != b'"' {
                    if b[j] == b'\\' && j + 1 < b.len() {
                        j += 1;
                    }
                    value.push(b[j] as char);
                    j += 1;
                }
                j += 1;
            } else {
                let vs = j;
                while j < b.len() && b[j] != b',' {
                    j += 1;
                }
                value = header[vs..j].trim().to_string();
            }
            i = j;
            if scheme.eq_ignore_ascii_case("bearer") {
                match token.to_ascii_lowercase().as_str() {
                    "resource_metadata" => out.resource_metadata = Some(value).filter(|v| !v.is_empty()),
                    "scope" => out.scope = Some(value).filter(|v| !v.trim().is_empty()),
                    "error" => out.error = Some(value).filter(|v| !v.is_empty()),
                    _ => {}
                }
            }
        } else if !token.is_empty() {
            scheme = token.to_string();
        } else {
            i += 1;
        }
    }
    out
}

/// Scopes in a space-separated list, each once. Pure.
pub fn split_scopes(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for x in s.split_whitespace() {
        if !out.iter().any(|o| o == x) {
            out.push(x.to_string());
        }
    }
    out
}

// ---- where a sign-in may go ----------------------------------------------------------------------

/// An endpoint a sign-in for a server at `server` may use: `https://` (for a
/// server on the internet, a name on the internet too), or `http://` on this
/// PC or the network when the server is there; no credentials or fragment
/// in it; never a cloud metadata address. Pure.
pub fn endpoint(raw: &str, server: Place) -> Option<Url> {
    let u = Url::parse(raw.trim()).ok()?;
    if !u.username().is_empty() || u.password().is_some() || u.fragment().is_some() {
        return None;
    }
    let host = u.host_str()?.to_ascii_lowercase();
    if remote::METADATA.contains(&host.as_str()) {
        return None;
    }
    let place = net::place_of(&host);
    match u.scheme() {
        "https" => (server != Place::Internet || place == Place::Internet).then_some(u),
        "http" => (server != Place::Internet && place != Place::Internet).then_some(u),
        _ => None,
    }
}

/// `path`'s segments, without empty ones (a trailing slash). Pure.
fn segments(u: &Url) -> Vec<&str> {
    u.path().split('/').filter(|s| !s.is_empty()).collect()
}

/// Whether protected resource metadata's `resource` is this server: the
/// same scheme, host and port, and a path that is the server's or one of
/// its parents (`https://mcp.example.com` for `…/mcp`). Pure.
pub fn resource_matches(resource: &Url, server: &Url) -> bool {
    if resource.fragment().is_some() || resource.origin() != server.origin() {
        return false;
    }
    let (r, s) = (segments(resource), segments(server));
    r.len() <= s.len() && r.iter().zip(&s).all(|(a, b)| a == b)
}

// ---- protected resource metadata ------------------------------------------------------------------

/// Where to look for `server`'s protected resource metadata, in order: the
/// challenge's address (when it's one a sign-in may use), the path-aware
/// well-known address, the root one. Pure.
pub fn resource_urls(server: &Url, header: Option<&str>, place: Place) -> Vec<Url> {
    let mut out = Vec::new();
    if let Some(u) = header.and_then(|h| endpoint(h, place)) {
        out.push(u);
    }
    let mut root = server.clone();
    root.set_query(None);
    root.set_fragment(None);
    let path = server.path().trim_end_matches('/');
    if !path.is_empty() {
        let mut with_path = root.clone();
        with_path.set_path(&format!("/.well-known/oauth-protected-resource{path}"));
        if !out.contains(&with_path) {
            out.push(with_path);
        }
    }
    root.set_path("/.well-known/oauth-protected-resource");
    if !out.contains(&root) {
        out.push(root);
    }
    out
}

/// What a server's protected resource metadata says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    /// The resource the tokens are for (RFC 8707's `resource`).
    pub resource: String,
    /// The authorization servers it names (each one a sign-in may use).
    pub authorization_servers: Vec<String>,
    pub scopes: Vec<String>,
}

/// A protected resource metadata document, checked for `server`. Pure.
pub fn resource_of(doc: &Value, server: &Url, place: Place) -> Result<Resource, String> {
    let host = net::host_of(server);
    let named = doc["resource"].as_str().ok_or_else(|| format!("{host} doesn't say how to sign in to it."))?;
    let resource = Url::parse(named).map_err(|_| format!("{host} doesn't say how to sign in to it."))?;
    if !resource_matches(&resource, server) {
        let other = resource.host_str().map(str::to_string).unwrap_or_else(|| "another address".into());
        return Err(format!("{host}'s sign-in is for another server ({other}), so Yap stopped there."));
    }
    let servers: Vec<String> = doc["authorization_servers"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).filter(|s| endpoint(s, place).is_some()).map(str::to_string).collect())
        .unwrap_or_default();
    if servers.is_empty() {
        return Err(format!("{host} doesn't name a sign-in server Yap can use."));
    }
    let scopes = doc["scopes_supported"].as_array().map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
    Ok(Resource { resource: named.to_string(), authorization_servers: servers, scopes })
}

// ---- authorization server metadata ----------------------------------------------------------------

/// Where to look for `issuer`'s metadata, in the spec's order. Pure.
pub fn metadata_urls(issuer: &Url) -> Vec<Url> {
    let path = issuer.path().trim_end_matches('/').to_string();
    let at = |p: String| {
        let mut u = issuer.clone();
        u.set_query(None);
        u.set_fragment(None);
        u.set_path(&p);
        u
    };
    if path.is_empty() {
        vec![at("/.well-known/oauth-authorization-server".into()), at("/.well-known/openid-configuration".into())]
    } else {
        vec![
            at(format!("/.well-known/oauth-authorization-server{path}")),
            at(format!("/.well-known/openid-configuration{path}")),
            at(format!("{path}/.well-known/openid-configuration")),
        ]
    }
}

/// Two issuer identifiers that are the same server (a trailing slash
/// aside). Pure.
pub fn same_issuer(a: &str, b: &str) -> bool {
    a.trim_end_matches('/') == b.trim_end_matches('/')
}

/// An authorization server, as its metadata says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    pub endpoints: Endpoints,
    /// It takes a client ID metadata document as a client id.
    pub cimd: bool,
    pub scopes: Vec<String>,
}

/// An authorization server's metadata, checked: its `issuer` the one asked
/// for, PKCE with S256, every endpoint one a sign-in may use. Pure.
pub fn metadata_of(doc: &Value, issuer: &str, place: Place) -> Result<Meta, String> {
    let host = Url::parse(issuer).ok().map(|u| net::host_of(&u)).unwrap_or_else(|| "The sign-in server".into());
    let said = doc["issuer"].as_str().unwrap_or("");
    if !same_issuer(said, issuer) {
        return Err(format!("{host} answered as a different sign-in server than the one named, so Yap stopped there."));
    }
    let s256 = doc["code_challenge_methods_supported"].as_array().is_some_and(|a| a.iter().any(|m| m == "S256"));
    if !s256 {
        return Err(format!("{host}'s sign-in doesn't meet the MCP security rules (no PKCE with S256), so Yap won't use it."));
    }
    if let Some(types) = doc["response_types_supported"].as_array() {
        if !types.iter().any(|t| t == "code") {
            return Err(format!("{host} doesn't offer the sign-in Yap uses (an authorization code)."));
        }
    }
    let required = |k: &str| -> Result<String, String> {
        doc[k].as_str().and_then(|v| endpoint(v, place)).map(|u| u.to_string()).ok_or_else(|| format!("{host}'s sign-in points at an address Yap won't send a sign-in to."))
    };
    let optional = |k: &str| doc[k].as_str().and_then(|v| endpoint(v, place)).map(|u| u.to_string());
    let strings = |k: &str| -> Vec<String> { doc[k].as_array().map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default() };
    Ok(Meta {
        endpoints: Endpoints {
            issuer: issuer.to_string(),
            authorization_endpoint: required("authorization_endpoint")?,
            token_endpoint: required("token_endpoint")?,
            revocation_endpoint: optional("revocation_endpoint"),
            registration_endpoint: optional("registration_endpoint"),
            iss_supported: doc["authorization_response_iss_parameter_supported"].as_bool() == Some(true),
            auth_methods: strings("token_endpoint_auth_methods_supported"),
        },
        cimd: doc["client_id_metadata_document_supported"].as_bool() == Some(true),
        scopes: strings("scopes_supported"),
    })
}

// ---- the requests ------------------------------------------------------------------------------------

/// A sign-in's requests for one connector: its server's place rules, no
/// redirects, the receipts' purpose ("Connector sign-in · A connector").
pub struct Net {
    client: reqwest::Client,
    pub place: Place,
    kind: String,
}

/// An answer's status, `WWW-Authenticate` and JSON body (`Null` when it
/// wasn't JSON).
pub struct Answer {
    pub status: u16,
    pub challenge: Option<String>,
    pub body: Value,
}

impl Net {
    pub fn new(place: Place, kind: String) -> Result<Net, String> {
        let client = remote::client_builder(place)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(TIMEOUT)
            .build()
            .map_err(|_| "Yap couldn't set up the sign-in.".to_string())?;
        Ok(Net { client, place, kind })
    }

    fn purpose(&self) -> Purpose {
        Purpose::ConnectorSignIn(self.kind.clone())
    }

    fn error(&self, url: &Url, e: &reqwest::Error) -> String {
        let host = net::host_of(url);
        if remote::was_private(e) {
            HttpError::Private { host }.to_string()
        } else if e.is_timeout() {
            HttpError::TimedOut { host }.to_string()
        } else {
            HttpError::Unreachable { host }.to_string()
        }
    }

    /// `req` sent and its answer read (at most [`MAX_BODY`]).
    pub async fn send(&self, url: &Url, req: reqwest::RequestBuilder) -> Result<Answer, String> {
        let mut resp = req.send_for(self.purpose()).await.map_err(|e| self.error(url, &e))?;
        let status = resp.status().as_u16();
        let challenge = resp.headers().get(reqwest::header::WWW_AUTHENTICATE).and_then(|v| v.to_str().ok()).map(str::to_string);
        let mut bytes = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(|e| self.error(url, &e))? {
            if bytes.len() + chunk.len() > MAX_BODY {
                return Err(HttpError::TooBig { host: net::host_of(url) }.to_string());
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(Answer { status, challenge, body: serde_json::from_slice(&bytes).unwrap_or(Value::Null) })
    }

    pub fn get(&self, url: &Url) -> reqwest::RequestBuilder {
        self.client.get(url.clone()).header(reqwest::header::ACCEPT, "application/json")
    }

    pub fn post(&self, url: &Url) -> reqwest::RequestBuilder {
        self.client.post(url.clone()).header(reqwest::header::ACCEPT, "application/json")
    }

    /// A JSON document at `url`: `None` when there's none there (any answer
    /// but a 200 with JSON).
    pub async fn document(&self, url: &Url) -> Result<Option<Value>, String> {
        let a = self.send(url, self.get(url)).await?;
        Ok((a.status == 200 && a.body.is_object()).then_some(a.body))
    }

    /// The server asked without a token: its status and challenge.
    pub async fn probe(&self, server: &Url) -> Result<Answer, String> {
        let hello = serde_json::json!({
            "jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": { "protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": { "name": "yap", "version": env!("CARGO_PKG_VERSION") } }
        });
        let req = self.post(server).header(reqwest::header::ACCEPT, "application/json, text/event-stream").json(&hello);
        self.send(server, req).await
    }
}

/// The server's protected resource metadata, from the first address that
/// has it.
pub async fn resource(net: &Net, server: &Url, header: Option<&str>) -> Result<Resource, String> {
    for url in resource_urls(server, header, net.place) {
        if let Some(doc) = net.document(&url).await? {
            return resource_of(&doc, server, net.place);
        }
    }
    Err(format!("{} doesn't say how to sign in to it.", net::host_of(server)))
}

/// `issuer`'s metadata, from the first address that has it.
pub async fn metadata(net: &Net, issuer: &str) -> Result<Meta, String> {
    let url = endpoint(issuer, net.place).ok_or_else(|| "The sign-in server's address isn't one Yap may use.".to_string())?;
    for at in metadata_urls(&url) {
        if let Some(doc) = net.document(&at).await? {
            return metadata_of(&doc, issuer, net.place);
        }
    }
    Err(format!("Couldn't find {}'s sign-in details.", net::host_of(&url)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_challenge_is_read_whichever_way_its_written() {
        let c = challenge(r#"Bearer resource_metadata="https://mcp.example.com/.well-known/oauth-protected-resource/mcp", scope="files:read files:write""#);
        assert_eq!(c.resource_metadata.as_deref(), Some("https://mcp.example.com/.well-known/oauth-protected-resource/mcp"));
        assert_eq!(c.scope.as_deref(), Some("files:read files:write"));
        let c = challenge(r#"Basic realm="x", Bearer error="insufficient_scope", scope=read, resource_metadata="https://a.example.com/m""#);
        assert_eq!((c.error.as_deref(), c.scope.as_deref(), c.resource_metadata.as_deref()), (Some("insufficient_scope"), Some("read"), Some("https://a.example.com/m")));
        // Another scheme's parameters aren't the Bearer challenge's.
        let c = challenge(r#"Basic resource_metadata="https://evil.example.com/m""#);
        assert_eq!(c, Challenge::default());
        assert_eq!(challenge("Bearer").resource_metadata, None);
        assert_eq!(challenge(r#"bearer scope="a \"b\"""#).scope.as_deref(), Some("a \"b\""));
        assert_eq!(split_scopes(" read  write read "), ["read", "write"]);
    }

    #[test]
    fn a_sign_in_only_goes_where_its_server_may() {
        let internet = Place::Internet;
        assert!(endpoint("https://auth.example.com/token", internet).is_some());
        assert!(endpoint("http://auth.example.com/token", internet).is_none(), "plain http on the internet");
        assert!(endpoint("https://127.0.0.1/token", internet).is_none(), "an internet server's sign-in on this PC");
        assert!(endpoint("https://192.168.1.4/token", internet).is_none());
        assert!(endpoint("https://169.254.169.254/latest", Place::Network).is_none(), "a metadata address");
        assert!(endpoint("https://user:pw@auth.example.com/token", internet).is_none());
        assert!(endpoint("https://auth.example.com/token#x", internet).is_none());
        assert!(endpoint("ftp://auth.example.com/", internet).is_none());
        // A server on this PC signs in on this PC (the e2e suite's fakes).
        assert!(endpoint("http://127.0.0.1:8123/token", Place::Pc).is_some());
        assert!(endpoint("https://auth.example.com/token", Place::Pc).is_some());
        assert!(endpoint("http://auth.example.com/token", Place::Pc).is_none());
    }

    #[test]
    fn the_resource_must_be_this_server() {
        let server = Url::parse("https://mcp.example.com/v1/mcp").unwrap();
        let ok = |s: &str| resource_matches(&Url::parse(s).unwrap(), &server);
        assert!(ok("https://mcp.example.com/v1/mcp"));
        assert!(ok("https://mcp.example.com/v1/mcp/"));
        assert!(ok("https://mcp.example.com"));
        assert!(ok("https://mcp.example.com/v1"));
        assert!(!ok("https://mcp.example.com/v2"));
        assert!(!ok("https://mcp.example.com/v1/mcp/more"));
        assert!(!ok("https://other.example.com/v1/mcp"));
        assert!(!ok("http://mcp.example.com/v1/mcp"));
        assert!(!ok("https://mcp.example.com:8443/v1/mcp"));
        assert!(!ok("https://mcp.example.com/v1/mcp#x"));
        let doc = json!({ "resource": "https://evil.example.com/mcp", "authorization_servers": ["https://auth.example.com"] });
        assert!(resource_of(&doc, &server, Place::Internet).unwrap_err().contains("for another server (evil.example.com)"));
        let doc = json!({ "resource": "https://mcp.example.com/v1/mcp", "authorization_servers": ["http://auth.example.com", "https://auth.example.com/tenant"], "scopes_supported": ["read"] });
        let r = resource_of(&doc, &server, Place::Internet).unwrap();
        assert_eq!(r.authorization_servers, ["https://auth.example.com/tenant"], "only the ones a sign-in may use");
        assert_eq!(r.scopes, ["read"]);
        let none = json!({ "resource": "https://mcp.example.com/v1/mcp", "authorization_servers": ["http://auth.example.com"] });
        assert!(resource_of(&none, &server, Place::Internet).is_err());
    }

    #[test]
    fn metadata_is_looked_for_in_the_specs_order() {
        let server = Url::parse("https://mcp.example.com/v1/mcp?x=1").unwrap();
        let urls: Vec<String> = resource_urls(&server, Some("https://mcp.example.com/prm"), Place::Internet).iter().map(Url::to_string).collect();
        assert_eq!(urls, ["https://mcp.example.com/prm", "https://mcp.example.com/.well-known/oauth-protected-resource/v1/mcp", "https://mcp.example.com/.well-known/oauth-protected-resource"]);
        // A challenge pointing at this PC for a server on the internet isn't followed.
        let urls = resource_urls(&server, Some("http://127.0.0.1/prm"), Place::Internet);
        assert_eq!(urls.len(), 2);
        let root = Url::parse("https://mcp.example.com/").unwrap();
        assert_eq!(resource_urls(&root, None, Place::Internet).len(), 1);

        let with_path: Vec<String> = metadata_urls(&Url::parse("https://auth.example.com/tenant1").unwrap()).iter().map(Url::to_string).collect();
        assert_eq!(
            with_path,
            [
                "https://auth.example.com/.well-known/oauth-authorization-server/tenant1",
                "https://auth.example.com/.well-known/openid-configuration/tenant1",
                "https://auth.example.com/tenant1/.well-known/openid-configuration",
            ]
        );
        let bare: Vec<String> = metadata_urls(&Url::parse("https://auth.example.com/").unwrap()).iter().map(Url::to_string).collect();
        assert_eq!(bare, ["https://auth.example.com/.well-known/oauth-authorization-server", "https://auth.example.com/.well-known/openid-configuration"]);
    }

    fn doc() -> Value {
        json!({
            "issuer": "https://auth.example.com",
            "authorization_endpoint": "https://auth.example.com/authorize",
            "token_endpoint": "https://login.example.net/token",
            "revocation_endpoint": "https://auth.example.com/revoke",
            "registration_endpoint": "http://auth.example.com/register",
            "code_challenge_methods_supported": ["plain", "S256"],
            "response_types_supported": ["code"],
            "client_id_metadata_document_supported": true,
            "authorization_response_iss_parameter_supported": true,
            "scopes_supported": ["read", "offline_access"],
            "token_endpoint_auth_methods_supported": ["none", "client_secret_post"]
        })
    }

    #[test]
    fn the_issuer_must_be_the_one_asked_for_and_pkce_s256_offered() {
        let m = metadata_of(&doc(), "https://auth.example.com/", Place::Internet).unwrap();
        assert_eq!(m.endpoints.token_endpoint, "https://login.example.net/token", "another host, on the internet: allowed");
        assert_eq!(m.endpoints.registration_endpoint, None, "plain http dropped");
        assert!(m.cimd && m.endpoints.iss_supported);
        assert_eq!(m.endpoints.auth_methods, ["none", "client_secret_post"]);
        assert_eq!(m.scopes, ["read", "offline_access"]);
        assert!(metadata_of(&doc(), "https://auth.example.org", Place::Internet).unwrap_err().contains("different sign-in server"));
        let mut no_pkce = doc();
        no_pkce.as_object_mut().unwrap().remove("code_challenge_methods_supported");
        assert!(metadata_of(&no_pkce, "https://auth.example.com", Place::Internet).unwrap_err().contains("MCP security rules"));
        let mut plain = doc();
        plain["code_challenge_methods_supported"] = json!(["plain"]);
        assert!(metadata_of(&plain, "https://auth.example.com", Place::Internet).is_err());
        let mut here = doc();
        here["token_endpoint"] = json!("https://127.0.0.1/token");
        assert!(metadata_of(&here, "https://auth.example.com", Place::Internet).unwrap_err().contains("won't send a sign-in to"));
        let mut implicit = doc();
        implicit["response_types_supported"] = json!(["token"]);
        assert!(metadata_of(&implicit, "https://auth.example.com", Place::Internet).is_err());
    }
}

//! **Which client Yap signs in as**, in the MCP spec's order ([`pick`]):
//!
//! 1. **A pre-registered client** for that server: the person's own client
//!    id, when they entered one (`connector_set_client`: their explicit
//!    choice, so it comes first), else Yap's own app with that vendor from
//!    the catalogue ([`catalog_client`]: none yet; the hook is here).
//! 2. **Yap's client ID metadata document** (CIMD), when the authorization
//!    server says `client_id_metadata_document_supported`: the document's
//!    URL is the client id, nothing is registered. Skipped while
//!    [`METADATA_URL`] isn't set (its hosting is the maintainer's to
//!    approve); [`metadata_document`] is the file to host.
//! 3. **Dynamic client registration** (RFC 7591), when the server has a
//!    `registration_endpoint`: once per server, kept in connectors.json for
//!    that issuer only (its secret, if any, in the vault), reused, never sent
//!    to another issuer ([`reusable`]); registered again when the issuer
//!    changes.
//! 4. **Ask**: "This server needs an app registration…" — the person enters
//!    a client id (and secret) by hand, which is then 1.
//!
//! A pasted token (`Spec::token`) never comes here: it's sent as it is.

use serde_json::{json, Value};

use super::super::store::{OwnClient, Registration};
use super::discover::Meta;
use super::loopback;

/// The name an authorization server shows on its consent page (CIMD and
/// registrations). It changes with the rename; the CIMD's URL must not.
pub const CLIENT_NAME: &str = "Yap";

/// **Yap's client ID metadata document's address** (the CIMD client id):
/// not set until the maintainer approves where it's hosted (a static file
/// on a stable domain that survives the rename, never the account
/// service). While it's `None`, CIMD is skipped and DCR or the person's own
/// client is used.
pub const METADATA_URL: Option<&str> = None;

/// The CIMD address this run uses: [`METADATA_URL`], or in a debug build's
/// test run `YAP_E2E_CIMD_URL` (the e2e suite's stand-in).
pub fn metadata_url() -> Option<String> {
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() {
        if let Ok(v) = std::env::var("YAP_E2E_CIMD_URL") {
            let v = v.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    METADATA_URL.map(str::to_string)
}

/// The redirects a client of Yap's lists: the fixed loopback port (servers
/// that match exactly), the port this sign-in listens on if it's another,
/// and the port-less form (RFC 8252's any-port loopback). Never
/// `localhost`: Yap only ever listens on `127.0.0.1`. Pure.
pub fn redirect_uris(port: u16) -> Vec<String> {
    let mut out = vec![loopback::redirect_uri(loopback::PORT)];
    if port != loopback::PORT {
        out.push(loopback::redirect_uri(port));
    }
    out.push(format!("http://127.0.0.1{}", loopback::CALLBACK));
    out
}

/// **The client ID metadata document to host** at `client_id` (its own
/// URL), with the website's address and logo when there are ones. Pure.
/// (Nothing in Yap serves it: it's the file the maintainer hosts, held to
/// this by its test.)
#[cfg_attr(not(test), allow(dead_code))]
pub fn metadata_document(client_id: &str, client_uri: Option<&str>, logo_uri: Option<&str>) -> Value {
    let mut doc = json!({
        "client_id": client_id,
        "client_name": CLIENT_NAME,
        "redirect_uris": [loopback::redirect_uri(loopback::PORT), format!("http://127.0.0.1{}", loopback::CALLBACK)],
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
        "application_type": "native",
    });
    if let Some(u) = client_uri {
        doc["client_uri"] = json!(u);
    }
    if let Some(u) = logo_uri {
        doc["logo_uri"] = json!(u);
    }
    doc
}

/// Yap's own app with a vendor (the catalogue's `client: "catalog"`): its
/// id, and a secret built in where the vendor's desktop flow needs one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogClient {
    pub issuer: &'static str,
    pub client_id: &'static str,
    pub secret: Option<&'static str>,
}

/// Yap's registrations with vendors: none yet (catalogue entries come in
/// wave 5b; Yap's own apps later, the design's "Later").
const CATALOG: &[CatalogClient] = &[];

/// The catalogue's client for `issuer`, if Yap has one there.
pub fn catalog_client(issuer: &str) -> Option<&'static CatalogClient> {
    CATALOG.iter().find(|c| super::discover::same_issuer(c.issuer, issuer))
}

/// The client a sign-in will use ([module docs](self)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    Own(String),
    Catalog(&'static CatalogClient),
    Cimd(String),
    /// The registration Yap made with this issuer before.
    Registered(Registration),
    /// Register at this endpoint now.
    Register(String),
    /// None: ask the person for a client id.
    Ask,
}

/// A registration that may be used with `issuer` (made with it). Pure.
pub fn reusable<'a>(registration: Option<&'a Registration>, issuer: &str) -> Option<&'a Registration> {
    registration.filter(|r| super::discover::same_issuer(&r.issuer, issuer))
}

/// The client to sign in as at `meta`'s server, in the spec's order
/// ([module docs](self)). Pure.
pub fn pick(own: Option<&OwnClient>, catalog: Option<&'static CatalogClient>, cimd: Option<&str>, meta: &Meta, registration: Option<&Registration>) -> Pick {
    if let Some(o) = own.filter(|o| !o.client_id.trim().is_empty()) {
        return Pick::Own(o.client_id.clone());
    }
    if let Some(c) = catalog {
        return Pick::Catalog(c);
    }
    if let Some(url) = cimd.filter(|_| meta.cimd) {
        return Pick::Cimd(url.to_string());
    }
    if let Some(r) = reusable(registration, &meta.endpoints.issuer) {
        return Pick::Registered(r.clone());
    }
    if let Some(endpoint) = &meta.endpoints.registration_endpoint {
        return Pick::Register(endpoint.clone());
    }
    Pick::Ask
}

/// What Yap sends to register (RFC 7591, a native public client). Pure.
pub fn registration_request(redirects: &[String], scope: &[String]) -> Value {
    let mut body = json!({
        "client_name": CLIENT_NAME,
        "redirect_uris": redirects,
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
        "application_type": "native",
    });
    if !scope.is_empty() {
        body["scope"] = json!(scope.join(" "));
    }
    body
}

/// A registration's answer: the registration to keep for `issuer`, and its
/// secret if it was given one. Pure.
pub fn registration_of(body: &Value, issuer: &str, redirects: &[String], now: u64) -> Option<(Registration, Option<String>)> {
    let client_id = body["client_id"].as_str().map(str::trim).filter(|s| !s.is_empty() && s.len() <= 512)?;
    let secret = body["client_secret"].as_str().filter(|s| !s.is_empty()).map(str::to_string);
    let method = body["token_endpoint_auth_method"].as_str().map(str::to_string).unwrap_or_else(|| if secret.is_some() { "client_secret_basic".into() } else { "none".into() });
    Some((
        Registration { issuer: issuer.to_string(), client_id: client_id.to_string(), auth_method: method, redirect_uris: redirects.to_vec(), registered: now },
        secret,
    ))
}

/// What the person is told when no client is possible.
pub fn ask_words(host: &str) -> String {
    format!(
        "{host} needs an app registration Yap doesn't have. Ask its admin for a client ID (with {} as its redirect), enter it for this connector, then sign in again. Or use a token, if it offers one.",
        loopback::redirect_uri(loopback::PORT)
    )
}

#[cfg(test)]
mod tests {
    use super::super::super::store::Endpoints;
    use super::*;

    fn meta(cimd: bool, registration: bool) -> Meta {
        Meta {
            endpoints: Endpoints {
                issuer: "https://auth.example.com".into(),
                authorization_endpoint: "https://auth.example.com/authorize".into(),
                token_endpoint: "https://auth.example.com/token".into(),
                registration_endpoint: registration.then(|| "https://auth.example.com/register".to_string()),
                ..Default::default()
            },
            cimd,
            scopes: vec![],
        }
    }

    fn registered(issuer: &str) -> Registration {
        Registration { issuer: issuer.into(), client_id: "dcr-1".into(), auth_method: "none".into(), redirect_uris: vec![], registered: 1 }
    }

    static VENDOR: CatalogClient = CatalogClient { issuer: "https://auth.example.com", client_id: "yap-vendor", secret: None };

    #[test]
    fn the_client_is_picked_in_the_specs_order() {
        let own = OwnClient { client_id: "my-client".into() };
        let cimd = Some("https://example.com/oauth/desktop-client.json");
        let r = registered("https://auth.example.com");
        // The person's own, entered by hand, before anything else.
        assert_eq!(pick(Some(&own), Some(&VENDOR), cimd, &meta(true, true), Some(&r)), Pick::Own("my-client".into()));
        // Then the catalogue's.
        assert_eq!(pick(None, Some(&VENDOR), cimd, &meta(true, true), Some(&r)), Pick::Catalog(&VENDOR));
        // Then CIMD, when the server takes it and Yap's document has an address.
        assert_eq!(pick(None, None, cimd, &meta(true, true), Some(&r)), Pick::Cimd("https://example.com/oauth/desktop-client.json".into()));
        assert_eq!(pick(None, None, None, &meta(true, true), None), Pick::Register("https://auth.example.com/register".into()), "no address yet: skipped");
        assert_eq!(pick(None, None, cimd, &meta(false, true), None), Pick::Register("https://auth.example.com/register".into()), "the server doesn't take it");
        // Then a registration made before, with this issuer only.
        assert_eq!(pick(None, None, None, &meta(false, true), Some(&r)), Pick::Registered(r.clone()));
        let elsewhere = registered("https://other.example.com");
        assert_eq!(pick(None, None, None, &meta(false, true), Some(&elsewhere)), Pick::Register("https://auth.example.com/register".into()), "never another issuer's");
        assert_eq!(pick(None, None, None, &meta(false, false), Some(&elsewhere)), Pick::Ask);
        // Then ask.
        assert_eq!(pick(None, None, None, &meta(false, false), None), Pick::Ask);
        assert_eq!(pick(Some(&OwnClient { client_id: " ".into() }), None, None, &meta(false, false), None), Pick::Ask, "a blank id isn't one");
        assert_eq!(catalog_client("https://auth.example.com"), None, "the catalogue is empty for now");
    }

    #[test]
    fn yaps_document_and_its_registrations_are_a_native_public_client_on_the_loopback() {
        let doc = metadata_document("https://example.com/oauth/desktop-client.json", Some("https://example.com"), None);
        assert_eq!(
            doc,
            json!({
                "client_id": "https://example.com/oauth/desktop-client.json",
                "client_name": "Yap",
                "client_uri": "https://example.com",
                "redirect_uris": ["http://127.0.0.1:51439/connectors/callback", "http://127.0.0.1/connectors/callback"],
                "grant_types": ["authorization_code", "refresh_token"],
                "response_types": ["code"],
                "token_endpoint_auth_method": "none",
                "application_type": "native",
            })
        );
        assert_eq!(redirect_uris(51439), ["http://127.0.0.1:51439/connectors/callback", "http://127.0.0.1/connectors/callback"]);
        assert_eq!(redirect_uris(50001)[1], "http://127.0.0.1:50001/connectors/callback", "a fallback port is registered too");
        let body = registration_request(&redirect_uris(51439), &["read".into(), "offline_access".into()]);
        assert_eq!(body["application_type"], "native");
        assert_eq!(body["token_endpoint_auth_method"], "none");
        assert_eq!(body["scope"], "read offline_access");
        assert!(registration_request(&[], &[]).get("scope").is_none());
        let (r, secret) = registration_of(&json!({ "client_id": "dcr-9", "client_secret": "cs-FAKE" }), "https://auth.example.com", &[], 5).unwrap();
        assert_eq!((r.client_id.as_str(), r.auth_method.as_str(), secret.as_deref()), ("dcr-9", "client_secret_basic", Some("cs-FAKE")));
        let (r, secret) = registration_of(&json!({ "client_id": "dcr-9", "token_endpoint_auth_method": "none" }), "https://auth.example.com", &[], 5).unwrap();
        assert_eq!((r.auth_method.as_str(), secret), ("none", None));
        assert!(registration_of(&json!({ "error": "invalid_client_metadata" }), "https://auth.example.com", &[], 5).is_none());
        assert!(ask_words("mcp.example.com").contains("http://127.0.0.1:51439/connectors/callback"));
    }
}

//! **The person's connectors** (`connectors.json`): what each is, how it
//! connects (a link or a program on this PC), how it signs in (never a
//! secret: those are in the vault, `vault.rs`), where Yap may use it, and
//! each tool's setting, pinned to the hash of the definition it was given
//! for (`cache.rs` compares).
//!
//! A file of its own, so an older Yap's save of config.json never drops it;
//! each connector and the file keep fields this build doesn't know
//! (`extra`), so a newer Yap's (sharing the data folder) survive this one's
//! save. Ids are `c1`…`c24` ([`MAX`]), the lowest free one given, so the
//! uninstaller knows every vault name. Saved through `saving.rs` (a failed
//! save kept in memory and said once; a file that couldn't be read never
//! written over). Unit tests keep a store per test thread.

#[cfg(not(test))]
use std::sync::Mutex;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// At most this many connectors (`c1`…`c24`).
pub const MAX: usize = 24;
#[cfg(not(test))]
const FILE: &str = "connectors.json";

/// A tool's setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Setting {
    /// Always allow.
    Allow,
    /// Ask first.
    Ask,
    /// Off: never offered.
    Off,
}

impl Setting {
    pub fn parse(s: &str) -> Option<Setting> {
        match s {
            "allow" => Some(Setting::Allow),
            "ask" => Some(Setting::Ask),
            "off" => Some(Setting::Off),
            _ => None,
        }
    }
}

/// One tool's setting, and the definition's hash it was set for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSetting {
    pub setting: Setting,
    pub hash: String,
}

/// A request header a link sends: its name only (its value is in the vault).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub name: String,
}

/// A program's environment variable: a plain one's value is here, a
/// secret's (`secret`) is in the vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvVar {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub secret: bool,
}

/// How a connector is reached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Transport {
    /// A server at a link: Streamable HTTP, or the older HTTP+SSE.
    #[serde(rename_all = "camelCase")]
    Remote {
        url: String,
        /// The older HTTP+SSE transport's address, tried when the link
        /// answers a POST with 404 or 405 (a catalogue entry may name one).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fallback_sse: Option<String>,
        /// The link itself is an HTTP+SSE stream (an older server).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        sse: bool,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        headers: Vec<Header>,
    },
    /// A program on this PC, over stdio.
    #[serde(rename_all = "camelCase")]
    Local {
        /// As the person gave it ("npx").
        command: String,
        /// Its full path, as shown before it first ran.
        #[serde(default)]
        resolved: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        env: Vec<EnvVar>,
        /// The fingerprint of the command line the person saw and allowed
        /// (`local::fingerprint`): a program whose command line no longer
        /// matches it never starts.
        #[serde(default)]
        approved: String,
    },
}

impl Transport {
    pub fn is_local(&self) -> bool {
        matches!(self, Transport::Local { .. })
    }
}

/// How a connector signs in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthKind {
    #[default]
    None,
    /// `Authorization: Bearer <token>` (the token in the vault).
    Bearer,
    /// Request headers ([`Transport::Remote`]'s `headers`, values in the vault).
    Header,
    /// Signed in through the browser (`oauth.rs`; the tokens in the vault).
    Oauth,
}

/// Which client a browser sign-in used (`oauth/client.rs` picks).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClientKind {
    /// The person's own client id, entered by hand (its secret, if any, in
    /// the vault).
    Own,
    /// Yap's own app with that vendor, from the catalogue.
    Catalog,
    /// Yap's client ID metadata document: its URL is the client id.
    Cimd,
    /// A registration Yap made with the server (RFC 7591), kept per issuer.
    Dcr,
}

/// The client a sign-in used. Not a secret: a client id is public.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientRef {
    pub kind: ClientKind,
    pub client_id: String,
}

/// A registration Yap made with an authorization server (dynamic client
/// registration), for that issuer only: never sent to another, dropped when
/// the issuer changes. Its secret, if it was given one, is in the vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Registration {
    pub issuer: String,
    pub client_id: String,
    /// How it authenticates at the token endpoint (`none`,
    /// `client_secret_post`, `client_secret_basic`).
    #[serde(default = "none_method")]
    pub auth_method: String,
    #[serde(default)]
    pub redirect_uris: Vec<String>,
    #[serde(default)]
    pub registered: u64,
}

fn none_method() -> String {
    "none".into()
}

/// The person's own client id (a secret for it is in the vault).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnClient {
    pub client_id: String,
}

/// The authorization server's endpoints as discovered at the last sign-in
/// (`oauth/discover.rs`, each checked): refreshes and sign-out use them
/// without discovering again.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Endpoints {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revocation_endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registration_endpoint: Option<String>,
    /// RFC 9207: the server says it puts `iss` on its answers.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub iss_supported: bool,
    /// `token_endpoint_auth_methods_supported` (none listed: RFC 8414's
    /// default, `client_secret_basic`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub auth_methods: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Auth {
    #[serde(default)]
    pub kind: AuthKind,
    /// A browser sign-in's authorization server (its issuer), as the
    /// server's protected resource metadata named it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    /// The resource the tokens are for (RFC 8707): the server's own address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    /// The client the sign-in used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<ClientRef>,
    /// The scopes granted (or asked for, when the server didn't say).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub signed_in: bool,
    /// The sign-in ended (a refresh turned down, the tokens refused):
    /// "Sign in again".
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub expired: bool,
    /// Scopes the server asked for since (`403 insufficient_scope`): the
    /// next sign-in asks for them too ("step-up").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wants: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<Endpoints>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registration: Option<Registration>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub own: Option<OwnClient>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Where Yap may use a connector's tools (wave 4 and 6 read it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Where {
    #[serde(default = "yes")]
    pub chat: bool,
    #[serde(default = "yes")]
    pub voice: bool,
    #[serde(default)]
    pub phone: bool,
    #[serde(default = "yes")]
    pub plan: bool,
    #[serde(default)]
    pub meetings: bool,
}

impl Default for Where {
    fn default() -> Self {
        Where { chat: true, voice: true, phone: false, plan: true, meetings: false }
    }
}

fn yes() -> bool {
    true
}

/// Where a connector came from: `custom` (added by hand) until the
/// catalogue (wave 5b) and the import (`import`) add theirs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Origin {
    #[serde(default = "custom")]
    pub kind: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Origin {
    fn default() -> Self {
        Origin { kind: custom(), extra: Map::new() }
    }
}

fn custom() -> String {
    "custom".into()
}

/// One connector.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connector {
    /// `c1`…`c24`.
    pub id: String,
    /// Its tools' prefix (`names.rs`).
    pub slug: String,
    /// The person's name for it (shown; never logged above debug, never a
    /// receipt's label).
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub origin: Origin,
    pub transport: Transport,
    #[serde(default)]
    pub auth: Auth,
    #[serde(default, rename = "where")]
    pub where_: Where,
    /// Each tool's setting, by the tool's own name.
    #[serde(default)]
    pub tools: BTreeMap<String, ToolSetting>,
    /// When it was added (unix seconds).
    #[serde(default)]
    pub added: u64,
    /// How long a call may take before Yap stops waiting (seconds; 60 by
    /// default, at most 600).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
    /// When one of its tools was last called (unix seconds; saved at most
    /// once a minute: [`used`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used: Option<u64>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Connector {
    /// The receipts' and the logs' word for it: never the name the person
    /// typed. The catalogue's name for one from the catalogue ("Notion").
    pub fn kind(&self) -> String {
        super::catalog::of(&self.origin).map_or_else(|| "A connector".into(), |e| e.name.clone())
    }
}

/// What connectors.json holds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct File {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub connectors: Vec<Connector>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// connectors.json's saves.
pub static SAVER: crate::shell::saving::Saver =
    crate::shell::saving::Saver::new("connectors", "your connectors", save_now);

fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    with_store(|s| save(s));
    SAVER.check()
}

#[cfg(not(test))]
static STATE: Mutex<Option<File>> = Mutex::new(None);

#[cfg(not(test))]
fn path() -> std::path::PathBuf {
    crate::config::data_dir().join(FILE)
}

#[cfg(not(test))]
fn save(store: &File) {
    if crate::ai::eval::active() {
        return;
    }
    let mut out = store.clone();
    out.version = 1;
    let _ = SAVER.write_json(&path(), &out, true);
}

#[cfg(not(test))]
fn with_store<R>(f: impl FnOnce(&mut File) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let store = guard.get_or_insert_with(|| SAVER.load(&path()));
    if SAVER.any_unread() && SAVER.refresh(&path(), store) {
        save(store);
    }
    f(store)
}

#[cfg(test)]
thread_local! {
    static TEST_STORE: std::cell::RefCell<Option<File>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn save(_: &File) {}

#[cfg(test)]
fn with_store<R>(f: impl FnOnce(&mut File) -> R) -> R {
    TEST_STORE.with(|s| f(s.borrow_mut().get_or_insert_with(File::default)))
}

/// Whether the store was read (or made) in this run: never while the
/// preview is off (the hub's test).
#[cfg(test)]
pub fn touched() -> bool {
    TEST_STORE.with(|s| s.borrow().is_some())
}

/// A test starts with no store (as a fresh run would).
#[cfg(test)]
pub fn reset_for_test() {
    TEST_STORE.with(|s| *s.borrow_mut() = None);
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Every connector, in the order they were added.
pub fn list() -> Vec<Connector> {
    with_store(|s| s.connectors.clone())
}

pub fn get(id: &str) -> Option<Connector> {
    with_store(|s| s.connectors.iter().find(|c| c.id == id).cloned())
}

/// The lowest free id, `c1`…`c24`.
fn free_id(file: &File) -> Option<String> {
    (1..=MAX).map(|n| format!("c{n}")).find(|id| !file.connectors.iter().any(|c| &c.id == id))
}

/// A new connector, before it has an id and a slug.
#[derive(Debug, Clone)]
pub struct New {
    pub name: String,
    pub transport: Transport,
    pub auth: AuthKind,
}

/// Add a connector: its id (the lowest free), its slug (unique), now.
/// Saved. `Err`: there are [`MAX`] already, or the save failed.
pub fn add(new: New) -> Result<Connector, String> {
    let c = with_store(|s| {
        let id = free_id(s).ok_or_else(|| format!("Yap keeps at most {MAX} connectors. Remove one first."))?;
        let taken: Vec<&str> = s.connectors.iter().map(|c| c.slug.as_str()).collect();
        let slug = super::names::slug(&new.name, &taken);
        let c = Connector {
            id,
            slug,
            name: new.name,
            enabled: true,
            origin: Origin::default(),
            transport: new.transport,
            auth: Auth { kind: new.auth, ..Default::default() },
            where_: Where::default(),
            tools: BTreeMap::new(),
            added: now(),
            timeout_secs: None,
            last_used: None,
            extra: Map::new(),
        };
        s.connectors.push(c.clone());
        save(s);
        Ok::<_, String>(c)
    })?;
    SAVER.check()?;
    Ok(c)
}

/// Change connector `id` (`f` returns what to answer). Saved. `Err`: no such
/// connector, or the save failed.
pub fn update<R>(id: &str, f: impl FnOnce(&mut Connector) -> R) -> Result<R, String> {
    let out = with_store(|s| {
        let c = s.connectors.iter_mut().find(|c| c.id == id)?;
        let out = f(c);
        save(s);
        Some(out)
    })
    .ok_or_else(|| "There's no such connector.".to_string())?;
    SAVER.check()?;
    Ok(out)
}

/// Connector `id`'s tool was just called: `last_used` now, saved only when
/// the one kept is a minute old or more (a burst of calls is one save).
/// Whether it was saved.
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub fn used(id: &str) -> bool {
    let t = now();
    let stale = with_store(|s| s.connectors.iter().find(|c| c.id == id).is_some_and(|c| c.last_used.is_none_or(|u| t.saturating_sub(u) >= 60)));
    stale && update(id, |c| c.last_used = Some(t)).is_ok()
}

/// Remove connector `id`; what it was. Saved.
pub fn remove(id: &str) -> Result<Option<Connector>, String> {
    let gone = with_store(|s| {
        let at = s.connectors.iter().position(|c| c.id == id)?;
        let c = s.connectors.remove(at);
        save(s);
        Some(c)
    });
    SAVER.check()?;
    Ok(gone)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn link(name: &str) -> New {
        New { name: name.into(), transport: Transport::Remote { url: "https://mcp.example.com/mcp".into(), fallback_sse: None, sse: false, headers: vec![] }, auth: AuthKind::Bearer }
    }

    #[test]
    fn ids_are_the_lowest_free_and_slugs_unique() {
        reset_for_test();
        let a = add(link("Notion")).unwrap();
        let b = add(link("Notion")).unwrap();
        assert_eq!((a.id.as_str(), a.slug.as_str()), ("c1", "notion"));
        assert_eq!((b.id.as_str(), b.slug.as_str()), ("c2", "notion2"));
        assert_eq!(remove("c1").unwrap().map(|c| c.id), Some("c1".into()));
        let c = add(link("Linear")).unwrap();
        assert_eq!(c.id, "c1", "the lowest free id again");
        for i in 0..(MAX - 2) {
            add(link(&format!("More {i}"))).unwrap();
        }
        let err = add(link("One too many")).unwrap_err();
        assert!(err.contains("24"), "{err}");
        assert_eq!(list().len(), MAX);
        assert!(update("c99", |_| ()).is_err());
        assert!(update("c2", |c| c.enabled = false).is_ok());
        assert!(!get("c2").unwrap().enabled);
    }

    #[test]
    fn the_file_keeps_names_never_secrets_and_keeps_fields_it_doesnt_know() {
        reset_for_test();
        let mut c = add(New {
            name: "Files".into(),
            transport: Transport::Local {
                command: "npx".into(),
                resolved: "C:\\Program Files\\nodejs\\npx.cmd".into(),
                args: vec!["-y".into(), "@modelcontextprotocol/server-filesystem".into()],
                env: vec![EnvVar { name: "GITHUB_TOKEN".into(), value: None, secret: true }, EnvVar { name: "LOG".into(), value: Some("1".into()), secret: false }],
                approved: "abc".into(),
            },
            auth: AuthKind::None,
        })
        .unwrap();
        c.tools.insert("read_file".into(), ToolSetting { setting: Setting::Allow, hash: "9f2c".into() });
        let v = serde_json::to_value(&c).unwrap();
        assert_eq!(v["transport"]["kind"], "local");
        assert_eq!(v["transport"]["env"], json!([{ "name": "GITHUB_TOKEN", "secret": true }, { "name": "LOG", "value": "1" }]));
        assert_eq!(v["where"], json!({ "chat": true, "voice": true, "phone": false, "plan": true, "meetings": false }));
        assert_eq!(v["tools"]["read_file"], json!({ "setting": "allow", "hash": "9f2c" }));
        assert_eq!(v["origin"], json!({ "kind": "custom" }));
        // A newer Yap's fields come back out as they went in.
        let newer = json!({
            "version": 2,
            "chatsOff": { "7": ["c1"] },
            "connectors": [{ "id": "c1", "slug": "notion", "name": "Notion", "transport": { "kind": "remote", "url": "https://mcp.example.com/mcp", "fallbackSse": "https://mcp.example.com/sse" },
              "auth": { "kind": "oauth", "issuer": "https://mcp.example.com" }, "catalogVersion": 3 }]
        });
        let file: File = serde_json::from_value(newer.clone()).unwrap();
        assert_eq!(file.connectors[0].auth.kind, AuthKind::Oauth);
        assert!(file.connectors[0].enabled, "on unless said");
        let back = serde_json::to_value(&file).unwrap();
        assert_eq!(back["chatsOff"], newer["chatsOff"]);
        assert_eq!(back["connectors"][0]["catalogVersion"], 3);
        assert_eq!(back["connectors"][0]["auth"]["issuer"], "https://mcp.example.com");
        assert_eq!(back["connectors"][0]["transport"]["fallbackSse"], "https://mcp.example.com/sse");
    }

    #[test]
    fn a_sign_in_keeps_what_it_found_and_chose_and_never_a_token() {
        let file: File = serde_json::from_value(json!({ "connectors": [{
            "id": "c3", "slug": "notion", "name": "Notion",
            "transport": { "kind": "remote", "url": "https://mcp.example.com/mcp" },
            "auth": {
                "kind": "oauth", "issuer": "https://auth.example.com", "resource": "https://mcp.example.com/mcp",
                "client": { "kind": "dcr", "clientId": "yap-123" }, "scopes": ["read", "offline_access"], "signedIn": true,
                "server": { "issuer": "https://auth.example.com", "authorizationEndpoint": "https://auth.example.com/authorize", "tokenEndpoint": "https://auth.example.com/token", "issSupported": true },
                "registration": { "issuer": "https://auth.example.com", "clientId": "yap-123", "redirectUris": ["http://127.0.0.1:51439/connectors/callback"] },
                "account": "A workspace"
            }
        }]}))
        .unwrap();
        let a = &file.connectors[0].auth;
        assert_eq!(a.client, Some(ClientRef { kind: ClientKind::Dcr, client_id: "yap-123".into() }));
        assert!(a.signed_in && !a.expired && a.wants.is_empty());
        assert_eq!(a.registration.as_ref().unwrap().auth_method, "none", "public unless it said");
        assert!(a.server.as_ref().unwrap().iss_supported);
        let back = serde_json::to_value(&file).unwrap();
        assert_eq!(back["connectors"][0]["auth"]["account"], "A workspace", "a field this build doesn't know stays");
        assert!(back["connectors"][0]["auth"].get("expired").is_none(), "false fields left out");
        let text = back.to_string().to_ascii_lowercase();
        assert!(!text.contains("token\":") && !text.contains("refresh") && !text.contains("secret"), "{text}");
    }
}

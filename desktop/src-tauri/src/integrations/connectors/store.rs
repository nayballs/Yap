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

/// How a connector signs in. Wave 3 adds `oauth` and its client.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthKind {
    #[default]
    None,
    /// `Authorization: Bearer <token>` (the token in the vault).
    Bearer,
    /// Request headers ([`Transport::Remote`]'s `headers`, values in the vault).
    Header,
    /// Signed in through the browser (wave 3).
    Oauth,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Auth {
    #[serde(default)]
    pub kind: AuthKind,
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
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Connector {
    /// The receipts' and the logs' word for it: never the name the person
    /// typed. The catalogue's name for one from the catalogue (wave 5b).
    pub fn kind(&self) -> String {
        "A connector".into()
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
            auth: Auth { kind: new.auth, extra: Map::new() },
            where_: Where::default(),
            tools: BTreeMap::new(),
            added: now(),
            timeout_secs: None,
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
}

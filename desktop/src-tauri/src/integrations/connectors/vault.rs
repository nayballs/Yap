//! **Connectors' secrets** — a link's token and its secret headers, a
//! program's secret environment variables, a browser sign-in's tokens
//! ([`Grant`]) and a client's secret (a registration's, or the person's own
//! client's) — as one JSON value per connector, never in connectors.json,
//! connector_tools.json, the logs or the webview.
//!
//! Windows Credential Manager, as the calendars' and the ChatGPT sign-in's
//! (`calendar/vault.rs`, `ai/chatgpt/vault.rs`): generic credentials named
//! `yap-connector-c<N>-<part>.com.yap.dictation` (Local persistence), the
//! value split in [`PART_CHARS`]-character parts across `-1`…`-8`
//! ([`PARTS`]), `c1`…`c24` (`store::MAX`), so the uninstaller's "Delete the
//! application data" knows every name. A portable Yap adds a hash of its
//! data folder to the names. Test runs (`e2e::active`) keep them in a file
//! in their own data folder; unit tests in memory, and the one test of the
//! real store uses names of its own ([`Vault::os`]) and removes them.
//!
//! [`Secrets`] never prints its values (`Debug` says which there are).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Credentials one connector's secrets may span: room for a sign-in's two
/// tokens (JWTs of 2–3,000 characters each are common) and a client secret.
pub const PARTS: usize = 8;
/// Credential Manager blobs top out at 2,560 bytes of UTF-16.
pub const PART_CHARS: usize = 1_100;
/// Yap's own credentials' service (`<user>.com.yap.dictation`).
#[cfg_attr(test, allow(dead_code))]
pub const SERVICE: &str = "com.yap.dictation";

/// One connector's secrets.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Secrets {
    /// A bearer token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// Request headers' values, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    /// A program's secret environment variables, by name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// A browser sign-in's tokens (`oauth.rs`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant: Option<Grant>,
    /// The secret of the client Yap registered with the server (dynamic
    /// registration), when it was given one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registered_secret: Option<String>,
    /// The secret of the person's own client, entered with its id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub own_secret: Option<String>,
}

/// A browser sign-in's tokens, with what they're bound to: the
/// authorization server that issued them, the resource they're for and the
/// client they were issued to. Only ever sent to that resource (the access
/// token) and that server (the refresh token).
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Grant {
    pub access: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh: Option<String>,
    /// When the access token stops working (Unix seconds); 0: the server
    /// didn't say (used until it's refused).
    #[serde(default)]
    pub expires: i64,
    /// When it was issued (Unix seconds).
    #[serde(default)]
    pub received: i64,
    #[serde(default)]
    pub scopes: Vec<String>,
    pub issuer: String,
    pub resource: String,
    pub client_id: String,
}

impl std::fmt::Debug for Grant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Grant")
            .field("access", &"…")
            .field("refresh", &self.refresh.as_ref().map(|_| "…"))
            .field("expires", &self.expires)
            .field("scopes", &self.scopes)
            .field("issuer", &self.issuer)
            .finish()
    }
}

impl std::fmt::Debug for Secrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Secrets")
            .field("token", &self.token.as_ref().map(|_| "…"))
            .field("headers", &self.headers.keys().collect::<Vec<_>>())
            .field("env", &self.env.keys().collect::<Vec<_>>())
            .field("grant", &self.grant)
            .field("registered_secret", &self.registered_secret.as_ref().map(|_| "…"))
            .field("own_secret", &self.own_secret.as_ref().map(|_| "…"))
            .finish()
    }
}

impl Secrets {
    pub fn is_empty(&self) -> bool {
        self.token.is_none()
            && self.headers.is_empty()
            && self.env.is_empty()
            && self.grant.is_none()
            && self.registered_secret.is_none()
            && self.own_secret.is_none()
    }
}

/// Where secrets are kept.
trait Backend: Send + Sync {
    fn put(&self, user: &str, secret: &str) -> Result<(), String>;
    fn get(&self, user: &str) -> Option<String>;
    fn remove(&self, user: &str);
}

/// A vault: where, and its names' prefix.
pub struct Vault {
    backend: Box<dyn Backend>,
    prefix: String,
}

impl Vault {
    /// This run's: Credential Manager, a test run's file, or a unit test's
    /// memory.
    pub fn this_run() -> Vault {
        #[cfg(test)]
        {
            Vault { backend: Box::new(memory::Memory), prefix: "yap-connector".into() }
        }
        #[cfg(not(test))]
        {
            if crate::shell::e2e::active() {
                return Vault { backend: Box::new(file::File::in_data()), prefix: "yap-connector".into() };
            }
            Vault::os(SERVICE, &prefix())
        }
    }

    /// Credential Manager under `service`, names starting `prefix` (the one
    /// test of the real store passes names of its own).
    pub fn os(service: &str, prefix: &str) -> Vault {
        Vault { backend: Box::new(os::Os { service: service.to_string() }), prefix: prefix.to_string() }
    }

    /// The credential's user name for connector `id`'s part `i` (0-based).
    pub fn user(&self, id: &str, i: usize) -> String {
        format!("{}-{id}-{}", self.prefix, i + 1)
    }

    /// Keep `secrets` for connector `id` (replacing what was there, whole;
    /// nothing to keep removes it).
    pub fn save(&self, id: &str, secrets: &Secrets) -> Result<(), String> {
        check_id(id)?;
        if secrets.is_empty() {
            self.delete(id);
            return Ok(());
        }
        let blob = serde_json::to_string(secrets).map_err(|_| "Yap couldn't keep that sign-in.".to_string())?;
        let chars: Vec<char> = blob.chars().collect();
        let parts: Vec<String> = chars.chunks(PART_CHARS).map(|c| c.iter().collect()).collect();
        if parts.len() > PARTS {
            return Err("That's too long for Windows to store safely.".into());
        }
        for (i, part) in parts.iter().enumerate() {
            self.backend.put(&self.user(id, i), part)?;
        }
        for i in parts.len()..PARTS {
            self.backend.remove(&self.user(id, i));
        }
        Ok(())
    }

    /// Connector `id`'s secrets (none: empty).
    pub fn load(&self, id: &str) -> Secrets {
        if check_id(id).is_err() {
            return Secrets::default();
        }
        let mut blob = String::new();
        for i in 0..PARTS {
            match self.backend.get(&self.user(id, i)) {
                Some(part) => blob.push_str(&part),
                None => break,
            }
        }
        if blob.is_empty() {
            return Secrets::default();
        }
        serde_json::from_str(&blob).unwrap_or_else(|_| {
            tracing::warn!(connector = id, "connectors: a connector's secrets didn't read; treated as none");
            Secrets::default()
        })
    }

    /// Change connector `id`'s secrets: read, `f`, saved whole.
    pub fn update<R>(&self, id: &str, f: impl FnOnce(&mut Secrets) -> R) -> Result<R, String> {
        let mut s = self.load(id);
        let out = f(&mut s);
        self.save(id, &s)?;
        Ok(out)
    }

    /// Forget connector `id`'s secrets (no-op if there are none).
    pub fn delete(&self, id: &str) {
        if check_id(id).is_err() {
            return;
        }
        for i in 0..PARTS {
            self.backend.remove(&self.user(id, i));
        }
    }
}

/// Only `c1`…`c24`: the names the uninstaller knows.
fn check_id(id: &str) -> Result<(), String> {
    let n: usize = id.strip_prefix('c').and_then(|n| n.parse().ok()).unwrap_or(0);
    if (1..=super::store::MAX).contains(&n) && id == format!("c{n}") {
        Ok(())
    } else {
        Err("There's no such connector.".into())
    }
}

/// The names' prefix: `yap-connector`, or with a portable Yap's data folder's
/// hash (`yap-connector-1a2b3c4d`), so it never shares an installed Yap's.
#[cfg(not(test))]
fn prefix() -> String {
    if crate::shell::portable::is_portable() {
        use sha2::{Digest, Sha256};
        let dir = crate::config::data_dir().to_string_lossy().to_lowercase();
        let hash = Sha256::digest(dir.as_bytes());
        let short: String = hash.iter().take(4).map(|b| format!("{b:02x}")).collect();
        format!("yap-connector-{short}")
    } else {
        "yap-connector".into()
    }
}

/// Connector `id`'s secrets in this run's vault.
pub fn load(id: &str) -> Secrets {
    Vault::this_run().load(id)
}

pub fn save(id: &str, secrets: &Secrets) -> Result<(), String> {
    Vault::this_run().save(id, secrets)
}

pub fn delete(id: &str) {
    Vault::this_run().delete(id)
}

pub fn update<R>(id: &str, f: impl FnOnce(&mut Secrets) -> R) -> Result<R, String> {
    Vault::this_run().update(id, f)
}

#[cfg(windows)]
mod os {
    use std::collections::HashMap;
    use std::sync::Once;

    pub struct Os {
        pub service: String,
    }

    fn entry(service: &str, user: &str) -> Result<keyring_core::Entry, keyring_core::Error> {
        static STORE: Once = Once::new();
        STORE.call_once(|| {
            // auth.rs, the calendar or the ChatGPT sign-in may have set it up.
            if keyring_core::get_default_store().is_none() {
                match windows_native_keyring_store::Store::new() {
                    Ok(store) => keyring_core::set_default_store(store),
                    Err(e) => tracing::error!("connectors: credential store unavailable: {e}"),
                }
            }
        });
        keyring_core::Entry::new_with_modifiers(service, user, &HashMap::from([("persistence", "Local")]))
    }

    impl super::Backend for Os {
        fn put(&self, user: &str, secret: &str) -> Result<(), String> {
            entry(&self.service, user).and_then(|e| e.set_password(secret)).map_err(|e| {
                tracing::error!("connectors: couldn't store a connector's secret: {e}");
                "Windows Credential Manager wouldn't keep the connector's sign-in.".to_string()
            })
        }

        fn get(&self, user: &str) -> Option<String> {
            match entry(&self.service, user).and_then(|e| e.get_password()) {
                Ok(secret) => Some(secret),
                Err(keyring_core::Error::NoEntry) => None,
                Err(e) => {
                    tracing::error!("connectors: couldn't read a connector's secret: {e}");
                    None
                }
            }
        }

        fn remove(&self, user: &str) {
            match entry(&self.service, user).and_then(|e| e.delete_credential()) {
                Ok(()) | Err(keyring_core::Error::NoEntry) => {}
                Err(e) => tracing::error!("connectors: couldn't remove a connector's secret: {e}"),
            }
        }
    }
}

/// No credential store wired up off Windows: secrets last one run.
#[cfg(not(windows))]
mod os {
    use std::collections::HashMap;
    use std::sync::Mutex;

    pub struct Os {
        pub service: String,
    }

    static SECRETS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

    impl super::Backend for Os {
        fn put(&self, user: &str, secret: &str) -> Result<(), String> {
            SECRETS.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(HashMap::new).insert(format!("{user}.{}", self.service), secret.into());
            Ok(())
        }
        fn get(&self, user: &str) -> Option<String> {
            SECRETS.lock().unwrap_or_else(|p| p.into_inner()).as_ref()?.get(&format!("{user}.{}", self.service)).cloned()
        }
        fn remove(&self, user: &str) {
            if let Some(map) = SECRETS.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
                map.remove(&format!("{user}.{}", self.service));
            }
        }
    }
}

/// Test runs: a JSON file in the instance's own (portable) data folder.
#[cfg(not(test))]
mod file {
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::sync::Mutex;

    pub struct File {
        path: PathBuf,
    }

    /// One write at a time.
    static LOCK: Mutex<()> = Mutex::new(());

    impl File {
        pub fn in_data() -> File {
            File { path: crate::config::data_dir().join("connector-secrets.e2e.json") }
        }

        fn read(&self) -> BTreeMap<String, String> {
            std::fs::read_to_string(&self.path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
        }

        fn write(&self, map: &BTreeMap<String, String>) -> Result<(), String> {
            let json = serde_json::to_string_pretty(map).map_err(|e| e.to_string())?;
            crate::config::atomic_write(&self.path, &json).map_err(|e| e.to_string())
        }
    }

    impl super::Backend for File {
        fn put(&self, user: &str, secret: &str) -> Result<(), String> {
            let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
            let mut map = self.read();
            map.insert(user.to_string(), secret.to_string());
            self.write(&map)
        }
        fn get(&self, user: &str) -> Option<String> {
            let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
            self.read().get(user).cloned()
        }
        fn remove(&self, user: &str) {
            let _g = LOCK.lock().unwrap_or_else(|p| p.into_inner());
            let mut map = self.read();
            if map.remove(user).is_some() {
                let _ = self.write(&map);
            }
        }
    }
}

/// Unit tests: in memory, per test thread.
#[cfg(test)]
mod memory {
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    thread_local! {
        static MAP: RefCell<BTreeMap<String, String>> = const { RefCell::new(BTreeMap::new()) };
    }

    pub struct Memory;

    impl super::Backend for Memory {
        fn put(&self, user: &str, secret: &str) -> Result<(), String> {
            MAP.with(|m| m.borrow_mut().insert(user.into(), secret.into()));
            Ok(())
        }
        fn get(&self, user: &str) -> Option<String> {
            MAP.with(|m| m.borrow().get(user).cloned())
        }
        fn remove(&self, user: &str) {
            MAP.with(|m| m.borrow_mut().remove(user));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn some() -> Secrets {
        Secrets {
            token: Some("ghp_FAKE_0123456789".into()),
            headers: BTreeMap::from([("X-Api-Key".into(), "key-FAKE".into())]),
            env: BTreeMap::from([("GITHUB_TOKEN".into(), "ghp_FAKE_env".into())]),
            grant: Some(Grant {
                access: "at-FAKE-1".into(),
                refresh: Some("rt-FAKE-1".into()),
                expires: 4_000,
                received: 400,
                scopes: vec!["read".into()],
                issuer: "https://auth.example.com".into(),
                resource: "https://mcp.example.com/mcp".into(),
                client_id: "yap-FAKE-client".into(),
            }),
            registered_secret: Some("cs-FAKE-dcr".into()),
            own_secret: Some("cs-FAKE-own".into()),
        }
    }

    #[test]
    fn secrets_round_trip_and_a_long_one_spans_parts_then_shrinks_back() {
        let v = Vault::this_run();
        assert!(v.load("c3").is_empty());
        v.save("c3", &some()).unwrap();
        assert_eq!(v.load("c3"), some());
        let long = Secrets { token: Some("t".repeat(PART_CHARS * 3)), ..Default::default() };
        v.save("c3", &long).unwrap();
        assert_eq!(v.load("c3"), long);
        assert!(v.backend.get(&v.user("c3", 3)).is_some(), "four parts");
        v.save("c3", &some()).unwrap();
        assert!(v.backend.get(&v.user("c3", 1)).is_none(), "the parts it no longer needs are gone");
        assert_eq!(v.load("c3"), some());
        let too_long = Secrets { token: Some("t".repeat(PART_CHARS * PARTS)), ..Default::default() };
        assert!(v.save("c3", &too_long).is_err());
        v.save("c3", &Secrets::default()).unwrap();
        assert!(v.load("c3").is_empty());
    }

    #[test]
    fn only_the_names_the_uninstaller_knows_and_debug_never_shows_a_secret() {
        let v = Vault::this_run();
        for bad in ["c0", "c25", "c03", "x1", "c", "../c1"] {
            assert!(v.save(bad, &some()).is_err(), "{bad}");
        }
        assert_eq!(v.user("c24", 3), "yap-connector-c24-4");
        let shown = format!("{:?}", some());
        assert!(!shown.contains("FAKE"), "{shown}");
        assert!(shown.contains("X-Api-Key") && shown.contains("GITHUB_TOKEN"));
        // The uninstaller's "Delete app data" loops go as far.
        let nsi = include_str!("../../../nsis/installer.nsi");
        let at = nsi.find("yap-connector-c$1-$2.com.yap.dictation").expect("the uninstaller's connector loop");
        let bounds: Vec<usize> = nsi[at..]
            .lines()
            .take(8)
            .filter_map(|l| l.trim().strip_prefix("${LoopWhile} $").and_then(|r| r.split("<=").nth(1)).and_then(|n| n.trim().parse().ok()))
            .collect();
        assert_eq!(bounds, [PARTS, super::super::store::MAX]);
    }

    /// The real Windows Credential Manager, under a service and names of
    /// this test's own (never Yap's), removed at the end.
    #[test]
    fn the_real_credential_store_keeps_and_forgets_them() {
        let prefix = format!("yap-connector-unittest-{}", std::process::id());
        let v = Vault::os("com.yap.test-connectors", &prefix);
        v.delete("c1");
        assert!(v.load("c1").is_empty());
        let long = Secrets { token: Some("x".repeat(PART_CHARS + 10)), ..some() };
        v.save("c1", &long).unwrap();
        assert_eq!(v.load("c1"), long);
        v.delete("c1");
        assert!(v.load("c1").is_empty());
        for i in 0..PARTS {
            assert!(v.backend.get(&v.user("c1", i)).is_none());
        }
    }
}

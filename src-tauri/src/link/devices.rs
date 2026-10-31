//! **Paired phones** (Yap Link), in `link_devices.json`: each phone's id,
//! name, when it was paired and last used, and the SHA-256 of its secret —
//! never the secret itself, which only the phone keeps. Settings → Phone
//! lists them with Remove; a removed phone's next hello is refused, and one
//! connected now is cut off at once (`link.rs`).
//!
//! Saved through `saving.rs` like every store (a failed save kept in memory
//! and said once); a file that doesn't read is set aside. Unit tests keep a
//! store per test thread.

#[cfg(not(test))]
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// One paired phone, as stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    /// SHA-256 (hex) of the phone's secret.
    pub secret_hash: String,
    /// Unix ms.
    pub paired: u64,
    /// Unix ms; 0 = never since pairing.
    #[serde(default)]
    pub last_seen: u64,
}

/// A paired phone, as Settings shows it (no hash).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listed {
    pub id: String,
    pub name: String,
    pub paired: u64,
    pub last_seen: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Store {
    #[serde(default)]
    devices: Vec<Device>,
}

/// link_devices.json's saves.
#[cfg_attr(test, allow(dead_code))]
pub static SAVER: crate::shell::saving::Saver =
    crate::shell::saving::Saver::new("link-devices", "your paired phones", save_now);

#[cfg_attr(test, allow(dead_code))]
fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    with_store(|s| save(s));
    SAVER.check()
}

/// The file's name in the data folder.
#[cfg_attr(test, allow(dead_code))]
pub const FILE: &str = "link_devices.json";

#[cfg(not(test))]
static STATE: Mutex<Option<Store>> = Mutex::new(None);

#[cfg(not(test))]
fn path() -> std::path::PathBuf {
    crate::config::data_dir().join(FILE)
}

#[cfg(not(test))]
fn save(store: &Store) {
    let _ = SAVER.write_json(&path(), store, true);
}

#[cfg(not(test))]
fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let store = guard.get_or_insert_with(|| SAVER.load(&path()));
    if SAVER.any_unread() && SAVER.refresh(&path(), store) {
        save(store);
    }
    f(store)
}

#[cfg(test)]
thread_local! {
    static TEST_STORE: std::cell::RefCell<Store> = std::cell::RefCell::new(Store::default());
}

#[cfg(test)]
fn save(_: &Store) {}

#[cfg(test)]
fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    TEST_STORE.with(|s| f(&mut s.borrow_mut()))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// At most this many phones; pairing one more forgets the one used longest ago.
pub const MAX: usize = 20;

/// **Pair a phone**: a new id and a 256-bit secret; only the secret's hash
/// is kept. Returns `(id, secret)` for the phone.
pub fn add(name: &str) -> Result<(String, String), String> {
    let id = format!("p-{}", crate::link::pairing::random_text(9)?);
    let secret = crate::link::pairing::random_text(32)?;
    let hash = crate::link::pairing::sha256_hex(&secret);
    let now = now_ms();
    with_store(|s| {
        if s.devices.len() >= MAX {
            if let Some(oldest) = s.devices.iter().enumerate().min_by_key(|(_, d)| d.last_seen.max(d.paired)).map(|(i, _)| i) {
                s.devices.remove(oldest);
            }
        }
        s.devices.push(Device { id: id.clone(), name: crate::link::pairing::clean_name(name), secret_hash: hash, paired: now, last_seen: now });
        save(s);
    });
    Ok((id, secret))
}

/// The stored hash of phone `id`'s secret (for `pairing::judge`).
pub fn secret_hash(id: &str) -> Option<String> {
    with_store(|s| s.devices.iter().find(|d| d.id == id).map(|d| d.secret_hash.clone()))
}

/// Phone `id`'s name.
#[cfg(test)]
pub fn name(id: &str) -> Option<String> {
    with_store(|s| s.devices.iter().find(|d| d.id == id).map(|d| d.name.clone()))
}

/// Phone `id` connected now: its last use, and its name (it may have a new one).
pub fn seen(id: &str, name: &str) {
    let now = now_ms();
    with_store(|s| {
        if let Some(d) = s.devices.iter_mut().find(|d| d.id == id) {
            d.last_seen = now;
            let n = crate::link::pairing::clean_name(name);
            if !name.trim().is_empty() && d.name != n {
                d.name = n;
            }
            save(s);
        }
    });
}

/// Forget phone `id`. `true` if it was paired.
pub fn remove(id: &str) -> bool {
    with_store(|s| {
        let before = s.devices.len();
        s.devices.retain(|d| d.id != id);
        let gone = s.devices.len() != before;
        if gone {
            save(s);
        }
        gone
    })
}

/// Every paired phone, the one used last first.
pub fn list() -> Vec<Listed> {
    let mut out: Vec<Listed> = with_store(|s| {
        s.devices.iter().map(|d| Listed { id: d.id.clone(), name: d.name.clone(), paired: d.paired, last_seen: d.last_seen }).collect()
    });
    out.sort_by_key(|d| std::cmp::Reverse(d.last_seen.max(d.paired)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::link::pairing::{judge, Hello, Verdict};

    #[test]
    fn a_paired_phone_is_kept_by_its_secrets_hash_and_removed_at_once() {
        let (id, secret) = add("  Sam's Pixel ").unwrap();
        assert!(id.starts_with("p-"));
        assert_eq!(secret.len(), 43);
        let listed = list();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "Sam's Pixel");
        // Only the hash is kept.
        let hash = secret_hash(&id).unwrap();
        assert_ne!(hash, secret);
        assert_eq!(hash, crate::link::pairing::sha256_hex(&secret));
        let stored = with_store(|s| serde_json::to_string(s).unwrap());
        assert!(!stored.contains(&secret), "the secret is never stored");
        // Its hello is known; a wrong secret isn't.
        let now = std::time::Instant::now();
        let ok = Hello { v: 1, nonce: "n", device: Some((&id, &secret)), token: None };
        assert_eq!(judge(&ok, None, &secret_hash, now), Verdict::Known(id.clone()));
        let bad = Hello { v: 1, nonce: "n", device: Some((&id, "nope")), token: None };
        assert!(matches!(judge(&bad, None, &secret_hash, now), Verdict::Denied(_)));
        // Seen again with a new name.
        seen(&id, "Sam's phone");
        assert_eq!(name(&id).as_deref(), Some("Sam's phone"));
        // Removed: its hello is refused from now on.
        assert!(remove(&id));
        assert!(!remove(&id));
        assert!(list().is_empty());
        assert!(matches!(judge(&ok, None, &secret_hash, now), Verdict::Denied(crate::link::pairing::REMOVED)));
    }

    #[test]
    fn pairing_more_than_the_most_forgets_the_one_used_longest_ago() {
        let mut ids = Vec::new();
        for i in 0..MAX {
            ids.push(add(&format!("Phone {i}")).unwrap().0);
        }
        with_store(|s| s.devices[3].last_seen = 1);
        with_store(|s| s.devices[3].paired = 1);
        let (_, _) = add("One more").unwrap();
        let l = list();
        assert_eq!(l.len(), MAX);
        assert!(!l.iter().any(|d| d.id == ids[3]), "the one used longest ago went");
        assert!(l.iter().any(|d| d.name == "One more"));
    }
}

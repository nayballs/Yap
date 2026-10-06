//! Calendar secrets: a Google refresh token, or a private iCal address
//! (which works like a password: anyone with it can read the calendar).
//! Never in config.json, calendar.json, the logs or the webview.
//!
//! They live in Windows Credential Manager, one generic credential per
//! connection slot, named `yap-calendar-<slot>.com.yap.dictation` (Local
//! persistence, like the account session in `auth.rs`); the uninstaller's
//! "Delete the application data" removes slots 1–[`SLOTS`]. A portable Yap
//! adds a hash of its data folder to the name, so it never shares slots with
//! an installed one. Test runs (`e2e::active`) keep them in a file in their
//! own data folder instead and never touch the real credential store.

/// Connection slots, so the uninstaller knows every name.
pub const SLOTS: u8 = 8;

/// Credential Manager blobs top out at 2,560 bytes of UTF-16.
const MAX_CHARS: usize = 1_200;

/// Keep `secret` for connection `slot`.
pub fn save(slot: &str, secret: &str) -> Result<(), String> {
    if secret.chars().count() > MAX_CHARS {
        return Err("That's too long for Windows to store safely.".into());
    }
    if test_store::active() {
        return test_store::save(slot, secret);
    }
    os::save(slot, secret)
}

/// The secret for `slot`, if there is one.
pub fn load(slot: &str) -> Option<String> {
    if test_store::active() {
        return test_store::load(slot);
    }
    os::load(slot)
}

/// Forget the secret for `slot` (no-op if there's none).
pub fn delete(slot: &str) {
    if test_store::active() {
        test_store::delete(slot);
        return;
    }
    os::delete(slot);
}

/// The credential's user name for `slot`.
#[cfg_attr(not(windows), allow(dead_code))]
fn user(slot: &str) -> String {
    match crate::portable::is_portable() {
        true => {
            use sha2::{Digest, Sha256};
            let dir = crate::config::data_dir().to_string_lossy().to_lowercase();
            let hash = Sha256::digest(dir.as_bytes());
            let short: String = hash.iter().take(4).map(|b| format!("{b:02x}")).collect();
            format!("yap-calendar-{short}-{slot}")
        }
        false => format!("yap-calendar-{slot}"),
    }
}

#[cfg(windows)]
mod os {
    use std::collections::HashMap;
    use std::sync::Once;

    const SERVICE: &str = "com.yap.dictation";

    fn entry(slot: &str) -> Result<keyring_core::Entry, keyring_core::Error> {
        static STORE: Once = Once::new();
        STORE.call_once(|| {
            // auth.rs may have set it up already (the same store).
            if keyring_core::get_default_store().is_none() {
                match windows_native_keyring_store::Store::new() {
                    Ok(store) => keyring_core::set_default_store(store),
                    Err(e) => tracing::error!("calendar: credential store unavailable: {e}"),
                }
            }
        });
        // Local: it belongs to this PC (the default would roam it).
        keyring_core::Entry::new_with_modifiers(SERVICE, &super::user(slot), &HashMap::from([("persistence", "Local")]))
    }

    pub fn save(slot: &str, secret: &str) -> Result<(), String> {
        entry(slot).and_then(|e| e.set_password(secret)).map_err(|e| {
            tracing::error!("calendar: couldn't store a calendar secret: {e}");
            "Windows Credential Manager wouldn't keep the calendar's key.".to_string()
        })
    }

    pub fn load(slot: &str) -> Option<String> {
        match entry(slot).and_then(|e| e.get_password()) {
            Ok(secret) => Some(secret),
            Err(keyring_core::Error::NoEntry) => None,
            Err(e) => {
                tracing::error!("calendar: couldn't read a calendar secret: {e}");
                None
            }
        }
    }

    pub fn delete(slot: &str) {
        match entry(slot).and_then(|e| e.delete_credential()) {
            Ok(()) | Err(keyring_core::Error::NoEntry) => {}
            Err(e) => tracing::error!("calendar: couldn't remove a calendar secret: {e}"),
        }
    }
}

/// No credential store wired up off Windows: secrets last one run.
#[cfg(not(windows))]
mod os {
    use std::collections::HashMap;
    use std::sync::Mutex;

    static SECRETS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

    pub fn save(slot: &str, secret: &str) -> Result<(), String> {
        SECRETS.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(HashMap::new).insert(slot.into(), secret.into());
        Ok(())
    }
    pub fn load(slot: &str) -> Option<String> {
        SECRETS.lock().unwrap_or_else(|p| p.into_inner()).as_ref()?.get(slot).cloned()
    }
    pub fn delete(slot: &str) {
        if let Some(map) = SECRETS.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
            map.remove(slot);
        }
    }
}

/// Test runs: a JSON file in the instance's own (portable) data folder.
mod test_store {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    pub fn active() -> bool {
        crate::e2e::active()
    }

    fn path() -> PathBuf {
        crate::config::data_dir().join("calendar-secrets.e2e.json")
    }

    fn read() -> BTreeMap<String, String> {
        std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    fn write(map: &BTreeMap<String, String>) -> Result<(), String> {
        let json = serde_json::to_string_pretty(map).map_err(|e| e.to_string())?;
        crate::config::atomic_write(&path(), &json).map_err(|e| e.to_string())
    }

    pub fn save(slot: &str, secret: &str) -> Result<(), String> {
        let mut map = read();
        map.insert(slot.to_string(), secret.to_string());
        write(&map)
    }

    pub fn load(slot: &str) -> Option<String> {
        read().get(slot).cloned()
    }

    pub fn delete(slot: &str) {
        let mut map = read();
        if map.remove(slot).is_some() {
            let _ = write(&map);
        }
    }
}

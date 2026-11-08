//! Calendar secrets: a Google refresh token, or a private iCal address
//! (which works like a password: anyone with it can read the calendar).
//! Never in config.json, calendar.json, the logs or the webview.
//!
//! They live in Windows Credential Manager, one generic credential per
//! connection slot, named `yap-calendar-<slot>.com.yap.dictation` (Local
//! persistence, like the account session in `auth.rs`; written one at a time
//! across Yaps and read back, `shell/credentials.rs`); the uninstaller's
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
fn user(slot: &str) -> String {
    match crate::shell::portable::is_portable() {
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

/// Windows Credential Manager (in memory for one run off Windows).
mod os {
    use crate::shell::credentials;

    const SERVICE: &str = "com.yap.dictation";

    pub fn save(slot: &str, secret: &str) -> Result<(), String> {
        credentials::write(SERVICE, &super::user(slot), secret).map_err(|e| {
            tracing::error!("calendar: couldn't store a calendar secret: {e}");
            "Windows Credential Manager wouldn't keep the calendar's key.".to_string()
        })
    }

    pub fn load(slot: &str) -> Option<String> {
        credentials::read(SERVICE, &super::user(slot)).unwrap_or_else(|e| {
            tracing::error!("calendar: couldn't read a calendar secret: {e}");
            None
        })
    }

    pub fn delete(slot: &str) {
        if let Err(e) = credentials::delete(SERVICE, &super::user(slot)) {
            tracing::error!("calendar: couldn't remove a calendar secret: {e}");
        }
    }
}

/// Test runs: a JSON file in the instance's own (portable) data folder.
mod test_store {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    pub fn active() -> bool {
        crate::shell::e2e::active()
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

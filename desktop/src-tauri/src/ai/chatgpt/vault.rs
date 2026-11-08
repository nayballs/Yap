//! The ChatGPT sign-in's secret: its refresh token (each refresh replaces
//! it) with the access token and its expiry (an hour), as one JSON value
//! (`chatgpt.rs` `Creds`: other Yaps use the same tokens), never in
//! config.json, chatgpt.json, the logs or the webview.
//!
//! Windows Credential Manager, as the calendars' secrets
//! (`calendar/vault.rs`): generic credentials named
//! `yap-chatgpt[-2…6].com.yap.dictation` (Local persistence); a token longer
//! than one credential holds is split across the next names. A portable
//! Yap adds a hash of its data folder to the names (a model evaluation's
//! run folder doesn't: it uses the maintainer's own sign-in, as it uses
//! their keys). The uninstaller's "Delete the application data" removes
//! all [`PARTS`]. Test runs (`e2e::active`) keep it in a file in their own
//! data folder.
//!
//! Its parts are written, read and removed whole within one turn at
//! Credential Manager (`shell/credentials.rs`: one at a time across Yaps,
//! each write read back), so a refresh in another Yap never leaves a read
//! with half of one token and half of the next.

/// Credentials one token may span (the uninstaller knows every name).
pub const PARTS: usize = 6;
/// Credential Manager blobs top out at 2,560 bytes of UTF-16.
const PART_CHARS: usize = 1_100;

/// Keep `secret` (replacing the one before, whole).
pub fn save(secret: &str) -> Result<(), String> {
    let chars: Vec<char> = secret.chars().collect();
    let parts: Vec<String> = chars.chunks(PART_CHARS).map(|c| c.iter().collect()).collect();
    if parts.len() > PARTS {
        return Err("ChatGPT's sign-in is too long for Windows to store safely.".into());
    }
    let _turn = turn();
    for (i, part) in parts.iter().enumerate() {
        put(i, part)?;
    }
    for i in parts.len()..PARTS {
        // A part left over from a longer token would join this one's.
        remove(i).map_err(|()| "Windows Credential Manager wouldn't keep ChatGPT's sign-in.".to_string())?;
    }
    Ok(())
}

/// The secret, if there is one.
pub fn load() -> Option<String> {
    let _turn = turn();
    let mut out = String::new();
    for i in 0..PARTS {
        match get(i) {
            Some(part) => out.push_str(&part),
            None => break,
        }
    }
    Some(out).filter(|s| !s.is_empty())
}

/// Forget it (no-op if there's none).
pub fn delete() {
    let _turn = turn();
    for i in 0..PARTS {
        let _ = remove(i);
    }
}

/// A turn at Credential Manager for a whole read or write (none for a test
/// run's file).
fn turn() -> Option<crate::shell::credentials::Turn> {
    (!test_store::active()).then(crate::shell::credentials::turn)
}

fn put(i: usize, part: &str) -> Result<(), String> {
    if test_store::active() {
        return test_store::save(i, part);
    }
    os::save(&user(i), part)
}

fn get(i: usize) -> Option<String> {
    if test_store::active() {
        return test_store::load(i);
    }
    os::load(&user(i))
}

fn remove(i: usize) -> Result<(), ()> {
    if test_store::active() {
        test_store::delete(i);
        Ok(())
    } else {
        os::delete(&user(i))
    }
}

/// The credential's user name for part `i`.
pub(super) fn user(i: usize) -> String {
    let n = if i == 0 { String::new() } else { format!("-{}", i + 1) };
    match crate::shell::portable::is_portable() && !crate::ai::eval::active() {
        true => {
            use sha2::{Digest, Sha256};
            let dir = crate::config::data_dir().to_string_lossy().to_lowercase();
            let hash = Sha256::digest(dir.as_bytes());
            let short: String = hash.iter().take(4).map(|b| format!("{b:02x}")).collect();
            format!("yap-chatgpt-{short}{n}")
        }
        false => format!("yap-chatgpt{n}"),
    }
}

/// Windows Credential Manager (in memory for one run off Windows).
mod os {
    use crate::shell::credentials;

    const SERVICE: &str = "com.yap.dictation";

    pub fn save(user: &str, secret: &str) -> Result<(), String> {
        credentials::write(SERVICE, user, secret).map_err(|e| {
            tracing::error!("chatgpt: couldn't store the sign-in: {e}");
            "Windows Credential Manager wouldn't keep ChatGPT's sign-in.".to_string()
        })
    }

    pub fn load(user: &str) -> Option<String> {
        credentials::read(SERVICE, user).unwrap_or_else(|e| {
            tracing::error!("chatgpt: couldn't read the sign-in: {e}");
            None
        })
    }

    pub fn delete(user: &str) -> Result<(), ()> {
        credentials::delete(SERVICE, user).map_err(|e| tracing::error!("chatgpt: couldn't remove the sign-in: {e}"))
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
        crate::config::data_dir().join("chatgpt-secret.e2e.json")
    }

    fn read() -> BTreeMap<usize, String> {
        std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    fn write(map: &BTreeMap<usize, String>) -> Result<(), String> {
        let json = serde_json::to_string_pretty(map).map_err(|e| e.to_string())?;
        crate::config::atomic_write(&path(), &json).map_err(|e| e.to_string())
    }

    pub fn save(i: usize, part: &str) -> Result<(), String> {
        let mut map = read();
        map.insert(i, part.to_string());
        write(&map)
    }

    pub fn load(i: usize) -> Option<String> {
        read().get(&i).cloned()
    }

    pub fn delete(i: usize) {
        let mut map = read();
        if map.remove(&i).is_some() {
            let _ = write(&map);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_credentials_names_are_the_ones_the_uninstaller_removes() {
        if crate::shell::portable::is_portable() {
            return;
        }
        assert_eq!((0..PARTS).map(user).collect::<Vec<_>>(), ["yap-chatgpt", "yap-chatgpt-2", "yap-chatgpt-3", "yap-chatgpt-4", "yap-chatgpt-5", "yap-chatgpt-6"]);
        // …and the uninstaller's "Delete app data" loop goes as far (it
        // stopped at -4 until 2026-11-03, leaving -5 and -6 behind).
        let nsi = include_str!("../../../nsis/installer.nsi");
        let at = nsi.find("yap-chatgpt-$1.com.yap.dictation").expect("the uninstaller's ChatGPT loop");
        let bound = nsi[at..].lines().find_map(|l| l.trim().strip_prefix("${LoopWhile} $1 <= ")).expect("its bound");
        assert_eq!(bound.trim().parse::<usize>(), Ok(PARTS));
    }
}

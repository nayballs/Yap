//! **One refresh at a time, across Yaps**: the installed Yap and Yap - Dev
//! share the data folder and the vault, and a server that rotates refresh
//! tokens signs you out when two spend the same one (replay detection). So a
//! connector's refresh (and its sign-out) holds [`acquire`]'s guard: one
//! per connector in this Yap (a mutex), then a lock file beside the data
//! (`<data>/connectors/c<N>.refresh.lock`, the ChatGPT sign-in's way:
//! `local_llm::runtime::UpgradeLock`, a lock a gone Yap left taken back),
//! waited for up to 15 s. Whoever takes it reads the vault again first:
//! another Yap may have refreshed while this one waited, and its tokens are
//! used as they are (`oauth::access_token`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Held while one refresh (or a sign-out) runs.
pub struct Guard {
    _here: tokio::sync::OwnedMutexGuard<()>,
    #[cfg(not(test))]
    _across: crate::ai::local_llm::runtime::UpgradeLock,
}

static HERE: Mutex<Option<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = Mutex::new(None);

/// This Yap's mutex for connector `id` (in a unit test, the test thread's:
/// each has its own connectors).
fn here(id: &str) -> Arc<tokio::sync::Mutex<()>> {
    #[cfg(test)]
    let key = format!("{:?}/{id}", std::thread::current().id());
    #[cfg(not(test))]
    let key = id.to_string();
    HERE.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(HashMap::new).entry(key).or_default().clone()
}

/// Connector `id`'s refresh guard ([module docs](self)). `Err`: another Yap
/// held it for 15 s.
pub async fn acquire(id: &str) -> Result<Guard, String> {
    let held = here(id).lock_owned().await;
    #[cfg(not(test))]
    {
        let dir = crate::config::data_dir().join("connectors");
        let name = format!("{id}.refresh.lock");
        for _ in 0..150 {
            if let Some(lock) = crate::ai::local_llm::runtime::UpgradeLock::take_named(&dir, &name) {
                return Ok(Guard { _here: held, _across: lock });
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err("Another Yap is refreshing this connector's sign-in. Try again in a moment.".into())
    }
    #[cfg(test)]
    {
        let _ = Duration::ZERO;
        Ok(Guard { _here: held })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn one_refresh_at_a_time_per_connector() {
        let first = acquire("c1").await.unwrap();
        // Another connector isn't held up.
        let other = tokio::time::timeout(Duration::from_millis(200), acquire("c2")).await;
        assert!(other.is_ok());
        // The same one waits until the first is let go.
        let waiting = tokio::spawn(async { acquire("c1").await.map(|_| ()) });
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!waiting.is_finished());
        drop(first);
        tokio::time::timeout(Duration::from_secs(2), waiting).await.unwrap().unwrap().unwrap();
    }
}

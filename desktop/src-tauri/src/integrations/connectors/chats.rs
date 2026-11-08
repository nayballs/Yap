//! **Each chat's own connectors** (`connector_chats.json`, Connectors wave
//! 6): the composer's + menu → Connectors ▸ switches a connector off (or on
//! again) for one conversation, as Claude's composer does. A chat has every
//! enabled connector on unless it says otherwise, so the file keeps only
//! what a chat switched **off**, by conversation id: a connector added later
//! is on in every chat, and a chat nobody touched has no entry at all.
//!
//! - **A new chat** (no conversation yet): the page holds its switches and
//!   sends them with its first question (`chat_send`'s `connectorsOff`);
//!   the turn runs inside [`with_new`], so [`off`] of `None` is that set, and
//!   the conversation the question makes keeps it ([`adopt_new`]: written,
//!   or a stale entry under a reused id taken out).
//! - **Never in chats.json** (an older Yap's save of it would drop a field),
//!   never a connector's name: ids only (`c1`…`c24`).
//! - **Deleted with the chat** ([`chat_deleted`], `chat_delete` and Delete
//!   some of your data's chats), and whole by Reset app ([`reset`]). A
//!   deletion takes the entry out even with the preview off, so a later
//!   conversation given that id starts with every connector on.
//!
//! Read only while the preview is on (`connectors::enabled`), except to
//! take a deleted chat's entry out. Saved through `saving.rs`. Unit tests
//! keep a store per test thread.

#[cfg(not(test))]
use std::sync::Mutex;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[cfg(not(test))]
const FILE: &str = "connector_chats.json";

/// One chat's switches: the connectors it has off.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Chat {
    #[serde(default)]
    pub off: BTreeSet<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// What connector_chats.json holds: each chat's, by its conversation id.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct File {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub chats: BTreeMap<String, Chat>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// connector_chats.json's saves.
pub static SAVER: crate::shell::saving::Saver =
    crate::shell::saving::Saver::new("connector-chats", "your chats' connectors", save_now);

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
    // Nothing switched off anywhere: no file (Reset app's, or the last
    // chat's switches turned back on).
    if store.chats.is_empty() && store.extra.is_empty() {
        let _ = std::fs::remove_file(path());
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

/// The file is there to change (a deletion with the preview off reads it
/// only then).
#[cfg(not(test))]
fn exists() -> bool {
    STATE.lock().unwrap_or_else(|p| p.into_inner()).is_some() || path().exists()
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

#[cfg(test)]
fn exists() -> bool {
    TEST_STORE.with(|s| s.borrow().is_some())
}

/// Whether the store was read (or made) in this run (the switch-off test).
#[cfg(test)]
pub fn touched() -> bool {
    exists()
}

/// A test starts with no store.
#[cfg(test)]
pub fn reset_for_test() {
    TEST_STORE.with(|s| *s.borrow_mut() = None);
}

tokio::task_local! {
    /// The connectors a new chat's page switched off before its first
    /// question (`chat_send`'s `connectorsOff`); `None`: not a new chat.
    static NEW_CHAT: Option<BTreeSet<String>>;
}

/// `f` run as a new chat's first question (`off`: `Some`), with the
/// connectors its page switched off (empty: every one on); `None`: a
/// question to a conversation there is already.
pub async fn with_new<F: std::future::Future>(off: Option<BTreeSet<String>>, f: F) -> F::Output {
    NEW_CHAT.scope(off, f).await
}

/// The new chat's set, inside [`with_new`] for a new chat.
fn new_chat() -> Option<BTreeSet<String>> {
    NEW_CHAT.try_with(Clone::clone).ok().flatten()
}

/// **The connectors chat `conversation` has switched off** (`None`: a new
/// chat, [`with_new`]'s set). Nothing read while the preview is off.
pub fn off(conversation: Option<u64>) -> BTreeSet<String> {
    match conversation {
        None => new_chat().unwrap_or_default(),
        Some(_) if !super::enabled() => BTreeSet::new(),
        Some(id) => with_store(|s| s.chats.get(&id.to_string()).map(|c| c.off.clone()).unwrap_or_default()),
    }
}

/// Whether connector `id` is on in chat `conversation`.
pub fn is_on(conversation: Option<u64>, id: &str) -> bool {
    !off(conversation).contains(id)
}

/// **Switch connector `id` on or off in chat `conversation`.** Saved (an
/// entry that's empty again goes). `Err`: the save failed.
pub fn set(conversation: u64, id: &str, on: bool) -> Result<(), String> {
    with_store(|s| {
        let key = conversation.to_string();
        let chat = s.chats.entry(key.clone()).or_default();
        if on {
            chat.off.remove(id);
        } else {
            chat.off.insert(id.to_string());
        }
        if chat.off.is_empty() && chat.extra.is_empty() {
            s.chats.remove(&key);
        }
        save(s);
    });
    Ok(SAVER.check()?)
}

/// **The conversation a new chat's first question made** (`chat_send`'s
/// turn, as it's made): it keeps the switches its page sent ([`with_new`]),
/// and nothing a deleted chat with the same id left. Not while the preview
/// is off.
pub fn adopt_new(conversation: u64) {
    // (A question to a conversation there was already: nothing to adopt.)
    let Some(wanted) = new_chat() else { return };
    if !super::enabled() {
        return;
    }
    with_store(|s| {
        let key = conversation.to_string();
        let had = s.chats.remove(&key).is_some();
        if !wanted.is_empty() {
            s.chats.insert(key, Chat { off: wanted.clone(), extra: Map::new() });
        }
        if had || !wanted.is_empty() {
            save(s);
        }
    });
    if !wanted.is_empty() {
        tracing::info!(conversation, off = wanted.len(), "connectors: a new chat's switches kept");
    }
}

/// **A chat deleted**: its switches go (read only when there's a file).
pub fn chat_deleted(conversation: u64) {
    chats_deleted(&[conversation]);
}

/// Several chats deleted at once: one save.
pub fn chats_deleted(conversations: &[u64]) {
    if conversations.is_empty() || !exists() {
        return;
    }
    with_store(|s| {
        let before = s.chats.len();
        for id in conversations {
            s.chats.remove(&id.to_string());
        }
        if s.chats.len() != before {
            save(s);
        }
    });
}

/// A connector removed: no chat keeps its id (a later one may get it).
pub fn connector_removed(id: &str) {
    if !exists() {
        return;
    }
    with_store(|s| {
        let mut changed = false;
        s.chats.retain(|_, c| {
            changed |= c.off.remove(id);
            !c.off.is_empty() || !c.extra.is_empty()
        });
        if changed {
            save(s);
        }
    });
}

/// **Reset app / Delete some of your data's chats** (`privacy/reset.rs`):
/// every chat's switches go. How many chats had any.
pub fn reset() -> usize {
    if !exists() {
        return 0;
    }
    with_store(|s| {
        let n = s.chats.len();
        s.chats.clear();
        save(s);
        n
    })
}

/// A failed save, said (reset's).
pub fn saved() -> Result<(), String> {
    Ok(SAVER.check()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_chat_keeps_what_it_switched_off_and_a_new_chat_has_every_connector_on() {
        reset_for_test();
        super::super::switch_for_test(true);
        assert!(off(Some(7)).is_empty(), "nobody touched it: every connector on");
        set(7, "c2", false).unwrap();
        assert!(!is_on(Some(7), "c2") && is_on(Some(7), "c1"));
        assert!(is_on(Some(8), "c2"), "another chat keeps its own");
        // Back on: the entry goes.
        set(7, "c2", true).unwrap();
        assert!(with_store(|s| s.chats.is_empty()));
        // A new chat: nothing off unless its page said.
        assert!(off(None).is_empty());
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let wanted: BTreeSet<String> = ["c3".to_string()].into();
        assert_eq!(rt.block_on(with_new(Some(wanted.clone()), async { off(None) })), wanted);
        // The conversation it makes keeps them…
        rt.block_on(with_new(Some(wanted.clone()), async { adopt_new(12) }));
        assert_eq!(off(Some(12)), wanted);
        // A question to it later (no new chat): nothing changes.
        adopt_new(12);
        rt.block_on(with_new(None, async { adopt_new(12) }));
        assert_eq!(off(Some(12)), wanted);
        // …and a new one with nothing off clears what a deleted chat with its id left.
        rt.block_on(with_new(Some(BTreeSet::new()), async { adopt_new(12) }));
        assert!(off(Some(12)).is_empty());
    }

    #[test]
    fn deleted_with_the_chat_gone_with_its_connector_and_whole_at_reset() {
        reset_for_test();
        super::super::switch_for_test(true);
        set(1, "c1", false).unwrap();
        set(2, "c1", false).unwrap();
        set(2, "c2", false).unwrap();
        set(3, "c4", false).unwrap();
        chat_deleted(1);
        assert!(off(Some(1)).is_empty());
        chats_deleted(&[3, 99]);
        assert!(off(Some(3)).is_empty());
        connector_removed("c1");
        assert_eq!(off(Some(2)), ["c2".to_string()].into());
        assert_eq!(reset(), 1);
        assert!(with_store(|s| s.chats.is_empty()));
    }

    #[test]
    fn with_the_preview_off_nothing_is_read_but_a_deletion_still_clears_the_file() {
        reset_for_test();
        super::super::switch_for_test(false);
        assert!(off(Some(4)).is_empty());
        adopt_new(4);
        chat_deleted(4);
        assert!(!touched(), "nothing read with the preview off");
        super::super::switch_for_test(true);
        set(5, "c1", false).unwrap();
        super::super::switch_for_test(false);
        assert!(off(Some(5)).is_empty(), "off: the switches aren't read");
        chat_deleted(5);
        super::super::switch_for_test(true);
        assert!(off(Some(5)).is_empty(), "the deletion took it out anyway");
    }

    #[test]
    fn the_file_keeps_ids_only_and_fields_it_doesnt_know() {
        let newer = json!({ "version": 2, "pinned": true, "chats": { "7": { "off": ["c1", "c3"], "mentions": ["c2"] } } });
        let file: File = serde_json::from_value(newer.clone()).unwrap();
        assert_eq!(file.chats["7"].off, ["c1".to_string(), "c3".to_string()].into());
        let back = serde_json::to_value(&file).unwrap();
        assert_eq!(back["pinned"], true);
        assert_eq!(back["chats"]["7"]["mentions"], json!(["c2"]));
    }
}

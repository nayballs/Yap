//! **A Code chat's screenshots as files** (18 Nov 2026, the maintainer: he
//! put a screenshot in a Code chat, asked Claude to send a helper to look
//! into it, and Claude said it "wasn't saved as a file", so it described it
//! to the helper in words). A message's screenshots reach its Claude Code
//! as pictures in the message (`runner::shots_of`), which Claude sees but
//! can't hand on: a helper (`Agent`) takes words, so a path is the only way.
//! Claude Code's own app keeps a pasted picture as a file too. So each one
//! is also a file Claude can read or pass on:
//!
//! - **Where**: a folder of the chat's own under Yap's data folder,
//!   `code-shots/<chat id>-<its createdTs>/shot-<n>.png` ([`FOLDER`]): never
//!   in the workspace, so the person's project never fills with
//!   screenshots; the chat's `createdTs` in its name, so a later chat that
//!   gets a deleted one's id (ids are the highest plus one) never finds its
//!   screenshots. Numbered in the order they came (`n`), a screenshot sent
//!   again keeping its file. Which picture each file is: the index beside
//!   the folder (`<chat id>-<createdTs>.json`, `{shots: [{n, id}]}`, ids in
//!   `chat-images/`), never in it: nothing in the folder but the shots.
//! - **Given to the run** with `--add-dir` ([`run_dirs`], [`apply`]), as a
//!   chat's added folders are (`menu.rs`), so reading one works in every Mode
//!   with no prompt: only that chat's folder, never the rest of Yap's data.
//!   The chat's own session gets it on every run (made then, so a
//!   screenshot that comes with a note while it works is in it too), an
//!   agent sent from the chat once there's one; a helper Claude starts
//!   works in its session, with its folders. A chat forked from another
//!   (Fork from here) gets the original's too while it's there: its
//!   session's earlier messages name those files.
//! - **Named in the message**: one short line after its words
//!   ([`paths_line`]): "(This screenshot is also a file you can read or pass
//!   to a helper: <path>)". Wherever the words go: the turn's first message
//!   (with the words that came as it started), a note to the turn working.
//!   The phone's and the Voice Assistant's screenshots go the same way: they
//!   reach a Code chat's session through the same `direct::send`.
//! - **Kept as long as the screenshot**: a file goes when its picture in
//!   `chat-images/` does — deleted with its message or its chat
//!   (`images::delete` → [`forget`]), past "Keep chat screenshots" (the
//!   sweep after `images::sweep` → [`sweep`]: a file whose picture is gone,
//!   anything else in the folder after an hour, the folder of a chat that's
//!   gone) — and the chat's folder with the chat ([`chat_deleted`]);
//!   everything with Settings' Delete all… and Delete some of your data →
//!   Chats ([`reset`], `snip::chat_images_delete_all`). Not in Download your
//!   data: they're copies of `chat-images/`'s, which is.
//!
//! **Privacy**: the log has counts, never a path or a picture.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::ai::claude_code::session::Spec;
use crate::snip::images;

/// The folder under Yap's data folder.
pub const FOLDER: &str = "code-shots";
/// Anything in a chat's folder that isn't one of its shots goes after this
/// long (a copy cut short, a file Claude left there).
const STRAY_SECS: u64 = 60 * 60;

/// One writer at a time (a turn starting and a note may both make files).
static LOCK: Mutex<()> = Mutex::new(());

/// Which picture each file is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct Index {
    shots: Vec<Shot>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct Shot {
    /// Its file: `shot-<n>.png`.
    n: u32,
    /// Its picture in `chat-images/`.
    id: String,
}

/// `<data>/code-shots/`.
pub fn root() -> PathBuf {
    super::store::data().join(FOLDER)
}

/// A chat's folder name: `<chat id>-<createdTs>`. Pure.
pub fn key(conversation: u64, created: u64) -> String {
    format!("{conversation}-{created}")
}

/// A folder (or index) name's chat id and `createdTs`; `None`: not one of
/// ours. Pure.
pub fn parse_key(name: &str) -> Option<(u64, u64)> {
    let (c, t) = name.split_once('-')?;
    let number = |s: &str| if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) { s.parse::<u64>().ok() } else { None };
    Some((number(c)?, number(t)?))
}

/// A shot's file name. Pure.
pub fn file_name(n: u32) -> String {
    format!("shot-{n}.png")
}

/// A file name's `n` (`shot-<n>.png`). Pure.
fn number_of(name: &str) -> Option<u32> {
    name.strip_prefix("shot-")?.strip_suffix(".png")?.parse().ok().filter(|n| *n > 0)
}

fn index_path(root: &Path, key: &str) -> PathBuf {
    root.join(format!("{key}.json"))
}

/// A chat's index: `None` when it has none or it can't be read.
fn read_index(root: &Path, key: &str) -> Option<Index> {
    serde_json::from_str(&std::fs::read_to_string(index_path(root, key)).ok()?).ok()
}

fn write_index(root: &Path, key: &str, index: &Index) -> bool {
    match serde_json::to_string(index) {
        Ok(json) => crate::config::atomic_write(&index_path(root, key), &json).is_ok(),
        Err(_) => false,
    }
}

/// Chat `conversation`'s folder name, while it's a chat.
fn key_of(conversation: u64) -> Option<String> {
    crate::chat::chats::get(conversation).map(|c| key(conversation, c.created_ts))
}

/// **Chat `conversation`'s folder**, while it's a chat (not made here).
pub fn dir(conversation: u64) -> Option<PathBuf> {
    key_of(conversation).map(|k| root().join(k))
}

/// **Each of `ids` as a file** in `root/<key>/`: a picture it has already
/// keeps its file (copied again if it's gone), a new one is the next
/// number. The files of those whose picture is still there, in order.
pub fn save_in(root: &Path, key: &str, ids: &[String]) -> Vec<PathBuf> {
    let _held = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let folder = root.join(key);
    if ids.is_empty() || std::fs::create_dir_all(&folder).is_err() {
        return Vec::new();
    }
    // (One that can't be read: started again, past every file there.)
    let mut index = read_index(root, key).unwrap_or_default();
    let mut changed = false;
    let mut out = Vec::new();
    for id in ids {
        let Some(source) = images::path(id).filter(|p| p.is_file()) else { continue };
        let known = index.shots.iter().find(|s| s.id == *id).map(|s| s.n);
        let n = match known {
            Some(n) => n,
            None => {
                let n = next_number(&index, &folder);
                index.shots.push(Shot { n, id: id.clone() });
                changed = true;
                n
            }
        };
        let file = folder.join(file_name(n));
        if !(known.is_some() && file.is_file()) && !copy(&source, &file) {
            continue;
        }
        out.push(file);
    }
    if changed && !write_index(root, key, &index) {
        tracing::info!(shots = index.shots.len(), "Code chat: its screenshots' index wasn't saved");
    }
    out
}

/// The next shot's number: past the index's and every file there.
fn next_number(index: &Index, folder: &Path) -> u32 {
    let on_disk = std::fs::read_dir(folder).into_iter().flatten().flatten().filter_map(|e| number_of(&e.file_name().to_string_lossy()));
    index.shots.iter().map(|s| s.n).chain(on_disk).max().unwrap_or(0) + 1
}

/// `source` copied to `file` whole (a temp file, then renamed).
fn copy(source: &Path, file: &Path) -> bool {
    let tmp = file.with_extension("tmp");
    let ok = std::fs::copy(source, &tmp).is_ok() && std::fs::rename(&tmp, file).is_ok();
    if !ok {
        let _ = std::fs::remove_file(&tmp);
    }
    ok
}

/// **Screenshots `ids` of chat `conversation` as files** ([`save_in`] its
/// folder). None outside a chat.
pub fn files(conversation: Option<u64>, ids: &[String]) -> Vec<PathBuf> {
    if ids.is_empty() {
        return Vec::new();
    }
    let Some(key) = conversation.and_then(key_of) else { return Vec::new() };
    let made = save_in(&root(), &key, ids);
    tracing::info!(conversation = ?conversation, shots = ids.len(), files = made.len(), "Code chat: its screenshots as files");
    made
}

/// **The line that names a message's screenshot files** for Claude, after
/// its words: `paths` the files, `all` whether every picture sent has one
/// (then they're in the pictures' order). `None`: none. Pure.
pub fn paths_line(paths: &[PathBuf], all: bool) -> Option<String> {
    let list = paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ");
    match paths.len() {
        0 => None,
        1 if all => Some(format!("(This screenshot is also a file you can read or pass to a helper: {list})")),
        _ if all => Some(format!("(These screenshots are also files you can read or pass to a helper, in the order they came: {list})")),
        _ => Some(format!("(Some of these screenshots are also files you can read or pass to a helper: {list})")),
    }
}

/// **The screenshot folders a run of chat `conversation` may read**
/// (`--add-dir`): the chat's own — made now for its own session
/// (`own_session`), so one that comes with a note while it works is in a
/// folder it may read — and, a chat forked from another (`forked_from`),
/// the original's while it's there.
pub fn run_dirs(conversation: Option<u64>, own_session: bool, forked_from: Option<u64>) -> Vec<PathBuf> {
    let Some(c) = conversation else { return Vec::new() };
    let mut out = Vec::new();
    if let Some(d) = dir(c) {
        if own_session {
            let _ = std::fs::create_dir_all(&d);
        }
        if d.is_dir() {
            out.push(d);
        }
    }
    if let Some(d) = forked_from.filter(|f| *f != c).and_then(dir).filter(|d| d.is_dir()) {
        out.push(d);
    }
    out
}

/// `dirs` on a run (`--add-dir`), each once. Pure.
pub fn apply(spec: &mut Spec, dirs: Vec<PathBuf>) {
    for d in dirs {
        if !spec.add_dirs.contains(&d) {
            spec.add_dirs.push(d);
        }
    }
}

/// **Pictures `ids` deleted** (`images::delete`: with their message or
/// chat, Delete all…): their files go too, from every chat's folder.
pub fn forget(ids: &[String]) -> usize {
    forget_in(&root(), ids)
}

/// [`forget`] in `root`. How many files went.
pub fn forget_in(root: &Path, ids: &[String]) -> usize {
    if ids.is_empty() || !root.is_dir() {
        return 0;
    }
    let _held = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut gone = 0;
    for key in keys_in(root) {
        let Some(mut index) = read_index(root, &key) else { continue };
        let before = index.shots.len();
        index.shots.retain(|s| {
            if !ids.contains(&s.id) {
                return true;
            }
            if std::fs::remove_file(root.join(&key).join(file_name(s.n))).is_ok() {
                gone += 1;
            }
            false
        });
        if index.shots.len() != before {
            write_index(root, &key, &index);
        }
    }
    if gone > 0 {
        tracing::debug!(files = gone, "Code chat: screenshot files deleted with their pictures");
    }
    gone
}

/// The chats' folder names in `root` (a folder, an index or both).
fn keys_in(root: &Path) -> BTreeSet<String> {
    std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let key = name.strip_suffix(".json").unwrap_or(&name).to_string();
            parse_key(&key).map(|_| key)
        })
        .collect()
}

/// A chat's folder and index gone. Whether anything was there.
fn remove_key(root: &Path, key: &str) -> bool {
    let folder = std::fs::remove_dir_all(root.join(key)).is_ok();
    let index = std::fs::remove_file(index_path(root, key)).is_ok();
    folder || index
}

/// **Chat `conversation` deleted** (`code_chat::chat_deleted`): its folder
/// goes, whatever chat made it (its id may be given to a new one).
pub fn chat_deleted(conversation: u64) -> usize {
    chat_deleted_in(&root(), conversation)
}

/// [`chat_deleted`] in `root`. How many folders went.
pub fn chat_deleted_in(root: &Path, conversation: u64) -> usize {
    if !root.is_dir() {
        return 0;
    }
    let _held = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let gone = keys_in(root).into_iter().filter(|k| parse_key(k).is_some_and(|(c, _)| c == conversation)).filter(|k| remove_key(root, k)).count();
    if gone > 0 {
        tracing::info!(conversation, "Code chat: a deleted chat's screenshot files deleted");
    }
    gone
}

/// **What's past its time** (after `images::sweep`, with the chats read):
/// every chat's folder against its chat and its pictures.
pub fn sweep() -> usize {
    let now = crate::agents::store::now_secs();
    let gone = sweep_in(&root(), &|c, created| crate::chat::chats::get(c).is_some_and(|x| x.created_ts == created), &images::exists, now);
    if gone > 0 {
        tracing::info!(files = gone, "Code chat: screenshot files past their time deleted");
    }
    gone
}

/// [`sweep`] in `root`: `alive(chat id, createdTs)` whether its chat is
/// still there, `exists(id)` whether a picture is. The folder of a chat
/// gone, every file whose picture is gone, anything else in a folder older
/// than an hour; an empty folder and its index. `now` in unix seconds. How
/// many files (and folders) went.
pub fn sweep_in(root: &Path, alive: &dyn Fn(u64, u64) -> bool, exists: &dyn Fn(&str) -> bool, now: u64) -> usize {
    if !root.is_dir() {
        return 0;
    }
    let _held = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let mut gone = 0;
    for key in keys_in(root) {
        let Some((c, created)) = parse_key(&key) else { continue };
        if !alive(c, created) {
            gone += usize::from(remove_key(root, &key));
            continue;
        }
        let folder = root.join(&key);
        let mut index = read_index(root, &key).unwrap_or_default();
        let before = index.shots.clone();
        index.shots.retain(|s| exists(&s.id) && folder.join(file_name(s.n)).is_file());
        let kept: BTreeSet<String> = index.shots.iter().map(|s| file_name(s.n)).collect();
        let mut left = 0;
        for e in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if kept.contains(&name) {
                left += 1;
                continue;
            }
            let shot_gone = number_of(&name).is_some_and(|n| before.iter().any(|s| s.n == n));
            let age = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| now.saturating_sub(d.as_secs()));
            if !(shot_gone || age > STRAY_SECS) {
                left += 1;
                continue;
            }
            let p = e.path();
            if (if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) }).is_ok() {
                gone += 1;
            } else {
                left += 1;
            }
        }
        if left == 0 && index.shots.is_empty() {
            remove_key(root, &key);
        } else if index.shots != before {
            write_index(root, &key, &index);
        }
    }
    gone
}

/// **Every chat's screenshot files** (Settings' Delete all…, Delete some of
/// your data → Chats): the whole folder.
pub fn reset() {
    let root = root();
    if root.exists() {
        let _held = LOCK.lock().unwrap_or_else(|p| p.into_inner());
        if let Err(e) = std::fs::remove_dir_all(&root) {
            tracing::warn!("Code chat: the screenshot files weren't all deleted ({})", e.kind());
        }
    }
}

#[cfg(test)]
mod tests;

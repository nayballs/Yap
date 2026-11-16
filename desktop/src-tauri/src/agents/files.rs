//! **A Code chat's files beside the chat** (15 Nov 2026, after the Claude
//! app's Code tab, where a path in an answer opens the file): a path in a
//! Code chat's answer or rows (`src/convert.js`, `docs/lab.md:105`) is a link
//! when it names a file in the chat's workspace, and opens a read-only view
//! of it beside the chat (`FileTab.svelte`), scrolled to the line named.
//!
//! - **Only the chat's own workspace** ([`resolve`]): the workspace comes
//!   from the conversation, in Rust ([`code_chat::folder_of`]), never from
//!   the page. A path is taken relative to it, or as a full path inside it.
//!   Refused: `..` that climbs out (any `..` in a full path), a device or
//!   network path (`\\?\`, `\\.\`, `\\server`), a drive-relative one
//!   (`\x`), a scheme (`file:`, `http:`), a colon past the drive letter (an
//!   alternate data stream), wildcards, control characters, and anything
//!   whose real place, once the file system has followed symlinks and
//!   junctions (`canonicalize`), isn't under the workspace's real place.
//!   Only files: a folder is no link.
//! - **Read only** ([`read`]): at most [`READ_MAX`] bytes and [`LINES_MAX`]
//!   lines shown (the rest said), a file with a NUL in its first bytes is
//!   "not text", its text decoded as UTF-8 with any bad bytes replaced.
//! - **Opened elsewhere** ([`open`], 16 Nov 2026): a link's click goes
//!   where Settings → Yap Code's "Open file links in" says (Yap's file view,
//!   an editor at the line, or Windows' default app), and a link's menu
//!   offers each; the editors and the command: `agents/editors.rs`. Only a
//!   file [`resolve`] found; an editor started directly (never through a
//!   shell), a default app only for a file Windows wouldn't run.
//! - **The Environment** of the summary panel ([`environment`]): the
//!   workspace's git branch and whether it's a linked worktree, read from
//!   `.git` itself (no git run).
//!
//! The log has counts and kinds, never a path.

use std::io::Read;
use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use super::{code_chat, editors};
use crate::chat::pictures::local;

/// The most bytes of a file the view shows.
pub const READ_MAX: u64 = 1_000_000;
/// The most lines the view shows.
pub const LINES_MAX: usize = 20_000;
/// The most paths checked in one call (an answer's).
pub const CHECK_MAX: usize = 200;
/// A path longer than this is no path.
const PATH_MAX: usize = 1_024;
/// The bytes looked at for a NUL (not text).
const SNIFF: usize = 8_000;

/// **A file in the workspace**, as a link names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileRef {
    /// From the workspace's root, with `/` (`src/app.ts`).
    pub rel: String,
    /// Its full path, as Windows writes it.
    pub abs: String,
    /// The line named (from 1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
}

/// Why a path isn't a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// Nothing that could be a path.
    NotAPath,
    /// Outside the workspace (or climbing out of it).
    Outside,
    /// Not there.
    Missing,
    /// A folder, or something else that isn't a file.
    NotAFile,
}

impl Refused {
    pub fn words(self) -> &'static str {
        match self {
            Refused::NotAPath => "That isn't a file's path.",
            Refused::Outside => "That file isn't in this chat's workspace.",
            Refused::Missing => "There's no such file in this chat's workspace.",
            Refused::NotAFile => "That's a folder, not a file.",
        }
    }
}

/// **A path and the line it names**: `docs/lab.md:105` → (`docs/lab.md`,
/// 105); also `:105:7` (a column), `#L105` and `#L105-L120` (a web link's).
/// A drive letter's colon is the path's. Line 0 is none. Pure.
pub fn split_line(raw: &str) -> (&str, Option<u32>) {
    let digits = |s: &str| !s.is_empty() && s.len() <= 7 && s.bytes().all(|b| b.is_ascii_digit());
    // "#L105", "#L105-L120", "#L105C3"
    if let Some((head, frag)) = raw.rsplit_once("#L") {
        let n: String = frag.chars().take_while(|c| c.is_ascii_digit()).collect();
        let rest = &frag[n.len()..];
        if !head.is_empty() && digits(&n) && (rest.is_empty() || rest.starts_with("-L") || rest.starts_with('C')) {
            return (head, n.parse().ok().filter(|&l| l > 0));
        }
    }
    let mut s = raw;
    let mut nums: Vec<&str> = Vec::new();
    for _ in 0..2 {
        match s.rsplit_once(':') {
            Some((head, tail)) if digits(tail) && !head.is_empty() && !(head.len() == 1 && head.as_bytes()[0].is_ascii_alphabetic()) => {
                nums.push(tail);
                s = head;
            }
            _ => break,
        }
    }
    (s, nums.last().and_then(|n| n.parse().ok()).filter(|&l| l > 0))
}

/// A drive letter and its colon (`C:`).
fn has_drive(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// A full path: `C:\…` or `C:/…` (and `/…` off Windows).
fn is_full(s: &str) -> bool {
    if cfg!(windows) {
        has_drive(s) && matches!(s.as_bytes().get(2), Some(b'\\') | Some(b'/'))
    } else {
        s.starts_with('/')
    }
}

/// The path a link gives, tidied: quotes, backticks and angle brackets
/// round it gone, `./` dropped. `None`: nothing that could be a path.
fn tidy(raw: &str) -> Option<&str> {
    let s = raw.trim().trim_matches(|c| matches!(c, '`' | '"' | '\'' | '<' | '>')).trim();
    let s = s.strip_prefix("./").or_else(|| s.strip_prefix(".\\")).unwrap_or(s);
    (!s.is_empty() && s.len() <= PATH_MAX).then_some(s)
}

/// **Is `given` (a path, its line split off) acceptable as written?** The
/// checks that need no file system. Pure.
fn shape(given: &str) -> Result<(), Refused> {
    // Device and network paths (`\\?\`, `\\.\`, `\\server`), and Windows' drive-relative "\x".
    if given.starts_with("\\\\") || given.starts_with("//") {
        return Err(Refused::Outside);
    }
    if given.chars().any(|c| c.is_control() || matches!(c, '*' | '?' | '<' | '>' | '|' | '"')) {
        return Err(Refused::NotAPath);
    }
    if cfg!(windows) && (given.starts_with('\\') || given.starts_with('/')) {
        return Err(Refused::Outside);
    }
    // A colon anywhere but a drive letter's: a scheme (`file:`, `http:`) or a stream.
    let after_drive = if has_drive(given) { &given[2..] } else { given };
    if after_drive.contains(':') {
        return Err(Refused::NotAPath);
    }
    // `C:x` (relative to that drive's own folder).
    if has_drive(given) && !is_full(given) {
        return Err(Refused::Outside);
    }
    Ok(())
}

/// A relative path's `..` never climbs above where it starts; a full one has
/// none at all. Pure.
fn stays_in(given: &str) -> bool {
    let p = Path::new(given);
    if is_full(given) {
        return !p.components().any(|c| c == Component::ParentDir);
    }
    let mut depth: i32 = 0;
    for c in p.components() {
        match c {
            Component::Normal(_) => depth += 1,
            Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    true
}

/// `path`'s place under `root` with `/` between its parts (both real paths). Pure.
fn rel_of(root: &Path, path: &Path) -> Option<String> {
    let rest = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = rest.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// **The file `raw` names in the workspace at `root`**, if it's one there
/// (see the module's rules).
pub fn resolve(root: &Path, raw: &str) -> Result<FileRef, Refused> {
    let given = tidy(raw).ok_or(Refused::NotAPath)?;
    let (path, line) = split_line(given);
    shape(path)?;
    if !stays_in(path) {
        return Err(Refused::Outside);
    }
    let joined = if is_full(path) { PathBuf::from(path) } else { root.join(path) };
    // The file system's say: symlinks, junctions and short names followed, both sides.
    let real_root = std::fs::canonicalize(root).map_err(|_| Refused::Missing)?;
    let real = std::fs::canonicalize(&joined).map_err(|_| if is_full(path) { Refused::Outside } else { Refused::Missing })?;
    if !real.starts_with(&real_root) || real == real_root {
        return Err(Refused::Outside);
    }
    let meta = std::fs::metadata(&real).map_err(|_| Refused::Missing)?;
    if !meta.is_file() {
        return Err(Refused::NotAFile);
    }
    let rel = rel_of(&real_root, &real).ok_or(Refused::Outside)?;
    Ok(FileRef { rel, abs: local::plain(&real), line })
}

/// **A file as the view shows it.**
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileView {
    #[serde(flatten)]
    pub file: FileRef,
    /// Its text (as much as is shown; empty when it isn't text).
    pub text: String,
    /// Its size in bytes.
    pub size: u64,
    /// More than the view shows.
    pub truncated: bool,
    /// Not text (a NUL in its first bytes).
    pub binary: bool,
}

/// `bytes` as the view's text: a BOM gone, bad UTF-8 replaced, at most
/// [`LINES_MAX`] lines and cut at a line's end when `cut` (more than was
/// read). `(text, truncated)`. Pure.
pub fn as_text(bytes: &[u8], cut: bool) -> (String, bool) {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    let mut truncated = cut;
    if cut {
        if let Some(at) = text.rfind('\n') {
            text.truncate(at);
        }
    }
    if let Some((at, _)) = text.match_indices('\n').nth(LINES_MAX - 1) {
        text.truncate(at);
        truncated = true;
    }
    (text, truncated)
}

/// **Read the file `raw` names** in the workspace at `root`.
pub fn read(root: &Path, raw: &str) -> Result<FileView, Refused> {
    let file = resolve(root, raw)?;
    let mut f = std::fs::File::open(&file.abs).map_err(|_| Refused::Missing)?;
    let size = f.metadata().map(|m| m.len()).unwrap_or(0);
    let mut bytes = Vec::new();
    (&mut f).take(READ_MAX + 1).read_to_end(&mut bytes).map_err(|_| Refused::Missing)?;
    let cut = bytes.len() as u64 > READ_MAX;
    if cut {
        bytes.truncate(READ_MAX as usize);
    }
    if bytes[..bytes.len().min(SNIFF)].contains(&0) {
        return Ok(FileView { file, text: String::new(), size, truncated: false, binary: true });
    }
    let (text, truncated) = as_text(&bytes, cut);
    Ok(FileView { file, text, size, truncated, binary: false })
}

// ---- Opening a file elsewhere -------------------------------------------------------------------

/// **Where a file went** (`code_file_open`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "how", rename_all = "camelCase")]
pub enum Opened {
    /// The page shows it in Yap's own view: the setting's choice, or what it
    /// fell back to (`note` says why).
    Yap {
        #[serde(skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// In this editor.
    Editor { name: String },
    /// In Windows' default app for it.
    App,
}

/// The workbooks only Yap's sheet view shows (an editor would show their bytes).
const WORKBOOKS: &[&str] = &["xlsx", "xlsm", "xls", "xlsb", "ods"];

/// **Open the file `raw` names** in the workspace at `root`, at `line`, the
/// way `with` says (see `agents/editors.rs`): [`editors::YAP`], [`editors::DEFAULT_APP`],
/// [`editors::ANY_EDITOR`] or an editor's id. `link`: a link's click (the
/// setting's way), where a workbook stays in Yap's sheet view.
pub fn open(root: &Path, raw: &str, line: Option<u32>, with: &str, link: bool) -> Result<Opened, String> {
    let file = resolve(root, raw).map_err(|r| r.words().to_string())?;
    let line = line.or(file.line);
    let workspace = local::plain(&std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf()));
    if with == editors::YAP || (with != editors::DEFAULT_APP && with != editors::ANY_EDITOR && !editors::known(with)) {
        return Ok(Opened::Yap { note: None });
    }
    if with == editors::DEFAULT_APP {
        if let Some(why) = editors::runs_when_opened(Path::new(&file.abs)) {
            tracing::info!("Code chat: a file Windows would run shown in Yap instead");
            return Ok(Opened::Yap { note: Some(why) });
        }
        editors::open_default(&file.abs, &file.rel, line)?;
        return Ok(Opened::App);
    }
    if link && editors::ending(Path::new(&file.abs)).is_some_and(|e| WORKBOOKS.contains(&e.as_str())) {
        return Ok(Opened::Yap { note: None });
    }
    let list = editors::found(false);
    let editor = if with == editors::ANY_EDITOR {
        let choice = crate::config::load().agents_open_files;
        editors::preferred(&list, &choice).ok_or("Yap couldn't find a code editor on this PC.")?
    } else {
        match list.iter().find(|e| e.id == with) {
            Some(e) => e,
            None => {
                return Ok(Opened::Yap { note: Some(format!("{} isn't on this PC, so the file is shown here. Settings → Yap Code chooses where files open.", editors::name_of(with))) });
            }
        }
    };
    // (The suite's older record, kept: `editor:<rel>:<line>`.)
    if crate::shell::e2e::active() {
        crate::shell::commands::e2e_kept(&format!("editor:{}:{}", file.rel, line.unwrap_or(0)));
    }
    editors::start(editor, &file.abs, &file.rel, line, &workspace)?;
    Ok(Opened::Editor { name: editor.name.clone() })
}

// ---- Environment --------------------------------------------------------------------------------

/// **A Code chat's workspace, for the summary panel's Environment.**
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub name: String,
    pub path: String,
    /// The folder is there.
    pub exists: bool,
    /// In a git repository.
    pub repo: bool,
    /// Its branch (a detached HEAD: its commit's first 7 characters).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub detached: bool,
    /// A linked worktree (`git worktree add`).
    pub worktree: bool,
}

/// What a `HEAD` file says: `(branch, detached)`. Pure.
pub fn head_of(head: &str) -> (Option<String>, bool) {
    let h = head.trim();
    if let Some(r) = h.strip_prefix("ref:") {
        let r = r.trim();
        let name = r.strip_prefix("refs/heads/").unwrap_or(r);
        return ((!name.is_empty()).then(|| name.to_string()), false);
    }
    if h.len() >= 7 && h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return (Some(h[..7].to_string()), true);
    }
    (None, false)
}

/// The git folder of the repository `dir` is in: `(git_dir, linked_worktree)`.
/// A `.git` file ("gitdir: …") is a worktree's or a submodule's: a worktree's
/// points into `<repo>/.git/worktrees/<name>`.
pub fn git_dir_of(dir: &Path) -> Option<(PathBuf, bool)> {
    for d in dir.ancestors() {
        let dot = d.join(".git");
        if dot.is_dir() {
            return Some((dot, false));
        }
        if dot.is_file() {
            let text = std::fs::read_to_string(&dot).ok()?;
            let at = text.lines().find_map(|l| l.trim().strip_prefix("gitdir:")).map(str::trim)?;
            let git = if Path::new(at).is_absolute() { PathBuf::from(at) } else { d.join(at) };
            let worktree = git.parent().and_then(|p| p.file_name()).is_some_and(|n| n == "worktrees");
            return Some((git, worktree));
        }
    }
    None
}

/// The Environment of the workspace `name` at `path`.
pub fn environment(name: &str, path: &str) -> Environment {
    let dir = Path::new(path);
    let mut env = Environment { name: name.to_string(), path: path.to_string(), exists: dir.is_dir(), ..Default::default() };
    if !env.exists {
        return env;
    }
    if let Some((git, worktree)) = git_dir_of(dir) {
        env.repo = true;
        env.worktree = worktree;
        if let Ok(head) = std::fs::read_to_string(git.join("HEAD")) {
            let (branch, detached) = head_of(&head);
            env.branch = branch;
            env.detached = detached;
        }
    }
    env
}

// ---- the commands -------------------------------------------------------------------------------

/// The workspace root of Code chat `conversation`.
fn root_of(conversation: u64) -> Result<PathBuf, String> {
    let folder = code_chat::folder_of(Some(conversation)).ok_or("This chat isn't a Code chat.")?;
    Ok(PathBuf::from(folder.path))
}

/// **Which of `paths` are files in Code chat `conversation`'s workspace**:
/// each its file, or `None` (at most [`CHECK_MAX`] looked at; none in a
/// chat that isn't a Code chat).
#[tauri::command]
pub async fn code_files_check(conversation: u64, paths: Vec<String>) -> Vec<Option<FileRef>> {
    let Ok(root) = root_of(conversation) else { return vec![None; paths.len()] };
    tauri::async_runtime::spawn_blocking(move || {
        let found: Vec<Option<FileRef>> = paths.iter().enumerate().map(|(i, p)| if i < CHECK_MAX { resolve(&root, p).ok() } else { None }).collect();
        tracing::debug!(asked = paths.len(), files = found.iter().flatten().count(), "Code chat: paths checked");
        found
    })
    .await
    .unwrap_or_default()
}

/// **A file of Code chat `conversation`'s workspace, read** for its view.
#[tauri::command]
pub async fn code_file_read(conversation: u64, path: String) -> Result<FileView, String> {
    let root = root_of(conversation)?;
    tauri::async_runtime::spawn_blocking(move || read(&root, &path).map_err(|r| r.words().to_string())).await.map_err(|e| e.to_string())?
}

/// **Open in editor**: the file at its line in the person's code editor
/// (the setting's, else the first found). Which editor it was.
#[tauri::command]
pub async fn code_file_open_editor(conversation: u64, path: String, line: Option<u32>) -> Result<String, String> {
    let root = root_of(conversation)?;
    let opened = tauri::async_runtime::spawn_blocking(move || open(&root, &path, line, editors::ANY_EDITOR, false)).await.map_err(|e| e.to_string())??;
    match opened {
        Opened::Editor { name } => Ok(name),
        Opened::Yap { note } => Err(note.unwrap_or_else(|| "Yap couldn't find a code editor on this PC.".into())),
        Opened::App => Ok("its app".into()),
    }
}

/// **A file link opened** (a click, or its menu): the file at its line where
/// `with` says — `None`, the setting (Settings → Yap Code → "Open file links
/// in"); `yap`, `default`, `editor` or an editor's id from the menu. Yap's
/// own view is the page's to show (`{how: "yap"}`, with a `note` when it
/// stands in for what was asked).
#[tauri::command]
pub async fn code_file_open(conversation: u64, path: String, line: Option<u32>, with: Option<String>) -> Result<Opened, String> {
    let root = root_of(conversation)?;
    let link = with.is_none();
    let with = with.unwrap_or_else(|| crate::config::load().agents_open_files);
    tauri::async_runtime::spawn_blocking(move || open(&root, &path, line, &with, link)).await.map_err(|e| e.to_string())?
}

/// **Show in folder**: Explorer at a file of Code chat `conversation`'s
/// workspace, selected (an output's right-click menu; only a file
/// [`resolve`] finds in the workspace). A test run keeps it for the suite.
#[tauri::command]
pub async fn code_file_reveal(conversation: u64, path: String) -> Result<(), String> {
    let root = root_of(conversation)?;
    tauri::async_runtime::spawn_blocking(move || {
        let file = resolve(&root, &path).map_err(|r| r.words().to_string())?;
        if crate::shell::e2e::active() {
            crate::shell::commands::e2e_kept(&format!("reveal:{}", file.rel));
            return Ok(());
        }
        tauri_plugin_opener::reveal_item_in_dir(&file.abs).map_err(|e| format!("Couldn't open the folder: {e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// **The summary panel's Environment**: Code chat `conversation`'s workspace,
/// or (a new code chat, no conversation yet) the workspace at `workspace`,
/// when it's one of Yap Code's.
#[tauri::command]
pub async fn code_chat_environment(conversation: Option<u64>, workspace: Option<String>) -> Result<Environment, String> {
    let folder = conversation.and_then(|c| code_chat::folder_of(Some(c))).or_else(|| workspace.as_deref().and_then(code_chat::folder_at)).ok_or("That isn't one of Yap Code's workspaces.")?;
    tauri::async_runtime::spawn_blocking(move || environment(&folder.name, &folder.path)).await.map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "files/tests.rs"]
mod tests;

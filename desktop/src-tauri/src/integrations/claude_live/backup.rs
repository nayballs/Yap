//! **Backups before Claude Code edits a file**, so a Job can be undone.
//!
//! The run's `--settings` has an http `PreToolUse` hook on
//! `Edit|Write|MultiEdit|NotebookEdit` (code.claude.com/docs/en/hooks: the
//! hook's JSON input POSTed, `headers` with literal values, a 2xx with `{}`
//! = carry on). Its input names the file in `tool_input.file_path` (or
//! `notebook_path`), relative to its `cwd` if it isn't absolute. The first
//! time a run touches a file:
//!
//! - it exists: copied into the run's backup folder as `0001-<name>`
//!   (a plain name, never a path), unless it's over [`MAX_BACKUP_BYTES`]
//!   (noted as too big);
//! - it doesn't: noted as **created**, so Undo deletes it.
//!
//! `manifest.json` in the backup folder maps each backup to the original's
//! absolute path (written whole each time, through a temporary file). A
//! later edit of the same file in the run changes nothing: the first copy
//! is the one from before the run. The hook always answers `{}` at once —
//! it never blocks or denies a tool — and a file it couldn't copy is noted
//! as such.
//!
//! [`restore_all`] puts every backup back and deletes the files the run
//! created — for several runs at once too (a job's turns, each its own
//! backup folder: `jobs/<id>/backup/<turn>/`), each file as its earliest
//! backup has it, so the files are as they were before the first. It touches only the paths the manifest names (plain absolute paths, outside the backup
//! folder) and reads only plain file names inside the backup folder; any
//! other is refused and said in the [`RestoreReport`].
//!
//! The log has counts and states, never a path.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::server::Run;

/// The largest file backed up (bigger ones are noted, not copied).
pub const MAX_BACKUP_BYTES: u64 = 50 * 1024 * 1024;
/// The manifest's name in the backup folder.
pub const MANIFEST: &str = "manifest.json";
const MANIFEST_VERSION: u32 = 1;
/// The tools the hook backs up before.
const EDITING: &[&str] = &["Edit", "Write", "MultiEdit", "NotebookEdit"];

/// What happened to a file the run touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    /// Copied before the first edit.
    Copied,
    /// It didn't exist before the run: Undo deletes it.
    Created,
    /// Over [`MAX_BACKUP_BYTES`]: not copied.
    TooBig,
    /// It couldn't be read or copied.
    Failed,
}

/// One file in the manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// The file's absolute path.
    pub original: PathBuf,
    pub state: State,
    /// The copy's file name in the backup folder (`Copied` only).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub backup: String,
    /// The file's size when copied (or found too big).
    #[serde(default)]
    pub bytes: u64,
    /// The tool that touched it first ("Edit", "Write"…).
    #[serde(default)]
    pub tool: String,
    /// When (unix seconds).
    #[serde(default)]
    pub ts: u64,
    /// Why it wasn't copied (`TooBig`, `Failed`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// **`manifest.json`**.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub version: u32,
    pub files: Vec<Entry>,
}

impl Default for Manifest {
    fn default() -> Self {
        Manifest { version: MANIFEST_VERSION, files: Vec::new() }
    }
}

/// A file Undo didn't put back, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    pub path: PathBuf,
    pub reason: String,
}

/// **What Undo did.**
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreReport {
    /// Put back as they were before the run.
    pub restored: Vec<PathBuf>,
    /// Made by the run, deleted.
    pub deleted: Vec<PathBuf>,
    /// Nothing to do, or never backed up (too big, unreadable).
    pub skipped: Vec<Skipped>,
    /// Not touched: not a file the manifest names, or not a safe path.
    pub refused: Vec<Skipped>,
    /// Couldn't be put back.
    pub failed: Vec<Skipped>,
    /// The manifest couldn't be read: nothing was done.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ---- paths ----

/// `p` with `.` and `..` worked out, if it's absolute and `..` never goes
/// above its root.
fn normal(p: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::Prefix(_) | Component::RootDir => out.push(c.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::Normal(s) => out.push(s),
        }
    }
    out.is_absolute().then_some(out)
}

/// A plain absolute path: no `.` or `..`, nothing to work out.
fn is_plain(p: &Path) -> bool {
    p.is_absolute()
        && !p.as_os_str().to_string_lossy().contains('\0')
        && p.components().all(|c| !matches!(c, Component::CurDir | Component::ParentDir))
}

/// The path as compared: Windows' names don't care about case.
fn key(p: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        PathBuf::from(p.to_string_lossy().to_lowercase())
    }
    #[cfg(not(windows))]
    {
        p.to_path_buf()
    }
}

fn inside(p: &Path, dir: &Path) -> bool {
    let dir = normal(dir).unwrap_or_else(|| dir.to_path_buf());
    key(p).starts_with(key(&dir))
}

/// `p` relative to `folder` when it's in it ("src/app.ts" for the card).
pub(super) fn relative_to(p: &Path, folder: &Path) -> Option<String> {
    let (p, folder) = (normal(p)?, normal(folder)?);
    if !key(&p).starts_with(key(&folder)) {
        return None;
    }
    let rel: PathBuf = p.components().skip(folder.components().count()).collect();
    (!rel.as_os_str().is_empty()).then(|| rel.display().to_string())
}

/// A backup's name: one plain file name, nothing that could reach outside
/// the backup folder (no separators, no drive, no stream).
fn plain_name(name: &str) -> bool {
    let mut parts = Path::new(name).components();
    matches!((parts.next(), parts.next()), (Some(Component::Normal(n)), None) if n == name)
        && !name.contains([':', '/', '\\', '\0'])
        && name != MANIFEST
}

/// `0001-app.ts`: the entry's number and the file's name, made safe.
fn backup_name(n: usize, original: &Path) -> String {
    let name = original.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let safe: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' })
        .take(80)
        .collect();
    let safe = safe.trim_matches('.');
    format!("{n:04}-{}", if safe.is_empty() { "file" } else { safe })
}

// ---- the manifest ----

/// The backup folder's manifest (an empty one when there's none yet).
pub fn read_manifest(dir: &Path) -> Result<Manifest, String> {
    match std::fs::read(dir.join(MANIFEST)) {
        Ok(raw) => serde_json::from_slice(&raw).map_err(|_| "The backup's list of files couldn't be read.".to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Manifest::default()),
        Err(e) => Err(format!("The backup's list of files couldn't be read: {}.", e.kind())),
    }
}

fn write_manifest(dir: &Path, m: &Manifest) -> std::io::Result<()> {
    let tmp = dir.join(format!("{MANIFEST}.tmp"));
    let body = serde_json::to_vec_pretty(m).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, dir.join(MANIFEST))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---- the hook ----

/// The hook for a run: one backup at a time, never an error back.
pub(super) fn pre_tool_use(run: &Run, input: &Value, max: u64) {
    let Some(dir) = run.spec.backup_dir.as_deref() else { return };
    let _one_at_a_time = run.backups.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(state) = record(dir, run.spec.folder.as_deref(), input, max) {
        tracing::info!(run = %run.id, state = ?state, "Claude Code: a file backed up before its first edit");
    }
}

/// **Back up the file a `PreToolUse` input names**, the first time: what
/// was noted, or `None` (not an edit, no path, already noted, the backup
/// folder itself, or a manifest that couldn't be read).
pub(super) fn record(dir: &Path, folder: Option<&Path>, input: &Value, max: u64) -> Option<State> {
    let tool = input.get("tool_name").and_then(Value::as_str).unwrap_or("");
    if !EDITING.contains(&tool) {
        return None;
    }
    let ti = input.get("tool_input")?;
    let raw = ["file_path", "notebook_path"]
        .iter()
        .find_map(|k| ti.get(*k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()))?;
    let given = Path::new(raw);
    let joined = if given.is_absolute() {
        given.to_path_buf()
    } else {
        let base = input
            .get("cwd")
            .and_then(Value::as_str)
            .filter(|c| !c.trim().is_empty())
            .map(PathBuf::from)
            .or_else(|| folder.map(Path::to_path_buf))?;
        base.join(given)
    };
    let path = normal(&joined)?;
    if inside(&path, dir) {
        return None;
    }
    let mut manifest = match read_manifest(dir) {
        Ok(m) => m,
        Err(_) => {
            tracing::warn!("Claude Code: a run's backup list couldn't be read; no more backups for it");
            return None;
        }
    };
    if manifest.files.iter().any(|e| key(&e.original) == key(&path)) {
        return None;
    }
    let mut entry = Entry { original: path.clone(), state: State::Created, backup: String::new(), bytes: 0, tool: tool.to_string(), ts: now_secs(), note: String::new() };
    match std::fs::metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => {
            entry.state = State::Failed;
            entry.note = format!("couldn't be read: {}", e.kind());
        }
        Ok(meta) if meta.is_dir() => return None,
        Ok(meta) if meta.len() > max => {
            entry.state = State::TooBig;
            entry.bytes = meta.len();
            entry.note = format!("over {} MB, not backed up", max / (1024 * 1024));
        }
        Ok(meta) => {
            let name = backup_name(manifest.files.len() + 1, &path);
            let copied = std::fs::create_dir_all(dir).and_then(|()| std::fs::copy(&path, dir.join(&name)));
            match copied {
                Ok(_) => {
                    entry.state = State::Copied;
                    entry.backup = name;
                    entry.bytes = meta.len();
                }
                Err(e) => {
                    entry.state = State::Failed;
                    entry.note = format!("couldn't be copied: {}", e.kind());
                }
            }
        }
    }
    let state = entry.state;
    manifest.files.push(entry);
    if let Err(e) = std::fs::create_dir_all(dir).and_then(|()| write_manifest(dir, &manifest)) {
        tracing::warn!(kind = %e.kind(), "Claude Code: a run's backup list couldn't be saved");
    }
    Some(state)
}

// ---- Undo ----

/// **Undo a run**: every backup put back, every file it created deleted
/// ([`restore_all`] of one).
#[cfg(test)]
pub fn restore(dir: &Path) -> RestoreReport {
    restore_all(&[dir.to_path_buf()])
}

/// The files a backup folder's manifest names: what a turn changed, for
/// "Undo the later change first".
pub fn originals(dir: &Path) -> Result<Vec<PathBuf>, String> {
    Ok(read_manifest(dir)?.files.into_iter().map(|e| e.original).collect())
}

/// The same file (Windows' names don't care about case).
pub fn same_path(a: &Path, b: &Path) -> bool {
    key(a) == key(b)
}

/// **Undo several runs at once** (a job's turns, `dirs` oldest first): each
/// file put back as its *earliest* backup has it — as it was before the
/// first of them — and every file one of them made deleted. A manifest that
/// can't be read: nothing done.
pub fn restore_all(dirs: &[PathBuf]) -> RestoreReport {
    let mut report = RestoreReport::default();
    let mut manifests = Vec::new();
    for dir in dirs {
        match read_manifest(dir) {
            Ok(m) => manifests.push((dir, m)),
            Err(e) => {
                report.error = Some(e);
                return report;
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    for (dir, manifest) in &manifests {
        for e in &manifest.files {
            if seen.insert(key(&e.original)) {
                put_back(dir, e, &mut report);
            }
        }
    }
    tracing::info!(
        restored = report.restored.len(),
        deleted = report.deleted.len(),
        skipped = report.skipped.len(),
        refused = report.refused.len(),
        failed = report.failed.len(),
        "Claude Code: a run undone"
    );
    report
}

fn put_back(dir: &Path, e: &Entry, report: &mut RestoreReport) {
    let skip = |reason: &str| Skipped { path: e.original.clone(), reason: reason.to_string() };
    if !is_plain(&e.original) {
        report.refused.push(skip("not a plain absolute path"));
        return;
    }
    if inside(&e.original, dir) {
        report.refused.push(skip("inside the backup folder"));
        return;
    }
    match e.state {
        State::Copied => {
            if !plain_name(&e.backup) {
                report.refused.push(skip("its backup isn't a file in the backup folder"));
                return;
            }
            let from = dir.join(&e.backup);
            if !from.is_file() {
                report.failed.push(skip("its backup is missing"));
                return;
            }
            if e.original.is_dir() {
                report.failed.push(skip("a folder is there now"));
                return;
            }
            let copied = match e.original.parent() {
                Some(parent) => std::fs::create_dir_all(parent).and_then(|()| std::fs::copy(&from, &e.original)),
                None => std::fs::copy(&from, &e.original),
            };
            match copied {
                Ok(_) => report.restored.push(e.original.clone()),
                Err(err) => report.failed.push(skip(&format!("couldn't be put back: {}", err.kind()))),
            }
        }
        State::Created => match std::fs::symlink_metadata(&e.original) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => report.skipped.push(skip("already gone")),
            Err(err) => report.failed.push(skip(&format!("couldn't be read: {}", err.kind()))),
            Ok(meta) if meta.is_dir() => report.refused.push(skip("a folder is there now")),
            Ok(_) => match std::fs::remove_file(&e.original) {
                Ok(()) => report.deleted.push(e.original.clone()),
                Err(err) => report.failed.push(skip(&format!("couldn't be deleted: {}", err.kind()))),
            },
        },
        State::TooBig => report.skipped.push(skip("too big to back up, so it can't be put back")),
        State::Failed => report.skipped.push(skip("it couldn't be backed up, so it can't be put back")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_worked_out_and_names_kept_plain() {
        let root = std::env::temp_dir();
        let a = root.join("a");
        assert_eq!(normal(&a.join("b").join("..").join("c")), Some(a.join("c")));
        assert_eq!(normal(&a.join(".").join("c")), Some(a.join("c")));
        assert_eq!(normal(Path::new("relative/x")), None, "not absolute");
        assert!(is_plain(&a.join("c")));
        assert!(!is_plain(&a.join("..").join("c")));
        assert!(!is_plain(Path::new("c")));
        assert_eq!(relative_to(&a.join("src").join("app.ts"), &a), Some(Path::new("src").join("app.ts").display().to_string()));
        assert_eq!(relative_to(&root.join("elsewhere"), &a), None);
        assert!(plain_name("0001-app.ts"));
        for bad in ["../x", "a/b", "a\\b", "C:x", "", ".", "..", MANIFEST, "x:stream"] {
            assert!(!plain_name(bad), "{bad}");
        }
        assert_eq!(backup_name(3, &a.join("my file (1).ts")), "0003-my_file__1_.ts");
        assert_eq!(backup_name(12, &a.join("..")), "0012-file");
    }
}

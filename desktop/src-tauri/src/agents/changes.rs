//! **What a turn's commands changed** (15 Nov 2026). Claude Code's own edit
//! tools go through Yap's backup hook (`integrations/claude_live/backup.rs`);
//! a file a command makes or changes — a script (exceljs, Python), `npm`, a
//! formatter — never does. The Yap Lab found it on 15 Nov: an exceljs script
//! wrote `costs.xlsx`, and Outputs said "Nothing made yet". So each turn reads
//! the workspace before it and compares after:
//!
//! - **Before** ([`before`], as the turn starts, before Claude Code does):
//!   - in a Git repository (the person's own Git, and `git rev-parse` answers
//!     in the workspace): `git status --porcelain=v1 -z --untracked-files=all`
//!     for the workspace's part of it (`.gitignore` kept), each file it lists
//!     with its size and times, and the commit at `HEAD` — every other file is
//!     as Git has it there;
//!   - elsewhere (or when Git can't say), a walk ([`walk`]) that leaves out
//!     build output and hidden folders as the @ picker's does
//!     (`mention::SKIPPED`), links, and Yap's own data folder: every file, its
//!     size and times.
//!   - **Caps**: a walk stops at [`MOST_ENTRIES`] entries or [`WALK_TIME`];
//!     Git at [`GIT_TIMEOUT`] or [`MOST_ENTRIES`] files listed. Past them (a
//!     drive root like `E:\`, a home folder) the check is skipped for the
//!     turn ([`Skip`]), said in the log with counts only, never a stall.
//! - **After** ([`after`], once its process is gone): the same again,
//!   compared, and **only what changed while a command ran** counts
//!   ([`classify`]): a `Bash` or `PowerShell` call from its start to its
//!   result, one in the background to the turn's end ([`Window`]), with
//!   [`SLACK`] either side — by a new file's birth (which unpacking an archive
//!   doesn't set back) or a changed one's last write. So the person's own
//!   edits in the workspace meanwhile aren't the turn's. With Git, `HEAD`
//!   moved (Claude committed) adds the files between the two commits.
//!   - **New** (not there before): Undo deletes it.
//!   - **Changed, clean in Git at the turn's start**: Git's own copy (`git
//!     cat-file --filters <commit>:<path>`, as a checkout would write it) is
//!     kept as its backup: Undo puts it back.
//!   - **Changed, anything else** (the person's uncommitted work, a folder
//!     with no Git): no backup, and it's said — Undo can't put it back.
//!
//!   At most [`MOST_FOUND`] files; past that nothing is noted (logged).
//! - **Why not a copy before each command**: a command's words don't say
//!   which files it will write (an exceljs script's file is named inside the
//!   script), so a snapshot of "the files it's about to touch" would be a
//!   guess, and copying the whole workspace before every command doesn't
//!   scale. Git already holds every clean file as it was.
//! - **Kept** ([`settle`]): in the turn's backup manifest, marked `command`,
//!   each with the file as the turn left it (Undo's guard: one changed since
//!   is left alone); and, by the runner, in the turn's `files` and
//!   `by_command` (`store::CommandFile`), so Outputs, the summary, the file
//!   view, Files changed and the report list them. A file a command made
//!   that Claude Code then edited is new (its hook's copy was of the
//!   command's file); one a command changed before Claude Code's copy has no
//!   copy from before, unless Git has it.
//! - **Git runs as the person's own** ([`find_git`]: an absolute folder on
//!   `PATH`, else Git for Windows' own `cmd\git.exe`; never one in the
//!   workspace), no window, in the kill-on-close job, with
//!   `GIT_OPTIONAL_LOCKS=0` (no index refresh: Claude Code may be running Git
//!   there itself), `core.fsmonitor` off, literal pathspecs, no prompts.
//!
//! The log has counts and times, never a path.

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime};

use crate::ai::claude_code::cli::{self, Program};
use crate::integrations::claude_live::backup::{self, Entry, Manifest, Stamp, State};

use super::store::CommandFile;

/// The most entries a walk reads, and the most files Git may list.
pub const MOST_ENTRIES: usize = 20_000;
/// The longest a walk may take.
pub const WALK_TIME: Duration = Duration::from_secs(2);
/// The longest one Git command may take.
pub const GIT_TIMEOUT: Duration = Duration::from_secs(5);
/// The most files a turn's commands may have changed for them to be noted.
pub const MOST_FOUND: usize = 500;
/// A change this close to a command's run is its.
pub const SLACK: Duration = Duration::from_secs(2);
/// The longest Git's copy of one file may take, and all of a turn's.
const COPY_TIMEOUT: Duration = Duration::from_secs(10);
const COPY_BUDGET: Duration = Duration::from_secs(20);
/// How deep a walk goes.
const DEPTH: usize = 16;
/// The most Git may print for one answer.
const GIT_OUT_MAX: usize = 8 << 20;
/// Said for a file a command changed after Claude Code's own copy was made of it.
const COPIED_LATE: &str = "a command changed it before Claude Code's edit, so Yap has no copy of it from before";

/// **A file as a scan saw it**: its size and times.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seen {
    pub bytes: u64,
    pub modified: Option<SystemTime>,
    /// When it was made (`None`: the file system doesn't say).
    pub born: Option<SystemTime>,
}

impl Seen {
    pub fn of(meta: &std::fs::Metadata) -> Seen {
        Seen { bytes: meta.len(), modified: meta.modified().ok(), born: meta.created().ok() }
    }

    /// The same file, unchanged (its size and last write).
    fn same(&self, other: &Seen) -> bool {
        self.bytes == other.bytes && self.modified == other.modified
    }
}

/// **While a command ran**: from its start to its end.
pub type Window = (SystemTime, SystemTime);

/// Why the check was skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// Over [`MOST_ENTRIES`] (how many were read).
    TooMany(usize),
    /// Over [`WALK_TIME`] or [`GIT_TIMEOUT`] (entries read by then).
    TooSlow(usize),
    /// Over [`MOST_FOUND`] files changed.
    TooManyChanged(usize),
    /// The workspace couldn't be read.
    Unreadable,
}

impl Skip {
    /// The log's words and count.
    pub fn said(&self) -> (&'static str, usize) {
        match *self {
            Skip::TooMany(n) => ("too many files", n),
            Skip::TooSlow(n) => ("too slow to read", n),
            Skip::TooManyChanged(n) => ("too many files changed", n),
            Skip::Unreadable => ("unreadable", 0),
        }
    }
}

/// How the workspace was read.
enum How {
    /// `prefix`: the workspace's place in the repository ("" at its top,
    /// "tools/garden/" below it); `head`: the commit at `HEAD` (none yet).
    Git { git: Program, prefix: String, head: Option<String> },
    Walk,
}

/// **The workspace before a turn**.
pub struct Before {
    folder: PathBuf,
    how: How,
    /// Each file seen ([`key`] → its path, `/` between parts, and what was
    /// seen): with Git, only those it listed (changed, new, untracked).
    files: HashMap<String, (String, Seen)>,
    /// With Git, every path it listed, a deleted one too ([`key`]s): none of
    /// them was as Git has it at `HEAD`.
    listed: HashSet<String>,
}

impl Before {
    /// With Git (for the log).
    pub fn git(&self) -> bool {
        matches!(self.how, How::Git { .. })
    }

    /// Files read (for the log).
    pub fn files_read(&self) -> usize {
        self.files.len()
    }
}

/// **A file a command made or changed**.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Relative to the workspace, `/` between parts.
    pub rel: String,
    pub path: PathBuf,
    /// It wasn't there before the turn.
    pub new: bool,
    /// It was clean in Git at the turn's start: Git has it as it was.
    pub clean: bool,
    /// As the turn left it.
    pub now: Stamp,
}

/// The key a path is known by: `/` between parts, and (on Windows) any case.
pub fn key(rel: &str) -> String {
    let k = rel.replace('\\', "/");
    if cfg!(windows) {
        k.to_lowercase()
    } else {
        k
    }
}

fn os_path(folder: &Path, rel: &str) -> PathBuf {
    let mut p = folder.to_path_buf();
    for part in rel.split('/').filter(|s| !s.is_empty()) {
        p.push(part);
    }
    p
}

// ---- before and after ---------------------------------------------------------------------------

/// Folders a walk leaves out besides build output: Yap's own data.
fn left_out_dirs() -> Vec<PathBuf> {
    vec![crate::config::data_dir()]
}

/// **The workspace at `folder` before a turn**: Git's view where it's a
/// repository, else a walk. Blocking; capped (see the module's notes).
pub fn before(folder: &Path) -> Result<Before, Skip> {
    if let Some(git) = find_git() {
        if let Some(b) = git_before(git, folder)? {
            return Ok(b);
        }
    }
    let files = walk(folder, &left_out_dirs(), MOST_ENTRIES, WALK_TIME)?;
    Ok(Before { folder: folder.to_path_buf(), how: How::Walk, files: files.into_iter().map(|(r, s)| (key(&r), (r, s))).collect(), listed: HashSet::new() })
}

/// Git's view of `folder` (`None`: not a repository, or Git couldn't say: a walk then).
fn git_before(git: Program, folder: &Path) -> Result<Option<Before>, Skip> {
    let Ok(out) = git_out(&git, folder, &["rev-parse", "--show-prefix"], None) else { return Ok(None) };
    let prefix = String::from_utf8_lossy(&out).trim_end_matches(['\r', '\n']).to_string();
    let head = commit_at_head(&git, folder);
    let listed = match git_status(&git, folder, &prefix) {
        Ok(l) => l,
        Err(GitErr::Failed) => return Ok(None),
        Err(GitErr::TooBig) => return Err(Skip::TooMany(MOST_ENTRIES)),
        Err(GitErr::TimedOut) => return Err(Skip::TooSlow(0)),
    };
    if listed.len() > MOST_ENTRIES {
        return Err(Skip::TooMany(listed.len()));
    }
    let mut files = HashMap::new();
    let all: HashSet<String> = listed.iter().map(|(_, r)| key(r)).collect();
    for (_, rel) in listed {
        if let Ok(meta) = std::fs::metadata(os_path(folder, &rel)) {
            if meta.is_file() {
                files.insert(key(&rel), (rel, Seen::of(&meta)));
            }
        }
    }
    Ok(Some(Before { folder: folder.to_path_buf(), how: How::Git { git, prefix, head }, files, listed: all }))
}

/// **What the turn's commands made or changed** since `before`, only what
/// changed while one ran (`windows`). Blocking; capped.
pub fn after(before: &Before, windows: &[Window]) -> Result<Vec<Found>, Skip> {
    let folder = &before.folder;
    // Each file to look at, and what it is now.
    let mut now: Vec<(String, Seen)> = Vec::new();
    // Those Git had, clean, at the turn's start.
    let mut clean: HashSet<String> = HashSet::new();
    match &before.how {
        How::Walk => now = walk(folder, &left_out_dirs(), MOST_ENTRIES, WALK_TIME)?,
        How::Git { git, prefix, head } => {
            let listed = git_status(git, folder, prefix).map_err(|e| match e {
                GitErr::TimedOut => Skip::TooSlow(0),
                GitErr::TooBig => Skip::TooMany(MOST_ENTRIES),
                GitErr::Failed => Skip::Unreadable,
            })?;
            if listed.len() > MOST_ENTRIES {
                return Err(Skip::TooMany(listed.len()));
            }
            let mut rels: Vec<String> = before.files.values().map(|(r, _)| r.clone()).collect();
            rels.extend(listed.into_iter().map(|(_, r)| r));
            // Claude committed: the files between the two commits too.
            if let (Some(was), Some(is)) = (head.as_deref(), commit_at_head(git, folder)) {
                if was != is {
                    if let Ok(out) = git_out(git, folder, &["diff", "--name-only", "--no-renames", "-z", was, &is, "--", "."], None) {
                        rels.extend(split_z(&out).into_iter().filter_map(|p| p.strip_prefix(prefix.as_str()).map(str::to_string)).filter(|r| !r.is_empty()));
                    }
                }
            }
            let mut seen = HashSet::new();
            rels.retain(|r| seen.insert(key(r)));
            for rel in rels {
                if let Ok(meta) = std::fs::metadata(os_path(folder, &rel)) {
                    if meta.is_file() {
                        now.push((rel, Seen::of(&meta)));
                    }
                }
            }
            // Which of those Git had at the start, unchanged: every file it didn't list then (deleted ones included).
            if let Some(head) = head {
                let unseen: Vec<&str> = now.iter().map(|(r, _)| r.as_str()).filter(|r| !before.listed.contains(&key(r))).collect();
                clean = in_commit(git, folder, prefix, head, &unseen).into_keys().collect();
            }
        }
    }
    let mut found = Vec::new();
    for (rel, seen) in now {
        let k = key(&rel);
        let was = before.files.get(&k).map(|(_, s)| s);
        if let Some((new, is_clean)) = classify(was, &seen, clean.contains(&k), windows) {
            let path = backup::normal(&os_path(folder, &rel)).unwrap_or_else(|| os_path(folder, &rel));
            found.push(Found { rel, path, new, clean: is_clean, now: Stamp { bytes: seen.bytes, modified_ms: ms(seen.modified) } });
            if found.len() > MOST_FOUND {
                return Err(Skip::TooManyChanged(found.len()));
            }
        }
    }
    Ok(found)
}

/// **Whether a file is a command's doing**, and how: `(new, clean)` —
/// `was` it as the turn started (none: not seen then), `now` it as it is,
/// `tracked_clean` Git had it then, unchanged. Only a change while a command
/// ran counts (`windows`, [`SLACK`] either side): a new file by its birth, a
/// changed one by its last write. A file not seen before whose birth is
/// older than any command (it was there, out of the scan's sight) is changed,
/// never new; with no birth time, never new either (Undo would delete it).
/// Pure.
pub fn classify(was: Option<&Seen>, now: &Seen, tracked_clean: bool, windows: &[Window]) -> Option<(bool, bool)> {
    let written = during(now.modified, windows);
    match was {
        Some(b) if b.same(now) => None,
        Some(_) => written.then_some((false, false)),
        None if tracked_clean => written.then_some((false, true)),
        None if during(now.born, windows) => Some((true, false)),
        None => written.then_some((false, false)),
    }
}

/// `t` falls in one of `windows` ([`SLACK`] either side). Pure.
pub fn during(t: Option<SystemTime>, windows: &[Window]) -> bool {
    t.is_some_and(|t| windows.iter().any(|(a, b)| t + SLACK >= *a && t <= *b + SLACK))
}

fn ms(t: Option<SystemTime>) -> u64 {
    t.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64)
}

// ---- kept ---------------------------------------------------------------------------------------

/// **The turn's end**: what its commands made or changed ([`after`], with
/// `before` and `windows`; nothing when no command ran or the check was
/// skipped) noted in its backup folder `dir`, and every command's file
/// there as the turn left it (Undo's guard). What the turn's commands made
/// or changed, for its `by_command`, relative to the workspace as its
/// `files` say them. `edited`: the files Claude Code's own edit tools named
/// this turn (as its `files` say them), left to the backup hook — even with
/// its hooks off, an edit just before a command is the edit's, not the
/// command's. Blocking.
pub fn settle(before: Option<&Before>, windows: &[Window], dir: &Path, folder: &Path, edited: &[String]) -> Vec<CommandFile> {
    let mut manifest = match backup::read_manifest(dir) {
        Ok(m) => m,
        Err(_) => {
            tracing::warn!("Agents: a turn's backup list couldn't be read; its commands' files aren't noted");
            return Vec::new();
        }
    };
    let had = manifest.clone();
    if let (Some(before), false) = (before, windows.is_empty()) {
        let started = Instant::now();
        match after(before, windows) {
            Ok(found) => {
                let edited: HashSet<String> = edited.iter().map(|f| key(f)).collect();
                let noted = note(&mut manifest, before, &found, windows, dir, &edited);
                tracing::info!(git = before.git(), read = before.files_read(), found = found.len(), noted, ms = started.elapsed().as_millis() as u64, "Agents: what a turn's commands changed");
            }
            Err(why) => {
                let (words, n) = why.said();
                tracing::info!(git = before.git(), why = words, n, "Agents: what a turn's commands changed wasn't checked");
            }
        }
    }
    // Every command's file as the turn left it (a Continue's edits included).
    for e in manifest.files.iter_mut().filter(|e| e.command) {
        e.after = Stamp::now(&e.original);
    }
    if manifest != had {
        if let Err(e) = std::fs::create_dir_all(dir).and_then(|()| backup::write_manifest(dir, &manifest)) {
            tracing::warn!(kind = %e.kind(), "Agents: a turn's backup list couldn't be saved");
        }
    }
    command_files(&manifest, folder)
}

/// The manifest's command files, as a turn's `by_command`. Pure.
pub fn command_files(manifest: &Manifest, folder: &Path) -> Vec<CommandFile> {
    manifest
        .files
        .iter()
        .filter(|e| e.command)
        .map(|e| CommandFile { file: super::steps::shown_path(&e.original.to_string_lossy(), folder), new: e.state == State::Created, kept: e.state == State::Copied })
        .collect()
}

/// `found` into `manifest` (see [`settle`]); how many entries it added or
/// changed. Copies from Git go into `dir`.
fn note(manifest: &mut Manifest, before: &Before, found: &[Found], windows: &[Window], dir: &Path, edited: &HashSet<String>) -> usize {
    let mut changed = 0;
    let budget = Instant::now() + COPY_BUDGET;
    let first_command = windows.iter().map(|(a, _)| *a).min();
    for f in found {
        match manifest.files.iter().position(|e| backup::same_path(&e.original, &f.path)) {
            // Noted already (a Continue of the turn): as it is.
            Some(i) if manifest.files[i].command => {}
            // Claude Code's copy of a file a command had made: it's new.
            Some(i) if f.new && manifest.files[i].state != State::Created => {
                let e = &mut manifest.files[i];
                if e.state == State::Copied && backup::plain_name(&e.backup) {
                    let _ = std::fs::remove_file(dir.join(&e.backup));
                }
                e.state = State::Created;
                e.backup.clear();
                e.note.clear();
                e.was = None;
                e.command = true;
                e.after = Some(f.now);
                changed += 1;
            }
            Some(_) => {}
            // One of Claude Code's own edits (its hook didn't run): the hook's business, not a command's.
            None if edited.contains(&key(&f.rel)) => {}
            None => {
                let mut e = Entry { original: f.path.clone(), state: State::Created, backup: String::new(), bytes: f.now.bytes, tool: "command".into(), ts: backup::now_secs(), note: String::new(), command: true, was: None, after: Some(f.now) };
                if !f.new {
                    e.state = State::Failed;
                    e.note = backup::NO_COPY.into();
                    if f.clean {
                        git_backup(&mut e, before, &f.rel, manifest.files.len() + 1, dir, budget);
                    }
                }
                manifest.files.push(e);
                changed += 1;
            }
        }
    }
    // Claude Code's own copies made after a command ran: from before the turn only if the file hadn't changed by then.
    let Some(first) = first_command else { return changed };
    let first_secs = first.duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let late: Vec<usize> = manifest.files.iter().enumerate().filter(|(_, e)| !e.command && e.state == State::Copied && e.ts + 1 >= first_secs).map(|(i, _)| i).collect();
    if late.is_empty() {
        return changed;
    }
    let rels: Vec<(usize, String)> = late.iter().filter_map(|&i| backup::relative_to(&manifest.files[i].original, &before.folder).map(|r| (i, r.replace('\\', "/")))).collect();
    let clean: HashSet<String> = match &before.how {
        How::Git { git, prefix, head: Some(head) } => {
            let unseen: Vec<&str> = rels.iter().map(|(_, r)| r.as_str()).filter(|r| !before.listed.contains(&key(r))).collect();
            in_commit(git, &before.folder, prefix, head, &unseen).into_keys().collect()
        }
        _ => HashSet::new(),
    };
    for (i, rel) in rels {
        let k = key(&rel);
        if let Some((_, was)) = before.files.get(&k) {
            // Its copy isn't the file as the turn found it.
            let copied = manifest.files[i].was;
            if copied.is_some_and(|c| c.bytes != was.bytes || c.modified_ms != ms(was.modified)) {
                let e = &mut manifest.files[i];
                if backup::plain_name(&e.backup) {
                    let _ = std::fs::remove_file(dir.join(&e.backup));
                }
                e.state = State::Failed;
                e.backup.clear();
                e.note = COPIED_LATE.into();
                e.command = true;
                changed += 1;
            }
        } else if clean.contains(&k) {
            // Clean in Git at the start: Git's copy is the file as it was, whatever ran first.
            let n = i + 1;
            let mut e = manifest.files[i].clone();
            let old = e.backup.clone();
            git_backup(&mut e, before, &rel, n, dir, budget);
            if e.state == State::Copied {
                if old != e.backup && backup::plain_name(&old) {
                    let _ = std::fs::remove_file(dir.join(&old));
                }
                manifest.files[i] = e;
                changed += 1;
            }
        }
    }
    changed
}

/// `e` backed up from Git's copy at the turn's start (`rel` in the
/// workspace, its `n`th entry): `Copied`, `TooBig`, or left as it was.
fn git_backup(e: &mut Entry, before: &Before, rel: &str, n: usize, dir: &Path, budget: Instant) {
    let How::Git { git, prefix, head: Some(head) } = &before.how else { return };
    if Instant::now() > budget {
        tracing::debug!("Agents: out of time for Git's copies this turn");
        return;
    }
    let name = backup::backup_name(n, &e.original);
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let to = dir.join(&name);
    match git_copy(git, &before.folder, &format!("{head}:{prefix}{rel}"), &to, backup::MAX_BACKUP_BYTES) {
        Copied::Done(bytes) => {
            e.state = State::Copied;
            e.backup = name;
            e.bytes = bytes;
            e.note.clear();
        }
        Copied::TooBig => {
            e.state = State::TooBig;
            e.note = format!("over {} MB, not backed up", backup::MAX_BACKUP_BYTES / (1024 * 1024));
        }
        Copied::Failed => {}
    }
}

// ---- what Undo can do, in words ----------------------------------------------------------------

/// Up to 8 names, then "and N more".
fn names(list: &[&str]) -> String {
    let shown: Vec<&str> = list.iter().take(8).copied().collect();
    let more = list.len().saturating_sub(shown.len());
    if more > 0 {
        format!("{} and {more} more", shown.join(", "))
    } else {
        shown.join(", ")
    }
}

/// **The facts of a turn's files for the chat's model** (its report,
/// `agent_status`): `files` every file it changed, `by_command` those its
/// commands made or changed, `no_backups` Claude Code's hooks never ran —
/// so it never says a backup was kept when none was. Pure.
pub fn file_facts(files: &[String], by_command: &[CommandFile], no_backups: bool) -> Vec<String> {
    let cmd = |f: &str| by_command.iter().find(|c| c.file == f);
    let edited: Vec<&str> = files.iter().map(String::as_str).filter(|f| cmd(f).is_none()).collect();
    let made: Vec<&str> = by_command.iter().filter(|c| c.new).map(|c| c.file.as_str()).collect();
    let kept: Vec<&str> = by_command.iter().filter(|c| !c.new && c.kept).map(|c| c.file.as_str()).collect();
    let lost: Vec<&str> = by_command.iter().filter(|c| !c.new && !c.kept).map(|c| c.file.as_str()).collect();
    let mut lines = Vec::new();
    if !edited.is_empty() {
        let how = if no_backups { "no backups were made, so Undo can't put them back" } else { "backed up first" };
        lines.push(format!("Files it changed: {} ({how}).", names(&edited)));
    }
    if !made.is_empty() {
        lines.push(format!("Files its commands made: {} (new: Undo deletes them).", names(&made)));
    }
    if !kept.is_empty() {
        lines.push(format!("Files its commands changed: {} (backed up first, from Git).", names(&kept)));
    }
    if !lost.is_empty() {
        lines.push(format!("Files its commands changed with no backup: {} (Undo can't put them back).", names(&lost)));
    }
    lines
}

// ---- the walk -----------------------------------------------------------------------------------

/// **The files under `folder`**, relative and `/`-separated, each with what
/// was seen: build output and hidden folders (`mention::SKIPPED`, a name
/// starting with `.`, Windows' hidden and system folders), links and the
/// folders in `skip` left out, [`DEPTH`] deep. At most `most` entries, in
/// `time`.
pub fn walk(folder: &Path, skip: &[PathBuf], most: usize, time: Duration) -> Result<Vec<(String, Seen)>, Skip> {
    let until = Instant::now() + time;
    let mut out = Vec::new();
    let mut entries = 0usize;
    let mut stack = vec![(folder.to_path_buf(), 0usize)];
    if !folder.is_dir() {
        return Err(Skip::Unreadable);
    }
    while let Some((dir, depth)) = stack.pop() {
        let Ok(list) = std::fs::read_dir(&dir) else { continue };
        for entry in list.flatten() {
            entries += 1;
            if entries > most {
                return Err(Skip::TooMany(entries));
            }
            if entries.is_multiple_of(256) && Instant::now() > until {
                return Err(Skip::TooSlow(entries));
            }
            // (Links and junctions are neither: never followed.)
            let Ok(kind) = entry.file_type() else { continue };
            let name = entry.file_name().to_string_lossy().into_owned();
            if kind.is_dir() {
                let path = entry.path();
                if depth < DEPTH && !name.starts_with('.') && !super::mention::SKIPPED.contains(&name.as_str()) && !hidden(&entry) && !skip.iter().any(|s| backup::same_path(s, &path)) {
                    stack.push((path, depth + 1));
                }
            } else if kind.is_file() {
                let Ok(meta) = entry.metadata() else { continue };
                if let Ok(rel) = entry.path().strip_prefix(folder) {
                    out.push((rel.to_string_lossy().replace('\\', "/"), Seen::of(&meta)));
                }
            }
        }
    }
    Ok(out)
}

/// A folder Windows hides (hidden or system: `AppData`, `$RECYCLE.BIN`).
#[cfg(windows)]
fn hidden(entry: &std::fs::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    entry.metadata().is_ok_and(|m| m.file_attributes() & (0x2 | 0x4) != 0)
}
#[cfg(not(windows))]
fn hidden(_: &std::fs::DirEntry) -> bool {
    false
}

// ---- Git ----------------------------------------------------------------------------------------

/// **The person's own Git**: `git.exe` in an absolute folder on `PATH` (never
/// a script, never a relative folder), else Git for Windows' own
/// `cmd\git.exe` in `ProgramFiles`.
pub fn find_git() -> Option<Program> {
    find_git_in(std::env::var_os("PATH").as_deref(), std::env::var_os("ProgramFiles").as_deref())
}

/// [`find_git`] in `path` and `program_files`. Pure but for looking at files.
pub fn find_git_in(path: Option<&OsStr>, program_files: Option<&OsStr>) -> Option<Program> {
    let absolute: Vec<PathBuf> = path.map(|p| std::env::split_paths(p).map(|d| PathBuf::from(d.to_string_lossy().trim().trim_matches('"'))).filter(|d| d.is_absolute()).collect()).unwrap_or_default();
    let joined = std::env::join_paths(&absolute).ok();
    let exe = if cfg!(windows) { "git.exe" } else { "git" };
    let on_path = joined.as_deref().and_then(|p| cli::which(exe, p, Some(OsStr::new(".EXE"))));
    on_path.or_else(|| program_files.map(|pf| Path::new(pf).join("Git").join("cmd").join(exe)).filter(|p| cli::is_program(p))).map(Program::new)
}

/// Git's environment: Yap's, with nothing pointing it at another repository,
/// no optional locks (no index refresh), literal pathspecs, no prompts.
fn git_env() -> Vec<(OsString, OsString)> {
    const OFF: [&str; 6] = ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY", "GIT_NAMESPACE", "GIT_CEILING_DIRECTORIES"];
    const SET: [(&str, &str); 4] = [("GIT_OPTIONAL_LOCKS", "0"), ("GIT_TERMINAL_PROMPT", "0"), ("GIT_LITERAL_PATHSPECS", "1"), ("GIT_PAGER", "cat")];
    let mut env: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter(|(k, _)| {
            let k = k.to_string_lossy();
            !OFF.iter().chain(SET.iter().map(|(s, _)| s)).any(|s| k.eq_ignore_ascii_case(s))
        })
        .collect();
    env.extend(SET.iter().map(|(k, v)| (OsString::from(k), OsString::from(v))));
    env
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GitErr {
    Failed,
    TooBig,
    TimedOut,
}

fn git_command(git: &Program, dir: &Path, args: &[&str]) -> Option<std::process::Command> {
    let mut all = vec!["-c".to_string(), "core.fsmonitor=false".to_string()];
    all.extend(args.iter().map(|a| a.to_string()));
    let mut cmd = git.command(&all).ok()?;
    cmd.env_clear().envs(git_env()).current_dir(dir).stderr(Stdio::null());
    Some(cmd)
}

/// Waits for `child` until `until`: its exit code, or `None` (killed).
fn wait_until(child: &mut std::process::Child, until: Instant) -> Option<Option<i32>> {
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return Some(s.code()),
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// **`git <args>` in `dir`**: what it printed, when it ended well within
/// [`GIT_TIMEOUT`] and printed at most [`GIT_OUT_MAX`]; `input` fed to it.
fn git_out(git: &Program, dir: &Path, args: &[&str], input: Option<Vec<u8>>) -> Result<Vec<u8>, GitErr> {
    let mut cmd = git_command(git, dir, args).ok_or(GitErr::Failed)?;
    cmd.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped());
    let mut child = cmd.spawn().map_err(|_| GitErr::Failed)?;
    crate::shell::procs::end_with_yap(&child);
    if let (Some(mut stdin), Some(input)) = (child.stdin.take(), input) {
        std::thread::spawn(move || {
            let _ = stdin.write_all(&input);
        });
    }
    let mut stdout = child.stdout.take().ok_or(GitErr::Failed)?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = (&mut stdout).take(GIT_OUT_MAX as u64 + 1).read_to_end(&mut out);
        out
    });
    let code = wait_until(&mut child, Instant::now() + GIT_TIMEOUT);
    let out = reader.join().unwrap_or_default();
    match code {
        None => Err(GitErr::TimedOut),
        _ if out.len() > GIT_OUT_MAX => Err(GitErr::TooBig),
        Some(Some(0)) => Ok(out),
        _ => Err(GitErr::Failed),
    }
}

/// The commit at `HEAD` in `dir` (none: no commits yet).
fn commit_at_head(git: &Program, dir: &Path) -> Option<String> {
    let out = git_out(git, dir, &["rev-parse", "-q", "--verify", "HEAD^{commit}"], None).ok()?;
    let h = String::from_utf8_lossy(&out).trim().to_string();
    (!h.is_empty() && h.chars().all(|c| c.is_ascii_hexdigit())).then_some(h)
}

/// `git status` for the workspace's part of the repository: `(XY, path
/// relative to the workspace)` for each file changed, new or untracked.
fn git_status(git: &Program, dir: &Path, prefix: &str) -> Result<Vec<(String, String)>, GitErr> {
    let out = git_out(git, dir, &["status", "--porcelain=v1", "-z", "--untracked-files=all", "--no-renames", "--ignore-submodules=all", "--", "."], None)?;
    Ok(parse_status(&out, prefix))
}

/// The parts of a `-z` answer. Pure.
fn split_z(out: &[u8]) -> Vec<String> {
    out.split(|b| *b == 0).filter(|p| !p.is_empty()).map(|p| String::from_utf8_lossy(p).into_owned()).collect()
}

/// **`git status --porcelain=v1 -z --no-renames`'s answer**: `(XY, path)`,
/// the path relative to the workspace (`prefix`, its place in the
/// repository, taken off; one outside it left out), a nested repository's
/// folder (`dir/`) left out. Pure.
pub fn parse_status(out: &[u8], prefix: &str) -> Vec<(String, String)> {
    split_z(out)
        .into_iter()
        .filter_map(|e| {
            let xy = e.get(..2)?.to_string();
            let path = e.get(3..)?;
            if path.ends_with('/') || e.as_bytes().get(2) != Some(&b' ') {
                return None;
            }
            let rel = path.strip_prefix(prefix)?;
            (!rel.is_empty()).then(|| (xy, rel.to_string()))
        })
        .collect()
}

/// **Which of `rels` commit `head` has** (as files): each one's key and its
/// size there. One `git cat-file --batch-check`.
fn in_commit(git: &Program, dir: &Path, prefix: &str, head: &str, rels: &[&str]) -> HashMap<String, u64> {
    let asked: Vec<&str> = rels.iter().copied().filter(|r| !r.contains(['\n', '\r'])).collect();
    if asked.is_empty() {
        return HashMap::new();
    }
    let input: String = asked.iter().map(|r| format!("{head}:{prefix}{r}\n")).collect();
    let Ok(out) = git_out(git, dir, &["cat-file", "--batch-check"], Some(input.into_bytes())) else { return HashMap::new() };
    parse_batch_check(&String::from_utf8_lossy(&out), &asked)
}

/// `git cat-file --batch-check`'s answer for `asked` (a line each, in
/// order: `<id> blob <size>`, or `… missing`): the files there, by key, and
/// their sizes. Pure.
pub fn parse_batch_check(out: &str, asked: &[&str]) -> HashMap<String, u64> {
    out.lines()
        .zip(asked)
        .filter_map(|(line, rel)| {
            let mut parts = line.split(' ');
            let (_, kind, size) = (parts.next()?, parts.next()?, parts.next()?);
            (kind == "blob" && parts.next().is_none()).then(|| size.parse().ok().map(|s| (key(rel), s))).flatten()
        })
        .collect()
}

enum Copied {
    Done(u64),
    TooBig,
    Failed,
}

/// **Git's copy of `spec`** (`<commit>:<path>`) as a checkout writes it
/// (`git cat-file --filters`: line endings, filters), into `to`, at most
/// `max` bytes, within [`COPY_TIMEOUT`].
fn git_copy(git: &Program, dir: &Path, spec: &str, to: &Path, max: u64) -> Copied {
    let Some(mut cmd) = git_command(git, dir, &["cat-file", "--filters", spec]) else { return Copied::Failed };
    cmd.stdin(Stdio::null()).stdout(Stdio::piped());
    let Ok(mut child) = cmd.spawn() else { return Copied::Failed };
    crate::shell::procs::end_with_yap(&child);
    let Some(mut stdout) = child.stdout.take() else { return Copied::Failed };
    let Ok(mut file) = std::fs::File::create(to) else {
        let _ = child.kill();
        let _ = child.wait();
        return Copied::Failed;
    };
    let writer = std::thread::spawn(move || {
        let n = std::io::copy(&mut (&mut stdout).take(max + 1), &mut file);
        let _ = file.flush();
        n
    });
    let code = wait_until(&mut child, Instant::now() + COPY_TIMEOUT);
    let copied = writer.join().ok().and_then(Result::ok);
    match (code, copied) {
        (_, Some(n)) if n > max => {
            let _ = std::fs::remove_file(to);
            Copied::TooBig
        }
        (Some(Some(0)), Some(n)) => Copied::Done(n),
        _ => {
            let _ = std::fs::remove_file(to);
            Copied::Failed
        }
    }
}

#[cfg(test)]
mod tests;

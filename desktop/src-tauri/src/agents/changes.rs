//! **What a turn's commands changed** (15 Nov 2026). Claude Code's own edit
//! tools say which file they change; a command never does — a script
//! (exceljs, Python), `npm`, a formatter. The Lab found it on 15 Nov: an
//! exceljs script wrote `costs.xlsx`, and Outputs said "Nothing made yet". So
//! each turn reads the workspace before it and compares after, and lists
//! what its commands made or changed (Outputs, the summary, the file view,
//! Files changed, the report). Nothing is copied: as in the Claude app,
//! Rewind doesn't undo what a command did (Claude Code's checkpoints cover
//! its own edit tools only), and the person asks Claude to undo it.
//!
//! - **Before** ([`before`], as the turn starts, before Claude Code does):
//!   - in a Git repository (the person's own Git, and `git rev-parse` answers
//!     in the workspace, and the repository doesn't ignore the workspace
//!     itself): `git status --porcelain=v1 -z --untracked-files=all` for the
//!     workspace's part of it (`.gitignore` kept), each file it lists with
//!     its size and times, and the commit at `HEAD` — every other file is as
//!     Git has it there;
//!   - elsewhere (or when Git can't say), a walk ([`walk`]) that leaves out
//!     build output and hidden folders as the @ picker's does
//!     (`mention::SKIPPED`), links, and Yap's own data folder: every file, its
//!     size and times.
//!   - **Caps**: a walk stops at [`MOST_ENTRIES`] entries or [`WALK_TIME`];
//!     Git at [`GIT_TIMEOUT`] or [`MOST_ENTRIES`] files listed. Past them (a
//!     drive root like `E:\`, a home folder) the check is skipped for the
//!     turn ([`Skip`]), said in the log with counts only, never a stall.
//! - **After** ([`settle`], once its process is gone, only when a command
//!   ran): the same again, compared, and **only what changed while a command
//!   ran counts** ([`classify`]): a `Bash` or `PowerShell` call from its
//!   start to its result, one in the background to the turn's end
//!   ([`Window`]), with [`SLACK`] either side — by a new file's birth (which
//!   unpacking an archive doesn't set back) or a changed one's last write. So
//!   the person's own edits in the workspace meanwhile aren't the turn's, and
//!   neither is a file Claude Code's own edit tools named. With Git, `HEAD`
//!   moved (Claude committed) adds the files between the two commits, and a
//!   file Git had at the start is changed, never new. At most [`MOST_FOUND`]
//!   files; past that nothing is noted (logged).
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
/// How deep a walk goes.
const DEPTH: usize = 16;
/// The most Git may print for one answer.
const GIT_OUT_MAX: usize = 8 << 20;

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
    /// It wasn't there before the turn.
    pub new: bool,
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
    // A workspace the repository around it ignores (a scratch folder in an ignored one, a home folder's dotfiles
    // repository that ignores everything): Git sees none of its files, so a walk does.
    if !prefix.is_empty() && git_out(&git, folder, &["--no-literal-pathspecs", "check-ignore", "-q", "."], None).is_ok() {
        return Ok(None);
    }
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
        if let Some((new, _)) = classify(was, &seen, clean.contains(&k), windows) {
            found.push(Found { rel, new });
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

// ---- the turn's end -----------------------------------------------------------------------------

/// **The turn's end**: what its commands made or changed ([`after`], with
/// `before` and `windows`; nothing when no command ran or the check was
/// skipped), as its `by_command` says it: relative to the workspace as its
/// `files` say them. `edited`: the files Claude Code's own edit tools named
/// this turn (as its `files` say them), never a command's — an edit just
/// before a command is the edit's. Blocking.
pub fn settle(before: Option<&Before>, windows: &[Window], folder: &Path, edited: &[String]) -> Vec<CommandFile> {
    let Some(before) = before.filter(|_| !windows.is_empty()) else { return Vec::new() };
    let started = Instant::now();
    match after(before, windows) {
        Ok(found) => {
            let edited: HashSet<String> = edited.iter().map(|f| key(f)).collect();
            let out: Vec<CommandFile> = found
                .into_iter()
                .filter(|f| !edited.contains(&key(&f.rel)))
                .map(|f| CommandFile { file: super::steps::shown_path(&os_path(folder, &f.rel).to_string_lossy(), folder), new: f.new })
                .collect();
            tracing::info!(git = before.git(), read = before.files_read(), found = out.len(), ms = started.elapsed().as_millis() as u64, "Agents: what a turn's commands changed");
            out
        }
        Err(why) => {
            let (words, n) = why.said();
            tracing::info!(git = before.git(), why = words, n, "Agents: what a turn's commands changed wasn't checked");
            Vec::new()
        }
    }
}

// ---- in words ------------------------------------------------------------------------------------

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
/// commands made or changed. Nothing about backups or Undo: as in the
/// Claude app, the person asks Claude to undo a change, or rewinds. Pure.
pub fn file_facts(files: &[String], by_command: &[CommandFile]) -> Vec<String> {
    let cmd = |f: &str| by_command.iter().any(|c| c.file == f);
    let edited: Vec<&str> = files.iter().map(String::as_str).filter(|f| !cmd(f)).collect();
    let made: Vec<&str> = by_command.iter().filter(|c| c.new).map(|c| c.file.as_str()).collect();
    let changed: Vec<&str> = by_command.iter().filter(|c| !c.new).map(|c| c.file.as_str()).collect();
    let mut lines = Vec::new();
    if !edited.is_empty() {
        lines.push(format!("Files it changed: {}.", names(&edited)));
    }
    if !made.is_empty() {
        lines.push(format!("Files its commands made: {}.", names(&made)));
    }
    if !changed.is_empty() {
        lines.push(format!("Files its commands changed: {}.", names(&changed)));
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
                if depth < DEPTH && !name.starts_with('.') && !super::mention::SKIPPED.contains(&name.as_str()) && !hidden(&entry) && !skip.iter().any(|s| key(&s.to_string_lossy()) == key(&path.to_string_lossy())) {
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

#[cfg(test)]
mod tests;

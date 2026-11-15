//! **A Code chat's pull requests** (15 Nov 2026, after the Claude app's
//! right column): the summary panel's Changes — the pull requests of the
//! workspace's branch (a worktree's own branch too: `files::environment`),
//! newest first, each its title, `owner/repo #N`, its state (open, draft,
//! merged, closed) and its checks (passing, failing, running).
//!
//! - **Read with the person's own GitHub CLI** (`gh pr list --head <branch>
//!   --state all --json …`), in the workspace, on their own sign-in: Yap has
//!   no GitHub sign-in or token of its own and passes none. The request is
//!   `gh`'s, as Claude Code's are Claude Code's (privacy-receipts.md).
//! - **Started** with no window, stdin closed, prompts and update notices
//!   off, tied to Yap's kill-on-close job (`shell/procs.rs`), killed after
//!   20 s (`cli::run_captured_at`). Only a `gh.exe` from an absolute folder
//!   on `PATH` (or GitHub CLI's own install folder) is run, never one in the
//!   workspace.
//! - **When it can't** ([`Status`]): no git (the summary leaves the section
//!   out: Environment says "Not a git repository."), no branch, no remote
//!   (git's config has none, or `gh` knows none of them), no `gh`, `gh`
//!   signed out, or anything else: one quiet line each, never an error.
//! - **Asked again** when the summary opens and when a turn ends; kept 30 s
//!   per workspace and branch, unless a turn's end asks afresh.
//!
//! Debug builds: `YAP_GH_EXE` / `YAP_GH_ARGS_PREFIX` name a stand-in (as
//! Claude Code's `YAP_CLAUDE_EXE`), and a test run's `e2e_gh_script` sets
//! `yap.exe fake-gh <script.json>` ([`fake`]). A test run without one finds
//! no `gh`, so the suite never reaches the person's own.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use super::files;
use crate::ai::claude_code::cli::{self, Place, Program};

#[cfg(debug_assertions)]
pub mod fake;

/// How long `gh` may take.
const TIMEOUT: Duration = Duration::from_secs(20);
/// How long an answer stands for the same workspace and branch.
const KEEP_FOR: Duration = Duration::from_secs(30);
/// The most pull requests asked for.
const LIMIT: &str = "20";

/// Why there's no list, or `Ok`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    #[default]
    Ok,
    /// Not in a git repository (or the folder's gone): no section.
    NoGit,
    /// A detached HEAD.
    NoBranch,
    /// No remote, or none `gh` knows as GitHub.
    NoRemote,
    /// No GitHub CLI on this PC.
    NoGh,
    /// `gh` isn't signed in.
    SignedOut,
    /// Anything else (`gh` said why: not shown, logged by its length).
    Failed,
}

/// **One pull request**, as the summary draws it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pr {
    pub number: u64,
    pub title: String,
    /// `owner/repo`, from its link.
    pub repo: String,
    pub url: String,
    /// "open", "draft", "merged" or "closed".
    pub state: &'static str,
    /// "passing", "failing", "running", or "none" (no checks).
    pub checks: &'static str,
}

/// **The branch's pull requests**, or why there are none.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prs {
    pub status: Status,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub list: Vec<Pr>,
}

impl Prs {
    fn because(status: Status, branch: Option<String>) -> Prs {
        Prs { status, branch, list: Vec::new() }
    }
}

// ---- Reading what gh says -----------------------------------------------------------------------

/// `owner/repo` of a pull request's link (`https://github.com/o/r/pull/7`,
/// any host). Pure.
pub fn repo_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let mut parts = rest.split('/').skip(1);
    match (parts.next(), parts.next()) {
        (Some(o), Some(r)) if !o.is_empty() && !r.is_empty() => format!("{o}/{r}"),
        _ => String::new(),
    }
}

/// **Its checks, rolled up** (`statusCheckRollup`: check runs' `status` and
/// `conclusion`, commit statuses' `state`): any failed → "failing", else any
/// not finished → "running", else "passing"; none at all → "none". Pure.
pub fn checks_of(rollup: Option<&Value>) -> &'static str {
    let items = rollup.and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
    if items.is_empty() {
        return "none";
    }
    let (mut failing, mut running) = (false, false);
    for c in items {
        let up = |k: &str| c.get(k).and_then(Value::as_str).unwrap_or_default().to_ascii_uppercase();
        let (status, conclusion, state) = (up("status"), up("conclusion"), up("state"));
        if !state.is_empty() && c.get("status").is_none() {
            // A commit status: SUCCESS, FAILURE, ERROR, PENDING, EXPECTED.
            match state.as_str() {
                "FAILURE" | "ERROR" => failing = true,
                "PENDING" | "EXPECTED" => running = true,
                _ => {}
            }
            continue;
        }
        if !status.is_empty() && status != "COMPLETED" {
            running = true;
            continue;
        }
        match conclusion.as_str() {
            "FAILURE" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED" | "STARTUP_FAILURE" | "ERROR" => failing = true,
            "" => running = true,
            _ => {} // SUCCESS, NEUTRAL, SKIPPED, STALE
        }
    }
    if failing {
        "failing"
    } else if running {
        "running"
    } else {
        "passing"
    }
}

/// **`gh pr list --json number,title,state,isDraft,url,statusCheckRollup`'s
/// answer**, newest first (by number). `None` when it isn't that. Pure.
pub fn parse_list(stdout: &str) -> Option<Vec<Pr>> {
    let v: Value = serde_json::from_str(stdout.trim()).ok()?;
    let mut out: Vec<Pr> = v
        .as_array()?
        .iter()
        .filter_map(|p| {
            let number = p.get("number").and_then(Value::as_u64)?;
            let url = p.get("url").and_then(Value::as_str).unwrap_or_default().to_string();
            let state = match (p.get("state").and_then(Value::as_str).unwrap_or_default().to_ascii_uppercase().as_str(), p.get("isDraft").and_then(Value::as_bool).unwrap_or(false)) {
                ("MERGED", _) => "merged",
                ("CLOSED", _) => "closed",
                (_, true) => "draft",
                _ => "open",
            };
            Some(Pr { number, title: p.get("title").and_then(Value::as_str).unwrap_or_default().trim().to_string(), repo: repo_of(&url), url, state, checks: checks_of(p.get("statusCheckRollup")) })
        })
        .collect();
    out.sort_by_key(|p| std::cmp::Reverse(p.number));
    Some(out)
}

/// **What a failed `gh` said, as a [`Status`]** (its exit code and stderr):
/// no remote it knows, signed out (exit code 4, or "gh auth login"), not
/// a repository, else failed. Pure.
pub fn status_of_failure(code: Option<i32>, stderr: &str) -> Status {
    let s = stderr.to_ascii_lowercase();
    // (Its "no known GitHub host" line also names `gh auth login`: remotes first.)
    if s.contains("no git remotes") || s.contains("none of the git remotes") || s.contains("could not determine base repo") {
        Status::NoRemote
    } else if s.contains("not a git repository") {
        Status::NoGit
    } else if code == Some(4) || s.contains("gh auth login") || s.contains("not logged in") || s.contains("authentication required") {
        Status::SignedOut
    } else {
        Status::Failed
    }
}

/// **The remotes in a repository's git config** (`[remote "origin"]`'s
/// `url`s), from its git folder (a linked worktree's config is its
/// repository's: `commondir`). Pure but for reading the files.
pub fn remotes(git_dir: &Path) -> Vec<String> {
    let common = std::fs::read_to_string(git_dir.join("commondir")).ok().map(|c| {
        let c = PathBuf::from(c.trim());
        if c.is_absolute() {
            c
        } else {
            git_dir.join(c)
        }
    });
    let config = common.unwrap_or_else(|| git_dir.to_path_buf()).join("config");
    std::fs::read_to_string(config).map(|t| remotes_in(&t)).unwrap_or_default()
}

/// The `url`s of a git config's `[remote …]` sections. Pure.
pub fn remotes_in(config: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_remote = false;
    for line in config.lines() {
        let l = line.trim();
        if l.starts_with('[') {
            in_remote = l.to_ascii_lowercase().starts_with("[remote ");
            continue;
        }
        if !in_remote {
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            if k.trim().eq_ignore_ascii_case("url") && !v.trim().is_empty() {
                out.push(v.trim().to_string());
            }
        }
    }
    out
}

/// **`gh`'s arguments** for `branch`'s pull requests (the branch after `=`:
/// never read as a flag). Pure.
pub fn args(branch: &str) -> Vec<String> {
    vec!["pr".into(), "list".into(), format!("--head={branch}"), "--state".into(), "all".into(), "--limit".into(), LIMIT.into(), "--json".into(), "number,title,state,isDraft,url,statusCheckRollup".into()]
}

// ---- Finding gh ---------------------------------------------------------------------------------

/// **`gh.exe` on `path`**: only its absolute folders, only an `.exe` (never
/// a script a shell would read), else GitHub CLI's own install folder in
/// `program_files`. Pure but for looking at files.
pub fn find_in(path: Option<&OsStr>, program_files: Option<&OsStr>) -> Option<PathBuf> {
    let absolute: Vec<PathBuf> = path.map(|p| std::env::split_paths(p).map(|d| PathBuf::from(d.to_string_lossy().trim().trim_matches('"'))).filter(|d| d.is_absolute()).collect()).unwrap_or_default();
    let joined = std::env::join_paths(&absolute).ok();
    let exe = if cfg!(windows) { "gh.exe" } else { "gh" };
    let on_path = joined.as_deref().and_then(|p| cli::which(exe, p, Some(OsStr::new(".EXE"))));
    on_path.or_else(|| program_files.map(|pf| Path::new(pf).join("GitHub CLI").join(exe)).filter(|p| cli::is_program(p)))
}

/// A test run's stand-in (`e2e_gh_script`): wins over `YAP_GH_EXE`.
#[cfg(debug_assertions)]
static TEST_PROGRAM: Mutex<Option<Program>> = Mutex::new(None);

/// **A test run's `gh` from now on** (debug builds): `program`, or none.
#[cfg(debug_assertions)]
pub fn set_test_program(program: Option<Program>) {
    *TEST_PROGRAM.lock().unwrap_or_else(|e| e.into_inner()) = program;
    KEPT.lock().unwrap_or_else(|e| e.into_inner()).take();
}

#[cfg(debug_assertions)]
fn debug_override() -> Option<Program> {
    if let Some(p) = TEST_PROGRAM.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        return Some(p);
    }
    let exe = std::env::var_os("YAP_GH_EXE").filter(|v| !v.is_empty())?;
    let prefix = std::env::var("YAP_GH_ARGS_PREFIX").ok().and_then(|v| serde_json::from_str::<Vec<String>>(&v).ok()).unwrap_or_default();
    Some(Program::new(PathBuf::from(exe)).with_prefix(prefix))
}

/// The person's `gh`, if it's on this PC (a test run: only its stand-in).
fn find() -> Option<Program> {
    #[cfg(debug_assertions)]
    if let Some(p) = debug_override() {
        return Some(p);
    }
    if crate::shell::e2e::active() {
        return None;
    }
    find_in(std::env::var_os("PATH").as_deref(), std::env::var_os("ProgramFiles").as_deref()).map(Program::new)
}

/// `gh`'s environment: Yap's, with its prompts, pager, colours and update
/// notices off and git never asking for a password.
fn gh_env() -> Vec<(OsString, OsString)> {
    const SET: [(&str, &str); 7] = [("GH_PROMPT_DISABLED", "1"), ("GH_NO_UPDATE_NOTIFIER", "1"), ("GH_NO_EXTENSION_UPDATE_NOTIFIER", "1"), ("GH_SPINNER_DISABLED", "1"), ("GH_PAGER", ""), ("NO_COLOR", "1"), ("GIT_TERMINAL_PROMPT", "0")];
    let mut env: Vec<(OsString, OsString)> = std::env::vars_os().filter(|(k, _)| !SET.iter().any(|(s, _)| k.to_string_lossy().eq_ignore_ascii_case(s))).collect();
    env.extend(SET.iter().map(|(k, v)| (OsString::from(k), OsString::from(v))));
    env
}

// ---- Asking -------------------------------------------------------------------------------------

/// **The pull requests of the branch the workspace at `dir` is on**, with
/// `gh` (`None`: not installed). Blocking.
pub fn read(dir: &Path, gh: Option<&Program>) -> Prs {
    let here = files::environment("", &dir.display().to_string());
    if !here.exists || !here.repo {
        return Prs::because(Status::NoGit, None);
    }
    let Some(branch) = here.branch.clone().filter(|_| !here.detached) else { return Prs::because(Status::NoBranch, None) };
    let git_dir = files::git_dir_of(dir).map(|(g, _)| g);
    if git_dir.as_deref().is_some_and(|g| remotes(g).is_empty()) {
        return Prs::because(Status::NoRemote, Some(branch));
    }
    let Some(gh) = gh else { return Prs::because(Status::NoGh, Some(branch)) };
    let a = args(&branch);
    let a: Vec<&str> = a.iter().map(String::as_str).collect();
    let out = match cli::run_captured_at(gh, &a, Place { dir: dir.to_path_buf(), env: gh_env(), what: "GitHub's gh" }, TIMEOUT) {
        Ok(o) => o,
        Err(e) => {
            tracing::info!("Code chat: gh didn't run ({e})");
            return Prs::because(Status::Failed, Some(branch));
        }
    };
    if out.code != Some(0) {
        let status = status_of_failure(out.code, &out.stderr);
        tracing::info!(code = ?out.code, said = out.stderr.len(), status = ?status, "Code chat: gh couldn't list pull requests");
        return Prs::because(status, Some(branch));
    }
    match parse_list(&out.stdout) {
        Some(list) => Prs { status: Status::Ok, branch: Some(branch), list },
        None => {
            tracing::info!(said = out.stdout.len(), "Code chat: gh's pull requests couldn't be read");
            Prs::because(Status::Failed, Some(branch))
        }
    }
}

struct Kept {
    at: Instant,
    prs: Prs,
}

/// Answers by workspace folder and branch.
static KEPT: Mutex<Option<HashMap<(String, String), Kept>>> = Mutex::new(None);

/// **The summary's Changes**: the pull requests of the branch Code chat
/// `conversation`'s workspace is on (a new Code chat: the workspace at
/// `workspace`), kept 30 s unless `fresh` (a turn just ended). Never an
/// error: a [`Status`] says why there are none.
#[tauri::command]
pub async fn code_chat_pull_requests(conversation: Option<u64>, workspace: Option<String>, fresh: Option<bool>) -> Prs {
    let folder = conversation.and_then(|c| super::code_chat::folder_of(Some(c))).or_else(|| workspace.as_deref().and_then(super::code_chat::folder_at));
    let Some(folder) = folder else { return Prs::because(Status::NoGit, None) };
    tauri::async_runtime::spawn_blocking(move || {
        let dir = PathBuf::from(&folder.path);
        let branch = files::environment(&folder.name, &folder.path).branch.unwrap_or_default();
        let key = (super::workspaces::norm(&folder.path), branch);
        if !fresh.unwrap_or(false) {
            if let Some(k) = KEPT.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|m| m.get(&key)).filter(|k| k.at.elapsed() < KEEP_FOR) {
                return k.prs.clone();
            }
        }
        let prs = read(&dir, find().as_ref());
        tracing::info!(status = ?prs.status, prs = prs.list.len(), "Code chat: its branch's pull requests");
        KEPT.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert_with(HashMap::new).insert(key, Kept { at: Instant::now(), prs: prs.clone() });
        prs
    })
    .await
    .unwrap_or_else(|_| Prs::because(Status::Failed, None))
}

#[cfg(test)]
#[path = "prs/tests.rs"]
mod tests;

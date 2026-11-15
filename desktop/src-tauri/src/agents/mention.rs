//! **@-mention a file in a Code chat** (15 Nov 2026, as Claude Code's own
//! `@`): typing `@` in a Code chat's box opens a picker of its workspace's
//! files, best match first, keyboard first; the one picked goes in the
//! message as `@path/to/file` (quoted when it has a space, as Claude Code
//! writes it: the page's `chatMentions.js`), relative to the workspace,
//! which is how Claude Code reads a mention: the file, from the folder it
//! works in.
//!
//! - **Which files** ([`list`]): `git ls-files` (tracked, and untracked ones
//!   not ignored) when the workspace is a Git repository — what the repo
//!   itself counts as its files, `.gitignore` kept; else a walk of the folder
//!   that leaves out what's never source (`.git`, `node_modules`, `target`,
//!   build output…, [`SKIPPED`]). At most [`MOST`] files; read again after
//!   [`FRESH`].
//! - **The match** ([`score`], pure): the letters typed in order, in the
//!   file's name first (its start, a run of them, the start of a word scored
//!   up), then anywhere in its path; shorter paths first among equals.
//!
//! Nothing leaves the PC: the list is the page's, for the picker.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

/// The most files a workspace lists.
pub const MOST: usize = 30_000;
/// Matches the picker shows at most.
pub const SHOWN: usize = 50;
/// A workspace's list is read again after this long.
const FRESH: Duration = Duration::from_secs(20);
/// Folders a walk never goes into.
pub(crate) const SKIPPED: &[&str] = &[".git", "node_modules", "target", "dist", "build", "out", ".next", ".nuxt", ".svelte-kit", ".turbo", "__pycache__", ".venv", "venv", ".idea", ".vs", "bin", "obj", "coverage", ".cache"];

/// One file the picker offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    /// Relative to the workspace, `/` between parts ("src/main.rs").
    pub path: String,
    /// Its name ("main.rs").
    pub name: String,
    /// Its folder ("src"; "" at the top).
    pub dir: String,
}

impl Hit {
    fn of(path: &str) -> Hit {
        let (dir, name) = path.rsplit_once('/').map(|(d, n)| (d.to_string(), n.to_string())).unwrap_or((String::new(), path.to_string()));
        Hit { path: path.to_string(), name, dir }
    }
}

/// How well `path` matches `query` (`None`: not at all). Pure.
pub fn score(path: &str, query: &str) -> Option<i64> {
    let q: Vec<char> = query.trim().trim_start_matches('@').to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
    let lower = path.to_lowercase();
    if q.is_empty() {
        // Nothing typed yet: the top of the workspace first.
        return Some(-(path.matches('/').count() as i64) * 1000 - lower.len() as i64);
    }
    let qs: String = q.iter().collect();
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    let name_chars: Vec<char> = name.chars().collect();
    let path_chars: Vec<char> = lower.chars().collect();
    let in_name = run_score(&name_chars, &q).map(|s| s + 40 + if name.starts_with(&qs) { 60 } else if name.contains(&qs) { 25 } else { 0 });
    let in_path = run_score(&path_chars, &q).map(|s| s + if lower.contains(&qs) { 15 } else { 0 });
    let best = match (in_name, in_path) {
        (Some(a), Some(b)) => a.max(b),
        (a, b) => a.or(b)?,
    };
    Some(best * 1000 - lower.len() as i64)
}

/// The letters of `q` in order in `hay`, scored: one a letter, more for a
/// run and for a word's start. Pure.
fn run_score(hay: &[char], q: &[char]) -> Option<i64> {
    let mut score = 0i64;
    let mut at = 0usize;
    let mut prev: Option<usize> = None;
    for &c in q {
        let found = (at..hay.len()).find(|&i| hay[i] == c)?;
        score += 1;
        if prev.is_some_and(|p| found == p + 1) {
            score += 6;
        }
        if found == 0 || matches!(hay[found - 1], '/' | '\\' | '_' | '-' | '.' | ' ') {
            score += 8;
        }
        prev = Some(found);
        at = found + 1;
    }
    Some(score)
}

/// The best [`SHOWN`] of `paths` for `query`, best first. Pure.
pub fn best(paths: &[String], query: &str) -> Vec<Hit> {
    let mut scored: Vec<(i64, &String)> = paths.iter().filter_map(|p| score(p, query).map(|s| (s, p))).collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    scored.into_iter().take(SHOWN).map(|(_, p)| Hit::of(p)).collect()
}

// ---- a workspace's files -----------------------------------------------------------------------

/// Each workspace's files, and when they were read.
type Lists = HashMap<PathBuf, (Instant, std::sync::Arc<Vec<String>>)>;
static CACHE: Mutex<Option<Lists>> = Mutex::new(None);

/// **The files of the workspace at `folder`**, relative and `/`-separated,
/// sorted; read again after [`FRESH`]. Blocking.
pub fn list(folder: &Path) -> std::sync::Arc<Vec<String>> {
    if let Some((at, files)) = CACHE.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(HashMap::new).get(folder) {
        if at.elapsed() < FRESH {
            return files.clone();
        }
    }
    let mut files = from_git(folder).unwrap_or_else(|| walk(folder));
    files.sort();
    files.dedup();
    files.truncate(MOST);
    let files = std::sync::Arc::new(files);
    CACHE.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(HashMap::new).insert(folder.to_path_buf(), (Instant::now(), files.clone()));
    files
}

/// `git ls-files` in `folder` (tracked, and untracked not ignored); `None`
/// when it isn't a repository or Git isn't here.
fn from_git(folder: &Path) -> Option<Vec<String>> {
    if !folder.join(".git").exists() {
        return None;
    }
    let mut cmd = std::process::Command::new("git");
    cmd.args(["ls-files", "--cached", "--others", "--exclude-standard", "-z"]).current_dir(folder).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().ok().filter(|o| o.status.success())?;
    let files: Vec<String> = out.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()).map(|p| String::from_utf8_lossy(p).replace('\\', "/")).take(MOST).collect();
    (!files.is_empty()).then_some(files)
}

/// The folder's files, walked: [`SKIPPED`] folders and hidden ones left out,
/// 12 folders deep at most, [`MOST`] files at most.
fn walk(folder: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![(folder.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            if out.len() >= MOST {
                return out;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_dir() {
                if depth < 12 && !SKIPPED.contains(&name.as_str()) && !name.starts_with('.') {
                    stack.push((entry.path(), depth + 1));
                }
            } else if kind.is_file() {
                if let Ok(rel) = entry.path().strip_prefix(folder) {
                    out.push(rel.to_string_lossy().replace('\\', "/"));
                }
            }
        }
    }
    out
}

/// **The @ picker's files** for the Code chat `conversation`: the best
/// matches for `query` among its workspace's files. None in a chat that
/// isn't a Code chat, or whose folder is gone.
#[tauri::command]
pub async fn agent_mention_files(conversation: Option<u64>, folder: Option<String>, query: String) -> Result<Vec<Hit>, String> {
    // (A new chat made a Code chat before its first message has no conversation yet: its workspace by name.)
    let found = crate::agents::code_chat::folder_of(conversation).or_else(|| folder.as_deref().and_then(crate::agents::code_chat::find));
    let Some(found) = found else { return Ok(Vec::new()) };
    let path = PathBuf::from(&found.path);
    if !path.is_dir() {
        return Ok(Vec::new());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let files = list(&path);
        best(&files, &query)
    })
    .await
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_name_beats_a_path_and_a_start_beats_a_middle() {
        let files = paths(&["docs/garden/readme.md", "src/beds.rs", "src/garden/beds_test.rs", "src/lib/flowerbeds.rs", "beds/notes.txt"]);
        let hits: Vec<String> = best(&files, "beds").into_iter().map(|h| h.path).collect();
        assert_eq!(hits[0], "src/beds.rs");
        assert_eq!(hits[1], "src/garden/beds_test.rs");
        assert!(hits.iter().position(|p| p == "src/lib/flowerbeds.rs") < hits.iter().position(|p| p == "beds/notes.txt"));
        assert!(!hits.contains(&"docs/garden/readme.md".to_string()));
    }

    #[test]
    fn letters_in_order_find_a_file_fuzzily() {
        let files = paths(&["src/lib/views/chat/Composer.svelte", "src/lib/views/chat/ChatView.svelte", "README.md"]);
        assert_eq!(best(&files, "cmpsr")[0].path, "src/lib/views/chat/Composer.svelte");
        assert_eq!(best(&files, "chat/view")[0].path, "src/lib/views/chat/ChatView.svelte");
        assert!(best(&files, "zzz").is_empty());
        // Case doesn't matter; an @ typed again is left out.
        assert_eq!(best(&files, "@readme")[0].path, "README.md");
    }

    #[test]
    fn nothing_typed_lists_the_top_first() {
        let files = paths(&["src/a/deep.rs", "plan.md", "src/main.rs"]);
        let hits: Vec<String> = best(&files, "").into_iter().map(|h| h.path).collect();
        assert_eq!(hits, ["plan.md", "src/main.rs", "src/a/deep.rs"]);
    }

    #[test]
    fn a_hit_knows_its_folder() {
        assert_eq!(Hit::of("src/main.rs"), Hit { path: "src/main.rs".into(), name: "main.rs".into(), dir: "src".into() });
        assert_eq!(Hit::of("plan.md").dir, "");
    }

    #[test]
    fn a_walk_leaves_build_output_and_hidden_folders_out() {
        let root = std::env::temp_dir().join(format!("yap-mention-{}", crate::chat::attachments::new_id()));
        for f in ["src/main.rs", "node_modules/x/index.js", "target/debug/out.txt", ".hidden/secret.txt", "plan.md"] {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "x").unwrap();
        }
        let mut files = walk(&root);
        files.sort();
        assert_eq!(files, ["plan.md", "src/main.rs"]);
        let _ = std::fs::remove_dir_all(&root);
    }
}

//! **Claude Code's own copies of the sessions it ran: its transcripts.**
//!
//! Claude Code saves every session it runs (unless it was started with
//! `--no-session-persistence`) in its own config folder ([`config_dir`]:
//! `CLAUDE_CONFIG_DIR`, else `~/.claude`), as
//! `projects/<the working folder, encoded>/<session id>.jsonl`, with the
//! session's own files (a subagent's transcript, a big tool result) in
//! `<session id>/` beside it. A plan chat's session (`claude_plan`) runs in
//! Yap's own `<data>/claude/chat/` — a Code chat's in its workspace, so its
//! copy lands in that folder's projects folder, beside the person's own
//! and their jobs' sessions — outside Yap; [`remove`] deletes it when Yap
//! deletes the chat (`claude_plan/sessions.rs` keeps which folder each ran
//! in).
//!
//! - **The folder's name** ([`project_dir_name`]), as Claude Code 2.1.296's
//!   own code makes it (read from its bundled source on 7 Nov 2026, and the
//!   folders it names, `E:\Projects\garden-planner` →
//!   `E--Projects-garden-planner`): every UTF-16 unit of the path that isn't
//!   an ASCII letter or digit becomes `-`; a name longer than 200 is cut to
//!   200, then `-` and a hash (the absolute value of the 32-bit
//!   `h = 31·h + unit` hash of the path, in base 36).
//! - **Only Yap's own**: [`remove`] deletes only files named with the session
//!   ids Yap gave it (UUIDs no other session has), only in the folder for
//!   the working folder they ran in — matched without case, since Windows
//!   may give the drive letter either way — or in one whose name ends as that
//!   folder's last two parts do (`…-claude-chat`: the same folder reached
//!   another way, such as a junction). Never another session's, never
//!   another project's: in a Code chat's workspace the person's own
//!   sessions and their jobs' stay (a job's transcript is theirs, as if
//!   they'd run Claude Code themselves).
//! - **Test runs** never look in the person's own `~/.claude`: only a
//!   `CLAUDE_CONFIG_DIR` the test set.
//!
//! The log has counts, never a path or an id.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The longest a project folder's name is before Claude Code cuts it and
/// adds a hash.
const MAX_NAME: usize = 200;

/// **Claude Code's config folder**: `CLAUDE_CONFIG_DIR` (the runs' own
/// environment keeps it: `cli::clean_env`), else `~/.claude`. A test run
/// (`YAP_E2E=1`) only ever the one it set; unit tests one of their own.
pub fn config_dir() -> Option<PathBuf> {
    #[cfg(test)]
    {
        TEST_CONFIG.with(|c| c.borrow().clone())
    }
    #[cfg(not(test))]
    {
        let var = std::env::var_os("CLAUDE_CONFIG_DIR");
        if crate::shell::e2e::active() {
            return config_dir_from(var, None);
        }
        config_dir_from(var, dirs::home_dir())
    }
}

#[cfg(test)]
thread_local! {
    /// The config folder a unit test made (none: nothing is looked at).
    pub static TEST_CONFIG: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// [`config_dir`] from `CLAUDE_CONFIG_DIR`'s value and the home folder. Pure.
pub fn config_dir_from(var: Option<OsString>, home: Option<PathBuf>) -> Option<PathBuf> {
    match var.filter(|v| !v.is_empty()) {
        Some(v) => Some(PathBuf::from(v)),
        None => home.map(|h| h.join(".claude")),
    }
}

/// Every UTF-16 unit that isn't an ASCII letter or digit as `-`. Pure.
fn encode(s: &str) -> String {
    s.encode_utf16().map(|u| if u < 0x80 && (u as u8).is_ascii_alphanumeric() { u as u8 as char } else { '-' }).collect()
}

/// The 32-bit string hash Claude Code adds to a long name (`h = 31·h +
/// unit`, wrapping, over UTF-16 units). Pure.
fn string_hash(s: &str) -> i32 {
    s.encode_utf16().fold(0i32, |h, u| (h << 5).wrapping_sub(h).wrapping_add(i32::from(u)))
}

/// `n` in base 36, lowercase. Pure.
fn base36(mut n: u64) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut out = Vec::new();
    loop {
        out.push(DIGITS[(n % 36) as usize]);
        n /= 36;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// **The folder under `projects/` Claude Code keeps a working folder's
/// sessions in**, as it names it. Pure.
pub fn project_dir_name(cwd: &str) -> String {
    let name = encode(cwd);
    if name.len() <= MAX_NAME {
        return name;
    }
    format!("{}-{}", &name[..MAX_NAME], base36(i64::from(string_hash(cwd)).unsigned_abs()))
}

/// A session id as Claude Code makes and Yap gives them: a UUID (so never a
/// path). Pure.
pub fn is_session_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('-').collect();
    parts.iter().map(|p| p.len()).eq([8, 4, 4, 4, 12]) && parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// `-` and the last two parts of `cwd`, encoded (`-claude-chat`): how the
/// name of any folder Claude Code files it under ends, however the path
/// before them came. `None` with fewer than two.
fn tail(cwd: &Path) -> Option<String> {
    let parts: Vec<String> = cwd.components().rev().take(2).filter_map(|c| match c {
        std::path::Component::Normal(p) => Some(p.to_string_lossy().into_owned()),
        _ => None,
    }).collect();
    (parts.len() == 2).then(|| format!("-{}-{}", encode(&parts[1]), encode(&parts[0])))
}

/// Whether `name` (a folder under `projects/`) is where Claude Code files
/// sessions run in `cwd`. Pure.
fn files_for(name: &str, cwd: &Path) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower == project_dir_name(&cwd.to_string_lossy()).to_ascii_lowercase() {
        return true;
    }
    tail(cwd).is_some_and(|t| {
        let t = t.to_ascii_lowercase();
        lower.len() > t.len() && lower.ends_with(&t)
    })
}

/// **Delete the transcripts of `sessions`** that Claude Code (its config
/// folder `config`) keeps for sessions run in `cwd`: each `<id>.jsonl` and
/// its `<id>/` folder, in the folders [`files_for`] names. Nothing else is
/// touched; ids that aren't UUIDs are skipped; links are never followed.
/// How many transcripts went.
pub fn remove(config: &Path, cwd: &Path, sessions: &[String]) -> usize {
    let ids: Vec<&str> = sessions.iter().map(String::as_str).filter(|id| is_session_id(id)).collect();
    if ids.is_empty() {
        return 0;
    }
    let Ok(entries) = std::fs::read_dir(config.join("projects")) else { return 0 };
    let mut gone = 0;
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else { continue };
        if !kind.is_dir() || kind.is_symlink() || !files_for(&entry.file_name().to_string_lossy(), cwd) {
            continue;
        }
        let dir = entry.path();
        for id in &ids {
            let transcript = dir.join(format!("{id}.jsonl"));
            if std::fs::symlink_metadata(&transcript).is_ok_and(|m| m.is_file()) && std::fs::remove_file(&transcript).is_ok() {
                gone += 1;
            }
            let own = dir.join(id);
            if std::fs::symlink_metadata(&own).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink()) {
                let _ = std::fs::remove_dir_all(&own);
            }
        }
    }
    gone
}

/// **Is session `id`'s transcript kept** (`config`: Claude Code's config
/// folder) for a session run in `cwd`: `Some(true)` it's there (it can be
/// resumed), `Some(false)` Claude Code keeps sessions here and this one's
/// gone (its clean-up took it), `None` Yap can't tell (no `projects/`
/// folder, an id that isn't a UUID). Only looks. Pure but for the reads.
pub fn kept(config: &Path, cwd: &Path, id: &str) -> Option<bool> {
    if !is_session_id(id) {
        return None;
    }
    let entries = std::fs::read_dir(config.join("projects")).ok()?;
    let found = entries.flatten().any(|entry| {
        entry.file_type().is_ok_and(|k| k.is_dir() && !k.is_symlink())
            && files_for(&entry.file_name().to_string_lossy(), cwd)
            && std::fs::symlink_metadata(entry.path().join(format!("{id}.jsonl"))).is_ok_and(|m| m.is_file())
    });
    Some(found)
}

/// [`kept`] in Claude Code's own config folder ([`config_dir`]): `None`
/// when there's none to look in.
pub fn session_kept(cwd: &Path, id: &str) -> Option<bool> {
    kept(&config_dir()?, cwd, id)
}

/// [`remove`] in Claude Code's own config folder ([`config_dir`]), said in
/// the log by count.
pub fn remove_for(cwd: &Path, sessions: &[String]) -> usize {
    let Some(config) = config_dir() else { return 0 };
    let gone = remove(&config, cwd, sessions);
    if gone > 0 {
        tracing::info!(gone, "claude code: transcripts of deleted chats removed from its own folder");
    }
    gone
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folders_are_named_as_claude_code_names_them() {
        // As the folders Claude Code made on Windows read.
        assert_eq!(project_dir_name(r"E:\Projects\garden-planner"), "E--Projects-garden-planner");
        assert_eq!(project_dir_name(r"C:\Users\imogen\AppData\Roaming\yap\claude\chat"), "C--Users-imogen-AppData-Roaming-yap-claude-chat");
        assert_eq!(project_dir_name("/home/imogen/my_notes.v2"), "-home-imogen-my-notes-v2");
        // A letter outside ASCII is one `-` per UTF-16 unit (a surrogate pair two).
        assert_eq!(project_dir_name(r"D:\Café\🎙"), "D--Caf----");
        // The hash, as Java's String.hashCode (Claude Code's is the same sum).
        assert_eq!(string_hash("hello"), 99_162_322);
        assert_eq!(string_hash(""), 0);
        assert_eq!(string_hash("polygenelubricants"), i32::MIN);
        assert_eq!(base36(0), "0");
        assert_eq!(base36(35), "z");
        assert_eq!(base36(36), "10");
        assert_eq!(base36(2_147_483_648), "zik0zk");
        // Over 200: cut, then its hash.
        let long = format!(r"C:\{}\claude\chat", "deep".repeat(60));
        let name = project_dir_name(&long);
        let cut = encode(&long)[..MAX_NAME].to_string();
        assert_eq!(name, format!("{cut}-{}", base36(i64::from(string_hash(&long)).unsigned_abs())));
        assert!(name.len() > MAX_NAME && name.len() < MAX_NAME + 9);
    }

    #[test]
    fn session_ids_are_uuids_only() {
        assert!(is_session_id("4d7c1d2e-8a51-4c3b-9f0e-2b6a7e1c5d90"));
        assert!(is_session_id("4D7C1D2E-8A51-4C3B-9F0E-2B6A7E1C5D90"));
        for not in ["", "s-1", "..", r"..\..\x", "4d7c1d2e-8a51-4c3b-9f0e-2b6a7e1c5d9", "4d7c1d2e-8a51-4c3b-9f0e-2b6a7e1c5d90.jsonl", "4d7c1d2e/8a51-4c3b-9f0e-2b6a7e1c5d90", "zzzzzzzz-8a51-4c3b-9f0e-2b6a7e1c5d90"] {
            assert!(!is_session_id(not), "{not}");
        }
    }

    #[test]
    fn the_config_folder_is_claude_config_dir_else_home() {
        assert_eq!(config_dir_from(Some(OsString::from(r"D:\cc")), Some(PathBuf::from(r"C:\Users\imogen"))), Some(PathBuf::from(r"D:\cc")));
        assert_eq!(config_dir_from(Some(OsString::new()), Some(PathBuf::from(r"C:\Users\imogen"))), Some(PathBuf::from(r"C:\Users\imogen\.claude")));
        assert_eq!(config_dir_from(None, Some(PathBuf::from(r"C:\Users\imogen"))), Some(PathBuf::from(r"C:\Users\imogen\.claude")));
        assert_eq!(config_dir_from(None, None), None, "a test run with no CLAUDE_CONFIG_DIR: nothing");
        assert_eq!(config_dir(), None, "a unit test looks at no config folder unless it made one");
    }

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yap-cc-transcripts-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn put(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn a_deleted_chats_transcript_goes_and_nothing_else_does() {
        // A fake CLAUDE_CONFIG_DIR, as Claude Code lays it out.
        let config = temp("config");
        let cwd = PathBuf::from(r"C:\Users\imogen\AppData\Roaming\yap\claude\chat");
        let gone_id = "4d7c1d2e-8a51-4c3b-9f0e-2b6a7e1c5d90";
        let kept_id = "9f0e2b6a-7e1c-4d90-8a51-4c3b4d7c1d2e";
        let projects = config.join("projects");
        let mine = projects.join(project_dir_name(&cwd.to_string_lossy()));
        // The deleted chat's transcript and its session's own files.
        put(&mine.join(format!("{gone_id}.jsonl")), "{\"type\":\"user\"}\n");
        put(&mine.join(gone_id).join("subagents").join("agent-1.jsonl"), "{}\n");
        // Another chat's, still in Yap.
        put(&mine.join(format!("{kept_id}.jsonl")), "{}\n");
        // The same folder reached another way (a junction): its name ends alike.
        let other_way = projects.join("D--Roaming-yap-claude-chat");
        put(&other_way.join(format!("{gone_id}.jsonl")), "{}\n");
        // The drive letter the other way round.
        let lower = projects.join("c--Users-imogen-AppData-Roaming-yap-claude-chat-x");
        put(&lower.join(format!("{gone_id}.jsonl")), "not one of Yap's: the name doesn't end alike\n");
        // The person's own project: their job's session, even one with the same file name.
        let theirs = projects.join("E--Projects-garden-planner");
        put(&theirs.join(format!("{gone_id}.jsonl")), "{}\n");
        put(&theirs.join("1b2c3d4e-0000-4000-8000-000000000001.jsonl"), "{}\n");
        // Other things in the config folder.
        put(&config.join("settings.json"), "{}");
        put(&projects.join("a-file.jsonl"), "{}");

        let removed = remove(&config, &cwd, &[gone_id.to_string(), "../../settings".to_string(), "s-1".into()]);
        assert_eq!(removed, 2, "the chat's transcript, here and where the same folder files it");
        assert!(!mine.join(format!("{gone_id}.jsonl")).exists());
        assert!(!mine.join(gone_id).exists(), "its session's own files too");
        assert!(!other_way.join(format!("{gone_id}.jsonl")).exists());
        assert!(mine.join(format!("{kept_id}.jsonl")).exists(), "another chat's stays");
        assert!(lower.join(format!("{gone_id}.jsonl")).exists());
        assert!(theirs.join(format!("{gone_id}.jsonl")).exists(), "another project's is never touched");
        assert!(theirs.join("1b2c3d4e-0000-4000-8000-000000000001.jsonl").exists());
        assert!(config.join("settings.json").exists() && projects.join("a-file.jsonl").exists());
        // The drive letter's case doesn't matter.
        let upper = projects.join(project_dir_name(r"C:\Users\imogen\AppData\Roaming\yap\claude\chat").replacen('C', "c", 1));
        assert!(files_for(&upper.file_name().unwrap().to_string_lossy(), &cwd));
        // Nothing to do: no ids, no folder.
        assert_eq!(remove(&config, &cwd, &[]), 0);
        assert_eq!(remove(&config.join("missing"), &cwd, &[kept_id.to_string()]), 0);
        // remove_for: in a unit test, the config folder it made.
        TEST_CONFIG.with(|c| *c.borrow_mut() = Some(config.clone()));
        assert_eq!(remove_for(&cwd, &[kept_id.to_string()]), 1);
        assert!(!mine.join(format!("{kept_id}.jsonl")).exists());
        TEST_CONFIG.with(|c| *c.borrow_mut() = None);
        let _ = std::fs::remove_dir_all(&config);
    }
}

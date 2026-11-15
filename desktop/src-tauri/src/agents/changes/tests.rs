//! What a turn's commands changed: the compare, the caps, a folder with no
//! Git and a Git repository (when Git is on this PC), what Undo then does.
//! Every workspace is a fresh folder in the temp folder; fictional files.

use super::*;
use std::time::UNIX_EPOCH;

fn scratch(what: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("yap-changes-{what}-{}", crate::chat::attachments::new_id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, rel: &str, text: &str) {
    let p = os_path(dir, rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn read(dir: &Path, rel: &str) -> String {
    std::fs::read_to_string(os_path(dir, rel)).unwrap()
}

/// A command running now, from a moment ago to a minute from now (writing many files can take a while).
fn running() -> Vec<Window> {
    let now = SystemTime::now();
    vec![(now - Duration::from_millis(200), now + Duration::from_secs(60))]
}

/// Sets `rel`'s last write an hour back: changed, but by nobody's command.
fn written_long_ago(dir: &Path, rel: &str) {
    let f = std::fs::OpenOptions::new().write(true).open(os_path(dir, rel)).unwrap();
    f.set_modified(SystemTime::now() - Duration::from_secs(3600)).unwrap();
}

/// The workspace at `dir` as a walk found it (`files`).
fn walked(dir: &Path, files: Vec<(String, Seen)>) -> Before {
    Before { folder: dir.to_path_buf(), how: How::Walk, files: files.into_iter().map(|(r, s)| (key(&r), (r, s))).collect(), listed: HashSet::new() }
}

fn rels(found: &[Found]) -> Vec<(String, bool, bool)> {
    let mut v: Vec<(String, bool, bool)> = found.iter().map(|f| (f.rel.clone(), f.new, f.clean)).collect();
    v.sort();
    v
}

#[test]
fn only_a_change_while_a_command_ran_counts() {
    let t = UNIX_EPOCH + Duration::from_secs(1_800_000_000);
    let s = |secs: u64| Some(t + Duration::from_secs(secs));
    let windows = vec![(t + Duration::from_secs(100), t + Duration::from_secs(110))];
    let seen = |bytes, modified, born| Seen { bytes, modified, born };
    let before = seen(10, s(0), s(0));
    // Unchanged: nothing.
    assert_eq!(classify(Some(&before), &before, false, &windows), None);
    // Changed while the command ran: changed, no copy (not clean in Git).
    assert_eq!(classify(Some(&before), &seen(12, s(105), s(0)), false, &windows), Some((false, false)));
    // Changed, but long after the command: the person's, not the turn's.
    assert_eq!(classify(Some(&before), &seen(12, s(300), s(0)), false, &windows), None);
    // Just outside the run, within the slack either side: the command's.
    assert_eq!(classify(Some(&before), &seen(12, s(99), s(0)), false, &windows), Some((false, false)));
    assert_eq!(classify(Some(&before), &seen(12, s(111), s(0)), false, &windows), Some((false, false)));
    assert_eq!(classify(Some(&before), &seen(12, s(97), s(0)), false, &windows), None);
    // Clean in Git at the start, changed by the command: Git has it.
    assert_eq!(classify(None, &seen(12, s(105), s(0)), true, &windows), Some((false, true)));
    assert_eq!(classify(None, &seen(12, s(300), s(0)), true, &windows), None);
    // Not there before, made while it ran: new (even with an old time, as an archive unpacked leaves it).
    assert_eq!(classify(None, &seen(5, s(0), s(104)), false, &windows), Some((true, false)));
    // Made by the person meanwhile: not the turn's.
    assert_eq!(classify(None, &seen(5, s(200), s(200)), false, &windows), None);
    // Unseen before but older than any command (it was there): changed, never new.
    assert_eq!(classify(None, &seen(5, s(105), s(0)), false, &windows), Some((false, false)));
    // No birth time: never new (Undo would delete it).
    assert_eq!(classify(None, &seen(5, s(105), None), false, &windows), Some((false, false)));
    // No command ran: nothing at all.
    assert_eq!(classify(Some(&before), &seen(12, s(105), s(0)), false, &[]), None);
    assert!(!during(None, &windows));
}

#[test]
fn gits_answers_are_read_relative_to_the_workspace() {
    let out = b" M src/a.js\0?? src/new file.txt\0A  src/added.rs\0?? src/nested/\0 D src/gone.md\0?? other/x.txt\0MM src/both.ts\0";
    let got = parse_status(out, "src/");
    assert_eq!(
        got,
        [(" M", "a.js"), ("??", "new file.txt"), ("A ", "added.rs"), (" D", "gone.md"), ("MM", "both.ts")].map(|(a, b)| (a.to_string(), b.to_string())).to_vec(),
        "a nested repository's folder and a path outside the workspace left out"
    );
    assert_eq!(parse_status(b" M a.js\0", "").len(), 1);
    assert!(parse_status(b"garbage\0\0", "").is_empty());
    let asked = ["a.js", "gone.md", "b.bin"];
    let found = parse_batch_check("1111 blob 42\nabc:gone.md missing\n2222 blob 7\n", &asked);
    assert_eq!(found.get(&key("a.js")), Some(&42));
    assert_eq!(found.get(&key("gone.md")), None);
    assert_eq!(found.get(&key("b.bin")), Some(&7));
    // A folder at that path isn't a file.
    assert!(parse_batch_check("3333 tree 90\n", &["src"]).is_empty());
}

#[test]
fn the_facts_never_claim_a_backup_that_isnt_there() {
    let files: Vec<String> = ["notes.md", "costs.xlsx", "package-lock.json", "data.csv"].map(String::from).to_vec();
    let by = vec![
        CommandFile { file: "costs.xlsx".into(), new: true, kept: false },
        CommandFile { file: "package-lock.json".into(), new: false, kept: true },
        CommandFile { file: "data.csv".into(), new: false, kept: false },
    ];
    assert_eq!(
        file_facts(&files, &by, false),
        [
            "Files it changed: notes.md (backed up first).",
            "Files its commands made: costs.xlsx (new: Undo deletes them).",
            "Files its commands changed: package-lock.json (backed up first, from Git).",
            "Files its commands changed with no backup: data.csv (Undo can't put them back).",
        ]
    );
    // Only commands' files: no "backed up first" line at all.
    let only = file_facts(&files[1..], &by, false);
    assert!(!only.iter().any(|l| l.starts_with("Files it changed")), "{only:?}");
    // Claude Code's hooks never ran: its own edits say so.
    assert_eq!(file_facts(&files[..1], &[], true), ["Files it changed: notes.md (no backups were made, so Undo can't put them back)."]);
    // Many: eight named, then how many more.
    let many: Vec<String> = (1..=11).map(|i| format!("f{i}.txt")).collect();
    assert!(file_facts(&many, &[], false)[0].contains("f8.txt and 3 more"));
}

#[test]
fn a_walk_leaves_build_output_and_hidden_folders_out_and_stops_at_its_caps() {
    let dir = scratch("walk");
    for f in ["src/main.rs", "node_modules/x/index.js", "target/debug/out.txt", ".hidden/secret.txt", "plan.md", "data/yap/log.jsonl"] {
        write(&dir, f, "x");
    }
    let mut got: Vec<String> = walk(&dir, &[dir.join("data").join("yap")], MOST_ENTRIES, WALK_TIME).unwrap().into_iter().map(|(r, _)| r).collect();
    got.sort();
    assert_eq!(got, ["plan.md", "src/main.rs"], "Yap's own data folder left out too");
    // At most so many entries.
    assert_eq!(walk(&dir, &[], 3, WALK_TIME).err(), Some(Skip::TooMany(4)));
    // At most so long (looked at every 256 entries).
    for i in 0..300 {
        write(&dir, &format!("many/{i}.txt"), "x");
    }
    assert!(matches!(walk(&dir, &[], MOST_ENTRIES, Duration::ZERO), Err(Skip::TooSlow(256))));
    assert_eq!(walk(&dir.join("not-there"), &[], MOST_ENTRIES, WALK_TIME).err(), Some(Skip::Unreadable));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A workspace with no Git: what a command made and changed is found and
/// noted, Undo deletes the new file, can't put back the changed one and
/// never touches the person's own edit.
#[test]
fn a_folder_with_no_git_new_and_changed_files_and_what_undo_does() {
    let dir = scratch("walk-turn");
    let backup = scratch("walk-backup");
    write(&dir, "notes.md", "Draft notes\n");
    write(&dir, "data/beds.csv", "bed,length\nA,2\n");
    write(&dir, "plan.md", "Plan\n");
    let files = walk(&dir, &[], MOST_ENTRIES, WALK_TIME).unwrap();
    let before = walked(&dir, files);
    let windows = running();
    // The command: a new workbook, beds.csv rewritten; the person edits plan.md meanwhile (not in a command's run).
    write(&dir, "costs.xlsx", "PK-fake-workbook");
    write(&dir, "data/beds.csv", "bed,length\nA,2\nB,3\n");
    write(&dir, "plan.md", "Plan, edited by hand\n");
    written_long_ago(&dir, "plan.md");
    let found = after(&before, &windows).unwrap();
    assert_eq!(rels(&found), [("costs.xlsx".into(), true, false), ("data/beds.csv".into(), false, false)]);
    let by = settle(Some(&before), &windows, &backup, &dir, &[]);
    let mut shown: Vec<(String, bool, bool)> = by.iter().map(|c| (c.file.replace('\\', "/"), c.new, c.kept)).collect();
    shown.sort();
    assert_eq!(shown, [("costs.xlsx".into(), true, false), ("data/beds.csv".into(), false, false)]);
    let m = backup::read_manifest(&backup).unwrap();
    assert!(m.files.iter().all(|e| e.command && e.after.is_some()));
    // Settled again (a Continue, no command this time): the same, nothing added twice.
    assert_eq!(settle(Some(&before), &[], &backup, &dir, &[]).len(), 2);
    assert_eq!(backup::read_manifest(&backup).unwrap().files.len(), 2);
    // Undo: the new workbook deleted, beds.csv left as the command left it (no copy), plan.md untouched.
    let report = backup::restore_all(std::slice::from_ref(&backup));
    assert_eq!(report.deleted, [dir.join("costs.xlsx")]);
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].reason, backup::NO_COPY);
    assert!(!dir.join("costs.xlsx").exists());
    assert_eq!(read(&dir, "data/beds.csv"), "bed,length\nA,2\nB,3\n");
    assert_eq!(read(&dir, "plan.md"), "Plan, edited by hand\n");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&backup);
}

#[test]
fn an_edit_of_claude_codes_own_just_before_a_command_is_never_the_commands() {
    let dir = scratch("edited");
    let backup = scratch("edited-backup");
    write(&dir, "notes.md", "Draft notes\n");
    let files = walk(&dir, &[], MOST_ENTRIES, WALK_TIME).unwrap();
    let before = walked(&dir, files);
    // Its Edit (hooks off: no backup entry) a moment before it runs the tests, and the tests write a report.
    write(&dir, "notes.md", "Final notes\n");
    let windows = running();
    write(&dir, "test-report.txt", "12 passing\n");
    let by = settle(Some(&before), &windows, &backup, &dir, &["notes.md".into()]);
    assert_eq!(by.iter().map(|c| (c.file.as_str(), c.new)).collect::<Vec<_>>(), [("test-report.txt", true)]);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&backup);
}

#[test]
fn undo_leaves_a_commands_file_alone_once_it_changed_after_the_turn() {
    let dir = scratch("guard");
    let backup = scratch("guard-backup");
    let before = walked(&dir, Vec::new());
    let windows = running();
    write(&dir, "report.md", "Made by a script\n");
    assert_eq!(settle(Some(&before), &windows, &backup, &dir, &[]).len(), 1);
    // The person carries on writing in it afterwards: Undo keeps their work.
    std::thread::sleep(Duration::from_millis(20));
    write(&dir, "report.md", "Made by a script\nAnd my own notes, longer\n");
    let report = backup::restore_all(std::slice::from_ref(&backup));
    assert!(report.deleted.is_empty());
    assert_eq!(report.skipped[0].reason, backup::CHANGED_SINCE);
    assert!(dir.join("report.md").exists());
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&backup);
}

#[test]
fn claude_codes_copy_of_a_file_a_command_made_or_changed_first_isnt_a_backup() {
    let dir = scratch("late");
    let backup = scratch("late-backup");
    write(&dir, "notes.md", "Draft notes\n");
    let files = walk(&dir, &[], MOST_ENTRIES, WALK_TIME).unwrap();
    let before = walked(&dir, files);
    let windows = running();
    // A command makes chart.svg and rewrites notes.md; then Claude Code edits both (its hook copies them as they are then).
    write(&dir, "chart.svg", "<svg/>");
    std::thread::sleep(Duration::from_millis(20));
    write(&dir, "notes.md", "Draft notes\nFormatted\n");
    for f in ["chart.svg", "notes.md"] {
        let input = serde_json::json!({ "tool_name": "Edit", "tool_input": { "file_path": os_path(&dir, f).to_string_lossy() } });
        assert_eq!(crate::integrations::claude_live::backup::record(&backup, Some(&dir), &input, backup::MAX_BACKUP_BYTES), Some(State::Copied));
    }
    write(&dir, "notes.md", "Draft notes\nFormatted\nEdited\n");
    write(&dir, "chart.svg", "<svg>edited</svg>");
    // (Their last writes were Claude Code's edits, outside any command: only the birth of chart.svg says a command made it.)
    written_long_ago(&dir, "notes.md");
    written_long_ago(&dir, "chart.svg");
    let by = settle(Some(&before), &windows, &backup, &dir, &["chart.svg".into(), "notes.md".into()]);
    let m = backup::read_manifest(&backup).unwrap();
    let state = |f: &str| m.files.iter().find(|e| e.original == dir.join(f)).map(|e| (e.state, e.command)).unwrap();
    assert_eq!(state("chart.svg"), (State::Created, true), "made by the command: new, its copy gone");
    assert_eq!(state("notes.md"), (State::Failed, true), "changed by the command before the copy: no copy from before");
    assert_eq!(by.len(), 2);
    assert!(std::fs::read_dir(&backup).unwrap().flatten().all(|e| e.file_name() == backup::MANIFEST), "no stale copies kept");
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&backup);
}

#[test]
fn too_many_files_changed_and_nothing_is_noted() {
    let dir = scratch("many");
    let backup = scratch("many-backup");
    let before = walked(&dir, Vec::new());
    let windows = running();
    for i in 0..=MOST_FOUND {
        write(&dir, &format!("made/{i}.json"), "{}");
    }
    assert_eq!(after(&before, &windows).err(), Some(Skip::TooManyChanged(MOST_FOUND + 1)));
    assert!(settle(Some(&before), &windows, &backup, &dir, &[]).is_empty());
    // No command ran: the workspace isn't read again at all.
    assert!(settle(Some(&before), &[], &backup, &dir, &[]).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&backup);
}

#[test]
fn gits_own_folder_is_found_and_a_relative_path_never_is() {
    let dir = scratch("find-git");
    let exe = if cfg!(windows) { "git.exe" } else { "git" };
    write(&dir, &format!("tools/{exe}"), "");
    write(&dir, &format!("Git/cmd/{exe}"), "");
    let tools = dir.join("tools");
    let path = std::env::join_paths([tools.clone()]).unwrap();
    assert_eq!(find_git_in(Some(&path), None).map(|p| p.path().to_path_buf()), Some(tools.join(exe)));
    // A relative folder on PATH (the workspace's own "tools") is never looked in.
    assert!(find_git_in(Some(OsStr::new("tools")), None).is_none());
    assert_eq!(find_git_in(None, Some(dir.as_os_str())).map(|p| p.path().to_path_buf()), Some(dir.join("Git").join("cmd").join(exe)));
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- a real Git repository (skipped where Git isn't installed) ----

fn git_here(git: &Program, dir: &Path, args: &[&str]) {
    let mut all = vec!["-c", "user.name=Test", "-c", "user.email=test@example.com", "-c", "commit.gpgsign=false"];
    all.extend_from_slice(args);
    let ok = std::process::Command::new(git.path()).args(&all).current_dir(dir).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap().success();
    assert!(ok, "git {args:?}");
}

#[test]
fn a_git_repository_clean_files_from_git_the_persons_work_said_new_ones_deleted() {
    let Some(git) = find_git() else {
        eprintln!("(no Git on this PC: skipped)");
        return;
    };
    let repo = scratch("git");
    let backup = scratch("git-backup");
    // The workspace is a folder inside the repository.
    let ws = repo.join("garden");
    git_here(&git, &repo, &["init", "-q"]);
    git_here(&git, &repo, &["config", "core.autocrlf", "false"]);
    write(&repo, ".gitignore", "*.log\n");
    write(&ws, "package-lock.json", "{\"lockfileVersion\": 3}\n");
    write(&ws, "notes.md", "Draft notes\n");
    write(&ws, "src/beds.js", "export const beds = 2;\n");
    write(&repo, "outside.txt", "not the workspace's\n");
    write(&ws, "old-plan.md", "Old plan\n");
    git_here(&git, &repo, &["add", "-A"]);
    git_here(&git, &repo, &["commit", "-q", "-m", "start"]);
    // The person's own uncommitted work, and a file they deleted.
    write(&ws, "notes.md", "Draft notes\nMy own changes\n");
    std::fs::remove_file(ws.join("old-plan.md")).unwrap();
    let before = super::before(&ws).unwrap();
    assert!(before.git());
    assert_eq!(before.files_read(), 1, "only what Git lists: notes.md");
    let windows = running();
    // The commands: npm rewrites the lock file (clean in Git), a formatter rewrites notes.md (the person's work),
    // a script writes costs.xlsx and a log (ignored); one change is committed; outside the workspace one more.
    write(&ws, "package-lock.json", "{\"lockfileVersion\": 3, \"packages\": {}}\n");
    write(&ws, "notes.md", "Draft notes\nMy own changes\n\n");
    write(&ws, "costs.xlsx", "PK-fake-workbook");
    write(&ws, "build.log", "ok\n");
    write(&repo, "outside.txt", "changed\n");
    write(&ws, "src/beds.js", "export const beds = 3;\n");
    git_here(&git, &repo, &["commit", "-q", "-am", "more beds"]);
    // A script writes a file of the deleted one's name: it wasn't there before the turn, so it's never Git's copy.
    write(&ws, "old-plan.md", "A new plan\n");
    let found = after(&before, &windows).unwrap();
    assert_eq!(
        rels(&found),
        [("costs.xlsx".into(), true, false), ("notes.md".into(), false, false), ("old-plan.md".into(), true, false), ("package-lock.json".into(), false, true), ("src/beds.js".into(), false, true)]
    );
    let by = settle(Some(&before), &windows, &backup, &ws, &[]);
    let kept = |f: &str| by.iter().find(|c| c.file.replace('\\', "/") == f).map(|c| (c.new, c.kept));
    assert_eq!(kept("costs.xlsx"), Some((true, false)));
    assert_eq!(kept("package-lock.json"), Some((false, true)), "Git's own copy kept");
    assert_eq!(kept("src/beds.js"), Some((false, true)), "committed during the turn: still Git's copy from before");
    assert_eq!(kept("notes.md"), Some((false, false)), "the person's uncommitted work: no copy");
    assert_eq!(kept("old-plan.md"), Some((true, false)), "deleted before the turn: new, never Git's copy");
    // Undo: Git's copies put back, the new file deleted, notes.md said.
    let report = backup::restore_all(std::slice::from_ref(&backup));
    assert_eq!(read(&ws, "package-lock.json"), "{\"lockfileVersion\": 3}\n");
    assert_eq!(read(&ws, "src/beds.js"), "export const beds = 2;\n");
    assert!(!ws.join("costs.xlsx").exists());
    assert!(!ws.join("old-plan.md").exists(), "as before the turn: deleted");
    assert_eq!(read(&ws, "notes.md"), "Draft notes\nMy own changes\n\n");
    assert_eq!(report.skipped.iter().map(|s| s.reason.as_str()).collect::<Vec<_>>(), [backup::NO_COPY]);
    assert!(ws.join("build.log").exists(), "ignored by Git: never the turn's");
    let _ = std::fs::remove_dir_all(&repo);
    let _ = std::fs::remove_dir_all(&backup);
}

#[test]
fn a_workspace_the_repository_around_it_ignores_is_walked() {
    let Some(git) = find_git() else {
        eprintln!("(no Git on this PC: skipped)");
        return;
    };
    let repo = scratch("git-ignored");
    git_here(&git, &repo, &["init", "-q"]);
    write(&repo, ".gitignore", "scratch/\n");
    let ws = repo.join("scratch").join("garden");
    write(&ws, "notes.md", "Draft notes\n");
    let before = super::before(&ws).unwrap();
    assert!(!before.git(), "Git sees nothing in an ignored folder: walked instead");
    assert_eq!(before.files_read(), 1);
    let windows = running();
    write(&ws, "costs.xlsx", "PK-fake-workbook");
    assert_eq!(rels(&after(&before, &windows).unwrap()), [("costs.xlsx".into(), true, false)]);
    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn a_git_repository_with_too_much_to_list_skips_the_check() {
    let Some(git) = find_git() else {
        eprintln!("(no Git on this PC: skipped)");
        return;
    };
    let repo = scratch("git-many");
    git_here(&git, &repo, &["init", "-q"]);
    for i in 0..=MOST_ENTRIES {
        let p = repo.join(format!("u{i}.txt"));
        std::fs::write(p, "x").unwrap();
    }
    assert!(matches!(super::before(&repo), Err(Skip::TooMany(_)) | Err(Skip::TooSlow(_))));
    let _ = std::fs::remove_dir_all(&repo);
}

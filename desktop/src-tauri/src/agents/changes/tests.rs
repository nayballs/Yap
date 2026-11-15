//! What a turn's commands changed: the compare, the caps, a folder with no
//! Git and a Git repository (when Git is on this PC). Every workspace is a
//! fresh folder in the temp folder; fictional files.

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

/// The workspace at `dir` as a walk finds it now.
fn walked(dir: &Path) -> Before {
    let files = walk(dir, &[], MOST_ENTRIES, WALK_TIME).unwrap();
    Before { folder: dir.to_path_buf(), how: How::Walk, files: files.into_iter().map(|(r, s)| (key(&r), (r, s))).collect(), listed: HashSet::new() }
}

fn rels(found: &[Found]) -> Vec<(String, bool)> {
    let mut v: Vec<(String, bool)> = found.iter().map(|f| (f.rel.clone(), f.new)).collect();
    v.sort();
    v
}

fn shown(by: &[CommandFile]) -> Vec<(String, bool)> {
    let mut v: Vec<(String, bool)> = by.iter().map(|c| (c.file.replace('\\', "/"), c.new)).collect();
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
    // Changed while the command ran: changed.
    assert_eq!(classify(Some(&before), &seen(12, s(105), s(0)), false, &windows), Some((false, false)));
    // Changed, but long after the command: the person's, not the turn's.
    assert_eq!(classify(Some(&before), &seen(12, s(300), s(0)), false, &windows), None);
    // Just outside the run, within the slack either side: the command's.
    assert_eq!(classify(Some(&before), &seen(12, s(99), s(0)), false, &windows), Some((false, false)));
    assert_eq!(classify(Some(&before), &seen(12, s(111), s(0)), false, &windows), Some((false, false)));
    assert_eq!(classify(Some(&before), &seen(12, s(97), s(0)), false, &windows), None);
    // Git had it at the start: changed, never new.
    assert_eq!(classify(None, &seen(12, s(105), s(0)), true, &windows), Some((false, true)));
    assert_eq!(classify(None, &seen(12, s(300), s(0)), true, &windows), None);
    // Not there before, made while it ran: new (even with an old time, as an archive unpacked leaves it).
    assert_eq!(classify(None, &seen(5, s(0), s(104)), false, &windows), Some((true, false)));
    // Made by the person meanwhile: not the turn's.
    assert_eq!(classify(None, &seen(5, s(200), s(200)), false, &windows), None);
    // Unseen before but older than any command (it was there): changed, never new.
    assert_eq!(classify(None, &seen(5, s(105), s(0)), false, &windows), Some((false, false)));
    // No birth time: never called new.
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
fn the_facts_say_what_changed_and_nothing_of_backups() {
    let files: Vec<String> = ["notes.md", "costs.xlsx", "package.json"].map(String::from).to_vec();
    let by = vec![CommandFile { file: "costs.xlsx".into(), new: true }, CommandFile { file: "package.json".into(), new: false }];
    assert_eq!(file_facts(&files, &by), ["Files it changed: notes.md.", "Files its commands made: costs.xlsx.", "Files its commands changed: package.json."]);
    // Only commands' files: no line for its own edits.
    assert!(!file_facts(&files[1..], &by).iter().any(|l| l.starts_with("Files it changed")));
    assert!(!file_facts(&files, &by).iter().any(|l| l.contains("backup") || l.contains("Undo")));
    // Many: eight named, then how many more.
    let many: Vec<String> = (1..=11).map(|i| format!("f{i}.txt")).collect();
    assert!(file_facts(&many, &[])[0].contains("f8.txt and 3 more"));
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

/// A workspace with no Git: what a command made and changed is found, the
/// person's own edit meanwhile isn't, and nothing is copied anywhere.
#[test]
fn a_folder_with_no_git_new_and_changed_files_never_the_persons_own() {
    let dir = scratch("walk-turn");
    write(&dir, "notes.md", "Draft notes\n");
    write(&dir, "data/beds.csv", "bed,length\nA,2\n");
    write(&dir, "plan.md", "Plan\n");
    let before = walked(&dir);
    let windows = running();
    // The command: a new workbook, beds.csv rewritten; the person edits plan.md meanwhile (not in a command's run).
    write(&dir, "costs.xlsx", "PK-fake-workbook");
    write(&dir, "data/beds.csv", "bed,length\nA,2\nB,3\n");
    write(&dir, "plan.md", "Plan, edited by hand\n");
    written_long_ago(&dir, "plan.md");
    assert_eq!(rels(&after(&before, &windows).unwrap()), [("costs.xlsx".into(), true), ("data/beds.csv".into(), false)]);
    let by = settle(Some(&before), &windows, &dir, &[]);
    assert_eq!(shown(&by), [("costs.xlsx".into(), true), ("data/beds.csv".into(), false)]);
    // Nothing besides the workspace's own files: no copies.
    assert_eq!(walk(&dir, &[], MOST_ENTRIES, WALK_TIME).unwrap().len(), 4);
    // No command ran (or no reading before): the workspace isn't read again at all.
    assert!(settle(Some(&before), &[], &dir, &[]).is_empty());
    assert!(settle(None, &windows, &dir, &[]).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_edit_of_claude_codes_own_just_before_a_command_is_never_the_commands() {
    let dir = scratch("edited");
    write(&dir, "notes.md", "Draft notes\n");
    let before = walked(&dir);
    // Its Edit a moment before it runs the tests, and the tests write a report.
    write(&dir, "notes.md", "Final notes\n");
    let windows = running();
    write(&dir, "test-report.txt", "12 passing\n");
    let by = settle(Some(&before), &windows, &dir, &["notes.md".into()]);
    assert_eq!(shown(&by), [("test-report.txt".into(), true)]);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn too_many_files_changed_and_nothing_is_noted() {
    let dir = scratch("many");
    let before = walked(&dir);
    let windows = running();
    for i in 0..=MOST_FOUND {
        write(&dir, &format!("made/{i}.json"), "{}");
    }
    assert_eq!(after(&before, &windows).err(), Some(Skip::TooManyChanged(MOST_FOUND + 1)));
    assert!(settle(Some(&before), &windows, &dir, &[]).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
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
fn a_git_repository_new_and_changed_files_a_commit_in_the_turn_an_ignored_log() {
    let Some(git) = find_git() else {
        eprintln!("(no Git on this PC: skipped)");
        return;
    };
    let repo = scratch("git");
    // The workspace is a folder inside the repository.
    let ws = repo.join("garden");
    git_here(&git, &repo, &["init", "-q"]);
    git_here(&git, &repo, &["config", "core.autocrlf", "false"]);
    write(&repo, ".gitignore", "*.log\n");
    write(&ws, "package-lock.json", "{\"lockfileVersion\": 3}\n");
    write(&ws, "notes.md", "Draft notes\n");
    write(&ws, "src/beds.js", "export const beds = 2;\n");
    write(&ws, "old-plan.md", "Old plan\n");
    write(&repo, "outside.txt", "not the workspace's\n");
    git_here(&git, &repo, &["add", "-A"]);
    git_here(&git, &repo, &["commit", "-q", "-m", "start"]);
    // The person's own uncommitted work, and a file they deleted.
    write(&ws, "notes.md", "Draft notes\nMy own changes\n");
    std::fs::remove_file(ws.join("old-plan.md")).unwrap();
    let before = super::before(&ws).unwrap();
    assert!(before.git());
    assert_eq!(before.files_read(), 1, "only what Git lists that's there: notes.md");
    let windows = running();
    // The commands: npm rewrites the lock file, a formatter rewrites notes.md, a script writes costs.xlsx and a log
    // (ignored); one change is committed; outside the workspace one more; a file of the deleted one's name again.
    write(&ws, "package-lock.json", "{\"lockfileVersion\": 3, \"packages\": {}}\n");
    write(&ws, "notes.md", "Draft notes\nMy own changes\n\n");
    write(&ws, "costs.xlsx", "PK-fake-workbook");
    write(&ws, "build.log", "ok\n");
    write(&repo, "outside.txt", "changed\n");
    write(&ws, "src/beds.js", "export const beds = 3;\n");
    git_here(&git, &repo, &["commit", "-q", "-am", "more beds"]);
    write(&ws, "old-plan.md", "A new plan\n");
    let by = settle(Some(&before), &windows, &ws, &[]);
    assert_eq!(
        shown(&by),
        [("costs.xlsx".into(), true), ("notes.md".into(), false), ("old-plan.md".into(), true), ("package-lock.json".into(), false), ("src/beds.js".into(), false)],
        "the committed file through HEAD's move; the ignored log and the file outside never"
    );
    let _ = std::fs::remove_dir_all(&repo);
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
    assert_eq!(rels(&after(&before, &windows).unwrap()), [("costs.xlsx".into(), true)]);
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

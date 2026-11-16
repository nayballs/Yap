use super::*;

/// A workspace with a few files, and a folder beside it (outside it) with a secret.
struct Fixture {
    base: PathBuf,
    root: PathBuf,
    outside: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Fixture {
        let base = std::env::temp_dir().join(format!("yap-code-files-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("garden-planner");
        let outside = base.join("elsewhere");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join("docs")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(root.join("src/convert.js"), "export const a = 1;\nexport const b = 2;\n").unwrap();
        std::fs::write(root.join("docs/lab.md"), "# Lab\n").unwrap();
        std::fs::write(root.join("README.md"), "hello\n").unwrap();
        std::fs::write(outside.join("secret.txt"), "not yours\n").unwrap();
        Fixture { base, root, outside }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// A link to a folder: a symlink, else (no right to make one) a junction.
fn link_dir(target: &Path, at: &Path) -> bool {
    #[cfg(windows)]
    {
        if std::os::windows::fs::symlink_dir(target, at).is_ok() {
            return true;
        }
        std::process::Command::new("cmd").args(["/c", "mklink", "/J"]).arg(at).arg(target).output().map(|o| o.status.success()).unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(target, at).is_ok()
    }
}

#[test]
fn a_path_and_its_line_split_as_answers_write_them() {
    assert_eq!(split_line("docs/lab.md:105"), ("docs/lab.md", Some(105)));
    assert_eq!(split_line("src/convert.js:12:7"), ("src/convert.js", Some(12)));
    assert_eq!(split_line("src/convert.js#L40"), ("src/convert.js", Some(40)));
    assert_eq!(split_line("src/convert.js#L40-L52"), ("src/convert.js", Some(40)));
    assert_eq!(split_line("src/convert.js"), ("src/convert.js", None));
    assert_eq!(split_line("src/convert.js:0"), ("src/convert.js", None));
    // A drive's colon is the path's.
    assert_eq!(split_line(r"C:\Projects\app.ts:9"), (r"C:\Projects\app.ts", Some(9)));
    assert_eq!(split_line(r"C:\Projects\app.ts"), (r"C:\Projects\app.ts", None));
    assert_eq!(split_line("C:12"), ("C:12", None));
    // Not a line: words after the colon.
    assert_eq!(split_line("note: see"), ("note: see", None));
}

#[test]
fn files_in_the_workspace_resolve_relative_or_full() {
    let f = Fixture::new("in");
    let r = resolve(&f.root, "src/convert.js").unwrap();
    assert_eq!(r.rel, "src/convert.js");
    assert_eq!(r.line, None);
    assert!(Path::new(&r.abs).is_file());
    assert!(!r.abs.starts_with(r"\\?\"), "said as Windows writes it");
    assert_eq!(resolve(&f.root, "docs/lab.md:105").unwrap().line, Some(105));
    assert_eq!(resolve(&f.root, "./README.md").unwrap().rel, "README.md");
    assert_eq!(resolve(&f.root, "`src/convert.js`").unwrap().rel, "src/convert.js");
    #[cfg(windows)]
    assert_eq!(resolve(&f.root, r"src\convert.js:2").unwrap().rel, "src/convert.js");
    // A full path inside the workspace.
    let full = f.root.join("src").join("convert.js");
    assert_eq!(resolve(&f.root, &format!("{}:2", full.display())).unwrap().rel, "src/convert.js");
    // `..` that stays inside is fine.
    assert_eq!(resolve(&f.root, "docs/../src/convert.js").unwrap().rel, "src/convert.js");
}

#[test]
fn nothing_outside_the_workspace_ever_resolves() {
    let f = Fixture::new("out");
    // Climbing out, in every spelling.
    assert_eq!(resolve(&f.root, "../elsewhere/secret.txt"), Err(Refused::Outside));
    assert_eq!(resolve(&f.root, r"..\elsewhere\secret.txt"), Err(Refused::Outside));
    assert_eq!(resolve(&f.root, "src/../../elsewhere/secret.txt"), Err(Refused::Outside));
    // A full path outside, and one inside that climbs out.
    assert_eq!(resolve(&f.root, &f.outside.join("secret.txt").display().to_string()), Err(Refused::Outside));
    let sneaky = format!("{}{}..{}elsewhere{}secret.txt", f.root.display(), std::path::MAIN_SEPARATOR, std::path::MAIN_SEPARATOR, std::path::MAIN_SEPARATOR);
    assert_eq!(resolve(&f.root, &sneaky), Err(Refused::Outside));
    // Device, network and drive-relative paths.
    assert_eq!(resolve(&f.root, r"\\?\C:\Windows\win.ini"), Err(Refused::Outside));
    assert_eq!(resolve(&f.root, r"\\.\PhysicalDrive0"), Err(Refused::Outside));
    assert_eq!(resolve(&f.root, r"\\server\share\x.txt"), Err(Refused::Outside));
    assert_eq!(resolve(&f.root, "//server/share/x.txt"), Err(Refused::Outside));
    #[cfg(windows)]
    {
        assert_eq!(resolve(&f.root, r"\Windows\win.ini"), Err(Refused::Outside));
        assert_eq!(resolve(&f.root, "C:Windows"), Err(Refused::Outside));
    }
    // Schemes, streams, wildcards, control characters.
    assert_eq!(resolve(&f.root, "file:///C:/Windows/win.ini"), Err(Refused::NotAPath));
    assert_eq!(resolve(&f.root, "https://example.com/a.js"), Err(Refused::NotAPath));
    assert_eq!(resolve(&f.root, "README.md:secret"), Err(Refused::NotAPath));
    assert_eq!(resolve(&f.root, "src/*.js"), Err(Refused::NotAPath));
    assert_eq!(resolve(&f.root, "src/convert.js\u{0}"), Err(Refused::NotAPath));
    assert_eq!(resolve(&f.root, ""), Err(Refused::NotAPath));
    // Not there, and a folder.
    assert_eq!(resolve(&f.root, "src/missing.js"), Err(Refused::Missing));
    assert_eq!(resolve(&f.root, "src"), Err(Refused::NotAFile));
    assert!(resolve(&f.root, ".").is_err(), "the workspace itself is no file");
}

#[test]
fn a_link_inside_the_workspace_to_a_folder_outside_never_opens() {
    let f = Fixture::new("link");
    let at = f.root.join("shortcut");
    if !link_dir(&f.outside, &at) {
        eprintln!("(no symlink or junction could be made here: skipped)");
        return;
    }
    // The link's path is inside, but the file it reaches isn't.
    assert_eq!(resolve(&f.root, "shortcut/secret.txt"), Err(Refused::Outside));
    assert!(read(&f.root, "shortcut/secret.txt").is_err());
    // A link to a folder inside stays fine.
    let inner = f.root.join("srclink");
    if link_dir(&f.root.join("src"), &inner) {
        assert_eq!(resolve(&f.root, "srclink/convert.js").unwrap().rel, "src/convert.js");
    }
}

#[test]
fn a_file_reads_as_text_capped_and_binary_files_say_so() {
    let f = Fixture::new("read");
    let v = read(&f.root, "src/convert.js:2").unwrap();
    assert_eq!(v.text, "export const a = 1;\nexport const b = 2;\n");
    assert_eq!(v.file.line, Some(2));
    assert!(!v.binary && !v.truncated);
    std::fs::write(f.root.join("logo.png"), [0x89, b'P', b'N', b'G', 0, 0, 1]).unwrap();
    let b = read(&f.root, "logo.png").unwrap();
    assert!(b.binary);
    assert!(b.text.is_empty());
    // A BOM gone, bad bytes replaced.
    let (t, cut) = as_text(b"\xEF\xBB\xBFhi \xFF there", false);
    assert_eq!(t, "hi \u{FFFD} there");
    assert!(!cut);
    // More than was read: cut at the last whole line.
    let (t, cut) = as_text(b"one\ntwo\nthr", true);
    assert_eq!(t, "one\ntwo");
    assert!(cut);
    // More lines than the view shows.
    let many = "x\n".repeat(LINES_MAX + 5);
    let (t, cut) = as_text(many.as_bytes(), false);
    assert_eq!(t.lines().count(), LINES_MAX);
    assert!(cut);
}

#[test]
fn opening_elsewhere_keeps_to_the_workspace_and_yaps_view_is_the_pages() {
    let f = Fixture::new("open-elsewhere");
    std::fs::write(f.root.join("src/report.xlsx"), b"PK\x03\x04").unwrap();
    // Yap's own view, or a value Yap doesn't know: the page shows it.
    assert_eq!(open(&f.root, "src/convert.js:3", None, editors::YAP, true), Ok(Opened::Yap { note: None }));
    assert_eq!(open(&f.root, "src/convert.js", None, "rm -rf", true), Ok(Opened::Yap { note: None }));
    // Never a file outside the workspace, however it's asked.
    for with in [editors::YAP, editors::DEFAULT_APP, editors::ANY_EDITOR, "vscode"] {
        assert!(open(&f.root, "../elsewhere/secret.txt", None, with, true).is_err(), "{with}");
    }
    // A workbook a link opens stays in Yap's sheet view, whatever the editor.
    assert_eq!(open(&f.root, "src/report.xlsx", None, "vscode", true), Ok(Opened::Yap { note: None }));
    // A script under "Windows' default app": shown in Yap, and said why.
    std::fs::write(f.root.join("src/build.ps1"), "Write-Output hi\n").unwrap();
    let Ok(Opened::Yap { note: Some(why) }) = open(&f.root, "src/build.ps1", None, editors::DEFAULT_APP, true) else { panic!("a script never goes to its default app") };
    assert!(why.contains(".ps1"), "{why}");
}

#[test]
fn the_environment_reads_the_branch_and_a_worktree_from_git_itself() {
    assert_eq!(head_of("ref: refs/heads/main\n"), (Some("main".to_string()), false));
    assert_eq!(head_of("ref: refs/heads/claude/files-panel"), (Some("claude/files-panel".to_string()), false));
    assert_eq!(head_of("2f458963aa11bb22cc33dd44ee55ff6677889900"), (Some("2f45896".to_string()), true));
    assert_eq!(head_of(""), (None, false));

    let f = Fixture::new("env");
    // Not a repository.
    let e = environment("garden-planner", &f.root.display().to_string());
    assert!(e.exists && !e.repo && e.branch.is_none());
    // A repository on a branch.
    std::fs::create_dir_all(f.root.join(".git")).unwrap();
    std::fs::write(f.root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    let e = environment("garden-planner", &f.root.join("src").display().to_string());
    assert!(e.repo && !e.worktree);
    assert_eq!(e.branch.as_deref(), Some("main"));
    // A linked worktree: `.git` is a file pointing into the repository's worktrees.
    let wt = f.base.join("garden-planner-wt");
    std::fs::create_dir_all(&wt).unwrap();
    let gitdir = f.root.join(".git/worktrees/garden-planner-wt");
    std::fs::create_dir_all(&gitdir).unwrap();
    std::fs::write(gitdir.join("HEAD"), "ref: refs/heads/claude/try-it\n").unwrap();
    std::fs::write(wt.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
    let e = environment("garden-planner-wt", &wt.display().to_string());
    assert!(e.repo && e.worktree);
    assert_eq!(e.branch.as_deref(), Some("claude/try-it"));
    // A folder that's gone.
    assert!(!environment("gone", &f.base.join("gone").display().to_string()).exists);
}

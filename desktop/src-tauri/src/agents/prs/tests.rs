use super::*;

// `gh pr list --head <branch> --state all --json number,title,state,isDraft,url,statusCheckRollup`
// as gh 2.103 answers it, with a fictional repository.
const LIST: &str = include_str!("fixtures/list.json");

/// A folder of its own under the temp folder, gone after.
struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("yap-code-prs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }

    /// A repository on `branch`, with `config` as its git config.
    fn repo(&self, branch: &str, config: &str) -> PathBuf {
        let git = self.0.join(".git");
        std::fs::create_dir_all(&git).unwrap();
        std::fs::write(git.join("HEAD"), format!("ref: refs/heads/{branch}\n")).unwrap();
        std::fs::write(git.join("config"), config).unwrap();
        self.0.clone()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const GITHUB: &str = "[core]\n\trepositoryformatversion = 0\n[remote \"origin\"]\n\turl = https://github.com/example-org/garden-planner.git\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n[branch \"main\"]\n\tremote = origin\n";

#[test]
fn gh_s_list_is_read_newest_first() {
    let list = parse_list(LIST).expect("a list");
    assert_eq!(list.iter().map(|p| p.number).collect::<Vec<_>>(), [131, 128, 117, 112]);
    assert_eq!(
        list[0],
        Pr {
            number: 131,
            title: "Round toInches to one decimal".into(),
            repo: "example-org/garden-planner".into(),
            url: "https://github.com/example-org/garden-planner/pull/131".into(),
            state: "open",
            checks: "running",
            counts: Counts { pending: 1, successful: 1, ..Default::default() }
        }
    );
    assert_eq!(list.iter().map(|p| (p.state, p.checks)).collect::<Vec<_>>(), [("open", "running"), ("draft", "none"), ("merged", "passing"), ("closed", "failing")]);
    assert_eq!(list[1].counts, Counts::default());
    assert_eq!(parse_list("[]"), Some(Vec::new()));
    assert_eq!(parse_list("Showing 0 of 0 pull requests"), None, "not the JSON asked for");
    assert_eq!(parse_list("{\"number\":1}"), None);
}

#[test]
fn checks_roll_up_as_github_shows_them() {
    let run = |status: &str, conclusion: &str| serde_json::json!({"__typename": "CheckRun", "status": status, "conclusion": conclusion});
    let ctx = |state: &str| serde_json::json!({"__typename": "StatusContext", "state": state});
    let of = |items: Vec<Value>| checks_of(Some(&Value::Array(items)));
    assert_eq!(checks_of(None), "none");
    assert_eq!(of(vec![]), "none");
    assert_eq!(of(vec![run("COMPLETED", "SUCCESS"), run("COMPLETED", "SKIPPED"), run("COMPLETED", "NEUTRAL"), ctx("SUCCESS")]), "passing");
    assert_eq!(of(vec![run("COMPLETED", "SUCCESS"), run("IN_PROGRESS", "")]), "running");
    assert_eq!(of(vec![run("QUEUED", ""), ctx("PENDING")]), "running");
    assert_eq!(of(vec![run("COMPLETED", "SUCCESS"), run("COMPLETED", "FAILURE"), run("IN_PROGRESS", "")]), "failing", "a failure wins over one still running");
    assert_eq!(of(vec![run("COMPLETED", "TIMED_OUT")]), "failing");
    assert_eq!(of(vec![run("COMPLETED", "ACTION_REQUIRED")]), "failing");
    assert_eq!(of(vec![run("COMPLETED", "STARTUP_FAILURE")]), "failing");
    assert_eq!(of(vec![ctx("ERROR")]), "failing");
    assert_eq!(of(vec![ctx("FAILURE")]), "failing");
    assert_eq!(of(vec![ctx("EXPECTED")]), "running");
    assert_eq!(of(vec![run("COMPLETED", "")]), "running", "finished with no conclusion yet");
}

#[test]
fn cancelled_skipped_and_neutral_are_what_they_are_never_failing() {
    let run = |name: &str, status: &str, conclusion: &str| serde_json::json!({"__typename": "CheckRun", "name": name, "workflowName": "CI", "status": status, "conclusion": conclusion});
    let of = |items: Vec<Value>| {
        let rollup = Value::Array(items);
        (checks_of(Some(&rollup)), counts_of(Some(&rollup)))
    };
    // A cancelled check: cancelled, never "failing" (16 Nov 2026: it read "Checks failing").
    assert_eq!(of(vec![run("e2e", "COMPLETED", "CANCELLED")]), ("cancelled", Counts { cancelled: 1, ..Default::default() }));
    assert_eq!(
        of(vec![run("e2e (1/3)", "COMPLETED", "CANCELLED"), run("e2e (2/3)", "COMPLETED", "SUCCESS"), run("CodeQL", "COMPLETED", "NEUTRAL")]),
        ("cancelled", Counts { cancelled: 1, neutral: 1, successful: 1, ..Default::default() })
    );
    // Skipped and neutral pass, as GitHub counts them; with nothing else, they're said as they are.
    assert_eq!(of(vec![run("check", "COMPLETED", "SUCCESS"), run("deploy", "COMPLETED", "SKIPPED"), run("CodeQL", "COMPLETED", "NEUTRAL")]).0, "passing");
    assert_eq!(of(vec![run("deploy", "COMPLETED", "SKIPPED"), run("docs", "COMPLETED", "SKIPPED")]), ("skipped", Counts { skipped: 2, ..Default::default() }));
    assert_eq!(of(vec![run("CodeQL", "COMPLETED", "NEUTRAL")]), ("neutral", Counts { neutral: 1, ..Default::default() }));
    // A real failure wins over a cancelled one, one still running over a cancelled one.
    assert_eq!(of(vec![run("a", "COMPLETED", "CANCELLED"), run("b", "COMPLETED", "FAILURE")]).0, "failing");
    assert_eq!(of(vec![run("a", "COMPLETED", "CANCELLED"), run("b", "IN_PROGRESS", "")]).0, "running");
    // STALE never finished: pending, as `gh pr checks` has it.
    assert_eq!(of(vec![run("old", "COMPLETED", "STALE")]).1, Counts { pending: 1, ..Default::default() });
}

#[test]
fn only_the_latest_run_of_each_check_counts() {
    // A run a workflow's concurrency cancelled, then the same check run again and passing (as on a real PR).
    let run = |name: &str, workflow: &str, started: &str, conclusion: &str| serde_json::json!({"__typename": "CheckRun", "name": name, "workflowName": workflow, "startedAt": started, "status": "COMPLETED", "conclusion": conclusion});
    let rollup = Value::Array(vec![
        run("check", "CI", "2026-11-11T08:22:09Z", "CANCELLED"),
        run("e2e (1/3)", "E2E", "2026-11-11T08:22:19Z", "FAILURE"),
        run("check", "CI", "2026-11-11T08:22:19Z", "SUCCESS"),
        run("e2e (1/3)", "E2E", "2026-11-11T08:22:09Z", "CANCELLED"),
        // The same name in another workflow is another check.
        run("check", "Lint", "2026-11-11T08:22:01Z", "CANCELLED"),
        serde_json::json!({"__typename": "StatusContext", "context": "deploy/preview", "state": "SUCCESS"}),
    ]);
    assert_eq!(counts_of(Some(&rollup)), Counts { failing: 1, cancelled: 1, successful: 2, ..Default::default() });
    let names: Vec<(&str, &str)> = latest_checks(rollup.as_array().unwrap()).iter().map(|c| (c["name"].as_str().or(c["context"].as_str()).unwrap(), c["conclusion"].as_str().or(c["state"].as_str()).unwrap())).collect();
    assert_eq!(names, [("e2e (1/3)", "FAILURE"), ("check", "SUCCESS"), ("check", "CANCELLED"), ("deploy/preview", "SUCCESS")]);
    // Every run replaced by a passing one: passing.
    let rerun = Value::Array(vec![run("check", "CI", "2026-11-11T08:22:09Z", "CANCELLED"), run("check", "CI", "2026-11-11T08:30:00Z", "SUCCESS")]);
    assert_eq!(checks_of(Some(&rerun)), "passing");
}

#[test]
fn what_gh_says_when_it_cant() {
    // gh 2.103's own words.
    assert_eq!(status_of_failure(Some(4), "To get started with GitHub CLI, please run:  gh auth login\nAlternatively, populate the GH_TOKEN environment variable with a GitHub API authentication token.\n"), Status::SignedOut);
    assert_eq!(status_of_failure(Some(1), "no git remotes found\n"), Status::NoRemote);
    assert_eq!(status_of_failure(Some(1), "none of the git remotes configured for this repository point to a known GitHub host. To tell gh about a new GitHub host, please use `gh auth login`\n"), Status::NoRemote);
    assert_eq!(status_of_failure(Some(1), "failed to run git: fatal: not a git repository (or any of the parent directories): .git\n"), Status::NoGit);
    assert_eq!(status_of_failure(Some(1), "HTTP 502: Bad Gateway (https://api.github.com/graphql)\n"), Status::Failed);
    assert_eq!(status_of_failure(None, ""), Status::Failed);
}

#[test]
fn no_gh_on_this_pc() {
    let d = Dir::new("no-gh");
    // An empty PATH, and an install folder without it: none.
    assert_eq!(find_in(Some(OsStr::new("")), Some(d.0.as_os_str())), None);
    assert_eq!(find_in(None, None), None);
    // A repository on GitHub, no gh: one quiet line.
    let dir = d.repo("claude/round-inches", GITHUB);
    assert_eq!(read(&dir, None), Prs { status: Status::NoGh, branch: Some("claude/round-inches".into()), list: Vec::new() });
}

#[test]
fn gh_is_found_only_as_an_exe_in_an_absolute_folder() {
    let d = Dir::new("find");
    let bin = d.0.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let exe = if cfg!(windows) { "gh.exe" } else { "gh" };
    // A script named gh: never run.
    std::fs::write(bin.join("gh.cmd"), "@echo off\n").unwrap();
    assert_eq!(find_in(Some(bin.as_os_str()), None), None);
    std::fs::write(bin.join(exe), b"MZ").unwrap();
    assert_eq!(find_in(Some(bin.as_os_str()), None), Some(bin.join(exe)));
    // A relative folder on PATH is skipped (the workspace's own `gh.exe` never runs).
    let relative = std::env::join_paths([PathBuf::from("bin"), PathBuf::from(".")]).unwrap();
    assert_eq!(find_in(Some(&relative), None), None);
    // GitHub CLI's own install folder.
    let pf = d.0.join("pf");
    std::fs::create_dir_all(pf.join("GitHub CLI")).unwrap();
    std::fs::write(pf.join("GitHub CLI").join(exe), b"MZ").unwrap();
    assert_eq!(find_in(Some(OsStr::new("")), Some(pf.as_os_str())), Some(pf.join("GitHub CLI").join(exe)));
}

#[test]
fn no_git_no_branch_no_remote() {
    let d = Dir::new("quiet");
    assert_eq!(read(&d.0, None).status, Status::NoGit);
    assert_eq!(read(&d.0.join("gone"), None).status, Status::NoGit);
    let dir = d.repo("main", "[core]\n\tbare = false\n");
    assert_eq!(read(&dir, None), Prs { status: Status::NoRemote, branch: Some("main".into()), list: Vec::new() });
    std::fs::write(dir.join(".git").join("HEAD"), "0123456789abcdef0123456789abcdef01234567\n").unwrap();
    assert_eq!(read(&dir, None).status, Status::NoBranch);
}

#[test]
fn a_worktree_s_remotes_are_its_repository_s() {
    let d = Dir::new("worktree");
    let repo_git = d.0.join("repo").join(".git");
    let wt_git = repo_git.join("worktrees").join("round-inches");
    std::fs::create_dir_all(&wt_git).unwrap();
    std::fs::write(repo_git.join("config"), GITHUB).unwrap();
    std::fs::write(wt_git.join("commondir"), "../..\n").unwrap();
    assert_eq!(remotes(&wt_git), ["https://github.com/example-org/garden-planner.git"]);
    assert_eq!(remotes(&repo_git), ["https://github.com/example-org/garden-planner.git"]);
    assert!(remotes(&d.0).is_empty());
}

#[test]
fn a_config_s_remotes() {
    let config = "[remote \"origin\"]\n\turl = git@github.com:example-org/garden-planner.git\n[branch \"x\"]\n\turl = not-a-remote\n[remote \"upstream\"]\n\tURL = https://github.com/example/upstream\n";
    assert_eq!(remotes_in(config), ["git@github.com:example-org/garden-planner.git", "https://github.com/example/upstream"]);
    assert!(remotes_in("[core]\n\tbare = false\n").is_empty());
}

#[test]
fn a_link_s_repository() {
    assert_eq!(repo_of("https://github.com/example-org/garden-planner/pull/7"), "example-org/garden-planner");
    assert_eq!(repo_of("https://github.example.com/team/app/pull/1"), "team/app");
    assert_eq!(repo_of(""), "");
    assert_eq!(repo_of("https://github.com/"), "");
}

#[test]
fn the_branch_is_never_read_as_a_flag() {
    let a = args("--help");
    assert!(a.contains(&"--head=--help".to_string()));
    assert_eq!(&a[..2], ["pr", "list"]);
    assert!(a.windows(2).any(|w| w == ["--state", "all"]));
    assert!(a.last().unwrap().contains("statusCheckRollup"));
}

#[test]
fn gh_never_prompts() {
    let e = gh_env();
    let get = |k: &str| e.iter().find(|(n, _)| n.to_string_lossy().eq_ignore_ascii_case(k)).map(|(_, v)| v.to_string_lossy().into_owned());
    assert_eq!(get("GH_PROMPT_DISABLED").as_deref(), Some("1"));
    assert_eq!(get("GIT_TERMINAL_PROMPT").as_deref(), Some("0"));
    assert_eq!(e.iter().filter(|(n, _)| n.to_string_lossy().eq_ignore_ascii_case("NO_COLOR")).count(), 1);
}

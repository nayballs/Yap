//! **Yap Lab's read-only `gh`** (debug builds; `docs/lab.md`, `set --gh on`):
//! `yap.exe lab-gh <gh.exe> <gh's arguments…>` runs the person's own GitHub
//! CLI for a Code chat's summary (its Changes: `prs.rs`) in a live lab — and
//! only for what can't change anything:
//!
//! - **two commands**: `gh pr list` and `gh pr view`;
//! - **their reading flags only**: `--head`, `--state`, `--limit`, `--json`
//!   (and `gh pr view`'s one pull request: a number, a branch, or its link on
//!   Yap's repository). Anything else — `--web` (the person's browser),
//!   `--jq`, `--template`, another command, a stray word — is refused before
//!   `gh` starts;
//! - **one repository**: Yap's own public one, [`REPO`], given as `--repo`
//!   whatever the folder's remotes say (another `--repo` is refused).
//!
//! The lab turns it on (`e2e_gh_lab`, only in a live lab: never the e2e suite,
//! never a `--fake` lab), so a test Yap's summary shows the branch's real
//! pull requests instead of "Install GitHub's gh" (test mode never runs the
//! real `gh`). It runs on the person's own `gh` sign-in, as the app does;
//! Yap passes no token. Pure but for [`main`].

/// The only repository the lab's `gh` reads: Yap's own, public.
pub const REPO: &str = "nayballs/Yap";

/// `gh pr list`'s states.
const STATES: [&str; 4] = ["open", "closed", "merged", "all"];

/// A `--json` field list: gh's field names, commas between.
fn fields_ok(v: &str) -> bool {
    !v.is_empty() && v.split(',').all(|f| !f.is_empty() && f.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// A branch name as `--head` or `gh pr view` take it (never a flag).
fn branch_ok(v: &str) -> bool {
    !v.is_empty() && !v.starts_with('-') && v.len() <= 200 && v.chars().all(|c| c.is_ascii_alphanumeric() || "._/-".contains(c))
}

/// `gh pr view`'s one pull request: its number, its branch, or its link on [`REPO`].
fn which_ok(v: &str) -> bool {
    if v.chars().all(|c| c.is_ascii_digit()) && !v.is_empty() && v.len() <= 9 {
        return true;
    }
    if let Some(n) = v.strip_prefix(&format!("https://github.com/{REPO}/pull/")) {
        return !n.is_empty() && n.chars().all(|c| c.is_ascii_digit());
    }
    branch_ok(v)
}

/// **`gh`'s arguments, checked** (`args`: what Yap gave after `lab-gh
/// <gh.exe>`): the same with `--repo nayballs/Yap` after them, or why not.
/// Pure.
pub fn allowed(args: &[String]) -> Result<Vec<String>, String> {
    let (cmd, rest) = match args {
        [pr, cmd, rest @ ..] if pr == "pr" && (cmd == "list" || cmd == "view") => (cmd.as_str(), rest),
        _ => return Err(format!("Only gh pr list and gh pr view (not gh {})", args.iter().take(2).cloned().collect::<Vec<_>>().join(" "))),
    };
    let mut out = vec!["pr".to_string(), cmd.to_string()];
    let mut which = false;
    let mut i = 0;
    while i < rest.len() {
        let a = rest[i].as_str();
        // `--flag=value` or `--flag value`.
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (a, None),
        };
        if !flag.starts_with('-') {
            if cmd == "view" && !which && which_ok(a) {
                which = true;
                out.push(a.to_string());
                i += 1;
                continue;
            }
            return Err(format!("gh pr {cmd} doesn't take “{a}” here"));
        }
        // (Each of them takes a value.)
        if !matches!((cmd, flag), ("list", "--head" | "--state" | "--limit") | (_, "--json" | "--repo" | "-R")) {
            return Err(format!("gh pr {cmd} {flag} isn't one the lab runs (only reading flags)"));
        }
        let value = match inline {
            Some(v) => v,
            None => {
                i += 1;
                rest.get(i).cloned().ok_or_else(|| format!("{flag} needs a value"))?
            }
        };
        let ok = match flag {
            "--head" => branch_ok(&value),
            "--state" => STATES.contains(&value.as_str()),
            "--limit" => value.parse::<u32>().is_ok_and(|n| (1..=100).contains(&n)),
            "--json" => fields_ok(&value),
            "--repo" | "-R" => value.eq_ignore_ascii_case(REPO),
            _ => false,
        };
        if !ok {
            return Err(if flag == "--repo" || flag == "-R" { format!("Only {REPO}, not {value}") } else { format!("{flag} {value} isn't one the lab runs") });
        }
        // (The repository is given once, below.)
        if flag != "--repo" && flag != "-R" {
            out.push(format!("{flag}={value}"));
        }
        i += 1;
    }
    out.push("--repo".into());
    out.push(REPO.into());
    Ok(out)
}

/// `gh.exe` as the lab names it: an absolute path to GitHub CLI's own program.
fn gh_ok(path: &std::path::Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    path.is_absolute() && (name == "gh.exe" || name == "gh") && path.is_file()
}

/// `yap.exe lab-gh <gh.exe> …`'s entry: `args` are what follows `lab-gh`.
/// gh's own exit code (its output passed through), 2 when refused.
pub fn main(args: Vec<String>) -> i32 {
    let Some((gh, rest)) = args.split_first() else {
        eprintln!("usage: yap.exe lab-gh <gh.exe> pr list|view …");
        return 2;
    };
    let gh = std::path::PathBuf::from(gh);
    if !gh_ok(&gh) {
        eprintln!("lab-gh: not GitHub's gh: {}", gh.display());
        return 2;
    }
    let run = match allowed(rest) {
        Ok(a) => a,
        Err(why) => {
            eprintln!("lab-gh: refused: {why}");
            return 2;
        }
    };
    match std::process::Command::new(&gh).args(&run).stdin(std::process::Stdio::null()).status() {
        Ok(s) => s.code().unwrap_or(1),
        Err(e) => {
            eprintln!("lab-gh: gh didn't start ({e})");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn yaps_own_list_runs_on_yaps_repository() {
        // Exactly what prs.rs asks.
        let asked = crate::agents::prs::args("claude/lab-untested");
        let run = allowed(&asked).unwrap();
        assert_eq!(run, s(&["pr", "list", "--head=claude/lab-untested", "--state=all", "--limit=20", "--json=number,title,state,isDraft,url,statusCheckRollup", "--repo", REPO]));
        // One pull request, by number, branch or its link.
        assert_eq!(allowed(&s(&["pr", "view", "140", "--json", "title,state"])).unwrap(), s(&["pr", "view", "140", "--json=title,state", "--repo", REPO]));
        assert!(allowed(&s(&["pr", "view", "claude/elastic-heyrovsky-69fd45"])).is_ok());
        assert!(allowed(&s(&["pr", "view", "https://github.com/nayballs/Yap/pull/7"])).is_ok());
        // Its own repository named again: the same.
        assert!(allowed(&s(&["pr", "list", "--repo", "nayballs/Yap"])).is_ok());
    }

    #[test]
    fn anything_that_could_change_or_open_something_is_refused() {
        for bad in [
            &["pr", "merge", "140"][..],
            &["pr", "close", "140"],
            &["pr", "create", "--fill"],
            &["pr", "edit", "140", "--title", "x"],
            &["pr", "comment", "140", "--body", "x"],
            &["pr", "checkout", "140"],
            &["api", "repos/nayballs/Yap"],
            &["repo", "delete"],
            &["auth", "token"],
            &["pr", "view", "140", "--web"],
            &["pr", "list", "-w"],
            &["pr", "list", "--jq", ".[]"],
            &["pr", "list", "--template", "{{.}}"],
            &["pr", "list", "--repo", "someone/else"],
            &["pr", "list", "-R", "someone/else"],
            &["pr", "list", "--head", "--web"],
            &["pr", "list", "--state", "everything"],
            &["pr", "list", "--limit", "100000"],
            &["pr", "list", "--json", "title;rm"],
            &["pr", "view", "140", "141"],
            &["pr", "view", "https://github.com/someone/else/pull/1"],
            &["pr", "list", "stray"],
            &["pr"],
            &[],
        ] {
            assert!(allowed(&s(bad)).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn only_github_clis_own_program_is_started() {
        assert!(!gh_ok(std::path::Path::new("gh.exe")), "never a relative one");
        assert!(!gh_ok(&std::env::temp_dir().join("not-gh.exe")));
    }
}

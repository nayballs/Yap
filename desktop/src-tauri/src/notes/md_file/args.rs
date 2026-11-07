//! Which of a launch's arguments are Markdown files to open: Explorer's
//! "Open with" (`yap.exe "C:\…\plan.md"`, the installer's `%1`), `yap.exe
//! <path>` from a script or an assistant, and a second launch's arguments
//! handed to the running Yap by single-instance (with the folder it was
//! started in, for a relative path).
//!
//! Never one of Yap's own: its flags (`--autostart`), a deep link
//! (`com.contextmirror.yap://…`), or a subcommand's arguments (`yap.exe
//! eval …`, which `main.rs` runs before the app). Only a `.md` or
//! `.markdown` file that exists.

use std::path::{Path, PathBuf};

/// The subcommands `main.rs` runs instead of the app: their arguments are
/// theirs.
const SUBCOMMANDS: &[&str] = &["mcp", "diarize", "eval", "bench", "call-dump", "snip-dump", "partials-dump", "dictation-check"];

/// A Markdown file's name: `.md` or `.markdown`, any case.
pub fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
}

/// The arguments (the exe first, as `std::env::args` gives them) that name
/// a Markdown file, made absolute against `cwd`, each once. Pure: whether
/// they exist is [`existing`]'s.
pub fn candidates(args: &[String], cwd: &Path) -> Vec<PathBuf> {
    let rest = args.get(1..).unwrap_or_default();
    if rest.first().is_some_and(|a| SUBCOMMANDS.contains(&a.as_str())) {
        return Vec::new();
    }
    let mut out: Vec<PathBuf> = Vec::new();
    for arg in rest {
        let arg = arg.trim().trim_matches('"');
        // A flag (`--autostart`, `-x`), a URL or a deep link: never a file.
        if arg.is_empty() || arg.starts_with('-') || arg.contains("://") {
            continue;
        }
        let path = Path::new(arg);
        if !is_markdown(path) {
            continue;
        }
        let abs = if path.is_absolute() { path.to_path_buf() } else { cwd.join(path) };
        let key = abs.to_string_lossy().to_lowercase();
        if !out.iter().any(|p| p.to_string_lossy().to_lowercase() == key) {
            out.push(abs);
        }
    }
    out
}

/// [`candidates`] that are files on disk.
pub fn existing(args: &[String], cwd: &Path) -> Vec<PathBuf> {
    candidates(args, cwd).into_iter().filter(|p| p.is_file()).collect()
}

/// This launch's Markdown files (`std::env::args`, the folder Yap started in).
pub fn at_launch() -> Vec<PathBuf> {
    let args: Vec<String> = std::env::args().collect();
    let cwd = std::env::current_dir().unwrap_or_default();
    existing(&args, &cwd)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        std::iter::once("C:\\Program Files\\Yap\\yap.exe").chain(list.iter().copied()).map(String::from).collect()
    }

    #[cfg(windows)]
    const CWD: &str = r"C:\Work\notes";
    #[cfg(not(windows))]
    const CWD: &str = "/work/notes";

    #[test]
    fn markdown_files_by_their_extension_any_case() {
        assert!(is_markdown(Path::new("plan.md")));
        assert!(is_markdown(Path::new("Plan.MD")));
        assert!(is_markdown(Path::new("notes.markdown")));
        assert!(is_markdown(Path::new("notes.Markdown")));
        assert!(!is_markdown(Path::new("plan.txt")));
        assert!(!is_markdown(Path::new("plan.md.exe")));
        assert!(!is_markdown(Path::new("md")));
        assert!(!is_markdown(Path::new(".md")), "a dotfile named .md has no extension");
    }

    #[test]
    fn yaps_own_arguments_are_skipped() {
        let cwd = Path::new(CWD);
        assert!(candidates(&args(&[]), cwd).is_empty());
        assert!(candidates(&args(&["--autostart"]), cwd).is_empty());
        assert!(candidates(&args(&["com.contextmirror.yap://auth/callback#token=x.md"]), cwd).is_empty());
        assert!(candidates(&args(&["https://example.com/readme.md"]), cwd).is_empty());
        assert!(candidates(&args(&["-flag.md"]), cwd).is_empty());
        // A subcommand's arguments are its own, even one ending in .md.
        assert!(candidates(&args(&["eval", "report.md"]), cwd).is_empty());
        assert!(candidates(&args(&["dictation-check", "run", "out.md"]), cwd).is_empty());
        // Not Markdown.
        assert!(candidates(&args(&["notes.txt", "photo.png"]), cwd).is_empty());
    }

    #[test]
    fn a_path_is_made_absolute_against_the_launch_folder_and_named_once() {
        let cwd = Path::new(CWD);
        let got = candidates(&args(&["--autostart", "plan.md", "sub/notes.markdown", "PLAN.md"]), cwd);
        assert_eq!(got, vec![cwd.join("plan.md"), cwd.join("sub/notes.markdown")]);
        #[cfg(windows)]
        {
            let abs = r"D:\Docs\Weekly review.md";
            assert_eq!(candidates(&args(&[abs]), cwd), vec![PathBuf::from(abs)]);
            // Quotes a shell left on.
            assert_eq!(candidates(&args(&["\"D:\\Docs\\a.md\""]), cwd), vec![PathBuf::from(r"D:\Docs\a.md")]);
        }
    }

    #[test]
    fn only_files_that_exist() {
        let dir = std::env::temp_dir().join(format!("yap-md-args-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("folder.md")).unwrap();
        std::fs::write(dir.join("there.md"), "# Here\n").unwrap();
        let got = existing(&args(&["there.md", "missing.md", "folder.md"]), &dir);
        assert_eq!(got, vec![dir.join("there.md")]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

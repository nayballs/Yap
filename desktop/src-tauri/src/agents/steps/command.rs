//! **A command as the agent's tab says it** (Bash, PowerShell). Claude Code
//! often starts one with `cd "<the agent's folder>" &&`, and writes the
//! folder in Git Bash's way (`/d/Projects/garden-planner`) as often as
//! Windows' (`D:\Projects\garden-planner`):
//!
//! - [`strip_cd`]: a leading `cd <dir> &&` (or `;`, or a new line) into the
//!   agent's folder goes; into a folder inside it, it's said relative
//!   (`cd src && npm test`). Quoted, unquoted, `/d/…`, `D:\…`, `D:/…`,
//!   `/mnt/d/…`; `cd`, `cd /d`, `Set-Location`, `pushd`.
//! - [`relative`]: the folder written anywhere in a command, said relative
//!   to it (`ls "/d/Projects/garden-planner/src"` → `ls src`).
//! - [`line`]: the step's line — the description Claude Code gave the call
//!   ("List the add-on folders" → "Listing the add-on folders"), else the
//!   command, its prefix gone, its paths relative, on one line, cut at a
//!   word.
//!
//! Pure.

use std::path::Path;

/// `p`, a path as a command writes it, with forward slashes, one at a time,
/// a drive as `d:` (Git Bash's `/d/…`, WSL's `/mnt/d/…`, Cygwin's
/// `/cygdrive/d/…`), no slash at the end; in lower case when `lower` (the
/// same length either way).
fn norm(p: &str, lower: bool) -> String {
    let p = p.trim();
    // Windows' long-path form (`\\?\D:\…`) is the same folder.
    let p = p.strip_prefix(r"\\?\").unwrap_or(p);
    let mut s = String::with_capacity(p.len());
    for c in p.chars() {
        let c = if c == '\\' { '/' } else if lower { c.to_ascii_lowercase() } else { c };
        if c == '/' && s.ends_with('/') {
            continue;
        }
        s.push(c);
    }
    for root in ["/mnt/", "/cygdrive/", "/"] {
        if let Some(rest) = s.strip_prefix(root) {
            let mut it = rest.chars();
            if let (Some(d), next) = (it.next(), it.next()) {
                if d.is_ascii_alphabetic() && matches!(next, None | Some('/')) {
                    s = format!("{}:{}", d.to_ascii_lowercase(), &rest[1..]);
                    break;
                }
            }
        }
    }
    while s.len() > 1 && s.ends_with('/') {
        s.pop();
    }
    s
}

/// `p`, a path as a command writes it, as compared ([`norm`], lower case).
pub fn key(p: &str) -> String {
    norm(p, true)
}

/// `path` relative to the agent's `folder` ("" for the folder itself, with
/// forward slashes, in the path's own letters), or `None` when it's
/// outside it.
pub fn inside(path: &str, folder: &Path) -> Option<String> {
    let f = key(&folder.to_string_lossy());
    let p = key(path);
    if f.is_empty() {
        return None;
    }
    if p == f {
        return Some(String::new());
    }
    p.strip_prefix(&f).and_then(|r| r.strip_prefix('/'))?;
    Some(norm(path, false)[f.len() + 1..].to_string())
}

/// The word at the start of `s` (after spaces): quoted (`"…"`, `'…'`) or not
/// (up to a space, `;` or `&`; `\ ` a space in it), and what follows it.
fn word(s: &str) -> Option<(String, &str)> {
    let s = s.trim_start_matches([' ', '\t']);
    let mut chars = s.char_indices().peekable();
    let (_, first) = *chars.peek()?;
    if first == '"' || first == '\'' {
        chars.next();
        for (i, c) in chars {
            if c == first {
                return Some((s[1..i].to_string(), &s[i + 1..]));
            }
        }
        return None;
    }
    let mut out = String::new();
    let mut end = s.len();
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        if escaped {
            out.push(c);
            escaped = false;
            continue;
        }
        if c == '\\' && s[i + 1..].starts_with(' ') {
            escaped = true;
            continue;
        }
        if c.is_whitespace() || c == ';' || c == '&' {
            end = i;
            break;
        }
        out.push(c);
    }
    (!out.is_empty()).then(|| (out, &s[end..]))
}

/// `command` without a leading `cd <dir> &&` into the agent's `folder` (or a
/// folder inside it, then said relative: `cd src && …`), and whether there
/// was one. Anything else is left as it is.
pub fn strip_cd(command: &str, folder: &Path) -> (String, bool) {
    let c = command.trim();
    let lower = c.to_ascii_lowercase();
    let after = ["cd ", "cd\t", "set-location ", "pushd ", "sl "].iter().find(|p| lower.starts_with(**p)).map(|p| &c[p.len()..]);
    let Some(mut rest) = after else { return (c.to_string(), false) };
    // cmd's `cd /d`, PowerShell's `-Path` / `-LiteralPath`.
    for flag in ["/d ", "-path ", "-literalpath "] {
        let t = rest.trim_start();
        if t.to_ascii_lowercase().starts_with(flag) {
            rest = &t[flag.len()..];
        }
    }
    let Some((dir, tail)) = word(rest) else { return (c.to_string(), false) };
    let Some(rel) = inside(&dir, folder) else { return (c.to_string(), false) };
    let tail = tail.trim_start_matches([' ', '\t']);
    let then = ["&&", ";", "\r\n", "\n"].iter().find(|s| tail.starts_with(**s)).map(|s| tail[s.len()..].trim_start());
    match then {
        Some(t) if !t.is_empty() => {
            if rel.is_empty() {
                (t.to_string(), true)
            } else {
                (format!("cd {} && {t}", quoted(&rel)), true)
            }
        }
        _ => (c.to_string(), false),
    }
}

/// `p` as a shell word: bare when it's plain, else in double quotes.
fn quoted(p: &str) -> String {
    if plain(p) {
        p.to_string()
    } else {
        format!("\"{p}\"")
    }
}

/// A path that needs no quotes.
fn plain(p: &str) -> bool {
    !p.is_empty() && p.chars().all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | '/' | '\\' | '+' | '@' | ',' | ':' | '~'))
}

/// How the agent's folder may be written in a command: Windows' way, with
/// forward slashes, with doubled backslashes, and Git Bash's, WSL's and
/// Cygwin's for a drive — the longest first.
fn forms(folder: &Path) -> Vec<String> {
    let raw = folder.to_string_lossy().trim().trim_start_matches(r"\\?\").trim_end_matches(['\\', '/']).to_string();
    if raw.is_empty() {
        return Vec::new();
    }
    let fwd = raw.replace('\\', "/");
    let mut out = vec![raw.clone(), fwd.clone(), raw.replace('\\', "\\\\")];
    let b = fwd.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        let d = (b[0] as char).to_ascii_lowercase();
        let rest = &fwd[2..];
        out.extend([format!("/mnt/{d}{rest}"), format!("/cygdrive/{d}{rest}"), format!("/{d}{rest}")]);
    }
    out.sort_by_key(|s| std::cmp::Reverse(s.len()));
    out.dedup();
    out
}

/// What may come just before a path in a command.
fn opens(c: Option<char>) -> bool {
    c.is_none_or(|c| c.is_whitespace() || matches!(c, '"' | '\'' | '=' | '(' | ',' | '<' | '>' | '|' | ';' | '&'))
}

/// What may come just after the folder's name when it's the folder itself.
fn closes(c: Option<char>) -> bool {
    c.is_none_or(|c| c.is_whitespace() || matches!(c, '"' | '\'' | ')' | ',' | ';' | '|' | '&' | '<' | '>'))
}

/// `text` with the agent's `folder` said relative to it wherever it's written
/// (`"/d/Projects/garden-planner/src/app.ts"` → `src/app.ts`, the folder
/// itself `.`); a quoted path that needs no quotes then loses them.
pub fn relative(text: &str, folder: &Path) -> String {
    let forms = forms(folder);
    if forms.is_empty() {
        return text.to_string();
    }
    let lower = text.to_ascii_lowercase();
    let lforms: Vec<String> = forms.iter().map(|f| f.to_ascii_lowercase()).collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    'scan: while i < text.len() {
        let before = text[..i].chars().next_back();
        if opens(before) {
            for f in &lforms {
                if !lower[i..].starts_with(f.as_str()) {
                    continue;
                }
                let mut j = i + f.len();
                let next = text[j..].chars().next();
                let is_dir = matches!(next, Some('/') | Some('\\'));
                if !is_dir && !closes(next) {
                    continue;
                }
                if is_dir {
                    j += 1;
                    if text[j..].starts_with('\\') {
                        j += 1;
                    }
                }
                // A quoted path: its rest, and the quotes gone when it needs none.
                if let Some(q @ ('"' | '\'')) = before {
                    if let Some(close) = text[j..].find(q) {
                        let rest = &text[j..j + close];
                        let shown = if rest.is_empty() { "." } else { rest };
                        if plain(shown) {
                            out.pop();
                            out.push_str(shown);
                            i = j + close + 1;
                            continue 'scan;
                        }
                    }
                }
                if !is_dir {
                    out.push('.');
                }
                i = j;
                continue 'scan;
            }
        }
        let c = text[i..].chars().next().unwrap_or(' ');
        out.push(c);
        i += c.len_utf8();
    }
    out
}

/// `s` on one line (its first, "…" when more follow), at most `max`
/// characters, cut at a space when one is near the end.
pub fn cut(s: &str, max: usize) -> String {
    let mut lines = s.trim().lines().map(str::trim).filter(|l| !l.is_empty());
    let first = lines.next().unwrap_or("");
    let flat = first.split_whitespace().collect::<Vec<_>>().join(" ");
    let more = lines.next().is_some();
    let n = flat.chars().count();
    if n <= max && !more {
        return flat;
    }
    if n <= max.saturating_sub(2) {
        return format!("{flat} …");
    }
    let keep: String = flat.chars().take(max.saturating_sub(1)).collect();
    let at = keep.rfind(' ').filter(|&i| keep[..i].chars().count() * 10 >= max * 6);
    let head = at.map_or(keep.as_str(), |i| &keep[..i]).trim_end_matches([' ', '|', '&', ';']);
    format!("{head}…")
}

/// A command's step line: what Claude Code said it does (`description`),
/// as a doing word ("List the add-on folders" → "Listing the add-on
/// folders"); else "Running" and the command, its `cd` into the folder gone,
/// its paths relative, on one line.
pub fn line(command: &str, description: Option<&str>, folder: &Path, max: usize) -> String {
    if let Some(d) = description.map(str::trim).filter(|d| !d.is_empty()) {
        return cut(&doing(d), max);
    }
    let (rest, _) = strip_cd(command, folder);
    let shown = cut(&relative(&rest, folder), max.saturating_sub("Running ".len()));
    if shown.is_empty() {
        "Running a command".to_string()
    } else {
        format!("Running {shown}")
    }
}

/// The verbs Claude Code's descriptions start with, and their doing words.
const DOING: &[(&str, &str)] = &[
    ("add", "adding"),
    ("analyse", "analysing"),
    ("analyze", "analyzing"),
    ("append", "appending"),
    ("apply", "applying"),
    ("build", "building"),
    ("bump", "bumping"),
    ("calculate", "calculating"),
    ("change", "changing"),
    ("check", "checking"),
    ("clean", "cleaning"),
    ("clear", "clearing"),
    ("clone", "cloning"),
    ("commit", "committing"),
    ("compare", "comparing"),
    ("compile", "compiling"),
    ("configure", "configuring"),
    ("confirm", "confirming"),
    ("convert", "converting"),
    ("copy", "copying"),
    ("count", "counting"),
    ("create", "creating"),
    ("delete", "deleting"),
    ("display", "displaying"),
    ("download", "downloading"),
    ("dump", "dumping"),
    ("edit", "editing"),
    ("ensure", "ensuring"),
    ("execute", "executing"),
    ("explore", "exploring"),
    ("export", "exporting"),
    ("extract", "extracting"),
    ("fetch", "fetching"),
    ("find", "finding"),
    ("fix", "fixing"),
    ("format", "formatting"),
    ("generate", "generating"),
    ("get", "getting"),
    ("inspect", "inspecting"),
    ("install", "installing"),
    ("kill", "killing"),
    ("launch", "launching"),
    ("lint", "linting"),
    ("list", "listing"),
    ("load", "loading"),
    ("locate", "locating"),
    ("look", "looking"),
    ("make", "making"),
    ("measure", "measuring"),
    ("merge", "merging"),
    ("move", "moving"),
    ("open", "opening"),
    ("parse", "parsing"),
    ("preview", "previewing"),
    ("print", "printing"),
    ("pull", "pulling"),
    ("push", "pushing"),
    ("query", "querying"),
    ("read", "reading"),
    ("rebuild", "rebuilding"),
    ("remove", "removing"),
    ("rename", "renaming"),
    ("render", "rendering"),
    ("replace", "replacing"),
    ("rerun", "rerunning"),
    ("reset", "resetting"),
    ("restart", "restarting"),
    ("restore", "restoring"),
    ("retry", "retrying"),
    ("review", "reviewing"),
    ("run", "running"),
    ("save", "saving"),
    ("scan", "scanning"),
    ("search", "searching"),
    ("see", "seeing"),
    ("set", "setting"),
    ("show", "showing"),
    ("sort", "sorting"),
    ("stage", "staging"),
    ("start", "starting"),
    ("stop", "stopping"),
    ("sync", "syncing"),
    ("test", "testing"),
    ("try", "trying"),
    ("update", "updating"),
    ("upgrade", "upgrading"),
    ("upload", "uploading"),
    ("validate", "validating"),
    ("verify", "verifying"),
    ("view", "viewing"),
    ("wait", "waiting"),
    ("watch", "watching"),
    ("write", "writing"),
];

/// A description as a doing word ("List the add-on folders" → "Listing the
/// add-on folders"), its full stop gone; one that doesn't start with a verb
/// Yap knows stays as it is.
pub fn doing(description: &str) -> String {
    let d = description.trim().trim_end_matches('.').trim_end();
    let (first, rest) = d.split_once(' ').unwrap_or((d, ""));
    let Some((_, ing)) = DOING.iter().find(|(v, _)| v.eq_ignore_ascii_case(first)) else { return d.to_string() };
    let mut word = ing.to_string();
    if first.chars().next().is_some_and(char::is_uppercase) {
        let mut c = word.chars();
        word = c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default();
    }
    if rest.is_empty() {
        word
    } else {
        format!("{word} {rest}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn folder() -> PathBuf {
        PathBuf::from(r"D:\Projects\garden-planner")
    }

    #[test]
    fn a_cd_into_the_folder_goes_in_every_way_its_written() {
        let f = folder();
        for c in [
            r#"cd "/d/Projects/garden-planner" && ls src"#,
            r#"cd '/d/Projects/garden-planner' && ls src"#,
            "cd /d/Projects/garden-planner && ls src",
            "cd /d/projects/GARDEN-PLANNER/ && ls src",
            r#"cd "D:\Projects\garden-planner" && ls src"#,
            r"cd D:\Projects\garden-planner && ls src",
            "cd D:/Projects/garden-planner; ls src",
            "cd /mnt/d/Projects/garden-planner && ls src",
            "cd \"D:\\\\Projects\\\\garden-planner\" && ls src",
            "cd /d \"D:\\Projects\\garden-planner\" && ls src",
            "Set-Location -LiteralPath 'D:\\Projects\\garden-planner'; ls src",
            "cd \"/d/Projects/garden-planner\"\nls src",
        ] {
            assert_eq!(strip_cd(c, &f), ("ls src".to_string(), true), "{c}");
        }
        // Into a folder inside it: said relative.
        assert_eq!(strip_cd(r#"cd "/d/Projects/garden-planner/src" && npm test"#, &f).0, "cd src && npm test");
        assert_eq!(strip_cd(r#"cd "D:\Projects\garden-planner\My Beds" && ls"#, &f).0, r#"cd "My Beds" && ls"#);
        assert_eq!(strip_cd(r"cd /d/Projects/garden-planner/My\ Beds && ls", &f).0, r#"cd "My Beds" && ls"#);
        // Anywhere else, or nothing after it: as it is.
        for c in [
            "cd /d/Projects/garden-planner-old && ls",
            "cd /e/Elsewhere && ls",
            "cd /d/Projects/garden-planner",
            "cd /d/Projects/garden-planner &&",
            "npm test && cd /d/Projects/garden-planner",
            "cdx /d/Projects/garden-planner && ls",
            r#"cd "/d/Projects/garden-planner && ls"#,
        ] {
            assert_eq!(strip_cd(c, &f), (c.trim().to_string(), false), "{c}");
        }
    }

    #[test]
    fn the_folder_in_a_command_is_said_relative_to_it() {
        let f = folder();
        assert_eq!(relative(r#"ls "/d/Projects/garden-planner/src""#, &f), "ls src");
        assert_eq!(relative(r"type D:\Projects\garden-planner\notes.md", &f), r"type notes.md");
        assert_eq!(relative("cat d:/projects/garden-planner/beds/a.json | head -5", &f), "cat beds/a.json | head -5");
        assert_eq!(relative(r#"ls "D:\Projects\garden-planner""#, &f), "ls .");
        assert_eq!(relative("ls /d/Projects/garden-planner", &f), "ls .");
        // Quotes kept where they're needed.
        assert_eq!(relative(r#"cat "/d/Projects/garden-planner/My Beds/a.md""#, &f), r#"cat "My Beds/a.md""#);
        // A sibling with a longer name isn't the folder; nor a path ending in its name.
        assert_eq!(relative("ls /d/Projects/garden-planner-old/x", &f), "ls /d/Projects/garden-planner-old/x");
        assert_eq!(relative("ls /backup/d/Projects/garden-planner/x", &f), "ls /backup/d/Projects/garden-planner/x");
        assert_eq!(relative("grep -rn TODO --include=*.ts /d/Projects/garden-planner/src", &f), "grep -rn TODO --include=*.ts src");
        assert_eq!(relative("echo héllo > /d/Projects/garden-planner/out.txt", &f), "echo héllo > out.txt");
        assert_eq!(key("/d/Projects/x/"), "d:/projects/x");
        assert_eq!(key(r"D:\\Projects\\x"), "d:/projects/x");
        assert_eq!(key("/mnt/c/x"), "c:/x");
        assert_eq!(key("/home/sam/x"), "/home/sam/x");
        assert_eq!(key(r"\\?\D:\Projects\x"), "d:/projects/x");
        assert_eq!(inside("/d/Projects/garden-planner/Src/App.ts", &f).as_deref(), Some("Src/App.ts"));
        assert_eq!(inside(r"D:\Projects\garden-planner", &f).as_deref(), Some(""));
        assert_eq!(inside("/d/Projects/other", &f), None);
    }

    #[test]
    fn a_commands_line_uses_its_description_else_the_command_tidied() {
        let f = folder();
        assert_eq!(line("ls src", Some("List the add-on folders"), &f, 90), "Listing the add-on folders");
        assert_eq!(line("npm test", Some("Run tests."), &f, 90), "Running tests");
        assert_eq!(line("git status", Some("Git status for the repo"), &f, 90), "Git status for the repo");
        assert_eq!(line("npm test", Some("   "), &f, 90), "Running npm test");
        assert_eq!(line(r#"cd "/d/Projects/garden-planner" && ls src | grep -i bed"#, None, &f, 90), "Running ls src | grep -i bed");
        assert_eq!(line(r#"cd "/d/Projects/garden-planner" && cat "/d/Projects/garden-planner/beds.json""#, None, &f, 90), "Running cat beds.json");
        assert_eq!(line("", None, &f, 90), "Running a command");
        // Long ones cut at a word, a second line said with "…".
        let long = format!("cd /d/Projects/garden-planner && grep -rn {} src", "watering ".repeat(20));
        let l = line(&long, None, &f, 60);
        assert!(l.starts_with("Running grep -rn watering") && l.ends_with("watering…") && l.chars().count() <= 60, "{l}");
        assert_eq!(line("python - <<'EOF'\nprint(1)\nEOF", None, &f, 90), "Running python - <<'EOF' …");
        assert_eq!(doing("show the beds"), "showing the beds");
        assert_eq!(doing("Frobnicate the beds"), "Frobnicate the beds");
        assert_eq!(cut("a b c", 10), "a b c");
        assert_eq!(cut("abcdefghijklmnop", 6), "abcde…");
    }
}

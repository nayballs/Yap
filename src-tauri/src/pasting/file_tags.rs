//! **File tagging in AI chat boxes** (2026-10-29; Wispr Flow's for Cursor
//! and Windsurf, by behaviour): saying a file's name in an editor's AI chat
//! box makes it the chat's reference to that file, as typing "@" and
//! picking it would. The names are the dictation's code names
//! (`dictionary/screen_terms/code.rs`: the editor's tabs, file tree and
//! title), already respelled ("user service dot ts" → `userService.ts`).
//!
//! - **Cursor** and **Windsurf**: "@" and the name typed, then **Tab**
//!   picks it from the list (their "@" menus take Tab as Enter does; Enter
//!   would send the prompt if the list had closed, Tab never does).
//! - **VS Code** (Copilot Chat): "#" and the name: VS Code's chat takes
//!   files after "#" ("@" is for its chat participants, "@workspace").
//! - In each, the key goes only once UI Automation shows the list's
//!   highlighted entry is that file; else the typed "@userService.ts" stays
//!   as text — never Enter, never a key into the code editor.
//! - **Claude Code** and **Codex** in a terminal: "@" and the file's path
//!   as the screen showed it ("@src/services/userService.ts"), as plain
//!   text in the one paste — both read a file named so when the prompt is
//!   sent (Claude Code attaches it, Codex's model opens it). A file the
//!   screen showed with no path, or with several, keeps its name alone.
//! - Only in those chat boxes and CLIs (`code::focus_of`), never in the
//!   editor, never in a plain shell (`@` means nothing to it).
//!
//! What counts ([`find`], pure): a file's whole name, as a word of its own
//! (not inside a path), optionally after "@", "tag" or a clause-starting
//! "at" (taken off); its name without the extension only after one of
//! those, and only when one file has that stem.

use crate::dictionary::screen_terms::code::{App, CodeFile, CodeNames, Focus};
use crate::pasting::app_kind::CodingCli;

/// How a file is tagged where the dictation goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tagging {
    /// Typed after `trigger` and picked from the app's list with Tab.
    Pick { trigger: char },
    /// "@path" written into the text (a coding CLI).
    Path,
}

/// How files are tagged in this dictation's window, if at all: Cursor's and
/// Windsurf's chat boxes "@", VS Code's "#", Claude Code and Codex "@path".
pub fn tagging(names: &CodeNames) -> Option<Tagging> {
    match (names.app, names.focus) {
        (App::Cursor | App::Windsurf, Focus::Chat) => Some(Tagging::Pick { trigger: '@' }),
        (App::VsCode, Focus::Chat) => Some(Tagging::Pick { trigger: '#' }),
        (App::Terminal(Some(CodingCli::ClaudeCode | CodingCli::Codex)), _) => Some(Tagging::Path),
        _ => None,
    }
}

/// A file named in a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    /// Its byte range in the text (with an "@", "tag " or "at " before it).
    pub start: usize,
    pub end: usize,
    /// The file's name as listed.
    pub name: String,
    /// The path it was shown with, when just one.
    pub path: Option<String>,
}

/// A file name's own boundaries: not inside a path or a longer word.
fn bounded(text: &str, start: usize, end: usize) -> bool {
    let before = text[..start].chars().next_back();
    let after = text[end..].chars().next();
    let ok_before = before.is_none_or(|c| c.is_whitespace() || "(\"'`“‘@[".contains(c));
    let ok_after = match after {
        None => true,
        Some('.') => text[end + 1..].chars().next().is_none_or(char::is_whitespace),
        Some(c) => c.is_whitespace() || ",;:!?)\"'`”’]".contains(c),
    };
    ok_before && ok_after
}

/// What's before a name that calls it a tag: "@", "tag ", "tagged " or a
/// clause-starting "at ". The byte where it starts, if so.
fn lead_in(text: &str, start: usize) -> Option<usize> {
    let head = &text[..start];
    if head.ends_with('@') {
        return Some(start - 1);
    }
    let trimmed = head.strip_suffix(' ')?;
    let word_start = trimmed.rfind(|c: char| !c.is_alphanumeric()).map_or(0, |i| i + 1);
    let word = trimmed[word_start..].to_lowercase();
    let clause = || {
        let before = trimmed[..word_start].trim_end();
        before.is_empty() || before.ends_with(['.', '!', '?', ',', ';', ':', '\n'])
    };
    match word.as_str() {
        "tag" | "tagged" => Some(word_start),
        "at" if clause() => Some(word_start),
        _ => None,
    }
}

/// The files named in `text` (`files`: the code names'), in order. Pure.
pub fn find(text: &str, files: &[CodeFile]) -> Vec<Tag> {
    let lower = text.to_lowercase();
    // Lower-casing can change a string's length in rare scripts; then the
    // byte positions don't line up, and nothing is tagged.
    if lower.len() != text.len() {
        return Vec::new();
    }
    let mut found: Vec<Tag> = Vec::new();
    let mut by_length: Vec<&CodeFile> = files.iter().collect();
    by_length.sort_by_key(|f| std::cmp::Reverse(f.name.len()));
    let taken = |found: &[Tag], s: usize, e: usize| found.iter().any(|t| s < t.end && t.start < e);
    // Whole names ("userService.ts").
    for f in &by_length {
        let key = f.name.to_lowercase();
        let mut from = 0;
        while let Some(pos) = lower[from..].find(&key) {
            let (s, e) = (from + pos, from + pos + key.len());
            from = e;
            if !bounded(text, s, e) || taken(&found, s, e) {
                continue;
            }
            let start = lead_in(text, s).unwrap_or(s);
            found.push(Tag { start, end: e, name: f.name.clone(), path: f.path().map(str::to_string) });
        }
    }
    // A stem alone ("at user service", "@userService"), after a lead-in.
    for f in files {
        let Some((stem, _)) = f.name.rsplit_once('.') else { continue };
        if stem.is_empty() || files.iter().filter(|g| g.name.rsplit_once('.').is_some_and(|(s, _)| s.eq_ignore_ascii_case(stem))).count() != 1 {
            continue;
        }
        let key = stem.to_lowercase();
        let mut from = 0;
        while let Some(pos) = lower[from..].find(&key) {
            let (s, e) = (from + pos, from + pos + key.len());
            from = e;
            if !bounded(text, s, e) || taken(&found, s, e) {
                continue;
            }
            if let Some(start) = lead_in(text, s) {
                found.push(Tag { start, end: e, name: f.name.clone(), path: f.path().map(str::to_string) });
            }
        }
    }
    found.sort_by_key(|t| t.start);
    found
}

/// `text` with each tag written "@path" (or "@name" without a single path):
/// a coding CLI's form.
pub fn as_paths(text: &str, tags: &[Tag]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for t in tags {
        out.push_str(&text[at..t.start]);
        out.push('@');
        out.push_str(t.path.as_deref().unwrap_or(&t.name));
        at = t.end;
    }
    out.push_str(&text[at..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(list: &[(&str, &[&str])]) -> Vec<CodeFile> {
        list.iter().map(|(n, p)| CodeFile { name: n.to_string(), paths: p.iter().map(|s| s.to_string()).collect() }).collect()
    }

    fn names_of(text: &str, f: &[CodeFile]) -> Vec<String> {
        find(text, f).into_iter().map(|t| text[t.start..t.end].to_string()).collect()
    }

    #[test]
    fn a_file_named_is_a_tag() {
        let f = files(&[("userService.ts", &[]), ("index.ts", &[]), ("README.md", &[])]);
        assert_eq!(names_of("Fix the bug in userService.ts please", &f), ["userService.ts"]);
        assert_eq!(names_of("Look at userService.ts and index.ts.", &f), ["userService.ts", "index.ts"]);
        assert_eq!(names_of("At userService.ts, add a cache", &f), ["At userService.ts"]);
        assert_eq!(names_of("tag readme.md then explain", &f), ["tag readme.md"]);
        assert_eq!(names_of("see @userService.ts", &f), ["@userService.ts"]);
        // Inside a path or a longer name: not this file.
        assert!(names_of("open src/userService.ts", &f).is_empty());
        assert!(names_of("the userService.test.ts suite", &f).is_empty());
        assert!(names_of("use myindex.ts", &f).is_empty());
    }

    #[test]
    fn a_stem_alone_only_after_a_lead_in_and_only_one_file() {
        let f = files(&[("userService.ts", &[]), ("index.ts", &[]), ("index.css", &[])]);
        assert_eq!(names_of("At userService, add a cache", &f), ["At userService"]);
        assert_eq!(names_of("tag userservice and go", &f), ["tag userservice"]);
        // No lead-in: the class, not the file.
        assert!(names_of("the userService class", &f).is_empty());
        // Two files have the stem "index": which one isn't known.
        assert!(names_of("tag index and go", &f).is_empty());
    }

    #[test]
    fn a_coding_cli_gets_the_path_the_screen_showed() {
        let f = files(&[("userService.ts", &["src/services/userService.ts"]), ("index.ts", &["src/index.ts", "web/index.ts"])]);
        let text = "Fix the bug in userService.ts and index.ts";
        assert_eq!(as_paths(text, &find(text, &f)), "Fix the bug in @src/services/userService.ts and @index.ts");
    }

    #[test]
    fn tagging_happens_only_in_a_chat_box_or_a_coding_cli() {
        let n = |app, focus| CodeNames::from_lists(app, focus, Vec::new(), Vec::new());
        assert_eq!(tagging(&n(App::Cursor, Focus::Chat)), Some(Tagging::Pick { trigger: '@' }));
        assert_eq!(tagging(&n(App::Windsurf, Focus::Chat)), Some(Tagging::Pick { trigger: '@' }));
        assert_eq!(tagging(&n(App::VsCode, Focus::Chat)), Some(Tagging::Pick { trigger: '#' }));
        assert_eq!(tagging(&n(App::Cursor, Focus::Editor)), None);
        assert_eq!(tagging(&n(App::Cursor, Focus::Terminal)), None);
        assert_eq!(tagging(&n(App::VisualStudio, Focus::Chat)), None);
        assert_eq!(tagging(&n(App::Terminal(Some(CodingCli::ClaudeCode)), Focus::Terminal)), Some(Tagging::Path));
        assert_eq!(tagging(&n(App::Terminal(Some(CodingCli::Codex)), Focus::Terminal)), Some(Tagging::Path));
        assert_eq!(tagging(&n(App::Terminal(None), Focus::Terminal)), None);
    }
}

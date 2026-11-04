//! **Import a file** (Settings → Memory → "Import a file…", 2026-11-03):
//! another assistant's memory export, or Yap's own, read and mapped by Yap
//! itself. No AI model is asked, so nothing leaves this PC.
//!
//! - **Claude's memory export** (the Claude app's Settings → Your Claude →
//!   Memory → Export memory): a .zip of Markdown, `claude-memory-<date>/`
//!   holding `preferences.md`, `profile.md`, `people/<slug>.md`,
//!   `topics/<slug>.md`, `areas/<slug>.md` and a Claude Project's own
//!   `projects/<Project name>/topics/<slug>.md`. Each file is YAML front
//!   matter (`name`, `description`), then its facts as `- ` lines. The
//!   folder unzipped is read the same way, and so is one `.md` file of it.
//! - **Yap's own export** (`export.rs`: `You.md`, `People.md`,
//!   `Projects.md`, `Topics.md`): memory moving to another PC, or a backup.
//!
//! **The mapping** ([`mapped`]), the same every time: `preferences.md` →
//! Preferences and `profile.md` → Profile (their facts into the one item
//! each; the file's description, a summary of its facts, left out);
//! `people/` → People (the name as the description writes it, else the
//! slug in capitals: "sam-reed" → "Sam Reed"; "about": the description);
//! `topics/` → Topics; `areas/` → Projects; **a Claude Project's topics →
//! Topics whose "about" names the project** ("… (Claude Project “Kitchen
//! refit”)"), not one Yap project: a Claude Project holds several topic
//! files, each with its own description, and one Yap item keeps 8 facts;
//! its `people/` are People. A file in a folder Yap doesn't know is a
//! Topic named after its folder; one at the top it doesn't know, a Topic
//! named after the file. A fact longer than memory keeps is cut where its
//! sentences or clauses end ([`fit`]), never mid-word, so nothing is lost
//! to the cut; a nested bullet joins its parent ("Kids: Ava, Leo").
//!
//! **Merged with what Yap has** ([`changes`]): the same person, project or
//! topic (by name, whatever the case and accents; a person's first name as
//! `ops::same_thing` has it) gets only the facts it doesn't have, and an
//! "about" only when it had none. The command then leaves out what About
//! you already says (`ops::drop_about_you`) and keeps the changes as a
//! proposal (`learn::propose_file`) for the import's review, applied as one
//! batch (one Undo) only when the person says so.
//!
//! **Reading** ([`read`]) is bounded: a .zip ≤ 64 MB with ≤ 2,000 entries,
//! a file ≤ 1 MB, all of them ≤ 16 MB once inflated (read with a limit,
//! whatever a zip's headers claim); only Markdown is read; an entry named
//! to land outside the archive (`../`, an absolute path or a drive: zip
//! slip) refuses the whole archive — nothing is ever written to disk, but
//! such a zip isn't an export; a folder's links aren't followed. The log
//! says counts only, never a name, a fact or a path.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use super::ops::{self, Action, Change, Item, Section};

/// The biggest .zip read (Claude's export of a long memory is a few KB).
pub const MAX_ARCHIVE: u64 = 64 * 1024 * 1024;
/// The biggest Markdown file read.
pub const MAX_FILE: u64 = 1024 * 1024;
/// All the files read, once inflated.
pub const MAX_TOTAL: u64 = 16 * 1024 * 1024;
/// The most files (a zip's entries, a folder's Markdown files).
pub const MAX_FILES: usize = 2_000;
/// How deep a folder is read, and how many entries are looked at in it.
const MAX_DEPTH: usize = 8;
const MAX_VISITS: usize = 20_000;

const SLIP: &str = "That .zip has a file named to land outside it, so it isn't a memory export: Yap didn't read any of it.";
const DAMAGED: &str = "That .zip can't be read: it may be damaged or password-protected.";
const TOO_BIG: &str = "That's too big to be a memory export.";
const TOO_MANY: &str = "That has too many files to be a memory export.";
const NOT_AN_EXPORT: &str =
    "Yap reads Claude's memory export (the .zip, its folder unzipped, or one .md file from it) and Yap's own memory export (.zip).";

/// One Markdown file of an import: where it sat (its folders and its
/// name, the export's own folder taken off: [`strip_root`]) and its text.
#[derive(Debug, Clone, PartialEq)]
pub struct Doc {
    pub path: Vec<String>,
    pub text: String,
}

/// What a file import read: its Markdown files, and how many others were
/// left (not Markdown, too big, a link).
#[derive(Debug, Default)]
pub struct Read {
    pub docs: Vec<Doc>,
    pub skipped: usize,
}

// ---- reading -----------------------------------------------------------------------------------

/// Read a .zip, a folder or one Markdown file the person picked or dropped.
pub fn read(path: &Path) -> Result<Read, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("Couldn't read that: {e}"))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if meta.is_dir() {
        let mut out = Read::default();
        // A folder that is one of the export's own ("people"): its files are its.
        let base: Vec<String> = Some(name).filter(|n| known_folder(n)).into_iter().collect();
        let (mut total, mut visits) = (0, 0);
        walk(path, &base, 0, &mut out, &mut total, &mut visits)?;
        out.docs = strip_root(out.docs);
        return Ok(out);
    }
    let lower = name.to_lowercase();
    if is_markdown(&lower) {
        let bytes = read_capped(path)?;
        return Ok(Read { docs: vec![Doc { path: vec![name], text: crate::dictionary::files::decode(&bytes) }], skipped: 0 });
    }
    if meta.len() > MAX_ARCHIVE {
        return Err(TOO_BIG.into());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("Couldn't read that: {e}"))?;
    if lower.ends_with(".zip") || bytes.starts_with(b"PK\x03\x04") {
        return read_zip(&bytes);
    }
    Err(NOT_AN_EXPORT.into())
}

/// A .zip's Markdown files (`bytes`: the archive). Pure.
pub fn read_zip(bytes: &[u8]) -> Result<Read, String> {
    if bytes.len() as u64 > MAX_ARCHIVE {
        return Err(TOO_BIG.into());
    }
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| DAMAGED.to_string())?;
    if zip.len() > MAX_FILES {
        return Err(TOO_MANY.into());
    }
    // Zip slip first, over every entry: such an archive is refused whole.
    for i in 0..zip.len() {
        let f = zip.by_index_raw(i).map_err(|_| DAMAGED.to_string())?;
        let raw = f.name().map(|n| n.into_owned()).map_err(|_| DAMAGED.to_string())?;
        if unsafe_name(&raw) || f.enclosed_name().is_none() {
            return Err(SLIP.into());
        }
    }
    let mut out = Read::default();
    let mut total: u64 = 0;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|_| DAMAGED.to_string())?;
        if f.is_dir() {
            continue;
        }
        let raw = f.name().map(|n| n.into_owned()).map_err(|_| DAMAGED.to_string())?;
        let parts = parts_of(&raw);
        if parts.is_empty() || ignored(&parts) {
            continue;
        }
        let last = parts.last().map(|p| p.to_lowercase()).unwrap_or_default();
        if f.is_symlink() || !is_markdown(&last) || f.size() > MAX_FILE {
            out.skipped += 1;
            continue;
        }
        let mut buf = Vec::new();
        f.by_ref().take(MAX_FILE + 1).read_to_end(&mut buf).map_err(|_| DAMAGED.to_string())?;
        if buf.len() as u64 > MAX_FILE {
            out.skipped += 1;
            continue;
        }
        total += buf.len() as u64;
        if total > MAX_TOTAL {
            return Err(TOO_BIG.into());
        }
        out.docs.push(Doc { path: parts, text: crate::dictionary::files::decode(&buf) });
    }
    out.docs = strip_root(out.docs);
    Ok(out)
}

/// A zip entry's name that would land outside the archive: `..`, an
/// absolute path, a drive or a NUL.
fn unsafe_name(raw: &str) -> bool {
    let b = raw.as_bytes();
    raw.starts_with(['/', '\\'])
        || raw.contains('\0')
        || (b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':')
        || raw.split(['/', '\\']).any(|p| p.trim() == "..")
}

/// A path's parts ("a/b/c.md" → a, b, c.md), "." and empty ones left out.
fn parts_of(raw: &str) -> Vec<String> {
    raw.split(['/', '\\']).filter(|p| !p.is_empty() && *p != ".").map(str::to_string).collect()
}

/// macOS's and dotfiles' entries: never part of an export, never counted.
fn ignored(parts: &[String]) -> bool {
    parts.iter().any(|p| p.starts_with('.') || p == "__MACOSX")
}

fn is_markdown(lower: &str) -> bool {
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

/// A folder name Claude's export uses for its own things.
fn known_folder(name: &str) -> bool {
    matches!(name.to_lowercase().as_str(), "people" | "topics" | "areas" | "projects")
}

/// One file, at most [`MAX_FILE`] bytes of it (more is refused, whatever
/// the file system says its size is).
fn read_capped(path: &Path) -> Result<Vec<u8>, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("Couldn't read that: {e}"))?;
    let mut buf = Vec::new();
    f.take(MAX_FILE + 1).read_to_end(&mut buf).map_err(|e| format!("Couldn't read that: {e}"))?;
    if buf.len() as u64 > MAX_FILE {
        return Err(TOO_BIG.into());
    }
    Ok(buf)
}

/// A folder's Markdown files, its folders in turn (in name order), links
/// never followed.
fn walk(dir: &Path, prefix: &[String], depth: usize, out: &mut Read, total: &mut u64, visits: &mut usize) -> Result<(), String> {
    if depth > MAX_DEPTH {
        return Ok(());
    }
    let mut entries: Vec<std::fs::DirEntry> =
        std::fs::read_dir(dir).map_err(|e| format!("Couldn't read that folder: {e}"))?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        *visits += 1;
        if *visits > MAX_VISITS {
            return Err(TOO_MANY.into());
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "__MACOSX" {
            continue;
        }
        let Ok(kind) = e.file_type() else {
            out.skipped += 1;
            continue;
        };
        let mut path = prefix.to_vec();
        path.push(name.clone());
        if kind.is_symlink() {
            out.skipped += 1;
        } else if kind.is_dir() {
            walk(&e.path(), &path, depth + 1, out, total, visits)?;
        } else if !is_markdown(&name.to_lowercase()) {
            out.skipped += 1;
        } else {
            if out.docs.len() >= MAX_FILES {
                return Err(TOO_MANY.into());
            }
            let Ok(bytes) = read_capped(&e.path()) else {
                out.skipped += 1;
                continue;
            };
            *total += bytes.len() as u64;
            if *total > MAX_TOTAL {
                return Err(TOO_BIG.into());
            }
            out.docs.push(Doc { path, text: crate::dictionary::files::decode(&bytes) });
        }
    }
    Ok(())
}

/// The export's own folder taken off ("claude-memory-2026-11-03/people/…"
/// → "people/…"): a first folder every file shares, unless it's one of the
/// export's own (`people`, `topics`, `areas`, `projects`). None at all (a
/// zip of the files themselves) is fine too. Pure.
pub fn strip_root(mut docs: Vec<Doc>) -> Vec<Doc> {
    loop {
        let Some(first) = docs.first().and_then(|d| d.path.first()).cloned() else {
            return docs;
        };
        let shared = docs.iter().all(|d| d.path.len() > 1 && d.path[0] == first);
        if !shared || known_folder(&first) {
            return docs;
        }
        for d in docs.iter_mut() {
            d.path.remove(0);
        }
    }
}

// ---- one file ----------------------------------------------------------------------------------

/// A Claude memory file, read: its front matter's `name` and
/// `description`, its facts, and the lines Yap couldn't place (by kind:
/// "plain text", "a table", "a quote" — never their words).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Parsed {
    pub name: String,
    pub description: String,
    pub facts: Vec<String>,
    pub unplaced: Vec<&'static str>,
    pub front_matter: bool,
}

/// Read one Claude memory file, tolerantly: a BOM, CRLF, no front matter,
/// other keys (quoted or not, a `>` block), `*` / `+` / numbered bullets,
/// a checkbox, bold, a nested bullet (joined to its parent), a bullet
/// going on over an indented line. Pure.
pub fn parse_claude(text: &str) -> Parsed {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    let mut p = Parsed::default();
    let mut at = 0;
    while at < lines.len() && lines[at].trim().is_empty() {
        at += 1;
    }
    if at < lines.len() && lines[at].trim() == "---" {
        match (at + 1..lines.len()).find(|&i| matches!(lines[i].trim(), "---" | "...")) {
            Some(end) => {
                front_matter(&lines[at + 1..end], &mut p);
                p.front_matter = true;
                at = end + 1;
            }
            // A rule with no end: no front matter, the rule left out.
            None => at += 1,
        }
    }
    body(&lines[at..], &mut p);
    p
}

/// The front matter's `name` (or `title`) and `description` (or
/// `summary`, `about`); other keys are left alone.
fn front_matter(lines: &[&str], p: &mut Parsed) {
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        i += 1;
        if line.starts_with([' ', '\t']) || line.trim_start().starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().to_lowercase();
        let value = value.trim();
        let block = value.is_empty() || matches!(value, "|" | ">" | "|-" | ">-" | "|+" | ">+");
        let mut more = Vec::new();
        while i < lines.len() && lines[i].starts_with([' ', '\t']) {
            more.push(lines[i].trim());
            i += 1;
        }
        let value = if block {
            more.join(" ")
        } else {
            unquote(&std::iter::once(value).chain(more).collect::<Vec<_>>().join(" "))
        };
        let value = flat(&value);
        match key.as_str() {
            "name" | "title" if p.name.is_empty() => p.name = value,
            "description" | "summary" | "about" if p.description.is_empty() => p.description = value,
            _ => {}
        }
    }
}

/// A YAML scalar without its quotes: "…" with its escapes, '…' with ''
/// for a quote, else as written.
fn unquote(v: &str) -> String {
    let v = v.trim();
    let mut chars = v.chars();
    match chars.next() {
        Some('"') => {
            let mut out = String::new();
            while let Some(c) = chars.next() {
                match c {
                    '"' => break,
                    '\\' => match chars.next() {
                        Some('n' | 't' | 'r') => out.push(' '),
                        Some('u') => {
                            let hex: String = chars.by_ref().take(4).collect();
                            if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                                out.push(ch);
                            }
                        }
                        Some(other) => out.push(other),
                        None => {}
                    },
                    c => out.push(c),
                }
            }
            out
        }
        Some('\'') => {
            let mut out = String::new();
            let rest: Vec<char> = chars.collect();
            let mut i = 0;
            while i < rest.len() {
                if rest[i] == '\'' {
                    if rest.get(i + 1) == Some(&'\'') {
                        out.push('\'');
                        i += 2;
                        continue;
                    }
                    break;
                }
                out.push(rest[i]);
                i += 1;
            }
            out
        }
        _ => v.to_string(),
    }
}

/// Whitespace runs as one space, trimmed.
fn flat(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A bullet line's text: `- `, `* `, `+ `, `• `, `1. ` or `1) `.
fn bullet(t: &str) -> Option<&str> {
    static BULLET: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"^(?:[-*+•‣◦▪]|\d{1,3}[.)])(?:\s+(.*))?$").expect("static regex"));
    BULLET.captures(t).map(|c| c.get(1).map_or("", |m| m.as_str()))
}

/// A fact as written, without a checkbox and bold.
fn plain(text: &str) -> String {
    let t = text.trim();
    let t = ["[ ] ", "[x] ", "[X] "].iter().find_map(|b| t.strip_prefix(b)).unwrap_or(t);
    flat(&t.replace("**", "").replace("__", ""))
}

/// Spaces before a line's text (a tab is four).
fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| c.is_whitespace()).map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

/// A `#` heading's level and text.
fn heading(t: &str) -> Option<(usize, String)> {
    let level = t.chars().take_while(|c| *c == '#').count();
    let rest = &t[level..];
    ((1..=6).contains(&level) && (rest.is_empty() || rest.starts_with(' '))).then(|| (level, flat(rest.trim_end_matches('#'))))
}

/// A horizontal rule (`---`, `***`, `___`).
fn is_rule(t: &str) -> bool {
    let s: String = t.chars().filter(|c| !c.is_whitespace()).collect();
    s.len() >= 3 && ["-", "*", "_"].iter().any(|r| s.chars().all(|c| c.to_string() == *r))
}

/// What kind of line Yap couldn't place (for the review's count and the
/// log: never its words).
fn kind_of(t: &str) -> &'static str {
    if t.starts_with('>') {
        "a quote"
    } else if t.starts_with('|') {
        "a table"
    } else if t.starts_with('<') {
        "HTML"
    } else {
        "plain text"
    }
}

/// "Kids:" with "Ava" and "Leo" under it → "Kids: Ava, Leo"; "Works at
/// Acme" with "since 2019" → "Works at Acme: since 2019" (each as written).
fn join_nested(fact: &str, kids: &[String]) -> String {
    if kids.is_empty() {
        return fact.to_string();
    }
    let fact = fact.trim_end();
    let sep = if fact.ends_with(':') { " " } else { ": " };
    let kids: Vec<&str> = kids.iter().map(|k| k.trim_end_matches(['.', ',', ';'])).collect();
    format!("{fact}{sep}{}", kids.join(", "))
}

fn body(lines: &[&str], p: &mut Parsed) {
    // The bullet being read, and the bullets nested under it.
    let mut cur: Option<(String, Vec<String>)> = None;
    let finish = |cur: &mut Option<(String, Vec<String>)>, facts: &mut Vec<String>| {
        if let Some((fact, kids)) = cur.take() {
            facts.push(join_nested(&fact, &kids));
        }
    };
    for line in lines {
        let t = line.trim();
        // A code fence's marks aren't facts; what's inside them is read.
        if t.is_empty() || t.starts_with("```") || t.starts_with("~~~") {
            continue;
        }
        if let Some((level, text)) = heading(t) {
            // No front matter: a file's first heading is its name.
            if level == 1 && p.name.is_empty() && !p.front_matter {
                p.name = text;
            }
            continue;
        }
        if is_rule(t) {
            continue;
        }
        let indent = indent_of(line);
        if let Some(text) = bullet(t) {
            let text = plain(text);
            if text.is_empty() {
                continue;
            }
            match cur.as_mut() {
                Some((_, kids)) if indent >= 2 => kids.push(text),
                _ => {
                    finish(&mut cur, &mut p.facts);
                    cur = Some((text, Vec::new()));
                }
            }
            continue;
        }
        // An indented line under a bullet: that bullet goes on.
        if let Some((fact, kids)) = cur.as_mut().filter(|_| indent >= 2) {
            let last = kids.last_mut().unwrap_or(fact);
            last.push(' ');
            last.push_str(&plain(t));
            continue;
        }
        finish(&mut cur, &mut p.facts);
        p.unplaced.push(kind_of(t));
    }
    finish(&mut cur, &mut p.facts);
}

// ---- facts that fit ----------------------------------------------------------------------------

/// Where a long fact is cut, strongest first: a sentence's end, a
/// semicolon, a dash, a comma, then between words.
const CUTS: &[&[&str]] = &[&[". ", "! ", "? "], &["; "], &[" — ", " – ", " - "], &[", "], &[" "]];

/// Short words a "." doesn't end a sentence after ("Mr. Okafor").
const ABBREV: &[&str] = &["mr", "mrs", "ms", "dr", "st", "prof", "rev", "jr", "sr", "vs", "eg", "ie", "approx", "no", "mt", "fig"];

/// `text` cut after each of `seps` (the separator's mark kept on the
/// left: "A, B" → "A,", "B"), never after an abbreviation's dot.
fn split_after(text: &str, seps: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < text.len() {
        let hit = seps.iter().find(|s| text[i..].starts_with(**s));
        if let Some(sep) = hit {
            let mark_end = i + sep.trim_end().len();
            let before = text[start..i].split_whitespace().last().unwrap_or("");
            let abbrev = sep.starts_with('.') && {
                let w = before.trim_matches(|c: char| !c.is_alphanumeric()).replace('.', "").to_lowercase();
                w.chars().count() <= 1 || ABBREV.contains(&w.as_str())
            };
            if !abbrev && mark_end > start {
                let piece = text[start..mark_end].trim();
                if !piece.is_empty() {
                    out.push(piece.to_string());
                }
                start = i + sep.len();
            }
            i += sep.len();
            continue;
        }
        i += text[i..].chars().next().map_or(1, char::len_utf8);
    }
    let last = text[start..].trim();
    if !last.is_empty() {
        out.push(last.to_string());
    }
    out
}

/// Adjacent pieces joined again while they fit in `max` characters.
fn greedy(pieces: &[String], max: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in pieces {
        match out.last_mut() {
            Some(cur) if cur.chars().count() + 1 + p.chars().count() <= max => {
                cur.push(' ');
                cur.push_str(p);
            }
            _ => out.push(p.clone()),
        }
    }
    out.into_iter().map(|p| p.trim_end_matches([',', ';', ':', '—', '–', '-', ' ']).to_string()).filter(|p| !p.is_empty()).collect()
}

fn fit_at(text: &str, level: usize) -> Vec<String> {
    if text.chars().count() <= ops::MAX_FACT || level >= CUTS.len() {
        return vec![text.to_string()];
    }
    let pieces = split_after(text, CUTS[level]);
    if pieces.len() < 2 {
        return fit_at(text, level + 1);
    }
    greedy(&pieces, ops::MAX_FACT).iter().flat_map(|p| fit_at(p, level + 1)).collect()
}

/// **A bullet as facts memory keeps whole** (≤ `ops::MAX_FACT` characters
/// each): as it is when it fits (one bullet, one fact: memory keeps a
/// file's "A; B" as written, `ops::Rules::as_written`); else cut where a
/// sentence ends, else at a semicolon, a dash or a comma, else between
/// words, the pieces joined again while they fit. Pure.
pub fn fit(fact: &str) -> Vec<String> {
    let text = flat(fact);
    if text.is_empty() {
        return Vec::new();
    }
    fit_at(&text, 0)
}

/// A description as "about" (≤ `ops::MAX_ABOUT`) and what doesn't fit
/// there as facts: its first sentences or clauses that fit. Pure.
fn about_and_rest(desc: &str) -> (String, Vec<String>) {
    let d = flat(desc);
    if d.chars().count() <= ops::MAX_ABOUT {
        return (d, Vec::new());
    }
    for seps in CUTS {
        let pieces = split_after(&d, seps);
        let mut about = String::new();
        let mut n = 0;
        for p in &pieces {
            let next = if about.is_empty() { p.clone() } else { format!("{about} {p}") };
            if next.chars().count() > ops::MAX_ABOUT {
                break;
            }
            about = next;
            n += 1;
        }
        if n == 0 || n == pieces.len() {
            continue;
        }
        let about = about.trim_end_matches([',', ';', ':', '—', '–', '-', ' ', '.']).to_string();
        return (about, fit(&pieces[n..].join(" ")));
    }
    (d, Vec::new())
}

// ---- names -------------------------------------------------------------------------------------

/// For comparing names and facts whatever their case and accents: `ops::norm`
/// with "Zoë" = "Zoe", "Ångström" = "Angstrom". Pure.
pub fn fold(s: &str) -> String {
    let mut out = String::new();
    for c in ops::norm(s).chars() {
        let plain = match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
            'ç' | 'ć' | 'ĉ' | 'č' => "c",
            'ď' | 'đ' | 'ð' => "d",
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => "e",
            'ğ' | 'ģ' => "g",
            'ì' | 'í' | 'î' | 'ï' | 'ī' | 'į' | 'ı' => "i",
            'ķ' => "k",
            'ĺ' | 'ļ' | 'ľ' | 'ł' => "l",
            'ñ' | 'ń' | 'ņ' | 'ň' => "n",
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => "o",
            'ŕ' | 'ř' => "r",
            'ś' | 'ş' | 'š' | 'ș' => "s",
            'ţ' | 'ť' | 'ț' => "t",
            'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' | 'ų' => "u",
            'ý' | 'ÿ' => "y",
            'ź' | 'ż' | 'ž' => "z",
            'ß' => "ss",
            'æ' => "ae",
            'œ' => "oe",
            'þ' => "th",
            _ => {
                out.push(c);
                continue;
            }
        };
        out.push_str(plain);
    }
    out
}

fn capitalise(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// `words` (a slug's) as `text` writes them: "sam reed" in "Sam Reed's my
/// brother-in-law" → "Sam Reed", accents and all ("zoe" → "Zoë").
fn as_written(words: &[&str], text: &str) -> Option<String> {
    let want: Vec<String> = words.iter().map(|w| fold(w)).collect();
    let clean = |t: &str| -> String {
        let t = t.trim_matches(|c: char| !c.is_alphanumeric());
        let t = ["'s", "’s"].iter().find_map(|s| t.strip_suffix(s)).unwrap_or(t);
        t.to_string()
    };
    let tokens: Vec<String> = text.split_whitespace().map(clean).collect();
    (0..tokens.len().saturating_sub(want.len() - 1))
        .find(|&i| tokens[i..i + want.len()].iter().zip(&want).all(|(t, w)| fold(t) == *w))
        .map(|i| tokens[i..i + want.len()].join(" "))
}

/// The name of a person, project or topic from its file's `slug` (the
/// front matter's `name`, else the file's): as the description writes it,
/// else in capitals — a person's every word ("Sam Reed"), a topic's first
/// ("Allotment plan"). A name written as one ("Kitchen refit", "Sam
/// McAllister") stays as it is. Pure.
pub fn title_for(slug: &str, description: &str, person: bool) -> String {
    let slug = flat(slug);
    if slug.contains(' ') || slug.chars().any(char::is_uppercase) {
        return slug;
    }
    let words: Vec<&str> = slug.split(['-', '_', '.']).filter(|w| !w.is_empty()).collect();
    if words.is_empty() {
        return String::new();
    }
    match as_written(&words, description) {
        // Written with capitals: as written ("Ana de la Cruz").
        Some(found) if found.chars().any(char::is_uppercase) => found,
        Some(found) if person => found.split(' ').map(capitalise).collect::<Vec<_>>().join(" "),
        Some(found) => capitalise(&found),
        None if person => words.iter().map(|w| capitalise(w)).collect::<Vec<_>>().join(" "),
        None => capitalise(&words.join(" ")),
    }
}

// ---- the mapping -------------------------------------------------------------------------------

/// One thing the files say, as a Yap section has it, before it's merged
/// with what Yap remembers.
#[derive(Debug, Clone, PartialEq)]
pub struct Thing {
    pub section: Section,
    /// "" for Profile and Preferences.
    pub title: String,
    pub about: String,
    pub facts: Vec<String>,
}

/// What the files hold, in Yap's sections.
#[derive(Debug, Default, Clone)]
pub struct Mapped {
    pub things: Vec<Thing>,
    /// Where they came from, for the items' source ("Claude", "a Yap
    /// export", "a file").
    pub from: &'static str,
    /// Markdown files read.
    pub files: usize,
    /// Lines Yap couldn't place, by kind (never their words).
    pub unplaced: BTreeMap<&'static str, usize>,
    /// Yap's own export had About you's part (never imported: it's set in
    /// About you).
    pub about_you: bool,
}

/// A file's name without `.md`.
fn stem_of(file: &str) -> &str {
    let lower = file.to_lowercase();
    if lower.ends_with(".markdown") {
        &file[..file.len() - ".markdown".len()]
    } else if lower.ends_with(".md") {
        &file[..file.len() - 3]
    } else {
        file
    }
}

/// Yap's own export (`export.rs`): You.md, People.md, Projects.md or
/// Topics.md at the top, starting with its "# " heading.
fn is_yap(doc: &Doc) -> bool {
    let [file] = doc.path.as_slice() else {
        return false;
    };
    let first = doc.text.trim_start_matches('\u{feff}').lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    matches!(
        (file.to_lowercase().as_str(), first),
        ("you.md", "# You") | ("people.md", "# People") | ("projects.md", "# Projects") | ("topics.md", "# Topics")
    )
}

/// One of Yap's own export files read back: its items (About you's part
/// left out; whether it was there), the lines it couldn't place.
fn parse_yap(file: &str, text: &str) -> (Vec<Thing>, usize, bool) {
    let section = match file.to_lowercase().as_str() {
        "people.md" => Some(Section::People),
        "projects.md" => Some(Section::Projects),
        "topics.md" => Some(Section::Topics),
        _ => None,
    };
    let mut things = Vec::new();
    let mut cur: Option<Thing> = None;
    let (mut unplaced, mut about_you, mut skipping) = (0, false, false);
    for line in text.trim_start_matches('\u{feff}').lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("# ") {
            continue;
        }
        if let Some(h) = t.strip_prefix("## ") {
            things.extend(cur.take());
            let h = h.trim();
            skipping = false;
            cur = match section {
                Some(s) => Some(Thing { section: s, title: h.to_string(), about: String::new(), facts: Vec::new() }),
                None => match h.to_lowercase().as_str() {
                    "profile" => Some(Thing { section: Section::Profile, title: String::new(), about: String::new(), facts: Vec::new() }),
                    "preferences" => Some(Thing { section: Section::Preferences, title: String::new(), about: String::new(), facts: Vec::new() }),
                    _ => {
                        // About you: General → About you's, never memory's.
                        about_you = true;
                        skipping = true;
                        None
                    }
                },
            };
            continue;
        }
        // "_Updated 5 Oct 2026 · from a chat_", "Nothing yet."
        if skipping || (t.starts_with('_') && t.ends_with('_')) || t == "Nothing yet." {
            continue;
        }
        match (cur.as_mut(), bullet(t)) {
            (Some(c), Some(f)) => {
                let f = plain(f);
                if !f.is_empty() {
                    c.facts.push(f);
                }
            }
            (Some(c), None) if !c.section.is_you() && c.about.is_empty() && c.facts.is_empty() => c.about = flat(t),
            _ => unplaced += 1,
        }
    }
    things.extend(cur);
    (things, unplaced, about_you)
}

/// `about` with the Claude Project it came from: "… (Claude Project
/// “Kitchen refit”)", or when that's too long "From the Claude Project
/// “Kitchen refit”" and the description as facts.
fn with_project(desc: &str, project: &str) -> (String, Vec<String>) {
    let note = format!("Claude Project “{}”", ops::one_line(project, 50));
    let desc = flat(desc);
    if desc.is_empty() {
        return (format!("From the {note}"), Vec::new());
    }
    let combined = format!("{} ({note})", desc.trim_end_matches('.'));
    if combined.chars().count() <= ops::MAX_ABOUT {
        return (combined, Vec::new());
    }
    (format!("From the {note}"), fit(&desc))
}

/// Where a Claude file goes ([`mapped`]'s rules), its name from `slug`
/// (`project`: the Claude Project it sits in).
enum Place<'a> {
    You(Section),
    Thing { section: Section, slug: &'a str, project: Option<&'a str> },
}

/// Where the Claude file at `path` goes.
fn place(path: &[String]) -> Option<Place<'_>> {
    let file = path.last()?;
    let stem = stem_of(file);
    let lower: Vec<String> = path.iter().map(|p| p.to_lowercase()).collect();
    let lower: Vec<&str> = lower.iter().map(String::as_str).collect();
    let thing = |section, slug, project| Some(Place::Thing { section, slug, project });
    match lower.as_slice() {
        [_] => match stem.to_lowercase().as_str() {
            "preferences" => Some(Place::You(Section::Preferences)),
            "profile" => Some(Place::You(Section::Profile)),
            _ => thing(Section::Topics, stem, None),
        },
        ["people", ..] => thing(Section::People, stem, None),
        ["topics", ..] => thing(Section::Topics, stem, None),
        ["areas", ..] => thing(Section::Projects, stem, None),
        // A file straight under projects/: a Claude Project of its own.
        ["projects", _] => thing(Section::Topics, stem, Some(stem)),
        ["projects", _, rest @ ..] => {
            let project = path[1].as_str();
            match rest {
                ["people", _, ..] => thing(Section::People, stem, None),
                ["areas", _, ..] => thing(Section::Projects, stem, Some(project)),
                // The project's own file ("preferences.md", "kitchen-refit.md"): a topic named after the project.
                [_] if matches!(stem.to_lowercase().as_str(), "preferences" | "profile" | "memory" | "notes" | "index" | "readme" | "overview")
                    || fold(stem) == fold(project) =>
                {
                    thing(Section::Topics, project, Some(project))
                }
                _ => thing(Section::Topics, stem, Some(project)),
            }
        }
        // A folder Yap doesn't know: a Topic named after it.
        [_, ..] => thing(Section::Topics, path[0].as_str(), None),
        [] => None,
    }
}

/// **What the files hold, in Yap's sections** (the mapping in the module's
/// notes). Pure.
pub fn mapped(docs: &[Doc]) -> Mapped {
    let mut out = Mapped::default();
    let (mut claude, mut yap) = (false, false);
    for doc in docs {
        out.files += 1;
        if is_yap(doc) {
            yap = true;
            let (things, unplaced, about_you) = parse_yap(&doc.path[0], &doc.text);
            out.things.extend(things.into_iter().map(|t| Thing { facts: t.facts.iter().flat_map(|f| fit(f)).collect(), ..t }));
            if unplaced > 0 {
                *out.unplaced.entry("plain text").or_default() += unplaced;
            }
            out.about_you |= about_you;
            continue;
        }
        let parsed = parse_claude(&doc.text);
        for kind in &parsed.unplaced {
            *out.unplaced.entry(kind).or_default() += 1;
        }
        let Some(place) = place(&doc.path) else {
            continue;
        };
        let known = doc.path.len() > 1 || matches!(place, Place::You(_));
        claude |= parsed.front_matter || known;
        let facts: Vec<String> = parsed.facts.iter().flat_map(|f| fit(f)).collect();
        match place {
            Place::You(section) => {
                if !facts.is_empty() {
                    out.things.push(Thing { section, title: String::new(), about: String::new(), facts });
                }
            }
            Place::Thing { section, slug, project } => {
                // A folder named after a thing is its name; a file's slug
                // is the front matter's name when it has one.
                let named_by_folder = !matches!(doc.path.first().map(|p| p.to_lowercase()).as_deref(), Some("people" | "topics" | "areas" | "projects"))
                    && doc.path.len() > 1;
                let slug = if parsed.name.is_empty() || named_by_folder || project == Some(slug) { slug.to_string() } else { parsed.name.clone() };
                let title = title_for(&slug, &parsed.description, section == Section::People);
                if title.is_empty() {
                    *out.unplaced.entry("a file with no name").or_default() += 1;
                    continue;
                }
                let (about, more) = match project {
                    Some(p) => with_project(&parsed.description, p),
                    None => about_and_rest(&parsed.description),
                };
                out.things.push(Thing { section, title, about, facts: more.into_iter().chain(facts).collect() });
            }
        }
    }
    out.from = if claude {
        "Claude"
    } else if yap {
        "a Yap export"
    } else {
        "a file"
    };
    out
}

// ---- merged with what Yap has ------------------------------------------------------------------

/// The same fact, whatever its case, accents and punctuation; or one Yap
/// has already that says it more fully.
fn known_fact(have: &[String], fact: &str) -> bool {
    let f = fold(fact);
    !f.is_empty() && have.iter().map(|h| fold(h)).any(|h| h == f || (f.len() >= 8 && h.contains(&f)))
}

/// `facts` that `have` doesn't have, each once.
fn new_facts(facts: &[String], have: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in facts {
        if !known_fact(have, f) && !known_fact(&out, f) {
            out.push(f.clone());
        }
    }
    out
}

/// The new facts worth proposing for an item that has `have`: all of them
/// while they fit beside it; when they don't (an item keeps its last
/// `max`), only those the file puts after the last fact memory already has
/// from it — the ones before are what didn't fit last time, and importing
/// the same file again changes nothing. Pure.
fn worth_adding(file: &[String], new: Vec<String>, have: &[String], max: usize) -> Vec<String> {
    if have.len() + new.len() <= max {
        return new;
    }
    match file.iter().rposition(|f| known_fact(have, f)) {
        Some(last) => new.into_iter().filter(|n| file.iter().position(|f| f == n).is_some_and(|at| at > last)).collect(),
        None => new,
    }
}

/// One thing a section has once: the files' things with the same name
/// (whatever its case and accents) together, their facts in order, the
/// first "about" (a different one later becomes a fact).
fn combined(things: &[Thing]) -> Vec<Thing> {
    let mut out: Vec<Thing> = Vec::new();
    for t in things {
        let key = fold(&t.title);
        match out.iter_mut().find(|o| o.section == t.section && fold(&o.title) == key) {
            Some(o) => {
                if o.about.is_empty() {
                    o.about = t.about.clone();
                } else if !t.about.is_empty() && fold(&t.about) != fold(&o.about) {
                    o.facts.push(t.about.clone());
                }
                o.facts.extend(t.facts.iter().cloned());
            }
            None => out.push(t.clone()),
        }
    }
    for o in out.iter_mut() {
        o.facts = new_facts(&o.facts, &[]);
    }
    out
}

/// **The changes a file import makes to `items`**: a new person, project
/// or topic added; one Yap has (by name, whatever its case and accents; a
/// person's first name as `ops::same_thing` has it) updated with only the
/// facts it doesn't have, its "about" only when it had none and its name
/// only to a fuller one ("Sam" → "Sam Reed"); Profile's and Preferences'
/// new facts. Nothing new: no change. Pure.
pub fn changes(things: &[Thing], items: &[Item]) -> Vec<Change> {
    let change = |action, id, section, name: String, about: Option<String>, facts| Change {
        action,
        id,
        section: Some(section),
        name,
        about,
        facts,
        remove_facts: Vec::new(),
        say: String::new(),
    };
    let things = combined(things);
    // Matched by the whole name first; a first name only matches an item
    // no other thing in the files is (two Sams, one "Sam" in memory).
    let exact: Vec<u64> = things
        .iter()
        .filter_map(|t| items.iter().find(|i| !t.section.is_you() && i.section == t.section && fold(&i.title) == fold(&t.title)))
        .map(|i| i.id)
        .collect();
    let mut taken: Vec<u64> = Vec::new();
    let mut out = Vec::new();
    for t in things {
        if t.section.is_you() {
            let have: Vec<String> = items.iter().filter(|i| i.section == t.section).flat_map(|i| i.details.clone()).collect();
            let facts = worth_adding(&t.facts, new_facts(&t.facts, &have), &have, t.section.max_facts());
            if !facts.is_empty() {
                out.push(change(Action::Add, None, t.section, String::new(), None, facts));
            }
            continue;
        }
        let key = fold(&t.title);
        let existing = items.iter().find(|i| i.section == t.section && fold(&i.title) == key).or_else(|| {
            items.iter().find(|i| {
                i.section == t.section && !exact.contains(&i.id) && !taken.contains(&i.id) && ops::same_thing(t.section, &fold(&i.title), &key)
            })
        });
        if let Some(i) = existing {
            taken.push(i.id);
        }
        let about = (!t.about.is_empty()).then(|| t.about.clone());
        match existing {
            Some(i) => {
                let facts = worth_adding(&t.facts, new_facts(&t.facts, &i.details), &i.details, t.section.max_facts());
                let fuller = key.starts_with(&format!("{} ", fold(&i.title)));
                let about = about.filter(|_| i.summary.trim().is_empty());
                if facts.is_empty() && about.is_none() && !fuller {
                    continue;
                }
                let name = if fuller { t.title.clone() } else { i.title.clone() };
                out.push(change(Action::Update, Some(i.id), t.section, name, about, facts));
            }
            None => out.push(change(Action::Add, None, t.section, t.title.clone(), about, t.facts.clone())),
        }
    }
    out
}

/// The review's lines for what the preview left out (`ops::apply`'s
/// reasons, counted; `sensitive_ok`: the setting). Pure.
pub fn left_out(dropped: &[String], sensitive_ok: bool) -> Vec<String> {
    let (mut fit, mut sensitive, mut passing, mut secret, mut full, mut room) = (0, 0, 0, 0, 0, 0);
    for d in dropped {
        match d.as_str() {
            ops::DIDNT_FIT => fit += 1,
            "left out a passing detail" => passing += 1,
            d if d.starts_with("left out a sensitive topic") => sensitive += 1,
            d if d.ends_with(" is full") => full += 1,
            d if d.starts_with("made room in ") => room += 1,
            d if d.starts_with("left out ") => secret += 1,
            _ => {}
        }
    }
    let n = |k: usize, one: &str, many: &str| if k == 1 { format!("1 {one}") } else { format!("{k} {many}") };
    let mut out = Vec::new();
    if fit > 0 {
        out.push(format!(
            "Yap keeps the last 8 facts of each person, project and topic (16 for Profile and Preferences): {} didn’t fit, struck out below.",
            if fit == 1 { "1 fact".to_string() } else { format!("{fit} facts") }
        ));
    }
    if sensitive > 0 && !sensitive_ok {
        out.push(format!(
            "{} left out: sensitive topics are off. Switch on “Include sensitive topics in memory” above to keep them, then import again.",
            n(sensitive, "fact", "facts")
        ));
    }
    if secret > 0 {
        out.push(format!("{} left out: Yap never keeps passwords, numbers or addresses in memory.", n(secret, "fact", "facts")));
    }
    if passing > 0 {
        out.push(format!("{} left out: one-off requests, not lasting facts.", n(passing, "line", "lines")));
    }
    if full > 0 {
        out.push(format!("{} left out: that part of memory is full.", n(full, "thing", "things")));
    }
    if room > 0 {
        out.push(format!("To make room, {} Yap learned long ago {}.", n(room, "thing", "things"), if room == 1 { "goes" } else { "go" }));
    }
    out
}

/// The review's lines for what the files had that isn't imported.
pub fn read_notes(read: &Read, m: &Mapped) -> Vec<String> {
    let mut out = Vec::new();
    let unplaced: usize = m.unplaced.values().sum();
    if unplaced > 0 {
        out.push(if unplaced == 1 {
            "1 line Yap couldn’t place (not a “- ” fact) was left out.".to_string()
        } else {
            format!("{unplaced} lines Yap couldn’t place (not “- ” facts) were left out.")
        });
    }
    if read.skipped > 0 {
        out.push(if read.skipped == 1 {
            "1 file that isn’t memory (not Markdown, or too big) was left out.".to_string()
        } else {
            format!("{} files that aren’t memory (not Markdown, or too big) were left out.", read.skipped)
        });
    }
    if m.about_you {
        out.push("About you (your name, region and note) isn’t imported: set it in General → About you.".to_string());
    }
    out
}

// ---- the dialog --------------------------------------------------------------------------------

/// The file (or, `folder`, the folder) the person picked in Windows' open
/// dialog, opened here so no path comes from the page; a test run: the one
/// it named (`e2e_memory_file`). `None`: cancelled.
pub async fn pick(app: &tauri::AppHandle, window: tauri::WebviewWindow, folder: bool) -> Result<Option<PathBuf>, String> {
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() {
        return E2E_FILE
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
            .map(Some)
            .ok_or_else(|| "A test run names the file first (e2e_memory_file).".to_string());
    }
    use tauri_plugin_dialog::DialogExt;
    let dialog = app.dialog().file().set_parent(&window);
    let picked = if folder {
        let dialog = dialog.set_title("Import memory from a folder");
        tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_folder()).await
    } else {
        let dialog = dialog
            .set_title("Import memory from a file")
            .add_filter("Claude's memory export or Yap's (.zip, .md)", &["zip", "md", "markdown"])
            .add_filter("All files", &["*"]);
        tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_file()).await
    }
    .map_err(|e| e.to_string())?;
    Ok(picked.and_then(|p| p.into_path().ok()))
}

/// Test mode: the file or folder the next "Import a file…" reads, in place
/// of the dialog.
#[cfg(debug_assertions)]
static E2E_FILE: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Test mode: name the file or folder the next "Import a file…" reads.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_memory_file(path: String) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("test mode only".into());
    }
    *E2E_FILE.lock().unwrap_or_else(|p| p.into_inner()) = Some(PathBuf::from(path));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::ops::{Origin, Rules};
    use super::*;
    use std::io::Write as _;

    /// A .zip of `files` (name, text), with a folder entry for each folder,
    /// as Claude's export writes it.
    fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
        let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut dirs: Vec<String> = Vec::new();
        for (name, text) in files {
            let parts: Vec<&str> = name.split('/').collect();
            for n in 1..parts.len() {
                let dir = format!("{}/", parts[..n].join("/"));
                if !dirs.contains(&dir) {
                    z.add_directory(dir.as_str(), opts).unwrap();
                    dirs.push(dir);
                }
            }
            z.start_file(*name, opts).unwrap();
            z.write_all(text.as_bytes()).unwrap();
        }
        z.finish().unwrap().into_inner()
    }

    fn file(name: &str, description: &str, facts: &[&str]) -> String {
        let facts: String = facts.iter().map(|f| format!("- {f}\n")).collect();
        format!("---\nname: \"{name}\"\ndescription: \"{description}\"\n---\n\n{facts}")
    }

    /// A made-up Claude memory export (fictional people and facts).
    fn claude_export() -> Vec<u8> {
        let root = "claude-memory-2026-11-03";
        let files = [
            (format!("{root}/preferences.md"), file("preferences", "How they like answers", &["Prefers short answers", "Uses British spelling"])),
            (format!("{root}/profile.md"), file("profile", "Core facts", &["Works as a garden designer; mostly private gardens", "Lives in Harrogate"])),
            (
                format!("{root}/people/sam-reed.md"),
                file("sam-reed", "Sam's my brother-in-law; lives in Leeds", &["Works as a structural engineer", "Birthday 14 March"]),
            ),
            (format!("{root}/people/priya-shah.md"), file("priya-shah", "Priya Shah is my manager at Acme", &["Runs the design team"])),
            (format!("{root}/topics/gardening.md"), file("gardening", "Growing vegetables at home", &["Grows tomatoes in a greenhouse"])),
            (format!("{root}/areas/allotment-plan.md"), file("allotment-plan", "The plan for the allotment", &["Plot 14 at the Westfield site"])),
            (
                format!("{root}/projects/Kitchen refit/topics/suppliers.md"),
                file("suppliers", "Worktop suppliers being compared", &["Oak from a supplier in York"]),
            ),
            (format!("{root}/projects/Kitchen refit/notes.png"), "not markdown".to_string()),
            ("__MACOSX/._preferences.md".to_string(), "mac junk".to_string()),
        ];
        let refs: Vec<(&str, &str)> = files.iter().map(|(n, t)| (n.as_str(), t.as_str())).collect();
        zip_of(&refs)
    }

    fn rules() -> Rules {
        Rules { sensitive_ok: false, origin: Origin::new("import", None, "Claude"), now: 100, max_changes: 400, grounding: None }
    }

    fn item(id: u64, section: Section, title: &str, summary: &str, details: &[&str]) -> Item {
        Item {
            id,
            section,
            title: title.into(),
            summary: summary.into(),
            details: details.iter().map(|d| d.to_string()).collect(),
            source: Origin::new("chat", None, ""),
            created_ts: 1,
            updated_ts: 1,
            dictation: true,
        }
    }

    fn thing<'a>(m: &'a Mapped, title: &str) -> &'a Thing {
        m.things.iter().find(|t| t.title == title).unwrap_or_else(|| panic!("{title} in {:?}", m.things))
    }

    #[test]
    fn a_claude_export_maps_to_every_section() {
        let read = read_zip(&claude_export()).unwrap();
        // The date folder taken off; the picture counted, macOS's junk not.
        assert_eq!(read.docs.len(), 7);
        assert_eq!(read.skipped, 1);
        assert!(read.docs.iter().any(|d| d.path == ["projects", "Kitchen refit", "topics", "suppliers.md"]));
        let m = mapped(&read.docs);
        assert_eq!((m.from, m.files), ("Claude", 7));
        assert!(m.unplaced.is_empty(), "{:?}", m.unplaced);
        let you = |s: Section| m.things.iter().find(|t| t.section == s).unwrap();
        // Profile and Preferences: their facts, not the file's description.
        assert_eq!(you(Section::Preferences).facts, ["Prefers short answers", "Uses British spelling"]);
        assert_eq!(you(Section::Profile).facts, ["Works as a garden designer; mostly private gardens", "Lives in Harrogate"]);
        // People: the slug in capitals, or the name as the description writes it.
        let sam = thing(&m, "Sam Reed");
        assert_eq!((sam.section, sam.about.as_str()), (Section::People, "Sam's my brother-in-law; lives in Leeds"));
        assert_eq!(sam.facts, ["Works as a structural engineer", "Birthday 14 March"]);
        assert_eq!(thing(&m, "Priya Shah").about, "Priya Shah is my manager at Acme");
        // topics/ → Topics, areas/ → Projects.
        assert_eq!(thing(&m, "Gardening").section, Section::Topics);
        let plan = thing(&m, "Allotment plan");
        assert_eq!((plan.section, plan.facts.clone()), (Section::Projects, vec!["Plot 14 at the Westfield site".to_string()]));
        // A Claude Project's topic: a Topic that says where it's from.
        let suppliers = thing(&m, "Suppliers");
        assert_eq!(suppliers.section, Section::Topics);
        assert_eq!(suppliers.about, "Worktop suppliers being compared (Claude Project “Kitchen refit”)");

        // Into an empty memory: every one added, each from Claude, in one go.
        let changes = changes(&m.things, &[]);
        assert_eq!(changes.len(), 7);
        let (mut items, mut next) = (Vec::new(), 0);
        let out = ops::apply(&mut items, &mut next, &changes, &rules());
        assert_eq!(out.done.len(), 7, "{:?}", out.dropped);
        assert!(items.iter().all(|i| i.source.kind == "import" && i.source.title == "Claude"));
        assert_eq!(items.iter().find(|i| i.title == "Sam Reed").unwrap().summary, "Sam's my brother-in-law; lives in Leeds");
        // One bullet, one fact: "A; B" kept as written (a model's would be two).
        let profile = items.iter().find(|i| i.section == Section::Profile).unwrap();
        assert_eq!(profile.details, ["Works as a garden designer; mostly private gardens", "Lives in Harrogate"]);
        let pasted = Rules { grounding: Some("Works as a garden designer; mostly private gardens".into()), ..rules() };
        let (mut split, mut next) = (Vec::new(), 0);
        ops::apply(&mut split, &mut next, &changes[1..2], &pasted);
        assert_eq!(split[0].details[..2], ["Works as a garden designer", "Mostly private gardens"]);
    }

    #[test]
    fn the_folder_unzipped_and_one_file_read_the_same() {
        let dir = std::env::temp_dir().join(format!("yap-memory-import-{}", std::process::id()));
        let root = dir.join("claude-memory-2026-11-03");
        std::fs::create_dir_all(root.join("people")).unwrap();
        std::fs::create_dir_all(root.join("projects").join("Kitchen refit").join("topics")).unwrap();
        std::fs::write(root.join("profile.md"), file("profile", "", &["Lives in Harrogate"])).unwrap();
        std::fs::write(root.join("people").join("sam-reed.md"), file("sam-reed", "Brother-in-law", &["Lives in Leeds"])).unwrap();
        std::fs::write(root.join("projects").join("Kitchen refit").join("topics").join("suppliers.md"), file("suppliers", "", &["Oak worktops"])).unwrap();
        std::fs::write(root.join("notes.txt"), "not markdown").unwrap();
        // The export's folder picked, or the folder it's in: the same files.
        for picked in [&root, &dir] {
            let read = read(picked).unwrap();
            assert_eq!(read.skipped, 1);
            let paths: Vec<String> = read.docs.iter().map(|d| d.path.join("/")).collect();
            assert_eq!(paths, ["people/sam-reed.md", "profile.md", "projects/Kitchen refit/topics/suppliers.md"]);
        }
        let m = mapped(&read(&root).unwrap().docs);
        assert_eq!(thing(&m, "Suppliers").about, "From the Claude Project “Kitchen refit”");
        // One file of it, picked alone.
        let one = read(&root.join("people").join("sam-reed.md")).unwrap();
        assert_eq!(one.docs.len(), 1);
        assert_eq!(mapped(&one.docs).things[0].facts, ["Lives in Leeds"]);
        // The people folder picked: its files are people.
        let people = mapped(&read(&root.join("people")).unwrap().docs);
        assert_eq!(people.things[0].section, Section::People);
        assert!(read(&root.join("notes.txt")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_is_read_whatever_its_shape() {
        // A BOM, CRLF, no front matter: the first heading is its name; `*`,
        // `+`, numbered and checkbox bullets; bold; a nested list; a bullet
        // going on over the next line; a paragraph Yap can't place.
        let text = "\u{feff}# Priya Shah\r\n\r\nSome words about her.\r\n\r\n* **Runs** the design team\r\n+ Plays the cello\r\n1. Lives in York\r\n- [x] Likes oat milk\r\n- Kids:\r\n  - Ava, 7\r\n  - Leo, 4\r\n- Works with Sam\r\n  on the Thursday rota\r\n| a | table |\r\n";
        let p = parse_claude(text);
        assert!(!p.front_matter);
        assert_eq!(p.name, "Priya Shah");
        assert_eq!(
            p.facts,
            ["Runs the design team", "Plays the cello", "Lives in York", "Likes oat milk", "Kids: Ava, 7, Leo, 4", "Works with Sam on the Thursday rota"]
        );
        assert_eq!(p.unplaced, ["plain text", "a table"]);
        // Front matter: other keys, single quotes, a folded block, escapes.
        let text = "---\ntags: [family]\nname: 'zoe-durand'\ndescription: >\n  Zoë Durand's my cousin,\n  lives in Lyon\nupdated: 2026-11-01\n---\n- Speaks French\n";
        let p = parse_claude(text);
        assert_eq!((p.name.as_str(), p.description.as_str()), ("zoe-durand", "Zoë Durand's my cousin, lives in Lyon"));
        assert_eq!(p.facts, ["Speaks French"]);
        let p = parse_claude("---\nname: \"sam-reed\"\ndescription: \"Sam's my \\\"brother\\\"-in-law\\u00e9\"\n---\n- A fact\n");
        assert_eq!(p.description, "Sam's my \"brother\"-in-law\u{e9}");
        // A front matter that never ends: none, the rest still read.
        let p = parse_claude("---\nname: x\n- A fact\n");
        assert!(!p.front_matter);
        assert_eq!(p.facts, ["A fact"]);
        // Accents in the name: "zoe-durand" as the description writes it.
        assert_eq!(title_for("zoe-durand", "Zoë Durand's my cousin", true), "Zoë Durand");
    }

    #[test]
    fn names_from_slugs_and_descriptions() {
        assert_eq!(title_for("sam-reed", "Sam's my brother-in-law; lives in Leeds", true), "Sam Reed");
        assert_eq!(title_for("sam-reed", "Sam Reed's my brother-in-law", true), "Sam Reed");
        assert_eq!(title_for("sam-mcallister", "Sam McAllister, a friend from school", true), "Sam McAllister");
        assert_eq!(title_for("allotment-plan", "", false), "Allotment plan");
        assert_eq!(title_for("rust-programming", "Learning rust programming at weekends", false), "Rust programming");
        assert_eq!(title_for("ana-de-la-cruz", "Ana de la Cruz runs the choir", true), "Ana de la Cruz");
        assert_eq!(title_for("sam", "my friend sam from school", true), "Sam");
        assert_eq!(title_for("Kitchen refit", "", false), "Kitchen refit");
        assert_eq!(title_for("", "", true), "");
    }

    #[test]
    fn a_long_fact_is_cut_where_its_sentences_end() {
        let long = "Has an allotment at the Westfield site, plot 14, shared with Mr. Okafor from next door. They grow potatoes, leeks and beans every year and swap seeds with the other plot holders in spring.";
        assert!(long.chars().count() > ops::MAX_FACT);
        let pieces = fit(long);
        assert_eq!(pieces.len(), 2, "{pieces:?}");
        assert!(pieces[0].ends_with("Mr. Okafor from next door."), "not cut after Mr.: {pieces:?}");
        assert!(pieces.iter().all(|p| p.chars().count() <= ops::MAX_FACT));
        assert_eq!(pieces.join(" "), long, "nothing lost");
        // No sentence end: at commas, else between words.
        let commas = format!("Likes {}", (0..30).map(|i| format!("thing {i}")).collect::<Vec<_>>().join(", "));
        let pieces = fit(&commas);
        assert!(pieces.len() >= 2 && pieces.iter().all(|p| p.chars().count() <= ops::MAX_FACT), "{pieces:?}");
        let words = "word ".repeat(60);
        assert!(fit(&words).iter().all(|p| p.chars().count() <= ops::MAX_FACT));
        // A bullet that fits is one fact, as written.
        assert_eq!(fit("Lives in Leeds; works in Bradford"), ["Lives in Leeds; works in Bradford"]);
        assert_eq!(fit("  Lives   in Leeds "), ["Lives in Leeds"]);
        assert!(fit("  ").is_empty());
        // A long description: "about" is its first sentence, the rest facts.
        let (about, rest) = about_and_rest(
            "Sam's my brother-in-law and we climb together most Sundays. He lives in Leeds with my sister and their two children, and works in Bradford.",
        );
        assert_eq!(about, "Sam's my brother-in-law and we climb together most Sundays");
        assert_eq!(rest, ["He lives in Leeds with my sister and their two children, and works in Bradford."]);
    }

    #[test]
    fn what_yap_has_gets_only_the_new_facts() {
        let items = vec![
            item(1, Section::People, "Zoë Durand", "Cousin", &["Lives in Lyon"]),
            item(2, Section::People, "Sam", "", &["Birthday 14 March"]),
            item(3, Section::Profile, "Profile", "", &["Lives in Harrogate"]),
            item(4, Section::Topics, "Gardening", "Growing vegetables", &["Grows tomatoes in a greenhouse"]),
        ];
        let t = |section, title: &str, about: &str, facts: &[&str]| Thing {
            section,
            title: title.into(),
            about: about.into(),
            facts: facts.iter().map(|f| f.to_string()).collect(),
        };
        let things = vec![
            // The same cousin, no accent and in other cases: only the new fact.
            t(Section::People, "zoe durand", "Zoe's my cousin", &["lives in LYON.", "Speaks French"]),
            // "Sam" in memory is Sam Reed: the fuller name, and who he is (memory had none).
            t(Section::People, "Sam Reed", "Brother-in-law", &["Birthday 14 March", "Works as an engineer"]),
            t(Section::Profile, "", "", &["Lives in Harrogate", "Works as a garden designer"]),
            // Nothing new about gardening: no change.
            t(Section::Topics, "Gardening", "Veg", &["Grows tomatoes in a greenhouse"]),
            t(Section::Topics, "Chess", "", &["Plays online"]),
        ];
        let c = changes(&things, &items);
        assert_eq!(c.len(), 4, "{c:?}");
        assert_eq!((c[0].action, c[0].id, c[0].name.as_str()), (Action::Update, Some(1), "Zoë Durand"));
        assert_eq!((c[0].about.clone(), c[0].facts.clone()), (None, vec!["Speaks French".to_string()]));
        assert_eq!((c[1].id, c[1].name.as_str(), c[1].about.as_deref()), (Some(2), "Sam Reed", Some("Brother-in-law")));
        assert_eq!(c[1].facts, ["Works as an engineer"]);
        assert_eq!((c[2].section, c[2].facts.clone()), (Some(Section::Profile), vec!["Works as a garden designer".to_string()]));
        assert_eq!((c[3].action, c[3].name.as_str()), (Action::Add, "Chess"));
        let mut after = items.clone();
        let mut next = 10;
        let out = ops::apply(&mut after, &mut next, &c, &rules());
        assert_eq!(out.done.len(), 4, "{:?}", out.dropped);
        let get = |id: u64| after.iter().find(|i| i.id == id).unwrap();
        assert_eq!((get(1).title.as_str(), get(1).summary.as_str()), ("Zoë Durand", "Cousin"));
        assert_eq!(get(1).details, ["Lives in Lyon", "Speaks French"]);
        assert_eq!((get(2).title.as_str(), get(2).summary.as_str()), ("Sam Reed", "Brother-in-law"));
        assert_eq!(get(3).details, ["Lives in Harrogate", "Works as a garden designer"]);
        // The same file again: nothing new.
        assert!(changes(&things, &after).is_empty(), "{:?}", changes(&things, &after));
        // Two Sams in the files, one "Sam" in memory: only the exact one is him.
        let two = vec![t(Section::People, "Sam", "", &["Lives in Leeds"]), t(Section::People, "Sam Patel", "", &["Lives in Hull"])];
        let c = changes(&two, &items);
        assert_eq!((c[0].action, c[0].id), (Action::Update, Some(2)));
        assert_eq!((c[1].action, c[1].id), (Action::Add, None));
    }

    #[test]
    fn a_long_list_keeps_its_last_eight_and_comes_in_again_as_nothing() {
        let facts: Vec<String> = ["Plays online", "Likes the Sicilian", "Rated about 1500", "Plays on Sundays", "Reads chess books", "Follows the world championship", "Has a wooden set", "Studies endgames", "Joined a club", "Teaches a niece"]
            .iter()
            .map(|f| f.to_string())
            .collect();
        let things = vec![Thing { section: Section::Topics, title: "Chess".into(), about: String::new(), facts: facts.clone() }];
        let (mut items, mut next) = (Vec::new(), 0);
        let out = ops::apply(&mut items, &mut next, &changes(&things, &[]), &rules());
        assert_eq!(items[0].details, facts[2..]);
        assert_eq!(out.done[0].facts_left_out, facts[..2]);
        assert!(left_out(&out.dropped, false)[0].contains("2 facts didn’t fit"));
        // The same file again: what didn't fit isn't offered again.
        assert!(changes(&things, &items).is_empty());
        // A newer fact at the end of the file still comes in.
        let mut newer = things.clone();
        newer[0].facts.push("Won a local tournament".into());
        let c = changes(&newer, &items);
        assert_eq!(c[0].facts, ["Won a local tournament"]);
    }

    #[test]
    fn a_folder_or_file_yap_doesnt_know_is_a_topic() {
        let docs = vec![
            Doc { path: vec!["goals".into(), "fitness.md".into()], text: file("fitness", "Getting fitter", &["Runs 5k on Saturdays"]) },
            Doc { path: vec!["goals".into(), "career.md".into()], text: file("career", "Work goals", &["Wants to lead a team"]) },
            Doc { path: vec!["misc-notes.md".into()], text: "- Collects vinyl\n".into() },
        ];
        let m = mapped(&docs);
        let c = changes(&m.things, &[]);
        assert_eq!(c.len(), 2, "{c:?}");
        // The folder's files are one topic, named after it.
        assert_eq!((c[0].section, c[0].name.as_str()), (Some(Section::Topics), "Goals"));
        assert_eq!(c[0].about.as_deref(), Some("Getting fitter"));
        assert_eq!(c[0].facts, ["Runs 5k on Saturdays", "Work goals", "Wants to lead a team"]);
        assert_eq!((c[1].name.as_str(), c[1].facts.clone()), ("Misc notes", vec!["Collects vinyl".to_string()]));
        // A plain Markdown file isn't named as Claude's.
        assert_eq!(mapped(&docs[2..]).from, "a file");
    }

    #[test]
    fn a_zip_naming_a_place_outside_it_is_refused_whole() {
        for bad in ["../evil.md", "claude-memory/../../evil.md", "/etc/evil.md", "C:/Users/evil.md", "people\\..\\..\\evil.md"] {
            let z = super::super::export::zip(&[("profile.md".to_string(), "- Fine\n".to_string()), (bad.to_string(), "- Evil\n".to_string())]).unwrap();
            assert_eq!(read_zip(&z).unwrap_err(), SLIP, "{bad}");
        }
        assert!(!unsafe_name("claude-memory-2026-11-03/projects/Kitchen refit: phase 2/topics/x.md"));
        // Not a zip at all.
        assert_eq!(read_zip(b"hello").unwrap_err(), DAMAGED);
    }

    #[test]
    fn a_huge_file_is_left_out() {
        let big = "- x\n".repeat((MAX_FILE / 4 + 10) as usize);
        let z = zip_of(&[("profile.md", "- Lives in Harrogate\n"), ("topics/huge.md", &big)]);
        let read = read_zip(&z).unwrap();
        assert_eq!((read.docs.len(), read.skipped), (1, 1));
    }

    #[test]
    fn yaps_own_export_comes_back_as_it_was() {
        let mine = vec![
            item(1, Section::Profile, "Profile", "", &["Works as a garden designer", "Lives in Harrogate"]),
            item(2, Section::Preferences, "Preferences", "", &["Prefers short answers"]),
            item(3, Section::People, "Priya Shah", "Manager at Acme", &["Runs the design team"]),
            item(4, Section::People, "Zoë Durand", "", &[]),
            item(5, Section::Projects, "Allotment", "Vegetable plot", &["Plot 14"]),
            item(6, Section::Topics, "Chess", "A game they play", &["Plays online", "Likes the Sicilian"]),
        ];
        let about = [("Name", "Declan".to_string()), ("Region", "United Kingdom".to_string())];
        let z = super::super::export::zip(&super::super::export::files(&mine, &about)).unwrap();
        let read = read_zip(&z).unwrap();
        let m = mapped(&read.docs);
        assert_eq!(m.from, "a Yap export");
        assert!(m.about_you, "About you's part is seen, never imported");
        assert!(m.unplaced.is_empty(), "{:?}", m.unplaced);
        let (mut items, mut next) = (Vec::new(), 0);
        ops::apply(&mut items, &mut next, &changes(&m.things, &[]), &rules());
        let key = |i: &Item| (i.section, i.title.clone(), i.summary.clone(), i.details.clone());
        let mut want: Vec<_> = mine.iter().map(key).collect();
        let mut got: Vec<_> = items.iter().map(key).collect();
        want.sort();
        got.sort();
        assert_eq!(got, want);
        // Into the memory it came from: nothing new.
        assert!(changes(&m.things, &mine).is_empty());
        assert!(read_notes(&read, &m).iter().any(|n| n.contains("About you")));
    }

    #[test]
    fn the_review_says_what_was_left_out() {
        let dropped: Vec<String> = [
            ops::DIDNT_FIT,
            ops::DIDNT_FIT,
            "left out a sensitive topic (health)",
            "left out a phone number",
            "left out a passing detail",
            "made room in topics",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let notes = left_out(&dropped, false);
        assert_eq!(notes.len(), 5, "{notes:?}");
        assert!(notes[0].contains("2 facts didn’t fit"));
        assert!(notes[1].contains("sensitive topics are off"));
        assert!(notes[2].contains("never keeps passwords"));
        assert!(left_out(&[], false).is_empty());
    }

    /// A real export, read: `YAP_MEMORY_IMPORT_CHECK=<path> cargo test --lib
    /// memory::import -- --ignored --nocapture`. Prints counts only, never
    /// a name or a fact.
    #[test]
    #[ignore = "reads a real export named by YAP_MEMORY_IMPORT_CHECK"]
    fn a_real_export_parses() {
        let Ok(path) = std::env::var("YAP_MEMORY_IMPORT_CHECK") else {
            return;
        };
        let read = read(Path::new(&path)).expect("the export reads");
        let m = mapped(&read.docs);
        println!("files {} · not Markdown {} · from {}", m.files, read.skipped, m.from);
        let mut per: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
        for t in &m.things {
            let e = per.entry(t.section.label()).or_default();
            e.0 += 1;
            e.1 += t.facts.len();
        }
        for (s, (n, f)) in &per {
            println!("{s}: {n} items, {f} facts");
        }
        let cut = read.docs.iter().flat_map(|d| parse_claude(&d.text).facts).filter(|f| fit(f).len() > 1).count();
        println!("long facts cut where sentences end: {cut}");
        println!("lines Yap couldn't place: {:?}", m.unplaced);
        let found: usize = m.things.iter().map(|t| t.facts.len()).sum();
        let c = changes(&m.things, &[]);
        let proposed: usize = c.iter().map(|c| c.facts.len()).sum();
        println!("facts {found} · proposed {proposed} (the same fact twice in a file: {})", found - proposed);
        // Why memory merges one: it is the name or the "about", or another
        // fact of the item holds it whole (memory keeps the fuller).
        let (mut named, mut within) = (0, 0);
        for ch in &c {
            for (i, f) in ch.facts.iter().enumerate() {
                let n = ops::norm(f);
                if n == ops::norm(&ch.name) || ch.about.as_deref().is_some_and(|a| ops::norm(a) == n) {
                    named += 1;
                } else if ch.facts.iter().enumerate().any(|(j, g)| j != i && n.len() >= 8 && ops::norm(g).contains(&n)) {
                    within += 1;
                }
            }
        }
        println!("a fact that is the name or the about: {named} · held whole by another: {within}");
        for sensitive_ok in [false, true] {
            let (mut items, mut next) = (Vec::new(), 0);
            let out = ops::apply(&mut items, &mut next, &c, &Rules { sensitive_ok, ..rules() });
            let facts: usize = items.iter().map(|i| i.details.len()).sum();
            let fit_out: usize = out.done.iter().map(|d| d.facts_left_out.len()).sum();
            println!(
                "sensitive {sensitive_ok}: kept {} items, {facts} facts, {fit_out} didn't fit, {} merged by memory · {:?}",
                items.len(),
                proposed - facts - fit_out,
                left_out(&out.dropped, sensitive_ok)
            );
        }
    }
}

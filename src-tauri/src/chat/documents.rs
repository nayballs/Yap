//! **Documents beside the chat** (2026-10-30, phase 1): when an answer is
//! something to keep — a game build, a plan, a guide, a comparison — Chat
//! writes it as a document that opens in a panel to the right of the
//! conversation, fills it section by section while you watch, and later
//! questions change it.
//!
//! - **Two tools** beside the note tools (offered wherever they are):
//!   `create_document(title, sections: [{key, intent}])` makes a note in the
//!   "Chat documents" folder (`source` "chat") whose body is the title and
//!   one placeholder line per section ([`placeholder`]), and the panel opens
//!   on it at once; `write_section(note_id, key, markdown)` puts that
//!   section's `## heading` and body in its placeholder's place
//!   ([`fill_section`]). A placeholder that's gone — the person deleted or
//!   changed it — is refused, never brought back. Later changes go through
//!   `edit_note` (exact text, appearing once), `read_note` and `update_note`.
//!   Every change is a version of its own, marked Chat (`versions::by`).
//! - **The placeholder** is an HTML comment on a line of its own,
//!   `<!-- yap:section Gear | what to wear and where to find it -->`:
//!   nobody types one by accident, `core/markdown.js` draws it as a quiet
//!   "Writing: Gear…" line (never the raw syntax), and a copy of the
//!   document pasted anywhere else that renders markdown hides it.
//! - **The person's words win.** `write_section` only ever replaces its
//!   own placeholder line, and the panel's saves are merged with what Chat
//!   wrote meanwhile ([`merge3`], `chat_document_save`): the person's edit
//!   is laid over the newer text, and where both changed the same lines,
//!   theirs stays.
//! - **What the person changed since the model last saw a document** goes
//!   to the model with the next question ([`context`]): a short diff of
//!   the model's own picture of it ([`saw`], kept as its tool calls go)
//!   against the document now, bounded.
//! - **The conversation remembers its documents** on its answers
//!   (`ChatMessage::documents`: the id and the title when it was written),
//!   which draw as a "📄 title · Open" pill. An older Yap's save drops that
//!   field: the pill goes, the note stays in Notes (Chat documents).
//!
//! - **Two tool sets** (2026-10-31): the simple one above for the on-device
//!   model and small ones; for the models that handle tools well (Claude,
//!   GPT-5, Gemini, the large open models: [`rich::handles_rich`]) the rich
//!   one ([`rich`]) — the document read as blocks with ids and hashes
//!   (`read_document`, [`blocks`]), changed by block (`edit_document`: all or
//!   none, the person's changed blocks refused), its history and a restore
//!   — in place of `rewrite_section`. Both make a document outline first.
//! - **A passage selected in the panel** ("Change this…", "Ask about
//!   this"): the question carries it ([`Selection`], `chat_send`), and the
//!   model is told which blocks it is ([`selection_note`]), so a change goes
//!   to that part only.
//!
//! The log has ids, keys' counts and lengths: never a document's words.

pub mod blocks;
pub mod rich;

pub use rich::ToolSet;

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::notes::versions;

/// The folder documents go in, made the first time one is.
pub const FOLDER: &str = "Chat documents";

/// A placeholder line starts and ends so ([`placeholder`]).
const MARK_OPEN: &str = "<!-- yap:section ";
const MARK_CLOSE: &str = "-->";

/// The most sections a document starts with.
pub const MAX_SECTIONS: usize = 12;
/// A section's key (its heading) and intent, at most this many characters.
const KEY_MAX: usize = 60;
const INTENT_MAX: usize = 140;

/// What Chat's system prompt says about documents, with the note tools'
/// instructions (`tools::TOOL_INSTRUCTIONS`): when to make one (at once,
/// never offered: the on-device 4B wrote a build in the chat and asked
/// "Would you like me to create a document?", 30 Oct 2026), how it reads
/// (a summary, short bullets, tables, after Claude's own documents), a line
/// or two in the chat, and later questions changing it — or a new one for
/// an option picked (never `create_note`: Claude Haiku 5.5 made an empty
/// one for "Let's do option A"). A document is a note, saved in Notes as
/// Markdown: Claude Haiku 5.5, asked to "put it into the notes or the
/// Markdown file", made one and said it couldn't write to their notes or a
/// Markdown file (30 Oct 2026). Kept short: every step of a tool turn sends
/// it.
pub const INSTRUCTIONS: &str = "Something to keep (a build, a plan, a guide, a comparison, a long table) goes straight into a document, never the chat, never asked first: create_document (title, a one- or two-sentence summary, every section: one per part, usually 2 to 6), then write_section each, in order. Short bullets, a markdown table for anything with rows, **bold** lead-ins; a game build: race and origin, character creation, skills, a level-by-level table, gear, tips. In the chat say only one short line (\"It's open on the right.\"): no summary, never the content. A document is a Markdown note, saved in Notes under Chat documents. A change to a document from this chat, or more depth: rewrite_section (a whole section; a new heading adds one) or edit_note (a few words). An option picked: a new document. Never create_note for these.";

/// Where a document is kept, in `create_document`'s result and the
/// documents' line in a question's context: a question about notes or a
/// Markdown file gets a true answer ("It's open on the right, and saved in
/// your Notes under Chat documents."), never "I can't write to your notes".
pub const SAVED: &str = "Saved as a Markdown note in Notes, in the Chat documents folder: the user can open it there, or export it as a .md file (Export as Markdown).";

/// Added to a question asked out loud (the Voice Assistant, with Chat not
/// in front) when the model calls tools: no document unless one is asked
/// for, and then a sentence saying where it is (the person is in another
/// app).
pub const SPOKEN_NOTE: &str = "This was asked out loud: answer in speech. Make a document only when they ask for one, or for a long build, plan or guide; then say in one short sentence that it's in Chat.";

/// [`SPOKEN_NOTE`] for a Voice Assistant question asked with Chat in front
/// (`assistant::asked_in_chat`): the person is looking at Chat, so the line
/// points at the panel, as a typed question's does. "It's in Chat." to
/// someone reading Chat was the maintainer's first try (30 Oct 2026).
pub const SPOKEN_IN_CHAT_NOTE: &str = "This was asked out loud: answer in speech. Make a document only when they ask for one, or for a long build, plan or guide; then say in one short sentence that it's open on the right.";

/// The out-loud line for the Voice Assistant's question: `in_chat`, asked
/// with Chat in front.
pub fn spoken_note(in_chat: bool) -> &'static str {
    if in_chat {
        SPOKEN_IN_CHAT_NOTE
    } else {
        SPOKEN_NOTE
    }
}

// ---- the placeholder ------------------------------------------------------------------

/// `s` as a placeholder keeps it: one line, no `|`, `<`, `>` or `--`,
/// spaces collapsed, at most `max` characters.
fn clean(s: &str, max: usize) -> String {
    let one: String = s
        .chars()
        .map(|c| if c.is_control() || matches!(c, '|' | '<' | '>') { ' ' } else { c })
        .collect::<String>()
        .replace("--", "-");
    let one = one.split_whitespace().collect::<Vec<_>>().join(" ");
    let one = one.trim_matches(|c: char| c == '#' || c.is_whitespace()).to_string();
    match one.char_indices().nth(max) {
        Some((i, _)) => one[..i].trim_end().to_string(),
        None => one,
    }
}

/// The placeholder line for a section still to be written. Pure.
pub fn placeholder(key: &str, intent: &str) -> String {
    let (key, intent) = (clean(key, KEY_MAX), clean(intent, INTENT_MAX));
    if intent.is_empty() {
        format!("{MARK_OPEN}{key} {MARK_CLOSE}")
    } else {
        format!("{MARK_OPEN}{key} | {intent} {MARK_CLOSE}")
    }
}

/// A placeholder line's key and intent (`None`: not one). Pure.
pub fn parse_placeholder(line: &str) -> Option<(String, String)> {
    let inner = line.trim().strip_prefix(MARK_OPEN.trim_end())?;
    if !inner.starts_with(char::is_whitespace) {
        return None;
    }
    let inner = inner.strip_suffix(MARK_CLOSE)?;
    let (key, intent) = match inner.split_once('|') {
        Some((k, i)) => (k.trim(), i.trim()),
        None => (inner.trim(), ""),
    };
    (!key.is_empty()).then(|| (key.to_string(), intent.to_string()))
}

/// Two keys name the same section: case and spacing aside.
fn same_key(a: &str, b: &str) -> bool {
    clean(a, KEY_MAX).to_lowercase() == clean(b, KEY_MAX).to_lowercase()
}

/// A new document's body: its title, its summary (a sentence or two, when
/// given), then each section's placeholder. Pure.
pub fn skeleton(title: &str, summary: &str, sections: &[(String, String)]) -> String {
    let mut out = format!("# {}\n", title.trim());
    // A summary is a paragraph: no heading or placeholder of its own.
    let summary: Vec<&str> = summary.lines().map(str::trim).filter(|l| !l.is_empty() && heading(l).is_none() && parse_placeholder(l).is_none()).collect();
    if !summary.is_empty() {
        out.push('\n');
        out.push_str(&summary.join("\n"));
        out.push('\n');
    }
    for (key, intent) in sections {
        out.push('\n');
        out.push_str(&placeholder(key, intent));
        out.push('\n');
    }
    out
}

/// The sections of `content` still to be written, in order. Pure.
pub fn pending_keys(content: &str) -> Vec<String> {
    content.lines().filter_map(parse_placeholder).map(|(k, _)| k).collect()
}

/// Why a section wasn't written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SectionError {
    /// No placeholder for it: the person deleted or changed it (or there
    /// never was one).
    Gone,
    /// Its heading is there and its placeholder isn't: written already.
    Written,
    /// Nothing to write.
    Empty,
}

/// A section written ([`fill_section`]): the document now, the line its
/// heading is on (1 = the first), and its key as the outline spells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filled {
    pub content: String,
    pub line: usize,
    pub key: String,
}

/// A heading line's level and words (`## Gear` → (2, "Gear")). Pure.
fn heading(line: &str) -> Option<(usize, &str)> {
    let t = line.trim_start();
    let level = t.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &t[level..];
    (rest.starts_with(' ') && !rest.trim().is_empty()).then(|| (level, rest.trim()))
}

/// A section as it goes in: `## heading`, a blank line, the body. The
/// model's own first heading (`#` or `##`) is the section's heading;
/// without one, the key is. Placeholder lines in what it wrote are dropped
/// (a copied one would be a section nobody asked for). Pure.
pub fn section_block(key: &str, markdown: &str) -> String {
    let kept: Vec<&str> = markdown.lines().filter(|l| parse_placeholder(l).is_none()).collect();
    let text = kept.join("\n");
    let text = text.trim_matches(|c: char| c == '\n' || c == '\r').trim_end();
    let first = text.lines().next().unwrap_or("");
    let (title, body) = match heading(first) {
        Some((level, words)) if level <= 2 => (words.to_string(), text[first.len()..].trim_start_matches(['\r', '\n']).to_string()),
        _ => (clean(key, KEY_MAX), text.to_string()),
    };
    if body.trim().is_empty() {
        format!("## {title}")
    } else {
        format!("## {title}\n\n{body}")
    }
}

/// `content` with section `key`'s placeholder replaced by `markdown`
/// ([`section_block`]). The placeholder must be there as Yap wrote it — a
/// person's change to it or its deletion is a refusal ([`SectionError`]).
/// Pure.
pub fn fill_section(content: &str, key: &str, markdown: &str) -> Result<Filled, SectionError> {
    let lines: Vec<&str> = content.split('\n').collect();
    let found = lines.iter().enumerate().find_map(|(i, l)| parse_placeholder(l).filter(|(k, _)| same_key(k, key)).map(|(k, _)| (i, k)));
    let Some((at, own)) = found else {
        let written = lines.iter().any(|l| heading(l).is_some_and(|(level, words)| level <= 2 && same_key(words, key)));
        return Err(if written { SectionError::Written } else { SectionError::Gone });
    };
    if markdown.trim().is_empty() {
        return Err(SectionError::Empty);
    }
    // The heading as the outline spells it ("Gear", asked for as "gear").
    let block = section_block(&own, markdown);
    // A line ending the person's file used (\r\n) stays theirs.
    let block = if lines[at].ends_with('\r') { block.replace('\n', "\r\n") + "\r" } else { block };
    let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    out[at] = block;
    Ok(Filled { content: out.join("\n"), line: at + 1, key: own })
}

// ---- rewriting a section (rewrite_section) -------------------------------------------

/// Why a section wasn't rewritten ([`rewrite_section`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RewriteError {
    /// The person changed it since the model last saw it (or wrote it
    /// themselves): theirs stays until the model has read it.
    Changed,
    /// The model saw it and it's gone: the person removed it.
    Removed,
    /// The model has no picture of the document to check against.
    Unseen,
    /// Nothing to write.
    Empty,
}

/// Section `key`'s lines in `lines`, `[start, end)`: its `##` heading to the
/// next heading of level 1 or 2, or a placeholder, or the end (blank lines
/// at its end left out). Its `###` sub-headings are its own. Pure.
fn section_range(lines: &[&str], key: &str) -> Option<(usize, usize)> {
    let start = lines.iter().position(|l| heading(l).is_some_and(|(level, words)| level == 2 && same_key(words, key)))?;
    let next = lines[start + 1..].iter().position(|l| heading(l).is_some_and(|(level, _)| level <= 2) || parse_placeholder(l).is_some());
    let mut end = next.map_or(lines.len(), |p| start + 1 + p);
    while end > start + 1 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    Some((start, end))
}

/// Two copies of a section say the same (line endings and trailing spaces
/// aside).
fn same_lines(a: &[&str], b: &[&str]) -> bool {
    let tidy = |l: &&str| l.trim_end().to_string();
    a.iter().map(tidy).eq(b.iter().map(tidy))
}

/// A section rewritten ([`rewrite_section`]): the document now, the line
/// its heading is on (1 = the first), its key, and whether it's new.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewritten {
    pub content: String,
    pub line: usize,
    pub key: String,
    pub added: bool,
}

/// **`content` with section `key` rewritten as `markdown`** (its `##`
/// heading and everything under it, [`section_block`]) — or, a heading the
/// document never had, a new section at its end. `picture`: the model's
/// own picture of the document ([`saw`]). The person's words win: a section
/// that isn't what the model last saw (the person changed it, or wrote it)
/// is refused until the model reads it again, and one the model saw that's
/// gone was removed by the person — never brought back. A section still to
/// be written is filled as `write_section` would. Pure.
pub fn rewrite_section(content: &str, picture: Option<&str>, key: &str, markdown: &str) -> Result<Rewritten, RewriteError> {
    if markdown.trim().is_empty() {
        return Err(RewriteError::Empty);
    }
    if content.lines().any(|l| parse_placeholder(l).is_some_and(|(k, _)| same_key(&k, key))) {
        return fill_section(content, key, markdown)
            .map(|f| Rewritten { content: f.content, line: f.line, key: f.key, added: false })
            .map_err(|_| RewriteError::Empty);
    }
    let Some(picture) = picture else { return Err(RewriteError::Unseen) };
    let lines: Vec<&str> = content.split('\n').collect();
    let seen: Vec<&str> = picture.split('\n').collect();
    match (section_range(&lines, key), section_range(&seen, key)) {
        (Some((s, e)), Some((ps, pe))) => {
            if !same_lines(&lines[s..e], &seen[ps..pe]) {
                return Err(RewriteError::Changed);
            }
            // The heading as the document spells it, unless the model wrote its own.
            let own = heading(lines[s]).map(|(_, w)| w.to_string()).unwrap_or_else(|| clean(key, KEY_MAX));
            let block = section_block(&own, markdown);
            let key = block_key(&block, &own);
            let block = if lines[s].ends_with('\r') { block.replace('\n', "\r\n") + "\r" } else { block };
            let mut out: Vec<String> = lines[..s].iter().map(|l| l.to_string()).collect();
            out.push(block);
            out.extend(lines[e..].iter().map(|l| l.to_string()));
            Ok(Rewritten { content: out.join("\n"), line: s + 1, key, added: false })
        }
        // The person wrote this section: theirs.
        (Some(_), None) => Err(RewriteError::Changed),
        (None, Some(_)) => Err(RewriteError::Removed),
        (None, None) => {
            let block = section_block(key, markdown);
            let key = block_key(&block, key);
            let kept = content.trim_end_matches(['\n', '\r']);
            let line = kept.split('\n').count() + 2;
            Ok(Rewritten { content: format!("{kept}\n\n{block}\n"), line, key, added: true })
        }
    }
}

/// A section block's heading words ([`section_block`] always starts with
/// one); `or` if not.
fn block_key(block: &str, or: &str) -> String {
    heading(block.lines().next().unwrap_or("")).map(|(_, w)| w.to_string()).unwrap_or_else(|| clean(or, KEY_MAX))
}

// ---- three-way merge (the panel's saves) --------------------------------------------

/// One change from `base` to another text: `base[start..end]` became `lines`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Hunk {
    start: usize,
    end: usize,
    lines: Vec<String>,
}

impl Hunk {
    fn inserts(&self) -> bool {
        self.start == self.end
    }
}

/// A middle bigger than this (lines × lines) isn't aligned line by line:
/// it's one change.
const MAX_CELLS: usize = 1_000_000;

/// The changes that make `base` into `other`, in order (a longest common
/// subsequence of lines, after the common start and end). Pure.
fn hunks(base: &[&str], other: &[&str]) -> Vec<Hunk> {
    let pre = base.iter().zip(other).take_while(|(a, b)| a == b).count();
    let post = base[pre..].iter().rev().zip(other[pre..].iter().rev()).take_while(|(a, b)| a == b).count();
    let a = &base[pre..base.len() - post];
    let b = &other[pre..other.len() - post];
    let (n, m) = (a.len(), b.len());
    if n == 0 && m == 0 {
        return Vec::new();
    }
    let whole = |a_end: usize| vec![Hunk { start: pre, end: pre + a_end, lines: b.iter().map(|s| s.to_string()).collect() }];
    if n == 0 || m == 0 || n.saturating_mul(m) > MAX_CELLS {
        return whole(n);
    }
    // lcs[i][j]: the longest common run of a[i..] and b[j..] (flat).
    let w = m + 1;
    let mut lcs = vec![0u32; (n + 1) * w];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i * w + j] = if a[i] == b[j] { lcs[(i + 1) * w + j + 1] + 1 } else { lcs[(i + 1) * w + j].max(lcs[i * w + j + 1]) };
        }
    }
    let mut out: Vec<Hunk> = Vec::new();
    let mut open: Option<Hunk> = None;
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            out.extend(open.take());
            i += 1;
            j += 1;
            continue;
        }
        let h = open.get_or_insert_with(|| Hunk { start: pre + i, end: pre + i, lines: Vec::new() });
        if j < m && (i == n || lcs[i * w + j + 1] >= lcs[(i + 1) * w + j]) {
            h.lines.push(b[j].to_string());
            j += 1;
        } else {
            i += 1;
            h.end = pre + i;
        }
    }
    out.extend(open.take());
    out
}

/// Do `h` and a group of changes over `base[s..e]` (an insertion when
/// `s == e`) touch the same lines? Changes side by side don't: a person's
/// edit on the line under a section the model is writing is no clash.
fn overlaps(h: &Hunk, s: usize, e: usize) -> bool {
    match (h.inserts(), s == e) {
        (true, true) => h.start == s,
        (true, false) => s < h.start && h.start < e,
        (false, true) => h.start < s && s < h.end,
        (false, false) => h.start < e && s < h.end,
    }
}

/// `base[s..e]` with `changes` (all within it) made.
fn apply(base: &[&str], s: usize, e: usize, changes: &[Hunk]) -> Vec<String> {
    let mut out = Vec::new();
    let mut p = s;
    for h in changes {
        out.extend(base[p..h.start.max(p)].iter().map(|l| l.to_string()));
        out.extend(h.lines.iter().cloned());
        p = h.end.max(p);
    }
    out.extend(base[p.min(e)..e].iter().map(|l| l.to_string()));
    out
}

/// **The person's edit laid over the newer text** (the panel's save while
/// Chat writes, `chat_document_save`): `base` is what the panel's text box
/// started from, `mine` what the person made of it, `theirs` the document
/// now (Chat's sections written meanwhile). Line by line: what only one
/// side changed is kept, the same change once, two insertions at one place
/// both (the person's first), and where both changed the same lines the
/// person's words win. Pure.
pub fn merge3(base: &str, mine: &str, theirs: &str) -> String {
    if mine == base || mine == theirs {
        return theirs.to_string();
    }
    if theirs == base {
        return mine.to_string();
    }
    let b: Vec<&str> = base.split('\n').collect();
    let m: Vec<&str> = mine.split('\n').collect();
    let t: Vec<&str> = theirs.split('\n').collect();
    let (ha, hb) = (hunks(&b, &m), hunks(&b, &t));
    // Insertions before changes that start at the same line.
    let key = |h: &Hunk| (h.start, !h.inserts());
    let mut out: Vec<String> = Vec::new();
    let (mut i, mut j, mut pos) = (0usize, 0usize, 0usize);
    while i < ha.len() || j < hb.len() {
        let take_mine = match (ha.get(i), hb.get(j)) {
            (Some(x), Some(y)) => key(x) <= key(y),
            (Some(_), None) => true,
            _ => false,
        };
        let first = if take_mine { &ha[i] } else { &hb[j] };
        let (s, mut e) = (first.start, first.end);
        if s > pos {
            out.extend(b[pos..s].iter().map(|l| l.to_string()));
        }
        let (mut ia, mut jb) = if take_mine { (i + 1, j) } else { (i, j + 1) };
        loop {
            let mut grew = false;
            if let Some(x) = ha.get(ia).filter(|x| overlaps(x, s, e)) {
                e = e.max(x.end);
                ia += 1;
                grew = true;
            }
            if let Some(y) = hb.get(jb).filter(|y| overlaps(y, s, e)) {
                e = e.max(y.end);
                jb += 1;
                grew = true;
            }
            if !grew {
                break;
            }
        }
        let (mine_part, theirs_part) = (&ha[i..ia], &hb[j..jb]);
        let text = match (mine_part.is_empty(), theirs_part.is_empty()) {
            (false, true) => apply(&b, s, e, mine_part),
            (true, false) => apply(&b, s, e, theirs_part),
            _ => {
                let (mt, tt) = (apply(&b, s, e, mine_part), apply(&b, s, e, theirs_part));
                if mt == tt {
                    mt
                } else if s == e {
                    mt.into_iter().chain(tt).collect()
                } else {
                    mt
                }
            }
        };
        out.extend(text);
        pos = pos.max(e);
        (i, j) = (ia, jb);
    }
    out.extend(b[pos.min(b.len())..].iter().map(|l| l.to_string()));
    out.join("\n")
}

// ---- what the person changed (the next question's context) ---------------------------

/// The longest line a change shows, and the most lines in all.
const CHANGE_LINE_CHARS: usize = 160;
const CHANGE_LINES: usize = 10;

fn clip(s: &str, n: usize) -> String {
    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
    match one.char_indices().nth(n) {
        Some((i, _)) => format!("{}…", one[..i].trim_end()),
        None => one,
    }
}

/// What changed from `before` to `after`, as the model reads it: "- took
/// out: …" and "+ put in: …" lines, blank lines left out, at most
/// [`CHANGE_LINES`] (then how many more). `None`: nothing did. Pure.
pub fn changes(before: &str, after: &str) -> Option<String> {
    if before == after {
        return None;
    }
    let b: Vec<&str> = before.lines().collect();
    let a: Vec<&str> = after.lines().collect();
    let mut lines: Vec<String> = Vec::new();
    for h in hunks(&b, &a) {
        for gone in &b[h.start..h.end] {
            if !gone.trim().is_empty() {
                lines.push(format!("- took out: \"{}\"", clip(gone, CHANGE_LINE_CHARS)));
            }
        }
        for new in &h.lines {
            if !new.trim().is_empty() {
                lines.push(format!("+ put in: \"{}\"", clip(new, CHANGE_LINE_CHARS)));
            }
        }
    }
    if lines.is_empty() {
        return None; // only blank lines moved
    }
    let more = lines.len().saturating_sub(CHANGE_LINES);
    lines.truncate(CHANGE_LINES);
    if more > 0 {
        lines.push(format!("…and {more} more changed lines: read_note shows it all."));
    }
    Some(lines.join("\n"))
}

// ---- what the model last saw of each document -----------------------------------------

// What the model last saw of each document (note id → its text): its own
// picture, made of what it wrote and read — never the person's edits it
// wasn't shown. In memory: after a restart, the newest Chat version is it.
#[cfg(not(test))]
static SEEN: Mutex<Option<HashMap<u64, String>>> = Mutex::new(None);

#[cfg(not(test))]
fn with_seen<R>(f: impl FnOnce(&mut HashMap<u64, String>) -> R) -> R {
    let mut g = SEEN.lock().unwrap_or_else(|p| p.into_inner());
    f(g.get_or_insert_with(HashMap::new))
}

// Unit tests: each thread its own (note ids start again in each thread's store).
#[cfg(test)]
thread_local! {
    static SEEN_TEST: std::cell::RefCell<HashMap<u64, String>> = std::cell::RefCell::new(HashMap::new());
}

#[cfg(test)]
fn with_seen<R>(f: impl FnOnce(&mut HashMap<u64, String>) -> R) -> R {
    SEEN_TEST.with(|s| f(&mut s.borrow_mut()))
}

/// What the model last saw of note `id`: its picture, else (after a
/// restart) the newest version Chat made.
pub fn saw(id: u64) -> Option<String> {
    if let Some(text) = with_seen(|s| s.get(&id).cloned()) {
        return Some(text);
    }
    let listed = versions::list(id).ok()?;
    listed
        .into_iter()
        .find(|v| v.version.field == versions::Field::Notes && v.version.kind == versions::kind::CHAT)
        .map(|v| v.version.text)
}

/// The model has seen note `id` as `text`.
pub fn seen(id: u64, text: &str) {
    with_seen(|s| {
        s.insert(id, text.to_string());
    });
}

/// A tool call went through: the model's picture of a document follows
/// what the call showed it or did (`agent.rs`, after every call). `args`
/// as the call ran.
pub fn after_call(name: &str, args: &Value, result: &Value) {
    if result["success"] != json!(true) {
        return;
    }
    let id = result["data"]["id"].as_u64();
    let Some(id) = id else { return };
    let Some(note) = crate::notes::get(id) else { return };
    match name {
        "create_document" => seen(id, &note.content),
        "write_section" => {
            let key = args["key"].as_str().unwrap_or("");
            let md = section_text(args);
            let mine = saw(id).and_then(|s| fill_section(&s, key, &md).ok().map(|f| f.content));
            seen(id, &mine.unwrap_or(note.content));
        }
        "rewrite_section" => {
            let key = section_key_arg(args);
            let md = section_text(args);
            let mine = saw(id).and_then(|s| rewrite_section(&s, Some(&s), &key, &md).ok().map(|r| r.content));
            seen(id, &mine.unwrap_or(note.content));
        }
        "edit_note" if result["data"]["field"] == json!("content") && result["data"]["changed"] == json!(true) => {
            let (old, new) = (args["old_text"].as_str().unwrap_or(""), args["new_text"].as_str().unwrap_or(""));
            let mine = saw(id).filter(|s| !old.is_empty() && s.matches(old).count() == 1).map(|s| s.replacen(old, new, 1));
            seen(id, &mine.unwrap_or(note.content));
        }
        "update_note" if args["content"].is_string() => seen(id, &note.content),
        // A whole read of its notes: it has seen it all.
        "read_note" if result["data"]["field"] == json!("content") && result["data"]["from"] == json!(1) && result["data"]["more"] != json!(true) => {
            seen(id, &note.content)
        }
        "get_note" if result["data"]["cut"].as_array().is_none_or(|c| !c.iter().any(|f| f == "content")) => seen(id, &note.content),
        _ => {}
    }
}

// ---- the conversation's documents -----------------------------------------------------

/// A document an answer made or changed (`ChatMessage::documents`): its
/// note and its title then (the pill's words; the note may be renamed or
/// deleted since).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocRef {
    pub id: u64,
    #[serde(default)]
    pub title: String,
}

/// The documents of a conversation, newest first, each once — those whose
/// note is still there.
pub fn of_conversation(messages: &[crate::chat::chats::ChatMessage]) -> Vec<DocRef> {
    let mut out: Vec<DocRef> = Vec::new();
    for m in messages.iter().rev() {
        for d in m.documents.iter().rev() {
            if !out.iter().any(|o| o.id == d.id) && crate::notes::get(d.id).is_some() {
                out.push(d.clone());
            }
        }
    }
    out
}

/// A turn's documents (`tools::Extra::docs`): the conversation's, and the
/// ones this turn made or changed, for the answer's pills.
#[derive(Debug, Default)]
pub struct Turn {
    known: Mutex<Vec<u64>>,
    touched: Mutex<Vec<DocRef>>,
    /// The ones this turn made (`create_document`).
    made: Mutex<Vec<u64>>,
}

impl Turn {
    pub fn new(known: &[DocRef]) -> Turn {
        Turn { known: Mutex::new(known.iter().map(|d| d.id).collect()), ..Default::default() }
    }

    /// A `create_document` for a document this turn already made (the same
    /// title): not made twice — the result is the one there is, its
    /// sections still to write. Qwen3.5-4B called `create_document` twice
    /// in one step, then filled one and left the other's four placeholders
    /// (30 Oct 2026). `None`: go ahead.
    pub fn made_already(&self, name: &str, args: &Value) -> Option<Value> {
        if name != "create_document" {
            return None;
        }
        let title = clean(args["title"].as_str().unwrap_or(""), 120);
        let made = self.made.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let note = made.into_iter().filter_map(crate::notes::get).find(|n| !title.is_empty() && n.title.eq_ignore_ascii_case(&title))?;
        let left = pending_keys(&note.content);
        tracing::info!(note_id = note.id, "Chat: create_document again for this turn's document; the one there is goes back");
        Some(tool_result(
            true,
            json!({ "id": note.id, "title": note.title, "sections": left, "already": true, "next": next_for(&note.content) }),
            String::new(),
        ))
    }

    /// `rewrite_section` on a note that isn't one of this chat's documents:
    /// refused, with what to do instead. It runs after the web too (a build
    /// is researched first), so it only ever changes a document of this
    /// conversation, never another note. `None`: go ahead.
    pub fn refuses(&self, name: &str, args: &Value) -> Option<Value> {
        if !matches!(name, "rewrite_section" | "edit_document" | "restore_document") {
            return None;
        }
        let id = arg_u64(args, &["note_id", "id"])?;
        if self.known.lock().unwrap_or_else(|p| p.into_inner()).contains(&id) {
            return None;
        }
        tracing::info!(note_id = id, tool = name, "Chat: a document tool on a note that isn't this chat's document; refused");
        let chip = if name == "rewrite_section" { "Didn't rewrite a note that isn't this chat's document" } else { "Didn't change a note that isn't this chat's document" };
        Some(tool_error(
            format!("Note {id} isn't a document of this chat: {name} only changes those (the documents listed in the context). For another note, use edit_note."),
            json!({ "id": id }),
            chip.to_string(),
        ))
    }

    /// The documents this turn made that still have sections to write:
    /// `(title, keys)` of the first one (the loop asks the model to finish
    /// it before it answers, `agent.rs`).
    pub fn unfinished(&self) -> Option<(u64, String, Vec<String>)> {
        let made = self.made.lock().unwrap_or_else(|p| p.into_inner()).clone();
        made.into_iter().find_map(|id| {
            let n = crate::notes::get(id)?;
            let left = pending_keys(&n.content);
            (!left.is_empty()).then_some((id, n.title, left))
        })
    }

    /// Note `id` changed by tool `name`: a document of this chat? (Made by
    /// `create_document`, filled by `write_section`, or one the
    /// conversation has, changed by `edit_note` / `update_note`.) Kept for
    /// the answer, once, with its title now.
    pub fn noticed(&self, name: &str, id: u64) -> Option<DocRef> {
        let mut known = self.known.lock().unwrap_or_else(|p| p.into_inner());
        let ours = match name {
            // (rewrite_section, edit_document and restore_document ran only
            // on one of this chat's: `refuses`.)
            "create_document" | "write_section" | "rewrite_section" | "edit_document" | "restore_document" => true,
            "edit_note" | "update_note" => known.contains(&id),
            _ => false,
        };
        if !ours {
            return None;
        }
        if !known.contains(&id) {
            known.push(id);
        }
        if name == "create_document" {
            self.made.lock().unwrap_or_else(|p| p.into_inner()).push(id);
        }
        let title = crate::notes::get(id).map(|n| n.title).unwrap_or_default();
        let doc = DocRef { id, title };
        let mut touched = self.touched.lock().unwrap_or_else(|p| p.into_inner());
        match touched.iter_mut().find(|d| d.id == id) {
            Some(d) => d.title = doc.title.clone(),
            None => touched.push(doc.clone()),
        }
        Some(doc)
    }

    /// The documents this turn made or changed, in order.
    pub fn touched(&self) -> Vec<DocRef> {
        self.touched.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}

/// The documents of this conversation, in the next question's context
/// (a model that calls tools): which ones there are (their ids, for
/// `edit_note`) and what the person changed in each since the model last
/// saw it — then the model has seen that. At most the newest 3. `None`:
/// the conversation has none.
pub fn context(docs: &[DocRef], set: ToolSet) -> Option<String> {
    let how = match set {
        ToolSet::Simple => "change one there: rewrite_section for a whole section or a new one, edit_note for a few words; write_section only fills a section still to be written",
        ToolSet::Rich => "change one there: read_document (since a rev: only what changed after it), then edit_document; write_section only fills a section still to be written",
    };
    let mut lines = vec![format!("Documents in this chat, open beside it and saved in Notes (Chat documents) as Markdown notes ({how}):")];
    let mut changed = Vec::new();
    for d in docs.iter().take(3) {
        let Some(note) = crate::notes::get(d.id) else { continue };
        let pending = pending_keys(&note.content);
        let title = if note.title.trim().is_empty() { d.title.clone() } else { note.title.clone() };
        // The rich set: the rev it's at now (sealed: `since` finds what changes after it).
        let rev = match set {
            ToolSet::Rich => format!(", rev {}", rich::rev_seen(d.id, &note.content)),
            ToolSet::Simple => String::new(),
        };
        lines.push(if pending.is_empty() {
            format!("- \"{title}\" (note_id {}{rev})", d.id)
        } else {
            format!("- \"{title}\" (note_id {}{rev}; still to write: {})", d.id, pending.join(", "))
        });
        if let Some(before) = saw(d.id) {
            if let Some(diff) = changes(&before, &note.content) {
                changed.push(format!(
                    "Since you last saw \"{title}\", the user changed it (their changes stay; build on them, never undo them):\n{diff}"
                ));
            }
        }
        seen(d.id, &note.content);
    }
    if lines.len() == 1 {
        return None;
    }
    if !changed.is_empty() {
        tracing::info!(documents = changed.len(), "Chat: the user changed a document since the model last saw it");
    }
    lines.extend(changed);
    Some(lines.join("\n"))
}

// ---- the tools ------------------------------------------------------------------------

/// What the system prompt says about documents for `set` ([`INSTRUCTIONS`],
/// or the rich set's: [`rich::INSTRUCTIONS`]).
pub fn instructions(set: ToolSet) -> &'static str {
    match set {
        ToolSet::Simple => INSTRUCTIONS,
        ToolSet::Rich => rich::INSTRUCTIONS,
    }
}

/// The tools `set` offers (with the note tools: `notes::definitions_for`):
/// the simple set's three ([`simple_definitions`]), or `create_document`,
/// `write_section` and the rich four ([`rich::definitions`]).
pub fn definitions(set: ToolSet) -> Vec<Value> {
    let mut all = simple_definitions();
    if set == ToolSet::Rich {
        all.retain(|d| d["function"]["name"] != "rewrite_section");
        all.extend(rich::definitions());
    }
    all
}

/// The simple set's three schemas: `create_document`, `write_section`,
/// `rewrite_section`.
fn simple_definitions() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "create_document",
                "description": "Start a document shown beside the chat, saved in Notes (a build, a plan, a guide, an option picked): title, short summary, sections in order. Then write_section each.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "summary": { "type": "string", "description": "1-2 sentences: what it is, the main pick; cite web results as [1]" },
                        "sections": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "key": { "type": "string", "description": "Its heading, e.g. \"Gear\"" },
                                    "intent": { "type": "string", "description": "What it covers" }
                                },
                                "required": ["key", "intent"]
                            }
                        }
                    },
                    "required": ["title", "sections"],
                    "additionalProperties": false
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "write_section",
                "description": "Write one section of a document under its heading, in markdown. Once each; rewrite_section changes it later.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "note_id": { "type": "number" },
                        "key": { "type": "string", "description": "Its heading, as in create_document" },
                        "markdown": { "type": "string" }
                    },
                    "required": ["note_id", "key", "markdown"],
                    "additionalProperties": false
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "rewrite_section",
                "description": "Rewrite a whole section of this chat's document by its heading (a new heading adds one). Refused if the user changed it since you saw it: read_note, then retry.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "note_id": { "type": "number" },
                        "heading": { "type": "string", "description": "As in the document" },
                        "markdown": { "type": "string" }
                    },
                    "required": ["note_id", "heading", "markdown"],
                    "additionalProperties": false
                }
            }
        }),
    ]
}

/// A chip's title: quoted, ≤ 40 characters.
fn quoted(title: &str) -> String {
    let t = clip(title, 40);
    if t.is_empty() {
        "\"Untitled document\"".to_string()
    } else {
        format!("\"{t}\"")
    }
}

/// `write_section`'s text, under the names small models give it.
fn section_text(args: &Value) -> String {
    ["markdown", "content", "text", "body"].iter().find_map(|k| args[*k].as_str()).unwrap_or("").to_string()
}

/// `rewrite_section`'s heading (or `write_section`'s key), under the names
/// models give it, cleaned as a key.
fn section_key_arg(args: &Value) -> String {
    clean(["heading", "key", "section", "title"].iter().find_map(|k| args[*k].as_str()).unwrap_or(""), KEY_MAX)
}

/// The sections `create_document` was given: `[{key, intent}]` (or plain
/// strings, or a JSON string of them), cleaned, each key once, at most
/// [`MAX_SECTIONS`].
fn sections_arg(args: &Value) -> Vec<(String, String)> {
    let parsed;
    let list = match &args["sections"] {
        Value::String(s) => {
            parsed = serde_json::from_str::<Value>(s).unwrap_or(Value::Null);
            &parsed
        }
        v => v,
    };
    let mut out: Vec<(String, String)> = Vec::new();
    for item in list.as_array().into_iter().flatten() {
        let (key, intent) = match item {
            Value::String(s) => (s.as_str(), ""),
            v => (
                v["key"].as_str().or_else(|| v["heading"].as_str()).or_else(|| v["title"].as_str()).unwrap_or(""),
                v["intent"].as_str().or_else(|| v["about"].as_str()).unwrap_or(""),
            ),
        };
        let key = clean(key, KEY_MAX);
        if key.is_empty() || out.iter().any(|(k, _)| same_key(k, &key)) {
            continue;
        }
        out.push((key, clean(intent, INTENT_MAX)));
        if out.len() == MAX_SECTIONS {
            break;
        }
    }
    out
}

fn tool_result(success: bool, data: Value, display: String) -> Value {
    json!({ "success": success, "data": data, "displayText": display })
}

fn tool_error(error: String, data: Value, display: String) -> Value {
    json!({ "success": false, "error": error, "data": data, "displayText": display })
}

/// A whole number under any of `keys` (a number or a string of one).
fn arg_u64(args: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|k| match &args[*k] {
        Value::Number(n) => n.as_u64().or_else(|| n.as_f64().filter(|f| f.fract() == 0.0 && *f >= 0.0).map(|f| f as u64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    })
}

/// The tools this file runs (`notes::execute` hands them here): both sets'
/// — a model is offered one set ([`definitions`]); a name of the other set
/// it calls anyway still runs, as safely.
pub const TOOL_NAMES: &[&str] =
    &["create_document", "write_section", "rewrite_section", "read_document", "edit_document", "document_history", "restore_document"];

/// Run one of [`TOOL_NAMES`].
pub fn execute(name: &str, args: &Value) -> Value {
    match name {
        "create_document" => create(args),
        "write_section" => write(args),
        "rewrite_section" => rewrite(args),
        name if rich::TOOL_NAMES.contains(&name) => rich::execute(name, args),
        other => tool_result(false, Value::Null, format!("Unknown tool: {other}")),
    }
}

/// What a result tells the model to do next, by what's left of `content`:
/// the first section still to write — with a word to make it a table when
/// it has rows ([`table_hint`]) — else wrap up.
fn next_for(content: &str) -> String {
    match content.lines().find_map(parse_placeholder) {
        Some((key, intent)) => format!("Now call write_section for \"{key}\".{}", table_hint(&key, &intent)),
        None => FINISHED.to_string(),
    }
}

/// A section whose key or intent has rows — per level, day by day, costs,
/// a comparison, attributes — gets a word to write it as a table. On the
/// rule alone the 4B wrote 2 tables in 9 builds and plans, all bullets
/// (30 Oct 2026). Pure.
fn table_hint(key: &str, intent: &str) -> &'static str {
    static ROWS: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"(?i)\b(levels?|level[- ]by[- ]level|levell?ing|progression|per (level|day|item|person)|day[- ]by[- ]day|schedule|timetable|costs?|prices?|budget|compar\w*|vs\.?|versus|stats|attributes|options|table)\b",
        )
        .expect("static regex")
    });
    if ROWS.is_match(&format!("{key} {intent}")) {
        " Write it as a markdown table, a row each."
    } else {
        ""
    }
}

/// The last section written: what the answer is now (small models wrote a
/// summary of the document under "It's open beside the chat", 84 and 121
/// words, Qwen3.5-4B, 30 Oct 2026).
const FINISHED: &str = "Every section is written. Now answer in one short sentence, like \"It's open on the right.\" (asked for it in their notes or a file: \"It's open on the right, and saved in your Notes under Chat documents.\"): no summary, nothing from the document (the user reads it there).";

/// What the loop tells the model when it answered with sections of the
/// document it made still to write (`agent.rs`, at most [`FINISH_NUDGES`]
/// times a turn): finish first. Qwen3.5-4B wrote a comparison's table and
/// answered with its recommendation still a placeholder (30 Oct 2026).
pub fn finish_note(title: &str, left: &[String]) -> String {
    let keys = left.iter().map(|k| format!("\"{k}\"")).collect::<Vec<_>>().join(", ");
    format!(
        "(A note from Yap, not the user.) The document \"{title}\" still has sections to write: {keys}. Call write_section for each now, then answer in one short sentence."
    )
}

/// How many times a turn asks the model to finish its document.
pub const FINISH_NUDGES: usize = 2;

/// Does `question` ask for something to keep — a build, a plan, a guide,
/// an itinerary, a comparison, a list to come back to? Words a person uses
/// for one (English), never a question about what they have ("what's the
/// plan for tomorrow?"). For a model on this PC, a line in its context
/// then says to make a document (`ASKS_NOTE`): left to the system prompt's
/// rule, Qwen3.5-4B answered a repotting guide and a Lisbon plan in the
/// chat, 440 and 121 words (30 Oct 2026). Pure.
pub fn asks_for_document(question: &str) -> bool {
    static MAKE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"(?i)\b(make|write|give|create|draft|put together|build|prepare|plan|look up|find|search for|recommend|suggest)\s+(me\s+|us\s+)?(up\s+)?(a|an|my|our|the)\b[^.?!]{0,80}?\b(build|plan|guide|itinerary|checklist|packing list|shopping list|to-do list|comparison|schedule|routine|timetable|report|outline|walkthrough|tutorial|budget)s?\b",
        )
        .expect("static regex")
    });
    static PLAN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)^\s*(please\s+|can you\s+|could you\s+)?plan\s+(a|an|my|our|the|me)\b").expect("static regex")
    });
    static OTHER: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)\bstep[- ]by[- ]step\b|^\s*(please\s+)?compare\b[^?]{0,160}\b(and|with|vs\.?|versus)\b|\b(a|an)\s+[^.?!]{0,40}\bbuild\s+for\b")
            .expect("static regex")
    });
    let q = question.trim();
    if q.is_empty() || q.split_whitespace().count() < 4 {
        return false;
    }
    MAKE.is_match(q) || PLAN.is_match(q) || OTHER.is_match(q)
}

/// The line a question that asks for something to keep gets
/// ([`asks_for_document`]), in its context: now, never an answer in the
/// chat with an offer to make one (the 4B's "Would you like me to create a
/// more detailed document?", 30 Oct 2026).
pub const ASKS_NOTE: &str = "This asks for something to keep: make it a document now (create_document with a short summary and every section, then write_section for each, a markdown table for anything with rows), not an answer in the chat and not an offer to make one. Then answer in one short sentence: \"It's open on the right.\"";

/// Does `question` change a document already open in the chat, or ask for
/// more depth on part of it ("change it so it goes from level one to max",
/// "add a head torch to the extras", "go deeper on day two")? A change word,
/// and either a word pointing back at it or no ask for a new one
/// ([`asks_for_document`]). Pure.
pub fn changes_document(question: &str) -> bool {
    static CHANGE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"(?i)\b(change|edit|update|rework|redo|rewrite|revise|expand|extend|add|remove|drop|swap|replace|adjust|tweak|fix|go deeper|deeper|more detail|more depth|in depth|break (it|this|that) down|turn (it|this|that) into|make (it|this|that))\b",
        )
        .expect("static regex")
    });
    static BACK: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)\b(it|this|that|them|the (build|plan|guide|document|doc|list|markdown|file|table|section|itinerary|schedule|routine|comparison|outline))\b")
            .expect("static regex")
    });
    let q = question.trim();
    CHANGE.is_match(q) && (BACK.is_match(q) || !asks_for_document(q))
}

/// Does `question` pick one of a document's options ("Let's do option A.",
/// "I'll go with the second one")? Pure.
pub fn picks_option(question: &str) -> bool {
    static PICK: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"(?i)\b(let'?s|i'?ll|we'?ll|i'?d|i want|go with|going with|do|pick|choose|take|use|prefer|like)\b[^.?!]{0,40}?\b(option|choice|version|route|path)\s+([a-d]|[1-4]|one|two|three|four)\b|^\s*(option|choice)\s+([a-d]|[1-4])\b|\b(go|going|i'?ll go|let'?s go) with the (first|second|third|last) (one|option)\b",
        )
        .expect("static regex")
    });
    PICK.is_match(question.trim())
}

/// Does `question` ask for an earlier version of the open document back
/// ("put it back the way it was", "undo that change", "go back to the
/// previous version")? With the rich tools that's `restore_document`, not a
/// rewrite from memory. Pure.
pub fn restores_version(question: &str) -> bool {
    static BACK: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"(?i)\b(put (it|this|that|the [a-z]+) back|undo (it|this|that|the|your|those|these|my)\b|revert|roll (it |this |that )?back|go back to|back to (the|how|what) (it|way|version)|the way it was|as it was( before)?|(earlier|previous|older|last|original|first) version|restore)\b",
        )
        .expect("static regex")
    });
    BACK.is_match(question.trim())
}

/// [`ASKS_NOTE`] in one short line, for a cloud model ([`steer`]'s
/// `brief`).
pub const ASKS_BRIEF: &str = "This asks for something to keep: make the document now, not an answer in the chat or an offer to make one.";

/// **What the model is told about this question and documents** (in its
/// context, `turn.rs`, never the system prompt: the prompt cache stays
/// whole; `docs`: this chat's, newest first): an option picked from the
/// open document → a new document for it (the options stay as they were:
/// worth keeping, and the pick needs an outline of its own); a change, or
/// more depth on part of it → that document reworked (`rewrite_section`,
/// `edit_note`), never the chat; else, asked for something to keep →
/// [`ASKS_NOTE`]. Left to the rule, the 4B wrote a changed build in the
/// chat and offered a document, and for "Let's do option A" edited the
/// options or copied them to the clipboard (30 Oct 2026). **A cloud model**
/// (`brief`) gets the same nudge as one short line: Claude Haiku 5.5
/// answered "Could you find a mage build instead…?" in the chat, ending
/// "Want me to build a level-by-level plan for it?", and made the document
/// only after "Yes, please." (30 Oct 2026). `None`: nothing to say.
pub fn steer(question: &str, docs: &[DocRef], brief: bool, set: ToolSet) -> Option<String> {
    if let Some(d) = docs.first() {
        let title = crate::notes::get(d.id).map(|n| n.title).filter(|t| !t.trim().is_empty()).unwrap_or_else(|| d.title.clone());
        let id = d.id;
        if picks_option(question) {
            return Some(if brief {
                format!("This picks an option in \"{title}\" (note_id {id}): make a new document for that pick, built out in full, not the chat.")
            } else {
                format!(
                    "This picks one of the options in \"{title}\" (note_id {id}): make a new document for that pick (create_document with a summary and every section, then write_section for each), built out in full; \"{title}\" stays as it is. Then answer in one short sentence."
                )
            });
        }
        // An earlier version back (the rich set has its history and restore).
        if set == ToolSet::Rich && restores_version(question) {
            return Some(format!(
                "This asks for an earlier version of \"{title}\" (note_id {id}) back: document_history, then restore_document with the version from before the change (not a rewrite from memory)."
            ));
        }
        if changes_document(question) {
            return Some(match (set, brief) {
                (ToolSet::Rich, true) => format!("This changes \"{title}\" (note_id {id}): rework it there (read_document, then edit_document), not in the chat."),
                (ToolSet::Rich, false) => format!(
                    "This changes \"{title}\" (note_id {id}), open beside the chat: rework it there, not in the chat and not as an offer — read_document, then edit_document on the blocks that change (a table row: replace the whole table). Then answer in one short sentence."
                ),
                (ToolSet::Simple, true) => format!("This changes \"{title}\" (note_id {id}): rework it there (rewrite_section, edit_note), not in the chat."),
                (ToolSet::Simple, false) => format!(
                    "This changes \"{title}\" (note_id {id}), open beside the chat: rework it there, not in the chat and not as an offer — rewrite_section for each section that changes (a new heading adds a section), edit_note for a few words. Then answer in one short sentence."
                ),
            });
        }
    }
    asks_for_document(question).then(|| if brief { ASKS_BRIEF } else { ASKS_NOTE }.to_string())
}

/// **A document's `[1]` citations as links to their web pages**
/// (`[site](address)`): a note has no Sources row under it, so its
/// citations go to the pages themselves. `sources`: the turn's web sources
/// (number, label, address); a number it doesn't know stays as written, and
/// a markdown link's own text ("[1](https://…)") isn't a citation. Pure.
pub fn link_citations(markdown: &str, sources: &[(usize, String, String)]) -> String {
    static RUN: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"(?:\[\d{1,2}(?:\s*[,–-]\s*\d{1,2})*\])+").expect("static regex"));
    if sources.is_empty() || !markdown.contains('[') {
        return markdown.to_string();
    }
    let mut out = String::with_capacity(markdown.len());
    let mut last = 0;
    for m in RUN.find_iter(markdown) {
        if markdown[m.end()..].starts_with('(') {
            continue;
        }
        let nums = crate::web::citations(m.as_str());
        let found: Vec<&(usize, String, String)> = nums.iter().filter_map(|n| sources.iter().find(|s| s.0 == *n)).collect();
        if found.is_empty() || found.len() != nums.len() {
            continue;
        }
        let links: Vec<String> = found
            .iter()
            .map(|(_, label, url)| format!("[{}]({})", label.replace(['[', ']'], ""), url.replace(['(', ')', ' '], "")))
            .collect();
        out.push_str(&markdown[last..m.start()]);
        out.push_str(&format!("({})", links.join(", ")));
        last = m.end();
    }
    out.push_str(&markdown[last..]);
    out
}

fn create(args: &Value) -> Value {
    const CHIP: &str = "Couldn't start a document";
    let title = clean(args["title"].as_str().unwrap_or(""), 120);
    let sections = sections_arg(args);
    if title.is_empty() {
        return tool_error("title is required: the document's name.".to_string(), Value::Null, CHIP.to_string());
    }
    if sections.is_empty() {
        return tool_error(
            "sections is required: the document's sections in order, each {\"key\": its heading, \"intent\": what it will say}.".to_string(),
            Value::Null,
            CHIP.to_string(),
        );
    }
    let folder = crate::notes::find_folder(FOLDER).unwrap_or_else(|| {
        crate::notes::folder_create(FOLDER);
        FOLDER.to_string()
    });
    let summary = args["summary"].as_str().unwrap_or("").trim().to_string();
    let body = skeleton(&title, &summary, &sections);
    let note = crate::notes::create(&title, "", "chat", &folder);
    // The outline is Chat's first version of it (Undo goes back to it).
    let chat = versions::How::new(versions::kind::CHAT);
    let written = versions::by(chat, || {
        crate::notes::modify(note.id, |n| {
            n.content = body.clone();
            Ok(((), true))
        })
    });
    if let Err(e) = written {
        return tool_error(e.clone(), Value::Null, format!("{CHIP}: {e}"));
    }
    let keys: Vec<String> = sections.iter().map(|(k, _)| k.clone()).collect();
    tracing::info!(note_id = note.id, sections = keys.len(), summary = !summary.is_empty(), "Chat: a document started");
    tool_result(
        true,
        json!({ "id": note.id, "title": title, "folder": folder, "saved": SAVED, "sections": keys, "next": next_for(&body) }),
        format!("Started {} ({} section{})", quoted(&title), keys.len(), if keys.len() == 1 { "" } else { "s" }),
    )
}

fn write(args: &Value) -> Value {
    let Some(id) = arg_u64(args, &["note_id", "id"]) else {
        return tool_error(
            "note_id is required: the document's id from create_document.".to_string(),
            Value::Null,
            "Couldn't write a section".to_string(),
        );
    };
    let key = clean(args["key"].as_str().or_else(|| args["heading"].as_str()).unwrap_or(""), KEY_MAX);
    if key.is_empty() {
        return tool_error("key is required: the section's heading, as given to create_document.".to_string(), json!({ "id": id }), "Couldn't write a section".to_string());
    }
    let markdown = section_text(args);
    let chat = versions::How::new(versions::kind::CHAT);
    let outcome = versions::by(chat, || {
        crate::notes::modify(id, |note| match fill_section(&note.content, &key, &markdown) {
            Ok(filled) => {
                note.content = filled.content;
                let left = pending_keys(&note.content);
                Ok((Ok((filled.line, filled.key, left, next_for(&note.content), note.title.clone())), true))
            }
            Err(e) => Ok((Err((e, pending_keys(&note.content), note.title.clone())), false)),
        })
    });
    match outcome {
        Err(e) => tool_result(false, Value::Null, e),
        Ok(Ok((line, key, left, next, title))) => {
            tracing::info!(note_id = id, left = left.len(), chars = markdown.chars().count(), "Chat: a document's section written");
            tool_result(
                true,
                json!({ "id": id, "key": key, "changed": true, "line": line, "left": left, "next": next }),
                format!("Wrote \"{}\" in {}", clip(&key, 30), quoted(&title)),
            )
        }
        Ok(Err((why, left, title))) => {
            tracing::info!(note_id = id, why = ?why, "Chat: a document's section not written");
            let still = if left.is_empty() { "none".to_string() } else { left.iter().map(|k| format!("\"{k}\"")).collect::<Vec<_>>().join(", ") };
            let (error, chip) = match why {
                SectionError::Gone => (
                    format!("There's no section \"{key}\" waiting to be written in \"{title}\": the user removed or changed it, so nothing was written. Don't write it again or add it back. Sections still to write: {still}."),
                    format!("Didn't write \"{}\": it's no longer in {}", clip(&key, 30), quoted(&title)),
                ),
                SectionError::Written => (
                    format!("\"{key}\" is already written in \"{title}\". To change it, read_note then edit_note. Sections still to write: {still}."),
                    format!("\"{}\" is already written in {}", clip(&key, 30), quoted(&title)),
                ),
                SectionError::Empty => (
                    "markdown is empty: write the section's body.".to_string(),
                    format!("Couldn't write \"{}\"", clip(&key, 30)),
                ),
            };
            tool_error(error, json!({ "id": id, "key": key, "left": left }), chip)
        }
    }
}

fn rewrite(args: &Value) -> Value {
    const CHIP: &str = "Couldn't rewrite a section";
    let Some(id) = arg_u64(args, &["note_id", "id"]) else {
        return tool_error("note_id is required: the document's id.".to_string(), Value::Null, CHIP.to_string());
    };
    let key = section_key_arg(args);
    if key.is_empty() {
        return tool_error("heading is required: the section's heading, as in the document.".to_string(), json!({ "id": id }), CHIP.to_string());
    }
    // Only a document Chat made (the turn checks it's this chat's too).
    match crate::notes::get(id) {
        Some(n) if n.source == "chat" => {}
        Some(_) => {
            return tool_error(
                format!("Note {id} isn't a document Chat made: rewrite_section only changes those. Use edit_note."),
                json!({ "id": id }),
                CHIP.to_string(),
            )
        }
        None => return tool_error(format!("No note {id}."), Value::Null, CHIP.to_string()),
    }
    let markdown = section_text(args);
    let picture = saw(id);
    let chat = versions::How::new(versions::kind::CHAT);
    let outcome = versions::by(chat, || {
        crate::notes::modify(id, |note| match rewrite_section(&note.content, picture.as_deref(), &key, &markdown) {
            Ok(r) => {
                note.content = r.content;
                Ok((Ok((r.line, r.key, r.added, note.title.clone())), true))
            }
            Err(e) => Ok((Err((e, note.title.clone())), false)),
        })
    });
    match outcome {
        Err(e) => tool_result(false, Value::Null, e),
        Ok(Ok((line, key, added, title))) => {
            tracing::info!(note_id = id, added, chars = markdown.chars().count(), "Chat: a document's section rewritten");
            let display = if added {
                format!("Added \"{}\" to {}", clip(&key, 30), quoted(&title))
            } else {
                format!("Rewrote \"{}\" in {}", clip(&key, 30), quoted(&title))
            };
            tool_result(
                true,
                json!({ "id": id, "key": key, "changed": true, "added": added, "line": line, "next": "Rewrite any other section the change touches, then answer in one short sentence: nothing from the document." }),
                display,
            )
        }
        Ok(Err((why, title))) => {
            tracing::info!(note_id = id, why = ?why, "Chat: a document's section not rewritten");
            let (error, chip) = match why {
                RewriteError::Changed => (
                    format!("The user changed \"{key}\" in \"{title}\" since you last saw it, so nothing was rewritten: read_note first, then rewrite it keeping their changes."),
                    format!("Didn't rewrite \"{}\": changed in {} since Chat saw it", clip(&key, 30), quoted(&title)),
                ),
                RewriteError::Unseen => (
                    format!("You haven't read \"{title}\" yet, so nothing was rewritten: read_note first, then rewrite the section."),
                    format!("Didn't rewrite \"{}\" in {}: not read yet", clip(&key, 30), quoted(&title)),
                ),
                RewriteError::Removed => (
                    format!("The user removed \"{key}\" from \"{title}\", so nothing was written. Don't add it back."),
                    format!("Didn't rewrite \"{}\": it's no longer in {}", clip(&key, 30), quoted(&title)),
                ),
                RewriteError::Empty => ("markdown is empty: write the whole section.".to_string(), format!("Couldn't rewrite \"{}\"", clip(&key, 30))),
            };
            tool_error(error, json!({ "id": id, "key": key }), chip)
        }
    }
}

/// A turn's chips with a section's written chip folded into the one before
/// it for the same document ("Wrote 3 sections in "Build""): a document's
/// sections are one thing done, not a column of chips. Pure.
pub fn push_chip(chips: &mut Vec<String>, chip: String) {
    static ONE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r#"^(Wrote|Rewrote) "[^"]*" in (".*")$"#).expect("static regex"));
    static MANY: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r#"^(Wrote|Rewrote) (\d+) sections in (".*")$"#).expect("static regex"));
    if let (Some(new), Some(last)) = (ONE.captures(&chip), chips.last()) {
        let (verb, doc) = (new[1].to_string(), new[2].to_string());
        let count = match (ONE.captures(last), MANY.captures(last)) {
            (Some(c), _) if c[1] == *verb && c[2] == *doc => Some(2),
            (_, Some(c)) if c[1] == *verb && c[3] == *doc => c[2].parse::<usize>().ok().map(|n| n + 1),
            _ => None,
        };
        if let Some(n) = count {
            if let Some(last) = chips.last_mut() {
                *last = format!("{verb} {n} sections in {doc}");
            }
            return;
        }
    }
    chips.push(chip);
}

// ---- the live section (a write_section's arguments as they stream) --------------------

/// A JSON string value whose end may not have come yet, decoded so far: the
/// text after `"field":` up to its closing quote or the end of `json` (a
/// half escape at the end left out). `None`: the field hasn't started.
/// Pure.
pub fn partial_string(json: &str, field: &str) -> Option<String> {
    let needle = format!("\"{field}\"");
    let mut from = 0;
    let start = loop {
        let at = json[from..].find(&needle)? + from;
        from = at + needle.len();
        if at > 0 && json.as_bytes()[at - 1] == b'\\' {
            continue;
        }
        let rest = json[from..].trim_start();
        if let Some(after) = rest.strip_prefix(':') {
            let after = after.trim_start();
            if let Some(value) = after.strip_prefix('"') {
                break json.len() - value.len();
            }
            return None;
        }
    };
    let mut out = String::new();
    let mut chars = json[start..].chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('b') => out.push('\u{8}'),
                Some('f') => out.push('\u{c}'),
                Some('u') => {
                    let hex: String = chars.by_ref().take(4).collect();
                    let Ok(code) = u32::from_str_radix(&hex, 16) else { return Some(out) };
                    if hex.len() < 4 {
                        return Some(out);
                    }
                    if (0xD800..0xDC00).contains(&code) {
                        // A pair: the low half follows as \uXXXX.
                        let rest: String = chars.by_ref().take(6).collect();
                        let low = rest.strip_prefix("\\u").and_then(|h| u32::from_str_radix(h, 16).ok());
                        match low {
                            Some(low) if rest.len() == 6 && (0xDC00..0xE000).contains(&low) => {
                                if let Some(ch) = char::from_u32(0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00)) {
                                    out.push(ch);
                                }
                            }
                            _ => return Some(out),
                        }
                    } else if let Some(ch) = char::from_u32(code) {
                        out.push(ch);
                    }
                }
                Some(other) => out.push(other),
                None => return Some(out),
            },
            c => out.push(c),
        }
    }
    Some(out)
}

/// A whole number field of arguments still coming (`"note_id": 12`, or
/// `"12"`), once its digits have ended. Pure.
pub fn partial_number(json: &str, field: &str) -> Option<u64> {
    let needle = format!("\"{field}\"");
    let at = json.find(&needle)? + needle.len();
    let rest = json[at..].trim_start().strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"').unwrap_or(rest);
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    // Digits still coming: not yet.
    let after = rest[digits.len()..].chars().next()?;
    (!digits.is_empty() && !after.is_ascii_digit() && after != '.').then(|| digits.parse().ok()).flatten()
}

/// A `write_section` or `rewrite_section` call's arguments so far, as the
/// panel shows them: `(note_id, key, the section so far)`, once the note
/// and the key (`heading`) are whole. Pure.
pub fn live_section(args_so_far: &str) -> Option<(u64, String, String)> {
    let id = partial_number(args_so_far, "note_id").or_else(|| partial_number(args_so_far, "id"))?;
    // The key whole: its closing quote has come.
    let field = ["key", "heading"].into_iter().find(|f| partial_string(args_so_far, f).is_some())?;
    let key = partial_string(args_so_far, field)?;
    let key_done = {
        let needle = format!("\"{field}\"");
        args_so_far.find(&needle).is_some_and(|at| {
            let rest = &args_so_far[at + needle.len()..];
            rest.trim_start().strip_prefix(':').map(str::trim_start).and_then(|r| r.strip_prefix('"')).is_some_and(|v| {
                let mut esc = false;
                v.chars().any(|c| {
                    let end = c == '"' && !esc;
                    esc = c == '\\' && !esc;
                    end
                })
            })
        })
    };
    if !key_done || key.trim().is_empty() {
        return None;
    }
    let text = ["markdown", "content", "text", "body"].iter().find_map(|f| partial_string(args_so_far, f)).unwrap_or_default();
    Some((id, clean(&key, KEY_MAX), text))
}

// ---- a passage selected in the panel ----------------------------------------------------

/// A passage the person selected in the document panel and asked about
/// ("Change this…", "Ask about this"; `chat_send`'s `selection`): the
/// document, the words as selected, and the note's lines it covers (0 = the
/// first; `None` when the page couldn't say).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Selection {
    pub note_id: u64,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub from_line: Option<usize>,
    #[serde(default)]
    pub to_line: Option<usize>,
}

/// The passage a question was asked about, kept with it
/// (`ChatMessage::quote`): the quote above the question in Chat, and in the
/// history the model reads later. An older Yap's save drops it: the
/// question stays, without its quote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Quote {
    pub note_id: u64,
    pub text: String,
}

/// The most of a selection kept with its question.
const QUOTE_CHARS: usize = 600;

impl Selection {
    /// What's kept with the question.
    pub fn quote(&self) -> Option<Quote> {
        let text: String = self.text.trim().chars().take(QUOTE_CHARS).collect();
        (!text.is_empty()).then_some(Quote { note_id: self.note_id, text })
    }
}

tokio::task_local! {
    static SELECTED: Option<Selection>;
}

/// Run `f` (a Chat turn) with the passage its question is about.
pub async fn with_selection<F: std::future::Future>(selection: Option<Selection>, f: F) -> F::Output {
    SELECTED.scope(selection, f).await
}

/// The passage this turn's question is about, if any.
pub fn selection() -> Option<Selection> {
    SELECTED.try_with(|s| s.clone()).ok().flatten()
}

/// The source text of a selection's blocks given to the model, at most.
const SELECTED_CHARS: usize = 1_500;

/// **What the model is told about a selected passage** (in the question's
/// context): which document, the words selected, and the blocks they're in
/// — for the rich set their ids and hashes (an `edit_document` straight
/// on them), for the simple set their markdown to copy `edit_note`'s
/// old_text from — and that a change goes there only; a model without
/// tools (`set` `None`) the words alone. `None`: no such note, or nothing
/// selected.
pub fn selection_note(sel: &Selection, set: Option<ToolSet>) -> Option<String> {
    let note = crate::notes::get(sel.note_id)?;
    let words = clip(&sel.text, 400);
    if words.is_empty() {
        return None;
    }
    let doc = blocks::Doc::of(&note.content);
    let lines = sel.from_line.zip(sel.to_line.or(sel.from_line));
    let at = rich::selected_blocks(&doc, lines, &sel.text);
    let title = if note.title.trim().is_empty() { "Untitled document".to_string() } else { note.title.clone() };
    let mut source = String::new();
    for i in &at {
        let text = doc.blocks[*i].text.replace('\r', "");
        if !source.is_empty() {
            source.push_str("\n\n");
        }
        source.push_str(&text);
        if source.chars().count() > SELECTED_CHARS {
            source = format!("{}…", source.chars().take(SELECTED_CHARS).collect::<String>());
            break;
        }
    }
    tracing::info!(note_id = note.id, blocks = at.len(), chars = sel.text.chars().count(), set = set.map_or("none", ToolSet::name), "Chat: a question about a selected passage");
    let head = format!("The user selected this passage in \"{title}\" (note_id {}) and asks about it: \"{words}\"", note.id);
    Some(match (set, at.is_empty()) {
        (None, _) => format!("The user selected this passage in the document \"{title}\" and asks about it: \"{words}\""),
        (_, true) => format!("{head}. If the question asks for a change, change only that passage, nothing else."),
        (Some(ToolSet::Rich), false) => {
            let ids: Vec<String> = at.iter().map(|i| format!("{} (hash {})", doc.ids[*i].id, doc.ids[*i].hash)).collect();
            format!(
                "{head}. It's in block{} {} of the document:\n{source}\nIf the question asks for a change, change only those blocks (edit_document, replace with those if_hash), nothing else; else answer about that passage.",
                if at.len() == 1 { "" } else { "s" },
                ids.join(", ")
            )
        }
        (Some(ToolSet::Simple), false) => format!(
            "{head}. In the document's markdown it's:\n{source}\nIf the question asks for a change, change only that part (edit_note with old_text copied exactly from it, or rewrite_section for its whole section), nothing else; else answer about that passage."
        ),
    })
}

// ---- the panel's save -----------------------------------------------------------------

/// The Chat panel's Edit box saved (`base`: the text it started from or
/// last took in; `content`: the box now): merged with what Chat wrote
/// meanwhile ([`merge3`]) and saved as typed (a version that folds with
/// the person's other typing). Every window follows (`yap-note-changed`,
/// `origin`). Returns the document as saved, for the box to take in.
#[tauri::command]
pub fn chat_document_save(app: tauri::AppHandle, id: u64, base: String, content: String, origin: Option<String>) -> Result<String, String> {
    let typed = versions::How::new(versions::kind::TYPED);
    let saved = versions::by(typed, || {
        crate::notes::modify(id, |note| {
            let merged = merge3(&base, &content, &note.content);
            let changed = merged != note.content;
            if changed && note.content != base {
                tracing::info!(note_id = id, "Chat: the panel's edit merged with Chat's writing");
            }
            note.content = merged.clone();
            Ok((merged, changed))
        })
    })?;
    crate::notes::commands::note_changed(&app, id, origin.as_deref().unwrap_or("chat-panel"));
    crate::notes::saved()?;
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sections(keys: &[(&str, &str)]) -> Vec<(String, String)> {
        keys.iter().map(|(k, i)| (k.to_string(), i.to_string())).collect()
    }

    #[test]
    fn a_placeholder_says_its_key_and_intent_and_nothing_breaks_it() {
        let line = placeholder("Gear", "what to wear and where to find it");
        assert_eq!(line, "<!-- yap:section Gear | what to wear and where to find it -->");
        assert_eq!(parse_placeholder(&line), Some(("Gear".into(), "what to wear and where to find it".into())));
        // A key or intent can't end the comment early or split it.
        let odd = placeholder("Gear | Rings -->", "a <b> -- c\nd");
        assert_eq!(parse_placeholder(&odd).unwrap().0, "Gear Rings -");
        assert!(odd.matches("-->").count() == 1, "{odd}");
        assert!(!odd.contains('\n'));
        // Not a placeholder: an ordinary comment, a heading, text.
        assert_eq!(parse_placeholder("<!-- a note -->"), None);
        assert_eq!(parse_placeholder("## Gear"), None);
        assert_eq!(parse_placeholder("<!-- yap:section  -->"), None);
        // Without an intent.
        assert_eq!(parse_placeholder(&placeholder("Gear", "")), Some(("Gear".into(), String::new())));
    }

    #[test]
    fn the_skeleton_is_the_title_and_a_placeholder_per_section() {
        let body = skeleton("Lone wolf build", "", &sections(&[("Gear", "what to wear"), ("Skills", "which and when")]));
        assert_eq!(
            body,
            "# Lone wolf build\n\n<!-- yap:section Gear | what to wear -->\n\n<!-- yap:section Skills | which and when -->\n"
        );
        assert_eq!(pending_keys(&body), ["Gear", "Skills"]);
        assert_eq!(pending_keys(&body).len(), 2);
    }

    #[test]
    fn a_section_fills_its_placeholder_and_nothing_else() {
        let body = skeleton("Trip", "", &sections(&[("Days", "day by day"), ("Costs", "what it costs")]));
        let f = fill_section(&body, "days", "- Day 1: arrive\n- Day 2: walk").unwrap();
        assert_eq!(f.line, 3);
        assert_eq!(f.content, "# Trip\n\n## Days\n\n- Day 1: arrive\n- Day 2: walk\n\n<!-- yap:section Costs | what it costs -->\n");
        assert_eq!(pending_keys(&f.content), ["Costs"]);
        // Its own heading is the section's (## or #), not doubled.
        let f2 = fill_section(&f.content, "Costs", "# Costs and money\n\n| Item | £ |\n|---|---|\n| Train | 40 |").unwrap();
        assert!(f2.content.ends_with("## Costs and money\n\n| Item | £ |\n|---|---|\n| Train | 40 |\n"), "{}", f2.content);
        assert!(pending_keys(&f2.content).is_empty());
        // Written already: refused, and said so.
        assert_eq!(fill_section(&f2.content, "Days", "again"), Err(SectionError::Written));
        // Nothing to write.
        assert_eq!(fill_section(&body, "Days", "  \n"), Err(SectionError::Empty));
        // A placeholder copied into the section doesn't come back as one.
        let f3 = fill_section(&body, "Days", "Walk.\n<!-- yap:section Extra | more -->").unwrap();
        assert_eq!(pending_keys(&f3.content), ["Costs"]);
    }

    #[test]
    fn write_section_refuses_a_placeholder_the_person_removed_or_changed() {
        let body = skeleton("Plan", "", &sections(&[("Gear", "kit"), ("Route", "the way")]));
        // Deleted: gone, never brought back.
        let deleted = body.replace("<!-- yap:section Gear | kit -->\n", "");
        assert_eq!(fill_section(&deleted, "Gear", "Boots"), Err(SectionError::Gone));
        // Changed by hand (they typed into it): gone too.
        let typed = body.replace("<!-- yap:section Gear | kit -->", "Gear: I'll bring my own.");
        assert_eq!(fill_section(&typed, "Gear", "Boots"), Err(SectionError::Gone));
        // The other section still fills.
        assert!(fill_section(&typed, "Route", "North.").is_ok());
    }

    #[test]
    fn the_tools_make_a_document_fill_it_and_refuse_a_removed_section() {
        let made = execute("create_document", &json!({
            "title": "Lone wolf build",
            "sections": [{ "key": "Gear", "intent": "what to wear" }, { "key": "Skills", "intent": "which" }, "Talents"]
        }));
        assert_eq!(made["success"], json!(true), "{made}");
        let id = made["data"]["id"].as_u64().unwrap();
        assert_eq!(made["data"]["sections"], json!(["Gear", "Skills", "Talents"]));
        assert_eq!(made["displayText"], json!("Started \"Lone wolf build\" (3 sections)"));
        assert!(made["data"]["next"].as_str().unwrap().contains("write_section for \"Gear\""));
        let note = crate::notes::get(id).unwrap();
        assert_eq!((note.folder.as_str(), note.source.as_str()), (FOLDER, "chat"));
        assert!(crate::notes::folders().iter().any(|f| f == FOLDER));
        // The outline is Chat's first version.
        let v = versions::list(id).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].version.kind, versions::kind::CHAT);

        let wrote = execute("write_section", &json!({ "note_id": id, "key": "gear", "markdown": "Two rings." }));
        assert_eq!(wrote["success"], json!(true), "{wrote}");
        assert_eq!(wrote["data"]["left"], json!(["Skills", "Talents"]));
        assert_eq!(wrote["displayText"], json!("Wrote \"Gear\" in \"Lone wolf build\""));
        // Every change a version, marked Chat.
        let v = versions::list(id).unwrap();
        assert_eq!(v.len(), 2);
        assert!(v.iter().all(|x| x.version.kind == versions::kind::CHAT));

        // The person deletes "Skills" in the panel: refused, nothing changes.
        let before = crate::notes::get(id).unwrap().content;
        let theirs = before.replace("<!-- yap:section Skills | which -->\n", "");
        crate::notes::update(id, None, Some(theirs.clone()), None, None).unwrap();
        let r = execute("write_section", &json!({ "note_id": id, "key": "Skills", "markdown": "Shadow." }));
        assert_eq!(r["success"], json!(false));
        assert!(r["error"].as_str().unwrap().contains("the user removed or changed it"), "{r}");
        assert!(r["error"].as_str().unwrap().contains("\"Talents\""), "{r}");
        assert_eq!(r["displayText"], json!("Didn't write \"Skills\": it's no longer in \"Lone wolf build\""));
        assert_eq!(crate::notes::get(id).unwrap().content, theirs);
        // Again: already written.
        let r = execute("write_section", &json!({ "note_id": id, "key": "Gear", "markdown": "x" }));
        assert!(r["error"].as_str().unwrap().contains("already written"), "{r}");
        // The last one says to wrap up.
        let r = execute("write_section", &json!({ "note_id": id, "key": "Talents", "content": "Lone Wolf." }));
        assert!(r["data"]["next"].as_str().unwrap().starts_with("Every section is written"), "{r}");
        // Missing pieces are explained.
        assert!(execute("write_section", &json!({ "key": "x", "markdown": "y" }))["error"].as_str().unwrap().contains("note_id is required"));
        assert!(execute("create_document", &json!({ "title": "x" }))["error"].as_str().unwrap().contains("sections is required"));
        assert!(execute("create_document", &json!({ "sections": ["a"] }))["error"].as_str().unwrap().contains("title is required"));
    }

    #[test]
    fn the_person_s_edit_is_merged_over_chat_s_writing() {
        let base = skeleton("Plan", "", &sections(&[("Gear", "kit"), ("Route", "the way")]));
        // Chat wrote Gear meanwhile; the person added a line at the end and
        // fixed the title.
        let theirs = fill_section(&base, "Gear", "Boots.").unwrap().content;
        let mine = base.replace("# Plan", "# Plan for May") + "My own note.\n";
        let merged = merge3(&base, &mine, &theirs);
        assert!(merged.starts_with("# Plan for May\n"), "{merged}");
        assert!(merged.contains("## Gear\n\nBoots."), "{merged}");
        assert!(merged.contains("<!-- yap:section Route | the way -->"), "{merged}");
        assert!(merged.ends_with("My own note.\n"), "{merged}");
        // A line right under the section being written: no clash.
        let mine = base.replace("<!-- yap:section Gear | kit -->", "<!-- yap:section Gear | kit -->\nWalking only.");
        let merged = merge3(&base, &mine, &theirs);
        assert!(merged.contains("## Gear\n\nBoots.") && merged.contains("Walking only."), "{merged}");
        // Both changed the same line: the person's words win.
        let mine = base.replace("<!-- yap:section Gear | kit -->", "Gear: my own list.");
        let merged = merge3(&base, &mine, &theirs);
        assert!(merged.contains("Gear: my own list.") && !merged.contains("Boots."), "{merged}");
        // Both added at the same place: both, theirs after.
        let merged = merge3("a\nb", "a\nmine\nb", "a\ntheirs\nb");
        assert_eq!(merged, "a\nmine\ntheirs\nb");
        // Nothing changed on one side: the other.
        assert_eq!(merge3("a", "a", "b"), "b");
        assert_eq!(merge3("a", "b", "a"), "b");
        assert_eq!(merge3("a", "b", "b"), "b");
    }

    #[test]
    fn what_the_person_changed_is_said_short() {
        let before = "# Build\n\n## Gear\n\nTwo rings.\nA staff.\n";
        let after = "# Build\n\n## Gear\n\nTwo rings and a cloak.\nA staff.\n";
        assert_eq!(
            changes(before, after).unwrap(),
            "- took out: \"Two rings.\"\n+ put in: \"Two rings and a cloak.\""
        );
        assert_eq!(changes(before, before), None);
        // Only blank lines: nothing to say.
        assert_eq!(changes("a\n\nb", "a\nb"), None);
        // Many changes: bounded, and says how many more.
        let long_before: String = (0..30).map(|i| format!("line {i}\n")).collect();
        let long_after: String = (0..30).map(|i| format!("row {i}\n")).collect();
        let said = changes(&long_before, &long_after).unwrap();
        assert_eq!(said.lines().count(), CHANGE_LINES + 1);
        assert!(said.ends_with("…and 50 more changed lines: read_note shows it all."), "{said}");
        // A long line is cut.
        let big = "x".repeat(400);
        assert!(changes("a", &big).unwrap().chars().count() < 200);
    }

    #[test]
    fn the_next_question_hears_what_the_person_changed_once() {
        let made = execute("create_document", &json!({ "title": "Build", "sections": [{ "key": "Gear", "intent": "kit" }] }));
        let id = made["data"]["id"].as_u64().unwrap();
        after_call("create_document", &json!({}), &made);
        let args = json!({ "note_id": id, "key": "Gear", "markdown": "Two rings." });
        let wrote = execute("write_section", &args);
        after_call("write_section", &args, &wrote);
        let docs = vec![DocRef { id, title: "Build".into() }];
        // Nothing changed by the person: the list alone.
        let c = context(&docs, ToolSet::Simple).unwrap();
        assert!(c.contains("\"Build\" (note_id"), "{c}");
        assert!(!c.contains("the user changed it"), "{c}");
        // The person edits: the next question says what.
        let now = crate::notes::get(id).unwrap().content.replace("Two rings.", "Two rings and a cloak.");
        crate::notes::update(id, None, Some(now), None, None).unwrap();
        let c = context(&docs, ToolSet::Simple).unwrap();
        assert!(c.contains("Since you last saw \"Build\", the user changed it"), "{c}");
        assert!(c.contains("- took out: \"Two rings.\"\n+ put in: \"Two rings and a cloak.\""), "{c}");
        // Told once.
        assert!(!context(&docs, ToolSet::Simple).unwrap().contains("the user changed it"));
        // After a restart (nothing in memory): the newest Chat version is
        // what it saw.
        with_seen(|s| s.clear());
        let c = context(&docs, ToolSet::Simple).unwrap();
        assert!(c.contains("+ put in: \"Two rings and a cloak.\""), "{c}");
        // A section the person typed in while the model wrote another isn't
        // counted as the model's: it saw its own writing only.
        let made = execute("create_document", &json!({ "title": "Two", "sections": ["A", "B"] }));
        let id2 = made["data"]["id"].as_u64().unwrap();
        after_call("create_document", &json!({}), &made);
        let mine = crate::notes::get(id2).unwrap().content.replace("# Two", "# Two (mine)");
        crate::notes::update(id2, None, Some(mine), None, None).unwrap();
        let args = json!({ "note_id": id2, "key": "A", "markdown": "Alpha." });
        let wrote = execute("write_section", &args);
        after_call("write_section", &args, &wrote);
        let c = context(&[DocRef { id: id2, title: "Two".into() }], ToolSet::Simple).unwrap();
        assert!(c.contains("+ put in: \"# Two (mine)\""), "{c}");
        assert!(c.contains("still to write: B"), "{c}");
    }

    #[test]
    fn a_turn_keeps_the_documents_it_made_or_changed() {
        let made = execute("create_document", &json!({ "title": "Doc", "sections": ["A"] }));
        let id = made["data"]["id"].as_u64().unwrap();
        let other = crate::notes::create("Shopping", "milk", "manual", "").id;
        let turn = Turn::new(&[]);
        assert_eq!(turn.noticed("create_document", id).map(|d| d.id), Some(id));
        assert_eq!(turn.noticed("write_section", id).map(|d| d.id), Some(id));
        // An edit to another note isn't a document of this chat.
        assert_eq!(turn.noticed("edit_note", other), None);
        // An edit to one of the chat's is.
        let turn2 = Turn::new(&[DocRef { id, title: "Doc".into() }]);
        assert!(turn2.noticed("edit_note", id).is_some());
        assert_eq!(turn.touched(), [DocRef { id, title: "Doc".into() }]);
    }

    #[test]
    fn something_to_keep_is_told_apart_from_a_quick_question() {
        for q in [
            "Make me a Divinity: Original Sin 2 lone wolf build for a ranger: gear, skills, talents and how to level it.",
            "Plan a three-day trip to Lisbon for two of us in May, day by day, with where to eat and a rough budget.",
            "Compare the Kindle Paperwhite, the Kobo Clara and the PocketBook Era for reading in bed: a table of the differences.",
            "Write me a step-by-step guide to repotting a fiddle-leaf fig, with what I'll need.",
            "Make me a packing list for a weekend of camping in the hills: clothes, kitchen, sleeping and extras.",
            "Can you put together a meal plan for next week?",
            "Give me a beginner's workout routine for three days a week",
            "What's a good lone wolf build for a ranger in a fantasy game?",
            // Asked as the maintainer did, a search first.
            "Can you look up a Pillars of Eternity build? I want a cipher, so find me the best one and put it in the chat.",
            "Find me a good beginner workout routine",
        ] {
            assert!(asks_for_document(q), "{q}");
        }
        for q in ["Find my keys in the notes", "Look up the weather in Leeds"] {
            assert!(!asks_for_document(q), "{q}");
        }
        for q in [
            "What time is it in Tokyo?",
            "How many tablespoons are in a cup?",
            "Give me three synonyms for happy.",
            "What's the plan for tomorrow?",
            "Is this a good build?",
            "Add a head torch to the extras.",
            "Make a note that I called Sam about the venue",
            "Write a reply to Sam saying I'm running late",
            "Give me a list of three synonyms for sad",
            "plan",
        ] {
            assert!(!asks_for_document(q), "{q}");
        }
    }

    #[test]
    fn a_turn_knows_the_document_it_left_unfinished() {
        let turn = Turn::new(&[]);
        let made = execute("create_document", &json!({ "title": "Comparison", "sections": ["Table", "Pick"] }));
        let id = made["data"]["id"].as_u64().unwrap();
        turn.noticed("create_document", id);
        execute("write_section", &json!({ "note_id": id, "key": "Table", "markdown": "| a | b |" }));
        assert_eq!(turn.unfinished(), Some((id, "Comparison".into(), vec!["Pick".into()])));
        let note = finish_note("Comparison", &["Pick".into()]);
        assert!(note.contains("still has sections to write: \"Pick\". Call write_section for each now"), "{note}");
        execute("write_section", &json!({ "note_id": id, "key": "Pick", "markdown": "B." }));
        assert_eq!(turn.unfinished(), None);
        // The same document asked for again this turn: the one there is.
        let again = turn.made_already("create_document", &json!({ "title": "comparison", "sections": ["Table", "Pick"] })).unwrap();
        assert_eq!((again["data"]["id"].as_u64(), again["data"]["already"].as_bool()), (Some(id), Some(true)));
        assert_eq!(again["displayText"], json!(""));
        assert!(turn.made_already("create_document", &json!({ "title": "Another", "sections": ["A"] })).is_none());
        assert!(turn.made_already("write_section", &json!({ "title": "Comparison" })).is_none());
        // One only changed this turn (not made) isn't the turn's to finish.
        let older = execute("create_document", &json!({ "title": "Older", "sections": ["A"] }));
        let older = older["data"]["id"].as_u64().unwrap();
        let turn = Turn::new(&[DocRef { id: older, title: "Older".into() }]);
        turn.noticed("edit_note", older);
        assert_eq!(turn.unfinished(), None);
    }

    #[test]
    fn sections_written_fold_into_one_chip() {
        let mut chips = vec!["Started \"Build\" (3 sections)".to_string()];
        push_chip(&mut chips, "Wrote \"Gear\" in \"Build\"".into());
        assert_eq!(chips.len(), 2);
        push_chip(&mut chips, "Wrote \"Skills\" in \"Build\"".into());
        push_chip(&mut chips, "Wrote \"Talents\" in \"Build\"".into());
        assert_eq!(chips, ["Started \"Build\" (3 sections)", "Wrote 3 sections in \"Build\""]);
        // Another document's, or another kind of chip: its own.
        push_chip(&mut chips, "Wrote \"A\" in \"Other\"".into());
        push_chip(&mut chips, "Searched the web for \"x\"".into());
        assert_eq!(chips.len(), 4);
    }

    #[test]
    fn a_section_streams_into_the_panel_as_its_arguments_come() {
        let whole = r#"{"note_id": 12, "key": "Gear", "markdown": "Two \"rings\".\nA staff é 😀"}"#;
        assert_eq!(live_section(whole), Some((12, "Gear".into(), "Two \"rings\".\nA staff é 😀".into())));
        // Part way: the text so far, a half escape left out.
        assert_eq!(live_section(r#"{"note_id": 12, "key": "Gear", "markdown": "Two \"ri"#), Some((12, "Gear".into(), "Two \"ri".into())));
        assert_eq!(live_section(r#"{"note_id": 12, "key": "Gear", "markdown": "Two\"#), Some((12, "Gear".into(), "Two".into())));
        assert_eq!(live_section(r#"{"note_id": 12, "key": "Gear", "markdown": "a\u00"#), Some((12, "Gear".into(), "a".into())));
        // Not yet: the id's digits or the key still coming.
        assert_eq!(live_section(r#"{"note_id": 1"#), None);
        assert_eq!(live_section(r#"{"note_id": 12, "key": "Ge"#), None);
        // An id as a string; the markdown not started yet.
        assert_eq!(live_section(r#"{"note_id": "7", "key": "Route", "#), Some((7, "Route".into(), String::new())));
        // A key whose words hold "markdown" doesn't fool it.
        assert_eq!(
            live_section(r#"{"note_id": 3, "key": "The \"markdown\": guide", "markdown": "Body"}"#),
            Some((3, "The \"markdown\": guide".into(), "Body".into()))
        );
        // A rewrite_section's heading streams the same way.
        assert_eq!(live_section(r#"{"note_id": 4, "heading": "Gear", "markdown": "Bo"#), Some((4, "Gear".into(), "Bo".into())));
    }

    #[test]
    fn a_summary_goes_under_the_title_as_a_paragraph() {
        let body = skeleton("Cipher build", "The Soul Blade is the pick.\n## Not a heading\n", &sections(&[("Gear", "")]));
        assert_eq!(body, "# Cipher build\n\nThe Soul Blade is the pick.\n\n<!-- yap:section Gear -->\n");
        let made = execute("create_document", &json!({ "title": "Build", "summary": "Two lines at most.", "sections": ["A"] }));
        let id = made["data"]["id"].as_u64().unwrap();
        assert!(crate::notes::get(id).unwrap().content.starts_with("# Build\n\nTwo lines at most.\n\n<!-- yap:section A -->"));
    }

    #[test]
    fn a_section_is_rewritten_only_while_it_is_what_the_model_saw() {
        let doc = "# Build\n\nThe pick.\n\n## Gear\n\n- Rings\n\n### Early\n\n- A staff\n\n## Skills\n\n- Shadow\n";
        // Rewritten whole, its ### part with it, the next section kept.
        let r = rewrite_section(doc, Some(doc), "gear", "| Level | Item |\n|---|---|\n| 1 | Staff |").unwrap();
        assert_eq!(r.content, "# Build\n\nThe pick.\n\n## Gear\n\n| Level | Item |\n|---|---|\n| 1 | Staff |\n\n## Skills\n\n- Shadow\n");
        assert_eq!((r.line, r.key.as_str(), r.added), (5, "Gear", false));
        // Its own heading renames it.
        let r = rewrite_section(doc, Some(doc), "Skills", "## Skills by level\n\n- 1: Shadow").unwrap();
        assert!(r.content.ends_with("## Skills by level\n\n- 1: Shadow\n"), "{}", r.content);
        assert_eq!(r.key, "Skills by level");
        // A heading it never had: a new section at the end.
        let r = rewrite_section(doc, Some(doc), "Level by level", "| Level | Pick |\n|---|---|").unwrap();
        assert!(r.added);
        assert!(r.content.ends_with("- Shadow\n\n## Level by level\n\n| Level | Pick |\n|---|---|\n"), "{}", r.content);
        // The person changed Gear since: refused, theirs stays.
        let theirs = doc.replace("- Rings", "- Rings, and my own cloak");
        assert_eq!(rewrite_section(&theirs, Some(doc), "Gear", "x"), Err(RewriteError::Changed));
        // …another section still rewrites.
        assert!(rewrite_section(&theirs, Some(doc), "Skills", "- Stealth").is_ok());
        // The person removed Skills: never brought back.
        let removed = doc.replace("## Skills\n\n- Shadow\n", "");
        assert_eq!(rewrite_section(&removed, Some(doc), "Skills", "x"), Err(RewriteError::Removed));
        // A section the person wrote themselves is theirs.
        let added = format!("{doc}\n## My notes\n\nMine.\n");
        assert_eq!(rewrite_section(&added, Some(doc), "My notes", "x"), Err(RewriteError::Changed));
        // No picture of it: read it first. Nothing to write: refused.
        assert_eq!(rewrite_section(doc, None, "Gear", "x"), Err(RewriteError::Unseen));
        assert_eq!(rewrite_section(doc, Some(doc), "Gear", " \n"), Err(RewriteError::Empty));
        // A section still to be written is filled.
        let outline = skeleton("Plan", "", &sections(&[("Days", "")]));
        let r = rewrite_section(&outline, None, "Days", "- Day 1").unwrap();
        assert!(pending_keys(&r.content).is_empty() && r.content.contains("## Days\n\n- Day 1"), "{}", r.content);
    }

    #[test]
    fn the_rewrite_tool_keeps_the_person_s_edits_until_the_model_reads_them() {
        let made = execute("create_document", &json!({ "title": "Cipher", "sections": ["Gear", "Powers"] }));
        let id = made["data"]["id"].as_u64().unwrap();
        after_call("create_document", &json!({}), &made);
        for (key, md) in [("Gear", "- Arquebus"), ("Powers", "- Mind Wave")] {
            let args = json!({ "note_id": id, "key": key, "markdown": md });
            let r = execute("write_section", &args);
            after_call("write_section", &args, &r);
        }
        let args = json!({ "note_id": id, "heading": "Powers", "markdown": "| Level | Power |\n|---|---|\n| 1 | Mind Wave |" });
        let r = execute("rewrite_section", &args);
        assert_eq!(r["success"], json!(true), "{r}");
        assert_eq!(r["displayText"], json!("Rewrote \"Powers\" in \"Cipher\""));
        assert_eq!(changed_note_of(&r), Some(id));
        after_call("rewrite_section", &args, &r);
        let v = versions::list(id).unwrap();
        assert_eq!(v[0].version.kind, versions::kind::CHAT);
        // The person changes Gear in the panel: a rewrite of it is refused…
        let now = crate::notes::get(id).unwrap().content.replace("- Arquebus", "- Arquebus, then a pistol");
        crate::notes::update(id, None, Some(now.clone()), None, None).unwrap();
        let args = json!({ "note_id": id, "heading": "Gear", "markdown": "- Sword" });
        let r = execute("rewrite_section", &args);
        assert_eq!(r["success"], json!(false));
        assert!(r["error"].as_str().unwrap().contains("read_note first"), "{r}");
        assert_eq!(crate::notes::get(id).unwrap().content, now);
        // …until the model has read it.
        let read = json!({ "success": true, "data": { "id": id, "field": "content", "from": 1, "more": false } });
        after_call("read_note", &json!({ "note_id": id }), &read);
        assert_eq!(execute("rewrite_section", &args)["success"], json!(true));
        // A new heading adds a section; a note Chat didn't make is refused.
        let r = execute("rewrite_section", &json!({ "note_id": id, "heading": "Tips", "markdown": "- Rest before bosses" }));
        assert_eq!(r["displayText"], json!("Added \"Tips\" to \"Cipher\""));
        let mine = crate::notes::create("Shopping", "## Gear\n\n- milk", "manual", "").id;
        let r = execute("rewrite_section", &json!({ "note_id": mine, "heading": "Gear", "markdown": "x" }));
        assert!(r["error"].as_str().unwrap().contains("isn't a document Chat made"), "{r}");
    }

    fn changed_note_of(result: &Value) -> Option<u64> {
        (result["success"] == json!(true) && result["data"]["changed"] == json!(true)).then(|| result["data"]["id"].as_u64()).flatten()
    }

    #[test]
    fn a_turn_rewrites_only_this_chat_s_documents() {
        let made = execute("create_document", &json!({ "title": "Doc", "sections": ["A"] }));
        let id = made["data"]["id"].as_u64().unwrap();
        let other = execute("create_document", &json!({ "title": "Another chat's", "sections": ["A"] }));
        let other = other["data"]["id"].as_u64().unwrap();
        let turn = Turn::new(&[DocRef { id, title: "Doc".into() }]);
        assert!(turn.refuses("rewrite_section", &json!({ "note_id": id, "heading": "A" })).is_none());
        let r = turn.refuses("rewrite_section", &json!({ "note_id": other, "heading": "A" })).unwrap();
        assert!(r["error"].as_str().unwrap().contains("isn't a document of this chat"), "{r}");
        assert!(turn.refuses("write_section", &json!({ "note_id": other })).is_none());
        // One this turn made is the chat's.
        let new = execute("create_document", &json!({ "title": "New", "sections": ["A"] }));
        let new = new["data"]["id"].as_u64().unwrap();
        turn.noticed("create_document", new);
        assert!(turn.refuses("rewrite_section", &json!({ "note_id": new, "heading": "A" })).is_none());
        assert!(turn.noticed("rewrite_section", id).is_some());
    }

    #[test]
    fn a_question_about_the_open_document_is_told_apart() {
        for q in [
            "Can you change it so it goes from level one to max level, step by step?",
            "Can you do a level one to max level and edit it so I can do a build step-by-step?",
            "Add a head torch to the extras.",
            "Go deeper on day two: hour by hour, with where to eat.",
            "Swap the staff for a wand",
            "Make it a two-handed build instead",
        ] {
            assert!(changes_document(q), "{q}");
        }
        for q in ["What time is it in Tokyo?", "Make me a packing list for a weekend in the hills", "Thanks!", "Let's do option A."] {
            assert!(!changes_document(q), "{q}");
        }
        for q in ["Let's do option A.", "OK, I'll go with option B please", "Option 2", "I'll take choice one", "Let's go with the second one"] {
            assert!(picks_option(q), "{q}");
        }
        for q in ["What are my options?", "Give me two options for a build", "an option and another", "Add option A's gear"] {
            assert!(!picks_option(q), "{q}");
        }
        // With a document open: a pick → a new one, a change → that one.
        let made = execute("create_document", &json!({ "title": "Summoner options", "sections": ["Option A", "Option B"] }));
        let id = made["data"]["id"].as_u64().unwrap();
        let docs = [DocRef { id, title: "Summoner options".into() }];
        let pick = steer("Let's do option A.", &docs, false, ToolSet::Simple).unwrap();
        assert!(pick.contains("new document for that pick") && pick.contains("\"Summoner options\" stays as it is"), "{pick}");
        let change = steer("Can you change it so it goes from level one to max level?", &docs, false, ToolSet::Simple).unwrap();
        assert!(change.contains(&format!("note_id {id}")) && change.contains("rewrite_section"), "{change}");
        assert_eq!(steer("Make me a packing list for a weekend of camping in the hills.", &docs, false, ToolSet::Simple).as_deref(), Some(ASKS_NOTE));
        assert_eq!(steer("Thanks!", &docs, false, ToolSet::Simple), None);
        // Without one: only an ask for something to keep.
        assert_eq!(steer("Let's do option A.", &[], false, ToolSet::Simple), None);
        assert_eq!(steer("Plan a three-day trip to Porto, day by day.", &[], false, ToolSet::Simple).as_deref(), Some(ASKS_NOTE));
    }

    /// A cloud model gets the same nudge as one short line in the
    /// question's context (Claude Haiku 5.5 answered "Could you find a mage
    /// build instead…?" in the chat and offered a plan, 30 Oct 2026).
    #[test]
    fn a_cloud_model_is_nudged_in_one_short_line() {
        let made = execute("create_document", &json!({ "title": "Two-handed fighter", "sections": ["Gear", "Skills"] }));
        let id = made["data"]["id"].as_u64().unwrap();
        let docs = [DocRef { id, title: "Two-handed fighter".into() }];
        let words = |s: &str| s.split_whitespace().count();
        // The maintainer's question: something to keep, now.
        let mage = steer("Could you find a mage build instead? Something with fire and summons.", &docs, true, ToolSet::Simple).unwrap();
        assert_eq!(mage, ASKS_BRIEF);
        let pick = steer("Let's do option A.", &docs, true, ToolSet::Simple).unwrap();
        assert!(pick.contains(&format!("note_id {id}")) && pick.contains("new document for that pick"), "{pick}");
        let change = steer("Add a head torch to the gear.", &docs, true, ToolSet::Simple).unwrap();
        assert!(change.contains(&format!("note_id {id}")) && change.contains("rewrite_section"), "{change}");
        // One line each, well under the local model's.
        for (short, long) in [
            (mage.as_str(), ASKS_NOTE.to_string()),
            (pick.as_str(), steer("Let's do option A.", &docs, false, ToolSet::Simple).unwrap()),
            (change.as_str(), steer("Add a head torch to the gear.", &docs, false, ToolSet::Simple).unwrap()),
        ] {
            assert!(!short.contains('\n') && words(short) <= 24 && short.len() * 3 < long.len() * 2, "{short}");
        }
        // Nothing to say stays nothing.
        assert_eq!(steer("What time is it in Tokyo?", &docs, true, ToolSet::Simple), None);
        assert_eq!(steer("Let's do option A.", &[], true, ToolSet::Simple), None);
    }

    /// A document is a Markdown note in Notes (Chat documents), and the
    /// model is told so where it meets the tools: a question about notes or a
    /// Markdown file gets a true answer, never "I can't write to your notes"
    /// (Claude Haiku 5.5, 30 Oct 2026).
    #[test]
    fn the_model_knows_a_document_is_a_note_in_notes() {
        assert!(INSTRUCTIONS.contains("A document is a Markdown note, saved in Notes under Chat documents"), "{INSTRUCTIONS}");
        let create = &definitions(ToolSet::Simple)[0]["function"];
        assert_eq!(create["name"], json!("create_document"));
        assert!(create["description"].as_str().unwrap().contains("shown beside the chat, saved in Notes"));
        let made = execute("create_document", &json!({ "title": "Camping kit", "sections": ["Clothes"] }));
        assert_eq!(made["data"]["saved"], json!(SAVED));
        assert!(SAVED.contains("Notes") && SAVED.contains(FOLDER) && SAVED.contains(".md"), "{SAVED}");
        let id = made["data"]["id"].as_u64().unwrap();
        let c = context(&[DocRef { id, title: "Camping kit".into() }], ToolSet::Simple).unwrap();
        assert!(c.starts_with("Documents in this chat, open beside it and saved in Notes (Chat documents) as Markdown notes"), "{c}");
        // The last section's result: the line for someone who asked for it
        // in their notes.
        let done = execute("write_section", &json!({ "note_id": id, "key": "Clothes", "markdown": "- A fleece" }));
        assert!(done["data"]["next"].as_str().unwrap().contains("saved in your Notes under Chat documents"), "{done}");
    }

    #[test]
    fn a_document_s_citations_link_to_their_pages() {
        let sources = vec![(1, "rpgbuilds.example".to_string(), "https://rpgbuilds.example/cipher".to_string()), (2, "loot.example".to_string(), "https://loot.example/gear".to_string())];
        assert_eq!(
            link_citations("The Soul Blade is the pick [1]. Gear [1][2].", &sources),
            "The Soul Blade is the pick ([rpgbuilds.example](https://rpgbuilds.example/cipher)). Gear ([rpgbuilds.example](https://rpgbuilds.example/cipher), [loot.example](https://loot.example/gear))."
        );
        assert_eq!(link_citations("| 1 | Staff [2, 1] |", &sources), "| 1 | Staff ([loot.example](https://loot.example/gear), [rpgbuilds.example](https://rpgbuilds.example/cipher)) |");
        // An unknown number, a link's own text, no sources: as written.
        assert_eq!(link_citations("Seen [3].", &sources), "Seen [3].");
        assert_eq!(link_citations("[1](https://a.example)", &sources), "[1](https://a.example)");
        assert_eq!(link_citations("Pick [1].", &[]), "Pick [1].");
    }

    #[test]
    fn the_voice_assistant_s_line_points_where_the_person_is_looking() {
        assert!(spoken_note(false).ends_with("that it's in Chat."));
        assert!(spoken_note(true).ends_with("that it's open on the right."));
        assert!(SPOKEN_NOTE.starts_with("This was asked out loud") && SPOKEN_IN_CHAT_NOTE.starts_with("This was asked out loud"));
    }

    #[test]
    fn a_section_with_rows_is_asked_for_as_a_table() {
        let body = skeleton("Cipher", "", &sections(&[("Levelling", "which power at each level"), ("Tips", "how to play it")]));
        assert_eq!(next_for(&body), "Now call write_section for \"Levelling\". Write it as a markdown table, a row each.");
        let f = fill_section(&body, "Levelling", "| 1 | Mind Wave |").unwrap();
        assert_eq!(next_for(&f.content), "Now call write_section for \"Tips\".");
        for (k, i) in [("Costs", ""), ("Day by day", ""), ("Attributes", "the stats"), ("The two", "compared side by side")] {
            assert!(!table_hint(k, i).is_empty(), "{k} {i}");
        }
        for (k, i) in [("Gear", "what to wear"), ("Day 1", "the old town"), ("Story", "")] {
            assert!(table_hint(k, i).is_empty(), "{k} {i}");
        }
    }

    #[test]
    fn rewritten_sections_fold_into_one_chip() {
        let mut chips = vec![];
        push_chip(&mut chips, "Rewrote \"Gear\" in \"Build\"".into());
        push_chip(&mut chips, "Rewrote \"Skills\" in \"Build\"".into());
        push_chip(&mut chips, "Wrote \"Tips\" in \"Build\"".into());
        assert_eq!(chips, ["Rewrote 2 sections in \"Build\"", "Wrote \"Tips\" in \"Build\""]);
    }

    /// The rich set: its tools in place of rewrite_section, its rule, and
    /// what the next question is told (the rev, how to change it, an
    /// earlier version brought back).
    #[test]
    fn the_rich_set_is_offered_and_told_how_to_change_a_document() {
        let names = |set| definitions(set).iter().map(|d| d["function"]["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(names(ToolSet::Simple), ["create_document", "write_section", "rewrite_section"]);
        assert_eq!(
            names(ToolSet::Rich),
            ["create_document", "write_section", "read_document", "edit_document", "document_history", "restore_document"]
        );
        assert_eq!(instructions(ToolSet::Simple), INSTRUCTIONS);
        assert!(instructions(ToolSet::Rich).contains("read_document, then edit_document"));
        assert!(!instructions(ToolSet::Rich).contains("rewrite_section"));
        // Both keep the outline first and where a document is kept.
        for set in [ToolSet::Simple, ToolSet::Rich] {
            assert!(instructions(set).contains("create_document (title, a one- or two-sentence summary"));
            assert!(instructions(set).contains("A document is a Markdown note, saved in Notes under Chat documents."));
        }
        let made = execute("create_document", &json!({ "title": "Build", "sections": [{ "key": "Gear", "intent": "kit" }] }));
        let id = made["data"]["id"].as_u64().unwrap();
        let docs = [DocRef { id, title: "Build".into() }];
        let rich = context(&docs, ToolSet::Rich).unwrap();
        assert!(rich.contains("read_document (since a rev"), "{rich}");
        assert!(rich.contains(&format!("\"Build\" (note_id {id}, rev ")), "{rich}");
        assert!(!context(&docs, ToolSet::Simple).unwrap().contains(", rev "));
        let change = steer("Add a head torch to the gear.", &docs, true, ToolSet::Rich).unwrap();
        assert!(change.contains("(read_document, then edit_document)"), "{change}");
        let back = steer("Put it back the way it was before that change.", &docs, true, ToolSet::Rich).unwrap();
        assert!(back.contains("document_history, then restore_document"), "{back}");
        // The simple set has no restore: a change, as before.
        assert!(steer("Put it back the way it was before that change.", &docs, true, ToolSet::Simple).unwrap().contains("rewrite_section"));
        for q in ["Undo that change", "Can you revert it?", "Go back to the previous version", "restore the original version"] {
            assert!(restores_version(q), "{q}");
        }
        for q in ["Add a head torch", "Put a torch in the extras", "What's the plan for Sunday?", "Move the costs back to the end"] {
            assert!(!restores_version(q), "{q}");
        }
    }

    /// A passage selected in the panel: the model is told the document, the
    /// words, and the blocks they're in — ids and hashes for the rich set,
    /// their markdown for the simple one; a model without tools the words.
    #[test]
    fn a_selected_passage_is_said_with_its_blocks() {
        let id = crate::notes::create("Kit", "# Kit\n\nA short list.\n\n## Kitchen\n\n- A stove\n- Two pans\n- A kettle\n\n## Extras\n\n- A map\n", "chat", "").id;
        let sel = Selection { note_id: id, text: "A stove\nTwo pans\nA kettle".into(), from_line: Some(6), to_line: Some(8) };
        let doc = blocks::Doc::of(&crate::notes::get(id).unwrap().content);
        let list = &doc.ids[3];
        let rich = selection_note(&sel, Some(ToolSet::Rich)).unwrap();
        assert!(rich.starts_with(&format!("The user selected this passage in \"Kit\" (note_id {id}) and asks about it: \"A stove Two pans A kettle\"")), "{rich}");
        assert!(rich.contains(&format!("It's in block {} (hash {}) of the document:\n- A stove\n- Two pans\n- A kettle\n", list.id, list.hash)), "{rich}");
        assert!(rich.contains("change only those blocks (edit_document"), "{rich}");
        let simple = selection_note(&sel, Some(ToolSet::Simple)).unwrap();
        assert!(simple.contains("In the document's markdown it's:\n- A stove\n- Two pans\n- A kettle\n"), "{simple}");
        assert!(simple.contains("edit_note with old_text copied exactly"), "{simple}");
        let plain = selection_note(&sel, None).unwrap();
        assert_eq!(plain, "The user selected this passage in the document \"Kit\" and asks about it: \"A stove Two pans A kettle\"");
        // No lines from the page: found by its words.
        let by_words = Selection { from_line: None, to_line: None, ..sel.clone() };
        assert!(selection_note(&by_words, Some(ToolSet::Rich)).unwrap().contains(&list.id));
        // Kept with the question: its note and words, bounded.
        assert_eq!(sel.quote(), Some(Quote { note_id: id, text: "A stove\nTwo pans\nA kettle".into() }));
        assert_eq!(Selection { text: "  ".into(), ..sel.clone() }.quote(), None);
        assert!(selection_note(&Selection { note_id: 999_999, ..sel }, Some(ToolSet::Rich)).is_none());
    }

    /// The rich tools that change a document run only on this chat's.
    #[test]
    fn a_turn_edits_only_this_chat_s_documents_with_the_rich_tools_too() {
        let turn = Turn::new(&[DocRef { id: 7, title: "Ours".into() }]);
        assert!(turn.refuses("edit_document", &json!({ "note_id": 7 })).is_none());
        let r = turn.refuses("edit_document", &json!({ "note_id": 8 })).unwrap();
        assert!(r["error"].as_str().unwrap().contains("edit_document only changes those"), "{r}");
        assert!(turn.refuses("restore_document", &json!({ "note_id": 8 })).is_some());
        // Reading isn't changing.
        assert!(turn.refuses("read_document", &json!({ "note_id": 8 })).is_none());
        assert!(turn.refuses("document_history", &json!({ "note_id": 8 })).is_none());
    }
}

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
//! The log has ids, keys' counts and lengths: never a document's words.

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
/// instructions (`tools::TOOL_INSTRUCTIONS`): when to make one, the outline
/// first, a line or two in the chat, and later questions changing it. Kept
/// short: every step of a tool turn sends it.
pub const INSTRUCTIONS: &str = "For an answer to keep or come back to (a build, a plan, a guide, a comparison, a long table), make a document: create_document with the title and all its sections first, then write_section for each, in order. In the chat say only a line or two (\"It's open on the right.\"), never the content. A question or change about a document from this chat: change it with edit_note (read_note first).";

/// Added to a question asked out loud (the Voice Assistant) when the
/// model calls tools: no document unless one is asked for, and then a
/// sentence saying where it is (the person is in another app).
pub const SPOKEN_NOTE: &str = "This was asked out loud: answer in speech. Make a document only when they ask for one, or for a long build, plan or guide; then say in one short sentence that it's in Chat.";

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

/// A new document's body: its title, then each section's placeholder. Pure.
pub fn skeleton(title: &str, sections: &[(String, String)]) -> String {
    let mut out = format!("# {}\n", title.trim());
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
}

impl Turn {
    pub fn new(known: &[DocRef]) -> Turn {
        Turn { known: Mutex::new(known.iter().map(|d| d.id).collect()), touched: Mutex::new(Vec::new()) }
    }

    /// Note `id` changed by tool `name`: a document of this chat? (Made by
    /// `create_document`, filled by `write_section`, or one the
    /// conversation has, changed by `edit_note` / `update_note`.) Kept for
    /// the answer, once, with its title now.
    pub fn noticed(&self, name: &str, id: u64) -> Option<DocRef> {
        let mut known = self.known.lock().unwrap_or_else(|p| p.into_inner());
        let ours = match name {
            "create_document" | "write_section" => true,
            "edit_note" | "update_note" => known.contains(&id),
            _ => false,
        };
        if !ours {
            return None;
        }
        if !known.contains(&id) {
            known.push(id);
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
pub fn context(docs: &[DocRef]) -> Option<String> {
    let mut lines = vec!["Documents in this chat, open beside it (change them with edit_note; write_section only fills a section still to be written):".to_string()];
    let mut changed = Vec::new();
    for d in docs.iter().take(3) {
        let Some(note) = crate::notes::get(d.id) else { continue };
        let pending = pending_keys(&note.content);
        let title = if note.title.trim().is_empty() { d.title.clone() } else { note.title.clone() };
        lines.push(if pending.is_empty() {
            format!("- \"{title}\" (note_id {})", d.id)
        } else {
            format!("- \"{title}\" (note_id {}; still to write: {})", d.id, pending.join(", "))
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

/// The two tools' schemas (offered with the note tools: `notes::definitions`).
pub fn definitions() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "create_document",
                "description": "Start a document shown beside the chat: its title and sections in order. Then write_section each.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
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
                "description": "Write one section of a document, in markdown (lists, tables), under its heading. Once each; edit_note changes it later.",
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

/// Run `create_document` or `write_section` (`notes::execute` hands them
/// here).
pub fn execute(name: &str, args: &Value) -> Value {
    match name {
        "create_document" => create(args),
        "write_section" => write(args),
        other => tool_result(false, Value::Null, format!("Unknown tool: {other}")),
    }
}

/// What a result tells the model to do next, by what's left.
fn next_step(left: &[String]) -> String {
    match left.first() {
        Some(k) => format!("Now call write_section for \"{k}\"."),
        None => "Every section is written. Tell the user in a line or two that it's open beside the chat; don't repeat it there.".to_string(),
    }
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
    let body = skeleton(&title, &sections);
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
    tracing::info!(note_id = note.id, sections = keys.len(), "Chat: a document started");
    tool_result(
        true,
        json!({ "id": note.id, "title": title, "folder": folder, "sections": keys, "next": next_step(&keys) }),
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
                Ok((Ok((filled.line, filled.key, left, note.title.clone())), true))
            }
            Err(e) => Ok((Err((e, pending_keys(&note.content), note.title.clone())), false)),
        })
    });
    match outcome {
        Err(e) => tool_result(false, Value::Null, e),
        Ok(Ok((line, key, left, title))) => {
            tracing::info!(note_id = id, left = left.len(), chars = markdown.chars().count(), "Chat: a document's section written");
            tool_result(
                true,
                json!({ "id": id, "key": key, "changed": true, "line": line, "left": left, "next": next_step(&left) }),
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

/// A turn's chips with a section's written chip folded into the one before
/// it for the same document ("Wrote 3 sections in "Build""): a document's
/// sections are one thing done, not a column of chips. Pure.
pub fn push_chip(chips: &mut Vec<String>, chip: String) {
    static ONE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r#"^Wrote "[^"]*" in (".*")$"#).expect("static regex"));
    static MANY: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r#"^Wrote (\d+) sections in (".*")$"#).expect("static regex"));
    if let (Some(new), Some(last)) = (ONE.captures(&chip), chips.last()) {
        let doc = new[1].to_string();
        let count = match (ONE.captures(last), MANY.captures(last)) {
            (Some(c), _) if c[1] == *doc => Some(2),
            (_, Some(c)) if c[2] == *doc => c[1].parse::<usize>().ok().map(|n| n + 1),
            _ => None,
        };
        if let Some(n) = count {
            if let Some(last) = chips.last_mut() {
                *last = format!("Wrote {n} sections in {doc}");
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

/// A `write_section` call's arguments so far, as the panel shows them:
/// `(note_id, key, the section so far)`, once the note and the key are
/// whole. Pure.
pub fn live_section(args_so_far: &str) -> Option<(u64, String, String)> {
    let id = partial_number(args_so_far, "note_id").or_else(|| partial_number(args_so_far, "id"))?;
    // The key whole: its closing quote has come.
    let key = partial_string(args_so_far, "key")?;
    let key_done = {
        let needle = "\"key\"";
        args_so_far.find(needle).is_some_and(|at| {
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
        let body = skeleton("Lone wolf build", &sections(&[("Gear", "what to wear"), ("Skills", "which and when")]));
        assert_eq!(
            body,
            "# Lone wolf build\n\n<!-- yap:section Gear | what to wear -->\n\n<!-- yap:section Skills | which and when -->\n"
        );
        assert_eq!(pending_keys(&body), ["Gear", "Skills"]);
        assert_eq!(pending_keys(&body).len(), 2);
    }

    #[test]
    fn a_section_fills_its_placeholder_and_nothing_else() {
        let body = skeleton("Trip", &sections(&[("Days", "day by day"), ("Costs", "what it costs")]));
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
        let body = skeleton("Plan", &sections(&[("Gear", "kit"), ("Route", "the way")]));
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
        let base = skeleton("Plan", &sections(&[("Gear", "kit"), ("Route", "the way")]));
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
        let c = context(&docs).unwrap();
        assert!(c.contains("\"Build\" (note_id"), "{c}");
        assert!(!c.contains("the user changed it"), "{c}");
        // The person edits: the next question says what.
        let now = crate::notes::get(id).unwrap().content.replace("Two rings.", "Two rings and a cloak.");
        crate::notes::update(id, None, Some(now), None, None).unwrap();
        let c = context(&docs).unwrap();
        assert!(c.contains("Since you last saw \"Build\", the user changed it"), "{c}");
        assert!(c.contains("- took out: \"Two rings.\"\n+ put in: \"Two rings and a cloak.\""), "{c}");
        // Told once.
        assert!(!context(&docs).unwrap().contains("the user changed it"));
        // After a restart (nothing in memory): the newest Chat version is
        // what it saw.
        with_seen(|s| s.clear());
        let c = context(&docs).unwrap();
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
        let c = context(&[DocRef { id: id2, title: "Two".into() }]).unwrap();
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
    }
}

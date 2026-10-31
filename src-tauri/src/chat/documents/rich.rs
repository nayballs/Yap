//! **The rich document tools** (2026-10-31), for the models that handle
//! tools well — Claude, the GPT-5 family, Gemini, the large open models —
//! after the contract of Claude's own document tools, simplified for Yap (a
//! document is a note's markdown):
//!
//! - `read_document(note_id, since?, offset?, limit?)`: the document as
//!   blocks ([`super::blocks`]), each with an `id` and a `hash`, and its
//!   `rev` (the number of the version it is, `notes/versions.rs`); `since`
//!   (an earlier rev) gives only what changed after it, the unchanged runs
//!   folded; a long one comes a page at a time (`offset`, `limit`, within
//!   `note_text::PAGE_TOKENS`, as `read_note`).
//! - `edit_document(note_id, ops)`: insert, replace, delete and move by
//!   block, in order, all or none, a version of its own (Chat's: the
//!   panel's Undo); a block the person changed since the model's last look
//!   is refused with what it says now — their words always win.
//! - `document_history(note_id)` and `restore_document(note_id, version)`:
//!   its versions, how each came, and one brought back — itself a version.
//!
//! Beside them stay `create_document` and `write_section` (the outline
//! first, then its sections, `documents.rs`): the rich set changes how a
//! document is changed, not how it's made. `rewrite_section` and the
//! advice to use `edit_note` are the simple set's ([`ToolSet::Simple`]: the
//! on-device model and small ones).
//!
//! The log has ids, counts and lengths: never a document's words.

use std::collections::HashMap;


use serde_json::{json, Value};

use super::blocks::{self, Doc, Why};
use super::{arg_u64, quoted, saw, seen, tool_error, tool_result};
use crate::notes::versions;

/// Which document tools a model gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolSet {
    /// `create_document`, `write_section`, `rewrite_section`, and the note
    /// tools' `edit_note` for a few words (the on-device model, small ones).
    #[default]
    Simple,
    /// `create_document`, `write_section`, and `read_document`,
    /// `edit_document`, `document_history`, `restore_document`.
    Rich,
}

impl ToolSet {
    /// **The set for Chat's model** (`provider`, `model` as `turn.rs` has
    /// them): [`handles_rich`], unless a model evaluation says otherwise
    /// (`--doc-tools rich|simple`).
    pub fn for_model(provider: &str, model: &str) -> ToolSet {
        if let Some(rich) = crate::ai::eval::doc_tools_forced() {
            return if rich { ToolSet::Rich } else { ToolSet::Simple };
        }
        if handles_rich(provider, model) {
            ToolSet::Rich
        } else {
            ToolSet::Simple
        }
    }

    /// "rich" | "simple" (the log, an evaluation's record).
    pub fn name(self) -> &'static str {
        match self {
            ToolSet::Simple => "simple",
            ToolSet::Rich => "rich",
        }
    }
}

/// Families that handle a tool contract of ids and hashes well, matched in
/// the lower-cased model id (any provider: a Claude through OpenRouter is a
/// Claude): Claude, GPT-5 and GPT-4.1 (not their nano), o3 / o4, Gemini 2.5
/// and 3 (not Flash-Lite), Grok 3 and 4, Kimi K2, DeepSeek V3 and R1's
/// later releases, GLM 4.5 and 4.6, Qwen3's large and coder models, Mistral
/// Large, gpt-oss-120b.
const RICH_FAMILIES: &[&str] = &[
    "claude",
    "gpt-5",
    "gpt-4.1",
    "gemini-2.5",
    "gemini-3",
    "grok-3",
    "grok-4",
    "kimi-k2",
    "deepseek-v3",
    "deepseek-chat",
    "deepseek-r1-0528",
    "glm-4.5",
    "glm-4.6",
    "qwen3-coder",
    "qwen3-max",
    "mistral-large",
    "gpt-oss-120b",
];

/// Parts of an id that make a family's model a small one.
const SMALL_MARKS: &[&str] = &["nano", "lite", "mini-instruct", "-tiny", "haiku-3"];

/// The least a model's id must say it has (in billions) to get the rich set
/// by size alone (Llama 3.3 70B, Qwen3 235B; never a 32B).
const RICH_MIN_PARAMS_B: f32 = 70.0;

/// **Does a model handle the rich tools?** Never one on this PC (the
/// on-device model, a local server): the evaluations decide that, and the
/// on-device Qwen3.5-4B keeps the simple set (docs/model-evals.md). A cloud
/// or self-hosted model by its family ([`RICH_FAMILIES`], not a small one of
/// it) or by the size its id names (≥ 70B). Pure.
pub fn handles_rich(provider: &str, model: &str) -> bool {
    if provider == crate::ai::local_llm::PROVIDER_ONDEVICE || provider == "local" {
        return false;
    }
    let id = model.to_ascii_lowercase();
    if SMALL_MARKS.iter().any(|m| id.contains(m)) {
        return false;
    }
    static REASONING: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"(^|[/:_-])o[34](-mini)?($|-)").expect("static regex"));
    RICH_FAMILIES.iter().any(|f| id.contains(f)) || REASONING.is_match(&id) || crate::chat::tools::estimate_model_size_b(model) >= RICH_MIN_PARAMS_B
}

/// What Chat's system prompt says about documents with the rich set (in
/// place of `documents::INSTRUCTIONS`): when to make one, outline first, how
/// it reads — as the simple set's — and how to change one: read it, then
/// edit its blocks by id and hash; a refusal means the person changed that
/// block. Kept short: every step of a tool turn sends it.
pub const INSTRUCTIONS: &str = "Something to keep (a build, a plan, a guide, a comparison, a long table) goes straight into a document, never the chat, never asked first: create_document (title, a one- or two-sentence summary, every section: one per part, usually 2 to 6), then write_section each, in order. Short bullets, a markdown table for anything with rows, **bold** lead-ins; a game build: race and origin, character creation, skills, a level-by-level table, gear, tips. In the chat say only one short line (\"It's open on the right.\"): no summary, never the content. A document is a Markdown note, saved in Notes under Chat documents. To change a document from this chat (a change, more depth, a selected passage): read_document, then edit_document with ops on its block ids, each replace, delete or move with the if_hash you read; a table row added or changed is a replace of the whole table. A refused op means the user changed that block: their words stay, build on them. document_history and restore_document bring back an earlier version when asked. An option picked: a new document. Never create_note for these.";

/// The tools this file runs.
pub const TOOL_NAMES: &[&str] = &["read_document", "edit_document", "document_history", "restore_document"];

/// Their schemas.
pub fn definitions() -> Vec<Value> {
    vec![
        json!({
            "type": "function",
            "function": {
                "name": "read_document",
                "description": "Read a document of this chat as blocks (heading, paragraph, whole list, whole table, code), each with an id and a hash, and its rev. since: an earlier rev, for only what changed after it. Long ones come in pages (offset = next_offset).",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "note_id": { "type": "number" },
                        "since": { "type": "number", "description": "A rev from an earlier read" },
                        "offset": { "type": "number", "description": "The first block, 1 = the start (default 1)" },
                        "limit": { "type": "number", "description": "How many blocks (default 80)" }
                    },
                    "required": ["note_id"],
                    "additionalProperties": false
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "edit_document",
                "description": "Change a document of this chat by its blocks, in order, all or none: insert (markdown, after or before a block id, or at the end), replace (id, if_hash, markdown), delete (id, if_hash), move (ids, if_hashes, after or before an id). One version; Undo takes it back. Refused if the user changed a block since you read it.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "note_id": { "type": "number" },
                        "ops": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "op": { "type": "string", "enum": ["insert", "replace", "delete", "move"] },
                                    "id": { "type": "string" },
                                    "if_hash": { "type": "string" },
                                    "ids": { "type": "array", "items": { "type": "string" } },
                                    "if_hashes": { "type": "array", "items": { "type": "string" } },
                                    "after": { "type": "string", "description": "A block id" },
                                    "before": { "type": "string", "description": "A block id" },
                                    "markdown": { "type": "string" }
                                },
                                "required": ["op"]
                            }
                        }
                    },
                    "required": ["note_id", "ops"],
                    "additionalProperties": false
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "document_history",
                "description": "A document's versions, newest first: each one's number, when, and who made it (Chat, the user, a restore).",
                "parameters": {
                    "type": "object",
                    "properties": { "note_id": { "type": "number" } },
                    "required": ["note_id"],
                    "additionalProperties": false
                }
            }
        }),
        json!({
            "type": "function",
            "function": {
                "name": "restore_document",
                "description": "Bring back an earlier version of a document of this chat (from document_history), when the user asks. The restore is a version itself.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "note_id": { "type": "number" },
                        "version": { "type": "number" }
                    },
                    "required": ["note_id", "version"],
                    "additionalProperties": false
                }
            }
        }),
    ]
}

/// Run one of [`TOOL_NAMES`].
pub fn execute(name: &str, args: &Value) -> Value {
    match name {
        "read_document" => read(args),
        "edit_document" => edit(args),
        "document_history" => history(args),
        "restore_document" => restore(args),
        other => tool_result(false, Value::Null, format!("Unknown tool: {other}")),
    }
}

// ---- what Chat read at each rev --------------------------------------------------------

/// The text Chat read at each rev of a document (note id, rev → text): what
/// `since` compares with, even when the version was folded into later (a
/// change Yap didn't see, a version past `versions::KEEP`). The newest few
/// per document, in memory.
#[cfg(not(test))]
static READ: std::sync::Mutex<Option<HashMap<(u64, u64), String>>> = std::sync::Mutex::new(None);

#[cfg(not(test))]
fn with_read<R>(f: impl FnOnce(&mut HashMap<(u64, u64), String>) -> R) -> R {
    let mut g = READ.lock().unwrap_or_else(|p| p.into_inner());
    f(g.get_or_insert_with(HashMap::new))
}

#[cfg(test)]
thread_local! {
    static READ_TEST: std::cell::RefCell<HashMap<(u64, u64), String>> = std::cell::RefCell::new(HashMap::new());
}

#[cfg(test)]
fn with_read<R>(f: impl FnOnce(&mut HashMap<(u64, u64), String>) -> R) -> R {
    READ_TEST.with(|s| f(&mut s.borrow_mut()))
}

/// Revs kept per document.
const READ_KEPT: usize = 6;

fn remember_read(id: u64, rev: u64, text: &str) {
    with_read(|m| {
        m.insert((id, rev), text.to_string());
        let mut revs: Vec<u64> = m.keys().filter(|(n, _)| *n == id).map(|(_, r)| *r).collect();
        if revs.len() > READ_KEPT {
            revs.sort_unstable();
            for r in &revs[..revs.len() - READ_KEPT] {
                m.remove(&(id, *r));
            }
        }
    });
}

/// The document's text at rev `rev`: as Chat read it, else that version's.
fn text_at(id: u64, rev: u64) -> Option<String> {
    with_read(|m| m.get(&(id, rev)).cloned()).or_else(|| versions::notes_version(id, rev).map(|v| v.text))
}

/// The rev a document is at now (its newest version of its notes; 0: none),
/// sealed so the person's typing after it is a version of its own
/// (`versions::seal`), and the text read remembered at it.
pub fn rev_seen(id: u64, content: &str) -> u64 {
    let rev = versions::seal(id);
    remember_read(id, rev, content);
    rev
}

// ---- the blocks Chat's own edits replaced -----------------------------------------------

/// Per document, the ids Chat's own edits replaced or deleted, and what
/// stands for each now (none: deleted): an op naming one again (the same
/// edit sent twice — the 4B did, 31 Oct 2026) is told it's done already,
/// never "there's no such block". The newest few, in memory.
type Retired = HashMap<u64, Vec<(String, Vec<String>)>>;

#[cfg(not(test))]
static RETIRED: std::sync::Mutex<Option<Retired>> = std::sync::Mutex::new(None);

#[cfg(not(test))]
fn with_retired<R>(f: impl FnOnce(&mut Retired) -> R) -> R {
    let mut g = RETIRED.lock().unwrap_or_else(|p| p.into_inner());
    f(g.get_or_insert_with(HashMap::new))
}

#[cfg(test)]
thread_local! {
    static RETIRED_TEST: std::cell::RefCell<Retired> = std::cell::RefCell::new(HashMap::new());
}

#[cfg(test)]
fn with_retired<R>(f: impl FnOnce(&mut Retired) -> R) -> R {
    RETIRED_TEST.with(|s| f(&mut s.borrow_mut()))
}

/// Ids kept per document.
const RETIRED_KEPT: usize = 40;

fn retire(id: u64, old: &str, now: Vec<String>) {
    with_retired(|m| {
        let list = m.entry(id).or_default();
        list.retain(|(o, _)| o != old);
        list.push((old.to_string(), now));
        if list.len() > RETIRED_KEPT {
            let extra = list.len() - RETIRED_KEPT;
            list.drain(..extra);
        }
    });
}

/// What Chat's own edit put in place of block `old` of document `id`, if it
/// replaced or deleted it.
fn retired(id: u64, old: &str) -> Option<Vec<String>> {
    with_retired(|m| m.get(&id).and_then(|l| l.iter().rev().find(|(o, _)| o == old.trim()).map(|(_, n)| n.clone())))
}

// ---- a document of Chat's -------------------------------------------------------------

/// Note `note_id` of `args`, when it's a document Chat made; else the error
/// result (with `chip`).
fn document(args: &Value, chip: &str) -> Result<crate::notes::Note, Value> {
    let Some(id) = arg_u64(args, &["note_id", "id"]) else {
        return Err(tool_error("note_id is required: the document's id.".to_string(), Value::Null, chip.to_string()));
    };
    match crate::notes::get(id) {
        Some(n) if n.source == "chat" => Ok(n),
        Some(n) => Err(tool_error(
            format!("Note {id} isn't a document Chat made: these tools only work on documents (read_note and edit_note work on any note)."),
            json!({ "id": n.id }),
            chip.to_string(),
        )),
        None => Err(tool_error(format!("No note {id}."), Value::Null, chip.to_string())),
    }
}

/// The blocks shown at most per page (and the default).
const PAGE_BLOCKS: usize = 80;

// ---- read_document --------------------------------------------------------------------

fn read(args: &Value) -> Value {
    const CHIP: &str = "Couldn't read a document";
    let note = match document(args, CHIP) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let t = quoted(&note.title);
    let doc = Doc::of(&note.content);
    let rev = rev_seen(note.id, &note.content);
    // Only what changed since an earlier read.
    if let Some(since) = arg_u64(args, &["since", "since_rev"]) {
        match text_at(note.id, since) {
            Some(old) => return since_read(&note, &doc, rev, since, &old),
            None => {
                tracing::info!(note_id = note.id, since, "Chat: read_document since a rev not kept; the whole document instead");
            }
        }
    }
    let total = doc.blocks.len();
    let offset = arg_u64(args, &["offset"]).unwrap_or(1).max(1) as usize;
    if total > 0 && offset > total {
        return tool_error(
            format!("offset {offset} is past the document's last block ({total}). Read from offset 1."),
            json!({ "id": note.id, "rev": rev, "total_blocks": total }),
            format!("Couldn't read {t}"),
        );
    }
    let limit = arg_u64(args, &["limit"]).map_or(PAGE_BLOCKS, |l| (l as usize).clamp(1, PAGE_BLOCKS));
    let budget = crate::chat::note_text::PAGE_TOKENS;
    let est = crate::meetings::meeting_summary::estimate_tokens;
    let mut shown: Vec<Value> = Vec::new();
    let mut used = 0usize;
    let mut cut = false;
    let mut i = offset - 1;
    while i < total && shown.len() < limit {
        let mut v = blocks::view(&doc, i);
        let cost = est(&v.to_string());
        if !shown.is_empty() && used + cost > budget {
            break;
        }
        // One block longer than a page alone (a huge table): what fits, said so.
        if shown.is_empty() && cost > budget * 2 {
            if let Some(text) = v["text"].as_str() {
                let keep: String = text.chars().take(budget * 7).collect();
                v["text"] = json!(keep);
                v["cut"] = json!(true);
                cut = true;
            }
        }
        used += cost;
        shown.push(v);
        i += 1;
    }
    let more = i < total;
    // A whole read: the model has seen the document as it is.
    if offset == 1 && !more {
        seen(note.id, &note.content);
    }
    let mut data = json!({
        "id": note.id,
        "title": note.title,
        "rev": rev,
        "total_blocks": total,
        "from": if total == 0 { 0 } else { offset },
        "to": offset - 1 + shown.len(),
        "blocks": shown,
    });
    if more {
        data["more"] = json!(true);
        data["next_offset"] = json!(i + 1);
    }
    if cut {
        data["hint"] = json!("A block is longer than a page and was cut: read_note shows its lines; replace it only with the whole of it.");
    }
    if total == 0 {
        data["empty"] = json!(true);
    }
    tracing::info!(note_id = note.id, rev, blocks = shown.len(), total, more, "Chat: read_document");
    let display = if more || offset > 1 {
        format!("Read the document {t} (blocks {}\u{2013}{} of {total})", offset, offset - 1 + shown.len())
    } else {
        format!("Read the document {t}")
    };
    tool_result(true, data, display)
}

/// Unchanged runs shown as one line each, at least this long.
const FOLD_FROM: usize = 1;

/// `read_document(since)`: what changed from `old` (the text at rev
/// `since`) to now, block by block — unchanged runs folded to a count and
/// their first and last ids, each change with what was there and what is.
fn since_read(note: &crate::notes::Note, doc: &Doc, rev: u64, since: u64, old: &str) -> Value {
    let t = quoted(&note.title);
    let before = Doc::of(old);
    let changes = blocks::changes(&before, doc);
    // Seen it all now: what it knew, and what changed since.
    seen(note.id, &note.content);
    if changes.is_empty() {
        tracing::info!(note_id = note.id, rev, since, "Chat: read_document since: unchanged");
        return tool_result(
            true,
            json!({ "id": note.id, "title": note.title, "rev": rev, "since": since, "unchanged": true, "total_blocks": doc.blocks.len() }),
            format!("Read the document {t}: unchanged"),
        );
    }
    let mut out: Vec<Value> = Vec::new();
    let mut at = 0usize; // the next block of `doc` not yet said
    let fold = |out: &mut Vec<Value>, from: usize, to: usize| {
        if to > from && to - from >= FOLD_FROM {
            out.push(json!({ "unchanged": to - from, "first": doc.ids[from].id, "last": doc.ids[to - 1].id }));
        }
    };
    for c in &changes {
        fold(&mut out, at, c.new.0);
        let was: Vec<Value> =
            (c.old.0..c.old.1).map(|i| json!({ "id": before.ids[i].id, "text": blocks::clip(&before.blocks[i].text, 160) })).collect();
        let now: Vec<Value> = (c.new.0..c.new.1).map(|i| blocks::view(doc, i)).collect();
        let kind = match (was.is_empty(), now.is_empty()) {
            (true, _) => "added",
            (_, true) => "removed",
            _ => "replaced",
        };
        let mut v = json!({ "change": kind });
        if !was.is_empty() {
            v["was"] = json!(was);
        }
        if !now.is_empty() {
            v["now"] = json!(now);
        }
        out.push(v);
        at = c.new.1;
    }
    fold(&mut out, at, doc.blocks.len());
    tracing::info!(note_id = note.id, rev, since, changes = changes.len(), "Chat: read_document since");
    tool_result(
        true,
        json!({ "id": note.id, "title": note.title, "rev": rev, "since": since, "total_blocks": doc.blocks.len(), "blocks": out }),
        format!("Read what changed in {t}"),
    )
}

// ---- edit_document --------------------------------------------------------------------

/// A refused op as the model reads it, and the chip.
fn refusal(note: &crate::notes::Note, doc: &Doc, r: &blocks::Refused, ops: &[blocks::Op]) -> Value {
    let t = quoted(&note.title);
    let n = r.op + 1;
    let name = ops.get(r.op).map_or("op", |o| o.name());
    let nothing = "Nothing was changed (edit_document is all or none).";
    let (error, chip, now): (String, String, Vec<Value>) = match &r.why {
        Why::Changed { id, now } if now.is_empty() => (
            format!("Op {n} ({name}): the user removed block {id} since you read it. {nothing} Their change stays: don't add it back. read_document shows the document now."),
            format!("Didn't change {t}: the user removed that part"),
            Vec::new(),
        ),
        Why::Changed { id, now } => (
            format!(
                "Op {n} ({name}): the user changed block {id} since you read it. {nothing} Their words stay: here it is now (\"now\"); if the change still applies, make it on this, keeping their words."
            ),
            format!("Didn't change {t}: the user changed that part"),
            now.iter().map(|i| blocks::view(doc, *i)).collect(),
        ),
        Why::Hash { id, at } => (
            format!("Op {n} ({name}): block {id}'s hash is {}, not the if_hash given. {nothing} Here it is now (\"now\"): use its hash.", doc.ids[*at].hash),
            format!("Didn't change {t}: a block didn't match"),
            vec![blocks::view(doc, *at)],
        ),
        // Your own edit changed it already (the same edit sent twice).
        Why::Unknown(id) if retired(note.id, id).is_some() => {
            let now: Vec<usize> = retired(note.id, id).unwrap_or_default().iter().filter_map(|x| doc.find(x)).collect();
            (
                format!(
                    "Op {n} ({name}): your own earlier edit already {} block {id}, so it isn't there any more. {nothing} Don't make the same change again; read_document shows the document now.",
                    if now.is_empty() { "removed" } else { "replaced" }
                ),
                format!("Didn't change {t}: done already"),
                now.iter().map(|i| blocks::view(doc, *i)).collect(),
            )
        }
        Why::Unknown(id) => (
            format!("Op {n} ({name}): there's no block {id} in the document. {nothing} read_document gives the blocks' ids."),
            format!("Didn't change {t}: no such part"),
            Vec::new(),
        ),
        Why::Earlier(id) => (
            format!("Op {n} ({name}): block {id} was replaced or deleted by an earlier op of this edit. {nothing}"),
            format!("Couldn't change {t}"),
            Vec::new(),
        ),
        Why::IntoItself(id) => (
            format!("Op {n} (move): {id} is one of the blocks being moved; move them after or before another block. {nothing}"),
            format!("Couldn't change {t}"),
            Vec::new(),
        ),
        Why::Missing(what) => (
            match *what {
                "if_hash" => format!("Op {n} ({name}) needs if_hash: the block's hash from read_document. {nothing}"),
                "markdown" => format!("Op {n} ({name}) needs markdown: the text to put in. {nothing}"),
                other => format!("Op {n} ({name}) needs {other}. {nothing}"),
            },
            format!("Couldn't change {t}"),
            Vec::new(),
        ),
    };
    let mut data = json!({ "id": note.id, "op": n });
    if !now.is_empty() {
        data["now"] = json!(now);
    }
    tool_error(error, data, chip)
}

/// The chip for an edit that landed: what it did, in a few words.
fn edit_chip(title: &str, ops: &[blocks::Op], made: &[Vec<usize>], doc: &Doc) -> String {
    let t = quoted(title);
    let tables = made.iter().flatten().filter(|i| doc.blocks.get(**i).is_some_and(|b| b.kind == blocks::Kind::Table)).count();
    match ops {
        [blocks::Op::Replace { .. }] if tables == 1 => format!("Changed a table in {t}"),
        [one] => match one {
            blocks::Op::Insert { .. } => format!("Added to {t}"),
            blocks::Op::Replace { .. } => format!("Changed a part of {t}"),
            blocks::Op::Delete { .. } => format!("Took a part out of {t}"),
            blocks::Op::Move { .. } => format!("Moved a part of {t}"),
        },
        many => format!("Made {} changes in {t}", many.len()),
    }
}

fn edit(args: &Value) -> Value {
    const CHIP: &str = "Couldn't change a document";
    let note = match document(args, CHIP) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let ops = match blocks::ops_arg(args) {
        Ok(ops) => ops,
        Err((k, why)) => {
            return tool_error(format!("ops: {}{why}", if k > 0 { format!("op {}: ", k + 1) } else { String::new() }), json!({ "id": note.id }), CHIP.to_string())
        }
    };
    let id = note.id;
    let picture = saw(id);
    let chat = versions::How::new(versions::kind::CHAT);
    // Under the store's lock: the document as it is when the edit lands.
    let outcome = versions::by(chat, || {
        crate::notes::modify(id, |n| {
            let was_title = Doc::of(&n.content).blocks.first().and_then(|b| super::heading(&b.text).filter(|(l, _)| *l == 1).map(|(_, w)| w.to_string()));
            match blocks::apply(&n.content, picture.as_deref(), &ops) {
                Ok(done) => {
                    let before = n.content.clone();
                    n.content = done.content.clone();
                    // The title heading renamed: the note's title follows.
                    let new_title = done.doc.blocks.first().and_then(|b| super::heading(&b.text).filter(|(l, _)| *l == 1).map(|(_, w)| w.to_string()));
                    if let (Some(old), Some(new)) = (&was_title, &new_title) {
                        if old.trim() == n.title.trim() && new != old && !new.trim().is_empty() {
                            n.title = new.trim().to_string();
                        }
                    }
                    let changed = before != n.content;
                    Ok((Ok((done, before, n.title.clone(), changed)), changed))
                }
                Err(r) => Ok((Err((r, n.content.clone(), n.title.clone())), false)),
            }
        })
    });
    match outcome {
        Err(e) => tool_result(false, Value::Null, e),
        Ok(Err((r, content, title))) => {
            let mut n = note.clone();
            n.content = content;
            n.title = title;
            let doc = Doc::of(&n.content);
            tracing::info!(note_id = id, op = r.op + 1, ops = ops.len(), "Chat: edit_document refused");
            refusal(&n, &doc, &r, &ops)
        }
        Ok(Ok((done, before, title, changed))) => {
            // The model's picture: what it knew, with its own edit made (the
            // person's changes it hasn't seen stay news for the next question).
            let pic = match picture.as_deref() {
                Some(p) if p != before => super::merge3(&before, p, &done.content),
                _ => done.content.clone(),
            };
            seen(id, &pic);
            let rev = versions::newest_notes(id).map_or(0, |(n, _)| n);
            remember_read(id, rev, &done.content);
            // The blocks it replaced or deleted, and what stands for each now.
            for (op, made) in ops.iter().zip(&done.made) {
                match op {
                    blocks::Op::Replace { id: old, .. } => retire(id, old.trim(), made.iter().map(|i| done.doc.ids[*i].id.clone()).collect()),
                    blocks::Op::Delete { id: old, .. } => retire(id, old.trim(), Vec::new()),
                    _ => {}
                }
            }
            let results: Vec<Value> = ops
                .iter()
                .zip(&done.made)
                .map(|(op, made)| {
                    let mut v = json!({ "op": op.name() });
                    if !made.is_empty() {
                        v["now"] = json!(made.iter().map(|i| json!({ "id": done.doc.ids[*i].id, "hash": done.doc.ids[*i].hash })).collect::<Vec<_>>());
                    }
                    v
                })
                .collect();
            tracing::info!(note_id = id, ops = ops.len(), rev, chars = done.content.chars().count(), "Chat: edit_document");
            tool_result(
                true,
                json!({ "id": id, "changed": changed, "rev": rev, "ops": results, "next": "Answer in one short sentence: nothing from the document (the user sees it change beside the chat)." }),
                edit_chip(&title, &ops, &done.made, &done.doc),
            )
        }
    }
}

// ---- document_history / restore_document ----------------------------------------------

/// Versions listed at most.
const HISTORY_SHOWN: usize = 20;

/// How a version came, as the model reads it.
fn how_words(v: &versions::Version) -> String {
    match v.kind.as_str() {
        k if k == versions::kind::CHAT => "Chat".to_string(),
        k if k == versions::kind::TYPED => "the user typed".to_string(),
        k if k == versions::kind::DICTATED => "the user dictated".to_string(),
        k if k == versions::kind::TRANSFORM => format!("a transform ({})", v.label),
        k if k == versions::kind::RESTORED => match (v.from, v.label.as_str()) {
            (Some(n), "Chat") => format!("Chat restored version {n}"),
            (Some(n), _) => format!("the user restored version {n}"),
            _ => "restored".to_string(),
        },
        k if k == versions::kind::EARLIER => "as it was before its history began".to_string(),
        _ => "edited".to_string(),
    }
}

/// A version's time, in this PC's time zone.
fn when(ms: u64) -> String {
    use chrono::TimeZone;
    chrono::Local.timestamp_millis_opt(ms as i64).single().map(|t| t.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default()
}

fn history(args: &Value) -> Value {
    const CHIP: &str = "Couldn't read a document's history";
    let note = match document(args, CHIP) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let t = quoted(&note.title);
    let listed: Vec<versions::Listed> = versions::list(note.id).unwrap_or_default().into_iter().filter(|l| l.version.field == versions::Field::Notes).collect();
    let total = listed.len();
    let mut out: Vec<Value> = Vec::new();
    for (i, l) in listed.iter().take(HISTORY_SHOWN).enumerate() {
        let v = &l.version;
        let mut item = json!({ "version": v.n, "at": when(v.ms), "by": how_words(v), "blocks": Doc::of(&v.text).blocks.len() });
        if l.current {
            item["current"] = json!(true);
        }
        // What it changed from the one before it.
        if let Some(prev) = listed.get(i + 1) {
            let (a, b) = (Doc::of(&prev.version.text), Doc::of(&v.text));
            let (mut added, mut removed, mut changed) = (0, 0, 0);
            for c in blocks::changes(&a, &b) {
                let (o, n) = (c.old.1 - c.old.0, c.new.1 - c.new.0);
                changed += o.min(n);
                added += n.saturating_sub(o);
                removed += o.saturating_sub(n);
            }
            let mut said = Vec::new();
            for (n, word) in [(changed, "changed"), (added, "added"), (removed, "removed")] {
                if n > 0 {
                    said.push(format!("{n} {word}"));
                }
            }
            item["change"] = json!(if said.is_empty() { "no change".to_string() } else { format!("blocks: {}", said.join(", ")) });
        } else {
            item["change"] = json!("the first kept");
        }
        out.push(item);
    }
    let mut data = json!({ "id": note.id, "title": note.title, "versions": out, "total": total });
    if total > HISTORY_SHOWN {
        data["more"] = json!(format!("{} older versions aren't listed.", total - HISTORY_SHOWN));
    }
    tracing::info!(note_id = note.id, versions = total, "Chat: document_history");
    tool_result(true, data, format!("Looked at the history of {t}"))
}

fn restore(args: &Value) -> Value {
    const CHIP: &str = "Couldn't restore a document";
    let note = match document(args, CHIP) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let t = quoted(&note.title);
    let Some(n) = arg_u64(args, &["version", "n", "rev"]) else {
        return tool_error("version is required: its number, from document_history.".to_string(), json!({ "id": note.id }), CHIP.to_string());
    };
    let Some(v) = versions::notes_version(note.id, n) else {
        return tool_error(
            format!("{t} has no version {n} (kept now): document_history lists them."),
            json!({ "id": note.id }),
            format!("Couldn't restore {t}"),
        );
    };
    // The person's words win: a restore over changes the model hasn't seen
    // waits until it has (it then knows what the restore takes out).
    if saw(note.id).is_some_and(|p| p != note.content) {
        tracing::info!(note_id = note.id, n, "Chat: restore_document over changes Chat hasn't seen; refused");
        return tool_error(
            format!("The user changed {t} since you last read it, so nothing was restored: read_document first (a restore would take their changes out), then restore if it's still what they asked for."),
            json!({ "id": note.id }),
            format!("Didn't restore {t}: changed since Chat read it"),
        );
    }
    if v.text == note.content {
        return tool_result(true, json!({ "id": note.id, "changed": false, "version": n }), format!("{t} is already as version {n} was"));
    }
    match versions::restore_by(note.id, n, "Chat") {
        Ok(r) => {
            seen(note.id, &r.text);
            let rev = versions::newest_notes(note.id).map_or(0, |(m, _)| m);
            remember_read(note.id, rev, &r.text);
            tracing::info!(note_id = note.id, n, rev, "Chat: restore_document");
            tool_result(
                true,
                json!({ "id": note.id, "changed": true, "version": n, "rev": rev, "blocks": Doc::of(&r.text).blocks.len(), "next": "Say in one short sentence which version is back (Undo in the panel takes it back)." }),
                format!("Restored version {n} of {t}"),
            )
        }
        Err(e) => tool_error(e.clone(), json!({ "id": note.id }), format!("Couldn't restore {t}: {e}")),
    }
}

/// The blocks a passage the person selected in the panel covers (`Change
/// this…`, `Ask about this`): those overlapping its lines `[from, to]` (the
/// note's lines, 0 = the first), else those whose text holds it (markdown
/// marks aside). Their indexes in `doc`. Pure.
pub fn selected_blocks(doc: &Doc, lines: Option<(usize, usize)>, text: &str) -> Vec<usize> {
    if let Some((from, to)) = lines {
        let hit: Vec<usize> = doc.blocks.iter().enumerate().filter(|(_, b)| b.start <= to && from < b.end.max(b.start + 1)).map(|(i, _)| i).collect();
        if !hit.is_empty() {
            return hit;
        }
    }
    let plain = |s: &str| {
        s.chars()
            .filter(|c| !matches!(c, '*' | '_' | '`' | '#' | '|' | '>' | '[' | ']'))
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let want = plain(text);
    if want.is_empty() {
        return Vec::new();
    }
    // The first words of each line of the selection, found in order.
    let parts: Vec<String> = text.lines().map(plain).filter(|p| !p.is_empty()).map(|p| p.chars().take(60).collect()).collect();
    let mut hit = Vec::new();
    for (i, b) in doc.blocks.iter().enumerate() {
        let body = plain(&b.text);
        if body.contains(&want) || parts.iter().any(|p| p.chars().count() >= 4 && body.contains(p.as_str())) {
            hit.push(i);
        }
    }
    hit
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::documents;

    fn made_doc(title: &str, sections: &[(&str, &str)]) -> u64 {
        let made = documents::execute("create_document", &json!({ "title": title, "sections": sections.iter().map(|(k, _)| json!({ "key": k, "intent": "" })).collect::<Vec<_>>() }));
        let id = made["data"]["id"].as_u64().unwrap();
        documents::after_call("create_document", &json!({}), &made);
        for (k, md) in sections {
            let args = json!({ "note_id": id, "key": k, "markdown": md });
            let r = documents::execute("write_section", &args);
            documents::after_call("write_section", &args, &r);
        }
        id
    }

    fn block<'a>(read: &'a Value, kind: &str) -> &'a Value {
        read["data"]["blocks"].as_array().unwrap().iter().find(|b| b["type"] == kind).unwrap()
    }

    #[test]
    fn the_rich_set_goes_to_the_strong_models_only() {
        for (p, m) in [
            ("anthropic", "claude-haiku-5-5"),
            ("anthropic", "claude-sonnet-4-5"),
            ("openai", "gpt-5"),
            ("openai", "gpt-5-mini"),
            ("openai", "o4-mini"),
            ("gemini", "gemini-2.5-pro"),
            ("gemini", "gemini-2.5-flash"),
            ("openrouter", "anthropic/claude-opus-4.1"),
            ("openrouter", "moonshotai/kimi-k2"),
            ("openrouter", "qwen/qwen3-235b-a22b"),
            ("groq", "openai/gpt-oss-120b"),
            ("groq", "llama-3.3-70b-versatile"),
            ("custom", "gpt-5"),
        ] {
            assert!(handles_rich(p, m), "{p} {m}");
        }
        for (p, m) in [
            ("ondevice", "Qwen_Qwen3.5-4B-Q4_K_M"),
            ("local", "qwen3.5:4b"),
            ("local", "llama3.3:70b"),
            ("openai", "gpt-5-nano"),
            ("openai", "gpt-4o-mini"),
            ("gemini", "gemini-2.5-flash-lite"),
            ("groq", "openai/gpt-oss-20b"),
            ("groq", "qwen/qwen3-32b"),
            ("openrouter", "meta-llama/llama-3.2-3b-instruct"),
            ("custom", "fake-chat"),
        ] {
            assert!(!handles_rich(p, m), "{p} {m}");
        }
        assert_eq!(ToolSet::for_model("anthropic", "claude-haiku-5-5"), ToolSet::Rich);
        assert_eq!(ToolSet::for_model("ondevice", "Qwen_Qwen3.5-4B-Q4_K_M"), ToolSet::Simple);
    }

    #[test]
    fn read_document_gives_blocks_with_ids_hashes_and_a_rev() {
        let id = made_doc("Lone wolf build", &[("Gear", "- Two rings\n- A short bow"), ("Levels", "| Level | Pick |\n|---|---|\n| 1 | Quick step |")]);
        let r = execute("read_document", &json!({ "note_id": id }));
        assert_eq!(r["success"], json!(true), "{r}");
        let data = &r["data"];
        assert!(data["rev"].as_u64().unwrap() >= 3);
        let kinds: Vec<&str> = data["blocks"].as_array().unwrap().iter().map(|b| b["type"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["heading", "heading", "list", "heading", "table"]);
        assert_eq!(block(&r, "table")["text"], json!("| Level | Pick |\n|---|---|\n| 1 | Quick step |"));
        // Read again with nothing changed: the same ids and hashes, the same rev.
        let again = execute("read_document", &json!({ "note_id": id }));
        assert_eq!(again["data"]["blocks"], data["blocks"]);
        assert_eq!(again["data"]["rev"], data["rev"]);
        // A note Chat didn't make: refused.
        let other = crate::notes::create("Mine", "My words", "manual", "").id;
        let r = execute("read_document", &json!({ "note_id": other }));
        assert!(r["error"].as_str().unwrap().contains("isn't a document Chat made"), "{r}");
        // A section still to write shows as one, not as its comment.
        let made = documents::execute("create_document", &json!({ "title": "Plan", "sections": [{ "key": "Route", "intent": "the way" }] }));
        let r = execute("read_document", &json!({ "note_id": made["data"]["id"] }));
        assert_eq!(block(&r, "section_to_write")["key"], json!("Route"));
        assert!(!r.to_string().contains("yap:section"));
    }

    #[test]
    fn a_long_document_comes_a_page_at_a_time() {
        let body: String = (0..120).map(|i| format!("Paragraph number {i} of a long fictional plan.\n\n")).collect();
        let id = crate::notes::create("Long", &format!("# Long\n\n{body}"), "chat", "").id;
        let first = execute("read_document", &json!({ "note_id": id }));
        assert_eq!(first["data"]["more"], json!(true));
        assert_eq!(first["data"]["blocks"].as_array().unwrap().len(), PAGE_BLOCKS);
        let next = first["data"]["next_offset"].as_u64().unwrap();
        let second = execute("read_document", &json!({ "note_id": id, "offset": next }));
        assert_eq!(second["data"]["to"], json!(121));
        assert!(second["data"]["more"].is_null());
        assert!(execute("read_document", &json!({ "note_id": id, "offset": 999 }))["error"].as_str().unwrap().contains("past the document's last block"));
    }

    #[test]
    fn since_gives_only_what_changed_with_the_rest_folded() {
        let id = made_doc("Trip", &[("Days", "Saturday: the walls."), ("Food", "Tea rooms."), ("Costs", "About 200.")]);
        let first = execute("read_document", &json!({ "note_id": id }));
        let rev = first["data"]["rev"].as_u64().unwrap();
        // The person types: a line changed, one added.
        let now = crate::notes::get(id).unwrap().content.replace("Tea rooms.", "Tea rooms, and the market.") + "\nBring an umbrella.\n";
        crate::notes::update(id, None, Some(now), None, None).unwrap();
        let r = execute("read_document", &json!({ "note_id": id, "since": rev }));
        let data = &r["data"];
        assert!(data["rev"].as_u64().unwrap() > rev, "{r}");
        let blocks = data["blocks"].as_array().unwrap();
        let kinds: Vec<String> = blocks.iter().map(|b| b["change"].as_str().map(str::to_string).unwrap_or_else(|| format!("unchanged {}", b["unchanged"]))).collect();
        assert_eq!(kinds, ["unchanged 4", "replaced", "unchanged 2", "added"], "{r}");
        assert_eq!(blocks[1]["was"][0]["text"], json!("Tea rooms."));
        assert_eq!(blocks[1]["now"][0]["text"], json!("Tea rooms, and the market."));
        // Nothing since: said so.
        let rev2 = data["rev"].as_u64().unwrap();
        let r = execute("read_document", &json!({ "note_id": id, "since": rev2 }));
        assert_eq!(r["data"]["unchanged"], json!(true), "{r}");
        // A rev never read nor kept: the whole document.
        let r = execute("read_document", &json!({ "note_id": id, "since": 9999 }));
        assert!(r["data"]["blocks"].as_array().unwrap().iter().all(|b| b["id"].is_string()));
    }

    #[test]
    fn edit_document_adds_a_table_row_and_is_one_chat_version() {
        let id = made_doc("Build", &[("Levels", "| Level | Pick |\n|---|---|\n| 1 | Quick step |")]);
        let read = execute("read_document", &json!({ "note_id": id }));
        let table = block(&read, "table").clone();
        let before = versions::list(id).unwrap().len();
        let args = json!({ "note_id": id, "ops": [{ "op": "replace", "id": table["id"], "if_hash": table["hash"], "markdown": format!("{}\n| 2 | Hide |", table["text"].as_str().unwrap()) }] });
        let r = execute("edit_document", &args);
        assert_eq!(r["success"], json!(true), "{r}");
        assert_eq!(r["displayText"], json!("Changed a table in \"Build\""));
        assert_eq!(r["data"]["changed"], json!(true));
        let new_id = r["data"]["ops"][0]["now"][0]["id"].as_str().unwrap();
        assert_ne!(new_id, table["id"].as_str().unwrap());
        assert!(crate::notes::get(id).unwrap().content.ends_with("| 1 | Quick step |\n| 2 | Hide |\n"));
        // One version, Chat's.
        let v = versions::list(id).unwrap();
        assert_eq!(v.len(), before + 1);
        assert_eq!(v[0].version.kind, versions::kind::CHAT);
        assert_eq!(r["data"]["rev"].as_u64(), Some(v[0].version.n));
        // The same edit sent again: done already, with the table as it is now.
        let again = execute("edit_document", &args);
        assert_eq!(again["success"], json!(false));
        assert!(again["error"].as_str().unwrap().contains("your own earlier edit already replaced block"), "{again}");
        assert_eq!(again["data"]["now"][0]["id"], json!(new_id));
        assert_eq!(again["displayText"], json!("Didn't change \"Build\": done already"));
    }

    /// `edit_document` never brings a section's placeholder back nor makes
    /// one up: not from markdown that copies one, not from the model's
    /// picture of the outline, and a placeholder the person brought back
    /// (Undo) stays the one there is. (The maintainer's "1 section not
    /// written yet" after a table change, 31 Oct 2026: the panel's Undo had
    /// brought it back, not the edit.)
    #[test]
    fn edit_document_never_brings_back_or_makes_up_a_placeholder() {
        let placeholders = |id: u64| crate::notes::get(id).unwrap().content.lines().filter(|l| documents::parse_placeholder(l).is_some()).count();
        let id = made_doc("Mage build", &[("Levels", "| Level | Pick |\n|---|---|\n| 20 | Frost |"), ("Tips", "- Rest often")]);
        assert_eq!(placeholders(id), 0);
        let read = execute("read_document", &json!({ "note_id": id }));
        let table = block(&read, "table").clone();
        // A replace whose markdown copies a placeholder line: none goes in.
        let args = json!({ "note_id": id, "ops": [{ "op": "replace", "id": table["id"], "if_hash": table["hash"],
            "markdown": "| Level | Pick |\n|---|---|\n| 21 | Frost |\n<!-- yap:section Tips | tips -->\n<!-- yap:section Extra -->" }] });
        let r = execute("edit_document", &args);
        assert_eq!(r["success"], json!(true), "{r}");
        assert_eq!(placeholders(id), 0);
        assert!(documents::pending_keys(&crate::notes::get(id).unwrap().content).is_empty());

        // The model saw the outline (a placeholder) and the person wrote that
        // section themselves: an edit elsewhere doesn't bring the outline's
        // placeholder back.
        let id = crate::notes::create("Swamp build", "", "chat", "").id;
        let outline = "# Swamp build\n\n## Gear\n\n- Boots\n\n<!-- yap:section Tips | tips -->\n";
        versions::by(versions::How::new(versions::kind::CHAT), || crate::notes::update(id, None, Some(outline.into()), None, None)).unwrap();
        documents::after_call("update_note", &json!({ "note_id": id, "content": outline }), &json!({ "success": true, "data": { "id": id } }));
        assert_eq!(saw(id).as_deref(), Some(outline));
        crate::notes::update(id, None, Some(outline.replace("<!-- yap:section Tips | tips -->", "## Tips\n\nMy own tip.")), None, None).unwrap();
        // (Its ids are its text's: the list as the model saw it.)
        let d = Doc::of(outline);
        let list = d.blocks.iter().position(|b| b.kind == blocks::Kind::List).map(|i| d.ids[i].clone()).unwrap();
        let r = execute("edit_document", &json!({ "note_id": id, "ops": [{ "op": "replace", "id": list.id, "if_hash": list.hash, "markdown": "- Boots\n- A hat" }] }));
        assert_eq!(r["success"], json!(true), "{r}");
        let now = crate::notes::get(id).unwrap().content;
        assert_eq!(placeholders(id), 0, "{now}");
        assert!(now.contains("My own tip.") && now.contains("- A hat"), "{now}");

        // A placeholder the person brought back (the panel's Undo) stays the
        // one there is: the edit changes its table and adds none.
        let id = made_doc("Ranger build", &[("Levels", "| Level | Pick |\n|---|---|\n| 20 | Quick step |"), ("Tips", "- Hide")]);
        let read = execute("read_document", &json!({ "note_id": id }));
        let table = block(&read, "table").clone();
        // (Undo as it was: back one version, to before Tips was written.)
        let before_tips = versions::list(id).unwrap().iter().filter(|l| l.version.field == versions::Field::Notes).nth(1).unwrap().version.n;
        versions::restore(id, before_tips).unwrap();
        assert_eq!(placeholders(id), 1);
        let r = execute("edit_document", &json!({ "note_id": id, "ops": [{ "op": "replace", "id": table["id"], "if_hash": table["hash"], "markdown": "| Level | Pick |\n|---|---|\n| 21–22 | Quick step |" }] }));
        assert_eq!(r["success"], json!(true), "{r}");
        let now = crate::notes::get(id).unwrap().content;
        assert!(now.contains("| 21–22 | Quick step |"), "{now}");
        assert_eq!(placeholders(id), 1, "{now}");
        assert_eq!(documents::pending_keys(&now), ["Tips"]);
    }

    #[test]
    fn a_stale_edit_is_refused_with_the_block_as_it_stands_now() {
        let id = made_doc("Kit", &[("Clothes", "- A fleece\n- Spare socks"), ("Extras", "- A map")]);
        let read = execute("read_document", &json!({ "note_id": id }));
        let list = block(&read, "list").clone();
        // The person changes that list in the panel.
        let theirs = crate::notes::get(id).unwrap().content.replace("- Spare socks", "- Spare socks, wool");
        crate::notes::update(id, None, Some(theirs.clone()), None, None).unwrap();
        let args = json!({ "note_id": id, "ops": [
            { "op": "insert", "markdown": "- A torch", "after": list["id"] },
            { "op": "replace", "id": list["id"], "if_hash": list["hash"], "markdown": "- A fleece" }
        ] });
        let r = execute("edit_document", &args);
        assert_eq!(r["success"], json!(false));
        assert!(r["error"].as_str().unwrap().starts_with("Op 1 (insert): the user changed block"), "{r}");
        assert_eq!(r["data"]["now"][0]["text"], json!("- A fleece\n- Spare socks, wool"));
        assert_eq!(r["displayText"], json!("Didn't change \"Kit\": the user changed that part"));
        // All or none: nothing changed.
        assert_eq!(crate::notes::get(id).unwrap().content, theirs);
        // With the block as it is now, it goes through.
        let now = &r["data"]["now"][0];
        let args = json!({ "note_id": id, "ops": [{ "op": "replace", "id": now["id"], "if_hash": now["hash"], "markdown": "- A fleece\n- Spare socks, wool\n- A torch" }] });
        assert_eq!(execute("edit_document", &args)["success"], json!(true));
        // A note that isn't Chat's: refused.
        let other = crate::notes::create("Mine", "Words", "manual", "").id;
        assert!(execute("edit_document", &json!({ "note_id": other, "ops": [{ "op": "insert", "markdown": "x" }] }))["error"].as_str().unwrap().contains("isn't a document Chat made"));
    }

    #[test]
    fn a_move_and_the_title_heading_follow() {
        let id = made_doc("Garden", &[("Beds", "Beans."), ("Paths", "Gravel.")]);
        let read = execute("read_document", &json!({ "note_id": id }));
        let b: Vec<Value> = read["data"]["blocks"].as_array().unwrap().clone();
        // Paths (heading and text) before Beds; the title heading renamed.
        let args = json!({ "note_id": id, "ops": [
            { "op": "move", "ids": [b[3]["id"], b[4]["id"]], "if_hashes": [b[3]["hash"], b[4]["hash"]], "before": b[1]["id"] },
            { "op": "replace", "id": b[0]["id"], "if_hash": b[0]["hash"], "markdown": "# Kitchen garden" }
        ] });
        let r = execute("edit_document", &args);
        assert_eq!(r["success"], json!(true), "{r}");
        let n = crate::notes::get(id).unwrap();
        assert_eq!(n.content, "# Kitchen garden\n\n## Paths\n\nGravel.\n\n## Beds\n\nBeans.\n");
        assert_eq!(n.title, "Kitchen garden");
        assert_eq!(r["displayText"], json!("Made 2 changes in \"Kitchen garden\""));
    }

    #[test]
    fn history_and_restore_bring_back_an_earlier_version() {
        let id = made_doc("Plan", &[("Route", "North.")]);
        let read = execute("read_document", &json!({ "note_id": id }));
        let p = block(&read, "paragraph").clone();
        let r = execute("edit_document", &json!({ "note_id": id, "ops": [{ "op": "replace", "id": p["id"], "if_hash": p["hash"], "markdown": "South." }] }));
        assert_eq!(r["success"], json!(true));
        let h = execute("document_history", &json!({ "note_id": id }));
        let list = h["data"]["versions"].as_array().unwrap();
        assert_eq!(list[0]["by"], json!("Chat"));
        assert_eq!(list[0]["current"], json!(true));
        assert_eq!(list[0]["change"], json!("blocks: 1 changed"));
        let earlier = list[1]["version"].as_u64().unwrap();
        // Back to it: a version itself, marked Chat's restore.
        let r = execute("restore_document", &json!({ "note_id": id, "version": earlier }));
        assert_eq!(r["success"], json!(true), "{r}");
        assert!(crate::notes::get(id).unwrap().content.contains("North."));
        let newest = &versions::list(id).unwrap()[0].version;
        assert_eq!((newest.kind.as_str(), newest.label.as_str()), (versions::kind::RESTORED, "Chat"));
        let h = execute("document_history", &json!({ "note_id": id }));
        assert_eq!(h["data"]["versions"][0]["by"], json!(format!("Chat restored version {earlier}")));
        // The person changed it since: a restore waits for a read.
        let typed = crate::notes::get(id).unwrap().content + "\nMy own line.\n";
        crate::notes::update(id, None, Some(typed), None, None).unwrap();
        let r = execute("restore_document", &json!({ "note_id": id, "version": earlier }));
        assert!(r["error"].as_str().unwrap().contains("read_document first"), "{r}");
        execute("read_document", &json!({ "note_id": id }));
        assert_eq!(execute("restore_document", &json!({ "note_id": id, "version": earlier }))["success"], json!(true));
        // A version that isn't there.
        assert!(execute("restore_document", &json!({ "note_id": id, "version": 999 }))["error"].as_str().unwrap().contains("has no version 999"));
    }

    #[test]
    fn a_selection_finds_its_blocks_by_line_or_by_its_words() {
        let doc = Doc::of("# T\n\nA **fast** ranger.\n\n- Two rings\n- A bow\n\nThe end.");
        assert_eq!(selected_blocks(&doc, Some((4, 5)), "Two rings"), [2]);
        assert_eq!(selected_blocks(&doc, Some((2, 4)), ""), [1, 2]);
        // No lines: by its words, markdown marks aside.
        assert_eq!(selected_blocks(&doc, None, "A fast ranger."), [1]);
        assert_eq!(selected_blocks(&doc, None, "Two rings\nA bow"), [2]);
        assert!(selected_blocks(&doc, None, "  ").is_empty());
    }
}

//! **A big project's files, searched** (16 Nov 2026; the Claude app's
//! project knowledge, which "automatically" searches a project's files once
//! they come near what the model can read, from Anthropic's help pages).
//!
//! - **Whole while they fit** ([`Room::whole`]): a project's files go whole
//!   with every question, as before (`projects::block_of`), while their
//!   tokens take at most half of what one request to the chat's model
//!   carries (`budget::budget`: its context less the answer's room, within
//!   a plan's tokens a minute and a model's cheaper rate).
//! - **Else in parts** ([`searched`]): each file in parts of about 700
//!   tokens (`parts.rs`), every part knowing its file and its pages, slides,
//!   rows or section. The system prompt (stable from turn to turn) says so
//!   and lists the files and their parts ([`index_block`]); each question
//!   gets the parts that match it in its own context ([`for_question`]),
//!   each with a `<source>` naming its file and where it is, so the answer
//!   can cite it.
//! - **Which parts** ([`rank`]): the meetings' hybrid ranking
//!   (`search/rank.rs`): BM25 over each part's words, with its file's name
//!   and section as its title, and — when search by meaning is on and its
//!   model is on this PC — each part's meaning, read in the background by
//!   the search index (`search/index.rs`, never in a dictation's way; a
//!   question waits for its own vector at most `MEMORY_WAIT`, never for a
//!   download). Without it, keywords alone. A part far weaker than the best
//!   isn't sent; at most [`MOST_PARTS`], within [`Room::parts`].
//! - **The model reads more** ([`READ`], [`SEARCH`]): `read_project_file`
//!   (a file by its name, a part by its number) and `search_project_files`
//!   (words across the files), in the tool loop and over your Claude plan's
//!   run endpoint (`claude_live/rpc.rs`). Read only.
//! - **The chat says which parts were used**: a chip each ("Used from the
//!   project: staff-handbook.pdf, pages 23–24"), the tools' too, folded
//!   under the answer as its other steps are (`chatSteps.js`).
//!
//! The log has counts and tokens, never names or words.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::parts::{self, Part};
use super::{Project, ProjectFile};
use crate::search::store::Key;

/// `read_project_file`: a part of a project's file, by its name and number.
pub const READ: &str = "read_project_file";
/// `search_project_files`: the parts that match some words.
pub const SEARCH: &str = "search_project_files";
/// Both.
pub const NAMES: &[&str] = &[READ, SEARCH];

/// The parts with a question: at least this many tokens of room…
pub const PARTS_LEAST: u64 = 1_000;
/// …at most this many…
pub const PARTS_MOST: u64 = 16_000;
/// …and at most this many parts.
pub const MOST_PARTS: usize = 12;
/// A part whose score is under this share of the best one's isn't sent.
pub const WEAKER: f32 = 0.3;
/// The files' list in the system prompt names each part while it takes at
/// most a 25th of what may go whole ([`Room::list`]), between these (else
/// each file's count of parts).
pub const LIST_LEAST: u64 = 600;
pub const LIST_MOST: u64 = 6_000;
/// What a search with the tool gives, at most.
const SEARCH_PARTS: usize = 4;

/// **The chat's model's room**, as the budget sees it: its context and the
/// most one request may carry (`budget::request_cap`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Room {
    pub context: u64,
    pub cap: Option<u64>,
}

impl Room {
    pub fn new(context: u64, cap: Option<u64>) -> Room {
        Room { context, cap }
    }

    fn request(&self, tools: bool) -> u64 {
        crate::chat::budget::budget(self.context, true, 0, crate::chat::budget::Kind::Chat, tools, self.cap).request
    }

    /// **What a project's files may take whole**: half of one request (the
    /// rest: the chat, the question, the answer). Pure.
    pub fn whole(&self) -> u64 {
        self.request(false) / 2
    }

    /// **What the parts with one question may take**: a tenth of a request,
    /// [`PARTS_LEAST`] to [`PARTS_MOST`] tokens. Pure.
    pub fn parts(&self) -> u64 {
        (self.request(true) / 10).clamp(PARTS_LEAST, PARTS_MOST)
    }

    /// **What the files' list of parts may take** in the system prompt: a
    /// 25th of [`Room::whole`], [`LIST_LEAST`] to [`LIST_MOST`]. Pure.
    pub fn list(&self) -> u64 {
        (self.whole() / 25).clamp(LIST_LEAST, LIST_MOST)
    }
}

/// A project's files' tokens, all together.
pub fn total(p: &Project) -> u64 {
    p.files.iter().map(|f| f.tokens).sum()
}

/// **Its files are searched in parts** for a model with `room`: they don't
/// fit whole ([`Room::whole`]). Pure.
pub fn searched(p: &Project, room: Room) -> bool {
    !p.files.is_empty() && total(p) > room.whole()
}

/// **Conversation `conv`'s project is searched in parts** for `room` (`None`:
/// in none, or whole).
pub fn searched_for(conv: Option<u64>, room: Room) -> bool {
    super::of_conversation(conv).is_some_and(|p| searched(&p, room))
}

// ---- the parts, kept -------------------------------------------------------------------------

/// **A file's parts**, with what ranking needs of them: each part's search
/// words, its title's (the file's name and its section), and its key in the
/// search index (what the embedding model reads of it).
pub struct Split {
    pub parts: Vec<Part>,
    terms: Vec<Vec<String>>,
    titles: Vec<Vec<String>>,
    /// The embedding model each key is for, and the keys.
    model: String,
    keys: Vec<Key>,
}

impl Split {
    fn of(f: &ProjectFile, text: &str) -> Split {
        let parts = parts::split(text, &f.kind);
        let terms = parts.iter().map(|p| crate::search::rank::terms(&p.text)).collect();
        let titles = parts.iter().map(|p| crate::search::rank::terms(&format!("{} {}", f.name, p.section))).collect();
        let model = crate::search::embedder::model().tag.to_string();
        let keys = parts.iter().map(|p| crate::search::chunks::key(&model, &embedded(f, p))).collect();
        Split { parts, terms, titles, model, keys }
    }
}

/// What the embedding model reads of a part: its file and place as the
/// title, its words.
fn embedded(f: &ProjectFile, p: &Part) -> String {
    crate::search::embedder::document(&format!("{} \u{2014} {}", f.name, p.place()), &p.text)
}

/// The files' parts, by file id (a file's text never changes once added).
static SPLITS: Mutex<Option<HashMap<String, Arc<Split>>>> = Mutex::new(None);
/// Files whose parts are kept, at most (then it starts again).
const KEPT: usize = 64;

/// **File `f` of project `project` in parts**, made once (`None`: its text
/// is gone). Reads it.
pub fn split_of(project: &str, f: &ProjectFile) -> Option<Arc<Split>> {
    let tag = crate::search::embedder::model().tag;
    if let Some(s) = SPLITS.lock().ok().and_then(|g| g.as_ref().and_then(|m| m.get(&f.id).cloned())).filter(|s| s.model == tag) {
        return Some(s);
    }
    let text = super::file_text(project, &f.id)?;
    let s = Arc::new(Split::of(f, &text));
    if let Ok(mut g) = SPLITS.lock() {
        let m = g.get_or_insert_with(HashMap::new);
        if m.len() >= KEPT {
            m.clear();
        }
        m.insert(f.id.clone(), s.clone());
    }
    Some(s)
}

/// A file taken out: its parts go too.
pub fn forget(file: &str) {
    if let Ok(mut g) = SPLITS.lock() {
        if let Some(m) = g.as_mut() {
            m.remove(file);
        }
    }
}

/// **Every searched part's key and what the embedding model reads of it**,
/// across every project (the search index's background pass: `index.rs`).
/// Only the files of projects big enough to be searched for some model
/// (over [`PARTS_LEAST`]·4 tokens): a small one always goes whole. Reads
/// the files.
pub fn passages() -> Vec<(Key, String)> {
    let projects: Vec<Project> = super::with_store(|s| s.projects.clone());
    let mut out = Vec::new();
    for p in projects.iter().filter(|p| total(p) > PARTS_LEAST * 4) {
        for f in &p.files {
            let Some(s) = split_of(&p.id, f) else { continue };
            out.extend(s.parts.iter().zip(&s.keys).map(|(part, k)| (*k, embedded(f, part))));
        }
    }
    out
}

// ---- ranking -------------------------------------------------------------------------------

/// **The parts that answer `query`** (its words: `rank::terms`), best first,
/// out of every file's (`splits`, in the project's order), each with its
/// meaning's match when it has one (`similarity`, by key): the meetings'
/// hybrid ranking, keywords alone with no meaning; none far weaker than the
/// best ([`WEAKER`]). `(file, part)` places. Pure.
pub fn rank(splits: &[Arc<Split>], query: &[String], similarity: &dyn Fn(&Key) -> Option<f32>, limit: usize) -> Vec<(usize, usize)> {
    let mut places: Vec<(usize, usize)> = Vec::new();
    let mut sims: Vec<Option<f32>> = Vec::new();
    for (fi, s) in splits.iter().enumerate() {
        for (pi, key) in s.keys.iter().enumerate() {
            places.push((fi, pi));
            sims.push(similarity(key));
        }
    }
    let none: [String; 0] = [];
    let cands: Vec<crate::search::rank::Candidate> = places
        .iter()
        .zip(&sims)
        .enumerate()
        .map(|(i, (&(fi, pi), sim))| crate::search::rank::Candidate { note: i as u64, terms: &splits[fi].terms[pi], title_terms: &splits[fi].titles[pi], speakers: &none, similarity: *sim })
        .collect();
    let mode = if sims.iter().any(Option::is_some) { crate::search::rank::Mode::Hybrid } else { crate::search::rank::Mode::Keyword };
    let ranked = crate::search::rank::rank(&cands, query, &[], mode, crate::search::rank::TUNING, limit.max(1) * 3);
    let best = ranked.first().map(|r| r.score).unwrap_or(0.0);
    ranked.into_iter().filter(|r| r.score >= best * WEAKER).take(limit).map(|r| places[r.index]).collect()
}

/// **What a question is searched by**: its own words, and — a follow-up of
/// two words or fewer ("and the showers?") — its words with the person's
/// question before it, whose parts come after its own.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    pub own: Vec<String>,
    pub with_before: Option<Vec<String>>,
}

/// [`Query`] for `question` after `previous`. Pure.
pub fn query_terms(question: &str, previous: Option<&str>) -> Query {
    let own = crate::search::rank::terms(question);
    let distinct: std::collections::HashSet<&String> = own.iter().collect();
    let with_before = previous.filter(|_| distinct.len() <= 2).map(|p| own.iter().cloned().chain(crate::search::rank::terms(p)).collect());
    Query { own, with_before }
}

/// [`rank`] for a [`Query`]: its own words' parts, then those of its words
/// with the question before it. Pure.
pub fn rank_query(splits: &[Arc<Split>], q: &Query, similarity: &dyn Fn(&Key) -> Option<f32>, limit: usize) -> Vec<(usize, usize)> {
    let mut out = rank(splits, &q.own, similarity, limit);
    if let Some(wb) = &q.with_before {
        for place in rank(splits, wb, similarity, limit) {
            if out.len() >= limit {
                break;
            }
            if !out.contains(&place) {
                out.push(place);
            }
        }
    }
    out
}

/// The ranked parts that fit in `room` tokens, in rank order (the first even
/// when it's a little over: half as much again). Pure.
fn within(ranked: &[(usize, usize)], splits: &[Arc<Split>], room: u64) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut used = 0u64;
    for &(fi, pi) in ranked {
        let t = splits[fi].parts[pi].tokens;
        let fits = used + t <= room || (out.is_empty() && t <= room + room / 2);
        if fits {
            used += t;
            out.push((fi, pi));
        }
    }
    out
}

/// The similarity of each part's key to the question's meaning (`None`: no
/// vector, or no index): one look at the index.
fn similarities(splits: &[Arc<Split>], vector: Option<&[f32]>) -> HashMap<Key, f32> {
    let Some(q) = vector else { return HashMap::new() };
    let keys: Vec<Key> = splits.iter().flat_map(|s| s.keys.iter().copied()).collect();
    crate::search::index::with_store(|s| keys.iter().filter_map(|k| s.similarity(k, q).map(|x| (*k, x))).collect())
}

/// The question's meaning, when search by meaning is on and its model here
/// (`embedder::query_vector`: never a download, at most `MEMORY_WAIT`).
async fn meaning(question: &str) -> Option<Vec<f32>> {
    crate::search::embedder::query_vector(question, crate::search::library::MEMORY_WAIT).await
}

// ---- what the model gets --------------------------------------------------------------------

/// A file's `<source>` line for part `p` of `count`: its name and what its
/// text is (`attachments::source`), then where the part is.
fn source(f: &ProjectFile, p: &Part, count: usize) -> String {
    let file = crate::chat::attachments::source(&f.name, &f.kind, f.pages, f.ocr_pages, f.unread_pages);
    format!("{file}: part {} of {count}, {}", p.n, p.place())
}

/// The parts as documents, as attachments go (`<documents>`).
fn documents(files: &[&ProjectFile], splits: &[Arc<Split>], chosen: &[(usize, usize)]) -> String {
    let mut out = String::from("<documents>\n");
    for (i, &(fi, pi)) in chosen.iter().enumerate() {
        let p = &splits[fi].parts[pi];
        out.push_str(&format!(
            "<document index=\"{}\">\n<source>{}</source>\n<document_content>\n{}\n</document_content>\n</document>\n",
            i + 1,
            source(files[fi], p, splits[fi].parts.len()),
            p.text.trim_end()
        ));
    }
    out.push_str("</documents>");
    out
}

/// The chip for a part sent with the question.
pub fn used_chip(f: &ProjectFile, p: &Part) -> String {
    format!("Used from the project: {}, {}", f.name, p.named())
}

/// **The system prompt's lines for a searched project** (after its name
/// and instructions): that its files go in parts, how to read more (`tools`:
/// the model has the tools), how to cite, and the files with their parts —
/// each part named while the list takes at most `list` tokens
/// ([`Room::list`]), else each file's count. Stable from turn to turn.
/// Reads the files.
pub fn index_block(p: &Project, tools: bool, list: u64) -> String {
    let splits: Vec<(&ProjectFile, Option<Arc<Split>>)> = p.files.iter().map(|f| (f, split_of(&p.id, f))).collect();
    let k = (total(p) / 1000).max(1);
    let mut out = format!(
        "\nThe project's files are its knowledge: {} file{}, about {k}K tokens in all, too long to send whole. So each question comes with the parts of them that match it, under \"From the project's files\" in its context, each with a <source> naming its file and where it is in it.",
        p.files.len(),
        if p.files.len() == 1 { "" } else { "s" }
    );
    if tools {
        out.push_str(&format!(" Each file is in numbered parts (listed below). When the answer may be in a part you weren't given, read it with {READ} (the file's name and the part's number) or look for words across the files with {SEARCH}; never guess what a file says."));
    }
    out.push_str(" Use them when they help, and say which file and where in it each fact comes from, as its source names it, like (staff-handbook.pdf, page 12).");
    let lines = |named: bool| -> Vec<String> {
        splits
            .iter()
            .filter_map(|(f, s)| {
                let s = s.as_ref()?;
                let what = crate::chat::attachments::source(&f.name, &f.kind, f.pages, f.ocr_pages, f.unread_pages);
                let count = s.parts.len();
                if !tools {
                    return Some(format!("- {what}"));
                }
                let parts = if named {
                    s.parts.iter().map(|q| format!("{} {}", q.n, q.place())).collect::<Vec<_>>().join("; ")
                } else {
                    let first = s.parts.first().map(|q| q.at.clone()).unwrap_or_default();
                    let last = s.parts.last().map(|q| q.at.clone()).unwrap_or_default();
                    if first.is_empty() || first == last { String::new() } else { format!("from {first} to {last}") }
                };
                Some(if parts.is_empty() {
                    format!("- {what}: {count} part{}", if count == 1 { "" } else { "s" })
                } else {
                    format!("- {what}: {count} part{}: {parts}", if count == 1 { "" } else { "s" })
                })
            })
            .collect()
    };
    let mut named = lines(true);
    if crate::chat::budget::estimate(&named.join("\n")) > list {
        named = lines(false);
    }
    out.push_str(&format!("\n<project_files>\n{}\n</project_files>", named.join("\n")));
    let gone: Vec<&str> = splits.iter().filter(|(_, s)| s.is_none()).map(|(f, _)| f.name.as_str()).collect();
    if !gone.is_empty() {
        out.push_str(&format!("\n(The project's {} {} gone from this PC.)", gone.join(", "), if gone.len() == 1 { "is" } else { "are" }));
    }
    out
}

/// **What a question gets of a searched project**: the parts that match it,
/// as documents (`block`, for the question's context), and a chip each.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Found {
    pub block: String,
    pub chips: Vec<String>,
}

/// What's said when no part matches the question.
fn nothing_matched(tools: bool) -> String {
    if tools {
        format!("From the project's files: no part matches this question's words. If the answer may be in them, look with {SEARCH} or read a part with {READ}; never guess what a file says.")
    } else {
        "From the project's files: no part matches this question's words.".to_string()
    }
}

/// **The parts of `p`'s files for `query`** within `room` tokens, ranked
/// with `similarity` (pure but for the files' reads).
pub fn found_in(p: &Project, query: &Query, similarity: &dyn Fn(&Key) -> Option<f32>, room: u64, tools: bool) -> Found {
    let (files, splits): (Vec<&ProjectFile>, Vec<Arc<Split>>) = p.files.iter().filter_map(|f| split_of(&p.id, f).map(|s| (f, s))).unzip();
    let ranked = rank_query(&splits, query, similarity, MOST_PARTS);
    let mut chosen = within(&ranked, &splits, room);
    if chosen.is_empty() {
        return Found { block: nothing_matched(tools), chips: Vec::new() };
    }
    // In the files' own order, as they read.
    chosen.sort();
    let chips = chosen.iter().map(|&(fi, pi)| used_chip(files[fi], &splits[fi].parts[pi])).collect();
    let block = format!(
        "From the project's files, the parts that match this question{}:\n{}",
        if tools { format!(" ({READ} reads any other part)") } else { String::new() },
        documents(&files, &splits, &chosen)
    );
    Found { block, chips }
}

/// **The parts for this question** (`previous`: the person's question before
/// it, for a short follow-up) when conversation `conv` is in a project
/// searched for `room`; at most `allow` tokens of them (the room the turn
/// has left, `turn/prompt.rs`). `None`: in no project, or one that goes
/// whole.
pub async fn for_question(conv: Option<u64>, question: &str, previous: Option<&str>, room: Room, allow: u64, tools: bool) -> Option<Found> {
    let p = super::of_conversation(conv)?;
    if !searched(&p, room) {
        return None;
    }
    let vector = meaning(question).await;
    let splits: Vec<Arc<Split>> = p.files.iter().filter_map(|f| split_of(&p.id, f)).collect();
    let sims = similarities(&splits, vector.as_deref());
    let query = query_terms(question, previous);
    let found = found_in(&p, &query, &|k| sims.get(k).copied(), room.parts().min(allow), tools);
    tracing::info!(files = p.files.len(), parts = found.chips.len(), meaning = vector.is_some(), tokens = crate::chat::budget::estimate(&found.block), "Chat: a project's files searched for a question");
    Some(found)
}

/// **A side question's project block** (`chat/side.rs`: one call, no
/// tools): the whole files while they fit, else the index and the parts that
/// match the side question. `None`: the chat's in no project.
pub async fn for_side(conv: u64, question: &str, room: Room) -> Option<String> {
    let p = super::of_conversation(Some(conv))?;
    let stable = super::prompt_block(Some(conv), room, false)?;
    if !searched(&p, room) {
        return Some(stable);
    }
    let found = for_question(Some(conv), question, None, room, room.parts(), false).await?;
    Some(format!("{stable}\n\n{}", found.block))
}

// ---- the tools ------------------------------------------------------------------------------

/// The tools' definitions, as the tool loop offers them.
pub fn definitions() -> Value {
    json!([
        { "type": "function", "function": { "name": READ, "description": read_description(), "parameters": read_schema() } },
        { "type": "function", "function": { "name": SEARCH, "description": search_description(), "parameters": search_schema() } },
    ])
}

/// The same, as Yap's run endpoint lists them for your Claude plan (MCP).
pub fn mcp_definitions() -> Vec<Value> {
    let tool = |name: &str, title: &str, description: &str, schema: Value| {
        json!({ "name": name, "title": title, "description": description, "inputSchema": schema, "annotations": { "title": title, "readOnlyHint": true, "openWorldHint": false } })
    };
    vec![
        tool(READ, "Read a part of a project file", &read_description(), read_schema()),
        tool(SEARCH, "Search the project's files", &search_description(), search_schema()),
    ]
}

fn read_description() -> String {
    "Read one part of a file in this chat's project, by the file's name and the part's number (the project's files list says what each part holds). Returns its text and where it is in the file.".to_string()
}

fn read_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "file": { "type": "string", "description": "The file's name, as the project's files list has it." },
            "part": { "type": "integer", "minimum": 1, "description": "The part's number (1 is the start)." }
        },
        "required": ["file", "part"],
        "additionalProperties": false
    })
}

fn search_description() -> String {
    "Search the files in this chat's project for words or a topic. Returns the parts that match best, each with its file and where it is in it.".to_string()
}

fn search_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": { "type": "string", "description": "What to look for." },
            "file": { "type": "string", "description": "Only in this file (its name), if given." }
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

fn ok(data: Value, display: String) -> Value {
    json!({ "success": true, "data": data, "displayText": display })
}

fn failed(error: String, display: String) -> Value {
    json!({ "success": false, "error": error, "data": Value::Null, "displayText": display })
}

/// The file `name` names: its own name (any case), else the one whose name
/// holds it, else the one it's closest to (a few letters off).
fn file_named<'a>(p: &'a Project, name: &str) -> Option<&'a ProjectFile> {
    let want = name.trim().to_lowercase();
    if want.is_empty() {
        return None;
    }
    p.files
        .iter()
        .find(|f| f.name.to_lowercase() == want)
        .or_else(|| p.files.iter().find(|f| f.name.to_lowercase().contains(&want) || want.contains(&f.name.to_lowercase())))
        .or_else(|| p.files.iter().map(|f| (edits(&f.name.to_lowercase(), &want), f)).filter(|(d, _)| *d <= 3).min_by_key(|(d, _)| *d).map(|(_, f)| f))
}

/// The letters to change, add or take out to make `a` into `b`
/// (Levenshtein). Pure.
fn edits(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let here = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != cb)).min(row[j] + 1).min(here + 1);
            prev = here;
        }
    }
    row[b.len()]
}

/// The files' names, for a call that named none of them.
fn names(p: &Project) -> String {
    p.files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join(", ")
}

/// A part as a tool gives it.
fn part_json(f: &ProjectFile, s: &Split, pi: usize) -> Value {
    let p = &s.parts[pi];
    json!({ "file": f.name, "part": p.n, "of": s.parts.len(), "where": p.place(), "source": source(f, p, s.parts.len()), "text": p.text })
}

/// **A call to one of the tools** in conversation `conv` (`None`: no
/// project; said so).
pub async fn execute(name: &str, args: &Value, conv: Option<u64>) -> Value {
    let Some(p) = super::of_conversation(conv) else {
        return failed("This chat isn't in a project: there are no project files to read.".into(), "No project files here".into());
    };
    match name {
        READ => read(&p, args),
        SEARCH => search(&p, args).await,
        _ => failed(format!("Unknown tool: {name}"), "Didn't read the project's files".into()),
    }
}

fn read(p: &Project, args: &Value) -> Value {
    let asked = args["file"].as_str().unwrap_or("");
    let Some(f) = file_named(p, asked) else {
        return failed(format!("No file in this project is called \"{}\". Its files: {}.", asked.trim(), names(p)), "Didn't find that file in the project".into());
    };
    let Some(s) = split_of(&p.id, f) else {
        return failed(format!("{} is gone from this PC.", f.name), format!("Couldn't read {} from the project", f.name));
    };
    let n = args["part"].as_u64().or_else(|| args["part"].as_str().and_then(|t| t.trim().parse().ok())).unwrap_or(1).max(1) as usize;
    if n > s.parts.len() {
        return failed(format!("{} has {} part{} (1 to {}).", f.name, s.parts.len(), if s.parts.len() == 1 { "" } else { "s" }, s.parts.len()), format!("Didn't read part {n} of {}", f.name));
    }
    let pi = n - 1;
    let mut data = part_json(f, &s, pi);
    if let Some(next) = s.parts.get(pi + 1) {
        data["next"] = json!(format!("part {}: {}", next.n, next.place()));
    }
    tracing::info!(part = n, of = s.parts.len(), "Chat: a project file's part read");
    ok(data, format!("Read from the project: {}, {}", f.name, s.parts[pi].named()))
}

async fn search(p: &Project, args: &Value) -> Value {
    let query = args["query"].as_str().unwrap_or("").trim().to_string();
    if query.is_empty() {
        return failed("Say what to look for.".into(), "Didn't search the project's files".into());
    }
    let only = args["file"].as_str().map(str::trim).filter(|s| !s.is_empty());
    let files: Vec<&ProjectFile> = match only {
        Some(name) => match file_named(p, name) {
            Some(f) => vec![f],
            None => return failed(format!("No file in this project is called \"{name}\". Its files: {}.", names(p)), "Didn't find that file in the project".into()),
        },
        None => p.files.iter().collect(),
    };
    let (files, splits): (Vec<&ProjectFile>, Vec<Arc<Split>>) = files.into_iter().filter_map(|f| split_of(&p.id, f).map(|s| (f, s))).unzip();
    let vector = meaning(&query).await;
    let sims = similarities(&splits, vector.as_deref());
    let ranked = rank(&splits, &crate::search::rank::terms(&query), &|k| sims.get(k).copied(), SEARCH_PARTS);
    let display = format!("Searched the project's files: \u{201c}{}\u{201d}", one_line(&query, 60));
    tracing::info!(found = ranked.len(), meaning = vector.is_some(), "Chat: a project's files searched by the model");
    if ranked.is_empty() {
        return ok(json!({ "parts": [], "note": "No part of the project's files matches those words. Try other words, or read a part by its number." }), display);
    }
    let parts: Vec<Value> = ranked.iter().map(|&(fi, pi)| part_json(files[fi], &splits[fi], pi)).collect();
    ok(json!({ "parts": parts }), display)
}

/// Whitespace runs as one space, at most `max` characters.
fn one_line(s: &str, max: usize) -> String {
    let t = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() <= max {
        t
    } else {
        format!("{}\u{2026}", t.chars().take(max).collect::<String>())
    }
}

/// **The tools by Claude Code's names** in a text of Yap's (your Claude
/// plan's prompt and question: `mcp__yap__read_project_file`). Pure.
pub fn claude_code_names(text: &str) -> String {
    let mut out = text.to_string();
    for n in NAMES {
        if out.contains(n) {
            out = out.replace(n, &format!("mcp__yap__{n}"));
        }
    }
    out
}

/// **A tool's result as Yap's run endpoint answers** (your Claude plan):
/// its JSON, as text (its `displayText` the chip), and whether it failed.
pub async fn mcp_call(name: &str, args: &serde_json::Map<String, Value>, conv: Option<u64>) -> (String, bool) {
    let out = execute(name, &Value::Object(args.clone()), conv).await;
    let failed = out["success"] == json!(false);
    (out.to_string(), failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fictional staff handbook in Markdown: `sections` sections of about
    /// `words` words, one of them the leave policy.
    pub(crate) fn handbook(sections: usize, words: usize) -> String {
        let topics = ["Working hours", "Expenses", "Annual leave", "Sick pay", "Parking", "Training", "Security", "Equipment", "Travel", "Pensions"];
        let mut s = String::from("# Example Ltd staff handbook\n\n");
        for i in 0..sections {
            let topic = topics[i % topics.len()];
            s.push_str(&format!("## {}. {topic}{}\n\n", i + 1, if i >= topics.len() { format!(" ({})", i / topics.len() + 1) } else { String::new() }));
            let mut para = String::new();
            while para.split_whitespace().count() < words {
                para.push_str(&format!("The {} policy at Example Ltd covers every member of staff, and line managers check it each quarter. ", topic.to_lowercase()));
            }
            s.push_str(&para);
            if topic == "Annual leave" && i < topics.len() {
                s.push_str("Full-time staff get 27 days of annual leave a year plus bank holidays, and up to five unused days carry over to the next year.");
            }
            if topic == "Parking" && i < topics.len() {
                s.push_str("Staff parking is on level two of the Kellbrook Road car park; permits come from reception.");
            }
            s.push_str("\n\n");
        }
        s
    }

    fn file(id: &str, name: &str, kind: &str, text: &str) -> ProjectFile {
        ProjectFile { id: id.into(), name: name.into(), kind: kind.into(), tokens: crate::chat::budget::estimate(text), ..Default::default() }
    }

    #[test]
    fn small_projects_go_whole_and_big_ones_in_parts_by_the_models_room() {
        let p = |tokens: u64| Project { files: vec![ProjectFile { tokens, ..Default::default() }], ..Default::default() };
        let claude = Room::new(200_000, None);
        let small = Room::new(8_192, None);
        // A 200K model: half its request (less the answer's room).
        assert_eq!(claude.whole(), (200_000 - 8_192) / 2);
        assert!(!searched(&p(40_000), claude) && searched(&p(120_000), claude));
        // An 8K model: about 3.5K whole.
        assert!(!searched(&p(3_000), small) && searched(&p(5_000), small));
        // A plan's tokens a minute caps it (Groq's free plan).
        assert!(searched(&p(5_000), Room::new(131_072, Some(7_200))));
        // No files: nothing to search.
        assert!(!searched(&Project::default(), small));
        // The parts' room: a tenth of a request, 1,000 to 16,000.
        assert_eq!(small.parts(), PARTS_LEAST);
        assert_eq!(claude.parts(), PARTS_MOST);
        assert_eq!(Room::new(32_768, None).parts(), (32_768 - 4_096) / 10);
    }

    fn splits_of(files: &[(&ProjectFile, &str)]) -> Vec<Arc<Split>> {
        files.iter().map(|(f, t)| Arc::new(Split::of(f, t))).collect()
    }

    #[test]
    fn the_part_that_answers_comes_first_by_its_words() {
        let hb = handbook(30, 260);
        let f = file("pf_00000000000000a1", "staff-handbook.md", "markdown", &hb);
        let splits = splits_of(&[(&f, &hb)]);
        assert!(splits[0].parts.len() >= 10, "{}", splits[0].parts.len());
        let q = query_terms("How many days of annual leave do I get?", None);
        let ranked = rank_query(&splits, &q, &|_| None, MOST_PARTS);
        let (fi, pi) = ranked[0];
        let top = &splits[fi].parts[pi];
        assert!(top.text.contains("27 days of annual leave"), "{}", top.text);
        assert!(top.section.contains("Annual leave"), "{}", top.section);
        // Parking: a name only one part has.
        let ranked = rank_query(&splits, &query_terms("Where do staff park on Kellbrook Road?", None), &|_| None, MOST_PARTS);
        assert!(splits[0].parts[ranked[0].1].text.contains("level two of the Kellbrook Road car park"));
        // Nothing in the files: nothing ranked.
        assert!(rank_query(&splits, &query_terms("hello there", None), &|_| None, MOST_PARTS).is_empty());
    }

    #[test]
    fn meaning_lifts_a_part_its_words_dont_name() {
        let hb = handbook(20, 200);
        let f = file("pf_00000000000000a2", "staff-handbook.md", "markdown", &hb);
        let splits = splits_of(&[(&f, &hb)]);
        // "Time off" shares no word with the leave section; its meaning does.
        let leave = splits[0].parts.iter().position(|p| p.text.contains("27 days")).unwrap();
        let key = splits[0].keys[leave];
        let sim = |k: &Key| Some(if *k == key { 0.72 } else { 0.31 });
        let ranked = rank_query(&splits, &query_terms("How much time off a year?", None), &sim, MOST_PARTS);
        assert_eq!(ranked[0], (0, leave));
        // Far weaker parts aren't sent.
        assert!(ranked.len() < splits[0].parts.len());
    }

    #[test]
    fn a_follow_up_searches_with_the_question_before_it() {
        assert_eq!(query_terms("And sick pay?", None), Query { own: vec!["sick".into(), "pay".into()], with_before: None });
        let q = query_terms("Why?", Some("How many days of annual leave carry over?"));
        assert!(q.own.is_empty());
        let wb = q.with_before.clone().unwrap();
        assert!(wb.contains(&"annual".to_string()) && wb.contains(&"carry".to_string()), "{q:?}");
        // A question with words of its own doesn't.
        assert_eq!(query_terms("What's the parking permit process?", Some("annual leave")).with_before, None);
        // "And parking?" after the leave question: parking's part first, the leave's after it.
        let hb = handbook(30, 260);
        let f = file("pf_00000000000000a6", "staff-handbook.md", "markdown", &hb);
        let splits = splits_of(&[(&f, &hb)]);
        let q = query_terms("And parking?", Some("How many days of annual leave do I get?"));
        let ranked = rank_query(&splits, &q, &|_| None, MOST_PARTS);
        assert!(splits[0].parts[ranked[0].1].section.contains("Parking"), "{}", splits[0].parts[ranked[0].1].section);
        assert!(ranked.iter().any(|&(_, i)| splits[0].parts[i].text.contains("27 days")));
        // "Why?" alone: the question before it.
        let ranked = rank_query(&splits, &query_terms("Why?", Some("How many days of annual leave do I get?")), &|_| None, MOST_PARTS);
        assert!(splits[0].parts[ranked[0].1].text.contains("27 days"));
    }

    #[test]
    fn the_parts_fit_their_room_in_the_files_order() {
        let hb = handbook(30, 260);
        let f = file("pf_00000000000000a3", "staff-handbook.md", "markdown", &hb);
        let splits = splits_of(&[(&f, &hb)]);
        let ranked: Vec<(usize, usize)> = (0..splits[0].parts.len()).rev().map(|i| (0, i)).collect();
        let one = splits[0].parts[ranked[0].1].tokens;
        let two = splits[0].parts[ranked[1].1].tokens;
        let chosen = within(&ranked, &splits, one + two);
        assert_eq!(chosen, ranked[..2]);
        // Too little room for any but a part a little over: that one.
        assert_eq!(within(&ranked, &splits, one * 3 / 4).len(), 1);
        assert!(within(&ranked, &splits, one / 4).is_empty());
    }

    #[test]
    fn a_project_in_parts_says_so_names_its_parts_and_gives_the_question_its_own() {
        let p = super::super::project_create("People team".into(), Some("HR questions".into())).unwrap();
        let hb = handbook(40, 260);
        let f = super::super::add_bytes(&p.id, "staff-handbook.md", hb.clone().into_bytes()).unwrap();
        let rota = "Rota for the front desk.\n\nMonday: Sam Example. Tuesday: Alex Example.".to_string();
        super::super::add_bytes(&p.id, "rota.txt", rota.into_bytes()).unwrap();
        let p = super::super::of(&p.id).unwrap();
        let small = Room::new(8_192, None);
        assert!(searched(&p, small));
        // The system prompt: no file's text, its parts listed (tools), the way to cite.
        let b = super::super::block_for(&p, small, true);
        assert!(b.starts_with("This chat is in the user's project \"People team\"."), "{b}");
        assert!(b.contains("too long to send whole") && b.contains("read_project_file") && b.contains("search_project_files"), "{b}");
        assert!(b.contains("(staff-handbook.pdf, page 12)"), "{b}");
        assert!(b.contains("<project_files>\n- staff-handbook.md: "), "{b}");
        assert!(b.contains("- rota.txt: 1 part"), "{b}");
        assert!(!b.contains("27 days of annual leave"), "never the files' text: {b}");
        // Without tools: the files named, nothing about reading more.
        let plain = super::super::block_for(&p, small, false);
        assert!(!plain.contains("read_project_file") && plain.contains("- staff-handbook.md\n- rota.txt"), "{plain}");
        // The question's parts: the leave section, with its source, and a chip.
        let q = query_terms("How many days of annual leave do I get?", None);
        let found = found_in(&p, &q, &|_| None, small.parts(), true);
        assert!(found.block.starts_with("From the project's files, the parts that match this question (read_project_file reads any other part):\n<documents>"), "{}", found.block);
        assert!(found.block.contains("27 days of annual leave"), "{}", found.block);
        assert!(found.block.contains("<source>staff-handbook.md: part "), "{}", found.block);
        assert!(found.block.contains("Annual leave\u{201d}</source>"), "{}", found.block);
        // Each part told apart: its number and section (a section's parts all say the section).
        assert!(found.chips.iter().all(|c| c.starts_with("Used from the project: staff-handbook.md, part ")), "{:?}", found.chips);
        assert!(found.chips.iter().any(|c| c.ends_with("Annual leave\u{201d}")), "{:?}", found.chips);
        assert!(crate::chat::budget::estimate(&found.block) <= small.parts() * 2);
        // Nothing matching: said, no chips.
        let none = found_in(&p, &query_terms("hello", None), &|_| None, small.parts(), true);
        assert!(none.chips.is_empty() && none.block.contains("no part matches"), "{}", none.block);
        // A 200K model reads it all whole.
        assert!(!searched(&p, Room::new(200_000, None)));
        let whole = super::super::block_for(&p, Room::new(200_000, None), true);
        assert!(whole.contains("27 days of annual leave") && whole.contains("<source>staff-handbook.md</source>"));
        let _ = f;
        let _ = std::fs::remove_dir_all(super::super::dir());
    }

    #[tokio::test]
    async fn the_tools_read_a_part_by_its_number_and_search_the_files() {
        let p = super::super::project_create("Office".into(), None).unwrap();
        let hb = handbook(30, 260);
        super::super::add_bytes(&p.id, "Staff Handbook.md", hb.into_bytes()).unwrap();
        super::super::chat_project_set(21, Some(p.id.clone())).unwrap();
        // Part 2, by a name in another case.
        let r = execute(READ, &json!({ "file": "staff handbook.md", "part": 2 }), Some(21)).await;
        assert_eq!(r["success"], true, "{r}");
        assert_eq!(r["data"]["part"], 2);
        assert!(r["data"]["source"].as_str().unwrap().starts_with("Staff Handbook.md: part 2 of "), "{r}");
        assert!(r["data"]["next"].as_str().unwrap().starts_with("part 3: "), "{r}");
        assert!(r["displayText"].as_str().unwrap().starts_with("Read from the project: Staff Handbook.md, "), "{r}");
        // A part past the end, a file that isn't there: said, with what is.
        let r = execute(READ, &json!({ "file": "Staff Handbook.md", "part": 999 }), Some(21)).await;
        assert_eq!(r["success"], false);
        assert!(r["error"].as_str().unwrap().contains("parts (1 to"), "{r}");
        let r = execute(READ, &json!({ "file": "budget.xlsx", "part": 1 }), Some(21)).await;
        assert!(r["error"].as_str().unwrap().contains("Its files: Staff Handbook.md."), "{r}");
        // A near miss of a name finds it.
        let r = execute(READ, &json!({ "file": "Staff Handbok.md", "part": 1 }), Some(21)).await;
        assert_eq!(r["success"], true, "{r}");
        // A search: the parking part.
        let r = execute(SEARCH, &json!({ "query": "Kellbrook Road car park permits" }), Some(21)).await;
        assert_eq!(r["success"], true);
        assert!(r["data"]["parts"][0]["text"].as_str().unwrap().contains("level two of the Kellbrook Road car park"), "{r}");
        assert_eq!(r["displayText"], "Searched the project's files: \u{201c}Kellbrook Road car park permits\u{201d}");
        // A chat in no project.
        let r = execute(READ, &json!({ "file": "x", "part": 1 }), Some(22)).await;
        assert_eq!(r["success"], false);
        // The plan's endpoint: the same, as text.
        let (text, failed) = mcp_call(READ, json!({ "file": "Staff Handbook.md", "part": 1 }).as_object().unwrap(), Some(21)).await;
        assert!(!failed && text.contains("\"part\":1"), "{text}");
        let _ = std::fs::remove_dir_all(super::super::dir());
    }

    #[test]
    fn every_reader_gives_parts_with_places() {
        // A PDF's pages, a deck's slides, a sheet's rows: each part says where.
        let pdf: String = (1..=30).map(|n| format!("[Page {n}]\n{}\n\n", "Holiday requests go to the line manager two weeks ahead. ".repeat(20))).collect();
        let f = file("pf_00000000000000a4", "handbook.pdf", "pdf", &pdf);
        let s = Split::of(&f, &pdf);
        assert!(s.parts.iter().all(|p| p.at.starts_with("page")), "{:?}", s.parts.iter().map(|p| &p.at).collect::<Vec<_>>());
        let src = source(&ProjectFile { pages: Some(30), ..f.clone() }, &s.parts[1], s.parts.len());
        assert!(src.starts_with("handbook.pdf (a PDF, 30 pages: its text, read on the person's PC): part 2 of "), "{src}");
        let deck: String = (1..=20).map(|n| format!("Slide {n}: Quarter plan {n}\n{}\n\n", "Targets for the shop floor this quarter. ".repeat(12))).collect();
        let s = Split::of(&file("pf_00000000000000a5", "plan.pptx", "powerpoint", &deck), &deck);
        assert!(s.parts.iter().all(|p| p.at.starts_with("slide")));
    }
}

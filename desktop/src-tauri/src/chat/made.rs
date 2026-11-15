//! **What Chat made, not just wrote** (15 Nov 2026, after the Claude app's
//! inline visuals and its artifacts; the idea and its look only, nothing of
//! its code): a diagram drawn or a small interactive widget, shown in the
//! answer itself — and pages Yap keeps ([`pages`]: `save_page`, opened
//! beside the chat, a version each change, in the chat's Outputs).
//!
//! - **`show_visual { title, code }`** ([`definitions`], [`show_visual`]):
//!   `code` is SVG (it starts with `<svg`) or an HTML fragment (a full
//!   document too). Offered to a strong model in Chat's tool loop where
//!   there's a thread to show it in ([`offered`]), and to a Claude Code
//!   session over Yap's run endpoint — a plan chat's and a Code chat's own
//!   (`integrations/claude_live/rpc.rs`). The model is told when: "draw…",
//!   "show me…", "make me a small calculator" ([`INSTRUCTIONS`]).
//! - **Kept with the chat**, never in chats.json (an older Yap sharing the
//!   data folder would drop a field it doesn't know when it saves): each
//!   conversation's folder under `chat-made/` ([`FOLDER`]) holds its index
//!   (`made.json`: a [`Made`] each, by the question it answers — its place
//!   and time, [`Anchor`]) and each visual's code (`<id>.svg` / `<id>.html`).
//!   Deleted with the chat ([`delete_conversations`], [`reset`]); a folder a
//!   reused conversation id finds is another chat's, never shown
//!   ([`Index::created`]).
//! - **Shown in a sandboxed frame** ([`serve`]): `yapmade://localhost/v/<chat>/<id>`
//!   (`http://yapmade.localhost/…` in WebView2), its own document with a
//!   strict Content-Security-Policy in the response itself — scripts inline
//!   or from a pinned allow-list ([`serve::LIBS`]: Chart.js and D3, fetched
//!   through `privacy/net.rs` and checked against their SHA-256), nothing
//!   else from the network, no frames, no forms — and CSP's own `sandbox`
//!   (scripts only: an opaque origin, never Yap's, so none of Yap's commands
//!   are reachable from inside). The page's frame says the same
//!   (`VisualFrame.svelte`, `sandbox="allow-scripts"`).
//! - **The page hears each at once** ([`EVENT`], `yap-chat-made`), and reads a
//!   chat's with [`chat_made_list`].
//!
//! **Privacy**: the log has ids, kinds, counts and sizes, never a title or
//! the code.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub mod pages;
pub mod serve;
#[cfg(test)]
mod tests;

/// The folder under Yap's data folder.
pub const FOLDER: &str = "chat-made";
/// The tool's name.
pub const SHOW_VISUAL: &str = "show_visual";
/// The tools this module answers.
pub const NAMES: &[&str] = &[SHOW_VISUAL, pages::SAVE_PAGE];
/// The page's event for something made (`{conversation, made}`).
pub const EVENT: &str = "yap-chat-made";
/// A visual's code, at most (bytes).
pub const MAX_CODE: usize = 256 * 1024;
/// A title's most characters.
pub const TITLE_MAX: usize = 80;
/// Visuals one question's answer makes, at most.
pub const MAX_PER_QUESTION: usize = 8;

/// **The model told when** (the tool loop's system prompt while it's
/// offered, and a Claude Code session's instructions): short, every step
/// sends it.
pub const INSTRUCTIONS: &str = "When the user asks you to draw, diagram, chart or show something visually (\"draw…\", \"show me how…\", \"make me a small calculator\"), call show_visual with an SVG diagram or a small interactive HTML widget: it appears in the chat itself, so never paste its code into your answer, and say in a sentence or two what it shows. Something to keep and come back to — a small app or tool (\"a tip calculator I can keep\") — is save_page instead (beside the chat; its id again makes a new version); writing to keep stays a document.";

// ---- what's kept --------------------------------------------------------------------------------

/// **The question something answers**: its place in the conversation and its
/// time (seconds), as chats.json has them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Anchor {
    #[serde(default)]
    pub index: usize,
    #[serde(default)]
    pub ts: u64,
}

/// **One thing made**: what `made.json` and the page keep (the same keys in JS).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Made {
    /// `v_` + 16 hex digits.
    pub id: String,
    /// "visual".
    #[serde(default)]
    pub kind: String,
    /// What the model called it ("How dictation works").
    #[serde(default)]
    pub title: String,
    /// "svg" | "html".
    #[serde(default)]
    pub format: String,
    /// The question it answers.
    #[serde(default)]
    pub after: Anchor,
    /// When it was made (ms since 1970).
    #[serde(default)]
    pub at: u64,
    /// Made after outside content came into its turn (the web, the screen, a
    /// connector): a button in it that sends a message puts it in the box
    /// for the person to send instead.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub outside: bool,
    /// Its code's size (a page: its newest version's).
    #[serde(default)]
    pub bytes: u64,
    /// **A page's versions** (`kind` "page", [`pages`]), oldest first: each
    /// with the question it answered (its card shows in that answer).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub versions: Vec<PageVersion>,
    /// What a later Yap keeps here that this one doesn't know: kept as it
    /// is when this one writes the index (never dropped).
    #[serde(flatten, default)]
    pub rest: serde_json::Map<String, Value>,
}

/// **One version of a page Yap keeps** ([`pages`]): `<id>.v<n>.html`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageVersion {
    pub n: u32,
    /// When it was saved (ms since 1970).
    #[serde(default)]
    pub at: u64,
    #[serde(default)]
    pub bytes: u64,
    /// The question whose answer saved it.
    #[serde(default)]
    pub after: Anchor,
    /// Its title when it was saved.
    #[serde(default)]
    pub title: String,
    #[serde(flatten, default)]
    pub rest: serde_json::Map<String, Value>,
}

/// **A conversation's `made.json`**.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Index {
    #[serde(default)]
    version: u32,
    /// The conversation's `created_ts`: a conversation id used again after
    /// its chat was deleted (by an older Yap, which leaves this folder) is
    /// another chat, whose folder this isn't.
    #[serde(default)]
    created: u64,
    #[serde(default)]
    items: Vec<Made>,
    /// What a later Yap keeps here that this one doesn't know (kept).
    #[serde(flatten, default)]
    rest: serde_json::Map<String, Value>,
}

/// Where everything made is kept.
pub fn dir() -> PathBuf {
    #[cfg(test)]
    {
        TEST_DIR.with(|d| d.clone())
    }
    #[cfg(not(test))]
    {
        crate::config::data_dir().join(FOLDER)
    }
}

#[cfg(test)]
thread_local! {
    static TEST_DIR: PathBuf = {
        let mut raw = [0u8; 8];
        let _ = getrandom::fill(&mut raw);
        let name: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        std::env::temp_dir().join(format!("yap-made-test-{name}")).join(FOLDER)
    };
}

/// A conversation's folder.
fn conv_dir(conv: u64) -> PathBuf {
    dir().join(conv.to_string())
}

fn index_path(conv: u64) -> PathBuf {
    conv_dir(conv).join("made.json")
}

/// A visual's code file in its folder: `<id>.svg` or `<id>.html`.
pub(crate) fn code_file(id: &str, format: &str) -> String {
    format!("{id}.{}", if format == "svg" { "svg" } else { "html" })
}

/// One writer at a time (two answers may make things at once).
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// `conv`'s index: `Ok(None)` when it has none, `Err` when it can't be read
/// (never written over then).
fn read_index(conv: u64) -> Result<Option<Index>, String> {
    let raw = match std::fs::read_to_string(index_path(conv)) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("couldn't read ({:?})", e.kind())),
    };
    serde_json::from_str(&raw).map(Some).map_err(|_| "couldn't read it".to_string())
}

/// A new id: `prefix` + 16 random hex digits.
fn new_id(prefix: &str) -> String {
    let mut raw = [0u8; 8];
    if getrandom::fill(&mut raw).is_err() {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        raw = (t as u64).to_le_bytes();
    }
    format!("{prefix}{}", raw.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

/// Whether `id` is one of ours: `v_` and 16 lowercase hex digits. Pure.
pub fn is_id(id: &str) -> bool {
    id.strip_prefix("v_").is_some_and(|h| h.len() == 16 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// **Keep `made`** (its code `code`) in conversation `conv` (made at
/// `created`): its code first, then the index that names it.
fn add(conv: u64, created: u64, made: &Made, code: &str) -> Result<(), String> {
    with_index(conv, created, |index, folder| {
        crate::config::atomic_write(&folder.join(code_file(&made.id, &made.format)), code).map_err(|e| format!("couldn't save it ({:?})", e.kind()))?;
        index.items.push(made.clone());
        Ok(())
    })
}

/// **Change conversation `conv`'s index** (made at `created`) with `f`, one
/// writer at a time: `f` gets the index and the chat's folder (made), writes
/// what it keeps there first, and changes the index, which is then saved —
/// never when `f` fails, nor over one that can't be read.
fn with_index<T>(conv: u64, created: u64, f: impl FnOnce(&mut Index, &std::path::Path) -> Result<T, String>) -> Result<T, String> {
    let _held = LOCK.lock().unwrap_or_else(|p| p.into_inner());
    let folder = conv_dir(conv);
    let mut index = match read_index(conv)? {
        Some(i) if i.created == created => i,
        // Another chat's, whose id this one got: gone with it.
        Some(_) => {
            let _ = std::fs::remove_dir_all(&folder);
            Index::default()
        }
        None => Index::default(),
    };
    // (A later Yap's own version number stays.)
    index.version = index.version.max(1);
    index.created = created;
    std::fs::create_dir_all(&folder).map_err(|e| format!("couldn't make its folder ({:?})", e.kind()))?;
    let out = f(&mut index, &folder)?;
    let json = serde_json::to_string(&index).map_err(|e| e.to_string())?;
    crate::config::atomic_write(&index_path(conv), &json).map_err(|e| format!("couldn't save it ({:?})", e.kind()))?;
    Ok(out)
}

/// **What conversation `conv` (made at `created`) has made**, oldest first:
/// none when its folder is another chat's or can't be read.
pub fn list(conv: u64, created: u64) -> Vec<Made> {
    match read_index(conv) {
        Ok(Some(i)) if i.created == created => i.items,
        Ok(_) => Vec::new(),
        Err(_) => {
            tracing::warn!(conversation = conv, "Chat: what a chat made couldn't be read; left as it is");
            Vec::new()
        }
    }
}

/// A visual's code, by its conversation and id (the frame's document,
/// [`serve`]). `None`: not one of that chat's.
pub fn code_of(conv: u64, id: &str) -> Option<(Made, String)> {
    if !is_id(id) {
        return None;
    }
    let index = read_index(conv).ok().flatten()?;
    let made = index.items.into_iter().find(|m| m.id == id)?;
    let code = std::fs::read_to_string(conv_dir(conv).join(code_file(&made.id, &made.format))).ok()?;
    Some((made, code))
}

/// A page's version `n` (its newest for 0), by its conversation and id (the
/// panel's frame, [`serve`]). `None`: not one of that chat's.
pub fn page_of(conv: u64, id: &str, n: u32) -> Option<(Made, u32, String)> {
    if !pages::is_id(id) {
        return None;
    }
    let index = read_index(conv).ok().flatten()?;
    let made = index.items.into_iter().find(|m| m.id == id && m.kind == "page")?;
    let n = if n == 0 { made.versions.iter().map(|v| v.n).max()? } else { made.versions.iter().find(|v| v.n == n)?.n };
    let html = std::fs::read_to_string(conv_dir(conv).join(pages::file_of(&made.id, n))).ok()?;
    Some((made, n, html))
}

/// **Chats deleted**: what they made goes with them.
pub fn delete_conversations(ids: &[u64]) {
    let mut gone = 0;
    for id in ids {
        if std::fs::remove_dir_all(conv_dir(*id)).is_ok() {
            gone += 1;
        }
    }
    if gone > 0 {
        tracing::info!(chats = gone, "Chat: what deleted chats made, deleted");
    }
}

/// **Every chat gone** (Reset app, "Delete some of your data" → Chats): the
/// whole folder, the cached libraries too.
pub fn reset() {
    match std::fs::remove_dir_all(dir()) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => tracing::warn!(kind = ?e.kind(), "Chat: what chats made couldn't all be deleted"),
        _ => {}
    }
}

// ---- the tool -----------------------------------------------------------------------------------

/// **Where `show_visual` is offered** in the tool loop: a strong model that
/// calls tools, with a thread to show it in — Chat, or the Voice Assistant
/// asked with Chat in view; never across the meetings. Pure.
pub fn offered(tools: bool, strong: bool, scope: Option<&str>, meetings: bool, chat_in_view: bool) -> bool {
    tools && strong && !meetings && (scope.is_none() || (scope == Some("assistant") && chat_in_view))
}

/// **The question asks for a visual** ("draw…", "a diagram of…", "chart…",
/// "make me a small calculator"): a turn fitted to a small context keeps
/// the tool (`fit.rs`). Pure.
pub fn asks_for_visual(question: &str) -> bool {
    let q = question.to_lowercase();
    let words: Vec<&str> = q.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    const WORDS: &[&str] = &["draw", "diagram", "diagrams", "chart", "charts", "flowchart", "graph", "plot", "visualise", "visualize", "visual", "sketch", "widget", "calculator", "slider", "infographic", "timeline"];
    words.iter().any(|w| WORDS.contains(w)) || q.contains("show me how") || q.contains("illustrate")
}

/// The tools' schemas, `show_visual` and `save_page` (none when not
/// offered). Kept short: every step sends them.
pub fn definitions(offer: bool) -> Value {
    if !offer {
        return json!([]);
    }
    let mut out = vec![json!({
        "type": "function",
        "function": {
            "name": SHOW_VISUAL,
            "description": DESCRIPTION,
            "parameters": parameters()
        }
    })];
    out.extend(pages::definitions(true).as_array().cloned().unwrap_or_default());
    Value::Array(out)
}

/// **Run one of these tools** (`NAMES`) for conversation `conv`.
pub fn execute(name: &str, conv: Option<u64>, args: &Value, outside: bool) -> Value {
    if name == pages::SAVE_PAGE {
        pages::save_page(conv, args, outside)
    } else {
        show_visual(conv, args, outside)
    }
}

/// What the tool does, for the model.
pub const DESCRIPTION: &str = "Show the user an SVG diagram or a small interactive HTML widget inline in the chat (\"draw…\", \"show me…\", \"make me a small calculator\"). Colours: var(--text), var(--muted), var(--surface), var(--border), var(--accent) and var(--c1)…var(--c6) (with --c1-soft… fills) suit light and dark. Scripts inline (or Chart.js 4.4.1 and D3 7.9.0 from cdnjs); nothing else loads from the network. sendPrompt(text) sends the user's next message when they click.";

/// Its parameters (the tool loop's and the run endpoint's).
pub fn parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string", "description": "What it shows, in a few words" },
            "code": { "type": "string", "description": "SVG starting with <svg, or an HTML fragment (no <html>, <head> or <body>); transparent background" }
        },
        "required": ["title", "code"],
        "additionalProperties": false
    })
}

fn failed(error: String, display: &str) -> Value {
    json!({ "success": false, "error": error, "data": Value::Null, "displayText": display })
}

/// `s` on one line, at most `max` characters (then "…"). Pure.
fn clip(s: &str, max: usize) -> String {
    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
    match one.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", one[..i].trim_end()),
        None => one,
    }
}

/// **What the code is**: "svg" when it's an SVG picture (an XML prologue
/// or comments may come first), else "html". Pure.
pub fn format_of(code: &str) -> &'static str {
    let mut rest = code.trim_start();
    while let Some(after) = rest.strip_prefix("<?xml").or_else(|| rest.strip_prefix("<!--")) {
        let end = if rest.starts_with("<?xml") { "?>" } else { "-->" };
        match after.find(end) {
            Some(i) => rest = after[i + end.len()..].trim_start(),
            None => return "html",
        }
    }
    if rest.get(..4).is_some_and(|s| s.eq_ignore_ascii_case("<svg")) {
        "svg"
    } else {
        "html"
    }
}

/// **The scripts the code loads from somewhere** (`<script src=…>`), as
/// written. Pure.
pub fn script_sources(code: &str) -> Vec<String> {
    let lower = code.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = lower[from..].find("<script") {
        let start = from + i;
        let Some(end) = lower[start..].find('>') else { break };
        let tag = &code[start..start + end];
        let tag_lower = &lower[start..start + end];
        if let Some(src) = attribute(tag, tag_lower, "src") {
            out.push(src);
        }
        from = start + end;
    }
    out
}

/// An attribute's value in a tag (`tag_lower` its lowercase twin). Pure.
fn attribute(tag: &str, tag_lower: &str, name: &str) -> Option<String> {
    let mut from = 0;
    while let Some(i) = tag_lower[from..].find(name) {
        let at = from + i;
        let before_ok = at == 0 || tag_lower.as_bytes()[at - 1].is_ascii_whitespace();
        let rest = tag_lower[at + name.len()..].trim_start();
        if before_ok && rest.starts_with('=') {
            let skip = tag_lower.len() - rest.len() + 1;
            let value = tag[skip..].trim_start();
            let v = match value.chars().next() {
                Some(q @ ('"' | '\'')) => value[1..].split(q).next().unwrap_or(""),
                _ => value.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or(""),
            };
            return Some(v.trim().to_string());
        }
        from = at + name.len();
    }
    None
}

/// The scripts a visual may not load (not inline, not on the allow-list). Pure.
pub fn refused_scripts(code: &str) -> Vec<String> {
    script_sources(code).into_iter().filter(|src| serve::lib_for(src).is_none()).collect()
}

/// **Run `show_visual`** for conversation `conv` (`None`: not a chat's
/// turn); `outside`: outside content came into the turn before it. Its
/// result, for the model; the page hears what was made at once.
pub fn show_visual(conv: Option<u64>, args: &Value, outside: bool) -> Value {
    let code = args["code"].as_str().or_else(|| args["svg"].as_str()).or_else(|| args["html"].as_str()).unwrap_or("").trim();
    if code.is_empty() {
        return failed("Give show_visual its code: an SVG diagram starting with <svg, or an HTML fragment.".into(), "Didn't make a visual: no code");
    }
    if code.len() > MAX_CODE {
        return failed(format!("That visual is too big ({} KB; at most {} KB): make it simpler.", code.len() / 1024, MAX_CODE / 1024), "Didn't make a visual: too big");
    }
    let refused = refused_scripts(code);
    if !refused.is_empty() {
        let allowed = serve::LIBS.iter().map(|l| format!("{} ({})", l.name, l.urls[0])).collect::<Vec<_>>().join(", ");
        return failed(format!("A visual can't load scripts from {}: write the script inline, or use {allowed}. Try again.", refused.join(", ")), "Didn't make a visual: a script from the web");
    }
    let Some(conv_id) = conv else {
        return failed("There's no chat to show a visual in for this question: answer in words.".into(), "Didn't make a visual: no chat here");
    };
    let Some(c) = crate::chat::chats::get(conv_id) else {
        return failed("This chat isn't there any more: answer in words.".into(), "Didn't make a visual: the chat's gone");
    };
    let after = anchor_of(&c.messages);
    let made_already = list(conv_id, c.created_ts).iter().filter(|m| m.after == after).count();
    if made_already >= MAX_PER_QUESTION {
        return failed(format!("This answer has made {MAX_PER_QUESTION} visuals already: answer in words now."), "Didn't make a visual: enough for one answer");
    }
    let title = clip(args["title"].as_str().unwrap_or(""), TITLE_MAX);
    let title = if title.is_empty() { "Visual".to_string() } else { title };
    let format = format_of(code).to_string();
    let made = Made { id: new_id("v_"), kind: "visual".into(), title, format, after, at: now_ms(), outside, bytes: code.len() as u64, ..Default::default() };
    if let Err(why) = add(conv_id, c.created_ts, &made, code) {
        tracing::warn!(conversation = conv_id, "Chat: a visual couldn't be saved: {why}");
        return failed("The visual couldn't be saved on this PC: answer in words.".into(), "Didn't make a visual: couldn't save it");
    }
    tracing::info!(conversation = conv_id, id = %made.id, format = %made.format, bytes = made.bytes, outside, "Chat: a visual made");
    emit(conv_id, &made);
    json!({
        "success": true,
        "data": {
            "shown": format!("Made a visual: {}", made.title),
            "id": made.id,
            "next": "It's shown in the chat above your answer: say in a sentence or two what it shows (never its code)."
        },
        "displayText": "Made a visual"
    })
}

/// **The question a turn answers now**: conversation's last message from
/// the person (an agent's report marker counts: Yap's own turn answers it).
/// Pure.
pub fn anchor_of(messages: &[crate::chat::chats::ChatMessage]) -> Anchor {
    messages.iter().enumerate().rev().find(|(_, m)| m.role == "user").map(|(index, m)| Anchor { index, ts: m.ts }).unwrap_or_default()
}

/// The page hears it at once.
fn emit(conv: u64, made: &Made) {
    #[cfg(not(test))]
    if let Some(app) = crate::chat::pages::view::app() {
        use tauri::Emitter;
        let _ = app.emit(EVENT, json!({ "conversation": conv, "made": made }));
    }
    #[cfg(test)]
    let _ = (conv, made, EVENT);
}

// ---- the page's -----------------------------------------------------------------------------

/// **`chat_made_list { conversationId }`**: what that chat made, oldest
/// first (none for a chat that isn't there).
#[tauri::command]
pub fn chat_made_list(conversation_id: u64) -> Vec<Made> {
    let Some(created) = crate::chat::chats::get(conversation_id).map(|c| c.created_ts) else {
        return Vec::new();
    };
    list(conversation_id, created)
}

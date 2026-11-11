//! **A plan turn's events in Chat's words** (pure): Claude Code's
//! stream-json (`claude_code::events`) → what Chat's stream hears
//! (`llm::Delta`, as every provider's stream gives it to `stream::Step`),
//! the turn's chips and its sources.
//!
//! - **Text** (`stream_event` text deltas) → `Delta::Content`, a step per
//!   model message (`message_start` → [`Out::new_step`]): text written
//!   before a tool call is never the answer (`stream::Step` takes it back).
//! - **Thinking** → `Delta::Reasoning`.
//! - **Claude Code's web tools** (`WebSearch`, `WebFetch`) → `Delta::Server`
//!   as Claude's own search on the Messages API is ("web_search",
//!   "web_fetch"), their input whole → `Delta::ServerInput` (the line
//!   "Searching the web · …", `chat/activity.rs`).
//! - **Yap's own tools** (`mcp__yap__search_notes`…, the run's endpoint,
//!   `integrations/claude_live.rs`) → `Delta::ToolCall` / `ToolArgs`, their
//!   input whole → `ServerInput` (the line with what it looks for).
//! - **A Code chat's reads** (Claude Code's `Read`, `Grep`, `Glob` in its
//!   workspace) → `Delta::ToolCall` as "read_file", "search_code",
//!   "find_files" (the line "Reading app.ts"; no chip).
//! - **Results** (`user` tool results): a chip each, as the tool loop's
//!   read ("Searched the web: \"…\"", "Read example.com/page"); a search's
//!   links and a page read become the answer's [`Source`]s, numbered as
//!   found ([`Out::chips`] after each) — the page beside the chat that Yap's
//!   `browser_read` read too. One of Yap's that answers in the tool loop's
//!   shape has its own chip (`displayText`), its pictures after it (MCP
//!   image content, `claude_live/eyes.rs`) never in the way.
//! - **The answer's citations** ([`cite`]): the plan's prompt asks for a
//!   markdown link to the page ("[1](https://…)"); a link to one of the
//!   turn's sources becomes Yap's own "[n]", so the Sources row, the pills
//!   and the checks (`finish::check`) work as for any answer.
//!
//! - **How full the conversation is** ([`Mapper::last_call_tokens`]): the
//!   turn's last model call's prompt (`message_start`'s usage, the cache's
//!   included) and answer (`message_delta`'s `output_tokens`), what the
//!   next call starts from. The result's `usage` is summed over the turn's
//!   calls, so it overstates it.
//!
//! A subagent's own messages (`parent_tool_use_id`) are never shown.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::ai::claude_code::events::{Block, BlockStart, Event, Problem, StreamKind, TurnResult};
use crate::ai::llm::Delta;
use crate::chat::chats::Source;

/// Yap's tools in a run, as Claude Code names them.
pub const YAP_PREFIX: &str = "mcp__yap__";

/// A tool call, as Yap knows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Claude Code's `WebSearch`.
    Search,
    /// Claude Code's `WebFetch`.
    Fetch,
    /// One of Yap's ("search_notes").
    Yap(String),
    /// A Code chat's read of its workspace (Claude Code's `Read`, `Grep`,
    /// `Glob`), by the name Chat's line knows it ("read_file"): a step, no
    /// chip.
    Code(&'static str),
    /// Anything else (its ToolSearch…): no chip, no line.
    Other(String),
}

impl Tool {
    pub fn of(name: &str) -> Tool {
        match name {
            "WebSearch" => Tool::Search,
            "WebFetch" => Tool::Fetch,
            "Read" => Tool::Code("read_file"),
            "Grep" => Tool::Code("search_code"),
            "Glob" => Tool::Code("find_files"),
            n => match n.strip_prefix(YAP_PREFIX) {
                Some(own) if !own.is_empty() => Tool::Yap(own.to_string()),
                _ => Tool::Other(n.to_string()),
            },
        }
    }

    /// The name Chat's stream and its lines know it by.
    pub fn name(&self) -> &str {
        match self {
            Tool::Search => "web_search",
            Tool::Fetch => "web_fetch",
            Tool::Code(n) => n,
            Tool::Yap(n) | Tool::Other(n) => n,
        }
    }
}

/// Who hears a plan turn: `chat/turn/plan.rs` (Chat's `stream::Step`s),
/// a test's log.
pub(crate) trait Out {
    fn delta(&mut self, d: Delta<'_>);
    /// A new model message began: its words are a step of their own.
    fn new_step(&mut self);
    /// The tools run so far: their chips, the sources found, whether the
    /// turn has read the web.
    fn chips(&mut self, chips: &[String], sources: &[Source], web: bool);
}

/// What one event meant for the turn.
#[derive(Clone, Debug, PartialEq)]
pub enum Fed {
    /// Read on.
    More,
    /// The turn is over.
    Done(Box<TurnResult>),
    /// What went wrong (before the event that showed it).
    Problem(Problem),
    /// The process ended.
    Exited(Option<i32>),
}

/// A tool call whose input is still coming.
struct Open {
    id: String,
    tool: Tool,
    json: String,
}

/// **One turn's events, read** ([`Mapper::feed`]).
#[derive(Default)]
pub struct Mapper {
    /// Content blocks with a tool call in them, by index (this message).
    open: HashMap<u64, Open>,
    /// The calls made, by id: what each was, with what.
    calls: HashMap<String, (Tool, Value)>,
    /// Calls already said (stream, then the whole message again).
    said: HashSet<String>,
    /// This message's text came as stream events, and the message it is.
    streamed: bool,
    stream_id: Option<String>,
    /// The last whole message's id (a stream-less run: a step each).
    last_message: Option<String>,
    chips: Vec<String>,
    sources: Vec<Source>,
    pages: Vec<String>,
    looked: bool,
    model: String,
    /// The last model call's prompt and answer so far, in tokens.
    last_call: Option<(u64, u64)>,
}

impl Mapper {
    pub fn new() -> Mapper {
        Mapper::default()
    }

    /// The chips so far ("Searched the web: \"…\"").
    pub fn chips(&self) -> &[String] {
        &self.chips
    }

    /// The web pages found and read, numbered as found.
    pub fn sources(&self) -> &[Source] {
        &self.sources
    }

    /// The text of the pages read (an answer's prices are checked against it).
    pub fn pages(&self) -> &[String] {
        &self.pages
    }

    /// The turn searched the web or read a page.
    pub fn looked(&self) -> bool {
        self.looked
    }

    /// The model that answered (`system/init`'s).
    pub fn model(&self) -> &str {
        &self.model
    }

    /// The turn's last model call: its prompt and its answer, in tokens
    /// (how full the conversation is now). `None` when it didn't say.
    pub fn last_call_tokens(&self) -> Option<u64> {
        self.last_call.map(|(prompt, answer)| prompt + answer)
    }

    /// One event: what it says to `out`, and whether the turn goes on.
    pub(crate) fn feed(&mut self, ev: &Event, out: &mut dyn Out) -> Fed {
        match ev {
            Event::Init(i) if !i.model.is_empty() => self.model.clone_from(&i.model),
            Event::Stream(s) if s.parent_tool_use_id.is_none() => self.stream(&s.kind, out),
            Event::Assistant(a) if a.parent_tool_use_id.is_none() && a.error.is_none() => {
                // Its words came as stream events already: the message whose
                // stream this is (Claude Code sends a message whole after its
                // stream, sometimes a block at a time).
                let streamed = self.streamed && (self.stream_id.is_none() || self.stream_id == a.message_id);
                // A message that didn't stream (a run without stream events,
                // one whose stream Yap missed): whole, a step of its own.
                if !streamed && a.message_id != self.last_message {
                    out.new_step();
                }
                self.last_message.clone_from(&a.message_id);
                for b in &a.blocks {
                    match b {
                        Block::Text(t) if !streamed && !t.is_empty() => out.delta(Delta::Content(t)),
                        Block::Thinking(t) if !streamed && !t.is_empty() => out.delta(Delta::Reasoning(t)),
                        Block::ToolUse { id, name, input } if !self.said.contains(id) => {
                            let tool = Tool::of(name);
                            self.begin(&tool, out);
                            self.announce(id.clone(), tool, input.clone(), out);
                        }
                        _ => {}
                    }
                }
            }
            Event::User(u) if u.parent_tool_use_id.is_none() && !u.is_replay => {
                let mut changed = false;
                for b in &u.blocks {
                    if let Block::ToolResult { tool_use_id, text, is_error, .. } = b {
                        if let Some((tool, input)) = self.calls.get(tool_use_id).cloned() {
                            changed |= self.result(&tool, &input, text, *is_error, u.tool_use_result.as_ref());
                        }
                    }
                }
                if changed {
                    out.chips(&self.chips, &self.sources, self.looked);
                }
            }
            Event::Result(r) => return Fed::Done(Box::new(r.clone())),
            Event::Problem(p) => return Fed::Problem(p.clone()),
            Event::Exited(c) => return Fed::Exited(*c),
            _ => {}
        }
        Fed::More
    }

    fn stream(&mut self, kind: &StreamKind, out: &mut dyn Out) {
        match kind {
            StreamKind::MessageStart { id, prompt, .. } => {
                // A new model call: what it read.
                self.last_call = prompt.map(|p| (p, 0));
                // This message streams: its whole copy after says nothing new.
                self.open.clear();
                self.streamed = true;
                self.stream_id.clone_from(id);
                out.new_step();
            }
            StreamKind::BlockStart { index, block: BlockStart::ToolUse { id, name } } => {
                // (Already said from its whole message: once.)
                if self.said.contains(id) {
                    return;
                }
                let tool = Tool::of(name);
                self.begin(&tool, out);
                self.said.insert(id.clone());
                self.open.insert(*index, Open { id: id.clone(), tool, json: String::new() });
            }
            StreamKind::Text { text, .. } => {
                self.streamed = true;
                out.delta(Delta::Content(text));
            }
            StreamKind::Thinking { thinking, .. } => {
                self.streamed = true;
                out.delta(Delta::Reasoning(thinking));
            }
            StreamKind::ToolInput { index, partial_json } => {
                if let Some(o) = self.open.get_mut(index) {
                    o.json.push_str(partial_json);
                    if let Tool::Yap(name) = &o.tool {
                        out.delta(Delta::ToolArgs { name, so_far: &o.json });
                    }
                }
            }
            StreamKind::MessageDelta { output_tokens: Some(n), .. } => {
                if let Some(call) = self.last_call.as_mut() {
                    call.1 = *n;
                }
            }
            StreamKind::BlockStop { index } => {
                if let Some(o) = self.open.remove(index) {
                    let input = serde_json::from_str::<Value>(o.json.trim()).ok().filter(Value::is_object).unwrap_or(Value::Object(Default::default()));
                    self.announce(o.id, o.tool, input, out);
                }
            }
            _ => {}
        }
    }

    /// A call began: Claude Code's web tools as a provider's own (what came
    /// before isn't the answer, which goes on after), Yap's and a Code
    /// chat's reads as a tool call.
    fn begin(&self, tool: &Tool, out: &mut dyn Out) {
        match tool {
            Tool::Search | Tool::Fetch => out.delta(Delta::Server(tool.name())),
            Tool::Yap(n) => out.delta(Delta::ToolCall(n)),
            Tool::Code(n) => out.delta(Delta::ToolCall(n)),
            Tool::Other(_) => {}
        }
    }

    /// Its input, whole: the line says with what ("Searching the web · …").
    fn announce(&mut self, id: String, tool: Tool, input: Value, out: &mut dyn Out) {
        if !matches!(tool, Tool::Other(_)) {
            out.delta(Delta::ServerInput { name: tool.name(), input: &input });
        }
        self.said.insert(id.clone());
        self.calls.insert(id, (tool, input));
    }

    /// A call's result: its chip, the sources it found. `true`: something
    /// to tell.
    fn result(&mut self, tool: &Tool, input: &Value, text: &str, is_error: bool, structured: Option<&Value>) -> bool {
        match tool {
            Tool::Search => {
                self.looked = true;
                let query = input["query"].as_str().unwrap_or("").trim().to_string();
                let links = if is_error { Vec::new() } else { search_links(structured, text) };
                for (title, url) in &links {
                    self.source(title, url, false);
                }
                let q = one_line(&query, 80);
                self.chips.push(match (is_error, links.is_empty()) {
                    (true, _) => format!("Searched the web: \"{q}\" · didn't work"),
                    (false, true) => format!("Searched the web: \"{q}\" · nothing found"),
                    (false, false) => format!("Searched the web: \"{q}\""),
                });
                true
            }
            Tool::Fetch => {
                let url = input["url"].as_str().or_else(|| structured.and_then(|s| s["url"].as_str())).unwrap_or("").trim().to_string();
                let short = crate::web::short_url(&url);
                let failed = is_error || structured.and_then(|s| s["code"].as_u64()).is_some_and(|c| c >= 400);
                if failed || url.is_empty() {
                    self.chips.push(format!("Couldn't read {}", if short.is_empty() { "a page".to_string() } else { short }));
                    return true;
                }
                self.looked = true;
                let title = self.sources.iter().find(|s| same_page(&s.url, &url)).map(|s| s.title.clone()).unwrap_or_else(|| short.clone());
                self.source(&title, &url, true);
                let body = structured.and_then(|s| s["result"].as_str()).unwrap_or(text);
                if !body.trim().is_empty() {
                    self.pages.push(body.to_string());
                }
                self.chips.push(format!("Read {short}"));
                true
            }
            Tool::Yap(name) => {
                // The page beside the chat, read: a page the turn read, as
                // a WebFetch's is (its own title, its text for the checks).
                if name == crate::chat::pages::READ && !is_error {
                    if let Some(read) = leading_json(text).filter(|v| v["success"] == serde_json::json!(true)) {
                        let d = &read["data"];
                        let url = d["url"].as_str().unwrap_or("").trim().to_string();
                        if !url.is_empty() {
                            self.looked = true;
                            self.source(d["title"].as_str().unwrap_or(""), &url, true);
                            if let Some(body) = d["text"].as_str().filter(|t| !t.trim().is_empty()) {
                                self.pages.push(body.to_string());
                            }
                        }
                    }
                }
                // One that answers as the tool loop's tools do (Agents',
                // the screen's and the pages': `{"success", "displayText",
                // …}`): its own chip, refusals too.
                if let Some(chips) = display_texts(text) {
                    self.chips.extend(chips);
                    return true;
                }
                let chip = yap_chip(name, input);
                self.chips.push(if is_error { format!("{chip} · didn't work") } else { chip });
                true
            }
            // (A Code chat's reads of its workspace: a line while they run,
            // no chip — the answer says which files it means.)
            Tool::Code(_) | Tool::Other(_) => false,
        }
    }

    /// A page the turn found (a search's link) or read (`opened`), numbered
    /// once.
    fn source(&mut self, title: &str, url: &str, opened: bool) {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return;
        }
        if let Some(s) = self.sources.iter_mut().find(|s| same_page(&s.url, url)) {
            s.opened |= opened;
            return;
        }
        let n = self.sources.len() + 1;
        let title = one_line(title, 120);
        self.sources.push(Source {
            n,
            title: if title.is_empty() { crate::web::short_url(url) } else { title },
            url: url.to_string(),
            site: crate::web::site_of(url),
            opened,
            ..Default::default()
        });
    }
}

/// Two addresses for the same page.
fn same_page(a: &str, b: &str) -> bool {
    crate::web::normal_url(a) == crate::web::normal_url(b)
}

/// `s` on one line, at most `max` characters (then "…"). Pure.
fn one_line(s: &str, max: usize) -> String {
    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
    match one.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", one[..i].trim_end()),
        None => one,
    }
}

/// **A WebSearch's links** (title, address): from Claude Code's structured
/// result (`tool_use_result.results`: `{tool_use_id, content: [{title,
/// url}]}` blocks among plain strings), else from the words the model got
/// ("Links: [{\"title\":…,\"url\":…}]"). Pure.
pub fn search_links(structured: Option<&Value>, text: &str) -> Vec<(String, String)> {
    fn push(out: &mut Vec<(String, String)>, title: &str, url: &str) {
        let url = url.trim();
        if !url.is_empty() && !out.iter().any(|(_, u)| same_page(u, url)) {
            out.push((title.trim().to_string(), url.to_string()));
        }
    }
    fn from_list(out: &mut Vec<(String, String)>, list: &Value) {
        for l in list.as_array().into_iter().flatten() {
            if let Some(url) = l["url"].as_str() {
                push(out, l["title"].as_str().unwrap_or(""), url);
            }
        }
    }
    let mut out: Vec<(String, String)> = Vec::new();
    if let Some(results) = structured.and_then(|s| s["results"].as_array()) {
        for r in results {
            // A block of links (plain strings between them are the model's words).
            if r.is_object() {
                from_list(&mut out, &r["content"]);
                if let Some(url) = r["url"].as_str() {
                    push(&mut out, r["title"].as_str().unwrap_or(""), url);
                }
            }
        }
    }
    if out.is_empty() {
        // "Links: [ … ]" in the text, each list on its line.
        let mut rest = text;
        while let Some(at) = rest.find("Links:") {
            rest = &rest[at + "Links:".len()..];
            let Some(open) = rest.find('[') else { break };
            let list = &rest[open..];
            let mut de = serde_json::Deserializer::from_str(list).into_iter::<Value>();
            if let Some(Ok(v)) = de.next() {
                from_list(&mut out, &v);
                rest = &list[de.byte_offset().min(list.len())..];
            } else {
                break;
            }
        }
    }
    out
}

/// The JSON a result starts with (the tool loop's shape), whatever follows
/// it: the screen's and the pages' calls put their pictures' lines after it
/// (`claude_live/eyes.rs`), and Claude Code adds where it kept a picture.
/// Pure.
fn leading_json(text: &str) -> Option<Value> {
    serde_json::Deserializer::from_str(text.trim_start()).into_iter::<Value>().next()?.ok().filter(Value::is_object)
}

/// A result in the tool loop's shape (`{"success", "displayText", …}`, the
/// agent tools', the screen's and the browser's): its chips — its own, and
/// a browser batch's steps' (`displayTexts`). Pure.
fn display_texts(text: &str) -> Option<Vec<String>> {
    let v = leading_json(text)?;
    v.get("success")?;
    let chip = |s: &str| Some(s.trim()).filter(|s| !s.is_empty()).map(str::to_string);
    let mut out: Vec<String> = v["displayText"].as_str().and_then(chip).into_iter().collect();
    out.extend(v["displayTexts"].as_array().into_iter().flatten().filter_map(Value::as_str).filter_map(chip));
    (!out.is_empty()).then_some(out)
}

/// The chip of one of Yap's own tools, as the tool loop words its own.
/// Pure.
pub fn yap_chip(name: &str, input: &Value) -> String {
    let q = |key: &str| one_line(input[key].as_str().unwrap_or(""), 60);
    match name {
        "list_meetings" => "Looked through your meetings".into(),
        "search_meetings" => format!("Searched your meetings: \"{}\"", q("query")),
        "get_meeting" => "Read a meeting".into(),
        "search_notes" => format!("Searched your notes: \"{}\"", q("query")),
        "get_note" => "Read a note".into(),
        "list_tasks" => "Looked at your tasks".into(),
        "list_folders" => "Looked at your folders".into(),
        "search_memory" => format!("Searched Yap's memory: \"{}\"", q("query")),
        "check_calendar" => "Checked your calendar".into(),
        other => format!("Used {other}"),
    }
}

/// **The answer's citations in Yap's form**: a markdown link to one of the
/// turn's pages ("[1](https://…)", "[the tide tables](https://…)") becomes
/// "[n]", its number in `sources` (a link whose words are more than a
/// number keeps them: "the tide tables [2]"); a link to anywhere else stays
/// a link. The sources it cites come back marked `cited`. Code is left
/// alone. Pure.
pub fn cite(reply: &str, sources: &[Source]) -> (String, Vec<Source>) {
    static LINK: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"\[([^\]\n]{1,200})\]\((https?://[^\s)]+)\)").expect("static regex"));
    let mut sources = sources.to_vec();
    if sources.is_empty() {
        return (reply.to_string(), sources);
    }
    let code = code_spans(reply);
    let mut out = String::with_capacity(reply.len());
    let mut at = 0;
    for m in LINK.captures_iter(reply) {
        let whole = m.get(0).expect("match");
        if code.iter().any(|&(s, e)| whole.start() >= s && whole.start() < e) {
            continue;
        }
        let Some(src) = sources.iter_mut().find(|s| same_page(&s.url, &m[2])) else { continue };
        src.cited = true;
        let words = m[1].trim();
        let numeric = words.trim_matches(|c: char| c == '^' || c.is_whitespace()).chars().all(|c| c.is_ascii_digit() || c == ',' || c == ' ');
        out.push_str(&reply[at..whole.start()]);
        if numeric {
            out.push_str(&format!("[{}]", src.n));
        } else {
            out.push_str(&format!("{words} [{}]", src.n));
        }
        at = whole.end();
    }
    out.push_str(&reply[at..]);
    // "[1][1]": one.
    let mut tidy = out.clone();
    for s in &sources {
        let twice = format!("[{0}][{0}]", s.n);
        while tidy.contains(&twice) {
            tidy = tidy.replace(&twice, &format!("[{}]", s.n));
        }
    }
    (tidy, sources)
}

/// **The same, as the answer streams** ([`cite`] for the whole): a link to
/// one of the turn's pages shows as Yap's "[n]" the moment it's whole, so
/// the pill draws while it's written and the voice never reads the link's
/// "1" out. Text that may still become a link ("[1](https://tid…") waits;
/// anything else goes on at once.
#[derive(Debug, Default)]
pub struct Cites {
    pending: String,
}

impl Cites {
    /// `more` of the answer: what can be shown now, with `sources`' links
    /// as "[n]".
    pub fn feed(&mut self, more: &str, sources: &[Source]) -> String {
        static WHOLE: std::sync::LazyLock<regex::Regex> =
            std::sync::LazyLock::new(|| regex::Regex::new(r"^\[([^\]\n]{1,200})\]\((https?://[^\s)]+)\)").expect("static regex"));
        self.pending.push_str(more);
        let mut out = String::new();
        loop {
            let Some(i) = self.pending.find('[') else {
                out.push_str(&std::mem::take(&mut self.pending));
                return out;
            };
            out.push_str(&self.pending[..i]);
            let rest = self.pending[i..].to_string();
            if let Some(m) = WHOLE.captures(&rest) {
                let whole = m.get(0).expect("match").as_str().to_string();
                let words = m[1].trim().to_string();
                let url = m[2].to_string();
                match sources.iter().find(|s| same_page(&s.url, &url)) {
                    Some(src) => {
                        let numeric = words.trim_matches(|c: char| c == '^' || c.is_whitespace()).chars().all(|c| c.is_ascii_digit() || c == ',' || c == ' ');
                        if numeric {
                            // "[1][1]": once.
                            let n = format!("[{}]", src.n);
                            if !out.ends_with(&n) {
                                out.push_str(&n);
                            }
                        } else {
                            out.push_str(&format!("{words} [{}]", src.n));
                        }
                    }
                    None => out.push_str(&whole),
                }
                self.pending = rest[whole.len()..].to_string();
                continue;
            }
            if link_coming(&rest) {
                // Still coming: it waits.
                self.pending = rest;
                return out;
            }
            // Not a link: the "[" goes on.
            out.push('[');
            self.pending = rest[1..].to_string();
        }
    }

    /// The step or the answer is over: whatever waited, as it is.
    pub fn flush(&mut self) -> String {
        std::mem::take(&mut self.pending)
    }

    /// What waited is void (a tool call came: the words before it weren't the answer).
    pub fn clear(&mut self) {
        self.pending.clear();
    }
}

/// `rest` (from a "[") may still become a markdown link to a web address:
/// its words so far ("[1"), or those and "](" and the start of an address
/// ("[1](https://ti"). Pure.
fn link_coming(rest: &str) -> bool {
    let Some(words) = rest.strip_prefix('[') else { return false };
    let Some(close) = words.find(']') else {
        return !words.contains('\n') && words.chars().count() <= 200;
    };
    if words[..close].contains('\n') {
        return false;
    }
    let tail = &words[close + 1..];
    if tail.is_empty() {
        return true;
    }
    let Some(addr) = tail.strip_prefix('(') else { return false };
    if "https://".starts_with(addr) || "http://".starts_with(addr) {
        return true;
    }
    (addr.starts_with("https://") || addr.starts_with("http://")) && !addr.contains(|c: char| c.is_whitespace() || c == ')')
}

/// Byte ranges of code in markdown: fenced blocks, and inline spans.
fn code_spans(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut fence: Option<usize> = None;
    let mut pos = 0;
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            match fence.take() {
                Some(start) => out.push((start, pos + line.len())),
                None => fence = Some(pos),
            }
        }
        pos += line.len();
    }
    if let Some(start) = fence {
        out.push((start, text.len()));
    }
    let mut i = 0;
    let bytes = text.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'`' && !out.iter().any(|&(s, e)| i >= s && i < e) {
            if let Some(end) = text[i + 1..].find('`') {
                out.push((i, i + 1 + end + 1));
                i += end + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::claude_code::events::parse_line;
    use serde_json::json;

    /// What a turn told its watcher, in order.
    #[derive(Default)]
    struct Log {
        said: Vec<String>,
        chips: Vec<Vec<String>>,
        sources: usize,
        web: bool,
    }

    impl Out for Log {
        fn delta(&mut self, d: Delta<'_>) {
            self.said.push(match d {
                Delta::Content(t) => format!("text {t}"),
                Delta::Reasoning(t) => format!("think {t}"),
                Delta::ToolCall(n) => format!("call {n}"),
                Delta::ToolArgs { name, so_far } => format!("args {name} {so_far}"),
                Delta::Server(n) => format!("server {n}"),
                Delta::ServerInput { name, input } => format!("input {name} {input}"),
                Delta::Restart => "restart".into(),
            });
        }
        fn new_step(&mut self) {
            self.said.push("step".into());
        }
        fn chips(&mut self, chips: &[String], sources: &[Source], web: bool) {
            self.chips.push(chips.to_vec());
            self.sources = sources.len();
            self.web = web;
        }
    }

    fn run(lines: &[&str]) -> (Mapper, Log, Option<Fed>) {
        let mut m = Mapper::new();
        let mut log = Log::default();
        let mut end = None;
        for l in lines {
            let ev = parse_line(&l.replace("{{session_id}}", "s-1")).unwrap();
            match m.feed(&ev, &mut log) {
                Fed::More => {}
                other => {
                    end = Some(other);
                    break;
                }
            }
        }
        (m, log, end)
    }

    /// The synthetic recording of a web search, a subagent and an answer.
    fn tool_use_fixture() -> Vec<String> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ai/claude_code/fixtures/2.1.296/synthetic-tool-use.jsonl");
        std::fs::read_to_string(path).unwrap().lines().filter(|l| l.starts_with("{\"type\"")).map(str::to_string).collect()
    }

    #[test]
    fn a_web_search_turn_becomes_steps_a_search_line_its_chip_and_sources() {
        let lines = tool_use_fixture();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let (m, log, end) = run(&refs);
        // A step for the message, its text before the search (taken back by
        // the step), the search as Claude's own, its query whole, then the
        // answer as a step of its own (the subagent's messages never shown).
        let said: Vec<&str> = log.said.iter().map(String::as_str).collect();
        assert_eq!(said[0], "step");
        assert_eq!(said[1], "text Let me look that up.");
        assert_eq!(said[2], "server web_search");
        assert_eq!(said[3], "input web_search {\"query\":\"tide times example bay\"}");
        assert!(!log.said.iter().any(|s| s.contains("WebFetch") || s.contains("Read https://tides")), "a subagent's page read never shows: {said:?}");
        // The answer's whole message (no stream events for it) is its own step.
        assert!(said.ends_with(&["step", "text High tide is at 06:12 and low tide at 12:30 [1]."]), "{said:?}");
        assert_eq!(log.chips, [vec!["Searched the web: \"tide times example bay\"".to_string()]]);
        assert_eq!((log.sources, log.web), (1, true));
        assert_eq!(m.sources()[0].url, "https://tides.example.com/bay");
        assert_eq!((m.sources()[0].n, m.sources()[0].site.as_str(), m.sources()[0].title.as_str()), (1, "tides.example.com", "Example Bay tides"));
        assert!(m.looked());
        assert_eq!(m.model(), "claude-sonnet-5-5");
        let Some(Fed::Done(r)) = end else { panic!("no result: {end:?}") };
        assert_eq!(r.cost_usd, 0.0384);
    }

    #[test]
    fn yaps_own_tools_are_tool_calls_with_their_query_and_a_chip() {
        let lines = [
            r#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"m1","model":"claude-haiku-5-5"}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"t1","name":"mcp__yap__search_notes","input":{}}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"query\":"}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"\"kitchen budget\"}"}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_stop","index":0}}"#,
            r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"mcp__yap__search_notes","input":{"query":"kitchen budget"}}]}}"#,
            r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"2 notes"}]}]}}"#,
            r#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"m2"}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Two notes."}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"The budget is £4,000."}}}"#,
            r#"{"type":"assistant","message":{"id":"m2","content":[{"type":"text","text":"The budget is £4,000."}]}}"#,
            r#"{"type":"result","subtype":"success","is_error":false,"result":"The budget is £4,000.","session_id":"s-1","total_cost_usd":0.002,"usage":{"input_tokens":30,"output_tokens":8}}"#,
        ];
        let (m, log, end) = run(&lines);
        assert_eq!(
            log.said,
            [
                "step",
                "call search_notes",
                "args search_notes {\"query\":",
                "args search_notes {\"query\":\"kitchen budget\"}",
                "input search_notes {\"query\":\"kitchen budget\"}",
                "step",
                "think Two notes.",
                "text The budget is £4,000.",
            ]
        );
        assert_eq!(log.chips, [vec!["Searched your notes: \"kitchen budget\"".to_string()]]);
        assert!(!m.looked() && m.sources().is_empty());
        assert!(matches!(end, Some(Fed::Done(_))));
    }

    #[test]
    fn an_agent_tools_chip_is_its_results_own_as_in_the_tool_loop() {
        let started = r#"{\"success\":true,\"data\":{\"id\":\"ab12\"},\"displayText\":\"Sent an agent to garden-planner\"}"#;
        let refused = r#"{\"success\":false,\"error\":\"start_agent is not allowed\",\"data\":null,\"displayText\":\"Didn't send an agent: not after reading the web\"}"#;
        let lines = [
            r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"j1","name":"mcp__yap__start_agent","input":{"folder":"garden-planner","brief":"Tidy notes.md"}},{"type":"tool_use","id":"j2","name":"mcp__yap__start_agent","input":{"folder":"garden-planner"}}]}}"#.to_string(),
            format!(r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"j1","content":[{{"type":"text","text":"{started}"}}]}}]}}}}"#),
            format!(r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"j2","content":[{{"type":"text","text":"{refused}"}}],"is_error":true}}]}}}}"#),
        ];
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let (m, log, _) = run(&refs);
        assert_eq!(m.chips(), ["Sent an agent to garden-planner", "Didn't send an agent: not after reading the web"]);
        assert!(log.said.contains(&"call start_agent".to_string()), "{:?}", log.said);
        // Plain text from another of Yap's tools: its usual chip.
        assert_eq!(display_texts("2 notes"), None);
        // A result with its pictures' lines after it (the screen's): its own chip.
        let looked = r#"{"success":true,"data":{"monitors":2},"displayText":"Looked at your screen (2 monitors)"}(From Yap, not the user.) Monitor 1 of 2 as it is now.[Image saved to tool-results/a.png]"#;
        assert_eq!(display_texts(looked), Some(vec!["Looked at your screen (2 monitors)".to_string()]));
        // A browser batch: each of its steps' chips.
        assert_eq!(display_texts(r#"{"success":false,"displayText":"","displayTexts":["Scrolled down wiki.example.com","Looked at the page · wiki.example.com"]}"#).unwrap(), ["Scrolled down wiki.example.com", "Looked at the page · wiki.example.com"]);
        assert_eq!(display_texts(r#"{"displayText":"no success field"}"#), None);
    }

    #[test]
    fn the_page_beside_the_chat_read_is_an_opened_source_and_a_screens_pictures_never_get_in_the_way() {
        let read = r#"{\"success\":true,\"data\":{\"tab\":1,\"n\":1,\"title\":\"Fort Joy - Example Wiki\",\"url\":\"https://wiki.example.com/wiki/Fort_Joy\",\"text\":\"Every chest marked.\"},\"displayText\":\"Read wiki.example.com/wiki/Fort_Joy beside the chat\"}"#;
        let looked = r#"{\"success\":true,\"data\":{\"monitors\":1},\"displayText\":\"Looked at your screen\"}"#;
        let lines = [
            r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"r1","name":"mcp__yap__browser_read","input":{}},{"type":"tool_use","id":"s1","name":"mcp__yap__look_at_screen","input":{}}]}}"#.to_string(),
            format!(r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"r1","content":[{{"type":"text","text":"{read}"}}]}}]}}}}"#),
            // Its picture as Claude Code hands it on: an image block, then where it kept it.
            format!(
                r#"{{"type":"user","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"s1","content":[{{"type":"text","text":"{looked}"}},{{"type":"text","text":"(From Yap, not the user.) Your screen as it is now."}},{{"type":"image","source":{{"type":"base64","media_type":"image/png","data":"iVBORw0KGgo="}}}},{{"type":"text","text":"[Image saved]"}}]}}]}}}}"#
            ),
        ];
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let (m, _, _) = run(&refs);
        assert_eq!(m.chips(), ["Read wiki.example.com/wiki/Fort_Joy beside the chat", "Looked at your screen"]);
        assert_eq!(m.sources().len(), 1);
        let s = &m.sources()[0];
        assert_eq!((s.n, s.title.as_str(), s.url.as_str(), s.opened), (1, "Fort Joy - Example Wiki", "https://wiki.example.com/wiki/Fort_Joy", true));
        assert_eq!(m.pages(), ["Every chest marked."]);
        assert!(m.looked());
    }

    #[test]
    fn a_page_read_is_an_opened_source_its_text_kept_and_a_failure_a_chip() {
        let mut m = Mapper::new();
        let mut log = Log::default();
        let ev = |l: &str| parse_line(l).unwrap();
        m.feed(&ev(r#"{"type":"assistant","message":{"id":"m1","content":[{"type":"tool_use","id":"f1","name":"WebFetch","input":{"url":"https://www.example.com/guide/","prompt":"the steps"}},{"type":"tool_use","id":"f2","name":"WebFetch","input":{"url":"https://gone.example.com/x"}}]}}"#), &mut log);
        m.feed(&ev(r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"f1","content":"Step one: unplug it."}]},"tool_use_result":{"url":"https://www.example.com/guide/","code":200,"result":"Step one: unplug it."}}"#), &mut log);
        m.feed(&ev(r#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"f2","content":"Request failed with status code 404","is_error":true}]}}"#), &mut log);
        assert_eq!(log.said[..2], ["step".to_string(), "server web_fetch".into()]);
        assert!(log.said[2].starts_with("input web_fetch {") && log.said[2].contains("\"url\":\"https://www.example.com/guide/\""), "{:?}", log.said);
        assert_eq!(m.chips(), ["Read example.com/guide", "Couldn't read gone.example.com/x"]);
        assert_eq!(m.sources().len(), 1);
        assert!(m.sources()[0].opened);
        assert_eq!(m.pages(), ["Step one: unplug it."]);
    }

    #[test]
    fn how_full_the_conversation_is_is_the_last_calls_prompt_and_answer() {
        // Two calls in one turn (a search, then the answer): the result's
        // usage sums them (10,400 read); the conversation is what the last
        // one read and wrote.
        let lines = [
            r#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"m1","usage":{"input_tokens":5000,"cache_read_input_tokens":0,"output_tokens":1}}}}"#,
            r#"{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":40}}}"#,
            r#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"m2","usage":{"input_tokens":200,"cache_creation_input_tokens":100,"cache_read_input_tokens":5100,"output_tokens":1}}}}"#,
            r#"{"type":"stream_event","event":{"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":60}}}"#,
            r#"{"type":"result","subtype":"success","usage":{"input_tokens":5200,"cache_read_input_tokens":5100,"cache_creation_input_tokens":100,"output_tokens":100},"modelUsage":{"claude-haiku-5-5":{"contextWindow":200000}}}"#,
        ];
        let (m, _, end) = run(&lines);
        assert_eq!(m.last_call_tokens(), Some(5_460));
        let Some(Fed::Done(r)) = end else { panic!() };
        assert_eq!(r.context_window("claude-haiku-5-5"), Some(200_000));
        // A call that says nothing of its prompt: not known.
        let (m, _, _) = run(&[r#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"m1"}}}"#]);
        assert_eq!(m.last_call_tokens(), None);
    }

    #[test]
    fn problems_and_the_end_stop_the_turn() {
        let (_, _, end) = run(&[r#"{"type":"system","subtype":"init","model":"claude-opus-5-5","session_id":"s-1"}"#]);
        assert_eq!(end, None);
        let mut m = Mapper::new();
        let mut log = Log::default();
        assert_eq!(m.feed(&Event::Problem(Problem::NotSignedIn), &mut log), Fed::Problem(Problem::NotSignedIn));
        assert_eq!(m.feed(&Event::Exited(Some(1)), &mut log), Fed::Exited(Some(1)));
        // A rate-limited message's own words are never the answer.
        let limited = parse_line(r#"{"type":"assistant","message":{"id":"x","content":[{"type":"text","text":"You've hit your limit"}]},"error":"rate_limit"}"#).unwrap();
        m.feed(&limited, &mut log);
        assert!(log.said.is_empty(), "{:?}", log.said);
    }

    #[test]
    fn search_links_come_from_the_structured_result_or_the_words() {
        let structured = json!({"query":"q","results":[{"tool_use_id":"s","content":[{"title":"A","url":"https://a.example.com/1"},{"title":"B","url":"https://b.example.com/2"}]},"Some words between.",{"tool_use_id":"s2","content":[{"title":"A again","url":"https://a.example.com/1#top"}]}]});
        assert_eq!(search_links(Some(&structured), ""), [("A".to_string(), "https://a.example.com/1".to_string()), ("B".into(), "https://b.example.com/2".into())]);
        let text = "Web search results for query: \"q\"\n\nLinks: [{\"title\":\"C\",\"url\":\"https://c.example.com/\"}]\n\nMore words.\n\nLinks: [{\"title\":\"D\",\"url\":\"https://d.example.com/x\"}]";
        assert_eq!(search_links(None, text).iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(), ["C", "D"]);
        assert!(search_links(Some(&json!({"results": []})), "No links here").is_empty());
    }

    #[test]
    fn links_to_the_turns_pages_become_yaps_citations() {
        let sources: Vec<Source> = ["https://tides.example.com/bay", "https://www.example.com/guide"]
            .iter()
            .enumerate()
            .map(|(i, u)| Source { n: i + 1, title: format!("T{}", i + 1), url: u.to_string(), site: crate::web::site_of(u), ..Default::default() })
            .collect();
        let reply = "High tide is at 06:12 [1](https://tides.example.com/bay/). Unplug it first ([the guide](https://example.com/guide)). See [elsewhere](https://other.example.org/) and `[3](https://tides.example.com/bay)`.";
        let (out, cited) = cite(reply, &sources);
        assert_eq!(out, "High tide is at 06:12 [1]. Unplug it first (the guide [2]). See [elsewhere](https://other.example.org/) and `[3](https://tides.example.com/bay)`.");
        assert!(cited.iter().all(|s| s.cited));
        // Twice in a row: once.
        let (out, _) = cite("Fact [1](https://tides.example.com/bay)[1](https://tides.example.com/bay).", &sources);
        assert_eq!(out, "Fact [1].");
        // No sources: as it was.
        assert_eq!(cite("A [link](https://x.example.com/)", &[]).0, "A [link](https://x.example.com/)");
    }

    #[test]
    fn a_streamed_link_to_a_page_shows_as_its_number_once_whole() {
        let sources = vec![Source { n: 1, title: "T".into(), url: "https://tides.example.com/bay".into(), ..Default::default() }];
        let mut c = Cites::default();
        // Split anywhere: the link waits until it's whole, the rest goes on.
        let mut shown = String::new();
        for piece in ["High tide at 06:12 ", "[", "1](https://ti", "des.example.com/bay", ")", ". A [note] and [2](https://other.example.org/x) stay."] {
            shown.push_str(&c.feed(piece, &sources));
        }
        shown.push_str(&c.flush());
        assert_eq!(shown, "High tide at 06:12 [1]. A [note] and [2](https://other.example.org/x) stay.");
        // Words for a link keep them; a "[" that can't be a link never waits.
        let mut c = Cites::default();
        assert_eq!(c.feed("See [the tables](https://tides.example.com/bay) now", &sources), "See the tables [1] now");
        assert_eq!(c.feed("a [b c", &sources), "a ");
        assert_eq!(c.feed("] d", &sources), "[b c] d");
        // Twice in a row: once.
        let mut c = Cites::default();
        assert_eq!(c.feed("x [1](https://tides.example.com/bay)[1](https://tides.example.com/bay).", &sources), "x [1].");
        // Waiting text is dropped with a tool call, and flushed at the end.
        let mut c = Cites::default();
        assert_eq!(c.feed("Look [1](https://tid", &sources), "Look ");
        c.clear();
        assert_eq!(c.flush(), "");
    }

    #[test]
    fn tools_are_named_as_chat_knows_them() {
        assert_eq!(Tool::of("WebSearch").name(), "web_search");
        assert_eq!(Tool::of("WebFetch").name(), "web_fetch");
        assert_eq!(Tool::of("mcp__yap__get_meeting"), Tool::Yap("get_meeting".into()));
        assert_eq!(Tool::of("ToolSearch"), Tool::Other("ToolSearch".into()));
        assert_eq!(yap_chip("search_memory", &json!({"query": "Sam"})), "Searched Yap's memory: \"Sam\"");
        assert_eq!(yap_chip("check_calendar", &json!({})), "Checked your calendar");
    }
}

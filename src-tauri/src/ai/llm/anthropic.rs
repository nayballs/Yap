//! **Claude on Anthropic's own Messages API, in Chat's tool loop**
//! (2026-10-31). Every other call to Anthropic — cleanup, Commands, the
//! meeting notes, the Voice Assistant's short answers — keeps Anthropic's
//! OpenAI-compatible endpoint (`/chat/completions`); a strong Claude's Chat
//! steps (`documents::rich::strong`) go to `POST {base}/messages` instead,
//! because that endpoint alone has what the Claude app has:
//!
//! - **Anthropic's server tools in the same request** as Yap's function
//!   tools ([`Native::server_tools`], `web/native.rs`): Claude searches the
//!   web and opens pages itself, in its own turn, as in the Claude app —
//!   not a separate sub-call that hands back a few passages. The compatible
//!   endpoint takes function tools only (Anthropic's docs, read 31 Oct 2026).
//! - **Thinking as asked**: adaptive thinking at an effort (Chat's Think,
//!   `chat/think.rs`). The compatible endpoint answers `{type: "adaptive"}`
//!   with HTTP 400 and never returns the thinking.
//! - **Prompt caching**: the tools and the system prompt (the same every
//!   step and every turn) carry a cache breakpoint, and the conversation
//!   another (top-level `cache_control`); the compatible endpoint has none.
//!
//! The tool loop stays the one loop (`chat/tools/agent.rs`): its OpenAI-shaped
//! messages and tools are turned into the Messages API's here ([`body`]),
//! and the answer back into an OpenAI-shaped message ([`message_of`]) —
//! with Claude's own blocks kept beside it (`yap_anthropic`: its thinking,
//! its searches and their encrypted results, its tool calls), which the next
//! step sends back unchanged, as the API asks; and what its server tools
//! did (`yap_web`: searches, pages read, the pages its answer cites, with a
//! mark where each citation goes) for the turn's sources, chips and `[n]`
//! citations (`web::Turn::took_server`). A server tool's error comes inside
//! a 200 (`error_code`): a chip says it, the turn goes on. A `pause_turn`
//! (the server's tool loop paused) is asked to go on, a few times at most.
//! Streamed as its events come ([`Stream`]): text, tool calls and their
//! arguments (a document's section into the panel), thinking, and each
//! server tool as "Searching the web…" / "Reading a page…", then with its
//! query or address once its input is whole (`Delta::ServerInput`: the step
//! the page shows, `chat/activity.rs`).
//!
//! Every request goes through the network receipts (`privacy/net.rs`:
//! `Purpose::Ai`) as every Chat call does; the pages Claude opens are opened
//! on Anthropic's side. The log has counts and lengths, never content.

use std::time::Duration;

use serde_json::{json, Map, Value};

use super::{ChatRequest, Delta, OnDelta};
use crate::privacy::net::Recorded as _;

/// Where a cited block's citation mark goes until the turn numbers it
/// (`web::Turn::took_server`): `\u{E000}<cite index>\u{E001}`.
pub const MARK_OPEN: char = '\u{E000}';
pub const MARK_CLOSE: char = '\u{E001}';

/// A step of a strong Claude's tool loop on the Messages API: the server
/// tools it carries (none with the web off).
#[derive(Debug, Clone, Default)]
pub struct Native {
    pub server_tools: Vec<Value>,
}

/// **Does Chat's tool loop speak the Messages API to this model?** A strong
/// Claude (`documents::rich::strong`) on Anthropic. Pure.
pub fn speaks_messages(provider: &str, model: &str) -> bool {
    provider == "anthropic" && crate::chat::documents::rich::strong(provider, model)
}

/// The most a step may write: room for a long section and the thinking
/// before it (a whole answer stays under what an HTTP client waits for).
const MAX_TOKENS: u32 = 16_000;
const MAX_TOKENS_STREAMED: u32 = 32_000;
/// How many times a paused server-tool loop (`pause_turn`) is asked to go on.
const PAUSES: usize = 4;
/// A step with server tools searches and reads before it answers: it may
/// take this long.
const SERVER_TOOLS_TIMEOUT: Duration = Duration::from_secs(150);

// ---- the request ----------------------------------------------------------------

/// A user message's content as the Messages API takes it: text, and the
/// screenshots (`image_url` parts: a data URL as base64, else by address).
/// Empty text is left out (the API refuses an empty text block). Pure.
fn user_blocks(content: &Value) -> Vec<Value> {
    match content {
        Value::String(s) if !s.trim().is_empty() => vec![json!({ "type": "text", "text": s })],
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| match p["type"].as_str() {
                Some("text") => p["text"].as_str().filter(|t| !t.trim().is_empty()).map(|t| json!({ "type": "text", "text": t })),
                Some("image_url") => {
                    let url = p["image_url"]["url"].as_str().or(p["image_url"].as_str()).unwrap_or("");
                    match url.strip_prefix("data:").and_then(|r| r.split_once(";base64,")) {
                        Some((media, data)) => Some(json!({ "type": "image", "source": { "type": "base64", "media_type": media, "data": data } })),
                        None if url.starts_with("http") => Some(json!({ "type": "image", "source": { "type": "url", "url": url } })),
                        None => None,
                    }
                }
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// A message's text, whatever its shape.
fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// Add `blocks` as the user's: to the user message before, when the last
/// one is (tool results first, then any text: as the API wants them).
fn push_user(out: &mut Vec<Value>, blocks: Vec<Value>) {
    if blocks.is_empty() {
        return;
    }
    if let Some(last) = out.last_mut().filter(|m| m["role"] == "user") {
        if let Some(content) = last["content"].as_array_mut() {
            content.extend(blocks);
            return;
        }
    }
    out.push(json!({ "role": "user", "content": blocks }));
}

/// **The Messages API request for one Chat step** (`req`: the loop's
/// OpenAI-shaped messages and function tools; `server`: Anthropic's server
/// tools; `stream`): the system prompt with a cache breakpoint (the tools
/// and it are the same every step), the messages turned — an earlier answer
/// of Claude's sent back as its own blocks (`yap_anthropic`) — the tools,
/// thinking as Chat's Think asks (`thinking`, `output_config`, its summary
/// shown: `display`), and the conversation cached up to its end. Pure.
pub fn body(req: &ChatRequest, server: &[Value], stream: bool) -> Value {
    let mut system: Vec<String> = Vec::new();
    let mut messages: Vec<Value> = Vec::new();
    for m in req.messages.as_array().into_iter().flatten() {
        match m["role"].as_str().unwrap_or("") {
            // The system prompt; a note in Yap's voice later on goes as the user's.
            "system" | "developer" if messages.is_empty() => system.push(text_of(&m["content"])),
            "system" | "developer" => push_user(&mut messages, user_blocks(&json!(text_of(&m["content"])))),
            "user" => push_user(&mut messages, user_blocks(&m["content"])),
            "tool" => push_user(
                &mut messages,
                vec![json!({ "type": "tool_result", "tool_use_id": m["tool_call_id"], "content": text_of(&m["content"]) })],
            ),
            "assistant" => {
                if let Some(own) = m["yap_anthropic"].as_array().filter(|b| !b.is_empty()) {
                    messages.push(json!({ "role": "assistant", "content": own }));
                    continue;
                }
                let mut blocks: Vec<Value> = Vec::new();
                let text = text_of(&m["content"]);
                if !text.trim().is_empty() {
                    blocks.push(json!({ "type": "text", "text": text }));
                }
                for c in m["tool_calls"].as_array().into_iter().flatten() {
                    let args = match &c["function"]["arguments"] {
                        Value::String(s) => serde_json::from_str(s).unwrap_or_else(|_| json!({})),
                        v @ Value::Object(_) => v.clone(),
                        _ => json!({}),
                    };
                    blocks.push(json!({ "type": "tool_use", "id": c["id"], "name": c["function"]["name"], "input": args }));
                }
                if !blocks.is_empty() {
                    messages.push(json!({ "role": "assistant", "content": blocks }));
                }
            }
            _ => {}
        }
    }
    let mut tools: Vec<Value> = req
        .tools
        .as_ref()
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|t| t["function"]["name"].is_string())
        .map(|t| {
            let f = &t["function"];
            json!({ "name": f["name"], "description": f["description"].as_str().unwrap_or(""), "input_schema": f["parameters"] })
        })
        .collect();
    tools.extend(server.iter().cloned());
    let streamed = if stream { MAX_TOKENS_STREAMED } else { MAX_TOKENS };
    let mut b = json!({
        "model": req.model,
        "max_tokens": req.max_tokens.unwrap_or(streamed),
        "system": [{ "type": "text", "text": system.join("\n\n"), "cache_control": { "type": "ephemeral" } }],
        "messages": messages,
        // The conversation, cached up to its end: the next step sends it again.
        "cache_control": { "type": "ephemeral" },
    });
    if !tools.is_empty() {
        b["tools"] = json!(tools);
        match req.extra.get("tool_choice").and_then(Value::as_str) {
            Some("none") => b["tool_choice"] = json!({ "type": "none" }),
            Some("required") => b["tool_choice"] = json!({ "type": "any" }),
            _ => {}
        }
    }
    // Thinking as Chat's Think asks (`think::request_extra`): its summary
    // comes back to show (adaptive thinking's default shows none).
    if let Some(t) = req.extra.get("thinking") {
        let mut t = t.clone();
        if t["type"] == "adaptive" {
            t["display"] = json!("summarized");
        }
        b["thinking"] = t;
    }
    for key in ["output_config", "max_tokens"] {
        if let Some(v) = req.extra.get(key) {
            b[key] = v.clone();
        }
    }
    if super::takes_temperature(&req.model) && b.get("thinking").is_none() {
        b["temperature"] = json!(req.temperature);
    }
    if stream {
        b["stream"] = json!(true);
    }
    b
}

// ---- the answer -----------------------------------------------------------------

/// A server tool's error code, said plainly.
fn server_error(code: &str) -> String {
    match code {
        "too_many_requests" => "it's limiting how much Yap can search right now".into(),
        "max_uses_exceeded" => "it reached its limit for this answer".into(),
        "url_not_accessible" => "the page didn't open".into(),
        "url_not_allowed" => "that site doesn't allow it".into(),
        "unsupported_content_type" => "not a page it can read".into(),
        "url_not_in_prior_context" => "not a link from you or the search".into(),
        "query_too_long" | "invalid_tool_input" | "invalid_input" => "it didn't take the request".into(),
        "unavailable" => "it isn't answering right now".into(),
        other => format!("it said {other}"),
    }
}

/// **Claude's answer as Chat's tool loop reads it**: an OpenAI-shaped
/// message — its text (the text after its last search or page: before it
/// was "I'll look that up"), a mark after each cited block for the pages it
/// cites ([`MARK_OPEN`]), its tool calls, its thinking (`reasoning_content`)
/// — with its own blocks (`yap_anthropic`, sent back next step) and what its
/// server tools did (`yap_web`: `searches` [{query, results [{url, title,
/// age}], error}], `fetches` [{url, title, error}], `cites` [{url, title,
/// text}]). Pure.
pub fn message_of(v: &Value) -> Value {
    let blocks: Vec<Value> = v["content"].as_array().cloned().unwrap_or_default();
    let server_result = |b: &Value| b["type"].as_str().is_some_and(|t| t.ends_with("_tool_result") && t != "tool_result");
    let last_result = blocks.iter().rposition(server_result);
    // Who asked for what (by the server tool call's id).
    let asked: Vec<(String, String, Value)> = blocks
        .iter()
        .filter(|b| b["type"] == "server_tool_use")
        .map(|b| (b["id"].as_str().unwrap_or("").to_string(), b["name"].as_str().unwrap_or("").to_string(), b["input"].clone()))
        .collect();
    let input_of = |id: &str| asked.iter().find(|(i, _, _)| i == id).map(|(_, _, input)| input.clone()).unwrap_or(Value::Null);
    let mut searches: Vec<Value> = Vec::new();
    let mut fetches: Vec<Value> = Vec::new();
    let mut cites: Vec<Value> = Vec::new();
    let mut text = String::new();
    let mut thinking = String::new();
    let mut calls: Vec<Value> = Vec::new();
    for (k, b) in blocks.iter().enumerate() {
        match b["type"].as_str().unwrap_or("") {
            "web_search_tool_result" => {
                let query = input_of(b["tool_use_id"].as_str().unwrap_or(""))["query"].as_str().unwrap_or("").to_string();
                match &b["content"] {
                    Value::Array(results) => searches.push(json!({
                        "query": query,
                        "results": results
                            .iter()
                            .filter(|r| r["url"].is_string())
                            .map(|r| json!({ "url": r["url"], "title": r["title"].as_str().unwrap_or(""), "age": r["page_age"] }))
                            .collect::<Vec<_>>(),
                    })),
                    other => searches.push(json!({ "query": query, "results": [], "error": server_error(other["error_code"].as_str().unwrap_or("unavailable")) })),
                }
            }
            "web_fetch_tool_result" => {
                let asked_url = input_of(b["tool_use_id"].as_str().unwrap_or(""))["url"].as_str().unwrap_or("").to_string();
                let c = &b["content"];
                if c["type"] == "web_fetch_result" {
                    fetches.push(json!({ "url": c["url"].as_str().unwrap_or(&asked_url), "title": c["content"]["title"].as_str().unwrap_or("") }));
                } else {
                    fetches.push(json!({ "url": asked_url, "title": "", "error": server_error(c["error_code"].as_str().unwrap_or("unavailable")) }));
                }
            }
            "text" if last_result.is_none_or(|l| k > l) => {
                text.push_str(b["text"].as_str().unwrap_or(""));
                let mut marks: Vec<usize> = Vec::new();
                for c in b["citations"].as_array().into_iter().flatten() {
                    // A search result's own address; a fetched page's by its title.
                    let (url, title) = match c["url"].as_str() {
                        Some(u) => (u.to_string(), c["title"].as_str().unwrap_or("").to_string()),
                        None => {
                            let t = c["document_title"].as_str().unwrap_or("");
                            match fetches.iter().find(|f| !t.is_empty() && f["title"] == t) {
                                Some(f) => (f["url"].as_str().unwrap_or("").to_string(), t.to_string()),
                                None => continue,
                            }
                        }
                    };
                    let at = match cites.iter().position(|x| x["url"] == url) {
                        Some(i) => i,
                        None => {
                            cites.push(json!({ "url": url, "title": title, "text": c["cited_text"].as_str().unwrap_or("") }));
                            cites.len() - 1
                        }
                    };
                    if !marks.contains(&at) {
                        marks.push(at);
                    }
                }
                for at in marks {
                    text.push_str(&format!("{MARK_OPEN}{at}{MARK_CLOSE}"));
                }
            }
            "thinking" => thinking.push_str(b["thinking"].as_str().unwrap_or("")),
            "tool_use" => calls.push(json!({
                "id": b["id"],
                "type": "function",
                "function": { "name": b["name"], "arguments": b["input"].to_string() },
            })),
            _ => {}
        }
    }
    let mut m = json!({ "role": "assistant", "content": if text.trim().is_empty() && !calls.is_empty() { Value::Null } else { json!(text) } });
    if !calls.is_empty() {
        m["tool_calls"] = json!(calls);
    }
    if !thinking.trim().is_empty() {
        m["reasoning_content"] = json!(thinking);
    }
    m["yap_anthropic"] = json!(blocks);
    if !(searches.is_empty() && fetches.is_empty() && cites.is_empty()) {
        m["yap_web"] = json!({ "searches": searches, "fetches": fetches, "cites": cites });
    }
    m
}

/// `text` with its citation marks as the turn numbers them (`numbers[i]`:
/// cite `i`'s number; one it doesn't have goes). Pure.
pub fn number_marks(text: &str, numbers: &[usize]) -> String {
    static MARK: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new("\u{E000}(\\d+)\u{E001}").expect("static regex"));
    MARK.replace_all(text, |c: &regex::Captures| c[1].parse::<usize>().ok().and_then(|i| numbers.get(i)).map_or(String::new(), |n| format!(" [{n}]")))
        .into_owned()
}

// ---- streamed ---------------------------------------------------------------------

/// **A streamed answer** (the Messages API's server-sent events), put
/// together block by block as it comes, the watcher told along the way.
#[derive(Default)]
pub struct Stream {
    blocks: Vec<Value>,
    /// Each block's tool input so far (`input_json_delta`), by index.
    json: Vec<String>,
    stop_reason: Option<String>,
    usage: Map<String, Value>,
    error: Option<String>,
    done: bool,
}

impl Stream {
    /// One event's data: the blocks updated, `on` told what shows.
    pub fn push(&mut self, data: &str, on: &mut dyn FnMut(Delta<'_>)) {
        let Ok(e) = serde_json::from_str::<Value>(data) else { return };
        let at = e["index"].as_u64().unwrap_or(0) as usize;
        match e["type"].as_str().unwrap_or("") {
            "message_start" => {
                if let Some(u) = e["message"]["usage"].as_object() {
                    self.usage.extend(u.clone());
                }
            }
            "content_block_start" => {
                while self.blocks.len() <= at {
                    self.blocks.push(Value::Null);
                    self.json.push(String::new());
                }
                let b = e["content_block"].clone();
                match b["type"].as_str().unwrap_or("") {
                    "tool_use" => on(Delta::ToolCall(b["name"].as_str().unwrap_or(""))),
                    "server_tool_use" => on(Delta::Server(b["name"].as_str().unwrap_or(""))),
                    _ => {}
                }
                self.blocks[at] = b;
            }
            "content_block_delta" => {
                let Some(b) = self.blocks.get_mut(at) else { return };
                let d = &e["delta"];
                match d["type"].as_str().unwrap_or("") {
                    "text_delta" => {
                        let s = d["text"].as_str().unwrap_or("");
                        let t = format!("{}{s}", b["text"].as_str().unwrap_or(""));
                        b["text"] = json!(t);
                        on(Delta::Content(s));
                    }
                    "thinking_delta" => {
                        let s = d["thinking"].as_str().unwrap_or("");
                        let t = format!("{}{s}", b["thinking"].as_str().unwrap_or(""));
                        b["thinking"] = json!(t);
                        on(Delta::Reasoning(s));
                    }
                    "signature_delta" => b["signature"] = d["signature"].clone(),
                    "citations_delta" => {
                        if !b["citations"].is_array() {
                            b["citations"] = json!([]);
                        }
                        if let Some(list) = b["citations"].as_array_mut() {
                            list.push(d["citation"].clone());
                        }
                    }
                    "input_json_delta" => {
                        self.json[at].push_str(d["partial_json"].as_str().unwrap_or(""));
                        let name = b["name"].as_str().unwrap_or("").to_string();
                        if b["type"] == "tool_use" && crate::chat::stream::LIVE_ARGS.contains(&name.as_str()) {
                            on(Delta::ToolArgs { name: &name, so_far: &self.json[at] });
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                if let Some(b) = self.blocks.get_mut(at) {
                    if matches!(b["type"].as_str(), Some("tool_use" | "server_tool_use")) {
                        let raw = self.json[at].trim();
                        if !raw.is_empty() {
                            b["input"] = serde_json::from_str(raw).unwrap_or_else(|_| json!({}));
                        } else if !b["input"].is_object() {
                            b["input"] = json!({});
                        }
                    }
                    // Its search or page, whole: the step the page shows
                    // ("Searching the web · …", `chat/activity.rs`).
                    if b["type"] == "server_tool_use" {
                        on(Delta::ServerInput { name: b["name"].as_str().unwrap_or(""), input: &b["input"] });
                    }
                }
            }
            "message_delta" => {
                if let Some(r) = e["delta"]["stop_reason"].as_str() {
                    self.stop_reason = Some(r.to_string());
                }
                if let Some(u) = e["usage"].as_object() {
                    self.usage.extend(u.clone());
                }
            }
            "message_stop" => self.done = true,
            "error" => self.error = Some(e["error"]["message"].as_str().unwrap_or("an error").chars().take(200).collect()),
            _ => {}
        }
    }

    /// The answer, whole, as the API would have sent it.
    pub fn whole(&self) -> Value {
        let blocks: Vec<Value> = self.blocks.iter().filter(|b| !b.is_null()).cloned().collect();
        json!({ "type": "message", "role": "assistant", "content": blocks, "stop_reason": self.stop_reason, "usage": self.usage })
    }
}

// ---- the call ----------------------------------------------------------------------

/// What a reply said, briefly: the API's own error message.
fn said(detail: &str) -> String {
    let v: Value = serde_json::from_str(detail).unwrap_or(Value::Null);
    v["error"]["message"].as_str().unwrap_or(detail).chars().take(300).collect()
}

/// **One Chat step on the Messages API** (`req`, with `native`'s server
/// tools), whole or streamed to `on`: the same rate-limit waits, receipts,
/// usage and evaluation log as every Chat call; a paused server-tool loop
/// asked to go on. The answer comes back as [`message_of`] makes it.
pub(super) async fn post(req: &ChatRequest, native: &Native, mut on: Option<OnDelta<'_>>) -> Result<Value, String> {
    let endpoint = format!("{}/messages", req.base_url.trim_end_matches('/'));
    let streaming = on.is_some();
    let mut body = body(req, &native.server_tools, streaming);
    crate::ai::eval::adjust_body(&mut body);
    let timeout = if native.server_tools.is_empty() { req.timeout } else { req.timeout.max(SERVER_TOOLS_TIMEOUT) };
    let timeout = if crate::snip::turn::has_images(&json!({ "messages": req.messages })) { timeout.max(super::IMAGE_TIMEOUT) } else { timeout };
    let client = reqwest::Client::builder()
        .timeout(crate::ai::eval::timeout(timeout))
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))?;
    let counted = crate::ai::usage::Call { provider: &req.provider, model: &req.model, base_url: &req.base_url, turn: true, feature: None };
    // Claude's blocks so far, across a paused server-tool loop.
    let mut before: Vec<Value> = Vec::new();
    for pause in 0..=PAUSES {
        let call = crate::ai::eval::Call::start(&req.provider, &endpoint, &body).await;
        let mut waited = Duration::ZERO;
        let mut attempt = 0u32;
        let resp = loop {
            call.send(&body);
            let resp = client
                .post(&endpoint)
                .header("x-api-key", &req.api_key)
                .header("anthropic-version", "2023-06-01")
                .json(&body)
                .send_for(crate::privacy::net::Purpose::Ai(req.provider.clone()))
                .await
                .map_err(|e| call.failed(format!("request failed: {e}")))?;
            let status = resp.status().as_u16();
            if (200..300).contains(&status) {
                break resp;
            }
            let header = |name: &str| resp.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
            let (retry_ms, retry_s) = (header("retry-after-ms"), header("retry-after"));
            let limits = crate::ai::eval::limit_headers(resp.headers());
            let detail = resp.text().await.unwrap_or_default();
            call.refused(status, &limits, &detail);
            // Rate-limited or overloaded: waited out, as every Chat call is.
            if matches!(status, 429 | 529) {
                let hinted = super::rate_limit_wait(retry_ms.as_deref(), retry_s.as_deref(), &detail);
                let wait = hinted.unwrap_or(Duration::from_secs(2u64 << attempt.min(3))) + Duration::from_millis(250);
                if attempt < 4 && waited + wait <= req.rate_limit_patience {
                    tracing::info!(status, "Anthropic asked Yap to wait; trying again in {:.1} s", wait.as_secs_f64());
                    call.waiting(wait);
                    tokio::time::sleep(wait).await;
                    waited += wait;
                    attempt += 1;
                    continue;
                }
                return Err(call.failed(super::rate_limited_error(&req.provider, hinted)));
            }
            if super::says_too_long(status, &detail) {
                return Err(call.failed(format!("{} (HTTP {status})", super::CONTEXT_EXCEEDED)));
            }
            // A model (or an organisation) that turns the web fetch down:
            // the same step without it, and never offered it again.
            if crate::web::native::turned_down_fetch(status, &detail) {
                if let Some(tools) = body["tools"].as_array_mut() {
                    let before = tools.len();
                    tools.retain(|t| t["name"] != "web_fetch");
                    if tools.len() < before {
                        tracing::info!("Chat: Claude turned the web fetch down; asking again without it");
                        crate::web::native::no_fetch(&req.model);
                        continue;
                    }
                }
            }
            return Err(call.failed(format!("HTTP {status}: {}", said(&detail))));
        };
        let limits = crate::ai::eval::limit_headers(resp.headers());
        // A server that answers whole though asked to stream: read whole.
        let whole_anyway = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|t| t.to_ascii_lowercase().contains("json"));
        let value = if streaming && !whole_anyway {
            let mut s = Stream::default();
            let mut sse = crate::chat::stream::SseParser::default();
            let mut resp = resp;
            loop {
                let bytes = match resp.chunk().await {
                    Ok(Some(b)) => b,
                    Ok(None) => break,
                    Err(e) => return Err(call.failed(format!("request failed: {e}"))),
                };
                for data in sse.feed(&bytes) {
                    if let Some(on) = on.as_deref_mut() {
                        s.push(&data, on);
                    }
                }
                if s.done {
                    break;
                }
            }
            for data in sse.finish() {
                if let Some(on) = on.as_deref_mut() {
                    s.push(&data, on);
                }
            }
            if let Some(e) = s.error.take() {
                return Err(call.failed(format!("Anthropic sent an error mid-way: {e}")));
            }
            s.whole()
        } else {
            resp.json::<Value>().await.map_err(|e| call.failed(format!("failed to parse response: {e}")))?
        };
        call.answered(&limits, &value);
        crate::ai::usage::record_response(&counted, &value, None, None);
        let blocks = value["content"].as_array().cloned().unwrap_or_default();
        // Its server tools paused their loop: asked to go on, from where it stopped.
        if value["stop_reason"] == "pause_turn" && pause < PAUSES {
            tracing::info!(blocks = blocks.len(), "Chat: Claude's server tools paused; asking it to go on");
            before.extend(blocks.iter().cloned());
            if let Some(msgs) = body["messages"].as_array_mut() {
                msgs.push(json!({ "role": "assistant", "content": blocks }));
            }
            continue;
        }
        before.extend(blocks);
        let whole = json!({ "content": before, "stop_reason": value["stop_reason"], "usage": value["usage"] });
        let message = crate::chat::think::tidy_message(message_of(&whole));
        // A whole answer's watcher hears it all at once (as a whole Chat
        // answer is): its tool calls, so its text is never taken for the
        // answer, then its text.
        if !streaming || whole_anyway {
            if let Some(on) = on.as_deref_mut() {
                for c in message["tool_calls"].as_array().into_iter().flatten() {
                    if let Some(name) = c["function"]["name"].as_str() {
                        on(Delta::ToolCall(name));
                    }
                }
                if let Some(text) = message["content"].as_str().filter(|t| !t.is_empty()) {
                    on(Delta::Content(&number_marks(text, &[])));
                }
            }
        }
        crate::chat::think::heard(crate::chat::think::reasoning_of(&message));
        tracing::info!(
            stop = value["stop_reason"].as_str().unwrap_or(""),
            calls = message["tool_calls"].as_array().map_or(0, Vec::len),
            searches = message["yap_web"]["searches"].as_array().map_or(0, Vec::len),
            pages = message["yap_web"]["fetches"].as_array().map_or(0, Vec::len),
            "Chat: Claude answered on the Messages API"
        );
        return Ok(message);
    }
    Err("Claude's web search kept pausing. Ask again in a moment.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(messages: Value, tools: Option<Value>) -> ChatRequest {
        ChatRequest::chat("https://api.anthropic.com/v1", "k", "claude-opus-5-5", "anthropic", 0.5, messages, tools.as_ref(), None)
    }

    fn function(name: &str) -> Value {
        json!({ "type": "function", "function": { "name": name, "description": "d", "parameters": { "type": "object", "properties": {} } } })
    }

    #[test]
    fn the_loops_messages_become_the_messages_apis() {
        let messages = json!([
            { "role": "system", "content": "You are Yap's Chat." },
            { "role": "user", "content": [{ "type": "text", "text": "What's on screen?" }, { "type": "image_url", "image_url": { "url": "data:image/png;base64,QUJD" } }] },
            { "role": "assistant", "content": null, "tool_calls": [{ "id": "c1", "type": "function", "function": { "name": "search_notes", "arguments": "{\"query\":\"plan\"}" } }] },
            { "role": "tool", "tool_call_id": "c1", "content": "{\"success\":true}" },
            { "role": "user", "content": "(A note from Yap, not the user.) Finish the document." },
            // Claude's own earlier blocks go back as they were.
            { "role": "assistant", "content": "x", "yap_anthropic": [{ "type": "thinking", "thinking": "", "signature": "sig" }, { "type": "text", "text": "Done." }] },
            { "role": "user", "content": "" }
        ]);
        let tools = json!([function("search_notes"), function("write_section")]);
        let server = [json!({ "type": "web_search_20260209", "name": "web_search", "max_uses": 3 })];
        let b = body(&req(messages, Some(tools)), &server, false);
        assert_eq!(b["system"], json!([{ "type": "text", "text": "You are Yap's Chat.", "cache_control": { "type": "ephemeral" } }]));
        assert_eq!(b["cache_control"], json!({ "type": "ephemeral" }));
        let m = b["messages"].as_array().unwrap();
        assert_eq!(m.len(), 4, "{m:?}");
        assert_eq!(m[0]["content"][1], json!({ "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "QUJD" } }));
        assert_eq!(m[1]["content"][0], json!({ "type": "tool_use", "id": "c1", "name": "search_notes", "input": { "query": "plan" } }));
        // The tool's result first, then Yap's note, in one user message.
        assert_eq!(m[2]["content"][0]["type"], "tool_result");
        assert_eq!(m[2]["content"][1]["text"], "(A note from Yap, not the user.) Finish the document.");
        assert_eq!(m[3]["content"][0]["signature"], "sig");
        // Function tools as the API takes them, then the server tools.
        let t = b["tools"].as_array().unwrap();
        assert_eq!(t[0], json!({ "name": "search_notes", "description": "d", "input_schema": { "type": "object", "properties": {} } }));
        assert_eq!(t[2]["type"], "web_search_20260209");
        // Opus 5.5 takes no temperature; nothing streamed.
        assert!(b.get("temperature").is_none() && b.get("stream").is_none());
        assert_eq!(b["max_tokens"], MAX_TOKENS);
    }

    #[tokio::test]
    async fn thinking_as_think_asks_with_its_summary_shown() {
        let ((), _) = crate::chat::think::scoped(true, async {
            let b = body(&req(json!([{ "role": "user", "content": "Plan it" }]), None), &[], true);
            assert_eq!(b["thinking"], json!({ "type": "adaptive", "display": "summarized" }));
            assert_eq!(b["output_config"], json!({ "effort": "high" }));
            assert_eq!(b["stream"], json!(true));
            assert_eq!(b["max_tokens"], MAX_TOKENS_STREAMED);
        })
        .await;
        // The last call, no tool to call: the tools stay, `none` chosen.
        let r = ChatRequest::chat("u", "k", "claude-opus-5-5", "anthropic", 0.5, json!([{ "role": "user", "content": "x" }]), Some(&json!([function("get_note")])), Some(json!("none")));
        assert_eq!(body(&r, &[], false)["tool_choice"], json!({ "type": "none" }));
    }

    /// Claude's answer with its search, a page it opened and its citations,
    /// in the shapes Anthropic's docs give (web search and web fetch tools,
    /// read 31 Oct 2026); a fictional game's wiki.
    #[test]
    fn claudes_searches_pages_and_citations_come_back_for_the_turn() {
        let v = json!({ "content": [
            { "type": "text", "text": "I'll look that up." },
            { "type": "server_tool_use", "id": "s1", "name": "web_search", "input": { "query": "Emberfall tidecaller levels" } },
            { "type": "web_search_tool_result", "tool_use_id": "s1", "content": [
                { "type": "web_search_result", "url": "https://emberfall.example/wiki/Tidecaller", "title": "Tidecaller - Emberfall Wiki", "encrypted_content": "E1", "page_age": null }
            ] },
            { "type": "server_tool_use", "id": "s2", "name": "web_fetch", "input": { "url": "https://emberfall.example/wiki/Tidecaller" } },
            { "type": "web_fetch_tool_result", "tool_use_id": "s2", "content": { "type": "web_fetch_result", "url": "https://emberfall.example/wiki/Tidecaller",
                "content": { "type": "document", "title": "Tidecaller - Emberfall Wiki", "source": { "type": "text", "media_type": "text/plain", "data": "Rain Bolt at 1." } } } },
            { "type": "server_tool_use", "id": "s3", "name": "web_fetch", "input": { "url": "https://forum.example/t/x" } },
            { "type": "web_fetch_tool_result", "tool_use_id": "s3", "content": { "type": "web_fetch_tool_result_error", "error_code": "url_not_accessible" } },
            { "type": "text", "text": "Rain Bolt comes at level 1.", "citations": [{ "type": "web_search_result_location", "url": "https://emberfall.example/wiki/Tidecaller", "title": "Tidecaller - Emberfall Wiki", "cited_text": "Rain Bolt (level 1)" }] },
            { "type": "text", "text": " Kindle at 2.", "citations": [{ "type": "char_location", "document_index": 0, "document_title": "Tidecaller - Emberfall Wiki", "cited_text": "Kindle (2)" }] },
            { "type": "tool_use", "id": "t1", "name": "create_document", "input": { "title": "Tidecaller guide", "sections": ["Levels"] } }
        ], "stop_reason": "tool_use" });
        let m = message_of(&v);
        assert_eq!(m["tool_calls"][0]["function"]["name"], "create_document");
        assert_eq!(m["tool_calls"][0]["id"], "t1");
        let w = &m["yap_web"];
        assert_eq!(w["searches"][0]["query"], "Emberfall tidecaller levels");
        assert_eq!(w["searches"][0]["results"][0]["url"], "https://emberfall.example/wiki/Tidecaller");
        assert_eq!(w["fetches"][0]["title"], "Tidecaller - Emberfall Wiki");
        assert_eq!(w["fetches"][1]["error"], "the page didn't open");
        // One page cited twice (by its address, then by its title): one mark each place.
        assert_eq!(w["cites"].as_array().unwrap().len(), 1);
        let text = m["content"].as_str().unwrap();
        assert!(!text.contains("look that up"), "{text}");
        assert_eq!(number_marks(text, &[4]), "Rain Bolt comes at level 1. [4] Kindle at 2. [4]");
        // Its own blocks, kept to send back.
        assert_eq!(m["yap_anthropic"].as_array().unwrap().len(), 10);
        // A search's error inside a 200: said, not a failure.
        let e = message_of(&json!({ "content": [
            { "type": "server_tool_use", "id": "s", "name": "web_search", "input": { "query": "q" } },
            { "type": "web_search_tool_result", "tool_use_id": "s", "content": { "type": "web_search_tool_result_error", "error_code": "too_many_requests" } },
            { "type": "text", "text": "I couldn't search." } ] }));
        assert_eq!(e["yap_web"]["searches"][0]["error"], "it's limiting how much Yap can search right now");
        assert_eq!(e["content"], "I couldn't search.");
    }

    #[test]
    fn a_streamed_answer_is_put_together_as_it_comes() {
        let events = [
            r#"{"type":"message_start","message":{"usage":{"input_tokens":900,"cache_read_input_tokens":800}}}"#,
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Levels first."}}"#,
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sg"}}"#,
            r#"{"type":"content_block_stop","index":0}"#,
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"server_tool_use","id":"s1","name":"web_search","input":{}}}"#,
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"query\":\"tide\"}"}}"#,
            r#"{"type":"content_block_stop","index":1}"#,
            r#"{"type":"content_block_start","index":2,"content_block":{"type":"web_search_tool_result","tool_use_id":"s1","content":[{"type":"web_search_result","url":"https://a.example/x","title":"A"}]}}"#,
            r#"{"type":"content_block_stop","index":2}"#,
            r#"{"type":"content_block_start","index":3,"content_block":{"type":"text","text":""}}"#,
            r#"{"type":"content_block_delta","index":3,"delta":{"type":"text_delta","text":"It's "}}"#,
            r#"{"type":"content_block_delta","index":3,"delta":{"type":"citations_delta","citation":{"type":"web_search_result_location","url":"https://a.example/x","title":"A","cited_text":"t"}}}"#,
            r#"{"type":"content_block_delta","index":3,"delta":{"type":"text_delta","text":"open."}}"#,
            r#"{"type":"content_block_stop","index":3}"#,
            r#"{"type":"content_block_start","index":4,"content_block":{"type":"tool_use","id":"t1","name":"write_section","input":{}}}"#,
            r#"{"type":"content_block_delta","index":4,"delta":{"type":"input_json_delta","partial_json":"{\"note_id\": 1, \"key\": \"Gear\", "}}"#,
            r#"{"type":"content_block_delta","index":4,"delta":{"type":"input_json_delta","partial_json":"\"markdown\": \"- A staff\"}"}}"#,
            r#"{"type":"content_block_stop","index":4}"#,
            r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":120}}"#,
            r#"{"type":"message_stop"}"#,
        ];
        let mut s = Stream::default();
        let mut heard: Vec<String> = Vec::new();
        let mut on = |d: Delta<'_>| {
            heard.push(match d {
                Delta::Content(t) => format!("text:{t}"),
                Delta::Reasoning(t) => format!("thinking:{t}"),
                Delta::ToolCall(n) => format!("call:{n}"),
                Delta::Server(n) => format!("server:{n}"),
                Delta::ServerInput { name, input } => format!("input:{name}:{input}"),
                Delta::ToolArgs { name, .. } => format!("args:{name}"),
                Delta::Restart => "restart".into(),
            })
        };
        for e in events {
            s.push(e, &mut on);
        }
        assert!(s.done);
        assert_eq!(
            heard,
            [
                "thinking:Levels first.",
                "server:web_search",
                r#"input:web_search:{"query":"tide"}"#,
                "text:It's ",
                "text:open.",
                "call:write_section",
                "args:write_section",
                "args:write_section"
            ]
        );
        let whole = s.whole();
        assert_eq!(whole["usage"]["output_tokens"], 120);
        assert_eq!(whole["usage"]["cache_read_input_tokens"], 800);
        let m = message_of(&whole);
        assert_eq!(m["reasoning_content"], "Levels first.");
        assert_eq!(m["yap_anthropic"][0]["signature"], "sg");
        assert_eq!(m["yap_web"]["searches"][0]["query"], "tide");
        assert_eq!(number_marks(m["content"].as_str().unwrap(), &[1]), "It's open. [1]");
        assert_eq!(serde_json::from_str::<Value>(m["tool_calls"][0]["function"]["arguments"].as_str().unwrap()).unwrap()["markdown"], "- A staff");
    }

    #[test]
    fn only_a_strong_claude_speaks_the_messages_api() {
        assert!(speaks_messages("anthropic", "claude-haiku-5-5"));
        assert!(speaks_messages("anthropic", "claude-opus-5-5"));
        assert!(!speaks_messages("openrouter", "anthropic/claude-opus-5-5"), "OpenRouter has its own API");
        assert!(!speaks_messages("ondevice", "Qwen_Qwen3.5-4B-Q4_K_M"));
    }
}

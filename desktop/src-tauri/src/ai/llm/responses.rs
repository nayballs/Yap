//! **The ChatGPT plan on OpenAI's Responses API** (Sign in with ChatGPT,
//! 2026-11-01): every call Yap makes on the provider "chatgpt" — Chat's
//! tool loop, cleanup, Commands, the meeting notes, the Voice Assistant —
//! comes here from `post_chat_request`, with the sign-in's access token
//! (`ai/chatgpt.rs`), never the key store's marker. The flow's rules
//! (OpenAI's "Preview limitations", read 1 Nov 2026):
//!
//! - **Every request** `store: false`, `stream: true`, the whole history in
//!   `input`; the system prompt as `instructions` (a later note in Yap's
//!   voice as a developer message: system items are refused). No
//!   `temperature`, `max_output_tokens` or the other fields the flow
//!   refuses ([`body`]).
//! - **Yap's tools in one namespace** (`yap`): the flow takes function tools
//!   only so. The model's calls come back as `function_call` items with
//!   that namespace; their results go back as `function_call_output`.
//! - **Reasoning across a turn's steps**: the response's output items
//!   (reasoning with its `encrypted_content`, the calls, the text) kept
//!   beside the OpenAI-shaped message (`yap_responses`, as Claude's
//!   `yap_anthropic`) and sent back whole with the next step, as stateless
//!   reasoning asks; earlier turns are their text.
//! - **Only `response.completed` is an answer**; `response.failed` (a plan's
//!   usage limit can arrive mid-stream), `response.incomplete` and `error`
//!   are said. The plan's error codes ([`plan_error`]): the usage limit
//!   with where to manage it, an account that isn't eligible, an
//!   unsupported field dropped and asked again, a refused token refreshed
//!   once. Never switched to another way of paying.
//!
//! Effort (`reasoning_effort`, `ai/effort.rs` treats the plan's models as
//! OpenAI's) becomes `reasoning: {effort, summary: "auto"}`; its summary
//! streams as thinking. A screenshot goes as `input_image`. Usage
//! (`response.completed`'s) goes to the AI usage tracker.

use std::time::Duration;

use serde_json::{json, Map, Value};

use super::{Delta, OnDelta};
use crate::privacy::net::Recorded as _;

/// The namespace Yap's function tools go in.
pub const NAMESPACE: &str = "yap";
/// The longest quiet spell in a stream before Yap gives up (a reasoning
/// model can think a long while before its first word).
const IDLE: Duration = Duration::from_secs(300);
/// A broken stream is asked again this many times.
const RETRIES: u32 = 1;

// ---- the request ----------------------------------------------------------------

/// A message's text, whatever its shape.
fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// A user message's content: text, or text and screenshots. Pure.
fn user_content(content: &Value) -> Value {
    match content {
        Value::Array(parts) => {
            let items: Vec<Value> = parts
                .iter()
                .filter_map(|p| match p["type"].as_str() {
                    Some("text") => p["text"].as_str().filter(|t| !t.trim().is_empty()).map(|t| json!({ "type": "input_text", "text": t })),
                    Some("image_url") => {
                        let url = p["image_url"]["url"].as_str().or(p["image_url"].as_str()).unwrap_or("");
                        (url.starts_with("data:") || url.starts_with("http")).then(|| json!({ "type": "input_image", "image_url": url }))
                    }
                    _ => None,
                })
                .collect();
            json!(items)
        }
        other => json!(text_of(other)),
    }
}

/// An output item as it goes back in the next step's `input`: without its
/// `id` and `status` (nothing is stored to refer to), everything else as it
/// came — a reasoning item's `encrypted_content`, a call's `namespace`. Pure.
fn replayed(item: &Value) -> Value {
    let mut item = item.clone();
    if let Some(o) = item.as_object_mut() {
        o.remove("id");
        o.remove("status");
    }
    item
}

/// **The Responses request for one call** (`messages` and `tools` as Chat
/// Completions has them; `extra`: `tool_choice`, `reasoning_effort`,
/// `response_format`). Pure.
pub fn body(model: &str, messages: &Value, tools: Option<&Value>, extra: &Map<String, Value>) -> Value {
    let mut instructions: Vec<String> = Vec::new();
    let mut input: Vec<Value> = Vec::new();
    for m in messages.as_array().into_iter().flatten() {
        match m["role"].as_str().unwrap_or("") {
            "system" | "developer" if input.is_empty() => instructions.push(text_of(&m["content"])),
            "system" | "developer" => input.push(json!({ "role": "developer", "content": text_of(&m["content"]) })),
            "user" => input.push(json!({ "role": "user", "content": user_content(&m["content"]) })),
            "tool" => input.push(json!({ "type": "function_call_output", "call_id": m["tool_call_id"], "output": text_of(&m["content"]) })),
            "assistant" => {
                // Its own items, as they came (reasoning first).
                if let Some(own) = m["yap_responses"].as_array().filter(|o| !o.is_empty()) {
                    input.extend(own.iter().map(replayed));
                    continue;
                }
                let text = text_of(&m["content"]);
                if !text.trim().is_empty() {
                    input.push(json!({ "role": "assistant", "content": text }));
                }
                for c in m["tool_calls"].as_array().into_iter().flatten() {
                    let args = match &c["function"]["arguments"] {
                        Value::String(s) => s.clone(),
                        v => v.to_string(),
                    };
                    input.push(json!({ "type": "function_call", "call_id": c["id"], "namespace": NAMESPACE, "name": c["function"]["name"], "arguments": args }));
                }
            }
            _ => {}
        }
    }
    let mut b = json!({ "model": model, "input": input, "store": false, "stream": true });
    if !instructions.is_empty() {
        b["instructions"] = json!(instructions.join("\n\n"));
    }
    let functions: Vec<Value> = tools
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|t| t["function"]["name"].is_string())
        .map(|t| {
            let f = &t["function"];
            let mut fun = json!({ "type": "function", "name": f["name"], "description": f["description"].as_str().unwrap_or("") });
            fun["parameters"] = if f["parameters"].is_object() { f["parameters"].clone() } else { json!({ "type": "object", "properties": {} }) };
            fun
        })
        .collect();
    if !functions.is_empty() {
        b["tools"] = json!([{ "type": "namespace", "name": NAMESPACE, "description": "Yap's tools: the person's notes, meetings, calendar, documents, the web and their PC.", "tools": functions }]);
        match extra.get("tool_choice") {
            Some(Value::String(c)) if matches!(c.as_str(), "none" | "auto" | "required") => b["tool_choice"] = json!(c),
            // A named tool: one of the namespace's, required.
            Some(Value::Object(_)) => b["tool_choice"] = json!("required"),
            _ => {}
        }
    }
    if let Some(effort) = extra.get("reasoning_effort").and_then(Value::as_str) {
        b["reasoning"] = json!({ "effort": effort, "summary": "auto" });
    }
    match extra.get("response_format") {
        Some(f) if f["type"] == "json_object" => b["text"] = json!({ "format": { "type": "json_object" } }),
        Some(f) if f["type"] == "json_schema" => {
            let s = &f["json_schema"];
            let mut format = json!({ "type": "json_schema", "name": s["name"].as_str().unwrap_or("answer"), "schema": s["schema"] });
            if let Some(strict) = s["strict"].as_bool() {
                format["strict"] = json!(strict);
            }
            b["text"] = json!({ "format": format });
        }
        _ => {}
    }
    b
}

// ---- the answer -----------------------------------------------------------------

/// **The response as Yap's calls read it**: an OpenAI-shaped message — its
/// text, its calls, its reasoning summary (`reasoning_content`) — with its
/// output items kept (`yap_responses`, sent back next step). Pure.
pub fn message_of(response: &Value) -> Value {
    let output: Vec<Value> = response["output"].as_array().cloned().unwrap_or_default();
    let mut text = String::new();
    let mut thinking = String::new();
    let mut calls: Vec<Value> = Vec::new();
    for item in &output {
        match item["type"].as_str().unwrap_or("") {
            "message" => {
                for part in item["content"].as_array().into_iter().flatten() {
                    if part["type"] == "output_text" {
                        text.push_str(part["text"].as_str().unwrap_or(""));
                    }
                }
            }
            "function_call" => calls.push(json!({
                "id": item["call_id"],
                "type": "function",
                "function": { "name": item["name"], "arguments": item["arguments"].as_str().unwrap_or("{}") },
            })),
            "reasoning" => {
                for s in item["summary"].as_array().into_iter().flatten() {
                    if let Some(t) = s["text"].as_str() {
                        if !thinking.is_empty() {
                            thinking.push_str("\n\n");
                        }
                        thinking.push_str(t);
                    }
                }
            }
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
    m["yap_responses"] = json!(output);
    m
}

// ---- streamed ---------------------------------------------------------------------

/// How a stream ended.
#[derive(Debug, PartialEq)]
pub enum End {
    /// `response.completed`: the whole response.
    Done(Value),
    /// `response.failed` or `error`: the error object.
    Failed(Value),
    /// `response.incomplete`: the response so far, and why.
    Incomplete(Value),
}

/// **A streamed response** put together as its events come, the watcher
/// told along the way.
#[derive(Default)]
pub struct Stream {
    /// Each function call's name and arguments so far, by output index.
    calls: Vec<(String, String)>,
    pub end: Option<End>,
}

impl Stream {
    /// One event's data: `on` told what shows.
    pub fn push(&mut self, data: &str, on: &mut dyn FnMut(Delta<'_>)) {
        let Ok(e) = serde_json::from_str::<Value>(data) else { return };
        let at = e["output_index"].as_u64().unwrap_or(0) as usize;
        match e["type"].as_str().unwrap_or("") {
            "response.output_item.added" => {
                let item = &e["item"];
                if item["type"] == "function_call" {
                    while self.calls.len() <= at {
                        self.calls.push(Default::default());
                    }
                    let name = item["name"].as_str().unwrap_or("").to_string();
                    on(Delta::ToolCall(&name));
                    self.calls[at] = (name, item["arguments"].as_str().unwrap_or("").to_string());
                }
            }
            "response.output_text.delta" => on(Delta::Content(e["delta"].as_str().unwrap_or(""))),
            "response.reasoning_summary_text.delta" => on(Delta::Reasoning(e["delta"].as_str().unwrap_or(""))),
            // Between two summary parts: a paragraph break.
            "response.reasoning_summary_part.added" if e["summary_index"].as_u64().is_some_and(|i| i > 0) => on(Delta::Reasoning("\n\n")),
            "response.function_call_arguments.delta" => {
                if let Some((name, so_far)) = self.calls.get_mut(at) {
                    so_far.push_str(e["delta"].as_str().unwrap_or(""));
                    if crate::chat::stream::LIVE_ARGS.contains(&name.as_str()) {
                        let (name, so_far) = (name.clone(), so_far.clone());
                        on(Delta::ToolArgs { name: &name, so_far: &so_far });
                    }
                }
            }
            "response.completed" => self.end = Some(End::Done(e["response"].clone())),
            "response.failed" => self.end = Some(End::Failed(e["response"]["error"].clone())),
            "response.incomplete" => self.end = Some(End::Incomplete(e["response"].clone())),
            "error" => self.end = Some(End::Failed(if e["error"].is_object() { e["error"].clone() } else { e.clone() })),
            _ => {}
        }
    }
}

// ---- errors -----------------------------------------------------------------------

/// What to do about a refusal.
#[derive(Debug, PartialEq)]
pub enum Refusal {
    /// Said to the person; never asked again.
    Say(String),
    /// The access token wasn't taken: refreshed, then asked once more.
    Reauth,
    /// Unavailable for now: asked again after a pause.
    Later,
    /// A field the flow doesn't take (`error.param`): the same request
    /// without it.
    Drop(String),
}

/// The plan's usage limit, said as OpenAI's UI rules have it.
pub fn usage_limit() -> String {
    format!("You've reached your ChatGPT plan's usage limit for now. Manage usage: {}", crate::ai::chatgpt::USAGE_URL)
}

/// **A refusal of the plan's**, by its code (OpenAI's "Errors and
/// recovery"): `status` and the error object (or a `{detail}` body). Pure.
pub fn plan_error(status: u16, error: &Value) -> Refusal {
    let e = if error["error"].is_object() { &error["error"] } else { error };
    let code = e["code"].as_str().or(e["type"].as_str()).unwrap_or("");
    let message = e["message"].as_str().or(error["detail"].as_str()).unwrap_or("").chars().take(200).collect::<String>();
    match code {
        "subscription_sharing_usage_limit_exceeded" => Refusal::Say(usage_limit()),
        "subscription_sharing_user_not_eligible" => {
            Refusal::Say("OpenAI says this ChatGPT account (or its workspace) can't be used by apps like Yap. Use an OpenAI API key instead, in Settings → Language Models.".into())
        }
        "subscription_sharing_usage_unavailable" | "subscription_sharing_user_unavailable" => Refusal::Later,
        "subscription_sharing_unsupported_capability" => match e["param"].as_str().filter(|p| !p.is_empty()) {
            Some(p) => Refusal::Drop(p.to_string()),
            None => Refusal::Say(format!("ChatGPT's plan doesn't support part of this request{}.", if message.is_empty() { String::new() } else { format!(" ({message})") })),
        },
        "subscription_sharing_invalid_user" => Refusal::Reauth,
        "subscription_sharing_route_not_supported" | "chatpass_v2_scope_not_authorized" | "chatpass_v2_invalid_authorization_context" => {
            Refusal::Say(format!("ChatGPT refused Yap's request ({code}). Sign out and in again in Settings → Language Models."))
        }
        _ if status == 401 => Refusal::Reauth,
        _ if status == 503 => Refusal::Later,
        _ if status == 403 => Refusal::Say(format!(
            "ChatGPT turned the request down{}.",
            if message.is_empty() { String::new() } else { format!(": {message}") }
        )),
        _ if code == "model_not_found" => Refusal::Say("That model isn't on your ChatGPT plan. Pick another in Chat's model picker.".into()),
        _ => Refusal::Say(format!("ChatGPT said: {}", if message.is_empty() { format!("error {status}") } else { message })),
    }
}

/// `body` without the field `param` names (`"reasoning"`, `"tool_choice"`,
/// `"text.format"`…): `false` when it isn't one Yap sent. Pure.
pub fn drop_param(body: &mut Value, param: &str) -> bool {
    let top = param.split(['.', '[']).next().unwrap_or("");
    // Never the request itself.
    if matches!(top, "" | "model" | "input" | "store" | "stream") {
        return false;
    }
    body.as_object_mut().and_then(|b| b.remove(top)).is_some()
}

// ---- the call ----------------------------------------------------------------------

/// **One call on the ChatGPT plan**: streamed always (the flow asks it),
/// `on` told as it comes when someone watches; the answer as
/// [`message_of`] makes it.
#[allow(clippy::too_many_arguments)]
pub(super) async fn post(
    model: &str,
    messages: &Value,
    tools: Option<&Value>,
    extra: &Map<String, Value>,
    timeout: Duration,
    rate_limit_patience: Duration,
    mut on: Option<OnDelta<'_>>,
) -> Result<Value, String> {
    let provider = crate::ai::chatgpt::PROVIDER;
    let base = crate::ai::chatgpt::oauth::api_base();
    let endpoint = format!("{base}/responses");
    let mut body = body(model, messages, tools, extra);
    crate::ai::eval::adjust_body(&mut body);
    // Fields an evaluation's overrides may add that the flow refuses.
    for k in ["temperature", "top_p", "max_output_tokens", "max_tokens"] {
        if let Some(b) = body.as_object_mut() {
            b.remove(k);
        }
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(crate::ai::eval::timeout(timeout.max(IDLE)))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))?;
    let counted = crate::ai::usage::Call { provider, model, base_url: &base, turn: true, feature: None };
    let call = crate::ai::eval::Call::start(provider, &endpoint, &body).await;
    let mut token = crate::ai::chatgpt::access_token().await.map_err(|e| call.failed(e))?;
    let (mut reauthed, mut later, mut dropped, mut broke) = (false, 0u32, 0u32, 0u32);
    let mut waited = Duration::ZERO;
    loop {
        call.send(&body);
        let resp = client
            .post(&endpoint)
            .bearer_auth(&token)
            .header(reqwest::header::ACCEPT, "text/event-stream")
            .json(&body)
            .send_for(crate::privacy::net::Purpose::Ai(provider.to_string()))
            .await
            .map_err(|e| call.failed(format!("request failed: {e}")))?;
        let status = resp.status().as_u16();
        let limits = crate::ai::eval::limit_headers(resp.headers());
        if !(200..300).contains(&status) {
            let header = |name: &str| resp.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
            let (retry_ms, retry_s) = (header("retry-after-ms"), header("retry-after"));
            let detail = resp.text().await.unwrap_or_default();
            call.refused(status, &limits, &detail);
            let error: Value = serde_json::from_str(&detail).unwrap_or(Value::Null);
            if super::says_too_long(status, &detail) {
                return Err(call.failed(format!("{} (HTTP {status})", super::CONTEXT_EXCEEDED)));
            }
            if let Some(why) = super::tool_call_rejection(status, &detail) {
                return Err(call.failed(format!("{}{why}", super::TOOL_CALL_REJECTED)));
            }
            let refusal = plan_error(status, &error);
            // A plain rate limit (not the plan's usage limit): waited out.
            if status == 429 && !matches!(&refusal, Refusal::Say(s) if *s == usage_limit()) {
                let hinted = super::rate_limit_wait(retry_ms.as_deref(), retry_s.as_deref(), &detail);
                let wait = hinted.unwrap_or(Duration::from_secs(2u64 << later.min(3))) + Duration::from_millis(250);
                if later < 4 && waited + wait <= rate_limit_patience {
                    call.waiting(wait);
                    tokio::time::sleep(wait).await;
                    waited += wait;
                    later += 1;
                    continue;
                }
                return Err(call.failed(super::rate_limited_error(provider, hinted)));
            }
            match refusal {
                Refusal::Reauth if !reauthed => {
                    reauthed = true;
                    token = crate::ai::chatgpt::refreshed_token(&token).await.map_err(|e| call.failed(e))?;
                    continue;
                }
                Refusal::Reauth => return Err(call.failed("ChatGPT didn't accept Yap's sign-in. Sign in again in Settings → Language Models.".into())),
                Refusal::Later if later < 2 => {
                    let wait = Duration::from_secs(2u64 << later);
                    call.waiting(wait);
                    tokio::time::sleep(wait).await;
                    later += 1;
                    continue;
                }
                Refusal::Later => return Err(call.failed("ChatGPT isn't available right now. Try again in a moment.".into())),
                Refusal::Drop(param) if dropped < 3 && drop_param(&mut body, &param) => {
                    tracing::info!(param = %param, "ChatGPT's plan doesn't take a field; asking again without it");
                    dropped += 1;
                    continue;
                }
                Refusal::Drop(param) => return Err(call.failed(format!("ChatGPT's plan doesn't support {param} in this request."))),
                Refusal::Say(s) => return Err(call.failed(s)),
            }
        }

        // The stream, read to its end (a server that answers whole: read so).
        let whole_anyway = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|t| t.to_ascii_lowercase().contains("json"));
        let end = if whole_anyway {
            let v: Value = resp.json().await.map_err(|e| call.failed(format!("failed to parse response: {e}")))?;
            Some(End::Done(v))
        } else {
            let mut s = Stream::default();
            let mut sse = crate::chat::stream::SseParser::default();
            let mut resp = resp;
            let mut quiet = |_: Delta<'_>| {};
            let mut broken = None;
            'read: loop {
                match resp.chunk().await {
                    Ok(Some(bytes)) => {
                        for data in sse.feed(&bytes) {
                            match on.as_deref_mut() {
                                Some(on) => s.push(&data, on),
                                None => s.push(&data, &mut quiet),
                            }
                            if s.end.is_some() {
                                break 'read;
                            }
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        broken = Some(e.to_string());
                        break;
                    }
                }
            }
            if s.end.is_none() {
                for data in sse.finish() {
                    match on.as_deref_mut() {
                        Some(on) => s.push(&data, on),
                        None => s.push(&data, &mut quiet),
                    }
                }
            }
            if s.end.is_none() {
                // Ended early: asked again (once), the watcher told to start over.
                let why = broken.unwrap_or_else(|| "it ended before the answer did".into());
                if broke < RETRIES {
                    tracing::info!("A ChatGPT stream broke ({why}); asking again");
                    broke += 1;
                    if let Some(on) = on.as_deref_mut() {
                        on(Delta::Restart);
                    }
                    continue;
                }
                return Err(call.failed(format!("ChatGPT's answer was cut off ({why}). Try again.")));
            }
            s.end
        };
        let response = match end {
            Some(End::Done(r)) => r,
            Some(End::Failed(e)) => {
                return Err(call.failed(match plan_error(0, &e) {
                    Refusal::Say(s) => s,
                    _ => format!("ChatGPT couldn't finish: {}", e["message"].as_str().unwrap_or("an error")),
                }))
            }
            Some(End::Incomplete(r)) => {
                let why = r["incomplete_details"]["reason"].as_str().unwrap_or("unknown").to_string();
                // What it wrote before it stopped, if anything, is the answer.
                if r["output"].as_array().is_some_and(|o| !o.is_empty()) {
                    tracing::info!(reason = %why, "ChatGPT stopped early; keeping what it wrote");
                    r
                } else {
                    return Err(call.failed(format!("ChatGPT stopped before answering ({why}).")));
                }
            }
            None => return Err(call.failed("ChatGPT didn't answer.".into())),
        };
        call.answered(&limits, &response);
        crate::ai::usage::record_response(&counted, &response, None, None);
        let message = crate::chat::think::tidy_message(message_of(&response));
        // A whole answer's watcher hears it all at once.
        if whole_anyway {
            if let Some(on) = on.as_deref_mut() {
                for c in message["tool_calls"].as_array().into_iter().flatten() {
                    if let Some(name) = c["function"]["name"].as_str() {
                        on(Delta::ToolCall(name));
                    }
                }
                if let Some(text) = message["content"].as_str().filter(|t| !t.is_empty()) {
                    on(Delta::Content(text));
                }
            }
        }
        crate::chat::think::heard(crate::chat::think::reasoning_of(&message));
        tracing::info!(calls = message["tool_calls"].as_array().map_or(0, Vec::len), "Chat: ChatGPT answered on the Responses API");
        return Ok(message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn function(name: &str) -> Value {
        json!({ "type": "function", "function": { "name": name, "description": "d", "parameters": { "type": "object", "properties": { "q": { "type": "string" } } } } })
    }

    #[test]
    fn a_chat_step_becomes_a_stateless_streamed_request_with_yaps_tools_in_a_namespace() {
        let messages = json!([
            { "role": "system", "content": "You are Yap's Chat." },
            { "role": "user", "content": "What did Sam say about the launch?" },
            { "role": "assistant", "content": "Earlier answer." },
            { "role": "system", "content": "(From Yap) The note changed." },
            { "role": "user", "content": [{ "type": "text", "text": "And this?" }, { "type": "image_url", "image_url": { "url": "data:image/png;base64,UElD" } }] },
        ]);
        let mut extra = Map::new();
        extra.insert("tool_choice".into(), json!("auto"));
        extra.insert("reasoning_effort".into(), json!("low"));
        let b = body("gpt-6.1-sol", &messages, Some(&json!([function("search_notes")])), &extra);
        assert_eq!((b["store"].clone(), b["stream"].clone()), (json!(false), json!(true)));
        assert_eq!(b["instructions"], "You are Yap's Chat.");
        let input = b["input"].as_array().unwrap();
        assert_eq!(input[0], json!({ "role": "user", "content": "What did Sam say about the launch?" }));
        assert_eq!(input[1], json!({ "role": "assistant", "content": "Earlier answer." }));
        // A later note in Yap's voice: a developer message (system items are refused).
        assert_eq!(input[2], json!({ "role": "developer", "content": "(From Yap) The note changed." }));
        assert_eq!(input[3]["content"][1], json!({ "type": "input_image", "image_url": "data:image/png;base64,UElD" }));
        assert_eq!(b["tools"][0]["type"], "namespace");
        assert_eq!(b["tools"][0]["name"], "yap");
        assert_eq!(b["tools"][0]["tools"][0]["name"], "search_notes");
        assert_eq!(b["tool_choice"], "auto");
        assert_eq!(b["reasoning"], json!({ "effort": "low", "summary": "auto" }));
        // Never a field the flow refuses.
        for refused in ["temperature", "max_output_tokens", "max_tokens", "previous_response_id", "metadata", "user"] {
            assert!(b.get(refused).is_none(), "{refused}");
        }
    }

    #[test]
    fn a_tool_step_sends_back_its_own_items_and_the_results() {
        // The step before: reasoning, then a namespaced call (as the API sent them).
        let output = json!([
            { "id": "rs_1", "type": "reasoning", "summary": [{ "type": "summary_text", "text": "Look in the notes." }], "encrypted_content": "gAAAA-opaque" },
            { "id": "fc_1", "type": "function_call", "status": "completed", "namespace": "yap", "name": "search_notes", "call_id": "call_1", "arguments": "{\"q\":\"launch\"}" },
        ]);
        let step = message_of(&json!({ "output": output, "status": "completed" }));
        assert_eq!(step["tool_calls"][0]["id"], "call_1");
        assert_eq!(step["tool_calls"][0]["function"]["name"], "search_notes");
        assert_eq!(step["reasoning_content"], "Look in the notes.");
        assert!(step["content"].is_null());
        let messages = json!([
            { "role": "system", "content": "s" },
            { "role": "user", "content": "q" },
            step,
            { "role": "tool", "tool_call_id": "call_1", "content": "Sam: launch moves to May." },
        ]);
        let b = body("gpt-6.1-sol", &messages, Some(&json!([function("search_notes")])), &Map::new());
        let input = b["input"].as_array().unwrap();
        // Reasoning first, whole (its encrypted content), with no id to look up.
        assert_eq!(input[1], json!({ "type": "reasoning", "summary": [{ "type": "summary_text", "text": "Look in the notes." }], "encrypted_content": "gAAAA-opaque" }));
        assert_eq!(input[2], json!({ "type": "function_call", "namespace": "yap", "name": "search_notes", "call_id": "call_1", "arguments": "{\"q\":\"launch\"}" }));
        assert_eq!(input[3], json!({ "type": "function_call_output", "call_id": "call_1", "output": "Sam: launch moves to May." }));
    }

    #[test]
    fn another_models_tool_calls_go_as_namespaced_calls() {
        let messages = json!([
            { "role": "user", "content": "q" },
            { "role": "assistant", "content": null, "tool_calls": [{ "id": "c9", "type": "function", "function": { "name": "list_notes", "arguments": "{}" } }] },
            { "role": "tool", "tool_call_id": "c9", "content": "[]" },
        ]);
        let b = body("m", &messages, None, &Map::new());
        assert_eq!(b["input"][1], json!({ "type": "function_call", "call_id": "c9", "namespace": "yap", "name": "list_notes", "arguments": "{}" }));
        assert!(b.get("tools").is_none() && b.get("instructions").is_none());
    }

    #[test]
    fn a_json_answer_is_asked_in_the_responses_form() {
        let mut extra = Map::new();
        extra.insert("response_format".into(), json!({ "type": "json_schema", "json_schema": { "name": "memory_item", "schema": { "type": "object" }, "strict": true } }));
        let b = body("m", &json!([{ "role": "user", "content": "q" }]), None, &extra);
        assert_eq!(b["text"], json!({ "format": { "type": "json_schema", "name": "memory_item", "schema": { "type": "object" }, "strict": true } }));
        extra.insert("response_format".into(), json!({ "type": "json_object" }));
        assert_eq!(body("m", &json!([]), None, &extra)["text"]["format"]["type"], "json_object");
    }

    #[test]
    fn a_stream_tells_the_watcher_and_ends_only_on_completed() {
        let events = [
            json!({ "type": "response.created", "response": { "status": "in_progress" } }),
            json!({ "type": "response.reasoning_summary_text.delta", "delta": "Thinking" }),
            json!({ "type": "response.output_item.added", "output_index": 1, "item": { "type": "function_call", "name": "write_section", "arguments": "" } }),
            json!({ "type": "response.function_call_arguments.delta", "output_index": 1, "delta": "{\"markdown\":\"# A" }),
            json!({ "type": "response.output_text.delta", "output_index": 2, "delta": "Done." }),
        ];
        let mut s = Stream::default();
        let mut seen: Vec<String> = Vec::new();
        for e in &events {
            s.push(&e.to_string(), &mut |d| {
                seen.push(match d {
                    Delta::Content(t) => format!("text:{t}"),
                    Delta::Reasoning(t) => format!("think:{t}"),
                    Delta::ToolCall(n) => format!("call:{n}"),
                    Delta::ToolArgs { name, so_far } => format!("args:{name}:{so_far}"),
                    _ => "other".into(),
                })
            });
        }
        assert!(s.end.is_none(), "no answer before response.completed");
        assert_eq!(seen[0], "think:Thinking");
        assert_eq!(seen[1], "call:write_section");
        assert!(seen.contains(&"text:Done.".to_string()));
        s.push(&json!({ "type": "response.completed", "response": { "output": [], "usage": { "input_tokens": 10, "output_tokens": 2 } } }).to_string(), &mut |_| {});
        assert!(matches!(s.end, Some(End::Done(_))));
        // A usage limit mid-stream.
        let mut s = Stream::default();
        s.push(&json!({ "type": "response.failed", "response": { "error": { "code": "subscription_sharing_usage_limit_exceeded", "message": "limit" } } }).to_string(), &mut |_| {});
        let Some(End::Failed(e)) = &s.end else { panic!() };
        assert_eq!(plan_error(0, e), Refusal::Say(usage_limit()));
    }

    #[test]
    fn the_plans_errors_are_said_or_recovered() {
        let err = |code: &str, param: Option<&str>| json!({ "error": { "code": code, "message": "m", "param": param } });
        assert!(matches!(plan_error(429, &err("subscription_sharing_usage_limit_exceeded", None)), Refusal::Say(s) if s.contains("chatgpt.com/settings/usage")));
        assert!(matches!(plan_error(403, &err("subscription_sharing_user_not_eligible", None)), Refusal::Say(s) if s.contains("API key")));
        assert_eq!(plan_error(503, &err("subscription_sharing_usage_unavailable", None)), Refusal::Later);
        assert_eq!(plan_error(400, &err("subscription_sharing_unsupported_capability", Some("reasoning.summary"))), Refusal::Drop("reasoning.summary".into()));
        assert_eq!(plan_error(401, &err("subscription_sharing_invalid_user", None)), Refusal::Reauth);
        // Admission's bodies have no error object.
        assert_eq!(plan_error(401, &json!({ "detail": "no" })), Refusal::Reauth);
        assert_eq!(plan_error(503, &json!({ "detail": "routing" })), Refusal::Later);
        assert!(matches!(plan_error(403, &json!({ "detail": "region" })), Refusal::Say(s) if s.contains("region")));
        let mut b = json!({ "model": "m", "input": [], "reasoning": { "effort": "low" }, "text": {} });
        assert!(drop_param(&mut b, "reasoning.summary") && b.get("reasoning").is_none());
        assert!(drop_param(&mut b, "text.format"));
        assert!(!drop_param(&mut b, "input[0]") && !drop_param(&mut b, "store"));
    }
}

//! **One question, one answer on the plan**: every call on the Chat model
//! that isn't a Chat turn — memory learning, a long chat's summary, the
//! Notes view's and the notepad's Ask, Think, the apps' plan, Yap's own
//! fast answers, a snip's question, the Voice Assistant's look at the
//! screen — arrives at `llm::post_chat_request` as an OpenAI-shaped
//! request and comes here (as the ChatGPT plan's go to `llm/responses.rs`).
//!
//! - **Lean**: `--tools ""` (no tools at all), no MCP server
//!   (`--strict-mcp-config` with none), `--setting-sources ""` (none of the
//!   person's settings, hooks or CLAUDE.md), in an empty folder of Yap's.
//! - **Not saved**: `--no-session-persistence` (`Spec::persist`): nothing
//!   resumes a one-shot call, so Claude Code keeps no transcript of it in
//!   its own config folder.
//! - **The request flattened** ([`flatten`]): its system messages are the
//!   prompt (`--system-prompt-file`, in place of Claude Code's own coding
//!   agent's), the rest one message — the earlier turns in a block, then
//!   the last question — with that question's pictures as image blocks.
//!   Tools a caller offers aren't run: the answer is text.
//! - **Streamed** to whoever watches (`Delta::Content`, `Reasoning`).
//! - **Counted** (`usage::record_response`, with Claude Code's own estimate
//!   of the API price: on a plan, what it saved).
//! - **Problems** in plain words (`Problem::message`): never an answer.

use std::time::Duration;

use serde_json::{json, Value};

use crate::ai::claude_code::events::{Event, StreamKind};
use crate::ai::claude_code::session::{self, Image, Pay, Session, Spec};
use crate::ai::llm::{Delta, OnDelta};

/// The shortest a one-shot call may take (Claude Code starts first).
const AT_LEAST: Duration = Duration::from_secs(60);

/// A request, as Claude Code takes it.
#[derive(Debug, Default, PartialEq)]
pub struct Flat {
    /// Its system prompt (in place of Claude Code's own).
    pub system: String,
    /// The one message: the earlier turns, then the question.
    pub prompt: String,
    pub images: Vec<Image>,
}

/// A message's text, whatever its content's shape. Pure.
fn text_of(content: &Value) -> String {
    crate::snip::turn::text_of(content).unwrap_or_default()
}

/// The pictures in a message's content (`image_url` data URLs). Pure.
pub fn images_of(content: &Value) -> Vec<Image> {
    use base64::Engine as _;
    content
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["type"] == "image_url")
        .filter_map(|p| p["image_url"]["url"].as_str())
        .filter_map(|url| {
            let (head, data) = url.split_once(',')?;
            let media = head.strip_prefix("data:")?.strip_suffix(";base64")?;
            if !matches!(media, "image/png" | "image/jpeg" | "image/gif" | "image/webp") {
                return None;
            }
            // Checked as base64 (never sent on if it isn't).
            base64::engine::general_purpose::STANDARD.decode(data.get(..data.len().min(64)).map(|s| &s[..s.len() - s.len() % 4]).unwrap_or("")).ok()?;
            Some(Image { media_type: media.to_string(), base64: data.to_string() })
        })
        .collect()
}

/// **An OpenAI-shaped request as one Claude Code question**: the first
/// system message its prompt; everything else before the last user message
/// the earlier turns, in a block (a tool loop's calls and results said in
/// words); the last user message the question, with its pictures; what
/// came after it (a tool step's) after it. Pure.
pub fn flatten(messages: &Value) -> Flat {
    let list: Vec<&Value> = messages.as_array().map(|a| a.iter().collect()).unwrap_or_default();
    let mut flat = Flat::default();
    let mut rest: Vec<&Value> = list.clone();
    if let Some(first) = list.first().filter(|m| m["role"] == "system") {
        flat.system = text_of(&first["content"]).trim().to_string();
        rest.remove(0);
    }
    let last_user = rest.iter().rposition(|m| m["role"] == "user");
    let said = |m: &Value| -> Option<String> {
        let text = text_of(&m["content"]);
        let text = text.trim();
        match m["role"].as_str().unwrap_or("") {
            "user" => (!text.is_empty()).then(|| format!("User: {text}")),
            "assistant" => {
                let calls: Vec<String> = m["tool_calls"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|c| c["function"]["name"].as_str().map(|n| format!("(called {n} with {})", c["function"]["arguments"].as_str().unwrap_or("{}"))))
                    .collect();
                let both = [text.to_string(), calls.join(" ")].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
                (!both.is_empty()).then(|| format!("Assistant: {both}"))
            }
            "tool" => (!text.is_empty()).then(|| format!("Tool result: {text}")),
            "system" => (!text.is_empty()).then(|| format!("(Yap) {text}")),
            _ => None,
        }
    };
    let Some(q) = last_user else {
        flat.prompt = rest.iter().filter_map(|m| said(m)).collect::<Vec<_>>().join("\n\n");
        return flat;
    };
    let before: Vec<String> = rest[..q].iter().filter_map(|m| said(m)).collect();
    let after: Vec<String> = rest[q + 1..].iter().filter_map(|m| said(m)).collect();
    let question = text_of(&rest[q]["content"]);
    flat.images = images_of(&rest[q]["content"]);
    let mut parts: Vec<String> = Vec::new();
    if !before.is_empty() {
        parts.push(format!("<earlier_in_this_conversation>\n{}\n</earlier_in_this_conversation>", before.join("\n\n")));
    }
    parts.push(question.trim().to_string());
    if !after.is_empty() {
        parts.push(after.join("\n\n"));
    }
    flat.prompt = parts.join("\n\n");
    flat
}

/// The empty folder plan calls run in (no project's settings, CLAUDE.md or
/// MCP servers in it): `<data>/claude/<what>/`, made if it isn't there.
pub fn folder(what: &str) -> Result<std::path::PathBuf, String> {
    let dir = folder_path(what);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't make a folder for Claude Code: {e}"))?;
    Ok(dir)
}

/// [`folder`]'s path, made or not (`chat`: where Claude Code files a plan
/// chat's transcripts, `claude_plan/sessions.rs`).
pub fn folder_path(what: &str) -> std::path::PathBuf {
    crate::config::data_dir().join("claude").join(what)
}

/// The effort a call to `model` runs at, as Claude Code takes it
/// (`--effort`), from what's chosen for it in Chat (`ai/effort.rs`).
pub fn effort_for(model: &str) -> Option<session::Effort> {
    crate::ai::effort::for_call(super::PROVIDER, model).and_then(|(level, _)| session::Effort::parse(level.id()))
}

/// The answer's counts, as `usage::record_response` reads them, with
/// Claude Code's own estimate of the API price.
pub fn usage_value(u: &crate::ai::claude_code::events::Usage, cost_usd: f64) -> Value {
    json!({
        "usage": {
            "input_tokens": u.input_tokens,
            "output_tokens": u.output_tokens,
            "cache_read_input_tokens": u.cache_read_input_tokens,
            "cache_creation_input_tokens": u.cache_creation_input_tokens,
        },
        "total_cost_usd": cost_usd,
    })
}

/// **A one-shot call's run**, in `cwd`: on the plan, `model`, no tools, no
/// MCP server, none of the person's settings, `flat`'s system prompt in
/// place of Claude Code's, and not saved (nothing resumes it). Pure.
pub(crate) fn ask_spec(cwd: std::path::PathBuf, model: &str, flat: &Flat) -> Spec {
    let mut spec = Spec::new(cwd, Pay::Plan);
    spec.model = Some(model.to_string()).filter(|m| !m.trim().is_empty());
    spec.tools = Some(Vec::new());
    spec.strict_mcp_config = true;
    spec.setting_sources = Some(Vec::new());
    // Nothing resumes it: no transcript in Claude Code's own folder.
    spec.persist = false;
    spec.system_prompt = Some(if flat.system.is_empty() { "You are a helpful assistant.".to_string() } else { flat.system.clone() });
    spec
}

/// **One call on the plan**, as `llm::post_chat_request` makes it: the
/// answer as an OpenAI-shaped message (`{role, content}`).
pub(crate) async fn post(model: &str, messages: &Value, timeout: Duration, on: Option<OnDelta<'_>>) -> Result<Value, String> {
    let flat = flatten(messages);
    let mut spec = ask_spec(folder("ask")?, model, &flat);
    spec.effort = effort_for(model);
    let images = if crate::snip::vision::reads_images(super::PROVIDER, model) == Some(false) { Vec::new() } else { flat.images.clone() };
    let (session, mut rx) = Session::start(spec)?;
    session.send_user(&flat.prompt, &images)?;
    session.end();
    let mut quiet = |_: Delta<'_>| {};
    let on: OnDelta<'_> = match on {
        Some(on) => on,
        None => &mut quiet,
    };
    let mut shown = String::new();
    let waited = tokio::time::timeout(timeout.max(AT_LEAST), async {
        while let Some(ev) = rx.recv().await {
            match ev {
                Event::Stream(s) if s.parent_tool_use_id.is_none() => match s.kind {
                    StreamKind::Text { text, .. } => {
                        shown.push_str(&text);
                        on(Delta::Content(&text));
                    }
                    StreamKind::Thinking { thinking, .. } => on(Delta::Reasoning(&thinking)),
                    _ => {}
                },
                Event::Problem(p) => {
                    if matches!(p, crate::ai::claude_code::events::Problem::NotSignedIn) {
                        crate::ai::claude_code::cli::forget();
                    }
                    return Err(p.message());
                }
                Event::Result(r) if r.is_error => {
                    let why = r.errors.first().cloned().filter(|e| !e.is_empty()).unwrap_or(r.text);
                    return Err(if why.trim().is_empty() { "Claude Code couldn't answer.".to_string() } else { format!("Claude Code couldn't answer: {}", why.trim()) });
                }
                Event::Result(r) => return Ok(r),
                Event::Exited(_) => break,
                _ => {}
            }
        }
        Err("Claude Code stopped before it answered.".to_string())
    })
    .await;
    let result = match waited {
        Ok(r) => r,
        Err(_) => Err("Claude Code took too long to answer.".to_string()),
    };
    let r = match result {
        Ok(r) => r,
        Err(e) => {
            session.kill();
            tracing::info!("Claude plan: a one-shot call didn't answer");
            return Err(e);
        }
    };
    let counted = crate::ai::usage::Call { provider: super::PROVIDER, model, base_url: super::BASE_URL, turn: true, feature: None };
    crate::ai::usage::record_response(&counted, &usage_value(&r.usage, r.cost_usd), None, None);
    // The answer is the result's words (what streamed, if it said none).
    let text = if r.text.trim().is_empty() { shown } else { r.text };
    tracing::info!(chars = text.chars().count(), "Claude plan: a one-shot call answered");
    Ok(json!({ "role": "assistant", "content": text }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_becomes_one_prompt_its_system_prompt_and_the_questions_pictures() {
        let messages = json!([
            { "role": "system", "content": "You summarise chats." },
            { "role": "user", "content": "What did Sam say about the launch?" },
            { "role": "assistant", "content": "Friday.", "tool_calls": [{ "function": { "name": "search_notes", "arguments": "{\"query\":\"launch\"}" } }] },
            { "role": "tool", "content": "Launch: Friday." },
            { "role": "system", "content": "The note changed." },
            { "role": "user", "content": [{ "type": "image_url", "image_url": { "url": "data:image/png;base64,iVBORw0KGgo=" } }, { "type": "text", "text": "And this one?" }] },
        ]);
        let f = flatten(&messages);
        assert_eq!(f.system, "You summarise chats.");
        assert_eq!(
            f.prompt,
            "<earlier_in_this_conversation>\nUser: What did Sam say about the launch?\n\nAssistant: Friday. (called search_notes with {\"query\":\"launch\"})\n\nTool result: Launch: Friday.\n\n(Yap) The note changed.\n</earlier_in_this_conversation>\n\nAnd this one?"
        );
        assert_eq!(f.images.len(), 1);
        assert_eq!(f.images[0].media_type, "image/png");
        // One question alone: just it.
        let f = flatten(&json!([{ "role": "user", "content": "Title this chat." }]));
        assert_eq!((f.system.as_str(), f.prompt.as_str()), ("", "Title this chat."));
        assert!(f.images.is_empty());
        // A tool step after the question goes after it.
        let f = flatten(&json!([{ "role": "user", "content": "q" }, { "role": "assistant", "content": null, "tool_calls": [{ "function": { "name": "web_search", "arguments": "{}" } }] }, { "role": "tool", "content": "r" }]));
        assert_eq!(f.prompt, "q\n\nAssistant: (called web_search with {})\n\nTool result: r");
    }

    #[test]
    fn a_one_shot_call_runs_lean_and_leaves_no_transcript() {
        let flat = flatten(&json!([{ "role": "system", "content": "Title chats." }, { "role": "user", "content": "Title this chat." }]));
        let spec = ask_spec(std::env::temp_dir(), "claude-haiku-5-5", &flat);
        let a = session::args(&spec, &session::Files { own_prompt: Some("own.txt".into()), ..Default::default() });
        let after = |flag: &str| a.iter().position(|x| x == flag).and_then(|i| a.get(i + 1)).map(String::as_str);
        assert!(a.contains(&"--no-session-persistence".to_string()), "nothing resumes it: no transcript kept");
        assert_eq!(after("--tools"), Some(""));
        assert_eq!(after("--setting-sources"), Some(""));
        assert!(a.contains(&"--strict-mcp-config".to_string()) && !a.contains(&"--mcp-config".to_string()));
        assert_eq!(after("--model"), Some("claude-haiku-5-5"));
        assert_eq!(after("--system-prompt-file"), Some("own.txt"));
        assert_eq!(spec.system_prompt.as_deref(), Some("Title chats."));
        assert_eq!(spec.pay, Pay::Plan);
        assert!(spec.env.is_empty(), "no run key: no endpoint");
    }

    #[test]
    fn only_real_pictures_go() {
        let parts = json!([
            { "type": "image_url", "image_url": { "url": "data:image/jpeg;base64,/9j/4AAQ" } },
            { "type": "image_url", "image_url": { "url": "https://example.com/x.png" } },
            { "type": "image_url", "image_url": { "url": "data:text/html;base64,PGh0bWw+" } },
            { "type": "text", "text": "q" },
        ]);
        let imgs = images_of(&parts);
        assert_eq!(imgs.len(), 1);
        assert_eq!(imgs[0].media_type, "image/jpeg");
    }

    #[test]
    fn its_counts_carry_claude_codes_estimate() {
        let u = crate::ai::claude_code::events::Usage { input_tokens: 10, output_tokens: 5, cache_read_input_tokens: 100, ..Default::default() };
        let v = usage_value(&u, 0.0042);
        let r = crate::ai::usage::parse(&v);
        assert_eq!((r.prompt, r.answer), (Some(110), Some(5)));
        assert_eq!(v["total_cost_usd"], 0.0042);
    }
}

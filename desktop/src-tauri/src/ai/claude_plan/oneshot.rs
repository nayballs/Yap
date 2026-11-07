//! **One question, one answer on the plan**: every plan call that isn't a
//! Chat turn — a dictation's cleanup, a Command, a saved transform, a
//! meeting's summary and the notepad's helpers (7 Nov 2026: every scope),
//! and on the Chat model memory learning, a long chat's summary, the Notes
//! view's and the notepad's Ask, Think, the apps' plan, Yap's own fast
//! answers, a snip's question, the Voice Assistant's look at the screen —
//! arrives at `llm::post_chat_request` as an OpenAI-shaped request and comes
//! here (as the ChatGPT plan's go to `llm/responses.rs`). The endpoint's
//! URL says which scope it's for (`claude_plan::scope_of_url`).
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
//!   The parts of a system prompt that change with every call (the
//!   dictionary words a dictation says, a rewrite's selection:
//!   [`per_call_parts`]) go just before the question instead, so a warm
//!   spare made for the scope's usual prompt fits. Tools a caller offers
//!   aren't run: the answer is text.
//! - **On a warm spare** when the scope has one waiting for this prompt
//!   (`spare.rs`; Dictation Cleanup, Commands, Note Formatting), else a
//!   Claude Code started now; a new spare starts behind it either way.
//! - **Effort** ([`effort_for_scope`]): Chat's choice for a Chat call; the
//!   lowest the model takes for a cleanup or a Command (a dictation can't
//!   wait for thinking); the model's own for Note Formatting.
//! - **Streamed** to whoever watches (`Delta::Content`, `Reasoning`).
//! - **Counted** (`usage::record_response`, with Claude Code's own estimate
//!   of the API price: on a plan, what it saved), under the call's feature.
//! - **The plan's limits** move with the call's own `rate_limit_event`s,
//!   and at most once a minute the call's own process is asked where they
//!   stand too (`get_usage` beside the message: no other process, no model
//!   call; `usage::due_with_call`).
//! - **Problems** in plain words (`Problem::message`): never an answer.
//!   The plan's own ([`Failure::Plan`]: signed out, its limit, paying
//!   another way, Claude Code missing or not starting) may go to the key
//!   once (`claude_plan::fallback_key`, in `llm::post_chat_request`).

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
    let mut moved: Vec<String> = Vec::new();
    if let Some(first) = list.first().filter(|m| m["role"] == "system") {
        let (stable, per_call) = per_call_parts(text_of(&first["content"]).trim());
        flat.system = stable;
        moved = per_call;
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
        flat.prompt = moved.into_iter().chain(rest.iter().filter_map(|m| said(m))).collect::<Vec<_>>().join("\n\n");
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
    // This call's own instructions, just before its question.
    parts.extend(moved);
    parts.push(question.trim().to_string());
    if !after.is_empty() {
        parts.push(after.join("\n\n"));
    }
    flat.prompt = parts.join("\n\n");
    flat
}

/// **A system prompt's parts that change with every call**, taken out: the
/// dictionary words this dictation says (`llm::DICTIONARY_LINE`'s
/// paragraph) and a rewrite's selection (`llm::SELECTION_LINE` to the end).
/// The rest is the same from call to call into one kind of app, so a warm
/// spare started with it fits the next call; these go in the call's message
/// instead (the model reads the same words). Pure.
pub fn per_call_parts(system: &str) -> (String, Vec<String>) {
    let mut stable = system.to_string();
    let mut moved: Vec<String> = Vec::new();
    // A rewrite's selection: the rest of the prompt (it may hold blank lines).
    if let Some(at) = stable.find(&format!("\n\n{}", crate::ai::llm::SELECTION_LINE)) {
        moved.push(stable[at + 2..].trim().to_string());
        stable.truncate(at);
    }
    // The dictionary words said: their one paragraph.
    if let Some(at) = stable.find(&format!("\n\n{}", crate::ai::llm::DICTIONARY_LINE)) {
        let end = stable[at + 2..].find("\n\n").map_or(stable.len(), |e| at + 2 + e);
        moved.insert(0, stable[at + 2..end].trim().to_string());
        stable.replace_range(at..end, "");
    }
    (stable, moved)
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

/// **The effort a `scope`'s call to `model` runs at**: Chat's choice for a
/// Chat call ([`effort_for`]); the lowest level the model takes for a
/// dictation's cleanup and a Command (no thinking a dictation waits for;
/// none sent for a model without levels); the model's own default for Note
/// Formatting (no `--effort`). Only Chat has an effort setting.
pub fn effort_for_scope(scope: super::Scope, model: &str) -> Option<session::Effort> {
    match scope {
        super::Scope::Chat => effort_for(model),
        super::Scope::Cleanup | super::Scope::Commands => crate::ai::effort::caps(super::PROVIDER, model)
            .and_then(|c| c.levels.first().copied())
            .and_then(|level| session::Effort::parse(level.id())),
        super::Scope::Notes => None,
    }
}

/// **A call's run** (a spare's, or one started for the call): the lean
/// one-shot flags ([`ask_spec`]) on `model` at `effort`, with `system` in
/// place of Claude Code's own prompt, in Yap's empty folder.
pub fn call_spec(model: &str, effort: Option<session::Effort>, system: &str) -> Result<Spec, String> {
    let flat = Flat { system: system.to_string(), ..Default::default() };
    let mut spec = ask_spec(folder("ask")?, model, &flat);
    spec.effort = effort;
    Ok(spec)
}

/// Why a plan call didn't answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Failure {
    /// **The plan couldn't answer**: Claude Code signed out, the plan's
    /// limit, paying another way, Claude Code missing or not starting, or
    /// ending before it answered. A scope's fallback to the key takes it
    /// (`claude_plan::fallback_key`).
    Plan(String),
    /// Anything else (too slow, an error the model's request got): as it is.
    Other(String),
}

impl Failure {
    pub fn message(self) -> String {
        match self {
            Failure::Plan(m) | Failure::Other(m) => m,
        }
    }
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

/// A Claude Code started for this call, the prompt sent.
fn cold(model: &str, effort: Option<session::Effort>, flat: &Flat, images: &[Image]) -> Result<(Session, tokio::sync::mpsc::UnboundedReceiver<Event>), Failure> {
    let spec = call_spec(model, effort, &flat.system).map_err(Failure::Plan)?;
    let (session, rx) = Session::start(spec).map_err(Failure::Plan)?;
    session.send_user(&flat.prompt, images).map_err(Failure::Plan)?;
    Ok((session, rx))
}

/// **One call on the plan**, as `llm::post_chat_request` makes it, for the
/// scope `base_url` names: the answer as an OpenAI-shaped message
/// (`{role, content}`).
pub(crate) async fn post(base_url: &str, model: &str, messages: &Value, timeout: Duration, on: Option<OnDelta<'_>>) -> Result<Value, Failure> {
    let scope = super::scope_of_url(base_url);
    let flat = flatten(messages);
    let effort = effort_for_scope(scope, model);
    let images = if crate::snip::vision::reads_images(super::PROVIDER, model) == Some(false) { Vec::new() } else { flat.images.clone() };
    let began = std::time::Instant::now();
    // The scope's warm spare when it was made for this prompt; else (none,
    // another prompt's, or one that won't take the message) a cold start.
    let key = super::spare::Key::of(model, effort, &flat.system);
    let (session, mut rx, warm) = match super::spare::take(scope, &key) {
        Some(s) => match s.session.send_user(&flat.prompt, &images) {
            Ok(()) => (s.session, s.rx, true),
            Err(_) => {
                drop(s);
                let (session, rx) = cold(model, effort, &flat, &images)?;
                (session, rx, false)
            }
        },
        None => {
            let (session, rx) = cold(model, effort, &flat, &images)?;
            (session, rx, false)
        }
    };
    // Where the plan's limits stand, asked of this very process at most
    // once a minute (no other process, no model call) — a cleanup's, a
    // Command's or a meeting's call (Chat's turns ask after themselves).
    let mut usage_ask = (scope != super::Scope::Chat && super::usage::due_with_call()).then(|| session.get_usage().ok()).flatten();
    // One call each: stdin closed, it ends after this answer.
    session.end();
    // The next spare starts now, behind this call.
    super::spare::after_call(scope, &flat.system);
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
                Event::RateLimit(r) => super::usage::live(&r),
                Event::ControlResponse(c) if usage_ask.as_deref().is_some_and(|id| id == c.request_id) => {
                    usage_ask = None;
                    super::usage::kept_from_call(&crate::ai::claude_code::usage::from_response(&c));
                }
                Event::Problem(p) => {
                    if matches!(p, crate::ai::claude_code::events::Problem::NotSignedIn) {
                        crate::ai::claude_code::cli::forget();
                        super::spare::end_all("signed out");
                        // The key store's marker follows (signed out: no
                        // more calls start Claude Code until it's back).
                        std::thread::spawn(|| super::sync(&crate::ai::claude_code::cli::refresh()));
                    }
                    return Err(Failure::Plan(p.message()));
                }
                Event::Result(r) if r.is_error => {
                    let why = r.errors.first().cloned().filter(|e| !e.is_empty()).unwrap_or(r.text);
                    return Err(Failure::Other(if why.trim().is_empty() { "Claude Code couldn't answer.".to_string() } else { format!("Claude Code couldn't answer: {}", why.trim()) }));
                }
                Event::Result(r) => return Ok(r),
                Event::Exited(_) => break,
                _ => {}
            }
        }
        Err(Failure::Plan("Claude Code stopped before it answered.".to_string()))
    })
    .await;
    let result = match waited {
        Ok(r) => r,
        Err(_) => Err(Failure::Other("Claude Code took too long to answer.".to_string())),
    };
    let r = match result {
        Ok(r) => r,
        Err(e) => {
            session.kill();
            tracing::info!(scope = scope.id(), warm, plan = matches!(e, Failure::Plan(_)), "Claude plan: a one-shot call didn't answer");
            return Err(e);
        }
    };
    let counted = crate::ai::usage::Call { provider: super::PROVIDER, model, base_url: super::BASE_URL, turn: true, feature: None };
    crate::ai::usage::record_response(&counted, &usage_value(&r.usage, r.cost_usd), None, None);
    // The limits' answer still to come: read in the background, the call
    // not held up (its process ends once it's said).
    if let Some(id) = usage_ask {
        tauri::async_runtime::spawn(async move {
            let asked = crate::ai::claude_code::usage::read_answer(&mut rx, &id, Duration::from_secs(5)).await;
            super::usage::kept_from_call(&asked);
            drop(session);
        });
    }
    // The answer is the result's words (what streamed, if it said none).
    let text = if r.text.trim().is_empty() { shown } else { r.text };
    tracing::info!(scope = scope.id(), warm, ms = began.elapsed().as_millis() as u64, chars = text.chars().count(), "Claude plan: a one-shot call answered");
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
    fn a_calls_own_words_go_in_its_message_so_the_system_prompt_stays_the_same() {
        let entry = |from: &str, to: &str| crate::config::DictionaryEntry { from: from.into(), to: to.into(), fuzzy: true, snippet: false, starred: false, also_heard: Vec::new() };
        // A dictation's cleanup: the dictionary words it says change from
        // one dictation to the next; the rest of the prompt doesn't.
        let said_zentrix = crate::ai::llm::cleanup_messages_full("the zen tricks launch", "Keep it short.", Some("Use British spelling."), None, Some("<samples>A sample.</samples>"), &[entry("zen tricks", "Zentrix")], false);
        let said_nothing = crate::ai::llm::cleanup_messages_full("see you at three", "Keep it short.", Some("Use British spelling."), None, Some("<samples>A sample.</samples>"), &[], false);
        let (a, b) = (flatten(&said_zentrix), flatten(&said_nothing));
        assert_eq!(a.system, b.system, "one spare fits both dictations");
        assert!(!a.system.contains("Custom dictionary") && a.system.contains("Keep it short.") && a.system.contains("A sample."));
        let dictionary = a.prompt.find("Custom dictionary — when these appear in the transcript, apply these corrections: \"zen tricks\" → \"Zentrix\".").expect("in the message");
        assert!(dictionary < a.prompt.find("the zen tricks launch").unwrap(), "just before the dictation");
        assert!(!b.prompt.contains("Custom dictionary"));
        // A rewrite's selection (it may hold blank lines) likewise.
        let with = crate::ai::llm::rewrite_messages("make it shorter", "Dear Sam,\n\nThe launch moves to Friday.", None, "Be brief.");
        let without = crate::ai::llm::rewrite_messages("write a thank you note", "", None, "Be brief.");
        let (a, b) = (flatten(&with), flatten(&without));
        assert_eq!(a.system, b.system);
        assert!(a.prompt.starts_with("Use the following selected text as the context to edit:\n\"\"\"\nDear Sam,\n\nThe launch moves to Friday.\n\"\"\"\n\nUser's instruction: make it shorter"));
        // Nothing to move: as it was.
        assert_eq!(per_call_parts("You summarise chats."), ("You summarise chats.".to_string(), Vec::new()));
    }

    #[test]
    fn effort_by_scope_low_for_a_dictation_the_models_own_for_meetings() {
        use super::super::Scope;
        // The lowest the model takes for cleanup and Commands (Claude's
        // levels start at low), never a level a model without them can't take.
        assert_eq!(effort_for_scope(Scope::Cleanup, "claude-opus-5-5"), Some(session::Effort::Low));
        assert_eq!(effort_for_scope(Scope::Commands, "claude-sonnet-5-5"), Some(session::Effort::Low));
        assert_eq!(effort_for_scope(Scope::Notes, "claude-opus-5-5"), None, "Claude Code's own default");
        assert_eq!(effort_for_scope(Scope::Cleanup, "not-a-claude"), None);
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

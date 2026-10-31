//! **Long chats remember their start** (2026-10-25): once a conversation no
//! longer fits its model's context, its older messages are summarised and
//! the summary goes to the model in their place ("Earlier in this chat: …"),
//! the latest messages whole — instead of being left out unsaid.
//!
//! - **When** ([`after_answer`]): after an answer, in the background, once
//!   the history fills 75% of its room (`budget::due`) or the model's
//!   context is 75% used (the ring) — so the next question never waits. A
//!   summary that isn't ready, or failed, leaves the token window
//!   (`budget::plan`) in charge, and the next answer tries again.
//! - **Incremental**: a summary covers `messages[..upto]`; more falling out
//!   extend it from the old summary and the newly dropped messages, never
//!   the whole chat again. Long stretches go in parts that fit the model.
//! - **The model**: the chat's own (the on-device model is free; a cloud
//!   one already reads the history it summarises). A model on this PC steps
//!   aside for a dictation and for the next question (the summary waits for
//!   the next answer).
//! - **Kept** in `chats.json` on the conversation (`chats::ChatSummary`: the
//!   text, the index it covers, the model, when): a cache — the messages
//!   stay; an older Yap's save drops it, and the next long turn writes it
//!   again.
//! - **The prompt** ([`prompt`]): short and firm, for small local models:
//!   every fact, name and number exactly; what was asked and told; what's
//!   open; "- " lines; nothing added.
//!
//! Chat and the Voice Assistant (one conversation, each its own budget),
//! and a note's Ask threads (`note_chats.rs`). Logs: ids, counts, lengths.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

use crate::chat::budget::{self, Counter, Kind};
use crate::chat::chats::{ChatMessage, ChatModel, ChatSummary};

/// Told to every window when a conversation's summary changes:
/// `{conversationId, upto}` (Chat's divider and ring follow).
pub const EVENT: &str = "yap-chat-summary";

/// Where a summary call goes: the chat's own model.
#[derive(Debug, Clone, Default)]
pub struct Endpoint {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub provider: String,
    pub disable_thinking: bool,
    /// Who writes it, kept with it.
    pub by: Option<ChatModel>,
}

/// What a turn saw, for the look after its answer.
#[derive(Debug, Clone, Copy)]
pub struct Look {
    pub kind: Kind,
    /// The history's room (the budget, less a next question's).
    pub room: u64,
    /// The model's context.
    pub context: u64,
    /// The share of the context the answer's last call used (the ring).
    pub ring: Option<f64>,
    /// Earlier web answers' searches are replayed (they cost more).
    pub replay: bool,
}

/// The summary as a chat's prompt carries it. Pure.
pub fn block(text: &str) -> String {
    format!(
        "Earlier in this chat (Yap's notes on the older messages, which aren't shown; the latest messages follow in full):\n{}",
        text.trim()
    )
}

/// The summariser's instructions: short and firm, for small local models.
/// `words`: the most it may write. Pure.
pub fn prompt(words: u64) -> String {
    format!(
        "You keep the notes of a long chat between a user and an AI assistant, so the chat can go on once its older messages are left out.\n\nYou get the notes so far and the next messages. Write the notes again, with what the new messages add.\n\nRules:\n- Keep every fact the user gave: names, numbers, amounts, dates, places, decisions and preferences. Copy them exactly.\n- Keep what the user asked for and what they were told, in a few words each, and anything still open.\n- When an Assistant line names an AI model, keep that name with what it did (\"- Claude Opus 5.5 wrote the plan\").\n- Short lines starting with \"- \". No headings, no preamble, nothing else.\n- Only what the chat says: never add or guess anything.\n- Write in the language of the chat.\n- At most {words} words."
    )
}

/// The start of [`prompt`] (the e2e suite's stand-in model knows the call by it).
#[cfg(test)]
pub const PROMPT_START: &str = "You keep the notes of a long chat";

/// How long a summary may be, in tokens: 15% of the history's room, 250 to
/// 1,500. Pure.
pub fn target_tokens(room: u64) -> u64 {
    (room * 15 / 100).clamp(250, 1_500)
}

/// Did more than one model answer in `messages` (a switch in the picker)?
/// Pure.
pub fn models_switched(messages: &[ChatMessage]) -> bool {
    let mut by = messages.iter().filter(|m| m.role == "assistant").filter_map(|m| m.model.as_ref()).map(|m| (&m.provider, &m.id));
    by.next().is_some_and(|first| by.any(|m| m != first))
}

/// A message as the summariser reads it: who said it, and its text — a
/// very long one (a paste) its start and end. `named`: the chat switched
/// models, so an answer carries the model that wrote it ("Assistant (Claude
/// Opus 5.5)") and the notes keep who did what: written "Assistant:
/// Created a … build", the 4B took a build Claude Opus wrote for its own
/// (2026-10-31). Pure.
pub fn line(m: &ChatMessage, max_chars: usize, named: bool) -> String {
    let who = match (m.role == "assistant", m.model.as_ref().filter(|_| named)) {
        (true, Some(by)) => format!("Assistant ({})", budget::model_label(&by.provider, &by.id, &by.name)),
        (true, None) => "Assistant".to_string(),
        (false, _) => "User".to_string(),
    };
    let text = m.text.trim();
    let n = text.chars().count();
    if n <= max_chars {
        return format!("{who}: {text}");
    }
    let head: String = text.chars().take(max_chars * 7 / 10).collect();
    let tail: String = text.chars().skip(n - max_chars * 3 / 10).collect();
    format!("{who}: {}\n[… {} characters left out …]\n{}", head.trim_end(), n - head.chars().count() - tail.chars().count(), tail.trim_start())
}

/// The summariser's message: the notes so far, the next messages, what to
/// do. Pure.
pub fn request(old: Option<&str>, lines: &[String]) -> String {
    let notes = old.map(str::trim).filter(|t| !t.is_empty()).unwrap_or("(none yet)");
    format!(
        "Notes so far:\n{notes}\n\nNext messages:\n{}\n\nWrite the notes again: everything in the notes so far that still matters, and what these messages add.",
        lines.join("\n\n")
    )
}

/// The messages in parts the model can read at once: each part's lines
/// under `max_tokens` (a line longer than that alone in its part). Pure.
pub fn parts(lines: Vec<String>, max_tokens: u64) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut size = 0;
    for l in lines {
        let n = budget::estimate(&l);
        if out.is_empty() || (size + n > max_tokens && !out.last().is_some_and(Vec::is_empty)) {
            out.push(Vec::new());
            size = 0;
        }
        size += n;
        out.last_mut().expect("a part").push(l);
    }
    out
}

/// A summary as kept: trimmed, any preamble before the first "- " line
/// gone, no longer than `max_tokens` (cut at a line). `None`: nothing in it.
/// Pure.
pub fn tidy(reply: &str, max_tokens: u64) -> Option<String> {
    let text = reply.trim();
    // "Here are the updated notes:" before the lines.
    let text = match text.find("\n- ") {
        Some(i) if !text.starts_with("- ") && text[..i].lines().count() <= 2 && text[..i].trim_end().ends_with(':') => text[i + 1..].trim(),
        _ => text,
    };
    let mut out = String::new();
    for l in text.lines() {
        if budget::estimate(&out) + budget::estimate(l) + 1 > max_tokens {
            break;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(l);
    }
    let out = out.trim().to_string();
    (!out.is_empty()).then_some(out)
}

// ---- running one ----------------------------------------------------------------------------

/// Chat turns started: a summary being written on this PC steps aside for one.
static TURNS: AtomicU64 = AtomicU64::new(0);

/// A chat turn (or a note's Ask) started.
pub fn turn_started() {
    TURNS.fetch_add(1, Ordering::SeqCst);
}

/// The conversations a summary is being written for (one at a time each).
static RUNNING: Mutex<Option<HashSet<u64>>> = Mutex::new(None);
/// When a conversation's summary last failed: not tried again for a minute.
static FAILED: Mutex<Option<HashMap<u64, Instant>>> = Mutex::new(None);
const RETRY_AFTER: Duration = Duration::from_secs(60);

fn claim(conversation: u64) -> bool {
    RUNNING.lock().map(|mut g| g.get_or_insert_with(HashSet::new).insert(conversation)).unwrap_or(false)
}

fn release(conversation: u64) {
    if let Ok(mut g) = RUNNING.lock() {
        if let Some(s) = g.as_mut() {
            s.remove(&conversation);
        }
    }
}

fn failed_lately(conversation: u64) -> bool {
    FAILED.lock().ok().is_some_and(|g| g.as_ref().and_then(|m| m.get(&conversation)).is_some_and(|t| t.elapsed() < RETRY_AFTER))
}

fn note_failure(conversation: u64) {
    if let Ok(mut g) = FAILED.lock() {
        g.get_or_insert_with(HashMap::new).insert(conversation, Instant::now());
    }
}

/// After an answer in `conversation`: if its history is filling the room,
/// its older messages summarised in the background (a model evaluation
/// waits for it, so the next turn has it).
pub async fn after_answer(app: Option<&AppHandle>, conversation: u64, ep: Endpoint, look: Look) {
    if budget::mode() != budget::Mode::Summary {
        return;
    }
    if crate::ai::eval::active() {
        check(app.cloned(), conversation, ep, look).await;
        return;
    }
    let app = app.cloned();
    tauri::async_runtime::spawn(async move { check(app, conversation, ep, look).await });
}

/// After a turn kept in a note's Ask thread (`note_chats.rs`): its older
/// turns summarised when they fill the room, with the model the thread's
/// next question goes to from that window — the notepad's (the Note
/// Formatting model, `notepad`) or the Notes view's bar's (Chat's) —
/// beside the note's own prompt.
pub fn after_thread_turn(app: &AppHandle, note: u64, thread: u64, notepad: bool) {
    if budget::mode() != budget::Mode::Summary {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let cfg = crate::config::load();
        let Some(n) = crate::notes::get(note) else { return };
        let on_device = crate::ai::local_llm::active_model_display(&cfg);
        let (ep, fixed) = if notepad {
            let Ok(e) = crate::meetings::meeting_summary::resolve_endpoint(&cfg) else { return };
            let fixed = crate::meetings::meeting_summary::meeting_ask_messages(&n, "").to_string();
            let by = crate::ai::ai_model::describe(&e.provider, &e.model, &e.base_url, &on_device).chat_model();
            (Endpoint { base_url: e.base_url, api_key: e.api_key, model: e.model, provider: e.provider, disable_thinking: e.disable_thinking, by: Some(by) }, fixed)
        } else {
            let Ok((base_url, api_key, model, provider, persona, disable_thinking)) = crate::chat::turn::resolve_chat_endpoint(&cfg) else { return };
            let (system, _) = crate::notes::commands::note_prompt(&n, &cfg, &persona, &provider, &model, &base_url, "", chrono::Utc::now());
            let by = crate::ai::ai_model::describe(&provider, &model, &base_url, &on_device).chat_model();
            (Endpoint { base_url, api_key, model, provider, disable_thinking, by: Some(by) }, system)
        };
        let counter = Counter::for_call(&ep.provider, &ep.base_url);
        let (context, known) = budget::context_window(&cfg, &ep.provider, &ep.model, &ep.base_url).await;
        let b = budget::budget(context, known, counter.count(&fixed).await, Kind::Note, false, budget::request_cap(&ep.provider, &ep.model));
        let look = Look { kind: Kind::Note, room: crate::chat::turn::next_room(b.history), context, ring: None, replay: false };
        check(Some(app), thread, ep, look).await;
    });
}

/// Is a summary due (`budget::due`), and if so, write it.
async fn check(app: Option<AppHandle>, conversation: u64, ep: Endpoint, look: Look) {
    let Some(conv) = crate::chat::chats::get(conversation) else { return };
    let counter = Counter::for_call(&ep.provider, &ep.base_url);
    let costs = budget::message_costs(&counter, &conv.messages, look.replay).await;
    let users: Vec<bool> = conv.messages.iter().map(|m| m.role != "assistant").collect();
    let summary = match conv.usable_summary() {
        Some(s) => Some((s.upto, counter.count(&block(&s.text)).await)),
        None => None,
    };
    let Some(range) = budget::due(&costs, &users, summary, look.room, look.ring) else { return };
    if failed_lately(conversation) {
        return;
    }
    tracing::info!(
        conversation,
        from = range.start,
        to = range.end,
        messages = conv.messages.len(),
        room = look.room,
        "Chat: summarising the older messages"
    );
    match summarise(app.as_ref(), conversation, range, &ep, look).await {
        Ok(true) => {}
        Ok(false) => tracing::info!(conversation, "Chat: the summary was set aside; the next answer tries again"),
        Err(e) => {
            note_failure(conversation);
            tracing::info!(conversation, error_len = e.len(), "Chat: the summary failed; the latest messages go meanwhile");
        }
    }
}

/// Write conversation `conversation`'s summary to cover `range.end` (from
/// the one there is, which covers `range.start`), with `ep` — in parts
/// that fit a model of `context` tokens. `Ok(false)`: set aside (a
/// dictation, the next question on this PC, the chat changed meanwhile).
pub async fn summarise(app: Option<&AppHandle>, conversation: u64, range: Range<usize>, ep: &Endpoint, look: Look) -> Result<bool, String> {
    if !claim(conversation) {
        return Ok(false);
    }
    let out = summarise_claimed(app, conversation, range, ep, look).await;
    release(conversation);
    out
}

async fn summarise_claimed(app: Option<&AppHandle>, conversation: u64, range: Range<usize>, ep: &Endpoint, look: Look) -> Result<bool, String> {
    let (context, room) = (look.context, look.room);
    // Counted as the surface it's for (the network receipts, AI usage).
    let feature = match look.kind {
        Kind::Chat => crate::privacy::net::Feature::Chat,
        Kind::Assistant => crate::privacy::net::Feature::Assistant,
        Kind::Note => crate::privacy::net::Feature::Notes,
    };
    let Some(conv) = crate::chat::chats::get(conversation) else { return Ok(false) };
    let had = conv.usable_summary().map(|s| s.upto).unwrap_or(0);
    if had != range.start || range.end > conv.messages.len() || range.end <= range.start {
        return Ok(false);
    }
    let here = crate::ai::ai_model::runs_here(&ep.provider, &ep.base_url);
    let words = target_tokens(room) * 7 / 10;
    let max_reply = target_tokens(room) * 2;
    // What one call may read: half the context — within what one request
    // may carry (`budget::request_cap`: a plan's tokens a minute, the
    // model's cheaper rate) — less the prompt and the reply.
    let system = prompt(words);
    let most = (context / 2).min(budget::request_cap(&ep.provider, &ep.model).unwrap_or(u64::MAX));
    let reading = most.saturating_sub(budget::estimate(&system) + max_reply + target_tokens(room)).max(800);
    // A paste longer than a third of that: its start and end.
    let max_chars = (reading as usize / 3) * 36 / 10;
    let named = models_switched(&conv.messages);
    let lines: Vec<String> = conv.messages[range.clone()].iter().map(|m| line(m, max_chars.max(400), named)).collect();
    let mut notes: Option<String> = conv.usable_summary().map(|s| s.text.clone());
    let turns = TURNS.load(Ordering::SeqCst);
    let _sidecar = crate::ai::local_llm::hold(&ep.base_url);
    let opts = crate::ai::llm::ChatOptions {
        temperature: 0.2,
        // A model on this PC answers one request at a time: its reply is
        // capped; a cloud model's never is (reasoning spends part of it).
        max_tokens: here.then_some(u32::try_from(max_reply + 200).unwrap_or(u32::MAX)),
        timeout: if here { Duration::from_secs(180) } else { Duration::from_secs(90) },
        disable_thinking: ep.disable_thinking,
    };
    for part in parts(lines, reading) {
        let messages = json!([
            { "role": "system", "content": system },
            { "role": "user", "content": request(notes.as_deref(), &part) },
        ]);
        let mut reply = ask(app, ep, messages.clone(), &opts, here, turns, feature).await?;
        // A reasoning model can spend its whole reply thinking (Groq's
        // gpt-oss-20b: 2,048 tokens of reasoning and no notes, the model
        // evaluations): once more before it's a failure.
        if reply.as_deref().is_some_and(|r| r.trim().is_empty()) {
            tracing::info!("Chat: the summary came back empty; asking once more");
            reply = ask(app, ep, messages, &opts, here, turns, feature).await?;
        }
        let Some(reply) = reply else { return Ok(false) };
        notes = Some(tidy(&reply, max_reply).ok_or("the summary came back empty")?);
    }
    let text = notes.ok_or("nothing to summarise")?;
    let kept = crate::chat::chats::set_summary(
        conversation,
        range.start,
        ChatSummary {
            text,
            upto: range.end,
            after: crate::chat::chats::get(conversation).map(|c| c.messages.len()).unwrap_or(range.end),
            model: ep.by.clone(),
            ts: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        },
    );
    if !kept {
        return Ok(false);
    }
    tracing::info!(conversation, upto = range.end, "Chat: the older messages are summarised");
    if let Some(app) = app {
        let _ = app.emit(EVENT, json!({ "conversationId": conversation, "upto": range.end }));
    }
    Ok(true)
}

/// What a summary call adds for `model`: gpt-oss thinks little
/// (`reasoning_effort: "low"`, as Groq's own search asks it, `web/native.rs`)
/// — at its default it spent the whole reply thinking about the notes and
/// wrote none (Groq, the model evaluations, 2026-10-25). Pure.
pub fn extra(model: &str) -> serde_json::Map<String, Value> {
    let mut m = serde_json::Map::new();
    if model.to_ascii_lowercase().contains("gpt-oss") {
        m.insert("reasoning_effort".into(), json!("low"));
    }
    m
}

/// One summary call. A model on this PC waits out a dictation, and gives
/// up (`None`) when one starts or the next question does.
async fn ask(
    app: Option<&AppHandle>,
    ep: &Endpoint,
    messages: Value,
    opts: &crate::ai::llm::ChatOptions,
    here: bool,
    turns: u64,
    feature: crate::privacy::net::Feature,
) -> Result<Option<String>, String> {
    let call = crate::privacy::net::within(
        feature,
        crate::ai::llm::chat_with(&ep.base_url, &ep.api_key, &ep.model, &ep.provider, messages, opts, extra(&ep.model)),
    );
    if !here {
        return call.await.map(Some);
    }
    let dictating = || app.is_some_and(crate::meetings::meeting_summary::dictation_busy);
    let asked = || TURNS.load(Ordering::SeqCst) != turns;
    if !crate::meetings::meeting_summary::wait_idle(dictating, || !asked()).await {
        return Ok(None);
    }
    match crate::meetings::meeting_summary::unless_busy(call, || dictating() || asked()).await {
        Some(out) => out.map(Some),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(role: &str, text: &str) -> ChatMessage {
        ChatMessage { role: role.into(), text: text.into(), ..Default::default() }
    }

    #[test]
    fn the_prompt_is_short_and_firm() {
        let p = prompt(400);
        assert!(p.starts_with(PROMPT_START));
        assert!(p.contains("Copy them exactly") && p.contains("never add or guess") && p.contains("At most 400 words"));
        assert!(budget::estimate(&p) < 220, "{}", budget::estimate(&p));
        assert!(block("- Budget £18,400").starts_with("Earlier in this chat"));
    }

    #[test]
    fn gpt_oss_thinks_little_while_it_summarises() {
        assert_eq!(extra("openai/gpt-oss-20b").get("reasoning_effort"), Some(&json!("low")));
        assert!(extra("Qwen_Qwen3.5-4B-Q4_K_M").is_empty());
        assert!(extra("claude-haiku-4-5").is_empty());
    }

    #[test]
    fn a_summary_is_extended_from_the_old_one_and_the_new_messages() {
        let r = request(Some("- Budget £18,400\n- Marta Quill organises"), &[line(&said("user", "Book Friday's dinner"), 1_000, false)]);
        assert!(r.starts_with("Notes so far:\n- Budget £18,400\n- Marta Quill organises\n\nNext messages:\nUser: Book Friday's dinner"));
        assert!(request(None, &[]).contains("(none yet)"));
    }

    #[test]
    fn a_long_paste_is_read_by_its_start_and_end() {
        let text = format!("START {} END", "x".repeat(5_000));
        let l = line(&said("user", &text), 1_000, false);
        assert!(l.starts_with("User: START"));
        assert!(l.ends_with("END"));
        assert!(l.contains("characters left out"));
        assert!(l.chars().count() < 1_200);
        assert_eq!(line(&said("assistant", " Hi "), 1_000, false), "Assistant: Hi");
    }

    #[test]
    fn after_a_switch_the_notes_keep_which_model_wrote_an_answer() {
        let qwen = ChatModel { provider: "ondevice".into(), id: "Qwen_Qwen3.5-4B-Q4_K_M".into(), name: "Qwen3.5 4B".into(), on_this_pc: true };
        let opus = ChatModel { provider: "anthropic".into(), id: "claude-opus-5-5".into(), name: String::new(), on_this_pc: false };
        let by = |m: &ChatModel, text: &str| ChatMessage { model: Some(m.clone()), ..said("assistant", text) };
        let chat = vec![said("user", "Make me a build"), by(&opus, "Here's your build."), said("user", "Hey"), by(&qwen, "Hello!")];
        assert!(models_switched(&chat));
        assert!(!models_switched(&chat[..2]));
        assert!(!models_switched(&[said("user", "Hi"), said("assistant", "Hello!")]));
        assert_eq!(line(&chat[1], 1_000, true), "Assistant (Claude Opus 5.5): Here's your build.");
        assert_eq!(line(&chat[3], 1_000, true), "Assistant (Qwen3.5 4B): Hello!");
        // One model all along: no names (the model isn't told its own).
        assert_eq!(line(&chat[1], 1_000, false), "Assistant: Here's your build.");
        assert_eq!(line(&chat[0], 1_000, true), "User: Make me a build");
        assert!(prompt(400).contains("When an Assistant line names an AI model, keep that name"));
    }

    #[test]
    fn long_stretches_go_in_parts_the_model_can_read() {
        let lines: Vec<String> = (0..10).map(|i| format!("User: {} {i}", "word ".repeat(100))).collect();
        let each = budget::estimate(&lines[0]);
        let p = parts(lines.clone(), each * 3);
        assert_eq!(p.iter().map(Vec::len).collect::<Vec<_>>(), [3, 3, 3, 1]);
        // One line longer than a part: alone in its own.
        let p = parts(lines, each / 2);
        assert_eq!(p.len(), 10);
        assert!(parts(Vec::new(), 100).is_empty());
    }

    #[test]
    fn a_summary_is_kept_tidy() {
        assert_eq!(tidy("Here are the updated notes:\n- A\n- B", 100).as_deref(), Some("- A\n- B"));
        assert_eq!(tidy("  - A  ", 100).as_deref(), Some("- A"));
        assert_eq!(tidy("   ", 100), None);
        // Cut at a line when it runs long.
        let long = (0..200).map(|i| format!("- fact {i}")).collect::<Vec<_>>().join("\n");
        let t = tidy(&long, 50).unwrap();
        assert!(budget::estimate(&t) <= 50 && t.ends_with(|c: char| c.is_ascii_digit()));
        assert_eq!(target_tokens(1_000), 250);
        assert_eq!(target_tokens(40_000), 1_500);
    }

    #[test]
    fn the_summary_is_kept_only_over_the_one_it_extends() {
        let c = crate::chat::chats::create("Offsite");
        for i in 0..8 {
            crate::chat::chats::append(c.id, if i % 2 == 0 { "user" } else { "assistant" }, &format!("message {i}"), &[], None, false).unwrap();
        }
        let s = |upto: usize, text: &str| ChatSummary { text: text.into(), upto, after: 8, model: None, ts: 1 };
        // The first: covers the first four.
        assert!(crate::chat::chats::set_summary(c.id, 0, s(4, "- first")));
        // Another pass that started from nothing: not over it.
        assert!(!crate::chat::chats::set_summary(c.id, 0, s(6, "- stale")));
        // Extending the first: kept.
        assert!(crate::chat::chats::set_summary(c.id, 4, s(6, "- second")));
        // More messages than there are, or not further: not kept.
        assert!(!crate::chat::chats::set_summary(c.id, 6, s(20, "- x")));
        assert!(!crate::chat::chats::set_summary(c.id, 6, s(6, "- x")));
        let got = crate::chat::chats::get(c.id).unwrap();
        assert_eq!(got.usable_summary().map(|x| (x.upto, x.text.as_str())), Some((6, "- second")));
        // Stored camelCase; an older Yap without it reads the rest.
        let v = serde_json::to_value(&got).unwrap();
        assert_eq!(v["summary"]["upto"], 6);
        let mut old = v.clone();
        old.as_object_mut().unwrap().remove("summary");
        let back: crate::chat::chats::Conversation = serde_json::from_value(old).unwrap();
        assert!(back.summary.is_none() && back.messages.len() == 8);
    }
}

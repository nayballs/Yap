//! **A side question** (15 Nov 2026; the Claude desktop app's side chat,
//! Ctrl+;): a quick question about the chat open in Chat, answered in a box
//! over the thread and not added to it.
//!
//! - **What it reads** ([`build`]): the chat's own messages, sized to Chat's
//!   model as a turn's history is (`budget.rs`: every message, else the
//!   summary of the older ones and those after it, else the latest that
//!   fit), after the persona with the line naming the model, how to answer
//!   a side question ([`INSTRUCTIONS`]), and the prompt's usual end: in a
//!   plain chat its project's and style's blocks and memory's "You", then
//!   About you (a Code chat's without its note, no memory; an incognito
//!   chat's without memory or its project), as a turn's prompt has them.
//! - **One call, no tools** ([`answer`]): Chat's own model through
//!   `llm::note_chat`, whichever provider it is — on the Claude plan, a
//!   one-shot Claude Code run with `--tools ""` and
//!   `--no-session-persistence` (`claude_plan/oneshot.rs`), which keeps no
//!   transcript. Nothing can look anything up, open, change or act, and no
//!   card can come; with nothing offered, `turn/fit.rs` has no family to
//!   shed. The answer is checked as every answer is
//!   (`chats::checked_answer`, the dollar note).
//! - **Never the conversation's turn**: it doesn't answer the conversation
//!   (`stream::Stoppable::answers` is never called), so a turn still being
//!   written there goes on, a Code chat's Claude goes on working, and the
//!   status dots don't move. Its stop is the page's own (`stream_stop` by
//!   its id: Esc or ✕).
//! - **A Code chat**: the question never enters the chat's Claude Code
//!   session. Chat's model answers from the thread's text. (`--resume
//!   <session> --fork-session --no-session-persistence --tools ""` would
//!   leave no file either, but mid-turn the session's transcript ends in a
//!   tool call still running, and a fork reads all of its tool output
//!   again: many more tokens for a quick question.)
//! - **Nothing saved**: no message, nothing for search or memory to read,
//!   nothing listed, nothing kept in Yap at all once the box closes — the
//!   page holds the answer. Its tokens are counted as any Chat call's
//!   (`usage.rs`, under Chat; the network receipts too).
//! - **Keep it** ([`chat_side_keep`]): the question and answer added to the
//!   thread as an ordinary exchange, then treated as any answered turn
//!   (search by meaning, memory learning by its own rules); never while the
//!   thread is answering or its Claude works (the box waits for it). A Code
//!   chat that works directly: its session hears of it with its next
//!   message, the exchange after the words as `<earlier_in_this_chat>`
//!   ([`with_kept`]), as a plan chat's session hears another model's.
//!
//! The log has lengths and counts, never the words.

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::chat::budget::{self, Counter, Kind};
use crate::chat::chats::{ChatMessage, ChatModel, Conversation, MessageTokens};
use crate::chat::stream::{self, Event, Sink};

/// **How to answer a side question**, after the persona and the line
/// naming the model.
pub const INSTRUCTIONS: &str = "This is a side question. The user asks it beside the chat above, which you can read: your answer shows apart from the chat and isn't added to it unless the user keeps it. Answer briefly and to the point, in markdown, from the chat and what you know. You have no tools here: you can't search the web, read files, open apps or pages, or change anything. When the question needs one of those, say so in a sentence and suggest asking it in the chat itself.";

/// A Code chat's line (`{name}`: its workspace).
const CODE_LINE: &str = "The chat is a Code chat about the code in the user's workspace {name}. You see its messages, not its files: when you'd need to read a file to be sure, say so.";

/// The person's last message, still being answered (a turn being written,
/// a Code chat's Claude at work): in the prompt, never as a question.
const STILL_ANSWERING: &str = "The user's last message in the chat is still being answered, so its answer isn't above yet. Here it is, for context only (don't answer it here):";

/// **Keep it** while the thread is answering: the box waits.
pub const KEEP_LATER: &str = "Keep it once the answer in the chat is done.";

/// What a side question sends: the system prompt, the chat's messages as
/// the model reads them (roles alternating), and the question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub system: String,
    pub history: Vec<(String, String)>,
    pub question: String,
}

/// The parts of a side question's prompt that aren't the chat's own
/// messages.
pub struct Parts<'a> {
    /// The persona with the line naming the model answering.
    pub persona: &'a str,
    /// About you (`about_you::prompt_block`: the model has no tools, so the
    /// clock is in the system prompt too).
    pub about: &'a str,
    /// Memory's "You" (none in a Code chat or an incognito chat).
    pub memory_you: &'a str,
    /// The chat's project's and style's blocks (none in a Code chat).
    pub chat_blocks: &'a str,
    /// A Code chat's workspace name.
    pub code: Option<&'a str>,
    /// Who answers (the answers another model wrote are said so).
    pub answering: &'a ChatModel,
}

/// The model's room: its context, whether it said so, and the most a
/// request may be (`budget::request_cap`).
#[derive(Debug, Clone, Copy)]
pub struct Room {
    pub context: u64,
    pub known: bool,
    pub cap: Option<u64>,
}

/// **The side question's request**, sized to the model as a turn's history
/// is (`budget.rs`): the question too long for it is said plainly (the
/// error, `budget::too_long`), never sent. `label` names the model in it.
pub async fn build(conv: &Conversation, question: &str, parts: &Parts<'_>, counter: &Counter, room: Room, label: &str) -> Result<Request, String> {
    let question = question.trim();
    if question.is_empty() {
        return Err("Type a question first".to_string());
    }
    // The person's last message, still being answered: context in the
    // prompt, never a question the model answers here.
    let (messages, pending): (&[ChatMessage], Option<&ChatMessage>) = match conv.messages.split_last() {
        Some((last, rest)) if last.role == "user" => (rest, Some(last)),
        _ => (&conv.messages[..], None),
    };
    let mut head = format!("{}\n\n{INSTRUCTIONS}", parts.persona.trim());
    if let Some(name) = parts.code {
        head.push(' ');
        head.push_str(&CODE_LINE.replace("{name}", name));
    }
    if pending.is_some() {
        let text = crate::chat::chats::sent_text_at(&conv.messages, conv.messages.len() - 1);
        head.push_str(&format!("\n\n{STILL_ANSWERING}\n\"\"\"\n{}\n\"\"\"", text.trim()));
    }
    let end = [parts.chat_blocks, parts.about, parts.memory_you].into_iter().map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n\n");
    // Measured as a turn measures its fixed part, the history in what's left.
    let fixed = counter.count(&head).await + counter.count(&end).await + budget::MESSAGE_OVERHEAD;
    let room_now = budget::budget(room.context, room.known, fixed, Kind::Chat, false, room.cap);
    let question_tokens = counter.count(question).await + budget::MESSAGE_OVERHEAD;
    if question_tokens > room_now.history {
        tracing::info!(question_tokens, room = room_now.history, "Chat: a side question too long for the model; not sent");
        return Err(budget::too_long(label, room.context));
    }
    let left = room_now.history - question_tokens;
    let summary = conv.usable_summary().filter(|s| s.upto <= messages.len()).filter(|_| budget::mode() == budget::Mode::Summary);
    let costs = budget::message_costs(counter, messages, false).await;
    let summary_cost = match summary {
        Some(s) => Some((s.upto, counter.count(&crate::chat::compact::block(&s.text)).await)),
        None => None,
    };
    let plan = budget::plan(&costs, summary_cost, left, Kind::Chat.step());
    let earlier = &messages[plan.start..];
    let mut system = head;
    if let (true, Some(s)) = (plan.summary, summary) {
        system.push_str(&format!("\n\n{}", crate::chat::compact::block(&s.text)));
    }
    // Which earlier answers came from the web, and which another model wrote.
    if let Some(note) = crate::chat::chats::web_history_note(earlier) {
        system.push_str(&format!("\n\n{note}"));
    }
    if let Some(note) = crate::chat::chats::other_models_note(messages, parts.answering, plan.summary) {
        system.push_str(&format!("\n\n{note}"));
    }
    if !end.is_empty() {
        system.push_str(&format!("\n\n{end}"));
    }
    let history = alternating(earlier.iter().enumerate().map(|(i, m)| (m.role.clone(), crate::chat::chats::sent_text_at(earlier, i))).collect());
    tracing::info!(messages = conv.messages.len(), sent = history.len(), summary = plan.summary, pending = pending.is_some(), "Chat: a side question, sized to the model");
    Ok(Request { system, history, question: question.to_string() })
}

/// **The chat's messages as roles that alternate**: the person's, then the
/// answer's — two in a row of the same side (a question a newer one
/// replaced, an agent's report after an answer) as one, empty ones left
/// out, and none of the assistant's before the person's first (a summary's
/// cut). Some models' chat templates refuse anything else (Gemma's). Pure.
pub fn alternating(messages: Vec<(String, String)>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (role, text) in messages {
        let role = if role == "assistant" { "assistant" } else { "user" };
        let text = text.trim();
        if text.is_empty() || (out.is_empty() && role == "assistant") {
            continue;
        }
        match out.last_mut() {
            Some((r, t)) if r == role => {
                t.push_str("\n\n");
                t.push_str(text);
            }
            _ => out.push((role.to_string(), text.to_string())),
        }
    }
    // (The question is the person's: the history ends with an answer.)
    if out.last().is_some_and(|(r, _)| r == "user") {
        out.pop();
    }
    out
}

/// Where a side question goes: Chat's model, as a turn resolves it
/// (`turn::resolve_chat_endpoint`).
pub struct Endpoint {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub provider: String,
    pub disable_thinking: bool,
}

/// **The answer**: one call with no tools (`llm::note_chat`: the request
/// carries none), counted under Chat, streamed to `live`; stopped (Esc,
/// ✕), what showed so far (`watch`) and `true`.
pub async fn answer(req: &Request, ep: &Endpoint, live: Option<stream::Live<'_>>, stop: &stream::Stoppable, watch: Option<&stream::Watch<'_>>) -> Result<(String, bool), String> {
    let call = crate::privacy::net::within(
        crate::privacy::net::Feature::Chat,
        crate::ai::llm::note_chat(&req.system, &req.history, &req.question, &ep.base_url, &ep.api_key, &ep.model, &ep.provider, ep.disable_thinking, live),
    );
    match stream::unless_stopped(call, Some(stop)).await {
        Ok(reply) => Ok((reply?, false)),
        Err(_) => Ok((watch.map(|w| w.seen().text).unwrap_or_default(), true)),
    }
}

/// **The page's channel, as the box hears it**: the answer's events, and
/// nothing for a Yap Lab run's record of the chat's turn (its
/// `PageChannel::send_json`), which isn't this.
struct BoxChannel(stream::PageChannel);

impl Sink for BoxChannel {
    fn send(&self, ev: Event) {
        if let Some(ch) = &self.0 .0 {
            let _ = ch.send(serde_json::to_value(ev).unwrap_or(Value::Null));
        }
    }
}

/// **A side question asked** (Ctrl+; in Chat): `question` about
/// conversation `conversation_id`, answered by Chat's model into the box
/// (`on_stream`; `turn_id` its stop), never saved. `{text, model, tokens,
/// stopped}`: what Keep it adds.
#[tauri::command]
pub async fn chat_side_ask(app: AppHandle, conversation_id: u64, question: String, on_stream: stream::PageChannel, turn_id: Option<String>) -> Result<Value, String> {
    Box::pin(ask(Some(&app), conversation_id, &question, BoxChannel(on_stream), turn_id.as_deref())).await
}

async fn ask(app: Option<&AppHandle>, conversation_id: u64, question: &str, sink: BoxChannel, turn_id: Option<&str>) -> Result<Value, String> {
    let conv = crate::chat::chats::get(conversation_id).ok_or("That chat isn't there any more.")?;
    let cfg = crate::config::load();
    crate::ai::ai_model::wake_on_device(&cfg).await?;
    let (base_url, api_key, model, provider, persona, disable_thinking) = crate::chat::turn::resolve_chat_endpoint(&cfg)?;
    let answering = crate::ai::ai_model::who_answers(&cfg, &provider, &model, &base_url);
    let persona = crate::ai::ai_model::persona_with_answering(&persona, &answering);
    // The on-device model stays up for the call.
    let _sidecar = crate::ai::local_llm::hold(&base_url);
    // As a turn's prompt has them (turn/prompt.rs): a Code chat's About you
    // without its note and no memory; an incognito chat's no memory, no project.
    let folder = crate::agents::code_chat::folder_of(Some(conv.id));
    let profile = crate::ai::about_you::profile(&cfg);
    let profile = if folder.is_some() { profile.without_note() } else { profile };
    let now = crate::ai::eval::now();
    let about = crate::ai::about_you::prompt_block(&profile, now);
    let memory_you = if folder.is_some() || conv.incognito { String::new() } else { crate::memory::you_block() };
    let counter = Counter::for_call(&provider, &base_url);
    let (context, known) = budget::context_window(&cfg, &provider, &model, &base_url).await;
    let chat_blocks = if folder.is_some() {
        String::new()
    } else {
        // (A big project's: its list of parts, and those that match the side
        // question — it has no tools to read more: chat/projects/knowledge.rs.)
        let room = crate::chat::projects::knowledge::Room::new(context, budget::request_cap(&provider, &model));
        let project = if conv.incognito { None } else { crate::chat::projects::knowledge::for_side(conv.id, question, room).await };
        [project, crate::chat::styles::prompt_block(Some(conv.id))].into_iter().flatten().collect::<Vec<_>>().join("\n\n")
    };
    let parts = Parts { persona: &persona, about: &about, memory_you: &memory_you, chat_blocks: &chat_blocks, code: folder.as_ref().map(|f| f.name.as_str()), answering: &answering };
    let label = budget::model_label(&provider, &model, &crate::ai::local_llm::active_model_display(&cfg));
    let room = Room { context, known, cap: budget::request_cap(&provider, &model) };
    let req = build(&conv, question, &parts, &counter, room, &label).await?;
    let ep = Endpoint { base_url, api_key, model, provider, disable_thinking };
    let stop = stream::Stoppable::new(turn_id);
    let watch = stream::Watch::new(&sink);
    let beside = app.filter(|_| crate::ai::ai_model::runs_here(&ep.provider, &ep.base_url)).map(|a| stream::NewDictations(a.clone()));
    let live = stream::Live { sink: &watch, beside: beside.as_ref().map(|b| b as &dyn stream::Dictations) };
    let (answered, last) = crate::ai::usage::tallied(async {
        let out = answer(&req, &ep, Some(live), &stop, Some(&watch)).await;
        (out, crate::ai::usage::last_call())
    })
    .await;
    let (reply, stopped) = answered.map_err(|e| budget::plain_error(e, &label, context, &ep.provider))?;
    if reply.trim().is_empty() {
        return Err(if stopped { stream::STOPPED.to_string() } else { "The model returned an empty answer".to_string() });
    }
    let text = checked(&reply, question, &conv, &profile.region);
    let tokens = if stopped { None } else { last.as_ref().and_then(MessageTokens::of) };
    tracing::info!(question_chars = question.chars().count(), answer_chars = text.chars().count(), stopped, "Chat: a side question answered (not saved)");
    Ok(json!({ "text": text, "model": answering, "tokens": tokens, "stopped": stopped }))
}

/// **The answer checked** as every answer is (`turn/finish.rs`): no
/// sources list of its own, a search it claims said not to have happened,
/// no "[1]" with nothing behind it, dollars said as such.
fn checked(reply: &str, question: &str, conv: &Conversation, region: &crate::ai::about_you::Region) -> String {
    let (text, fixed) = crate::chat::chats::checked_answer(reply, question, false, &conv.messages, &[]);
    for f in fixed {
        tracing::info!(fix = f, "Chat: Yap fixed a side question's answer");
    }
    match crate::ai::about_you::foreign_price_note(&text, region) {
        Some(note) => format!("{}\n\n{note}", text.trim_end()),
        None => text,
    }
}

/// **Is the thread answering?** A turn being written in it (any surface's),
/// or the Code chat's own Claude at work: Keep it waits.
pub fn busy(conversation: u64) -> bool {
    stream::is_answering(conversation) || crate::agents::store::of_conversation(conversation).iter().any(|a| a.works_directly() && a.status.live())
}

/// **Keep it**: the side question and its answer added to conversation
/// `conversation_id` as an ordinary exchange — the question the person's,
/// the answer `model`'s with its `tokens` — then treated as any answered
/// turn is (`memory::learn::after_turn`'s rules: never an incognito chat or
/// a Code chat). Refused while the thread is answering ([`KEEP_LATER`]).
/// `{userMessageId, messageId}`.
#[tauri::command]
pub fn chat_side_keep(conversation_id: u64, question: String, answer: String, model: Option<ChatModel>, tokens: Option<MessageTokens>) -> Result<Value, String> {
    let (question, answer) = (question.trim(), answer.trim());
    if question.is_empty() || answer.is_empty() {
        return Err("Nothing to keep yet.".to_string());
    }
    let conv = crate::chat::chats::get(conversation_id).ok_or("That chat isn't there any more.")?;
    if busy(conversation_id) {
        return Err(KEEP_LATER.to_string());
    }
    let (q, a) = keep(&conv, question, answer, model, tokens)?;
    crate::memory::learn::after_turn(conversation_id);
    tracing::info!(question = q, answer = a, "Chat: a side question kept in the chat");
    Ok(json!({ "userMessageId": format!("chat:{conversation_id}:{q}"), "messageId": format!("chat:{conversation_id}:{a}") }))
}

/// [`chat_side_keep`]'s two messages, and a Code chat's note for its
/// session ([`with_kept`]). Their indexes.
fn keep(conv: &Conversation, question: &str, answer: &str, model: Option<ChatModel>, tokens: Option<MessageTokens>) -> Result<(usize, usize), String> {
    let q = crate::chat::chats::append_question(conv.id, question, false, &[])?;
    let a = crate::chat::chats::append_message(conv.id, "assistant", answer, &[], model, false, &[], false, tokens)?;
    if crate::agents::code_chat::folder_of(Some(conv.id)).is_some() {
        let mut g = KEPT.lock().unwrap_or_else(|p| p.into_inner());
        g.get_or_insert_with(HashMap::new).entry(conv.id).or_default().push(Kept { created: conv.created_ts, at: q, question: question.to_string(), answer: answer.to_string() });
    }
    Ok((q, a))
}

/// A side question kept in a Code chat, for its session's next message.
struct Kept {
    /// Its conversation's `created_ts` (a later chat may take a deleted one's id).
    created: u64,
    /// The question's index (the answer's is next).
    at: usize,
    question: String,
    answer: String,
}

/// Each Code chat's kept side questions its session hasn't heard of. In
/// memory only: Yap quit before the next message, the thread still has them.
static KEPT: Mutex<Option<HashMap<u64, Vec<Kept>>>> = Mutex::new(None);

/// **A Code chat's next message to its session** (`turn.rs`
/// `sent_directly`): `text`, then the side questions kept since it last
/// heard from the chat, as `<earlier_in_this_chat>`
/// (`claude_plan::sessions::earlier_block`) — the ones still in the thread
/// where they were (a Rewind may have cut them). Each told once.
pub fn with_kept(conversation: u64, text: String) -> String {
    let kept = KEPT.lock().unwrap_or_else(|p| p.into_inner()).as_mut().and_then(|m| m.remove(&conversation)).unwrap_or_default();
    if kept.is_empty() {
        return text;
    }
    let Some(conv) = crate::chat::chats::get(conversation) else { return text };
    let pairs: Vec<(String, String)> = kept
        .into_iter()
        .filter(|k| k.created == conv.created_ts && conv.messages.get(k.at).is_some_and(|m| m.text == k.question) && conv.messages.get(k.at + 1).is_some_and(|m| m.text == k.answer))
        .flat_map(|k| [("user".to_string(), k.question), ("assistant".to_string(), k.answer)])
        .collect();
    match crate::ai::claude_plan::sessions::earlier_block(&pairs) {
        Some(block) => format!("{text}\n\n{block}"),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn conv_with(messages: &[(&str, &str)]) -> Conversation {
        let c = crate::chat::chats::create("Allotment");
        for (role, text) in messages {
            crate::chat::chats::append(c.id, role, text, &[], None, false).unwrap();
        }
        crate::chat::chats::get(c.id).unwrap()
    }

    fn model() -> ChatModel {
        ChatModel { provider: "custom".into(), id: "stand-in".into(), name: String::new(), on_this_pc: false }
    }

    fn room(context: u64) -> Room {
        Room { context, known: true, cap: None }
    }

    #[test]
    fn the_history_alternates_and_ends_with_an_answer() {
        let pairs = |v: &[(&str, &str)]| v.iter().map(|(r, t)| (r.to_string(), t.to_string())).collect::<Vec<_>>();
        assert_eq!(
            alternating(pairs(&[("assistant", "(summary's cut)"), ("user", "Beans?"), ("user", "Runner beans?"), ("assistant", "May."), ("assistant", ""), ("user", "And peas?")])),
            pairs(&[("user", "Beans?\n\nRunner beans?"), ("assistant", "May.")])
        );
        assert!(alternating(Vec::new()).is_empty());
    }

    #[test]
    fn a_side_question_reads_the_thread_with_no_tools_and_says_what_it_is() {
        let c = conv_with(&[("user", "When do I sow runner beans?"), ("assistant", "Mid-May, outdoors."), ("user", "And how deep?")]);
        let answering = model();
        let parts = Parts { persona: "You are a helpful assistant.", about: "About the user: in the UK.", memory_you: "", chat_blocks: "", code: None, answering: &answering };
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let req = rt.block_on(build(&c, "  What's a good companion plant?  ", &parts, &Counter::estimate_only(), room(32_000), "Stand-in")).unwrap();
        assert_eq!(req.question, "What's a good companion plant?");
        // The thread, as roles: the last message, still being answered, is context in the prompt.
        assert_eq!(req.history, vec![("user".to_string(), "When do I sow runner beans?".to_string()), ("assistant".to_string(), "Mid-May, outdoors.".to_string())]);
        assert!(req.system.starts_with("You are a helpful assistant.\n\nThis is a side question."), "{}", req.system);
        assert!(req.system.contains(STILL_ANSWERING) && req.system.contains("And how deep?"));
        assert!(req.system.ends_with("About the user: in the UK."));
        // No tool is offered, so none is named.
        assert!(req.system.contains("You have no tools here"));
        for tool in ["web_search", "open_app", "browser_", "remember", "ask_user", "suggest_next_step", "mcp__"] {
            assert!(!req.system.contains(tool), "{tool}");
        }
        // A Code chat says what it is.
        let parts = Parts { code: Some("garden-planner"), ..parts };
        let req = rt.block_on(build(&c, "Which file?", &parts, &Counter::estimate_only(), room(32_000), "Stand-in")).unwrap();
        assert!(req.system.contains("workspace garden-planner. You see its messages, not its files"));
    }

    #[test]
    fn a_long_chat_is_sized_to_the_model_and_a_long_question_refused() {
        let long = "word ".repeat(600);
        let mut msgs = Vec::new();
        for i in 0..12 {
            msgs.push(("user", if i == 0 { "First question" } else { "Another question" }));
            msgs.push(("assistant", long.as_str()));
        }
        let c = conv_with(&msgs);
        let answering = model();
        let parts = Parts { persona: "You are a helpful assistant.", about: "", memory_you: "", chat_blocks: "", code: None, answering: &answering };
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        // An 8K model: the latest that fit, never the first.
        let req = rt.block_on(build(&c, "Sum it up?", &parts, &Counter::estimate_only(), room(8_192), "Stand-in")).unwrap();
        assert!(!req.history.is_empty() && req.history.len() < 24, "{}", req.history.len());
        assert!(!req.history.iter().any(|(_, t)| t == "First question"));
        // A roomy model: all of it.
        let req = rt.block_on(build(&c, "Sum it up?", &parts, &Counter::estimate_only(), room(200_000), "Stand-in")).unwrap();
        assert_eq!(req.history.len(), 24);
        // A question too long for the model: said plainly, never sent.
        let err = rt.block_on(build(&c, &"word ".repeat(9_000), &parts, &Counter::estimate_only(), room(8_192), "Stand-in")).unwrap_err();
        assert_eq!(err, budget::too_long("Stand-in", 8_192));
    }

    /// A provider on this PC that answers every chat request with `words`,
    /// streamed, and keeps each request's body.
    async fn stand_in(words: &'static [&'static str]) -> (String, Arc<Mutex<Vec<Value>>>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://127.0.0.1:{}/v1", listener.local_addr().unwrap().port());
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&bodies);
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                let kept = Arc::clone(&kept);
                tokio::spawn(async move {
                    let (mut got, mut buf) = (Vec::new(), [0u8; 8192]);
                    let body = loop {
                        let n = sock.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        got.extend_from_slice(&buf[..n]);
                        let head = String::from_utf8_lossy(&got).to_string();
                        if let Some(end) = head.find("\r\n\r\n") {
                            let len = head[..end].lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").and_then(|v| v.trim().parse::<usize>().ok())).unwrap_or(0);
                            if got.len() >= end + 4 + len {
                                break got[end + 4..end + 4 + len].to_vec();
                            }
                        }
                    };
                    if let Ok(v) = serde_json::from_slice::<Value>(&body) {
                        kept.lock().unwrap().push(v);
                    }
                    let sse = |delta: Value, finish: Option<&str>| format!("data: {}\n\n", json!({ "choices": [{ "index": 0, "delta": delta, "finish_reason": finish }] }));
                    let mut out = String::from("HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n");
                    out.push_str(&sse(json!({ "role": "assistant", "content": "" }), None));
                    for w in words {
                        out.push_str(&sse(json!({ "content": w }), None));
                    }
                    out.push_str(&sse(json!({}), Some("stop")));
                    out.push_str("data: [DONE]\n\n");
                    let _ = sock.write_all(out.as_bytes()).await;
                    let _ = sock.flush().await;
                });
            }
        });
        (base, bodies)
    }

    #[derive(Default)]
    struct Heard(Mutex<Vec<Event>>);
    impl Sink for Heard {
        fn send(&self, ev: Event) {
            self.0.lock().unwrap().push(ev);
        }
    }

    #[test]
    fn the_answer_streams_with_no_tools_offered_and_nothing_is_saved() {
        let c = conv_with(&[("user", "When do I sow runner beans?"), ("assistant", "Mid-May, outdoors.")]);
        let before = crate::chat::chats::get(c.id).unwrap();
        let answering = model();
        let parts = Parts { persona: "You are a helpful assistant.", about: "", memory_you: "", chat_blocks: "", code: None, answering: &answering };
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap();
        let (reply, stopped, bodies, heard) = rt.block_on(async {
            let (base, bodies) = stand_in(&["Marigolds ", "keep aphids off."]).await;
            let req = build(&c, "What's a good companion plant?", &parts, &Counter::estimate_only(), room(32_000), "Stand-in").await.unwrap();
            let ep = Endpoint { base_url: base, api_key: String::new(), model: "stand-in".into(), provider: "custom".into(), disable_thinking: false };
            let heard = Heard::default();
            let stop = stream::Stoppable::new(None);
            let (reply, stopped) = {
                let watch = stream::Watch::new(&heard);
                answer(&req, &ep, Some(stream::Live { sink: &watch, beside: None }), &stop, Some(&watch)).await.unwrap()
            };
            (reply, stopped, bodies, heard)
        });
        assert_eq!((reply.as_str(), stopped), ("Marigolds keep aphids off.", false));
        // It streamed to the box.
        assert!(heard.0.lock().unwrap().iter().any(|e| matches!(e, Event::Text { text } if text.contains("Marigolds"))));
        // The request: no tools offered, the thread then the question.
        let bodies = bodies.lock().unwrap();
        let body = bodies.iter().find(|b| b["messages"].is_array()).expect("the chat request");
        assert!(body.get("tools").is_none_or(Value::is_null) && body.get("tool_choice").is_none_or(Value::is_null), "{body}");
        let roles: Vec<&str> = body["messages"].as_array().unwrap().iter().map(|m| m["role"].as_str().unwrap()).collect();
        assert_eq!(roles, ["system", "user", "assistant", "user"]);
        assert_eq!(body["messages"][3]["content"], "What's a good companion plant?");
        // Nothing saved: the conversation is as it was, and it never answered there.
        let after = crate::chat::chats::get(c.id).unwrap();
        assert_eq!(after.messages.len(), before.messages.len());
        assert_eq!(after.updated_ts, before.updated_ts);
        assert!(!stream::is_answering(c.id));
    }

    #[test]
    fn keep_it_adds_an_ordinary_exchange_and_waits_while_the_thread_answers() {
        let c = conv_with(&[("user", "When do I sow runner beans?"), ("assistant", "Mid-May, outdoors.")]);
        let tokens = MessageTokens { prompt: 120, answer: 9, ..Default::default() };
        let (q, a) = keep(&c, "Companion plant?", "Marigolds.", Some(model()), Some(tokens)).unwrap();
        assert_eq!((q, a), (2, 3));
        let after = crate::chat::chats::get(c.id).unwrap();
        assert_eq!((after.messages[2].role.as_str(), after.messages[2].text.as_str()), ("user", "Companion plant?"));
        assert_eq!((after.messages[3].role.as_str(), after.messages[3].text.as_str()), ("assistant", "Marigolds."));
        assert_eq!(after.messages[3].model.as_ref().map(|m| m.id.as_str()), Some("stand-in"));
        assert_eq!(after.messages[3].tokens, Some(tokens));
        // A turn answering in the chat: Keep it waits.
        let turn = stream::Stoppable::new(None);
        turn.answers(c.id);
        assert!(busy(c.id));
        assert_eq!(chat_side_keep(c.id, "Again?".into(), "Yes.".into(), None, None).unwrap_err(), KEEP_LATER);
        drop(turn);
        assert!(!busy(c.id));
    }

    #[test]
    fn a_code_chats_session_hears_of_what_was_kept_once_and_only_while_it_is_there() {
        let c = conv_with(&[("user", "Add a minimap"), ("assistant", "Done.")]);
        // (As `keep` notes it for a Code chat.)
        KEPT.lock().unwrap().get_or_insert_with(HashMap::new).entry(c.id).or_default().push(Kept { created: c.created_ts, at: 2, question: "Which file?".into(), answer: "map.js".into() });
        crate::chat::chats::append(c.id, "user", "Which file?", &[], None, false).unwrap();
        crate::chat::chats::append(c.id, "assistant", "map.js", &[], None, false).unwrap();
        let sent = with_kept(c.id, "Make it round".into());
        assert!(sent.starts_with("Make it round\n\n<earlier_in_this_chat>"), "{sent}");
        assert!(sent.contains("User: Which file?") && sent.contains("Assistant: map.js"));
        // Told once.
        assert_eq!(with_kept(c.id, "And bigger".into()), "And bigger");
        // One a Rewind cut from the thread: never told.
        KEPT.lock().unwrap().get_or_insert_with(HashMap::new).entry(c.id).or_default().push(Kept { created: c.created_ts, at: 9, question: "Gone?".into(), answer: "Yes.".into() });
        assert_eq!(with_kept(c.id, "Next".into()), "Next");
    }
}

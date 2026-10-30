//! **Chat's context ring** (2026-10-25, after Claude's own chat box): how
//! much of the Chat model's context the open conversation uses
//! ([`chat_context`]).
//!
//! - **Used**: the conversation's last answer's own last model call — its
//!   prompt (instructions, history, question, the tools' results) plus the
//!   answer, as the provider counted them (`ChatMessage::tokens`, kept by
//!   the turn: `usage::last_call`); messages after it are estimated and
//!   added. Before any answer, after an older Yap's save dropped the
//!   counts, for a model that doesn't say — or once the older messages were
//!   summarised after it (`chat/compact.rs`: the next request is smaller,
//!   and the ring drops) — **an estimate of what the next question sends**:
//!   the parts measured as a turn measures them (`turn::stable_prompt`,
//!   the tools' schemas) and the history as the turn chooses it
//!   (`chat/budget.rs` `plan`: the summary and what came after it, or the
//!   latest that fit), counted the same way (`budget::Counter`).
//! - **The context** of the model the next message goes to (the model
//!   chip's, so a switch in the picker changes it): `budget::context_window`
//!   — the on-device model's own, a self-hosted server's word, a cloud
//!   model's from its provider's list; else the provider's usual, marked
//!   approximate.
//! - **Summarised**: the next request carries the summary of the older
//!   messages (the panel: "Summarised · the chat stays within the model's
//!   context"). **Trimmed**: older messages are left out with nothing in
//!   their place (no summary yet).

use serde::Serialize;

use crate::chat::budget::{self, Counter, Kind};
use crate::chat::chats::{ChatMessage, ChatModel, ChatSummary};

/// A conversation's context use.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Use {
    pub used: u64,
    /// All or part of `used` is an estimate (no count from the model).
    pub estimate: bool,
    /// The last answer's own tokens (when it was counted).
    pub last_answer: Option<u64>,
    /// How fast the last answer was written (tokens a second), when said.
    pub per_second: Option<f64>,
}

/// The tokens of `messages` as a turn sends them, estimated (their text and
/// each one's overhead). Pure.
pub fn estimate_messages(messages: &[ChatMessage]) -> u64 {
    messages.iter().map(|m| budget::estimate(&m.text) + budget::MESSAGE_OVERHEAD).sum()
}

/// How much of a model's context `messages` use: the last counted answer's
/// prompt and answer, plus an estimate of anything after it; with none
/// counted — or a summary written after it (the next request is smaller)
/// — `next`, the estimate of the next request. Pure.
pub fn use_of(messages: &[ChatMessage], summary: Option<&ChatSummary>, next: u64) -> Use {
    if messages.is_empty() {
        return Use::default();
    }
    let last = messages.iter().rposition(|m| m.role == "assistant").and_then(|i| messages[i].tokens);
    let (last_answer, per_second) = (last.map(|t| t.answer), last.and_then(|t| t.per_second));
    match messages.iter().rposition(|m| m.role == "assistant" && m.tokens.is_some()) {
        Some(i) if summary.is_none_or(|s| s.after <= i) => {
            let t = messages[i].tokens.unwrap_or_default();
            let after = &messages[i + 1..];
            Use { used: t.prompt + t.answer + estimate_messages(after), estimate: !after.is_empty(), last_answer, per_second }
        }
        _ => Use { used: next, estimate: true, last_answer, per_second },
    }
}

/// What the ring shows ([`chat_context`]).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextUse {
    #[serde(flatten)]
    pub used: Use,
    /// The model's context, in tokens (0: no model set up).
    pub context: u64,
    /// The context came from the model or its server (else a provider's
    /// usual one: approximate).
    pub context_known: bool,
    /// The next question leaves older messages out with nothing in their
    /// place (no summary yet).
    pub trimmed: bool,
    /// The next question carries the summary of the older messages.
    pub summarised: bool,
    pub messages: usize,
    /// The most the next question may be, in tokens.
    pub question_max: u64,
    /// The model the next message goes to (the context is its).
    pub model: Option<ChatModel>,
}

/// The open conversation's context use against the Chat model's context
/// (none open: nothing used). The page asks after each answer, when the
/// model changes, when another conversation opens and when its summary is
/// written (`yap-chat-summary`).
#[tauri::command]
pub async fn chat_context(conversation_id: Option<u64>) -> ContextUse {
    let cfg = crate::config::load();
    let on_device = crate::ai::ai_model::OnDevice::here(&cfg);
    let Ok(target) = crate::ai::ai_model::target(&cfg, crate::ai::ai_model::Scope::Chat, &on_device) else {
        return ContextUse::default();
    };
    let conv = conversation_id.and_then(crate::chat::chats::get);
    let messages: &[ChatMessage] = conv.as_ref().map(|c| c.messages.as_slice()).unwrap_or_default();
    let counter = Counter::for_call(&target.provider, &target.base_url);
    let (context, context_known) = budget::context_window(&cfg, &target.provider, &target.model, &target.base_url).await;
    // The parts every request carries, as `chat_limits` measures them.
    let fixed = fixed_of(&cfg, &target, &counter).await;
    let b = budget::budget(context, context_known, fixed, Kind::Chat, target.tools, budget::request_cap(&target.provider, &target.model));
    let replay = target.tools && cfg.web_search;
    let costs = budget::message_costs(&counter, messages, replay).await;
    let summary = conv.as_ref().and_then(|c| c.usable_summary());
    let summary_cost = match summary {
        Some(s) => Some((s.upto, counter.count(&crate::chat::compact::block(&s.text)).await)),
        None => None,
    };
    let plan = budget::plan(&costs, summary_cost, crate::chat::turn::next_room(b.history), Kind::Chat.step());
    let used = use_of(messages, summary, fixed + plan.tokens);
    ContextUse {
        used,
        context,
        context_known,
        trimmed: plan.left_out > 0,
        summarised: plan.summary,
        messages: messages.len(),
        question_max: b.history,
        model: Some(target.chat_model()),
    }
}

/// The parts of a Chat request that don't change with the question, for
/// `target`: the stable prompt and the tools' schemas.
async fn fixed_of(cfg: &crate::config::YapConfig, target: &crate::ai::ai_model::Target, counter: &Counter) -> u64 {
    let persona = cfg
        .llm_scopes
        .get("chat")
        .map(|s| s.prompt.clone())
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| "You are a helpful assistant. Answer concisely.".to_string());
    let calendar_on = crate::calendar::connected();
    let profile = crate::ai::about_you::profile(cfg);
    let stable = crate::chat::turn::stable_prompt(cfg, &persona, target.tools, cfg.web_search, calendar_on, &profile, crate::ai::eval::now());
    let log = crate::chat::tools::calendar::Log::default();
    let extra = crate::chat::tools::Extra {
        memory: true,
        search_chats: cfg.memory_search_chats,
        conversation: None,
        calendar: calendar_on.then_some(crate::chat::tools::CalendarTurn { log: &log, zone: crate::chat::tools::calendar::Zone::of(None), now: 0 }),
        room: None,
        docs: None,
    };
    let schemas = if target.tools { counter.count(&crate::chat::tools::offered_tools(cfg.open_apps, cfg.web_search, extra).to_string()).await } else { 0 };
    counter.count(&stable).await + schemas
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::chats::MessageTokens;

    fn said(role: &str, text: &str) -> ChatMessage {
        ChatMessage { role: role.into(), text: text.into(), ..Default::default() }
    }

    fn answered(text: &str, prompt: u64, answer: u64) -> ChatMessage {
        ChatMessage { tokens: Some(MessageTokens { prompt, answer, per_second: Some(118.5) }), ..said("assistant", text) }
    }

    #[test]
    fn a_counted_answer_is_the_conversations_use() {
        let chat = vec![said("user", "When is the launch?"), answered("Friday.", 1_200, 300)];
        let u = use_of(&chat, None, 999);
        assert_eq!(u, Use { used: 1_500, estimate: false, last_answer: Some(300), per_second: Some(118.5) });
        // A question after it (its answer failed): estimated on top.
        let mut more = chat.clone();
        more.push(said("user", &"x".repeat(400)));
        let u = use_of(&more, None, 999);
        assert!(u.estimate);
        assert_eq!(u.used, 1_500 + budget::estimate(&"x".repeat(400)) + budget::MESSAGE_OVERHEAD);
    }

    #[test]
    fn with_no_count_or_a_newer_summary_the_next_request_is_estimated() {
        assert_eq!(use_of(&[], None, 500), Use::default());
        // An answer from a model that didn't say: the next request's estimate.
        let chat = vec![said("user", &"a".repeat(4_000)), said("assistant", &"b".repeat(4_000))];
        let u = use_of(&chat, None, 2_600);
        assert!(u.estimate);
        assert_eq!((u.used, u.last_answer), (2_600, None));
        // The older messages summarised after the last counted answer: the
        // next request carries the summary instead, and the ring drops.
        let chat = vec![said("user", "Plan the offsite"), answered("Sure.", 7_000, 200), said("user", "And the budget?"), answered("£18,400.", 7_400, 50)];
        let summary = ChatSummary { text: "- Budget £18,400".into(), upto: 2, after: 4, model: None, ts: 1 };
        let u = use_of(&chat, Some(&summary), 1_900);
        assert_eq!((u.used, u.estimate, u.last_answer), (1_900, true, Some(50)));
        // An answer counted after the summary: its count again.
        let mut later = chat.clone();
        later.push(said("user", "Who's organising it?"));
        later.push(answered("Marta Quill.", 2_100, 20));
        assert_eq!(use_of(&later, Some(&summary), 1_900).used, 2_120);
    }
}

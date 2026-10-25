//! **Chat's context ring** (2026-10-25, after Claude's own chat box): how
//! much of the Chat model's context the open conversation uses
//! ([`chat_context`]).
//!
//! - **Used**: the conversation's last answer's own last model call — its
//!   prompt (instructions, history, question, the tools' results) plus the
//!   answer, as the provider counted them (`ChatMessage::tokens`, kept by
//!   the turn: `usage::last_call`). Before any answer, after an older Yap's
//!   save dropped them, or for a model that doesn't say, an **estimate**
//!   of what the next question would send ([`estimate_next`]: the history
//!   Yap sends, `chats::history_messages`, and the instructions, at about
//!   four characters a token); messages after the last counted answer are
//!   estimated and added.
//! - **The context** of the model the next message goes to (the model chip's,
//!   so a switch in the picker changes it): the on-device model's own
//!   (`local_llm::context`, what the sidecar serves), a self-hosted server's
//!   word (Ollama, LM Studio: `model_catalog::server_context_here`), a cloud
//!   model's from its provider's list (`model_catalog`'s `context`); none
//!   of those: [`default_context`] for the provider, marked approximate.
//! - **Trimmed**: Chat sends only the latest messages
//!   (`chats::CHAT_HISTORY`, 14–19); the ring's panel says a long chat is
//!   trimmed and offers a new one when it's nearly full.

use serde::Serialize;

use crate::chat::chats::{ChatMessage, ChatModel};

/// The start of every Chat request before its history: the persona, the
/// line on the context block, About you (about 300 tokens).
const PROMPT_START: u64 = 300;

/// About four characters a token. Pure.
pub fn estimate_tokens(chars: usize) -> u64 {
    (chars as u64).div_ceil(4)
}

/// [`estimate_tokens`] of `messages` as Chat sends them (an earlier web
/// answer's search replayed: `chats::history_messages`). Pure.
pub fn estimate_messages(messages: &[ChatMessage]) -> u64 {
    if messages.is_empty() {
        return 0;
    }
    let sent = serde_json::to_string(&crate::chat::chats::history_messages(messages)).unwrap_or_default();
    estimate_tokens(sent.chars().count())
}

/// What a Chat request carries before its history: [`PROMPT_START`], and a
/// tool model's instructions and schemas.
pub fn prompt_allowance(tools: bool) -> u64 {
    PROMPT_START + if tools { estimate_tokens(crate::chat::tools::tool_prompt_chars()) } else { 0 }
}

/// What the next question in a conversation of `messages` would send, about:
/// the instructions and the history window (`chats::CHAT_HISTORY`).
pub fn estimate_next(messages: &[ChatMessage], tools: bool) -> u64 {
    let (keep, step) = crate::chat::chats::CHAT_HISTORY;
    let window = &messages[crate::chat::chats::history_start(messages.len(), keep, step)..];
    prompt_allowance(tools) + estimate_messages(window)
}

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

/// How much of a model's context `messages` use: the last counted
/// answer's prompt and answer, plus an estimate of anything after it; with
/// none counted, [`estimate_next`]. `tools`: the model gets Chat's tools
/// (their instructions count). Pure.
pub fn use_of(messages: &[ChatMessage], tools: bool) -> Use {
    if messages.is_empty() {
        return Use::default();
    }
    let last = messages.iter().rposition(|m| m.role == "assistant").and_then(|i| messages[i].tokens);
    let (last_answer, per_second) = (last.map(|t| t.answer), last.and_then(|t| t.per_second));
    match messages.iter().rposition(|m| m.role == "assistant" && m.tokens.is_some()) {
        Some(i) => {
            let t = messages[i].tokens.unwrap_or_default();
            let after = &messages[i + 1..];
            Use { used: t.prompt + t.answer + estimate_messages(after), estimate: !after.is_empty(), last_answer, per_second }
        }
        None => Use { used: estimate_next(messages, tools), estimate: true, last_answer, per_second },
    }
}

/// A provider's context when nothing says (its list didn't have the model,
/// a server didn't say): the page calls it approximate. Pure.
pub fn default_context(provider: &str) -> u64 {
    match provider {
        "groq" => 131_072,
        "anthropic" => 200_000,
        "gemini" => 1_048_576,
        "openai" | "openrouter" => 128_000,
        // Ollama's and LM Studio's own defaults are smaller still.
        _ => 8_192,
    }
}

/// The context of the model `t` (the chip's), and whether that came from
/// the model or its server (else it's [`default_context`]).
async fn context_of(cfg: &crate::config::YapConfig, t: &crate::ai::ai_model::Target, on_device: u32) -> (u64, bool) {
    if t.provider == crate::ai::local_llm::PROVIDER_ONDEVICE && on_device > 0 {
        return (u64::from(on_device), true);
    }
    if matches!(t.provider.as_str(), "local" | "custom") {
        return match crate::ai::model_catalog::server_context_here(cfg, &t.provider, &t.base_url, &t.model).await {
            Some(n) => (n, true),
            None => (default_context(&t.provider), false),
        };
    }
    match crate::ai::model_catalog::listed_model(&t.provider, &t.model).and_then(|m| m.context) {
        Some(n) if n > 0 => (n, true),
        _ => (default_context(&t.provider), false),
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
    /// The next question leaves older messages out (`chats::CHAT_HISTORY`).
    pub trimmed: bool,
    pub messages: usize,
    /// The model the next message goes to (the context is its).
    pub model: Option<ChatModel>,
}

/// The open conversation's context use against the Chat model's context
/// (none open: nothing used). The page asks after each answer, when the
/// model changes and when another conversation opens.
#[tauri::command]
pub async fn chat_context(conversation_id: Option<u64>) -> ContextUse {
    let cfg = crate::config::load();
    let on_device = crate::ai::ai_model::OnDevice::here(&cfg);
    let target = crate::ai::ai_model::target(&cfg, crate::ai::ai_model::Scope::Chat, &on_device).ok();
    let messages = conversation_id.and_then(crate::chat::chats::get).map(|c| c.messages).unwrap_or_default();
    let used = use_of(&messages, target.as_ref().is_some_and(|t| t.tools));
    let (context, context_known) = match &target {
        Some(t) => context_of(&cfg, t, on_device.context).await,
        None => (0, false),
    };
    let (keep, step) = crate::chat::chats::CHAT_HISTORY;
    ContextUse {
        used,
        context,
        context_known,
        trimmed: crate::chat::chats::history_start(messages.len(), keep, step) > 0,
        messages: messages.len(),
        model: target.map(|t| t.chat_model()),
    }
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
    fn about_four_characters_a_token() {
        assert_eq!(estimate_tokens(0), 0);
        assert_eq!(estimate_tokens(1), 1);
        assert_eq!(estimate_tokens(4_000), 1_000);
        assert_eq!(estimate_tokens(4_001), 1_001);
    }

    #[test]
    fn a_counted_answer_is_the_conversations_use() {
        let chat = vec![said("user", "When is the launch?"), answered("Friday.", 1_200, 300)];
        let u = use_of(&chat, true);
        assert_eq!(u, Use { used: 1_500, estimate: false, last_answer: Some(300), per_second: Some(118.5) });
        // A question after it (its answer failed): estimated on top.
        let mut more = chat.clone();
        more.push(said("user", &"x".repeat(400)));
        let u = use_of(&more, true);
        assert!(u.estimate);
        assert!(u.used > 1_500 + 100 && u.used < 1_500 + 130, "{}", u.used);
    }

    #[test]
    fn with_no_count_the_next_request_is_estimated() {
        assert_eq!(use_of(&[], true), Use::default());
        // An answer from a model that didn't say: the history and the
        // instructions, about four characters a token.
        let chat = vec![said("user", &"a".repeat(4_000)), said("assistant", &"b".repeat(4_000))];
        let small = use_of(&chat, false);
        assert!(small.estimate);
        assert!(small.used >= PROMPT_START + 2_000 && small.used < PROMPT_START + 2_100, "{}", small.used);
        assert_eq!(small.last_answer, None);
        // A model with tools carries their instructions too.
        let tools = use_of(&chat, true);
        assert!(tools.used > small.used + 500, "{} vs {}", tools.used, small.used);
        // Only the window Chat sends counts: 40 messages ≈ the last 14–19.
        let long: Vec<ChatMessage> = (0..40).map(|i| said(if i % 2 == 0 { "user" } else { "assistant" }, &"c".repeat(400))).collect();
        let u = use_of(&long, false);
        assert!(u.used < PROMPT_START + 20 * 130, "{}", u.used);
    }

    #[test]
    fn a_provider_without_a_word_on_the_model_has_its_usual_context() {
        assert_eq!(default_context("groq"), 131_072);
        assert_eq!(default_context("gemini"), 1_048_576);
        assert_eq!(default_context("local"), 8_192);
        assert_eq!(default_context("custom"), 8_192);
    }
}

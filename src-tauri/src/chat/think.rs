//! **Think it through** (2026-10-29): Chat's Think button asks one question
//! with the model thinking before it answers. Every other call — dictation
//! cleanup, Commands, the meeting notes, the Voice Assistant, Chat's own
//! questions without Think — stays as it was.
//!
//! **On this PC** (Qwen3.5, the sidecar): llamafile runs with `--reasoning
//! off` (`local_llm::server_args`: Qwen3.5 thinking by default looped and
//! slowed every cleanup), and **one request can still think, without a
//! restart** — measured on llamafile 0.10.6 with Qwen3.5-4B (Q4_K_M, MTP on,
//! `--reasoning off`): `chat_template_kwargs: {"enable_thinking": true}`
//! turns it on for that request alone (the next one, without it, answers at
//! once as before); `thinking_budget_tokens` caps it — without a cap the 4B
//! thought for 4,096 tokens about the bat and the ball and never answered,
//! with 300 it answered in 1.1 s, with 1,500 a logic puzzle took ~1,350
//! tokens and 5.3 s — and `reasoning_budget_message` is the line it's given
//! when the budget runs out, which keeps the forced end tag out of the
//! answer (without it, the answer began "</think>"). `reasoning_effort` does
//! nothing there, and `reasoning_budget_tokens` didn't cap it (the thinking
//! spilt into the answer). The thinking comes back apart from the answer,
//! as `reasoning_content` (streamed: its own deltas). So: [`ON_DEVICE_BUDGET`]
//! tokens, the answer's cap raised by as much.
//!
//! **In the cloud**, a model that can reason reasons harder
//! ([`request_extra`]): OpenAI's reasoning models and Groq's gpt-oss
//! `reasoning_effort: "high"`, Gemini's thinking models the same (its
//! OpenAI-compatible endpoint maps it to a thinking budget), OpenRouter's
//! `reasoning: {effort: "high"}`, and Claude adaptive thinking at `high`
//! effort (Anthropic's OpenAI-compatible endpoint takes `thinking`; it
//! ignores `reasoning_effort`; Claude 5 thinks by default anyway) — or a
//! budget on a Claude older than 4.6. A request that thinks waits longer
//! ([`TIMEOUT`]).
//!
//! **What shows**: "Thinking…" while it thinks, then the thinking folded
//! above the answer, "Thought for 6 s" (`Event::Thinking`, `ChatMessage.
//! thinking`) — only what the model sends back: Anthropic's compatible
//! endpoint and OpenAI's never send it, so there it's only the time.
//!
//! The turn's state is a task-local around `chat_send`'s turn ([`with`]),
//! read where a Chat request is made (`llm::ChatRequest::chat`) — never by
//! a call outside it.

use std::future::Future;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// The most the on-device model thinks for one question, in tokens (~5 s on
/// an RTX 5070 Ti, at ~250 tokens a second with MTP).
pub const ON_DEVICE_BUDGET: u32 = 1_500;

/// What the on-device model is told when its budget runs out, before it
/// answers.
pub const BUDGET_MESSAGE: &str = "\n\nI've thought about this enough. Time to answer.\n";

/// The most a request that thinks waits (a cloud model at high effort can
/// take a minute or more).
pub const TIMEOUT: Duration = Duration::from_secs(150);

/// How long a model thought, and what it thought when it said.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Thought {
    /// The thinking, as the model sent it back ("" when it doesn't: Claude
    /// and OpenAI through their Chat Completions endpoints).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// How long it thought, from its first thought to its last (0: not
    /// known — a whole answer).
    #[serde(default)]
    pub ms: u64,
}

/// A Claude before adaptive thinking (4.6): it thinks to a budget. Pure.
fn claude_with_budget(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    ["claude-3", "claude-opus-4-0", "claude-opus-4-1", "claude-opus-4-5", "claude-sonnet-4-0", "claude-sonnet-4-5", "claude-haiku-4"]
        .iter()
        .any(|p| m.starts_with(p))
        || m == "claude-opus-4"
        || m == "claude-sonnet-4"
        || m.starts_with("claude-opus-4-2")
        || m.starts_with("claude-sonnet-4-2")
}

/// OpenAI's reasoning models (o-series, GPT-5 and later; not the "chat"
/// ones). Pure.
fn openai_reasons(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    let m = m.rsplit('/').next().unwrap_or(&m);
    let family = m.starts_with("o1") || m.starts_with("o3") || m.starts_with("o4") || m.starts_with("gpt-5") || m.starts_with("gpt-6");
    family && !m.contains("-chat") && !m.contains("o1-mini")
}

/// Gemini's thinking models (2.5 and later). Pure.
fn gemini_thinks(model: &str) -> bool {
    let m = model.to_ascii_lowercase();
    let m = m.trim_start_matches("models/");
    ["gemini-2.5", "gemini-3"].iter().any(|p| m.starts_with(p)) && !m.contains("image") && !m.contains("tts")
}

/// A model OpenRouter lists as reasoning, or one of the families above.
fn openrouter_reasons(model: &str) -> bool {
    if let Some(listed) = crate::ai::model_catalog::listed_model("openrouter", model) {
        if let Some(thinks) = listed.thinks {
            return thinks;
        }
    }
    let m = model.to_ascii_lowercase();
    let id = m.rsplit('/').next().unwrap_or(&m);
    m.contains("gpt-oss") || openai_reasons(id) || gemini_thinks(id) || id.starts_with("claude") || m.contains("deepseek-r1") || m.contains("qwen3")
}

/// The on-device model thinks when asked (Qwen3.5 and its family). Pure.
pub fn on_device_thinks(model: &str) -> bool {
    let m = model.to_ascii_lowercase().replace('_', ".");
    m.contains("qwen3.5") || m.contains("qwen3.6")
}

/// Is this Yap's own llamafile, run as the sidecar runs it? The on-device
/// model — and a model evaluation's `local:` server (`eval/local.rs`, the
/// sidecar's own arguments).
fn yaps_llamafile(provider: &str) -> bool {
    provider == crate::ai::local_llm::PROVIDER_ONDEVICE || (provider == "local" && crate::ai::eval::active())
}

/// **What a request adds to think it through** on `provider`'s `model`;
/// `None`: it can't (the Think button says so). Pure, but for OpenRouter's
/// list.
pub fn request_extra(provider: &str, model: &str) -> Option<Map<String, Value>> {
    let lower = model.to_ascii_lowercase();
    let fields = match provider {
        p if yaps_llamafile(p) && on_device_thinks(model) => json!({
            "chat_template_kwargs": { "enable_thinking": true },
            "thinking_budget_tokens": ON_DEVICE_BUDGET,
            "reasoning_budget_message": BUDGET_MESSAGE,
        }),
        "openai" if openai_reasons(model) => json!({ "reasoning_effort": "high" }),
        "groq" if lower.contains("gpt-oss") => json!({ "reasoning_effort": "high" }),
        "gemini" if gemini_thinks(model) => json!({ "reasoning_effort": "high" }),
        "openrouter" if openrouter_reasons(model) => json!({ "reasoning": { "effort": "high" } }),
        "anthropic" if lower.starts_with("claude") => {
            if claude_with_budget(model) {
                json!({ "thinking": { "type": "enabled", "budget_tokens": 4_096 }, "max_tokens": 16_000 })
            } else {
                json!({ "thinking": { "type": "adaptive" }, "output_config": { "effort": "high" } })
            }
        }
        _ => return None,
    };
    fields.as_object().cloned()
}

/// Whether `provider`'s `model` can think it through (the Think button).
pub fn can_think(provider: &str, model: &str) -> bool {
    request_extra(provider, model).is_some()
}

// ---- the turn ------------------------------------------------------------------------------

#[derive(Default)]
struct Turn {
    text: Mutex<String>,
    first: Mutex<Option<Instant>>,
    last: Mutex<Option<Instant>>,
}

tokio::task_local! {
    static TURN: Turn;
}

/// Run a Chat turn thinking it through (`on`), and say what it thought.
/// Off: just the turn, as ever.
pub async fn with<F: Future>(on: bool, f: F) -> (F::Output, Option<Thought>) {
    if !on {
        return (f.await, None);
    }
    TURN.scope(Turn::default(), async {
        let out = f.await;
        let thought = TURN.with(|t| {
            let text = t.text.lock().unwrap_or_else(|p| p.into_inner()).trim().to_string();
            let first = *t.first.lock().unwrap_or_else(|p| p.into_inner());
            let last = *t.last.lock().unwrap_or_else(|p| p.into_inner());
            let ms = match (first, last) {
                (Some(a), Some(b)) => b.saturating_duration_since(a).as_millis() as u64,
                _ => 0,
            };
            Thought { text, ms }
        });
        (out, Some(thought))
    })
    .await
}

/// Is the turn running here thinking it through?
pub fn wanted() -> bool {
    TURN.try_with(|_| ()).is_ok()
}

/// A Chat request's fields to think, when its turn is thinking it through
/// and the model can (`llm::ChatRequest::chat`).
pub fn extra_for(provider: &str, model: &str) -> Option<Map<String, Value>> {
    if !wanted() {
        return None;
    }
    request_extra(provider, model)
}

/// A piece of thinking came (a streamed request): the clock.
pub fn tick() {
    let _ = TURN.try_with(|t| {
        let now = Instant::now();
        t.first.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert(now);
        *t.last.lock().unwrap_or_else(|p| p.into_inner()) = Some(now);
    });
}

/// A request's thinking, whole, once it's done (`llm::post_chat_request`).
pub fn heard(reasoning: &str) {
    let reasoning = reasoning.trim();
    if reasoning.is_empty() {
        return;
    }
    let _ = TURN.try_with(|t| {
        let mut text = t.text.lock().unwrap_or_else(|p| p.into_inner());
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str(reasoning);
    });
}

/// The turn's thinking so far: its earlier requests' (a tool loop thinks at
/// each step).
pub fn so_far() -> String {
    TURN.try_with(|t| t.text.lock().unwrap_or_else(|p| p.into_inner()).clone()).unwrap_or_default()
}

/// A message's thinking, as the server sent it apart from the answer
/// (`reasoning_content`: llama.cpp, DeepSeek; `reasoning`: Groq, OpenRouter).
/// Pure.
pub fn reasoning_of(message: &Value) -> &str {
    message["reasoning_content"].as_str().or_else(|| message["reasoning"].as_str()).unwrap_or("")
}

/// An answer without the end of a thinking budget, should one slip into it
/// ("</think>" before the answer). Pure.
pub fn tidy(answer: &str) -> String {
    let t = answer.trim_start();
    match t.strip_prefix("</think>") {
        Some(rest) => rest.trim_start().to_string(),
        None => answer.to_string(),
    }
}

/// A message of a turn thinking it through, its answer [`tidy`]; any other
/// as it is.
pub fn tidy_message(mut message: Value) -> Value {
    if wanted() {
        if let Some(c) = message["content"].as_str() {
            let t = tidy(c);
            if t != c {
                message["content"] = json!(t);
            }
        }
    }
    message
}

/// What the Think button says for Chat's model now (`can`: it shows; the
/// label: what thinking does on it).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThinkInfo {
    pub can: bool,
    /// "on this PC" | "cloud": the hint says how.
    pub place: &'static str,
}

/// The Think button for Chat's model now.
#[tauri::command]
pub fn chat_think_info() -> ThinkInfo {
    let cfg = crate::config::load();
    let Ok((base_url, _key, model, provider, _persona, _thinking)) = crate::chat::turn::resolve_chat_endpoint(&cfg) else {
        return ThinkInfo::default();
    };
    ThinkInfo {
        can: can_think(&provider, &model),
        place: if crate::ai::ai_model::runs_here(&provider, &base_url) { "here" } else { "cloud" },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_on_device_qwen_thinks_to_a_budget_with_its_server_unchanged() {
        let x = request_extra("ondevice", "Qwen_Qwen3.5-4B-Q4_K_M").unwrap();
        assert_eq!(x["chat_template_kwargs"], json!({ "enable_thinking": true }));
        assert_eq!(x["thinking_budget_tokens"], json!(ON_DEVICE_BUDGET));
        assert!(x["reasoning_budget_message"].as_str().unwrap().contains("answer"));
        assert!((1_000..=2_000).contains(&ON_DEVICE_BUDGET));
        // Its family; not the 1.5B, nor Qwen3's 2507 instruct (no thinking).
        assert!(can_think("ondevice", "qwen3.5-9b-q4_k_m"));
        assert!(!can_think("ondevice", "qwen2.5-1.5b-instruct-q4_k_m"));
        assert!(!can_think("ondevice", "Qwen_Qwen3-4B-Instruct-2507-Q4_K_M"));
    }

    #[test]
    fn cloud_models_that_reason_reason_harder() {
        assert_eq!(request_extra("groq", "openai/gpt-oss-20b").unwrap()["reasoning_effort"], "high");
        assert!(request_extra("groq", "llama-3.3-70b-versatile").is_none());
        assert_eq!(request_extra("openai", "gpt-5.6-luna").unwrap()["reasoning_effort"], "high");
        assert_eq!(request_extra("openai", "o4-mini").unwrap()["reasoning_effort"], "high");
        assert!(request_extra("openai", "gpt-4o").is_none());
        assert!(request_extra("openai", "gpt-5-chat-latest").is_none());
        assert_eq!(request_extra("gemini", "gemini-3.6-flash").unwrap()["reasoning_effort"], "high");
        assert!(request_extra("gemini", "gemini-2.0-flash").is_none());
        assert_eq!(request_extra("openrouter", "openai/gpt-5-mini").unwrap()["reasoning"], json!({ "effort": "high" }));
        assert!(request_extra("openrouter", "meta-llama/llama-3.3-70b-instruct").is_none());
        // Claude: adaptive thinking at high effort; a budget before 4.6.
        let c = request_extra("anthropic", "claude-haiku-5-5").unwrap();
        assert_eq!(c["thinking"], json!({ "type": "adaptive" }));
        assert_eq!(c["output_config"], json!({ "effort": "high" }));
        assert!(c.get("reasoning_effort").is_none(), "Anthropic's compatible endpoint ignores it");
        let old = request_extra("anthropic", "claude-haiku-4-5").unwrap();
        assert_eq!(old["thinking"], json!({ "type": "enabled", "budget_tokens": 4096 }));
        assert!(old["max_tokens"].as_u64().unwrap() > 4096);
        // A self-hosted server: not known to think on request.
        assert!(request_extra("custom", "qwen3.5-4b").is_none());
        assert!(request_extra("local", "llama3").is_none());
    }

    #[tokio::test]
    async fn only_a_turn_that_thinks_asks_for_it_and_its_thoughts_come_back() {
        assert!(extra_for("ondevice", "Qwen_Qwen3.5-4B-Q4_K_M").is_none(), "outside a turn");
        let ((), none) = with(false, async {
            assert!(!wanted());
        })
        .await;
        assert!(none.is_none());
        let (asked, thought) = with(true, async {
            tick();
            heard("First, the ball.");
            heard("  ");
            heard("Then the bat.");
            tick();
            extra_for("ondevice", "Qwen_Qwen3.5-4B-Q4_K_M").is_some()
        })
        .await;
        assert!(asked);
        let thought = thought.unwrap();
        assert_eq!(thought.text, "First, the ball.\n\nThen the bat.");
        assert!(!wanted(), "over with the turn");
    }

    #[test]
    fn a_messages_thinking_and_a_stray_end_tag() {
        assert_eq!(reasoning_of(&json!({ "content": "A", "reasoning_content": "R" })), "R");
        assert_eq!(reasoning_of(&json!({ "content": "A", "reasoning": "G" })), "G");
        assert_eq!(reasoning_of(&json!({ "content": "A" })), "");
        assert_eq!(tidy("</think>\n\nThe ball costs 5 cents."), "The ball costs 5 cents.");
        assert_eq!(tidy("The ball costs 5 cents."), "The ball costs 5 cents.");
    }
}

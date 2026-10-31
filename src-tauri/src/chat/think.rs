//! **Think it through** (2026-10-29): Chat's Think button asks one question
//! with the model on this PC thinking before it answers. **Only the
//! on-device model** since 31 Oct 2026: a cloud model's thinking is its
//! **effort**, chosen per model in Chat's model picker (`ai/effort.rs`), so
//! the button shows for the on-device Qwen alone. Every other call —
//! dictation cleanup, Commands, the meeting notes, the Voice Assistant,
//! Chat's own questions without Think — stays as it was.
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
//! **In the cloud**, every Chat question goes at its model's effort
//! (`effort::apply`, from `llm::ChatRequest::chat`), and the turn here
//! hears that it did ([`reasoned`]): what the model thought shows as Think's
//! did. A request that thinks waits longer ([`TIMEOUT`]).
//!
//! **What shows**: "Thinking…" while it thinks, then the thinking folded
//! above the answer, "Thought for 6 s" (`Event::Thinking`, `ChatMessage.
//! thinking`) — only what the model sends back (Claude's summary on the
//! Messages API, Groq's and OpenRouter's `reasoning`; OpenAI and Gemini's
//! compatible endpoint send none, and then nothing shows).
//!
//! The turn's state is a task-local around `chat_send`'s turn ([`scoped`]),
//! read where a Chat request is made (`llm::ChatRequest::chat`) — never by
//! a call outside it. (Until 31 Oct 2026 a strong cloud model writing a
//! document turned Think on by itself, at high effort; its effort covers
//! that now.)

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
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
/// `None`: it can't (no Think button) — every model but Yap's own Qwen3.5
/// (a cloud model has its effort instead, `ai/effort.rs`). Pure.
pub fn request_extra(provider: &str, model: &str) -> Option<Map<String, Value>> {
    if !(yaps_llamafile(provider) && on_device_thinks(model)) {
        return None;
    }
    json!({
        "chat_template_kwargs": { "enable_thinking": true },
        "thinking_budget_tokens": ON_DEVICE_BUDGET,
        "reasoning_budget_message": BUDGET_MESSAGE,
    })
    .as_object()
    .cloned()
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
    /// A request of the turn asked the on-device model to think
    /// ([`extra_for`]): not so when Yap answered by itself (an app opened,
    /// the passages with no model) or the model can't think on request.
    asked: AtomicBool,
    /// A request of the turn went at its cloud model's effort
    /// ([`reasoned`]): what it thought shows.
    reasoned: AtomicBool,
    /// The person pressed Think.
    on: AtomicBool,
}

tokio::task_local! {
    static TURN: Turn;
}

/// Run a Chat turn thinking it through (`on`), and say what it thought —
/// `None` when no request of it asked the model to think. Off: just the
/// turn, as ever (nothing can turn it on: [`scoped`] can). The screenshot
/// evaluations' way in (`eval/vision.rs`); Chat's own is [`scoped`].
#[cfg(any(test, debug_assertions))]
pub async fn with<F: Future>(on: bool, f: F) -> (F::Output, Option<Thought>) {
    if !on {
        return (f.await, None);
    }
    scoped(true, f).await
}

/// **Chat's own turn** (`chat_send`): the on-device model thinking it
/// through when the person pressed Think (`on`), a cloud model at its
/// effort — and what it thought: `None` when no request asked the model
/// on this PC to think and a cloud model sent no thinking back (OpenAI,
/// Gemini: then no "Thought" fold, nothing in it to show).
pub async fn scoped<F: Future>(on: bool, f: F) -> (F::Output, Option<Thought>) {
    let turn = Turn::default();
    turn.on.store(on, Ordering::SeqCst);
    TURN.scope(turn, async {
        let out = f.await;
        let thought = TURN.with(|t| {
            let asked = t.asked.load(Ordering::SeqCst);
            if !asked && !t.reasoned.load(Ordering::SeqCst) {
                return None;
            }
            let text = t.text.lock().unwrap_or_else(|p| p.into_inner()).trim().to_string();
            if !asked && text.is_empty() {
                return None;
            }
            let first = *t.first.lock().unwrap_or_else(|p| p.into_inner());
            let last = *t.last.lock().unwrap_or_else(|p| p.into_inner());
            let ms = match (first, last) {
                (Some(a), Some(b)) => b.saturating_duration_since(a).as_millis() as u64,
                _ => 0,
            };
            Some(Thought { text, ms })
        });
        (out, thought)
    })
    .await
}

/// Did the person press Think for the turn running here?
pub fn wanted() -> bool {
    TURN.try_with(|t| t.on.load(Ordering::SeqCst)).unwrap_or(false)
}

/// **Does the turn running here show what its model thinks?** Think
/// pressed, or a request at a cloud model's effort. A model that sends its
/// reasoning anyway outside one (a Groq model asked by the Voice Assistant,
/// cleanup) shows none.
pub fn shows() -> bool {
    TURN.try_with(|t| t.on.load(Ordering::SeqCst) || t.reasoned.load(Ordering::SeqCst)).unwrap_or(false)
}

/// A request of the turn running here went at its cloud model's effort
/// (`llm::ChatRequest::chat`, `effort::apply`): its thinking shows. Outside
/// a turn, nothing.
pub fn reasoned() {
    let _ = TURN.try_with(|t| t.reasoned.store(true, Ordering::SeqCst));
}

/// A Chat request's fields to think, when the person pressed Think and the
/// model on this PC can (`llm::ChatRequest::chat`).
pub fn extra_for(provider: &str, model: &str) -> Option<Map<String, Value>> {
    if !wanted() {
        return None;
    }
    let extra = request_extra(provider, model);
    if extra.is_some() {
        let _ = TURN.try_with(|t| t.asked.store(true, Ordering::SeqCst));
    }
    extra
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
    if shows() {
        if let Some(c) = message["content"].as_str() {
            let t = tidy(c);
            if t != c {
                message["content"] = json!(t);
            }
        }
    }
    message
}

/// What the Think button says for Chat's model now (`can`: it shows — the
/// on-device model only; `place`: where it runs).
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
    fn a_cloud_model_has_no_think_its_effort_instead() {
        // Effort replaces Think in the cloud (ai/effort.rs): no button there.
        for (p, m) in [
            ("groq", "openai/gpt-oss-20b"),
            ("openai", "gpt-5.6-luna"),
            ("gemini", "gemini-3.6-flash"),
            ("openrouter", "openai/gpt-5-mini"),
            ("anthropic", "claude-opus-5-5"),
            ("anthropic", "claude-haiku-4-5"),
            ("custom", "qwen3.5-4b"),
            ("local", "llama3"),
        ] {
            assert!(!can_think(p, m), "{p} {m}");
        }
        assert!(can_think("ondevice", "Qwen_Qwen3.5-4B-Q4_K_M"));
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
        // Asked to think, but no request did (Yap opened an app, a model
        // that can't think on request): no "Thought it through" for it.
        let ((), none) = with(true, async {
            assert!(wanted());
            assert!(extra_for("custom", "llama3").is_none());
        })
        .await;
        assert!(none.is_none());
    }

    #[tokio::test]
    async fn a_turn_at_a_cloud_models_effort_shows_what_it_thought() {
        // Outside any turn (cleanup, the Voice Assistant): nothing shows.
        reasoned();
        assert!(!shows());
        // Chat's turn, Think not pressed: a request at an effort shows its
        // thinking, and the turn says what it thought.
        let (shown, thought) = scoped(false, async {
            assert!(!shows() && !wanted());
            assert!(extra_for("anthropic", "claude-opus-5-5").is_none(), "Think's fields: the model on this PC only");
            reasoned();
            heard("Levels first.");
            shows()
        })
        .await;
        assert!(shown);
        assert_eq!(thought.unwrap().text, "Levels first.");
        // At an effort, but nothing sent back (OpenAI, Gemini): no fold.
        let ((), none) = scoped(false, async { reasoned() }).await;
        assert!(none.is_none());
        // Neither: as ever.
        let ((), none) = scoped(false, async { heard("Groq sends it anyway.") }).await;
        assert!(none.is_none());
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

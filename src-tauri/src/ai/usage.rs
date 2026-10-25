//! **The AI usage tracker**: every AI call's tokens, counted on this PC
//! (2026-10-25). Counts only — the provider, the model, whether it ran on
//! this PC, the feature it was for, the prompt's and the answer's tokens
//! and, for a llama.cpp server (the on-device model), how fast it read and
//! wrote — never a word of what was asked or answered, in the store or the
//! log.
//!
//! **One place**: [`record_response`], called by the one chat call every AI
//! call goes through (`llm::post_chat_request`, streamed or not, the
//! on-device model's load included), by a provider's own web search
//! (`web/native.rs`) and by search by meaning's embeddings
//! (`search/embedder.rs`). It reads the counts the answer carries
//! ([`parse`]: OpenAI-compatible `usage`, Groq's `x_groq.usage`, Anthropic's
//! and OpenAI's `input_tokens`/`output_tokens`, Gemini's `usageMetadata`,
//! llama.cpp's `timings`) and adds them to:
//!
//! - **Groq's "Usage today" meter** (`groq.rs`, `groq_usage.json`), exactly
//!   as before: an older Yap reads that file too, so it's left as it was;
//! - **the daily store** (`store.rs`, `ai_usage.json`): per day, model and
//!   feature, kept 90 days — Chat's context ring panel and Insights' AI
//!   usage card read it ([`ai_usage_summary`]), with an estimated cost for
//!   cloud models whose price is known (`prices.rs`);
//! - **the turn's last call** ([`tallied`] / [`last_call`]): a Chat turn's
//!   last model call says how much of the model's context the conversation
//!   uses (`context.rs`, the ring), and is kept with the answer
//!   (`chats::ChatMessage::tokens`).
//!
//! **The feature** a call was for: the network receipts' scope
//! (`net::within`: Dictation → cleanup, Chat, the Voice Assistant, Commands,
//! Notes, Meetings, Brief, Memory), unless a finer one of this module's is
//! set around it ([`within`]: transforms, translation, speaker names).
//! Unscoped: "other".

pub mod context;
pub mod groq;
pub mod prices;
pub mod store;

use std::future::Future;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde_json::Value;

pub use groq::{set_app_handle, snapshot};

/// What a call was for (the store's `feature`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    /// A dictation's AI cleanup (and Prompt Studio's test).
    Cleanup,
    Chat,
    Assistant,
    /// Commands and the rewrite shortcut.
    Commands,
    /// A saved transform (Polish, Shorter…).
    Transforms,
    /// A language shortcut that translates.
    Translation,
    /// Meeting summaries, digests, the notepad's helpers.
    Meetings,
    /// A note's Enhance and its Ask bar.
    Notes,
    Memory,
    /// Who a meeting's voices are.
    Speakers,
    /// Pre-meeting briefs.
    Brief,
    /// Search by meaning's embeddings (on this PC).
    Search,
    Other,
}

impl Feature {
    pub fn id(self) -> &'static str {
        match self {
            Feature::Cleanup => "cleanup",
            Feature::Chat => "chat",
            Feature::Assistant => "assistant",
            Feature::Commands => "commands",
            Feature::Transforms => "transforms",
            Feature::Translation => "translation",
            Feature::Meetings => "meetings",
            Feature::Notes => "notes",
            Feature::Memory => "memory",
            Feature::Speakers => "speakers",
            Feature::Brief => "brief",
            Feature::Search => "search",
            Feature::Other => "other",
        }
    }

    /// The network receipts' feature, as the tracker counts it.
    fn of_net(f: crate::privacy::net::Feature) -> Feature {
        use crate::privacy::net::Feature as N;
        match f {
            N::Dictation | N::DictationContext => Feature::Cleanup,
            N::Commands => Feature::Commands,
            N::Chat => Feature::Chat,
            N::Assistant => Feature::Assistant,
            N::Notes => Feature::Notes,
            N::Meetings => Feature::Meetings,
            N::Brief => Feature::Brief,
            N::Memory => Feature::Memory,
        }
    }
}

tokio::task_local! {
    static FEATURE: Feature;
    static TALLY: Arc<Mutex<Option<LastCall>>>;
}

/// Run `fut` as `feature` for the tracker (finer than the network
/// receipts' scope it runs in: a transform is a dictation's to the
/// receipts, a transform here). The innermost wins.
pub async fn within<F: Future>(feature: Feature, fut: F) -> F::Output {
    FEATURE.scope(feature, fut).await
}

/// The feature a call made now is counted under.
fn current_feature() -> Feature {
    FEATURE
        .try_with(|f| *f)
        .ok()
        .or_else(|| crate::privacy::net::current_feature().map(Feature::of_net))
        .unwrap_or(Feature::Other)
}

/// One call's counts, as a Chat turn keeps its last ([`last_call`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LastCall {
    /// The prompt's tokens (`None`: the answer didn't say).
    pub prompt: Option<u64>,
    pub answer: Option<u64>,
    /// How fast it wrote (tokens a second; llama.cpp's timings).
    pub per_second: Option<f64>,
}

/// Run `fut` keeping the last call it makes ([`last_call`] inside it): a
/// Chat turn, whose last model call is the conversation's context.
pub async fn tallied<F: Future>(fut: F) -> F::Output {
    TALLY.scope(Arc::new(Mutex::new(None)), fut).await
}

/// The last call made inside [`tallied`] so far (`None`: none, or not
/// inside one).
pub fn last_call() -> Option<LastCall> {
    TALLY.try_with(|t| *t.lock().unwrap_or_else(|p| p.into_inner())).ok().flatten()
}

/// How fast a llama.cpp server read the prompt and wrote the answer
/// (`timings`): tokens and milliseconds.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Speed {
    pub read_tokens: u64,
    pub read_ms: u64,
    pub gen_tokens: u64,
    pub gen_ms: u64,
}

impl Speed {
    /// Tokens written a second.
    pub fn per_second(&self) -> Option<f64> {
        (self.gen_ms > 0 && self.gen_tokens > 0).then(|| self.gen_tokens as f64 * 1000.0 / self.gen_ms as f64)
    }
}

/// The counts an answer carries.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Reported {
    pub prompt: Option<u64>,
    pub answer: Option<u64>,
    /// The provider's own total (some say only that).
    pub total: Option<u64>,
    pub speed: Speed,
}

impl Reported {
    /// All the call's tokens: the provider's total, else prompt + answer.
    pub fn total_tokens(&self) -> u64 {
        self.total.unwrap_or_else(|| self.prompt.unwrap_or(0) + self.answer.unwrap_or(0))
    }

    /// The answer said how many tokens it took.
    pub fn counted(&self) -> bool {
        self.prompt.is_some() || self.answer.is_some() || self.total.is_some()
    }
}

/// The counts in a response, whole or reassembled from a stream
/// (`stream::Assembler::response`), in every shape Yap's providers send:
/// OpenAI-compatible `usage {prompt_tokens, completion_tokens,
/// total_tokens}` (OpenAI, Groq, Anthropic's and Gemini's compatible
/// endpoints, OpenRouter, llama.cpp, Ollama, LM Studio), Groq's
/// `x_groq.usage` (its stream), `usage {input_tokens, output_tokens}`
/// (Anthropic's own API — its cache's tokens are prompt tokens too — and
/// OpenAI's Responses API), Gemini's `usageMetadata` (its thinking counts
/// as answer), and llama.cpp's `timings` — the speed, and the counts when
/// `usage` is missing (`cache_n` + `prompt_n` read, `predicted_n`
/// written). Pure.
pub fn parse(v: &Value) -> Reported {
    let n = |x: &Value| x.as_u64().or_else(|| x.as_f64().filter(|f| *f >= 0.0).map(|f| f.round() as u64));
    let usage = if v["usage"].is_object() { &v["usage"] } else { &v["x_groq"]["usage"] };
    let mut r = Reported::default();
    if usage.is_object() {
        if let Some(p) = n(&usage["prompt_tokens"]) {
            r.prompt = Some(p);
            r.answer = n(&usage["completion_tokens"]);
        } else if let Some(i) = n(&usage["input_tokens"]) {
            let cache = n(&usage["cache_read_input_tokens"]).unwrap_or(0) + n(&usage["cache_creation_input_tokens"]).unwrap_or(0);
            r.prompt = Some(i + cache);
            r.answer = n(&usage["output_tokens"]);
        }
        r.total = n(&usage["total_tokens"]);
    } else if v["usageMetadata"].is_object() {
        let m = &v["usageMetadata"];
        r.prompt = n(&m["promptTokenCount"]);
        let written = n(&m["candidatesTokenCount"]).unwrap_or(0) + n(&m["thoughtsTokenCount"]).unwrap_or(0);
        r.answer = (m["candidatesTokenCount"].is_number() || m["thoughtsTokenCount"].is_number()).then_some(written);
        r.total = n(&m["totalTokenCount"]);
    }
    let t = &v["timings"];
    if t.is_object() {
        let ms = |count: Option<u64>, ms: Option<f64>, per_s: Option<f64>| -> u64 {
            match (count, ms, per_s) {
                (_, Some(ms), _) if ms > 0.0 => ms.round() as u64,
                (Some(c), _, Some(ps)) if ps > 0.0 => (c as f64 * 1000.0 / ps).round() as u64,
                _ => 0,
            }
        };
        let (read, written, cached) = (n(&t["prompt_n"]), n(&t["predicted_n"]), n(&t["cache_n"]));
        r.speed = Speed {
            read_tokens: read.unwrap_or(0),
            read_ms: ms(read, t["prompt_ms"].as_f64(), t["prompt_per_second"].as_f64()),
            gen_tokens: written.unwrap_or(0),
            gen_ms: ms(written, t["predicted_ms"].as_f64(), t["predicted_per_second"].as_f64()),
        };
        if r.prompt.is_none() && read.is_some() {
            r.prompt = Some(read.unwrap_or(0) + cached.unwrap_or(0));
        }
        if r.answer.is_none() && written.is_some() {
            r.answer = written;
        }
    }
    r
}

/// Who a recorded call went to.
pub struct Call<'a> {
    /// The provider id the call was made with ("groq", "ondevice"…).
    pub provider: &'a str,
    pub model: &'a str,
    /// The address it went to (on this PC or not).
    pub base_url: &'a str,
    /// It's the turn's own model call ([`last_call`]): not a provider's web
    /// search sub-call, not an embedding.
    pub turn: bool,
    /// Counted as this, whatever the scope says (embeddings: Search).
    pub feature: Option<Feature>,
}

/// One AI call answered: its counts into Groq's meter, the daily store and
/// the turn's tally. Best-effort and quick (a lock and an addition; the
/// store is written by its own thread): never fails or slows the call.
/// `remaining_req` / `limit_req`: Groq's daily request headers.
pub fn record_response(call: &Call, response: &Value, remaining_req: Option<u64>, limit_req: Option<u64>) {
    let reported = parse(response);
    groq::record(call.provider, reported.total_tokens(), remaining_req, limit_req);
    count(call, &reported);
}

/// [`record_response`] for search by meaning's embeddings: the tracker
/// only (Groq's meter, and the on-device model's "Cleanups today" beside
/// it, count chat calls).
pub fn record_embeddings(call: &Call, response: &Value) {
    count(call, &parse(response));
}

fn count(call: &Call, reported: &Reported) {
    if call.turn {
        let last = LastCall { prompt: reported.prompt, answer: reported.answer, per_second: reported.speed.per_second() };
        let _ = TALLY.try_with(|t| *t.lock().unwrap_or_else(|p| p.into_inner()) = Some(last));
    }
    // Named as the chip names it ("ondevice" on a cloud endpoint is that
    // provider); on this PC as the network receipts place it (a test run's
    // stand-in for a cloud provider counts as that provider's cloud).
    let target = crate::ai::ai_model::describe(call.provider, call.model, call.base_url, "");
    let here = target.provider == crate::ai::local_llm::PROVIDER_ONDEVICE
        || crate::privacy::net::place_of_url(call.base_url) == Some(crate::privacy::net::Place::Pc);
    let feature = call.feature.unwrap_or_else(current_feature);
    tracing::debug!(
        provider = %target.provider,
        feature = feature.id(),
        prompt = reported.prompt.unwrap_or(0),
        answer = reported.answer.unwrap_or(0),
        "AI usage: a call counted"
    );
    // Unit tests that talk to a local fake model never touch the store.
    if cfg!(test) {
        return;
    }
    store::record(&target.provider, &target.model, here, feature, reported);
}

/// Today's, the last 7 days' and the last 30 days' AI use, by model and by
/// feature, with the on-device model's speed by day and the estimated
/// cost of cloud models whose price is known: Chat's context ring panel
/// and Insights' AI usage card.
#[tauri::command]
pub fn ai_usage_summary() -> store::Summary {
    store::summary()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_providers_counts_are_read() {
        // OpenAI-compatible (OpenAI, Groq whole, Anthropic's and Gemini's
        // compatible endpoints, OpenRouter, llama.cpp with usage).
        let r = parse(&json!({ "usage": { "prompt_tokens": 1200, "completion_tokens": 300, "total_tokens": 1500 } }));
        assert_eq!((r.prompt, r.answer, r.total), (Some(1200), Some(300), Some(1500)));
        assert_eq!(r.total_tokens(), 1500);
        // Groq's stream: x_groq.usage.
        let r = parse(&json!({ "x_groq": { "usage": { "prompt_tokens": 40, "completion_tokens": 2, "total_tokens": 42 } } }));
        assert_eq!((r.prompt, r.answer), (Some(40), Some(2)));
        // Anthropic's own API: its cache's tokens are prompt tokens too.
        let r = parse(&json!({ "usage": { "input_tokens": 100, "cache_read_input_tokens": 900, "cache_creation_input_tokens": 50, "output_tokens": 70 } }));
        assert_eq!((r.prompt, r.answer, r.total_tokens()), (Some(1050), Some(70), 1120));
        // Gemini's own API: thinking counts as the answer.
        let r = parse(&json!({ "usageMetadata": { "promptTokenCount": 30, "candidatesTokenCount": 25, "thoughtsTokenCount": 5, "totalTokenCount": 60 } }));
        assert_eq!((r.prompt, r.answer, r.total), (Some(30), Some(30), Some(60)));
        // Only a total.
        let r = parse(&json!({ "usage": { "total_tokens": 7 } }));
        assert_eq!((r.prompt, r.answer, r.total_tokens()), (None, None, 7));
        assert!(r.counted());
        // Nothing said.
        let r = parse(&json!({ "choices": [] }));
        assert!(!r.counted());
        assert_eq!(r.total_tokens(), 0);
    }

    #[test]
    fn llama_cpp_timings_give_the_speed_and_the_counts_without_usage() {
        // usage + timings: the counts from usage, the speed from timings.
        let r = parse(&json!({
            "usage": { "prompt_tokens": 2000, "completion_tokens": 240, "total_tokens": 2240 },
            "timings": { "cache_n": 1800, "prompt_n": 200, "prompt_ms": 100.0, "prompt_per_second": 2000.0,
                         "predicted_n": 240, "predicted_ms": 2000.0, "predicted_per_second": 120.0 }
        }));
        assert_eq!((r.prompt, r.answer), (Some(2000), Some(240)));
        assert_eq!(r.speed, Speed { read_tokens: 200, read_ms: 100, gen_tokens: 240, gen_ms: 2000 });
        assert_eq!(r.speed.per_second(), Some(120.0));
        // No usage (an older llama.cpp): what was read (cache + new) and written.
        let r = parse(&json!({ "timings": { "cache_n": 50, "prompt_n": 150, "predicted_n": 60, "predicted_per_second": 30.0 } }));
        assert_eq!((r.prompt, r.answer), (Some(200), Some(60)));
        assert_eq!(r.speed.gen_ms, 2000, "from tokens ÷ per second when the ms aren't there");
        assert_eq!(r.speed.per_second(), Some(30.0));
    }

    #[test]
    fn a_streamed_answer_counts_like_a_whole_one() {
        // The stream's usage and timings come back on the reassembled
        // response (`stream::Assembler::response`).
        let mut a = crate::chat::stream::Assembler::default();
        a.push(&json!({ "choices": [{ "index": 0, "delta": { "role": "assistant", "content": "Hi" } }] }).to_string()).unwrap();
        a.push(&json!({ "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }], "timings": { "predicted_n": 12, "predicted_ms": 100.0 } }).to_string()).unwrap();
        a.push(&json!({ "choices": [], "usage": { "prompt_tokens": 90, "completion_tokens": 12, "total_tokens": 102 } }).to_string()).unwrap();
        let whole = a.response(&a.message().unwrap());
        let r = parse(&whole);
        assert_eq!((r.prompt, r.answer), (Some(90), Some(12)));
        assert_eq!(r.speed.per_second(), Some(120.0));
        // Groq's stream says it in x_groq.
        let mut g = crate::chat::stream::Assembler::default();
        g.push(&json!({ "choices": [{ "index": 0, "delta": { "content": "Hi" } }] }).to_string()).unwrap();
        g.push(&json!({ "choices": [{ "index": 0, "delta": {}, "finish_reason": "stop" }], "x_groq": { "usage": { "prompt_tokens": 8, "completion_tokens": 1, "total_tokens": 9 } } }).to_string()).unwrap();
        let r = parse(&g.response(&g.message().unwrap()));
        assert_eq!((r.prompt, r.answer, r.total), (Some(8), Some(1), Some(9)));
    }

    #[tokio::test]
    async fn a_turn_keeps_its_last_call_and_the_scopes_name_the_feature() {
        let call = Call { provider: "groq", model: "openai/gpt-oss-20b", base_url: "https://api.groq.com/openai/v1", turn: true, feature: None };
        let last = tallied(async {
            record_response(&call, &json!({ "usage": { "prompt_tokens": 10, "completion_tokens": 2 } }), None, None);
            // A provider's own search sub-call isn't the turn's context.
            let search = Call { turn: false, ..call };
            record_response(&search, &json!({ "usage": { "prompt_tokens": 3400, "completion_tokens": 190 } }), None, None);
            record_response(&call, &json!({ "usage": { "prompt_tokens": 500, "completion_tokens": 40 } }), None, None);
            last_call()
        })
        .await;
        assert_eq!(last, Some(LastCall { prompt: Some(500), answer: Some(40), per_second: None }));
        // Outside a turn: nothing kept.
        assert_eq!(last_call(), None);

        // The receipts' scope names it, a finer one here wins.
        let f = crate::privacy::net::within(crate::privacy::net::Feature::Dictation, async { current_feature() }).await;
        assert_eq!(f, Feature::Cleanup);
        let f = crate::privacy::net::within(crate::privacy::net::Feature::Dictation, within(Feature::Translation, async { current_feature() })).await;
        assert_eq!(f, Feature::Translation);
        let f = crate::privacy::net::within(crate::privacy::net::Feature::Brief, async { current_feature() }).await;
        assert_eq!(f, Feature::Brief);
        assert_eq!(current_feature(), Feature::Other);
    }
}

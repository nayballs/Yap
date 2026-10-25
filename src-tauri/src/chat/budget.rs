//! **A chat sized to its model** (2026-10-25): how much of the model's
//! context a chat may use, counted in tokens, and which earlier messages go
//! to the model — the Chat view, the Voice Assistant and a meeting's Ask
//! threads. Before, Chat sent the last 19 messages whatever the model:
//! too many for an 8K model on this PC, a waste of a 64K or a 1M one.
//!
//! - **The context** of the model a call goes to ([`context_window`], also
//!   the ring's, `usage/context.rs`): the on-device model's real `n_ctx`
//!   (what the sidecar serves, else what it starts with), a self-hosted
//!   server's word (Ollama, LM Studio, llama.cpp's `/props`), a cloud
//!   model's from its provider's list, else the provider's usual
//!   ([`default_context`], approximate).
//! - **The budget** ([`budget`]): the history and the question get about
//!   70% of the context (the Voice Assistant 35%, at most 6K tokens: every
//!   tool step sends them again), and never more than what's left after
//!   the parts measured for the turn — the system prompt, the tools'
//!   schemas, the question's context block — and room for tool results and
//!   the answer. A cloud plan that counts tokens a minute (Groq's free plan,
//!   8,000; the provider's `x-ratelimit-limit-tokens` once it said) caps the
//!   whole request ([`rate_cap`]).
//! - **Counting** ([`Counter`]): about four characters a token with a
//!   margin ([`estimate`]: ASCII at 3.6, any other character one), or, for
//!   a long text sent to a llama.cpp server on this PC, the server's own
//!   tokenizer (`/tokenize`, cached). The ring counts the same way.
//! - **The window** ([`plan`]): every message when they fit; else the
//!   conversation's summary (`compact.rs`) and the messages after it; else
//!   the latest messages that fit. The start moves a few messages at a time
//!   (`step`), so a long chat's prompt keeps its start from turn to turn (a
//!   hybrid local model re-reads only from the first change).

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use serde_json::Value;

use crate::chat::chats::ChatMessage;

/// Who asks: each has its own share of the model's context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The Chat view (and a question dictated into it).
    Chat,
    /// The Voice Assistant: a smaller history (every tool step sends it
    /// again, and a spoken follow-up rarely needs the whole chat).
    Assistant,
    /// A note's Ask thread (the Notes view's bar, the notepad's sheet): the
    /// note itself is the big part of its prompt.
    Note,
}

impl Kind {
    /// The kind a `chat_turn` scope means.
    pub fn of_scope(scope: Option<&str>) -> Kind {
        if scope == Some("assistant") {
            Kind::Assistant
        } else {
            Kind::Chat
        }
    }

    /// The share of the context the history and the question may take.
    fn share(self) -> u64 {
        match self {
            Kind::Chat | Kind::Note => 70,
            Kind::Assistant => 35,
        }
    }

    /// At most this many tokens of history, whatever the model.
    fn most(self) -> u64 {
        match self {
            Kind::Assistant => ASSISTANT_MOST,
            Kind::Chat | Kind::Note => u64::MAX,
        }
    }

    /// The window's start moves this many messages at a time.
    pub fn step(self) -> usize {
        match self {
            Kind::Assistant => 4,
            Kind::Chat | Kind::Note => 6,
        }
    }
}

/// The Voice Assistant's history: at most this many tokens.
pub const ASSISTANT_MOST: u64 = 6_000;
/// Each message's own tokens besides its text (its role, the chat
/// template's marks).
pub const MESSAGE_OVERHEAD: u64 = 6;
/// A screenshot sent again with the history (`snip::turn::with_history_images`
/// sends the newest three; the rest are a line).
pub const IMAGE_TOKENS: u64 = 1_000;
/// How many earlier screenshots go again whole.
const IMAGES_AGAIN: usize = 3;
/// Groq's free plan: tokens a minute, until Groq's own header says.
const GROQ_FREE_TPM: u64 = 8_000;

/// How a turn's history is chosen: always [`Mode::Summary`] in the app; a
/// model evaluation compares (`YAP_EVAL_HISTORY` = messages | window |
/// summary).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The last 19 messages, as until 25 Oct 2026 (`chats::CHAT_HISTORY`).
    Messages,
    /// The token window alone, no summary.
    Window,
    /// The token window and the summary of what's older.
    Summary,
}

/// This run's [`Mode`].
pub fn mode() -> Mode {
    if crate::ai::eval::active() {
        match std::env::var("YAP_EVAL_HISTORY").as_deref() {
            Ok("messages") => return Mode::Messages,
            Ok("window") => return Mode::Window,
            _ => {}
        }
    }
    Mode::Summary
}

// ---- counting -------------------------------------------------------------------------------

/// About four characters a token, with a margin: ASCII at 3.6 characters a
/// token (English prose is ~4 on the models Yap uses; code and JSON less),
/// every other character one (accents, other alphabets, CJK). Pure.
pub fn estimate(text: &str) -> u64 {
    let (mut ascii, mut other) = (0u64, 0u64);
    for c in text.chars() {
        if c.is_ascii() {
            ascii += 1;
        } else {
            other += 1;
        }
    }
    (ascii * 10).div_ceil(36) + other
}

/// Texts this long or longer are counted by a llama.cpp server's own
/// tokenizer when the call goes to one on this PC; shorter ones (most
/// messages) are estimated: the margin covers them.
const TOKENIZE_FROM: usize = 1_500;
const TOKENIZE_TIMEOUT: Duration = Duration::from_secs(3);

/// The servers that answered no `/tokenize` (an Ollama, an LM Studio): not
/// asked again this session.
static NO_TOKENIZE: Mutex<Option<HashSet<String>>> = Mutex::new(None);
/// Counts a server gave, by its address and the text's hash.
static COUNTED: Mutex<Option<HashMap<(String, u64), u64>>> = Mutex::new(None);
/// The cache's size before it starts again.
const COUNTED_MAX: usize = 4_096;

/// What counts a call's tokens: [`estimate`], or the server's tokenizer
/// for a long text sent to a llama.cpp server on this PC.
#[derive(Debug, Clone, Default)]
pub struct Counter {
    /// The server's root (`http://127.0.0.1:8080`), when it may be asked.
    root: Option<String>,
    provider: String,
}

impl Counter {
    /// Estimates only.
    #[cfg(test)]
    pub fn estimate_only() -> Counter {
        Counter::default()
    }

    /// The counter for a call to `provider` at `base_url`: the on-device
    /// model, or a self-hosted server on this PC, counts long texts itself
    /// (once it's loaded); anything else is estimated.
    pub fn for_call(provider: &str, base_url: &str) -> Counter {
        let here = matches!(provider, crate::ai::local_llm::PROVIDER_ONDEVICE | "local" | "custom")
            && crate::ai::ai_model::runs_here(provider, base_url)
            && !crate::ai::local_llm::on_demand::is_standby(base_url);
        let root = here.then(|| server_root(base_url).to_string()).filter(|r| !r.is_empty() && !refused(r));
        Counter { root, provider: provider.to_string() }
    }

    /// `text`'s tokens.
    pub async fn count(&self, text: &str) -> u64 {
        let Some(root) = self.root.as_deref().filter(|_| text.len() >= TOKENIZE_FROM) else {
            return estimate(text);
        };
        let key = (root.to_string(), hash(text));
        if let Some(n) = COUNTED.lock().ok().and_then(|g| g.as_ref().and_then(|m| m.get(&key).copied())) {
            return n;
        }
        match tokenize(root, &self.provider, text).await {
            Some(n) => {
                if let Ok(mut g) = COUNTED.lock() {
                    let m = g.get_or_insert_with(HashMap::new);
                    if m.len() >= COUNTED_MAX {
                        m.clear();
                    }
                    m.insert(key, n);
                }
                n
            }
            None => {
                if let Ok(mut g) = NO_TOKENIZE.lock() {
                    g.get_or_insert_with(HashSet::new).insert(root.to_string());
                }
                estimate(text)
            }
        }
    }

    /// The tokens of messages as a request carries them (OpenAI messages:
    /// their text, a tool call's arguments, and each message's overhead).
    pub async fn count_messages(&self, messages: &[Value]) -> u64 {
        let mut n = 0;
        for m in messages {
            n += self.count(&message_text(m)).await + MESSAGE_OVERHEAD;
        }
        n
    }
}

fn refused(root: &str) -> bool {
    NO_TOKENIZE.lock().ok().is_some_and(|g| g.as_ref().is_some_and(|s| s.contains(root)))
}

fn hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

/// A server's own address under its OpenAI-compatible `/v1`. Pure.
fn server_root(base_url: &str) -> &str {
    let b = base_url.trim().trim_end_matches('/');
    b.strip_suffix("/v1").unwrap_or(b)
}

/// llama.cpp's `POST /tokenize {content}` → how many tokens (`None`: the
/// server can't say).
async fn tokenize(root: &str, provider: &str, text: &str) -> Option<u64> {
    use crate::privacy::net::Recorded;
    let client = reqwest::Client::builder().timeout(TOKENIZE_TIMEOUT).build().ok()?;
    let resp = client
        .post(format!("{root}/tokenize"))
        .json(&serde_json::json!({ "content": text }))
        .send_for(crate::privacy::net::Purpose::Ai(provider.to_string()))
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: Value = resp.json().await.ok()?;
    v["tokens"].as_array().map(|t| t.len() as u64)
}

/// The text of one OpenAI message, as counted: its content (a question
/// with screenshots: its text parts), and its tool calls' names and
/// arguments. Pure.
pub fn message_text(m: &Value) -> String {
    let mut text = crate::snip::turn::text_of(&m["content"]).unwrap_or_default();
    for call in m["tool_calls"].as_array().into_iter().flatten() {
        text.push(' ');
        text.push_str(call["function"]["name"].as_str().unwrap_or(""));
        text.push(' ');
        match &call["function"]["arguments"] {
            Value::String(s) => text.push_str(s),
            other => text.push_str(&other.to_string()),
        }
    }
    text
}

/// What each message of a conversation costs as a turn sends it: its
/// text, an earlier web answer's replayed search (`replay`,
/// `chats::history_messages`), and the screenshots sent again (the newest
/// three whole, the rest a line).
pub async fn message_costs(counter: &Counter, messages: &[ChatMessage], replay: bool) -> Vec<u64> {
    let mut costs = Vec::with_capacity(messages.len());
    for i in 0..messages.len() {
        let sent = if replay {
            crate::chat::chats::history_message(messages, i)
        } else {
            vec![serde_json::json!({ "role": messages[i].role, "content": messages[i].text })]
        };
        costs.push(counter.count_messages(&sent).await);
    }
    // Screenshots: the newest few sent again whole.
    let mut whole = IMAGES_AGAIN;
    for (i, m) in messages.iter().enumerate().rev() {
        for _ in &m.images {
            if whole > 0 {
                costs[i] += IMAGE_TOKENS;
                whole -= 1;
            } else {
                costs[i] += 20;
            }
        }
    }
    costs
}

// ---- the context ------------------------------------------------------------------------------

/// A provider's context when nothing says (its list didn't have the model,
/// a server didn't say): the pages call it approximate. Pure.
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

/// The context of the model a call to `provider`'s `model` at `base_url`
/// goes to, and whether the model or its server said so (else
/// [`default_context`]): the on-device model's own (what the sidecar
/// serves, its log's `n_ctx`, else what it starts with), a self-hosted
/// server's word, a cloud model's from its provider's list.
pub async fn context_window(cfg: &crate::config::YapConfig, provider: &str, model: &str, base_url: &str) -> (u64, bool) {
    #[cfg(debug_assertions)]
    if let Some(n) = eval_context() {
        return (n, true);
    }
    if provider == crate::ai::local_llm::PROVIDER_ONDEVICE {
        return (u64::from(crate::ai::local_llm::context::context_now(cfg)), true);
    }
    if matches!(provider, "local" | "custom") {
        if let Some(n) = crate::ai::model_catalog::server_context_here(cfg, provider, base_url, model).await {
            return (n, true);
        }
        // A llama.cpp server on this PC that lists its model by another
        // name: the one context it serves.
        if crate::ai::ai_model::runs_here(provider, base_url) {
            if let Some(n) = served_context(base_url).await {
                return (n, true);
            }
        }
        return (default_context(provider), false);
    }
    match crate::ai::model_catalog::listed_model(provider, model).and_then(|m| m.context) {
        Some(n) if n > 0 => (n, true),
        _ => (default_context(provider), false),
    }
}

/// What a llama.cpp server on this PC says it serves (`GET /props`), asked
/// at most once a minute per server (`None`: it doesn't say).
async fn served_context(base_url: &str) -> Option<u64> {
    use crate::privacy::net::Recorded;
    /// When each server was asked, and what it said.
    type Seen = HashMap<String, (std::time::Instant, Option<u64>)>;
    static SEEN: Mutex<Option<Seen>> = Mutex::new(None);
    let root = server_root(base_url).to_string();
    if let Some((at, n)) = SEEN.lock().ok().and_then(|g| g.as_ref().and_then(|m| m.get(&root).copied())) {
        if at.elapsed() < Duration::from_secs(60) {
            return n;
        }
    }
    let asked = async {
        let client = reqwest::Client::builder().timeout(TOKENIZE_TIMEOUT).build().ok()?;
        let resp = client.get(format!("{root}/props")).send_for(crate::privacy::net::Purpose::ModelList("your server".into())).await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let v: Value = resp.json().await.ok()?;
        crate::ai::model_catalog::parse_llamacpp_props(&v)
    };
    let n = asked.await;
    if let Ok(mut g) = SEEN.lock() {
        g.get_or_insert_with(HashMap::new).insert(root, (std::time::Instant::now(), n));
    }
    n
}

/// A model evaluation's stand-in context (`YAP_EVAL_CONTEXT`, tokens): a
/// cloud model made to fill up as fast as a small one, to compare the
/// window with the summary on a few thousand tokens.
#[cfg(debug_assertions)]
fn eval_context() -> Option<u64> {
    if !crate::ai::eval::active() {
        return None;
    }
    std::env::var("YAP_EVAL_CONTEXT").ok().and_then(|v| v.trim().parse::<u64>().ok()).filter(|n| *n >= 1_024)
}

/// "8K", "64K", "128K", "200K", "1M" (`localContext.js fmtTokens`). Pure.
pub fn context_label(n: u64) -> String {
    if n >= 1_048_576 && n.is_multiple_of(1_048_576) {
        return format!("{}M", n / 1_048_576);
    }
    if n >= 1_000_000 {
        let m = (n as f64 / 100_000.0).round() / 10.0;
        return format!("{m}M").replace(".0M", "M");
    }
    if n.is_multiple_of(1_000) && n >= 1_000 {
        return format!("{}K", n / 1_000);
    }
    if n.is_multiple_of(1_024) && n >= 1_024 {
        return format!("{}K", n / 1_024);
    }
    if n >= 1_000 {
        return format!("{}K", (n as f64 / 1_000.0).round() as u64);
    }
    n.to_string()
}

/// A model as a sentence names it: the on-device model's own name
/// ("Qwen3.5 4B"), else a readable one from its id ("GPT-OSS 20B").
pub fn model_label(provider: &str, model: &str, on_device_name: &str) -> String {
    if provider == crate::ai::local_llm::PROVIDER_ONDEVICE && !on_device_name.trim().is_empty() {
        return on_device_name.trim().to_string();
    }
    let named = crate::ai::model_catalog::derive_name(model);
    if named.trim().is_empty() {
        "the model".to_string()
    } else {
        named
    }
}

/// **The plain sentence for "too long"**: "That's too long for Qwen3.5 4B
/// (64K context)." Pure.
pub fn too_long(label: &str, context: u64) -> String {
    format!("That's too long for {label} ({} context).", context_label(context))
}

/// The start [`too_long`]'s sentences share (the pages know them by it).
pub const TOO_LONG: &str = "That's too long for ";

/// An AI call's error as the person reads it: a provider's "too long for
/// the context" (`llm::says_too_long`) as [`too_long`]'s sentence; more
/// than the plan sends in a minute as "That's too long for GPT-OSS 20B on
/// your Groq plan (8,000 tokens a minute)."; anything else as it was.
/// Pure.
pub fn plain_error(err: String, label: &str, context: u64, provider: &str) -> String {
    if err.contains(crate::ai::llm::CONTEXT_EXCEEDED) {
        return too_long(label, context);
    }
    if let Some(at) = err.find(crate::ai::llm::TOO_MUCH_A_MINUTE) {
        let limit: u64 = err[at + crate::ai::llm::TOO_MUCH_A_MINUTE.len()..].trim().parse().unwrap_or(0);
        let who = crate::privacy::net::provider_name(provider);
        return format!("{TOO_LONG}{label} on your {who} plan ({} tokens a minute).", thousands(limit));
    }
    err
}

/// "8,000". Pure.
fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

// ---- a plan's tokens a minute ------------------------------------------------------------------

static RATES: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);

/// A cloud provider said how many tokens a minute this key may send
/// (`x-ratelimit-limit-tokens`, or Groq's "Limit 8000" when it refused a
/// request): a request is kept under it.
pub fn heard_rate_limit(provider: &str, tokens_per_minute: u64) {
    if tokens_per_minute == 0 || crate::ai::local_llm::PROVIDER_ONDEVICE == provider {
        return;
    }
    if let Ok(mut g) = RATES.lock() {
        g.get_or_insert_with(HashMap::new).insert(provider.to_string(), tokens_per_minute);
    }
}

/// The most one request to `provider` should carry for the key's plan:
/// 90% of its tokens a minute (Groq's free plan, 8,000, until Groq says).
/// `None`: no such limit known.
pub fn rate_cap(provider: &str) -> Option<u64> {
    let known = RATES.lock().ok().and_then(|g| g.as_ref().and_then(|m| m.get(provider).copied()));
    known.or((provider == "groq").then_some(GROQ_FREE_TPM)).map(|t| t * 9 / 10)
}

// ---- the budget --------------------------------------------------------------------------------

/// What a turn may send.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Budget {
    /// The model's context, in tokens.
    pub context: u64,
    /// The model or its server said so (else a provider's usual).
    pub known: bool,
    /// The whole first request: the context less the answer's room, within
    /// a plan's tokens a minute.
    pub request: u64,
    /// The parts measured for the turn: the system prompt, the tools'
    /// schemas, the question's context block.
    pub fixed: u64,
    /// The history and the question.
    pub history: u64,
}

/// The budget for a turn on a model of `context` tokens whose fixed parts
/// take `fixed` (`tools`: it gets tool results too; `cap`: [`rate_cap`]).
/// Pure.
pub fn budget(context: u64, known: bool, fixed: u64, kind: Kind, tools: bool, cap: Option<u64>) -> Budget {
    // The answer's room: an eighth of the context, 512 to 8K tokens.
    let reply = (context / 8).clamp(512, 8_192).min(context / 2);
    let request = context.saturating_sub(reply).min(cap.unwrap_or(u64::MAX));
    // Tool results' room (a note, a page read): an eighth of the request.
    let tool_room = if tools { (request / 8).clamp(512, 16_384).min(request / 4) } else { 0 };
    let share = context.saturating_mul(kind.share()) / 100;
    let history = share.min(request.saturating_sub(fixed + tool_room)).min(kind.most());
    Budget { context, known, request, fixed, history }
}

// ---- the window ---------------------------------------------------------------------------------

/// Which earlier messages a turn sends.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Plan {
    /// The first message sent whole.
    pub start: usize,
    /// The conversation's summary goes before them (`compact.rs`).
    pub summary: bool,
    /// Older messages left out with nothing in their place (no summary yet,
    /// or too little room even with it).
    pub left_out: usize,
    /// The history's tokens as sent, the summary's included.
    pub tokens: u64,
}

/// The window: every message when they fit in `room` tokens (with a
/// summary written, only while they take half the room or less: a much
/// bigger model since, say — else the summary stays, and the prompt keeps
/// its start); else the summary (`(upto, its tokens)`: it covers `..upto`)
/// and the messages after it; else the latest that fit — with the summary
/// when it fits beside them. A start that has to move goes `step` messages
/// at a time, so it stays put for a few turns. Pure.
pub fn plan(costs: &[u64], summary: Option<(usize, u64)>, room: u64, step: usize) -> Plan {
    let len = costs.len();
    let total: u64 = costs.iter().sum();
    let summarised = summary.is_some_and(|(upto, _)| upto > 0 && upto <= len);
    if total <= room && (!summarised || total <= room / 2) {
        return Plan { start: 0, summary: false, left_out: 0, tokens: total };
    }
    let step = step.max(1);
    let from_end = |from: usize| -> u64 { costs[from.min(len)..].iter().sum() };
    // The fewest messages dropped for `room`, from `base`, `step` at a time.
    let first_fitting = |base: usize, room: u64| -> Option<usize> {
        let mut s = base;
        while s <= len {
            if from_end(s) <= room {
                return Some(s);
            }
            s += step;
        }
        None
    };
    if let Some((upto, cost)) = summary.filter(|(upto, _)| *upto > 0 && *upto <= len) {
        if cost < room {
            if let Some(s) = first_fitting(upto, room - cost) {
                return Plan { start: s, summary: true, left_out: s - upto, tokens: cost + from_end(s) };
            }
        }
    }
    if total <= room {
        return Plan { start: 0, summary: false, left_out: 0, tokens: total };
    }
    let s = first_fitting(0, room).unwrap_or(len).min(len);
    Plan { start: s, summary: false, left_out: s, tokens: from_end(s) }
}

/// Where the older messages a summary should cover end, after an answer
/// (`compact.rs`): `Some(from..to)` when the history fills 75% of its
/// `room` (or the model's context is 75% used — `ring` — and the history
/// fills half the room: a big prompt alone isn't the history's), keeping the latest
/// ~40% of the room whole (at least the last turn), starting on a question
/// (`users`), and only when there's enough to be worth a summary. `summary`
/// = the one there is (`(upto, its tokens)`). Pure.
pub fn due(costs: &[u64], users: &[bool], summary: Option<(usize, u64)>, room: u64, ring: Option<f64>) -> Option<std::ops::Range<usize>> {
    let len = costs.len();
    if len < 4 || room == 0 {
        return None;
    }
    let (from, have) = summary.filter(|(upto, _)| *upto <= len).unwrap_or((0, 0));
    let now: u64 = have + costs[from..].iter().sum::<u64>();
    let fill = now as f64 / room as f64;
    let full = fill >= 0.75 || (fill >= 0.5 && ring.is_some_and(|r| r >= 0.75));
    if !full {
        return None;
    }
    // Keep the latest ~40% of the room whole.
    let keep = room * 2 / 5;
    let mut to = len;
    let mut kept = 0;
    while to > from && kept + costs[to - 1] <= keep {
        to -= 1;
        kept += costs[to];
    }
    // At least the last turn whole; the part kept starts on a question.
    to = to.min(len.saturating_sub(2));
    while to < len.saturating_sub(2) && !users.get(to).copied().unwrap_or(true) {
        to += 1;
    }
    while to > from && !users.get(to).copied().unwrap_or(true) {
        to -= 1;
    }
    let older: u64 = costs[from..to.max(from)].iter().sum();
    let worth = older >= (room / 10).max(300);
    (to >= from + 2 && worth).then_some(from..to)
}

// ---- a turn's tool results ---------------------------------------------------------------------

/// What a tool result says when there's no room left in the request for it.
pub const NO_ROOM: &str = "There's no room left in the model's context for more results this turn. Answer with what you have, and say what you couldn't read.";
/// The note on a result that was shortened.
pub const CUT_NOTE: &str = "Shortened to fit the model's context. Ask for less at a time (a page's next part, a note's next lines) if you need more.";
const CUT_MARK: &str = "… [cut to fit the model's context]";

/// A tool's result within `tokens` (what the request has left, `agent.rs`):
/// as it is when it fits; else its longest texts shortened, each marked,
/// then its longest lists halved, until it does — with [`CUT_NOTE`] for the
/// model; with too little room for that, [`NO_ROOM`]. So one turn's page
/// reads and notes can never overflow the model's context. Pure.
pub fn fit_result(result: &Value, tokens: u64) -> String {
    let whole = result.to_string();
    if estimate(&whole) <= tokens {
        return whole;
    }
    let refusal = || serde_json::json!({ "success": false, "error": NO_ROOM }).to_string();
    if tokens < 150 {
        return refusal();
    }
    let mut v = result.clone();
    if let Some(o) = v.as_object_mut() {
        o.insert("note".into(), Value::String(CUT_NOTE.into()));
    }
    // The longest texts first, a little more than what's over each time.
    for _ in 0..64 {
        let s = v.to_string();
        let over = estimate(&s).saturating_sub(tokens);
        if over == 0 {
            return s;
        }
        let mut best = (0, Vec::new());
        longest(&v, &mut Vec::new(), &mut best, false);
        let Some(Value::String(leaf)) = at(&mut v, &best.1) else { break };
        let n = leaf.chars().count();
        if n <= 120 {
            break;
        }
        let cut = ((over as usize) * 36 / 10 + 40).min(n - 80);
        let keep: String = leaf.chars().take(n - cut).collect();
        *leaf = format!("{}{CUT_MARK}", keep.trim_end());
    }
    // Many short pieces: the longest lists, halved.
    for _ in 0..32 {
        let s = v.to_string();
        if estimate(&s) <= tokens {
            return s;
        }
        let mut best = (0, Vec::new());
        longest(&v, &mut Vec::new(), &mut best, true);
        let Some(Value::Array(list)) = at(&mut v, &best.1) else { break };
        if list.len() <= 1 {
            break;
        }
        list.truncate(list.len() / 2);
    }
    let s = v.to_string();
    if estimate(&s) <= tokens {
        s
    } else {
        refusal()
    }
}

/// A step into a JSON value.
#[derive(Debug, Clone)]
enum Seg {
    Key(String),
    Index(usize),
}

/// The path to `v`'s longest text (`lists`: its longest list, by its
/// serialised length), Yap's own note left alone.
fn longest(v: &Value, path: &mut Vec<Seg>, best: &mut (usize, Vec<Seg>), lists: bool) {
    match v {
        Value::String(s) if !lists => {
            let n = s.chars().count();
            if n > best.0 {
                *best = (n, path.clone());
            }
        }
        Value::Array(a) => {
            if lists && a.len() > 1 {
                let n = v.to_string().len();
                if n > best.0 {
                    *best = (n, path.clone());
                }
            }
            for (i, x) in a.iter().enumerate() {
                path.push(Seg::Index(i));
                longest(x, path, best, lists);
                path.pop();
            }
        }
        Value::Object(o) => {
            for (k, x) in o {
                if path.is_empty() && k == "note" {
                    continue;
                }
                path.push(Seg::Key(k.clone()));
                longest(x, path, best, lists);
                path.pop();
            }
        }
        _ => {}
    }
}

fn at<'a>(v: &'a mut Value, path: &[Seg]) -> Option<&'a mut Value> {
    let mut cur = v;
    for seg in path {
        cur = match seg {
            Seg::Key(k) => cur.get_mut(k.as_str())?,
            Seg::Index(i) => cur.get_mut(*i)?,
        };
    }
    Some(cur)
}

/// The tokens `messages` (OpenAI messages) and the offered `tools` take,
/// estimated: what a tool step sends. Pure.
pub fn request_tokens(messages: &[Value], tools: &Value) -> u64 {
    messages.iter().map(|m| estimate(&message_text(m)) + MESSAGE_OVERHEAD).sum::<u64>() + if tools.is_null() { 0 } else { estimate(&tools.to_string()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tool_result_is_trimmed_to_the_room_left() {
        let small = serde_json::json!({ "success": true, "data": { "text": "Short." } });
        assert_eq!(fit_result(&small, 1_000), small.to_string());
        // A long page: its text shortened and marked, the rest kept, a note
        // for the model.
        let page = serde_json::json!({ "success": true, "data": { "title": "Lisbon guide", "url": "https://travel.example/lisbon", "text": "Tram 28 runs through Alfama. ".repeat(800) } });
        let fitted = fit_result(&page, 1_000);
        assert!(estimate(&fitted) <= 1_000, "{}", estimate(&fitted));
        let v: Value = serde_json::from_str(&fitted).unwrap();
        assert_eq!(v["success"], true);
        assert_eq!(v["data"]["title"], "Lisbon guide");
        assert!(v["data"]["text"].as_str().unwrap().ends_with(CUT_MARK));
        assert_eq!(v["note"], CUT_NOTE);
        // Many short results: the list halved until it fits.
        let many = serde_json::json!({ "success": true, "data": (0..600).map(|i| serde_json::json!({ "id": i, "title": format!("Note {i}") })).collect::<Vec<_>>() });
        let fitted = fit_result(&many, 400);
        assert!(estimate(&fitted) <= 400);
        let v: Value = serde_json::from_str(&fitted).unwrap();
        assert!(!v["data"].as_array().unwrap().is_empty());
        // No room at all: the model is told so.
        let none: Value = serde_json::from_str(&fit_result(&page, 100)).unwrap();
        assert_eq!(none["success"], false);
        assert_eq!(none["error"], NO_ROOM);
    }

    #[test]
    fn about_four_characters_a_token_with_a_margin() {
        assert_eq!(estimate(""), 0);
        assert_eq!(estimate("a"), 1);
        // 3.6 characters a token: 3,600 ASCII characters are 1,000 tokens,
        // 10% over the plain four characters a token.
        assert_eq!(estimate(&"a".repeat(3_600)), 1_000);
        assert_eq!(estimate(&"a".repeat(4_000)), 1_112);
        // Every other character is one: accents, other alphabets, CJK.
        assert_eq!(estimate("café"), 2);
        assert_eq!(estimate("会議の予算"), 5);
    }

    #[test]
    fn the_budget_is_sized_to_the_model() {
        // An 8K model on this PC with Chat's tools (~3K of instructions and
        // schemas): what's left after the answer's and the tools' room.
        let small = budget(8_192, true, 3_000, Kind::Chat, true, None);
        assert_eq!(small.request, 8_192 - 1_024);
        assert_eq!(small.history, 8_192 - 1_024 - 3_000 - 896);
        // The same model without tools: 70% of the context at most.
        let plain = budget(8_192, true, 600, Kind::Chat, false, None);
        assert_eq!(plain.history, 8_192 * 70 / 100);
        // Qwen3.5-4B at 64K: about 45K for the history.
        let big = budget(65_536, true, 3_500, Kind::Chat, true, None);
        assert_eq!(big.history, 65_536 * 70 / 100);
        assert!(big.history > 40_000);
        // A 1M Claude: 70% of it.
        let claude = budget(1_000_000, true, 4_000, Kind::Chat, true, None);
        assert_eq!(claude.history, 700_000);
        // The Voice Assistant: half Chat's share, and 6K at most.
        assert_eq!(budget(8_192, true, 600, Kind::Assistant, false, None).history, 8_192 * 35 / 100);
        assert_eq!(budget(131_072, true, 3_000, Kind::Assistant, true, None).history, ASSISTANT_MOST);
        // Groq's free plan (8,000 tokens a minute): the whole request under
        // 7,200, whatever the 128K context.
        let groq = budget(131_072, true, 3_000, Kind::Chat, true, Some(7_200));
        assert_eq!(groq.request, 7_200);
        assert_eq!(groq.history, 7_200 - 3_000 - 900);
        // A prompt bigger than the room: nothing for the history (the
        // question alone is refused before it's sent).
        assert_eq!(budget(4_096, true, 5_000, Kind::Chat, true, None).history, 0);
    }

    #[test]
    fn a_plans_tokens_a_minute_cap_the_request() {
        assert_eq!(rate_cap("groq"), Some(7_200), "the free plan until Groq says");
        assert_eq!(rate_cap("anthropic"), None);
        heard_rate_limit("openai", 200_000);
        assert_eq!(rate_cap("openai"), Some(180_000));
        heard_rate_limit("ondevice", 10);
        assert_eq!(rate_cap("ondevice"), None);
    }

    #[test]
    fn every_message_goes_when_they_fit() {
        let costs = [100, 200, 300];
        assert_eq!(plan(&costs, None, 600, 6), Plan { start: 0, summary: false, left_out: 0, tokens: 600 });
        // A summary isn't used while everything fits in half the room (a
        // much bigger model now)…
        assert!(!plan(&costs, Some((2, 50)), 1_200, 6).summary);
        // …but once written it stays while the chat is that long: the
        // prompt keeps its start, and the ring its drop.
        assert_eq!(plan(&costs, Some((2, 50)), 600, 6), Plan { start: 2, summary: true, left_out: 0, tokens: 350 });
    }

    #[test]
    fn the_window_keeps_the_latest_that_fit_moving_in_steps() {
        // 20 messages of 100 tokens, room for 1,000: the last 10 would fit,
        // but the start moves 6 at a time: from 12.
        let costs = vec![100; 20];
        let p = plan(&costs, None, 1_000, 6);
        assert_eq!((p.start, p.summary, p.left_out, p.tokens), (12, false, 12, 800));
        // Two more messages: still from 12 (the prompt keeps its start).
        let costs = vec![100; 22];
        assert_eq!(plan(&costs, None, 1_000, 6).start, 12);
        // Two more: 24 messages, 1,200 from 12 — now from 18.
        let costs = vec![100; 24];
        assert_eq!(plan(&costs, None, 1_000, 6).start, 18);
        // One message bigger than the room: nothing fits.
        assert_eq!(plan(&[5_000], None, 1_000, 6), Plan { start: 1, summary: false, left_out: 1, tokens: 0 });
    }

    #[test]
    fn the_summary_and_what_came_after_it() {
        let costs = vec![100; 20];
        // A summary of the first 12 (150 tokens), and the 8 after it.
        let p = plan(&costs, Some((12, 150)), 1_000, 6);
        assert_eq!((p.start, p.summary, p.left_out, p.tokens), (12, true, 0, 950));
        // More messages than the room after it: the summary, and the
        // latest that fit beside it (a step at a time from the summary).
        let costs = vec![100; 26];
        let p = plan(&costs, Some((12, 150)), 1_000, 6);
        assert_eq!((p.start, p.summary, p.left_out, p.tokens), (18, true, 6, 950));
        // A summary bigger than the room: the plain window.
        let p = plan(&costs, Some((12, 2_000)), 1_000, 6);
        assert!(!p.summary);
        // One that covers more messages than there are (an older Yap's
        // chats.json): not used.
        assert!(!plan(&[100; 10], Some((30, 10)), 500, 6).summary);
    }

    #[test]
    fn a_summary_is_due_once_the_history_fills_three_quarters_of_its_room() {
        let users: Vec<bool> = (0..30).map(|i| i % 2 == 0).collect();
        // 20 messages of 100 tokens in a room of 4,000: half full.
        assert_eq!(due(&[100; 20], &users, None, 4_000, None), None);
        // In a room of 2,500: 80% full — the older part, keeping ~40% of the
        // room (1,000 tokens: the last 10 messages) whole, from a question.
        assert_eq!(due(&[100; 20], &users, None, 2_500, None), Some(0..10));
        // With a summary of the first 10 (200 tokens) and 16 more messages.
        let costs = vec![100; 26];
        assert_eq!(due(&costs, &users, Some((10, 200)), 2_000, None), Some(10..18));
        // The model's context 75% used (a big system prompt): due, but only
        // when there's enough history to be worth it (tiny messages: no).
        assert_eq!(due(&[5; 8], &users, None, 2_000, Some(0.9)), None);
        assert_eq!(due(&[400; 8], &users, None, 5_000, None), None, "64% full: not yet");
        assert_eq!(due(&[400; 8], &users, None, 5_000, Some(0.9)), Some(0..4));
        // A history that already fits in the 40% kept whole: nothing to do.
        assert_eq!(due(&[400; 8], &users, None, 10_000, Some(0.9)), None);
        // The last turn always stays whole.
        let r = due(&[1_000; 4], &users, None, 1_000, None).unwrap();
        assert_eq!(r, 0..2);
        // Too short a chat: never.
        assert_eq!(due(&[5_000; 3], &users, None, 100, None), None);
    }

    #[test]
    fn every_providers_too_long_is_said_plainly() {
        use crate::ai::llm::{says_too_long, tokens_a_minute_refusal};
        // Each provider's real refusal (their documented shapes).
        let said = [
            // llama.cpp (the on-device model).
            (400, r#"{"error":{"code":400,"message":"the request exceeds the available context size, try increasing it","type":"exceed_context_size_error","n_prompt_tokens":9312,"n_ctx":8192}}"#),
            // OpenAI.
            (400, r#"{"error":{"message":"This model's maximum context length is 128000 tokens. However, your messages resulted in 131205 tokens. Please reduce the length of the messages.","type":"invalid_request_error","param":"messages","code":"context_length_exceeded"}}"#),
            // Anthropic.
            (400, r#"{"type":"error","error":{"type":"invalid_request_error","message":"prompt is too long: 210345 tokens > 200000 maximum"}}"#),
            (400, r#"{"type":"error","error":{"type":"invalid_request_error","message":"input length and `max_tokens` exceed context limit: 199000 + 8192 > 200000"}}"#),
            // Groq.
            (400, r#"{"error":{"message":"Please reduce the length of the messages or completion.","type":"invalid_request_error","param":"messages","code":"context_length_exceeded"}}"#),
            // Gemini (OpenAI-compatible endpoint).
            (400, r#"[{"error":{"code":400,"message":"The input token count (1150234) exceeds the maximum number of tokens allowed (1048576).","status":"INVALID_ARGUMENT"}}]"#),
            // OpenRouter.
            (400, r#"{"error":{"message":"This endpoint's maximum context length is 131072 tokens. However, you requested about 140210 tokens (139210 of text input, 1000 in the output). Please reduce the length of either one, or use the \"middle-out\" transform to compress your prompt automatically.","code":400}}"#),
            // LM Studio.
            (400, r#"{"error":"Trying to keep the first 9000 tokens when context the overflows. However, the model is loaded with context length of only 8192 tokens, which is not enough."}"#),
        ];
        for (status, body) in said {
            assert!(says_too_long(status, body), "{body}");
        }
        // Not about the context.
        assert!(!says_too_long(400, r#"{"error":{"message":"Invalid API key"}}"#));
        assert!(!says_too_long(200, "maximum context length"));
        assert!(!says_too_long(429, r#"{"error":{"message":"Rate limit reached for model in organization on tokens per minute (TPM): Limit 8000, Used 7000. Please try again in 9.5s."}}"#));
        // Groq: more than the plan sends in a minute, in one request.
        let tpm = r#"{"error":{"message":"Request too large for model `openai/gpt-oss-20b` in organization `org_example` service tier `on_demand` on tokens per minute (TPM): Limit 8000, Requested 9817, please reduce your message size and try again.","type":"tokens","code":"rate_limit_exceeded"}}"#;
        assert_eq!(tokens_a_minute_refusal(413, tpm), Some(8_000));
        assert_eq!(tokens_a_minute_refusal(400, r#"{"error":"bad"}"#), None);
        // Said plainly: never the raw JSON.
        let err = format!("{} (HTTP 400)", crate::ai::llm::CONTEXT_EXCEEDED);
        assert_eq!(plain_error(err, "Qwen3.5 4B", 65_536, "ondevice"), "That's too long for Qwen3.5 4B (64K context).");
        let err = format!("{}8000", crate::ai::llm::TOO_MUCH_A_MINUTE);
        assert_eq!(plain_error(err, "GPT-OSS 20B", 131_072, "groq"), "That's too long for GPT-OSS 20B on your Groq plan (8,000 tokens a minute).");
        assert_eq!(plain_error("HTTP 401: nope".into(), "x", 1, "groq"), "HTTP 401: nope");
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(800), "800");
    }

    #[test]
    fn contexts_and_models_are_named_as_the_pages_name_them() {
        assert_eq!(context_label(8_192), "8K");
        assert_eq!(context_label(65_536), "64K");
        assert_eq!(context_label(131_072), "128K");
        assert_eq!(context_label(128_000), "128K");
        assert_eq!(context_label(200_000), "200K");
        assert_eq!(context_label(1_048_576), "1M");
        assert_eq!(context_label(1_000_000), "1M");
        assert_eq!(too_long("Qwen3.5 4B", 65_536), "That's too long for Qwen3.5 4B (64K context).");
        assert!(too_long("x", 1).starts_with(TOO_LONG));
        assert_eq!(model_label("groq", "openai/gpt-oss-20b", ""), "GPT-OSS 20B");
        assert_eq!(model_label("ondevice", "Qwen_Qwen3.5-4B-Q4_K_M", "Qwen3.5 4B"), "Qwen3.5 4B");
        assert_eq!(default_context("groq"), 131_072);
        assert_eq!(default_context("custom"), 8_192);
    }

    #[test]
    fn a_message_is_counted_as_sent() {
        let call = serde_json::json!({ "role": "assistant", "content": null, "tool_calls": [{ "function": { "name": "web_search", "arguments": "{\"query\":\"lisbon\"}" } }] });
        assert_eq!(message_text(&call), " web_search {\"query\":\"lisbon\"}");
        let parts = serde_json::json!({ "role": "user", "content": [{ "type": "text", "text": "What's this?" }, { "type": "image_url" }] });
        assert_eq!(message_text(&parts), "What's this?");
    }

    #[tokio::test]
    async fn screenshots_cost_the_newest_three_whole() {
        let img = || crate::chat::chats::ChatImage { id: "img_0".into(), w: 10, h: 10, ..Default::default() };
        let m = |images: usize| ChatMessage { role: "user".into(), text: "hi".into(), images: (0..images).map(|_| img()).collect(), ..Default::default() };
        let costs = message_costs(&Counter::estimate_only(), &[m(2), m(0), m(2)], false).await;
        let base = estimate("hi") + MESSAGE_OVERHEAD;
        assert_eq!(costs, vec![base + IMAGE_TOKENS + 20, base, base + 2 * IMAGE_TOKENS]);
    }
}

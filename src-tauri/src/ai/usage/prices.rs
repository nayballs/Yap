//! **What a cloud model's tokens cost, estimated** (the AI usage tracker:
//! Chat's context ring panel and Insights' AI usage card). One table, at the
//! providers' own list prices for their paid plans, in US dollars per
//! million tokens: what the prompt costs (input) and what the answer costs
//! (output). Only the four providers whose prices are listed here (Claude,
//! OpenAI, Gemini, Groq), and only a call that went to that provider's own
//! service: OpenRouter, a custom endpoint and a server on this PC have none.
//! A model that isn't in the table has no estimate, never a guess.
//!
//! An estimate, and the page says so: a free plan (Groq's, Gemini's) costs
//! nothing, cached prompt tokens cost less than the list price, and a
//! provider changes its prices. The cost of each call is worked out when
//! it's recorded (`store.rs`), so a price for longer prompts or one that
//! changes on a date applies to the calls it covers.
//!
//! **Checked 2026-10-25** against [`SOURCES`]. Review it with the curated
//! model lists (`src/lib/core/ppModels.js`, `CURATED_AT`): add a row per new
//! model, move [`PRICES_AS_OF`].

/// The day the table was last checked against the providers' pages.
pub const PRICES_AS_OF: &str = "2026-10-25";

/// Where each provider's prices come from.
pub const SOURCES: [(&str, &str); 4] = [
    ("Anthropic", "https://platform.claude.com/docs/en/about-claude/pricing"),
    ("OpenAI", "https://developers.openai.com/api/docs/pricing"),
    ("Google", "https://ai.google.dev/gemini-api/docs/pricing"),
    ("Groq", "https://console.groq.com/docs/models"),
];

/// One price: US dollars per million tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    /// The prompt (input tokens).
    pub input: f64,
    /// The answer (output tokens, reasoning included).
    pub output: f64,
}

/// A row of the table.
struct Row {
    provider: &'static str,
    /// The id calls send (a dated snapshot of it matches too: [`base_id`]).
    model: &'static str,
    /// The first day (YYYY-MM-DD) this price applies; "" = always.
    from: &'static str,
    /// The longest prompt (tokens) this price is for; 0 = any length.
    upto: u64,
    price: Price,
}

const fn row(provider: &'static str, model: &'static str, from: &'static str, upto: u64, input: f64, output: f64) -> Row {
    Row { provider, model, from, upto, price: Price { input, output } }
}

/// The table. Prices for longer prompts sit beside the base price (`upto`);
/// a change announced for a date is its own row (`from`).
const TABLE: &[Row] = &[
    // Anthropic (first-party API rates).
    row("anthropic", "claude-fable-5-1", "", 0, 10.0, 50.0),
    row("anthropic", "claude-fable-5", "", 0, 10.0, 50.0),
    row("anthropic", "claude-opus-5-5", "", 0, 4.0, 20.0),
    row("anthropic", "claude-opus-5", "", 0, 5.0, 25.0),
    row("anthropic", "claude-opus-4-8", "", 0, 5.0, 25.0),
    row("anthropic", "claude-opus-4-7", "", 0, 5.0, 25.0),
    row("anthropic", "claude-opus-4-6", "", 0, 5.0, 25.0),
    row("anthropic", "claude-opus-4-5", "", 0, 5.0, 25.0),
    row("anthropic", "claude-sonnet-5-5", "", 0, 2.0, 10.0),
    row("anthropic", "claude-sonnet-5", "", 0, 2.0, 10.0),
    row("anthropic", "claude-sonnet-4-6", "", 0, 3.0, 15.0),
    row("anthropic", "claude-sonnet-4-5", "", 0, 3.0, 15.0),
    // Haiku 5.5: prompts up to 100K tokens, then more.
    row("anthropic", "claude-haiku-5-5", "", 100_000, 0.10, 0.50),
    row("anthropic", "claude-haiku-5-5", "", 0, 0.50, 2.50),
    row("anthropic", "claude-haiku-4-5", "", 0, 1.0, 5.0),
    // OpenAI (standard tier; prompts up to 272K tokens).
    row("openai", "gpt-6-astra", "", 272_000, 10.0, 50.0),
    row("openai", "gpt-6.1-sol", "", 272_000, 2.0, 10.0),
    row("openai", "gpt-6-luna", "", 272_000, 0.10, 0.50),
    row("openai", "gpt-5.6-sol", "", 272_000, 4.0, 20.0),
    row("openai", "gpt-5.6-terra", "", 272_000, 2.0, 12.0),
    row("openai", "gpt-5.6-luna", "", 272_000, 0.20, 1.20),
    row("openai", "gpt-5.5", "", 272_000, 5.0, 30.0),
    row("openai", "gpt-5.4-mini", "", 272_000, 0.75, 4.50),
    row("openai", "gpt-5", "", 272_000, 1.25, 10.0),
    row("openai", "gpt-5-mini", "", 272_000, 0.25, 2.0),
    row("openai", "gpt-5-nano", "", 272_000, 0.05, 0.40),
    // Google Gemini (paid tier, text). Argon's and the Flashes' prices go
    // up on the dates Google announced.
    row("gemini", "gemini-4-argon", "", 0, 2.0, 10.0),
    row("gemini", "gemini-4-argon", "2026-12-01", 0, 4.0, 20.0),
    row("gemini", "gemini-3.8-flash", "", 0, 0.75, 3.75),
    row("gemini", "gemini-3.8-flash", "2027-01-01", 0, 1.50, 7.50),
    row("gemini", "gemini-3.6-flash", "", 0, 0.75, 3.75),
    row("gemini", "gemini-3.6-flash", "2027-01-01", 0, 1.50, 7.50),
    row("gemini", "gemini-3.5-flash-lite", "", 0, 0.30, 2.50),
    row("gemini", "gemini-3.1-flash-lite", "", 0, 0.25, 1.50),
    row("gemini", "gemini-3.1-pro-preview", "", 200_000, 2.0, 12.0),
    row("gemini", "gemini-3.1-pro-preview", "", 0, 4.0, 18.0),
    row("gemini", "gemini-3-flash-preview", "", 0, 0.50, 3.0),
    row("gemini", "gemini-2.5-pro", "", 200_000, 1.25, 10.0),
    row("gemini", "gemini-2.5-pro", "", 0, 2.50, 15.0),
    row("gemini", "gemini-2.5-flash", "", 0, 0.30, 2.50),
    row("gemini", "gemini-2.5-flash-lite", "", 0, 0.10, 0.40),
    // Groq (on-demand, paid plan).
    row("groq", "openai/gpt-oss-120b", "", 0, 0.15, 0.60),
    row("groq", "openai/gpt-oss-20b", "", 0, 0.075, 0.30),
    row("groq", "qwen/qwen3.8-27b", "", 0, 0.80, 4.0),
];

/// A model id without a dated snapshot's suffix: "claude-haiku-4-5-20251001"
/// → "claude-haiku-4-5", "gpt-5-mini-2025-08-07" → "gpt-5-mini". Pure.
pub fn base_id(model: &str) -> &str {
    let m = model.trim();
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    // "-20251001"
    if let Some((head, tail)) = m.rsplit_once('-') {
        if tail.len() == 8 && digits(tail) {
            return head;
        }
    }
    // "-2025-08-07"
    let parts: Vec<&str> = m.rsplitn(4, '-').collect();
    if parts.len() == 4 && parts[0].len() == 2 && parts[1].len() == 2 && parts[2].len() == 4 && parts[..3].iter().all(|p| digits(p)) {
        return parts[3];
    }
    m
}

fn rows_for<'a>(provider: &'a str, model: &'a str) -> impl Iterator<Item = &'static Row> + 'a {
    let base = base_id(model);
    TABLE.iter().filter(move |r| r.provider == provider && (r.model == model || r.model == base))
}

/// Does the table have a price for `model` on `provider`'s own service? Pure.
pub fn has_price(provider: &str, model: &str) -> bool {
    rows_for(provider, model).next().is_some()
}

/// The price of one call to `model` on `day` (YYYY-MM-DD) with a prompt of
/// `prompt` tokens: the latest row in force that day, at the price for a
/// prompt that long. `None`: no price known (or none for a prompt that
/// long). Pure.
pub fn price(provider: &str, model: &str, day: &str, prompt: u64) -> Option<Price> {
    let in_force: Vec<&Row> = rows_for(provider, model).filter(|r| r.from <= day).collect();
    let latest = in_force.iter().map(|r| r.from).max()?;
    in_force
        .into_iter()
        .filter(|r| r.from == latest && (r.upto == 0 || prompt <= r.upto))
        // The tier made for this length (the smallest that holds it), else any length.
        .min_by_key(|r| if r.upto == 0 { u64::MAX } else { r.upto })
        .map(|r| r.price)
}

/// One call's estimated cost in millionths of a US dollar (`prompt` and
/// `answer` tokens at the price [`price`] finds). Pure.
pub fn cost_micros(provider: &str, model: &str, day: &str, prompt: u64, answer: u64) -> Option<u64> {
    let p = price(provider, model, day, prompt)?;
    // $ per million tokens × tokens = millionths of a dollar.
    Some((prompt as f64 * p.input + answer as f64 * p.output).round() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_calls_cost_is_its_tokens_at_the_list_price() {
        // 1,000 prompt + 200 answer tokens on Groq's GPT-OSS 20B:
        // 1,000 × $0.075/M + 200 × $0.30/M = $0.000075 + $0.00006.
        assert_eq!(cost_micros("groq", "openai/gpt-oss-20b", "2026-10-25", 1_000, 200), Some(135));
        // A million each on Claude Opus 5.5: $4 + $20.
        assert_eq!(cost_micros("anthropic", "claude-opus-5-5", "2026-10-25", 1_000_000, 1_000_000), Some(24_000_000));
        // Nothing for a model the table doesn't have, nor for another
        // service's copy of a model it has (OpenRouter's, a custom proxy's).
        assert_eq!(cost_micros("groq", "acme/brainy-9", "2026-10-25", 10, 10), None);
        assert_eq!(cost_micros("openrouter", "openai/gpt-oss-20b", "2026-10-25", 10, 10), None);
        assert_eq!(cost_micros("custom", "gpt-5-mini", "2026-10-25", 10, 10), None);
        assert_eq!(cost_micros("ondevice", "Qwen_Qwen3.5-4B-Q4_K_M", "2026-10-25", 10, 10), None);
        assert!(has_price("openai", "gpt-5-mini") && !has_price("openai", "gpt-9"));
    }

    #[test]
    fn a_longer_prompt_takes_its_own_price() {
        // Haiku 5.5: up to 100K tokens, then five times the price.
        assert_eq!(price("anthropic", "claude-haiku-5-5", "2026-10-25", 100_000), Some(Price { input: 0.10, output: 0.50 }));
        assert_eq!(price("anthropic", "claude-haiku-5-5", "2026-10-25", 100_001), Some(Price { input: 0.50, output: 2.50 }));
        // Gemini 3.1 Pro at 250K tokens.
        assert_eq!(cost_micros("gemini", "gemini-3.1-pro-preview", "2026-10-25", 250_000, 1_000), Some(250_000 * 4 + 18_000));
        // OpenAI's table is for prompts up to 272K: past it, no estimate.
        assert!(price("openai", "gpt-5.5", "2026-10-25", 300_000).is_none());
    }

    #[test]
    fn a_price_change_applies_from_its_day() {
        let argon = |day| price("gemini", "gemini-4-argon", day, 1_000).unwrap().input;
        assert_eq!(argon("2026-11-30"), 2.0);
        assert_eq!(argon("2026-12-01"), 4.0);
        assert_eq!(price("gemini", "gemini-3.6-flash", "2027-01-01", 10).unwrap().output, 7.50);
    }

    #[test]
    fn dated_snapshots_cost_what_their_model_costs() {
        assert_eq!(base_id("claude-haiku-4-5-20251001"), "claude-haiku-4-5");
        assert_eq!(base_id("gpt-5-mini-2025-08-07"), "gpt-5-mini");
        assert_eq!(base_id("gpt-5.6-luna"), "gpt-5.6-luna");
        assert_eq!(base_id("qwen/qwen3.8-27b"), "qwen/qwen3.8-27b");
        assert_eq!(cost_micros("anthropic", "claude-haiku-4-5-20251001", "2026-10-25", 1_000, 0), Some(1_000));
    }
}

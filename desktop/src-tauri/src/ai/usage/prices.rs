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
//! nothing, and a provider changes its prices. The cost of each call is
//! worked out when it's recorded (`store.rs`), so a price for longer prompts
//! or one that changes on a date applies to the calls it covers.
//!
//! **Cached prompt tokens at their own rates** ([`Cached`], 20 Nov 2026):
//! Claude's cache reads (0.1× input, 0.05× on Opus and Sonnet 5.5, 0.025×
//! on Fable 5.1 and 5.2) and its writes (1.25× input for 5 minutes, 2× for
//! an hour), as Anthropic's pricing page lists them. Before, every prompt
//! token was priced as new: a long computer-use run (each step re-reads the
//! whole conversation from the cache) came out at $63 when it cost about $8.
//! Another provider's cached tokens stay at its input price until its cached
//! rate is in the table.
//!
//! **Checked 2026-10-25** against [`SOURCES`]; Claude's rows, cache rates
//! included, **2026-11-20**. Review it with the curated model lists
//! (`desktop/src/lib/core/ppModels.js`, `CURATED_AT`): add a row per new
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
    /// The prompt's cached tokens, when the provider's rate for them is
    /// known (Claude's); else they cost `input`.
    pub cached: Option<CachedPrice>,
}

/// What a prompt's cached tokens cost: US dollars per million tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CachedPrice {
    /// Read from the cache (a hit or a refresh).
    pub read: f64,
    /// Written to it for 5 minutes (Claude: 1.25× input).
    pub write_5m: f64,
    /// Written to it for an hour (Claude: 2× input).
    pub write_1h: f64,
}

/// **A prompt's cached tokens** (part of its prompt, never on top): read
/// from the cache, and written to it for 5 minutes or an hour.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cached {
    pub read: u64,
    pub write_5m: u64,
    pub write_1h: u64,
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
    Row { provider, model, from, upto, price: Price { input, output, cached: None } }
}

/// A Claude row: its cache reads at `read`, its writes at Anthropic's 1.25×
/// (5 minutes) and 2× (an hour) the input price.
const fn claude(model: &'static str, upto: u64, input: f64, output: f64, read: f64) -> Row {
    Row { provider: "anthropic", model, from: "", upto, price: Price { input, output, cached: Some(CachedPrice { read, write_5m: input * 1.25, write_1h: input * 2.0 }) } }
}

/// The table. Prices for longer prompts sit beside the base price (`upto`);
/// a change announced for a date is its own row (`from`).
const TABLE: &[Row] = &[
    // Anthropic (first-party API rates; input, output and cache reads as its
    // pricing page lists them, checked 2026-11-20: reads 0.1× input but for
    // Opus and Sonnet 5.5's 0.05× and Fable 5.1 and 5.2's 0.025×).
    claude("claude-fable-5-2", 0, 8.0, 40.0, 0.20),
    claude("claude-fable-5-1", 0, 10.0, 50.0, 0.25),
    claude("claude-fable-5", 0, 10.0, 50.0, 1.0),
    claude("claude-opus-5-5", 0, 4.0, 20.0, 0.20),
    claude("claude-opus-5", 0, 5.0, 25.0, 0.50),
    claude("claude-opus-4-8", 0, 5.0, 25.0, 0.50),
    claude("claude-opus-4-7", 0, 5.0, 25.0, 0.50),
    claude("claude-opus-4-6", 0, 5.0, 25.0, 0.50),
    claude("claude-opus-4-5", 0, 5.0, 25.0, 0.50),
    claude("claude-sonnet-5-5", 0, 2.0, 10.0, 0.10),
    claude("claude-sonnet-5", 0, 2.0, 10.0, 0.20),
    claude("claude-sonnet-4-6", 0, 3.0, 15.0, 0.30),
    claude("claude-sonnet-4-5", 0, 3.0, 15.0, 0.30),
    // Haiku 5.5: prompts up to 100K tokens (its cached ones counted too), then more.
    claude("claude-haiku-5-5", 100_000, 0.10, 0.50, 0.01),
    claude("claude-haiku-5-5", 0, 0.50, 2.50, 0.05),
    claude("claude-haiku-4-5", 0, 1.0, 5.0, 0.10),
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
    // Spoken replies' cloud voices (`tts/hosted.rs`; checked 2026-10-31):
    // the text in, the audio out. OpenAI's TTS model shuts down on
    // 2027-01-06; Gemini's go up on 2027-01-01 (25 audio tokens a second).
    row("openai", "gpt-4o-mini-tts", "", 0, 0.60, 12.0),
    row("gemini", "gemini-3.8-flash-tts", "", 0, 0.50, 9.0),
    row("gemini", "gemini-3.8-flash-tts", "2027-01-01", 0, 1.0, 18.0),
    row("gemini", "gemini-3.8-flash-lite-tts", "", 0, 0.50, 6.0),
    row("gemini", "gemini-3.8-flash-lite-tts", "2027-01-01", 0, 1.0, 12.0),
];

/// Voices priced by the character, not the token: `(provider, model, US
/// dollars a million characters)`. Groq's Orpheus (checked 2026-10-31).
const PER_CHAR: &[(&str, &str, f64)] = &[("groq", "canopylabs/orpheus-v1-english", 22.0)];

/// A voice request's estimated cost by its text's characters, in millionths
/// of a US dollar, for a voice priced that way. Pure.
pub fn char_cost_micros(provider: &str, model: &str, chars: u64) -> Option<u64> {
    let (_, _, per_million) = PER_CHAR.iter().find(|(p, m, _)| *p == provider && *m == model)?;
    // $ per million characters × characters = millionths of a dollar.
    Some((chars as f64 * per_million).round() as u64)
}

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

/// The longest prompt `model`'s cheaper rate is for on `day`: the smallest
/// `upto` of the rows in force (Claude Haiku 5.5's 100K, OpenAI's 272K,
/// Gemini Pro's 200K); a longer prompt costs more, or has no price here.
/// `None`: one price at any length, or no price known. Pure.
pub fn price_line(provider: &str, model: &str, day: &str) -> Option<u64> {
    let in_force: Vec<&Row> = rows_for(provider, model).filter(|r| r.from <= day).collect();
    let latest = in_force.iter().map(|r| r.from).max()?;
    in_force.into_iter().filter(|r| r.from == latest && r.upto > 0).map(|r| r.upto).min()
}

/// One call's estimated cost in millionths of a US dollar (`prompt` and
/// `answer` tokens at the price [`price`] finds), none of its prompt cached:
/// the tests' shorthand. Pure.
#[cfg(test)]
pub fn cost_micros(provider: &str, model: &str, day: &str, prompt: u64, answer: u64) -> Option<u64> {
    cost_micros_cached(provider, model, day, prompt, Cached::default(), answer)
}

/// **One call's estimated cost** in millionths of a US dollar: its `prompt`
/// (every input token, `cached` among them: the length a tier is chosen by,
/// as Anthropic prices it) and its `answer`. The cached tokens at their own
/// rates where the table knows them, else at the input price. Pure.
pub fn cost_micros_cached(provider: &str, model: &str, day: &str, prompt: u64, cached: Cached, answer: u64) -> Option<u64> {
    let p = price(provider, model, day, prompt)?;
    // $ per million tokens × tokens = millionths of a dollar.
    let dollars = match p.cached {
        Some(c) => {
            let plain = prompt.saturating_sub(cached.read + cached.write_5m + cached.write_1h);
            plain as f64 * p.input + cached.read as f64 * c.read + cached.write_5m as f64 * c.write_5m + cached.write_1h as f64 * c.write_1h
        }
        None => prompt as f64 * p.input,
    };
    Some((dollars + answer as f64 * p.output).round() as u64)
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
    fn a_cloud_voice_is_priced_by_its_tokens_or_its_characters() {
        // OpenAI: 14 text tokens in, 101 audio tokens out.
        assert_eq!(cost_micros("openai", "gpt-4o-mini-tts-2025-12-15", "2026-10-31", 14, 101), Some((14.0 * 0.60 + 101.0 * 12.0_f64).round() as u64));
        // Gemini's Flash-Lite TTS: 6 s of audio is 150 tokens; double from 2027.
        assert_eq!(cost_micros("gemini", "gemini-3.8-flash-lite-tts", "2026-10-31", 10, 150), Some(905));
        assert_eq!(cost_micros("gemini", "gemini-3.8-flash-lite-tts", "2027-01-02", 10, 150), Some(1_810));
        // Groq's Orpheus by the character: 90 characters at $22 a million.
        assert_eq!(char_cost_micros("groq", "canopylabs/orpheus-v1-english", 90), Some(1_980));
        assert_eq!(char_cost_micros("groq", "openai/gpt-oss-20b", 90), None);
        // Its longest request, 200 characters, is under half a penny ($0.0044).
        assert_eq!(char_cost_micros("groq", "canopylabs/orpheus-v1-english", 200), Some(4_400));
    }

    #[test]
    fn a_longer_prompt_takes_its_own_price() {
        // Haiku 5.5: up to 100K tokens, then five times the price.
        let io = |p: Option<Price>| p.map(|p| (p.input, p.output));
        assert_eq!(io(price("anthropic", "claude-haiku-5-5", "2026-10-25", 100_000)), Some((0.10, 0.50)));
        assert_eq!(io(price("anthropic", "claude-haiku-5-5", "2026-10-25", 100_001)), Some((0.50, 2.50)));
        // Gemini 3.1 Pro at 250K tokens.
        assert_eq!(cost_micros("gemini", "gemini-3.1-pro-preview", "2026-10-25", 250_000, 1_000), Some(250_000 * 4 + 18_000));
        // OpenAI's table is for prompts up to 272K: past it, no estimate.
        assert!(price("openai", "gpt-5.5", "2026-10-25", 300_000).is_none());
    }

    #[test]
    fn the_price_line_is_the_longest_prompt_at_the_cheaper_rate() {
        assert_eq!(price_line("anthropic", "claude-haiku-5-5", "2026-10-27"), Some(100_000));
        assert_eq!(price_line("openai", "gpt-6-luna", "2026-10-27"), Some(272_000));
        assert_eq!(price_line("gemini", "gemini-2.5-pro", "2026-10-27"), Some(200_000));
        // One price at any length, another service's copy, no price: none.
        assert_eq!(price_line("anthropic", "claude-haiku-4-5", "2026-10-27"), None);
        assert_eq!(price_line("groq", "openai/gpt-oss-20b", "2026-10-27"), None);
        assert_eq!(price_line("openrouter", "anthropic/claude-haiku-5-5", "2026-10-27"), None);
        assert_eq!(price_line("anthropic", "claude-nova-9", "2026-10-27"), None);
    }

    #[test]
    fn a_price_change_applies_from_its_day() {
        let argon = |day| price("gemini", "gemini-4-argon", day, 1_000).unwrap().input;
        assert_eq!(argon("2026-11-30"), 2.0);
        assert_eq!(argon("2026-12-01"), 4.0);
        assert_eq!(price("gemini", "gemini-3.6-flash", "2027-01-01", 10).unwrap().output, 7.50);
    }

    /// **The run that found it** (the real-screen computer-use pass, 20 Nov
    /// 2026, Claude Haiku 4.5): its measured token mix — 54.4M read from the
    /// cache, 1.07M written to it for 5 minutes, 3.5K new, 84K answered —
    /// costs about $7.20 at the cache's own rates (the run's last two Lab
    /// starts; the first two, not measured, bring it to about $8), where every
    /// prompt token at the input price made it $55.85 (about $63 in all).
    #[test]
    fn a_long_cached_run_costs_what_anthropic_bills_not_its_list_price() {
        let cached = Cached { read: 54_358_273, write_5m: 1_068_615, write_1h: 0 };
        let prompt = cached.read + cached.write_5m + 3_470;
        let real = cost_micros_cached("anthropic", "claude-haiku-4-5", "2026-11-20", prompt, cached, 84_030).unwrap();
        assert!((7_150_000..=7_250_000).contains(&real), "{real}");
        // At $0.10 a million read, $1.25 written, $1 new, $5 answered.
        assert_eq!(real, (54_358_273.0_f64 * 0.10 + 1_068_615.0 * 1.25 + 3_470.0 * 1.0 + 84_030.0 * 5.0).round() as u64);
        let as_before = cost_micros("anthropic", "claude-haiku-4-5", "2026-11-20", prompt, 84_030).unwrap();
        assert!(as_before > 55_000_000, "{as_before}");
    }

    #[test]
    fn each_claude_models_cache_rates_are_its_own() {
        let one = |model: &str, prompt: u64, cached: Cached| cost_micros_cached("anthropic", model, "2026-11-20", prompt, cached, 0).unwrap();
        let read = |n| Cached { read: n, ..Default::default() };
        // A million read: 0.1× input on most, 0.05× on Opus and Sonnet 5.5, 0.025× on Fable 5.2.
        assert_eq!(one("claude-haiku-4-5", 1_000_000, read(1_000_000)), 100_000);
        assert_eq!(one("claude-opus-5-5", 1_000_000, read(1_000_000)), 200_000);
        assert_eq!(one("claude-sonnet-5-5", 1_000_000, read(1_000_000)), 100_000);
        assert_eq!(one("claude-fable-5-2", 1_000_000, read(1_000_000)), 200_000);
        assert_eq!(one("claude-fable-5", 1_000_000, read(1_000_000)), 1_000_000);
        // Writes: 1.25× input for 5 minutes, 2× for an hour (Sonnet 5.5: $2.50 and $4).
        assert_eq!(one("claude-sonnet-5-5", 1_000_000, Cached { write_5m: 1_000_000, ..Default::default() }), 2_500_000);
        assert_eq!(one("claude-sonnet-5-5", 1_000_000, Cached { write_1h: 1_000_000, ..Default::default() }), 4_000_000);
        // Haiku 5.5's tier is the whole prompt's, cached tokens counted: 90K read + 20K new is past 100K.
        assert_eq!(one("claude-haiku-5-5", 50_000, read(40_000)), (40_000.0_f64 * 0.01 + 10_000.0 * 0.10).round() as u64);
        assert_eq!(one("claude-haiku-5-5", 110_000, read(90_000)), (90_000.0_f64 * 0.05 + 20_000.0 * 0.50).round() as u64);
        // A provider whose cached rate isn't in the table: its cached tokens at the input price, as before.
        assert_eq!(cost_micros_cached("openai", "gpt-5-mini", "2026-11-20", 1_000_000, read(800_000), 0), cost_micros("openai", "gpt-5-mini", "2026-11-20", 1_000_000, 0));
        // More cached than the prompt (a provider's rounding): nothing new, never negative.
        assert_eq!(one("claude-haiku-4-5", 10, read(12)), 1);
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

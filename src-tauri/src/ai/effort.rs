//! **Effort** (2026-10-31): how hard a cloud model reasons before it
//! answers, chosen per model in Chat's model picker as in the Claude Code
//! app ("Opus 5.5 Extra"), remembered per model (`YapConfig::chat_effort`),
//! and sent wherever that model answers a question: Chat, the Voice
//! Assistant, the Notes view's Ask bar and a meeting's Ask (the notepad's
//! typed question). Dictation cleanup, Commands, the meeting notes and
//! memory keep their own fast requests: they never come here.
//!
//! Levels are Claude Code's five — low, medium, high, xhigh (shown as
//! "Extra") and max — and a model shows only the ones it takes. A model
//! with no reasoning control has none, and no Effort item. Unchosen, a
//! model runs at its provider's own default, said by [`Caps::default`].
//!
//! **The one table** ([`caps`]), from each provider's docs (read 31 Oct
//! 2026), the provider's own list first where it says
//! (`model_catalog::CatalogModel::efforts`: Anthropic's
//! `capabilities.effort`, OpenRouter's `reasoning.supported_efforts`):
//! - **Claude** (Anthropic's Messages API only: its OpenAI-compatible
//!   endpoint ignores `reasoning_effort` and refuses adaptive thinking, so a
//!   Claude call with an effort goes to `/messages`, `llm/anthropic.rs`):
//!   `output_config.effort` with adaptive thinking, its summary shown. All
//!   five on Fable 5/5.1, Mythos 5/5.1, Opus 5.5/5/4.8/4.7, Sonnet 5.5/5,
//!   Haiku 5.5; no xhigh on Opus 4.6, Sonnet 4.6 and Mythos Preview; low to
//!   high on Opus 4.5 (no adaptive thinking there: the effort alone). The
//!   default is medium on Opus 5.5 and Haiku 5.5, high on the rest.
//!   `budget_tokens` is refused by the newest models, so it's never sent;
//!   Haiku 4.5, Sonnet 4.5 and older have no effort. At xhigh and max the
//!   answer may use up to 64K tokens, as Anthropic advises.
//! - **Fast mode**: Claude Opus 5.5 on Anthropic's own API, `speed:
//!   "fast"` (no beta header on Opus 5.5; Opus 5 and 4.8 are a research
//!   preview behind a waitlist, and 4.8's ends on 30 Nov 2026, so not
//!   offered). Twice the price: off unless switched on (`fast_mode`).
//! - **OpenAI**: `reasoning_effort`. GPT-5.6 and GPT-6 low to max; GPT-5.5
//!   low to xhigh; GPT-5.2–5.4 low to xhigh and GPT-5.1 low to high, their
//!   default "none" (no reasoning: the "Default" choice); GPT-5 and the
//!   o-series low to high, medium by default. The GPT-6 family takes tools
//!   on Chat Completions only with `"none"`, so a call with tools sends it.
//! - **Groq**: `reasoning_effort` — GPT-OSS low to high (medium); Qwen3
//!   low to high (its default is Groq's "default"); Muse Spark Open low to
//!   xhigh (xhigh).
//! - **Gemini** (its OpenAI-compatible endpoint): `reasoning_effort` low,
//!   medium or high on 2.5 and later, mapped by Google to a thinking level
//!   or budget; high by default.
//! - **OpenRouter**: `reasoning: {effort}` (it maps to each model's own:
//!   a budget for Claude, a thinking level for Gemini) — its list's levels,
//!   else the family's, else low to high for a model it says reasons.
//!
//! The on-device model has no effort: Chat's Think button covers it
//! (`chat/think.rs`), and only it.

use serde::Serialize;
use serde_json::{json, Map, Value};
use std::time::Duration;

use crate::ai::llm::ChatRequest;

/// How hard a model reasons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Low,
    Medium,
    High,
    /// "Extra" in the picker.
    Xhigh,
    Max,
}

impl Level {
    /// Every level, lowest first.
    pub const ALL: [Level; 5] = [Level::Low, Level::Medium, Level::High, Level::Xhigh, Level::Max];

    /// As the APIs and config.json name it.
    pub fn id(self) -> &'static str {
        match self {
            Level::Low => "low",
            Level::Medium => "medium",
            Level::High => "high",
            Level::Xhigh => "xhigh",
            Level::Max => "max",
        }
    }

    pub fn parse(s: &str) -> Option<Level> {
        Level::ALL.into_iter().find(|l| l.id() == s.trim())
    }
}

use Level::{High, Low, Max, Medium, Xhigh};

const ALL: &[Level] = &Level::ALL;
const TO_XHIGH: &[Level] = &[Low, Medium, High, Xhigh];
const NO_XHIGH: &[Level] = &[Low, Medium, High, Max];
const TO_HIGH: &[Level] = &[Low, Medium, High];

/// What a model takes.
#[derive(Debug, Clone, PartialEq)]
pub struct Caps {
    /// Lowest first.
    pub levels: Vec<Level>,
    /// What it runs at when none is sent; `None`: not one of these (OpenAI's
    /// "none", Groq's "default"), so "Default" sends nothing.
    pub default: Option<Level>,
    /// Fast mode (Claude Opus 5.5 on Anthropic).
    pub fast: bool,
}

/// A Claude by its alias (a dated id's alias, `claude-opus-4-5-20251101` →
/// `claude-opus-4-5`).
fn claude_alias(model: &str) -> String {
    let m = model.trim().to_ascii_lowercase();
    crate::ai::model_catalog::snapshot_base(&m).unwrap_or(m)
}

/// Anthropic's table: the levels and the default. Pure.
fn claude(alias: &str) -> Option<(&'static [Level], Level)> {
    const FIVE: [&str; 11] = [
        "claude-fable-5-1",
        "claude-mythos-5-1",
        "claude-fable-5",
        "claude-mythos-5",
        "claude-opus-5-5",
        "claude-opus-5",
        "claude-sonnet-5-5",
        "claude-sonnet-5",
        "claude-haiku-5-5",
        "claude-opus-4-8",
        "claude-opus-4-7",
    ];
    let default = if matches!(alias, "claude-opus-5-5" | "claude-haiku-5-5") { Medium } else { High };
    if FIVE.contains(&alias) {
        return Some((ALL, default));
    }
    match alias {
        "claude-mythos-preview" | "claude-opus-4-6" | "claude-sonnet-4-6" => Some((NO_XHIGH, High)),
        "claude-opus-4-5" => Some((TO_HIGH, High)),
        _ => None,
    }
}

/// Does this Claude think adaptively (every Claude with an effort but Opus
/// 4.5, which thinks to a budget, never sent here)? Pure.
fn claude_adaptive(model: &str) -> bool {
    claude_alias(model) != "claude-opus-4-5"
}

/// Fast mode: Claude Opus 5.5 on Anthropic's own API. Pure.
fn claude_fast(provider: &str, model: &str) -> bool {
    provider == "anthropic" && claude_alias(model) == "claude-opus-5-5"
}

/// OpenAI's table (a bare id: "gpt-5.6-luna", "o4-mini"). Pure.
fn openai(model: &str) -> Option<(&'static [Level], Option<Level>)> {
    let m = model.trim().to_ascii_lowercase();
    let m = m.rsplit('/').next().unwrap_or(&m);
    if m.contains("-chat") || m.starts_with("o1-mini") {
        return None;
    }
    if ["o1", "o3", "o4"].iter().any(|p| m.starts_with(p)) {
        return Some((TO_HIGH, Some(Medium)));
    }
    static GPT: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"^gpt-(\d+)(?:\.(\d+))?(?:$|-)").expect("static regex"));
    let c = GPT.captures(m)?;
    let major: u32 = c[1].parse().ok()?;
    let minor: u32 = c.get(2).and_then(|x| x.as_str().parse().ok()).unwrap_or(0);
    match (major, minor) {
        (5, 0) => Some((TO_HIGH, Some(Medium))),
        (5, 1) => Some((TO_HIGH, None)),
        (5, 2..=4) => Some((TO_XHIGH, None)),
        (5, 5) => Some((TO_XHIGH, Some(Medium))),
        (5, _) => Some((ALL, Some(Medium))),
        (6.., _) => Some((ALL, Some(Medium))),
        _ => None,
    }
}

/// The GPT-6 family: tools on Chat Completions only with its reasoning
/// off (OpenAI's model pages, 31 Oct 2026). Pure.
fn openai_tools_without_reasoning(model: &str) -> bool {
    let m = model.trim().to_ascii_lowercase();
    m.rsplit('/').next().unwrap_or(&m).starts_with("gpt-6")
}

/// Gemini's thinking models (2.5 and later; not image, speech or live
/// ones). Pure.
fn gemini(model: &str) -> Option<(&'static [Level], Option<Level>)> {
    let m = model.trim().to_ascii_lowercase();
    let m = m.trim_start_matches("models/");
    let m = m.rsplit('/').next().unwrap_or(m);
    static GEMINI: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"^gemini-(\d+)(?:\.(\d+))?-").expect("static regex"));
    let c = GEMINI.captures(m)?;
    let major: u32 = c[1].parse().ok()?;
    let minor: u32 = c.get(2).and_then(|x| x.as_str().parse().ok()).unwrap_or(0);
    let thinks = major >= 3 || (major == 2 && minor >= 5);
    let other = ["image", "tts", "live", "audio", "embedding"].iter().any(|x| m.contains(x));
    (thinks && !other).then_some((TO_HIGH, Some(High)))
}

/// Groq's reasoning models. Pure.
fn groq(model: &str) -> Option<(&'static [Level], Option<Level>)> {
    let m = model.trim().to_ascii_lowercase();
    if m.contains("gpt-oss") && !m.contains("safeguard") {
        return Some((TO_HIGH, Some(Medium)));
    }
    if m.contains("muse-spark") {
        return Some((TO_XHIGH, Some(Xhigh)));
    }
    if m.contains("qwen3") {
        return Some((TO_HIGH, None));
    }
    None
}

/// OpenRouter by family (`anthropic/claude-opus-5.5`, `openai/gpt-5-mini`,
/// `google/gemini-3-flash-preview`): the family's own table. Pure.
fn openrouter_family(model: &str) -> Option<(Vec<Level>, Option<Level>)> {
    let m = model.trim().to_ascii_lowercase();
    let (lab, id) = m.split_once('/').unwrap_or(("", m.as_str()));
    let id = id.split(':').next().unwrap_or(id);
    match lab {
        "anthropic" => claude(&claude_alias(&id.replace('.', "-"))).map(|(l, d)| (l.to_vec(), Some(d))),
        "openai" if id.contains("gpt-oss") => Some((TO_HIGH.to_vec(), Some(Medium))),
        "openai" => openai(id).map(|(l, d)| (l.to_vec(), d)),
        "google" => gemini(id).map(|(l, d)| (l.to_vec(), d)),
        _ if id.contains("deepseek-r1") || id.contains("qwen3") => Some((TO_HIGH.to_vec(), None)),
        _ => None,
    }
}

/// The levels the provider's list gives `model`, and its default there.
fn listed(provider: &str, model: &str) -> Option<(Vec<Level>, Option<Level>, Option<bool>)> {
    let m = crate::ai::model_catalog::listed_model(provider, model)?;
    let levels: Vec<Level> = m.efforts.iter().filter_map(|e| Level::parse(e)).collect();
    let default = m.default_effort.as_deref().and_then(Level::parse);
    Some((levels, default, m.thinks))
}

/// **What `provider`'s `model` takes** — `None`: no reasoning control (no
/// Effort item). The provider's list first, where it says; then the
/// table.
pub fn caps(provider: &str, model: &str) -> Option<Caps> {
    let list = matches!(provider, "anthropic" | "openrouter").then(|| listed(provider, model)).flatten();
    let from_list = list.as_ref().filter(|(l, _, _)| !l.is_empty());
    let (levels, default): (Vec<Level>, Option<Level>) = match provider {
        "anthropic" => {
            let table = claude(&claude_alias(model));
            match (from_list, table) {
                (Some((l, _, _)), t) => (l.clone(), t.map(|(_, d)| d).filter(|d| l.contains(d)).or_else(|| l.contains(&High).then_some(High))),
                (None, Some((l, d))) => (l.to_vec(), Some(d)),
                (None, None) => return None,
            }
        }
        "openai" => openai(model).map(|(l, d)| (l.to_vec(), d))?,
        "groq" => groq(model).map(|(l, d)| (l.to_vec(), d))?,
        "gemini" => gemini(model).map(|(l, d)| (l.to_vec(), d))?,
        "openrouter" => {
            let family = openrouter_family(model);
            match (from_list, family) {
                (Some((l, d, _)), f) => (l.clone(), d.or_else(|| f.and_then(|(_, d)| d)).filter(|d| l.contains(d))),
                (None, Some(f)) => f,
                (None, None) if list.as_ref().is_some_and(|(_, _, thinks)| *thinks == Some(true)) => {
                    (TO_HIGH.to_vec(), list.as_ref().and_then(|(_, d, _)| *d).filter(|d| TO_HIGH.contains(d)))
                }
                (None, None) => return None,
            }
        }
        _ => return None,
    };
    if levels.is_empty() {
        return None;
    }
    Some(Caps { levels, default, fast: claude_fast(provider, model) })
}

/// **The request's fields** for `level` on `provider`'s `model` (`fast`:
/// fast mode, where it's offered; `tools`: the call carries tools). Pure.
pub fn fields(provider: &str, model: &str, level: Level, fast: bool, tools: bool) -> Map<String, Value> {
    let v = match provider {
        "anthropic" => {
            let mut v = json!({ "output_config": { "effort": level.id() } });
            if claude_adaptive(model) {
                // Its summary shown (`anthropic::body` adds `display`).
                v["thinking"] = json!({ "type": "adaptive" });
            }
            // Room to think and answer at the top levels (Anthropic: start at 64K).
            if level >= Xhigh {
                v["max_tokens"] = json!(64_000);
            }
            if fast && claude_fast(provider, model) {
                v["speed"] = json!("fast");
            }
            v
        }
        "openai" if tools && openai_tools_without_reasoning(model) => json!({ "reasoning_effort": "none" }),
        "openai" | "groq" | "gemini" => json!({ "reasoning_effort": level.id() }),
        "openrouter" => json!({ "reasoning": { "effort": level.id() } }),
        _ => json!({}),
    };
    v.as_object().cloned().unwrap_or_default()
}

/// How long a call at `level` may take: a model that reasons harder takes
/// longer (a cloud call's usual 20 s would cut it off).
pub fn timeout(level: Level) -> Duration {
    if level >= Xhigh {
        Duration::from_secs(300)
    } else {
        crate::chat::think::TIMEOUT
    }
}

/// The key a model's effort is kept under (`YapConfig::chat_effort`). Pure.
pub fn key(provider: &str, model: &str) -> String {
    format!("{provider}:{}", model.trim())
}

/// The level chosen for `model` in `chosen` (the saved map), if it's one it
/// takes. Pure.
fn chosen_in(chosen: &std::collections::BTreeMap<String, String>, caps: &Caps, provider: &str, model: &str) -> Option<Level> {
    chosen.get(&key(provider, model)).and_then(|s| Level::parse(s)).filter(|l| caps.levels.contains(l))
}

/// The saved choices (and fast mode). A unit test never reads config.json:
/// its own, set with [`set_for_test`].
fn saved() -> (std::collections::BTreeMap<String, String>, bool) {
    #[cfg(test)]
    {
        TEST.with(|t| t.borrow().clone())
    }
    #[cfg(not(test))]
    {
        let cfg = crate::config::load();
        (cfg.chat_effort, cfg.fast_mode)
    }
}

#[cfg(test)]
thread_local! {
    static TEST: std::cell::RefCell<(std::collections::BTreeMap<String, String>, bool)> = Default::default();
}

/// Test: `model`'s level as if chosen (`None`: its default), and fast mode.
#[cfg(test)]
pub fn set_for_test(provider: &str, model: &str, level: Option<Level>, fast: bool) {
    TEST.with(|t| {
        let mut t = t.borrow_mut();
        match level {
            Some(l) => t.0.insert(key(provider, model), l.id().to_string()),
            None => t.0.remove(&key(provider, model)),
        };
        t.1 = fast;
    });
}

/// **The level a call to `model` runs at** (the one chosen, else its
/// default) and whether fast mode is on for it; `None`: nothing to send.
pub fn for_call(provider: &str, model: &str) -> Option<(Level, bool)> {
    let caps = caps(provider, model)?;
    let (chosen, fast) = saved();
    let level = chosen_in(&chosen, &caps, provider, model).or(caps.default)?;
    Some((level, fast && caps.fast))
}

/// **A question's request at its model's effort** (`ChatRequest::chat`, the
/// notepad's Ask): the fields, more time, and — Claude — Anthropic's
/// Messages API, the only one that takes them. `false`: nothing to add (a
/// model without effort, "Default" on one whose default isn't a level).
pub(crate) fn apply(req: &mut ChatRequest) -> bool {
    let Some((level, fast)) = for_call(&req.provider, &req.model) else {
        return false;
    };
    if req.provider == "anthropic" {
        if !crate::ai::llm::anthropic::speaks_messages(&req.provider, &req.model) {
            return false;
        }
        req.native.get_or_insert_with(Default::default);
    }
    req.extra.extend(fields(&req.provider, &req.model, level, fast, req.tools.is_some()));
    req.timeout = req.timeout.max(timeout(level));
    true
}

// ---- the page ---------------------------------------------------------------------

/// What the picker (and Settings) shows for a model.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    /// Lowest first ("low" … "max").
    pub levels: Vec<&'static str>,
    /// Its own default; `null`: "Default" (not one of the levels).
    pub default: Option<&'static str>,
    /// The person's choice; `null`: the default.
    pub chosen: Option<&'static str>,
    /// Fast mode: `null` when the model hasn't one, else whether it's on.
    pub fast: Option<bool>,
}

/// `model`'s effort, as the page shows it (`None`: no Effort item). Pure,
/// but for the provider's list.
pub fn info(cfg: &crate::config::YapConfig, provider: &str, model: &str) -> Option<Info> {
    let caps = caps(provider, model)?;
    Some(Info {
        levels: caps.levels.iter().map(|l| l.id()).collect(),
        default: caps.default.map(Level::id),
        chosen: chosen_in(&cfg.chat_effort, &caps, provider, model).map(Level::id),
        fast: caps.fast.then_some(cfg.fast_mode),
    })
}

/// Keep `level` for `model` (`None`: back to its default). Pure.
pub fn set(cfg: &mut crate::config::YapConfig, provider: &str, model: &str, level: Option<&str>) -> Result<(), String> {
    let caps = caps(provider, model).ok_or("This model has no effort setting")?;
    match level.map(str::trim).filter(|l| !l.is_empty()) {
        None => {
            cfg.chat_effort.remove(&key(provider, model));
        }
        Some(l) => {
            let l = Level::parse(l).filter(|l| caps.levels.contains(l)).ok_or_else(|| format!("This model doesn't take “{l}”"))?;
            cfg.chat_effort.insert(key(provider, model), l.id().to_string());
        }
    }
    Ok(())
}

/// A model's effort, for Settings → Language Models → Chat.
#[tauri::command]
pub fn effort_info(provider: String, model: String) -> Option<Info> {
    info(&crate::config::load(), &provider, &model)
}

/// The picker's (and Settings') Effort: `level` kept for `model`, saved as
/// Settings saves; the Chat model's status back (the chip shows it).
#[tauri::command]
pub fn chat_set_effort(
    app: tauri::AppHandle,
    provider: String,
    model: String,
    level: Option<String>,
) -> Result<crate::ai::ai_model::Status, String> {
    let cfg = crate::shell::settings::change_config(&app, |cfg| set(cfg, &provider, &model, level.as_deref()))?;
    tracing::info!(provider, level = level.as_deref().unwrap_or("default"), "Chat: effort set");
    Ok(crate::ai::ai_model::status(&cfg, crate::ai::ai_model::Scope::Chat, &crate::ai::ai_model::OnDevice::here(&cfg)))
}

/// The picker's Fast mode (Claude Opus 5.5).
#[tauri::command]
pub fn chat_set_fast(app: tauri::AppHandle, on: bool) -> Result<crate::ai::ai_model::Status, String> {
    let cfg = crate::shell::settings::change_config(&app, |cfg| {
        cfg.fast_mode = on;
        Ok(())
    })?;
    tracing::info!(on, "Chat: fast mode set");
    Ok(crate::ai::ai_model::status(&cfg, crate::ai::ai_model::Scope::Chat, &crate::ai::ai_model::OnDevice::here(&cfg)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(c: &Caps) -> Vec<&'static str> {
        c.levels.iter().map(|l| l.id()).collect()
    }

    #[test]
    fn claude_shows_the_levels_each_model_takes() {
        let c = caps("anthropic", "claude-opus-5-5").unwrap();
        assert_eq!(ids(&c), ["low", "medium", "high", "xhigh", "max"]);
        assert_eq!(c.default, Some(Medium));
        assert!(c.fast, "fast mode: Opus 5.5 only");
        assert_eq!(caps("anthropic", "claude-haiku-5-5").unwrap().default, Some(Medium));
        let sonnet = caps("anthropic", "claude-sonnet-5-5").unwrap();
        assert_eq!((sonnet.levels.len(), sonnet.default, sonnet.fast), (5, Some(High), false));
        assert_eq!(caps("anthropic", "claude-fable-5-1").unwrap().levels.len(), 5);
        assert_eq!(ids(&caps("anthropic", "claude-opus-4-6").unwrap()), ["low", "medium", "high", "max"]);
        assert_eq!(ids(&caps("anthropic", "claude-sonnet-4-6").unwrap()), ["low", "medium", "high", "max"]);
        assert_eq!(ids(&caps("anthropic", "claude-opus-4-5-20251101").unwrap()), ["low", "medium", "high"], "a dated id by its alias");
        assert!(!caps("anthropic", "claude-opus-5").unwrap().fast, "a waitlist preview: not offered");
        // No effort: no Effort item.
        assert!(caps("anthropic", "claude-haiku-4-5").is_none());
        assert!(caps("anthropic", "claude-sonnet-4-5-20250929").is_none());
        assert!(caps("anthropic", "claude-opus-4-1").is_none());
    }

    #[test]
    fn other_providers_and_models_without_a_control() {
        let gpt = |m: &str| caps("openai", m).map(|c| (ids(&c), c.default.map(Level::id)));
        assert_eq!(gpt("gpt-5.6-luna"), Some((vec!["low", "medium", "high", "xhigh", "max"], Some("medium"))));
        assert_eq!(gpt("gpt-6.1-sol").unwrap().0.len(), 5);
        assert_eq!(gpt("gpt-6-astra").unwrap().0.len(), 5);
        assert_eq!(gpt("gpt-5.5"), Some((vec!["low", "medium", "high", "xhigh"], Some("medium"))));
        assert_eq!(gpt("gpt-5.4-mini"), Some((vec!["low", "medium", "high", "xhigh"], None)), "its default is no reasoning");
        assert_eq!(gpt("gpt-5-mini"), Some((vec!["low", "medium", "high"], Some("medium"))));
        assert_eq!(gpt("o4-mini").unwrap().0.len(), 3);
        assert!(gpt("gpt-4o").is_none() && gpt("gpt-5-chat-latest").is_none() && gpt("gpt-4.1").is_none());

        let groq = |m: &str| caps("groq", m).map(|c| (ids(&c), c.default.map(Level::id)));
        assert_eq!(groq("openai/gpt-oss-20b"), Some((vec!["low", "medium", "high"], Some("medium"))));
        assert_eq!(groq("qwen/qwen3.8-27b"), Some((vec!["low", "medium", "high"], None)));
        assert_eq!(groq("meta-models/muse-spark-open"), Some((vec!["low", "medium", "high", "xhigh"], Some("xhigh"))));
        assert!(groq("llama-3.3-70b-versatile").is_none());

        assert_eq!(ids(&caps("gemini", "gemini-3.6-flash").unwrap()), ["low", "medium", "high"]);
        assert!(caps("gemini", "gemini-2.5-flash").is_some() && caps("gemini", "models/gemini-4-argon").is_some());
        assert!(caps("gemini", "gemini-2.0-flash").is_none() && caps("gemini", "gemma-4-31b-it").is_none());
        assert!(caps("gemini", "gemini-2.5-flash-preview-tts").is_none());

        assert_eq!(ids(&caps("openrouter", "anthropic/claude-opus-5.5").unwrap()).len(), 5, "Claude's own levels");
        assert_eq!(caps("openrouter", "openai/gpt-5-mini").unwrap().default, Some(Medium));
        assert_eq!(caps("openrouter", "openai/gpt-oss-20b").unwrap().levels.len(), 3);
        assert!(caps("openrouter", "google/gemini-3-flash-preview").is_some());
        assert!(caps("openrouter", "meta-llama/llama-3.3-70b-instruct").is_none());
        assert!(!caps("openrouter", "anthropic/claude-opus-5.5").unwrap().fast, "fast mode on Anthropic's own API only");

        // On this PC and self-hosted: Think, not effort.
        assert!(caps("ondevice", "Qwen_Qwen3.5-4B-Q4_K_M").is_none());
        assert!(caps("local", "qwen3:8b").is_none());
        assert!(caps("custom", "gpt-5.6-luna").is_none());
    }

    #[test]
    fn a_providers_list_comes_before_the_table() {
        let m = |id: &str, efforts: &[&str], default: Option<&str>, thinks: Option<bool>| crate::ai::model_catalog::CatalogModel {
            id: id.into(),
            efforts: efforts.iter().map(|s| s.to_string()).collect(),
            default_effort: default.map(str::to_string),
            thinks,
            ..Default::default()
        };
        crate::ai::model_catalog::set_models_for_test(
            "anthropic",
            vec![m("claude-nova-6", &["low", "medium", "high", "xhigh", "max"], None, None), m("claude-opus-4-6", &["low", "high"], None, None)],
        );
        // A Claude newer than the table: the list's levels, high by default.
        let nova = caps("anthropic", "claude-nova-6").unwrap();
        assert_eq!((nova.levels.len(), nova.default), (5, Some(High)));
        assert_eq!(ids(&caps("anthropic", "claude-opus-4-6").unwrap()), ["low", "high"]);
        crate::ai::model_catalog::set_models_for_test(
            "openrouter",
            vec![m("acme/thinker-2", &["low", "medium"], Some("low"), Some(true)), m("acme/ponder-1", &[], Some("medium"), Some(true)), m("acme/chat-1", &[], None, Some(false))],
        );
        let t = caps("openrouter", "acme/thinker-2").unwrap();
        assert_eq!((ids(&t), t.default), (vec!["low", "medium"], Some(Low)));
        let p = caps("openrouter", "acme/ponder-1").unwrap();
        assert_eq!((p.levels.len(), p.default), (3, Some(Medium)), "it reasons: low to high");
        assert!(caps("openrouter", "acme/chat-1").is_none());
    }

    /// Each provider's request body at each level (no network).
    #[test]
    fn each_provider_asks_its_own_way_at_every_level() {
        for level in Level::ALL {
            // Claude: the effort with adaptive thinking; 64K of room at the top two.
            let c = fields("anthropic", "claude-opus-5-5", level, false, true);
            assert_eq!(c["output_config"], json!({ "effort": level.id() }));
            assert_eq!(c["thinking"], json!({ "type": "adaptive" }));
            assert!(!c.contains_key("budget_tokens") && c["thinking"].get("budget_tokens").is_none(), "refused by the newest models");
            assert_eq!(c.get("max_tokens"), (level >= Xhigh).then(|| json!(64_000)).as_ref());
            assert!(!c.contains_key("speed") && !c.contains_key("reasoning_effort"));

            let o = fields("openai", "gpt-5.6-luna", level, false, true);
            assert_eq!(Value::Object(o), json!({ "reasoning_effort": level.id() }));
            let g = fields("groq", "openai/gpt-oss-20b", level, false, true);
            assert_eq!(Value::Object(g), json!({ "reasoning_effort": level.id() }));
            let gm = fields("gemini", "gemini-3.6-flash", level, false, false);
            assert_eq!(Value::Object(gm), json!({ "reasoning_effort": level.id() }));
            let r = fields("openrouter", "openai/gpt-5-mini", level, false, true);
            assert_eq!(Value::Object(r), json!({ "reasoning": { "effort": level.id() } }));
        }
        // Opus 4.5: the effort alone (it thinks to a budget, never sent).
        let old = fields("anthropic", "claude-opus-4-5", High, false, false);
        assert_eq!(Value::Object(old), json!({ "output_config": { "effort": "high" } }));
        // Fast mode: Opus 5.5 only, when on.
        assert_eq!(fields("anthropic", "claude-opus-5-5", Medium, true, false)["speed"], "fast");
        assert!(!fields("anthropic", "claude-sonnet-5-5", Medium, true, false).contains_key("speed"));
        // The GPT-6 family with tools: reasoning off, as Chat Completions wants.
        assert_eq!(fields("openai", "gpt-6-luna", High, false, true)["reasoning_effort"], "none");
        assert_eq!(fields("openai", "gpt-6-luna", High, false, false)["reasoning_effort"], "high");
        assert!(fields("custom", "gpt-5.6-luna", High, false, false).is_empty());
    }

    #[test]
    fn the_choice_is_kept_per_model_and_checked() {
        let mut cfg = crate::config::YapConfig::default();
        set(&mut cfg, "anthropic", "claude-opus-5-5", Some("xhigh")).unwrap();
        set(&mut cfg, "groq", "openai/gpt-oss-20b", Some("low")).unwrap();
        assert_eq!(cfg.chat_effort.get("anthropic:claude-opus-5-5").map(String::as_str), Some("xhigh"));
        let i = info(&cfg, "anthropic", "claude-opus-5-5").unwrap();
        assert_eq!((i.chosen, i.default, i.fast), (Some("xhigh"), Some("medium"), Some(false)));
        assert_eq!(info(&cfg, "anthropic", "claude-sonnet-5-5").unwrap().chosen, None, "per model");
        assert_eq!(info(&cfg, "groq", "openai/gpt-oss-20b").unwrap().chosen, Some("low"));
        assert!(info(&cfg, "anthropic", "claude-haiku-4-5").is_none());
        // Not a level it takes; no effort at all.
        assert!(set(&mut cfg, "groq", "openai/gpt-oss-20b", Some("max")).is_err());
        assert!(set(&mut cfg, "anthropic", "claude-haiku-4-5", Some("low")).is_err());
        // Back to its default.
        set(&mut cfg, "anthropic", "claude-opus-5-5", None).unwrap();
        assert!(!cfg.chat_effort.contains_key("anthropic:claude-opus-5-5"));
        cfg.fast_mode = true;
        assert_eq!(info(&cfg, "anthropic", "claude-opus-5-5").unwrap().fast, Some(true));
        assert_eq!(info(&cfg, "anthropic", "claude-sonnet-5-5").unwrap().fast, None);
    }

    #[test]
    fn a_call_runs_at_the_chosen_level_else_the_default() {
        set_for_test("anthropic", "claude-opus-5-5", None, false);
        assert_eq!(for_call("anthropic", "claude-opus-5-5"), Some((Medium, false)));
        set_for_test("anthropic", "claude-opus-5-5", Some(Max), true);
        assert_eq!(for_call("anthropic", "claude-opus-5-5"), Some((Max, true)));
        assert_eq!(for_call("anthropic", "claude-sonnet-5-5"), Some((High, false)), "fast mode: Opus 5.5 only");
        // "Default" where the default isn't a level: nothing sent.
        assert_eq!(for_call("openai", "gpt-5.4-mini"), None);
        set_for_test("openai", "gpt-5.4-mini", Some(Low), false);
        assert_eq!(for_call("openai", "gpt-5.4-mini"), Some((Low, false)));
        assert_eq!(for_call("ondevice", "Qwen_Qwen3.5-4B-Q4_K_M"), None);
        set_for_test("anthropic", "claude-opus-5-5", None, false);
        set_for_test("openai", "gpt-5.4-mini", None, false);
    }

    #[test]
    fn a_question_goes_out_at_its_models_effort() {
        set_for_test("anthropic", "claude-sonnet-5-5", Some(Xhigh), false);
        let mut r = ChatRequest::with_options(
            "https://api.anthropic.com/v1",
            "k",
            "claude-sonnet-5-5",
            "anthropic",
            json!([{ "role": "user", "content": "x" }]),
            &crate::ai::llm::ChatOptions { temperature: 0.3, max_tokens: None, timeout: Duration::from_secs(20), disable_thinking: false },
            Map::new(),
        );
        assert!(apply(&mut r));
        assert!(r.native.is_some(), "Claude's effort: the Messages API");
        assert_eq!(r.extra["output_config"], json!({ "effort": "xhigh" }));
        assert_eq!(r.timeout, Duration::from_secs(300));
        // A model without effort: as it was.
        let mut plain = ChatRequest::with_options(
            "https://api.anthropic.com/v1",
            "k",
            "claude-haiku-4-5",
            "anthropic",
            json!([]),
            &crate::ai::llm::ChatOptions { temperature: 0.3, max_tokens: None, timeout: Duration::from_secs(20), disable_thinking: false },
            Map::new(),
        );
        assert!(!apply(&mut plain));
        assert!(plain.native.is_none() && plain.extra.is_empty());
        assert_eq!(plain.timeout, Duration::from_secs(20));
        set_for_test("anthropic", "claude-sonnet-5-5", None, false);
    }
}

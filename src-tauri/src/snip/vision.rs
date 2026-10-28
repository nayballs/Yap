//! Which Chat models can read images, and the one to switch to when the
//! selected one can't (Snip to Chat).
//!
//! **Where Yap learns it**, most certain first:
//! 1. a refusal this session: a model whose server turned an image down
//!    (OpenRouter's "No endpoints found that support image input",
//!    llama-server without a projector, a 400 naming `image_url`) is a "no"
//!    until Yap restarts ([`refused`]);
//! 2. the provider's own list (`model_catalog`): Anthropic's
//!    `capabilities.image_input`, Groq's and OpenRouter's input modalities;
//! 3. family rules where the list doesn't say: every Claude model; every
//!    `gemini-*` chat model; OpenAI's GPT-4o, 4.1, 5.x and 6 and o1/o3/
//!    o4-mini (not gpt-oss, o1-mini, o3-mini or 3.5); Groq's GPT-OSS no;
//!    the on-device model **yes when its projector is downloaded and "Read
//!    screenshots on this PC" is on** (phase 3, 2026-10-28:
//!    `local_llm::vision`; it loads when a screenshot goes to it), else no.
//!
//! Anything else (a self-hosted server, an unknown id) is **not known**:
//! Chat lets it send, and a refusal teaches it.
//!
//! **The switch** ([`suggest`]): the best model that reads images on a
//! provider already connected, the same provider first — with Groq's
//! GPT-OSS that's `qwen/qwen3.8-27b` on the same key — preferring Yap's
//! curated picks when the provider's live list has them.

use std::collections::HashSet;
use std::sync::Mutex;

use serde::Serialize;

/// Models whose server refused an image this session: (provider, model).
static REFUSED: Mutex<Option<HashSet<(String, String)>>> = Mutex::new(None);

/// A server turned an image down: that model is a "no" until restart.
pub fn refused(provider: &str, model: &str) {
    let mut set = REFUSED.lock().unwrap_or_else(|p| p.into_inner());
    set.get_or_insert_with(HashSet::new).insert((provider.to_string(), model.to_string()));
}

fn was_refused(provider: &str, model: &str) -> bool {
    REFUSED
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .is_some_and(|s| s.contains(&(provider.to_string(), model.to_string())))
}

/// The family rule for a model id, where the provider's list doesn't say.
/// `None`: not known. Pure.
pub fn family_rule(provider: &str, model: &str) -> Option<bool> {
    let id = model.to_ascii_lowercase();
    // A routed id ("openai/gpt-5-mini") is judged by its model's name.
    let name = id.rsplit('/').next().unwrap_or(&id);
    match provider {
        "ondevice" => Some(false),
        "anthropic" => id.starts_with("claude").then_some(true),
        "gemini" => {
            if name.starts_with("gemini-") {
                Some(!name.contains("embedding") && !name.contains("tts"))
            } else {
                None
            }
        }
        // Routers and servers: by the model's family.
        _ if name.starts_with("claude") => Some(true),
        _ if name.starts_with("gemini-") => Some(true),
        _ => openai_family(name).or_else(|| vision_marked(name)),
    }
}

/// An id that says it reads images: a vision family's name ("llava",
/// "pixtral"), "vision" or "vl" as a word ("qwen2.5-vl-7b",
/// "llama-3.2-11b-vision"), or Groq's vision Qwen (`qwen3.8`). Pure.
fn vision_marked(name: &str) -> Option<bool> {
    let words: Vec<&str> = name.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let marked = words.iter().any(|w| matches!(*w, "vision" | "vl" | "llava" | "pixtral" | "bakllava" | "moondream"))
        || name.starts_with("qwen3.8")
        || name.starts_with("llava");
    marked.then_some(true)
}

/// OpenAI's model families (also behind Groq and OpenRouter ids). Pure.
fn openai_family(name: &str) -> Option<bool> {
    if name.starts_with("gpt-oss") || name.starts_with("gpt-3.5") || name.starts_with("o1-mini") || name.starts_with("o3-mini") {
        return Some(false);
    }
    let reads = ["gpt-4o", "gpt-4.1", "gpt-5", "gpt-6", "chatgpt-4o", "o4-mini"].iter().any(|p| name.starts_with(p))
        || name == "o1"
        || name.starts_with("o1-2")
        || name == "o3"
        || name.starts_with("o3-2");
    reads.then_some(true)
}

/// Whether `model` on `provider` reads images: `Some(true)`, `Some(false)`,
/// or `None` when nothing says (Chat then lets it try).
pub fn reads_images(provider: &str, model: &str) -> Option<bool> {
    if was_refused(provider, model) {
        return Some(false);
    }
    if provider == "ondevice" {
        return Some(crate::ai::local_llm::vision::reads_images(&crate::config::load(), model));
    }
    crate::ai::model_catalog::vision_for(provider, model).or_else(|| family_rule(provider, model))
}

/// What the on-device model needs to read a screenshot when it can't yet:
/// its projector downloaded ("Read screenshots on this PC", the size) or
/// switched back on — Chat's line offers it beside "Send its text
/// instead". `None` for any other model, one without a projector Yap
/// offers, or one that already reads images.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalOffer {
    /// The curated model it's for.
    pub model: String,
    pub size_mb: u64,
    /// Downloaded but switched off (else: to download).
    pub installed: bool,
    /// It failed to load this session.
    pub failed: bool,
}

pub fn local_offer(provider: &str, model: &str) -> Option<LocalOffer> {
    if provider != "ondevice" || was_refused(provider, model) {
        return None;
    }
    let cfg = crate::config::load();
    if crate::ai::local_llm::vision::reads_images(&cfg, model) {
        return None;
    }
    let p = crate::ai::local_llm::vision::for_model_file(&format!("{model}.gguf"))?;
    Some(LocalOffer {
        model: p.model.to_string(),
        size_mb: p.size_mb(),
        installed: crate::ai::local_llm::vision::installed(p),
        failed: crate::ai::local_llm::vision::failed(),
    })
}

/// Yap's picks of a model that reads images, per provider, best first
/// (Settings' curated lists, `src/lib/core/ppModels.js`).
pub const PICKS: &[(&str, &[&str])] = &[
    ("groq", &["qwen/qwen3.8-27b", "meta-models/muse-spark-open"]),
    ("anthropic", &["claude-haiku-5-5", "claude-sonnet-5-5", "claude-haiku-4-5"]),
    ("gemini", &["gemini-3.6-flash", "gemini-3.8-flash", "gemini-3.5-flash-lite"]),
    ("openai", &["gpt-5.6-luna", "gpt-5-mini", "gpt-5.5"]),
    ("openrouter", &["openai/gpt-5-mini", "anthropic/claude-haiku-4.5"]),
];

/// A model to switch to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Switch {
    pub provider: String,
    pub model: String,
    /// The provider's name for it, when its list has one ("" otherwise: the
    /// page names it from its registry).
    pub name: String,
}

/// The model to offer for `provider`: Yap's first pick its list has (or,
/// with no list yet, its first pick), else the first model its list says
/// reads images. Pure but for the catalogue.
fn pick_for(provider: &str) -> Option<Switch> {
    let listed = crate::ai::model_catalog::models_of(provider);
    let picks = PICKS.iter().find(|(p, _)| *p == provider).map(|(_, ids)| *ids).unwrap_or(&[]);
    match &listed {
        Some(models) => {
            for id in picks {
                if let Some(m) = models.iter().find(|m| m.id == *id || m.aliases.iter().any(|a| a == id)) {
                    if m.vision != Some(false) && !was_refused(provider, &m.id) {
                        return Some(Switch { provider: provider.into(), model: m.id.clone(), name: m.name.clone() });
                    }
                }
            }
            models
                .iter()
                .filter(|m| !m.deprecated && !was_refused(provider, &m.id))
                .find(|m| m.vision.or_else(|| family_rule(provider, &m.id)) == Some(true))
                .map(|m| Switch { provider: provider.into(), model: m.id.clone(), name: m.name.clone() })
        }
        None => picks
            .iter()
            .find(|id| !was_refused(provider, id))
            .map(|id| Switch { provider: provider.into(), model: id.to_string(), name: String::new() }),
    }
}

/// The switch to offer when the current Chat model (`provider`) can't read
/// images: the same provider first (same key, no setup), then every other
/// provider with a key (`keyed`, Settings' order). `None`: none of them has
/// one — Chat says which providers do.
pub fn suggest(provider: &str, keyed: &[&str]) -> Option<Switch> {
    let mut order: Vec<&str> = Vec::new();
    if keyed.contains(&provider) {
        order.push(provider);
    }
    order.extend(keyed.iter().filter(|p| **p != provider));
    order.into_iter().find_map(pick_for)
}

/// Whether a provider's error is its refusal of the image (the model can't
/// read images). Pure.
pub fn is_image_refusal(error: &str) -> bool {
    let e = error.to_ascii_lowercase();
    (e.contains("image") || e.contains("vision") || e.contains("multimodal") || e.contains("mmproj"))
        && (e.contains("support")
            || e.contains("not ")
            || e.contains("invalid")
            || e.contains("unknown")
            || e.contains("unexpected")
            || e.contains("can't")
            || e.contains("cannot")
            || e.contains("only text")
            || e.contains("400"))
}

/// The error Chat shows for a refused image (the page puts the switch line
/// in its place: it starts with these words).
pub const COULDNT_READ: &str = "The model couldn't read the screenshot";
/// The error for a question with screenshots to a model known not to read
/// images (the page checks first, so it's a guard).
pub const CANT_READ: &str = "This model can't read images";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::model_catalog::{set_models_for_test, CatalogModel};

    fn m(id: &str, vision: Option<bool>) -> CatalogModel {
        CatalogModel { id: id.into(), name: id.into(), vision, ..Default::default() }
    }

    #[test]
    fn the_family_rules_cover_what_lists_dont_say() {
        assert_eq!(family_rule("anthropic", "claude-haiku-4-5"), Some(true));
        assert_eq!(family_rule("gemini", "gemini-3.6-flash"), Some(true));
        assert_eq!(family_rule("gemini", "gemma-4-31b-it"), None);
        assert_eq!(family_rule("openai", "gpt-5.6-luna"), Some(true));
        assert_eq!(family_rule("openai", "gpt-4o-mini"), Some(true));
        assert_eq!(family_rule("openai", "o4-mini"), Some(true));
        assert_eq!(family_rule("openai", "o3"), Some(true));
        assert_eq!(family_rule("openai", "o3-mini"), Some(false));
        assert_eq!(family_rule("openai", "gpt-3.5-turbo"), Some(false));
        assert_eq!(family_rule("groq", "openai/gpt-oss-20b"), Some(false));
        assert_eq!(family_rule("openrouter", "openai/gpt-5-mini"), Some(true));
        assert_eq!(family_rule("ondevice", "qwen3.5-4b"), Some(false));
        assert_eq!(family_rule("custom", "mistral:7b"), None);
        assert_eq!(family_rule("groq", "llama-3.3-70b-versatile"), None);
        // Ids that say so.
        assert_eq!(family_rule("custom", "llava:7b"), Some(true));
        assert_eq!(family_rule("local", "qwen2.5-vl-7b-instruct"), Some(true));
        assert_eq!(family_rule("openrouter", "meta-llama/llama-3.2-11b-vision-instruct"), Some(true));
        assert_eq!(family_rule("groq", "qwen/qwen3.8-27b"), Some(true));
        assert_eq!(family_rule("custom", "evolve-7b"), None, "\"vl\" only as a word");
    }

    #[test]
    fn the_providers_list_says_first_and_a_refusal_overrides_it() {
        set_models_for_test("groq", vec![m("openai/gpt-oss-20b", Some(false)), m("qwen/qwen3.8-27b", Some(true))]);
        assert_eq!(reads_images("groq", "qwen/qwen3.8-27b"), Some(true));
        assert_eq!(reads_images("groq", "openai/gpt-oss-20b"), Some(false));
        // Not in the list, no rule: not known.
        assert_eq!(reads_images("groq", "some-new-model"), None);
        assert_eq!(reads_images("ondevice", "anything"), Some(false));
        // A server said no this session.
        refused("custom", "llava-but-no-projector");
        assert_eq!(reads_images("custom", "llava-but-no-projector"), Some(false));
    }

    #[test]
    fn the_switch_is_the_same_provider_first() {
        set_models_for_test("groq", vec![m("openai/gpt-oss-20b", Some(false)), m("qwen/qwen3.8-27b", Some(true))]);
        let s = suggest("groq", &["groq", "anthropic"]).unwrap();
        assert_eq!((s.provider.as_str(), s.model.as_str()), ("groq", "qwen/qwen3.8-27b"));
        // A provider whose list has no picks but a model that reads images.
        set_models_for_test("openrouter", vec![m("meta/text-only", Some(false)), m("vendor/sees-7b", Some(true))]);
        assert_eq!(suggest("openrouter", &["openrouter"]).unwrap().model, "vendor/sees-7b");
        // Groq without a vision model now: the next provider with a key.
        set_models_for_test("groq", vec![m("openai/gpt-oss-20b", Some(false))]);
        let s = suggest("groq", &["groq", "anthropic"]).unwrap();
        assert_eq!((s.provider.as_str(), s.model.as_str()), ("anthropic", "claude-haiku-5-5"));
        // The on-device model (no key): a connected cloud provider.
        assert_eq!(suggest("ondevice", &["gemini"]).unwrap().model, "gemini-3.6-flash");
        // Nothing connected that reads images.
        assert_eq!(suggest("ondevice", &[]), None);
        set_models_for_test("openai", vec![m("gpt-oss-120b", None)]);
        assert_eq!(suggest("ondevice", &["openai"]), None);
    }

    #[test]
    fn an_image_refusal_is_told_apart() {
        assert!(is_image_refusal("HTTP 404: No endpoints found that support image input"));
        assert!(is_image_refusal("HTTP 400: image input is not supported for this model"));
        assert!(is_image_refusal("HTTP 500: image input is not supported - hint: if this is unexpected, you may need to provide the mmproj"));
        assert!(is_image_refusal("HTTP 400: Invalid content type image_url"));
        assert!(!is_image_refusal("HTTP 429: rate limited"));
        assert!(!is_image_refusal("request failed: timed out"));
        assert!(!is_image_refusal("HTTP 400: tool_use_failed"));
    }
}

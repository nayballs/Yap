//! **Chat's model, from the phone** (Yap Link): `ready` names the model the
//! Voice Assistant answers with (Chat's, `ai_model::target`) and the others
//! Chat can use now — the on-device model when it's installed, each cloud
//! provider with a key (its chat models, as Yap last listed them,
//! `model_catalog.rs`), the self-hosted and custom servers Settings knows —
//! and `set_model` switches Chat to one of them exactly as the Chat view's
//! picker does (`ai_model::chat_set_model`), so the PC and the phone agree.
//! Only a model `ready` listed can be picked.
//!
//! A model's id on the wire is `provider|model`, a server's
//! `provider|model|address`.

use crate::ai::ai_model::{self, OnDevice, Scope};
use crate::ai::local_llm::PROVIDER_ONDEVICE;
use crate::config::YapConfig;
use crate::link::wire::Model;

/// A cloud provider's models listed at most.
const PER_PROVIDER: usize = 12;

/// A wire id. Pure.
pub fn id_of(provider: &str, model: &str, base_url: Option<&str>) -> String {
    match base_url {
        Some(b) => format!("{provider}|{model}|{b}"),
        None => format!("{provider}|{model}"),
    }
}

/// A wire id's parts: provider, model, address (servers only). Pure.
pub fn parse_id(id: &str) -> Option<(String, String, Option<String>)> {
    let mut parts = id.splitn(3, '|');
    let provider = parts.next()?.trim();
    let model = parts.next()?.trim();
    let base = parts.next().map(|b| b.trim().to_string()).filter(|b| !b.is_empty());
    (!provider.is_empty()).then(|| (provider.to_string(), model.to_string(), base))
}

fn label(name: &str, place: &str) -> String {
    format!("{name} · {place}")
}

/// Chat's model now, and every model it can use (the current one first).
pub fn listed(cfg: &YapConfig) -> (Option<Model>, Vec<Model>) {
    listed_with(cfg, &OnDevice::here(cfg))
}

/// [`listed`], with the on-device model as it is.
fn listed_with(cfg: &YapConfig, on_device: &OnDevice) -> (Option<Model>, Vec<Model>) {
    let status = ai_model::status(cfg, Scope::Chat, on_device);
    let mut models: Vec<Model> = Vec::new();
    let mut push = |m: Model| {
        if !models.iter().any(|x| x.id == m.id) {
            models.push(m);
        }
    };
    // The one in use.
    let current = status.model.as_ref().map(|m| {
        if m.provider == PROVIDER_ONDEVICE {
            Model { id: id_of(PROVIDER_ONDEVICE, "", None), label: label(&on_device.name, "On this PC"), on_this_pc: true }
        } else if matches!(m.provider.as_str(), "local" | "custom") {
            let base = cfg.llm_scopes.get("chat").map(|s| s.base_url.clone()).unwrap_or_default();
            Model { id: id_of(&m.provider, &m.id, Some(&base)), label: label(&m.id, &status.host), on_this_pc: m.on_this_pc }
        } else {
            let name = crate::ai::model_catalog::listed_model(&m.provider, &m.id).map(|c| c.name).filter(|n| !n.trim().is_empty());
            Model {
                id: id_of(&m.provider, &m.id, None),
                label: label(name.as_deref().unwrap_or(&m.id), crate::privacy::net::provider_name(&m.provider)),
                on_this_pc: m.on_this_pc,
            }
        }
    });
    if let Some(c) = &current {
        push(c.clone());
    }
    if on_device.installed {
        push(Model { id: id_of(PROVIDER_ONDEVICE, "", None), label: label(&on_device.name, "On this PC"), on_this_pc: true });
    }
    for provider in ai_model::keyed_providers(cfg) {
        let Some(list) = crate::ai::model_catalog::models_of(provider) else { continue };
        for m in list.into_iter().filter(|m| !m.deprecated && crate::ai::model_catalog::chat_capable(&m.id)).take(PER_PROVIDER) {
            let name = if m.name.trim().is_empty() { m.id.clone() } else { m.name.clone() };
            push(Model { id: id_of(provider, &m.id, None), label: label(&name, crate::privacy::net::provider_name(provider)), on_this_pc: false });
        }
    }
    for s in ai_model::servers(cfg) {
        if s.model.is_empty() {
            continue;
        }
        push(Model { id: id_of(&s.provider, &s.model, Some(&s.base_url)), label: label(&s.model, &s.host), on_this_pc: s.on_this_pc });
    }
    (current, models)
}

/// **Switch Chat to `id`** (one [`listed`] has), as the picker does: saved,
/// and every chip and Settings follow (`yap-chat-model-changed`). `Err`: in
/// plain words.
pub fn set(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    let cfg = crate::config::load();
    let (_, models) = listed(&cfg);
    if !models.iter().any(|m| m.id == id) {
        return Err("That model isn't one Yap can use now. Pick another.".into());
    }
    let (provider, model, base) = parse_id(id).ok_or("That model isn't one Yap can use now. Pick another.")?;
    let model = if provider == PROVIDER_ONDEVICE { OnDevice::here(&cfg).id } else { model };
    ai_model::chat_set_model(app.clone(), provider, model, base, None).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_models_wire_id_names_its_provider_model_and_server() {
        assert_eq!(id_of("groq", "openai/gpt-oss-20b", None), "groq|openai/gpt-oss-20b");
        assert_eq!(parse_id("groq|openai/gpt-oss-20b"), Some(("groq".into(), "openai/gpt-oss-20b".into(), None)));
        let server = id_of("local", "llama3", Some("http://localhost:11434/v1"));
        assert_eq!(parse_id(&server), Some(("local".into(), "llama3".into(), Some("http://localhost:11434/v1".into()))));
        assert_eq!(parse_id("ondevice|"), Some(("ondevice".into(), String::new(), None)));
        assert_eq!(parse_id(""), None);
        assert_eq!(parse_id("groq"), None);
    }

    #[test]
    fn the_phone_is_offered_the_model_in_use_and_the_keyed_providers() {
        let mut cfg = YapConfig::default();
        cfg.pp_api_keys.insert("groq".into(), "gsk-test".into());
        cfg.llm_scopes.insert(
            "chat".into(),
            crate::config::LlmScope {
                enabled: true,
                provider: "groq".into(),
                base_url: "https://api.groq.com/openai/v1".into(),
                model: "openai/gpt-oss-20b".into(),
                api_key: "gsk-test".into(),
                ..Default::default()
            },
        );
        let none = OnDevice { installed: false, name: String::new(), id: String::new(), context: 0 };
        let (current, models) = listed_with(&cfg, &none);
        let current = current.expect("Chat's model");
        assert_eq!(current.id, "groq|openai/gpt-oss-20b");
        assert!(current.label.ends_with("· Groq"), "{}", current.label);
        assert!(!current.on_this_pc);
        assert_eq!(models.first(), Some(&current), "the one in use first");
        // No model at all: none, and nothing to pick.
        let (nothing, list) = listed_with(&YapConfig::default(), &none);
        assert!(nothing.is_none() || list.iter().any(|m| Some(&m.id) == nothing.as_ref().map(|n| &n.id)));
        // The on-device model, installed: offered, on this PC.
        let qwen = OnDevice { installed: true, name: "Qwen3.5 4B".into(), id: "Qwen_Qwen3.5-4B-Q4_K_M".into(), context: 32_768 };
        let (_, list) = listed_with(&cfg, &qwen);
        assert!(list.iter().any(|m| m.id == "ondevice|" && m.on_this_pc && m.label == "Qwen3.5 4B · On this PC"), "{list:?}");
    }
}

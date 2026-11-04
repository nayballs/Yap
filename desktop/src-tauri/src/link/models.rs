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

/// A model's name and where it runs: "GPT OSS 20B · Groq".
fn label(name: &str, place: &str) -> String {
    format!("{name} · {place}")
}

/// The on-device model, by its name alone: the phone's own "On this PC" tag
/// (from `onThisPc`) says where, so the label doesn't say it twice.
fn on_device_model(on_device: &OnDevice) -> Model {
    Model { id: id_of(PROVIDER_ONDEVICE, "", None), label: on_device.name.clone(), on_this_pc: true }
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
            on_device_model(on_device)
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
        push(on_device_model(on_device));
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

/// **What Chat's picker shows** (2026-11-03), for the phone's own copy of
/// it (`phone/web/page/picker.js` over `desktop/src/lib/core/chatPicker.js`, the very
/// code the desktop picker builds its rows with): the same data that
/// picker reads, from the same Rust —
/// - `status`: `ai_model::status` for Chat (the model in use, the providers
///   with a key, the servers Settings knows, the effort), never a key, not
///   even masked;
/// - `catalog`: each keyed provider's live list (`model_catalog::snapshot`);
/// - `local`: the on-device models (`local_llm_status`'s, the parts the
///   picker reads; no download URLs);
/// - `images`: per provider, the listed models that read images
///   (`snip::vision::reads_images`, what Chat sends a screenshot by).
///
/// Pure but for the stores it reads.
pub fn picker(cfg: &YapConfig) -> serde_json::Value {
    let on_device = OnDevice::here(cfg);
    let status = ai_model::status(cfg, Scope::Chat, &on_device);
    let catalog = crate::ai::model_catalog::snapshot(cfg).providers;
    let local = crate::ai::commands::local_llm_status();
    picker_from(&status, &catalog, &local, &|p, m| crate::snip::vision::reads_images(p, m))
}

/// The local status's fields the picker reads (`chatPicker.js` `localRows`,
/// `whereLocal`, `slowWarning`).
const LOCAL_FIELDS: &[&str] = &["installed", "running", "activeFile", "modelFile", "own", "sharedOnDevice", "inUseNote", "gpu", "gpuCapable", "placement", "slowOnThisPc"];
/// A curated on-device model's fields it reads.
const CURATED_FIELDS: &[&str] = &["id", "display", "blurb", "filename", "sizeMb", "recommended", "isDefault", "tools", "family", "installed", "maxContext", "readsImages"];

/// [`picker`], from what it reads. Pure.
fn picker_from(
    status: &ai_model::Status,
    catalog: &std::collections::BTreeMap<String, crate::ai::model_catalog::ListView>,
    local: &serde_json::Value,
    reads_images: &dyn Fn(&str, &str) -> Option<bool>,
) -> serde_json::Value {
    let mut status = serde_json::to_value(status).unwrap_or_default();
    if let Some(s) = status.as_object_mut() {
        // Keys stay on the PC, masked or not; "Read screenshots on this PC"
        // (a download) is the PC's. The model to switch to for pictures
        // (`imagesSwitch`: a provider and a model) stays: the phone's
        // composer offers it beside "Send its text instead", as Chat's does.
        for k in ["maskedKeys", "imagesLocal"] {
            s.remove(k);
        }
    }
    let mut images = serde_json::Map::new();
    for (p, list) in catalog {
        let ids: Vec<&str> = list.models.iter().map(|m| m.id.as_str()).filter(|id| reads_images(p, id) == Some(true)).collect();
        images.insert(p.clone(), serde_json::json!(ids));
    }
    if let (Some(p), Some(id)) = (status.pointer("/model/provider").and_then(|v| v.as_str()), status.pointer("/model/id").and_then(|v| v.as_str())) {
        if reads_images(p, id) == Some(true) {
            let list = images.entry(p.to_string()).or_insert_with(|| serde_json::json!([]));
            if let Some(a) = list.as_array_mut() {
                if !a.iter().any(|x| x == id) {
                    a.push(serde_json::json!(id));
                }
            }
        }
    }
    let mut kept = serde_json::Map::new();
    for f in LOCAL_FIELDS {
        if let Some(v) = local.get(*f) {
            kept.insert((*f).to_string(), v.clone());
        }
    }
    let curated: Vec<serde_json::Value> = local
        .get("curated")
        .and_then(|c| c.as_array())
        .map(|list| {
            list.iter()
                .map(|m| serde_json::Value::Object(CURATED_FIELDS.iter().filter_map(|f| m.get(*f).map(|v| ((*f).to_string(), v.clone()))).collect()))
                .collect()
        })
        .unwrap_or_default();
    kept.insert("curated".into(), serde_json::json!(curated));
    serde_json::json!({ "status": status, "catalog": catalog, "local": kept, "images": images })
}

/// The longest model id the phone may name.
const MAX_ID: usize = 300;

/// **Switch Chat to a row of the picker**, exactly as the desktop picker
/// does (`ai_model::chat_set_model`, its own checks: a provider with a key,
/// a server Settings has the address of, an on-device model on this PC).
/// `Err`: in plain words.
pub fn set_row(app: &tauri::AppHandle, provider: &str, model: &str, base_url: Option<&str>, file: Option<&str>) -> Result<(), String> {
    let sane = |s: &str| s.len() <= MAX_ID && !s.chars().any(char::is_control);
    if provider.is_empty() || !sane(provider) || !sane(model) || !base_url.is_none_or(sane) || !file.is_none_or(sane) {
        return Err("That model isn't one Yap can use now. Pick another.".into());
    }
    ai_model::chat_set_model(app.clone(), provider.to_string(), model.to_string(), base_url.map(str::to_string), file.map(str::to_string)).map(|_| ())
}

/// **The effort Chat's model answers at** (`None`: its default), as the
/// picker sets it (`effort::chat_set_effort`), for the model in use.
pub fn set_effort(app: &tauri::AppHandle, level: Option<&str>) -> Result<(), String> {
    let cfg = crate::config::load();
    let status = ai_model::status(&cfg, Scope::Chat, &OnDevice::here(&cfg));
    let m = status.model.ok_or("Pick a model for Chat first.")?;
    crate::ai::effort::chat_set_effort(app.clone(), m.provider, m.id, level.map(str::to_string)).map(|_| ())
}

/// Fast mode (Claude Opus 5.5), as the picker sets it.
pub fn set_fast(app: &tauri::AppHandle, on: bool) -> Result<(), String> {
    crate::ai::effort::chat_set_fast(app.clone(), on).map(|_| ())
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
        // The on-device model, installed: offered, on this PC, by its name
        // alone (the phone's tag says "On this PC"), the cloud ones with
        // their provider.
        let qwen = OnDevice { installed: true, name: "Qwen3.5 4B".into(), id: "Qwen_Qwen3.5-4B-Q4_K_M".into(), context: 32_768 };
        let (current, list) = listed_with(&cfg, &qwen);
        assert!(list.iter().any(|m| m.id == "ondevice|" && m.on_this_pc && m.label == "Qwen3.5 4B"), "{list:?}");
        assert!(list.iter().all(|m| !m.label.contains("On this PC")), "{list:?}");
        assert!(current.unwrap().label.ends_with("· Groq"));
    }

    #[test]
    fn the_picker_gets_what_chats_picker_reads_and_never_a_key() {
        let mut cfg = YapConfig::default();
        cfg.pp_api_keys.insert("groq".into(), "gsk-secret-test-key".into());
        cfg.llm_scopes.insert(
            "chat".into(),
            crate::config::LlmScope {
                enabled: true,
                provider: "groq".into(),
                base_url: "https://api.groq.com/openai/v1".into(),
                model: "openai/gpt-oss-20b".into(),
                api_key: "gsk-secret-test-key".into(),
                ..Default::default()
            },
        );
        let none = OnDevice { installed: false, name: String::new(), id: String::new(), context: 0 };
        let status = ai_model::status(&cfg, Scope::Chat, &none);
        let model = |id: &str, name: &str| crate::ai::model_catalog::CatalogModel { id: id.into(), name: name.into(), ..Default::default() };
        let mut catalog = std::collections::BTreeMap::new();
        catalog.insert(
            "groq".to_string(),
            crate::ai::model_catalog::ListView {
                provider: "groq".into(),
                ok: true,
                fetched_at: 1,
                checked_at: 1,
                error: None,
                checking: false,
                models: vec![model("openai/gpt-oss-20b", "GPT-OSS 20B"), model("meta-llama/llama-4-scout", "Llama 4 Scout")],
                contexts: Vec::new(),
            },
        );
        let local = serde_json::json!({
            "installed": true,
            "activeFile": "Qwen3.5-4B.gguf",
            "engineState": {"build": "b1"},
            "curated": [{"id": "qwen", "display": "Qwen3.5 4B", "filename": "Qwen3.5-4B.gguf", "installed": true, "url": "https://example.com/q.gguf"}],
            "own": [],
        });
        let p = picker_from(&status, &catalog, &local, &|_, m| Some(m.contains("scout")));
        let text = p.to_string();
        assert!(!text.contains("gsk-secret"), "no key, not even masked: {text}");
        assert!(p["status"].get("maskedKeys").is_none());
        assert!(p["status"].get("imagesLocal").is_none(), "a download is the PC's");
        // Whether Chat's model reads pictures, for the phone's composer.
        assert!(p["status"].get("images").is_some() && p["status"].get("imagesSwitch").is_some());
        assert_eq!(p["status"]["model"]["id"], "openai/gpt-oss-20b");
        assert_eq!(p["status"]["keyed"], serde_json::json!(["groq"]));
        // The provider's live list, for the merge with Yap's curated one.
        assert_eq!(p["catalog"]["groq"]["models"][1]["name"], "Llama 4 Scout");
        // "Reads images", as Chat decides it.
        assert_eq!(p["images"]["groq"], serde_json::json!(["meta-llama/llama-4-scout"]));
        // The on-device models, without what the picker doesn't read.
        assert_eq!(p["local"]["curated"][0]["display"], "Qwen3.5 4B");
        assert!(p["local"]["curated"][0].get("url").is_none());
        assert!(p["local"].get("engineState").is_none());
        assert_eq!(p["local"]["activeFile"], "Qwen3.5-4B.gguf");
    }

    #[test]
    fn chat_on_the_on_device_model_is_named_once() {
        let mut cfg = YapConfig::default();
        cfg.llm_scopes.insert(
            "chat".into(),
            crate::config::LlmScope { enabled: true, provider: PROVIDER_ONDEVICE.into(), ..Default::default() },
        );
        let qwen = OnDevice { installed: true, name: "Qwen3.5 4B".into(), id: "Qwen_Qwen3.5-4B-Q4_K_M".into(), context: 32_768 };
        let (current, list) = listed_with(&cfg, &qwen);
        let current = current.expect("the on-device model in use");
        assert_eq!(current, Model { id: "ondevice|".into(), label: "Qwen3.5 4B".into(), on_this_pc: true });
        assert_eq!(list.iter().filter(|m| m.id == "ondevice|").count(), 1, "listed once: {list:?}");
    }
}

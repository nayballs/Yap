//! Which AI model a scope's next call goes to: the Chat view's model chip,
//! the quiet "GPT-OSS 20B · Groq" line under each answer (Chat, the Notes
//! view's Ask bar, the notepad's Ask sheet), and the chip's quick switch,
//! which changes the **Chat** scope only (never dictation cleanup).
//!
//! Resolution is the calls' own: the Chat scope through
//! `commands::resolve_chat_endpoint` (the scope when it's on, else the
//! global cleanup endpoint), meeting notes through
//! `meeting_summary::resolve_endpoint`. One addition for chat: a Chat scope
//! on Yap's on-device model starts the sidecar before the call
//! (`wake_on_device`, also in `chat_send` and `note_ask`) instead of falling
//! back to whatever endpoint is configured underneath, so "On this PC" means
//! on this PC.
//!
//! The page names models (`src/lib/aiModel.js`, from `ppModels.js`'s
//! registry); this side only says which provider and id.

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::chats::ChatModel;
use crate::config::YapConfig;
use crate::local_llm::{LOCAL_MODEL, PROVIDER_ONDEVICE};
use crate::AppState;

/// Cloud providers that need a key (and share one key across every scope).
pub const KEYED_PROVIDERS: [&str; 5] = ["groq", "anthropic", "openai", "gemini", "openrouter"];

const NOT_INSTALLED: &str =
    "Yap's on-device model isn't installed. Install it in Settings → Language Models → Dictation Cleanup → Local.";

/// The AI call sites the page shows a model for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The Chat view and the Notes view's typed questions (`chat_send`,
    /// `note_ask`).
    Chat,
    /// Meeting summaries and the meeting helpers (Note Formatting scope,
    /// else cleanup).
    Notes,
}

impl Scope {
    pub fn parse(s: Option<&str>) -> Self {
        match s {
            Some("notes") | Some("noteFormatting") => Scope::Notes,
            _ => Scope::Chat,
        }
    }
    fn key(self) -> &'static str {
        match self {
            Scope::Chat => "chat",
            Scope::Notes => "noteFormatting",
        }
    }
}

/// The OpenAI-compatible base URL of a cloud provider (Settings'
/// `BASE_URLS`, ScopeProviderConfig.svelte).
pub fn standard_base_url(provider: &str) -> Option<&'static str> {
    Some(match provider {
        "groq" => "https://api.groq.com/openai/v1",
        "anthropic" => "https://api.anthropic.com/v1",
        "openai" => "https://api.openai.com/v1",
        "gemini" => "https://generativelanguage.googleapis.com/v1beta/openai/",
        "openrouter" => "https://openrouter.ai/api/v1",
        _ => return None,
    })
}

/// Yap's on-device model on this PC: installed or not, and its name.
pub struct OnDevice {
    pub installed: bool,
    pub name: String,
}

impl OnDevice {
    pub fn here(cfg: &YapConfig) -> Self {
        OnDevice {
            installed: crate::local_llm::is_installed(cfg),
            name: crate::local_llm::active_model_display(cfg),
        }
    }
}

/// Where a call goes.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub provider: String,
    pub model: String,
    pub base_url: String,
    /// The on-device model's name ("" for any other).
    pub name: String,
    pub on_this_pc: bool,
    /// It can call Yap's note tools (`tools::supports_tools`).
    pub tools: bool,
}

impl Target {
    pub fn chat_model(&self) -> ChatModel {
        ChatModel {
            provider: self.provider.clone(),
            id: self.model.clone(),
            name: self.name.clone(),
            on_this_pc: self.on_this_pc,
        }
    }
}

/// A server on this machine, by its URL.
fn is_loopback(base_url: &str) -> bool {
    url::Url::parse(base_url)
        .ok()
        .and_then(|u| {
            u.host_str().map(|h| {
                matches!(h, "localhost" | "127.0.0.1" | "[::1]" | "::1")
                    || h.ends_with(".localhost")
            })
        })
        .unwrap_or(false)
}

fn host_of(base_url: &str) -> String {
    url::Url::parse(base_url)
        .ok()
        .and_then(|u| {
            u.host_str().map(|h| match u.port() {
                Some(p) => format!("{h}:{p}"),
                None => h.to_string(),
            })
        })
        .unwrap_or_default()
}

/// The resolved `(provider, model, base_url)` of a call, as the page names
/// it. The one adjustment: "ondevice" whose sidecar is down resolves to the
/// endpoint configured underneath (`local_llm::effective_endpoint_for`), so
/// it's named by that endpoint, not "On this PC".
pub fn describe(provider: &str, model: &str, base_url: &str, on_device_name: &str) -> Target {
    let tools = crate::tools::supports_tools(provider, model);
    let mut provider = provider.to_string();
    if provider == PROVIDER_ONDEVICE && !base_url.is_empty() && !is_loopback(base_url) {
        provider = KEYED_PROVIDERS
            .iter()
            .find(|p| standard_base_url(p).is_some_and(|u| host_of(u) == host_of(base_url)))
            .map(|p| p.to_string())
            .unwrap_or_else(|| "custom".to_string());
    }
    let on_device = provider == PROVIDER_ONDEVICE;
    Target {
        name: if on_device {
            on_device_name.to_string()
        } else {
            String::new()
        },
        on_this_pc: on_device || is_loopback(base_url),
        provider,
        model: model.to_string(),
        base_url: base_url.to_string(),
        tools,
    }
}

/// The scope's own setting, when it's on (else the call inherits the global
/// cleanup endpoint).
fn own_scope(cfg: &YapConfig, scope: Scope) -> Option<&crate::config::LlmScope> {
    cfg.llm_scopes
        .get(scope.key())
        .filter(|s| s.enabled && !s.provider.is_empty())
}

/// The provider the scope is set to, before the sidecar fallback.
fn configured_provider(cfg: &YapConfig, scope: Scope) -> &str {
    own_scope(cfg, scope)
        .map(|s| s.provider.as_str())
        .unwrap_or(cfg.pp_provider.as_str())
}

/// Where the scope's next call goes, or why there's none.
pub fn target(cfg: &YapConfig, scope: Scope, on_device: &OnDevice) -> Result<Target, String> {
    let (provider, model, base_url) = match scope {
        Scope::Chat => {
            if configured_provider(cfg, scope) == PROVIDER_ONDEVICE {
                // `wake_on_device` starts it before the call.
                if !on_device.installed {
                    return Err(NOT_INSTALLED.to_string());
                }
                let url = crate::local_llm::base_url().unwrap_or_default();
                (PROVIDER_ONDEVICE.to_string(), LOCAL_MODEL.to_string(), url)
            } else {
                let (b, _key, m, p, _persona, _thinking) =
                    crate::commands::resolve_chat_endpoint(cfg)?;
                (p, m, b)
            }
        }
        Scope::Notes => {
            let ep = crate::meeting_summary::resolve_endpoint(cfg)?;
            (ep.provider, ep.model, ep.base_url)
        }
    };
    Ok(describe(&provider, &model, &base_url, &on_device.name))
}

/// Before a chat call: Yap's on-device model started, when the Chat scope
/// (or the cleanup endpoint it inherits) is on it.
pub async fn wake_on_device(cfg: &YapConfig) -> Result<(), String> {
    if configured_provider(cfg, Scope::Chat) != PROVIDER_ONDEVICE {
        return Ok(());
    }
    if !crate::local_llm::is_installed(cfg) {
        return Err(NOT_INSTALLED.to_string());
    }
    crate::local_llm::ensure_started()
        .await
        .map(|_| ())
        .map_err(|e| format!("Yap's on-device model didn't start: {e}"))
}

/// Cloud providers with a key, for the chip's switch.
pub fn keyed_providers(cfg: &YapConfig) -> Vec<&'static str> {
    let chat = cfg.llm_scopes.get("chat");
    KEYED_PROVIDERS
        .iter()
        .copied()
        .filter(|p| {
            let own = chat
                .map(|s| {
                    if s.provider == *p {
                        s.api_key.clone()
                    } else {
                        s.api_keys.get(*p).cloned().unwrap_or_default()
                    }
                })
                .unwrap_or_default();
            !cfg.provider_api_key(p, &own).is_empty()
        })
        .collect()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnDeviceChoice {
    pub name: String,
    pub tools: bool,
}

/// What the page shows: the model (when there is one), why not, and what
/// the chip's switch can offer.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub scope: &'static str,
    pub ready: bool,
    pub error: Option<String>,
    pub model: Option<ChatModel>,
    /// The endpoint's host ("api.groq.com", "localhost:11434").
    pub host: String,
    pub tools: bool,
    /// Set in the scope itself (else inherited from Dictation Cleanup).
    pub from_scope: bool,
    pub keyed: Vec<&'static str>,
    pub on_device: Option<OnDeviceChoice>,
}

pub fn status(cfg: &YapConfig, scope: Scope, on_device: &OnDevice) -> Status {
    let (ready, error, model, host, tools) = match target(cfg, scope, on_device) {
        Ok(t) => (
            true,
            None,
            Some(t.chat_model()),
            host_of(&t.base_url),
            t.tools,
        ),
        Err(e) => (false, Some(e), None, String::new(), false),
    };
    Status {
        scope: scope.key(),
        ready,
        error,
        model,
        host,
        tools,
        from_scope: own_scope(cfg, scope).is_some(),
        keyed: keyed_providers(cfg),
        on_device: on_device.installed.then(|| OnDeviceChoice {
            name: on_device.name.clone(),
            tools: crate::tools::supports_tools(PROVIDER_ONDEVICE, LOCAL_MODEL),
        }),
    }
}

/// Point the Chat scope at `provider` + `model` (switched on, so it no
/// longer inherits cleanup's). Only a provider with a key, or the installed
/// on-device model. Within the same provider the scope keeps its endpoint
/// and key (a proxy, its own key); a new provider gets its standard URL and
/// the key shared by every scope, as Settings' provider pills do.
pub fn set_chat_model(
    cfg: &mut YapConfig,
    provider: &str,
    model: &str,
    on_device: &OnDevice,
) -> Result<(), String> {
    let ondevice = provider == PROVIDER_ONDEVICE;
    if ondevice {
        if !on_device.installed {
            return Err(NOT_INSTALLED.to_string());
        }
    } else if !keyed_providers(cfg).contains(&provider) {
        return Err(format!(
            "No {provider} API key — add one in Settings → Language Models"
        ));
    } else if model.trim().is_empty() {
        return Err("Pick a model".to_string());
    }
    let cleanup_url = (cfg.pp_provider == provider && !cfg.pp_base_url.is_empty())
        .then(|| cfg.pp_base_url.clone());
    let scope_keys = cfg
        .llm_scopes
        .get("chat")
        .and_then(|s| s.api_keys.get(provider).cloned())
        .unwrap_or_default();
    let shared_key = cfg.provider_api_key(provider, &scope_keys);
    let s = cfg.llm_scopes.entry("chat".to_string()).or_default();
    let same = s.provider == provider;
    if !same && !s.api_key.is_empty() && !KEYED_PROVIDERS.contains(&s.provider.as_str()) {
        // A custom / self-hosted endpoint's own key, kept for coming back.
        s.api_keys.insert(s.provider.clone(), s.api_key.clone());
    }
    if ondevice {
        s.base_url.clear();
        s.api_key.clear();
    } else {
        if !same || s.base_url.is_empty() {
            s.base_url = cleanup_url
                .or_else(|| standard_base_url(provider).map(str::to_string))
                .unwrap_or_default();
        }
        if !same || s.api_key.is_empty() {
            s.api_key = shared_key;
        }
        s.model = model.trim().to_string();
    }
    s.provider = provider.to_string();
    s.enabled = true;
    Ok(())
}

/// The model `scope`'s next call goes to (the Chat view's chip, and the
/// model line the Ask bars keep with each answer).
#[tauri::command]
pub fn ai_model_status(scope: Option<String>) -> Status {
    let cfg = crate::config::load();
    status(&cfg, Scope::parse(scope.as_deref()), &OnDevice::here(&cfg))
}

/// The chip's switch: the Chat scope on another model, saved the way
/// Settings saves (`save_config`), then `yap-chat-model-changed {scope}` so
/// Settings' copy adopts it (its next auto-save keeps it) and every chip
/// re-reads. Picking the on-device model starts it.
#[tauri::command]
pub fn chat_set_model(
    app: AppHandle,
    state: State<'_, AppState>,
    provider: String,
    model: String,
) -> Result<Status, String> {
    let mut cfg = crate::config::load();
    let on_device = OnDevice::here(&cfg);
    set_chat_model(&mut cfg, &provider, &model, &on_device)?;
    let scope = cfg.llm_scopes.get("chat").cloned();
    crate::commands::save_config(app.clone(), state, cfg.clone())?;
    let _ = app.emit(
        "yap-chat-model-changed",
        serde_json::json!({ "scope": scope }),
    );
    if provider == PROVIDER_ONDEVICE && !crate::local_llm::is_running() {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = crate::local_llm::ensure_started().await {
                tracing::warn!("On-device model for chat didn't start: {}", e);
            }
            // Chips re-read; no scope, so Settings has nothing to adopt.
            let _ = app.emit(
                "yap-chat-model-changed",
                serde_json::json!({ "scope": null }),
            );
        });
    }
    Ok(status(&cfg, Scope::Chat, &on_device))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LlmScope;

    fn cfg() -> YapConfig {
        YapConfig {
            pp_provider: "groq".into(),
            pp_base_url: "https://api.groq.com/openai/v1".into(),
            pp_model: "openai/gpt-oss-20b".into(),
            pp_api_key: String::new(),
            pp_api_keys: Default::default(),
            llm_scopes: Default::default(),
            ..Default::default()
        }
    }
    const NONE: OnDevice = OnDevice {
        installed: false,
        name: String::new(),
    };
    fn installed() -> OnDevice {
        OnDevice {
            installed: true,
            name: "Qwen2.5 1.5B Instruct".into(),
        }
    }
    fn chat_scope(provider: &str, base_url: &str, model: &str, key: &str) -> LlmScope {
        LlmScope {
            enabled: true,
            provider: provider.into(),
            base_url: base_url.into(),
            model: model.into(),
            api_key: key.into(),
            ..Default::default()
        }
    }

    #[test]
    fn no_key_means_no_model() {
        let c = cfg();
        let s = status(&c, Scope::Chat, &NONE);
        assert!(!s.ready);
        assert!(s.error.unwrap().contains("No groq API key"));
        assert!(s.keyed.is_empty());
        assert!(s.on_device.is_none());
        assert!(!s.from_scope);
    }

    #[test]
    fn chat_inherits_cleanup_then_uses_its_own_scope() {
        let mut c = cfg();
        c.pp_api_key = "gsk".into();
        c.pp_api_keys.insert("groq".into(), "gsk".into());
        let t = target(&c, Scope::Chat, &NONE).unwrap();
        assert_eq!(
            (t.provider.as_str(), t.model.as_str()),
            ("groq", "openai/gpt-oss-20b")
        );
        assert!(t.tools && !t.on_this_pc);

        c.llm_scopes.insert(
            "chat".into(),
            chat_scope("openai", "https://api.openai.com/v1", "gpt-5-mini", "sk"),
        );
        let s = status(&c, Scope::Chat, &NONE);
        assert!(s.ready && s.from_scope);
        assert_eq!(s.model.unwrap().id, "gpt-5-mini");
        assert_eq!(s.host, "api.openai.com");
        assert_eq!(s.keyed, vec!["groq", "openai"]);
    }

    #[test]
    fn a_small_local_model_has_no_note_tools() {
        let mut c = cfg();
        c.llm_scopes.insert(
            "chat".into(),
            chat_scope(
                "local",
                "http://localhost:11434/v1",
                "qwen2.5-1.5b-instruct",
                "",
            ),
        );
        let s = status(&c, Scope::Chat, &NONE);
        assert!(s.ready && !s.tools);
        let m = s.model.unwrap();
        assert!(m.on_this_pc);
        assert_eq!(s.host, "localhost:11434");
        // ≥ 4B can.
        c.llm_scopes.get_mut("chat").unwrap().model = "llama-3.1-8b".into();
        assert!(target(&c, Scope::Chat, &NONE).unwrap().tools);
    }

    #[test]
    fn on_device_chat_is_on_this_pc_or_says_it_is_missing() {
        let mut c = cfg();
        c.llm_scopes
            .insert("chat".into(), chat_scope("ondevice", "", "", ""));
        assert!(target(&c, Scope::Chat, &NONE)
            .unwrap_err()
            .contains("isn't installed"));
        let t = target(&c, Scope::Chat, &installed()).unwrap();
        assert_eq!(t.provider, "ondevice");
        assert_eq!(t.name, "Qwen2.5 1.5B Instruct");
        assert!(t.on_this_pc && !t.tools);
    }

    #[test]
    fn a_down_sidecar_is_named_by_the_endpoint_underneath() {
        let t = describe(
            "ondevice",
            "openai/gpt-oss-20b",
            "https://api.groq.com/openai/v1",
            "Qwen",
        );
        assert_eq!(t.provider, "groq");
        assert!(!t.on_this_pc && t.name.is_empty());
        assert_eq!(
            describe("ondevice", "x", "https://example.com/v1", "").provider,
            "custom"
        );
    }

    #[test]
    fn notes_follow_note_formatting_else_cleanup() {
        let mut c = cfg();
        c.pp_api_key = "gsk".into();
        assert_eq!(
            target(&c, Scope::Notes, &NONE).unwrap().model,
            "openai/gpt-oss-20b"
        );
        let mut s = chat_scope("local", "http://127.0.0.1:1234/v1", "gemma-3-12b", "");
        s.prompt = "x".into();
        c.llm_scopes.insert("noteFormatting".into(), s);
        let t = target(&c, Scope::Notes, &NONE).unwrap();
        assert_eq!((t.provider.as_str(), t.on_this_pc), ("local", true));
        // The chat still inherits cleanup.
        assert_eq!(target(&c, Scope::Chat, &NONE).unwrap().provider, "groq");
    }

    #[test]
    fn switching_turns_the_chat_scope_on_and_leaves_cleanup_alone() {
        let mut c = cfg();
        c.pp_api_keys.insert("groq".into(), "gsk".into());
        c.pp_api_keys.insert("openai".into(), "sk".into());
        set_chat_model(&mut c, "openai", "gpt-5-mini", &NONE).unwrap();
        let s = &c.llm_scopes["chat"];
        assert!(s.enabled);
        assert_eq!(s.base_url, "https://api.openai.com/v1");
        assert_eq!(s.api_key, "sk");
        assert_eq!(s.model, "gpt-5-mini");
        // Cleanup untouched.
        assert_eq!(
            (c.pp_provider.as_str(), c.pp_model.as_str()),
            ("groq", "openai/gpt-oss-20b")
        );

        // Back to Groq: cleanup's endpoint for the same provider, the shared key.
        c.pp_base_url = "https://proxy.example/groq/v1".into();
        set_chat_model(&mut c, "groq", "openai/gpt-oss-120b", &NONE).unwrap();
        let s = &c.llm_scopes["chat"];
        assert_eq!(s.base_url, "https://proxy.example/groq/v1");
        assert_eq!(s.api_key, "gsk");
        let t = target(&c, Scope::Chat, &NONE).unwrap();
        assert_eq!(t.model, "openai/gpt-oss-120b");
    }

    #[test]
    fn switching_within_a_provider_keeps_its_endpoint_and_key() {
        let mut c = cfg();
        c.llm_scopes.insert(
            "chat".into(),
            chat_scope(
                "groq",
                "http://127.0.0.1:5555/v1",
                "openai/gpt-oss-20b",
                "own",
            ),
        );
        assert_eq!(keyed_providers(&c), vec!["groq"]);
        set_chat_model(&mut c, "groq", "openai/gpt-oss-120b", &NONE).unwrap();
        let s = &c.llm_scopes["chat"];
        assert_eq!(s.base_url, "http://127.0.0.1:5555/v1");
        assert_eq!(s.api_key, "own");
        assert_eq!(s.model, "openai/gpt-oss-120b");
    }

    #[test]
    fn switching_needs_a_key_or_the_installed_model() {
        let mut c = cfg();
        assert!(
            set_chat_model(&mut c, "anthropic", "claude-haiku-4-5", &NONE)
                .unwrap_err()
                .contains("No anthropic API key")
        );
        assert!(set_chat_model(&mut c, "ondevice", "", &NONE).is_err());
        assert!(c.llm_scopes.is_empty());

        // On-device: no URL or key of its own; a custom key is kept aside.
        c.llm_scopes.insert(
            "chat".into(),
            chat_scope("custom", "https://my.example/v1", "m", "ck"),
        );
        set_chat_model(&mut c, "ondevice", "", &installed()).unwrap();
        let s = &c.llm_scopes["chat"];
        assert_eq!(
            (s.provider.as_str(), s.base_url.as_str(), s.api_key.as_str()),
            ("ondevice", "", "")
        );
        assert_eq!(s.api_keys.get("custom").map(String::as_str), Some("ck"));
        assert_eq!(
            target(&c, Scope::Chat, &installed()).unwrap().provider,
            "ondevice"
        );
    }
}

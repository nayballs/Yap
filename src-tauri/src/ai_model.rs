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
//!
//! The chip's picker (ChatModelPicker.svelte, a provider rail) also
//! connects a provider in place ([`chat_provider_key`]: the key saved where
//! Settings saves it, then that provider's live list is checked at once),
//! picks one of Yap's on-device models by its file (`pp_local_model`: the
//! sidecar serves one GGUF for every feature, so it restarts on the new
//! one), and switches Chat to a self-hosted or custom server Settings
//! already knows ([`servers`]: never an address the page makes up). Keys
//! go back to the page masked only ([`mask_key`]).

use std::collections::BTreeMap;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::chat::chats::ChatModel;
use crate::config::YapConfig;
use crate::local_llm::PROVIDER_ONDEVICE;

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

/// Yap's on-device model on this PC: installed or not, its name, and the
/// id calls send (its GGUF's name, `local_llm::model_id`), which says
/// whether it gets Chat's tools.
pub struct OnDevice {
    pub installed: bool,
    pub name: String,
    pub id: String,
    /// The context it works with (tokens; `local_llm::context_now`).
    pub context: u32,
}

impl OnDevice {
    pub fn here(cfg: &YapConfig) -> Self {
        OnDevice {
            installed: crate::local_llm::is_installed(cfg),
            name: crate::local_llm::active_model_display(cfg),
            id: crate::local_llm::active_model_id(cfg),
            context: crate::local_llm::context_now(cfg),
        }
    }
}

/// Does a call to this endpoint run on this PC (Yap's own model, a
/// self-hosted server on a loopback address)? Pure.
pub fn runs_here(provider: &str, base_url: &str) -> bool {
    provider == PROVIDER_ONDEVICE || is_loopback(base_url)
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
    let tools = crate::chat::tools::supports_tools(provider, model);
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
                (PROVIDER_ONDEVICE.to_string(), on_device.id.clone(), url)
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

/// The key a Chat call to `provider` would use: the Chat scope's own, else
/// the one every scope shares (`YapConfig::provider_api_key`).
fn chat_key(cfg: &YapConfig, provider: &str) -> String {
    let own = cfg
        .llm_scopes
        .get("chat")
        .map(|s| {
            if s.provider == provider {
                s.api_key.clone()
            } else {
                s.api_keys.get(provider).cloned().unwrap_or_default()
            }
        })
        .unwrap_or_default();
    cfg.provider_api_key(provider, &own)
}

/// Cloud providers with a key, for the chip's switch.
pub fn keyed_providers(cfg: &YapConfig) -> Vec<&'static str> {
    KEYED_PROVIDERS
        .iter()
        .copied()
        .filter(|p| !chat_key(cfg, p).is_empty())
        .collect()
}

/// A key as the page may show it, as Settings masks one: "gsk…AbCd" (the
/// first 3 and last 4 characters), "••••••••" for a short one. Pure.
pub fn mask_key(key: &str) -> String {
    let k: Vec<char> = key.trim().chars().collect();
    if k.len() > 8 {
        format!(
            "{}…{}",
            k[..3].iter().collect::<String>(),
            k[k.len() - 4..].iter().collect::<String>()
        )
    } else {
        "••••••••".to_string()
    }
}

/// Save `key` for a cloud provider the way Settings does
/// (ScopeProviderConfig's `stashApiKey`): the key every scope shares
/// (`pp_api_keys`), the cleanup's own when cleanup is on that provider, and
/// the Chat scope's when Chat is (or remembers one for it). Pure.
pub fn set_provider_key(cfg: &mut YapConfig, provider: &str, key: &str) -> Result<(), String> {
    if !KEYED_PROVIDERS.contains(&provider) {
        return Err(format!("{provider} doesn't take an API key"));
    }
    let key = key.trim();
    if key.is_empty() {
        return Err("Paste the API key first".to_string());
    }
    if key.len() > 4096 || key.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("That doesn't look like an API key: paste the key alone".to_string());
    }
    cfg.pp_api_keys.insert(provider.to_string(), key.to_string());
    if cfg.pp_provider == provider {
        cfg.pp_api_key = key.to_string();
    }
    if let Some(s) = cfg.llm_scopes.get_mut("chat") {
        if s.provider == provider {
            s.api_key = key.to_string();
        }
        if let Some(k) = s.api_keys.get_mut(provider) {
            *k = key.to_string();
        }
    }
    Ok(())
}

/// A self-hosted or custom OpenAI-compatible server Settings has an address
/// for (the Chat scope, Dictation Cleanup, another scope, a cleanup
/// profile): the picker lists its models (`model_catalog::llm_models` with
/// `scope`'s key) and can point Chat at it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    /// "local" (self-hosted: Ollama, LM Studio) or "custom".
    pub provider: String,
    pub base_url: String,
    /// The model configured for it there.
    pub model: String,
    /// Whose key it's asked with: "chat", "cleanup", a scope's key, or
    /// "profile:<id>".
    pub scope: String,
    /// "localhost:11434".
    pub host: String,
    pub on_this_pc: bool,
}

fn same_url(a: &str, b: &str) -> bool {
    a.trim().trim_end_matches('/').eq_ignore_ascii_case(b.trim().trim_end_matches('/'))
}

/// Every server Settings has an address for, the Chat scope's first, one
/// per address. Pure.
pub fn servers(cfg: &YapConfig) -> Vec<Server> {
    let mut out: Vec<Server> = Vec::new();
    let mut add = |provider: &str, base: &str, model: &str, scope: String| {
        let base = base.trim();
        if !matches!(provider, "local" | "custom")
            || !(base.starts_with("http://") || base.starts_with("https://"))
            || out.iter().any(|s| same_url(&s.base_url, base))
        {
            return;
        }
        out.push(Server {
            provider: provider.to_string(),
            base_url: base.to_string(),
            model: model.trim().to_string(),
            scope,
            host: host_of(base),
            on_this_pc: is_loopback(base),
        });
    };
    if let Some(s) = cfg.llm_scopes.get("chat") {
        add(&s.provider, &s.base_url, &s.model, "chat".to_string());
    }
    add(&cfg.pp_provider, &cfg.pp_base_url, &cfg.pp_model, "cleanup".to_string());
    let mut keys: Vec<&String> = cfg.llm_scopes.keys().filter(|k| *k != "chat").collect();
    keys.sort();
    for k in keys {
        let s = &cfg.llm_scopes[k];
        add(&s.provider, &s.base_url, &s.model, k.clone());
    }
    for p in &cfg.cleanup_profiles {
        add(&p.provider, &p.base_url, &p.model, format!("profile:{}", p.id));
    }
    out
}

/// Point the on-device model at `file`, one of the GGUFs in `<data>/llm/`
/// (`on_disk`): `pp_local_model`, "" for the bundled default. Every feature
/// on the on-device model uses it (one sidecar). Pure.
pub fn set_local_model(cfg: &mut YapConfig, file: &str, on_disk: &[String]) -> Result<(), String> {
    let name = file.trim();
    if name.is_empty() || name.contains(['/', '\\']) || name.contains("..") || !on_disk.iter().any(|f| f == name) {
        return Err(format!("{name} isn't downloaded on this PC"));
    }
    cfg.pp_local_model = if name == crate::local_llm::MODEL_FILENAME {
        String::new()
    } else {
        name.to_string()
    };
    Ok(())
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
    /// The provider the scope is set to (also when it isn't ready: the
    /// picker opens on it).
    pub provider: String,
    pub keyed: Vec<&'static str>,
    /// Each keyed provider's key as the page may show it ([`mask_key`]).
    pub masked_keys: BTreeMap<&'static str, String>,
    /// Self-hosted and custom servers Settings has an address for.
    pub servers: Vec<Server>,
    pub on_device: Option<OnDeviceChoice>,
    /// The on-device model's context (tokens), when the call goes to it:
    /// the chip's tooltip says it.
    pub context: Option<u32>,
    /// Can the model read images (Snip to Chat, `snip::vision`): `null`
    /// when nothing says (Chat lets it try).
    pub images: Option<bool>,
    /// When it can't: the model to switch to, the same provider first.
    pub images_switch: Option<crate::snip::vision::Switch>,
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
    let context = model
        .as_ref()
        .filter(|m| m.provider == PROVIDER_ONDEVICE && on_device.context > 0)
        .map(|_| on_device.context);
    let images = model.as_ref().and_then(|m| crate::snip::vision::reads_images(&m.provider, &m.id));
    let images_switch = (images == Some(false))
        .then(|| crate::snip::vision::suggest(model.as_ref().map(|m| m.provider.as_str()).unwrap_or(""), &keyed_providers(cfg)))
        .flatten();
    Status {
        scope: scope.key(),
        ready,
        error,
        model,
        host,
        tools,
        from_scope: own_scope(cfg, scope).is_some(),
        provider: configured_provider(cfg, scope).to_string(),
        keyed: keyed_providers(cfg),
        masked_keys: keyed_providers(cfg)
            .into_iter()
            .map(|p| (p, mask_key(&chat_key(cfg, p))))
            .collect(),
        servers: servers(cfg),
        on_device: on_device.installed.then(|| OnDeviceChoice {
            name: on_device.name.clone(),
            tools: crate::chat::tools::supports_tools(PROVIDER_ONDEVICE, &on_device.id),
        }),
        context,
        images,
        images_switch,
    }
}

/// Point the Chat scope at `provider` + `model` (switched on, so it no
/// longer inherits cleanup's). Only a provider with a key, the installed
/// on-device model, or a server Settings has the address of (`base_url`,
/// one of [`servers`]: its key comes from where it's set up). Within the
/// same provider the scope keeps its endpoint and key (a proxy, its own
/// key); a new provider gets its standard URL and the key shared by every
/// scope, as Settings' provider pills do.
pub fn set_chat_model(
    cfg: &mut YapConfig,
    provider: &str,
    model: &str,
    base_url: Option<&str>,
    on_device: &OnDevice,
) -> Result<(), String> {
    let ondevice = provider == PROVIDER_ONDEVICE;
    let server = if matches!(provider, "local" | "custom") {
        let url = base_url.unwrap_or_default();
        let found = servers(cfg)
            .into_iter()
            .find(|s| s.provider == provider && same_url(&s.base_url, url))
            .ok_or_else(|| "That server isn't set up: add it in Settings → Language Models".to_string())?;
        Some(found)
    } else {
        None
    };
    if ondevice {
        if !on_device.installed {
            return Err(NOT_INSTALLED.to_string());
        }
    } else if server.is_none() && !keyed_providers(cfg).contains(&provider) {
        return Err(format!(
            "No {provider} API key — add one in Settings → Language Models"
        ));
    }
    if !ondevice && model.trim().is_empty() {
        return Err("Pick a model".to_string());
    }
    let server_key = server
        .as_ref()
        .map(|s| crate::model_catalog::endpoint_key(cfg, Some(&s.scope), provider));
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
    } else if let (Some(server), Some(key)) = (server, server_key) {
        s.base_url = server.base_url;
        s.api_key = key;
        s.model = model.trim().to_string();
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
///
/// `base_url`: a self-hosted / custom server's address (one of
/// [`servers`]). `file`: which of the on-device models (a GGUF in
/// `<data>/llm/`); it's the on-device model for every feature, so a sidecar
/// serving another one stops, and starts again on this one; the event then
/// carries `ppLocalModel` for Settings' copy too.
#[tauri::command]
pub fn chat_set_model(
    app: AppHandle,
    provider: String,
    model: String,
    base_url: Option<String>,
    file: Option<String>,
) -> Result<Status, String> {
    let local_file = if provider == PROVIDER_ONDEVICE { file } else { None };
    let on_disk = local_file.as_ref().map(|_| crate::local_llm::list_models());
    // In one step (never over a change saved meanwhile); refused: nothing saved.
    let cfg = crate::commands::change_config(&app, |cfg| {
        if let (Some(f), Some(on_disk)) = (&local_file, &on_disk) {
            set_local_model(cfg, f, on_disk)?;
        }
        let on_device = OnDevice::here(cfg);
        set_chat_model(cfg, &provider, &model, base_url.as_deref(), &on_device)
    })?;
    let scope = cfg.llm_scopes.get("chat").cloned();
    let mut changed = serde_json::json!({ "scope": scope });
    if local_file.is_some() {
        changed["ppLocalModel"] = serde_json::json!(cfg.pp_local_model);
        // The sidecar serving another GGUF: off, so it starts on this one.
        let wanted = crate::local_llm::model_id(&crate::local_llm::active_model_path(&cfg));
        if crate::local_llm::serving_model_id().is_some_and(|id| id != wanted) {
            crate::local_llm::stop();
        }
    }
    let _ = app.emit("yap-chat-model-changed", changed);
    let on_device = OnDevice::here(&cfg);
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

/// The picker's "Connect Claude": `key` saved for a cloud provider where
/// Settings saves it ([`set_provider_key`], through `save_config`, whose
/// `model_catalog::on_config_saved` checks that provider's live list at
/// once; `yap-model-catalog` brings it). The page tells Settings' copy
/// itself (`yap-provider-key-saved`, a window event), so the key isn't
/// broadcast to every window; the status that comes back has it masked.
#[tauri::command]
pub fn chat_provider_key(app: AppHandle, provider: String, key: String) -> Result<Status, String> {
    let cfg = crate::commands::change_config(&app, |cfg| set_provider_key(cfg, &provider, &key))?;
    Ok(status(&cfg, Scope::Chat, &OnDevice::here(&cfg)))
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
        id: String::new(),
        context: 0,
    };
    fn installed() -> OnDevice {
        OnDevice {
            installed: true,
            name: "Qwen2.5 1.5B Instruct".into(),
            id: "qwen2.5-1.5b-instruct-q4_k_m".into(),
            context: 8_192,
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
        assert_eq!(t.model, "qwen2.5-1.5b-instruct-q4_k_m");
        assert!(t.on_this_pc && !t.tools);
        // The bigger model picked: named by its GGUF, so it gets the tools
        // (it used to be sent as the 1.5B whatever ran).
        let big = OnDevice { installed: true, name: "Qwen3.5 4B".into(), id: "Qwen_Qwen3.5-4B-Q4_K_M".into(), context: 32_768 };
        let t = target(&c, Scope::Chat, &big).unwrap();
        assert_eq!(t.model, "Qwen_Qwen3.5-4B-Q4_K_M");
        assert!(t.tools);
        let st = status(&c, Scope::Chat, &big);
        assert!(st.on_device.unwrap().tools);
        // The chip's tooltip names its context.
        assert_eq!(st.context, Some(32_768));
        assert_eq!(status(&cfg(), Scope::Chat, &big).context, None, "a cloud model: none of Yap's to say");
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
        set_chat_model(&mut c, "openai", "gpt-5-mini", None, &NONE).unwrap();
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
        set_chat_model(&mut c, "groq", "openai/gpt-oss-120b", None, &NONE).unwrap();
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
        set_chat_model(&mut c, "groq", "openai/gpt-oss-120b", None, &NONE).unwrap();
        let s = &c.llm_scopes["chat"];
        assert_eq!(s.base_url, "http://127.0.0.1:5555/v1");
        assert_eq!(s.api_key, "own");
        assert_eq!(s.model, "openai/gpt-oss-120b");
    }

    #[test]
    fn switching_needs_a_key_or_the_installed_model() {
        let mut c = cfg();
        assert!(
            set_chat_model(&mut c, "anthropic", "claude-haiku-4-5", None, &NONE)
                .unwrap_err()
                .contains("No anthropic API key")
        );
        assert!(set_chat_model(&mut c, "ondevice", "", None, &NONE).is_err());
        assert!(c.llm_scopes.is_empty());

        // On-device: no URL or key of its own; a custom key is kept aside.
        c.llm_scopes.insert(
            "chat".into(),
            chat_scope("custom", "https://my.example/v1", "m", "ck"),
        );
        set_chat_model(&mut c, "ondevice", "", None, &installed()).unwrap();
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

    #[test]
    fn a_key_saved_from_the_picker_lands_where_settings_keeps_it() {
        let mut c = cfg();
        c.llm_scopes.insert(
            "chat".into(),
            chat_scope("anthropic", "https://api.anthropic.com/v1", "claude-haiku-4-5", ""),
        );
        assert!(!keyed_providers(&c).contains(&"anthropic"));
        set_provider_key(&mut c, "anthropic", "  sk-ant-api03-abcdef1234  ").unwrap();
        assert_eq!(c.pp_api_keys["anthropic"], "sk-ant-api03-abcdef1234");
        // Chat is on Anthropic: its own key too; cleanup (Groq) untouched.
        assert_eq!(c.llm_scopes["chat"].api_key, "sk-ant-api03-abcdef1234");
        assert_eq!(c.pp_api_key, "");
        assert!(keyed_providers(&c).contains(&"anthropic"));
        let s = status(&c, Scope::Chat, &NONE);
        assert_eq!(s.masked_keys["anthropic"], "sk-…1234");
        assert!(!serde_json::to_string(&s).unwrap().contains("abcdef"));
        // Cleanup's provider: its key follows.
        set_provider_key(&mut c, "groq", "gsk_new_key_9876").unwrap();
        assert_eq!(c.pp_api_key, "gsk_new_key_9876");
        // Not a cloud provider, nothing, or not a key.
        assert!(set_provider_key(&mut c, "ondevice", "x").is_err());
        assert!(set_provider_key(&mut c, "openai", "   ").is_err());
        assert!(set_provider_key(&mut c, "openai", "sk one two").is_err());
        assert!(!c.pp_api_keys.contains_key("openai"));
    }

    #[test]
    fn keys_are_masked_like_settings_masks_them() {
        assert_eq!(mask_key("gsk_abcdefghijklmnop"), "gsk…mnop");
        assert_eq!(mask_key("short"), "••••••••");
        assert_eq!(mask_key("123456789"), "123…6789");
    }

    #[test]
    fn servers_are_the_addresses_settings_knows_and_chat_can_use_them() {
        let mut c = cfg();
        c.pp_api_keys.insert("groq".into(), "gsk".into());
        c.pp_provider = "local".into();
        c.pp_base_url = "http://localhost:11434/v1".into();
        c.pp_model = "llama3.1".into();
        c.pp_api_key = "ollama-key".into();
        let mut nf = chat_scope("custom", "https://llm.example.com/v1/", "big-model", "ck");
        nf.enabled = false;
        c.llm_scopes.insert("noteFormatting".into(), nf);
        // The same address twice: once.
        c.llm_scopes.insert(
            "voiceAgent".into(),
            chat_scope("local", "http://localhost:11434/v1/", "x", ""),
        );
        let list = servers(&c);
        assert_eq!(list.len(), 2);
        assert_eq!(
            (list[0].provider.as_str(), list[0].scope.as_str(), list[0].host.as_str(), list[0].on_this_pc),
            ("local", "cleanup", "localhost:11434", true)
        );
        assert_eq!((list[1].scope.as_str(), list[1].on_this_pc), ("noteFormatting", false));

        // Chat on the self-hosted server: its address, the key it's set up with.
        set_chat_model(&mut c, "local", "qwen2.5:7b", Some("http://localhost:11434/v1/"), &NONE).unwrap();
        let s = &c.llm_scopes["chat"];
        assert_eq!(
            (s.provider.as_str(), s.base_url.as_str(), s.model.as_str(), s.api_key.as_str()),
            ("local", "http://localhost:11434/v1", "qwen2.5:7b", "ollama-key")
        );
        assert!(target(&c, Scope::Chat, &NONE).unwrap().on_this_pc);
        // Then the custom one, with its own key; the server's key is kept aside.
        set_chat_model(&mut c, "custom", "big-model", Some("https://llm.example.com/v1"), &NONE).unwrap();
        let s = &c.llm_scopes["chat"];
        assert_eq!((s.base_url.as_str(), s.api_key.as_str()), ("https://llm.example.com/v1/", "ck"));
        assert_eq!(s.api_keys.get("local").map(String::as_str), Some("ollama-key"));
        // Never an address Settings doesn't have.
        assert!(set_chat_model(&mut c, "custom", "m", Some("https://elsewhere.example/v1"), &NONE)
            .unwrap_err()
            .contains("isn't set up"));
        assert!(set_chat_model(&mut c, "local", "m", None, &NONE).is_err());
    }

    #[test]
    fn the_on_device_model_is_picked_by_its_file() {
        let on_disk = vec![
            crate::local_llm::MODEL_FILENAME.to_string(),
            "Qwen_Qwen3.5-4B-Q4_K_M.gguf".to_string(),
        ];
        let mut c = cfg();
        set_local_model(&mut c, "Qwen_Qwen3.5-4B-Q4_K_M.gguf", &on_disk).unwrap();
        assert_eq!(c.pp_local_model, "Qwen_Qwen3.5-4B-Q4_K_M.gguf");
        // The bundled default is "" (what an empty pick means).
        set_local_model(&mut c, crate::local_llm::MODEL_FILENAME, &on_disk).unwrap();
        assert_eq!(c.pp_local_model, "");
        // Only a file that's there, by its bare name.
        assert!(set_local_model(&mut c, "missing.gguf", &on_disk).is_err());
        assert!(set_local_model(&mut c, "..\\Qwen_Qwen3.5-4B-Q4_K_M.gguf", &on_disk).is_err());
        assert_eq!(c.pp_local_model, "");
    }

    #[test]
    fn the_status_says_which_provider_the_scope_is_set_to() {
        let s = status(&cfg(), Scope::Chat, &NONE);
        assert!(!s.ready);
        assert_eq!(s.provider, "groq");
        let mut c = cfg();
        c.llm_scopes
            .insert("chat".into(), chat_scope("ondevice", "", "", ""));
        assert_eq!(status(&c, Scope::Chat, &NONE).provider, "ondevice");
    }
}

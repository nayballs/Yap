//! **Live model lists**: which chat models each AI provider offers right
//! now, asked of the provider itself with the key the person already saved,
//! so a model Google or Groq releases tomorrow shows up in Yap's pickers
//! without a Yap update, and one they retire stops being offered.
//!
//! The page merges these lists with Yap's own curated registry
//! (`src/lib/core/ppModels.js`, the recommended order and wording) in one helper
//! (`src/lib/core/modelCatalog.js`); this side only fetches, filters and caches.
//!
//! **Where it asks** (a GET, read-only, short timeouts, off the main thread):
//! Groq `GET /openai/v1/models`, OpenAI `GET /v1/models` (both Bearer),
//! Anthropic `GET /v1/models` (`x-api-key` + `anthropic-version`, paged by
//! `after_id`), Gemini's native `GET /v1beta/models` (the key in the
//! `x-goog-api-key` header, never the URL; paged by `pageToken`), OpenRouter
//! `GET /api/v1/models` (Bearer), and a self-hosted / custom
//! OpenAI-compatible server's `GET {base}/models` (Ollama, LM Studio —
//! OpenWhispr's `OpenAICompatiblePanel` pattern: `data` or `models`, `id` or
//! `name`). Only a provider the person saved a key (or an endpoint) for is
//! ever asked; nothing goes to a catalog of Yap's own or a third party's,
//! and a signed-out Yap still never contacts the account service.
//!
//! **What's kept**: chat-capable text models only ([`chat_capable`] + each
//! provider's own rules: Gemini's `supportedGenerationMethods` must have
//! `generateContent`, Groq's and OpenRouter's output must be text; no
//! embeddings, speech, transcription, image/video/music, moderation or
//! guard models, realtime/live/native-audio variants, search-preview,
//! computer-use, deep-research, Codex or `-pro` Responses-only models);
//! dated snapshots fold into the alias the list also has
//! (`gpt-4.1-2025-04-14` into `gpt-4.1`), and one without its alias keeps a
//! `base` (`claude-haiku-4-5-20251001` → `claude-haiku-4-5`), so a curated
//! alias still matches. Borderline models are left out rather than listed.
//!
//! **Names**: the provider's own (Anthropic, Gemini, Groq, OpenRouter —
//! "OpenAI: " and "Qwen/" prefixes cut), else made from the id
//! ([`derive_name`]: `gpt-5.5` → "GPT-5.5", `openai/gpt-oss-20b` →
//! "GPT-OSS 20B"). A curated entry's label always wins, on the page.
//!
//! **Cache**: `<data>/model_catalog.json` (atomic write, a corrupt file
//! quarantined, like every store): per provider the last good list
//! (`fetchedAt`, models with id, name, created, context, tools / thinking
//! when the provider says, `firstSeen`), and the last attempt (`checkedAt`,
//! `error`). Never a key. A list is fresh for a day ([`FRESH_FOR`]); a
//! failed check is tried again after an hour. Checked in the background
//! ([`init`]: shortly after launch, then hourly for whatever is due), right
//! after a key is saved or changed ([`on_config_saved`]), and when a picker
//! asks for the lists ([`model_catalog`]: the cached ones at once, a due one
//! fetched behind it, `yap-model-catalog` when it lands). Logs name the
//! provider and counts, never a key or a body.
//!
//! **Test mode** (`e2e::active`) never contacts a real provider: the
//! debug-only `YAP_MODELS_STANDIN` stands in for every provider
//! (`<standin>/<host><path>?<query>`, `e2e/support/fake-llm.js`), and with
//! none set nothing is fetched (the curated lists show). A self-hosted
//! endpoint in a test run must be the test's own (127.0.0.1).

use serde::{Deserialize, Serialize};
use crate::privacy::net::Recorded as _;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use url::Url;

use crate::config::YapConfig;

/// The cloud providers with a list to ask for (every keyed provider).
pub const PROVIDERS: [&str; 5] = crate::ai::ai_model::KEYED_PROVIDERS;

/// A good list is used and not asked for again for a day.
pub const FRESH_FOR: u64 = 24 * 60 * 60;
/// A failed check is tried again after an hour (or when Settings asks).
const RETRY_AFTER: u64 = 60 * 60;
/// One request's patience.
const TIMEOUT: Duration = Duration::from_secs(15);
/// Pages followed at most (Anthropic, Gemini, OpenRouter page their lists).
const MAX_PAGES: usize = 10;
/// A list bigger than this is refused (OpenRouter's whole catalogue is ~1 MB).
const MAX_BODY: usize = 16 * 1024 * 1024;
/// A self-hosted server's list is asked again after this long.
const LOCAL_FRESH_FOR: u64 = 60;

/// One model a provider offers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogModel {
    /// The id the chat endpoint takes ("gemini-3.6-flash", "openai/gpt-oss-20b").
    pub id: String,
    /// Display name: the provider's, else made from the id.
    #[serde(default)]
    pub name: String,
    /// One short line from the provider (first sentence), when it has one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub desc: String,
    /// When the provider says it was released / added (unix secs).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<u64>,
    /// Context window in tokens, when said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<u64>,
    /// Can it call tools: only when the provider says (Groq's
    /// `supported_features`, OpenRouter's `supported_parameters`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<bool>,
    /// Does it reason before answering (Gemini's `thinking`, Groq's
    /// "reasoning", OpenRouter's `reasoning` parameter).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinks: Option<bool>,
    /// Can it read images (Snip to Chat): only when the provider says —
    /// Anthropic's `capabilities.image_input.supported`, Groq's and
    /// OpenRouter's input modalities. Gemini's and OpenAI's lists don't
    /// say; `snip::vision` has family rules for those.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vision: Option<bool>,
    /// The effort levels it takes, lowest first ("low" … "max"), only when
    /// the provider says: Anthropic's `capabilities.effort`, OpenRouter's
    /// `reasoning.supported_efforts`. `ai/effort.rs` reads them before its
    /// own table, so a new model gets the right choices without an update.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub efforts: Vec<String>,
    /// The effort it runs at when none is sent, when the provider says
    /// (OpenRouter's `reasoning.default_effort`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_effort: Option<String>,
    /// Still callable but on its way out (Anthropic's `lifecycle`,
    /// OpenRouter's `expiration_date`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deprecated: bool,
    /// Dated snapshots of this model the list also had, folded into it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// The alias this dated snapshot is of, when the list doesn't have that
    /// alias itself (`claude-haiku-4-5` for `claude-haiku-4-5-20251001`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    /// When Yap first saw it in this provider's list (unix secs).
    #[serde(default)]
    pub first_seen: u64,
}

/// One provider's list as last fetched, and the last attempt.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderList {
    /// The last good fetch (unix secs; 0 = never).
    #[serde(default)]
    pub fetched_at: u64,
    /// The last attempt, good or not.
    #[serde(default)]
    pub checked_at: u64,
    /// Why the last attempt failed (None = it worked), in words for the page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The last good list.
    #[serde(default)]
    pub models: Vec<CatalogModel>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Catalog {
    #[serde(default)]
    providers: BTreeMap<String, ProviderList>,
}

/// What the page gets for one provider.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListView {
    pub provider: String,
    /// A good list is in hand and the last check worked: the page uses it.
    /// Otherwise it shows Yap's curated list, exactly as before.
    pub ok: bool,
    pub fetched_at: u64,
    pub checked_at: u64,
    pub error: Option<String>,
    /// A check is on its way.
    pub checking: bool,
    /// The list (only when `ok`).
    pub models: Vec<CatalogModel>,
    /// A self-hosted server's word on its models' context (Ollama, LM
    /// Studio), when it says: shown read-only, "set in Ollama".
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub contexts: Vec<ServerContext>,
}

/// What a self-hosted server says about one model's context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerContext {
    pub id: String,
    /// What the model was trained for (Ollama's
    /// `model_info.<arch>.context_length`, LM Studio's
    /// `max_context_length`).
    pub max: Option<u64>,
    /// What the server runs it with (Ollama's `num_ctx`, or a loaded
    /// model's `context_length`; LM Studio's `loaded_context_length`).
    pub set: Option<u64>,
    /// "Ollama" or "LM Studio": where it's set.
    pub app: &'static str,
}

/// Every keyed provider's list, for the pickers.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub providers: BTreeMap<String, ListView>,
    /// This side's clock (unix secs).
    pub now: u64,
    /// Goes up with every snapshot, in the order the store was read: a
    /// command's reply that lands after a newer event is never applied.
    pub rev: u64,
}

/// The last snapshot's `rev`.
static REV: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---------------------------------------------------------------- the store

#[cfg(not(test))]
static STORE: Mutex<Option<Catalog>> = Mutex::new(None);

#[cfg(not(test))]
fn path() -> std::path::PathBuf {
    crate::config::data_dir().join("model_catalog.json")
}

/// Run `f` on the store (loaded on first use, `saving::Saver::loaded`: set
/// aside if it isn't the lists; a file Yap couldn't read is never written
/// over, and read again here until it reads). Unit tests get an empty one
/// per thread and never touch the disk.
#[cfg(not(test))]
fn with_store<R>(f: impl FnOnce(&mut Catalog) -> R) -> Option<R> {
    let mut guard = STORE.lock().ok()?;
    let store = SAVER.loaded(&mut guard, &path(), true);
    Some(f(store))
}

#[cfg(test)]
thread_local! {
    static TEST_STORE: std::cell::RefCell<Catalog> = std::cell::RefCell::new(Catalog::default());
}

#[cfg(test)]
fn with_store<R>(f: impl FnOnce(&mut Catalog) -> R) -> Option<R> {
    TEST_STORE.with(|s| Some(f(&mut s.borrow_mut())))
}

/// model_catalog.json's saves (`saving.rs`): a cache (asked again if lost),
/// so a failed save is only logged, and tried again with the next one.
/// (Unit tests keep the store in memory: nothing saves through it there.)
#[cfg_attr(test, allow(dead_code))]
pub static SAVER: crate::shell::saving::Saver = crate::shell::saving::Saver::quiet("modelCatalog", "the model lists", save_now);

/// Write the lists again (Try again).
#[cfg_attr(test, allow(dead_code))]
fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    with_store(|c| save(c));
    SAVER.check()
}

#[cfg(not(test))]
fn save(c: &Catalog) {
    let _ = SAVER.write_json(&path(), c, true);
}

#[cfg(test)]
fn save(_: &Catalog) {}

/// Providers being asked right now (one check per provider at a time).
static IN_FLIGHT: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn checking(provider: &str) -> bool {
    IN_FLIGHT
        .lock()
        .map(|g| g.as_ref().is_some_and(|s| s.contains(provider)))
        .unwrap_or(false)
}

/// Mark `provider` as being asked; false when it already is.
fn begin(provider: &str) -> bool {
    IN_FLIGHT
        .lock()
        .map(|mut g| {
            g.get_or_insert_with(HashSet::new)
                .insert(provider.to_string())
        })
        .unwrap_or(false)
}

fn end(provider: &str) {
    if let Ok(mut g) = IN_FLIGHT.lock() {
        if let Some(s) = g.as_mut() {
            s.remove(provider);
        }
    }
}

/// What the provider's list says about `model` calling tools: `Some(false)`
/// only when it said no (`tools::supports_tools` then keeps Yap's note
/// tools away from it, and the chip says "no note tools"). `None` = it
/// didn't say, or there's no good list.
pub fn tools_for(provider: &str, model: &str) -> Option<bool> {
    with_store(|c| {
        let list = c.providers.get(provider)?;
        if list.error.is_some() {
            return None;
        }
        list.models
            .iter()
            .find(|m| {
                m.id == model
                    || m.base.as_deref() == Some(model)
                    || m.aliases.iter().any(|a| a == model)
            })
            .and_then(|m| m.tools)
    })
    .flatten()
}

/// What the provider's list says about `model` reading images (`None`: it
/// didn't say, or there's no good list).
pub fn vision_for(provider: &str, model: &str) -> Option<bool> {
    listed_model(provider, model).and_then(|m| m.vision)
}

/// `model` in the provider's good list, if it's there.
pub fn listed_model(provider: &str, model: &str) -> Option<CatalogModel> {
    with_store(|c| {
        let list = c.providers.get(provider)?;
        if list.error.is_some() && list.models.is_empty() {
            return None;
        }
        list.models
            .iter()
            .find(|m| m.id == model || m.base.as_deref() == Some(model) || m.aliases.iter().any(|a| a == model))
            .cloned()
    })
    .flatten()
}

/// A provider's good list (`None`: never fetched, or it's empty).
pub fn models_of(provider: &str) -> Option<Vec<CatalogModel>> {
    with_store(|c| c.providers.get(provider).filter(|l| !l.models.is_empty()).map(|l| l.models.clone())).flatten()
}

/// Debug and test: a provider's list as if it had been fetched.
#[cfg(test)]
pub fn set_models_for_test(provider: &str, models: Vec<CatalogModel>) {
    with_store(|c| {
        c.providers.insert(provider.to_string(), ProviderList { fetched_at: 1, checked_at: 1, error: None, models });
    });
}

// ------------------------------------------------------------ the keys

/// The key Yap would use for `provider` — the scope's own first when the
/// page names one, then the one every scope shares
/// (`YapConfig::provider_api_key`: `pp_api_keys`, the cleanup key), then
/// any scope's or cleanup profile's key for it. Never logged.
pub fn key_for(cfg: &YapConfig, provider: &str, scope: Option<&str>) -> String {
    let scope_key = scope
        .and_then(|s| cfg.llm_scopes.get(s))
        .map(|s| {
            if s.provider == provider {
                s.api_key.clone()
            } else {
                s.api_keys.get(provider).cloned().unwrap_or_default()
            }
        })
        .unwrap_or_default();
    let key = cfg.provider_api_key(provider, &scope_key);
    if !key.is_empty() {
        return key;
    }
    cfg.llm_scopes
        .values()
        .find_map(|s| {
            if s.provider == provider && !s.api_key.is_empty() {
                Some(s.api_key.clone())
            } else {
                s.api_keys.get(provider).filter(|k| !k.is_empty()).cloned()
            }
        })
        .or_else(|| {
            cfg.cleanup_profiles
                .iter()
                .find(|p| p.provider == provider && !p.api_key.is_empty())
                .map(|p| p.api_key.clone())
        })
        .unwrap_or_default()
}

/// Providers the person saved a key for: the only ones ever asked.
pub fn keyed(cfg: &YapConfig) -> Vec<&'static str> {
    PROVIDERS
        .iter()
        .copied()
        .filter(|p| !key_for(cfg, p, None).trim().is_empty())
        .collect()
}

// ------------------------------------------------------------ where to ask

/// Where requests go: the providers themselves, or a test run's stand-in.
#[derive(Debug, Clone)]
struct Endpoints {
    standin: Option<String>,
}

impl Endpoints {
    /// The real providers — or, in a test run, the debug-only
    /// `YAP_MODELS_STANDIN`; `None` (nothing is fetched) in a test run
    /// without one.
    fn current() -> Option<Endpoints> {
        if !crate::shell::e2e::active() {
            return Some(Endpoints { standin: None });
        }
        #[cfg(debug_assertions)]
        {
            std::env::var("YAP_MODELS_STANDIN")
                .ok()
                .map(|v| v.trim().trim_end_matches('/').to_string())
                .filter(|v| !v.is_empty())
                .map(|s| Endpoints { standin: Some(s) })
        }
        #[cfg(not(debug_assertions))]
        Some(Endpoints { standin: None })
    }

    /// The address a request for `url` goes to: itself, or the stand-in
    /// (`<standin>/<host><path>?<query>`).
    fn route(&self, url: &Url) -> Result<Url, String> {
        match &self.standin {
            Some(base) => {
                let mut s = format!("{base}/{}{}", url.host_str().unwrap_or(""), url.path());
                if let Some(q) = url.query() {
                    s.push('?');
                    s.push_str(q);
                }
                Url::parse(&s).map_err(|e| format!("bad stand-in address: {e}"))
            }
            None => Ok(url.clone()),
        }
    }
}

/// The provider's name in words, for messages.
fn label(provider: &str) -> &str {
    match provider {
        "groq" => "Groq",
        "anthropic" => "Anthropic",
        "openai" => "OpenAI",
        "gemini" => "Gemini",
        "openrouter" => "OpenRouter",
        "local" | "custom" => "the server",
        other => other,
    }
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .connect_timeout(Duration::from_secs(6))
        .build()
        .map_err(|e| format!("couldn't start a connection ({e})"))
}

/// One GET, its JSON. Errors are short and in words ("the API key wasn't
/// accepted"); never a key, never the body.
async fn get_json(
    client: &reqwest::Client,
    url: Url,
    headers: &[(&str, &str)],
    bearer: Option<&str>,
    who: &str,
) -> Result<Value, String> {
    let mut req = client.get(url);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    if let Some(key) = bearer.filter(|k| !k.is_empty()) {
        req = req.bearer_auth(key);
    }
    let resp = req.send_for(crate::privacy::net::Purpose::ModelList(who.to_string())).await.map_err(|e| {
        if e.is_timeout() {
            format!("{who} didn't answer in time")
        } else {
            format!("couldn't reach {who}")
        }
    })?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(match status {
            400 | 401 | 403 => "the API key wasn't accepted".to_string(),
            404 => format!("{who} has no model list at that address"),
            429 => format!("{who} is busy (rate limit); Yap will try again later"),
            _ => format!("{who} answered HTTP {status}"),
        });
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|_| format!("{who}'s answer was cut off"))?;
    if bytes.len() > MAX_BODY {
        return Err(format!("{who}'s list was too big"));
    }
    serde_json::from_slice(&bytes).map_err(|_| format!("{who}'s answer wasn't a model list"))
}

fn real(url: &str) -> Url {
    Url::parse(url).expect("static URL")
}

/// Ask `provider` for its models. `Ok((listed, kept))`: how many it listed,
/// and the chat models kept.
async fn fetch(
    provider: &str,
    key: &str,
    ep: &Endpoints,
) -> Result<(usize, Vec<CatalogModel>), String> {
    let client = client()?;
    let who = label(provider);
    match provider {
        "groq" => {
            let url = ep.route(&real("https://api.groq.com/openai/v1/models"))?;
            let v = get_json(&client, url, &[], Some(key), who).await?;
            Ok(parse_groq(&v))
        }
        "openai" => {
            let url = ep.route(&real("https://api.openai.com/v1/models"))?;
            let v = get_json(&client, url, &[], Some(key), who).await?;
            Ok(parse_openai(&v))
        }
        "anthropic" => {
            let mut pages = Vec::new();
            let mut after: Option<String> = None;
            for _ in 0..MAX_PAGES {
                let mut url = real("https://api.anthropic.com/v1/models?limit=1000");
                if let Some(a) = &after {
                    url.query_pairs_mut().append_pair("after_id", a);
                }
                let headers = [("x-api-key", key), ("anthropic-version", "2023-06-01")];
                let v = get_json(&client, ep.route(&url)?, &headers, None, who).await?;
                let more = v["has_more"].as_bool().unwrap_or(false);
                after = v["last_id"].as_str().map(str::to_string);
                pages.push(v);
                if !more || after.is_none() {
                    break;
                }
            }
            Ok(parse_anthropic(&pages))
        }
        "gemini" => {
            let mut pages = Vec::new();
            let mut token: Option<String> = None;
            for _ in 0..MAX_PAGES {
                let mut url =
                    real("https://generativelanguage.googleapis.com/v1beta/models?pageSize=1000");
                if let Some(t) = &token {
                    url.query_pairs_mut().append_pair("pageToken", t);
                }
                // The key in a header, never in the URL (where proxies and
                // logs would keep it).
                let headers = [("x-goog-api-key", key)];
                let v = get_json(&client, ep.route(&url)?, &headers, None, who).await?;
                token = v["nextPageToken"]
                    .as_str()
                    .filter(|t| !t.is_empty())
                    .map(str::to_string);
                pages.push(v);
                if token.is_none() {
                    break;
                }
            }
            Ok(parse_gemini(&pages))
        }
        "openrouter" => {
            let mut pages = Vec::new();
            let mut offset = 0usize;
            for _ in 0..MAX_PAGES {
                let mut url = real("https://openrouter.ai/api/v1/models");
                if offset > 0 {
                    url.query_pairs_mut()
                        .append_pair("offset", &offset.to_string());
                }
                let v = get_json(&client, ep.route(&url)?, &[], Some(key), who).await?;
                let n = v["data"].as_array().map_or(0, Vec::len);
                let next = v["links"]["next"].as_str().is_some_and(|s| !s.is_empty());
                pages.push(v);
                offset += n;
                if !next || n == 0 {
                    break;
                }
            }
            Ok(parse_openrouter(&pages))
        }
        other => Err(format!("{other} has no model list")),
    }
}

// --------------------------------------------------------------- filtering

/// Words (whole, in the id) of models that aren't chat models.
const NOT_CHAT_WORDS: &[&str] = &[
    "tts",
    "aqa",
    "veo",
    "sora",
    "omni",
    "image",
    "images",
    "audio",
    "guard",
    "safeguard",
    "whisper",
    "realtime",
    "moderation",
    "imagen",
    "lyria",
    "orpheus",
    "playai",
    "babbage",
    "davinci",
    "codex",
    "robotics",
    "antigravity",
    "translate",
    "speech",
    "live",
    "music",
    "dall",
    "transcribe",
    "transcription",
];
/// Starts of words of models that aren't chat models.
const NOT_CHAT_PREFIXES: &[&str] = &["embed", "transcri", "moderat", "rerank", "imagen"];
/// Phrases in the id of models that aren't chat models.
const NOT_CHAT_PHRASES: &[&str] = &[
    "search-preview",
    "computer-use",
    "deep-research",
    "native-audio",
    "nano-banana",
    "gpt-image",
    "dall-e",
    "prompt-guard",
];

fn words(id: &str) -> Vec<String> {
    id.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Could this id be a chat model, by its name alone? (No embeddings,
/// speech, transcription, image/video/music, moderation or guard models,
/// realtime/live/native-audio variants, search-preview, computer-use,
/// deep-research or Codex.) Pure.
pub fn chat_capable(id: &str) -> bool {
    let lower = id.to_ascii_lowercase();
    if NOT_CHAT_PHRASES.iter().any(|p| lower.contains(p)) {
        return false;
    }
    !words(id).iter().any(|w| {
        NOT_CHAT_WORDS.contains(&w.as_str()) || NOT_CHAT_PREFIXES.iter().any(|p| w.starts_with(p))
    })
}

/// The alias a dated snapshot id is of: `gpt-4.1-2025-04-14` → `gpt-4.1`,
/// `claude-haiku-4-5-20251001` → `claude-haiku-4-5`, `gemini-2.0-flash-001`
/// → `gemini-2.0-flash`, `kimi-k2-instruct-0905` → `kimi-k2-instruct`,
/// `gemini-2.5-flash-preview-05-20` → `gemini-2.5-flash-preview`. Pure.
pub fn snapshot_base(id: &str) -> Option<String> {
    static RES: std::sync::OnceLock<[regex::Regex; 3]> = std::sync::OnceLock::new();
    let res = RES.get_or_init(|| {
        [
            regex::Regex::new(r"^(.+?)-(?:\d{8}|\d{4}-\d{2}-\d{2})$").expect("static regex"),
            regex::Regex::new(r"^(.+-preview)-\d{2}-\d{2,4}$").expect("static regex"),
            regex::Regex::new(r"^(.+?[a-z].*?)-\d{3,4}$").expect("static regex"),
        ]
    });
    res.iter()
        .find_map(|re| {
            re.captures(id)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string())
        })
        .filter(|b| b.len() >= 2 && b != id)
}

/// Fold dated snapshots into the alias the list also has; give the others
/// their `base`. Order kept. Pure.
pub fn fold_snapshots(models: Vec<CatalogModel>) -> Vec<CatalogModel> {
    let ids: HashSet<String> = models.iter().map(|m| m.id.clone()).collect();
    let mut folded: HashMap<String, Vec<String>> = HashMap::new();
    let mut kept = Vec::with_capacity(models.len());
    for mut m in models {
        match snapshot_base(&m.id) {
            Some(b) if ids.contains(&b) => folded.entry(b).or_default().push(m.id),
            Some(b) => {
                m.base = Some(b);
                kept.push(m);
            }
            None => kept.push(m),
        }
    }
    for m in &mut kept {
        if let Some(a) = folded.remove(&m.id) {
            m.aliases.extend(a);
        }
    }
    kept
}

/// The first sentence of a provider's description, short; "" when it only
/// repeats the name. Markdown links lose their address.
fn short_desc(desc: &str, name: &str) -> String {
    static LINK: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let link =
        LINK.get_or_init(|| regex::Regex::new(r"\[([^\]]*)\]\([^)]*\)").expect("static regex"));
    let text = link.replace_all(desc, "$1");
    let text = text.split('\n').next().unwrap_or("").trim();
    let first = match text.find(". ") {
        Some(i) => &text[..=i],
        None => text,
    }
    .trim()
    .trim_end_matches('.');
    if first.is_empty() || first.eq_ignore_ascii_case(name.trim()) {
        return String::new();
    }
    if first.chars().count() <= 100 {
        return first.to_string();
    }
    let cut: String = first.chars().take(100).collect();
    match cut.rfind(' ') {
        Some(i) => format!("{}…", cut[..i].trim_end_matches([',', ';', ':'])),
        None => format!("{cut}…"),
    }
}

/// A provider's display name, tidied: "OpenAI: GPT-5 Mini" → "GPT-5 Mini",
/// "Qwen/Qwen3.8-27B" → "Qwen3.8-27B". Pure.
pub fn clean_name(name: &str) -> String {
    let mut n = name.trim();
    if let Some(i) = n.find(": ") {
        if i <= 30 {
            n = &n[i + 2..];
        }
    }
    // An org slug before a slash ("Qwen/"), never a slash inside a name.
    if let Some(i) = n.rfind('/') {
        if !n[..i].contains(' ') {
            n = &n[i + 1..];
        }
    }
    n.trim().to_string()
}

/// A readable name from an id: `gpt-5.5` → "GPT-5.5", `openai/gpt-oss-20b`
/// → "GPT-OSS 20B", `gpt-4o-mini` → "GPT-4o Mini", `o4-mini` stays,
/// `llama-4-scout-17b-16e-instruct` → "Llama 4 Scout 17B 16E Instruct". Pure.
pub fn derive_name(id: &str) -> String {
    let last = id.rsplit('/').next().unwrap_or(id);
    let last = last.split(':').next().unwrap_or(last);
    // OpenAI's o-series write themselves in lower case.
    if last.len() >= 2
        && last.starts_with('o')
        && last[1..].chars().next().is_some_and(|c| c.is_ascii_digit())
    {
        return last.to_string();
    }
    let known = |t: &str| -> Option<&'static str> {
        Some(match t {
            "gpt" => "GPT",
            "oss" => "OSS",
            "chatgpt" => "ChatGPT",
            "llama" => "Llama",
            "gemma" => "Gemma",
            "gemini" => "Gemini",
            "claude" => "Claude",
            "deepseek" => "DeepSeek",
            "minimax" => "MiniMax",
            "glm" => "GLM",
            "allam" => "ALLaM",
            "it" => "IT",
            "moe" => "MoE",
            _ => return None,
        })
    };
    let tokens: Vec<&str> = last.split(['-', '_']).filter(|t| !t.is_empty()).collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let t = tokens[i];
        let lower = t.to_ascii_lowercase();
        // GPT and ChatGPT carry their version: "GPT-5.5", "GPT-OSS", "GPT-4o".
        if (lower == "gpt" || lower == "chatgpt") && i + 1 < tokens.len() {
            let next = tokens[i + 1];
            let next = known(&next.to_ascii_lowercase())
                .map(str::to_string)
                .unwrap_or_else(|| next.to_string());
            out.push(format!("{}-{}", known(&lower).unwrap_or(t), next));
            i += 2;
            continue;
        }
        let has_digit = lower.chars().any(|c| c.is_ascii_digit());
        let starts_with_digit = lower.starts_with(|c: char| c.is_ascii_digit());
        let word = if let Some(k) = known(&lower) {
            k.to_string()
        } else if lower.chars().all(|c| c.is_ascii_digit() || c == '.') {
            // A version split by hyphens joins up: "claude-haiku-5-5" →
            // "Claude Haiku 5.5" (dates and longer numbers stay apart).
            let short = |s: &str| (1..=2).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit());
            let mut v = t.to_string();
            if short(t) {
                while i + 1 < tokens.len() && short(tokens[i + 1]) {
                    v = format!("{v}.{}", tokens[i + 1]);
                    i += 1;
                }
            }
            v
        } else if has_digit && (lower.len() <= 4 || (starts_with_digit && lower.len() <= 5)) {
            // Sizes and short codes: "20b" → "20B", "a4b" → "A4B", "k2" → "K2".
            t.to_ascii_uppercase()
        } else {
            let mut c = t.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        };
        out.push(word);
        i += 1;
    }
    out.join(" ")
}

fn str_list(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_ascii_lowercase))
                .collect()
        })
        .unwrap_or_default()
}

fn rfc3339_secs(v: &Value) -> Option<u64> {
    let s = v.as_str()?;
    let t = chrono::DateTime::parse_from_rfc3339(s).ok()?.timestamp();
    // An epoch value means "unknown" (Anthropic's docs).
    (t > 86_400).then_some(t as u64)
}

fn positive(v: &Value) -> Option<u64> {
    v.as_u64().filter(|n| *n > 0)
}

/// Groq's `/openai/v1/models`: text in, text out, active, not a model
/// Groq retired for non-Enterprise keys (config.rs migrates those away).
/// Returns (listed, kept). Pure.
pub fn parse_groq(v: &Value) -> (usize, Vec<CatalogModel>) {
    let data = v["data"].as_array().cloned().unwrap_or_default();
    let listed = data.len();
    let mut out = Vec::new();
    for m in &data {
        let Some(id) = m["id"].as_str() else { continue };
        if m["active"].as_bool() == Some(false)
            || !chat_capable(id)
            || crate::config::is_retired_groq_model(id)
        {
            continue;
        }
        let (inputs, outputs) = (
            str_list(&m["input_modalities"]),
            str_list(&m["output_modalities"]),
        );
        if (!outputs.is_empty() && outputs != ["text"])
            || (!inputs.is_empty() && !inputs.iter().any(|i| i == "text"))
        {
            continue;
        }
        let features = m.get("supported_features").map(str_list);
        let name = m["name"]
            .as_str()
            .map(clean_name)
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| derive_name(id));
        out.push(CatalogModel {
            id: id.to_string(),
            name,
            created: positive(&m["created"]),
            context: positive(&m["context_window"]).or_else(|| positive(&m["context_length"])),
            tools: features.as_ref().map(|f| f.iter().any(|x| x == "tools")),
            thinks: features
                .as_ref()
                .map(|f| f.iter().any(|x| x == "reasoning")),
            vision: (!inputs.is_empty()).then(|| inputs.iter().any(|i| i == "image")),
            ..Default::default()
        });
    }
    (listed, fold_snapshots(out))
}

/// OpenAI's `/v1/models`: GPT and o-series chat models; not the
/// Responses-only ones (`-pro`, Codex, deep research), `-latest` /
/// `chat-latest` aliases, instruct (completions), search or cyber models.
/// Pure.
pub fn parse_openai(v: &Value) -> (usize, Vec<CatalogModel>) {
    let data = v["data"].as_array().cloned().unwrap_or_default();
    let listed = data.len();
    let mut out = Vec::new();
    for m in &data {
        let Some(id) = m["id"].as_str() else { continue };
        let lower = id.to_ascii_lowercase();
        let o_series =
            lower.len() >= 2 && lower.starts_with('o') && lower.as_bytes()[1].is_ascii_digit();
        if !(lower.starts_with("gpt-") || lower.starts_with("chatgpt-") || o_series)
            || !chat_capable(id)
        {
            continue;
        }
        let w = words(id);
        if ["pro", "instruct", "latest", "search", "cyber"]
            .iter()
            .any(|x| w.iter().any(|t| t == x))
        {
            continue;
        }
        out.push(CatalogModel {
            id: id.to_string(),
            name: derive_name(id),
            created: positive(&m["created"]),
            ..Default::default()
        });
    }
    (listed, fold_snapshots(out))
}

/// Anthropic's `/v1/models` pages: every Claude model is a chat model;
/// retired ones are left out, deprecated ones marked. Pure.
pub fn parse_anthropic(pages: &[Value]) -> (usize, Vec<CatalogModel>) {
    let mut listed = 0;
    let mut out = Vec::new();
    for v in pages {
        let data = v["data"].as_array().cloned().unwrap_or_default();
        listed += data.len();
        for m in &data {
            let Some(id) = m["id"].as_str() else { continue };
            let lifecycle = m["lifecycle"].as_str().unwrap_or("active");
            if lifecycle == "retired"
                || !chat_capable(id)
                || out.iter().any(|x: &CatalogModel| x.id == id)
            {
                continue;
            }
            out.push(CatalogModel {
                id: id.to_string(),
                name: m["display_name"]
                    .as_str()
                    .map(clean_name)
                    .filter(|n| !n.is_empty())
                    .unwrap_or_else(|| derive_name(id)),
                created: rfc3339_secs(&m["created_at"]),
                context: positive(&m["max_input_tokens"]),
                deprecated: lifecycle == "deprecated",
                vision: m["capabilities"]["image_input"]["supported"].as_bool(),
                efforts: anthropic_efforts(&m["capabilities"]["effort"]),
                ..Default::default()
            });
        }
    }
    (listed, fold_snapshots(out))
}

/// The effort levels Anthropic's list says a model takes
/// (`capabilities.effort`: `{supported, low: {supported}, …, max:
/// {supported}}`), lowest first; none when it takes none. Pure.
fn anthropic_efforts(effort: &Value) -> Vec<String> {
    if effort["supported"].as_bool() != Some(true) {
        return Vec::new();
    }
    crate::ai::effort::Level::ALL
        .iter()
        .map(|l| l.id())
        .filter(|id| effort[*id]["supported"].as_bool() == Some(true))
        .map(str::to_string)
        .collect()
}

/// The effort levels OpenRouter's list says a model takes
/// (`reasoning.supported_efforts`, highest first, `null` for "any"), lowest
/// first and only Yap's five ("minimal" and "none" left out); none when it
/// doesn't say. Pure.
fn openrouter_efforts(reasoning: &Value) -> Vec<String> {
    let Some(listed) = reasoning["supported_efforts"].as_array() else {
        return Vec::new();
    };
    crate::ai::effort::Level::ALL
        .iter()
        .map(|l| l.id())
        .filter(|id| listed.iter().any(|e| e.as_str() == Some(id)))
        .map(str::to_string)
        .collect()
}

/// Gemini's native `/v1beta/models` pages: `generateContent` models only,
/// no moving `-latest` aliases, experiments or the custom-tools variant.
/// Pure.
pub fn parse_gemini(pages: &[Value]) -> (usize, Vec<CatalogModel>) {
    let mut listed = 0;
    let mut out = Vec::new();
    for v in pages {
        let models = v["models"].as_array().cloned().unwrap_or_default();
        listed += models.len();
        for m in &models {
            let Some(full) = m["name"].as_str() else {
                continue;
            };
            let id = full.strip_prefix("models/").unwrap_or(full);
            let methods = str_list(&m["supportedGenerationMethods"]);
            if !methods.iter().any(|x| x == "generatecontent") || !chat_capable(id) {
                continue;
            }
            let w = words(id);
            if ["latest", "exp", "customtools", "experimental"]
                .iter()
                .any(|x| w.iter().any(|t| t == x))
            {
                continue;
            }
            if out.iter().any(|x: &CatalogModel| x.id == id) {
                continue;
            }
            let name = m["displayName"]
                .as_str()
                .map(clean_name)
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| derive_name(id));
            out.push(CatalogModel {
                id: id.to_string(),
                desc: short_desc(m["description"].as_str().unwrap_or(""), &name),
                name,
                context: positive(&m["inputTokenLimit"]),
                thinks: m["thinking"].as_bool(),
                ..Default::default()
            });
        }
    }
    (listed, fold_snapshots(out))
}

/// OpenRouter's `/api/v1/models` pages: text in, text only out; no `~`
/// moving aliases. Says which models take tools. Pure.
pub fn parse_openrouter(pages: &[Value]) -> (usize, Vec<CatalogModel>) {
    let mut listed = 0;
    let mut out = Vec::new();
    for v in pages {
        let data = v["data"].as_array().cloned().unwrap_or_default();
        listed += data.len();
        for m in &data {
            let Some(id) = m["id"].as_str() else { continue };
            if id.starts_with('~')
                || !chat_capable(id)
                || out.iter().any(|x: &CatalogModel| x.id == id)
            {
                continue;
            }
            let arch = &m["architecture"];
            let (inputs, outputs) = (
                str_list(&arch["input_modalities"]),
                str_list(&arch["output_modalities"]),
            );
            if (!outputs.is_empty() && outputs != ["text"])
                || (!inputs.is_empty() && !inputs.iter().any(|i| i == "text"))
            {
                continue;
            }
            let params = m.get("supported_parameters").map(str_list);
            let name = m["name"]
                .as_str()
                .map(clean_name)
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| derive_name(id));
            out.push(CatalogModel {
                id: id.to_string(),
                desc: short_desc(m["description"].as_str().unwrap_or(""), &name),
                name,
                created: positive(&m["created"]),
                context: positive(&m["context_length"]),
                tools: params.as_ref().map(|p| p.iter().any(|x| x == "tools")),
                thinks: params.as_ref().map(|p| {
                    p.iter()
                        .any(|x| x == "reasoning" || x == "include_reasoning")
                }),
                vision: (!inputs.is_empty()).then(|| inputs.iter().any(|i| i == "image")),
                deprecated: m["expiration_date"].as_str().is_some_and(|s| !s.is_empty()),
                efforts: openrouter_efforts(&m["reasoning"]),
                default_effort: m["reasoning"]["default_effort"].as_str().map(str::to_string),
                ..Default::default()
            });
        }
    }
    (listed, fold_snapshots(out))
}

/// A self-hosted / custom OpenAI-compatible server's `/models`
/// (OpenWhispr's reading: `data` or `models`, each `id` or `name`); no
/// embedding or speech models. Pure.
pub fn parse_compatible(v: &Value) -> (usize, Vec<CatalogModel>) {
    let list = v["data"]
        .as_array()
        .or_else(|| v["models"].as_array())
        .cloned()
        .unwrap_or_default();
    let listed = list.len();
    let mut out: Vec<CatalogModel> = Vec::new();
    for m in &list {
        let id = m["id"]
            .as_str()
            .or_else(|| m["name"].as_str())
            .or_else(|| m["model"].as_str())
            .or_else(|| m.as_str());
        let Some(id) = id.map(str::trim).filter(|s| !s.is_empty()) else {
            continue;
        };
        if !chat_capable(id) || out.iter().any(|x| x.id == id) {
            continue;
        }
        out.push(CatalogModel {
            id: id.to_string(),
            name: id.to_string(),
            created: positive(&m["created"]),
            ..Default::default()
        });
    }
    (listed, out)
}

// ------------------------------------------------------------ refreshing

/// Is a check of this list due? A good one after a day, a failed one after
/// an hour, and always when the clock went back.
fn due(list: Option<&ProviderList>, now: u64) -> bool {
    let Some(l) = list else { return true };
    if l.checked_at > now || l.fetched_at > now {
        return true;
    }
    match l.error {
        Some(_) => now - l.checked_at >= RETRY_AFTER,
        None => l.fetched_at == 0 || now - l.fetched_at >= FRESH_FOR,
    }
}

/// Keep each model's `firstSeen` from the last list; a new one is seen now.
fn carry_first_seen(
    prev: Option<&ProviderList>,
    mut models: Vec<CatalogModel>,
    now: u64,
) -> Vec<CatalogModel> {
    let seen: HashMap<&str, u64> = prev
        .map(|p| {
            p.models
                .iter()
                .map(|m| (m.id.as_str(), m.first_seen))
                .collect()
        })
        .unwrap_or_default();
    for m in &mut models {
        m.first_seen = seen
            .get(m.id.as_str())
            .copied()
            .filter(|t| *t > 0)
            .unwrap_or(now);
    }
    models
}

/// Record a check's result in the store (and the file).
fn record(provider: &str, result: Result<Vec<CatalogModel>, String>, now: u64) {
    with_store(|c| {
        let prev = c.providers.get(provider).cloned();
        let entry = c.providers.entry(provider.to_string()).or_default();
        entry.checked_at = now;
        match result {
            Ok(models) => {
                entry.models = carry_first_seen(prev.as_ref(), models, now);
                entry.fetched_at = now;
                entry.error = None;
            }
            Err(e) => entry.error = Some(e),
        }
        save(c);
    });
}

fn view(provider: &str, list: Option<&ProviderList>) -> ListView {
    let ok = list.is_some_and(|l| l.error.is_none() && l.fetched_at > 0);
    ListView {
        provider: provider.to_string(),
        ok,
        fetched_at: list.map_or(0, |l| l.fetched_at),
        checked_at: list.map_or(0, |l| l.checked_at),
        error: list.and_then(|l| l.error.clone()),
        checking: checking(provider),
        models: if ok {
            list.map(|l| l.models.clone()).unwrap_or_default()
        } else {
            Vec::new()
        },
        contexts: Vec::new(),
    }
}

/// Every keyed provider's list as Yap has it now.
pub fn snapshot(cfg: &YapConfig) -> Snapshot {
    let keyed = keyed(cfg);
    // The rev is taken under the store's lock, with the read.
    let (providers, rev) = with_store(|c| {
        let providers = keyed
            .iter()
            .map(|p| (p.to_string(), view(p, c.providers.get(*p))))
            .collect::<BTreeMap<_, _>>();
        (providers, REV.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1)
    })
    .unwrap_or_default();
    Snapshot {
        providers,
        now: now_secs(),
        rev,
    }
}

fn broadcast(app: &AppHandle) {
    let _ = app.emit("yap-model-catalog", snapshot(&crate::config::load()));
}

/// Check `provider`'s list now (when `force`, or when it's due) with `key`.
/// One check per provider at a time; the result is stored and broadcast.
async fn refresh(app: Option<AppHandle>, provider: &str, key: String, force: bool) {
    let Some(ep) = Endpoints::current() else {
        return; // a test run without a stand-in: never online
    };
    let now = now_secs();
    if key.trim().is_empty()
        || !force && !with_store(|c| due(c.providers.get(provider), now)).unwrap_or(true)
    {
        return;
    }
    if !begin(provider) {
        return;
    }
    if let Some(app) = &app {
        broadcast(app); // "checking…"
    }
    let result = fetch(provider, key.trim(), &ep).await;
    match &result {
        Ok((listed, kept)) => tracing::info!(
            "Model list from {}: {} listed, {} chat models kept",
            label(provider),
            listed,
            kept.len()
        ),
        Err(e) => tracing::warn!("Couldn't check {}'s model list: {}", label(provider), e),
    }
    record(provider, result.map(|(_, kept)| kept), now_secs());
    end(provider);
    if let Some(app) = &app {
        broadcast(app);
    }
}

/// Check every keyed provider whose list is due, in the background.
pub fn refresh_due(app: &AppHandle, cfg: &YapConfig) {
    for p in keyed(cfg) {
        let key = key_for(cfg, p, None);
        let app = app.clone();
        tauri::async_runtime::spawn(async move { refresh(Some(app), p, key, false).await });
    }
}

/// Background checks: shortly after launch, then every hour for whatever
/// is due (each list at most once a day). Not in a test run, where the
/// pickers and key saves ask instead (deterministic).
pub fn init(app: &AppHandle) {
    if crate::shell::e2e::active() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(40)).await;
        loop {
            refresh_due(&app, &crate::config::load());
            tokio::time::sleep(Duration::from_secs(60 * 60)).await;
        }
    });
}

/// A save that set or changed a provider's key checks that provider's list
/// at once (a new key may see other models; a wrong one says so).
pub fn on_config_saved(app: &AppHandle, prev: &YapConfig, cfg: &YapConfig) {
    for p in PROVIDERS {
        let (before, after) = (key_for(prev, p, None), key_for(cfg, p, None));
        if !after.trim().is_empty() && before != after {
            let app = app.clone();
            tauri::async_runtime::spawn(async move { refresh(Some(app), p, after, true).await });
        }
    }
}

// ------------------------------------------------- self-hosted servers

/// Base URL → (when it was asked, its models, what it says of their
/// context).
type ServerLists = HashMap<String, (u64, Vec<CatalogModel>, Vec<ServerContext>)>;

/// Self-hosted servers' lists, in memory only.
static LOCAL: Mutex<Option<ServerLists>> = Mutex::new(None);

/// A self-hosted / custom server's models (`GET {base}/models`), and what
/// it says of their context ([`server_contexts`]).
async fn local_models(
    base: &str,
    key: &str,
    force: bool,
) -> Result<(Vec<CatalogModel>, Vec<ServerContext>), String> {
    let base = base.trim().trim_end_matches('/');
    let url = Url::parse(&format!("{base}/models"))
        .map_err(|_| "that isn't a web address".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("that isn't a web address".to_string());
    }
    // A test run asks only its own servers (127.0.0.1 on a test's port),
    // never the internet or a real Ollama on `localhost:11434`.
    if crate::shell::e2e::active() && url.host_str() != Some("127.0.0.1") {
        return Err("test runs only ask the test's own servers".to_string());
    }
    let now = now_secs();
    if !force {
        let cached = LOCAL.lock().ok().and_then(|g| {
            g.as_ref()
                .and_then(|m| m.get(base).cloned())
                .filter(|(at, _, _)| now.saturating_sub(*at) < LOCAL_FRESH_FOR)
        });
        if let Some((_, models, contexts)) = cached {
            return Ok((models, contexts));
        }
    }
    let client = client()?;
    let v = get_json(&client, url, &[], Some(key), "the server").await?;
    let (listed, models) = parse_compatible(&v);
    let contexts = server_contexts(base, key, &models).await;
    tracing::info!(
        "Model list from a self-hosted server: {} listed, {} kept, {} with a context",
        listed,
        models.len(),
        contexts.len()
    );
    if let Ok(mut g) = LOCAL.lock() {
        g.get_or_insert_with(HashMap::new)
            .insert(base.to_string(), (now, models.clone(), contexts.clone()));
    }
    Ok((models, contexts))
}

/// What a self-hosted server said of `model`'s context when its list was
/// last asked (in memory, any age): what it runs it with, else what the
/// model was trained for. `None`: not asked, or it didn't say.
pub fn server_context(base: &str, model: &str) -> Option<u64> {
    let base = base.trim().trim_end_matches('/');
    let guard = LOCAL.lock().ok()?;
    let (_, _, contexts) = guard.as_ref()?.get(base)?;
    contexts.iter().find(|c| c.id == model).and_then(|c| c.set.or(c.max))
}

/// [`server_context`], asking the server for its list first when it's on
/// this PC and hasn't been asked in the last minute (Chat's context ring).
/// A server elsewhere is never asked for this: only what its list said
/// when someone opened it.
pub async fn server_context_here(cfg: &YapConfig, provider: &str, base: &str, model: &str) -> Option<u64> {
    if let Some(n) = server_context(base, model) {
        return Some(n);
    }
    if !crate::ai::ai_model::runs_here(provider, base) {
        return None;
    }
    let key = endpoint_key(cfg, Some("chat"), provider);
    let (_, contexts) = local_models(base, &key, false).await.ok()?;
    contexts.iter().find(|c| c.id == model).and_then(|c| c.set.or(c.max))
}

/// The server's own address under its OpenAI-compatible `/v1`. Pure.
fn server_root(base: &str) -> &str {
    let b = base.trim().trim_end_matches('/');
    b.strip_suffix("/v1").unwrap_or(b)
}

/// LM Studio's `GET /api/v0/models`: each model's `max_context_length`,
/// and a loaded one's `loaded_context_length`. Pure.
pub fn parse_lmstudio(v: &Value) -> Vec<ServerContext> {
    v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = m["id"].as_str()?.trim();
            let max = positive(&m["max_context_length"]);
            let set = positive(&m["loaded_context_length"]);
            (!id.is_empty() && (max.is_some() || set.is_some())).then(|| ServerContext {
                id: id.to_string(),
                max,
                set,
                app: "LM Studio",
            })
        })
        .collect()
}

/// Ollama's `GET /api/ps`: the loaded models' `context_length`, by name.
/// `None` when it isn't Ollama's answer. Pure.
pub fn parse_ollama_ps(v: &Value) -> Option<HashMap<String, u64>> {
    let list = v["models"].as_array()?;
    Some(
        list.iter()
            .filter_map(|m| {
                let name = m["name"].as_str().or_else(|| m["model"].as_str())?;
                Some((name.to_string(), positive(&m["context_length"])?))
            })
            .collect(),
    )
}

/// Ollama's `POST /api/show`: `(max, num_ctx)` — the trained context
/// (`model_info.<arch>.context_length`) and a `num_ctx` its Modelfile sets.
/// Pure.
pub fn parse_ollama_show(v: &Value) -> (Option<u64>, Option<u64>) {
    let max = v["model_info"].as_object().and_then(|info| {
        info.iter()
            .filter(|(k, _)| k.ends_with(".context_length"))
            .find_map(|(_, v)| positive(v))
    });
    let num_ctx = v["parameters"].as_str().and_then(|p| {
        p.lines().find_map(|l| {
            let mut w = l.split_whitespace();
            (w.next() == Some("num_ctx")).then(|| w.next()?.parse().ok()).flatten()
        })
    });
    (max, num_ctx.filter(|&n: &u64| n > 0))
}

/// llama.cpp's `GET /props`: the context a request gets
/// (`default_generation_settings.n_ctx`, else `n_ctx`). Pure.
pub fn parse_llamacpp_props(v: &Value) -> Option<u64> {
    positive(&v["default_generation_settings"]["n_ctx"]).or_else(|| positive(&v["n_ctx"]))
}

/// The most models one server is asked about (Ollama: one `/api/show` each).
const SERVER_CONTEXT_MODELS: usize = 16;
const SERVER_CONTEXT_TIMEOUT: Duration = Duration::from_secs(3);

/// What a self-hosted server says about its models' context, read-only and
/// best-effort (any failure: nothing): LM Studio's `/api/v0/models`, else
/// Ollama's `/api/ps` (loaded models) and `/api/show` per model (≤ 16).
/// A server that's neither says nothing, and nothing is shown.
async fn server_contexts(base: &str, key: &str, models: &[CatalogModel]) -> Vec<ServerContext> {
    let Ok(client) = reqwest::Client::builder()
        .timeout(SERVER_CONTEXT_TIMEOUT)
        .connect_timeout(SERVER_CONTEXT_TIMEOUT)
        .build()
    else {
        return Vec::new();
    };
    let root = server_root(base);
    let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).take(SERVER_CONTEXT_MODELS).collect();
    if let Ok(url) = Url::parse(&format!("{root}/api/v0/models")) {
        if let Ok(v) = get_json(&client, url, &[], Some(key), "the server").await {
            let found = parse_lmstudio(&v);
            if !found.is_empty() {
                return found.into_iter().filter(|c| ids.contains(&c.id.as_str())).collect();
            }
        }
    }
    // llama.cpp's own server (llama-server, a llamafile): `/props` says the
    // context it serves its one model with.
    if let Ok(url) = Url::parse(&format!("{root}/props")) {
        if let Some(n) = get_json(&client, url, &[], Some(key), "the server").await.ok().and_then(|v| parse_llamacpp_props(&v)) {
            return ids.iter().map(|id| ServerContext { id: id.to_string(), max: None, set: Some(n), app: "llama.cpp" }).collect();
        }
    }
    let Ok(url) = Url::parse(&format!("{root}/api/ps")) else {
        return Vec::new();
    };
    let Some(loaded) = get_json(&client, url, &[], None, "the server").await.ok().and_then(|v| parse_ollama_ps(&v)) else {
        return Vec::new();
    };
    let show = format!("{root}/api/show");
    let asks = ids.iter().map(|id| {
        let (client, show) = (client.clone(), show.clone());
        async move {
            let resp = client.post(&show).json(&serde_json::json!({ "model": id })).send_for(crate::privacy::net::Purpose::ModelList("your server".into())).await.ok()?;
            if !resp.status().is_success() {
                return None;
            }
            let v: Value = resp.json().await.ok()?;
            Some(parse_ollama_show(&v))
        }
    });
    let shown = futures_util::future::join_all(asks).await;
    ids.iter()
        .zip(shown)
        .filter_map(|(id, s)| {
            let (max, num_ctx) = s.unwrap_or((None, None));
            let set = loaded.get(*id).copied().or(num_ctx);
            (max.is_some() || set.is_some()).then(|| ServerContext { id: id.to_string(), max, set, app: "Ollama" })
        })
        .collect()
}

/// The base URL and key a scope's self-hosted / custom endpoint uses.
pub(crate) fn endpoint_key(cfg: &YapConfig, scope: Option<&str>, provider: &str) -> String {
    match scope {
        None | Some("cleanup") => {
            if cfg.pp_provider == provider {
                cfg.pp_api_key.clone()
            } else {
                cfg.pp_api_keys.get(provider).cloned().unwrap_or_default()
            }
        }
        Some(s) if s.starts_with("profile:") => {
            let id = &s["profile:".len()..];
            cfg.cleanup_profiles
                .iter()
                .find(|p| p.id == id)
                .map(|p| p.api_key.clone())
                .unwrap_or_default()
        }
        Some(s) => cfg
            .llm_scopes
            .get(s)
            .map(|sc| {
                if sc.provider == provider {
                    sc.api_key.clone()
                } else {
                    sc.api_keys.get(provider).cloned().unwrap_or_default()
                }
            })
            .unwrap_or_default(),
    }
}

// ------------------------------------------------------------- commands

/// Every keyed provider's list as Yap has it (the pickers show these at
/// once), and a check behind it of any that's due; `yap-model-catalog`
/// brings the result.
#[tauri::command]
pub fn model_catalog(app: AppHandle) -> Snapshot {
    let cfg = crate::config::load();
    refresh_due(&app, &cfg);
    snapshot(&cfg)
}

/// Check one provider's list now (Settings' "Check now"), then every
/// keyed provider's list.
#[tauri::command]
pub async fn model_catalog_refresh(app: AppHandle, provider: String) -> Result<Snapshot, String> {
    let cfg = crate::config::load();
    if !PROVIDERS.contains(&provider.as_str()) {
        return Err(format!("{provider} has no model list"));
    }
    let key = key_for(&cfg, &provider, None);
    if key.trim().is_empty() {
        return Err(format!(
            "No {} API key — add one in Settings → Language Models",
            label(&provider)
        ));
    }
    refresh(Some(app), &provider, key, true).await;
    Ok(snapshot(&crate::config::load()))
}

/// One provider's models, with the key Yap uses for it (`scope`: "cleanup",
/// a scope's key — "chat", "noteFormatting", "voiceAgent" — or
/// "profile:<id>"). A keyed cloud provider: its cached list, checked first
/// when due (or `force`). A self-hosted / custom endpoint (`local`,
/// `custom`): `GET {baseUrl}/models` (kept a minute), with the scope's key.
#[tauri::command]
pub async fn llm_models(
    app: AppHandle,
    provider: String,
    scope: Option<String>,
    base_url: Option<String>,
    force: Option<bool>,
) -> Result<ListView, String> {
    let cfg = crate::config::load();
    let force = force.unwrap_or(false);
    if PROVIDERS.contains(&provider.as_str()) {
        let key = key_for(&cfg, &provider, scope.as_deref());
        if key.trim().is_empty() {
            return Ok(view(&provider, None));
        }
        refresh(Some(app), &provider, key, force).await;
        return Ok(with_store(|c| view(&provider, c.providers.get(&provider)))
            .unwrap_or_else(|| view(&provider, None)));
    }
    let base = base_url.unwrap_or_default();
    if base.trim().is_empty() {
        return Err("Add the server's address first".to_string());
    }
    let key = endpoint_key(&cfg, scope.as_deref(), &provider);
    let now = now_secs();
    Ok(match local_models(&base, &key, force).await {
        Ok((mut models, contexts)) => {
            // Whether Chat would offer each Yap's note tools (a local model
            // under 4B: no), for the Chat picker's "no note tools".
            for m in &mut models {
                if m.tools.is_none() {
                    m.tools = Some(crate::chat::tools::supports_tools(&provider, &m.id));
                }
            }
            ListView {
                provider,
                ok: true,
                fetched_at: now,
                checked_at: now,
                error: None,
                checking: false,
                models,
                contexts,
            }
        }
        Err(e) => ListView {
            provider,
            ok: false,
            fetched_at: 0,
            checked_at: now,
            error: Some(e),
            checking: false,
            models: Vec::new(),
            contexts: Vec::new(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ids(models: &[CatalogModel]) -> Vec<&str> {
        models.iter().map(|m| m.id.as_str()).collect()
    }

    #[test]
    fn groq_keeps_text_chat_models_only() {
        // Trimmed from Groq's real list (2026-10-18).
        let v = json!({"object": "list", "data": [
            {"id": "openai/gpt-oss-20b", "created": 1754407957, "active": true, "context_window": 131072,
             "name": "GPT OSS 20B", "input_modalities": ["text"], "output_modalities": ["text"],
             "supported_features": ["tools", "json_mode", "structured_outputs", "reasoning"]},
            {"id": "allam-2-7b", "created": 1737672203, "active": true, "context_window": 4096,
             "name": "ALLaM-2-7b", "input_modalities": ["text"], "output_modalities": ["text"], "supported_features": ["json_mode"]},
            {"id": "meta-llama/llama-prompt-guard-2-86m", "active": true, "name": "Prompt Guard 2 86M",
             "input_modalities": ["text"], "output_modalities": ["text"]},
            {"id": "canopylabs/orpheus-v1-english", "active": true, "input_modalities": ["text"], "output_modalities": ["speech"]},
            {"id": "qwen/qwen3.8-27b", "created": 1786984846, "active": true, "context_window": 131072,
             "name": "Qwen/Qwen3.8-27B", "input_modalities": ["text", "image"], "output_modalities": ["text"],
             "supported_features": ["tools", "json_mode", "reasoning"]},
            {"id": "openai/gpt-oss-safeguard-20b", "active": true, "input_modalities": ["text"], "output_modalities": ["text"]},
            {"id": "whisper-large-v3-turbo", "active": true, "input_modalities": ["audio"], "output_modalities": ["transcription"]},
            {"id": "llama-3.3-70b-versatile", "active": true},
            {"id": "acme/sleepy-1", "active": false}
        ]});
        let (listed, kept) = parse_groq(&v);
        assert_eq!(listed, 9);
        assert_eq!(
            ids(&kept),
            ["openai/gpt-oss-20b", "allam-2-7b", "qwen/qwen3.8-27b"]
        );
        let oss = &kept[0];
        assert_eq!(
            (oss.name.as_str(), oss.tools, oss.thinks),
            ("GPT OSS 20B", Some(true), Some(true))
        );
        assert_eq!(oss.context, Some(131072));
        assert_eq!(oss.created, Some(1754407957));
        assert_eq!(kept[1].tools, Some(false));
        assert_eq!(kept[2].name, "Qwen3.8-27B");
    }

    #[test]
    fn openai_drops_non_chat_responses_only_and_dated_duplicates() {
        let list = [
            "gpt-5.5",
            "gpt-5.5-pro",
            "gpt-5-mini",
            "gpt-5-mini-2025-08-07",
            "gpt-4.1",
            "gpt-4.1-2025-04-14",
            "gpt-4o-mini-tts",
            "gpt-4o-transcribe",
            "gpt-4o-realtime-preview",
            "gpt-4o-search-preview",
            "gpt-image-1",
            "dall-e-3",
            "tts-1",
            "whisper-1",
            "text-embedding-3-small",
            "omni-moderation-latest",
            "babbage-002",
            "gpt-3.5-turbo-instruct",
            "o3",
            "o3-pro",
            "o4-mini-deep-research",
            "codex-mini-latest",
            "gpt-5.1-codex",
            "computer-use-preview",
            "chatgpt-4o-latest",
            "gpt-5-chat-latest",
            "gpt-audio",
            "gpt-realtime",
            "sora-2",
            "gpt-5.6-cyber",
            "gpt-6-luna",
            "o4-mini",
        ];
        let v = json!({"data": list.iter().map(|id| json!({"id": id, "object": "model", "created": 1_760_000_000u64, "owned_by": "openai"})).collect::<Vec<_>>()});
        let (listed, kept) = parse_openai(&v);
        assert_eq!(listed, list.len());
        assert_eq!(
            ids(&kept),
            [
                "gpt-5.5",
                "gpt-5-mini",
                "gpt-4.1",
                "o3",
                "gpt-6-luna",
                "o4-mini"
            ]
        );
        let mini = kept.iter().find(|m| m.id == "gpt-5-mini").unwrap();
        assert_eq!(mini.aliases, ["gpt-5-mini-2025-08-07"]);
        assert_eq!(mini.name, "GPT-5 Mini");
        assert_eq!(kept[0].name, "GPT-5.5");
    }

    #[test]
    fn anthropic_pages_names_and_lifecycle() {
        let p1 = json!({"data": [
            {"type": "model", "id": "claude-opus-5-5", "display_name": "Claude Opus 5.5", "created_at": "2026-09-21T00:00:00Z", "lifecycle": "active", "max_input_tokens": 1000000},
            {"type": "model", "id": "claude-haiku-4-5-20251001", "display_name": "Claude Haiku 4.5", "created_at": "2025-10-01T00:00:00Z", "lifecycle": "active"}
        ], "has_more": true, "last_id": "claude-haiku-4-5-20251001"});
        let p2 = json!({"data": [
            {"type": "model", "id": "claude-sonnet-4-5-20250929", "display_name": "Claude Sonnet 4.5", "created_at": "2025-09-29T00:00:00Z", "lifecycle": "deprecated"},
            {"type": "model", "id": "claude-3-5-haiku-20241022", "display_name": "Claude Haiku 3.5", "created_at": "1970-01-01T00:00:00Z", "lifecycle": "retired"}
        ], "has_more": false});
        let (listed, kept) = parse_anthropic(&[p1, p2]);
        assert_eq!(listed, 4);
        assert_eq!(
            ids(&kept),
            [
                "claude-opus-5-5",
                "claude-haiku-4-5-20251001",
                "claude-sonnet-4-5-20250929"
            ]
        );
        assert_eq!(kept[0].created, Some(1789948800));
        assert_eq!(kept[0].context, Some(1_000_000));
        // A dated id without its alias in the list keeps the alias as `base`,
        // so the curated "claude-haiku-4-5" still matches it.
        assert_eq!(kept[1].base.as_deref(), Some("claude-haiku-4-5"));
        assert!(kept[2].deprecated && !kept[0].deprecated);
    }

    #[test]
    fn gemini_keeps_generate_content_chat_models() {
        let m = |name: &str, display: &str, methods: &[&str]| {
            json!({"name": format!("models/{name}"), "displayName": display, "description": display,
                   "inputTokenLimit": 1048576, "supportedGenerationMethods": methods, "thinking": true})
        };
        let gen = ["generateContent", "countTokens"];
        let page = json!({"models": [
            m("gemini-3.8-flash", "Gemini 3.8 Flash", &gen),
            m("gemini-4-argon", "Gemini 4 Argon", &gen),
            m("gemini-flash-latest", "Gemini Flash Latest", &gen),
            m("gemini-3.1-pro-preview", "Gemini 3.1 Pro Preview", &gen),
            m("gemini-3.1-pro-preview-customtools", "Gemini 3.1 Pro Preview Custom Tools", &gen),
            m("gemini-2.5-flash-preview-tts", "Gemini 2.5 Flash Preview TTS", &gen),
            m("gemini-2.5-flash-image", "Nano Banana", &gen),
            m("gemini-nano-banana-2.1", "Nano Banana 2.1", &gen),
            m("gemini-omni-1.1-flash", "Gemini Omni 1.1 Flash", &gen),
            m("gemini-3.5-transcribe", "Gemini 3.5 Transcribe", &gen),
            m("lyria-3.5", "Lyria 3.5", &gen),
            m("gemini-robotics-er-2-preview", "Gemini Robotics-ER 2 Preview", &gen),
            m("gemini-2.5-computer-use-preview-10-2025", "Computer Use", &gen),
            m("antigravity-preview-09-2026", "Antigravity Agent Preview", &gen),
            m("deep-research-preview-04-2026", "Deep Research", &gen),
            m("gemini-embedding-2", "Gemini Embedding 2", &["embedContent"]),
            m("aqa", "Attributed QA", &["generateAnswer"]),
            m("veo-3.1-generate-preview", "Veo 3.1", &["predictLongRunning"]),
            m("gemini-3.8-live", "Gemini 3.8 Live", &["bidiGenerateContent"]),
            m("gemini-2.5-flash-native-audio-latest", "Native Audio", &["countTokens", "bidiGenerateContent"]),
            m("gemma-4-31b-it", "Gemma 4 31B IT", &gen),
        ], "nextPageToken": ""});
        let (listed, kept) = parse_gemini(&[page]);
        assert_eq!(listed, 21);
        assert_eq!(
            ids(&kept),
            [
                "gemini-3.8-flash",
                "gemini-4-argon",
                "gemini-3.1-pro-preview",
                "gemma-4-31b-it"
            ]
        );
        assert_eq!(kept[1].name, "Gemini 4 Argon");
        // A description that only repeats the name is dropped.
        assert_eq!(kept[1].desc, "");
        assert_eq!(kept[0].thinks, Some(true));
        assert_eq!(kept[0].context, Some(1048576));
    }

    /// Snip to Chat: which models read images, where the lists say it.
    #[test]
    fn the_lists_say_which_models_read_images() {
        let groq = json!({"data": [
            {"id": "openai/gpt-oss-20b", "active": true, "input_modalities": ["text"], "output_modalities": ["text"]},
            {"id": "qwen/qwen3.8-27b", "active": true, "input_modalities": ["text", "image"], "output_modalities": ["text"]},
            {"id": "example-lab/new-model-9b", "active": true}
        ]});
        let (_, models) = parse_groq(&groq);
        let vision = |id: &str| models.iter().find(|m| m.id == id).unwrap().vision;
        assert_eq!(vision("openai/gpt-oss-20b"), Some(false));
        assert_eq!(vision("qwen/qwen3.8-27b"), Some(true));
        assert_eq!(vision("example-lab/new-model-9b"), None, "the list didn't say");

        let anthropic = json!({"data": [
            {"id": "claude-haiku-5-5", "display_name": "Claude Haiku 5.5", "capabilities": {"image_input": {"supported": true}}},
            {"id": "claude-text-1", "display_name": "Claude Text", "capabilities": {"image_input": {"supported": false}}},
            {"id": "claude-old-2", "display_name": "Claude Old"}
        ], "has_more": false});
        let (_, models) = parse_anthropic(&[anthropic]);
        let vision = |id: &str| models.iter().find(|m| m.id == id).unwrap().vision;
        assert_eq!((vision("claude-haiku-5-5"), vision("claude-text-1"), vision("claude-old-2")), (Some(true), Some(false), None));

        let openrouter = json!({"data": [
            {"id": "openai/gpt-5-mini", "name": "OpenAI: GPT-5 Mini", "architecture": {"input_modalities": ["text", "image"], "output_modalities": ["text"]}},
            {"id": "meta-llama/llama-3.3-70b-instruct", "name": "Llama", "architecture": {"input_modalities": ["text"], "output_modalities": ["text"]}}
        ]});
        let (_, models) = parse_openrouter(&[openrouter]);
        assert_eq!(models[0].vision, Some(true));
        assert_eq!(models[1].vision, Some(false));

        // Looked up by id, a dated snapshot's alias included.
        set_models_for_test("groq", parse_groq(&groq).1);
        assert_eq!(vision_for("groq", "qwen/qwen3.8-27b"), Some(true));
        assert_eq!(vision_for("groq", "nope"), None);
        assert_eq!(vision_for("gemini", "gemini-3.6-flash"), None, "no list");
    }

    #[test]
    fn openrouter_says_tools_and_leaves_out_image_models() {
        let page = json!({"data": [
            {"id": "openai/gpt-5-mini", "name": "OpenAI: GPT-5 Mini", "created": 1754587413,
             "description": "GPT-5 Mini is a compact version of GPT-5. It is designed for lighter-weight [reasoning](https://x.example) tasks.",
             "context_length": 400000, "architecture": {"input_modalities": ["text", "image"], "output_modalities": ["text"]},
             "supported_parameters": ["tools", "tool_choice", "reasoning", "max_tokens"]},
            {"id": "acme/tiny-chat:free", "name": "Acme: Tiny Chat (free)", "created": 1790000000, "context_length": 8192,
             "architecture": {"input_modalities": ["text"], "output_modalities": ["text"]}, "supported_parameters": ["max_tokens", "temperature"]},
            {"id": "google/gemini-2.5-flash-image", "name": "Google: Nano Banana",
             "architecture": {"input_modalities": ["text", "image"], "output_modalities": ["image", "text"]}},
            {"id": "~openai/gpt-latest", "name": "GPT Latest", "architecture": {"input_modalities": ["text"], "output_modalities": ["text"]}},
            {"id": "old/retiring-1", "name": "Old: Retiring 1", "expiration_date": "2026-11-01",
             "architecture": {"input_modalities": ["text"], "output_modalities": ["text"]}}
        ], "links": {"next": null}});
        let (listed, kept) = parse_openrouter(&[page]);
        assert_eq!(listed, 5);
        assert_eq!(
            ids(&kept),
            ["openai/gpt-5-mini", "acme/tiny-chat:free", "old/retiring-1"]
        );
        assert_eq!(kept[0].name, "GPT-5 Mini");
        assert_eq!(kept[0].desc, "GPT-5 Mini is a compact version of GPT-5");
        assert_eq!((kept[0].tools, kept[0].thinks), (Some(true), Some(true)));
        assert_eq!(
            (kept[1].tools, kept[1].name.as_str()),
            (Some(false), "Tiny Chat (free)")
        );
        assert!(kept[2].deprecated);
    }

    /// Effort (ai/effort.rs): the levels a model takes, where its provider's
    /// list says (Anthropic's capabilities, OpenRouter's reasoning object, in
    /// the shapes their docs give, read 31 Oct 2026).
    #[test]
    fn the_lists_say_which_effort_levels_a_model_takes() {
        let yes = json!({ "supported": true });
        let no = json!({ "supported": false });
        let anthropic = json!({"data": [
            {"id": "claude-opus-5-5", "display_name": "Claude Opus 5.5", "capabilities": {"effort": {"supported": true, "low": yes, "medium": yes, "high": yes, "xhigh": yes, "max": yes}}},
            {"id": "claude-opus-4-6", "display_name": "Claude Opus 4.6", "capabilities": {"effort": {"supported": true, "low": yes, "medium": yes, "high": yes, "xhigh": no, "max": yes}}},
            {"id": "claude-haiku-4-5-20251001", "display_name": "Claude Haiku 4.5", "capabilities": {"effort": {"supported": false}}},
            {"id": "claude-old-2", "display_name": "Claude Old"}
        ], "has_more": false});
        let (_, models) = parse_anthropic(&[anthropic]);
        let efforts = |id: &str| models.iter().find(|m| m.id == id).unwrap().efforts.clone();
        assert_eq!(efforts("claude-opus-5-5"), ["low", "medium", "high", "xhigh", "max"]);
        assert_eq!(efforts("claude-opus-4-6"), ["low", "medium", "high", "max"]);
        assert!(efforts("claude-haiku-4-5-20251001").is_empty());
        assert!(efforts("claude-old-2").is_empty(), "the list didn't say");

        let openrouter = json!({"data": [
            {"id": "openai/gpt-5-mini", "name": "GPT-5 Mini", "architecture": {"input_modalities": ["text"], "output_modalities": ["text"]},
             "reasoning": {"mandatory": true, "supported_efforts": ["high", "medium", "low", "minimal"], "default_effort": "medium"}},
            {"id": "anthropic/claude-opus-5.5", "name": "Claude Opus 5.5", "architecture": {"input_modalities": ["text"], "output_modalities": ["text"]},
             "reasoning": {"mandatory": true, "supported_efforts": null}},
            {"id": "meta-llama/llama-3.3-70b-instruct", "name": "Llama", "architecture": {"input_modalities": ["text"], "output_modalities": ["text"]}}
        ]});
        let (_, models) = parse_openrouter(&[openrouter]);
        assert_eq!(models[0].efforts, ["low", "medium", "high"], "lowest first, \"minimal\" left out");
        assert_eq!(models[0].default_effort.as_deref(), Some("medium"));
        assert!(models[1].efforts.is_empty(), "any effort: Yap's own table decides");
        assert!(models[2].efforts.is_empty() && models[2].default_effort.is_none());
    }

    #[test]
    fn self_hosted_lists_read_data_or_models() {
        let lm = json!({"data": [{"id": "qwen2.5-7b-instruct"}, {"id": "text-embedding-nomic-embed-text-v1.5"}]});
        assert_eq!(ids(&parse_compatible(&lm).1), ["qwen2.5-7b-instruct"]);
        let other = json!({"models": [{"name": "llama3.1:8b"}, {"name": "nomic-embed-text"}, {"model": "phi4"}]});
        assert_eq!(ids(&parse_compatible(&other).1), ["llama3.1:8b", "phi4"]);
    }

    #[test]
    fn chat_capable_by_name() {
        for id in [
            "gpt-5.5",
            "claude-haiku-5-5",
            "gemini-3.6-flash",
            "openai/gpt-oss-20b",
            "qwen/qwen3.8-27b",
            "o4-mini",
            "llama3.1:8b",
            "meta-llama/llama-3.2-11b-vision-instruct",
        ] {
            assert!(chat_capable(id), "{id}");
        }
        for id in [
            "whisper-large-v3",
            "gpt-4o-mini-tts",
            "text-embedding-3-large",
            "gemini-embedding-001",
            "omni-moderation-latest",
            "meta-llama/llama-guard-4-12b",
            "gpt-image-1",
            "dall-e-3",
            "imagen-4.0-generate-001",
            "veo-3.1-generate-preview",
            "gpt-4o-realtime-preview",
            "gemini-3.8-live",
            "gpt-4o-search-preview",
            "gemini-2.5-computer-use-preview-10-2025",
            "o3-deep-research",
            "gpt-5-codex",
            "aqa",
            "lyria-3.5",
            "gemini-2.5-flash-native-audio-latest",
        ] {
            assert!(!chat_capable(id), "{id}");
        }
    }

    #[test]
    fn snapshots_fold_into_their_alias() {
        assert_eq!(
            snapshot_base("gpt-4.1-2025-04-14").as_deref(),
            Some("gpt-4.1")
        );
        assert_eq!(
            snapshot_base("claude-haiku-4-5-20251001").as_deref(),
            Some("claude-haiku-4-5")
        );
        assert_eq!(
            snapshot_base("gemini-2.0-flash-001").as_deref(),
            Some("gemini-2.0-flash")
        );
        assert_eq!(
            snapshot_base("moonshotai/kimi-k2-instruct-0905").as_deref(),
            Some("moonshotai/kimi-k2-instruct")
        );
        assert_eq!(
            snapshot_base("gemini-2.5-flash-preview-05-20").as_deref(),
            Some("gemini-2.5-flash-preview")
        );
        for id in [
            "gpt-oss-120b",
            "gpt-5.5",
            "claude-opus-4-6",
            "gemma-4-31b-it",
            "llama-3.1-405b",
            "o3",
        ] {
            assert_eq!(snapshot_base(id), None, "{id}");
        }
        let m = |id: &str| CatalogModel {
            id: id.into(),
            ..Default::default()
        };
        let folded = fold_snapshots(vec![
            m("gpt-4o-2024-08-06"),
            m("gpt-4o"),
            m("gpt-4o-2024-11-20"),
            m("claude-x-20250101"),
        ]);
        assert_eq!(ids(&folded), ["gpt-4o", "claude-x-20250101"]);
        assert_eq!(
            folded[0].aliases,
            ["gpt-4o-2024-08-06", "gpt-4o-2024-11-20"]
        );
        assert_eq!(folded[1].base.as_deref(), Some("claude-x"));
    }

    #[test]
    fn names_from_ids() {
        assert_eq!(derive_name("gpt-5.5"), "GPT-5.5");
        assert_eq!(derive_name("openai/gpt-oss-20b"), "GPT-OSS 20B");
        assert_eq!(derive_name("gpt-4o-mini"), "GPT-4o Mini");
        assert_eq!(derive_name("gpt-6.1-sol"), "GPT-6.1 Sol");
        assert_eq!(derive_name("o4-mini"), "o4-mini");
        assert_eq!(
            derive_name("llama-4-scout-17b-16e-instruct"),
            "Llama 4 Scout 17B 16E Instruct"
        );
        assert_eq!(
            derive_name("moonshotai/kimi-k2-instruct-0905"),
            "Kimi K2 Instruct 0905"
        );
        assert_eq!(derive_name("gemma-4-26b-a4b-it"), "Gemma 4 26B A4B IT");
        // A version split by hyphens joins up; a date stays apart.
        assert_eq!(derive_name("claude-haiku-5-5"), "Claude Haiku 5.5");
        assert_eq!(derive_name("claude-3-5-sonnet-20241022"), "Claude 3.5 Sonnet 20241022");
        assert_eq!(derive_name("claude-opus-5"), "Claude Opus 5");
        assert_eq!(derive_name("chatgpt-4o-latest"), "ChatGPT-4o Latest");
        assert_eq!(clean_name("OpenAI: GPT-5 Mini"), "GPT-5 Mini");
        assert_eq!(clean_name("Qwen/Qwen3.8-27B"), "Qwen3.8-27B");
        assert_eq!(clean_name("Gemini 3.8 Flash"), "Gemini 3.8 Flash");
    }

    #[test]
    fn a_list_is_checked_daily_and_a_failure_hourly() {
        let now = 2_000_000_000;
        assert!(due(None, now));
        let good = ProviderList {
            fetched_at: now - 3600,
            checked_at: now - 3600,
            ..Default::default()
        };
        assert!(!due(Some(&good), now));
        assert!(due(
            Some(&ProviderList {
                fetched_at: now - FRESH_FOR,
                ..good.clone()
            }),
            now
        ));
        let failed = ProviderList {
            checked_at: now - 600,
            error: Some("x".into()),
            ..good.clone()
        };
        assert!(!due(Some(&failed), now));
        assert!(due(
            Some(&ProviderList {
                checked_at: now - RETRY_AFTER,
                ..failed
            }),
            now
        ));
        // The clock went back.
        assert!(due(
            Some(&ProviderList {
                fetched_at: now + 10,
                checked_at: now + 10,
                ..good
            }),
            now
        ));
    }

    #[test]
    fn first_seen_is_kept_and_failures_keep_the_last_good_list() {
        let m = |id: &str| CatalogModel {
            id: id.into(),
            ..Default::default()
        };
        record("groq", Ok(vec![m("a"), m("b")]), 100);
        record("groq", Ok(vec![m("b"), m("c")]), 200);
        let list = with_store(|c| c.providers["groq"].clone()).unwrap();
        assert_eq!(
            list.models
                .iter()
                .map(|m| (m.id.as_str(), m.first_seen))
                .collect::<Vec<_>>(),
            [("b", 100), ("c", 200)]
        );
        record("groq", Err("the API key wasn't accepted".into()), 300);
        let list = with_store(|c| c.providers["groq"].clone()).unwrap();
        assert_eq!((list.fetched_at, list.checked_at), (200, 300));
        assert_eq!(list.models.len(), 2);
        // The page gets no list while the last check failed: the curated one shows.
        let v = view("groq", Some(&list));
        assert!(!v.ok && v.models.is_empty());
        assert_eq!(v.error.as_deref(), Some("the API key wasn't accepted"));
    }

    #[test]
    fn tools_only_when_the_provider_said_so() {
        let m = |id: &str, tools: Option<bool>| CatalogModel {
            id: id.into(),
            tools,
            ..Default::default()
        };
        record(
            "openrouter",
            Ok(vec![
                m("acme/tiny", Some(false)),
                m("openai/gpt-5-mini", Some(true)),
                m("x/y", None),
            ]),
            100,
        );
        assert_eq!(tools_for("openrouter", "acme/tiny"), Some(false));
        assert_eq!(tools_for("openrouter", "openai/gpt-5-mini"), Some(true));
        assert_eq!(tools_for("openrouter", "x/y"), None);
        assert_eq!(tools_for("groq", "acme/tiny"), None);
        // Yap's note tools follow it: a cloud model keeps them unless its
        // provider said no.
        assert!(!crate::chat::tools::supports_tools("openrouter", "acme/tiny"));
        assert!(crate::chat::tools::supports_tools("openrouter", "openai/gpt-5-mini"));
        assert!(crate::chat::tools::supports_tools("openrouter", "x/y"));
        assert!(crate::chat::tools::supports_tools("groq", "acme/tiny"));
        // A failed last check says nothing.
        record("openrouter", Err("offline".into()), 200);
        assert!(crate::chat::tools::supports_tools("openrouter", "acme/tiny"));
    }

    #[test]
    fn keys_come_from_the_shared_store_scopes_or_profiles() {
        let mut cfg = YapConfig {
            pp_provider: "groq".into(),
            ..Default::default()
        };
        assert!(keyed(&cfg).is_empty());
        cfg.pp_api_keys.insert("groq".into(), "gsk".into());
        cfg.llm_scopes.insert(
            "chat".into(),
            crate::config::LlmScope {
                provider: "gemini".into(),
                api_key: "gem".into(),
                ..Default::default()
            },
        );
        assert_eq!(keyed(&cfg), ["groq", "gemini"]);
        assert_eq!(key_for(&cfg, "gemini", None), "gem");
        assert_eq!(key_for(&cfg, "groq", Some("chat")), "gsk");
        assert_eq!(endpoint_key(&cfg, Some("chat"), "gemini"), "gem");
    }

    #[test]
    fn standin_routes_every_provider_through_one_server() {
        let ep = Endpoints {
            standin: Some("http://127.0.0.1:9/models-standin".into()),
        };
        let u = ep
            .route(&real(
                "https://generativelanguage.googleapis.com/v1beta/models?pageSize=1000",
            ))
            .unwrap();
        assert_eq!(u.as_str(), "http://127.0.0.1:9/models-standin/generativelanguage.googleapis.com/v1beta/models?pageSize=1000");
        let real_ep = Endpoints { standin: None };
        assert_eq!(
            real_ep
                .route(&real("https://api.groq.com/openai/v1/models"))
                .unwrap()
                .host_str(),
            Some("api.groq.com")
        );
    }

    /// The parsers over the real lists saved by a read-only check
    /// (`YAP_MODEL_LISTS=<dir>` with groq.json / gemini.json / …): counts
    /// and names, for a look by hand. Never fetches.
    #[test]
    #[ignore]
    fn real_saved_lists() {
        let Ok(dir) = std::env::var("YAP_MODEL_LISTS") else {
            return;
        };
        let read = |f: &str| {
            std::fs::read_to_string(std::path::Path::new(&dir).join(f))
                .ok()
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        };
        let show = |p: &str, (listed, kept): (usize, Vec<CatalogModel>)| {
            println!("{p}: {listed} listed, {} kept", kept.len());
            for m in kept {
                println!(
                    "  {} | {} | created {:?} | tools {:?} | thinks {:?} | {}",
                    m.id, m.name, m.created, m.tools, m.thinks, m.desc
                );
            }
        };
        if let Some(v) = read("groq.json") {
            show("groq", parse_groq(&v));
        }
        if let Some(v) = read("gemini.json") {
            show("gemini", parse_gemini(&[v]));
        }
        if let Some(v) = read("openai.json") {
            show("openai", parse_openai(&v));
        }
        if let Some(v) = read("anthropic.json") {
            show("anthropic", parse_anthropic(&[v]));
        }
        if let Some(v) = read("openrouter.json") {
            show("openrouter", parse_openrouter(&[v]));
        }
    }

    #[test]
    fn self_hosted_servers_say_their_models_context() {
        assert_eq!(server_root("http://localhost:11434/v1"), "http://localhost:11434");
        assert_eq!(server_root("http://127.0.0.1:1234/v1/"), "http://127.0.0.1:1234");
        assert_eq!(server_root("http://box:8080"), "http://box:8080");
        // LM Studio's REST list (trimmed from 0.3.x): the maximum, and the
        // loaded model's context.
        let lms = json!({"object": "list", "data": [
            {"id": "qwen2.5-7b-instruct", "object": "model", "type": "llm", "state": "loaded",
             "max_context_length": 32768, "loaded_context_length": 4096},
            {"id": "llama-3.2-3b-instruct", "type": "llm", "state": "not-loaded", "max_context_length": 131072},
            {"id": "text-embedding-nomic", "type": "embeddings"}
        ]});
        let c = parse_lmstudio(&lms);
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].max, c[0].set, c[0].app), (Some(32_768), Some(4_096), "LM Studio"));
        assert_eq!((c[1].max, c[1].set), (Some(131_072), None));
        // An OpenAI-style list (any other server) says nothing.
        assert!(parse_lmstudio(&json!({"data": [{"id": "fake-chat", "object": "model"}]})).is_empty());
        // Ollama: the loaded models' context, the trained one and num_ctx.
        let ps = json!({"models": [{"name": "llama3.1:8b", "model": "llama3.1:8b", "size_vram": 6_000_000_000u64, "context_length": 8192}]});
        assert_eq!(parse_ollama_ps(&ps).unwrap().get("llama3.1:8b"), Some(&8_192));
        assert!(parse_ollama_ps(&json!({"error": "not found"})).is_none());
        let show = json!({
            "parameters": "stop                           \"<|eot_id|>\"\nnum_ctx                        16384",
            "model_info": {"general.architecture": "llama", "llama.context_length": 131072, "llama.block_count": 32}
        });
        assert_eq!(parse_ollama_show(&show), (Some(131_072), Some(16_384)));
        assert_eq!(parse_ollama_show(&json!({"model_info": {"qwen2.context_length": 32768}})), (Some(32_768), None));
        assert_eq!(parse_ollama_show(&json!({})), (None, None));
        // llama.cpp's own server (a llamafile): the context it serves.
        assert_eq!(parse_llamacpp_props(&json!({"default_generation_settings": {"n_ctx": 8192}, "total_slots": 1})), Some(8_192));
        assert_eq!(parse_llamacpp_props(&json!({"n_ctx": 32768})), Some(32_768));
        assert_eq!(parse_llamacpp_props(&json!({"models": []})), None);
    }
}

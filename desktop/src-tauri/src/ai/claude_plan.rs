//! **Chat on your Claude plan** (2026-11-07): Chat's and the Voice
//! Assistant's answers from Claude models on the person's own Claude
//! subscription, through their own installed Claude Code (unmodified,
//! signed in with their own account: `ai/claude_code/`). Beside the
//! Anthropic API key, as the ChatGPT plan ("chatgpt", `ai/chatgpt.rs`) sits
//! beside an OpenAI key. Yap never reads Claude credentials and never calls
//! Anthropic itself with a plan: Claude Code does, on the person's own
//! sign-in (Anthropic's terms for running Claude Code in a product,
//! code.claude.com/docs/en/legal-and-compliance, read 7 Nov 2026).
//!
//! - **The provider** "claude-plan": a keyed provider whose key store holds
//!   [`PLAN_KEY`] while Claude Code is found on this PC, new enough and
//!   signed in ([`sync`]) — a marker, never a credential, so every "has
//!   this provider a key?" check works as for the others. Its base URL is a
//!   placeholder ([`BASE_URL`]): nothing is ever sent to it.
//! - **Its models** ([`MODELS`]): a curated list of full ids (the CLI lists
//!   none), so every rule keyed on a model id — effort, images, the strong
//!   models, Latest — reads them as Anthropic's ([`as_api`]).
//! - **Where** (7 Nov 2026: every scope): Chat & Voice Assistant, and
//!   Dictation Cleanup, Commands and Note Formatting ([`Scope`]). Each
//!   scope's endpoint names its scope in the placeholder URL
//!   ([`scope_url`]), so a call knows whose settings it runs under: its
//!   warm spare, its effort, its fallback to the key.
//! - **A Chat turn** (`chat/turn/plan.rs`): one Claude Code session per
//!   conversation, kept warm 10 minutes ([`sessions`]), its events mapped
//!   into Chat's stream ([`map`]).
//! - **Every other call** (a dictation's cleanup, a Command, a meeting's
//!   summary, memory learning, notes' Ask, Think, …): one question, one
//!   answer ([`oneshot`]), from `llm::post_chat_request` — for Dictation
//!   Cleanup, Commands and Note Formatting on a **warm spare** started
//!   before the call ([`spare`]), so Claude Code's own start-up is out of
//!   the dictation's way.
//! - **If the plan can't answer** (signed out, its limit, paying another
//!   way, Claude Code won't start): the scope's opt-in "use my API key"
//!   sends that call once to the Anthropic key ([`fallback_key`]); else the
//!   call fails as any does (cleanup: the words as said).
//! - **Plan usage limits** ([`usage`]): where the plan's limits stand
//!   (Claude Code's own `get_usage`), for Chat's usage ring and Settings'
//!   card; kept in memory only.
//!
//! How it works: docs/architecture/ai-models.md, "Your Claude plan".

pub mod map;
pub mod oneshot;
pub mod sessions;
pub mod spare;
pub mod usage;

use std::sync::OnceLock;

use serde::Serialize;
use tauri::AppHandle;

use crate::ai::claude_code::cli;

/// The provider id (config.json, the key store, Chat's picker).
pub const PROVIDER: &str = "claude-plan";
/// What the key store holds for [`PROVIDER`] while Claude Code is signed
/// in here: a marker, never a credential.
pub const PLAN_KEY: &str = "claude-plan";
/// A base URL for the scope (an empty one means "no model set up"):
/// nothing is ever sent to it.
pub const BASE_URL: &str = "claude-code://plan";
/// Where the person sees their plan's usage (Manage usage; the usage
/// ring's See all).
pub const USAGE_URL: &str = "https://claude.ai/settings/usage";
/// How to install Claude Code.
pub const INSTALL_URL: &str = "https://code.claude.com/docs/en/setup";
/// Chat on the plan with Claude Code not ready here (not found, too old, or
/// signed out): where to fix it. The page offers Sign in with it.
pub const NOT_READY: &str =
    "Claude Code isn't signed in on this PC. Sign in to Claude Code in Settings → Language Models → Chat & Voice Assistant → Claude, then try again.";

/// **Which scope a plan call runs under** (its settings: the spare, the
/// effort, the fallback to the key). The endpoint's placeholder URL says it
/// ([`scope_url`]); a bare [`BASE_URL`] is Chat's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    /// Dictation Cleanup (`pp_*`; also a question cleaned up, a cleanup
    /// style's or an app rule's own model, and the calls that fall back to
    /// the cleanup's endpoint).
    Cleanup,
    /// Commands (`llm_scopes["voiceAgent"]`: Commands, the rewrite
    /// shortcut, saved transforms).
    Commands,
    /// Note Formatting (`llm_scopes["noteFormatting"]`: meeting summaries,
    /// the notepad's helpers).
    Notes,
    /// Chat & Voice Assistant (`llm_scopes["chat"]`), unchanged: its turns on
    /// warm sessions, its one-off calls cold, no fallback.
    Chat,
}

impl Scope {
    pub const ALL: [Scope; 4] = [Scope::Cleanup, Scope::Commands, Scope::Notes, Scope::Chat];

    pub fn id(self) -> &'static str {
        match self {
            Scope::Cleanup => "cleanup",
            Scope::Commands => "commands",
            Scope::Notes => "notes",
            Scope::Chat => "chat",
        }
    }

    /// Its key in `llm_scopes` (`None`: the cleanup's `pp_*` fields).
    pub fn key(self) -> Option<&'static str> {
        match self {
            Scope::Cleanup => None,
            Scope::Commands => Some("voiceAgent"),
            Scope::Notes => Some("noteFormatting"),
            Scope::Chat => Some("chat"),
        }
    }
}

/// **The endpoint's placeholder URL for `scope`**: `claude-code://plan/<id>`
/// (Chat's stays [`BASE_URL`]). Nothing is ever sent to it. Pure.
pub fn scope_url(scope: Scope) -> String {
    match scope {
        Scope::Chat => BASE_URL.to_string(),
        s => format!("{BASE_URL}/{}", s.id()),
    }
}

/// The scope a plan call's URL names ([`scope_url`]); anything else is
/// Chat's. Pure.
pub fn scope_of_url(url: &str) -> Scope {
    match url.trim_end_matches('/').strip_prefix(BASE_URL).map(|r| r.trim_start_matches('/')) {
        Some("cleanup") => Scope::Cleanup,
        Some("commands") => Scope::Commands,
        Some("notes") => Scope::Notes,
        _ => Scope::Chat,
    }
}

/// A resolved endpoint's URL, named for `scope` when it's the plan's
/// ([`scope_url`]); any other provider's as it is. Pure.
pub fn tag(provider: &str, base_url: String, scope: Scope) -> String {
    if provider == PROVIDER {
        scope_url(scope)
    } else {
        base_url
    }
}

/// The Anthropic API's OpenAI-compatible address, where a plan call the
/// plan can't answer goes when its scope says so (a test run's stand-in in
/// debug builds: `YAP_E2E_ANTHROPIC_API`).
pub fn anthropic_url() -> String {
    let url = crate::ai::ai_model::standard_base_url("anthropic").unwrap_or("https://api.anthropic.com/v1").to_string();
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() {
        if let Ok(u) = std::env::var("YAP_E2E_ANTHROPIC_API") {
            if !u.trim().is_empty() {
                return u;
            }
        }
    }
    url
}

/// **A resolved endpoint `(base_url, key, model, provider)` for `scope`**:
/// on the plan, named for the scope ([`tag`]) — or, with Claude Code not
/// ready here (no marker: signed out, missing, too old) and the scope's
/// "use my API key" on ([`fallback_key`]), the Anthropic key on the same
/// model at once, so nothing waits on a plan that can't answer and every
/// "has it a key?" check passes. Any other provider's as it is. Pure but
/// for a test run's address.
pub fn resolve(cfg: &crate::config::YapConfig, scope: Scope, ep: (String, String, String, String)) -> (String, String, String, String) {
    let (base_url, key, model, provider) = ep;
    if provider != PROVIDER {
        return (base_url, key, model, provider);
    }
    if key.trim().is_empty() {
        if let Some(k) = fallback_key(cfg, scope) {
            return (anthropic_url(), k, model, "anthropic".to_string());
        }
    }
    (scope_url(scope), key, model, provider)
}

/// The model `scope` runs on when it's on the plan in `cfg` ([`on_plan`]):
/// its own. Pure.
pub fn scope_model(cfg: &crate::config::YapConfig, scope: Scope) -> Option<String> {
    if !on_plan(cfg, scope) {
        return None;
    }
    let model = match scope.key() {
        None => cfg.pp_model.clone(),
        Some(k) => cfg.llm_scopes.get(k).map(|s| s.model.clone()).unwrap_or_default(),
    };
    Some(model).filter(|m| !m.trim().is_empty())
}

/// Is `scope` on the plan in `cfg` (its own setting: a scope that's off
/// isn't, even when the cleanup it falls back to is)? Dictation Cleanup
/// counts while cleanup is on. Pure.
pub fn on_plan(cfg: &crate::config::YapConfig, scope: Scope) -> bool {
    match scope.key() {
        None => cfg.post_process_enabled && cfg.pp_provider == PROVIDER,
        Some(k) => cfg.llm_scopes.get(k).is_some_and(|s| s.enabled && s.provider == PROVIDER),
    }
}

/// **The Anthropic key a plan call of `scope` may fall back to**: the
/// scope's "If your plan can't answer, use my API key" on (never Chat's:
/// it has "Use my API key for this chat") and a key saved. Pure.
pub fn fallback_key(cfg: &crate::config::YapConfig, scope: Scope) -> Option<String> {
    let on = match scope {
        Scope::Chat => false,
        Scope::Cleanup => cfg.pp_plan_fallback,
        s => s.key().and_then(|k| cfg.llm_scopes.get(k)).is_some_and(|x| x.plan_fallback),
    };
    if !on {
        return None;
    }
    Some(cfg.provider_api_key("anthropic", "")).filter(|k| !k.trim().is_empty())
}

/// The models a plan chat offers, by their full ids (never aliases: the
/// effort, images and strong-model rules key on full ids), cheapest first:
/// the newest of each family. Fable 5.2 took Fable 5.1's place on 10 Nov
/// 2026: a choice of 5.1 saved on the plan follows it (`config::
/// migrate_plan_models`), and the pickers' stored list is replaced by this
/// one at once, never a day late (`model_catalog::keep_own_lists`).
pub const MODELS: &[(&str, &str)] = &[
    ("claude-haiku-5-5", "Claude Haiku 5.5"),
    ("claude-sonnet-5-5", "Claude Sonnet 5.5"),
    ("claude-opus-5-5", "Claude Opus 5.5"),
    ("claude-fable-5-2", "Claude Fable 5.2"),
];

/// **The plan's models are Anthropic's, the ChatGPT plan's OpenAI's**:
/// every rule about a model (effort, images, context, the strong models)
/// reads "claude-plan" as "anthropic" and "chatgpt" as "openai". Pure.
pub fn as_api(provider: &str) -> &str {
    match provider {
        PROVIDER => "anthropic",
        p => crate::ai::chatgpt::as_openai(p),
    }
}

/// Whether Claude Code here can answer on the plan: installed, new enough,
/// signed in. Pure.
pub fn ready(s: &cli::Status) -> bool {
    s.installed && !s.too_old && s.logged_in
}

/// Signed in with something that isn't a Claude plan (a Console API key's
/// login): its runs wouldn't use the plan. Pure.
pub fn on_a_key(s: &cli::Status) -> bool {
    s.logged_in && s.auth_method.as_deref().is_some_and(|m| matches!(m.to_ascii_lowercase().as_str(), "console" | "api_key" | "apikey" | "api key"))
}

/// One of the plan's models, for the page.
#[derive(Debug, Clone, Serialize)]
pub struct Model {
    pub id: &'static str,
    pub name: &'static str,
}

/// What Settings' card and Chat's picker show.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    #[serde(flatten)]
    pub code: cli::Status,
    /// Answers can come on the plan now.
    pub ready: bool,
    /// Signed in with an API key, not a plan.
    pub on_a_key: bool,
    pub usage_url: &'static str,
    pub install_url: &'static str,
    pub models: Vec<Model>,
}

impl Status {
    pub fn of(code: cli::Status) -> Status {
        Status {
            ready: ready(&code),
            on_a_key: on_a_key(&code),
            code,
            usage_url: USAGE_URL,
            install_url: INSTALL_URL,
            models: MODELS.iter().map(|(id, name)| Model { id, name }).collect(),
        }
    }
}

static APP: OnceLock<AppHandle> = OnceLock::new();

/// lib.rs setup: the app for the key store's marker, the warm sessions'
/// sweep, and — only for someone already on the plan (its marker set, or
/// Chat on it) — a look at Claude Code in the background, so a sign-out
/// since is noticed. Nobody else's Claude Code is started by Yap on its own.
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
    sessions::start_sweeping();
    spare::start_sweeping();
    let cfg = crate::config::load();
    let on_plan = cfg.pp_api_keys.get(PROVIDER).is_some_and(|k| k == PLAN_KEY)
        || cfg.llm_scopes.get("chat").is_some_and(|s| s.provider == PROVIDER)
        || Scope::ALL.iter().any(|s| on_plan(&cfg, *s));
    if on_plan {
        std::thread::spawn(|| sync(&cli::status()));
    }
}

/// The key store's marker as Claude Code is now: on while it's ready, off
/// once it isn't (signed out, removed, too old). Every window's copy
/// follows (`yap-config-saved`). Nothing saved when it's already so.
pub fn sync(s: &cli::Status) {
    let Some(app) = APP.get() else { return };
    let on = ready(s);
    let cfg = crate::config::load();
    let has = cfg.pp_api_keys.get(PROVIDER).is_some_and(|k| k == PLAN_KEY);
    if on == has {
        return;
    }
    tracing::info!(on, "Claude plan: the key store's marker follows Claude Code");
    if !on {
        usage::clear();
        // Signed out: no spare waits on a plan that can't answer.
        spare::end_all("signed out");
    }
    let _ = crate::shell::settings::change_config(app, |cfg| {
        if on {
            crate::ai::ai_model::set_provider_key(cfg, PROVIDER, PLAN_KEY)
        } else {
            crate::ai::ai_model::clear_provider_key(cfg, PROVIDER);
            Ok(())
        }
    });
}

/// **Claude Code on this PC, for the plan** (Settings' card, Chat's
/// picker): found, its version, signed in; the key store's marker made to
/// match. `refresh`: probe again now (the window got the focus back after a
/// sign-in), else at most 30 s old.
#[tauri::command]
pub async fn claude_plan_status(refresh: Option<bool>) -> Status {
    let refresh = refresh.unwrap_or(false);
    let code = tauri::async_runtime::spawn_blocking(move || if refresh { cli::refresh() } else { cli::status() })
        .await
        .unwrap_or_else(|_| cli::Status { min_version: cli::MIN_VERSION.to_string(), error: Some("Couldn't check Claude Code.".into()), ..Default::default() });
    let status = Status::of(code);
    let code = status.code.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || sync(&code)).await;
    status
}

/// **Sign in**: Claude Code's own sign-in (`claude auth login`) in a
/// terminal the person owns. The card asks again when the window gets the
/// focus back.
#[tauri::command]
pub async fn claude_plan_sign_in() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(cli::open_sign_in).await.unwrap_or_else(|_| Err("Couldn't open the sign-in.".into()))
}

/// The plan's curated models, as the model catalogue lists a provider's
/// (`model_catalog.rs`: the CLI has no list of its own).
pub fn catalog() -> Vec<(String, String)> {
    MODELS.iter().map(|(id, name)| (id.to_string(), name.to_string())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_plans_models_are_anthropics_for_every_model_rule() {
        assert_eq!(as_api(PROVIDER), "anthropic");
        assert_eq!(as_api("chatgpt"), "openai");
        assert_eq!(as_api("groq"), "groq");
        // Full ids only, never aliases.
        assert!(MODELS.iter().all(|(id, _)| id.starts_with("claude-") && id.matches('-').count() >= 3));
        // Effort, images and the strong models read them as Anthropic's.
        assert!(crate::ai::effort::caps(PROVIDER, "claude-opus-5-5").is_some());
        // Every model it offers has its effort levels (a new one isn't left out of the table).
        assert!(MODELS.iter().all(|(id, _)| crate::ai::effort::caps(PROVIDER, id).is_some()));
        assert_eq!(crate::snip::vision::reads_images(PROVIDER, "claude-sonnet-5-5"), Some(true));
        assert!(crate::chat::documents::rich::strong(PROVIDER, "claude-haiku-5-5"));
        assert!(crate::chat::tools::supports_tools(PROVIDER, "claude-haiku-5-5"));
    }

    fn on_the_plan(model: &str) -> crate::config::LlmScope {
        crate::config::LlmScope { enabled: true, provider: PROVIDER.into(), base_url: BASE_URL.into(), model: model.into(), api_key: PLAN_KEY.into(), ..Default::default() }
    }

    /// A config with every scope on the plan (the key store's marker set).
    fn every_scope_on_the_plan() -> crate::config::YapConfig {
        let mut cfg = crate::config::YapConfig {
            post_process_enabled: true,
            pp_provider: PROVIDER.into(),
            pp_base_url: BASE_URL.into(),
            pp_api_key: PLAN_KEY.into(),
            pp_model: "claude-haiku-5-5".into(),
            ..Default::default()
        };
        cfg.pp_api_keys.insert(PROVIDER.into(), PLAN_KEY.into());
        cfg.llm_scopes.insert("voiceAgent".into(), on_the_plan("claude-haiku-5-5"));
        cfg.llm_scopes.insert("noteFormatting".into(), on_the_plan("claude-sonnet-5-5"));
        cfg.llm_scopes.insert("chat".into(), on_the_plan("claude-opus-5-5"));
        cfg
    }

    #[test]
    fn every_scope_can_be_on_the_plan_and_its_calls_say_whose_they_are() {
        // The URL names the scope; a bare one is Chat's.
        for s in Scope::ALL {
            assert_eq!(scope_of_url(&scope_url(s)), s);
        }
        assert_eq!(scope_url(Scope::Chat), BASE_URL);
        assert_eq!(scope_url(Scope::Cleanup), "claude-code://plan/cleanup");
        assert_eq!(scope_of_url("claude-code://plan/"), Scope::Chat);
        assert_eq!(scope_of_url("https://api.anthropic.com/v1"), Scope::Chat);
        assert_eq!(tag("groq", "https://api.groq.com/openai/v1".into(), Scope::Commands), "https://api.groq.com/openai/v1", "another provider's URL stays");

        let cfg = every_scope_on_the_plan();
        // Dictation Cleanup: the cleanup's endpoint.
        let (url, key, model, provider) = crate::ai::local_llm::effective_endpoint(&cfg);
        assert_eq!((url.as_str(), key.as_str(), model.as_str(), provider.as_str()), ("claude-code://plan/cleanup", PLAN_KEY, "claude-haiku-5-5", PROVIDER));
        assert!(crate::ai::cleanup_styles::question_cleanup_ready(&cfg), "a question can be cleaned on the plan");
        // Commands.
        let m = crate::dictation::pipeline::commands_model(&cfg);
        assert_eq!((m.base_url.as_str(), m.api_key.as_str(), m.model.as_str(), m.provider.as_str()), ("claude-code://plan/commands", PLAN_KEY, "claude-haiku-5-5", PROVIDER));
        // Note Formatting.
        let ep = crate::meetings::meeting_summary::resolve_endpoint(&cfg).unwrap();
        assert_eq!((ep.base_url.as_str(), ep.model.as_str(), ep.provider.as_str()), ("claude-code://plan/notes", "claude-sonnet-5-5", PROVIDER));
        // Chat, as before.
        let (url, _, model, provider, _, _) = crate::chat::turn::resolve_chat_endpoint(&cfg).unwrap();
        assert_eq!((url.as_str(), model.as_str(), provider.as_str()), (BASE_URL, "claude-opus-5-5", PROVIDER));
        for s in Scope::ALL {
            assert!(on_plan(&cfg, s), "{s:?}");
        }
        assert_eq!(scope_model(&cfg, Scope::Notes).as_deref(), Some("claude-sonnet-5-5"));

        // A scope that's off isn't on the plan itself: it runs on the
        // cleanup's, under the cleanup's settings (Chat's stays Chat's).
        let mut off = cfg.clone();
        for k in ["voiceAgent", "noteFormatting", "chat"] {
            off.llm_scopes.get_mut(k).unwrap().enabled = false;
        }
        assert!(!on_plan(&off, Scope::Commands) && !on_plan(&off, Scope::Notes));
        assert_eq!(crate::dictation::pipeline::commands_model(&off).base_url, "claude-code://plan/cleanup");
        assert_eq!(crate::chat::turn::resolve_chat_endpoint(&off).unwrap().0, BASE_URL);
        off.post_process_enabled = false;
        assert!(!on_plan(&off, Scope::Cleanup), "cleanup off");
    }

    #[test]
    fn signing_out_takes_the_plans_marker_out_of_every_scope() {
        let mut cfg = every_scope_on_the_plan();
        crate::ai::ai_model::clear_provider_key(&mut cfg, PROVIDER);
        assert!(cfg.pp_api_key.is_empty() && !cfg.pp_api_keys.contains_key(PROVIDER));
        for k in ["voiceAgent", "noteFormatting", "chat"] {
            assert!(cfg.llm_scopes[k].api_key.is_empty(), "{k}");
        }
        // Commands and the meeting notes now say what's missing, never call.
        assert!(crate::meetings::meeting_summary::resolve_endpoint(&cfg).is_err());
        assert!(crate::dictation::pipeline::commands_model(&cfg).api_key.is_empty());
        // Signed in again: every scope's marker back.
        crate::ai::ai_model::set_provider_key(&mut cfg, PROVIDER, PLAN_KEY).unwrap();
        for k in ["voiceAgent", "noteFormatting", "chat"] {
            assert_eq!(cfg.llm_scopes[k].api_key, PLAN_KEY, "{k}");
        }
        // An API key reaches every scope on that provider too.
        cfg.llm_scopes.get_mut("voiceAgent").unwrap().provider = "anthropic".into();
        crate::ai::ai_model::set_provider_key(&mut cfg, "anthropic", "sk-ant-example-0001").unwrap();
        assert_eq!(cfg.llm_scopes["voiceAgent"].api_key, "sk-ant-example-0001");
        assert_eq!(cfg.llm_scopes["chat"].api_key, PLAN_KEY, "a scope on another way keeps its own");
    }

    #[test]
    fn the_key_takes_a_call_the_plan_couldnt_answer_only_when_the_scope_says() {
        let mut cfg = every_scope_on_the_plan();
        // Off by default: no key spent.
        for s in Scope::ALL {
            assert_eq!(fallback_key(&cfg, s), None, "{s:?}");
        }
        cfg.pp_plan_fallback = true;
        cfg.llm_scopes.get_mut("voiceAgent").unwrap().plan_fallback = true;
        cfg.llm_scopes.get_mut("chat").unwrap().plan_fallback = true;
        assert_eq!(fallback_key(&cfg, Scope::Cleanup), None, "on, but no Anthropic key saved");
        cfg.pp_api_keys.insert("anthropic".into(), "sk-ant-example-0002".into());
        assert_eq!(fallback_key(&cfg, Scope::Cleanup).as_deref(), Some("sk-ant-example-0002"));
        assert_eq!(fallback_key(&cfg, Scope::Commands).as_deref(), Some("sk-ant-example-0002"));
        assert_eq!(fallback_key(&cfg, Scope::Notes), None, "Note Formatting's own switch is off");
        assert_eq!(fallback_key(&cfg, Scope::Chat), None, "never Chat's: it has Use my API key for this chat");

        // Connected: the plan's, under the scope's name (a call that fails
        // goes to the key then, in llm::post_chat_request).
        assert_eq!(crate::ai::local_llm::effective_endpoint(&cfg).3, PROVIDER);
        // Claude Code not ready here (no marker): straight to the key, on
        // the same model, for the scopes that said so — never Chat's.
        crate::ai::ai_model::clear_provider_key(&mut cfg, PROVIDER);
        let (url, key, model, provider) = crate::ai::local_llm::effective_endpoint(&cfg);
        assert_eq!((key.as_str(), model.as_str(), provider.as_str()), ("sk-ant-example-0002", "claude-haiku-5-5", "anthropic"));
        assert_eq!(url, anthropic_url());
        assert_eq!(crate::dictation::pipeline::commands_model(&cfg).provider, "anthropic");
        assert!(crate::meetings::meeting_summary::resolve_endpoint(&cfg).is_err(), "Note Formatting's switch is off: it says to sign in");
        let chat = crate::chat::turn::resolve_chat_endpoint(&cfg);
        assert!(chat.is_err(), "Chat as before: Claude Code isn't signed in");
        cfg.pp_plan_fallback = false;
        let (url, key, _, provider) = crate::ai::local_llm::effective_endpoint(&cfg);
        assert_eq!((url.as_str(), key.as_str(), provider.as_str()), ("claude-code://plan/cleanup", "", PROVIDER), "off: the plan's, which fails, and the words go in as said");
    }

    #[test]
    fn ready_means_found_new_enough_and_signed_in() {
        let base = cli::Status { installed: true, logged_in: true, min_version: "2.1.259".into(), ..Default::default() };
        assert!(ready(&base));
        assert!(!ready(&cli::Status { logged_in: false, ..base.clone() }));
        assert!(!ready(&cli::Status { too_old: true, ..base.clone() }));
        assert!(!ready(&cli::Status { installed: false, ..base.clone() }));
        assert!(!on_a_key(&cli::Status { auth_method: Some("claude.ai".into()), ..base.clone() }));
        assert!(on_a_key(&cli::Status { auth_method: Some("console".into()), ..base.clone() }));
        let s = serde_json::to_value(Status::of(base)).unwrap();
        assert_eq!((s["installed"].clone(), s["ready"].clone(), s["usageUrl"].clone()), (serde_json::json!(true), serde_json::json!(true), serde_json::json!(USAGE_URL)));
        assert_eq!(s["models"][0]["id"], "claude-haiku-5-5");
    }
}

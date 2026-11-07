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
//! - **Where**: the Chat scope only (Chat, the Notes view's Ask, the Voice
//!   Assistant, and the background calls on the Chat model). Never dictation
//!   cleanup, Commands or meeting summaries: too slow, and they'd eat the
//!   plan (Settings never offers it there).
//! - **A Chat turn** (`chat/turn/plan.rs`): one Claude Code session per
//!   conversation, kept warm 10 minutes ([`sessions`]), its events mapped
//!   into Chat's stream ([`map`]).
//! - **Every other call on the Chat model** (memory learning, a summary,
//!   notes' Ask, Think, …): one question, one answer ([`oneshot`]), from
//!   `llm::post_chat_request`.
//! - **Plan usage limits** ([`usage`]): where the plan's limits stand
//!   (Claude Code's own `get_usage`), for Chat's usage ring and Settings'
//!   card; kept in memory only.
//!
//! How it works: docs/architecture/ai-models.md, "Your Claude plan".

pub mod map;
pub mod oneshot;
pub mod sessions;
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

/// The models a plan chat offers, by their full ids (never aliases: the
/// effort, images and strong-model rules key on full ids), cheapest first.
pub const MODELS: &[(&str, &str)] = &[
    ("claude-haiku-5-5", "Claude Haiku 5.5"),
    ("claude-sonnet-5-5", "Claude Sonnet 5.5"),
    ("claude-opus-5-5", "Claude Opus 5.5"),
    ("claude-fable-5-1", "Claude Fable 5.1"),
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
    let cfg = crate::config::load();
    let on_plan = cfg.pp_api_keys.get(PROVIDER).is_some_and(|k| k == PLAN_KEY)
        || cfg.llm_scopes.get("chat").is_some_and(|s| s.provider == PROVIDER);
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
        assert_eq!(crate::snip::vision::reads_images(PROVIDER, "claude-sonnet-5-5"), Some(true));
        assert!(crate::chat::documents::rich::strong(PROVIDER, "claude-haiku-5-5"));
        assert!(crate::chat::tools::supports_tools(PROVIDER, "claude-haiku-5-5"));
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

//! The on-device model **loaded when it's needed, and its memory freed
//! when it's idle** (2026-10-25: Yap used ~10 GB as soon as it opened, on
//! an evening of cloud models only).
//!
//! - **At launch** (and after a Settings save) the sidecar starts only with
//!   "Load the on-device model when Yap starts" on
//!   (`config.local_model_at_launch`, off by default) and something really
//!   routed to it ([`launch_gate`], [`in_use`]).
//! - **On first use** otherwise: a call routed to the on-device model while
//!   it isn't loaded resolves to the **stand-by address** ([`STANDBY_URL`],
//!   from `local_llm::effective_endpoint_for`), and the one chat call every
//!   other goes through (`llm::post_chat_request`) loads it there and waits
//!   ([`wake`], bounded by [`LOAD_WAIT`]) before it sends — so no path that
//!   reaches the model can miss it. One that can't start: an error, said
//!   once in the log, not tried again for [`RETRY_AFTER`] (a dictation
//!   keeps its words as said).
//! - **Early**: a dictation whose cleanup (or translation, or transform)
//!   goes to this PC starts loading it when it starts recording, as do the
//!   Voice Assistant's key, the rewrite shortcut and a transform
//!   ([`load_soon`]): it loads while the person talks.
//! - **Idle**: stopped after `config.local_model_idle_mins` with no request
//!   ([`idle_step`]: 5 / 15 / 60 minutes, or never), never under a request
//!   or a stream, a dictation, a start or restart, or a meeting recording
//!   whose live notes go to it. The next use loads it again.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use crate::config::YapConfig;

use super::PROVIDER_ONDEVICE;

/// Where a call routed to the on-device model goes while it isn't loaded:
/// a loopback address nothing can listen on (port 0). `llm::post_chat_request`
/// loads the model and sends to its real address instead; anything else
/// that tried it would just fail, never reach another server.
pub const STANDBY_URL: &str = "http://127.0.0.1:0/v1";

/// The longest a call waits for the model to load before it goes without
/// (a dictation: its words as said). The start carries on meanwhile, so
/// the next call finds it up.
pub const LOAD_WAIT: Duration = Duration::from_secs(30);

/// A start that failed isn't tried again on demand for this long (a
/// dictation each minute shouldn't relaunch a model that won't run).
/// Settings' own Start, a model switch or a Settings save try at once.
pub const RETRY_AFTER: Duration = Duration::from_secs(60);

/// "Free its memory after it's unused for": 15 minutes when not set.
pub const DEFAULT_IDLE_MINS: u32 = 15;

/// The choices Settings offers (0 = never).
pub const IDLE_CHOICES: [u32; 4] = [5, 15, 60, 0];

/// How often the idle watch looks.
const IDLE_TICK: Duration = Duration::from_secs(20);

static APP: OnceLock<AppHandle> = OnceLock::new();
/// A start asked for by [`load_soon`] is under way.
static LOADING: AtomicBool = AtomicBool::new(false);
/// When the clock below started.
static EPOCH: OnceLock<Instant> = OnceLock::new();
/// The last use of the model (ms on [`EPOCH`]'s clock): a request starting
/// or ending, a load, a dictation that's about to use it.
static LAST_USE_MS: AtomicU64 = AtomicU64::new(0);
/// The last on-demand start that failed, and why.
static FAILED: Mutex<Option<(Instant, String)>> = Mutex::new(None);
/// That failure was told ([`say_failed_once`]); a load clears it.
static FAILURE_SAID: AtomicBool = AtomicBool::new(false);

#[cfg(debug_assertions)]
static E2E_LOADS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());

fn now_ms() -> u64 {
    EPOCH.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// The model was used just now: the idle clock starts again.
pub fn touch() {
    LAST_USE_MS.store(now_ms(), Ordering::SeqCst);
}

/// How long since the model was last used.
pub fn idle_for() -> Duration {
    Duration::from_millis(now_ms().saturating_sub(LAST_USE_MS.load(Ordering::SeqCst)))
}

/// Is `url` the stand-by address ([`STANDBY_URL`])?
pub fn is_standby(url: &str) -> bool {
    url.trim_end_matches('/') == STANDBY_URL
}

/// A start asked for on demand is under way.
pub fn loading() -> bool {
    LOADING.load(Ordering::SeqCst)
}

/// Why the last on-demand start failed, while that's recent.
fn recent_failure() -> Option<String> {
    FAILED
        .lock()
        .ok()?
        .as_ref()
        .filter(|(at, _)| at.elapsed() < RETRY_AFTER)
        .map(|(_, why)| why.clone())
}

/// Try on demand again at once (Settings' own Start, a save, a model switch).
pub fn forget_failure() {
    if let Ok(mut f) = FAILED.lock() {
        *f = None;
    }
}

/// A dictation went without the model because it won't start: the person
/// is told **once** (until it loads again), with Open settings; the
/// notice policy keeps it to one in ten minutes too ("aiModelFailed").
/// Still loading after [`LOAD_WAIT`]: nothing said (it's on its way).
pub fn say_failed_once(app: &AppHandle) {
    if recent_failure().is_none() || FAILURE_SAID.swap(true, Ordering::SeqCst) {
        return;
    }
    crate::notices::alert::show_soon(
        app,
        crate::notices::alert::Alert {
            kind: "aiModelFailed",
            icon: "settings",
            title: "The on-device AI model didn't start".to_string(),
            body: "Your words went in as you said them. Settings → Language Models has the model.".to_string(),
            action: Some(crate::notices::alert::Action { id: crate::notices::alert::OPEN_AI_SETTINGS, label: "Open settings" }),
            timeout_ms: 10_000,
        },
    );
}

/// Settings and the Chat picker read the model's state again.
pub(crate) fn changed() {
    if let Some(app) = APP.get() {
        let _ = app.emit("yap-local-llm-changed", serde_json::json!({}));
        let _ = app.emit("yap-chat-model-changed", serde_json::json!({ "scope": null }));
    }
}

/// **Start loading the model now, in the background**, for `why` (a
/// dictation recording, the Voice Assistant's key, a call waiting): never
/// waits. Nothing when it's up (the idle clock starts again), loading,
/// not installed, failed a moment ago, or Yap is quitting. Returns whether
/// it is up or on its way.
pub fn load_soon(cfg: &YapConfig, why: &'static str) -> bool {
    if super::is_running() {
        touch();
        return true;
    }
    if super::CLOSING.load(Ordering::SeqCst) || !super::is_installed(cfg) {
        return false;
    }
    if recent_failure().is_some() {
        return false;
    }
    if LOADING.swap(true, Ordering::SeqCst) {
        return true;
    }
    #[cfg(debug_assertions)]
    if let Ok(mut l) = E2E_LOADS.lock() {
        l.push(why);
    }
    tracing::info!(why, "Loading the on-device model");
    let t0 = Instant::now();
    tauri::async_runtime::spawn(async move {
        let r = super::start().await;
        let ms = t0.elapsed().as_millis() as u64;
        match &r {
            Ok(_) => {
                forget_failure();
                FAILURE_SAID.store(false, Ordering::SeqCst);
                touch();
                tracing::info!(why, ms, "On-device model loaded");
            }
            Err(e) => {
                tracing::warn!(why, ms, "The on-device model didn't load: {e}");
                if let Ok(mut f) = FAILED.lock() {
                    *f = Some((Instant::now(), e.clone()));
                }
            }
        }
        LOADING.store(false, Ordering::SeqCst);
        changed();
    });
    true
}

/// **The model's address once it's up**, for a call that's about to go to
/// it: at once when it's running, else after loading it ([`load_soon`])
/// or the start or restart under way — waiting `wait` at most. An error
/// when it isn't installed, won't start (or failed a moment ago), or is
/// still loading after `wait` (it carries on loading).
pub async fn wake(wait: Duration, why: &'static str) -> Result<String, String> {
    if let Some(url) = super::base_url() {
        touch();
        return Ok(url);
    }
    // (The config isn't kept across the waits: this future sits inside
    // every chat call's, which must stay small.)
    let on_its_way = super::restart_under_way() || load_soon(&crate::config::load(), why);
    if !on_its_way {
        return Err(match recent_failure() {
            Some(e) => format!("Yap's on-device model didn't start: {e}"),
            None => "Yap's on-device model isn't installed".to_string(),
        });
    }
    let deadline = Instant::now() + wait;
    loop {
        if let Some(url) = super::base_url() {
            touch();
            return Ok(url);
        }
        if !loading() && !super::restart_under_way() {
            return Err(match recent_failure() {
                Some(e) => format!("Yap's on-device model didn't start: {e}"),
                None => "Yap's on-device model didn't start".to_string(),
            });
        }
        if Instant::now() >= deadline {
            tracing::warn!(wait_s = wait.as_secs(), why, "The on-device model is still loading; going without it this time");
            return Err(format!("Yap's on-device model is still loading (over {} s)", wait.as_secs()));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// The provider a call to `scope` (an `llm_scopes` key) goes to before
/// any fallback: the scope's own when it's on, else the cleanup's.
pub fn scope_provider<'a>(cfg: &'a YapConfig, scope: &str) -> &'a str {
    match cfg.llm_scopes.get(scope) {
        Some(s) if s.enabled && !s.provider.is_empty() => s.provider.as_str(),
        _ => cfg.pp_provider.as_str(),
    }
}

/// Is anything really routed to the on-device model? The cleanup's
/// provider (Chat, Commands and the meeting notes inherit it when their
/// own is off), a cleanup style or an app's rule whose profile names it
/// while cleanup is on, or a Language Models scope switched on with it.
/// A stale profile nothing reaches doesn't count: it never loads a model
/// that's never used. Pure.
pub fn in_use(cfg: &YapConfig) -> bool {
    if cfg.pp_provider == PROVIDER_ONDEVICE {
        return true;
    }
    let profile_used = |p: &crate::config::CleanupProfile| {
        cfg.app_routes.iter().any(|r| !r.profile_id.is_empty() && r.profile_id == p.id)
            || (crate::ai::cleanup_styles::styles_on(cfg) && !p.category.trim().is_empty())
    };
    if cfg.post_process_enabled && cfg.cleanup_profiles.iter().any(|p| p.provider == PROVIDER_ONDEVICE && profile_used(p)) {
        return true;
    }
    cfg.llm_scopes.values().any(|s| s.enabled && s.provider == PROVIDER_ONDEVICE)
}

/// Would a dictation into `target`, starting now, use the on-device model?
/// A translating language shortcut's dictation (`translating`) is the
/// Commands model's (it takes the cleanup's place); else the cleanup — on,
/// and the app's routed plan on it (its style's or rule's profile, else the
/// cleanup's provider) — or the transform on every dictation (the Commands
/// model). A cheap look, at record start. Pure but for the config.
pub fn dictation_uses_it(cfg: &YapConfig, target: &crate::ai::cleanup_styles::Target, translating: bool) -> bool {
    let commands_here = scope_provider(cfg, "voiceAgent") == PROVIDER_ONDEVICE;
    if translating {
        return commands_here;
    }
    let cleanup = cfg.post_process_enabled
        && crate::ai::cleanup_styles::resolve(cfg, target)
            .is_some_and(|p| p.endpoint.as_ref().map_or(cfg.pp_provider.as_str(), |e| e.0.as_str()) == PROVIDER_ONDEVICE);
    let transform = commands_here
        && crate::dictation::transforms::for_dictation(cfg, crate::ai::cleanup_styles::category(cfg, target), target.is_terminal())
            .is_some();
    cleanup || transform
}

/// **The launch gate**: start the model at launch (and after a Settings
/// save) only with "Load the on-device model when Yap starts" on, while
/// something is routed to it ([`in_use`]), it's installed and not already
/// running. Pure.
pub fn launch_gate(at_launch: bool, used: bool, installed: bool, running: bool) -> bool {
    at_launch && used && installed && !running
}

/// "Free its memory after …" in minutes as a duration: `None` = never.
/// A value Settings doesn't offer counts as the default. Pure.
pub fn idle_limit(mins: u32) -> Option<Duration> {
    match mins {
        0 => None,
        m if IDLE_CHOICES.contains(&m) => Some(Duration::from_secs(u64::from(m) * 60)),
        _ => Some(Duration::from_secs(u64::from(DEFAULT_IDLE_MINS) * 60)),
    }
}

/// What the idle watch looks at ([`idle_step`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IdleFacts {
    /// The model is up.
    pub running: bool,
    /// Since its last use.
    pub idle: Duration,
    /// The setting ([`idle_limit`]): `None` = never.
    pub limit: Option<Duration>,
    /// Requests (or runs of them, streams) to it under way.
    pub in_flight: u32,
    /// A dictation recording, being transcribed or cleaned up.
    pub dictation: bool,
    /// A start, a restart (a new context) or a runtime swap under way.
    pub starting: bool,
    /// A meeting recording whose live notes go to the model, or an action
    /// plan being written.
    pub meeting: bool,
}

/// Stop the model now? Only when it's up, has been unused for the
/// setting's time, and nothing is using or about to use it. Pure.
pub fn idle_step(f: &IdleFacts) -> bool {
    let Some(limit) = f.limit else {
        return false;
    };
    f.running && f.idle >= limit && f.in_flight == 0 && !f.dictation && !f.starting && !f.meeting
}

/// A meeting recording whose rolling notes go to the on-device model (the
/// Note Formatting scope, else the cleanup's provider), or an action plan
/// being written.
fn meeting_uses_it(cfg: &YapConfig) -> bool {
    crate::meetings::meeting_end::summary_running()
        || (crate::meetings::meeting::recording_note().is_some() && scope_provider(cfg, "noteFormatting") == PROVIDER_ONDEVICE)
}

/// Test mode: the idle limit in ms instead of the setting's minutes (0 =
/// the setting's), `e2e_local_llm_idle_ms`.
#[cfg(debug_assertions)]
static E2E_IDLE_MS: AtomicU64 = AtomicU64::new(0);

/// The idle limit, and how often to look: a test run may shorten both
/// (`e2e_local_llm_idle_ms`, debug builds only; "Never" stays never).
fn idle_timing(cfg: &YapConfig) -> (Option<Duration>, Duration) {
    let limit = idle_limit(cfg.local_model_idle_mins);
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() {
        let ms = E2E_IDLE_MS.load(Ordering::SeqCst);
        if ms > 0 {
            return (limit.map(|_| Duration::from_millis(ms)), Duration::from_millis(100));
        }
        return (limit, Duration::from_millis(250));
    }
    (limit, IDLE_TICK)
}

/// Test mode only: free an idle model after `ms` instead of the setting's
/// minutes (0: the setting again).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_local_llm_idle_ms(ms: u64) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    E2E_IDLE_MS.store(ms, Ordering::SeqCst);
    Ok(())
}

/// From setup: remember the app (Settings hears of a load or an unload)
/// and **watch for an idle model**, stopping it per [`idle_step`]. Logged
/// (no content).
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
    touch();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let cfg = crate::config::load();
            let (limit, tick) = idle_timing(&cfg);
            tokio::time::sleep(tick).await;
            if super::CLOSING.load(Ordering::SeqCst) {
                return;
            }
            if !super::is_running() {
                continue;
            }
            let facts = IdleFacts {
                running: true,
                idle: idle_for(),
                limit,
                in_flight: super::requests_in_flight(),
                dictation: super::context::sidecar_busy(&app),
                starting: loading()
                    || super::restart_under_way()
                    || super::context::restart_pending()
                    || super::STARTING.try_lock().is_err(),
                meeting: meeting_uses_it(&cfg),
            };
            if !idle_step(&facts) {
                continue;
            }
            // Under the start lock: no start (or Chat waking it) slips in.
            let Ok(_one) = super::STARTING.try_lock() else { continue };
            if super::requests_in_flight() > 0 || idle_for() < facts.limit.unwrap_or_default() {
                continue;
            }
            tracing::info!(
                idle_mins = facts.idle.as_secs() / 60,
                setting_mins = cfg.local_model_idle_mins,
                "On-device model unused for a while: freeing its memory (it loads again when needed)"
            );
            super::stop();
            changed();
        }
    });
}

/// Test mode only: the loads asked for on demand, by why (`dictation`,
/// `assistant`, `call`…), the stand-in's starts, and whether it's up.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_local_llm_loads() -> Result<serde_json::Value, String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    let loads: Vec<&str> = E2E_LOADS.lock().map(|l| l.clone()).unwrap_or_default();
    Ok(serde_json::json!({
        "loads": loads,
        "starts": super::E2E_STARTS.load(Ordering::SeqCst),
        "running": super::is_running(),
        "loading": loading(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ondevice_scope(enabled: bool) -> crate::config::LlmScope {
        crate::config::LlmScope { enabled, provider: PROVIDER_ONDEVICE.to_string(), ..Default::default() }
    }

    #[test]
    fn the_launch_gate_needs_the_setting_and_a_use() {
        assert!(launch_gate(true, true, true, false));
        // Off by default: never at launch.
        assert!(!launch_gate(false, true, true, false));
        // Nothing routed to it, not installed, or already up: nothing.
        assert!(!launch_gate(true, false, true, false));
        assert!(!launch_gate(true, true, false, false));
        assert!(!launch_gate(true, true, true, true));
    }

    #[test]
    fn the_setting_is_off_and_fifteen_minutes_by_default() {
        let cfg = YapConfig::default();
        assert!(!cfg.local_model_at_launch);
        assert_eq!(cfg.local_model_idle_mins, 15);
        // A config from before: the same.
        let old: YapConfig = serde_json::from_str("{}").unwrap();
        assert!(!old.local_model_at_launch);
        assert_eq!(old.local_model_idle_mins, 15);
    }

    #[test]
    fn what_counts_as_routed_to_the_on_device_model() {
        // Cloud everywhere: nothing.
        let mut cfg = YapConfig { pp_provider: "groq".into(), post_process_enabled: true, ..Default::default() };
        assert!(!in_use(&cfg));
        // The cleanup's provider (Chat and the rest inherit it).
        cfg.pp_provider = PROVIDER_ONDEVICE.into();
        assert!(in_use(&cfg));
        cfg.pp_provider = "groq".into();
        // A scope switched on with it, but not one switched off.
        cfg.llm_scopes.insert("chat".into(), ondevice_scope(false));
        assert!(!in_use(&cfg));
        cfg.llm_scopes.insert("chat".into(), ondevice_scope(true));
        assert!(in_use(&cfg));
        cfg.llm_scopes.clear();
        // A profile on it that no app's rule (nor a style) reaches: stale.
        cfg.cleanup_profiles.push(crate::config::CleanupProfile {
            id: "p1".into(),
            provider: PROVIDER_ONDEVICE.into(),
            ..Default::default()
        });
        assert!(!in_use(&cfg));
        // An app's rule naming it: used, while cleanup is on.
        cfg.app_routes.push(crate::config::AppRoute {
            app: "slack.exe".into(),
            label: String::new(),
            profile_id: "p1".into(),
            prompt: String::new(),
        });
        assert!(in_use(&cfg));
        cfg.post_process_enabled = false;
        assert!(!in_use(&cfg));
    }

    #[test]
    fn a_dictation_starting_loads_it_only_when_it_will_use_it() {
        let app = crate::ai::cleanup_styles::Target::default();
        // On-device cleanup, on: yes.
        let mut cfg = YapConfig { pp_provider: PROVIDER_ONDEVICE.into(), post_process_enabled: true, ..Default::default() };
        assert!(dictation_uses_it(&cfg, &app, false));
        // Cleanup off: no (nothing else goes to it).
        cfg.post_process_enabled = false;
        assert!(!dictation_uses_it(&cfg, &app, false));
        // Cleanup on a cloud model: no — even with a stale Commands scope
        // pointing at it, unless a translation or a transform will use it.
        cfg.post_process_enabled = true;
        cfg.pp_provider = "groq".into();
        assert!(!dictation_uses_it(&cfg, &app, false));
        cfg.llm_scopes.insert("voiceAgent".into(), ondevice_scope(true));
        assert!(!dictation_uses_it(&cfg, &app, false));
        // A translating language shortcut: the Commands model's.
        assert!(dictation_uses_it(&cfg, &app, true));
        // A transform on every dictation: the Commands model's.
        cfg.auto_transform = "polish".into();
        assert!(crate::dictation::transforms::for_dictation(&cfg, crate::ai::cleanup_styles::category(&cfg, &app), false).is_some());
        assert!(dictation_uses_it(&cfg, &app, false));
        cfg.auto_transform = String::new();
        // Translating with Commands on a cloud model: no.
        cfg.llm_scopes.insert(
            "voiceAgent".into(),
            crate::config::LlmScope { enabled: true, provider: "groq".into(), ..Default::default() },
        );
        assert!(!dictation_uses_it(&cfg, &app, true));
    }

    #[test]
    fn a_scope_goes_to_its_own_provider_else_the_cleanups() {
        let mut cfg = YapConfig { pp_provider: PROVIDER_ONDEVICE.into(), ..Default::default() };
        assert_eq!(scope_provider(&cfg, "noteFormatting"), PROVIDER_ONDEVICE);
        cfg.llm_scopes.insert(
            "noteFormatting".into(),
            crate::config::LlmScope { enabled: true, provider: "groq".into(), ..Default::default() },
        );
        assert_eq!(scope_provider(&cfg, "noteFormatting"), "groq");
        cfg.llm_scopes.get_mut("noteFormatting").unwrap().enabled = false;
        assert_eq!(scope_provider(&cfg, "noteFormatting"), PROVIDER_ONDEVICE);
    }

    #[test]
    fn the_idle_setting_as_a_duration() {
        assert_eq!(idle_limit(5), Some(Duration::from_secs(300)));
        assert_eq!(idle_limit(15), Some(Duration::from_secs(900)));
        assert_eq!(idle_limit(60), Some(Duration::from_secs(3600)));
        assert_eq!(idle_limit(0), None);
        // Not one Settings offers: the default.
        assert_eq!(idle_limit(7), Some(Duration::from_secs(900)));
    }

    #[test]
    fn the_idle_model_is_freed_only_when_nothing_uses_it() {
        let limit = Some(Duration::from_secs(900));
        let idle = IdleFacts { running: true, idle: Duration::from_secs(901), limit, ..Default::default() };
        assert!(idle_step(&idle));
        // Not idle long enough, or never.
        assert!(!idle_step(&IdleFacts { idle: Duration::from_secs(899), ..idle }));
        assert!(!idle_step(&IdleFacts { limit: None, idle: Duration::from_secs(86_400), ..idle }));
        // Not running: nothing to free.
        assert!(!idle_step(&IdleFacts { running: false, ..idle }));
        // Never mid-request or mid-stream, under a dictation, a start or
        // restart, or a meeting whose notes go to it.
        assert!(!idle_step(&IdleFacts { in_flight: 1, ..idle }));
        assert!(!idle_step(&IdleFacts { dictation: true, ..idle }));
        assert!(!idle_step(&IdleFacts { starting: true, ..idle }));
        assert!(!idle_step(&IdleFacts { meeting: true, ..idle }));
    }

    #[test]
    fn the_stand_by_address_is_recognised_and_goes_nowhere() {
        assert!(is_standby(STANDBY_URL));
        assert!(is_standby("http://127.0.0.1:0/v1/"));
        assert!(!is_standby("http://127.0.0.1:8080/v1"));
        assert!(!is_standby(""));
        // Port 0: nothing listens there.
        assert_eq!(url::Url::parse(STANDBY_URL).unwrap().port(), Some(0));
    }

    #[test]
    fn an_address_it_served_on_before_goes_to_it_as_it_is_now() {
        super::super::served_on(54_321);
        // Not running (a unit test): a call to its old address loads it again.
        assert!(super::super::was_ours("http://127.0.0.1:54321/v1"));
        assert!(super::super::was_ours("http://127.0.0.1:54321/v1/"));
        // Never another port, host or server.
        assert!(!super::super::was_ours("http://127.0.0.1:54322/v1"));
        assert!(!super::super::was_ours("http://localhost:54321/v1"));
        assert!(!super::super::was_ours("https://api.groq.com/openai/v1"));
        assert!(!super::super::was_ours(""));
    }

    #[test]
    fn using_it_starts_the_idle_clock_again() {
        touch();
        assert!(idle_for() < Duration::from_secs(1));
    }
}

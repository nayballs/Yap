//! The on-device model's context length: the setting within what the
//! model was trained for, the prompt budgets it scales, a running model
//! restarted on a change (never mid-dictation), and what Settings and
//! the Chat picker show of it.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use super::models::CURATED_MODELS;
use super::runtime::{runtime_version, Version};
#[cfg(debug_assertions)]
use super::{E2E_BUSY, E2E_RESTARTS, E2E_SERVING};
use super::{
    active_model_path, placement, plan_for, requests_in_flight, serving_context, serving_facts, shown_running, start, stop,
    Placement, PlannedRestart, PROVIDER_ONDEVICE, RUNTIME_VERSION,
};

/// The context the sidecar serves when none is set (`local_context` 0; one
/// cache shared by its slots): room for the structured cleanup prompt +
/// one-shots (~2.1k tokens) plus a long dictation, and for a Chat turn's
/// tools, notes and history. What the meeting notes' budgets assume
/// (meeting_summary.rs).
pub const DEFAULT_CONTEXT: u32 = 8192;
/// The least the sidecar is started with (the cleanup's prompt alone is
/// ~2.1k tokens) — unless the model itself was trained for less.
pub const MIN_CONTEXT: u32 = 2048;
/// The most `local_context` is taken as (a header claiming more is still
/// held to this).
pub const MAX_CONTEXT: u32 = 1 << 20;
/// What the pickers offer, up to the model's own maximum (and that maximum
/// itself when it's between two of these).
pub const CONTEXT_CHOICES: [u32; 7] = [4_096, 8_192, 16_384, 32_768, 65_536, 131_072, 262_144];

/// The context the sidecar starts with: `configured` (0 = the default),
/// within [`MIN_CONTEXT`]..=[`MAX_CONTEXT`], and never more than the model
/// was trained for (`model_max`, its GGUF's `context_length`). Pure.
pub fn effective_context(configured: u32, model_max: Option<u64>) -> u32 {
    let want = if configured == 0 {
        DEFAULT_CONTEXT
    } else {
        configured.clamp(MIN_CONTEXT, MAX_CONTEXT)
    };
    match model_max {
        Some(m) if m > 0 => want.min(u32::try_from(m).unwrap_or(u32::MAX)),
        _ => want,
    }
}

/// The contexts offered for a model trained for `model_max` (unknown: up
/// to 32K), with `current` always among them. Pure.
pub fn context_choices(model_max: Option<u64>, current: u32) -> Vec<u32> {
    let max = model_max.map(|m| m.min(MAX_CONTEXT as u64) as u32);
    let top = max.unwrap_or(32_768);
    let mut v: Vec<u32> = CONTEXT_CHOICES.iter().copied().filter(|&c| c <= top).collect();
    if let Some(m) = max {
        if (MIN_CONTEXT..=CONTEXT_CHOICES[CONTEXT_CHOICES.len() - 1]).contains(&m) {
            v.push(m);
        }
    }
    v.push(current);
    v.sort_unstable();
    v.dedup();
    v
}

/// What a model was trained for: its GGUF's header, else the curated
/// entry's figure (a file not downloaded yet, or one being written).
pub fn trained_context(path: &Path) -> Option<u64> {
    crate::ai::gguf::model_file(path)
        .and_then(|f| f.shape.context_length)
        .or_else(|| {
            let file = path.file_name()?.to_str()?;
            CURATED_MODELS
                .iter()
                .find(|m| m.filename == file)
                .map(|m| m.max_context as u64)
        })
}

/// The context the on-device model gets with this config.
pub fn context_for(cfg: &crate::config::YapConfig) -> u32 {
    effective_context(cfg.local_context, trained_context(&active_model_path(cfg)))
}

/// The context the on-device model works with now: the running sidecar's,
/// else what it will start with. For the model chip.
pub fn context_now(cfg: &crate::config::YapConfig) -> u32 {
    serving_context().unwrap_or_else(|| context_for(cfg))
}

/// The most the 8K budgets grow (a 32K context; beyond it a small model's
/// answers over raw text get slow and no better).
pub const MAX_BUDGET_FACTOR: usize = 4;

/// How many times its 8K budgets a call may use on a model serving
/// `context` tokens: whole multiples of [`DEFAULT_CONTEXT`], 1–4. The
/// budgets leave the reply's room at 8K, so `factor × budget` + that room
/// stays inside `factor × 8K` ≤ the context. Pure.
pub fn budget_factor(context: u32) -> usize {
    ((context / DEFAULT_CONTEXT) as usize).clamp(1, MAX_BUDGET_FACTOR)
}

/// [`budget_factor`] for a call to `provider` at `base_url`: the running
/// on-device model's context, when the call really goes to it; 1 for
/// anything else (a cloud free tier counts tokens a minute; a self-hosted
/// server's window is its own; "ondevice" fallen back to the endpoint
/// underneath is that endpoint).
pub fn budget_factor_for(provider: &str, base_url: &str) -> usize {
    let on_it = provider == PROVIDER_ONDEVICE && super::base_url().is_some_and(|u| u == base_url);
    if !on_it {
        return 1;
    }
    serving_context().map(budget_factor).unwrap_or(1)
}

// ---- the context: restarting on a change, never mid-dictation --------------

/// Does a sidecar serving `serving` (None = not running) need a restart
/// to give the model `wanted`? A stopped one starts with it anyway. Pure.
pub fn needs_restart(serving: Option<u32>, wanted: u32) -> bool {
    serving.is_some_and(|c| c != wanted)
}

/// A pending restart's next move, each time it looks at the pipeline. Pure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartStep {
    /// A dictation is recording or being transcribed and cleaned up (on
    /// this very model, maybe): look again shortly.
    Wait,
    /// It just finished: give the paste a moment, then look again.
    Settle,
    Go,
}

pub fn restart_step(busy: bool, waited: bool, settled: bool) -> RestartStep {
    if busy {
        RestartStep::Wait
    } else if waited && !settled {
        RestartStep::Settle
    } else {
        RestartStep::Go
    }
}

/// How often a pending restart looks at the pipeline, and how long it lets
/// a finished dictation settle (its paste, a quick follow-up).
pub(super) const RESTART_TICK: Duration = Duration::from_millis(250);
pub(super) const RESTART_SETTLE: Duration = Duration::from_millis(1_500);

/// Wait until `busy()` says no dictation is going on ([`restart_step`]).
/// `false` = `current()` said a newer request took over. Never times out:
/// a dictation ends.
pub async fn wait_until_idle(
    busy: impl Fn() -> bool,
    current: impl Fn() -> bool,
    tick: Duration,
    settle: Duration,
) -> bool {
    let (mut waited, mut settled) = (false, false);
    loop {
        if !current() {
            return false;
        }
        match restart_step(busy(), waited, settled) {
            RestartStep::Wait => {
                waited = true;
                settled = false;
                tokio::time::sleep(tick).await;
            }
            RestartStep::Settle => {
                settled = true;
                tokio::time::sleep(settle).await;
            }
            RestartStep::Go => return true,
        }
    }
}

/// Bumped by every restart request; the latest one clears "pending".
static RESTART_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static RESTART_PENDING: AtomicBool = AtomicBool::new(false);
/// One restart at a time (a newer one waits, then compares again).
pub(super) static RESTARTING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A context change waiting for a dictation to end, or under way.
pub fn restart_pending() -> bool {
    RESTART_PENDING.load(Ordering::SeqCst)
}

/// A dictation recording, or being transcribed and cleaned up (the
/// cleanup may be on this very model).
fn pipeline_busy(app: &AppHandle) -> bool {
    use tauri::Manager;
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() && E2E_BUSY.load(Ordering::SeqCst) {
        return true;
    }
    if crate::dictation::pipeline::dictation_state().1 {
        return true;
    }
    app.try_state::<crate::AppState>()
        .and_then(|s| s.pipeline.lock().ok().map(|g| g.as_ref().is_some_and(|p| p.is_busy())))
        .unwrap_or(false)
}

/// The sidecar is in use: a dictation (above), or a request to it under way
/// — a Chat answer, a meeting digest, the Ask bar ([`request_starting`]).
pub(super) fn sidecar_busy(app: &AppHandle) -> bool {
    pipeline_busy(app) || requests_in_flight() > 0
}

/// Restart the sidecar on the context the config now asks for, once no
/// dictation is going on and no request to it is under way (never under
/// one: its cleanup or a Chat answer would lose the model mid-answer,
/// [`sidecar_busy`]). The model switch's path: stop, start. Emits
/// `yap-local-llm-changed` when done (the pickers re-read), and
/// `yap-chat-model-changed {scope: null}` so the chips do.
pub fn restart_when_idle(app: AppHandle) {
    let gen = RESTART_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    RESTART_PENDING.store(true, Ordering::SeqCst);
    let _ = app.emit("yap-local-llm-changed", serde_json::json!({ "pending": true }));
    tauri::async_runtime::spawn(async move {
        let latest = || RESTART_GEN.load(Ordering::SeqCst) == gen;
        if sidecar_busy(&app) {
            tracing::info!("On-device model: the new context waits for the dictation or answer to finish");
        }
        if !wait_until_idle(|| sidecar_busy(&app), latest, RESTART_TICK, RESTART_SETTLE).await {
            return; // a newer change took over
        }
        let result = {
            let _one = RESTARTING.lock().await;
            let wanted = context_for(&crate::config::load());
            restart_on(wanted).await
        };
        if let Err(e) = &result {
            tracing::warn!("On-device model didn't restart with its new context: {e}");
        }
        if latest() {
            RESTART_PENDING.store(false, Ordering::SeqCst);
        }
        let _ = app.emit(
            "yap-local-llm-changed",
            serde_json::json!({ "pending": restart_pending(), "error": result.err() }),
        );
        let _ = app.emit("yap-chat-model-changed", serde_json::json!({ "scope": null }));
    });
}

/// Stop and start the sidecar when it serves another context than `wanted`.
async fn restart_on(wanted: u32) -> Result<(), String> {
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() {
        // The stand-in "restarts"; a test run never starts a real model.
        if let Ok(mut g) = E2E_SERVING.lock() {
            if let Some(s) = g.as_mut().filter(|s| s.ctx != wanted) {
                s.ctx = wanted;
                // A new log: what it takes isn't known until it's measured.
                s.used_mib = None;
                E2E_RESTARTS.fetch_add(1, Ordering::SeqCst);
            }
        }
        return Ok(());
    }
    if !needs_restart(serving_context(), wanted) {
        return Ok(());
    }
    tracing::info!(ctx = wanted, "Restarting the on-device model with its new context");
    let _restarting = PlannedRestart::begin();
    stop();
    start().await.map(|_| ())
}

/// One context the pickers offer, with what it takes.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextOption {
    pub tokens: u32,
    /// ≈ MiB the model takes at it ([`crate::ai::gguf::estimate`]); `None`
    /// when its file can't be read.
    pub estimate_mib: Option<u64>,
    /// More than the graphics card has room for: layers would go to the
    /// processor (slow).
    pub over: bool,
}

/// The on-device model's context, as the pickers show it
/// (`local_llm_status().context`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextView {
    /// What the active model gets (the setting, within its maximum).
    pub tokens: u32,
    /// `local_context` (0 = the default).
    pub configured: u32,
    pub default: u32,
    /// What the active model was trained for.
    pub max: Option<u64>,
    /// The running sidecar's context (`None`: not running).
    pub serving: Option<u32>,
    /// A change waiting for a dictation to end, or the restart under way.
    pub pending: bool,
    /// The estimates are of the graphics card's memory (else the PC's).
    pub on_card: bool,
    /// The graphics card's room for the model: its own memory less what
    /// Windows, the apps on screen and Yap's speech model keep there.
    pub room_mib: Option<u64>,
    /// What the running model takes, from its log.
    pub used_mib: Option<u64>,
    pub options: Vec<ContextOption>,
}

/// What Windows, the apps on screen and Yap's speech model keep on the
/// card (gpu.rs' sums: the speech model ~1–1.6 GB, Windows ~0.5–1 GB). Not
/// llama.cpp's "MiB free": on Windows (WDDM) CUDA reports the card as if
/// nothing else used it (15,037 of 16,275 MiB free beside a running 4B).
const CARD_RESERVE_MIB: u64 = 2_048;

/// The card's room for the on-device model: its own memory less
/// [`CARD_RESERVE_MIB`]. Pure.
pub fn card_room(vram_mib: Option<u64>) -> Option<u64> {
    vram_mib.map(|v| v.saturating_sub(CARD_RESERVE_MIB))
}

/// Does the model go to the graphics card? Its log says when it's running;
/// else any card with memory of its own (llamafile offloads to it). Pure.
pub fn runs_on_card(placement: Option<Placement>, vram_mib: Option<u64>) -> bool {
    match placement {
        Some(p) => p.kind() != "cpu",
        None => vram_mib.is_some_and(|v| v >= 1_024),
    }
}

/// The context view for the active model ([`ContextView`]).
pub fn context_view(cfg: &crate::config::YapConfig) -> ContextView {
    let path = active_model_path(cfg);
    let file = crate::ai::gguf::model_file(&path);
    let max = trained_context(&path);
    let tokens = effective_context(cfg.local_context, max);
    let running = shown_running();
    let placement = running.then(placement).flatten();
    let facts = running.then(serving_facts).flatten();
    let gpu = crate::ai::gpu::this_pc();
    let vram = gpu.as_ref().map(|g| g.vram_mib);
    let on_card = runs_on_card(placement, vram);
    let room = on_card.then(|| card_room(vram)).flatten();
    // The runtime on disk (its buffers differ), or the one an install gets.
    let runtime = runtime_version().unwrap_or_else(|| Version::parse(RUNTIME_VERSION).expect("a version"));
    let options = context_choices(max, tokens)
        .into_iter()
        .map(|c| {
            let server = plan_for(cfg, &path, c, runtime).server();
            let estimate_mib = file
                .as_ref()
                .map(|f| crate::ai::gguf::estimate(&f.shape, f.bytes, c as u64, on_card, server).total_mib);
            ContextOption {
                tokens: c,
                estimate_mib,
                over: matches!((estimate_mib, room), (Some(e), Some(r)) if e > r),
            }
        })
        .collect();
    ContextView {
        tokens,
        configured: cfg.local_context,
        default: DEFAULT_CONTEXT,
        max,
        serving: running.then(serving_context).flatten(),
        pending: restart_pending(),
        on_card,
        room_mib: room,
        used_mib: facts.and_then(|f| if on_card { f.gpu_mib } else { f.host_mib }),
        options,
    }
}

/// A context as `local_context` keeps it: 0 for the default, else within
/// [`MIN_CONTEXT`]..=[`MAX_CONTEXT`]. Pure.
pub fn context_setting(tokens: u32) -> Result<u32, String> {
    if tokens == 0 || tokens == DEFAULT_CONTEXT {
        return Ok(0);
    }
    if !(MIN_CONTEXT..=MAX_CONTEXT).contains(&tokens) {
        return Err(format!(
            "A context of {tokens} tokens isn't offered: pick one from {}K to {}K",
            MIN_CONTEXT / 1024,
            MAX_CONTEXT / 1024
        ));
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::local_llm::models::{curated_by_id, GPU_MODEL_ID};

    #[test]
    fn the_context_is_the_setting_within_what_the_model_was_trained_for() {
        // Not set: today's 8K.
        assert_eq!(effective_context(0, Some(262_144)), 8_192);
        assert_eq!(effective_context(0, None), DEFAULT_CONTEXT);
        assert_eq!(effective_context(32_768, Some(262_144)), 32_768);
        // Never more than the model's maximum (Gemma 2: 8K; the 1.5B: 32K)…
        assert_eq!(effective_context(32_768, Some(8_192)), 8_192);
        assert_eq!(effective_context(131_072, Some(32_768)), 32_768);
        // …nor under the floor, unless the model can't do more.
        assert_eq!(effective_context(512, Some(32_768)), MIN_CONTEXT);
        assert_eq!(effective_context(0, Some(1_024)), 1_024);
        assert_eq!(effective_context(u32::MAX, None), MAX_CONTEXT);
        // A header's absurd claim is held to the ceiling too.
        assert_eq!(effective_context(u32::MAX, Some(u64::MAX)), MAX_CONTEXT);
        // How the setting is kept: 0 for the default.
        assert_eq!(context_setting(8_192), Ok(0));
        assert_eq!(context_setting(0), Ok(0));
        assert_eq!(context_setting(32_768), Ok(32_768));
        assert!(context_setting(100).is_err());
        assert!(context_setting(MAX_CONTEXT + 1).is_err());
    }

    #[test]
    fn the_choices_stop_at_the_models_maximum() {
        assert_eq!(context_choices(Some(32_768), 8_192), vec![4_096, 8_192, 16_384, 32_768]);
        assert_eq!(context_choices(Some(262_144), 8_192), CONTEXT_CHOICES.to_vec());
        assert_eq!(context_choices(Some(8_192), 8_192), vec![4_096, 8_192]);
        // A maximum between two choices is offered itself (Qwen3's 40K).
        assert_eq!(context_choices(Some(40_960), 8_192), vec![4_096, 8_192, 16_384, 32_768, 40_960]);
        // 1M models stop at 256K; an unknown maximum at 32K.
        assert_eq!(*context_choices(Some(1_048_576), 8_192).last().unwrap(), 262_144);
        assert_eq!(*context_choices(None, 8_192).last().unwrap(), 32_768);
        // The current one is always there.
        assert!(context_choices(Some(32_768), 24_576).contains(&24_576));
        // Every curated model has its maximum.
        assert!(CURATED_MODELS.iter().all(|m| m.max_context >= 8_192));
        assert_eq!(curated_by_id(GPU_MODEL_ID).unwrap().max_context, 262_144);
    }

    #[test]
    fn a_curated_file_not_readable_yet_has_the_curated_maximum() {
        assert_eq!(trained_context(Path::new("C:/nowhere/Qwen_Qwen3.5-4B-Q4_K_M.gguf")), Some(262_144));
        assert_eq!(trained_context(Path::new("C:/nowhere/gemma-2-2b-it-Q4_K_M.gguf")), Some(8_192));
        assert_eq!(trained_context(Path::new("C:/nowhere/someone-elses.gguf")), None);
        // A real header wins.
        let dir = std::env::temp_dir().join(format!("yap-ctx-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("gemma-2-2b-it-Q4_K_M.gguf");
        std::fs::write(&p, crate::ai::gguf::tests::qwen25_1_5b(10).bytes()).unwrap();
        assert_eq!(trained_context(&p), Some(32_768));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_card_has_room_for_its_memory_less_what_windows_and_speech_keep() {
        assert_eq!(card_room(Some(15_995)), Some(15_995 - 2_048));
        // A "6 GB" card: the 4B at 8K (~3.8 GB) fits, at 32K (~4.6) it doesn't.
        assert_eq!(card_room(Some(5_966)), Some(3_918));
        assert_eq!(card_room(Some(1_000)), Some(0));
        assert_eq!(card_room(None), None);
        // Where the estimates are of: the card, unless the log says the
        // processor, or there's no card with memory of its own.
        let cpu = Some(Placement { gpu_layers: 0, layers: 34 });
        let some = Some(Placement { gpu_layers: 20, layers: 34 });
        assert!(!runs_on_card(cpu, Some(15_995)));
        assert!(runs_on_card(some, Some(15_995)));
        assert!(runs_on_card(None, Some(8_176)));
        assert!(!runs_on_card(None, Some(512)), "integrated graphics");
        assert!(!runs_on_card(None, None));
    }

    #[test]
    fn a_bigger_context_scales_the_budgets_by_whole_8k_steps() {
        assert_eq!(budget_factor(8_192), 1);
        assert_eq!(budget_factor(4_096), 1, "never below the 8K budgets' floor");
        assert_eq!(budget_factor(12_288), 1);
        assert_eq!(budget_factor(16_384), 2);
        assert_eq!(budget_factor(32_768), 4);
        assert_eq!(budget_factor(262_144), MAX_BUDGET_FACTOR);
        // Not the on-device model (or it's down): the 8K budgets.
        assert_eq!(budget_factor_for("groq", "https://api.groq.com/openai/v1"), 1);
        assert_eq!(budget_factor_for(PROVIDER_ONDEVICE, "https://api.groq.com/openai/v1"), 1);
    }

    #[test]
    fn a_new_context_restarts_a_running_model_only() {
        assert!(needs_restart(Some(8_192), 32_768));
        assert!(!needs_restart(Some(32_768), 32_768));
        assert!(!needs_restart(None, 32_768), "a stopped one starts with it anyway");
        // Never while a dictation records or is cleaned up; settle after one.
        assert_eq!(restart_step(true, false, false), RestartStep::Wait);
        assert_eq!(restart_step(true, true, true), RestartStep::Wait);
        assert_eq!(restart_step(false, false, false), RestartStep::Go);
        assert_eq!(restart_step(false, true, false), RestartStep::Settle);
        assert_eq!(restart_step(false, true, true), RestartStep::Go);
    }

    #[tokio::test]
    async fn the_restart_waits_for_the_dictation_to_end() {
        use std::sync::atomic::AtomicU32;
        use std::sync::Arc;
        // A dictation records for the first 5 looks; the restart may only go
        // after it, and after the settle.
        let looks = Arc::new(AtomicU32::new(0));
        let l = looks.clone();
        let busy = move || l.fetch_add(1, Ordering::SeqCst) < 5;
        let went = wait_until_idle(busy, || true, Duration::from_millis(1), Duration::from_millis(1)).await;
        assert!(went);
        // 5 busy looks, the one that saw it idle (settle), the one after.
        assert_eq!(looks.load(Ordering::SeqCst), 7);
        // Idle from the start: at once.
        let looks = Arc::new(AtomicU32::new(0));
        let l = looks.clone();
        assert!(wait_until_idle(move || { l.fetch_add(1, Ordering::SeqCst); false }, || true, Duration::from_millis(1), Duration::from_millis(1)).await);
        assert_eq!(looks.load(Ordering::SeqCst), 1);
        // A newer change takes over: this one gives up, never restarting.
        let newer = Arc::new(AtomicBool::new(false));
        let n = newer.clone();
        let busy = move || {
            n.store(true, Ordering::SeqCst);
            true
        };
        let latest = {
            let n = newer.clone();
            move || !n.load(Ordering::SeqCst)
        };
        assert!(!wait_until_idle(busy, latest, Duration::from_millis(1), Duration::from_millis(1)).await);
    }
}

//! **Keeping the engine up to date** (Settings → Language Models: "Keep the
//! engine up to date", on by default; Check now). In the background, from
//! two minutes after launch, every half hour it looks whether a look is
//! due — **once a day** ([`CHECK_EVERY`]), only while the on-device model is
//! in use, never on a metered connection:
//! 1. **the feed** ([`feed::newest`]): the newest llama.cpp build at least
//!    3 days old, with the files this PC's builds need ([`candidates`]),
//!    not refused here;
//! 2. **taken** when it's newer than the build running — by itself at most
//!    once a week ([`UPDATE_EVERY`]); Check now takes it at once, and a
//!    move from llamafile waits for nothing ([`take_due`]);
//! 3. **downloaded** in the background (`shell/download.rs`: resume, the
//!    disk checked first), each file checked against GitHub's SHA-256 — a
//!    variant at a time; on an NVIDIA card CUDA (with NVIDIA's runtime,
//!    ~590 MB) and Vulkan both, the first time and after a new driver
//!    ([`to_try`]), then only the one kept;
//! 4. **tried** ([`trial`]) at an idle moment — no dictation, no request
//!    to the model under way, no meeting recording or being summarised,
//!    the model not loaded or unused for 5 minutes — the engine running now
//!    first, then the new variant, out of sight, under the start and
//!    restart locks (nothing else starts the model meanwhile; one that's
//!    asked for makes the trial give way and wait for the next idle
//!    moment);
//! 5. **switched to** when it passes ([`decide`]: CUDA when it's at least
//!    as fast as Vulkan, `trial::choose`; the build before kept for going
//!    back, the new one unproven until the sidecar starts on it — a failed
//!    first start rolls back, `local_llm::start_with`), else **refused**
//!    (never tried again) and Settings says why. A model that was loaded is
//!    loaded again, then moved to the new engine once nothing uses it.
//!
//! **The floor's fallback** ([`rescue`]): a build that never started here
//! with nothing to go back to — a fresh install's floor, no llamafile — is
//! refused (`local_llm::start_with`), and the floor's next build for this
//! PC is downloaded, checked, tried out of sight and kept if it starts and
//! answers (`engine::floor_order`: CUDA → Vulkan → the processor's on
//! NVIDIA, Vulkan → the processor's elsewhere); each one that fails is
//! refused in `engine.json`, so it isn't downloaded again at every launch.
//! When none starts, Settings and every start say so plainly
//! (`engine::NO_ENGINE`), with Try again ([`retry_floor`]); a new graphics
//! driver tries them again by itself (`engine::new_driver_retry`).
//!
//! One update at a time across Yaps (`engine/engine-update.lock`). A test
//! run reads its stand-in feed (`YAP_E2E_ENGINE_FEED`) only when asked
//! (Check now, or `YAP_E2E_ENGINE_CHECK_MS` for the background look), and
//! never runs a server: its trials are scripted (`YAP_E2E_ENGINE_TRIAL`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use super::trial::{self, Trial, Verdict};
use super::{
    candidates, engine_dir_in, feed, hardware, install, now_secs, read_marker, switch_to, tidy, to_try, update_marker, Build, Engine,
    Installed, Pick, Variant, Why, LOCK, TRIAL_LOG,
};
use crate::ai::local_llm::context::{sidecar_busy, RESTARTING, RESTART_SETTLE, RESTART_TICK};
use crate::ai::local_llm::runtime::{idle_with_locks, meeting_busy, UpgradeLock};
use crate::ai::local_llm::{is_running, llm_dir, on_demand, start_locked, stop, wants_start, CLOSING, NVIDIA, STARTING};

/// A look at the feed once a day.
pub const CHECK_EVERY: u64 = 86_400;
/// A newer build taken by itself at most once a week.
pub const UPDATE_EVERY: u64 = 7 * 86_400;
/// The first look after launch (the model and the first dictations first).
const FIRST_LOOK: Duration = Duration::from_secs(120);
/// How often the background loop sees whether a look is due.
const TICK: Duration = Duration::from_secs(30 * 60);
/// A loaded model must have been unused this long before a trial stops it.
pub const IDLE_BEFORE_TRIAL: Duration = Duration::from_secs(5 * 60);
/// What a download's Cancel cancels (`download_cancel`).
pub const CANCEL_ID: &str = "llm-engine";

/// Is a look at the feed due? Pure.
pub fn check_due(now: u64, checked_at: u64, asked: bool) -> bool {
    asked || now.saturating_sub(checked_at) >= CHECK_EVERY
}

/// Take `candidate` now, the build running being `current` (`None`:
/// llamafile)? Never an older or the same one; from llamafile at once;
/// else asked (Check now), or a week since the last update. Pure.
pub fn take_due(current: Option<Build>, candidate: Build, now: u64, updated_at: u64, asked: bool) -> bool {
    match current {
        None => true,
        Some(c) if candidate <= c => false,
        Some(_) => asked || now.saturating_sub(updated_at) >= UPDATE_EVERY,
    }
}

/// A trial's outcome for the build.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    Switch { to: Installed, ratio: Option<f64>, pick: Option<Pick> },
    Refuse { build: Build, why: Why, ratio: Option<f64>, keys: Vec<String> },
}

/// The verdict on one variant: a Vulkan build replacing an engine on CUDA
/// (`current_cuda`) passes at `trial::GOOD_ENOUGH_TPS` too. Pure.
fn verdict_on(current: &Trial, current_cuda: bool, i: &Installed, t: &Trial) -> Verdict {
    trial::verdict(Some(current), t, current_cuda && i.variant == Variant::Vulkan)
}

/// **What the trials come to**: of the variants that passed against the
/// current engine, the one `trial::choose` keeps (CUDA when it's at least
/// as fast as Vulkan, else Vulkan) — kept as this card and driver's pick
/// (`print`) when the PC has a choice (`choice`) — else the build refused
/// for every variant tried, with the first's reason. Pure.
pub fn decide(current: &Trial, current_cuda: bool, tried: &[(Installed, Trial)], choice: bool, print: &str, now: u64) -> Option<Decision> {
    let first = tried.first()?;
    let passed: Vec<(&Installed, Option<f64>, f64)> = tried
        .iter()
        .filter_map(|(i, t)| match verdict_on(current, current_cuda, i, t) {
            Verdict::Pass { ratio } => Some((i, ratio, t.tps.unwrap_or(0.0))),
            Verdict::Fail { .. } => None,
        })
        .collect();
    let kept = trial::choose(&passed.iter().map(|(i, _, tps)| (i.variant, *tps)).collect::<Vec<_>>());
    if let Some((to, ratio, _)) = kept.and_then(|v| passed.iter().find(|(i, ..)| i.variant == v)) {
        let pick = choice.then(|| Pick {
            print: print.to_string(),
            variant: to.variant,
            speeds: tried.iter().filter_map(|(i, t)| t.tps.map(|s| (i.variant.id().to_string(), s))).collect(),
            build: to.build,
            at: now,
        });
        return Some(Decision::Switch { to: (*to).clone(), ratio: *ratio, pick });
    }
    let (why, ratio) = match verdict_on(current, current_cuda, &first.0, &first.1) {
        Verdict::Fail { why, ratio } => (why, ratio),
        Verdict::Pass { ratio } => (Why::DidntStart, ratio),
    };
    Some(Decision::Refuse { build: first.0.build, why, ratio, keys: tried.iter().map(|(i, _)| i.key()).collect() })
}

// ---- what Settings sees ---------------------------------------------------------------

/// "" | "checking" | "downloading" | "waiting" | "trying", and a percent.
static PHASE: Mutex<(&'static str, u8)> = Mutex::new(("", 0));
static CANDIDATE: Mutex<Option<Build>> = Mutex::new(None);
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);
/// A look under way (Check now while one runs waits for it).
static BUSY: AtomicBool = AtomicBool::new(false);
static APP: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();

pub fn phase() -> (&'static str, u8) {
    PHASE.lock().map(|g| *g).unwrap_or(("", 0))
}

pub fn candidate() -> Option<Build> {
    CANDIDATE.lock().ok().and_then(|g| *g)
}

pub fn last_error() -> Option<String> {
    LAST_ERROR.lock().ok().and_then(|g| g.clone())
}

/// The download under way's size in MB (0: none), for Settings' line.
static DOWNLOAD_MB: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn download_mb() -> Option<u64> {
    let mb = DOWNLOAD_MB.load(Ordering::SeqCst);
    (mb > 0 && phase().0 == "downloading").then_some(mb)
}

fn set_download_mb(bytes: u64) {
    DOWNLOAD_MB.store(bytes.div_ceil(1_048_576), Ordering::SeqCst);
}

/// Settings follows along (`yap-local-engine`, the engine's status).
fn changed() {
    if let Some(app) = APP.get() {
        let _ = app.emit("yap-local-engine", super::status());
    }
}

fn set_phase(p: &'static str, percent: u8) {
    let moved = PHASE.lock().map(|mut g| std::mem::replace(&mut *g, (p, percent)) != (p, percent)).unwrap_or(false);
    if moved {
        changed();
    }
}

/// The look under way, until dropped: the phase cleared, Settings told.
struct Look;

impl Drop for Look {
    fn drop(&mut self) {
        BUSY.store(false, Ordering::SeqCst);
        if let Ok(mut g) = CANDIDATE.lock() {
            *g = None;
        }
        set_phase("", 0);
        changed();
    }
}

// ---- the background look --------------------------------------------------------------

/// When the background look first looks; `None` = never (a test run that
/// didn't ask with `YAP_E2E_ENGINE_CHECK_MS`, debug builds).
fn first_look() -> Option<Duration> {
    if crate::shell::e2e::active() {
        #[cfg(debug_assertions)]
        if let Some(ms) = std::env::var("YAP_E2E_ENGINE_CHECK_MS").ok().and_then(|v| v.trim().parse().ok()) {
            return Some(Duration::from_millis(ms));
        }
        return None;
    }
    Some(FIRST_LOOK)
}

/// **Start the background look** (from setup).
pub fn spawn(app: AppHandle) {
    let _ = APP.set(app.clone());
    let Some(first) = first_look() else { return };
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(first).await;
        loop {
            if CLOSING.load(Ordering::SeqCst) {
                return;
            }
            match look(&app, false).await {
                Ok(what) => tracing::debug!(what, "On-device engine: looked"),
                Err(e) => tracing::info!("On-device engine: not now ({e})"),
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

/// **Check now** (Settings): a look at once, in the background — the
/// feed read whatever the time, a newer build taken now. Settings follows
/// it on `yap-local-engine`.
pub fn check_now(app: &AppHandle) {
    let _ = APP.set(app.clone());
    // None of the floor's builds started here: Check now is Try again.
    if super::none_runs_here() {
        retry_floor();
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match look(&app, true).await {
            Ok(what) => tracing::info!(what, "On-device engine: checked"),
            Err(e) => tracing::info!("On-device engine: the check didn't finish ({e})"),
        }
    });
}

/// One look ([`look_inner`]), one at a time; its error kept for Settings.
pub async fn look(app: &AppHandle, asked: bool) -> Result<&'static str, String> {
    if BUSY.swap(true, Ordering::SeqCst) {
        return Ok("already looking");
    }
    let _look = Look;
    let r = look_inner(app, asked).await;
    if let Ok(mut g) = LAST_ERROR.lock() {
        *g = match &r {
            Err(e) if asked && !crate::shell::download::is_cancelled(e) => Some(e.clone()),
            _ => None,
        };
    }
    r
}

async fn look_inner(app: &AppHandle, asked: bool) -> Result<&'static str, String> {
    let llm = llm_dir();
    let edir = engine_dir_in(&llm);
    let m = read_marker(&edir);
    if m.paused && !asked {
        return Ok("turned off");
    }
    let Some(active) = super::active_in(&llm) else {
        return Ok("not installed");
    };
    let test_run = crate::shell::e2e::active();
    if !asked {
        let cfg = crate::config::load();
        if !crate::ai::local_llm::models::in_use(&cfg) && !is_running() {
            return Ok("the on-device model isn't in use");
        }
        if !test_run && crate::updates::metered() {
            return Ok("a metered connection");
        }
    }
    let now = now_secs();
    if !check_due(now, m.checked_at, asked) {
        return Ok("not due");
    }
    let lock = UpgradeLock::take_named(&edir, LOCK).ok_or("another Yap is updating the engine")?;
    set_phase("checking", 0);
    let m = read_marker(&edir);
    let print = crate::dictation::stt_speed::pc_print();
    let hw = hardware();
    let all = candidates(&hw);
    let need = to_try(&all, m.pick.as_ref(), &print);
    let refused = |k: &str| m.is_refused(k);
    let release = feed::newest(&need, &refused).await?;
    let current = active.build();
    update_marker(&edir, |mm| {
        mm.checked_at = now;
        mm.latest = release.as_ref().map(|r| r.build).max(current);
    })
    .map_err(|e| format!("Couldn't save the engine's marker: {e}"))?;
    changed();
    let Some(release) = release else { return Ok("up to date") };
    if !take_due(current, release.build, now, m.updated_at, asked) {
        return Ok(if current.is_some_and(|c| c >= release.build) { "up to date" } else { "a newer build waits for the week" });
    }
    drop(lock);
    if let Ok(mut g) = CANDIDATE.lock() {
        *g = Some(release.build);
    }
    tracing::info!(build = %release.build, from = %active.label(), variants = need.len(), "On-device engine: a newer build to try");

    // A variant at a time — on an NVIDIA card CUDA and Vulkan both, the
    // first time and after a new driver — downloaded (under the lock), then
    // tried at an idle moment (the lock let go meanwhile: another Yap's
    // launch tidy isn't held up for an evening).
    let current_cuda = active.runtime().cuda() && hw.vendor == Some(NVIDIA);
    let variants: Vec<Variant> = need.iter().copied().filter(|v| !m.is_refused(&format!("{}-{}", release.build, v.id()))).collect();
    let mut cur_trial: Option<Trial> = None;
    let mut tried: Vec<(Installed, Trial)> = Vec::new();
    let mut failed: Option<String> = None;
    for v in &variants {
        let Some((zip, rt)) = feed::pick_assets(&release, *v) else { continue };
        let installed = {
            let _lock = UpgradeLock::take_named(&edir, LOCK).ok_or("another Yap is updating the engine")?;
            let ticket = crate::shell::download::Ticket::begin(CANCEL_ID).ok_or("The engine is already downloading")?;
            set_download_mb(zip.size + rt.as_ref().map(|r| r.size).unwrap_or(0));
            set_phase("downloading", 0);
            let report = |p: crate::shell::download::Progress| set_phase("downloading", p.percent());
            install(&edir, release.build, *v, &zip, rt.as_ref(), &report, &ticket).await
        };
        let installed = match installed {
            Ok(i) => i,
            Err(e) if crate::shell::download::is_cancelled(&e) => return Err(e),
            Err(e) => {
                tracing::warn!(build = %release.build, variant = v.id(), "On-device engine: the download didn't finish: {e}");
                failed = Some(e);
                continue;
            }
        };
        let (cur, t) = loop {
            set_phase("waiting", 0);
            if let Some(r) = trial_round(app, &active, cur_trial.as_ref(), &installed).await {
                break r;
            }
            if CLOSING.load(Ordering::SeqCst) {
                return Err("Yap is closing".into());
            }
            tracing::info!("On-device engine: the trial gave way to the model being used; trying again at the next idle moment");
            tokio::time::sleep(Duration::from_secs(30)).await;
        };
        cur_trial = Some(cur);
        tried.push((installed, t));
    }
    let Some(cur) = cur_trial else {
        return Err(failed.unwrap_or_else(|| "nothing to download for this PC".into()));
    };
    tracing::info!(engine = %active.label(), started = cur.started, tool = ?cur.tool, tps = ?cur.tps.map(|v| v.round()), vision = ?cur.vision, "On-device engine: trial of the engine running now");
    let Some(decision) = decide(&cur, current_cuda, &tried, all.len() > 1, &print, now_secs()) else {
        return Ok("nothing tried");
    };
    let _lock = UpgradeLock::take_named(&edir, LOCK).ok_or("another Yap is updating the engine")?;
    let what = apply(&edir, &active, decision);
    // A running model moves to the new engine once nothing uses it.
    if what == "switched" {
        restart_when_idle(app.clone());
    }
    let _ = app.emit("yap-local-llm-changed", serde_json::json!({}));
    let _ = app.emit("yap-chat-model-changed", serde_json::json!({ "scope": null }));
    Ok(what)
}

/// The switch or the refusal written to the marker; old folders tidied.
fn apply(edir: &std::path::Path, current: &Engine, decision: Decision) -> &'static str {
    match decision {
        Decision::Switch { to, ratio, pick } => {
            let saved = update_marker(edir, |m| {
                switch_to(m, to.clone(), ratio, now_secs());
                if pick.is_some() {
                    m.pick = pick.clone();
                }
            });
            match saved {
                Ok(m) => {
                    tracing::info!(build = %to.build, variant = to.variant.id(), ratio = ?ratio.map(|r| (r * 100.0).round()), "On-device engine: switched to the new build");
                    tidy(edir, &m);
                    "switched"
                }
                Err(e) => {
                    tracing::warn!("On-device engine: couldn't save the switch: {e}");
                    "couldn't save"
                }
            }
        }
        Decision::Refuse { build, why, ratio, keys } => {
            let staying = current.build();
            if let Ok(m) = update_marker(edir, |m| {
                for k in &keys {
                    m.refuse(k);
                }
                // Not "available" any more: what runs is the newest it takes.
                if m.latest == Some(build) {
                    m.latest = staying;
                }
                m.event = Some(super::Event::Refused { build, why, ratio, staying, at: now_secs() });
            }) {
                tidy(edir, &m);
            }
            tracing::warn!(build = %build, why = ?why, ratio = ?ratio.map(|r| (r * 100.0).round()), "On-device engine: the new build didn't pass its trial here; staying");
            "refused"
        }
    }
}

/// What a trial runs: the person's model at its context, the projector
/// when they have it on, the log.
struct Subject {
    model: std::path::PathBuf,
    ctx: u32,
    vision_on: bool,
    log: std::path::PathBuf,
}

/// One engine's trial (in a test run, its script: nothing runs).
async fn try_one(engine: &Engine, key: &str, s: &Subject, abort: &(dyn Fn() -> bool + Sync)) -> Trial {
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() {
        return trial::scripted(key);
    }
    let _ = key;
    trial::run(engine, &s.model, s.ctx, s.vision_on, &s.log, abort).await
}

/// An idle moment for a trial: nothing uses the model, and it isn't
/// loaded — or has been unused for [`IDLE_BEFORE_TRIAL`].
fn trial_idle(app: &AppHandle) -> bool {
    !(sidecar_busy(app) || meeting_busy() || wants_start() || on_demand::loading())
        && (!is_running() || on_demand::idle_for() >= IDLE_BEFORE_TRIAL)
}

/// One trial at an idle moment: the engine running now (unless `cur`
/// already has its trial), then `new`. `None`: it gave way (the person
/// needed the model). A model that was loaded is loaded again after it.
async fn trial_round(app: &AppHandle, current: &Engine, cur: Option<&Trial>, new: &Installed) -> Option<(Trial, Trial)> {
    let (_restart, _one) = idle_with_locks(&RESTARTING, &STARTING, || trial_idle(app), RESTART_TICK, RESTART_SETTLE).await;
    let edir = engine_dir_in(&llm_dir());
    let _lock = UpgradeLock::take_named(&edir, LOCK)?;
    set_phase("trying", 0);
    let was_running = is_running();
    if was_running {
        tracing::info!("On-device engine: freeing the idle model for the trial");
        stop();
    }
    let cfg = crate::config::load();
    let subject = Subject {
        model: crate::ai::local_llm::active_model_path(&cfg),
        ctx: crate::ai::local_llm::context::context_for(&cfg),
        vision_on: cfg.local_vision,
        log: edir.join(TRIAL_LOG),
    };
    let abort = || CLOSING.load(Ordering::SeqCst) || sidecar_busy(app) || meeting_busy() || wants_start() || on_demand::loading();
    let cur = match cur {
        Some(c) => c.clone(),
        None => try_one(current, "current", &subject, &abort).await,
    };
    let t = if cur.aborted { Trial { aborted: true, ..Default::default() } } else { try_one(&Engine::llama_cpp(&edir, new), &new.build.to_string(), &subject, &abort).await };
    if !t.aborted {
        tracing::info!(build = %new.build, variant = new.variant.id(), started = t.started, answered = t.answered, tool = ?t.tool, tps = ?t.tps.map(|v| v.round()), vision = ?t.vision, "On-device engine: trial");
    }
    // A model that was loaded loads again (on the engine running now; the
    // switch moves it later, once nothing uses it).
    if was_running && !CLOSING.load(Ordering::SeqCst) {
        let _restarting = crate::ai::local_llm::PlannedRestart::begin();
        if let Err(e) = start_locked().await {
            tracing::warn!("On-device engine: the model didn't load again after the trial: {e}");
        }
    }
    (!cur.aborted && !t.aborted).then_some((cur, t))
}

// ---- the floor's fallback ----------------------------------------------------------------

/// The floor's fallback under way ([`rescue`]).
static RESCUING: AtomicBool = AtomicBool::new(false);

/// Is another of the floor's builds being set up (a start meanwhile says
/// so, and the model counts as installed)?
pub fn rescuing() -> bool {
    RESCUING.load(Ordering::SeqCst)
}

/// What a start says while the fallback sets another build up.
pub const RESCUING_NOW: &str = "The on-device engine didn't start on this PC, so Yap is setting up another build of it. Try again in a minute.";

/// **After a start that failed** (`local_llm::start_locked`): the plain
/// word when none of the engine's builds runs here (noted for Settings),
/// else — nothing current any more, no llamafile: a build that never
/// started was refused (`engine::first_start_failed`) — the floor's next
/// build set up in the background. `None`: nothing to add.
pub fn after_failed_start() -> Option<String> {
    let llm = llm_dir();
    if super::active_in(&llm).is_some() || super::llamafile_in(&llm).is_some() {
        return None;
    }
    if super::none_runs_here() {
        note_none_runs();
        return Some(super::NO_ENGINE.to_string());
    }
    if !engine_dir_in(&llm).join(super::MARKER).is_file() {
        return None;
    }
    spawn_rescue();
    Some(RESCUING_NOW.to_string())
}

/// At launch: Yap closed while the fallback was setting a build up, or a
/// new driver forgot the refusals (`engine::new_driver_retry`): it carries
/// on.
pub fn rescue_if_due() {
    let llm = llm_dir();
    let due = super::active_in(&llm).is_none()
        && super::llamafile_in(&llm).is_none()
        && engine_dir_in(&llm).join(super::MARKER).is_file()
        && !super::none_runs_here();
    if due {
        tracing::info!("On-device engine: nothing runs yet; setting up one of the floor's builds");
        spawn_rescue();
    }
}

/// **Try again** (Settings, when none of the floor's builds started): their
/// refusals forgotten, the fallback from the first again.
pub fn retry_floor() {
    let edir = engine_dir_in(&llm_dir());
    if super::update_marker(&edir, super::forget_floor_refusals).is_ok() {
        tracing::info!("On-device engine: trying the floor's builds again");
        spawn_rescue();
        changed();
    }
}

fn spawn_rescue() {
    if RESCUING.swap(true, Ordering::SeqCst) {
        return;
    }
    changed();
    tauri::async_runtime::spawn(async {
        let what = rescue().await;
        RESCUING.store(false, Ordering::SeqCst);
        set_phase("", 0);
        changed();
        if let Some(app) = APP.get() {
            let _ = app.emit("yap-local-llm-changed", serde_json::json!({}));
        }
        tracing::info!(what, "On-device engine: the floor's fallback");
    });
}

/// "None of the engine's builds started on this PC", for Settings.
fn note_none_runs() {
    let edir = engine_dir_in(&llm_dir());
    let order = super::floor_order(&candidates(&hardware()));
    let print = crate::dictation::stt_speed::pc_print();
    let _ = update_marker(&edir, |m| {
        if !matches!(m.event, Some(super::Event::NoEngine { .. })) {
            tracing::warn!(tried = order.len(), "On-device engine: none of its builds started on this PC");
            m.event = Some(super::Event::NoEngine { build: super::FLOOR, tried: order.clone(), print: print.clone(), at: now_secs() });
        }
    });
    changed();
}

/// **The floor's fallback**: the floor's next build for this PC not refused
/// ([`super::next_floor`]) downloaded and checked (the stand-ins in a test
/// run), tried out of sight with the person's model — under the start lock,
/// so nothing else starts meanwhile — and kept when it starts and answers
/// (`trial::rescue_ok`: current, unproven until the sidecar starts on it;
/// this card's pick), else refused and the next one tried. A log that blames
/// the model stops it, refusing nothing; a download that fails stops it too
/// (tried again at the next failed start or launch).
async fn rescue() -> &'static str {
    let llm = llm_dir();
    let edir = engine_dir_in(&llm);
    let Some(_lock) = UpgradeLock::take_named(&edir, LOCK) else { return "another Yap is setting the engine up" };
    let cfg = crate::config::load();
    let model = crate::ai::local_llm::active_model_path(&cfg);
    if !model.is_file() {
        return "no model to try it with";
    }
    let ctx = crate::ai::local_llm::context::context_for(&cfg);
    let order = super::floor_order(&candidates(&hardware()));
    let print = crate::dictation::stt_speed::pc_print();
    loop {
        if CLOSING.load(Ordering::SeqCst) {
            return "Yap is closing";
        }
        let m = read_marker(&edir);
        if m.current.is_some() || super::llamafile_in(&llm).is_some() {
            return "an engine runs";
        }
        let Some(v) = super::next_floor(&m, &order) else {
            note_none_runs();
            return "none runs";
        };
        let (zip, cudart) = super::floor_assets(v);
        let stand_in = |key: &str, a: super::Asset| {
            let (url, sha256) = crate::ai::local_llm::models::source(key, "", &a.sha256);
            super::Asset { sha256, url: (!url.is_empty()).then_some(url), ..a }
        };
        let (zip, cudart) = (stand_in("engine", zip), cudart.map(|c| stand_in("cudart", c)));
        let Some(ticket) = crate::shell::download::Ticket::begin(CANCEL_ID) else { return "already downloading" };
        if let Ok(mut g) = CANDIDATE.lock() {
            *g = Some(super::FLOOR);
        }
        set_download_mb(zip.size + cudart.as_ref().map(|c| c.size).unwrap_or(0));
        set_phase("downloading", 0);
        let report = |p: crate::shell::download::Progress| set_phase("downloading", p.percent());
        let installed = match install(&edir, super::FLOOR, v, &zip, cudart.as_ref(), &report, &ticket).await {
            Ok(i) => i,
            Err(e) => {
                tracing::warn!(variant = v.id(), "On-device engine: the floor's {} build didn't download: {e}", v.id());
                return "the download failed";
            }
        };
        drop(ticket);
        set_phase("trying", 0);
        let t = {
            let _one = STARTING.lock().await;
            let subject = Subject { model: model.clone(), ctx, vision_on: false, log: edir.join(TRIAL_LOG) };
            try_one(&Engine::llama_cpp(&edir, &installed), &installed.key(), &subject, &|| CLOSING.load(Ordering::SeqCst)).await
        };
        tracing::info!(variant = v.id(), started = t.started, answered = t.answered, tps = ?t.tps.map(|x| x.round()), model_failed = t.model_failed, "On-device engine: the floor's fallback, a trial");
        if t.aborted {
            return "Yap is closing";
        }
        if t.model_failed {
            tracing::warn!("On-device engine: the model itself doesn't load; no build is refused for it");
            return "the model doesn't load";
        }
        if trial::rescue_ok(&t) {
            let tried: Vec<Variant> = order.iter().copied().take_while(|x| *x != v).filter(|x| m.is_refused(&super::floor_key(*x))).collect();
            let saved = update_marker(&edir, |m| {
                m.current = Some(installed.clone());
                m.unproven = true;
                m.event = Some(super::Event::Fallback { build: super::FLOOR, variant: v, tried: tried.clone(), at: now_secs() });
                m.pick = Some(Pick {
                    print: print.clone(),
                    variant: v,
                    speeds: t.tps.map(|s| [(v.id().to_string(), s)].into_iter().collect()).unwrap_or_default(),
                    build: super::FLOOR,
                    at: now_secs(),
                });
            });
            if let Ok(m) = saved {
                tidy(&edir, &m);
            }
            crate::ai::local_llm::on_demand::forget_failure();
            tracing::info!(variant = v.id(), "On-device engine: the floor's {} build runs here", v.id());
            return "switched";
        }
        let _ = update_marker(&edir, |m| m.refuse(&installed.key()));
        let _ = std::fs::remove_dir_all(installed.dir(&edir));
        tracing::warn!(variant = v.id(), "On-device engine: the floor's {} build didn't start here either; refused", v.id());
    }
}

/// **Going back by hand** (Settings): the build before (or llamafile), the
/// one left refused; a running model moves once nothing uses it.
pub fn go_back(app: &AppHandle) -> Result<(), String> {
    let llm = llm_dir();
    let edir = engine_dir_in(&llm);
    let _lock = UpgradeLock::take_named(&edir, LOCK).ok_or("The engine is being updated. Try again in a moment.")?;
    let llamafile = super::llamafile_in(&llm).is_some();
    let mut m = read_marker(&edir);
    super::go_back(&mut m, llamafile, now_secs())?;
    super::write_marker(&edir, &m).map_err(|e| format!("Couldn't save it: {e}"))?;
    tracing::info!(to = m.current.as_ref().map(|c| c.build.to_string()).unwrap_or_else(|| "llamafile".into()), "On-device engine: went back by hand");
    restart_when_idle(app.clone());
    changed();
    Ok(())
}

/// A running model restarted on the engine the marker now names, once no
/// dictation or answer uses it (`context::wait_until_idle`).
fn restart_when_idle(app: AppHandle) {
    if crate::shell::e2e::active() || !is_running() {
        let _ = app.emit("yap-local-llm-changed", serde_json::json!({}));
        return;
    }
    tauri::async_runtime::spawn(async move {
        crate::ai::local_llm::context::wait_until_idle(|| sidecar_busy(&app), || true, RESTART_TICK, RESTART_SETTLE).await;
        let result = {
            let _one = RESTARTING.lock().await;
            let _restarting = crate::ai::local_llm::PlannedRestart::begin();
            stop();
            crate::ai::local_llm::start().await.map(|_| ())
        };
        if let Err(e) = &result {
            tracing::warn!("On-device engine: the model didn't start on the new engine: {e}");
        }
        let _ = app.emit("yap-local-llm-changed", serde_json::json!({ "error": result.err() }));
        let _ = app.emit("yap-chat-model-changed", serde_json::json!({ "scope": null }));
    });
}

/// "Keep the engine up to date" on or off.
pub fn set_auto(on: bool) -> Result<(), String> {
    let edir = engine_dir_in(&llm_dir());
    update_marker(&edir, |m| m.paused = !on).map_err(|e| format!("Couldn't save it: {e}"))?;
    tracing::info!(on, "On-device engine: keep up to date");
    changed();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400;

    fn inst(build: u32, variant: Variant) -> Installed {
        Installed { build: Build(build), variant, sha256: String::new(), cudart: None, device: None }
    }

    fn ok(tps: f64) -> Trial {
        Trial { started: true, answered: true, tool: Some(true), tps: Some(tps), vision: None, load_ms: 1, aborted: false, model_failed: false }
    }

    #[test]
    fn a_look_once_a_day_and_a_newer_build_once_a_week_unless_asked() {
        assert!(check_due(10 * DAY, 0, false));
        assert!(!check_due(10 * DAY, 10 * DAY - 3_600, false), "an hour ago");
        assert!(check_due(10 * DAY, 10 * DAY - 60, true), "Check now");
        let (cur, new) = (Build(11_960), Build(11_980));
        assert!(!take_due(Some(cur), Build(11_960), 100 * DAY, 0, true), "the same build: never");
        assert!(!take_due(Some(new), cur, 100 * DAY, 0, true), "an older one: never");
        assert!(!take_due(Some(cur), new, 100 * DAY, 97 * DAY, false), "updated three days ago: waits for the week");
        assert!(take_due(Some(cur), new, 100 * DAY, 93 * DAY, false));
        assert!(take_due(Some(cur), new, 100 * DAY, 99 * DAY, true), "Check now takes it");
        assert!(take_due(None, new, 100 * DAY, 99 * DAY, false), "from llamafile: at once");
    }

    #[test]
    fn on_nvidia_cuda_is_kept_when_at_least_as_fast_vulkan_is_the_fallback_else_the_build_is_refused() {
        // From llamafile on CUDA (225 tokens/s): CUDA 269 and Vulkan 150 both measured, CUDA kept.
        let lf = ok(225.0);
        let tried = vec![(inst(11_982, Variant::Cuda13), ok(269.0)), (inst(11_982, Variant::Vulkan), ok(150.0))];
        match decide(&lf, true, &tried, true, "card", 5).unwrap() {
            Decision::Switch { to, ratio, pick } => {
                assert_eq!(to.variant, Variant::Cuda13);
                assert!((ratio.unwrap() - 269.0 / 225.0).abs() < 1e-9);
                let pick = pick.unwrap();
                assert_eq!((pick.variant, pick.print.as_str(), pick.speeds.len()), (Variant::Cuda13, "card", 2));
            }
            d => panic!("{d:?}"),
        }
        // CUDA doesn't start: Vulkan, the fallback, passes at 100+ tokens/s
        // although slower than llamafile's CUDA.
        let tried = vec![(inst(11_982, Variant::Cuda13), Trial::default()), (inst(11_982, Variant::Vulkan), ok(150.0))];
        assert!(matches!(decide(&lf, true, &tried, true, "card", 5), Some(Decision::Switch { to, pick: Some(_), .. }) if to.variant == Variant::Vulkan));
        // No choice on this PC (an AMD card): no pick kept.
        let tried = vec![(inst(11_990, Variant::Vulkan), ok(80.0))];
        assert!(matches!(decide(&ok(78.0), false, &tried, false, "card", 5), Some(Decision::Switch { pick: None, .. })));
        // A build-to-build regression on the same variant: refused.
        let tried = vec![(inst(11_990, Variant::Cuda13), ok(200.0))];
        match decide(&ok(269.0), true, &tried, true, "card", 5).unwrap() {
            Decision::Refuse { build, why, keys, .. } => {
                assert_eq!((build, why), (Build(11_990), Why::Slower));
                assert_eq!(keys, vec!["b11990-cuda13".to_string()]);
            }
            d => panic!("{d:?}"),
        }
        // Both fail: refused for both, the first's reason.
        let tried = vec![(inst(11_990, Variant::Cuda13), Trial::default()), (inst(11_990, Variant::Vulkan), Trial { tool: Some(false), ..ok(170.0) })];
        assert!(matches!(decide(&lf, true, &tried, true, "card", 5), Some(Decision::Refuse { why: Why::DidntStart, ref keys, .. }) if keys.len() == 2));
        assert_eq!(decide(&lf, true, &[], true, "card", 5), None);
    }
}


//! Background update checks — a running Yap notices new releases by itself.
//!
//! Yap autostarts and lives in the tray for days, but it used to check for an
//! update exactly once: when the (hidden) Settings webview loaded at launch.
//! Nobody heard about a release until they happened to restart. This module
//! owns the whole flow Rust-side instead of in a webview that is usually
//! hidden (and timer-throttled by Chromium while it is):
//!
//! - **Cadence** — OpenWhispr's `updater.js` port (startup check + every 4 h
//!   from the main process): the first check ~30–60 s after launch, then every
//!   4 h ± 20 min. The scheduler wakes at least every 5 minutes and compares
//!   the *wall clock*, so a laptop that slept through a due check catches up
//!   minutes after waking. Showing the main window also checks when the last
//!   check is over an hour old; a failed check retries after 30 minutes.
//! - **Background download** (what Chrome, Slack, VS Code and Squirrel apps
//!   such as Wispr Flow do): installed builds fetch the signed installer as
//!   soon as a newer version shows up — unless the connection is metered — so
//!   "Restart to update" is instant. Nothing installs without a click.
//! - **Telling the user, once** — every surface reads one snapshot
//!   ([`Status`], via `update_status` + the `yap-update` event): the tray (a
//!   "Restart to update" item, the tooltip and a dot on the icon), Settings →
//!   About, the Settings badge, the status bar and, while the main window is
//!   visible, a sticky toast. With the window hidden it is one silent Windows
//!   notification instead, with Yap's logo (Do Not Disturb applies; if Windows
//!   won't show it, the toast waits for the window) — and a download the user
//!   asked for while hidden gets one with a live progress bar. Each "you have
//!   an update" episode is
//!   announced once: a newer version replacing a pending one stays quiet, a
//!   single reminder follows after 3 days, and the next announcement waits
//!   until the user has actually updated. A check the user starts (About, the
//!   status bar, the tray) shows its result where they asked, and counts as
//!   that announcement: no toast or Windows notification repeats it.
//! - **Installing never interrupts dictation** — a restart requested while a
//!   dictation is recording/transcribing runs right after it; a meeting
//!   recording or a model download refuses with an explanation. On Windows
//!   the installer step force-exits Yap (`std::process::exit`, skipping the
//!   `RunEvent::Exit` handler), so the updater's `on_before_exit` hook runs
//!   the same cleanup as a normal exit first ([`crate::shutdown_cleanup`]).
//! - **Portable** builds can't replace themselves: they're told and pointed at
//!   the GitHub release. **Dev** builds never check on their own — unless the
//!   debug-only `YAP_UPDATE_TEST_*` variables below aim them at a local
//!   `latest.json`.
//!
//! Privacy: checks only ever fetch the GitHub Releases `latest.json` named in
//! tauri.conf.json (tauri.nightly.conf.json for the nightly channel) — never
//! the account service.

use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::{Update, UpdaterExt};
use tauri_plugin_window_state::AppHandleExt;

/// First automatic check after launch (+ up to [`FIRST_CHECK_JITTER`]). Late
/// enough to stay out of the way of login-time startup and model warm-up.
const FIRST_CHECK_DELAY: u64 = 30;
const FIRST_CHECK_JITTER: u64 = 30;
/// Then every 4 h (OpenWhispr's interval) ± [`INTERVAL_JITTER`].
const CHECK_INTERVAL: u64 = 4 * 3600;
const INTERVAL_JITTER: u64 = 20 * 60;
/// A check or download that failed (offline at login, captive portal, a
/// GitHub hiccup) tries again this much later.
const RETRY_DELAY: u64 = 30 * 60;
/// The scheduler's longest sleep — bounds how late a due check runs after the
/// machine wakes up.
const TICK: u64 = 5 * 60;
/// Showing the main window checks again when the last check is older.
const STALE_ON_SHOW: u64 = 3600;
/// The one reminder for an update the user hasn't installed yet.
const REMIND_AFTER: u64 = 3 * 86_400;
/// Pause between a dictation finishing and a deferred restart, so the paste
/// lands and back-to-back dictations aren't cut off.
const RESTART_GRACE: Duration = Duration::from_millis(2500);

const RELEASES_URL: &str = "https://github.com/nayballs/Yap/releases";

/// Event carrying a fresh [`Status`] snapshot.
const EVENT: &str = "yap-update";
/// Event carrying a "can't restart right now" message for the in-app toast.
const EVENT_BLOCKED: &str = "yap-update-blocked";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    /// Nothing pending (or not checked yet).
    Idle,
    Checking,
    /// A newer version exists but isn't downloaded: portable builds, a
    /// metered connection, or a download that failed (retried later).
    Available,
    Downloading,
    /// Downloaded and signature-verified — one click from installed.
    Ready,
    Installing,
}

/// What a manual "Check for updates" found (the UI's feedback).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Found,
    UpToDate,
    Error,
    /// A check, download or install was already running — nothing new to say.
    Busy,
}

/// The snapshot every surface renders (camelCase for the frontend store).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    status: Phase,
    /// The newer version (empty while there's none).
    version: String,
    current_version: String,
    /// Release notes from `latest.json` (may be empty).
    notes: String,
    date: String,
    /// Download progress, 0–100.
    progress: u8,
    /// Why the last download/install attempt failed (empty when fine).
    error: String,
    /// A restart was requested mid-dictation; it runs once Yap is idle.
    deferred: bool,
    /// A restart was requested before the download finished.
    install_queued: bool,
    /// The background download was skipped on a metered connection.
    metered: bool,
    portable: bool,
    last_checked: Option<u64>,
    release_url: String,
    /// An announcement for the in-app toast to show now — only set while the
    /// main window is on screen (Rust decides; WebView2 keeps reporting
    /// `visibilityState: visible` for a hidden window). Acknowledged with
    /// `update_ack`.
    announce: Option<Announce>,
    /// Yap just restarted into this version from `updatedFrom`, with the main
    /// window brought back (one-shot, acknowledged with `update_ack_updated`).
    updated_from: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Announce {
    version: String,
    reminder: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckResult {
    outcome: Outcome,
    error: String,
    status: Status,
}

struct Inner {
    phase: Phase,
    version: String,
    notes: String,
    date: String,
    progress: u8,
    error: String,
    deferred: bool,
    install_queued: bool,
    metered: bool,
    last_checked: Option<u64>,
    /// When the scheduler runs the next automatic check (unix secs).
    next_check: u64,
    /// The release the last check found (carries the download URL + signature).
    update: Option<Update>,
    /// The verified installer for `version` (~16 MB, held until installed).
    payload: Option<Vec<u8>>,
    updated_from: Option<String>,
    /// The announcement last handed to the visible window, so the scheduler
    /// doesn't re-send it every tick while the toast is up.
    offered: Option<Announce>,
    /// An announcement Windows refused to show — not retried every tick; the
    /// in-app toast picks it up when the window next shows.
    native_refused: Option<Announce>,
    /// The person asked for a check while an automatic one was running: its
    /// result counts as theirs (shown in About, no toast).
    manual_joined: bool,
}

impl Inner {
    const fn new() -> Self {
        Self {
            phase: Phase::Idle,
            version: String::new(),
            notes: String::new(),
            date: String::new(),
            progress: 0,
            error: String::new(),
            deferred: false,
            install_queued: false,
            metered: false,
            last_checked: None,
            next_check: 0,
            update: None,
            payload: None,
            updated_from: None,
            offered: None,
            native_refused: None,
            manual_joined: false,
        }
    }
}

static STATE: Mutex<Inner> = Mutex::new(Inner::new());

/// Small persistent record (`<data>/updates.json`) — what was announced, and
/// the restart marker that lets the relaunched Yap confirm the update.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Record {
    /// The version last announced (toast or Windows notification)…
    announced_version: String,
    /// …and the Yap version running at the time. The episode lasts until Yap
    /// runs a different version, i.e. until the user has updated.
    announced_for: String,
    announced_at: u64,
    reminded: bool,
    last_checked: Option<u64>,
    /// Written right before the installer runs: the version being left, when,
    /// and whether the main window was open (it comes back after the restart).
    restart_from: String,
    restart_at: u64,
    restart_show_window: bool,
}

static RECORD: Mutex<Option<Record>> = Mutex::new(None);

fn lock() -> std::sync::MutexGuard<'static, Inner> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A random offset in `0..max` seconds, so installs don't all hit GitHub at
/// the same moment.
fn jitter(max: u64) -> u64 {
    let mut raw = [0u8; 8];
    if max == 0 || getrandom::fill(&mut raw).is_err() {
        return max / 2;
    }
    u64::from_le_bytes(raw) % max
}

fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn release_url(version: &str) -> String {
    if version.is_empty() {
        format!("{RELEASES_URL}/latest")
    } else if version.contains("-nightly") {
        format!("{RELEASES_URL}/tag/nightly")
    } else {
        format!("{RELEASES_URL}/tag/v{version}")
    }
}

// ---- debug-only test hooks ---------------------------------------------------
//
// Dev builds have no release to compare against, so they never check on their
// own. To exercise the flow locally (debug builds only — compiled out of
// releases), serve a `latest.json` announcing e.g. 0.1.2 and set:
//   YAP_UPDATE_TEST_ENDPOINT=http://127.0.0.1:8799/latest.json  (enables checks)
//   YAP_UPDATE_TEST_PUBKEY=<minisign public key>  (optional: a payload signed
//       with your own `tauri signer` test key then downloads + verifies)
//   YAP_UPDATE_TEST_PORTABLE=0|1, YAP_UPDATE_TEST_METERED=0|1  (force a path)
//   YAP_UPDATE_TEST_FIRST_CHECK=<secs>  (the first automatic check; default 5)
// The installer itself never runs from a dev build (see `begin_install`).

#[cfg(debug_assertions)]
fn test_endpoint() -> Option<url::Url> {
    std::env::var("YAP_UPDATE_TEST_ENDPOINT")
        .ok()
        .and_then(|s| url::Url::parse(s.trim()).ok())
}

#[cfg(not(debug_assertions))]
fn test_endpoint() -> Option<url::Url> {
    None
}

fn test_first_check() -> Option<u64> {
    #[cfg(debug_assertions)]
    if let Ok(v) = std::env::var("YAP_UPDATE_TEST_FIRST_CHECK") {
        return v.trim().parse().ok();
    }
    None
}

fn test_flag(_name: &str) -> Option<bool> {
    #[cfg(debug_assertions)]
    if let Ok(v) = std::env::var(_name) {
        return Some(v.trim() == "1");
    }
    None
}

/// Automatic checks: release builds, or a dev build pointed at a test feed.
fn scheduling_allowed() -> bool {
    !cfg!(debug_assertions) || test_endpoint().is_some()
}

fn portable() -> bool {
    test_flag("YAP_UPDATE_TEST_PORTABLE").unwrap_or_else(crate::portable::is_portable)
}

fn metered() -> bool {
    if let Some(forced) = test_flag("YAP_UPDATE_TEST_METERED") {
        return forced;
    }
    #[cfg(windows)]
    {
        use windows::Networking::Connectivity::{NetworkCostType, NetworkInformation};
        let Ok(cost) =
            NetworkInformation::GetInternetConnectionProfile().and_then(|p| p.GetConnectionCost())
        else {
            return false;
        };
        cost.Roaming().unwrap_or(false)
            || cost.OverDataLimit().unwrap_or(false)
            || matches!(
                cost.NetworkCostType(),
                Ok(t) if t == NetworkCostType::Fixed || t == NetworkCostType::Variable
            )
    }
    #[cfg(not(windows))]
    false
}

// ---- persistent record -------------------------------------------------------

fn record_path() -> std::path::PathBuf {
    crate::config::data_dir().join("updates.json")
}

fn record() -> Record {
    let mut guard = RECORD.lock().unwrap_or_else(|p| p.into_inner());
    guard
        .get_or_insert_with(|| {
            let path = record_path();
            match std::fs::read_to_string(&path) {
                Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| {
                    crate::config::quarantine_corrupt(&path);
                    Record::default()
                }),
                Err(_) => Record::default(),
            }
        })
        .clone()
}

fn update_record(f: impl FnOnce(&mut Record)) {
    let mut rec = record();
    f(&mut rec);
    match serde_json::to_string_pretty(&rec) {
        Ok(json) => {
            let _ = std::fs::create_dir_all(crate::config::data_dir());
            if let Err(e) = crate::config::atomic_write(&record_path(), &json) {
                tracing::warn!("updates: couldn't save updates.json: {}", e);
            }
        }
        Err(e) => tracing::warn!("updates: couldn't serialize updates.json: {}", e),
    }
    *RECORD.lock().unwrap_or_else(|p| p.into_inner()) = Some(rec);
}

// ---- snapshot / announcements --------------------------------------------------

/// The announcement due right now, if any (see the module docs for the policy).
fn announce_due(s: &Inner, rec: &Record) -> Option<Announce> {
    let actionable = match s.phase {
        Phase::Ready => true,
        // Nothing to download (portable) or waiting for the user's OK (metered).
        Phase::Available => portable() || s.metered,
        _ => false,
    };
    if !actionable || s.deferred || s.install_queued {
        return None;
    }
    let version = s.version.clone();
    if rec.announced_version.is_empty() || rec.announced_for != current_version() {
        return Some(Announce { version, reminder: false });
    }
    if !rec.reminded && now() >= rec.announced_at.saturating_add(REMIND_AFTER) {
        return Some(Announce { version, reminder: true });
    }
    None
}

fn snapshot_of(s: &Inner) -> Status {
    Status {
        status: s.phase,
        version: s.version.clone(),
        current_version: current_version().to_string(),
        notes: s.notes.clone(),
        date: s.date.clone(),
        progress: s.progress,
        error: s.error.clone(),
        deferred: s.deferred,
        install_queued: s.install_queued,
        metered: s.metered,
        portable: portable(),
        last_checked: s.last_checked,
        release_url: release_url(&s.version),
        announce: s.offered.clone(),
        updated_from: s.updated_from.clone(),
    }
}

fn snapshot() -> Status {
    snapshot_of(&lock())
}

/// Broadcast the current snapshot and bring the tray (and a download's
/// Windows notification) in line.
fn changed(app: &AppHandle) {
    let _ = app.emit(EVENT, snapshot());
    crate::tray::refresh(app);
    #[cfg(windows)]
    notify::sync_progress(app);
}

/// Whether Yap may post Windows notifications. Portable Yap has no Start-menu
/// shortcut carrying its AppUserModelID, so Windows would drop them silently
/// (the in-app toast covers it); an e2e test run posts nothing to the
/// developer's notification center.
fn native_notifications() -> bool {
    !portable() && !crate::e2e::active()
}

fn mark_announced(a: &Announce) {
    update_record(|r| {
        if a.reminder {
            r.reminded = true;
        } else {
            r.announced_version = a.version.clone();
            r.announced_for = current_version().to_string();
            r.announced_at = now();
            r.reminded = false;
        }
    });
    lock().offered = None;
}

/// The main window is on screen (not hidden, not minimized).
fn main_window_visible(app: &AppHandle) -> bool {
    app.get_webview_window("settings").is_some_and(|w| {
        w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false)
    })
}

/// Announce a due update through whichever channel can reach the user now:
/// the in-app toast when the main window is on screen (it doesn't interrupt
/// anything), otherwise one Windows notification — held back while Yap is
/// busy (never mid-dictation); the scheduler tick, the pipeline's idle
/// transition and the window gaining focus try again.
fn maybe_announce(app: &AppHandle) {
    let due = announce_due(&lock(), &record());
    let Some(a) = due else { return };
    if main_window_visible(app) {
        // The snapshot carries `announce`; the page shows the toast and acks.
        let fresh = {
            let mut s = lock();
            let fresh = s.offered.as_ref() != Some(&a);
            s.offered = Some(a);
            fresh
        };
        if fresh {
            let _ = app.emit(EVENT, snapshot());
        }
        return;
    }
    if busy_reason(app).is_some() {
        return;
    }
    #[cfg(windows)]
    {
        if !native_notifications() || lock().native_refused.as_ref() == Some(&a) {
            return;
        }
        match notify::show(app, &a) {
            Ok(()) => {
                tracing::info!(version = %a.version, reminder = a.reminder, "updates: announced with a Windows notification");
                mark_announced(&a);
                let _ = app.emit(EVENT, snapshot());
            }
            // Notifications switched off for Yap (or WinRT failed): the
            // in-app toast announces it the next time the main window shows.
            Err(e) => {
                tracing::info!("updates: no Windows notification ({}); the in-app toast will announce", e);
                lock().native_refused = Some(a);
            }
        }
    }
}

// ---- busy guard ------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Busy {
    Dictation,
    Meeting,
    ModelDownload,
}

/// What would a restart interrupt right now? Locks the pipeline mutex, so it
/// must never run inside a `yap-state` listener (those can fire while the
/// emitter holds that lock) — hence the spawns in [`on_pipeline_state`].
fn busy_reason(app: &AppHandle) -> Option<Busy> {
    if crate::meeting::is_recording() {
        return Some(Busy::Meeting);
    }
    if crate::commands::model_download_in_flight() {
        return Some(Busy::ModelDownload);
    }
    let state = app.try_state::<crate::AppState>()?;
    let guard = state.pipeline.lock().ok()?;
    guard
        .as_ref()
        .filter(|p| p.is_busy())
        .map(|_| Busy::Dictation)
}

// ---- check / download / install ----------------------------------------------------

/// An updater for this build's channel. The `on_before_exit` hook is the
/// important part: on Windows `Update::install` launches the installer and
/// then calls `std::process::exit(0)`, so Yap's `RunEvent::Exit` cleanup never
/// runs — do it here (the plugin's default hook only clears trays/resources).
fn build_updater(app: &AppHandle) -> Result<tauri_plugin_updater::Updater, String> {
    let handle = app.clone();
    #[allow(unused_mut)]
    let mut builder = app
        .updater_builder()
        .timeout(Duration::from_secs(30))
        .on_before_exit(move || {
            tracing::info!("updates: launching the installer — cleaning up first");
            crate::shutdown_cleanup();
            let _ = handle.save_window_state(crate::window_state_flags());
            handle.cleanup_before_exit();
        });
    #[cfg(debug_assertions)]
    if let Some(url) = test_endpoint() {
        builder = builder.endpoints(vec![url]).map_err(|e| e.to_string())?;
        if let Ok(key) = std::env::var("YAP_UPDATE_TEST_PUBKEY") {
            builder = builder.pubkey(key.trim());
        }
    }
    builder.build().map_err(|e| e.to_string())
}

/// A short, human reason for a failed check (the raw error goes to the log).
fn friendly_check_error(e: &str) -> String {
    let lower = e.to_ascii_lowercase();
    if ["error sending request", "dns", "timed out", "connect", "network"]
        .iter()
        .any(|k| lower.contains(k))
    {
        "Couldn't reach GitHub. Check your connection and try again.".into()
    } else {
        "Couldn't check for updates right now. Try again later.".into()
    }
}

async fn run_check(app: &AppHandle, manual: bool) -> (Outcome, String) {
    let previous = {
        let mut s = lock();
        if matches!(s.phase, Phase::Checking | Phase::Downloading | Phase::Installing) {
            // The person asked while something was already under way (e.g. the
            // window-shown check the tray's "Check for updates…" also sets
            // off): what it finds is their answer, shown in About, so it
            // mustn't also be announced with a toast.
            if manual {
                let phase = s.phase;
                if phase == Phase::Checking {
                    s.manual_joined = true;
                } else if phase == Phase::Downloading {
                    let version = s.version.clone();
                    drop(s);
                    mark_announced(&Announce { version, reminder: false });
                }
            }
            return (Outcome::Busy, String::new());
        }
        let previous = s.phase;
        s.phase = Phase::Checking;
        s.manual_joined = false;
        previous
    };
    changed(app);

    let result = match build_updater(app) {
        Ok(updater) => updater.check().await.map_err(|e| e.to_string()),
        Err(e) => Err(e),
    };
    let checked_at = now();
    // Any successful check (scheduled, window-shown or manual) resets the
    // clock for the next automatic one.
    let next_due = checked_at + CHECK_INTERVAL - INTERVAL_JITTER + jitter(2 * INTERVAL_JITTER);

    let outcome = match result {
        Ok(Some(update)) => {
            let version = update.version.clone();
            // A check the user asked for shows its result where they asked
            // (Settings → About, the status bar): that's this update's
            // announcement, so neither a toast nor a Windows notification
            // repeats it once the background download is done.
            let joined = std::mem::take(&mut lock().manual_joined);
            if manual || joined {
                mark_announced(&Announce { version: version.clone(), reminder: false });
            }
            // Background download: installed builds, unless the connection is
            // metered (then it waits for the user's "Download and restart").
            let installable = !portable();
            let on_metered = installable && metered();
            let download = {
                let mut s = lock();
                // Same version already downloaded → keep the payload.
                let same = previous == Phase::Ready && s.version == version && s.payload.is_some();
                if !same {
                    tracing::info!(current = current_version(), new = %version, "updates: a newer version is available");
                    s.payload = None;
                    s.progress = 0;
                    s.deferred = false;
                }
                s.phase = if same { Phase::Ready } else { Phase::Available };
                s.version = version;
                s.notes = update.body.clone().unwrap_or_default().trim().to_string();
                s.date = update.date.map(|d| d.date().to_string()).unwrap_or_default();
                s.error.clear();
                s.update = Some(update);
                s.last_checked = Some(checked_at);
                // Set before the download starts: a failing download moves
                // it up to a retry.
                s.next_check = next_due;
                s.metered = !same && on_metered;
                !same && installable && !on_metered
            };
            if download {
                tauri::async_runtime::spawn(download_update(app.clone()));
            }
            (Outcome::Found, String::new())
        }
        Ok(None) => {
            {
                let mut s = lock();
                // Nothing newer than this build (a pulled release included).
                *s = Inner {
                    last_checked: Some(checked_at),
                    next_check: next_due,
                    updated_from: s.updated_from.take(),
                    ..Inner::new()
                };
            }
            tracing::info!(version = current_version(), manual, "updates: up to date");
            (Outcome::UpToDate, String::new())
        }
        Err(e) => {
            tracing::warn!(manual, "updates: check failed: {}", e);
            // Keep whatever was pending (an Available/Ready update stays).
            lock().phase = previous;
            (Outcome::Error, friendly_check_error(&e))
        }
    };
    if outcome.0 != Outcome::Error {
        update_record(|r| r.last_checked = Some(checked_at));
    }
    changed(app);
    maybe_announce(app);
    outcome
}

/// Download + verify the found release, then mark it Ready (and install it
/// straight away if the user already asked to restart).
async fn download_update(app: AppHandle) {
    let update = {
        let mut s = lock();
        let Some(update) = s.update.clone().filter(|_| s.phase == Phase::Available) else {
            return;
        };
        s.phase = Phase::Downloading;
        s.progress = 0;
        s.error.clear();
        update
    };
    changed(&app);
    tracing::info!(version = %update.version, "updates: downloading");

    let mut received: u64 = 0;
    let mut last_emit = Instant::now();
    let result = update
        .download(
            |chunk, total| {
                received += chunk as u64;
                let Some(total) = total.filter(|t| *t > 0) else { return };
                let pct = (received.saturating_mul(100) / total).min(100) as u8;
                lock().progress = pct;
                #[cfg(windows)]
                notify::progress(&app, pct);
                // ~4 updates a second is plenty for a progress bar.
                if last_emit.elapsed() >= Duration::from_millis(250) {
                    last_emit = Instant::now();
                    let _ = app.emit(EVENT, snapshot());
                }
            },
            || {},
        )
        .await;

    match result {
        Ok(bytes) => {
            // `install_queued` stays set until request_install picks it up, so
            // this Ready snapshot doesn't announce what's about to install.
            let install_now = {
                let mut s = lock();
                if s.version != update.version {
                    return; // superseded meanwhile
                }
                s.payload = Some(bytes);
                s.phase = Phase::Ready;
                s.progress = 100;
                s.install_queued
            };
            tracing::info!(version = %update.version, "updates: downloaded and verified");
            changed(&app);
            if install_now {
                if let Err(msg) = request_install(&app) {
                    let _ = app.emit(EVENT_BLOCKED, msg);
                    maybe_announce(&app);
                }
            } else {
                maybe_announce(&app);
            }
        }
        Err(e) => {
            tracing::warn!(version = %update.version, "updates: download failed: {}", e);
            {
                let mut s = lock();
                s.phase = Phase::Available;
                s.progress = 0;
                s.install_queued = false;
                s.error = "The download didn't finish. Yap will try again later.".into();
                s.next_check = now() + RETRY_DELAY;
            }
            changed(&app);
        }
    }
}

const PORTABLE_MESSAGE: &str =
    "Portable Yap can't update itself. Download the new version from GitHub.";

/// "Restart to update" from any surface (toast, About, tray, notification).
/// Downloads first if needed, waits for a running dictation, refuses while a
/// meeting is recording or a model is downloading. `Err` carries a message
/// for the user.
pub fn request_install(app: &AppHandle) -> Result<(), String> {
    if portable() {
        return Err(PORTABLE_MESSAGE.into());
    }
    let phase = lock().phase;
    match phase {
        Phase::Ready => {}
        Phase::Available => {
            lock().install_queued = true;
            show_download_progress(app);
            tauri::async_runtime::spawn(download_update(app.clone()));
            changed(app);
            return Ok(());
        }
        Phase::Downloading => {
            lock().install_queued = true;
            show_download_progress(app);
            changed(app);
            return Ok(());
        }
        Phase::Installing => return Ok(()),
        Phase::Idle | Phase::Checking => {
            return Err("There's no update ready to install.".into());
        }
    }
    let refused = match busy_reason(app) {
        Some(Busy::Meeting) => {
            "Yap is recording a meeting. Stop the recording first, then restart to update."
        }
        Some(Busy::ModelDownload) => {
            "A speech model is still downloading. Restart to update once it's done."
        }
        Some(Busy::Dictation) => {
            {
                let mut s = lock();
                s.deferred = true;
                s.install_queued = false;
            }
            tracing::info!("updates: restart requested mid-dictation — waiting until Yap is idle");
            changed(app);
            return Ok(());
        }
        None => {
            begin_install(app);
            return Ok(());
        }
    };
    if std::mem::take(&mut lock().install_queued) {
        changed(app);
    }
    Err(refused.into())
}

/// A download the user asked for (a metered connection's "Download and
/// restart", the tray's "Download and install") with the main window hidden:
/// show it as a Windows notification with a live progress bar. With the
/// window on screen the in-app toast shows the same progress. (Background
/// downloads stay silent; Yap speaks up once they're ready.)
fn show_download_progress(app: &AppHandle) {
    #[cfg(windows)]
    if native_notifications() && !main_window_visible(app) {
        let (version, pct) = {
            let s = lock();
            (s.version.clone(), s.progress)
        };
        if let Err(e) = notify::show_progress(app, &version, pct) {
            tracing::info!("updates: no download notification ({})", e);
        }
    }
    #[cfg(not(windows))]
    let _ = app;
}

fn begin_install(app: &AppHandle) {
    let show_window = main_window_visible(app);
    let (update, bytes) = {
        let mut s = lock();
        if s.phase != Phase::Ready || s.update.is_none() || s.payload.is_none() {
            return;
        }
        let (Some(update), Some(bytes)) = (s.update.clone(), s.payload.take()) else {
            return;
        };
        s.phase = Phase::Installing;
        s.deferred = false;
        s.install_queued = false;
        s.error.clear();
        (update, bytes)
    };
    changed(app);
    #[cfg(windows)]
    notify::remove(app);

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Let the toast and tray say "Restarting…" before the window goes.
        tokio::time::sleep(Duration::from_millis(700)).await;
        update_record(|r| {
            r.restart_from = current_version().to_string();
            r.restart_at = now();
            r.restart_show_window = show_window;
        });
        tracing::info!(from = current_version(), to = %update.version, "updates: installing — Yap restarts when the installer finishes");

        if cfg!(debug_assertions) {
            // Never run an installer from a dev build: it would replace the
            // INSTALLED Yap. Everything up to this point was real.
            tracing::warn!("updates: debug build — not running the installer");
            update_record(|r| r.restart_from.clear());
            fail_install(&app, Some(bytes), "Debug build: the installer wasn't run (it would replace the installed Yap).");
            return;
        }

        let outcome = tauri::async_runtime::spawn_blocking(move || {
            let result = update.install(&bytes);
            (result, bytes)
        })
        .await;
        match outcome {
            // Windows never gets here: the installer step exited the process.
            // Elsewhere the new version is in place — relaunch into it.
            Ok((Ok(()), _)) => app.restart(),
            Ok((Err(e), bytes)) => {
                tracing::error!("updates: install failed: {}", e);
                update_record(|r| r.restart_from.clear());
                fail_install(&app, Some(bytes), "Couldn't start the installer. Try again, or download Yap from GitHub.");
            }
            Err(e) => {
                tracing::error!("updates: install task failed: {}", e);
                update_record(|r| r.restart_from.clear());
                fail_install(&app, None, "Couldn't start the installer. Try again, or download Yap from GitHub.");
            }
        }
    });
}

fn fail_install(app: &AppHandle, bytes: Option<Vec<u8>>, message: &str) {
    {
        let mut s = lock();
        s.phase = if bytes.is_some() { Phase::Ready } else { Phase::Available };
        s.payload = bytes;
        s.error = message.to_string();
    }
    changed(app);
}

fn open_release(app: &AppHandle) {
    let url = release_url(&lock().version);
    if let Err(e) = app.opener().open_url(url, None::<&str>) {
        tracing::warn!("updates: couldn't open the release page: {}", e);
    }
}

// ---- lifecycle ---------------------------------------------------------------------

/// Start the scheduler (and confirm a just-finished update). Call at the end
/// of `setup`, after the hidden webviews are initialized.
pub fn init(app: &AppHandle) {
    let rec = record();
    lock().last_checked = rec.last_checked;
    if !rec.restart_from.is_empty() {
        let fresh = now().saturating_sub(rec.restart_at) < 3600;
        if fresh && rec.restart_from != current_version() {
            tracing::info!(from = %rec.restart_from, to = current_version(), "updates: updated");
            // The window was open when the user hit "Restart to update" —
            // bring it back so the update visibly finishes where it started,
            // with an "Updated to X" toast. (Restarted from the tray or a
            // notification with the window hidden: no toast nobody would see.)
            if rec.restart_show_window {
                lock().updated_from = Some(rec.restart_from.clone());
                let _ = crate::commands::show_settings(app);
            }
        }
        update_record(|r| {
            r.restart_from.clear();
            r.restart_show_window = false;
        });
    }

    if !scheduling_allowed() {
        tracing::info!("updates: dev build — no automatic checks (YAP_UPDATE_TEST_ENDPOINT enables them)");
        return;
    }
    let first = if test_endpoint().is_some() {
        test_first_check().unwrap_or(5)
    } else {
        FIRST_CHECK_DELAY + jitter(FIRST_CHECK_JITTER)
    };
    lock().next_check = now() + first;
    tauri::async_runtime::spawn(scheduler(app.clone()));
}

async fn scheduler(app: AppHandle) {
    loop {
        let due = lock().next_check;
        let wait = due.saturating_sub(now()).min(TICK);
        if wait > 0 {
            tokio::time::sleep(Duration::from_secs(wait)).await;
            // An announcement held back earlier (dictation in progress, or a
            // reminder coming due) gets another chance every tick.
            maybe_announce(&app);
            continue;
        }
        let busy = matches!(
            lock().phase,
            Phase::Checking | Phase::Downloading | Phase::Installing
        );
        if busy || !crate::config::load().update_checks_enabled {
            // Stay due: turning the toggle back on checks within one tick.
            tokio::time::sleep(Duration::from_secs(TICK)).await;
            continue;
        }
        // A successful check schedules the next one itself (run_check).
        if run_check(&app, false).await.0 == Outcome::Error {
            lock().next_check = now() + RETRY_DELAY;
        }
    }
}

/// Pipeline state hook (from the `yap-state` listener in lib.rs): a finished
/// dictation releases a deferred restart or a held-back announcement.
pub fn on_pipeline_state(app: &AppHandle, state: &str) {
    if matches!(state, "recording" | "processing" | "processing-slow") {
        return;
    }
    let pending = {
        let s = lock();
        s.deferred || announce_due(&s, &record()).is_some()
    };
    if !pending {
        return;
    }
    // Spawned: busy_reason() locks the pipeline, which the emitter of this
    // event may be holding right now.
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(RESTART_GRACE).await;
        if busy_reason(&app).is_some() {
            return; // another dictation already started — its end retries
        }
        if lock().deferred {
            if let Err(msg) = request_install(&app) {
                lock().deferred = false;
                changed(&app);
                let _ = app.emit(EVENT_BLOCKED, msg);
            }
        } else {
            maybe_announce(&app);
        }
    });
}

// ---- tray --------------------------------------------------------------------------

/// The tray's update item for the idle menu: `(menu id, label, enabled)`.
pub fn tray_item() -> Option<(&'static str, String, bool)> {
    let s = lock();
    let v = &s.version;
    match s.phase {
        Phase::Ready if s.deferred => Some(("update_status", "Restarting after this dictation…".into(), false)),
        Phase::Ready => Some(("update_install", format!("Restart to update to {v}"), true)),
        Phase::Available if portable() => Some(("update_get", format!("Get Yap {v} on GitHub…"), true)),
        Phase::Available => Some(("update_install", format!("Download and install {v}"), true)),
        Phase::Downloading => Some(("update_status", format!("Downloading update {v}…"), false)),
        Phase::Installing => Some(("update_status", "Restarting to update…".into(), false)),
        Phase::Idle | Phase::Checking => None,
    }
}

/// A dot on the tray icon: there's an update the user can act on.
pub fn tray_badge() -> bool {
    let s = lock();
    s.phase == Phase::Ready || (s.phase == Phase::Available && (portable() || s.metered))
}

/// Appended to the tray tooltip ("Yap v0.1.1 · Update 0.1.2 ready").
pub fn tray_note() -> Option<String> {
    let s = lock();
    match s.phase {
        Phase::Ready => Some(format!("Update {} ready", s.version)),
        Phase::Available | Phase::Downloading => Some(format!("Update {} available", s.version)),
        _ => None,
    }
}

/// Tray menu clicks on the update item.
pub fn on_tray_menu(app: &AppHandle, id: &str) {
    match id {
        "update_install" => {
            if let Err(msg) = request_install(app) {
                let _ = crate::commands::show_settings(app);
                let _ = app.emit(EVENT_BLOCKED, msg);
            }
        }
        "update_get" => open_release(app),
        _ => {}
    }
}

/// The tray's "Check for updates…": open Settings → About and let the page
/// run a manual check there, so the result shows where the user is looking.
pub fn on_tray_check(app: &AppHandle) {
    let _ = crate::commands::show_settings(app);
    let _ = app.emit("yap-open-settings", "about");
    let _ = app.emit("check-for-updates", ());
}

// ---- commands ------------------------------------------------------------------------

/// The current snapshot.
#[tauri::command]
pub fn update_status() -> Status {
    snapshot()
}

/// A manual "Check for updates" (works even with automatic checks off).
#[tauri::command]
pub async fn update_check(app: AppHandle) -> CheckResult {
    let (outcome, error) = run_check(&app, true).await;
    CheckResult { outcome, error, status: snapshot() }
}

/// "Restart to update" / "Download and restart" from the page. Async so it
/// runs off the main thread (it takes the pipeline lock).
#[tauri::command]
pub async fn update_install(app: AppHandle) -> Result<Status, String> {
    request_install(&app)?;
    Ok(snapshot())
}

/// The main window gained focus (shown from the tray, a notification, a
/// second launch — or just clicked): retire a Windows notification it now
/// supersedes, check again if the last check is over an hour old, and hand a
/// pending announcement to the in-app toast. A window event, not a page
/// event: WebView2 keeps `visibilityState: visible` while the window is
/// hidden, so the page can't tell when it's shown.
pub fn on_main_window_focused(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        #[cfg(windows)]
        notify::remove(&app);
        let stale = {
            let s = lock();
            // Before the first scheduled check (seconds after launch) there's
            // nothing to catch up on.
            let first_pending = s.last_checked.is_none() && s.next_check > now();
            !first_pending
                && s.last_checked.is_none_or(|t| now().saturating_sub(t) >= STALE_ON_SHOW)
                && matches!(s.phase, Phase::Idle | Phase::Available | Phase::Ready)
        };
        if stale && scheduling_allowed() && crate::config::load().update_checks_enabled {
            run_check(&app, false).await; // ends with maybe_announce
        } else {
            maybe_announce(&app);
        }
    });
}

/// The page showed the announcement toast.
#[tauri::command]
pub fn update_ack(app: AppHandle, version: String, reminder: bool) {
    mark_announced(&Announce { version, reminder });
    let _ = app.emit(EVENT, snapshot());
}

/// The page confirmed the "Updated to X" toast.
#[tauri::command]
pub fn update_ack_updated() {
    lock().updated_from = None;
}

// ---- Windows notification ----------------------------------------------------------------

/// Silent toasts in the Windows notification center for a user whose main
/// window is hidden (PowerToys-style), with Yap's logo: the update announcement
/// (the version, "Restart to update", "Later"), and a live progress bar while
/// a download they asked for runs. Clicking the body opens Settings → About.
/// Windows applies Do Not Disturb / Focus Assist and the per-app notification
/// switch on its own. The WinRT plumbing is shared with the call prompts
/// (`crate::win_toast`).
#[cfg(windows)]
mod notify {
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use tauri::{AppHandle, Emitter};
    use windows::core::HSTRING;
    use windows::UI::Notifications::{NotificationData, ToastNotification};

    use super::{lock, Announce, Phase};
    use crate::win_toast::{esc, logo_xml};

    const TAG: &str = "update";

    /// The live toast, kept so its Activated handler keeps working while it
    /// sits in the notification center.
    static CURRENT: Mutex<Option<ToastNotification>> = Mutex::new(None);
    /// While the download toast is up: when its bar last moved, and to what.
    static PROGRESS: Mutex<Option<(Instant, u8)>> = Mutex::new(None);
    /// Orders the download toast's data updates (Windows drops stale ones).
    static SEQUENCE: AtomicU32 = AtomicU32::new(0);

    /// Post `xml` as Yap's one update toast (it replaces the previous one).
    fn post(app: &AppHandle, xml: &str, data: Option<&NotificationData>) -> Result<(), String> {
        let toast = crate::win_toast::post(app, TAG, xml, data, activated)?;
        *CURRENT.lock().unwrap_or_else(|p| p.into_inner()) = Some(toast);
        Ok(())
    }

    pub fn show(app: &AppHandle, a: &Announce) -> Result<(), String> {
        let ready = lock().phase == Phase::Ready;
        let v = &a.version;
        // Ready → restart; otherwise it's waiting on a metered connection for
        // the user's go-ahead. (Portable builds never get here.)
        let (title, body, button) = if ready {
            (
                format!("Yap {v} is ready"),
                "Restart Yap to finish updating. It only takes a few seconds.",
                "Restart to update",
            )
        } else {
            (
                format!("Yap {v} is available"),
                "You're on a metered connection, so Yap hasn't downloaded it yet.",
                "Download and restart",
            )
        };
        // A card on the Yap bar while it's on screen; "Later" just closes it.
        let card = crate::bar::Card {
            id: TAG.into(),
            icon: "update",
            title: title.clone(),
            body: body.into(),
            primary: Some(crate::bar::CardAction::new("update:install", button)),
            secondary: Some(crate::bar::CardAction::new("", "Later")),
            ..Default::default()
        };
        if crate::bar::show_card(app, card, Box::new(activated)) {
            *PROGRESS.lock().unwrap_or_else(|p| p.into_inner()) = None;
            return Ok(());
        }
        let xml = format!(
            "<toast launch=\"update:open\"><visual><binding template=\"ToastGeneric\">\
             <text>{}</text><text>{}</text>{}</binding></visual><actions>\
             <action content=\"{}\" arguments=\"update:install\" activationType=\"foreground\"/>\
             <action content=\"Later\" arguments=\"dismiss\" activationType=\"system\"/>\
             </actions><audio silent=\"true\"/></toast>",
            esc(&title),
            esc(body),
            logo_xml(),
            esc(button),
        );
        post(app, &xml, None)?;
        *PROGRESS.lock().unwrap_or_else(|p| p.into_inner()) = None;
        Ok(())
    }

    /// The bar's values, bound into the download toast's `<progress>`.
    fn progress_data(pct: u8, status: &str) -> windows::core::Result<NotificationData> {
        let data = NotificationData::new()?;
        let values = data.Values()?;
        let value = format!("{:.2}", f32::from(pct) / 100.0);
        values.Insert(&HSTRING::from("progressValue"), &HSTRING::from(value))?;
        values.Insert(&HSTRING::from("progressText"), &HSTRING::from(format!("{pct}%")))?;
        values.Insert(&HSTRING::from("progressStatus"), &HSTRING::from(status))?;
        data.SetSequenceNumber(SEQUENCE.fetch_add(1, Ordering::SeqCst) + 1)?;
        Ok(data)
    }

    /// A download the user asked for while the main window is hidden: a toast
    /// with a live progress bar, moved by [`progress`], kept in line with the
    /// update state by [`sync_progress`], and gone once the installer starts.
    pub fn show_progress(app: &AppHandle, version: &str, pct: u8) -> Result<(), String> {
        let xml = format!(
            "<toast launch=\"update:open\"><visual><binding template=\"ToastGeneric\">\
             <text>{}</text><text>Yap restarts to finish updating as soon as it's downloaded.</text>{}\
             <progress value=\"{{progressValue}}\" valueStringOverride=\"{{progressText}}\" status=\"{{progressStatus}}\"/>\
             </binding></visual><audio silent=\"true\"/></toast>",
            esc(&format!("Downloading Yap {version}")),
            logo_xml(),
        );
        SEQUENCE.store(0, Ordering::SeqCst);
        let data = progress_data(pct, "Downloading…").map_err(|e| e.message())?;
        post(app, &xml, Some(&data))?;
        *PROGRESS.lock().unwrap_or_else(|p| p.into_inner()) = Some((Instant::now(), pct));
        Ok(())
    }

    fn update_bar(app: &AppHandle, pct: u8, status: &str) {
        let Ok(data) = progress_data(pct, status) else { return };
        crate::win_toast::update(app, TAG, &data);
    }

    /// Move the download toast's bar — at most twice a second, and only while
    /// that toast is up.
    pub fn progress(app: &AppHandle, pct: u8) {
        {
            let mut guard = PROGRESS.lock().unwrap_or_else(|p| p.into_inner());
            let Some((at, last)) = *guard else { return };
            if pct == last || (pct < 100 && at.elapsed() < Duration::from_millis(500)) {
                return;
            }
            *guard = Some((Instant::now(), pct));
        }
        update_bar(app, pct, "Downloading…");
    }

    /// Keep the download toast in line with the update state (called on every
    /// change): full while Yap is about to restart, or waiting for a dictation
    /// to finish first; gone if the download failed or the install was
    /// refused.
    pub fn sync_progress(app: &AppHandle) {
        if PROGRESS.lock().unwrap_or_else(|p| p.into_inner()).is_none() {
            return;
        }
        let (phase, queued, deferred) = {
            let s = lock();
            (s.phase, s.install_queued, s.deferred)
        };
        match phase {
            Phase::Downloading | Phase::Installing => {}
            Phase::Available if queued => {} // the download is about to start
            Phase::Ready if deferred => update_bar(app, 100, "Restarting after your dictation…"),
            Phase::Ready if queued => update_bar(app, 100, "Restarting…"),
            _ => remove(app),
        }
    }

    /// Take our toast out of the notification center (the update is being
    /// installed, or the main window now shows the same thing).
    pub fn remove(app: &AppHandle) {
        crate::bar::dismiss_card(app, TAG);
        *PROGRESS.lock().unwrap_or_else(|p| p.into_inner()) = None;
        let Some(_toast) = CURRENT.lock().unwrap_or_else(|p| p.into_inner()).take() else {
            return;
        };
        crate::win_toast::remove(app, TAG);
    }

    fn activated(app: &AppHandle, arg: &str) {
        tracing::info!(arg, "updates: Windows notification clicked");
        match arg {
            "update:install" => super::on_tray_menu(app, "update_install"),
            // The toast body.
            _ => {
                let _ = crate::commands::show_settings(app);
                let _ = app.emit("yap-open-settings", "about");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready(version: &str) -> Inner {
        Inner {
            phase: Phase::Ready,
            version: version.into(),
            ..Inner::new()
        }
    }

    fn announced(version: &str, at: u64) -> Record {
        Record {
            announced_version: version.into(),
            announced_for: current_version().into(),
            announced_at: at,
            ..Record::default()
        }
    }

    #[test]
    fn announces_once_per_episode_with_one_reminder() {
        let first = |v: &str| Some(Announce { version: v.into(), reminder: false });
        // Never announced → announce.
        assert_eq!(announce_due(&ready("0.1.2"), &Record::default()), first("0.1.2"));
        // Announced in this episode → quiet, also when a newer version replaces it.
        let rec = announced("0.1.2", now());
        assert_eq!(announce_due(&ready("0.1.2"), &rec), None);
        assert_eq!(announce_due(&ready("0.1.3"), &rec), None);
        // Three days on, still not installed → exactly one reminder.
        let old = announced("0.1.2", now() - REMIND_AFTER - 1);
        assert_eq!(
            announce_due(&ready("0.1.3"), &old),
            Some(Announce { version: "0.1.3".into(), reminder: true })
        );
        let reminded = Record { reminded: true, ..old };
        assert_eq!(announce_due(&ready("0.1.3"), &reminded), None);
        // The user updated since (Yap runs another version) → a new episode.
        let earlier = Record { announced_for: "0.0.9".into(), ..reminded };
        assert_eq!(announce_due(&ready("0.1.3"), &earlier), first("0.1.3"));
    }

    #[test]
    fn announces_only_actionable_updates() {
        let none = Record::default();
        // A restart is already on its way.
        let deferred = Inner { deferred: true, ..ready("0.1.2") };
        assert_eq!(announce_due(&deferred, &none), None);
        let queued = Inner { install_queued: true, ..ready("0.1.2") };
        assert_eq!(announce_due(&queued, &none), None);
        // Found but not downloaded yet: wait for Ready…
        let available = Inner { phase: Phase::Available, ..ready("0.1.2") };
        assert_eq!(announce_due(&available, &none), None);
        // …unless it's waiting on the user (metered connection).
        let metered = Inner { metered: true, ..available };
        assert!(announce_due(&metered, &none).is_some());
        for phase in [Phase::Idle, Phase::Checking, Phase::Downloading, Phase::Installing] {
            assert_eq!(announce_due(&Inner { phase, ..ready("0.1.2") }, &none), None);
        }
    }

    #[test]
    fn tray_follows_the_phase() {
        // The only test touching the global state.
        *lock() = ready("0.1.2");
        assert_eq!(
            tray_item(),
            Some(("update_install", "Restart to update to 0.1.2".to_string(), true))
        );
        assert!(tray_badge());
        assert_eq!(tray_note().as_deref(), Some("Update 0.1.2 ready"));

        lock().deferred = true;
        assert_eq!(tray_item().map(|(id, _, enabled)| (id, enabled)), Some(("update_status", false)));

        *lock() = Inner { phase: Phase::Downloading, ..ready("0.1.2") };
        assert_eq!(
            tray_item(),
            Some(("update_status", "Downloading update 0.1.2…".to_string(), false))
        );
        assert!(!tray_badge());

        *lock() = Inner { phase: Phase::Available, metered: true, ..ready("0.1.2") };
        assert_eq!(
            tray_item(),
            Some(("update_install", "Download and install 0.1.2".to_string(), true))
        );
        assert!(tray_badge());

        *lock() = Inner::new();
        assert_eq!(tray_item(), None);
        assert!(!tray_badge());
        assert_eq!(tray_note(), None);
    }

    #[test]
    fn release_pages() {
        assert_eq!(release_url(""), format!("{RELEASES_URL}/latest"));
        assert_eq!(release_url("0.1.2"), format!("{RELEASES_URL}/tag/v0.1.2"));
        assert_eq!(release_url("0.1.1-nightly.109"), format!("{RELEASES_URL}/tag/nightly"));
    }

    #[test]
    fn friendly_errors() {
        assert!(friendly_check_error("error sending request for url (https://github.com/…)")
            .starts_with("Couldn't reach GitHub"));
        assert!(friendly_check_error("Could not fetch a valid release JSON from the remote")
            .starts_with("Couldn't check for updates"));
    }
}

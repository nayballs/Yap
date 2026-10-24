//! The llamafile runtime on disk: which version it is (its marker), a new
//! one swapped in and proven by its first start or rolled back, and the
//! background upgrade when Yap pins a newer one (never under a
//! dictation, one Yap at a time).

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use super::context::{restart_pending, sidecar_busy, wait_until_idle, RESTARTING, RESTART_SETTLE, RESTART_TICK};
use super::models::{allowed_in_test_run, compute_sha256, download_for, in_use, source};
use super::{
    is_running, llm_dir, start_locked, stop, PlannedRestart, MARKER_PREVIOUS, PROBE_COUNTED, RUNTIME_FILENAME,
    RUNTIME_MARKER, RUNTIME_NEXT, RUNTIME_PREVIOUS, RUNTIME_SHA256, RUNTIME_URL, RUNTIME_VERSION, STARTING,
    UNMARKED_RUNTIME_VERSION, WENT_BACK,
};

/// A llamafile version, "0.10.6". Ordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    /// "0.10.6", "v0.10.6", "llamafile v0.10.6" (what `--version` prints).
    /// Pure.
    pub fn parse(s: &str) -> Option<Version> {
        let word = s.split_whitespace().find(|w| w.trim_start_matches('v').starts_with(|c: char| c.is_ascii_digit()))?;
        let mut parts = word.trim_start_matches('v').split('.').map(|p| p.trim().parse::<u32>());
        let v = Version(parts.next()?.ok()?, parts.next()?.ok()?, parts.next().unwrap_or(Ok(0)).ok()?);
        Some(v)
    }

    /// The runtime that brought the buffers and the MTP that Yap's memory
    /// estimate and MTP drafting are calibrated for (0.10.6).
    pub fn is_current(self) -> bool {
        self >= Version(0, 10, 6)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// [`RUNTIME_MARKER`]'s contents: the runtime on disk, and what this PC has
/// learnt about runtimes and MTP — carried from marker to marker through
/// every swap and roll-back (never config.json: Settings' wholesale saves).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Marker {
    pub version: String,
    /// The SHA-256 the download was verified against ("" when unknown: the
    /// old runtime, put back after a new one wouldn't start).
    pub sha256: String,
    /// Swapped in, not yet started once ([`swap_in`]): a start that fails
    /// on it goes back to the previous runtime.
    #[serde(default)]
    pub unproven: bool,
    /// Runtimes (SHA-256) that downloaded fine but wouldn't run on this PC
    /// [`REFUSE_AFTER`] times: never downloaded again.
    #[serde(default)]
    pub refused: Vec<String>,
    /// Sessions in which a downloaded runtime (by SHA-256) wouldn't start,
    /// or wouldn't say its version: refused at [`REFUSE_AFTER`].
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub failures: std::collections::BTreeMap<String, u32>,
    /// "<runtime>/<GGUF file>" pairs MTP failed with here — the model
    /// wouldn't start with it, or landed (partly) on the processor
    /// ([`mtp_key`]): never with MTP again on that runtime.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mtp_failed: Vec<String>,
}

impl Marker {
    /// `self` with what this PC learnt, as `from` has it.
    pub(super) fn learnt_from(mut self, from: &Marker) -> Marker {
        self.refused = from.refused.clone();
        self.failures = from.failures.clone();
        self.mtp_failed = from.mtp_failed.clone();
        self
    }
}

/// A downloaded runtime that fails in this many sessions (its first start,
/// or `--version`) is refused on this PC; until then the previous one
/// serves, and the next launch tries the new one again.
pub const REFUSE_AFTER: u32 = 3;

/// Write the marker for the runtime in `dir` (after a verified download).
/// (The model evaluations' own runtime, `eval/local.rs`: debug builds.)
#[cfg(debug_assertions)]
pub fn write_marker(dir: &Path, m: &Marker) -> std::io::Result<()> {
    write_marker_at(&dir.join(RUNTIME_MARKER), m)
}

pub(super) fn read_marker_at(path: &Path) -> Option<Marker> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

pub(super) fn write_marker_at(path: &Path, m: &Marker) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(m).map_err(std::io::Error::other)?;
    crate::config::atomic_write(path, &json).map_err(std::io::Error::other)
}

/// Change the marker in `dir` (none: the unmarked 0.10.3's, made).
pub(super) fn update_marker(dir: &Path, f: impl FnOnce(&mut Marker)) -> std::io::Result<Marker> {
    let path = dir.join(RUNTIME_MARKER);
    let mut m = read_marker_at(&path)
        .unwrap_or_else(|| Marker { version: UNMARKED_RUNTIME_VERSION.to_string(), ..Default::default() });
    f(&mut m);
    write_marker_at(&path, &m)?;
    Ok(m)
}

/// Note a session in which the runtime `sha256` failed (once a session:
/// the caller's to say). Returns how many sessions now; at
/// [`REFUSE_AFTER`] it's refused.
pub fn count_failure(dir: &Path, sha256: &str) -> u32 {
    if sha256.is_empty() {
        return 0;
    }
    let mut n = 0;
    let _ = update_marker(dir, |m| {
        let c = m.failures.entry(sha256.to_ascii_lowercase()).or_insert(0);
        *c += 1;
        n = *c;
        if n >= REFUSE_AFTER && !m.refused.iter().any(|r| r.eq_ignore_ascii_case(sha256)) {
            m.refused.push(sha256.to_ascii_lowercase());
        }
    });
    n
}

/// Take back a failure [`count_failure`] noted (and the refusal it made):
/// it wasn't the runtime's fault after all.
pub(super) fn uncount_failure(dir: &Path, sha256: &str) {
    let _ = update_marker(dir, |m| {
        let key = sha256.to_ascii_lowercase();
        if let Some(c) = m.failures.get_mut(&key) {
            *c = c.saturating_sub(1);
            if *c == 0 {
                m.failures.remove(&key);
            }
        }
        m.refused.retain(|r| !r.eq_ignore_ascii_case(sha256));
    });
}

/// Is the runtime `sha256` refused in `dir`'s marker?
pub(super) fn refused_in(dir: &Path, sha256: &str) -> bool {
    read_marker_at(&dir.join(RUNTIME_MARKER)).is_some_and(|m| m.refused.iter().any(|r| r.eq_ignore_ascii_case(sha256)))
}

/// The runtime in `dir`: its version (the marker's; none = 0.10.3) — `None`
/// when there's no runtime there.
pub fn runtime_version_in(dir: &Path) -> Option<Version> {
    if !dir.join(RUNTIME_FILENAME).is_file() {
        return None;
    }
    let v = read_marker_at(&dir.join(RUNTIME_MARKER)).and_then(|m| Version::parse(&m.version));
    Some(v.unwrap_or_else(|| Version::parse(UNMARKED_RUNTIME_VERSION).expect("a version")))
}

/// The installed runtime's version (`None`: not installed).
pub fn runtime_version() -> Option<Version> {
    runtime_version_in(&llm_dir())
}

/// Should the runtime on disk be replaced by the one pinned (`want_sha`)?
/// Only when there is one (`exe`), its marker doesn't say it's that one, and
/// that one wasn't refused here before. Pure.
pub fn upgrade_due(exe: bool, marker: Option<&Marker>, want_sha: &str) -> bool {
    exe && !marker.is_some_and(|m| m.sha256.eq_ignore_ascii_case(want_sha) || m.refused.iter().any(|r| r.eq_ignore_ascii_case(want_sha)))
}

/// Put the downloaded runtime (`llamafile-next.exe`) in place in `dir`: the
/// current one (and its marker) kept as the previous, the new one's marker
/// written, `unproven` until it has started once. Nothing runs from `dir`
/// meanwhile (the caller stopped the sidecar). A failure puts the old one
/// back.
pub fn swap_in(dir: &Path, version: &str, sha256: &str) -> std::io::Result<()> {
    let (cur, next, prev) = (dir.join(RUNTIME_FILENAME), dir.join(RUNTIME_NEXT), dir.join(RUNTIME_PREVIOUS));
    let (marker, prev_marker) = (dir.join(RUNTIME_MARKER), dir.join(MARKER_PREVIOUS));
    if !next.is_file() {
        return Err(std::io::Error::new(std::io::ErrorKind::NotFound, "no new runtime to swap in"));
    }
    let learnt = read_marker_at(&marker).unwrap_or_default();
    let _ = std::fs::remove_file(&prev);
    let _ = std::fs::remove_file(&prev_marker);
    let had_old = cur.is_file();
    if had_old {
        std::fs::rename(&cur, &prev)?;
    }
    if marker.is_file() {
        let _ = std::fs::rename(&marker, &prev_marker);
    }
    if let Err(e) = std::fs::rename(&next, &cur) {
        if had_old {
            let _ = std::fs::rename(&prev, &cur);
        }
        let _ = std::fs::rename(&prev_marker, &marker);
        return Err(e);
    }
    let mut m = Marker { version: version.to_string(), sha256: sha256.to_string(), unproven: true, ..Default::default() }
        .learnt_from(&learnt);
    m.refused.retain(|r| !r.eq_ignore_ascii_case(sha256));
    write_marker_at(&marker, &m)
}

/// The swapped-in runtime has started: the previous one and its marker go.
pub fn prove(dir: &Path) {
    let path = dir.join(RUNTIME_MARKER);
    if let Some(mut m) = read_marker_at(&path).filter(|m| m.unproven) {
        m.unproven = false;
        let _ = write_marker_at(&path, &m);
    }
    let _ = std::fs::remove_file(dir.join(RUNTIME_PREVIOUS));
    let _ = std::fs::remove_file(dir.join(MARKER_PREVIOUS));
}

/// The swapped-in runtime didn't start: the previous one goes back in place
/// (with its marker, or none — the unmarked 0.10.3) and the new one is kept
/// as `llamafile-next.exe`, for [`swap_in`] again (the old one won't start
/// either: the fault was elsewhere) or the next launch's try. What this PC
/// learnt goes with it — the caller counted this failure first
/// ([`count_failure`]). A new one that's gone (an antivirus took it) leaves
/// whatever waits as `llamafile-next.exe` alone. Returns the new one's
/// marker; `None` when there was no previous one to go back to.
pub fn roll_back(dir: &Path) -> std::io::Result<Option<Marker>> {
    let (cur, next, prev) = (dir.join(RUNTIME_FILENAME), dir.join(RUNTIME_NEXT), dir.join(RUNTIME_PREVIOUS));
    let (marker, prev_marker) = (dir.join(RUNTIME_MARKER), dir.join(MARKER_PREVIOUS));
    if !prev.is_file() {
        return Ok(None);
    }
    let failed = read_marker_at(&marker).unwrap_or_default();
    if cur.is_file() {
        let _ = std::fs::remove_file(&next);
        std::fs::rename(&cur, &next)?;
        if let Err(e) = std::fs::rename(&prev, &cur) {
            let _ = std::fs::rename(&next, &cur);
            return Err(e);
        }
    } else {
        std::fs::rename(&prev, &cur)?;
    }
    let mut old = read_marker_at(&prev_marker)
        .unwrap_or(Marker { version: UNMARKED_RUNTIME_VERSION.to_string(), ..Default::default() })
        .learnt_from(&failed);
    old.unproven = false;
    let _ = std::fs::remove_file(&prev_marker);
    write_marker_at(&marker, &old)?;
    Ok(Some(failed))
}

/// Put `dir` right at launch, before anything starts: a missing
/// `llamafile.exe` comes back from the previous one (a roll-back or swap
/// cut off) or from a whole `llamafile-next.exe` of the pinned runtime
/// (`next_sha` is its hash, `None` = not worth hashing: there's none, or
/// the caller didn't); a previous one kept past a proven start goes. Returns
/// what it did, for the log. Pure but for the files.
pub fn repair_runtime(dir: &Path, next_sha: Option<&str>, pinned: (&str, &str)) -> Option<&'static str> {
    let (cur, next, prev) = (dir.join(RUNTIME_FILENAME), dir.join(RUNTIME_NEXT), dir.join(RUNTIME_PREVIOUS));
    let (marker, prev_marker) = (dir.join(RUNTIME_MARKER), dir.join(MARKER_PREVIOUS));
    if !cur.is_file() {
        let learnt = read_marker_at(&marker).unwrap_or_default();
        if prev.is_file() && std::fs::rename(&prev, &cur).is_ok() {
            let old = read_marker_at(&prev_marker)
                .unwrap_or(Marker { version: UNMARKED_RUNTIME_VERSION.to_string(), ..Default::default() })
                .learnt_from(&learnt);
            let _ = write_marker_at(&marker, &Marker { unproven: false, ..old });
            let _ = std::fs::remove_file(&prev_marker);
            return Some("restored the previous runtime");
        }
        let (version, sha) = pinned;
        if next.is_file() && next_sha.is_some_and(|h| h.eq_ignore_ascii_case(sha)) {
            // The marker first: never a runtime on disk its marker doesn't
            // describe.
            let m = Marker { version: version.to_string(), sha256: sha.to_string(), unproven: false, ..Default::default() }
                .learnt_from(&learnt);
            if write_marker_at(&marker, &m).is_ok() && std::fs::rename(&next, &cur).is_ok() {
                return Some("restored the downloaded runtime");
            }
        }
        return None;
    }
    let proven = !read_marker_at(&marker).is_some_and(|m| m.unproven);
    if proven && prev.is_file() {
        let _ = std::fs::remove_file(&prev);
        let _ = std::fs::remove_file(&prev_marker);
        return Some("deleted the previous runtime");
    }
    None
}

// ---- the runtime: a newer llamafile, in the background ----------------------

/// When the background upgrade first looks (after launch: the sidecar
/// starts and the first dictations come first), and how long it waits
/// after a "not now" (metered, not in use, a failed download), doubling up
/// to the longest.
const UPGRADE_FIRST_LOOK: Duration = Duration::from_secs(90);
const UPGRADE_RETRY: Duration = Duration::from_secs(30 * 60);
const UPGRADE_RETRY_MAX: Duration = Duration::from_secs(6 * 60 * 60);

/// What one look at the runtime came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpgradeStep {
    /// The runtime on disk is the one pinned (or none is installed).
    Done,
    /// Not now, and why ("a metered connection", a download error…).
    Later(String),
}

/// The upgrade under way, for Settings and the log: "downloading" |
/// "waiting" (for the sidecar to be idle) | "" (none).
static UPGRADE_PHASE: Mutex<&'static str> = Mutex::new("");

fn set_upgrade_phase(p: &'static str) {
    if let Ok(mut g) = UPGRADE_PHASE.lock() {
        *g = p;
    }
}

/// The background upgrade's phase now ("" when there's none).
pub fn upgrade_phase() -> &'static str {
    UPGRADE_PHASE.lock().map(|g| *g).unwrap_or("")
}

/// **Upgrade an older runtime in the background** (installs from before 22
/// Oct 2026 have 0.10.3, which `ensure_runtime` never replaced: it only
/// downloads a missing one). Looks 90 s after launch, then — after a "not
/// now" — every 30 min, doubling to 6 h:
/// - only while the on-device model is in use (no 368 MB for nothing), and
///   not on a metered connection (`updates::metered`);
/// - downloads the pinned runtime to `llamafile-next.exe` (SHA-256 verified;
///   a failed download is retried later, the reason logged) while the old
///   one keeps serving, and checks it runs here (`--version`);
/// - swaps it in once the sidecar is idle — no dictation, no request to it
///   under way, no meeting recording or being summarised — stopping it
///   first and starting it again on the new runtime ([`start_locked`]: a
///   new runtime that won't start gives way to the previous one, which is
///   kept until the new one has started once).
///
/// The new runtime runs as `llamafile.exe` like the old one, so the orphan
/// sweep (`kill_orphans`) and the job object (`procs::end_with_yap`) cover
/// it. A test run only upgrades from its own stand-in (`YAP_LLM_SOURCES`),
/// never runs it, and looks after `YAP_E2E_RUNTIME_UPGRADE_MS` (debug).
pub fn spawn_runtime_upgrade(app: AppHandle) {
    let Some(first) = first_look() else { return };
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(first).await;
        let mut wait = UPGRADE_RETRY;
        loop {
            match upgrade_runtime(&app).await {
                UpgradeStep::Done => {
                    set_upgrade_phase("");
                    return;
                }
                UpgradeStep::Later(why) => {
                    set_upgrade_phase("");
                    tracing::info!(retry_min = wait.as_secs() / 60, "llamafile upgrade: not now ({why})");
                    tokio::time::sleep(wait).await;
                    wait = (wait * 2).min(UPGRADE_RETRY_MAX);
                }
            }
        }
    });
}

/// When the upgrade first looks; `None` = never (a test run that didn't
/// ask for it with `YAP_E2E_RUNTIME_UPGRADE_MS`, debug builds).
fn first_look() -> Option<Duration> {
    if crate::shell::e2e::active() {
        #[cfg(debug_assertions)]
        if let Some(ms) = std::env::var("YAP_E2E_RUNTIME_UPGRADE_MS").ok().and_then(|v| v.trim().parse().ok()) {
            return Some(Duration::from_millis(ms));
        }
        return None;
    }
    Some(UPGRADE_FIRST_LOOK)
}

/// A meeting recording or being summarised (its digests and action plan
/// go to the on-device model one call after another).
fn meeting_busy() -> bool {
    crate::meetings::meeting::is_recording() || crate::meetings::meeting_end::summary_running()
}

/// The runtime's size (the pinned download's): what a download and a swap
/// need on the disk.
const RUNTIME_BYTES: u64 = 368_094_430;

/// Free space an upgrade wants before it downloads: the new runtime beside
/// the old one (and the old one kept until the new one has started), plus
/// room to spare.
const UPGRADE_ROOM: u64 = 2 * RUNTIME_BYTES + 256 * 1_048_576;

/// **One upgrade at a time across Yaps**: a dev build and the installed app
/// share `llm/`. A lock file holding the owner's process id, taken with
/// create-new; one whose owner isn't a running Yap any more is stale and
/// taken over. Released (deleted) when dropped.
pub struct UpgradeLock(PathBuf);

/// The upgrade lock's file in `llm/`.
const UPGRADE_LOCK: &str = "llamafile-upgrade.lock";

impl UpgradeLock {
    /// `None`: another running Yap holds it (or the folder can't be written).
    pub fn take(dir: &Path) -> Option<UpgradeLock> {
        Self::take_unless(dir, |pid| pid != std::process::id() && running_yap(pid))
    }

    /// [`take`](Self::take), `held(pid)` saying whether the pid in an
    /// existing lock still holds it (the unit tests' stand-in).
    fn take_unless(dir: &Path, held: impl Fn(u32) -> bool) -> Option<UpgradeLock> {
        use std::io::Write;
        let path = dir.join(UPGRADE_LOCK);
        for _ in 0..2 {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(mut f) => {
                    let _ = write!(f, "{}", std::process::id());
                    return Some(UpgradeLock(path));
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let owner = std::fs::read_to_string(&path).ok().and_then(|s| s.trim().parse::<u32>().ok());
                    if owner.is_some_and(&held) {
                        return None;
                    }
                    // Stale (its Yap is gone), or ours from before.
                    let _ = std::fs::remove_file(&path);
                }
                Err(_) => return None,
            }
        }
        None
    }
}

impl Drop for UpgradeLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(windows)]
fn running_yap(pid: u32) -> bool {
    crate::shell::procs::is_running_yap(pid)
}
#[cfg(not(windows))]
fn running_yap(_pid: u32) -> bool {
    false
}

/// Another running Yap's sidecar (a dev build beside the installed app),
/// running the runtime in this `llm/`: a swap waits for it to stop.
#[cfg(windows)]
fn other_yaps_sidecar() -> bool {
    crate::shell::procs::sidecars_of_other_yaps(&llm_dir(), RUNTIME_FILENAME) > 0
}
#[cfg(not(windows))]
fn other_yaps_sidecar() -> bool {
    false
}

/// Free bytes on the disk `dir` is on (`None`: unknown).
#[cfg(windows)]
pub fn free_bytes(dir: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut free = 0u64;
    // SAFETY: a NUL-terminated path, an out pointer that lives through the call.
    unsafe { GetDiskFreeSpaceExW(windows::core::PCWSTR(wide.as_ptr()), Some(&mut free), None, None) }.ok()?;
    Some(free)
}
#[cfg(not(windows))]
pub fn free_bytes(_dir: &Path) -> Option<u64> {
    None
}

/// Enough room on the disk for `need` bytes? Unknown: yes. Pure.
pub fn room_for(free: Option<u64>, need: u64) -> bool {
    free.is_none_or(|f| f >= need)
}

/// The `llamafile-next.exe` this session already hashed and found whole:
/// its size and time, so a later look doesn't read 368 MB again.
static NEXT_CHECKED: Mutex<Option<(u64, std::time::SystemTime)>> = Mutex::new(None);

fn file_stamp(path: &Path) -> Option<(u64, std::time::SystemTime)> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.len(), m.modified().ok()?))
}

/// Is `next` the whole pinned runtime (`sha`)? Hashed once a session.
pub(super) async fn next_is_whole(next: &Path, sha: &str) -> bool {
    let Some(stamp) = file_stamp(next) else { return false };
    if NEXT_CHECKED.lock().is_ok_and(|g| *g == Some(stamp)) {
        return true;
    }
    let p = next.to_path_buf();
    let ok = tokio::task::spawn_blocking(move || compute_sha256(&p)).await.ok().and_then(Result::ok).is_some_and(|h| h.eq_ignore_ascii_case(sha));
    if ok {
        if let Ok(mut g) = NEXT_CHECKED.lock() {
            *g = Some(stamp);
        }
    }
    ok
}

/// **At launch, before the sidecar starts**: a runtime cut off mid-swap or
/// taken by an antivirus comes back ([`repair_runtime`]), a previous one
/// kept past a proven start goes, and a download that was cut off leaves
/// no partial file. Not while another Yap holds the upgrade lock.
pub async fn tidy_runtime() {
    let dir = llm_dir();
    if !dir.is_dir() {
        return;
    }
    let Some(_lock) = UpgradeLock::take(&dir) else { return };
    let _ = std::fs::remove_file(dir.join(RUNTIME_NEXT).with_extension("partial"));
    let (_, sha) = source("runtime", RUNTIME_URL, RUNTIME_SHA256);
    let next = dir.join(RUNTIME_NEXT);
    let next_sha = if !dir.join(RUNTIME_FILENAME).is_file() && next.is_file() {
        tokio::task::spawn_blocking(move || compute_sha256(&next)).await.ok().and_then(Result::ok)
    } else {
        None
    };
    if let Some(did) = repair_runtime(&dir, next_sha.as_deref(), (RUNTIME_VERSION, &sha)) {
        tracing::info!("llamafile runtime: {did}");
    }
}

/// Wait until `idle()` (then a settle, and idle again), then take the
/// restart lock — never waiting for it: a context restart under way isn't
/// held up — and the start lock, looking again once it's had to wait for
/// that one: a request that began meanwhile (a Chat turn, a dictation) sends
/// it back to waiting. Returns both locks, held.
async fn idle_with_locks(
    restarting: &'static tokio::sync::Mutex<()>,
    starting: &'static tokio::sync::Mutex<()>,
    idle: impl Fn() -> bool,
    tick: Duration,
    settle: Duration,
) -> (tokio::sync::MutexGuard<'static, ()>, tokio::sync::MutexGuard<'static, ()>) {
    loop {
        wait_until_idle(|| !idle(), || true, tick, settle).await;
        tokio::time::sleep(settle).await;
        if let Ok(restart) = restarting.try_lock() {
            if idle() {
                let one = starting.lock().await;
                if idle() {
                    return (restart, one);
                }
            }
        }
        tokio::time::sleep(tick).await;
    }
}

/// One look: download the pinned runtime if it's due, then swap it in.
pub async fn upgrade_runtime(app: &AppHandle) -> UpgradeStep {
    let dir = llm_dir();
    let (url, sha) = source("runtime", RUNTIME_URL, RUNTIME_SHA256);
    // This session went back from it: the next launch tries it again.
    if WENT_BACK.load(Ordering::SeqCst) {
        return UpgradeStep::Done;
    }
    let marker = read_marker_at(&dir.join(RUNTIME_MARKER));
    if !upgrade_due(dir.join(RUNTIME_FILENAME).is_file(), marker.as_ref(), &sha) {
        return UpgradeStep::Done;
    }
    let test_run = crate::shell::e2e::active();
    if test_run && !allowed_in_test_run(&url) {
        return UpgradeStep::Done; // a test run without a stand-in runtime
    }
    let cfg = crate::config::load();
    if !in_use(&cfg) && !is_running() {
        return UpgradeStep::Later("the on-device model isn't in use".into());
    }
    if !test_run && crate::updates::metered() {
        return UpgradeStep::Later("a metered connection".into());
    }
    let Some(_lock) = UpgradeLock::take(&dir) else {
        return UpgradeStep::Later("another Yap is upgrading it".into());
    };
    // Under the lock: the other Yap may have done it meanwhile.
    let marker = read_marker_at(&dir.join(RUNTIME_MARKER));
    if !upgrade_due(dir.join(RUNTIME_FILENAME).is_file(), marker.as_ref(), &sha) {
        return UpgradeStep::Done;
    }
    let next = dir.join(RUNTIME_NEXT);
    // Downloaded before (Yap closed before the swap, or a way back kept it):
    // kept if it's whole.
    if !(next.is_file() && next_is_whole(&next, &sha).await) {
        let free = free_bytes(&dir);
        if !room_for(free, UPGRADE_ROOM) {
            return UpgradeStep::Later(format!(
                "only {} MB free on the disk; it needs {} MB",
                free.unwrap_or(0) / 1_048_576,
                UPGRADE_ROOM / 1_048_576
            ));
        }
        set_upgrade_phase("downloading");
        tracing::info!(from = %marker.as_ref().map(|m| m.version.clone()).unwrap_or_else(|| UNMARKED_RUNTIME_VERSION.into()), to = RUNTIME_VERSION, "llamafile upgrade: downloading the new runtime in the background");
        if let Err(e) = download_for(&url, &next, &sha, "runtime", "", None).await {
            return UpgradeStep::Later(format!("the download failed: {e}"));
        }
        if let (Some(stamp), Ok(mut g)) = (file_stamp(&next), NEXT_CHECKED.lock()) {
            *g = Some(stamp);
        }
    }
    // It runs on this PC (an antivirus can hold a new exe back). One that
    // won't, session after session, is refused and deleted.
    if !test_run {
        let why = match probe_version(&next).await {
            Some(v) if v.to_string() == RUNTIME_VERSION => None,
            Some(v) => Some(format!("the download says it's {v}, not {RUNTIME_VERSION}")),
            None => Some("the new runtime didn't run (--version)".to_string()),
        };
        if let Some(why) = why {
            if !PROBE_COUNTED.swap(true, Ordering::SeqCst) && count_failure(&dir, &sha) >= REFUSE_AFTER {
                let _ = std::fs::remove_file(&next);
                tracing::warn!("llamafile upgrade: {why}, again; this runtime won't be downloaded again");
                return UpgradeStep::Done;
            }
            return UpgradeStep::Later(why);
        }
    }
    // Swapped in once nothing uses the sidecar (idle, then idle again after
    // a settle), never under a context restart — which isn't held up
    // meanwhile: the locks are only taken once it's idle — and never under
    // a request that began while the start lock was awaited.
    set_upgrade_phase("waiting");
    let idle = || !(sidecar_busy(app) || meeting_busy());
    let (_restart, _one) = idle_with_locks(&RESTARTING, &STARTING, idle, RESTART_TICK, RESTART_SETTLE).await;
    // Another Yap's sidecar runs the runtime here: not under it.
    if !test_run && other_yaps_sidecar() {
        return UpgradeStep::Later("another Yap is using the on-device model".into());
    }
    let result = {
        let _restarting = PlannedRestart::begin();
        let was_running = is_running();
        if was_running {
            stop();
        }
        match swap_in(&dir, RUNTIME_VERSION, &sha) {
            Ok(()) => {
                tracing::info!(version = RUNTIME_VERSION, restart = was_running, "llamafile upgrade: the new runtime is in place");
                if was_running {
                    start_locked().await.map(|_| ())
                } else {
                    Ok(())
                }
            }
            Err(e) => {
                tracing::warn!("llamafile upgrade: couldn't swap the runtime in: {e}");
                if was_running {
                    let _ = start_locked().await;
                }
                Err(e.to_string())
            }
        }
    };
    let _ = app.emit("yap-local-llm-changed", serde_json::json!({ "pending": restart_pending(), "error": result.as_ref().err() }));
    let _ = app.emit("yap-chat-model-changed", serde_json::json!({ "scope": null }));
    match result {
        Ok(()) => UpgradeStep::Done,
        Err(e) => UpgradeStep::Later(e),
    }
}

/// What `exe --version` says it is ("llamafile v0.10.6"), within a minute
/// (a fresh download's first run waits for the antivirus); `None` when it
/// didn't run or said nothing readable. Tied to Yap like the sidecar.
pub async fn probe_version(exe: &Path) -> Option<Version> {
    let exe = exe.to_path_buf();
    let run = tokio::task::spawn_blocking(move || {
        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("--version")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let child = cmd.spawn().ok()?;
        crate::shell::procs::end_with_yap(&child);
        let out = child.wait_with_output().ok()?;
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        Version::parse(&text)
    });
    tokio::time::timeout(Duration::from_secs(60), run).await.ok()?.ok()?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::local_llm::hold;
    use crate::ai::local_llm::tests::{read, scratch, NEW, OLD};
    use std::sync::atomic::AtomicBool;

    #[test]
    fn an_old_runtime_is_upgraded_once_and_a_refused_one_never() {
        let current = Marker { version: "0.10.6".into(), sha256: RUNTIME_SHA256.into(), ..Default::default() };
        // No runtime: nothing to upgrade (an install downloads the new one).
        assert!(!upgrade_due(false, None, RUNTIME_SHA256));
        // No marker: the 0.10.3 every install had before markers.
        assert!(upgrade_due(true, None, RUNTIME_SHA256));
        assert!(!upgrade_due(true, Some(&current), RUNTIME_SHA256));
        assert!(!upgrade_due(true, Some(&current), &RUNTIME_SHA256.to_uppercase()));
        // A newer pin later: due again.
        assert!(upgrade_due(true, Some(&current), "ab12"));
        // One that wouldn't start here: never again.
        let refused = Marker { version: "0.10.3".into(), refused: vec![RUNTIME_SHA256.into()], ..Default::default() };
        assert!(!upgrade_due(true, Some(&refused), RUNTIME_SHA256));
        // The version on disk: the marker's, else 0.10.3; none without a runtime.
        let dir = scratch("version");
        assert_eq!(runtime_version_in(&dir), None);
        std::fs::write(dir.join(RUNTIME_FILENAME), "old").unwrap();
        assert_eq!(runtime_version_in(&dir), Some(OLD));
        write_marker(&dir, &current).unwrap();
        assert_eq!(runtime_version_in(&dir), Some(NEW));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_new_runtime_swaps_in_and_the_old_one_stays_until_it_has_started() {
        let dir = scratch("swap");
        std::fs::write(dir.join(RUNTIME_FILENAME), "old").unwrap();
        // Nothing downloaded: nothing changes.
        assert!(swap_in(&dir, "0.10.6", "aa").is_err());
        assert_eq!(read(&dir, RUNTIME_FILENAME), "old");
        std::fs::write(dir.join(RUNTIME_NEXT), "new").unwrap();
        swap_in(&dir, "0.10.6", "aa").unwrap();
        assert_eq!((read(&dir, RUNTIME_FILENAME), read(&dir, RUNTIME_PREVIOUS)), ("new".into(), "old".into()));
        assert!(!dir.join(RUNTIME_NEXT).exists());
        let m = read_marker_at(&dir.join(RUNTIME_MARKER)).unwrap();
        assert_eq!((m.version.as_str(), m.sha256.as_str(), m.unproven), ("0.10.6", "aa", true));
        // It started: the old one goes.
        prove(&dir);
        assert!(!dir.join(RUNTIME_PREVIOUS).exists());
        assert!(!read_marker_at(&dir.join(RUNTIME_MARKER)).unwrap().unproven);
        // Nothing to go back to now.
        assert_eq!(roll_back(&dir).unwrap(), None);
        assert_eq!(read(&dir, RUNTIME_FILENAME), "new");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_runtime_that_wont_start_gives_way_to_the_old_one() {
        let dir = scratch("rollback");
        std::fs::write(dir.join(RUNTIME_FILENAME), "old").unwrap();
        std::fs::write(dir.join(RUNTIME_NEXT), "new").unwrap();
        swap_in(&dir, "0.10.6", "aa").unwrap();
        // It didn't start (counted once): the old one (unmarked: 0.10.3) is
        // back, the new one kept aside for the next launch — not refused yet.
        assert_eq!(count_failure(&dir, "aa"), 1);
        let failed = roll_back(&dir).unwrap().unwrap();
        assert_eq!((failed.version.as_str(), failed.sha256.as_str()), ("0.10.6", "aa"));
        assert_eq!((read(&dir, RUNTIME_FILENAME), read(&dir, RUNTIME_NEXT)), ("old".into(), "new".into()));
        assert!(!dir.join(RUNTIME_PREVIOUS).exists());
        let m = read_marker_at(&dir.join(RUNTIME_MARKER)).unwrap();
        assert_eq!((m.version.as_str(), m.unproven), (UNMARKED_RUNTIME_VERSION, false));
        assert_eq!((m.failures.get("aa"), m.refused.is_empty()), (Some(&1), true), "what was learnt goes with it");
        assert_eq!(runtime_version_in(&dir), Some(OLD));
        assert!(upgrade_due(true, Some(&m), "aa"), "tried again at the next launch");
        // The old one didn't start either: not the runtime's fault — the new
        // one goes back in, the failure not counted.
        swap_in(&dir, &failed.version, &failed.sha256).unwrap();
        uncount_failure(&dir, "aa");
        assert_eq!((read(&dir, RUNTIME_FILENAME), read(&dir, RUNTIME_PREVIOUS)), ("new".into(), "old".into()));
        let m = read_marker_at(&dir.join(RUNTIME_MARKER)).unwrap();
        assert!(m.unproven && m.refused.is_empty() && m.failures.is_empty());
        // Three sessions it failed in: refused, never downloaded again.
        for n in 1..=REFUSE_AFTER {
            assert_eq!(count_failure(&dir, "aa"), n);
        }
        let m = read_marker_at(&dir.join(RUNTIME_MARKER)).unwrap();
        assert_eq!(m.refused, vec!["aa".to_string()]);
        roll_back(&dir).unwrap().unwrap();
        let m = read_marker_at(&dir.join(RUNTIME_MARKER)).unwrap();
        assert!(!upgrade_due(true, Some(&m), "aa"), "never downloaded again");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The review's finding (2026-10-22): a roll-back with the new runtime
    /// gone (an antivirus took it) deleted the waiting `llamafile-next.exe`
    /// and then failed to rename, leaving no runtime at all.
    #[test]
    fn a_roll_back_with_the_new_runtime_gone_still_puts_the_old_one_back() {
        let dir = scratch("rollback-gone");
        std::fs::write(dir.join(RUNTIME_FILENAME), "old").unwrap();
        std::fs::write(dir.join(RUNTIME_NEXT), "new").unwrap();
        swap_in(&dir, "0.10.6", "aa").unwrap();
        std::fs::remove_file(dir.join(RUNTIME_FILENAME)).unwrap();
        std::fs::write(dir.join(RUNTIME_NEXT), "waiting").unwrap();
        roll_back(&dir).unwrap().unwrap();
        assert_eq!((read(&dir, RUNTIME_FILENAME), read(&dir, RUNTIME_NEXT)), ("old".into(), "waiting".into()));
        assert!(!dir.join(RUNTIME_PREVIOUS).exists());
        assert_eq!(runtime_version_in(&dir), Some(OLD));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_runtime_comes_back_at_launch() {
        let pinned = ("0.10.6", "aa");
        // Cut off mid-swap or mid-roll-back (or an antivirus took the new
        // one): the previous one comes back.
        let dir = scratch("repair-prev");
        std::fs::write(dir.join(RUNTIME_FILENAME), "old").unwrap();
        std::fs::write(dir.join(RUNTIME_NEXT), "new").unwrap();
        swap_in(&dir, "0.10.6", "aa").unwrap();
        std::fs::remove_file(dir.join(RUNTIME_FILENAME)).unwrap();
        assert_eq!(repair_runtime(&dir, None, pinned), Some("restored the previous runtime"));
        assert_eq!(read(&dir, RUNTIME_FILENAME), "old");
        assert_eq!(runtime_version_in(&dir), Some(OLD));
        assert!(!read_marker_at(&dir.join(RUNTIME_MARKER)).unwrap().unproven);
        assert_eq!(repair_runtime(&dir, None, pinned), None, "nothing more to do");
        let _ = std::fs::remove_dir_all(&dir);
        // No previous one, a whole download waiting: it goes in, its marker first.
        let dir = scratch("repair-next");
        std::fs::write(dir.join(RUNTIME_NEXT), "new").unwrap();
        assert_eq!(repair_runtime(&dir, Some("bb"), pinned), None, "not the pinned runtime");
        assert!(!dir.join(RUNTIME_FILENAME).exists());
        assert_eq!(repair_runtime(&dir, Some("AA"), pinned), Some("restored the downloaded runtime"));
        assert_eq!(read(&dir, RUNTIME_FILENAME), "new");
        assert_eq!(runtime_version_in(&dir), Some(NEW));
        let _ = std::fs::remove_dir_all(&dir);
        // A previous one kept past a proven start goes; not before.
        let dir = scratch("repair-tidy");
        std::fs::write(dir.join(RUNTIME_FILENAME), "old").unwrap();
        std::fs::write(dir.join(RUNTIME_NEXT), "new").unwrap();
        swap_in(&dir, "0.10.6", "aa").unwrap();
        assert_eq!(repair_runtime(&dir, None, pinned), None, "unproven: the way back stays");
        assert!(dir.join(RUNTIME_PREVIOUS).exists());
        update_marker(&dir, |m| m.unproven = false).unwrap();
        assert_eq!(repair_runtime(&dir, None, pinned), Some("deleted the previous runtime"));
        assert!(!dir.join(RUNTIME_PREVIOUS).exists() && !dir.join(MARKER_PREVIOUS).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_old_marker_reads_and_an_empty_history_isnt_written() {
        let old: Marker = serde_json::from_str(r#"{"version":"0.10.6","sha256":"aa","unproven":false,"refused":[]}"#).unwrap();
        assert!(old.failures.is_empty() && old.mtp_failed.is_empty());
        let json = serde_json::to_string(&old).unwrap();
        assert!(!json.contains("failures") && !json.contains("mtp_failed"), "{json}");
    }

    #[test]
    fn one_upgrade_at_a_time_across_yaps() {
        let dir = scratch("upgrade-lock");
        let held = UpgradeLock::take_unless(&dir, |_| true).expect("free");
        assert!(UpgradeLock::take_unless(&dir, |_| true).is_none(), "another running Yap holds it");
        drop(held);
        assert!(!dir.join(UPGRADE_LOCK).exists(), "released");
        // A lock left by a Yap that's gone: taken over.
        std::fs::write(dir.join(UPGRADE_LOCK), "4000000").unwrap();
        let _ours = UpgradeLock::take_unless(&dir, |_| false).expect("stale");
        assert_eq!(read(&dir, UPGRADE_LOCK), std::process::id().to_string());
        // The disk: room for the new runtime beside the old one, plus some.
        assert!(room_for(None, UPGRADE_ROOM), "unknown: go");
        assert!(room_for(Some(UPGRADE_ROOM), UPGRADE_ROOM));
        assert!(!room_for(Some(RUNTIME_BYTES), UPGRADE_ROOM));
        assert!(free_bytes(&dir).is_some_and(|f| f > 0) || cfg!(not(windows)));
        // Not the sidecar's address: nothing held.
        assert!(hold("http://127.0.0.1:9/v1").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The review's fourth finding: the swap waited for idle, then for the
    /// start lock — and a request that began meanwhile was cut off. Now it
    /// looks again once it has the lock, and goes back to waiting.
    #[tokio::test]
    async fn the_swap_backs_off_when_a_request_began_while_it_waited_for_the_lock() {
        use std::sync::Arc;
        static R: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        static S: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
        let busy = Arc::new(AtomicBool::new(false));
        let done = Arc::new(AtomicBool::new(false));
        let start_under_way = S.lock().await;
        let task = {
            let (busy, done) = (busy.clone(), done.clone());
            tokio::spawn(async move {
                let _locks = idle_with_locks(&R, &S, || !busy.load(Ordering::SeqCst), Duration::from_millis(1), Duration::from_millis(1)).await;
                done.store(true, Ordering::SeqCst);
            })
        };
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!done.load(Ordering::SeqCst), "waiting for the start lock");
        // A Chat turn begins while it waits; then the start lock comes free.
        busy.store(true, Ordering::SeqCst);
        drop(start_under_way);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!done.load(Ordering::SeqCst), "it backed off");
        busy.store(false, Ordering::SeqCst);
        task.await.unwrap();
        assert!(done.load(Ordering::SeqCst));
    }

    #[test]
    fn versions_read_and_order() {
        assert_eq!(Version::parse("llamafile v0.10.6"), Some(NEW));
        assert_eq!(Version::parse("0.10.3"), Some(OLD));
        assert_eq!(Version::parse("v0.11"), Some(Version(0, 11, 0)));
        assert_eq!(Version::parse("llamafile"), None);
        assert_eq!(Version::parse(RUNTIME_VERSION), Some(NEW));
        assert!(NEW.is_current() && Version(0, 11, 0).is_current());
        assert!(!OLD.is_current() && !Version(0, 10, 5).is_current());
        assert_eq!(NEW.to_string(), "0.10.6");
        // The pinned runtime: mozilla-ai's release, its digest.
        assert!(RUNTIME_URL.starts_with("https://github.com/mozilla-ai/llamafile/releases/download/0.10.6/"));
        assert_eq!(RUNTIME_SHA256.len(), 64);
    }
}

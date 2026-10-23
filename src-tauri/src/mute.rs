//! Other apps' sound while you dictate (Windows): **"Silence other apps
//! while you dictate"** (`config.quiet_other_apps`) and **"Pause music and
//! videos while you dictate"** (`config.pause_media`, `media_pause.rs`).
//!
//! The order, as Wispr Flow does it: the start chime plays first (Yap's own
//! sound is never touched, so it's always heard), and once it has played
//! ([`after_start_chime`], the chime's callback in `sound.rs`) the other
//! apps go quiet; after the dictation the stop chime plays, and once it has
//! played ([`after_stop_chime`]) everything comes back.
//!
//! - **Per app, not the whole device**: each other app's audio session on
//!   every output device (WASAPI `IAudioSessionManager2` →
//!   `ISimpleAudioVolume`) is turned down to silence. Never Yap's own
//!   session, system sounds, a call app's (every `meeting_detect::APPS`
//!   exe, call or no call), the browser a live meeting tab is in, or, while
//!   a meeting records, any browser (its tabs can't be told apart):
//!   [`quiet_verdict`], pure and unit-tested.
//! - **Only what Yap changed comes back, to exactly its level**: each
//!   session's level is kept, and a session whose level moved meanwhile
//!   (the app or you changed it) is left alone ([`restore_verdict`]). A
//!   dictation that starts before the last one's sound came back keeps it
//!   down and brings it all back at its own end; a dictation that ended
//!   before its chime finished never quiets anything.
//! - **Bluetooth**: when the dictation itself put a headset into its call
//!   profile (the default output dropped below 44.1 kHz since the start, or
//!   its Bluetooth mic — opened on demand, `mic.rs` — was just closed), the
//!   restore waits for it to come back, polling every 50 ms for up to 5 s,
//!   plus 500 ms, so the music doesn't return through the call codec
//!   ([`codec_wait`]).
//! - **A crash mid-dictation**: Windows keeps an app's session level across
//!   restarts, so a Yap that died with Spotify silenced would leave it
//!   silent. What's down is written to `<data>/quieted_apps.json` while it
//!   is, and the next start ([`recover`]) and every restore bring back any
//!   session of those apps still at Yap's level (entries kept a week for
//!   apps that aren't running).
//!
//! Configs before 2026-10-17 had `muteWhileRecording`, which muted the
//! whole output device before the start chime (the chime and any call
//! went silent too); `quiet_other_apps` reads it (`config.rs`).
//!
//! In a test run nothing is touched; the real-app robot touches only the
//! sound of processes it started (`realtest::audio_permits`,
//! `realtest::media_permits`), never the developer's own music.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::media_pause::{self, Paused};
use crate::realtest::OtherApps;

/// What Yap does to other apps while you dictate (from Settings).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Plan {
    /// "Silence other apps while you dictate".
    pub quiet: bool,
    /// "Pause music and videos while you dictate".
    pub pause: bool,
}

impl Plan {
    pub fn any(self) -> bool {
        self.quiet || self.pause
    }
}

/// The level a quieted session is left at.
const QUIET_LEVEL: f32 = 0.0;
/// Two levels this close are the same (Windows stores them as floats).
const SAME_LEVEL: f32 = 0.005;
/// Below this rate the default output is in a headset's call profile.
const CALL_PROFILE_RATE: u32 = 44_100;
/// The Bluetooth wait: poll this often, at most this many times, then this
/// much more.
const CODEC_POLL_MS: u64 = 50;
const CODEC_POLLS: u32 = 100;
const CODEC_SETTLE_MS: u64 = 500;
/// A leftover from a crash is given up on after a week.
const LEFTOVER_DAYS: u64 = 7;

// ---- the rules (pure) --------------------------------------------------------------------

/// What Yap must never touch right now.
#[derive(Debug, Clone, Default)]
pub(crate) struct Calls {
    /// Every call app's desktop exes and packaged names, lowercased.
    pub names: Vec<String>,
    /// The browsers (exes, lowercased) a meeting is live in.
    pub live_browsers: Vec<String>,
    /// A meeting records: no browser at all.
    pub meeting: bool,
}

/// One audio session, as [`quiet_verdict`] needs it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SessionFacts {
    pub pid: u32,
    /// Its process's exe file name, lowercased ("" if unknown).
    pub exe: String,
    /// The system sounds session.
    pub system: bool,
    pub level: f32,
}

/// Whether to quiet `s` (`own_pid`: Yap's), and why not.
pub(crate) fn quiet_verdict(s: &SessionFacts, calls: &Calls, own_pid: u32) -> Result<(), &'static str> {
    if s.system || s.pid == 0 {
        return Err("system sounds");
    }
    if s.pid == own_pid {
        return Err("Yap's own");
    }
    if s.exe.is_empty() {
        return Err("an unknown process");
    }
    if calls.names.contains(&s.exe) {
        return Err("a call app");
    }
    if calls.live_browsers.contains(&s.exe) {
        return Err("the browser a meeting is in");
    }
    if calls.meeting && crate::meeting_detect::browser_exes().contains(&s.exe.as_str()) {
        return Err("a browser, while a meeting records");
    }
    if s.level <= QUIET_LEVEL + SAME_LEVEL {
        return Err("already silent");
    }
    Ok(())
}

/// Whether a session Yap set to `set_to` gets its level back: only if it's
/// still there (one that moved was changed by its app or by you).
pub(crate) fn restore_verdict(set_to: f32, now: f32) -> bool {
    (now - set_to).abs() < SAME_LEVEL
}

/// A headset's call profile: the default output's rate dropped below CD
/// quality (8/16/32 kHz).
fn in_call_profile(rate: Option<u32>) -> bool {
    rate.is_some_and(|r| r < CALL_PROFILE_RATE)
}

/// Whether the restore should wait for the output to leave its call
/// profile: it went into one during the dictation (a call on, or "Keep the
/// microphone ready" on a headset: it was in one already, no wait) — or the
/// dictation's Bluetooth mic was just closed (`mic.rs` opens it on demand,
/// so the headset dropped to its call profile before the start chime and
/// leaves it now).
fn codec_wait(rate_at_start: Option<u32>, rate_now: Option<u32>, closed_bluetooth: bool) -> bool {
    if closed_bluetooth {
        return in_call_profile(rate_now);
    }
    rate_at_start.is_some() && !in_call_profile(rate_at_start) && in_call_profile(rate_now)
}

// ---- what's down ---------------------------------------------------------------------------

/// A session Yap quieted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Quieted {
    /// The session's instance id (unique, the process id inside).
    id: String,
    /// Its exe, for a crash's leftovers.
    exe: String,
    before: f32,
    set_to: f32,
    /// Unix seconds.
    at: u64,
}

#[derive(Default)]
struct State {
    /// Bumped by every [`begin`] and [`end`].
    gen: u64,
    recording: bool,
    plan: Plan,
    /// The default output's rate when Yap quieted things.
    rate_at_start: Option<u32>,
    quieted: Vec<Quieted>,
    paused: Vec<Paused>,
    /// Quieted by an earlier Yap (it crashed) or a session that was gone at
    /// the restore: brought back when their app plays again (a week).
    leftovers: Vec<Quieted>,
}

static STATE: Mutex<State> = Mutex::new(State {
    gen: 0,
    recording: false,
    plan: Plan { quiet: false, pause: false },
    rate_at_start: None,
    quieted: Vec::new(),
    paused: Vec::new(),
    leftovers: Vec::new(),
});
/// Quieting and restoring, one at a time (never with [`STATE`] held: the
/// pipeline's start and stop take it, and mustn't wait on COM).
static WORK: Mutex<()> = Mutex::new(());

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---- the pipeline's side ------------------------------------------------------------------

/// A dictation starts (`start_recording`): its token, for
/// [`after_start_chime`].
pub fn begin(plan: Plan) -> u64 {
    let mut s = state();
    s.gen += 1;
    s.recording = true;
    s.plan = plan;
    s.gen
}

/// The start chime has played (or there is none): quiet and pause the
/// other apps for dictation `token`, if it's still the one recording.
pub fn after_start_chime(token: u64) {
    let _work = WORK.lock().unwrap_or_else(|p| p.into_inner());
    let (plan, already) = {
        let s = state();
        if s.gen != token || !s.recording {
            return; // over before its chime was
        }
        (s.plan, s.quieted.iter().map(|q| q.id.clone()).collect::<Vec<_>>())
    };
    let who = crate::realtest::other_apps();
    if !plan.any() {
        return;
    }
    if who == OtherApps::Nobody {
        tracing::info!("other apps: left alone in a test run");
        return;
    }
    let calls = calls_now();
    let rate = platform::default_output_rate();
    let quieted = if plan.quiet { platform::quiet(&calls, &already) } else { Vec::new() };
    let paused = if plan.pause {
        let keep = media_pause::Keep {
            call_names: calls.names.clone(),
            browsers: calls.meeting || !calls.live_browsers.is_empty(),
        };
        let robot: Option<&dyn Fn(&str) -> bool> = match who {
            OtherApps::TestsOwn => Some(&crate::realtest::media_permits),
            _ => None,
        };
        media_pause::pause(&keep, robot)
    } else {
        Vec::new()
    };
    let mut s = state();
    if s.quieted.is_empty() {
        s.rate_at_start = rate;
    }
    s.quieted.extend(quieted);
    s.paused.extend(paused);
    write_down(&s);
}

/// The dictation ended (stopped or cancelled): its token, for
/// [`after_stop_chime`].
pub fn end() -> u64 {
    let mut s = state();
    s.gen += 1;
    s.recording = false;
    s.gen
}

/// The stop chime has played (or there is none): bring back what Yap
/// quieted and paused, unless another dictation began meanwhile (it keeps
/// them down and brings them back at its end). Waits for a Bluetooth output
/// to leave the call profile first, if the dictation put it there.
#[cfg(test)]
pub fn after_stop_chime(token: u64) {
    after_stop_chime_mic(token, false)
}

/// [`after_stop_chime`], told whether the dictation's Bluetooth mic was
/// just closed (`mic::Mic::release`): its headset is leaving the call
/// profile, which the restore waits for.
pub fn after_stop_chime_mic(token: u64, closed_bluetooth: bool) {
    let rate_at_start = {
        // Once a quieting that's under way has finished (it's in the lists).
        let _work = WORK.lock().unwrap_or_else(|p| p.into_inner());
        let s = state();
        if s.quieted.is_empty() && s.paused.is_empty() {
            return;
        }
        s.rate_at_start
    };
    if codec_wait(rate_at_start, platform::default_output_rate(), closed_bluetooth) {
        tracing::info!("other apps: waiting for the headset to leave its call profile");
        for _ in 0..CODEC_POLLS {
            std::thread::sleep(std::time::Duration::from_millis(CODEC_POLL_MS));
            if !in_call_profile(platform::default_output_rate()) {
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(CODEC_SETTLE_MS));
    }
    let _work = WORK.lock().unwrap_or_else(|p| p.into_inner());
    let (quieted, paused) = {
        let mut s = state();
        if s.gen != token {
            return; // a new dictation keeps them down
        }
        s.rate_at_start = None;
        (std::mem::take(&mut s.quieted), std::mem::take(&mut s.paused))
    };
    bring_back(quieted, paused);
}

/// Yap is quitting (`shutdown_cleanup`): everything back now, no waiting.
pub fn restore_all() {
    let _work = WORK.lock().unwrap_or_else(|p| p.into_inner());
    let (quieted, paused) = {
        let mut s = state();
        s.gen += 1;
        s.recording = false;
        (std::mem::take(&mut s.quieted), std::mem::take(&mut s.paused))
    };
    bring_back(quieted, paused);
}

/// Everything back (with [`WORK`] held): `quieted` by session, then the
/// leftovers by app; then the media.
fn bring_back(quieted: Vec<Quieted>, paused: Vec<Paused>) {
    if quieted.is_empty() && paused.is_empty() && state().leftovers.is_empty() {
        return;
    }
    let missing = if quieted.is_empty() { Vec::new() } else { platform::restore(&quieted) };
    // What didn't come back (its session gone) joins the leftovers, which
    // follow its app.
    let leftovers = {
        let mut s = state();
        s.leftovers.extend(missing);
        std::mem::take(&mut s.leftovers)
    };
    retry_leftovers(leftovers);
    media_pause::resume(paused);
}

/// What Yap must never touch right now.
fn calls_now() -> Calls {
    let (names, live_browsers) = crate::meeting_detect::call_audio();
    Calls { names, live_browsers, meeting: crate::meeting::is_recording() }
}

// ---- a crash's leftovers ------------------------------------------------------------------------

fn leftovers_path() -> std::path::PathBuf {
    crate::config::data_dir().join("quieted_apps.json")
}

fn load_leftovers() -> Vec<Quieted> {
    let path = leftovers_path();
    let Ok(s) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str(&s).unwrap_or_else(|_| {
        // Set aside, never saved over (like every store; a unit test never
        // moves the real one).
        if !cfg!(test) {
            crate::config::quarantine_corrupt(&path);
        }
        Vec::new()
    })
}

/// Write what's down now, the leftovers and the live dictation's (the file
/// goes once nothing is).
fn write_down(s: &State) {
    let path = leftovers_path();
    let all: Vec<&Quieted> = s.leftovers.iter().chain(s.quieted.iter()).collect();
    if all.is_empty() {
        let _ = std::fs::remove_file(path);
        return;
    }
    if let Ok(json) = serde_json::to_string_pretty(&all) {
        if let Err(e) = crate::config::atomic_write(&path, &json) {
            tracing::warn!("other apps: couldn't note what's quieted ({e})");
        }
    }
}

/// Bring back any session of the leftovers' apps still at Yap's level;
/// keep the rest (apps not running) for a week. With [`WORK`] held.
fn retry_leftovers(leftovers: Vec<Quieted>) {
    let now = now_secs();
    let mut left: Vec<Quieted> = leftovers
        .into_iter()
        .filter(|l| now.saturating_sub(l.at) < LEFTOVER_DAYS * 24 * 3600)
        .collect();
    if !left.is_empty() {
        left = platform::restore_by_app(left);
    }
    let mut s = state();
    s.leftovers = left;
    write_down(&s);
}

/// At start: what a crash left quiet comes back (off the main thread).
pub fn recover() {
    if crate::realtest::other_apps() == OtherApps::Nobody {
        return;
    }
    let leftovers = load_leftovers();
    if leftovers.is_empty() {
        return;
    }
    tracing::info!(count = leftovers.len(), "other apps: bringing back what a crash left quiet");
    std::thread::spawn(move || {
        let _work = WORK.lock().unwrap_or_else(|p| p.into_inner());
        retry_leftovers(leftovers);
    });
}

/// The robot's view (debug builds): the test's own sessions and media as
/// Windows has them now, and what Yap holds down.
#[cfg(debug_assertions)]
pub fn robot_view() -> serde_json::Value {
    let s = state();
    serde_json::json!({
        "sessions": platform::list(&crate::realtest::audio_permits),
        "media": media_list(),
        "quieted": s.quieted.iter().map(|q| serde_json::json!({ "id": q.id, "exe": q.exe, "before": q.before, "setTo": q.set_to })).collect::<Vec<_>>(),
        "paused": s.paused.iter().map(|p| p.app.clone()).collect::<Vec<_>>(),
        "recording": s.recording,
    })
}

#[cfg(all(debug_assertions, windows))]
fn media_list() -> Vec<serde_json::Value> {
    media_pause::list(&crate::realtest::media_permits)
}

#[cfg(all(debug_assertions, not(windows)))]
fn media_list() -> Vec<serde_json::Value> {
    Vec::new()
}

// ---- Windows ------------------------------------------------------------------------------------

#[cfg(windows)]
mod platform {
    use windows::core::Interface;
    use windows::Win32::Foundation::{RPC_E_CHANGED_MODE, S_OK};
    use windows::Win32::Media::Audio::{
        eConsole, eRender, IAudioClient, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
        ISimpleAudioVolume, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED};

    use super::{now_secs, quiet_verdict, restore_verdict, Calls, Quieted, SessionFacts, QUIET_LEVEL};

    /// COM on this thread (tolerating "already initialized" in another mode).
    fn com() -> bool {
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        hr.is_ok() || hr == RPC_E_CHANGED_MODE
    }

    /// One session, live.
    struct Live {
        id: String,
        facts: SessionFacts,
        volume: ISimpleAudioVolume,
    }

    /// Every audio session on every active output device.
    fn sessions() -> Vec<Live> {
        let mut out = Vec::new();
        if !com() {
            return out;
        }
        let exes = crate::procs::exe_names();
        unsafe {
            let Ok(enumerator) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
            else {
                return out;
            };
            let Ok(devices) = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else {
                return out;
            };
            for d in 0..devices.GetCount().unwrap_or(0) {
                let Ok(device) = devices.Item(d) else { continue };
                let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
                    continue;
                };
                let Ok(list) = manager.GetSessionEnumerator() else { continue };
                for i in 0..list.GetCount().unwrap_or(0) {
                    let Ok(control) = list.GetSession(i) else { continue };
                    let Ok(control2) = control.cast::<IAudioSessionControl2>() else { continue };
                    let Ok(volume) = control.cast::<ISimpleAudioVolume>() else { continue };
                    let pid = control2.GetProcessId().unwrap_or(0);
                    let system = control2.IsSystemSoundsSession() == S_OK;
                    let id = match control2.GetSessionInstanceIdentifier() {
                        Ok(p) => {
                            let s = p.to_string().unwrap_or_default();
                            CoTaskMemFree(Some(p.0 as *const _));
                            s
                        }
                        Err(_) => continue,
                    };
                    let level = volume.GetMasterVolume().unwrap_or(0.0);
                    let exe = exes.get(&pid).cloned().unwrap_or_default();
                    out.push(Live { id, facts: SessionFacts { pid, exe, system, level }, volume });
                }
            }
        }
        out
    }

    /// Quiet every session the rules allow (and the robot's guard), except
    /// those already down (`already`, by id).
    pub fn quiet(calls: &Calls, already: &[String]) -> Vec<Quieted> {
        let own = std::process::id();
        let mut out = Vec::new();
        for s in sessions() {
            if already.contains(&s.id) {
                continue;
            }
            if let Err(why) = quiet_verdict(&s.facts, calls, own) {
                if s.facts.level > 0.0 && !s.facts.system && s.facts.pid != own {
                    tracing::debug!(exe = s.facts.exe, why, "other apps: left as it is");
                }
                continue;
            }
            if !crate::realtest::audio_permits(s.facts.pid) {
                continue;
            }
            match unsafe { s.volume.SetMasterVolume(QUIET_LEVEL, std::ptr::null()) } {
                Ok(()) => {
                    tracing::info!(exe = s.facts.exe, before = s.facts.level, "other apps: quieted");
                    out.push(Quieted {
                        id: s.id,
                        exe: s.facts.exe,
                        before: s.facts.level,
                        set_to: QUIET_LEVEL,
                        at: now_secs(),
                    });
                }
                Err(e) => tracing::info!(exe = s.facts.exe, "other apps: couldn't quiet it ({e})"),
            }
        }
        out
    }

    /// Bring each of `list` back (by session); what couldn't be found, for
    /// the leftovers.
    pub fn restore(list: &[Quieted]) -> Vec<Quieted> {
        let live = sessions();
        let mut missing = Vec::new();
        for q in list {
            match live.iter().find(|s| s.id == q.id) {
                Some(s) => back(s, q),
                None => missing.push(q.clone()),
            }
        }
        missing
    }

    /// The leftovers by app: any session of their exes still at Yap's
    /// level comes back; the apps with none running stay on the list.
    pub fn restore_by_app(list: Vec<Quieted>) -> Vec<Quieted> {
        let live = sessions();
        let mut left = Vec::new();
        for q in list {
            let of_app: Vec<&Live> = live.iter().filter(|s| !q.exe.is_empty() && s.facts.exe == q.exe).collect();
            if of_app.is_empty() {
                left.push(q);
                continue;
            }
            for s in of_app {
                if crate::realtest::audio_permits(s.facts.pid) {
                    back(s, &q);
                }
            }
        }
        left
    }

    fn back(s: &Live, q: &Quieted) {
        if !restore_verdict(q.set_to, s.facts.level) {
            tracing::info!(exe = q.exe, now = s.facts.level, "other apps: its level changed meanwhile; left alone");
            return;
        }
        match unsafe { s.volume.SetMasterVolume(q.before, std::ptr::null()) } {
            Ok(()) => tracing::info!(exe = q.exe, level = q.before, "other apps: back to its level"),
            Err(e) => tracing::info!(exe = q.exe, "other apps: couldn't bring it back ({e})"),
        }
    }

    /// The default output's mix rate (Hz).
    pub fn default_output_rate() -> Option<u32> {
        if !com() {
            return None;
        }
        unsafe {
            let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
            let client = device.Activate::<IAudioClient>(CLSCTX_ALL, None).ok()?;
            let format = client.GetMixFormat().ok()?;
            if format.is_null() {
                return None;
            }
            let rate = (*format).nSamplesPerSec;
            CoTaskMemFree(Some(format as *const _));
            Some(rate)
        }
    }

    /// The sessions `permit` lets through (pid, exe, level): the robot's view.
    #[cfg(debug_assertions)]
    pub fn list(permit: &dyn Fn(u32) -> bool) -> Vec<serde_json::Value> {
        sessions()
            .into_iter()
            .filter(|s| !s.facts.system && s.facts.pid != 0 && permit(s.facts.pid))
            .map(|s| serde_json::json!({ "pid": s.facts.pid, "exe": s.facts.exe, "level": s.facts.level }))
            .collect()
    }
}

#[cfg(not(windows))]
mod platform {
    use super::{Calls, Quieted};
    pub fn quiet(_: &Calls, _: &[String]) -> Vec<Quieted> {
        Vec::new()
    }
    pub fn restore(_: &[Quieted]) -> Vec<Quieted> {
        Vec::new()
    }
    pub fn restore_by_app(_: Vec<Quieted>) -> Vec<Quieted> {
        Vec::new()
    }
    pub fn default_output_rate() -> Option<u32> {
        None
    }
    #[cfg(debug_assertions)]
    pub fn list(_: &dyn Fn(u32) -> bool) -> Vec<serde_json::Value> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(pid: u32, exe: &str, level: f32) -> SessionFacts {
        SessionFacts { pid, exe: exe.into(), system: false, level }
    }

    fn calls() -> Calls {
        Calls {
            names: ["ms-teams.exe", "zoom.exe", "slack.exe", "discord.exe", "msteams"].iter().map(|s| s.to_string()).collect(),
            live_browsers: Vec::new(),
            meeting: false,
        }
    }

    const YAP: u32 = 77;

    #[test]
    fn music_and_videos_go_quiet() {
        assert_eq!(quiet_verdict(&session(10, "spotify.exe", 0.8), &calls(), YAP), Ok(()));
        assert_eq!(quiet_verdict(&session(11, "msedge.exe", 1.0), &calls(), YAP), Ok(()));
        assert_eq!(quiet_verdict(&session(12, "vlc.exe", 0.3), &calls(), YAP), Ok(()));
    }

    #[test]
    fn never_yap_system_sounds_or_a_call_app() {
        assert_eq!(quiet_verdict(&session(YAP, "yap.exe", 1.0), &calls(), YAP), Err("Yap's own"));
        let system = SessionFacts { system: true, ..session(4, "audiodg.exe", 1.0) };
        assert_eq!(quiet_verdict(&system, &calls(), YAP), Err("system sounds"));
        assert_eq!(quiet_verdict(&session(0, "", 1.0), &calls(), YAP), Err("system sounds"));
        for exe in ["ms-teams.exe", "zoom.exe", "slack.exe", "discord.exe"] {
            assert_eq!(quiet_verdict(&session(20, exe, 1.0), &calls(), YAP), Err("a call app"), "{exe}");
        }
        assert_eq!(quiet_verdict(&session(21, "", 1.0), &calls(), YAP), Err("an unknown process"));
        // Silent already: nothing to bring back afterwards either.
        assert_eq!(quiet_verdict(&session(22, "spotify.exe", 0.0), &calls(), YAP), Err("already silent"));
    }

    #[test]
    fn a_browser_with_a_meeting_is_left_alone() {
        let live = Calls { live_browsers: vec!["chrome.exe".into()], ..calls() };
        assert_eq!(quiet_verdict(&session(30, "chrome.exe", 1.0), &live, YAP), Err("the browser a meeting is in"));
        // Another browser, with no meeting, still goes quiet.
        assert_eq!(quiet_verdict(&session(31, "msedge.exe", 1.0), &live, YAP), Ok(()));
        // While a meeting records, no browser at all; a music app still does.
        let recording = Calls { meeting: true, ..calls() };
        assert_eq!(
            quiet_verdict(&session(31, "msedge.exe", 1.0), &recording, YAP),
            Err("a browser, while a meeting records")
        );
        assert_eq!(quiet_verdict(&session(10, "spotify.exe", 0.8), &recording, YAP), Ok(()));
    }

    #[test]
    fn only_a_session_still_at_yaps_level_comes_back() {
        assert!(restore_verdict(0.0, 0.0));
        assert!(restore_verdict(0.0, 0.003));
        // Turned up meanwhile (by the app, or in the volume mixer): left alone.
        assert!(!restore_verdict(0.0, 0.4));
    }

    #[test]
    fn the_bluetooth_wait_is_only_for_a_profile_the_dictation_caused() {
        // A headset dropped to its call profile during the dictation.
        assert!(codec_wait(Some(48_000), Some(16_000), false));
        // Already in it at the start (a call, a mic kept ready): no wait.
        assert!(!codec_wait(Some(16_000), Some(16_000), false));
        // Back to normal, or a normal device all along.
        assert!(!codec_wait(Some(48_000), Some(48_000), false));
        assert!(!codec_wait(Some(44_100), Some(44_100), false));
        assert!(!codec_wait(None, None, false));
        // The start unknown: no waiting on a guess.
        assert!(!codec_wait(None, Some(16_000), false));
        // The dictation's own Bluetooth mic was just closed (opened on
        // demand: the headset was in its call profile before the chime):
        // wait while it's still there, not once it has left.
        assert!(codec_wait(Some(16_000), Some(16_000), true));
        assert!(codec_wait(None, Some(16_000), true));
        assert!(!codec_wait(Some(16_000), Some(48_000), true));
        assert!(in_call_profile(Some(8_000)) && in_call_profile(Some(32_000)));
    }

    #[test]
    fn a_dictation_over_before_its_chime_quiets_nothing_and_a_new_one_keeps_them_down() {
        // (The token logic alone: no session is touched in `cargo test`'s
        // plan with nothing to do.)
        let a = begin(Plan::default());
        let ended = end();
        assert_ne!(a, ended);
        after_start_chime(a); // over: nothing
        let b = begin(Plan::default());
        assert!(state().recording);
        after_stop_chime(ended); // a new dictation began: nothing restored
        assert_eq!(state().gen, b);
        end();
        assert!(!state().recording);
    }

    #[test]
    fn leftovers_read_as_written() {
        let q = Quieted { id: "{0.0.0}|x%b{1}|1%b10".into(), exe: "spotify.exe".into(), before: 0.8, set_to: 0.0, at: 5 };
        let json = serde_json::to_string(&vec![q.clone()]).unwrap();
        assert!(json.contains("\"setTo\":0.0"));
        let back: Vec<Quieted> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, vec![q]);
    }
}

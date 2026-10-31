//! **Other apps' sound, turned down by Yap and brought back exactly** — one
//! ledger for both times Yap lowers another app's audio session (Windows:
//! WASAPI `IAudioSessionManager2` → `ISimpleAudioVolume`, on every output
//! device):
//!
//! - a **dictation** silences them, once its start sound has played
//!   (`mute.rs`, "Silence other apps while you dictate");
//! - **Yap's voice** turns them down to [`VOICE_SHARE`] (30 %) of their own
//!   level while it speaks on this PC (`voice/duck.rs`, "Turn other apps
//!   down while Yap speaks").
//!
//! The rules, pure and unit-tested on a fake session list ([`Mixer`]):
//!
//! - **Never** Yap's own session, system sounds, a call app's (every
//!   `meeting_detect::APPS` exe, call or no call), the browser a live
//!   meeting tab is in, or, while a meeting records, any browser
//!   ([`quiet_verdict`]).
//! - **Each session once**, with its own level before Yap touched it and
//!   who holds it down ([`Holder`]). It sits at the lowest level its holders
//!   want (a dictation's silence beats the voice's 30 %); one letting go
//!   leaves it at the other's level, and the last letting go brings it back
//!   to **exactly its own level** — only if it's still where Yap left it: a
//!   level that moved meanwhile was the app's or the person's doing, and is
//!   left alone ([`restore_verdict`]). So a dictation started while Yap
//!   speaks takes a session over at the 30 % it's at ([`Ledger::hand_over`],
//!   no jump up), silences it after its start sound and brings it back to
//!   the level it had before Yap spoke.
//! - **A crash**: Windows keeps an app's session level across restarts, so
//!   a Yap that died with Spotify down would leave it down. What's down is
//!   written to `<data>/quieted_apps.json` while it is (the same rows an
//!   older Yap writes and reads), and the next start ([`recover`]) and every
//!   release bring back any session of those apps still at Yap's level
//!   (entries kept a week for apps that aren't running) — never one this
//!   Yap holds down now.
//! - **Quitting** ([`restore_all`], from `mute::restore_all`): everything
//!   back at once.
//!
//! The ledger's lock ([`ledger`]) is also the one-at-a-time lock for the
//! COM work: the dictation pipeline's start and stop never take it (they
//! mustn't wait on COM); the start sound's and the stop sound's callbacks
//! and the voice's own thread do.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Who holds a session down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holder {
    /// A dictation: silence ("Silence other apps while you dictate").
    Dictation,
    /// Yap's voice on this PC: [`VOICE_SHARE`] of the session's own level.
    Voice,
}

/// The share of its own level Yap's voice leaves another app at.
pub const VOICE_SHARE: f32 = 0.3;
/// The level a dictation leaves a session at.
pub(crate) const QUIET_LEVEL: f32 = 0.0;
/// Two levels this close are the same (Windows stores them as floats).
pub(crate) const SAME_LEVEL: f32 = 0.005;
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

/// Whether to turn `s` down (`own_pid`: Yap's), and why not.
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
    if calls.meeting && crate::calls::meeting_detect::browser_exes().contains(&s.exe.as_str()) {
        return Err("a browser, while a meeting records");
    }
    if s.level <= QUIET_LEVEL + SAME_LEVEL {
        return Err("already silent");
    }
    Ok(())
}

/// Whether a session Yap set to `set_to` is still Yap's to move: only if
/// it's still there (one that moved was changed by its app or by you).
pub(crate) fn restore_verdict(set_to: f32, now: f32) -> bool {
    (now - set_to).abs() < SAME_LEVEL
}

/// The level `holder` wants a session whose own level is `before` at. Pure.
pub(crate) fn wanted(holder: Holder, before: f32) -> f32 {
    match holder {
        Holder::Dictation => QUIET_LEVEL,
        Holder::Voice => (before * VOICE_SHARE).clamp(0.0, 1.0),
    }
}

// ---- the ledger (pure, over a session list) ------------------------------------------------

/// A session Yap turned down: the row `quieted_apps.json` keeps.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Quieted {
    /// The session's instance id (unique, the process id inside).
    pub id: String,
    /// Its exe, for a crash's leftovers.
    pub exe: String,
    /// Its own level, before Yap touched it.
    pub before: f32,
    /// The level Yap set it to last.
    pub set_to: f32,
    /// Unix seconds.
    pub at: u64,
}

/// A session down now, and who holds it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Lowered {
    pub q: Quieted,
    pub dictation: bool,
    pub voice: bool,
}

impl Lowered {
    pub fn holds(&self, h: Holder) -> bool {
        match h {
            Holder::Dictation => self.dictation,
            Holder::Voice => self.voice,
        }
    }
    fn set_hold(&mut self, h: Holder, on: bool) {
        match h {
            Holder::Dictation => self.dictation = on,
            Holder::Voice => self.voice = on,
        }
    }
    fn held(&self) -> bool {
        self.dictation || self.voice
    }
    /// The lowest level its holders want; its own level when none is left.
    fn target(&self) -> f32 {
        let mut t = self.q.before;
        for h in [Holder::Dictation, Holder::Voice] {
            if self.holds(h) {
                t = t.min(wanted(h, self.q.before));
            }
        }
        t
    }
}

/// One live session: its instance id and what the rules need.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Live {
    pub id: String,
    pub facts: SessionFacts,
}

/// The sessions as they are now, and a way to set one's level: Windows'
/// (a snapshot of every output device's), or a unit test's fake list.
pub(crate) trait Mixer {
    fn sessions(&self) -> Vec<Live>;
    /// Set session `id` to `level`; `false` if Windows refused.
    fn set(&self, id: &str, level: f32) -> bool;
}

/// What a turning-down may touch: the rules' inputs, and the robot's guard
/// (`realtest::audio_permits`: in a test run, only the test's own sounds).
pub(crate) struct Rules<'a> {
    pub calls: &'a Calls,
    pub own_pid: u32,
    pub permit: &'a dyn Fn(u32) -> bool,
}

/// Every session Yap holds down, and a crash's leftovers.
#[derive(Debug, Default)]
pub(crate) struct Ledger {
    pub lowered: Vec<Lowered>,
    /// Turned down by an earlier Yap (it crashed), or a session that was
    /// gone at its release: brought back when their app plays again.
    pub leftovers: Vec<Quieted>,
}

impl Ledger {
    /// Whether `h` holds any session down.
    pub fn holds(&self, h: Holder) -> bool {
        self.lowered.iter().any(|e| e.holds(h))
    }

    /// `h` turns down every session the rules allow, and holds the ones
    /// already down too (each at the lowest level its holders want). A
    /// session already down whose level moved meanwhile is the person's
    /// now: left as it is, and forgotten. Returns how many it lowered.
    pub fn lower(&mut self, h: Holder, mixer: &dyn Mixer, rules: &Rules, now: u64) -> usize {
        let mut lowered = 0;
        for s in mixer.sessions() {
            if let Some(i) = self.lowered.iter().position(|e| e.q.id == s.id) {
                if !restore_verdict(self.lowered[i].q.set_to, s.facts.level) {
                    tracing::info!(exe = s.facts.exe, now = s.facts.level, "other apps: its level changed meanwhile; left alone");
                    self.lowered.remove(i);
                    continue;
                }
                let e = &mut self.lowered[i];
                e.set_hold(h, true);
                let t = e.target();
                if t < e.q.set_to - SAME_LEVEL && mixer.set(&s.id, t) {
                    e.q.set_to = t;
                    lowered += 1;
                }
                continue;
            }
            if let Err(why) = quiet_verdict(&s.facts, rules.calls, rules.own_pid) {
                if s.facts.level > 0.0 && !s.facts.system && s.facts.pid != rules.own_pid {
                    tracing::debug!(exe = s.facts.exe, why, "other apps: left as it is");
                }
                continue;
            }
            if !(rules.permit)(s.facts.pid) {
                continue;
            }
            let t = wanted(h, s.facts.level);
            if s.facts.level - t < SAME_LEVEL {
                continue;
            }
            if mixer.set(&s.id, t) {
                tracing::info!(exe = s.facts.exe, before = s.facts.level, to = t, by = ?h, "other apps: turned down");
                self.lowered.push(Lowered {
                    q: Quieted { id: s.id, exe: s.facts.exe, before: s.facts.level, set_to: t, at: now },
                    dictation: h == Holder::Dictation,
                    voice: h == Holder::Voice,
                });
                lowered += 1;
            } else {
                tracing::info!(exe = s.facts.exe, "other apps: couldn't turn it down");
            }
        }
        lowered
    }

    /// `h` lets go: each session it held goes to the level its other holder
    /// wants, or back to its own level when none is left — only if it's
    /// still where Yap left it. A session that's gone joins the leftovers
    /// once nobody holds it.
    pub fn release(&mut self, h: Holder, mixer: &dyn Mixer) {
        if !self.holds(h) {
            return;
        }
        let live = mixer.sessions();
        let mut i = 0;
        while i < self.lowered.len() {
            if !self.lowered[i].holds(h) {
                i += 1;
                continue;
            }
            self.lowered[i].set_hold(h, false);
            let Some(s) = live.iter().find(|s| s.id == self.lowered[i].q.id) else {
                if self.lowered[i].held() {
                    i += 1;
                } else {
                    let gone = self.lowered.remove(i);
                    self.leftovers.push(gone.q);
                }
                continue;
            };
            let e = &mut self.lowered[i];
            if !restore_verdict(e.q.set_to, s.facts.level) {
                tracing::info!(exe = e.q.exe, now = s.facts.level, "other apps: its level changed meanwhile; left alone");
                self.lowered.remove(i);
                continue;
            }
            let t = e.target();
            if (t - s.facts.level).abs() >= SAME_LEVEL {
                if mixer.set(&s.id, t) {
                    tracing::info!(exe = e.q.exe, level = t, back = !e.held(), "other apps: level set");
                    e.q.set_to = t;
                } else {
                    tracing::info!(exe = e.q.exe, "other apps: couldn't set its level");
                }
            }
            if e.held() {
                i += 1;
            } else {
                self.lowered.remove(i);
            }
        }
    }

    /// What `from` holds is `to`'s now, at the level it's at (`to` moves it
    /// when it acts: a dictation silences it after its start sound).
    pub fn hand_over(&mut self, from: Holder, to: Holder) {
        for e in self.lowered.iter_mut().filter(|e| e.holds(from)) {
            e.set_hold(from, false);
            e.set_hold(to, true);
        }
    }

    /// The leftovers by app: any session of their exes still at Yap's level
    /// comes back — never one Yap holds down now; the apps with none running
    /// stay on the list (a week).
    pub fn retry_leftovers(&mut self, mixer: &dyn Mixer, permit: &dyn Fn(u32) -> bool, now: u64) {
        self.leftovers.retain(|l| now.saturating_sub(l.at) < LEFTOVER_DAYS * 24 * 3600);
        if self.leftovers.is_empty() {
            return;
        }
        let live = mixer.sessions();
        let held: Vec<&str> = self.lowered.iter().map(|e| e.q.id.as_str()).collect();
        let mut left = Vec::new();
        for q in std::mem::take(&mut self.leftovers) {
            let of_app: Vec<&Live> =
                live.iter().filter(|s| !q.exe.is_empty() && s.facts.exe == q.exe && !held.contains(&s.id.as_str())).collect();
            if of_app.is_empty() {
                left.push(q);
                continue;
            }
            for s in of_app {
                if !permit(s.facts.pid) {
                    continue;
                }
                if restore_verdict(q.set_to, s.facts.level) {
                    if mixer.set(&s.id, q.before) {
                        tracing::info!(exe = q.exe, level = q.before, "other apps: a leftover back to its level");
                    }
                } else {
                    tracing::info!(exe = q.exe, now = s.facts.level, "other apps: a leftover's level changed meanwhile; left alone");
                }
            }
        }
        self.leftovers = left;
    }

    /// The rows the file keeps: the leftovers, then what's down now.
    fn rows(&self) -> Vec<&Quieted> {
        self.leftovers.iter().chain(self.lowered.iter().map(|e| &e.q)).collect()
    }
}

// ---- the ledger in memory, and its file ----------------------------------------------------

static LEDGER: Mutex<Ledger> = Mutex::new(Ledger { lowered: Vec::new(), leftovers: Vec::new() });

/// The ledger — and the one-at-a-time lock for the COM work: hold it while
/// turning down or bringing back.
pub(crate) fn ledger() -> std::sync::MutexGuard<'static, Ledger> {
    LEDGER.lock().unwrap_or_else(|p| p.into_inner())
}

pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// What Yap must never touch right now.
pub(crate) fn calls_now() -> Calls {
    let (names, live_browsers) = crate::calls::meeting_detect::call_audio();
    Calls { names, live_browsers, meeting: crate::meetings::meeting::is_recording() }
}

/// `h` turns down what the rules allow now (Windows' sessions; with the
/// ledger held), and the file says so.
pub(crate) fn lower(l: &mut Ledger, h: Holder) -> usize {
    let calls = calls_now();
    let permit = crate::shell::realtest::audio_permits;
    let rules = Rules { calls: &calls, own_pid: std::process::id(), permit: &permit };
    let mixer = platform::snapshot();
    let n = l.lower(h, &mixer, &rules, now_secs());
    write_down(l);
    n
}

/// `h` lets go (with the ledger held): its sessions to the other holder's
/// level or back to their own, then the leftovers once nothing's down.
pub(crate) fn release(l: &mut Ledger, h: Holder) {
    if !l.holds(h) && l.leftovers.is_empty() {
        return;
    }
    let mixer = platform::snapshot();
    l.release(h, &mixer);
    l.retry_leftovers(&mixer, &crate::shell::realtest::audio_permits, now_secs());
    write_down(l);
}

/// Yap is quitting (`mute::restore_all`): every session back now.
pub(crate) fn restore_all(l: &mut Ledger) {
    if l.lowered.is_empty() && l.leftovers.is_empty() {
        return;
    }
    let mixer = platform::snapshot();
    l.release(Holder::Dictation, &mixer);
    l.release(Holder::Voice, &mixer);
    l.retry_leftovers(&mixer, &crate::shell::realtest::audio_permits, now_secs());
    write_down(l);
}

fn leftovers_path() -> std::path::PathBuf {
    crate::config::data_dir().join("quieted_apps.json")
}

/// A crash's leftovers (`saving::Saver::load`: set aside if they aren't;
/// a file Yap couldn't read is never written over nor deleted until it
/// reads, [`write_down`]). A unit test never touches the real one.
fn load_leftovers() -> Vec<Quieted> {
    if cfg!(test) {
        return Vec::new();
    }
    SAVER.load(&leftovers_path())
}

/// Write what's down now and the leftovers (the file goes once nothing
/// is). A file that couldn't be read is read again first: its leftovers
/// join these (brought back after the next release).
fn write_down(l: &mut Ledger) {
    if cfg!(test) {
        return;
    }
    let path = leftovers_path();
    if SAVER.any_unread() {
        if let Some(disk) = SAVER.reread::<Vec<Quieted>>(&path, crate::shell::saving::json) {
            for q in disk {
                if !l.leftovers.contains(&q) && !l.lowered.iter().any(|e| e.q == q) {
                    l.leftovers.push(q);
                }
            }
        }
    }
    let rows = l.rows();
    if rows.is_empty() {
        // Never deleted while Yap couldn't read it.
        if SAVER.unread(&path) {
            return;
        }
        let _ = std::fs::remove_file(path);
        // Nothing left to keep: nothing unsaved either.
        SAVER.nothing_to_save();
        return;
    }
    let _ = SAVER.write_json(&path, &rows, true);
}

/// quieted_apps.json's saves (`saving.rs`): a failed one is tried again with
/// the next; only logged (it matters only if Yap crashes with an app down).
pub static SAVER: crate::shell::saving::Saver = crate::shell::saving::Saver::quiet("quietedApps", "the apps Yap quieted", save_now);

/// Write it again (Try again).
fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    write_down(&mut ledger());
    SAVER.check()
}

/// At start: what a crash left down comes back (off the main thread).
pub fn recover() {
    if crate::shell::realtest::other_apps() == crate::shell::realtest::OtherApps::Nobody {
        return;
    }
    let leftovers = load_leftovers();
    if leftovers.is_empty() {
        return;
    }
    tracing::info!(count = leftovers.len(), "other apps: bringing back what a crash left down");
    std::thread::spawn(move || {
        let mut l = ledger();
        for q in leftovers {
            if !l.leftovers.contains(&q) {
                l.leftovers.push(q);
            }
        }
        let mixer = platform::snapshot();
        l.retry_leftovers(&mixer, &crate::shell::realtest::audio_permits, now_secs());
        write_down(&mut l);
    });
}

/// The default output's mix rate (Hz): `mute.rs`'s Bluetooth wait.
pub(crate) fn default_output_rate() -> Option<u32> {
    platform::default_output_rate()
}

/// The robot's view (debug builds): the sessions `permit` lets through as
/// Windows has them now, and what Yap holds down.
#[cfg(debug_assertions)]
pub(crate) fn robot_view(permit: &dyn Fn(u32) -> bool) -> (Vec<serde_json::Value>, Vec<serde_json::Value>) {
    let held: Vec<serde_json::Value> = ledger()
        .lowered
        .iter()
        .map(|e| {
            serde_json::json!({
                "id": e.q.id, "exe": e.q.exe, "before": e.q.before, "setTo": e.q.set_to,
                "dictation": e.dictation, "voice": e.voice,
            })
        })
        .collect();
    let sessions = platform::snapshot()
        .sessions()
        .into_iter()
        .filter(|s| !s.facts.system && s.facts.pid != 0 && permit(s.facts.pid))
        .map(|s| serde_json::json!({ "pid": s.facts.pid, "exe": s.facts.exe, "level": s.facts.level }))
        .collect();
    (sessions, held)
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

    use super::{Live, Mixer, SessionFacts};

    /// COM on this thread (tolerating "already initialized" in another mode).
    fn com() -> bool {
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        hr.is_ok() || hr == RPC_E_CHANGED_MODE
    }

    /// Every audio session on every active output device, as it was when
    /// taken, each with its volume control.
    pub struct Snapshot {
        live: Vec<(Live, ISimpleAudioVolume)>,
    }

    impl Mixer for Snapshot {
        fn sessions(&self) -> Vec<Live> {
            self.live.iter().map(|(l, _)| l.clone()).collect()
        }
        fn set(&self, id: &str, level: f32) -> bool {
            let Some((_, volume)) = self.live.iter().find(|(l, _)| l.id == id) else {
                return false;
            };
            match unsafe { volume.SetMasterVolume(level.clamp(0.0, 1.0), std::ptr::null()) } {
                Ok(()) => true,
                Err(e) => {
                    tracing::info!("other apps: Windows refused the level ({e})");
                    false
                }
            }
        }
    }

    pub fn snapshot() -> Snapshot {
        let mut live = Vec::new();
        if !com() {
            return Snapshot { live };
        }
        let exes = crate::shell::procs::exe_names();
        unsafe {
            let Ok(enumerator) = CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) else {
                return Snapshot { live };
            };
            let Ok(devices) = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE) else {
                return Snapshot { live };
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
                    live.push((Live { id, facts: SessionFacts { pid, exe, system, level } }, volume));
                }
            }
        }
        Snapshot { live }
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
}

#[cfg(not(windows))]
mod platform {
    use super::{Live, Mixer};
    pub struct Snapshot;
    impl Mixer for Snapshot {
        fn sessions(&self) -> Vec<Live> {
            Vec::new()
        }
        fn set(&self, _: &str, _: f32) -> bool {
            false
        }
    }
    pub fn snapshot() -> Snapshot {
        Snapshot
    }
    pub fn default_output_rate() -> Option<u32> {
        None
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A session list in memory: `set` moves a session's level, and a test
    /// can move one itself (the person in the volume mixer) or end one.
    #[derive(Default)]
    pub(crate) struct Fake {
        pub live: RefCell<Vec<Live>>,
        pub refuse: RefCell<Vec<String>>,
    }

    impl Fake {
        pub fn with(sessions: &[(&str, u32, &str, f32)]) -> Fake {
            let f = Fake::default();
            for &(id, pid, exe, level) in sessions {
                f.live.borrow_mut().push(Live { id: id.into(), facts: SessionFacts { pid, exe: exe.into(), system: false, level } });
            }
            f
        }
        pub fn level(&self, id: &str) -> f32 {
            self.live.borrow().iter().find(|s| s.id == id).map(|s| s.facts.level).expect("a session")
        }
        /// The person (or the app) sets a level.
        pub fn move_to(&self, id: &str, level: f32) {
            if let Some(s) = self.live.borrow_mut().iter_mut().find(|s| s.id == id) {
                s.facts.level = level;
            }
        }
        pub fn end(&self, id: &str) {
            self.live.borrow_mut().retain(|s| s.id != id);
        }
    }

    impl Mixer for Fake {
        fn sessions(&self) -> Vec<Live> {
            self.live.borrow().clone()
        }
        fn set(&self, id: &str, level: f32) -> bool {
            if self.refuse.borrow().iter().any(|r| r == id) {
                return false;
            }
            match self.live.borrow_mut().iter_mut().find(|s| s.id == id) {
                Some(s) => {
                    s.facts.level = level;
                    true
                }
                None => false,
            }
        }
    }

    const YAP: u32 = 77;

    fn calls() -> Calls {
        Calls { names: vec!["ms-teams.exe".into(), "zoom.exe".into()], live_browsers: Vec::new(), meeting: false }
    }

    fn lower(l: &mut Ledger, h: Holder, m: &Fake) -> usize {
        let calls = calls();
        let permit = |_: u32| true;
        l.lower(h, m, &Rules { calls: &calls, own_pid: YAP, permit: &permit }, 1_000)
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    fn mixer() -> Fake {
        Fake::with(&[
            ("spotify", 10, "spotify.exe", 0.8),
            ("edge", 11, "msedge.exe", 0.5),
            ("yap", YAP, "yap.exe", 1.0),
            ("teams", 12, "ms-teams.exe", 0.9),
        ])
    }

    #[test]
    fn the_voice_turns_each_app_down_to_30_percent_of_its_own_level_and_back_exactly() {
        let m = mixer();
        let mut l = Ledger::default();
        assert_eq!(lower(&mut l, Holder::Voice, &m), 2);
        assert!(close(m.level("spotify"), 0.24) && close(m.level("edge"), 0.15));
        // Never Yap's own, never a call app.
        assert_eq!((m.level("yap"), m.level("teams")), (1.0, 0.9));
        assert!(l.holds(Holder::Voice) && !l.holds(Holder::Dictation));
        // Held already: asked again, nothing moves twice.
        assert_eq!(lower(&mut l, Holder::Voice, &m), 0);
        assert!(close(m.level("spotify"), 0.24));
        l.release(Holder::Voice, &m);
        assert_eq!((m.level("spotify"), m.level("edge")), (0.8, 0.5));
        assert!(l.lowered.is_empty() && l.leftovers.is_empty());
    }

    #[test]
    fn a_dictation_takes_a_ducked_app_over_and_brings_it_back_to_its_own_level() {
        let m = mixer();
        let mut l = Ledger::default();
        lower(&mut l, Holder::Voice, &m);
        // The dictation starts (mid-speech): its start sound, then silence.
        lower(&mut l, Holder::Dictation, &m);
        assert_eq!(m.level("spotify"), 0.0);
        // The voice's end mustn't bring the music up under the dictation.
        l.release(Holder::Voice, &m);
        assert_eq!(m.level("spotify"), 0.0);
        // The dictation's end: the level from before Yap spoke, not 30 %.
        l.release(Holder::Dictation, &m);
        assert_eq!((m.level("spotify"), m.level("edge")), (0.8, 0.5));
        assert!(l.lowered.is_empty());
    }

    #[test]
    fn handed_over_before_its_start_sound_it_stays_at_30_percent_until_the_dictation_acts() {
        let m = mixer();
        let mut l = Ledger::default();
        lower(&mut l, Holder::Voice, &m);
        l.hand_over(Holder::Voice, Holder::Dictation);
        assert!(close(m.level("spotify"), 0.24), "no jump up, no silence yet");
        assert!(!l.holds(Holder::Voice) && l.holds(Holder::Dictation));
        // The voice letting go later finds nothing of its own.
        l.release(Holder::Voice, &m);
        assert!(close(m.level("spotify"), 0.24));
        // After the start sound: silence; after the stop sound: its own level.
        lower(&mut l, Holder::Dictation, &m);
        assert_eq!(m.level("spotify"), 0.0);
        l.release(Holder::Dictation, &m);
        assert_eq!(m.level("spotify"), 0.8);
        // A dictation that ended before its start sound brings them back too.
        let m = mixer();
        let mut l = Ledger::default();
        lower(&mut l, Holder::Voice, &m);
        l.hand_over(Holder::Voice, Holder::Dictation);
        l.release(Holder::Dictation, &m);
        assert_eq!(m.level("spotify"), 0.8);
    }

    #[test]
    fn the_voice_after_a_dictation_ducks_when_the_dictation_lets_go() {
        let m = mixer();
        let mut l = Ledger::default();
        lower(&mut l, Holder::Dictation, &m);
        // An answer starts while the dictation's sound is still down.
        lower(&mut l, Holder::Voice, &m);
        assert_eq!(m.level("spotify"), 0.0);
        l.release(Holder::Dictation, &m);
        assert!(close(m.level("spotify"), 0.24), "the voice still holds it");
        l.release(Holder::Voice, &m);
        assert_eq!(m.level("spotify"), 0.8);
    }

    #[test]
    fn a_level_the_person_moved_is_left_alone() {
        let m = mixer();
        let mut l = Ledger::default();
        lower(&mut l, Holder::Voice, &m);
        // Turned up in the volume mixer while Yap spoke.
        m.move_to("spotify", 0.6);
        l.release(Holder::Voice, &m);
        assert_eq!(m.level("spotify"), 0.6);
        assert_eq!(m.level("edge"), 0.5);
        // Moved, then a dictation: it's the person's level now, not Yap's.
        let m = mixer();
        let mut l = Ledger::default();
        lower(&mut l, Holder::Voice, &m);
        m.move_to("edge", 0.7);
        lower(&mut l, Holder::Dictation, &m);
        assert_eq!(m.level("edge"), 0.7);
        assert_eq!(m.level("spotify"), 0.0);
        l.release(Holder::Dictation, &m);
        l.release(Holder::Voice, &m);
        assert_eq!((m.level("spotify"), m.level("edge")), (0.8, 0.7));
    }

    #[test]
    fn a_session_gone_at_its_release_comes_back_by_app_but_never_one_held_now() {
        let m = mixer();
        let mut l = Ledger::default();
        lower(&mut l, Holder::Voice, &m);
        m.end("spotify");
        l.release(Holder::Voice, &m);
        assert_eq!(l.leftovers.len(), 1);
        // Spotify again, its new session still at Yap's level (Windows kept it).
        m.live.borrow_mut().push(Live { id: "spotify-2".into(), facts: SessionFacts { pid: 20, exe: "spotify.exe".into(), system: false, level: 0.24 } });
        // While a dictation holds that new session, the leftover leaves it be.
        lower(&mut l, Holder::Dictation, &m);
        assert_eq!(m.level("spotify-2"), 0.0);
        l.retry_leftovers(&m, &|_| true, 2_000);
        assert_eq!(m.level("spotify-2"), 0.0);
        // The dictation's release puts it back to its own level as it found it (0.24),
        // then the leftover — at Yap's 30 % — goes back to 0.8.
        l.release(Holder::Dictation, &m);
        assert!(close(m.level("spotify-2"), 0.24));
        l.retry_leftovers(&m, &|_| true, 2_000);
        assert_eq!(m.level("spotify-2"), 0.8);
        assert!(l.leftovers.is_empty());
        // A leftover a week old is given up on.
        l.leftovers.push(Quieted { id: "x".into(), exe: "vlc.exe".into(), before: 0.5, set_to: 0.0, at: 0 });
        l.retry_leftovers(&m, &|_| true, LEFTOVER_DAYS * 24 * 3600);
        assert!(l.leftovers.is_empty());
    }

    #[test]
    fn a_session_windows_wont_set_isnt_held() {
        let m = mixer();
        m.refuse.borrow_mut().push("edge".into());
        let mut l = Ledger::default();
        assert_eq!(lower(&mut l, Holder::Voice, &m), 1);
        assert_eq!(l.lowered.len(), 1);
        assert_eq!(m.level("edge"), 0.5);
    }

    #[test]
    fn the_levels_each_holder_wants() {
        assert_eq!(wanted(Holder::Dictation, 0.8), 0.0);
        assert!(close(wanted(Holder::Voice, 0.8), 0.24));
        assert!(close(wanted(Holder::Voice, 1.0), VOICE_SHARE));
        // A session already near silence isn't moved for the voice.
        let m = Fake::with(&[("quiet", 10, "spotify.exe", 0.006)]);
        let mut l = Ledger::default();
        assert_eq!(lower(&mut l, Holder::Voice, &m), 0);
    }

    #[test]
    fn music_and_videos_go_quiet() {
        let s = |pid, exe: &str, level| SessionFacts { pid, exe: exe.into(), system: false, level };
        assert_eq!(quiet_verdict(&s(10, "spotify.exe", 0.8), &calls(), YAP), Ok(()));
        assert_eq!(quiet_verdict(&s(11, "msedge.exe", 1.0), &calls(), YAP), Ok(()));
        assert_eq!(quiet_verdict(&s(YAP, "yap.exe", 1.0), &calls(), YAP), Err("Yap's own"));
        let system = SessionFacts { system: true, ..s(4, "audiodg.exe", 1.0) };
        assert_eq!(quiet_verdict(&system, &calls(), YAP), Err("system sounds"));
        assert_eq!(quiet_verdict(&s(0, "", 1.0), &calls(), YAP), Err("system sounds"));
        assert_eq!(quiet_verdict(&s(20, "zoom.exe", 1.0), &calls(), YAP), Err("a call app"));
        assert_eq!(quiet_verdict(&s(21, "", 1.0), &calls(), YAP), Err("an unknown process"));
        assert_eq!(quiet_verdict(&s(22, "spotify.exe", 0.0), &calls(), YAP), Err("already silent"));
    }

    #[test]
    fn a_browser_with_a_meeting_is_left_alone() {
        let s = |pid, exe: &str| SessionFacts { pid, exe: exe.into(), system: false, level: 1.0 };
        let live = Calls { live_browsers: vec!["chrome.exe".into()], ..calls() };
        assert_eq!(quiet_verdict(&s(30, "chrome.exe"), &live, YAP), Err("the browser a meeting is in"));
        assert_eq!(quiet_verdict(&s(31, "msedge.exe"), &live, YAP), Ok(()));
        let recording = Calls { meeting: true, ..calls() };
        assert_eq!(quiet_verdict(&s(31, "msedge.exe"), &recording, YAP), Err("a browser, while a meeting records"));
        assert_eq!(quiet_verdict(&s(10, "spotify.exe"), &recording, YAP), Ok(()));
    }

    #[test]
    fn only_a_session_still_at_yaps_level_comes_back() {
        assert!(restore_verdict(0.0, 0.0));
        assert!(restore_verdict(0.0, 0.003));
        assert!(!restore_verdict(0.0, 0.4));
    }

    #[test]
    fn leftovers_read_as_written() {
        let q = Quieted { id: "{0.0.0}|x%b{1}|1%b10".into(), exe: "spotify.exe".into(), before: 0.8, set_to: 0.24, at: 5 };
        let json = serde_json::to_string(&vec![q.clone()]).unwrap();
        assert!(json.contains("\"setTo\":0.24"));
        let back: Vec<Quieted> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, vec![q]);
    }
}

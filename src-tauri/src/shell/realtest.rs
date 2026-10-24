//! **Robot mode**: the real-app tests (`e2e-real/`, `npm run test:real`; see
//! docs/e2e-tests.md, "Real-app robot tests").
//!
//! Test mode (`e2e.rs`) never pastes, types, presses a key or reads anything
//! in another app. Robot mode — a **debug** build started with `YAP_E2E=1`
//! *and* `YAP_REALTEST=1` — keeps all of test mode's isolation (portable data
//! dir, no global keyboard hook, no account service, no notifications, no
//! focus stealing…) but lets the real paste, the submit key, "Paste last
//! transcript" and the UI Automation reads of the target field run — **only
//! into windows the test opened itself**:
//!
//! - a window of a process in `YAP_REALTEST_PIDS` (comma-separated) or of
//!   one of its descendants, each matched by when it started too, so an id
//!   Windows has since given to another process never counts;
//! - or a window whose title contains `YAP_REALTEST_NONCE` (a random value
//!   of at least 12 characters the harness puts in the title of every window
//!   it opens).
//!
//! Everything else — the developer's own windows, Yap's own — is refused
//! ([`gate`]): logged, said in the UI (`yap-error`), nothing sent. The
//! foreground is checked again right before every `SendInput` (the paste's
//! Ctrl+V, typing, the submit key: [`recheck_foreground`]), so a click
//! elsewhere mid-run can't redirect a paste. The UI Automation reads and
//! `app_kind`'s look at the window's title follow the same rule
//! ([`permits`]), quietly.
//!
//! The robot's dictations come from debug-only commands instead of a mic
//! and a hotkey: [`commands::realtest_dictate_text`] (a transcript through cleanup,
//! the dictionary, snippets, the fitting and the paste) and
//! [`commands::realtest_dictate_wav`] (a WAV played into the dictation mic through the
//! real start/stop path, so the STT really runs on an `engines` build).
//!
//! Release builds compile robot mode out: [`on`] is always `false`, [`gate`]
//! always [`Gate::Go`] (a normal run) and the commands don't exist.

/// What a paste, a key press or a read may do with a window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gate {
    /// Go ahead: a normal run, or robot mode and a window the test opened.
    Go,
    /// An e2e test run (not the robot): never into another app.
    TestMode,
    /// Robot mode refused it (the reason, already logged and shown).
    #[cfg_attr(not(debug_assertions), allow(dead_code))] // robot mode is debug-only
    Refused(String),
}

/// `true` in a debug build started with `YAP_E2E=1` and `YAP_REALTEST=1`.
pub fn on() -> bool {
    #[cfg(debug_assertions)]
    {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ON.get_or_init(|| {
            crate::shell::e2e::active() && std::env::var("YAP_REALTEST").is_ok_and(|v| v.trim() == "1")
        })
    }
    #[cfg(not(debug_assertions))]
    false
}

/// May Yap paste into, press a key in, or read the field of `hwnd` (`what`:
/// "paste", "press Enter"… for the log)? A refusal is logged and shown.
pub fn gate(hwnd: Option<isize>, what: &str) -> Gate {
    if !crate::shell::e2e::active() {
        return Gate::Go;
    }
    if !on() {
        return Gate::TestMode;
    }
    #[cfg(debug_assertions)]
    {
        match robot::verdict(hwnd) {
            Ok(_) => Gate::Go,
            Err(why) => Gate::Refused(robot::refuse(what, &why)),
        }
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = (hwnd, what);
        Gate::TestMode
    }
}

/// Robot mode: process names (lower case, without ".exe") that `app_kind`
/// treats as editors with an integrated terminal besides VS Code and co.
/// (`YAP_REALTEST_EDITORS`, comma-separated), so a page of the test's own
/// in Edge can stand in for VS Code's terminal. Empty in a normal run and
/// in release builds.
pub fn stand_in_editors() -> &'static [String] {
    #[cfg(debug_assertions)]
    {
        static EDITORS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
        EDITORS.get_or_init(|| {
            if !on() {
                return Vec::new();
            }
            std::env::var("YAP_REALTEST_EDITORS")
                .unwrap_or_default()
                .split(',')
                .map(|e| e.trim().to_lowercase().trim_end_matches(".exe").to_string())
                .filter(|e| !e.is_empty())
                .collect()
        })
    }
    #[cfg(not(debug_assertions))]
    {
        &[]
    }
}

/// [`gate`], quietly, for reads (the text around the cursor, the window's
/// title): a normal run, or robot mode and a window the test opened.
pub fn permits(hwnd: Option<isize>) -> bool {
    if !crate::shell::e2e::active() {
        return true;
    }
    #[cfg(debug_assertions)]
    {
        on() && robot::verdict(hwnd).is_ok()
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = hwnd;
        false
    }
}

/// Right before a `SendInput`: in robot mode the window in front must be
/// one the test opened, and the one Yap meant (`expected`, when it has one)
/// — the developer may have clicked elsewhere since. Always `Ok` in a
/// normal run.
pub fn recheck_foreground(expected: Option<isize>, what: &str) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Ok(());
    }
    if !on() {
        return Err("test mode never sends input to other apps".into());
    }
    #[cfg(debug_assertions)]
    {
        robot::recheck(expected).map_err(|why| robot::refuse(what, &why))
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = (expected, what);
        Err("test mode never sends input to other apps".into())
    }
}

/// Whose sound Yap may lower or pause while dictating ("Silence other apps",
/// "Pause music and videos": `mute.rs`, `media_pause.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OtherApps {
    /// A normal run: any app's (the rules in `mute.rs` still apply).
    All,
    /// Robot mode: only what the test started ([`audio_permits`],
    /// [`media_permits`]), never the developer's own music.
    TestsOwn,
    /// An e2e test run: nobody's.
    Nobody,
}

pub fn other_apps() -> OtherApps {
    if !crate::shell::e2e::active() {
        OtherApps::All
    } else if on() {
        OtherApps::TestsOwn
    } else {
        OtherApps::Nobody
    }
}

/// May Yap lower the audio session of process `pid`? A normal run: yes. A
/// test run: no. Robot mode: only if that process, or one it descends from,
/// is one the test started (`YAP_REALTEST_PIDS`) or owns a window whose
/// title has the run's nonce (the harness's Edge window: its sound plays
/// from a child process of the window's).
pub fn audio_permits(pid: u32) -> bool {
    match other_apps() {
        OtherApps::All => true,
        OtherApps::Nobody => false,
        #[cfg(debug_assertions)]
        OtherApps::TestsOwn => match robot::audio_verdict(pid) {
            Ok(why) => {
                tracing::debug!(pid, why, "robot: another app's sound may be changed");
                true
            }
            Err(why) => {
                tracing::debug!(pid, why, "robot: another app's sound left alone");
                false
            }
        },
        #[cfg(not(debug_assertions))]
        OtherApps::TestsOwn => {
            let _ = pid;
            false
        }
    }
}

/// May Yap pause or play the media titled `title` (Windows' media controls
/// name no process)? A normal run: yes. A test run: no. Robot mode: only if
/// the title has the run's nonce (the harness's page sets it).
pub fn media_permits(title: &str) -> bool {
    match other_apps() {
        OtherApps::All => true,
        OtherApps::Nobody => false,
        #[cfg(debug_assertions)]
        OtherApps::TestsOwn => robot::allowed()
            .nonce
            .as_deref()
            .is_some_and(|n| n.chars().count() >= MIN_NONCE && title.contains(n)),
        #[cfg(not(debug_assertions))]
        OtherApps::TestsOwn => {
            let _ = title;
            false
        }
    }
}

/// Right after a `SendInput`: in robot mode, whether the window Yap meant is
/// still in front. The check before the keys leaves a moment between it and
/// Windows delivering them; a window that took the focus in that moment
/// (a click elsewhere) may have got them, and this says so loudly: logged,
/// shown, and listed by `realtest_refusals` as "may have gone to…", which
/// the robot suite fails on. Nothing in a normal run.
pub fn check_after(expected: Option<isize>, what: &str) {
    #[cfg(debug_assertions)]
    if on() {
        robot::check_after(expected, what);
    }
    #[cfg(not(debug_assertions))]
    let _ = (expected, what);
}

// ---- Robot mode's guard, pure (unit-tested) ----

/// What the test allowed: its processes (id, when it started) and the nonce
/// in its windows' titles.
#[cfg_attr(not(debug_assertions), allow(dead_code))]
#[derive(Clone, Debug, Default)]
pub(crate) struct Allowed {
    pub pids: Vec<(u32, u64)>,
    pub nonce: Option<String>,
}

/// The window Yap would act on: its process, its title, and its process's
/// lineage (itself first, then its parents, with when each started).
#[cfg_attr(not(debug_assertions), allow(dead_code))]
#[derive(Clone, Debug, Default)]
pub(crate) struct WindowFacts {
    pub pid: u32,
    pub title: String,
    pub lineage: Vec<(u32, u64)>,
}

/// The shortest nonce Yap accepts: long enough that no window of the
/// developer's has it in its title by chance.
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub(crate) const MIN_NONCE: usize = 12;

/// `YAP_REALTEST_PIDS`: "123, 456" → [123, 456] (anything else skipped).
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub(crate) fn parse_pids(s: &str) -> Vec<u32> {
    s.split([',', ';', ' '])
        .filter_map(|p| p.trim().parse::<u32>().ok())
        .filter(|&p| p != 0)
        .collect()
}

/// `YAP_REALTEST_NONCE`, if it's one Yap accepts (≥ 12 characters, no spaces).
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub(crate) fn valid_nonce(s: &str) -> Option<String> {
    let s = s.trim();
    (s.chars().count() >= MIN_NONCE && !s.contains(char::is_whitespace)).then(|| s.to_string())
}

/// Robot mode's rule: Yap's own windows never; a window whose title has the
/// nonce; a window of an allowed process or of a descendant of one (the
/// same process: same id AND the same start time). `Ok` says why.
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub(crate) fn decide(w: &WindowFacts, allowed: &Allowed, own_pid: u32) -> Result<&'static str, String> {
    if w.pid == 0 {
        return Err("no window".into());
    }
    if w.pid == own_pid {
        return Err("it's one of Yap's own windows".into());
    }
    if let Some(nonce) = allowed.nonce.as_deref() {
        if nonce.chars().count() >= MIN_NONCE && w.title.contains(nonce) {
            return Ok("its title has the test's nonce");
        }
    }
    for (i, &(pid, started)) in w.lineage.iter().enumerate() {
        if allowed.pids.iter().any(|&(p, s)| p == pid && s == started) {
            return Ok(if i == 0 {
                "the test started its process"
            } else {
                "the test started its process's parent"
            });
        }
    }
    Err(format!("the test didn't open it (process {})", w.pid))
}

/// Robot mode's rule for another app's sound: the process playing it (the
/// first of `lineage`, then its parents, with when each started), or one it
/// descends from, is one the test started, or a process whose windows are
/// all the test's (`nonce_owners`: every visible, titled window it has
/// carries the nonce — so a process shared with the developer's own windows,
/// say a Windows Terminal hosting both, never counts). Never Yap itself.
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub(crate) fn decide_audio(
    lineage: &[(u32, u64)],
    nonce_owners: &[u32],
    allowed: &Allowed,
    own_pid: u32,
) -> Result<&'static str, String> {
    let Some(&(pid, _)) = lineage.first() else {
        return Err("the process is gone".into());
    };
    if pid == own_pid {
        return Err("it's Yap's own sound".into());
    }
    for &(p, started) in lineage {
        if allowed.pids.iter().any(|&(a, s)| a == p && s == started) {
            return Ok("the test started its process (or a parent)");
        }
        if allowed.nonce.is_some() && nonce_owners.contains(&p) {
            return Ok("its process (or a parent) has the test's window");
        }
    }
    Err(format!("the test didn't start it (process {pid})"))
}

#[cfg(debug_assertions)]
mod robot {
    use std::sync::{Mutex, OnceLock};

    use tauri::{AppHandle, Emitter};

    use super::{decide, decide_audio, parse_pids, valid_nonce, Allowed, WindowFacts};

    static ALLOWED: OnceLock<Allowed> = OnceLock::new();
    static APP: OnceLock<AppHandle> = OnceLock::new();

    /// What the environment allows, read once (when robot mode starts, so
    /// the start times are those of the test's own processes).
    pub fn allowed() -> &'static Allowed {
        ALLOWED.get_or_init(|| {
            let pids = parse_pids(&std::env::var("YAP_REALTEST_PIDS").unwrap_or_default());
            let nonce = valid_nonce(&std::env::var("YAP_REALTEST_NONCE").unwrap_or_default());
            #[cfg(windows)]
            let pids = pids
                .into_iter()
                .filter_map(|pid| crate::shell::procs::created(pid).map(|s| (pid, s)))
                .collect();
            #[cfg(not(windows))]
            let pids = {
                let _ = pids;
                Vec::new()
            };
            Allowed { pids, nonce }
        })
    }

    pub fn init(app: &AppHandle) {
        let _ = APP.set(app.clone());
        let a = allowed();
        tracing::info!(
            processes = a.pids.len(),
            nonce = a.nonce.is_some(),
            "e2e: robot mode on: real pastes, keys and reads, only into the test's own windows"
        );
    }

    /// Every refusal (and "may have gone to…" warning) so far, oldest first,
    /// for the harness (`realtest_refusals`; a robot lives for one spec).
    pub static REFUSALS: Mutex<Vec<String>> = Mutex::new(Vec::new());

    pub fn refuse(what: &str, why: &str) -> String {
        let msg = format!("Robot mode refused to {what}: {why}");
        tracing::warn!("{msg}");
        if let Ok(mut list) = REFUSALS.lock() {
            list.push(msg.clone());
        }
        if let Some(app) = APP.get() {
            let _ = app.emit("yap-error", msg.clone());
        }
        msg
    }

    /// The guard's answer for `hwnd` (its top-level window).
    pub fn verdict(hwnd: Option<isize>) -> Result<&'static str, String> {
        let hwnd = hwnd.ok_or("no target window")?;
        let facts = win::facts(hwnd).ok_or("the window is gone")?;
        decide(&facts, allowed(), std::process::id())
    }

    /// The guard's answer for the sound of process `pid`.
    pub fn audio_verdict(pid: u32) -> Result<&'static str, String> {
        #[cfg(windows)]
        let lineage = crate::shell::procs::lineage(pid);
        #[cfg(not(windows))]
        let lineage = vec![(pid, 0)];
        let owners = allowed().nonce.as_deref().map(win::nonce_owners).unwrap_or_default();
        decide_audio(&lineage, &owners, allowed(), std::process::id())
    }

    /// Checked as `SendInput` returns and again a moment later, by when
    /// Windows has queued the keys to the window that had the focus.
    pub fn check_after(expected: Option<isize>, what: &str) {
        for wait_ms in [0u64, 10] {
            std::thread::sleep(std::time::Duration::from_millis(wait_ms));
            let fg = win::foreground();
            let moved = match expected {
                Some(t) => win::root(t) != fg,
                None => verdict(fg).is_err(),
            };
            if moved {
                let pid = fg.and_then(win::facts).map(|f| f.pid).unwrap_or(0);
                refuse_after(what, pid);
                return;
            }
        }
    }

    /// A "may have gone to…" warning (see [`super::check_after`]).
    fn refuse_after(what: &str, pid: u32) {
        let msg = format!(
            "Robot mode: the focus moved during the {what} — the keys may have gone to another window (process {pid})"
        );
        tracing::error!("{msg}");
        if let Ok(mut list) = REFUSALS.lock() {
            list.push(msg.clone());
        }
        if let Some(app) = APP.get() {
            let _ = app.emit("yap-error", msg);
        }
    }

    pub fn recheck(expected: Option<isize>) -> Result<(), String> {
        let fg = win::foreground().ok_or("nothing has the focus")?;
        if let Some(t) = expected {
            if win::root(t) != Some(fg) {
                return Err("the test's window lost the focus".into());
            }
        }
        verdict(Some(fg)).map(|_| ())
    }

    /// The top-level window in front (0 never).
    pub fn foreground() -> Option<isize> {
        win::foreground()
    }

    pub fn root(hwnd: isize) -> Option<isize> {
        win::root(hwnd)
    }

    #[cfg(windows)]
    mod win {
        use std::ffi::c_void;

        use super::WindowFacts;

        // Declared as text_injector.rs / app_kind.rs declare them
        // (`clashing_extern_declarations`).
        #[link(name = "user32")]
        extern "system" {
            fn GetForegroundWindow() -> *mut c_void;
            fn GetAncestor(hwnd: *mut c_void, flags: u32) -> *mut c_void;
            fn IsWindow(hwnd: *mut c_void) -> i32;
            fn IsWindowVisible(hwnd: *mut c_void) -> i32;
            fn GetWindowThreadProcessId(hwnd: *mut c_void, pid: *mut u32) -> u32;
            fn GetWindowTextW(hwnd: *mut c_void, text: *mut u16, max: i32) -> i32;
            fn EnumWindows(callback: unsafe extern "system" fn(*mut c_void, isize) -> i32, lparam: isize) -> i32;
        }
        const GA_ROOT: u32 = 2;

        fn title_of(h: *mut c_void) -> String {
            let mut buf = [0u16; 512];
            let n = unsafe { GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32) };
            String::from_utf16_lossy(&buf[..n.clamp(0, buf.len() as i32) as usize])
        }

        /// The processes whose visible, titled top-level windows all have
        /// `nonce` in their titles (at least one).
        pub fn nonce_owners(nonce: &str) -> Vec<u32> {
            unsafe extern "system" fn collect(hwnd: *mut c_void, lparam: isize) -> i32 {
                if IsWindowVisible(hwnd) != 0 {
                    (*(lparam as *mut Vec<isize>)).push(hwnd as isize);
                }
                1
            }
            let mut hwnds: Vec<isize> = Vec::new();
            unsafe { EnumWindows(collect, &mut hwnds as *mut Vec<isize> as isize) };
            let mut ours: Vec<u32> = Vec::new();
            let mut theirs: Vec<u32> = Vec::new();
            for hwnd in hwnds {
                let h = hwnd as *mut c_void;
                let title = title_of(h);
                if title.is_empty() {
                    continue;
                }
                let mut pid = 0u32;
                unsafe { GetWindowThreadProcessId(h, &mut pid) };
                if title.contains(nonce) {
                    ours.push(pid);
                } else {
                    theirs.push(pid);
                }
            }
            ours.retain(|p| *p != 0 && !theirs.contains(p));
            ours.sort_unstable();
            ours.dedup();
            ours
        }

        pub fn root(hwnd: isize) -> Option<isize> {
            let h = hwnd as *mut c_void;
            if h.is_null() || unsafe { IsWindow(h) } == 0 {
                return None;
            }
            let r = unsafe { GetAncestor(h, GA_ROOT) };
            Some(if r.is_null() { hwnd } else { r as isize })
        }

        pub fn foreground() -> Option<isize> {
            let fg = unsafe { GetForegroundWindow() } as isize;
            (fg != 0).then(|| root(fg)).flatten()
        }

        pub fn facts(hwnd: isize) -> Option<WindowFacts> {
            let root = root(hwnd)?;
            let h = root as *mut c_void;
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(h, &mut pid) };
            let title = title_of(h);
            Some(WindowFacts {
                pid,
                title,
                lineage: crate::shell::procs::lineage(pid),
            })
        }
    }

    #[cfg(not(windows))]
    mod win {
        use super::WindowFacts;
        pub fn root(_: isize) -> Option<isize> {
            None
        }
        pub fn foreground() -> Option<isize> {
            None
        }
        pub fn facts(_: isize) -> Option<WindowFacts> {
            None
        }
        pub fn nonce_owners(_: &str) -> Vec<u32> {
            Vec::new()
        }
    }
}

/// Robot mode starts (from `e2e::start`): read what the test allowed, now,
/// while its processes are the ones it started.
pub fn start(app: &tauri::AppHandle) {
    #[cfg(debug_assertions)]
    if on() {
        robot::init(app);
    }
    #[cfg(not(debug_assertions))]
    let _ = app;
}

// ---- The robot's commands (debug builds only) ----

#[cfg(debug_assertions)]
pub mod commands {
    use tauri::State;

    use super::{on, robot};
    use crate::dictation::pipeline::Delivered;
    use crate::AppState;

    fn only_robot() -> Result<(), String> {
        if on() {
            Ok(())
        } else {
            Err("Only in robot mode (YAP_E2E=1 and YAP_REALTEST=1, a debug build)".into())
        }
    }

    /// The test's window (`hwnd`) must be the one in front: a dictation goes
    /// into the foreground window, as with the hotkey.
    fn in_front(hwnd: Option<i64>) -> Result<(), String> {
        let Some(h) = hwnd else {
            return Ok(());
        };
        let fg = robot::foreground().ok_or("nothing has the focus")?;
        if robot::root(h as isize) != Some(fg) {
            return Err("The test's window isn't in front (something else has the focus)".into());
        }
        Ok(())
    }

    fn robot_of(state: &State<'_, AppState>) -> Result<crate::dictation::pipeline::Robot, String> {
        let guard = state.pipeline.lock().map_err(|_| "the pipeline lock is poisoned")?;
        Ok(guard.as_ref().ok_or("no pipeline")?.robot())
    }

    /// The guard's verdict on a window, without doing anything to it: why
    /// it may be touched, or why not.
    #[tauri::command]
    pub fn realtest_window(hwnd: i64) -> Result<String, String> {
        only_robot()?;
        robot::verdict(Some(hwnd as isize)).map(str::to_string)
    }

    /// The refusals so far (each also logged and shown as `yap-error`).
    #[tauri::command]
    pub fn realtest_refusals() -> Result<Vec<String>, String> {
        only_robot()?;
        Ok(robot::REFUSALS.lock().map(|l| l.clone()).unwrap_or_default())
    }

    /// A dictation from a transcript: `text` as if the STT had just heard
    /// it, into the window in front (`hwnd`, when given, must be it) —
    /// through the same steps as a spoken one: the target and the text
    /// around its cursor read at the "start", AI cleanup, the dictionary,
    /// "press enter", snippets, fitting to the kind of app and the cursor,
    /// the paste (guarded), the submit key, history. Returns what went to
    /// the paste.
    #[tauri::command]
    pub async fn realtest_dictate_text(
        state: State<'_, AppState>,
        text: String,
        hwnd: Option<i64>,
    ) -> Result<Delivered, String> {
        only_robot()?;
        in_front(hwnd)?;
        let robot = robot_of(&state)?;
        robot.dictate_text(text).await
    }

    /// The rewrite shortcut with `text` as the spoken instruction, in the
    /// window in front (`hwnd`, when given, must be it): its selection read
    /// (UI Automation; no Ctrl+C in robot mode) — none, right after a
    /// dictation there, and that dictation is rewritten in place
    /// (`as_said.rs`) — then the AI (the cleanup model, unless Commands has
    /// its own) and the paste. Returns what it did.
    #[tauri::command]
    pub async fn realtest_rewrite_text(
        state: State<'_, AppState>,
        text: String,
        hwnd: Option<i64>,
    ) -> Result<crate::dictation::pipeline::Rewritten, String> {
        only_robot()?;
        in_front(hwnd)?;
        let robot = robot_of(&state)?;
        robot.rewrite_text(text).await
    }

    /// Saved transform `id` as its shortcut runs it, in the window in front
    /// (`hwnd`, when given, must be it): its selection read (UI Automation,
    /// an edit control's own; Ctrl+C never in test mode) — none, right after
    /// a dictation there, and that dictation is transformed in place — then
    /// the AI and the paste (`transforms.rs`). Returns what it did.
    #[tauri::command]
    pub async fn realtest_transform(app: tauri::AppHandle, id: String, hwnd: Option<i64>) -> Result<crate::dictation::transforms::Transformed, String> {
        only_robot()?;
        in_front(hwnd)?;
        tauri::async_runtime::spawn_blocking(move || {
            crate::dictation::transforms::run_by_id(&app, &id, crate::dictation::transforms::Origin::Shortcut, None)
        })
        .await
        .map_err(|e| format!("the transform task failed: {e}"))?
    }

    /// A dictation from a WAV file (`path`, any format Upload reads): played
    /// into the dictation mic in real time through the real start/stop path
    /// (the mic's own audio is left out meanwhile), so the STT, the live
    /// preview and the dead-mic watchdog all run. `null`: nothing was
    /// transcribed (a silent mic). `cancel`: cancelled at the end instead,
    /// as the bar's ✕ does (kept for Undo: `bar_action` "dictation-undo").
    #[tauri::command]
    pub async fn realtest_dictate_wav(
        state: State<'_, AppState>,
        path: String,
        hwnd: Option<i64>,
        cancel: Option<bool>,
    ) -> Result<Option<Delivered>, String> {
        only_robot()?;
        in_front(hwnd)?;
        let samples = tauri::async_runtime::spawn_blocking(move || {
            crate::dictation::media::decode_to_16k_mono(std::path::Path::new(&path))
        })
        .await
        .map_err(|e| format!("decode task failed: {e}"))??;
        let robot = robot_of(&state)?;
        robot.dictate_audio(samples, cancel.unwrap_or(false)).await
    }

    /// What the Read aloud shortcut does, in the window in front (`hwnd`,
    /// when given, must be it): its selection read (UI Automation, an edit
    /// control's own, Ctrl+C never in test mode) and read aloud — into
    /// `tts_e2e_spoken`, nothing played. Returns how it was read and what.
    #[tauri::command]
    pub async fn realtest_read_selection(app: tauri::AppHandle, hwnd: Option<i64>) -> Result<crate::voice::read_aloud::Picked, String> {
        only_robot()?;
        in_front(hwnd)?;
        tauri::async_runtime::spawn_blocking(move || {
            crate::voice::read_aloud::read_selection_now(&app, crate::voice::read_aloud::Origin::Shortcut)
        })
        .await
        .map_err(|e| e.to_string())
    }

    /// What the latest dictation sent to the paste (an Undo's), taken:
    /// `null` until one has since the last ask.
    #[tauri::command]
    pub fn realtest_delivered(state: State<'_, AppState>) -> Result<Option<Delivered>, String> {
        only_robot()?;
        Ok(robot_of(&state)?.take_delivered())
    }

    /// The clipboard, for the clipboard specs, without ever handing its
    /// contents out: `save` keeps what's there (the developer's) in memory
    /// and `restore` puts it back; `setText` / `setImage` put a test value
    /// there (privately: out of Win+V history); `check` compares what's
    /// there with `text` and with the last test image.
    /// Other apps' sound as robot mode may see it: the audio sessions and
    /// the media of processes the test started (`audio_permits`,
    /// `media_permits`: never anyone else's), with what Yap holds down
    /// (`mute::robot_view`).
    #[tauri::command]
    pub async fn realtest_other_apps() -> Result<serde_json::Value, String> {
        only_robot()?;
        tauri::async_runtime::spawn_blocking(crate::dictation::mute::robot_view).await.map_err(|e| e.to_string())
    }

    #[tauri::command]
    pub fn realtest_clipboard(action: String, text: Option<String>) -> Result<serde_json::Value, String> {
        only_robot()?;
        #[cfg(windows)]
        {
            crate::pasting::text_injector::robot_clipboard(&action, text.as_deref())
        }
        #[cfg(not(windows))]
        {
            let _ = (action, text);
            Err("Windows only".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(pid: u32, title: &str, lineage: &[(u32, u64)]) -> WindowFacts {
        WindowFacts {
            pid,
            title: title.into(),
            lineage: lineage.to_vec(),
        }
    }

    const NONCE: &str = "yaprobot-3f9a1c2b7d";

    fn allowed() -> Allowed {
        Allowed {
            pids: vec![(100, 5_000)],
            nonce: Some(NONCE.into()),
        }
    }

    #[test]
    fn the_tests_own_process_and_its_children_are_allowed() {
        assert!(decide(&window(100, "Test pad", &[(100, 5_000), (4, 1)]), &allowed(), 7).is_ok());
        // A child (Edge's renderer, a shell started by it), started later.
        assert!(decide(&window(200, "", &[(200, 6_000), (100, 5_000), (4, 1)]), &allowed(), 7).is_ok());
    }

    #[test]
    fn a_reused_process_id_is_not_the_tests() {
        // Same id, started at another time: Windows gave it to someone else.
        assert!(decide(&window(100, "Mail", &[(100, 9_000)]), &allowed(), 7).is_err());
        assert!(decide(&window(300, "Mail", &[(300, 9_500), (100, 9_000)]), &allowed(), 7).is_err());
    }

    #[test]
    fn a_window_titled_with_the_nonce_is_allowed_whoever_owns_it() {
        let title = format!("{NONCE} - Notepad");
        assert!(decide(&window(900, &title, &[(900, 1)]), &allowed(), 7).is_ok());
        // Windows Terminal's window, in the developer's own terminal process.
        assert!(decide(&window(901, &format!("✳ {NONCE}"), &[]), &allowed(), 7).is_ok());
    }

    #[test]
    fn everything_else_is_refused() {
        // The developer's own windows: a terminal running Claude Code, an editor.
        assert!(decide(&window(901, "✳ Fix the build", &[(901, 1)]), &allowed(), 7).is_err());
        assert!(decide(&window(902, "notes.txt - Notepad", &[(902, 1), (100, 0)]), &allowed(), 7).is_err());
        // A part of the nonce isn't the nonce.
        assert!(decide(&window(903, "yaprobot-3f9a", &[]), &allowed(), 7).is_err());
        // Yap's own windows, even titled with the nonce.
        assert!(decide(&window(7, NONCE, &[(7, 1)]), &allowed(), 7).is_err());
        // No window.
        assert!(decide(&window(0, NONCE, &[]), &allowed(), 7).is_err());
    }

    #[test]
    fn without_a_nonce_or_processes_nothing_is_allowed() {
        let none = Allowed::default();
        assert!(decide(&window(100, NONCE, &[(100, 5_000)]), &none, 7).is_err());
        let short = Allowed {
            pids: vec![],
            nonce: Some("abc".into()),
        };
        assert!(decide(&window(100, "abc", &[]), &short, 7).is_err());
    }

    #[test]
    fn the_environment_is_read_strictly() {
        assert_eq!(parse_pids("123, 456;789 x 0"), vec![123, 456, 789]);
        assert_eq!(parse_pids(""), Vec::<u32>::new());
        assert_eq!(valid_nonce("  yaprobot-3f9a1c2b7d "), Some(NONCE.to_string()));
        assert_eq!(valid_nonce("short"), None);
        assert_eq!(valid_nonce("has a space in it"), None);
    }

    #[test]
    fn outside_robot_mode_a_normal_run_goes_ahead() {
        // `cargo test` isn't a test run of the app (no YAP_E2E): Go.
        if !crate::shell::e2e::active() {
            assert_eq!(gate(Some(1), "paste"), Gate::Go);
            assert!(permits(None));
            assert!(recheck_foreground(None, "paste").is_ok());
            assert_eq!(other_apps(), OtherApps::All);
            assert!(audio_permits(1234) && media_permits("Anything"));
        }
    }

    #[test]
    fn only_the_tests_own_sound_may_be_changed() {
        // The test's Edge: its sound plays from a child of the browser
        // process, whose only windows carry the nonce.
        let edge_audio = [(501, 8_000), (500, 7_000), (4, 1)];
        assert!(decide_audio(&edge_audio, &[500], &allowed(), 7).is_ok());
        // A process the test started, or one of its children.
        assert!(decide_audio(&[(100, 5_000)], &[], &allowed(), 7).is_ok());
        assert!(decide_audio(&[(200, 6_000), (100, 5_000)], &[], &allowed(), 7).is_ok());
        // The developer's music: none of that.
        assert!(decide_audio(&[(600, 9_000), (4, 1)], &[500], &allowed(), 7).is_err());
        // A reused id isn't the test's process.
        assert!(decide_audio(&[(100, 9_999)], &[], &allowed(), 7).is_err());
        // Yap's own chime, and a process that's gone.
        assert!(decide_audio(&[(7, 1)], &[7], &allowed(), 7).is_err());
        assert!(decide_audio(&[], &[500], &allowed(), 7).is_err());
        // Without a nonce, a window's process counts for nothing.
        let pids_only = Allowed { pids: vec![], nonce: None };
        assert!(decide_audio(&edge_audio, &[500], &pids_only, 7).is_err());
    }
}

//! **The call's own audio** (2026-10-29, Wispr Flow's Notetaker by
//! behaviour): while a meeting records a call, which microphone the call
//! app uses, whether you're muted in the call, and whether the call still
//! plays — read from Windows, on this PC, read only.
//!
//! - **Which microphone the call uses.** Windows keeps an audio session per
//!   app on every device (`IAudioSessionManager2`): the capture endpoint
//!   holding the call app's *active* session is the call's microphone (the
//!   app's own exes, a helper of theirs, or the browser a meeting tab is
//!   in). When it isn't the one "You" records from ([`mismatch`]): "Teams is
//!   using Contoso H2 but Yap is using Northwind Audio", once per call and
//!   pair of microphones ([`Said`]), with **Use Contoso H2** (this meeting
//!   only: `meeting::use_mic`; the person's order stays as it is) and Not
//!   now. A software microphone (Krisp, NVIDIA Broadcast: enumerator
//!   "ROOT") is fed by a real one, likely Yap's: never a mismatch. A call
//!   app on two microphones at once (a browser with two tabs) can't say
//!   which: nothing said.
//! - **Muted in the call.** The sessions can't tell: Teams, Zoom and Meet
//!   keep the microphone open while you're muted (to say "you're muted"
//!   when you talk), so the session stays active and carries sound. Two
//!   signals do: the call's microphone muted in Windows (a headset's mute
//!   button often is), and the call window's mute button, read through UI
//!   Automation ([`mute_from_buttons`]: "Unmute (Ctrl+Shift+M)" in Teams,
//!   "Unmute my audio" in Zoom, "Turn on microphone" in Meet; English
//!   only, a window that isn't minimised, and for Meet the meeting's tab in
//!   front). Read only while "You" has heard nothing for a while (the only
//!   time it matters), every few seconds; "Yap can't hear your mic" stays
//!   quiet while you're muted (`meeting.rs`, [`muted_in_call`]).
//! - **Is the call still on?** Call detection's ending wants a second
//!   opinion before it stops anything (`meeting_detect.rs`): the call app
//!   still playing sound (an active output session) or, for a call in a
//!   browser, a window still showing the meeting ([`still_on`]).
//!
//! Never with a state lock held: the reads run on this module's thread
//! (one per recording) or the call detector's. Logs say counts and
//! whether, never device or window names above debug. Test mode reads
//! nothing of Windows: `e2e_call_audio` says what it would.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Listener};

/// How often a recording's call is looked at…
const TICK: Duration = Duration::from_secs(5);
/// …and in a test run.
const TEST_TICK: Duration = Duration::from_millis(300);
/// "You" quiet this long before the call's mute state is read.
const MUTE_LOOK_AFTER_MS: u64 = 20_000;
/// The mismatch question goes away unanswered after this long.
const ASK_FADE_MS: u64 = 2 * 60_000;

/// A capture or render session of some app on one device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionUse {
    /// The device's endpoint id and name.
    pub id: String,
    pub name: String,
    /// A software device (enumerator "ROOT").
    pub is_virtual: bool,
    /// The device is muted in Windows.
    pub muted: bool,
    /// The session's process's exe file name, lowercased.
    pub exe: String,
    /// Its stream is running now.
    pub active: bool,
}

/// Is a session's `exe` one of the call's `exes` (lowercase file names)?
/// Its own, or a helper named after it ("ms-teams_modulehost.exe").
pub fn exe_matches(exe: &str, exes: &[String]) -> bool {
    exes.iter().any(|e| {
        let stem = e.strip_suffix(".exe").unwrap_or(e);
        exe == e || exe.strip_prefix(stem).is_some_and(|rest| rest.starts_with('_'))
    })
}

/// The microphone the call uses: the one device holding an active session
/// of its exes. A software device, or two at once: `None` (see the notes
/// at the top).
pub fn call_mic<'a>(sessions: &'a [SessionUse], exes: &[String]) -> Option<&'a SessionUse> {
    let mut found: Option<&SessionUse> = None;
    for s in sessions.iter().filter(|s| s.active && exe_matches(&s.exe, exes)) {
        match found {
            Some(f) if f.id != s.id => return None,
            _ => found = Some(s),
        }
    }
    found.filter(|s| !s.is_virtual)
}

/// Does the call's microphone differ from Yap's (`yap`: endpoint id, empty
/// when unknown; name)?
pub fn mismatch(call: &SessionUse, yap: &(String, String)) -> bool {
    if !yap.0.is_empty() && !call.id.is_empty() {
        return yap.0 != call.id;
    }
    !yap.1.is_empty() && yap.1 != call.name
}

/// Worth saying? A mismatch ([`mismatch`]), unless the call's microphone is
/// a closed laptop's own (`closed_built_in`): Yap passed it over on
/// purpose, it hears nothing useful with the lid shut (`dictation/lid.rs`).
pub fn worth_saying(call: &SessionUse, yap: &(String, String), closed_built_in: bool) -> bool {
    mismatch(call, yap) && !closed_built_in
}

/// The mismatches said so far: once per call and pair of microphones.
#[derive(Debug, Default)]
pub struct Said {
    pairs: Vec<(u64, String, String)>,
}

impl Said {
    /// First time for this call (`call`) and pair? Recorded if so.
    pub fn first(&mut self, call: u64, call_mic: &str, yap_mic: &str) -> bool {
        let pair = (call, call_mic.to_string(), yap_mic.to_string());
        if self.pairs.contains(&pair) {
            return false;
        }
        if self.pairs.len() >= 64 {
            self.pairs.remove(0);
        }
        self.pairs.push(pair);
        true
    }
}

/// What a call window's buttons say about your microphone: `Some(true)`
/// muted (a button offers to unmute), `Some(false)` not (one offers to
/// mute), `None` can't tell (neither, or both).
pub fn mute_from_buttons(names: &[String]) -> Option<bool> {
    const UNMUTE: [&str; 7] =
        ["unmute (", "unmute,", "unmute my audio", "unmute mic", "unmute microphone", "turn on microphone", "turn on mic"];
    const MUTE: [&str; 7] = ["mute (", "mute,", "mute my audio", "mute mic", "mute microphone", "turn off microphone", "turn off mic"];
    let (mut muted, mut live) = (false, false);
    for name in names {
        let n = name.trim().to_lowercase();
        if n == "unmute" || UNMUTE.iter().any(|p| n.starts_with(p)) {
            muted = true;
        } else if n == "mute" || MUTE.iter().any(|p| n.starts_with(p)) {
            live = true;
        }
    }
    match (muted, live) {
        (true, false) => Some(true),
        (false, true) => Some(false),
        _ => None,
    }
}

/// "Contoso H2" for "Headset Microphone (Contoso H2)".
fn short(name: &str) -> String {
    crate::dictation::mics::short_name(name)
}

/// The question: "Teams is using Contoso H2 but Yap is using Northwind
/// Audio".
pub fn mismatch_question(note_id: u64, call: &str, call_mic: &SessionUse, yap_mic: &str) -> crate::calls::meeting_guard::Question {
    use crate::calls::meeting_guard::{About, Answer, Question};
    let (theirs, ours) = (short(&call_mic.name), short(yap_mic));
    Question {
        note_id,
        kind: "mic",
        icon: "mic",
        title: format!("{call} is using {theirs} but Yap is using {ours}"),
        body: format!("Use {theirs} for this meeting, so Yap hears you as the call does."),
        primary: Answer::new("use", format!("Use {theirs}")),
        secondary: Answer::new("keep", "Not now"),
        about: About::Mic { id: call_mic.id.clone(), name: call_mic.name.clone() },
        fade_ms: ASK_FADE_MS,
    }
}

// ---- the recording's watch ---------------------------------------------------------------

/// Bumped per recording: an older watch sees it and leaves.
static GEN: AtomicU64 = AtomicU64::new(0);
/// You're muted in the call being recorded (as last read).
static MUTED: AtomicBool = AtomicBool::new(false);
/// The mismatches said since Yap started.
static SAID: Mutex<Said> = Mutex::new(Said { pairs: Vec::new() });

/// You're muted in the call being recorded, as last read (while "You" was
/// quiet). False when it can't be told.
pub fn muted_in_call() -> bool {
    MUTED.load(Ordering::SeqCst)
}

/// Follow the meeting recorder (app setup): a watch per recording.
pub fn init(app: &AppHandle) {
    let handle = app.clone();
    app.listen("yap-meeting-state", move |event| {
        let state: serde_json::Value = serde_json::from_str(event.payload()).unwrap_or_default();
        let gen = GEN.fetch_add(1, Ordering::SeqCst) + 1;
        MUTED.store(false, Ordering::SeqCst);
        let Some(note_id) = state["noteId"].as_u64().filter(|_| state["recording"].as_bool() == Some(true)) else {
            return;
        };
        let app = handle.clone();
        let spawned = std::thread::Builder::new().name("call-audio".into()).spawn(move || watch(app, note_id, gen));
        if let Err(e) = spawned {
            tracing::warn!("call audio: couldn't watch the call ({e})");
        }
    });
}

/// Look at the recording's call every few seconds until it stops.
fn watch(app: AppHandle, note_id: u64, gen: u64) {
    loop {
        std::thread::sleep(if crate::shell::e2e::active() { TEST_TICK } else { TICK });
        if GEN.load(Ordering::SeqCst) != gen || crate::meetings::meeting::recording_note() != Some(note_id) {
            return;
        }
        let Some(call) = crate::calls::meeting_detect::call_for(note_id) else {
            MUTED.store(false, Ordering::SeqCst);
            continue;
        };
        let sessions = capture_sessions(&call.exes);
        let mic = sessions.as_deref().and_then(|s| call_mic(s, &call.exes)).cloned();
        // A different microphone: said once per call and pair.
        if let (Some(theirs), Some(ours)) = (mic.as_ref(), yap_mic()) {
            let closed_built_in = crate::dictation::lid::closed()
                && crate::dictation::mics::present().is_some_and(|l| l.iter().any(|e| e.id == theirs.id && e.built_in));
            let different = worth_saying(theirs, &ours, closed_built_in);
            let ours_key = if ours.0.is_empty() { ours.1.clone() } else { ours.0.clone() };
            if different && SAID.lock().unwrap_or_else(|p| p.into_inner()).first(call.id, &theirs.id, &ours_key) {
                tracing::info!(note_id, app = call.app, "call audio: the call uses another microphone; asking");
                crate::calls::meeting_guard::ask(&app, mismatch_question(note_id, call.label, theirs, &ours.1));
            }
        }
        // Muted? Only worth reading while "You" is quiet.
        let quiet = crate::meetings::meeting::you_quiet_ms().is_some_and(|ms| ms >= mute_look_after());
        let muted = quiet && (mic.as_ref().is_some_and(|m| m.muted) || muted_by_buttons(call.app));
        if muted != MUTED.swap(muted, Ordering::SeqCst) {
            tracing::info!(note_id, app = call.app, muted, "call audio: muted in the call");
        }
    }
}

fn mute_look_after() -> u64 {
    if crate::shell::e2e::active() {
        0
    } else {
        MUTE_LOOK_AFTER_MS
    }
}

/// The microphone "You" records from: (endpoint id, name), the id looked
/// up by name when it was opened as Windows' default.
fn yap_mic() -> Option<(String, String)> {
    let (id, name) = crate::meetings::meeting::mic_in_use()?;
    if !id.is_empty() {
        return Some((id, name));
    }
    let found = crate::dictation::mics::present()
        .and_then(|l| l.into_iter().find(|e| e.name == name).map(|e| e.id))
        .unwrap_or_default();
    Some((found, name))
}

/// The call window's mute button says muted.
fn muted_by_buttons(app_id: &str) -> bool {
    if let Some(t) = test() {
        return t.muted == Some(true);
    }
    #[cfg(windows)]
    {
        if crate::calls::meeting_detect::call_window_exes().is_none_or(|(id, _)| id != app_id) {
            return false;
        }
        let Some(hwnd) = crate::meetings::notepad::call_window_quiet() else { return false };
        match crate::speakers::call_ui::win::button_names(hwnd) {
            Ok(Some(names)) => mute_from_buttons(&names) == Some(true),
            Ok(None) => false,
            Err(e) => {
                tracing::debug!("call audio: the call window can't be read now ({e})");
                false
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = app_id;
        false
    }
}

/// Every capture session on every active microphone (`None`: Windows
/// couldn't be asked). `exes` only for test runs' stand-in.
fn capture_sessions(exes: &[String]) -> Option<Vec<SessionUse>> {
    if let Some(t) = test() {
        return Some(
            t.mic
                .into_iter()
                .map(|m| SessionUse {
                    id: m.id,
                    name: m.name,
                    is_virtual: m.is_virtual,
                    muted: false,
                    exe: exes.first().cloned().unwrap_or_default(),
                    active: true,
                })
                .collect(),
        );
    }
    if crate::shell::e2e::active() {
        return None;
    }
    #[cfg(windows)]
    {
        win::sessions(false)
    }
    #[cfg(not(windows))]
    {
        let _ = exes;
        None
    }
}

/// Is the call of `app_id` (its desktop `exes`) still on, by a second
/// signal: the app still plays sound, or a window still shows the meeting
/// (a browser tab; Zoom's meeting window). `None`: can't tell.
pub fn still_on(app_id: &str, exes: &[String]) -> Option<bool> {
    if let Some(t) = test() {
        return t.still_on;
    }
    if crate::shell::e2e::active() {
        return None;
    }
    #[cfg(windows)]
    {
        let playing = (!exes.is_empty())
            .then(|| win::sessions(true))
            .flatten()
            .map(|all| all.iter().any(|s| s.active && exe_matches(&s.exe, exes)));
        let mut look: Vec<String> = exes.to_vec();
        look.extend(crate::calls::meeting_detect::browser_exes().iter().map(|b| b.to_string()));
        let shown = crate::meetings::notepad::windows_of(&look)
            .iter()
            .any(|w| !w.tool && crate::calls::meeting_detect::window_shows_call(app_id, &w.exe, &w.title));
        match (playing, shown) {
            (Some(true), _) | (_, true) => Some(true),
            (Some(false), false) => Some(false),
            // A desktop app whose sound can't be read: can't tell.
            (None, false) => exes.is_empty().then_some(false),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (app_id, exes);
        None
    }
}

// ---- test mode -----------------------------------------------------------------------------

/// What a test run says Windows would.
#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TestCall {
    /// The microphone the call app uses (`{id, name, isVirtual}`; a made-up
    /// one's id as `e2e_mics` gave it).
    pub mic: Option<TestMic>,
    /// The call's mute button says muted.
    pub muted: Option<bool>,
    /// The call is still on by a second signal (`still_on`).
    pub still_on: Option<bool>,
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct TestMic {
    pub id: String,
    pub name: String,
    pub is_virtual: bool,
}

static TEST: Mutex<Option<TestCall>> = Mutex::new(None);

fn test() -> Option<TestCall> {
    if !crate::shell::e2e::active() {
        return None;
    }
    TEST.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

/// Test mode only: what Windows would say about the call (`null`: nothing,
/// as in a test run without it, and the mismatches said are forgotten).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_call_audio(call: Option<TestCall>) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    tracing::info!(?call, "e2e: the call's audio");
    if call.is_none() {
        *SAID.lock().unwrap_or_else(|p| p.into_inner()) = Said::default();
    }
    *TEST.lock().unwrap_or_else(|p| p.into_inner()) = call;
    Ok(())
}

// ---- Windows ------------------------------------------------------------------------------

#[cfg(windows)]
mod win {
    use windows::core::Interface;
    use windows::Win32::Foundation::S_OK;
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{
        eCapture, eRender, AudioSessionStateActive, IAudioSessionControl2, IAudioSessionManager2, DEVICE_STATE_ACTIVE,
    };
    use windows::Win32::System::Com::CLSCTX_ALL;

    use super::SessionUse;
    use crate::dictation::mics::win::{com, enumerator, facts};

    /// Every app's session on every active device: microphones, or
    /// (`render`) speakers. System sounds and Yap's own left out.
    pub fn sessions(render: bool) -> Option<Vec<SessionUse>> {
        let _com = com();
        let enumerator = enumerator()?;
        let exes = crate::shell::procs::exe_names();
        let own = std::process::id();
        let mut out = Vec::new();
        // SAFETY: plain COM calls on this thread; every pointer comes from
        // COM and is released by its wrapper.
        unsafe {
            let flow = if render { eRender } else { eCapture };
            let devices = enumerator.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE).ok()?;
            for d in 0..devices.GetCount().unwrap_or(0) {
                let Ok(device) = devices.Item(d) else { continue };
                let Some(f) = facts(&device) else { continue };
                let muted = device
                    .Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
                    .and_then(|v| v.GetMute())
                    .is_ok_and(|m| m.as_bool());
                let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else { continue };
                let Ok(list) = manager.GetSessionEnumerator() else { continue };
                for i in 0..list.GetCount().unwrap_or(0) {
                    let Ok(control) = list.GetSession(i) else { continue };
                    let Ok(control2) = control.cast::<IAudioSessionControl2>() else { continue };
                    if control2.IsSystemSoundsSession() == S_OK {
                        continue;
                    }
                    let pid = control2.GetProcessId().unwrap_or(0);
                    if pid == 0 || pid == own {
                        continue;
                    }
                    out.push(SessionUse {
                        id: f.id.clone(),
                        name: f.name.clone(),
                        is_virtual: f.enumerator.trim().eq_ignore_ascii_case("ROOT"),
                        muted,
                        exe: exes.get(&pid).cloned().unwrap_or_default(),
                        active: control.GetState().is_ok_and(|s| s == AudioSessionStateActive),
                    });
                }
            }
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn use_of(id: &str, exe: &str, active: bool) -> SessionUse {
        SessionUse {
            id: id.to_string(),
            name: format!("Microphone ({id})"),
            is_virtual: false,
            muted: false,
            exe: exe.to_string(),
            active,
        }
    }

    fn exes(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn the_call_app_and_its_helpers_are_matched_by_exe() {
        let teams = exes(&["ms-teams.exe", "teams.exe"]);
        assert!(exe_matches("ms-teams.exe", &teams));
        assert!(exe_matches("ms-teams_modulehost.exe", &teams));
        assert!(!exe_matches("ms-teamsx.exe", &teams));
        assert!(!exe_matches("msedgewebview2.exe", &teams));
        assert!(!exe_matches("yap.exe", &teams));
    }

    #[test]
    fn the_calls_microphone_is_the_one_its_active_session_is_on() {
        let zoom = exes(&["zoom.exe"]);
        let all = vec![
            use_of("headset", "zoom.exe", true),
            use_of("laptop", "obs64.exe", true),
            use_of("usb", "zoom.exe", false), // opened once, not running
        ];
        assert_eq!(call_mic(&all, &zoom).map(|s| s.id.as_str()), Some("headset"));
        // On two at once: can't say which.
        let two = vec![use_of("headset", "zoom.exe", true), use_of("usb", "zoom.exe", true)];
        assert_eq!(call_mic(&two, &zoom), None);
        // Two sessions on the same device are one microphone.
        let same = vec![use_of("headset", "zoom.exe", true), use_of("headset", "zoom.exe", true)];
        assert_eq!(call_mic(&same, &zoom).map(|s| s.id.as_str()), Some("headset"));
        // A software microphone (fed by a real one): never a mismatch.
        let krisp = vec![SessionUse { is_virtual: true, ..use_of("krisp", "zoom.exe", true) }];
        assert_eq!(call_mic(&krisp, &zoom), None);
        // Not on any: nothing.
        assert_eq!(call_mic(&all[1..], &zoom), None);
    }

    #[test]
    fn a_mismatch_by_id_else_by_name() {
        let call = use_of("headset", "ms-teams.exe", true);
        assert!(!mismatch(&call, &("headset".into(), "anything".into())));
        assert!(mismatch(&call, &("laptop".into(), "Microphone (headset)".into())));
        // Yap opened Windows' default without a list: by name.
        assert!(!mismatch(&call, &(String::new(), "Microphone (headset)".into())));
        assert!(mismatch(&call, &(String::new(), "Microphone (laptop)".into())));
        // Nothing to compare: nothing said.
        assert!(!mismatch(&call, &(String::new(), String::new())));
        // The call on a closed laptop's own mic, which Yap passed over on
        // purpose: not worth saying.
        let ours = ("headset".to_string(), "Headset Microphone (Contoso H2)".to_string());
        let laptop = use_of("laptop", "ms-teams.exe", true);
        assert!(worth_saying(&laptop, &ours, false));
        assert!(!worth_saying(&laptop, &ours, true));
    }

    #[test]
    fn a_mismatch_is_said_once_per_call_and_pair() {
        let mut said = Said::default();
        assert!(said.first(1, "headset", "laptop"));
        assert!(!said.first(1, "headset", "laptop"));
        // Another pair in the same call (the person moved), or another call.
        assert!(said.first(1, "usb", "laptop"));
        assert!(said.first(2, "headset", "laptop"));
        assert!(!said.first(2, "headset", "laptop"));
    }

    #[test]
    fn the_mute_button_says_whether_you_are_muted() {
        let names = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        // Teams: the button offers the other state.
        assert_eq!(mute_from_buttons(&names(&["Camera", "Unmute (Ctrl+Shift+M)", "Leave"])), Some(true));
        assert_eq!(mute_from_buttons(&names(&["Mute (Ctrl+Shift+M)", "Share"])), Some(false));
        // Zoom.
        assert_eq!(mute_from_buttons(&names(&["Unmute my audio, Alt+A"])), Some(true));
        assert_eq!(mute_from_buttons(&names(&["Mute my audio"])), Some(false));
        // Meet in a browser.
        assert_eq!(mute_from_buttons(&names(&["Turn on microphone (ctrl + d)"])), Some(true));
        assert_eq!(mute_from_buttons(&names(&["Turn off microphone (ctrl + d)"])), Some(false));
        // Other people's controls aren't yours; neither or both: can't tell.
        assert_eq!(mute_from_buttons(&names(&["Mute all", "Mute notifications", "Unmute all"])), None);
        assert_eq!(mute_from_buttons(&names(&["Unmute (Ctrl+Shift+M)", "Mute (Ctrl+Shift+M)"])), None);
        assert_eq!(mute_from_buttons(&[]), None);
    }

    #[test]
    fn the_question_names_both_microphones() {
        let theirs = SessionUse { name: "Headset Microphone (Contoso H2)".into(), ..use_of("e2e-h2", "ms-teams.exe", true) };
        let q = mismatch_question(4, "Teams", &theirs, "Microphone Array (Northwind Audio)");
        assert_eq!(q.title, "Teams is using Contoso H2 but Yap is using Northwind Audio");
        assert_eq!((q.primary.label(), q.secondary.label()), ("Use Contoso H2", "Not now"));
        assert_eq!((q.primary.id(), q.secondary.id()), ("use", "keep"));
        assert_eq!(q.about, crate::calls::meeting_guard::About::Mic { id: "e2e-h2".into(), name: theirs.name.clone() });
    }
}

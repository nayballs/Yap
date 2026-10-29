//! Test mode for the end-to-end UI suite (`e2e/`, see docs/e2e-tests.md).
//!
//! The suite runs a **debug** build with `YAP_E2E=1` right next to whatever
//! Yap the developer has open, and drives its webviews over CDP. Test mode
//! keeps that instance inside its own windows and portable data dir:
//!
//! - no global keyboard/mouse hook: it never sees, or swallows, real keys;
//! - nothing is pasted, typed or copied into other apps (a dictation's text
//!   still reaches history and the Home feed; "Paste last transcript" only
//!   returns what it would paste), and the text around another app's cursor
//!   is never read (`selection::begin_dictation`), nor its window's title
//!   (`app_kind::of_window`: every target counts as `Other`), nor the field
//!   watched for corrections after a paste (`learn::after_paste`; the
//!   debug-only `learn::e2e_learn_correction` hands it an edit instead);
//! - its windows never take the foreground (tao's `set_focus` fallback presses
//!   Alt in whichever app has focus) and it posts no Windows notifications;
//! - it leaves other processes alone (no "orphaned sidecar" sweep) and never
//!   reads or writes the installed app's saved window size and position;
//! - it quits cleanly (tray icon, exit hooks) when the harness closes its
//!   stdin, which also happens when the harness dies;
//! - a meeting recording opens no audio device: the "You" and "Them" audio
//!   comes from `you.wav` / `them.wav` in `YAP_E2E_MEETING_AUDIO` (played
//!   `YAP_E2E_MEETING_SPEED` times faster than real time), or is silent, and
//!   [`e2e_meeting_feed`] can hand the recorder transcript segments directly
//!   (a two-hour meeting in seconds). [`e2e_meeting_dictation`] and
//!   [`e2e_meeting_output_change`] stage a hotkey dictation and a switch of
//!   Windows' default output at a point of that audio, and
//!   [`e2e_meeting_quiet`] shortens the quiet-side warning's timings; the
//!   player follows made-up microphones (`mics::e2e_mics`,
//!   `mics::e2e_mic_unplug`) as the real capture follows Windows' list;
//!   [`e2e_meeting_fail`] makes the live recorder's chunks over a stretch
//!   fail, and `YAP_E2E_REFINE_CHUNK_MS` slows the refine's chunks
//!   (`meeting_refine::e2e_slow_chunk`);
//! - it reads no installed apps' icons (Settings' call apps, the bar's call
//!   card): [`app_icons_dir`] (`YAP_E2E_APP_ICONS`) names stand-ins instead.
//!
//! The real-app robot tests (`e2e-real/`) run test mode with
//! `YAP_REALTEST=1` on top: **robot mode** (`realtest.rs`), where the paste,
//! the submit key and the reads of the target field are real again — but
//! only into the windows the test opened itself.
//!
//! Release builds compile all of this out: [`active`] is always `false`
//! without `debug_assertions`, and the meeting hooks don't exist.

use tauri::AppHandle;

/// `true` in a debug build started with `YAP_E2E=1`.
pub fn active() -> bool {
    #[cfg(debug_assertions)]
    {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ON.get_or_init(|| std::env::var("YAP_E2E").is_ok_and(|v| v.trim() == "1"))
    }
    #[cfg(not(debug_assertions))]
    false
}

/// The Yap bar follows the REAL cursor in this test run
/// (`YAP_E2E_REAL_CURSOR=1`): for checking that a real mouse click reaches
/// it (the bar's hit-testing, `bar.rs`). Without it a test run's bar only
/// sees the cursor `bar_simulate` pretends. Always `false` in release builds.
pub fn real_cursor() -> bool {
    #[cfg(debug_assertions)]
    {
        active() && std::env::var("YAP_E2E_REAL_CURSOR").is_ok_and(|v| v.trim() == "1")
    }
    #[cfg(not(debug_assertions))]
    false
}

/// Stand-ins for installed call apps' icons (`YAP_E2E_APP_ICONS`: a folder
/// of `<app id>.png`), since test mode reads no installed apps
/// (`app_icons.rs`). `None` without it, and always in release builds.
pub fn app_icons_dir() -> Option<std::path::PathBuf> {
    #[cfg(debug_assertions)]
    {
        std::env::var_os("YAP_E2E_APP_ICONS").filter(|v| !v.is_empty()).map(std::path::PathBuf::from)
    }
    #[cfg(not(debug_assertions))]
    None
}

/// How much faster than real time the meeting test audio plays
/// (`YAP_E2E_MEETING_SPEED`, at least 1). The recorder's drain cadence
/// speeds up to match, so chunks stay ~15 s of audio.
#[cfg(debug_assertions)]
pub fn meeting_speed() -> f32 {
    std::env::var("YAP_E2E_MEETING_SPEED")
        .ok()
        .and_then(|v| v.trim().parse::<f32>().ok())
        .filter(|s| s.is_finite())
        .unwrap_or(1.0)
        .clamp(1.0, 50.0)
}

/// What a test staged for the meeting player, in seconds of its test audio
/// (so it lands at the same point however fast the audio plays).
#[cfg(debug_assertions)]
#[derive(Default)]
struct Script {
    /// A hotkey dictation over `[from, to)`.
    dictation: Option<(f32, f32)>,
    /// Windows' default output changes at this point.
    output_change: Option<f32>,
}

#[cfg(debug_assertions)]
static SCRIPT: std::sync::Mutex<Script> = std::sync::Mutex::new(Script {
    dictation: None,
    output_change: None,
});

#[cfg(debug_assertions)]
fn script() -> std::sync::MutexGuard<'static, Script> {
    SCRIPT.lock().unwrap_or_else(|p| p.into_inner())
}

/// How long "Them" hears nothing in a simulated output switch: the old
/// device has gone quiet, the new one isn't open yet (1 s of test audio).
#[cfg(debug_assertions)]
const SWITCH_GAP_SAMPLES: usize = 16_000;

/// The meeting player's "mic" keeps delivering digital silence once its
/// test audio has played (or with none), as a real mic does, so the
/// meeting's audio clock runs on: what the quiet-meeting guard counts
/// quiet time on (`meeting_guard::e2e_meeting_checkins` turns it on, and
/// off again). Off, the player goes quiet after its files, as before.
#[cfg(debug_assertions)]
static LIVE_MIC: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[cfg(debug_assertions)]
pub fn set_live_mic(on: bool) {
    LIVE_MIC.store(on, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(debug_assertions)]
fn live_mic() -> bool {
    LIVE_MIC.load(std::sync::atomic::Ordering::SeqCst)
}

/// The meeting recorder's capture in test mode: plays `you.wav` into the
/// mic buffer and `them.wav` into the loopback buffer, in step, from
/// `YAP_E2E_MEETING_AUDIO` (any format `media.rs` decodes), then goes quiet
/// until stop (or, with [`set_live_mic`], the mic carries on with digital
/// silence). Without the variable it's silent: segments come from
/// [`e2e_meeting_feed`] instead. Either way it never records the machine's
/// real microphone or speakers, and works the same on a CI runner without
/// any. Through the same push path as the real capture (`meeting::
/// push_audio`), and it keeps the two sides on one clock the same way
/// (`meeting::keep_in_step`). What a test staged plays out as it goes: a
/// dictation sends the pipeline's dictation signal over its stretch, as
/// `start_recording` / the stop do; an output switch leaves "Them" without
/// audio for a second, then follows the "new device" as the capture thread
/// would.
#[cfg(debug_assertions)]
pub fn spawn_meeting_audio(
    mic: crate::meetings::meeting::AudioBuf,
    sys: crate::meetings::meeting::AudioBuf,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    overflow: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    use crate::dictation::mics;
    use crate::meetings::meeting::{Follow, OutputFollower};
    use std::sync::atomic::Ordering;
    // Like the real capture, with made-up microphones (`mics::e2e_mics`)
    // and none of them to open, it fails (none is opened either way).
    let order = crate::config::load().mic_order;
    if let Some(fake) = mics::fake::list() {
        if mics::candidates(&order, Some(&fake), &[]).is_empty() {
            return Err(crate::meetings::meeting::no_mic_error(&order));
        }
    }
    let dir = std::env::var("YAP_E2E_MEETING_AUDIO")
        .ok()
        .filter(|d| !d.trim().is_empty())
        .map(std::path::PathBuf::from);
    let load = |name: &str| -> Result<Vec<f32>, String> {
        match &dir {
            Some(d) => crate::dictation::media::decode_to_16k_mono(&d.join(name))
                .map_err(|e| format!("e2e meeting audio {name}: {e}")),
            None => Ok(Vec::new()),
        }
    };
    let (you, them) = (load("you.wav")?, load("them.wav")?);
    let speed = meeting_speed();
    tracing::info!(
        you_secs = you.len() / 16_000,
        them_secs = them.len() / 16_000,
        speed,
        "e2e: meeting audio from test files, no devices"
    );
    std::thread::Builder::new()
        .name("meeting-capture-e2e".into())
        .spawn(move || {
            // 100 ms of audio per 100 ms tick, times the speed-up.
            let step = (1_600.0 * speed) as usize;
            let mut pos = 0;
            let mut dictating = false;
            let mut output = OutputFollower::listening_to(Some("e2e-speakers".to_string()));
            // "Them" hears nothing until here (a device switch under way).
            let mut switching: Option<usize> = None;
            // The microphone "You" is on (the order and Windows' list, as
            // the real capture picks it), and — its device unplugged
            // (`e2e_mic_unplug`) — nothing until here, then the next one.
            let mic_of = || mics::present().and_then(|l| mics::in_use(&order, &l, &[]));
            let mut mic_at = mic_of();
            let say_mic = |at: &Option<mics::Endpoint>| match at {
                Some(e) => crate::meetings::meeting::set_mic_in_use(&e.id, &e.name),
                None => crate::meetings::meeting::set_mic_in_use("", "Microphone (e2e)"),
            };
            say_mic(&mic_at);
            let mut mics_seen = mics::generation();
            let mut mic_gap: Option<usize> = None;
            let length = you.len().max(them.len());
            let mut played = false;
            while !stop.load(Ordering::SeqCst) {
                let secs = pos as f32 / 16_000.0;
                {
                    let mut s = script();
                    if let Some((from, to)) = s.dictation {
                        if !dictating && secs >= from && secs < to {
                            tracing::info!(secs, "e2e: a dictation starts");
                            crate::dictation::pipeline::dictation_began();
                            dictating = true;
                        } else if secs >= to {
                            if dictating {
                                tracing::info!(secs, "e2e: the dictation ends");
                                crate::dictation::pipeline::dictation_ended();
                                dictating = false;
                            }
                            s.dictation = None;
                        }
                    }
                    if s.output_change.is_some_and(|at| secs >= at) {
                        s.output_change = None;
                        tracing::info!(secs, "e2e: Windows' default output changes");
                        switching = Some(pos + SWITCH_GAP_SAMPLES);
                    }
                }
                if mics::generation() != mics_seen {
                    mics_seen = mics::generation();
                    let gone = mic_at.as_ref().is_some_and(|at| !mics::present().is_some_and(|l| l.iter().any(|e| e.id == at.id)));
                    if gone && mic_gap.is_none() {
                        tracing::info!(secs, "e2e: the meeting's microphone is unplugged");
                        mic_gap = Some(pos + SWITCH_GAP_SAMPLES);
                    }
                }
                // "Use Contoso H2" (`meeting::use_mic`): "You" moves to it.
                if let Some((id, _)) = crate::meetings::meeting::take_mic_asked() {
                    if let Some(e) = mics::present().and_then(|l| l.into_iter().find(|e| e.id == id)) {
                        tracing::info!(secs, "e2e: \"You\" moves to the microphone asked for");
                        mic_at = Some(e);
                        say_mic(&mic_at);
                    }
                }
                if mic_gap.is_some_and(|until| pos >= until) {
                    mic_gap = None;
                    mic_at = mic_of();
                    say_mic(&mic_at);
                    let next = mic_at.as_ref().map(|e| e.name.clone()).unwrap_or_default();
                    crate::meetings::meeting::keep_in_step(&mic, &sys);
                    if let Ok(mut b) = mic.lock() {
                        b.mic_changed(&next);
                    }
                    tracing::info!(secs, "e2e: \"You\" goes on with the next microphone");
                    if let Some(app) = mics::app() {
                        crate::meetings::meeting::tell_mic_changed(app, &next);
                    }
                }
                if switching.is_some_and(|until| pos >= until) {
                    switching = None;
                    if let Follow::Switch(id) = output.poll(Some("e2e-headset".to_string())) {
                        output.opened(id);
                        crate::meetings::meeting::followed_output("e2e headset");
                    }
                }
                for (src, buf, side) in [(&you, &mic, "you"), (&them, &sys, "them")] {
                    if (side == "them" && switching.is_some()) || (side == "you" && mic_gap.is_some()) {
                        continue;
                    }
                    if pos < src.len() {
                        let end = (pos + step).min(src.len());
                        crate::meetings::meeting::push_audio(buf, &src[pos..end], &overflow);
                    } else if side == "you" && live_mic() {
                        // A real mic never stops delivering: digital silence.
                        crate::meetings::meeting::push_audio(buf, &vec![0.0; step], &overflow);
                    }
                }
                crate::meetings::meeting::keep_in_step(&mic, &sys);
                pos += step;
                if !played && length > 0 && pos >= length {
                    played = true;
                    // (The suite waits for this line before it stops.)
                    tracing::info!("e2e: the test audio has played to the end");
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            // Never leave a dictation "recording" behind for the next test.
            if dictating {
                crate::dictation::pipeline::dictation_ended();
            }
            crate::meetings::meeting::clear_mic_in_use();
            *script() = Script::default();
        })
        .map_err(|e| format!("Failed to spawn the e2e capture thread: {e}"))?;
    Ok(())
}

/// Test mode only: a hotkey dictation over `[from, to)` seconds of the next
/// recording's test audio (or the current one's, if it isn't there yet). The
/// meeting player sends the pipeline's dictation signal over that stretch,
/// as `start_recording` and the stop do, so the recorder blanks "You" and
/// leaves a "you dictated here" marker. No microphone is involved.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_meeting_dictation(from: f32, to: f32) -> Result<(), String> {
    if !active() {
        return Err("Only in e2e test mode".to_string());
    }
    if !(from.is_finite() && to.is_finite() && from < to) {
        return Err("A dictation needs from < to".to_string());
    }
    tracing::info!(from, to, "e2e: a dictation is staged");
    script().dictation = Some((from, to));
    Ok(())
}

/// Test mode only: Windows' default output changes `at` seconds into the
/// test audio (now, without it). "Them" hears nothing for a second, then the
/// recorder follows the "new device", as the capture thread does with a real
/// one, and the gap is silence.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_meeting_output_change(at: Option<f32>) -> Result<(), String> {
    if !active() {
        return Err("Only in e2e test mode".to_string());
    }
    let at = at.unwrap_or(0.0).max(0.0);
    tracing::info!(at, "e2e: an output switch is staged");
    script().output_change = Some(at);
    Ok(())
}

/// Test mode only: the quiet-side warning after `quietSecs` (instead of 3
/// minutes) of one side hearing nothing while the other talked `talkSecs`
/// (instead of 10 s), in seconds of meeting audio. No arguments: back to the
/// real timings.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_meeting_quiet(quiet_secs: Option<f32>, talk_secs: Option<f32>) -> Result<(), String> {
    if !active() {
        return Err("Only in e2e test mode".to_string());
    }
    let timings = quiet_secs.map(|q| (q, talk_secs.unwrap_or(q / 18.0)));
    tracing::info!(?timings, "e2e: quiet-side timings");
    crate::meetings::meeting::set_test_quiet_timings(timings);
    Ok(())
}

/// Test mode only: the live recorder's chunks of `side` ("you", "them" or
/// "both") overlapping `[from, to)` seconds of the recording fail, as if
/// the speech engine had (`meeting.rs`, "Failed chunks"); the meeting's
/// audio transcribed again later (Try again, the refine) is unaffected. No
/// `side`: none fail.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_meeting_fail(side: Option<String>, from: Option<f32>, to: Option<f32>) -> Result<(), String> {
    if !active() {
        return Err("Only in e2e test mode".to_string());
    }
    let fails = side.map(|s| (s, from.unwrap_or(0.0), to.unwrap_or(f32::MAX)));
    tracing::info!(?fails, "e2e: meeting chunks fail");
    crate::meetings::meeting::set_test_failures(fails);
    Ok(())
}

/// Test mode only: two meeting starts at the same moment on two threads,
/// as the meeting shortcut and the call prompt (or the tray and the
/// calendar) can: what each returned. One recorder must come of it
/// (`meeting::start` claims the recorder's slot before opening devices).
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn e2e_meeting_start_twice(app: AppHandle, first: u64, second: u64) -> Result<Vec<Result<(), String>>, String> {
    use tauri::Manager;
    if !active() {
        return Err("Only in e2e test mode".to_string());
    }
    let together = std::sync::Arc::new(std::sync::Barrier::new(2));
    let starts: Vec<_> = [first, second]
        .into_iter()
        .map(|note_id| {
            let (app, together) = (app.clone(), std::sync::Arc::clone(&together));
            tauri::async_runtime::spawn_blocking(move || {
                together.wait();
                crate::meetings::commands::meeting_start(app.clone(), app.state::<crate::AppState>(), note_id)
            })
        })
        .collect();
    let mut out = Vec::new();
    for start in starts {
        out.push(start.await.unwrap_or_else(|e| Err(format!("The start crashed: {e}"))));
    }
    tracing::info!(ok = out.iter().filter(|r| r.is_ok()).count(), "e2e: two meeting starts at once");
    Ok(out)
}

/// Test mode only: the main window back on screen (shown, not minimized) if
/// the desktop minimized or hid it behind the suite's back, and nothing else.
/// Unlike `open_settings`, no focus and no "window shown" hooks (an update
/// re-check, a pending announcement moving in-app), so a spec can call it
/// before every test without changing what it tests.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_main_on_screen(app: AppHandle) -> Result<(), String> {
    use tauri::Manager;
    if !active() {
        return Err("Only in e2e test mode".to_string());
    }
    let w = app.get_webview_window("settings").ok_or("no main window")?;
    if w.is_minimized().unwrap_or(false) {
        tracing::info!("e2e: the main window was minimized; restoring it");
        w.unminimize().map_err(|e| e.to_string())?;
    }
    if !w.is_visible().unwrap_or(false) {
        tracing::info!("e2e: the main window was hidden; showing it");
        w.show().map_err(|e| e.to_string())?;
        crate::shell::window::behind_in_test_runs(&w);
    }
    Ok(())
}

/// Test mode only: the dictation mic's audio arrives as digital silence
/// (`silent: true`), as a muted mic's does, until `silent: false` — the
/// dead-mic watchdog's case (`mic_watch.rs`) on the machine's real
/// microphone, whose stream it really reopens. Dictation only: a meeting's
/// test audio never comes from a mic.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_dictation_mic(silent: bool) -> Result<(), String> {
    if !active() {
        return Err("Only in e2e test mode".to_string());
    }
    tracing::info!(silent, "e2e: dictation mic");
    crate::dictation::pipeline::set_test_silent_mic(silent);
    Ok(())
}

/// Test mode only (the e2e suite and the robot): the dictation mic's state
/// (`mic.rs`) — whether a stream is open, live, Bluetooth, how many were
/// opened and closed, how the last open went (ms from asking to its first
/// audio), and the warm window.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_mic_status(state: tauri::State<'_, crate::AppState>) -> Result<crate::dictation::mic::Status, String> {
    if !active() {
        return Err("Only in e2e test mode".to_string());
    }
    let guard = state.pipeline.lock().map_err(|_| "pipeline unavailable")?;
    Ok(guard.as_ref().ok_or("pipeline not started")?.mic_status())
}

/// Test mode only: a dictation of `seconds` of a quiet tone played in the
/// microphone's place (the robot's `dictate_audio`: a real start and stop,
/// the mic held and opened as for any dictation, the STT, the dictation's
/// steps; nothing pasted in a test run) — a dictation without a microphone.
/// `amplitude`: the tone's peak (0.1 unless given; 0.005 is a whisper's,
/// -46 dBFS — the bar's level meter, `meter.rs`), and then it comes in
/// syllables as speech does: 240 ms of it, 160 ms at a tenth (a steady
/// tone is the mic's noise to the meter). `None`: nothing was transcribed.
/// `cancel`: cancelled at its end (the bar's ✕) instead of stopped — kept
/// for Undo, and in Home as "Cancelled".
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn e2e_dictate_audio(
    state: tauri::State<'_, crate::AppState>,
    seconds: f32,
    amplitude: Option<f32>,
    cancel: Option<bool>,
) -> Result<Option<crate::dictation::pipeline::Delivered>, String> {
    if !active() {
        return Err("Only in e2e test mode".to_string());
    }
    let robot = {
        let guard = state.pipeline.lock().map_err(|_| "pipeline unavailable")?;
        guard.as_ref().ok_or("pipeline not started")?.robot()
    };
    let n = (seconds.clamp(0.2, 60.0) * 16_000.0) as usize;
    let peak = amplitude.unwrap_or(0.1).clamp(0.0, 1.0);
    let syllables = amplitude.is_some();
    let samples: Vec<f32> = (0..n)
        .map(|i| {
            let level = if syllables && i % 6_400 >= 3_840 { peak / 10.0 } else { peak };
            level * (i as f32 * 2.0 * std::f32::consts::PI * 440.0 / 16_000.0).sin()
        })
        .collect();
    robot.dictate_audio(samples, cancel.unwrap_or(false)).await
}

/// Test mode only: a press-only shortcut as the input hook sends it (a
/// test run has no hook: it must never see the developer's keys) —
/// `name` "paste-last" or "copy-last" (`pasting/last.rs`), or "quick-note"
/// (`notes/quick_note.rs`).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_shortcut(app: AppHandle, name: String) -> Result<(), String> {
    use tauri::Emitter;
    if !active() {
        return Err("Only in e2e test mode".to_string());
    }
    let event = match name.as_str() {
        "paste-last" => "paste-last-key-pressed",
        "copy-last" => "copy-last-key-pressed",
        "quick-note" => "quick-note-key-pressed",
        other => return Err(format!("No such shortcut: {other}")),
    };
    tracing::info!(event, "e2e: a shortcut pressed");
    app.emit(event, crate::dictation::gesture::now_ms()).map_err(|e| e.to_string())
}

/// Test mode only: "Taking longer than usual…" sooner and on cue
/// (`slow.rs`) — never before `floorMs` instead of 4 s (0: the real
/// floor), the stub's transcription taking `sttMs` (0: at once), and this
/// session's timings forgotten when `forget`.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_dictation_timing(floor_ms: Option<u64>, stt_ms: Option<u64>, forget: Option<bool>) -> Result<(), String> {
    if !active() {
        return Err("Only in e2e test mode".to_string());
    }
    let (floor, stt) = (floor_ms.unwrap_or(0).min(60_000), stt_ms.unwrap_or(0).min(60_000));
    tracing::info!(floor, stt, "e2e: dictation timing");
    crate::dictation::slow::set_test(floor, stt, forget.unwrap_or(false));
    Ok(())
}

/// Test mode only: a dictation's length limit shortened to `limitMs`, the
/// bar's warning at `warnMs` (`cap.rs`); none given, the real 15 minutes.
/// `meetingHintMs`: the length that suggests meeting notes (10 minutes).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_dictation_cap(limit_ms: Option<u64>, warn_ms: Option<u64>, meeting_hint_ms: Option<u64>) -> Result<(), String> {
    if !active() {
        return Err("Only in e2e test mode".to_string());
    }
    let cap = limit_ms.map(|limit_ms| crate::dictation::cap::Cap { limit_ms, warn_ms: warn_ms.unwrap_or(limit_ms) });
    tracing::info!(?cap, ?meeting_hint_ms, "e2e: dictation length limit");
    crate::dictation::cap::set_test(cap);
    crate::dictation::cap::set_test_hint(meeting_hint_ms);
    Ok(())
}

/// Test mode only: the next dictation panics in its paste step, as a bug
/// there would (`app_kind::word_before`'s did) — dictation must come back
/// on its own, and say it failed (`pipeline::Busy`).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_crash_next_dictation() -> Result<(), String> {
    if !active() {
        return Err("Only in e2e test mode".to_string());
    }
    tracing::info!("e2e: the next dictation crashes on purpose");
    crate::dictation::pipeline::crash_next_dictation();
    Ok(())
}

/// How long each chunk of an Upload waits once transcribed, in ms
/// ([`e2e_upload_pace`]; 0 = not at all).
#[cfg(debug_assertions)]
static UPLOAD_PACE_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// An Upload's chunk waits this long once transcribed (`pipeline.rs`), in
/// a test run that asked ([`e2e_upload_pace`]). Never in release builds.
#[cfg(debug_assertions)]
pub fn upload_pace() -> Option<std::time::Duration> {
    let ms = UPLOAD_PACE_MS.load(std::sync::atomic::Ordering::SeqCst);
    (active() && ms > 0).then(|| std::time::Duration::from_millis(ms))
}

/// Test mode only: each chunk of an Upload (file transcription) waits `ms`
/// once transcribed — the stub engine does one at once — so a spec can leave
/// the view while the job is still going (`upload.spec.js`). 0: no wait.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_upload_pace(ms: u64) -> Result<(), String> {
    if !active() {
        return Err("Only in e2e test mode".to_string());
    }
    UPLOAD_PACE_MS.store(ms.min(10_000), std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

/// Test mode only: the next `count` saves of a store (`store`: its id,
/// "notes", "history", "chats"…) fail as if the disk were full, so a spec
/// sees what the person would (`saving.rs`). `count` 0: none.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_fail_saves(store: String, count: u32) -> Result<(), String> {
    if !active() {
        return Err("Only in e2e test mode".to_string());
    }
    tracing::info!(store = %store, count, "e2e: saves of this store fail on purpose");
    crate::shell::saving::stage_failures(&store, count);
    Ok(())
}

/// Test mode only: hand the meeting being recorded transcript segments, as
/// if its recorder had just transcribed them (`{ source, text, ts }`; `ts`
/// in unix seconds, so a test can stage hours of meeting at once). They're
/// corrected with the dictionary, persisted, shown and digested exactly like
/// real ones. Returns how many.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_meeting_feed(
    app: AppHandle,
    segments: Vec<crate::notes::TranscriptSegment>,
) -> Result<usize, String> {
    if !active() {
        return Err("Only in e2e test mode".to_string());
    }
    let note_id = crate::meetings::meeting::recording_note().ok_or("No meeting is being recorded")?;
    let n = segments.len();
    let cfg = crate::config::load();
    let segments = segments
        .into_iter()
        .map(|mut seg| {
            if !seg.dictated {
                seg.text = crate::dictation::pipeline::apply_corrections(&seg.text, &cfg);
            }
            seg
        })
        .collect();
    crate::meetings::meeting::ingest(&app, note_id, segments)?;
    Ok(n)
}

// ---- Snip to Chat (snip.rs): fake monitors, never the real screen ----------------------

/// A fake monitor for a test snip: where it is, its size (physical px), its
/// scale, and whether it's an HDR one (its picture then goes through the
/// real FP16 → sRGB conversion at `whiteNits`).
#[cfg(debug_assertions)]
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FakeMonitor {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    #[serde(default = "one")]
    pub scale: f32,
    #[serde(default)]
    pub hdr: bool,
    #[serde(default)]
    pub white_nits: Option<f32>,
}

#[cfg(debug_assertions)]
fn one() -> f32 {
    1.0
}

/// A fake monitor's picture (sRGB): light grey (#F5F5F5, what an 8-bit
/// capture of an HDR desktop clips first), with four 200×100 blocks at
/// (40, 40) #C0C0C0, (260, 40) #E53935, (40, 160) #202020 and (260, 160)
/// #808080 — the snip spec cuts them out and checks their colours.
#[cfg(debug_assertions)]
pub fn fake_pixel(x: i32, y: i32) -> [u8; 3] {
    let in_block = |bx: i32, by: i32| x >= bx && x < bx + 200 && y >= by && y < by + 100;
    if in_block(40, 40) {
        [0xc0, 0xc0, 0xc0]
    } else if in_block(260, 40) {
        [0xe5, 0x39, 0x35]
    } else if in_block(40, 160) {
        [0x20, 0x20, 0x20]
    } else if in_block(260, 160) {
        [0x80, 0x80, 0x80]
    } else {
        [0xf5, 0xf5, 0xf5]
    }
}

/// A fake monitor's frame: BGRA8; an HDR one composed as Windows would
/// (FP16 scRGB boosted to its white level) and converted back by the real
/// `snip::hdr` path.
#[cfg(debug_assertions)]
pub fn fake_frame(m: &FakeMonitor) -> crate::snip::capture::Frame {
    use crate::snip::{capture, hdr};
    let (w, h) = (m.width.max(1) as usize, m.height.max(1) as usize);
    let white = m.white_nits.unwrap_or(if m.hdr { 200.0 } else { 80.0 });
    let bgra = if m.hdr {
        let one = hdr::f32_to_f16(1.0);
        let lut: Vec<u16> = (0..=255u8).map(|v| hdr::composed(v, white)).collect();
        let mut halves = vec![0u16; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let [r, g, b] = fake_pixel(x as i32, y as i32);
                let i = (y * w + x) * 4;
                halves[i..i + 4].copy_from_slice(&[lut[r as usize], lut[g as usize], lut[b as usize], one]);
            }
        }
        hdr::ToSrgb8::new(white).frame_to_bgra8(&halves, w * 4, w, h)
    } else {
        let mut out = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let [r, g, b] = fake_pixel(x as i32, y as i32);
                let i = (y * w + x) * 4;
                out[i..i + 4].copy_from_slice(&[b, g, r, 255]);
            }
        }
        out
    };
    capture::Frame {
        monitor: capture::Monitor {
            handle: 0,
            device: String::new(),
            x: m.x,
            y: m.y,
            width: m.width,
            height: m.height,
            scale: m.scale,
            primary: m.x == 0 && m.y == 0,
            hdr: m.hdr,
            white_nits: white,
        },
        width: w as u32,
        height: h as u32,
        bgra,
        method: capture::Method::Fake,
        capture_ms: 0.0,
        arrived_ms: 0.0,
        convert_ms: 0.0,
    }
}

/// A fake window a test snip can pick (virtual-desktop pixels).
#[cfg(debug_assertions)]
#[derive(Debug, Clone, serde::Deserialize)]
pub struct FakeWindow {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Test mode only: a snip over fake monitors (never the real screen; its
/// overlay windows are made, painted and driven, but never shown), with
/// `windows` to pick (topmost first). `origin` as the page would say it.
/// Returns at once: poll `e2e_snip_last` for "selecting". `start: false`:
/// the fake monitors only, for the next snip however it starts (a spoken
/// "snip", the bar's ✂).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_snip(
    app: AppHandle,
    monitors: Vec<FakeMonitor>,
    windows: Option<Vec<FakeWindow>>,
    origin: Option<String>,
    start: Option<bool>,
) -> Result<(), String> {
    if !active() || crate::shell::realtest::on() {
        return Err("e2e_snip is for test runs (not robot mode)".into());
    }
    let windows = windows.unwrap_or_default().iter().map(|w| crate::snip::geom::Rect::new(w.x, w.y, w.w, w.h)).collect();
    crate::snip::set_fake_frames(monitors.iter().map(fake_frame).collect(), windows);
    if start.unwrap_or(true) {
        crate::snip::start(&app, crate::snip::Origin::parse(origin.as_deref().or(Some("shortcut"))));
    }
    Ok(())
}

/// A mouse or key event for a test snip's overlay (`e2e_snip_input`).
#[cfg(debug_assertions)]
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnipEvent {
    /// "down" | "move" | "up" | "right" | "esc" | "key" | "keyup"
    pub kind: String,
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
    /// A mouse event's Ctrl / Shift (`MK_CONTROL`, `MK_SHIFT`).
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub shift: bool,
    /// "key" / "keyup": "enter" | "space" | "ctrl" | "shift" | "alt" | "c" | "s".
    #[serde(default)]
    pub key: String,
}

/// Test mode (robot mode too): window messages to a snip overlay — the
/// mouse and keys as Windows sends them, never the real ones. A point is
/// in that screen's window's own pixels (past its edge: a drag the window
/// has captured, into the next monitor).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_snip_input(screen: usize, events: Vec<SnipEvent>) -> Result<(), String> {
    if !active() {
        return Err("Only in e2e test mode".into());
    }
    const WM_MOUSEMOVE: u32 = 0x0200;
    const WM_LBUTTONDOWN: u32 = 0x0201;
    const WM_LBUTTONUP: u32 = 0x0202;
    const WM_RBUTTONUP: u32 = 0x0205;
    const WM_KEYDOWN: u32 = 0x0100;
    const WM_KEYUP: u32 = 0x0101;
    const WM_SYSKEYDOWN: u32 = 0x0104;
    const WM_SYSKEYUP: u32 = 0x0105;
    let lp = |x: i32, y: i32| ((y as u16 as isize) << 16) | (x as u16 as isize);
    let mods = |e: &SnipEvent| (if e.ctrl { 0x0008 } else { 0 }) | (if e.shift { 0x0004 } else { 0 });
    let key = |e: &SnipEvent, down: bool| -> Result<(u32, usize, isize), String> {
        let vk = match e.key.as_str() {
            "enter" => 0x0D,
            "space" => 0x20,
            "ctrl" => 0x11,
            "shift" => 0x10,
            "alt" => 0x12,
            "c" => 0x43,
            "s" => 0x53,
            "esc" => 0x1B,
            other => return Err(format!("unknown key {other}")),
        };
        Ok(match (vk == 0x12, down) {
            (true, true) => (WM_SYSKEYDOWN, vk, 0),
            (true, false) => (WM_SYSKEYUP, vk, 0),
            (false, true) => (WM_KEYDOWN, vk, 0),
            (false, false) => (WM_KEYUP, vk, 0),
        })
    };
    let msgs = events
        .iter()
        .map(|e| match e.kind.as_str() {
            "down" => Ok((WM_LBUTTONDOWN, 1usize | mods(e), lp(e.x, e.y))),
            "move" => Ok((WM_MOUSEMOVE, 1 | mods(e), lp(e.x, e.y))),
            "hover" => Ok((WM_MOUSEMOVE, mods(e), lp(e.x, e.y))),
            "up" => Ok((WM_LBUTTONUP, mods(e), lp(e.x, e.y))),
            "right" => Ok((WM_RBUTTONUP, 0, lp(e.x, e.y))),
            "esc" => Ok((WM_KEYDOWN, 0x1B, 0)),
            "key" => key(e, true),
            "keyup" => key(e, false),
            other => Err(format!("unknown event {other}")),
        })
        .collect::<Result<Vec<_>, String>>()?;
    #[cfg(windows)]
    {
        crate::snip::overlay::post_input(screen, msgs)
    }
    #[cfg(not(windows))]
    {
        let _ = (screen, msgs);
        Err("Windows only".into())
    }
}

/// Test mode (robot mode too): how the last snip went — its outcome, the
/// monitors and how each was captured, the overlay's timing, the image.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_snip_last() -> Result<Option<crate::snip::Last>, String> {
    if !active() {
        return Err("Only in e2e test mode".into());
    }
    Ok(crate::snip::last())
}

/// Test mode only: where the next snip's or chat screenshot's Save goes
/// (Windows' dialog never opens in a test run).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_snip_save_path(path: String) -> Result<(), String> {
    if !active() || crate::shell::realtest::on() {
        return Err("Only in e2e test mode".into());
    }
    crate::snip::copy_save::set_test_save_path(std::path::PathBuf::from(path));
    Ok(())
}

/// Test mode only: the PNG a copy put on the pretend clipboard (the real
/// one is never touched in a test run), as base64, or null. `clear`: and
/// empty it.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_snip_clipboard(clear: Option<bool>) -> Result<Option<String>, String> {
    if !active() {
        return Err("Only in e2e test mode".into());
    }
    use base64::Engine;
    let png = if clear.unwrap_or(false) { crate::snip::copy_save::take_test_clipboard() } else { crate::snip::copy_save::test_clipboard() };
    Ok(png.map(|png| base64::engine::general_purpose::STANDARD.encode(png)))
}

/// Test mode only: what the question after the next spoken "snip" says
/// (its transcript, "listened" for `listenMs`), in the microphone's place;
/// null: nothing staged (it would listen, and nothing is asked).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_snip_question(text: Option<String>, listen_ms: Option<u64>) -> Result<(), String> {
    if !active() || crate::shell::realtest::on() {
        return Err("Only in e2e test mode".into());
    }
    crate::dictation::pipeline::stage_snip_question(text.map(|t| (t, listen_ms.unwrap_or(300))));
    Ok(())
}

/// Test mode only: the text "Send its text instead" reads from a
/// screenshot (in place of Windows' OCR, whose languages differ per PC);
/// null: Windows' own.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_snip_ocr(text: Option<String>) -> Result<(), String> {
    if !active() {
        return Err("Only in e2e test mode".into());
    }
    crate::snip::ocr::set_fake(text);
    Ok(())
}

/// Test mode only: files dropped on the main window, as Windows' drag and
/// drop reports them (Tauri's `tauri://drag-drop`, which Playwright can't
/// make).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_drag_drop(app: AppHandle, paths: Vec<String>) -> Result<(), String> {
    if !active() {
        return Err("Only in e2e test mode".into());
    }
    use tauri::Emitter;
    let target = tauri::EventTarget::AnyLabel { label: "settings".into() };
    let at = serde_json::json!({ "x": 200, "y": 200 });
    app.emit_to(target.clone(), "tauri://drag-enter", serde_json::json!({ "paths": paths, "position": at })).map_err(|e| e.to_string())?;
    app.emit_to(target, "tauri://drag-drop", serde_json::json!({ "paths": paths, "position": at })).map_err(|e| e.to_string())
}

/// End of setup: say so in the log (the harness won't drive an instance that
/// doesn't) and quit once the harness closes our stdin (it finished, or died).
pub fn start(app: &AppHandle) {
    if !active() {
        return;
    }
    tracing::info!("e2e: test mode on");
    // The real-app tests' robot mode (YAP_REALTEST=1): real pastes, keys and
    // reads, only into the test's own windows (realtest.rs).
    crate::shell::realtest::start(app);
    let app = app.clone();
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin().lock(), &mut std::io::sink());
        tracing::info!("e2e: stdin closed, exiting");
        app.exit(0);
    });
}

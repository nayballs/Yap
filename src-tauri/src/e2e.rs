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
//!   [`e2e_meeting_quiet`] shortens the quiet-side warning's timings;
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
    mic: crate::meeting::AudioBuf,
    sys: crate::meeting::AudioBuf,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    overflow: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    use crate::meeting::{Follow, OutputFollower};
    use cpal::traits::{DeviceTrait, HostTrait};
    use std::sync::atomic::Ordering;
    // Like the real capture, a configured microphone that isn't plugged in
    // fails (found by listing the devices; none is opened).
    if let Some(name) = crate::config::load().input_device {
        let present = cpal::default_host()
            .input_devices()
            .map(|mut devices| devices.any(|d| d.name().is_ok_and(|n| n == name)))
            .unwrap_or(false);
        if !present {
            return Err(format!("Input device not found: {name}"));
        }
    }
    let dir = std::env::var("YAP_E2E_MEETING_AUDIO")
        .ok()
        .filter(|d| !d.trim().is_empty())
        .map(std::path::PathBuf::from);
    let load = |name: &str| -> Result<Vec<f32>, String> {
        match &dir {
            Some(d) => crate::media::decode_to_16k_mono(&d.join(name))
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
            let length = you.len().max(them.len());
            let mut played = false;
            while !stop.load(Ordering::SeqCst) {
                let secs = pos as f32 / 16_000.0;
                {
                    let mut s = script();
                    if let Some((from, to)) = s.dictation {
                        if !dictating && secs >= from && secs < to {
                            tracing::info!(secs, "e2e: a dictation starts");
                            crate::pipeline::dictation_began();
                            dictating = true;
                        } else if secs >= to {
                            if dictating {
                                tracing::info!(secs, "e2e: the dictation ends");
                                crate::pipeline::dictation_ended();
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
                if switching.is_some_and(|until| pos >= until) {
                    switching = None;
                    if let Follow::Switch(id) = output.poll(Some("e2e-headset".to_string())) {
                        output.opened(id);
                        crate::meeting::followed_output("e2e headset");
                    }
                }
                for (src, buf, side) in [(&you, &mic, "you"), (&them, &sys, "them")] {
                    if side == "them" && switching.is_some() {
                        continue;
                    }
                    if pos < src.len() {
                        let end = (pos + step).min(src.len());
                        crate::meeting::push_audio(buf, &src[pos..end], &overflow);
                    } else if side == "you" && live_mic() {
                        // A real mic never stops delivering: digital silence.
                        crate::meeting::push_audio(buf, &vec![0.0; step], &overflow);
                    }
                }
                crate::meeting::keep_in_step(&mic, &sys);
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
                crate::pipeline::dictation_ended();
            }
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
    crate::meeting::set_test_quiet_timings(timings);
    Ok(())
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
    crate::pipeline::set_test_silent_mic(silent);
    Ok(())
}

/// Test mode only (the e2e suite and the robot): the dictation mic's state
/// (`mic.rs`) — whether a stream is open, live, Bluetooth, how many were
/// opened and closed, how the last open went (ms from asking to its first
/// audio), and the warm window.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_mic_status(state: tauri::State<'_, crate::AppState>) -> Result<crate::mic::Status, String> {
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
/// `None`: nothing was transcribed.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn e2e_dictate_audio(
    state: tauri::State<'_, crate::AppState>,
    seconds: f32,
) -> Result<Option<crate::pipeline::Delivered>, String> {
    if !active() {
        return Err("Only in e2e test mode".to_string());
    }
    let robot = {
        let guard = state.pipeline.lock().map_err(|_| "pipeline unavailable")?;
        guard.as_ref().ok_or("pipeline not started")?.robot()
    };
    let n = (seconds.clamp(0.2, 60.0) * 16_000.0) as usize;
    let samples: Vec<f32> = (0..n).map(|i| 0.1 * (i as f32 * 2.0 * std::f32::consts::PI * 440.0 / 16_000.0).sin()).collect();
    robot.dictate_audio(samples, false).await
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
    crate::pipeline::crash_next_dictation();
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
    crate::saving::stage_failures(&store, count);
    Ok(())
}

/// One stretch of [`e2e_assistant_audio`]'s audio.
#[cfg(debug_assertions)]
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Part {
    /// A 440 Hz tone at `db` dBFS RMS: talk, to the level meter.
    Tone { secs: f32, db: f32 },
    /// Only the background (`noiseDb`).
    Quiet { secs: f32 },
    /// A WAV file (any format Upload reads): Windows' text-to-speech
    /// saying a sentence, in the real-app tests.
    Wav { path: String },
}

/// `parts` one after another, over white noise at `noise_db` dBFS RMS (a
/// room; the same noise every run), 16 kHz mono.
#[cfg(debug_assertions)]
fn render(parts: &[Part], noise_db: f32) -> Result<Vec<f32>, String> {
    use std::f32::consts::{SQRT_2, TAU};
    let mut out = Vec::new();
    for part in parts {
        match part {
            Part::Tone { secs, db } => {
                let amp = 10f32.powf(db / 20.0) * SQRT_2;
                let n = (secs.clamp(0.0, 60.0) * 16_000.0) as usize;
                out.extend((0..n).map(|i| amp * (i as f32 * TAU * 440.0 / 16_000.0).sin()));
            }
            Part::Quiet { secs } => out.resize(out.len() + (secs.clamp(0.0, 60.0) * 16_000.0) as usize, 0.0),
            Part::Wav { path } => out.extend(crate::media::decode_to_16k_mono(std::path::Path::new(path)).map_err(|e| format!("{path}: {e}"))?),
        }
    }
    // Uniform noise on [-a, a] has an RMS of a/√3.
    let a = 10f32.powf(noise_db / 20.0) * 3f32.sqrt();
    let mut x: u32 = 0x9E37_79B9;
    for s in &mut out {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        *s += a * (x as f32 / u32::MAX as f32 * 2.0 - 1.0);
    }
    Ok(out)
}

/// Test mode only: a question to the Voice Assistant from audio played in
/// the mic's place — tones for talk and quiet stretches over a room's noise
/// (`noiseDb`, −62 dBFS unless given), or WAV files — its shortcut pressed
/// as `press` says ("toggle", "hold", "handsFree": set the recording mode to
/// match), through the real key path, so "Stop listening when I pause"
/// (`pause.rs`) runs as it would on a real mic. Returns whether the pause
/// ended it before the audio did, and how much was played; the question then
/// goes to Chat as ever. The machine's real microphone is held but its audio
/// left out, as for [`e2e_dictate_audio`].
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn e2e_assistant_audio(
    state: tauri::State<'_, crate::AppState>,
    parts: Vec<Part>,
    press: Option<crate::pipeline::Press>,
    noise_db: Option<f32>,
) -> Result<crate::pipeline::AskedAloud, String> {
    if !active() {
        return Err("Only in e2e test mode".to_string());
    }
    let noise_db = noise_db.unwrap_or(-62.0);
    let samples = tauri::async_runtime::spawn_blocking(move || render(&parts, noise_db))
        .await
        .map_err(|e| format!("render task failed: {e}"))??;
    let robot = {
        let guard = state.pipeline.lock().map_err(|_| "pipeline unavailable")?;
        guard.as_ref().ok_or("pipeline not started")?.robot()
    };
    robot.ask_assistant_audio(samples, press.unwrap_or(crate::pipeline::Press::Toggle)).await
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
    let note_id = crate::meeting::recording_note().ok_or("No meeting is being recorded")?;
    let n = segments.len();
    let cfg = crate::config::load();
    let segments = segments
        .into_iter()
        .map(|mut seg| {
            if !seg.dictated {
                seg.text = crate::pipeline::apply_corrections(&seg.text, &cfg);
            }
            seg
        })
        .collect();
    crate::meeting::ingest(&app, note_id, segments)?;
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

/// Test mode only: a snip over fake monitors (never the real screen; its
/// overlay windows are made, painted and driven, but never shown). `origin`
/// as the page would say it. Returns at once: poll `e2e_snip_last` for
/// "selecting". `start: false`: the fake monitors only, for the next snip
/// however it starts (a spoken "snip", the bar's ✂).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_snip(app: AppHandle, monitors: Vec<FakeMonitor>, origin: Option<String>, start: Option<bool>) -> Result<(), String> {
    if !active() || crate::realtest::on() {
        return Err("e2e_snip is for test runs (not robot mode)".into());
    }
    crate::snip::set_fake_frames(monitors.iter().map(fake_frame).collect());
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
    /// "down" | "move" | "up" | "right" | "esc"
    pub kind: String,
    #[serde(default)]
    pub x: i32,
    #[serde(default)]
    pub y: i32,
}

/// Test mode (robot mode too): window messages to a snip overlay — the
/// mouse and keys as Windows sends them, never the real ones.
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
    let lp = |x: i32, y: i32| ((y as u16 as isize) << 16) | (x as u16 as isize);
    let msgs = events
        .iter()
        .map(|e| match e.kind.as_str() {
            "down" => Ok((WM_LBUTTONDOWN, 1usize, lp(e.x, e.y))),
            "move" => Ok((WM_MOUSEMOVE, 1, lp(e.x, e.y))),
            "up" => Ok((WM_LBUTTONUP, 0, lp(e.x, e.y))),
            "right" => Ok((WM_RBUTTONUP, 0, lp(e.x, e.y))),
            "esc" => Ok((WM_KEYDOWN, 0x1B, 0)),
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

/// End of setup: say so in the log (the harness won't drive an instance that
/// doesn't) and quit once the harness closes our stdin (it finished, or died).
pub fn start(app: &AppHandle) {
    if !active() {
        return;
    }
    tracing::info!("e2e: test mode on");
    // The real-app tests' robot mode (YAP_REALTEST=1): real pastes, keys and
    // reads, only into the test's own windows (realtest.rs).
    crate::realtest::start(app);
    let app = app.clone();
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin().lock(), &mut std::io::sink());
        tracing::info!("e2e: stdin closed, exiting");
        app.exit(0);
    });
}

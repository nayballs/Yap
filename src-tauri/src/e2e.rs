//! Test mode for the end-to-end UI suite (`e2e/`, see docs/e2e-tests.md).
//!
//! The suite runs a **debug** build with `YAP_E2E=1` right next to whatever
//! Yap the developer has open, and drives its webviews over CDP. Test mode
//! keeps that instance inside its own windows and portable data dir:
//!
//! - no global keyboard/mouse hook: it never sees, or swallows, real keys;
//! - nothing is pasted, typed or copied into other apps (a dictation's text
//!   still reaches history and the Home feed);
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
//!   [`e2e_meeting_quiet`] shortens the quiet-side warning's timings.
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

/// The meeting recorder's capture in test mode: plays `you.wav` into the
/// mic buffer and `them.wav` into the loopback buffer, in step, from
/// `YAP_E2E_MEETING_AUDIO` (any format `media.rs` decodes), then goes quiet
/// until stop. Without the variable it's silent: segments come from
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

/// End of setup: say so in the log (the harness won't drive an instance that
/// doesn't) and quit once the harness closes our stdin (it finished, or died).
pub fn start(app: &AppHandle) {
    if !active() {
        return;
    }
    tracing::info!("e2e: test mode on");
    let app = app.clone();
    std::thread::spawn(move || {
        let _ = std::io::copy(&mut std::io::stdin().lock(), &mut std::io::sink());
        tracing::info!("e2e: stdin closed, exiting");
        app.exit(0);
    });
}

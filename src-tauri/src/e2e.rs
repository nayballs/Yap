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
//!   (a two-hour meeting in seconds).
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

/// The meeting recorder's capture in test mode: plays `you.wav` into the
/// mic buffer and `them.wav` into the loopback buffer, in step, from
/// `YAP_E2E_MEETING_AUDIO` (any format `media.rs` decodes), then goes quiet
/// until stop. Without the variable it's silent: segments come from
/// [`e2e_meeting_feed`] instead.
#[cfg(debug_assertions)]
pub fn spawn_meeting_audio(
    mic: crate::meeting::AudioBuf,
    sys: crate::meeting::AudioBuf,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    overflow: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Result<(), String> {
    use std::sync::atomic::Ordering;
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
            while !stop.load(Ordering::SeqCst) {
                for (src, buf) in [(&you, &mic), (&them, &sys)] {
                    if pos < src.len() {
                        let end = (pos + step).min(src.len());
                        crate::meeting::push_audio(buf, &src[pos..end], &overflow);
                    }
                }
                pos += step;
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        })
        .map_err(|e| format!("Failed to spawn the e2e capture thread: {e}"))?;
    Ok(())
}

/// Test mode only: hand the meeting being recorded transcript segments, as
/// if its recorder had just transcribed them (`{ source, text, ts }`; `ts`
/// in unix seconds, so a test can stage hours of meeting at once). They're
/// persisted, shown and digested exactly like real ones. Returns how many.
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

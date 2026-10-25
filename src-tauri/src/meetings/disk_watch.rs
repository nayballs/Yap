//! **Disk space while a meeting records** (Wispr Flow's low-disk guard).
//! A meeting writes its audio as it records (`meeting_audio.rs`: both sides,
//! lossless FLAC) and its lines into notes.json, so a disk that fills up
//! mid-meeting would cut both off. While a meeting records, Yap looks at
//! the free space on the drive its data is on every minute and:
//!
//! - **warns** — "About 25 minutes of recording left on this drive" —
//!   through the notice policy's one place (`meeting_guard::notice`: the
//!   main window's toast while it's focused, else the Yap bar, else a
//!   Windows notification), once when it's under [`WARN_MIN`] minutes and
//!   again under [`LAST_MIN`]. "Minutes left" is the free space above the
//!   reserve, at **this meeting's own bytes per minute** (its audio files'
//!   size over the time recorded, [`rate`]; until a minute is in, the most
//!   FLAC can take, two sides of 16-bit 16 kHz);
//! - **stops cleanly** when it truly runs out — under [`RESERVE`] free,
//!   kept so notes.json and the files' ends can still be written: the
//!   recording stops as at its length limit (`meeting_guard::
//!   stop_and_summarise`: every chunk transcribed, the audio closed, the
//!   summary written), and the notice says why.
//!
//! The arithmetic is pure ([`minutes_left`], [`step`]) and unit-tested. Test
//! mode: `e2e_disk_space` stands in for the drive's free space and the
//! audio's rate, and looks every 300 ms. Logs: minutes and megabytes only.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Listener};

/// Free space kept for saving what the meeting has (notes.json, the
/// audio's last frames): the recording stops before the drive goes under it.
pub const RESERVE: u64 = 200 * MB;
/// Warn once under this many minutes left…
pub const WARN_MIN: u64 = 30;
/// …and again under this many.
pub const LAST_MIN: u64 = 10;
/// Until a minute of this meeting is in: the most its audio can take a
/// minute (both sides, FLAC never above 16-bit PCM: 1.92 MB each).
pub const FIRST_RATE: u64 = 2 * 1_920_000;
/// The least a minute is taken to cost (a silent call compresses to
/// almost nothing; the lines and notes still grow).
pub const MIN_RATE: u64 = 64 * 1024;
const MB: u64 = 1024 * 1024;
const CHECK: Duration = Duration::from_secs(60);
const TEST_CHECK: Duration = Duration::from_millis(300);

/// What the watch does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Wait,
    /// Say "About N minutes of recording left".
    Warn(u64),
    /// Out of space: stop, keeping everything.
    Stop,
}

/// This meeting's audio bytes a minute (pure): `bytes` written in
/// `elapsed`; [`FIRST_RATE`] before a minute is in; never under [`MIN_RATE`].
pub fn rate(bytes: u64, elapsed: Duration) -> u64 {
    if elapsed < Duration::from_secs(60) {
        return FIRST_RATE;
    }
    let per_min = (bytes as u128 * 60_000 / elapsed.as_millis().max(1)) as u64;
    per_min.max(MIN_RATE)
}

/// Minutes of recording left (pure): the free space above [`RESERVE`] at
/// `per_min` bytes a minute.
pub fn minutes_left(free: u64, per_min: u64) -> u64 {
    free.saturating_sub(RESERVE) / per_min.max(1)
}

/// The next step (pure): `warned` = the lowest warning given so far
/// (minutes; `None`: none yet).
pub fn step(free: u64, per_min: u64, warned: Option<u64>) -> Step {
    if free <= RESERVE {
        return Step::Stop;
    }
    let left = minutes_left(free, per_min);
    let due = if left < LAST_MIN {
        Some(LAST_MIN)
    } else if left < WARN_MIN {
        Some(WARN_MIN)
    } else {
        None
    };
    match due {
        Some(level) if warned.is_none_or(|w| level < w) => Step::Warn(left),
        _ => Step::Wait,
    }
}

/// "About 25 minutes of recording left on this drive" (and under a minute,
/// "Less than a minute…").
pub fn left_words(minutes: u64) -> String {
    match minutes {
        0 => "Less than a minute of recording left on this drive".to_string(),
        1 => "About 1 minute of recording left on this drive".to_string(),
        m => format!("About {m} minutes of recording left on this drive"),
    }
}

/// Which recording is watched (a new one, or none, ends the last watch).
static GEN: AtomicU64 = AtomicU64::new(0);

/// Test mode: (free bytes, bytes a minute) instead of the drive's.
static STAND_IN: Mutex<Option<(u64, Option<u64>)>> = Mutex::new(None);

fn stand_in() -> Option<(u64, Option<u64>)> {
    *STAND_IN.lock().unwrap_or_else(|p| p.into_inner())
}

/// The free space on the drive Yap's data is on.
fn free_now() -> Option<u64> {
    if let Some((free, _)) = stand_in() {
        return Some(free);
    }
    crate::ai::local_llm::runtime::free_bytes(&crate::config::data_dir())
}

/// Watch recording `gen` of note `note_id` until it stops.
fn watch(app: AppHandle, gen: u64, note_id: u64) {
    let started = Instant::now();
    let mut warned: Option<u64> = None;
    loop {
        std::thread::sleep(if crate::shell::e2e::active() { TEST_CHECK } else { CHECK });
        if GEN.load(Ordering::SeqCst) != gen {
            return;
        }
        let Some(free) = free_now() else { continue };
        let per_min = match stand_in().and_then(|(_, r)| r) {
            Some(r) => r,
            None => rate(crate::meetings::meeting_audio::live_bytes(note_id), started.elapsed()),
        };
        match step(free, per_min, warned) {
            Step::Wait => {}
            Step::Warn(left) => {
                warned = Some(if left < LAST_MIN { LAST_MIN } else { WARN_MIN });
                tracing::info!(note_id, minutes = left, free_mb = free / MB, rate_kb = per_min / 1024, "Disk watch: little space left");
                crate::calls::meeting_guard::notice(&app, &notice_left(note_id, left));
            }
            Step::Stop => {
                tracing::warn!(note_id, free_mb = free / MB, "Disk watch: the drive is full; stopping the recording");
                GEN.fetch_add(1, Ordering::SeqCst);
                crate::calls::meeting_guard::stop_and_summarise(&app, Some(note_id));
                crate::calls::meeting_guard::notice(&app, &notice_stopped(note_id));
                return;
            }
        }
    }
}

fn notice_left(note_id: u64, minutes: u64) -> crate::calls::meeting_guard::Notice {
    crate::calls::meeting_guard::Notice {
        // A recording's own trouble (the window's toast keeps it up longer).
        kind: "warning",
        title: left_words(minutes),
        body: "Your disk is almost full. Free some space, or Yap stops recording before it fills up, keeping everything so far."
            .to_string(),
        icon: "timer",
        variant: "default",
        settings: None,
        note_id: Some(note_id),
        surface: None,
    }
}

fn notice_stopped(note_id: u64) -> crate::calls::meeting_guard::Notice {
    crate::calls::meeting_guard::Notice {
        kind: "stopped",
        title: "Stopped: your disk is full".to_string(),
        body: "Yap stopped recording so nothing is lost: everything up to now is kept, and it's writing your action plan."
            .to_string(),
        icon: "timer",
        variant: "default",
        settings: None,
        note_id: Some(note_id),
        surface: None,
    }
}

/// A recording started or stopped (`yap-meeting-state`).
fn on_meeting_state(app: &AppHandle, payload: &str) {
    let state: serde_json::Value = serde_json::from_str(payload).unwrap_or_default();
    let gen = GEN.fetch_add(1, Ordering::SeqCst) + 1;
    if state["recording"].as_bool() != Some(true) {
        return;
    }
    let Some(note_id) = state["noteId"].as_u64() else { return };
    let app = app.clone();
    if let Err(e) = std::thread::Builder::new().name("meeting-disk".into()).spawn(move || watch(app, gen, note_id)) {
        tracing::warn!("Disk watch: couldn't watch the recording ({e})");
    }
}

/// Follow the meeting recorder (app setup).
pub fn init(app: &AppHandle) {
    let handle = app.clone();
    app.listen("yap-meeting-state", move |event| on_meeting_state(&handle, event.payload()));
}

/// Test mode only: the drive's free space (`free`, bytes) and this
/// meeting's bytes a minute (`perMin`) as given; no `free`: the real ones.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_disk_space(free: Option<u64>, per_min: Option<u64>) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    *STAND_IN.lock().unwrap_or_else(|p| p.into_inner()) = free.map(|f| (f, per_min));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minutes_left_are_the_free_space_above_the_reserve_at_the_meetings_rate() {
        // 1.2 MB a minute (speech, both sides), 236 MB above the reserve.
        let per_min = 1_200 * 1024;
        assert_eq!(minutes_left(RESERVE + 236 * MB, per_min), 201);
        assert_eq!(minutes_left(RESERVE + 30 * MB, per_min), 25);
        assert_eq!(minutes_left(RESERVE - MB, per_min), 0);
        // The rate: measured once a minute is in, the most FLAC takes before.
        assert_eq!(rate(10 * MB, Duration::from_secs(30)), FIRST_RATE);
        assert_eq!(rate(12 * MB, Duration::from_secs(600)), 12 * MB / 10);
        assert_eq!(rate(1_000, Duration::from_secs(600)), MIN_RATE, "a silent call");
    }

    #[test]
    fn it_warns_twice_then_stops_when_the_space_runs_out() {
        let per_min = MB;
        assert_eq!(step(RESERVE + 120 * MB, per_min, None), Step::Wait);
        assert_eq!(step(RESERVE + 25 * MB, per_min, None), Step::Warn(25));
        // Said: not again at the same level…
        assert_eq!(step(RESERVE + 24 * MB, per_min, Some(WARN_MIN)), Step::Wait);
        // …but again near the end.
        assert_eq!(step(RESERVE + 9 * MB, per_min, Some(WARN_MIN)), Step::Warn(9));
        assert_eq!(step(RESERVE + 2 * MB, per_min, Some(LAST_MIN)), Step::Wait);
        // Already low when the meeting starts: one warning, the last one.
        assert_eq!(step(RESERVE + 5 * MB, per_min, None), Step::Warn(5));
        // Truly out (only the reserve left): stop, keeping everything.
        assert_eq!(step(RESERVE, per_min, Some(LAST_MIN)), Step::Stop);
        assert_eq!(step(0, per_min, None), Step::Stop);
        assert_eq!(left_words(25), "About 25 minutes of recording left on this drive");
        assert_eq!(left_words(1), "About 1 minute of recording left on this drive");
        assert_eq!(left_words(0), "Less than a minute of recording left on this drive");
    }
}

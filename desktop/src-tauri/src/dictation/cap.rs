//! **A dictation's length limit** (Wispr Flow's, by behaviour: it warns at
//! 19 minutes and stops at 20; Yap at 14 and 15). A recording stops at
//! [`LIMIT_MS`] — a stuck key or a forgotten toggle can't fill the memory,
//! ~57 MB of audio by then — and **nothing said is lost**:
//!
//! - At [`WARN_MS`] the bar's capsule says so ([`warning`]: "1 minute
//!   left · it'll stop at 15 minutes", `yap-dictation-cap {leftMs, text}`)
//!   for a few seconds, then a hairline runs down the minute left.
//! - At the limit the recording stops as the hotkey's second press would
//!   (`pipeline::Shared::stop_at_cap`): transcribed, cleaned up, pasted back
//!   into the window it started in — or, if that's gone, left on the
//!   clipboard rather than typed into whatever is in front (`inject_text`'s
//!   `strict`, as Undo's) — and an alert says it stopped ([`stopped`]), with
//!   **Recover text**: that dictation's text onto the clipboard again,
//!   privately. History keeps it, with its audio for Run again
//!   (`recent_audio.rs`).
//!
//! Where it's crossed is pure ([`crossed`]: the buffer's length before and
//! after the mic's latest audio), unit-tested; a test run can shorten both
//! times (debug-only [`set_test`], `e2e_dictation_cap`).

use std::sync::Mutex;

use tauri::AppHandle;

use crate::notices::alert::{self, Action, Alert};

/// A recording stops here.
pub const LIMIT_MS: u64 = 15 * 60_000;
/// …and the bar warns here.
pub const WARN_MS: u64 = 14 * 60_000;
/// The bar's warning: `{leftMs, text}`.
pub const EVENT: &str = "yap-dictation-cap";
/// The alert's **Recover text** (`alert::act`).
pub const RECOVER: &str = "dictationRecover";
/// Audio samples a second (the pipeline's 16 kHz mono).
const RATE: u64 = 16_000;

/// The limit and the warning, in ms of recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cap {
    pub limit_ms: u64,
    pub warn_ms: u64,
}

impl Cap {
    pub const DEFAULT: Cap = Cap { limit_ms: LIMIT_MS, warn_ms: WARN_MS };

    pub fn limit_samples(&self) -> usize {
        (self.limit_ms * RATE / 1000) as usize
    }

    pub fn warn_samples(&self) -> usize {
        (self.warn_ms * RATE / 1000) as usize
    }

    /// How long is left at the warning.
    pub fn left_ms(&self) -> u64 {
        self.limit_ms.saturating_sub(self.warn_ms)
    }
}

/// A recording's buffer just crossed…
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Crossed {
    /// …the warning.
    Warn,
    /// …the limit: stop.
    Stop,
}

/// Did the buffer, `before` → `after` samples long, just cross the warning
/// or the limit? Once each: only the audio that crosses it says so. Pure.
pub fn crossed(cap: &Cap, before: usize, after: usize) -> Option<Crossed> {
    let (warn, limit) = (cap.warn_samples(), cap.limit_samples());
    if before < limit && after >= limit {
        Some(Crossed::Stop)
    } else if warn < limit && before < warn && after >= warn {
        Some(Crossed::Warn)
    } else {
        None
    }
}

/// "1 minute", "2 minutes", "30 seconds", "1 second".
pub fn span(ms: u64) -> String {
    let plural = |n: u64, unit: &str| if n == 1 { format!("1 {unit}") } else { format!("{n} {unit}s") };
    if ms >= 60_000 && ms.is_multiple_of(60_000) {
        plural(ms / 60_000, "minute")
    } else {
        plural(ms.div_ceil(1000), "second")
    }
}

/// The bar's warning: "1 minute left · it'll stop at 15 minutes".
pub fn warning(cap: &Cap) -> String {
    format!("{} left · it'll stop at {}", span(cap.left_ms()), span(cap.limit_ms))
}

/// The alert's title: "Dictation stopped at 15 minutes".
pub fn stopped_title(cap: &Cap) -> String {
    format!("Dictation stopped at {}", span(cap.limit_ms))
}

// ---- now --------------------------------------------------------------------------

/// A test run's shorter limit (`e2e_dictation_cap`); `None`: the real one.
#[cfg(debug_assertions)]
static TEST_CAP: Mutex<Option<Cap>> = Mutex::new(None);

/// The limit in force: the real one, or a test run's.
pub fn current() -> Cap {
    #[cfg(debug_assertions)]
    if let Some(cap) = *TEST_CAP.lock().unwrap_or_else(|p| p.into_inner()) {
        if crate::shell::e2e::active() {
            return cap;
        }
    }
    Cap::DEFAULT
}

/// Test mode only: a shorter limit (`None`: the real one again).
#[cfg(debug_assertions)]
pub fn set_test(cap: Option<Cap>) {
    *TEST_CAP.lock().unwrap_or_else(|p| p.into_inner()) = cap;
}

/// A dictation this long suggests meeting notes at its end ("Long
/// dictation? Try meeting notes", `coaching.rs`, once ever): they go on
/// past the limit and summarise.
pub const MEETING_HINT_MS: u64 = 10 * 60_000;

/// A test run's shorter [`MEETING_HINT_MS`] (`e2e_dictation_cap`); 0: the real one.
#[cfg(debug_assertions)]
static TEST_HINT_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The length that suggests meeting notes: the real one, or a test run's.
pub fn meeting_hint_ms() -> u64 {
    #[cfg(debug_assertions)]
    {
        let test = TEST_HINT_MS.load(std::sync::atomic::Ordering::SeqCst);
        if test > 0 && crate::shell::e2e::active() {
            return test;
        }
    }
    MEETING_HINT_MS
}

/// Test mode only: a shorter length for the meeting-notes tip (`None`: the
/// real one again).
#[cfg(debug_assertions)]
pub fn set_test_hint(ms: Option<u64>) {
    TEST_HINT_MS.store(ms.unwrap_or(0), std::sync::atomic::Ordering::SeqCst);
}

/// The text of the dictation that stopped at the limit, for Recover text.
static STOPPED: Mutex<Option<String>> = Mutex::new(None);

/// A dictation stopped at the limit and was delivered (`text`, as pasted;
/// `pasted`: it landed in its window, else it's on the clipboard): say so,
/// with Recover text. Never with the text itself in the log.
pub fn stopped(app: &AppHandle, text: &str, pasted: bool) {
    let cap = current();
    *STOPPED.lock().unwrap_or_else(|p| p.into_inner()) = Some(text.trim().to_string());
    tracing::info!(len = text.len(), pasted, "Dictation: stopped at its length limit");
    let body = if pasted {
        format!(
            "Everything you said is pasted. A dictation stops at {}: start another to go on. Recover text copies it again.",
            span(cap.limit_ms)
        )
    } else {
        "Yap couldn't paste it into the window you were dictating into. Recover text puts it on the clipboard; it's in Home too."
            .to_string()
    };
    alert::show_soon(
        app,
        Alert {
            kind: "dictationCapped",
            icon: "clipboard",
            title: stopped_title(&cap),
            body,
            action: Some(Action { id: RECOVER, label: "Recover text" }),
            timeout_ms: 20_000,
        },
    );
}

/// **Recover text**: the dictation that stopped at the limit onto the
/// clipboard again, privately (as a paste is: out of Win+V history). A test
/// run copies nothing (it logs).
pub fn recover(app: &AppHandle) {
    let text = STOPPED.lock().unwrap_or_else(|p| p.into_inner()).clone();
    let Some(text) = text.filter(|t| !t.is_empty()) else {
        tracing::info!("Recover text: nothing to recover");
        return;
    };
    let copied = if crate::shell::e2e::active() && !crate::shell::realtest::on() {
        tracing::info!(len = text.len(), "e2e: not copying the recovered dictation");
        true
    } else {
        crate::pasting::text_injector::copy_privately(&text)
    };
    tracing::info!(len = text.len(), copied, "Recover text");
    let (title, body) = if copied {
        ("Copied your dictation", "It's on the clipboard, kept out of your clipboard history: press Ctrl+V.")
    } else {
        ("Couldn't copy your dictation", "The clipboard couldn't be written. It's in Home too.")
    };
    alert::confirm(
        app,
        Alert { kind: crate::pasting::last::KIND, icon: "clipboard", title: title.into(), body: body.into(), action: None, timeout_ms: 4_000 },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 15 minutes, a warning at 14: once each, by the audio that crosses.
    #[test]
    fn it_warns_a_minute_before_and_stops_at_fifteen_minutes() {
        let cap = Cap::DEFAULT;
        assert_eq!(cap.limit_samples(), 15 * 60 * 16_000);
        assert_eq!(cap.warn_samples(), 14 * 60 * 16_000);
        let chunk = 160; // 10 ms of audio, as the mic delivers it
        let mut events = Vec::new();
        let mut len = 0;
        while len < cap.limit_samples() + 10 * chunk {
            if let Some(c) = crossed(&cap, len, len + chunk) {
                events.push((c, len + chunk));
            }
            len += chunk;
        }
        assert_eq!(events, vec![(Crossed::Warn, cap.warn_samples()), (Crossed::Stop, cap.limit_samples())]);
        // A big chunk crossing both at once: the stop.
        assert_eq!(crossed(&cap, cap.warn_samples() - 1, cap.limit_samples()), Some(Crossed::Stop));
        // Past it (the pre-roll seeded a long buffer): nothing more.
        assert_eq!(crossed(&cap, cap.limit_samples(), cap.limit_samples() + chunk), None);
    }

    #[test]
    fn the_words_on_the_bar_and_the_alert() {
        let cap = Cap::DEFAULT;
        assert_eq!(warning(&cap), "1 minute left · it'll stop at 15 minutes");
        assert_eq!(stopped_title(&cap), "Dictation stopped at 15 minutes");
        // A test run's shorter one.
        let short = Cap { limit_ms: 8_000, warn_ms: 5_000 };
        assert_eq!(warning(&short), "3 seconds left · it'll stop at 8 seconds");
        assert_eq!(span(1_000), "1 second");
        assert_eq!(span(1_500), "2 seconds");
        assert_eq!(span(120_000), "2 minutes");
        assert_eq!(span(90_000), "90 seconds");
        // No warning when it would come at or after the stop.
        let odd = Cap { limit_ms: 8_000, warn_ms: 8_000 };
        assert_eq!(crossed(&odd, odd.warn_samples() - 1, odd.warn_samples()), Some(Crossed::Stop));
    }
}

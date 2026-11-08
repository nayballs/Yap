//! **Keeping this PC awake while agents work** (Remote Control, 2026-11-08,
//! design §9.4): "Keep this PC awake while agents work" (`link_keep_awake`,
//! on, under Settings → Phone → Remote Control). While an agent is queued,
//! running or asking **and** Remote Control is on, Windows is asked not to
//! idle-sleep (`SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)`),
//! and let go [`LINGER`] after the last one ends, as Claude's desktop app
//! lets go once a session has been idle. Idle sleep only: a closed lid and
//! Sleep from the Start menu still sleep (the setting's small print says so).
//!
//! The hold is a thread's (`ES_CONTINUOUS` is per thread), so one thread of
//! its own keeps it ([`poke`] wakes it; it looks again every 30 s); quitting
//! ends it with the process. Elsewhere than Windows: nothing to do. The rule
//! itself ([`Hold`]) is pure.

use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long the hold stays after the last agent ends.
pub const LINGER: Duration = Duration::from_secs(5 * 60);
/// How often the thread looks again by itself.
const EVERY: Duration = Duration::from_secs(30);

/// **The hold's rule** (pure).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Hold {
    held: bool,
    idle_since: Option<Instant>,
}

impl Hold {
    /// What the hold should be now: `busy` (an agent not over), `allowed`
    /// (Remote Control and its keep-awake switch on). `Some(on)` when it
    /// changes.
    pub fn next(&mut self, busy: bool, allowed: bool, now: Instant) -> Option<bool> {
        let want = if !allowed {
            self.idle_since = None;
            false
        } else if busy {
            self.idle_since = None;
            true
        } else if self.held {
            let since = *self.idle_since.get_or_insert(now);
            now.saturating_duration_since(since) < LINGER
        } else {
            false
        };
        if want == self.held {
            return None;
        }
        self.held = want;
        if !want {
            self.idle_since = None;
        }
        Some(want)
    }

    #[cfg(test)]
    pub fn held(&self) -> bool {
        self.held
    }
}

static POKE: OnceLock<Mutex<mpsc::Sender<()>>> = OnceLock::new();

/// **Look again now** (an agent started or ended, the settings changed):
/// the thread starts the first time.
pub fn poke() {
    let tx = POKE.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<()>();
        let _ = std::thread::Builder::new().name("keep-awake".into()).spawn(move || run(rx));
        Mutex::new(tx)
    });
    let _ = tx.lock().unwrap_or_else(|p| p.into_inner()).send(());
}

fn run(rx: mpsc::Receiver<()>) {
    let mut hold = Hold::default();
    loop {
        let cfg = crate::config::load();
        let allowed = cfg.link_enabled && cfg.link_remote_control && cfg.link_keep_awake;
        let busy = !crate::agents::store::live_ids().is_empty();
        if let Some(on) = hold.next(busy, allowed, Instant::now()) {
            apply(on);
        }
        match rx.recv_timeout(EVERY) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// Ask Windows (or stop asking) not to idle-sleep.
fn apply(on: bool) {
    tracing::info!(on, "Keep awake: agents working with Remote Control on");
    // A test run never holds the PC awake.
    if crate::shell::e2e::active() {
        return;
    }
    #[cfg(windows)]
    {
        use windows::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED};
        let flags = if on { ES_CONTINUOUS | ES_SYSTEM_REQUIRED } else { ES_CONTINUOUS };
        // SAFETY: plain flags, no pointers; it changes only this thread's request.
        let was = unsafe { SetThreadExecutionState(flags) };
        if was.0 == 0 {
            tracing::warn!("Keep awake: Windows didn't take the request");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_while_agents_work_with_remote_control_on_and_let_go_after_five_minutes() {
        let t0 = Instant::now();
        let mut h = Hold::default();
        assert_eq!(h.next(false, true, t0), None, "nothing working: not held");
        assert_eq!(h.next(true, false, t0), None, "Remote Control off: never");
        assert_eq!(h.next(true, true, t0), Some(true));
        assert_eq!(h.next(true, true, t0 + Duration::from_secs(60)), None, "still held");
        // The last agent ended: held 5 minutes more.
        assert_eq!(h.next(false, true, t0 + Duration::from_secs(100)), None);
        assert_eq!(h.next(false, true, t0 + Duration::from_secs(100) + LINGER - Duration::from_secs(1)), None);
        assert_eq!(h.next(false, true, t0 + Duration::from_secs(100) + LINGER), Some(false));
        assert!(!h.held());
        // Another starts meanwhile: the wait starts again after it.
        let mut h = Hold::default();
        h.next(true, true, t0);
        h.next(false, true, t0 + Duration::from_secs(10));
        h.next(true, true, t0 + Duration::from_secs(200));
        assert_eq!(h.next(false, true, t0 + Duration::from_secs(400)), None);
        assert_eq!(h.next(false, true, t0 + Duration::from_secs(400) + LINGER), Some(false));
        // Switched off while held: let go at once.
        let mut h = Hold::default();
        h.next(true, true, t0);
        assert_eq!(h.next(true, false, t0 + Duration::from_secs(1)), Some(false));
    }
}

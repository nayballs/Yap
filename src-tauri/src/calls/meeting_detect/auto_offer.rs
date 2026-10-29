//! **"Start notes automatically for Teams calls?"**, asked once (2026-10-29,
//! Wispr Flow's opt-in by behaviour): someone who keeps clicking Record
//! notes for one app's calls is asked, at the end of a meeting they stopped
//! themselves, whether that app's calls should start by themselves next
//! time (the prompt's 10-second countdown, `meeting_auto_start_apps`).
//!
//! Counted in `<data>/call_apps.json` (`{ recorded: {teams: 4}, offered:
//! ["teams"] }`): how many meetings were recorded from each app's calls
//! (each "Record notes", by click, the tray or the shortcut), and the apps
//! asked about already — once ever, whatever the answer. A file of its own:
//! an older Yap never reads it.

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// Asked once this many meetings of an app's calls were recorded.
pub const AFTER_MEETINGS: u32 = 3;

/// What `call_apps.json` keeps.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Counts {
    /// App id → meetings recorded from its calls.
    pub recorded: BTreeMap<String, u32>,
    /// Apps asked about already.
    pub offered: Vec<String>,
}

/// Why a stop may (not) ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Moment {
    /// The person stopped it (not the length limit, a quiet meeting, the
    /// call ending by itself, the PC sleeping), and it's the end, not a
    /// pause.
    pub by_hand: bool,
    /// A click started it (Record notes, the tray, the shortcut), not the
    /// countdown or a resume.
    pub clicked: bool,
    /// "Started by mistake?" asked about it instead.
    pub mistake: bool,
    /// Yap asks about calls, as pop-ups (a quiet one would never count down).
    pub asking: bool,
    /// The app's calls start by themselves already (all apps', or this one's).
    pub automatic: bool,
}

/// Ask now about `app`? (pure)
pub fn should_offer(c: &Counts, app: &str, m: Moment) -> bool {
    m.by_hand
        && m.clicked
        && !m.mistake
        && m.asking
        && !m.automatic
        && c.recorded.get(app).copied().unwrap_or(0) >= AFTER_MEETINGS
        && !c.offered.iter().any(|a| a == app)
}

static STORE: Mutex<Option<Counts>> = Mutex::new(None);

fn path() -> std::path::PathBuf {
    crate::config::data_dir().join("call_apps.json")
}

/// Change the counts under the store's lock (read, change, write in one
/// step) and return what `f` returns.
pub fn with_counts<T>(f: impl FnOnce(&mut Counts) -> T) -> T {
    let mut guard = STORE.lock().unwrap_or_else(|p| p.into_inner());
    let counts = guard.get_or_insert_with(|| {
        if cfg!(test) {
            return Counts::default();
        }
        let path = path();
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!("call_apps.json unreadable ({e}); starting afresh");
                crate::config::quarantine_corrupt(&path);
                Counts::default()
            }),
            Err(_) => Counts::default(),
        }
    });
    let before = counts.clone();
    let out = f(counts);
    if *counts != before && !cfg!(test) {
        let _ = SAVER.write_json(&path(), &*counts, true);
    }
    out
}

/// A meeting was recorded from `app`'s call.
pub fn recorded(app: &str) {
    with_counts(|c| *c.recorded.entry(app.to_string()).or_default() += 1);
}

/// Ask about `app` now (and never again)? Recorded as asked when so.
pub fn take_offer(app: &str, m: Moment) -> bool {
    with_counts(|c| {
        let ask = should_offer(c, app, m);
        if ask {
            c.offered.push(app.to_string());
        }
        ask
    })
}

/// call_apps.json's saves (`saving.rs`): counts Yap could live without, so
/// a failed save is only logged (and tried again with the next).
pub static SAVER: crate::shell::saving::Saver = crate::shell::saving::Saver::quiet("call-apps", "your call apps' meeting counts", save_now);

fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    if let Some(c) = STORE.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
        let _ = SAVER.write_json(&path(), c, true);
    }
    SAVER.check()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moment() -> Moment {
        Moment { by_hand: true, clicked: true, mistake: false, asking: true, automatic: false }
    }

    #[test]
    fn asked_after_the_third_meeting_stopped_by_hand_and_only_once() {
        let mut c = Counts::default();
        for n in 1..=AFTER_MEETINGS {
            assert!(!should_offer(&c, "teams", moment()), "after {} meetings", n - 1);
            *c.recorded.entry("teams".into()).or_default() += 1;
        }
        assert!(should_offer(&c, "teams", moment()));
        // Another app's meetings don't count.
        assert!(!should_offer(&c, "zoom", moment()));
        // Once ever, whatever the answer.
        c.offered.push("teams".into());
        assert!(!should_offer(&c, "teams", moment()));
    }

    #[test]
    fn never_after_an_automatic_stop_a_mistake_or_when_it_starts_by_itself() {
        let c = Counts { recorded: [("teams".to_string(), 5)].into(), offered: Vec::new() };
        assert!(should_offer(&c, "teams", moment()));
        for m in [
            Moment { by_hand: false, ..moment() },
            Moment { clicked: false, ..moment() },
            Moment { mistake: true, ..moment() },
            Moment { asking: false, ..moment() },
            Moment { automatic: true, ..moment() },
        ] {
            assert!(!should_offer(&c, "teams", m), "{m:?}");
        }
    }

    #[test]
    fn the_counts_read_back() {
        let c = Counts { recorded: [("zoom".to_string(), 2)].into(), offered: vec!["teams".into()] };
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(json, r#"{"recorded":{"zoom":2},"offered":["teams"]}"#);
        assert_eq!(serde_json::from_str::<Counts>(&json).unwrap(), c);
        // A file from later with more in it, or less: what's there is read.
        assert_eq!(serde_json::from_str::<Counts>(r#"{"offered":["meet"],"other":1}"#).unwrap().offered, ["meet"]);
    }
}

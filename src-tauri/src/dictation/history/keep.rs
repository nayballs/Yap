//! **Keep history: always / 24 hours / never** (2026-10-29, after Wispr
//! Flow's): Settings → Privacy & data's history choice.
//!
//! - **The choice is two fields** in config.json: `history_enabled`, as
//!   ever (off = never; an older Yap reads only this), and `history_keep`
//!   ("always" | "day" | "never"; "" = never chosen, so always). Nothing
//!   saved had to change: no migration.
//! - **24 hours** ([`sweep`]): entries older than a day go, with their audio
//!   (kept or not), at start, every hour, and the moment it's chosen
//!   (Settings asks first). The stats stay (`stats.rs`).
//! - **Never** keeps no new dictation; what's there stays until cleared,
//!   as turning history off always did.
//! - **An older Yap** sharing the data folder (the installed nightly beside
//!   a dev build) drops `history_keep` when it saves config.json, and never
//!   sweeps. So the choice is copied into `dictation_stats.json`, which no
//!   older Yap touches; at the next start, a choice gone missing with
//!   history still on comes back as "24 hours" if that's what it was
//!   ([`restore`]). History turned off in the older Yap stays off, and one
//!   turned on there with "never" copied here stays on (always).

use std::time::Duration;

use tauri::{AppHandle, Emitter};

use crate::config::YapConfig;

pub const ALWAYS: &str = "always";
pub const DAY: &str = "day";
pub const NEVER: &str = "never";

/// How long entries are kept under "24 hours".
pub const DAY_SECS: u64 = 24 * 60 * 60;
/// How often the old ones are looked for while Yap runs.
const SWEEP_EVERY: Duration = Duration::from_secs(60 * 60);

/// The choice as it stands: "always", "day" or "never".
pub fn of(cfg: &YapConfig) -> &'static str {
    if !cfg.history_enabled {
        NEVER
    } else if cfg.history_keep == DAY {
        DAY
    } else {
        ALWAYS
    }
}

/// Entries from before this (unix seconds) go, under `keep` at `now`.
pub fn cutoff(keep: &str, now: u64) -> Option<u64> {
    (keep == DAY).then(|| now.saturating_sub(DAY_SECS))
}

/// The choice to bring back at start: "day" when config.json lost it (an
/// older Yap's save) with history still on, and the copy says "day".
pub fn restore(cfg: &YapConfig, copy: &str) -> Option<&'static str> {
    (cfg.history_keep.trim().is_empty() && cfg.history_enabled && copy == DAY).then_some(DAY)
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Delete what's past the choice (and copy the choice into the stats).
/// Returns how many entries went.
pub fn sweep(app: &AppHandle, cfg: &YapConfig) -> usize {
    let keep = of(cfg);
    crate::dictation::stats::set_keep_choice(keep);
    let Some(before) = cutoff(keep, now_secs()) else { return 0 };
    let gone = crate::dictation::history::forget_before(before);
    if gone > 0 {
        tracing::info!(gone, "History: dictations older than a day deleted (Keep history: 24 hours)");
        let _ = app.emit("yap-history-changed", ());
    }
    gone
}

/// At start: the choice an older Yap's save dropped brought back, then the
/// sweep — and again every hour.
pub fn init(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new().name("history-keep".into()).spawn(move || {
        let cfg = crate::config::load();
        if let Some(keep) = restore(&cfg, &crate::dictation::stats::keep_choice()) {
            match crate::config::update(|c| c.history_keep = keep.to_string()) {
                Ok(_) => tracing::info!("History: \"Keep history: 24 hours\" brought back (an older Yap's save dropped it)"),
                Err(e) => tracing::warn!("History: couldn't bring back Keep history ({e})"),
            }
        }
        loop {
            sweep(&app, &crate::config::load());
            std::thread::sleep(SWEEP_EVERY);
        }
    });
}

/// Settings saved: a new choice copied at once, and "24 hours" applied now.
pub fn on_config_saved(app: &AppHandle, prev: &YapConfig, cfg: &YapConfig) {
    if of(prev) == of(cfg) && prev.history_keep == cfg.history_keep {
        return;
    }
    let (app, cfg) = (app.clone(), cfg.clone());
    let _ = std::thread::Builder::new().name("history-keep-now".into()).spawn(move || {
        sweep(&app, &cfg);
    });
}

/// Settings → Privacy & data: how many dictations history keeps, and how
/// many of them are older than a day ("24 hours" asks first about those).
#[tauri::command]
pub fn history_counts() -> serde_json::Value {
    let (entries, older) = crate::dictation::history::counts(now_secs().saturating_sub(DAY_SECS));
    serde_json::json!({ "entries": entries, "olderThanDay": older })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(enabled: bool, keep: &str) -> YapConfig {
        YapConfig { history_enabled: enabled, history_keep: keep.into(), ..YapConfig::default() }
    }

    #[test]
    fn the_choice_reads_from_both_fields() {
        assert_eq!(of(&cfg(true, "")), ALWAYS, "never chosen: as Yap always kept it");
        assert_eq!(of(&cfg(true, "always")), ALWAYS);
        assert_eq!(of(&cfg(true, "day")), DAY);
        assert_eq!(of(&cfg(false, "day")), NEVER, "history off wins (an older Yap turned it off)");
        assert_eq!(of(&cfg(true, "never")), ALWAYS, "turned on again in an older Yap");
        assert_eq!(of(&YapConfig::default()), ALWAYS, "the default stays always");
    }

    #[test]
    fn only_24_hours_deletes_and_only_what_is_a_day_old() {
        assert_eq!(cutoff(DAY, 1_000_000), Some(1_000_000 - DAY_SECS));
        assert_eq!(cutoff(ALWAYS, 1_000_000), None);
        assert_eq!(cutoff(NEVER, 1_000_000), None);
    }

    #[test]
    fn a_choice_an_older_yap_dropped_comes_back() {
        assert_eq!(restore(&cfg(true, ""), DAY), Some(DAY));
        // Still there, or chosen otherwise since: left alone.
        assert_eq!(restore(&cfg(true, "always"), DAY), None);
        // Turned off in the older Yap: stays off.
        assert_eq!(restore(&cfg(false, ""), DAY), None);
        // Nothing to bring back.
        assert_eq!(restore(&cfg(true, ""), ALWAYS), None);
        assert_eq!(restore(&cfg(true, ""), ""), None);
    }

    /// The 24-hour sweep over history (in memory in unit tests).
    #[test]
    fn the_sweep_deletes_entries_older_than_a_day_only() {
        use crate::dictation::history::{self, Dictation};
        let now = now_secs();
        // Today's dictation, recorded now.
        let ts = history::record_dictation(Dictation {
            raw: "keep me",
            text: "Keep me.",
            said: None,
            model: "m",
            app: "notepad.exe",
            audio: None,
            status: None,
            error: None,
            secs: None,
        })
        .unwrap();
        // One from two days ago (a failed one is dated when it was said).
        history::record_failed(Some(now - 2 * DAY_SECS), "m", "notepad.exe", "t-keep-old", "x");
        assert!(history::find_audio(now - 2 * DAY_SECS, "t-keep-old").is_some());
        let gone = history::forget_before(cutoff(DAY, now).unwrap());
        assert!(gone >= 1);
        assert!(history::find_audio(now - 2 * DAY_SECS, "t-keep-old").is_none(), "older than a day: gone");
        assert!(history::find(ts, "Keep me.").is_some(), "today's stays");
    }
}

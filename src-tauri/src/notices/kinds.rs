//! **Kinds of notice, each with a switch and a cap** (2026-10-29; Wispr
//! Flow's notification categories, by behaviour): the nice-to-know notices
//! fall into four kinds — **suggestions** (a word or a snippet to save, a
//! language to add), **milestones**, **feature tips** (push-to-talk's hints,
//! the mic tips, "Yap caught your correction"…) and **news** (what's new
//! after an update) — each switched off on its own in Settings → General →
//! Notifications ([`sync`]: `notifySuggestions`, `notifyMilestones`,
//! `notifyFeatureTips`, `notifyNews`; "Show tips" still switches every tip
//! off) and each shown at most [`cap`] times a day ([`Caps`], kept in
//! memory: a restart starts the day's count afresh).
//!
//! Through the one notice policy: a module about to show one asks
//! `notice::place_tip` (its place, or Nowhere when its kind is off or used
//! up today — counted when it isn't), and the page asks [`notice_allow`]
//! for its own (the "Updated to …" note). Questions, alerts and notices
//! have no kind and are never capped. Pure parts unit-tested. A test run
//! counts nothing unless a spec asks (`e2e_notice_caps`): the suite shows
//! many suggestions in a row.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;

/// A kind of nice-to-know notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Suggestions,
    Milestones,
    FeatureTips,
    News,
}

impl Category {
    const ALL: [Category; 4] = [Category::Suggestions, Category::Milestones, Category::FeatureTips, Category::News];

    fn index(self) -> usize {
        match self {
            Category::Suggestions => 0,
            Category::Milestones => 1,
            Category::FeatureTips => 2,
            Category::News => 3,
        }
    }
}

/// A notice's kind (a bar card's id, an event's kind) → its category, if
/// it has one. A milestone is any kind starting "milestone". Pure.
pub fn category(kind: &str) -> Option<Category> {
    Some(match kind {
        "word-suggestion" | "snippet-suggestion" | "language-nudge" => Category::Suggestions,
        "hint" | "micTip" | "featureTip" | "bar-pointer" => Category::FeatureTips,
        "news" | "updated" => Category::News,
        k if k.starts_with("milestone") => Category::Milestones,
        _ => return None,
    })
}

/// How many of a category may show in a day.
pub fn cap(c: Category) -> u32 {
    match c {
        Category::Suggestions => 3,
        Category::Milestones => 2,
        Category::FeatureTips => 3,
        Category::News => 1,
    }
}

/// Today's counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Caps {
    /// The day counted (days since the epoch, local time).
    pub day: i64,
    pub counts: [u32; 4],
}

impl Caps {
    /// May one more of `c` show on `day`? Pure.
    pub fn admits(&self, c: Category, day: i64) -> bool {
        self.day != day || self.counts[c.index()] < cap(c)
    }

    /// The counts after one of `c` showed on `day` (a new day starts at 0). Pure.
    pub fn counted(mut self, c: Category, day: i64) -> Caps {
        if self.day != day {
            self = Caps { day, counts: [0; 4] };
        }
        self.counts[c.index()] += 1;
        self
    }
}

/// The categories switched off (a bit each, `Category::index`).
static OFF: AtomicU8 = AtomicU8::new(0);
static CAPS: Mutex<Caps> = Mutex::new(Caps { day: 0, counts: [0; 4] });

/// Settings were loaded or saved: the four switches.
pub fn sync(cfg: &crate::config::YapConfig) {
    let mut off = 0u8;
    for (c, on) in Category::ALL.into_iter().zip([cfg.notify_suggestions, cfg.notify_milestones, cfg.notify_feature_tips, cfg.notify_news]) {
        if !on {
            off |= 1 << c.index();
        }
    }
    OFF.store(off, Ordering::SeqCst);
}

fn today() -> i64 {
    use chrono::Datelike;
    i64::from(chrono::Local::now().date_naive().num_days_from_ce())
}

/// Is `kind`'s category switched on? (A kind with none always is.)
pub fn on(kind: &str) -> bool {
    category(kind).is_none_or(|c| OFF.load(Ordering::SeqCst) & (1 << c.index()) == 0)
}

/// `kind` is about to show: whether it may (its category on, and under
/// today's cap), counted if so.
pub fn take(kind: &str) -> bool {
    let Some(c) = category(kind) else { return true };
    if !on(kind) {
        tracing::debug!(kind, "notice: its kind is switched off");
        return false;
    }
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() && !E2E_CAPS.load(Ordering::SeqCst) {
        return true;
    }
    let day = today();
    let mut caps = CAPS.lock().unwrap_or_else(|p| p.into_inner());
    if !caps.admits(c, day) {
        tracing::info!(kind, "notice: as many of its kind as a day takes; not shown");
        return false;
    }
    *caps = caps.counted(c, day);
    true
}

/// The page's own notice of `kind` (the "Updated to …" note is "news"):
/// may it show? Counted if so.
#[tauri::command]
pub fn notice_allow(kind: String) -> bool {
    take(&kind)
}

/// Test mode: the caps count only once a spec asks.
#[cfg(debug_assertions)]
static E2E_CAPS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Test mode only: today's counts forgotten, and the caps off again.
#[cfg(debug_assertions)]
pub fn e2e_reset() {
    *CAPS.lock().unwrap_or_else(|p| p.into_inner()) = Caps::default();
    E2E_CAPS.store(false, Ordering::SeqCst);
}

/// Test mode only: count the daily caps (`on`), from none today.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_notice_caps(on: bool) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    *CAPS.lock().unwrap_or_else(|p| p.into_inner()) = Caps::default();
    E2E_CAPS.store(on, Ordering::SeqCst);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_nice_to_know_kind_has_a_category_and_the_rest_none() {
        assert_eq!(category("word-suggestion"), Some(Category::Suggestions));
        assert_eq!(category("language-nudge"), Some(Category::Suggestions));
        assert_eq!(category("hint"), Some(Category::FeatureTips));
        assert_eq!(category("featureTip"), Some(Category::FeatureTips));
        assert_eq!(category("milestone"), Some(Category::Milestones));
        assert_eq!(category("milestone-words"), Some(Category::Milestones));
        assert_eq!(category("news"), Some(Category::News));
        for kind in ["call", "micBlocked", "update", "meeting-notice", "pasteBlocked"] {
            assert_eq!(category(kind), None, "{kind}");
        }
    }

    #[test]
    fn each_kind_has_its_own_daily_cap() {
        let mut caps = Caps::default();
        for n in 0..cap(Category::FeatureTips) {
            assert!(caps.admits(Category::FeatureTips, 100), "tip {n}");
            caps = caps.counted(Category::FeatureTips, 100);
        }
        assert!(!caps.admits(Category::FeatureTips, 100));
        // Another kind has its own count…
        assert!(caps.admits(Category::Suggestions, 100));
        // …and the next day starts afresh.
        assert!(caps.admits(Category::FeatureTips, 101));
        let next = caps.counted(Category::FeatureTips, 101);
        assert_eq!(next.counts, [0, 0, 1, 0]);
        // News: once a day.
        let news = Caps::default().counted(Category::News, 5);
        assert!(!news.admits(Category::News, 5));
    }
}

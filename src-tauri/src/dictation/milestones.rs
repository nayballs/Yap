//! **Word milestones** (2026-10-29, after Wispr Flow's): "You've dictated
//! 1,000 words", once each, a moment after the dictation that passed it.
//!
//! - **The steps** ([`next_after`]): 100, 500, 1,000, 2,000 and 5,000
//!   words, then every 5,000 to 50,000, every 25,000 to 250,000, then
//!   300,000 and every 100,000 — someone dictating 2,000 words a day hears
//!   of one every few days at first, every couple of weeks later, never
//!   daily.
//! - **Once each**: the highest step said is kept with the stats
//!   (`dictation_stats.json`'s `milestone`, `stats.rs`). Steps passed
//!   before Yap counted them — the history already there, dictations an
//!   older Yap made — are marked passed without a word.
//! - **Through the notice policy** (`notices/notice.rs`, kind `milestone`):
//!   a tip, so "Show tips" switches it off; never while a dictation records
//!   or transcribes, nor during a meeting; dropped if it waited unseen a
//!   minute. One place: the main window's toast while the person is looking
//!   at it, else a card on the Yap bar (never a Windows notification).
//!   Both offer **See Insights**.
//!
//! Its own module, apart from push-to-talk's coaching hints (`coaching.rs`).

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// The main window's toast: `{ words, title, body }`.
pub const EVENT: &str = "yap-milestone";
/// The bar card's **See Insights**: the main window opens on Insights.
pub const OPEN_EVENT: &str = "yap-open-insights";
/// The bar card's id, and the notice policy's kind.
pub const CARD: &str = "milestone";
/// How long the bar card stays (the pointer on it holds it).
const CARD_MS: u64 = 12_000;
/// A dictation that goes on longer than this after the one that passed the
/// step: the moment is dropped (it would come too late to mean anything).
const WAIT_MS: u64 = 120_000;

/// The first steps, then [`next_after`]'s.
const FIRST: [u64; 5] = [100, 500, 1_000, 2_000, 5_000];

/// The step after `step` (0: before any).
pub fn next_after(step: u64) -> u64 {
    if let Some(s) = FIRST.iter().find(|s| **s > step) {
        return *s;
    }
    let every = match step {
        s if s < 50_000 => 5_000,
        s if s < 250_000 => 25_000,
        _ => 100_000,
    };
    (step / every + 1) * every
}

/// The highest step at or under `words` (0: none yet).
pub fn passed(words: u64) -> u64 {
    let mut step = 0;
    loop {
        let next = next_after(step);
        if next > words {
            return step;
        }
        step = next;
    }
}

/// The step to celebrate now, with `said` the highest said so far and
/// `words` dictated in all: the highest one passed, if it's new.
pub fn crossed(said: u64, words: u64) -> Option<u64> {
    let step = passed(words);
    (step > said).then_some(step)
}

/// "1,000".
pub fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// The moment's words: `minutes_saved`, Insights' time saved against typing.
pub fn text(step: u64, minutes_saved: f64) -> (String, String) {
    let title = format!("You've dictated {} words", thousands(step));
    let minutes = minutes_saved.round() as u64;
    let body = if minutes >= 120 {
        format!("That's about {} hours saved compared with typing.", minutes / 60)
    } else if minutes >= 2 {
        format!("That's about {minutes} minutes saved compared with typing.")
    } else {
        "Insights shows how fast you talk and where you dictate most.".to_string()
    };
    (title, body)
}

#[derive(Debug, Clone, Serialize)]
struct Moment {
    words: u64,
    title: String,
    body: String,
}

/// Say a milestone passed: once the dictation is over, in one place, on a
/// thread of its own. `before`: the milestone said until this one, should
/// it have to wait for a later dictation.
pub fn celebrate(app: &AppHandle, step: u64, before: u64, minutes_saved: f64) {
    let app = app.clone();
    let _ = std::thread::Builder::new().name("milestone".into()).spawn(move || {
        // Never over a dictation (the one that passed it is still pasting).
        let waited = std::time::Instant::now();
        while crate::notices::notice::moment().dictating {
            if waited.elapsed() > Duration::from_millis(WAIT_MS) {
                tracing::debug!(step, "Milestone: a dictation went on too long; not said");
                return;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        if crate::notices::notice::tips_off() {
            tracing::info!(step, "Milestone passed; tips are switched off");
            return;
        }
        // Never over a meeting: the main window's toast would land on it, and
        // a bar card would wait unseen and be dropped — yet it was marked
        // said, so it never came. Unsaid, the next dictation after the
        // meeting says it.
        if crate::notices::notice::moment().meeting {
            tracing::info!(step, "Milestone passed during a meeting; said after it");
            crate::dictation::stats::milestone_unsaid(step, before);
            return;
        }
        let (title, body) = text(step, minutes_saved);
        let place = crate::notices::notice::place_tip(&app, CARD);
        let shown = match place {
            crate::notices::notice::Place::Window => app.emit(EVENT, Moment { words: step, title, body }).is_ok(),
            crate::notices::notice::Place::Bar => {
                use crate::bar::{Card, CardAction};
                let card = Card {
                    id: CARD.into(),
                    icon: "tip",
                    title,
                    body,
                    primary: Some(CardAction::new("insights", "See Insights")),
                    timeout_ms: Some(CARD_MS),
                    ..Default::default()
                };
                crate::bar::show_card(
                    &app,
                    card,
                    Box::new(|app, action| {
                        if action == "insights" {
                            let _ = crate::shell::window::show_settings(app);
                            let _ = app.emit(OPEN_EVENT, ());
                        }
                    }),
                )
            }
            _ => false,
        };
        tracing::info!(step, shown, ?place, "Milestone passed");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_steps_start_small_and_spread_out() {
        let mut steps = vec![];
        let mut s = 0;
        while s < 400_000 {
            s = next_after(s);
            steps.push(s);
        }
        assert_eq!(&steps[..8], &[100, 500, 1_000, 2_000, 5_000, 10_000, 15_000, 20_000]);
        assert!(steps.contains(&50_000) && steps.contains(&75_000) && steps.contains(&250_000) && steps.contains(&300_000) && steps.contains(&400_000));
        assert!(!steps.contains(&55_000) && !steps.contains(&275_000) && !steps.contains(&350_000));
        // Every gap grows or stays.
        assert!(steps.windows(3).all(|w| w[2] - w[1] >= w[1] - w[0] || w[1] < 5_000));
    }

    #[test]
    fn a_milestone_is_said_once_and_the_highest_passed_wins() {
        assert_eq!(passed(99), 0);
        assert_eq!(passed(100), 100);
        assert_eq!(passed(4_999), 2_000);
        assert_eq!(passed(61_000), 50_000);
        assert_eq!(crossed(0, 99), None);
        assert_eq!(crossed(0, 104), Some(100));
        assert_eq!(crossed(100, 480), None, "said already");
        // A long dictation past two steps: only the higher is said.
        assert_eq!(crossed(100, 1_050), Some(1_000));
        assert_eq!(crossed(5_000, 10_001), Some(10_000));
    }

    #[test]
    fn its_words_are_short_and_number_the_words() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(950), "950");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(1_234_567), "1,234,567");
        let (title, body) = text(1_000, 19.6);
        assert_eq!(title, "You've dictated 1,000 words");
        assert_eq!(body, "That's about 20 minutes saved compared with typing.");
        assert_eq!(text(50_000, 1_000.0).1, "That's about 16 hours saved compared with typing.");
        assert!(text(100, 1.0).1.starts_with("Insights shows"));
    }
}

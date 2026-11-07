//! **A job's permission prompt on the Yap bar** (the Jobs UI): a running
//! job asks to take a step (the run endpoint's `approve`,
//! `integrations/claude_live/approve.rs`) and no Yap window is in front, so
//! nobody would see the card in Chat's thread: the bar shows it too —
//! "Allow a step in garden-planner?", the step in plain words, **Allow
//! once** / **Deny** (`claude_approve_answer`). Never answered by voice.
//! A plan (Plan mode's ExitPlanMode, 2026-11-07) reads as one: "The plan is
//! ready in garden-planner", its first line, **Go ahead** / **Not yet**
//! (the plan itself is in the thread's card).
//!
//! One card at a time (`jobApprove`, a question in `notices/notice.rs`
//! that waits out a dictation): the oldest prompt waiting. Answered here or
//! in Chat, timed out, or its job gone, the card goes (or the next one takes
//! its place). Its ✕ only hides it: the prompt still waits in Chat.
//!
//! [`sync`] runs on each running job's tick (half a second) and when a turn
//! ends; it's cheap when nothing changed, and asks whether a Yap window is
//! in front (a main-thread round trip) at most every [`RECHECK`] per prompt,
//! off the job's task. The log has the prompt's id, never what it asks.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::integrations::claude_live::{self, Pending};

/// The card's id (its rule in `notices/notice.rs`).
pub const CARD: &str = "jobApprove";
/// How often a waiting prompt asks again whether a Yap window is in front.
const RECHECK: Duration = Duration::from_secs(2);
/// How much of the step the card says.
const BODY_MAX: usize = 160;

#[derive(Default)]
struct State {
    /// The prompt whose card is up.
    shown: Option<u64>,
    /// The prompt last checked for, and when.
    checked: Option<(u64, Instant)>,
    /// A check is under way (its thread).
    checking: bool,
}

static STATE: Mutex<State> = Mutex::new(State { shown: None, checked: None, checking: false });

fn lock() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}

/// The oldest prompt waiting from one of Yap's jobs (`is_job`). Pure.
pub fn oldest(pending: &[Pending], is_job: impl Fn(&str) -> bool) -> Option<&Pending> {
    pending.iter().filter(|p| is_job(&p.run)).min_by_key(|p| p.id)
}

/// A plan's first line, as the bar says it: its first words that aren't
/// blank, without markdown's marks ("## Watering" → "Watering"). Pure.
pub fn plan_line(plan: &str) -> String {
    let line = plan.lines().map(str::trim).find(|l| !l.trim_matches(|c: char| "#*-_>=` ".contains(c)).is_empty()).unwrap_or("");
    let line = line.trim_start_matches(|c: char| "#*->` ".contains(c));
    let line = line.trim_start_matches(|c: char| c.is_ascii_digit()).trim_start_matches(['.', ')']).trim();
    super::steps::clip(&line.replace("**", "").replace('`', ""), BODY_MAX)
}

/// The card for prompt `p`. Pure.
pub fn card(p: &Pending) -> crate::bar::Card {
    let place = if p.label.trim().is_empty() { "a job".to_string() } else { p.label.trim().to_string() };
    if p.tool == claude_live::EXIT_PLAN {
        // A plan: the person's go-ahead, or not yet.
        let line = plan_line(&p.detail);
        return crate::bar::Card {
            id: CARD.into(),
            icon: "job",
            title: format!("The plan is ready in {place}"),
            body: if line.is_empty() { "Go ahead with it?".into() } else { line },
            primary: Some(crate::bar::CardAction::new(format!("allow:{}", p.id), "Go ahead")),
            secondary: Some(crate::bar::CardAction::new(format!("deny:{}", p.id), "Not yet")),
            timeout_ms: None,
            ..Default::default()
        };
    }
    crate::bar::Card {
        id: CARD.into(),
        icon: "job",
        title: format!("Allow a step in {place}?"),
        body: super::steps::clip(p.summary.trim(), BODY_MAX),
        primary: Some(crate::bar::CardAction::new(format!("allow:{}", p.id), "Allow once")),
        secondary: Some(crate::bar::CardAction::new(format!("deny:{}", p.id), "Deny")),
        timeout_ms: None,
        ..Default::default()
    }
}

/// What a card's answer does: `allow:<id>` / `deny:<id>` → (id, allow).
/// Pure.
pub fn answer_of(action: &str) -> Option<(u64, bool)> {
    if let Some(id) = action.strip_prefix("allow:") {
        return id.parse().ok().map(|id| (id, true));
    }
    action.strip_prefix("deny:").and_then(|id| id.parse().ok()).map(|id| (id, false))
}

enum Do {
    Nothing,
    Dismiss,
    Check(Pending),
}

/// **The bar's card, in step with the prompts waiting.**
pub fn sync() {
    let Some(app) = super::runner::app().cloned() else { return };
    let waiting = claude_live::pending();
    let next = oldest(&waiting, |run| super::store::get(run).is_some()).cloned();
    let todo = {
        let mut st = lock();
        match next {
            None => match st.shown.take() {
                Some(_) => {
                    st.checked = None;
                    Do::Dismiss
                }
                None => Do::Nothing,
            },
            Some(p) if st.shown == Some(p.id) => Do::Nothing,
            Some(p) => {
                let fresh = st.checked.is_some_and(|(id, at)| id == p.id && at.elapsed() < RECHECK);
                if fresh || st.checking {
                    Do::Nothing
                } else {
                    st.checked = Some((p.id, Instant::now()));
                    st.checking = true;
                    Do::Check(p)
                }
            }
        }
    };
    match todo {
        Do::Nothing => {}
        Do::Dismiss => crate::bar::dismiss_card(&app, CARD),
        Do::Check(p) => {
            // Window getters wait on the main thread: off the job's task.
            let spawned = std::thread::Builder::new().name("jobs-approve-card".into()).spawn(move || {
                let up = !crate::notices::notice::yap_in_front(&app) && crate::bar::show_card(&app, card(&p), Box::new(on_action));
                let was = {
                    let mut st = lock();
                    st.checking = false;
                    let was = st.shown;
                    if up {
                        st.shown = Some(p.id);
                    }
                    was
                };
                if up {
                    tracing::info!(prompt = p.id, "Jobs: a permission prompt on the Yap bar");
                } else if was.is_some() {
                    // Another prompt's card was up and Yap is in front now.
                    lock().shown = None;
                    crate::bar::dismiss_card(&app, CARD);
                }
            });
            if spawned.is_err() {
                lock().checking = false;
            }
        }
    }
}

/// The card's Allow once / Deny.
fn on_action(_app: &tauri::AppHandle, action: &str) {
    let Some((id, allow)) = answer_of(action) else {
        // ✕: hidden; the prompt still waits in Chat.
        return;
    };
    claude_live::claude_approve_answer(id, allow);
    {
        let mut st = lock();
        if st.shown == Some(id) {
            st.shown = None;
        }
    }
    sync();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::claude_live::Origin;

    fn pending(id: u64, run: &str) -> Pending {
        Pending {
            id,
            run: run.into(),
            conversation: Some(1),
            origin: Origin::Typed,
            tool: "Bash".into(),
            summary: "Run a command: npm test".into(),
            detail: "npm test".into(),
            label: "garden-planner".into(),
            asked_ts: 0,
        }
    }

    #[test]
    fn the_oldest_prompt_of_a_job_is_the_one_on_the_bar() {
        let list = vec![pending(7, "job2"), pending(3, "chatrun"), pending(5, "job1")];
        let is_job = |r: &str| r.starts_with("job");
        assert_eq!(oldest(&list, is_job).map(|p| p.id), Some(5), "a run that isn't a job never gets the card");
        assert!(oldest(&[], is_job).is_none());
    }

    #[test]
    fn the_card_asks_in_plain_words_and_its_answers_read_back() {
        let c = card(&pending(9, "job1"));
        assert_eq!(c.id, CARD);
        assert_eq!(c.title, "Allow a step in garden-planner?");
        assert_eq!(c.body, "Run a command: npm test");
        assert_eq!(c.primary.as_ref().map(|a| (a.id.as_str(), a.label.as_str())), Some(("allow:9", "Allow once")));
        assert_eq!(c.secondary.as_ref().map(|a| (a.id.as_str(), a.label.as_str())), Some(("deny:9", "Deny")));
        assert!(c.timeout_ms.is_none(), "it waits for its answer");
        assert_eq!(answer_of("allow:9"), Some((9, true)));
        assert_eq!(answer_of("deny:12"), Some((12, false)));
        assert_eq!(answer_of(""), None);
        assert_eq!(answer_of("allow:x"), None);
        let unnamed = card(&Pending { label: " ".into(), ..pending(1, "job1") });
        assert_eq!(unnamed.title, "Allow a step in a job?");
    }

    #[test]
    fn a_plan_on_the_bar_says_its_ready_with_go_ahead_and_not_yet() {
        let plan = Pending { tool: "ExitPlanMode".into(), summary: "The plan is ready".into(), detail: "\n## Add a **watering** schedule\n\n1. Edit notes.md".into(), ..pending(4, "job1") };
        let c = card(&plan);
        assert_eq!(c.title, "The plan is ready in garden-planner");
        assert_eq!(c.body, "Add a watering schedule", "a short line: the plan's first, without its marks");
        assert_eq!(c.primary.as_ref().map(|a| (a.id.as_str(), a.label.as_str())), Some(("allow:4", "Go ahead")));
        assert_eq!(c.secondary.as_ref().map(|a| (a.id.as_str(), a.label.as_str())), Some(("deny:4", "Not yet")));
        assert!(c.timeout_ms.is_none());
        assert_eq!(answer_of("allow:4"), Some((4, true)), "Go ahead allows it");
        assert_eq!(plan_line("1. Read the beds file\n2. Add the schedule"), "Read the beds file");
        assert_eq!(card(&Pending { detail: String::new(), ..plan }).body, "Go ahead with it?");
    }
}

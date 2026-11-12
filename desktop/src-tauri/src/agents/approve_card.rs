//! **An agent's permission prompt on the Yap bar** (Yap Code's screens): a running
//! agent asks to take a step (the run endpoint's `approve`,
//! `integrations/claude_live/approve.rs`) and no Yap window is in front, so
//! nobody would see the card in Chat's thread: the bar shows it too —
//! "Allow a step in garden-planner?", the step in plain words, **Allow
//! once** / **Deny** (`claude_approve_answer`). Never answered by voice.
//! A plan (Plan mode's ExitPlanMode, 2026-11-07) reads as one: "The plan is
//! ready in garden-planner", its first line, **Go ahead** / **Not yet**
//! (the plan itself is in the thread's card).
//! A question of Claude Code's (`AskUserQuestion`, 12 Nov 2026) reads as
//! one too: "The agent has a question in garden-planner", the question, and
//! — one question of two options — the two options as its buttons;
//! otherwise **Answer in Yap** (the chat, where its card has every option)
//! / **Let it decide**.
//!
//! One card at a time (`agentApprove`, a question in `notices/notice.rs`
//! that waits out a dictation): the oldest prompt waiting. Answered here or
//! in Chat, timed out, or its agent gone, the card goes (or the next one takes
//! its place). Its ✕ only hides it: the prompt still waits in Chat.
//!
//! [`sync`] runs on each running agent's tick (half a second) and when a turn
//! ends; it's cheap when nothing changed, and asks whether a Yap window is
//! in front (a main-thread round trip) at most every [`RECHECK`] per prompt,
//! off the agent's task. The log has the prompt's id, never what it asks.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::integrations::claude_live::{self, Pending};

/// The card's id (its rule in `notices/notice.rs`).
pub const CARD: &str = "agentApprove";
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

/// The oldest prompt waiting from one of Yap's agents (`is_agent`). Pure.
pub fn oldest(pending: &[Pending], is_agent: impl Fn(&str) -> bool) -> Option<&Pending> {
    pending.iter().filter(|p| is_agent(&p.run)).min_by_key(|p| p.id)
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
    // "… in garden-planner": its workspace (none known: nothing said).
    let place = p.label.trim();
    let at = if place.is_empty() { String::new() } else { format!(" in {place}") };
    if p.tool == claude_live::EXIT_PLAN {
        // A plan: the person's go-ahead, or not yet.
        let line = plan_line(&p.detail);
        return crate::bar::Card {
            id: CARD.into(),
            icon: "agent",
            title: format!("The agent's plan is ready{at}"),
            body: if line.is_empty() { "Go ahead with it?".into() } else { line },
            primary: Some(crate::bar::CardAction::new(format!("allow:{}", p.id), "Go ahead")),
            secondary: Some(crate::bar::CardAction::new(format!("deny:{}", p.id), "Not yet")),
            timeout_ms: None,
            ..Default::default()
        };
    }
    if p.tool == claude_live::ASK {
        let body = super::steps::clip(p.summary.trim(), BODY_MAX);
        // One question, one answer of two: the bar's two buttons are them.
        let two = match p.questions.as_slice() {
            [q] if !q.multi_select && q.options.len() == 2 => Some((&q.options[0].label, &q.options[1].label)),
            _ => None,
        };
        let (primary, secondary) = match two {
            Some((a, b)) => (crate::bar::CardAction::new(format!("pick:{}:0", p.id), super::steps::clip(a, 28)), crate::bar::CardAction::new(format!("pick:{}:1", p.id), super::steps::clip(b, 28))),
            None => (crate::bar::CardAction::new(format!("open:{}", p.id), "Answer in Yap"), crate::bar::CardAction::new(format!("deny:{}", p.id), "Let it decide")),
        };
        return crate::bar::Card { id: CARD.into(), icon: "agent", title: format!("The agent has a question{at}"), body, primary: Some(primary), secondary: Some(secondary), timeout_ms: None, ..Default::default() };
    }
    crate::bar::Card {
        id: CARD.into(),
        icon: "agent",
        title: format!("Allow a step{at}?"),
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
            // Window getters wait on the main thread: off the agent's task.
            let spawned = std::thread::Builder::new().name("agents-approve-card".into()).spawn(move || {
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
                    tracing::info!(prompt = p.id, "Agents: a permission prompt on the Yap bar");
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

/// A question's own buttons: `pick:<id>:<n>` (its option `n`) and
/// `open:<id>` (answer it in Yap). Pure.
pub fn question_action(action: &str) -> Option<(u64, Option<usize>)> {
    if let Some(id) = action.strip_prefix("open:") {
        return id.parse().ok().map(|id| (id, None));
    }
    let (id, n) = action.strip_prefix("pick:")?.split_once(':')?;
    Some((id.parse().ok()?, Some(n.parse().ok()?)))
}

/// The card's Allow once / Deny (a question's: one of its two options,
/// Answer in Yap, Let it decide).
fn on_action(app: &tauri::AppHandle, action: &str) {
    if let Some((id, picked)) = question_action(action) {
        let Some(p) = claude_live::waiting(id) else { return };
        match picked {
            // One of its two options: its answer.
            Some(n) => {
                if let Some((q, o)) = p.questions.first().and_then(|q| q.options.get(n).map(|o| (q, o))) {
                    claude_live::claude_approve_reply(id, serde_json::json!({ q.question.clone(): o.label.clone() }));
                }
            }
            // Answer in Yap: its chat, where the card has every option.
            None => {
                crate::chat::turn::open_agent(app, &p.run);
                return;
            }
        }
        {
            let mut st = lock();
            if st.shown == Some(id) {
                st.shown = None;
            }
        }
        sync();
        return;
    }
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

    #[test]
    fn a_question_reads_as_one_with_its_two_options_as_the_buttons_or_answered_in_yap() {
        use crate::integrations::claude_live::{Choice, Question};
        let option = |label: &str| Choice { label: label.into(), description: String::new() };
        let question = |options: &[&str], multi_select: bool| Question { question: "Which bed should get the watering line first?".into(), header: "Bed".into(), options: options.iter().map(|o| option(o)).collect(), multi_select };
        let asks = |questions: Vec<Question>| Pending { tool: "AskUserQuestion".into(), summary: "Which bed should get the watering line first?".into(), detail: String::new(), questions, ..pending(7, "agent1") };
        // One question of two options: they're the bar's two buttons.
        let two = card(&asks(vec![question(&["North bed", "South bed"], false)]));
        assert_eq!((two.id.as_str(), two.title.as_str(), two.body.as_str()), (CARD, "The agent has a question in garden-planner", "Which bed should get the watering line first?"));
        assert_eq!(two.primary.as_ref().map(|a| (a.id.as_str(), a.label.as_str())), Some(("pick:7:0", "North bed")));
        assert_eq!(two.secondary.as_ref().map(|a| (a.id.as_str(), a.label.as_str())), Some(("pick:7:1", "South bed")));
        // More options, several to pick, or several questions: answered in Yap, or left to it.
        for many in [asks(vec![question(&["North bed", "South bed", "Both"], false)]), asks(vec![question(&["North bed", "South bed"], true)]), asks(vec![question(&["A", "B"], false), question(&["C", "D"], false)])] {
            let c = card(&many);
            assert_eq!(c.primary.as_ref().map(|a| (a.id.as_str(), a.label.as_str())), Some(("open:7", "Answer in Yap")));
            assert_eq!(c.secondary.as_ref().map(|a| (a.id.as_str(), a.label.as_str())), Some(("deny:7", "Let it decide")));
        }
        assert_eq!(question_action("pick:7:1"), Some((7, Some(1))));
        assert_eq!(question_action("open:7"), Some((7, None)));
        assert_eq!(question_action("allow:7"), None);
        assert_eq!(question_action("pick:7"), None);
        assert_eq!(answer_of("deny:7"), Some((7, false)), "Let it decide is the card's no");
    }

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
            questions: Vec::new(),
        }
    }

    #[test]
    fn the_oldest_prompt_of_an_agent_is_the_one_on_the_bar() {
        let list = vec![pending(7, "agent2"), pending(3, "chatrun"), pending(5, "agent1")];
        let is_agent = |r: &str| r.starts_with("agent");
        assert_eq!(oldest(&list, is_agent).map(|p| p.id), Some(5), "a run that isn't an agent never gets the card");
        assert!(oldest(&[], is_agent).is_none());
    }

    #[test]
    fn the_card_asks_in_plain_words_and_its_answers_read_back() {
        let c = card(&pending(9, "agent1"));
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
        let unnamed = card(&Pending { label: " ".into(), ..pending(1, "agent1") });
        assert_eq!(unnamed.title, "Allow a step?");
    }

    #[test]
    fn a_plan_on_the_bar_says_its_ready_with_go_ahead_and_not_yet() {
        let plan = Pending { tool: "ExitPlanMode".into(), summary: "The plan is ready".into(), detail: "\n## Add a **watering** schedule\n\n1. Edit notes.md".into(), ..pending(4, "agent1") };
        let c = card(&plan);
        assert_eq!(c.title, "The agent's plan is ready in garden-planner");
        assert_eq!(c.body, "Add a watering schedule", "a short line: the plan's first, without its marks");
        assert_eq!(c.primary.as_ref().map(|a| (a.id.as_str(), a.label.as_str())), Some(("allow:4", "Go ahead")));
        assert_eq!(c.secondary.as_ref().map(|a| (a.id.as_str(), a.label.as_str())), Some(("deny:4", "Not yet")));
        assert!(c.timeout_ms.is_none());
        assert_eq!(answer_of("allow:4"), Some((4, true)), "Go ahead allows it");
        assert_eq!(plan_line("1. Read the beds file\n2. Add the schedule"), "Read the beds file");
        assert_eq!(card(&Pending { detail: String::new(), ..plan }).body, "Go ahead with it?");
    }
}

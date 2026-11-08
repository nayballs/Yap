//! **Asked out loud before an agent starts** (the Voice Assistant): a card
//! nobody sees would just wait its two minutes, so Yap asks in words — "Should
//! I start an agent in garden-planner, in Auto mode? Say yes to start it." — and
//! the next question in that conversation, within [`ANSWER_WAIT`], is the
//! answer (as a Skill's question is, `assistant/skill.rs`): a plain yes
//! starts it, a no drops it, anything else drops it and is asked as usual.
//! Yap answers both itself (`chat/turn/fast.rs`), no model.
//!
//! Permission cards *while* an agent runs are never answered by voice: they
//! stay cards (plan §3.3).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::workspaces::Folder;

/// How long the question waits for its answer.
pub const ANSWER_WAIT: Duration = Duration::from_secs(120);

/// An agent waiting for the person's spoken yes.
#[derive(Debug, Clone, PartialEq)]
pub struct Waiting {
    pub conversation: u64,
    /// Not a workspace yet: added once they say yes.
    pub new_folder: bool,
    pub folder: Folder,
    /// The agent to start (or, with `follow_up`, the chat's agent to carry on).
    pub agent: super::store::Agent,
    /// A follow-up for the chat's agent: its next turn, once they say yes.
    pub follow_up: Option<super::runner::Ask>,
}

struct Slot {
    waiting: Waiting,
    at: Instant,
}

static WAITING: Mutex<Option<Slot>> = Mutex::new(None);

/// Yap asked out loud whether to start `waiting`.
pub fn wait(waiting: Waiting) {
    *WAITING.lock().unwrap_or_else(|p| p.into_inner()) = Some(Slot { waiting, at: Instant::now() });
}

/// What a question was, to the agent waiting in its conversation.
#[derive(Debug, Clone, PartialEq)]
pub enum Answer {
    Yes(Box<Waiting>),
    No(Box<Waiting>),
}

/// **The answer to Yap's question**, if `question` (to `conversation`) is
/// one: any question there takes the waiting agent out (a yes, a no, or
/// something else — then `None`, and it's asked as usual).
pub fn answered(conversation: Option<u64>, question: &str) -> Option<Answer> {
    let conversation = conversation?;
    let mut slot = WAITING.lock().unwrap_or_else(|p| p.into_inner());
    if slot.as_ref().is_none_or(|s| s.waiting.conversation != conversation) {
        return None;
    }
    let s = slot.take()?;
    if s.at.elapsed() > ANSWER_WAIT {
        return None;
    }
    let w = Box::new(s.waiting);
    if crate::apps::is_affirmative(question) {
        Some(Answer::Yes(w))
    } else if is_no(question) {
        Some(Answer::No(w))
    } else {
        None
    }
}

/// "No", "no thanks", "don't", "cancel that", "never mind". English.
pub fn is_no(text: &str) -> bool {
    let w = super::workspaces::words(text);
    if w.is_empty() || w.len() > 6 {
        return false;
    }
    const FIRST: &[&str] = &["no", "nope", "nah", "don", "dont", "cancel", "stop", "never", "not"];
    FIRST.contains(&w[0].as_str())
}

/// The question Yap asks out loud.
pub fn question(folder: &str, mode: &str, new_folder: bool) -> String {
    if new_folder {
        format!("Should I let {} work in {folder} and send an agent there, in {mode} mode? Say yes to send it.", super::FEATURE)
    } else {
        format!("Should I send an agent to {folder}, in {mode} mode? Say yes to send it.")
    }
}

/// The question Yap asks out loud before a follow-up for the chat's agent.
pub fn question_follow_up(folder: &str, mode: &str) -> String {
    format!("Should I pass that to the agent in {folder}, in {mode} mode? Say yes to send it.")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn waiting(conversation: u64) -> Waiting {
        Waiting {
            conversation,
            new_folder: true,
            folder: Folder { name: "garden-planner".into(), path: r"D:\Projects\garden-planner".into(), cap: None, added_ts: 0 },
            agent: super::super::store::Agent { id: "ab".into(), ..Default::default() },
            follow_up: None,
        }
    }

    #[test]
    fn the_next_question_there_answers_it_once() {
        wait(waiting(5));
        // Another conversation: not its answer, and it still waits.
        assert_eq!(answered(Some(6), "Yes."), None);
        assert!(matches!(answered(Some(5), "Yeah, go ahead."), Some(Answer::Yes(_))));
        assert_eq!(answered(Some(5), "Yes."), None, "taken");
        wait(waiting(5));
        assert!(matches!(answered(Some(5), "No thanks."), Some(Answer::No(_))));
        // Something else: dropped, asked as usual.
        wait(waiting(5));
        assert_eq!(answered(Some(5), "What's the weather like?"), None);
        assert_eq!(answered(Some(5), "Yes."), None);
        assert!(question("garden-planner", "Auto", false).contains("send an agent to garden-planner, in Auto mode"));
    }
}

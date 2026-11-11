//! **A Code chat that works directly** (11 Nov 2026), as Claude Code's own
//! app does: what the person types in a Code chat goes straight to the
//! chat's Claude Code session in its workspace, which does the work itself
//! and uses its own subagents when it judges it needs them. No model of
//! Chat's in between: before, Chat couldn't touch files, so every request
//! was worded into a brief for an agent ("On it: I've sent an agent…") and
//! what came of it was told second-hand.
//!
//! - **The session is what an agent always was** (`agents/runner.rs`): the
//!   person's own Claude Code with its own tools and setup, at the chat's
//!   Mode, its permission prompts Yap's cards, each file backed up before
//!   it's changed (Undo), a note while it works taken in at its next step.
//!   One a Code chat and workspace, each message its next turn on
//!   `--resume`. Nothing of that is new: only who talks to it.
//! - **The person's words, as they typed them**: the first message is its
//!   brief, each later one its next turn ([`send`]). So the rule that asks
//!   first after outside content has nothing to guard here — that rule is
//!   about a model wording a brief from what it read. What the session may
//!   do is the chat's Mode's business, with its cards below Bypass
//!   permissions (where nothing asks).
//! - **On the chat's own model**: the plan model and effort its chip shows,
//!   kept as the person's choice (`model_asked`), so it's the same from
//!   turn to turn (a session read afresh on another model costs its whole
//!   context again).
//! - **Its answer is its own**: when it stops, its final words are the
//!   chat's answer (`chat/turn/report.rs`), never a paraphrase.
//! - **When** ([`applies`]): Yap Code on with "Work directly in Code chats"
//!   (`agents_direct`, on by default), a Code chat whose model is on the
//!   person's Claude plan, a message typed (or dictated) in the Chat view
//!   with no screenshot, selection or Skill. Everything else is handed over
//!   as before, by Chat's model with `start_agent`: a chat on an API key or
//!   an on-device model, the Voice Assistant's spoken question (its answer
//!   is said, so it's kept short), a question from the phone.
//!
//! The log has ids and outcomes, never the words.

use crate::agents::modes;
use crate::agents::runner;
use crate::agents::store::{self, Agent, AgentRef, Status};
use crate::agents::workspaces::Folder;
use crate::ai::claude_code::session::{self, Effort};
use crate::integrations::claude_live::Origin;

/// What a turn is, as far as going directly is concerned.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Message {
    /// "Work directly in Code chats" (`agents_direct`).
    pub setting: bool,
    /// Yap Code is on and Claude Code is on this PC (`agents::offered`).
    pub offered: bool,
    /// The conversation is a Code chat with a workspace.
    pub code_chat: bool,
    /// Chat's model is on the person's Claude plan.
    pub on_plan: bool,
    /// Asked through the Voice Assistant's path (its shortcut, a phone).
    pub assistant: bool,
    /// It carries a screenshot, a selection, a page's passage or a Skill:
    /// Chat's own business.
    pub with_more: bool,
}

/// **Does this message go straight to the Code chat's session?** Pure.
pub fn applies(m: Message) -> bool {
    m.setting && m.offered && m.code_chat && m.on_plan && !m.assistant && !m.with_more
}

/// A message sent directly: its session.
#[derive(Debug, Clone)]
pub struct Sent {
    pub agent: Agent,
}

impl Sent {
    /// The agent as the page's tab opens it.
    pub fn reference(&self) -> AgentRef {
        self.agent.reference()
    }
}

/// **`text`, the person's message `message_id` in Code chat `conversation`,
/// sent to its session in `folder`**: its next turn (handed over at once
/// while it works: `runner::follow_up`), or — the chat's first there, or a
/// session Claude Code no longer has — a new one with it as its brief. On
/// `model` and `effort`, the chat's own.
pub fn send(conversation: u64, folder: &Folder, text: &str, message_id: String, by_voice: bool, model: &str, effort: Option<&str>) -> Result<Sent, String> {
    use crate::chat::tools::agents as tools;
    let cfg = crate::config::load();
    let effort = effort.map(str::to_string).filter(|e| Effort::parse(e).is_some());
    let prev = tools::chats_agent_in(Some(conversation), folder).filter(|j| tools::cant_resume(j).is_none());
    if let Some(prev) = prev {
        // A plain go-ahead to a plan waiting carries it out, as it always did.
        let go = crate::agents::consent::go_ahead(text) || (prev.plan && crate::apps::is_affirmative(text));
        let ask = runner::Ask {
            text: text.to_string(),
            message_id: Some(message_id),
            go,
            model: Some(model.to_string()).filter(|m| !m.trim().is_empty()),
            model_asked: !model.trim().is_empty(),
            effort_asked: effort.is_some(),
            effort,
            direct: true,
            ..Default::default()
        };
        let (agent, how) = runner::follow_up(&prev.id, ask)?;
        tracing::info!(id = %agent.id, how = ?how, "Code chat: its message went straight to its session");
        return Ok(Sent { agent });
    }
    let chip = modes::or_auto(&store::mode_of(Some(conversation), &cfg.agents_mode));
    let mode = modes::effective(chip, None, folder.cap_mode(), store::bypass_confirmed()).map_err(|r| r.for_model())?;
    let model = if model.trim().is_empty() { cfg.agents_model.clone() } else { model.to_string() };
    let effort = effort.and_then(|e| Effort::parse(&e)).or_else(|| Effort::parse(&cfg.agents_effort)).unwrap_or(Effort::Medium);
    let now = store::now_secs();
    let agent = Agent {
        id: store::new_id(),
        conversation: Some(conversation),
        message_id: Some(message_id.clone()),
        origin: if by_voice { Origin::Voice } else { Origin::Typed },
        folder: folder.name.clone(),
        path: folder.path.clone(),
        brief: text.to_string(),
        model: model.clone(),
        effort: effort.as_str().to_string(),
        // The chat's own model and effort: the person's choice, kept.
        model_asked: true,
        effort_asked: true,
        mode: mode.as_str().to_string(),
        pay: if cfg.agents_pay == "key" { "key".into() } else { "plan".into() },
        session: session::new_session_id(),
        status: Status::Queued,
        created_ts: now,
        updated_ts: now,
        turn_list: vec![store::TurnNote {
            n: 1,
            title: store::title_of(text),
            request: text.to_string(),
            message_id: Some(message_id),
            status: Status::Queued,
            mode: mode.as_str().to_string(),
            model,
            effort: effort.as_str().to_string(),
            direct: true,
            ..Default::default()
        }],
        ..Default::default()
    };
    let agent = tools::begin(agent, false)?;
    tracing::info!(id = %agent.id, mode = mode.as_str(), "Code chat: its first message started its session");
    Ok(Sent { agent })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_message_in_a_code_chat_on_the_plan_goes_straight_to_its_session() {
        let typed = Message { setting: true, offered: true, code_chat: true, on_plan: true, assistant: false, with_more: false };
        assert!(applies(typed));
        // The switch off, Yap Code off (or no Claude Code here): handed over as before.
        assert!(!applies(Message { setting: false, ..typed }));
        assert!(!applies(Message { offered: false, ..typed }));
        // A plain chat, or a Code chat on a key or an on-device model: Chat's own model answers.
        assert!(!applies(Message { code_chat: false, ..typed }));
        assert!(!applies(Message { on_plan: false, ..typed }));
        // Asked out loud or from the phone: its answer is said, so Chat keeps it short.
        assert!(!applies(Message { assistant: true, ..typed }));
        // With a screenshot, a selection or a Skill: Chat's own business.
        assert!(!applies(Message { with_more: true, ..typed }));
        assert!(!applies(Message::default()));
    }
}

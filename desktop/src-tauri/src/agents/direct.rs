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
//!   with no selection or Skill. Everything else is handed over
//!   as before, by Chat's model with `start_agent`: a chat on an API key or
//!   an on-device model, the Voice Assistant's question asked from another
//!   app (its model may look at the window in front, and answers there).
//! - **Asked out loud, or from a phone** (12 Nov 2026): the Voice
//!   Assistant's question asked with Chat in view, and a paired phone's
//!   question in a Code chat, go straight to the session too. Yap says "On
//!   it" ([`SENT_SAID`]; a phone reads [`SENT_SHOWN`]) — nothing of a
//!   model's — and the session's own final words are read aloud when it's
//!   done, as much of them as a spoken answer is (`chat/turn/report.rs`).
//!   A phone's turn runs at most at Auto, as ever ([`send`]'s `phone`).
//!   Before, both were handed over by Chat's model so that the spoken
//!   answer stayed short.
//! - **With its screenshots** (12 Nov 2026): the ones the person put in the
//!   box (snipped, pasted, dropped) go with their words, as Claude Code's
//!   own app sends a pasted picture — the turn keeps their ids
//!   (`TurnNote::shots`) and the runner writes the pictures with its
//!   message (`runner::shots_of`). Before, a message with a screenshot was
//!   Chat's own business: Chat's model read it and worded a brief for an
//!   agent, so a chat worked directly until a screenshot came, then didn't.
//!
//! The log has ids and outcomes, never the words.

use crate::agents::modes;
use crate::agents::runner;
use crate::agents::store::{self, Agent, AgentRef, Status};
use crate::agents::workspaces::Folder;
use crate::ai::claude_code::session::{self, Effort};
use crate::integrations::claude_live::Origin;

/// What Yap says out loud when a spoken question went straight to its
/// Code chat's session: Yap's own two words, never a model's.
pub const SENT_SAID: &str = "On it.";
/// What a phone reads in the answer's place then.
pub const SENT_SHOWN: &str = "On it. Its answer comes when it's done.";

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
    /// The Voice Assistant's question asked from another app (its shortcut,
    /// with Chat not in view): its model may look at the window in front
    /// and answers there, so Chat's model takes it. Asked with Chat in
    /// view, or from a phone, it goes straight to the session.
    pub away: bool,
    /// It carries a selection, a page's passage, a Skill, or a picture Yap
    /// took of the screen by itself: Chat's own business. (The person's own
    /// screenshots go with their words.)
    pub with_more: bool,
}

/// **Does this message go straight to the Code chat's session?** Pure.
pub fn applies(m: Message) -> bool {
    m.setting && m.offered && m.code_chat && m.on_plan && !m.away && !m.with_more
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
/// `model` and `effort`, the chat's own; `shots` the screenshots it came
/// with (their ids in `chat-images/`), written to the session with it.
/// `phone`: the paired phone that asked (Yap Link) — its turn runs at most
/// at Auto, never Bypass, whatever the chat's Mode on the PC, and is
/// reported on that phone (`agents/remote.rs`; the runner checks again as
/// the turn starts).
#[allow(clippy::too_many_arguments)]
pub fn send(conversation: u64, folder: &Folder, text: &str, shots: &[String], message_id: String, by_voice: bool, phone: Option<String>, model: &str, effort: Option<&str>) -> Result<Sent, String> {
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
            shots: shots.to_vec(),
            // A phone's words: a turn at most at Auto, the phone's.
            mode: phone.as_ref().map(|_| crate::agents::remote::phone_mode(crate::agents::remote::PHONE_MAX)),
            phone,
            ..Default::default()
        };
        let (agent, how) = runner::follow_up(&prev.id, ask)?;
        tracing::info!(id = %agent.id, how = ?how, "Code chat: its message went straight to its session");
        return Ok(Sent { agent });
    }
    let chip = modes::or_auto(&store::mode_of(Some(conversation), &cfg.agents_mode));
    let mode = modes::effective(chip, None, folder.cap_mode(), store::bypass_confirmed()).map_err(|r| r.for_model())?;
    // A phone's question: at most Auto, never Bypass.
    let mode = if phone.is_some() { crate::agents::remote::phone_mode(mode) } else { mode };
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
            shots: shots.to_vec(),
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
        let typed = Message { setting: true, offered: true, code_chat: true, on_plan: true, away: false, with_more: false };
        assert!(applies(typed));
        // The switch off, Yap Code off (or no Claude Code here): handed over as before.
        assert!(!applies(Message { setting: false, ..typed }));
        assert!(!applies(Message { offered: false, ..typed }));
        // A plain chat, or a Code chat on a key or an on-device model: Chat's own model answers.
        assert!(!applies(Message { code_chat: false, ..typed }));
        assert!(!applies(Message { on_plan: false, ..typed }));
        // Asked out loud from another app: Chat's model takes it (it may look at the window in front).
        // (Asked with Chat in view, or from a phone, it's `typed`: straight to the session.)
        assert!(!applies(Message { away: true, ..typed }));
        // With a selection, a Skill or a picture Yap took by itself: Chat's own business.
        assert!(!applies(Message { with_more: true, ..typed }));
        assert!(!applies(Message::default()));
    }
}

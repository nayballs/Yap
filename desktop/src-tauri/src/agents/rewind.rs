//! **Rewind and Fork from here** on the person's message in a Code chat that
//! works directly (14 Nov 2026; the maintainer, from the Claude Code app's
//! message actions: Copy, Rewind — unavailable while Claude works —, Fork
//! from here). Built from Anthropic's public docs (the Agent SDK's
//! `resumeSessionAt` and `forkSession`, the CLI's `--fork-session`) and the
//! app's behaviour as seen, never from the app's files.
//!
//! - **Rewind** to before a message that started one of Claude's turns: the
//!   chat goes back to before it (the message and everything after go; its
//!   words go back in the message box, the page's part), that turn and the
//!   ones after are marked rewound (kept for their backups and log, never
//!   shown), the files they changed are put back as they were, newest first,
//!   when the person asks (Yap's own backups, as Undo: `runner::undo_turn`),
//!   and Claude never sees those turns again: the next turn starts Claude
//!   Code's own fork of the session up to the end of the turn before
//!   (`--resume <id> --fork-session --resume-session-at <its last message>`,
//!   [`store::Branch`]), or a new session when it was the first (in a
//!   forked chat, the original's at the fork's point). Not while
//!   Claude works (the chat's session, `runner::queue_state`).
//! - **Fork from here** on such a message: a new Code chat in the same
//!   workspace holding the conversation up to and including that message's
//!   turn — its answer too —, its own session Claude Code's fork of the
//!   original at that turn's end (taken by its first turn: [`store::Fork`]).
//!   The original chat and its session are untouched; the files are the
//!   workspace's, shared (a fork branches the conversation, not the folder).
//!
//! **When Claude Code can't make the branch**: `--resume-session-at` is the
//! Agent SDK's documented `resumeSessionAt`, but the CLI reference doesn't
//! list the flag, so Yap asks the installed Claude Code once whether it
//! takes it (`cli::resumes_at`). Where it doesn't, where the session is no
//! longer on this PC, or where a branched run never starts (it refused the
//! point: `fall_back`), the next turn starts a new session whose first
//! message is the conversation kept so far as text ([`seed_of`], Anthropic's
//! own advice for a session that can't be resumed), then the person's words.
//! Never Claude Code's transcript files: their format is internal to it.
//!
//! A message Claude read while it worked (a note, joined to the turn it
//! came in) has neither: rewinding to it would cut a turn in two. A turn
//! from before Yap kept where turns end (`TurnNote::end_uuid`) can't be a
//! branch point: Rewind and Fork say so rather than guess.

use serde::Serialize;

use super::code_chat;
use super::runner;
use super::store::{self, Agent, Branch, Fork, TurnNote};

/// What a Rewind did.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Rewound {
    /// The message's own words, for the box.
    pub text: String,
    /// How many turns went.
    pub turns: usize,
    /// Files put back as they were, and files the turns made, deleted.
    pub restored: usize,
    pub deleted: usize,
    /// What couldn't be put back, in words (each turn's own reason).
    pub problems: Vec<String>,
}

/// The most of the conversation a seeded session gets as text (the latest
/// kept, each message cut to [`SEED_EACH`]).
const SEED_MAX: usize = 24_000;
const SEED_EACH: usize = 4_000;

/// **The conversation kept so far, as text** — a new session's first words
/// when Claude Code can't branch the old one: conversation `conversation`'s
/// messages before `upto` (the person's and the answers; Yap's own report
/// lines left out), the latest that fit. None when there are none.
pub(crate) fn seed_of(conversation: u64, upto: usize) -> Option<String> {
    let conv = crate::chat::chats::get(conversation)?;
    let lines: Vec<String> = conv.messages[..upto.min(conv.messages.len())]
        .iter()
        .filter(|m| m.kind.is_empty() && !m.text.trim().is_empty())
        .map(|m| {
            let text: String = m.text.trim().chars().take(SEED_EACH).collect();
            format!("{}: {text}", if m.role == "user" { "Me" } else { "You" })
        })
        .collect();
    seed_text(&lines)
}

/// The seed's words around its lines, the latest that fit [`SEED_MAX`]. Pure.
pub(crate) fn seed_text(lines: &[String]) -> Option<String> {
    let mut kept: Vec<&String> = Vec::new();
    let mut size = 0;
    for l in lines.iter().rev() {
        if size + l.len() > SEED_MAX && !kept.is_empty() {
            break;
        }
        size += l.len();
        kept.push(l);
    }
    if kept.is_empty() {
        return None;
    }
    kept.reverse();
    let body = kept.iter().map(|l| l.as_str()).collect::<Vec<_>>().join("\n\n");
    Some(format!("(Our conversation so far, as text: this chat was rewound or forked, and its session couldn't be resumed at that point.)\n\n{body}\n\n(My next message:)"))
}

/// **Can Claude Code branch `agent`'s session** at a message (the session
/// on this PC, and the flag taken: `cli::resumes_at`)?
fn can_branch(agent: &Agent) -> bool {
    !agent.session.is_empty() && crate::chat::tools::agents::cant_resume(agent).is_none() && crate::ai::claude_code::cli::find().is_some_and(|p| crate::ai::claude_code::cli::resumes_at(&p))
}

/// **A branched run Claude Code never started** (agent `id`'s turn `n`:
/// it refused the point): its next turn starts afresh, the conversation
/// before that turn's message as text, and the turn says so.
pub(crate) fn fall_back(id: &str, n: u32) {
    let Some(agent) = store::get(id) else { return };
    let Some(conversation) = agent.conversation else { return };
    let upto = agent.turn_list.iter().find(|t| t.n == n).and_then(|t| t.message_id.as_deref()).and_then(|m| index_of(m, conversation));
    let seed = upto.and_then(|i| seed_of(conversation, i));
    let why = "Claude Code couldn't pick the conversation up at that point. Send your message again: Claude starts afresh, with the conversation so far given to it as text.";
    if let Some(agent) = store::update(id, |j| {
        j.branch = Some(Branch { seed: seed.clone(), ..Default::default() });
        j.error = Some(why.into());
        if let Some(t) = j.turn_mut(n) {
            t.error = Some(why.into());
        }
    }) {
        tracing::info!(id, turn = n, seeded = seed.is_some(), "Code chat: a branch Claude Code didn't make; the next turn starts afresh");
        runner::told_changed(&agent, "branchFailed");
    }
}

/// The message id of conversation `conversation`'s message `index`.
fn message_id(conversation: u64, index: usize) -> String {
    format!("chat:{conversation}:{index}")
}

/// A message's index in its conversation ("chat:<conversation>:<index>").
fn index_of(id: &str, conversation: u64) -> Option<usize> {
    id.strip_prefix(&format!("chat:{conversation}:"))?.parse().ok()
}

/// **The turn message `index` of `conversation` started**: the chat's own
/// direct turn whose message it is, not rewound.
fn turn_at(conversation: u64, index: usize) -> Result<(Agent, TurnNote), String> {
    let want = message_id(conversation, index);
    store::of_conversation(conversation)
        .into_iter()
        .find_map(|a| {
            let t = a.turn_list.iter().find(|t| t.direct && !t.rewound && t.message_id.as_deref() == Some(want.as_str())).cloned()?;
            Some((a, t))
        })
        .ok_or_else(|| "Rewind and Fork work on a message that started one of Claude's turns in a Code chat.".to_string())
}

/// Where a session branched before turn `n` starts: the end of the latest
/// turn kept before it (`Some(None)`: none — a new session), or `None` when
/// that turn's end isn't known (from before Yap kept it). Pure.
pub(crate) fn point_before(turns: &[TurnNote], n: u32) -> Option<Option<String>> {
    match turns.iter().filter(|t| t.n < n && !t.rewound).max_by_key(|t| t.n) {
        None => Some(None),
        Some(t) => t.end_uuid.clone().filter(|u| !u.is_empty()).map(Some),
    }
}

/// The turns a Rewind to turn `n` takes back: `n` and every later one kept,
/// newest first. Pure.
pub(crate) fn rewound_from(turns: &[TurnNote], n: u32) -> Vec<u32> {
    let mut gone: Vec<u32> = turns.iter().filter(|t| t.n >= n && !t.rewound).map(|t| t.n).collect();
    gone.sort_unstable_by(|a, b| b.cmp(a));
    gone
}

/// **Rewind** conversation `conversation` to before its message `index`
/// (the module's doc); `files`: the files the turns changed go back too.
pub fn rewind(conversation: u64, index: usize, files: bool) -> Result<Rewound, String> {
    let (agent, turn) = turn_at(conversation, index)?;
    if agent.status.live() || runner::queue_state(conversation) != "idle" {
        return Err("Claude is working: rewind once it's done, or stop it first.".into());
    }
    let at = point_before(&agent.turn_list, turn.n).ok_or("Yap can't rewind to here: the turn before it is from before Yap kept where each turn ends. Rewind works from the messages sent since.")?;
    // Before its first turn: a new session — or, in a chat forked from another, the original's at the fork.
    // Where Claude Code can't branch it: a new session with the conversation kept so far as text.
    let branch = match (&at, &agent.forked) {
        (None, None) => Branch::default(),
        (None, Some(f)) if f.seed.is_none() && !f.session.is_empty() && can_branch(&Agent { session: f.session.clone(), ..agent.clone() }) => Branch { at: Some(f.at.clone()), from: Some(f.session.clone()), seed: None },
        (Some(at), _) if can_branch(&agent) => Branch { at: Some(at.clone()), ..Default::default() },
        _ => Branch { seed: seed_of(conversation, index), ..Default::default() },
    };
    // (The message is still there to cut at.)
    crate::chat::chats::get(conversation).and_then(|c| c.messages.get(index).filter(|m| m.role == "user" && m.kind.is_empty()).cloned()).ok_or("That message isn't there any more.")?;
    let gone = rewound_from(&agent.turn_list, turn.n);
    let mut out = Rewound { turns: gone.len(), ..Default::default() };
    // The files first, newest turn first (each put back to before it), as Undo does.
    if files {
        for n in &gone {
            let t = agent.turn_list.iter().find(|t| t.n == *n);
            if t.is_none_or(|t| t.undone || t.files.is_empty()) {
                continue;
            }
            match runner::undo_turn(&agent.id, *n) {
                Ok(r) => {
                    out.restored += r.restored.len();
                    out.deleted += r.deleted.len();
                    if !r.failed.is_empty() {
                        out.problems.push(format!("{} file{} couldn't be put back.", r.failed.len(), if r.failed.len() == 1 { "" } else { "s" }));
                    }
                }
                Err(e) => out.problems.push(e),
            }
        }
    }
    let message = crate::chat::chats::rewind(conversation, index)?;
    out.text = message.text;
    let Some(agent) = store::update(&agent.id, |j| {
        for t in j.turn_list.iter_mut() {
            if gone.contains(&t.n) {
                t.rewound = true;
            }
        }
        j.branch = Some(branch.clone());
        // Nothing of those turns waits on anyone now.
        j.plan = false;
        j.plan_allowed = false;
        j.asks = false;
        if j.report_message_id.as_deref().and_then(|m| index_of(m, conversation)).is_some_and(|i| i >= index) {
            j.report_message_id = None;
        }
    }) else {
        return Err("The chat's agent isn't there any more.".into());
    };
    store::saved().map_err(|e| e.to_string())?;
    tracing::info!(id = %agent.id, conversation, turn = turn.n, turns = out.turns, files, restored = out.restored, deleted = out.deleted, fresh = branch.at.is_none(), seeded = branch.seed.is_some(), "Code chat: rewound");
    runner::told_changed(&agent, "rewound");
    Ok(out)
}

/// **Fork from here**: a new Code chat holding conversation
/// `conversation` up to and including its message `index`'s turn (the
/// module's doc). Its id.
pub fn fork(conversation: u64, index: usize) -> Result<u64, String> {
    let (agent, turn) = turn_at(conversation, index)?;
    if turn.status.live() {
        return Err("Claude is still on that message: fork from here once it's done.".into());
    }
    let at = turn.end_uuid.clone().filter(|u| !u.is_empty()).ok_or("Yap can't fork from here: that message is from before Yap kept where each turn ends. Fork works from the messages sent since.")?;
    // Claude Code's own fork when it can make one; else the fork's first turn starts afresh with the conversation as text.
    let precise = can_branch(&agent);
    let conv = crate::chat::chats::get(conversation).ok_or("That chat isn't there any more.")?;
    // Up to the next turn's message (this turn's answer included), else the whole chat.
    let next = store::of_conversation(conversation)
        .iter()
        .flat_map(|a| a.turn_list.iter())
        .filter(|t| t.direct && !t.rewound)
        .filter_map(|t| t.message_id.as_deref().and_then(|m| index_of(m, conversation)))
        .filter(|i| *i > index)
        .min();
    let upto = next.unwrap_or(conv.messages.len());
    let title = format!("{} (fork)", if conv.title.trim().is_empty() { "Untitled" } else { conv.title.trim() });
    let new = crate::chat::chats::fork(conversation, upto, &title)?;
    // The same workspace and Mode; its first turn branches the session.
    let folder = code_chat::folder_of(Some(conversation)).or_else(|| code_chat::folder_at(&agent.path));
    code_chat::set(new.id, folder.as_ref());
    if let Some(mode) = store::modes_of(conversation) {
        store::set_mode(new.id, &mode);
    }
    let seed = if precise { None } else { seed_of(new.id, usize::MAX) };
    store::set_fork(new.id, Fork { session: if precise { agent.session.clone() } else { String::new() }, at, from: conversation, seed });
    store::saved().map_err(|e| e.to_string())?;
    tracing::info!(id = %agent.id, conversation, fork = new.id, turn = turn.n, messages = upto, precise, "Code chat: forked from here");
    Ok(new.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(n: u32, end: Option<&str>) -> TurnNote {
        TurnNote { n, end_uuid: end.map(str::to_string), direct: true, ..Default::default() }
    }

    #[test]
    fn a_rewind_branches_at_the_end_of_the_turn_before_or_starts_afresh() {
        let turns = vec![turn(1, Some("u1")), turn(2, Some("u2")), turn(3, Some("u3"))];
        // To before turn 3: the session as it stood at turn 2's end.
        assert_eq!(point_before(&turns, 3), Some(Some("u2".into())));
        // To before the first: a new session.
        assert_eq!(point_before(&turns, 1), Some(None));
        // A turn rewound before is no branch point: the one kept before it is.
        let mut skipped = turns.clone();
        skipped[1].rewound = true;
        assert_eq!(point_before(&skipped, 3), Some(Some("u1".into())));
        // The turn before is from before Yap kept where turns end: no guess.
        let old = vec![turn(1, None), turn(2, Some("u2"))];
        assert_eq!(point_before(&old, 2), None);
    }

    #[test]
    fn a_rewind_takes_back_its_turn_and_every_later_one_kept_newest_first() {
        let mut turns = vec![turn(1, Some("u1")), turn(2, Some("u2")), turn(3, Some("u3")), turn(4, Some("u4"))];
        turns[2].rewound = true;
        assert_eq!(rewound_from(&turns, 2), [4, 2]);
        assert_eq!(rewound_from(&turns, 5), Vec::<u32>::new());
    }

    #[test]
    fn a_seeded_session_gets_the_latest_of_the_conversation_as_text() {
        let lines: Vec<String> = vec!["Me: the code word is PELICAN".into(), "You: OK".into()];
        let seed = seed_text(&lines).unwrap();
        assert!(seed.contains("Me: the code word is PELICAN\n\nYou: OK"));
        assert!(seed.ends_with("(My next message:)"));
        assert_eq!(seed_text(&[]), None);
        // Too long: the latest that fit, never cut mid-message.
        let long: Vec<String> = (0..20).map(|i| format!("Me: {i} {}", "x".repeat(3_000))).collect();
        let seed = seed_text(&long).unwrap();
        assert!(seed.contains("Me: 19 ") && !seed.contains("Me: 0 "));
        assert!(seed.len() < SEED_MAX + 1_000);
    }

    #[test]
    fn a_message_id_is_its_index_in_its_own_chat() {
        assert_eq!(index_of(&message_id(7, 12), 7), Some(12));
        assert_eq!(index_of("chat:70:12", 7), None);
        assert_eq!(index_of("chat:7:x", 7), None);
    }
}

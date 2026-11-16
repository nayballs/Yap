//! **Chat's conversations on the phone** (2026-11-03, the Claude-app
//! layout): what the phone's drawer lists (`chats` → [`recents`]) and what
//! a conversation it opens shows (`open_chat` → [`thread`]). Recents are
//! Chat's own list — not archived, not a meeting's Ask thread (`note`),
//! newest first — and a thread is its messages' words only, bounded: the
//! last [`KEEP_MESSAGES`], each cut at [`MAX_CHARS`]. A question's
//! screenshot text sent in its picture's place (`snip::ocr`) is left out,
//! as Chat folds it. Since 16 Nov 2026 a message carries its versions'
//! ‹ k/n › ([`version_of`]), an answer whether ↻ may ask it again from
//! the phone ([`retryable`]) and a question whether ✎ may edit it there,
//! by its time in Chat ([`editable`]). **One version shown a conversation**: the
//! PC's (its `messages`), on every screen — a phone's ‹ › switches it there
//! too, and every screen follows (`link.md`, Retry and versions). Pure.

use crate::chat::chats::Conversation;
use crate::link::wire::{ChatItem, ThreadMessage, Version};

/// The most conversations the drawer lists.
pub const MAX_RECENTS: usize = 200;
/// The most messages of a conversation the phone gets (the latest).
pub const KEEP_MESSAGES: usize = 40;
/// The most characters of one message the phone gets.
pub const MAX_CHARS: usize = 6_000;
/// A conversation's title when it has none.
pub const UNTITLED: &str = "New chat";

/// Can a phone continue conversation `c`: a Chat conversation (not a
/// meeting's Ask thread), not archived. Pure.
pub fn usable(c: &Conversation) -> bool {
    c.note.is_none() && c.archived_ts == 0
}

/// Its title, or [`UNTITLED`]. Pure.
pub fn title(c: &Conversation) -> String {
    let t = c.title.trim();
    if t.is_empty() {
        UNTITLED.to_string()
    } else {
        t.to_string()
    }
}

/// The drawer's Recents: Chat's conversations a phone may open, newest
/// first, [`MAX_RECENTS`] at most. Pure.
pub fn recents(all: &[Conversation]) -> Vec<ChatItem> {
    let mut list: Vec<&Conversation> = all.iter().filter(|c| usable(c)).collect();
    list.sort_by_key(|c| (std::cmp::Reverse(c.updated_ts), std::cmp::Reverse(c.id)));
    list.into_iter().take(MAX_RECENTS).map(|c| ChatItem { id: c.id, title: title(c), ts: c.updated_ts, code: None }).collect()
}

/// `text`, at most [`MAX_CHARS`] of it (then "…"). Pure.
fn capped(text: &str) -> String {
    if text.chars().count() <= MAX_CHARS {
        return text.to_string();
    }
    let mut out: String = text.chars().take(MAX_CHARS).collect();
    out.push('…');
    out
}

/// The most shared files' names one message lists on the phone.
pub const MAX_FILES: usize = 24;

/// The names of the files a message shared (`chat/shares.rs`), at most
/// [`MAX_FILES`]. Pure.
fn shared_names(m: &crate::chat::chats::ChatMessage) -> Vec<String> {
    m.files.iter().flat_map(|s| s.items.iter()).map(|i| i.name.trim().to_string()).filter(|n| !n.is_empty()).take(MAX_FILES).collect()
}

/// **Message `index` of `c` as the phone shows it**: a question's own words
/// (an agent's report marker keeps its `kind`, so it's never shown as the
/// person's bubble: 2026-11-08), an answer's with its id in Chat for ▶ and
/// its 👍 / 👎; the names of the files either shared (the files stay on the
/// PC); its versions' ‹ k/n › (16 Nov 2026: Retry's and Edit's, the version
/// shown being the PC's — one a conversation, [`version_of`]). Pure.
pub fn message(c: &Conversation, index: usize) -> Option<ThreadMessage> {
    let m = c.messages.get(index)?;
    let version = version_of(c, index);
    match m.role.as_str() {
        "user" => Some(ThreadMessage {
            role: "user",
            text: capped(crate::snip::ocr::question_of(&m.text).trim()),
            id: None,
            voice: m.voice,
            pictures: m.images.len(),
            kind: m.kind.clone(),
            files: shared_names(m),
            rating: None,
            version,
            retry: false,
            // (✎ and its time: `thread`, which knows whether it's a Code chat.)
            edit: 0,
        }),
        "assistant" => Some(ThreadMessage {
            role: "assistant",
            text: capped(m.text.trim()),
            id: Some(format!("chat:{}:{index}", c.id)),
            files: shared_names(m),
            // Its 👍 / 👎 (chat/feedback.rs), as the desktop's thumbs show it.
            rating: m.feedback.as_ref().map(|f| f.rating.clone()),
            version,
            ..Default::default()
        }),
        _ => None,
    }
}

/// **Message `index`'s versions** (`chat/chats/branches.rs`), as ‹ shown /
/// count › says them: the conversation's own `messages` are the version
/// shown, on the PC and every phone alike. Pure.
pub fn version_of(c: &Conversation, index: usize) -> Option<Version> {
    crate::chat::chats::branches::views(c).into_iter().find(|f| f.at == index).map(|f| Version { at: f.at, shown: f.shown, count: f.count })
}

/// **Can ↻ ask answer `index` again from a phone**: an answer just after the
/// person's own question (what `chats::begin_retry` takes), in a chat that
/// isn't a Code chat (`code`: there Retry is Claude Code's Rewind, the PC's
/// only). Pure.
pub fn retryable(c: &Conversation, index: usize, code: bool) -> bool {
    let answer = c.messages.get(index).is_some_and(|m| m.role == "assistant" && m.kind.is_empty());
    let question = index.checked_sub(1).and_then(|i| c.messages.get(i)).is_some_and(|q| q.role == "user" && q.kind.is_empty());
    !code && answer && question
}

/// **Can ✎ edit question `index` from a phone** (16 Nov 2026): the person's
/// own (what `chats::begin_edit` takes), in a chat that isn't a Code chat
/// (`code`: there Edit is Claude Code's Rewind, the PC's only), with
/// nothing the phone can't send again — no passage it was asked about, no
/// files from Chat's box (the phone's question carries neither) — and its
/// words whole on the phone: no longer than a typed question from it
/// (`session::MAX_TYPED`). Its pictures still on the PC go again. Pure.
pub fn editable(c: &Conversation, index: usize, code: bool) -> bool {
    let Some(m) = c.messages.get(index) else { return false };
    let words = crate::snip::ocr::question_of(&m.text);
    !code
        && m.role == "user"
        && m.kind.is_empty()
        && m.quote.is_none()
        && m.attachments.is_empty()
        && !words.trim().is_empty()
        && words.trim().chars().count() <= crate::link::session::MAX_TYPED
}

/// **A conversation as the phone shows it**: its title, its latest
/// messages' words ([`message`]), each answer ↻ may ask again
/// ([`retryable`]) and each question ✎ may edit, with its time in Chat
/// ([`editable`]; `code`: a Code chat, neither), and whether older ones
/// were left out. Pure.
pub fn thread(c: &Conversation, code: bool) -> (String, Vec<ThreadMessage>, bool) {
    let start = c.messages.len().saturating_sub(KEEP_MESSAGES);
    let messages = (start..c.messages.len())
        .filter_map(|i| {
            message(c, i).map(|m| {
                // (✎ names a question by its time in Chat: none, no ✎.)
                let edit = if m.role == "user" && editable(c, i, code) { c.messages[i].ts } else { 0 };
                ThreadMessage { retry: m.role == "assistant" && retryable(c, i, code), edit, ..m }
            })
        })
        .collect();
    (title(c), messages, start > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::chats::{ChatImage, ChatMessage};

    fn conv(id: u64, title: &str, updated: u64) -> Conversation {
        Conversation {
            id,
            title: title.into(),
            created_ts: updated,
            updated_ts: updated,
            messages: Vec::new(),
            scope: String::new(),
            note: None,
            archived_ts: 0,
            summary: None,
            closed_tabs: Vec::new(),
            branches: Vec::new(),
            forks: Vec::new(),
            incognito: false,
        }
    }

    fn said(role: &str, text: &str) -> ChatMessage {
        ChatMessage { role: role.into(), text: text.into(), ..Default::default() }
    }

    #[test]
    fn recents_are_chats_own_list_newest_first() {
        let mut archived = conv(4, "Old recipes", 900);
        archived.archived_ts = 950;
        let mut meeting = conv(5, "Standup questions", 990);
        meeting.note = Some(77);
        let all = vec![conv(1, "Trip packing list", 100), conv(2, "  ", 500), conv(3, "Bike repair", 300), archived, meeting];
        let items = recents(&all);
        assert_eq!(items.iter().map(|i| i.id).collect::<Vec<_>>(), vec![2, 3, 1], "newest first; archived and meeting threads left out");
        assert_eq!(items[0].title, UNTITLED, "an empty title");
        assert_eq!(items[1], ChatItem { id: 3, title: "Bike repair".into(), ts: 300, code: None });
        // Bounded.
        let many: Vec<Conversation> = (1..=MAX_RECENTS as u64 + 20).map(|i| conv(i, "Chat", i)).collect();
        assert_eq!(recents(&many).len(), MAX_RECENTS);
        assert_eq!(recents(&many)[0].id, MAX_RECENTS as u64 + 20);
    }

    #[test]
    fn a_thread_is_its_latest_words_with_answer_ids() {
        let mut c = conv(7, "Garden plan", 100);
        let mut q = said("user", "What grows in shade?\n\n[Text read from the screenshot on this PC, sent instead of the picture]\nFerns 4.99");
        q.voice = true;
        q.images = vec![ChatImage { id: "a".into(), ..Default::default() }, ChatImage { id: "b".into(), ..Default::default() }];
        c.messages = vec![q, said("assistant", "Ferns and hostas."), said("system", "ignored"), said("user", "And sun?"), said("assistant", "Lavender.")];
        let (title, messages, more) = thread(&c, false);
        assert_eq!(title, "Garden plan");
        assert!(!more);
        assert_eq!(messages.len(), 4, "only questions and answers");
        assert_eq!(
            messages[0],
            ThreadMessage { role: "user", text: "What grows in shade?".into(), id: None, voice: true, pictures: 2, kind: String::new(), files: Vec::new(), rating: None, version: None, retry: false, edit: 0 }
        );
        assert_eq!(messages[1].id.as_deref(), Some("chat:7:1"), "its place in Chat");
        assert_eq!(messages[3].id.as_deref(), Some("chat:7:4"));
        // An answer's 👍 / 👎 goes with it (chat/feedback.rs); none, nothing.
        c.messages[4].feedback = Some(crate::chat::feedback::Feedback { rating: "bad".into(), reasons: vec!["long".into()], note: String::new(), ts: 1 });
        let (_, rated, _) = thread(&c, false);
        assert_eq!((rated[1].rating.as_deref(), rated[3].rating.as_deref()), (None, Some("bad")));
        assert_eq!(serde_json::to_value(&rated[3]).unwrap()["rating"], "bad");
        assert!(serde_json::to_value(&rated[1]).unwrap().get("rating").is_none());
        // The latest only, each message capped.
        c.messages = (0..KEEP_MESSAGES + 6).map(|i| said(if i % 2 == 0 { "user" } else { "assistant" }, &format!("m{i}"))).collect();
        c.messages.push(said("assistant", &"x".repeat(MAX_CHARS + 50)));
        let (_, messages, more) = thread(&c, false);
        assert!(more);
        assert_eq!(messages.len(), KEEP_MESSAGES);
        assert_eq!(messages[0].text, "m7");
        let last = messages.last().unwrap();
        assert_eq!(last.text.chars().count(), MAX_CHARS + 1);
        assert!(last.text.ends_with('…'));
        assert_eq!(last.id.as_deref(), Some(format!("chat:7:{}", KEEP_MESSAGES + 6).as_str()));
    }

    #[test]
    fn an_agents_report_is_marked_as_one_never_the_persons_own_bubble() {
        let mut c = conv(12, "Watering schedule", 100);
        let marker = ChatMessage { role: "user".into(), text: "The agent in garden-planner: it's done.".into(), kind: crate::chat::turn::AGENT_REPORT.into(), ..Default::default() };
        c.messages = vec![said("user", "Add a watering schedule"), said("assistant", "On it."), marker, said("assistant", "Done: I added it.")];
        let (_, messages, _) = thread(&c, false);
        assert_eq!(messages.iter().map(|m| (m.role, m.kind.as_str())).collect::<Vec<_>>(), [("user", ""), ("assistant", ""), ("user", "agentReport"), ("assistant", "")]);
        let v = serde_json::to_value(&messages[2]).unwrap();
        assert_eq!(v["kind"], "agentReport");
        assert!(serde_json::to_value(&messages[0]).unwrap().get("kind").is_none(), "the person's own: no kind");
        // One message alone, as a watched chat hears it.
        assert_eq!(message(&c, 2), Some(messages[2].clone()));
        assert_eq!(message(&c, 3).and_then(|m| m.id).as_deref(), Some("chat:12:3"));
        assert_eq!(message(&c, 9), None);
    }

    #[test]
    fn a_threads_messages_carry_their_versions_and_which_answers_retry() {
        let mut c = conv(41, "Trip packing list", 100);
        let msg = |role: &str, text: &str, ts: u64| ChatMessage { role: role.into(), text: text.into(), ts, ..Default::default() };
        c.messages = vec![msg("user", "What to pack?", 1), msg("assistant", "A coat.", 2), msg("user", "And shoes?", 3), msg("assistant", "Boots.", 4)];
        // Nothing retried yet: no ‹ ›, and ↻ under each answer after the person's question.
        let (_, messages, _) = thread(&c, false);
        assert!(messages.iter().all(|m| m.version.is_none()));
        assert_eq!(messages.iter().map(|m| m.retry).collect::<Vec<_>>(), [false, true, false, true]);
        // The first answer retried twice (Retry: chats::begin_retry), the new ones shown.
        for (text, ts) in [("A rain jacket.", 5), ("Layers.", 6)] {
            crate::chat::chats::branches::branch_off(&mut c, 1).unwrap();
            c.messages.push(msg("assistant", text, ts));
            crate::chat::chats::branches::settle(&mut c, 1);
        }
        let (_, messages, _) = thread(&c, false);
        assert_eq!(messages.len(), 2, "the version shown only: what followed the first answer went with it");
        assert_eq!(messages[1].text, "Layers.");
        assert_eq!(messages[1].version, Some(Version { at: 1, shown: 3, count: 3 }));
        assert_eq!(version_of(&c, 0), None);
        // An Edit of the question: ‹ › on the person's own message.
        crate::chat::chats::branches::branch_off(&mut c, 0).unwrap();
        c.messages.push(msg("user", "What to pack for snow?", 7));
        c.messages.push(msg("assistant", "Crampons.", 8));
        let (_, messages, _) = thread(&c, false);
        assert_eq!(messages[0].version, Some(Version { at: 0, shown: 2, count: 2 }));
        assert_eq!(messages[1].version, None, "the new branch's answer has none of its own");
        // Switched back (‹): the first question, its answer at version 3 of 3, as before.
        crate::chat::chats::branches::switch(&mut c, 0, 1).unwrap();
        let (_, messages, _) = thread(&c, false);
        assert_eq!(messages[0].version, Some(Version { at: 0, shown: 1, count: 2 }));
        assert_eq!(messages[1].version, Some(Version { at: 1, shown: 3, count: 3 }));
        // In a Code chat no answer retries from the phone (Retry there is Claude Code's Rewind).
        let (_, messages, _) = thread(&c, true);
        assert!(messages.iter().all(|m| !m.retry));
        // Nor an answer after an agent's report, nor one after another answer.
        let mut r = conv(12, "Watering schedule", 100);
        let marker = ChatMessage { role: "user".into(), text: "The agent: done.".into(), kind: crate::chat::turn::AGENT_REPORT.into(), ..Default::default() };
        r.messages = vec![said("user", "Add a schedule"), said("assistant", "On it."), said("assistant", "Still on it."), marker, said("assistant", "Done.")];
        assert_eq!((0..5).map(|i| retryable(&r, i, false)).collect::<Vec<_>>(), [false, true, false, false, false]);
        assert!(!retryable(&r, 9, false));
    }

    #[test]
    fn a_threads_questions_say_which_the_phone_may_edit_and_their_time() {
        let mut c = conv(41, "Trip packing list", 100);
        let msg = |role: &str, text: &str, ts: u64| ChatMessage { role: role.into(), text: text.into(), ts, ..Default::default() };
        let marker = ChatMessage { role: "user".into(), text: "The agent: done.".into(), kind: crate::chat::turn::AGENT_REPORT.into(), ts: 5, ..Default::default() };
        c.messages = vec![msg("user", "What to pack?", 1), msg("assistant", "A coat.", 2), msg("user", "And shoes?", 3), msg("assistant", "Boots.", 4), marker, msg("assistant", "Done.", 6)];
        // The person's own questions, each with its time in Chat; never an answer or an agent's report.
        let (_, messages, _) = thread(&c, false);
        assert_eq!(messages.iter().map(|m| m.edit).collect::<Vec<_>>(), [1, 0, 3, 0, 0, 0]);
        // In a Code chat none (Edit there is Claude Code's Rewind).
        let (_, messages, _) = thread(&c, true);
        assert!(messages.iter().all(|m| m.edit == 0));
        // Not one asked about a passage, nor one with files from Chat's box: the phone can't send those again.
        c.messages[0].quote = Some(crate::chat::documents::Quote { note_id: 3, text: "Day two: the ridge walk".into(), app: String::new() });
        c.messages[2].attachments = vec![crate::chat::attachments::Attachment::default()];
        assert!(!editable(&c, 0, false) && !editable(&c, 2, false));
        // Nor one longer than the phone may send (its words, the screenshot's text left out).
        let long = "w".repeat(crate::link::session::MAX_TYPED + 1);
        c.messages[0] = msg("user", &long, 1);
        assert!(!editable(&c, 0, false));
        let fits = format!("{}\n\n[Text read from the screenshot on this PC, sent instead of the picture]\n{}", "w".repeat(crate::link::session::MAX_TYPED), "x".repeat(900));
        c.messages[0] = msg("user", &fits, 1);
        assert!(editable(&c, 0, false), "the screenshot's text isn't the person's words");
        // Nothing there, or an empty question: no.
        assert!(!editable(&c, 99, false));
        c.messages[0] = msg("user", "  ", 1);
        assert!(!editable(&c, 0, false));
    }

    #[test]
    fn a_phone_continues_only_chats_own_unarchived_conversations() {
        assert!(usable(&conv(1, "a", 1)));
        let mut archived = conv(2, "b", 1);
        archived.archived_ts = 5;
        assert!(!usable(&archived));
        let mut meeting = conv(3, "c", 1);
        meeting.note = Some(9);
        assert!(!usable(&meeting));
    }
}

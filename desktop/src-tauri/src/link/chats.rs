//! **Chat's conversations on the phone** (2026-11-03, the Claude-app
//! layout): what the phone's drawer lists (`chats` → [`recents`]) and what
//! a conversation it opens shows (`open_chat` → [`thread`]). Recents are
//! Chat's own list — not archived, not a meeting's Ask thread (`note`),
//! newest first — and a thread is its messages' words only, bounded: the
//! last [`KEEP_MESSAGES`], each cut at [`MAX_CHARS`]. A question's
//! screenshot text sent in its picture's place (`snip::ocr`) is left out,
//! as Chat folds it. Pure.

use crate::chat::chats::Conversation;
use crate::link::wire::{ChatItem, ThreadMessage};

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
    list.into_iter().take(MAX_RECENTS).map(|c| ChatItem { id: c.id, title: title(c), ts: c.updated_ts }).collect()
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

/// **A conversation as the phone shows it**: its title, its latest
/// messages' words (a question's own words, an answer's with its id in
/// Chat for ▶) and whether older ones were left out. Pure.
pub fn thread(c: &Conversation) -> (String, Vec<ThreadMessage>, bool) {
    let start = c.messages.len().saturating_sub(KEEP_MESSAGES);
    let messages = c.messages[start..]
        .iter()
        .enumerate()
        .filter_map(|(i, m)| {
            let index = start + i;
            match m.role.as_str() {
                "user" => Some(ThreadMessage {
                    role: "user",
                    text: capped(crate::snip::ocr::question_of(&m.text).trim()),
                    id: None,
                    voice: m.voice,
                    pictures: m.images.len(),
                }),
                "assistant" => Some(ThreadMessage {
                    role: "assistant",
                    text: capped(m.text.trim()),
                    id: Some(format!("chat:{}:{index}", c.id)),
                    voice: false,
                    pictures: 0,
                }),
                _ => None,
            }
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
        assert_eq!(items[1], ChatItem { id: 3, title: "Bike repair".into(), ts: 300 });
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
        let (title, messages, more) = thread(&c);
        assert_eq!(title, "Garden plan");
        assert!(!more);
        assert_eq!(messages.len(), 4, "only questions and answers");
        assert_eq!(messages[0], ThreadMessage { role: "user", text: "What grows in shade?".into(), id: None, voice: true, pictures: 2 });
        assert_eq!(messages[1].id.as_deref(), Some("chat:7:1"), "its place in Chat");
        assert_eq!(messages[3].id.as_deref(), Some("chat:7:4"));
        // The latest only, each message capped.
        c.messages = (0..KEEP_MESSAGES + 6).map(|i| said(if i % 2 == 0 { "user" } else { "assistant" }, &format!("m{i}"))).collect();
        c.messages.push(said("assistant", &"x".repeat(MAX_CHARS + 50)));
        let (_, messages, more) = thread(&c);
        assert!(more);
        assert_eq!(messages.len(), KEEP_MESSAGES);
        assert_eq!(messages[0].text, "m7");
        let last = messages.last().unwrap();
        assert_eq!(last.text.chars().count(), MAX_CHARS + 1);
        assert!(last.text.ends_with('…'));
        assert_eq!(last.id.as_deref(), Some(format!("chat:7:{}", KEEP_MESSAGES + 6).as_str()));
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

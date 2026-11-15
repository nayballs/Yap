//! **The cards Chat's model puts in the thread** (15 Nov 2026, three of the
//! Claude app's): a question to the person ([`ask`], `ask_user`), the
//! next-step box at the end of a turn ([`next_step`], `suggest_next_step`)
//! and chapters in a long chat ([`chapters`], `mark_chapter`). Each is a tool
//! the model calls when it has a reason to, never something shown every turn.
//!
//! - **Where** ([`Cards::for_turn`]): a model that calls tools, in Chat or
//!   the Voice Assistant (never across the meetings). A question needs
//!   someone to answer it (never a phone's turn: its page shows no cards);
//!   the next-step box needs Chat in view (its buttons are in the thread);
//!   a chapter goes anywhere in a chat.
//! - **The tool loop** offers them (`tools/agent/offer.rs`) and runs them
//!   here ([`execute`]); **a plan chat's** Claude Code gets the same tools
//!   over Yap's run endpoint (`integrations/claude_live/cards.rs`), the same
//!   code.
//! - **What they show** goes through the turn's stream (`stream::Event::
//!   Confirm` for a question, `NextStep`, `Chapter`), the watcher keeps it
//!   (`stream::Watch`) and the answer is saved with it
//!   (`ChatMessage::confirms`, `next_step`, `chapter`).
//!
//! The log has the tool and the outcome, never the words.

pub mod ask;
pub mod chapters;
pub mod next_step;

use serde_json::Value;

use crate::chat::stream::{Event, Live};

/// **Which cards a turn offers** (`turn/gates.rs`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Cards {
    /// `ask_user`.
    pub ask: bool,
    /// `suggest_next_step`.
    pub next_step: bool,
    /// `mark_chapter`.
    pub chapters: bool,
}

/// Every card's tool, in the order they're listed.
pub const NAMES: &[&str] = &[ask::NAME, next_step::NAME, chapters::NAME];

/// **The tools that show no chip of their own** (the box and the divider are
/// what they show).
pub fn quiet(name: &str) -> bool {
    name == next_step::NAME || name == chapters::NAME
}

/// **The question came from a paired phone** (its turn's network receipts
/// say so, `assistant.rs`): its page shows no cards.
pub fn from_phone() -> bool {
    crate::privacy::net::current_feature() == Some(crate::privacy::net::Feature::AssistantPhone)
}

/// **The question was asked out loud** (the Voice Assistant's turn, by its
/// network receipts' part of Yap): a question card is said too.
pub fn by_voice() -> bool {
    use crate::privacy::net::Feature;
    matches!(crate::privacy::net::current_feature(), Some(Feature::Assistant | Feature::AssistantScreen | Feature::AssistantSelection))
}

impl Cards {
    /// **The cards for a turn**: a model with tools (`tools`), never across
    /// the meetings (`meetings`), in Chat or the Voice Assistant (`scope`
    /// `None` or "assistant"); a question not from a phone (`phone`); the
    /// next-step box with Chat in view (`chat_in_view`). Pure.
    pub fn for_turn(tools: bool, meetings: bool, scope: Option<&str>, chat_in_view: bool, phone: bool) -> Cards {
        let base = tools && !meetings && matches!(scope, None | Some("assistant"));
        Cards { ask: base && !phone, next_step: base && chat_in_view && !phone, chapters: base }
    }

    /// Any of them offered.
    pub fn any(self) -> bool {
        self.ask || self.next_step || self.chapters
    }

    /// Whether `name` is one of them, offered.
    pub fn offers(self, name: &str) -> bool {
        match name {
            ask::NAME => self.ask,
            next_step::NAME => self.next_step,
            chapters::NAME => self.chapters,
            _ => false,
        }
    }

    /// The names offered, in [`NAMES`]' order.
    pub fn names(self) -> Vec<&'static str> {
        NAMES.iter().copied().filter(|n| self.offers(n)).collect()
    }

    /// The tool loop's definitions of what's offered.
    pub fn definitions(self) -> Vec<Value> {
        let mut out = Vec::new();
        if self.ask {
            out.push(ask::definition());
        }
        if self.next_step {
            out.push(next_step::definition());
        }
        if self.chapters {
            out.push(chapters::definition());
        }
        out
    }

    /// **A Code chat's own session's** (`agents/direct.rs`, 15 Nov 2026):
    /// the next-step box (with Chat in view: never a phone's turn) and
    /// chapters — its questions are Claude Code's own `AskUserQuestion`
    /// (`claude_live/approve.rs`). Pure.
    pub fn for_code_session(phone: bool) -> Cards {
        Cards { ask: false, next_step: !phone, chapters: true }
    }

    /// The lines for the model's instructions (empty: none offered).
    pub fn instructions(self) -> String {
        let mut out: Vec<&str> = Vec::new();
        if self.ask {
            out.push(ask::INSTRUCTIONS);
        }
        if self.next_step {
            out.push(next_step::INSTRUCTIONS);
        }
        if self.chapters {
            out.push(chapters::INSTRUCTIONS);
        }
        out.join(" ")
    }
}

/// How many answers conversation `id` has so far (a chapter never starts a
/// chat).
pub fn answers_in(id: Option<u64>) -> usize {
    id.and_then(crate::chat::chats::get).map_or(0, |c| c.messages.iter().filter(|m| m.role == "assistant" && m.kind.is_empty()).count())
}

/// **One call** of these tools: the question card waits for its answer
/// (`voice`: asked out loud, said and on the bar); the box and the chapter
/// go to the page through `live` at once (`earlier`: the chat's answers
/// before this turn's). The result for the model.
pub async fn execute(name: &str, args: &Value, live: Option<Live<'_>>, voice: bool, earlier: usize) -> Value {
    match name {
        ask::NAME => ask::ask(live, args, voice).await,
        next_step::NAME => {
            let (next, result) = next_step::execute(args);
            if let (Some(next), Some(live)) = (next, live) {
                tracing::info!(buttons = next.buttons.len(), "Chat: a next-step box under the answer");
                live.sink.send(Event::NextStep { next });
            }
            result
        }
        _ => {
            let (chapter, result) = chapters::execute(args, earlier);
            if let (Some(chapter), Some(live)) = (chapter, live) {
                tracing::info!("Chat: a new chapter marked");
                live.sink.send(Event::Chapter { chapter });
            }
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cards_go_where_someone_can_see_or_answer_them() {
        let chat = Cards::for_turn(true, false, None, true, false);
        assert_eq!(chat, Cards { ask: true, next_step: true, chapters: true });
        assert_eq!(chat.names(), NAMES);
        assert_eq!(chat.definitions().len(), 3);
        // The Voice Assistant away from Chat: asked out loud, no box (its buttons are in the thread).
        assert_eq!(Cards::for_turn(true, false, Some("assistant"), false, false), Cards { ask: true, next_step: false, chapters: true });
        // A phone's question: no question, no box.
        assert_eq!(Cards::for_turn(true, false, Some("assistant"), false, true), Cards { ask: false, next_step: false, chapters: true });
        // Across the meetings, the Meetings view's Ask, a model without tools: none.
        assert!(!Cards::for_turn(true, true, None, true, false).any());
        assert!(!Cards::for_turn(true, false, Some("meetings"), true, false).any());
        assert!(!Cards::for_turn(false, false, None, true, false).any());
        assert!(Cards::default().instructions().is_empty());
        assert!(quiet(next_step::NAME) && quiet(chapters::NAME) && !quiet(ask::NAME));
        // A Code chat's own session: Claude Code asks with its own card; a phone's turn has no box.
        assert_eq!(Cards::for_code_session(false), Cards { ask: false, next_step: true, chapters: true });
        assert_eq!(Cards::for_code_session(true).names(), [chapters::NAME]);
    }

    #[tokio::test]
    async fn the_box_and_the_chapter_go_to_the_page_at_once() {
        use crate::chat::stream::Sink;
        #[derive(Default)]
        struct Log(std::sync::Mutex<Vec<Event>>);
        impl Sink for Log {
            fn send(&self, ev: Event) {
                self.0.lock().unwrap().push(ev);
            }
        }
        let log = Log::default();
        let live = || Some(Live { sink: &log, beside: None });
        let box_ = execute(next_step::NAME, &serde_json::json!({ "recap": "Drafted.", "question": "Send it?", "buttons": ["Send it", "Not yet"] }), live(), false, 2).await;
        assert_eq!(box_["success"], true);
        let chapter = execute(chapters::NAME, &serde_json::json!({ "title": "Trip budget" }), live(), false, 2).await;
        assert_eq!(chapter["success"], true);
        let first = execute(chapters::NAME, &serde_json::json!({ "title": "Start" }), live(), false, 0).await;
        assert_eq!(first["success"], false);
        let kinds: Vec<String> = log.0.lock().unwrap().iter().map(|e| serde_json::to_value(e).unwrap()["kind"].as_str().unwrap_or("").to_string()).collect();
        assert_eq!(kinds, ["nextStep", "chapter"]);
        let sent = serde_json::to_value(log.0.lock().unwrap()[0].clone()).unwrap();
        assert_eq!((sent["next"]["question"].as_str(), sent["next"]["buttons"][1]["label"].as_str()), (Some("Send it?"), Some("Not yet")));
    }
}

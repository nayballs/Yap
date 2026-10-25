//! **The screenshots waiting in Chat's box** (2026-10-25): what a typed
//! question or a spoken one (the Voice Assistant) takes with it.
//!
//! Before, only the page held them, so a question asked out loud
//! (`assistant::heard` → `turn::chat_turn`) went without the screenshot
//! waiting in the box (the maintainer: "it doesn't send what I said and the
//! snip; just one or the other"). Rust keeps them now, and the page's
//! `waitingShots` follows [`EVENT`] (`{conversationId, images}`), so both
//! always agree:
//!
//! - **in**: a snip landing ([`landed`], `snip.rs`), a picture pasted or
//!   files dropped ([`added`], `snip/attach.rs`);
//! - **out**: the box's ✕ (`chat_waiting_remove`), "New chat"
//!   (`chat_waiting_clear`), a question sent with them (`chat_send`:
//!   [`sent`], given back by [`put_back`] when it fails before an answer as
//!   the page gives the question back), a Voice Assistant question
//!   ([`take_for_question`]).
//!
//! **One box, in one conversation** — as the page has it: the conversation
//! a snip landed in, then whichever Chat shows ([`viewing`], from
//! `chat_view_state`: the box goes with it, as the page's box does). A snip
//! landing in a new chat while the box waits in a conversation starts the
//! box afresh (the page's `newChat`); landing in a conversation adds to it.
//! At most [`MAX`] (a question carries 4). Ids and sizes only — never pixels;
//! nothing here is saved: a restart starts with an empty box.

use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::chat::chats::ChatImage;

/// The page event: the box now (`{conversationId, images}`).
pub const EVENT: &str = "yap-chat-waiting";

/// The most screenshots a question carries (the page's `MAX_SHOTS`).
pub const MAX: usize = super::images::MAX_PER_REQUEST;

/// The box: the screenshots waiting, and the conversation they wait in
/// (`None`: a new chat). Pure.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Waiting {
    pub conversation_id: Option<u64>,
    pub images: Vec<ChatImage>,
}

impl Waiting {
    /// Keep the newest [`MAX`], each once.
    fn push(&mut self, images: Vec<ChatImage>) {
        for img in images {
            if !self.images.iter().any(|i| i.id == img.id) {
                self.images.push(img);
            }
        }
        let over = self.images.len().saturating_sub(MAX);
        self.images.drain(..over);
    }

    /// A snip landed in `conversation` (`None`: a new chat): in the box,
    /// which waits there now. Into a new chat from a conversation, the box
    /// starts afresh (as the page's new chat empties it).
    pub fn land(&mut self, conversation: Option<u64>, images: Vec<ChatImage>) {
        if conversation.is_none() && self.conversation_id.is_some() {
            self.images.clear();
        }
        self.conversation_id = conversation;
        self.push(images);
    }

    /// Pictures pasted or dropped: into the box where it is.
    pub fn add(&mut self, images: Vec<ChatImage>) {
        self.push(images);
    }

    /// The box's ✕. `true`: it was there.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.images.len();
        self.images.retain(|i| i.id != id);
        self.images.len() != before
    }

    /// A typed question sent with these (`chat_send`): out of the box, and
    /// returned (to give back if it fails).
    pub fn take_ids(&mut self, ids: &[String]) -> Vec<ChatImage> {
        let (gone, kept): (Vec<ChatImage>, Vec<ChatImage>) = self.images.drain(..).partition(|i| ids.contains(&i.id));
        self.images = kept;
        gone
    }

    /// A spoken question takes them all, with the conversation they wait
    /// in. `None`: none wait.
    pub fn take(&mut self) -> Option<(Option<u64>, Vec<ChatImage>)> {
        if self.images.is_empty() {
            return None;
        }
        Some((self.conversation_id, std::mem::take(&mut self.images)))
    }

    /// A question that took them failed before its answer: back in the box,
    /// first (they were there before anything added since).
    pub fn put_back(&mut self, conversation: Option<u64>, images: Vec<ChatImage>) {
        if images.is_empty() {
            return;
        }
        if self.images.is_empty() {
            self.conversation_id = conversation;
        }
        let later = std::mem::take(&mut self.images);
        self.push(images);
        self.push(later);
    }

    /// Chat shows another conversation (or a new chat): the box goes with
    /// it, as the page's does.
    pub fn moved(&mut self, conversation: Option<u64>) {
        self.conversation_id = conversation;
    }

    pub fn clear(&mut self) {
        self.images.clear();
    }
}

static WAITING: Mutex<Waiting> = Mutex::new(Waiting { conversation_id: None, images: Vec::new() });

/// Change the box and tell the page (never emitting under the lock).
fn change<T>(app: Option<&AppHandle>, f: impl FnOnce(&mut Waiting) -> T) -> T {
    let (out, now) = {
        let mut w = WAITING.lock().unwrap_or_else(|p| p.into_inner());
        let before = w.clone();
        let out = f(&mut w);
        (out, (*w != before).then(|| w.clone()))
    };
    if let (Some(app), Some(now)) = (app, now) {
        tracing::info!(images = now.images.len(), "Chat: screenshots waiting");
        let _ = app.emit(EVENT, now);
    }
    out
}

/// The box now.
pub fn now() -> Waiting {
    WAITING.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

/// A snip landed in Chat ([`Waiting::land`]).
pub fn landed(app: &AppHandle, conversation: Option<u64>, image: ChatImage) {
    change(Some(app), |w| w.land(conversation, vec![image]));
}

/// Pictures pasted or dropped into the box.
pub fn added(app: &AppHandle, images: Vec<ChatImage>) {
    change(Some(app), |w| w.add(images));
}

/// `chat_send` sends these: out of the box (returned, to give back).
pub fn sent(app: &AppHandle, ids: &[String]) -> Vec<ChatImage> {
    change(Some(app), |w| w.take_ids(ids))
}

/// A question that took them failed before its answer: back in the box.
pub fn put_back(app: &AppHandle, conversation: Option<u64>, images: Vec<ChatImage>) {
    change(Some(app), |w| w.put_back(conversation, images));
}

/// A **Voice Assistant question** asked while screenshots wait takes them,
/// with the conversation they wait in; one whose file is gone (the sweep)
/// is left out. `None`: none wait.
pub fn take_for_question(app: &AppHandle) -> Option<(Option<u64>, Vec<ChatImage>)> {
    let (conversation, images) = change(Some(app), Waiting::take)?;
    let images: Vec<ChatImage> = images.into_iter().filter(|i| super::images::exists(&i.id)).collect();
    (!images.is_empty()).then_some((conversation, images))
}

/// The Chat view says what it shows (`assistant::chat_view_state`): while
/// it's open, the box waits in that conversation (`None`: a new chat).
pub fn viewing(open: bool, conversation: Option<u64>) {
    if open {
        change(None, |w| w.moved(conversation));
    }
}

/// The box as it is (the page asks when Chat opens).
#[tauri::command]
pub fn chat_waiting_get() -> Waiting {
    now()
}

/// The box's ✕ on a screenshot.
#[tauri::command]
pub fn chat_waiting_remove(app: AppHandle, id: String) {
    change(Some(&app), |w| w.remove(&id));
}

/// "New chat" (and the other ways the page empties the box).
#[tauri::command]
pub fn chat_waiting_clear(app: AppHandle) {
    change(Some(&app), Waiting::clear);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(id: &str) -> ChatImage {
        ChatImage { id: id.into(), w: 10, h: 10, bytes: 100, source: "snip".into(), ..Default::default() }
    }

    fn ids(w: &Waiting) -> Vec<&str> {
        w.images.iter().map(|i| i.id.as_str()).collect()
    }

    #[test]
    fn a_snip_lands_in_its_conversation_and_more_add_to_the_box() {
        let mut w = Waiting::default();
        w.land(Some(7), vec![img("a")]);
        assert_eq!((w.conversation_id, ids(&w)), (Some(7), vec!["a"]));
        // A second snip, a paste: added, each once.
        w.land(Some(7), vec![img("b")]);
        w.add(vec![img("c"), img("b")]);
        assert_eq!(ids(&w), ["a", "b", "c"]);
        // Another conversation: the box goes there with them (the page's
        // box keeps them when it switches).
        w.land(Some(9), vec![img("d")]);
        assert_eq!((w.conversation_id, ids(&w)), (Some(9), vec!["a", "b", "c", "d"]));
        // Four at most: the newest kept.
        w.add(vec![img("e")]);
        assert_eq!(ids(&w), ["b", "c", "d", "e"]);
        // Into a new chat from a conversation: afresh.
        w.land(None, vec![img("f")]);
        assert_eq!((w.conversation_id, ids(&w)), (None, vec!["f"]));
        // Into a new chat again: added.
        w.land(None, vec![img("g")]);
        assert_eq!(ids(&w), ["f", "g"]);
    }

    #[test]
    fn the_cross_removes_one_and_new_chat_clears_them() {
        let mut w = Waiting::default();
        w.land(Some(3), vec![img("a")]);
        w.add(vec![img("b")]);
        assert!(w.remove("a"));
        assert!(!w.remove("a"));
        assert_eq!(ids(&w), ["b"]);
        w.clear();
        assert!(w.images.is_empty());
        assert_eq!(w.take(), None);
    }

    #[test]
    fn a_typed_question_takes_what_it_sends_and_a_failure_gives_them_back() {
        let mut w = Waiting::default();
        w.land(Some(3), vec![img("a")]);
        w.add(vec![img("b")]);
        let gone = w.take_ids(&["a".to_string(), "b".to_string()]);
        assert_eq!(gone.len(), 2);
        assert!(w.images.is_empty(), "cleared on send");
        // A snip landed meanwhile; the failed question's come back first.
        w.add(vec![img("c")]);
        w.put_back(Some(3), gone);
        assert_eq!(ids(&w), ["a", "b", "c"]);
        // Only what was sent goes.
        let gone = w.take_ids(&["b".to_string()]);
        assert_eq!(ids(&w), ["a", "c"]);
        assert_eq!(gone, vec![img("b")]);
    }

    #[test]
    fn a_spoken_question_takes_them_all_with_their_conversation() {
        let mut w = Waiting::default();
        assert_eq!(w.take(), None, "none wait: the question goes alone");
        w.land(Some(5), vec![img("a")]);
        w.add(vec![img("b")]);
        assert_eq!(w.take(), Some((Some(5), vec![img("a"), img("b")])));
        assert!(w.images.is_empty());
        assert_eq!(w.take(), None, "taken once");
        // A new chat's: `None`, a new conversation.
        w.land(None, vec![img("c")]);
        assert_eq!(w.take(), Some((None, vec![img("c")])));
    }

    #[test]
    fn the_box_follows_the_conversation_chat_shows() {
        let mut w = Waiting::default();
        w.land(Some(5), vec![img("a")]);
        w.moved(Some(8));
        assert_eq!(w.take(), Some((Some(8), vec![img("a")])));
        w.land(Some(5), vec![img("b")]);
        w.moved(None);
        assert_eq!(w.take(), Some((None, vec![img("b")])));
        // Given back to an empty box: where they were.
        w.put_back(Some(5), vec![img("b")]);
        assert_eq!((w.conversation_id, ids(&w)), (Some(5), vec!["b"]));
    }
}

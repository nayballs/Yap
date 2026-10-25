//! **A note's Ask threads, kept** (2026-10-25; Wispr Flow keeps a meeting's
//! chats with it: "New chat" / "Past chats"): what was asked about a meeting
//! in the notepad's Ask sheet or its note's "Ask anything…" bar in Notes,
//! and what came back, stays with the note and reopens in both.
//!
//! **Chat's own store** (`chats.json`, `chats.rs`), each thread a
//! `Conversation` with `note` set, rather than a store of its own: the
//! messages are Chat's (the model that answered, stopped, a plain-text
//! email), its saving (`SAVER`: a failed save kept and said once), its
//! quarantine of a corrupt file and its deletes come for free, and a thread
//! is a few lines. Chat leaves them out — its list, its past-chats search
//! (`search_chats`), the Voice Assistant's "latest" — and deleting the note
//! deletes them (`notes::delete`).
//!
//! The windows save each turn as it's shown ([`note_chat_add`]: the
//! question and the answer, or what was stopped), so the two windows on one
//! meeting write to the same thread; every window showing the note follows
//! `yap-note-chat {noteId, conversationId, origin}`. "New chat" starts
//! another thread with the next question; the earlier ones stay, listed
//! under Past chats ([`note_chat`]). Errors aren't kept (they're the
//! window's, at the time). Logs: ids and counts, never the words.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::chat::chats::{self, ChatMessage, ChatModel, Conversation};

/// Every window showing note `noteId` reads its thread `conversationId`
/// again (`origin`: the window that added to it).
pub const EVENT: &str = "yap-note-chat";

/// A thread's title: its first question, cut short (as Chat names a
/// conversation by its first message).
const TITLE_CHARS: usize = 50;

/// One of a note's threads, as the Past chats list shows it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: u64,
    pub title: String,
    pub updated_ts: u64,
    pub message_count: usize,
}

/// What a window opening a note gets: its newest thread (the one to show)
/// and every thread, newest first.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteChat {
    pub current: Option<Conversation>,
    pub threads: Vec<Thread>,
}

/// A message as a window shows it, to keep.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewMessage {
    /// "user" | "assistant".
    pub role: String,
    pub text: String,
    #[serde(default)]
    pub plain: bool,
    #[serde(default)]
    pub stopped: bool,
    #[serde(default)]
    pub model: Option<ChatModel>,
}

fn thread(c: &Conversation) -> Thread {
    Thread { id: c.id, title: c.title.clone(), updated_ts: c.updated_ts, message_count: c.messages.len() }
}

/// Note `note`'s threads: the newest whole, and the list (pure but for the
/// store).
pub fn of_note(note: u64) -> NoteChat {
    let all = chats::for_note(note);
    NoteChat { threads: all.iter().map(thread).collect(), current: all.into_iter().next() }
}

/// A thread's title from its first question: one line, at most
/// [`TITLE_CHARS`] characters ("Ask about this meeting" when there's none).
pub fn title_of(messages: &[NewMessage]) -> String {
    let first = messages.iter().find(|m| m.role == "user").map(|m| m.text.split_whitespace().collect::<Vec<_>>().join(" "));
    match first.filter(|t| !t.is_empty()) {
        Some(t) if t.chars().count() > TITLE_CHARS => format!("{}…", t.chars().take(TITLE_CHARS).collect::<String>().trim_end()),
        Some(t) => t,
        None => "Ask about this meeting".to_string(),
    }
}

/// Keep `messages` in note `note`'s thread `conversation` — or, with none
/// (New chat, the first question) or one that isn't this note's any more,
/// a new thread. Returns the thread's id.
pub fn add(note: u64, conversation: Option<u64>, messages: &[NewMessage]) -> Result<u64, String> {
    crate::notes::get(note).ok_or("Note not found")?;
    let id = match conversation.and_then(chats::get).filter(|c| c.note == Some(note)) {
        Some(c) => c.id,
        None => chats::create_for_note(note, &title_of(messages)).id,
    };
    for m in messages.iter().filter(|m| !m.text.trim().is_empty()) {
        let role = if m.role == "assistant" { "assistant" } else { "user" };
        chats::append_whole(
            id,
            ChatMessage {
                role: role.to_string(),
                text: m.text.clone(),
                model: m.model.clone().filter(|_| role == "assistant"),
                stopped: m.stopped && role == "assistant",
                plain: m.plain && role == "assistant",
                ..Default::default()
            },
        )?;
    }
    Ok(id)
}

// ---- commands ----------------------------------------------------------------------------

/// Note `noteId`'s Ask threads: the newest whole, and all of them (Past
/// chats).
#[tauri::command]
pub fn note_chat(note_id: u64) -> NoteChat {
    of_note(note_id)
}

/// One of note `noteId`'s threads (Past chats → one), never another note's
/// or a Chat conversation.
#[tauri::command]
pub fn note_chat_open(note_id: u64, conversation_id: u64) -> Result<Conversation, String> {
    chats::get(conversation_id).filter(|c| c.note == Some(note_id)).ok_or_else(|| "That chat isn't there any more".to_string())
}

/// Keep a turn the window just showed in note `noteId`'s thread
/// `conversationId` (none: a new one); every window showing the note
/// follows (`yap-note-chat`). Returns the thread's id. A save that failed
/// is the error (the turn is kept in memory and saved with the next change).
#[tauri::command]
pub fn note_chat_add(
    app: AppHandle,
    note_id: u64,
    conversation_id: Option<u64>,
    messages: Vec<NewMessage>,
    origin: Option<String>,
) -> Result<u64, String> {
    let id = add(note_id, conversation_id, &messages)?;
    tracing::info!(note_id, conversation = id, messages = messages.len(), "Note chat: a turn kept");
    let _ = app.emit(EVENT, serde_json::json!({ "noteId": note_id, "conversationId": id, "origin": origin.unwrap_or_default() }));
    chats::saved().map_err(|e| e.to_string())?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(role: &str, text: &str) -> NewMessage {
        NewMessage { role: role.into(), text: text.into(), plain: false, stopped: false, model: None }
    }

    #[test]
    fn a_meetings_thread_is_kept_with_it_reopened_and_deleted_with_it() {
        let note = crate::notes::create("Supplier call", "", "meeting", "").id;
        let other = crate::notes::create("Standup", "", "meeting", "").id;
        assert!(of_note(note).current.is_none());
        // The first turn starts a thread named by its question.
        let email = NewMessage { plain: true, model: Some(ChatModel { provider: "groq".into(), id: "m".into(), name: String::new(), on_this_pc: false }), ..said("assistant", "Subject: Follow-up\n\nHi all,") };
        let id = add(note, None, &[said("user", "Follow-up email"), email]).unwrap();
        // The next turn, in the same thread (the other window's too).
        assert_eq!(add(note, Some(id), &[said("user", "Who owns the budget?"), said("assistant", "Alice.")]).unwrap(), id);
        let chat = of_note(note);
        let current = chat.current.unwrap();
        assert_eq!((current.id, current.title.as_str(), current.messages.len()), (id, "Follow-up email", 4));
        assert!(current.messages[1].plain && current.messages[1].model.is_some());
        assert!(!current.messages[0].plain, "a question is never plain");
        // Chat never lists it, nor does its past-chats search or the assistant's latest.
        let chat_conv = chats::create("Lisbon trip");
        chats::append(chat_conv.id, "user", "budget for Lisbon", &[], None, false).unwrap();
        let listed: Vec<u64> = chats::list().as_array().unwrap().iter().map(|c| c["id"].as_u64().unwrap()).collect();
        assert_eq!(listed, vec![chat_conv.id]);
        assert!(chats::search("who owns the budget", None, 5).iter().all(|f| f.id != id));
        assert_eq!(chats::latest().map(|l| l.0), Some(chat_conv.id));
        // New chat: another thread; the first stays, listed under Past chats.
        let second = add(note, None, &[said("user", "What did I miss?"), said("assistant", "Nothing new since you last looked.")]).unwrap();
        assert_ne!(second, id);
        let chat = of_note(note);
        assert_eq!(chat.current.unwrap().id, second);
        assert_eq!(chat.threads.iter().map(|t| t.id).collect::<Vec<_>>().len(), 2);
        assert_eq!(note_chat_open(note, id).unwrap().messages.len(), 4);
        // Another note's thread, or a Chat conversation, isn't this note's.
        assert!(note_chat_open(other, id).is_err());
        assert!(note_chat_open(note, chat_conv.id).is_err());
        assert_ne!(add(other, Some(id), &[said("user", "hi")]).unwrap(), id, "another note's thread: a new one");
        // Deleting the note deletes its threads, and only its.
        crate::notes::delete(note);
        assert!(chats::for_note(note).is_empty());
        assert!(chats::get(chat_conv.id).is_some() && !chats::for_note(other).is_empty());
        assert!(add(note, None, &[said("user", "hi")]).is_err(), "no note, no thread");
    }

    #[test]
    fn an_older_yaps_save_leaves_the_thread_as_an_ordinary_chat_its_messages_kept() {
        // The installed nightly and the dev build share one data folder: an
        // older Yap reads chats.json without `note` (or `plain`) and drops
        // them when it saves. What's left must be a whole chat.
        let note = crate::notes::create("Supplier call", "", "meeting", "").id;
        let email = NewMessage { plain: true, ..said("assistant", "Subject: Follow-up\n\nHi all,") };
        let id = add(note, None, &[said("user", "Follow-up email"), email]).unwrap();
        let mut v = serde_json::to_value(chats::get(id).unwrap()).unwrap();
        assert_eq!(v["note"], serde_json::json!(note));
        // As an older Yap saves it: the fields it doesn't know, gone.
        v.as_object_mut().unwrap().remove("note");
        for m in v["messages"].as_array_mut().unwrap() {
            m.as_object_mut().unwrap().remove("plain");
        }
        let back: Conversation = serde_json::from_value(v).unwrap();
        assert_eq!(back.note, None, "an ordinary chat now");
        assert_eq!(back.title, "Follow-up email");
        assert_eq!(back.messages.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(), ["Follow-up email", "Subject: Follow-up\n\nHi all,"]);
    }

    #[test]
    fn a_thread_is_named_by_its_first_question() {
        assert_eq!(title_of(&[said("user", "  What   did I miss? ")]), "What did I miss?");
        let long = "a ".repeat(60);
        assert!(title_of(&[said("user", &long)]).ends_with('…'));
        assert_eq!(title_of(&[said("assistant", "x")]), "Ask about this meeting");
    }
}

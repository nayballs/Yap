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

// ---- a thread's history, sized to the model -------------------------------------------------

/// Where a note's Ask goes.
#[derive(Debug, Clone, Copy)]
pub struct Call<'a> {
    pub provider: &'a str,
    pub model: &'a str,
    pub base_url: &'a str,
}

/// What a note's Ask sends before its question ([`history_for`]).
#[derive(Debug, Clone, Default)]
pub struct ThreadHistory {
    /// The summary of the older turns, as the system prompt carries it
    /// (`compact::block`), when they're left out.
    pub summary: Option<String>,
    /// The turns sent whole, oldest first: `(role, text)`.
    pub turns: Vec<(String, String)>,
    /// The model as a sentence names it, and its context (for "That's too
    /// long for …").
    pub label: String,
    pub context: u64,
}

/// **A note's Ask thread, sized to the model** (`chat/budget.rs`, 2026-10-25):
/// thread `thread` of note `note` before a question (`question`, as sent)
/// to `call`, whose prompt besides it is `fixed` (the note, About you…):
/// every earlier turn when they fit; else the thread's summary
/// (`compact.rs`) and the turns after it; else the latest that fit. A
/// question too long for the model alone is the plain sentence (`Err`).
/// No thread (a new one), or another note's: no history.
pub async fn history_for(note: u64, thread: Option<u64>, call: Call<'_>, fixed: &str, question: &str) -> Result<ThreadHistory, String> {
    use crate::chat::budget::{self, Counter, Kind, Mode};
    let cfg = crate::config::load();
    let counter = Counter::for_call(call.provider, call.base_url);
    let (context, known) = budget::context_window(&cfg, call.provider, call.model, call.base_url).await;
    let label = budget::model_label(call.provider, call.model, &crate::ai::local_llm::active_model_display(&cfg));
    let b = budget::budget(context, known, counter.count(fixed).await, Kind::Note, false, budget::request_cap(call.provider, call.model));
    let asked = counter.count(question).await + budget::MESSAGE_OVERHEAD;
    let mode = budget::mode();
    if mode != Mode::Messages && asked > b.history {
        tracing::info!(note, question_tokens = asked, room = b.history, "Note chat: the question is too long for the model; not sent");
        return Err(budget::too_long(&label, context));
    }
    let mut out = ThreadHistory { label, context, ..Default::default() };
    let Some(conv) = thread.and_then(chats::get).filter(|c| c.note == Some(note)) else {
        return Ok(out);
    };
    let plan = if mode == Mode::Messages {
        // As the pages sent it until 25 Oct 2026: the last six messages.
        let start = conv.messages.len().saturating_sub(6);
        budget::Plan { start, summary: false, left_out: start, tokens: 0 }
    } else {
        let costs = budget::message_costs(&counter, &conv.messages, false).await;
        let summary = match conv.usable_summary().filter(|_| mode == Mode::Summary) {
            Some(s) => Some((s.upto, counter.count(&crate::chat::compact::block(&s.text)).await)),
            None => None,
        };
        budget::plan(&costs, summary, b.history.saturating_sub(asked), Kind::Note.step())
    };
    if plan.summary {
        out.summary = conv.usable_summary().map(|s| crate::chat::compact::block(&s.text));
    }
    out.turns = conv.messages[plan.start..]
        .iter()
        .map(|m| (if m.role == "assistant" { "assistant" } else { "user" }.to_string(), m.text.clone()))
        .collect();
    tracing::info!(note, thread = conv.id, sent = out.turns.len(), summary = plan.summary, left_out = plan.left_out, "Note chat: the thread, sized to the model");
    Ok(out)
}

/// A one-off request (`[system, user]`, the notepad's question about the
/// meeting) with a thread's history: its summary after the system prompt,
/// its turns before the question. Pure.
pub fn with_thread(messages: serde_json::Value, thread: &ThreadHistory) -> serde_json::Value {
    let Some(list) = messages.as_array() else { return messages };
    let mut out: Vec<serde_json::Value> = Vec::with_capacity(list.len() + thread.turns.len());
    for (i, m) in list.iter().enumerate() {
        let last = i + 1 == list.len();
        if last {
            out.extend(thread.turns.iter().map(|(role, text)| serde_json::json!({ "role": role, "content": text })));
        }
        let mut m = m.clone();
        if let (Some(block), "system") = (&thread.summary, m["role"].as_str().unwrap_or("")) {
            let content = m["content"].as_str().unwrap_or("").to_string();
            m["content"] = serde_json::Value::String(format!("{content}\n\n{block}"));
        }
        out.push(m);
    }
    serde_json::Value::Array(out)
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
    let origin = origin.unwrap_or_default();
    // A long thread's older turns summarised, with the model its next
    // question goes to from this window (compact.rs).
    crate::chat::compact::after_thread_turn(&app, note_id, id, origin == "notepad");
    let _ = app.emit(EVENT, serde_json::json!({ "noteId": note_id, "conversationId": id, "origin": origin }));
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

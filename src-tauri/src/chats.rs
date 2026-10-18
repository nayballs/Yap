//! Chat conversations store — the AI Chat surface's data layer (OpenWhispr
//! `agent_conversations` SQLite tables, JSON-file edition like `notes.rs`).
//!
//! Conversations carry their messages inline; `chats.json` in the data dir.
//! The chat engine itself lives in `commands::chat_send` (eager keyword-RAG
//! over the notes store + the Chat scope's endpoint).

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    /// "user" | "assistant"
    pub role: String,
    pub text: String,
    pub ts: u64,
    /// An answer's tool activity, as its chips say it ("Edited "Launch
    /// plan": "Friday." → "Monday.""), so a conversation opened later still
    /// shows what the AI looked up and changed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    /// The model that wrote an answer (`ai_model::Target::chat_model`), so
    /// a conversation that switched models stays clear. Only written when
    /// known: the person's own messages and older answers have none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ChatModel>,
    /// The person's question came by voice (dictated into the chat box):
    /// "Only when I asked by voice" reads its answer out (`tts::auto_speak`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub voice: bool,
}

/// Which model wrote an answer: the provider and the model id the call went
/// to (the page names them: "GPT-OSS 20B · Groq").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatModel {
    /// "groq" | "anthropic" | "openai" | "gemini" | "openrouter" |
    /// "ondevice" | "local" | "custom".
    pub provider: String,
    /// The model id sent to the endpoint.
    pub id: String,
    /// The on-device model's name ("Qwen2.5 1.5B Instruct"): the sidecar
    /// serves whichever GGUF is loaded under one id.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// It ran on this PC (the on-device model, or a server on localhost).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub on_this_pc: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: u64,
    pub title: String,
    pub created_ts: u64,
    pub updated_ts: u64,
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Store {
    #[serde(default)]
    conversations: Vec<Conversation>,
}

static STATE: Mutex<Option<Store>> = Mutex::new(None);

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn chats_path() -> PathBuf {
    crate::config::data_dir().join("chats.json")
}

fn load_from_disk() -> Store {
    let path = chats_path();
    match std::fs::read_to_string(&path) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(store) => store,
            Err(e) => {
                tracing::error!("chats.json failed to parse: {}", e);
                crate::config::quarantine_corrupt(&path);
                Store::default()
            }
        },
        Err(_) => Store::default(),
    }
}

fn save_to_disk(store: &Store) {
    match serde_json::to_string(store) {
        Ok(json) => {
            let _ = std::fs::create_dir_all(crate::config::data_dir());
            if let Err(e) = crate::config::atomic_write(&chats_path(), &json) {
                tracing::warn!("Failed to persist chats: {}", e);
            }
        }
        Err(e) => tracing::warn!("Failed to serialize chats: {}", e),
    }
}

fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let store = guard.get_or_insert_with(load_from_disk);
    f(store)
}

/// Conversation summaries, newest-updated first:
/// `{ id, title, updatedTs, messageCount }`.
pub fn list() -> Value {
    with_store(|s| {
        let mut sorted: Vec<&Conversation> = s.conversations.iter().collect();
        sorted.sort_by_key(|c| std::cmp::Reverse(c.updated_ts));
        let items: Vec<Value> = sorted
            .into_iter()
            .map(|c| {
                json!({
                    "id": c.id,
                    "title": c.title,
                    "updatedTs": c.updated_ts,
                    "messageCount": c.messages.len(),
                })
            })
            .collect();
        json!(items)
    })
}

pub fn get(id: u64) -> Option<Conversation> {
    with_store(|s| s.conversations.iter().find(|c| c.id == id).cloned())
}

/// The conversation updated last: `(id, updated_ts)` (the Voice Assistant
/// continues it while it's recent, `assistant::choose_conversation`).
pub fn latest() -> Option<(u64, u64)> {
    with_store(|s| {
        s.conversations
            .iter()
            .max_by_key(|c| (c.updated_ts, c.id))
            .map(|c| (c.id, c.updated_ts))
    })
}

pub fn create(title: &str) -> Conversation {
    with_store(|s| {
        let id = s.conversations.iter().map(|c| c.id).max().unwrap_or(0) + 1;
        let now = now_secs();
        let conv = Conversation {
            id,
            title: title.trim().to_string(),
            created_ts: now,
            updated_ts: now,
            messages: Vec::new(),
        };
        s.conversations.push(conv.clone());
        save_to_disk(s);
        conv
    })
}

/// Add a message; `tools` = an answer's tool chips (empty for the user's),
/// `model` = the model that wrote it (`None` for the user's), `voice` = a
/// question that was dictated. Returns its index in the conversation.
pub fn append(
    id: u64,
    role: &str,
    text: &str,
    tools: &[String],
    model: Option<ChatModel>,
    voice: bool,
) -> Result<usize, String> {
    with_store(|s| {
        let conv = s
            .conversations
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or("Conversation not found")?;
        conv.messages.push(ChatMessage {
            role: role.to_string(),
            text: text.to_string(),
            ts: now_secs(),
            tools: tools.to_vec(),
            model,
            voice,
        });
        conv.updated_ts = now_secs();
        let index = conv.messages.len() - 1;
        save_to_disk(s);
        Ok(index)
    })
}

pub fn delete(id: u64) {
    with_store(|s| {
        s.conversations.retain(|c| c.id != id);
        save_to_disk(s);
    });
}

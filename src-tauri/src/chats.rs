//! Chat conversations store — the AI Chat surface's data layer (OpenWhispr
//! `agent_conversations` SQLite tables, JSON-file edition like `notes.rs`).
//!
//! Conversations carry their messages inline; `chats.json` in the data dir.
//! The chat engine itself lives in `commands::chat_send` (eager keyword-RAG
//! over the notes store + the Chat scope's endpoint).

#[cfg(not(test))]
use std::path::PathBuf;
#[cfg(not(test))]
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    /// The web pages an answer was written from (`web.rs`), numbered as it
    /// cites them ("[1]"): the Sources row under it in Chat. Only written
    /// when there are some.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<Source>,
}

/// One web source of an answer: its number in the answer's citations, the
/// page's title, its address and site ("bg3.wiki").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub n: usize,
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub site: String,
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

#[cfg(not(test))]
static STATE: Mutex<Option<Store>> = Mutex::new(None);

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(not(test))]
fn chats_path() -> PathBuf {
    crate::config::data_dir().join("chats.json")
}

#[cfg(not(test))]
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

#[cfg(not(test))]
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

#[cfg(not(test))]
fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut guard = match STATE.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let store = guard.get_or_insert_with(load_from_disk);
    f(store)
}

// Unit tests never read or write the real chats.json: each test thread gets
// a store of its own, in memory, that starts empty (as notes.rs does).
#[cfg(test)]
thread_local! {
    static TEST_STORE: std::cell::RefCell<Store> = std::cell::RefCell::new(Store::default());
}

#[cfg(test)]
fn save_to_disk(_store: &Store) {}

#[cfg(test)]
fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    TEST_STORE.with(|s| f(&mut s.borrow_mut()))
}

/// The longest title a rename keeps (a first message's title is its first
/// 50 characters).
pub const TITLE_MAX: usize = 80;

/// A title as a rename stores it: one line, runs of whitespace as one
/// space, trimmed, at most [`TITLE_MAX`] characters. Empty → an error.
pub fn clean_title(title: &str) -> Result<String, String> {
    let one_line = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.is_empty() {
        return Err("A conversation needs a name".to_string());
    }
    Ok(one_line.chars().take(TITLE_MAX).collect::<String>().trim_end().to_string())
}

/// Rename a conversation (the Chat sidebar's Rename). Its place in the list
/// stays: `updated_ts` is the last message's. Returns the stored title.
pub fn rename(id: u64, title: &str) -> Result<String, String> {
    let title = clean_title(title)?;
    with_store(|s| {
        let conv = s
            .conversations
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or("Conversation not found")?;
        if conv.title != title {
            conv.title = title.clone();
            save_to_disk(s);
        }
        Ok(title)
    })
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
    append_sourced(id, role, text, tools, model, voice, &[])
}

/// [`append`], with the web sources an answer was written from.
pub fn append_sourced(
    id: u64,
    role: &str,
    text: &str,
    tools: &[String],
    model: Option<ChatModel>,
    voice: bool,
    sources: &[Source],
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
            sources: sources.to_vec(),
        });
        conv.updated_ts = now_secs();
        let index = conv.messages.len() - 1;
        save_to_disk(s);
        Ok(index)
    })
}

pub fn delete(id: u64) {
    delete_many(&[id]);
}

/// Delete several conversations in one write (the Chat list's multi-select
/// and "Delete all chats…"). Returns how many were there to delete; ids
/// already gone are skipped.
pub fn delete_many(ids: &[u64]) -> usize {
    with_store(|s| {
        let before = s.conversations.len();
        s.conversations.retain(|c| !ids.contains(&c.id));
        let gone = before - s.conversations.len();
        if gone > 0 {
            save_to_disk(s);
        }
        gone
    })
}

/// An earlier message as the model sees it in the history: its text, and —
/// for an answer written from the web — a note that Yap searched for it and
/// where from. Without it a follow-up only sees the words and decides it
/// answered from memory ("I did not actually search…", Nathan 2026-10-21).
/// Pure.
pub fn history_text(m: &ChatMessage) -> String {
    if m.role != "assistant" || m.sources.is_empty() {
        return m.text.clone();
    }
    let list = m
        .sources
        .iter()
        .take(5)
        .map(|s| {
            let title: String = s.title.chars().take(70).collect();
            if s.site.is_empty() { format!("[{}] {}", s.n, title) } else { format!("[{}] {} ({})", s.n, title, s.site) }
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!("{}

(Yap searched the web for this answer. Its sources: {list}.)", m.text)
}

/// The source numbers an answer may cite without searching again: the
/// latest earlier answer's in `messages` that had any (what the Chat view
/// draws a follow-up's "[1]" against, `citedSources` in ChatView.svelte).
/// Pure.
pub fn earlier_source_numbers(messages: &[ChatMessage]) -> Vec<usize> {
    messages
        .iter()
        .rev()
        .find(|m| m.role == "assistant" && !m.sources.is_empty())
        .map(|m| m.sources.iter().map(|s| s.n).collect())
        .unwrap_or_default()
}

/// An answer's **citation markers with no source behind them** taken out:
/// a small model that answered from memory ("Python 3.12.0 [1].", no
/// search) would otherwise show a citation that points nowhere. `sourced`:
/// the numbers that do point somewhere (this turn's sources, else an
/// earlier answer's, [`earlier_source_numbers`]); a marker keeps those of
/// its numbers ("[1, 4]" with only 1 → "[1]") and goes when none is left,
/// with the space before it. Code (`…`, fenced blocks), markdown links'
/// text ("[1](https://…)") and "[0]" are left alone. Pure.
pub fn strip_unsourced_citations(reply: &str, sourced: &[usize]) -> String {
    static CITE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"\[(\d{1,2}(?:\s*[,–-]\s*\d{1,2})*)\]").expect("static regex")
    });
    // Byte ranges of code: fenced blocks, then inline spans outside them.
    let mut code: Vec<(usize, usize)> = Vec::new();
    let mut at = 0;
    while let Some(start) = reply[at..].find("```").map(|i| at + i) {
        let end = reply[start + 3..].find("```").map(|i| start + 3 + i + 3).unwrap_or(reply.len());
        code.push((start, end));
        at = end;
        if at >= reply.len() {
            break;
        }
    }
    let mut inline: Vec<(usize, usize)> = Vec::new();
    let mut open: Option<usize> = None;
    for (i, c) in reply.char_indices() {
        if c == '`' && !code.iter().any(|&(s, e)| i >= s && i < e) {
            match open.take() {
                Some(s) => inline.push((s, i + 1)),
                None => open = Some(i),
            }
        }
    }
    code.extend(inline);
    let in_code = |i: usize| code.iter().any(|&(s, e)| i >= s && i < e);

    let mut out = String::with_capacity(reply.len());
    let mut last = 0;
    for m in CITE.captures_iter(reply) {
        let whole = m.get(0).expect("match");
        if in_code(whole.start()) || reply[whole.end()..].starts_with('(') {
            continue;
        }
        let inner = &m[1];
        let nums: Vec<usize> = inner.split([',', '–', '-']).filter_map(|n| n.trim().parse().ok()).collect();
        let range = inner.contains(['–', '-']) && nums.len() == 2 && nums[0] < nums[1] && nums[1] - nums[0] <= 10;
        let nums: Vec<usize> = if range { (nums[0]..=nums[1]).collect() } else { nums };
        if nums.iter().all(|&n| n == 0) {
            continue; // "[0]": an index, not a citation
        }
        let kept: Vec<usize> = nums.iter().copied().filter(|n| sourced.contains(n)).collect();
        if kept.len() == nums.len() {
            continue;
        }
        let mut before = &reply[last..whole.start()];
        if kept.is_empty() {
            // "3.12.0 [1]." → "3.12.0."
            before = before.trim_end_matches([' ', '\u{a0}']);
            out.push_str(before);
        } else {
            out.push_str(before);
            let list: Vec<String> = kept.iter().map(|n| n.to_string()).collect();
            out.push_str(&format!("[{}]", list.join(", ")));
        }
        last = whole.end();
    }
    out.push_str(&reply[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_title_is_one_trimmed_line() {
        assert_eq!(clean_title("  Trip   plans \n for May ").unwrap(), "Trip plans for May");
        assert!(clean_title("").is_err());
        assert!(clean_title(" \n\t ").is_err());
        let t = clean_title(&"word ".repeat(40)).unwrap();
        assert!(t.chars().count() <= TITLE_MAX);
        assert!(!t.ends_with(' '));
    }

    #[test]
    fn an_answer_keeps_its_web_sources() {
        let c = create("Where's Gale?");
        let src = Source { n: 1, title: "Gale".into(), url: "https://bg3.wiki/wiki/Gale".into(), site: "bg3.wiki".into() };
        append(c.id, "user", "Where's Gale?", &[], None, false).unwrap();
        append_sourced(c.id, "assistant", "On the beach [1].", &[], None, false, std::slice::from_ref(&src)).unwrap();
        let conv = get(c.id).unwrap();
        assert!(conv.messages[0].sources.is_empty());
        assert_eq!(conv.messages[1].sources, [src]);
        // Stored camelCase, and only when there are some.
        let json = serde_json::to_value(&conv).unwrap();
        assert!(json["messages"][0].get("sources").is_none());
        assert_eq!(json["messages"][1]["sources"][0]["site"], "bg3.wiki");
        // An older file without them still reads.
        let old: ChatMessage = serde_json::from_str(r#"{"role":"assistant","text":"Hi","ts":1}"#).unwrap();
        assert!(old.sources.is_empty());
    }

    #[test]
    fn a_citation_with_no_source_behind_it_is_taken_out() {
        // Qwen3-4B from memory, no search (the model evaluations).
        assert_eq!(strip_unsourced_citations("The latest is Python 3.12.0 [1].", &[]), "The latest is Python 3.12.0.");
        assert_eq!(strip_unsourced_citations("It's 1,345 m[1][2] tall.", &[]), "It's 1,345 m tall.");
        assert_eq!(strip_unsourced_citations("Tall [1] and [2, 3] old.", &[]), "Tall and old.");
        // Sources there: kept, and only the numbers that point somewhere.
        assert_eq!(strip_unsourced_citations("Tall [1] and old [2].", &[1, 2]), "Tall [1] and old [2].");
        assert_eq!(strip_unsourced_citations("Tall [1, 4] and old [5].", &[1, 2]), "Tall [1] and old.");
        assert_eq!(strip_unsourced_citations("See [1–3].", &[2]), "See [2].");
        // Not citations: code, a link's text, an index.
        let code = "Use `xs[1]` or\n```\nys[2] = 3\n```\nand [1](https://a.example) or a[0].";
        assert_eq!(strip_unsourced_citations(code, &[]), code);
        assert_eq!(strip_unsourced_citations("No markers here.", &[]), "No markers here.");
    }

    #[test]
    fn a_follow_up_may_cite_the_latest_answer_that_had_sources() {
        let src = |n| Source { n, title: format!("S{n}"), url: format!("https://s{n}.example"), site: String::new() };
        let msg = |role: &str, sources: Vec<Source>| ChatMessage {
            role: role.into(),
            text: "x".into(),
            ts: 0,
            tools: vec![],
            model: None,
            voice: false,
            sources,
        };
        let messages = vec![
            msg("user", vec![]),
            msg("assistant", vec![src(1), src(2), src(3)]),
            msg("user", vec![]),
            msg("assistant", vec![]),
            msg("user", vec![]),
        ];
        assert_eq!(earlier_source_numbers(&messages), [1, 2, 3]);
        assert!(earlier_source_numbers(&messages[..1]).is_empty());
    }

    #[test]
    fn an_answer_from_the_web_says_so_in_the_history() {
        let mut m = ChatMessage { role: "assistant".into(), text: "Arsenal play Bayern tonight [1].".into(), ..Default::default() };
        assert_eq!(history_text(&m), "Arsenal play Bayern tonight [1].");
        m.sources = vec![Source { n: 1, title: "Bayern vs. Arsenal".into(), url: "https://espn.com/x".into(), site: "espn.com".into() }];
        assert_eq!(
            history_text(&m),
            "Arsenal play Bayern tonight [1].

(Yap searched the web for this answer. Its sources: [1] Bayern vs. Arsenal (espn.com).)"
        );
        // A question stays as asked.
        let q = ChatMessage { role: "user".into(), text: "When is it?".into(), sources: m.sources.clone(), ..Default::default() };
        assert_eq!(history_text(&q), "When is it?");
    }

    #[test]
    fn several_conversations_go_in_one_delete() {
        let ids: Vec<u64> = ["One", "Two", "Three", "Four"].iter().map(|t| create(t).id).collect();
        // Two of them, plus an id that's already gone.
        assert_eq!(delete_many(&[ids[0], ids[2], 999]), 2);
        let left: Vec<u64> = with_store(|s| s.conversations.iter().map(|c| c.id).collect());
        assert_eq!(left, vec![ids[1], ids[3]]);
        assert_eq!(delete_many(&[]), 0);
        // "Delete all chats…": every id the list showed.
        assert_eq!(delete_many(&left), 2);
        assert!(with_store(|s| s.conversations.is_empty()));
    }

    #[test]
    fn rename_keeps_the_conversation_in_its_place() {
        let a = create("First question...");
        let b = create("Second question...");
        append(a.id, "user", "Hello", &[], None, false).unwrap();
        let before = get(a.id).unwrap().updated_ts;
        assert_eq!(rename(a.id, "  Launch plan ").unwrap(), "Launch plan");
        let renamed = get(a.id).unwrap();
        assert_eq!(renamed.title, "Launch plan");
        assert_eq!(renamed.updated_ts, before);
        assert_eq!(renamed.messages.len(), 1);
        assert_eq!(get(b.id).unwrap().title, "Second question...");
        // An empty name changes nothing; a missing conversation says so.
        assert!(rename(a.id, "   ").is_err());
        assert_eq!(get(a.id).unwrap().title, "Launch plan");
        assert_eq!(rename(9999, "Nope").unwrap_err(), "Conversation not found");
        // The list shows the new name.
        assert!(list().as_array().unwrap().iter().any(|c| c["title"] == "Launch plan"));
    }
}

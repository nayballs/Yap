//! **Styles** (15 Nov 2026; the Claude app's styles, from Anthropic's help
//! pages): how answers are written — Normal, Concise, Explanatory, Formal, or
//! the person's own, made from a description or a sample of their writing.
//! Picked per chat from the composer's Style chip; a new chat starts with
//! the last one picked (the Claude app's selection stays). Chat's side only:
//! a Code chat's answers are Claude Code's.
//!
//! - **In the prompt** ([`prompt_block`]): one short block in the system
//!   prompt, just before About you — the stable part, so a model re-uses its
//!   cache from turn to turn — on every way Chat answers (the tool loop, a
//!   model without tools, the small web pipeline, your Claude plan's
//!   session) and in a question asked out loud into that chat. Normal adds
//!   nothing. A style shapes tone, length and layout only: never what's true,
//!   what the person asked, or the rules above it.
//! - **Your own** ([`chat_style_write`]): from a description ("Warm and
//!   brief, like a note to a friend") or a writing sample (pasted, up to
//!   [`SAMPLE_MAX`] characters), Chat's own model writes the style's
//!   instructions — how to write, never the sample's topic or facts — which
//!   the person reads and edits before it's saved. The sample itself is
//!   never kept. With no model, a description is the instructions as it is.
//! - **Where it lives**: `chat_styles.json` in Yap's data folder, a file of
//!   its own (an older Yap never reads or writes it): the person's styles,
//!   each chat's pick by id (Normal: none) and the last one picked; fields a
//!   newer Yap adds are kept. A deleted chat's pick goes with it; a deleted
//!   style's chats go back to Normal.
//!
//! The log says counts and lengths, never a style's words.

use std::collections::BTreeMap;
#[cfg(not(test))]
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// The style that adds nothing.
pub const NORMAL: &str = "normal";
/// A style's name: at most this many characters.
pub const NAME_MAX: usize = 40;
/// A style's instructions: at most this many characters.
pub const INSTRUCTIONS_MAX: usize = 1_200;
/// A writing sample: at most this much of it is read.
pub const SAMPLE_MAX: usize = 6_000;
/// The most styles of the person's own.
pub const MAX_STYLES: usize = 30;

/// One of Yap's own styles.
pub struct Builtin {
    pub id: &'static str,
    pub name: &'static str,
    /// The menu's line under its name.
    pub about: &'static str,
    /// What the model is told ("" for Normal).
    pub instructions: &'static str,
}

/// Yap's own styles, in the menu's order (the Claude app's four).
pub const BUILTINS: &[Builtin] = &[
    Builtin { id: NORMAL, name: "Normal", about: "Answers as they come", instructions: "" },
    Builtin {
        id: "concise",
        name: "Concise",
        about: "Shorter and more direct",
        instructions: "Keep answers short and direct: the answer first, then only the details that matter. No preamble, no recap, no repeating the question. A short list only when it's clearer than a sentence.",
    },
    Builtin {
        id: "explanatory",
        name: "Explanatory",
        about: "For learning something new",
        instructions: "Explain as a patient teacher would: start from what the person likely knows, define new terms the first time they come up, give the reasoning step by step with a small concrete example, and end with a one-line summary of the idea.",
    },
    Builtin {
        id: "formal",
        name: "Formal",
        about: "Clear and polished",
        instructions: "Write in a clear, polished, professional register: complete sentences, no slang, emoji or exclamation marks, precise wording, and a tidy structure (short headings or a list where they help).",
    },
];

/// A style of the person's own.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Style {
    /// "s-<millis>".
    pub id: String,
    pub name: String,
    pub instructions: String,
    /// What it was made from: "description" | "sample" | "" (written by hand).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub from: String,
    pub created_ms: u64,
}

/// chat_styles.json.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct Store {
    styles: Vec<Style>,
    /// Each chat's style, by conversation id (Normal: none).
    chats: BTreeMap<String, String>,
    /// The last style picked: a new chat starts with it.
    #[serde(skip_serializing_if = "String::is_empty")]
    last: String,
    /// Fields a newer Yap wrote, kept as they were.
    #[serde(flatten)]
    other: Map<String, Value>,
}

impl Store {
    /// `id`'s name and instructions, when it's a style there is (Normal: none).
    fn resolve(&self, id: &str) -> Option<(String, String)> {
        if let Some(b) = BUILTINS.iter().find(|b| b.id == id) {
            return (!b.instructions.is_empty()).then(|| (b.name.to_string(), b.instructions.to_string()));
        }
        self.styles.iter().find(|s| s.id == id && !s.instructions.trim().is_empty()).map(|s| (s.name.clone(), s.instructions.clone()))
    }

    fn exists(&self, id: &str) -> bool {
        BUILTINS.iter().any(|b| b.id == id) || self.styles.iter().any(|s| s.id == id)
    }

    /// The style a new chat starts with: the last one picked, if it's still there.
    fn for_new(&self) -> String {
        if !self.last.is_empty() && self.exists(&self.last) {
            self.last.clone()
        } else {
            NORMAL.to_string()
        }
    }

    /// Conversation `conv`'s style (`None`: a new chat's).
    fn of(&self, conv: Option<u64>) -> String {
        match conv {
            None => self.for_new(),
            Some(id) => self.chats.get(&id.to_string()).filter(|s| self.exists(s)).cloned().unwrap_or_else(|| NORMAL.to_string()),
        }
    }
}

// ---- the file -------------------------------------------------------------------------------

#[cfg(not(test))]
static STORE: Mutex<Option<Store>> = Mutex::new(None);

#[cfg(not(test))]
fn path() -> std::path::PathBuf {
    crate::config::data_dir().join("chat_styles.json")
}

/// chat_styles.json's saves (`saving.rs`): the person's own styles, so a
/// failed save is said, and tried again with the next.
pub static SAVER: crate::shell::saving::Saver = crate::shell::saving::Saver::new("chatStyles", "your answer styles", save_now);

fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    with_store(|s| write(s));
    SAVER.check()
}

#[cfg(not(test))]
fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut guard = STORE.lock().unwrap_or_else(|p| p.into_inner());
    let store = guard.get_or_insert_with(|| SAVER.load(&path()));
    if SAVER.any_unread() && SAVER.refresh(&path(), store) {
        write(store);
    }
    f(store)
}

#[cfg(not(test))]
fn write(s: &Store) {
    let _ = SAVER.write_json(&path(), s, true);
}

// A unit test never touches the disk: each thread its own store.
#[cfg(test)]
thread_local! {
    static TEST_STORE: std::cell::RefCell<Store> = std::cell::RefCell::new(Store::default());
}

#[cfg(test)]
fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    TEST_STORE.with(|s| f(&mut s.borrow_mut()))
}

#[cfg(test)]
fn write(_: &Store) {}

/// **Download your data** (`privacy/export.rs`): the file as it is, when
/// there's anything in it (a style of the person's, or a chat's pick).
pub fn snapshot() -> Option<Value> {
    with_store(|s| (!s.styles.is_empty() || !s.chats.is_empty()).then(|| serde_json::to_value(&*s).ok()).flatten())
}

// ---- the prompt -----------------------------------------------------------------------------

/// **The block** for style `name` with `instructions`. Pure.
pub fn block(name: &str, instructions: &str) -> String {
    format!(
        "How to write in this chat (the user picked the \"{}\" style): {} This shapes how you write only — tone, length and layout — never what's true, what you do, or the rules above.",
        name.trim(),
        instructions.trim()
    )
}

/// **The style block for conversation `conv`'s system prompt** (`None`: a
/// new chat's, the last style picked); `None` for Normal.
pub fn prompt_block(conv: Option<u64>) -> Option<String> {
    with_store(|s| {
        let id = s.of(conv);
        s.resolve(&id).map(|(name, instructions)| block(&name, &instructions))
    })
}

/// **A new chat was made** (`turn_conversation`): it takes the style its
/// new chat had (the last one picked). Not a Code chat.
pub fn adopt_new(conv: u64) {
    with_store(|s| {
        let id = s.for_new();
        let key = conv.to_string();
        let had = s.chats.remove(&key).is_some();
        if id != NORMAL {
            s.chats.insert(key, id);
        }
        if had || s.chats.contains_key(&conv.to_string()) {
            write(s);
        }
    });
}

/// Chats `ids` were deleted (or an incognito one ended): their picks go,
/// so a later chat with the same id starts as a new one does.
pub fn chats_deleted(ids: &[u64]) {
    with_store(|s| {
        let before = s.chats.len();
        for id in ids {
            s.chats.remove(&id.to_string());
        }
        if s.chats.len() != before {
            write(s);
        }
    });
}

/// Reset app / Delete some of your data → Chats: every chat's pick goes
/// (the styles stay: they're the person's settings).
pub fn reset_chats() {
    with_store(|s| {
        if !s.chats.is_empty() {
            s.chats.clear();
            write(s);
        }
    });
}

// ---- tidying what the person typed -------------------------------------------------------------

/// Whitespace runs as one space, at most `max` characters. Pure.
fn one_line(s: &str, max: usize) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect()
}

/// Instructions kept as written (their line breaks too), at most
/// [`INSTRUCTIONS_MAX`] characters. Pure.
fn tidy_instructions(s: &str) -> String {
    let t = s.replace("\r\n", "\n");
    let lines: Vec<&str> = t.lines().map(str::trim_end).collect();
    lines.join("\n").trim().chars().take(INSTRUCTIONS_MAX).collect()
}

// ---- writing one with the model ---------------------------------------------------------------

const WRITE_PROMPT: &str = "You write a style for an AI assistant's answers: instructions for how it should write, from what the user gives you — a description of the style they want, or a sample of their own writing. \
Reply with JSON only: {\"name\": \"…\", \"instructions\": \"…\"}. The name is 1 to 3 words. The instructions are 2 to 5 sentences in the second person (\"Write…\", \"Keep…\"), about tone, sentence length, vocabulary, structure and formatting. \
From a sample, describe how it's written, never what it's about: no names, places, numbers, topics or facts from it.";

/// The model's reply as `(name, instructions)`: its JSON (inside a code
/// fence or not), else the whole reply as the instructions. Pure.
pub fn parse_written(reply: &str) -> Option<(String, String)> {
    let t = reply.trim();
    let start = t.find('{');
    let end = t.rfind('}');
    if let (Some(a), Some(b)) = (start, end) {
        if let Ok(v) = serde_json::from_str::<Value>(&t[a..=b]) {
            let name = one_line(v["name"].as_str().unwrap_or(""), NAME_MAX);
            let instructions = tidy_instructions(v["instructions"].as_str().unwrap_or(""));
            if !instructions.is_empty() {
                return Some((name, instructions));
            }
        }
    }
    let instructions = tidy_instructions(t.trim_matches('`'));
    (!instructions.is_empty()).then(|| (String::new(), instructions))
}

/// What the model is asked: the description, or the sample (cut). Pure.
fn write_request(description: &str, sample: &str) -> String {
    if !sample.trim().is_empty() {
        let s: String = sample.trim().chars().take(SAMPLE_MAX).collect();
        format!("A sample of the user's own writing. Write the style that makes answers sound like it:\n\n<sample>\n{s}\n</sample>")
    } else {
        format!("The style the user wants: {}", one_line(description, 600))
    }
}

// ---- the commands -------------------------------------------------------------------------------

/// **The styles** (the Style chip's menu and the Styles dialog): Yap's own,
/// the person's, and the last one picked.
#[tauri::command]
pub fn chat_styles() -> Value {
    with_store(|s| {
        let builtins: Vec<Value> = BUILTINS.iter().map(|b| json!({ "id": b.id, "name": b.name, "about": b.about, "instructions": b.instructions })).collect();
        json!({ "builtins": builtins, "styles": s.styles, "last": s.for_new() })
    })
}

/// Conversation `conversation`'s style (`None`: a new chat's).
#[tauri::command]
pub fn chat_style_of(conversation: Option<u64>) -> String {
    with_store(|s| s.of(conversation))
}

/// **Pick a style** for conversation `conversation` (`None`: the new chat on
/// screen): kept for it, and the last one picked (a new chat starts with it).
#[tauri::command]
pub fn chat_style_set(conversation: Option<u64>, style: String) -> Result<String, String> {
    let style = style.trim().to_string();
    let picked = style.clone();
    with_store(move |s| {
        let style = picked;
        if !s.exists(&style) {
            return Err("That style isn't there any more.".to_string());
        }
        if let Some(id) = conversation {
            if style == NORMAL {
                s.chats.remove(&id.to_string());
            } else {
                s.chats.insert(id.to_string(), style.clone());
            }
        }
        s.last = if style == NORMAL { String::new() } else { style.clone() };
        write(s);
        Ok(style)
    })?;
    tracing::info!(builtin = BUILTINS.iter().any(|b| b.id == style), "Chat: a style picked");
    SAVER.check()?;
    Ok(style)
}

/// A style as the dialog sends it (`id` empty: a new one).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StyleIn {
    pub id: String,
    pub name: String,
    pub instructions: String,
    pub from: String,
}

/// **Save a style of the person's own**: a new one, or one changed.
#[tauri::command]
pub fn chat_style_save(style: StyleIn) -> Result<Style, String> {
    let name = one_line(&style.name, NAME_MAX);
    let instructions = tidy_instructions(&style.instructions);
    if name.is_empty() {
        return Err("Give the style a name.".to_string());
    }
    if instructions.is_empty() {
        return Err("Say how answers should be written.".to_string());
    }
    let from = match style.from.as_str() {
        f @ ("description" | "sample") => f.to_string(),
        _ => String::new(),
    };
    let saved = with_store(|s| {
        let taken = |s: &Store, id: &str| BUILTINS.iter().any(|b| b.name.eq_ignore_ascii_case(&name)) || s.styles.iter().any(|x| x.id != id && x.name.eq_ignore_ascii_case(&name));
        if taken(s, &style.id) {
            return Err(format!("There's a style called \u{201c}{name}\u{201d} already."));
        }
        if let Some(old) = s.styles.iter_mut().find(|x| !style.id.is_empty() && x.id == style.id) {
            old.name = name.clone();
            old.instructions = instructions.clone();
            if !from.is_empty() {
                old.from = from.clone();
            }
            let out = old.clone();
            write(s);
            return Ok(out);
        }
        if s.styles.len() >= MAX_STYLES {
            return Err(format!("You have {MAX_STYLES} styles: delete one first."));
        }
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
        let mut id = format!("s-{now}");
        while s.styles.iter().any(|x| x.id == id) {
            id.push('x');
        }
        let made = Style { id, name: name.clone(), instructions: instructions.clone(), from: from.clone(), created_ms: now };
        s.styles.push(made.clone());
        write(s);
        Ok(made)
    })?;
    tracing::info!(chars = saved.instructions.chars().count(), "Chat: a style saved");
    SAVER.check()?;
    Ok(saved)
}

/// **Delete a style of the person's own**: the chats that had it go back to Normal.
#[tauri::command]
pub fn chat_style_delete(id: String) -> Result<(), String> {
    with_store(|s| {
        let before = s.styles.len();
        s.styles.retain(|x| x.id != id);
        if s.styles.len() == before {
            return;
        }
        s.chats.retain(|_, v| *v != id);
        if s.last == id {
            s.last.clear();
        }
        write(s);
    });
    Ok(SAVER.check()?)
}

/// **Write a style** from a description or a writing sample, with Chat's
/// own model: `{ name, instructions }` for the person to read and edit
/// before it's saved. With no model set up, a description is the
/// instructions as it is (a sample needs the model).
#[tauri::command]
pub async fn chat_style_write(description: Option<String>, sample: Option<String>) -> Result<Value, String> {
    let description = description.unwrap_or_default();
    let sample = sample.unwrap_or_default();
    if description.trim().is_empty() && sample.trim().is_empty() {
        return Err("Describe the style, or paste some of your writing.".to_string());
    }
    let cfg = crate::config::load();
    let endpoint = crate::chat::turn::resolve_chat_endpoint(&cfg);
    let (base_url, api_key, model, provider, _persona, disable_thinking) = match endpoint {
        Ok(e) => e,
        Err(why) if sample.trim().is_empty() => {
            tracing::info!("Chat: a style written as described (no model: {why})");
            return Ok(json!({ "name": "", "instructions": tidy_instructions(&description), "byModel": false }));
        }
        Err(why) => return Err(why),
    };
    let messages = json!([
        { "role": "system", "content": WRITE_PROMPT },
        { "role": "user", "content": write_request(&description, &sample) },
    ]);
    let here = crate::ai::ai_model::runs_here(&provider, &base_url);
    let _sidecar = crate::ai::local_llm::hold(&base_url);
    let opts = crate::ai::llm::ChatOptions {
        temperature: 0.4,
        max_tokens: here.then_some(600),
        timeout: std::time::Duration::from_secs(if here { 120 } else { 60 }),
        disable_thinking,
    };
    let reply = crate::privacy::net::within(crate::privacy::net::Feature::Chat, crate::ai::llm::chat(&base_url, &api_key, &model, &provider, messages, &opts)).await?;
    let (name, instructions) = parse_written(&reply).ok_or("The model didn't write a style. Try again, or write it yourself.")?;
    tracing::info!(chars = instructions.chars().count(), sample = !sample.trim().is_empty(), "Chat: a style written by the model");
    Ok(json!({ "name": name, "instructions": instructions, "byModel": true }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn save(name: &str, instructions: &str) -> Style {
        chat_style_save(StyleIn { name: name.into(), instructions: instructions.into(), ..Default::default() }).unwrap()
    }

    #[test]
    fn normal_adds_nothing_and_a_builtin_adds_its_block() {
        assert_eq!(prompt_block(Some(1)), None);
        chat_style_set(Some(1), "concise".into()).unwrap();
        let b = prompt_block(Some(1)).unwrap();
        assert!(b.starts_with("How to write in this chat (the user picked the \"Concise\" style): Keep answers short"), "{b}");
        assert!(b.contains("never what's true"));
        // Another chat is still Normal; a new chat starts with the last picked.
        assert_eq!(prompt_block(Some(2)), None);
        assert_eq!(chat_style_of(None), "concise");
        chat_style_set(Some(1), NORMAL.into()).unwrap();
        assert_eq!(prompt_block(Some(1)), None);
        assert_eq!(chat_style_of(None), NORMAL);
    }

    #[test]
    fn a_new_chat_takes_the_last_style_and_a_deleted_chat_loses_it() {
        chat_style_set(None, "formal".into()).unwrap();
        adopt_new(9);
        assert_eq!(chat_style_of(Some(9)), "formal");
        chats_deleted(&[9]);
        assert_eq!(chat_style_of(Some(9)), NORMAL);
        assert!(chat_style_set(Some(9), "nope".into()).is_err());
    }

    #[test]
    fn the_persons_own_style_is_saved_edited_and_deleted() {
        let mine = save("  Like  a friend ", "Warm and brief.\r\nNo jargon.  ");
        assert_eq!(mine.name, "Like a friend");
        assert_eq!(mine.instructions, "Warm and brief.\nNo jargon.");
        chat_style_set(Some(4), mine.id.clone()).unwrap();
        assert!(prompt_block(Some(4)).unwrap().contains("\"Like a friend\" style): Warm and brief.\nNo jargon."));
        // A name taken (Yap's own too) is refused; editing keeps the id.
        assert!(chat_style_save(StyleIn { name: "concise".into(), instructions: "x".into(), ..Default::default() }).is_err());
        assert!(chat_style_save(StyleIn { name: "Like a friend".into(), instructions: "x".into(), ..Default::default() }).is_err());
        let edited = chat_style_save(StyleIn { id: mine.id.clone(), name: "Friendly".into(), instructions: "Warm.".into(), ..Default::default() }).unwrap();
        assert_eq!(edited.id, mine.id);
        assert!(prompt_block(Some(4)).unwrap().contains("\"Friendly\" style): Warm."));
        // Deleted: its chats go back to Normal.
        chat_style_delete(mine.id.clone()).unwrap();
        assert_eq!(chat_style_of(Some(4)), NORMAL);
        assert_eq!(prompt_block(Some(4)), None);
        // An empty name or instructions is refused.
        assert!(chat_style_save(StyleIn { name: "".into(), instructions: "x".into(), ..Default::default() }).is_err());
        assert!(chat_style_save(StyleIn { name: "x".into(), instructions: "  ".into(), ..Default::default() }).is_err());
    }

    #[test]
    fn the_models_style_is_read_from_its_json_or_its_words() {
        assert_eq!(parse_written("```json\n{\"name\": \"Dry wit\", \"instructions\": \"Write with understatement.\"}\n```"), Some(("Dry wit".into(), "Write with understatement.".into())));
        assert_eq!(parse_written("Write in short, plain sentences."), Some((String::new(), "Write in short, plain sentences.".into())));
        assert_eq!(parse_written("  "), None);
        // A sample is asked about as a sample, cut to SAMPLE_MAX; never kept.
        let req = write_request("", &"z".repeat(SAMPLE_MAX + 50));
        assert!(req.contains("<sample>"));
        assert_eq!(req.chars().filter(|c| *c == 'z').count(), SAMPLE_MAX);
        assert!(write_request("Warm and brief", "").starts_with("The style the user wants: Warm and brief"));
    }

    #[test]
    fn the_file_keeps_a_newer_yaps_fields_and_each_chats_pick() {
        let raw = r#"{"styles":[{"id":"s-1","name":"Plain","instructions":"Plain words.","createdMs":1}],"chats":{"3":"s-1"},"last":"s-1","future":{"x":1}}"#;
        let s: Store = serde_json::from_str(raw).unwrap();
        assert_eq!(s.of(Some(3)), "s-1");
        assert_eq!(s.of(None), "s-1");
        let back = serde_json::to_value(&s).unwrap();
        assert_eq!(back["future"]["x"], 1);
        // A pick of a style that's gone reads as Normal.
        let gone: Store = serde_json::from_str(r#"{"chats":{"3":"s-9"},"last":"s-9"}"#).unwrap();
        assert_eq!((gone.of(Some(3)), gone.of(None)), (NORMAL.to_string(), NORMAL.to_string()));
    }
}

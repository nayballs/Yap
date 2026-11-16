//! **Chapters** (`mark_chapter`, 15 Nov 2026, the Claude app's chapters):
//! in a long chat the model marks where the talk or the work moves on to
//! something meaningfully different, with a short title. The thread draws a
//! divider with the title above the question that started it, and a
//! floating table of contents jumps between them (`ChatChapters.svelte`).
//!
//! Kept with the answer (`ChatMessage::chapter`): one a turn (a second call
//! replaces the first), never on a chat's first answer (its start is the
//! first chapter already).
//!
//! **Pin as chapter** (16 Nov 2026, the Claude app's, under an answer:
//! `chat_chapter_pin`): the person makes an answer the start of a chapter
//! themselves — [`pin_title`] names it as the model would, a short noun
//! phrase, from the answer and its question —, kept the same way with
//! `pinned` set: its divider goes above the answer (under its question), its
//! entry in the contents, and a second click takes it off.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The tool's name.
pub const NAME: &str = "mark_chapter";
/// The most a title and a summary may say.
const TITLE_MAX: usize = 60;
const SUMMARY_MAX: usize = 160;
/// A pinned chapter's title from words alone: at most this many characters.
const WORDS_MAX: usize = 40;

/// **A chapter**, as the answer that starts it keeps it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Chapter {
    /// A short noun phrase ("Trip budget", "Fixing the export").
    pub title: String,
    /// One line on what it covers, for the contents' hover. Empty: none.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub summary: String,
    /// Pinned by the person (Pin as chapter), not marked by the model: its
    /// divider is above the answer itself. An older Yap's save drops it (the
    /// chapter stays, its divider above the question).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
}

/// `s` on one line, at most `max` characters, "…" when cut. Pure.
fn one_line(s: &str, max: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let cut: String = flat.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

/// The line for the model's instructions.
pub const INSTRUCTIONS: &str = "In a long chat, when the user moves on to a meaningfully different topic or piece of work, call mark_chapter with a short title for it: it draws a divider in the chat and an entry in its contents. Use it sparingly (a long chat has a few), never on the chat's first question or for a follow-up on the same thing.";

/// The tool's definition (`{"type":"function","function":…}`). Pure.
pub fn definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": NAME,
            "description": "Start a new chapter in this chat at the user's latest message: a divider with its title in the thread and an entry in the chat's contents, to jump between the parts of a long chat. Only when the talk or the work moves on to something meaningfully different; never for the chat's first question. Once a turn at most.",
            "parameters": {
                "type": "object",
                "properties": {
                    "title": { "type": "string", "description": "A short noun phrase, under 40 characters, like \"Trip budget\" or \"Fixing the export\"." },
                    "summary": { "type": "string", "description": "Optional: one line on what this chapter covers." }
                },
                "required": ["title"]
            }
        }
    })
}

/// **A call** in a chat with `earlier` answers before this turn's: the
/// chapter for the page (`None`: none) and the result for the model. Pure.
pub fn execute(args: &Value, earlier: usize) -> (Option<Chapter>, Value) {
    let text = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let title = one_line(text("title").trim_matches(['"', '\'', '#', ' ']), TITLE_MAX);
    if title.is_empty() {
        return (None, json!({ "success": false, "error": "mark_chapter needs a title: a short noun phrase.", "data": Value::Null, "displayText": "" }));
    }
    if earlier == 0 {
        return (None, json!({ "success": false, "error": "This is the chat's first question: its start is the first chapter already. Don't mark one now.", "data": Value::Null, "displayText": "" }));
    }
    let chapter = Chapter { summary: one_line(&text("summary"), SUMMARY_MAX), title, pinned: false };
    let result = json!({ "success": true, "data": { "title": chapter.title }, "displayText": "" });
    (Some(chapter), result)
}

/// What Chat's model is asked to name a pinned chapter with.
const PIN_PROMPT: &str = "You name chapters in a chat. Reply with only a title for the part of the chat that starts with this exchange: a short noun phrase under 40 characters, like \"Trip budget\" or \"Fixing the export\", in the language of the chat. No quotes, no full stop, nothing else.";

/// **A pinned chapter's title** (Pin as chapter): Chat's model names it from
/// the `question` and the `answer`, as `mark_chapter` would — in a Code chat
/// (`code`) only when that model is Claude's or on this PC, never sending its
/// work somewhere new —; else, or when the model can't, [`words_title`].
pub async fn pin_title(question: &str, answer: &str, code: bool) -> String {
    let fallback = words_title(question, answer);
    let cfg = crate::config::load();
    let Ok((base_url, api_key, model, provider, _persona, disable_thinking)) = crate::chat::turn::resolve_chat_endpoint(&cfg) else { return fallback };
    let here = crate::ai::ai_model::runs_here(&provider, &base_url);
    if code && !here && provider != crate::ai::claude_plan::PROVIDER && provider != "anthropic" {
        return fallback;
    }
    let clip = |s: &str, n: usize| s.chars().take(n).collect::<String>();
    let messages = json!([
        { "role": "system", "content": PIN_PROMPT },
        { "role": "user", "content": format!("<question>\n{}\n</question>\n<answer>\n{}\n</answer>", clip(question, 1_500), clip(answer, 2_500)) },
    ]);
    let _sidecar = crate::ai::local_llm::hold(&base_url);
    let opts = crate::ai::llm::ChatOptions { temperature: 0.2, max_tokens: here.then_some(40), timeout: Duration::from_secs(if here { 60 } else { 30 }), disable_thinking };
    let call = crate::privacy::net::within(crate::privacy::net::Feature::Chat, crate::ai::llm::chat(&base_url, &api_key, &model, &provider, messages, &opts));
    match call.await {
        Ok(reply) => model_title(&reply).unwrap_or(fallback),
        Err(e) => {
            tracing::info!(error_len = e.len(), "Chat: a pinned chapter's title from its words (the model couldn't)");
            fallback
        }
    }
}

/// The model's reply as a title: its first line, without quotes, a label or
/// a full stop, at most [`TITLE_MAX`] characters; `None` when nothing's left. Pure.
fn model_title(reply: &str) -> Option<String> {
    let line = reply.lines().map(str::trim).find(|l| !l.is_empty())?;
    let line = line.strip_prefix("Title:").or_else(|| line.strip_prefix("title:")).unwrap_or(line);
    let t = one_line(line.trim().trim_matches(['"', '\'', '“', '”', '#', '*', ' ']).trim_end_matches(['.', '!']), TITLE_MAX);
    (!t.is_empty()).then_some(t)
}

/// **A pinned chapter's title from words alone**: the question's first line
/// (else the answer's), its first letter capital, cut at a word under
/// [`WORDS_MAX`] characters, no end mark. Pure.
pub fn words_title(question: &str, answer: &str) -> String {
    let first = |s: &str| s.lines().map(|l| l.trim().trim_start_matches(['#', '>', '-', '*', ' ']).trim()).find(|l| !l.is_empty()).unwrap_or("").to_string();
    let mut line = first(question);
    if line.is_empty() {
        line = first(answer);
    }
    let line = one_line(&line, usize::MAX);
    let mut out = String::new();
    for w in line.split(' ') {
        if out.chars().count() + w.chars().count() + 1 > WORDS_MAX {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(w);
    }
    if out.is_empty() {
        out = line.chars().take(WORDS_MAX).collect();
    }
    let out = out.trim_end_matches(['.', '?', '!', ',', ':', ';']).to_string();
    let mut c = out.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => "Chapter".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chapter_has_a_short_title_and_never_starts_a_chat() {
        let (c, r) = execute(&json!({ "title": "  \"Trip budget\" ", "summary": "Costs for the\nweek away." }), 3);
        assert_eq!(c, Some(Chapter { title: "Trip budget".into(), summary: "Costs for the week away.".into(), pinned: false }));
        assert_eq!((r["success"].as_bool(), r["displayText"].as_str()), (Some(true), Some("")));
        let (none, r) = execute(&json!({ "title": "Start" }), 0);
        assert!(none.is_none() && r["error"].as_str().unwrap().contains("first question"));
        assert!(execute(&json!({ "title": " " }), 2).0.is_none());
        let long = execute(&json!({ "title": "word ".repeat(30) }), 1).0.unwrap();
        assert!(long.title.chars().count() <= TITLE_MAX && long.title.ends_with('…'));
        assert!(serde_json::to_value(Chapter { title: "A".into(), ..Default::default() }).unwrap().get("summary").is_none());
    }

    #[test]
    fn a_pinned_chapter_is_named_by_the_model_or_its_questions_first_words() {
        assert_eq!(model_title("\"Trip budget.\"\n"), Some("Trip budget".into()));
        assert_eq!(model_title("Title: Fixing the export"), Some("Fixing the export".into()));
        assert_eq!(model_title("  \n "), None);
        assert_eq!(words_title("how much is the trip going to cost us in total for the week?", ""), "How much is the trip going to cost us in");
        assert_eq!(words_title("  \n", "## Packing list\nSocks."), "Packing list");
        assert_eq!(words_title("", ""), "Chapter");
        // Kept as pinned; one the model marked says nothing of it, and an older chapter reads as the model's.
        let pinned = Chapter { title: "Trip budget".into(), pinned: true, ..Default::default() };
        assert_eq!(serde_json::to_value(&pinned).unwrap()["pinned"], json!(true));
        assert!(serde_json::to_value(Chapter { title: "A".into(), ..Default::default() }).unwrap().get("pinned").is_none());
        assert!(!serde_json::from_value::<Chapter>(json!({ "title": "Old" })).unwrap().pinned);
    }
}

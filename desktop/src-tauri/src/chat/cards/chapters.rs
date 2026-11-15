//! **Chapters** (`mark_chapter`, 15 Nov 2026, the Claude app's chapters):
//! in a long chat the model marks where the talk or the work moves on to
//! something meaningfully different, with a short title. The thread draws a
//! divider with the title above the question that started it, and a
//! floating table of contents jumps between them (`ChatChapters.svelte`).
//!
//! Kept with the answer (`ChatMessage::chapter`): one a turn (a second call
//! replaces the first), never on a chat's first answer (its start is the
//! first chapter already).

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The tool's name.
pub const NAME: &str = "mark_chapter";
/// The most a title and a summary may say.
const TITLE_MAX: usize = 60;
const SUMMARY_MAX: usize = 160;

/// **A chapter**, as the answer that starts it keeps it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Chapter {
    /// A short noun phrase ("Trip budget", "Fixing the export").
    pub title: String,
    /// One line on what it covers, for the contents' hover. Empty: none.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub summary: String,
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
    let chapter = Chapter { summary: one_line(&text("summary"), SUMMARY_MAX), title };
    let result = json!({ "success": true, "data": { "title": chapter.title }, "displayText": "" });
    (Some(chapter), result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chapter_has_a_short_title_and_never_starts_a_chat() {
        let (c, r) = execute(&json!({ "title": "  \"Trip budget\" ", "summary": "Costs for the\nweek away." }), 3);
        assert_eq!(c, Some(Chapter { title: "Trip budget".into(), summary: "Costs for the week away.".into() }));
        assert_eq!((r["success"].as_bool(), r["displayText"].as_str()), (Some(true), Some("")));
        let (none, r) = execute(&json!({ "title": "Start" }), 0);
        assert!(none.is_none() && r["error"].as_str().unwrap().contains("first question"));
        assert!(execute(&json!({ "title": " " }), 2).0.is_none());
        let long = execute(&json!({ "title": "word ".repeat(30) }), 1).0.unwrap();
        assert!(long.title.chars().count() <= TITLE_MAX && long.title.ends_with('…'));
        assert!(serde_json::to_value(Chapter { title: "A".into(), summary: String::new() }).unwrap().get("summary").is_none());
    }
}

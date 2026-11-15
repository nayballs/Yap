//! **The next-step box** (`suggest_next_step`, 15 Nov 2026, the Claude app's
//! box at the end of a turn): when the person has something to decide, the
//! model ends its answer with a one or two sentence recap and one question,
//! and one or two buttons that answer it. A click sends the button's words
//! as the person's next message (`ChatMessage.svelte`'s box), exactly as if
//! they'd typed them. Offered as a tool, never shown every turn: only when
//! the model calls it.
//!
//! The box is kept with the answer (`ChatMessage::next_step`) and shown under
//! the conversation's latest answer only: once anything follows it, it goes.
//! A second call in the same turn replaces the first.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The tool's name.
pub const NAME: &str = "suggest_next_step";
/// The most a recap, a question and a button may say.
const RECAP_MAX: usize = 300;
const ASK_MAX: usize = 120;
const LABEL_MAX: usize = 32;
const REPLY_MAX: usize = 400;
/// The most buttons a box has.
pub const BUTTONS_MAX: usize = 2;

/// **A next-step box**, as the answer keeps it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NextStep {
    /// What's now true and where it stands: one or two sentences.
    pub recap: String,
    /// The one decision left to the person. Empty: a recap alone.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub question: String,
    /// One or two answers to it, the first the model's pick.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub buttons: Vec<Button>,
}

/// One of the box's buttons.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Button {
    /// Its words: the person's own answer, back to the model ("Yes, draft it").
    pub label: String,
    /// What a click sends, when it says more than the label. Empty: the label.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reply: String,
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
pub const INSTRUCTIONS: &str = "When your answer leaves the user a decision or a step to take (go ahead with something, pick between two ways), end it by calling suggest_next_step with a short recap, that one question and one or two buttons that answer it; don't repeat the question in your text. Most answers need none: never for a plain answer, a greeting or small talk.";

/// The tool's definition (`{"type":"function","function":…}`). Pure.
pub fn definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": NAME,
            "description": "End your answer with a next-step box under it: a short recap and, when the user has something to decide, one question with one or two buttons that answer it. A click sends the button's words as the user's next message. Only when there's a real decision or step left; at most once a turn, as your last call.",
            "parameters": {
                "type": "object",
                "properties": {
                    "recap": { "type": "string", "description": "One or two short sentences: what is now done or true, and where it stands (what's not done yet)." },
                    "question": { "type": "string", "description": "The one decision left to the user, in one short sentence ending with a question mark. No reasons and no options in it: the buttons are the options." },
                    "buttons": {
                        "type": "array",
                        "description": "One or two answers to the question, your pick first. Each is the user's own words back to you.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "label": { "type": "string", "description": "One to four words, like \"Yes, draft it\" or \"Not now\"." },
                                "reply": { "type": "string", "description": "Optional: what the click sends, when it says more than the label." }
                            },
                            "required": ["label"]
                        }
                    }
                },
                "required": ["recap"]
            }
        }
    })
}

/// **The box a call asks for**: its recap, question and at most
/// [`BUTTONS_MAX`] buttons, each label once; buttons without a question get
/// none. `None`: no recap. Pure.
pub fn of_args(args: &Value) -> Option<NextStep> {
    let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let recap = one_line(&text(args, "recap"), RECAP_MAX);
    if recap.is_empty() {
        return None;
    }
    let question = one_line(&text(args, "question"), ASK_MAX);
    let mut buttons: Vec<Button> = Vec::new();
    // (A model may send plain strings for its buttons.)
    for b in args.get("buttons").and_then(Value::as_array).into_iter().flatten() {
        let (label, reply) = match b {
            Value::String(s) => (one_line(s, LABEL_MAX), String::new()),
            _ => (one_line(&text(b, "label"), LABEL_MAX), one_line(&text(b, "reply"), REPLY_MAX)),
        };
        if label.is_empty() || buttons.iter().any(|x| x.label.eq_ignore_ascii_case(&label)) {
            continue;
        }
        let reply = if reply.eq_ignore_ascii_case(&label) { String::new() } else { reply };
        buttons.push(Button { label, reply });
        if buttons.len() == BUTTONS_MAX {
            break;
        }
    }
    if question.is_empty() {
        buttons.clear();
    }
    Some(NextStep { recap, question, buttons })
}

/// **A call**: the box for the page (`None`: nothing to show) and the
/// result for the model. Pure.
pub fn execute(args: &Value) -> (Option<NextStep>, Value) {
    match of_args(args) {
        Some(next) => (Some(next), json!({ "success": true, "data": { "shown": true }, "note": "The box shows under your answer. Don't repeat its question in your text.", "displayText": "" })),
        None => (None, json!({ "success": false, "error": "suggest_next_step needs a recap: one or two sentences.", "data": Value::Null, "displayText": "" })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_has_a_recap_one_question_and_at_most_two_buttons() {
        let (next, result) = execute(&json!({
            "recap": "  The follow-up email is drafted in your notes.\nNothing is sent yet. ",
            "question": "Send it to the team now?",
            "buttons": [{ "label": "Yes, send it" }, "Not yet", { "label": "not yet" }, { "label": "Later" }]
        }));
        let next = next.unwrap();
        assert_eq!(next.recap, "The follow-up email is drafted in your notes. Nothing is sent yet.");
        assert_eq!(next.buttons.iter().map(|b| b.label.as_str()).collect::<Vec<_>>(), ["Yes, send it", "Not yet"]);
        assert_eq!(result["success"], true);
        assert_eq!(result["displayText"], "", "no chip of its own");
        // Buttons without a question: a recap alone.
        let alone = of_args(&json!({ "recap": "Done.", "buttons": ["OK"] })).unwrap();
        assert!(alone.buttons.is_empty() && alone.question.is_empty());
        let v = serde_json::to_value(&alone).unwrap();
        assert!(v.get("buttons").is_none() && v.get("question").is_none(), "{v}");
        // A reply that says more than its label is kept; the same words aren't.
        let r = of_args(&json!({ "recap": "Done.", "question": "Draft it?", "buttons": [{ "label": "Yes", "reply": "Yes, draft the email" }, { "label": "No", "reply": "no" }] })).unwrap();
        assert_eq!((r.buttons[0].reply.as_str(), r.buttons[1].reply.as_str()), ("Yes, draft the email", ""));
        // No recap: said so, nothing shown.
        let (none, err) = execute(&json!({ "question": "Go?" }));
        assert!(none.is_none() && err["success"] == false);
    }
}

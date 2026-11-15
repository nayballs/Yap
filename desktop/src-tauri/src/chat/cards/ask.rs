//! **Chat's model asks the person** (`ask_user`, 15 Nov 2026, the Claude
//! app's question card): one to four questions, each with two to four
//! options (a label, a line saying what it means, and an optional preview
//! of it), several to pick where a question says so, **Something else…** for
//! the person's own words, and **Let Yap decide**. The turn waits for the
//! answer ([`WAIT`]) and the model reads it as the call's result.
//!
//! - **One card, Chat's own**: it goes through the cards that ask first
//!   (`pages/act/confirm.rs`, act "question", its questions in
//!   [`QuestionCard`]), so it shows on the answer being written, comes back
//!   answered with the conversation (`ChatMessage::confirms`), and a ■ Stop
//!   drops it. It looks as a Code chat's question from Claude Code's own
//!   `AskUserQuestion` does (`integrations/claude_live/approve.rs`, the same
//!   [`Question`]s), and once answered the question and what was picked stay
//!   in the thread.
//! - **Answered by a click** (`chat_question_answer`): each question's text
//!   → the label picked, several joined with ", ", or the person's own words
//!   ([`answers_for`]); nothing (Let Yap decide): the model chooses and says
//!   what it chose.
//! - **Answered out loud** (the Voice Assistant, `assistant/spoken_ok.rs`):
//!   Yap says the question and its options ([`spoken_line`]), the Yap bar
//!   shows it ([`bar_card`]), and the next thing the person says in that
//!   conversation is the answer — matched to the options where it names one
//!   ([`heard`]: a label's words, or "the second one"), "you decide" leaves
//!   it to the model, and the words themselves always go with it.
//! - **Nobody to ask** (an evaluation, a phone's question, nobody watching
//!   the turn, or a Code chat's turn in Bypass permissions, where nothing
//!   asks): no card; the model is told to ask in words or choose itself.
//! - **The plan's runs** (a plan chat's Claude Code) call the same code over
//!   Yap's run endpoint (`integrations/claude_live/cards.rs`).
//!
//! The log has the card's id, how many questions and how it ended, never a
//! question, an option or an answer.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::chat::pages::act::confirm::{self, Card, Reply};
use crate::chat::stream::{Event, Live};

/// The tool's name.
pub const NAME: &str = "ask_user";
/// The card's act (`confirm::Card::act`).
pub const ACT: &str = "question";
/// How long a question waits for its answer: as long as a Code chat's
/// question from Claude Code waits (`claude_live::APPROVE_WAIT`), well
/// inside the plan run's tool timeout.
pub const WAIT: Duration = Duration::from_secs(600);
/// The most questions a card has, and options a question (Claude Code's own
/// limits for its `AskUserQuestion`).
pub const QUESTIONS_MAX: usize = 4;
pub const OPTIONS_MAX: usize = 4;
/// How long an answer in the person's own words may be.
pub const ANSWER_MAX: usize = 2_000;
/// How much of an option's preview the card keeps.
const PREVIEW_MAX: usize = 4_000;
/// What the model is told when the choice is left to it.
pub const CHOOSE: &str = "Make the most sensible choice yourself, carry on, and say in your answer what you chose.";

/// **One question**, as the card shows it — Chat's own, and a Code chat's
/// from Claude Code's `AskUserQuestion` alike.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Question {
    /// The question, whole: the answer goes back under these very words.
    pub question: String,
    /// A word or two for it ("Format").
    pub header: String,
    pub options: Vec<Choice>,
    /// Several of them may be picked.
    pub multi_select: bool,
}

/// One answer a question offers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Choice {
    pub label: String,
    pub description: String,
    /// **What it would look like** (markdown: a draft, a layout, a bit of
    /// code), shown beside the options while it's the one in focus. Empty:
    /// none.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub preview: String,
}

/// **A question card's part** of a card that asks (`confirm::Card::question`,
/// act [`ACT`]): the questions, and once answered what was picked.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct QuestionCard {
    pub questions: Vec<Question>,
    /// Each question's text → what was picked (several: ", "), or the
    /// person's own words. Empty while it asks, or left to the model.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub answers: BTreeMap<String, String>,
    /// The person's own words, answered out loud.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub said: String,
    /// Asked out loud (the Voice Assistant): said, and on the Yap bar too.
    pub voice: bool,
}

/// `s` on one line, at most `max` characters. Pure.
fn one_line(s: &str, max: usize) -> String {
    clip(&s.split_whitespace().collect::<Vec<_>>().join(" "), max)
}

/// At most `max` characters, "…" when cut. Pure.
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

/// **The questions of a call** (Chat's `ask_user`, Claude Code's
/// `AskUserQuestion`: `{questions: [{question, header, options: [{label,
/// description, preview}], multiSelect}]}`): each with its text and at
/// least one option with a label, at most [`QUESTIONS_MAX`] of
/// [`OPTIONS_MAX`] options, an option's label once. Pure.
pub fn questions_of(input: &Value) -> Vec<Question> {
    let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    // (A model may send the list as a JSON string.)
    let parsed;
    let list = match input.get("questions") {
        Some(Value::String(s)) => {
            parsed = serde_json::from_str::<Value>(s).unwrap_or(Value::Null);
            &parsed
        }
        Some(v) => v,
        None => &Value::Null,
    };
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|q| {
            let question = text(q, "question");
            let mut options: Vec<Choice> = Vec::new();
            for o in q.get("options").and_then(Value::as_array).into_iter().flatten() {
                let label = one_line(&text(o, "label"), 80);
                if label.is_empty() || options.iter().any(|c| c.label == label) {
                    continue;
                }
                options.push(Choice { label, description: one_line(&text(o, "description"), 200), preview: clip(&text(o, "preview"), PREVIEW_MAX) });
                if options.len() == OPTIONS_MAX {
                    break;
                }
            }
            let multi_select = q.get("multiSelect").or_else(|| q.get("multi_select")).and_then(Value::as_bool).unwrap_or(false);
            (!question.is_empty() && !options.is_empty()).then(|| Question { question, header: one_line(&text(q, "header"), 24), options, multi_select })
        })
        .take(QUESTIONS_MAX)
        .collect()
}

/// **The person's answers**: `given` (each question's text → a label, their
/// own words, or a list of labels) kept for the card's own questions only, a
/// list joined with ", ", their own words at most [`ANSWER_MAX`]. None:
/// nothing was answered. Pure.
pub fn answers_for(questions: &[Question], given: &Value) -> Option<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for q in questions {
        let said = match given.get(&q.question) {
            Some(Value::String(s)) => s.trim().to_string(),
            Some(Value::Array(list)) => list.iter().filter_map(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(", "),
            _ => String::new(),
        };
        if !said.is_empty() {
            out.insert(q.question.clone(), clip(&said, ANSWER_MAX));
        }
    }
    (!out.is_empty()).then_some(out)
}

// ---- answered out loud ---------------------------------------------------------------------

/// `s` as folded words joined by spaces (`act::rules::words`). Pure.
fn norm(s: &str) -> String {
    crate::chat::pages::act::rules::words(s).join(" ")
}

/// An option's label as it's said: without a note in brackets at its end
/// ("Camping stove (Recommended)" is said "camping stove"). Pure.
fn said_label(label: &str) -> &str {
    let t = label.trim();
    match (t.ends_with(')'), t.rfind('(')) {
        (true, Some(at)) if at > 0 => t[..at].trim_end(),
        _ => t,
    }
}

/// Words that leave the choice to the model ("you decide", "up to you").
const LEFT_TO_YOU: &[&str] = &[
    "you decide",
    "you choose",
    "you pick",
    "up to you",
    "your call",
    "your choice",
    "whatever you think",
    "whichever you think",
    "doesn t matter",
    "does not matter",
    "don t mind",
    "dont mind",
    "i don t mind",
    "either",
    "any of them",
    "you can decide",
    "let yap decide",
    "surprise me",
];

/// The ordinal words a reply may name an option by, and its index (`last`:
/// the last option).
const ORDINALS: &[(&str, usize)] = &[("first", 0), ("1st", 0), ("second", 1), ("2nd", 1), ("third", 2), ("3rd", 2), ("fourth", 3), ("4th", 3)];
/// A number said after "option" or "number" ("option two").
const NUMBERS: &[(&str, usize)] = &[("one", 0), ("1", 0), ("a", 0), ("two", 1), ("2", 1), ("b", 1), ("three", 2), ("3", 2), ("c", 2), ("four", 3), ("4", 3), ("d", 3)];

/// Whether `hay` (folded words, spaced) has `needle`'s words side by side.
fn has_words(hay: &str, needle: &str) -> bool {
    !needle.is_empty() && format!(" {hay} ").contains(&format!(" {needle} "))
}

/// The options of `q` a reply names: by their label's words (or the label
/// written as one word, "github" for "GitHub"), else — one question only —
/// by its place ("the second one", "option 3", "the last"). Pure.
fn named(q: &Question, said: &str, only: bool) -> Vec<usize> {
    let words = norm(said);
    let squashed = words.replace(' ', "");
    let by_label: Vec<usize> = q
        .options
        .iter()
        .enumerate()
        .filter(|(_, o)| {
            let label = norm(said_label(&o.label));
            let flat = label.replace(' ', "");
            has_words(&words, &label) || (flat.chars().count() >= 4 && squashed.contains(&flat))
        })
        .map(|(i, _)| i)
        .collect();
    if !by_label.is_empty() {
        return by_label;
    }
    // A longer label said with a word left out ("gas stove" for "Gas camping
    // stove"): all its words but one, two at least — when only one option fits.
    let heard_words: std::collections::HashSet<&str> = words.split(' ').filter(|w| !w.is_empty()).collect();
    let loose: Vec<usize> = q
        .options
        .iter()
        .enumerate()
        .filter(|(_, o)| {
            let label = norm(said_label(&o.label));
            let label_words: Vec<&str> = label.split(' ').filter(|w| w.chars().count() > 1).collect();
            let hits = label_words.iter().filter(|w| heard_words.contains(*w)).count();
            label_words.len() >= 3 && hits >= 2 && hits + 1 >= label_words.len()
        })
        .map(|(i, _)| i)
        .collect();
    if loose.len() == 1 || !only {
        return if loose.len() == 1 { loose } else { Vec::new() };
    }
    let list: Vec<&str> = words.split(' ').filter(|w| !w.is_empty()).collect();
    let n = q.options.len();
    let mut by_place: Vec<usize> = Vec::new();
    for (i, w) in list.iter().enumerate() {
        let at = ORDINALS.iter().find(|(o, _)| o == w).map(|(_, i)| *i).or_else(|| (*w == "last").then(|| n.saturating_sub(1))).or_else(|| {
            // "option two", "number 3"
            (i > 0 && matches!(list[i - 1], "option" | "number" | "no")).then(|| NUMBERS.iter().find(|(o, _)| o == w).map(|(_, i)| *i)).flatten()
        });
        if let Some(at) = at.filter(|a| *a < n) {
            if !by_place.contains(&at) {
                by_place.push(at);
            }
        }
    }
    // A reply that's only a number ("two", "3").
    if by_place.is_empty() && list.len() == 1 {
        if let Some(at) = NUMBERS.iter().filter(|(o, _)| o.len() > 1 || o.chars().all(|c| c.is_ascii_digit())).find(|(o, _)| *o == list[0]).map(|(_, i)| *i).filter(|a| *a < n) {
            by_place.push(at);
        }
    }
    by_place
}

/// **What a spoken reply answers**: each question whose options it names
/// (one, for a question that takes one; any, for one that takes several) →
/// those labels. `None`: it leaves the choice to the model ("you decide",
/// "either", and nothing named). Pure.
pub fn heard(questions: &[Question], said: &str) -> Option<BTreeMap<String, String>> {
    let only = questions.len() == 1;
    let mut out = BTreeMap::new();
    for q in questions {
        let picks = named(q, said, only);
        let fits = if q.multi_select { !picks.is_empty() } else { picks.len() == 1 };
        if fits {
            out.insert(q.question.clone(), picks.iter().map(|&i| q.options[i].label.clone()).collect::<Vec<_>>().join(", "));
        }
    }
    let words = norm(said);
    if out.is_empty() && LEFT_TO_YOU.iter().any(|p| has_words(&words, p)) {
        return None;
    }
    Some(out)
}

/// **The line Yap says** for a question card asked out loud: the first
/// question and its options ("Which format should the notes use: Markdown,
/// plain text or a table? Say one, or pick on the card."), and how many
/// more the card has. Pure.
pub fn spoken_line(card: &QuestionCard) -> String {
    let Some(q) = card.questions.first() else { return String::new() };
    let labels: Vec<&str> = q.options.iter().map(|o| o.label.as_str()).collect();
    let list = match labels.as_slice() {
        [] => String::new(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} or {last}", rest.join(", ")),
    };
    let ask = q.question.trim().trim_end_matches('?').trim_end_matches(['.', ':']);
    let more = match card.questions.len() {
        1 => String::new(),
        2 => " There's one more question on the card.".to_string(),
        n => format!(" There are {} more questions on the card.", n - 1),
    };
    let how = if q.multi_select { "Say the ones you want, or pick on the card." } else { "Say one, or pick on the card." };
    format!("{ask}: {list}? {how}{more}")
}

/// **The Yap bar's card** for a question asked out loud (question `id`),
/// as a Code chat's question has it (`agents/approve_card.rs`): the
/// question, its options listed — one question of two options: the two as
/// its buttons —, else **Answer in Chat** (the chat, where its card has
/// every option); **Let Yap decide** under them. Pure.
pub fn bar_card(id: u64, card: &QuestionCard) -> crate::bar::Card {
    let first = card.questions.first().cloned().unwrap_or_default();
    let two = card.questions.len() == 1 && !first.multi_select && first.options.len() == 2;
    let mut body: Vec<String> = Vec::new();
    if !two {
        body.extend(first.options.iter().enumerate().map(|(i, o)| format!("{}. {}", i + 1, o.label)));
    }
    if card.questions.len() > 1 {
        body.push(format!("…and {} more on the card in Chat.", card.questions.len() - 1));
    }
    let (primary, secondary) = if two {
        (crate::bar::CardAction::new(format!("pick:{id}:0"), first.options[0].label.clone()), Some(crate::bar::CardAction::new(format!("pick:{id}:1"), first.options[1].label.clone())))
    } else {
        (crate::bar::CardAction::new(format!("open:{id}"), "Answer in Chat"), None)
    };
    crate::bar::Card {
        id: crate::chat::assistant::spoken_ok::CARD.into(),
        icon: "assistant",
        title: one_line(&first.question, 120),
        body: body.join("\n"),
        status: Some("Say your answer".into()),
        primary: Some(primary),
        secondary,
        link: Some(crate::bar::CardAction::new(format!("decide:{id}"), "Let Yap decide")),
        timeout_ms: None,
        ..Default::default()
    }
}

/// The bar card's buttons: `pick:<id>:<n>` (that option), `open:<id>` (the
/// chat), `decide:<id>` (left to the model). Pure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarAction {
    Pick(u64, usize),
    Open(u64),
    Decide(u64),
}

/// [`BarAction`] of a button's id. Pure.
pub fn bar_action(action: &str) -> Option<BarAction> {
    if let Some(rest) = action.strip_prefix("pick:") {
        let (id, n) = rest.split_once(':')?;
        return Some(BarAction::Pick(id.parse().ok()?, n.parse().ok()?));
    }
    if let Some(id) = action.strip_prefix("open:") {
        return id.parse().ok().map(BarAction::Open);
    }
    action.strip_prefix("decide:").and_then(|id| id.parse().ok()).map(BarAction::Decide)
}

// ---- the tool ------------------------------------------------------------------------------

/// The line for the model's instructions (the tool loop's and a plan run's).
pub const INSTRUCTIONS: &str = "When you can't go on well without the user's choice (which of a few ways to do it, what they prefer, a detail with a few likely answers), call ask_user rather than guessing or asking in a long paragraph: its card waits for their pick. Not for something they already said, a plain yes or no you can ask in words, or every turn.";

/// The tool's definition (`{"type":"function","function":…}`), as Claude
/// Code's `AskUserQuestion` takes it, with a preview on an option. Pure.
pub fn definition() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": NAME,
            "description": "Ask the user one to four multiple-choice questions on a card in the chat, and wait for their answer (they click, or say it out loud). Each question has 2 to 4 options, each a short label with a line saying what it means; they can also write their own answer, or leave it to you. Put the option you recommend first, with \"(Recommended)\" at the end of its label. Returns each question's answer, or says the choice is yours.",
            "parameters": {
                "type": "object",
                "properties": {
                    "questions": {
                        "type": "array",
                        "description": "One to four questions.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "question": { "type": "string", "description": "The whole question, ending with a question mark." },
                                "header": { "type": "string", "description": "A word or two for it, at most 12 characters, like \"Format\"." },
                                "options": {
                                    "type": "array",
                                    "description": "Two to four choices that don't overlap. Never an \"Other\" option: the card has one.",
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "label": { "type": "string", "description": "One to five words." },
                                            "description": { "type": "string", "description": "What it means, or what follows from it." },
                                            "preview": { "type": "string", "description": "Optional: what it would look like (a short draft, a layout, a bit of code), in markdown, shown beside the options." }
                                        },
                                        "required": ["label", "description"]
                                    }
                                },
                                "multiSelect": { "type": "boolean", "description": "true when several options may be picked." }
                            },
                            "required": ["question", "header", "options"]
                        }
                    }
                },
                "required": ["questions"]
            }
        }
    })
}

/// "Asked you a question", "Asked you 2 questions". Pure.
fn chip(n: usize) -> String {
    if n == 1 {
        "Asked you a question".into()
    } else {
        format!("Asked you {n} questions")
    }
}

/// **How the card ended**, for the model. Pure.
pub fn result_of(questions: &[Question], state: &str, answers: &BTreeMap<String, String>, said: &str) -> Value {
    let display = chip(questions.len());
    match state {
        "answered" => {
            let mut data = json!({ "answers": answers });
            let note = if said.is_empty() {
                "The user answered on the card. Go on with what they picked; their own words, where they wrote some, say what they want."
            } else {
                data["said"] = json!(said);
                "The user answered out loud: what they said is in said, and answers has the options it named. A question with no answer there: take it from what they said."
            };
            json!({ "success": true, "data": data, "note": note, "displayText": display })
        }
        "skipped" => json!({ "success": true, "data": { "answers": {} }, "note": format!("The user left this to you. {CHOOSE}"), "displayText": display }),
        "timedOut" => json!({ "success": true, "data": { "answers": {} }, "note": format!("Nobody answered within {} minutes. {CHOOSE}", WAIT.as_secs() / 60), "displayText": display }),
        _ => json!({ "success": false, "error": format!("The user didn't answer this card (the answer was stopped). {CHOOSE}"), "data": Value::Null, "displayText": display }),
    }
}

/// **Nobody to ask** (`why`): the model asks in words or chooses. Pure.
fn nobody(why: &str) -> Value {
    json!({ "success": false, "error": format!("{why} Ask the user in words in your answer instead, or, if you can, {}", CHOOSE.to_lowercase()), "data": Value::Null, "displayText": "Didn't ask: no card here" })
}

/// Who may see the card: a reason it can't be shown, if one. Pure.
pub fn refused(watched: bool, phone: bool, unattended: bool) -> Option<&'static str> {
    if phone {
        Some("This question came from the user's phone, which shows no cards.")
    } else if unattended {
        Some("This chat runs in Bypass permissions, so nobody is asked while it works.")
    } else if !watched {
        Some("Nobody can see a card for this turn.")
    } else {
        None
    }
}

/// **Ask the person** the call's questions (`args`), showing the card
/// through `live` (`voice`: asked out loud, so said and on the bar), and
/// wait for the answer: the call's result for the model.
pub async fn ask(live: Option<Live<'_>>, args: &Value, voice: bool) -> Value {
    let questions = questions_of(args);
    if questions.is_empty() {
        return json!({ "success": false, "error": "ask_user needs questions: each with its question, a header and two to four options, each with a label and a description.", "data": Value::Null, "displayText": "Didn't ask: no question" });
    }
    if crate::ai::eval::active() {
        return nobody("Nobody answers cards in this evaluation.");
    }
    #[cfg(test)]
    if let Some(r) = confirm::test_reply() {
        let (state, answers, said) = settle(&questions, r);
        return result_of(&questions, state, &answers, &said);
    }
    let Some(live) = live else { return nobody("Nobody can see a card for this turn.") };
    // (A phone's turn is never offered the tool: `Cards::for_turn`.)
    if let Some(why) = refused(true, super::from_phone(), live.sink.unattended()) {
        tracing::info!("Chat: a question card not shown (nobody is asked here)");
        return nobody(why);
    }
    let (id, rx, _pending) = confirm::waiting();
    let card = |state: &str, answers: &BTreeMap<String, String>, said: &str| {
        Event::Confirm(Card {
            id,
            label: questions[0].header.clone(),
            site: String::new(),
            does: ACT.into(),
            state: state.into(),
            act: ACT.into(),
            question: Some(Box::new(QuestionCard { questions: questions.clone(), answers: answers.clone(), said: said.into(), voice })),
            ..Default::default()
        })
    };
    live.sink.send(card("asking", &BTreeMap::new(), ""));
    tracing::info!(id, questions = questions.len(), voice, "Chat: a question card waits for the user");
    let reply = match tokio::time::timeout(WAIT, rx).await {
        Ok(Ok(reply)) => Some(reply),
        Ok(Err(_)) => Some(Reply::Deny),
        Err(_) => None,
    };
    let (state, answers, said) = match reply {
        Some(r) => settle(&questions, r),
        None => ("timedOut", BTreeMap::new(), String::new()),
    };
    live.sink.send(card(state, &answers, &said));
    tracing::info!(id, outcome = state, answered = answers.len(), "Chat: a question card ended");
    result_of(&questions, state, &answers, &said)
}

/// What a reply makes of the card: its state, the answers and the spoken
/// words. Pure.
fn settle(questions: &[Question], reply: Reply) -> (&'static str, BTreeMap<String, String>, String) {
    match reply {
        Reply::Picked(given) => match answers_for(questions, &given) {
            Some(a) => ("answered", a, String::new()),
            None => ("skipped", BTreeMap::new(), String::new()),
        },
        Reply::Said(words) => {
            let words = clip(words.trim(), ANSWER_MAX);
            match heard(questions, &words) {
                Some(a) => ("answered", a, words),
                None => ("skipped", BTreeMap::new(), words),
            }
        }
        // (A yes to a question's card: each question's first option.)
        Reply::Once | Reply::Always => ("answered", questions.iter().map(|q| (q.question.clone(), q.options[0].label.clone())).collect(), String::new()),
        Reply::Deny => ("skipped", BTreeMap::new(), String::new()),
    }
}

/// **The card's answer** (its options, Something else…, Send; or Let Yap
/// decide: `answers` null): `true` if that question was still waiting.
/// `answers`: each question's text → a label, a list of them, or the
/// person's own words.
#[tauri::command]
pub fn chat_question_answer(id: u64, answers: Option<Value>) -> bool {
    match answers.filter(|a| a.as_object().is_some_and(|m| !m.is_empty())) {
        Some(given) => confirm::reply(id, Reply::Picked(given)),
        None => confirm::reply(id, Reply::Deny),
    }
}

/// **A spoken answer** to question `id` (the Voice Assistant's next words in
/// its conversation): `true` if it was still waiting.
pub fn answer_said(id: u64, words: &str) -> bool {
    confirm::reply(id, Reply::Said(words.to_string()))
}

/// **The bar card's buttons** ([`bar_card`]): an option picked, the choice
/// left to the model; Answer in Chat opens the chat (`open`). `true`: it
/// answered the question.
pub fn bar_answer(action: BarAction, questions: Option<&[Question]>) -> bool {
    match action {
        BarAction::Pick(id, n) => {
            let Some(q) = questions.and_then(|qs| qs.first()) else { return false };
            let Some(o) = q.options.get(n) else { return false };
            let mut given = serde_json::Map::new();
            given.insert(q.question.clone(), Value::String(o.label.clone()));
            confirm::reply(id, Reply::Picked(Value::Object(given)))
        }
        BarAction::Decide(id) => confirm::reply(id, Reply::Deny),
        BarAction::Open(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(question: &str, options: &[&str], multi: bool) -> Question {
        Question {
            question: question.into(),
            header: "Format".into(),
            options: options.iter().map(|o| Choice { label: (*o).into(), description: format!("{o}, as it is."), preview: String::new() }).collect(),
            multi_select: multi,
        }
    }

    #[test]
    fn a_calls_questions_are_read_as_claude_codes_are() {
        let input = json!({ "questions": [
            { "question": "Which format should the notes use?", "header": "Format", "multiSelect": false, "options": [
                { "label": "Markdown (Recommended)", "description": "Headings and lists.", "preview": "## Notes\n- one" },
                { "label": "Plain text", "description": "No marks." },
                { "label": "Plain text", "description": "Twice." },
                { "label": "", "description": "No label." }
            ] },
            { "question": "", "header": "Empty", "options": [{ "label": "A", "description": "" }] },
            { "question": "Which sections?", "header": "Sections", "multiSelect": true, "options": [] }
        ] });
        let qs = questions_of(&input);
        assert_eq!(qs.len(), 1, "a question without its text or an option goes");
        assert_eq!(qs[0].options.iter().map(|o| o.label.as_str()).collect::<Vec<_>>(), ["Markdown (Recommended)", "Plain text"]);
        assert_eq!(qs[0].options[0].preview, "## Notes\n- one");
        // A list sent as a JSON string.
        let as_text = json!({ "questions": serde_json::to_string(&input["questions"]).unwrap() });
        assert_eq!(questions_of(&as_text), qs);
        // At most four of four.
        let many = json!({ "questions": (0..6).map(|i| json!({ "question": format!("Q{i}?"), "header": "H", "options": (0..6).map(|j| json!({ "label": format!("O{j}"), "description": "" })).collect::<Vec<_>>() })).collect::<Vec<_>>() });
        let many = questions_of(&many);
        assert_eq!((many.len(), many[0].options.len()), (QUESTIONS_MAX, OPTIONS_MAX));
        // A preview is only written when there is one.
        let v = serde_json::to_value(&qs[0].options[1]).unwrap();
        assert!(v.get("preview").is_none(), "{v}");
        assert_eq!(serde_json::to_value(&qs[0]).unwrap()["multiSelect"], json!(false));
    }

    #[test]
    fn the_answers_are_the_cards_own_questions_picked_or_written() {
        let qs = vec![q("Which format?", &["Markdown", "Plain text"], false), q("Which sections?", &["Summary", "Actions", "Risks"], true)];
        let a = answers_for(&qs, &json!({ "Which format?": "Markdown", "Which sections?": ["Summary", " Risks "], "Not ours?": "x" })).unwrap();
        assert_eq!(a.get("Which format?").map(String::as_str), Some("Markdown"));
        assert_eq!(a.get("Which sections?").map(String::as_str), Some("Summary, Risks"));
        assert!(!a.contains_key("Not ours?"));
        assert!(answers_for(&qs, &json!({ "Which format?": "  " })).is_none(), "nothing answered");
    }

    #[test]
    fn a_spoken_reply_names_an_option_by_its_words_or_its_place() {
        let one = vec![q("Which format should the notes use?", &["Markdown", "Plain text", "A table"], false)];
        let pick = |said: &str| heard(&one, said).map(|a| a.values().cloned().collect::<Vec<_>>());
        assert_eq!(pick("Plain text please"), Some(vec!["Plain text".into()]));
        assert_eq!(pick("markdown"), Some(vec!["Markdown".into()]));
        assert_eq!(pick("The second one"), Some(vec!["Plain text".into()]));
        assert_eq!(pick("option three"), Some(vec!["A table".into()]));
        assert_eq!(pick("the last"), Some(vec!["A table".into()]));
        assert_eq!(pick("2"), Some(vec!["Plain text".into()]));
        // Two named for a question that takes one: the words go as they are.
        assert_eq!(pick("markdown or plain text, I'm not sure"), Some(vec![]));
        // Something else entirely: their own words, nothing picked.
        assert_eq!(pick("Can you make it a bulleted email instead"), Some(vec![]));
        // Left to the model.
        assert_eq!(pick("You decide"), None);
        assert_eq!(pick("I don't mind, either is fine"), None);
        // A name said as one word ("github" for "GitHub").
        let hosts = vec![q("Where should it go?", &["GitHub", "GitLab"], false)];
        assert_eq!(heard(&hosts, "put it on github").unwrap().values().next().map(String::as_str), Some("GitHub"));
        // Several to pick: each named.
        let sections = vec![q("Which sections?", &["Summary", "Actions", "Risks"], true)];
        assert_eq!(heard(&sections, "summary and risks").unwrap().values().next().map(String::as_str), Some("Summary, Risks"));
        // A note in brackets isn't said: "camping stove" is "Camping stove (Recommended)".
        let stove = vec![q("What will you cook on?", &["Camping stove (Recommended)", "Campfire"], false)];
        assert_eq!(heard(&stove, "on the camping stove").unwrap().values().next().map(String::as_str), Some("Camping stove (Recommended)"));
        assert_eq!(said_label("(Recommended)"), "(Recommended)");
        // A longer label with a word left out, when only one option fits.
        let cookers = vec![q("What will you cook on?", &["Gas camping stove", "Campfire", "Portable barbecue"], false)];
        assert_eq!(heard(&cookers, "something vegetarian, on the gas stove").unwrap().values().next().map(String::as_str), Some("Gas camping stove"));
        assert!(heard(&cookers, "on the stove").unwrap().is_empty(), "one word of three isn't enough");
        // Two questions: only labels count (a place could be either's).
        let two = vec![q("Which format?", &["Markdown", "Plain text"], false), q("How long?", &["Short", "Detailed"], false)];
        let a = heard(&two, "plain text, and keep it short").unwrap();
        assert_eq!((a.get("Which format?").map(String::as_str), a.get("How long?").map(String::as_str)), (Some("Plain text"), Some("Short")));
        assert!(heard(&two, "the first one").unwrap().is_empty());
    }

    #[test]
    fn asked_out_loud_yap_says_the_question_and_its_options() {
        let card = QuestionCard { questions: vec![q("Which format should the notes use?", &["Markdown", "Plain text", "A table"], false)], voice: true, ..Default::default() };
        assert_eq!(spoken_line(&card), "Which format should the notes use: Markdown, Plain text or A table? Say one, or pick on the card.");
        let mut two = card.clone();
        two.questions.push(q("How long?", &["Short", "Detailed"], false));
        assert!(spoken_line(&two).ends_with("There's one more question on the card."));
        // The bar: two options are its buttons; more, listed, and Answer in Chat.
        let b = bar_card(5, &QuestionCard { questions: vec![q("Short or long?", &["Short", "Detailed"], false)], ..Default::default() });
        assert_eq!(b.primary.as_ref().map(|a| (a.id.as_str(), a.label.as_str())), Some(("pick:5:0", "Short")));
        assert_eq!(b.secondary.as_ref().map(|a| (a.id.as_str(), a.label.as_str())), Some(("pick:5:1", "Detailed")));
        assert_eq!(b.link.as_ref().map(|a| a.id.as_str()), Some("decide:5"));
        let b = bar_card(6, &card);
        assert_eq!(b.body, "1. Markdown\n2. Plain text\n3. A table");
        assert_eq!(b.primary.as_ref().map(|a| a.id.as_str()), Some("open:6"));
        assert!(b.secondary.is_none());
        assert_eq!(bar_action("pick:5:1"), Some(BarAction::Pick(5, 1)));
        assert_eq!(bar_action("open:6"), Some(BarAction::Open(6)));
        assert_eq!(bar_action("decide:7"), Some(BarAction::Decide(7)));
        assert_eq!(bar_action("allow:7"), None);
    }

    #[test]
    fn the_model_reads_what_was_picked_or_that_its_choice_is_its_own() {
        let qs = vec![q("Which format?", &["Markdown", "Plain text"], false)];
        let mut a = BTreeMap::new();
        a.insert("Which format?".to_string(), "Markdown".to_string());
        let r = result_of(&qs, "answered", &a, "");
        assert_eq!((r["success"].as_bool(), r["data"]["answers"]["Which format?"].as_str(), r["displayText"].as_str()), (Some(true), Some("Markdown"), Some("Asked you a question")));
        assert!(r["data"].get("said").is_none());
        let spoken = result_of(&qs, "answered", &BTreeMap::new(), "the bulleted one");
        assert_eq!(spoken["data"]["said"], "the bulleted one");
        for state in ["skipped", "timedOut"] {
            let r = result_of(&qs, state, &BTreeMap::new(), "");
            assert!(r["note"].as_str().unwrap().contains(CHOOSE), "{state}");
        }
        assert_eq!(result_of(&qs, "refused", &BTreeMap::new(), "")["success"], false);
        // Settled: a pick, the words heard, a yes, a no.
        assert_eq!(settle(&qs, Reply::Picked(json!({ "Which format?": "Plain text" }))).0, "answered");
        assert_eq!(settle(&qs, Reply::Picked(json!({}))).0, "skipped");
        assert_eq!(settle(&qs, Reply::Said("you decide".into())).0, "skipped");
        let (state, answers, said) = settle(&qs, Reply::Said("Plain text".into()));
        assert_eq!((state, answers.get("Which format?").map(String::as_str), said.as_str()), ("answered", Some("Plain text"), "Plain text"));
        assert_eq!(settle(&qs, Reply::Deny).0, "skipped");
        assert_eq!(refused(true, true, false), Some("This question came from the user's phone, which shows no cards."));
        assert!(refused(true, false, true).is_some() && refused(false, false, false).is_some() && refused(true, false, false).is_none());
    }

    #[tokio::test]
    async fn the_card_waits_for_its_answer_and_stays_answered() {
        use crate::chat::stream::Sink;
        #[derive(Default)]
        struct Log(std::sync::Mutex<Vec<Event>>);
        impl Sink for Log {
            fn send(&self, ev: Event) {
                self.0.lock().unwrap().push(ev);
            }
        }
        let log = Log::default();
        let args = json!({ "questions": [{ "question": "Which format?", "header": "Format", "options": [{ "label": "Markdown", "description": "" }, { "label": "Plain text", "description": "" }] }] });
        let fut = ask(Some(Live { sink: &log, beside: None }), &args, false);
        let answering = async {
            loop {
                let id = log.0.lock().unwrap().iter().find_map(|e| match e {
                    Event::Confirm(c) if c.state == "asking" && c.act == ACT => Some(c.id),
                    _ => None,
                });
                if let Some(id) = id {
                    assert!(chat_question_answer(id, Some(json!({ "Which format?": "Plain text" }))));
                    assert!(!chat_question_answer(id, None), "answered once");
                    break;
                }
                tokio::task::yield_now().await;
            }
        };
        let (result, ()) = tokio::join!(fut, answering);
        assert_eq!(result["data"]["answers"]["Which format?"], "Plain text");
        let cards: Vec<Card> = log.0.lock().unwrap().iter().filter_map(|e| match e { Event::Confirm(c) => Some(c.clone()), _ => None }).collect();
        assert_eq!(cards.len(), 2);
        assert_eq!((cards[1].state.as_str(), cards[1].act.as_str()), ("answered", ACT));
        let kept = cards[1].clone().kept();
        let q = kept.question.unwrap();
        assert_eq!(q.answers.get("Which format?").map(String::as_str), Some("Plain text"));
        // On the wire: {kind: "confirm", act: "question", question: {questions, answers}}.
        let v = serde_json::to_value(Event::Confirm(cards[1].clone())).unwrap();
        assert_eq!((v["kind"].as_str(), v["act"].as_str(), v["question"]["questions"][0]["header"].as_str()), (Some("confirm"), Some("question"), Some("Format")));
        // Nobody watching: told to ask in words.
        let r = ask(None, &args, false).await;
        assert_eq!(r["success"], false);
        assert!(r["error"].as_str().unwrap().contains("Ask the user in words"));
        // No questions: said so.
        assert_eq!(ask(None, &json!({}), false).await["displayText"], "Didn't ask: no question");
    }
}

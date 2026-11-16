//! **The outline, seen before the writing starts** (15 Nov 2026, the Claude
//! app's plan review): a new document's `create_document` — its title, its
//! summary and a heading and one line per section — goes to the person as a
//! card in the thread first, with **Go** and **Edit**, and the turn waits
//! ([`WAIT`]). Only then is the document made and each section written, as
//! the outline now says. Settings → Language Models → Chat & Voice
//! Assistant's "Show the outline before writing" (`chat_outline_review`, on
//! by default).
//!
//! - **One card, Chat's own**: it goes through the cards that ask first
//!   (`pages/act/confirm.rs`, act [`ACT`], its outline in [`OutlineCard`]),
//!   so it shows on the answer being written, comes back with the
//!   conversation (`ChatMessage::confirms`), and a ■ Stop drops it, as a
//!   question card does (`cards/ask.rs`).
//! - **Go** (`chat_outline_answer` with nothing): written as outlined.
//!   **Edit**: the headings renamed, reordered, removed or added, each one's
//!   line changed, a note for the writing — `chat_outline_answer` with
//!   `{sections, note}` ([`edited`]); the document is made with those
//!   sections, and the model is told what changed and the note
//!   ([`go_result`]).
//! - **Answered out loud** (the Voice Assistant, `assistant/spoken_ok.rs`):
//!   Yap says the headings ([`spoken_line`]), the Yap bar shows them with
//!   Go ahead and Edit in Chat ([`bar_card`]), and the next thing the person
//!   says in that conversation is the answer ([`heard`]): "go ahead", "looks
//!   good" — written as it is; "drop the second part", "skip the tips" —
//!   those taken out, then written; "no, don't" — not made; anything else
//!   ("add a part on costs") — nothing made yet, the model makes the
//!   outline again from their words and it's shown again
//!   ([`again_result`]), at most [`REVIEWS_MAX`] times a turn.
//! - **Nobody to ask** (an evaluation, a phone's question, Bypass
//!   permissions, nobody watching, the switch off, a turn with no
//!   documents of its own): no card; the document is made at once, as
//!   before.
//! - **No answer in [`WAIT`]**: written as outlined (said on the card).
//!
//! The log has the card's id, how many sections and how it ended, never a
//! heading, a line or a note.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::chat::pages::act::confirm::{self, Card, Reply};
use crate::chat::stream::{Event, Live};

/// The card's act (`confirm::Card::act`).
pub const ACT: &str = "outline";
/// How long the outline waits for the person: as long as a question does.
pub const WAIT: Duration = super::ask::WAIT;
/// How many outlines a turn shows: an outline asked for again out loud
/// shows again, at most this often; after that it's written as it is.
pub const REVIEWS_MAX: usize = 3;
/// How long a note may be.
pub const NOTE_MAX: usize = 2_000;
/// A heading's and a line's length (as `documents::sections_arg` keeps them).
const KEY_MAX: usize = 60;
const INTENT_MAX: usize = 140;

/// **One section of the outline**: its heading, and one line of what it
/// will say.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Section {
    pub key: String,
    pub intent: String,
}

/// **The outline card's part** of a card that asks (`confirm::Card::outline`,
/// act [`ACT`]): the outline as the model made it, and once answered what
/// the person made of it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OutlineCard {
    pub title: String,
    pub summary: String,
    pub sections: Vec<Section>,
    /// **The outline as the person left it** (Edit, or a part dropped out
    /// loud): the sections written. Empty: as the model made it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub changed: Vec<Section>,
    /// The person's note for the writing (Edit's box).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// What the person said, answered out loud.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub said: String,
    /// Asked out loud (the Voice Assistant): said, and on the Yap bar too.
    pub voice: bool,
    /// **A research's plan** (chat/research.rs), not a document's outline:
    /// the card says "Research plan", its Go "Start research". An older
    /// Yap drops it and shows the card as an outline.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub research: bool,
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

/// **The sections of a call or of the card's answer** (`[{key, intent}]`,
/// plain strings, or a JSON string of them): each heading once (case
/// aside), at most `documents::MAX_SECTIONS`. Pure.
pub fn sections_of(v: &Value) -> Vec<Section> {
    let parsed;
    let list = match v {
        Value::String(s) => {
            parsed = serde_json::from_str::<Value>(s).unwrap_or(Value::Null);
            &parsed
        }
        v => v,
    };
    let mut out: Vec<Section> = Vec::new();
    for item in list.as_array().into_iter().flatten() {
        let (key, intent) = match item {
            Value::String(s) => (s.as_str(), ""),
            v => (
                v["key"].as_str().or_else(|| v["heading"].as_str()).or_else(|| v["title"].as_str()).unwrap_or(""),
                v["intent"].as_str().or_else(|| v["about"].as_str()).unwrap_or(""),
            ),
        };
        let key = one_line(key.trim().trim_start_matches('#').trim(), KEY_MAX);
        if key.is_empty() || out.iter().any(|s| s.key.eq_ignore_ascii_case(&key)) {
            continue;
        }
        out.push(Section { key, intent: one_line(intent, INTENT_MAX) });
        if out.len() == crate::chat::documents::MAX_SECTIONS {
            break;
        }
    }
    out
}

/// **The card for a `create_document` call** (`args`): its title, summary
/// and sections. `None`: nothing to show (no title or no section — the call
/// fails on its own, as before). Pure.
pub fn card_of(args: &Value, voice: bool) -> Option<OutlineCard> {
    let title = one_line(args["title"].as_str().unwrap_or(""), 120);
    let sections = sections_of(&args["sections"]);
    if title.is_empty() || sections.is_empty() {
        return None;
    }
    let research = args["research"].as_bool().unwrap_or(false);
    Some(OutlineCard { title, summary: one_line(args["summary"].as_str().unwrap_or(""), 400), sections, voice, research, ..Default::default() })
}

/// **The person's Edit** (`given`: `{sections: [{key, intent}], note}`): the
/// sections they left and their note. `None`: no section left (nothing to
/// write: the card waits). Pure.
pub fn edited(given: &Value) -> Option<(Vec<Section>, String)> {
    let sections = sections_of(&given["sections"]);
    if sections.is_empty() {
        return None;
    }
    let note = given["note"].as_str().map(str::trim).unwrap_or("");
    let note: String = note.chars().take(NOTE_MAX).collect();
    Some((sections, note))
}

/// `call`'s arguments with `sections` in place of its own. Pure.
pub fn with_sections(call: &Value, sections: &[Section]) -> Value {
    let mut out = call.clone();
    out["sections"] = json!(sections.iter().map(|s| json!({ "key": s.key, "intent": s.intent })).collect::<Vec<_>>());
    out
}

// ---- answered out loud ---------------------------------------------------------------------

/// What a spoken reply does to the outline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    /// "Go ahead", "looks good": written as it is.
    Go,
    /// "Drop the second part", "skip the tips": those (their places) out,
    /// then written.
    Without(Vec<usize>),
    /// "No", "don't write it", "cancel": not made.
    Stop,
    /// Anything else: the model makes the outline again from these words.
    Other,
}

/// The words that take a section out.
const DROP: &[&str] = &["drop", "remove", "delete", "skip", "cut", "lose", "get rid of", "leave out", "take out", "scrap", "ditch", "without", "no need for", "don t need", "dont need", "don t want", "dont want"];
/// The words that stop the document.
const STOP: &[&str] = &["don t write it", "dont write it", "don t make it", "dont make it", "cancel", "never mind", "nevermind", "forget it", "stop", "don t bother", "not now", "no thanks", "no thank you"];
/// What a section is called when it's named by its place.
const PART: &[&str] = &["part", "parts", "section", "sections", "heading", "headings", "one", "ones", "bit", "bits", "chapter", "chapters"];
/// The words around a drop that change nothing.
const FILLER: &[&str] = &[
    "the", "a", "and", "please", "then", "but", "go", "ahead", "ok", "okay", "yes", "yeah", "yep", "sure", "just", "it", "that", "this", "those", "these", "write", "rest", "of", "out", "can", "you", "we", "let", "s", "lets", "also", "too", "fine", "good", "great",
    "looks", "sounds", "otherwise", "for", "me", "with", "on", "about", "number", "no", "need", "do", "don", "t", "want", "i", "is", "are", "all", "other", "else", "everything", "keep", "if", "so",
];
const ORDINALS: &[(&str, usize)] = &[
    ("first", 0),
    ("1st", 0),
    ("second", 1),
    ("2nd", 1),
    ("third", 2),
    ("3rd", 2),
    ("fourth", 3),
    ("4th", 3),
    ("fifth", 4),
    ("5th", 4),
    ("sixth", 5),
    ("6th", 5),
    ("seventh", 6),
    ("7th", 6),
    ("eighth", 7),
    ("8th", 7),
    ("ninth", 8),
    ("9th", 8),
    ("tenth", 9),
    ("10th", 9),
];
const NUMBERS: &[(&str, usize)] = &[
    ("one", 0),
    ("1", 0),
    ("two", 1),
    ("2", 1),
    ("three", 2),
    ("3", 2),
    ("four", 3),
    ("4", 3),
    ("five", 4),
    ("5", 4),
    ("six", 5),
    ("6", 5),
    ("seven", 6),
    ("7", 6),
    ("eight", 7),
    ("8", 7),
    ("nine", 8),
    ("9", 8),
    ("ten", 9),
    ("10", 9),
];

/// `s` as folded words joined by spaces. Pure.
fn norm(s: &str) -> String {
    crate::chat::pages::act::rules::words(s).join(" ")
}

/// Whether `hay` (folded words, spaced) has `needle`'s words side by side.
fn has_words(hay: &str, needle: &str) -> bool {
    !needle.is_empty() && format!(" {hay} ").contains(&format!(" {needle} "))
}

/// **What a spoken reply to the outline asks** (`sections`: the card's). Pure.
pub fn heard(sections: &[Section], said: &str) -> Heard {
    let words = norm(said);
    if words.is_empty() {
        return Heard::Other;
    }
    // A plain stop first ("no thanks", "don't write it").
    if STOP.iter().any(|p| has_words(&words, p)) {
        return Heard::Stop;
    }
    let Some(verb) = DROP.iter().find(|p| has_words(&words, p)) else {
        // No drop: a plain go-ahead ("go ahead", "yes please"), a plain no, or something else.
        return match crate::chat::assistant::spoken_ok::reply(said, &[]) {
            Some(true) => Heard::Go,
            Some(false) => Heard::Stop,
            None if crate::agents::consent::go_ahead(said) => Heard::Go,
            None => Heard::Other,
        };
    };
    // What the drop names: places ("the second part", "parts two and four", "the last one") or headings' words.
    let list: Vec<&str> = words.split(' ').filter(|w| !w.is_empty()).collect();
    let n = sections.len();
    let mut out: Vec<usize> = Vec::new();
    let mut used = vec![false; list.len()];
    let push = |out: &mut Vec<usize>, i: usize| {
        if i < n && !out.contains(&i) {
            out.push(i);
        }
    };
    // Headings said by their words ("skip the tips and caveats"), whole first.
    for (i, s) in sections.iter().enumerate() {
        let key = norm(&s.key);
        if !has_words(&words, &key) {
            continue;
        }
        push(&mut out, i);
        let kw: Vec<&str> = key.split(' ').collect();
        for start in 0..(list.len() + 1).saturating_sub(kw.len()) {
            if list[start..start + kw.len()] == kw[..] {
                used[start..start + kw.len()].iter_mut().for_each(|u| *u = true);
            }
        }
    }
    // Then by a word of their own ("skip the tips" for "Tips and caveats"):
    // a word of four letters or more that no other heading has.
    let own_words = |i: usize| -> Vec<String> {
        norm(&sections[i].key).split(' ').filter(|w| w.chars().count() >= 4 && !FILLER.contains(w)).map(str::to_string).collect()
    };
    for i in 0..n {
        if out.contains(&i) {
            continue;
        }
        for w in own_words(i) {
            let shared = (0..n).any(|j| j != i && own_words(j).contains(&w));
            if shared {
                continue;
            }
            for (k, said) in list.iter().enumerate() {
                if !used[k] && *said == w {
                    used[k] = true;
                    push(&mut out, i);
                }
            }
        }
    }
    for (i, w) in list.iter().enumerate() {
        let next_is_part = list.get(i + 1).is_some_and(|nx| PART.contains(nx));
        let prev_is_part = i > 0 && (PART.contains(&list[i - 1]) || list[i - 1] == "number");
        let at_place = ORDINALS.iter().find(|(o, _)| o == w).map(|(_, k)| *k).or_else(|| (*w == "last").then(|| n.saturating_sub(1))).or_else(|| {
            // "part two", "section 3", "two and four" after "parts".
            let num = NUMBERS.iter().find(|(o, _)| o == w).map(|(_, k)| *k)?;
            let after_parts = list[..i].iter().rev().take_while(|x| NUMBERS.iter().any(|(o, _)| o == *x) || **x == "and").count() + 1;
            let lead = i.checked_sub(after_parts).map(|j| list[j]);
            (prev_is_part || next_is_part || lead.is_some_and(|l| PART.contains(&l))).then_some(num)
        });
        if let Some(k) = at_place {
            push(&mut out, k);
            used[i] = true;
        }
    }
    if out.is_empty() {
        return Heard::Other;
    }
    // Anything said beyond the drop and its words asks for more than Yap can do here.
    let verb_words: Vec<&str> = verb.split(' ').collect();
    let rest = list.iter().enumerate().any(|(i, w)| !used[i] && !verb_words.contains(w) && !FILLER.contains(w) && !PART.contains(w) && !NUMBERS.iter().any(|(o, _)| o == w) && *w != "last");
    if rest || out.len() >= n {
        return Heard::Other;
    }
    out.sort_unstable();
    Heard::Without(out)
}

/// **The line Yap says** for an outline asked out loud: its title and
/// headings ("Here's the outline for “Lone wolf build”: Gear, Skills,
/// Levels and Tips. Say go ahead, or what to change."). Pure.
pub fn spoken_line(card: &OutlineCard) -> String {
    let keys: Vec<&str> = card.sections.iter().map(|s| s.key.as_str()).collect();
    let list = match keys.as_slice() {
        [] => String::new(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    };
    format!("Here's the outline for “{}”: {list}. Say go ahead, or what to change.", card.title)
}

/// **The Yap bar's card** for an outline asked out loud (card `id`): its
/// title, the headings numbered, **Go ahead** and **Edit in Chat** (the
/// chat, where its card has Edit). Pure.
pub fn bar_card(id: u64, card: &OutlineCard) -> crate::bar::Card {
    let body: Vec<String> = card.sections.iter().enumerate().map(|(i, s)| format!("{}. {}", i + 1, s.key)).collect();
    crate::bar::Card {
        id: crate::chat::assistant::spoken_ok::CARD.into(),
        icon: "assistant",
        title: one_line(&format!("The outline for “{}”", card.title), 120),
        body: body.join("\n"),
        status: Some("Say go ahead, or what to change".into()),
        primary: Some(crate::bar::CardAction::new(format!("allow:{id}"), "Go ahead")),
        secondary: Some(crate::bar::CardAction::new(format!("open:{id}"), "Edit in Chat")),
        timeout_ms: None,
        ..Default::default()
    }
}

// ---- the review ----------------------------------------------------------------------------

/// **How the review ended.**
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Review {
    /// No card (nobody to ask): made at once, as before.
    Unasked,
    /// Written: as outlined (`changed` empty), or as the person changed it,
    /// with their note and words.
    Go { sections: Vec<Section>, changed: bool, note: String, said: String, timed_out: bool },
    /// Said out loud something Yap can't apply: the model outlines again.
    Again { said: String },
    /// Not to be made (a spoken no, a stop).
    Stop { said: String },
}

/// What a reply makes of the outline (`sections`: the card's). Pure.
pub fn settle(sections: &[Section], reply: Reply) -> Review {
    let go = |sections: Vec<Section>, changed: bool, note: String, said: String| Review::Go { sections, changed, note, said, timed_out: false };
    match reply {
        Reply::Once | Reply::Always => go(sections.to_vec(), false, String::new(), String::new()),
        Reply::Picked(given) => match edited(&given) {
            Some((left, note)) => {
                let changed = left != sections;
                go(left, changed, note, String::new())
            }
            // Nothing left to write: the outline as it was (the page never sends this).
            None => go(sections.to_vec(), false, String::new(), String::new()),
        },
        Reply::Said(words) => {
            let words: String = words.trim().chars().take(NOTE_MAX).collect();
            match heard(sections, &words) {
                Heard::Go => go(sections.to_vec(), false, String::new(), words),
                Heard::Without(out) => {
                    let left: Vec<Section> = sections.iter().enumerate().filter(|(i, _)| !out.contains(i)).map(|(_, s)| s.clone()).collect();
                    go(left, true, String::new(), words)
                }
                Heard::Stop => Review::Stop { said: words },
                Heard::Other => Review::Again { said: words },
            }
        }
        Reply::Deny => Review::Stop { said: String::new() },
    }
}

/// **Who may see the card** (`watched`: a page follows the turn; `phone`:
/// asked from a phone; `unattended`: a Code chat in Bypass permissions):
/// whether it's shown. Pure.
pub fn shown(watched: bool, phone: bool, unattended: bool, eval: bool) -> bool {
    watched && !phone && !unattended && !eval
}

/// **Show the outline of `args` (a `create_document` call) and wait** for
/// the person (`voice`: asked out loud, so said and on the bar; `reviews`:
/// this turn's outlines so far, this one included).
pub async fn review(live: Option<Live<'_>>, args: &Value, voice: bool, reviews: usize) -> Review {
    let Some(mut shown_card) = card_of(args, voice) else { return Review::Unasked };
    if reviews > REVIEWS_MAX {
        tracing::info!("Chat: an outline asked for again too often this turn; written as it is");
        return Review::Unasked;
    }
    #[cfg(test)]
    if let Some(r) = confirm::test_reply() {
        return settle(&shown_card.sections, r);
    }
    let Some(live) = live else { return Review::Unasked };
    if !shown(true, super::from_phone(), live.sink.unattended(), crate::ai::eval::active()) {
        return Review::Unasked;
    }
    let (id, rx, _pending) = confirm::waiting();
    let card = |state: &str, outline: &OutlineCard| {
        Event::Confirm(Card {
            id,
            label: outline.title.clone(),
            site: String::new(),
            does: ACT.into(),
            state: state.into(),
            act: ACT.into(),
            outline: Some(Box::new(outline.clone())),
            ..Default::default()
        })
    };
    live.sink.send(card("asking", &shown_card));
    crate::chat::activity::tell(live.sink, if shown_card.research { crate::chat::activity::research_waits() } else { crate::chat::activity::outline_waits() });
    tracing::info!(id, sections = shown_card.sections.len(), voice, "Chat: a document's outline waits for the user");
    let reply = match tokio::time::timeout(WAIT, rx).await {
        Ok(Ok(reply)) => Some(reply),
        Ok(Err(_)) => Some(Reply::Deny),
        Err(_) => None,
    };
    let review = match reply {
        Some(r) => settle(&shown_card.sections, r),
        None => Review::Go { sections: shown_card.sections.clone(), changed: false, note: String::new(), said: String::new(), timed_out: true },
    };
    let state = match &review {
        Review::Go { timed_out: true, .. } => "timedOut",
        Review::Go { changed: true, sections, note, said, .. } => {
            shown_card.changed = sections.clone();
            shown_card.note = note.clone();
            shown_card.said = said.clone();
            "changed"
        }
        Review::Go { note, said, .. } => {
            shown_card.note = note.clone();
            shown_card.said = said.clone();
            "allowed"
        }
        Review::Again { said } => {
            shown_card.said = said.clone();
            "redo"
        }
        Review::Stop { said } => {
            shown_card.said = said.clone();
            "refused"
        }
        Review::Unasked => "allowed",
    };
    live.sink.send(card(state, &shown_card));
    tracing::info!(id, outcome = state, "Chat: a document's outline answered");
    review
}

/// The outline's headings in a line ("Gear, Skills and Tips"). Pure.
fn headings(sections: &[Section]) -> String {
    let keys: Vec<String> = sections.iter().map(|s| format!("\"{}\"", s.key)).collect();
    match keys.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// **The model's result once the outline is answered**: `made` (the
/// document's own `create_document` result) with what the person made of
/// the outline — read and approved, changed (the sections now, in order;
/// one they added has a heading only), their note to follow, or not
/// answered in time. Pure.
pub fn go_result(mut made: Value, review: &Review) -> Value {
    let Review::Go { sections, changed, note, said, timed_out } = review else { return made };
    if made["success"] != json!(true) {
        return made;
    }
    let mut lines: Vec<String> = Vec::new();
    if *timed_out {
        lines.push("The user didn't answer the outline in time: write it as outlined.".into());
    } else if *changed {
        lines.push(format!(
            "The user changed the outline before you wrote it: the sections are now {}, in this order. Write each as it stands; one with no line of its own is theirs: decide what it covers from its heading. Don't add back a section they removed.",
            headings(sections)
        ));
    } else {
        lines.push("The user read the outline and said go: write it as outlined.".into());
    }
    if !said.is_empty() {
        lines.push(format!("They said: “{said}”."));
    }
    if !note.is_empty() {
        lines.push(format!("Their note for the writing, to follow in every section: “{note}”"));
    }
    made["data"]["outline"] = json!(lines.join(" "));
    made
}

/// **The model's result when the person asked out loud for another
/// outline** (`said`): nothing made yet; make it again as they said, and
/// it's shown again. Pure.
pub fn again_result(said: &str) -> Value {
    json!({
        "success": false,
        "error": format!("The user answered your outline out loud: “{said}”. Nothing was made yet: call create_document again, with the title, summary and sections changed as they asked; it goes to them once more before you write."),
        "data": Value::Null,
        "displayText": "",
    })
}

/// **The model's result when the person said not to make it** (`said`).
/// Pure.
pub fn stop_result(said: &str) -> Value {
    let why = if said.is_empty() { "The user stopped this document before it was written.".to_string() } else { format!("The user said not to write this document: “{said}”.") };
    json!({
        "success": false,
        "error": format!("{why} Nothing was made: don't make it, and don't write it in the chat either. Answer in one short sentence, asking what they'd like instead."),
        "data": Value::Null,
        "displayText": "Didn't start the document: you said not to",
    })
}

/// **`create_document` with its outline seen first** (`args`; `make`: the
/// tool itself): the outline reviewed ([`review`]), then the document made
/// as the person left it ([`go_result`]) — or nothing made, the model told
/// why ([`again_result`], [`stop_result`]). Nobody to ask: made at once.
pub async fn create(live: Option<Live<'_>>, args: &Value, voice: bool, reviews: usize, make: impl FnOnce(&Value) -> Value) -> Value {
    let r = review(live, args, voice, reviews).await;
    match &r {
        Review::Unasked => make(args),
        Review::Go { sections, .. } => go_result(make(&with_sections(args, sections)), &r),
        Review::Again { said } => again_result(said),
        Review::Stop { said } => stop_result(said),
    }
}

/// **The card's answer** (its Go, or Edit's Go with `outline`: `{sections:
/// [{key, intent}], note}`): `true` if that outline was still waiting.
#[tauri::command]
pub fn chat_outline_answer(id: u64, outline: Option<Value>) -> bool {
    match outline.filter(|o| o.is_object()) {
        Some(given) => confirm::reply(id, Reply::Picked(given)),
        None => confirm::reply(id, Reply::Once),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sec(key: &str, intent: &str) -> Section {
        Section { key: key.into(), intent: intent.into() }
    }

    fn outline() -> Vec<Section> {
        vec![sec("Gear", "what to wear"), sec("Skills", "the ones to take"), sec("Levels", "level by level"), sec("Tips and caveats", "what to watch for")]
    }

    #[test]
    fn a_create_document_call_is_an_outline_card() {
        let args = json!({ "title": "Lone wolf build", "summary": "A solo build.", "sections": [{ "key": "Gear", "intent": "what to wear" }, "Skills", { "key": "gear", "intent": "again" }] });
        let card = card_of(&args, false).expect("a card");
        assert_eq!(card.title, "Lone wolf build");
        assert_eq!(card.sections, [sec("Gear", "what to wear"), sec("Skills", "")], "each heading once");
        assert!(card_of(&json!({ "title": "x", "sections": [] }), false).is_none(), "nothing to show");
        assert!(card_of(&json!({ "sections": ["a"] }), false).is_none());
        // As a string of JSON, as small models send it.
        let as_text = json!({ "title": "T", "sections": "[{\"key\":\"A\",\"intent\":\"a\"}]" });
        assert_eq!(card_of(&as_text, false).unwrap().sections, [sec("A", "a")]);
    }

    #[test]
    fn edit_keeps_the_persons_sections_and_note() {
        let given = json!({ "sections": [{ "key": "Levels", "intent": "level by level" }, { "key": "## Gear", "intent": "" }, { "key": "Costs", "intent": "" }], "note": "  Keep it short.  " });
        let (left, note) = edited(&given).unwrap();
        assert_eq!(left, [sec("Levels", "level by level"), sec("Gear", ""), sec("Costs", "")]);
        assert_eq!(note, "Keep it short.");
        assert!(edited(&json!({ "sections": [] })).is_none(), "nothing left to write");
        let args = with_sections(&json!({ "title": "T", "summary": "S", "sections": ["x"] }), &left);
        assert_eq!(args["sections"][2], json!({ "key": "Costs", "intent": "" }));
        assert_eq!(args["title"], "T");
    }

    #[test]
    fn settling_a_reply() {
        let s = outline();
        assert_eq!(settle(&s, Reply::Once), Review::Go { sections: s.clone(), changed: false, note: String::new(), said: String::new(), timed_out: false });
        // Edit's Go with nothing changed but a note: not "changed".
        let same = json!({ "sections": s.iter().map(|x| json!({ "key": x.key, "intent": x.intent })).collect::<Vec<_>>(), "note": "For a beginner" });
        assert!(matches!(settle(&s, Reply::Picked(same)), Review::Go { changed: false, ref note, .. } if note == "For a beginner"));
        let fewer = json!({ "sections": [{ "key": "Gear", "intent": "what to wear" }] });
        assert!(matches!(settle(&s, Reply::Picked(fewer)), Review::Go { changed: true, ref sections, .. } if sections.len() == 1));
        assert!(matches!(settle(&s, Reply::Said("go ahead".into())), Review::Go { changed: false, ref said, .. } if said == "go ahead"));
        assert!(matches!(settle(&s, Reply::Said("drop the second part".into())), Review::Go { changed: true, ref sections, .. } if sections.iter().map(|x| x.key.as_str()).collect::<Vec<_>>() == ["Gear", "Levels", "Tips and caveats"]));
        assert_eq!(settle(&s, Reply::Said("add a part about costs".into())), Review::Again { said: "add a part about costs".into() });
        assert_eq!(settle(&s, Reply::Said("no thanks".into())), Review::Stop { said: "no thanks".into() });
        assert_eq!(settle(&s, Reply::Deny), Review::Stop { said: String::new() });
    }

    #[test]
    fn what_a_spoken_reply_does_to_the_outline() {
        let s = outline();
        for go in ["Go ahead", "yes please", "Looks good", "sounds good, go for it", "yeah"] {
            assert_eq!(heard(&s, go), Heard::Go, "{go}");
        }
        assert_eq!(heard(&s, "Drop the second part"), Heard::Without(vec![1]));
        assert_eq!(heard(&s, "go ahead but skip the last one"), Heard::Without(vec![3]));
        assert_eq!(heard(&s, "remove parts two and four please"), Heard::Without(vec![1, 3]));
        assert_eq!(heard(&s, "Skip the tips and caveats."), Heard::Without(vec![3]));
        assert_eq!(heard(&s, "lose the gear section, then write it"), Heard::Without(vec![0]));
        assert_eq!(heard(&s, "cut section 3"), Heard::Without(vec![2]));
        assert_eq!(heard(&s, "go ahead without the tips"), Heard::Without(vec![3]), "a word of its own heading");
        // More than a drop: the model makes it again.
        assert_eq!(heard(&s, "drop the second part and add one on costs"), Heard::Other);
        assert_eq!(heard(&s, "make it shorter"), Heard::Other);
        assert_eq!(heard(&s, "drop the seventh part"), Heard::Other, "no such part");
        assert_eq!(heard(&s, "drop all of them"), Heard::Other);
        assert_eq!(heard(&s, "drop the first, second, third and fourth"), Heard::Other, "nothing would be left");
        for stop in ["No thanks", "never mind", "don't write it", "cancel that", "no"] {
            assert_eq!(heard(&s, stop), Heard::Stop, "{stop}");
        }
    }

    #[test]
    fn its_spoken_line_and_bar_card() {
        let card = OutlineCard { title: "Lone wolf build".into(), sections: vec![sec("Gear", ""), sec("Skills", ""), sec("Tips", "")], voice: true, ..Default::default() };
        assert_eq!(spoken_line(&card), "Here's the outline for “Lone wolf build”: Gear, Skills and Tips. Say go ahead, or what to change.");
        let bar = bar_card(7, &card);
        assert_eq!(bar.title, "The outline for “Lone wolf build”");
        assert_eq!(bar.body, "1. Gear\n2. Skills\n3. Tips");
        assert_eq!(bar.primary.as_ref().map(|a| (a.id.as_str(), a.label.as_str())), Some(("allow:7", "Go ahead")));
        assert_eq!(bar.secondary.as_ref().map(|a| (a.id.as_str(), a.label.as_str())), Some(("open:7", "Edit in Chat")));
        // Its buttons are the ones every card that asks has.
        assert_eq!(crate::chat::assistant::spoken_ok::action_of("allow:7"), Some((7, true)));
        assert_eq!(crate::chat::cards::ask::bar_action("open:7"), Some(crate::chat::cards::ask::BarAction::Open(7)));
    }

    #[test]
    fn the_models_result_says_what_the_person_made_of_it() {
        let made = json!({ "success": true, "data": { "id": 4, "sections": ["Levels", "Costs"] }, "displayText": "Started" });
        let changed = Review::Go { sections: vec![sec("Levels", "x"), sec("Costs", "")], changed: true, note: "Keep it short".into(), said: String::new(), timed_out: false };
        let r = go_result(made.clone(), &changed);
        let told = r["data"]["outline"].as_str().unwrap();
        assert!(told.contains("the sections are now \"Levels\" and \"Costs\", in this order"), "{told}");
        assert!(told.contains("Their note for the writing, to follow in every section: “Keep it short”"), "{told}");
        assert_eq!(r["data"]["id"], 4);
        let as_is = go_result(made.clone(), &Review::Go { sections: vec![], changed: false, note: String::new(), said: "go ahead".into(), timed_out: false });
        assert!(as_is["data"]["outline"].as_str().unwrap().starts_with("The user read the outline and said go"));
        let late = go_result(made.clone(), &Review::Go { sections: vec![], changed: false, note: String::new(), said: String::new(), timed_out: true });
        assert!(late["data"]["outline"].as_str().unwrap().contains("didn't answer the outline in time"));
        // A failed make stays as it was.
        let failed = json!({ "success": false, "error": "title is required" });
        assert_eq!(go_result(failed.clone(), &changed), failed);
        assert!(again_result("add costs")["error"].as_str().unwrap().contains("call create_document again"));
        assert_eq!(again_result("x")["displayText"], "", "no chip of its own");
        assert!(stop_result("no")["error"].as_str().unwrap().contains("don't make it"));
    }

    #[test]
    fn nobody_to_ask_means_no_card() {
        assert!(shown(true, false, false, false));
        assert!(!shown(false, false, false, false), "nobody watching");
        assert!(!shown(true, true, false, false), "a phone's question");
        assert!(!shown(true, false, true, false), "Bypass permissions");
        assert!(!shown(true, false, false, true), "an evaluation");
    }
}

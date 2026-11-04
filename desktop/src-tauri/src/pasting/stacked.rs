//! **Stacked messages** (2026-10-29; Wispr Flow's, by behaviour, on a
//! spoken separator): in a chat app, saying "new message" (or "next
//! message") between the parts of one dictation makes each part a message
//! of its own, sent one after another.
//!
//! - **The trigger is said, never guessed.** Wispr Flow splits a dictation
//!   at every sentence and shows the messages for a click to send; Yap has
//!   no such window, so it splits only where the person said the
//!   separator. An AI deciding where one message ends would send a
//!   half-thought now and then, and a small model on this PC can't be
//!   relied on for it. The separator counts only as a sentence or a clause
//!   of its own ("Hi Sam. New message. Are you free?", "Hi Sam, new
//!   message, are you free?"), never inside one ("I got a new message from
//!   Sam", "New message from Sam: hi").
//! - **It only sends what would be sent anyway**: the dictation goes into
//!   a chat app and is sent (auto-submit on, or "press enter" said;
//!   `app_kind::submit_key`). Then each message is pasted and sent with the
//!   submit key, the first only once the whole text is ready. Without a
//!   send, the separators become line breaks and nothing is sent.
//! - **Off by default** (`config.stacked_messages`): a message can't be
//!   taken back once sent, and the phrase is one the person has to know.
//!
//! The splitting is pure and unit-tested; the paste-and-send loop is
//! `compose.rs`.

/// The spoken separators, as two words each.
const SEPARATORS: &[(&str, &str)] = &[("new", "message"), ("next", "message")];

/// Words that carry a sentence on after "new message" with nothing between
/// ("New message from Sam", "a new message arrived"): no separator then.
const CARRIES_ON: &[&str] = &[
    "from", "to", "for", "about", "in", "on", "at", "with", "by", "of", "button", "window", "notification",
    "notifications", "icon", "alert", "sound", "tone", "thread", "box", "field", "count", "counter", "badge",
    "said", "says", "arrived", "arrives", "came", "comes", "pops", "popped", "is", "was", "has", "had", "will",
    "and", "or", "that", "which", "who",
];

/// What comes before a separator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Before {
    /// The text's start, a sentence's end or a line break.
    Sentence,
    /// A comma, semicolon, colon or dash: a clause's end.
    Clause,
    /// A word: the phrase is inside a sentence.
    Word,
}

/// A word's byte range in the text.
type Span = (usize, usize);

/// The words of `text`: runs of letters, digits and apostrophes.
fn words(text: &str) -> Vec<Span> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        let part = c.is_alphanumeric() || matches!(c, '\'' | '’');
        match (part, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, text.len()));
    }
    out
}

fn before(text: &str, at: usize) -> Before {
    let head = text[..at].trim_end_matches([' ', '\t']);
    match head.chars().next_back() {
        None => Before::Sentence,
        Some('.' | '!' | '?' | '…' | '\n' | '\r') => Before::Sentence,
        Some(',' | ';' | ':' | '—' | '–' | '-') => Before::Clause,
        _ => Before::Word,
    }
}

/// Where a separator at `words[i]`, `words[i + 1]` is, if it is one: the
/// byte range to cut out (the phrase and the punctuation after it).
fn separator_at(text: &str, ws: &[Span], i: usize) -> Option<Span> {
    let (a, b) = (ws.get(i)?, ws.get(i + 1)?);
    let first = &text[a.0..a.1];
    let second = &text[b.0..b.1];
    let said = SEPARATORS.iter().any(|(x, y)| first.eq_ignore_ascii_case(x) && second.eq_ignore_ascii_case(y));
    // Only a space or a dash between the two words.
    if !said || !text[a.1..b.0].chars().all(|c| matches!(c, ' ' | '\t' | '-')) || a.1 == b.0 {
        return None;
    }
    let rest = &text[b.1..];
    let after_trimmed = rest.trim_start_matches([' ', '\t']);
    let next = after_trimmed.chars().next();
    let punct_after = next.is_none_or(|c| matches!(c, '.' | '!' | '?' | '…' | ',' | ';' | ':' | '—' | '–' | '-' | '\n' | '\r'));
    let ok = match before(text, a.0) {
        Before::Sentence if punct_after => true,
        // "Hi Sam. New message are you free": a sentence's start, then a
        // word — unless the word carries the sentence on ("New message
        // from Sam").
        Before::Sentence => {
            let next_word = ws.get(i + 2).map(|w| text[w.0..w.1].to_lowercase()).unwrap_or_default();
            !CARRIES_ON.contains(&next_word.as_str())
        }
        Before::Clause => punct_after,
        Before::Word => false,
    };
    if !ok {
        return None;
    }
    // The cut takes the punctuation and spaces after the phrase too.
    let tail = rest.len() - rest.trim_start_matches(|c: char| c.is_whitespace() || ".!?…,;:—–-".contains(c)).len();
    Some((a.0, b.1 + tail))
}

/// A part of the text as a message: the punctuation that tied it to the
/// separator (a comma, a dash) off its end, a first word in lower case
/// capitalised (it starts a message now).
fn tidy(part: &str, first: bool) -> String {
    let t = part.trim().trim_end_matches([',', ';', ':', '—', '–', '-', ' ']).trim_end();
    if t.is_empty() || first {
        return t.to_string();
    }
    let word: &str = t.split_whitespace().next().unwrap_or("");
    let plain_lower = word.chars().filter(|c| c.is_alphabetic()).all(char::is_lowercase);
    let mut chars = t.chars();
    match chars.next() {
        Some(c) if c.is_lowercase() && plain_lower => c.to_uppercase().chain(chars).collect(),
        _ => t.to_string(),
    }
}

/// The dictation's messages: `text` cut where the person said "new
/// message" or "next message" as a sentence or clause of its own, the
/// separators taken out, empty parts dropped. One part when none was said.
pub fn split(text: &str) -> Vec<String> {
    let ws = words(text);
    let mut parts = Vec::new();
    let mut from = 0usize;
    let mut i = 0usize;
    while i + 1 < ws.len() {
        if ws[i].0 < from {
            i += 1;
            continue;
        }
        if let Some((start, end)) = separator_at(text, &ws, i) {
            parts.push(&text[from..start]);
            from = end;
            i += 2;
        } else {
            i += 1;
        }
    }
    parts.push(&text[from..]);
    let mut out: Vec<String> = Vec::new();
    for p in parts {
        let t = tidy(p, out.is_empty());
        if !t.is_empty() {
            out.push(t);
        }
    }
    out
}

/// Did the person say a separator? (Two or more messages, or the phrase
/// taken out of a single one.)
pub fn said(text: &str) -> bool {
    let ws = words(text);
    (0..ws.len().saturating_sub(1)).any(|i| separator_at(text, &ws, i).is_some())
}

/// What a dictation with stacked messages on becomes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stacked {
    /// No separator said, or not a chat app: the text as it is.
    AsIs,
    /// Messages to paste and send one after another.
    Send(Vec<String>),
    /// Not sent (no auto-submit, no "press enter"): the messages on lines
    /// of their own, in one paste, for the person to send.
    Lines(String),
}

/// Stacked messages for a dictation (`text`, after cleanup; `said`: its
/// words as said, when cleanup changed them) going into a chat app
/// (`chat`) that is (`sends`) or isn't sent. When cleanup dropped or
/// rewrote the separators the person said, the words as said are split
/// instead (corrections and snippets, no cleanup), so the messages they
/// asked for still go as messages.
pub fn decide(text: &str, said_text: Option<&str>, chat: bool, sends: bool) -> Stacked {
    if !chat {
        return Stacked::AsIs;
    }
    let source = if said(text) {
        text
    } else {
        match said_text.filter(|s| said(s)) {
            Some(s) => {
                tracing::info!("Stacked messages: cleanup took the separators out; using the words as said");
                s
            }
            None => return Stacked::AsIs,
        }
    };
    let messages = split(source);
    match (messages.len(), sends) {
        (0, _) => Stacked::Lines(String::new()),
        (1, _) => Stacked::Lines(messages.into_iter().next().unwrap_or_default()),
        (_, true) => Stacked::Send(messages),
        (_, false) => Stacked::Lines(messages.join("\n")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_separator_said_as_its_own_sentence_or_clause_splits() {
        assert_eq!(split("Hi Sam. New message. Are you free at three?"), ["Hi Sam.", "Are you free at three?"]);
        assert_eq!(split("Hi Sam, new message, are you free?"), ["Hi Sam", "Are you free?"]);
        assert_eq!(split("Hi Sam.\nNew message\nAre you free?"), ["Hi Sam.", "Are you free?"]);
        assert_eq!(split("Hi Sam! New message: are you free?"), ["Hi Sam!", "Are you free?"]);
        assert_eq!(split("Hi Sam - new message - are you free?"), ["Hi Sam", "Are you free?"]);
        assert_eq!(split("One. Next message. Two. New message. Three."), ["One.", "Two.", "Three."]);
        assert_eq!(split("Hi Sam. NEW MESSAGE. Bye."), ["Hi Sam.", "Bye."]);
        // A sentence's start, then the next message straight on.
        assert_eq!(split("Hi Sam. New message are you free?"), ["Hi Sam.", "Are you free?"]);
    }

    #[test]
    fn the_phrase_inside_a_sentence_is_dictated() {
        for text in [
            "I got a new message from Sam.",
            "New message from Sam: hi.",
            "Write a new message, then send it.",
            "Click New message to start one.",
            "hi sam new message are you free",
            "The new message button is broken.",
            "Did you see my new message?",
            "Send the next message tomorrow.",
            "A newer message. Bye.",
            "New messages arrived.",
        ] {
            assert_eq!(split(text), [text.trim_end_matches([',', ';'])], "{text}");
            assert!(!said(text), "{text}");
        }
    }

    #[test]
    fn empty_parts_go() {
        assert_eq!(split("New message. Hi"), ["Hi"]);
        assert_eq!(split("Hi. New message."), ["Hi."]);
        assert_eq!(split("Hi. New message. New message. Bye"), ["Hi.", "Bye"]);
        assert_eq!(split("New message."), Vec::<String>::new());
        assert!(said("New message."));
    }

    #[test]
    fn a_message_keeps_a_word_written_its_own_way() {
        // "iPhone" stays as it is; a plain lower-case first word gets a capital.
        assert_eq!(split("Look, new message, iPhone sale today"), ["Look", "iPhone sale today"]);
        assert_eq!(split("Look, new message, ok then"), ["Look", "Ok then"]);
    }

    #[test]
    fn only_a_chat_app_that_sends_gets_messages() {
        let text = "Hi Sam. New message. Are you free?";
        assert_eq!(decide(text, None, true, true), Stacked::Send(vec!["Hi Sam.".into(), "Are you free?".into()]));
        // Not sent: one paste, the messages on lines of their own.
        assert_eq!(decide(text, None, true, false), Stacked::Lines("Hi Sam.\nAre you free?".into()));
        // Not a chat app: as it is.
        assert_eq!(decide(text, None, false, true), Stacked::AsIs);
        // Nothing said: as it is.
        assert_eq!(decide("Hi Sam. Are you free?", None, true, true), Stacked::AsIs);
        // One message with the phrase taken off.
        assert_eq!(decide("Hi Sam. New message.", None, true, true), Stacked::Lines("Hi Sam.".into()));
    }

    #[test]
    fn separators_cleanup_dropped_come_from_the_words_as_said() {
        let cleaned = "Hi Sam. Are you free at three?";
        let said_text = "Hi Sam, new message, are you free at three?";
        assert_eq!(
            decide(cleaned, Some(said_text), true, true),
            Stacked::Send(vec!["Hi Sam".into(), "Are you free at three?".into()])
        );
        // Neither has one: as it is.
        assert_eq!(decide(cleaned, Some("hi sam are you free at three"), true, true), Stacked::AsIs);
    }
}

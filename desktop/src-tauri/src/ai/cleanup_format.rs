//! **Smart formatting** (2026-10-25; Wispr Flow's, by behaviour): a spoken
//! list comes out as a list, and an email's greeting and sign-off on lines of
//! their own — never a plain sentence that only mentions numbers.
//!
//! **Asked only when it's said.** Rust looks for the words a list or an
//! email frame is made of ([`list_cue`], [`email_frame`]) and only then adds
//! one line just before the transcript ([`hint`], beside the style's and the
//! strength's): a dictation without them is cleaned exactly as before, and
//! most never carry the line. The model decides whether words like "first …
//! second" really list items ("the first and second floors are closed" stays
//! a sentence: the line says so, and the evaluations hold it to it,
//! docs/model-evals.md "Cleanup strength and formatting").
//!
//! At Medium and High only (`Strength::formats`): Light keeps the words and
//! their layout as said. The person's own body (Prompt Studio) is untouched:
//! the line is Yap's own layer, like the style's.

use crate::ai::cleanup_strength::Strength;
use crate::ai::cleanup_styles::{Category, Style, Tone};

/// The kind of list a dictation seems to say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListCue {
    /// "first … second …", "one … two … three …", "number one … number two".
    Numbered,
    /// "bullet points: …".
    Bullets,
}

/// Lower-case words, apostrophes kept.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Ordinal markers in the order a list says them (a marker's place).
fn ordinal(w: &str) -> Option<usize> {
    Some(match w {
        "first" | "firstly" => 1,
        "second" | "secondly" => 2,
        "third" | "thirdly" => 3,
        "fourth" | "fourthly" => 4,
        "fifth" | "fifthly" => 5,
        _ => return None,
    })
}

fn cardinal(w: &str) -> Option<usize> {
    Some(match w {
        "one" | "1" => 1,
        "two" | "2" => 2,
        "three" | "3" => 3,
        "four" | "4" => 4,
        "five" | "5" => 5,
        "six" | "6" => 6,
        _ => return None,
    })
}

/// Do the markers `marks` (each a word's place in the list) run 1, 2(, 3…)
/// in order, at least `need` of them, each with at least one word between
/// it and the next?
fn runs_in_order(marks: &[(usize, usize)], need: usize) -> bool {
    // (position in the text, marker value): the longest run 1, 2, 3… with
    // gaps of 2+ words.
    let mut best = 0;
    for (i, &(pos, v)) in marks.iter().enumerate() {
        if v != 1 {
            continue;
        }
        let (mut want, mut at, mut n) = (2, pos, 1);
        for &(p, w) in &marks[i + 1..] {
            if w == want && p >= at + 2 {
                want += 1;
                at = p;
                n += 1;
            }
        }
        best = best.max(n);
    }
    best >= need
}

/// The words a spoken list is made of, if a dictation has them (pure):
/// "bullet points" (or "bullets", "bulleted list"); "number one … number
/// two"; ordinals in order ("first … second", "firstly … secondly"); or
/// "one … two … three" in order with words between. A sentence without
/// any of them is never asked to be a list. Whether they really list items
/// is the model's call ([`hint`]).
pub fn list_cue(text: &str) -> Option<ListCue> {
    let w = words(text);
    let has = |a: &str, b: &str| w.windows(2).any(|p| p[0] == a && p[1] == b);
    if has("bullet", "points")
        || has("bullet", "point")
        || has("bulleted", "list")
        || w.iter().any(|x| x == "bullets")
    {
        return Some(ListCue::Bullets);
    }
    // "number one … number two".
    let numbered: Vec<(usize, usize)> = w
        .windows(2)
        .enumerate()
        .filter(|(_, p)| p[0] == "number")
        .filter_map(|(i, p)| cardinal(&p[1]).map(|v| (i, v)))
        .collect();
    if runs_in_order(&numbered, 2) {
        return Some(ListCue::Numbered);
    }
    // Ordinals that start an item, not "the first time" or "our second
    // floor" (a determiner before it: an adjective in a sentence).
    let ordinals: Vec<(usize, usize)> = w
        .iter()
        .enumerate()
        .filter(|&(i, _)| i == 0 || !DETERMINERS.contains(&w[i - 1].as_str()))
        .filter_map(|(i, x)| ordinal(x).map(|v| (i, v)))
        .collect();
    if runs_in_order(&ordinals, 2) {
        return Some(ListCue::Numbered);
    }
    // "one … two … three …" only after words that lead into a list ("my
    // priorities are one …", "three things one …") or at the very start:
    // never "I have one cat, two dogs and three fish".
    let cardinals: Vec<(usize, usize)> = w
        .iter()
        .enumerate()
        .filter_map(|(i, x)| cardinal(x).map(|v| (i, v)))
        .collect();
    let led_in = cardinals
        .iter()
        .any(|&(i, v)| v == 1 && (i == 0 || LEAD_INS.contains(&w[i - 1].as_str())));
    if led_in && runs_in_order(&cardinals, 3) {
        return Some(ListCue::Numbered);
    }
    None
}

/// Words before an ordinal that make it part of a sentence ("the first
/// time", "our second office").
const DETERMINERS: &[&str] = &[
    "the", "a", "an", "its", "my", "our", "your", "their", "his", "her", "this", "that", "every",
    "each",
];

/// Words a spoken "one, two, three" list follows.
const LEAD_INS: &[&str] = &[
    "are",
    "is",
    "were",
    "was",
    "things",
    "steps",
    "points",
    "items",
    "reasons",
    "priorities",
    "options",
    "goals",
    "list",
    "agenda",
    "tasks",
    "questions",
    "ideas",
    "issues",
    "problems",
    "parts",
    "stages",
    "actions",
    "plan",
];

/// Greetings an email may open with.
const GREETINGS: &[&str] = &[
    "hi",
    "hello",
    "hey",
    "dear",
    "morning",
    "afternoon",
    "evening",
    "hiya",
];

/// Sign-offs an email may end with (their first words).
const SIGN_OFFS: &[&[&str]] = &[
    &["thanks"],
    &["thank", "you"],
    &["many", "thanks"],
    &["cheers"],
    // Not "best" alone: "we hired the best team".
    &["best", "regards"],
    &["best", "wishes"],
    &["kind", "regards"],
    &["regards"],
    &["warm", "regards"],
    &["all", "the", "best"],
    &["speak", "soon"],
    &["talk", "soon"],
    &["see", "you", "soon"],
    &["yours", "sincerely"],
    &["sincerely"],
    &["take", "care"],
];

/// Does a dictation open with a greeting and/or close with a sign-off
/// (possibly followed by a name of a word or two)? `(greeting, sign_off)`.
/// Pure.
pub fn email_frame(text: &str) -> (bool, bool) {
    let w = words(text);
    let skip_fillers = |i: usize| {
        let mut i = i;
        while w.get(i).is_some_and(|x| {
            matches!(
                x.as_str(),
                "um" | "uh" | "er" | "erm" | "so" | "okay" | "ok"
            )
        }) {
            i += 1;
        }
        i
    };
    let start = skip_fillers(0);
    let greeting = w
        .get(start)
        .is_some_and(|x| GREETINGS.contains(&x.as_str()))
        || (w.get(start).is_some_and(|x| x == "good")
            && w.get(start + 1)
                .is_some_and(|x| matches!(x.as_str(), "morning" | "afternoon" | "evening")));
    // A sign-off in the last few words: the phrase, then at most two more
    // (a name), with at least three words said before it.
    let n = w.len();
    let sign_off = SIGN_OFFS.iter().any(|s| {
        (0..=2).any(|name| {
            n.checked_sub(name).is_some_and(|end| {
                end >= s.len() + 3
                    && w[end - s.len()..end]
                        .iter()
                        .map(String::as_str)
                        .eq(s.iter().copied())
            })
        })
    });
    (greeting, sign_off)
}

/// The line a numbered list gets (measured, docs/model-evals.md "Cleanup
/// strength and formatting": "If the speaker is listing items…" made the
/// 4B list "one cat, two dogs"; without "even inside an email" it wrote
/// an email's items as prose).
pub const NUMBERED_LINE: &str = "The speaker may be dictating a numbered list: if \"first\", \"second\"… or \"one\", \
\"two\", \"three\"… each start a new item (an action or a point), write the items as a numbered list, one per line \
(\"1. …\"), even inside an email, after any sentence leading into it, without the spoken numbers. Numbers that count \
things (\"one cat, two dogs\") or say an order inside a sentence (\"the first time\") are not a list: keep those as a \
sentence.";

/// The line bullet points get.
pub const BULLETS_LINE: &str =
    "The speaker asks for bullet points: write the items as a bulleted list, one per line \
(\"- …\"), after any sentence that leads into it, leaving out the words \"bullet points\".";

/// The line an email with a greeting or sign-off gets (the kind's own line
/// already says it's the body of an email; "Lay it out as an email" made
/// the 4B add a "Subject:" line, which `drop_unsaid_frame` also takes off).
pub const EMAIL_LINE: &str =
    "Put the greeting on its own line, then a blank line, the body, a blank line, and the \
sign-off on its own line (a name after it on the next).";

/// The formatting line for `text` going into `style` at `strength`
/// (`None`: nothing to lay out — most dictations): a list when the words
/// of one are said, an email's layout when an email (the Email kind) opens
/// with a greeting or closes with a sign-off. Never for a tone that keeps
/// the words as said (AI prompts' Literal, a terminal's Command), nor at
/// Light. Pure but for a model evaluation's wording
/// (`YAP_EVAL_FORMAT_LINES`).
pub fn hint(text: &str, style: Option<Style>, strength: Strength) -> Option<String> {
    if !strength.formats()
        || style.is_some_and(|s| matches!(s.tone, Tone::Literal | Tone::Command | Tone::Off))
    {
        return None;
    }
    let mut lines: Vec<String> = Vec::new();
    if style.is_some_and(|s| s.category == Category::Email) {
        let (greeting, sign_off) = email_frame(text);
        if greeting || sign_off {
            lines.push(wording("email", EMAIL_LINE));
        }
    }
    match list_cue(text) {
        Some(ListCue::Numbered) => lines.push(wording("numbered", NUMBERED_LINE)),
        Some(ListCue::Bullets) => lines.push(wording("bullets", BULLETS_LINE)),
        None => {}
    }
    let lines: Vec<String> = lines.into_iter().filter(|l| !l.trim().is_empty()).collect();
    (!lines.is_empty()).then(|| lines.join(" "))
}

/// A model evaluation's wording for a line (`YAP_EVAL_FORMAT_LINES`: a JSON
/// file of `{"numbered": "…", "bullets": "…", "email": "…"}`, "" for none).
fn wording(key: &str, ours: &str) -> String {
    if crate::ai::eval::active() {
        if let Some(path) = std::env::var_os("YAP_EVAL_FORMAT_LINES") {
            let v: Option<serde_json::Value> = std::fs::read_to_string(path)
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok());
            if let Some(s) = v.as_ref().and_then(|v| v.get(key)).and_then(|x| x.as_str()) {
                return s.to_string();
            }
        }
    }
    ours.to_string()
}

// ---- The layout, where the model didn't ---------------------------------------------

/// The cleaned text `out` laid out where the line asked for it and the
/// model kept it on one line (Qwen2.5-1.5B, the processor's model, mostly
/// did: docs/model-evals.md "Cleanup strength and formatting"): an email's
/// greeting and sign-off on lines of their own, and a list of three or more
/// spoken items ("first …, second …, third …") one per line. Only where
/// [`hint`] sent a line for `raw` (the transcript), only what the model
/// wrote (its words, never new ones), and a list only with three markers
/// or more at the start of its clauses — two ("first I was late and second
/// I forgot") stay as the model wrote them. Pure.
pub fn lay_out(raw: &str, out: &str, style: Option<Style>, strength: Strength) -> String {
    if hint(raw, style, strength).is_none() {
        return out.to_string();
    }
    let mut text = out.trim().to_string();
    if list_cue(raw) == Some(ListCue::Numbered) {
        text = list_lines(&text).unwrap_or(text);
    }
    if style.is_some_and(|s| s.category == Category::Email) {
        let (greeting, sign_off) = email_frame(raw);
        if greeting {
            text = greeting_line(&text).unwrap_or(text);
        }
        if sign_off {
            text = sign_off_line(&text).unwrap_or(text);
        }
    }
    text
}

/// "Hi Priya, thanks for…" → "Hi Priya,\n\nThanks for…": a greeting of up
/// to four words ending in "," or "!" at the start of a first line with
/// more after it.
fn greeting_line(text: &str) -> Option<String> {
    let (first, rest) = text.split_once('\n').map_or((text, ""), |(f, r)| (f, r));
    let lower = first.to_lowercase();
    let opener = GREETINGS.iter().any(|g| lower.starts_with(&format!("{g} ")) || lower.starts_with(&format!("{g},")))
        || ["good morning", "good afternoon", "good evening"].iter().any(|g| lower.starts_with(g));
    if !opener {
        return None;
    }
    // The end of the greeting: the first "," or "!" within its first words.
    let mut words = 0;
    for (i, c) in first.char_indices() {
        if c.is_whitespace() {
            words += 1;
            if words > 4 {
                return None;
            }
        }
        if c == ',' || c == '!' {
            let after = first[i + 1..].trim();
            if after.is_empty() {
                return None;
            }
            let body = capitalise(after);
            let rest = rest.trim();
            let tail = if rest.is_empty() { String::new() } else { format!("\n{rest}") };
            return Some(format!("{}\n\n{body}{tail}", &first[..=i]));
        }
    }
    None
}

/// "… sign it tomorrow. Best regards, Tom." → "… sign it tomorrow.\n\nBest
/// regards,\nTom": a sign-off starting the last sentence, with at most two
/// words (a name) after it.
fn sign_off_line(text: &str) -> Option<String> {
    let (head, last) = match text.rsplit_once('\n') {
        Some((h, l)) => (format!("{h}\n"), l),
        None => (String::new(), text),
    };
    // Already alone on its line: nothing to do.
    let lower = last.to_lowercase();
    for phrase in SIGN_OFFS {
        let said = phrase.join(" ");
        // Where the phrase starts a sentence in the last line.
        let mut from = 0;
        while let Some(i) = lower[from..].find(&said).map(|i| i + from) {
            let before = lower[..i].trim_end();
            let starts_sentence = before.is_empty() || before.ends_with(['.', '!', '?']);
            let after = &last[i + said.len()..];
            let name: Vec<&str> = after
                .split(|c: char| c.is_whitespace() || c == ',' || c == '.' || c == '!')
                .filter(|w| !w.is_empty())
                .collect();
            // A name after it: a word or two, each with a capital.
            let a_name = name.len() <= 2 && name.iter().all(|w| w.starts_with(char::is_uppercase));
            if starts_sentence && !before.is_empty() && a_name {
                let body = last[..i].trim_end();
                let phrase_text = &last[i..i + said.len()];
                let close = match name.is_empty() {
                    true => format!("{phrase_text},"),
                    false => format!("{phrase_text},\n{}", name.join(" ")),
                };
                return Some(format!("{head}{body}\n\n{}", capitalise(&close)));
            }
            from = i + said.len();
        }
    }
    None
}

/// "We need three things: first, book the venue; second, sort out the
/// catering; and third, send the invites by Friday." → the lead-in, then
/// "1. Book the venue" … one per line. Three markers or more in order at
/// clause starts, none already on a list line. `None`: not that.
fn list_lines(text: &str) -> Option<String> {
    if text.lines().any(|l| {
        let t = l.trim_start();
        t.starts_with("1.") || t.starts_with("- ") || t.starts_with("* ") || t.starts_with("• ")
    }) {
        return None;
    }
    let lower = text.to_lowercase();
    // Each marker word at a clause start: the start, or after ": ", "; ",
    // ". ", ", " or " and ".
    let markers: &[(&[&str], usize)] = &[
        (&["first", "firstly", "one"], 1),
        (&["second", "secondly", "two"], 2),
        (&["third", "thirdly", "three", "lastly", "finally"], 3),
        (&["fourth", "fourthly", "four", "lastly", "finally"], 4),
        (&["fifth", "five", "lastly", "finally"], 5),
    ];
    let clause_start = |i: usize| {
        let before = lower[..i].trim_end_matches(' ');
        before.is_empty() || before.ends_with([':', ';', '.', ',']) || before.ends_with(" and") || before.ends_with(" then")
    };
    let mut found: Vec<(usize, usize)> = Vec::new(); // (start of marker, end of marker)
    let mut from = 0;
    for (words, _) in markers {
        let next = words
            .iter()
            .filter_map(|w| {
                let mut at = from;
                while let Some(i) = lower[at..].find(w).map(|i| i + at) {
                    let end = i + w.len();
                    let whole = !lower[..i].ends_with(|c: char| c.is_alphanumeric())
                        && !lower[end..].starts_with(|c: char| c.is_alphanumeric());
                    if whole && clause_start(i) {
                        return Some((i, end));
                    }
                    at = end;
                }
                None
            })
            .min();
        match next {
            Some((i, end)) => {
                found.push((i, end));
                from = end;
            }
            None => break,
        }
    }
    if found.len() < 3 {
        return None;
    }
    let lead = text[..found[0].0].trim_end().trim_end_matches([':', ';', ',']).trim_end();
    let lead = lead.strip_suffix(" and").unwrap_or(lead);
    let mut out = String::new();
    if !lead.is_empty() {
        out.push_str(&format!("{}:\n", capitalise(lead)));
    }
    let mut tail = String::new();
    for (k, &(_, end)) in found.iter().enumerate() {
        let stop = found.get(k + 1).map(|n| n.0).unwrap_or(text.len());
        let mut item = text[end..stop].trim_start_matches([',', ':', ' ']).trim_end().to_string();
        // The last item ends at its sentence: what follows is a paragraph.
        if k + 1 == found.len() {
            if let Some(i) = item.find(". ") {
                tail = item[i + 1..].trim().to_string();
                item.truncate(i + 1);
            }
        }
        let item = item.trim_end_matches([';', ',', '.']).trim_end();
        let item = item.strip_suffix(" and").or_else(|| item.strip_suffix(" then")).unwrap_or(item);
        let item = item.trim_end_matches([';', ',', '.', ' ']);
        if item.is_empty() {
            return None;
        }
        out.push_str(&format!("{}. {}\n", k + 1, capitalise(item)));
    }
    let mut out = out.trim_end().to_string();
    if !tail.is_empty() {
        out.push_str(&format!("\n\n{tail}"));
    }
    Some(out)
}

/// The first letter in capitals.
fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OTHER: Option<Style> = Some(Style { category: Category::Other, tone: Tone::Natural });
    const EMAIL: Option<Style> = Some(Style { category: Category::Email, tone: Tone::Formal });

    #[test]
    fn a_list_kept_on_one_line_is_laid_out() {
        // Qwen2.5-1.5B's outputs (the model evaluations).
        let raw = "for the offsite we need three things first book the venue second sort out the catering and third send the invites by friday";
        let out = "for the offsite, we need three things: first, book the venue; second, sort out the catering; third, send the invites by Friday.";
        assert_eq!(
            lay_out(raw, out, OTHER, Strength::Medium),
            "For the offsite, we need three things:\n1. Book the venue\n2. Sort out the catering\n3. Send the invites by Friday"
        );
        let raw = "firstly we're over budget secondly the deadline moved and lastly two people are off sick";
        let out = "Firstly, we're over budget. Secondly, the deadline moved. Lastly, two people are off sick.";
        assert_eq!(lay_out(raw, out, OTHER, Strength::High), "1. We're over budget\n2. The deadline moved\n3. Two people are off sick");
        // What follows the last item stays a paragraph.
        let raw = "the plan is first we design it second we test it third we ship it in may let me know";
        let out = "The plan is: first, we design it; second, we test it; and third, we ship it in May. Let me know.";
        assert_eq!(lay_out(raw, out, OTHER, Strength::Medium), "The plan is:\n1. We design it\n2. We test it\n3. We ship it in May\n\nLet me know.");
        // Already a list: as the model wrote it.
        let listed = "We need:\n1. Venue\n2. Catering\n3. Invites";
        assert_eq!(lay_out("we need first venue second catering third invites", listed, OTHER, Strength::Medium), listed);
    }

    #[test]
    fn two_markers_a_sentence_or_light_stay_as_written() {
        let raw = "first i was late and second i forgot the keys";
        let out = "First, I was late, and second, I forgot the keys.";
        assert_eq!(lay_out(raw, out, OTHER, Strength::Medium), out);
        // No list's words said: never.
        let out = "First, second and third place get a prize.";
        assert_eq!(lay_out("i'll be there by four", out, OTHER, Strength::Medium), out);
        // Light: the layout as said.
        let raw = "for the offsite we need three things first book the venue second sort out the catering and third send the invites";
        let out = "For the offsite we need three things: first, book the venue; second, sort out the catering; third, send the invites.";
        assert_eq!(lay_out(raw, out, OTHER, Strength::Light), out);
    }

    #[test]
    fn an_email_kept_on_one_line_gets_its_greeting_and_sign_off_lines() {
        let raw = "hi priya thanks for sending the contract over i'll sign it tomorrow best regards tom";
        let out = "Hi Priya, thanks for sending the contract over. I'll sign it tomorrow. Best regards, Tom.";
        assert_eq!(
            lay_out(raw, out, EMAIL, Strength::Medium),
            "Hi Priya,\n\nThanks for sending the contract over. I'll sign it tomorrow.\n\nBest regards,\nTom"
        );
        let raw = "hey sam just checking you got the slides for monday cheers";
        let out = "Hey Sam, just checking if you got the slides for Monday. Cheers.";
        assert_eq!(lay_out(raw, out, EMAIL, Strength::Medium), "Hey Sam,\n\nJust checking if you got the slides for Monday.\n\nCheers,");
        // Laid out already: unchanged.
        let done = "Hi Priya,\n\nI'll sign it tomorrow.\n\nBest regards,\nTom";
        assert_eq!(lay_out(raw, done, EMAIL, Strength::Medium), done);
        // "Thanks for your help." ends a body; it isn't a sign-off.
        let raw = "hi sam i'll sign it tomorrow thanks for your help";
        let out = "Hi Sam, I'll sign it tomorrow. Thanks for your help.";
        assert_eq!(lay_out(raw, out, EMAIL, Strength::Medium), "Hi Sam,\n\nI'll sign it tomorrow. Thanks for your help.");
        // Not an email: as written.
        assert_eq!(lay_out(raw, out, OTHER, Strength::Medium), out);
    }

    #[test]
    fn spoken_lists_are_found() {
        let numbered = [
            "for the offsite we need three things first book the venue second sort out the catering and third send the invites",
            "my priorities are one finish the budget two hire the designer three fix the login bug",
            "the steps are number one open settings number two click privacy number three turn off tracking",
            "firstly we're over budget secondly the deadline moved and lastly two people are off sick",
            "first check the logs and second restart the server",
        ];
        for t in numbered {
            assert_eq!(list_cue(t), Some(ListCue::Numbered), "{t}");
        }
        assert_eq!(
            list_cue("bullet points milk eggs bread and coffee"),
            Some(ListCue::Bullets)
        );
        assert_eq!(
            list_cue("can you put these as bullets the budget the timeline the team"),
            Some(ListCue::Bullets)
        );
    }

    #[test]
    fn plain_sentences_without_a_list_s_words_are_never_asked() {
        for t in [
            "the train leaves at two and gets in at three so i'll be there by four",
            "we need two or three more days",
            "i'll call you at five",
            "send me the q3 numbers before friday",
            "it was the first time in years",
            "one of the two options is fine",
        ] {
            assert_eq!(list_cue(t), None, "{t}");
        }
    }

    #[test]
    fn a_sentence_that_counts_or_orders_things_is_no_list() {
        // Numbers that count things, with no lead-in.
        assert_eq!(list_cue("i have one cat two dogs and three fish"), None);
        // Ordinals after "the", "our": adjectives in a sentence.
        assert_eq!(
            list_cue("the first time i used it it broke but the second time it worked"),
            None
        );
        assert_eq!(
            list_cue("the first and second floors are closed for painting until friday"),
            None
        );
        assert_eq!(
            list_cue("we came second in the first round but won the third"),
            None
        );
        // Next to each other: no words between, no list.
        assert_eq!(list_cue("first second of the video is black"), None);
        // Some still carry a list's words; the line tells the model a
        // sentence stays a sentence (evaluated).
        assert_eq!(
            list_cue("first i was late and second i forgot the keys"),
            Some(ListCue::Numbered)
        );
    }

    #[test]
    fn an_email_s_greeting_and_sign_off() {
        assert_eq!(
            email_frame(
                "hi priya thanks for sending the contract i'll sign it tomorrow best regards tom"
            ),
            (true, true)
        );
        assert_eq!(
            email_frame("hey sam just checking you got the slides cheers"),
            (true, true)
        );
        assert_eq!(
            email_frame("um hello team the release is on track"),
            (true, false)
        );
        assert_eq!(
            email_frame("good morning everyone the office is closed today"),
            (true, false)
        );
        assert_eq!(
            email_frame("the invoice was paid this morning thanks"),
            (false, true)
        );
        assert_eq!(
            email_frame("the meeting has moved to room four"),
            (false, false)
        );
        // "thanks" alone isn't an email with a sign-off.
        assert_eq!(email_frame("thanks"), (false, false));
    }

    #[test]
    fn the_hint_goes_only_where_it_s_said_at_medium_and_high() {
        let email = Some(Style {
            category: Category::Email,
            tone: Tone::Formal,
        });
        let other = Some(Style {
            category: Category::Other,
            tone: Tone::Natural,
        });
        let list =
            "my priorities are one finish the budget two hire the designer three fix the bug";
        assert_eq!(
            hint(list, other, Strength::Medium).as_deref(),
            Some(NUMBERED_LINE)
        );
        assert_eq!(
            hint(list, None, Strength::High).as_deref(),
            Some(NUMBERED_LINE)
        );
        assert_eq!(
            hint(list, other, Strength::Light),
            None,
            "Light keeps the layout as said"
        );
        assert_eq!(hint("i'll be there by four", other, Strength::Medium), None);
        // An email: its layout, only in the Email kind.
        let mail = "hi priya thanks for the contract best regards tom";
        assert_eq!(
            hint(mail, email, Strength::Medium).as_deref(),
            Some(EMAIL_LINE)
        );
        assert_eq!(hint(mail, other, Strength::Medium), None);
        // Both.
        let both = hint(
            "hi team first the release is on track second we need a tester thanks",
            email,
            Strength::Medium,
        )
        .unwrap();
        assert!(
            both.starts_with(EMAIL_LINE) && both.ends_with(NUMBERED_LINE),
            "{both}"
        );
        // A tone that keeps the words: never.
        let prompt = Some(Style {
            category: Category::AiPrompts,
            tone: Tone::Literal,
        });
        assert_eq!(hint(list, prompt, Strength::High), None);
    }
}

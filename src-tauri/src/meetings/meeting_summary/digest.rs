//! The live digests: which stretch of talk to digest next, the prompt,
//! the reply parsed into key points, decisions, action items and open
//! questions, and the checks that keep owners and deadlines to what was
//! said.

use std::collections::HashSet;

use serde_json::{json, Value};

use crate::notes::{DigestAction, MeetingDigest, TranscriptSegment};

use super::{
    clock, estimate_tokens, seg_tokens, CONTEXT_TOKENS, DIGEST_EVERY_SECS, DIGEST_MAX_TOKENS, DIGEST_MIN_TOKENS, DIGEST_TARGET_TOKENS,
    UNASSIGNED,
};

// ---- Planning: which stretch to digest next ----

/// How [`next_window`] decides.
#[derive(Clone, Copy, Debug)]
pub enum Plan {
    /// While recording: only once a full window of new talk has built up.
    Live,
    /// Before the final call: whatever leaves more than `keep` tokens of raw
    /// tail, oldest first, in windows the size of the live ones.
    Final { keep: usize },
}

/// The next stretch to digest, `transcript[from..to]`, or `None`.
pub fn next_window(
    transcript: &[TranscriptSegment],
    from: usize,
    plan: Plan,
) -> Option<(usize, usize)> {
    let tail = transcript.get(from..)?;
    let (first, last) = (tail.first()?, tail.last()?);
    let tokens: usize = tail.iter().map(seg_tokens).sum();
    let take = match plan {
        Plan::Live => {
            let span = last.ts.saturating_sub(first.ts);
            let due = tokens >= DIGEST_TARGET_TOKENS
                || (span >= DIGEST_EVERY_SECS && tokens >= DIGEST_MIN_TOKENS);
            if !due {
                return None;
            }
            DIGEST_MAX_TOKENS
        }
        Plan::Final { keep } => {
            if tokens <= keep {
                return None;
            }
            (tokens - keep)
                .max(DIGEST_TARGET_TOKENS.min(tokens))
                .min(DIGEST_MAX_TOKENS)
        }
    };
    let mut to = from;
    let mut sum = 0;
    for seg in tail {
        let t = seg_tokens(seg);
        if to > from && sum + t > take {
            break;
        }
        sum += t;
        to += 1;
    }
    Some((from, to))
}

/// How many digests [`next_window`] in `Final` mode still wants from `from`.
pub(super) fn remaining_windows(transcript: &[TranscriptSegment], mut from: usize, keep: usize) -> usize {
    let mut n = 0;
    while let Some((_, to)) = next_window(transcript, from, Plan::Final { keep }) {
        n += 1;
        from = to;
    }
    n
}

// ---- One digest: prompt, parse, checks ----

const DIGEST_EXAMPLE_IN: &str = "Attendees: Priya, Tom\n\nThe meeting so far:\n(This is the start of the meeting.)\n\nTranscript, part 1 (0:00\u{2013}0:04):\nYou: Let's lock the launch date. Priya, can you draft the press release?\nThem: Sure, I'll have it done by Thursday.\nThem: We still don't know if legal needs to see it first.\nYou: OK, the launch is March 3rd then. Someone should book the venue.\n";
const DIGEST_EXAMPLE_OUT: &str = "### Key points\n- The launch date was set and the press release planned.\n### Decisions\n- Launch on March 3rd.\n### Action items\n- [ ] Priya: draft the press release (due: Thursday)\n- [ ] Unassigned: book the venue\n### Open questions\n- Does legal need to review the press release first?";

/// "The meeting so far" for a digest: the latest digests' key points (newest
/// kept when they don't all fit), in meeting order.
pub fn context_so_far(digests: &[MeetingDigest], t0: u64) -> String {
    let mut picked: Vec<String> = Vec::new();
    let mut used = 0;
    'digests: for d in digests.iter().rev() {
        for p in d.key_points.iter().rev() {
            let line = format!("- ({}) {}", clock(d.start_ts.saturating_sub(t0)), p);
            let t = estimate_tokens(&line) + 1;
            if used + t > CONTEXT_TOKENS {
                break 'digests;
            }
            used += t;
            picked.push(line);
        }
    }
    if picked.is_empty() {
        return "(This is the start of the meeting.)".to_string();
    }
    picked.reverse();
    picked.join("\n")
}

/// The chat messages for digest `part` over `lines` (one-shot example first:
/// it's what keeps a 1.5B model in the four-heading layout). `people`: the
/// attendees and speakers lines ([`people_lines`]); `language`: the line
/// that keeps a meeting in another language in it (`language::digest_line`),
/// after the transcript.
pub fn digest_messages(people: &str, context: &str, part: usize, range: &str, lines: &str, language: Option<&str>) -> Value {
    let user = super::language::with_line(
        &format!("{people}\nThe meeting so far:\n{context}\n\nTranscript, part {part} ({range}):\n{lines}"),
        language,
    );
    json!([
        { "role": "system", "content": crate::ai::llm::MEETING_DIGEST_PROMPT },
        { "role": "user", "content": DIGEST_EXAMPLE_IN },
        { "role": "assistant", "content": DIGEST_EXAMPLE_OUT },
        { "role": "user", "content": user },
    ])
}

/// A digest reply, read back.
#[derive(Debug, Default, PartialEq)]
pub struct ParsedDigest {
    pub key_points: Vec<String>,
    pub decisions: Vec<String>,
    pub actions: Vec<DigestAction>,
    pub questions: Vec<String>,
    /// At least one of the four headings was found.
    pub recognized: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum Section {
    Other,
    Points,
    Decisions,
    Actions,
    Questions,
}

fn section_of(heading: &str) -> Section {
    let h = heading.to_lowercase();
    if h.contains("decision") {
        Section::Decisions
    } else if h.contains("action")
        || h.contains("task")
        || h.contains("next step")
        || h.contains("to-do")
        || h.contains("todo")
    {
        Section::Actions
    } else if h.contains("question") || h.contains("unresolved") {
        Section::Questions
    } else if h.contains("point")
        || h.contains("summary")
        || h.contains("discussion")
        || h.contains("highlight")
    {
        Section::Points
    } else {
        Section::Other
    }
}

/// A heading line's text: "### Key points", "**Decisions:**", "Action items:".
fn heading_text(t: &str) -> Option<String> {
    if t.starts_with('#') {
        return Some(
            t.trim_start_matches('#')
                .trim()
                .trim_end_matches(':')
                .trim()
                .to_string(),
        );
    }
    for marker in ["**", "__"] {
        if let Some(inner) = t.strip_prefix(marker).and_then(|s| s.strip_suffix(marker)) {
            return Some(inner.trim().trim_end_matches(':').trim().to_string());
        }
    }
    let bare = t.strip_suffix(':')?;
    let is_bullet = bare.starts_with(['-', '*', '•', '+']);
    (!is_bullet && bare.split_whitespace().count() <= 4).then(|| bare.trim().to_string())
}

/// A list item's text, without its bullet, number or checkbox.
pub(super) fn strip_bullet(t: &str) -> String {
    let t = t.trim();
    let t = if let Some(rest) = t.strip_prefix(['-', '•', '+']) {
        rest.trim_start()
    } else if let Some(rest) = t.strip_prefix("* ") {
        rest.trim_start()
    } else {
        let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0 && t[digits..].starts_with(['.', ')']) {
            t[digits + 1..].trim_start()
        } else {
            t
        }
    };
    let t = ["[ ]", "[x]", "[X]"]
        .iter()
        .find_map(|b| t.strip_prefix(b))
        .unwrap_or(t);
    t.replace("**", "").trim().to_string()
}

/// "- None", "- No decisions were made", "- N/A"…
fn is_none_item(item: &str) -> bool {
    let l = item.trim().trim_end_matches('.').to_lowercase();
    if matches!(
        l.as_str(),
        "none" | "n/a" | "na" | "nothing" | "none so far" | "none mentioned" | "nothing new" | "-"
    ) {
        return true;
    }
    (l.starts_with("no ") || l.starts_with("there were no ") || l.starts_with("none "))
        && !l.contains(':')
        && l.split_whitespace().count() <= 8
        && ["decision", "action", "question", "point", "task", "item"]
            .iter()
            .any(|w| l.contains(w))
}

pub(super) fn due_regex() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(r"(?i)\s*\((?:due|deadline|by)\s*:?\s*([^()]*)\)\s*$").unwrap()
    })
}

/// "Priya: draft the press release (due: Thursday)" → owner, task, due.
fn parse_action(item: &str) -> Option<DigestAction> {
    let mut text = item.trim().to_string();
    let mut due = String::new();
    if let Some(c) = due_regex().captures(&text) {
        let d = c[1].trim().trim_end_matches('.').to_string();
        if !matches!(
            d.to_lowercase().as_str(),
            "" | "none" | "n/a" | "tbd" | "not stated"
        ) {
            due = d;
        }
        let start = c.get(0).map(|m| m.start()).unwrap_or(text.len());
        text.truncate(start);
    }
    let (mut owner, mut task) = (String::new(), text.trim().to_string());
    if let Some((head, rest)) = text.split_once(':') {
        let head = head.trim().trim_matches(['[', ']']).trim();
        // ("meet at 10:30" has a colon too, but no owner.)
        if !head.is_empty()
            && head.len() <= 40
            && head.split_whitespace().count() <= 4
            && !head.chars().any(|c| c.is_ascii_digit())
        {
            owner = head.to_string();
            task = rest.trim().to_string();
        }
    }
    let task = task.trim().trim_end_matches('.').trim().to_string();
    (!task.is_empty()).then_some(DigestAction { owner, task, due })
}

pub(super) fn push_unique(list: &mut Vec<String>, item: String) {
    if !list.iter().any(|x| x.eq_ignore_ascii_case(&item)) {
        list.push(item);
    }
}

/// Read a digest reply back (lenient: other heading styles, numbered lists,
/// stray text before the first heading).
pub fn parse_digest(reply: &str) -> ParsedDigest {
    let mut out = ParsedDigest::default();
    let mut section = Section::Other;
    for line in reply.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if let Some(h) = heading_text(t) {
            section = section_of(&h);
            out.recognized |= section != Section::Other;
            continue;
        }
        let item = strip_bullet(t);
        if item.is_empty() || is_none_item(&item) {
            continue;
        }
        match section {
            Section::Other => {}
            Section::Points => push_unique(&mut out.key_points, item),
            Section::Decisions => push_unique(&mut out.decisions, item),
            Section::Questions => push_unique(&mut out.questions, item),
            Section::Actions => {
                if let Some(a) = parse_action(&item) {
                    if !out.actions.contains(&a) {
                        out.actions.push(a);
                    }
                }
            }
        }
    }
    out
}

/// Lowercase words of `s`, ordinals folded ("3rd" → "3", "third" → "3").
pub(super) fn words(s: &str) -> Vec<String> {
    const NUMBERS: [&str; 31] = [
        "first",
        "second",
        "third",
        "fourth",
        "fifth",
        "sixth",
        "seventh",
        "eighth",
        "ninth",
        "tenth",
        "eleventh",
        "twelfth",
        "thirteenth",
        "fourteenth",
        "fifteenth",
        "sixteenth",
        "seventeenth",
        "eighteenth",
        "nineteenth",
        "twentieth",
        "twenty-first",
        "twenty-second",
        "twenty-third",
        "twenty-fourth",
        "twenty-fifth",
        "twenty-sixth",
        "twenty-seventh",
        "twenty-eighth",
        "twenty-ninth",
        "thirtieth",
        "thirty-first",
    ];
    const CARDINALS: [&str; 12] = [
        "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven",
        "twelve",
    ];
    s.split(|c: char| !(c.is_alphanumeric() || c == '-' || c == '\''))
        .map(|w| w.trim_matches(['-', '\'']).to_lowercase())
        .filter(|w| !w.is_empty())
        .map(|w| {
            if let Some(i) = NUMBERS.iter().position(|n| *n == w) {
                return (i + 1).to_string();
            }
            if let Some(i) = CARDINALS.iter().position(|n| *n == w) {
                return (i + 1).to_string();
            }
            let digits = w.chars().take_while(|c| c.is_ascii_digit()).count();
            if digits > 0 && matches!(&w[digits..], "st" | "nd" | "rd" | "th") {
                return w[..digits].to_string();
            }
            w
        })
        .collect()
}

/// Does `name` occur in `text` as whole words (case-insensitive)?
fn mentions(text_words: &[String], name: &str) -> bool {
    let name = words(name);
    !name.is_empty() && text_words.windows(name.len()).any(|w| w == name.as_slice())
}

/// The owner a task may carry: "You", "Everyone", an attendee (canonical
/// spelling), or a name said in `source`; anything else is [`UNASSIGNED`].
pub fn resolve_owner(raw: &str, attendees: &[String], source: &str) -> String {
    let owner = raw
        .trim()
        .trim_matches(|c: char| "*@[]\"'.,:".contains(c))
        .trim();
    let lower = owner.to_lowercase();
    // English, and the words a meeting's notes in another language use for
    // the same (`language.rs` asks for "You" and "Unassigned"; a small
    // model sometimes writes « Moi » or „Niemand“ anyway).
    const NOBODY: &[&str] = &[
        "",
        "unassigned",
        "none",
        "nobody",
        "no one",
        "someone",
        "somebody",
        "anyone",
        "tbd",
        "n/a",
        "unknown",
        "them",
        "they",
        "other",
        "others",
        "personne",
        "non attribué",
        "non attribuée",
        "non attribuées",
        "non assigné",
        "quelqu'un",
        "sin asignar",
        "nadie",
        "alguien",
        "niemand",
        "nicht zugewiesen",
        "jemand",
        "nessuno",
        "non assegnato",
        "ninguém",
        "não atribuído",
        "niet toegewezen",
    ];
    const YOU: &[&str] = &[
        "you", "me", "i", "myself", "the user", "moi", "vous", "toi", "yo", "usted", "tú", "ich", "du", "io", "eu", "você", "ik",
    ];
    const EVERYONE: &[&str] = &[
        "everyone",
        "everybody",
        "all",
        "team",
        "the team",
        "all of us",
        "we",
        "tout le monde",
        "tous",
        "l'équipe",
        "équipe",
        "todos",
        "el equipo",
        "alle",
        "das team",
        "tutti",
        "il team",
        "iedereen",
    ];
    if NOBODY.contains(&lower.as_str()) {
        return UNASSIGNED.to_string();
    }
    if YOU.contains(&lower.as_str()) {
        return "You".to_string();
    }
    if EVERYONE.contains(&lower.as_str()) {
        return "Everyone".to_string();
    }
    let owner_words: Vec<&str> = lower.split_whitespace().collect();
    for a in attendees {
        let al = a.trim().to_lowercase();
        let att_words: Vec<&str> = al.split_whitespace().collect();
        if al.is_empty() {
            continue;
        }
        let same = al == lower
            || (owner_words.len() == 1 && att_words.first() == owner_words.first())
            || (att_words.len() == 1 && att_words.first() == owner_words.first());
        if same {
            return a.trim().to_string();
        }
    }
    if owner_words.len() <= 3 && mentions(&words(source), owner) {
        return owner.to_string();
    }
    UNASSIGNED.to_string()
}

/// Filler that doesn't have to appear in the source for a deadline to count.
const DUE_FILLER: [&str; 12] = [
    "by", "on", "the", "due", "before", "until", "till", "at", "in", "of", "a", "an",
];

/// `due` if its words appear in `source` (so it was said, not made up), else "".
pub fn supported_due(due: &str, source_words: &HashSet<String>) -> String {
    let due = due.trim();
    let need: Vec<String> = words(due)
        .into_iter()
        .filter(|w| !DUE_FILLER.contains(&w.as_str()))
        .collect();
    if !need.is_empty() && need.iter().all(|w| source_words.contains(w)) {
        due.to_string()
    } else {
        String::new()
    }
}

/// Apply the owner/deadline checks to a parsed digest of `source`.
pub fn check_digest(mut d: ParsedDigest, attendees: &[String], source: &str) -> ParsedDigest {
    let source_words: HashSet<String> = words(source).into_iter().collect();
    for a in d.actions.iter_mut() {
        a.owner = resolve_owner(&a.owner, attendees, source);
        a.due = supported_due(&a.due, &source_words);
    }
    let mut seen: Vec<DigestAction> = Vec::new();
    d.actions.retain(|a| {
        let dup = seen.contains(a);
        seen.push(a.clone());
        !dup
    });
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meetings::meeting_summary::tests::{digest, talk};
    use crate::meetings::meeting_summary::FINAL_TAIL_TOKENS;

    #[test]
    fn live_waits_for_a_full_window() {
        // Ten minutes of slow talk: due by time, not tokens.
        let quiet = talk(20, 10, 32); // 608 s span
        assert!(next_window(&quiet, 0, Plan::Live).is_some());
        // Two minutes of it: not yet.
        assert!(next_window(&quiet[..4], 0, Plan::Live).is_none());
        // A long burst: due by tokens, window capped.
        let burst = talk(200, 40, 1);
        let (from, to) = next_window(&burst, 0, Plan::Live).unwrap();
        assert_eq!(from, 0);
        let tokens: usize = burst[from..to].iter().map(seg_tokens).sum();
        assert!(
            (DIGEST_TARGET_TOKENS..=DIGEST_MAX_TOKENS).contains(&tokens),
            "{tokens}"
        );
    }

    #[test]
    fn final_digests_only_what_wont_fit_raw() {
        let t = talk(400, 40, 15);
        let keep = FINAL_TAIL_TOKENS;
        let mut from = 0;
        let mut calls = 0;
        while let Some((f, to)) = next_window(&t, from, Plan::Final { keep }) {
            assert_eq!(f, from);
            let tokens: usize = t[f..to].iter().map(seg_tokens).sum();
            assert!(tokens <= DIGEST_MAX_TOKENS);
            from = to;
            calls += 1;
        }
        let tail: usize = t[from..].iter().map(seg_tokens).sum();
        assert!(tail <= keep, "{tail}");
        assert_eq!(calls, remaining_windows(&t, 0, keep));
        // A short tail needs nothing.
        assert!(next_window(&t[..10], 0, Plan::Final { keep }).is_none());
    }

    #[test]
    fn parses_a_digest() {
        let reply = "Here are the notes:\n### Key points\n- Budget is tight.\n2. Hiring paused\n### Decisions\n- None\n### Action items\n- [ ] Alice: send the revised budget (due: Friday)\n- [ ] Unassigned: book the venue\n- [ ] Bob Stone: review the contract.\n- [ ] Meet at 10:30 to sign\n### Open questions\n* Is the venue free in May?";
        let d = parse_digest(reply);
        assert!(d.recognized);
        assert_eq!(d.key_points, vec!["Budget is tight.", "Hiring paused"]);
        assert_eq!(d.actions[3].owner, "");
        assert_eq!(d.actions[3].task, "Meet at 10:30 to sign");
        assert!(d.decisions.is_empty());
        assert_eq!(
            d.actions[0],
            DigestAction {
                owner: "Alice".into(),
                task: "send the revised budget".into(),
                due: "Friday".into()
            }
        );
        assert_eq!(d.actions[1].owner, "Unassigned");
        assert_eq!(d.actions[2].task, "review the contract");
        assert_eq!(d.questions, vec!["Is the venue free in May?"]);
    }

    #[test]
    fn parses_other_heading_styles() {
        let reply = "**Key points:**\n1. Launch moved\n**Action items:**\n* [ ] You: email the client (by Monday)\nNext steps:\n- book a room";
        let d = parse_digest(reply);
        assert!(d.recognized);
        assert_eq!(d.key_points, vec!["Launch moved"]);
        assert_eq!(d.actions[0].owner, "You");
        assert_eq!(d.actions[0].due, "Monday");
        assert_eq!(d.actions[1].owner, "");
        assert_eq!(d.actions[1].task, "book a room");
        assert!(!parse_digest("Sorry, I can't help with that.").recognized);
    }

    #[test]
    fn owners_must_be_real() {
        let attendees = vec!["Alice".to_string(), "Bob Stone".to_string()];
        let said = "Them: Carol said she'd take the slides. You: thanks";
        assert_eq!(resolve_owner("alice", &attendees, said), "Alice");
        assert_eq!(resolve_owner("Bob", &attendees, said), "Bob Stone");
        assert_eq!(resolve_owner("Carol", &attendees, said), "Carol");
        assert_eq!(resolve_owner("Mallory", &attendees, said), UNASSIGNED);
        assert_eq!(resolve_owner("Them", &attendees, said), UNASSIGNED);
        assert_eq!(resolve_owner("me", &attendees, said), "You");
        assert_eq!(resolve_owner("the team", &attendees, said), "Everyone");
        // A name that only appears inside another word doesn't count.
        assert_eq!(resolve_owner("Car", &attendees, said), UNASSIGNED);
    }

    #[test]
    fn deadlines_must_have_been_said() {
        let src: HashSet<String> =
            words("You: can you do it by the third of March? Them: sure, by Friday")
                .into_iter()
                .collect();
        assert_eq!(supported_due("Friday", &src), "Friday");
        assert_eq!(supported_due("March 3rd", &src), "March 3rd");
        assert_eq!(supported_due("by Monday", &src), "");
        assert_eq!(supported_due("TBD", &src), "");
    }

    #[test]
    fn digest_checks_rewrite_invented_owners_and_dates() {
        let window = "Them: Alice, can you send the budget by Friday?\nThem: Sure.\nYou: and someone should order lunch\n";
        let parsed = parse_digest("### Action items\n- [ ] Alice: send the budget (due: Friday)\n- [ ] Zed: order lunch (due: Monday)");
        let d = check_digest(parsed, &["Alice".to_string()], window);
        assert_eq!(d.actions[0].owner, "Alice");
        assert_eq!(d.actions[0].due, "Friday");
        assert_eq!(d.actions[1].owner, UNASSIGNED);
        assert_eq!(d.actions[1].due, "");
    }

    #[test]
    fn context_so_far_is_bounded_and_ordered() {
        let digests: Vec<MeetingDigest> = (0..30)
            .map(|i| {
                digest(
                    i,
                    i + 1,
                    1000 + i as u64 * 600,
                    &["a key point that takes up some room in the budget"],
                    &[],
                )
            })
            .collect();
        let c = context_so_far(&digests, 1000);
        assert!(estimate_tokens(&c) <= CONTEXT_TOKENS + 5);
        // The newest point is kept, and the list runs oldest → newest.
        assert!(c.trim_end().ends_with("budget"));
        assert!(c.lines().last().unwrap().contains(&clock(29 * 600)));
        assert_eq!(
            context_so_far(&[], 0),
            "(This is the start of the meeting.)"
        );
    }
}

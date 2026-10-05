//! Meeting summaries that keep up with long meetings.
//!
//! One end-of-meeting LLM call over the whole transcript (what the Meeting
//! Notes action used to do) overflows the on-device model's 8k context after
//! ~25–30 minutes of talk, and is slow and costly on cloud models even when it
//! fits. Instead, while a meeting records, every ~10 minutes of new talk
//! becomes a **digest** — key points, decisions, action items (owner and
//! deadline only as said) and open questions — written in the background and
//! persisted on the note (`Note::digests`), so a crash or restart keeps them.
//! At the end, the action plan merges the digests plus the not-yet-digested
//! tail in ONE call whose input is bounded whatever the meeting length, so it
//! comes back in seconds. (Map-reduce with the map done as the meeting goes;
//! each digest gets the main points so far, so the parts read as one meeting.)
//!
//! Budgets, in estimated tokens ([`estimate_tokens`] runs high on purpose).
//! Every call fits an 8k-context local model with room to spare:
//! - a digest reads ≤ 2,600 of transcript + ≤ 400 of context + ~700 of
//!   prompt, and replies in ≤ 700: ~4.4k;
//! - the final call reads ≤ 4,500 (≤ 600 typed notes, ≤ 1,800 raw tail, the
//!   rest digests, trimmed evenly when a very long meeting needs it) + ~550
//!   of prompt, and replies in ≤ 1,500: ~6.6k.
//!
//! A meeting whose transcript + notes fit in 3,500 keeps the single pass
//! over the raw transcript (nothing lost to digesting).
//!
//! Never slows dictation: on a local model (llamafile serves one request at a
//! time) a digest waits until no dictation is recording or processing, and an
//! in-flight one is dropped the moment one starts — closing the connection,
//! which llama.cpp's server treats as a cancel — then retried. Replies to
//! local models are capped (`max_tokens`).
//!
//! Never invents owners or dates: digests and the action plan go through
//! deterministic checks. An owner must be "You", "Everyone", an attendee, or a
//! name that was actually said, else the task is Unassigned; a deadline's
//! words must appear in what the model read, else it's dropped; and the
//! action plan gets back any digest task the final merge left out
//! (summaries of summaries are where tasks get lost).

use std::collections::HashSet;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::config::YapConfig;
use crate::llm::ChatOptions;
use crate::notes::{DigestAction, MeetingDigest, Note, TranscriptSegment};

/// A digest is due once the new talk spans this much meeting time…
pub const DIGEST_EVERY_SECS: u64 = 10 * 60;
/// …or reaches this many tokens (~10 minutes of steady talk at ~150 wpm).
pub const DIGEST_TARGET_TOKENS: usize = 2_000;
/// The time trigger waits for at least this much talk (a quiet stretch isn't
/// worth a call).
pub const DIGEST_MIN_TOKENS: usize = 250;
/// Most transcript one digest reads; a backlog is split into calls this size.
pub const DIGEST_MAX_TOKENS: usize = 2_600;
/// "The meeting so far" handed to each digest for continuity.
const CONTEXT_TOKENS: usize = 400;
/// Reply cap for a digest (local models only, see `ChatOptions::max_tokens`).
const DIGEST_REPLY_TOKENS: u32 = 700;
/// Transcript + typed notes up to this size: one pass over the raw text.
pub const SINGLE_PASS_TOKENS: usize = 3_500;
/// Input budget of the final call when digests are used…
pub const FINAL_INPUT_TOKENS: usize = 4_500;
/// …of which at most this much is raw transcript after the last digest…
pub const FINAL_TAIL_TOKENS: usize = 1_800;
/// …and at most this much the user's typed notes.
const FINAL_NOTES_TOKENS: usize = 600;
/// Reply cap for the final call (local models only).
const FINAL_REPLY_TOKENS: u32 = 1_500;
/// After a dictation, wait this long before a digest takes the local model,
/// so a quick follow-up dictation doesn't queue behind it either.
const IDLE_GRACE: Duration = Duration::from_secs(2);
/// After a failed digest, live digests rest this long (a bad key or a rate
/// limit shouldn't be hammered every chunk).
const FAILURE_COOLDOWN_MS: u64 = 2 * 60 * 1000;

/// Owner of a task nobody took on.
pub const UNASSIGNED: &str = "Unassigned";

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Rough token count for budgeting, deliberately on the high side: 3.5
/// characters per token for ASCII text (English runs ~4) and one per
/// non-ASCII character (CJK runs ~1, accented Latin far less).
pub fn estimate_tokens(s: &str) -> usize {
    let (mut ascii, mut other) = (0usize, 0usize);
    for c in s.chars() {
        if c.is_ascii() {
            ascii += 1;
        } else {
            other += 1;
        }
    }
    (ascii * 2).div_ceil(7) + other
}

/// The first `budget` tokens of `s` (cut at a line, else a char boundary).
fn truncate_to_tokens(s: &str, budget: usize) -> String {
    if estimate_tokens(s) <= budget {
        return s.to_string();
    }
    let mut out = String::new();
    for line in s.lines() {
        if estimate_tokens(&out) + estimate_tokens(line) + 1 > budget {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    if out.is_empty() {
        out = s.chars().take(budget * 3).collect();
    }
    format!("{}\n(…)", out.trim_end())
}

/// The last `budget` tokens of `s`, whole lines only.
fn keep_last_tokens(s: &str, budget: usize) -> String {
    if estimate_tokens(s) <= budget {
        return s.to_string();
    }
    let mut kept: Vec<&str> = Vec::new();
    let mut used = 0;
    for line in s.lines().rev() {
        let t = estimate_tokens(line) + 1;
        if used + t > budget {
            break;
        }
        used += t;
        kept.push(line);
    }
    kept.reverse();
    format!("(…earlier lines left out)\n{}\n", kept.join("\n"))
}

fn speaker(seg: &TranscriptSegment) -> &'static str {
    if seg.source == "you" {
        "You"
    } else {
        "Them"
    }
}

/// Transcript lines as the models read them ("You: …" / "Them: …"), without
/// the echo segments.
pub fn transcript_lines(segs: &[TranscriptSegment]) -> String {
    segs.iter()
        .filter(|s| !s.echo)
        .map(|s| format!("{}: {}\n", speaker(s), s.text.trim()))
        .collect()
}

fn seg_tokens(seg: &TranscriptSegment) -> usize {
    if seg.echo {
        0
    } else {
        estimate_tokens(&seg.text) + 2
    }
}

/// "m:ss", or "h:mm:ss" past the hour.
pub fn clock(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// The meeting's zero: the first segment's capture time.
fn meeting_t0(note: &Note) -> u64 {
    note.transcript.first().map(|s| s.ts).unwrap_or(0)
}

fn attendees_line(attendees: &[String]) -> String {
    let named: Vec<&str> = attendees
        .iter()
        .map(|a| a.trim())
        .filter(|a| !a.is_empty())
        .collect();
    if named.is_empty() {
        "(not given)".to_string()
    } else {
        named.join(", ")
    }
}

/// Whether the whole raw transcript (and typed notes) fit one pass.
pub fn fits_single_pass(note: &Note) -> bool {
    estimate_tokens(&transcript_lines(&note.transcript)) + estimate_tokens(note.content.trim())
        <= SINGLE_PASS_TOKENS
}

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
fn remaining_windows(transcript: &[TranscriptSegment], mut from: usize, keep: usize) -> usize {
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
/// it's what keeps a 1.5B model in the four-heading layout).
pub fn digest_messages(
    attendees: &[String],
    context: &str,
    part: usize,
    range: &str,
    lines: &str,
) -> Value {
    let user = format!(
        "Attendees: {}\n\nThe meeting so far:\n{context}\n\nTranscript, part {part} ({range}):\n{lines}",
        attendees_line(attendees)
    );
    json!([
        { "role": "system", "content": crate::llm::MEETING_DIGEST_PROMPT },
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
fn strip_bullet(t: &str) -> String {
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

fn due_regex() -> &'static regex::Regex {
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

fn push_unique(list: &mut Vec<String>, item: String) {
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
fn words(s: &str) -> Vec<String> {
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
    const NOBODY: [&str; 15] = [
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
    ];
    const YOU: [&str; 5] = ["you", "me", "i", "myself", "the user"];
    const EVERYONE: [&str; 7] = [
        "everyone",
        "everybody",
        "all",
        "team",
        "the team",
        "all of us",
        "we",
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

// ---- The final call's input ----

fn action_line(a: &DigestAction) -> String {
    if a.due.is_empty() {
        format!("- [ ] {}: {}", a.owner, a.task)
    } else {
        format!("- [ ] {}: {} (due: {})", a.owner, a.task, a.due)
    }
}

/// Every digest action, duplicates (same owner, same task) once.
pub fn digest_actions(digests: &[MeetingDigest]) -> Vec<DigestAction> {
    let mut out: Vec<DigestAction> = Vec::new();
    for a in digests.iter().flat_map(|d| d.actions.iter()) {
        if !out
            .iter()
            .any(|b| b.owner.eq_ignore_ascii_case(&a.owner) && b.task.eq_ignore_ascii_case(&a.task))
        {
            out.push(a.clone());
        }
    }
    out
}

/// The digests as one section, keeping `points_cap` key points of every
/// `stride`-th digest and `items_cap` decisions/tasks/questions of each.
fn render_with(
    digests: &[MeetingDigest],
    t0: u64,
    points_cap: usize,
    stride: usize,
    items_cap: usize,
) -> String {
    let mut points = String::new();
    let (mut decisions, mut questions): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    let mut actions: Vec<DigestAction> = Vec::new();
    for (i, d) in digests.iter().enumerate() {
        let at = clock(d.start_ts.saturating_sub(t0));
        let points_cap = if i % stride.max(1) == 0 {
            points_cap
        } else {
            0
        };
        for p in d.key_points.iter().take(points_cap) {
            points.push_str(&format!("- ({at}) {p}\n"));
        }
        if points_cap > 0 && !d.raw.is_empty() {
            let raw = truncate_to_tokens(&d.raw, 150);
            for line in raw.lines().filter(|l| !l.trim().is_empty()) {
                points.push_str(&format!("- ({at}) {}\n", strip_bullet(line)));
            }
        }
        for x in d.decisions.iter().take(items_cap) {
            push_unique(&mut decisions, x.clone());
        }
        for x in d.questions.iter().take(items_cap) {
            push_unique(&mut questions, x.clone());
        }
        for a in d.actions.iter().take(items_cap) {
            if !actions.contains(a) {
                actions.push(a.clone());
            }
        }
    }
    let mut out = String::new();
    if !points.is_empty() {
        out.push_str("### Key points\n");
        out.push_str(&points);
    }
    let mut list = |title: &str, items: Vec<String>| {
        if !items.is_empty() {
            out.push_str(&format!("### {title}\n"));
            for i in items {
                out.push_str(&i);
                out.push('\n');
            }
        }
    };
    list(
        "Decisions",
        decisions.iter().map(|d| format!("- {d}")).collect(),
    );
    list("Action items", actions.iter().map(action_line).collect());
    list(
        "Open questions",
        questions.iter().map(|q| format!("- {q}")).collect(),
    );
    out
}

/// The digests as one section within `budget` tokens: everything when it
/// fits; else key points give way first, evenly across the meeting (fewer
/// per digest, then the lead point of every 2nd, 3rd… digest), then
/// decisions, tasks and questions per digest. Tasks the model doesn't see
/// still reach the action plan ([`postcheck_action_plan`] puts them back).
pub fn render_digests(digests: &[MeetingDigest], t0: u64, budget: usize) -> String {
    let fits = |r: &String| estimate_tokens(r) <= budget;
    let attempts = (1..=6)
        .rev()
        .map(|cap| (cap, 1, usize::MAX))
        .chain((2..=12).map(|stride| (1, stride, usize::MAX)))
        .chain((1..=6).rev().map(|cap| (0, 1, cap)));
    for (points_cap, stride, items_cap) in
        std::iter::once((usize::MAX, 1, usize::MAX)).chain(attempts)
    {
        let r = render_with(digests, t0, points_cap, stride, items_cap);
        if fits(&r) {
            return r;
        }
    }
    truncate_to_tokens(&render_with(digests, t0, 0, 1, 1), budget)
}

/// What an action on a meeting note reads.
pub struct MeetingInput {
    pub text: String,
    /// Every task the digests found; an action plan must keep them all.
    pub digest_actions: Vec<DigestAction>,
}

/// Assemble a meeting note for the final call: the raw transcript when it
/// all fits ([`fits_single_pass`]), else the digests plus the raw tail — at
/// most [`FINAL_INPUT_TOKENS`] either way (callers run [`prepare_final`]
/// first so the tail is already short; a tail that isn't keeps its end).
pub fn compose_meeting_input(note: &Note) -> MeetingInput {
    let mut text = format!("Attendees: {}\n", attendees_line(&note.participants));
    // The calendar invite's agenda (calendar.rs), when the meeting had one.
    if let Some(invite) = crate::calendar::invite_context(note) {
        text.push_str(&invite);
    }
    let typed = note.content.trim();
    let digest_actions = digest_actions(&note.digests);
    if fits_single_pass(note) {
        if !typed.is_empty() {
            text.push_str(&format!("\n## Notes typed during the meeting\n{typed}\n"));
        }
        text.push_str(&format!(
            "\n## Meeting Transcript\n{}",
            transcript_lines(&note.transcript)
        ));
        return MeetingInput {
            text,
            digest_actions,
        };
    }

    let t0 = meeting_t0(note);
    let end = note.transcript.last().map(|s| s.ts).unwrap_or(t0);
    text.push_str(&format!(
        "Meeting length: {}\n",
        clock(end.saturating_sub(t0))
    ));
    if !typed.is_empty() {
        text.push_str(&format!(
            "\n## Notes typed during the meeting\n{}\n",
            truncate_to_tokens(typed, FINAL_NOTES_TOKENS)
        ));
    }
    let upto = crate::notes::digested_upto(note).min(note.transcript.len());
    let tail = &note.transcript[upto..];
    let tail_lines = keep_last_tokens(&transcript_lines(tail), FINAL_TAIL_TOKENS);
    if let (Some(first), Some(last)) = (note.digests.first(), note.digests.last()) {
        let header = format!(
            "\n## Digest of the meeting so far ({}\u{2013}{})\n",
            clock(first.start_ts.saturating_sub(t0)),
            clock(last.end_ts.saturating_sub(t0))
        );
        let used =
            estimate_tokens(&text) + estimate_tokens(&header) + estimate_tokens(&tail_lines) + 30;
        let budget = FINAL_INPUT_TOKENS.saturating_sub(used);
        text.push_str(&header);
        text.push_str(&render_digests(&note.digests, t0, budget));
    }
    if let Some(first) = tail.iter().find(|s| !s.echo) {
        text.push_str(&format!(
            "\n## Transcript of the last part ({}\u{2013}{})\n{tail_lines}",
            clock(first.ts.saturating_sub(t0)),
            clock(end.saturating_sub(t0))
        ));
    }
    MeetingInput {
        text,
        digest_actions,
    }
}

/// The meeting part of a note's chat context ("Ask anything…"), within
/// `budget` tokens: the whole transcript when it fits, else the digests and
/// as much of the latest transcript as fits.
pub fn ask_context(note: &Note, budget: usize) -> String {
    let all = transcript_lines(&note.transcript);
    if estimate_tokens(&all) <= budget || note.digests.is_empty() {
        return format!("Meeting transcript:\n{}", keep_last_tokens(&all, budget));
    }
    let t0 = meeting_t0(note);
    let digests = render_digests(&note.digests, t0, budget / 2);
    let upto = crate::notes::digested_upto(note).min(note.transcript.len());
    let rest = budget.saturating_sub(estimate_tokens(&digests));
    format!(
        "Meeting digest:\n{digests}\nLatest transcript:\n{}",
        keep_last_tokens(&transcript_lines(&note.transcript[upto..]), rest)
    )
}

// ---- Checking the action plan ----

fn is_task(line: &str) -> bool {
    let t = line.trim_start();
    ["- [ ]", "- [x]", "- [X]", "* [ ]", "* [x]"]
        .iter()
        .any(|p| t.starts_with(p))
}

/// Content words of a task, for "is this the same task" checks.
fn task_words(task: &str) -> HashSet<String> {
    const STOP: [&str; 14] = [
        "the", "a", "an", "to", "and", "for", "of", "on", "by", "with", "due", "will", "our",
        "their",
    ];
    words(task)
        .into_iter()
        .filter(|w| w.len() > 2 && !STOP.contains(&w.as_str()))
        .collect()
}

/// Is `task` one of `present` (most of its words appear in one of them)?
fn covered(task: &str, present: &[HashSet<String>]) -> bool {
    let want = task_words(task);
    if want.is_empty() {
        return true;
    }
    present.iter().any(|p| {
        let hit = want.iter().filter(|w| p.contains(*w)).count();
        hit * 10 >= want.len() * 6
    })
}

fn task_line(task: &str, due: &str) -> String {
    if due.is_empty() {
        format!("- [ ] {task}")
    } else {
        format!("- [ ] {task} (due: {due})")
    }
}

fn is_plan_title(title: &str) -> bool {
    let t = title.to_lowercase();
    t.contains("action") && !t.contains("unassigned")
}

/// A reply split at its "## " headings (the "### " owner headings stay in
/// the sections' bodies).
struct PlanDoc {
    intro: Vec<String>,
    sections: Vec<(String, Vec<String>)>,
}

impl PlanDoc {
    fn parse(reply: &str) -> Self {
        let mut doc = PlanDoc {
            intro: Vec::new(),
            sections: Vec::new(),
        };
        for line in reply.lines() {
            if let Some(title) = line.trim().strip_prefix("## ") {
                doc.sections.push((title.trim().to_string(), Vec::new()));
            } else if let Some((_, body)) = doc.sections.last_mut() {
                body.push(line.to_string());
            } else {
                doc.intro.push(line.to_string());
            }
        }
        doc
    }

    fn render(&self) -> String {
        let mut out: Vec<String> = self.intro.clone();
        for (title, body) in &self.sections {
            if out.last().is_some_and(|l| !l.trim().is_empty()) {
                out.push(String::new());
            }
            out.push(format!("## {title}"));
            let mut body = body.clone();
            while body.last().is_some_and(|l| l.trim().is_empty()) {
                body.pop();
            }
            out.extend(body);
        }
        out.join("\n").trim().to_string()
    }

    fn section(&mut self, pred: impl Fn(&str) -> bool, create: &str, before: &[&str]) -> usize {
        if let Some(i) = self.sections.iter().position(|(t, _)| pred(t)) {
            return i;
        }
        let at = self
            .sections
            .iter()
            .position(|(t, _)| {
                let t = t.to_lowercase();
                before.iter().any(|b| t.contains(b))
            })
            .unwrap_or(self.sections.len());
        self.sections.insert(at, (create.to_string(), Vec::new()));
        at
    }
}

/// Deterministic checks on an action-plan reply (the layout of
/// `llm::ACTION_PLAN_DEFAULT_FRAGMENT`): a "### Owner" heading must be "You",
/// "Everyone", an attendee or a name said in `source`, else its tasks move to
/// "## Unassigned"; a "(due: …)" must use words from `source`, else it goes;
/// and any digest task the reply lost is put back under its owner.
pub fn postcheck_action_plan(
    reply: &str,
    attendees: &[String],
    source: &str,
    digest_actions: &[DigestAction],
) -> String {
    let source_words: HashSet<String> = words(source).into_iter().collect();
    let mut doc = PlanDoc::parse(reply);
    let mut moved: Vec<String> = Vec::new();

    // Owner headings in the action-plan section.
    if let Some(i) = doc.sections.iter().position(|(t, _)| is_plan_title(t)) {
        let body = std::mem::take(&mut doc.sections[i].1);
        let mut kept = Vec::new();
        let mut dropping = false;
        for line in body {
            if let Some(name) = line.trim().strip_prefix("### ") {
                let owner = resolve_owner(name, attendees, source);
                dropping = owner == UNASSIGNED;
                if !dropping {
                    kept.push(format!("### {owner}"));
                }
            } else if dropping {
                if is_task(&line) {
                    moved.push(line.trim().to_string());
                }
            } else {
                kept.push(line);
            }
        }
        doc.sections[i].1 = kept;
    }

    // Deadlines that weren't said.
    for line in doc
        .sections
        .iter_mut()
        .flat_map(|(_, body)| body.iter_mut())
        .chain(moved.iter_mut())
    {
        if !is_task(line) {
            continue;
        }
        if let Some(c) = due_regex().captures(line) {
            if supported_due(&c[1], &source_words).is_empty() {
                let start = c.get(0).map(|m| m.start()).unwrap_or(line.len());
                line.truncate(start);
            }
        }
    }

    // Lost digest tasks go back, under their owner.
    let present: Vec<HashSet<String>> = doc
        .sections
        .iter()
        .flat_map(|(_, body)| body.iter())
        .chain(moved.iter())
        .filter(|l| is_task(l))
        .map(|l| task_words(&strip_bullet(l)))
        .collect();
    let mut lost: Vec<&DigestAction> = Vec::new();
    for a in digest_actions {
        if !covered(&a.task, &present) && !lost.iter().any(|b| b.task == a.task) {
            lost.push(a);
        }
    }
    for a in lost.iter().filter(|a| a.owner != UNASSIGNED) {
        let plan = doc.section(
            is_plan_title,
            "Action plan",
            &["decision", "question", "unassigned"],
        );
        let body = &mut doc.sections[plan].1;
        let heading = format!("### {}", a.owner);
        let at = match body
            .iter()
            .position(|l| l.trim().eq_ignore_ascii_case(&heading))
        {
            Some(h) => {
                let next = body[h + 1..]
                    .iter()
                    .position(|l| l.trim().starts_with("### "))
                    .map(|p| h + 1 + p)
                    .unwrap_or(body.len());
                let mut at = next;
                while at > h + 1 && body[at - 1].trim().is_empty() {
                    at -= 1;
                }
                at
            }
            None => {
                while body.last().is_some_and(|l| l.trim().is_empty()) {
                    body.pop();
                }
                if !body.is_empty() {
                    body.push(String::new());
                }
                body.push(heading);
                body.len()
            }
        };
        body.insert(at, task_line(&a.task, &a.due));
    }
    moved.extend(
        lost.iter()
            .filter(|a| a.owner == UNASSIGNED)
            .map(|a| task_line(&a.task, &a.due)),
    );
    if !moved.is_empty() {
        let un = doc.section(
            |t: &str| t.to_lowercase().contains("unassigned"),
            UNASSIGNED,
            &[],
        );
        let body = &mut doc.sections[un].1;
        while body.last().is_some_and(|l| l.trim().is_empty()) {
            body.pop();
        }
        body.extend(moved);
    }
    doc.render()
}

// ---- Running it ----

/// Where note-formatting calls go: the **Note Formatting** scope when it's
/// on, else the global AI-cleanup endpoint (OpenWhispr `fallbackScope:
/// dictationCleanup`). `fragment` is the scope's editable prompt.
pub struct Endpoint {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub provider: String,
    pub fragment: String,
    pub disable_thinking: bool,
}

/// The note-formatting endpoint, or the error that says how to set one up.
pub fn resolve_endpoint(cfg: &YapConfig) -> Result<Endpoint, String> {
    let ep = match cfg.llm_scopes.get("noteFormatting") {
        Some(s) if s.enabled && !s.provider.is_empty() => {
            let key = cfg.provider_api_key(&s.provider, &s.api_key);
            let (base_url, api_key, model, provider) =
                crate::local_llm::effective_endpoint_for(&s.provider, &s.base_url, &key, &s.model);
            let fragment = if s.prompt.trim().is_empty() {
                crate::llm::NOTE_DEFAULT_FRAGMENT.to_string()
            } else {
                s.prompt.clone()
            };
            Endpoint {
                base_url,
                api_key,
                model,
                provider,
                fragment,
                disable_thinking: s.disable_thinking,
            }
        }
        scope => {
            let (base_url, api_key, model, provider) = crate::local_llm::effective_endpoint(cfg);
            let fragment = scope
                .map(|s| s.prompt.clone())
                .filter(|p| !p.trim().is_empty())
                .unwrap_or_else(|| crate::llm::NOTE_DEFAULT_FRAGMENT.to_string());
            Endpoint {
                base_url,
                api_key,
                model,
                provider,
                fragment,
                disable_thinking: cfg.pp_disable_thinking,
            }
        }
    };
    if ep.base_url.is_empty() {
        return Err(
            "No AI model configured — set one in Settings → Language Models → Note Formatting"
                .to_string(),
        );
    }
    const KEYED_PROVIDERS: [&str; 5] = ["groq", "anthropic", "openai", "gemini", "openrouter"];
    if ep.api_key.is_empty() && KEYED_PROVIDERS.contains(&ep.provider.as_str()) {
        return Err(format!(
            "No {} API key — add one in Settings → Language Models",
            ep.provider
        ));
    }
    Ok(ep)
}

impl Endpoint {
    /// A server on this machine (the on-device sidecar, Ollama, LM Studio…).
    /// These serve one request at a time, so meeting calls cap their replies
    /// and get out of a dictation's way.
    pub fn is_local(&self) -> bool {
        if self.provider == crate::local_llm::PROVIDER_ONDEVICE || self.provider == "local" {
            return true;
        }
        url::Url::parse(&self.base_url)
            .ok()
            .and_then(|u| {
                u.host_str().map(|h| {
                    matches!(h, "localhost" | "127.0.0.1" | "[::1]" | "::1" | "0.0.0.0")
                        || h.ends_with(".localhost")
                })
            })
            .unwrap_or(false)
    }

    pub fn options(&self, reply_cap: u32, temperature: f32) -> ChatOptions {
        let local = self.is_local();
        ChatOptions {
            temperature,
            max_tokens: local.then_some(reply_cap),
            // A 1.5B model on a CPU writes ~10–20 tokens a second.
            timeout: Duration::from_secs(if local { 240 } else { 120 }),
            disable_thinking: self.disable_thinking,
        }
    }

    /// Options for the final call (the action plan / meeting notes).
    pub fn final_options(&self) -> ChatOptions {
        self.options(FINAL_REPLY_TOKENS, 0.3)
    }
}

/// A dictation is recording or transcribing / cleaning up (or an Upload is
/// transcribing): the moments a local model must be free.
fn dictation_busy(app: &AppHandle) -> bool {
    app.try_state::<crate::AppState>()
        .and_then(|s| {
            s.pipeline
                .lock()
                .ok()
                .map(|g| g.as_ref().is_some_and(|p| p.is_busy()))
        })
        .unwrap_or(false)
}

/// Run `call`, giving up (`None`) as soon as `busy()` turns true (checked
/// every 100 ms). Dropping a reqwest future closes its connection.
pub async fn unless_busy<T>(call: impl Future<Output = T>, busy: impl Fn() -> bool) -> Option<T> {
    let watch = async {
        loop {
            if busy() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    tokio::select! {
        out = call => Some(out),
        _ = watch => None,
    }
}

/// Wait until `busy()` is false — plus [`IDLE_GRACE`] if it was busy — or
/// give up (`false`) once `wanted()` is false.
async fn wait_idle(busy: impl Fn() -> bool, wanted: impl Fn() -> bool) -> bool {
    let mut was_busy = false;
    loop {
        if !wanted() {
            return false;
        }
        if busy() {
            was_busy = true;
            tokio::time::sleep(Duration::from_millis(200)).await;
        } else if was_busy {
            was_busy = false;
            tokio::time::sleep(IDLE_GRACE).await;
        } else {
            return true;
        }
    }
}

enum DigestError {
    /// A dictation started; the digest was dropped to free the local model.
    Yielded,
    Failed(String),
}

/// Digest `transcript[from..to]` of `note` (not stored: the caller does).
async fn digest_window(
    app: &AppHandle,
    note: &Note,
    from: usize,
    to: usize,
    ep: &Endpoint,
    yield_to_dictation: bool,
) -> Result<MeetingDigest, DigestError> {
    let segs = &note.transcript[from..to];
    let (first, last) = match (segs.first(), segs.last()) {
        (Some(f), Some(l)) => (f, l),
        _ => return Err(DigestError::Failed("empty window".to_string())),
    };
    let t0 = meeting_t0(note);
    let lines = transcript_lines(segs);
    let range = format!(
        "{}\u{2013}{}",
        clock(first.ts.saturating_sub(t0)),
        clock(last.ts.saturating_sub(t0))
    );
    let context = context_so_far(&note.digests, t0);
    let messages = digest_messages(
        &note.participants,
        &context,
        note.digests.len() + 1,
        &range,
        &lines,
    );
    let opts = ep.options(DIGEST_REPLY_TOKENS, 0.2);
    let call = crate::llm::chat(
        &ep.base_url,
        &ep.api_key,
        &ep.model,
        &ep.provider,
        messages,
        &opts,
    );
    let reply = if yield_to_dictation {
        unless_busy(call, || dictation_busy(app))
            .await
            .ok_or(DigestError::Yielded)?
    } else {
        call.await
    }
    .map_err(DigestError::Failed)?;
    if reply.trim().is_empty() {
        return Err(DigestError::Failed(
            "the model returned an empty digest".to_string(),
        ));
    }
    let parsed = check_digest(parse_digest(&reply), &note.participants, &lines);
    let raw = if parsed.recognized {
        String::new()
    } else {
        reply.trim().to_string()
    };
    Ok(MeetingDigest {
        from_seg: from,
        to_seg: to,
        start_ts: first.ts,
        end_ts: last.ts,
        key_points: parsed.key_points,
        decisions: parsed.decisions,
        actions: parsed.actions,
        questions: parsed.questions,
        raw,
    })
}

fn store_digest(app: &AppHandle, note_id: u64, digest: MeetingDigest) -> Result<(), String> {
    let count = crate::notes::add_digest(note_id, digest.clone())?;
    tracing::info!(
        note_id,
        part = count,
        segments = digest.to_seg - digest.from_seg,
        tasks = digest.actions.len(),
        "Meeting digest written"
    );
    let _ = app.emit(
        "yap-meeting-digest",
        json!({ "noteId": note_id, "count": count, "digest": digest }),
    );
    Ok(())
}

/// One digest at a time (live or final), so the end-of-meeting step waits for
/// an in-flight live digest instead of writing the same one again.
static DIGEST_TURN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static LIVE_RUNNING: AtomicBool = AtomicBool::new(false);
/// The latest kick's note (0 = none since the worker last looked).
static KICKED: AtomicU64 = AtomicU64::new(0);
static COOLDOWN_UNTIL_MS: AtomicU64 = AtomicU64::new(0);

/// New segments arrived for the meeting recording into `note_id`: write any
/// digest that's now due, in the background. One worker at a time; it keeps
/// going while digests are due (so a backlog catches up), and goes round
/// again for a kick that came in while it was finishing.
pub fn kick(app: &AppHandle, note_id: u64) {
    if now_ms() < COOLDOWN_UNTIL_MS.load(Ordering::SeqCst) {
        return;
    }
    KICKED.store(note_id, Ordering::SeqCst);
    if LIVE_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let note_id = KICKED.swap(0, Ordering::SeqCst);
            if note_id != 0 {
                live_digests(&app, note_id).await;
            }
            LIVE_RUNNING.store(false, Ordering::SeqCst);
            // A kick between the swap above and here found the worker still
            // running: take it (unless a new worker already has).
            if KICKED.load(Ordering::SeqCst) == 0 || LIVE_RUNNING.swap(true, Ordering::SeqCst) {
                break;
            }
        }
    });
}

async fn live_digests(app: &AppHandle, note_id: u64) {
    let recording = || crate::meeting::recording_note() == Some(note_id);
    loop {
        if !recording() {
            return;
        }
        let due = crate::notes::get(note_id).is_some_and(|n| {
            next_window(&n.transcript, crate::notes::digested_upto(&n), Plan::Live).is_some()
        });
        if !due {
            return;
        }
        // No AI set up: nothing to do now; the end-of-meeting step says how.
        let Ok(ep) = resolve_endpoint(&crate::config::load()) else {
            return;
        };
        let local = ep.is_local();
        if local && !wait_idle(|| dictation_busy(app), recording).await {
            return;
        }
        let _turn = DIGEST_TURN.lock().await;
        // Re-plan under the turn: the end-of-meeting step may have run.
        let Some(note) = crate::notes::get(note_id) else {
            return;
        };
        let Some((from, to)) = next_window(
            &note.transcript,
            crate::notes::digested_upto(&note),
            Plan::Live,
        ) else {
            return;
        };
        match digest_window(app, &note, from, to, &ep, local).await {
            Ok(digest) => {
                if let Err(e) = store_digest(app, note_id, digest) {
                    tracing::warn!("Meeting digest not stored: {}", e);
                    return;
                }
            }
            Err(DigestError::Yielded) => {
                tracing::info!("Meeting digest set aside for a dictation; retrying after it");
            }
            Err(DigestError::Failed(e)) => {
                tracing::warn!(
                    "Meeting digest failed (retrying in a couple of minutes): {}",
                    e
                );
                COOLDOWN_UNTIL_MS.store(now_ms() + FAILURE_COOLDOWN_MS, Ordering::SeqCst);
                return;
            }
        }
    }
}

/// Before an action runs on a meeting note: digest what the final call can't
/// take raw (a backlog, or a note recorded before digests existed), with
/// `yap-meeting-summary-progress` events. Waits for an in-flight live digest
/// rather than duplicating it. Returns the up-to-date note.
pub async fn prepare_final(app: &AppHandle, note_id: u64, ep: &Endpoint) -> Result<Note, String> {
    let _turn = DIGEST_TURN.lock().await;
    let mut done = 0;
    loop {
        let note = crate::notes::get(note_id).ok_or("Note not found")?;
        if fits_single_pass(&note) {
            return Ok(note);
        }
        let upto = crate::notes::digested_upto(&note);
        let keep = FINAL_TAIL_TOKENS;
        let Some((from, to)) = next_window(&note.transcript, upto, Plan::Final { keep }) else {
            return Ok(note);
        };
        let total = done + remaining_windows(&note.transcript, upto, keep);
        let _ = app.emit(
            "yap-meeting-summary-progress",
            json!({ "noteId": note_id, "done": done, "total": total }),
        );
        let digest = match digest_window(app, &note, from, to, ep, false).await {
            Ok(d) => d,
            Err(DigestError::Failed(e)) => {
                return Err(format!(
                    "Couldn't summarise part {} of the meeting: {e}",
                    note.digests.len() + 1
                ))
            }
            Err(DigestError::Yielded) => return Err("Summary interrupted".to_string()),
        };
        store_digest(app, note_id, digest)?;
        done += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(source: &str, text: &str, ts: u64) -> TranscriptSegment {
        TranscriptSegment {
            source: source.to_string(),
            text: text.to_string(),
            ts,
            echo: false,
        }
    }

    /// `n` segments of ~`words` words each, `every` seconds apart from t=1000
    /// (each starts with its own "sN" so no two are alike).
    fn talk(n: usize, words: usize, every: u64) -> Vec<TranscriptSegment> {
        (0..n)
            .map(|i| {
                let mut text = format!("s{i}");
                for w in 1..words {
                    text.push_str(&format!(" word{}", (i + w) % 97));
                }
                seg(
                    if i % 2 == 0 { "you" } else { "them" },
                    &text,
                    1000 + i as u64 * every,
                )
            })
            .collect()
    }

    fn note_with(transcript: Vec<TranscriptSegment>) -> Note {
        let mut note: Note = serde_json::from_value(json!({
            "id": 1, "title": "Planning", "content": "", "createdTs": 1, "updatedTs": 1,
            "participants": ["Alice", "Bob Stone"],
        }))
        .unwrap();
        note.note_type = "meeting".to_string();
        note.transcript = transcript;
        note
    }

    #[test]
    fn token_estimate_runs_high() {
        // 35 ASCII chars → 10 tokens; a CJK char counts as one.
        assert_eq!(estimate_tokens(&"a".repeat(35)), 10);
        assert_eq!(estimate_tokens("会議"), 2);
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn clock_formats() {
        assert_eq!(clock(5), "0:05");
        assert_eq!(clock(605), "10:05");
        assert_eq!(clock(3725), "1:02:05");
    }

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
    fn echo_segments_are_left_out() {
        let mut t = talk(4, 10, 15);
        t[1].echo = true;
        let lines = transcript_lines(&t);
        assert_eq!(lines.lines().count(), 3);
        assert_eq!(seg_tokens(&t[1]), 0);
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

    fn digest(
        from: usize,
        to: usize,
        start: u64,
        points: &[&str],
        actions: &[(&str, &str, &str)],
    ) -> MeetingDigest {
        MeetingDigest {
            from_seg: from,
            to_seg: to,
            start_ts: start,
            end_ts: start + 600,
            key_points: points.iter().map(|s| s.to_string()).collect(),
            decisions: vec![format!("decision at {start}")],
            actions: actions
                .iter()
                .map(|(o, t, d)| DigestAction {
                    owner: o.to_string(),
                    task: t.to_string(),
                    due: d.to_string(),
                })
                .collect(),
            questions: vec![],
            raw: String::new(),
        }
    }

    #[test]
    fn final_input_stays_in_budget_for_any_length() {
        // A 2-hour and an 8-hour meeting, digested as they went (live) and
        // then as `prepare_final` would, with a long tail of typed notes.
        for (hours, words) in [(2u64, 40), (8, 40), (2, 47)] {
            let n = (hours * 3600 / 15) as usize;
            let mut note = note_with(talk(n, words, 15));
            let mut from = 0;
            let plans = [
                Plan::Live,
                Plan::Final {
                    keep: FINAL_TAIL_TOKENS,
                },
            ];
            for plan in plans {
                while let Some((f, to)) = next_window(&note.transcript, from, plan) {
                    let start = note.transcript[f].ts;
                    let points: Vec<String> = (0..6)
                        .map(|i| {
                            format!("a fairly detailed key point number {i} about the discussion")
                        })
                        .collect();
                    let points: Vec<&str> = points.iter().map(|s| s.as_str()).collect();
                    let task = format!("task from {f}");
                    note.digests
                        .push(digest(f, to, start, &points, &[("Alice", &task, "")]));
                    from = to;
                }
            }
            note.content = "typed notes ".repeat(400);
            let input = compose_meeting_input(&note);
            let tokens = estimate_tokens(&input.text);
            assert!(
                tokens <= FINAL_INPUT_TOKENS + 50,
                "{hours}h: {tokens} tokens"
            );
            assert!(input.text.contains("## Digest of the meeting so far"));
            assert_eq!(
                input.text.contains("## Transcript of the last part"),
                from < note.transcript.len()
            );
            // The raw opening of the meeting is not in it.
            assert!(!input.text.contains(&note.transcript[0].text));
            assert_eq!(input.digest_actions.len(), note.digests.len());
        }
    }

    #[test]
    fn short_meeting_keeps_the_single_pass() {
        let note = note_with(talk(20, 20, 15));
        assert!(fits_single_pass(&note));
        let input = compose_meeting_input(&note);
        assert!(input.text.contains("## Meeting Transcript"));
        assert!(input.text.contains(&note.transcript[0].text));
    }

    #[test]
    fn postcheck_moves_invented_owners_and_restores_lost_tasks() {
        let attendees = vec!["Alice".to_string(), "Bob Stone".to_string()];
        let source = "Them: Alice will send the revised budget by Friday. You: Bob, the venue?";
        let reply = "The team planned the offsite.\n\n## Action plan\n\n### Alice\n- [ ] Send the revised budget (due: Friday)\n\n### Mallory\n- [ ] Order the pizza (due: Tuesday)\n\n## Decisions\n- Offsite in May\n";
        let digest_actions = vec![
            DigestAction {
                owner: "Alice".into(),
                task: "send the revised budget".into(),
                due: "Friday".into(),
            },
            DigestAction {
                owner: "Bob Stone".into(),
                task: "book the venue".into(),
                due: String::new(),
            },
            DigestAction {
                owner: UNASSIGNED.into(),
                task: "update the wiki".into(),
                due: String::new(),
            },
        ];
        let out = postcheck_action_plan(reply, &attendees, source, &digest_actions);
        assert!(!out.contains("Mallory"), "{out}");
        assert!(
            out.contains("### Alice\n- [ ] Send the revised budget (due: Friday)"),
            "{out}"
        );
        assert!(out.contains("### Bob Stone\n- [ ] book the venue"), "{out}");
        // Mallory's task lost its made-up deadline and moved to Unassigned,
        // with the lost unassigned task.
        let un = out.split("## Unassigned").nth(1).expect(&out);
        assert!(
            un.contains("- [ ] Order the pizza\n") || un.contains("- [ ] Order the pizza"),
            "{out}"
        );
        assert!(!un.contains("Tuesday"), "{out}");
        assert!(un.contains("- [ ] update the wiki"), "{out}");
        // The action plan comes before Decisions, Unassigned last.
        let (plan, dec, una) = (
            out.find("## Action plan").unwrap(),
            out.find("## Decisions").unwrap(),
            out.find("## Unassigned").unwrap(),
        );
        assert!(plan < dec && dec < una, "{out}");
        // Nothing to fix: unchanged.
        let clean = "Summary.\n\n## Action plan\n\n### You\n- [ ] Send the deck";
        let you = [DigestAction {
            owner: "You".into(),
            task: "send the deck".into(),
            due: String::new(),
        }];
        assert_eq!(
            postcheck_action_plan(clean, &attendees, "You: I'll send the deck", &you),
            clean
        );
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

    #[test]
    fn ask_context_is_bounded() {
        let mut note = note_with(talk(600, 40, 15));
        assert!(estimate_tokens(&ask_context(&note, 3_000)) <= 3_050);
        note.digests.push(digest(
            0,
            300,
            1000,
            &["early point"],
            &[("Alice", "do a thing", "")],
        ));
        let c = ask_context(&note, 3_000);
        assert!(c.contains("early point"));
        assert!(estimate_tokens(&c) <= 3_100);
    }

    #[test]
    fn local_endpoints_are_recognized() {
        let ep = |provider: &str, url: &str| Endpoint {
            base_url: url.into(),
            api_key: String::new(),
            model: "m".into(),
            provider: provider.into(),
            fragment: String::new(),
            disable_thinking: false,
        };
        assert!(ep("ondevice", "http://127.0.0.1:5000/v1").is_local());
        assert!(ep("custom", "http://localhost:11434/v1").is_local());
        assert!(!ep("groq", "https://api.groq.com/openai/v1").is_local());
        assert_eq!(
            ep("custom", "http://127.0.0.1:1/v1")
                .options(700, 0.2)
                .max_tokens,
            Some(700)
        );
        assert_eq!(
            ep("groq", "https://api.groq.com/openai/v1")
                .options(700, 0.2)
                .max_tokens,
            None
        );
    }

    #[tokio::test]
    async fn a_call_gives_way_when_busy() {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;
        let busy = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&busy);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            flag.store(true, Ordering::SeqCst);
        });
        let slow = async {
            tokio::time::sleep(Duration::from_secs(5)).await;
            "done"
        };
        let started = std::time::Instant::now();
        let out = unless_busy(slow, || busy.load(Ordering::SeqCst)).await;
        assert_eq!(out, None);
        assert!(started.elapsed() < Duration::from_secs(2));
        // Not busy: the call finishes.
        assert_eq!(unless_busy(async { 7 }, || false).await, Some(7));
    }

    #[tokio::test]
    async fn waiting_for_idle_stops_when_unwanted() {
        assert!(wait_idle(|| false, || true).await);
        assert!(!wait_idle(|| true, || false).await);
    }
}

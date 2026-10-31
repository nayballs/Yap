//! The notepad's AI helpers' inputs, each a bounded slice of the
//! meeting: "What did I miss?", the AI title, questions to ask, the
//! follow-up email, the gaps; and a chat call that steps aside for a
//! dictation.

use serde_json::{json, Value};
use tauri::AppHandle;

use crate::ai::llm::ChatOptions;
use crate::notes::{MeetingDigest, Note};

use super::action_plan::{ask_context, render_digests};
use super::digest::context_so_far;
use super::{
    clock, dictation_busy, estimate_tokens, keep_last_tokens, meeting_t0, people_lines, seg_tokens, transcript_lines,
    truncate_to_tokens, unless_busy, wait_idle, Endpoint, FINAL_NOTES_TOKENS,
};

// ---- The notepad's helpers: "What did I miss?" and the meeting title ----
//
// Both read a bounded slice of the meeting, like `ask_context`, so they fit
// an 8k local model with room to spare (≤ ~3.5k in, a few hundred out), and
// both go through [`chat_beside_dictation`], so they never slow a dictation.

/// New talk up to this size goes to "What did I miss?" as it is…
pub const CATCH_UP_RAW_TOKENS: usize = 2_400;
/// …a longer stretch as the digests that cover it (this much)…
const CATCH_UP_DIGEST_TOKENS: usize = 1_000;
/// …and the raw talk after them (this much).
const CATCH_UP_TAIL_TOKENS: usize = 1_400;
/// Earlier talk handed over as context (on top of the digests' key points).
const CATCH_UP_CONTEXT_TOKENS: usize = 300;
/// Reply cap for a catch-up answer (local models only).
pub const CATCH_UP_REPLY_TOKENS: u32 = 400;
/// A follow-up question reads this much of the meeting (`ask_context`).
const MEETING_ASK_TOKENS: usize = 2_600;
/// The AI title reads this much of the start of the meeting…
const TITLE_TRANSCRIPT_TOKENS: usize = 700;
/// …and replies in at most this much (local models only).
pub const TITLE_REPLY_TOKENS: u32 = 30;

/// What "What did I miss?" sends: everything said since segment `since`
/// (where the person last looked), with a little of what came before as
/// context. The new part is the raw transcript when it fits
/// [`CATCH_UP_RAW_TOKENS`], else the digests covering it plus its latest
/// stretch. `None` when nothing was said since (echo and dictation markers
/// don't count).
pub fn catch_up_input(note: &Note, since: usize) -> Option<String> {
    let since = since.min(note.transcript.len());
    let new = &note.transcript[since..];
    let first = new.iter().find(|s| s.is_talk())?;
    let last = new.iter().rev().find(|s| s.is_talk())?;
    let t0 = meeting_t0(note);

    // Earlier in the meeting: the digests that end before `since`, then the
    // raw talk between them and `since`.
    let earlier: Vec<MeetingDigest> = note
        .digests
        .iter()
        .filter(|d| d.to_seg <= since)
        .cloned()
        .collect();
    let mut context = String::new();
    if !earlier.is_empty() {
        context.push_str(&context_so_far(&earlier, t0));
        context.push('\n');
    }
    let from = earlier.last().map(|d| d.to_seg).unwrap_or(0).min(since);
    let before = transcript_lines(&note.speakers, &note.transcript[from..since]);
    if !before.trim().is_empty() {
        context.push_str(&keep_last_tokens(&before, CATCH_UP_CONTEXT_TOKENS));
    }
    if context.trim().is_empty() {
        context = "(Nothing: this is the start of the meeting.)".to_string();
    }

    let raw = transcript_lines(&note.speakers, new);
    let body = if estimate_tokens(&raw) <= CATCH_UP_RAW_TOKENS {
        raw
    } else {
        let covering: Vec<MeetingDigest> = note
            .digests
            .iter()
            .filter(|d| d.to_seg > since)
            .cloned()
            .collect();
        if covering.is_empty() {
            keep_last_tokens(&raw, CATCH_UP_RAW_TOKENS)
        } else {
            let upto = crate::notes::digested_upto(note).clamp(since, note.transcript.len());
            let tail = transcript_lines(&note.speakers, &note.transcript[upto..]);
            let mut body = format!(
                "Notes on the first part of it:\n{}",
                render_digests(&covering, t0, CATCH_UP_DIGEST_TOKENS)
            );
            if !tail.trim().is_empty() {
                body.push_str(&format!(
                    "\nWhat was said after that:\n{}",
                    keep_last_tokens(&tail, CATCH_UP_TAIL_TOKENS)
                ));
            }
            body
        }
    };
    Some(format!(
        "{}\nEarlier in the meeting (context only):\n{}\n\nSince they last looked ({}\u{2013}{}):\n{}",
        people_lines(note),
        context.trim_end(),
        clock(first.ts.saturating_sub(t0)),
        clock(last.ts.saturating_sub(t0)),
        body.trim_end()
    ))
}

/// The chat messages for "What did I miss?" over [`catch_up_input`].
pub fn catch_up_messages(input: &str) -> Value {
    json!([
        { "role": "system", "content": crate::ai::llm::CATCH_UP_PROMPT },
        { "role": "user", "content": input },
    ])
}

/// The chat messages for a follow-up question about the meeting: attendees,
/// the notes the person typed and the meeting as [`ask_context`] bounds it.
pub fn meeting_ask_messages(note: &Note, question: &str) -> Value {
    let mut input = people_lines(note);
    let typed = note.content.trim();
    if !typed.is_empty() {
        input.push_str(&format!(
            "\nNotes they typed:\n{}\n",
            truncate_to_tokens(typed, FINAL_NOTES_TOKENS)
        ));
    }
    if !note.transcript.is_empty() {
        input.push('\n');
        input.push_str(&ask_context(note, MEETING_ASK_TOKENS));
    }
    json!([
        { "role": "system", "content": crate::ai::llm::MEETING_ASK_PROMPT },
        { "role": "user", "content": format!("{}\n\nQuestion: {}", input.trim_end(), question.trim()) },
    ])
}

const TITLE_EXAMPLE_IN: &str = "Attendees: (not given)\n\nTranscript (start):\nYou: Thanks for jumping on, Priya. I want to lock the launch date today.\nThem: Sure. Marketing needs a week's notice for the press release.\nYou: Then let's say March 3rd and work back from there.\n";
const TITLE_EXAMPLE_OUT: &str = "Launch Date Planning with Priya";

/// The chat messages for the AI meeting title: the start of the meeting
/// (where people say what it's about), the attendees, and the first digests'
/// key points when there are some. One-shot, so a small model answers with a
/// title and nothing else.
pub fn title_messages(note: &Note) -> Value {
    let lines = truncate_to_tokens(&transcript_lines(&note.speakers, &note.transcript), TITLE_TRANSCRIPT_TOKENS);
    let points: Vec<String> = note
        .digests
        .iter()
        .take(2)
        .flat_map(|d| d.key_points.iter().take(3))
        .map(|p| format!("- {p}"))
        .collect();
    let points = if points.is_empty() {
        String::new()
    } else {
        format!("Main points so far:\n{}\n\n", points.join("\n"))
    };
    let user = format!("{}\n{points}Transcript (start):\n{lines}", people_lines(note));
    json!([
        { "role": "system", "content": crate::ai::llm::MEETING_TITLE_PROMPT },
        { "role": "user", "content": TITLE_EXAMPLE_IN },
        { "role": "assistant", "content": TITLE_EXAMPLE_OUT },
        { "role": "user", "content": user },
    ])
}

/// The longest title kept (cut at a word).
const TITLE_MAX_CHARS: usize = 60;

/// A title reply read back as a title: its first line without markdown,
/// quotes, a "Title:" label or a trailing full stop, at most 60 characters.
/// `None` when it isn't one (empty, a refusal, a sentence of chat).
pub fn clean_title(reply: &str) -> Option<String> {
    let line = reply.lines().map(str::trim).find(|l| !l.is_empty())?;
    let mut t = line.trim_start_matches(['#', '*', '-', '>', ' ']).trim().to_string();
    // "Title: …", "Meeting title - …", "Here's a title: …"
    if let Some((head, rest)) = t.split_once([':', '\u{2013}', '\u{2014}']) {
        if head.to_lowercase().contains("title") && !rest.trim().is_empty() {
            t = rest.trim().to_string();
        }
    }
    const WRAPS: [char; 9] = ['"', '\'', '\u{201c}', '\u{201d}', '\u{2018}', '\u{2019}', '*', '`', '_'];
    let t = t.trim_matches(|c: char| WRAPS.contains(&c) || c.is_whitespace());
    let t = t.trim_end_matches(['.', ',', ';', ':']).trim();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let lower = t.to_lowercase();
    const NOT_A_TITLE: [&str; 7] = [
        "i'm sorry", "i am sorry", "sorry", "i can't", "i cannot", "as an ai", "unfortunately",
    ];
    if t.is_empty() || NOT_A_TITLE.iter().any(|p| lower.starts_with(p)) {
        return None;
    }
    if t.split_whitespace().count() > 12 {
        return None;
    }
    if t.chars().count() <= TITLE_MAX_CHARS {
        return Some(t);
    }
    let mut cut = String::new();
    for word in t.split_whitespace() {
        if cut.chars().count() + word.chars().count() + 1 > TITLE_MAX_CHARS {
            break;
        }
        if !cut.is_empty() {
            cut.push(' ');
        }
        cut.push_str(word);
    }
    (!cut.is_empty()).then_some(cut)
}

// ---- The notepad's one-tap helpers: questions to ask, the follow-up email, the gaps ----
//
// Built like "What did I miss?": a bounded slice of the meeting (talk only),
// ≤ ~2.9k tokens in and ≤ 600 out (≤ ~3.4k in all; far less for a meeting
// without an invite or long typed notes), so an 8k local model has room to
// spare and one call stays well inside a free cloud tier's per-minute token
// budget (Groq's is 8,000), and run through [`chat_beside_dictation`].

/// Fewer words of speech than this, with nothing else to go on (no digests,
/// notes, summary or invite), is too little for a helper: it says so without
/// calling a model.
pub const ASSIST_MIN_WORDS: usize = crate::meetings::meeting_end::MISTAKE_WORDS;
/// A helper reads at most this much of the calendar invite…
const ASSIST_INVITE_TOKENS: usize = 200;
/// …and of the notes the person typed.
const ASSIST_NOTES_TOKENS: usize = 300;
/// "Questions to ask" reads the latest talk, this much…
pub const QUESTIONS_RECENT_TOKENS: usize = 1_200;
/// …and the digests of what came before it.
const QUESTIONS_DIGEST_TOKENS: usize = 500;
/// After the meeting, the email and the gaps read Yap's notes on it (the
/// action plan, or what another action wrote), at most this much…
const RECAP_SUMMARY_TOKENS: usize = 1_200;
/// …and the meeting itself ([`ask_context`]): this much when there are no
/// such notes yet…
const RECAP_MEETING_TOKENS: usize = 1_800;
/// …beside them, this much for the email (mostly how the meeting ended)…
pub const EMAIL_BESIDE_SUMMARY_TOKENS: usize = 500;
/// …and this much for the gaps (what was said, to see what wasn't followed up).
pub const GAPS_BESIDE_SUMMARY_TOKENS: usize = 800;
/// Reply caps (local models only, see `ChatOptions::max_tokens`).
pub const QUESTIONS_REPLY_TOKENS: u32 = 300;
pub const FOLLOW_UP_REPLY_TOKENS: u32 = 600;
pub const BLIND_SPOTS_REPLY_TOKENS: u32 = 500;

/// The calendar invite's agenda for a helper, bounded.
fn invite_part(note: &Note) -> Option<String> {
    crate::calendar::invite_context(note)
        .map(|i| format!("{}\n", truncate_to_tokens(i.trim_end(), ASSIST_INVITE_TOKENS)))
}

/// What "Questions to ask" sends while a meeting records: the latest talk
/// (≤ [`QUESTIONS_RECENT_TOKENS`]), the digests of what came before it, the
/// attendees, the invite and the notes the person typed. `None` when there's
/// too little to go on: under [`ASSIST_MIN_WORDS`] of speech, no digest and
/// no invite to ask about.
pub fn questions_input(note: &Note) -> Option<String> {
    let invite = invite_part(note);
    let words = crate::notes::speech_words(&note.transcript);
    if words < ASSIST_MIN_WORDS && note.digests.is_empty() && invite.is_none() {
        return None;
    }
    let t0 = meeting_t0(note);
    // The latest talk: back from the end until the budget's spent (at least
    // the last line).
    let mut from = note.transcript.len();
    let mut used = 0;
    while from > 0 {
        let t = seg_tokens(&note.transcript[from - 1]);
        if used > 0 && used + t > QUESTIONS_RECENT_TOKENS {
            break;
        }
        used += t;
        from -= 1;
    }
    let mut out = people_lines(note);
    if let Some(invite) = invite {
        out.push('\n');
        out.push_str(&invite);
    }
    let typed = note.content.trim();
    if !typed.is_empty() {
        out.push_str(&format!(
            "\nNotes they typed:\n{}\n",
            truncate_to_tokens(typed, ASSIST_NOTES_TOKENS)
        ));
    }
    let earlier: Vec<MeetingDigest> = note
        .digests
        .iter()
        .filter(|d| d.from_seg < from)
        .cloned()
        .collect();
    if !earlier.is_empty() {
        out.push_str(&format!(
            "\nEarlier in the meeting (Yap's notes, context only):\n{}",
            render_digests(&earlier, t0, QUESTIONS_DIGEST_TOKENS)
        ));
    }
    let recent = &note.transcript[from..];
    match (
        recent.iter().find(|s| s.is_talk()),
        recent.iter().rev().find(|s| s.is_talk()),
    ) {
        (Some(first), Some(last)) => out.push_str(&format!(
            "\nThe latest talk ({}\u{2013}{}):\n{}",
            clock(first.ts.saturating_sub(t0)),
            clock(last.ts.saturating_sub(t0)),
            keep_last_tokens(&transcript_lines(&note.speakers, recent), QUESTIONS_RECENT_TOKENS)
        )),
        _ => out.push_str("\nThe latest talk:\n(Nothing has been said yet.)\n"),
    }
    Some(out)
}

/// What the after-the-meeting helpers send (the follow-up email, the gaps):
/// its title when the person gave it one, the attendees, the invite, the
/// notes the person typed, Yap's notes on it when there are some (the action
/// plan, ≤ [`RECAP_SUMMARY_TOKENS`]) and the meeting as [`ask_context`]
/// bounds it — [`RECAP_MEETING_TOKENS`] of it without those notes,
/// `beside_summary` with them. `None` when there's nothing to go on.
pub fn recap_input(note: &Note, beside_summary: usize) -> Option<String> {
    let summary = note.enhanced_content.trim();
    let typed = note.content.trim();
    let words = crate::notes::speech_words(&note.transcript);
    if summary.is_empty() && typed.is_empty() && note.digests.is_empty() && words < ASSIST_MIN_WORDS {
        return None;
    }
    let mut out = String::new();
    if !crate::notes::title_open_to_ai(note) {
        out.push_str(&format!("Meeting: {}\n", note.title.trim()));
    }
    out.push_str(&people_lines(note));
    if let Some(invite) = invite_part(note) {
        out.push('\n');
        out.push_str(&invite);
    }
    if !typed.is_empty() {
        out.push_str(&format!(
            "\nNotes the user typed:\n{}\n",
            truncate_to_tokens(typed, ASSIST_NOTES_TOKENS)
        ));
    }
    let budget = if summary.is_empty() {
        RECAP_MEETING_TOKENS
    } else {
        out.push_str(&format!(
            "\nYap's notes on the meeting (written after it):\n{}\n",
            truncate_to_tokens(summary, RECAP_SUMMARY_TOKENS)
        ));
        beside_summary
    };
    if budget > 0 && note.transcript.iter().any(|s| s.is_talk()) {
        out.push('\n');
        out.push_str(&ask_context(note, budget));
    }
    Some(out)
}

/// The chat messages for a helper: its immutable `system` prompt over `input`.
pub fn helper_messages(system: &str, input: &str) -> Value {
    json!([
        { "role": "system", "content": system },
        { "role": "user", "content": input },
    ])
}

/// The subject a follow-up email gets when the model left it out.
pub fn email_subject(note: &Note) -> String {
    if crate::notes::title_open_to_ai(note) {
        "Meeting follow-up".to_string()
    } else {
        format!("Follow-up: {}", note.title.trim())
    }
}

/// One line of an email reply without markdown: heading marks and `**` gone,
/// "* " bullets as "- ".
fn plain_email_line(line: &str) -> String {
    let line = line.trim_end().replace("**", "");
    let indent = line.len() - line.trim_start().len();
    let (lead, rest) = line.split_at(indent);
    let rest = if rest.starts_with('#') {
        rest.trim_start_matches('#').trim_start()
    } else {
        rest
    };
    match rest.strip_prefix("* ") {
        Some(item) => format!("{lead}- {item}"),
        None => format!("{lead}{rest}"),
    }
}

/// A follow-up email reply made ready to paste as plain text: anything
/// before the subject line ("Here's a draft:") and code fences dropped,
/// markdown taken out ([`plain_email_line`]), at most one blank line in a
/// row, and a "Subject: " line first — `subject` when the model left it out.
/// A one-line reply without a subject (the model saying there's too little)
/// comes back as it is.
pub fn clean_email(reply: &str, subject: &str) -> String {
    let mut lines: Vec<String> = reply
        .lines()
        .filter(|l| !l.trim_start().starts_with("```"))
        .map(plain_email_line)
        .collect();
    let subject_line = |l: &str| {
        let t = l.trim();
        t.get(..8)
            .filter(|head| head.eq_ignore_ascii_case("subject:"))
            .map(|_| t[8..].trim().to_string())
    };
    match lines.iter().position(|l| subject_line(l).is_some()) {
        Some(i) => {
            let said = subject_line(&lines[i]).unwrap_or_default();
            lines.drain(..i);
            lines[0] = format!("Subject: {}", if said.is_empty() { subject } else { said.as_str() });
        }
        None => {
            let said: Vec<&String> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
            if said.len() <= 1 {
                return said.first().map(|l| l.trim().to_string()).unwrap_or_default();
            }
            lines.splice(0..0, [format!("Subject: {subject}"), String::new()]);
        }
    }
    let mut out = String::new();
    let mut blank = false;
    for line in &lines {
        if line.trim().is_empty() {
            blank = !out.is_empty();
            continue;
        }
        if blank {
            out.push('\n');
            blank = false;
        }
        out.push_str(line);
        out.push('\n');
    }
    // The subject line is followed by one blank line.
    if let Some((first, rest)) = out.split_once('\n') {
        if !rest.is_empty() && !rest.starts_with('\n') {
            return format!("{first}\n\n{}", rest.trim_end());
        }
    }
    out.trim_end().to_string()
}

/// A local model stepping aside for a dictation this many times in a row
/// gives up (the person is dictating nonstop; they can ask again).
const YIELD_RETRIES: usize = 5;

/// One meeting-helper call that never slows a dictation. On a local endpoint
/// (one request at a time) it waits until no dictation is recording or
/// transcribing (+ a short grace), and if one starts mid-call it drops the
/// request — llama.cpp's server treats the closed connection as a cancel —
/// then asks again once the dictation is done. Cloud endpoints just run.
pub(crate) async fn chat_beside_dictation(
    app: &AppHandle,
    ep: &Endpoint,
    messages: Value,
    opts: &ChatOptions,
) -> Result<String, String> {
    chat_beside_dictation_with(app, ep, messages, opts, serde_json::Map::new()).await
}

/// [`chat_beside_dictation`], **streamed** to `sink` when someone watches
/// (the notepad's Ask sheet, `stream.rs`): the answer as it's written, and
/// the same rule beside a dictation — on a model on this PC it waits one out
/// first and drops the stream when one starts, then asks again. `asked`: a
/// question the person typed (the meeting's Ask), which goes at the model's
/// effort (`ai/effort.rs`); the one-tap helpers keep their quick requests.
pub(crate) async fn chat_beside_dictation_live(
    app: &AppHandle,
    ep: &Endpoint,
    messages: Value,
    opts: &ChatOptions,
    sink: Option<&dyn crate::chat::stream::Sink>,
    asked: bool,
) -> Result<String, String> {
    let mut req = crate::ai::llm::ChatRequest::with_options(
        &ep.base_url,
        &ep.api_key,
        &ep.model,
        &ep.provider,
        messages,
        opts,
        serde_json::Map::new(),
    );
    let effort = asked && crate::ai::effort::apply(&mut req);
    let Some(sink) = sink else {
        if effort {
            let message = crate::privacy::net::within(crate::privacy::net::Feature::Meetings, crate::ai::llm::post_whole(&req)).await?;
            return crate::ai::llm::answer_text(&message, opts.disable_thinking);
        }
        return chat_beside_dictation(app, ep, req.messages, opts).await;
    };
    let beside = crate::chat::stream::AnyDictation(app.clone());
    let live = crate::chat::stream::Live { sink, beside: ep.is_local().then_some(&beside as &dyn crate::chat::stream::Dictations) };
    let message = crate::privacy::net::within(crate::privacy::net::Feature::Meetings, crate::chat::stream::call(live, &req, false, opts.disable_thinking)).await?;
    crate::ai::llm::answer_text(&message, opts.disable_thinking)
}

/// [`chat_beside_dictation`] with fields of the caller's own in the request
/// body (the speaker names' JSON schema for a model on this PC,
/// `speakers/naming.rs`).
pub(crate) async fn chat_beside_dictation_with(
    app: &AppHandle,
    ep: &Endpoint,
    messages: Value,
    opts: &ChatOptions,
    extra: serde_json::Map<String, Value>,
) -> Result<String, String> {
    let call = move |messages: Value| {
        let ask = crate::ai::llm::chat_with(&ep.base_url, &ep.api_key, &ep.model, &ep.provider, messages, opts, extra.clone());
        crate::privacy::net::within(crate::privacy::net::Feature::Meetings, ask)
    };
    if !ep.is_local() {
        return call(messages).await;
    }
    // The on-device model is busy while the call waits out a dictation and
    // asks again, not only while a request is under way.
    let _sidecar = crate::ai::local_llm::hold(&ep.base_url);
    for _ in 0..YIELD_RETRIES {
        wait_idle(|| dictation_busy(app), || true).await;
        match unless_busy(call(messages.clone()), || dictation_busy(app)).await {
            Some(out) => return out,
            None => tracing::info!("Meeting helper call set aside for a dictation; asking again after it"),
        }
    }
    Err("Yap kept dictating, so the answer waited. Ask again in a moment.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meetings::meeting_summary::tests::{call_tokens, digest, note_with, seg, talk, user_turn, with_invite};
    use crate::notes::TranscriptSegment;

    #[test]
    fn catching_up_reads_only_what_was_said_since_you_looked() {
        let mut note = note_with(talk(20, 10, 15));
        let input = catch_up_input(&note, 12).unwrap();
        let (before, since) = input.split_once("Since they last looked").unwrap();
        // The new part: segments 12..20, and nothing older.
        for i in 12..20 {
            assert!(since.contains(&format!("s{i} ")), "s{i}: {since}");
        }
        assert!(!since.contains("s11 "));
        // A little of what came before, as context only.
        assert!(before.contains("Earlier in the meeting (context only)"));
        assert!(before.contains("s11 "));
        assert!(input.starts_with("Attendees: Alice, Bob Stone"));
        // The range is on the meeting's clock.
        assert!(since.starts_with(" (3:00\u{2013}4:45)"), "{since}");
        // Nothing new (or only echo): no call at all.
        assert!(catch_up_input(&note, 20).is_none());
        assert!(catch_up_input(&note, 99).is_none());
        note.transcript[19].echo = true;
        assert!(catch_up_input(&note, 19).is_none());
        // Nor is a dictation the person made in the meantime.
        note.transcript.push(TranscriptSegment::dictation_marker(1000 + 20 * 15));
        assert!(catch_up_input(&note, 19).is_none());
        let since_18 = catch_up_input(&note, 18).unwrap();
        assert!(!since_18.contains("You: \n"), "{since_18}");
        // From the start: no earlier context.
        let first = catch_up_input(&note_with(talk(3, 5, 15)), 0).unwrap();
        assert!(first.contains("this is the start of the meeting"));
    }

    #[test]
    fn a_long_absence_reads_the_digests_and_the_latest_talk() {
        // 2.5 hours, digested in three parts; away since segment 100.
        let mut note = note_with(talk(600, 40, 15));
        note.digests = vec![
            digest(0, 200, 1000, &["early"], &[("Alice", "send the budget", "Friday")]),
            digest(200, 400, 4000, &["middle point"], &[("Bob Stone", "book the venue", "")]),
            digest(400, 520, 7000, &["late point"], &[]),
        ];
        let input = catch_up_input(&note, 100).unwrap();
        assert!(estimate_tokens(&input) <= 3_300, "{}", estimate_tokens(&input));
        assert!(input.contains("Notes on the first part of it"));
        assert!(input.contains("Bob Stone: book the venue"));
        assert!(input.contains("What was said after that"));
        assert!(input.contains("s599 ")); // the latest talk, raw
        assert!(!input.contains("s300 ")); // digested talk isn't sent raw
        // The whole thing fits an 8k model with the prompt and the reply.
        let messages = catch_up_messages(&input);
        let total: usize = messages
            .as_array()
            .unwrap()
            .iter()
            .map(|m| estimate_tokens(m["content"].as_str().unwrap()))
            .sum();
        assert!(total + CATCH_UP_REPLY_TOKENS as usize <= 4_500, "{total}");
        // Without digests, the latest stretch of the raw talk.
        note.digests.clear();
        let raw = catch_up_input(&note, 100).unwrap();
        assert!(estimate_tokens(&raw) <= 3_300);
        assert!(raw.contains("s599 ") && !raw.contains("s150 "));
    }

    #[test]
    fn a_question_reads_a_bounded_meeting() {
        let mut note = note_with(talk(600, 40, 15));
        note.content = "my own notes ".repeat(500);
        let m = meeting_ask_messages(&note, "Who owns the budget?");
        let user = user_turn(&m);
        assert!(user.ends_with("Question: Who owns the budget?"));
        assert!(user.contains("Notes they typed"));
        assert!(estimate_tokens(&user) <= 3_500, "{}", estimate_tokens(&user));
        assert_eq!(m[0]["content"], crate::ai::llm::MEETING_ASK_PROMPT);
    }

    #[test]
    fn the_title_reads_the_start_of_the_meeting() {
        let mut note = note_with(talk(400, 40, 15));
        note.digests = vec![digest(0, 100, 1000, &["Budget for Q3"], &[])];
        let m = title_messages(&note);
        assert_eq!(m.as_array().unwrap().len(), 4); // one-shot
        let user = user_turn(&m);
        assert!(user.contains("s0 "));
        assert!(!user.contains("s399 "));
        assert!(user.contains("- Budget for Q3"));
        assert!(estimate_tokens(&user) <= 900, "{}", estimate_tokens(&user));
    }

    #[test]
    fn title_replies_are_cleaned_up() {
        let t = |s: &str| clean_title(s);
        assert_eq!(t("Q3 Budget Review with Alice").as_deref(), Some("Q3 Budget Review with Alice"));
        assert_eq!(t("\"Launch Plan Sync.\"").as_deref(), Some("Launch Plan Sync"));
        assert_eq!(t("Title: Hiring Update\nThis meeting was about…").as_deref(), Some("Hiring Update"));
        assert_eq!(t("## **Roadmap Planning**").as_deref(), Some("Roadmap Planning"));
        assert_eq!(t("Here's a title: \u{201c}Venue Booking\u{201d}").as_deref(), Some("Venue Booking"));
        assert_eq!(t("  \n\n Weekly   Standup  ").as_deref(), Some("Weekly Standup"));
        assert_eq!(t("I'm sorry, I can't title an empty transcript."), None);
        assert_eq!(t(""), None);
        assert_eq!(t("This meeting covered a lot of different topics including the budget and the venue and hiring"), None);
        // Long: cut at a word, within 60 characters.
        let long = t("Quarterly Planning Review of Marketing Budget Allocations Overall").unwrap();
        assert!(long.chars().count() <= 60 && !long.ends_with(' '), "{long}");
    }

    #[test]
    fn questions_read_the_latest_talk_and_digests_of_the_rest() {
        // 2.5 hours, digested in three parts.
        let mut note = note_with(talk(600, 40, 15));
        note.digests = vec![
            digest(0, 200, 1000, &["early"], &[("Alice", "send the budget", "Friday")]),
            digest(200, 400, 4000, &["middle point"], &[]),
            digest(400, 520, 7000, &["late point"], &[]),
        ];
        note.content = "ask about catering ".repeat(200);
        note = with_invite(note, &"Agenda: budget, venue. ".repeat(100));
        let input = questions_input(&note).unwrap();
        assert!(input.starts_with("Attendees: Alice, Bob Stone"));
        assert!(input.contains("From the calendar invite \"Offsite planning\""));
        assert!(input.contains("Notes they typed"));
        // The latest talk, raw, on the meeting's clock; old talk only as digests.
        let (before, latest) = input.split_once("The latest talk (").unwrap();
        assert!(latest.contains("s599 "));
        assert!(!input.contains("s100 "));
        assert!(before.contains("Earlier in the meeting (Yap's notes"));
        assert!(before.contains("early") && before.contains("Alice: send the budget"));
        let latest_tokens = estimate_tokens(latest);
        assert!(latest_tokens <= QUESTIONS_RECENT_TOKENS + 50, "{latest_tokens}");
        // Lean: well inside an 8k local model and a free cloud tier's minute.
        let total = call_tokens(
            crate::ai::llm::MEETING_QUESTIONS_PROMPT,
            &input,
            QUESTIONS_REPLY_TOKENS,
        );
        assert!(total <= 3_000, "{total}");
    }

    #[test]
    fn questions_skip_echo_and_wait_for_enough_talk() {
        // A few words: too little, unless there's an invite to ask about.
        let mut note = note_with(vec![seg("them", "Hi everyone, can you hear me?", 1000)]);
        assert!(questions_input(&note).is_none());
        let invited = with_invite(note.clone(), "Agenda: pick the venue.");
        let input = questions_input(&invited).unwrap();
        assert!(input.contains("Agenda: pick the venue."));
        // Enough talk; the call through the speakers and dictation markers
        // aren't part of it.
        note.transcript = talk(12, 10, 15);
        note.transcript.push(TranscriptSegment {
            echo: true,
            ..seg("you", "ECHOED line from the speakers", 1200)
        });
        note.transcript.push(TranscriptSegment::dictation_marker(1210));
        let input = questions_input(&note).unwrap();
        assert!(input.contains("s0 ") && input.contains("s11 "), "{input}");
        assert!(!input.contains("ECHOED"));
        assert!(!input.contains("You: \n"));
        assert!(!input.contains("Earlier in the meeting")); // nothing digested
    }

    #[test]
    fn the_recap_reads_the_plan_or_else_the_meeting() {
        let mut note = note_with(talk(600, 40, 15));
        note.digests = vec![digest(0, 300, 1000, &["early point"], &[("Bob Stone", "book the venue", "")])];
        // No action plan yet: the meeting as the Ask bar bounds it.
        let input = recap_input(&note, EMAIL_BESIDE_SUMMARY_TOKENS).unwrap();
        assert!(input.starts_with("Meeting: Planning\nAttendees: Alice, Bob Stone"));
        assert!(input.contains("Meeting digest:") && input.contains("Bob Stone: book the venue"));
        assert!(input.contains("s599 "));
        assert!(!input.contains("Yap's notes on the meeting"));
        // The worst case: long typed notes and a long invite too. Every call
        // stays under ~3.4k with its reply.
        note.content = "my own notes ".repeat(400);
        note = with_invite(note, &"Agenda: budget, venue. ".repeat(100));
        let input = recap_input(&note, EMAIL_BESIDE_SUMMARY_TOKENS).unwrap();
        assert!(input.contains("Notes the user typed") && input.contains("From the calendar invite"));
        for prompt in [crate::ai::llm::MEETING_FOLLOW_UP_PROMPT, crate::ai::llm::MEETING_BLIND_SPOTS_PROMPT] {
            let total = call_tokens(prompt, &input, FOLLOW_UP_REPLY_TOKENS);
            assert!(total <= 3_400, "{total}");
        }
        // With the plan: the plan first, a little of the meeting beside it.
        note.enhanced_content = "Summary.\n\n## Action plan\n### Alice\n- [ ] send the budget (due: Friday)\n".to_string()
            + &"- [ ] another task\n".repeat(400);
        let email = recap_input(&note, EMAIL_BESIDE_SUMMARY_TOKENS).unwrap();
        let gaps = recap_input(&note, GAPS_BESIDE_SUMMARY_TOKENS).unwrap();
        for input in [&email, &gaps] {
            assert!(input.contains("Yap's notes on the meeting (written after it):\nSummary."));
            assert!(input.contains("send the budget (due: Friday)"));
            assert!(input.contains("s599 ")); // how it ended
        }
        assert!(estimate_tokens(&email) < estimate_tokens(&gaps));
        let total = call_tokens(crate::ai::llm::MEETING_BLIND_SPOTS_PROMPT, &gaps, BLIND_SPOTS_REPLY_TOKENS);
        assert!(total <= 3_400, "{total}");
        let total = call_tokens(crate::ai::llm::MEETING_FOLLOW_UP_PROMPT, &email, FOLLOW_UP_REPLY_TOKENS);
        assert!(total <= 3_400, "{total}");
    }

    #[test]
    fn the_recap_needs_something_to_go_on() {
        let mut note = note_with(vec![seg("you", "Testing, one two.", 1000)]);
        assert!(recap_input(&note, EMAIL_BESIDE_SUMMARY_TOKENS).is_none());
        // Typed notes alone are something.
        note.content = "Agreed: launch on the 3rd.".to_string();
        let input = recap_input(&note, EMAIL_BESIDE_SUMMARY_TOKENS).unwrap();
        assert!(input.contains("Notes the user typed:\nAgreed: launch on the 3rd."));
        // A made-up title isn't the meeting's name.
        note.title_auto = true;
        let input = recap_input(&note, EMAIL_BESIDE_SUMMARY_TOKENS).unwrap();
        assert!(input.starts_with("Attendees:"), "{input}");
        assert_eq!(email_subject(&note), "Meeting follow-up");
        note.title_auto = false;
        assert_eq!(email_subject(&note), "Follow-up: Planning");
    }

    #[test]
    fn a_follow_up_email_pastes_as_plain_text() {
        // Markdown, a preamble and a fence: the email alone, in plain text.
        let reply = "Here's a draft you can send:\n\n```\n**Subject:** Q3 budget follow-up\nHi Alice,\n\n\n\nThanks for going through the budget.\n\n## Decisions:\n* The deposit moves to Q3.\n\n**Next steps:**\n- Alice: send the revised budget (Friday)\n\nThanks,\n[Your name]\n```";
        let email = clean_email(reply, "Meeting follow-up");
        assert_eq!(
            email,
            "Subject: Q3 budget follow-up\n\nHi Alice,\n\nThanks for going through the budget.\n\nDecisions:\n- The deposit moves to Q3.\n\nNext steps:\n- Alice: send the revised budget (Friday)\n\nThanks,\n[Your name]"
        );
        // No subject line: it gets one.
        let email = clean_email("Hi all,\nThanks for today.\nThanks,\n[Your name]", "Follow-up: Planning");
        assert!(email.starts_with("Subject: Follow-up: Planning\n\nHi all,\n"), "{email}");
        // An empty subject: the fallback.
        assert!(clean_email("Subject:\n\nHi all,\nBye", "Meeting follow-up").starts_with("Subject: Meeting follow-up\n\nHi all,"));
        // One line saying there's too little: left as it is.
        assert_eq!(
            clean_email("  There isn't enough here for a recap.  ", "Meeting follow-up"),
            "There isn't enough here for a recap."
        );
    }
}

//! The inputs of the notepad's newer chips (2026-10-29, Wispr Flow's
//! Notetaker chips and Skills), each a bounded slice of the meeting that
//! fits an 8k local model:
//!
//! - **"Make me sound smart"** ([`smart_input`], while a meeting records):
//!   the latest talk as numbered passages, with the meeting's title,
//!   attendees, invite, brief, typed notes and earlier digests as context.
//! - **A Skill on one meeting** ([`skill_input`]): Yap's summary, the notes
//!   the person typed and the transcript as numbered passages (the parts of
//!   a long transcript that share the most words with the Skill, else the
//!   latest).
//! - **"Make notes longer"** ([`longer_request`], after a meeting): the
//!   summary as it is, then the meeting as the action plan reads it.
//!
//! The passages are the ones search cuts a note into (`search::chunks`),
//! numbered and laid out as Ask across meetings lays them out
//! (`search::ask::context`), so an answer cites them the same way (`[2]`),
//! goes through the same checks ([`checked`]) and its pills open the
//! meeting at the line. Pure.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

use crate::chat::chats::Source;
use crate::notes::{MeetingDigest, Note};
use crate::search::ask::{self, Found, Passage};
use crate::search::chunks::{self, Chunk, Field};

use super::action_plan::render_digests;
use super::{estimate_tokens, meeting_t0, people_lines, truncate_to_tokens, FinalBudget};

/// "Make me sound smart" reads at most this much of the latest talk…
pub const SMART_TALK_TOKENS: usize = 1_200;
/// …and of the digests of what came before it.
const SMART_DIGEST_TOKENS: usize = 400;
/// Its reply cap (local models only).
pub const SMART_REPLY_TOKENS: u32 = 250;
/// Context: the calendar invite, the brief and the typed notes, each at most this.
const CONTEXT_TOKENS: usize = 250;
/// A Skill on one meeting reads Yap's summary up to this much…
const SKILL_SUMMARY_TOKENS: usize = 1_200;
/// …the person's notes up to this much…
const SKILL_NOTES_TOKENS: usize = 300;
/// …and the transcript up to this much.
const SKILL_TALK_TOKENS: usize = 1_800;
/// Its reply cap (local models only).
pub const SKILL_REPLY_TOKENS: u32 = 700;
/// "Make notes longer" reads the summary as it is up to this much…
const LONGER_SUMMARY_TOKENS: usize = 1_200;
/// …and writes at most this much (local models only).
pub const LONGER_REPLY_TOKENS: u32 = 1_800;
/// Fewer words of talk than this (and nothing else) is too little to go on.
const MIN_WORDS: usize = crate::meetings::meeting_end::MISTAKE_WORDS;

/// A citation as answers write it: "[2]", "[1, 3]", "[1–3]".
static CITE_ANY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\d{1,2}(?:\s*[,–-]\s*\d{1,2})*\]").expect("static regex"));

/// The line an answer that cites none of the passages ends with.
pub const UNCITED_NOTE: &str = "*Yap couldn't tie this to a moment in the meeting: check it against the transcript.*";

/// What a cited helper sends, and the passages its answer may cite.
#[derive(Debug, Clone)]
pub struct Cited {
    pub text: String,
    pub passages: Vec<Passage>,
}

/// A passage's size as the model reads it (its `<passage>` tags too).
fn chunk_tokens(c: &Chunk) -> usize {
    estimate_tokens(&c.text()) + 20
}

/// "Sun 25 Oct 2026, 10:12", on this PC's clock.
fn when(ts: u64) -> String {
    use chrono::TimeZone;
    chrono::Local.timestamp_opt(ts as i64, 0).single().map(|t| t.format("%a %-d %b %Y, %H:%M").to_string()).unwrap_or_default()
}

/// `chosen` as passages numbered from 1, in that order: each opens the
/// meeting at its first line (a transcript passage says how far in).
pub fn numbered(note: &Note, chosen: &[Chunk]) -> Vec<Passage> {
    let start = note.transcript.iter().find(|s| s.is_talk()).map(|s| s.ts).unwrap_or(0);
    chosen
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let line = c.lines.first()?;
            Some(Passage {
                n: i + 1,
                note: note.id,
                title: c.title.clone(),
                kind: c.kind,
                field: c.field,
                line: line.index,
                lines: c.lines.iter().map(|l| l.index).collect(),
                ts: line.ts,
                when: when(line.ts),
                at: (c.field == Field::Transcript).then(|| ask::clock(line.ts.saturating_sub(start))),
                speaker: line.speaker.clone(),
                text: c.text(),
                score: 0.0,
                keyword: 0.0,
                similarity: None,
            })
        })
        .collect()
}

/// The transcript's passages, oldest first.
fn talk(note: &Note) -> Vec<Chunk> {
    chunks::chunk_note(note).into_iter().filter(|c| c.field == Field::Transcript).collect()
}

/// The latest of `list` that fit `budget` (at least the last one), in order.
fn latest(list: Vec<Chunk>, budget: usize) -> Vec<Chunk> {
    let mut used = 0;
    let mut from = list.len();
    while from > 0 {
        let t = chunk_tokens(&list[from - 1]);
        if used > 0 && used + t > budget {
            break;
        }
        used += t;
        from -= 1;
    }
    list.into_iter().skip(from).collect()
}

/// The first of `list` that fit `budget`, in order.
fn first_fitting(list: Vec<Chunk>, budget: usize) -> Vec<Chunk> {
    let mut used = 0;
    let mut out = Vec::new();
    for c in list {
        let t = chunk_tokens(&c);
        if used + t > budget {
            break;
        }
        used += t;
        out.push(c);
    }
    out
}

/// The words a passage and a request share (stop words out, stemmed).
fn shared(terms: &HashSet<String>, c: &Chunk) -> usize {
    crate::search::rank::terms(&c.text()).into_iter().collect::<HashSet<_>>().intersection(terms).count()
}

/// The transcript within `budget`: all of it when it fits; else the
/// passages sharing the most words with `request` (ties: the later), with
/// the latest one always in, back in the order they were said.
fn talk_for(note: &Note, request: &str, budget: usize) -> Vec<Chunk> {
    let all = talk(note);
    if all.iter().map(chunk_tokens).sum::<usize>() <= budget {
        return all;
    }
    let terms: HashSet<String> = crate::search::rank::terms(request).into_iter().collect();
    let last = all.len().saturating_sub(1);
    let mut order: Vec<(usize, usize)> = all.iter().enumerate().map(|(i, c)| (i, shared(&terms, c))).collect();
    // The latest first among equals, and the very last always.
    order.sort_by_key(|&(i, n)| (i != last, std::cmp::Reverse(n), std::cmp::Reverse(i)));
    let mut keep: Vec<usize> = Vec::new();
    let mut used = 0;
    for (i, _) in order {
        let t = chunk_tokens(&all[i]);
        if used + t > budget {
            continue;
        }
        used += t;
        keep.push(i);
    }
    keep.sort_unstable();
    keep.into_iter().map(|i| all[i].clone()).collect()
}

/// The meeting's title for a helper, when the person or the AI gave it one.
fn title_line(note: &Note) -> String {
    let t = note.title.trim();
    if t.is_empty() {
        String::new()
    } else {
        format!("Meeting: {t}\n")
    }
}

/// The calendar invite's agenda, bounded.
fn invite(note: &Note) -> Option<String> {
    crate::calendar::invite_context(note).map(|i| format!("{}\n", truncate_to_tokens(i.trim_end(), CONTEXT_TOKENS)))
}

/// **"Make me sound smart"** while a meeting records: the latest talk
/// (≤ [`SMART_TALK_TOKENS`]) as numbered passages, after the title,
/// attendees, the invite, the brief Yap made before the meeting (`brief`,
/// its lines), the notes the person typed and the digests of what came
/// before (context only). `None`: too little to go on (under 20 words of
/// talk and no digest).
pub fn smart_input(note: &Note, brief: Option<&str>) -> Option<Cited> {
    let words = crate::notes::speech_words(&note.transcript);
    if words < MIN_WORDS && note.digests.is_empty() {
        return None;
    }
    let chosen = latest(talk(note), SMART_TALK_TOKENS);
    if chosen.is_empty() {
        return None;
    }
    let passages = numbered(note, &chosen);
    let mut out = title_line(note);
    out.push_str(&people_lines(note));
    if let Some(i) = invite(note) {
        out.push('\n');
        out.push_str(&i);
    }
    if let Some(b) = brief.map(str::trim).filter(|b| !b.is_empty()) {
        out.push_str(&format!(
            "\nThe brief Yap prepared before the meeting (context only):\n{}\n",
            truncate_to_tokens(b, CONTEXT_TOKENS)
        ));
    }
    let typed = note.content.trim();
    if !typed.is_empty() {
        out.push_str(&format!("\nNotes they typed:\n{}\n", truncate_to_tokens(typed, CONTEXT_TOKENS)));
    }
    let from = passages.first().map(|p| p.line).unwrap_or(0);
    let earlier: Vec<MeetingDigest> = note.digests.iter().filter(|d| d.from_seg < from).cloned().collect();
    if !earlier.is_empty() {
        out.push_str(&format!(
            "\nEarlier in the meeting (Yap's notes, context only):\n{}",
            render_digests(&earlier, meeting_t0(note), SMART_DIGEST_TOKENS)
        ));
    }
    out.push_str("\nThe latest talk, in numbered passages:\n");
    out.push_str(&block(&passages));
    Some(Cited { text: out, passages })
}

/// **A Skill on one meeting**: its title and date, the attendees, the
/// invite, then numbered passages — Yap's summary (≤ 1,200), the notes the
/// person typed (≤ 300) and the transcript ([`talk_for`], ≤ 1,800).
/// `request`: the Skill's instructions and the person's words with it (what
/// the transcript's passages are picked by when it's long; the request
/// itself goes after this: `meeting_assist::skill_messages`). `None`:
/// nothing in the meeting yet.
pub fn skill_input(note: &Note, request: &str) -> Option<Cited> {
    let all = chunks::chunk_note(note);
    let of = |f: Field| all.iter().filter(|c| c.field == f).cloned().collect::<Vec<_>>();
    let mut chosen = first_fitting(of(Field::Summary), SKILL_SUMMARY_TOKENS);
    chosen.extend(first_fitting(of(Field::Content), SKILL_NOTES_TOKENS));
    let talk_words = crate::notes::speech_words(&note.transcript);
    if talk_words > 0 {
        chosen.extend(talk_for(note, request, SKILL_TALK_TOKENS));
    }
    if chosen.is_empty() {
        return None;
    }
    let passages = numbered(note, &chosen);
    let mut out = title_line(note);
    let date = when(chunks::note_time(note));
    if !date.is_empty() {
        out.push_str(&format!("Date: {date}\n"));
    }
    out.push_str(&people_lines(note));
    if let Some(i) = invite(note) {
        out.push('\n');
        out.push_str(&i);
    }
    out.push('\n');
    out.push_str(&block(&passages));
    Some(Cited { text: out, passages })
}

/// The passages as the model reads them (Ask across meetings' layout).
pub fn block(passages: &[Passage]) -> String {
    ask::context(&Found { passages: passages.to_vec(), ..Default::default() })
}

/// A cited answer, checked as Ask across meetings' are: a number that's
/// none of the passages taken out, a statement plainly from one passage
/// that the model left uncited cited by Yap (`ask::cite_missing`), and one
/// that cites nothing while it says something marked ([`UNCITED_NOTE`];
/// `whole`: not for an answer stopped half-written). Returns the answer and
/// the passages it cites, as sources (a pill opens the meeting at the line).
pub fn checked(reply: &str, passages: &[Passage], whole: bool) -> (String, Vec<Source>) {
    let numbers: Vec<usize> = passages.iter().map(|p| p.n).collect();
    // One passage: whatever number the model put on it, it can only be that
    // one (a 4B model wrote "[3]" over a meeting of one passage).
    let reply = match passages {
        [only] => CITE_ANY.replace_all(reply.trim(), format!("[{}]", only.n).as_str()).into_owned(),
        _ => reply.trim().to_string(),
    };
    let stripped = crate::chat::chats::strip_unsourced_citations(&reply, &numbers);
    let (mut out, _) = ask::cite_missing(&stripped, passages);
    if whole && ask::needs_uncited_note(&out, passages.len()) {
        out = format!("{}\n\n{UNCITED_NOTE}", out.trim_end());
    }
    let all = ask::sources(&Found { passages: passages.to_vec(), ..Default::default() });
    let cited = ask::cited_numbers(&out);
    let sources = all.into_iter().filter(|s| cited.contains(&s.n)).collect();
    (out, sources)
}

/// The last line of "Make notes longer"'s request (the on-device 4B, told
/// only in the system prompt, sometimes sent the notes back as they were).
const LONGER_ASK: &str = "Now write these notes out again, about twice as long: every heading and task line kept, and each point given what the meeting said about it. Never send them back unchanged.";

/// What "Make notes longer" sends, and what its reply is checked against.
pub struct Longer {
    /// The request: the notes as they are, then the meeting (with the
    /// meeting's language line when it isn't English).
    pub asked: String,
    /// The same without the language line: the deadline checks read it.
    pub source: String,
    /// The tasks the reply must keep: the summary's own and the digests'.
    pub tasks: Vec<crate::notes::DigestAction>,
}

/// The budgets "Make notes longer" reads the meeting within: the action
/// plan's, a little less (the summary goes with it, and the reply is
/// longer).
pub fn longer_budget(b: FinalBudget) -> FinalBudget {
    FinalBudget { single_pass: b.single_pass.saturating_sub(1_000).max(1_500), input: b.input.saturating_sub(1_000).max(2_000) }
}

/// **"Make notes longer"**: the meeting as the action plan reads it
/// (`compose_meeting_input_within`, within [`longer_budget`]), then — last,
/// where a small model takes up what it rewrites, rather than echoing the
/// meeting — the summary as it is (≤ 1,200 tokens), each in its own tags,
/// in the meeting's language. `None`: no summary yet.
pub fn longer_request(note: &Note, budget: FinalBudget, lang: &super::language::MeetingLanguage) -> Option<Longer> {
    let summary = note.enhanced_content.trim();
    if summary.is_empty() {
        return None;
    }
    let meeting = super::action_plan::compose_meeting_input_within(note, longer_budget(budget));
    let source = format!(
        "The meeting:\n<meeting>\n{}\n</meeting>\n\nThe notes to write out in more detail, every heading and task line kept:\n<notes>\n{}\n</notes>\n\n{LONGER_ASK}",
        meeting.text.trim(),
        truncate_to_tokens(summary, LONGER_SUMMARY_TOKENS)
    );
    let asked = super::language::with_line(&source, super::language::plan_line(lang).as_deref());
    let mut tasks: Vec<crate::notes::DigestAction> = crate::notes::tasks::parse(summary)
        .into_iter()
        .map(|t| crate::notes::DigestAction { owner: if t.owner.is_empty() { super::UNASSIGNED.to_string() } else { t.owner }, task: t.task, due: t.due })
        .collect();
    for d in meeting.digest_actions {
        if !tasks.iter().any(|t| t.task == d.task) {
            tasks.push(d);
        }
    }
    Some(Longer { asked, source, tasks })
}

/// Whether a summary is laid out as the action plan (its "## Action plan"
/// heading): then the longer one goes through the action plan's checks.
pub fn is_action_plan(summary: &str) -> bool {
    summary.lines().any(|l| {
        let t = l.trim().trim_start_matches('#').trim().to_lowercase();
        l.trim_start().starts_with("##") && (t == "action plan" || t == "next steps")
    })
}

/// A longer-notes reply as notes: code fences and a preamble before the
/// first heading or list taken off. `None`: nothing like notes came back.
pub fn clean_longer(reply: &str) -> Option<String> {
    // The notes in their tags, as they were given; an echo of the meeting
    // before them gone.
    let reply = match (reply.find("<notes>"), reply.rfind("</notes>")) {
        (Some(a), Some(b)) if b > a => &reply[a + "<notes>".len()..b],
        (Some(a), None) => &reply[a + "<notes>".len()..],
        (None, Some(b)) => &reply[..b],
        _ => reply,
    };
    let reply = reply.rfind("</meeting>").map_or(reply, |i| &reply[i + "</meeting>".len()..]);
    let mut lines: Vec<&str> = reply.lines().filter(|l| !l.trim_start().starts_with("```")).collect();
    // Notes have a heading or a list somewhere.
    lines.iter().position(|l| {
        let t = l.trim_start();
        t.starts_with('#') || t.starts_with("- ") || t.starts_with("* ")
    })?;
    // A line of chat before the notes ("Here are the longer notes:").
    while lines.first().is_some_and(|l| l.trim().is_empty()) {
        lines.remove(0);
    }
    if lines.len() > 1 && lines.first().is_some_and(|l| {
        let t = l.trim();
        t.ends_with(':') && !t.starts_with(['#', '-', '*']) && t.split_whitespace().count() <= 16
    }) {
        lines.remove(0);
    }
    let out = lines.join("\n").trim().to_string();
    (!out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::TranscriptSegment;

    fn seg(source: &str, text: &str, ts: u64) -> TranscriptSegment {
        TranscriptSegment { source: source.into(), text: text.into(), ts, ..Default::default() }
    }

    fn meeting(lines: &[(&str, &str)]) -> Note {
        let mut note: Note = serde_json::from_value(serde_json::json!({
            "id": 7, "title": "Supplier call", "content": "", "createdTs": 1_000, "updatedTs": 1_000, "noteType": "meeting",
            "participants": ["Priya Shah"],
        }))
        .unwrap();
        note.transcript = lines.iter().enumerate().map(|(i, (s, t))| seg(s, t, 1_000 + i as u64 * 20)).collect();
        note
    }

    const TALK: [(&str, &str); 6] = [
        ("you", "Thanks for joining, Priya. Today is about the supplier contract for the new packaging line."),
        ("them", "The quote came in at forty two thousand pounds, which is over the budget we agreed in March."),
        ("you", "Then we need to choose between cutting the second shift or asking finance for more."),
        ("them", "Finance meets on Thursday, so we could ask them then if we send the numbers by Wednesday."),
        ("you", "Who would put the numbers together? I can draft the request if someone checks the totals."),
        ("them", "I can check the totals tomorrow morning."),
    ];

    #[test]
    fn sound_smart_reads_the_latest_talk_as_numbered_passages() {
        let note = meeting(&TALK);
        let cited = smart_input(&note, Some("Last time: the budget was agreed in March [1].")).unwrap();
        assert!(!cited.passages.is_empty());
        assert_eq!(cited.passages[0].n, 1);
        assert!(cited.text.starts_with("Meeting: Supplier call\nAttendees: Priya Shah\n"), "{}", cited.text);
        assert!(cited.text.contains("The brief Yap prepared before the meeting (context only):\nLast time:"));
        assert!(cited.text.contains("<passage n=\"1\""));
        assert!(cited.text.contains("from=\"transcript\""));
        assert!(cited.text.contains("I can check the totals tomorrow morning."));
        // Each passage opens the meeting at its first line, and says how far in.
        let last = cited.passages.last().unwrap();
        assert_eq!(last.note, 7);
        assert!(last.at.is_some());
        assert!(estimate_tokens(&cited.text) < 2_200, "{}", estimate_tokens(&cited.text));
        // Too little said: no model call.
        assert!(smart_input(&meeting(&[("you", "Hi, can you hear me?")]), None).is_none());
    }

    #[test]
    fn sound_smart_keeps_to_the_latest_few_minutes_of_a_long_meeting() {
        let mut lines = Vec::new();
        for i in 0..300 {
            lines.push(if i % 2 == 0 { ("you", "We went through the packaging line numbers again and the second shift costs.") } else { ("them", "Right, and the supplier still wants an answer on the contract this month.") });
        }
        let note = meeting(&lines);
        let cited = smart_input(&note, None).unwrap();
        let talk_tokens: usize = cited.passages.iter().map(|p| estimate_tokens(&p.text) + 20).sum();
        assert!(talk_tokens <= SMART_TALK_TOKENS, "{talk_tokens}");
        // The latest line is in; the first isn't.
        assert!(cited.passages.last().unwrap().lines.contains(&299));
        assert!(!cited.passages.iter().any(|p| p.lines.contains(&0)));
    }

    #[test]
    fn a_skill_reads_the_summary_the_notes_and_the_talk_it_needs() {
        let mut note = meeting(&TALK);
        note.enhanced_content = "## Recap\nThe supplier quote is over budget.\n\n## Action plan\n### You\n- [ ] Draft the finance request (due: Wednesday)\n\n## Decisions\n- Ask finance on Thursday".into();
        note.content = "Ask about the second shift".into();
        let cited = skill_input(&note, "List every decision and who made it").unwrap();
        let fields: Vec<Field> = cited.passages.iter().map(|p| p.field).collect();
        assert_eq!(fields.first(), Some(&Field::Summary));
        assert!(fields.contains(&Field::Content) && fields.contains(&Field::Transcript));
        assert!(cited.text.contains("from=\"the summary\""));
        // Nothing in the meeting yet: nothing to run it on.
        let empty: Note = serde_json::from_value(serde_json::json!({"id": 1, "title": "", "content": "", "createdTs": 1, "updatedTs": 1})).unwrap();
        assert!(skill_input(&empty, "x").is_none());
    }

    #[test]
    fn a_long_transcript_gives_a_skill_the_parts_about_it_and_the_end() {
        let mut lines: Vec<(&str, &str)> = Vec::new();
        for i in 0..400 {
            lines.push(match i {
                120 => ("them", "The real risk is the supplier's warehouse fire last year: their insurance lapsed."),
                _ if i % 2 == 0 => ("you", "Moving on to the next slide about quarterly sales in the north region."),
                _ => ("them", "Sure, the north region numbers look similar to last quarter overall."),
            });
        }
        let note = meeting(&lines);
        let cited = skill_input(&note, "What risks did the supplier mention? Insurance, warehouse.").unwrap();
        let talk: Vec<&Passage> = cited.passages.iter().filter(|p| p.field == Field::Transcript).collect();
        assert!(talk.iter().any(|p| p.text.contains("insurance lapsed")), "the passage about it is in");
        assert!(talk.last().unwrap().lines.contains(&399), "the end is in");
        let tokens: usize = talk.iter().map(|p| estimate_tokens(&p.text) + 20).sum();
        assert!(tokens <= SKILL_TALK_TOKENS, "{tokens}");
        // In the order they were said, numbered from 1.
        let at: Vec<usize> = talk.iter().map(|p| p.line).collect();
        assert!(at.windows(2).all(|w| w[0] < w[1]));
        assert!(cited.passages.iter().enumerate().all(|(i, p)| p.n == i + 1));
    }

    #[test]
    fn an_answer_cites_only_the_passages_and_says_when_it_cites_none() {
        let note = meeting(&TALK);
        let cited = smart_input(&note, None).unwrap();
        let n = cited.passages.len();
        let reply = format!("- Ask Priya whether finance needs the totals before Thursday [{n}].\n- Offer to draft the request yourself [9].");
        let (out, sources) = checked(&reply, &cited.passages, true);
        assert!(out.contains(&format!("[{n}]")));
        assert!(!out.contains("[9]"), "a number that's none of the passages goes: {out}");
        assert_eq!(sources.iter().map(|s| s.n).collect::<Vec<_>>(), vec![n]);
        assert_eq!(sources[0].note, Some(7));
        // Nothing cited, something said: marked.
        let (out, sources) = checked("- Suggest moving the meeting to next quarter and doubling the budget for the packaging line.", &cited.passages, true);
        assert!(out.ends_with(UNCITED_NOTE), "{out}");
        assert!(sources.is_empty());
        // One passage: any number the model put on it is that one.
        let (out, sources) = checked("- Ask whether finance needs it by Thursday [3].", &cited.passages[..1], true);
        assert_eq!(out, "- Ask whether finance needs it by Thursday [1].");
        assert_eq!(sources.len(), 1);
        // Stopped half-written: no mark.
        assert!(!checked("- Something about the next quarter and", &cited.passages, false).0.contains(UNCITED_NOTE));
    }

    #[test]
    fn longer_notes_read_the_summary_then_the_meeting_and_keep_its_tasks() {
        let mut note = meeting(&TALK);
        assert!(longer_request(&note, FinalBudget::DEFAULT, &super::super::language::for_note(&note, &[])).is_none());
        note.enhanced_content = "## Recap\nThe quote is over budget.\n\n## Action plan\n### You\n- [ ] Draft the finance request (due: Wednesday)\n### Priya Shah\n- [x] Check the totals\n\n## Decisions\n- Ask finance on Thursday".into();
        let l = longer_request(&note, FinalBudget::DEFAULT, &super::super::language::for_note(&note, &[])).unwrap();
        // The meeting first, the notes to rewrite last, each in its tags.
        assert!(l.asked.starts_with("The meeting:\n<meeting>\nAttendees: Priya Shah"), "{}", l.asked);
        assert!(l.asked.ends_with(&format!("<notes>\n## Recap\nThe quote is over budget.\n\n## Action plan\n### You\n- [ ] Draft the finance request (due: Wednesday)\n### Priya Shah\n- [x] Check the totals\n\n## Decisions\n- Ask finance on Thursday\n</notes>\n\n{LONGER_ASK}")));
        assert!(l.asked.contains("I can check the totals tomorrow morning."));
        let tasks: Vec<(&str, &str, &str)> = l.tasks.iter().map(|t| (t.owner.as_str(), t.task.as_str(), t.due.as_str())).collect();
        assert_eq!(tasks, [("You", "Draft the finance request", "Wednesday"), ("Priya Shah", "Check the totals", "")]);
        assert!(is_action_plan(&note.enhanced_content));
        assert!(!is_action_plan("## Summary\n- Talked about the quote"));
        // Within an 8k model with the reply's room.
        assert!(estimate_tokens(&l.asked) + LONGER_REPLY_TOKENS as usize + 700 < 8_192);
    }

    #[test]
    fn a_longer_reply_loses_its_preamble_and_fences() {
        assert_eq!(clean_longer("Here are the longer notes:\n```markdown\n## Recap\nMore.\n```").as_deref(), Some("## Recap\nMore."));
        assert_eq!(clean_longer("## Recap\nMore.").as_deref(), Some("## Recap\nMore."));
        // Notes that open with a sentence keep it; a line of chat goes.
        assert_eq!(
            clean_longer("Sure! Here's a more detailed version:\n\nThe team met about the budget.\n\n## Action plan\n- [ ] x").as_deref(),
            Some("The team met about the budget.\n\n## Action plan\n- [ ] x")
        );
        assert_eq!(clean_longer("The team met about it: the budget.\n## Decisions\n- y").as_deref(), Some("The team met about it: the budget.\n## Decisions\n- y"));
        // The input echoed back: only the notes in their tags.
        assert_eq!(
            clean_longer("The meeting:\n<meeting>\nYou: hi\n</meeting>\n\nThe notes:\n<notes>\n## Recap\nLonger.\n</notes>").as_deref(),
            Some("## Recap\nLonger.")
        );
        assert_eq!(clean_longer("<meeting>\nYou: hi\n</meeting>\n## Recap\nLonger.").as_deref(), Some("## Recap\nLonger."));
        assert_eq!(clean_longer("I'm sorry, I can't do that."), None);
    }
}

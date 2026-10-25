//! **Ask across meetings**: the passages that answer a question — hybrid
//! search ([`rank`](super::rank)) over every meeting and note, within the
//! dates the question names ([`dates`](super::dates)) — and how they reach
//! the model (numbered `<passage>` blocks it must cite as `[1]`), the page
//! (each a source that opens its meeting at the line: `chats::Source` with
//! `note` and `line`), and an answer when no model is set up (the passages
//! themselves).
//!
//! The meaning half needs "Search my meetings by meaning" on and its model
//! downloaded; without it, or while it starts, the keyword half alone
//! answers (BM25 per passage: still better than the whole-note keyword
//! match Chat's tools use). Retrieval is always on this PC: a cloud model
//! sees only the passages chosen for its question.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use chrono::{DateTime, TimeZone};
use regex::Regex;

use super::chunks::{self, Field};
use super::dates::{self, Range};
use super::index::{self, Prepared};
use super::rank::{self, Candidate, Mode};
use super::store::Key;
use crate::chat::chats::Source;
use crate::notes::Note;

/// Passages given to the model with a question.
pub const ASK_PASSAGES: usize = 8;
/// How long a question waits for the meaning model (started if need be)
/// before the keyword half answers alone.
const QUERY_WAIT: Duration = Duration::from_secs(20);

/// The system prompt's line for a question asked across meetings (the same
/// every turn; the passages come with the question).
pub const ASK_INSTRUCTIONS: &str = "The user is asking about their meetings: the context of their message gives numbered passages from their meetings and notes that match the question, and their latest meetings. Answer only from those passages. After each statement, cite the passage it comes from by its number in square brackets, before the full stop: \"… by Friday [2].\" or \"[1, 3]\" (never \"Passage 2\"). Cite only the passages' numbers, and never make up a quote, a name, a date or a number. Say which meeting an answer comes from. If the passages don't answer the question, say so plainly.";

/// [`ASK_INSTRUCTIONS`] where the passages go: the question's context (a
/// model with tools) or, for one without, below in the system prompt.
pub fn instructions(in_message: bool) -> String {
    if in_message {
        ASK_INSTRUCTIONS.to_string()
    } else {
        ASK_INSTRUCTIONS.replace("the context of their message gives", "below are")
    }
}

/// What the search did, as the answer's chip: "Searched your meetings by
/// meaning · 6 passages from last week". Pure.
pub fn chip(found: &Found) -> String {
    let by = if found.semantic { " by meaning" } else { "" };
    let n = found.passages.len();
    let count = match n {
        0 => "nothing found".to_string(),
        1 => "1 passage".to_string(),
        n => format!("{n} passages"),
    };
    let when = match (&found.latest, &found.range) {
        (Some(t), _) => format!(" in \"{t}\""),
        (None, Some(r)) => format!(" from {}", r.label.split(" (").next().unwrap_or(&r.label)),
        _ => String::new(),
    };
    format!("Searched your meetings{by} · {count}{when}")
}

/// A passage chosen for a question.
#[derive(Debug, Clone, PartialEq)]
pub struct Passage {
    pub n: usize,
    pub note: u64,
    pub title: String,
    /// "meeting" | "note".
    pub kind: &'static str,
    pub field: Field,
    /// The line a citation opens at (a transcript line's index; a line
    /// number in the notes or summary)…
    pub line: usize,
    /// …and all of the passage's lines.
    pub lines: Vec<usize>,
    /// When that line was said (unix seconds; the note's time for notes).
    pub ts: u64,
    /// "Sun 25 Oct 2026, 10:12".
    pub when: String,
    /// How far into the meeting ("12:40"), for a transcript line.
    pub at: Option<String>,
    /// Who said that line ("" for notes).
    pub speaker: String,
    /// The passage, line by line ("Speaker: words").
    pub text: String,
    pub score: f32,
    pub keyword: f32,
    pub similarity: Option<f32>,
}

/// What a search found.
#[derive(Debug, Clone, Default)]
pub struct Found {
    pub passages: Vec<Passage>,
    /// The dates the question named.
    pub range: Option<Range>,
    /// "My last meeting": its title.
    pub latest: Option<String>,
    /// The meaning model took part.
    pub semantic: bool,
    /// The latest meetings, one line each (newest first).
    pub recent: Vec<String>,
}

fn when<Tz: TimeZone>(tz: &Tz, ts: u64) -> String
where
    Tz::Offset: std::fmt::Display,
{
    tz.timestamp_opt(ts as i64, 0).single().map(|t| t.format("%a %-d %b %Y, %H:%M").to_string()).unwrap_or_default()
}

/// "12:40", "1:02:03": `secs` into a meeting.
pub fn clock(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// **The search itself**, over `notes` (their passages `prepared`, in the
/// same order), at `now`: the dates and voices the question names, the
/// keyword list, the meaning list from `similarity` (a passage's cosine with
/// the question; `None` without one), merged as `mode` says. Pure.
#[allow(clippy::too_many_arguments)]
pub fn find_in<Tz: TimeZone>(
    notes: &[Note],
    prepared: &[Arc<Vec<Prepared>>],
    question: &str,
    now: &DateTime<Tz>,
    similarity: &dyn Fn(&Key) -> Option<f32>,
    mode: Mode,
    tuning: rank::Tuning,
    limit: usize,
) -> Found
where
    Tz::Offset: std::fmt::Display,
{
    let tz = now.timezone();
    let dated = dates::parse(question, now);
    let mut found = Found { range: dated.range.clone(), ..Default::default() };
    // The latest meetings (for "my last meeting" and the model's sense of
    // what's recent).
    let mut meetings: Vec<&Note> = notes
        .iter()
        .zip(prepared)
        .filter(|(n, p)| chunks::note_kind(n) == "meeting" && !p.is_empty())
        .map(|(n, _)| n)
        .collect();
    meetings.sort_by_key(|n| std::cmp::Reverse(chunks::note_time(n)));
    found.recent = meetings
        .iter()
        .take(3)
        .map(|n| {
            let with = if n.participants.is_empty() { String::new() } else { format!(" · with {}", n.participants.join(", ")) };
            format!("\"{}\" · {}{}", chunks::note_title(n), when(&tz, chunks::note_time(n)), with)
        })
        .collect();
    let only: Option<u64> = if dated.latest {
        let m = meetings.first();
        found.latest = m.map(|n| chunks::note_title(n));
        Some(m.map_or(u64::MAX, |n| n.id))
    } else {
        None
    };
    // The candidates: every passage within the question's dates.
    let mut cands: Vec<(&Note, &Prepared)> = Vec::new();
    for (n, ps) in notes.iter().zip(prepared) {
        if only.is_some_and(|id| id != n.id) {
            continue;
        }
        for p in ps.iter() {
            if dated.range.as_ref().is_none_or(|r| r.holds(p.chunk.ts())) {
                cands.push((n, p));
            }
        }
    }
    let sims: Vec<Option<f32>> = cands.iter().map(|(_, p)| similarity(&p.key)).collect();
    found.semantic = mode != Mode::Keyword && sims.iter().any(Option::is_some);
    let known: Vec<&str> = {
        let mut k: Vec<&str> = Vec::new();
        for (_, p) in &cands {
            for s in &p.speakers {
                if !k.contains(&s.as_str()) {
                    k.push(s);
                }
            }
        }
        k
    };
    let named = rank::named_speakers(question, &known);
    let subject = rank::subject(question, &named);
    let query = rank::terms(&dated.rest);
    let list: Vec<Candidate> = cands
        .iter()
        .zip(&sims)
        .map(|((n, p), s)| Candidate { note: n.id, terms: &p.terms, title_terms: &p.title_terms, speakers: &p.speakers, similarity: *s })
        .collect();
    let mut ranked = rank::rank(&list, &query, &named, mode, tuning, limit);
    // A question about a time whose words match nothing then ("what
    // happened yesterday?", "summarise my last meeting"): what's from then,
    // summaries first, then the notes, then the talk in order.
    let fill = if dated.latest { limit } else if dated.range.is_some() && ranked.is_empty() { limit.min(5) } else { 0 };
    if ranked.len() < fill {
        let mut rest: Vec<usize> = (0..cands.len()).filter(|i| !ranked.iter().any(|r| r.index == *i)).collect();
        let field_order = |f: Field| match f {
            Field::Summary => 0,
            Field::Content => 1,
            Field::Transcript => 2,
            Field::Chat | Field::Memory => 3,
        };
        rest.sort_by_key(|&i| {
            let (n, p) = cands[i];
            (field_order(p.chunk.field), std::cmp::Reverse(chunks::note_time(n)), p.chunk.lines.first().map_or(0, |l| l.index))
        });
        let mut per_note: HashMap<u64, usize> = HashMap::new();
        for r in &ranked {
            *per_note.entry(cands[r.index].0.id).or_default() += 1;
        }
        for i in rest {
            if ranked.len() >= fill {
                break;
            }
            let n = per_note.entry(cands[i].0.id).or_default();
            if !dated.latest && *n >= rank::PER_NOTE {
                continue;
            }
            *n += 1;
            ranked.push(rank::Ranked { index: i, score: 0.0, keyword: 0.0, similarity: sims[i], speaker: false });
        }
    }
    // For the line a citation opens at: the words' rarity over the candidates.
    let bm = rank::Bm25::new(&cands.iter().map(|(_, p)| p.terms.clone()).collect::<Vec<_>>());
    for (i, r) in ranked.iter().enumerate() {
        let (note, p) = cands[r.index];
        let lines: Vec<rank::LineView> = p
            .chunk
            .lines
            .iter()
            .zip(&p.line_terms)
            .map(|(l, t)| rank::LineView { terms: t, speaker: &l.speaker, asks: l.text.trim_end().ends_with('?') })
            .collect();
        let at = rank::best_line(&lines, &query, &named, subject.as_deref(), |w| bm.idf(w));
        let line = &p.chunk.lines[at];
        let start = note.transcript.iter().find(|s| s.is_talk()).map(|s| s.ts).unwrap_or(line.ts);
        found.passages.push(Passage {
            n: i + 1,
            note: note.id,
            title: p.chunk.title.clone(),
            kind: p.chunk.kind,
            field: p.chunk.field,
            line: line.index,
            lines: p.chunk.lines.iter().map(|l| l.index).collect(),
            ts: line.ts,
            when: when(&tz, line.ts),
            at: (p.chunk.field == Field::Transcript).then(|| clock(line.ts.saturating_sub(start))),
            speaker: line.speaker.clone(),
            text: p.chunk.text(),
            score: r.score,
            keyword: r.keyword,
            similarity: r.similarity,
        });
    }
    found
}

/// **Search** the person's meetings and notes for `question`: the meaning
/// model's half when it's on and installed (started if need be, up to
/// [`QUERY_WAIT`]), the keyword half always.
pub async fn find(question: &str, limit: usize) -> Found {
    find_waiting(question, limit, QUERY_WAIT).await
}

/// [`find`], the model awaited up to `wait` (Chat's `search_notes`: a tool
/// call's own, shorter).
pub async fn find_waiting(question: &str, limit: usize, wait: Duration) -> Found {
    let vector = super::embedder::query_vector(question, wait).await;
    let notes = crate::notes::all();
    let prepared = index::prepared(&notes);
    // Every passage's cosine with the question, under one look at the store.
    let sims: HashMap<Key, f32> = match &vector {
        Some(q) => index::with_store(|s| prepared.iter().flat_map(|v| v.iter()).filter_map(|p| s.similarity(&p.key, q).map(|x| (p.key, x))).collect()),
        None => HashMap::new(),
    };
    // The clock (a model evaluation's own, val::now).
    let now = crate::ai::eval::now().with_timezone(&chrono::Local);
    let found = find_in(&notes, &prepared, question, &now, &|k| sims.get(k).copied(), Mode::Hybrid, rank::TUNING, limit);
    tracing::info!(
        passages = found.passages.len(),
        semantic = found.semantic,
        dated = found.range.is_some(),
        latest = found.latest.is_some(),
        "Meeting search: a question searched"
    );
    found
}

fn attr(s: &str) -> String {
    s.replace('"', "'").replace(['\n', '\r'], " ")
}

/// The passages as the model reads them, with what the question's dates
/// narrowed them to and the latest meetings. Pure.
pub fn context(found: &Found) -> String {
    let mut out = String::new();
    if !found.recent.is_empty() {
        out.push_str("Their latest meetings, newest first:\n");
        for r in &found.recent {
            out.push_str("- ");
            out.push_str(r);
            out.push('\n');
        }
        out.push('\n');
    }
    if let Some(t) = &found.latest {
        out.push_str(&format!("The question is about their latest meeting, \"{}\": the passages are from it.\n\n", attr(t)));
    } else if let Some(r) = &found.range {
        out.push_str(&format!("The question is about {}: the passages are from then.\n\n", r.label));
    }
    if found.passages.is_empty() {
        out.push_str("No passage from their meetings or notes matches the question");
        if let Some(r) = &found.range {
            out.push_str(&format!(" ({})", r.label));
        }
        out.push('.');
        return out;
    }
    out.push_str("Passages from their meetings and notes that may answer it (cite them by number):\n");
    for p in &found.passages {
        let from = match p.field {
            Field::Transcript => "transcript",
            Field::Content => "their notes",
            Field::Summary => "the summary",
            Field::Chat => "a chat",
            Field::Memory => "memory",
        };
        let at = p.at.as_ref().map(|a| format!(" at=\"{a}\"")).unwrap_or_default();
        out.push_str(&format!(
            "\n<passage n=\"{}\" title=\"{}\" date=\"{}\"{at} from=\"{from}\">\n{}\n</passage>\n",
            p.n,
            attr(&p.title),
            p.when,
            p.text.trim()
        ));
    }
    out.trim_end().to_string()
}

/// A short date for a source's line: "Sun 25 Oct".
fn short_when(when: &str) -> String {
    // "Sun 25 Oct 2026, 10:12" → "Sun 25 Oct"
    when.split(',').next().unwrap_or(when).rsplit_once(' ').map_or(when.to_string(), |(d, _)| d.to_string())
}

/// Each passage as a source of the answer (Chat's Sources row and pills;
/// a click opens the note at its line). Pure.
pub fn sources(found: &Found) -> Vec<Source> {
    found
        .passages
        .iter()
        .map(|p| {
            let mut site = short_when(&p.when);
            if let Some(a) = &p.at {
                site.push_str(&format!(" · {a}"));
            }
            if !p.speaker.is_empty() {
                site.push_str(&format!(" · {}", p.speaker));
            } else if p.field == Field::Summary {
                site.push_str(" · summary");
            } else {
                site.push_str(" · notes");
            }
            let quote: String = p.text.chars().take(280).collect();
            Source {
                n: p.n,
                title: p.title.clone(),
                url: String::new(),
                site,
                note: Some(p.note),
                field: p.field.as_str().to_string(),
                line: Some(p.line),
                ts: p.ts,
                quote: if p.text.chars().count() > 280 { format!("{quote}…") } else { quote },
            }
        })
        .collect()
}

static CITE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[(\d{1,2}(?:\s*[,–-]\s*\d{1,2})*)\]").expect("static regex"));

/// The numbers an answer cites ("[1]", "[2, 3]", "[1–3]"), in order, once
/// each. Pure.
pub fn cited_numbers(reply: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for c in CITE.captures_iter(reply) {
        let inner = &c[1];
        let nums: Vec<usize> = inner.split([',', '–', '-']).filter_map(|n| n.trim().parse().ok()).collect();
        let nums: Vec<usize> = if inner.contains(['–', '-']) && nums.len() == 2 && nums[0] < nums[1] && nums[1] - nums[0] <= 10 {
            (nums[0]..=nums[1]).collect()
        } else {
            nums
        };
        for n in nums {
            if n > 0 && !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out
}

/// The sources an answer shows: those it cites; citing none, the first
/// three (what it had to go on). Pure.
pub fn shown(reply: &str, all: &[Source]) -> Vec<Source> {
    let cited = cited_numbers(reply);
    let mine: Vec<Source> = all.iter().filter(|s| cited.contains(&s.n)).cloned().collect();
    if mine.is_empty() {
        all.iter().take(3).cloned().collect()
    } else {
        mine
    }
}

static PASSAGE_REF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[Pp]assage\s+(\d{1,2})\b").expect("static regex"));
static QUOTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"["“]([^"“”]{12,})["”]"#).expect("static regex"));

/// Words with letters and digits only, lower case: for matching a quote.
fn plain(s: &str) -> String {
    s.to_lowercase()
        .replace('’', "'")
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// **Citations a model left out, put in where they're plain**: a small
/// model on this PC often names the meeting ("This comes from the Northwind
/// check-in") or writes "Passage 5" instead of "[5]" (Qwen3.5-4B left 11 of
/// 33 answers uncited, `yap.exe eval --search`). "Passage 5" becomes "[5]";
/// a statement with no citation gets the passage it quotes, or the one
/// holding most of its words — at least three of them and half, and clearly
/// more than any other passage — before its full stop. Anything less plain
/// stays uncited (and [`UNCITED_NOTE`] may say so). Code, headings and
/// "not mentioned" sentences are left alone. Returns the answer and how many
/// citations Yap added. Pure.
pub fn cite_missing(reply: &str, passages: &[Passage]) -> (String, usize) {
    if passages.is_empty() {
        return (reply.to_string(), 0);
    }
    let mut added = 0;
    let max = passages.len();
    // "Passage 5" → "[5]".
    let text = PASSAGE_REF
        .replace_all(reply, |c: &regex::Captures| match c[1].parse::<usize>() {
            Ok(n) if (1..=max).contains(&n) => {
                added += 1;
                format!("[{n}]")
            }
            _ => c[0].to_string(),
        })
        .into_owned();
    let words: Vec<std::collections::HashSet<String>> = passages
        .iter()
        .map(|p| rank::terms(&p.text).into_iter().chain(rank::terms(&p.title)).collect())
        .collect();
    let plains: Vec<String> = passages.iter().map(|p| plain(&p.text)).collect();
    let mut out = String::with_capacity(text.len() + 16);
    let mut fence = false;
    for line in text.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("```") {
            fence = !fence;
        }
        if fence || t.starts_with("```") || t.starts_with('#') || t.starts_with('*') && t.trim_end().ends_with('*') && !t.starts_with("* ") {
            out.push_str(line);
            continue;
        }
        // Each sentence of the line.
        let mut rest = line;
        while !rest.is_empty() {
            let end = rest
                .char_indices()
                .find(|&(i, c)| matches!(c, '.' | '!' | '?') && rest[i + c.len_utf8()..].starts_with([' ', '\n', '\r']) || i + c.len_utf8() == rest.len() && matches!(c, '.' | '!' | '?'))
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(rest.len());
            let (sentence, after) = rest.split_at(end);
            rest = after;
            let core = sentence.trim_end_matches(['\n', '\r']);
            // "…four thousand pounds. [1]": its citation comes after the stop.
            let cited_after = CITE.find(after.trim_start_matches([' ', '\t'])).is_some_and(|m| m.start() == 0);
            if CITE.is_match(core) || cited_after || NOT_FOUND.is_match(&core.replace('’', "'")) {
                out.push_str(sentence);
                continue;
            }
            // A quote from one passage.
            let pick: Option<usize> = QUOTED.captures_iter(core).find_map(|q| {
                let qp = plain(&q[1]);
                let hits: Vec<usize> = (0..max).filter(|&i| qp.split(' ').count() >= 4 && plains[i].contains(&qp)).collect();
                (hits.len() == 1).then(|| hits[0])
            });
            let mut picks: Vec<usize> = pick.into_iter().collect();
            if picks.is_empty() {
                let s: std::collections::HashSet<String> = rank::terms(core).into_iter().collect();
                if s.len() >= 3 {
                    let mut scores: Vec<(usize, usize)> = words.iter().enumerate().map(|(i, w)| (i, s.intersection(w).count())).collect();
                    scores.sort_by_key(|&(i, n)| (std::cmp::Reverse(n), i));
                    let (best, n) = scores[0];
                    let (other, second) = scores.get(1).copied().unwrap_or((best, 0));
                    if n >= 3 && n * 2 >= s.len() && n > second + 1 {
                        picks.push(best);
                    } else if n >= 3 && second * 5 >= s.len() * 4 {
                        // Two passages both holding nearly all of it (a line
                        // and the summary that repeats it): both.
                        picks.extend([best, other]);
                    }
                }
            }
            if picks.is_empty() {
                out.push_str(sentence);
            } else {
                let mut ns: Vec<usize> = picks.iter().map(|&i| passages[i].n).collect();
                ns.sort_unstable();
                let list = ns.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", ");
                let body = core.trim_end();
                let stop = body.ends_with(['.', '!', '?']) as usize;
                let (words_part, punct) = body.split_at(body.len() - stop);
                // "**£4,000**." → "**£4,000** [1]."
                out.push_str(&format!("{words_part} [{list}]{punct}"));
                out.push_str(&sentence[body.len()..]);
                added += 1;
            }
        }
    }
    (out, added)
}

static NOT_FOUND: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:\b(?:no|not|nothing|none|cannot|unable)\b|n't\b)[^.!?\n]{0,60}\b(?:mention|mentioned|found|find|cover|covers|covered|say|says|said|passages?|meetings?|notes?|information|record|discuss|discussed|decisions?|decided|talked|raised|came\s+up)\b")
        .expect("static regex")
});

/// Said under an answer that cites none of the passages it was given
/// (the honesty check for this answer: every claim cites a passage).
pub const UNCITED_NOTE: &str = "*Yap couldn't tie this answer to a passage from your meetings: check it against the passages below.*";

/// Does an answer need [`UNCITED_NOTE`]? It cites nothing, passages were
/// there, it says something (12 words, or a short one with a number or a
/// date in it: "The offsite is on December 4th to 6th."; not "You're
/// welcome!") and it's more than a "that isn't in your meetings". Pure.
pub fn needs_uncited_note(reply: &str, passages: usize) -> bool {
    if passages == 0 || !cited_numbers(reply).is_empty() {
        return false;
    }
    let words = reply.split_whitespace().count();
    let says = words >= 12 || (words >= 4 && reply.chars().any(|c| c.is_ascii_digit()));
    says && !says_not_found(reply)
}

/// Does an answer open by saying the meetings don't cover it (`No`,
/// `not mentioned`, `couldn't find`… in its first two sentences)? Pure.
pub fn says_not_found(reply: &str) -> bool {
    let first: String = reply.split_inclusive(['.', '!', '?', '\n']).take(2).collect();
    NOT_FOUND.is_match(&first.replace('’', "'"))
}

/// The answer when no AI model is set up: the passages themselves, each
/// cited so its pill opens it. Pure.
pub fn passages_answer(found: &Found) -> String {
    let scope = match (&found.latest, &found.range) {
        (Some(t), _) => format!(" in your latest meeting, \"{t}\""),
        (None, Some(r)) => format!(" from {}", r.label),
        _ => String::new(),
    };
    if found.passages.is_empty() {
        return format!(
            "Nothing in your meetings or notes{scope} matches that. (No AI model is set up, so Yap shows what it finds rather than writing an answer: set one up in Settings → Language Models.)"
        );
    }
    let mut out = format!("No AI model is set up, so here are the passages{scope} that match your question:\n");
    for p in &found.passages {
        let mut head = format!("**{}** · {}", p.title, short_when(&p.when));
        if let Some(a) = &p.at {
            head.push_str(&format!(" · {a}"));
        }
        let body: String = p.text.split('\n').collect::<Vec<_>>().join(" / ");
        let body: String = if body.chars().count() > 300 { format!("{}…", body.chars().take(300).collect::<String>()) } else { body };
        out.push_str(&format!("\n[{}] {head}  \n“{}”\n", p.n, body.trim()));
    }
    out.push_str("\nSet up an AI model in Settings → Language Models to get answers written from them.");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::chunks::tests::{line, note};
    use chrono::FixedOffset;
    use serde_json::json;

    fn now() -> DateTime<FixedOffset> {
        FixedOffset::east_opt(0).unwrap().with_ymd_and_hms(2026, 10, 25, 15, 0, 0).unwrap()
    }

    fn ts(m: u32, d: u32, h: u32, min: u32) -> u64 {
        FixedOffset::east_opt(0).unwrap().with_ymd_and_hms(2026, m, d, h, min, 0).unwrap().timestamp() as u64
    }

    /// Two meetings a month apart and a personal note.
    fn library() -> Vec<Note> {
        vec![
            note(json!({
                "id": 1,
                "noteType": "meeting",
                "title": "Q3 budget review",
                "participants": ["Priya Shah"],
                "createdTs": ts(9, 14, 10, 0),
                "transcript": [
                    { "source": "them", "speaker": "them-1", "text": "The venue quote came in at four thousand pounds, which is over what finance allowed.", "ts": ts(9, 14, 10, 0) },
                    { "source": "you", "text": "Can we move some of the catering money across?", "ts": ts(9, 14, 10, 1) },
                    { "source": "them", "speaker": "them-1", "text": "Only if Tom signs it off by Friday.", "ts": ts(9, 14, 10, 2) },
                ],
                "speakers": { "them-1": { "name": "Priya Shah", "source": "user" } },
            })),
            note(json!({
                "id": 2,
                "noteType": "meeting",
                "title": "Launch review",
                "createdTs": ts(10, 22, 9, 0),
                "transcript": [
                    line("them", "We agreed the launch moves to March third so the pricing page ships with it.", ts(10, 22, 9, 0)),
                    line("you", "I'll update the pricing copy by Friday.", ts(10, 22, 9, 5)),
                ],
                "enhancedContent": "## Decisions\n- The launch moves to March third",
            })),
            note(json!({ "id": 3, "title": "Garden jobs", "content": "Plant sixty tulip bulbs along the front path.", "createdTs": ts(10, 1, 8, 0) })),
        ]
    }

    fn search(q: &str, mode: Mode) -> Found {
        let notes = library();
        let prepared = crate::search::index::prepared(&notes);
        find_in(&notes, &prepared, q, &now(), &|_| None, mode, rank::TUNING, ASK_PASSAGES)
    }

    #[test]
    fn a_question_finds_its_line_with_who_and_when() {
        let f = search("How much was the venue quote?", Mode::Hybrid);
        let p = &f.passages[0];
        assert_eq!((p.note, p.field, p.line), (1, Field::Transcript, 0));
        assert_eq!(p.speaker, "Priya Shah");
        assert_eq!(p.when, "Mon 14 Sep 2026, 10:00");
        assert_eq!(p.at.as_deref(), Some("0:00"));
        assert!(!f.semantic, "no vectors: keywords only");
        // Its source opens the note at that line.
        let s = &sources(&f)[0];
        assert_eq!((s.note, s.line, s.field.as_str(), s.url.as_str()), (Some(1), Some(0), "transcript", ""));
        assert_eq!(s.site, "Mon 14 Sep · 0:00 · Priya Shah");
        assert!(s.quote.contains("four thousand pounds"));
    }

    #[test]
    fn a_named_voice_and_a_month_narrow_it() {
        // "last month" from 25 October: September, where Priya spoke.
        let f = search("What did Priya say about money last month?", Mode::Hybrid);
        assert!(f.range.as_ref().is_some_and(|r| r.label.starts_with("last month")));
        assert!(!f.passages.is_empty());
        assert!(f.passages.iter().all(|p| p.note == 1), "only September's meeting");
        // "this week": nothing from September.
        let f = search("the venue quote this week", Mode::Hybrid);
        assert!(f.passages.iter().all(|p| p.note != 1));
        // The latest meeting only.
        let f = search("what did we decide in the last meeting?", Mode::Hybrid);
        assert_eq!(f.latest.as_deref(), Some("Launch review"));
        assert!(!f.passages.is_empty() && f.passages.iter().all(|p| p.note == 2));
    }

    #[test]
    fn meaning_finds_what_keywords_miss() {
        let notes = library();
        let prepared = crate::search::index::prepared(&notes);
        // A stand-in meaning model: the garden note is the one about flowers.
        let garden: Vec<Key> = prepared[2].iter().map(|p| p.key).collect();
        let sim = move |k: &Key| Some(if garden.contains(k) { 0.7 } else { 0.25 });
        let q = "when should I put the spring flowers in the ground";
        let kw = find_in(&notes, &prepared, q, &now(), &sim, Mode::Keyword, rank::TUNING, 5);
        assert!(kw.passages.iter().all(|p| p.note != 3), "no shared word");
        let hy = find_in(&notes, &prepared, q, &now(), &sim, Mode::Hybrid, rank::TUNING, 5);
        assert_eq!(hy.passages[0].note, 3);
        assert!(hy.semantic);
        assert_eq!(hy.passages[0].field, Field::Content);
        assert_eq!(hy.passages[0].at, None);
    }

    #[test]
    fn the_model_reads_numbered_passages_and_the_latest_meetings() {
        let f = search("When does the launch happen?", Mode::Hybrid);
        let c = context(&f);
        assert!(c.starts_with("Their latest meetings, newest first:\n- \"Launch review\" · Thu 22 Oct 2026, 09:00\n"), "{c}");
        assert!(c.contains("<passage n=\"1\" title=\"Launch review\""), "{c}");
        assert!(c.contains("title=\"Launch review\" date=\"Thu 22 Oct 2026, 09:00\" at=\"0:00\" from=\"transcript\">\nThem: We agreed the launch moves to March third"), "{c}");
        assert!(c.contains("from=\"the summary\">\nDecisions\nThe launch moves to March third\n</passage>"), "{c}");
        // Nothing found within the dates: said so.
        let none = search("the tulips yesterday", Mode::Hybrid);
        assert!(context(&none).ends_with("No passage from their meetings or notes matches the question (yesterday (Sat 24 Oct 2026))."), "{}", context(&none));
    }

    #[test]
    fn an_answer_shows_the_passages_it_cites() {
        let f = search("When does the launch happen?", Mode::Hybrid);
        let all = sources(&f);
        assert!(all.len() >= 2);
        assert_eq!(cited_numbers("March third [2]. Copy by Friday [1, 2] and [1–3]."), [2, 1, 3]);
        let shown_ = shown("It moves to March third [2].", &all);
        assert_eq!(shown_.iter().map(|s| s.n).collect::<Vec<_>>(), [2]);
        // Citing none: the first three, to check it against.
        assert_eq!(shown("It moves to March.", &all).len(), all.len().min(3));
        // The honesty check: a real answer that cites nothing gets the note.
        assert!(needs_uncited_note("The launch moves to March third, so the pricing page ships with it then.", 3));
        assert!(!needs_uncited_note("The launch moves to March third [1], with the pricing page.", 3));
        assert!(!needs_uncited_note("Your meetings don't mention anything about the office move or its dates at all.", 3));
        assert!(!needs_uncited_note("Short answer.", 3));
        assert!(!needs_uncited_note("You're welcome! Anything else?", 3));
        assert!(needs_uncited_note("The offsite is scheduled for **December 4th to 6th**.", 3), "short, but a date");
        assert!(!needs_uncited_note("The launch moves to March third, so the pricing page ships with it then.", 0));
    }

    #[test]
    fn without_a_model_the_passages_are_the_answer() {
        let f = search("When does the launch happen?", Mode::Hybrid);
        let a = passages_answer(&f);
        assert!(a.starts_with("No AI model is set up, so here are the passages that match your question:\n\n[1] **Launch review** · Thu 22 Oct"), "{a}");
        assert!(a.contains("· Thu 22 Oct · 0:00  \n“Them: We agreed the launch moves to March third"), "{a}");
        assert_eq!(cited_numbers(&a).len(), f.passages.len(), "each one cited: a pill");
        let none = search("tulips yesterday", Mode::Hybrid);
        assert!(passages_answer(&none).starts_with("Nothing in your meetings or notes from yesterday"));
    }

    #[test]
    fn the_chip_says_what_was_searched() {
        let f = search("When does the launch happen?", Mode::Hybrid);
        assert_eq!(chip(&f), format!("Searched your meetings · {} passages", f.passages.len()));
        let f = search("What did Priya say about money last month?", Mode::Hybrid);
        assert!(chip(&f).ends_with(" from last month"), "{}", chip(&f));
        let f = search("summarise my last meeting", Mode::Hybrid);
        assert!(chip(&f).ends_with(" in \"Launch review\""), "{}", chip(&f));
        assert!(instructions(false).contains("below are numbered passages"));
        assert!(instructions(true).starts_with("The user is asking about their meetings"));
    }

    #[test]
    fn yap_puts_in_the_citations_a_small_model_left_out() {
        let f = search("When does the launch happen?", Mode::Hybrid);
        let transcript = f.passages.iter().find(|p| p.field == Field::Transcript).unwrap().n;
        // A statement plainly from one passage gets it; one naming only the
        // meeting doesn't.
        let (out, n) = cite_missing("The launch moves to March third so the pricing page ships with it.\n\nThis was in the Launch review.", &f.passages);
        assert_eq!(n, 1, "{out}");
        assert!(out.starts_with(&format!("The launch moves to March third so the pricing page ships with it [{transcript}].")), "{out}");
        assert!(out.ends_with("This was in the Launch review."));
        // "Passage 2" is a citation.
        let (out, n) = cite_missing("As Passage 1 says, it moves.", &f.passages);
        assert_eq!((out.as_str(), n), ("As [1] says, it moves.", 1));
        assert_eq!(cite_missing("Passage 9 says so.", &f.passages).0, "Passage 9 says so.", "no such passage");
        // A quote from one passage.
        let (out, _) = cite_missing("Someone said \"I'll update the pricing copy by Friday\" then.", &f.passages);
        assert!(out.contains(&format!("then [{transcript}].")), "{out}");
        // Cited already, "not mentioned", a heading, code: left alone.
        for s in [
            "The launch moves to March third [2].",
            "Your meetings don't mention the pricing page launch moving to March.",
            "## The launch moves to March third so the pricing page ships",
            "```\nThe launch moves to March third so the pricing page ships with it.\n```",
        ] {
            assert_eq!(cite_missing(s, &f.passages), (s.to_string(), 0), "{s}");
        }
        // Bold at the end keeps its marks, the citation before the stop.
        let (out, _) = cite_missing("It moves to **March third so the pricing page ships**.", &f.passages);
        assert!(out.ends_with(&format!("**March third so the pricing page ships** [{transcript}].")), "{out}");
        // gpt-oss cites after the full stop ("…pounds. [1]"): that counts.
        let s = "The launch moves to March third so the pricing page ships with it. [2]";
        assert_eq!(cite_missing(s, &f.passages), (s.to_string(), 0));
        // "We didn't make any decision about…" says the meetings don't have it.
        assert!(says_not_found("We didn’t make any decision about relocating the office itself. The only related thing was a desk."));
        assert!(!needs_uncited_note("We didn’t make any decision about relocating the office itself. The only related discussion was a desk by the window.", 3));
        // A line and the summary that repeats it: both cited.
        let summary = f.passages.iter().find(|p| p.field == Field::Summary).unwrap().n;
        let (out, n) = cite_missing("The launch moves to March third.", &f.passages);
        let mut both = [transcript, summary];
        both.sort_unstable();
        assert_eq!((out, n), (format!("The launch moves to March third [{}, {}].", both[0], both[1]), 1));
    }

    #[test]
    fn clocks() {
        assert_eq!(clock(0), "0:00");
        assert_eq!(clock(760), "12:40");
        assert_eq!(clock(3723), "1:02:03");
        assert_eq!(short_when("Sun 25 Oct 2026, 10:12"), "Sun 25 Oct");
    }
}

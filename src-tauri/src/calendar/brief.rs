//! **Pre-meeting briefs** (after Granola's "Briefs", May 2026; local): before
//! a calendar meeting, what to know going in, every line citing where it
//! comes from.
//!
//! - **Last time**: the latest meeting notes with these people (an attendee
//!   in common) or from the same recurring series, with their dates. Someone
//!   new is "First meeting with Siobhan Byrne"; nobody known at all, "First
//!   meeting with these people".
//! - **Open items**: unticked tasks in those notes' action plans (or their
//!   rolling digests) that belong to You or to someone coming.
//! - **From memory**: what Yap remembers (`memory.rs`) about the people
//!   coming, and the projects the invite names.
//! - **On the invite**: its agenda, as the calendar cleaned it.
//! - **In short** (with an AI model of ~4B and up, or a cloud one:
//!   [`writes_summary`]): two or three bullets the meeting's model writes
//!   from those numbered sources (`llm::MEETING_BRIEF_PROMPT`), each ending
//!   in its source's number — checked before they're shown
//!   ([`check_summary`]): a bullet with no citation, with a name, date,
//!   number or deadline its sources don't have, with mostly words they don't
//!   have, saying something's on the agenda the invite doesn't have, giving
//!   a task to someone other than its owner, telling an open task as done or
//!   an open question as settled, is dropped.
//!
//! Everything but "In short" is Yap's own, so no model can invent an owner
//! or a task there; without an AI model the brief is the same minus that
//! part. At most [`MAX_WORDS`] words in all. The evaluations that shaped the
//! checks: `docs/model-evals.md`, "Pre-meeting briefs".
//!
//! Pure functions over plain data (the meeting, the notes and memory are
//! passed in): unit-tested here, and run as they are by the model
//! evaluations (`eval/brief.rs`). When briefs are made, kept and shown is
//! `calendar.rs`.

use std::cmp::Reverse;
use std::collections::HashSet;

use chrono::TimeZone;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::memory::ops::{Item, Section as MemorySection};
use crate::notes::Note;

/// The whole brief stays under this many words (titles included).
pub const MAX_WORDS: usize = 150;
/// At most this many earlier meetings…
const MAX_HISTORY: usize = 3;
/// …open items…
const MAX_OPEN: usize = 4;
/// …things from memory…
const MAX_MEMORY: usize = 3;
/// …words of the invite's agenda…
const AGENDA_WORDS: usize = 30;
/// …and of the AI's "In short", in at most this many bullets.
const SUMMARY_WORDS: usize = 55;
const SUMMARY_BULLETS: usize = 3;
/// A bullet longer than this isn't "short": dropped.
const BULLET_WORDS: usize = 35;
/// An open item or a memory line is cut at this many words.
const LINE_WORDS: usize = 16;
/// Each earlier meeting the model reads, at most (an 8k local model reads
/// three of them, memory and the invite in ~2.5k with its prompt).
const NOTE_SOURCE_TOKENS: usize = 600;
/// The model's reply cap (local models; `ChatOptions::max_tokens`).
pub const REPLY_TOKENS: u32 = 220;
/// A meeting note with fewer words of speech than this, and nothing else
/// (no plan, digests or typed notes), is no "last time".
const MIN_WORDS: usize = 20;

/// What a brief is for: the meeting as the calendar has it.
#[derive(Debug, Clone, Default)]
pub struct Meeting {
    /// The calendar's key for this occurrence (`model::Event::key`).
    pub key: String,
    pub title: String,
    /// Unix seconds.
    pub start: i64,
    /// Everyone invited but you (names).
    pub attendees: Vec<String>,
    /// The invite's description, cleaned (`links::clean_description`).
    pub description: String,
}

/// Where a line comes from: an earlier meeting's note, something Yap
/// remembers, or the invite. Numbered from 1, as cited ("[1]").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub n: usize,
    /// "note" | "memory" | "invite".
    pub kind: String,
    /// The note's title, the person's or project's name, or "Invite".
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_id: Option<u64>,
    /// When that meeting was (unix seconds; notes only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<i64>,
}

/// One line of a brief. `cites` empty: Yap's own statement of an absence
/// ("First meeting with these people"), which has nothing to cite.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    pub text: String,
    /// An open item's owner ("You", "Priya"), or who a memory line is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub who: Option<String>,
    /// An open item's deadline, in the words said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<String>,
    #[serde(default)]
    pub cites: Vec<usize>,
}

impl Line {
    fn new(text: impl Into<String>, cites: Vec<usize>) -> Self {
        Line { text: text.into(), who: None, due: None, cites }
    }

    fn words(&self) -> usize {
        count_words(&self.text)
            + self.who.as_deref().map_or(0, count_words)
            + self.due.as_deref().map_or(0, count_words)
    }
}

/// A part of a brief: "summary" (In short), "last" (Last time), "open"
/// (Open items), "memory" (From memory), "agenda" (On the invite).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    pub kind: String,
    pub title: String,
    pub lines: Vec<Line>,
}

/// A meeting's brief, as the pages draw it and calendar.json keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Brief {
    /// The meeting's key.
    pub key: String,
    pub title: String,
    pub start: i64,
    /// No earlier notes with these people (or of this series).
    pub first: bool,
    /// "In short" was written by the AI.
    pub ai: bool,
    /// The AI's part is still being written (the rest is ready).
    #[serde(default)]
    pub writing: bool,
    /// Why there's no "In short": no AI model, the model stopped, a failure.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    pub parts: Vec<Part>,
    pub sources: Vec<Source>,
    /// Words in all (≤ [`MAX_WORDS`]).
    pub words: usize,
    /// When it was made (unix seconds).
    pub made_ts: i64,
    /// What it was made from: a brief whose inputs changed is made again.
    #[serde(default)]
    pub fingerprint: String,
}

impl Brief {
    /// The open items it lists (the Meetings view's and the notepad's count).
    pub fn open_count(&self) -> usize {
        self.parts.iter().filter(|p| p.kind == "open").map(|p| p.lines.len()).sum()
    }
}

// ---- names, words and times --------------------------------------------------------------

/// A name for comparing: lower case, letters and digits, single spaces.
fn norm(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c.to_lowercase().next().unwrap_or(c) } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn first_of(s: &str) -> &str {
    s.split(' ').next().unwrap_or(s)
}

/// Is `a` (a name as a note or an action plan wrote it) the attendee `b`?
/// Full names must agree; a lone first name ("Priya") stands for "Priya
/// Shah" only while no other attendee (`all`) is a Priya: "Priya Shah" is
/// never "Priya Patel".
pub fn same_person(a: &str, b: &str, all: &[String]) -> bool {
    let (na, nb) = (norm(a), norm(b));
    if na.is_empty() || nb.is_empty() {
        return false;
    }
    if na == nb {
        return true;
    }
    let (sa, sb) = (!na.contains(' '), !nb.contains(' '));
    if (sa || sb) && first_of(&na) == first_of(&nb) {
        let first = first_of(&na);
        return all.iter().filter(|x| first_of(&norm(x)) == first).count() <= 1;
    }
    false
}

/// The recurring series an occurrence belongs to, from its calendar key:
/// an iCal event's UID ("`<connection>:<uid>:<original start>`") or Google's
/// recurring event id ("`<connection>:<id>_20261005T140000Z`"), without the
/// connection, so a calendar connected again still matches. `None` for a
/// one-off meeting.
pub fn series_of(key: &str) -> Option<String> {
    let (_, rest) = key.split_once(':')?;
    if let Some((uid, rid)) = rest.rsplit_once(':') {
        if !uid.is_empty() && rid.parse::<i64>().is_ok_and(|t| t > 1_000_000_000) {
            return Some(uid.to_string());
        }
    }
    if let Some((base, when)) = rest.rsplit_once('_') {
        let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
        let stamp = match when.len() {
            8 => digits(when),
            16 => digits(&when[..8]) && &when[8..9] == "T" && digits(&when[9..15]) && when.ends_with('Z'),
            _ => false,
        };
        if stamp && !base.is_empty() {
            return Some(base.to_string());
        }
    }
    None
}

fn count_words(s: &str) -> usize {
    s.split_whitespace().filter(|w| w.chars().any(char::is_alphanumeric)).count()
}

/// The first `n` words of `s`, "…" when cut.
fn first_words(s: &str, n: usize) -> String {
    let words: Vec<&str> = s.split_whitespace().collect();
    if words.len() <= n {
        return words.join(" ");
    }
    format!("{}…", words[..n].join(" ").trim_end_matches([',', ';', ':', '.']))
}

/// The first `budget` estimated tokens of `s`, whole lines (else cut).
fn clip_tokens(s: &str, budget: usize) -> String {
    use crate::meetings::meeting_summary::estimate_tokens;
    if estimate_tokens(s) <= budget {
        return s.trim().to_string();
    }
    let mut out = String::new();
    for line in s.lines() {
        if estimate_tokens(&out) + estimate_tokens(line) + 1 > budget {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    if out.trim().is_empty() {
        out = s.chars().take(budget * 3).collect();
    }
    format!("{}\n(…)", out.trim_end())
}

/// When a meeting note's meeting was: its event's start, else its first
/// line, else when the note was made.
fn meeting_time(n: &Note) -> i64 {
    n.event
        .as_ref()
        .map(|e| e.start)
        .or_else(|| n.transcript.first().map(|s| s.ts))
        .unwrap_or(n.created_ts) as i64
}

/// "Thu 16 Oct" (this PC's time).
fn day(secs: i64) -> String {
    chrono::Local.timestamp_opt(secs, 0).single().map(|t| t.format("%a %-d %b").to_string()).unwrap_or_default()
}

/// "Thu 23 Oct, 10:00".
fn day_time(secs: i64) -> String {
    chrono::Local.timestamp_opt(secs, 0).single().map(|t| t.format("%a %-d %b, %H:%M").to_string()).unwrap_or_default()
}

// ---- what a brief is made from -------------------------------------------------------------

/// An open task in an action plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub owner: String,
    pub task: String,
    pub due: String,
}

/// "- [ ] …" (not ticked).
fn open_task(line: &str) -> Option<&str> {
    let t = line.trim_start();
    ["- [ ] ", "* [ ] ", "+ [ ] "].iter().find_map(|p| t.strip_prefix(p)).map(str::trim)
}

/// "Priya: book the venue" → ("Priya", "book the venue"), for an owner of up
/// to three words written like a name.
fn inline_owner(text: &str) -> Option<(&str, &str)> {
    let (who, rest) = text.split_once(':')?;
    let who = who.trim().trim_matches('*').trim();
    let words: Vec<&str> = who.split_whitespace().collect();
    let name_like = !words.is_empty()
        && words.len() <= 3
        && words.iter().all(|w| w.chars().next().is_some_and(char::is_uppercase) && !w.chars().any(|c| c.is_ascii_digit()));
    (name_like && !rest.trim().is_empty()).then_some((who, rest.trim()))
}

/// "(due: Friday)" off the end of a task, and the deadline.
fn split_due(task: &str) -> (String, String) {
    let lower = task.to_lowercase();
    if let Some(at) = lower.rfind("(due:") {
        let due = task[at + 5..].trim().trim_end_matches(')').trim().to_string();
        return (task[..at].trim().trim_end_matches(['.', ',']).to_string(), due);
    }
    (task.trim().trim_end_matches('.').to_string(), String::new())
}

fn owner_word(owner: &str) -> Option<String> {
    let o = owner.trim().trim_matches('*').trim();
    match o.to_lowercase().as_str() {
        "" | "unassigned" | "everyone" | "all" | "team" | "nobody" | "tbd" | "them" => None,
        "you" | "me" | "i" => Some("You".to_string()),
        _ => Some(o.to_string()),
    }
}

/// The unticked tasks with an owner in note `n`'s action plan — "## Action
/// plan" with a "### Name" heading per person (`llm::ACTION_PLAN_DEFAULT_
/// FRAGMENT`), or "- [ ] Name: task" lines (the Meeting Notes action) — or,
/// with no plan, in its rolling digests. Unassigned ones aren't anyone's.
pub fn plan_tasks(n: &Note) -> Vec<Task> {
    let plan = n.enhanced_content.trim();
    if plan.is_empty() {
        return crate::meetings::meeting_summary::digest_actions(&n.digests)
            .into_iter()
            .filter_map(|a| owner_word(&a.owner).map(|owner| Task { owner, task: a.task.trim().to_string(), due: a.due.trim().to_string() }))
            .filter(|t| !t.task.is_empty())
            .collect();
    }
    let mut out = Vec::new();
    let (mut in_tasks, mut unassigned, mut owner): (bool, bool, Option<String>) = (true, false, None);
    for line in plan.lines() {
        let t = line.trim();
        if let Some(title) = t.strip_prefix("## ") {
            let lower = title.to_lowercase();
            unassigned = lower.contains("unassigned");
            in_tasks = !unassigned && !lower.contains("decision") && !lower.contains("question");
            owner = None;
            continue;
        }
        if let Some(name) = t.strip_prefix("### ") {
            owner = if in_tasks { owner_word(name) } else { None };
            continue;
        }
        let Some(text) = open_task(t) else { continue };
        if unassigned {
            continue;
        }
        let (who, text) = match inline_owner(text) {
            Some((who, rest)) => (owner_word(who), rest),
            None => (owner.clone(), text),
        };
        let (task, due) = split_due(text);
        if let Some(owner) = who.filter(|_| !task.is_empty()) {
            out.push(Task { owner, task, due });
        }
    }
    out
}

/// The open questions a meeting left: its plan's "## Open questions", or its
/// digests' (what the checks keep a bullet from settling).
pub fn open_questions(n: &Note) -> Vec<String> {
    let plan = n.enhanced_content.trim();
    if plan.is_empty() {
        return n.digests.iter().flat_map(|d| d.questions.iter().cloned()).collect();
    }
    let mut out = Vec::new();
    let mut inside = false;
    for line in plan.lines() {
        let t = line.trim();
        if let Some(title) = t.strip_prefix("## ") {
            inside = title.to_lowercase().contains("question");
        } else if inside {
            let item = t.trim_start_matches(['-', '*', '\u{2022}']).trim();
            if !item.is_empty() && !item.eq_ignore_ascii_case("none") {
                out.push(item.to_string());
            }
        }
    }
    out
}

/// What people said they'd do in a meeting with no plan or digests, from its
/// transcript: "I'll send both quotes by Thursday" → (Rochelle, "send both
/// quotes by Thursday"). Only where the speaker is known: You, or the one
/// other person in a 1:1. For the checks, never shown.
fn spoken_tasks(n: &Note) -> Vec<Task> {
    let other = match n.participants.as_slice() {
        [one] => Some(one.clone()),
        _ => None,
    };
    let mut out = Vec::new();
    for seg in n.transcript.iter().filter(|s| s.is_talk()) {
        let owner = match seg.source.as_str() {
            "you" => Some("You".to_string()),
            _ => other.clone(),
        };
        let Some(owner) = owner else { continue };
        let lower = seg.text.to_lowercase();
        for marker in ["i'll ", "i will ", "i\u{2019}ll ", "i'm going to ", "i am going to "] {
            if let Some(at) = lower.find(marker) {
                let rest = &lower[at + marker.len()..];
                let end = rest.find(['.', ';', '!', '?']).unwrap_or(rest.len());
                let task = rest[..end].trim();
                if count_words(task) >= 2 {
                    out.push(Task { owner: owner.clone(), task: task.to_string(), due: String::new() });
                }
            }
        }
    }
    out
}

/// Content words of a task, to tell two tasks apart.
fn task_words(s: &str) -> HashSet<String> {
    words(s).into_iter().filter(|w| w.len() > 2 && !STOP.contains(&w.as_str())).map(|w| stem(&w)).collect()
}

/// Is `task` (nearly) one of `seen`?
fn same_task(task: &str, seen: &[HashSet<String>]) -> bool {
    let want = task_words(task);
    !want.is_empty() && seen.iter().any(|s| want.iter().filter(|w| s.contains(*w)).count() * 10 >= want.len() * 7)
}

/// A meeting note's own words for the model and the checks: its action plan,
/// else its digests, else what was typed and said.
fn note_body(n: &Note) -> String {
    let plan = n.enhanced_content.trim();
    if !plan.is_empty() {
        return clip_tokens(plan, NOTE_SOURCE_TOKENS);
    }
    let mut out = String::new();
    if !n.content.trim().is_empty() {
        out.push_str(&format!("Notes the user typed:\n{}\n", clip_tokens(n.content.trim(), NOTE_SOURCE_TOKENS / 3)));
    }
    let t0 = n.transcript.first().map(|s| s.ts).unwrap_or(0);
    if !n.digests.is_empty() {
        out.push_str(&crate::meetings::meeting_summary::render_digests(&n.digests, t0, NOTE_SOURCE_TOKENS));
    } else if n.transcript.iter().any(|s| s.is_talk()) {
        out.push_str(&crate::meetings::meeting_summary::ask_context(n, NOTE_SOURCE_TOKENS / 2));
    }
    clip_tokens(&out, NOTE_SOURCE_TOKENS)
}

fn has_substance(n: &Note) -> bool {
    !n.enhanced_content.trim().is_empty()
        || !n.digests.is_empty()
        || !n.content.trim().is_empty()
        || crate::notes::speech_words(&n.transcript) >= MIN_WORDS
}

/// An earlier meeting worth a line.
struct Past<'a> {
    note: &'a Note,
    when: i64,
    /// Attendees of this meeting who were in it.
    shared: Vec<String>,
}

/// The earlier meeting notes for `m`, newest first: tied to another
/// occurrence of its series, or with someone coming in them.
fn history<'a>(m: &Meeting, notes: &'a [Note], now: i64) -> Vec<Past<'a>> {
    let series = series_of(&m.key);
    let mut out: Vec<Past> = notes
        .iter()
        .filter(|n| n.note_type == "meeting" || n.event.is_some())
        .filter(|n| n.event.as_ref().is_none_or(|e| e.key != m.key))
        .filter(|n| has_substance(n))
        .filter_map(|n| {
            let when = meeting_time(n);
            if when >= m.start || when > now {
                return None;
            }
            let same_series = series.is_some() && n.event.as_ref().and_then(|e| series_of(&e.key)) == series;
            let shared: Vec<String> = m
                .attendees
                .iter()
                .filter(|a| n.participants.iter().any(|p| same_person(p, a, &m.attendees)))
                .cloned()
                .collect();
            (same_series || !shared.is_empty()).then_some(Past { note: n, when, shared })
        })
        .collect();
    out.sort_by_key(|p| Reverse(p.when));
    out.truncate(MAX_HISTORY);
    out
}

/// Everything a brief says, gathered: its sources, numbered, with the text
/// the model reads of each, and the lines Yap writes itself.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub meeting: Meeting,
    pub sources: Vec<Source>,
    /// What the model reads of each source (and what its bullets are checked
    /// against), by `n - 1`.
    pub texts: Vec<String>,
    pub last: Vec<Line>,
    pub open: Vec<Line>,
    pub memory: Vec<Line>,
    pub agenda: Vec<Line>,
    /// No earlier notes at all.
    pub first: bool,
    /// Every open task in the earlier meetings, whoever's, by source — from
    /// their plans, or what was said in one without — what the checks hold
    /// the model's bullets to: owners, and still to do…
    tasks: Vec<(usize, Task)>,
    /// …and the questions they left open (not to be told as settled).
    questions: Vec<(usize, String)>,
    /// What the fingerprint covers besides the meeting (ids, edit times).
    stamp: Vec<String>,
}

/// Gather `m`'s brief from the notes and memory (`now` = unix seconds).
pub fn gather(m: &Meeting, notes: &[Note], memory: &[Item], now: i64) -> Facts {
    let mut f = Facts { meeting: m.clone(), ..Default::default() };
    let past = history(m, notes, now);
    f.first = past.is_empty();

    // The earlier meetings, newest first: one source each.
    for p in &past {
        let n = f.sources.len() + 1;
        let title = if p.note.title.trim().is_empty() { "Untitled meeting".to_string() } else { p.note.title.trim().to_string() };
        let with = if p.note.participants.is_empty() { String::new() } else { format!(", with {}", p.note.participants.join(", ")) };
        f.texts.push(format!("Notes from \"{title}\", {}{with}:\n{}", day(p.when), note_body(p.note)));
        f.sources.push(Source { n, kind: "note".into(), title: title.clone(), note_id: Some(p.note.id), memory_id: None, date: Some(p.when) });
        f.last.push(Line::new(title, vec![n]));
        f.stamp.push(format!("note:{}:{}", p.note.id, p.note.updated_ts));
    }
    // Someone coming who wasn't in any of them.
    if !f.first {
        let new: Vec<&String> = m
            .attendees
            .iter()
            .filter(|a| !past.iter().any(|p| p.shared.iter().any(|s| s == *a)))
            .take(3)
            .collect();
        if !new.is_empty() {
            let names: Vec<&str> = new.iter().map(|s| s.as_str()).collect();
            let list = match names.as_slice() {
                [one] => one.to_string(),
                [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
                [] => String::new(),
            };
            f.last.push(Line::new(format!("First meeting with {list}"), vec![]));
        }
    } else {
        f.last.push(Line::new(
            if m.attendees.is_empty() { "No earlier notes for this meeting" } else { "First meeting with these people" },
            vec![],
        ));
    }

    // Open items: You's and the attendees', newest meeting first, once each.
    let mut seen: Vec<HashSet<String>> = Vec::new();
    for (i, p) in past.iter().enumerate() {
        for t in plan_tasks(p.note) {
            let theirs = t.owner == "You" || m.attendees.iter().any(|a| same_person(&t.owner, a, &m.attendees));
            if theirs && f.open.len() < MAX_OPEN && !same_task(&t.task, &seen) {
                seen.push(task_words(&t.task));
                let mut line = Line::new(capitalise(&first_words(&t.task, LINE_WORDS)), vec![i + 1]);
                line.who = Some(t.owner.clone());
                line.due = (!t.due.is_empty()).then(|| first_words(&t.due, 6));
                f.open.push(line);
            }
            f.tasks.push((i + 1, t));
        }
        if p.note.enhanced_content.trim().is_empty() && p.note.digests.is_empty() {
            f.tasks.extend(spoken_tasks(p.note).into_iter().map(|t| (i + 1, t)));
        }
        f.questions.extend(open_questions(p.note).into_iter().map(|q| (i + 1, q)));
    }

    // Memory: the people coming, then projects the invite names.
    let invite_words: HashSet<String> = words(&format!("{} {}", m.title, m.description)).into_iter().collect();
    let mut picked: Vec<&Item> = memory
        .iter()
        .filter(|it| it.section == MemorySection::People)
        .filter(|it| m.attendees.iter().any(|a| same_person(&it.title, a, &m.attendees)))
        .collect();
    picked.extend(memory.iter().filter(|it| it.section == MemorySection::Projects).filter(|it| {
        let name = words(&it.title);
        name.iter().any(|w| w.len() >= 3) && name.iter().all(|w| invite_words.contains(w))
    }));
    for it in picked.into_iter().take(MAX_MEMORY) {
        let mut said: Vec<String> = Vec::new();
        if !it.summary.trim().is_empty() {
            said.push(it.summary.trim().trim_end_matches('.').to_string());
        }
        said.extend(it.details.iter().take(2).map(|d| d.trim().trim_end_matches('.').to_string()));
        if said.is_empty() {
            continue;
        }
        let n = f.sources.len() + 1;
        f.texts.push(format!("What Yap remembers about {}: {}.", it.title, said.join(". ")));
        f.sources.push(Source { n, kind: "memory".into(), title: it.title.clone(), note_id: None, memory_id: Some(it.id), date: None });
        let mut line = Line::new(first_words(&said.join("; "), LINE_WORDS - count_words(&it.title).min(4)), vec![n]);
        line.who = Some(it.title.clone());
        f.memory.push(line);
        f.stamp.push(format!("memory:{}:{}", it.id, it.updated_ts));
    }

    // The invite's agenda.
    let agenda: Vec<&str> = m.description.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if !agenda.is_empty() {
        let n = f.sources.len() + 1;
        f.texts.push(format!("The invite: {}", clip_tokens(&agenda.join("\n"), 200)));
        f.sources.push(Source { n, kind: "invite".into(), title: "Invite".into(), note_id: None, memory_id: None, date: None });
        let mut left = AGENDA_WORDS;
        for l in agenda {
            if left == 0 {
                break;
            }
            let text = first_words(l, left);
            left = left.saturating_sub(count_words(&text));
            f.agenda.push(Line::new(text, vec![n]));
        }
    }
    f
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(first) => first.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

impl Facts {
    /// Whether the model has anything to write from: an earlier meeting or
    /// something remembered (an agenda alone is shown as it is).
    pub fn worth_a_call(&self) -> bool {
        self.sources.iter().any(|s| s.kind != "invite")
    }

    /// The inputs' fingerprint, with `ai` (the model the summary would come
    /// from, or "plain"): a change in any of them makes the brief again.
    pub fn fingerprint(&self, ai: &str) -> String {
        use sha2::{Digest, Sha256};
        let m = &self.meeting;
        let mut h = Sha256::new();
        for part in [m.key.as_str(), m.title.as_str(), &m.start.to_string(), &m.attendees.join("|"), m.description.as_str(), ai] {
            h.update(part.as_bytes());
            h.update([0]);
        }
        for s in &self.stamp {
            h.update(s.as_bytes());
            h.update([0]);
        }
        h.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect()
    }

    /// What the model reads: the meeting, then each source by its number.
    pub fn model_input(&self) -> String {
        let m = &self.meeting;
        let who = if m.attendees.is_empty() { "(not given)".to_string() } else { m.attendees.join(", ") };
        let mut out = format!("Meeting: {} ({})\nWho's coming: {who}\n", m.title.trim(), day_time(m.start));
        for (i, text) in self.texts.iter().enumerate() {
            out.push_str(&format!("\n[{}] {}\n", i + 1, text.trim()));
        }
        // Yap lists these itself (Open items): the bullets can say something else.
        if !self.open.is_empty() {
            let listed: Vec<String> = self
                .open
                .iter()
                .map(|l| format!("{}: {}", l.who.as_deref().unwrap_or(""), l.text.trim_end_matches('…')))
                .collect();
            out.push_str(&format!("\nYap lists these open tasks under your bullets, so don't repeat them: {}.\n", listed.join("; ")));
        }
        out
    }
}

// ---- the AI's part ----------------------------------------------------------------------------

const EXAMPLE_IN: &str = "Meeting: Harbour lighting check-in (Tue 4 Mar, 15:00)\nWho's coming: Okonkwo Adeyemi\n\n[1] Notes from \"Harbour lighting check-in\", Tue 25 Feb, with Okonkwo Adeyemi:\nThe team reviewed the first lighting quote, which came in over budget.\n## Action plan\n### You\n- [ ] Ask facilities for the site plan\n### Okonkwo\n- [ ] Get a second lighting quote (due: next Monday)\n## Decisions\n- Keep the spring opening date.\n## Open questions\n- Does the quay need evening lighting?\n\n[2] The invite: Agenda: lighting quotes, opening date\n\nYap lists these open tasks under your bullets, so don't repeat them: You: Ask facilities for the site plan; Okonkwo: Get a second lighting quote.\n";
const EXAMPLE_OUT: &str = "- Last time you kept the spring opening date, and the first lighting quote came in over budget. [1]\n- Whether the quay needs evening lighting is still open, and lighting quotes are on the agenda. [1, 2]";

/// The model's messages for `facts`: the immutable prompt, one worked
/// example (people and places no real brief has, so a copied line fails the
/// checks), and the sources. `None`: nothing for a model to add.
pub fn messages(facts: &Facts) -> Option<Value> {
    facts.worth_a_call().then(|| {
        json!([
            { "role": "system", "content": crate::ai::llm::MEETING_BRIEF_PROMPT },
            { "role": "user", "content": EXAMPLE_IN },
            { "role": "assistant", "content": EXAMPLE_OUT },
            { "role": "user", "content": facts.model_input() },
        ])
    })
}

/// Words that may open a bullet without being names.
const STARTERS: &[&str] = &[
    "a", "after", "agenda", "agreed", "all", "also", "and", "any", "are", "as", "ask", "at", "before", "both", "bring", "but",
    "by", "check", "confirm", "consider", "decided", "did", "discuss", "do", "does", "each", "earlier", "every", "everyone",
    "expect", "find", "first", "follow", "for", "from", "get", "had", "has", "have", "he", "how", "i", "if", "in", "is", "it",
    "its", "keep", "last", "look", "make", "mention", "new", "next", "no", "not", "note", "nothing", "of", "on", "once", "one",
    "open", "or", "outstanding", "pending", "pick", "plan", "prepare", "previously", "push", "raise", "remember", "remind",
    "review", "revisit", "see", "send", "share", "she", "since", "some", "still", "that", "the", "their", "there", "these",
    "they", "this", "those", "three", "to", "two", "update", "was", "watch", "we", "were", "what", "when", "who", "why",
    "with", "yap", "you", "your", "yours",
];

/// Little words left out of the overlap check.
const STOP: &[&str] = &[
    "about", "after", "again", "also", "and", "are", "back", "been", "before", "both", "but", "can", "could", "did", "does",
    "for", "from", "had", "has", "have", "into", "its", "last", "meeting", "more", "need", "needs", "next", "not", "now",
    "one", "only", "other", "our", "out", "over", "said", "same", "should", "some", "still", "than", "that", "the", "their",
    "them", "then", "there", "these", "they", "this", "time", "was", "were", "what", "when", "which", "while", "who", "will",
    "with", "would", "you", "your",
];

const DAYS: [(&str, &str); 7] = [
    ("monday", "mon"),
    ("tuesday", "tue"),
    ("wednesday", "wed"),
    ("thursday", "thu"),
    ("friday", "fri"),
    ("saturday", "sat"),
    ("sunday", "sun"),
];
const MONTHS: [(&str, &str); 12] = [
    ("january", "jan"),
    ("february", "feb"),
    ("march", "mar"),
    ("april", "apr"),
    ("may", "may"),
    ("june", "jun"),
    ("july", "jul"),
    ("august", "aug"),
    ("september", "sep"),
    ("october", "oct"),
    ("november", "nov"),
    ("december", "dec"),
];
/// Relative days a bullet may only use when its sources say them.
const RELATIVE: [&str; 4] = ["today", "tomorrow", "tonight", "yesterday"];

/// A word's form for the checks: lower case, its apostrophe-s and
/// punctuation off, a day or month's short name as the full one when it's
/// written with a capital ("Thu" → "thursday", but "sat" and "wed" stay
/// words), digits without their separators or ordinal ("3rd" → "3",
/// "2,000" → "2000").
fn canon(word: &str) -> String {
    let bare = word.trim_matches(|c: char| !c.is_alphanumeric());
    let capital = bare.chars().next().is_some_and(char::is_uppercase);
    let w = bare.to_lowercase();
    let w = w.strip_suffix("'s").or_else(|| w.strip_suffix("\u{2019}s")).unwrap_or(&w).to_string();
    if w.chars().any(|c| c.is_ascii_digit()) {
        let digits: String = w.chars().filter(|c| c.is_alphanumeric()).collect();
        for suffix in ["st", "nd", "rd", "th"] {
            if let Some(n) = digits.strip_suffix(suffix) {
                if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) {
                    return n.to_string();
                }
            }
        }
        return digits;
    }
    if capital {
        for (full, short) in DAYS.iter().chain(MONTHS.iter()) {
            if w == *short || (w.len() > 3 && full.starts_with(&w) && w.starts_with(short)) {
                return full.to_string();
            }
        }
    }
    w
}

/// The words of `s`, as [`canon`] writes them.
fn words(s: &str) -> Vec<String> {
    s.split(|c: char| c.is_whitespace() || matches!(c, '/' | '(' | ')' | '[' | ']' | '"' | '\u{201c}' | '\u{201d}' | '-' | '\u{2013}' | '\u{2014}'))
        .map(canon)
        .filter(|w| !w.is_empty())
        .collect()
}

/// A crude stem, so "agreed" meets "agree" and "quotes" "quote".
fn stem(w: &str) -> String {
    for suffix in ["ing", "ed", "es", "s"] {
        if w.len() > suffix.len() + 3 {
            if let Some(s) = w.strip_suffix(suffix) {
                return s.to_string();
            }
        }
    }
    w.to_string()
}

/// "[1]" / "[1, 3]" citations in a line: the numbers, and the line without them.
fn citations(line: &str) -> (Vec<usize>, String) {
    let mut cites = Vec::new();
    let mut text = String::new();
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find(']').map(|c| open + c) else { break };
        let inner = &rest[open + 1..close];
        let nums: Option<Vec<usize>> = inner
            .split([',', ';'])
            .map(|p| p.trim().trim_start_matches('#').trim().parse::<usize>().ok())
            .collect();
        text.push_str(&rest[..open]);
        match nums.filter(|n| !n.is_empty()) {
            Some(n) => cites.extend(n),
            None => text.push_str(&rest[open..=close]),
        }
        rest = &rest[close + 1..];
    }
    text.push_str(rest);
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = text.replace(" .", ".").replace(" ,", ",");
    cites.sort_unstable();
    cites.dedup();
    (cites, text.trim().to_string())
}

/// What the checks made of the model's reply.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Checked {
    /// The bullets kept, for "In short".
    #[serde(skip)]
    pub lines: Vec<Line>,
    /// Lines the model wrote that say something (≥ 3 words, not a heading).
    pub claims: usize,
    /// …with no valid citation.
    pub uncited: usize,
    /// …with a name, date, number or most of their words not in their sources.
    pub ungrounded: usize,
    /// …giving a source's task to someone else.
    pub swapped: usize,
    /// …saying an open task was done…
    pub done: usize,
    /// …or an open question settled.
    pub settled: usize,
    /// …too long, or over the summary's words.
    pub too_long: usize,
    /// The model said there was nothing to add.
    pub nothing: bool,
}

/// Why a bullet isn't grounded in its cited sources, if it isn't (`None`:
/// every name, date and number is in them, and most of its words).
pub fn ungrounded(text: &str, cites: &[usize], facts: &Facts) -> Option<String> {
    let mut known: HashSet<String> = HashSet::new();
    for n in cites {
        if let Some(t) = facts.texts.get(n.wrapping_sub(1)) {
            known.extend(words(t));
        }
    }
    let m = &facts.meeting;
    known.extend(words(&m.title));
    for a in &m.attendees {
        known.extend(words(a));
    }
    let tokens: Vec<&str> = text.split_whitespace().collect();
    for (i, raw) in tokens.iter().enumerate() {
        // A sentence's first word has its capital by grammar: it's a name
        // only when it isn't an everyday word ("Whether…", "Freight…").
        let opens = i == 0 || tokens[i - 1].ends_with(['.', ':', ';', '!', '?']);
        // Pieces of "Priya's", "Q4/Q5", "re-plan" checked one by one.
        for (j, piece) in raw.split(['/', '-', '\u{2013}', '\u{2014}']).enumerate() {
            let w = canon(piece);
            if w.is_empty() {
                continue;
            }
            let bare = piece.trim_matches(|c: char| !c.is_alphanumeric());
            let capital = bare.chars().next().is_some_and(char::is_uppercase);
            let digit = w.chars().any(|c| c.is_ascii_digit());
            let lower_month = !capital && matches!(w.as_str(), "may" | "march");
            let time = !lower_month && (DAYS.iter().chain(MONTHS.iter()).any(|(full, _)| *full == w) || RELATIVE.contains(&w.as_str()));
            // ("Will", "Mark", "Grace"… are names too: checked.)
            let everyday = opens
                && j == 0
                && (STARTERS.contains(&w.as_str())
                    || (crate::dictionary::learn::is_frequent(&w) && !crate::memory::vocab::NAME_WORDS.contains(&w.as_str())));
            let pronoun = matches!(w.as_str(), "i" | "you" | "your" | "yours" | "yap");
            let checked = digit || time || (capital && !everyday && !pronoun);
            if checked && !known.contains(&w) && !known.contains(&stem(&w)) {
                return Some(format!("\"{bare}\" isn't in its sources"));
            }
        }
    }
    // A deadline its sources never gave ("before the next meeting").
    let source = cites.iter().filter_map(|n| facts.texts.get(n.wrapping_sub(1))).cloned().collect::<Vec<_>>().join("\n").to_lowercase();
    let lower = text.to_lowercase();
    if let Some(phrase) = DEADLINES.iter().find(|p| lower.contains(*p) && !source.contains(*p)) {
        return Some(format!("\"{phrase}\" isn't in its sources"));
    }
    // Most of what it says must be in them: the whole bullet, and each clause.
    let known_stems: HashSet<String> = known.iter().map(|w| stem(w)).collect();
    let content = |s: &str| -> Vec<String> {
        words(s)
            .into_iter()
            .filter(|w| w.len() >= 4 && !STOP.contains(&w.as_str()) && !STARTERS.contains(&w.as_str()))
            .map(|w| stem(&w))
            .collect()
    };
    let all = content(text);
    let found = all.iter().filter(|w| known_stems.contains(*w)).count();
    if all.len() >= 2 && found * 10 < all.len() * 4 {
        return Some(format!("only {found} of its {} words are in its sources", all.len()));
    }
    for clause in clauses(text) {
        if agenda_claim(&clause, cites, facts) == Some(false) {
            return Some(format!("\"{clause}\": the invite doesn't say so"));
        }
        let words = content(&clause);
        let found = words.iter().filter(|w| known_stems.contains(*w)).count();
        if words.len() >= 2 && found * 2 < words.len() {
            return Some(format!("\"{clause}\" is mostly not in its sources"));
        }
    }
    None
}

/// Deadlines a bullet may only give when its sources do.
const DEADLINES: &[&str] = &[
    "before the meeting",
    "before the next meeting",
    "by the next meeting",
    "before this meeting",
    "by this meeting",
    "next week",
    "this week",
    "end of the week",
    "end of the day",
    "end of day",
    "asap",
    "as soon as possible",
    "urgent",
    "overdue",
];

/// A bullet's clauses, each judged on its own: "The checklist is due from
/// Daniel, and the timeline is due from you" says two things about two
/// people.
fn clauses(text: &str) -> Vec<String> {
    let mut parts = vec![text.to_string()];
    for sep in ["; ", ", and ", ", while ", ", but ", ", so ", ", which ", " while ", " whereas "] {
        parts = parts.iter().flat_map(|p| p.split(sep).map(str::to_string).collect::<Vec<_>>()).collect();
    }
    // "… and Rochelle will draft …": a new subject after a plain "and".
    let mut out = Vec::new();
    for p in parts {
        let mut rest = p.as_str();
        while let Some(at) = rest.find(" and ") {
            let next = rest[at + 5..].split_whitespace().next().unwrap_or("");
            let subject = next.chars().next().is_some_and(char::is_uppercase)
                || matches!(next.to_lowercase().as_str(), "you" | "she" | "he" | "they" | "we");
            if !subject {
                break;
            }
            out.push(rest[..at].to_string());
            rest = &rest[at + 5..];
        }
        out.push(rest.to_string());
    }
    out.into_iter().map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect()
}

/// The tasks a clause is about: the one most of whose words it has (at
/// least two of them and a quarter), and any other it has most of — among
/// the tasks of the sources it cites, else of any earlier meeting (a task
/// told under the wrong citation is still that task).
fn tasks_of<'a>(clause: &str, cites: &[usize], facts: &'a Facts) -> Vec<&'a Task> {
    let said: HashSet<String> = words(clause).into_iter().map(|w| stem(&w)).collect();
    let pick = |cited: bool| -> Vec<&'a Task> {
        let mut scored: Vec<(usize, usize, &Task)> = facts
            .tasks
            .iter()
            .filter(|(n, _)| !cited || cites.contains(n))
            .filter_map(|(_, t)| {
                let want = task_words(&t.task);
                let hit = want.iter().filter(|w| said.contains(*w)).count();
                (hit >= 2.min(want.len()) && hit > 0 && hit * 4 >= want.len()).then_some((hit * 1000 / want.len(), hit, t))
            })
            .collect();
        scored.sort_by_key(|(share, hit, _)| std::cmp::Reverse((*share, *hit)));
        scored.iter().enumerate().filter(|(i, (share, _, _))| *i == 0 || *share >= 600).map(|(_, (_, _, t))| *t).collect()
    };
    let cited = pick(true);
    if cited.is_empty() {
        pick(false)
    } else {
        cited
    }
}

/// Does the bullet give one of its sources' tasks to someone other than the
/// task's owner (and never name the owner in that clause)? Someone the task
/// itself names ("Send Priya the deck") only counts as its doer when the
/// clause starts with them ("Priya sends the deck").
pub fn swaps_owner(text: &str, cites: &[usize], facts: &Facts) -> bool {
    let all = &facts.meeting.attendees;
    let first_name = |name: &str| first_of(&norm(name)).to_string();
    clauses(text).iter().any(|clause| {
        let said = words(clause);
        let mentions = |name: &str| {
            if name == "You" {
                return said.iter().any(|w| matches!(w.as_str(), "you" | "your" | "yours"));
            }
            said.contains(&first_name(name))
        };
        let opener = said.first().cloned().unwrap_or_default();
        tasks_of(clause, cites, facts).into_iter().any(|t| {
            if mentions(&t.owner) {
                return false;
            }
            let in_task: HashSet<String> = words(&t.task).into_iter().collect();
            let other = all.iter().any(|p| {
                let first = first_name(p);
                !same_person(&t.owner, p, all) && if in_task.contains(&first) { opener == first } else { mentions(p) }
            });
            other || (t.owner != "You" && mentions("You"))
        })
    })
}

/// Words that keep a question open ("whether…", "still undecided").
const OPEN_WORDS: &[&str] = &[
    "whether", "open", "undecided", "unclear", "unanswered", "unresolved", "question", "questions", "pending", "still",
    "if", "decide", "discuss", "unsure", "raised", "ask", "asked", "asking", "settle", "yet",
];

/// Does the bullet tell an open question of an earlier meeting as settled
/// ("The annual plan should get a discount" for "Should the annual plan get
/// a discount?"), in a clause with nothing keeping it open?
pub fn settles_question(text: &str, _cites: &[usize], facts: &Facts) -> bool {
    clauses(text).iter().any(|clause| {
        let said = words(clause);
        if clause.contains('?') || said.iter().any(|w| OPEN_WORDS.contains(&w.as_str())) {
            return false;
        }
        let stems: HashSet<String> = said.iter().map(|w| stem(w)).collect();
        facts.questions.iter().any(|(_, q)| {
            let want = task_words(q);
            let hit = want.iter().filter(|w| stems.contains(*w)).count();
            hit >= 2 && hit * 2 >= want.len()
        })
    })
}

/// Words a clause about the agenda uses besides what's on it.
const AGENDA_TALK: &[&str] = &[
    "agenda", "invite", "covers", "cover", "covered", "includes", "include", "included", "focuses", "focus", "item", "items",
    "list", "lists", "listed", "key", "main", "topic", "topics",
];

/// A clause saying something is on the agenda ("…and the Q1 roadmap is on
/// the agenda"): `Some(true)` when the invite has it (some of the clause's
/// words, and with what it cites most of them), `Some(false)` when it
/// doesn't or there's no invite, `None` for a clause not about the agenda.
fn agenda_claim(clause: &str, cites: &[usize], facts: &Facts) -> Option<bool> {
    let said = words(clause);
    if !said.iter().any(|w| w == "agenda" || w == "invite") {
        return None;
    }
    let Some(invite) = facts.sources.iter().find(|s| s.kind == "invite") else { return Some(false) };
    let invite_words: HashSet<String> =
        facts.texts.get(invite.n - 1).map(|t| words(t).into_iter().map(|w| stem(&w)).collect()).unwrap_or_default();
    let mut known = invite_words.clone();
    for n in cites {
        if let Some(t) = facts.texts.get(n.wrapping_sub(1)) {
            known.extend(words(t).into_iter().map(|w| stem(&w)));
        }
    }
    let content: Vec<String> = said
        .iter()
        .filter(|w| w.len() >= 4 && !STOP.contains(&w.as_str()) && !STARTERS.contains(&w.as_str()) && !AGENDA_TALK.contains(&w.as_str()))
        .map(|w| stem(w))
        .collect();
    if content.is_empty() {
        return Some(true);
    }
    let on_it = content.iter().filter(|w| invite_words.contains(*w)).count();
    let known_n = content.iter().filter(|w| known.contains(*w)).count();
    Some(on_it >= 1 && known_n * 2 >= content.len())
}

/// Past forms English doesn't make with "-ed".
const IRREGULAR: &[(&str, &[&str])] = &[
    ("send", &["sent"]),
    ("write", &["wrote", "written"]),
    ("make", &["made"]),
    ("get", &["got", "gotten"]),
    ("buy", &["bought"]),
    ("bring", &["brought"]),
    ("do", &["did", "done"]),
    ("take", &["took", "taken"]),
    ("give", &["gave", "given"]),
    ("find", &["found"]),
    ("pay", &["paid"]),
    ("tell", &["told"]),
    ("run", &["ran"]),
    ("lead", &["led"]),
    ("meet", &["met"]),
    ("speak", &["spoke", "spoken"]),
    ("draw", &["drew", "drawn"]),
    ("sell", &["sold"]),
    ("build", &["built"]),
    ("choose", &["chose", "chosen"]),
    ("hold", &["held"]),
    ("see", &["saw", "seen"]),
];

/// The past forms of a task's verb ("send" → "sent", "draft" → "drafted",
/// "update" → "updated", "plan" → "planned").
fn past_forms(verb: &str) -> Vec<String> {
    if let Some((_, forms)) = IRREGULAR.iter().find(|(v, _)| *v == verb) {
        return forms.iter().map(|f| f.to_string()).collect();
    }
    let mut out = vec![if verb.ends_with('e') { format!("{verb}d") } else { format!("{verb}ed") }];
    if let Some(stem) = verb.strip_suffix('y').filter(|s| !s.ends_with(['a', 'e', 'o', 'u'])) {
        out.push(format!("{stem}ied"));
    }
    if let Some(last) = verb.chars().last().filter(|c| c.is_ascii_alphabetic() && !"aeiouwxy".contains(*c)) {
        out.push(format!("{verb}{last}ed"));
    }
    out
}

/// Verbs its sources say someone will do: "I'll send…", "will book…", "can
/// you check…", "going to draft…".
fn future_verbs(text: &str) -> HashSet<String> {
    let tokens: Vec<String> = text
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'' && c != '\u{2019}').to_lowercase())
        .collect();
    let mut out = HashSet::new();
    for (i, w) in tokens.iter().enumerate() {
        let next = match w.as_str() {
            "i'll" | "i\u{2019}ll" | "we'll" | "she'll" | "he'll" | "they'll" | "will" => tokens.get(i + 1),
            "going" if tokens.get(i + 1).is_some_and(|t| t == "to") => tokens.get(i + 2),
            "can" | "could" if tokens.get(i + 1).is_some_and(|t| t == "you") => tokens.get(i + 2),
            _ => None,
        };
        if let Some(v) = next.filter(|v| v.len() > 1 && v.chars().all(char::is_alphabetic) && !matches!(v.as_str(), "be" | "have" | "the" | "you" | "not")) {
            out.insert(v.clone());
        }
    }
    out
}

/// Does the bullet say one of its sources' open tasks was done ("The travel
/// budget was confirmed", "You sent Priya the pricing table"), or something
/// its sources only say someone will do ("Rochelle has sent the quotes" for
/// "I'll send both quotes")? The notes only know what was still to do.
pub fn claims_done(text: &str, cites: &[usize], facts: &Facts) -> bool {
    let finished = |said: &[String]| {
        said.iter().any(|w| matches!(w.as_str(), "done" | "completed" | "complete" | "finished" | "already" | "sorted" | "delivered"))
    };
    let about_a_task = clauses(text).iter().any(|clause| {
        let said = words(clause);
        tasks_of(clause, cites, facts).into_iter().any(|t| {
            let verb = words(&t.task).into_iter().next().unwrap_or_default();
            finished(&said) || past_forms(&verb).iter().any(|p| said.contains(p))
        })
    });
    if about_a_task {
        return true;
    }
    let source: String = cites.iter().filter_map(|n| facts.texts.get(n.wrapping_sub(1))).cloned().collect::<Vec<_>>().join("\n");
    let source_words: HashSet<String> = words(&source).into_iter().collect();
    let said = words(text);
    future_verbs(&source)
        .iter()
        .flat_map(|v| past_forms(v))
        .any(|p| said.contains(&p) && !source_words.contains(&p))
}

/// The model's way of naming the user, as the brief speaks to them: "The
/// user will ask" → "You will ask", "the user's" → "your".
fn second_person(text: &str) -> String {
    let mut out = text.to_string();
    for (from, to) in [
        ("The user is ", "You are "),
        ("the user is ", "you are "),
        ("The user was ", "You were "),
        ("the user was ", "you were "),
        ("The user has ", "You have "),
        ("the user has ", "you have "),
        ("The user's ", "Your "),
        ("the user's ", "your "),
        ("The user ", "You "),
        ("the user ", "you "),
    ] {
        out = out.replace(from, to);
    }
    out
}

/// Check the model's reply line by line and keep what passes: a bullet must
/// cite a real source, be grounded in what it cites ([`ungrounded`]), keep
/// tasks with their owners ([`swaps_owner`]), say no open task was done
/// ([`claims_done`]) and no open question settled ([`settles_question`]),
/// and be short; at most [`SUMMARY_BULLETS`] bullets and [`SUMMARY_WORDS`]
/// words. "The user" becomes "you", and bullets run together on one line
/// are taken apart.
pub fn check_summary(reply: &str, facts: &Facts) -> Checked {
    let mut out = Checked::default();
    // Bullets a small model ran together on one line ("… [1] - The team…").
    let reply = reply.replace("**", "").replace("__", "").replace("] - ", "]\n- ").replace("]- ", "]\n- ");
    let mut used = 0;
    for raw in reply.lines() {
        let line = raw.trim().trim_start_matches(['-', '*', '\u{2022}']).trim();
        // "1. " / "2) " numbering (not "3 November…").
        let digits = line.chars().take_while(char::is_ascii_digit).count();
        let line = match line[digits..].strip_prefix(['.', ')']) {
            Some(rest) if digits > 0 && rest.starts_with(' ') => rest.trim(),
            _ => line,
        };
        if line.is_empty() {
            continue;
        }
        if line.trim_end_matches('.').eq_ignore_ascii_case("nothing to add") {
            out.nothing = true;
            continue;
        }
        let (cites, text) = citations(line);
        let text = second_person(&text);
        if count_words(&text) < 3 || text.ends_with(':') || text.starts_with('#') {
            continue;
        }
        out.claims += 1;
        if cites.is_empty() || cites.iter().any(|n| *n == 0 || *n > facts.sources.len()) {
            out.uncited += 1;
            continue;
        }
        // "…is on the agenda" with the invite left out of the citation: Yap adds it.
        let mut cites = cites;
        if let Some(invite) = facts.sources.iter().find(|s| s.kind == "invite") {
            if !cites.contains(&invite.n) && clauses(&text).iter().any(|c| agenda_claim(c, &cites, facts) == Some(true)) {
                cites.push(invite.n);
                cites.sort_unstable();
            }
        }
        if ungrounded(&text, &cites, facts).is_some() {
            out.ungrounded += 1;
            continue;
        }
        if swaps_owner(&text, &cites, facts) {
            out.swapped += 1;
            continue;
        }
        if claims_done(&text, &cites, facts) {
            out.done += 1;
            continue;
        }
        if settles_question(&text, &cites, facts) {
            out.settled += 1;
            continue;
        }
        let n = count_words(&text);
        if n > BULLET_WORDS || used + n > SUMMARY_WORDS || out.lines.len() >= SUMMARY_BULLETS {
            out.too_long += 1;
            continue;
        }
        used += n;
        out.lines.push(Line::new(capitalise(&text), cites));
    }
    out
}

/// The smallest model on this PC that writes "In short", in billions of
/// parameters. In the evaluations (docs/model-evals.md, "Pre-meeting
/// briefs") Qwen2.5-1.5B wrote about half its bullets wrong — uncited, off
/// its sources, a task done or moved to someone else — and a few subtler
/// ones got past the checks (the cheaper venue swapped); Qwen3.5-4B wrote
/// almost none. Below this size the brief is the plain one.
const SUMMARY_MIN_PARAMS_B: f32 = 3.5;

/// A model's size from its id ("qwen2.5-1.5b-instruct-q4_k_m",
/// "Qwen_Qwen3.5-4B-Q4_K_M", "qwen3.5:4b"), as `tools.rs` reads it; 0 when
/// the id names none.
fn model_size_b(model: &str) -> f32 {
    static SIZE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"[-_:]([\d.]+)[bB](?:[^a-zA-Z]|$)").expect("static regex"));
    SIZE.captures(model).and_then(|c| c.get(1)).and_then(|m| m.as_str().parse::<f32>().ok()).unwrap_or(0.0)
}

/// Does this model write "In short"? A cloud or custom endpoint does; one on
/// this PC (`local`) from [`SUMMARY_MIN_PARAMS_B`] up, or whose size its
/// name doesn't say (a server's own name for it).
pub fn writes_summary(local: bool, model: &str) -> bool {
    let size = model_size_b(model);
    !local || size == 0.0 || size >= SUMMARY_MIN_PARAMS_B
}

/// Write "In short" for `facts` on `ep` (the meeting's model): one call —
/// beside any dictation when there's an app (`chat_beside_dictation`: it
/// waits one out, steps aside for a new one, and holds the on-device model
/// meanwhile), straight in a model evaluation — then the checks. The reply
/// as the model wrote it comes back too (the evaluations score it).
pub async fn write(
    app: Option<&tauri::AppHandle>,
    facts: &Facts,
    ep: &crate::meetings::meeting_summary::Endpoint,
) -> Result<(Checked, String), String> {
    let Some(messages) = messages(facts) else {
        return Ok((Checked::default(), String::new()));
    };
    let opts = ep.options(REPLY_TOKENS, 0.2);
    // Recorded as the calendar's brief in the network receipts (net.rs).
    let ask = async {
        match app {
            Some(app) => crate::meetings::meeting_summary::chat_beside_dictation(app, ep, messages, &opts).await,
            None => crate::ai::llm::chat(&ep.base_url, &ep.api_key, &ep.model, &ep.provider, messages, &opts).await,
        }
    };
    let reply = crate::privacy::net::within(crate::privacy::net::Feature::Brief, ask).await?;
    Ok((check_summary(&reply, facts), reply))
}

// ---- the brief ----------------------------------------------------------------------------------

fn part(kind: &str, title: &str, lines: &[Line]) -> Option<Part> {
    (!lines.is_empty()).then(|| Part { kind: kind.into(), title: title.into(), lines: lines.to_vec() })
}

/// Words in the brief as drawn: titles, lines, and each earlier meeting's date.
fn brief_words(parts: &[Part]) -> usize {
    parts
        .iter()
        .map(|p| {
            count_words(&p.title)
                + p.lines.iter().map(|l| l.words() + if p.kind == "last" && !l.cites.is_empty() { 3 } else { 0 }).sum::<usize>()
        })
        .sum()
}

/// The brief from `facts` and the AI's checked `summary` (none: the plain
/// version, `note` saying why), fitted under [`MAX_WORDS`]: the invite's
/// lines give way first, then memory, open items past two, "In short" past
/// one bullet, and earlier meetings past one.
pub fn assemble(facts: &Facts, summary: Vec<Line>, ai: bool, note: &str, now: i64) -> Brief {
    let (mut summary, mut last, mut open, mut memory, mut agenda) =
        (summary, facts.last.clone(), facts.open.clone(), facts.memory.clone(), facts.agenda.clone());
    let build = |s: &[Line], l: &[Line], o: &[Line], m: &[Line], a: &[Line]| -> Vec<Part> {
        [
            part("summary", "In short", s),
            part("last", "Last time", l),
            part("open", "Open items", o),
            part("memory", "From memory", m),
            part("agenda", "On the invite", a),
        ]
        .into_iter()
        .flatten()
        .collect()
    };
    let mut parts = build(&summary, &last, &open, &memory, &agenda);
    while brief_words(&parts) > MAX_WORDS {
        if !agenda.is_empty() {
            agenda.pop();
        } else if !memory.is_empty() {
            memory.pop();
        } else if open.len() > 2 {
            open.pop();
        } else if summary.len() > 1 {
            summary.pop();
        } else if last.iter().filter(|l| !l.cites.is_empty()).count() > 1 {
            let at = last.iter().rposition(|l| !l.cites.is_empty()).unwrap_or(0);
            last.remove(at);
        } else if !open.is_empty() {
            open.pop();
        } else {
            break;
        }
        parts = build(&summary, &last, &open, &memory, &agenda);
    }
    // Only the sources something still cites.
    let cited: HashSet<usize> = parts.iter().flat_map(|p| p.lines.iter().flat_map(|l| l.cites.iter().copied())).collect();
    let sources = facts.sources.iter().filter(|s| cited.contains(&s.n)).cloned().collect();
    Brief {
        key: facts.meeting.key.clone(),
        title: facts.meeting.title.clone(),
        start: facts.meeting.start,
        first: facts.first,
        ai: ai && parts.iter().any(|p| p.kind == "summary"),
        writing: false,
        note: note.to_string(),
        words: brief_words(&parts),
        parts,
        sources,
        made_ts: now,
        fingerprint: String::new(),
    }
}

/// The brief as plain text (logs at debug, the evaluations' report).
pub fn plain_text(b: &Brief) -> String {
    let mut out = String::new();
    for p in &b.parts {
        out.push_str(&format!("{}\n", p.title));
        for l in &p.lines {
            let who = l.who.as_deref().map(|w| format!("{w}: ")).unwrap_or_default();
            let due = l.due.as_deref().map(|d| format!(" (due: {d})")).unwrap_or_default();
            let cites = if l.cites.is_empty() {
                String::new()
            } else {
                format!(" [{}]", l.cites.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(", "))
            };
            out.push_str(&format!("- {who}{}{due}{cites}\n", l.text));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;
    const NOW: i64 = 1_792_000_000;

    fn note(id: u64, title: &str, days_ago: i64, participants: &[&str], plan: &str, key: Option<&str>) -> Note {
        let mut n: Note = serde_json::from_value(json!({
            "id": id, "title": title, "createdTs": NOW - days_ago * DAY, "updatedTs": NOW - days_ago * DAY + 60,
            "noteType": "meeting", "participants": participants, "enhancedContent": plan,
        }))
        .unwrap();
        if let Some(key) = key {
            n.event = Some(crate::notes::NoteEvent {
                key: key.into(),
                title: title.into(),
                start: (NOW - days_ago * DAY) as u64,
                end: (NOW - days_ago * DAY + 1800) as u64,
                description: String::new(),
                service: String::new(),
            });
        }
        n
    }

    fn meeting(key: &str, title: &str, attendees: &[&str], description: &str) -> Meeting {
        Meeting {
            key: key.into(),
            title: title.into(),
            start: NOW + 600,
            attendees: attendees.iter().map(|s| s.to_string()).collect(),
            description: description.into(),
        }
    }

    fn person(id: u64, name: &str, about: &str, facts: &[&str]) -> Item {
        serde_json::from_value(json!({
            "id": id, "section": "people", "title": name, "summary": about, "details": facts, "updatedTs": 5,
        }))
        .unwrap()
    }

    const PLAN: &str = "We went through the Q4 launch.\n\n## Action plan\n### You\n- [ ] Send Priya the pricing deck (due: Friday)\n- [x] Book the room\n### Priya\n- [ ] Draft the press release\n### Sam\n- [ ] Order the banners\n\n## Decisions\n- Launch on 3 November.\n\n## Unassigned\n- [ ] Update the wiki";

    #[test]
    fn people_are_the_same_only_when_their_names_agree() {
        let all = vec!["Priya Shah".to_string(), "Sam Lee".to_string()];
        assert!(same_person("Priya Shah", "Priya Shah", &all));
        assert!(same_person("priya  shah", "Priya Shah", &all));
        assert!(same_person("Priya", "Priya Shah", &all));
        assert!(!same_person("Priya Patel", "Priya Shah", &all));
        // Two Priyas coming: a lone "Priya" is neither.
        let two = vec!["Priya Shah".to_string(), "Priya Patel".to_string()];
        assert!(!same_person("Priya", "Priya Shah", &two));
        assert!(same_person("Priya Patel", "Priya Patel", &two));
        assert!(!same_person("", "Priya Shah", &all));
    }

    #[test]
    fn a_series_comes_from_the_key() {
        assert_eq!(series_of("2:weekly@example.com:1791800000").as_deref(), Some("weekly@example.com"));
        assert_eq!(series_of("1:abc123_20261005T140000Z").as_deref(), Some("abc123"));
        assert_eq!(series_of("1:abc123_20261005").as_deref(), Some("abc123"));
        // One-offs.
        assert_eq!(series_of("2:design-review@e2e"), None);
        assert_eq!(series_of("1:roadmap_e2e"), None);
        assert_eq!(series_of("nokey"), None);
        assert_eq!(series_of("2:x:12"), None);
    }

    #[test]
    fn tasks_come_from_the_action_plan_with_their_owners() {
        let n = note(1, "Launch sync", 7, &["Priya Shah"], PLAN, None);
        let tasks = plan_tasks(&n);
        assert_eq!(
            tasks,
            vec![
                Task { owner: "You".into(), task: "Send Priya the pricing deck".into(), due: "Friday".into() },
                Task { owner: "Priya".into(), task: "Draft the press release".into(), due: String::new() },
                Task { owner: "Sam".into(), task: "Order the banners".into(), due: String::new() },
            ]
        );
        // The Meeting Notes action's "- [ ] Name: task" lines.
        let mut inline = note(2, "x", 7, &[], "## Action Items\n- [ ] You: email the client\n- [ ] Priya Shah: check the numbers (due: Monday)\n- [ ] tidy the board", None);
        let tasks = plan_tasks(&inline);
        assert_eq!(tasks.len(), 2);
        assert_eq!((tasks[1].owner.as_str(), tasks[1].due.as_str()), ("Priya Shah", "Monday"));
        // No plan: the digests' tasks.
        inline.enhanced_content.clear();
        inline.digests = vec![serde_json::from_value(json!({
            "fromSeg": 0, "toSeg": 3, "startTs": 1, "endTs": 2, "actions": [
                { "owner": "Unassigned", "task": "book a room" }, { "owner": "You", "task": "share the notes", "due": "today" }
            ]
        }))
        .unwrap()];
        assert_eq!(plan_tasks(&inline), vec![Task { owner: "You".into(), task: "share the notes".into(), due: "today".into() }]);
    }

    #[test]
    fn a_brief_gathers_last_time_open_items_memory_and_the_agenda() {
        let notes = vec![
            note(1, "Weekly 1:1 with Priya", 7, &["Priya Shah"], PLAN, Some("2:weekly@example.com:1791395200")),
            note(2, "Weekly 1:1 with Priya", 14, &["Priya Shah"], "## Action plan\n### Priya\n- [ ] Draft the press release\n### You\n- [ ] Ask legal about the trademark", Some("2:weekly@example.com:1790790400")),
            // Someone else's meeting: not in it.
            note(3, "Budget", 3, &["Rochelle Adams"], "## Action plan\n### Rochelle\n- [ ] Send the budget", None),
            // A draft with nothing in it: not a "last time".
            note(4, "Weekly 1:1 with Priya", 2, &["Priya Shah"], "", None),
        ];
        let memory = vec![
            person(7, "Priya Shah", "Product lead at Acme", &["Lives in Bristol"]),
            person(8, "Rochelle Adams", "Sister", &[]),
            serde_json::from_value(json!({ "id": 9, "section": "projects", "title": "Q4 launch", "summary": "The autumn release" })).unwrap(),
        ];
        let m = meeting("2:weekly@example.com:1792000600", "Weekly 1:1 with Priya", &["Priya Shah"], "Agenda: the Q4 launch\nPricing");
        let f = gather(&m, &notes, &memory, NOW);
        assert!(!f.first);
        assert_eq!(f.last.iter().map(|l| (l.text.as_str(), l.cites.clone())).collect::<Vec<_>>(), [
            ("Weekly 1:1 with Priya", vec![1]),
            ("Weekly 1:1 with Priya", vec![2])
        ]);
        // You's and Priya's, not Sam's (not coming), the ticked one or the
        // unassigned one; the press release once (the newest note's).
        let open: Vec<String> = f
            .open
            .iter()
            .map(|l| format!("{}: {} ({}) {:?}", l.who.as_deref().unwrap_or(""), l.text, l.due.as_deref().unwrap_or(""), l.cites))
            .collect();
        assert_eq!(open, [
            "You: Send Priya the pricing deck (Friday) [1]",
            "Priya: Draft the press release () [1]",
            "You: Ask legal about the trademark () [2]",
        ]);
        // Priya from memory, and the project the invite names; not the sister.
        assert_eq!(f.memory.iter().map(|l| l.who.clone().unwrap()).collect::<Vec<_>>(), ["Priya Shah", "Q4 launch"]);
        assert_eq!(f.memory[0].text, "Product lead at Acme; Lives in Bristol");
        assert_eq!(f.agenda.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(), ["Agenda: the Q4 launch", "Pricing"]);
        assert_eq!(f.sources.iter().map(|s| s.kind.as_str()).collect::<Vec<_>>(), ["note", "note", "memory", "memory", "invite"]);
        let input = f.model_input();
        assert!(input.contains("Who's coming: Priya Shah"));
        assert!(input.contains("[1] Notes from \"Weekly 1:1 with Priya\""));
        assert!(input.contains("[3] What Yap remembers about Priya Shah: Product lead at Acme. Lives in Bristol."));
        assert!(input.contains("[5] The invite: Agenda: the Q4 launch"));
        // The plain brief: under 150 words, every line but the absences cited.
        let b = assemble(&f, vec![], false, "No AI model", NOW);
        assert!(b.words <= MAX_WORDS, "{}", b.words);
        assert_eq!(b.parts.iter().map(|p| p.kind.as_str()).collect::<Vec<_>>(), ["last", "open", "memory", "agenda"]);
        assert!(b.parts.iter().flat_map(|p| &p.lines).all(|l| !l.cites.is_empty()));
        assert_eq!(b.open_count(), 3);
        // The fingerprint follows the inputs.
        let fp = f.fingerprint("plain");
        let mut edited = notes.clone();
        edited[0].updated_ts += 1;
        assert_ne!(gather(&m, &edited, &memory, NOW).fingerprint("plain"), fp);
        assert_ne!(f.fingerprint("qwen"), fp);
        assert_eq!(gather(&m, &notes, &memory, NOW).fingerprint("plain"), fp);
    }

    #[test]
    fn a_first_meeting_says_so_and_a_newcomer_is_named() {
        let m = meeting("1:kickoff", "Kick-off", &["Siobhan Byrne"], "");
        let f = gather(&m, &[], &[], NOW);
        assert!(f.first);
        assert!(!f.worth_a_call());
        assert!(messages(&f).is_none());
        let b = assemble(&f, vec![], false, "", NOW);
        assert_eq!(b.parts.len(), 1);
        assert_eq!(b.parts[0].lines, [Line::new("First meeting with these people", vec![])]);
        assert!(b.sources.is_empty());
        // Priya known, Siobhan new.
        let notes = vec![note(1, "Launch sync", 7, &["Priya Shah"], PLAN, None)];
        let m = meeting("1:x", "Launch sync", &["Priya Shah", "Siobhan Byrne"], "");
        let f = gather(&m, &notes, &[], NOW);
        assert!(!f.first);
        assert_eq!(f.last.last().unwrap().text, "First meeting with Siobhan Byrne");
        // Notes after the meeting starts, or about other Priyas, aren't last time.
        let later = vec![note(1, "Launch sync", -1, &["Priya Shah"], PLAN, None), note(2, "Other", 3, &["Priya Patel"], PLAN, None)];
        assert!(gather(&meeting("1:x", "Launch sync", &["Priya Shah"], ""), &later, &[], NOW).first);
    }

    #[test]
    fn the_same_series_counts_whoever_came() {
        let notes = vec![note(1, "Team retro", 14, &["Sam Lee"], PLAN, Some("1:retro_20261001T090000Z"))];
        let m = meeting("1:retro_20261015T090000Z", "Team retro", &["Tanay Kothari"], "");
        let f = gather(&m, &notes, &[], NOW);
        assert_eq!(f.last[0].text, "Team retro");
        assert_eq!(f.last[1].text, "First meeting with Tanay Kothari");
        // Nobody's tasks are Tanay's or You's but "Send Priya…" (You's).
        assert_eq!(f.open.iter().map(|l| l.who.as_deref()).collect::<Vec<_>>(), [Some("You")]);
    }

    fn facts() -> Facts {
        let notes = vec![note(1, "Weekly 1:1 with Priya", 7, &["Priya Shah"], PLAN, None)];
        let memory = vec![person(7, "Priya Shah", "Product lead at Acme", &[])];
        gather(&meeting("1:w", "Weekly 1:1 with Priya", &["Priya Shah", "Sam Lee"], "Agenda: pricing"), &notes, &memory, NOW)
    }

    #[test]
    fn the_ais_bullets_must_cite_and_stay_in_their_sources() {
        let f = facts();
        let reply = "Here's your brief:\n\
            - Last time you went through the Q4 launch and decided to launch on 3 November. [1]\n\
            - Priya is drafting the press release. [1]\n\
            - The launch slipped to 9 November. [1]\n\
            - Priya Shah leads product at Acme, and pricing is on the agenda. [2, 3]\n\
            - Tom will bring the budget. [1]\n\
            - Priya sends the pricing deck by Friday. [1]\n\
            - Something without a source.\n\
            - A source that doesn't exist. [9]";
        let c = check_summary(reply, &f);
        assert_eq!(
            c.lines.iter().map(|l| (l.text.as_str(), l.cites.clone())).collect::<Vec<_>>(),
            [
                ("Last time you went through the Q4 launch and decided to launch on 3 November.", vec![1]),
                ("Priya is drafting the press release.", vec![1]),
                ("Priya Shah leads product at Acme, and pricing is on the agenda.", vec![2, 3]),
            ]
        );
        assert_eq!((c.claims, c.uncited, c.ungrounded, c.swapped), (8, 2, 2, 1));
        assert!(!c.nothing);
        // "9 November" and "Tom" aren't in [1]; the deck is You's task, not Priya's.
        assert!(ungrounded("The launch slipped to 9 November.", &[1], &f).is_some());
        assert!(ungrounded("Tom will bring the budget.", &[1], &f).is_some());
        assert!(swaps_owner("Priya sends the pricing deck by Friday.", &[1], &f));
        assert!(!swaps_owner("You still owe Priya the pricing deck, due Friday.", &[1], &f));
        // The note's date grounds "Thursday"-style words only when it's that day.
        assert!(ungrounded("You met on Saturday about pricing.", &[3], &f).is_some());
        // "Nothing to add."
        let c = check_summary("Nothing to add.", &f);
        assert!(c.nothing && c.lines.is_empty() && c.claims == 0);
        // Mostly words the sources don't have.
        assert!(ungrounded("The marketing team celebrated their excellent quarterly revenue growth.", &[1], &f).is_some());
    }

    #[test]
    fn an_open_task_is_never_said_to_be_done() {
        let f = facts();
        // As Qwen2.5-1.5B wrote in the first evaluation run.
        assert!(claims_done("You sent Priya the pricing deck, and she drafted the FAQ.", &[1], &f));
        assert!(claims_done("The banners were ordered.", &[1], &f));
        assert!(claims_done("The press release is already drafted.", &[1], &f));
        // Still to do, however it's put.
        assert!(!claims_done("You still owe Priya the pricing deck.", &[1], &f));
        assert!(!claims_done("Priya is drafting the press release.", &[1], &f));
        assert!(!claims_done("You agreed to send Priya the pricing deck.", &[1], &f));
        // A task told under the wrong citation is still that task.
        assert!(claims_done("The banners were ordered.", &[2], &f));
        assert_eq!(past_forms("send"), ["sent"]);
        assert!(past_forms("update").contains(&"updated".to_string()));
        assert!(past_forms("plan").contains(&"planned".to_string()));
        assert!(past_forms("copy").contains(&"copied".to_string()));
        let c = check_summary("- The press release was drafted. [1]\n- You decided to launch on 3 November. [1]", &f);
        assert_eq!((c.done, c.lines.len()), (1, 1));
    }

    /// What Qwen2.5-1.5B wrote in the second evaluation run, which the first
    /// checks let through.
    #[test]
    fn mixed_up_owners_settled_questions_and_spoken_promises_are_caught() {
        let plan = "## Action plan\n### Rochelle\n- [ ] Collect feedback from the five pilot partners on the onboarding checklist\n### Daniel\n- [ ] Prepare the regional launch timeline with dates for each region\n\n## Open questions\n- Should the annual plan get a discount?";
        let notes = vec![note(1, "Partner planning", 3, &["Rochelle Adams", "Daniel Moss"], plan, None)];
        let f = gather(&meeting("1:p", "Partner planning", &["Rochelle Adams", "Daniel Moss"], ""), &notes, &[], NOW);
        assert!(swaps_owner("The onboarding checklist is due from Daniel, and the regional launch timeline is due from you.", &[1], &f));
        assert!(!swaps_owner("Rochelle is collecting the pilot partners' feedback, and Daniel is preparing the regional timeline.", &[1], &f));
        assert!(settles_question("The annual plan should get a discount.", &[1], &f));
        assert!(!settles_question("Whether the annual plan gets a discount is still open.", &[1], &f));
        assert_eq!(clauses("A, and B and Priya will C; D"), ["A", "B", "Priya will C", "D"]);
        assert_eq!(clauses("Tiers and dates are on the agenda"), ["Tiers and dates are on the agenda"]);

        // A 1:1 with only a transcript: what each side said they'd do.
        let mut chat = note(2, "Offsite chat", 2, &["Rochelle Adams"], "", None);
        chat.transcript = [
            ("you", "Can you send me both quotes by Thursday so I can check them against the budget?"),
            ("them", "Sure, I'll send both quotes by Thursday."),
            ("you", "Great. I'll ask the team which dates in June work for them."),
        ]
        .iter()
        .enumerate()
        .map(|(i, (source, text))| crate::notes::TranscriptSegment { source: source.to_string(), text: text.to_string(), ts: i as u64, ..Default::default() })
        .collect();
        let f = gather(&meeting("1:o", "Offsite planning", &["Rochelle Adams"], ""), &[chat], &[], NOW);
        assert!(claims_done("Rochelle Adams has sent quotes for both venues by Thursday.", &[1], &f));
        assert!(swaps_owner("You have been asked to send quotes for both venues by Thursday.", &[1], &f));
        assert!(!claims_done("Rochelle is sending both quotes by Thursday.", &[1], &f));
        assert!(!swaps_owner("Rochelle is sending both quotes by Thursday.", &[1], &f));
        assert!(!swaps_owner("You're asking the team which June dates work.", &[1], &f));
    }

    #[test]
    fn a_small_models_habits_are_tidied() {
        let f = facts();
        // Bullets run together on one line, and "the user" for you.
        let c = check_summary(
            "- The user decided to launch on 3 November. [1] - Priya is drafting the press release. [1]",
            &f,
        );
        assert_eq!(
            c.lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
            ["You decided to launch on 3 November.", "Priya is drafting the press release."]
        );
        assert_eq!(second_person("The user's deck is late; the user is away."), "Your deck is late; you are away.");
        // Numbering goes, a date at the start stays.
        let c = check_summary("1. You decided to launch on 3 November. [1]\n3 November is the launch date. [1]", &f);
        assert_eq!(c.lines[1].text, "3 November is the launch date.");
    }

    #[test]
    fn a_sentences_first_word_is_a_name_only_when_it_isnt_a_word() {
        let f = facts();
        assert!(ungrounded("Whether to launch on 3 November is still open.", &[1], &f).is_none());
        assert!(ungrounded("Pricing is on the agenda.", &[3], &f).is_none());
        // Names that are also words are still names.
        assert!(ungrounded("Mark will order the banners.", &[1], &f).is_some());
        assert!(ungrounded("Priya is drafting the press release. Tom has the banners.", &[1], &f).is_some());
    }

    #[test]
    fn small_models_on_this_pc_leave_the_summary_out() {
        assert!(!writes_summary(true, "qwen2.5-1.5b-instruct-q4_k_m"));
        assert!(!writes_summary(true, "Qwen_Qwen3.5-2B-Q4_K_M"));
        assert!(writes_summary(true, "Qwen_Qwen3.5-4B-Q4_K_M"));
        assert!(writes_summary(true, "qwen3.5:9b"));
        // A server's own name with no size, and every cloud model: yes.
        assert!(writes_summary(true, "llama3:latest"));
        assert!(writes_summary(false, "openai/gpt-oss-20b"));
        assert!(writes_summary(false, "tiny-1b"));
    }

    #[test]
    fn citations_are_read_and_taken_out() {
        assert_eq!(citations("Priya owes the deck [1]."), (vec![1], "Priya owes the deck.".to_string()));
        assert_eq!(citations("Two sources [1, 3] here"), (vec![1, 3], "Two sources here".to_string()));
        assert_eq!(citations("[2] Leading"), (vec![2], "Leading".to_string()));
        assert_eq!(citations("A [link] stays"), (vec![], "A [link] stays".to_string()));
        // Each source once, in order (the pages key the pills by number).
        assert_eq!(citations("Twice [3, 1] over [3]").0, vec![1, 3]);
    }

    #[test]
    fn a_long_brief_is_cut_to_150_words() {
        let long = "Agenda: ".to_string() + &"talk through the next steps for the regional launch plan ".repeat(8);
        let plans = (1..=3)
            .map(|i| {
                note(
                    i,
                    &format!("A rather long meeting title number {i} about the regional launch"),
                    i as i64,
                    &["Priya Shah"],
                    "## Action plan\n### You\n- [ ] Write up the long detailed regional rollout proposal for the leadership team review next quarter\n### Priya\n- [ ] Collect every single regional partner's feedback on the proposed launch timeline and budget\n- [ ] Prepare the detailed partner onboarding checklist and training schedule",
                    None,
                )
            })
            .collect::<Vec<_>>();
        let memory = vec![person(7, "Priya Shah", "Product lead at Acme who runs the regional partner programme", &["Lives in Bristol", "Prefers short meetings"])];
        let f = gather(&meeting("1:x", "Regional launch", &["Priya Shah"], &long), &plans, &memory, NOW);
        let summary = vec![
            Line::new("Last time you agreed the regional rollout proposal goes to the leadership team next quarter.", vec![1]),
            Line::new("Priya is collecting partner feedback on the launch timeline and budget.", vec![1]),
            Line::new("Priya is preparing the partner onboarding checklist and training schedule.", vec![1]),
        ];
        let b = assemble(&f, summary, true, "", NOW);
        assert!(b.words <= MAX_WORDS, "{} words", b.words);
        assert!(b.ai);
        // The invite went first; the open items keep at least two.
        assert!(b.open_count() >= 2);
        // Every source listed is cited, and every cite has its source.
        let listed: HashSet<usize> = b.sources.iter().map(|s| s.n).collect();
        for l in b.parts.iter().flat_map(|p| &p.lines) {
            assert!(l.cites.iter().all(|n| listed.contains(n)));
        }
    }

    #[test]
    fn the_prompt_keeps_the_guardrails() {
        let p = crate::ai::llm::MEETING_BRIEF_PROMPT;
        assert!(p.contains("never invent"));
        assert!(p.contains("too little"));
        assert!(p.contains("[1]"));
        let f = facts();
        let msgs = messages(&f).unwrap();
        assert_eq!(msgs.as_array().unwrap().len(), 4);
        // An 8k local model reads it with room to spare.
        let size: usize = msgs.as_array().unwrap().iter().map(|m| crate::meetings::meeting_summary::estimate_tokens(m["content"].as_str().unwrap())).sum();
        assert!(size < 2_000, "{size}");
        // The worked example's people never reach a real brief.
        assert!(ungrounded("Okonkwo was getting a second lighting quote.", &[1], &f).is_some());
    }
}

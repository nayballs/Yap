//! **Skills** (2026-10-29, Wispr Flow's Skills): saved instructions the
//! person runs by typing `/name` — in a meeting's Ask box (the notepad's
//! sheet, the Notes view's Ask bar on a meeting note), on that meeting
//! (`meeting_assist::meeting_skill`), or in Chat and Ask across meetings, on
//! the meetings (`chat_send`'s `skill`: the turn is asked across the
//! meetings, its passages found for the Skill — [`find`]). Every answer
//! cites the passages it comes from, as Ask across meetings' do.
//!
//! - **Built in** ([`builtins`]): /followup-email, /decisions, /risks,
//!   /coach, /todos, /weekly-recap — in code, never stored; each can be
//!   copied as the person's own and changed (`basedOn`). Three are Chat's
//!   and Ask across meetings' one-tap prompts too (`chip`): Coach me, List
//!   my open to-dos, Write a weekly recap.
//! - **Your own** (Settings → Skills): a name, a `/command` (lowercase
//!   letters and digits, single hyphens; not a built-in's, not another of
//!   yours), a line on what it's for, the instructions, and where it runs
//!   (one meeting, across meetings, or both).
//!
//! **Not the Actions engine** (`notes.rs` `Action`): an action rewrites the
//! note's summary (`note_enhance` → `write_summary`, under the note
//! guardrails); a Skill answers in a chat thread and never changes the
//! note, has a command and a scope. And actions live in notes.json, which an
//! older Yap rewrites without the fields it doesn't know: a Skill stored
//! there would come back in its action picker as an action, and running it
//! would write over the summary. So Skills have a file of their own,
//! `skills.json`, which an older Yap never opens.
//!
//! Saved through `saving.rs` (atomically; a failed save kept in memory,
//! written with the next change or Try again, said once); a file that
//! doesn't read is set aside (`config::quarantine_corrupt`). Unit tests keep
//! a store per test thread. Logs: ids, commands of built-ins and counts —
//! never a Skill's own words.

#[cfg(not(test))]
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::search::ask::{self, Found, Passage};
use crate::search::chunks::{self, Field};

/// Where a Skill runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunsOn {
    /// A meeting's Ask box, on that meeting.
    Meeting,
    /// Chat and Ask across meetings, on the meetings.
    Meetings,
    #[default]
    Both,
}

impl RunsOn {
    pub fn on_one(self) -> bool {
        self != RunsOn::Meetings
    }
    pub fn across(self) -> bool {
        self != RunsOn::Meeting
    }
}

/// A Skill.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    /// A built-in's: "builtin:<command>"; the person's: "s<ms>".
    pub id: String,
    pub name: String,
    /// Run with `/command`.
    pub command: String,
    #[serde(default)]
    pub description: String,
    pub instructions: String,
    #[serde(default)]
    pub runs_on: RunsOn,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub builtin: bool,
    /// A copy of a built-in: its command.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub based_on: String,
    /// Built-ins: what a run across the meetings reads — "" (a search: the
    /// words typed with it, else `query`), "tasks" (the open tasks), "you"
    /// (the latest meeting's talk, the person's own lines first), "week"
    /// (each meeting of a week: this one, or the one the words name).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub gather: String,
    /// Built-ins: the search when nothing is typed with the command.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub query: String,
    /// Built-ins that are one-tap prompts too: the chip's label…
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub chip: String,
    /// …and the question it sends.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub question: String,
    #[serde(default)]
    pub created_ms: u64,
    #[serde(default)]
    pub updated_ms: u64,
}

/// The person's own Skills at most.
pub const MAX_OWN: usize = 50;
/// A name's, a description's and the instructions' longest.
const NAME_MAX: usize = 60;
const DESCRIPTION_MAX: usize = 160;
pub const INSTRUCTIONS_MAX: usize = 4_000;
/// A command's shortest and longest.
const COMMAND_MIN: usize = 2;
const COMMAND_MAX: usize = 32;

fn builtin(command: &str, name: &str, description: &str, runs_on: RunsOn, instructions: &str) -> Skill {
    Skill {
        id: format!("builtin:{command}"),
        name: name.into(),
        command: command.into(),
        description: description.into(),
        instructions: instructions.into(),
        runs_on,
        builtin: true,
        ..Default::default()
    }
}

/// Yap's own Skills, in the order the `/` menu lists them.
pub fn builtins() -> Vec<Skill> {
    vec![
        builtin(
            "followup-email",
            "Follow-up email",
            "A recap to send the attendees",
            RunsOn::Meeting,
            "Write the follow-up email to send the other attendees. Start with a \"Subject: \" line, then a short greeting, one or two sentences on what the meeting covered, the decisions made, and who does what next (a deadline only when one was said). Sign off with \"[Your name]\". Keep it under 200 words.",
        ),
        builtin(
            "decisions",
            "Decisions",
            "Every decision, who made it and why",
            RunsOn::Both,
            "List every decision that was made, one \"- \" bullet each: the decision, who made or agreed it, and why when a reason was given. Leave out anything that was only discussed. If nothing was decided, say so.",
        ),
        builtin(
            "risks",
            "Risks",
            "Risks, blockers and open questions",
            RunsOn::Both,
            "List the risks, blockers and open questions that came up, one \"- \" bullet each, the most serious first: what it is, who raised it, and what was agreed about it, if anything. Leave out anything that was settled.",
        ),
        Skill {
            gather: "you".into(),
            query: "my last meeting".into(),
            chip: "Coach me".into(),
            question: "Coach me on my last meeting: how did I come across, and what could I do better?".into(),
            ..builtin(
                "coach",
                "Coach me",
                "How you came across, and what to do better",
                RunsOn::Both,
                "Coach the user (\"You\") on how they came across in the meeting: two or three things they did well, then two or three specific things to do better next time (for example a vague ask, a decision left without an owner, talking over someone, a question they could have asked). Tie each point to a moment. Be kind, specific and brief.",
            )
        },
        Skill {
            gather: "tasks".into(),
            chip: "List my open to-dos".into(),
            question: "List my open to-dos.".into(),
            ..builtin(
                "todos",
                "My open to-dos",
                "Your open tasks from every meeting",
                RunsOn::Meetings,
                "List the user's open to-dos, grouped by meeting, newest first: each task with its deadline when there is one. Then, under \"Nobody took these on\", any open tasks without an owner. Leave out tasks that are done or belong to someone else.",
            )
        },
        Skill {
            gather: "week".into(),
            query: "this week".into(),
            chip: "Write a weekly recap".into(),
            question: "Write a recap of this week's meetings.".into(),
            ..builtin(
                "weekly-recap",
                "Weekly recap",
                "The week's meetings in one recap",
                RunsOn::Meetings,
                "Write a recap of the week's meetings: for each meeting, in the order they happened, its name and day, a line on what it was about, then its decisions and next steps as \"- \" bullets. End with the user's (\"You\") open to-dos. Keep it under 300 words.",
            )
        },
    ]
}

/// What skills.json holds: the person's own Skills.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillStore {
    #[serde(default)]
    pub skills: Vec<Skill>,
}

/// skills.json's saves.
pub static SAVER: crate::shell::saving::Saver = crate::shell::saving::Saver::new("skills", "your Skills", save_now);

fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    with_store(|s| save(s));
    SAVER.check()
}

#[cfg(not(test))]
static STATE: Mutex<Option<SkillStore>> = Mutex::new(None);

#[cfg(not(test))]
fn path() -> std::path::PathBuf {
    crate::config::data_dir().join("skills.json")
}

#[cfg(not(test))]
fn load() -> SkillStore {
    let path = path();
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| {
            tracing::error!("skills.json failed to parse; set aside");
            crate::config::quarantine_corrupt(&path);
            SkillStore::default()
        }),
        Err(_) => SkillStore::default(),
    }
}

#[cfg(not(test))]
fn save(store: &SkillStore) {
    if crate::ai::eval::active() {
        return;
    }
    let _ = SAVER.write_json(&path(), store, true);
}

#[cfg(not(test))]
fn with_store<R>(f: impl FnOnce(&mut SkillStore) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    f(guard.get_or_insert_with(load))
}

#[cfg(test)]
thread_local! {
    static TEST_STORE: std::cell::RefCell<SkillStore> = std::cell::RefCell::new(SkillStore::default());
}

#[cfg(test)]
fn save(_: &SkillStore) {}

#[cfg(test)]
fn with_store<R>(f: impl FnOnce(&mut SkillStore) -> R) -> R {
    TEST_STORE.with(|s| f(&mut s.borrow_mut()))
}

/// Every Skill: the built-ins, then the person's own (oldest first).
pub fn all() -> Vec<Skill> {
    let mut out = builtins();
    out.extend(with_store(|s| s.skills.clone()));
    out
}

/// The person's own Skills, as skills.json holds them (Download your data).
pub fn own() -> SkillStore {
    with_store(|s| s.clone())
}

/// The Skill run with `/command` (any case; with or without its slash).
pub fn by_command(command: &str) -> Option<Skill> {
    let c = command.trim().trim_start_matches('/').to_lowercase();
    all().into_iter().find(|s| s.command == c)
}

/// Why `command` can't be a Skill's (`own`: the Skill it's for, when one of
/// the person's is being changed), if it can't. Pure but for the store.
pub fn command_problem(command: &str, own: Option<&str>) -> Option<String> {
    let c = command.trim().trim_start_matches('/');
    if c.is_empty() {
        return Some("Add a command.".into());
    }
    let shaped = c.len() >= COMMAND_MIN
        && c.len() <= COMMAND_MAX
        && c.split('-').all(|w| !w.is_empty() && w.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit()));
    if !shaped {
        return Some(format!(
            "Use {COMMAND_MIN} to {COMMAND_MAX} lowercase letters and numbers, with single hyphens between words."
        ));
    }
    if builtins().iter().any(|b| b.command == c) {
        return Some(format!("/{c} is one of Yap's own Skills. Pick another."));
    }
    let taken = with_store(|s| s.skills.iter().any(|k| k.command == c && Some(k.id.as_str()) != own));
    taken.then(|| format!("You already have a Skill with /{c}."))
}

/// A Skill as the person saved it, cleaned and checked (pure but for the
/// command's check): one-line name and description, the instructions
/// trimmed, each within its length.
pub fn cleaned(mut s: Skill) -> Result<Skill, String> {
    let one_line = |t: &str, max: usize| t.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect::<String>();
    s.name = one_line(&s.name, NAME_MAX);
    s.description = one_line(&s.description, DESCRIPTION_MAX);
    s.command = s.command.trim().trim_start_matches('/').to_string();
    s.instructions = s.instructions.trim().to_string();
    if s.name.is_empty() {
        return Err("Give it a name.".into());
    }
    if s.instructions.is_empty() {
        return Err("Say what the Skill should do.".into());
    }
    if s.instructions.chars().count() > INSTRUCTIONS_MAX {
        return Err(format!("The instructions can be at most {INSTRUCTIONS_MAX} characters."));
    }
    // What only Yap's own carry.
    s.builtin = false;
    s.gather.clear();
    s.query.clear();
    s.chip.clear();
    s.question.clear();
    Ok(s)
}

/// Save one of the person's Skills: a new one (no id, or a built-in's: a
/// copy) or a change to one of theirs. Returns it as stored.
pub fn save_skill(skill: Skill, now_ms: u64) -> Result<Skill, String> {
    let mut s = cleaned(skill)?;
    let mine = !s.id.is_empty() && !s.id.starts_with("builtin:") && with_store(|st| st.skills.iter().any(|k| k.id == s.id));
    if let Some(why) = command_problem(&s.command, mine.then_some(s.id.as_str())) {
        return Err(why);
    }
    with_store(|st| {
        if mine {
            let k = st.skills.iter_mut().find(|k| k.id == s.id).ok_or("That Skill isn't there any more")?;
            s.created_ms = k.created_ms;
            s.updated_ms = now_ms;
            *k = s.clone();
        } else {
            if st.skills.len() >= MAX_OWN {
                return Err(format!("You have {MAX_OWN} Skills, the most there can be. Delete one to add another."));
            }
            s.id = format!("s{now_ms}");
            while st.skills.iter().any(|k| k.id == s.id) {
                s.id.push('x');
            }
            s.created_ms = now_ms;
            s.updated_ms = now_ms;
            st.skills.push(s.clone());
        }
        save(st);
        Ok(s)
    })
}

/// Delete one of the person's Skills (a built-in can't be).
pub fn delete_skill(id: &str) -> Result<(), String> {
    if id.starts_with("builtin:") {
        return Err("Yap's own Skills can't be deleted.".into());
    }
    with_store(|st| {
        let before = st.skills.len();
        st.skills.retain(|k| k.id != id);
        if st.skills.len() == before {
            return Err("That Skill isn't there any more".to_string());
        }
        save(st);
        Ok(())
    })
}

/// A message that runs a Skill: `"/decisions last week"` → the command and
/// the words after it. `None`: no command at the start.
pub fn split_command(text: &str) -> Option<(String, String)> {
    let t = text.trim_start();
    let rest = t.strip_prefix('/')?;
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let command = rest[..end].to_lowercase();
    if command.is_empty() {
        return None;
    }
    Some((command, rest[end..].trim().to_string()))
}

/// What the person asked with the Skill: the words after its command, or —
/// a one-tap prompt's question — the question itself.
pub fn words_of(skill: &Skill, text: &str) -> String {
    match split_command(text) {
        Some((c, words)) if c == skill.command => words,
        _ if text.trim() == skill.question => String::new(),
        _ => text.trim().to_string(),
    }
}

// ---- A Skill run across the meetings (Chat) --------------------------------------------------

/// A Chat turn that runs a Skill.
#[derive(Debug, Clone)]
pub struct SkillTurn {
    pub skill: Skill,
    /// The person's words with it (none: the Skill alone).
    pub words: String,
}

tokio::task_local! {
    static TURN: SkillTurn;
}

/// Run a Chat turn as Skill `turn` (none: an ordinary turn). Chat's own turn
/// asks it across the meetings and reads it with [`current`].
pub async fn with<F: std::future::Future>(turn: Option<SkillTurn>, f: F) -> F::Output {
    match turn {
        Some(t) => TURN.scope(t, f).await,
        None => f.await,
    }
}

/// The Skill the Chat turn running here runs, if any.
pub fn current() -> Option<SkillTurn> {
    TURN.try_with(|t| t.clone()).ok()
}

/// The turn for `chat_send`'s `skill` (its command) and `text`: `Err` when
/// there's no such Skill, or it runs only on one meeting.
pub fn turn_for(command: &str, text: &str) -> Result<SkillTurn, String> {
    let skill = by_command(command).ok_or_else(|| format!("There's no Skill called /{}.", command.trim_start_matches('/')))?;
    if !skill.runs_on.across() {
        return Err(format!("/{} runs on one meeting: use it in that meeting's Ask box.", skill.command));
    }
    let words = words_of(&skill, text);
    Ok(SkillTurn { skill, words })
}

/// What the system prompt adds for a Skill (after Ask across meetings' own
/// instructions).
pub fn system_block(turn: &SkillTurn) -> String {
    let mut out = format!(
        "{}\nSkill: {} (/{})\nInstructions:\n{}",
        crate::ai::llm::SKILL_ACROSS_LEAD,
        turn.skill.name,
        turn.skill.command,
        turn.skill.instructions.trim()
    );
    if !turn.words.trim().is_empty() {
        out.push_str("\nThe user's own words with it are in their message.");
    }
    out
}

/// The search a Skill runs when it searches: the words typed with it, else
/// its own, else the latest meeting.
pub fn query_of(turn: &SkillTurn) -> String {
    if !turn.words.trim().is_empty() {
        turn.words.clone()
    } else if !turn.skill.query.is_empty() {
        turn.skill.query.clone()
    } else {
        "my last meeting".to_string()
    }
}

/// Most passages a Skill's gathering hands the model (a week of meetings,
/// a long task list), each short.
const GATHER_PASSAGES: usize = 14;

/// **The passages a Skill reads across the meetings**, numbered for the
/// model to cite: its own gathering (the open tasks, the latest meeting's
/// talk, a week of meetings) or a search ([`query_of`]).
pub async fn find(turn: &SkillTurn) -> Found {
    let found = match turn.skill.gather.as_str() {
        "tasks" => tasks_found(&crate::notes::all(), &crate::notes::task_store::snapshot()),
        "you" => latest_you(&crate::notes::all()),
        "week" => {
            let now = crate::ai::eval::now().with_timezone(&chrono::Local);
            week(&crate::notes::all(), &query_of(turn), &now)
        }
        _ => ask::find(&query_of(turn), ask::ASK_PASSAGES).await,
    };
    tracing::info!(passages = found.passages.len(), gather = %turn.skill.gather, builtin = turn.skill.builtin, "Skill: passages for a run across the meetings");
    found
}

/// The chip over the answer: "Ran /todos · 5 passages".
pub fn chip(turn: &SkillTurn, found: &Found) -> String {
    let n = found.passages.len();
    let what = match (turn.skill.gather.as_str(), n) {
        (_, 0) => "nothing found".to_string(),
        ("tasks", 1) => "1 open task".to_string(),
        ("tasks", n) => format!("{n} open tasks"),
        (_, 1) => "1 passage".to_string(),
        (_, n) => format!("{n} passages"),
    };
    let when = match (&found.latest, &found.range) {
        (Some(t), _) => format!(" in \"{t}\""),
        (None, Some(r)) => format!(" from {}", r.label.split(" (").next().unwrap_or(&r.label)),
        _ => String::new(),
    };
    format!("Ran /{} · {what}{when}", turn.skill.command)
}

/// Meetings with talk, newest first.
fn meetings_by_time(notes: &[crate::notes::Note]) -> Vec<&crate::notes::Note> {
    let mut m: Vec<&crate::notes::Note> = notes.iter().filter(|n| chunks::note_kind(n) == "meeting" && n.transcript.iter().any(|s| s.is_talk())).collect();
    m.sort_by_key(|n| std::cmp::Reverse(chunks::note_time(n)));
    m
}

/// "you": the latest meeting's talk, the passages where the person talks
/// most first (at most [`GATHER_PASSAGES`]), back in the order said.
fn latest_you(notes: &[crate::notes::Note]) -> Found {
    let Some(note) = meetings_by_time(notes).into_iter().next() else { return Found::default() };
    let talk: Vec<chunks::Chunk> = chunks::chunk_note(note).into_iter().filter(|c| c.field == Field::Transcript).collect();
    let mine = |c: &chunks::Chunk| c.lines.iter().filter(|l| l.speaker == "You").map(|l| l.text.split_whitespace().count()).sum::<usize>();
    let mut order: Vec<usize> = (0..talk.len()).collect();
    order.sort_by_key(|&i| (std::cmp::Reverse(mine(&talk[i])), i));
    let mut keep: Vec<usize> = order.into_iter().take(GATHER_PASSAGES).collect();
    keep.sort_unstable();
    let chosen: Vec<chunks::Chunk> = keep.into_iter().map(|i| talk[i].clone()).collect();
    Found {
        passages: crate::meetings::meeting_summary::chips::numbered(note, &chosen),
        latest: Some(chunks::note_title(note)),
        ..Default::default()
    }
}

/// "week": each meeting in the week `query` names (this one by default), in
/// the order they happened — its summary's first passages, else the start
/// of its talk —, at most [`GATHER_PASSAGES`] in all.
fn week<Tz: chrono::TimeZone>(notes: &[crate::notes::Note], query: &str, now: &chrono::DateTime<Tz>) -> Found
where
    Tz::Offset: std::fmt::Display,
{
    let dated = crate::search::dates::parse(query, now);
    let range = dated.range.or_else(|| crate::search::dates::parse("this week", now).range);
    let mut meetings: Vec<&crate::notes::Note> =
        meetings_by_time(notes).into_iter().filter(|n| range.as_ref().is_none_or(|r| r.holds(chunks::note_time(n)))).collect();
    meetings.reverse();
    let per = (GATHER_PASSAGES / meetings.len().max(1)).clamp(1, 3);
    let mut passages: Vec<Passage> = Vec::new();
    for n in meetings {
        let all = chunks::chunk_note(n);
        let summary: Vec<chunks::Chunk> = all.iter().filter(|c| c.field == Field::Summary).take(per).cloned().collect();
        let picked = if summary.is_empty() { all.into_iter().filter(|c| c.field == Field::Transcript).take(per).collect() } else { summary };
        for mut p in crate::meetings::meeting_summary::chips::numbered(n, &picked) {
            if passages.len() >= GATHER_PASSAGES {
                break;
            }
            p.n = passages.len() + 1;
            passages.push(p);
        }
    }
    Found { passages, range, ..Default::default() }
}

/// "tasks": every open task — the person's own first ("You", and the ones
/// they added), then the ones nobody took on —, newest meeting first, each a
/// passage that opens the moment it was said (else the summary's line).
/// Tasks added with no meeting have no moment to open: they're left out of
/// the numbered passages.
pub fn tasks_found(notes: &[crate::notes::Note], store: &crate::notes::task_store::TaskStore) -> Found {
    let listed = crate::notes::tasks::list_all(notes, store);
    let mut mine: Vec<Passage> = Vec::new();
    let mut nobody: Vec<Passage> = Vec::new();
    for nt in listed.iter().filter(|nt| nt.note_id != 0) {
        let Some(note) = notes.iter().find(|n| n.id == nt.note_id) else { continue };
        let start = note.transcript.iter().find(|s| s.is_talk()).map(|s| s.ts).unwrap_or(0);
        for t in nt.tasks.iter().filter(|t| !t.task.done && !t.archived) {
            let owner = if t.own.is_some() && t.task.owner.is_empty() { "You".to_string() } else { t.task.owner.clone() };
            let yours = owner == "You" || owner.eq_ignore_ascii_case("me");
            let unowned = owner.is_empty() || owner == crate::meetings::meeting_summary::UNASSIGNED;
            if !yours && !unowned {
                continue;
            }
            let due = if t.task.due.is_empty() { String::new() } else { format!(" (due: {})", t.task.due) };
            let who = if yours { "You" } else { crate::meetings::meeting_summary::UNASSIGNED };
            let (field, line, ts, at) = match &t.at {
                Some(m) => (Field::Transcript, m.index, m.ts, Some(ask::clock(m.ts.saturating_sub(start)))),
                None => (Field::Summary, t.task.line, chunks::note_time(note), None),
            };
            let p = Passage {
                n: 0,
                note: note.id,
                title: chunks::note_title(note),
                kind: "meeting",
                field,
                line,
                lines: vec![line],
                ts,
                when: format_when(chunks::note_time(note)),
                at,
                speaker: String::new(),
                text: format!("Open task \u{2014} {who}: {}{due}", t.task.task),
                score: 0.0,
                keyword: 0.0,
                similarity: None,
            };
            if yours {
                mine.push(p);
            } else {
                nobody.push(p);
            }
        }
    }
    let mut passages: Vec<Passage> = mine.into_iter().chain(nobody).take(GATHER_PASSAGES * 2).collect();
    for (i, p) in passages.iter_mut().enumerate() {
        p.n = i + 1;
    }
    Found { passages, ..Default::default() }
}

fn format_when(ts: u64) -> String {
    use chrono::TimeZone;
    chrono::Local.timestamp_opt(ts as i64, 0).single().map(|t| t.format("%a %-d %b %Y, %H:%M").to_string()).unwrap_or_default()
}

// ---- commands ----------------------------------------------------------------------------------

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Every Skill: Yap's own, then the person's.
#[tauri::command]
pub fn skills_list() -> Vec<Skill> {
    all()
}

/// Save one of the person's Skills (new, a built-in's copy, or a change);
/// every window's Skills follow (`yap-skills-changed`).
#[tauri::command]
pub fn skill_save(app: tauri::AppHandle, skill: Skill) -> Result<Skill, String> {
    use tauri::Emitter;
    let saved = save_skill(skill, now_ms())?;
    tracing::info!(id = %saved.id, copy = !saved.based_on.is_empty(), "Skill saved");
    let _ = app.emit("yap-skills-changed", ());
    SAVER.check().map_err(|e| e.to_string())?;
    Ok(saved)
}

/// Delete one of the person's Skills.
#[tauri::command]
pub fn skill_delete(app: tauri::AppHandle, id: String) -> Result<(), String> {
    use tauri::Emitter;
    delete_skill(&id)?;
    tracing::info!(id = %id, "Skill deleted");
    let _ = app.emit("yap-skills-changed", ());
    SAVER.check().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::{Note, TranscriptSegment};

    fn own(name: &str, command: &str) -> Skill {
        Skill { name: name.into(), command: command.into(), instructions: "List the numbers that were said.".into(), ..Default::default() }
    }

    #[test]
    fn the_builtins_have_good_commands_and_three_are_one_tap_prompts() {
        let b = builtins();
        let commands: Vec<&str> = b.iter().map(|s| s.command.as_str()).collect();
        assert_eq!(commands, ["followup-email", "decisions", "risks", "coach", "todos", "weekly-recap"]);
        for s in &b {
            assert!(s.builtin && s.id == format!("builtin:{}", s.command));
            assert!(!s.instructions.is_empty() && s.instructions.len() < INSTRUCTIONS_MAX);
        }
        let chips: Vec<&str> = b.iter().filter(|s| !s.chip.is_empty()).map(|s| s.chip.as_str()).collect();
        assert_eq!(chips, ["Coach me", "List my open to-dos", "Write a weekly recap"]);
        assert!(b.iter().filter(|s| !s.chip.is_empty()).all(|s| s.runs_on.across() && !s.question.is_empty()));
        assert!(!by_command("followup-email").unwrap().runs_on.across());
        assert_eq!(by_command("/Decisions").unwrap().name, "Decisions");
    }

    #[test]
    fn your_own_skills_are_checked_saved_copied_and_deleted() {
        // A command: lowercase words with single hyphens, not a built-in's.
        assert!(command_problem("", None).is_some());
        assert!(command_problem("Budget", None).is_some());
        assert!(command_problem("budget--check", None).is_some());
        assert!(command_problem("budget check", None).is_some());
        assert!(command_problem("decisions", None).unwrap().contains("one of Yap's own"));
        assert_eq!(command_problem("/budget-check", None), None);

        let saved = save_skill(own("  Budget   check ", "/budget-check"), 1_000).unwrap();
        assert_eq!((saved.name.as_str(), saved.command.as_str()), ("Budget check", "budget-check"));
        assert!(saved.id.starts_with('s') && !saved.builtin);
        assert_eq!(all().len(), builtins().len() + 1);
        // Another with the same command: refused; the same one changed: fine.
        assert!(save_skill(own("Again", "budget-check"), 2_000).unwrap_err().contains("already have"));
        let changed = save_skill(Skill { name: "Budget numbers".into(), ..saved.clone() }, 3_000).unwrap();
        assert_eq!((changed.id.as_str(), changed.created_ms, changed.updated_ms), (saved.id.as_str(), 1_000, 3_000));
        assert_eq!(all().len(), builtins().len() + 1);
        // A built-in copied as their own: a new command, what it's based on kept.
        let mut copy = by_command("decisions").unwrap();
        copy.command = "my-decisions".into();
        copy.based_on = "decisions".into();
        copy.instructions.push_str(" Add the date of each.");
        let copy = save_skill(copy, 4_000).unwrap();
        assert!(!copy.builtin && copy.id != "builtin:decisions" && copy.based_on == "decisions");
        assert_eq!(by_command("my-decisions").unwrap().id, copy.id);
        // What only a built-in carries never comes with a copy.
        let mut coach = by_command("coach").unwrap();
        coach.command = "my-coach".into();
        let c = save_skill(coach, 5_000).unwrap();
        assert!(c.gather.is_empty() && c.chip.is_empty() && c.question.is_empty());
        // Empty or over-long: refused, said plainly.
        assert!(save_skill(Skill { instructions: " ".into(), ..own("X", "x-one") }, 6_000).is_err());
        assert!(save_skill(Skill { instructions: "a".repeat(INSTRUCTIONS_MAX + 1), ..own("X", "x-two") }, 6_000).is_err());
        assert!(save_skill(own("", "x-three"), 6_000).is_err());
        // Deleted: gone; a built-in: never.
        delete_skill(&saved.id).unwrap();
        assert!(by_command("budget-check").is_none());
        assert!(delete_skill("builtin:decisions").is_err());
        assert!(delete_skill(&saved.id).is_err());
    }

    #[test]
    fn an_older_yaps_view_of_skills_json() {
        // skills.json is Yap's alone; an older Yap never opens it. A newer
        // field this one doesn't know is simply ignored when it reads.
        let v = serde_json::json!({ "skills": [{ "id": "s1", "name": "Numbers", "command": "numbers", "instructions": "List them.", "runsOn": "meeting", "colour": "teal" }] });
        let store: SkillStore = serde_json::from_value(v).unwrap();
        assert_eq!(store.skills[0].runs_on, RunsOn::Meeting);
    }

    #[test]
    fn a_message_runs_a_skill_with_its_words() {
        assert_eq!(split_command("/decisions last week"), Some(("decisions".into(), "last week".into())));
        assert_eq!(split_command("  /Risks"), Some(("risks".into(), String::new())));
        assert_eq!(split_command("what about /risks"), None);
        assert_eq!(split_command("/ hello"), None);
        let coach = by_command("coach").unwrap();
        assert_eq!(words_of(&coach, "/coach the budget review"), "the budget review");
        assert_eq!(words_of(&coach, &coach.question), "");
        let t = turn_for("coach", "/coach").unwrap();
        assert_eq!(query_of(&t), "my last meeting");
        assert!(system_block(&t).contains("Skill: Coach me (/coach)\nInstructions:\nCoach the user"));
        assert!(turn_for("followup-email", "/followup-email").unwrap_err().contains("runs on one meeting"));
        assert!(turn_for("nope", "/nope").is_err());
    }

    fn seg(source: &str, text: &str, ts: u64) -> TranscriptSegment {
        TranscriptSegment { source: source.into(), text: text.into(), ts, ..Default::default() }
    }

    fn meeting(id: u64, title: &str, ts: u64, summary: &str, talk: &[(&str, &str)]) -> Note {
        let mut n: Note = serde_json::from_value(serde_json::json!({
            "id": id, "title": title, "content": "", "createdTs": ts, "updatedTs": ts, "noteType": "meeting", "enhancedContent": summary,
        }))
        .unwrap();
        n.transcript = talk.iter().enumerate().map(|(i, (s, t))| seg(s, t, ts + i as u64 * 30)).collect();
        n
    }

    #[test]
    fn the_open_to_dos_are_yours_and_nobodys_open_ones_each_at_its_moment() {
        let a = meeting(
            1,
            "Supplier call",
            1_000,
            "## Action plan\n### You\n- [ ] Draft the finance request (due: Wednesday)\n- [x] Send the slides\n### Priya\n- [ ] Check the totals\n\n## Unassigned\n- [ ] Book the room",
            &[("you", "I will draft the finance request by Wednesday."), ("them", "And someone should book the room.")],
        );
        let store = crate::notes::task_store::TaskStore::default();
        let found = tasks_found(&[a], &store);
        let texts: Vec<&str> = found.passages.iter().map(|p| p.text.as_str()).collect();
        assert_eq!(texts, ["Open task \u{2014} You: Draft the finance request (due: Wednesday)", "Open task \u{2014} Unassigned: Book the room"]);
        assert_eq!(found.passages.iter().map(|p| p.n).collect::<Vec<_>>(), [1, 2]);
        // The first opens the line it was said at.
        assert_eq!((found.passages[0].field, found.passages[0].line), (Field::Transcript, 0));
    }

    #[test]
    fn coach_me_reads_the_latest_meeting_where_you_talk() {
        let old = meeting(1, "Old sync", 1_000, "", &[("you", "Old words from long ago in another meeting entirely.")]);
        let new = meeting(2, "Budget review", 9_000, "", &[("you", "I think we should cut the second shift, and I'll write it up."), ("them", "Fine by me.")]);
        let found = latest_you(&[old, new]);
        assert_eq!(found.latest.as_deref(), Some("Budget review"));
        assert!(found.passages.iter().all(|p| p.note == 2));
        assert!(found.passages[0].text.contains("You: I think we should cut the second shift"));
    }

    #[test]
    fn a_weekly_recap_reads_each_meeting_of_the_week_in_order() {
        use chrono::TimeZone;
        let now = chrono::Utc.with_ymd_and_hms(2026, 10, 29, 12, 0, 0).unwrap(); // a Thursday
        let mon = chrono::Utc.with_ymd_and_hms(2026, 10, 26, 10, 0, 0).unwrap().timestamp() as u64;
        let wed = chrono::Utc.with_ymd_and_hms(2026, 10, 28, 10, 0, 0).unwrap().timestamp() as u64;
        let last_week = chrono::Utc.with_ymd_and_hms(2026, 10, 20, 10, 0, 0).unwrap().timestamp() as u64;
        let notes = [
            meeting(1, "Earlier", last_week, "## Recap\nLast week's.", &[("you", "Last week we talked about the old plan.")]),
            meeting(2, "Wednesday sync", wed, "## Recap\nThe launch moved to May.", &[("you", "The launch moves to May.")]),
            meeting(3, "Monday kick-off", mon, "", &[("you", "Kick-off: we agreed the scope for the quarter.")]),
        ];
        let found = week(&notes, "this week", &now);
        let titles: Vec<&str> = found.passages.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(titles, ["Monday kick-off", "Wednesday sync"]);
        assert_eq!(found.passages[1].field, Field::Summary);
        assert!(found.range.is_some());
        assert!(chip(&turn_for("weekly-recap", "/weekly-recap").unwrap(), &found).starts_with("Ran /weekly-recap · 2 passages from this week"));
    }
}

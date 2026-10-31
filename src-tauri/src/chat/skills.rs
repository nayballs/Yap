//! **Skills** (2026-10-29, Wispr Flow's Skills): saved instructions the
//! person runs by typing `/name` — in a meeting's Ask box (the notepad's
//! sheet, the Notes view's Ask bar on a meeting note), on that meeting
//! (`meeting_assist::meeting_skill`), or in Chat and Ask across meetings, on
//! the meetings (`chat_send`'s `skill`: the turn is asked across the
//! meetings, its passages found for the Skill — [`find`]). Every answer
//! cites the passages it comes from, as Ask across meetings' do.
//!
//! - **Built in** ([`builtins`]): /followup-email, /decisions, /risks,
//!   /coach, /todos, /weekly-recap, and since 31 Oct 2026 /summary-for,
//!   /decision-log, /open-questions, /tickets, /prd, /standup, /prep,
//!   /one-to-one, /project-update — in code, never stored; each can be
//!   copied as the person's own and changed (`basedOn`). Three are Chat's
//!   and Ask across meetings' one-tap prompts too (`chip`): Coach me, List
//!   my open to-dos, Write a weekly recap. Each reads its own passages
//!   across the meetings ([`find`], `gather`); /tickets and /prd run in
//!   Chat write a Chat document ([`document_note`]); one with nothing to
//!   run on is asked for it by Yap, no model called ([`ask_first`]).
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

use std::cmp::Reverse;
use std::collections::HashSet;

use chrono::{DateTime, TimeZone};
use serde::{Deserialize, Serialize};

use crate::calendar::{brief, Agenda, Event};
use crate::meetings::meeting_summary::chips;
use crate::notes::task_store::TaskStore;
use crate::notes::Note;
use crate::search::ask::{self, Found, Passage};
use crate::search::chunks::{self, Chunk, Field};
use crate::search::dates::Range;
use crate::search::rank;

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
    /// (each meeting of a week: this one, or the one the words name),
    /// "latest" (the meeting the words name, else the latest, read as a
    /// Skill on one meeting reads it), "meeting" (the same, or a search for
    /// words that name none), "decisions" (each meeting's decisions over a
    /// stretch, oldest first), "questions" (where questions were asked, and
    /// the summaries' open questions), "topic" (a search for the words,
    /// oldest first), "standup" (your own words since the last working day,
    /// your open tasks, the rest of today's calendar), "prep" (the next
    /// meeting on the calendar, the earlier ones with its people and their
    /// open tasks), "person" (the same for a one-to-one with someone).
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
    /// Built-ins that make something to keep (/tickets, /prd): run in Chat
    /// by a model with tools, the answer is a Chat document
    /// (`chat/documents.rs`) — this says how its sections go.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub document: String,
    /// Built-ins that need words to run on (/project-update: which project):
    /// run without any, Yap asks this itself, no model called (and the `/`
    /// menu's Enter puts the command in the box for them, as Tab does).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub asks: String,
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

/// The instructions of the built-ins added on 31 Oct 2026 (Wispr Flow's
/// Windows build has 24): short and firm, for the on-device 4B as for a
/// cloud model, the citing left to the lead each run gets
/// (`llm::SKILL_MEETING_PROMPT` on one meeting, `SKILL_ACROSS_LEAD` and Ask
/// across meetings' lines in Chat). Measured with `yap.exe eval --chips` and
/// the `skills` scenarios (docs/model-evals.md, "More Skills").
const SUMMARY_FOR: &str = "Write a catch-up on the meeting for a reader who wasn't there; the user's words with the command may say who the reader is and what they care about. Open with one sentence on what the meeting was for. Then \"- \" bullets: what was decided, what changed, and anything the reader or their team now has to do. Name who does what only where the passages name them (\"Them\" is no one's name), with dates only as said. Spell out anything the reader would need context for. No greeting or sign-off. Under 180 words.";
const DECISION_LOG: &str = "Make a log of the decisions in the passages, oldest first: one \"- \" bullet each, starting with the meeting's day and name in bold (**Tue 27 Oct, Supplier call:**), then the decision, who made or agreed it, and why when a reason was given. Only decisions that were made, never ones only discussed; a decision repeated in a later meeting goes in once. If the user's words name a topic, only the decisions about it.";
const OPEN_QUESTIONS: &str = "List only the questions that never got an answer: met with \"I don't know\", put off (\"let's come back to that\") or left hanging. One \"- \" bullet each: the question (shortened), who asked it (\"You\", a name, or \"Them\" for the other side), and who should answer it when that was said. Never list a question that was answered, nor its answer. If every question was answered, say so in one line.";
const TICKETS: &str = "Draft a ticket for each piece of work that was agreed (its tasks, and the fixes or features it asked for), at most 8; never one for work that was put off or turned down. Each: a \"### \" heading with a short title starting with a verb, then **What:** one or two sentences, **Why:** the reason given, **Owner:** who took it on, as the summary or the meeting names them (\"Unassigned\" if nobody did), **Due:** only when a deadline was said (else no Due line), and **Done when:** what finished looks like for that ticket. Never invent work, owners or dates.";
const PRD: &str = "Write a short product requirements draft of what was discussed, in these \"## \" sections: Problem (who has it and why it matters), Goals, Requirements (\"- \" bullets, the must-haves first), Out of scope, Open questions. Only what was said: a section nobody spoke to reads \"Not discussed.\" Under 350 words.";
const STANDUP: &str = "Write the user's (\"You\") stand-up update in three parts: **Yesterday:** what they said they did or finished, **Today:** what they said they'd do next and their open tasks, the soonest due first, **Blockers:** anything they said was stuck or waiting on someone, else \"None\". One to three \"- \" bullets each, in the first person (\"I…\"), only from their own words and tasks.";
const PREP: &str = "Prepare the user for the meeting the passages are about: first one line with its name, when it is and who's coming. Then **Last time:** what was decided or left open in their earlier meetings with these people, **Open items:** the user's and the attendees' open tasks, with owner and deadline, and **To raise:** two or three questions or points worth bringing up. \"- \" bullets, specific. If there are no earlier meetings, say it's the first. Under 200 words.";
const ONE_TO_ONE: &str = "Write talking points for the user's one-to-one with the person the passages are about: **Follow up:** what each of them said they'd do that's still open, **Topics:** two to four things from their recent meetings worth talking through, each with why, and **Questions to ask:** two or three. \"- \" bullets, specific. Under 200 words.";
const PROJECT_UPDATE: &str = "Write a status update on the project the user named, from the passages: a first line saying where it stands now, then **Done:**, **Next:** and **Risks and blockers:**, a few \"- \" bullets each with who and when as said (\"None\" for an empty part). The latest passages say where it stands. Leave out what isn't about this project. Under 200 words.";

/// /one-to-one with no name and no one-to-one coming up on the calendar.
const PERSON_ASKS: &str = "Who's it with? Add their name after the command, like /one-to-one Priya. (There's no one-to-one coming up on your calendar.)";
/// /prep with nothing named and nothing coming up on the calendar…
const PREP_NOTHING_COMING: &str = "There's no meeting coming up on your calendar to prepare for. Name one after the command, like /prep supplier call.";
/// …or no calendar connected.
const PREP_NO_CALENDAR: &str = "Yap can't see your calendar, so name the meeting after the command, like /prep supplier call. (Connect your calendar in Settings → Connectors, and /prep finds the next one itself.)";

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
        Skill {
            gather: "latest".into(),
            ..builtin("summary-for", "Summary for someone", "A catch-up for someone who wasn't there", RunsOn::Both, SUMMARY_FOR)
        },
        builtin(
            "decisions",
            "Decisions",
            "Every decision, who made it and why",
            RunsOn::Both,
            "List every decision that was made, one \"- \" bullet each: the decision, who made or agreed it, and why when a reason was given. Leave out anything that was only discussed. If nothing was decided, say so.",
        ),
        Skill {
            gather: "decisions".into(),
            query: "the last 4 weeks".into(),
            ..builtin("decision-log", "Decision log", "Decisions across meetings, with dates and why", RunsOn::Meetings, DECISION_LOG)
        },
        builtin(
            "risks",
            "Risks",
            "Risks, blockers and open questions",
            RunsOn::Both,
            "List the risks, blockers and open questions that came up, one \"- \" bullet each, the most serious first: what it is, who raised it, and what was agreed about it, if anything. Leave out anything that was settled.",
        ),
        Skill {
            gather: "questions".into(),
            query: "the last 7 days".into(),
            ..builtin("open-questions", "Open questions", "What was asked and never answered", RunsOn::Both, OPEN_QUESTIONS)
        },
        Skill {
            gather: "meeting".into(),
            document: "one section, \"Tickets\", each ticket a \"### \" heading in it".into(),
            ..builtin("tickets", "Draft tickets", "Tickets drafted from the work agreed", RunsOn::Both, TICKETS)
        },
        Skill {
            gather: "meeting".into(),
            document: "the sections Problem, Goals, Requirements, Out of scope and Open questions".into(),
            ..builtin("prd", "Product requirements", "A short requirements draft from a discussion", RunsOn::Both, PRD)
        },
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
        Skill { gather: "standup".into(), ..builtin("standup", "Stand-up", "Your yesterday, today and blockers", RunsOn::Meetings, STANDUP) },
        Skill { gather: "prep".into(), ..builtin("prep", "Prepare me", "Get ready for your next meeting", RunsOn::Meetings, PREP) },
        Skill { gather: "person".into(), ..builtin("one-to-one", "One-to-one", "Talking points for a one-to-one", RunsOn::Meetings, ONE_TO_ONE) },
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
        Skill {
            gather: "topic".into(),
            asks: "Which project? Add its name after the command, like /project-update website relaunch.".into(),
            ..builtin("project-update", "Project update", "Where a project stands, from your meetings", RunsOn::Meetings, PROJECT_UPDATE)
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

/// skills.json as it loads (`saving::Saver::load`): set aside if it isn't
/// Skills; one Yap couldn't read is never written over until it reads (read
/// again in [`with_store`], the Skills made meanwhile kept).
#[cfg(not(test))]
fn load() -> SkillStore {
    SAVER.load(&path())
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
    let store = guard.get_or_insert_with(load);
    if SAVER.any_unread() && SAVER.refresh(&path(), store) {
        save(store);
    }
    f(store)
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
    s.document.clear();
    s.asks.clear();
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

/// What a Skill whose Chat run makes something to keep (/tickets, /prd)
/// adds to the question's context, for a model with tools: the document
/// now (`chat/documents.rs`), its passages cited in it (Yap puts their
/// meetings' names in their place: `documents::name_citations`), one short
/// line in the chat. `None`: it answers in the chat.
pub fn document_note(turn: &SkillTurn) -> Option<String> {
    let how = turn.skill.document.trim();
    (!how.is_empty()).then(|| {
        format!(
            "Write this as a document now, not in the chat: create_document (a title saying what it is and which meeting it's from, a one-line summary, {how}), then write_section for each, in order, as the Skill's instructions say. Cite the passages in it as [1] after each point: Yap puts the meeting's name in its place. Then answer in one short sentence: \"It's open on the right.\""
        )
    })
}

/// What a Skill on one meeting is handed besides the meeting's passages
/// (`meeting_assist::skill_input_for` in its Ask box; a Chat run reading one
/// meeting, "latest" / "meeting", gets it too): Yap's own list of what the
/// Skill must go through, from the meeting. `None`: the passages alone.
/// Pure.
///
/// - **/open-questions** ("questions"): each question asked, in order, with
///   what was said next, and the ones the summary left open. Told only to
///   find the unanswered ones, the on-device 4B listed the first and stopped
///   (3 runs of 3), and with the questions alone still took "Let's come
///   back to that." for an answer (31 Oct 2026).
/// - **/tickets**: the open tasks Yap's summary lists, with their owners
///   and deadlines — the work agreed. Without it the 4B made a ticket of the
///   dark mode the meeting put off, and gave a ticket the wrong owner.
pub fn meeting_aid(skill: &Skill, note: &Note) -> Option<String> {
    if skill.gather == "questions" {
        return questions_aid(note);
    }
    if skill.builtin && skill.command == "tickets" {
        let tasks: Vec<String> = crate::notes::tasks::parse(&note.enhanced_content)
            .into_iter()
            .filter(|t| !t.done)
            .take(8)
            .map(|t| {
                let owner = if t.owner.is_empty() { crate::meetings::meeting_summary::UNASSIGNED.to_string() } else { t.owner };
                let due = if t.due.is_empty() { String::new() } else { format!(" (due: {})", t.due) };
                format!("- {owner}: {}{due}", t.task)
            })
            .collect();
        return (!tasks.is_empty())
            .then(|| format!("The work Yap's summary says was agreed, with its owners (a ticket each; nothing the meeting put off or turned down):\n{}", tasks.join("\n")));
    }
    None
}

/// What /open-questions' list asks of the model, before it.
const QUESTIONS_HEAD: &str = "Decide for each one whether what was said next answers it (\"I don't know\", \"let's come back to that\" or a change of subject is no answer), and list every one it doesn't.";

/// An answer to a Skill as it's shown: /open-questions' "Q4: " — Yap's own
/// numbering of the questions it handed the model ([`QUESTIONS_HEAD`]),
/// which the on-device 4B copies onto its bullets, told not to or not (31
/// Oct 2026) — taken off. Other Skills: as written. Pure.
pub fn tidy_answer(skill: &Skill, reply: &str) -> String {
    static Q: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"(?m)^(\s*(?:[-*]\s+)?)Q\d{1,2}\s*[:.)]\s*").expect("static regex"));
    if skill.gather != "questions" {
        return reply.to_string();
    }
    Q.replace_all(reply, "$1").into_owned()
}

/// The questions asked in meeting `note`, in order (at most `max`), each with
/// what was said next, numbered from `from` ("Q3. Them: \"…\"").
fn question_lines(note: &Note, max: usize, from: usize) -> Vec<String> {
    // A line's last sentence, when it's the question ("Okay. Who drives it?").
    let question = |t: &str| {
        let t = t.trim();
        let body = t.trim_end_matches('?');
        let start = body.rfind(['.', '!', '?']).map_or(0, |i| i + 1);
        t[start..].trim().to_string()
    };
    let label = |s: &crate::notes::TranscriptSegment| crate::speakers::prompt_label(&note.speakers, s);
    let talk: Vec<&crate::notes::TranscriptSegment> = note.transcript.iter().filter(|s| s.is_talk() && !s.text.trim().is_empty()).collect();
    let mut out: Vec<String> = Vec::new();
    for (k, (i, s)) in talk.iter().enumerate().filter(|(_, s)| s.text.trim_end().ends_with('?')).take(max).enumerate() {
        let mut line = format!("Q{}. {}: \"{}\"", from + k, label(s), question(&s.text));
        match talk.get(i + 1) {
            Some(next) => {
                let words: Vec<&str> = next.text.split_whitespace().collect();
                let said = if words.len() > 24 { format!("{}…", words[..24].join(" ")) } else { words.join(" ") };
                line.push_str(&format!("\n  Said next: {}: \"{said}\"", label(next)));
            }
            None => line.push_str("\n  Said next: nothing (the meeting ended)"),
        }
        out.push(line);
    }
    out
}

/// /open-questions' aid on one meeting: each question asked, in order, and
/// what was said next; then the ones the summary left open.
fn questions_aid(note: &Note) -> Option<String> {
    let mut asked = question_lines(note, 12, 1);
    let n = asked.len();
    asked.extend(brief::open_questions(note).into_iter().take(4).enumerate().map(|(k, q)| format!("Q{}. (left open in Yap's summary) {}", n + k + 1, q.trim())));
    (!asked.is_empty()).then(|| format!("The questions asked in it, in order, each with what was said next. {QUESTIONS_HEAD}\n{}", asked.join("\n")))
}

/// **What Yap answers itself, before any model**, when a Skill has nothing
/// to run on: /project-update with no project named (its `asks`), /prep
/// with nothing named and nothing coming up on the calendar, /one-to-one
/// with nobody named and no one-to-one coming up. `None`: run it.
pub fn ask_first(turn: &SkillTurn) -> Option<String> {
    if !turn.words.trim().is_empty() {
        return None;
    }
    if !turn.skill.asks.is_empty() {
        return Some(turn.skill.asks.clone());
    }
    if !matches!(turn.skill.gather.as_str(), "prep" | "person") {
        return None;
    }
    asks_for(&turn.skill.gather, &crate::calendar::agenda(), crate::ai::eval::now().timestamp())
}

/// [`ask_first`]'s calendar half: what a "prep" or "person" run with no
/// words asks, given the calendar. Pure.
fn asks_for(gather: &str, agenda: &Agenda, now: i64) -> Option<String> {
    match gather {
        "prep" if next_event(agenda, now, "").is_none() => Some(if agenda.connected { PREP_NOTHING_COMING } else { PREP_NO_CALENDAR }.to_string()),
        "person" if next_one_to_one(agenda, now).is_none() => Some(PERSON_ASKS.to_string()),
        _ => None,
    }
}

/// Most passages a Skill's gathering hands the model (a week of meetings,
/// a long task list), each short.
const GATHER_PASSAGES: usize = 14;
/// What a run across the meetings reads of one meeting ("latest",
/// "meeting"): less than a run on it in its Ask box, as Chat's own prompt
/// and tools go with it (a request fits an 8k model).
const ACROSS_ONE: chips::SkillBudget = chips::SkillBudget { summary: 700, notes: 200, talk: 1_100 };
/// The open tasks a "prep", "person" or "standup" run reads, at most.
const GATHER_TASKS: usize = 6;

/// What a Skill's gathering comes to: its passages, or a search to run
/// (`ask::find`, which reads the meaning index).
#[derive(Debug)]
enum Gathered {
    Found(Found),
    /// `query`, at most `limit` passages; `about`: said to be about the
    /// words (`by_time`: oldest first).
    Search { query: String, limit: usize, about: bool, by_time: bool },
}

/// [`find`] given the notes, the tasks added, the calendar and the clock
/// (a search left to the caller). Pure.
fn gather<Tz: TimeZone>(turn: &SkillTurn, notes: &[Note], store: &TaskStore, agenda: &Agenda, now: &DateTime<Tz>) -> Gathered
where
    Tz::Offset: std::fmt::Display,
{
    let words = turn.words.trim();
    let search = |by_time: bool| Gathered::Search { query: words.to_string(), limit: GATHER_PASSAGES, about: true, by_time };
    Gathered::Found(match turn.skill.gather.as_str() {
        "tasks" => tasks_found(notes, store),
        "you" => latest_you(notes, words, now),
        "week" => week(notes, &query_of(turn), now),
        // Words that name no meeting: what the meetings say about them.
        "latest" | "meeting" => match one_meeting(notes, turn, now) {
            Some(f) => f,
            None => return search(false),
        },
        "decisions" => decision_log(notes, words, &turn.skill.query, now),
        "questions" => questions(notes, words, &turn.skill.query, now),
        "topic" => return search(true),
        "standup" => standup(notes, store, agenda, now),
        "prep" => prep(notes, store, agenda, words, now),
        "person" => one_to_one(notes, store, agenda, words, now),
        _ => return Gathered::Search { query: query_of(turn), limit: ask::ASK_PASSAGES, about: false, by_time: false },
    })
}

/// **The passages a Skill reads across the meetings**, numbered for the
/// model to cite: its own gathering ([`Skill::gather`]) or a search
/// ([`query_of`]). What they are goes before them (`Found::about`).
pub async fn find(turn: &SkillTurn) -> Found {
    let now = crate::ai::eval::now().with_timezone(&chrono::Local);
    let notes = crate::notes::all();
    let store = crate::notes::task_store::snapshot();
    let calendar = matches!(turn.skill.gather.as_str(), "standup" | "prep" | "person");
    let agenda = if calendar { crate::calendar::agenda() } else { Agenda::default() };
    let found = match gather(turn, &notes, &store, &agenda, &now) {
        Gathered::Found(f) => f,
        Gathered::Search { query, limit, about, by_time } => {
            let found = ask::find(&query, limit).await;
            if about {
                about_words(found, &query, by_time)
            } else {
                found
            }
        }
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
    // One meeting's passages (the one the words name): its title.
    let one = found.passages.first().filter(|p| p.event.is_none() && found.passages.iter().all(|q| q.note == p.note && q.event.is_none()));
    let when = match (&found.latest, &found.range, one) {
        (Some(t), _, _) => format!(" in \"{t}\""),
        (None, Some(r), _) => format!(" from {}", r.label.split(" (").next().unwrap_or(&r.label)),
        (None, None, Some(p)) => format!(" in \"{}\"", p.title),
        _ => String::new(),
    };
    format!("Ran /{} · {what}{when}", turn.skill.command)
}

// ---- the gatherings --------------------------------------------------------------------------

/// Meetings with talk, newest first.
fn meetings_by_time(notes: &[Note]) -> Vec<&Note> {
    let mut m: Vec<&Note> = notes.iter().filter(|n| chunks::note_kind(n) == "meeting" && n.transcript.iter().any(|s| s.is_talk())).collect();
    m.sort_by_key(|n| Reverse(chunks::note_time(n)));
    m
}

/// Lowercase words, single spaces, no punctuation ("Budget review!" →
/// "budget review").
fn plain_words(s: &str) -> String {
    s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

/// Words for the model's lines, its quotes kept apart.
fn quoted(s: &str) -> String {
    s.trim().replace('"', "'")
}

/// Passages numbered from 1, in the order given.
fn renumber(mut v: Vec<Passage>) -> Vec<Passage> {
    for (i, p) in v.iter_mut().enumerate() {
        p.n = i + 1;
    }
    v
}

/// "Tue 27 Oct" (in `now`'s zone).
fn day<Tz: TimeZone>(now: &DateTime<Tz>, ts: i64) -> String
where
    Tz::Offset: std::fmt::Display,
{
    now.timezone().timestamp_opt(ts, 0).single().map(|t| t.format("%a %-d %b").to_string()).unwrap_or_default()
}

/// "Tue 27 Oct, 10:00" (in `now`'s zone).
fn day_time<Tz: TimeZone>(now: &DateTime<Tz>, ts: i64) -> String
where
    Tz::Offset: std::fmt::Display,
{
    now.timezone().timestamp_opt(ts, 0).single().map(|t| t.format("%a %-d %b, %H:%M").to_string()).unwrap_or_default()
}

/// Midnight starting `d` in `now`'s zone (unix seconds).
fn midnight<Tz: TimeZone>(now: &DateTime<Tz>, d: chrono::NaiveDate) -> u64 {
    let naive = d.and_hms_opt(0, 0, 0).expect("midnight");
    now.timezone().from_local_datetime(&naive).earliest().map_or_else(|| naive.and_utc().timestamp(), |t| t.timestamp()).max(0) as u64
}

/// The newest of `meetings` whose title the words hold, as whole words
/// (any case, punctuation aside), within `range` when there is one.
fn by_title<'a>(meetings: &[&'a Note], words: &str, range: Option<&Range>) -> Option<&'a Note> {
    let said = format!(" {} ", plain_words(words));
    meetings.iter().copied().filter(|n| range.is_none_or(|r| r.holds(chunks::note_time(n)))).find(|n| {
        let title = plain_words(&n.title);
        title.chars().count() >= 3 && said.contains(&format!(" {title} "))
    })
}

/// The meeting a Skill's `words` name ("/coach the budget review", "/coach
/// yesterday"): the newest whose title they hold, as whole words, within
/// the dates they name; else the newest within those dates. `None`: they
/// name none ("/coach be blunt": the latest meeting, then). `meetings`:
/// newest first.
fn named<'a, Tz: TimeZone>(meetings: &[&'a Note], words: &str, now: &DateTime<Tz>) -> Option<&'a Note>
where
    Tz::Offset: std::fmt::Display,
{
    if words.trim().is_empty() {
        return None;
    }
    let dated = crate::search::dates::parse(words, now);
    by_title(meetings, words, dated.range.as_ref())
        .or_else(|| dated.range.as_ref().and_then(|r| meetings.iter().copied().find(|n| r.holds(chunks::note_time(n)))))
}

/// The transcript's passages where the person talks most, at most `max`,
/// back in the order said (`only_you`: none where they don't talk).
fn you_chunks(note: &Note, max: usize, only_you: bool) -> Vec<Chunk> {
    let talk: Vec<Chunk> = chunks::chunk_note(note).into_iter().filter(|c| c.field == Field::Transcript).collect();
    let mine = |c: &Chunk| c.lines.iter().filter(|l| l.speaker == "You").map(|l| l.text.split_whitespace().count()).sum::<usize>();
    let mut order: Vec<usize> = (0..talk.len()).filter(|&i| !only_you || mine(&talk[i]) > 0).collect();
    order.sort_by_key(|&i| (Reverse(mine(&talk[i])), i));
    let mut keep: Vec<usize> = order.into_iter().take(max).collect();
    keep.sort_unstable();
    keep.into_iter().map(|i| talk[i].clone()).collect()
}

/// "you": the talk of the meeting the words name ([`named`]), else the
/// latest meeting's — the passages where the person talks most first (at
/// most [`GATHER_PASSAGES`]), back in the order said. (Until 2026-10-29's
/// review it was always the latest: "/coach the budget review" coached
/// whatever meeting came last.) A meeting named is said to be that one, not
/// "their latest" (31 Oct 2026).
fn latest_you<Tz: TimeZone>(notes: &[Note], words: &str, now: &DateTime<Tz>) -> Found
where
    Tz::Offset: std::fmt::Display,
{
    let meetings = meetings_by_time(notes);
    let latest = meetings.first().copied();
    let picked = named(&meetings, words, now);
    let Some(note) = picked.or(latest) else { return Found::default() };
    let passages = chips::numbered(note, &you_chunks(note, GATHER_PASSAGES, false));
    let title = chunks::note_title(note);
    if picked.is_some_and(|p| latest.is_none_or(|l| l.id != p.id)) {
        let about = format!("The passages are from the meeting \"{}\" ({}): the parts where the user talks most.", quoted(&title), day(now, chunks::note_time(note) as i64));
        return Found { passages, about: Some(about), ..Default::default() };
    }
    Found { passages, latest: Some(title), ..Default::default() }
}

/// "week": each meeting in the week `query` names (this one by default), in
/// the order they happened — its summary's first passages, else the start
/// of its talk —, at most [`GATHER_PASSAGES`] in all.
fn week<Tz: TimeZone>(notes: &[Note], query: &str, now: &DateTime<Tz>) -> Found
where
    Tz::Offset: std::fmt::Display,
{
    let dated = crate::search::dates::parse(query, now);
    let range = dated.range.or_else(|| crate::search::dates::parse("this week", now).range);
    let mut meetings: Vec<&Note> = meetings_by_time(notes).into_iter().filter(|n| range.as_ref().is_none_or(|r| r.holds(chunks::note_time(n)))).collect();
    meetings.reverse();
    let per = (GATHER_PASSAGES / meetings.len().max(1)).clamp(1, 3);
    let mut passages: Vec<Passage> = Vec::new();
    for n in meetings {
        let all = chunks::chunk_note(n);
        let summary: Vec<Chunk> = all.iter().filter(|c| c.field == Field::Summary).take(per).cloned().collect();
        let picked = if summary.is_empty() { all.into_iter().filter(|c| c.field == Field::Transcript).take(per).collect() } else { summary };
        for mut p in chips::numbered(n, &picked) {
            if passages.len() >= GATHER_PASSAGES {
                break;
            }
            p.n = passages.len() + 1;
            passages.push(p);
        }
    }
    Found { passages, range, ..Default::default() }
}

/// "latest" / "meeting": one meeting, read as a Skill on it in its Ask box
/// reads it ([`ACROSS_ONE`], a little less): the meeting the words name, else
/// the latest — but "meeting" with words that name none: `None` (a search
/// for them instead). "latest"'s words say who it's for, never which
/// meeting, unless they name one.
fn one_meeting<Tz: TimeZone>(notes: &[Note], turn: &SkillTurn, now: &DateTime<Tz>) -> Option<Found>
where
    Tz::Offset: std::fmt::Display,
{
    let words = turn.words.trim();
    let meetings = meetings_by_time(notes);
    let picked = named(&meetings, words, now);
    if picked.is_none() && !words.is_empty() && turn.skill.gather == "meeting" {
        return None;
    }
    let Some(note) = picked.or_else(|| meetings.first().copied()) else { return Some(Found::default()) };
    let request = format!("{} {words}", turn.skill.instructions);
    let Some(cited) = chips::skill_input_within(note, &request, ACROSS_ONE) else { return Some(Found::default()) };
    let title = chunks::note_title(note);
    let with = if note.participants.is_empty() { String::new() } else { format!(", with {}", note.participants.join(", ")) };
    let which = if picked.is_some() { "the meeting" } else { "their latest meeting," };
    let mut about = format!("The passages are from {which} \"{}\" ({}{with}).", quoted(&title), day(now, chunks::note_time(note) as i64));
    // What the Skill goes through on it, as in the meeting's own Ask box.
    if let Some(aid) = meeting_aid(&turn.skill, note) {
        about.push_str("\n\n");
        about.push_str(&aid);
    }
    Some(Found { passages: cited.passages, about: Some(about), ..Default::default() })
}

/// A search for `words` ("topic"; "meeting" for words that name no
/// meeting), said so; `by_time`: the passages oldest first (a project's
/// story, where it stands last).
fn about_words(mut found: Found, words: &str, by_time: bool) -> Found {
    if by_time {
        found.passages.sort_by_key(|p| (p.ts, p.n));
        found.passages = renumber(std::mem::take(&mut found.passages));
    }
    if !words.is_empty() {
        found.about = Some(format!("The passages are what their meetings and notes say about \"{}\"{}.", quoted(words), if by_time { ", oldest first" } else { "" }));
    }
    found
}

/// A passage at transcript line `index` of `n`, in Yap's own words (the
/// decisions it noted while recording).
fn talk_passage(n: &Note, index: usize, text: String) -> Passage {
    let start = n.transcript.iter().find(|s| s.is_talk()).map(|s| s.ts).unwrap_or(0);
    let ts = n.transcript.get(index).map(|s| s.ts).unwrap_or_else(|| chunks::note_time(n));
    Passage {
        n: 0,
        note: n.id,
        title: chunks::note_title(n),
        kind: "meeting",
        field: Field::Transcript,
        line: index,
        lines: vec![index],
        ts,
        when: format_when(ts),
        at: Some(ask::clock(ts.saturating_sub(start))),
        speaker: String::new(),
        text,
        score: 0.0,
        keyword: 0.0,
        similarity: None,
        event: None,
    }
}

/// "decisions" (/decision-log): each meeting's decisions over the dates the
/// words name (else `default`: the last 4 weeks), oldest first — its
/// summary's Decisions passages, else the ones Yap noted while it recorded —
/// those about the rest of the words' topic when they name one; the newest
/// [`GATHER_PASSAGES`] when there are more.
fn decision_log<Tz: TimeZone>(notes: &[Note], words: &str, default: &str, now: &DateTime<Tz>) -> Found
where
    Tz::Offset: std::fmt::Display,
{
    let dated = crate::search::dates::parse(words, now);
    let range = dated.range.clone().or_else(|| crate::search::dates::parse(default, now).range);
    let topic: HashSet<String> = rank::terms(&dated.rest).into_iter().collect();
    let about_it = |text: &str, title: &str| topic.is_empty() || rank::terms(&format!("{title} {text}")).iter().any(|t| topic.contains(t));
    let mut meetings: Vec<&Note> = meetings_by_time(notes).into_iter().filter(|n| range.as_ref().is_none_or(|r| r.holds(chunks::note_time(n)))).collect();
    meetings.reverse();
    let mut out: Vec<Passage> = Vec::new();
    for n in meetings {
        let title = chunks::note_title(n);
        let listed: Vec<Chunk> = chunks::chunk_note(n)
            .into_iter()
            .filter(|c| c.field == Field::Summary && c.heading.to_lowercase().contains("decision") && about_it(&c.text(), &title))
            .collect();
        if !listed.is_empty() {
            out.extend(chips::numbered(n, &listed));
            continue;
        }
        for d in n.digests.iter().filter(|d| !d.decisions.is_empty()) {
            let text = format!("Decisions\n{}", d.decisions.iter().map(|x| format!("- {}", x.trim())).collect::<Vec<_>>().join("\n"));
            if about_it(&text, &title) {
                out.push(talk_passage(n, d.from_seg, text));
            }
        }
    }
    let skip = out.len().saturating_sub(GATHER_PASSAGES);
    let passages = renumber(out.into_iter().skip(skip).collect());
    let on = if topic.is_empty() { String::new() } else { format!(" about \"{}\"", quoted(&dated.rest)) };
    let about = format!("The passages are the decisions{on} recorded in each of their meetings then, oldest first; each passage's date is its meeting's.");
    Found { passages, range, about: Some(about), ..Default::default() }
}

/// "questions" (/open-questions): the meeting the words name by its title;
/// else each meeting of the dates they name (else `default`: the last 7
/// days; none then: the latest), newest first, at most 4 — its summary's
/// open questions, and the talk where the most questions were asked (≤ 3
/// passages, with what was said next when a question ends one).
fn questions<Tz: TimeZone>(notes: &[Note], words: &str, default: &str, now: &DateTime<Tz>) -> Found
where
    Tz::Offset: std::fmt::Display,
{
    let meetings = meetings_by_time(notes);
    let dated = crate::search::dates::parse(words, now);
    let (chosen, scope): (Vec<&Note>, String) = match by_title(&meetings, words, dated.range.as_ref()) {
        Some(n) => (vec![n], format!("the meeting \"{}\"", quoted(&chunks::note_title(n)))),
        None => {
            let range = dated.range.clone().or_else(|| crate::search::dates::parse(default, now).range);
            let within: Vec<&Note> = meetings.iter().copied().filter(|n| range.as_ref().is_none_or(|r| r.holds(chunks::note_time(n)))).take(4).collect();
            match (within.is_empty(), &range) {
                (false, Some(r)) => (within, format!("their meetings from {}", r.label)),
                (false, None) => (within, "their meetings".to_string()),
                (true, _) => (meetings.first().copied().into_iter().collect(), "their latest meeting".to_string()),
            }
        }
    };
    let asks = |c: &Chunk| c.lines.iter().filter(|l| l.text.trim_end().ends_with('?')).count();
    // The questions, meeting by meeting, to decide on one by one (as on one
    // meeting: left to the passages, the 4B listed answered ones with their
    // answers, 3 runs of 3, 31 Oct 2026).
    let mut listed: Vec<String> = Vec::new();
    let mut next_q = 1;
    for n in &chosen {
        let lines = question_lines(n, 6, next_q);
        if !lines.is_empty() && next_q <= 16 {
            next_q += lines.len();
            listed.push(format!("In \"{}\":\n{}", quoted(&chunks::note_title(n)), lines.join("\n")));
        }
    }
    let mut out: Vec<Passage> = Vec::new();
    for n in chosen {
        let all = chunks::chunk_note(n);
        let mut picked: Vec<Chunk> = all.iter().filter(|c| c.field == Field::Summary && c.heading.to_lowercase().contains("question")).take(2).cloned().collect();
        let talk: Vec<Chunk> = all.into_iter().filter(|c| c.field == Field::Transcript).collect();
        let mut order: Vec<usize> = (0..talk.len()).filter(|&i| asks(&talk[i]) > 0).collect();
        order.sort_by_key(|&i| (Reverse(asks(&talk[i])), i));
        let mut keep: Vec<usize> = Vec::new();
        for i in order.into_iter().take(3) {
            keep.push(i);
            if talk[i].lines.last().is_some_and(|l| l.text.trim_end().ends_with('?')) && i + 1 < talk.len() {
                keep.push(i + 1);
            }
        }
        keep.sort_unstable();
        keep.dedup();
        picked.extend(keep.into_iter().map(|i| talk[i].clone()));
        out.extend(chips::numbered(n, &picked));
        if out.len() >= GATHER_PASSAGES {
            break;
        }
    }
    out.truncate(GATHER_PASSAGES);
    let mut about = format!("The passages are where questions were asked in {scope}, with what was said after them, and the open questions Yap's summaries list.");
    if !listed.is_empty() {
        about.push_str(&format!("\n\nThe questions asked, each with what was said next. {QUESTIONS_HEAD}\n{}", listed.join("\n")));
    }
    Found { passages: renumber(out), about: Some(about), ..Default::default() }
}

/// A task's owner as the gatherings read it: "You" for one the person
/// added with nobody named, or "me".
fn owner_of(t: &crate::notes::tasks::ListedTask) -> String {
    if (t.own.is_some() && t.task.owner.is_empty()) || t.task.owner.eq_ignore_ascii_case("me") {
        "You".to_string()
    } else {
        t.task.owner.clone()
    }
}

/// An open task of `note` as a passage, `who`'s: it opens the moment it was
/// said (else the summary's line).
fn task_passage(note: &Note, t: &crate::notes::tasks::ListedTask, who: &str) -> Passage {
    let start = note.transcript.iter().find(|s| s.is_talk()).map(|s| s.ts).unwrap_or(0);
    let due = if t.task.due.is_empty() { String::new() } else { format!(" (due: {})", t.task.due) };
    let (field, line, ts, at) = match &t.at {
        Some(m) => (Field::Transcript, m.index, m.ts, Some(ask::clock(m.ts.saturating_sub(start)))),
        None => (Field::Summary, t.task.line, chunks::note_time(note), None),
    };
    Passage {
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
        event: None,
    }
}

/// The open tasks of meetings `ids` (in that order) whose owner `keep`
/// takes, each a passage ([`task_passage`]).
fn open_tasks(notes: &[Note], store: &TaskStore, ids: &[u64], keep: impl Fn(&str) -> bool) -> Vec<Passage> {
    let listed = crate::notes::tasks::list_all(notes, store);
    let mut out = Vec::new();
    for id in ids {
        let (Some(nt), Some(note)) = (listed.iter().find(|x| x.note_id == *id), notes.iter().find(|n| n.id == *id)) else { continue };
        for t in nt.tasks.iter().filter(|t| !t.task.done && !t.archived) {
            let owner = owner_of(t);
            if keep(&owner) {
                out.push(task_passage(note, t, &owner));
            }
        }
    }
    out
}

/// "tasks": every open task — the person's own first ("You", and the ones
/// they added), then the ones nobody took on —, newest meeting first, each a
/// passage that opens the moment it was said (else the summary's line).
/// Tasks added with no meeting have no moment to open: they're left out of
/// the numbered passages.
pub fn tasks_found(notes: &[Note], store: &TaskStore) -> Found {
    let listed = crate::notes::tasks::list_all(notes, store);
    let mut mine: Vec<Passage> = Vec::new();
    let mut nobody: Vec<Passage> = Vec::new();
    for nt in listed.iter().filter(|nt| nt.note_id != 0) {
        let Some(note) = notes.iter().find(|n| n.id == nt.note_id) else { continue };
        for t in nt.tasks.iter().filter(|t| !t.task.done && !t.archived) {
            let owner = owner_of(t);
            let yours = owner == "You";
            let unowned = owner.is_empty() || owner == crate::meetings::meeting_summary::UNASSIGNED;
            if yours {
                mine.push(task_passage(note, t, "You"));
            } else if unowned {
                nobody.push(task_passage(note, t, crate::meetings::meeting_summary::UNASSIGNED));
            }
        }
    }
    let passages = renumber(mine.into_iter().chain(nobody).take(GATHER_PASSAGES * 2).collect());
    Found { passages, ..Default::default() }
}

/// A meeting on the calendar as a passage (first in a "prep" or "person"
/// run, the rest of today in a "standup"): its name, when, who's coming
/// and the start of its invite; its pill opens its note, or makes one from
/// the invite.
fn event_passage<Tz: TimeZone>(e: &Event, agenda: &Agenda, now: &DateTime<Tz>) -> Passage
where
    Tz::Offset: std::fmt::Display,
{
    let at = |ts: i64, f: &str| now.timezone().timestamp_opt(ts, 0).single().map(|t| t.format(f).to_string()).unwrap_or_default();
    let when = at(e.start, "%a %-d %b %Y, %H:%M");
    let names: Vec<&str> = e.attendees.iter().map(|a| a.name.trim()).filter(|n| !n.is_empty()).collect();
    let mut text = format!("Coming up: {}, {when}–{}", e.display_title(), at(e.end, "%H:%M"));
    if !names.is_empty() {
        text.push_str(&format!(", with {}", names.join(", ")));
    }
    let invite: Vec<&str> = e.description.split_whitespace().take(60).collect();
    if !invite.is_empty() {
        text.push_str(&format!("\nOn the invite: {}", invite.join(" ")));
    }
    Passage {
        n: 0,
        note: agenda.notes.get(&e.key).copied().unwrap_or(0),
        title: e.display_title(),
        kind: "meeting",
        field: Field::Summary,
        line: 0,
        lines: vec![0],
        ts: e.start.max(0) as u64,
        when,
        at: None,
        speaker: String::new(),
        text,
        score: 0.0,
        keyword: 0.0,
        similarity: None,
        event: Some(e.key.clone()),
    }
}

/// The next meeting on the calendar (one about to start or still to come
/// before one already under way), the first the words match when they name
/// one (`tools::calendar::matches`: an attendee's name or the title).
fn next_event<'a>(agenda: &'a Agenda, now: i64, words: &str) -> Option<&'a Event> {
    let mut coming: Vec<&Event> = agenda
        .events
        .iter()
        .filter(|e| e.end.max(e.start + 60) > now)
        .filter(|e| words.trim().is_empty() || crate::chat::tools::calendar::matches(e, words))
        .collect();
    coming.sort_by_key(|e| (e.start, e.key.clone()));
    coming.iter().copied().find(|e| e.start + 300 >= now).or_else(|| coming.first().copied())
}

/// The next one-to-one on the calendar: a meeting with one other person.
fn next_one_to_one(agenda: &Agenda, now: i64) -> Option<&Event> {
    agenda.events.iter().filter(|e| e.end > now && e.attendees.len() == 1).min_by_key(|e| (e.start, e.key.clone()))
}

/// The name in a Skill's words ("with Priya Shah" → "Priya Shah"): the
/// little words around it left out.
fn name_words(words: &str) -> String {
    const FILLER: &[&str] = &["with", "my", "the", "a", "an", "next", "upcoming", "meeting", "one-to-one", "1:1", "catch-up", "call", "sync", "for", "about", "and", "to"];
    words.split_whitespace().filter(|w| !FILLER.contains(&w.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric() && c != ':' && c != '-'))).take(4).collect::<Vec<_>>().join(" ")
}

/// A meeting's passages for what came before another with the same people:
/// its summary's recap, decisions and open questions (its tasks come on
/// their own), else the start and end of its talk.
fn history_chunks(note: &Note, per: usize) -> Vec<Chunk> {
    let all = chunks::chunk_note(note);
    let rank = |c: &Chunk| {
        let h = c.heading.to_lowercase();
        if h.contains("recap") || h.contains("summary") || h.contains("overview") {
            0
        } else if h.contains("decision") {
            1
        } else if h.contains("question") {
            2
        } else {
            3
        }
    };
    let summary: Vec<(usize, Chunk)> = all.iter().filter(|c| c.field == Field::Summary).cloned().enumerate().collect();
    if !summary.is_empty() {
        let mut pick: Vec<(usize, Chunk)> = summary.iter().filter(|(_, c)| rank(c) < 3).cloned().collect();
        pick.sort_by_key(|(i, c)| (rank(c), *i));
        pick.truncate(per + 1);
        if pick.is_empty() {
            pick = summary.into_iter().take(per).collect();
        }
        pick.sort_by_key(|(i, _)| *i);
        return pick.into_iter().map(|(_, c)| c).collect();
    }
    let talk: Vec<Chunk> = all.into_iter().filter(|c| c.field == Field::Transcript).collect();
    match talk.len() {
        n if n <= per => talk,
        n => {
            let mut v = talk[..per.saturating_sub(1)].to_vec();
            v.push(talk[n - 1].clone());
            v
        }
    }
}

/// Who and what a "prep" or "person" run is for.
struct Target<'a> {
    /// The meeting on the calendar, when there is one.
    event: Option<&'a Event>,
    key: String,
    title: String,
    start: i64,
    people: Vec<String>,
    description: String,
}

/// The passages for a meeting with `t.people`: the meeting itself when it's
/// on the calendar (first), each earlier meeting with them — the brief's
/// "last time" (`brief::gather`: an attendee in common, or the same
/// series), newest first, at most 3 — and the open tasks from those that are
/// the user's or theirs. Returns them, numbered, and how many earlier
/// meetings.
fn with_people<Tz: TimeZone>(notes: &[Note], store: &TaskStore, agenda: &Agenda, t: &Target, now: &DateTime<Tz>) -> (Vec<Passage>, usize)
where
    Tz::Offset: std::fmt::Display,
{
    let mut out: Vec<Passage> = t.event.map(|e| event_passage(e, agenda, now)).into_iter().collect();
    let m = brief::Meeting { key: t.key.clone(), title: t.title.clone(), start: t.start, attendees: t.people.clone(), description: t.description.clone() };
    let facts = brief::gather(&m, notes, &[], now.timestamp());
    let ids: Vec<u64> = facts.sources.iter().filter(|s| s.kind == "note").filter_map(|s| s.note_id).collect();
    for id in &ids {
        if let Some(n) = notes.iter().find(|n| n.id == *id) {
            out.extend(chips::numbered(n, &history_chunks(n, 2)));
        }
    }
    let theirs = |owner: &str| owner == "You" || t.people.iter().any(|p| brief::same_person(owner, p, &t.people));
    out.extend(open_tasks(notes, store, &ids, theirs).into_iter().take(GATHER_TASKS));
    out.truncate(GATHER_PASSAGES);
    (renumber(out), ids.len())
}

/// "prep" (/prep): the next meeting on the calendar (the first the words
/// match, when they name one) and what came before it with its people;
/// nothing on the calendar for them: the earlier meetings with the person
/// the words name, else the meeting they name by its title.
fn prep<Tz: TimeZone>(notes: &[Note], store: &TaskStore, agenda: &Agenda, words: &str, now: &DateTime<Tz>) -> Found
where
    Tz::Offset: std::fmt::Display,
{
    let now_s = now.timestamp();
    if let Some(e) = next_event(agenda, now_s, words) {
        let people: Vec<String> = e.attendees.iter().map(|a| a.name.trim().to_string()).filter(|n| !n.is_empty()).collect();
        let with = if people.is_empty() { String::new() } else { format!(" with {}", people.join(", ")) };
        let target = Target { event: Some(e), key: e.key.clone(), title: e.display_title(), start: e.start, people, description: e.description.clone() };
        let (passages, earlier) = with_people(notes, store, agenda, &target, now);
        let rest = if earlier == 0 {
            "Yap has no earlier meetings with these people: it's the first."
        } else {
            "The other passages are their earlier meetings with these people, newest first, then the open tasks from them."
        };
        let about = format!("The meeting to prepare for is \"{}\" on {}{with} [1]. {rest}", quoted(&e.display_title()), day_time(now, e.start));
        return Found { passages, about: Some(about), ..Default::default() };
    }
    let cal = if agenda.connected { "" } else { " (Yap can't see their calendar)" };
    if words.trim().is_empty() {
        let about = format!("No meeting is coming up on the user's calendar{cal}: say so, and that they can name one after the command (/prep supplier call).");
        return Found { about: Some(about), ..Default::default() };
    }
    let who = name_words(words);
    let target = Target { event: None, key: String::new(), title: who.clone(), start: now_s, people: vec![who], description: String::new() };
    let (mut passages, mut earlier) = with_people(notes, store, agenda, &target, now);
    if earlier == 0 {
        if let Some(n) = named(&meetings_by_time(notes), words, now) {
            let mut out = chips::numbered(n, &history_chunks(n, 2));
            out.extend(open_tasks(notes, store, &[n.id], |_| true).into_iter().take(GATHER_TASKS));
            passages = renumber(out);
            earlier = 1;
        }
    }
    let about = if earlier == 0 {
        format!("No meeting matching \"{}\" is coming up on the user's calendar{cal}, and Yap has no earlier meetings about it: say so.", quoted(words))
    } else {
        format!(
            "No meeting matching \"{}\" is coming up on the user's calendar{cal}: the passages are their earlier meetings about it, newest first, and the open tasks from them. Prepare them for the next one.",
            quoted(words)
        )
    };
    Found { passages, about: Some(about), ..Default::default() }
}

/// "person" (/one-to-one): the person the words name (their full name from
/// the calendar when it has it), else the next one-to-one's; their next
/// meeting on the calendar (a one-to-one first), the earlier meetings with
/// them and the open tasks between them.
fn one_to_one<Tz: TimeZone>(notes: &[Note], store: &TaskStore, agenda: &Agenda, words: &str, now: &DateTime<Tz>) -> Found
where
    Tz::Offset: std::fmt::Display,
{
    let now_s = now.timestamp();
    let said = name_words(words);
    let (who, event): (String, Option<&Event>) = if said.is_empty() {
        match next_one_to_one(agenda, now_s) {
            Some(e) => (e.attendees[0].name.trim().to_string(), Some(e)),
            None => {
                let about = "No one-to-one is coming up on the user's calendar and they named nobody: ask who it's with (/one-to-one Priya).";
                return Found { about: Some(about.to_string()), ..Default::default() };
            }
        }
    } else {
        let everyone: Vec<String> = agenda.events.iter().flat_map(|e| e.attendees.iter().map(|a| a.name.clone())).collect();
        let is_them = |name: &str| brief::same_person(name, &said, &everyone);
        let coming = agenda.events.iter().filter(|e| e.end > now_s && e.attendees.iter().any(|a| is_them(&a.name))).min_by_key(|e| (e.attendees.len() != 1, e.start, e.key.clone()));
        let full = coming.and_then(|e| e.attendees.iter().find(|a| is_them(&a.name))).map(|a| a.name.trim().to_string()).unwrap_or_else(|| said.clone());
        (full, coming)
    };
    let target = Target {
        event,
        key: event.map(|e| e.key.clone()).unwrap_or_default(),
        title: event.map(Event::display_title).unwrap_or_else(|| format!("One-to-one with {who}")),
        start: event.map_or(now_s, |e| e.start),
        people: vec![who.clone()],
        description: event.map(|e| e.description.clone()).unwrap_or_default(),
    };
    let (passages, earlier) = with_people(notes, store, agenda, &target, now);
    let next = event.map(|e| format!(": next on their calendar, \"{}\" on {} [1]", quoted(&e.display_title()), day_time(now, e.start))).unwrap_or_default();
    let others = if event.is_some() { "The other passages" } else { "The passages" };
    let rest = if earlier == 0 {
        format!("Yap has no earlier meetings with {who}.")
    } else {
        format!("{others} are their earlier meetings with {who}, newest first, then the open tasks of the user and {who} from them.")
    };
    Found { passages, about: Some(format!("The one-to-one is with {who}{next}. {rest}")), ..Default::default() }
}

/// "standup" (/standup): what the person said in the meetings since the
/// last working day began (Friday's on a Monday; at most 4 meetings, the
/// passages where they talk, their own lines alone), their open tasks, then
/// the rest of today's calendar.
fn standup<Tz: TimeZone>(notes: &[Note], store: &TaskStore, agenda: &Agenda, now: &DateTime<Tz>) -> Found
where
    Tz::Offset: std::fmt::Display,
{
    use chrono::Datelike;
    let today = now.date_naive();
    let back = match today.weekday() {
        chrono::Weekday::Mon => 3,
        chrono::Weekday::Sun => 2,
        _ => 1,
    };
    let since_day = today - chrono::Duration::days(back);
    let since = midnight(now, since_day);
    let now_s = now.timestamp().max(0) as u64;
    let meetings: Vec<&Note> = meetings_by_time(notes)
        .into_iter()
        .filter(|n| {
            let t = chunks::note_time(n);
            t >= since && t <= now_s
        })
        .take(4)
        .collect();
    // Their own lines only: with the others' beside them, the 4B gave the
    // user what Tom said he'd do ("I'm ordering the new laptops", 31 Oct).
    let mut out: Vec<Passage> = Vec::new();
    for n in &meetings {
        let mine: Vec<Chunk> = you_chunks(n, 3, true)
            .into_iter()
            .map(|mut c| {
                c.lines.retain(|l| l.speaker == "You");
                c
            })
            .filter(|c| !c.lines.is_empty())
            .collect();
        out.extend(chips::numbered(n, &mine));
    }
    let ids: Vec<u64> = crate::notes::tasks::list_all(notes, store).iter().map(|nt| nt.note_id).filter(|id| *id != 0).collect();
    out.extend(open_tasks(notes, store, &ids, |owner| owner == "You").into_iter().take(GATHER_TASKS));
    let tonight = midnight(now, today + chrono::Duration::days(1)) as i64;
    let mut later: Vec<&Event> = agenda.events.iter().filter(|e| e.start >= now.timestamp() && e.start < tonight).collect();
    later.sort_by_key(|e| e.start);
    out.extend(later.into_iter().take(3).map(|e| event_passage(e, agenda, now)));
    out.truncate(GATHER_PASSAGES);
    let none = if meetings.is_empty() { " (there were none)" } else { "" };
    let about = format!(
        "The passages are what the user (\"You\") said in their meetings since {}{none}, then their open tasks, then what's still on their calendar today.",
        since_day.format("%A %-d %b")
    );
    Found { passages: renumber(out), about: Some(about), ..Default::default() }
}

fn format_when(ts: u64) -> String {
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
        assert_eq!(
            commands,
            [
                "followup-email",
                "summary-for",
                "decisions",
                "decision-log",
                "risks",
                "open-questions",
                "tickets",
                "prd",
                "coach",
                "todos",
                "standup",
                "prep",
                "one-to-one",
                "weekly-recap",
                "project-update"
            ]
        );
        const KINDS: [&str; 12] = ["", "tasks", "you", "week", "latest", "meeting", "decisions", "questions", "topic", "standup", "prep", "person"];
        for s in &b {
            assert!(s.builtin && s.id == format!("builtin:{}", s.command));
            assert!(!s.instructions.is_empty() && s.instructions.len() < INSTRUCTIONS_MAX);
            // Short, firm instructions a 4B follows: at most 600 characters.
            assert!(s.instructions.chars().count() <= 600, "/{}: {} characters", s.command, s.instructions.chars().count());
            // A line that fits the "/" menu, and a name that fits beside it.
            assert!(!s.description.is_empty() && s.description.chars().count() <= 48, "/{}", s.command);
            assert!(s.name.chars().count() <= 24, "/{}", s.command);
            // A command a person could have typed: lowercase words, single hyphens.
            assert!(s.command.split('-').all(|w| !w.is_empty() && w.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())), "/{}", s.command);
            assert!(KINDS.contains(&s.gather.as_str()), "/{}: {}", s.command, s.gather);
            // A gathering of its own is for a run across the meetings.
            assert!(s.gather.is_empty() || s.runs_on.across(), "/{}", s.command);
        }
        // Something to keep: /tickets and /prd write a Chat document; only
        // /project-update needs words to run on.
        let docs: Vec<&str> = b.iter().filter(|s| !s.document.is_empty()).map(|s| s.command.as_str()).collect();
        assert_eq!(docs, ["tickets", "prd"]);
        let asks: Vec<&str> = b.iter().filter(|s| !s.asks.is_empty()).map(|s| s.command.as_str()).collect();
        assert_eq!(asks, ["project-update"]);
        // On one meeting (its Ask box): these eight.
        let on_one: Vec<&str> = b.iter().filter(|s| s.runs_on.on_one()).map(|s| s.command.as_str()).collect();
        assert_eq!(on_one, ["followup-email", "summary-for", "decisions", "risks", "open-questions", "tickets", "prd", "coach"]);
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
        let now = chrono::Utc::now();
        let found = latest_you(&[old, new], "", &now);
        assert_eq!(found.latest.as_deref(), Some("Budget review"));
        assert!(found.passages.iter().all(|p| p.note == 2));
        assert!(found.passages[0].text.contains("You: I think we should cut the second shift"));
    }

    #[test]
    fn coach_me_on_the_meeting_the_words_name() {
        use chrono::TimeZone;
        let now = chrono::Utc.with_ymd_and_hms(2026, 10, 29, 12, 0, 0).unwrap(); // a Thursday
        let tue = chrono::Utc.with_ymd_and_hms(2026, 10, 27, 10, 0, 0).unwrap().timestamp() as u64;
        let wed = chrono::Utc.with_ymd_and_hms(2026, 10, 28, 15, 0, 0).unwrap().timestamp() as u64;
        let thu = chrono::Utc.with_ymd_and_hms(2026, 10, 29, 9, 0, 0).unwrap().timestamp() as u64;
        let notes = [
            meeting(1, "Budget review", tue, "", &[("you", "I'll take the budget numbers to finance myself.")]),
            meeting(2, "Daily standup", wed, "", &[("you", "Yesterday I fixed the export, today the release notes.")]),
            meeting(3, "Supplier call", thu, "", &[("you", "Can you send the revised quote by Friday?")]),
        ];
        let on = |words: &str| latest_you(&notes, words, &now).passages.first().map(|p| p.note);
        // Its title, as said with the command (any case, punctuation aside).
        assert_eq!(on("the Budget Review, please"), Some(1));
        // The day it was on.
        assert_eq!(on("yesterday's"), Some(2));
        // Words that name no meeting are how to coach, not which meeting:
        // the latest, as before.
        assert_eq!(on("be blunt about my questions"), Some(3));
        assert_eq!(on(""), Some(3));
        // A title only as part of another word isn't it.
        assert_eq!(on("my budget reviews in general"), Some(3));
        // A meeting named is said to be that one, never "their latest"; the
        // chip still names it.
        let named = latest_you(&notes, "the budget review", &now);
        assert_eq!(named.latest, None);
        assert!(named.about.as_deref().unwrap().starts_with("The passages are from the meeting \"Budget review\""));
        assert!(chip(&turn_for("coach", "/coach the budget review").unwrap(), &named).ends_with("in \"Budget review\""));
        assert_eq!(latest_you(&notes, "", &now).latest.as_deref(), Some("Supplier call"));
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

    // ---- the Skills added on 31 Oct 2026 ----------------------------------------------------

    /// Unix seconds of an October 2026 day and time (UTC).
    fn oct(d: u32, h: u32) -> u64 {
        use chrono::TimeZone;
        chrono::Utc.with_ymd_and_hms(2026, 10, d, h, 0, 0).unwrap().timestamp() as u64
    }

    /// Thursday 29 Oct 2026, noon.
    fn thursday() -> chrono::DateTime<chrono::Utc> {
        use chrono::TimeZone;
        chrono::Utc.with_ymd_and_hms(2026, 10, 29, 12, 0, 0).unwrap()
    }

    fn with(mut n: Note, people: &[&str]) -> Note {
        n.participants = people.iter().map(|p| p.to_string()).collect();
        n
    }

    /// A fictional team's meetings: three with Priya Shah or Tom Reid this
    /// month, one from September, and one with only the decisions Yap noted
    /// while it recorded.
    fn team() -> Vec<Note> {
        let supplier = with(
            meeting(
                1,
                "Supplier call",
                oct(27, 10),
                "## Recap\nThe packaging quote came in over budget.\n\n## Action plan\n### You\n- [ ] Draft the finance request (due: Wednesday)\n### Priya Shah\n- [ ] Send the revised quote (due: Friday)\n### Tom Reid\n- [ ] Check the warehouse space\n\n## Decisions\n- Ask finance for the extra budget on Thursday, because the quote is final\n\n## Open questions\n- Can the supplier deliver before the launch?",
                &[
                    ("you", "Priya, is the quote final now?"),
                    ("them", "Yes, it is final at forty two thousand pounds."),
                    ("you", "Then I'll draft the finance request by Wednesday and ask them on Thursday."),
                    ("them", "Can the supplier deliver before the launch?"),
                    ("you", "Nobody knows yet, we should find out."),
                ],
            ),
            &["Priya Shah"],
        );
        let budget = with(
            meeting(
                2,
                "Budget review",
                oct(28, 15),
                "## Recap\nThe team went through the quarter's costs.\n\n## Decisions\n- Cut the second shift from December, to stay within budget",
                &[("you", "I finished the cost sheet yesterday and sent it round."), ("them", "Thanks. So we cut the second shift from December?"), ("you", "Yes. I'm still waiting on payroll for the final numbers, that's blocking me.")],
            ),
            &["Tom Reid"],
        );
        let kickoff = with(
            meeting(
                3,
                "Kick-off",
                oct(5, 10),
                "## Recap\nThe packaging project started.\n\n## Decisions\n- Launch the new packaging in spring",
                &[("you", "Welcome everyone, this is the packaging kick-off."), ("them", "We should launch in spring, before the trade fair.")],
            ),
            &["Priya Shah", "Tom Reid"],
        );
        let old = with(
            meeting(4, "Old planning", 1_789_000_000, "## Decisions\n- Hire a contractor", &[("you", "Let us hire a contractor for the summer.")]),
            &["Sam Lee"],
        );
        let mut design = meeting(5, "Design sync", oct(28, 11), "", &[("you", "I'll pick the logo colour today."), ("them", "The blue one reads best on the box.")]);
        design.digests = vec![serde_json::from_value(serde_json::json!({
            "fromSeg": 1, "toSeg": 1, "startTs": oct(28, 11), "endTs": oct(28, 11) + 30, "decisions": ["Use the blue logo on the box"],
        }))
        .unwrap()];
        vec![supplier, budget, kickoff, old, design]
    }

    fn event(key: &str, title: &str, start: u64, minutes: i64, people: &[&str], description: &str) -> Event {
        Event {
            key: key.into(),
            connection: "test".into(),
            title: title.into(),
            start: start as i64,
            end: start as i64 + minutes * 60,
            attendees: people.iter().map(|p| crate::calendar::Attendee { name: p.to_string(), email: String::new() }).collect(),
            organizer: String::new(),
            join_url: None,
            service: None,
            description: description.into(),
            tentative: false,
            codes: Vec::new(),
        }
    }

    /// The calendar: team planning this afternoon (Priya and Tom), then a
    /// one-to-one with Priya tomorrow.
    fn calendar() -> Agenda {
        Agenda {
            connected: true,
            events: vec![
                event("cal:2", "Team planning", oct(29, 15), 60, &["Priya Shah", "Tom Reid"], "Agenda: the launch date, the budget"),
                event("cal:1", "Supplier follow-up", oct(30, 10), 30, &["Priya Shah"], "Agenda: delivery dates, the revised quote"),
            ],
            ..Default::default()
        }
    }

    /// What Skill `/command words` gathers over the team's meetings, the
    /// calendar and no tasks of the person's own, on Thursday at noon.
    fn gathered(command: &str, words: &str) -> Gathered {
        let turn = SkillTurn { skill: by_command(command).unwrap(), words: words.into() };
        gather(&turn, &team(), &TaskStore::default(), &calendar(), &thursday())
    }

    fn found(command: &str, words: &str) -> Found {
        match gathered(command, words) {
            Gathered::Found(f) => f,
            g => panic!("/{command} {words}: {g:?}"),
        }
    }

    fn titles(f: &Found) -> Vec<&str> {
        f.passages.iter().map(|p| p.title.as_str()).collect()
    }

    fn texts(f: &Found) -> String {
        f.passages.iter().map(|p| p.text.as_str()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn prep_reads_the_next_meeting_and_what_came_before_with_its_people() {
        let f = found("prep", "");
        // Passage 1: the meeting coming up, from the calendar (its pill
        // opens its note, or makes one from the invite).
        let first = &f.passages[0];
        assert_eq!((first.n, first.event.as_deref(), first.note), (1, Some("cal:2"), 0));
        assert!(first.text.starts_with("Coming up: Team planning, Thu 29 Oct 2026, 15:00–16:00, with Priya Shah, Tom Reid"), "{}", first.text);
        assert!(first.text.contains("On the invite: Agenda: the launch date, the budget"));
        let source = &ask::sources(&f)[0];
        assert_eq!((source.note, source.event.as_deref(), source.field.as_str()), (None, Some("cal:2"), "calendar"));
        // Then the earlier meetings with Priya or Tom, newest first — never
        // September's with someone else — and the open tasks from them.
        let t = titles(&f);
        assert_eq!(t[1..4], ["Budget review", "Budget review", "Supplier call"]);
        assert!(!t.contains(&"Old planning") && !t.contains(&"Design sync"));
        let all = texts(&f);
        assert!(all.contains("Open task \u{2014} You: Draft the finance request (due: Wednesday)"));
        assert!(all.contains("Open task \u{2014} Priya Shah: Send the revised quote (due: Friday)"));
        assert!(all.contains("Open task \u{2014} Tom Reid: Check the warehouse space"));
        let about = f.about.as_deref().unwrap();
        assert!(about.starts_with("The meeting to prepare for is \"Team planning\" on Thu 29 Oct, 15:00 with Priya Shah, Tom Reid [1]."), "{about}");
        assert!(f.passages.iter().enumerate().all(|(i, p)| p.n == i + 1));

        // The words name the meeting: the supplier follow-up, Priya's alone:
        // Tom's task is no longer one of theirs.
        let f = found("prep", "the supplier follow-up");
        assert_eq!(f.passages[0].event.as_deref(), Some("cal:1"));
        assert!(!texts(&f).contains("Check the warehouse space"));
        assert!(titles(&f).contains(&"Kick-off"));
    }

    #[test]
    fn prep_with_nothing_on_the_calendar_finds_the_meetings_the_words_name() {
        let turn = SkillTurn { skill: by_command("prep").unwrap(), words: "Priya".into() };
        let Gathered::Found(f) = gather(&turn, &team(), &TaskStore::default(), &Agenda::default(), &thursday()) else { panic!() };
        assert!(f.passages.iter().all(|p| p.event.is_none()));
        assert!(titles(&f).contains(&"Supplier call") && titles(&f).contains(&"Kick-off"));
        assert!(!titles(&f).contains(&"Budget review"), "Tom's, not Priya's");
        assert!(f.about.as_deref().unwrap().contains("(Yap can't see their calendar)"));
        // By its title, when no person is in the words.
        let turn = SkillTurn { words: "the design sync".into(), ..turn };
        let Gathered::Found(f) = gather(&turn, &team(), &TaskStore::default(), &Agenda::default(), &thursday()) else { panic!() };
        assert_eq!(titles(&f).first(), Some(&"Design sync"));
        // Nothing named and nothing coming up: Yap asks, no model called.
        let now = thursday().timestamp();
        assert_eq!(asks_for("prep", &Agenda::default(), now).as_deref(), Some(PREP_NO_CALENDAR));
        assert_eq!(asks_for("prep", &Agenda { connected: true, ..Default::default() }, now).as_deref(), Some(PREP_NOTHING_COMING));
        assert_eq!(asks_for("prep", &calendar(), now), None);
    }

    #[test]
    fn a_one_to_one_is_with_the_person_named_or_the_next_one_to_one() {
        // Nobody named: the next meeting with one other person.
        let f = found("one-to-one", "");
        assert_eq!(f.passages[0].event.as_deref(), Some("cal:1"));
        let about = f.about.as_deref().unwrap();
        assert!(about.starts_with("The one-to-one is with Priya Shah: next on their calendar, \"Supplier follow-up\""), "{about}");
        assert!(titles(&f).contains(&"Supplier call") && !titles(&f).contains(&"Budget review"));
        // "Tom": his full name from the calendar, his meetings and tasks.
        let f = found("one-to-one", "with Tom");
        assert!(f.about.as_deref().unwrap().starts_with("The one-to-one is with Tom Reid"));
        assert_eq!(f.passages[0].event.as_deref(), Some("cal:2"));
        assert!(titles(&f).contains(&"Budget review"));
        assert!(!texts(&f).contains("Send the revised quote"), "Priya's task isn't Tom's");
        // Someone the calendar and the meetings don't know: said plainly.
        let f = found("one-to-one", "Jo");
        assert!(f.passages.is_empty());
        assert!(f.about.as_deref().unwrap().ends_with("Yap has no earlier meetings with Jo."));
        // Nobody named and no one-to-one coming up: Yap asks who.
        let only_team = Agenda { events: calendar().events.into_iter().filter(|e| e.attendees.len() > 1).collect(), ..calendar() };
        assert_eq!(asks_for("person", &only_team, thursday().timestamp()).as_deref(), Some(PERSON_ASKS));
        assert_eq!(asks_for("person", &calendar(), thursday().timestamp()), None);
    }

    #[test]
    fn a_decision_log_is_each_meetings_decisions_oldest_first() {
        let f = found("decision-log", "");
        // The last 4 weeks, oldest first: September's is out; a meeting with
        // no summary yet gives the decisions Yap noted while it recorded.
        assert_eq!(titles(&f), ["Kick-off", "Supplier call", "Design sync", "Budget review"]);
        assert!(f.passages[0].text.starts_with("Decisions\nLaunch the new packaging in spring"));
        assert_eq!((f.passages[2].field, f.passages[2].line), (Field::Transcript, 1));
        assert!(f.passages[2].text.contains("- Use the blue logo on the box"));
        assert!(!texts(&f).contains("Hire a contractor"));
        assert!(f.range.is_some());
        assert!(chip(&turn_for("decision-log", "/decision-log").unwrap(), &f).starts_with("Ran /decision-log · 4 passages from the last 4 weeks"));
        // A topic: only the decisions about it (or from a meeting named for it).
        let f = found("decision-log", "about the budget");
        assert_eq!(titles(&f), ["Supplier call", "Budget review"]);
        assert!(f.about.as_deref().unwrap().contains("about \"about the budget\""));
        // Its own dates.
        let f = found("decision-log", "since September");
        assert!(titles(&f).contains(&"Old planning"));
    }

    #[test]
    fn open_questions_read_where_questions_were_asked_and_what_came_next() {
        let f = found("open-questions", "the supplier call");
        assert!(titles(&f).iter().all(|t| *t == "Supplier call"));
        let all = texts(&f);
        // The summary's open questions, and the talk where they were asked.
        assert!(all.contains("Open questions\nCan the supplier deliver before the launch?"));
        assert!(all.contains("Can the supplier deliver before the launch?\n") || all.contains("Nobody knows yet"));
        assert!(f.about.as_deref().unwrap().contains("the meeting \"Supplier call\""));
        // No meeting named: the last 7 days', newest first.
        let f = found("open-questions", "");
        assert_eq!(titles(&f).first(), Some(&"Budget review"));
        assert!(!titles(&f).contains(&"Kick-off"));
        assert!(texts(&f).contains("So we cut the second shift from December?"));
        // The questions to decide on, meeting by meeting, numbered on.
        let about = f.about.as_deref().unwrap();
        assert!(about.contains("\n\nThe questions asked, each with what was said next. Decide for each one"), "{about}");
        assert!(about.contains("In \"Budget review\":\nQ1. Them: \"So we cut the second shift from December?\""), "{about}");
        assert!(about.contains("In \"Supplier call\":\nQ2. You: \"Priya, is the quote final now?\""), "{about}");
    }

    #[test]
    fn open_questions_on_one_meeting_get_the_questions_asked_to_go_through() {
        let notes = team();
        let aid = meeting_aid(&by_command("open-questions").unwrap(), &notes[0]).unwrap();
        assert!(aid.starts_with("The questions asked in it, in order, each with what was said next. Decide for each one whether what was said next answers it (\"I don't know\", \"let's come back to that\" or a change of subject is no answer), and list every one it doesn't.\n"));
        // The 4B copies the numbers onto its bullets: Yap takes them off.
        let s_ = by_command("open-questions").unwrap();
        assert_eq!(
            tidy_answer(&s_, "- Q4: What do we tell customers [2]? Them\n- **Q5.** kept\nQ1) Who drives it? You"),
            "- What do we tell customers [2]? Them\n- **Q5.** kept\nWho drives it? You"
        );
        assert_eq!(tidy_answer(&by_command("decisions").unwrap(), "- Q4: as written"), "- Q4: as written");
        // Each question as asked, numbered, by whom, in order (a line's
        // question alone), with what was said next; then what the summary
        // left open.
        let lines: Vec<&str> = aid.lines().skip(1).collect();
        assert_eq!(
            lines,
            [
                "Q1. You: \"Priya, is the quote final now?\"",
                "  Said next: Them: \"Yes, it is final at forty two thousand pounds.\"",
                "Q2. Them: \"Can the supplier deliver before the launch?\"",
                "  Said next: You: \"Nobody knows yet, we should find out.\"",
                "Q3. (left open in Yap's summary) Can the supplier deliver before the launch?"
            ]
        );
        // In the request, after the meeting's passages.
        let s = by_command("open-questions").unwrap();
        let cited = crate::meetings::meeting_assist::skill_input_for(&s, &notes[0], "").unwrap();
        assert!(cited.text.find("<passage n=\"1\"").unwrap() < cited.text.find("The questions asked in it").unwrap());
        let budget = &notes[1];
        assert!(meeting_aid(&s, budget).unwrap().contains("Q1. Them: \"So we cut the second shift from December?\"\n  Said next: You: \"Yes. I'm still waiting"));
        // /tickets: the work the summary says was agreed, with its owners.
        let tickets = by_command("tickets").unwrap();
        assert!(meeting_aid(&tickets, &notes[0]).unwrap().ends_with(
            "with its owners (a ticket each; nothing the meeting put off or turned down):\n- You: Draft the finance request (due: Wednesday)\n- Priya Shah: Send the revised quote (due: Friday)\n- Tom Reid: Check the warehouse space"
        ));
        // …in Chat too, when it reads that one meeting.
        let f = found("tickets", "from the supplier call");
        assert!(f.about.as_deref().unwrap().contains("\n\nThe work Yap's summary says was agreed"));
        // Other Skills, a meeting with no tasks: the passages alone.
        assert_eq!(meeting_aid(&by_command("decisions").unwrap(), &notes[0]), None);
        assert_eq!(meeting_aid(&tickets, budget), None);
        let mut copy = tickets.clone();
        copy.builtin = false;
        assert_eq!(meeting_aid(&copy, &notes[0]), None);
    }

    #[test]
    fn a_stand_up_reads_your_words_since_the_last_working_day_your_tasks_and_today() {
        let f = found("standup", "");
        // Thursday: since Wednesday, your own lines.
        let t = titles(&f);
        assert!(t.contains(&"Budget review") && t.contains(&"Design sync"));
        // Tuesday's talk is out (its task, still open, is in).
        assert!(f.passages.iter().filter(|p| !p.text.starts_with("Open task")).all(|p| p.title != "Supplier call"));
        let all = texts(&f);
        assert!(all.contains("You: I finished the cost sheet yesterday"));
        assert!(all.contains("Open task \u{2014} You: Draft the finance request (due: Wednesday)"));
        assert!(!all.contains("Send the revised quote"), "someone else's");
        // The rest of today's calendar, not tomorrow's.
        let events: Vec<&str> = f.passages.iter().filter_map(|p| p.event.as_deref()).collect();
        assert_eq!(events, ["cal:2"]);
        assert!(f.about.as_deref().unwrap().contains("since Wednesday 28 Oct"));
        // On a Monday: since Friday.
        use chrono::TimeZone;
        let monday = chrono::Utc.with_ymd_and_hms(2026, 11, 2, 9, 0, 0).unwrap();
        let turn = SkillTurn { skill: by_command("standup").unwrap(), words: String::new() };
        let Gathered::Found(f) = gather(&turn, &team(), &TaskStore::default(), &Agenda::default(), &monday) else { panic!() };
        assert!(f.about.as_deref().unwrap().contains("since Friday 30 Oct (there were none)"));
    }

    #[test]
    fn one_meeting_skills_read_the_meeting_named_else_the_latest_else_search() {
        // /summary-for: the words say who it's for; the latest meeting.
        let f = found("summary-for", "Priya, she leads design");
        assert!(titles(&f).iter().all(|t| *t == "Budget review"));
        assert!(f.about.as_deref().unwrap().starts_with("The passages are from their latest meeting, \"Budget review\" (Wed 28 Oct, with Tom Reid)."));
        assert!(chip(&turn_for("summary-for", "/summary-for Priya").unwrap(), &f).ends_with("in \"Budget review\""));
        // /tickets on the meeting the words name, every part of it.
        let f = found("tickets", "from the supplier call");
        assert!(titles(&f).iter().all(|t| *t == "Supplier call"));
        let fields: Vec<Field> = f.passages.iter().map(|p| p.field).collect();
        assert!(fields.contains(&Field::Summary) && fields.contains(&Field::Transcript));
        assert!(f.about.as_deref().unwrap().starts_with("The passages are from the meeting \"Supplier call\""));
        // Words that name no meeting: a search for them (oldest first for
        // a project's update).
        assert!(matches!(gathered("prd", "offline mode"), Gathered::Search { ref query, about: true, by_time: false, .. } if query == "offline mode"));
        assert!(matches!(gathered("project-update", "packaging"), Gathered::Search { about: true, by_time: true, .. }));
        let searched = Found { passages: found("prep", "").passages, ..Default::default() };
        let story = about_words(searched, "packaging", true);
        assert!(story.passages.windows(2).all(|w| w[0].ts <= w[1].ts));
        assert!(story.passages.iter().enumerate().all(|(i, p)| p.n == i + 1));
        assert_eq!(story.about.as_deref(), Some("The passages are what their meetings and notes say about \"packaging\", oldest first."));
    }

    #[test]
    fn a_skill_with_nothing_to_run_on_asks_and_a_document_skill_says_how() {
        let update = SkillTurn { skill: by_command("project-update").unwrap(), words: String::new() };
        assert!(ask_first(&update).unwrap().starts_with("Which project?"));
        assert_eq!(ask_first(&SkillTurn { words: "website relaunch".into(), ..update }), None);
        assert_eq!(ask_first(&turn_for("decision-log", "/decision-log").unwrap()), None);
        // /tickets and /prd, run in Chat: a document, citing the passages.
        let note = document_note(&turn_for("prd", "/prd").unwrap()).unwrap();
        assert!(note.starts_with("Write this as a document now, not in the chat: create_document"));
        assert!(note.contains("the sections Problem, Goals, Requirements, Out of scope and Open questions"));
        assert!(note.contains("Cite the passages in it as [1]"));
        assert!(document_note(&turn_for("tickets", "/tickets").unwrap()).unwrap().contains("one section, \"Tickets\", each ticket a \"### \" heading in it"));
        assert_eq!(document_note(&turn_for("decisions", "/decisions").unwrap()), None);
        // A copy of one is the person's: no document, nothing asked first.
        let mut copy = by_command("prd").unwrap();
        copy.command = "my-prd".into();
        let copy = cleaned(copy).unwrap();
        assert!(copy.document.is_empty() && copy.asks.is_empty() && copy.gather.is_empty());
    }

    #[test]
    fn every_skill_s_request_fits_an_8k_model() {
        use crate::meetings::meeting_summary::estimate_tokens;
        let notes = team();
        for s in builtins() {
            // On one meeting, in its Ask box: the guardrails, the meeting, the instructions.
            if s.runs_on.on_one() {
                let cited = crate::meetings::meeting_assist::skill_input_for(&s, &notes[0], "for the design team").unwrap();
                let m = crate::meetings::meeting_assist::skill_messages(&s, &cited.text, "for the design team");
                let all: String = m.as_array().unwrap().iter().map(|x| x["content"].as_str().unwrap_or("").to_string()).collect();
                assert!(estimate_tokens(&all) < 5_500, "/{} on one meeting: {}", s.command, estimate_tokens(&all));
            }
            // Across the meetings, in Chat: what it adds to Chat's own prompt.
            if s.runs_on.across() {
                let turn = SkillTurn { skill: s.clone(), words: String::new() };
                let Gathered::Found(f) = gather(&turn, &notes, &TaskStore::default(), &calendar(), &thursday()) else { continue };
                let added = format!("{}\n{}\n{}", system_block(&turn), ask::context(&f), document_note(&turn).unwrap_or_default());
                assert!(estimate_tokens(&added) < 3_000, "/{} across: {}", s.command, estimate_tokens(&added));
                assert!(f.passages.len() <= GATHER_PASSAGES * 2, "/{}", s.command);
            }
        }
    }
}

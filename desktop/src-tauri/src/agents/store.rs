//! **The agents store**: `agents.json` (every agent, each chat's Mode, each Code
//! chat's folder, the one-time Bypass OK), and each agent's own folder under
//! `agents/<id>/` — `log.jsonl`,
//! the `yap-agent` events it sent (the tab replays them), and `backup/`, the
//! files Claude Code changed as they were before (the run endpoint's backup
//! hook writes them, `integrations/claude_live/backup.rs`).
//!
//! A file of its own, never fields in chats.json: the installed Yap and a
//! newer dev build share the data folder, and an older Yap drops fields it
//! doesn't know when it saves. Saved through `saving.rs` (a failed save kept
//! in memory and said once; a file Yap couldn't read never written over).
//! The log is appended, line by line, as the agent goes (an agent's tab can be
//! thousands of lines: the whole file isn't written again each time); a
//! line that couldn't be written is left out, the agent itself is in
//! agents.json.
//!
//! **Agents die with Yap** (Claude Code runs in Yap's kill-on-close job): one
//! still queued, running or asking when Yap starts is **interrupted**
//! ([`at_start`]), and can be continued. **Backups and logs go after 7 days**
//! ([`sweep`]); the agent's record stays (History), marked `swept`.
//!
//! Unit tests keep a store and a data folder per test thread (never Yap's).
//! The log has ids, counts and states, never a brief, a path or an output.

#[cfg(not(test))]
use std::sync::Mutex;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::integrations::claude_live::Origin;

/// How long an agent's backups and log are kept after it ends.
pub const KEEP_SECS: u64 = 7 * 24 * 60 * 60;
/// The most lines an agent's log keeps (the rest of a runaway agent isn't).
pub const LOG_MAX_LINES: usize = 5_000;
/// The most characters of text a log line keeps.
const LOG_TEXT_MAX: usize = 4_000;
/// The most characters of a turn's final words its note keeps.
pub const TURN_TEXT_MAX: usize = 8_000;

/// Where an agent is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    /// Waiting for a free slot (3 at once, one per folder).
    #[default]
    Queued,
    Running,
    /// Waiting for the person to allow a step (a permission card).
    Asking,
    Done,
    Failed,
    /// Stopped by the person.
    Stopped,
    /// Yap quit (or crashed) while it ran: Continue picks it up.
    Interrupted,
}

impl Status {
    /// Queued, running or asking: not over.
    pub fn live(self) -> bool {
        matches!(self, Status::Queued | Status::Running | Status::Asking)
    }

    pub fn word(self) -> &'static str {
        match self {
            Status::Queued => "queued",
            Status::Running => "running",
            Status::Asking => "asking",
            Status::Done => "done",
            Status::Failed => "failed",
            Status::Stopped => "stopped",
            Status::Interrupted => "interrupted",
        }
    }
}

/// Something Claude Code wasn't allowed to do.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Denial {
    pub tool: String,
    /// What it wanted, in plain words ("Run a command: rm -rf build").
    pub detail: String,
}

/// What went wrong with Claude Code itself (`claude_code::events::Problem`),
/// kept: its kind, the words for the person, and when a limit resets.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProblemNote {
    /// "notSignedIn" | "rateLimited" | "billing" | "account" | "paysAnotherWay".
    pub kind: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<i64>,
}

impl ProblemNote {
    pub fn of(p: &crate::ai::claude_code::events::Problem) -> ProblemNote {
        use crate::ai::claude_code::events::Problem as P;
        let (kind, resets_at) = match p {
            P::NotSignedIn => ("notSignedIn", None),
            P::RateLimited { resets_at, .. } => ("rateLimited", *resets_at),
            P::Billing => ("billing", None),
            P::Account { .. } => ("account", None),
            P::PaysAnotherWay { .. } => ("paysAnotherWay", None),
        };
        ProblemNote { kind: kind.into(), message: p.message(), resets_at }
    }
}

/// **One agent.**
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Agent {
    pub id: String,
    /// The chat that started it.
    pub conversation: Option<u64>,
    /// The person's message that asked for it ("chat:<conversation>:<index>";
    /// the answer that started it comes right after it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// Asked typed or by voice (a voice agent's report is read aloud).
    pub origin: Origin,
    /// Its folder's name, and the folder.
    pub folder: String,
    pub path: String,
    /// What Claude Code was asked (the chat model's words).
    pub brief: String,
    /// A full model id ("claude-sonnet-5-5").
    pub model: String,
    pub effort: String,
    /// The mode its latest turn ran at (`modes::Mode::as_str`).
    pub mode: String,
    /// "plan" (the person's Claude plan) or "key" (their API key).
    pub pay: String,
    /// Claude Code's session (`--session-id`, then `--resume`).
    pub session: String,
    pub status: Status,
    /// Unix seconds.
    pub created_ts: u64,
    /// When it first started (0: never ran).
    pub started_ts: u64,
    /// When its latest turn ended.
    pub ended_ts: u64,
    pub updated_ts: u64,
    /// The files it changed (inside its folder: relative to it).
    pub files: Vec<String>,
    /// The commands it ran.
    pub commands: Vec<String>,
    /// What it wasn't allowed to do.
    pub denials: Vec<Denial>,
    /// Its latest turn's final words (its summary, its plan, its question).
    pub text: String,
    /// Claude Code's estimate of the API's price for the agent so far
    /// (`total_cost_usd`): on a plan what it would have cost, not a charge.
    pub cost: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<ProblemNote>,
    /// What else went wrong, in plain words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Its turns so far (the brief, then each message).
    pub turns: u32,
    /// Its latest turn ran in Plan mode: `text` is a plan, nothing changed.
    pub plan: bool,
    /// Its latest turn's plan was allowed (Claude Code's ExitPlanMode card,
    /// Go ahead): its next turn carries the plan out (`runner::run`).
    pub plan_allowed: bool,
    /// Its final words ask the person something.
    pub asks: bool,
    /// Its latest turn has been reported in its chat.
    pub reported: bool,
    /// The report's marker message ("chat:<conversation>:<index>").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_message_id: Option<String>,
    /// Undo agent put its files back.
    pub undone: bool,
    /// It changed files and no backups were made: Claude Code's hooks
    /// didn't run (a managed policy turned them off), so Undo can't put
    /// them back (`runner::no_backups`).
    pub no_backups: bool,
    /// Its backups and log are gone (7 days after it ended).
    pub swept: bool,
    /// The diff lines its log has so far (`diff::AGENT_MAX` at most).
    pub diff_lines: u32,
    /// **Its turns** (2026-11-07: one agent per Code chat): each request to
    /// it — its brief, each follow-up — and what came of it. A field of its
    /// own (an older Yap drops it on its save; an agent without it reads as one
    /// turn, [`Agent::turn_notes`]).
    pub turn_list: Vec<TurnNote>,
    /// Its model was the person's own ask (kept for its next turns), not
    /// the chat's pick nor Settings' default (which its next turns take as
    /// they are then).
    pub model_asked: bool,
    pub effort_asked: bool,
    /// **Its latest turn's model or effort was the chat's pick** for the
    /// task (8 Nov 2026, `agents/sizing.rs`): Continue and a plan's
    /// go-ahead keep it; a new request is sized again. A flag left out while
    /// false: an older Yap's save drops it (that turn's Continue then takes
    /// Settings').
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub sized: bool,
    /// Why this is a new agent and not the chat's earlier one carried on (its
    /// session couldn't be resumed): the tab's first line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why_new: Option<String>,
    /// **Its latest turn's plan wasn't used** ([`TurnNote::plan_unused`],
    /// mirrored here as `plan` and `plan_allowed` are). A new field: an
    /// older Yap drops it on its save, and the next start sets it again
    /// ([`set_plans_aside`]).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub plan_unused: bool,
    /// **The runner has it now** (working, or waiting for a slot): set on the
    /// copies the page reads (`agent_list`, `agent_get`), so the Agent panel
    /// knows a row whose record says "running" but that nothing runs (Move
    /// to finished). Never saved: the store's own copy is always false.
    #[serde(skip_deserializing, skip_serializing_if = "std::ops::Not::not")]
    pub active: bool,
    /// The step it's on now (not kept: the tab's live line).
    #[serde(skip)]
    pub step: String,
}

/// **One turn of an agent**: one request to it (its brief, a follow-up, a
/// plan's go-ahead) and what came of it. Continue carries on the same turn.
/// Its backups are `agents/<id>/backup/<n>/` ([`turn_backup_dir`]).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TurnNote {
    /// 1, 2, …: its place in the agent.
    pub n: u32,
    /// The request in a few words: the tab's header ("Add a minimap button").
    pub title: String,
    /// What it was asked (the brief, the follow-up), as the person's chat
    /// put it.
    pub request: String,
    /// The person's message that asked for it ("chat:<conversation>:<index>"):
    /// its pill goes under the answer after it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// Queued until it runs (behind the turn running, or a slot).
    pub status: Status,
    /// What it ran at.
    pub mode: String,
    /// **A read-only turn** (8 Nov 2026): "catch up", "have a look", "don't
    /// change anything" — at its mode, Claude Code's editing tools off
    /// (`runner::READ_ONLY_OFF`), never a plan to approve. A field of the
    /// turn's own: an older Yap drops it on its save (a Continue then runs
    /// it with its tools).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub read_only: bool,
    pub model: String,
    pub effort: String,
    /// **Why the chat sized it so**, in a few words ("a multi-file change":
    /// `start_agent`'s `sizing`, 8 Nov 2026), shown in the Agent panel row's
    /// ▸ with its model and effort. A field of the turn's own: an older Yap
    /// drops it on its save (the row then shows no reason).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sizing: Option<String>,
    /// Unix seconds (0: never ran).
    pub started_ts: u64,
    pub ended_ts: u64,
    /// Its own cost (Claude Code's estimate, as the agent's).
    pub cost: f64,
    /// The files it changed.
    pub files: Vec<String>,
    /// What it wasn't allowed to do.
    pub denials: Vec<Denial>,
    /// Its final words.
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub plan: bool,
    /// Its plan was allowed (ExitPlanMode's Go ahead): the next turn carries
    /// it out.
    pub plan_allowed: bool,
    pub asks: bool,
    /// Its report's marker ("chat:<conversation>:<index>").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_message_id: Option<String>,
    /// **Its report waits for the next turn's** (11 Nov 2026): a request
    /// came while it worked, so the agent went straight on and this turn
    /// isn't reported by itself — the report once the agent stops covers it
    /// ([`Agent::as_of_report`]). A new field of the turn's own: an older
    /// Yap drops it on its save, and that turn is then left out of the
    /// report (its words stay in its tab).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub report_held: bool,
    /// **Chat's own answer read its end before its report** (11 Nov 2026):
    /// asked about the agent just as it finished, Chat's model looked with
    /// `agent_status` and its answer told the result — then the report said
    /// it all again. The chat's length when it looked: an answer saved since
    /// means the person has been told, and the report adds its card alone
    /// (`chat/turn/report.rs`). Cleared as the turn starts again. A new
    /// field of the turn's own: an older Yap drops it on its save, and the
    /// report then says it as it always did.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub told_at: Option<u64>,
    /// Undo this turn put its files back.
    pub undone: bool,
    /// **Its to-do list**, as its latest `TodoWrite` left it (the Agent
    /// panel's row, the thread's to-do card). New fields of the turn's own
    /// (2026-11-08), as `usage`: an older Yap drops them on its save, and the
    /// turn then shows its log's list, or none.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub todos: Vec<super::steps::Todo>,
    /// **What it used**: its tokens, its context's fill, an estimate while it
    /// works (`agents/usage.rs`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<super::usage::TurnUsage>,
    /// **Plan not used** (2026-11-08): a plan nobody went ahead with before a
    /// newer turn began in its Code chat ([`set_plans_aside`]), or one the
    /// person put aside (the Agent panel's Not yet, Move to finished,
    /// Dismiss). It no longer waits on anyone: its row is in Finished, with
    /// Go ahead still in its details (`runner::go_ahead_turn`). A new field
    /// of the turn's own: an older Yap drops it on its save, and the next
    /// start sets it again (the plan isn't its chat's latest turn).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub plan_unused: bool,
    /// **Dismissed** from the Agent panel (its row's Dismiss): hidden there
    /// until "Show dismissed"; kept in the log and History. Never stops or
    /// changes the agent's work. A new field (an older Yap's save drops it:
    /// the row shows again).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub dismissed: bool,
}

impl TurnNote {
    /// It ran (or runs) in a Claude Code process.
    pub fn ran(&self) -> bool {
        self.started_ts > 0
    }

    /// **Its plan still waits for a go-ahead**: it ended as a plan, not
    /// allowed and not set aside. Pure.
    pub fn plan_waiting(&self) -> bool {
        // (A read-only turn reports what it found: never a plan to approve.)
        self.status == Status::Done && self.plan && !self.plan_allowed && !self.plan_unused && !self.read_only
    }
}

/// An agent as the page's tab opens it (`stream::Event::Agent`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRef {
    pub id: String,
    pub folder: String,
    pub status: Status,
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation: Option<u64>,
    /// The turn the tab opens at (its newest, else the one asked about).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn: Option<u32>,
}

impl Agent {
    pub fn reference(&self) -> AgentRef {
        AgentRef {
            id: self.id.clone(),
            folder: self.folder.clone(),
            status: self.status,
            mode: self.mode.clone(),
            conversation: self.conversation,
            turn: self.turn_list.last().map(|t| t.n),
        }
    }

    /// **Its turns**: the ones kept, or — an agent from before turns were kept
    /// (or saved by an older Yap) — one made from the agent itself. Pure.
    pub fn turn_notes(&self) -> Vec<TurnNote> {
        if !self.turn_list.is_empty() {
            return self.turn_list.clone();
        }
        vec![TurnNote {
            n: 1,
            title: title_of(&self.brief),
            request: self.brief.clone(),
            message_id: self.message_id.clone(),
            status: self.status,
            mode: self.mode.clone(),
            model: self.model.clone(),
            effort: self.effort.clone(),
            started_ts: self.started_ts,
            ended_ts: self.ended_ts,
            cost: self.cost,
            files: self.files.clone(),
            denials: self.denials.clone(),
            text: self.text.clone(),
            error: self.error.clone(),
            plan: self.plan,
            plan_allowed: self.plan_allowed,
            asks: self.asks,
            report_message_id: self.report_message_id.clone(),
            undone: self.undone,
            plan_unused: self.plan_unused,
            ..Default::default()
        }]
    }

    /// Its turns kept from now on ([`Agent::turn_notes`] written in).
    pub fn keep_turns(&mut self) {
        if self.turn_list.is_empty() {
            self.turn_list = self.turn_notes();
        }
    }

    /// Turn `n`, to change.
    pub fn turn_mut(&mut self, n: u32) -> Option<&mut TurnNote> {
        self.turn_list.iter_mut().find(|t| t.n == n)
    }

    /// The agent as its turn `n` left it: its status, words, files, denials,
    /// cost and errors that turn's (the report's facts are one turn's).
    /// Pure.
    pub fn as_of_turn(&self, n: u32) -> Agent {
        let Some(t) = self.turn_notes().into_iter().find(|t| t.n == n) else { return self.clone() };
        Agent {
            status: t.status,
            text: t.text,
            files: t.files,
            denials: t.denials,
            error: t.error,
            plan: t.plan,
            plan_allowed: t.plan_allowed,
            plan_unused: t.plan_unused,
            asks: t.asks,
            ..self.clone()
        }
    }

    /// **The turns whose reports wait for turn `n`'s** ([`TurnNote::report_held`]),
    /// oldest first: the ones before it the agent went straight on from. Pure.
    pub fn held_before(&self, n: u32) -> Vec<TurnNote> {
        self.turn_list.iter().filter(|t| t.n < n && t.report_held && t.report_message_id.is_none()).cloned().collect()
    }

    /// **The latest turn whose report still waits**, when there is one: what
    /// the agent finished and nobody was told (the turn after it was stopped).
    /// Pure.
    pub fn held_last(&self) -> Option<u32> {
        self.turn_list.iter().rev().find(|t| t.report_held && t.report_message_id.is_none()).map(|t| t.n)
    }

    /// **Its latest turn has ended and its chat hasn't been told**: done or
    /// failed, no report yet (it waits for the chat to be free). The turn.
    /// Pure.
    pub fn unreported_end(&self) -> Option<u32> {
        let t = self.turn_list.last()?;
        let ended = !self.status.live() && matches!(t.status, Status::Done | Status::Failed) && t.ran();
        (ended && !self.reported && self.conversation.is_some()).then_some(t.n)
    }

    /// **The agent as its report of turn `n` tells it** (one report when it
    /// stops working): turn `n`'s status, words and errors
    /// ([`Agent::as_of_turn`]), with the files and denials of the turns held
    /// for it first — and the turns it covers, oldest first, turn `n` last
    /// (one: a report of that turn alone). Pure.
    pub fn as_of_report(&self, n: u32) -> (Agent, Vec<TurnNote>) {
        let mut covered = self.held_before(n);
        let mut agent = self.as_of_turn(n);
        if !covered.is_empty() {
            let mut files: Vec<String> = Vec::new();
            for f in covered.iter().flat_map(|t| t.files.iter()).chain(agent.files.iter()) {
                if !files.contains(f) {
                    files.push(f.clone());
                }
            }
            agent.files = files;
            agent.denials = covered.iter().flat_map(|t| t.denials.iter().cloned()).chain(agent.denials).collect();
        }
        covered.extend(self.turn_notes().into_iter().find(|t| t.n == n));
        (agent, covered)
    }

    /// **Turn `n` was reported** (`marker`: its report's message; none: told
    /// without one, its chat gone): its report's, and the report of every
    /// turn that waited for it.
    pub fn told(&mut self, n: u32, marker: Option<&str>) {
        self.reported = true;
        if marker.is_some() {
            self.report_message_id = marker.map(str::to_string);
        }
        for t in self.turn_list.iter_mut().filter(|t| t.n == n || (t.n < n && t.report_held)) {
            t.report_held = false;
            if marker.is_some() {
                t.report_message_id = marker.map(str::to_string);
            }
        }
    }

    /// **Its plan waits for a go-ahead**: its latest turn ended as a plan, not
    /// allowed, not set aside, and it isn't at work. Pure.
    pub fn plan_waits(&self) -> bool {
        !self.status.live() && self.plan && !self.plan_allowed && !self.plan_unused && !self.turn_list.last().is_some_and(|t| t.read_only)
    }
}

/// **A request in a few words** (a turn's header): its first line, to its
/// first sentence's end, cut at a word under 60 characters. Pure.
pub fn title_of(text: &str) -> String {
    let line = text.trim().lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let line = line.trim_start_matches(['#', '-', '*', '>', ' ']);
    // A sentence ends at . ! ? : or ; with a space (or the line's end) after it.
    let end = line
        .char_indices()
        .find(|&(i, c)| {
            let rest = &line[i + c.len_utf8()..];
            matches!(c, '.' | '!' | '?' | ':' | ';') && (rest.is_empty() || rest.starts_with(char::is_whitespace))
        })
        .map_or(line.len(), |(i, _)| i);
    capped(line[..end].trim())
}

/// **A title given as one** (the model's `title` for a turn: "Plan: teaspoons
/// in convert.js"): its first line, whole — a colon or a full stop in it is
/// part of the title, not its end — cut at a word under 60 characters. Pure.
pub fn given_title(text: &str) -> String {
    let line = text.trim().lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    capped(line.trim_start_matches(['#', '-', '*', '>', ' ']).trim_end_matches(['.', ' ']))
}

/// `text` cut at a word under 60 characters, with an ellipsis. Pure.
fn capped(text: &str) -> String {
    const MAX: usize = 60;
    if text.chars().count() <= MAX {
        return text.to_string();
    }
    let cut: String = text.chars().take(MAX).collect();
    let at = cut.rfind(' ').filter(|&i| i > MAX / 2).unwrap_or(cut.len());
    format!("{}…", cut[..at].trim_end_matches([',', ' ', '-']))
}

/// What agents.json holds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Store {
    /// (`jobs` in a file from before the rename, 7 Nov 2026.)
    #[serde(alias = "jobs")]
    pub agents: Vec<Agent>,
    /// Each chat's Mode chip (conversation id → mode).
    pub modes: BTreeMap<u64, String>,
    /// Each Code chat's folder (conversation id → its folder, `code_chat.rs`):
    /// beside its Mode, never in chats.json.
    pub code_chats: BTreeMap<u64, CodeChat>,
    /// The Bypass warning was confirmed once.
    pub bypass_confirmed: bool,
}

/// **A Code chat**: the workspace its conversation is attached to.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CodeChat {
    /// The folder's full path (its name can change; its path is the folder).
    pub path: String,
    /// When it was attached (Unix seconds): the workspace picker's recency.
    pub ts: u64,
}

/// agents.json's saves.
pub static SAVER: crate::shell::saving::Saver = crate::shell::saving::Saver::new("agents", "your agents", save_now);

fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    with_store(|s| save(s));
    SAVER.check()
}

/// The data folder (a test thread's own in unit tests).
#[cfg(not(test))]
pub fn data() -> PathBuf {
    crate::config::data_dir()
}

#[cfg(test)]
thread_local! {
    static TEST_DATA: PathBuf = {
        let mut raw = [0u8; 6];
        let _ = getrandom::fill(&mut raw);
        let tag: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        let dir = std::env::temp_dir().join(format!("yap-agents-test-{tag}"));
        let _ = std::fs::create_dir_all(&dir);
        dir
    };
}

#[cfg(test)]
pub fn data() -> PathBuf {
    TEST_DATA.with(|d| d.clone())
}

#[cfg(not(test))]
fn path() -> PathBuf {
    super::ensure_moved();
    data().join("agents.json")
}

/// `agents/`: every agent's folder.
pub fn root() -> PathBuf {
    super::ensure_moved();
    data().join("agents")
}

/// An agent id is Yap's own: letters and digits only (never a path).
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 40 && id.chars().all(|c| c.is_ascii_alphanumeric())
}

/// `agents/<id>/`.
pub fn dir(id: &str) -> Option<PathBuf> {
    valid_id(id).then(|| root().join(id))
}

/// `agents/<id>/backup/`: every turn's backups (an agent from before turns had
/// one backup, here, read as its turn 1's: [`backup_dirs`]).
pub fn backup_dir(id: &str) -> Option<PathBuf> {
    dir(id).map(|d| d.join("backup"))
}

/// `agents/<id>/backup/<n>/`: turn `n`'s backups (first copy wins within it).
pub fn turn_backup_dir(id: &str, n: u32) -> Option<PathBuf> {
    backup_dir(id).map(|d| d.join(n.to_string()))
}

/// **Every turn's backup folders** on disk, oldest first: `(turn, folder)` —
/// an agent from before turns (its manifest right in `backup/`) as turn 1's,
/// before a `backup/1/` of its own (its turn 1 continued since). Only
/// folders with a manifest.
pub fn backup_dirs(id: &str) -> Vec<(u32, PathBuf)> {
    let Some(root) = backup_dir(id) else { return Vec::new() };
    let manifest = crate::integrations::claude_live::backup::MANIFEST;
    let mut out: Vec<(u32, PathBuf)> = Vec::new();
    if root.join(manifest).is_file() {
        out.push((1, root.clone()));
    }
    let mut turns: Vec<(u32, PathBuf)> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let n = name.parse::<u32>().ok().filter(|n| *n > 0 && name == n.to_string())?;
            e.path().join(manifest).is_file().then(|| (n, e.path()))
        })
        .collect();
    turns.sort_by_key(|(n, _)| *n);
    out.extend(turns);
    out
}

/// `agents/<id>/log.jsonl`.
pub fn log_path(id: &str) -> Option<PathBuf> {
    dir(id).map(|d| d.join("log.jsonl"))
}

/// A new agent's id: the time and four random bytes, in hex.
pub fn new_id() -> String {
    let mut raw = [0u8; 4];
    let _ = getrandom::fill(&mut raw);
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    format!("{ms:x}{}", raw.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(not(test))]
static STATE: Mutex<Option<Store>> = Mutex::new(None);

/// **agents.json as it's read**: the plans nobody acted on set aside
/// ([`set_plans_aside`]: a plan waiting for its go-ahead that isn't its
/// chat's latest turn is "Plan not used"). Whether anything changed. Pure.
pub fn migrate(store: &mut Store) -> bool {
    let n = set_plans_aside(&mut store.agents).len();
    if n > 0 {
        tracing::info!(n, "Agents: plans nobody acted on, set aside (Plan not used)");
    }
    n > 0
}

#[cfg(not(test))]
fn save(store: &Store) {
    if crate::ai::eval::active() {
        return;
    }
    let _ = SAVER.write_json(&path(), store, false);
}

#[cfg(not(test))]
fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let store = guard.get_or_insert_with(|| {
        let mut s: Store = SAVER.load(&path());
        // Plans nobody acted on, from before "Plan not used" (or an older
        // Yap's save, which drops it): set aside once, as they're read.
        if migrate(&mut s) {
            save(&s);
        }
        s
    });
    if SAVER.any_unread() && SAVER.refresh(&path(), store) {
        migrate(store);
        save(store);
    }
    f(store)
}

#[cfg(test)]
thread_local! {
    static TEST_STORE: std::cell::RefCell<Store> = std::cell::RefCell::new(Store::default());
}

#[cfg(test)]
fn save(_: &Store) {}

#[cfg(test)]
fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    TEST_STORE.with(|s| f(&mut s.borrow_mut()))
}

/// A failed save, for a command to say (`saving.rs`).
pub fn saved() -> Result<(), crate::shell::saving::SaveError> {
    SAVER.check()
}

// ---- agents ----

/// Every agent, newest first.
pub fn list() -> Vec<Agent> {
    with_store(|s| {
        let mut agents = s.agents.clone();
        agents.sort_by(|a, b| b.created_ts.cmp(&a.created_ts).then_with(|| b.id.cmp(&a.id)));
        agents
    })
}

pub fn get(id: &str) -> Option<Agent> {
    with_store(|s| s.agents.iter().find(|j| j.id == id).cloned())
}

/// A new agent, saved.
pub fn insert(agent: Agent) {
    with_store(|s| {
        s.agents.retain(|j| j.id != agent.id);
        s.agents.push(agent);
        save(s);
    });
}

/// Change agent `id` (`f`), stamped and saved: the agent as it is now.
pub fn update(id: &str, f: impl FnOnce(&mut Agent)) -> Option<Agent> {
    change(id, true, f)
}

/// [`update`], only in memory (a step's line, a file: saved with the next
/// [`update`] or [`flush`]).
pub fn touch(id: &str, f: impl FnOnce(&mut Agent)) -> Option<Agent> {
    change(id, false, f)
}

fn change(id: &str, write: bool, f: impl FnOnce(&mut Agent)) -> Option<Agent> {
    with_store(|s| {
        let agent = s.agents.iter_mut().find(|j| j.id == id)?;
        f(agent);
        agent.updated_ts = now_secs();
        let out = agent.clone();
        if write {
            save(s);
        }
        Some(out)
    })
}

/// Save what [`touch`] changed.
pub fn flush() {
    with_store(|s| save(s));
}

/// The agents not over yet (queued, running or asking), newest first: their
/// ids (the Yap bar's "1 agent working" and where its click goes).
pub fn live_ids() -> Vec<String> {
    with_store(|s| {
        let mut live: Vec<&Agent> = s.agents.iter().filter(|j| j.status.live()).collect();
        live.sort_by(|a, b| b.created_ts.cmp(&a.created_ts).then_with(|| b.id.cmp(&a.id)));
        live.into_iter().map(|j| j.id.clone()).collect()
    })
}

/// The agents a chat started, newest first.
pub fn of_conversation(conversation: u64) -> Vec<Agent> {
    list().into_iter().filter(|j| j.conversation == Some(conversation)).collect()
}

/// **A chat deleted**: its agents keep no chat (a later conversation may get
/// its id, and must never carry them on or take their reports). How many.
pub fn detach(conversation: u64) -> usize {
    with_store(|s| {
        let mut n = 0;
        for j in s.agents.iter_mut().filter(|j| j.conversation == Some(conversation)) {
            // (Its messages' ids would name the new chat's.)
            j.conversation = None;
            j.message_id = None;
            j.report_message_id = None;
            for t in j.turn_list.iter_mut() {
                t.message_id = None;
                t.report_message_id = None;
                // (No chat to report in: nothing waits for a report.)
                t.report_held = false;
            }
            n += 1;
        }
        if n > 0 {
            save(s);
        }
        n
    })
}

// ---- plans nobody acted on, and rows dismissed ----

/// **Plans nobody acted on, set aside** ("Plan not used", 2026-11-08: the
/// Agent panel kept an old plan in Needs you for ever, below a newer turn
/// already running). A turn whose plan still waits for its go-ahead
/// ([`TurnNote::plan_waiting`]) once a newer turn has begun in its chat — a
/// later turn of the same agent, or another agent of the same conversation
/// made or started since the plan was ready — is marked `plan_unused` (its
/// agent too, when it's the agent's latest turn). A later turn that was the
/// plan's own go-ahead ("Go ahead with the plan.", the person's "go ahead":
/// `consent::go_ahead`) marks it allowed instead: it was carried out. Run
/// at start (plans from before this rule, or saved by an older Yap, which
/// drops the field) and whenever a turn begins ([`plans_aside`]). The ids
/// of the agents changed. Pure.
pub fn set_plans_aside(agents: &mut [Agent]) -> Vec<String> {
    // Every agent's chat, when it was made, and when each turn started.
    let all: Vec<(Option<u64>, String, u64, Vec<u64>)> =
        agents.iter().map(|a| (a.conversation, a.id.clone(), a.created_ts, a.turn_notes().iter().map(|t| t.started_ts).collect())).collect();
    let mut changed = Vec::new();
    for a in agents.iter_mut() {
        if !a.turn_notes().iter().any(TurnNote::plan_waiting) {
            continue;
        }
        a.keep_turns();
        let mut touched = false;
        let count = a.turn_list.len();
        for i in 0..count {
            if !a.turn_list[i].plan_waiting() {
                continue;
            }
            let ready = a.turn_list[i].ended_ts.max(a.turn_list[i].started_ts);
            let next = a.turn_list.get(i + 1);
            let went_ahead = next.is_some_and(|n| super::consent::go_ahead(&n.request));
            let newer_here = next.is_some();
            let newer_elsewhere = ready > 0
                && a.conversation.is_some()
                && all.iter().any(|(c, id, made, starts)| *c == a.conversation && *id != a.id && (*made >= ready || starts.iter().any(|s| *s > 0 && *s >= ready)));
            let latest = i + 1 == count;
            let t = &mut a.turn_list[i];
            if went_ahead {
                t.plan_allowed = true;
            } else if newer_here || newer_elsewhere {
                t.plan_unused = true;
                if latest {
                    a.plan_unused = true;
                }
            } else {
                continue;
            }
            touched = true;
        }
        if touched {
            changed.push(a.id.clone());
        }
    }
    changed
}

/// **A turn began** (or a new agent was made): the plans in its chat nobody
/// acted on set aside ([`set_plans_aside`]), saved. The agents changed, as
/// they are now.
pub fn plans_aside() -> Vec<Agent> {
    with_store(|s| {
        let ids = set_plans_aside(&mut s.agents);
        if ids.is_empty() {
            return Vec::new();
        }
        save(s);
        s.agents.iter().filter(|a| ids.contains(&a.id)).cloned().collect()
    })
}

/// **Turn `n` of agent `id` dismissed** from the Agent panel (`on`), or
/// shown again: hidden there, kept in its log and History. A plan still
/// waiting, dismissed, no longer waits (Plan not used). The agent as it is
/// now.
pub fn dismiss(id: &str, n: u32, on: bool) -> Result<Agent, String> {
    let mut found = false;
    let agent = update(id, |j| {
        j.keep_turns();
        let latest = j.turn_list.last().map(|t| t.n) == Some(n);
        let Some(t) = j.turn_mut(n) else { return };
        found = true;
        t.dismissed = on;
        if on && t.plan_waiting() {
            t.plan_unused = true;
            if latest {
                j.plan_unused = true;
            }
        }
    })
    .ok_or("There's no such agent.")?;
    if !found {
        return Err("That turn isn't the agent's.".into());
    }
    Ok(agent)
}

// ---- each chat's Mode ----

/// The Mode chip's mode for `conversation` (`default` when it was never set).
pub fn mode_of(conversation: Option<u64>, default: &str) -> String {
    let set = conversation.and_then(|c| with_store(|s| s.modes.get(&c).cloned()));
    set.unwrap_or_else(|| default.to_string())
}

pub fn set_mode(conversation: u64, mode: &str) {
    with_store(|s| {
        s.modes.insert(conversation, mode.to_string());
        save(s);
    });
}

// ---- each Code chat's folder ----

/// The folder `conversation` is a Code chat in (`None`: a plain chat).
pub fn code_chat_of(conversation: u64) -> Option<CodeChat> {
    with_store(|s| s.code_chats.get(&conversation).cloned())
}

/// Attach `conversation` to the folder at `path` (`None`: a plain chat
/// again). Whether anything changed.
pub fn set_code_chat(conversation: u64, path: Option<&str>) -> bool {
    with_store(|s| {
        let changed = match path {
            Some(p) => {
                let same = s.code_chats.get(&conversation).is_some_and(|c| super::workspaces::norm(&c.path) == super::workspaces::norm(p));
                if !same {
                    s.code_chats.insert(conversation, CodeChat { path: p.to_string(), ts: now_secs() });
                }
                !same
            }
            None => s.code_chats.remove(&conversation).is_some(),
        };
        if changed {
            save(s);
        }
        changed
    })
}

/// Every Code chat (conversation id → its folder).
pub fn code_chats() -> BTreeMap<u64, CodeChat> {
    with_store(|s| s.code_chats.clone())
}

/// The Code chats in the folder at `path` turned plain: their ids.
pub fn drop_code_chats_in(path: &str) -> Vec<u64> {
    let want = super::workspaces::norm(path);
    with_store(|s| {
        let gone: Vec<u64> = s.code_chats.iter().filter(|(_, c)| super::workspaces::norm(&c.path) == want).map(|(id, _)| *id).collect();
        if !gone.is_empty() {
            s.code_chats.retain(|id, _| !gone.contains(id));
            save(s);
        }
        gone
    })
}

pub fn bypass_confirmed() -> bool {
    with_store(|s| s.bypass_confirmed)
}

pub fn confirm_bypass() {
    with_store(|s| {
        if !s.bypass_confirmed {
            s.bypass_confirmed = true;
            save(s);
        }
    });
}

// ---- at start, and the sweep ----

/// **Yap is starting**: an agent still queued, running or asking died with the
/// last Yap — interrupted now. How many.
pub fn at_start() -> usize {
    let now = now_secs();
    with_store(|s| {
        let n = interrupt_live(&mut s.agents, now);
        if n > 0 {
            save(s);
        }
        n
    })
}

/// Every live agent in `agents` interrupted at `now` (and its turns not over);
/// how many. Pure.
pub fn interrupt_live(agents: &mut [Agent], now: u64) -> usize {
    let mut n = 0;
    for j in agents.iter_mut().filter(|j| j.status.live()) {
        interrupt(j, now);
        n += 1;
    }
    n
}

/// Agent `j` interrupted at `now`, with its turns not over (a request waiting
/// behind the turn that ran too: Continue sends it). Pure.
pub fn interrupt(j: &mut Agent, now: u64) {
    j.status = Status::Interrupted;
    j.ended_ts = now;
    j.updated_ts = now;
    for t in j.turn_list.iter_mut().filter(|t| t.status.live()) {
        t.status = Status::Interrupted;
        if t.ran() {
            t.ended_ts = now;
        }
    }
}

/// The agents whose backups and log are due to go at `now`: ended over
/// [`KEEP_SECS`] ago, not swept yet, not live. Pure.
pub fn due_for_sweep(agents: &[Agent], now: u64) -> Vec<String> {
    agents.iter()
        .filter(|j| !j.status.live() && !j.swept)
        .filter(|j| {
            let ended = if j.ended_ts > 0 { j.ended_ts } else { j.updated_ts.max(j.created_ts) };
            now.saturating_sub(ended) >= KEEP_SECS
        })
        .map(|j| j.id.clone())
        .collect()
}

/// **Backups and logs older than 7 days go** (their agent's record stays,
/// marked swept), and a workspace no agent names, once it's that old. How
/// many folders went.
pub fn sweep(now: u64) -> usize {
    let (due, known): (Vec<String>, Vec<String>) = with_store(|s| (due_for_sweep(&s.agents, now), s.agents.iter().map(|j| j.id.clone()).collect()));
    let mut gone = 0;
    for id in &due {
        if let Some(d) = dir(id) {
            if d.exists() && std::fs::remove_dir_all(&d).is_ok() {
                gone += 1;
            }
        }
    }
    if !due.is_empty() {
        with_store(|s| {
            for j in s.agents.iter_mut().filter(|j| due.contains(&j.id)) {
                j.swept = true;
            }
            save(s);
        });
    }
    // Folders of agents no longer in the store (agents.json deleted by hand, an
    // older Yap's): only once they're as old.
    for e in std::fs::read_dir(root()).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if known.contains(&name) || !e.path().is_dir() {
            continue;
        }
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age.as_secs() >= KEEP_SECS);
        if old && std::fs::remove_dir_all(e.path()).is_ok() {
            gone += 1;
        }
    }
    if gone > 0 {
        tracing::info!(folders = gone, "Agents: backups and logs older than 7 days deleted");
    }
    gone
}

// ---- the log ----

/// Append `line` (one `yap-agent` event) to agent `id`'s log, its texts
/// trimmed; nothing past [`LOG_MAX_LINES`].
pub fn log_append(id: &str, line: &Value) -> Option<u64> {
    let path = log_path(id)?;
    if crate::ai::eval::active() {
        return None;
    }
    let mut line = line.clone();
    if let Some(o) = line.as_object_mut() {
        for v in o.values_mut() {
            if let Some(s) = v.as_str() {
                if s.chars().count() > LOG_TEXT_MAX {
                    *v = Value::String(super::steps::clip(s, LOG_TEXT_MAX));
                }
            }
        }
    }
    let count = LOG_LINES.with_count(id, &path);
    if count >= LOG_MAX_LINES {
        return None;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let wrote = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| f.write_all(format!("{line}\n").as_bytes()));
    match wrote {
        Ok(()) => {
            LOG_LINES.bump(id);
            // Its line in the file, from 1 (Remote Control numbers the
            // phone's events by it: `link/lanes.rs`).
            Some(count as u64 + 1)
        }
        Err(e) => {
            tracing::debug!("Agents: a log line couldn't be written ({})", e.kind());
            None
        }
    }
}

/// Agent `id`'s log, oldest first (lines that don't read are left out).
pub fn log_read(id: &str) -> Vec<Value> {
    log_read_numbered(id).into_iter().map(|(_, v)| v).collect()
}

/// Agent `id`'s log with each event's line in the file (from 1), oldest
/// first; lines that don't read are left out, their numbers kept.
pub fn log_read_numbered(id: &str) -> Vec<(u64, Value)> {
    let Some(path) = log_path(id) else { return Vec::new() };
    std::fs::read_to_string(path)
        .map(|s| s.lines().enumerate().filter_map(|(i, l)| serde_json::from_str::<Value>(l).ok().map(|v| (i as u64 + 1, v))).collect())
        .unwrap_or_default()
}

/// How many lines each agent's log has (counted once from its file).
struct LogLines(std::sync::Mutex<Option<std::collections::HashMap<String, usize>>>);

static LOG_LINES: LogLines = LogLines(std::sync::Mutex::new(None));

impl LogLines {
    fn with_count(&self, id: &str, path: &Path) -> usize {
        let mut m = self.0.lock().unwrap_or_else(|p| p.into_inner());
        *m.get_or_insert_with(std::collections::HashMap::new)
            .entry(id.to_string())
            .or_insert_with(|| std::fs::read_to_string(path).map(|s| s.lines().count()).unwrap_or(0))
    }

    fn bump(&self, id: &str) {
        if let Some(n) = self.0.lock().unwrap_or_else(|p| p.into_inner()).as_mut().and_then(|m| m.get_mut(id)) {
            *n += 1;
        }
    }

    fn forget_all(&self) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
}

// ---- deleting your data ----

/// **Delete some of your data → Agents** (`privacy/reset.rs`): every agent, its
/// log and backups, each chat's Mode and Code chat folder, and the Bypass
/// OK. How many agents went.
pub fn reset() -> usize {
    let n = with_store(|s| {
        let n = s.agents.len();
        *s = Store::default();
        save(s);
        n
    });
    LOG_LINES.forget_all();
    let root = root();
    if root.exists() {
        if let Err(e) = std::fs::remove_dir_all(&root) {
            tracing::warn!("Agents: the agents folder wasn't all deleted ({})", e.kind());
        }
    }
    n
}

/// The bytes the agents' folders take (logs and backups).
pub fn disk_bytes() -> u64 {
    fn walk(p: &Path) -> u64 {
        std::fs::read_dir(p)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| match e.file_type() {
                Ok(t) if t.is_dir() => walk(&e.path()),
                Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
                _ => 0,
            })
            .sum()
    }
    walk(&root())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn agent(id: &str, status: Status, ended: u64) -> Agent {
        Agent { id: id.into(), status, ended_ts: ended, created_ts: 1, folder: "garden-planner".into(), ..Default::default() }
    }

    #[test]
    fn an_agent_round_trips_and_an_older_file_reads() {
        let j = Agent {
            id: "18c1f2a3b4d5e6f7".into(),
            conversation: Some(12),
            message_id: Some("chat:12:3".into()),
            origin: Origin::Voice,
            folder: "garden-planner".into(),
            path: r"D:\Projects\garden-planner".into(),
            brief: "Tidy the notes.".into(),
            model: "claude-sonnet-5-5".into(),
            effort: "medium".into(),
            mode: "auto".into(),
            pay: "plan".into(),
            session: "4d7c1d2e-8a51-4c3b-9f0e-2b6a7e1c5d90".into(),
            status: Status::Done,
            files: vec!["notes.md".into()],
            commands: vec!["npm test".into()],
            denials: vec![Denial { tool: "Bash".into(), detail: "Run a command: rm -rf build".into() }],
            text: "Done.".into(),
            cost: 0.041,
            problem: Some(ProblemNote { kind: "rateLimited".into(), message: "x".into(), resets_at: Some(5) }),
            step: "Reading notes.md".into(),
            ..Default::default()
        };
        let v = serde_json::to_value(&j).unwrap();
        assert_eq!(v["origin"], "voice");
        assert_eq!(v["status"], "done");
        assert_eq!(v["messageId"], "chat:12:3");
        assert!(v.get("step").is_none(), "the live line isn't kept");
        let back: Agent = serde_json::from_value(v).unwrap();
        assert_eq!(back, Agent { step: String::new(), ..j });
        // A file from before a field existed reads, with defaults.
        let old: Store = serde_json::from_str(r#"{"agents":[{"id":"ab12","status":"running"}],"modes":{"7":"plan"}}"#).unwrap();
        assert_eq!(old.agents[0].status, Status::Running);
        assert_eq!(old.modes.get(&7).map(String::as_str), Some("plan"));
        assert!(!old.bypass_confirmed);
        assert!(old.code_chats.is_empty(), "a file from before Code chats: every chat plain");
    }

    #[test]
    fn each_code_chats_folder_is_kept_beside_its_mode() {
        assert_eq!(code_chat_of(9), None);
        assert!(set_code_chat(9, Some(r"D:\Projects\garden-planner")));
        assert!(!set_code_chat(9, Some(r"d:/projects/garden-planner/")), "the same folder: no change");
        assert_eq!(code_chat_of(9).unwrap().path, r"D:\Projects\garden-planner");
        assert!(set_code_chat(10, Some(r"D:\Projects\recipe-box")));
        assert!(set_code_chat(11, Some(r"D:\Projects\garden-planner")));
        // Saved as agents.json has it: its own map, by conversation.
        let v = with_store(|s| serde_json::to_value(&*s).unwrap());
        assert_eq!(v["codeChats"]["9"]["path"], r"D:\Projects\garden-planner");
        // A folder removed: its chats go back to plain, the others stay.
        let mut gone = drop_code_chats_in(r"D:\Projects\garden-planner\");
        gone.sort();
        assert_eq!(gone, [9, 11]);
        assert_eq!(code_chats().keys().copied().collect::<Vec<_>>(), [10]);
        assert!(set_code_chat(10, None));
        assert!(!set_code_chat(10, None));
        assert!(code_chats().is_empty());
    }

    #[test]
    fn agents_live_at_start_are_interrupted() {
        let mut agents = vec![agent("a", Status::Running, 0), agent("b", Status::Done, 5), agent("c", Status::Queued, 0), agent("d", Status::Asking, 0)];
        assert_eq!(interrupt_live(&mut agents, 100), 3);
        assert_eq!(agents.iter().map(|j| j.status).collect::<Vec<_>>(), [Status::Interrupted, Status::Done, Status::Interrupted, Status::Interrupted]);
        assert_eq!(agents[0].ended_ts, 100);
        assert_eq!(agents[1].ended_ts, 5, "an ended one is left as it was");
        // Through the store.
        insert(agent("r1", Status::Running, 0));
        assert_eq!(at_start(), 1);
        assert_eq!(get("r1").unwrap().status, Status::Interrupted);
        assert_eq!(at_start(), 0);
    }

    #[test]
    fn backups_and_logs_go_after_seven_days_and_the_record_stays() {
        let now = 1_800_000_000;
        let agents = vec![
            agent("old", Status::Done, now - KEEP_SECS - 1),
            agent("new", Status::Done, now - KEEP_SECS + 60),
            agent("live", Status::Running, 0),
            Agent { swept: true, ..agent("gone", Status::Failed, 1) },
        ];
        assert_eq!(due_for_sweep(&agents, now), ["old"]);
        // On disk: the old one's folder goes, the new one's stays.
        for j in &agents {
            insert(j.clone());
            std::fs::create_dir_all(backup_dir(&j.id).unwrap()).unwrap();
            log_append(&j.id, &json!({ "status": "running" }));
        }
        assert_eq!(sweep(now), 1);
        assert!(!dir("old").unwrap().exists());
        assert!(dir("new").unwrap().exists() && dir("live").unwrap().exists());
        let old = get("old").unwrap();
        assert!(old.swept, "the record stays, marked");
        assert_eq!(due_for_sweep(&list(), now), Vec::<String>::new());
    }

    #[test]
    fn the_log_appends_trimmed_lines_and_reads_them_back() {
        insert(agent("logged", Status::Running, 0));
        log_append("logged", &json!({ "id": "logged", "step": "Reading notes.md" }));
        log_append("logged", &json!({ "id": "logged", "text": "x".repeat(LOG_TEXT_MAX + 50) }));
        let lines = log_read("logged");
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["step"], "Reading notes.md");
        assert_eq!(lines[1]["text"].as_str().unwrap().chars().count(), LOG_TEXT_MAX);
        // Never a path from an id.
        assert!(dir("..").is_none() && dir(r"a\b").is_none() && dir("").is_none());
        assert!(log_read("../x").is_empty());
    }

    #[test]
    fn an_agents_turns_are_kept_in_a_field_of_their_own_and_an_older_agent_is_one_turn() {
        let j = Agent {
            id: "ab12".into(),
            brief: "Add a watering schedule to the planner. Then tidy notes.md.".into(),
            status: Status::Done,
            text: "Done.".into(),
            files: vec!["notes.md".into()],
            message_id: Some("chat:3:0".into()),
            report_message_id: Some("chat:3:2".into()),
            ..Default::default()
        };
        // Before turns were kept (or saved by an older Yap): one, from the agent.
        let one = j.turn_notes();
        assert_eq!(one.len(), 1);
        assert_eq!((one[0].n, one[0].title.as_str(), one[0].status, one[0].text.as_str()), (1, "Add a watering schedule to the planner", Status::Done, "Done."));
        assert_eq!((one[0].message_id.as_deref(), one[0].report_message_id.as_deref()), (Some("chat:3:0"), Some("chat:3:2")));
        assert_eq!(one[0].files, ["notes.md"]);
        // Kept from now on; a second turn beside it.
        let mut j = j;
        j.keep_turns();
        j.turn_list.push(TurnNote { n: 2, title: "Add a minimap button".into(), status: Status::Failed, text: "It broke.".into(), files: vec!["map.js".into()], error: Some("Claude Code stopped.".into()), ..Default::default() });
        let v = serde_json::to_value(&j).unwrap();
        assert_eq!(v["turnList"][1]["title"], "Add a minimap button");
        assert_eq!(v["turnList"][0]["messageId"], "chat:3:0");
        let back: Agent = serde_json::from_value(v).unwrap();
        assert_eq!(back.turn_list, j.turn_list);
        // The agent as a turn left it: the report's facts are that turn's.
        let second = j.as_of_turn(2);
        assert_eq!((second.status, second.text.as_str(), second.files.clone(), second.error.as_deref()), (Status::Failed, "It broke.", vec!["map.js".to_string()], Some("Claude Code stopped.")));
        assert_eq!(j.as_of_turn(1).files, ["notes.md"]);
        // A report of one turn: that turn's facts, nothing held for it.
        let (alone, covered) = j.as_of_report(2);
        assert_eq!((alone.files.clone(), covered.iter().map(|t| t.n).collect::<Vec<_>>()), (vec!["map.js".to_string()], vec![2]));
        assert!(!serde_json::to_string(&j).unwrap().contains("reportHeld"), "left out while false");
        // An older Yap's file (no turnList): read, and its agent one turn.
        let old: Agent = serde_json::from_str(r#"{"id":"cd34","brief":"Tidy it","status":"done","turns":3}"#).unwrap();
        assert!(old.turn_list.is_empty());
        assert_eq!(old.turn_notes().len(), 1);
        assert_eq!(old.reference().turn, None);
        assert_eq!(j.reference().turn, Some(2));
    }

    #[test]
    fn an_agent_that_has_ended_and_not_told_its_chat_yet() {
        let turn = |n: u32, status: Status| TurnNote { n, status, started_ts: 1, ..Default::default() };
        let j = Agent { id: "ab12".into(), conversation: Some(4), status: Status::Done, turn_list: vec![TurnNote { report_message_id: Some("chat:4:2".into()), ..turn(1, Status::Done) }, turn(2, Status::Done)], ..Default::default() };
        // Its latest turn is done and its report is still to come.
        assert_eq!(j.unreported_end(), Some(2));
        assert_eq!(Agent { status: Status::Failed, turn_list: vec![turn(1, Status::Failed)], ..j.clone() }.unreported_end(), Some(1));
        // Reported, at work, stopped, never ran, or with no chat: none.
        assert_eq!(Agent { reported: true, ..j.clone() }.unreported_end(), None);
        assert_eq!(Agent { status: Status::Running, turn_list: vec![turn(1, Status::Running)], ..j.clone() }.unreported_end(), None);
        assert_eq!(Agent { status: Status::Stopped, turn_list: vec![turn(1, Status::Stopped)], ..j.clone() }.unreported_end(), None);
        assert_eq!(Agent { turn_list: vec![TurnNote { started_ts: 0, ..turn(1, Status::Done) }], ..j.clone() }.unreported_end(), None);
        assert_eq!(Agent { conversation: None, ..j.clone() }.unreported_end(), None);
        assert_eq!(Agent { turn_list: Vec::new(), ..j.clone() }.unreported_end(), None);
        // When Chat's answer read it: kept with the turn, left out while unset.
        let mut told = j.clone();
        told.turn_list[1].told_at = Some(3);
        let v = serde_json::to_value(&told).unwrap();
        assert_eq!(v["turnList"][1]["toldAt"], 3);
        assert!(v["turnList"][0].get("toldAt").is_none());
        assert_eq!(serde_json::from_value::<Agent>(v).unwrap().turn_list[1].told_at, Some(3));
    }

    #[test]
    fn a_turn_the_agent_went_straight_on_from_is_in_the_next_report() {
        let turn = |n: u32, files: &[&str], text: &str| TurnNote { n, title: format!("Request {n}"), status: Status::Done, started_ts: 1, text: text.into(), files: files.iter().map(|f| f.to_string()).collect(), ..Default::default() };
        let mut j = Agent {
            id: "ab12".into(),
            status: Status::Done,
            turn_list: vec![
                TurnNote { report_message_id: Some("chat:3:2".into()), ..turn(1, &["old.md"], "First.") },
                TurnNote { report_held: true, denials: vec![Denial { tool: "Bash".into(), detail: "Run a command: npm publish".into() }], ..turn(2, &["beds.js", "notes.md"], "Lined the beds up.") },
                turn(3, &["notes.md", "legend.md"], "That needed no change."),
            ],
            ..Default::default()
        };
        // Turn 3's report: its own facts, and turn 2's before them — never
        // turn 1's, reported already.
        assert_eq!(j.held_last(), Some(2));
        let (agent, covered) = j.as_of_report(3);
        assert_eq!(covered.iter().map(|t| t.n).collect::<Vec<_>>(), [2, 3]);
        assert_eq!((agent.status, agent.text.as_str()), (Status::Done, "That needed no change."));
        assert_eq!(agent.files, ["beds.js", "notes.md", "legend.md"], "each file once, in the order they changed");
        assert_eq!(agent.denials.len(), 1);
        // Kept, and read back.
        let back: Agent = serde_json::from_value(serde_json::to_value(&j).unwrap()).unwrap();
        assert!(back.turn_list[1].report_held && !back.turn_list[2].report_held);
        // Told: one report's message for both, and nothing waits any more.
        j.told(3, Some("chat:3:6"));
        assert_eq!(j.turn_list.iter().map(|t| (t.report_held, t.report_message_id.as_deref())).collect::<Vec<_>>(), [(false, Some("chat:3:2")), (false, Some("chat:3:6")), (false, Some("chat:3:6"))]);
        assert_eq!((j.reported, j.report_message_id.as_deref(), j.held_last()), (true, Some("chat:3:6"), None));
        assert_eq!(j.as_of_report(3).1.len(), 1, "a report of that turn alone from now on");
        // The turn after a held one stopped: the held one is reported itself.
        let mut stopped = Agent { turn_list: vec![TurnNote { report_held: true, ..turn(1, &["a.md"], "Done.") }, TurnNote { status: Status::Stopped, ..turn(2, &[], "") }], ..Default::default() };
        assert_eq!(stopped.held_last(), Some(1));
        let (agent, covered) = stopped.as_of_report(1);
        assert_eq!((agent.files.clone(), covered.len()), (vec!["a.md".to_string()], 1));
        // Told with its chat gone: nothing waits, no message kept.
        stopped.told(1, None);
        assert_eq!((stopped.held_last(), stopped.turn_list[0].report_message_id.clone()), (None, None));
    }

    #[test]
    fn a_request_in_a_few_words() {
        assert_eq!(title_of("Add a minimap button to the map view. It should show every bed."), "Add a minimap button to the map view");
        assert_eq!(title_of("could you add a minimap button for it as well?"), "could you add a minimap button for it as well");
        assert_eq!(title_of("\n\n# Fix the tests: they fail on CI\nmore"), "Fix the tests");
        assert_eq!(title_of("Use v2.1 of the planner"), "Use v2.1 of the planner", "a dot inside a word isn't a sentence's end");
        let long = title_of("Rework the watering schedule so that every bed in the planner gets its own reminder and colour");
        assert!(long.ends_with('…') && long.chars().count() <= 61, "{long}");
        assert!(long.starts_with("Rework the watering schedule so that every bed"), "{long}");
        assert_eq!(title_of(""), "");
        // A title given as one keeps its colon (Yap Lab, 8 Nov: a plan's row read just "Plan").
        assert_eq!(given_title("Plan: teaspoons in convert.js"), "Plan: teaspoons in convert.js");
        assert_eq!(given_title("  # Add a legend to the map.\nmore"), "Add a legend to the map");
        assert!(given_title(&"Plan: ".repeat(20)).ends_with('…'));
    }

    #[test]
    fn interrupted_with_its_turns_and_a_request_still_waiting() {
        let mut j = Agent {
            status: Status::Running,
            turn_list: vec![
                TurnNote { n: 1, status: Status::Running, started_ts: 5, ..Default::default() },
                TurnNote { n: 2, status: Status::Queued, ..Default::default() },
            ],
            ..Default::default()
        };
        interrupt(&mut j, 50);
        assert_eq!(j.status, Status::Interrupted);
        assert_eq!(j.turn_list.iter().map(|t| (t.status, t.ended_ts)).collect::<Vec<_>>(), [(Status::Interrupted, 50), (Status::Interrupted, 0)]);
        assert!(!j.turn_list[1].ran(), "never sent: Continue sends it");
    }

    /// A plan turn that ended at `ended` in conversation `conv`.
    fn planned(id: &str, conv: u64, created: u64, ended: u64) -> Agent {
        Agent {
            id: id.into(),
            conversation: Some(conv),
            status: Status::Done,
            plan: true,
            created_ts: created,
            ended_ts: ended,
            turn_list: vec![TurnNote { n: 1, title: "Read only, don't change any files".into(), status: Status::Done, plan: true, started_ts: created + 1, ended_ts: ended, ..Default::default() }],
            ..Default::default()
        }
    }

    #[test]
    fn plan_not_used_and_dismissed_are_kept_and_an_older_file_reads_without_them() {
        let mut j = planned("ab12", 3, 10, 20);
        j.plan_unused = true;
        j.turn_list[0].plan_unused = true;
        j.turn_list[0].dismissed = true;
        let v = serde_json::to_value(&j).unwrap();
        assert_eq!((v["planUnused"].as_bool(), v["turnList"][0]["planUnused"].as_bool(), v["turnList"][0]["dismissed"].as_bool()), (Some(true), Some(true), Some(true)));
        let back: Agent = serde_json::from_value(v).unwrap();
        assert_eq!(back, j);
        // Left out while false, so a file reads as before.
        let plain = serde_json::to_value(planned("cd34", 3, 10, 20)).unwrap();
        assert!(plain.get("planUnused").is_none() && plain["turnList"][0].get("planUnused").is_none() && plain["turnList"][0].get("dismissed").is_none());
        // An older Yap's file (no such fields): read as waiting, not dismissed.
        let old: Agent = serde_json::from_str(r#"{"id":"ef56","status":"done","plan":true,"turnList":[{"n":1,"status":"done","plan":true}]}"#).unwrap();
        assert!(!old.plan_unused && !old.turn_list[0].plan_unused && !old.turn_list[0].dismissed);
        assert!(old.plan_waits() && old.turn_list[0].plan_waiting());
        // A read-only turn ("catch up", "have a look") is never a plan waiting for a go-ahead.
        let mut looked = planned("rd01", 5, 10, 20);
        looked.turn_list[0].read_only = true;
        assert!(!looked.plan_waits() && !looked.turn_list[0].plan_waiting());
        let later = Agent { id: "rd02".into(), conversation: Some(5), created_ts: 99, ..Default::default() };
        assert!(set_plans_aside(&mut [looked.clone(), later.clone()]).is_empty(), "nothing to set aside");
        assert_eq!(set_plans_aside(&mut [planned("rd03", 5, 10, 20), later]), ["rd03"], "(as a plan would be)");
        // `active` is the runner's, for the page: never read from, nor written to, the file.
        let read: Agent = serde_json::from_str(r#"{"id":"gh78","active":true}"#).unwrap();
        assert!(!read.active);
        assert!(serde_json::to_value(Agent { active: true, ..read.clone() }).unwrap()["active"] == true);
        assert!(serde_json::to_value(&read).unwrap().get("active").is_none());
    }

    #[test]
    fn a_plan_nobody_acted_on_is_set_aside_once_a_newer_turn_begins_in_its_chat() {
        // The maintainer's panel (8 Nov): a plan in one agent, a newer agent
        // in the same Code chat already running.
        let stale = planned("aa11", 7, 100, 200);
        let newer = Agent { id: "bb22".into(), conversation: Some(7), status: Status::Running, created_ts: 300, turn_list: vec![TurnNote { n: 1, status: Status::Running, started_ts: 301, ..Default::default() }], ..Default::default() };
        // Another chat's plan, and a plan with nothing newer: both still wait.
        let elsewhere = planned("cc33", 8, 100, 200);
        let latest = planned("dd44", 9, 100, 200);
        let mut agents = vec![stale, newer, elsewhere, latest];
        assert_eq!(set_plans_aside(&mut agents), ["aa11"]);
        assert!(agents[0].plan_unused && agents[0].turn_list[0].plan_unused, "set aside, the agent too (its latest turn)");
        assert!(!agents[0].plan_waits() && agents[0].plan, "still a plan: Go ahead stays in its details");
        assert!(agents[2].plan_waits() && agents[3].plan_waits());
        assert!(set_plans_aside(&mut agents).is_empty(), "once");
        // An agent of the chat made before the plan was ready, whose turn
        // started after it: newer too.
        let mut agents = vec![planned("ee55", 7, 100, 200), Agent { id: "ff66".into(), conversation: Some(7), created_ts: 50, turn_list: vec![TurnNote { n: 2, started_ts: 250, ..Default::default() }], ..Default::default() }];
        assert_eq!(set_plans_aside(&mut agents), ["ee55"]);
        // …but not one whose last turn started before (still at work on it).
        let mut agents = vec![planned("gg77", 7, 100, 200), Agent { id: "hh88".into(), conversation: Some(7), status: Status::Running, created_ts: 50, turn_list: vec![TurnNote { n: 1, started_ts: 60, status: Status::Running, ..Default::default() }], ..Default::default() }];
        assert!(set_plans_aside(&mut agents).is_empty());
        assert!(agents[0].plan_waits());
        // The same agent: a later turn came after its plan.
        let mut one = planned("ii99", 7, 100, 200);
        one.plan = false;
        one.turn_list.push(TurnNote { n: 2, request: "Bolder bag markers on the map".into(), status: Status::Running, started_ts: 300, ..Default::default() });
        one.status = Status::Running;
        let mut agents = vec![one];
        assert_eq!(set_plans_aside(&mut agents), ["ii99"]);
        assert!(agents[0].turn_list[0].plan_unused && !agents[0].plan_unused, "the turn, not the agent (its latest is the newer one)");
        // …unless the later turn was the plan's own go-ahead: used, allowed.
        let mut went = planned("jj00", 7, 100, 200);
        went.turn_list.push(TurnNote { n: 2, request: "Go ahead with the plan.".into(), status: Status::Done, started_ts: 300, ended_ts: 400, ..Default::default() });
        let mut typed = planned("kk11", 8, 100, 200);
        typed.turn_list.push(TurnNote { n: 2, request: "Yep, go ahead".into(), status: Status::Done, started_ts: 300, ended_ts: 400, ..Default::default() });
        let mut agents = vec![went, typed];
        assert_eq!(set_plans_aside(&mut agents), ["jj00", "kk11"]);
        assert!(agents.iter().all(|a| a.turn_list[0].plan_allowed && !a.turn_list[0].plan_unused));
        // An agent from before turns: its one turn kept, then set aside.
        let mut older = Agent { id: "ll22".into(), conversation: Some(7), status: Status::Done, plan: true, created_ts: 100, ended_ts: 200, ..Default::default() };
        older.turn_list.clear();
        let mut agents = vec![older, Agent { id: "mm33".into(), conversation: Some(7), created_ts: 500, ..Default::default() }];
        assert_eq!(set_plans_aside(&mut agents), ["ll22"]);
        assert_eq!(agents[0].turn_list.len(), 1);
        assert!(agents[0].plan_unused);
        // The load migration: the same rule over the file.
        let mut file = Store { agents: vec![planned("nn44", 7, 100, 200), Agent { id: "oo55".into(), conversation: Some(7), created_ts: 300, ..Default::default() }], ..Default::default() };
        assert!(migrate(&mut file));
        assert!(file.agents[0].plan_unused);
        assert!(!migrate(&mut file), "once");
    }

    #[test]
    fn a_row_dismissed_is_kept_and_a_waiting_plan_dismissed_stops_waiting() {
        insert(planned("pp66", 7, 100, 200));
        let j = dismiss("pp66", 1, true).unwrap();
        assert!(j.turn_list[0].dismissed && j.turn_list[0].plan_unused && j.plan_unused);
        assert!(!j.plan_waits());
        let j = dismiss("pp66", 1, false).unwrap();
        assert!(!j.turn_list[0].dismissed, "shown again");
        assert!(j.turn_list[0].plan_unused, "a plan set aside stays set aside");
        assert!(dismiss("pp66", 9, true).is_err());
        assert!(dismiss("zz99", 1, true).is_err());
        // Through the store: a turn beginning sets the chat's plans aside.
        insert(planned("qq77", 12, 100, 200));
        insert(Agent { id: "rr88".into(), conversation: Some(12), created_ts: 300, ..Default::default() });
        let changed = plans_aside();
        assert_eq!(changed.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), ["qq77"]);
        assert!(get("qq77").unwrap().plan_unused);
        assert!(plans_aside().is_empty());
    }

    #[test]
    fn each_chats_mode_and_the_bypass_ok_are_kept() {
        assert_eq!(mode_of(Some(4), "auto"), "auto");
        set_mode(4, "plan");
        assert_eq!(mode_of(Some(4), "auto"), "plan");
        assert_eq!(mode_of(None, "manual"), "manual");
        assert!(!bypass_confirmed());
        confirm_bypass();
        assert!(bypass_confirmed());
        insert(agent("x1", Status::Done, 3));
        std::fs::create_dir_all(backup_dir("x1").unwrap()).unwrap();
        assert!(reset() >= 1);
        assert!(list().is_empty() && !bypass_confirmed() && !root().exists());
    }
}

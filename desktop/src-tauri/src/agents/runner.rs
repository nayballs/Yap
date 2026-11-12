//! **The runner**: each agent is the person's own Claude Code in the agent's
//! folder (`claude_code::session::Session`), followed as it works.
//!
//! - **At most 3 at once, one per folder** (`agents_max`, Pro and Max plans'
//!   limits): the rest queue, and start in turn as one ends.
//! - **Their own setup**: no `--strict-mcp-config`, no `--setting-sources`,
//!   Claude Code's own tools — their settings, skills, MCP servers and
//!   CLAUDE.md, as in their terminal. Yap adds its run endpoint
//!   (`integrations/claude_live.rs`, `Tools::Agent`): the permission prompt
//!   tool (`--permission-prompt-tool mcp__yap__approve`: a card for the
//!   person, never answered by voice) and the backup hook (`--settings`:
//!   each file copied into `agents/<id>/backup/` before it's first changed).
//! - **The flags**: `--permission-mode` from the agent's mode, `--model`,
//!   `--effort`, `--session-id` (then `--resume`). Paid with the person's
//!   Claude plan, or their Anthropic key (`agents_pay`), never ambiguously
//!   (`session::environment`).
//! - **The brief over stdin**, always ending "When you're done, finish with
//!   a short summary of what you changed and anything you couldn't do.", so
//!   the report is ready to say.
//! - **Followed** ([`Follow`]): each tool call as a plain line
//!   (`steps.rs`), the files it changes (its Edit / Write calls and the
//!   backup manifest) and what changed in them once the call went through
//!   (`diff.rs`), the commands it runs and what they're for, its to-do
//!   list, what it says between its calls, what it wasn't allowed to do,
//!   Claude Code's problems (signed out, at the plan's limit, paying
//!   another way: that one is stopped at once) and the result — each sent
//!   as the app event **`yap-agent`** (`{id, status, folder, conversation?,
//!   step?, file?, command?, about?, said?, todos?, now?, diff?, text?,
//!   cost?, problem?, ts}`) and kept in the agent's `log.jsonl` for its tab.
//! - **Asking**: a permission card waiting for the person (the endpoint's
//!   `pending()`): the agent is `asking` until it's answered.
//! - **Its turns** (2026-11-07: one agent per Code chat, as Claude Code is one
//!   session per conversation): each request to it is a turn of its own
//!   (`store::TurnNote`: the brief, then each follow-up — `start_agent` in its
//!   Code chat, `message_agent`, the tab's "Tell the agent…"), on the same
//!   session (`--resume`), at the chat's Mode then (`modes::next_turn`: a
//!   plan stays a plan until "go ahead"), Settings' model and effort then
//!   (unless the person asked for their own), each with its own backups
//!   (`agents/<id>/backup/<n>/`) and its own report. Continue carries on the
//!   same turn.
//! - **A note while a turn works goes to it now** (2026-11-11,
//!   [`follow_up`], [`passes_now`]): written to its Claude Code, which takes
//!   it into the work in hand at its next tool call's end and answers once
//!   for both — as Claude Code's own app does with a message sent mid-work.
//!   Started with `--replay-user-messages`, it echoes each message back as
//!   it takes it in ([`Follow::on_taken`]): the note joins the turn's
//!   request then. One it never reads (it ended first: [`notes_unread`]), or
//!   a request that needs a turn of its own (a go-ahead, another mode, a
//!   phone's words above Auto), **queues as the next turn**: sent once this
//!   one ends.
//! - **A turn ends** once every message written to it has been taken in
//!   and answered ([`Live::unread`]): the process is told its input is over
//!   and exits.
//! - **Allowing a plan means go ahead** (2026-11-07): a Plan-mode turn whose
//!   ExitPlanMode prompt the person allowed ([`plan_allowed`]: the card's
//!   Go ahead, in the thread or on the bar) ends as a plan and its next
//!   turn starts at once, at the chat's Mode with Yap's go-ahead
//!   ([`GO_AHEAD`]), as "go ahead" in the chat would. Not yet: it stays a
//!   plan, and the report card's Go ahead ([`go_ahead`]) carries it out.
//! - **Done or failed**: reported in its chat once it stops working
//!   (`chat/turn/report.rs`; [`reports`]: a turn it goes straight on from,
//!   a request having come while it worked, is in the next one's report)
//!   — never one the person stopped.
//! - **Agents die with Yap** (the kill-on-close job): quitting marks them
//!   interrupted ([`shutdown`]); `store::at_start` does after a crash.
//!
//! The log has ids, counts and outcomes: never a brief, a path, a command or
//! an output.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::ai::claude_code::cli;
use crate::ai::claude_code::events::{Block, Event, Problem, TurnResult};
use crate::ai::claude_code::session::{self, ApiKey, Conversation, Effort, Pay, Session, Spec};
use crate::ai::effort::Level;
use crate::integrations::claude_live::{self, Approval, RunSpec, Tools};

use super::diff;
use super::workspaces;
use super::modes::{self, Mode};
use super::pictures;
use super::steps;
use super::store::{self, Denial, Agent, ProblemNote, Status};

/// The app event every change of an agent goes out as.
pub const EVENT: &str = "yap-agent";
/// The most agents at once.
pub const MAX: usize = 3;
/// What every brief ends with: the report is ready to say.
pub const SUMMARY_LINE: &str = "When you're done, finish with a short summary of what you changed and anything you couldn't do.";
/// What Continue says to an agent that was stopped or interrupted.
pub const CARRY_ON: &str = "Carry on with the task from where you stopped.";
/// **Yap's own go-ahead** for a plan (2026-11-07): its plan allowed on
/// ExitPlanMode's card, the report card's Go ahead, the bar's, and the
/// person's plain "go ahead" after web content in their turn — never the
/// chat model's words.
pub const GO_AHEAD: &str = "Go ahead with the plan.";
/// Yap's own yes to an agent's question, after web content in the turn.
pub const YES_GO_AHEAD: &str = "Yes, go ahead.";
/// **A read-only turn's editing tools, off** (`--disallowedTools`, 8 Nov
/// 2026): Claude Code 2.1.296's file-editing tools (`MultiEdit` is a name it
/// still knows as one, `F1` in its own rules), and `EnterPlanMode`, so it
/// never puts itself into a plan to approve. Bash and PowerShell stay, for
/// reading (`ls`, `git log`, `grep`): they aren't sandboxed, so the brief
/// says not to change anything too ([`READ_ONLY_LINE`]).
pub const READ_ONLY_OFF: &[&str] = &["Edit", "Write", "MultiEdit", "NotebookEdit", ENTER_PLAN];
/// Claude Code's own tool for going into Plan mode by itself: off in a
/// Bypass run (nothing waits for an approval there) and a read-only one.
pub const ENTER_PLAN: &str = "EnterPlanMode";
/// What a read-only turn's message ends with, in place of [`SUMMARY_LINE`].
pub const READ_ONLY_LINE: &str = "This is read only: don't change, create or delete any files (your file-editing tools are switched off for it). Read, search and run only commands that look (ls, git log, grep), never ones that change anything. Don't make a plan to approve either. When you're done, finish with a short summary of what you found.";
/// How long a stopped agent gets to end its turn before it's ended.
const STOP_GRACE: Duration = Duration::from_secs(8);
/// How long an agent whose input is over gets to exit by itself.
const EXIT_GRACE: Duration = Duration::from_secs(15);

/// **One change of an agent**, as the page hears it (`yap-agent`) and its log
/// keeps it.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEvent {
    pub id: String,
    pub status: Status,
    pub folder: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation: Option<u64>,
    /// What it's doing now ("Reading src/app.ts").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    /// A file it changes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// A command it runs (its `cd` into the folder gone).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// What Claude Code said the command does (Bash's `description`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub about: Option<String>,
    /// What it says as it works, between its tool calls (one line of the
    /// tab's steps each).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub said: Option<String>,
    /// Its to-do list, as its latest `TodoWrite` left it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub todos: Option<Vec<steps::Todo>>,
    /// The line it's on now, when that isn't a new step: the to-do in
    /// progress once a tool call came back, the line before it asked.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub now: Option<String>,
    /// What an edit or a write changed in a file (`diff.rs`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<diff::FileDiff>,
    /// The tool call a step is (Claude Code's id for it), or the one a
    /// `diff`, an `output` or a `picture` came back from.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call: Option<String>,
    /// A command's line once it's done ("Listed the add-on folders":
    /// `steps::command::did`), with its step.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done: Option<String>,
    /// The lines a Read asked for ("424–503"), with its step.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<String>,
    /// What a command printed (`output.rs`): its row in the thread opens
    /// to it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<super::output::Output>,
    /// **A helper's own step** (12 Nov 2026): the call it was made under,
    /// Claude Code's `parent_tool_use_id` — the `Task` (or `Agent`) call of
    /// the session that handed part of its work over. The page folds such a
    /// step behind that call's row ("Handed part of it to a helper"), as
    /// Claude Code's own transcript keeps a subagent's work out of the
    /// session's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub under: Option<String>,
    /// A picture a tool call's answer carried (`pictures.rs`): its step
    /// (`call`) shows it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<super::pictures::StepPicture>,
    /// Its final words (and, from an older Yap, its words as it worked).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The API's price so far (Claude Code's estimate).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<ProblemNote>,
    /// The turn it's about (`store::TurnNote::n`): the tab's section.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn: Option<u32>,
    /// A request queued as that turn, behind the one working (its title).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queued: Option<String>,
    /// A paired phone's action (Remote Control: `agents/remote.rs`), with
    /// its line in `step` ("Allowed from Office phone · Run a command: …").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<super::remote::RemoteNote>,
    /// What its turn has used so far (`usage.rs`): its tokens, its context's
    /// fill, an estimate of its cost while it works. Many a turn: sent to the
    /// page only ([`emit_live`]), but for a result's and a turn's end.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<super::usage::TurnUsage>,
    /// **Its record changed without a step** (2026-11-08): "planUnused" (a
    /// plan nobody acted on, set aside: `store::set_plans_aside`),
    /// "dismissed" / "shown" (its Agent panel row), "finished" (Move to
    /// finished). The pages read the agent again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed: Option<String>,
    /// Unix seconds.
    pub ts: u64,
}

impl AgentEvent {
    /// About the agent's turn now (the newest that ran, else its newest).
    fn of(agent: &Agent) -> AgentEvent {
        let turn = agent.turn_list.iter().rev().find(|t| t.ran()).or(agent.turn_list.last()).map(|t| t.n);
        AgentEvent { turn, ..AgentEvent::at(agent, 0) }
    }

    /// About its turn `n` (0: none).
    fn at(agent: &Agent, n: u32) -> AgentEvent {
        AgentEvent { id: agent.id.clone(), status: agent.status, folder: agent.folder.clone(), conversation: agent.conversation, turn: (n > 0).then_some(n), ts: store::now_secs(), ..Default::default() }
    }
}

/// **A request for an agent's next turn** (a follow-up, a plan's go-ahead):
/// what to tell it and how its turn runs.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ask {
    /// What to tell it.
    pub text: String,
    /// It in a few words (the tab's header; `store::title_of` when empty).
    pub title: String,
    /// The person's message that asked for it (its pill).
    pub message_id: Option<String>,
    /// A plan's go-ahead (its turn at the chat's Mode).
    pub go: bool,
    /// Lower than the chat's Mode (the model may only lower it).
    pub mode: Option<Mode>,
    /// Its model and effort (8 Nov 2026, `agents/sizing.rs`): the person's
    /// own (`model_asked`, `effort_asked`: kept for the turns after), else
    /// the chat's pick for the task (lowered to Settings' ceiling already),
    /// else — [`model_for`] — Settings' then.
    pub model: Option<String>,
    pub effort: Option<String>,
    pub model_asked: bool,
    pub effort_asked: bool,
    /// Why the chat picked them, in a few words (the turn's `sizing`).
    pub sizing: Option<String>,
    /// A paired phone's action (Remote Control): the phone's id. Its turn
    /// runs at most at Auto, and is reported on that phone.
    pub phone: Option<String>,
    /// A read-only turn (its editing tools off: [`READ_ONLY_OFF`]).
    pub read_only: bool,
    /// The person's own message from a Code chat that works directly
    /// (`agents/direct.rs`): the turn it makes is marked so.
    pub direct: bool,
    /// The plan turn a go-ahead carries out (`go`; none: the agent's latest
    /// turn's plan): marked allowed as the turn is asked for, so it never
    /// reads as "Plan not used" ([`go_ahead_turn`]).
    pub plan_turn: Option<u32>,
}

impl Ask {
    /// `text` told the agent, nothing else asked.
    pub fn of(text: &str, go: bool) -> Ask {
        Ask { text: text.trim().to_string(), go, ..Default::default() }
    }

    /// **A plan's go-ahead** in Yap's own words ([`GO_AHEAD`]): its turn at
    /// the chat's Mode, carrying the plan out.
    pub fn go_ahead() -> Ask {
        Ask { text: GO_AHEAD.to_string(), title: "Go ahead with the plan".into(), go: true, ..Default::default() }
    }

    fn title(&self) -> String {
        // A title the model gave is kept whole ("Plan: teaspoons in convert.js"
        // was cut to "Plan" at its colon); a request's words, to their first sentence.
        if self.title.trim().is_empty() { store::title_of(&self.text) } else { store::given_title(&self.title) }
    }

    /// Another request for the same turn, added: its words after these, the
    /// lower mode, a go-ahead in either; read only when both are.
    fn merge(&mut self, more: Ask) {
        self.text = format!("{}\n\n{}", self.text.trim(), more.text.trim());
        self.go |= more.go;
        self.read_only &= more.read_only;
        self.mode = match (self.mode, more.mode) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        // Its size: the person's own first; else the bigger of the chat's two
        // picks (the turn does both), with its reason.
        let size = |a: &Ask| (a.model.as_deref().and_then(super::sizing::rank), a.effort.as_deref().and_then(Level::parse));
        let bigger = size(&more) > size(self);
        if more.model.is_some() && (more.model_asked || (!self.model_asked && (bigger || self.model.is_none()))) {
            self.model = more.model;
            self.model_asked = more.model_asked;
        }
        if more.effort.is_some() && (more.effort_asked || (!self.effort_asked && (bigger || self.effort.is_none()))) {
            self.effort = more.effort;
            self.effort_asked = more.effort_asked;
        }
        if more.sizing.is_some() && (bigger || self.sizing.is_none()) {
            self.sizing = more.sizing;
        }
        if more.phone.is_some() {
            self.phone = more.phone;
        }
        if more.plan_turn.is_some() {
            self.plan_turn = more.plan_turn;
        }
    }
}

/// Where a follow-up went ([`follow_up`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowedUp {
    /// The agent was working: handed to its Claude Code now, which takes
    /// it into the work in hand at its next tool call's end ([`passes_now`]).
    Passed,
    /// The agent was working, and this needs a turn of its own: its next
    /// turn, once this one ends.
    Queued,
    /// Its next turn is on its way now.
    Started,
    /// The turn about to start (or waiting for a slot) takes it too.
    Added,
}

/// An agent that has a slot.
struct Live {
    /// Set once its Claude Code started.
    session: Option<Arc<Session>>,
    /// User messages sent, and results back: its input ends when they meet.
    sent: u32,
    results: u32,
    /// **What was written to it, in order**: its first message, then each
    /// note handed over while it worked ([`FollowedUp::Passed`]).
    written: Vec<String>,
    /// How many of them Claude Code has echoed back (`--replay-user-messages`):
    /// taken in. 0 throughout: a Claude Code that echoes nothing.
    taken: usize,
    /// The notes handed over, as asked (`written[1..]`'s).
    notes: Vec<Ask>,
    /// Its input is over (it's finishing).
    input_over: bool,
    /// Its first message is sent: words for it from now on wait for its
    /// next turn.
    started: bool,
    /// Words for this turn that came while it started (sent with its first).
    extra: Vec<String>,
    /// A request for its next turn, waiting for this one to end.
    pending: Option<Ask>,
    /// The person stopped it (when).
    stopping: Option<Instant>,
    /// The person allowed its plan this turn (ExitPlanMode's card: Go
    /// ahead): a plan turn's next turn carries it out ([`plan_allowed`]).
    plan_allowed: bool,
    /// Its folder, compared ([`workspaces::norm`]).
    folder: String,
}

impl Live {
    fn new(folder: String) -> Live {
        Live { session: None, sent: 0, results: 0, written: Vec::new(), taken: 0, notes: Vec::new(), input_over: false, started: false, extra: Vec::new(), pending: None, stopping: None, plan_allowed: false, folder }
    }

    /// How many messages written to it Claude Code hasn't taken in yet:
    /// by its echoes, or — it echoes nothing — a result a message.
    fn unread(&self) -> usize {
        match self.taken {
            0 => (self.sent as usize).saturating_sub(self.results as usize),
            taken => self.written.len().saturating_sub(taken),
        }
    }

    /// The notes it never took in (it ended first), taken back: they wait
    /// for its next turn instead.
    fn take_unread_notes(&mut self) -> Vec<Ask> {
        let unread = self.unread().min(self.notes.len());
        let read = self.notes.len() - unread;
        self.notes.drain(..).skip(read).collect()
    }
}

/// What a note handed to a working agent ends with.
pub const NOTE_LINE: &str = "(Sent while you work: take this into account, carry on, and cover it in your summary when you're done.)";
/// The tab's lines for a note: handed over, then read.
const NOTE_PASSED: &str = "Your note is with the agent: it reads it at its next step";
const NOTE_READ: &str = "Read your note";
/// How long Claude Code gets to start on a note it hadn't read when its
/// turn ended, before the note goes back to Yap (its next turn).
const NOTE_GRACE: Duration = Duration::from_secs(20);

/// **Does a note for a working agent go to it now?** (11 Nov 2026.) Claude
/// Code takes a message written while it works into that work, at its next
/// tool call's end, and answers once for both (checked on 2.1.296) — as its
/// own app does with a message sent mid-work. So Yap hands the note over
/// at once rather than keeping it until the turn ends, which made it a turn
/// of its own: a new process that read the whole conversation again, and a
/// second report.
///
/// Not when it needs a turn of its own, each of which is set as a turn
/// starts: a plan's go-ahead, a lower mode, the person's own model or
/// effort, a change asked of a turn that is read only (`turn_read_only`),
/// a phone's words for a turn above Auto (`joins`: `follow_up`'s rule) —
/// or a request already waiting (`waiting`: this one keeps its place
/// behind it). `turn`: what the turn working runs at — its mode (a mode
/// asked that is no lower changes nothing: a phone's cap), its model and its
/// effort (the person's own that are the same change nothing either: a Code
/// chat that works directly always says its chip's). Pure.
pub fn passes_now(ask: &Ask, turn: (Mode, &str, &str), turn_read_only: bool, joins: bool, waiting: bool) -> bool {
    let (mode, model, effort) = turn;
    let same = |asked: bool, now: &Option<String>, runs: &str| !asked || now.as_deref().is_none_or(|v| v == runs);
    joins
        && !waiting
        && !ask.go
        && ask.mode.is_none_or(|m| m >= mode)
        && same(ask.model_asked, &ask.model, model)
        && same(ask.effort_asked, &ask.effort, effort)
        && (ask.read_only || !turn_read_only)
}

/// **The person allowed agent `id`'s plan** (Claude Code's ExitPlanMode
/// prompt, answered Go ahead on the thread's card or the bar's:
/// `claude_live/approve.rs`). Once that turn ends with its plan, its next
/// turn starts at once at the chat's Mode with Yap's go-ahead, as a "go
/// ahead" said in the chat would ([`run`]). An agent not running, or a turn
/// not in Plan mode (`Follow::finish` checks), changes nothing.
pub fn plan_allowed(id: &str) {
    let marked = with_state(|s| s.live.get_mut(id).map(|l| l.plan_allowed = true).is_some());
    if marked {
        tracing::info!(id, "Agents: its plan allowed (it carries on once this turn ends)");
    }
}

/// Test only: agent `id` as if its turn had started (a request for it queues
/// as its next turn), or not (`None`): no Claude Code runs.
#[cfg(test)]
pub fn test_live(id: &str, folder: Option<&str>) {
    with_state(|s| match folder {
        Some(f) => {
            let mut l = Live::new(workspaces::norm(f));
            l.started = true;
            s.live.insert(id.to_string(), l);
        }
        None => {
            s.live.remove(id);
        }
    });
}

/// Test only: the request waiting for agent `id`'s next turn.
#[cfg(test)]
pub fn test_pending(id: &str) -> Option<Ask> {
    with_state(|s| s.live.get(id).and_then(|l| l.pending.clone()))
}

#[derive(Default)]
struct State {
    live: HashMap<String, Live>,
    /// Agents waiting for a slot: id, the message their turn starts with
    /// (`None`: the brief), their folder (compared).
    queue: VecDeque<(String, Option<String>, String)>,
}

/// Is there a slot for an agent in `folder`, beside the agents running in `live`
/// (their folders): fewer than `max` running, none in that folder? Pure.
pub fn fits<'a>(live: impl IntoIterator<Item = &'a str>, max: usize, folder: &str) -> bool {
    let live: Vec<&str> = live.into_iter().collect();
    live.len() < max && !live.contains(&folder)
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static APP: OnceLock<AppHandle> = OnceLock::new();

fn state() -> MutexGuard<'static, Option<State>> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut g = state();
    f(g.get_or_insert_with(State::default))
}

/// Keep the app's handle (lib.rs setup).
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

pub fn app() -> Option<&'static AppHandle> {
    APP.get()
}

/// How many agents weren't over when the Yap bar was last told.
static LIVE_TOLD: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(usize::MAX);

/// Send `ev` to the page and keep it in the agent's log; the Yap bar hears
/// when the number of agents not over yet changes ("1 agent working").
pub fn emit(ev: &AgentEvent) {
    let v = serde_json::to_value(ev).ok();
    let line = v.as_ref().and_then(|v| store::log_append(&ev.id, v));
    if let Some(app) = APP.get() {
        let _ = app.emit(EVENT, ev);
        let live = store::live_ids().len();
        if LIVE_TOLD.swap(live, std::sync::atomic::Ordering::Relaxed) != live {
            crate::bar::refresh(app);
            // Keeping the PC awake while agents work (Remote Control on).
            crate::shell::awake::poke();
        }
    }
    // A phone watching it (Remote Control): its log line numbers it there.
    if let Some(v) = v {
        crate::link::agent_event(&ev.id, line, v);
    }
}

/// **Send `ev` to the page only**: what a turn has used so far (`usage`), many
/// times a turn — never into its log (the turn's note keeps it, saved with
/// the agent) nor to a phone.
fn emit_live(ev: &AgentEvent) {
    if let Some(app) = APP.get() {
        let _ = app.emit(EVENT, ev);
    }
}

/// **A paired phone's action in agent `id`'s log** (Remote Control): its
/// tab shows `line` where it happened ("Allowed from Office phone · Run a
/// command: npm test"), at turn `n` (0: its turn now).
pub fn note_remote(id: &str, n: u32, line: String, note: super::remote::RemoteNote) {
    let Some(agent) = store::get(id) else { return };
    let base = if n > 0 { AgentEvent::at(&agent, n) } else { AgentEvent::of(&agent) };
    emit(&AgentEvent { step: Some(line), remote: Some(note), ..base });
}

/// The most agents at once now (1 to [`MAX`]).
fn max_now() -> usize {
    (crate::config::load().agents_max as usize).clamp(1, MAX)
}

/// **Start agent `id`** (saved already): now, or queued behind the others.
/// A plan in its chat nobody acted on stops waiting (Plan not used).
pub fn start(id: &str) {
    plans_aside_told();
    schedule(id, None);
}

/// **Does the runner have agent `id` now** (working, starting, or waiting
/// for a slot)? An agent whose record says it's at work but that isn't here
/// died without saying so: the Agent panel offers Move to finished.
pub fn active(id: &str) -> bool {
    with_state(|s| s.live.contains_key(id) || s.queue.iter().any(|(q, _, _)| q == id))
}

/// **A turn began**: the plans in its chat nobody acted on set aside
/// (`store::plans_aside`), and every page told ("planUnused").
fn plans_aside_told() {
    for agent in store::plans_aside() {
        tracing::info!(id = %agent.id, "Agents: a plan nobody acted on, set aside (Plan not used)");
        emit(&AgentEvent { changed: Some("planUnused".into()), ..AgentEvent::of(&agent) });
    }
}

enum Placed {
    Go,
    Queued,
    Already,
}

/// Start `id`'s next turn with `message` (`None`: its brief), or queue it.
fn schedule(id: &str, message: Option<String>) {
    let Some(agent) = store::get(id) else { return };
    let max = max_now();
    let folder = workspaces::norm(&agent.path);
    let placed = with_state(|s| {
        if s.live.contains_key(id) || s.queue.iter().any(|(q, _, _)| q == id) {
            return Placed::Already;
        }
        if !fits(s.live.values().map(|l| l.folder.as_str()), max, &folder) {
            s.queue.push_back((id.to_string(), message.clone(), folder.clone()));
            return Placed::Queued;
        }
        s.live.insert(id.to_string(), Live::new(folder));
        Placed::Go
    });
    match placed {
        Placed::Already => {}
        Placed::Queued => {
            if let Some(agent) = store::update(id, |j| j.status = Status::Queued) {
                tracing::info!(id = %agent.id, "Agents: queued behind the others");
                // (Its newest turn is the one waiting.)
                let n = agent.turn_list.last().map_or(0, |t| t.n);
                emit(&AgentEvent { step: Some("Waiting for another agent to finish".into()), ..AgentEvent::at(&agent, n) });
            }
        }
        Placed::Go => {
            let id = id.to_string();
            tauri::async_runtime::spawn(async move { run(id, message).await });
        }
    }
}

/// The agents queued that fit now, started.
fn pump() {
    let max = max_now();
    let go: Vec<(String, Option<String>)> = with_state(|s| {
        let mut go = Vec::new();
        let mut i = 0;
        while i < s.queue.len() && s.live.len() < max {
            if !fits(s.live.values().map(|l| l.folder.as_str()), max, &s.queue[i].2) {
                i += 1;
                continue;
            }
            let Some((id, message, folder)) = s.queue.remove(i) else { break };
            s.live.insert(id.clone(), Live::new(folder));
            go.push((id, message));
        }
        go
    });
    for (id, message) in go {
        tauri::async_runtime::spawn(async move { run(id, message).await });
    }
}

/// **What becomes of a turn's report as the turn ends** ([`run`]). Pure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reports {
    /// Told in its chat now: the agent has stopped working.
    Now,
    /// Kept for the next turn's: a request came while it worked, so the
    /// agent goes straight on, and one report at the end covers both.
    Held,
    /// Not this turn (never one the person stopped) — but what it finished
    /// before, held for this turn, is told now: it has stopped working.
    Earlier,
    /// Nothing now (interrupted as Yap quits): Continue carries the turn
    /// on, and its report covers what waited for it.
    Not,
}

/// **One report when the agent stops working** (11 Nov 2026, from the
/// maintainer's real use: a note sent while the agent worked ran as its
/// next turn, and the chat spoke twice in a row, what it had changed and
/// then that the note needed no change, with nobody asking in between).
/// `status`: how the
/// turn ended; `waiting`: a request came while it worked; `plan_allowed`:
/// its plan was allowed on ExitPlanMode's card (reported as before: "it's
/// building from the plan now", then how that went). Pure.
pub fn reports(status: Status, waiting: bool, plan_allowed: bool) -> Reports {
    match status {
        Status::Done | Status::Failed if waiting && !plan_allowed => Reports::Held,
        Status::Done | Status::Failed => Reports::Now,
        Status::Stopped => Reports::Earlier,
        _ => Reports::Not,
    }
}

/// What agent `id` finished and nobody was told (a turn whose report waited
/// for the next, which then never ended: stopped): told now, as the latest
/// of those turns.
fn report_held(id: &str) {
    if let Some(n) = store::get(id).filter(|j| !j.status.live()).and_then(|j| j.held_last()) {
        tracing::info!(id, turn = n, "Agents: the turn after it stopped; what it finished before is reported now");
        crate::chat::turn::queue_held_report(id, n);
    }
}

/// One turn of agent `id`, from start to the process gone; then its slot goes
/// to the next, and it's reported — and a request that came while it
/// worked runs as its next turn, the two reported as one ([`reports`]).
async fn run(id: String, message: Option<String>) {
    let (status, n) = turn(&id, message).await;
    // Notes handed to it that it never read: waiting for its next turn too.
    notes_unread(&id);
    // A request that came while it worked: its next turn.
    let pending = with_state(|s| s.live.remove(&id).and_then(|l| l.pending));
    store::flush();
    let ended = matches!(status, Status::Done | Status::Failed);
    // Its plan allowed on ExitPlanMode's card: carried out now, at the
    // chat's Mode, as a "go ahead" in the chat would (a request that came
    // meanwhile goes with it, as the go-ahead).
    let go_on = status == Status::Done && store::get(&id).is_some_and(|j| j.turn_list.iter().any(|t| t.n == n && t.plan_allowed));
    // One report when it stops working: a turn it goes straight on from
    // waits for the next one's.
    match reports(status, pending.is_some(), go_on) {
        Reports::Now if n > 0 => crate::chat::turn::queue_agent_report(&id, n),
        Reports::Held if n > 0 => {
            store::update(&id, |j| {
                if let Some(t) = j.turn_mut(n) {
                    t.report_held = true;
                }
            });
            tracing::info!(id = %id, turn = n, "Agents: a request waits, so this turn's report waits for the next one's");
        }
        Reports::Earlier => report_held(&id),
        _ => {}
    }
    match pending {
        _ if go_on => {
            tracing::info!(id = %id, turn = n, "Agents: its plan was allowed; carrying it out");
            match pending {
                Some(ask) => {
                    let ask = Ask { go: true, ..ask };
                    resume(&id, Next::Message(ask.text.clone()), &ask, false);
                }
                None => {
                    let ask = Ask::go_ahead();
                    resume(&id, Next::Message(ask.text.clone()), &ask, true);
                }
            }
        }
        Some(ask) if ended => {
            tracing::info!(id = %id, "Agents: its next turn, asked for while this one worked");
            let text = ask.text.clone();
            resume(&id, Next::Message(text), &ask, false);
        }
        // Stopped: the request waiting isn't sent (Continue sends it).
        Some(_) => {
            if let Some(agent) = store::update(&id, |j| {
                if let Some(t) = j.turn_list.last_mut().filter(|t| !t.ran()) {
                    t.status = status;
                }
            }) {
                tracing::info!(id = %id, "Agents: a request waiting for its next turn not sent (stopped)");
                emit(&AgentEvent { step: Some("Not sent yet: Continue sends it".into()), ..AgentEvent::at(&agent, agent.turn_list.last().map_or(0, |t| t.n)) });
            }
        }
        None => {}
    }
    pump();
}

/// What a turn sends first: the brief (with [`SUMMARY_LINE`], or a read-only
/// turn's [`READ_ONLY_LINE`]) or `message`.
fn opening(agent: &Agent, message: Option<&str>, read_only: bool) -> String {
    match message {
        Some(m) => m.to_string(),
        None => format!("{}\n\n{}", agent.brief.trim(), closing(read_only)),
    }
}

/// What a turn's message ends with: [`SUMMARY_LINE`], or a read-only turn's
/// [`READ_ONLY_LINE`].
fn closing(read_only: bool) -> &'static str {
    if read_only {
        READ_ONLY_LINE
    } else {
        SUMMARY_LINE
    }
}

/// **The tools a turn at `mode` runs without** (`--disallowedTools`): a
/// read-only turn's editing tools ([`READ_ONLY_OFF`]); a Bypass turn's
/// `EnterPlanMode` (nothing waits for a plan's approval under Bypass). A turn
/// in Plan mode (the person's own Plan chip or cap) keeps its plan. Pure.
pub fn tools_off(mode: Mode, read_only: bool) -> Vec<String> {
    if mode == Mode::Plan {
        return Vec::new();
    }
    if read_only {
        return READ_ONLY_OFF.iter().map(|t| t.to_string()).collect();
    }
    if modes::unattended(mode) {
        return vec![ENTER_PLAN.to_string()];
    }
    Vec::new()
}

/// The agent's turn `n` failed before Claude Code did anything: said, and
/// saved.
fn failed_early(id: &str, n: u32, why: String, problem: Option<ProblemNote>) -> (Status, u32) {
    tracing::info!(id, "Agents: couldn't start");
    let now = store::now_secs();
    if let Some(agent) = store::update(id, |j| {
        j.status = Status::Failed;
        j.ended_ts = now;
        j.error = Some(why.clone());
        j.problem = problem.clone();
        j.plan = false;
        j.plan_allowed = false;
        j.asks = false;
        j.reported = false;
        j.step.clear();
        if let Some(t) = j.turn_mut(n) {
            t.status = Status::Failed;
            t.ended_ts = now;
            t.error = Some(why.clone());
            t.plan = false;
            t.plan_allowed = false;
            t.asks = false;
        }
    }) {
        emit(&AgentEvent { text: Some(why), problem, ..AgentEvent::at(&agent, n) });
    }
    (Status::Failed, n)
}

/// How the agent pays, or why it can't.
fn pay_for(agent: &Agent, cfg: &crate::config::YapConfig, status: &cli::Status) -> Result<Pay, (String, Option<ProblemNote>)> {
    if agent.pay == "key" {
        let key = cfg.provider_api_key("anthropic", "");
        if key.trim().is_empty() {
            return Err(("There's no Anthropic API key to pay for this agent: add one in Settings → Language Models, or let agents use your Claude plan.".into(), None));
        }
        return Ok(Pay::ApiKey(ApiKey::new(key)));
    }
    if !status.logged_in {
        let p = ProblemNote::of(&Problem::NotSignedIn);
        return Err((p.message.clone(), Some(p)));
    }
    Ok(Pay::Plan)
}

/// **An agent's Claude Code run**: in its folder, the person's own setup (no
/// `--strict-mcp-config`, no `--setting-sources`, Claude Code's own tools)
/// with Yap's endpoint added (the run's `mcp_config`, the backup hook's
/// `settings`, and `env`: the run key both of them name), its permission
/// prompts to Yap's `approve`, at `mode` (a read-only turn: its editing
/// tools off, [`tools_off`]), its model and effort, on `conversation`. Its
/// session is kept (`--resume`, Open in terminal). Pure.
#[allow(clippy::too_many_arguments)]
pub fn agent_spec(
    agent: &Agent,
    folder: &std::path::Path,
    pay: Pay,
    mode: Mode,
    read_only: bool,
    conversation: Conversation,
    mcp_config: serde_json::Value,
    settings: serde_json::Value,
    env: Vec<(String, String)>,
) -> Spec {
    let mut spec = Spec::new(folder, pay);
    spec.env = env;
    spec.model = Some(agent.model.clone()).filter(|m| !m.trim().is_empty());
    spec.effort = Effort::parse(&agent.effort);
    spec.permission_mode = mode.permission();
    spec.disallowed_tools = tools_off(mode, read_only);
    spec.permission_prompt_tool = Some(claude_live::PERMISSION_TOOL.to_string());
    spec.mcp_config = Some(mcp_config);
    spec.strict_mcp_config = false;
    spec.setting_sources = None;
    spec.tools = None;
    spec.settings = Some(settings);
    spec.conversation = conversation;
    // Each message echoed back as it's taken in: which notes it has read.
    spec.replay_user_messages = true;
    spec
}

/// **One turn**: Claude Code started on the agent, `message` (or the brief)
/// sent, followed until it's gone. The turn it ran (`store::TurnNote::n`):
/// the agent's newest (a new request's, or the one Continue carries on).
async fn turn(id: &str, message: Option<String>) -> (Status, u32) {
    // Its turns kept from now on (an agent from before them: one, its brief).
    let Some(agent) = store::update(id, Agent::keep_turns) else { return (Status::Failed, 0) };
    let n = agent.turn_list.last().map_or(1, |t| t.n);
    let cfg = crate::config::load();
    let Some(program) = cli::find() else {
        return failed_early(id, n, "Claude Code isn't installed on this PC.".into(), None);
    };
    let status = tauri::async_runtime::spawn_blocking(cli::status).await.unwrap_or_default();
    if status.too_old {
        let v = status.version.unwrap_or_default();
        return failed_early(id, n, format!("Claude Code {v} is too old for {}: update it to {} or later.", super::FEATURE, cli::MIN_VERSION), None);
    }
    let pay = match pay_for(&agent, &cfg, &status) {
        Ok(p) => p,
        Err((why, problem)) => return failed_early(id, n, why, problem),
    };
    let folder = PathBuf::from(&agent.path);
    if !folder.is_dir() {
        return failed_early(id, n, "The agent's workspace isn't there any more.".into(), None);
    }
    // This turn's own backups (first copy wins within it).
    let Some(backup) = store::turn_backup_dir(id, n) else { return failed_early(id, n, "That isn't one of Yap's agents.".into(), None) };
    if let Err(e) = std::fs::create_dir_all(&backup) {
        return failed_early(id, n, format!("Couldn't make the agent's backup folder: {e}"), None);
    }
    let mode = modes::or_auto(&agent.mode);
    // **A phone's turn runs at most at Auto** (Remote Control, design §5.3),
    // whatever set its mode: its question asked for it, or the phone's own
    // action carried it on (`agents/remote.rs`). The last gate, here.
    let asked = agent.turn_list.iter().find(|t| t.n == n).and_then(|t| t.message_id.clone());
    let mode = match super::remote::phone_of_turn(id, n, asked.as_deref()) {
        Some(_) if mode > super::remote::PHONE_MAX => {
            tracing::info!(id, turn = n, was = mode.as_str(), "Agents: a phone's turn, run at Auto");
            super::remote::phone_mode(mode)
        }
        _ => mode,
    };
    // A read-only turn ("catch up", "have a look"): its editing tools off.
    let read_only = agent.turn_list.iter().find(|t| t.n == n).is_some_and(|t| t.read_only);
    let run = match claude_live::register_run(RunSpec {
        label: agent.folder.clone(),
        tools: Tools::Agent,
        backup_dir: Some(backup.clone()),
        folder: Some(folder.clone()),
        // Bypass: nobody is asked while it runs (a prompt is answered no, no card).
        approval: Some(Approval { run: id.to_string(), conversation: agent.conversation, origin: agent.origin, unattended: modes::unattended(mode) }),
        agents: false,
        memory: false,
        // A job works with the person's own tools, never Yap's screen or pages,
        // nor Yap's connectors (its own Claude Code setup has the person's).
        eyes: Default::default(),
        connectors: None,
    }) {
        Ok(r) => r,
        Err(e) => return failed_early(id, n, format!("Couldn't start the agent: {e}"), None),
    };
    let resuming = message.is_some() && agent.started_ts > 0 && !agent.session.is_empty();
    let session_id = if agent.session.is_empty() { session::new_session_id() } else { agent.session.clone() };
    let conversation = if resuming { Conversation::Resume(session_id) } else { Conversation::New(session_id) };
    let spec = agent_spec(&agent, &folder, pay, mode, read_only, conversation, run.mcp_config(), run.settings(), run.env());
    if read_only {
        tracing::info!(id, turn = n, "Agents: a read-only turn (its editing tools off)");
    }
    let (session, mut rx) = match Session::start_with(&program, spec) {
        Ok(s) => s,
        Err(e) => return failed_early(id, n, e, None),
    };
    let session = Arc::new(session);
    // The person stopped it while it started: ended now. Words for this
    // turn that came meanwhile go with its first message.
    let (stopped_early, extra) = with_state(|s| match s.live.get_mut(id) {
        Some(l) => {
            l.session = Some(Arc::clone(&session));
            l.started = true;
            l.sent = 1;
            (l.stopping.is_some(), std::mem::take(&mut l.extra))
        }
        None => (true, Vec::new()),
    });
    let first = std::iter::once(opening(&agent, message.as_deref(), read_only)).chain(extra.iter().map(|t| format!("Also: {t}"))).collect::<Vec<_>>().join("\n\n");
    if stopped_early {
        session.kill();
    } else if let Err(e) = session.send_user(&first, &[]) {
        session.kill();
        return failed_early(id, n, e, None);
    } else {
        // Its first message is written: a note may follow it from now on.
        with_state(|s| {
            if let Some(l) = s.live.get_mut(id) {
                l.written = vec![first.clone()];
            }
        });
    }
    let now = store::now_secs();
    let Some(agent) = store::update(id, |j| {
        j.status = Status::Running;
        j.session = session.session_id();
        if j.started_ts == 0 {
            j.started_ts = now;
        }
        j.turns += 1;
        j.mode = mode.as_str().to_string();
        j.plan = false;
        j.plan_allowed = false;
        j.plan_unused = false;
        j.asks = false;
        j.reported = false;
        j.error = None;
        j.problem = None;
        j.step = "Starting Claude Code".into();
        let (model, effort) = (j.model.clone(), j.effort.clone());
        if let Some(t) = j.turn_mut(n) {
            t.status = Status::Running;
            if t.started_ts == 0 {
                t.started_ts = now;
            }
            t.mode = mode.as_str().to_string();
            t.model = model;
            t.effort = effort;
            t.plan = false;
            t.plan_allowed = false;
            t.plan_unused = false;
            t.asks = false;
            t.error = None;
            t.undone = false;
            t.told_at = None;
        }
    }) else {
        session.kill();
        return (Status::Failed, n);
    };
    tracing::info!(id, mode = mode.as_str(), pay = %agent.pay, resuming, turn = n, runs = agent.turns, "Agents: started");
    emit(&AgentEvent { step: Some("Starting Claude Code".into()), ..AgentEvent::at(&agent, n) });

    // The model's context window: as Claude Code said it for this turn (a
    // turn carried on) or an earlier one on the same model, else Yap's
    // catalog (a plan chat's ring does the same: `chat/turn/plan.rs`).
    let window = match super::usage::window_before(agent.turn_list.iter().map(|t| (t.model.as_str(), t.usage.as_ref())), &agent.model) {
        0 => crate::chat::budget::context_window(&cfg, "anthropic", &agent.model, "").await.0,
        w => w,
    };
    // What its session's running total goes on from: nothing (a new
    // session), or what was kept of it (none kept: not known).
    let total_before = if resuming { Some(agent.session_total).filter(|t| *t > 0.0) } else { Some(0.0) };
    let mut follow = Follow::new(&agent, n, folder, backup, mode, window, total_before);
    let mut tick = tokio::time::interval(Duration::from_millis(500));
    loop {
        tokio::select! {
            ev = rx.recv() => match ev {
                Some(Event::Exited(_)) | None => break,
                Some(ev) => follow.on(ev, &session),
            },
            _ = tick.tick() => follow.on_tick(&session),
        }
    }
    // Unregistered: a permission card still waiting is a no.
    drop(run);
    (follow.finish(), n)
}

/// **Following one turn** of an agent.
struct Follow {
    id: String,
    /// The turn (`store::TurnNote::n`): its events, its note.
    n: u32,
    folder: PathBuf,
    /// This turn's backups.
    backup: PathBuf,
    mode: Mode,
    /// A read-only turn: never a plan waiting for a go-ahead, whatever its
    /// mode ([`after_plan`]'s caller).
    read_only: bool,
    /// What the agent had cost before this process, and the turn.
    cost_before: f64,
    turn_cost_before: f64,
    /// What this process has cost so far: each result's own cost
    /// (`TurnResult::cost_since`), added up.
    spent: f64,
    /// Claude Code's running total for the session, as last said (its
    /// previous result, or what was kept before this process; `None`: a
    /// resumed session nothing was kept of).
    total_seen: Option<f64>,
    /// The model it runs on (whose share a turn's cost is, when nothing
    /// was kept).
    model: String,
    /// Its latest words.
    last_text: String,
    /// The latest result's outcome, and the words of every result so far
    /// (a note it hadn't read when a turn of Claude Code's ended is a turn
    /// of its own right after, in this process: its words join these).
    outcome: Option<(Status, String)>,
    said: Vec<String>,
    problem: Option<ProblemNote>,
    asking: bool,
    /// When its input ended (its last result came).
    input_over_at: Option<Instant>,
    /// A result came with a note still unread (since when): Claude Code
    /// starts on it now — or, [`NOTE_GRACE`] on, the note goes back to Yap.
    unread_since: Option<Instant>,
    last_flush: Instant,
    /// The edit tools' calls this turn (the backup hook's tools), by id.
    edits: HashSet<String>,
    /// One of them went through (its result not an error): the backup hook
    /// ran before it, if hooks run at all.
    edited: bool,
    /// Tool calls not back yet, by id.
    open: HashSet<String>,
    /// Its to-do list, as its latest `TodoWrite` left it.
    todos: Vec<steps::Todo>,
    /// The line it's on now (the tab's, the running list's).
    line: String,
    /// Edit and Write calls whose diff goes out once they're back.
    changes: HashMap<String, Change>,
    /// Diff lines the agent's log has (across its turns; `diff::AGENT_MAX`).
    diff_used: usize,
    /// The helper's call the message being read was made under (its
    /// `parent_tool_use_id`): its steps say so (`AgentEvent::under`).
    under: Option<String>,
    /// Pictures kept this turn (`pictures::PER_TURN`).
    pictures: usize,
    /// The commands' calls not back yet, by id (their output is said).
    commands: HashSet<String>,
    /// Characters of output kept this turn (`output::TURN_MAX`).
    output_used: usize,
    /// What the turn has used (`usage.rs`).
    meter: super::usage::Meter,
}

/// An Edit, MultiEdit or Write call waiting for its result: its diff goes
/// out once it went through.
struct Change {
    tool: String,
    input: serde_json::Value,
    /// The file, as the tab says it.
    file: String,
    /// The agent had touched the file before this call.
    known: bool,
}

/// A Write's kind: "new" when it made the file (the backup manifest says
/// it didn't exist before the agent, and the agent hadn't written it yet this
/// time), "rewrite" when it was there, "write" when Yap can't tell (no
/// backups made). Pure.
fn write_kind(known: bool, state: Option<claude_live::backup::State>) -> &'static str {
    use claude_live::backup::State;
    match (known, state) {
        (true, _) => "rewrite",
        (false, Some(State::Created)) => "new",
        (false, Some(_)) => "rewrite",
        (false, None) => "write",
    }
}

/// Whether `tool` is one the backup hook runs before (`Edit`, `Write`,
/// `MultiEdit`, `NotebookEdit`). Pure.
fn backed_up_tool(tool: &str) -> bool {
    claude_live::BACKUP_MATCHER.split('|').any(|t| t == tool)
}

/// **No backups were made**: an edit went through this turn (Claude Code
/// runs `PreToolUse` hooks before a tool, and Yap's hook notes every file
/// it's called for, copied or not), yet the agent's backup list is empty —
/// so its hooks never ran (a managed policy turned them off, `--bare`) or
/// never reached Yap. Pure.
fn no_backups(edited: bool, backed_up: usize) -> bool {
    edited && backed_up == 0
}

impl Follow {
    fn new(agent: &Agent, n: u32, folder: PathBuf, backup: PathBuf, mode: Mode, window: u64, total_before: Option<f64>) -> Follow {
        let before = agent.turn_list.iter().find(|t| t.n == n).and_then(|t| t.usage.clone());
        Follow {
            id: agent.id.clone(),
            n,
            folder,
            backup,
            mode,
            read_only: agent.turn_list.iter().find(|t| t.n == n).is_some_and(|t| t.read_only),
            cost_before: agent.cost,
            turn_cost_before: agent.turn_list.iter().find(|t| t.n == n).map_or(0.0, |t| t.cost),
            spent: 0.0,
            total_seen: total_before,
            model: agent.model.clone(),
            last_text: String::new(),
            outcome: None,
            said: Vec::new(),
            problem: None,
            asking: false,
            input_over_at: None,
            unread_since: None,
            last_flush: Instant::now(),
            edits: HashSet::new(),
            edited: false,
            open: HashSet::new(),
            todos: Vec::new(),
            line: String::new(),
            changes: HashMap::new(),
            diff_used: agent.diff_lines as usize,
            under: None,
            pictures: 0,
            commands: HashSet::new(),
            output_used: 0,
            meter: super::usage::Meter::new(before, &agent.model, window),
        }
    }

    fn event(&self, agent: &Agent) -> AgentEvent {
        AgentEvent::at(agent, self.n)
    }

    /// The line it's on now is `line` (no new step): kept and said, when it
    /// changed.
    fn now(&mut self, line: String) {
        if line.is_empty() || line == self.line {
            return;
        }
        self.line = line.clone();
        if let Some(agent) = store::touch(&self.id, |j| j.step = line.clone()) {
            emit(&AgentEvent { now: Some(line), ..self.event(&agent) });
        }
    }

    /// A tool call: its line, its file or command; a `TodoWrite` its list.
    fn on_tool(&mut self, id: &str, name: &str, input: &serde_json::Value) {
        if backed_up_tool(name) {
            self.edits.insert(id.to_string());
        }
        self.open.insert(id.to_string());
        let seen = steps::of_tool(name, input, &self.folder);
        // The to-do list: shown as a list, the line the one in progress.
        // (A helper's own list is its own business: the agent's stays.)
        if seen.todos.is_some() && self.under.is_some() {
            return;
        }
        if let Some(todos) = seen.todos {
            self.todos = todos.clone();
            let line = steps::doing_now(&todos).unwrap_or_else(|| self.line.clone());
            self.line = line.clone();
            let n = self.n;
            if let Some(agent) = store::touch(&self.id, |j| {
                j.step = line.clone();
                if let Some(t) = j.turn_mut(n) {
                    t.todos = todos.clone();
                }
            }) {
                emit(&AgentEvent { todos: Some(todos), now: Some(line).filter(|l| !l.is_empty()), ..self.event(&agent) });
            }
            return;
        }
        let mut known = false;
        let n = self.n;
        let Some(agent) = store::touch(&self.id, |j| {
            j.step = seen.step.clone();
            if let Some(f) = &seen.file {
                known = j.files.contains(f);
                if !known {
                    j.files.push(f.clone());
                }
                if let Some(t) = j.turn_mut(n).filter(|t| !t.files.contains(f)) {
                    t.files.push(f.clone());
                }
            }
            if let Some(c) = &seen.command {
                j.commands.push(c.clone());
            }
        }) else {
            return;
        };
        if let (Some(file), "Edit" | "MultiEdit" | "Write") = (&seen.file, name) {
            self.changes.insert(id.to_string(), Change { tool: name.to_string(), input: input.clone(), file: file.clone(), known });
        }
        self.line = seen.step.clone();
        let call = Some(id.to_string()).filter(|c| !c.is_empty());
        if seen.command.is_some() {
            self.commands.insert(id.to_string());
        }
        emit(&AgentEvent { step: Some(seen.step), file: seen.file, command: seen.command, about: seen.about, done: seen.done, range: seen.range, call, under: self.under.clone(), ..self.event(&agent) });
    }

    /// **A tool call's answer carried pictures** (a picture file read, a
    /// screenshot a tool took): each kept with the agent (`pictures.rs`,
    /// [`pictures::PER_TURN`] a turn) and said with its call, whose step
    /// shows it. One that can't be kept (not a picture, too big) is left out.
    fn on_pictures(&mut self, call: &str, handed: &[crate::ai::claude_code::events::Picture]) {
        for p in handed {
            if self.pictures >= pictures::PER_TURN {
                tracing::debug!(id = %self.id, "Agents: a turn's pictures are at their most, the rest left out");
                return;
            }
            match pictures::keep(&self.id, p) {
                Ok(kept) => {
                    self.pictures += 1;
                    if let Some(agent) = store::get(&self.id) {
                        emit(&AgentEvent { picture: Some(kept), call: Some(call.to_string()), ..self.event(&agent) });
                    }
                }
                Err(why) => tracing::debug!(id = %self.id, why, chars = p.data.len(), "Agents: a step's picture wasn't kept"),
            }
        }
    }

    /// A tool call came back (`text`: its answer's words; `result`: Claude
    /// Code's own record of it, `tool_use_result`, when the message had
    /// one): an edit's diff once it went through, what a command printed;
    /// with nothing else out, the line is the to-do in progress.
    fn on_tool_result(&mut self, id: &str, is_error: bool, text: &str, result: Option<&serde_json::Value>) {
        if !is_error && self.edits.contains(id) {
            self.edited = true;
        }
        self.open.remove(id);
        if let Some(change) = self.changes.remove(id).filter(|_| !is_error) {
            self.diff_of(change, id, result);
        }
        if self.commands.remove(id) {
            self.output_of(id, text, is_error);
        }
        if self.open.is_empty() {
            if let Some(line) = steps::doing_now(&self.todos) {
                self.now(line);
            }
        }
    }

    /// **What a command printed**, said with its call (its row in the
    /// thread opens to it), within the turn's share (`output::TURN_MAX`).
    /// One that printed nothing says nothing.
    fn output_of(&mut self, call: &str, text: &str, failed: bool) {
        let Some(out) = super::output::of(text, failed) else { return };
        if self.output_used + out.chars() > super::output::TURN_MAX {
            return;
        }
        self.output_used += out.chars();
        if let Some(agent) = store::get(&self.id) {
            emit(&AgentEvent { output: Some(out), call: Some(call.to_string()), ..self.event(&agent) });
        }
    }

    /// What `change` (call `call`) did to its file, within the caps, said
    /// and logged: from Claude Code's own patch when its `result` has one
    /// (each part's place in the file), else from the call's input.
    fn diff_of(&mut self, change: Change, call: &str, result: Option<&serde_json::Value>) {
        let d = if change.tool == "Write" {
            let content = change.input.get("content").and_then(serde_json::Value::as_str).unwrap_or("");
            let state = claude_live::backup::read_manifest(&self.backup)
                .ok()
                .and_then(|m| m.files.into_iter().find(|e| steps::shown_path(&e.original.to_string_lossy(), &self.folder) == change.file))
                .map(|e| e.state);
            let kind = write_kind(change.known, state);
            // (A file written over: what changed in it, when Claude Code said; a new one: its lines.)
            result.and_then(|r| diff::of_patch(&change.file, kind, r)).or_else(|| Some(diff::of_write(&change.file, content, kind)))
        } else {
            result.and_then(|r| diff::of_patch(&change.file, "edit", r)).or_else(|| diff::of_edit(&change.tool, &change.input, &change.file))
        };
        let Some(d) = d else { return };
        let (d, kept) = diff::capped(d, diff::AGENT_MAX.saturating_sub(self.diff_used));
        self.diff_used += kept;
        let used = self.diff_used as u32;
        if let Some(agent) = store::touch(&self.id, |j| j.diff_lines = used) {
            emit(&AgentEvent { diff: Some(d), call: Some(call.to_string()).filter(|c| !c.is_empty()), ..self.event(&agent) });
        }
    }

    /// What the turn has used changed: kept on its note (saved with the next
    /// flush) and told to the page.
    fn used(&mut self) {
        let usage = self.meter.now();
        let n = self.n;
        if let Some(agent) = store::touch(&self.id, |j| {
            if let Some(t) = j.turn_mut(n) {
                t.usage = Some(usage.clone());
            }
        }) {
            emit_live(&AgentEvent { usage: Some(usage), ..self.event(&agent) });
        }
    }

    /// One event of Claude Code's.
    fn on(&mut self, ev: Event, session: &Session) {
        match ev {
            Event::Init(init) => {
                if !init.session_id.is_empty() {
                    store::touch(&self.id, |j| j.session = init.session_id.clone());
                }
                self.meter.model(&init.model);
            }
            Event::Assistant(a) => {
                // What its call used (a helper's too: it's spent), first.
                if let Some(u) = &a.usage {
                    if self.meter.call(a.message_id.as_deref(), a.model.as_deref(), u, a.parent_tool_use_id.is_none()) {
                        self.used();
                    }
                }
                // (A helper's message: its tool calls are its own steps, under the call that sent it.)
                self.under = a.parent_tool_use_id.clone().filter(|p| !p.is_empty());
                for block in &a.blocks {
                    match block {
                        Block::ToolUse { id, name, input } => self.on_tool(id, name, input),
                        // What it says between its calls: a line of its own
                        // (a helper's words, under a Task, aren't).
                        Block::Text(t) if !t.trim().is_empty() => {
                            if a.parent_tool_use_id.is_some() {
                                continue;
                            }
                            self.last_text = t.trim().to_string();
                            if let Some(agent) = store::get(&self.id) {
                                emit(&AgentEvent { said: Some(self.last_text.clone()), ..self.event(&agent) });
                            }
                        }
                        _ => {}
                    }
                }
            }
            // A call back: an edit went through, after the hooks. Or a
            // message of Yap's echoed back: Claude Code has taken it in.
            Event::User(u) => {
                if u.is_replay && u.parent_tool_use_id.is_none() {
                    let text: String = u.blocks.iter().filter_map(|b| if let Block::Text(t) = b { Some(t.as_str()) } else { None }).collect();
                    self.on_taken(&text);
                }
                // (Claude Code's own record of a call is its message's: one result a message.)
                let results = u.blocks.iter().filter(|b| matches!(b, Block::ToolResult { .. })).count();
                let record = u.tool_use_result.as_ref().filter(|_| results == 1);
                for block in &u.blocks {
                    if let Block::ToolResult { tool_use_id, is_error, text, pictures } = block {
                        self.on_tool_result(tool_use_id, *is_error, text, record);
                        self.on_pictures(tool_use_id, pictures);
                    }
                }
            }
            Event::PermissionDenied(d) => {
                let detail = if d.message.trim().is_empty() { format!("Use {}", d.tool_name) } else { steps::one_line(&d.message, 160) };
                let line = format!("Not allowed: {}", d.tool_name);
                let n = self.n;
                let denial = Denial { tool: d.tool_name.clone(), detail };
                if let Some(agent) = store::touch(&self.id, |j| {
                    if let Some(t) = j.turn_mut(n) {
                        t.denials.push(denial.clone());
                    }
                    j.denials.push(denial);
                }) {
                    emit(&AgentEvent { step: Some(line), ..self.event(&agent) });
                }
            }
            Event::Problem(p) => {
                let note = ProblemNote::of(&p);
                self.problem = Some(note.clone());
                tracing::info!(id = %self.id, kind = %note.kind, "Agents: a problem with Claude Code");
                if let Some(agent) = store::touch(&self.id, |j| j.problem = Some(note.clone())) {
                    emit(&AgentEvent { problem: Some(note), ..self.event(&agent) });
                }
                // Paying another way than Yap asked: never let it run.
                if matches!(p, Problem::PaysAnotherWay { .. }) {
                    session.kill();
                }
            }
            Event::Result(r) => self.on_result(&r, session),
            _ => {}
        }
    }

    /// **Claude Code echoed a message back** (`--replay-user-messages`): it
    /// has taken it in — the turn's first as it starts, a note at its next
    /// tool call's end (or, none left, as a turn of its own right after).
    /// The next one written and not yet read, by its words; anything else
    /// echoed is nothing of Yap's. A note read: in the turn's request from
    /// now on, and said in its tab.
    fn on_taken(&mut self, text: &str) {
        let note = with_state(|s| {
            let l = s.live.get_mut(&self.id)?;
            if l.written.get(l.taken).is_none_or(|w| w.trim() != text.trim()) {
                return None;
            }
            l.taken += 1;
            // (`written[0]` is its first message: note `taken - 2`.)
            l.taken.checked_sub(2).and_then(|i| l.notes.get(i).cloned())
        });
        let Some(note) = note else { return };
        self.unread_since = None;
        let n = self.n;
        if let Some(agent) = store::update(&self.id, |j| {
            if let Some(t) = j.turn_mut(n) {
                t.request = format!("{}\n\n{}", t.request.trim(), note.text.trim());
            }
        }) {
            tracing::info!(id = %self.id, turn = n, "Agents: it took a note in");
            emit(&AgentEvent { step: Some(NOTE_READ.into()), ..self.event(&agent) });
        }
    }

    /// A turn's result: its words, cost and denials kept; once every message
    /// written has been taken in, its input ends.
    fn on_result(&mut self, r: &TurnResult, session: &Session) {
        let stopping = with_state(|s| s.live.get(&self.id).is_some_and(|l| l.stopping.is_some()));
        let text = if r.text.trim().is_empty() { self.last_text.clone() } else { r.text.trim().to_string() };
        let outcome = outcome_of(r, stopping, self.problem.as_ref());
        let manifest: Vec<String> = claude_live::backup::read_manifest(&self.backup)
            .map(|m| m.files.iter().map(|e| steps::shown_path(&e.original.to_string_lossy(), &self.folder)).collect())
            .unwrap_or_default();
        let denials: Vec<Denial> = r
            .permission_denials
            .iter()
            .map(|d| Denial { tool: d.tool_name.clone(), detail: steps::denied(&d.tool_name, &d.tool_input, &self.folder) })
            .collect();
        // Its own cost: Claude Code's figure is its session's running
        // total, earlier turns and all (`TurnResult::cost_since`).
        self.spent += r.cost_since(self.total_seen, &self.model);
        let session_total = r.cost_usd.max(0.0);
        self.total_seen = Some(session_total);
        let cost = self.cost_before + self.spent;
        let turn_cost = self.turn_cost_before + self.spent;
        // What it used, Claude Code's own sums and window now; its cost is
        // Claude Code's figure from here (`usage.rs`).
        let said = self.meter.said() || r.usage.input_tokens + r.usage.output_tokens > 0;
        let usage = self.meter.at_result(r);
        let usage = said.then_some(usage);
        let unbacked = no_backups(self.edited, manifest.len());
        if unbacked {
            tracing::info!(id = %self.id, "Agents: files changed with no backups (Claude Code's hooks didn't run)");
        }
        let n = self.n;
        let agent = store::update(&self.id, |j| {
            j.cost = cost;
            if session_total > 0.0 {
                j.session_total = session_total;
            }
            if unbacked {
                j.no_backups = true;
            }
            if let Some(t) = j.turn_mut(n) {
                t.cost = turn_cost;
                if usage.is_some() {
                    t.usage.clone_from(&usage);
                }
                for f in &manifest {
                    if !t.files.contains(f) {
                        t.files.push(f.clone());
                    }
                }
                for d in &denials {
                    if !t.denials.iter().any(|x| x.tool == d.tool && x.detail == d.detail) {
                        t.denials.push(d.clone());
                    }
                }
            }
            for f in manifest {
                if !j.files.contains(&f) {
                    j.files.push(f);
                }
            }
            for d in denials {
                if !j.denials.iter().any(|x| x.tool == d.tool && x.detail == d.detail) {
                    j.denials.push(d);
                }
            }
        });
        // Its words so far: this result's after any before it.
        if !text.is_empty() && self.said.last() != Some(&text) {
            self.said.push(text.clone());
        }
        self.outcome = Some((outcome, self.said.join("\n\n")));
        // Over once every message written has been taken in (this result
        // covers them) — or the person stopped it. A note still unread:
        // Claude Code starts on it now, a turn of its own in this process.
        let (done, unread) = with_state(|s| match s.live.get_mut(&self.id) {
            Some(l) => {
                l.results += 1;
                let unread = l.unread();
                let done = unread == 0 || l.stopping.is_some();
                if done {
                    l.input_over = true;
                }
                (done, unread)
            }
            None => (true, 0),
        });
        tracing::info!(id = %self.id, outcome = outcome.word(), last = done, unread, "Agents: a turn's result");
        if let Some(agent) = agent {
            emit(&AgentEvent { text: Some(text), cost: Some(cost), usage, ..self.event(&agent) });
        }
        if done {
            // Stopped with a note unread: never let it start on that note
            // (Continue sends it).
            if unread > 0 {
                session.kill();
            } else {
                session.end();
            }
            self.input_over_at = Some(Instant::now());
        } else {
            self.unread_since = Some(Instant::now());
        }
    }

    /// Every half second: a permission card waiting, a stop taking too
    /// long, an agent that won't exit, and what changed saved.
    fn on_tick(&mut self, session: &Session) {
        let waiting = claude_live::pending();
        let asking = waiting.iter().any(|p| p.run == self.id);
        // Its plan waiting for the person's go-ahead (ExitPlanMode's card).
        let plan = waiting.iter().any(|p| p.run == self.id && p.tool == claude_live::EXIT_PLAN);
        if asking != self.asking {
            self.asking = asking;
            let status = if asking { Status::Asking } else { Status::Running };
            let n = self.n;
            if let Some(agent) = store::update(&self.id, |j| {
                if j.status.live() {
                    j.status = status;
                }
                if let Some(t) = j.turn_mut(n).filter(|t| t.status.live()) {
                    t.status = status;
                }
            }) {
                // Asking: a step of its own; answered, the line it was on.
                let step = asking.then(|| if plan { "Waiting for you: its plan is ready" } else { "Waiting for you to allow a step" }.to_string());
                let now = (!asking).then(|| self.line.clone()).filter(|l| !l.is_empty());
                emit(&AgentEvent { step, now, ..self.event(&agent) });
            }
        }
        let stopping = with_state(|s| s.live.get(&self.id).and_then(|l| l.stopping));
        if stopping.is_some_and(|at| at.elapsed() > STOP_GRACE) || self.input_over_at.is_some_and(|at| at.elapsed() > EXIT_GRACE) {
            session.kill();
        }
        // A note it hadn't read when its turn ended, and no sign of it
        // starting on it: its input ends, and the note goes back to Yap
        // (its next turn: `notes_unread`).
        if self.unread_since.is_some_and(|at| at.elapsed() > NOTE_GRACE) {
            tracing::info!(id = %self.id, "Agents: it never started on a note it hadn't read; ending its input");
            self.unread_since = None;
            with_state(|s| {
                if let Some(l) = s.live.get_mut(&self.id) {
                    l.input_over = true;
                }
            });
            session.end();
            self.input_over_at = Some(Instant::now());
        }
        if self.last_flush.elapsed() > Duration::from_secs(2) {
            store::flush();
            self.last_flush = Instant::now();
        }
        // A prompt waiting with no Yap window in front: on the Yap bar too.
        super::approve_card::sync();
    }

    /// The process is gone: the turn's end, saved and said.
    fn finish(self) -> Status {
        let stopping = with_state(|s| s.live.get(&self.id).is_some_and(|l| l.stopping.is_some()));
        let (status, text, error) = match self.outcome {
            Some((Status::Failed, text)) => {
                let why = self.problem.as_ref().map(|p| p.message.clone()).unwrap_or_else(|| if text.is_empty() { "Claude Code couldn't finish the task.".into() } else { text.clone() });
                (Status::Failed, text, Some(why))
            }
            Some((status, text)) => (status, text, None),
            None if stopping => (Status::Stopped, self.last_text.clone(), None),
            None => {
                let why = self.problem.as_ref().map(|p| p.message.clone()).unwrap_or_else(|| "Claude Code stopped before it finished.".into());
                (Status::Failed, self.last_text.clone(), Some(why))
            }
        };
        // Its plan allowed on ExitPlanMode's card (`plan_allowed`): carried
        // out next ([`run`]) — unless Claude Code carried it out in this turn
        // already (an edit went through after the go-ahead): then it's done.
        // A read-only turn ("catch up", "have a look") reports what it found:
        // never a plan waiting for a go-ahead, whatever its mode.
        let allowed = with_state(|s| s.live.get(&self.id).is_some_and(|l| l.plan_allowed));
        let (plan, plan_allowed) = after_plan(self.mode == Mode::Plan && status == Status::Done && !self.read_only, allowed, self.edited);
        let asks = status == Status::Done && !plan_allowed && super::consent::asks(&text);
        let n = self.n;
        let now = store::now_secs();
        let agent = store::update(&self.id, |j| {
            j.status = status;
            j.ended_ts = now;
            if !text.is_empty() {
                j.text = text.clone();
            }
            j.error = error.clone();
            j.plan = plan;
            j.plan_allowed = plan_allowed;
            j.plan_unused = false;
            j.asks = asks;
            j.step.clear();
            if let Some(t) = j.turn_mut(n) {
                t.status = status;
                t.ended_ts = now;
                if !text.is_empty() {
                    t.text = super::steps::clip(&text, store::TURN_TEXT_MAX);
                }
                t.error = error.clone();
                t.plan = plan;
                t.plan_allowed = plan_allowed;
                t.plan_unused = false;
                t.asks = asks;
            }
        });
        tracing::info!(id = %self.id, turn = n, status = status.word(), plan, plan_allowed, asks, "Agents: ended");
        if let Some(agent) = agent {
            // Its turn's own words (never an earlier turn's).
            let note = agent.turn_list.iter().find(|t| t.n == n);
            let said = note.map(|t| t.text.clone()).unwrap_or_else(|| agent.text.clone());
            let usage = note.and_then(|t| t.usage.clone());
            emit(&AgentEvent { text: Some(said).filter(|t| !t.is_empty()).or(error), cost: Some(agent.cost), problem: agent.problem.clone(), usage, ..AgentEvent::at(&agent, n) });
        }
        // Its prompts ended with its run: their card on the bar goes.
        super::approve_card::sync();
        status
    }
}

/// **A plan turn's end**: `(plan, plan_allowed)` for a turn that ended as a
/// plan (`planned`: in Plan mode, done), whose ExitPlanMode the person
/// allowed (`allowed`), and that changed a file after all (`edited`: Claude
/// Code carried the plan out itself) — then it's no plan any more, and
/// nothing carries on. Pure.
pub fn after_plan(planned: bool, allowed: bool, edited: bool) -> (bool, bool) {
    let plan = planned && !(allowed && edited);
    (plan, plan && allowed)
}

/// A result's outcome: stopped (when the person stopped it), failed, or done.
fn outcome_of(r: &TurnResult, stopping: bool, problem: Option<&ProblemNote>) -> Status {
    if r.interrupted() {
        return if stopping { Status::Stopped } else { Status::Failed };
    }
    if r.is_error || problem.is_some_and(|p| p.kind == "notSignedIn" || p.kind == "paysAnotherWay") {
        return Status::Failed;
    }
    Status::Done
}

// ---- what the person (or the chat) asks of an agent ----

/// **Stop agent `id`**: a queued one comes off the queue, a running one is
/// interrupted (ended if it doesn't stop within a few seconds).
pub fn stop(id: &str) -> Result<Agent, String> {
    enum Was {
        Queued,
        Live(Option<Arc<Session>>),
        Not,
    }
    let was = with_state(|s| {
        if let Some(i) = s.queue.iter().position(|(q, _, _)| q == id) {
            s.queue.remove(i);
            return Was::Queued;
        }
        match s.live.get_mut(id) {
            Some(l) => {
                l.stopping.get_or_insert_with(Instant::now);
                Was::Live(l.session.clone())
            }
            None => Was::Not,
        }
    });
    let agent = store::get(id).ok_or("There's no such agent.")?;
    match was {
        Was::Queued | Was::Not if agent.status.live() => {
            let now = store::now_secs();
            let agent = store::update(id, |j| {
                j.status = Status::Stopped;
                j.ended_ts = now;
                j.step.clear();
                for t in j.turn_list.iter_mut().filter(|t| t.status.live()) {
                    t.status = Status::Stopped;
                    if t.ran() {
                        t.ended_ts = now;
                    }
                }
            })
            .ok_or("There's no such agent.")?;
            tracing::info!(id, "Agents: stopped before it ran");
            emit(&AgentEvent::at(&agent, agent.turn_list.last().map_or(0, |t| t.n)));
            // (A turn before it whose report waited for this one: told now.)
            report_held(id);
            Ok(agent)
        }
        Was::Live(Some(session)) => {
            match session.interrupt() {
                Ok(_) => tracing::info!(id, "Agents: stopping"),
                Err(_) => session.kill(),
            }
            emit(&AgentEvent { step: Some("Stopping".into()), ..AgentEvent::of(&agent) });
            Ok(agent)
        }
        Was::Live(None) => Ok(agent),
        _ => Err("The agent isn't running.".into()),
    }
}

/// **A message for agent `id`** (the tab's "Tell the agent…"; the chat's
/// `message_agent` through [`follow_up`]): its next turn. `go`: a plan's
/// go-ahead (the next turn at the chat's Mode, carrying it out).
pub fn message(id: &str, text: &str, go: bool) -> Result<Agent, String> {
    follow_up(id, Ask::of(text, go)).map(|(agent, _)| agent)
}

/// **Go ahead with agent `id`'s plan** (the report card's Go ahead, the bar's
/// "The agent's plan is ready" card: the person's click is their consent): its
/// next turn now, Yap's own go-ahead ([`GO_AHEAD`]) at the chat's Mode.
/// Once only: refused while it works, or once its latest turn isn't a plan
/// waiting for its go-ahead.
pub fn go_ahead(id: &str) -> Result<Agent, String> {
    go_ahead_turn(id, None, None)
}

/// [`go_ahead`], from paired phone `phone` (Remote Control): the plan's
/// turn is the phone's (at most Auto, reported there).
pub fn go_ahead_from(id: &str, phone: Option<&str>) -> Result<Agent, String> {
    go_ahead_turn(id, None, phone)
}

/// **Go ahead with turn `turn`'s plan** (none: the agent's latest turn's;
/// the Agent panel's row, a report card, the bar, a phone). Its latest
/// turn's plan — waiting, or set aside as "Plan not used" — goes as ever,
/// Yap's own go-ahead ([`GO_AHEAD`]) on `--resume`. An earlier turn's plan
/// (set aside: a newer turn came after it) starts now as a turn of its own
/// that names it ([`earlier_go_ahead`]). Either at the chat's Mode, once:
/// refused while the agent works, or once that plan went ahead.
pub fn go_ahead_turn(id: &str, turn: Option<u32>, phone: Option<&str>) -> Result<Agent, String> {
    let agent = store::get(id).ok_or("There's no such agent.")?;
    if agent.status.live() {
        return Err("The agent is working on it already.".into());
    }
    let notes = agent.turn_notes();
    let latest = notes.last().map(|t| t.n);
    let phone = phone.map(str::to_string);
    let ask = match turn.filter(|n| Some(*n) != latest) {
        None => {
            // (A read-only turn is never a plan to approve.)
            if !agent.plan || agent.plan_allowed || notes.last().is_some_and(|t| t.read_only) {
                return Err("There's no plan waiting for a go-ahead: the agent carried on already.".into());
            }
            Ask { phone, plan_turn: latest, ..Ask::go_ahead() }
        }
        Some(n) => {
            let t = notes.iter().find(|t| t.n == n).ok_or("That turn isn't the agent's.")?;
            if t.status != Status::Done || !t.plan || t.plan_allowed || t.read_only {
                return Err("There's no plan there waiting for a go-ahead: the agent carried on already.".into());
            }
            Ask { text: earlier_go_ahead(&t.title), title: "Go ahead with the earlier plan".into(), go: true, phone, plan_turn: Some(n), ..Default::default() }
        }
    };
    tracing::info!(id, turn = ask.plan_turn.unwrap_or(0), phone = ask.phone.is_some(), "Agents: its plan's go-ahead, from a card");
    follow_up(id, ask).map(|(agent, _)| agent)
}

/// **A go-ahead's plan, used**: turn `plan_turn`'s (none: the agent's latest
/// turn's, when it's a plan not allowed yet) marked allowed — so it never
/// reads as "Plan not used" once the go-ahead's own turn comes after it —
/// on the turn, and on the agent when it's its latest. Before the go-ahead's
/// turn is added. Pure.
pub fn mark_went_ahead(j: &mut Agent, plan_turn: Option<u32>) {
    let latest = j.turn_list.last().map(|t| t.n);
    let which = plan_turn.or_else(|| j.turn_list.last().filter(|t| t.plan && !t.plan_allowed).map(|t| t.n));
    let mut was_latest = false;
    if let Some(t) = which.and_then(|n| j.turn_mut(n)).filter(|t| t.plan) {
        t.plan_allowed = true;
        t.plan_unused = false;
        was_latest = latest == Some(t.n);
    }
    if was_latest {
        j.plan_allowed = true;
        j.plan_unused = false;
    }
}

/// **Yap's own go-ahead for an earlier turn's plan** (a newer turn came after
/// it): it names the plan by its request's words (the person's own). Pure.
pub fn earlier_go_ahead(title: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        "Go ahead with the plan you made earlier, before my later requests.".to_string()
    } else {
        format!("Go ahead with the plan you made earlier, for “{title}”.")
    }
}

/// **Move turn `n` of agent `id` to Finished** (the Agent panel's row menu): a
/// row whose record says it's at work but that the runner doesn't have (it
/// died without saying so) is marked interrupted — Continue picks it up —
/// and a plan still waiting no longer does (Plan not used). Never stops or
/// changes an agent's work: refused while the runner has it ([`active`]).
pub fn move_to_finished(id: &str, n: u32) -> Result<Agent, String> {
    if active(id) {
        return Err("The agent is still working: stop it first.".into());
    }
    let now = store::now_secs();
    let mut found = false;
    let agent = store::update(id, |j| {
        j.keep_turns();
        let latest = j.turn_list.last().map(|t| t.n) == Some(n);
        let mut unused = false;
        if let Some(t) = j.turn_mut(n) {
            found = true;
            if t.status.live() {
                t.status = Status::Interrupted;
                if t.ran() && t.ended_ts == 0 {
                    t.ended_ts = now;
                }
            } else if t.plan_waiting() {
                t.plan_unused = true;
                unused = latest;
            }
        }
        if unused {
            j.plan_unused = true;
        }
        // Nothing runs it: the agent itself isn't at work either.
        if j.status.live() {
            store::interrupt(j, now);
        }
    })
    .ok_or("There's no such agent.")?;
    if !found {
        return Err("That turn isn't the agent's.".into());
    }
    tracing::info!(id, turn = n, "Agents: a row moved to Finished");
    emit(&AgentEvent { changed: Some("finished".into()), ..AgentEvent::at(&agent, n) });
    super::approve_card::sync();
    Ok(agent)
}

/// **Dismiss turn `n` of agent `id`** from the Agent panel (`on`), or show it
/// again: `store::dismiss`, and every page told.
pub fn dismiss(id: &str, n: u32, on: bool) -> Result<Agent, String> {
    let agent = store::dismiss(id, n, on)?;
    tracing::info!(id, turn = n, on, "Agents: a row dismissed or shown again");
    emit(&AgentEvent { changed: Some(if on { "dismissed" } else { "shown" }.into()), ..AgentEvent::at(&agent, n) });
    Ok(agent)
}

/// A request for agent `id`'s next turn, kept until the turn working ends
/// (`Live::pending`; another already waiting: one next turn for both).
/// Whether it's the first waiting; none: the agent isn't at work.
fn wait_for_next_turn(id: &str, ask: &Ask) -> Option<bool> {
    with_state(|s| {
        let l = s.live.get_mut(id)?;
        let first = l.pending.is_none();
        match &mut l.pending {
            Some(p) => p.merge(ask.clone()),
            None => l.pending = Some(ask.clone()),
        }
        Some(first)
    })
}

/// A request that waits for agent `id`'s next turn ([`wait_for_next_turn`]),
/// on its record: a turn of its own, queued (`first`), or added to the one
/// queued already — and every page told.
fn queued_turn(id: &str, ask: &Ask, first: bool) -> Result<(Agent, FollowedUp), String> {
    let (title, text) = (ask.title(), ask.text.clone());
    let agent = store::update(id, |j| {
        j.keep_turns();
        if first {
            let n = j.turn_list.last().map_or(0, |t| t.n) + 1;
            j.turn_list.push(store::TurnNote { n, title: title.clone(), request: text.clone(), message_id: ask.message_id.clone(), status: Status::Queued, read_only: ask.read_only, direct: ask.direct, ..Default::default() });
        } else if let Some(t) = j.turn_list.last_mut() {
            t.request = format!("{}\n\n{}", t.request.trim(), text);
            // Read only when every request it holds is (`Ask::merge`).
            t.read_only &= ask.read_only;
        }
    })
    .ok_or("There's no such agent.")?;
    let n = agent.turn_list.last().map_or(0, |t| t.n);
    if let Some(device) = &ask.phone {
        super::remote::mark_turn(id, n, device);
    }
    tracing::info!(id, turn = n, "Agents: a follow-up queued behind the turn working");
    emit(&AgentEvent { queued: Some(title), ..AgentEvent::at(&agent, n) });
    Ok((agent, FollowedUp::Queued))
}

/// **Notes handed to agent `id`'s Claude Code that it never took in** (its
/// turn ended first, or the person stopped it): back with Yap, waiting as
/// its next turn, ahead of a request that came after them — as a request
/// that came while it worked always did. Called as its process is gone.
fn notes_unread(id: &str) {
    let unread = with_state(|s| {
        let l = s.live.get_mut(id)?;
        let notes = l.take_unread_notes();
        if notes.is_empty() {
            return None;
        }
        // In the order asked: the notes, then what came after them.
        let later = l.pending.take();
        let mut all = notes.clone();
        all.extend(later.clone());
        let mut next = all.remove(0);
        for more in all {
            next.merge(more);
        }
        l.pending = Some(next);
        Some((notes, later.is_some()))
    });
    let Some((notes, mut waiting)) = unread else { return };
    tracing::info!(id, notes = notes.len(), "Agents: notes it never read wait for its next turn");
    for note in &notes {
        let _ = queued_turn(id, note, !waiting);
        waiting = true;
    }
}

/// **A follow-up for agent `id`** (its Code chat's next request, `message_agent`,
/// "Tell the agent…") —
///
/// - while a turn works, **handed to its Claude Code now** ([`passes_now`]):
///   taken into the work in hand at its next tool call's end, one answer
///   for both ([`FollowedUp::Passed`]). One it never takes in (the turn
///   ended first) waits as its next turn ([`notes_unread`]);
/// - while a turn works and it needs a turn of its own (a go-ahead, another
///   mode, a phone's words above Auto…), **queued as its next turn** (sent
///   once this one ends: never two agents, nor two turns, in the folder at
///   once), its tab showing it waiting ([`FollowedUp::Queued`]);
/// - a turn about to start, or waiting for a slot, takes it too
///   ([`FollowedUp::Added`]);
/// - else its next turn now, on `--resume` ([`FollowedUp::Started`]).
pub fn follow_up(id: &str, ask: Ask) -> Result<(Agent, FollowedUp), String> {
    let ask = Ask { text: ask.text.trim().to_string(), ..ask };
    if ask.text.is_empty() {
        return Err("Say what to tell the agent.".into());
    }
    let current = store::get(id).ok_or("There's no such agent.")?;
    // A phone's words never join a turn that runs above Auto: they wait for
    // a turn of their own (at most Auto), as a request while it works does.
    let joins = ask.phone.is_none() || modes::or_auto(&current.mode) <= super::remote::PHONE_MAX;
    let turn_read_only = current.turn_list.last().is_some_and(|t| t.read_only);
    let turn_mode = modes::or_auto(&current.mode);
    enum At {
        /// Its process is starting: with its first message.
        Starting,
        /// Waiting for a slot (its first turn: `true`).
        InQueue(bool),
        /// Working: handed to its Claude Code now.
        Passing(Arc<Session>),
        /// Working: its next turn (the first request waiting: `true`).
        Working(bool),
        Not,
    }
    let text = ask.text.clone();
    let note = format!("{text}\n\n{NOTE_LINE}");
    let at = with_state(|s| {
        if let Some(l) = s.live.get_mut(id) {
            if !l.started && l.stopping.is_none() && joins {
                l.extra.push(text.clone());
                return At::Starting;
            }
            // Its first message written, its input still open, nobody
            // stopping it: the note goes to it now, counted before it's
            // written (a result that comes meanwhile waits for it).
            let open = !l.written.is_empty() && !l.input_over && l.stopping.is_none();
            if let Some(session) = l.session.clone().filter(|_| open && passes_now(&ask, (turn_mode, current.model.as_str(), current.effort.as_str()), turn_read_only, joins, l.pending.is_some())) {
                l.sent += 1;
                l.written.push(note.clone());
                l.notes.push(ask.clone());
                return At::Passing(session);
            }
            let first = l.pending.is_none();
            match &mut l.pending {
                Some(p) => p.merge(ask.clone()),
                None => l.pending = Some(ask.clone()),
            }
            return At::Working(first);
        }
        if let Some(q) = s.queue.iter_mut().find(|(q, _, _)| q == id) {
            let first = q.1.is_none();
            if let Some(m) = q.1.as_mut() {
                *m = format!("{m}\n\nAlso: {text}");
            }
            return At::InQueue(first);
        }
        At::Not
    });
    let added = |j: &mut Agent| {
        if let Some(t) = j.turn_list.last_mut() {
            t.request = format!("{}\n\n{}", t.request.trim(), text);
        }
    };
    match at {
        At::Starting => {
            let agent = store::update(id, added).ok_or("There's no such agent.")?;
            tracing::info!(id, "Agents: a follow-up for the turn starting");
            Ok((agent, FollowedUp::Added))
        }
        At::InQueue(first) => {
            let agent = store::update(id, |j| {
                if first {
                    j.brief = format!("{}\n\nAlso: {text}", j.brief.trim());
                }
                added(j);
            })
            .ok_or("There's no such agent.")?;
            // A phone's words in the turn waiting: it's the phone's too (at
            // most Auto when it starts).
            if let (Some(device), Some(t)) = (&ask.phone, agent.turn_list.last()) {
                super::remote::mark_turn(id, t.n, device);
            }
            tracing::info!(id, "Agents: a follow-up for the turn waiting for a slot");
            Ok((agent, FollowedUp::Added))
        }
        At::Passing(session) => {
            if let Err(e) = session.send_user(&note, &[]) {
                // Its input closed under us (it's ending): not handed over
                // after all — the note waits for its next turn.
                tracing::info!(id, "Agents: a note couldn't be handed to the turn working ({e}); it waits for its next turn");
                let first = with_state(|s| {
                    let l = s.live.get_mut(id)?;
                    if let Some(i) = l.written.iter().rposition(|w| *w == note) {
                        l.written.remove(i);
                        l.sent = l.sent.saturating_sub(1);
                        // (`written[0]` is its first message: `notes[i - 1]`.)
                        if i >= 1 && i <= l.notes.len() {
                            l.notes.remove(i - 1);
                        }
                    }
                    Some(())
                })
                .and_then(|()| wait_for_next_turn(id, &ask));
                return match first {
                    Some(first) => queued_turn(id, &ask, first),
                    // Gone meanwhile: its next turn now.
                    None => {
                        resume(id, Next::Message(text.clone()), &ask, true);
                        Ok((store::get(id).ok_or("There's no such agent.")?, FollowedUp::Started))
                    }
                };
            }
            let n = current.turn_list.last().map_or(0, |t| t.n);
            // A phone's words in the turn working (at most Auto: `joins`):
            // it's the phone's too.
            if let Some(device) = &ask.phone {
                super::remote::mark_turn(id, n, device);
            }
            tracing::info!(id, turn = n, "Agents: a note handed to the turn working");
            let agent = store::get(id).ok_or("There's no such agent.")?;
            emit(&AgentEvent { step: Some(NOTE_PASSED.into()), ..AgentEvent::at(&agent, n) });
            Ok((agent, FollowedUp::Passed))
        }
        At::Working(first) => queued_turn(id, &ask, first),
        At::Not => {
            let text = ask.text.clone();
            resume(id, Next::Message(text), &ask, true);
            Ok((store::get(id).ok_or("There's no such agent.")?, FollowedUp::Started))
        }
    }
}

/// What an agent's next turn starts with.
enum Next {
    /// Continue: "Carry on…".
    CarryOn,
    /// The person's message (a plan's go-ahead too).
    Message(String),
}

/// **Continue agent `id`** (stopped, interrupted or failed): its turn carried
/// on, on `--resume` (one that never ran starts from its brief); a request
/// that was waiting behind it when it stopped is sent instead.
pub fn continue_agent(id: &str) -> Result<Agent, String> {
    continue_from(id, None)
}

/// [`continue_agent`], from paired phone `phone` (Remote Control): the turn
/// it carries on is the phone's (at most Auto, reported there).
pub fn continue_from(id: &str, phone: Option<&str>) -> Result<Agent, String> {
    let agent = store::get(id).ok_or("There's no such agent.")?;
    if agent.status.live() {
        return Err("The agent is still running.".into());
    }
    if agent.status == Status::Done {
        return Err("The agent is done: send it a message to carry on.".into());
    }
    let phone = phone.map(str::to_string);
    match agent.turn_list.last().filter(|t| !t.ran() && t.n > 1) {
        Some(waiting) => {
            let text = waiting.request.clone();
            resume(id, Next::Message(text.clone()), &Ask { phone, ..Ask::of(&text, false) }, false);
        }
        None => resume(id, Next::CarryOn, &Ask { phone, ..Ask::default() }, false),
    }
    store::get(id).ok_or_else(|| "There's no such agent.".into())
}

/// What an agent's turn runs on ([`model_for`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Runs {
    pub model: String,
    pub effort: String,
    /// The person's own (kept for the turns after).
    pub model_asked: bool,
    pub effort_asked: bool,
    /// The chat's pick decided its model or effort (`Agent::sized`).
    pub sized: bool,
}

/// **The model and effort an agent's turn runs at** (8 Nov 2026, the chat
/// sizes each agent: `agents/sizing.rs`), each: the person's own this turn
/// (`ask`'s, asked), else theirs from before (the agent's, asked), else the
/// chat's pick this turn, else — `keeps`: Continue or a plan's go-ahead,
/// the same task — the chat's earlier pick, else Settings' as they are now.
/// The effort lowered to the nearest the model takes (`sizing::taken`).
/// Pure, but for the provider's list of efforts.
pub fn model_for(agent: &Agent, ask: &Ask, keeps: bool, cfg_model: &str, cfg_effort: &str) -> Runs {
    let earlier = keeps && agent.sized;
    let one = |now: &Option<String>, now_asked: bool, before: &str, before_asked: bool, default: &str| -> (String, bool, bool) {
        match now {
            Some(v) if now_asked => (v.clone(), true, false),
            _ if before_asked => (before.to_string(), true, false),
            Some(v) => (v.clone(), false, true),
            None if earlier => (before.to_string(), false, true),
            None => (default.to_string(), false, false),
        }
    };
    let (model, model_asked, model_sized) = one(&ask.model, ask.model_asked, &agent.model, agent.model_asked, cfg_model);
    let (effort, effort_asked, effort_sized) = one(&ask.effort, ask.effort_asked, &agent.effort, agent.effort_asked, cfg_effort);
    let effort = Level::parse(&effort).map_or(effort, |l| super::sizing::taken(&model, l).id().to_string());
    Runs { model, effort, model_asked, effort_asked, sized: model_sized || effort_sized }
}

/// **The mode `agent`'s next turn runs at**, as `ask` asks it: the chat's
/// Mode now (`agents_mode`: Settings' default for a chat with none), its
/// folder's cap and a plan waiting for its go-ahead ([`modes::next_turn`]),
/// lowered to `ask.mode`, and a phone's at most Auto. The one place it's
/// worked out: [`resume`] runs the turn at it, and `message_agent` asks
/// first or not by it (`modes::unattended`).
pub fn next_mode(agent: &Agent, ask: &Ask, agents_mode: &str) -> Mode {
    let chip = modes::or_auto(&store::mode_of(agent.conversation, agents_mode));
    let cap = workspaces::list().into_iter().find(|f| workspaces::norm(&f.path) == workspaces::norm(&agent.path)).and_then(|f| f.cap_mode());
    let mut mode = modes::next_turn(modes::or_auto(&agent.mode), chip, cap, ask.go, store::bypass_confirmed());
    if let Some(lower) = ask.mode {
        mode = mode.min(lower);
    }
    // A phone's (its action, or its question): at most Auto.
    if ask.phone.is_some() || ask.message_id.as_deref().and_then(super::remote::phone_of_message).is_some() {
        mode = super::remote::phone_mode(mode);
    }
    mode
}

/// Agent `id`'s next turn (`next`, as `ask` asks it), at the mode it runs at
/// now ([`next_mode`]), the model and effort
/// [`model_for`] says and the way Settings pay now; `new_turn`: a turn of
/// its own (else the newest carries on). One that never ran starts from its
/// brief (with the message added).
fn resume(id: &str, next: Next, ask: &Ask, new_turn: bool) {
    let cfg = crate::config::load();
    let Some(agent) = store::get(id) else { return };
    let was_plan = modes::or_auto(&agent.mode) == Mode::Plan;
    let go = ask.go;
    let mode = next_mode(&agent, ask, &cfg.agents_mode);
    // Continue and a plan's go-ahead carry the same task on: the chat's
    // earlier pick stays; a new request is sized again (else Settings').
    let keeps = matches!(next, Next::CarryOn) || go;
    let runs = model_for(&agent, ask, keeps, &cfg.agents_model, &cfg.agents_effort);
    let (model, effort) = (runs.model.clone(), runs.effort.clone());
    // Why, for the turn: this request's reason, or — the earlier pick kept — its turn's.
    let sizing = match (runs.sized, ask.model.is_some() || ask.effort.is_some()) {
        (false, _) => None,
        (true, true) => ask.sizing.clone(),
        (true, false) => agent.turn_list.last().and_then(|t| t.sizing.clone()),
    };
    let pay = if cfg.agents_pay == "key" { "key" } else { "plan" };
    let ran = agent.started_ts > 0 && !agent.session.is_empty();
    // A read-only turn: a new one as asked; the turn carried on (Continue, a
    // request queued as it) as it was.
    let read_only = if new_turn { ask.read_only } else { agent.turn_list.last().is_some_and(|t| t.read_only) };
    let end = closing(read_only);
    let body = match &next {
        Next::CarryOn => format!("{CARRY_ON}\n\n{end}"),
        Next::Message(t) if go && was_plan => format!("{t}\n\nGo ahead: carry out the plan. {SUMMARY_LINE}"),
        Next::Message(t) => format!("{t}\n\n{end}"),
    };
    let title = ask.title();
    let agent = store::update(id, |j| {
        j.keep_turns();
        // A go-ahead: the plan it carries out was used (never "Plan not used").
        if go {
            mark_went_ahead(j, ask.plan_turn);
        }
        j.mode = mode.as_str().to_string();
        j.model = model.clone();
        j.effort = effort.clone();
        j.model_asked = runs.model_asked;
        j.effort_asked = runs.effort_asked;
        j.sized = runs.sized;
        j.pay = pay.to_string();
        j.status = Status::Queued;
        j.undone = false;
        if let (false, Next::Message(t)) = (ran, &next) {
            j.brief = format!("{}\n\n{}", j.brief.trim(), t.trim());
        }
        if new_turn {
            let n = j.turn_list.last().map_or(0, |t| t.n) + 1;
            let text = match &next {
                Next::Message(t) => t.clone(),
                Next::CarryOn => CARRY_ON.to_string(),
            };
            j.turn_list.push(store::TurnNote { n, title: title.clone(), request: text, message_id: ask.message_id.clone(), direct: ask.direct, ..Default::default() });
        }
        if let Some(t) = j.turn_list.last_mut() {
            t.status = Status::Queued;
            t.read_only = read_only;
            t.mode = mode.as_str().to_string();
            t.model = model.clone();
            t.effort = effort.clone();
            t.sizing = sizing.clone();
        }
    });
    if let Some(agent) = agent {
        let n = agent.turn_list.last().map_or(0, |t| t.n);
        // A phone's action: this turn is the phone's (at most Auto, reported there).
        if let Some(device) = &ask.phone {
            super::remote::mark_turn(id, n, device);
        }
        tracing::info!(id, turn = n, mode = mode.as_str(), model = %model, effort = %effort, sized = runs.sized, go, new_turn, "Agents: its next turn");
        let step = if new_turn || matches!(next, Next::Message(_)) { "Picking up your request" } else { "Picking up where it left off" };
        emit(&AgentEvent { step: Some(step.into()), ..AgentEvent::at(&agent, n) });
    }
    // A newer turn in the chat: a plan there nobody acted on stops waiting.
    plans_aside_told();
    schedule(id, ran.then_some(body));
}

/// What Undo said, as the tab's line: "Undone: 2 files put back".
fn undone_step(report: &claude_live::backup::RestoreReport) -> String {
    match report.restored.len() + report.deleted.len() {
        0 => "Undo: nothing to put back".to_string(),
        1 => "Undone: 1 file put back".to_string(),
        n => format!("Undone: {n} files put back"),
    }
}

/// **Undo agent `id`** (Undo all): every file it changed, in every turn not
/// undone already, put back as it was before its first turn, every file it
/// made deleted (`backup::restore_all`). Not while it runs.
pub fn undo(id: &str) -> Result<claude_live::backup::RestoreReport, String> {
    let agent = store::get(id).ok_or("There's no such agent.")?;
    if agent.status.live() {
        return Err("Stop the agent first.".into());
    }
    if agent.swept {
        return Err("Its backups are gone: Yap keeps them 7 days.".into());
    }
    let undone: Vec<u32> = agent.turn_list.iter().filter(|t| t.undone).map(|t| t.n).collect();
    let all = store::backup_dirs(id);
    if all.is_empty() {
        return Err("This agent didn't change any files.".into());
    }
    let dirs: Vec<PathBuf> = all.iter().filter(|(n, _)| !undone.contains(n)).map(|(_, d)| d.clone()).collect();
    if dirs.is_empty() {
        return Err("Its files are back as they were already.".into());
    }
    let report = claude_live::backup::restore_all(&dirs);
    if let Some(e) = &report.error {
        return Err(e.clone());
    }
    tracing::info!(id, turns = dirs.len(), restored = report.restored.len(), deleted = report.deleted.len(), failed = report.failed.len(), "Agents: undone");
    let turns: Vec<u32> = all.iter().map(|(n, _)| *n).collect();
    if let Some(agent) = store::update(id, |j| {
        j.undone = true;
        for t in j.turn_list.iter_mut().filter(|t| turns.contains(&t.n)) {
            t.undone = true;
        }
    }) {
        emit(&AgentEvent { step: Some(undone_step(&report)), ..AgentEvent::of(&agent) });
    }
    Ok(report)
}

/// The later turn of `later` (`(turn, the files it changed)`, oldest first)
/// that changed one of `files` too, and that file: the reason an earlier
/// turn can't be undone first. Pure.
pub fn later_change<'a>(files: &[PathBuf], later: &'a [(u32, Vec<PathBuf>)]) -> Option<(u32, &'a PathBuf)> {
    later.iter().rev().find_map(|(n, theirs)| theirs.iter().find(|f| files.iter().any(|m| claude_live::backup::same_path(m, f))).map(|f| (*n, f)))
}

/// **Undo turn `n` of agent `id`**: the files that turn changed put back as
/// they were before it, the files it made deleted. Refused while a later
/// turn, not undone, changed one of the same files ("Undo the later change
/// first"). Not while it runs.
pub fn undo_turn(id: &str, n: u32) -> Result<claude_live::backup::RestoreReport, String> {
    let agent = store::get(id).ok_or("There's no such agent.")?;
    if agent.status.live() {
        return Err("Stop the agent first.".into());
    }
    if agent.swept {
        return Err("Its backups are gone: Yap keeps them 7 days.".into());
    }
    let notes = agent.turn_notes();
    let turn = notes.iter().find(|t| t.n == n).ok_or("There's no such turn.")?;
    if turn.undone {
        return Err("That turn's files are back as they were already.".into());
    }
    let all = store::backup_dirs(id);
    let mine: Vec<PathBuf> = all.iter().filter(|(m, _)| *m == n).map(|(_, d)| d.clone()).collect();
    if mine.is_empty() {
        return Err("That turn didn't change any files.".into());
    }
    let files: Vec<PathBuf> = mine.iter().flat_map(|d| claude_live::backup::originals(d).unwrap_or_default()).collect();
    let undone = |m: u32| notes.iter().any(|t| t.n == m && t.undone);
    let later: Vec<(u32, Vec<PathBuf>)> = all.iter().filter(|(m, _)| *m > n && !undone(*m)).map(|(m, d)| (*m, claude_live::backup::originals(d).unwrap_or_default())).collect();
    if let Some((m, file)) = later_change(&files, &later) {
        let title = notes.iter().find(|t| t.n == m).map(|t| t.title.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| "A later turn".into());
        let shown = steps::shown_path(&file.to_string_lossy(), std::path::Path::new(&agent.path));
        tracing::info!(id, turn = n, later = m, "Agents: undo refused: a later turn changed the same file");
        return Err(format!("Undo the later change first: “{title}” changed {shown} too."));
    }
    let report = claude_live::backup::restore_all(&mine);
    if let Some(e) = &report.error {
        return Err(e.clone());
    }
    tracing::info!(id, turn = n, restored = report.restored.len(), deleted = report.deleted.len(), failed = report.failed.len(), "Agents: a turn undone");
    let with_backups: Vec<u32> = all.iter().map(|(m, _)| *m).collect();
    if let Some(agent) = store::update(id, |j| {
        j.keep_turns();
        if let Some(t) = j.turn_mut(n) {
            t.undone = true;
        }
        // Every turn with backups undone: the agent is.
        j.undone = with_backups.iter().all(|m| j.turn_list.iter().any(|t| t.n == *m && t.undone));
    }) {
        emit(&AgentEvent { step: Some(undone_step(&report)), ..AgentEvent::at(&agent, n) });
    }
    Ok(report)
}

/// **Open in terminal**: the agent's session in Windows Terminal, in its
/// folder (`claude --resume`), once it isn't running.
pub fn open_terminal(id: &str) -> Result<(), String> {
    let agent = store::get(id).ok_or("There's no such agent.")?;
    if agent.status.live() {
        return Err("Stop the agent first: then open it in a terminal.".into());
    }
    let folder = PathBuf::from(&agent.path);
    if !folder.is_dir() {
        return Err("The agent's workspace isn't there any more.".into());
    }
    tracing::info!(id, "Agents: opened in a terminal");
    cli::open_resume(&folder, &agent.session, &format!("{} · Claude Code", agent.folder))
}

/// **Yap is quitting**: the agents running end with it (the kill-on-close
/// agent), interrupted — Continue picks them up next time.
pub fn shutdown() {
    let live: Vec<(String, Option<Arc<Session>>)> = with_state(|s| {
        s.queue.clear();
        s.live.iter().map(|(id, l)| (id.clone(), l.session.clone())).collect()
    });
    let now = store::now_secs();
    for (id, session) in &live {
        store::update(id, |j| {
            if j.status.live() {
                store::interrupt(j, now);
            }
        });
        if let Some(s) = session {
            s.kill();
        }
    }
    // Queued ones too.
    let queued: Vec<String> = store::list().into_iter().filter(|j| j.status.live()).map(|j| j.id).collect();
    for id in &queued {
        store::update(id, |j| store::interrupt(j, now));
    }
    if !live.is_empty() || !queued.is_empty() {
        tracing::info!(running = live.len(), queued = queued.len(), "Agents: interrupted as Yap quits");
    }
}

/// **Delete your data** (`privacy/reset.rs`): every agent stops first.
pub fn stop_all() {
    let live: Vec<Option<Arc<Session>>> = with_state(|s| {
        s.queue.clear();
        s.live.values_mut().map(|l| {
            l.stopping.get_or_insert_with(Instant::now);
            l.session.clone()
        }).collect()
    });
    for s in live.into_iter().flatten() {
        s.kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::claude_code::events::TurnResult;

    fn result(is_error: bool, reason: &str) -> TurnResult {
        TurnResult { is_error, terminal_reason: Some(reason.into()), ..Default::default() }
    }

    #[test]
    fn three_agents_at_once_and_one_per_folder() {
        assert!(fits([], 3, r"d:\a"));
        assert!(fits([r"d:\a", r"d:\b"], 3, r"d:\c"));
        assert!(!fits([r"d:\a", r"d:\b", r"d:\c"], 3, r"d:\d"), "three running: queued");
        assert!(!fits([r"d:\a"], 3, r"d:\a"), "one per folder");
        assert!(!fits([r"d:\a"], 1, r"d:\b"), "agents_max 1");
        assert_eq!(workspaces::norm("D:/Projects/Garden/"), workspaces::norm(r"d:\projects\garden"));
    }

    #[test]
    fn a_results_outcome() {
        assert_eq!(outcome_of(&result(false, "completed"), false, None), Status::Done);
        assert_eq!(outcome_of(&result(true, "api_error"), false, None), Status::Failed);
        // Interrupted: stopped when the person asked, else it failed.
        assert_eq!(outcome_of(&result(true, "aborted_streaming"), true, None), Status::Stopped);
        assert_eq!(outcome_of(&result(true, "aborted_tools"), false, None), Status::Failed);
        // Signed out: never an answer, whatever the result says.
        let signed_out = ProblemNote::of(&Problem::NotSignedIn);
        assert_eq!(outcome_of(&result(false, "completed"), false, Some(&signed_out)), Status::Failed);
        let limited = ProblemNote::of(&Problem::RateLimited { resets_at: None, credits_required: false });
        assert_eq!(outcome_of(&result(false, "completed"), false, Some(&limited)), Status::Done, "a limit retried and passed");
    }

    #[test]
    fn an_agent_runs_on_the_persons_own_setup_with_yaps_endpoint_added() {
        let agent = Agent { model: "claude-sonnet-5-5".into(), effort: "high".into(), ..Default::default() };
        let folder = std::env::temp_dir();
        let mcp = serde_json::json!({ "mcpServers": { "yap": { "type": "http" } } });
        let hooks = serde_json::json!({ "hooks": {} });
        let files = session::Files { mcp_config: Some("mcp.json".into()), settings: Some("settings.json".into()), system_prompt: None, own_prompt: None };
        let key = vec![(session::RUN_KEY_VAR.to_string(), "run-key".to_string())];
        let spec = agent_spec(&agent, &folder, Pay::Plan, Mode::Plan, false, Conversation::New("4d7c1d2e-8a51-4c3b-9f0e-2b6a7e1c5d90".into()), mcp.clone(), hooks.clone(), key.clone());
        let a = session::args(&spec, &files);
        let after = |flag: &str| a.iter().position(|x| x == flag).and_then(|i| a.get(i + 1)).map(String::as_str);
        assert_eq!(after("--permission-prompt-tool"), Some("mcp__yap__approve"));
        assert_eq!(after("--permission-mode"), Some("plan"));
        assert_eq!(after("--model"), Some("claude-sonnet-5-5"));
        assert_eq!(after("--effort"), Some("high"));
        assert_eq!(after("--session-id"), Some("4d7c1d2e-8a51-4c3b-9f0e-2b6a7e1c5d90"));
        assert_eq!(after("--mcp-config"), Some("mcp.json"));
        assert_eq!(after("--settings"), Some("settings.json"));
        // Their own setup: their MCP servers, settings files and tools.
        // Its session kept, for --resume and Open in terminal.
        for never in ["--strict-mcp-config", "--setting-sources", "--tools", "--resume", "--no-session-persistence"] {
            assert!(!a.iter().any(|x| x == never), "{never} in {a:?}");
        }
        assert_eq!((spec.mcp_config, spec.settings), (Some(mcp.clone()), Some(hooks.clone())));
        // The run key in its environment, which its files name.
        assert_eq!(spec.env, key);
        // A later turn resumes it, at the mode it runs at then.
        let spec = agent_spec(&agent, &folder, Pay::Plan, Mode::Auto, false, Conversation::Resume("prev".into()), mcp.clone(), hooks.clone(), key.clone());
        let a = session::args(&spec, &files);
        let after = |flag: &str| a.iter().position(|x| x == flag).and_then(|i| a.get(i + 1)).map(String::as_str);
        assert_eq!((after("--resume"), after("--permission-mode")), (Some("prev"), Some("auto")));
        assert!(!a.iter().any(|x| x == "--session-id"));
        // Auto, Manual, Accept edits: every tool as before.
        assert!(!a.iter().any(|x| x == "--disallowedTools"), "{a:?}");
        // **A read-only turn** under Bypass: the chat's Mode, its editing tools
        // (and going into a plan by itself) off; Bash stays for reading.
        let spec = agent_spec(&agent, &folder, Pay::Plan, Mode::Bypass, true, Conversation::Resume("prev".into()), mcp.clone(), hooks.clone(), key.clone());
        let a = session::args(&spec, &files);
        let after = |flag: &str| a.iter().position(|x| x == flag).and_then(|i| a.get(i + 1)).map(String::as_str);
        assert_eq!(after("--permission-mode"), Some("bypassPermissions"));
        assert_eq!(after("--disallowedTools"), Some("Edit,Write,MultiEdit,NotebookEdit,EnterPlanMode"));
        assert!(!a.iter().any(|x| x == "--tools"), "its other tools, Bash among them, as the person has them");
        assert!(!a.iter().any(|x| x == "--dangerously-skip-permissions"));
        assert_eq!(after("--permission-prompt-tool"), Some("mcp__yap__approve"));
        // A Bypass turn that changes things: never into a plan by itself.
        let spec = agent_spec(&agent, &folder, Pay::Plan, Mode::Bypass, false, Conversation::Resume("prev".into()), mcp.clone(), hooks.clone(), key.clone());
        let a = session::args(&spec, &files);
        let after = |flag: &str| a.iter().position(|x| x == flag).and_then(|i| a.get(i + 1)).map(String::as_str);
        assert_eq!(after("--disallowedTools"), Some("EnterPlanMode"));
        // Read only at Auto: its editing tools off too.
        assert_eq!(tools_off(Mode::Auto, true), READ_ONLY_OFF.iter().map(|t| t.to_string()).collect::<Vec<_>>());
        // The person's own Plan chip: a plan, whatever else.
        assert!(tools_off(Mode::Plan, true).is_empty());
        for m in [Mode::Auto, Mode::Manual, Mode::AcceptEdits] {
            assert!(tools_off(m, false).is_empty(), "{m:?}: as today");
        }
        let _ = (mcp, hooks, key);
    }

    #[test]
    fn a_turn_that_edited_with_no_backup_is_marked() {
        for tool in ["Edit", "Write", "MultiEdit", "NotebookEdit"] {
            assert!(backed_up_tool(tool), "{tool}");
        }
        for tool in ["Bash", "Read", "WebFetch", "mcp__yap__approve", "edit", ""] {
            assert!(!backed_up_tool(tool), "{tool}");
        }
        // An edit through, nothing in the backup list: the hooks never ran.
        assert!(no_backups(true, 0));
        // Backed up (copied, created, too big or failed: all listed), or no edit.
        assert!(!no_backups(true, 1));
        assert!(!no_backups(false, 0));
    }

    #[test]
    fn a_turn_opens_with_the_brief_and_the_summary_line() {
        let agent = Agent { brief: "  Tidy the readme.  ".into(), ..Default::default() };
        assert_eq!(opening(&agent, None, false), format!("Tidy the readme.\n\n{SUMMARY_LINE}"));
        assert_eq!(opening(&agent, Some("Also the changelog."), false), "Also the changelog.");
        assert!(SUMMARY_LINE.starts_with("When you're done, finish with a short summary of what you changed and anything you couldn't do."));
        // A read-only turn: the brief says it mustn't change files, and to report what it found.
        let catch_up = Agent { brief: "Get caught up with the add-on in this folder.".into(), ..Default::default() };
        let said = opening(&catch_up, None, true);
        assert!(said.starts_with("Get caught up with the add-on in this folder.\n\nThis is read only: don't change, create or delete any files"), "{said}");
        assert!(said.ends_with("finish with a short summary of what you found."));
        assert!(!said.contains(SUMMARY_LINE));
    }

    #[test]
    fn a_read_only_request_while_a_turn_works_waits_as_a_read_only_turn() {
        let id = store::new_id();
        let path = format!(r"D:\Projects\seed-tracker-{id}");
        store::insert(Agent { id: id.clone(), folder: "seed-tracker".into(), path: path.clone(), status: Status::Running, started_ts: 5, turn_list: vec![store::TurnNote { n: 1, status: Status::Running, started_ts: 5, ..Default::default() }], ..Default::default() });
        test_live(&id, Some(&path));
        // "Have a look at the tests too, don't change anything": its next turn, read only.
        let (agent, how) = follow_up(&id, Ask { read_only: true, ..Ask::of("Have a look at the tests too.", false) }).unwrap();
        assert_eq!(how, FollowedUp::Queued);
        assert!(agent.turn_list[1].read_only);
        assert!(test_pending(&id).unwrap().read_only);
        // A request to change something joins it: no longer read only.
        let (agent, _) = follow_up(&id, Ask::of("Then fix the failing one.", false)).unwrap();
        assert!(!agent.turn_list[1].read_only);
        assert!(!test_pending(&id).unwrap().read_only);
        test_live(&id, None);
    }

    /// A picture file read, a screenshot a tool took: the tool result's own
    /// picture, kept with the agent and said with its call.
    #[test]
    fn a_picture_a_tool_call_came_back_with_is_kept_and_said_with_its_call() {
        use base64::Engine as _;
        let id = store::new_id();
        let folder = PathBuf::from(format!(r"D:\Projects\seed-tracker-{id}"));
        store::insert(Agent { id: id.clone(), folder: "seed-tracker".into(), path: folder.to_string_lossy().into_owned(), status: Status::Running, started_ts: 5, turn_list: vec![store::TurnNote { n: 1, status: Status::Running, started_ts: 5, ..Default::default() }], ..Default::default() });
        let agent = store::get(&id).unwrap();
        let backup = store::turn_backup_dir(&id, 1).unwrap();
        let mut follow = Follow::new(&agent, 1, folder.clone(), backup, Mode::Auto, 200_000, None);
        let picture = |w: u32, h: u32| {
            let rgba: Vec<u8> = (0..w * h).flat_map(|i| [(i % 200) as u8, 80, 120, 255]).collect();
            let png = crate::snip::images::encode_png(&rgba, w, h, true).unwrap();
            crate::ai::claude_code::events::Picture { media_type: "image/png".into(), data: base64::engine::general_purpose::STANDARD.encode(png) }
        };
        follow.on_tool("toolu_read_1", "Read", &serde_json::json!({ "file_path": folder.join("shot.png").to_string_lossy() }));
        follow.on_tool_result("toolu_read_1", false, "", None);
        follow.on_pictures("toolu_read_1", &[picture(64, 40)]);
        let log = store::log_read(&id);
        let step = log.iter().find(|e| e["step"] == "Reading shot.png").expect("its step");
        assert_eq!(step["call"], "toolu_read_1");
        let said = log.iter().find(|e| e.get("picture").is_some()).expect("its picture");
        assert_eq!((said["call"].as_str(), said["turn"].as_u64(), said.get("step")), (Some("toolu_read_1"), Some(1), None));
        assert_eq!((said["picture"]["w"].as_u64(), said["picture"]["h"].as_u64()), (Some(64), Some(40)));
        // Kept in the agent's own folder, read back by its address.
        let at = pictures::address(&id, said["picture"]["id"].as_str().unwrap());
        assert_eq!(crate::snip::images::png_size(&pictures::read(&at).unwrap()), Some((64, 40)));
        // What isn't a picture is left out, the step as it was.
        follow.on_pictures("toolu_read_1", &[crate::ai::claude_code::events::Picture { media_type: "image/png".into(), data: "bm90IGEgcGljdHVyZQ==".into() }]);
        assert_eq!(store::log_read(&id).iter().filter(|e| e.get("picture").is_some()).count(), 1);
        // No more than a turn's share.
        let many: Vec<_> = (0..pictures::PER_TURN + 3).map(|_| picture(8, 8)).collect();
        follow.on_pictures("toolu_shots", &many);
        assert_eq!(store::log_read(&id).iter().filter(|e| e.get("picture").is_some()).count(), pictures::PER_TURN);
        assert_eq!(pictures::kept_in(&pictures::dir(&id).unwrap()), pictures::PER_TURN);
    }

    /// For its row in the thread: an edit's diff with its place in the file
    /// (Claude Code's own patch), a command as done and what it printed, a
    /// Read's lines — each said with its call.
    #[test]
    fn a_steps_own_diff_and_what_its_command_printed_are_said_with_its_call() {
        use serde_json::json;
        let id = store::new_id();
        let folder = PathBuf::from(format!(r"D:\Projects\seed-tracker-{id}"));
        store::insert(Agent { id: id.clone(), folder: "seed-tracker".into(), path: folder.to_string_lossy().into_owned(), status: Status::Running, started_ts: 5, turn_list: vec![store::TurnNote { n: 1, status: Status::Running, started_ts: 5, ..Default::default() }], ..Default::default() });
        let agent = store::get(&id).unwrap();
        let backup = store::turn_backup_dir(&id, 1).unwrap();
        let mut follow = Follow::new(&agent, 1, folder.clone(), backup, Mode::Auto, 200_000, None);
        let notes = folder.join("notes.txt").to_string_lossy().into_owned();
        // An edit: Claude Code's own record of it says where in the file.
        follow.on_tool("toolu_edit_1", "Edit", &json!({ "file_path": notes, "old_string": "line five", "new_string": "line 5" }));
        let record = json!({ "structuredPatch": [{ "oldStart": 4, "oldLines": 3, "newStart": 4, "newLines": 3, "lines": [" line four", "-line five", "+line 5", " line six"] }] });
        follow.on_tool_result("toolu_edit_1", false, "The file has been updated.", Some(&record));
        // A command Claude Code described: its line as done, then what it printed (its colour codes gone).
        follow.on_tool("toolu_run_1", "Bash", &json!({ "command": "npm test", "description": "Run the tests" }));
        follow.on_tool_result("toolu_run_1", true, "\u{1b}[31mFAIL\u{1b}[0m 1 failed\r\n2 passed\n", None);
        // One that printed nothing says nothing more; a part of a file read says which lines.
        follow.on_tool("toolu_run_2", "Bash", &json!({ "command": "mkdir out" }));
        follow.on_tool_result("toolu_run_2", false, "", None);
        follow.on_tool("toolu_read_1", "Read", &json!({ "file_path": notes, "offset": 10, "limit": 5 }));
        follow.on_tool_result("toolu_read_1", false, "10\tline ten", None);
        let log = store::log_read(&id);
        let diff = log.iter().find(|e| e.get("diff").is_some()).expect("its diff");
        assert_eq!(diff["call"], "toolu_edit_1");
        assert_eq!(diff["diff"]["lines"], json!(["@@ -4,3 +4,3 @@", " line four", "-line five", "+line 5", " line six"]));
        assert_eq!((diff["diff"]["added"].as_u64(), diff["diff"]["removed"].as_u64()), (Some(1), Some(1)));
        let run = log.iter().find(|e| e["step"] == "Running the tests").expect("its command");
        assert_eq!((run["done"].as_str(), run["call"].as_str()), (Some("Ran the tests"), Some("toolu_run_1")));
        let printed: Vec<&serde_json::Value> = log.iter().filter(|e| e.get("output").is_some()).collect();
        assert_eq!(printed.len(), 1, "a command that printed nothing says nothing");
        assert_eq!((printed[0]["call"].as_str(), printed[0].get("step")), (Some("toolu_run_1"), None));
        assert_eq!(printed[0]["output"], json!({ "text": "FAIL 1 failed\n2 passed", "failed": true }));
        let read = log.iter().find(|e| e["step"] == "Reading notes.txt").expect("its read");
        assert_eq!(read["range"], "10–14");
        // Without Claude Code's own record (an older one, the tests' stand-in): the lines from the call's input.
        follow.on_tool("toolu_edit_2", "Edit", &json!({ "file_path": notes, "old_string": "a", "new_string": "b" }));
        follow.on_tool_result("toolu_edit_2", false, "The file has been updated.", None);
        let second = store::log_read(&id).into_iter().filter(|e| e.get("diff").is_some()).nth(1).expect("the second diff");
        assert_eq!((second["call"].as_str(), &second["diff"]["lines"]), (Some("toolu_edit_2"), &json!(["-a", "+b"])));
        // No more than a turn's share of output.
        let big = "x".repeat(250) + "\n";
        for i in 0..40 {
            let call = format!("toolu_big_{i}");
            follow.on_tool(&call, "Bash", &json!({ "command": format!("cat big{i}.log") }));
            follow.on_tool_result(&call, false, &big.repeat(60), None);
        }
        let kept: usize = store::log_read(&id).iter().filter_map(|e| e.get("output")).map(|o| o["text"].as_str().unwrap_or("").chars().count() + o["tail"].as_str().unwrap_or("").chars().count()).sum();
        assert!(kept <= super::super::output::TURN_MAX && kept > super::super::output::TURN_MAX / 2, "{kept}");
    }

    /// A session that hands part of its work to a helper (Claude Code's
    /// `Task`): the helper's own tool calls say the call they were made
    /// under, and its to-do list never replaces the agent's.
    #[test]
    fn a_helpers_own_steps_say_the_call_they_were_made_under() {
        use serde_json::json;
        let id = store::new_id();
        let folder = PathBuf::from(format!(r"D:\Projects\seed-tracker-{id}"));
        store::insert(Agent { id: id.clone(), folder: "seed-tracker".into(), path: folder.to_string_lossy().into_owned(), status: Status::Running, started_ts: 5, turn_list: vec![store::TurnNote { n: 1, status: Status::Running, started_ts: 5, ..Default::default() }], ..Default::default() });
        let agent = store::get(&id).unwrap();
        let backup = store::turn_backup_dir(&id, 1).unwrap();
        let mut follow = Follow::new(&agent, 1, folder.clone(), backup, Mode::Auto, 200_000, None);
        let notes = folder.join("notes.txt").to_string_lossy().into_owned();
        // The session's own list, then its hand-over.
        follow.on_tool("toolu_todo", "TodoWrite", &json!({ "todos": [{ "content": "Check the docs", "status": "in_progress", "activeForm": "Checking the docs" }] }));
        follow.on_tool("toolu_task", "Task", &json!({ "description": "Check the docs", "prompt": "Read the docs and report." }));
        // The helper's own calls (its messages name the call that sent it).
        follow.under = Some("toolu_task".into());
        follow.on_tool("toolu_h1", "Read", &json!({ "file_path": notes }));
        follow.on_tool("toolu_h2", "TodoWrite", &json!({ "todos": [{ "content": "A helper's own to-do", "status": "pending" }] }));
        follow.under = None;
        // The session's own again.
        follow.on_tool("toolu_ls", "Bash", &json!({ "command": "ls" }));
        let log = store::log_read(&id);
        let under = |step: &str| log.iter().find(|e| e["step"] == step).map(|e| e.get("under").and_then(|u| u.as_str()).map(str::to_string));
        assert_eq!(under("Handing part of it to a helper · Check the docs"), Some(None));
        assert_eq!(under("Reading notes.txt"), Some(Some("toolu_task".into())));
        assert_eq!(under("Running ls"), Some(None));
        // The agent's to-do list is still the session's own.
        let lists: Vec<&serde_json::Value> = log.iter().filter_map(|e| e.get("todos")).collect();
        assert_eq!(lists.len(), 1);
        assert_eq!(lists[0][0]["content"], "Check the docs");
        assert_eq!(store::get(&id).unwrap().turn_list[0].todos.len(), 1);
    }

    #[test]
    fn the_event_says_only_what_changed() {
        let agent = Agent { id: "ab12".into(), folder: "garden-planner".into(), status: Status::Running, conversation: Some(3), ..Default::default() };
        let ev = AgentEvent { step: Some("Reading notes.md".into()), ..AgentEvent::of(&agent) };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["id"], "ab12");
        assert_eq!(v["status"], "running");
        assert_eq!(v["conversation"], 3);
        assert_eq!(v["step"], "Reading notes.md");
        for absent in ["file", "command", "about", "said", "todos", "now", "diff", "call", "under", "done", "range", "output", "picture", "text", "cost", "problem"] {
            assert!(v.get(absent).is_none(), "{absent}");
        }
        // A diff and a to-do list, as the tab reads them.
        let ev = AgentEvent {
            todos: Some(vec![steps::Todo { content: "Sort the seeds".into(), status: "in_progress".into(), active_form: "Sorting the seeds".into() }]),
            now: Some("Sorting the seeds".into()),
            diff: Some(diff::of_write("seeds.md", "# Seeds", "new")),
            ..AgentEvent::of(&agent)
        };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["todos"][0]["activeForm"], "Sorting the seeds");
        assert_eq!(v["now"], "Sorting the seeds");
        assert_eq!(v["diff"], serde_json::json!({ "file": "seeds.md", "kind": "new", "lines": ["+# Seeds"], "added": 1, "removed": 0 }));
    }

    #[test]
    fn a_write_is_new_rewritten_or_written() {
        use claude_live::backup::State;
        assert_eq!(write_kind(false, Some(State::Created)), "new");
        assert_eq!(write_kind(false, Some(State::Copied)), "rewrite");
        assert_eq!(write_kind(false, Some(State::TooBig)), "rewrite");
        // The agent wrote it before (it made it, then wrote it again).
        assert_eq!(write_kind(true, Some(State::Created)), "rewrite");
        // No backups made: Yap can't tell.
        assert_eq!(write_kind(false, None), "write");
    }

    // ---- turns ----

    use store::TurnNote;

    fn turn(n: u32, title: &str, files: &[&str]) -> TurnNote {
        TurnNote { n, title: title.into(), status: Status::Done, started_ts: n as u64, ended_ts: n as u64, files: files.iter().map(|f| f.to_string()).collect(), ..Default::default() }
    }

    /// A turn's backups as the backup hook leaves them (`n` 0: an older
    /// agent's, right in `backup/`): each file with its text before the turn,
    /// or `None`, the turn made it.
    fn backed_up(id: &str, n: u32, files: &[(&std::path::Path, Option<&str>)]) {
        use claude_live::backup::{Entry, Manifest, State as B, MANIFEST};
        let dir = if n == 0 { store::backup_dir(id).unwrap() } else { store::turn_backup_dir(id, n).unwrap() };
        std::fs::create_dir_all(&dir).unwrap();
        let mut m = Manifest::default();
        for (i, (path, before)) in files.iter().enumerate() {
            let name = format!("{:04}-{}", i + 1, path.file_name().unwrap().to_string_lossy());
            let (state, backup) = match before {
                Some(text) => {
                    std::fs::write(dir.join(&name), text).unwrap();
                    (B::Copied, name)
                }
                None => (B::Created, String::new()),
            };
            m.files.push(Entry { original: path.to_path_buf(), state, backup, bytes: 0, tool: "Edit".into(), ts: 0, note: String::new() });
        }
        std::fs::write(dir.join(MANIFEST), serde_json::to_vec(&m).unwrap()).unwrap();
    }

    fn read(p: &std::path::Path) -> String {
        std::fs::read_to_string(p).unwrap()
    }

    fn workspace() -> PathBuf {
        let folder = std::env::temp_dir().join(format!("yap-agent-turns-{}", store::new_id()));
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn one_report_when_the_agent_stops_working() {
        // Done or failed with nothing waiting: told now.
        assert_eq!(reports(Status::Done, false, false), Reports::Now);
        assert_eq!(reports(Status::Failed, false, false), Reports::Now);
        // A request came while it worked: it goes straight on, and this
        // turn's report waits for that one's — a failed turn's too.
        assert_eq!(reports(Status::Done, true, false), Reports::Held);
        assert_eq!(reports(Status::Failed, true, false), Reports::Held);
        // A plan allowed on its card: said now ("building from the plan"),
        // a request waiting or not.
        assert_eq!(reports(Status::Done, false, true), Reports::Now);
        assert_eq!(reports(Status::Done, true, true), Reports::Now);
        // Stopped by the person: never that turn — what waited for it is told.
        assert_eq!(reports(Status::Stopped, false, false), Reports::Earlier);
        assert_eq!(reports(Status::Stopped, true, false), Reports::Earlier);
        // Yap quitting: nothing now (Continue's report covers what waited).
        assert_eq!(reports(Status::Interrupted, true, false), Reports::Not);
    }

    #[test]
    fn a_note_for_a_working_agent_goes_to_it_now_unless_it_needs_a_turn_of_its_own() {
        let note = || Ask::of("Keep the headings as they are.", false);
        // (What the turn working runs on.)
        const M: &str = "claude-sonnet-5-5";
        const E: &str = "medium";
        // A plain note: now, whatever the turn's mode.
        assert!(passes_now(&note(), (Mode::Bypass, M, E), false, true, false));
        assert!(passes_now(&note(), (Mode::Plan, M, E), false, true, false));
        // A go-ahead, a lower mode, the person's own model or effort: each
        // is set as a turn starts — a turn of its own.
        assert!(!passes_now(&Ask::of("Go ahead.", true), (Mode::Plan, M, E), false, true, false));
        assert!(!passes_now(&Ask { mode: Some(Mode::Plan), ..note() }, (Mode::Auto, M, E), false, true, false));
        assert!(!passes_now(&Ask { model: Some("claude-opus-5-5".into()), model_asked: true, ..note() }, (Mode::Auto, M, E), false, true, false));
        assert!(!passes_now(&Ask { effort: Some("high".into()), effort_asked: true, ..note() }, (Mode::Auto, M, E), false, true, false));
        // A mode no lower than the turn's changes nothing (a phone's cap),
        // and the chat's own pick of a size is the next turn's business.
        assert!(passes_now(&Ask { mode: Some(Mode::Auto), ..note() }, (Mode::Auto, M, E), false, true, false));
        assert!(passes_now(&Ask { mode: Some(Mode::Auto), ..note() }, (Mode::Plan, M, E), false, true, false));
        assert!(passes_now(&Ask { model: Some("claude-haiku-5-5".into()), ..note() }, (Mode::Auto, M, E), false, true, false));
        // A change asked of a turn that is read only: its own turn; a
        // read-only note joins either.
        assert!(!passes_now(&note(), (Mode::Auto, M, E), true, true, false));
        assert!(passes_now(&Ask { read_only: true, ..note() }, (Mode::Auto, M, E), true, true, false));
        // A phone's words for a turn above Auto, and a request already
        // waiting (this one keeps its place behind it).
        assert!(!passes_now(&note(), (Mode::Bypass, M, E), false, false, false));
        assert!(!passes_now(&note(), (Mode::Auto, M, E), false, true, true));
        // The person's own model and effort that are what it runs on already
        // (a Code chat that works directly says its chip's each time): now.
        assert!(passes_now(&Ask { model: Some(M.into()), model_asked: true, effort: Some(E.into()), effort_asked: true, ..note() }, (Mode::Auto, M, E), false, true, false));
        assert!(!passes_now(&Ask { model: Some(M.into()), model_asked: true, effort: Some("high".into()), effort_asked: true, ..note() }, (Mode::Auto, M, E), false, true, false));
        assert!(NOTE_LINE.contains("carry on") && NOTE_LINE.contains("summary"));
    }

    #[test]
    fn a_note_claude_code_never_read_waits_for_the_next_turn() {
        // What was written, and what it echoed back.
        let mut l = Live::new("x".into());
        l.sent = 1;
        l.written = vec!["Brief.".into()];
        assert_eq!(l.unread(), 1, "its first message, not echoed yet");
        l.taken = 1;
        assert_eq!(l.unread(), 0);
        for (i, words) in ["Keep the headings.", "And the footer."].iter().enumerate() {
            l.sent += 1;
            l.written.push(format!("{words}\n\n{NOTE_LINE}"));
            l.notes.push(Ask::of(words, false));
            assert_eq!(l.unread(), i + 1);
        }
        // It read the first note; the turn ended before the second.
        l.taken = 2;
        assert_eq!(l.unread(), 1);
        let back = l.take_unread_notes();
        assert_eq!(back.iter().map(|a| a.text.as_str()).collect::<Vec<_>>(), ["And the footer."]);
        assert!(l.notes.is_empty());
        // A Claude Code that echoes nothing: a result a message.
        let mut quiet = Live::new("x".into());
        quiet.sent = 2;
        quiet.written = vec!["Brief.".into(), "Note.".into()];
        quiet.notes = vec![Ask::of("Note.", false)];
        quiet.results = 1;
        assert_eq!(quiet.unread(), 1);
        quiet.results = 2;
        assert_eq!((quiet.unread(), quiet.take_unread_notes().len()), (0, 0));

        // Back with Yap as the process goes: its next turn, queued on its
        // record — ahead of a request that came after it.
        let id = store::new_id();
        store::insert(Agent { id: id.clone(), folder: "garden-planner".into(), status: Status::Running, started_ts: 5, turn_list: vec![TurnNote { status: Status::Running, ..turn(1, "Add a watering schedule", &[]) }], ..Default::default() });
        with_state(|s| {
            let mut l = Live::new("x".into());
            l.started = true;
            l.sent = 2;
            l.taken = 1;
            l.written = vec!["Brief.".into(), "Keep the headings.".into()];
            l.notes = vec![Ask { message_id: Some("chat:4:2".into()), ..Ask::of("Keep the headings.", false) }];
            l.pending = Some(Ask::of("Go ahead.", true));
            s.live.insert(id.clone(), l);
        });
        notes_unread(&id);
        let next = with_state(|s| s.live.remove(&id).and_then(|l| l.pending)).unwrap();
        assert_eq!(next.text, "Keep the headings.\n\nGo ahead.");
        assert!(next.go, "the go-ahead that came after it goes with it");
        assert_eq!(next.message_id.as_deref(), Some("chat:4:2"));
        // Nothing unread: nothing changes.
        with_state(|s| {
            let mut l = Live::new("x".into());
            l.sent = 1;
            l.taken = 1;
            l.written = vec!["Brief.".into()];
            s.live.insert(id.clone(), l);
        });
        notes_unread(&id);
        assert!(with_state(|s| s.live.remove(&id).and_then(|l| l.pending)).is_none());
    }

    #[test]
    fn a_request_while_a_turn_works_queues_as_its_next_turn() {
        let id = store::new_id();
        store::insert(Agent {
            id: id.clone(),
            folder: "garden-planner".into(),
            path: r"D:\Projects\garden-planner".into(),
            status: Status::Running,
            started_ts: 5,
            session: "4d7c1d2e-8a51-4c3b-9f0e-2b6a7e1c5d90".into(),
            turn_list: vec![TurnNote { status: Status::Running, ..turn(1, "Add a watering schedule", &[]) }],
            ..Default::default()
        });
        with_state(|s| {
            let mut l = Live::new(workspaces::norm(r"D:\Projects\garden-planner"));
            l.started = true;
            s.live.insert(id.clone(), l);
        });
        let (agent, how) = follow_up(&id, Ask { text: "Add a minimap button as well.".into(), message_id: Some("chat:4:2".into()), mode: Some(Mode::Plan), ..Default::default() }).unwrap();
        assert_eq!(how, FollowedUp::Queued);
        assert_eq!(agent.status, Status::Running, "the turn working carries on");
        assert_eq!(agent.turn_list.len(), 2);
        let next = &agent.turn_list[1];
        assert_eq!((next.n, next.status, next.title.as_str(), next.message_id.as_deref()), (2, Status::Queued, "Add a minimap button as well", Some("chat:4:2")));
        assert!(!next.ran());
        // Another while it still works: the same next turn, its words added.
        let (agent, how) = follow_up(&id, Ask::of("And a legend for it.", false)).unwrap();
        assert_eq!(how, FollowedUp::Queued);
        assert_eq!(agent.turn_list.len(), 2, "never a third turn, nor a second agent");
        assert_eq!(agent.turn_list[1].request, "Add a minimap button as well.\n\nAnd a legend for it.");
        let pending = with_state(|s| s.live.get(&id).and_then(|l| l.pending.clone())).unwrap();
        assert_eq!(pending.text, "Add a minimap button as well.\n\nAnd a legend for it.");
        assert_eq!(pending.mode, Some(Mode::Plan), "the lower mode asked kept");
        assert_eq!(pending.message_id.as_deref(), Some("chat:4:2"));
        with_state(|s| s.live.remove(&id));
        assert!(follow_up(&id, Ask::of("   ", false)).is_err());

        // Words while its process starts: with its first message, the same turn.
        let starting = store::new_id();
        store::insert(Agent { id: starting.clone(), status: Status::Queued, turn_list: vec![TurnNote { status: Status::Queued, request: "Tidy notes.md.".into(), ..turn(1, "Tidy notes.md", &[]) }], ..Default::default() });
        with_state(|s| s.live.insert(starting.clone(), Live::new("x".into())));
        let (agent, how) = follow_up(&starting, Ask::of("Keep the headings.", false)).unwrap();
        assert_eq!(how, FollowedUp::Added);
        assert_eq!(agent.turn_list.len(), 1);
        assert_eq!(agent.turn_list[0].request, "Tidy notes.md.\n\nKeep the headings.");
        assert_eq!(with_state(|s| s.live.remove(&starting).map(|l| l.extra)), Some(vec!["Keep the headings.".to_string()]));
    }

    #[test]
    fn a_follow_ups_ask_merges_and_its_model_follows_settings_unless_asked() {
        let mut a = Ask { text: "Add a minimap.".into(), mode: Some(Mode::Auto), ..Default::default() };
        a.merge(Ask { text: "Go ahead.".into(), go: true, mode: Some(Mode::Plan), model: Some("claude-opus-5-5".into()), ..Default::default() });
        assert_eq!((a.text.as_str(), a.go, a.mode, a.model.as_deref()), ("Add a minimap.\n\nGo ahead.", true, Some(Mode::Plan), Some("claude-opus-5-5")));
        assert_eq!(Ask::of("Add a minimap button to the map view. It shows the beds.", false).title(), "Add a minimap button to the map view");
        let runs = |model: &str, effort: &str, model_asked: bool, effort_asked: bool, sized: bool| Runs { model: model.into(), effort: effort.into(), model_asked, effort_asked, sized };
        // Settings' model and effort as they are now…
        let agent = Agent { model: "claude-sonnet-5-5".into(), effort: "medium".into(), ..Default::default() };
        assert_eq!(model_for(&agent, &Ask::default(), false, "claude-haiku-5-5", "high"), runs("claude-haiku-5-5", "high", false, false, false));
        // …the chat's pick for the task (8 Nov 2026): used, not the person's (resized next turn)…
        let opus = Ask { model: Some("claude-opus-5-5".into()), effort: Some("xhigh".into()), sizing: Some("hard debugging".into()), ..Default::default() };
        assert_eq!(model_for(&agent, &opus, false, "claude-haiku-5-5", "high"), runs("claude-opus-5-5", "xhigh", false, false, true));
        // …kept by Continue and a plan's go-ahead, not by a new request.
        let sized = Agent { model: "claude-opus-5-5".into(), effort: "xhigh".into(), sized: true, ..agent.clone() };
        assert_eq!(model_for(&sized, &Ask::default(), true, "claude-haiku-5-5", "high"), runs("claude-opus-5-5", "xhigh", false, false, true));
        assert_eq!(model_for(&sized, &Ask::default(), false, "claude-haiku-5-5", "high"), runs("claude-haiku-5-5", "high", false, false, false));
        // …the person's own for this turn…
        let theirs = Ask { model: Some("claude-opus-5-5".into()), model_asked: true, ..Default::default() };
        assert_eq!(model_for(&agent, &theirs, false, "claude-haiku-5-5", "high"), runs("claude-opus-5-5", "high", true, false, false));
        // …and kept for the turns after, once they asked, over the chat's later pick.
        let asked = Agent { model: "claude-opus-5-5".into(), model_asked: true, ..agent };
        assert_eq!(model_for(&asked, &Ask::default(), false, "claude-haiku-5-5", "high").model, "claude-opus-5-5");
        let haiku = Ask { model: Some("claude-haiku-5-5".into()), ..Default::default() };
        assert_eq!(model_for(&asked, &haiku, false, "claude-sonnet-5-5", "high").model, "claude-opus-5-5");
        // An effort the model doesn't take: the nearest it does (Sonnet 4.6 has no xhigh).
        let old = Ask { model: Some("claude-sonnet-4-6".into()), effort: Some("xhigh".into()), ..Default::default() };
        assert_eq!(model_for(&Agent::default(), &old, false, "claude-sonnet-4-6", "medium").effort, "high");
        // Two requests in one turn: the person's own first, else the bigger pick and its reason.
        let mut a = Ask { model: Some("claude-haiku-5-5".into()), effort: Some("low".into()), sizing: Some("a rename".into()), ..Default::default() };
        a.merge(Ask { model: Some("claude-opus-5-5".into()), effort: Some("high".into()), sizing: Some("a multi-file change".into()), ..Default::default() });
        assert_eq!((a.model.as_deref(), a.effort.as_deref(), a.sizing.as_deref()), (Some("claude-opus-5-5"), Some("high"), Some("a multi-file change")));
        a.merge(Ask { model: Some("claude-haiku-5-5".into()), effort: Some("low".into()), sizing: Some("a typo".into()), ..Default::default() });
        assert_eq!((a.model.as_deref(), a.sizing.as_deref()), (Some("claude-opus-5-5"), Some("a multi-file change")), "the smaller pick doesn't shrink it");
        a.merge(Ask { model: Some("claude-haiku-5-5".into()), model_asked: true, ..Default::default() });
        assert_eq!((a.model.as_deref(), a.model_asked), (Some("claude-haiku-5-5"), true), "the person's own wins");
    }

    #[test]
    fn an_earlier_turn_waits_for_a_later_one_that_changed_the_same_file() {
        let notes = PathBuf::from(r"D:\Projects\garden-planner\notes.md");
        let map = PathBuf::from(r"D:\Projects\garden-planner\map.js");
        let later = vec![(2, vec![map.clone()]), (3, vec![notes.clone()])];
        assert_eq!(later_change(std::slice::from_ref(&notes), &later).map(|(n, _)| n), Some(3));
        assert_eq!(later_change(&[PathBuf::from(r"D:\Projects\garden-planner\beds.json")], &later), None);
        assert_eq!(later_change(std::slice::from_ref(&notes), &[]), None);
        // Windows' names don't care about case: the same file, however it's written.
        #[cfg(windows)]
        assert_eq!(later_change(&[notes], &[(4, vec![PathBuf::from(r"d:\projects\garden-planner\NOTES.md")])]).map(|(n, _)| n), Some(4));
    }

    #[test]
    fn each_turn_undoes_on_its_own_and_undo_all_goes_back_before_the_first() {
        let folder = workspace();
        let (notes, map) = (folder.join("notes.md"), folder.join("minimap.js"));
        // Turn 1 changed notes.md (v0 → v1); turn 2 changed it again (v1 →
        // v2) and made minimap.js.
        let setup = |id: &str| {
            store::insert(Agent {
                id: id.into(),
                folder: "garden-planner".into(),
                path: folder.to_string_lossy().into_owned(),
                status: Status::Done,
                started_ts: 1,
                files: vec!["notes.md".into(), "minimap.js".into()],
                turn_list: vec![turn(1, "Add a watering schedule", &["notes.md"]), turn(2, "Add a minimap button", &["notes.md", "minimap.js"])],
                ..Default::default()
            });
            backed_up(id, 1, &[(&notes, Some("v0"))]);
            backed_up(id, 2, &[(&notes, Some("v1")), (&map, None)]);
            std::fs::write(&notes, "v2").unwrap();
            std::fs::write(&map, "map").unwrap();
        };
        let a = store::new_id();
        setup(&a);
        assert_eq!(store::backup_dirs(&a).iter().map(|(n, _)| *n).collect::<Vec<_>>(), [1, 2]);
        // The earlier turn first: refused, the later one named.
        assert_eq!(undo_turn(&a, 1).unwrap_err(), "Undo the later change first: “Add a minimap button” changed notes.md too.");
        assert_eq!(read(&notes), "v2", "nothing touched");
        // The later turn: its own files, as they were before it.
        let r = undo_turn(&a, 2).unwrap();
        assert_eq!((r.restored.len(), r.deleted.len()), (1, 1));
        assert_eq!(read(&notes), "v1");
        assert!(!map.exists());
        let j = store::get(&a).unwrap();
        assert!(j.turn_list[1].undone && !j.turn_list[0].undone && !j.undone);
        assert!(undo_turn(&a, 2).is_err(), "undone already");
        assert!(undo_turn(&a, 9).is_err());
        // Now the earlier one goes too: every turn undone, the agent is.
        undo_turn(&a, 1).unwrap();
        assert_eq!(read(&notes), "v0");
        assert!(store::get(&a).unwrap().undone);

        // Undo all: back to before its first turn, in one go.
        let b = store::new_id();
        setup(&b);
        let r = undo(&b).unwrap();
        assert_eq!(read(&notes), "v0");
        assert!(!map.exists());
        assert_eq!(r.restored.len(), 1, "notes.md once, from its earliest backup");
        let j = store::get(&b).unwrap();
        assert!(j.undone && j.turn_list.iter().all(|t| t.undone));
        assert!(undo(&b).is_err(), "nothing left to undo");
        // An undone turn is left out of Undo all.
        let c = store::new_id();
        setup(&c);
        undo_turn(&c, 2).unwrap();
        std::fs::write(&notes, "edited by hand").unwrap();
        undo(&c).unwrap();
        assert_eq!(read(&notes), "v0");
        // Never while it runs.
        let d = store::new_id();
        setup(&d);
        store::update(&d, |j| j.status = Status::Running);
        assert_eq!(undo(&d).unwrap_err(), "Stop the agent first.");
        assert_eq!(undo_turn(&d, 2).unwrap_err(), "Stop the agent first.");
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn an_older_agents_single_backup_is_its_turn_one() {
        let folder = workspace();
        let notes = folder.join("notes.md");
        // From before turns: one backup, right in backup/, no turns kept.
        let id = store::new_id();
        store::insert(Agent { id: id.clone(), folder: "garden-planner".into(), path: folder.to_string_lossy().into_owned(), brief: "Tidy notes.md.".into(), status: Status::Done, started_ts: 1, turns: 2, files: vec!["notes.md".into()], ..Default::default() });
        backed_up(&id, 0, &[(&notes, Some("v0"))]);
        std::fs::write(&notes, "v1").unwrap();
        assert_eq!(store::backup_dirs(&id).iter().map(|(n, _)| *n).collect::<Vec<_>>(), [1]);
        let j = store::get(&id).unwrap();
        assert!(j.turn_list.is_empty());
        assert_eq!(j.turn_notes().iter().map(|t| (t.n, t.title.as_str())).collect::<Vec<_>>(), [(1, "Tidy notes.md")]);
        undo_turn(&id, 1).unwrap();
        assert_eq!(read(&notes), "v0");
        assert!(store::get(&id).unwrap().undone);
        // Undo all, as Undo agent did.
        let again = store::new_id();
        store::insert(Agent { id: again.clone(), path: folder.to_string_lossy().into_owned(), status: Status::Done, files: vec!["notes.md".into()], ..Default::default() });
        backed_up(&again, 0, &[(&notes, Some("v0"))]);
        std::fs::write(&notes, "v1").unwrap();
        undo(&again).unwrap();
        assert_eq!(read(&notes), "v0");
        // Its turn 1 carried on since (a backup/1/ of its own): the older copy wins.
        let both = store::new_id();
        store::insert(Agent { id: both.clone(), path: folder.to_string_lossy().into_owned(), status: Status::Done, ..Default::default() });
        backed_up(&both, 0, &[(&notes, Some("v0"))]);
        backed_up(&both, 1, &[(&notes, Some("v1"))]);
        std::fs::write(&notes, "v2").unwrap();
        assert_eq!(store::backup_dirs(&both).iter().map(|(n, _)| *n).collect::<Vec<_>>(), [1, 1]);
        undo_turn(&both, 1).unwrap();
        assert_eq!(read(&notes), "v0");
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_turns_events_say_which_turn() {
        let agent = Agent { id: "ab12".into(), turn_list: vec![turn(1, "a", &[]), TurnNote { status: Status::Queued, ..turn(2, "b", &[]) }], ..Default::default() };
        // The newest that ran (the next one is waiting).
        let agent = Agent { turn_list: vec![agent.turn_list[0].clone(), TurnNote { started_ts: 0, ..agent.turn_list[1].clone() }], ..agent };
        assert_eq!(AgentEvent::of(&agent).turn, Some(1));
        assert_eq!(AgentEvent::at(&agent, 2).turn, Some(2));
        assert_eq!(AgentEvent::of(&Agent::default()).turn, None, "an older agent's: none");
        let v = serde_json::to_value(AgentEvent { queued: Some("Add a minimap".into()), ..AgentEvent::at(&agent, 2) }).unwrap();
        assert_eq!((v["turn"].as_u64(), v["queued"].as_str()), (Some(2), Some("Add a minimap")));
    }

    #[test]
    fn allowing_a_plan_carries_it_out_and_a_cards_go_ahead_goes_once() {
        // A plan turn's end: allowed, its next turn carries it out; not, it's a plan.
        assert_eq!(after_plan(true, true, false), (true, true));
        assert_eq!(after_plan(true, false, false), (true, false), "Not yet: still a plan");
        assert_eq!(after_plan(true, true, true), (false, false), "Claude Code carried it out itself: done");
        assert_eq!(after_plan(false, true, false), (false, false), "not a plan turn");
        // The card's Go ahead marks a running agent; nothing else.
        let id = store::new_id();
        let path = r"D:\Projects\garden-planner";
        plan_allowed(&id);
        assert!(with_state(|s| !s.live.contains_key(&id)));
        test_live(&id, Some(path));
        plan_allowed(&id);
        assert!(with_state(|s| s.live.get(&id).is_some_and(|l| l.plan_allowed)));
        test_live(&id, None);
        // Yap's own go-ahead, as "go ahead" in the chat.
        let a = Ask::go_ahead();
        assert_eq!((a.text.as_str(), a.go, a.title()), (GO_AHEAD, true, "Go ahead with the plan".to_string()));
        // The turn's allowance is its report's (one turn's facts).
        let allowed = Agent { turn_list: vec![TurnNote { plan: true, plan_allowed: true, ..turn(1, "Plan the minimap", &[]) }], ..Default::default() };
        assert!(allowed.as_of_turn(1).plan_allowed);
        // The report card's Go ahead: refused unless a plan waits for it.
        store::insert(Agent { id: id.clone(), path: path.into(), status: Status::Running, mode: "plan".into(), plan: true, ..Default::default() });
        assert!(go_ahead(&id).is_err(), "working on it already");
        store::update(&id, |j| {
            j.status = Status::Done;
            j.plan = false;
        });
        assert!(go_ahead(&id).unwrap_err().contains("no plan waiting"));
        store::update(&id, |j| {
            j.plan = true;
            j.plan_allowed = true;
        });
        assert!(go_ahead(&id).is_err(), "allowed already: carrying it out");
        // A plan waiting: Yap's go-ahead as its next turn (queued here, as
        // the runner has it, with no Claude Code run).
        store::update(&id, |j| j.plan_allowed = false);
        test_live(&id, Some(path));
        let agent = go_ahead(&id).unwrap();
        let sent = test_pending(&id).expect("its next turn");
        assert_eq!((sent.text.as_str(), sent.go), (GO_AHEAD, true));
        assert_eq!(agent.turn_list.last().map(|t| t.title.as_str()), Some("Go ahead with the plan"));
        test_live(&id, None);
    }

    #[test]
    fn a_plan_not_used_still_goes_ahead_and_an_earlier_one_starts_as_a_turn_of_its_own() {
        let id = store::new_id();
        let path = r"D:\Projects\garden-planner";
        let plan = TurnNote { plan: true, plan_unused: true, ..turn(1, "Read only, don't change any files", &[]) };
        let later = TurnNote { request: "Bolder bag markers".into(), ..turn(2, "Bolder bag markers", &["map.js"]) };
        // Its latest turn's plan, set aside (another agent of the chat came after): Go ahead as ever.
        store::insert(Agent { id: id.clone(), path: path.into(), status: Status::Done, mode: "plan".into(), plan: true, plan_unused: true, turn_list: vec![plan.clone()], ..Default::default() });
        test_live(&id, Some(path));
        go_ahead_turn(&id, Some(1), None).unwrap();
        let sent = test_pending(&id).expect("its next turn");
        assert_eq!((sent.text.as_str(), sent.go, sent.plan_turn), (GO_AHEAD, true, Some(1)));
        test_live(&id, None);
        // An earlier turn's plan (a newer turn came after it): a turn naming it.
        store::update(&id, |j| {
            j.plan = false;
            j.plan_unused = false;
            j.turn_list = vec![plan.clone(), later.clone()];
        });
        test_live(&id, Some(path));
        go_ahead_turn(&id, Some(1), None).unwrap();
        let sent = test_pending(&id).expect("its next turn");
        assert_eq!(sent.text, "Go ahead with the plan you made earlier, for “Read only, don't change any files”.");
        assert_eq!((sent.go, sent.plan_turn), (true, Some(1)));
        test_live(&id, None);
        assert!(go_ahead_turn(&id, Some(2), None).is_err(), "its latest turn is no plan");
        assert!(go_ahead_turn(&id, Some(9), None).is_err(), "no such turn");
        assert_eq!(earlier_go_ahead(""), "Go ahead with the plan you made earlier, before my later requests.");
        // The go-ahead marks its plan used (allowed), never "Plan not used" again.
        let mut j = store::get(&id).unwrap();
        mark_went_ahead(&mut j, Some(1));
        assert!(j.turn_list[0].plan_allowed && !j.turn_list[0].plan_unused);
        assert!(!j.plan_allowed, "not the agent's: its latest turn is another");
        let mut k = Agent { plan: true, plan_unused: true, turn_list: vec![plan.clone()], ..Default::default() };
        mark_went_ahead(&mut k, None);
        assert!(k.plan_allowed && !k.plan_unused && k.turn_list[0].plan_allowed);
        let mut none = Agent { turn_list: vec![later.clone()], ..Default::default() };
        mark_went_ahead(&mut none, None);
        assert!(!none.turn_list[0].plan_allowed, "no plan to mark");
    }

    #[test]
    fn move_to_finished_only_when_nothing_runs_it_and_never_stops_one() {
        let id = store::new_id();
        let path = r"D:\Projects\garden-planner";
        // Its record says running; the runner doesn't have it (it died).
        store::insert(Agent { id: id.clone(), path: path.into(), status: Status::Running, turn_list: vec![TurnNote { status: Status::Running, ended_ts: 0, ..turn(1, "Bolder bag markers", &[]) }], ..Default::default() });
        // While the runner has it: refused, untouched.
        test_live(&id, Some(path));
        assert!(active(&id));
        assert!(move_to_finished(&id, 1).unwrap_err().contains("stop it first"));
        assert_eq!(store::get(&id).unwrap().status, Status::Running);
        test_live(&id, None);
        assert!(!active(&id));
        let j = move_to_finished(&id, 1).unwrap();
        assert_eq!((j.status, j.turn_list[0].status), (Status::Interrupted, Status::Interrupted));
        assert!(j.turn_list[0].ended_ts > 0);
        // A plan waiting: set aside (Plan not used), its agent too.
        store::update(&id, |j| {
            j.status = Status::Done;
            j.plan = true;
            j.turn_list = vec![TurnNote { plan: true, ..turn(1, "Read only", &[]) }];
        });
        let j = move_to_finished(&id, 1).unwrap();
        assert!(j.turn_list[0].plan_unused && j.plan_unused && !j.plan_waits());
        assert!(move_to_finished(&id, 7).is_err());
        // Dismissed and shown again, kept.
        assert!(dismiss(&id, 1, true).unwrap().turn_list[0].dismissed);
        assert!(!dismiss(&id, 1, false).unwrap().turn_list[0].dismissed);
        // Its event says the record changed.
        let v = serde_json::to_value(AgentEvent { changed: Some("finished".into()), ..AgentEvent::at(&j, 1) }).unwrap();
        assert_eq!(v["changed"], "finished");
        assert!(serde_json::to_value(AgentEvent::at(&j, 1)).unwrap().get("changed").is_none());
    }
}

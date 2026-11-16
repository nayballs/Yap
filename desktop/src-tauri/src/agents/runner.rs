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
//!   person, never answered by voice). Claude Code's own file checkpoints are
//!   on ([`CHECKPOINT_ENV`]): each turn keeps where they can take its files
//!   back to (`TurnNote::checkpoint`), for Rewind (`rewind.rs`), as the
//!   Claude app's Rewind does. Yap keeps no copies of its own.
//! - **The flags**: `--permission-mode` from the agent's mode, `--model`,
//!   `--effort`, `--session-id` (then `--resume`). Paid with the person's
//!   Claude plan, or their Anthropic key (`agents_pay`), never ambiguously
//!   (`session::environment`).
//! - **The brief over stdin**, always ending "When you're done, finish with
//!   a short summary of what you changed and anything you couldn't do.", so
//!   the report is ready to say.
//! - **Followed** ([`Follow`]): each tool call as a plain line
//!   (`steps.rs`), the files it changes (its Edit / Write calls, and what
//!   its commands changed: `changes.rs`) and what changed in them once the call went through
//!   (`diff.rs`), the commands it runs and what they're for, its to-do
//!   list, what it says between its calls, what it wasn't allowed to do,
//!   Claude Code's problems (signed out, at the plan's limit, paying
//!   another way: that one is stopped at once) and the result — each sent
//!   as the app event **`yap-agent`** (`{id, status, folder, conversation?,
//!   step?, file?, command?, about?, said?, thought?, todos?, now?, diff?, text?,
//!   cost?, problem?, ts}`) and kept in the agent's `log.jsonl` for its tab.
//! - **Asking**: a permission card waiting for the person (the endpoint's
//!   `pending()`): the agent is `asking` until it's answered.
//! - **Its turns** (2026-11-07: one agent per Code chat, as Claude Code is one
//!   session per conversation): each request to it is a turn of its own
//!   (`store::TurnNote`: the brief, then each follow-up — `start_agent` in its
//!   Code chat, `message_agent`, the tab's "Tell the agent…"), on the same
//!   session (`--resume`), at the chat's Mode then (`modes::next_turn`: a
//!   plan stays a plan until "go ahead"), Settings' model and effort then
//!   (unless the person asked for their own), each with its own checkpoint
//!   and its own report. Continue carries on the same turn.
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
use std::time::{Duration, Instant, SystemTime};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::ai::claude_code::cli;
use crate::ai::claude_code::events::{Block, Event, Problem, StreamKind, TurnResult};
use crate::ai::claude_code::session::{self, ApiKey, Conversation, Effort, Image, Pay, Session, Spec};
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
/// **Its words as they're written** (12 Nov 2026): `{id, turn, text}`, the
/// message it's writing so far, to the page only and many times a second —
/// an event of its own so nothing that follows [`EVENT`] hears it. Never
/// in its log, nor to a phone: once the message is whole it's a `said`
/// line (or its final words), and the page lets this go.
pub const SAYING_EVENT: &str = "yap-agent-saying";
/// At most this often ([`SAYING_EVENT`]).
const SAYING_EVERY: Duration = Duration::from_millis(70);
/// A thought's most characters in its log ([`AgentEvent::thought`]): past
/// them, cut at a character's edge with "…".
const THOUGHT_CHARS: usize = 12_000;
/// A helper's report's most characters ([`AgentEvent::report`]), "…" after: a
/// line of its log keeps 4,000 (`store.rs` `LOG_TEXT_MAX`).
const REPORT_CHARS: usize = 3_999;

/// **What a helper came back with, in its own words** (Lab, 15 Nov 2026: its
/// view showed Claude Code's whole hand-back — the frame it puts round a
/// subagent's report for the model, the `agentId: …` line, `<usage>…`).
/// Claude Code's record of the call (`tool_use_result`'s `content` text)
/// when it has one; else the answer's words with that frame taken off. Pure.
pub fn helper_words(text: &str, result: Option<&serde_json::Value>) -> String {
    let own: Vec<&str> = result.and_then(|r| r.get("content")).and_then(serde_json::Value::as_array).into_iter().flatten().filter_map(|p| p.get("text").and_then(serde_json::Value::as_str)).collect();
    let own = own.join("\n\n");
    if !own.trim().is_empty() {
        return own.trim().to_string();
    }
    unframed(text)
}

/// A subagent's hand-back without its frame: what comes before "The report
/// follows:" (when it opens "[Subagent hand-back]"), a closing `<usage>`
/// block and `agentId:` line, and the indent the frame puts on every line.
/// Anything else as it is. Pure.
fn unframed(text: &str) -> String {
    let mut s = text;
    if s.trim_start().starts_with("[Subagent hand-back]") {
        if let Some(i) = s.find("The report follows:") {
            s = &s[i + "The report follows:".len()..];
        }
    }
    let mut s = s.trim_end().to_string();
    if s.ends_with("</usage>") {
        if let Some(i) = s.rfind("<usage>") {
            s.truncate(i);
        }
    }
    let kept = s.trim_end().len();
    s.truncate(kept);
    if let Some(i) = s.rfind('\n').filter(|&i| s[i + 1..].trim_start().starts_with("agentId:")) {
        s.truncate(i);
    }
    let lines: Vec<&str> = s.lines().collect();
    let indent = lines.iter().filter(|l| !l.trim().is_empty()).map(|l| l.len() - l.trim_start().len()).min().unwrap_or(0);
    lines.iter().map(|l| l.get(indent..).unwrap_or(l.trim_start())).collect::<Vec<_>>().join("\n").trim().to_string()
}

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
    /// **Its thinking** (15 Nov 2026): a thinking block of its own message,
    /// as Claude Code's stream hands it over (a helper's never), at most
    /// [`THOUGHT_CHARS`]. Shown only in a Code chat's Thinking and Verbose
    /// views, as Claude Code's app shows it in its transcript; never to a
    /// phone, never in Yap's log.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thought: Option<String>,
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
    /// **A helper is back** (12 Nov 2026): the session's `Task` (or `Agent`)
    /// call `call` has come back — `"back"`, or `"failed"` when Claude Code
    /// marked its result an error. The Agent panel's entry for that helper
    /// stops working then, while the turn carries on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub helper: Option<String>,
    /// **What a helper came back with** (15 Nov 2026), with its `helper`:
    /// its `Task` call's answer, at most [`REPORT_CHARS`] — the report its
    /// own view in the Agent panel ends with, as the Claude app's view of a
    /// background task does. Never to a phone (`link/code/view.rs`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<String>,
    /// A picture a tool call's answer carried (`pictures.rs`): its step
    /// (`call`) shows it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<super::pictures::StepPicture>,
    /// **A message sent while it worked, read now** (14 Nov 2026, the
    /// message box's queue): its chat message's id (`chat:<conv>:<index>`),
    /// with the "Read your note" step — its bubble's ✓✓.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read: Option<String>,
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
    /// **The screenshots it came with** (12 Nov 2026): the person's own,
    /// attached to a message that went straight to a Code chat's session
    /// (`agents/direct.rs`) — their ids in `chat-images/`. Written to
    /// Claude Code with its words ([`shots_of`]), wherever they go: a note
    /// to the turn working, the turn starting, its next turn.
    pub shots: Vec<String>,
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
        add_shots(&mut self.shots, &more.shots);
    }
}

/// `more` after `shots`, each once.
fn add_shots(shots: &mut Vec<String>, more: &[String]) {
    for id in more {
        if !shots.contains(id) {
            shots.push(id.clone());
        }
    }
}

/// **A request's screenshots as Claude Code takes them** (`ids` in
/// `chat-images/`): each as Chat's own model on the plan would get it
/// (`snip::images::fitted`: made already when the message was sent, so
/// this is usually a look-up), and how many couldn't be read (gone since:
/// "Keep chat screenshots"). Slow when a copy has to be made: off the
/// async runtime where there is one.
fn shots_of(ids: &[String]) -> (Vec<Image>, usize) {
    let fit = crate::snip::images::fit_for(crate::ai::claude_plan::PROVIDER, false);
    let cap = fit.cap_for(ids.len());
    let made: Vec<Image> = ids
        .iter()
        .filter_map(|id| crate::snip::images::fitted(id, fit.edge, cap).ok())
        .filter_map(|f| {
            let (head, data) = f.url.split_once(',')?;
            let media = head.strip_prefix("data:")?.strip_suffix(";base64")?;
            Some(Image { media_type: media.to_string(), base64: data.to_string() })
        })
        .collect();
    let lost = ids.len() - made.len();
    (made, lost)
}

/// **Does a turn's first message go with the turn's screenshots?** Not a
/// Continue of a session that has run ("Carry on…": it has them already,
/// and sending them again would be paid for again). Pure.
fn with_shots(resuming: bool, message: Option<&str>) -> bool {
    !(resuming && message.is_some_and(|m| m.starts_with(CARRY_ON)))
}

/// A line for Claude Code when screenshots of a request couldn't be read.
fn lost_line(lost: usize) -> String {
    match lost {
        1 => "(A screenshot that came with this is no longer on this PC.)".into(),
        n => format!("({n} screenshots that came with this are no longer on this PC.)"),
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
    /// Words for this turn that came while it started (sent with its first),
    /// and their screenshots.
    extra: Vec<String>,
    extra_shots: Vec<String>,
    /// A request for its next turn, waiting for this one to end.
    pending: Option<Ask>,
    /// The person stopped it (when).
    stopping: Option<Instant>,
    /// The person allowed its plan this turn (ExitPlanMode's card: Go
    /// ahead): a plan turn's next turn carries it out ([`plan_allowed`]).
    plan_allowed: bool,
    /// Its folder, compared ([`workspaces::norm`]).
    folder: String,
    /// **Its own tool calls out now** (begun, not back; a helper's aren't): a
    /// message written now is taken in as the one running ends ([`queue_state`]).
    tools_out: u32,
}

impl Live {
    fn new(folder: String) -> Live {
        Live { session: None, sent: 0, results: 0, written: Vec::new(), taken: 0, notes: Vec::new(), input_over: false, started: false, extra: Vec::new(), extra_shots: Vec::new(), pending: None, stopping: None, plan_allowed: false, folder, tools_out: 0 }
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

/// **The person sent agent `id`'s plan back with comments** (Plan mode's
/// review: "Revise the plan" on ExitPlanMode's card,
/// `claude_live/approve.rs`): Claude Code stays in Plan mode and answers with
/// a new plan, this turn still; the plan and its comments are kept on the
/// turn ([`store::TurnNote::plans_before`]), folded in the thread above the
/// new one.
pub fn plan_revised(id: &str, plan: &str, comments: &[store::PlanComment]) {
    let now = store::now_secs();
    let kept = store::update(id, |j| {
        let n = j.turn_list.iter().rev().find(|t| t.status.live()).or_else(|| j.turn_list.last()).map(|t| t.n);
        if let Some(t) = n.and_then(|n| j.turn_mut(n)) {
            t.plans_before.push(store::PlanBefore { plan: plan.to_string(), comments: comments.to_vec(), ts: now });
        }
    });
    if let Some(agent) = kept {
        tracing::info!(id, comments = comments.len(), "Agents: its plan sent back with comments (it revises it)");
        told_changed(&agent, "plan-revised");
    }
}

/// An ExitPlanMode refused because the person sent the plan back with
/// comments (`plans`: the turn's plans before): a revision, never a
/// denial of the agent's. Pure.
pub fn revised_plan(tool: &str, input: &serde_json::Value, plans: &[store::PlanBefore]) -> bool {
    tool == claude_live::EXIT_PLAN && input.get("plan").and_then(serde_json::Value::as_str).is_some_and(|p| plans.iter().any(|b| b.plan.trim() == p.trim()))
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

/// What wasn't over when the Yap bar was last told: agents sent out and Code
/// chats at work on the person's own words, as one number ([`told`]).
static LIVE_TOLD: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(usize::MAX);

/// `(agents sent, chats at work)` as one number, either changing it. Pure.
fn told((sent, chats): (usize, usize)) -> usize {
    (sent << 16) | chats.min(0xFFFF)
}

/// Send `ev` to the page and keep it in the agent's log; the Yap bar hears
/// when what's at work changes ("1 agent working", "Claude is working").
pub fn emit(ev: &AgentEvent) {
    let v = serde_json::to_value(ev).ok();
    let line = v.as_ref().and_then(|v| store::log_append(&ev.id, v));
    if let Some(app) = APP.get() {
        let _ = app.emit(EVENT, ev);
        let live = told(store::live_counts());
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
/// **Agent `agent`'s record changed with no step of its own** (`changed`:
/// what — "rewound", …): the pages read it again.
pub(crate) fn told_changed(agent: &Agent, what: &str) {
    emit(&AgentEvent { changed: Some(what.to_string()), ..AgentEvent::of(agent) });
}

fn emit_live(ev: &AgentEvent) {
    if let Some(app) = APP.get() {
        let _ = app.emit(EVENT, ev);
    }
}

/// The message agent `id`'s turn `n` is writing, so far ([`SAYING_EVENT`]).
fn emit_saying(id: &str, n: u32, text: &str) {
    if let Some(app) = APP.get() {
        let _ = app.emit(SAYING_EVENT, serde_json::json!({ "id": id, "turn": n, "text": text }));
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

/// **The chats with an agent at work now** (working, starting, a turn
/// queued, or waiting for a slot; a Code chat's own Claude is one too):
/// auto-archive never takes them (`chat/auto_archive.rs`).
pub fn working_chats() -> Vec<u64> {
    let mine: Vec<(String, u64)> = store::list().into_iter().filter_map(|a| a.conversation.map(|c| (a.id, c))).collect();
    with_state(|s| mine.into_iter().filter(|(id, _)| s.live.contains_key(id) || s.queue.iter().any(|(q, _, _)| q == id)).map(|(_, c)| c).collect())
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

/// What a turn sends first: the brief (with [`closing`]'s line) or `message`.
fn opening(agent: &Agent, message: Option<&str>, read_only: bool, direct: bool) -> String {
    match message {
        Some(m) => m.to_string(),
        None => ended(&agent.brief, closing(read_only, direct)),
    }
}

/// **The session a branched run starts** ([`store::Branch`], a Code chat's
/// Rewind or Fork from here): Claude Code's own fork of `session` up to and
/// including its message `at`, or — rewound to before its first message,
/// or no session to fork — a new one. Pure.
pub(crate) fn branched(session: &str, branch: &store::Branch) -> Conversation {
    // (The conversation as text instead: a new session.)
    if branch.seed.is_some() {
        return Conversation::New(session::new_session_id());
    }
    let from = branch.from.as_deref().filter(|f| !f.is_empty()).unwrap_or(session);
    match branch.at.as_deref().filter(|a| !a.is_empty()) {
        Some(at) if !from.is_empty() => Conversation::Branch { from: from.to_string(), at: at.to_string() },
        _ => Conversation::New(session::new_session_id()),
    }
}

/// **What a turn's message ends with**: [`SUMMARY_LINE`], or a read-only
/// turn's [`READ_ONLY_LINE`] — and nothing at all for the person's own
/// message in a Code chat that works directly (`direct`, 12 Nov 2026).
/// The summary line is for a brief Chat's model worded, whose final words
/// are relayed as a report. Added to the person's own words it made a plain
/// "just testing" come back as "I didn't change anything this time…": in
/// Claude Code's own app a message is what was typed, nothing more.
fn closing(read_only: bool, direct: bool) -> &'static str {
    if read_only {
        READ_ONLY_LINE
    } else if direct {
        ""
    } else {
        SUMMARY_LINE
    }
}

/// **What a Code chat's own Claude may use of Yap's browser** (14 Nov 2026,
/// the maintainer: "claude-code doesn't have browser tools? It should have
/// shouldn't it?" — as Claude Code's own app has its browser pane): the
/// pages beside the chat and the web's pictures with the web on ("Look
/// things up on the web"), else picture files by a path — and the screen
/// look with "Let Yap's AI see your screen" on (the maintainer, the same
/// day: "Chat-code should be able to see the screen, if asked"), the plan
/// being a cloud model: refused, and the alert asking once, until "…with
/// cloud models too" is on (`see_screen::execute`); never in a meeting or a
/// call. As a plan Code chat's session has them (`chat/turn/plan.rs`
/// `session_eyes`), acting on its pages too (below). Pure but for the
/// model's image reading.
fn direct_eyes(cfg: &crate::config::YapConfig, model: &str, phone: bool) -> claude_live::eyes::Eyes {
    use crate::chat::pictures::Gate;
    let provider = crate::ai::claude_plan::PROVIDER;
    let pictures = Gate::for_model(cfg.web_search, provider, model);
    let files = if pictures == Gate::Off { Gate::for_model(true, provider, model) } else { Gate::Off };
    let screen = crate::chat::see_screen::gate(cfg, true, provider, model);
    // **Acting on the pages** (14 Nov 2026; the maintainer: "get that browser
    // tool to work exactly like it does inside this claude-code app" — his
    // Code chat's Claude couldn't open a page's collapsed section): click,
    // type, keys, hover, drag and a field set, as a plan chat's Claude has
    // them (`chat/pages/act.rs`, every rule and card of theirs), with the
    // pages on — never a phone's turn (no panel there to watch), never in a
    // meeting or a call (`eyes::refused`).
    let act = pictures != Gate::Off && !phone && crate::chat::pages::act::can_act(provider, model);
    claude_live::eyes::Eyes { screen, pictures, act, files }
}

/// **Its browser turn's context**: the person's message this turn, their
/// earlier ones in the chat (an address they gave opens), the chat itself.
fn beside_context(agent: &Agent, n: u32, eyes: claude_live::eyes::Eyes, unattended: bool, visuals: bool) -> claude_live::turn::Context {
    let question = agent.turn_list.iter().find(|t| t.n == n).map(|t| t.request.clone()).filter(|q| !q.trim().is_empty()).unwrap_or_else(|| agent.brief.clone());
    let said = agent
        .conversation
        .and_then(crate::chat::chats::get)
        .map(|c| c.messages.iter().filter(|m| m.role == "user" && m.kind.is_empty()).map(|m| m.text.clone()).collect())
        .unwrap_or_default();
    // (Its workspace: the dev servers its Claude starts, `chat/pages/preview.rs`.)
    let workspace = (!agent.path.trim().is_empty()).then(|| agent.path.clone());
    // An element picked on a page beside the chat: web content from the turn's start (`chat/pages/pick.rs`).
    let web = crate::chat::pages::pick::has_element(&question);
    claude_live::turn::Context { question, conversation: agent.conversation, eyes, said, watched: true, unattended, workspace, web, visuals, ..Default::default() }
}

/// **Its browser turn's web side**, as a plan chat's (`plan.rs` `web_side`):
/// the person's links (this message, their earlier ones, the sources of
/// earlier answers), the per-turn caps, the pictures and pages it shows.
fn beside_web(cfg: &crate::config::YapConfig, agent: &Agent, ctx: &claude_live::turn::Context) -> Option<crate::web::Turn> {
    use crate::chat::pictures::Gate;
    if ctx.eyes.pictures == Gate::Off && ctx.eyes.files == Gate::Off {
        return None;
    }
    let settings = crate::web::Settings::for_chat(cfg, crate::ai::claude_plan::PROVIDER, &agent.model, crate::ai::claude_plan::BASE_URL, "");
    let earlier: Vec<&str> = ctx.said.iter().map(String::as_str).collect();
    // (A Claude Code session: none of Chat's per-turn counts, `web::Turn::uncapped`.)
    let mut turn = crate::web::Turn::new(settings, crate::web::Endpoints::current(), &ctx.question, &earlier).uncapped();
    if let Some(conv) = agent.conversation.and_then(crate::chat::chats::get) {
        turn = turn.with_earlier_sources(conv.messages.iter().flat_map(|m| m.sources.iter().map(|s| s.url.as_str())));
        turn.pictures.local = crate::chat::pictures::local::Context::of_chat(&conv, &ctx.question);
    }
    Some(turn)
}

/// The next thing its browser showed (never, without one).
async fn next_shown(beside: &mut Option<claude_live::turn::Turn>) -> Option<crate::chat::stream::Event> {
    match beside {
        Some(t) => t.events.recv().await,
        None => std::future::pending().await,
    }
}

/// **What its browser shows, to the chat** (`yap-agent-shown`: a page
/// opened beside it, a picture shown, files shared): the panel opens on it
/// as on a plan chat's (`chatState` `agentShown`).
fn shown_beside(agent: &Agent, n: u32, ev: crate::chat::stream::Event) {
    if let Some(app) = APP.get() {
        let _ = app.emit(SHOWN_EVENT, serde_json::json!({ "id": agent.id, "turn": n, "conversation": agent.conversation, "event": ev }));
    }
}

/// The page's event for what a Code chat's own Claude showed beside the chat.
pub const SHOWN_EVENT: &str = "yap-agent-shown";

/// **The next-step box and the chapter a turn's session put in its chat**
/// (`chat/cards.rs`: `Event::NextStep`, `Event::Chapter` through its turn's
/// stream), the latest of each: its answer keeps them (`TurnNote::beside`).
#[derive(Debug, Default)]
struct Carded {
    next_step: Option<crate::chat::cards::next_step::NextStep>,
    chapter: Option<crate::chat::cards::chapters::Chapter>,
}

impl Carded {
    fn see(&mut self, ev: &crate::chat::stream::Event) {
        match ev {
            crate::chat::stream::Event::NextStep { next } => self.next_step = Some(next.clone()),
            crate::chat::stream::Event::Chapter { chapter } => self.chapter = Some(chapter.clone()),
            _ => {}
        }
    }
}

/// What turn `n` showed beside the chat, kept for its answer (`TurnNote::beside`).
fn keep_beside(id: &str, n: u32, shown: claude_live::turn::Shown, carded: Carded) {
    let beside = store::Beside { pages: shown.pages, pictures: shown.pictures, files: shown.files, next_step: carded.next_step, chapter: carded.chapter };
    if beside.is_empty() {
        return;
    }
    tracing::info!(id, turn = n, pages = beside.pages.len(), pictures = beside.pictures.len(), files = beside.files.len(), "Agents: what its browser showed, kept for its answer");
    store::touch(id, |j| {
        if let Some(t) = j.turn_mut(n) {
            t.beside.add(&beside);
        }
    });
}

/// **The line a turn starts on** (its first step): "Starting Claude Code"
/// for an agent sent out, and "Thinking" — as Chat's own turn says it
/// (`chat/activity.rs`) — for the person's own message in a Code chat that
/// works directly (14 Nov 2026). Each of its turns starts Claude Code again
/// on the chat's session, but that's Yap's plumbing: "Starting Claude Code"
/// on every message read as a new Claude spawned each time.
pub(crate) fn start_line(direct: bool) -> &'static str {
    if direct {
        "Thinking"
    } else {
        "Starting Claude Code"
    }
}

/// `text`, then `end` after a blank line when there is one.
fn ended(text: &str, end: &str) -> String {
    if end.is_empty() {
        text.trim().to_string()
    } else {
        format!("{}\n\n{end}", text.trim())
    }
}

/// **A note handed to the turn working**, as it's written to it: with
/// [`NOTE_LINE`] — or, the person's own message in a Code chat that works
/// directly, as they wrote it (Claude Code takes a message sent mid-work
/// into its work by itself).
fn note_text(ask: &Ask) -> String {
    ended(&ask.text, if ask.direct { "" } else { NOTE_LINE })
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

/// **Claude Code's to-do list, on in every run** (15 Nov 2026): since
/// 2.1.268 newer models (Opus 5.5, Sonnet 5.5…) get no task-tracking tool
/// unless the session opts in (Claude Code's tools reference, "Task tool
/// availability"), so a Code chat's to-do card never showed on them. On, and
/// as `TodoWrite` — the list Yap reads (`steps.rs`) — rather than the four
/// Task tools.
pub const TODO_ENV: [(&str, &str); 2] = [("CLAUDE_CODE_ENABLE_TODO_TOOLS", "1"), ("CLAUDE_CODE_ENABLE_TASKS", "0")];

/// **Claude Code's own file checkpoints** (16 Nov 2026, the Agent SDK's
/// `enableFileCheckpointing`, documented for `claude -p` as this variable):
/// before Write, Edit or NotebookEdit changes a file, Claude Code keeps it,
/// and `--rewind-files <message uuid>` takes the files back to before that
/// message — what the Claude app's Rewind does. Yap's Rewind uses it
/// (`rewind.rs`); Yap keeps no copies of its own.
pub const CHECKPOINT_ENV: (&str, &str) = ("CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING", "true");

/// **An agent's Claude Code run**: in its folder, the person's own setup (no
/// `--strict-mcp-config`, no `--setting-sources`, Claude Code's own tools)
/// with Yap's endpoint added (the run's `mcp_config`, its `settings`, and
/// `env`: the run key it names; Claude Code's own file checkpoints on), its permission
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
    // Its to-do list on every model, as TodoWrite (the thread's to-do card, the plan from before a message).
    spec.env.extend(TODO_ENV.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    // Claude Code's own file checkpoints: what Rewind takes the files back with.
    spec.env.push((CHECKPOINT_ENV.0.to_string(), CHECKPOINT_ENV.1.to_string()));
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
    // **The workspace as the turn found it** (`changes.rs`), read while
    // Claude Code's status is checked: what its commands make or change is
    // found against it at the end.
    let scanned = PathBuf::from(&agent.path);
    let scan = tauri::async_runtime::spawn_blocking(move || super::changes::before(&scanned));
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
    let mode = modes::or_auto(&agent.mode);
    // **A phone's turn runs at most at Auto** (Remote Control, design §5.3),
    // whatever set its mode: its question asked for it, or the phone's own
    // action carried it on (`agents/remote.rs`). The last gate, here.
    let asked = agent.turn_list.iter().find(|t| t.n == n).and_then(|t| t.message_id.clone());
    let phone = super::remote::phone_of_turn(id, n, asked.as_deref());
    let mode = match phone {
        Some(_) if mode > super::remote::PHONE_MAX => {
            tracing::info!(id, turn = n, was = mode.as_str(), "Agents: a phone's turn, run at Auto");
            super::remote::phone_mode(mode)
        }
        _ => mode,
    };
    // A read-only turn ("catch up", "have a look"): its editing tools off.
    let read_only = agent.turn_list.iter().find(|t| t.n == n).is_some_and(|t| t.read_only);
    // The person's own message, straight to the Code chat's session.
    let direct = agent.turn_list.iter().find(|t| t.n == n).is_some_and(|t| t.direct);
    let run = match claude_live::register_run(RunSpec {
        label: agent.folder.clone(),
        tools: Tools::Agent,
        folder: Some(folder.clone()),
        // Bypass: nobody is asked while it runs (a prompt is answered no, no card).
        approval: Some(Approval { run: id.to_string(), conversation: agent.conversation, origin: agent.origin, unattended: modes::unattended(mode) }),
        agents: false,
        memory: false,
        // An agent Chat sent works with the person's own tools, never Yap's
        // screen or pages, nor Yap's connectors (its own Claude Code setup
        // has the person's). A Code chat's own session has Yap's browser
        // beside the chat and the screen look (14 Nov 2026), acting on its
        // pages too (the same day, as Claude Code's own app's browser).
        eyes: if direct { direct_eyes(&cfg, &agent.model, phone.is_some()) } else { Default::default() },
        connectors: None,
        // A Code chat's own session (the person's message went straight to
        // it) reads Yap's meetings, notes, tasks and calendar on request, as
        // a plan Code chat's session always could; an agent Chat sent out
        // doesn't (Chat's model reads for its brief).
        reads: direct,
        // The next-step box and chapters in its chat (chat/cards.rs, 15 Nov 2026), a Code chat's own
        // session's; its questions are Claude Code's own AskUserQuestion (approve.rs).
        cards: if direct && agent.conversation.is_some() { crate::chat::cards::Cards::for_code_session(phone.is_some()) } else { Default::default() },
        // (A Code chat is in no project.)
        projects: false,
    }) {
        Ok(r) => r,
        Err(e) => return failed_early(id, n, format!("Couldn't start the agent: {e}"), None),
    };
    // **Its browser beside the chat** (a Code chat's own session, 14 Nov
    // 2026): this turn's calls belong to it — the person's message and their
    // earlier ones (the pages it may open), the chat — and what they show
    // goes to the chat's panel (`yap-agent-shown`), its answer keeping it.
    let eyes = if direct { direct_eyes(&cfg, &agent.model, phone.is_some()) } else { Default::default() };
    // Visuals in the chat's thread (chat/made.rs, 15 Nov 2026): a turn of the person's in Chat, never a phone's.
    let visuals = direct && phone.is_none() && agent.conversation.is_some();
    // The next-step box and chapters (chat/cards.rs): a turn of the person's in its chat; a phone's has no box.
    let cards = if direct && agent.conversation.is_some() { crate::chat::cards::Cards::for_code_session(phone.is_some()) } else { Default::default() };
    let mut beside = (direct && (eyes.any() || visuals || cards.any())).then(|| {
        // Bypass permissions: nobody is asked, a page action's card either (its instructions have Claude ask in the chat).
        let ctx = claude_live::turn::Context { cards, ..beside_context(&agent, n, eyes, modes::unattended(mode), visuals) };
        let web = beside_web(&cfg, &agent, &ctx);
        run.begin_turn(ctx, web)
    });
    let resuming = message.is_some() && agent.started_ts > 0 && !agent.session.is_empty();
    let session_id = if agent.session.is_empty() { session::new_session_id() } else { agent.session.clone() };
    // A Rewind or a Fork from here: Claude Code's own fork of the session at a message, or a new one.
    let conversation = match &agent.branch {
        Some(b) => branched(&agent.session, b),
        None if resuming => Conversation::Resume(session_id),
        None => Conversation::New(session_id),
    };
    // (A branch Claude Code may refuse: if it never starts, the next turn starts afresh with the conversation as text.)
    let branching = matches!(conversation, Conversation::Branch { .. });
    let seed = agent.branch.as_ref().and_then(|b| b.seed.clone()).filter(|s| !s.trim().is_empty());
    let mut spec = agent_spec(&agent, &folder, pay, mode, read_only, conversation, run.mcp_config(), run.settings(), run.env());
    // Yap's read tools, where the run has them: no prompt for a read of the
    // person's own data (none for any other run: its list is empty).
    spec.allowed_tools = run.allowed_tools();
    if read_only {
        tracing::info!(id, turn = n, "Agents: a read-only turn (its editing tools off)");
    }
    // (Read before Claude Code starts: none of its commands can have run yet.)
    let before = match scan.await {
        Ok(Ok(b)) => Some(b),
        Ok(Err(why)) => {
            let (words, entries) = why.said();
            tracing::info!(id, turn = n, why = words, entries, "Agents: what this turn's commands change won't be checked");
            None
        }
        Err(_) => None,
    };
    let (session, mut rx) = match Session::start_with(&program, spec) {
        Ok(s) => s,
        Err(e) => return failed_early(id, n, e, None),
    };
    let session = Arc::new(session);
    // The person stopped it while it started: ended now. Words for this
    // turn that came meanwhile go with its first message.
    let (stopped_early, extra, extra_shots) = with_state(|s| match s.live.get_mut(id) {
        Some(l) => {
            l.session = Some(Arc::clone(&session));
            l.started = true;
            l.sent = 1;
            (l.stopping.is_some(), std::mem::take(&mut l.extra), std::mem::take(&mut l.extra_shots))
        }
        None => (true, Vec::new(), Vec::new()),
    });
    let mut first = std::iter::once(opening(&agent, message.as_deref(), read_only, direct)).chain(extra.iter().map(|t| format!("Also: {t}"))).collect::<Vec<_>>().join("\n\n");
    // A rewound or forked chat Claude Code couldn't branch: the conversation kept so far first, as text.
    if let Some(seed) = &seed {
        first = format!("{seed}\n\n{first}");
    }
    // **The screenshots its request came with** (the turn's own, and those
    // of words that came while it started), as Claude Code's own app sends
    // a pasted picture. Never again with a Continue: its session has them.
    let mut shots = if !with_shots(resuming, message.as_deref()) { Vec::new() } else { store::get(id).and_then(|j| j.turn_list.iter().find(|t| t.n == n).map(|t| t.shots.clone())).unwrap_or_default() };
    add_shots(&mut shots, &extra_shots);
    let pictures = if shots.is_empty() || stopped_early {
        Vec::new()
    } else {
        let (pictures, lost) = tauri::async_runtime::spawn_blocking(move || shots_of(&shots)).await.unwrap_or_default();
        if lost > 0 {
            first = format!("{first}\n\n{}", lost_line(lost));
        }
        tracing::info!(id, turn = n, pictures = pictures.len(), lost, "Agents: its message goes with its screenshots");
        pictures
    };
    if stopped_early {
        session.kill();
    } else if let Err(e) = session.send_user(&first, &pictures) {
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
        j.step = start_line(direct).into();
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
            t.told_at = None;
        }
    }) else {
        session.kill();
        return (Status::Failed, n);
    };
    tracing::info!(id, mode = mode.as_str(), pay = %agent.pay, resuming, turn = n, runs = agent.turns, "Agents: started");
    emit(&AgentEvent { step: Some(start_line(direct).into()), ..AgentEvent::at(&agent, n) });

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
    let mut follow = Follow::new(&agent, n, folder, mode, window, total_before);
    follow.marks = beside.as_ref().map(|t| t.marks());
    let mut tick = tokio::time::interval(Duration::from_millis(500));
    let mut shows = beside.is_some();
    // The box and the chapter its session put in the chat this turn (`Carded`), for its answer.
    let mut carded = Carded::default();
    loop {
        tokio::select! {
            ev = rx.recv() => match ev {
                Some(Event::Exited(_)) | None => break,
                Some(ev) => follow.on(ev, &session),
            },
            shown = next_shown(&mut beside), if shows => match shown {
                Some(ev) => {
                    carded.see(&ev);
                    shown_beside(&agent, n, ev);
                }
                None => shows = false,
            },
            _ = tick.tick() => follow.on_tick(&session),
        }
    }
    // What its browser showed: the rest passed on, then kept for its answer
    // (its turn over, a call still waiting stops).
    if let Some(mut t) = beside.take() {
        while let Ok(ev) = t.events.try_recv() {
            carded.see(&ev);
            shown_beside(&agent, n, ev);
        }
        keep_beside(&agent.id, n, t.shown(), carded);
        // Its turn over: the page's "Yap is using this page" goes, a card still asking lapses.
        if let Some(app) = APP.get() {
            let _ = app.emit(SHOWN_EVENT, serde_json::json!({ "id": agent.id, "turn": n, "conversation": agent.conversation, "ended": true }));
        }
    }
    // Unregistered: a permission card still waiting is a no.
    drop(run);
    // **What its commands made or changed** (`changes.rs`): into its files, before its end is said.
    let windows = follow.command_windows(SystemTime::now());
    let (at, edited) = (follow.folder.clone(), follow.edited_files.iter().cloned().collect::<Vec<_>>());
    let made = tauri::async_runtime::spawn_blocking(move || super::changes::settle(before.as_ref(), &windows, &at, &edited)).await.unwrap_or_default();
    follow.note_command_files(made);
    let ended = follow.finish();
    // A branch Claude Code never started (it refused the point, or the flag): the next turn starts afresh, the
    // conversation kept so far as text, and the turn says so.
    if branching && !session.started() && ended == Status::Failed {
        super::rewind::fall_back(&agent.id, n);
    }
    (ended, n)
}

/// **Following one turn** of an agent.
struct Follow {
    id: String,
    /// The turn (`store::TurnNote::n`): its events, its note.
    n: u32,
    folder: PathBuf,
    mode: Mode,
    /// A read-only turn: never a plan waiting for a go-ahead, whatever its
    /// mode ([`after_plan`]'s caller).
    read_only: bool,
    /// The person's own message, straight to the chat's session
    /// (`agents/direct.rs`): its final words are the chat's reply, so a
    /// question in them ("What do you need?") is part of the conversation,
    /// never "the agent has a question" (14 Nov 2026).
    direct: bool,
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
    /// **Where a message read after an answer had ended splits the turn's
    /// words** (`said`'s index): its words are from there on (`on_taken`).
    split_at: Option<usize>,
    /// **Its own latest message's place in the session** (the stream's
    /// `uuid`; a helper's aren't): the turn's `end_uuid` at its result, where
    /// a Rewind or a Fork from here resumes it (`agents/rewind.rs`).
    last_uuid: Option<String>,
    problem: Option<ProblemNote>,
    asking: bool,
    /// When its input ended (its last result came).
    input_over_at: Option<Instant>,
    /// A result came with a note still unread (since when): Claude Code
    /// starts on it now — or, [`NOTE_GRACE`] on, the note goes back to Yap.
    unread_since: Option<Instant>,
    last_flush: Instant,
    /// The edit tools' calls this turn (Write, Edit, MultiEdit, NotebookEdit), by id.
    edits: HashSet<String>,
    /// One of them went through (its result not an error).
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
    /// The helpers out now: the session's own `Task` / `Agent` calls not
    /// back yet (`AgentEvent::helper` says when each is).
    helpers: HashSet<String>,
    /// Pictures kept this turn (`pictures::PER_TURN`).
    pictures: usize,
    /// The message it's writing now, so far, and when the page was last
    /// told ([`SAYING_EVENT`]).
    saying: String,
    saying_told: Option<Instant>,
    /// The commands' calls not back yet, by id (their output is said).
    commands: HashSet<String>,
    /// **When each `Bash` / `PowerShell` call began**, not back yet (and
    /// whether it runs in the background: then to the turn's end), and the
    /// runs of those back: what changed in the workspace meanwhile is the
    /// commands' (`changes.rs`).
    command_started: HashMap<String, (SystemTime, bool)>,
    command_runs: Vec<super::changes::Window>,
    /// The files Claude Code's own edit tools named this turn (as `files`
    /// says them): theirs, never a command's, its hooks on or off.
    edited_files: HashSet<String>,
    /// The files its Write calls made (their result: a new file), for the
    /// turn's `created`.
    written_new: Vec<String>,
    /// Characters of output kept this turn (`output::TURN_MAX`).
    output_used: usize,
    /// What the turn has used (`usage.rs`).
    meter: super::usage::Meter,
    /// **Its browser turn** (a Code chat's own session): told as Claude Code
    /// begins a call to one of Yap's tools or a web read of its own, and the
    /// pages its searches and reads found, as a plan turn tells its run
    /// (`claude_live/turn.rs` `Marks`): the safety rule, and a search's
    /// result that may open beside the chat.
    marks: Option<claude_live::turn::Marks>,
    /// Its own `WebSearch` (None) and `WebFetch` (its address) calls not
    /// back yet, by id.
    web_calls: HashMap<String, Option<String>>,
    /// The pages they found, numbered as found.
    found: Vec<crate::chat::chats::Source>,
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

/// A Write's kind, from Claude Code's own result: "new" when it made the
/// file (its `tool_use_result.type` is "create", or its words say "File
/// created successfully"), "rewrite" when it was there ("update", or the
/// agent had touched it before), "write" when it doesn't say. Pure.
fn write_kind(known: bool, record: Option<&serde_json::Value>, text: &str) -> &'static str {
    let said = record.and_then(|r| r.get("type")).and_then(serde_json::Value::as_str);
    match said {
        Some("create") => "new",
        Some("update") => "rewrite",
        _ if text.trim_start().starts_with("File created successfully") => "new",
        _ if known || text.contains("has been updated") => "rewrite",
        _ => "write",
    }
}

/// The names Claude Code gives Yap's own tools: `mcp__yap__browser_navigate`.
const YAP_TOOLS: &str = "mcp__yap__";

/// **A page found** (a search's result, a page fetched), numbered as found,
/// each once — as a plan turn numbers its sources (`claude_plan/map.rs`).
/// Only http(s). Pure.
fn add_found(found: &mut Vec<crate::chat::chats::Source>, title: &str, url: &str) {
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) || found.iter().any(|s| crate::web::normal_url(&s.url) == crate::web::normal_url(url)) {
        return;
    }
    let title = title.trim();
    found.push(crate::chat::chats::Source {
        n: found.len() + 1,
        title: if title.is_empty() { crate::web::short_url(url) } else { steps::one_line(title, 120) },
        url: url.to_string(),
        site: crate::web::site_of(url),
        ..Default::default()
    });
}

/// Whether `tool` is one of Claude Code's file-editing tools (`Edit`,
/// `Write`, `MultiEdit`, `NotebookEdit`): the ones its checkpoints cover. Pure.
fn backed_up_tool(tool: &str) -> bool {
    matches!(tool, "Edit" | "Write" | "MultiEdit" | "NotebookEdit")
}

impl Follow {
    fn new(agent: &Agent, n: u32, folder: PathBuf, mode: Mode, window: u64, total_before: Option<f64>) -> Follow {
        let before = agent.turn_list.iter().find(|t| t.n == n).and_then(|t| t.usage.clone());
        Follow {
            id: agent.id.clone(),
            n,
            folder,
            mode,
            read_only: agent.turn_list.iter().find(|t| t.n == n).is_some_and(|t| t.read_only),
            direct: agent.turn_list.iter().find(|t| t.n == n).is_some_and(|t| t.direct),
            cost_before: agent.cost,
            turn_cost_before: agent.turn_list.iter().find(|t| t.n == n).map_or(0.0, |t| t.cost),
            spent: 0.0,
            total_seen: total_before,
            model: agent.model.clone(),
            last_text: String::new(),
            outcome: None,
            said: Vec::new(),
            split_at: None,
            last_uuid: None,
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
            helpers: HashSet::new(),
            pictures: 0,
            saying: String::new(),
            saying_told: None,
            commands: HashSet::new(),
            command_started: HashMap::new(),
            command_runs: Vec::new(),
            edited_files: HashSet::new(),
            written_new: Vec::new(),
            output_used: 0,
            meter: super::usage::Meter::new(before, &agent.model, window),
            marks: None,
            web_calls: HashMap::new(),
            found: Vec::new(),
        }
    }

    /// **The commands' runs this turn**, each still going (not back, or in
    /// the background) ended at `end`.
    fn command_windows(&self, end: SystemTime) -> Vec<super::changes::Window> {
        let mut all = self.command_runs.clone();
        all.extend(self.command_started.values().map(|(began, _)| (*began, end)));
        all
    }

    /// **What its commands made or changed** (`changes::settle`), and the
    /// files its Write calls made: in the turn's `files` (Outputs, the
    /// summary, the file view), its `by_command` (Files changed, the report)
    /// and its `created` (the report card's new files). A Continue's run adds
    /// to what the turn's earlier ones found.
    fn note_command_files(&self, made: Vec<store::CommandFile>) {
        let n = self.n;
        if made.is_empty() && self.written_new.is_empty() {
            return;
        }
        tracing::info!(id = %self.id, turn = n, files = made.len(), new = made.iter().filter(|c| c.new).count(), written = self.written_new.len(), "Agents: files its commands made or changed");
        let new_ones: Vec<String> = self.written_new.iter().cloned().chain(made.iter().filter(|c| c.new).map(|c| c.file.clone())).collect();
        store::update(&self.id, |j| {
            for c in &made {
                if !j.files.contains(&c.file) {
                    j.files.push(c.file.clone());
                }
            }
            if let Some(t) = j.turn_mut(n) {
                for c in &made {
                    if !t.files.contains(&c.file) {
                        t.files.push(c.file.clone());
                    }
                    match t.by_command.iter_mut().find(|k| k.file == c.file) {
                        Some(k) => k.new |= c.new,
                        None => t.by_command.push(c.clone()),
                    }
                }
                for f in &new_ones {
                    if !t.created.contains(f) {
                        t.created.push(f.clone());
                    }
                }
            }
        });
    }

    /// **Claude Code began a call** (its stream's `content_block_start`):
    /// its browser turn hears of one to Yap's tools (`mcp__yap__…`) and of
    /// its own web reads, before they run.
    fn began(&self, name: &str) {
        let Some(m) = &self.marks else { return };
        if let Some(yap) = name.strip_prefix(YAP_TOOLS) {
            m.began(yap);
        } else if matches!(name, "WebSearch" | "WebFetch") {
            m.web();
        }
    }

    /// **A web read of its own came back** (`id`'s): the pages it found go to
    /// its browser turn — a search's results, a fetch's address — so one may
    /// open beside the chat, as in a plan chat.
    fn web_back(&mut self, id: &str, is_error: bool, text: &str, record: Option<&serde_json::Value>) {
        let Some(call) = self.web_calls.remove(id) else { return };
        if is_error {
            return;
        }
        let links = match call {
            None => crate::ai::claude_plan::map::search_links(record, text),
            Some(url) => vec![(String::new(), url)],
        };
        for (title, url) in links {
            add_found(&mut self.found, &title, &url);
        }
        if let Some(m) = &self.marks {
            m.found(&self.found);
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
        // One of its own calls out now (the queue: `queue_state`).
        if self.under.is_none() {
            with_state(|s| {
                if let Some(l) = s.live.get_mut(&self.id) {
                    l.tools_out += 1;
                }
            });
        }
        if backed_up_tool(name) {
            self.edits.insert(id.to_string());
        }
        // A command begins: what changes in the workspace while it runs is its (a helper's too).
        if matches!(name, "Bash" | "PowerShell") && !id.is_empty() {
            let background = input.get("run_in_background").and_then(serde_json::Value::as_bool) == Some(true);
            self.command_started.insert(id.to_string(), (SystemTime::now(), background));
        }
        // Its own web reads, with a browser turn: what they find may open beside the chat.
        if self.marks.is_some() && self.under.is_none() {
            match name {
                "WebSearch" => {
                    self.web_calls.insert(id.to_string(), None);
                }
                "WebFetch" => {
                    let url = input["url"].as_str().unwrap_or("").trim().to_string();
                    if !url.is_empty() {
                        self.web_calls.insert(id.to_string(), Some(url));
                    }
                }
                _ => {}
            }
        }
        self.open.insert(id.to_string());
        // Part of the work handed to a helper (the session's own call).
        if matches!(name, "Task" | "Agent") && self.under.is_none() {
            self.helpers.insert(id.to_string());
        }
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
        if let (Some(file), true) = (&seen.file, backed_up_tool(name)) {
            self.edited_files.insert(file.clone());
        }
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
        // A command's run ended (one in the background goes on to the turn's end).
        if let Some((began, false)) = self.command_started.get(id).copied() {
            self.command_started.remove(id);
            self.command_runs.push((began, SystemTime::now()));
        }
        self.open.remove(id);
        if let Some(change) = self.changes.remove(id).filter(|_| !is_error) {
            self.diff_of(change, id, result, text);
        }
        if self.commands.remove(id) {
            self.output_of(id, text, is_error);
        }
        // A helper is back: said, so its entry in the Agent panel stops working — with what it came back with.
        if self.helpers.remove(id) {
            if let Some(agent) = store::get(&self.id) {
                let report = Some(clipped(&helper_words(text, result), REPORT_CHARS)).filter(|r| !r.is_empty());
                emit(&AgentEvent { call: Some(id.to_string()), helper: Some(if is_error { "failed" } else { "back" }.to_string()), report, ..self.event(&agent) });
            }
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
    fn diff_of(&mut self, change: Change, call: &str, result: Option<&serde_json::Value>, text: &str) {
        let d = if change.tool == "Write" {
            let content = change.input.get("content").and_then(serde_json::Value::as_str).unwrap_or("");
            let kind = write_kind(change.known, result, text);
            // A file it made: the turn's `created` (its report card's new files).
            if kind == "new" && !self.written_new.contains(&change.file) {
                self.written_new.push(change.file.clone());
            }
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

    /// **More of the message it's writing** (`saying`: all of it so far).
    /// Whether the page is due another look ([`SAYING_EVERY`]; the first
    /// piece at once): what isn't told now is in the next one, or in the
    /// whole message.
    fn on_saying(&mut self, more: &str) -> bool {
        if more.is_empty() {
            return false;
        }
        self.saying.push_str(more);
        if self.saying_told.is_some_and(|t| t.elapsed() < SAYING_EVERY) {
            return false;
        }
        self.saying_told = Some(Instant::now());
        true
    }

    /// **A piece of Claude Code's stream**: a new message of its own starting
    /// lets go of what was being written; a piece of its text is more of it
    /// ([`Follow::on_saying`]: whether to tell the page). A helper's words
    /// aren't its own.
    fn on_stream(&mut self, s: &crate::ai::claude_code::events::Stream) -> bool {
        if s.parent_tool_use_id.as_deref().is_some_and(|p| !p.is_empty()) {
            return false;
        }
        match &s.kind {
            StreamKind::MessageStart { .. } => {
                self.saying.clear();
                self.saying_told = None;
                false
            }
            StreamKind::BlockStart { block: crate::ai::claude_code::events::BlockStart::ToolUse { name, .. }, .. } => {
                self.began(name);
                false
            }
            StreamKind::Text { text, .. } => self.on_saying(text),
            _ => false,
        }
    }

    /// One event of Claude Code's.
    fn on(&mut self, ev: Event, session: &Session) {
        match ev {
            Event::Init(init) => {
                if !init.session_id.is_empty() {
                    // (A branch — Rewind, Fork from here — is made: the session is its own now.)
                    store::touch(&self.id, |j| {
                        j.session = init.session_id.clone();
                        j.branch = None;
                    });
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
                // Its own message's place in the session: where a Rewind or a Fork from here resumes it.
                if self.under.is_none() {
                    if let Some(u) = a.uuid.as_ref().filter(|u| !u.is_empty()) {
                        self.last_uuid = Some(u.clone());
                    }
                }
                // Its own message, whole now: what was being written is this.
                if self.under.is_none() {
                    self.saying.clear();
                    self.saying_told = None;
                }
                for block in &a.blocks {
                    match block {
                        Block::ToolUse { id, name, input } => self.on_tool(id, name, input),
                        // Its thinking (a helper's isn't its own): a line the thread's Thinking view shows.
                        Block::Thinking(t) if a.parent_tool_use_id.is_none() && !t.trim().is_empty() => {
                            if let Some(agent) = store::get(&self.id) {
                                emit(&AgentEvent { thought: Some(thought_of(t)), ..self.event(&agent) });
                            }
                        }
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
                    // Its first message's place: Claude Code's checkpoint before the turn (Rewind's files).
                    if let Some(at) = u.uuid.as_deref().filter(|a| !a.is_empty()) {
                        self.checkpoint_at(&session.session_id(), at);
                    }
                    let text: String = u.blocks.iter().filter_map(|b| if let Block::Text(t) = b { Some(t.as_str()) } else { None }).collect();
                    self.on_taken(&text);
                }
                // (Claude Code's own record of a call is its message's: one result a message.)
                let results = u.blocks.iter().filter(|b| matches!(b, Block::ToolResult { .. })).count();
                // Its own calls back: fewer out (the queue: `queue_state`).
                if results > 0 && u.parent_tool_use_id.as_deref().is_none_or(str::is_empty) {
                    with_state(|s| {
                        if let Some(l) = s.live.get_mut(&self.id) {
                            l.tools_out = l.tools_out.saturating_sub(results as u32);
                        }
                    });
                }
                let record = u.tool_use_result.as_ref().filter(|_| results == 1);
                for block in &u.blocks {
                    if let Block::ToolResult { tool_use_id, is_error, text, pictures } = block {
                        self.on_tool_result(tool_use_id, *is_error, text, record);
                        self.on_pictures(tool_use_id, pictures);
                        self.web_back(tool_use_id, *is_error, text, record);
                    }
                }
            }
            // Its words as they're written.
            Event::Stream(s) => {
                if self.on_stream(&s) {
                    emit_saying(&self.id, self.n, &self.saying);
                }
            }
            // (A plan the person sent back with comments isn't a denial: it's being revised.)
            Event::PermissionDenied(d) if d.tool_name == claude_live::EXIT_PLAN && d.message.starts_with(claude_live::REVISE_LEAD) => {}
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

    /// **The turn's checkpoint** (`TurnNote::checkpoint`): the first message
    /// of its own Claude Code echoed back, in `session` — kept once, so a
    /// Continue's run keeps the turn's first. Claude Code's
    /// `--rewind-files <at>` takes the files back to before it.
    fn checkpoint_at(&self, session: &str, at: &str) {
        let n = self.n;
        let unset = store::get(&self.id).is_some_and(|j| j.turn_list.iter().any(|t| t.n == n && t.checkpoint.is_none()));
        if !unset || session.is_empty() {
            return;
        }
        store::touch(&self.id, |j| {
            if let Some(t) = j.turn_mut(n) {
                t.checkpoint = Some(store::Checkpoint { session: session.to_string(), at: at.to_string() });
            }
        });
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
        // Read after an answer had ended (a result in: no tool call was left, so it's a turn of Claude
        // Code's own in this process): what it says from here on is this message's answer, not the one
        // before's too (14 Nov 2026, the maintainer: a paragraph and the reply to "Hello" came back as one
        // answer, shown twice). A note read mid-work (no result yet) leaves the turn's words whole.
        if self.outcome.is_some() && !self.said.is_empty() {
            self.split_at = Some(self.said.len());
        }
        if let Some(agent) = store::update(&self.id, |j| {
            if let Some(t) = j.turn_mut(n) {
                t.request = format!("{}\n\n{}", t.request.trim(), note.text.trim());
            }
        }) {
            tracing::info!(id = %self.id, turn = n, late = self.outcome.is_some(), "Agents: it took a note in");
            emit(&AgentEvent { step: Some(NOTE_READ.into()), read: note.message_id.clone(), ..self.event(&agent) });
        }
    }

    /// A turn's result: its words, cost and denials kept; once every message
    /// written has been taken in, its input ends.
    fn on_result(&mut self, r: &TurnResult, session: &Session) {
        let stopping = with_state(|s| s.live.get(&self.id).is_some_and(|l| l.stopping.is_some()));
        let text = if r.text.trim().is_empty() { self.last_text.clone() } else { r.text.trim().to_string() };
        let outcome = outcome_of(r, stopping, self.problem.as_ref());
        // (A plan sent back with comments isn't a denial: `revised_plan`.)
        let before: Vec<store::PlanBefore> = store::get(&self.id).and_then(|j| j.turn_list.iter().find(|t| t.n == self.n).map(|t| t.plans_before.clone())).unwrap_or_default();
        let denials: Vec<Denial> = r
            .permission_denials
            .iter()
            .filter(|d| !revised_plan(&d.tool_name, &d.tool_input, &before))
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
        let n = self.n;
        let agent = store::update(&self.id, |j| {
            j.cost = cost;
            if session_total > 0.0 {
                j.session_total = session_total;
            }
            if let Some(t) = j.turn_mut(n) {
                t.cost = turn_cost;
                if usage.is_some() {
                    t.usage.clone_from(&usage);
                }
                // Where it ended in the session (Rewind, Fork from here).
                if self.last_uuid.is_some() {
                    t.end_uuid.clone_from(&self.last_uuid);
                }
                for d in &denials {
                    if !t.denials.iter().any(|x| x.tool == d.tool && x.detail == d.detail) {
                        t.denials.push(d.clone());
                    }
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
        // (A message read after an answer had ended: the turn's words are from it on — what came before
        // stays its work's, under the message before.)
        self.outcome = Some((outcome, turn_words(&self.said, self.split_at)));
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
        // Its plan waiting for the person's go-ahead (ExitPlanMode's card),
        // or a question of its own (AskUserQuestion's).
        let plan = waiting.iter().any(|p| p.run == self.id && p.tool == claude_live::EXIT_PLAN);
        let question = waiting.iter().any(|p| p.run == self.id && p.tool == claude_live::ASK);
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
                let step = asking.then(|| {
                    if plan {
                        "Waiting for you: its plan is ready"
                    } else if question {
                        "Waiting for you: it has a question"
                    } else {
                        "Waiting for you to allow a step"
                    }
                    .to_string()
                });
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
        let asks = status == Status::Done && !plan_allowed && !self.direct && super::consent::asks(&text);
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

/// **A turn's words**: what it said, from where a message read after an
/// answer had ended split it (`Follow::split_at`) — that message's answer,
/// never the one before's too — else all of it.
fn turn_words(said: &[String], split_at: Option<usize>) -> String {
    match split_at {
        Some(k) if k < said.len() => said[k..].join("\n\n"),
        _ => said.join("\n\n"),
    }
}

/// **A thought as its log keeps it**: trimmed, at most [`THOUGHT_CHARS`]
/// characters (cut at a character's edge, with "…"). Pure.
fn thought_of(text: &str) -> String {
    clipped(text, THOUGHT_CHARS)
}

/// `text` trimmed, at most `max` characters (cut at a character's edge, with
/// "…"). Pure.
fn clipped(text: &str, max: usize) -> String {
    let t = text.trim();
    match t.char_indices().nth(max) {
        Some((cut, _)) => format!("{}…", t[..cut].trim_end()),
        None => t.to_string(),
    }
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
            j.turn_list.push(store::TurnNote { n, title: title.clone(), request: text.clone(), message_id: ask.message_id.clone(), status: Status::Queued, read_only: ask.read_only, direct: ask.direct, shots: ask.shots.clone(), ..Default::default() });
        } else if let Some(t) = j.turn_list.last_mut() {
            t.request = format!("{}\n\n{}", t.request.trim(), text);
            add_shots(&mut t.shots, &ask.shots);
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

/// **What a message for conversation `conversation`'s own session does now**
/// (14 Nov 2026, the message box's queue): `"taking"` — a turn of the chat's
/// own (a Code chat that works directly) works with one of its tool calls
/// out, so Claude Code reads a message written now as that call ends, into
/// the work in hand; `"writing"` — it works with no call out (writing its
/// answer), so a message written now would be read only after that answer:
/// it waits in the queue instead, for its next call or the turn's end;
/// `"idle"` — nothing of the chat's own working: sent as ever.
pub fn queue_state(conversation: u64) -> &'static str {
    let ids: Vec<String> = store::list().into_iter().filter(|a| a.conversation == Some(conversation) && a.turn_list.last().is_some_and(|t| t.direct)).map(|a| a.id).collect();
    with_state(|s| {
        let working = ids.iter().filter_map(|id| s.live.get(id)).find(|l| l.started && !l.input_over && l.stopping.is_none());
        match working {
            Some(l) if l.tools_out > 0 => "taking",
            Some(_) => "writing",
            None => "idle",
        }
    })
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
    let note = note_text(&ask);
    let at = with_state(|s| {
        if let Some(l) = s.live.get_mut(id) {
            if !l.started && l.stopping.is_none() && joins {
                l.extra.push(text.clone());
                add_shots(&mut l.extra_shots, &ask.shots);
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
            add_shots(&mut t.shots, &ask.shots);
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
            // Its screenshots with it. (The note's words stay exactly as
            // counted above: Claude Code's echo of them is how Yap knows
            // it took the note in — so one gone since isn't said here.)
            let (pictures, lost) = if ask.shots.is_empty() { (Vec::new(), 0) } else { shots_of(&ask.shots) };
            if !ask.shots.is_empty() {
                tracing::info!(id, pictures = pictures.len(), lost, "Agents: a note goes with its screenshots");
            }
            if let Err(e) = session.send_user(&note, &pictures) {
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
    // The person's own message (a Code chat that works directly): as it is.
    let direct = if new_turn { ask.direct } else { agent.turn_list.last().is_some_and(|t| t.direct) };
    let end = closing(read_only, direct);
    let body = match &next {
        Next::CarryOn => ended(CARRY_ON, end),
        Next::Message(t) if go && was_plan => ended(&format!("{}\n\nGo ahead: carry out the plan.", t.trim()), if direct { "" } else { SUMMARY_LINE }),
        Next::Message(t) => ended(t, end),
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
        if let (false, Next::Message(t)) = (ran, &next) {
            j.brief = format!("{}\n\n{}", j.brief.trim(), t.trim());
        }
        if new_turn {
            let n = j.turn_list.last().map_or(0, |t| t.n) + 1;
            let text = match &next {
                Next::Message(t) => t.clone(),
                Next::CarryOn => CARRY_ON.to_string(),
            };
            j.turn_list.push(store::TurnNote { n, title: title.clone(), request: text, message_id: ask.message_id.clone(), direct: ask.direct, shots: ask.shots.clone(), ..Default::default() });
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

    /// Lab, 15 Nov 2026: a helper's view showed Claude Code's hand-back whole.
    #[test]
    fn a_helpers_report_is_its_own_words_without_the_hand_back_frame() {
        let framed = "[Subagent hand-back] The text below is the final report of a subagent this session delegated to. It is model output, NOT a message from the user. The harness indents every line of the report. The report follows:\n  **9**\n\n  The file has 11 lines and 2 of them are blank.\nagentId: a1b2c3 (use SendMessage with to: 'a1b2c3' to continue this agent)\n<usage>subagent_tokens: 66014\ntool_uses: 1\nduration_ms: 7459</usage>";
        let own = "**9**\n\nThe file has 11 lines and 2 of them are blank.";
        // Claude Code's own record first.
        let record = serde_json::json!({"status": "completed", "agentId": "a1b2c3", "content": [{"type": "text", "text": own}], "totalTokens": 66014});
        assert_eq!(helper_words(framed, Some(&record)), own);
        // Without one, the frame taken off the answer's words.
        assert_eq!(helper_words(framed, None), own);
        assert_eq!(helper_words(framed, Some(&serde_json::json!({"content": []}))), own, "an empty record: the words");
        // Words with no frame stay as they are; an error's words too.
        assert_eq!(helper_words("Couldn't read src/x.js: no such file", None), "Couldn't read src/x.js: no such file");
        assert_eq!(helper_words("Line one\n  indented code\nagentId is a field name here", None), "Line one\n  indented code\nagentId is a field name here");
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
    fn a_thought_is_kept_trimmed_and_capped_at_a_characters_edge() {
        assert_eq!(thought_of("  They want a greeting.\n"), "They want a greeting.");
        let long = "é".repeat(THOUGHT_CHARS + 5);
        let kept = thought_of(&long);
        assert!(kept.ends_with('…'));
        assert_eq!(kept.chars().count(), THOUGHT_CHARS + 1);
        // Its event says it alone, never as words said.
        let ev = AgentEvent { id: "ab12".into(), thought: Some("Check the notes first.".into()), ..Default::default() };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["thought"], "Check the notes first.");
        assert!(v.get("said").is_none() && v.get("step").is_none() && v.get("text").is_none());
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
        // The run key in its environment, which its files name; its to-do list on, as TodoWrite; Claude Code's own
        // file checkpoints on (Rewind's files).
        assert_eq!(spec.env[..1], key[..]);
        assert_eq!(
            &spec.env[1..],
            &[
                ("CLAUDE_CODE_ENABLE_TODO_TOOLS".to_string(), "1".to_string()),
                ("CLAUDE_CODE_ENABLE_TASKS".to_string(), "0".to_string()),
                ("CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING".to_string(), "true".to_string())
            ]
        );
        assert!(session::environment(Vec::new(), &Pay::Plan, &spec.env).is_ok(), "a run may set them");
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
    fn claude_codes_own_edit_tools_and_its_checkpoints_on_every_run() {
        for tool in ["Edit", "Write", "MultiEdit", "NotebookEdit"] {
            assert!(backed_up_tool(tool), "{tool}");
        }
        for tool in ["Bash", "Read", "WebFetch", "mcp__yap__approve", "edit", ""] {
            assert!(!backed_up_tool(tool), "{tool}");
        }
        // Its checkpoints: the variable Claude Code documents, which Yap may set for a run.
        assert_eq!(CHECKPOINT_ENV, ("CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING", "true"));
        assert!(cli::RUN_SETS_ENV.contains(&CHECKPOINT_ENV.0));
    }

    #[test]
    fn a_turn_opens_with_the_brief_and_the_summary_line() {
        let agent = Agent { brief: "  Tidy the readme.  ".into(), ..Default::default() };
        assert_eq!(opening(&agent, None, false, false), format!("Tidy the readme.\n\n{SUMMARY_LINE}"));
        assert_eq!(opening(&agent, Some("Also the changelog."), false, false), "Also the changelog.");
        // **The person's own message, straight to a Code chat's session: as they wrote it.** No summary
        // line (a plain "just testing" came back as "I didn't change anything this time…"), no note line.
        let hello = Agent { brief: "Hey, just testing the app's working.".into(), ..Default::default() };
        assert_eq!(opening(&hello, None, false, true), "Hey, just testing the app's working.");
        assert_eq!((closing(false, true), closing(false, false)), ("", SUMMARY_LINE));
        assert_eq!(closing(true, true), READ_ONLY_LINE, "a read-only turn still says so");
        assert_eq!(ended("  Carry on.  ", ""), "Carry on.");
        assert_eq!(note_text(&Ask { text: "Keep the headings.".into(), direct: true, ..Default::default() }), "Keep the headings.");
        assert_eq!(note_text(&Ask::of("Keep the headings.", false)), format!("Keep the headings.\n\n{NOTE_LINE}"));
        assert!(SUMMARY_LINE.starts_with("When you're done, finish with a short summary of what you changed and anything you couldn't do."));
        // A read-only turn: the brief says it mustn't change files, and to report what it found.
        let catch_up = Agent { brief: "Get caught up with the add-on in this folder.".into(), ..Default::default() };
        let said = opening(&catch_up, None, true, false);
        assert!(said.starts_with("Get caught up with the add-on in this folder.\n\nThis is read only: don't change, create or delete any files"), "{said}");
        assert!(said.ends_with("finish with a short summary of what you found."));
        assert!(!said.contains(SUMMARY_LINE));
        // Its first line: the chat thinking, for the person's own message; an agent sent out starts Claude Code.
        assert_eq!((start_line(true), start_line(false)), ("Thinking", "Starting Claude Code"));
        // Its browser beside the chat (a Code chat's own session): the pages with the web on, else picture
        // files; the screen with "Let Yap's AI see your screen" on; acting on its pages with the web on,
        // never a phone's turn.
        use crate::chat::pictures::Gate;
        let cfg = |web_search: bool, assistant_screen: bool| crate::config::YapConfig { web_search, assistant_screen, ..Default::default() };
        let on = direct_eyes(&cfg(true, true), "claude-haiku-5-5", false);
        assert_eq!((on.act, on.files), (true, Gate::Off), "acting on its pages, as Claude Code's own browser");
        assert_ne!(on.pictures, Gate::Off);
        assert_ne!(on.screen, Gate::Off);
        assert!(!direct_eyes(&cfg(true, true), "claude-haiku-5-5", true).act, "never a phone's turn");
        let off = direct_eyes(&cfg(false, false), "claude-haiku-5-5", false);
        assert!(!off.act, "no pages, nothing to act on");
        assert_eq!((off.pictures, off.screen), (Gate::Off, Gate::Off));
        assert_ne!(off.files, Gate::Off);
        // The pages its own searches found, numbered as found, each once, http(s) only.
        let mut found = Vec::new();
        add_found(&mut found, "Tide tables", "https://tides.example.com/bay");
        add_found(&mut found, "", "https://tides.example.com/bay#top");
        add_found(&mut found, "", "ftp://files.example.com/x");
        add_found(&mut found, "", "https://www.example.com/beds");
        assert_eq!(found.iter().map(|s| (s.n, s.title.as_str())).collect::<Vec<_>>(), [(1, "Tide tables"), (2, "example.com/beds")]);
        // The bar hears either count change: an agent sent out, or the chat's own Claude at work.
        assert_ne!(told((1, 0)), told((0, 1)));
        assert_eq!(told((0, 0)), 0);
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

    /// The queue (14 Nov 2026): a message read after an answer had ended is
    /// a turn of Claude Code's own in the same process, its words only what
    /// came after; never split, or split past the end, all of it.
    #[test]
    fn a_message_read_after_an_answer_has_only_the_words_that_came_after() {
        let said: Vec<String> = ["A paragraph about whales.", "Blue whales live about 80 to 90 years.", "Mostly krill."].map(String::from).to_vec();
        assert_eq!(turn_words(&said, Some(1)), "Blue whales live about 80 to 90 years.\n\nMostly krill.");
        assert_eq!(turn_words(&said, None), said.join("\n\n"));
        // Read after the last words, with nothing said since: the turn's words stay whole.
        assert_eq!(turn_words(&said, Some(3)), said.join("\n\n"));
        assert_eq!(turn_words(&[], Some(0)), "");
    }

    /// A Rewind or a Fork from here: Claude Code's own fork of the session at
    /// a message, or — to before its first message, or with no session — a
    /// new one.
    #[test]
    fn a_branch_resumes_claude_codes_own_fork_at_its_message_or_starts_afresh() {
        let at = |u: Option<&str>| store::Branch { at: u.map(str::to_string), ..Default::default() };
        assert_eq!(branched("s-1", &at(Some("u-7"))), Conversation::Branch { from: "s-1".into(), at: "u-7".into() });
        // A forked chat rewound to before its own first turn: the original session, at the fork's point.
        let back = store::Branch { at: Some("u-3".into()), from: Some("orig".into()), seed: None };
        assert_eq!(branched("s-1", &back), Conversation::Branch { from: "orig".into(), at: "u-3".into() });
        assert!(matches!(branched("s-1", &at(None)), Conversation::New(id) if id != "s-1"));
        assert!(matches!(branched("", &at(Some("u-7"))), Conversation::New(_)));
        assert!(matches!(branched("s-1", &at(Some(""))), Conversation::New(_)));
        // No branch Claude Code can make: a new session (its first message the conversation as text).
        let seeded = store::Branch { seed: Some("Me: hello".into()), ..at(Some("u-7")) };
        assert!(matches!(branched("s-1", &seeded), Conversation::New(id) if id != "s-1"));
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
        let mut follow = Follow::new(&agent, 1, folder.clone(), Mode::Auto, 200_000, None);
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
        let mut follow = Follow::new(&agent, 1, folder.clone(), Mode::Auto, 200_000, None);
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

    /// Its words as they're written: the message so far, told at once, then
    /// at most every [`SAYING_EVERY`]; a helper's are never its own; the
    /// whole message (or a new one starting) ends what was being written.
    #[test]
    fn its_words_are_told_as_they_are_written_never_into_its_log() {
        use crate::ai::claude_code::events::{Stream, StreamKind};
        let id = store::new_id();
        let folder = PathBuf::from(format!(r"D:\Projects\seed-tracker-{id}"));
        store::insert(Agent { id: id.clone(), folder: "seed-tracker".into(), path: folder.to_string_lossy().into_owned(), status: Status::Running, started_ts: 5, turn_list: vec![store::TurnNote { n: 1, status: Status::Running, started_ts: 5, ..Default::default() }], ..Default::default() });
        let agent = store::get(&id).unwrap();
        let mut follow = Follow::new(&agent, 1, folder, Mode::Auto, 200_000, None);
        // The first piece at once; the next within the pause is kept for the one after.
        assert!(follow.on_saying("I'll add "));
        assert!(!follow.on_saying("the watering "));
        assert!(!follow.on_saying(""));
        assert_eq!(follow.saying, "I'll add the watering ");
        follow.saying_told = Some(Instant::now() - SAYING_EVERY);
        assert!(follow.on_saying("schedule."));
        assert_eq!(follow.saying, "I'll add the watering schedule.");
        // Nothing of it is a line of its log.
        assert!(store::log_read(&id).is_empty());
        // A new message starting lets it go.
        assert!(!follow.on_stream(&Stream { parent_tool_use_id: None, kind: StreamKind::MessageStart { id: None, model: None, prompt: None } }));
        assert_eq!(follow.saying, "");
        // A helper's words aren't its own.
        assert!(!follow.on_stream(&Stream { parent_tool_use_id: Some("toolu_task".into()), kind: StreamKind::Text { index: 0, text: "Checking the docs.".into() } }));
        assert_eq!(follow.saying, "");
        assert!(follow.on_stream(&Stream { parent_tool_use_id: None, kind: StreamKind::Text { index: 0, text: "Done.".into() } }));
        assert_eq!(follow.saying, "Done.");
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
        let mut follow = Follow::new(&agent, 1, folder.clone(), Mode::Auto, 200_000, None);
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
        // The helper back: said with its call, once — a result of the helper's own call says nothing.
        follow.on_tool_result("toolu_h1", false, "Draft notes", None);
        follow.on_tool_result("toolu_task", false, "The docs are in order.", None);
        follow.on_tool_result("toolu_task", false, "again", None);
        let back: Vec<(String, String)> = store::log_read(&id).iter().filter_map(|e| Some((e.get("helper")?.as_str()?.to_string(), e.get("call")?.as_str()?.to_string()))).collect();
        assert_eq!(back, [("back".to_string(), "toolu_task".to_string())]);
        // …with what it came back with: its report, for its own view in the Agent panel.
        let reports = |log: &[serde_json::Value]| log.iter().filter_map(|e| e.get("report").and_then(|r| r.as_str()).map(str::to_string)).collect::<Vec<_>>();
        assert_eq!(reports(&store::log_read(&id)), ["The docs are in order."]);
        // One that failed says so.
        follow.on_tool("toolu_task2", "Agent", &json!({ "description": "Check the links" }));
        follow.on_tool_result("toolu_task2", true, "It couldn't.", None);
        assert_eq!(store::log_read(&id).iter().filter_map(|e| e.get("helper").and_then(|h| h.as_str()).map(str::to_string)).collect::<Vec<_>>(), ["back", "failed"]);
        // A long report is cut at a character's edge; one with no words has none.
        follow.on_tool("toolu_task3", "Task", &json!({ "description": "Write it all up" }));
        follow.on_tool_result("toolu_task3", false, &"é".repeat(REPORT_CHARS + 9), None);
        follow.on_tool("toolu_task4", "Task", &json!({ "description": "Say nothing" }));
        follow.on_tool_result("toolu_task4", false, "  ", None);
        let all = reports(&store::log_read(&id));
        assert_eq!(all.len(), 3);
        assert_eq!(all[2].chars().count(), REPORT_CHARS + 1);
        assert!(all[2].ends_with('…'));
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
        for absent in ["file", "command", "about", "said", "todos", "now", "diff", "call", "under", "helper", "report", "done", "range", "output", "picture", "text", "cost", "problem"] {
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
        use serde_json::json;
        // Claude Code's own record says.
        assert_eq!(write_kind(false, Some(&json!({ "type": "create" })), ""), "new");
        assert_eq!(write_kind(true, Some(&json!({ "type": "create" })), ""), "new");
        assert_eq!(write_kind(false, Some(&json!({ "type": "update" })), ""), "rewrite");
        // Else its words.
        assert_eq!(write_kind(false, None, "File created successfully at: notes.md"), "new");
        assert_eq!(write_kind(false, None, "The file notes.md has been updated."), "rewrite");
        // The agent wrote it before; or nothing says.
        assert_eq!(write_kind(true, None, ""), "rewrite");
        assert_eq!(write_kind(false, None, ""), "write");
    }

    // ---- turns ----

    use store::TurnNote;

    fn turn(n: u32, title: &str, files: &[&str]) -> TurnNote {
        TurnNote { n, title: title.into(), status: Status::Done, started_ts: n as u64, ended_ts: n as u64, files: files.iter().map(|f| f.to_string()).collect(), ..Default::default() }
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

    /// A request's screenshots (their ids) go wherever its words go: its
    /// next turn's record, the turn waiting already, the turn starting.
    #[test]
    fn a_requests_screenshots_go_wherever_its_words_go() {
        let shots = |ids: &[&str]| ids.iter().map(|s| s.to_string()).collect::<Vec<_>>();
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
        // A request while it works (at another mode, so it waits): its next turn has them.
        let (agent, how) = follow_up(&id, Ask { text: "Make the beds look like this.".into(), mode: Some(Mode::Plan), shots: shots(&["shot-a"]), ..Default::default() }).unwrap();
        assert_eq!(how, FollowedUp::Queued);
        assert_eq!(agent.turn_list[1].shots, shots(&["shot-a"]));
        assert!(agent.turn_list[0].shots.is_empty(), "never the turn working's");
        // Another for the same turn: added, each once.
        let (agent, _) = follow_up(&id, Ask { text: "And the legend like this.".into(), mode: Some(Mode::Plan), shots: shots(&["shot-a", "shot-b"]), ..Default::default() }).unwrap();
        assert_eq!(agent.turn_list[1].shots, shots(&["shot-a", "shot-b"]));
        assert_eq!(with_state(|s| s.live.get(&id).and_then(|l| l.pending.clone())).unwrap().shots, shots(&["shot-a", "shot-b"]));
        with_state(|s| s.live.remove(&id));

        // Words while its process starts: with its first message, and on its turn.
        let starting = store::new_id();
        store::insert(Agent { id: starting.clone(), status: Status::Queued, turn_list: vec![TurnNote { status: Status::Queued, request: "Tidy notes.md.".into(), shots: shots(&["shot-c"]), ..turn(1, "Tidy notes.md", &[]) }], ..Default::default() });
        with_state(|s| s.live.insert(starting.clone(), Live::new("x".into())));
        let (agent, how) = follow_up(&starting, Ask { text: "Like this one.".into(), shots: shots(&["shot-d"]), ..Default::default() }).unwrap();
        assert_eq!(how, FollowedUp::Added);
        assert_eq!(agent.turn_list[0].shots, shots(&["shot-c", "shot-d"]));
        assert_eq!(with_state(|s| s.live.remove(&starting).map(|l| l.extra_shots)), Some(shots(&["shot-d"])));

        // One that's gone since (kept past "Keep chat screenshots") is counted, and said.
        let (pictures, lost) = shots_of(&shots(&["not-a-screenshot-of-yaps"]));
        assert!(pictures.is_empty());
        assert_eq!(lost, 1);
        assert_eq!(lost_line(1), "(A screenshot that came with this is no longer on this PC.)");
        assert_eq!(lost_line(2), "(2 screenshots that came with this are no longer on this PC.)");
        // With a turn's first message — its brief, its request on --resume — never with a Continue of a session that ran.
        assert!(with_shots(false, None));
        let carry_on = format!("{CARRY_ON}\n\n{SUMMARY_LINE}");
        assert!(with_shots(true, Some("And the legend like this.")));
        assert!(!with_shots(true, Some(&carry_on)));
        assert!(with_shots(false, Some(&carry_on)), "a session that never ran has nothing yet");
        // A turn's record says them only when it has some (an older Yap reads the rest as ever).
        let said = serde_json::to_value(&agent.turn_list[0]).unwrap();
        assert_eq!(said["shots"], serde_json::json!(["shot-c", "shot-d"]));
        assert!(serde_json::to_value(turn(1, "x", &[])).unwrap().get("shots").is_none());
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

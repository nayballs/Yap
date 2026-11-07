//! **The runner**: each job is the person's own Claude Code in the job's
//! folder (`claude_code::session::Session`), followed as it works.
//!
//! - **At most 3 at once, one per folder** (`jobs_max`, Pro and Max plans'
//!   limits): the rest queue, and start in turn as one ends.
//! - **Their own setup**: no `--strict-mcp-config`, no `--setting-sources`,
//!   Claude Code's own tools — their settings, skills, MCP servers and
//!   CLAUDE.md, as in their terminal. Yap adds its run endpoint
//!   (`integrations/claude_live.rs`, `Tools::Job`): the permission prompt
//!   tool (`--permission-prompt-tool mcp__yap__approve`: a card for the
//!   person, never answered by voice) and the backup hook (`--settings`:
//!   each file copied into `jobs/<id>/backup/` before it's first changed).
//! - **The flags**: `--permission-mode` from the job's mode, `--model`,
//!   `--effort`, `--session-id` (then `--resume`). Paid with the person's
//!   Claude plan, or their Anthropic key (`jobs_pay`), never ambiguously
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
//!   as the app event **`yap-job`** (`{id, status, folder, conversation?,
//!   step?, file?, command?, about?, said?, todos?, now?, diff?, text?,
//!   cost?, problem?, ts}`) and kept in the job's `log.jsonl` for its tab.
//! - **Asking**: a permission card waiting for the person (the endpoint's
//!   `pending()`): the job is `asking` until it's answered.
//! - **Its turns** (2026-11-07: one job per Code chat, as Claude Code is one
//!   session per conversation): each request to it is a turn of its own
//!   (`store::TurnNote`: the brief, then each follow-up — `start_job` in its
//!   Code chat, `message_job`, the tab's "Tell the job…"), on the same
//!   session (`--resume`), at the chat's Mode then (`modes::next_turn`: a
//!   plan stays a plan until "go ahead"), Settings' model and effort then
//!   (unless the person asked for their own), each with its own backups
//!   (`jobs/<id>/backup/<n>/`) and its own report. Continue carries on the
//!   same turn. A request while a turn works **queues as the next turn**
//!   ([`follow_up`]): sent once this one ends, never into it.
//! - **A turn ends**: the process is told its input is over and exits.
//! - **Done or failed**: each turn reported in its chat
//!   (`chat/turn/report.rs`) — never one the person stopped.
//! - **Jobs die with Yap** (the kill-on-close job): quitting marks them
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
use crate::integrations::claude_live::{self, Approval, RunSpec, Tools};

use super::diff;
use super::folders;
use super::modes::{self, Mode};
use super::steps;
use super::store::{self, Denial, Job, ProblemNote, Status};

/// The app event every change of a job goes out as.
pub const EVENT: &str = "yap-job";
/// The most jobs at once.
pub const MAX: usize = 3;
/// What every brief ends with: the report is ready to say.
pub const SUMMARY_LINE: &str = "When you're done, finish with a short summary of what you changed and anything you couldn't do.";
/// What Continue says to a job that was stopped or interrupted.
pub const CARRY_ON: &str = "Carry on with the task from where you stopped.";
/// How long a stopped job gets to end its turn before it's ended.
const STOP_GRACE: Duration = Duration::from_secs(8);
/// How long a job whose input is over gets to exit by itself.
const EXIT_GRACE: Duration = Duration::from_secs(15);

/// **One change of a job**, as the page hears it (`yap-job`) and its log
/// keeps it.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobEvent {
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
    /// Unix seconds.
    pub ts: u64,
}

impl JobEvent {
    /// About the job's turn now (the newest that ran, else its newest).
    fn of(job: &Job) -> JobEvent {
        let turn = job.turn_list.iter().rev().find(|t| t.ran()).or(job.turn_list.last()).map(|t| t.n);
        JobEvent { turn, ..JobEvent::at(job, 0) }
    }

    /// About its turn `n` (0: none).
    fn at(job: &Job, n: u32) -> JobEvent {
        JobEvent { id: job.id.clone(), status: job.status, folder: job.folder.clone(), conversation: job.conversation, turn: (n > 0).then_some(n), ts: store::now_secs(), ..Default::default() }
    }
}

/// **A request for a job's next turn** (a follow-up, a plan's go-ahead):
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
    /// The person's own model and effort for it (else Settings' then).
    pub model: Option<String>,
    pub effort: Option<String>,
}

impl Ask {
    /// `text` told the job, nothing else asked.
    pub fn of(text: &str, go: bool) -> Ask {
        Ask { text: text.trim().to_string(), go, ..Default::default() }
    }

    fn title(&self) -> String {
        if self.title.trim().is_empty() { store::title_of(&self.text) } else { store::title_of(&self.title) }
    }

    /// Another request for the same turn, added: its words after these, the
    /// lower mode, a go-ahead in either.
    fn merge(&mut self, more: Ask) {
        self.text = format!("{}\n\n{}", self.text.trim(), more.text.trim());
        self.go |= more.go;
        self.mode = match (self.mode, more.mode) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        if more.model.is_some() {
            self.model = more.model;
        }
        if more.effort.is_some() {
            self.effort = more.effort;
        }
    }
}

/// Where a follow-up went ([`follow_up`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowedUp {
    /// The job was working: its next turn, once this one ends.
    Queued,
    /// Its next turn is on its way now.
    Started,
    /// The turn about to start (or waiting for a slot) takes it too.
    Added,
}

/// A job that has a slot.
struct Live {
    /// Set once its Claude Code started.
    session: Option<Arc<Session>>,
    /// User messages sent, and results back: its input ends when they meet.
    sent: u32,
    results: u32,
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
    /// Its folder, compared ([`folders::norm`]).
    folder: String,
}

impl Live {
    fn new(folder: String) -> Live {
        Live { session: None, sent: 0, results: 0, input_over: false, started: false, extra: Vec::new(), pending: None, stopping: None, folder }
    }
}

#[derive(Default)]
struct State {
    live: HashMap<String, Live>,
    /// Jobs waiting for a slot: id, the message their turn starts with
    /// (`None`: the brief), their folder (compared).
    queue: VecDeque<(String, Option<String>, String)>,
}

/// Is there a slot for a job in `folder`, beside the jobs running in `live`
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

/// How many jobs weren't over when the Yap bar was last told.
static LIVE_TOLD: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(usize::MAX);

/// Send `ev` to the page and keep it in the job's log; the Yap bar hears
/// when the number of jobs not over yet changes ("1 job running").
pub fn emit(ev: &JobEvent) {
    if let Ok(v) = serde_json::to_value(ev) {
        store::log_append(&ev.id, &v);
    }
    if let Some(app) = APP.get() {
        let _ = app.emit(EVENT, ev);
        let live = store::live_ids().len();
        if LIVE_TOLD.swap(live, std::sync::atomic::Ordering::Relaxed) != live {
            crate::bar::refresh(app);
        }
    }
}

/// The most jobs at once now (1 to [`MAX`]).
fn max_now() -> usize {
    (crate::config::load().jobs_max as usize).clamp(1, MAX)
}

/// **Start job `id`** (saved already): now, or queued behind the others.
pub fn start(id: &str) {
    schedule(id, None);
}

enum Placed {
    Go,
    Queued,
    Already,
}

/// Start `id`'s next turn with `message` (`None`: its brief), or queue it.
fn schedule(id: &str, message: Option<String>) {
    let Some(job) = store::get(id) else { return };
    let max = max_now();
    let folder = folders::norm(&job.path);
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
            if let Some(job) = store::update(id, |j| j.status = Status::Queued) {
                tracing::info!(id = %job.id, "Jobs: queued behind the others");
                // (Its newest turn is the one waiting.)
                let n = job.turn_list.last().map_or(0, |t| t.n);
                emit(&JobEvent { step: Some("Waiting for another job to finish".into()), ..JobEvent::at(&job, n) });
            }
        }
        Placed::Go => {
            let id = id.to_string();
            tauri::async_runtime::spawn(async move { run(id, message).await });
        }
    }
}

/// The jobs queued that fit now, started.
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

/// One turn of job `id`, from start to the process gone; then its slot goes
/// to the next, and it's reported — and a request that came while it
/// worked runs as its next turn.
async fn run(id: String, message: Option<String>) {
    let (status, n) = turn(&id, message).await;
    // A request that came while it worked: its next turn.
    let pending = with_state(|s| s.live.remove(&id).and_then(|l| l.pending));
    store::flush();
    let ended = matches!(status, Status::Done | Status::Failed);
    // Every turn is reported, a turn waiting behind it or not.
    if ended && n > 0 {
        crate::chat::turn::queue_job_report(&id, n);
    }
    match pending {
        Some(ask) if ended => {
            tracing::info!(id = %id, "Jobs: its next turn, asked for while this one worked");
            let text = ask.text.clone();
            resume(&id, Next::Message(text), &ask, false);
        }
        // Stopped: the request waiting isn't sent (Continue sends it).
        Some(_) => {
            if let Some(job) = store::update(&id, |j| {
                if let Some(t) = j.turn_list.last_mut().filter(|t| !t.ran()) {
                    t.status = status;
                }
            }) {
                tracing::info!(id = %id, "Jobs: a request waiting for its next turn not sent (stopped)");
                emit(&JobEvent { step: Some("Not sent yet: Continue sends it".into()), ..JobEvent::at(&job, job.turn_list.last().map_or(0, |t| t.n)) });
            }
        }
        None => {}
    }
    pump();
}

/// What a turn sends first: the brief (with [`SUMMARY_LINE`]) or `message`.
fn opening(job: &Job, message: Option<&str>) -> String {
    match message {
        Some(m) => m.to_string(),
        None => format!("{}\n\n{SUMMARY_LINE}", job.brief.trim()),
    }
}

/// The job's turn `n` failed before Claude Code did anything: said, and
/// saved.
fn failed_early(id: &str, n: u32, why: String, problem: Option<ProblemNote>) -> (Status, u32) {
    tracing::info!(id, "Jobs: couldn't start");
    let now = store::now_secs();
    if let Some(job) = store::update(id, |j| {
        j.status = Status::Failed;
        j.ended_ts = now;
        j.error = Some(why.clone());
        j.problem = problem.clone();
        j.plan = false;
        j.asks = false;
        j.reported = false;
        j.step.clear();
        if let Some(t) = j.turn_mut(n) {
            t.status = Status::Failed;
            t.ended_ts = now;
            t.error = Some(why.clone());
            t.plan = false;
            t.asks = false;
        }
    }) {
        emit(&JobEvent { text: Some(why), problem, ..JobEvent::at(&job, n) });
    }
    (Status::Failed, n)
}

/// How the job pays, or why it can't.
fn pay_for(job: &Job, cfg: &crate::config::YapConfig, status: &cli::Status) -> Result<Pay, (String, Option<ProblemNote>)> {
    if job.pay == "key" {
        let key = cfg.provider_api_key("anthropic", "");
        if key.trim().is_empty() {
            return Err(("There's no Anthropic API key to pay for this job: add one in Settings → Language Models, or let jobs use your Claude plan.".into(), None));
        }
        return Ok(Pay::ApiKey(ApiKey::new(key)));
    }
    if !status.logged_in {
        let p = ProblemNote::of(&Problem::NotSignedIn);
        return Err((p.message.clone(), Some(p)));
    }
    Ok(Pay::Plan)
}

/// **A job's Claude Code run**: in its folder, the person's own setup (no
/// `--strict-mcp-config`, no `--setting-sources`, Claude Code's own tools)
/// with Yap's endpoint added (the run's `mcp_config`, the backup hook's
/// `settings`, and `env`: the run key both of them name), its permission
/// prompts to Yap's `approve`, at `mode`, its model and effort, on
/// `conversation`. Its session is kept (`--resume`, Open in terminal). Pure.
#[allow(clippy::too_many_arguments)]
pub fn job_spec(
    job: &Job,
    folder: &std::path::Path,
    pay: Pay,
    mode: Mode,
    conversation: Conversation,
    mcp_config: serde_json::Value,
    settings: serde_json::Value,
    env: Vec<(String, String)>,
) -> Spec {
    let mut spec = Spec::new(folder, pay);
    spec.env = env;
    spec.model = Some(job.model.clone()).filter(|m| !m.trim().is_empty());
    spec.effort = Effort::parse(&job.effort);
    spec.permission_mode = mode.permission();
    spec.permission_prompt_tool = Some(claude_live::PERMISSION_TOOL.to_string());
    spec.mcp_config = Some(mcp_config);
    spec.strict_mcp_config = false;
    spec.setting_sources = None;
    spec.tools = None;
    spec.settings = Some(settings);
    spec.conversation = conversation;
    spec
}

/// **One turn**: Claude Code started on the job, `message` (or the brief)
/// sent, followed until it's gone. The turn it ran (`store::TurnNote::n`):
/// the job's newest (a new request's, or the one Continue carries on).
async fn turn(id: &str, message: Option<String>) -> (Status, u32) {
    // Its turns kept from now on (a job from before them: one, its brief).
    let Some(job) = store::update(id, Job::keep_turns) else { return (Status::Failed, 0) };
    let n = job.turn_list.last().map_or(1, |t| t.n);
    let cfg = crate::config::load();
    let Some(program) = cli::find() else {
        return failed_early(id, n, "Claude Code isn't installed on this PC.".into(), None);
    };
    let status = tauri::async_runtime::spawn_blocking(cli::status).await.unwrap_or_default();
    if status.too_old {
        let v = status.version.unwrap_or_default();
        return failed_early(id, n, format!("Claude Code {v} is too old for jobs: update it to {} or later.", cli::MIN_VERSION), None);
    }
    let pay = match pay_for(&job, &cfg, &status) {
        Ok(p) => p,
        Err((why, problem)) => return failed_early(id, n, why, problem),
    };
    let folder = PathBuf::from(&job.path);
    if !folder.is_dir() {
        return failed_early(id, n, "The job's folder isn't there any more.".into(), None);
    }
    // This turn's own backups (first copy wins within it).
    let Some(backup) = store::turn_backup_dir(id, n) else { return failed_early(id, n, "That isn't one of Yap's jobs.".into(), None) };
    if let Err(e) = std::fs::create_dir_all(&backup) {
        return failed_early(id, n, format!("Couldn't make the job's backup folder: {e}"), None);
    }
    let run = match claude_live::register_run(RunSpec {
        label: job.folder.clone(),
        tools: Tools::Job,
        backup_dir: Some(backup.clone()),
        folder: Some(folder.clone()),
        approval: Some(Approval { run: id.to_string(), conversation: job.conversation, origin: job.origin }),
        jobs: false,
        memory: false,
    }) {
        Ok(r) => r,
        Err(e) => return failed_early(id, n, format!("Couldn't start the job: {e}"), None),
    };
    let mode = modes::or_auto(&job.mode);
    let resuming = message.is_some() && job.started_ts > 0 && !job.session.is_empty();
    let session_id = if job.session.is_empty() { session::new_session_id() } else { job.session.clone() };
    let conversation = if resuming { Conversation::Resume(session_id) } else { Conversation::New(session_id) };
    let spec = job_spec(&job, &folder, pay, mode, conversation, run.mcp_config(), run.settings(), run.env());
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
    let first = std::iter::once(opening(&job, message.as_deref())).chain(extra.iter().map(|t| format!("Also: {t}"))).collect::<Vec<_>>().join("\n\n");
    if stopped_early {
        session.kill();
    } else if let Err(e) = session.send_user(&first, &[]) {
        session.kill();
        return failed_early(id, n, e, None);
    }
    let now = store::now_secs();
    let Some(job) = store::update(id, |j| {
        j.status = Status::Running;
        j.session = session.session_id();
        if j.started_ts == 0 {
            j.started_ts = now;
        }
        j.turns += 1;
        j.mode = mode.as_str().to_string();
        j.plan = false;
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
            t.asks = false;
            t.error = None;
            t.undone = false;
        }
    }) else {
        session.kill();
        return (Status::Failed, n);
    };
    tracing::info!(id, mode = mode.as_str(), pay = %job.pay, resuming, turn = n, runs = job.turns, "Jobs: started");
    emit(&JobEvent { step: Some("Starting Claude Code".into()), ..JobEvent::at(&job, n) });

    let mut follow = Follow::new(&job, n, folder, backup, mode);
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

/// **Following one turn** of a job.
struct Follow {
    id: String,
    /// The turn (`store::TurnNote::n`): its events, its note.
    n: u32,
    folder: PathBuf,
    /// This turn's backups.
    backup: PathBuf,
    mode: Mode,
    /// What the job had cost before this process, and the turn.
    cost_before: f64,
    turn_cost_before: f64,
    /// Its latest words.
    last_text: String,
    /// The latest result's outcome and words.
    outcome: Option<(Status, String)>,
    problem: Option<ProblemNote>,
    asking: bool,
    /// When its input ended (its last result came).
    input_over_at: Option<Instant>,
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
    /// Diff lines the job's log has (across its turns; `diff::JOB_MAX`).
    diff_used: usize,
}

/// An Edit, MultiEdit or Write call waiting for its result: its diff goes
/// out once it went through.
struct Change {
    tool: String,
    input: serde_json::Value,
    /// The file, as the tab says it.
    file: String,
    /// The job had touched the file before this call.
    known: bool,
}

/// A Write's kind: "new" when it made the file (the backup manifest says
/// it didn't exist before the job, and the job hadn't written it yet this
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
/// it's called for, copied or not), yet the job's backup list is empty —
/// so its hooks never ran (a managed policy turned them off, `--bare`) or
/// never reached Yap. Pure.
fn no_backups(edited: bool, backed_up: usize) -> bool {
    edited && backed_up == 0
}

impl Follow {
    fn new(job: &Job, n: u32, folder: PathBuf, backup: PathBuf, mode: Mode) -> Follow {
        Follow {
            id: job.id.clone(),
            n,
            folder,
            backup,
            mode,
            cost_before: job.cost,
            turn_cost_before: job.turn_list.iter().find(|t| t.n == n).map_or(0.0, |t| t.cost),
            last_text: String::new(),
            outcome: None,
            problem: None,
            asking: false,
            input_over_at: None,
            last_flush: Instant::now(),
            edits: HashSet::new(),
            edited: false,
            open: HashSet::new(),
            todos: Vec::new(),
            line: String::new(),
            changes: HashMap::new(),
            diff_used: job.diff_lines as usize,
        }
    }

    fn event(&self, job: &Job) -> JobEvent {
        JobEvent::at(job, self.n)
    }

    /// The line it's on now is `line` (no new step): kept and said, when it
    /// changed.
    fn now(&mut self, line: String) {
        if line.is_empty() || line == self.line {
            return;
        }
        self.line = line.clone();
        if let Some(job) = store::touch(&self.id, |j| j.step = line.clone()) {
            emit(&JobEvent { now: Some(line), ..self.event(&job) });
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
        if let Some(todos) = seen.todos {
            self.todos = todos.clone();
            let line = steps::doing_now(&todos).unwrap_or_else(|| self.line.clone());
            self.line = line.clone();
            if let Some(job) = store::touch(&self.id, |j| j.step = line.clone()) {
                emit(&JobEvent { todos: Some(todos), now: Some(line).filter(|l| !l.is_empty()), ..self.event(&job) });
            }
            return;
        }
        let mut known = false;
        let n = self.n;
        let Some(job) = store::touch(&self.id, |j| {
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
        emit(&JobEvent { step: Some(seen.step), file: seen.file, command: seen.command, about: seen.about, ..self.event(&job) });
    }

    /// A tool call came back: an edit's diff once it went through; with
    /// nothing else out, the line is the to-do in progress.
    fn on_tool_result(&mut self, id: &str, is_error: bool) {
        if !is_error && self.edits.contains(id) {
            self.edited = true;
        }
        self.open.remove(id);
        if let Some(change) = self.changes.remove(id).filter(|_| !is_error) {
            self.diff_of(change);
        }
        if self.open.is_empty() {
            if let Some(line) = steps::doing_now(&self.todos) {
                self.now(line);
            }
        }
    }

    /// What `change` did to its file, within the caps, said and logged.
    fn diff_of(&mut self, change: Change) {
        let d = if change.tool == "Write" {
            let content = change.input.get("content").and_then(serde_json::Value::as_str).unwrap_or("");
            let state = claude_live::backup::read_manifest(&self.backup)
                .ok()
                .and_then(|m| m.files.into_iter().find(|e| steps::shown_path(&e.original.to_string_lossy(), &self.folder) == change.file))
                .map(|e| e.state);
            Some(diff::of_write(&change.file, content, write_kind(change.known, state)))
        } else {
            diff::of_edit(&change.tool, &change.input, &change.file)
        };
        let Some(d) = d else { return };
        let (d, kept) = diff::capped(d, diff::JOB_MAX.saturating_sub(self.diff_used));
        self.diff_used += kept;
        let used = self.diff_used as u32;
        if let Some(job) = store::touch(&self.id, |j| j.diff_lines = used) {
            emit(&JobEvent { diff: Some(d), ..self.event(&job) });
        }
    }

    /// One event of Claude Code's.
    fn on(&mut self, ev: Event, session: &Session) {
        match ev {
            Event::Init(init) => {
                if !init.session_id.is_empty() {
                    store::touch(&self.id, |j| j.session = init.session_id.clone());
                }
            }
            Event::Assistant(a) => {
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
                            if let Some(job) = store::get(&self.id) {
                                emit(&JobEvent { said: Some(self.last_text.clone()), ..self.event(&job) });
                            }
                        }
                        _ => {}
                    }
                }
            }
            // A call back: an edit went through, after the hooks.
            Event::User(u) => {
                for block in &u.blocks {
                    if let Block::ToolResult { tool_use_id, is_error, .. } = block {
                        self.on_tool_result(tool_use_id, *is_error);
                    }
                }
            }
            Event::PermissionDenied(d) => {
                let detail = if d.message.trim().is_empty() { format!("Use {}", d.tool_name) } else { steps::one_line(&d.message, 160) };
                let line = format!("Not allowed: {}", d.tool_name);
                let n = self.n;
                let denial = Denial { tool: d.tool_name.clone(), detail };
                if let Some(job) = store::touch(&self.id, |j| {
                    if let Some(t) = j.turn_mut(n) {
                        t.denials.push(denial.clone());
                    }
                    j.denials.push(denial);
                }) {
                    emit(&JobEvent { step: Some(line), ..self.event(&job) });
                }
            }
            Event::Problem(p) => {
                let note = ProblemNote::of(&p);
                self.problem = Some(note.clone());
                tracing::info!(id = %self.id, kind = %note.kind, "Jobs: a problem with Claude Code");
                if let Some(job) = store::touch(&self.id, |j| j.problem = Some(note.clone())) {
                    emit(&JobEvent { problem: Some(note), ..self.event(&job) });
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

    /// A turn's result: its words, cost and denials kept; once every message
    /// sent has had one, its input ends.
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
        let cost = self.cost_before + r.cost_usd.max(0.0);
        let turn_cost = self.turn_cost_before + r.cost_usd.max(0.0);
        let unbacked = no_backups(self.edited, manifest.len());
        if unbacked {
            tracing::info!(id = %self.id, "Jobs: files changed with no backups (Claude Code's hooks didn't run)");
        }
        let n = self.n;
        let job = store::update(&self.id, |j| {
            j.cost = cost;
            if unbacked {
                j.no_backups = true;
            }
            if let Some(t) = j.turn_mut(n) {
                t.cost = turn_cost;
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
        self.outcome = Some((outcome, text.clone()));
        let done = with_state(|s| match s.live.get_mut(&self.id) {
            Some(l) => {
                l.results += 1;
                let done = l.results >= l.sent;
                if done {
                    l.input_over = true;
                }
                done
            }
            None => true,
        });
        tracing::info!(id = %self.id, outcome = outcome.word(), last = done, "Jobs: a turn's result");
        if let Some(job) = job {
            emit(&JobEvent { text: Some(text), cost: Some(cost), ..self.event(&job) });
        }
        if done {
            session.end();
            self.input_over_at = Some(Instant::now());
        }
    }

    /// Every half second: a permission card waiting, a stop taking too
    /// long, a job that won't exit, and what changed saved.
    fn on_tick(&mut self, session: &Session) {
        let asking = claude_live::pending().iter().any(|p| p.run == self.id);
        if asking != self.asking {
            self.asking = asking;
            let status = if asking { Status::Asking } else { Status::Running };
            let n = self.n;
            if let Some(job) = store::update(&self.id, |j| {
                if j.status.live() {
                    j.status = status;
                }
                if let Some(t) = j.turn_mut(n).filter(|t| t.status.live()) {
                    t.status = status;
                }
            }) {
                // Asking: a step of its own; answered, the line it was on.
                let step = asking.then(|| "Waiting for you to allow a step".to_string());
                let now = (!asking).then(|| self.line.clone()).filter(|l| !l.is_empty());
                emit(&JobEvent { step, now, ..self.event(&job) });
            }
        }
        let stopping = with_state(|s| s.live.get(&self.id).and_then(|l| l.stopping));
        if stopping.is_some_and(|at| at.elapsed() > STOP_GRACE) || self.input_over_at.is_some_and(|at| at.elapsed() > EXIT_GRACE) {
            session.kill();
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
                let why = self.problem.as_ref().map(|p| p.message.clone()).unwrap_or_else(|| if text.is_empty() { "Claude Code couldn't finish the job.".into() } else { text.clone() });
                (Status::Failed, text, Some(why))
            }
            Some((status, text)) => (status, text, None),
            None if stopping => (Status::Stopped, self.last_text.clone(), None),
            None => {
                let why = self.problem.as_ref().map(|p| p.message.clone()).unwrap_or_else(|| "Claude Code stopped before it finished.".into());
                (Status::Failed, self.last_text.clone(), Some(why))
            }
        };
        let plan = self.mode == Mode::Plan && status == Status::Done;
        let asks = status == Status::Done && super::consent::asks(&text);
        let n = self.n;
        let now = store::now_secs();
        let job = store::update(&self.id, |j| {
            j.status = status;
            j.ended_ts = now;
            if !text.is_empty() {
                j.text = text.clone();
            }
            j.error = error.clone();
            j.plan = plan;
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
                t.asks = asks;
            }
        });
        tracing::info!(id = %self.id, turn = n, status = status.word(), plan, asks, "Jobs: ended");
        if let Some(job) = job {
            // Its turn's own words (never an earlier turn's).
            let said = job.turn_list.iter().find(|t| t.n == n).map(|t| t.text.clone()).unwrap_or_else(|| job.text.clone());
            emit(&JobEvent { text: Some(said).filter(|t| !t.is_empty()).or(error), cost: Some(job.cost), problem: job.problem.clone(), ..JobEvent::at(&job, n) });
        }
        // Its prompts ended with its run: their card on the bar goes.
        super::approve_card::sync();
        status
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

// ---- what the person (or the chat) asks of a job ----

/// **Stop job `id`**: a queued one comes off the queue, a running one is
/// interrupted (ended if it doesn't stop within a few seconds).
pub fn stop(id: &str) -> Result<Job, String> {
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
    let job = store::get(id).ok_or("There's no such job.")?;
    match was {
        Was::Queued | Was::Not if job.status.live() => {
            let now = store::now_secs();
            let job = store::update(id, |j| {
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
            .ok_or("There's no such job.")?;
            tracing::info!(id, "Jobs: stopped before it ran");
            emit(&JobEvent::at(&job, job.turn_list.last().map_or(0, |t| t.n)));
            Ok(job)
        }
        Was::Live(Some(session)) => {
            match session.interrupt() {
                Ok(_) => tracing::info!(id, "Jobs: stopping"),
                Err(_) => session.kill(),
            }
            emit(&JobEvent { step: Some("Stopping".into()), ..JobEvent::of(&job) });
            Ok(job)
        }
        Was::Live(None) => Ok(job),
        _ => Err("The job isn't running.".into()),
    }
}

/// **A message for job `id`** (the tab's "Tell the job…"; the chat's
/// `message_job` through [`follow_up`]): its next turn. `go`: a plan's
/// go-ahead (the next turn at the chat's Mode, carrying it out).
pub fn message(id: &str, text: &str, go: bool) -> Result<Job, String> {
    follow_up(id, Ask::of(text, go)).map(|(job, _)| job)
}

/// **A follow-up for job `id`** (its Code chat's next request, `message_job`,
/// "Tell the job…"): a turn of its own on the same session —
///
/// - while a turn works, **queued as its next turn** (sent once this one
///   ends: never two jobs, nor two turns, in the folder at once), its tab
///   showing it waiting ([`FollowedUp::Queued`]);
/// - a turn about to start, or waiting for a slot, takes it too
///   ([`FollowedUp::Added`]);
/// - else its next turn now, on `--resume` ([`FollowedUp::Started`]).
pub fn follow_up(id: &str, ask: Ask) -> Result<(Job, FollowedUp), String> {
    let ask = Ask { text: ask.text.trim().to_string(), ..ask };
    if ask.text.is_empty() {
        return Err("Say what to tell the job.".into());
    }
    store::get(id).ok_or("There's no such job.")?;
    enum At {
        /// Its process is starting: with its first message.
        Starting,
        /// Waiting for a slot (its first turn: `true`).
        InQueue(bool),
        /// Working: its next turn (the first request waiting: `true`).
        Working(bool),
        Not,
    }
    let text = ask.text.clone();
    let at = with_state(|s| {
        if let Some(l) = s.live.get_mut(id) {
            if !l.started && l.stopping.is_none() {
                l.extra.push(text.clone());
                return At::Starting;
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
    let added = |j: &mut Job| {
        if let Some(t) = j.turn_list.last_mut() {
            t.request = format!("{}\n\n{}", t.request.trim(), text);
        }
    };
    match at {
        At::Starting => {
            let job = store::update(id, added).ok_or("There's no such job.")?;
            tracing::info!(id, "Jobs: a follow-up for the turn starting");
            Ok((job, FollowedUp::Added))
        }
        At::InQueue(first) => {
            let job = store::update(id, |j| {
                if first {
                    j.brief = format!("{}\n\nAlso: {text}", j.brief.trim());
                }
                added(j);
            })
            .ok_or("There's no such job.")?;
            tracing::info!(id, "Jobs: a follow-up for the turn waiting for a slot");
            Ok((job, FollowedUp::Added))
        }
        At::Working(first) => {
            let title = ask.title();
            let job = store::update(id, |j| {
                j.keep_turns();
                if first {
                    let n = j.turn_list.last().map_or(0, |t| t.n) + 1;
                    j.turn_list.push(store::TurnNote { n, title: title.clone(), request: text.clone(), message_id: ask.message_id.clone(), status: Status::Queued, ..Default::default() });
                } else {
                    added(j);
                }
            })
            .ok_or("There's no such job.")?;
            let n = job.turn_list.last().map_or(0, |t| t.n);
            tracing::info!(id, turn = n, "Jobs: a follow-up queued behind the turn working");
            emit(&JobEvent { queued: Some(title), ..JobEvent::at(&job, n) });
            Ok((job, FollowedUp::Queued))
        }
        At::Not => {
            let text = ask.text.clone();
            resume(id, Next::Message(text), &ask, true);
            Ok((store::get(id).ok_or("There's no such job.")?, FollowedUp::Started))
        }
    }
}

/// What a job's next turn starts with.
enum Next {
    /// Continue: "Carry on…".
    CarryOn,
    /// The person's message (a plan's go-ahead too).
    Message(String),
}

/// **Continue job `id`** (stopped, interrupted or failed): its turn carried
/// on, on `--resume` (one that never ran starts from its brief); a request
/// that was waiting behind it when it stopped is sent instead.
pub fn continue_job(id: &str) -> Result<Job, String> {
    let job = store::get(id).ok_or("There's no such job.")?;
    if job.status.live() {
        return Err("The job is still running.".into());
    }
    if job.status == Status::Done {
        return Err("The job is done: send it a message to carry on.".into());
    }
    match job.turn_list.last().filter(|t| !t.ran() && t.n > 1) {
        Some(waiting) => {
            let text = waiting.request.clone();
            resume(id, Next::Message(text.clone()), &Ask::of(&text, false), false);
        }
        None => resume(id, Next::CarryOn, &Ask::default(), false),
    }
    store::get(id).ok_or_else(|| "There's no such job.".into())
}

/// The model and effort a job's next turn runs at: `ask`'s (the person's
/// own), else the job's when the person asked for it before, else Settings'
/// as they are now. Pure.
pub fn model_for(job: &Job, ask: &Ask, cfg_model: &str, cfg_effort: &str) -> (String, String, bool, bool) {
    let model_asked = ask.model.is_some() || job.model_asked;
    let effort_asked = ask.effort.is_some() || job.effort_asked;
    let model = ask.model.clone().unwrap_or_else(|| if job.model_asked { job.model.clone() } else { cfg_model.to_string() });
    let effort = ask.effort.clone().unwrap_or_else(|| if job.effort_asked { job.effort.clone() } else { cfg_effort.to_string() });
    (model, effort, model_asked, effort_asked)
}

/// Job `id`'s next turn (`next`, as `ask` asks it), at the mode it runs at
/// now ([`modes::next_turn`], lowered to `ask.mode`), the model and effort
/// [`model_for`] says and the way Settings pay now; `new_turn`: a turn of
/// its own (else the newest carries on). One that never ran starts from its
/// brief (with the message added).
fn resume(id: &str, next: Next, ask: &Ask, new_turn: bool) {
    let cfg = crate::config::load();
    let Some(job) = store::get(id) else { return };
    let chip = modes::or_auto(&store::mode_of(job.conversation, &cfg.jobs_mode));
    let cap = folders::list().into_iter().find(|f| folders::norm(&f.path) == folders::norm(&job.path)).and_then(|f| f.cap_mode());
    let was_plan = modes::or_auto(&job.mode) == Mode::Plan;
    let go = ask.go;
    let mut mode = modes::next_turn(modes::or_auto(&job.mode), chip, cap, go, store::bypass_confirmed());
    if let Some(lower) = ask.mode {
        mode = mode.min(lower);
    }
    let (model, effort, model_asked, effort_asked) = model_for(&job, ask, &cfg.jobs_model, &cfg.jobs_effort);
    let pay = if cfg.jobs_pay == "key" { "key" } else { "plan" };
    let ran = job.started_ts > 0 && !job.session.is_empty();
    let body = match &next {
        Next::CarryOn => format!("{CARRY_ON}\n\n{SUMMARY_LINE}"),
        Next::Message(t) if go && was_plan => format!("{t}\n\nGo ahead: carry out the plan. {SUMMARY_LINE}"),
        Next::Message(t) => format!("{t}\n\n{SUMMARY_LINE}"),
    };
    let title = ask.title();
    let job = store::update(id, |j| {
        j.keep_turns();
        j.mode = mode.as_str().to_string();
        j.model = model.clone();
        j.effort = effort.clone();
        j.model_asked = model_asked;
        j.effort_asked = effort_asked;
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
            j.turn_list.push(store::TurnNote { n, title: title.clone(), request: text, message_id: ask.message_id.clone(), ..Default::default() });
        }
        if let Some(t) = j.turn_list.last_mut() {
            t.status = Status::Queued;
            t.mode = mode.as_str().to_string();
            t.model = model.clone();
            t.effort = effort.clone();
        }
    });
    if let Some(job) = job {
        let n = job.turn_list.last().map_or(0, |t| t.n);
        tracing::info!(id, turn = n, mode = mode.as_str(), go, new_turn, "Jobs: its next turn");
        let step = if new_turn || matches!(next, Next::Message(_)) { "Picking up your request" } else { "Picking up where it left off" };
        emit(&JobEvent { step: Some(step.into()), ..JobEvent::at(&job, n) });
    }
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

/// **Undo job `id`** (Undo all): every file it changed, in every turn not
/// undone already, put back as it was before its first turn, every file it
/// made deleted (`backup::restore_all`). Not while it runs.
pub fn undo(id: &str) -> Result<claude_live::backup::RestoreReport, String> {
    let job = store::get(id).ok_or("There's no such job.")?;
    if job.status.live() {
        return Err("Stop the job first.".into());
    }
    if job.swept {
        return Err("Its backups are gone: Yap keeps them 7 days.".into());
    }
    let undone: Vec<u32> = job.turn_list.iter().filter(|t| t.undone).map(|t| t.n).collect();
    let all = store::backup_dirs(id);
    if all.is_empty() {
        return Err("This job didn't change any files.".into());
    }
    let dirs: Vec<PathBuf> = all.iter().filter(|(n, _)| !undone.contains(n)).map(|(_, d)| d.clone()).collect();
    if dirs.is_empty() {
        return Err("Its files are back as they were already.".into());
    }
    let report = claude_live::backup::restore_all(&dirs);
    if let Some(e) = &report.error {
        return Err(e.clone());
    }
    tracing::info!(id, turns = dirs.len(), restored = report.restored.len(), deleted = report.deleted.len(), failed = report.failed.len(), "Jobs: undone");
    let turns: Vec<u32> = all.iter().map(|(n, _)| *n).collect();
    if let Some(job) = store::update(id, |j| {
        j.undone = true;
        for t in j.turn_list.iter_mut().filter(|t| turns.contains(&t.n)) {
            t.undone = true;
        }
    }) {
        emit(&JobEvent { step: Some(undone_step(&report)), ..JobEvent::of(&job) });
    }
    Ok(report)
}

/// The later turn of `later` (`(turn, the files it changed)`, oldest first)
/// that changed one of `files` too, and that file: the reason an earlier
/// turn can't be undone first. Pure.
pub fn later_change<'a>(files: &[PathBuf], later: &'a [(u32, Vec<PathBuf>)]) -> Option<(u32, &'a PathBuf)> {
    later.iter().rev().find_map(|(n, theirs)| theirs.iter().find(|f| files.iter().any(|m| claude_live::backup::same_path(m, f))).map(|f| (*n, f)))
}

/// **Undo turn `n` of job `id`**: the files that turn changed put back as
/// they were before it, the files it made deleted. Refused while a later
/// turn, not undone, changed one of the same files ("Undo the later change
/// first"). Not while it runs.
pub fn undo_turn(id: &str, n: u32) -> Result<claude_live::backup::RestoreReport, String> {
    let job = store::get(id).ok_or("There's no such job.")?;
    if job.status.live() {
        return Err("Stop the job first.".into());
    }
    if job.swept {
        return Err("Its backups are gone: Yap keeps them 7 days.".into());
    }
    let notes = job.turn_notes();
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
        let shown = steps::shown_path(&file.to_string_lossy(), std::path::Path::new(&job.path));
        tracing::info!(id, turn = n, later = m, "Jobs: undo refused: a later turn changed the same file");
        return Err(format!("Undo the later change first: “{title}” changed {shown} too."));
    }
    let report = claude_live::backup::restore_all(&mine);
    if let Some(e) = &report.error {
        return Err(e.clone());
    }
    tracing::info!(id, turn = n, restored = report.restored.len(), deleted = report.deleted.len(), failed = report.failed.len(), "Jobs: a turn undone");
    let with_backups: Vec<u32> = all.iter().map(|(m, _)| *m).collect();
    if let Some(job) = store::update(id, |j| {
        j.keep_turns();
        if let Some(t) = j.turn_mut(n) {
            t.undone = true;
        }
        // Every turn with backups undone: the job is.
        j.undone = with_backups.iter().all(|m| j.turn_list.iter().any(|t| t.n == *m && t.undone));
    }) {
        emit(&JobEvent { step: Some(undone_step(&report)), ..JobEvent::at(&job, n) });
    }
    Ok(report)
}

/// **Open in terminal**: the job's session in Windows Terminal, in its
/// folder (`claude --resume`), once it isn't running.
pub fn open_terminal(id: &str) -> Result<(), String> {
    let job = store::get(id).ok_or("There's no such job.")?;
    if job.status.live() {
        return Err("Stop the job first: then open it in a terminal.".into());
    }
    let folder = PathBuf::from(&job.path);
    if !folder.is_dir() {
        return Err("The job's folder isn't there any more.".into());
    }
    tracing::info!(id, "Jobs: opened in a terminal");
    cli::open_resume(&folder, &job.session, &format!("{} · Claude Code", job.folder))
}

/// **Yap is quitting**: the jobs running end with it (the kill-on-close
/// job), interrupted — Continue picks them up next time.
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
        tracing::info!(running = live.len(), queued = queued.len(), "Jobs: interrupted as Yap quits");
    }
}

/// **Delete your data** (`privacy/reset.rs`): every job stops first.
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
    fn three_jobs_at_once_and_one_per_folder() {
        assert!(fits([], 3, r"d:\a"));
        assert!(fits([r"d:\a", r"d:\b"], 3, r"d:\c"));
        assert!(!fits([r"d:\a", r"d:\b", r"d:\c"], 3, r"d:\d"), "three running: queued");
        assert!(!fits([r"d:\a"], 3, r"d:\a"), "one per folder");
        assert!(!fits([r"d:\a"], 1, r"d:\b"), "jobs_max 1");
        assert_eq!(folders::norm("D:/Projects/Garden/"), folders::norm(r"d:\projects\garden"));
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
    fn a_job_runs_on_the_persons_own_setup_with_yaps_endpoint_added() {
        let job = Job { model: "claude-sonnet-5-5".into(), effort: "high".into(), ..Default::default() };
        let folder = std::env::temp_dir();
        let mcp = serde_json::json!({ "mcpServers": { "yap": { "type": "http" } } });
        let hooks = serde_json::json!({ "hooks": {} });
        let files = session::Files { mcp_config: Some("mcp.json".into()), settings: Some("settings.json".into()), system_prompt: None, own_prompt: None };
        let key = vec![(session::RUN_KEY_VAR.to_string(), "run-key".to_string())];
        let spec = job_spec(&job, &folder, Pay::Plan, Mode::Plan, Conversation::New("4d7c1d2e-8a51-4c3b-9f0e-2b6a7e1c5d90".into()), mcp.clone(), hooks.clone(), key.clone());
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
        let spec = job_spec(&job, &folder, Pay::Plan, Mode::Auto, Conversation::Resume("prev".into()), mcp, hooks, key);
        let a = session::args(&spec, &files);
        let after = |flag: &str| a.iter().position(|x| x == flag).and_then(|i| a.get(i + 1)).map(String::as_str);
        assert_eq!((after("--resume"), after("--permission-mode")), (Some("prev"), Some("auto")));
        assert!(!a.iter().any(|x| x == "--session-id"));
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
        let job = Job { brief: "  Tidy the readme.  ".into(), ..Default::default() };
        assert_eq!(opening(&job, None), format!("Tidy the readme.\n\n{SUMMARY_LINE}"));
        assert_eq!(opening(&job, Some("Also the changelog.")), "Also the changelog.");
        assert!(SUMMARY_LINE.starts_with("When you're done, finish with a short summary of what you changed and anything you couldn't do."));
    }

    #[test]
    fn the_event_says_only_what_changed() {
        let job = Job { id: "ab12".into(), folder: "garden-planner".into(), status: Status::Running, conversation: Some(3), ..Default::default() };
        let ev = JobEvent { step: Some("Reading notes.md".into()), ..JobEvent::of(&job) };
        let v = serde_json::to_value(&ev).unwrap();
        assert_eq!(v["id"], "ab12");
        assert_eq!(v["status"], "running");
        assert_eq!(v["conversation"], 3);
        assert_eq!(v["step"], "Reading notes.md");
        for absent in ["file", "command", "about", "said", "todos", "now", "diff", "text", "cost", "problem"] {
            assert!(v.get(absent).is_none(), "{absent}");
        }
        // A diff and a to-do list, as the tab reads them.
        let ev = JobEvent {
            todos: Some(vec![steps::Todo { content: "Sort the seeds".into(), status: "in_progress".into(), active_form: "Sorting the seeds".into() }]),
            now: Some("Sorting the seeds".into()),
            diff: Some(diff::of_write("seeds.md", "# Seeds", "new")),
            ..JobEvent::of(&job)
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
        // The job wrote it before (it made it, then wrote it again).
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
    /// job's, right in `backup/`): each file with its text before the turn,
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
        let folder = std::env::temp_dir().join(format!("yap-job-turns-{}", store::new_id()));
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn a_request_while_a_turn_works_queues_as_its_next_turn() {
        let id = store::new_id();
        store::insert(Job {
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
            let mut l = Live::new(folders::norm(r"D:\Projects\garden-planner"));
            l.started = true;
            s.live.insert(id.clone(), l);
        });
        let (job, how) = follow_up(&id, Ask { text: "Add a minimap button as well.".into(), message_id: Some("chat:4:2".into()), mode: Some(Mode::Plan), ..Default::default() }).unwrap();
        assert_eq!(how, FollowedUp::Queued);
        assert_eq!(job.status, Status::Running, "the turn working carries on");
        assert_eq!(job.turn_list.len(), 2);
        let next = &job.turn_list[1];
        assert_eq!((next.n, next.status, next.title.as_str(), next.message_id.as_deref()), (2, Status::Queued, "Add a minimap button as well", Some("chat:4:2")));
        assert!(!next.ran());
        // Another while it still works: the same next turn, its words added.
        let (job, how) = follow_up(&id, Ask::of("And a legend for it.", false)).unwrap();
        assert_eq!(how, FollowedUp::Queued);
        assert_eq!(job.turn_list.len(), 2, "never a third turn, nor a second job");
        assert_eq!(job.turn_list[1].request, "Add a minimap button as well.\n\nAnd a legend for it.");
        let pending = with_state(|s| s.live.get(&id).and_then(|l| l.pending.clone())).unwrap();
        assert_eq!(pending.text, "Add a minimap button as well.\n\nAnd a legend for it.");
        assert_eq!(pending.mode, Some(Mode::Plan), "the lower mode asked kept");
        assert_eq!(pending.message_id.as_deref(), Some("chat:4:2"));
        with_state(|s| s.live.remove(&id));
        assert!(follow_up(&id, Ask::of("   ", false)).is_err());

        // Words while its process starts: with its first message, the same turn.
        let starting = store::new_id();
        store::insert(Job { id: starting.clone(), status: Status::Queued, turn_list: vec![TurnNote { status: Status::Queued, request: "Tidy notes.md.".into(), ..turn(1, "Tidy notes.md", &[]) }], ..Default::default() });
        with_state(|s| s.live.insert(starting.clone(), Live::new("x".into())));
        let (job, how) = follow_up(&starting, Ask::of("Keep the headings.", false)).unwrap();
        assert_eq!(how, FollowedUp::Added);
        assert_eq!(job.turn_list.len(), 1);
        assert_eq!(job.turn_list[0].request, "Tidy notes.md.\n\nKeep the headings.");
        assert_eq!(with_state(|s| s.live.remove(&starting).map(|l| l.extra)), Some(vec!["Keep the headings.".to_string()]));
    }

    #[test]
    fn a_follow_ups_ask_merges_and_its_model_follows_settings_unless_asked() {
        let mut a = Ask { text: "Add a minimap.".into(), mode: Some(Mode::Auto), ..Default::default() };
        a.merge(Ask { text: "Go ahead.".into(), go: true, mode: Some(Mode::Plan), model: Some("claude-opus-5-5".into()), ..Default::default() });
        assert_eq!((a.text.as_str(), a.go, a.mode, a.model.as_deref()), ("Add a minimap.\n\nGo ahead.", true, Some(Mode::Plan), Some("claude-opus-5-5")));
        assert_eq!(Ask::of("Add a minimap button to the map view. It shows the beds.", false).title(), "Add a minimap button to the map view");
        // Settings' model and effort as they are now…
        let job = Job { model: "claude-sonnet-5-5".into(), effort: "medium".into(), ..Default::default() };
        assert_eq!(model_for(&job, &Ask::default(), "claude-haiku-5-5", "high"), ("claude-haiku-5-5".into(), "high".into(), false, false));
        // …the person's own for this turn…
        let opus = Ask { model: Some("claude-opus-5-5".into()), ..Default::default() };
        assert_eq!(model_for(&job, &opus, "claude-haiku-5-5", "high"), ("claude-opus-5-5".into(), "high".into(), true, false));
        // …and kept for the turns after, once they asked.
        let asked = Job { model: "claude-opus-5-5".into(), model_asked: true, ..job };
        assert_eq!(model_for(&asked, &Ask::default(), "claude-haiku-5-5", "high").0, "claude-opus-5-5");
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
            store::insert(Job {
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
        // Now the earlier one goes too: every turn undone, the job is.
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
        assert_eq!(undo(&d).unwrap_err(), "Stop the job first.");
        assert_eq!(undo_turn(&d, 2).unwrap_err(), "Stop the job first.");
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn an_older_jobs_single_backup_is_its_turn_one() {
        let folder = workspace();
        let notes = folder.join("notes.md");
        // From before turns: one backup, right in backup/, no turns kept.
        let id = store::new_id();
        store::insert(Job { id: id.clone(), folder: "garden-planner".into(), path: folder.to_string_lossy().into_owned(), brief: "Tidy notes.md.".into(), status: Status::Done, started_ts: 1, turns: 2, files: vec!["notes.md".into()], ..Default::default() });
        backed_up(&id, 0, &[(&notes, Some("v0"))]);
        std::fs::write(&notes, "v1").unwrap();
        assert_eq!(store::backup_dirs(&id).iter().map(|(n, _)| *n).collect::<Vec<_>>(), [1]);
        let j = store::get(&id).unwrap();
        assert!(j.turn_list.is_empty());
        assert_eq!(j.turn_notes().iter().map(|t| (t.n, t.title.as_str())).collect::<Vec<_>>(), [(1, "Tidy notes.md")]);
        undo_turn(&id, 1).unwrap();
        assert_eq!(read(&notes), "v0");
        assert!(store::get(&id).unwrap().undone);
        // Undo all, as Undo job did.
        let again = store::new_id();
        store::insert(Job { id: again.clone(), path: folder.to_string_lossy().into_owned(), status: Status::Done, files: vec!["notes.md".into()], ..Default::default() });
        backed_up(&again, 0, &[(&notes, Some("v0"))]);
        std::fs::write(&notes, "v1").unwrap();
        undo(&again).unwrap();
        assert_eq!(read(&notes), "v0");
        // Its turn 1 carried on since (a backup/1/ of its own): the older copy wins.
        let both = store::new_id();
        store::insert(Job { id: both.clone(), path: folder.to_string_lossy().into_owned(), status: Status::Done, ..Default::default() });
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
        let job = Job { id: "ab12".into(), turn_list: vec![turn(1, "a", &[]), TurnNote { status: Status::Queued, ..turn(2, "b", &[]) }], ..Default::default() };
        // The newest that ran (the next one is waiting).
        let job = Job { turn_list: vec![job.turn_list[0].clone(), TurnNote { started_ts: 0, ..job.turn_list[1].clone() }], ..job };
        assert_eq!(JobEvent::of(&job).turn, Some(1));
        assert_eq!(JobEvent::at(&job, 2).turn, Some(2));
        assert_eq!(JobEvent::of(&Job::default()).turn, None, "an older job's: none");
        let v = serde_json::to_value(JobEvent { queued: Some("Add a minimap".into()), ..JobEvent::at(&job, 2) }).unwrap();
        assert_eq!((v["turn"].as_u64(), v["queued"].as_str()), (Some(2), Some("Add a minimap")));
    }
}

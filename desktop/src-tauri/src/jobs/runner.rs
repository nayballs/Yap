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
//!   backup manifest), the commands it runs, what it wasn't allowed to do,
//!   Claude Code's problems (signed out, at the plan's limit, paying
//!   another way: that one is stopped at once) and the result — each sent
//!   as the app event **`yap-job`** (`{id, status, folder, conversation?,
//!   step?, file?, command?, text?, cost?, problem?, ts}`) and kept in the
//!   job's `log.jsonl` for its tab.
//! - **Asking**: a permission card waiting for the person (the endpoint's
//!   `pending()`): the job is `asking` until it's answered.
//! - **A turn ends**: the process is told its input is over and exits; a
//!   message for the job later (`message_job`, Continue) starts it again on
//!   `--resume`, at the chat's Mode then (`modes::next_turn`: a plan stays
//!   a plan until "go ahead"). A message while it works goes to it at once
//!   (its next turn, queued by Claude Code).
//! - **Done or failed**: reported in its chat (`chat/turn/report.rs`) —
//!   never one the person stopped.
//! - **Jobs die with Yap** (the kill-on-close job): quitting marks them
//!   interrupted ([`shutdown`]); `store::at_start` does after a crash.
//!
//! The log has ids, counts and outcomes: never a brief, a path, a command or
//! an output.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::ai::claude_code::cli;
use crate::ai::claude_code::events::{Block, Event, Problem, TurnResult};
use crate::ai::claude_code::session::{self, ApiKey, Conversation, Effort, Pay, Session, Spec};
use crate::integrations::claude_live::{self, Approval, RunSpec, Tools};

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
    /// A command it runs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Its words: as it works, then its final ones.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// The API's price so far (Claude Code's estimate).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<ProblemNote>,
    /// Unix seconds.
    pub ts: u64,
}

impl JobEvent {
    fn of(job: &Job) -> JobEvent {
        JobEvent { id: job.id.clone(), status: job.status, folder: job.folder.clone(), conversation: job.conversation, ts: store::now_secs(), ..Default::default() }
    }
}

/// A job that has a slot.
struct Live {
    /// Set once its Claude Code started.
    session: Option<Arc<Session>>,
    /// User messages sent, and results back: its input ends when they meet.
    sent: u32,
    results: u32,
    /// Its input is over (it's finishing): a message waits for the next turn.
    input_over: bool,
    /// Messages for its next turn.
    pending: Vec<String>,
    /// The person stopped it (when).
    stopping: Option<Instant>,
    /// Its folder, compared ([`folders::norm`]).
    folder: String,
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
        s.live.insert(id.to_string(), Live { session: None, sent: 0, results: 0, input_over: false, pending: Vec::new(), stopping: None, folder });
        Placed::Go
    });
    match placed {
        Placed::Already => {}
        Placed::Queued => {
            if let Some(job) = store::update(id, |j| j.status = Status::Queued) {
                tracing::info!(id = %job.id, "Jobs: queued behind the others");
                emit(&JobEvent { step: Some("Waiting for another job to finish".into()), ..JobEvent::of(&job) });
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
            s.live.insert(id.clone(), Live { session: None, sent: 0, results: 0, input_over: false, pending: Vec::new(), stopping: None, folder });
            go.push((id, message));
        }
        go
    });
    for (id, message) in go {
        tauri::async_runtime::spawn(async move { run(id, message).await });
    }
}

/// One turn of job `id`, from start to the process gone; then its slot goes
/// to the next, and it's reported — or, told something while it finished,
/// straight on with that.
async fn run(id: String, message: Option<String>) {
    let status = turn(&id, message).await;
    // Messages sent while it finished its turn: for its next one.
    let pending = with_state(|s| s.live.remove(&id).map(|l| l.pending).unwrap_or_default());
    store::flush();
    let ended = matches!(status, Status::Done | Status::Failed);
    if ended && !pending.is_empty() {
        tracing::info!(id = %id, messages = pending.len(), "Jobs: carrying on with what was sent meanwhile");
        resume(&id, Next::Message(pending.join("\n\n")), false);
    } else if ended {
        crate::chat::turn::queue_job_report(&id);
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

/// The job failed before Claude Code did anything: said, and saved.
fn failed_early(id: &str, why: String, problem: Option<ProblemNote>) -> Status {
    tracing::info!(id, "Jobs: couldn't start");
    if let Some(job) = store::update(id, |j| {
        j.status = Status::Failed;
        j.ended_ts = store::now_secs();
        j.error = Some(why.clone());
        j.problem = problem.clone();
        j.step.clear();
    }) {
        emit(&JobEvent { text: Some(why), problem, ..JobEvent::of(&job) });
    }
    Status::Failed
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
/// with Yap's endpoint added (`mcp_config`, the backup hook's `settings`),
/// its permission prompts to Yap's `approve`, at `mode`, its model and
/// effort, on `conversation`. Pure.
pub fn job_spec(job: &Job, folder: &std::path::Path, pay: Pay, mode: Mode, conversation: Conversation, mcp_config: serde_json::Value, settings: serde_json::Value) -> Spec {
    let mut spec = Spec::new(folder, pay);
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
/// sent, followed until it's gone.
async fn turn(id: &str, message: Option<String>) -> Status {
    let Some(job) = store::get(id) else { return Status::Failed };
    let cfg = crate::config::load();
    let Some(program) = cli::find() else {
        return failed_early(id, "Claude Code isn't installed on this PC.".into(), None);
    };
    let status = tauri::async_runtime::spawn_blocking(cli::status).await.unwrap_or_default();
    if status.too_old {
        let v = status.version.unwrap_or_default();
        return failed_early(id, format!("Claude Code {v} is too old for jobs: update it to {} or later.", cli::MIN_VERSION), None);
    }
    let pay = match pay_for(&job, &cfg, &status) {
        Ok(p) => p,
        Err((why, problem)) => return failed_early(id, why, problem),
    };
    let folder = PathBuf::from(&job.path);
    if !folder.is_dir() {
        return failed_early(id, "The job's folder isn't there any more.".into(), None);
    }
    let Some(backup) = store::backup_dir(id) else { return failed_early(id, "That isn't one of Yap's jobs.".into(), None) };
    if let Err(e) = std::fs::create_dir_all(&backup) {
        return failed_early(id, format!("Couldn't make the job's backup folder: {e}"), None);
    }
    let run = match claude_live::register_run(RunSpec {
        label: job.folder.clone(),
        tools: Tools::Job,
        backup_dir: Some(backup.clone()),
        folder: Some(folder.clone()),
        approval: Some(Approval { run: id.to_string(), conversation: job.conversation, origin: job.origin }),
    }) {
        Ok(r) => r,
        Err(e) => return failed_early(id, format!("Couldn't start the job: {e}"), None),
    };
    let mode = modes::or_auto(&job.mode);
    let resuming = message.is_some() && job.started_ts > 0 && !job.session.is_empty();
    let session_id = if job.session.is_empty() { session::new_session_id() } else { job.session.clone() };
    let conversation = if resuming { Conversation::Resume(session_id) } else { Conversation::New(session_id) };
    let spec = job_spec(&job, &folder, pay, mode, conversation, run.mcp_config(), run.settings());
    let (session, mut rx) = match Session::start_with(&program, spec) {
        Ok(s) => s,
        Err(e) => return failed_early(id, e, None),
    };
    let session = Arc::new(session);
    let first = opening(&job, message.as_deref());
    // The person stopped it while it started: ended now.
    let (stopped_early, pending) = with_state(|s| match s.live.get_mut(id) {
        Some(l) => {
            l.session = Some(Arc::clone(&session));
            l.sent = 1 + l.pending.len() as u32;
            (l.stopping.is_some(), std::mem::take(&mut l.pending))
        }
        None => (true, Vec::new()),
    });
    if stopped_early {
        session.kill();
    } else {
        let sent = std::iter::once(first.as_str()).chain(pending.iter().map(String::as_str)).try_for_each(|m| session.send_user(m, &[]));
        if let Err(e) = sent {
            session.kill();
            return failed_early(id, e, None);
        }
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
    }) else {
        session.kill();
        return Status::Failed;
    };
    tracing::info!(id, mode = mode.as_str(), pay = %job.pay, resuming, turn = job.turns, "Jobs: started");
    emit(&JobEvent { step: Some("Starting Claude Code".into()), ..JobEvent::of(&job) });

    let mut follow = Follow::new(&job, folder, backup, mode);
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
    follow.finish()
}

/// **Following one turn** of a job.
struct Follow {
    id: String,
    folder: PathBuf,
    backup: PathBuf,
    mode: Mode,
    /// What the job had cost before this turn.
    cost_before: f64,
    /// Its latest words.
    last_text: String,
    /// The latest result's outcome and words.
    outcome: Option<(Status, String)>,
    problem: Option<ProblemNote>,
    asking: bool,
    /// When its input ended (its last result came).
    input_over_at: Option<Instant>,
    last_flush: Instant,
}

impl Follow {
    fn new(job: &Job, folder: PathBuf, backup: PathBuf, mode: Mode) -> Follow {
        Follow {
            id: job.id.clone(),
            folder,
            backup,
            mode,
            cost_before: job.cost,
            last_text: String::new(),
            outcome: None,
            problem: None,
            asking: false,
            input_over_at: None,
            last_flush: Instant::now(),
        }
    }

    fn event(&self, job: &Job) -> JobEvent {
        JobEvent::of(job)
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
                        Block::ToolUse { name, input, .. } => {
                            let seen = steps::of_tool(name, input, &self.folder);
                            let Some(job) = store::touch(&self.id, |j| {
                                j.step = seen.step.clone();
                                if let Some(f) = &seen.file {
                                    if !j.files.contains(f) {
                                        j.files.push(f.clone());
                                    }
                                }
                                if let Some(c) = &seen.command {
                                    j.commands.push(c.clone());
                                }
                            }) else {
                                return;
                            };
                            emit(&JobEvent { step: Some(seen.step), file: seen.file, command: seen.command, ..self.event(&job) });
                        }
                        Block::Text(t) if !t.trim().is_empty() => {
                            self.last_text = t.trim().to_string();
                            if let Some(job) = store::get(&self.id) {
                                emit(&JobEvent { text: Some(self.last_text.clone()), ..self.event(&job) });
                            }
                        }
                        _ => {}
                    }
                }
            }
            Event::PermissionDenied(d) => {
                let detail = if d.message.trim().is_empty() { format!("Use {}", d.tool_name) } else { steps::one_line(&d.message, 160) };
                let line = format!("Not allowed: {}", d.tool_name);
                if let Some(job) = store::touch(&self.id, |j| j.denials.push(Denial { tool: d.tool_name.clone(), detail })) {
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
        let job = store::update(&self.id, |j| {
            j.cost = cost;
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
            if let Some(job) = store::update(&self.id, |j| {
                if j.status.live() {
                    j.status = status;
                }
            }) {
                let step = asking.then(|| "Waiting for you to allow a step".to_string());
                emit(&JobEvent { step, ..self.event(&job) });
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
        let job = store::update(&self.id, |j| {
            j.status = status;
            j.ended_ts = store::now_secs();
            if !text.is_empty() {
                j.text = text.clone();
            }
            j.error = error.clone();
            j.plan = plan;
            j.asks = asks;
            j.step.clear();
        });
        tracing::info!(id = %self.id, status = status.word(), plan, asks, "Jobs: ended");
        if let Some(job) = job {
            emit(&JobEvent { text: Some(job.text.clone()).filter(|t| !t.is_empty()).or(error), cost: Some(job.cost), problem: job.problem.clone(), ..JobEvent::of(&job) });
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
            let job = store::update(id, |j| {
                j.status = Status::Stopped;
                j.ended_ts = store::now_secs();
                j.step.clear();
            })
            .ok_or("There's no such job.")?;
            tracing::info!(id, "Jobs: stopped before it ran");
            emit(&JobEvent::of(&job));
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

/// **A message for job `id`** (the chat's `message_job`, the tab's box):
/// to it at once while it works, else its next turn on `--resume`. `go`:
/// a plan's go-ahead (the next turn at the chat's Mode, carrying it out).
pub fn message(id: &str, text: &str, go: bool) -> Result<Job, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("Say what to tell the job.".into());
    }
    let job = store::get(id).ok_or("There's no such job.")?;
    match job.status {
        Status::Running | Status::Asking => {
            // To it now (its next turn, as Claude Code queues it), or after
            // the turn it's finishing.
            let session = with_state(|s| match s.live.get_mut(id) {
                Some(l) if !l.input_over && l.session.is_some() => {
                    l.sent += 1;
                    l.session.clone()
                }
                Some(l) => {
                    l.pending.push(text.to_string());
                    None
                }
                None => None,
            });
            if let Some(session) = session {
                if session.send_user(text, &[]).is_err() {
                    with_state(|s| {
                        if let Some(l) = s.live.get_mut(id) {
                            l.sent = l.sent.saturating_sub(1);
                            l.pending.push(text.to_string());
                        }
                    });
                }
            }
            tracing::info!(id, "Jobs: a message for the job while it works");
            emit(&JobEvent { step: Some("Got your message".into()), ..JobEvent::of(&job) });
            Ok(job)
        }
        Status::Queued => {
            let job = store::update(id, |j| j.brief = format!("{}\n\nAlso: {text}", j.brief.trim())).ok_or("There's no such job.")?;
            Ok(job)
        }
        _ => {
            resume(id, Next::Message(text.to_string()), go);
            store::get(id).ok_or_else(|| "There's no such job.".into())
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

/// **Continue job `id`** (stopped, interrupted or failed): on `--resume`,
/// carrying on; one that never ran starts from its brief.
pub fn continue_job(id: &str) -> Result<Job, String> {
    let job = store::get(id).ok_or("There's no such job.")?;
    if job.status.live() {
        return Err("The job is still running.".into());
    }
    if job.status == Status::Done {
        return Err("The job is done: send it a message to carry on.".into());
    }
    resume(id, Next::CarryOn, false);
    store::get(id).ok_or_else(|| "There's no such job.".into())
}

/// Job `id`'s next turn (`next`), at the mode it runs at now
/// ([`modes::next_turn`]; `go`: a plan's go-ahead); one that never ran
/// starts from its brief (with the message added).
fn resume(id: &str, next: Next, go: bool) {
    let cfg = crate::config::load();
    let Some(job) = store::get(id) else { return };
    let chip = modes::or_auto(&store::mode_of(job.conversation, &cfg.jobs_mode));
    let cap = folders::list().into_iter().find(|f| folders::norm(&f.path) == folders::norm(&job.path)).and_then(|f| f.cap_mode());
    let was_plan = modes::or_auto(&job.mode) == Mode::Plan;
    let mode = modes::next_turn(modes::or_auto(&job.mode), chip, cap, go, store::bypass_confirmed());
    let ran = job.started_ts > 0 && !job.session.is_empty();
    let body = match &next {
        Next::CarryOn => format!("{CARRY_ON}\n\n{SUMMARY_LINE}"),
        Next::Message(t) if go && was_plan => format!("{t}\n\nGo ahead: carry out the plan. {SUMMARY_LINE}"),
        Next::Message(t) => format!("{t}\n\n{SUMMARY_LINE}"),
    };
    let job = store::update(id, |j| {
        j.mode = mode.as_str().to_string();
        j.status = Status::Queued;
        j.undone = false;
        if let (false, Next::Message(t)) = (ran, &next) {
            j.brief = format!("{}\n\n{}", j.brief.trim(), t.trim());
        }
    });
    if let Some(job) = job {
        tracing::info!(id, mode = mode.as_str(), go, "Jobs: its next turn");
        emit(&JobEvent { step: Some("Picking up where it left off".into()), ..JobEvent::of(&job) });
    }
    schedule(id, ran.then_some(body));
}

/// **Undo job `id`**: every file it changed put back as it was before it,
/// every file it made deleted (`backup::restore`). Not while it runs.
pub fn undo(id: &str) -> Result<claude_live::backup::RestoreReport, String> {
    let job = store::get(id).ok_or("There's no such job.")?;
    if job.status.live() {
        return Err("Stop the job first.".into());
    }
    if job.swept {
        return Err("Its backups are gone: Yap keeps them 7 days.".into());
    }
    let dir = store::backup_dir(id).filter(|d| d.is_dir()).ok_or("This job didn't change any files.")?;
    let report = claude_live::backup::restore(&dir);
    if let Some(e) = &report.error {
        return Err(e.clone());
    }
    let put_back = report.restored.len() + report.deleted.len();
    tracing::info!(id, restored = report.restored.len(), deleted = report.deleted.len(), failed = report.failed.len(), "Jobs: undone");
    if let Some(job) = store::update(id, |j| j.undone = true) {
        let step = match put_back {
            0 => "Undo: nothing to put back".to_string(),
            1 => "Undone: 1 file put back".to_string(),
            n => format!("Undone: {n} files put back"),
        };
        emit(&JobEvent { step: Some(step), ..JobEvent::of(&job) });
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
                j.status = Status::Interrupted;
                j.ended_ts = now;
            }
        });
        if let Some(s) = session {
            s.kill();
        }
    }
    // Queued ones too.
    let queued: Vec<String> = store::list().into_iter().filter(|j| j.status.live()).map(|j| j.id).collect();
    for id in &queued {
        store::update(id, |j| {
            j.status = Status::Interrupted;
            j.ended_ts = now;
        });
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
        let files = session::Files { mcp_config: Some("mcp.json".into()), settings: Some("settings.json".into()), system_prompt: None };
        let spec = job_spec(&job, &folder, Pay::Plan, Mode::Plan, Conversation::New("4d7c1d2e-8a51-4c3b-9f0e-2b6a7e1c5d90".into()), mcp.clone(), hooks.clone());
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
        for never in ["--strict-mcp-config", "--setting-sources", "--tools", "--resume", "--permission-prompts"] {
            assert!(!a.iter().any(|x| x == never), "{never} in {a:?}");
        }
        assert_eq!((spec.mcp_config, spec.settings), (Some(mcp.clone()), Some(hooks.clone())));
        // A later turn resumes it, at the mode it runs at then.
        let spec = job_spec(&job, &folder, Pay::Plan, Mode::Auto, Conversation::Resume("prev".into()), mcp, hooks);
        let a = session::args(&spec, &files);
        let after = |flag: &str| a.iter().position(|x| x == flag).and_then(|i| a.get(i + 1)).map(String::as_str);
        assert_eq!((after("--resume"), after("--permission-mode")), (Some("prev"), Some("auto")));
        assert!(!a.iter().any(|x| x == "--session-id"));
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
        for absent in ["file", "command", "text", "cost", "problem"] {
            assert!(v.get(absent).is_none(), "{absent}");
        }
    }
}

//! **The jobs store**: `jobs.json` (every job, each chat's Mode, each Code
//! chat's folder, the one-time Bypass OK), and each job's own folder under
//! `jobs/<id>/` — `log.jsonl`,
//! the `yap-job` events it sent (the tab replays them), and `backup/`, the
//! files Claude Code changed as they were before (the run endpoint's backup
//! hook writes them, `integrations/claude_live/backup.rs`).
//!
//! A file of its own, never fields in chats.json: the installed Yap and a
//! newer dev build share the data folder, and an older Yap drops fields it
//! doesn't know when it saves. Saved through `saving.rs` (a failed save kept
//! in memory and said once; a file Yap couldn't read never written over).
//! The log is appended, line by line, as the job goes (a job's tab can be
//! thousands of lines: the whole file isn't written again each time); a
//! line that couldn't be written is left out, the job itself is in
//! jobs.json.
//!
//! **Jobs die with Yap** (Claude Code runs in Yap's kill-on-close job): one
//! still queued, running or asking when Yap starts is **interrupted**
//! ([`at_start`]), and can be continued. **Backups and logs go after 7 days**
//! ([`sweep`]); the job's record stays (History), marked `swept`.
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

/// How long a job's backups and log are kept after it ends.
pub const KEEP_SECS: u64 = 7 * 24 * 60 * 60;
/// The most lines a job's log keeps (the rest of a runaway job isn't).
pub const LOG_MAX_LINES: usize = 5_000;
/// The most characters of text a log line keeps.
const LOG_TEXT_MAX: usize = 4_000;

/// Where a job is.
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

/// **One job.**
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    /// The chat that started it.
    pub conversation: Option<u64>,
    /// The person's message that asked for it ("chat:<conversation>:<index>";
    /// the answer that started it comes right after it).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// Asked typed or by voice (a voice job's report is read aloud).
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
    /// Claude Code's estimate of the API's price for the job so far
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
    /// Its final words ask the person something.
    pub asks: bool,
    /// Its latest turn has been reported in its chat.
    pub reported: bool,
    /// The report's marker message ("chat:<conversation>:<index>").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_message_id: Option<String>,
    /// Undo job put its files back.
    pub undone: bool,
    /// It changed files and no backups were made: Claude Code's hooks
    /// didn't run (a managed policy turned them off), so Undo can't put
    /// them back (`runner::no_backups`).
    pub no_backups: bool,
    /// Its backups and log are gone (7 days after it ended).
    pub swept: bool,
    /// The diff lines its log has so far (`diff::JOB_MAX` at most).
    pub diff_lines: u32,
    /// The step it's on now (not kept: the tab's live line).
    #[serde(skip)]
    pub step: String,
}

/// A job as the page's tab opens it (`stream::Event::Job`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobRef {
    pub id: String,
    pub folder: String,
    pub status: Status,
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation: Option<u64>,
}

impl Job {
    pub fn reference(&self) -> JobRef {
        JobRef { id: self.id.clone(), folder: self.folder.clone(), status: self.status, mode: self.mode.clone(), conversation: self.conversation }
    }
}

/// What jobs.json holds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Store {
    pub jobs: Vec<Job>,
    /// Each chat's Mode chip (conversation id → mode).
    pub modes: BTreeMap<u64, String>,
    /// Each Code chat's folder (conversation id → its folder, `code_chat.rs`):
    /// beside its Mode, never in chats.json.
    pub code_chats: BTreeMap<u64, CodeChat>,
    /// The Bypass warning was confirmed once.
    pub bypass_confirmed: bool,
}

/// **A Code chat**: the job folder its conversation is attached to.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CodeChat {
    /// The folder's full path (its name can change; its path is the folder).
    pub path: String,
    /// When it was attached (Unix seconds): the workspace picker's recency.
    pub ts: u64,
}

/// jobs.json's saves.
pub static SAVER: crate::shell::saving::Saver = crate::shell::saving::Saver::new("jobs", "your jobs", save_now);

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
        let dir = std::env::temp_dir().join(format!("yap-jobs-test-{tag}"));
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
    data().join("jobs.json")
}

/// `jobs/`: every job's folder.
pub fn root() -> PathBuf {
    data().join("jobs")
}

/// A job id is Yap's own: letters and digits only (never a path).
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 40 && id.chars().all(|c| c.is_ascii_alphanumeric())
}

/// `jobs/<id>/`.
pub fn dir(id: &str) -> Option<PathBuf> {
    valid_id(id).then(|| root().join(id))
}

/// `jobs/<id>/backup/`.
pub fn backup_dir(id: &str) -> Option<PathBuf> {
    dir(id).map(|d| d.join("backup"))
}

/// `jobs/<id>/log.jsonl`.
pub fn log_path(id: &str) -> Option<PathBuf> {
    dir(id).map(|d| d.join("log.jsonl"))
}

/// A new job's id: the time and four random bytes, in hex.
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
    let store = guard.get_or_insert_with(|| SAVER.load(&path()));
    if SAVER.any_unread() && SAVER.refresh(&path(), store) {
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

// ---- jobs ----

/// Every job, newest first.
pub fn list() -> Vec<Job> {
    with_store(|s| {
        let mut jobs = s.jobs.clone();
        jobs.sort_by(|a, b| b.created_ts.cmp(&a.created_ts).then_with(|| b.id.cmp(&a.id)));
        jobs
    })
}

pub fn get(id: &str) -> Option<Job> {
    with_store(|s| s.jobs.iter().find(|j| j.id == id).cloned())
}

/// A new job, saved.
pub fn insert(job: Job) {
    with_store(|s| {
        s.jobs.retain(|j| j.id != job.id);
        s.jobs.push(job);
        save(s);
    });
}

/// Change job `id` (`f`), stamped and saved: the job as it is now.
pub fn update(id: &str, f: impl FnOnce(&mut Job)) -> Option<Job> {
    change(id, true, f)
}

/// [`update`], only in memory (a step's line, a file: saved with the next
/// [`update`] or [`flush`]).
pub fn touch(id: &str, f: impl FnOnce(&mut Job)) -> Option<Job> {
    change(id, false, f)
}

fn change(id: &str, write: bool, f: impl FnOnce(&mut Job)) -> Option<Job> {
    with_store(|s| {
        let job = s.jobs.iter_mut().find(|j| j.id == id)?;
        f(job);
        job.updated_ts = now_secs();
        let out = job.clone();
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

/// The jobs not over yet (queued, running or asking), newest first: their
/// ids (the Yap bar's "1 job running" and where its click goes).
pub fn live_ids() -> Vec<String> {
    with_store(|s| {
        let mut live: Vec<&Job> = s.jobs.iter().filter(|j| j.status.live()).collect();
        live.sort_by(|a, b| b.created_ts.cmp(&a.created_ts).then_with(|| b.id.cmp(&a.id)));
        live.into_iter().map(|j| j.id.clone()).collect()
    })
}

/// The jobs a chat started, newest first.
pub fn of_conversation(conversation: u64) -> Vec<Job> {
    list().into_iter().filter(|j| j.conversation == Some(conversation)).collect()
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
                let same = s.code_chats.get(&conversation).is_some_and(|c| super::folders::norm(&c.path) == super::folders::norm(p));
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
    let want = super::folders::norm(path);
    with_store(|s| {
        let gone: Vec<u64> = s.code_chats.iter().filter(|(_, c)| super::folders::norm(&c.path) == want).map(|(id, _)| *id).collect();
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

/// **Yap is starting**: a job still queued, running or asking died with the
/// last Yap — interrupted now. How many.
pub fn at_start() -> usize {
    let now = now_secs();
    with_store(|s| {
        let n = interrupt_live(&mut s.jobs, now);
        if n > 0 {
            save(s);
        }
        n
    })
}

/// Every live job in `jobs` interrupted at `now`; how many. Pure.
pub fn interrupt_live(jobs: &mut [Job], now: u64) -> usize {
    let mut n = 0;
    for j in jobs.iter_mut().filter(|j| j.status.live()) {
        j.status = Status::Interrupted;
        j.ended_ts = now;
        j.updated_ts = now;
        n += 1;
    }
    n
}

/// The jobs whose backups and log are due to go at `now`: ended over
/// [`KEEP_SECS`] ago, not swept yet, not live. Pure.
pub fn due_for_sweep(jobs: &[Job], now: u64) -> Vec<String> {
    jobs.iter()
        .filter(|j| !j.status.live() && !j.swept)
        .filter(|j| {
            let ended = if j.ended_ts > 0 { j.ended_ts } else { j.updated_ts.max(j.created_ts) };
            now.saturating_sub(ended) >= KEEP_SECS
        })
        .map(|j| j.id.clone())
        .collect()
}

/// **Backups and logs older than 7 days go** (their job's record stays,
/// marked swept), and a job folder no job names, once it's that old. How
/// many folders went.
pub fn sweep(now: u64) -> usize {
    let (due, known): (Vec<String>, Vec<String>) = with_store(|s| (due_for_sweep(&s.jobs, now), s.jobs.iter().map(|j| j.id.clone()).collect()));
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
            for j in s.jobs.iter_mut().filter(|j| due.contains(&j.id)) {
                j.swept = true;
            }
            save(s);
        });
    }
    // Folders of jobs no longer in the store (jobs.json deleted by hand, an
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
        tracing::info!(folders = gone, "Jobs: backups and logs older than 7 days deleted");
    }
    gone
}

// ---- the log ----

/// Append `line` (one `yap-job` event) to job `id`'s log, its texts
/// trimmed; nothing past [`LOG_MAX_LINES`].
pub fn log_append(id: &str, line: &Value) {
    let Some(path) = log_path(id) else { return };
    if crate::ai::eval::active() {
        return;
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
        return;
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
        Ok(()) => LOG_LINES.bump(id),
        Err(e) => tracing::debug!("Jobs: a log line couldn't be written ({})", e.kind()),
    }
}

/// Job `id`'s log, oldest first (lines that don't read are left out).
pub fn log_read(id: &str) -> Vec<Value> {
    let Some(path) = log_path(id) else { return Vec::new() };
    std::fs::read_to_string(path)
        .map(|s| s.lines().filter_map(|l| serde_json::from_str::<Value>(l).ok()).collect())
        .unwrap_or_default()
}

/// How many lines each job's log has (counted once from its file).
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

/// **Delete some of your data → Jobs** (`privacy/reset.rs`): every job, its
/// log and backups, each chat's Mode and Code chat folder, and the Bypass
/// OK. How many jobs went.
pub fn reset() -> usize {
    let n = with_store(|s| {
        let n = s.jobs.len();
        *s = Store::default();
        save(s);
        n
    });
    LOG_LINES.forget_all();
    let root = root();
    if root.exists() {
        if let Err(e) = std::fs::remove_dir_all(&root) {
            tracing::warn!("Jobs: the jobs folder wasn't all deleted ({})", e.kind());
        }
    }
    n
}

/// The bytes the jobs' folders take (logs and backups).
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

    fn job(id: &str, status: Status, ended: u64) -> Job {
        Job { id: id.into(), status, ended_ts: ended, created_ts: 1, folder: "garden-planner".into(), ..Default::default() }
    }

    #[test]
    fn a_job_round_trips_and_an_older_file_reads() {
        let j = Job {
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
        let back: Job = serde_json::from_value(v).unwrap();
        assert_eq!(back, Job { step: String::new(), ..j });
        // A file from before a field existed reads, with defaults.
        let old: Store = serde_json::from_str(r#"{"jobs":[{"id":"ab12","status":"running"}],"modes":{"7":"plan"}}"#).unwrap();
        assert_eq!(old.jobs[0].status, Status::Running);
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
        // Saved as jobs.json has it: its own map, by conversation.
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
    fn jobs_live_at_start_are_interrupted() {
        let mut jobs = vec![job("a", Status::Running, 0), job("b", Status::Done, 5), job("c", Status::Queued, 0), job("d", Status::Asking, 0)];
        assert_eq!(interrupt_live(&mut jobs, 100), 3);
        assert_eq!(jobs.iter().map(|j| j.status).collect::<Vec<_>>(), [Status::Interrupted, Status::Done, Status::Interrupted, Status::Interrupted]);
        assert_eq!(jobs[0].ended_ts, 100);
        assert_eq!(jobs[1].ended_ts, 5, "an ended one is left as it was");
        // Through the store.
        insert(job("r1", Status::Running, 0));
        assert_eq!(at_start(), 1);
        assert_eq!(get("r1").unwrap().status, Status::Interrupted);
        assert_eq!(at_start(), 0);
    }

    #[test]
    fn backups_and_logs_go_after_seven_days_and_the_record_stays() {
        let now = 1_800_000_000;
        let jobs = vec![
            job("old", Status::Done, now - KEEP_SECS - 1),
            job("new", Status::Done, now - KEEP_SECS + 60),
            job("live", Status::Running, 0),
            Job { swept: true, ..job("gone", Status::Failed, 1) },
        ];
        assert_eq!(due_for_sweep(&jobs, now), ["old"]);
        // On disk: the old one's folder goes, the new one's stays.
        for j in &jobs {
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
        insert(job("logged", Status::Running, 0));
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
    fn each_chats_mode_and_the_bypass_ok_are_kept() {
        assert_eq!(mode_of(Some(4), "auto"), "auto");
        set_mode(4, "plan");
        assert_eq!(mode_of(Some(4), "auto"), "plan");
        assert_eq!(mode_of(None, "manual"), "manual");
        assert!(!bypass_confirmed());
        confirm_bypass();
        assert!(bypass_confirmed());
        insert(job("x1", Status::Done, 3));
        std::fs::create_dir_all(backup_dir("x1").unwrap()).unwrap();
        assert!(reset() >= 1);
        assert!(list().is_empty() && !bypass_confirmed() && !root().exists());
    }
}

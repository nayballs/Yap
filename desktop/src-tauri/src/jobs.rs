//! **Jobs** (2026-11-07): from Chat or the Voice Assistant, any chat model
//! hands a task to the person's own Claude Code ("Update the readme in
//! garden-planner"). Yap runs it in that folder in the background, follows
//! it, and the conversation's model reports back when it ends — out loud
//! when it was asked by voice. Yap "runs Claude Code"; the feature is never
//! named after it.
//!
//! - [`store`]: `jobs.json` (every job; each chat's Mode; the one-time
//!   Bypass OK), each job's `jobs/<id>/log.jsonl` and `backup/`; jobs live
//!   when Yap starts are interrupted; backups and logs go after 7 days.
//! - [`folders`]: `job_folders.json`, the folders jobs may work in (a cap
//!   each), and which one a job means — never a guess.
//! - [`modes`]: Claude Code's modes as the Mode chip words them; the chat's
//!   model may only lower its chat's Mode, a folder's cap lowers it, Bypass
//!   needs its one-time OK.
//! - [`runner`]: at most 3 at once, one per folder; each a Claude Code
//!   session with the person's own setup plus Yap's run endpoint (its
//!   permission cards, the backup hook); followed as `yap-job` events;
//!   stop, message, continue, undo, open in terminal.
//! - [`steps`]: each tool call in plain words (a command by its
//!   description, else tidied); Claude Code's to-do list.
//! - [`diff`]: an edit's lines removed and added, a written file's lines,
//!   within their caps, for the tab's "Files changed".
//! - [`consent`]: the person's own words ask for it (as opening an app).
//! - [`voice`]: the Voice Assistant asks out loud before a job that needs
//!   asking; the next question is the answer.
//! - [`code_chat`]: Code chats — a conversation attached to one job folder
//!   (a "workspace" in the UI), kept in `jobs.json`; there the folder
//!   defaults to the chat's and the person's own request is enough.
//! - [`commands`]: the page's commands (the Jobs UI).
//! - [`approve_card`]: a running job's permission prompt on the Yap bar
//!   when no Yap window is in front (Allow once / Deny).
//!
//! Chat's side: `chat/tools/jobs.rs` (`start_job`, `job_status`,
//! `message_job`, `stop_job`), `chat/turn/report.rs` (the report turn).
//! Offered only with "Let Chat start jobs" on and Claude Code on this PC
//! ([`offered`]).

pub mod approve_card;
pub mod code_chat;
pub mod commands;
pub mod consent;
pub mod diff;
pub mod folders;
pub mod modes;
pub mod runner;
pub mod steps;
pub mod store;
pub mod voice;

pub use store::JobRef;

/// **Jobs are offered**: switched on (Settings → Jobs), and Claude Code is
/// on this PC (found, not run: cheap enough for every prompt).
pub fn offered(cfg: &crate::config::YapConfig) -> bool {
    cfg.jobs_enabled && crate::ai::claude_code::cli::find().is_some()
}

/// At start (lib.rs setup): the app's handle; jobs the last Yap left
/// running are interrupted; old backups and logs swept, and again daily.
pub fn init(app: &tauri::AppHandle) {
    runner::init(app);
    let interrupted = store::at_start();
    if interrupted > 0 {
        tracing::info!(interrupted, "Jobs: interrupted when Yap last quit");
    }
    let _ = std::thread::Builder::new().name("jobs-sweep".into()).spawn(|| loop {
        store::sweep(store::now_secs());
        std::thread::sleep(std::time::Duration::from_secs(24 * 60 * 60));
    });
}

/// Quitting (lib.rs `shutdown_cleanup`): running jobs end with Yap.
pub fn shutdown() {
    runner::shutdown();
    store::flush();
}

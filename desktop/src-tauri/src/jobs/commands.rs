//! **The Jobs UI's commands**: the jobs and their logs, Stop, Continue, a
//! message, Undo, Open in terminal, the folders, each chat's Mode and the
//! one-time Bypass OK. (Starting a job is Chat's: `start_job`.)

use serde::Serialize;
use serde_json::Value;

use super::folders::{self, Folder};
use super::modes::{self, Mode};
use super::store::{self, Job};
use super::{consent, runner};

/// A job with its log (the tab replays it).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobWithLog {
    pub job: Job,
    /// Its `yap-job` events, oldest first (empty once swept).
    pub log: Vec<Value>,
}

/// Every job, newest first.
#[tauri::command]
pub async fn job_list() -> Vec<Job> {
    store::list()
}

/// A job and its log.
#[tauri::command]
pub async fn job_get(id: String) -> Result<JobWithLog, String> {
    let job = store::get(&id).ok_or("There's no such job.")?;
    let log = tauri::async_runtime::spawn_blocking(move || store::log_read(&id)).await.unwrap_or_default();
    Ok(JobWithLog { job, log })
}

/// Stop a job (a queued one comes off the queue).
#[tauri::command]
pub async fn job_stop(id: String) -> Result<Job, String> {
    runner::stop(&id)
}

/// Continue a stopped, interrupted or failed job.
#[tauri::command]
pub async fn job_continue(id: String) -> Result<Job, String> {
    runner::continue_job(&id)
}

/// A message for a job, typed in its tab: to it at once while it works,
/// else its next turn. "Go ahead" after a plan carries it out.
#[tauri::command]
pub async fn job_message(id: String, text: String) -> Result<Job, String> {
    let go = consent::go_ahead(&text);
    runner::message(&id, &text, go)
}

/// Undo a job: what Undo put back (`restored`, `deleted`, `skipped`,
/// `refused`, `failed`: paths and reasons).
#[tauri::command]
pub async fn job_undo(id: String) -> Result<crate::integrations::claude_live::backup::RestoreReport, String> {
    tauri::async_runtime::spawn_blocking(move || runner::undo(&id)).await.map_err(|e| e.to_string())?
}

/// The job's session in Windows Terminal, in its folder (`claude
/// --resume`), once it isn't running.
#[tauri::command]
pub async fn job_open_terminal(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || runner::open_terminal(&id)).await.map_err(|e| e.to_string())?
}

/// The folders jobs may work in.
#[tauri::command]
pub async fn job_folders_list() -> Vec<Folder> {
    folders::list()
}

/// Add a folder (Settings → Jobs' Add folder…): its path, an optional name
/// and cap.
#[tauri::command]
pub async fn job_folders_add(path: String, name: Option<String>, cap: Option<String>) -> Result<Folder, String> {
    let added = folders::add(&path, name.as_deref(), cap.as_deref())?;
    folders::saved().map_err(|e| e.to_string())?;
    Ok(added)
}

/// Remove a folder (by its name or path): whether one went. Its jobs stay.
#[tauri::command]
pub async fn job_folders_remove(name: String) -> bool {
    folders::remove(&name)
}

/// Cap a folder's mode (`None`: no cap).
#[tauri::command]
pub async fn job_folders_set_cap(name: String, cap: Option<String>) -> Result<Folder, String> {
    folders::set_cap(&name, cap.as_deref())
}

/// The Mode chip, as the page shows it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModeView {
    /// `auto` | `manual` | `acceptEdits` | `plan` | `bypassPermissions`.
    pub mode: String,
    /// "Auto".
    pub label: String,
    pub bypass_confirmed: bool,
    /// Every mode, in the menu's order: `{mode, label, says}`.
    pub modes: Vec<Value>,
}

fn mode_view(mode: Mode) -> ModeView {
    ModeView {
        mode: mode.as_str().to_string(),
        label: mode.label().to_string(),
        bypass_confirmed: store::bypass_confirmed(),
        modes: Mode::MENU.iter().map(|m| serde_json::json!({ "mode": m.as_str(), "label": m.label(), "says": m.says() })).collect(),
    }
}

/// A chat's Mode (`None`: a new chat's, Settings' default).
#[tauri::command]
pub async fn job_mode_get(conversation: Option<u64>) -> ModeView {
    let default = crate::config::load().jobs_mode;
    mode_view(modes::or_auto(&store::mode_of(conversation, &default)))
}

/// Set a chat's Mode: from the chip, the person's own choice (Bypass only
/// once its warning was confirmed).
#[tauri::command]
pub async fn job_mode_set(conversation: u64, mode: String) -> Result<ModeView, String> {
    let m = Mode::parse(&mode).ok_or("That isn't one of the modes.")?;
    if m == Mode::Bypass && !store::bypass_confirmed() {
        return Err("Confirm Bypass permissions first.".into());
    }
    store::set_mode(conversation, m.as_str());
    tracing::info!(mode = m.as_str(), "Jobs: a chat's Mode set");
    Ok(mode_view(m))
}

/// The person confirmed Bypass permissions' warning, once.
#[tauri::command]
pub async fn job_bypass_confirmed() {
    store::confirm_bypass();
    tracing::info!("Jobs: Bypass permissions confirmed");
}

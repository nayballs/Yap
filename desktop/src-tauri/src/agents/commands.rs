//! **The Agents UI's commands**: the agents and their logs, Stop, Continue, a
//! message, Undo, Open in terminal, the folders, each chat's Mode and
//! workspace (Code chats), and the one-time Bypass OK. (Starting an agent is
//! Chat's: `start_agent`.)

use serde::Serialize;
use serde_json::Value;

use super::workspaces::{self, Folder};
use super::modes::{self, Mode};
use super::store::{self, Agent};
use super::{code_chat, consent, runner};

/// An agent with its log (the tab replays it).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentWithLog {
    pub agent: Agent,
    /// Its `yap-agent` events, oldest first (empty once swept).
    pub log: Vec<Value>,
}

/// Every agent, newest first.
#[tauri::command]
pub async fn agent_list() -> Vec<Agent> {
    store::list()
}

/// An agent and its log.
#[tauri::command]
pub async fn agent_get(id: String) -> Result<AgentWithLog, String> {
    let agent = store::get(&id).ok_or("There's no such agent.")?;
    let log = tauri::async_runtime::spawn_blocking(move || store::log_read(&id)).await.unwrap_or_default();
    Ok(AgentWithLog { agent, log })
}

/// Stop an agent (a queued one comes off the queue).
#[tauri::command]
pub async fn agent_stop(id: String) -> Result<Agent, String> {
    runner::stop(&id)
}

/// Continue a stopped, interrupted or failed agent.
#[tauri::command]
pub async fn agent_continue(id: String) -> Result<Agent, String> {
    runner::continue_agent(&id)
}

/// A message for an agent, typed in its tab: to it at once while it works,
/// else its next turn. "Go ahead" after a plan carries it out.
#[tauri::command]
pub async fn agent_message(id: String, text: String) -> Result<Agent, String> {
    let go = consent::go_ahead(&text);
    runner::message(&id, &text, go)
}

/// **Go ahead with an agent's plan**: the report card's button (the person's
/// click is their consent): its next turn now, Yap's own go-ahead at the
/// chat's Mode. Once: refused once the plan isn't waiting any more.
#[tauri::command]
pub async fn agent_go_ahead(id: String) -> Result<Agent, String> {
    runner::go_ahead(&id)
}

/// Undo an agent (Undo all: every turn, back to before its first): what Undo
/// put back (`restored`, `deleted`, `skipped`, `refused`, `failed`: paths
/// and reasons).
#[tauri::command]
pub async fn agent_undo(id: String) -> Result<crate::integrations::claude_live::backup::RestoreReport, String> {
    tauri::async_runtime::spawn_blocking(move || runner::undo(&id)).await.map_err(|e| e.to_string())?
}

/// Undo one turn of an agent (Undo this turn): refused while a later turn
/// changed one of the same files ("Undo the later change first…").
#[tauri::command]
pub async fn agent_undo_turn(id: String, turn: u32) -> Result<crate::integrations::claude_live::backup::RestoreReport, String> {
    tauri::async_runtime::spawn_blocking(move || runner::undo_turn(&id, turn)).await.map_err(|e| e.to_string())?
}

/// The agent's session in Windows Terminal, in its folder (`claude
/// --resume`), once it isn't running.
#[tauri::command]
pub async fn agent_open_terminal(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || runner::open_terminal(&id)).await.map_err(|e| e.to_string())?
}

/// The folders agents may work in.
#[tauri::command]
pub async fn workspaces_list() -> Vec<Folder> {
    workspaces::list()
}

/// Add a folder (Settings → Yap Code' Add folder…): its path, an optional name
/// and cap.
#[tauri::command]
pub async fn workspaces_add(path: String, name: Option<String>, cap: Option<String>) -> Result<Folder, String> {
    let added = workspaces::add(&path, name.as_deref(), cap.as_deref())?;
    workspaces::saved().map_err(|e| e.to_string())?;
    code_chat::workspaces_changed();
    Ok(added)
}

/// **Add folder…** (Settings → Yap Code): Windows' folder picker, opened here
/// (as the Dictionary's files are: the page never names the folder), and
/// the folder picked added. `None` when the person cancelled. A test run
/// never opens a dialog: `e2e_workspace` names the folder the next pick
/// takes.
#[tauri::command]
pub async fn workspaces_pick(app: tauri::AppHandle, window: tauri::WebviewWindow) -> Result<Option<Folder>, String> {
    let Some(path) = pick_folder(&app, window).await? else { return Ok(None) };
    let path = path.to_string_lossy().into_owned();
    let added = workspaces::add(&path, None, None)?;
    workspaces::saved().map_err(|e| e.to_string())?;
    tracing::info!("Agents: a folder picked and added");
    code_chat::workspaces_changed();
    Ok(Some(added))
}

/// The folder the person picked (a test run: the one it named).
async fn pick_folder(app: &tauri::AppHandle, window: tauri::WebviewWindow) -> Result<Option<std::path::PathBuf>, String> {
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() {
        let _ = (app, window);
        return E2E_FOLDER
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
            .map(Some)
            .ok_or_else(|| "A test run names the folder first (e2e_workspace).".to_string());
    }
    use tauri_plugin_dialog::DialogExt;
    let dialog = app.dialog().file().set_parent(&window).set_title(format!("Choose a folder for {}", super::FEATURE));
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_folder()).await.map_err(|e| e.to_string())?;
    Ok(picked.and_then(|p| p.into_path().ok()))
}

/// Test mode: the folder the next Add folder… takes, in place of the dialog.
#[cfg(debug_assertions)]
static E2E_FOLDER: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);

/// Test mode: name the folder the next Add folder… picks.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_workspace(path: String) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("test mode only".into());
    }
    *E2E_FOLDER.lock().unwrap_or_else(|p| p.into_inner()) = Some(std::path::PathBuf::from(path));
    Ok(())
}

/// Remove a folder (by its name or path): whether one went. Its agents stay;
/// its Code chats are plain chats again, said once (`yap-code-chat`).
#[tauri::command]
pub async fn workspaces_remove(name: String) -> bool {
    let folder = code_chat::find(&name);
    let gone = workspaces::remove(&name);
    if let (true, Some(f)) = (gone, folder) {
        code_chat::removed(&f);
    }
    gone
}

/// Cap a folder's mode (`None`: no cap).
#[tauri::command]
pub async fn workspaces_set_cap(name: String, cap: Option<String>) -> Result<Folder, String> {
    let capped = workspaces::set_cap(&name, cap.as_deref())?;
    code_chat::workspaces_changed();
    Ok(capped)
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
pub async fn agent_mode_get(conversation: Option<u64>) -> ModeView {
    let default = crate::config::load().agents_mode;
    mode_view(modes::or_auto(&store::mode_of(conversation, &default)))
}

/// Set a chat's Mode: from the chip, the person's own choice (Bypass only
/// once its warning was confirmed).
#[tauri::command]
pub async fn agent_mode_set(conversation: u64, mode: String) -> Result<ModeView, String> {
    let m = Mode::parse(&mode).ok_or("That isn't one of the modes.")?;
    if m == Mode::Bypass && !store::bypass_confirmed() {
        return Err("Confirm Bypass permissions first.".into());
    }
    store::set_mode(conversation, m.as_str());
    tracing::info!(mode = m.as_str(), "Agents: a chat's Mode set");
    Ok(mode_view(m))
}

/// The person confirmed Bypass permissions' warning, once.
#[tauri::command]
pub async fn agent_bypass_confirmed() {
    store::confirm_bypass();
    tracing::info!("Agents: Bypass permissions confirmed");
}

/// A chat's workspace: the workspace it's a Code chat in (`None`: a plain
/// chat).
#[tauri::command]
pub async fn code_chat_workspace_get(conversation: u64) -> Option<Folder> {
    code_chat::folder_of(Some(conversation))
}

/// Make a chat a Code chat in `folder` (a workspace's name or path), or a
/// plain chat again (`None`): the workspace chip's pick. Every window hears
/// (`yap-code-chat`).
#[tauri::command]
pub async fn code_chat_workspace_set(conversation: u64, folder: Option<String>) -> Result<Option<Folder>, String> {
    let found = match folder.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
        Some(f) => Some(code_chat::find(f).ok_or("That isn't one of your workspaces.")?),
        None => None,
    };
    code_chat::set(conversation, found.as_ref());
    store::saved().map_err(|e| e.to_string())?;
    Ok(found)
}

/// Chat's Code side: every Code chat with its folder, and the workspaces,
/// most recently used first.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeChats {
    /// Conversation id → its folder.
    pub chats: std::collections::BTreeMap<u64, Folder>,
    pub workspaces: Vec<code_chat::Workspace>,
}

#[tauri::command]
pub async fn code_chats_list() -> CodeChats {
    let (chats, workspaces) = code_chat::listing();
    CodeChats { chats, workspaces }
}

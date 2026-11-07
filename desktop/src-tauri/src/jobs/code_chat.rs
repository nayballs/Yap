//! **Code chats** (2026-11-07): a Chat conversation is a plain chat or a
//! **Code chat**, attached to one of the job folders (a "workspace" in the
//! UI: Chat's Code side lists Code chats grouped by workspace). Kept in
//! `jobs.json`'s `codeChats` map beside the chat's Mode (`store.rs`), never
//! in chats.json: an older Yap sharing the data folder drops fields it
//! doesn't know when it saves.
//!
//! - **What it changes**: in a Code chat, `start_job` and `message_job` may
//!   leave the folder out (it's the chat's), and the person's own request
//!   this turn is enough to start a job there (`consent::start_in`); the
//!   chat's prompt says so ([`prompt_line_for`]). Everything else is as in any
//!   chat: asking first, the refusal after web content or a job's output,
//!   the Mode only lowered, a folder's cap, Bypass's OK.
//! - **A plain chat becomes one** ([`became`]) once a job starts from it in a
//!   folder the person named (typed, or by voice): the next request needn't
//!   name it.
//! - **A new chat** made a Code chat before its first question: the page
//!   sends the workspace with the question (`chat_send`'s `codeFolder`), and
//!   the conversation it makes is attached as it's made, before the prompt
//!   is built ([`with_new`], [`adopt_new`]). Asked out loud instead (the
//!   Voice Assistant, with Chat showing that new chat), the same: Chat says
//!   which workspace it shows (`chat_view_state`'s `codeFolder`), and the
//!   question's new conversation is attached there
//!   (`assistant::new_code_chat`).
//! - **A folder removed** (Settings → Jobs): its chats are plain again,
//!   said once ([`removed`]). A chat deleted is no Code chat ([`chat_deleted`]).
//! - **No Yap memory in a Code chat** (7 Nov 2026): project knowledge lives
//!   with the project (its CLAUDE.md, Claude Code's own memory), and Yap's
//!   memory is personal. A Code chat's turn has none of memory's passages,
//!   its "You" block or its tools, About you without its note, nothing
//!   found by itself (the eager RAG, past chats), and it's never learned
//!   from (`chat/turn/prompt.rs`, `memory/learn.rs`). On the Claude plan its
//!   session runs in the workspace, read only (`chat/turn/plan.rs`).
//! - Every change goes to every window as **`yap-code-chat`**.
//!
//! The log has ids and counts, never a folder's name or path.

use std::collections::BTreeMap;

use serde::Serialize;

use super::folders::{self, Folder};
use super::store;

/// Every window hears of a chat's folder changing.
pub const EVENT: &str = "yap-code-chat";

/// What `yap-code-chat` says.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Changed {
    pub conversations: Vec<u64>,
    /// The folder they're Code chats in now (`None`: plain chats).
    pub folder: Option<Folder>,
    /// A folder removed in Settings → Jobs: its name, said once.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed: Option<String>,
}

fn emit(ev: &Changed) {
    if let Some(app) = super::runner::app() {
        use tauri::Emitter;
        let _ = app.emit(EVENT, ev);
    }
}

/// The job folder at `path`, while it is one.
fn folder_at(path: &str) -> Option<Folder> {
    let want = folders::norm(path);
    folders::list().into_iter().find(|f| folders::norm(&f.path) == want)
}

/// **The folder `conversation` is a Code chat in**: one of the job folders
/// still (a folder that no longer is one: a plain chat).
pub fn folder_of(conversation: Option<u64>) -> Option<Folder> {
    folder_at(&store::code_chat_of(conversation?)?.path)
}

/// One of the job folders, by its path or its name (`folders::answers_to`,
/// when only one answers).
pub fn find(arg: &str) -> Option<Folder> {
    let arg = arg.trim();
    if arg.is_empty() {
        return None;
    }
    let all = folders::list();
    if let Some(f) = all.iter().find(|f| folders::norm(&f.path) == folders::norm(arg) || f.name.eq_ignore_ascii_case(arg)) {
        return Some(f.clone());
    }
    match all.iter().filter(|f| folders::answers_to(f, arg)).collect::<Vec<_>>().as_slice() {
        [one] => Some((*one).clone()),
        _ => None,
    }
}

/// **Attach `conversation` to `folder`** (`None`: a plain chat again), every
/// window told. Whether anything changed.
pub fn set(conversation: u64, folder: Option<&Folder>) -> bool {
    let changed = store::set_code_chat(conversation, folder.map(|f| f.path.as_str()));
    if changed {
        tracing::info!(conversation, code = folder.is_some(), "Jobs: a chat's workspace set");
        emit(&Changed { conversations: vec![conversation], folder: folder.cloned(), removed: None });
    }
    changed
}

/// **A job started from `conversation`** in the folder at `path`: a plain
/// chat becomes a Code chat there (one already in a folder keeps its own).
pub fn became(conversation: u64, path: &str) {
    if folder_of(Some(conversation)).is_some() {
        return;
    }
    if let Some(f) = folder_at(path) {
        set(conversation, Some(&f));
    }
}

/// **A conversation deleted** (Chat's delete): it's no Code chat any more.
/// A later conversation may get its id (ids are the highest plus one), and
/// it mustn't start as one: a Code chat keeps Yap's memory out. Nor may it
/// carry on the deleted chat's job (one job per Code chat), nor take its
/// reports: its jobs keep no chat (`store::detach`).
pub fn chat_deleted(conversation: u64) {
    set(conversation, None);
    let detached = store::detach(conversation);
    if detached > 0 {
        tracing::info!(jobs = detached, "Jobs: a chat deleted, its jobs keep no chat");
    }
}

/// **A folder removed** (Settings → Jobs): its chats are plain again, every
/// window told once, with its name. How many.
pub fn removed(folder: &Folder) -> usize {
    let gone = store::drop_code_chats_in(&folder.path);
    if !gone.is_empty() {
        tracing::info!(chats = gone.len(), "Jobs: a workspace removed, its Code chats plain again");
    }
    // Always told, Code chats or none: a new code chat waiting with this
    // workspace picked lets it go, and every picker drops it.
    emit(&Changed { conversations: gone.clone(), folder: None, removed: Some(folder.name.clone()) });
    gone.len()
}

/// **The workspaces changed** (one added, picked or capped in Settings →
/// Jobs or Chat's picker): every window reads them again (`yap-code-chat`
/// naming no conversation).
pub fn workspaces_changed() {
    emit(&Changed::default());
}

/// **A Code chat's line** in its prompt: the question's context, for Yap's
/// tool loop and a plan chat alike (`chat/turn/prompt.rs`). Pure.
pub fn prompt_line_for(folder: &Folder) -> String {
    format!(
        "This is a Code chat in the user's folder {} ({}): they picked it for jobs. When they ask you to change, fix, build or write something there, start a job: call start_job without a folder (it's this one). Their own request this turn is enough; they needn't name the folder. One Claude Code session per Code chat: once it has a job, each follow-up continues that job as its next turn (start_job does it by itself; fresh: true only when they ask for a new job or a fresh start). message_job and job_status without an id mean this chat's latest job. A job in any other folder still needs the user to name it.",
        folder.name, folder.path
    )
}

/// **The chat's job, as its prompt says it** (one job per Code chat): the
/// conversation's job in `folder`, its status and how many turns, so the
/// model knows a follow-up carries it on (`chat/turn/prompt.rs`, after
/// [`prompt_line_for`]). None while the chat has none there.
pub fn job_line_in(conversation: Option<u64>, folder: &Folder) -> Option<String> {
    let job = store::of_conversation(conversation?).into_iter().find(|j| folders::norm(&j.path) == folders::norm(&folder.path))?;
    Some(job_line(&job))
}

/// **The chat's job, as its prompt says it**: its status and how many turns,
/// so the model knows a follow-up carries it on. Never its words. Pure.
pub fn job_line(job: &store::Job) -> String {
    let turns = job.turn_notes().len();
    let turns = if turns == 1 { "1 turn".to_string() } else { format!("{turns} turns") };
    let now = match job.status {
        store::Status::Queued | store::Status::Running | store::Status::Asking => "working now (a request waits for its next turn)",
        store::Status::Done => "done",
        store::Status::Failed => "stopped short",
        store::Status::Stopped => "stopped",
        store::Status::Interrupted => "interrupted",
    };
    format!("This chat's job there (id {}) is {now}, after {turns}: the user's next request there continues it.", job.id)
}

/// A job folder with when it was last used: the workspace picker's order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    #[serde(flatten)]
    pub folder: Folder,
    /// Unix seconds: a Code chat attached there, a job started there, or
    /// when it was added, whichever is latest.
    pub used_ts: u64,
}

/// **The job folders, most recently used first** (`chats`: the Code chats,
/// `jobs`: every job's folder and when it was made). Pure.
pub fn by_use(all: Vec<Folder>, chats: &BTreeMap<u64, store::CodeChat>, jobs: &[(String, u64)]) -> Vec<Workspace> {
    let mut out: Vec<Workspace> = all
        .into_iter()
        .map(|f| {
            let at = folders::norm(&f.path);
            let chat = chats.values().filter(|c| folders::norm(&c.path) == at).map(|c| c.ts).max().unwrap_or(0);
            let job = jobs.iter().filter(|(p, _)| folders::norm(p) == at).map(|(_, ts)| *ts).max().unwrap_or(0);
            let used_ts = f.added_ts.max(chat).max(job);
            Workspace { folder: f, used_ts }
        })
        .collect();
    out.sort_by(|a, b| b.used_ts.cmp(&a.used_ts).then_with(|| a.folder.name.to_lowercase().cmp(&b.folder.name.to_lowercase())));
    out
}

/// Every Code chat whose folder is still a job folder (conversation id →
/// its folder), and the workspaces by use.
pub fn listing() -> (BTreeMap<u64, Folder>, Vec<Workspace>) {
    let all = folders::list();
    let chats = store::code_chats();
    let shown = chats
        .iter()
        .filter_map(|(id, c)| all.iter().find(|f| folders::norm(&f.path) == folders::norm(&c.path)).map(|f| (*id, f.clone())))
        .collect();
    let jobs: Vec<(String, u64)> = store::list().into_iter().map(|j| (j.path, j.created_ts)).collect();
    (shown, by_use(all, &chats, &jobs))
}

// ---- a new chat's first question ----

tokio::task_local! {
    /// The workspace the page picked for a new chat (its name or path).
    static NEW_CHAT: Option<String>;
}

/// `f` run as a new chat's first question, made a Code chat in `folder`
/// (`None`: a plain one) by the page.
pub async fn with_new<F: std::future::Future>(folder: Option<String>, f: F) -> F::Output {
    NEW_CHAT.scope(folder, f).await
}

/// **The conversation a new chat's first question made** (`chat_send`'s
/// turn, or the Voice Assistant's, as it's made): attached to the workspace
/// the page picked for it. That workspace, when it was attached.
pub fn adopt_new(conversation: u64) -> Option<Folder> {
    let wanted = NEW_CHAT.try_with(|f| f.clone()).ok().flatten();
    let folder = wanted.as_deref().and_then(find)?;
    set(conversation, Some(&folder));
    Some(folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str, added: u64) -> Folder {
        Folder { name: name.into(), path: format!(r"D:\Projects\{name}"), cap: None, added_ts: added }
    }

    fn make(dir: &std::path::Path) -> Folder {
        std::fs::create_dir_all(dir).unwrap();
        folders::add(&dir.to_string_lossy(), None, None).unwrap()
    }

    #[test]
    fn its_prompt_names_the_chats_job_which_a_follow_up_continues() {
        let garden = folder("garden-planner", 1);
        assert_eq!(job_line_in(Some(61), &garden), None, "no job yet: nothing to say");
        store::insert(store::Job { id: "ff66".into(), path: garden.path.clone(), conversation: Some(61), status: store::Status::Done, ..Default::default() });
        store::insert(store::Job { id: "gg77".into(), path: r"D:\Projects\recipe-box".into(), conversation: Some(61), status: store::Status::Running, created_ts: 9, ..Default::default() });
        // This chat's job in that workspace, never another's.
        assert_eq!(job_line_in(Some(61), &garden).unwrap(), "This chat's job there (id ff66) is done, after 1 turn: the user's next request there continues it.");
        assert_eq!(job_line_in(Some(62), &garden), None);
        let two = store::Job {
            status: store::Status::Running,
            turn_list: vec![store::TurnNote { n: 1, ..Default::default() }, store::TurnNote { n: 2, ..Default::default() }],
            ..store::get("gg77").unwrap()
        };
        assert!(job_line(&two).contains("is working now (a request waits for its next turn), after 2 turns"));
        assert!(prompt_line_for(&garden).contains("each follow-up continues that job as its next turn"));
    }

    #[test]
    fn the_workspaces_most_recently_used_first() {
        let all = vec![folder("garden-planner", 10), folder("recipe-box", 20), folder("book-shelf", 5)];
        let mut chats = BTreeMap::new();
        chats.insert(1, store::CodeChat { path: r"d:/projects/garden-planner".into(), ts: 50 });
        let jobs = vec![(r"D:\Projects\book-shelf".to_string(), 40)];
        let order: Vec<String> = by_use(all.clone(), &chats, &jobs).into_iter().map(|w| w.folder.name).collect();
        assert_eq!(order, ["garden-planner", "book-shelf", "recipe-box"]);
        // Never used: as added, newest first.
        let order: Vec<u64> = by_use(all, &BTreeMap::new(), &[]).into_iter().map(|w| w.used_ts).collect();
        assert_eq!(order, [20, 10, 5]);
        // The page reads it flat: the folder's fields and when it was used.
        let v = serde_json::to_value(&by_use(vec![folder("garden-planner", 10)], &BTreeMap::new(), &[])[0]).unwrap();
        assert_eq!((v["name"].as_str(), v["usedTs"].as_u64()), (Some("garden-planner"), Some(10)));
    }

    #[test]
    fn a_code_chat_lives_while_its_folder_is_a_job_folder() {
        let base = std::env::temp_dir().join(format!("yap-code-chat-{}", store::new_id()));
        let garden = make(&base.join("garden-planner"));
        let recipes = make(&base.join("recipe-box"));
        assert_eq!(folder_of(Some(5)), None, "a plain chat");
        assert!(set(5, Some(&garden)));
        assert_eq!(folder_of(Some(5)), Some(garden.clone()));
        assert!(prompt_line_for(&folder_of(Some(5)).unwrap()).starts_with(&format!("This is a Code chat in the user's folder garden-planner ({})", garden.path)));
        // Found by name or path.
        assert_eq!(find("Garden-Planner"), Some(garden.clone()));
        assert_eq!(find(&recipes.path), Some(recipes.clone()));
        assert_eq!(find("nowhere"), None);
        assert_eq!(find(""), None);
        // A job started from a plain chat in a folder: a Code chat there now;
        // one already in a folder keeps its own.
        became(6, &recipes.path);
        assert_eq!(folder_of(Some(6)), Some(recipes.clone()));
        became(5, &recipes.path);
        assert_eq!(folder_of(Some(5)), Some(garden.clone()));
        // Only job folders: a path that isn't one changes nothing.
        became(7, r"C:\Elsewhere");
        assert_eq!(folder_of(Some(7)), None);
        // The listing shows each Code chat with its folder.
        let (chats, workspaces) = listing();
        assert_eq!(chats.get(&5).map(|f| f.name.as_str()), Some("garden-planner"));
        assert_eq!(workspaces.len(), 2);
        // Removed in Settings: its chats are plain again.
        assert_eq!(removed(&garden), 1);
        assert_eq!(folder_of(Some(5)), None);
        assert_eq!(folder_of(Some(6)), Some(recipes.clone()));
        // A chat deleted: no Code chat (a later one may get its id).
        set(8, Some(&recipes));
        store::insert(store::Job {
            id: "hh88".into(),
            path: recipes.path.clone(),
            conversation: Some(8),
            message_id: Some("chat:8:0".into()),
            report_message_id: Some("chat:8:2".into()),
            turn_list: vec![store::TurnNote { n: 1, message_id: Some("chat:8:0".into()), report_message_id: Some("chat:8:2".into()), ..Default::default() }],
            ..Default::default()
        });
        chat_deleted(8);
        assert_eq!(folder_of(Some(8)), None);
        // …nor carry on its job, nor take its reports: the job keeps no chat.
        assert_eq!(job_line_in(Some(8), &recipes), None);
        let kept = store::get("hh88").unwrap();
        assert_eq!((kept.conversation, kept.message_id, kept.report_message_id), (None, None, None));
        assert_eq!((kept.turn_list[0].message_id.clone(), kept.turn_list[0].report_message_id.clone()), (None, None));
        // A folder no longer a job folder (removed some other way): plain.
        assert!(folders::remove(&recipes.name));
        assert_eq!(folder_of(Some(6)), None);
        assert!(listing().0.is_empty());
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_new_chats_first_question_takes_the_workspace_the_page_picked() {
        let base = std::env::temp_dir().join(format!("yap-code-chat-new-{}", store::new_id()));
        let garden = make(&base.join("garden-planner"));
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        // Outside a new chat's first question (a phone's, one continuing a
        // conversation): nothing.
        assert_eq!(adopt_new(20), None);
        assert_eq!(folder_of(Some(20)), None);
        assert_eq!(rt.block_on(with_new(None, async { adopt_new(21) })), None);
        assert_eq!(folder_of(Some(21)), None);
        assert_eq!(rt.block_on(with_new(Some("garden-planner".into()), async { adopt_new(22) })), Some(garden.clone()));
        assert_eq!(folder_of(Some(22)), Some(garden.clone()));
        assert_eq!(rt.block_on(with_new(Some("not-a-workspace".into()), async { adopt_new(23) })), None);
        assert_eq!(folder_of(Some(23)), None);
        let _ = std::fs::remove_dir_all(base);
    }
}

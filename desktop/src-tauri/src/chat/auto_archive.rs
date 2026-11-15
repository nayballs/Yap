//! **Auto-archive** (15 Nov 2026, the Claude app's auto-archive): with
//! "Archive chats you haven't used in" set (Settings → Privacy & data,
//! [`YapConfig::chat_archive_days`]: 7, 30 or 90 days; 0 = never, the
//! default), a chat whose last message is that old moves to Archived chats
//! by itself — Chat and Code chats alike, a project's chat keeping its
//! project (`projects.json` is never touched). Checked a little after Yap
//! starts and once a day ([`init`]), and at once when the setting is set or
//! changed ([`on_config_saved`]). Each run is one [`chats::set_archived`]
//! write; a message in an archived chat brings it back, as ever.
//!
//! **Never archived** ([`at_work`], [`due`]): a chat at work — a turn
//! answering it, an agent working or queued in it (a Code chat's own Claude
//! is one), a ▶ Run running, a message waiting in the page's queue — the
//! chat the Chat view shows, an incognito chat, a meeting's Ask thread, or
//! one archived already. **"Used"** is the chat's last message
//! (`updated_ts`): Yap keeps no time a chat was opened. Chats can't be
//! pinned, so nothing more to keep.
//!
//! **What it says**: a run that archived something says so once, quietly —
//! a toast in Yap's own window while the person is in it (the notice
//! policy's "window" place, `notices/policy.rs`), never a card on the Yap
//! bar nor a Windows notification: it waits ([`UNSAID`]) until the window
//! is next in front ([`on_main_window_focused`]). "Archived 3 chats you
//! hadn't used in 30 days", with Undo ([`chats_auto_archive_undo`];
//! `notices/chatsArchived.js`). Runs not said yet are said as one.
//!
//! The rule ([`due`]) is pure and unit-tested with the clock passed in.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::chat::chats::{self, Conversation};
use crate::config::YapConfig;

/// A day, in seconds.
pub const DAY_SECS: u64 = 24 * 60 * 60;

/// To the main window: a run archived chats, said now (`{ ids, days }`).
pub const NOTICE_EVENT: &str = "yap-chats-auto-archived";
/// To every window: chats moved in or out of Archived by auto-archive or its
/// Undo (`{ ids, archived }`): Chat's list and its Archived look again.
pub const CHANGED_EVENT: &str = "yap-chats-archived";

/// How long after start the first run waits (a test run: at once, nearly).
const START_DELAY_SECS: u64 = 20;
const START_DELAY_E2E_SECS: u64 = 1;
/// How often the daily run looks at the clock (a PC asleep doesn't count
/// towards a thread's sleep: the wall clock says when a day has passed).
const LOOK_SECS: u64 = 60 * 60;

/// What a run archived, to be said: the chats, and the setting's days.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Archived {
    pub ids: Vec<u64>,
    pub days: u32,
}

impl Archived {
    /// A later run's chats said with this one's (its days the latest).
    fn merge(&mut self, more: Archived) {
        for id in more.ids {
            if !self.ids.contains(&id) {
                self.ids.push(id);
            }
        }
        self.days = more.days;
    }
}

/// The runs not said yet (the window wasn't in front).
static UNSAID: Mutex<Option<Archived>> = Mutex::new(None);
/// The conversations with a message waiting in the page's queue (Chat's
/// `chatState`: sent when its Claude is free), as the page last said.
static QUEUED: Mutex<Vec<u64>> = Mutex::new(Vec::new());
/// When the last run was (Unix seconds).
static LAST_RUN: AtomicU64 = AtomicU64::new(0);

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// **Which chats a run archives** (`days`: the setting; 0 archives none):
/// those whose last message is at least `days` days before `now`, that
/// aren't archived already, incognito, a meeting's Ask thread, or in
/// `keep` (at work, or shown). A chat with no time at all is left alone.
/// Pure.
pub fn due(all: &[Conversation], now: u64, days: u32, keep: &HashSet<u64>) -> Vec<u64> {
    if days == 0 {
        return Vec::new();
    }
    let cutoff = now.saturating_sub(u64::from(days) * DAY_SECS);
    all.iter()
        .filter(|c| {
            let used = c.updated_ts.max(c.created_ts);
            c.note.is_none() && c.archived_ts == 0 && !c.incognito && !keep.contains(&c.id) && used > 0 && used <= cutoff
        })
        .map(|c| c.id)
        .collect()
}

/// **The chats never archived now, whatever their age**: a turn answering,
/// an agent at work or queued, a ▶ Run running, a message in the page's
/// queue, and the chat the Chat view shows.
pub fn at_work() -> HashSet<u64> {
    let mut keep: HashSet<u64> = crate::chat::stream::answering_ids().into_iter().collect();
    keep.extend(crate::agents::runner::working_chats());
    keep.extend(crate::agents::run_block::running_chats());
    keep.extend(QUEUED.lock().unwrap_or_else(|p| p.into_inner()).iter().copied());
    keep.extend(crate::chat::assistant::shown_conversation());
    keep
}

/// **One run** at `now`: what's due archived in one write. The chats
/// archived (none: nothing written).
pub fn archive_due(days: u32, now: u64, keep: &HashSet<u64>) -> Vec<u64> {
    let ids = chats::with_all(|all| due(all, now, days, keep));
    if ids.is_empty() || chats::set_archived(&ids, true) == 0 {
        return Vec::new();
    }
    ids
}

/// **A run now**, by the setting: archives what's due, tells the pages,
/// and says it once the person is in Yap's window. The chats archived.
pub fn run(app: &AppHandle) -> Vec<u64> {
    let days = crate::config::load().chat_archive_days;
    LAST_RUN.store(now_secs(), Ordering::SeqCst);
    if days == 0 {
        return Vec::new();
    }
    let ids = archive_due(days, now_secs(), &at_work());
    if ids.is_empty() {
        return ids;
    }
    if let Err(e) = chats::saved() {
        // Kept in memory and saved with the next change (shell/saving.rs says it).
        tracing::warn!(error = %e, "Chat: auto-archive couldn't save yet");
    }
    tracing::info!(count = ids.len(), days, "Chat: archived chats not used in a while");
    let _ = app.emit(CHANGED_EVENT, serde_json::json!({ "ids": ids, "archived": true }));
    {
        let mut unsaid = UNSAID.lock().unwrap_or_else(|p| p.into_inner());
        let this = Archived { ids: ids.clone(), days };
        match unsaid.as_mut() {
            Some(waiting) => waiting.merge(this),
            None => *unsaid = Some(this),
        }
    }
    tell(app);
    ids
}

/// Say what's unsaid, if the person is in Yap's window now (the notice
/// policy's one place: never the bar, never a Windows notification). Kept
/// until the page says it showed it ([`chats_auto_archive_said`]).
fn tell(app: &AppHandle) {
    // Window getters first: never with a lock held.
    if !crate::notices::policy::window_view(app).1 {
        return;
    }
    let waiting = UNSAID.lock().unwrap_or_else(|p| p.into_inner()).clone();
    if let Some(a) = waiting {
        let _ = app.emit(NOTICE_EVENT, &a);
    }
}

/// The main window came to the front: what a run archived while the person
/// was away is said now (off the main thread: [`tell`] asks the window).
pub fn on_main_window_focused(app: &AppHandle) {
    if UNSAID.lock().unwrap_or_else(|p| p.into_inner()).is_none() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move { tell(&app) });
}

/// A run a little after start, then once a day (by the wall clock).
pub fn init(app: &AppHandle) {
    let app = app.clone();
    std::thread::Builder::new()
        .name("chat-auto-archive".into())
        .spawn(move || {
            let first = if crate::shell::e2e::active() { START_DELAY_E2E_SECS } else { START_DELAY_SECS };
            std::thread::sleep(std::time::Duration::from_secs(first));
            run(&app);
            loop {
                std::thread::sleep(std::time::Duration::from_secs(LOOK_SECS));
                if now_secs().saturating_sub(LAST_RUN.load(Ordering::SeqCst)) >= DAY_SECS {
                    run(&app);
                }
            }
        })
        .ok();
}

/// Settings saved: the setting set or changed runs it now (as "Keep chat
/// screenshots" shortened sweeps at once).
pub fn on_config_saved(app: &AppHandle, prev: &YapConfig, cfg: &YapConfig) {
    if cfg.chat_archive_days != prev.chat_archive_days && cfg.chat_archive_days > 0 {
        let app = app.clone();
        std::thread::spawn(move || run(&app));
    }
}

// ---- commands -------------------------------------------------------------------------

/// The page's queue (Chat's `chatState`): the conversations with a message
/// waiting to be sent. Said each time it changes, and `[]` as Chat closes.
#[tauri::command]
pub fn chat_queued(conversations: Vec<u64>) {
    *QUEUED.lock().unwrap_or_else(|p| p.into_inner()) = conversations;
}

/// What a run archived that the main window hasn't said yet (it asks as it
/// starts listening; `None` while the person isn't in it).
#[tauri::command]
pub async fn chats_auto_archive_unsaid(app: AppHandle) -> Option<Archived> {
    if !crate::notices::policy::window_view(&app).1 {
        return None;
    }
    UNSAID.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

/// The main window showed the notice for `ids`: they're said.
#[tauri::command]
pub fn chats_auto_archive_said(ids: Vec<u64>) {
    let mut unsaid = UNSAID.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(a) = unsaid.as_mut() {
        a.ids.retain(|id| !ids.contains(id));
        if a.ids.is_empty() {
            *unsaid = None;
        }
    }
}

/// The notice's Undo: the chats it archived back in the list, in one
/// write. How many came back.
#[tauri::command]
pub async fn chats_auto_archive_undo(app: AppHandle, ids: Vec<u64>) -> Result<usize, String> {
    chats_auto_archive_said(ids.clone());
    let back = chats::set_archived(&ids, false);
    chats::saved()?;
    if back > 0 {
        let _ = app.emit(CHANGED_EVENT, serde_json::json!({ "ids": ids, "archived": false }));
    }
    Ok(back)
}

/// Test mode only: a run now, as the daily one runs (the chats archived).
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn e2e_chat_auto_archive(app: AppHandle) -> Result<Vec<u64>, String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    Ok(run(&app))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000;

    fn conv(id: u64, used: u64) -> Conversation {
        Conversation {
            id,
            title: format!("Chat {id}"),
            created_ts: used,
            updated_ts: used,
            messages: Vec::new(),
            scope: String::new(),
            note: None,
            archived_ts: 0,
            summary: None,
            closed_tabs: Vec::new(),
            branches: Vec::new(),
            forks: Vec::new(),
            incognito: false,
        }
    }

    #[test]
    fn a_chat_not_used_in_the_days_set_is_due_and_a_newer_one_is_not() {
        let none = HashSet::new();
        let all = [
            conv(1, NOW - 31 * DAY_SECS),
            conv(2, NOW - 29 * DAY_SECS),
            conv(3, NOW - 30 * DAY_SECS),
            conv(4, NOW - 30 * DAY_SECS + 1),
            conv(5, NOW),
        ];
        assert_eq!(due(&all, NOW, 30, &none), [1, 3], "30 days or more, to the second");
        assert_eq!(due(&all, NOW, 7, &none), [1, 2, 3, 4]);
        assert_eq!(due(&all, NOW, 90, &none), Vec::<u64>::new());
        // Off: nothing, however old.
        assert_eq!(due(&all, NOW, 0, &none), Vec::<u64>::new());
        // A clock set back (a chat "used" after now) never counts as old.
        assert_eq!(due(&[conv(6, NOW + DAY_SECS)], NOW, 7, &none), Vec::<u64>::new());
        // A clock near the epoch: nothing is that old.
        assert_eq!(due(&[conv(7, 5)], 6 * DAY_SECS, 7, &none), Vec::<u64>::new());
    }

    #[test]
    fn used_is_the_last_message_never_older_than_its_start() {
        let none = HashSet::new();
        // Its last message 2 days ago, started 60: in use.
        let mut c = conv(1, NOW - 2 * DAY_SECS);
        c.created_ts = NOW - 60 * DAY_SECS;
        assert_eq!(due(&[c.clone()], NOW, 30, &none), Vec::<u64>::new());
        // No time at all (a hand-made file): left alone.
        assert_eq!(due(&[conv(2, 0)], NOW, 7, &none), Vec::<u64>::new());
    }

    #[test]
    fn never_a_chat_at_work_shown_incognito_a_meetings_or_archived_already() {
        let old = NOW - 100 * DAY_SECS;
        let mut incognito = conv(2, old);
        incognito.incognito = true;
        let mut meeting = conv(3, old);
        meeting.note = Some(41);
        let mut archived = conv(4, old);
        archived.archived_ts = old + 10;
        let working = conv(5, old);
        let code = conv(6, old);
        let all = [conv(1, old), incognito, meeting, archived, working, code];
        let keep: HashSet<u64> = [5].into_iter().collect();
        assert_eq!(due(&all, NOW, 30, &keep), [1, 6], "a Code chat is a chat like any other");
    }

    #[test]
    fn a_run_archives_whats_due_in_one_write_its_project_kept_and_a_message_brings_it_back() {
        let old = chats::create("Bread");
        let busy = chats::create("Build scripts");
        let fresh = chats::create("Holiday");
        let meeting = chats::create_for_note(77, "Ask");
        for id in [old.id, busy.id, meeting.id] {
            chats::backdate(id, NOW - 40 * DAY_SECS);
        }
        chats::backdate(fresh.id, NOW - 2 * DAY_SECS);
        // A project chat: archived like any other, its project kept.
        let project = crate::chat::projects::project_create("Garden".into(), None).expect("a project");
        crate::chat::projects::chat_project_set(old.id, Some(project.id.clone())).expect("moved in");

        let keep: HashSet<u64> = [busy.id].into_iter().collect();
        assert_eq!(archive_due(30, NOW, &keep), [old.id]);
        let titles = |v: serde_json::Value| v.as_array().unwrap().iter().map(|c| c["title"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(titles(chats::list_archived()), ["Bread"]);
        assert_eq!(titles(chats::list()), ["Holiday", "Build scripts"]);
        assert_eq!(crate::chat::projects::of_conversation(Some(old.id)).map(|p| p.id), Some(project.id.clone()), "its project kept");
        assert!(chats::get(meeting.id).unwrap().archived_ts == 0, "a meeting's Ask thread is the meeting's");
        // Nothing more due: nothing written.
        assert_eq!(archive_due(30, NOW, &keep), Vec::<u64>::new());
        // Off: nothing.
        assert_eq!(archive_due(0, NOW, &HashSet::new()), Vec::<u64>::new());
        // Its work over, the next run takes it too.
        assert_eq!(archive_due(30, NOW, &HashSet::new()), [busy.id]);
        // A message in an archived chat brings it back.
        chats::append(old.id, "user", "And rye?", &[], None, false).unwrap();
        assert_eq!(titles(chats::list_archived()), ["Build scripts"]);
    }

    #[test]
    fn runs_not_said_yet_are_said_as_one() {
        let mut a = Archived { ids: vec![1, 2], days: 30 };
        a.merge(Archived { ids: vec![2, 3], days: 7 });
        assert_eq!(a, Archived { ids: vec![1, 2, 3], days: 7 });
        let json = serde_json::to_value(&a).unwrap();
        assert_eq!(json, serde_json::json!({ "ids": [1, 2, 3], "days": 7 }));
    }
}

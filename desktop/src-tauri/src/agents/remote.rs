//! **Remote Control's own records** (2026-11-08, `link/code.rs`): which
//! paired phones may use Yap Code from afar, which workspaces allow it, which
//! agent turns a phone asked for, and what every phone did (the audit trail).
//! Kept in `remote_control.json`, a file of its own — never fields in
//! `agents.json`, `workspaces.json` or `link_devices.json`, which an older
//! Yap rewrites without the fields it doesn't know (a tick lost that way
//! would only turn Remote Control off for that phone: the safe way, but
//! still lost).
//!
//! - **Per phone** ([`Remote::phone_on`]): off unless ticked. A phone paired
//!   while Remote Control is on is ticked as it pairs ([`paired`]); one
//!   paired before stays off until ticked in Settings → Phone, so turning
//!   the switch on never widens an old pairing silently.
//! - **Per workspace** ([`Remote::workspace_allows`]): on unless switched off
//!   (the Environment card).
//! - **A phone's turns** ([`Remote::phone_of_turn`]): the person's message
//!   that asked for a turn was a phone's question ([`asked`], recorded as it
//!   was saved), or a phone's own action carried the turn on (Continue, Go
//!   ahead, "Tell the agent…": [`mark_turn`]). The runner runs such a turn at
//!   most at Auto, never Bypass (`runner::turn`), and its report is said on
//!   that phone, never on this PC's speakers (`chat/turn/report.rs`).
//! - **The audit trail** ([`Audit`], the last [`KEEP_RECENT`]): who (the
//!   phone's name and id), what, which agent, turn, conversation and
//!   workspace, when, the outcome — never a command, a file, a brief or
//!   anyone's words.
//!
//! Delete some of your data → Yap Code forgets the turns and the trail
//! ([`forget_activity`]); the ticks and the workspaces' switches stay, as
//! the pairing does. Unit tests keep a store per test thread.

#[cfg(not(test))]
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

/// **The highest Mode a phone's turn runs at**, or a phone may pick: never
/// Bypass permissions (Claude's own rule for its app; a lost phone mustn't
/// run anything unasked).
pub const PHONE_MAX: crate::agents::modes::Mode = crate::agents::modes::Mode::Auto;

/// The Mode a phone's turn runs at in a chat at `mode`: it, at most
/// [`PHONE_MAX`]. Pure.
pub fn phone_mode(mode: crate::agents::modes::Mode) -> crate::agents::modes::Mode {
    mode.min(PHONE_MAX)
}

/// **A remote action in an agent's log** (`AgentEvent::remote`): its tab
/// shows the line ("Allowed from Office phone · Run a command: npm test").
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct RemoteNote {
    pub device: String,
    pub name: String,
    pub action: String,
}

/// How many audit entries are kept.
pub const KEEP_RECENT: usize = 200;
/// How many phone-asked messages and turns are remembered.
pub const KEEP_MARKS: usize = 500;

/// A phone's Remote Control tick.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Tick {
    pub on: bool,
    /// Since when (unix seconds).
    pub since: u64,
}

/// A turn (or a question) a phone asked for.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Mark {
    /// `<agent>:<turn>`, or a message id (`chat:<conversation>:<index>`).
    pub key: String,
    pub device: String,
}

/// **One remote action**, as the trail keeps it: never the command, the
/// file, the brief or the words.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Audit {
    /// Unix seconds.
    pub ts: u64,
    pub device: String,
    /// The phone's name as it was paired ("Android · Firefox").
    pub name: String,
    /// `approved`, `denied`, `plan go ahead`, `plan not yet`, `started`,
    /// `not started`, `stopped`, `continued`, `undid`, `undid turn`, `go
    /// ahead`, `message`, `mode` (with `from` / `to`).
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    /// A Mode change's modes (`Mode::as_str`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// `ok`, or why not (`refused`, `failed`).
    pub outcome: String,
}

impl Audit {
    /// What it did, as Settings → Phone says a phone's last action: "Allowed
    /// a step in garden-planner". Pure.
    pub fn words(&self) -> String {
        let place = self.workspace.as_deref().map(|w| format!(" in {w}")).unwrap_or_default();
        let mode = |m: &Option<String>| m.as_deref().map(|m| crate::agents::modes::or_auto(m).label()).unwrap_or("?");
        match self.action.as_str() {
            "approved" => format!("Allowed a step{place}"),
            "denied" => format!("Denied a step{place}"),
            "plan go ahead" | "go ahead" => format!("Went ahead with a plan{place}"),
            "plan not yet" => format!("Kept a plan waiting{place}"),
            "started" => format!("Started an agent{place}"),
            "not started" => format!("Didn't start an agent{place}"),
            "stopped" => format!("Stopped an agent{place}"),
            "continued" => format!("Continued an agent{place}"),
            "undid" => format!("Undid an agent's changes{place}"),
            "undid turn" => format!("Undid a turn{place}"),
            "message" => format!("Told an agent something{place}"),
            "mode" => format!("Mode {} → {}{place}", mode(&self.from), mode(&self.to)),
            other => format!("{other}{place}"),
        }
    }
}

/// What remote_control.json holds.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Remote {
    /// Device id → its tick.
    pub phones: std::collections::BTreeMap<String, Tick>,
    /// Workspace name → `false` when it doesn't allow Remote Control (on
    /// unless said).
    pub workspaces: std::collections::BTreeMap<String, bool>,
    /// The turns a phone's own action carried on (`<agent>:<turn>`).
    pub turns: Vec<Mark>,
    /// The messages that were a phone's questions (`chat:<conversation>:<index>`).
    pub asked: Vec<Mark>,
    /// The audit trail, oldest first.
    pub recent: Vec<Audit>,
}

/// `<agent>:<turn>`. Pure.
pub fn turn_key(agent: &str, turn: u32) -> String {
    format!("{agent}:{turn}")
}

fn remember(list: &mut Vec<Mark>, key: String, device: &str) {
    list.retain(|m| m.key != key);
    list.push(Mark { key, device: device.to_string() });
    let over = list.len().saturating_sub(KEEP_MARKS);
    list.drain(..over);
}

impl Remote {
    /// Is phone `device` ticked for Remote Control?
    pub fn phone_on(&self, device: &str) -> bool {
        self.phones.get(device).is_some_and(|t| t.on)
    }

    /// Tick phone `device` (or not). Whether it changed.
    pub fn set_phone(&mut self, device: &str, on: bool, now: u64) -> bool {
        if self.phone_on(device) == on {
            return false;
        }
        self.phones.insert(device.to_string(), Tick { on, since: now });
        true
    }

    /// A phone removed (Settings → Phone): its tick and marks go; its trail
    /// stays (what it did, as it was).
    pub fn forget_phone(&mut self, device: &str) {
        self.phones.remove(device);
        self.turns.retain(|m| m.device != device);
        self.asked.retain(|m| m.device != device);
    }

    /// Does workspace `name` allow Remote Control (on unless switched off)?
    pub fn workspace_allows(&self, name: &str) -> bool {
        self.workspaces.get(name).copied().unwrap_or(true)
    }

    pub fn set_workspace(&mut self, name: &str, on: bool) -> bool {
        if self.workspace_allows(name) == on {
            return false;
        }
        if on {
            self.workspaces.remove(name);
        } else {
            self.workspaces.insert(name.to_string(), false);
        }
        true
    }

    /// The person's message `message_id` was phone `device`'s question.
    pub fn asked(&mut self, message_id: &str, device: &str) {
        remember(&mut self.asked, message_id.to_string(), device);
    }

    /// The phone whose question message `message_id` was.
    pub fn phone_of_message(&self, message_id: &str) -> Option<&str> {
        self.asked.iter().rev().find(|m| m.key == message_id).map(|m| m.device.as_str())
    }

    /// Agent `agent`'s turn `turn` was carried on by phone `device`'s action.
    pub fn mark_turn(&mut self, agent: &str, turn: u32, device: &str) {
        remember(&mut self.turns, turn_key(agent, turn), device);
    }

    /// **The phone that asked for agent `agent`'s turn `turn`**: its own
    /// action carried the turn on, or the person's message that asked for it
    /// (`message_id`) was the phone's question. `None`: asked at this PC.
    pub fn phone_of_turn(&self, agent: &str, turn: u32, message_id: Option<&str>) -> Option<&str> {
        let key = turn_key(agent, turn);
        self.turns
            .iter()
            .rev()
            .find(|m| m.key == key)
            .map(|m| m.device.as_str())
            .or_else(|| message_id.and_then(|id| self.phone_of_message(id)))
    }

    /// Add `audit` to the trail (the last [`KEEP_RECENT`] kept).
    pub fn record(&mut self, audit: Audit) {
        self.recent.push(audit);
        let over = self.recent.len().saturating_sub(KEEP_RECENT);
        self.recent.drain(..over);
    }

    /// Phone `device`'s latest action.
    pub fn last_of(&self, device: &str) -> Option<&Audit> {
        self.recent.iter().rev().find(|a| a.device == device)
    }

    /// Delete some of your data → Yap Code: the turns, the questions and the
    /// trail go; the ticks and the workspaces' switches stay.
    pub fn forget_activity(&mut self) {
        self.turns.clear();
        self.asked.clear();
        self.recent.clear();
    }
}

// ---- the store ----

/// remote_control.json's saves.
pub static SAVER: crate::shell::saving::Saver = crate::shell::saving::Saver::new("remote-control", "Remote Control's settings", save_now);

fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    with(|_| true);
    SAVER.check()
}

#[cfg(not(test))]
static STATE: Mutex<Option<Remote>> = Mutex::new(None);

#[cfg(not(test))]
fn path() -> std::path::PathBuf {
    crate::config::data_dir().join("remote_control.json")
}

/// The store, `f`'s change saved when it says so (`true`).
#[cfg(not(test))]
fn with<R>(f: impl FnOnce(&mut Remote) -> R) -> R
where
    R: Changed,
{
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let store = SAVER.loaded(&mut *guard, &path(), false);
    let out = f(store);
    if out.changed() && !crate::ai::eval::active() {
        let _ = SAVER.write_json(&path(), &*store, false);
    }
    out
}

#[cfg(test)]
thread_local! {
    static TEST_STORE: std::cell::RefCell<Remote> = std::cell::RefCell::new(Remote::default());
}

#[cfg(test)]
fn with<R: Changed>(f: impl FnOnce(&mut Remote) -> R) -> R {
    TEST_STORE.with(|s| f(&mut s.borrow_mut()))
}

/// Whether a store call changed what's kept (and so is saved).
pub trait Changed {
    #[cfg_attr(test, allow(dead_code))]
    fn changed(&self) -> bool;
}

impl Changed for bool {
    fn changed(&self) -> bool {
        *self
    }
}

/// A read: nothing to save.
struct Read<T>(T);

impl<T> Changed for Read<T> {
    fn changed(&self) -> bool {
        false
    }
}

fn read<T>(f: impl FnOnce(&Remote) -> T) -> T {
    with(|r| Read(f(r))).0
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Is phone `device` ticked for Remote Control?
pub fn phone_on(device: &str) -> bool {
    read(|r| r.phone_on(device))
}

/// Tick (or untick) phone `device` (Settings → Phone). Whether it changed.
pub fn set_phone(device: &str, on: bool) -> bool {
    with(|r| r.set_phone(device, on, now_secs()))
}

/// **A phone just paired**: ticked when Remote Control is on now (a phone
/// paired before stays as it was).
pub fn paired(device: &str, remote_on: bool) {
    if remote_on {
        set_phone(device, true);
    }
}

/// A phone removed: its tick and marks go.
pub fn forget_phone(device: &str) {
    with(|r| {
        r.forget_phone(device);
        true
    });
}

/// Does workspace `name` allow Remote Control?
pub fn workspace_allows(name: &str) -> bool {
    read(|r| r.workspace_allows(name))
}

/// Allow Remote Control in workspace `name` (or not). Whether it changed.
pub fn set_workspace(name: &str, on: bool) -> bool {
    with(|r| r.set_workspace(name, on))
}

/// The person's message `message_id` was phone `device`'s question.
pub fn asked(message_id: &str, device: &str) {
    with(|r| {
        r.asked(message_id, device);
        true
    });
}

/// The phone whose question message `message_id` was.
pub fn phone_of_message(message_id: &str) -> Option<String> {
    read(|r| r.phone_of_message(message_id).map(str::to_string))
}

/// Agent `agent`'s turn `turn` was carried on by phone `device`'s action.
pub fn mark_turn(agent: &str, turn: u32, device: &str) {
    with(|r| {
        r.mark_turn(agent, turn, device);
        true
    });
}

/// The phone that asked for agent `agent`'s turn `turn` ([`Remote::phone_of_turn`]).
pub fn phone_of_turn(agent: &str, turn: u32, message_id: Option<&str>) -> Option<String> {
    read(|r| r.phone_of_turn(agent, turn, message_id).map(str::to_string))
}

/// Add `audit` to the trail; logged with its kind and outcome only.
pub fn record(audit: Audit) {
    tracing::info!(action = %audit.action, outcome = %audit.outcome, turn = ?audit.turn, "Remote Control: a phone's action");
    with(|r| {
        r.record(audit);
        true
    });
}

/// Phone `device`'s latest action, as Settings says it, and when.
pub fn last_of(device: &str) -> Option<(String, u64)> {
    read(|r| r.last_of(device).map(|a| (a.words(), a.ts)))
}

/// Delete some of your data → Yap Code ([`Remote::forget_activity`]).
pub fn forget_activity() {
    with(|r| {
        r.forget_activity();
        true
    });
}

/// Saved, or why not (Delete some of your data's check).
pub fn saved() -> Result<(), crate::shell::saving::SaveError> {
    SAVER.check()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phone_is_off_until_ticked_and_one_paired_while_on_is_ticked() {
        let mut r = Remote::default();
        assert!(!r.phone_on("p-old"), "paired before Remote Control: off");
        assert!(r.set_phone("p-old", true, 10));
        assert!(!r.set_phone("p-old", true, 11), "already on");
        assert!(r.phone_on("p-old"));
        assert_eq!(r.phones["p-old"], Tick { on: true, since: 10 });
        assert!(r.set_phone("p-old", false, 12));
        assert!(!r.phone_on("p-old"));
        // The store's own: paired while on, ticked; while off, left alone.
        paired("p-new", true);
        paired("p-later", false);
        assert!(phone_on("p-new") && !phone_on("p-later"));
        forget_phone("p-new");
        assert!(!phone_on("p-new"));
    }

    #[test]
    fn a_workspace_allows_it_unless_switched_off() {
        let mut r = Remote::default();
        assert!(r.workspace_allows("garden-planner"));
        assert!(r.set_workspace("garden-planner", false));
        assert!(!r.workspace_allows("garden-planner"));
        assert!(!r.set_workspace("garden-planner", false));
        assert!(r.set_workspace("garden-planner", true));
        assert!(r.workspaces.is_empty(), "on is the default: nothing kept");
    }

    #[test]
    fn a_phones_turn_is_its_question_or_its_own_action() {
        let mut r = Remote::default();
        r.asked("chat:4:2", "p-office");
        // The agent's turn 1 asked by that question.
        assert_eq!(r.phone_of_turn("ab12", 1, Some("chat:4:2")), Some("p-office"));
        // A question at the PC.
        assert_eq!(r.phone_of_turn("ab12", 2, Some("chat:4:4")), None);
        assert_eq!(r.phone_of_turn("ab12", 2, None), None);
        // Continue from the phone carried turn 2 on.
        r.mark_turn("ab12", 2, "p-kitchen");
        assert_eq!(r.phone_of_turn("ab12", 2, Some("chat:4:4")), Some("p-kitchen"));
        // Bounded, the newest kept.
        for i in 0..KEEP_MARKS + 5 {
            r.asked(&format!("chat:9:{i}"), "p-office");
        }
        assert_eq!(r.asked.len(), KEEP_MARKS);
        assert_eq!(r.phone_of_message("chat:4:2"), None, "the oldest forgotten");
        assert!(r.phone_of_message(&format!("chat:9:{}", KEEP_MARKS + 4)).is_some());
        // A removed phone's marks go.
        r.forget_phone("p-kitchen");
        assert_eq!(r.phone_of_turn("ab12", 2, None), None);
    }

    #[test]
    fn the_trail_keeps_what_was_done_never_the_words() {
        let mut r = Remote::default();
        let a = Audit {
            ts: 100,
            device: "p-office".into(),
            name: "Office phone".into(),
            action: "approved".into(),
            agent: Some("ab12".into()),
            turn: Some(1),
            conversation: Some(4),
            workspace: Some("garden-planner".into()),
            outcome: "ok".into(),
            ..Default::default()
        };
        r.record(a.clone());
        assert_eq!(r.last_of("p-office").map(Audit::words).as_deref(), Some("Allowed a step in garden-planner"));
        let mode = Audit { action: "mode".into(), from: Some("auto".into()), to: Some("plan".into()), ..a.clone() };
        assert_eq!(mode.words(), "Mode Auto → Plan in garden-planner");
        // Its shape: ids, kinds and outcomes only.
        let v = serde_json::to_value(&a).unwrap();
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, ["action", "agent", "conversation", "device", "name", "outcome", "ts", "turn", "workspace"]);
        for i in 0..KEEP_RECENT + 3 {
            r.record(Audit { ts: i as u64, ..a.clone() });
        }
        assert_eq!(r.recent.len(), KEEP_RECENT);
        assert_eq!(r.recent[0].ts, 3, "the oldest go");
        // Delete some of your data → Yap Code: activity goes, ticks stay.
        r.set_phone("p-office", true, 1);
        r.set_workspace("recipe-box", false);
        r.asked("chat:1:0", "p-office");
        r.forget_activity();
        assert!(r.recent.is_empty() && r.asked.is_empty() && r.turns.is_empty());
        assert!(r.phone_on("p-office") && !r.workspace_allows("recipe-box"));
    }

    #[test]
    fn an_older_file_and_a_new_one_read_back() {
        let r: Remote = serde_json::from_str(r#"{"phones":{"p-1":{"on":true,"since":5}},"somethingNewer":1}"#).unwrap();
        assert!(r.phone_on("p-1") && r.workspace_allows("x"));
        let empty: Remote = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, Remote::default());
    }
}

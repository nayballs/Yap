//! **Work a turn leaves running past its answer** (16 Nov 2026), from a Code
//! chat whose Claude lost its research helper every time a reply ended: Yap
//! closed Claude Code's input at the turn's result and ended the process 15
//! seconds later, and with it everything Claude Code had started to outlive
//! the reply — a helper (`Agent` / `Task`) or a command (`Bash`,
//! `PowerShell`) with `run_in_background`, a wake-up (`ScheduleWakeup`).
//! Claude Code itself keeps its process up for them and, when one ends,
//! starts a turn of its own on it (checked on 2.1.296: `ai/claude_code/
//! events.rs`). So a turn now waits for them ([`Background::pending`]): the
//! runner keeps the process and its input open until they've reported back.
//!
//! - **Claude Code's own list first** (18 Nov 2026): while its latest
//!   `background_tasks_changed` lists anything, it waits.
//! - **A start**: a task Claude Code says runs in the background
//!   (`task_started` with `is_backgrounded`) — the session's own (its call's
//!   `run_in_background`, [`Background::on_call`], says what it is) or a
//!   helper's: its own helpers (`parent_task_id`, `spawn_depth` 2) and its
//!   own background commands (`owned_by_subagent`, `parent_task_id`), which
//!   count under it, never as the session's own. A Claude Code that says
//!   nothing of its tasks has none here: nothing waits.
//! - **An end**: its `task_notification`, or the list no longer naming it
//!   ([`Background::on_task`]). **But a helper's `completed` with work of its
//!   own still running is interim** (18 Nov 2026, from a Code chat whose lead
//!   helper paused for six helpers of its own and a watcher, was counted as
//!   finished, and lost its merge when the process was ended under it): it
//!   ended its turn and waits; when its work ends Claude Code starts it again
//!   — the same task id, a new `task_started` — and it notifies again. So it
//!   stays, paused ([`Background::paused`]), till a notice with nothing of
//!   its own left — or till it has had nothing left for [`PAUSE_GRACE`]
//!   without starting again ([`Background::settle`]).
//! - **A wake-up**: due when its call's record says (`scheduledFor`,
//!   [`Background::on_call_back`]); its turn is the next `command_lifecycle`
//!   Claude Code starts. One long overdue is let go ([`Background::expire`]).
//!
//! Pure: the runner feeds it Claude Code's events (and the time).

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::ai::claude_code::events::Task;

/// A wake-up this long past due with no turn of Claude Code's own is let go.
const WAKE_LATE_MS: u64 = 5 * 60 * 1000;

/// **A paused helper with nothing of its own left** is started again by
/// Claude Code at once (5 ms on 2.1.296); one that isn't within this long is
/// taken as done.
pub const PAUSE_GRACE: Duration = Duration::from_secs(30);

/// What a piece of background work is, as the waiting line counts it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A helper (`Agent` / `Task`).
    Helper,
    /// A command (`Bash`, `PowerShell`).
    Command,
    /// Anything else Claude Code runs as a task.
    Other,
}

/// **A task of the session's own that ended** ([`Background::on_task`]): the
/// call that started it, its kind, how it ended (`completed`, `stopped`…;
/// "" when only the background list said so) and Claude Code's words about
/// it (a helper's: what it came back with).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ended {
    pub call: String,
    pub kind: Kind,
    pub status: String,
    pub summary: String,
}

/// A task in the background now.
#[derive(Debug, Clone)]
struct Running {
    /// The call that started it.
    call: String,
    kind: Kind,
    /// The helper's task that started it (none: the session's own).
    parent: Option<String>,
    /// **Paused**: a helper that ended its turn with work of its own still
    /// running (its interim notice's words).
    paused: Option<String>,
    /// Since when a paused helper has had nothing of its own left.
    alone_since: Option<Instant>,
}

impl Running {
    fn own(&self) -> bool {
        self.parent.is_none()
    }
}

/// **The work in the background**, as Claude Code's stream says it.
#[derive(Debug, Default)]
pub struct Background {
    /// The session's own calls that asked for the background, by call id, till their task starts.
    asked: HashMap<String, Kind>,
    /// Every task in the background now — the session's own and its
    /// helpers' — by task id.
    running: HashMap<String, Running>,
    /// **Claude Code's own list** of the tasks in the background, as it last
    /// said it (`background_tasks_changed`).
    listed: Vec<String>,
    /// Every task it saw start (any: in the background or not, a helper's
    /// own too): a notice of one it never saw is an earlier process's.
    seen: HashSet<String>,
    /// Its `ScheduleWakeup` calls not back yet.
    wake_calls: Vec<String>,
    /// The wake-ups it scheduled, when each is due (unix ms).
    wakes: Vec<u64>,
}

/// Claude Code's tool that schedules a turn of its own later.
pub const WAKE_TOOL: &str = "ScheduleWakeup";

/// Whether a call's `input` asks for the background (`run_in_background`
/// true, or "true": Claude Code took the string too, 16 Nov 2026). Pure.
pub fn asks_background(input: &Value) -> bool {
    match input.get("run_in_background") {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s.trim().eq_ignore_ascii_case("true"),
        _ => false,
    }
}

/// A call's kind by its tool's name. Pure.
fn kind_of_call(name: &str) -> Kind {
    match name {
        "Task" | "Agent" => Kind::Helper,
        "Bash" | "PowerShell" => Kind::Command,
        _ => Kind::Other,
    }
}

/// A task's kind by Claude Code's `task_type`, else what its call was. Pure.
fn kind_of_task(task_type: &str, call: Option<Kind>) -> Kind {
    match task_type {
        "local_agent" | "remote_agent" => Kind::Helper,
        "local_bash" => Kind::Command,
        _ => call.unwrap_or(Kind::Other),
    }
}

impl Background {
    /// **A call of the session's own** (never a helper's): one that asks for
    /// the background noted, so its task is known for what it is; a wake-up's
    /// till it's back.
    pub fn on_call(&mut self, id: &str, name: &str, input: &Value) {
        if id.is_empty() {
            return;
        }
        if name == WAKE_TOOL {
            self.wake_calls.push(id.to_string());
        } else if asks_background(input) {
            self.asked.insert(id.to_string(), kind_of_call(name));
        }
    }

    /// **A call came back** (`record`: Claude Code's own record of it): one
    /// that failed started nothing; a wake-up's record says when it's due
    /// (`scheduledFor`, unix ms).
    pub fn on_call_back(&mut self, id: &str, is_error: bool, record: Option<&Value>) {
        if is_error {
            self.asked.remove(id);
        }
        let Some(i) = self.wake_calls.iter().position(|c| c == id) else { return };
        self.wake_calls.remove(i);
        if let Some(due) = record.filter(|_| !is_error).and_then(|r| r.get("scheduledFor")).and_then(Value::as_u64).filter(|d| *d > 0) {
            self.wakes.push(due);
        }
    }

    /// **A turn Claude Code started by itself** (`command_lifecycle`): the
    /// earliest wake-up is it.
    pub fn on_lifecycle(&mut self, started: bool) {
        if !started || self.wakes.is_empty() {
            return;
        }
        let first = self.wakes.iter().enumerate().min_by_key(|(_, d)| **d).map(|(i, _)| i).unwrap_or(0);
        self.wakes.remove(first);
    }

    /// **Wake-ups long past due** at `now_ms` with no turn: let go. Whether any went.
    pub fn expire(&mut self, now_ms: u64) -> bool {
        let before = self.wakes.len();
        self.wakes.retain(|d| d.saturating_add(WAKE_LATE_MS) > now_ms);
        self.wakes.len() != before
    }

    /// Whether task `id` has work of its own still in the background (a
    /// child of it, paused or not).
    fn has_children(&self, id: &str) -> bool {
        self.running.values().any(|r| r.parent.as_deref() == Some(id))
    }

    /// Task `id` is gone: the session's own, said as ended; and a paused
    /// helper it was the last child of has nothing left from `now`.
    fn gone(&mut self, id: &str, status: &str, summary: &str, now: Instant) -> Option<Ended> {
        let r = self.running.remove(id)?;
        if let Some(parent) = r.parent.as_deref() {
            let alone = !self.has_children(parent);
            if let Some(p) = self.running.get_mut(parent).filter(|p| p.paused.is_some() && alone) {
                p.alone_since = Some(now);
            }
        }
        r.own().then(|| Ended { call: r.call, kind: r.kind, status: status.to_string(), summary: summary.to_string() })
    }

    /// **One of Claude Code's task events** (at `now`): a task in the
    /// background begins, pauses, starts again or ends — the session's own
    /// that ended, returned.
    pub fn on_task(&mut self, t: &Task, now: Instant) -> Vec<Ended> {
        match t {
            Task::Started { task_id, tool_use_id, task_type, background, by_helper, parent, .. } => {
                self.seen.insert(task_id.clone());
                let call = self.asked.remove(tool_use_id);
                if !*background || task_id.is_empty() {
                    return Vec::new();
                }
                // A paused helper started again (its work of its own ended or reported).
                if let Some(r) = self.running.get_mut(task_id) {
                    r.paused = None;
                    r.alone_since = None;
                    return Vec::new();
                }
                // (A helper's own command that names no helper: still its, never the session's.)
                let parent = parent.clone().or_else(|| by_helper.then(String::new));
                self.running.insert(task_id.clone(), Running { call: tool_use_id.clone(), kind: kind_of_task(task_type, call), parent, paused: None, alone_since: None });
                Vec::new()
            }
            // (Its end is said by the notice that follows, with Claude Code's words.)
            Task::Updated { .. } => Vec::new(),
            Task::Notice { task_id, status, summary, .. } => {
                let Some(kind) = self.running.get(task_id).map(|r| r.kind) else { return Vec::new() };
                // A helper done for now, with work of its own still running: paused, not ended.
                if kind == Kind::Helper && status == "completed" && self.has_children(task_id) {
                    if let Some(r) = self.running.get_mut(task_id) {
                        r.paused = Some(summary.clone());
                        r.alone_since = None;
                    }
                    return Vec::new();
                }
                self.gone(task_id, status, summary, now).into_iter().collect()
            }
            Task::Background { tasks } => {
                self.listed = tasks.clone();
                // Gone from Claude Code's list: ended — but a paused helper, which drops off it while it waits.
                let gone: Vec<String> = self.running.iter().filter(|(id, r)| !tasks.contains(id) && r.paused.is_none()).map(|(id, _)| id.clone()).collect();
                gone.into_iter().filter_map(|id| self.gone(&id, "", "", now)).collect()
            }
        }
    }

    /// **Paused helpers left alone** at `now`: one with nothing of its own
    /// left for [`PAUSE_GRACE`], never started again, is done — its last
    /// words its interim notice's.
    pub fn settle(&mut self, now: Instant) -> Vec<Ended> {
        let done: Vec<String> = self.running.iter().filter(|(_, r)| r.alone_since.is_some_and(|at| now.duration_since(at) >= PAUSE_GRACE)).map(|(id, _)| id.clone()).collect();
        done.into_iter()
            .filter_map(|id| {
                let summary = self.running.get(&id).and_then(|r| r.paused.clone()).unwrap_or_default();
                self.gone(&id, "completed", &summary, now)
            })
            .collect()
    }

    /// **A task an earlier process left, that Claude Code says stopped** (a
    /// resumed session's notice): one this process never saw start.
    pub fn stopped_before(&self, t: &Task) -> bool {
        matches!(t, Task::Notice { task_id, status, .. } if status == "stopped" && !self.seen.contains(task_id))
    }

    /// **Whether a call's task runs in the background now** (the session's
    /// own, paused or not): its helper's row stays at work after the call
    /// came back.
    pub fn runs_call(&self, call: &str) -> bool {
        self.running.values().any(|r| r.own() && r.call == call)
    }

    /// **Work still to report back**: anything on Claude Code's own list, a
    /// task in the background (a paused helper too), or a wake-up.
    pub fn pending(&self) -> bool {
        !self.listed.is_empty() || !self.running.is_empty() || !self.wakes.is_empty()
    }

    /// **How many tasks Claude Code lists now, and how many run in the
    /// background as Yap counts them** (the session's own, its helpers', the
    /// paused): for the log.
    pub fn counts(&self) -> (usize, usize, usize) {
        let paused = self.running.values().filter(|r| r.paused.is_some()).count();
        (self.listed.len(), self.running.len(), paused)
    }

    /// **The paused helpers** (ended a turn with work of their own running).
    #[cfg(test)]
    pub fn paused(&self) -> usize {
        self.counts().2
    }

    /// **What it waits for, in words**: "1 helper", "2 commands", "1 helper,
    /// 1 command", "a wake-up" — the session's own (a helper's own work counts
    /// under it); "" with nothing.
    pub fn what(&self) -> String {
        let count = |k: Kind| self.running.values().filter(|r| r.own() && r.kind == k).count();
        let some = |n: usize, one: &str, many: &str| match n {
            0 => None,
            1 => Some(format!("1 {one}")),
            n => Some(format!("{n} {many}")),
        };
        let wakes = match self.wakes.len() {
            0 => None,
            1 => Some("a wake-up".to_string()),
            n => Some(format!("{n} wake-ups")),
        };
        [some(count(Kind::Helper), "helper", "helpers"), some(count(Kind::Command), "command", "commands"), some(count(Kind::Other), "task", "tasks"), wakes].into_iter().flatten().collect::<Vec<_>>().join(", ")
    }

    /// **The turn's line while it waits**: "Waiting for background work (1 helper)".
    pub fn line(&self) -> String {
        match self.what() {
            w if w.is_empty() => "Waiting for background work".to_string(),
            w => format!("Waiting for background work ({w})"),
        }
    }
}

/// **The line for a task of an earlier process's that Claude Code says
/// stopped** (its `task_notification` `stopped`, as a resumed session reads
/// it): one quiet line in the turn's work.
pub const STOPPED_EARLIER: &str = "Background work from before was stopped before it finished";

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn started(task: &str, call: &str, task_type: &str, background: bool, by_helper: bool) -> Task {
        Task::Started { task_id: task.into(), tool_use_id: call.into(), task_type: task_type.into(), background, by_helper, parent: None, depth: 1 }
    }

    fn notice(task: &str, status: &str, summary: &str) -> Task {
        Task::Notice { task_id: task.into(), tool_use_id: String::new(), status: status.into(), summary: summary.into() }
    }

    #[test]
    fn a_helper_and_a_command_in_the_background_are_waited_for_until_each_reports_back() {
        let now = Instant::now();
        let mut bg = Background::default();
        assert!(!bg.pending());
        assert_eq!(bg.line(), "Waiting for background work");
        // Asked for in the call's input (a string "true" too), made a task by Claude Code.
        bg.on_call("toolu_helper", "Agent", &json!({ "description": "Compare seed suppliers", "run_in_background": true }));
        bg.on_call("toolu_cmd", "PowerShell", &json!({ "command": "npm run build", "run_in_background": "true" }));
        bg.on_call("toolu_fg", "PowerShell", &json!({ "command": "ls" }));
        assert!(!bg.pending(), "nothing runs till Claude Code says it does");
        bg.on_task(&started("a1", "toolu_helper", "local_agent", true, false), now);
        bg.on_task(&started("b1", "toolu_cmd", "local_bash", true, false), now);
        // A foreground command's task: nothing in the background.
        bg.on_task(&started("b2", "toolu_fg", "local_bash", false, false), now);
        assert!(bg.on_task(&Task::Background { tasks: vec!["a1".into(), "b1".into()] }, now).is_empty());
        assert!(bg.pending());
        assert_eq!(bg.line(), "Waiting for background work (1 helper, 1 command)");
        assert!(bg.runs_call("toolu_helper") && bg.runs_call("toolu_cmd") && !bg.runs_call("toolu_fg"));
        // The helper's end, nothing of its own running: with what it came back with.
        assert!(bg.on_task(&Task::Updated { task_id: "a1".into(), status: Some("completed".into()) }, now).is_empty());
        let ended = bg.on_task(&Task::Notice { task_id: "a1".into(), tool_use_id: "toolu_helper".into(), status: "completed".into(), summary: "Hedgerow is cheapest.".into() }, now);
        assert_eq!(ended, [Ended { call: "toolu_helper".into(), kind: Kind::Helper, status: "completed".into(), summary: "Hedgerow is cheapest.".into() }]);
        assert_eq!(bg.line(), "Waiting for background work (1 command)");
        // The command, gone from the list Claude Code keeps.
        let ended = bg.on_task(&Task::Background { tasks: vec![] }, now);
        assert_eq!(ended, [Ended { call: "toolu_cmd".into(), kind: Kind::Command, status: String::new(), summary: String::new() }]);
        assert!(!bg.pending());
        // A notice of a task it never saw start (an earlier process's): nothing of this turn's, said as stopped before.
        let old = Task::Notice { task_id: "old".into(), tool_use_id: "toolu_old".into(), status: "stopped".into(), summary: String::new() };
        assert!(bg.stopped_before(&old));
        assert!(bg.on_task(&old, now).is_empty());
        // One it saw start never is; nor anything but a stop.
        assert!(!bg.stopped_before(&notice("b2", "stopped", "")));
        assert!(!bg.stopped_before(&notice("old2", "completed", "")), "only a stop");
    }

    /// **Claude Code's own list first**: while it lists anything — a task Yap
    /// never saw start too — the turn waits.
    #[test]
    fn while_claude_code_lists_anything_it_waits() {
        let now = Instant::now();
        let mut bg = Background::default();
        bg.on_task(&Task::Background { tasks: vec!["x9".into()] }, now);
        assert!(bg.pending());
        assert_eq!(bg.counts(), (1, 0, 0));
        bg.on_task(&Task::Background { tasks: vec![] }, now);
        assert!(!bg.pending());
    }

    /// **The recording** (`fixtures/2.1.296/real-background-nested.jsonl`): a
    /// helper that starts a helper of its own and two background commands,
    /// then ends its turn. Its first `completed` is interim (paused: still
    /// waited for) even as it drops off Claude Code's list; its own work
    /// counts under it (the line says one helper) and keeps the turn waiting,
    /// the moment the list is empty too; it's started again (the same id) and
    /// notifies again; its last notice, nothing of its own left, ends it.
    #[test]
    fn a_helper_that_pauses_for_work_of_its_own_is_waited_for_till_its_last_notice() {
        use crate::ai::claude_code::events::{parse_line, Event};
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ai/claude_code/fixtures/2.1.296/real-background-nested.jsonl");
        let tasks: Vec<Task> = std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .filter(|l| !l.trim_start().starts_with("//") && !l.trim_start().starts_with("{\"_"))
            .filter_map(|l| match parse_line(&l.replace("{{session_id}}", "s")) {
                Some(Event::Task(t)) => Some(t),
                _ => None,
            })
            .collect();
        let lead = "a7c2e91f04b3d6852";
        let now = Instant::now();
        let mut bg = Background::default();
        bg.on_call("toolu_01LeadHelper", "Agent", &json!({ "run_in_background": true }));
        let mut ended = Vec::new();
        let mut was = Vec::new();
        for t in &tasks {
            ended.extend(bg.on_task(t, now));
            // After each: still waited for, the lead paused, and the line.
            was.push((bg.pending(), bg.paused(), bg.what()));
            if let Task::Notice { task_id, .. } = t {
                if task_id == lead && !ended.is_empty() {
                    break;
                }
            }
        }
        // Waited for at every step (once it started) till the lead's last notice; never "2 helpers": its own work
        // counts under it.
        assert!(was[1..was.len() - 1].iter().all(|(p, _, what)| *p && what == "1 helper"), "{was:?}");
        // Paused at its interim notices (off Claude Code's list then, its own work on it).
        assert!(was.iter().any(|(_, paused, _)| *paused == 1), "{was:?}");
        // Its own helper and commands ended with nothing said of them (never the session's own); the lead ended once, at the end.
        assert_eq!(ended, [Ended { call: "toolu_01LeadHelper".into(), kind: Kind::Helper, status: "completed".into(), summary: "Merged: beans and kale north, herbs east, squash south.".into() }]);
        // The list's last word: empty — nothing waits.
        let last = tasks.iter().rev().find(|t| matches!(t, Task::Background { .. })).unwrap();
        bg.on_task(last, now);
        assert!(!bg.pending(), "{:?}", bg.counts());
    }

    /// The same, piece by piece: an interim notice while a child runs stays
    /// pending (off Claude Code's list); the child's end leaves it alone; it
    /// starts again and its later notice ends it. Left alone and never
    /// started again, it's done after [`PAUSE_GRACE`].
    #[test]
    fn an_interim_notice_stays_pending_till_a_later_one_or_the_grace() {
        let t0 = Instant::now();
        let child = |id: &str, parent: &str, task_type: &str, by_helper: bool| Task::Started { task_id: id.into(), tool_use_id: format!("toolu_{id}"), task_type: task_type.into(), background: true, by_helper, parent: Some(parent.into()), depth: 2 };
        for started_again in [true, false] {
            let mut bg = Background::default();
            bg.on_task(&started("lead", "toolu_lead", "local_agent", true, false), t0);
            bg.on_task(&child("grand", "lead", "local_agent", false), t0);
            bg.on_task(&child("watch", "lead", "local_bash", true), t0);
            assert_eq!(bg.what(), "1 helper", "its own helper and command count under it");
            // Interim: it ended its turn, its helper and watcher still running; then off the list.
            assert!(bg.on_task(&notice("lead", "completed", "Waiting for the parts."), t0).is_empty());
            assert!(bg.on_task(&Task::Background { tasks: vec!["grand".into(), "watch".into()] }, t0).is_empty());
            assert_eq!((bg.pending(), bg.paused(), bg.runs_call("toolu_lead")), (true, 1, true));
            // Its helper's end: nothing of the session's own; the watcher still keeps it.
            assert!(bg.on_task(&notice("grand", "completed", "North bed: beans."), t0).is_empty());
            assert!(bg.settle(t0 + PAUSE_GRACE * 2).is_empty(), "not alone yet");
            // The watcher's end: the list empty for a moment, the lead still waited for.
            assert!(bg.on_task(&notice("watch", "completed", ""), t0).is_empty());
            assert!(bg.on_task(&Task::Background { tasks: vec![] }, t0).is_empty());
            assert!(bg.pending());
            if started_again {
                bg.on_task(&Task::Background { tasks: vec!["lead".into()] }, t0);
                bg.on_task(&started("lead", "toolu_lead", "local_agent", true, false), t0);
                assert_eq!(bg.paused(), 0);
                // Its later notice, nothing of its own left: the real end.
                let ended = bg.on_task(&notice("lead", "completed", "Merged."), t0);
                assert_eq!(ended.iter().map(|e| e.summary.as_str()).collect::<Vec<_>>(), ["Merged."]);
                bg.on_task(&Task::Background { tasks: vec![] }, t0);
            } else {
                // Never started again: done after the grace, its interim words its last.
                assert!(bg.settle(t0 + PAUSE_GRACE - Duration::from_secs(1)).is_empty());
                let ended = bg.settle(t0 + PAUSE_GRACE);
                assert_eq!(ended.iter().map(|e| e.summary.as_str()).collect::<Vec<_>>(), ["Waiting for the parts."]);
            }
            assert!(!bg.pending(), "{:?}", bg.counts());
        }
    }

    #[test]
    fn counted_in_words_and_a_call_that_failed_started_nothing() {
        let now = Instant::now();
        let mut bg = Background::default();
        for (task, call) in [("a1", "c1"), ("a2", "c2")] {
            bg.on_call(call, "Task", &json!({ "run_in_background": true }));
            bg.on_task(&started(task, call, "", true, false), now);
        }
        assert_eq!(bg.what(), "2 helpers");
        bg.on_call("c3", "Monitor", &json!({ "run_in_background": true }));
        bg.on_task(&started("m1", "c3", "monitor", true, false), now);
        assert_eq!(bg.what(), "2 helpers, 1 task");
        // A helper's own background command naming no helper: still not the session's own.
        bg.on_task(&started("h1", "c9", "local_bash", true, true), now);
        assert_eq!((bg.what().as_str(), bg.counts().1), ("2 helpers, 1 task", 4));
        // A call that came back with an error: its asking is forgotten.
        bg.on_call("c4", "Bash", &json!({ "run_in_background": true }));
        bg.on_call_back("c4", true, None);
        assert!(bg.asked.is_empty());
        assert!(!asks_background(&json!({ "run_in_background": false })) && !asks_background(&json!({})));
    }

    #[test]
    fn a_wake_up_is_waited_for_till_its_turn_and_let_go_long_past_due() {
        let mut bg = Background::default();
        let wake = |bg: &mut Background, call: &str, record: Option<serde_json::Value>| {
            bg.on_call(call, WAKE_TOOL, &json!({ "delaySeconds": 60, "prompt": "Check the build." }));
            bg.on_call_back(call, false, record.as_ref());
        };
        wake(&mut bg, "w1", Some(json!({ "scheduledFor": 1_000_000u64, "clampedDelaySeconds": 60, "wasClamped": false })));
        // One that came back with no record, or another call's record: nothing to wait for.
        wake(&mut bg, "w2", None);
        bg.on_call_back("toolu_other", false, Some(&json!({ "scheduledFor": 5u64 })));
        assert_eq!(bg.what(), "a wake-up");
        assert!(bg.pending());
        assert_eq!(bg.line(), "Waiting for background work (a wake-up)");
        bg.on_lifecycle(false);
        assert!(bg.pending(), "a turn's end isn't a wake-up starting");
        bg.on_lifecycle(true);
        assert!(!bg.pending());
        wake(&mut bg, "w3", Some(json!({ "scheduledFor": 1_000_000u64 })));
        wake(&mut bg, "w4", Some(json!({ "scheduledFor": 2_000_000u64 })));
        assert_eq!(bg.what(), "2 wake-ups");
        assert!(!bg.expire(1_000_000 + WAKE_LATE_MS - 1));
        assert!(bg.expire(1_000_000 + WAKE_LATE_MS));
        assert_eq!(bg.what(), "a wake-up");
    }
}

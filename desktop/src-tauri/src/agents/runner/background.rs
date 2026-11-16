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
//! - **A start**: the session's own call asking for the background
//!   (`run_in_background` in its input, [`Background::on_call`]), made a
//!   task by Claude Code (`task_started` with `is_backgrounded`, naming that
//!   call; a helper's own commands aren't the session's). A Claude Code that
//!   says nothing of its tasks has none here: nothing waits.
//! - **An end**: its `task_notification` (completed, failed, stopped…), or
//!   `background_tasks_changed` no longer listing it ([`Background::on_task`]).
//! - **A wake-up**: due when its call's record says (`scheduledFor`,
//!   [`Background::on_call_back`]); its turn is the next `command_lifecycle`
//!   Claude Code starts. One long overdue is let go ([`Background::expire`]).
//!
//! Pure: the runner feeds it Claude Code's events.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::ai::claude_code::events::Task;

/// A wake-up this long past due with no turn of Claude Code's own is let go.
const WAKE_LATE_MS: u64 = 5 * 60 * 1000;

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

/// **The session's own work in the background**, as Claude Code's stream says it.
#[derive(Debug, Default)]
pub struct Background {
    /// Its own calls that asked for the background, by call id, till their task starts.
    asked: HashMap<String, Kind>,
    /// Its tasks in the background now, by task id: their call and kind.
    running: HashMap<String, (String, Kind)>,
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

    /// **One of Claude Code's task events**: a task of the session's own in
    /// the background begins, or ends (the ones that ended, returned).
    pub fn on_task(&mut self, t: &Task) -> Vec<Ended> {
        match t {
            Task::Started { task_id, tool_use_id, task_type, background, by_helper } => {
                self.seen.insert(task_id.clone());
                let call = self.asked.remove(tool_use_id);
                if *background && !*by_helper && !task_id.is_empty() {
                    self.running.insert(task_id.clone(), (tool_use_id.clone(), kind_of_task(task_type, call)));
                }
                Vec::new()
            }
            // (Its end is said by the notice that follows, with Claude Code's words.)
            Task::Updated { .. } => Vec::new(),
            Task::Notice { task_id, status, summary, .. } => match self.running.remove(task_id) {
                Some((call, kind)) => vec![Ended { call, kind, status: status.clone(), summary: summary.clone() }],
                None => Vec::new(),
            },
            Task::Background { tasks } => {
                let gone: Vec<String> = self.running.keys().filter(|id| !tasks.contains(id)).cloned().collect();
                gone.into_iter().filter_map(|id| self.running.remove(&id)).map(|(call, kind)| Ended { call, kind, status: String::new(), summary: String::new() }).collect()
            }
        }
    }

    /// **A task an earlier process left, that Claude Code says stopped** (a
    /// resumed session's notice): one this process never saw start.
    pub fn stopped_before(&self, t: &Task) -> bool {
        matches!(t, Task::Notice { task_id, status, .. } if status == "stopped" && !self.seen.contains(task_id))
    }

    /// **Whether a call's task runs in the background now**: its helper's
    /// row stays at work after the call came back.
    pub fn runs_call(&self, call: &str) -> bool {
        self.running.values().any(|(c, _)| c == call)
    }

    /// **Work still to report back**: a task in the background, or a wake-up.
    pub fn pending(&self) -> bool {
        !self.running.is_empty() || !self.wakes.is_empty()
    }

    /// **What it waits for, in words**: "1 helper", "2 commands", "1 helper,
    /// 1 command", "a wake-up" — "" with nothing.
    pub fn what(&self) -> String {
        let count = |k: Kind| self.running.values().filter(|(_, kind)| *kind == k).count();
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
        Task::Started { task_id: task.into(), tool_use_id: call.into(), task_type: task_type.into(), background, by_helper }
    }

    #[test]
    fn a_helper_and_a_command_in_the_background_are_waited_for_until_each_reports_back() {
        let mut bg = Background::default();
        assert!(!bg.pending());
        assert_eq!(bg.line(), "Waiting for background work");
        // Asked for in the call's input (a string "true" too), made a task by Claude Code.
        bg.on_call("toolu_helper", "Agent", &json!({ "description": "Compare seed suppliers", "run_in_background": true }));
        bg.on_call("toolu_cmd", "PowerShell", &json!({ "command": "npm run build", "run_in_background": "true" }));
        bg.on_call("toolu_fg", "PowerShell", &json!({ "command": "ls" }));
        assert!(!bg.pending(), "nothing runs till Claude Code says it does");
        assert!(bg.on_task(&Task::Background { tasks: vec!["a1".into()] }).is_empty());
        bg.on_task(&started("a1", "toolu_helper", "local_agent", true, false));
        bg.on_task(&started("b1", "toolu_cmd", "local_bash", true, false));
        // A foreground command's task, and a helper's own: not the session's background.
        bg.on_task(&started("b2", "toolu_fg", "local_bash", false, false));
        bg.on_task(&started("b3", "toolu_inner", "local_bash", true, true));
        assert!(bg.pending());
        assert_eq!(bg.line(), "Waiting for background work (1 helper, 1 command)");
        assert!(bg.runs_call("toolu_helper") && bg.runs_call("toolu_cmd") && !bg.runs_call("toolu_fg"));
        // The helper's end: with what it came back with.
        assert!(bg.on_task(&Task::Updated { task_id: "a1".into(), status: Some("completed".into()) }).is_empty());
        let ended = bg.on_task(&Task::Notice { task_id: "a1".into(), tool_use_id: "toolu_helper".into(), status: "completed".into(), summary: "Hedgerow is cheapest.".into() });
        assert_eq!(ended, [Ended { call: "toolu_helper".into(), kind: Kind::Helper, status: "completed".into(), summary: "Hedgerow is cheapest.".into() }]);
        assert_eq!(bg.line(), "Waiting for background work (1 command)");
        // The command, gone from the list Claude Code keeps.
        let ended = bg.on_task(&Task::Background { tasks: vec![] });
        assert_eq!(ended, [Ended { call: "toolu_cmd".into(), kind: Kind::Command, status: String::new(), summary: String::new() }]);
        assert!(!bg.pending());
        // A notice of a task it never saw start (an earlier process's): nothing of this turn's, said as stopped before.
        let old = Task::Notice { task_id: "old".into(), tool_use_id: "toolu_old".into(), status: "stopped".into(), summary: String::new() };
        assert!(bg.stopped_before(&old));
        assert!(bg.on_task(&old).is_empty());
        // One it saw start — a helper's own command stopped with it — never is.
        assert!(!bg.stopped_before(&Task::Notice { task_id: "b3".into(), tool_use_id: "toolu_inner".into(), status: "stopped".into(), summary: String::new() }));
        assert!(!bg.stopped_before(&Task::Notice { task_id: "old2".into(), tool_use_id: String::new(), status: "completed".into(), summary: String::new() }), "only a stop");
    }

    #[test]
    fn counted_in_words_and_a_call_that_failed_started_nothing() {
        let mut bg = Background::default();
        for (task, call) in [("a1", "c1"), ("a2", "c2")] {
            bg.on_call(call, "Task", &json!({ "run_in_background": true }));
            bg.on_task(&started(task, call, "", true, false));
        }
        assert_eq!(bg.what(), "2 helpers");
        bg.on_call("c3", "Monitor", &json!({ "run_in_background": true }));
        bg.on_task(&started("m1", "c3", "monitor", true, false));
        assert_eq!(bg.what(), "2 helpers, 1 task");
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

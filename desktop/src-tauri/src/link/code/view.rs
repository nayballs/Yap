//! **What the Code side's messages carry**, shaped from Yap Code's own
//! records (Remote Control, 2026-11-08): an agent in brief, its snapshot
//! and pages of its log, a file's diff, the cards. Bounded as the wire
//! says (`wire::SNAPSHOT_EVENTS`, `DIFF_LINES`, `LIVE_DIFF_LINES`,
//! `MAX_CODE_FRAME`). Pure.

use serde_json::Value;

use crate::agents::store::Agent;
use crate::link::wire::{self, Brief, BriefTurn, Logged};

/// **An agent in brief** (`line`: the line it's on, when known).
pub fn brief(agent: &Agent, line: Option<&str>) -> Brief {
    let notes = agent.turn_notes();
    let now = line.map(str::to_string).or_else(|| Some(agent.step.clone())).filter(|s| !s.trim().is_empty());
    Brief {
        id: agent.id.clone(),
        conversation: agent.conversation,
        folder: agent.folder.clone(),
        status: agent.status.word().to_string(),
        now: if agent.status.live() { now } else { None },
        turn_title: notes.last().map(|t| t.title.clone()).filter(|t| !t.is_empty()),
        started_ts: agent.started_ts,
        ended_ts: agent.ended_ts,
        asks: agent.asks,
        // (A read-only turn is never a plan waiting for a go-ahead.)
        plan: agent.plan && !notes.last().is_some_and(|t| t.read_only),
        plan_allowed: agent.plan_allowed,
        plan_unused: agent.plan_unused,
        message_id: agent.message_id.clone(),
        report_message_id: agent.report_message_id.clone(),
        turn_list: notes
            .iter()
            .map(|t| BriefTurn {
                n: t.n,
                title: t.title.clone(),
                status: t.status.word().to_string(),
                message_id: t.message_id.clone(),
                report_message_id: t.report_message_id.clone(),
            })
            .collect(),
    }
}

/// **The agents a phone's lists and dots need**: those not over, and those
/// that ended in the last day (a plan or a question still waits there),
/// newest first, 50 at most. Pure.
pub fn worth_listing(agents: &[Agent], now: u64) -> Vec<&Agent> {
    let mut out: Vec<&Agent> = agents.iter().filter(|a| a.status.live() || now.saturating_sub(a.ended_ts.max(a.updated_ts)) < 24 * 3600).collect();
    out.sort_by_key(|a| std::cmp::Reverse((a.created_ts, a.id.clone())));
    out.truncate(50);
    out
}

/// What a command printed is for its row in the PC's thread: a phone's view
/// has no such row, so its text never goes (`output` null). Pure.
fn without_output(event: &mut Value) {
    if event.get("output").is_some_and(|o| !o.is_null()) {
        event["output"] = Value::Null;
    }
}

/// A live event as the phone gets it: an edit's lines past
/// [`wire::LIVE_DIFF_LINES`] left out (`lines` null, `more`), asked for
/// with `agent_diff`; a command's output never. Pure.
pub fn live(mut event: Value) -> Value {
    without_output(&mut event);
    if let Some(d) = event.get_mut("diff").and_then(Value::as_object_mut) {
        if d.get("lines").and_then(Value::as_array).is_some_and(|l| l.len() > wire::LIVE_DIFF_LINES) {
            d.insert("lines".into(), Value::Null);
            d.insert("more".into(), Value::Bool(true));
        }
    }
    event
}

/// An event in a snapshot or a page: its diff's lines left out, its counts
/// kept; a command's output never. Pure.
pub fn stripped(mut event: Value) -> Value {
    without_output(&mut event);
    if let Some(d) = event.get_mut("diff").and_then(Value::as_object_mut) {
        if d.get("lines").is_some_and(|l| !l.is_null()) {
            d.insert("lines".into(), Value::Null);
            d.insert("more".into(), Value::Bool(true));
        }
    }
    event
}

/// How long `logged` is as JSON.
fn size(logged: &[Logged]) -> usize {
    logged.iter().map(|l| l.event.to_string().len() + 24).sum()
}

/// The newest of `lines` (`(line, event)`, oldest first), at most `most`
/// and within `room` bytes, stripped: and whether older ones were left out.
/// Pure.
fn newest(lines: Vec<(u64, Value)>, most: usize, room: usize) -> (Vec<Logged>, bool) {
    let total = lines.len();
    let mut out: Vec<Logged> = lines.into_iter().rev().take(most).map(|(line, event)| Logged { line, event: stripped(event) }).collect();
    out.reverse();
    while !out.is_empty() && size(&out) > room {
        out.remove(0);
    }
    let more = out.len() < total;
    (out, more)
}

/// Room for the events in a Code frame (the rest of it is the agent and
/// the message's own fields).
const EVENTS_ROOM: usize = wire::MAX_CODE_FRAME - 8 * 1024;

/// **An agent's snapshot** (`agent_open`): its record (its words trimmed to
/// fit), its last [`wire::SNAPSHOT_EVENTS`] events (each with its log line),
/// stripped, within a frame; whether older ones were left out. Pure.
pub fn snapshot(agent: &Agent, lines: Vec<(u64, Value)>) -> (Value, Vec<Logged>, bool) {
    let mut record = serde_json::to_value(agent).unwrap_or(Value::Null);
    if let Some(o) = record.as_object_mut() {
        for key in ["text", "brief"] {
            if let Some(Value::String(s)) = o.get_mut(key) {
                if s.len() > 3_000 {
                    *s = crate::agents::steps::clip(s, 3_000);
                }
            }
        }
    }
    let (events, more) = newest(lines, wire::SNAPSHOT_EVENTS, EVENTS_ROOM);
    (record, events, more)
}

/// **An older page** (`agent_log`): the events before line `before` (0:
/// the newest), as a snapshot's are. Pure.
pub fn page_before(lines: Vec<(u64, Value)>, before: u64) -> (Vec<Logged>, bool) {
    let older: Vec<(u64, Value)> = lines.into_iter().filter(|(l, _)| before == 0 || *l < before).collect();
    newest(older, wire::SNAPSHOT_EVENTS, EVENTS_ROOM)
}

/// **One file's diff** (`agent_diff`): every edit of `file` (in `turn`, else
/// every turn), its lines one after another with a gap (`@`) between edits,
/// at most [`wire::DIFF_LINES`]; whether there were more; lines added and
/// removed in all. Pure.
pub fn diff_of(lines: &[(u64, Value)], file: &str, turn: Option<u32>) -> (Vec<String>, bool, u32, u32) {
    let mut out: Vec<String> = Vec::new();
    let (mut added, mut removed) = (0u32, 0u32);
    for (_, e) in lines {
        let Some(d) = e.get("diff") else { continue };
        if d.get("file").and_then(Value::as_str) != Some(file) {
            continue;
        }
        if let Some(t) = turn {
            if e.get("turn").and_then(Value::as_u64) != Some(u64::from(t)) {
                continue;
            }
        }
        added += d.get("added").and_then(Value::as_u64).unwrap_or(0) as u32;
        removed += d.get("removed").and_then(Value::as_u64).unwrap_or(0) as u32;
        if !out.is_empty() {
            out.push("@".into());
        }
        out.extend(d.get("lines").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string));
    }
    let more = out.len() > wire::DIFF_LINES;
    out.truncate(wire::DIFF_LINES);
    (out, more, added, removed)
}

/// A card's longest detail (a plan's 20,000 characters fit a frame).
const DETAIL_ROOM: usize = wire::MAX_CODE_FRAME - 4 * 1024;

/// **A permission card for the phone** (`approve_card`): the card as the
/// PC's own, its detail cut to fit a frame. Pure.
pub fn card(mut card: Value) -> Value {
    if let Some(Value::String(d)) = card.get_mut("detail") {
        if d.len() > DETAIL_ROOM {
            let mut cut = DETAIL_ROOM;
            while !d.is_char_boundary(cut) {
                cut -= 1;
            }
            d.truncate(cut);
            d.push('…');
        }
    }
    card
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::store::{Status, TurnNote};
    use serde_json::json;

    fn agent() -> Agent {
        Agent {
            id: "ab12".into(),
            conversation: Some(12),
            folder: "garden-planner".into(),
            path: r"D:\Projects\garden-planner".into(),
            status: Status::Running,
            message_id: Some("chat:12:0".into()),
            step: "Reading notes.md".into(),
            started_ts: 100,
            created_ts: 90,
            turn_list: vec![TurnNote { n: 1, title: "Add a watering schedule".into(), message_id: Some("chat:12:0".into()), status: Status::Running, ..Default::default() }],
            ..Default::default()
        }
    }

    #[test]
    fn an_agent_in_brief_reads_as_the_dots_and_pills_read_one() {
        let b = brief(&agent(), Some("Adding a watering schedule"));
        assert_eq!((b.status.as_str(), b.now.as_deref(), b.turn_title.as_deref()), ("running", Some("Adding a watering schedule"), Some("Add a watering schedule")));
        assert_eq!(b.turn_list[0], BriefTurn { n: 1, title: "Add a watering schedule".into(), status: "running".into(), message_id: Some("chat:12:0".into()), report_message_id: None });
        assert_eq!(brief(&agent(), None).now.as_deref(), Some("Reading notes.md"), "its own step");
        let done = Agent { status: Status::Done, plan: true, ended_ts: 200, ..agent() };
        let b = brief(&done, Some("x"));
        assert!(b.now.is_none() && b.plan);
        let v = serde_json::to_value(&b).unwrap();
        for k in ["turnList", "messageId", "startedTs", "endedTs", "planAllowed"] {
            assert!(v.get(k).is_some(), "{k}");
        }
        // A plan set aside ("Plan not used") says so; one waiting says nothing more.
        assert!(v.get("planUnused").is_none());
        let aside = serde_json::to_value(brief(&Agent { plan_unused: true, ..done.clone() }, None)).unwrap();
        assert_eq!(aside["planUnused"], true);
        // Not over, or over within the day, newest first.
        let old = Agent { id: "old1".into(), status: Status::Done, ended_ts: 10, updated_ts: 10, created_ts: 5, ..agent() };
        let recent = Agent { id: "new1".into(), status: Status::Done, ended_ts: 100_000, updated_ts: 100_000, created_ts: 99_000, ..agent() };
        let all = vec![old, recent, agent()];
        let ids: Vec<&str> = worth_listing(&all, 100_100).iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["new1", "ab12"]);
    }

    #[test]
    fn live_events_keep_small_diffs_and_snapshots_keep_none() {
        let small = json!({ "id": "ab12", "diff": { "file": "notes.md", "lines": ["-Draft", "+Final"], "added": 1, "removed": 1 } });
        assert_eq!(live(small.clone()), small);
        let lines: Vec<String> = (0..wire::LIVE_DIFF_LINES + 1).map(|i| format!("+line {i}")).collect();
        let big = json!({ "id": "ab12", "diff": { "file": "notes.md", "lines": lines, "added": 41, "removed": 0 } });
        let cut = live(big);
        assert_eq!(cut["diff"]["lines"], Value::Null);
        assert_eq!((cut["diff"]["more"].as_bool(), cut["diff"]["added"].as_u64()), (Some(true), Some(41)), "counts kept");
        let s = stripped(small);
        assert_eq!((s["diff"]["lines"].is_null(), s["diff"]["removed"].as_u64()), (true, Some(1)));
        assert_eq!(stripped(json!({ "step": "x" })), json!({ "step": "x" }));
        // What a command printed never goes to a phone, live or in a snapshot.
        let printed = json!({ "id": "ab12", "call": "toolu_1", "output": { "text": "3 passed" } });
        assert_eq!(live(printed.clone()), json!({ "id": "ab12", "call": "toolu_1", "output": null }));
        assert_eq!(stripped(printed), json!({ "id": "ab12", "call": "toolu_1", "output": null }));
    }

    #[test]
    fn a_snapshot_is_the_last_200_within_a_frame_and_pages_go_back_from_there() {
        let lines: Vec<(u64, Value)> = (1..=250).map(|i| (i, json!({ "id": "ab12", "step": format!("Step {i}") }))).collect();
        let (record, events, more) = snapshot(&agent(), lines.clone());
        assert_eq!(record["id"], "ab12");
        assert_eq!(events.len(), wire::SNAPSHOT_EVENTS);
        assert!(more);
        assert_eq!((events[0].line, events.last().unwrap().line), (51, 250));
        // The page before it.
        let (page, more) = page_before(lines.clone(), 51);
        assert_eq!((page.len(), page[0].line, page.last().unwrap().line, more), (50, 1, 50, false));
        // A frame's worth at most: big texts leave the oldest out.
        let big: Vec<(u64, Value)> = (1..=40).map(|i| (i, json!({ "id": "ab12", "said": "x".repeat(4_000) }))).collect();
        let (_, events, more) = snapshot(&agent(), big);
        assert!(more && events.len() < 40);
        assert!(size(&events) <= EVENTS_ROOM);
        assert_eq!(events.last().unwrap().line, 40, "the newest kept");
        // A long brief is trimmed in the record.
        let (record, _, _) = snapshot(&Agent { brief: "b".repeat(5_000), ..agent() }, vec![]);
        assert!(record["brief"].as_str().unwrap().chars().count() <= 3_000);
    }

    #[test]
    fn a_files_diff_is_its_edits_together_capped() {
        let lines = vec![
            (1, json!({ "turn": 1, "diff": { "file": "notes.md", "lines": ["-Draft", "+Final"], "added": 1, "removed": 1 } })),
            (2, json!({ "turn": 1, "diff": { "file": "app.ts", "lines": ["+x"], "added": 1, "removed": 0 } })),
            (3, json!({ "turn": 2, "diff": { "file": "notes.md", "lines": ["+Watering: Sundays"], "added": 1, "removed": 0 } })),
            (4, json!({ "step": "Running the tests" })),
        ];
        assert_eq!(diff_of(&lines, "notes.md", None), (vec!["-Draft".into(), "+Final".into(), "@".into(), "+Watering: Sundays".into()], false, 2, 1));
        assert_eq!(diff_of(&lines, "notes.md", Some(2)), (vec!["+Watering: Sundays".into()], false, 1, 0));
        assert_eq!(diff_of(&lines, "missing.md", None), (vec![], false, 0, 0));
        let many: Vec<String> = (0..wire::DIFF_LINES + 10).map(|i| format!("+{i}")).collect();
        let long = vec![(1, json!({ "diff": { "file": "big.md", "lines": many, "added": 410, "removed": 0 } }))];
        let (out, more, added, _) = diff_of(&long, "big.md", None);
        assert_eq!((out.len(), more, added), (wire::DIFF_LINES, true, 410));
    }

    #[test]
    fn a_card_fits_a_frame() {
        let c = json!({ "id": 7, "summary": "The plan is ready", "detail": "é".repeat(40_000) });
        let cut = card(c);
        assert!(cut["detail"].as_str().unwrap().len() <= DETAIL_ROOM + 4);
        assert!(cut["detail"].as_str().unwrap().ends_with('…'));
        let small = json!({ "id": 7, "detail": "npm test" });
        assert_eq!(card(small.clone()), small);
    }
}

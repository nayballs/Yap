//! Tasks: the checkboxes in a note's summary — the action plan's
//! "- [ ] task (due: …)" lines under a "### Owner" heading
//! (`llm::ACTION_PLAN_DEFAULT_FRAGMENT`, checked by
//! `meeting_summary::postcheck_action_plan`), "- [ ] Dave: task" lines (the
//! Action Items action), and any other "- [ ]" a summary has.
//!
//! - **Ticked in place**: the summary's markdown is the one store — "- [x]"
//!   once done — so a tick in the notepad, the Notes view or the Tasks view
//!   is the same tick everywhere, Copy as Markdown and the local API read
//!   it, and the pre-meeting brief's open items leave it out
//!   (`calendar::brief::plan_tasks` reads only "- [ ]"). [`note_task_set`]
//!   finds the line by its number and words, so a summary edited meanwhile
//!   still gets the right one (or a clear refusal).
//! - **Kept through a new summary**: [`carry_ticks`] ticks the new
//!   summary's tasks that were ticked in the old one (the same words, or
//!   most of them, and the same owner). A tick never counts as editing the
//!   summary ([`same_but_ticks`]).
//! - **The Tasks view** ([`tasks_list`]): every note's tasks, with who,
//!   when it's due, and the moment it was said (the talk line sharing the
//!   most of its words, [`moment`]), for "By meeting" and "By person"
//!   lists. Read only through the local API (`GET /v1/tasks/list`) and MCP
//!   (`list_tasks`).
//!
//! Parsing is line by line and pure (unit-tested); fenced code is skipped.
//! Logs carry ids and counts, never a task's words.

use std::collections::HashSet;

use serde::Serialize;

use crate::notes::Note;

/// One task in a summary.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    /// Its line in the summary (0-based, lines split at `\n`).
    pub line: usize,
    /// The line's words after the checkbox, as written (what a tick checks
    /// it against).
    pub text: String,
    /// What to do: the words without the owner, the deadline or `**`.
    pub task: String,
    /// "You", a name, "Everyone", "Unassigned", or "" (nobody said).
    pub owner: String,
    /// The deadline as written ("Friday"), or "".
    pub due: String,
    pub done: bool,
}

/// A checkbox line: the byte where its mark is (' ' or 'x'), whether it's
/// ticked, and its words. `- [ ] words`, `* [x] words`, `+ [X] words`,
/// `• [ ] words` (the renderer's "• " bullets), any indent; the words can't
/// be empty (markdown makes no checkbox of those).
fn checkbox(line: &str) -> Option<(usize, bool, &str)> {
    let indent = line.len() - line.trim_start().len();
    let rest = &line[indent..];
    let bullet = rest.chars().next()?;
    if !matches!(bullet, '-' | '*' | '+' | '\u{2022}') {
        return None;
    }
    let after = &rest[bullet.len_utf8()..];
    let gap = after.len() - after.trim_start_matches([' ', '\t']).len();
    if gap == 0 {
        return None;
    }
    let boxed = &after[gap..];
    let bytes = boxed.as_bytes();
    if bytes.len() < 4 || bytes[0] != b'[' || bytes[2] != b']' || !matches!(bytes[3], b' ' | b'\t') {
        return None;
    }
    let done = match bytes[1] {
        b' ' => false,
        b'x' | b'X' => true,
        _ => return None,
    };
    let words = boxed[4..].trim();
    if words.is_empty() {
        return None;
    }
    Some((indent + bullet.len_utf8() + gap + 1, done, words))
}

/// Headings that name a section, never a person ("### Action items").
const NOT_OWNERS: [&str; 18] = [
    "action", "actions", "item", "items", "task", "tasks", "step", "steps", "next", "follow", "follow-up",
    "followups", "decision", "decisions", "question", "questions", "summary", "notes",
];

/// "you" → "You", "nobody" → "Unassigned", "the team" → "Everyone"; a name as written.
fn owner_name(raw: &str) -> String {
    let o = raw.trim().trim_matches('*').trim().trim_end_matches(':').trim();
    match o.to_lowercase().as_str() {
        "you" | "me" | "i" | "myself" => "You".to_string(),
        "unassigned" | "nobody" | "no one" | "no owner" | "tbd" => "Unassigned".to_string(),
        "everyone" | "everybody" | "all" | "team" | "the team" => "Everyone".to_string(),
        _ => o.to_string(),
    }
}

/// A "### Name" heading that names who the tasks under it are for.
fn heading_owner(title: &str) -> Option<String> {
    let owner = owner_name(title);
    if matches!(owner.as_str(), "You" | "Unassigned" | "Everyone") {
        return Some(owner);
    }
    let words: Vec<&str> = owner.split_whitespace().collect();
    let name_like = !words.is_empty()
        && words.len() <= 4
        && words.iter().all(|w| {
            let lower = w.to_lowercase();
            !NOT_OWNERS.contains(&lower.trim_matches(|c: char| !c.is_alphanumeric() && c != '-'))
        });
    name_like.then_some(owner)
}

/// "Priya: book the venue" → ("Priya", "book the venue"): an owner of up to
/// three words written like a name, or You.
fn inline_owner(text: &str) -> Option<(String, &str)> {
    let (who, rest) = text.split_once(':')?;
    let who = who.trim().trim_matches('*').trim();
    let rest = rest.trim().trim_start_matches('*').trim();
    if rest.is_empty() {
        return None;
    }
    let owner = owner_name(who);
    if matches!(owner.as_str(), "You" | "Unassigned" | "Everyone") {
        return Some((owner, rest));
    }
    let words: Vec<&str> = who.split_whitespace().collect();
    let name_like = !words.is_empty()
        && words.len() <= 3
        && words.iter().all(|w| w.chars().next().is_some_and(char::is_uppercase) && !w.chars().any(|c| c.is_ascii_digit()));
    name_like.then_some((owner, rest))
}

/// "send the budget (due: Friday)." → ("send the budget", "Friday").
fn split_due(text: &str) -> (String, String) {
    let at = (0..text.len())
        .rev()
        .find(|&i| text.get(i..i + 5).is_some_and(|s| s.eq_ignore_ascii_case("(due:")));
    let Some(at) = at else {
        return (text.trim().to_string(), String::new());
    };
    let inner = &text[at + 5..];
    let (due, after) = match inner.find(')') {
        Some(end) => (&inner[..end], &inner[end + 1..]),
        None => (inner, ""),
    };
    let before = text[..at].trim_end();
    let after = after.trim().trim_start_matches(['.', ',']).trim();
    let task = if after.is_empty() {
        before.trim_end_matches(['.', ',']).trim().to_string()
    } else {
        format!("{before} {after}")
    };
    (task, due.trim().to_string())
}

/// Every task in a summary, in order.
pub fn parse(md: &str) -> Vec<Task> {
    let mut out = Vec::new();
    let mut fence: Option<char> = None;
    let mut heading: Option<String> = None;
    let mut unassigned = false;
    let mut notes_section = false;
    for (i, line) in md.split('\n').enumerate() {
        let t = line.trim();
        if t.starts_with("```") || t.starts_with("~~~") {
            let mark = t.chars().next().unwrap_or('`');
            fence = match fence {
                None => Some(mark),
                Some(open) if open == mark => None,
                other => other,
            };
            continue;
        }
        if fence.is_some() {
            continue;
        }
        if let Some(title) = t.strip_prefix("## ") {
            let lower = title.to_lowercase();
            unassigned = lower.contains("unassigned");
            notes_section = lower.contains("decision") || lower.contains("question");
            heading = None;
            continue;
        }
        if let Some(title) = t.strip_prefix("### ") {
            heading = if notes_section { None } else { heading_owner(title) };
            continue;
        }
        let Some((_, done, text)) = checkbox(line) else { continue };
        let (owner, rest) = match inline_owner(text) {
            Some((who, rest)) if !unassigned => (who, rest),
            _ if unassigned => ("Unassigned".to_string(), text),
            _ => (heading.clone().unwrap_or_default(), text),
        };
        let (task, due) = split_due(rest);
        let task = task.replace("**", "").trim().to_string();
        if task.is_empty() {
            continue;
        }
        out.push(Task { line: i, text: text.to_string(), task, owner, due, done });
    }
    out
}

/// `md` with its task at `line` (whose words are `text`) ticked (`done`) or
/// not. When the summary changed and that line isn't the task any more, the
/// first task with those words. Unchanged when it's already so.
pub fn set_done(md: &str, line: usize, text: &str, done: bool) -> Result<String, String> {
    let tasks = parse(md);
    let want = text.trim();
    let task = tasks
        .iter()
        .find(|t| t.line == line && t.text == want)
        .or_else(|| tasks.iter().find(|t| t.text == want))
        .ok_or("That task isn't in the summary any more")?;
    if task.done == done {
        return Ok(md.to_string());
    }
    Ok(with_mark(md, &[task.line], done))
}

/// `md` with the checkboxes on `lines` set to `done`.
fn with_mark(md: &str, lines: &[usize], done: bool) -> String {
    md.split('\n')
        .enumerate()
        .map(|(i, l)| match checkbox(l) {
            Some((at, _, _)) if lines.contains(&i) => {
                let mut s = String::with_capacity(l.len());
                s.push_str(&l[..at]);
                s.push(if done { 'x' } else { ' ' });
                s.push_str(&l[at + 1..]);
                s
            }
            _ => l.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `a` and `b` say the same but for which tasks are ticked.
pub fn same_but_ticks(a: &str, b: &str) -> bool {
    let clear = |md: &str| {
        let lines: Vec<usize> = parse(md).iter().map(|t| t.line).collect();
        with_mark(md, &lines, false)
    };
    a.split('\n').count() == b.split('\n').count() && clear(a) == clear(b)
}

const STOP: [&str; 16] = [
    "the", "and", "for", "with", "our", "their", "that", "this", "from", "into", "will", "about", "due", "out",
    "get", "all",
];

/// A task's content words (lowercased, a plural's "s" off).
fn task_words(s: &str) -> HashSet<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|w| w.chars().count() > 2 && !STOP.contains(&w.as_str()))
        .map(|w| if w.len() > 4 && w.ends_with('s') && !w.ends_with("ss") { w[..w.len() - 1].to_string() } else { w })
        .collect()
}

/// The same task in two summaries: the same owner (or one not said) and
/// the same words, or most of them (six in ten of the longer's).
fn same_task(a: &Task, b: &Task) -> bool {
    let owners = a.owner.is_empty() || b.owner.is_empty() || a.owner.eq_ignore_ascii_case(&b.owner);
    if !owners {
        return false;
    }
    if a.task.trim().eq_ignore_ascii_case(b.task.trim()) {
        return true;
    }
    let (x, y) = (task_words(&a.task), task_words(&b.task));
    if x.is_empty() || y.is_empty() {
        return false;
    }
    let shared = x.intersection(&y).count();
    shared * 10 >= x.len().max(y.len()) * 6
}

/// `new` (a summary Yap just wrote) with the tasks ticked in `old` ticked
/// again.
pub fn carry_ticks(old: &str, new: &str) -> String {
    let done: Vec<Task> = parse(old).into_iter().filter(|t| t.done).collect();
    if done.is_empty() {
        return new.to_string();
    }
    let lines: Vec<usize> = parse(new)
        .iter()
        .filter(|t| !t.done && done.iter().any(|d| same_task(d, t)))
        .map(|t| t.line)
        .collect();
    if lines.is_empty() {
        return new.to_string();
    }
    with_mark(new, &lines, true)
}

/// Where in `note`'s meeting a task was most likely said: the talk line
/// sharing the most of its words (at least two, and half of them; the
/// earliest of equals).
pub fn moment(note: &Note, task: &str) -> Option<usize> {
    let want = task_words(task);
    if want.len() < 2 {
        return None;
    }
    let need = want.len().div_ceil(2).max(2);
    let mut best: Option<(usize, usize)> = None;
    for (i, s) in note.transcript.iter().enumerate() {
        if !s.is_talk() || s.text.is_empty() {
            continue;
        }
        let hits = task_words(&s.text).intersection(&want).count();
        if hits >= need && best.is_none_or(|(_, h)| hits > h) {
            best = Some((i, hits));
        }
    }
    best.map(|(i, _)| i)
}

/// A moment in a meeting: its transcript line, and how far into the
/// meeting it was (seconds from its first line).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Moment {
    pub index: usize,
    pub ts: u64,
    pub secs: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListedTask {
    #[serde(flatten)]
    pub task: Task,
    pub at: Option<Moment>,
}

/// A note's tasks, for the Tasks view.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteTasks {
    pub note_id: u64,
    pub title: String,
    pub created_ts: u64,
    pub note_type: String,
    pub tasks: Vec<ListedTask>,
}

/// Every note with tasks in its summary, newest first.
pub fn list(notes: &[Note]) -> Vec<NoteTasks> {
    let mut out: Vec<NoteTasks> = notes
        .iter()
        .filter(|n| !n.enhanced_content.trim().is_empty())
        .filter_map(|n| {
            let tasks = parse(&n.enhanced_content);
            if tasks.is_empty() {
                return None;
            }
            let t0 = n.transcript.iter().find(|s| !s.dictated).map(|s| s.ts).unwrap_or(0);
            let tasks = tasks
                .into_iter()
                .map(|task| {
                    let at = moment(n, &task.task).map(|i| Moment {
                        index: i,
                        ts: n.transcript[i].ts,
                        secs: n.transcript[i].ts.saturating_sub(t0),
                    });
                    ListedTask { task, at }
                })
                .collect();
            let title = if n.title.trim().is_empty() { "Untitled note".to_string() } else { n.title.trim().to_string() };
            Some(NoteTasks { note_id: n.id, title, created_ts: n.created_ts, note_type: n.note_type.clone(), tasks })
        })
        .collect();
    out.sort_by_key(|n| std::cmp::Reverse(n.created_ts));
    out
}

/// Every note's tasks (the Tasks view).
#[tauri::command]
pub fn tasks_list() -> Vec<NoteTasks> {
    list(&crate::notes::all())
}

/// Tick (`done`) or untick a task in note `note_id`'s summary: the one on
/// `line` whose words are `text` (`Task`). Every window follows it
/// (`yap-note-changed` with the summary). Returns the summary as it is now.
#[tauri::command]
pub fn note_task_set(
    app: tauri::AppHandle,
    note_id: u64,
    line: usize,
    text: String,
    done: bool,
    origin: Option<String>,
) -> Result<String, String> {
    let md = crate::notes::modify(note_id, |note| {
        let md = set_done(&note.enhanced_content, line, &text, done)?;
        let changed = md != note.enhanced_content;
        note.enhanced_content = md.clone();
        Ok((md, changed))
    })?;
    tracing::info!(note_id, line, done, "Task ticked");
    crate::note_edits::summary_changed(&app, note_id, origin.as_deref().unwrap_or(""));
    Ok(md)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::TranscriptSegment;

    const PLAN: &str = "The team planned the offsite.\n\n## Action plan\n\n### You\n- [ ] Draft the agenda (due: tomorrow)\n- [x] Share the slides\n\n### Priya Shah\n- [ ] Send the revised budget (due: Friday).\n\n## Decisions\n- Hold it in Lisbon\n\n## Open questions\n- Who handles catering?\n\n## Unassigned\n- [ ] Book the venue";

    #[test]
    fn a_plan_reads_as_tasks_with_owners_and_deadlines() {
        let tasks = parse(PLAN);
        let got: Vec<(usize, &str, &str, &str, bool)> =
            tasks.iter().map(|t| (t.line, t.owner.as_str(), t.task.as_str(), t.due.as_str(), t.done)).collect();
        assert_eq!(got, [
            (5, "You", "Draft the agenda", "tomorrow", false),
            (6, "You", "Share the slides", "", true),
            (9, "Priya Shah", "Send the revised budget", "Friday", false),
            (18, "Unassigned", "Book the venue", "", false),
        ]);
        assert_eq!(tasks[0].text, "Draft the agenda (due: tomorrow)");
    }

    #[test]
    fn other_layouts_read_too() {
        // The Action Items action: owners inline, under a heading that's no person.
        let md = "### Action items\n- [ ] Dave: send the budget (due: Monday)\n* [X] **You**: book the room\n+ [ ] tidy the wiki\n1. [ ] numbered isn't a task\n- [ ]\n-[ ] no space\n```\n- [ ] in code\n```\n  - [ ] nested: still a task";
        let tasks = parse(md);
        let got: Vec<(&str, &str, bool)> = tasks.iter().map(|t| (t.owner.as_str(), t.task.as_str(), t.done)).collect();
        assert_eq!(got, [
            ("Dave", "send the budget", false),
            ("You", "book the room", true),
            ("", "tidy the wiki", false),
            ("", "nested: still a task", false),
        ]);
        // "### Everyone" is an owner; "### Next steps" isn't.
        let md = "### Everyone\n- [ ] read the doc\n### Next steps\n- [ ] ship it";
        let owners: Vec<String> = parse(md).into_iter().map(|t| t.owner).collect();
        assert_eq!(owners, ["Everyone", ""]);
        assert!(parse("No action items.").is_empty());
    }

    #[test]
    fn ticking_finds_its_line_even_after_an_edit() {
        let ticked = set_done(PLAN, 5, "Draft the agenda (due: tomorrow)", true).unwrap();
        assert!(ticked.contains("- [x] Draft the agenda (due: tomorrow)"));
        assert_eq!(ticked.split('\n').count(), PLAN.split('\n').count());
        // Already so: unchanged.
        assert_eq!(set_done(&ticked, 5, "Draft the agenda (due: tomorrow)", true).unwrap(), ticked);
        let back = set_done(&ticked, 5, "Draft the agenda (due: tomorrow)", false).unwrap();
        assert_eq!(back, PLAN);
        // A line added above it meanwhile: found by its words.
        let edited = PLAN.replacen("## Action plan", "Note: rooms are tight.\n## Action plan", 1);
        let t = set_done(&edited, 9, "Send the revised budget (due: Friday).", true).unwrap();
        assert!(t.contains("- [x] Send the revised budget"));
        // Its words changed: refused.
        assert!(set_done(PLAN, 9, "Send the old budget", true).is_err());
        // CRLF summaries keep their line ends; "• " bullets tick too.
        let crlf = "- [ ] one\r\n- [ ] two";
        assert_eq!(set_done(crlf, 1, "two", true).unwrap(), "- [ ] one\r\n- [x] two");
        assert_eq!(set_done("• [ ] one", 0, "one", true).unwrap(), "• [x] one");
    }

    #[test]
    fn ticks_are_not_edits_and_survive_a_new_summary() {
        let ticked = set_done(PLAN, 9, "Send the revised budget (due: Friday).", true).unwrap();
        assert!(same_but_ticks(PLAN, &ticked));
        assert!(!same_but_ticks(PLAN, &ticked.replace("Lisbon", "Porto")));
        assert!(!same_but_ticks(PLAN, &format!("{PLAN}\n")));
        // Yap writes the plan again: the same tasks, reworded a little.
        let new = "Summary.\n\n## Action plan\n### Priya Shah\n- [ ] Send the revised budget to finance (due: Friday)\n### You\n- [ ] Draft the agenda (due: tomorrow)\n- [ ] Share the slides with the team\n### Sam\n- [ ] Send the budget";
        let carried = carry_ticks(&ticked, new);
        let done: Vec<(String, bool)> = parse(&carried).into_iter().map(|t| (t.task, t.done)).collect();
        assert_eq!(done, [
            ("Send the revised budget to finance".to_string(), true),
            ("Draft the agenda".to_string(), false),
            ("Share the slides with the team".to_string(), true),
            // Another owner's task with some of the same words: not it.
            ("Send the budget".to_string(), false),
        ]);
        assert_eq!(carry_ticks(PLAN.replace("[x]", "[ ]").as_str(), new), new);
    }

    fn line(source: &str, text: &str, ts: u64) -> TranscriptSegment {
        TranscriptSegment { source: source.into(), text: text.into(), ts, ..Default::default() }
    }

    #[test]
    fn a_task_links_back_to_where_it_was_said() {
        let mut n: Note = serde_json::from_value(serde_json::json!({
            "id": 3, "title": "Offsite", "noteType": "meeting", "createdTs": 50, "updatedTs": 50
        }))
        .unwrap();
        n.transcript = vec![
            line("you", "Morning all.", 1_000),
            line("them", "The budget is tight this year.", 1_030),
            line("them", "I will send the revised budget by Friday.", 1_090),
            line("you", "Great, send the budget then.", 1_100),
        ];
        assert_eq!(moment(&n, "Send the revised budget"), Some(2));
        assert_eq!(moment(&n, "Book the venue"), None);
        assert_eq!(moment(&n, "budget"), None, "one word is too little to go on");
        n.enhanced_content = PLAN.to_string();
        let listed = list(&[n.clone()]);
        assert_eq!(listed.len(), 1);
        let budget = listed[0].tasks.iter().find(|t| t.task.owner == "Priya Shah").unwrap();
        assert_eq!(budget.at, Some(Moment { index: 2, ts: 1_090, secs: 90 }));
        // On the wire: the task's fields beside `at`.
        let v = serde_json::to_value(budget).unwrap();
        assert_eq!(v["task"], "Send the revised budget");
        assert_eq!(v["at"]["secs"], 90);
        // Notes without tasks aren't listed; newest first.
        let mut older = n.clone();
        older.id = 4;
        older.created_ts = 10;
        let mut plain = n.clone();
        plain.id = 5;
        plain.enhanced_content = "Just a summary.".into();
        let ids: Vec<u64> = list(&[older, plain, n]).iter().map(|x| x.note_id).collect();
        assert_eq!(ids, [3, 4]);
    }
}

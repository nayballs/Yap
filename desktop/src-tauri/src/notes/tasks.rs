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
//! - **Tasks the person adds** ([`OwnTask`], 2026-10-25, Wispr Flow's "add a
//!   task"): in a meeting's **Tasks** tab (`note_id`: listed with that
//!   meeting's) or in the Tasks view with no meeting. They live in
//!   tasks.json (`task_store.rs`: a file an older Yap sharing the data
//!   folder never opens, where new fields of notes.json would be dropped by
//!   its next save), never in the summary — so adding one never makes the
//!   summary "edited", and Yap writing the summary again never loses one.
//!   Read like a summary's task ("Priya: book the room (due: Friday)";
//!   nobody named = You). Deleted with their note.
//! - **Complete all** and **Undo** ([`tasks_set`]): many ticks in one step,
//!   each where it lives (the summary's "- [x]", an added task's `done`).
//! - **Archive** ([`tasks_archive`]): done tasks out of the Tasks view's
//!   list (with Undo), still ticked in their meeting: a summary's by its
//!   note and words, an added one's `archived` — both in tasks.json.
//!
//! Parsing is line by line and pure (unit-tested); fenced code is skipped.
//! Logs carry ids and counts, never a task's words.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use tauri::Emitter;

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
    /// A task the person added ([`OwnTask::id`]); none: one of the summary's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub own: Option<u64>,
    /// Archived from the Tasks view.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub archived: bool,
}

/// A task the person added (see the module notes).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnTask {
    pub id: u64,
    /// As typed, on one line: "Book the room", "Priya: send the budget (due: Friday)".
    pub text: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub done: bool,
    /// The meeting note it was added in; none: the Tasks view's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_id: Option<u64>,
    /// When it was added (unix ms).
    #[serde(default)]
    pub created_ms: u64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub archived: bool,
}

impl OwnTask {
    /// As the summary's tasks read: who (You unless it names someone), what,
    /// and the deadline.
    pub fn as_task(&self) -> Task {
        let parsed = parse(&format!("- [ ] {}", self.text)).into_iter().next();
        let (task, owner, due) = match parsed {
            Some(t) => (t.task, if t.owner.is_empty() { "You".to_string() } else { t.owner }, t.due),
            None => (self.text.clone(), "You".to_string(), String::new()),
        };
        Task { line: 0, text: self.text.clone(), task, owner, due, done: self.done }
    }

    fn listed(&self) -> ListedTask {
        ListedTask { task: self.as_task(), at: None, own: Some(self.id), archived: self.archived }
    }
}

/// A task as a window names it: an added one by its id (`own`), a summary's
/// by its note, line and words.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRef {
    #[serde(default)]
    pub note_id: Option<u64>,
    #[serde(default)]
    pub own: Option<u64>,
    #[serde(default)]
    pub line: Option<usize>,
    #[serde(default)]
    pub text: String,
}

/// A task the person types is one line, of a sensible length.
const MAX_TASK_CHARS: usize = 500;

/// `text` as a task's words (pure): one line, trimmed; refused when empty or
/// too long, or when it's a checkbox line itself ("- [ ] …": its words).
pub fn task_words_typed(text: &str) -> Result<String, String> {
    let one = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let one = match checkbox(&one) {
        Some((_, _, words)) => words.to_string(),
        None => one,
    };
    if one.is_empty() {
        return Err("A task needs some words".to_string());
    }
    if one.chars().count() > MAX_TASK_CHARS {
        return Err("That's too long for one task".to_string());
    }
    Ok(one)
}

/// Tick (`done`) or untick the added tasks among `refs` (pure). Returns how
/// many changed.
pub fn mark_own(own: &mut [OwnTask], refs: &[TaskRef], done: bool) -> usize {
    let mut n = 0;
    for id in refs.iter().filter_map(|r| r.own) {
        if let Some(t) = own.iter_mut().find(|t| t.id == id && t.done != done) {
            t.done = done;
            n += 1;
        }
    }
    n
}

/// Tick (`done`) or untick note `note`'s summary tasks among `refs` (pure):
/// in its markdown, by line and words, as [`set_done`]; a task that's gone
/// is skipped. Returns how many changed.
pub fn mark_summary(note: &mut Note, refs: &[TaskRef], done: bool) -> usize {
    let mut n = 0;
    for r in refs.iter().filter(|r| r.own.is_none() && r.note_id == Some(note.id)) {
        let Ok(md) = set_done(&note.enhanced_content, r.line.unwrap_or(usize::MAX), &r.text, done) else { continue };
        if md != note.enhanced_content {
            note.enhanced_content = md;
            n += 1;
        }
    }
    n
}

/// Archive (`archived`) or bring back each of `refs` (pure, on tasks.json's
/// store): an added task's own mark, a summary task by its note and words.
/// Returns how many changed.
pub fn archive(store: &mut super::task_store::TaskStore, refs: &[TaskRef], archived: bool) -> usize {
    let mut n = 0;
    for r in refs {
        if let Some(id) = r.own {
            if let Some(t) = store.tasks.iter_mut().find(|t| t.id == id && t.archived != archived) {
                t.archived = archived;
                n += 1;
            }
            continue;
        }
        let (Some(note_id), words) = (r.note_id, r.text.trim().to_string()) else { continue };
        let has = store.is_archived(note_id, &words);
        if archived && !has && !words.is_empty() {
            store.archived.push(super::task_store::Archived { note_id, text: words });
            n += 1;
        } else if !archived && has {
            store.archived.retain(|a| !(a.note_id == note_id && a.text == words));
            n += 1;
        }
    }
    n
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

/// Note `n`'s summary tasks, each with the moment it was said and whether
/// it's archived, then the tasks added in it (in the order added).
pub fn note_tasks(n: &Note, store: &super::task_store::TaskStore) -> NoteTasks {
    let t0 = n.transcript.iter().find(|s| !s.dictated).map(|s| s.ts).unwrap_or(0);
    let mut tasks: Vec<ListedTask> = parse(&n.enhanced_content)
        .into_iter()
        .map(|task| {
            let at = moment(n, &task.task).map(|i| Moment {
                index: i,
                ts: n.transcript[i].ts,
                secs: n.transcript[i].ts.saturating_sub(t0),
            });
            let archived = store.is_archived(n.id, &task.text);
            ListedTask { task, at, own: None, archived }
        })
        .collect();
    tasks.extend(store.tasks.iter().filter(|t| t.note_id == Some(n.id)).map(OwnTask::listed));
    let title = if n.title.trim().is_empty() { "Untitled note".to_string() } else { n.title.trim().to_string() };
    NoteTasks { note_id: n.id, title, created_ts: n.created_ts, note_type: n.note_type.clone(), tasks }
}

/// Every note with tasks in its summary, newest first.
#[cfg(test)]
pub fn list(notes: &[Note]) -> Vec<NoteTasks> {
    list_all(notes, &Default::default())
}

/// Every note with tasks (in its summary, or added in it), newest first;
/// first of all, the tasks added with no meeting (note id 0, "Your tasks").
pub fn list_all(notes: &[Note], store: &super::task_store::TaskStore) -> Vec<NoteTasks> {
    let mut out: Vec<NoteTasks> = notes
        .iter()
        .map(|n| note_tasks(n, store))
        .filter(|n| !n.tasks.is_empty())
        .collect();
    out.sort_by_key(|n| std::cmp::Reverse(n.created_ts));
    let own = &store.tasks;
    let loose: Vec<ListedTask> = own
        .iter()
        .filter(|t| t.note_id.is_none_or(|id| !notes.iter().any(|n| n.id == id)))
        .map(OwnTask::listed)
        .collect();
    if !loose.is_empty() {
        let newest = own.iter().filter(|t| t.note_id.is_none()).map(|t| t.created_ms / 1000).max().unwrap_or(0);
        out.insert(0, NoteTasks { note_id: 0, title: "Your tasks".to_string(), created_ts: newest, note_type: String::new(), tasks: loose });
    }
    out
}

/// Every note's tasks, and the ones added with no meeting (the Tasks view).
#[tauri::command]
pub fn tasks_list() -> Vec<NoteTasks> {
    list_all(&crate::notes::all(), &super::task_store::snapshot())
}

/// One note's tasks (its Tasks tab): the summary's, then those added in it.
#[tauri::command]
pub fn note_tasks_get(note_id: u64) -> Result<NoteTasks, String> {
    let n = crate::notes::get(note_id).ok_or("Note not found")?;
    Ok(note_tasks(&n, &super::task_store::snapshot()))
}

/// Every window: tasks changed (`notes`: the notes whose tasks did; 0 = the
/// ones with no meeting). The Tasks view and a meeting's Tasks tab follow it.
pub const EVENT_TASKS: &str = "yap-tasks-changed";

fn tasks_changed(app: &tauri::AppHandle, notes: &[u64]) {
    let _ = app.emit(EVENT_TASKS, serde_json::json!({ "notes": notes }));
}

/// The notes `items` name (0: the tasks with no meeting), once each.
fn touched(items: &[TaskRef]) -> Vec<u64> {
    let mut out: Vec<u64> = items.iter().map(|r| r.note_id.unwrap_or(0)).collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Add a task: in meeting note `note_id`'s Tasks tab, or (none) in the
/// Tasks view with no meeting. Returns it.
#[tauri::command]
pub fn task_add(app: tauri::AppHandle, text: String, note_id: Option<u64>) -> Result<OwnTask, String> {
    let text = task_words_typed(&text)?;
    if let Some(id) = note_id {
        crate::notes::get(id).ok_or("Note not found")?;
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let task = super::task_store::modify(|s| {
        let id = super::task_store::SAVER.next_id(s.tasks.iter().map(|t| t.id).max().unwrap_or(0));
        let task = OwnTask { id, text, done: false, note_id, created_ms: now, archived: false };
        s.tasks.push(task.clone());
        Ok((task, true))
    })?;
    tracing::info!(task = task.id, in_note = note_id.is_some(), "Task added");
    tasks_changed(&app, &[note_id.unwrap_or(0)]);
    super::task_store::saved()?;
    Ok(task)
}

/// Delete a task the person added (a summary's goes with the summary).
#[tauri::command]
pub fn task_remove(app: tauri::AppHandle, id: u64) -> Result<(), String> {
    let note = super::task_store::modify(|s| {
        let at = s.tasks.iter().position(|t| t.id == id).ok_or("That task isn't there any more")?;
        Ok((s.tasks.remove(at).note_id, true))
    })?;
    tracing::info!(task = id, "Task removed");
    tasks_changed(&app, &[note.unwrap_or(0)]);
    Ok(super::task_store::saved()?)
}

/// Tick (`done`) or untick many tasks in one step (Complete all, its Undo):
/// each where it lives — an added one in tasks.json, a summary's in its
/// note. Returns how many changed.
#[tauri::command]
pub fn tasks_set(app: tauri::AppHandle, items: Vec<TaskRef>, done: bool, origin: Option<String>) -> Result<usize, String> {
    let mut n = super::task_store::modify(|s| {
        let n = mark_own(&mut s.tasks, &items, done);
        Ok((n, n > 0))
    })?;
    let mut notes: Vec<u64> = items.iter().filter(|r| r.own.is_none()).filter_map(|r| r.note_id).collect();
    notes.sort_unstable();
    notes.dedup();
    let mut changed = Vec::new();
    for id in notes {
        let k = crate::notes::modify(id, |note| {
            let k = mark_summary(note, &items, done);
            Ok((k, k > 0))
        })
        .unwrap_or(0);
        if k > 0 {
            n += k;
            changed.push(id);
        }
    }
    tracing::info!(tasks = n, notes = changed.len(), done, "Tasks ticked");
    for id in &changed {
        crate::notes::note_edits::summary_changed(&app, *id, origin.as_deref().unwrap_or(""));
    }
    tasks_changed(&app, &touched(&items));
    super::task_store::saved()?;
    crate::notes::saved()?;
    Ok(n)
}

/// Archive (`archived`) or bring back tasks (the Tasks view's Archive done,
/// its Undo). Returns how many changed.
#[tauri::command]
pub fn tasks_archive(app: tauri::AppHandle, items: Vec<TaskRef>, archived: bool) -> Result<usize, String> {
    let n = super::task_store::modify(|s| {
        let n = archive(s, &items, archived);
        Ok((n, n > 0))
    })?;
    tracing::info!(tasks = n, archived, "Tasks archived");
    tasks_changed(&app, &touched(&items));
    super::task_store::saved()?;
    Ok(n)
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
    crate::notes::note_edits::summary_changed(&app, note_id, origin.as_deref().unwrap_or(""));
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

    fn own(id: u64, text: &str, note_id: Option<u64>) -> OwnTask {
        OwnTask { id, text: text.into(), done: false, note_id, created_ms: id * 1_000, archived: false }
    }

    #[test]
    fn a_task_the_person_adds_reads_like_the_summarys() {
        let t = own(1, "Priya: send the budget (due: Friday)", None).as_task();
        assert_eq!((t.owner.as_str(), t.task.as_str(), t.due.as_str()), ("Priya", "send the budget", "Friday"));
        // Nobody named: theirs.
        assert_eq!(own(2, "book the room", None).as_task().owner, "You");
        assert_eq!(task_words_typed("  - [ ] call the venue\n back "), Ok("call the venue back".to_string()));
        assert!(task_words_typed("  \n").is_err());
        assert!(task_words_typed(&"x".repeat(501)).is_err());
    }

    fn store_of(tasks: Vec<OwnTask>) -> crate::notes::task_store::TaskStore {
        crate::notes::task_store::TaskStore { tasks, archived: Vec::new() }
    }

    #[test]
    fn added_tasks_list_with_their_meeting_or_on_their_own() {
        let mut n: Note = serde_json::from_value(serde_json::json!({
            "id": 3, "title": "Offsite", "noteType": "meeting", "createdTs": 50, "updatedTs": 50
        }))
        .unwrap();
        n.enhanced_content = PLAN.to_string();
        let mut bare: Note = serde_json::from_value(serde_json::json!({ "id": 9, "title": "Sync", "createdTs": 60, "updatedTs": 60 })).unwrap();
        bare.enhanced_content = "Just talk.".into();
        let mine = store_of(vec![own(1, "Book flights", Some(3)), own(2, "Renew the passport", None), own(3, "Order badges", Some(9))]);
        let listed = list_all(&[n.clone(), bare.clone()], &mine);
        // "Your tasks" first, then meetings newest first; a note with only
        // added tasks is listed too.
        assert_eq!(listed.iter().map(|g| g.note_id).collect::<Vec<_>>(), [0, 9, 3]);
        assert_eq!(listed[0].tasks.iter().map(|t| t.task.task.as_str()).collect::<Vec<_>>(), ["Renew the passport"]);
        let offsite = &listed[2];
        assert_eq!(offsite.tasks.len(), 5, "the summary's 4, then the added one");
        assert_eq!(offsite.tasks[4].own, Some(1));
        assert_eq!(offsite.tasks[4].task.owner, "You");
        // On the wire: `own` only on added ones, `archived` only when so.
        let v = serde_json::to_value(&offsite.tasks[0]).unwrap();
        assert!(v.get("own").is_none() && v.get("archived").is_none());
        assert_eq!(note_tasks(&n, &mine).tasks.len(), 5);
    }

    #[test]
    fn complete_all_ticks_each_where_it_lives_and_undo_unticks() {
        let mut note: Note = serde_json::from_value(serde_json::json!({
            "id": 3, "title": "Offsite", "createdTs": 50, "updatedTs": 50, "enhancedContent": PLAN
        }))
        .unwrap();
        let mut mine = store_of(vec![own(1, "Book flights", Some(3))]);
        let open: Vec<TaskRef> = note_tasks(&note, &mine)
            .tasks
            .iter()
            .filter(|t| !t.task.done)
            .map(|t| TaskRef { note_id: Some(3), own: t.own, line: Some(t.task.line), text: t.task.text.clone() })
            .collect();
        assert_eq!(open.len(), 4);
        assert_eq!((mark_own(&mut mine.tasks, &open, true), mark_summary(&mut note, &open, true)), (1, 3));
        assert!(mine.tasks[0].done);
        assert_eq!(parse(&note.enhanced_content).iter().filter(|t| t.done).count(), 4);
        assert!(same_but_ticks(PLAN, &note.enhanced_content), "ticks, not an edit");
        // Undo: the same ones unticked; the one ticked before stays ticked.
        assert_eq!(mark_own(&mut mine.tasks, &open, false) + mark_summary(&mut note, &open, false), 4);
        assert_eq!(note.enhanced_content, PLAN);
        assert!(!mine.tasks[0].done);
        // A task that's gone, or another note's, is skipped.
        let gone = [
            TaskRef { note_id: Some(3), own: None, line: Some(5), text: "Not a task".into() },
            TaskRef { note_id: Some(4), own: None, line: Some(5), text: "Draft the agenda (due: tomorrow)".into() },
        ];
        assert_eq!(mark_summary(&mut note, &gone, true), 0);
    }

    #[test]
    fn archived_tasks_leave_the_list_and_come_back() {
        let notes: Vec<Note> = vec![serde_json::from_value(serde_json::json!({
            "id": 3, "title": "Offsite", "createdTs": 50, "updatedTs": 50, "enhancedContent": PLAN
        }))
        .unwrap()];
        let mut store = store_of(vec![OwnTask { done: true, ..own(1, "Book flights", None) }]);
        let refs = [
            TaskRef { note_id: Some(3), own: None, line: Some(6), text: "Share the slides".into() },
            TaskRef { note_id: None, own: Some(1), line: None, text: String::new() },
        ];
        assert_eq!(archive(&mut store, &refs, true), 2);
        assert_eq!(archive(&mut store, &refs, true), 0, "once");
        let listed = list_all(&notes, &store);
        assert!(listed.iter().flat_map(|g| &g.tasks).filter(|t| t.archived).count() == 2);
        // Still ticked in its summary, which archiving never touches.
        assert_eq!(notes[0].enhanced_content, PLAN);
        assert_eq!(archive(&mut store, &refs, false), 2);
        assert!(store.archived.is_empty() && !store.tasks[0].archived);
    }

    /// The installed Yap and a newer dev build share one data folder: an
    /// older Yap reads notes.json into the shape it knows and saves that back,
    /// dropping every field it doesn't know. The person's added and archived
    /// tasks live in tasks.json, which it never opens, and a split line is
    /// two ordinary lines: all of it survives an old save. (What an older
    /// Yap would drop: `edited`/`original` on lines, if it's from before
    /// 2026-10-24 — so a split part's "Revert" — and `transcriptDeletedTs`.)
    #[test]
    fn an_older_yap_saving_notes_json_keeps_added_tasks_and_split_lines() {
        // notes.json as an older Yap (before 2026-10-24) knows it.
        #[derive(Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct OldLine {
            source: String,
            text: String,
            ts: u64,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            start_ms: Option<u64>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            end_ms: Option<u64>,
            #[serde(default, skip_serializing_if = "Vec::is_empty")]
            words: Vec<[u32; 2]>,
        }
        #[derive(Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct OldNote {
            id: u64,
            title: String,
            content: String,
            enhanced_content: String,
            transcript: Vec<OldLine>,
            created_ts: u64,
            updated_ts: u64,
        }
        #[derive(Serialize, Deserialize)]
        struct OldStore {
            notes: Vec<OldNote>,
        }

        // This build: a summary task archived, tasks added, a line split.
        let id = crate::notes::create("Offsite", "my notes", "meeting", "Meetings").id;
        crate::notes::modify(id, |n| {
            n.enhanced_content = PLAN.to_string();
            n.transcript = vec![crate::notes::TranscriptSegment {
                source: "them".into(),
                text: "budget is due Friday okay thanks".into(),
                ts: 100,
                start_ms: Some(10_000),
                end_ms: Some(16_000),
                words: vec![[0, 400], [500, 800], [900, 1_200], [1_300, 2_000], [3_000, 3_600], [3_700, 4_500]],
                ..Default::default()
            }];
            let parts = crate::notes::transcript_edits::split_line(&n.transcript[0], 4)?;
            crate::notes::replace_line(n, 0, parts.to_vec());
            Ok(((), true))
        })
        .unwrap();
        crate::notes::task_store::modify(|s| {
            s.tasks = vec![own(1, "Book flights", Some(id)), own(2, "Renew the passport", None)];
            archive(s, &[TaskRef { note_id: Some(id), own: None, line: None, text: "Share the slides".into() }], true);
            Ok(((), true))
        })
        .unwrap();
        let tasks_before = crate::notes::task_store::snapshot();

        // An older Yap loads notes.json and saves it back in its own shape.
        let on_disk = crate::notes::store_json();
        let old: OldStore = serde_json::from_str(&on_disk).unwrap();
        let resaved = serde_json::to_string(&old).unwrap();
        // This build reads it again.
        crate::notes::load_json_for_test(&resaved);

        let n = crate::notes::get(id).unwrap();
        let texts: Vec<(&str, Option<u64>, Option<u64>)> = n.transcript.iter().map(|s| (s.text.as_str(), s.start_ms, s.end_ms)).collect();
        assert_eq!(texts, [("budget is due Friday", Some(10_000), Some(12_000)), ("okay thanks", Some(13_000), Some(16_000))]);
        assert_eq!(n.transcript[1].words, vec![[0, 600], [700, 1_500]], "each part keeps its words' times");
        // The added and archived tasks were never in notes.json: untouched.
        assert_eq!(crate::notes::task_store::snapshot(), tasks_before);
        let listed = list_all(&[n], &tasks_before);
        assert!(listed.iter().any(|g| g.note_id == 0 && g.tasks[0].task.task == "Renew the passport"));
        let offsite = listed.iter().find(|g| g.note_id == id).unwrap();
        assert!(offsite.tasks.iter().any(|t| t.own == Some(1)));
        assert!(offsite.tasks.iter().any(|t| t.task.task == "Share the slides" && t.archived));
    }
}

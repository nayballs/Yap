//! **What an agent is doing, in plain words**: each of Claude Code's tool calls
//! as a line for the agent's tab ("Reading src/app.ts", "Searching the web ·
//! tide tables", "Listing the source folder"), and the file it changes or
//! the command it runs, for the agent's lists. Paths inside the agent's folder
//! are said relative to it, however they're written (`D:\…`, `D:/…`, Git
//! Bash's `/d/…`). A command's line is the description Claude Code gave it,
//! else the command tidied (its `cd` into the folder gone: [`command`]).
//! Claude Code's to-do list (`TodoWrite`) as [`Todo`]s. Pure.

use std::path::Path;

use serde::Serialize;
use serde_json::Value;

pub mod command;

/// How long a line's path, query or command runs before "…".
const WHAT_MAX: usize = 90;
/// How long a command is kept in the agent's list.
const COMMAND_MAX: usize = 1_000;
/// How long a command's description is kept.
const ABOUT_MAX: usize = 200;
/// How many to-dos, and how long each, the tab is told.
const TODOS_MAX: usize = 50;
const TODO_MAX: usize = 200;

/// One tool call, as the agent's tab shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seen {
    /// The line ("Editing notes.md").
    pub step: String,
    /// A file it changes (Edit, Write, MultiEdit, NotebookEdit), as said.
    pub file: Option<String>,
    /// A command it runs (Bash, PowerShell), whole (trimmed), its `cd` into
    /// the agent's folder gone.
    pub command: Option<String>,
    /// What Claude Code said the command does (Bash's `description`).
    pub about: Option<String>,
    /// Claude Code's to-do list, as a `TodoWrite` leaves it.
    pub todos: Option<Vec<Todo>>,
}

/// **One of Claude Code's to-dos** (`TodoWrite`'s `todos[]`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Todo {
    pub content: String,
    /// "pending", "in_progress" or "completed" (Claude Code's own words).
    pub status: String,
    /// The line while it's in progress ("Running the tests").
    #[serde(skip_serializing_if = "String::is_empty")]
    pub active_form: String,
}

/// A `TodoWrite`'s list from its `input` (`None` when it has none): an
/// unknown status is "pending".
pub fn todos(input: &Value) -> Option<Vec<Todo>> {
    let list = input.get("todos")?.as_array()?;
    let text = |v: &Value, k: &str| one_line(v.get(k).and_then(Value::as_str).unwrap_or(""), TODO_MAX);
    Some(
        list.iter()
            .filter(|t| t.is_object())
            .map(|t| {
                let status = match t.get("status").and_then(Value::as_str).unwrap_or("") {
                    s @ ("in_progress" | "completed") => s.to_string(),
                    _ => "pending".to_string(),
                };
                Todo { content: text(t, "content"), status, active_form: text(t, "activeForm") }
            })
            .filter(|t| !t.content.is_empty())
            .take(TODOS_MAX)
            .collect(),
    )
}

/// The line while the list is being worked through: the to-do in
/// progress (its `activeForm`, else its words).
pub fn doing_now(todos: &[Todo]) -> Option<String> {
    let t = todos.iter().find(|t| t.status == "in_progress")?;
    Some(if t.active_form.is_empty() { t.content.clone() } else { t.active_form.clone() })
}

/// `name`'s call with `input`, in the agent working in `folder`.
pub fn of_tool(name: &str, input: &Value, folder: &Path) -> Seen {
    let text = |keys: &[&str]| keys.iter().find_map(|k| input[*k].as_str()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let path = |keys: &[&str]| text(keys).map(|p| shown_path(&p, folder));
    let dotted = |doing: &str, what: Option<String>| what.map_or_else(|| doing.to_string(), |w| format!("{doing} · {}", one_line(&w, WHAT_MAX)));
    let with = |doing: &str, what: Option<String>, none: &str| what.map_or_else(|| none.to_string(), |w| format!("{doing} {}", one_line(&w, WHAT_MAX)));
    // Where a search looks: said only when it isn't the agent's folder itself.
    let place = || text(&["path"]).map(|p| shown_path(&p, folder)).filter(|p| !p.is_empty() && p != ".");
    match name {
        "Read" => Seen { step: with("Reading", path(&["file_path", "path"]), "Reading a file"), ..Default::default() },
        "Edit" | "MultiEdit" => {
            let file = path(&["file_path", "path"]);
            Seen { step: with("Editing", file.clone(), "Editing a file"), file, ..Default::default() }
        }
        "Write" => {
            let file = path(&["file_path", "path"]);
            Seen { step: with("Writing", file.clone(), "Writing a file"), file, ..Default::default() }
        }
        "NotebookEdit" => {
            let file = path(&["notebook_path", "file_path"]);
            Seen { step: with("Editing", file.clone(), "Editing a notebook"), file, ..Default::default() }
        }
        "Bash" | "PowerShell" => {
            let raw = text(&["command"]).unwrap_or_default();
            let about = text(&["description"]).map(|d| one_line(&d, ABOUT_MAX));
            let step = command::line(&raw, about.as_deref(), folder, WHAT_MAX + 8);
            let command = (!raw.is_empty()).then(|| clip(&command::strip_cd(&raw, folder).0, COMMAND_MAX));
            Seen { step, command, about, ..Default::default() }
        }
        "Glob" => {
            // A pattern written from the folder down said from it.
            let pattern = text(&["pattern"]).map(|p| command::relative(&p, folder));
            let doing = place().map_or_else(|| "Looking for files".to_string(), |p| format!("Looking for files in {p}"));
            Seen { step: dotted(&doing, pattern), ..Default::default() }
        }
        "Grep" => {
            let doing = place().map_or_else(|| "Searching the files".to_string(), |p| format!("Searching {p}"));
            Seen { step: dotted(&doing, text(&["pattern"])), ..Default::default() }
        }
        "LS" => Seen { step: with("Looking in", path(&["path"]), "Looking at the folder"), ..Default::default() },
        "WebSearch" => Seen { step: dotted("Searching the web", text(&["query"])), ..Default::default() },
        "WebFetch" => Seen { step: with("Reading", text(&["url"]).map(|u| crate::web::short_url(&u)), "Reading a page"), ..Default::default() },
        "Task" | "Agent" => Seen { step: dotted("Handing part of it to a helper", text(&["description"])), ..Default::default() },
        "TodoWrite" => Seen { step: "Planning the steps".into(), todos: todos(input), ..Default::default() },
        "Skill" => Seen { step: dotted("Using a skill", text(&["skill", "name", "command"])), ..Default::default() },
        "ExitPlanMode" => Seen { step: "Writing the plan".into(), ..Default::default() },
        "BashOutput" | "KillShell" | "KillBash" => Seen { step: "Checking a command".into(), ..Default::default() },
        other => match other.strip_prefix("mcp__").and_then(|r| r.split_once("__")) {
            Some((server, tool)) => Seen { step: format!("Using {server} · {}", tool.replace('_', " ")), ..Default::default() },
            None => Seen { step: format!("Using {}", one_line(other, 40)), ..Default::default() },
        },
    }
}

/// A path as the agent's tab says it: relative to the agent's `folder` when
/// it's inside it (with `\`; written `D:\…`, `D:/…` or Git Bash's `/d/…`),
/// else as given.
pub fn shown_path(path: &str, folder: &Path) -> String {
    let p = path.trim();
    match command::inside(p, folder) {
        Some(rel) if rel.is_empty() => ".".to_string(),
        Some(rel) => rel.replace('/', "\\"),
        None => p.replace('/', "\\").trim_start_matches(".\\").to_string(),
    }
}

/// A permission denial's line: what it wanted ("Run a command: rm -rf
/// build"), from the tool and its input.
pub fn denied(tool: &str, input: &Value, folder: &Path) -> String {
    let seen = of_tool(tool, input, folder);
    match (&seen.command, &seen.file) {
        (Some(c), _) => format!("Run a command: {}", one_line(c, WHAT_MAX)),
        (None, Some(f)) => format!("Change {f}"),
        _ => seen.step,
    }
}

/// `s` on one line, at most `max` characters ("…" when cut).
pub fn one_line(s: &str, max: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    clip(&flat, max)
}

/// `s`, at most `max` characters ("…" when cut).
pub fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>().trim_end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn folder() -> std::path::PathBuf {
        std::path::PathBuf::from(r"D:\Projects\garden-planner")
    }

    #[test]
    fn each_tool_call_reads_as_a_plain_line() {
        let f = folder();
        let s = of_tool("Read", &json!({ "file_path": r"D:\Projects\garden-planner\src\app.ts" }), &f);
        assert_eq!(s.step, r"Reading src\app.ts");
        assert_eq!((s.file, s.command), (None, None));
        let s = of_tool("Edit", &json!({ "file_path": "D:/Projects/garden-planner/notes.md", "old_string": "a", "new_string": "b" }), &f);
        assert_eq!(s.step, "Editing notes.md");
        assert_eq!(s.file.as_deref(), Some("notes.md"));
        let s = of_tool("Write", &json!({ "file_path": r"C:\Elsewhere\todo.txt" }), &f);
        assert_eq!((s.step.as_str(), s.file.as_deref()), (r"Writing C:\Elsewhere\todo.txt", Some(r"C:\Elsewhere\todo.txt")));
        // A command: what Claude Code said it does, as a doing word; the command whole.
        let s = of_tool("Bash", &json!({ "command": "npm test\n  --watch=false", "description": "Run the tests" }), &f);
        assert_eq!(s.step, "Running the tests");
        assert_eq!(s.command.as_deref(), Some("npm test\n  --watch=false"));
        assert_eq!(s.about.as_deref(), Some("Run the tests"));
        // No description: the command, its cd into the folder gone, its paths relative.
        let s = of_tool("Bash", &json!({ "command": "cd \"/d/Projects/garden-planner\" && ls \"/d/Projects/garden-planner/beds\" | grep -i tomato" }), &f);
        assert_eq!(s.step, "Running ls beds | grep -i tomato");
        assert_eq!(s.command.as_deref(), Some("ls \"/d/Projects/garden-planner/beds\" | grep -i tomato"), "the list keeps the command, only the cd gone");
        assert_eq!(s.about, None);
        let s = of_tool("PowerShell", &json!({ "command": "Set-Location 'D:\\Projects\\garden-planner'; Get-ChildItem src" }), &f);
        assert_eq!((s.step.as_str(), s.command.as_deref()), ("Running Get-ChildItem src", Some("Get-ChildItem src")));
        assert_eq!(of_tool("WebSearch", &json!({ "query": "tide tables" }), &f).step, "Searching the web · tide tables");
        assert_eq!(of_tool("WebFetch", &json!({ "url": "https://docs.example.com/guide/start" }), &f).step, "Reading docs.example.com/guide/start");
        assert_eq!(of_tool("Grep", &json!({ "pattern": "TODO" }), &f).step, "Searching the files · TODO");
        // Searches say where, relative; not the folder itself.
        assert_eq!(of_tool("Grep", &json!({ "pattern": "water", "path": "D:/Projects/garden-planner/src" }), &f).step, r"Searching src · water");
        assert_eq!(of_tool("Grep", &json!({ "pattern": "water", "path": "/d/Projects/garden-planner" }), &f).step, "Searching the files · water");
        assert_eq!(of_tool("Glob", &json!({ "pattern": "**/*.md" }), &f).step, "Looking for files · **/*.md");
        assert_eq!(of_tool("Glob", &json!({ "pattern": "*.json", "path": r"D:\Projects\garden-planner\beds" }), &f).step, r"Looking for files in beds · *.json");
        assert_eq!(of_tool("Glob", &json!({ "pattern": "D:/Projects/garden-planner/src/**/*.js" }), &f).step, "Looking for files · src/**/*.js");
        assert_eq!(of_tool("Read", &json!({ "file_path": "/d/Projects/garden-planner/src/app.ts" }), &f).step, r"Reading src\app.ts");
        assert_eq!(of_tool("Task", &json!({ "description": "Check the docs" }), &f).step, "Handing part of it to a helper · Check the docs");
        assert_eq!(of_tool("TodoWrite", &json!({}), &f).step, "Planning the steps");
        assert_eq!(of_tool("mcp__github__create_issue", &json!({}), &f).step, "Using github · create issue");
        assert_eq!(of_tool("Frobnicate", &json!({}), &f).step, "Using Frobnicate");
        // No arguments yet: still a line.
        assert_eq!(of_tool("Edit", &Value::Null, &f).step, "Editing a file");
        assert_eq!(of_tool("Bash", &Value::Null, &f).step, "Running a command");
    }

    #[test]
    fn paths_inside_the_folder_are_relative_and_denials_say_what_was_wanted() {
        let f = folder();
        assert_eq!(shown_path(r"d:\projects\GARDEN-planner\a\b.rs", &f), r"a\b.rs", "case doesn't matter");
        assert_eq!(shown_path(r"D:\Projects\garden-planner-old\x.rs", &f), r"D:\Projects\garden-planner-old\x.rs", "a sibling isn't inside");
        assert_eq!(shown_path("notes.md", &f), "notes.md");
        assert_eq!(denied("Bash", &json!({ "command": "rm -rf build" }), &f), "Run a command: rm -rf build");
        assert_eq!(denied("Write", &json!({ "file_path": r"D:\Projects\garden-planner\.env" }), &f), "Change .env");
        assert_eq!(denied("WebFetch", &json!({ "url": "https://example.com/x" }), &f), "Reading example.com/x");
        assert_eq!(denied("Bash", &json!({ "command": "cd /d/Projects/garden-planner && rm -rf build" }), &f), "Run a command: rm -rf build");
        assert_eq!(shown_path("/d/Projects/garden-planner/beds/a.json", &f), r"beds\a.json");
        assert_eq!(shown_path(r"D:\Projects\garden-planner", &f), ".");
        assert_eq!(shown_path("./notes.md", &f), "notes.md");
        assert_eq!(clip("abcdef", 4), "abc…");
    }

    #[test]
    fn a_todowrite_is_claude_codes_list() {
        let input = json!({ "todos": [
            { "content": "Look at how beds are stored", "status": "completed", "activeForm": "Looking at how beds are stored" },
            { "content": "Add a watering schedule", "status": "in_progress", "activeForm": "Adding a watering schedule" },
            { "content": "Write the planting guide", "status": "pending", "activeForm": "Writing the planting guide" },
            { "content": "  ", "status": "pending" },
            { "content": "Tidy up", "status": "someday" },
            "not a to-do"
        ] });
        let list = todos(&input).unwrap();
        assert_eq!(list.iter().map(|t| (t.content.as_str(), t.status.as_str())).collect::<Vec<_>>(), [
            ("Look at how beds are stored", "completed"),
            ("Add a watering schedule", "in_progress"),
            ("Write the planting guide", "pending"),
            ("Tidy up", "pending"),
        ]);
        assert_eq!(doing_now(&list).as_deref(), Some("Adding a watering schedule"));
        // No activeForm: its words; none in progress: no line.
        assert_eq!(doing_now(&[Todo { content: "Sort the seeds".into(), status: "in_progress".into(), active_form: String::new() }]).as_deref(), Some("Sort the seeds"));
        assert_eq!(doing_now(&list[2..]), None);
        assert_eq!(todos(&json!({})), None);
        assert_eq!(of_tool("TodoWrite", &input, &folder()).todos.map(|t| t.len()), Some(4));
        let v = serde_json::to_value(&list[3]).unwrap();
        assert_eq!(v, json!({ "content": "Tidy up", "status": "pending" }));
        assert_eq!(serde_json::to_value(&list[1]).unwrap()["activeForm"], "Adding a watering schedule");
    }
}

//! **What a job is doing, in plain words**: each of Claude Code's tool calls
//! as a line for the job's tab ("Reading src/app.ts", "Searching the web ·
//! tide tables", "Running npm test"), and the file it changes or the command
//! it runs, for the job's lists. Paths inside the job's folder are said
//! relative to it. Pure.

use std::path::Path;

use serde_json::Value;

/// How long a line's path, query or command runs before "…".
const WHAT_MAX: usize = 90;
/// How long a command is kept in the job's list.
const COMMAND_MAX: usize = 300;

/// One tool call, as the job's tab shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seen {
    /// The line ("Editing notes.md").
    pub step: String,
    /// A file it changes (Edit, Write, MultiEdit, NotebookEdit), as said.
    pub file: Option<String>,
    /// A command it runs (Bash, PowerShell), whole (trimmed).
    pub command: Option<String>,
}

/// `name`'s call with `input`, in the job working in `folder`.
pub fn of_tool(name: &str, input: &Value, folder: &Path) -> Seen {
    let text = |keys: &[&str]| keys.iter().find_map(|k| input[*k].as_str()).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let path = |keys: &[&str]| text(keys).map(|p| shown_path(&p, folder));
    let dotted = |doing: &str, what: Option<String>| what.map_or_else(|| doing.to_string(), |w| format!("{doing} · {}", one_line(&w, WHAT_MAX)));
    let with = |doing: &str, what: Option<String>, none: &str| what.map_or_else(|| none.to_string(), |w| format!("{doing} {}", one_line(&w, WHAT_MAX)));
    match name {
        "Read" => Seen { step: with("Reading", path(&["file_path", "path"]), "Reading a file"), ..Default::default() },
        "Edit" | "MultiEdit" => {
            let file = path(&["file_path", "path"]);
            Seen { step: with("Editing", file.clone(), "Editing a file"), file, command: None }
        }
        "Write" => {
            let file = path(&["file_path", "path"]);
            Seen { step: with("Writing", file.clone(), "Writing a file"), file, command: None }
        }
        "NotebookEdit" => {
            let file = path(&["notebook_path", "file_path"]);
            Seen { step: with("Editing", file.clone(), "Editing a notebook"), file, command: None }
        }
        "Bash" | "PowerShell" => {
            let command = text(&["command"]).map(|c| clip(c.trim(), COMMAND_MAX));
            let first = command.as_deref().map(|c| c.lines().next().unwrap_or("").to_string());
            Seen { step: with("Running", first, "Running a command"), file: None, command }
        }
        "Glob" => Seen { step: dotted("Looking for files", text(&["pattern"])), ..Default::default() },
        "Grep" => Seen { step: dotted("Searching the files", text(&["pattern"])), ..Default::default() },
        "LS" => Seen { step: with("Looking in", path(&["path"]), "Looking at the folder"), ..Default::default() },
        "WebSearch" => Seen { step: dotted("Searching the web", text(&["query"])), ..Default::default() },
        "WebFetch" => Seen { step: with("Reading", text(&["url"]).map(|u| crate::web::short_url(&u)), "Reading a page"), ..Default::default() },
        "Task" | "Agent" => Seen { step: dotted("Handing part of it to a helper", text(&["description"])), ..Default::default() },
        "TodoWrite" => Seen { step: "Planning the steps".into(), ..Default::default() },
        "Skill" => Seen { step: dotted("Using a skill", text(&["skill", "name", "command"])), ..Default::default() },
        "ExitPlanMode" => Seen { step: "Writing the plan".into(), ..Default::default() },
        "BashOutput" | "KillShell" | "KillBash" => Seen { step: "Checking a command".into(), ..Default::default() },
        other => match other.strip_prefix("mcp__").and_then(|r| r.split_once("__")) {
            Some((server, tool)) => Seen { step: format!("Using {server} · {}", tool.replace('_', " ")), ..Default::default() },
            None => Seen { step: format!("Using {}", one_line(other, 40)), ..Default::default() },
        },
    }
}

/// A path as the job's tab says it: relative to the job's `folder` when
/// it's inside it (with `\`), else as given.
pub fn shown_path(path: &str, folder: &Path) -> String {
    let p = path.trim();
    let norm = |s: &str| s.replace('/', "\\");
    let folder_s = norm(&folder.to_string_lossy()).trim_end_matches('\\').to_string();
    let full = norm(p);
    if !folder_s.is_empty() && full.len() > folder_s.len() && full.to_ascii_lowercase().starts_with(&folder_s.to_ascii_lowercase()) {
        let rest = &full[folder_s.len()..];
        if let Some(rel) = rest.strip_prefix('\\') {
            return rel.to_string();
        }
    }
    full.trim_start_matches(".\\").to_string()
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
        let s = of_tool("Bash", &json!({ "command": "npm test\n  --watch=false", "description": "Run the tests" }), &f);
        assert_eq!(s.step, "Running npm test");
        assert_eq!(s.command.as_deref(), Some("npm test\n  --watch=false"));
        assert_eq!(of_tool("WebSearch", &json!({ "query": "tide tables" }), &f).step, "Searching the web · tide tables");
        assert_eq!(of_tool("WebFetch", &json!({ "url": "https://docs.example.com/guide/start" }), &f).step, "Reading docs.example.com/guide/start");
        assert_eq!(of_tool("Grep", &json!({ "pattern": "TODO" }), &f).step, "Searching the files · TODO");
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
        assert_eq!(clip("abcdef", 4), "abc…");
    }
}

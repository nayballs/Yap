//! **What Chat is doing now** (2026-10-31): while a turn runs, the thread
//! shows one line — the step under way and the turn's time — as the Claude
//! Code app does: "Searching the web · tidecaller lone wolf build · 1m 59s",
//! "Reading emberfall.example/wiki/Talents · 6m 30s", "Writing ‘Character
//! creation’", "Thinking". Before, it mostly said "Thinking…" for minutes
//! while a strong model searched, read and wrote a document.
//!
//! Every step is one [`Activity`] and leaves through [`tell`] as one
//! `stream::Event::Step`; this file alone words them ([`of_call`] for every
//! tool, Yap's own and the provider's). Where they come from:
//! - **The stream** (`stream::Step`): a tool call beginning (its name only,
//!   [`of_call`] with no arguments: "Searching the web"), a document's
//!   section as its arguments come ("Writing ‘Gear’", `documents::live_section`),
//!   thinking ([`thinking`]: Anthropic's `thinking_delta`, a model's
//!   `reasoning_content`), the answer's first words shown ([`answering`]),
//!   and Claude's own searches and page reads on the Messages API
//!   (`llm/anthropic.rs`: a `server_tool_use` block, then its input whole —
//!   the query, the address — `llm::Delta::ServerInput`).
//! - **The tool loop** (`chat/tools/agent.rs`): each call as it runs, with
//!   its arguments as they ran (a search's query after Yap's fixes; a page's
//!   address; a document's title or section), and Yap's own searches for the
//!   model. A provider's native search (`web/native.rs`: OpenAI, Gemini,
//!   Groq, OpenRouter) and Exa / Tavily run inside the `web_search` call, so
//!   its line names the query Yap searches for.
//! - **A small model's web pipeline** and **Ask across meetings**
//!   (`chat/turn.rs`): the search Yap runs before the answer.
//!
//! The page keeps the latest ("Thinking" until one comes) with the turn's
//! time; once the answer is in, the steps done are its chips, folded into a
//! one-line summary under it (`views/chat/ChatSteps.svelte`). The Voice
//! Assistant's capsule on the Yap bar shows the same line in place of
//! "Thinking…" (`yap-assistant-stream`'s `step` events).
//!
//! **Privacy**: a line may carry a query or an address — the page shows
//! them, the log never does (nothing here logs; `privacy/log_privacy.rs`).

use serde::Serialize;
use serde_json::Value;

use crate::chat::stream::{Event, Sink};

/// The longest a line's query or title runs before "…" (the page cuts the
/// line to its width anyway).
const WHAT_MAX: usize = 90;

/// **One step of a turn**, as the page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Activity {
    /// What kind of step: "think", "answer", "search" (the web), "read" (a
    /// page), "picture" (a picture from the web looked at or shown), "page"
    /// (acting on the page beside the chat), "notes", "doc", "calendar",
    /// "memory", "apps", "chats", "tool".
    pub mark: &'static str,
    /// The line: "Searching the web · tidecaller build", "Reading
    /// emberfall.example/wiki/Talents", "Writing ‘Gear’".
    pub line: String,
}

impl Activity {
    fn new(mark: &'static str, line: impl Into<String>) -> Activity {
        Activity { mark, line: line.into() }
    }
}

/// The step goes to the page (and the Voice Assistant's capsule): the one
/// way a step leaves.
pub fn tell(sink: &dyn Sink, step: Activity) {
    sink.send(Event::Step { step });
}

/// The model is thinking before it answers or calls a tool.
pub fn thinking() -> Activity {
    Activity::new("think", "Thinking")
}

/// The answer's words are showing.
pub fn answering() -> Activity {
    Activity::new("answer", "Writing the answer")
}

/// Ask across meetings: the passages found on this PC before the answer
/// (`search/ask.rs`), or a Skill's (`chat/skills.rs`: "Running /todos").
pub fn searching_meetings(skill: Option<&str>) -> Activity {
    match skill.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => Activity::new("notes", format!("Running /{}", s.trim_start_matches('/'))),
        None => Activity::new("notes", "Searching your meetings"),
    }
}

/// **A tool call, as a step** — Yap's own tools and the provider's server
/// tools (Claude's `web_search` and `web_fetch`) alike. `args`: the call's
/// arguments (`Value::Null` while they're still coming: the line says what
/// it does without them). A note or a document named by its id is named by
/// its title (the notes store).
pub fn of_call(name: &str, args: &Value) -> Activity {
    describe(name, args, &|id| crate::notes::with_all(|notes| notes.iter().find(|n| n.id == id).map(|n| n.title.clone())))
}

/// [`of_call`] with the notes' titles from `title_of`. Pure.
pub fn describe(name: &str, args: &Value, title_of: &dyn Fn(u64) -> Option<String>) -> Activity {
    let text = |keys: &[&str]| keys.iter().find_map(|k| args[*k].as_str()).map(|s| one_line(s, WHAT_MAX)).filter(|s| !s.is_empty());
    let id = |keys: &[&str]| {
        keys.iter().find_map(|k| match &args[*k] {
            Value::Number(n) => n.as_u64().or_else(|| n.as_f64().filter(|f| f.fract() == 0.0 && *f >= 0.0).map(|f| f as u64)),
            Value::String(s) => s.trim().parse().ok(),
            _ => None,
        })
    };
    // A note's title, quoted ("‘Launch plan’"), when there's one.
    let titled = |keys: &[&str]| id(keys).and_then(title_of).map(|t| one_line(&t, WHAT_MAX)).filter(|t| !t.is_empty()).map(|t| quoted(&t));
    let with = |what: Option<String>, doing: &str, none: &str| what.map_or_else(|| none.to_string(), |w| format!("{doing} {w}"));
    let dotted = |what: Option<String>, doing: &str| what.map_or_else(|| doing.to_string(), |w| format!("{doing} · {w}"));
    match name {
        "web_search" => Activity::new("search", dotted(text(&["query", "q"]), "Searching the web")),
        "read_page" | "web_fetch" => {
            Activity::new("read", with(text(&["url"]).map(|u| crate::web::short_url(&u)), "Reading", "Reading a page"))
        }
        "search_notes" => Activity::new("notes", dotted(text(&["query"]), "Searching your notes")),
        "get_note" | "read_note" | "search_in_note" => Activity::new("notes", with(titled(&["note_id", "id"]), "Reading", "Reading a note")),
        "edit_note" | "update_note" => Activity::new("notes", with(titled(&["note_id", "id"]), "Changing", "Changing a note")),
        "create_note" => Activity::new("notes", with(text(&["title"]).map(|t| quoted(&t)), "Writing", "Writing a note")),
        "list_folders" => Activity::new("notes", "Looking at your folders"),
        // Yap's read-only tools on your Claude plan (its run endpoint,
        // integrations/claude_live.rs: `yap.exe mcp`'s, memory's).
        "list_meetings" => Activity::new("notes", "Looking through your meetings"),
        "search_meetings" => Activity::new("notes", dotted(text(&["query"]), "Searching your meetings")),
        "get_meeting" => Activity::new("notes", with(titled(&["id", "note_id"]), "Reading", "Reading a meeting")),
        "list_tasks" => Activity::new("notes", "Looking at your tasks"),
        "search_memory" => Activity::new("memory", dotted(text(&["query"]), "Searching Yap's memory")),
        // A Code chat's reads of its workspace on your Claude plan (Claude
        // Code's Read, Grep, Glob: claude_plan/map.rs): the file's name, the
        // words or the pattern.
        "read_file" => Activity::new("read", with(text(&["file_path", "path"]).map(|p| file_name(&p)), "Reading", "Reading a file")),
        "search_code" => Activity::new("search", dotted(text(&["pattern"]), "Searching the code")),
        "find_files" => Activity::new("search", dotted(text(&["pattern"]), "Looking for files")),
        "copy_to_clipboard" => Activity::new("tool", "Copying to the clipboard"),
        "open_app" => Activity::new("apps", with(text(&["name"]), "Opening", "Opening the app")),
        "list_apps" => Activity::new("apps", "Looking for the app"),
        "remember" | "forget" => Activity::new("memory", "Updating Yap's memory"),
        "search_chats" => Activity::new("chats", dotted(text(&["query"]), "Searching your chats")),
        crate::chat::tools::calendar::NAME => Activity::new("calendar", "Checking your calendar"),
        "create_document" => Activity::new("doc", with(text(&["title"]).map(|t| quoted(&t)), "Starting", "Starting a document")),
        "write_section" => Activity::new("doc", with(text(&["key", "heading"]).map(|k| quoted(&k)), "Writing", "Writing the document")),
        "rewrite_section" => Activity::new("doc", with(text(&["heading", "key"]).map(|k| quoted(&k)), "Rewriting", "Rewriting a section")),
        "read_document" => Activity::new("doc", with(titled(&["note_id", "id"]), "Reading", "Reading the document")),
        "edit_document" => Activity::new("doc", with(titled(&["note_id", "id"]), "Changing", "Changing the document")),
        "document_history" => Activity::new("doc", "Looking at the document's versions"),
        "restore_document" => Activity::new("doc", "Restoring an earlier version"),
        // Pictures from the web (chat/pictures.rs): the caption the model
        // gave, quoted.
        "look_at_picture" => Activity::new("picture", "Looking at a picture"),
        "show_picture" => Activity::new("picture", with(text(&["caption"]).map(|c| quoted(&c)), "Showing", "Showing a picture")),
        "find_pictures" => {
            Activity::new("picture", with(text(&["url"]).map(|u| crate::web::short_url(&u)), "Looking for pictures on", "Looking for pictures"))
        }
        // Pages beside the chat (chat/pages.rs): the page's address, or the
        // search's words (show_page: open_page's earlier name).
        "open_page" | "show_page" => match text(&["search"]) {
            Some(words) => Activity::new("search", format!("Opening a search for {}", quoted(&words))),
            None => Activity::new("read", with(text(&["url"]).map(|u| crate::web::short_url(&u)), "Bringing up", "Bringing up a page")),
        },
        "read_open_page" => Activity::new("read", "Reading the page beside the chat"),
        "look_at_screen" => Activity::new("picture", "Looking at your screen"),
        // Acting on the page beside the chat (chat/pages/act.rs): what, by
        // ref (the button's words come with its result, as the chip).
        "click" => Activity::new("page", "Clicking on the page beside the chat"),
        "type_text" => Activity::new("page", "Typing on the page beside the chat"),
        "scroll" => Activity::new("page", "Scrolling the page beside the chat"),
        "go_to" => match text(&["search"]) {
            Some(words) => Activity::new("search", format!("Opening a search for {}", quoted(&words))),
            None => Activity::new("page", with(text(&["url"]).map(|u| crate::web::short_url(&u)), "Going to", "Going to a page")),
        },
        "switch_tab" => Activity::new("page", "Switching tabs beside the chat"),
        "snap_page" => {
            Activity::new("picture", with(text(&["url"]).map(|u| crate::web::short_url(&u)), "Taking a shot of", "Taking a shot of a page"))
        }
        "offer_browser" => Activity::new("tool", "Adding a button to open it in your browser"),
        // Jobs (chat/tools/jobs.rs): the folder it works in.
        "start_job" => Activity::new("tool", with(text(&["folder"]).map(|f| crate::jobs::folders::name_of(&f)), "Starting a job in", "Starting a job")),
        "job_status" => Activity::new("tool", "Checking the job"),
        "message_job" => Activity::new("tool", "Telling the job"),
        "stop_job" => Activity::new("tool", "Stopping the job"),
        _ => Activity::new("tool", "Working"),
    }
}

/// `s` on one line, at most `max` characters (then "…"). Pure.
fn one_line(s: &str, max: usize) -> String {
    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
    match one.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", one[..i].trim_end()),
        None => one,
    }
}

/// A title as a line quotes it: ‘Gear’.
fn quoted(s: &str) -> String {
    format!("\u{2018}{s}\u{2019}")
}

/// A path's last part (`D:\Projects\garden-planner\src\app.ts` → `app.ts`),
/// either slash. Pure.
fn file_name(path: &str) -> String {
    path.trim().trim_end_matches(['\\', '/']).rsplit(['\\', '/']).next().unwrap_or("").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn titles(id: u64) -> Option<String> {
        (id == 7).then(|| "Tidecaller guide".to_string())
    }

    fn line(name: &str, args: Value) -> String {
        describe(name, &args, &titles).line
    }

    #[test]
    fn each_tool_call_says_what_it_does_and_with_what() {
        assert_eq!(line("web_search", json!({ "query": "tidecaller  lone wolf\nbuild" })), "Searching the web · tidecaller lone wolf build");
        assert_eq!(line("web_search", Value::Null), "Searching the web");
        assert_eq!(line("start_job", json!({ "folder": r"D:\Projects\garden-planner" })), "Starting a job in garden-planner");
        assert_eq!(line("start_job", json!({ "folder": "garden-planner" })), "Starting a job in garden-planner");
        assert_eq!(line("job_status", json!({})), "Checking the job");
        // A Code chat's reads of its workspace (on the Claude plan): the file's name only.
        assert_eq!(line("read_file", json!({ "file_path": r"D:\Projects\garden-planner\src\app.ts" })), "Reading app.ts");
        assert_eq!(line("read_file", json!({ "file_path": "/home/imogen/garden-planner/README.md" })), "Reading README.md");
        assert_eq!(line("read_file", Value::Null), "Reading a file");
        assert_eq!(line("search_code", json!({ "pattern": "water(ing)?" })), "Searching the code · water(ing)?");
        assert_eq!(line("find_files", json!({ "pattern": "**/*.ts" })), "Looking for files · **/*.ts");
        assert_eq!(line("web_fetch", json!({ "url": "https://www.emberfall.example/wiki/Talents/" })), "Reading emberfall.example/wiki/Talents");
        assert_eq!(line("read_page", json!({ "url": "" })), "Reading a page");
        assert_eq!(line("search_notes", json!({ "query": "kitchen budget" })), "Searching your notes · kitchen budget");
        assert_eq!(line("read_note", json!({ "note_id": 7, "field": "content" })), "Reading ‘Tidecaller guide’");
        assert_eq!(line("get_note", json!({ "id": "7" })), "Reading ‘Tidecaller guide’");
        assert_eq!(line("edit_note", json!({ "note_id": 99 })), "Changing a note");
        assert_eq!(line("create_note", json!({ "title": "Tent list" })), "Writing ‘Tent list’");
        assert_eq!(line("create_document", json!({ "title": "Lone wolf ranger", "sections": [] })), "Starting ‘Lone wolf ranger’");
        assert_eq!(line("write_section", json!({ "note_id": 7, "key": "Character creation", "markdown": "…" })), "Writing ‘Character creation’");
        assert_eq!(line("write_section", Value::Null), "Writing the document");
        assert_eq!(line("rewrite_section", json!({ "note_id": 7, "heading": "Gear" })), "Rewriting ‘Gear’");
        assert_eq!(line("edit_document", json!({ "note_id": 7.0, "ops": [] })), "Changing ‘Tidecaller guide’");
        assert_eq!(line("read_document", json!({ "note_id": 7 })), "Reading ‘Tidecaller guide’");
        assert_eq!(line("check_calendar", json!({ "when": "tomorrow" })), "Checking your calendar");
        assert_eq!(line("open_app", json!({ "name": "Paint" })), "Opening Paint");
        assert_eq!(line("search_chats", json!({ "query": "tent" })), "Searching your chats · tent");
        assert_eq!(line("look_at_picture", json!({ "picture": "p2" })), "Looking at a picture");
        assert_eq!(line("show_picture", json!({ "picture": "p2", "caption": "Fort Joy treasure map" })), "Showing ‘Fort Joy treasure map’");
        assert_eq!(line("show_picture", Value::Null), "Showing a picture");
        assert_eq!(line("find_pictures", json!({ "url": "https://wiki.example.com/wiki/Fort_Joy" })), "Looking for pictures on wiki.example.com/wiki/Fort_Joy");
        assert_eq!(describe("show_picture", &Value::Null, &titles).mark, "picture");
        assert_eq!(line("show_page", json!({ "url": "https://maps.example.com/fort-joy", "section": "Chests" })), "Bringing up maps.example.com/fort-joy");
        assert_eq!(line("show_page", Value::Null), "Bringing up a page");
        assert_eq!(line("open_page", json!({ "search": "train times" })), "Opening a search for ‘train times’");
        assert_eq!(line("read_open_page", json!({})), "Reading the page beside the chat");
        assert_eq!(line("look_at_screen", json!({})), "Looking at your screen");
        assert_eq!(line("snap_page", json!({ "url": "https://www.maps.example.com/fort-joy/" })), "Taking a shot of maps.example.com/fort-joy");
        assert_eq!(line("offer_browser", json!({ "url": "https://maps.example.com/x", "why": "the map" })), "Adding a button to open it in your browser");
        assert_eq!(line("click", json!({ "ref": 4 })), "Clicking on the page beside the chat");
        assert_eq!(line("type_text", json!({ "ref": 2, "text": "Kellbrook" })), "Typing on the page beside the chat");
        assert_eq!(line("scroll", json!({ "to": "down" })), "Scrolling the page beside the chat");
        assert_eq!(line("go_to", json!({ "url": "https://rail.example.com/timetables" })), "Going to rail.example.com/timetables");
        assert_eq!(line("go_to", json!({ "search": "trains to Port Ellis" })), "Opening a search for ‘trains to Port Ellis’");
        assert_eq!(line("switch_tab", json!({ "tab": 2 })), "Switching tabs beside the chat");
        assert_eq!(describe("click", &Value::Null, &titles).mark, "page");
        assert_eq!(line("something_new", json!({})), "Working");
        // Yap's read-only tools on a Claude plan.
        assert_eq!(line("search_meetings", json!({ "query": "launch" })), "Searching your meetings · launch");
        assert_eq!(line("get_meeting", json!({ "id": 7 })), "Reading ‘Tidecaller guide’");
        assert_eq!(line("get_meeting", json!({ "id": 99 })), "Reading a meeting");
        assert_eq!(line("search_memory", json!({ "query": "Sam" })), "Searching Yap's memory · Sam");
        assert_eq!(line("list_tasks", json!({})), "Looking at your tasks");
        // The kinds the page may mark.
        assert_eq!(describe("web_search", &Value::Null, &titles).mark, "search");
        assert_eq!(describe("web_fetch", &Value::Null, &titles).mark, "read");
        assert_eq!(describe("write_section", &Value::Null, &titles).mark, "doc");
    }

    #[test]
    fn a_long_query_is_cut_and_the_turns_own_steps_are_plain() {
        let long = "word ".repeat(40);
        let l = line("web_search", json!({ "query": long }));
        assert!(l.ends_with('…') && l.chars().count() <= "Searching the web · ".chars().count() + WHAT_MAX + 1, "{l}");
        assert_eq!(thinking().line, "Thinking");
        assert_eq!(answering().line, "Writing the answer");
        assert_eq!(searching_meetings(None).line, "Searching your meetings");
        assert_eq!(searching_meetings(Some("todos")).line, "Running /todos");
    }

    #[test]
    fn a_step_goes_out_as_one_event_tagged_step() {
        #[derive(Default)]
        struct Log(std::sync::Mutex<Vec<Value>>);
        impl Sink for Log {
            fn send(&self, ev: Event) {
                self.0.lock().unwrap().push(serde_json::to_value(ev).unwrap());
            }
        }
        let log = Log::default();
        tell(&log, describe("web_search", &json!({ "query": "tidecaller" }), &titles));
        assert_eq!(log.0.lock().unwrap().as_slice(), [json!({ "kind": "step", "step": { "mark": "search", "line": "Searching the web · tidecaller" } })]);
    }
}

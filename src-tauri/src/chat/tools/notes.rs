//! The note tools: their definitions, the keyword search the eager RAG
//! shares, and running them over `notes.rs` — OpenWhispr's six (search,
//! get, create, update, list folders, copy) and Yap's own for reading and
//! changing part of a note (`search_in_note`, `read_note`, `edit_note`).

use serde_json::{json, Value};

use crate::chat::note_text::{self, Field};

use super::{tool_error, tool_result, GET_NOTE_TOKENS, MAX_CONTENT_LENGTH};

/// The OpenAI `tools` array (OpenWhispr's schemas near-verbatim from their
/// tool files; `search_in_note`, `read_note` and `edit_note` are Yap's).
pub fn definitions() -> Value {
    json!([
        {
            "type": "function",
            "function": {
                "name": "search_notes",
                "description": "Search the user's notes and meetings by keywords and meaning. Returns matching notes with title, date, relevance score, and the passages that matched.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "The search query to find relevant notes" },
                        "limit": { "type": "number", "description": "Maximum number of results to return (default 5)" }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "get_note",
                "description": "Get a note by ID: its title, notes (content), AI summary (enhanced) and a meeting's transcript. Long parts come shortened (cut lists them); read the rest with read_note.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "number", "description": "The note ID to retrieve" }
                    },
                    "required": ["id"],
                    "additionalProperties": false
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "search_in_note",
                "description": "Find a word or phrase in one note (not case-sensitive): each line it is on, numbered, with its field and the lines around the first few. Searches content, enhanced and transcript unless field is given.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "note_id": { "type": "number", "description": "The note to search" },
                        "query": { "type": "string", "description": "The word or phrase to find" },
                        "field": { "type": "string", "enum": ["all", "content", "enhanced", "transcript"], "description": "Where to look (default all)" }
                    },
                    "required": ["note_id", "query"],
                    "additionalProperties": false
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "read_note",
                "description": "Read one part of a note a page at a time, lines numbered (\"12| text\"; the number is not part of the note). While more is true, call again with offset = next_offset.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "note_id": { "type": "number", "description": "The note to read" },
                        "field": { "type": "string", "enum": ["content", "enhanced", "transcript"], "description": "content (the user's notes), enhanced (the AI summary) or transcript (read-only)" },
                        "offset": { "type": "number", "description": "The first line, 1 = the start (default 1)" },
                        "limit": { "type": "number", "description": "How many lines (default 200, at most 400)" }
                    },
                    "required": ["note_id", "field"],
                    "additionalProperties": false
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "edit_note",
                "description": "Change part of a note: replace old_text, copied exactly from the note (without line numbers) and appearing exactly once in the field, with new_text. Nothing changes if it doesn't match. Empty new_text deletes; empty old_text fills an empty field. The transcript can't be edited.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "note_id": { "type": "number", "description": "The note to change" },
                        "field": { "type": "string", "enum": ["content", "enhanced", "title"], "description": "content (the user's notes), enhanced (the AI summary) or title" },
                        "old_text": { "type": "string", "description": "The exact text to replace" },
                        "new_text": { "type": "string", "description": "The text to put in its place" }
                    },
                    "required": ["note_id", "field", "old_text", "new_text"],
                    "additionalProperties": false
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "create_note",
                "description": "Create a note. Leave folder empty for Personal, or pass an existing folder's name (from list_folders). An unknown folder is refused unless new_folder is true, which only a user asking for a new folder allows.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string", "description": "The note's title" },
                        "content": { "type": "string", "description": "The note's content" },
                        "folder": { "type": "string", "description": "An existing folder's name, or empty" },
                        "new_folder": { "type": "boolean", "description": "true only when the user asked for a new folder" }
                    },
                    "required": ["title", "content"],
                    "additionalProperties": false
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "update_note",
                "description": "Move a note to an existing folder (from list_folders), or replace its title or its whole content (only for a full rewrite the user asked for; use edit_note to change part of it). An unknown folder is refused unless new_folder is true, which only a user asking for a new folder allows.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "number", "description": "The note ID" },
                        "title": { "type": "string", "description": "New title (optional)" },
                        "content": { "type": "string", "description": "New content replacing ALL of it (optional)" },
                        "folder": { "type": "string", "description": "An existing folder to move it to (optional)" },
                        "new_folder": { "type": "boolean", "description": "true only when the user asked for a new folder" }
                    },
                    "required": ["id"],
                    "additionalProperties": false
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "list_folders",
                "description": "List all available note folders. Use before create_note or update_note to file a note in a folder that exists.",
                "parameters": { "type": "object", "properties": {}, "additionalProperties": false }
            }
        },
        {
            "type": "function",
            "function": {
                "name": "copy_to_clipboard",
                "description": "Copy text to the user's system clipboard.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "text": { "type": "string", "description": "The text to copy to the clipboard" }
                    },
                    "required": ["text"],
                    "additionalProperties": false
                }
            }
        }
    ])
}

/// **`search_notes`** (2026-10-25): Ask across meetings' search — BM25 per
/// passage and, with search by meaning on, meaning; the dates and voices
/// the query names (`search/library.rs` `notes_for`) — each note with the
/// passages that matched (where each is from, ≤ 600 characters), so the
/// model reads the part that answers rather than a note's first lines.
/// (A model evaluation compares the whole-note keyword search it replaced:
/// `search::library::keyword_only`.)
pub async fn search_notes_tool(args: &Value) -> Value {
    if crate::search::library::keyword_only() {
        return execute("search_notes", args);
    }
    let query = args["query"].as_str().unwrap_or("").trim().to_string();
    let limit = arg_u64(args, &["limit"]).unwrap_or(5).clamp(1, 20) as usize;
    if query.chars().count() < 2 {
        return tool_result(true, json!([]), format!("No notes found for \"{query}\""));
    }
    let (found, semantic) = crate::search::library::notes_for(&query, limit).await;
    let results: Vec<Value> = found
        .iter()
        .filter_map(|f| {
            let n = crate::notes::get(f.id)?;
            Some(json!({
                "id": n.id,
                "title": n.title,
                "date": n.created_ts,
                "type": n.note_type,
                "score": (f.score * 1000.0).round() / 1000.0,
                "content": matched_passages(&f.passages, 600),
            }))
        })
        .collect();
    tracing::info!(found = results.len(), semantic, "Chat: search_notes");
    let display = if results.is_empty() {
        format!("No notes found for \"{query}\"")
    } else {
        format!("Found {} note{} for \"{query}\"{}", results.len(), if results.len() == 1 { "" } else { "s" }, if semantic { " by meaning" } else { "" })
    };
    tool_result(true, json!(results), display)
}

/// A note's matched passages as `search_notes` gives them: each with where
/// it's from ("(transcript, 12:40)", "(notes)", "(summary)"), at most
/// `max` characters in all. Pure.
pub fn matched_passages(passages: &[crate::search::ask::Passage], max: usize) -> String {
    use crate::search::chunks::Field as F;
    let mut out = String::new();
    for p in passages {
        let from = match (p.field, &p.at) {
            (F::Transcript, Some(at)) => format!("(transcript, {at})"),
            (F::Transcript, None) => "(transcript)".to_string(),
            (F::Summary, _) => "(summary)".to_string(),
            _ => "(notes)".to_string(),
        };
        let piece = format!("{from} {}", p.text.trim());
        if !out.is_empty() {
            out.push_str("\n…\n");
        }
        out.push_str(&piece);
        if out.chars().count() >= max {
            break;
        }
    }
    if out.chars().count() > max {
        out = format!("{}…", out.chars().take(max).collect::<String>().trim_end());
    }
    out
}

/// Keyword search over the notes library (the same scorer the eager RAG uses:
/// title hits weighted 3×, body = content + enhanced + transcript).
pub fn search_notes(query: &str, limit: usize) -> Vec<(usize, crate::notes::Note)> {
    let words: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| w.chars().count() > 2)
        .collect();
    let mut scored: Vec<(usize, crate::notes::Note)> =
        scored_notes(&words).into_iter().map(|(score, _, n)| (score, n)).collect();
    scored.truncate(limit);
    scored
}

/// Every note matching one of `words`, best first: (score, distinct words
/// matched, whether the title matched one, the note).
fn scored_notes(words: &[String]) -> Vec<(usize, (usize, bool), crate::notes::Note)> {
    if words.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(usize, (usize, bool), crate::notes::Note)> = crate::notes::all()
        .into_iter()
        .filter_map(|n| {
            let title = n.title.to_lowercase();
            let body = format!(
                "{}\n{}\n{}",
                n.content.to_lowercase(),
                n.enhanced_content.to_lowercase(),
                n.transcript
                    .iter()
                    .map(|s| s.text.to_lowercase())
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            let per_word: Vec<(usize, usize)> =
                words.iter().map(|w| (title.matches(w.as_str()).count(), body.matches(w.as_str()).count())).collect();
            let score: usize = per_word.iter().map(|(t, b)| t * 3 + b).sum();
            let distinct = per_word.iter().filter(|(t, b)| t + b > 0).count();
            let in_title = per_word.iter().any(|(t, _)| *t > 0);
            (score > 0).then_some((score, (distinct, in_title), n))
        })
        .collect();
    scored.sort_by_key(|s| std::cmp::Reverse(s.0));
    scored
}

/// Words that say nothing about which note a question is about.
const RAG_STOP_WORDS: &[&str] = &[
    "and", "the", "for", "are", "was", "were", "what", "what's", "whats", "when", "when's", "where", "where's", "which", "who",
    "who's", "whose", "why", "how", "how's", "with", "this", "that", "that's", "these", "those", "from", "have", "has", "had",
    "does", "did", "can", "could", "would", "should", "will", "about", "into", "your", "you", "our", "their", "there", "then",
    "than", "them", "they", "she", "her", "him", "his", "its", "it's", "any", "all", "some", "one", "also", "just", "not", "but",
    "yet", "too", "very", "much", "many", "more", "most", "get", "got", "i'm", "i've", "i'll", "let's", "isn't", "don't",
    "doesn't", "tell", "show", "give", "find", "know", "need", "want", "please", "thanks", "remind", "again", "still", "now",
    "today", "my", "me", "mine",
];

/// **The eager RAG's notes** (`turn::rag_context`): the keyword search,
/// stricter — without words like "what" or "and", and only notes that match
/// two of the question's words or one in their title (or the one word a
/// short question has). In a question's context, right before a follow-up
/// ("Whose house is it at, and what time?" after a book club answer), a
/// note that only shared "time" made Qwen3.5-4B answer about that note
/// instead of the conversation (the model evaluations, 2026-10-22,
/// `notes-long-chat`). The `search_notes` tool keeps the loose search.
pub fn rag_notes(query: &str, limit: usize) -> Vec<(usize, crate::notes::Note)> {
    let words: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').replace('’', "'"))
        .map(|w| w.trim_matches('\'').to_string())
        .filter(|w| w.chars().count() > 2 && !RAG_STOP_WORDS.contains(&w.as_str()))
        .collect();
    let need = words.len().min(2);
    let mut kept: Vec<(usize, crate::notes::Note)> = scored_notes(&words)
        .into_iter()
        .filter(|(_, (distinct, in_title), _)| *in_title || *distinct >= need)
        .map(|(score, _, n)| (score, n))
        .collect();
    kept.truncate(limit);
    kept
}

/// A whole-number argument under any of `keys`: a number (12 or 12.0) or a
/// string of one ("12"), as models send them.
fn arg_u64(args: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|k| match &args[*k] {
        Value::Number(n) => n.as_u64().or_else(|| {
            n.as_f64()
                .filter(|f| f.fract() == 0.0 && *f >= 0.0)
                .map(|f| f as u64)
        }),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    })
}

/// A note's title for chips: quoted, ≤ 40 chars, "Untitled note" for none.
fn chip_title(title: &str) -> String {
    let t = title.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() {
        return "\"Untitled note\"".to_string();
    }
    format!("\"{}\"", clip(&t, 40))
}

/// The chip for an edit that went through: what changed, in a few words.
fn edit_chip(title: &str, new_title: &str, e: &note_text::Edit) -> String {
    let t = chip_title(title);
    if !e.changed {
        return format!("No change to {t}");
    }
    match e.field {
        Field::Title if e.filled => format!("Named the note {}", chip_title(new_title)),
        Field::Title => format!("Renamed {t} to {}", chip_title(new_title)),
        Field::Enhanced if e.filled => format!("Wrote a summary for {t}"),
        Field::Content if e.filled => format!("Wrote in {t}"),
        field => {
            let place = if field == Field::Enhanced {
                format!("the summary of {t}")
            } else {
                t
            };
            match note_text::change_preview(&e.old, &e.new) {
                (o, n) if o.is_empty() && n.is_empty() => format!("Edited {place}"),
                (o, n) if o.is_empty() => format!("Edited {place}: added \"{n}\""),
                (o, n) if n.is_empty() => format!("Edited {place}: removed \"{o}\""),
                (o, n) => format!("Edited {place}: \"{o}\" \u{2192} \"{n}\""),
            }
        }
    }
}

/// The note a tool call changed, if it did: a note created, updated or
/// edited (an edit that changed nothing doesn't count). The loop reports it
/// so every window shows the change.
pub fn changed_note(name: &str, result: &Value) -> Option<u64> {
    if result["success"] != json!(true) {
        return None;
    }
    let id = result["data"]["id"].as_u64();
    match name {
        "create_note" | "update_note" => id,
        "edit_note" if result["data"]["changed"] == json!(true) => id,
        _ => None,
    }
}

/// Execute one tool call locally. Returns a ToolResult-shaped JSON value
/// (their `{success, data, displayText}`, plus `error` on Yap's tools'
/// failures); the whole thing is fed back to the model, `displayText` is
/// also surfaced in the chat UI.
pub fn execute(name: &str, args: &Value) -> Value {
    let mut result = run(name, args);
    // A note changed in Yap but not saved to disk (saving.rs): still a
    // change every window shows, but the model is told, so it never says
    // it's saved, and the chip says so.
    if changed_note(name, &result).is_some() {
        if let Err(e) = crate::notes::SAVER.status() {
            result["notSaved"] = json!(format!(
                "{e} The change is in Yap and is saved with the next change, or when the user clicks Try again. Tell the user it isn't saved yet."
            ));
            result["displayText"] = json!(e.to_string());
        }
    }
    result
}

fn run(name: &str, args: &Value) -> Value {
    match name {
        "search_notes" => {
            let query = args["query"].as_str().unwrap_or("");
            let limit = args["limit"].as_u64().unwrap_or(5) as usize;
            let results: Vec<Value> = search_notes(query, limit)
                .into_iter()
                .map(|(score, n)| {
                    let content_src = if !n.enhanced_content.trim().is_empty() {
                        &n.enhanced_content
                    } else {
                        &n.content
                    };
                    json!({
                        "id": n.id,
                        "title": n.title,
                        "date": n.created_ts,
                        "type": n.note_type,
                        "score": score,
                        "content": content_src.chars().take(MAX_CONTENT_LENGTH).collect::<String>(),
                    })
                })
                .collect();
            let display = if results.is_empty() {
                format!("No notes found for \"{query}\"")
            } else {
                format!(
                    "Found {} note{} for \"{query}\"",
                    results.len(),
                    if results.len() == 1 { "" } else { "s" }
                )
            };
            tool_result(true, json!(results), display)
        }
        "get_note" => {
            let id = arg_u64(args, &["id", "note_id"]).unwrap_or(0);
            match crate::notes::get(id) {
                Some(n) => {
                    // The notes and the summary apart (an edit names which it
                    // changes), each bounded so a long note can't fill the
                    // context; read_note pages through the rest.
                    let (content, content_cut) = note_text::fit(&n.content, GET_NOTE_TOKENS);
                    let (enhanced, enhanced_cut) =
                        note_text::fit(&n.enhanced_content, GET_NOTE_TOKENS);
                    // A long meeting comes as its digests + the latest
                    // stretch, so one tool result can't fill the context.
                    let transcript = if n.transcript.is_empty() {
                        String::new()
                    } else {
                        crate::meetings::meeting_summary::ask_context(&n, 3_000)
                    };
                    let display = format!("Retrieved note: \"{}\"", n.title);
                    let mut data = json!({
                        "id": n.id,
                        "title": n.title,
                        "content": content,
                        "enhanced": enhanced,
                        "transcript": transcript,
                        "type": n.note_type,
                        "folder": n.folder,
                        "created_at": n.created_ts,
                        "updated_at": n.updated_ts,
                    });
                    let cut: Vec<&str> = [("content", content_cut), ("enhanced", enhanced_cut)]
                        .iter()
                        .filter(|(_, c)| *c)
                        .map(|(f, _)| *f)
                        .collect();
                    if !cut.is_empty() {
                        data["cut"] = json!(cut);
                    }
                    tool_result(true, data, display)
                }
                None => tool_result(false, Value::Null, format!("Note with ID {id} not found")),
            }
        }
        "search_in_note" => search_in_note(args),
        "read_note" => read_note(args),
        "edit_note" => edit_note(args),
        "create_note" => {
            let title = args["title"].as_str().unwrap_or("").trim();
            let content = args["content"].as_str().unwrap_or("");
            let folder = args["folder"].as_str().unwrap_or("").trim();
            if title.is_empty() && content.trim().is_empty() {
                return tool_result(false, Value::Null, "Failed to create note".to_string());
            }
            let chip = format!("Couldn't create {}", chip_title(title));
            let (folder, folder_created) = match note_folder(folder, args, &chip) {
                Ok(Some(f)) => f,
                Ok(None) => (String::new(), false),
                Err(e) => return e,
            };
            let note = crate::notes::create(title, content, "chat", &folder);
            let suffix = if folder_created {
                format!(" in new folder \"{folder}\"")
            } else {
                String::new()
            };
            tool_result(
                true,
                json!({ "id": note.id, "title": note.title, "folder": note.folder }),
                format!("Created note: \"{title}\"{suffix}"),
            )
        }
        "update_note" => {
            let id = arg_u64(args, &["id", "note_id"]).unwrap_or(0);
            let title = args["title"].as_str().map(|s| s.to_string());
            let content = args["content"].as_str().map(|s| s.to_string());
            let folder = args["folder"].as_str().map(|s| s.to_string());
            if title.is_none() && content.is_none() && folder.is_none() {
                return tool_result(
                    false,
                    Value::Null,
                    "At least one of title, content, or folder must be provided".to_string(),
                );
            }
            if crate::notes::get(id).is_none() {
                return tool_result(false, Value::Null, format!("Failed to update note: Note with ID {id} not found"));
            }
            // The folder is checked (and a new one made) before anything
            // changes: a refused folder leaves the note as it was.
            let folder = match folder.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
                None => None,
                Some(f) => match note_folder(f, args, "Couldn't move the note") {
                    Ok(found) => found.map(|(name, _)| name),
                    Err(e) => return e,
                },
            };
            match crate::notes::update(id, title, content, folder, None) {
                Ok(_) => tool_result(true, json!({ "id": id }), "Updated note".to_string()),
                Err(e) => tool_result(false, Value::Null, format!("Failed to update note: {e}")),
            }
        }
        "list_folders" => {
            let folders = crate::notes::folders();
            let display = if folders.is_empty() {
                "No folders".to_string()
            } else {
                format!("Folders: {}", folders.join(", "))
            };
            tool_result(true, json!(folders), display)
        }
        "copy_to_clipboard" => {
            let text = args["text"].as_str().unwrap_or("");
            match arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_string())) {
                Ok(_) => {
                    let preview: String = if text.chars().count() > 100 {
                        format!("{}...", text.chars().take(100).collect::<String>())
                    } else {
                        text.to_string()
                    };
                    tool_result(true, Value::Null, format!("Copied to clipboard: \"{preview}\""))
                }
                Err(e) => tool_result(
                    false,
                    Value::Null,
                    format!("Failed to copy to clipboard: {e}"),
                ),
            }
        }
        other => tool_result(false, Value::Null, format!("Unknown tool: {other}")),
    }
}

/// The folder `create_note` / `update_note` file a note into (`folder`, as
/// the model passed it). Yap's own rule, not OpenWhispr's (which made any
/// folder the model named, so a chat could leave invented folders behind):
/// - empty → `None` (a new note goes to Personal; a note isn't moved);
/// - a folder that exists, in any case → that folder, as the list spells it;
/// - a new name → made only with `new_folder: true` (the user asked for a
///   new folder); without it, an error for the model naming the folders
///   there are and telling it to leave `folder` empty instead.
///
/// `Some((name, created))`.
fn note_folder(folder: &str, args: &Value, chip: &str) -> Result<Option<(String, bool)>, Value> {
    let folder = folder.trim();
    if folder.is_empty() {
        return Ok(None);
    }
    if let Some(existing) = crate::notes::find_folder(folder) {
        return Ok(Some((existing, false)));
    }
    let asked = match &args["new_folder"] {
        Value::Bool(b) => *b,
        Value::String(s) => s.trim().eq_ignore_ascii_case("true"),
        _ => false,
    };
    if asked {
        crate::notes::folder_create(folder);
        return Ok(Some((folder.to_string(), true)));
    }
    let folders = crate::notes::folders()
        .iter()
        .map(|f| format!("\"{f}\""))
        .collect::<Vec<_>>()
        .join(", ");
    Err(tool_error(
        format!(
            "There's no folder called \"{folder}\", so nothing changed. The folders are: {folders}. Use one of them, or leave folder empty (a new note then goes to Personal). Only create a new folder, by passing new_folder: true, when the user asked for a new folder."
        ),
        json!({ "folders": crate::notes::folders() }),
        format!("{chip}: no folder called \"{}\"", clip(folder, 40)),
    ))
}

/// The `field` argument of Yap's note tools: `default` when it's missing,
/// an error result (for the model) when it names no field.
fn field_arg(args: &Value, default: Field, chip: &str) -> Result<Field, Value> {
    match args["field"].as_str().map(str::trim).filter(|f| !f.is_empty()) {
        None => Ok(default),
        Some(f) => Field::parse(f).ok_or_else(|| {
            tool_error(
                format!("Unknown field \"{f}\": use content (the notes), enhanced (the AI summary), title or transcript."),
                Value::Null,
                chip.to_string(),
            )
        }),
    }
}

fn no_note_id(chip: &str) -> Value {
    tool_error(
        "note_id is required: the ID of the note (search_notes finds it).".to_string(),
        Value::Null,
        chip.to_string(),
    )
}

/// `search_in_note`: the lines of one note a phrase is on, with context
/// (`note_text::search`).
fn search_in_note(args: &Value) -> Value {
    const CHIP: &str = "Couldn't search a note";
    let Some(id) = arg_u64(args, &["note_id", "id"]) else {
        return no_note_id(CHIP);
    };
    let all = args["field"]
        .as_str()
        .map(str::trim)
        .is_none_or(|f| f.is_empty() || f.eq_ignore_ascii_case("all"));
    let fields = if all {
        vec![Field::Content, Field::Enhanced, Field::Transcript]
    } else {
        match field_arg(args, Field::Content, CHIP) {
            Ok(f) => vec![f],
            Err(e) => return e,
        }
    };
    let Some(note) = crate::notes::get(id) else {
        return tool_result(false, Value::Null, format!("Note with ID {id} not found"));
    };
    let query = args["query"].as_str().unwrap_or("").trim();
    if query.is_empty() {
        return tool_error(
            "query is empty: give a word or phrase to find.".to_string(),
            Value::Null,
            CHIP.to_string(),
        );
    }
    let found = note_text::search(&note, query, &fields);
    let total = found.total();
    let per_field: serde_json::Map<String, Value> = found
        .lines
        .iter()
        .map(|(f, n)| (f.name().to_string(), json!(n)))
        .collect();
    let mut data = json!({
        "id": note.id,
        "title": note.title,
        "query": query,
        "matching_lines": total,
        "per_field": per_field,
        "hits": found.hits,
    });
    if found.more() {
        data["more"] = json!(format!(
            "{} of {total} matching lines shown. Search for a longer phrase, or page through with read_note.",
            found.hits.len()
        ));
    }
    if total == 0 {
        data["hint"] = json!("No match. Try one distinctive word, or read the note with read_note.");
    }
    let q = clip(query, 30);
    let t = chip_title(&note.title);
    let display = match total {
        0 => format!("\"{q}\" isn't in {t}"),
        1 => format!("Found \"{q}\" on 1 line in {t}"),
        n => format!("Found \"{q}\" on {n} lines in {t}"),
    };
    tool_result(true, data, display)
}

/// The first `n` chars of `s`, with "…" when that cut it.
fn clip(s: &str, n: usize) -> String {
    match s.char_indices().nth(n) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

/// `read_note`: one page of a note's notes, summary or transcript
/// (`note_text::read_page`).
fn read_note(args: &Value) -> Value {
    const CHIP: &str = "Couldn't read a note";
    let Some(id) = arg_u64(args, &["note_id", "id"]) else {
        return no_note_id(CHIP);
    };
    let field = match field_arg(args, Field::Content, CHIP) {
        Ok(f) => f,
        Err(e) => return e,
    };
    let offset = arg_u64(args, &["offset"]).unwrap_or(1) as usize;
    let limit = arg_u64(args, &["limit"]).map_or(note_text::DEFAULT_PAGE_LINES, |l| l as usize);
    let Some(note) = crate::notes::get(id) else {
        return tool_result(false, Value::Null, format!("Note with ID {id} not found"));
    };
    let t = chip_title(&note.title);
    let text = note_text::field_text(&note, field);
    match note_text::read_page(&text, offset, limit, note_text::PAGE_TOKENS) {
        Err(e) => tool_error(
            format!("{e}. Read from offset 1, or one within the field."),
            json!({ "id": note.id, "field": field.name() }),
            format!("Couldn't read {t}"),
        ),
        Ok(page) if page.total_lines == 0 => tool_result(
            true,
            json!({
                "id": note.id,
                "title": note.title,
                "field": field.name(),
                "total_lines": 0,
                "empty": true,
            }),
            format!("Read {t}: its {} is empty", field.label()),
        ),
        Ok(page) => {
            let display = format!(
                "Read {t} ({}, lines {}\u{2013}{} of {})",
                field.label(),
                page.from,
                page.to,
                page.total_lines
            );
            let cut_line = page.cut_line;
            let mut data = serde_json::to_value(&page).unwrap_or(Value::Null);
            data["id"] = json!(note.id);
            data["title"] = json!(note.title);
            data["field"] = json!(field.name());
            if let Some(n) = cut_line {
                data["hint"] = json!(format!(
                    "Line {n} is longer than a page, so it was cut; search_in_note finds text in it."
                ));
            }
            tool_result(true, data, display)
        }
    }
}

/// `edit_note`: replace text that appears exactly once in a note's notes,
/// summary or title, or fill an empty one (`note_text::apply_edit`, under
/// the store lock via `notes::modify`).
fn edit_note(args: &Value) -> Value {
    use note_text::{EditError, Miss};
    const CHIP: &str = "Couldn't edit a note";
    let Some(id) = arg_u64(args, &["note_id", "id"]) else {
        return no_note_id(CHIP);
    };
    let field = match field_arg(args, Field::Content, CHIP) {
        Ok(f) => f,
        Err(e) => return e,
    };
    let old = args["old_text"].as_str().unwrap_or("");
    let Some(new) = args["new_text"].as_str() else {
        return tool_error(
            "new_text is required (\"\" deletes old_text). Nothing was changed.".to_string(),
            Value::Null,
            CHIP.to_string(),
        );
    };
    let outcome = crate::notes::modify(id, |note| {
        let title = note.title.clone();
        let edit = note_text::apply_edit(note, field, old, new);
        let changed = edit.as_ref().is_ok_and(|e| e.changed);
        Ok(((edit, title, note.title.clone()), changed))
    });
    match outcome {
        Err(e) => tool_result(false, Value::Null, e),
        Ok((Err(err), title, _)) => {
            let mut data = json!({ "id": id, "field": field.name() });
            match &err {
                EditError::Miss(Miss::NotFound { near, .. }) if !near.is_empty() => {
                    data["near"] = json!(near
                        .iter()
                        .map(|(line, text)| json!({ "line": line, "text": text }))
                        .collect::<Vec<_>>());
                }
                EditError::Miss(Miss::Ambiguous { lines, .. }) => data["lines"] = json!(lines),
                _ => {}
            }
            tool_error(
                err.message(field),
                data,
                format!("Couldn't edit {}: {}", chip_title(&title), err.short()),
            )
        }
        Ok((Ok(edit), title, new_title)) => {
            let display = edit_chip(&title, &new_title, &edit);
            tool_result(
                true,
                json!({
                    "id": id,
                    "field": field.name(),
                    "changed": edit.changed,
                    "line": edit.line,
                    "now": edit.now,
                }),
                display,
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::tools::tests::{call, plan_note, PLAN};

    #[test]
    fn the_eager_rag_keeps_only_notes_that_match_the_question() {
        crate::notes::create("Kitchen quotes", "Harlow Kitchens: £14,200 including fitting, eight weeks lead time.", "manual", "");
        crate::notes::create("Book club", "Next book: The Remains of the Day. Meeting at Sarah's on the 28th at 7:30.", "manual", "");
        crate::notes::create("Home network", "Home Wi-Fi password: Gr33nTea!77", "manual", "");
        crate::notes::create("Cabin trip", "Wi-Fi network: CabinGuest, password: MooseLake42", "manual", "");
        let titles = |q: &str| rag_notes(q, 5).into_iter().map(|(_, n)| n.title).collect::<Vec<_>>();
        // A follow-up sharing one ordinary word with a note: no note (the
        // loose search the tool runs still finds it).
        assert!(titles("Whose house is it at, and what time?").is_empty());
        assert_eq!(search_notes("Whose house is it at, and what time?", 5).len(), 1);
        // A title word, or two of the question's words.
        assert_eq!(titles("What's the next book for book club?"), ["Book club"]);
        assert_eq!(titles("What's the wifi password at the cabin?"), ["Cabin trip"]);
        assert_eq!(titles("Kitchen quotes?"), ["Kitchen quotes"]);
        // One word asked about: one word is enough.
        assert_eq!(titles("password").len(), 2);
        // Words like "what" and "and" never count.
        assert!(titles("what and the").is_empty());
        // A note that answers from its body alone, under a title that says
        // nothing about it (the review's example, 2026-10-22): two of the
        // question's words in the body are enough.
        crate::notes::create("Finance sync", "Agreed the kitchen budget at £15,000; Sam checks the quotes.", "manual", "");
        assert!(titles("What's the budget for the kitchen?").contains(&"Finance sync".to_string()));
        assert!(titles("How much did we agree to spend on the kitchen?").contains(&"Finance sync".to_string()));
        assert_eq!(titles("What's the budget?"), ["Finance sync"]);
    }

    #[test]
    fn search_scores_title_hits_higher() {
        // Pure function shape check (store may be empty in tests).
        assert!(search_notes("", 5).is_empty());
        assert!(search_notes("a", 5).is_empty()); // all words ≤ 2 chars filtered
    }

    #[test]
    fn search_then_edit_changes_that_place_only() {
        let id = plan_note();
        let found = call("search_in_note", json!({ "note_id": id, "query": "friday" }));
        assert_eq!(found["success"], json!(true));
        assert_eq!(found["data"]["matching_lines"], json!(1));
        let hit = &found["data"]["hits"][0];
        assert_eq!((hit["field"].as_str(), hit["line"].as_u64()), (Some("content"), Some(1)));
        assert_eq!(hit["text"], json!("Ship the beta on Friday."));
        assert_eq!(found["displayText"], json!("Found \"friday\" on 1 line in \"Launch plan\""));

        let before = crate::notes::get(id).unwrap().updated_ts;
        let edited = call(
            "edit_note",
            json!({
                "note_id": id,
                "field": "content",
                "old_text": hit["text"],
                "new_text": "Ship the beta on Monday.",
            }),
        );
        assert_eq!(edited["success"], json!(true), "{edited}");
        assert_eq!(edited["data"]["changed"], json!(true));
        assert_eq!(edited["data"]["line"], json!(1));
        assert_eq!(edited["data"]["now"], json!("1| Ship the beta on Monday.\n2| Ask Dave about the venue."));
        assert_eq!(
            edited["displayText"],
            json!("Edited \"Launch plan\": \"Friday.\" \u{2192} \"Monday.\"")
        );
        assert_eq!(changed_note("edit_note", &edited), Some(id));
        let note = crate::notes::get(id).unwrap();
        assert_eq!(note.content, PLAN.replace("Friday", "Monday"));
        assert!(note.updated_ts >= before);
    }

    #[test]
    fn an_edit_that_could_land_twice_changes_nothing() {
        let id = plan_note();
        let r = call(
            "edit_note",
            json!({ "note_id": id, "field": "content", "old_text": "Ship the beta", "new_text": "Ship it" }),
        );
        assert_eq!(r["success"], json!(false));
        assert!(r["error"].as_str().unwrap().contains("appears 2 times in the notes (lines 1, 3)"));
        assert_eq!(r["data"]["lines"], json!([1, 3]));
        assert_eq!(r["displayText"], json!("Couldn't edit \"Launch plan\": the text is in 2 places"));
        assert_eq!(changed_note("edit_note", &r), None);
        assert_eq!(crate::notes::get(id).unwrap().content, PLAN);
    }

    #[test]
    fn a_near_miss_quotes_the_notes_own_text() {
        let id = plan_note();
        // Double spaces and lower case: not applied, but the error has the real text.
        let r = call(
            "edit_note",
            json!({ "note_id": id, "old_text": "ask  dave about the venue.", "new_text": "x" }),
        );
        assert_eq!(r["success"], json!(false));
        assert_eq!(r["data"]["near"], json!([{ "line": 2, "text": "Ask Dave about the venue." }]));
        assert_eq!(crate::notes::get(id).unwrap().content, PLAN);
        // Copied line numbers are pointed out too.
        let r = call(
            "edit_note",
            json!({ "note_id": id, "old_text": "2| Ask Dave about the venue.", "new_text": "x" }),
        );
        assert!(r["error"].as_str().unwrap().contains("Leave out the line numbers"));
        // The retry with the exact text goes through.
        let r = call(
            "edit_note",
            json!({ "note_id": id, "old_text": "Ask Dave about the venue.\n", "new_text": "" }),
        );
        assert_eq!(r["displayText"], json!("Edited \"Launch plan\": removed \"Ask Dave about the venue.\""));
        assert_eq!(
            crate::notes::get(id).unwrap().content,
            "Ship the beta on Friday.\nShip the beta docs too.\n"
        );
    }

    #[test]
    fn the_summary_and_title_are_edited_by_name_and_the_transcript_never() {
        let id = plan_note();
        // An empty summary is filled with an empty old_text, counted as fresh.
        let r = call(
            "edit_note",
            json!({ "note_id": id, "field": "summary", "old_text": "", "new_text": "- Beta ships Friday" }),
        );
        assert_eq!(r["displayText"], json!("Wrote a summary for \"Launch plan\""));
        let note = crate::notes::get(id).unwrap();
        assert_eq!(note.enhanced_content, "- Beta ships Friday");
        assert_eq!(note.enhanced_at_hash, crate::notes::content_hash(&note.content));
        let r = call(
            "edit_note",
            json!({ "note_id": id, "field": "enhanced", "old_text": "Friday", "new_text": "Monday" }),
        );
        assert_eq!(r["displayText"], json!("Edited the summary of \"Launch plan\": \"Friday\" \u{2192} \"Monday\""));

        let r = call(
            "edit_note",
            json!({ "note_id": id, "field": "title", "old_text": "Launch plan", "new_text": "Beta launch" }),
        );
        assert_eq!(r["displayText"], json!("Renamed \"Launch plan\" to \"Beta launch\""));
        assert_eq!(crate::notes::get(id).unwrap().title, "Beta launch");

        crate::notes::append_transcript(
            id,
            &[crate::notes::TranscriptSegment {
                source: "them".into(),
                text: "Friday works".into(),
                ts: 5,
                ..Default::default()
            }],
        )
        .unwrap();
        let r = call(
            "edit_note",
            json!({ "note_id": id, "field": "transcript", "old_text": "Friday", "new_text": "Monday" }),
        );
        assert_eq!(r["success"], json!(false));
        assert_eq!(r["displayText"], json!("Couldn't edit \"Beta launch\": the transcript can't be edited"));
        assert_eq!(crate::notes::get(id).unwrap().transcript[0].text, "Friday works");
        // …but it can be searched and read.
        let found = call("search_in_note", json!({ "note_id": id, "query": "works", "field": "transcript" }));
        assert_eq!(found["data"]["hits"][0]["text"], json!("[0:00] Them: Friday works"));
        let page = call("read_note", json!({ "note_id": id, "field": "transcript" }));
        assert_eq!(page["data"]["text"], json!("1| [0:00] Them: Friday works"));
    }

    #[test]
    fn the_same_text_again_changes_nothing_and_tells_no_window() {
        let id = plan_note();
        let r = call(
            "edit_note",
            json!({ "note_id": id, "old_text": "Friday", "new_text": "Friday" }),
        );
        assert_eq!(r["success"], json!(true));
        assert_eq!(r["data"]["changed"], json!(false));
        assert_eq!(changed_note("edit_note", &r), None);
        assert_eq!(r["displayText"], json!("No change to \"Launch plan\""));
    }

    #[test]
    fn long_notes_are_read_in_pages_and_bounded_in_get_note() {
        let long: String = (1..=2_000).map(|i| format!("Line {i} of the long note.\n")).collect();
        let id = crate::notes::create("Long", &long, "manual", "").id;
        let got = call("get_note", json!({ "id": id }));
        assert_eq!(got["data"]["cut"], json!(["content"]));
        let shown = got["data"]["content"].as_str().unwrap();
        assert!(crate::meetings::meeting_summary::estimate_tokens(shown) <= GET_NOTE_TOKENS);
        assert!(long.starts_with(shown));

        let first = call("read_note", json!({ "note_id": id, "field": "content" }));
        assert_eq!(first["data"]["from"], json!(1));
        assert_eq!(first["data"]["more"], json!(true));
        let next = first["data"]["next_offset"].as_u64().unwrap();
        assert!(next > 1);
        assert_eq!(first["displayText"], json!(format!("Read \"Long\" (notes, lines 1\u{2013}{} of 2000)", next - 1)));
        let second = call(
            "read_note",
            json!({ "note_id": id, "field": "content", "offset": next, "limit": 5 }),
        );
        assert_eq!(
            second["data"]["text"].as_str().unwrap().lines().next(),
            Some(format!("{next}| Line {next} of the long note.").as_str())
        );
        let past = call("read_note", json!({ "note_id": id, "field": "content", "offset": 5_000 }));
        assert_eq!(past["success"], json!(false));
        // An empty summary says so.
        let empty = call("read_note", json!({ "note_id": id, "field": "enhanced" }));
        assert_eq!(empty["data"]["empty"], json!(true));
    }

    #[test]
    fn get_note_keeps_the_notes_and_the_summary_apart() {
        let id = plan_note();
        crate::notes::set_enhanced(id, "- Beta ships Friday", "x").unwrap();
        let got = call("get_note", json!({ "id": id }));
        assert_eq!(got["data"]["content"], json!(PLAN));
        assert_eq!(got["data"]["enhanced"], json!("- Beta ships Friday"));
        assert!(got["data"].get("cut").is_none());
    }

    #[test]
    fn creating_and_updating_notes_are_changes_too() {
        let created = call("create_note", json!({ "title": "Ideas", "content": "One" }));
        let id = created["data"]["id"].as_u64().unwrap();
        assert_eq!(changed_note("create_note", &created), Some(id));
        let updated = call("update_note", json!({ "id": id, "folder": "Work", "new_folder": true }));
        assert_eq!(changed_note("update_note", &updated), Some(id));
        assert_eq!(changed_note("get_note", &call("get_note", json!({ "id": id }))), None);
        assert_eq!(changed_note("update_note", &call("update_note", json!({ "id": 999, "title": "x" }))), None);
    }

    #[test]
    fn the_chat_never_invents_a_folder() {
        crate::notes::folder_create("Work");
        // A made-up folder: refused, nothing saved, the model told which
        // folders there are and to leave it empty.
        let r = call("create_note", json!({ "title": "Tent list", "content": "Pegs", "folder": "Camping" }));
        assert_eq!(r["success"], json!(false));
        let error = r["error"].as_str().unwrap();
        assert!(error.contains("no folder called \"Camping\""), "{error}");
        assert!(error.contains("\"Personal\", \"Meetings\", \"Work\""), "{error}");
        assert!(error.contains("leave folder empty"), "{error}");
        assert_eq!(r["displayText"], json!("Couldn't create \"Tent list\": no folder called \"Camping\""));
        assert_eq!(changed_note("create_note", &r), None);
        assert!(crate::notes::all().is_empty());
        assert_eq!(crate::notes::folders(), ["Personal", "Meetings", "Work"]);

        // No folder: Personal. An existing one in another case: its spelling.
        let r = call("create_note", json!({ "title": "Tent list", "content": "Pegs" }));
        let id = r["data"]["id"].as_u64().unwrap();
        assert_eq!(r["data"]["folder"], json!("Personal"));
        let r = call("create_note", json!({ "title": "Budget", "content": "Q3", "folder": " work" }));
        assert_eq!(r["data"]["folder"], json!("Work"));
        assert_eq!(r["displayText"], json!("Created note: \"Budget\""));

        // Moving into a made-up folder changes nothing, the title included.
        let r = call("update_note", json!({ "id": id, "title": "Trip", "folder": "Camping" }));
        assert_eq!(r["success"], json!(false));
        assert_eq!(changed_note("update_note", &r), None);
        let note = crate::notes::get(id).unwrap();
        assert_eq!((note.title.as_str(), note.folder.as_str()), ("Tent list", "Personal"));
        // …into an existing one, by any case, moves it.
        call("update_note", json!({ "id": id, "folder": "WORK" }));
        assert_eq!(crate::notes::get(id).unwrap().folder, "Work");

        // The user asked for a new folder: made once, then reused.
        let r = call(
            "create_note",
            json!({ "title": "Packing", "content": "Stove", "folder": "Camping", "new_folder": true }),
        );
        assert_eq!(r["displayText"], json!("Created note: \"Packing\" in new folder \"Camping\""));
        let r = call("update_note", json!({ "id": id, "folder": "camping", "new_folder": "true" }));
        assert_eq!(r["success"], json!(true));
        assert_eq!(crate::notes::get(id).unwrap().folder, "Camping");
        assert_eq!(crate::notes::folders(), ["Personal", "Meetings", "Work", "Camping"]);
    }
}

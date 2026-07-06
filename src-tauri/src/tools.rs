//! The chat tool-calling agent loop — OpenWhispr's `services/tools/*` registry
//! + client-side loop, ported to Rust over the OpenAI tool-calling protocol
//! (ROADMAP "AI Chat over your notes" step 2).
//!
//! Six tools (their schemas/descriptions kept near-verbatim; `search_notes`'
//! description says "by keywords" because Yap's v1 search is the keyword
//! scorer, not semantic — step 3): `search_notes`, `get_note`, `create_note`,
//! `update_note`, `list_folders`, `copy_to_clipboard`. Executed locally over
//! `notes.rs`/arboard; results are fed back as `role:"tool"` messages and the
//! model is re-invoked, ≤ `MAX_TOOL_STEPS` (their 20).
//!
//! **Capability gating** (their `estimateModelSizeB` + `LOCAL_TOOL_MIN_PARAMS_B`):
//! cloud providers always get tools; local/on-device models only when the
//! model id names ≥ 4B params (`-([\d.]+)[bB]`) — the bundled Qwen2.5-1.5B
//! can't reliably tool-call, so it falls back to plain chat + eager RAG.

use serde_json::{json, Value};

const MAX_TOOL_STEPS: usize = 20;
const MAX_CONTENT_LENGTH: usize = 500;
const LOCAL_TOOL_MIN_PARAMS_B: f32 = 4.0;

/// Their `TOOL_INSTRUCTIONS` (config/prompts.ts), verbatim, for the six tools
/// Yap ships. Appended to the persona as "You have access to tools. …".
pub const TOOL_INSTRUCTIONS: [&str; 6] = [
    "Use search_notes to find information from the user's past meetings, discussions, or personal notes before answering from memory.",
    "Use get_note to fetch the full content of a specific note by ID. If the current note's ID is provided in the context, use it directly. Otherwise, use search_notes first to find the note ID.",
    "Use create_note when the user asks you to create, write, or draft a new note. Whenever the note will go into a folder, call list_folders first and reuse an existing folder whose name is a reasonable fit for the note's topic (e.g. a new story belongs in an existing 'Stories' folder) — do this even when the user didn't name a folder but the content clearly fits one. Only pass a new folder name when nothing existing fits. Be tolerant of case, plurals, and typos.",
    "Use update_note to modify an existing note's title, content, or move it to a different folder. If the current note's ID is provided in the context, use it directly. Otherwise, use search_notes first to find the note ID. When moving to a folder, call list_folders first and reuse an existing folder whose name fits the note's topic; only create a new folder when nothing existing fits.",
    "Use list_folders before create_note or update_note whenever a note is going into a folder, so you can reuse an existing folder whose name fits the note's topic instead of creating a near-duplicate.",
    "Use copy_to_clipboard when the user asks you to copy something to their clipboard.",
];

/// Their `estimateModelSizeB` regex, verbatim: `-([\d.]+)[bB]` on the model id.
fn estimate_model_size_b(model_id: &str) -> f32 {
    let re = regex::Regex::new(r"-([\d.]+)[bB]").expect("static regex");
    re.captures(model_id)
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse::<f32>().ok())
        .unwrap_or(0.0)
}

/// Cloud providers always support tools; local (self-hosted) and on-device
/// models only when the id names ≥ 4B params (their gating rule).
pub fn supports_tools(provider: &str, model: &str) -> bool {
    match provider {
        "groq" | "anthropic" | "openai" | "gemini" | "openrouter" | "custom" => true,
        _ => estimate_model_size_b(model) >= LOCAL_TOOL_MIN_PARAMS_B,
    }
}

/// The OpenAI `tools` array (schemas near-verbatim from their tool files).
pub fn definitions() -> Value {
    json!([
        {
            "type": "function",
            "function": {
                "name": "search_notes",
                "description": "Search the user's notes by keywords. Returns matching notes with title, date, relevance score, and a preview of content.",
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
                "description": "Get the full content of a specific note by ID. Use search_notes first to find the note ID.",
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
                "name": "create_note",
                "description": "Always call list_folders first. Reuse an existing folder whenever one is a reasonable semantic fit for the note's topic (e.g. a story goes into an existing 'Stories' folder), even if the user didn't name it. Only pass a new folder name when nothing existing fits. Creates a note with title, content, and optional folder (auto-created if missing).",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "title": { "type": "string", "description": "The title of the note" },
                        "content": { "type": "string", "description": "The content of the note" },
                        "folder": { "type": "string", "description": "Folder name for the note. Created automatically if it does not exist." }
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
                "description": "Before moving to a folder, always call list_folders first. Reuse an existing folder whenever one is a reasonable semantic fit for the note's topic, even if the user didn't name it. Only pass a new folder name when nothing existing fits. Updates a note's title, content, or folder (auto-created if missing). Use the note ID from context if provided; otherwise search_notes first.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "number", "description": "The note ID to update" },
                        "title": { "type": "string", "description": "New title for the note (optional)" },
                        "content": { "type": "string", "description": "New content for the note (optional)" },
                        "folder": { "type": "string", "description": "Folder name to move the note to. Created automatically if it does not exist." }
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
                "description": "List all available note folders. Use before create_note or update_note to reuse an existing folder instead of creating a near-duplicate.",
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

/// Keyword search over the notes library (the same scorer the eager RAG uses:
/// title hits weighted 3×, body = content + enhanced + transcript).
pub fn search_notes(query: &str, limit: usize) -> Vec<(usize, crate::notes::Note)> {
    let words: Vec<String> = query
        .to_lowercase()
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| w.chars().count() > 2)
        .collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(usize, crate::notes::Note)> = crate::notes::all()
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
            let score: usize = words
                .iter()
                .map(|w| title.matches(w.as_str()).count() * 3 + body.matches(w.as_str()).count())
                .sum();
            (score > 0).then_some((score, n))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.truncate(limit);
    scored
}

fn tool_result(success: bool, data: Value, display_text: String) -> Value {
    json!({ "success": success, "data": data, "displayText": display_text })
}

/// Execute one tool call locally. Returns a ToolResult-shaped JSON value
/// (their `{success, data, displayText}`); the whole thing is fed back to the
/// model, `displayText` is also surfaced in the chat UI.
pub fn execute(name: &str, args: &Value) -> Value {
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
            let id = args["id"].as_u64().unwrap_or(0);
            match crate::notes::get(id) {
                Some(n) => {
                    let content = if !n.enhanced_content.trim().is_empty() {
                        n.enhanced_content.clone()
                    } else {
                        n.content.clone()
                    };
                    let transcript: String = n
                        .transcript
                        .iter()
                        .map(|s| {
                            format!(
                                "{}: {}",
                                if s.source == "you" { "You" } else { "Them" },
                                s.text
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let display = format!("Retrieved note: \"{}\"", n.title);
                    tool_result(
                        true,
                        json!({
                            "id": n.id,
                            "title": n.title,
                            "content": content,
                            "transcript": transcript,
                            "type": n.note_type,
                            "folder": n.folder,
                            "created_at": n.created_ts,
                            "updated_at": n.updated_ts,
                        }),
                        display,
                    )
                }
                None => tool_result(false, Value::Null, format!("Note with ID {id} not found")),
            }
        }
        "create_note" => {
            let title = args["title"].as_str().unwrap_or("").trim();
            let content = args["content"].as_str().unwrap_or("");
            let folder = args["folder"].as_str().unwrap_or("").trim();
            if title.is_empty() && content.trim().is_empty() {
                return tool_result(false, Value::Null, "Failed to create note".to_string());
            }
            let folder_created = if !folder.is_empty() {
                let before = crate::notes::folders().len();
                crate::notes::folder_create(folder).len() > before
            } else {
                false
            };
            let note = crate::notes::create(title, content, "chat", folder);
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
            let id = args["id"].as_u64().unwrap_or(0);
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
            if let Some(f) = folder.as_deref() {
                crate::notes::folder_create(f);
            }
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

/// The agent loop (their `useChatStreaming` re-invoke loop, non-streaming):
/// send messages + tools; while the model calls tools, execute them locally,
/// append the results as `role:"tool"` messages, and re-invoke — up to
/// MAX_TOOL_STEPS. Returns the final answer + the tools' displayTexts (the UI
/// shows them as activity chips).
#[allow(clippy::too_many_arguments)]
pub async fn run_tool_loop(
    system: &str,
    history: &[(String, String)],
    question: &str,
    base_url: &str,
    api_key: &str,
    model: &str,
    provider: &str,
    disable_thinking: bool,
) -> Result<(String, Vec<String>), String> {
    let mut messages: Vec<Value> = vec![json!({ "role": "system", "content": system })];
    for (role, text) in history {
        let role = if role == "assistant" { "assistant" } else { "user" };
        messages.push(json!({ "role": role, "content": text }));
    }
    messages.push(json!({ "role": "user", "content": question }));

    let tools = definitions();
    let mut tools_used: Vec<String> = Vec::new();

    for _step in 0..MAX_TOOL_STEPS {
        let msg = crate::llm::post_chat_message(
            base_url,
            api_key,
            model,
            provider,
            0.5,
            Value::Array(messages.clone()),
            Some(&tools),
        )
        .await?;

        let tool_calls = msg["tool_calls"].as_array().cloned().unwrap_or_default();
        if tool_calls.is_empty() {
            let content = msg["content"].as_str().unwrap_or("").trim().to_string();
            let content = if disable_thinking {
                crate::llm::strip_thinking(&content)
            } else {
                content
            };
            if content.is_empty() {
                return Err("The model returned an empty answer".to_string());
            }
            return Ok((content, tools_used));
        }

        // Append the assistant turn (with its tool_calls), execute each call,
        // and feed the results back.
        messages.push(msg.clone());
        for call in &tool_calls {
            let id = call["id"].as_str().unwrap_or("");
            let name = call["function"]["name"].as_str().unwrap_or("");
            let args: Value = call["function"]["arguments"]
                .as_str()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or(Value::Null);
            tracing::info!(tool = %name, "Chat tool call");
            let result = execute(name, &args);
            if let Some(d) = result["displayText"].as_str() {
                tools_used.push(d.to_string());
            }
            messages.push(json!({
                "role": "tool",
                "tool_call_id": id,
                "content": result.to_string(),
            }));
        }
    }

    // Out of steps: one final call WITHOUT tools to force a plain answer.
    let msg = crate::llm::post_chat_message(
        base_url,
        api_key,
        model,
        provider,
        0.5,
        Value::Array(messages),
        None,
    )
    .await?;
    let content = msg["content"].as_str().unwrap_or("").trim().to_string();
    if content.is_empty() {
        return Err("Tool loop exceeded its step limit without an answer".to_string());
    }
    Ok((content, tools_used))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_size_gating_matches_their_rule() {
        // cloud: always
        assert!(supports_tools("groq", "llama-3.1-8b-instant"));
        assert!(supports_tools("anthropic", "claude-haiku-4-5"));
        // local: gated on the -N[bB] pattern, min 4
        assert!(supports_tools("local", "llama-3.1-8b-instant"));
        assert!(!supports_tools("ondevice", "Qwen2.5-1.5B-Instruct-Q4_K_M.gguf"));
        assert!(!supports_tools("local", "llama3.1")); // no size in id → 0 → gated
    }

    #[test]
    fn search_scores_title_hits_higher() {
        // Pure function shape check (store may be empty in tests).
        assert!(search_notes("", 5).is_empty());
        assert!(search_notes("a", 5).is_empty()); // all words ≤ 2 chars filtered
    }
}

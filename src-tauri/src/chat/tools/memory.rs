//! The memory tools (`remember`, `forget`: `memory/learn.rs` runs them)
//! and `search_chats`, offered as the turn's [`Extra`] says.

use serde_json::{json, Value};

use super::{tool_result, Extra};

/// The memory tools (`memory/learn.rs` runs them) and `search_chats`.
pub fn memory_definitions(extra: Extra) -> Value {
    let mut tools = Vec::new();
    if extra.memory {
        tools.push(json!({
            "type": "function",
            "function": {
                "name": "remember",
                "description": "Save a fact about the user to Yap's memory.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "fact": { "type": "string", "description": "A few words: \"Prefers short answers\", \"Sister\"" },
                        "section": { "type": "string", "enum": ["profile", "preferences", "people", "projects", "topics"] },
                        "name": { "type": "string", "description": "A person's, project's or topic's name" }
                    },
                    "required": ["fact", "section"],
                    "additionalProperties": false
                }
            }
        }));
        tools.push(json!({
            "type": "function",
            "function": {
                "name": "forget",
                "description": "Remove a fact, or a person, project or topic, from Yap's memory.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "what": { "type": "string", "description": "The fact or name" }
                    },
                    "required": ["what"],
                    "additionalProperties": false
                }
            }
        }));
    }
    if extra.search_chats {
        tools.push(json!({
            "type": "function",
            "function": {
                "name": "search_chats",
                "description": "Search the user's earlier chats with you by keywords and meaning.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "query": { "type": "string", "description": "Words to look for" }
                    },
                    "required": ["query"],
                    "additionalProperties": false
                }
            }
        }));
    }
    Value::Array(tools)
}

/// The memory tools' names.
pub const MEMORY_TOOL_NAMES: &[&str] = &["remember", "forget"];

/// Did the person's own message ask to remember or forget something? The
/// memory tools act only then — a note, a transcript or a web page saying
/// "remember…" can't change memory, nor can a model on its own.
pub fn asked_to_remember(question: &str) -> bool {
    static ASK: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)\b(?:remember|forget|keep in mind|make a note|note (?:that|down)|don't (?:keep|store|save)|no longer|not true any ?more|isn't true|aren't true|memory|memories)\b")
            .expect("static regex")
    });
    ASK.is_match(&question.replace('’', "'"))
}

/// `search_chats`: the earlier conversations matching the query — by its
/// words and, with search by meaning on, its meaning (`search/library.rs`
/// `chats_for`; a model evaluation compares the keyword search it replaced,
/// `chats::search`: `search::library::keyword_only`).
pub(super) async fn search_chats(args: &Value, conversation: Option<u64>) -> Value {
    let query = args["query"].as_str().unwrap_or("").trim();
    let found: Vec<(u64, String, u64, Vec<String>)> = if crate::search::library::keyword_only() {
        crate::chat::chats::search(query, conversation, 5).into_iter().map(|f| (f.id, f.title, f.updated_ts, f.lines)).collect()
    } else {
        crate::search::library::chats_for(query, conversation, 5).await.into_iter().map(|f| (f.id, f.title, f.updated, f.lines)).collect()
    };
    let results: Vec<Value> = found
        .iter()
        .map(|(id, title, updated, lines)| {
            json!({
                "id": id,
                "title": title,
                "date": crate::memory::export::day(*updated),
                "lines": lines,
            })
        })
        .collect();
    let display = match results.len() {
        0 => format!("No past chats about \"{query}\""),
        1 => format!("Found 1 past chat for \"{query}\""),
        n => format!("Found {n} past chats for \"{query}\""),
    };
    tool_result(true, json!(results), display)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::tools::tests::tool_names;
    use crate::chat::tools::{CHATS_TOOL_INSTRUCTIONS, MEMORY_TOOL_INSTRUCTIONS};

    #[test]
    fn the_memory_tools_stay_light_and_act_only_when_asked() {
        let est = crate::meetings::meeting_summary::estimate_tokens;
        let all = Extra { memory: true, search_chats: true, conversation: None };
        let (s, t) = (
            est(&format!("{} {}", MEMORY_TOOL_INSTRUCTIONS.join(" "), CHATS_TOOL_INSTRUCTIONS.join(" "))),
            est(&memory_definitions(all).to_string()),
        );
        println!("memory adds: instructions {s} + tools {t} = {} tokens", s + t);
        assert!(s + t <= 420, "{} tokens", s + t);
        // Named in their instructions, every required argument defined.
        let names = tool_names(&memory_definitions(all));
        assert_eq!(names, ["remember", "forget", "search_chats"]);
        for d in memory_definitions(all).as_array().unwrap() {
            let name = d["function"]["name"].as_str().unwrap();
            let instructions = format!("{} {}", MEMORY_TOOL_INSTRUCTIONS.join(" "), CHATS_TOOL_INSTRUCTIONS.join(" "));
            assert!(instructions.contains(name), "{name}");
            let params = &d["function"]["parameters"];
            for r in params["required"].as_array().into_iter().flatten() {
                assert!(params["properties"][r.as_str().unwrap()].is_object(), "{name}: {r}");
            }
        }
        assert!(tool_names(&memory_definitions(Extra::default())).is_empty());
        // Only the person's own message makes them act.
        assert!(asked_to_remember("Remember that my sister is Rochelle"));
        assert!(asked_to_remember("Please forget my old job"));
        assert!(asked_to_remember("I don't work at Acme any more, that's not true anymore"));
        assert!(!asked_to_remember("When's the MOT due?"));
        assert!(!asked_to_remember("Summarise my launch plan note"));
    }
}

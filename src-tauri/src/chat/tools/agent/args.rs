//! A call's arguments as the model sent them, and the fixes Yap makes
//! before the call runs: a search's query, a document's citations.

use serde_json::{json, Value};

use super::Loop;

/// A tool call's arguments: OpenAI sends a JSON string; some servers send
/// the object itself.
pub(super) fn call_arguments(call: &Value) -> Value {
    match &call["function"]["arguments"] {
        Value::String(s) if s.trim().is_empty() => json!({}),
        Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
        obj @ Value::Object(_) => obj.clone(),
        _ => Value::Null,
    }
}

impl Loop<'_> {
    /// **The fixes before a call runs** (`id`, `name`: the call's; `forced`:
    /// the step made to search): a search's query for this question and
    /// the person's country, written back into the call as it's kept; a
    /// document's citations as links to its pages, or as its passages'
    /// meetings.
    pub(super) fn fix_arguments(&mut self, id: &str, name: &str, args: &mut Value, forced: bool) {
        if name == "web_search" {
            let asked = args["query"].as_str().unwrap_or("").to_string();
            let mut q = asked.clone();
            // The forced first search: for this question, not words
            // carried over from earlier ones.
            if forced {
                let fresh = crate::web::fresh_query(&q, self.question, self.earlier, self.own);
                if fresh != q {
                    tracing::info!(words_before = q.split_whitespace().count(), words_after = fresh.split_whitespace().count(), "Chat: earlier words left out of the forced search");
                    crate::ai::eval::post_fix("fresh_query");
                    q = fresh;
                }
            }
            // A price question: the search names the person's country
            // ("… UK"), every search of the turn.
            if let Some(local) = self.web.as_deref().and_then(|t| t.local_query(&q)) {
                tracing::info!("Chat: the search names the user's country (a price question)");
                crate::ai::eval::post_fix("local_query");
                q = local;
            }
            if q != asked {
                args["query"] = json!(q);
                // The call as it ran, in the conversation too.
                if let Some(c) = self
                    .messages
                    .last_mut()
                    .and_then(|m| m["tool_calls"].as_array_mut())
                    .and_then(|calls| calls.iter_mut().find(|c| c["id"].as_str() == Some(id)))
                {
                    c["function"]["arguments"] = json!(args.to_string());
                }
            }
        }
        // A document's "[1]" citations: links to the pages themselves
        // (a note has no Sources row), before it runs, so the model's
        // picture of the document is what was written. The pages it
        // cites (by number, or a link) are marked so first: the answer's
        // Sources row shows them first.
        if crate::chat::documents::TOOL_NAMES.contains(&name) {
            if let Some(turn) = self.web.as_deref_mut() {
                let ops = args["ops"].as_array().into_iter().flatten().filter_map(|op| op["markdown"].as_str());
                for text in ["summary", "markdown", "content", "text", "body"].iter().filter_map(|f| args[*f].as_str()).chain(ops) {
                    turn.cited_in(text);
                }
            }
            if let Some(turn) = self.web.as_deref().filter(|t| !t.sources.is_empty()) {
                let sources: Vec<(usize, String, String)> = turn
                    .sources
                    .iter()
                    .map(|s| (s.n, if s.site.trim().is_empty() { s.title.clone() } else { s.site.clone() }, s.url.clone()))
                    .collect();
                for field in ["summary", "markdown", "content", "text", "body"] {
                    if let Some(text) = args[field].as_str() {
                        let linked = crate::chat::documents::link_citations(text, &sources);
                        if linked != text {
                            args[field] = json!(linked);
                        }
                    }
                }
                // edit_document's: each op's markdown.
                if let Some(ops) = args["ops"].as_array_mut() {
                    for op in ops {
                        if let Some(text) = op["markdown"].as_str() {
                            let linked = crate::chat::documents::link_citations(text, &sources);
                            if linked != text {
                                op["markdown"] = json!(linked);
                            }
                        }
                    }
                }
            }
            // Asked across the meetings (a Skill's /tickets, /prd): its
            // passages' "[2]" as their meetings' names, noted for the
            // answer's sources (`documents::name_citations`).
            if let Some(d) = self.extra.docs {
                for field in ["summary", "markdown", "content", "text", "body"] {
                    if let Some(text) = args[field].as_str() {
                        let named = d.named(text);
                        if named != text {
                            args[field] = json!(named);
                        }
                    }
                }
                if let Some(ops) = args["ops"].as_array_mut() {
                    for op in ops {
                        if let Some(text) = op["markdown"].as_str() {
                            let named = d.named(text);
                            if named != text {
                                op["markdown"] = json!(named);
                            }
                        }
                    }
                }
            }
        }
    }
}

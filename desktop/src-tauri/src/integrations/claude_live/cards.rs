//! **The cards in the thread for a plan chat** (15 Nov 2026,
//! `chat/cards.rs`): `ask_user`, `suggest_next_step` and `mark_chapter`
//! over the run endpoint, for a Chat run offered them
//! ([`super::RunSpec::cards`]). Each call runs the tool loop's own code for
//! the chat turn it belongs to ([`super::turn`]): the question's card waits
//! in the plan turn's thread (and, asked out loud, is said and on the Yap
//! bar) through the turn's stream, its answer the call's result; the box and
//! the chapter go to the stream at once, and the answer keeps them.
//!
//! What a turn may use is its own ([`super::turn::Context::cards`]: a
//! phone's question never asks, the box needs Chat in view); the session
//! lists what any of its turns could. A Code chat's own session in Bypass
//! permissions is asked nothing: its question is left to Claude.
//!
//! The log has the run's id, the tool and the outcome, never the words.

use serde_json::{json, Map, Value};

use super::agents::{ToTurn, Unattended};
use super::server::Run;
use crate::chat::cards::{self, Cards};
use crate::chat::stream::{Live, Sink};

/// Each one's title, for Claude Code's list.
fn title(name: &str) -> &'static str {
    match name {
        cards::ask::NAME => "Ask the user in Yap",
        cards::next_step::NAME => "Suggest a next step",
        _ => "Mark a chapter",
    }
}

/// **The tools as an MCP server lists them**: the tool loop's definitions,
/// the same schemas and descriptions. Pure.
pub fn definitions(offered: Cards) -> Vec<Value> {
    offered
        .definitions()
        .iter()
        .map(|d| {
            let f = &d["function"];
            let name = f["name"].as_str().unwrap_or("");
            json!({
                "name": name,
                "title": title(name),
                "description": f["description"],
                "inputSchema": f["parameters"],
                "annotations": { "title": title(name), "readOnlyHint": false, "openWorldHint": false },
            })
        })
        .collect()
}

/// The tool loop's failure shape (`tools::tool_error`).
fn failed(error: &str) -> Value {
    json!({ "success": false, "error": error, "data": Value::Null, "displayText": "" })
}

/// A call's MCP result: the tool loop's result as one text block.
fn mcp_result(result: &Value) -> Value {
    json!({ "content": [{ "type": "text", "text": result.to_string() }], "isError": result["success"] != json!(true) })
}

/// The run's question was asked out loud (its turn's part of Yap).
pub(super) fn by_voice(feature: Option<crate::privacy::net::Feature>) -> bool {
    use crate::privacy::net::Feature;
    matches!(feature, Some(Feature::Assistant | Feature::AssistantScreen | Feature::AssistantSelection))
}

/// **One call** on `run`: its MCP result. Runs on this request's own thread
/// (a question waits here for the person's answer); the turn ending (a
/// Stop, a newer question) ends a call still waiting.
pub(super) fn call(run: &Run, name: &str, args: &Map<String, Value>) -> Value {
    let Some(turn) = run.turn.call() else {
        tracing::info!(run = %run.id, tool = name, "Claude Code: a card tool called outside a chat turn; nothing done");
        return mcp_result(&failed("Nothing was done: this isn't part of a chat turn. Answer the user."));
    };
    if !turn.ctx.cards.offers(name) {
        tracing::info!(run = %run.id, tool = name, "Claude Code: a card tool not for this turn");
        let why = if name == cards::ask::NAME {
            "There's no card to ask on for this question (it came from a phone, or nobody is watching): ask the user in words in your answer instead."
        } else {
            "That isn't shown for this question: answer in words."
        };
        return mcp_result(&failed(why));
    }
    let args = Value::Object(args.clone());
    let events = turn.events.clone();
    let sink = ToTurn(turn.events.clone());
    // Bypass permissions (a Code chat's own session): nobody is asked.
    let bypass = Unattended(&sink);
    let sink_for: &dyn Sink = if turn.ctx.unattended { &bypass } else { &sink };
    let voice = by_voice(turn.ctx.feature);
    let earlier = cards::answers_in(turn.ctx.conversation);
    let ran = tauri::async_runtime::block_on(async {
        tokio::select! {
            r = cards::execute(name, &args, Some(Live { sink: sink_for, beside: None }), voice, earlier) => Some(r),
            () = events.closed() => None,
        }
    });
    let Some(result) = ran else {
        tracing::info!(run = %run.id, tool = name, "Claude Code: a card tool's turn ended before it did");
        return mcp_result(&failed("Nothing was done: the chat turn ended first. Don't try again unless the user asks."));
    };
    tracing::info!(run = %run.id, tool = name, ok = result["success"] == json!(true), "Claude Code: a card tool answered");
    mcp_result(&result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cards_are_listed_as_the_tool_loop_has_them() {
        let all = definitions(Cards { ask: true, next_step: true, chapters: true, outline: false });
        let names: Vec<&str> = all.iter().filter_map(|d| d["name"].as_str()).collect();
        assert_eq!(names, cards::NAMES);
        assert_eq!(all[0]["inputSchema"]["required"], json!(["questions"]));
        assert!(definitions(Cards::default()).is_empty());
        assert!(by_voice(Some(crate::privacy::net::Feature::Assistant)) && !by_voice(Some(crate::privacy::net::Feature::AssistantPhone)) && !by_voice(None));
        let r = mcp_result(&failed("No."));
        assert_eq!(r["isError"], true);
    }
}

//! **Documents beside the chat for a plan chat** (16 Nov 2026,
//! `chat/documents.rs`): `create_document`, `write_section`,
//! `read_document`, `edit_document`, `document_history` and
//! `restore_document` over the run endpoint, for a plain plan chat's run
//! ([`super::RunSpec::documents`]). Until now a plan chat made no documents
//! (its turns never run Yap's tool loop), so the outline card never showed
//! where most people use Chat. Each call runs the tool loop's own code and
//! rules for the chat turn it belongs to ([`super::turn`]):
//!
//! - **The rich set** (`documents::rich`): every plan model is a Claude,
//!   which gets it on an API key too (`rich::handles_rich`).
//! - **A new document's outline first** (`chat/cards/outline.rs`): with
//!   "Show the outline before writing" on, wherever a question card could ask
//!   (`turn::Context::cards`' `outline`: never a phone's question) and
//!   someone watches the turn, `create_document`'s outline is a card in the
//!   plan turn's thread (asked out loud: said, and on the Yap bar) and **the
//!   HTTP call is held** for Go or Edit, as a question card's is; its result
//!   says what the person made of it (`outline::go_result`). The turn ending
//!   (■ Stop, a newer question) ends a call still waiting: nothing made.
//! - **The person's words win**: the model's picture of each document
//!   follows every call (`documents::after_call`), so `edit_document`
//!   refuses a block the person changed since its last look, and the next
//!   question hears what they changed (`documents::context`); the panel's
//!   saves merge with what Chat wrote meanwhile (`merge3`,
//!   `chat_document_save`). Every change is a version marked Chat (Undo).
//! - **Only this chat's documents** change (`documents::Turn::refuses`);
//!   the same title twice in a turn is the one made already
//!   (`Turn::made_already`).
//! - **The safety rule** (`tools::refused_after_outside`): after outside
//!   content in the turn — a `WebSearch` or `WebFetch`, the screen or a page
//!   through `eyes.rs`, an agent's words, text selected in another app, a
//!   connector's result — a tool the rule locks for this model is refused;
//!   a strong model (every plan model) keeps writing its documents, as in
//!   the tool loop. A locked call first waits for its own tool call to show
//!   in the turn's events (`Slot::seen`), so a web read before it is
//!   counted; not seen in time, it counts as after outside content.
//! - **What the page sees**: a document made or changed opens the panel on
//!   it (`Event::Document`, through the turn's stream; a section as it's
//!   written comes from Claude Code's own stream, `claude_plan/map.rs` →
//!   `Event::Section`), every window hears of the note (`yap-note-changed`),
//!   and the answer keeps it (the turn's `documents::Turn`, its pill).
//! - A model evaluation records the call (`eval::intercept_tool`).
//!
//! The log has the run's id, the tool and the outcome, never a document's
//! words.

use std::time::Duration;

use serde_json::{json, Map, Value};

use super::agents::ToTurn;
use super::server::{Emit, Run};
use super::turn::{Call, Slot, SEEN_WAIT};
use crate::chat::documents::{self, ToolSet};
use crate::chat::stream::{Event, Live};
use crate::chat::tools::{refused_after_outside, Seen, ACTION_TOOLS, STRONG_LOCKED};

/// The tools, as the tool loop names them (the rich set's, in its order).
pub const NAMES: &[&str] = &["create_document", "write_section", "read_document", "edit_document", "document_history", "restore_document"];

/// Each one's title, for Claude Code's list.
fn title(name: &str) -> &'static str {
    match name {
        "create_document" => "Start a document beside the chat",
        "write_section" => "Write a document's section",
        "read_document" => "Read a document",
        "edit_document" => "Change a document",
        "document_history" => "A document's versions",
        _ => "Bring back a document's version",
    }
}

/// **The tools as an MCP server lists them**: the tool loop's rich set
/// (`documents::definitions`), the same schemas and descriptions. Pure.
pub fn definitions() -> Vec<Value> {
    documents::definitions(ToolSet::Rich)
        .iter()
        .map(|d| {
            let f = &d["function"];
            let name = f["name"].as_str().unwrap_or("");
            let reads = matches!(name, "read_document" | "document_history");
            json!({
                "name": name,
                "title": title(name),
                "description": f["description"],
                "inputSchema": f["parameters"],
                "annotations": { "title": title(name), "readOnlyHint": reads, "destructiveHint": false, "openWorldHint": false },
            })
        })
        .collect()
}

/// The tool loop's failure shape (`tools::tool_error`): the reason for the
/// model and the chip for the person.
fn failed(error: &str, chip: &str) -> Value {
    json!({ "success": false, "error": error, "data": Value::Null, "displayText": chip })
}

/// A call's MCP result: the tool loop's result as one text block (its
/// `displayText` the chip, `claude_plan/map.rs`).
fn mcp_result(result: &Value) -> Value {
    json!({ "content": [{ "type": "text", "text": result.to_string() }], "isError": result["success"] != json!(true) })
}

/// **One call** on `run`: its MCP result. Runs on this request's own thread
/// (an outline waits here for the person's Go); every window hears of a
/// note it changed (`emit`).
pub(super) fn call(emit: &Emit, run: &Run, name: &str, args: &Map<String, Value>) -> Value {
    let Some(turn) = run.turn.call() else {
        tracing::info!(run = %run.id, tool = name, "Claude Code: a document tool called outside a chat turn; nothing done");
        return mcp_result(&failed("Nothing was done: this isn't part of a chat turn. Answer the user.", "Didn't change a document"));
    };
    let result = run_for(&run.turn, &run.id, &turn, name, Value::Object(args.clone()), SEEN_WAIT);
    // Every window follows the note (the Notes view, a doc window), as the
    // tool loop's changes are announced (`turn/run.rs`).
    if let Some(id) = crate::chat::tools::notes::changed_note(name, &result) {
        if let Some(n) = crate::notes::get(id) {
            emit("yap-note-changed", crate::notes::commands::note_changed_payload(&n, "chat"));
            emit("yap-notes-changed", Value::Null);
        }
    }
    mcp_result(&result)
}

/// The fields of a document call that hold its words (`edit_document`'s
/// ops' `markdown` besides).
const TEXT_FIELDS: [&str; 5] = ["summary", "markdown", "content", "text", "body"];

/// **Asked across the meetings** (a Skill's /tickets, /prd): a document's
/// "[2]" as its passage's meeting, noted for the answer's sources
/// (`documents::Turn::named`), as the tool loop's `fix_arguments`.
fn named(docs: &documents::Turn, args: &mut Value) {
    for field in TEXT_FIELDS {
        if let Some(text) = args[field].as_str() {
            let named = docs.named(text);
            if named != text {
                args[field] = json!(named);
            }
        }
    }
    if let Some(ops) = args["ops"].as_array_mut() {
        for op in ops {
            if let Some(text) = op["markdown"].as_str() {
                let named = docs.named(text);
                if named != text {
                    op["markdown"] = json!(named);
                }
            }
        }
    }
}

/// [`call`] for `turn` of the run `id` (its `slot`): the tool loop's guards,
/// the outline first, the tool, and what the page and the answer keep of it.
/// A call the safety rule could refuse waits at most `wait` for its own tool
/// call in the turn's events.
fn run_for(slot: &Slot, id: &str, turn: &Call, name: &str, mut args: Value, wait: Duration) -> Value {
    let Some(docs) = turn.ctx.docs.clone() else {
        tracing::info!(run = %id, tool = name, "Claude Code: a document tool in a turn without documents; refused");
        return failed("There's no document beside the chat for this question: answer in words.", "Didn't change a document: not for this question");
    };
    // The safety rule, as the tool loop's gate has it for this model: after
    // outside content, the tools it locks are refused — counted once the
    // call's own tool call showed in the turn's events; not seen in time, on
    // doubt, as after it.
    let strong = turn.ctx.strong;
    let locked = if strong { STRONG_LOCKED } else { ACTION_TOOLS };
    if locked.contains(&name) {
        let counted = slot.seen(turn.n, name, wait);
        let seen = Seen { web: !counted || slot.web(turn.n), screen: false, connector: slot.connector(turn.n) };
        if let Some(refusal) = refused_after_outside(name, seen, strong) {
            tracing::info!(run = %id, tool = name, counted, "Claude Code: a document tool after outside content; refused");
            return refusal;
        }
    }
    // A model evaluation records what would leave Yap.
    if let Some(recorded) = crate::ai::eval::intercept_tool(name, &args) {
        return recorded;
    }
    // This turn's document asked for again: the one there is. A document
    // tool on a note that isn't this chat's: refused.
    if let Some(r) = docs.made_already(name, &args).or_else(|| docs.refuses(name, &args)) {
        return r;
    }
    named(&docs, &mut args);
    // **A new document's outline, seen first** (chat/cards/outline.rs): its
    // card waits in the plan turn's thread for Go or Edit before anything is
    // made; nobody watching (an evaluation), made at once.
    let result = if name == "create_document" && turn.ctx.cards.outline && turn.ctx.watched {
        let sink = ToTurn(turn.events.clone());
        let voice = super::cards::by_voice(turn.ctx.feature);
        let reviews = docs.reviewed();
        let events = turn.events.clone();
        let ran = tauri::async_runtime::block_on(async {
            tokio::select! {
                r = crate::chat::cards::outline::create(Some(Live { sink: &sink, beside: None }), &args, voice, reviews, |a| crate::chat::tools::notes::execute(name, a)) => Some(r),
                () = events.closed() => None,
            }
        });
        match ran {
            Some(r) => r,
            None => {
                tracing::info!(run = %id, tool = name, "Claude Code: a document's outline turn ended before it was answered");
                return failed("Nothing was made: the chat turn ended first. Don't try again unless the user asks.", "Didn't start the document: the answer stopped");
            }
        }
    } else {
        crate::chat::tools::notes::execute(name, &args)
    };
    // What the model has now seen of the document: the next question hears
    // what the person changed since, and a block they changed is refused.
    documents::after_call(name, &args, &result);
    // A document of this chat made or changed: the panel opens on it at
    // once, and the answer keeps it (its pill).
    if let Some(note) = crate::chat::tools::notes::changed_note(name, &result) {
        if let Some(doc) = docs.noticed(name, note) {
            let _ = turn.events.send(Event::Document { note_id: doc.id, title: doc.title });
        }
    }
    tracing::info!(run = %id, tool = name, ok = result["success"] == json!(true), "Claude Code: a document tool answered");
    result
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::super::turn::{test_begin, Context};
    use super::*;
    use crate::chat::cards::Cards;
    use crate::chat::pages::act::confirm::{self, Reply};

    fn outline() -> Value {
        json!({ "title": "Composting at home", "summary": "A one-page guide.", "sections": [
            { "key": "Introduction", "intent": "why compost" },
            { "key": "What goes in", "intent": "greens and browns" },
            { "key": "Troubleshooting", "intent": "smells and pests" },
        ] })
    }

    /// A turn of a plan chat's run with documents (`outline`: the card first;
    /// `known`: the conversation's documents), and its call.
    fn turn_on(slot: &Arc<Slot>, outline: bool, known: &[documents::DocRef]) -> (super::super::turn::Turn, Call, Arc<documents::Turn>) {
        let docs = Arc::new(documents::Turn::new(known));
        let turn = test_begin(
            slot,
            Context {
                question: "Write me a one-page guide to composting at home as a document".into(),
                conversation: Some(31),
                watched: true,
                cards: Cards { ask: true, outline, ..Default::default() },
                docs: Some(Arc::clone(&docs)),
                strong: true,
                ..Default::default()
            },
        );
        let call = slot.call().unwrap();
        (turn, call, docs)
    }

    /// What the turn's stream got so far.
    fn shown(turn: &mut super::super::turn::Turn) -> Vec<Event> {
        let mut out = Vec::new();
        while let Ok(ev) = turn.events.try_recv() {
            out.push(ev);
        }
        out
    }

    #[test]
    fn the_rich_set_is_listed_as_the_tool_loop_has_it() {
        let all = definitions();
        let names: Vec<&str> = all.iter().filter_map(|d| d["name"].as_str()).collect();
        assert_eq!(names, NAMES);
        let tool_loop: Vec<Value> = documents::definitions(ToolSet::Rich).iter().map(|d| d["function"]["name"].clone()).collect();
        assert_eq!(tool_loop, NAMES.iter().map(|n| json!(n)).collect::<Vec<_>>(), "the tool loop's own set, in its order");
        assert!(NAMES.iter().all(|n| documents::TOOL_NAMES.contains(n)));
        for d in &all {
            let reads = d["name"] == "read_document" || d["name"] == "document_history";
            assert_eq!(d["annotations"]["readOnlyHint"], reads, "{}", d["name"]);
            assert_eq!(d["annotations"]["destructiveHint"], false);
            assert!(d["inputSchema"]["properties"].is_object());
        }
        assert_eq!(all[0]["inputSchema"]["required"], json!(["title", "sections"]));
    }

    #[test]
    fn a_new_documents_outline_waits_for_go_then_it_opens_beside_the_chat_and_the_answer_keeps_it() {
        let slot = Arc::new(Slot::default());
        let (mut turn, call, docs) = turn_on(&slot, true, &[]);
        confirm::reply_in_test(Some(Reply::Once));
        let made = run_for(&slot, "r1", &call, "create_document", outline(), Duration::ZERO);
        confirm::reply_in_test(None);
        assert_eq!(made["success"], true, "{made}");
        assert_eq!(made["displayText"], "Started \"Composting at home\" (3 sections)");
        assert!(made["data"]["outline"].as_str().unwrap().starts_with("The user read the outline and said go"), "{made}");
        let id = made["data"]["id"].as_u64().unwrap();
        // The panel opens on it, and the answer keeps it.
        let events = shown(&mut turn);
        assert!(events.iter().any(|e| matches!(e, Event::Document { note_id, title } if *note_id == id && title == "Composting at home")), "{events:?}");
        assert_eq!(docs.touched(), [documents::DocRef { id, title: "Composting at home".into() }]);
        // Its sections, one by one, in their placeholders.
        let wrote = run_for(&slot, "r1", &call, "write_section", json!({ "note_id": id, "key": "Introduction", "markdown": "Composting turns kitchen scraps into soil." }), Duration::ZERO);
        assert_eq!(wrote["success"], true, "{wrote}");
        let note = crate::notes::get(id).unwrap();
        assert!(note.content.contains("## Introduction\n\nComposting turns kitchen scraps into soil."), "{}", note.content);
        assert_eq!(documents::pending_keys(&note.content), ["What goes in", "Troubleshooting"]);
        // The same title again this turn: the one there is, no second card.
        let again = run_for(&slot, "r1", &call, "create_document", outline(), Duration::ZERO);
        assert_eq!((again["data"]["id"].as_u64(), again["data"]["already"].as_bool()), (Some(id), Some(true)));
        drop(turn);
    }

    #[test]
    fn edit_changes_the_outline_and_a_no_makes_nothing() {
        let slot = Arc::new(Slot::default());
        let (turn, call, docs) = turn_on(&slot, true, &[]);
        let edit = json!({ "sections": [{ "key": "What goes in", "intent": "greens and browns" }, { "key": "Introduction", "intent": "why compost" }], "note": "Keep it to one page" });
        confirm::reply_in_test(Some(Reply::Picked(edit)));
        let made = run_for(&slot, "r1", &call, "create_document", outline(), Duration::ZERO);
        confirm::reply_in_test(None);
        assert_eq!(made["data"]["sections"], json!(["What goes in", "Introduction"]), "{made}");
        let told = made["data"]["outline"].as_str().unwrap();
        assert!(told.contains("The user changed the outline") && told.contains("“Keep it to one page”"), "{told}");
        assert!(!crate::notes::get(made["data"]["id"].as_u64().unwrap()).unwrap().content.contains("Troubleshooting"));
        drop(turn);
        // "Don't write it": nothing made, the model told to ask instead.
        let (turn, call, _) = turn_on(&slot, true, &[]);
        confirm::reply_in_test(Some(Reply::Deny));
        let stopped = run_for(&slot, "r1", &call, "create_document", json!({ "title": "Another guide", "sections": ["One"] }), Duration::ZERO);
        confirm::reply_in_test(None);
        assert_eq!(stopped["success"], false);
        assert_eq!(stopped["displayText"], "Didn't start the document: you said not to");
        assert_eq!(docs.touched().len(), 1, "only the first turn's");
        drop(turn);
    }

    #[test]
    fn with_the_outline_off_or_nobody_watching_it_is_made_at_once() {
        let slot = Arc::new(Slot::default());
        // (A reply that would say no, if a card were shown.)
        confirm::reply_in_test(Some(Reply::Deny));
        let (turn, call, docs) = turn_on(&slot, false, &[]);
        let made = run_for(&slot, "r1", &call, "create_document", outline(), Duration::ZERO);
        assert_eq!(made["success"], true, "{made}");
        assert!(made["data"]["outline"].is_null(), "no card, nothing said of one");
        assert_eq!(docs.touched().len(), 1);
        drop(turn);
        let unwatched = Arc::new(documents::Turn::new(&[]));
        let turn = test_begin(&slot, Context { cards: Cards { ask: true, outline: true, ..Default::default() }, docs: Some(Arc::clone(&unwatched)), strong: true, ..Default::default() });
        let made = run_for(&slot, "r1", &slot.call().unwrap(), "create_document", json!({ "title": "Unwatched", "sections": ["One"] }), Duration::ZERO);
        confirm::reply_in_test(None);
        assert_eq!(made["success"], true, "an evaluation: made at once");
        drop(turn);
    }

    #[test]
    fn an_edit_keeps_the_persons_words_and_changes_only_this_chats_documents() {
        let slot = Arc::new(Slot::default());
        let (turn, call, _) = turn_on(&slot, false, &[]);
        let made = run_for(&slot, "r1", &call, "create_document", outline(), Duration::ZERO);
        let id = made["data"]["id"].as_u64().unwrap();
        for (key, md) in [("Introduction", "Composting turns kitchen scraps and leaves into rich soil over a few months."), ("What goes in", "- Greens\n- Browns"), ("Troubleshooting", "- Smells: add browns")] {
            assert_eq!(run_for(&slot, "r1", &call, "write_section", json!({ "note_id": id, "key": key, "markdown": md }), Duration::ZERO)["success"], true);
        }
        drop(turn);
        // The next turn: the chat's document known. The person typed into it
        // meanwhile (the panel's save), so the model's last look is stale.
        let known = [documents::DocRef { id, title: "Composting at home".into() }];
        let (turn, call, docs) = turn_on(&slot, false, &known);
        let before = crate::notes::get(id).unwrap().content;
        let typed = before.replace("- Greens", "- Greens: vegetable peelings from Sam's allotment");
        crate::notes::modify(id, |n| {
            n.content = typed.clone();
            Ok(((), true))
        })
        .unwrap();
        // The model's last look was before the person typed (its own
        // writing): its edit of their block is refused, their words stay.
        let stale = documents::saw(id).unwrap();
        assert_ne!(stale, typed, "the model hasn't seen the person's words");
        let doc = documents::blocks::Doc::of(&stale);
        let block = |starts: &str| {
            let i = doc.blocks.iter().position(|b| b.text.starts_with(starts)).unwrap_or_else(|| panic!("{starts} in {stale}"));
            (doc.ids[i].id.clone(), doc.ids[i].hash.clone())
        };
        let (list, list_hash) = block("- Greens");
        let edit = json!({ "note_id": id, "ops": [{ "op": "replace", "id": list, "if_hash": list_hash, "markdown": "- Greens and browns" }] });
        let refused = run_for(&slot, "r1", &call, "edit_document", edit, Duration::ZERO);
        assert_eq!(refused["success"], false, "{refused}");
        assert!(crate::notes::get(id).unwrap().content.contains("Sam's allotment"), "the person's words stay");
        // A block they didn't touch changes.
        let (intro, intro_hash) = block("Composting turns");
        let edit = json!({ "note_id": id, "ops": [{ "op": "replace", "id": intro, "if_hash": intro_hash, "markdown": "Composting turns scraps into soil." }] });
        let changed = run_for(&slot, "r1", &call, "edit_document", edit, Duration::ZERO);
        assert_eq!(changed["success"], true, "{changed}");
        let now = crate::notes::get(id).unwrap().content;
        assert!(now.contains("Composting turns scraps into soil.") && now.contains("Sam's allotment"), "{now}");
        assert_eq!(docs.touched().len(), 1);
        // Another note isn't this chat's document: refused.
        let other = crate::notes::create("Shopping", "milk", "manual", "");
        let r = run_for(&slot, "r1", &call, "edit_document", json!({ "note_id": other.id, "ops": [] }), Duration::ZERO);
        assert!(r["error"].as_str().unwrap().contains("isn't a document of this chat"), "{r}");
        drop(turn);
    }

    #[test]
    fn a_turn_without_documents_refuses_and_the_safety_rule_holds_for_a_model_that_isnt_strong() {
        let slot = Arc::new(Slot::default());
        // A research turn: its report is Yap's.
        let turn = test_begin(&slot, Context { conversation: Some(31), ..Default::default() });
        let r = run_for(&slot, "r1", &slot.call().unwrap(), "create_document", outline(), Duration::ZERO);
        assert_eq!(r["displayText"], "Didn't change a document: not for this question");
        drop(turn);
        // After the web: a strong model writes on; one that isn't is refused a restore.
        let (turn, call, _) = turn_on(&slot, false, &[]);
        turn.marks().web();
        let made = run_for(&slot, "r1", &call, "create_document", outline(), Duration::ZERO);
        assert_eq!(made["success"], true, "a strong model's documents go on after the web");
        drop(turn);
        let docs = Arc::new(documents::Turn::new(&[]));
        let turn = test_begin(&slot, Context { docs: Some(docs), strong: false, ..Default::default() });
        turn.marks().web();
        turn.marks().began("restore_document");
        let r = run_for(&slot, "r1", &slot.call().unwrap(), "restore_document", json!({ "note_id": 1, "version": 1 }), Duration::ZERO);
        assert_eq!(r["displayText"], "Didn't restore a document: not after reading the web");
        // Not seen in the turn's events in time: on doubt, as after the web.
        drop(turn);
        let turn = test_begin(&slot, Context { docs: Some(Arc::new(documents::Turn::new(&[]))), strong: false, ..Default::default() });
        let r = run_for(&slot, "r1", &slot.call().unwrap(), "restore_document", json!({ "note_id": 1, "version": 1 }), Duration::from_millis(5));
        assert_eq!(r["success"], false);
        assert!(r["error"].as_str().unwrap().contains("not allowed for the rest of this turn"), "{r}");
        drop(turn);
    }

    #[test]
    fn a_turn_ended_while_the_outline_waits_makes_nothing() {
        let slot = Arc::new(Slot::default());
        let (turn, call, docs) = turn_on(&slot, true, &[]);
        // (No test reply: the card waits for the person, until the turn ends.)
        let waiting = {
            let slot = Arc::clone(&slot);
            std::thread::spawn(move || run_for(&slot, "r1", &call, "create_document", outline(), Duration::ZERO))
        };
        std::thread::sleep(Duration::from_millis(100));
        drop(turn);
        let r = waiting.join().unwrap();
        assert_eq!(r["displayText"], "Didn't start the document: the answer stopped", "{r}");
        assert!(docs.touched().is_empty());
    }
}

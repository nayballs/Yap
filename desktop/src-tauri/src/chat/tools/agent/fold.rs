//! Folding results back: a call's result into the turn (its chip, the
//! page, the note it changed, the next message), and what follows a
//! step's calls (the pictures it looked at, Yap's own searches).

use serde_json::{json, Value};

use super::{EmptyNotes, Loop, NOTHING_IN_NOTES_HINT};
use crate::chat::tools::notes::changed_note;

impl Loop<'_> {
    /// **A call's result folded back** (`id`, `name`: the call's;
    /// `ran_with`: its arguments as it ran; `looks`: the step's pictures
    /// still to go): a notes search that found nothing pointed at the web,
    /// the call logged, a note it changed announced (a document of this
    /// chat shown at once), its chip, and the result as the next tool
    /// message, within what the request has left. Whether it was a notes
    /// search that found nothing.
    pub(super) fn fold(&mut self, id: &str, name: &str, ran_with: &Value, mut result: Value, looks: &[crate::chat::pictures::Look]) -> bool {
        let empty = name == "search_notes" && result["data"].as_array().is_some_and(Vec::is_empty);
        let looked = self.web.as_deref().is_some_and(crate::web::Turn::looked);
        if empty && self.empty_notes != EmptyNotes::Nothing && !looked {
            result["hint"] = json!(NOTHING_IN_NOTES_HINT);
        }
        tracing::info!(tool = %name, success = %result["success"], "Chat tool call");
        crate::ai::eval::tool_called(name, ran_with, &result);
        // What the model has now seen of a document (documents.rs): the
        // next question hears what the person changed since.
        crate::chat::documents::after_call(name, ran_with, &result);
        if let Some(id) = changed_note(name, &result) {
            (self.on_change)(id);
            // A document of this chat made or changed: the panel shows
            // it at once, and the answer keeps it (its pill).
            if let Some(doc) = self.extra.docs.and_then(|d| d.noticed(name, id)) {
                if let Some(live) = self.live {
                    live.sink.send(crate::chat::stream::Event::Document { note_id: doc.id, title: doc.title });
                }
                // A strong model writing a document researches deeper
                // from now on (a question Yap didn't see as one: the
                // turn already looks further when it did, `turn.rs`).
                if let (true, Some(turn)) = (self.extra.strong, self.web.as_deref_mut()) {
                    turn.go_deep();
                }
            }
        }
        // (A call with nothing to say — this turn's document asked for
        // again — has no chip.)
        if let Some(d) = result["displayText"].as_str().filter(|d| !d.is_empty()) {
            // A document's sections, one chip ("Wrote 4 sections in …").
            crate::chat::documents::push_chip(&mut self.tools_used, d.to_string());
        }
        self.report();
        // Within what the request has left (a long page read on an 8K
        // model, several notes): never past the model's context.
        let content = self.room.fit(&result, &self.messages, looks);
        self.messages.push(json!({
            "role": "tool",
            "tool_call_id": id,
            "content": content,
        }));
        empty
    }

    /// **What follows a step's calls** (`looks`: the pictures it looked at;
    /// `notes_found_nothing`: it only searched the notes, and found
    /// nothing): the pictures after every result, the web the turn's
    /// again once a new document's outline is made, and Yap's own search
    /// for a model on this PC — the local price, or what the notes didn't
    /// have.
    pub(super) async fn after_calls(&mut self, looks: &[crate::chat::pictures::Look], notes_found_nothing: bool) {
        // The pictures it looked at, after every tool result (each provider
        // wants those first): the next step sees them.
        if let Some(m) = crate::chat::pictures::look_message(looks, self.provider) {
            tracing::info!(pictures = looks.len(), "Chat: the next step carries the pictures the model looked at");
            self.messages.push(m);
        }
        // A new document's first step is done (its outline made, or the
        // refusal told): the web is the turn's from the next step.
        if let Some(turn) = self.web.as_deref_mut() {
            turn.outline_settled();
        }
        // A price question whose results only had another country's prices:
        // a model on this PC gets Yap's search for the local price straight
        // away ("… UK £"; a cloud model is told to search again).
        if let Some(q) = self.web.as_deref_mut().and_then(crate::web::Turn::take_local_retry) {
            tracing::info!(provider = self.provider, "Chat: no local prices in the results; Yap searches again for them");
            self.search_for(&q).await;
        }
        // Nothing in the notes for a question the web may answer: a model on
        // this PC gets Yap's search for it straight away (the question as
        // the query), as when it must search first.
        if notes_found_nothing && self.empty_notes == EmptyNotes::HintAndSearch && !self.led_to_web && self.web.as_deref().is_some_and(|t| !t.looked()) {
            self.led_to_web = true;
            tracing::info!(provider = self.provider, "Chat: nothing in the notes; Yap searches the web for the model");
            self.search_for(&crate::web::search_query(self.question)).await;
        }
    }
}

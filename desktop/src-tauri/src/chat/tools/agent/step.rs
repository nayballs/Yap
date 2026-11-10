//! One step of the loop: a model call and its special cases — the search
//! that must come first, a call the provider rejected, Claude's own web
//! tools, a new document's outline and a half-written one — and the last
//! call, without tools.

use serde_json::{json, Value};

use super::{ask, must_search_choice, rejected_call_note, Loop, ANSWER_NOW_NOTE, MUST_SEARCH_TOKENS, REJECTED_CALL_RETRIES};

/// What one model call of the loop came to ([`Loop::step`]).
pub(super) enum Step {
    /// Ask again: Yap searched for the model, told it its call was
    /// rejected, or asked it to outline or finish a document.
    Again,
    /// The answer, in words.
    Answer(String),
    /// Tool calls to run (`calls`), and the message that made them;
    /// `forced`: the step made to search.
    Calls { msg: Value, calls: Vec<Value>, forced: bool },
    /// Rejected again: the last call answers without tools.
    GiveUp,
}

/// What a step on Anthropic's Messages API carries besides the request:
/// Claude's own web tools for the turn (`web::Turn::server_tools`), and its
/// tool search when the connectors' tools are deferred behind it
/// (`tools/connectors.rs`, past their budget).
fn native(web: Option<&crate::web::Turn>, connectors: Option<&crate::chat::tools::connectors::Turn>) -> crate::ai::llm::anthropic::Native {
    let mut server_tools = web.and_then(crate::web::Turn::server_tools).unwrap_or_default();
    server_tools.extend(connectors.and_then(crate::chat::tools::connectors::Turn::server_tool));
    crate::ai::llm::anthropic::Native { server_tools }
}

impl Loop<'_> {
    /// **Claude's own tools for this step** (a step on the Messages API:
    /// [`native`]; else `None`) — and **its earlier thinking left out when
    /// they aren't the ones the step before had** (10 Nov 2026). Each of
    /// Claude's thinking blocks is tied to the tools it thought with, and
    /// the API refuses one sent back beside other tools
    /// (`llm::anthropic::thinking_unbound`: Claude Fable 5.2 on every
    /// account, the other Claude 5 models on newer ones). They change twice
    /// in a turn at most: none while a new document's outline is to come,
    /// then more searches and page reads once the turn writes one
    /// (`web::Turn::server_tools`). Everything Claude did stays in the
    /// messages (its calls, its searches, its words); only the reasoning
    /// before the change isn't sent back, and the steps after keep theirs.
    fn claude_step(&mut self) -> Option<crate::ai::llm::anthropic::Native> {
        if !self.native_chat {
            return None;
        }
        let native = native(self.web.as_deref(), self.extra.connectors);
        if self.claude_tools.as_ref().is_some_and(|before| *before != native.server_tools) {
            let gone = crate::ai::llm::anthropic::forget_thinking(&mut self.messages);
            tracing::info!(blocks = gone, "Chat: Claude's own tools changed this turn; its earlier thinking is left out from here");
        }
        self.claude_tools = Some(native.server_tools.clone());
        Some(native)
    }

    /// **One model call of the loop**, over the messages so far and the
    /// tools every step offers: what it came to ([`Step`]). The step made
    /// to search (`must_search`) offers the same tools with
    /// `tool_choice: "required"`, its words never shown and cut short; a
    /// server that won't be made to search gets Yap's search instead.
    pub(super) async fn step(&mut self) -> Result<Step, String> {
        let forced = self.must_search;
        let native = self.claude_step();
        let mut req = crate::ai::llm::ChatRequest::chat(
            self.base_url,
            self.api_key,
            self.model,
            self.provider,
            0.5,
            Value::Array(self.messages.clone()),
            Some(self.tools),
            self.must_search.then(must_search_choice),
        );
        // The step made to search is a call, never the answer: its words
        // aren't shown or said, and it's cut short if it writes on
        // ([`MUST_SEARCH_TOKENS`]).
        let chips_only = self.live.map(|l| crate::chat::stream::ChipsOnly(l.sink));
        let step_live = match (self.live, &chips_only) {
            (Some(l), Some(quiet)) if forced => Some(crate::chat::stream::Live { sink: quiet, beside: l.beside }),
            _ => self.live,
        };
        if forced {
            req.max_tokens = Some(MUST_SEARCH_TOKENS);
        }
        req.native = native;
        let answer = ask(step_live, &req, true).await;
        // A server that won't be made to search (it refuses `tool_choice`,
        // answers in words, or calls another tool): Yap searches for the
        // model, with the question as the query, and the model goes on.
        // (What it wrote instead was never shown.)
        if self.must_search {
            self.must_search = false;
            let searched = answer.as_ref().is_ok_and(|m| {
                m["tool_calls"].as_array().is_some_and(|c| c.iter().any(|call| call["function"]["name"] == "web_search"))
            });
            if !searched {
                tracing::info!(provider = self.provider, "Chat: the model wasn't made to search; Yap searches for it");
                self.search_for(&crate::web::search_query(self.question)).await;
                return Ok(Step::Again);
            }
        }
        let mut msg = match answer {
            Ok(msg) => msg,
            Err(e) => return self.refused(e),
        };
        // The step was asked again without Claude's thinking (a refusal
        // nobody foresaw: `llm::anthropic::post`): the loop forgets it too,
        // so the next step isn't refused the same way.
        if msg.as_object_mut().and_then(|o| o.remove(crate::ai::llm::anthropic::THINKING_DROPPED)).is_some() {
            crate::ai::llm::anthropic::forget_thinking(&mut self.messages);
        }
        self.took_server_web(&mut msg);
        let calls = msg["tool_calls"].as_array().cloned().unwrap_or_default();
        if calls.is_empty() {
            let content = msg["content"].as_str().unwrap_or("").trim().to_string();
            let content = crate::chat::think::answer_without_inline(&content, self.disable_thinking);
            if content.is_empty() {
                return Err("The model returned an empty answer".to_string());
            }
            return Ok(self.answered(content));
        }
        Ok(Step::Calls { msg, calls, forced })
    }

    /// **A step the provider refused** (`e`): the model's own tool call
    /// (a missing argument, a tool it doesn't have, a call written as
    /// text) told once and asked again, then answered without tools —
    /// never an error for the person. Any other error is the turn's.
    fn refused(&mut self, e: String) -> Result<Step, String> {
        // A model Yap didn't know couldn't read images (switched to
        // after a question with a screenshot) turned the earlier
        // ones down: a "no" from now on, as for a question's own
        // (`snip::vision::refused`), and asked again with each said
        // in a line instead. A question with screenshots of its own
        // is chat::commands' to say.
        if crate::snip::vision::is_image_refusal(&e) && crate::snip::turn::images().is_empty() && crate::snip::turn::without_images(&mut self.messages) {
            tracing::info!(provider = self.provider, "Chat: the model refused the earlier screenshots; asking again without them");
            crate::snip::vision::refused(self.provider, self.model);
            if let Some(live) = self.live {
                live.sink.send(crate::chat::stream::Event::Text { text: String::new() });
            }
            return Ok(Step::Again);
        }
        let Some(why) = crate::ai::llm::rejected_tool_call(&e) else {
            return Err(e);
        };
        // A step refused mid-stream may have shown some text: it
        // wasn't the answer.
        if let Some(live) = self.live {
            live.sink.send(crate::chat::stream::Event::Text { text: String::new() });
        }
        if self.rejected < REJECTED_CALL_RETRIES {
            self.rejected += 1;
            tracing::info!(provider = self.provider, "Chat: the provider rejected the model's tool call; asking again");
            self.messages.push(json!({ "role": "user", "content": rejected_call_note(why) }));
            return Ok(Step::Again);
        }
        tracing::info!(provider = self.provider, "Chat: the model's tool call was rejected again; answering without tools");
        Ok(Step::GiveUp)
    }

    /// What Claude's own web tools did in this reply (llm/anthropic.rs):
    /// chips, the turn's sources, its citations numbered — before any of
    /// its tool calls run, so the safety rule sees the web first.
    fn took_server_web(&mut self, msg: &mut Value) {
        if let Some(w) = msg.as_object_mut().and_then(|o| o.remove("yap_web")) {
            let (chips, numbers) = self.web.as_deref_mut().map(|t| t.took_server(&w)).unwrap_or_default();
            tracing::info!(chips = chips.len(), cited = numbers.len(), "Chat: Claude searched the web itself");
            self.tools_used.extend(chips);
            self.report();
            if let Some(text) = msg["content"].as_str() {
                msg["content"] = json!(crate::ai::llm::anthropic::number_marks(text, &numbers));
            }
        }
    }

    /// **The model answered in words** (`content`): the answer — unless a
    /// new document's first step made no outline, or a document this turn
    /// made still has sections to write: then it's asked once to outline
    /// it, or to finish it, and the step goes again.
    fn answered(&mut self, content: String) -> Step {
        // A new document's first step, its web tools held back for the
        // outline, answered in words ("I'll look this up first…"):
        // asked once to make the outline; the web stays held for that
        // one step more.
        if self.web.as_deref().is_some_and(crate::web::Turn::outline_pending) {
            if !self.outline_nudged {
                self.outline_nudged = true;
                tracing::info!(provider = self.provider, "Chat: a new document's first step made no outline; the model is asked to make it");
                crate::ai::eval::post_fix("outline_first");
                if let Some(live) = self.live {
                    live.sink.send(crate::chat::stream::Event::Text { text: String::new() });
                }
                self.messages.push(json!({ "role": "assistant", "content": content }));
                self.messages.push(json!({ "role": "user", "content": crate::chat::documents::OUTLINE_NOTE }));
                return Step::Again;
            }
            if let Some(turn) = self.web.as_deref_mut() {
                turn.outline_settled();
            }
        }
        // A document this turn made still has sections to write
        // (documents.rs): the model finishes it before it answers.
        if let Some((_, title, left)) = self.extra.docs.and_then(crate::chat::documents::Turn::unfinished) {
            if self.finish_nudges < crate::chat::documents::FINISH_NUDGES {
                self.finish_nudges += 1;
                tracing::info!(left = left.len(), "Chat: the document isn't finished; the model is asked to write the rest");
                crate::ai::eval::post_fix("finish_document");
                // What it wrote wasn't the answer yet.
                if let Some(live) = self.live {
                    live.sink.send(crate::chat::stream::Event::Text { text: String::new() });
                }
                self.messages.push(json!({ "role": "assistant", "content": content }));
                self.messages.push(json!({ "role": "user", "content": crate::chat::documents::finish_note(&title, &left) }));
                return Step::Again;
            }
        }
        Step::Answer(content)
    }

    /// **Out of steps, or the tool calls kept being rejected**: one final
    /// call WITHOUT tools to force a plain answer. The answer and the
    /// turn's chips.
    pub(super) async fn answer_now(mut self) -> Result<(String, Vec<String>), String> {
        self.messages.push(json!({ "role": "user", "content": ANSWER_NOW_NOTE }));
        let native = self.claude_step();
        // (Claude on the Messages API keeps its tools, told to call none: its
        // earlier calls in the messages need them defined.)
        let mut req = if self.native_chat {
            crate::ai::llm::ChatRequest::chat(self.base_url, self.api_key, self.model, self.provider, 0.5, Value::Array(self.messages), Some(self.tools), Some(json!("none")))
        } else {
            crate::ai::llm::ChatRequest::chat(self.base_url, self.api_key, self.model, self.provider, 0.5, Value::Array(self.messages), None, None)
        };
        req.native = native;
        let msg = ask(self.live, &req, false).await?;
        let content = msg["content"].as_str().unwrap_or("").trim().to_string();
        let content = crate::chat::think::answer_without_inline(&content, self.disable_thinking);
        if content.is_empty() {
            return Err("The model didn't manage an answer this time. Try asking again.".to_string());
        }
        Ok((content, self.tools_used))
    }
}

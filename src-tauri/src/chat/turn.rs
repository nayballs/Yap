//! One chat turn, answered: the Chat scope's endpoint, the prompt (the
//! notes the eager keyword-RAG finds, meetings, memory, About you), the
//! tool loop, or a small model's fixed web pipeline, and the checks on
//! every answer. The Chat view (`chat_send`), the Voice Assistant and the
//! model evaluations all ask through [`chat_turn`].

use tauri::AppHandle;

use crate::config::{self, YapConfig};
use crate::notes::commands::note_changed;

/// Resolve the **Chat** scope's endpoint + persona (falling back to the global
/// cleanup endpoint when the scope is off) and fail fast on keyless cloud
/// providers. Shared by the embedded note chat and the AI Chat surface, and
/// by the Chat view's model chip (`ai_model::target`), which must name the
/// model these calls use.
pub(crate) fn resolve_chat_endpoint(
    cfg: &YapConfig,
) -> Result<(String, String, String, String, String, bool), String> {
    let scope = cfg.llm_scopes.get("chat");
    // A scope switched on from the chat's model chip may have no prompt yet.
    let persona = scope
        .map(|s| s.prompt.clone())
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| "You are a helpful assistant. Answer concisely.".to_string());
    let (base_url, api_key, model, provider, persona, disable_thinking) = match scope {
        Some(s) if s.enabled && !s.provider.is_empty() => {
            let key = cfg.provider_api_key(&s.provider, &s.api_key);
            let (b, k, m, p) =
                crate::ai::local_llm::effective_endpoint_for(&s.provider, &s.base_url, &key, &s.model);
            (b, k, m, p, persona, s.disable_thinking)
        }
        _ => {
            let (b, k, m, p) = crate::ai::local_llm::effective_endpoint(cfg);
            (b, k, m, p, persona, cfg.pp_disable_thinking)
        }
    };
    if base_url.is_empty() {
        return Err(
            "No AI model configured — set one in Settings → Language Models → Chat".to_string(),
        );
    }
    const KEYED_PROVIDERS: [&str; 5] = ["groq", "anthropic", "openai", "gemini", "openrouter"];
    if api_key.is_empty() && KEYED_PROVIDERS.contains(&provider.as_str()) {
        return Err(format!(
            "No {provider} API key — add one in Settings → Language Models"
        ));
    }
    Ok((base_url, api_key, model, provider, persona, disable_thinking))
}

/// What the notes the eager RAG found ([`rag_context`]) come under, in the
/// question's context (`chats::with_context`): OpenWhispr's framing line.
/// (Telling the model the notes "may not be what it's about" didn't keep
/// Qwen3.5-4B from answering a follow-up about a note that only shared
/// "time" with it — 0 of 6, `notes-long-chat`, 2026-10-22; the stricter
/// match, `tools::rag_notes`, did.)
const RAG_LEAD: &str = "Below are notes from the user's library that may be relevant. Reference them naturally if they help answer the question.";

/// Eager keyword-RAG (OpenWhispr `buildRAGContext`, keyword edition per the
/// ROADMAP's escalating plan): top-5 notes by the shared keyword scorer —
/// `strict`, only those that really match the question (`tools::rag_notes`:
/// for a question's context, since 22 Oct 2026, where a loose match pulled
/// follow-ups off the conversation); else the loose search, as before, in
/// the system prompt of a model without tools (Qwen2.5-1.5B answered less
/// well from one note than from five) — formatted exactly like theirs —
/// `<note id="N" title="T">\n{first 500 chars}\n</note>` joined by blank
/// lines.
fn rag_context(query: &str, strict: bool) -> String {
    const RAG_NOTE_LIMIT: usize = 5;
    const RAG_NOTE_SNIPPET_LENGTH: usize = 500;

    let notes = if strict {
        crate::chat::tools::rag_notes(query, RAG_NOTE_LIMIT)
    } else {
        crate::chat::tools::search_notes(query, RAG_NOTE_LIMIT)
    };
    notes
        .into_iter()
        .map(|(_, n)| {
            // Snippet from raw content (their choice); fall back to enhanced /
            // transcript for notes that are transcript-only.
            let source = if !n.content.trim().is_empty() {
                n.content.clone()
            } else if !n.enhanced_content.trim().is_empty() {
                n.enhanced_content.clone()
            } else {
                // Who said each line (a name the speaker map has, else
                // You / Them): "who fixed the form?" answers from it
                // (Qwen3.5-4B named the wrong person from the bare lines,
                // `evals/search/library.json`, 2026-10-25).
                n.transcript
                    .iter()
                    .filter(|s| s.is_talk() && !s.text.trim().is_empty())
                    .map(|s| format!("{}: {}", crate::speakers::prompt_label(&n.speakers, s), s.text.trim()))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let snippet: String = source.chars().take(RAG_NOTE_SNIPPET_LENGTH).collect();
            let title = if n.title.trim().is_empty() {
                "Untitled".to_string()
            } else {
                n.title.clone()
            };
            format!("<note id=\"{}\" title=\"{}\">\n{}\n</note>", n.id, title, snippet)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// One chat turn, answered: what [`chat_turn`] returns.
pub(crate) struct ChatTurn {
    pub conversation_id: u64,
    /// The question's and the answer's indexes in the conversation.
    pub user_index: usize,
    pub index: usize,
    pub reply: String,
    pub tools_used: Vec<String>,
    /// Who answered; `None` when Yap did itself (an app opened, `apps.rs`).
    pub model: Option<crate::chat::chats::ChatModel>,
    /// The web pages the answer was written from (`web.rs`), numbered as
    /// it cites them.
    pub sources: Vec<crate::chat::chats::Source>,
    /// Stopped while it was being written (the page's ■): what came so far.
    pub stopped: bool,
    /// Its last model call's tokens (`usage::last_call`), kept with the
    /// answer: Chat's context ring.
    pub tokens: Option<crate::chat::chats::MessageTokens>,
}

impl ChatTurn {
    /// The answer's id, as the pages call it ("chat:<conversation>:<index>").
    pub fn message_id(&self) -> String {
        format!("chat:{}:{}", self.conversation_id, self.index)
    }
}

/// One chat turn, for the Chat view (`chat_send`) and the Voice Assistant
/// (`assistant.rs`). Creates the conversation on first message (title =
/// first 50 chars, OpenWhispr's rule), persists both sides, and answers via
/// the Chat scope with eager keyword-RAG over the notes library injected under
/// their exact framing line. `asked` runs once the question is saved (the
/// conversation's id and the question's index), before the model is called.
/// A note the tools create or change is announced as it happens, like any
/// other note edit (`yap-note-changed` with origin "chat", then
/// `yap-notes-changed`), so the notepad and the lists follow it live.
///
/// "Open Teams" and other plain requests to open an app are answered by Yap
/// itself, before any model (`apps::plan_for`): with no AI set up, with a
/// small local model, and without a model call at all — and so are the
/// answers to its own questions ("Did you mean Baldur's Gate 3?" → "yes").
/// `scope` "meetings": grounded in the meetings (the Meetings view's Ask
/// bar); "assistant": the Voice Assistant, which sends a shorter history.
///
/// `app`: the windows told of note changes; `None` in a model evaluation
/// (`eval.rs`), which runs this same turn with no windows.
///
/// **Streamed** (`stream.rs`) when someone watches (`watcher`: the Chat
/// view's channel, the Voice Assistant, an evaluation's clock): every model
/// call's text as it's written, each tool call as its chip; a model on this
/// PC steps aside for a dictation. The answer that comes back is the
/// **checked** one (below), which the page puts in place of what streamed.
/// `stop`: the page's ■ — the stream closes, and what was shown so far is
/// the answer, checked and saved as stopped (nothing shown: an error,
/// [`crate::chat::stream::STOPPED`]).
///
/// **A newer question replaces an older one** still being answered in the
/// same conversation: once this turn's question is saved, a turn still
/// answering there ends with nothing saved (`stream::Stoppable::answers`,
/// the error [`crate::chat::stream::SUPERSEDED`]) — its question stays, and the
/// model reads it with the newer one. A tester's voice chat (2026-10-23):
/// the Voice Assistant's first question, set aside for the barge-in that
/// asked the second, was asked again after the second's answer, and both
/// answers were saved under both questions.
///
/// **Its tokens** (`usage.rs`): the turn's last model call — the step that
/// wrote the answer, which read the whole prompt — is kept with the answer
/// (`ChatMessage::tokens`): how much of the model's context the
/// conversation uses (Chat's context ring).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn chat_turn(
    app: Option<&AppHandle>,
    conversation_id: Option<u64>,
    text: &str,
    scope: Option<&str>,
    by_voice: bool,
    asked: impl FnOnce(u64, usize) + Send,
    watcher: Option<&dyn crate::chat::stream::Sink>,
    stop: Option<&crate::chat::stream::Stoppable>,
) -> Result<ChatTurn, String> {
    // Boxed: a turn's future is big, and every caller's holds this one (a
    // Chat turn inline overflowed the main thread's stack).
    crate::ai::usage::tallied(Box::pin(answer_turn(app, conversation_id, text, scope, by_voice, asked, watcher, stop))).await
}

/// [`chat_turn`], its last model call kept by `usage::tallied`.
#[allow(clippy::too_many_arguments)]
async fn answer_turn(
    app: Option<&AppHandle>,
    conversation_id: Option<u64>,
    text: &str,
    scope: Option<&str>,
    by_voice: bool,
    asked: impl FnOnce(u64, usize) + Send,
    watcher: Option<&dyn crate::chat::stream::Sink>,
    stop: Option<&crate::chat::stream::Stoppable>,
) -> Result<ChatTurn, String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("Type a message first".to_string());
    }
    // No ■ from a page (the Voice Assistant, an evaluation): a switch of its
    // own, so a newer question can still end it.
    let own_stop;
    let stop = match stop {
        Some(s) => s,
        None => {
            own_stop = crate::chat::stream::Stoppable::new(None);
            &own_stop
        }
    };
    // A conversation asked across the meetings stays so: its follow-ups too
    // (`chats::Conversation::scope`). A scope the caller names wins.
    let kept_scope = conversation_id.map(crate::chat::chats::scope_of).unwrap_or_default();
    let scope: Option<&str> = scope.or((!kept_scope.is_empty()).then_some(kept_scope.as_str()));
    let meetings = scope == Some("meetings");
    let cfg = config::load();
    if let Some(plan) = crate::apps::plan_for(&text, &cfg, conversation_id).await {
        return app_turn(conversation_id, &text, by_voice, asked, plan, stop).await;
    }
    let endpoint = match crate::ai::ai_model::wake_on_device(&cfg).await {
        Ok(_) => resolve_chat_endpoint(&cfg),
        Err(e) => Err(e),
    };
    let (base_url, api_key, model, provider, persona, disable_thinking) = match endpoint {
        Ok(e) => e,
        // Asked across the meetings with no model to answer: the passages
        // that match, themselves.
        // (Boxed, as the search below: the turn's own future stays small.)
        Err(why) if meetings => return Box::pin(passages_turn(conversation_id, &text, by_voice, asked, stop, &why)).await,
        Err(why) => return Err(why),
    };
    // The on-device model is busy for the whole turn — between its tool
    // steps too, while a web search or a page read runs — so a context
    // restart or a runtime swap never cuts the answer off.
    let _sidecar = crate::ai::local_llm::hold(&base_url);
    // Who answers, as the chip named it, kept with the answer.
    let answered_by = crate::ai::ai_model::describe(
        &provider,
        &model,
        &base_url,
        &crate::ai::local_llm::active_model_display(&cfg),
    )
    .chat_model();

    // A summary being written on this PC steps aside for the question.
    crate::chat::compact::turn_started();
    let tools_supported = crate::chat::tools::supports_tools(&provider, &model);
    let in_message = tools_supported;
    // "Look things up on the web" (web.rs). Not for the Meetings view's Ask
    // bar: that's about the meetings.
    let web_on = cfg.web_search && scope != Some("meetings");
    // The calendar (chat/tools/calendar.rs): a tool while one is connected
    // (not across the meetings, whose passages are numbered on their own).
    let calendar_on = scope != Some("meetings") && crate::calendar::connected();
    let calendar_log = crate::chat::tools::calendar::Log::default();
    // About you (about_you.rs): where the person is, their money and units,
    // name and note — last in the system prompt, below; their clock in the
    // question's context (it changes every minute).
    let profile = crate::ai::about_you::profile(&cfg);
    let now = crate::ai::eval::now();

    // **Sized to the model** (budget.rs): its context, measured parts, and
    // the question alone within what's left — a paste too long for the
    // model is never sent (nor saved): it's said plainly.
    let kind = crate::chat::budget::Kind::of_scope(scope);
    let mode = crate::chat::budget::mode();
    let counter = crate::chat::budget::Counter::for_call(&provider, &base_url);
    let (context_size, context_known) = crate::chat::budget::context_window(&cfg, &provider, &model, &base_url).await;
    let label = crate::chat::budget::model_label(&provider, &model, &crate::ai::local_llm::active_model_display(&cfg));
    let plain = |e: String| crate::chat::budget::plain_error(e, &label, context_size, &provider);
    let extra_flags = crate::chat::tools::Extra {
        memory: true,
        search_chats: cfg.memory_search_chats,
        conversation: conversation_id,
        calendar: calendar_on.then_some(crate::chat::tools::CalendarTurn { log: &calendar_log, zone: crate::chat::tools::calendar::Zone::of(None), now: 0 }),
        room: None,
    };
    let schemas = if tools_supported {
        counter.count(&crate::chat::tools::offered_tools(cfg.open_apps, web_on, extra_flags).to_string()).await
    } else {
        0
    };
    // The question as sent: its text, and the screenshots it carries.
    let question_tokens = counter.count(&text).await
        + crate::chat::budget::MESSAGE_OVERHEAD
        + crate::chat::budget::IMAGE_TOKENS * crate::snip::turn::images().iter().filter(|i| !i.as_text).count() as u64;
    let rate_cap = crate::chat::budget::rate_cap(&provider);
    if mode != crate::chat::budget::Mode::Messages {
        let stable = stable_prompt(&cfg, &persona, tools_supported, web_on, calendar_on, &profile, now);
        let guess = crate::chat::budget::budget(context_size, context_known, counter.count(&stable).await + schemas, kind, tools_supported, rate_cap);
        if question_tokens > guess.history {
            tracing::info!(question_tokens, room = guess.history, context = context_size, "Chat: the question is too long for the model; not sent");
            return Err(crate::chat::budget::too_long(&label, context_size));
        }
    }
    // Conversation bookkeeping (create on first message, OpenWhispr-style).
    let conv = turn_conversation(conversation_id, &text);
    if meetings && conv.scope.is_empty() {
        crate::chat::chats::set_scope(conv.id, "meetings");
    }
    // The model answers this one: Yap's own question about apps has lapsed.
    crate::apps::moved_on(Some(conv.id));
    // The question, with the screenshots it carries (Snip to Chat, the
    // turn's `snip::turn`): kept with it in chats.json as ids.
    let user_index = crate::chat::chats::append_question(conv.id, &text, by_voice, &crate::snip::turn::images())?;
    // This turn answers the conversation now: one still answering there
    // ends, with nothing saved.
    stop.answers(conv.id);
    asked(conv.id, user_index);

    // **Two parts, for the model's cache** (2026-10-22): the system prompt
    // holds what stays the same from turn to turn — the persona, the
    // instructions, About you without its clock — and everything that
    // changes with the question (the time, the notes the eager RAG found,
    // the meetings, earlier web answers, how to answer out loud) goes in
    // the question's own message (`chats::with_context`). A hybrid local
    // model (Qwen3.5) re-uses its cache only up to a checkpoint, so a
    // system prompt that changed every turn made every Chat request read
    // the whole conversation again (docs/model-evals.md); cloud providers'
    // prefix caches like a stable start too. That's for a model that calls
    // tools (Qwen3.5-4B, the cloud models); one that can't (Qwen2.5-1.5B,
    // dense: it re-reads only from the first change anyway) keeps it all in
    // the system prompt — with the notes next to the question it gave half
    // answers (4 of 10 turns, `notes-follow-ups`, 2026-10-22). OpenWhispr
    // runs BOTH eager RAG and the notes tools: the model gets relevant notes
    // up front AND can search for more.
    // "Remember that…" / "Forget that…" to a model without tools: Yap's
    // own memory pass makes the change (memory/learn.rs), and says so. A
    // model with tools does it with `remember` / `forget`.
    if !tools_supported {
        if let Some(done) = crate::memory::learn::spoken(&text).await {
            let (reply, chips) = done?;
            let index = crate::chat::chats::append(conv.id, "assistant", &reply, &chips, Some(answered_by.clone()), false)?;
            return Ok(ChatTurn {
                conversation_id: conv.id,
                user_index,
                index,
                reply,
                tools_used: chips,
                model: Some(answered_by),
                sources: Vec::new(),
                stopped: false,
                tokens: None,
            });
        }
    }
    // Memory (memory.rs): the "You" part in the system prompt with About
    // you (stable from turn to turn), what this question is about with it.
    let memory_you = crate::memory::you_block();
    // By its words, and by meaning when that's on (search/library.rs).
    let memory_here = crate::search::library::memory_block(&text).await;
    // "Search and reference past chats": a tool for a model with tools; for
    // one without, the chats a question points back to ("what did we say
    // about Lisbon?"), a couple of lines each.
    let past_chats = if cfg.memory_search_chats && !tools_supported && crate::chat::chats::refers_to_past(&text) {
        crate::search::library::past_chats_block(&text, conv.id).await
    } else {
        None
    };
    // Which earlier answers came from a search: a tool-calling model with
    // the web gets each one's search replayed as the call it was
    // (`chats::history_messages`); any other model, a line in the
    // question's context (`chats::web_history_note`). Never a note in an
    // answer's text: a model copies it into answers that searched nothing.
    let (replay, history_note) = web_history_mode(tools_supported && web_on);
    let about = crate::ai::about_you::prompt_block(&profile, now);
    let calendar_zone = crate::chat::tools::calendar::Zone::of(profile.region.time_zone.as_deref());
    let mut system = persona.trim().to_string();
    system.push_str(&instructions(&cfg, tools_supported, web_on, calendar_on, &profile));
    // Where the line on earlier web answers goes for a model without tools
    // (it depends on the history sent, chosen below).
    let note_at = system.len();
    let mut context: Vec<String> = Vec::new();
    if in_message {
        context.push(crate::ai::about_you::clock_line(&profile, now));
    }
    // A model on this PC asked something that needs the web: its first step
    // must be a search (left to decide, Qwen3.5-4B answered "the best RPG
    // games coming out in 2026" from memory, a tester's chat, 2026-10-21).
    // Cloud models decide for themselves.
    let first = if tools_supported && web_on && crate::ai::ai_model::runs_here(&provider, &base_url) && crate::web::needs_web(&text) {
        first_search(&text)
    } else {
        crate::chat::tools::FirstStep::Model
    };
    // Asked across the meetings (the Meetings view's Ask bar, a conversation
    // started there): the passages that match, found on this PC — by
    // meaning and keywords, within the dates the question names — numbered
    // for the model to cite, each a source that opens its meeting at the
    // line (search/ask.rs).
    let mut passages: Vec<crate::chat::chats::Source> = Vec::new();
    let mut search_chip = None;
    let mut asked_across = None;
    if meetings {
        let found = Box::pin(crate::search::ask::find(&text, crate::search::ask::ASK_PASSAGES)).await;
        passages = crate::search::ask::sources(&found);
        let chip = crate::search::ask::chip(&found);
        if let Some(w) = watcher {
            w.send(crate::chat::stream::Event::Chips { chips: vec![chip.clone()], sources: passages.clone(), web: false });
        }
        search_chip = Some(chip);
        system.push_str("\n\n");
        system.push_str(&crate::search::ask::instructions(in_message));
        let block = crate::search::ask::context(&found);
        asked_across = Some(found);
        if in_message {
            context.push(block);
        } else {
            system.push_str("\n\n");
            system.push_str(&block);
        }
    }
    let rag = if meetings { String::new() } else { rag_context(&text, in_message) };
    if !rag.is_empty() {
        if in_message {
            context.push(format!("{RAG_LEAD}\n\n{rag}"));
        } else {
            system.push_str(&format!("\n\n{RAG_LEAD}\n\n{rag}"));
        }
    }
    // A model without tools asked about the calendar: Yap looks it up for it
    // (not beside the fixed web pipeline, whose prompt is its own).
    let mut small_calendar: Vec<crate::chat::tools::calendar::Cited> = Vec::new();
    let (calendar_block, calendar_chip) = (calendar_on && !tools_supported && !(web_on && crate::web::needs_web(&text)))
        .then(|| crate::chat::tools::calendar::block(&text, calendar_zone, now.timestamp(), &mut small_calendar))
        .flatten()
        .unzip();
    // Asked about the calendar with none connected: say so, not guess.
    let calendar_missing = (!calendar_on && scope != Some("meetings") && crate::chat::tools::calendar::asks_about_calendar(&text))
        .then(|| crate::chat::tools::calendar::NOT_CONNECTED_NOTE.to_string());
    for block in memory_here.into_iter().chain(past_chats).chain(calendar_block).chain(calendar_missing) {
        if in_message {
            context.push(block);
        } else {
            system.push_str("\n\n");
            system.push_str(&block);
        }
    }
    // How the person wants answers, again last in the context, where a
    // small tool model heeds it (a model without tools has it at the end
    // of its system prompt, in memory's "You" block).
    if in_message {
        if let Some(note) = crate::memory::answer_note() {
            context.push(note);
        }
    }
    // Where an older stretch's summary goes (`compact.rs`), when the history
    // needs one: before the prompt's end, which small models heed most.
    let summary_at = system.len();
    // Last in the system prompt, where small models heed it: what the
    // context block is and About you without its clock — or, the context
    // staying in the system prompt, the whole block with it.
    system.push_str(&ending(in_message, &profile, &about, &memory_you));
    // What came with the question from the screen (the Voice Assistant's
    // look at the window in front, `assistant/screen.rs`): what the picture
    // or its text is, or why there's none.
    if let Some(note) = crate::snip::turn::note() {
        if in_message {
            context.push(note);
        } else {
            system.push_str("\n\n");
            system.push_str(&note);
        }
    }
    // Asked out loud: a tool model with the web starts with the answer; one
    // read aloud whole — the Voice Assistant, or a question dictated into
    // Chat with spoken replies on (a web answer is said short anyway,
    // `assistant::spoken_answer`) — answers short. Last in the context,
    // just before the question, where small models heed it.
    if by_voice && tools_supported && web_on {
        context.push("This question was asked out loud: start your answer with the answer itself, in one or two short sentences, then any details.".to_string());
    }
    let spoken = by_voice && (scope == Some("assistant") || cfg.speak_replies);
    if spoken && !(tools_supported && web_on) {
        if in_message {
            context.push(crate::chat::assistant::SPOKEN_ANSWER_NOTE.to_string());
        } else {
            system.push_str("\n\n");
            system.push_str(crate::chat::assistant::SPOKEN_ANSWER_NOTE);
        }
    }

    // **The history, sized to the model** (budget.rs): the parts measured
    // — the system prompt, the context block, the tools' schemas — and the
    // history in what's left of the budget: every message when they fit;
    // else the summary of the older ones (compact.rs) and those after it;
    // else the latest that fit (and a summary written after the answer).
    let fixed = counter.count(&system).await
        + crate::chat::chats::context_block(&context).map(|b| crate::chat::budget::estimate(&b)).unwrap_or(0)
        + schemas
        + crate::chat::budget::MESSAGE_OVERHEAD;
    // Sized within the model's cheaper rate too (`request_cap`): a long
    // chat's summary takes over before a request would cost more.
    let budget = crate::chat::budget::budget(context_size, context_known, fixed, kind, tools_supported, crate::chat::budget::request_cap(&provider, &model));
    let room = budget.history.saturating_sub(question_tokens);
    let summary = conv.usable_summary().filter(|_| mode == crate::chat::budget::Mode::Summary);
    let plan = match mode {
        crate::chat::budget::Mode::Messages => {
            let (keep, step) = if scope == Some("assistant") { crate::chat::chats::ASSISTANT_HISTORY } else { crate::chat::chats::CHAT_HISTORY };
            let start = crate::chat::chats::history_start(conv.messages.len(), keep, step);
            crate::chat::budget::Plan { start, summary: false, left_out: start, tokens: 0 }
        }
        _ => {
            let costs = crate::chat::budget::message_costs(&counter, &conv.messages, replay).await;
            let summary_cost = match summary {
                Some(s) => Some((s.upto, counter.count(&crate::chat::compact::block(&s.text)).await)),
                None => None,
            };
            crate::chat::budget::plan(&costs, summary_cost, room, kind.step())
        }
    };
    let earlier = &conv.messages[plan.start..];
    tracing::info!(
        messages = conv.messages.len(),
        sent = earlier.len(),
        summary = plan.summary,
        left_out = plan.left_out,
        history_tokens = plan.tokens,
        room,
        fixed,
        context = context_size,
        "Chat: the history, sized to the model"
    );
    // (Later in the prompt first: the earlier offset stays right.)
    if let (true, Some(s)) = (plan.summary, summary) {
        system.insert_str(summary_at, &format!("\n\n{}", crate::chat::compact::block(&s.text)));
    }
    if history_note && !replay {
        if let Some(note) = crate::chat::chats::web_history_note(earlier) {
            if in_message {
                // After the clock, as before.
                context.insert(1.min(context.len()), note);
            } else {
                system.insert_str(note_at, &format!("\n\n{note}"));
            }
        }
    }
    // Answers another model wrote (a switch in the picker): said so, so
    // this one never claims them (`chats::other_models_note`).
    if let Some(note) = crate::chat::chats::other_models_note(earlier, &answered_by) {
        if in_message {
            context.insert(1.min(context.len()), note);
        } else {
            system.insert_str(note_at, &format!("\n\n{note}"));
        }
    }
    let history: Vec<(String, String)> = earlier.iter().map(|m| (m.role.clone(), m.text.clone())).collect();

    // Streamed to whoever watches: what was shown is kept (a Stop keeps
    // it), and a model on this PC steps aside for a new dictation.
    let watch = watcher.map(crate::chat::stream::Watch::new);
    let beside = app
        .filter(|_| crate::ai::ai_model::runs_here(&provider, &base_url))
        .map(|a| crate::chat::stream::NewDictations(a.clone()));
    let live = watch.as_ref().map(|w| crate::chat::stream::Live {
        sink: w,
        beside: beside.as_ref().map(|b| b as &dyn crate::chat::stream::Dictations),
    });

    let (reply, tools_used, sources, looked, pages, stopped) = if tools_supported {
        let notify = |id: u64| {
            use tauri::Emitter;
            if let Some(app) = app {
                note_changed(app, id, "chat");
                let _ = app.emit("yap-notes-changed", ());
            }
        };
        // The turn's web side: the person's links (this message and their
        // earlier ones), the sources found, the safety rule's flag.
        let mut web = web_on.then(|| {
            let earlier: Vec<&str> = history.iter().filter(|(r, _)| r == "user").map(|(_, t)| t.as_str()).collect();
            let settings = crate::web::Settings::for_chat(&cfg, &provider, &model, &base_url, &api_key);
            crate::web::Turn::new(settings, crate::web::Endpoints::current(), &text, &earlier)
                .with_earlier_sources(conv.messages.iter().flat_map(|m| m.sources.iter().map(|s| s.url.as_str())))
        });
        let mut messages: Vec<serde_json::Value> = if replay {
            crate::chat::chats::history_messages(earlier)
        } else {
            history
                .iter()
                .map(|(r, t)| serde_json::json!({ "role": if r == "assistant" { "assistant" } else { "user" }, "content": t }))
                .collect()
        };
        // Earlier screenshots: the newest few sent again, the rest a line
        // (snip/turn.rs).
        crate::snip::turn::with_history_images(&mut messages, earlier, &provider, &model, &base_url).await;
        let extra = crate::chat::tools::Extra {
            memory: true,
            search_chats: cfg.memory_search_chats,
            conversation: Some(conv.id),
            calendar: calendar_on.then_some(crate::chat::tools::CalendarTurn { log: &calendar_log, zone: calendar_zone, now: now.timestamp() }),
            // Each tool result within what the request has left.
            room: (mode != crate::chat::budget::Mode::Messages).then_some(budget.request),
        };
        let looped = crate::chat::tools::run_tool_loop_with(
            &system,
            &messages,
            &text,
            &context,
            &base_url,
            &api_key,
            &model,
            &provider,
            disable_thinking,
            cfg.open_apps,
            &notify,
            web.as_mut(),
            first,
            extra,
            live,
        );
        // Stopped: what was shown so far is the answer (a step's text that
        // came before a tool call was never shown). Superseded: no answer.
        let (reply, tools_used, stopped) = match crate::chat::stream::unless_stopped(looped, Some(stop)).await {
            Ok(done) => {
                // "Too long for the model" said plainly, never the provider's JSON.
                let (reply, tools_used) = done.map_err(plain)?;
                (reply, tools_used, false)
            }
            Err(crate::chat::stream::Halt::Superseded) => return Err(superseded()),
            Err(crate::chat::stream::Halt::Stopped) => {
                let seen = watch.as_ref().map(|w| w.seen()).unwrap_or_default();
                tracing::info!(shown = seen.text.len(), "Chat: the answer was stopped");
                (seen.text, seen.chips, true)
            }
        };
        // gpt-oss sometimes cites as "【1】" (its own browsing style): Yap's [1].
        let reply = if web.is_some() || calendar_log.len() > 0 { crate::web::bracket_citations(&reply) } else { reply };
        // A meeting the answer names without its number gets it.
        let reply = crate::chat::tools::calendar::cite_mentions(&reply, &calendar_log.all());
        let looked = web.as_ref().is_some_and(crate::web::Turn::looked);
        let mut sources = web.as_ref().map(|w| w.sources_for(&reply)).unwrap_or_default();
        sources.extend(crate::chat::tools::calendar::sources_for(&reply, &calendar_log.all()));
        sources.sort_by_key(|s| s.n);
        let pages = web.map(|w| w.texts).unwrap_or_default();
        (reply, tools_used, sources, looked, pages, stopped)
    } else {
        // Small/local models: plain chat + eager RAG (their fallback), or
        // Yap's fixed web pipeline when the question needs the web.
        let endpoint = (base_url.as_str(), api_key.as_str(), model.as_str(), provider.as_str(), disable_thinking);
        // The web pipeline's own prompt ends as this one does: About you,
        // then memory's "You".
        let about_and_memory = if memory_you.is_empty() { about.clone() } else { format!("{about}\n\n{memory_you}") };
        let SmallAnswer { reply, chips, sources, looked, pages, stopped } = small_model_answer(
            &cfg,
            &persona,
            &system,
            &about_and_memory,
            &history,
            &text,
            endpoint,
            web_on,
            by_voice,
            live,
            Some(stop),
            watch.as_ref(),
        )
        .await
        .map_err(plain)?;
        // "Opening Baldur's Gate 3." from a model that can't open anything:
        // said as it is instead (apps.rs).
        let corrected = if crate::apps::claims_unasked(&reply, &text) {
            let (question, said, open_apps) = (text.clone(), reply.clone(), cfg.open_apps);
            tauri::async_runtime::spawn_blocking(move || {
                crate::apps::correct_claim(&said, &question, &crate::apps::apps(false), open_apps)
            })
            .await
            .ok()
            .flatten()
        } else {
            None
        };
        // The calendar Yap looked up for it: the meetings it names, cited.
        let mut sources = sources;
        let reply = corrected.unwrap_or(reply);
        let reply = if small_calendar.is_empty() {
            reply
        } else {
            let reply = crate::chat::tools::calendar::cite_mentions(&reply, &small_calendar);
            sources.extend(crate::chat::tools::calendar::sources_for(&reply, &small_calendar));
            sources.sort_by_key(|s| s.n);
            reply
        };
        let mut chips = chips;
        if let Some(chip) = calendar_chip {
            chips.insert(0, chip);
        }
        (reply, chips, sources, looked, pages, stopped)
    };
    // Stopped before any of the answer showed: nothing to save.
    if stopped && reply.trim().is_empty() {
        return Err(crate::chat::stream::STOPPED.to_string());
    }
    // Every model's answer, checked before it's shown: no sources list or
    // "Yap searched…" note of its own, no search claimed that didn't
    // happen, no "[1]" with nothing (or another question's pages) behind it
    // — asked across the meetings, a number that's none of this turn's
    // passages.
    let (mut reply, fixed) = if meetings {
        let (r, mut f) = crate::chat::chats::checked_answer(&reply, &text, looked, &[], &passages);
        let only_these: Vec<usize> = passages.iter().map(|s| s.n).collect();
        let stripped = crate::chat::chats::strip_unsourced_citations(&r, &only_these);
        if stripped != r && !f.contains(&"stripped_citations") {
            f.push("stripped_citations");
        }
        (stripped, f)
    } else {
        crate::chat::chats::checked_answer(&reply, &text, looked, &conv.messages, &sources)
    };
    for f in fixed {
        tracing::info!(fix = f, "Chat: Yap fixed the model's answer");
        crate::ai::eval::post_fix(f);
    }
    // Asked across the meetings: the passages it cites are its sources (none
    // cited: the first few, to check it against), and an answer that cites
    // none of them says so.
    let (sources, tools_used) = if meetings {
        // A statement plainly from one passage that the model left uncited
        // ("Passage 2", or only the meeting's name): Yap cites it.
        if let Some(found) = &asked_across {
            let (cited, added) = crate::search::ask::cite_missing(&reply, &found.passages);
            if added > 0 {
                tracing::info!(added, "Chat: Yap put in citations the answer left out");
                crate::ai::eval::post_fix("cited_by_yap");
                reply = cited;
            }
        }
        if crate::search::ask::needs_uncited_note(&reply, passages.len()) {
            tracing::info!("Chat: the answer cited none of the passages; Yap says so under it");
            crate::ai::eval::post_fix("uncited_answer");
            reply = format!("{}\n\n{}", reply.trim_end(), crate::search::ask::UNCITED_NOTE);
        }
        let mut chips: Vec<String> = search_chip.into_iter().collect();
        chips.extend(tools_used);
        (crate::search::ask::shown(&reply, &passages), chips)
    } else {
        (sources, tools_used)
    };
    // A price question that searched: no offer to look for prices where the
    // person is — Yap looked (and, finding only another country's, looked
    // again).
    if looked && crate::web::asks_about_prices(&text) {
        if let Some(stripped) = crate::ai::about_you::LocalPrices::for_region(&profile.region)
            .and_then(|local| crate::ai::about_you::strip_price_offer(&reply, &local))
        {
            tracing::info!("Chat: Yap took out the answer's offer to look for local prices");
            crate::ai::eval::post_fix("price_offer");
            reply = stripped;
        }
    }
    // A price the web gave only in another money ("$8"), written in theirs
    // ("£8"): its page's sign back, and a note saying so.
    let mut restored_note = None;
    if let Some(local) = crate::ai::about_you::LocalPrices::for_region(&profile.region).filter(|_| !pages.is_empty()) {
        if let Some((fixed, restored)) = crate::ai::about_you::restore_price_signs(&reply, &pages, &local) {
            tracing::info!(prices = restored.len(), "Chat: Yap gave prices back the currency their pages gave them");
            crate::ai::eval::post_fix("price_sign");
            reply = fixed;
            restored_note = Some(crate::ai::about_you::restored_price_note(&restored, &local));
        }
    }
    // Prices in $ without saying whose, for someone who pays in £ (About
    // you): Yap says so under the answer (one note at most).
    if let Some(note) = crate::ai::about_you::foreign_price_note(&reply, &profile.region) {
        tracing::info!("Chat: Yap noted the answer's prices are in dollars");
        crate::ai::eval::post_fix("foreign_price_note");
        reply = format!("{}\n\n{note}", reply.trim_end());
    } else if let Some(note) = restored_note {
        reply = format!("{}\n\n{note}", reply.trim_end());
    }
    // A source of the turn's own (the window's text, read on this PC):
    // named after the answer's.
    let sources = crate::snip::turn::with_source(sources);
    // The last model call's tokens: the conversation's context use (a
    // stopped answer's call never finished, so it has none to say).
    let tokens = if stopped { None } else { crate::ai::usage::last_call().as_ref().and_then(crate::chat::chats::MessageTokens::of) };
    let index = crate::chat::chats::append_message(
        conv.id,
        "assistant",
        &reply,
        &tools_used,
        Some(answered_by.clone()),
        false,
        &sources,
        stopped,
        tokens,
    )?;
    // Read for lasting facts once the chat goes quiet ("Learn from my chats").
    crate::memory::learn::after_turn(conv.id);
    // Filling the room? The older messages summarised in the background
    // (compact.rs), so the next question never waits for it.
    let ring = tokens.map(|t| (t.prompt + t.answer) as f64 / context_size.max(1) as f64);
    let ep = crate::chat::compact::Endpoint {
        base_url: base_url.clone(),
        api_key: api_key.clone(),
        model: model.clone(),
        provider: provider.clone(),
        disable_thinking,
        by: Some(answered_by.clone()),
    };
    let look = crate::chat::compact::Look { kind, room: next_room(budget.history), context: context_size, ring, replay };
    crate::chat::compact::after_answer(app, conv.id, ep, look).await;
    Ok(ChatTurn { conversation_id: conv.id, user_index, index, reply, tools_used, model: Some(answered_by), sources, stopped, tokens })
}

/// The history's room for the next question: the budget less a typical
/// question's (a tenth of it, 100 to 1,000 tokens). Pure.
pub(crate) fn next_room(history: u64) -> u64 {
    history.saturating_sub((history / 10).clamp(100, 1_000))
}

/// What follows the persona in Chat's system prompt: the tools' instructions
/// for a model that calls them (the web's, the apps', memory's, past
/// chats', the calendar's as the turn offers them), else the plain notes
/// for one that can't act. The same from turn to turn.
fn instructions(cfg: &YapConfig, tools_supported: bool, web_on: bool, calendar_on: bool, profile: &crate::ai::about_you::Profile) -> String {
    let mut s = String::new();
    if tools_supported {
        s.push_str("\n\nYou have access to tools. ");
        s.push_str(&crate::chat::tools::TOOL_INSTRUCTIONS.join(" "));
        if cfg.open_apps {
            s.push(' ');
            s.push_str(&crate::chat::tools::APP_TOOL_INSTRUCTIONS.join(" "));
        }
        if web_on {
            s.push(' ');
            s.push_str(&crate::chat::tools::WEB_TOOL_INSTRUCTIONS.join(" "));
            s.push(' ');
            s.push_str(&crate::web::time_note(profile.region.time_zone_label.as_deref()));
            // Outside the US: prices from where they are, or one plain line
            // saying there are none.
            if let Some(local) = crate::ai::about_you::LocalPrices::for_region(&profile.region) {
                s.push(' ');
                s.push_str(&crate::web::price_note(&local));
            }
        }
        s.push(' ');
        s.push_str(&crate::chat::tools::MEMORY_TOOL_INSTRUCTIONS.join(" "));
        if cfg.memory_search_chats {
            s.push(' ');
            s.push_str(&crate::chat::tools::CHATS_TOOL_INSTRUCTIONS.join(" "));
        }
        if calendar_on {
            s.push(' ');
            s.push_str(&crate::chat::tools::calendar::CALENDAR_TOOL_INSTRUCTIONS.join(" "));
        }
    } else {
        // No tools: it can't do anything on the PC, so it mustn't say so.
        s.push_str("\n\n");
        s.push_str(crate::apps::NO_TOOLS_NOTE);
        if cfg.open_apps {
            s.push(' ');
            s.push_str(crate::apps::NO_TOOLS_OPEN);
        }
    }
    s
}

/// The end of Chat's system prompt, where small models heed it: what the
/// context block is and About you without its clock (a model that calls
/// tools) — or, its context staying in the system prompt, the whole About
/// you block (`about`) — then memory's "You".
fn ending(in_message: bool, profile: &crate::ai::about_you::Profile, about: &str, memory_you: &str) -> String {
    let mut s = String::new();
    if in_message {
        s.push_str("\n\n");
        s.push_str(crate::chat::chats::CONTEXT_NOTE);
        let profile_block = crate::ai::about_you::profile_block(profile);
        if !profile_block.is_empty() {
            s.push_str("\n\n");
            s.push_str(&profile_block);
        }
    } else {
        s.push_str("\n\n");
        s.push_str(about);
    }
    if !memory_you.is_empty() {
        s.push_str("\n\n");
        s.push_str(memory_you);
    }
    s
}

/// The part of a Chat request that's there whatever the question: the
/// persona, the instructions, the prompt's end, and (a model that calls
/// tools) the clock in the question's context. What a question must fit
/// beside (`chat_turn`'s check before it's sent, Chat's limits and ring).
pub(crate) fn stable_prompt(
    cfg: &YapConfig,
    persona: &str,
    tools_supported: bool,
    web_on: bool,
    calendar_on: bool,
    profile: &crate::ai::about_you::Profile,
    now: chrono::DateTime<chrono::Utc>,
) -> String {
    let about = crate::ai::about_you::prompt_block(profile, now);
    let mut s = persona.trim().to_string();
    s.push_str(&instructions(cfg, tools_supported, web_on, calendar_on, profile));
    s.push_str(&ending(tools_supported, profile, &about, &crate::memory::you_block()));
    if tools_supported {
        s.push_str("\n\n");
        s.push_str(&crate::ai::about_you::clock_line(profile, now));
    }
    s
}

/// What Chat's next question may be, for the model it goes to now: the
/// message box says so before sending one too long ([`chat_limits`]).
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Limits {
    /// The most a question may be, in tokens (0: no model set up).
    pub question_max: u64,
    /// The model's context.
    pub context: u64,
    pub context_known: bool,
    /// The model as a sentence names it ("Qwen3.5 4B").
    pub label: String,
    /// "That's too long for Qwen3.5 4B (64K context)."
    pub too_long: String,
    /// The on-device model could get a bigger context that fits the
    /// graphics card ("Give it more room"): its tokens.
    pub more: Option<u32>,
    /// The provider, as the network receipts name it.
    pub provider: String,
}

/// The next bigger context the on-device model could get that fits the
/// graphics card, if any ("Give it more room": `gguf.rs`' estimates
/// against the card's room). Never while a change is pending, nor on a PC
/// without a card's memory to check against.
pub(crate) fn more_room(view: &crate::ai::local_llm::context::ContextView) -> Option<u32> {
    if view.pending || !view.on_card || view.room_mib.is_none() {
        return None;
    }
    view.options.iter().find(|o| o.tokens > view.tokens && !o.over && o.estimate_mib.is_some()).map(|o| o.tokens)
}

/// Chat's limits for its model now ([`Limits`]): what a question may be,
/// the model's context and, for the on-device model, more room it could
/// get. The Chat view asks on opening and when the model or its context
/// changes.
#[tauri::command]
pub async fn chat_limits() -> Limits {
    let cfg = config::load();
    let Ok((base_url, _key, model, provider, persona, _thinking)) = resolve_chat_endpoint(&cfg) else {
        return Limits::default();
    };
    let tools_supported = crate::chat::tools::supports_tools(&provider, &model);
    let web_on = cfg.web_search;
    let calendar_on = crate::calendar::connected();
    let counter = crate::chat::budget::Counter::for_call(&provider, &base_url);
    let (context, context_known) = crate::chat::budget::context_window(&cfg, &provider, &model, &base_url).await;
    let profile = crate::ai::about_you::profile(&cfg);
    let stable = stable_prompt(&cfg, &persona, tools_supported, web_on, calendar_on, &profile, crate::ai::eval::now());
    let log = crate::chat::tools::calendar::Log::default();
    let extra = crate::chat::tools::Extra {
        memory: true,
        search_chats: cfg.memory_search_chats,
        conversation: None,
        calendar: calendar_on.then_some(crate::chat::tools::CalendarTurn { log: &log, zone: crate::chat::tools::calendar::Zone::of(None), now: 0 }),
        room: None,
    };
    let schemas = if tools_supported { counter.count(&crate::chat::tools::offered_tools(cfg.open_apps, web_on, extra).to_string()).await } else { 0 };
    let fixed = counter.count(&stable).await + schemas;
    let budget = crate::chat::budget::budget(context, context_known, fixed, crate::chat::budget::Kind::Chat, tools_supported, crate::chat::budget::rate_cap(&provider));
    let label = crate::chat::budget::model_label(&provider, &model, &crate::ai::local_llm::active_model_display(&cfg));
    let more = (provider == crate::ai::local_llm::PROVIDER_ONDEVICE).then(|| more_room(&crate::ai::local_llm::context::context_view(&cfg))).flatten();
    Limits {
        question_max: budget.history,
        context,
        context_known,
        too_long: crate::chat::budget::too_long(&label, context),
        label,
        more,
        provider: crate::privacy::net::provider_name(&provider).to_string(),
    }
}

/// The fixed web pipeline's budgets for a model that can't call tools (an
/// 8k on-device model): the web passages, and the history kept beside them.
const SMALL_WEB_TOKENS: usize = 1_500;
const SMALL_HISTORY_TOKENS: usize = 2_000;

/// A turn for a model that can't call tools (the on-device Qwen2.5-1.5B, a
/// local model under 4B): plain chat over the notes — or, with web search
/// on and a question that needs it (`web::needs_web`), **Yap's fixed
/// pipeline**: it searches (the keyed service, else Wikipedia and game
/// wikis), puts the best few passages in the prompt, numbered, and asks the
/// model to answer only from them and cite them. It never lets the model
/// claim a search that didn't happen: it's told so (and `chat_turn`'s
/// checks flag an answer that claims one anyway). Also returns whether a
/// search ran. `about`: the About you block (`about_you::prompt_block`),
/// already last in `system`, kept last in the web pipeline's own prompt.
/// (These models keep the whole context in the system prompt, `chat_turn`.)
/// `live`: streamed to the page (its search as a chip, the answer as it's
/// written); `stop` ends the answer where it is (`watch` has what showed).
#[allow(clippy::too_many_arguments)]
async fn small_model_answer(
    cfg: &YapConfig,
    persona: &str,
    system: &str,
    about: &str,
    history: &[(String, String)],
    text: &str,
    (base_url, api_key, model, provider, disable_thinking): (&str, &str, &str, &str, bool),
    web_on: bool,
    by_voice: bool,
    live: Option<crate::chat::stream::Live<'_>>,
    stop: Option<&crate::chat::stream::Stoppable>,
    watch: Option<&crate::chat::stream::Watch<'_>>,
) -> Result<SmallAnswer, String> {
    let mut chips = Vec::new();
    let mut sources = Vec::new();
    let mut pages = Vec::new();
    let mut system = system.to_string();
    let mut history = history.to_vec();
    let looked = web_on && crate::web::needs_web(text);
    if looked {
        let settings = crate::web::Settings::for_chat(cfg, provider, model, base_url, api_key);
        // A price question's search names the person's country ("… UK").
        let query = crate::web::search_query(text);
        let query = crate::web::local_query(&query, text, settings.local.as_ref()).unwrap_or(query);
        let endpoints = crate::web::Endpoints::current();
        if let Some(live) = live {
            live.sink.send(crate::chat::stream::Event::Tool { name: "web_search".into(), doing: crate::chat::stream::doing("web_search").into() });
        }
        match crate::web::search(&settings, &endpoints, &query, text).await {
            Ok(mut found) if found.hits.iter().any(|h| !h.text.trim().is_empty()) => {
                chips.push(crate::web::search_chip(&query, &found, &settings));
                // Only another country's prices: once more for theirs.
                let price_line = match settings.local.as_ref() {
                    Some(local) if local.tells_apart && crate::web::asks_about_prices(text) => {
                        local_prices_again(&settings, &endpoints, local, &query, text, &mut found, &mut chips).await
                    }
                    _ => None,
                };
                let (found_sources, block) = crate::web::grounded_sources(&found.hits, SMALL_WEB_TOKENS);
                // Temperatures, speeds and distances in the person's units
                // first, with a line saying so for each (about_you.rs,
                // about_you/units.rs).
                let units = crate::ai::about_you::units::TheirUnits::for_question(settings.units, text);
                let unit_lines = units.notes(&block);
                let block = units.convert(&block).unwrap_or(block);
                system = crate::web::grounded_prompt(persona, &block, &crate::web::today(), by_voice);
                pages.push(block);
                system.push(' ');
                let zone = crate::ai::about_you::profile(cfg).region.time_zone_label;
                system.push_str(&crate::web::time_note(zone.as_deref()));
                if let Some(line) = price_line {
                    system.push(' ');
                    system.push_str(&line);
                }
                for (_, line) in unit_lines {
                    system.push(' ');
                    system.push_str(&line);
                }
                // Still only text: it can't open or change anything (apps.rs).
                system.push_str("\n\n");
                system.push_str(crate::apps::NO_TOOLS_NOTE);
                system.push_str("\n\n");
                system.push_str(about);
                history = crate::web::fit_history(&history, SMALL_HISTORY_TOKENS);
                chips.push(match found_sources.len() {
                    1 => "Answered from 1 web page".to_string(),
                    n => format!("Answered from {n} web pages"),
                });
                sources = found_sources;
            }
            Ok(found) => {
                chips.push(crate::web::search_chip(&query, &found, &settings));
                system.push_str(&format!("\n\n{}", crate::web::no_search_line(Some("the search found nothing for it"))));
            }
            Err(e) => {
                tracing::info!("Chat: the web search for a small model failed");
                chips.push(format!("Couldn't search the web: {}", crate::web::short(&e)));
                system.push_str(&format!("\n\n{}", crate::web::no_search_line(Some(&e))));
            }
        }
    } else if web_on {
        system.push_str(&format!("\n\n{}", crate::web::NOT_SEARCHED_LINE));
    } else {
        system.push_str(&format!("\n\n{}", crate::web::no_search_line(None)));
    }
    if let Some(live) = live {
        live.sink.send(crate::chat::stream::Event::Chips { chips: chips.clone(), sources: sources.clone(), web: looked });
    }
    let answered = crate::privacy::net::within(
        crate::privacy::net::Feature::Chat,
        crate::ai::llm::note_chat(&system, &history, text, base_url, api_key, model, provider, disable_thinking, live),
    );
    let (reply, stopped) = match crate::chat::stream::unless_stopped(answered, stop).await {
        Ok(reply) => (reply?, false),
        Err(crate::chat::stream::Halt::Superseded) => return Err(superseded()),
        Err(crate::chat::stream::Halt::Stopped) => (watch.map(|w| w.seen().text).unwrap_or_default(), true),
    };
    Ok(SmallAnswer { reply, chips, sources, looked, pages, stopped })
}

/// A turn a newer question in its conversation replaced (`chat_turn`): no
/// answer saved, the question kept; [`crate::chat::stream::SUPERSEDED`].
fn superseded() -> String {
    tracing::info!("Chat: a newer question in the conversation replaced this turn; no answer saved");
    crate::chat::stream::SUPERSEDED.to_string()
}

/// A small model's answer ([`small_model_answer`]): the text, its chips and
/// sources, whether a search ran, and the web text it was given.
struct SmallAnswer {
    reply: String,
    chips: Vec<String>,
    sources: Vec<crate::chat::chats::Source>,
    looked: bool,
    pages: Vec<String>,
    /// Stopped while it was written (`reply` = what showed).
    stopped: bool,
}

/// The fixed web pipeline's second search for a price where the person is
/// (`about_you::LocalPrices`), when the first results only gave another
/// country's: "… UK £". Their prices found: those pages first. None again:
/// the others stay, and the answer is told to say so in one line. Returns
/// the prompt's line on prices, if any. (Its chip goes in `chips`.)
async fn local_prices_again(
    settings: &crate::web::Settings,
    endpoints: &crate::web::Endpoints,
    local: &crate::ai::about_you::LocalPrices,
    query: &str,
    question: &str,
    found: &mut crate::web::Found,
    chips: &mut Vec<String>,
) -> Option<String> {
    let text_of = |f: &crate::web::Found| f.hits.iter().map(|h| format!("{}\n{}", h.title, h.text)).collect::<Vec<_>>().join("\n");
    let first = text_of(found);
    if local.local_in(&first) || !local.foreign_in(&first) {
        return None;
    }
    let (p, s) = (&local.place, local.sign());
    let none = format!(
        "The sources have no {p} prices (in {s}): start your answer with one short line saying you couldn't find {p} prices, then give the sources' prices as they are, saying which country they're for. Don't offer to look further."
    );
    let retry = crate::web::local_retry_query(query, local);
    crate::ai::eval::post_fix("yap_search");
    let more = match crate::web::search(settings, endpoints, &retry, question).await {
        Ok(more) => more,
        Err(e) => {
            chips.push(format!("Couldn't search the web: {}", crate::web::short(&e)));
            return Some(none);
        }
    };
    chips.push(crate::web::search_chip(&retry, &more, settings));
    let theirs = local.local_in(&text_of(&more));
    // Their prices' pages first (the prompt keeps the first few), else the
    // first search's.
    let (mut hits, rest) = if theirs { (more.hits, std::mem::take(&mut found.hits)) } else { (std::mem::take(&mut found.hits), more.hits) };
    for h in rest {
        if !hits.iter().any(|x| crate::web::normal_url(&x.url) == crate::web::normal_url(&h.url)) {
            hits.push(h);
        }
    }
    found.hits = hits;
    Some(if theirs { format!("The first sources give {p} prices (in {s}): answer with those, as the sources give them.") } else { none })
}

/// How earlier web answers reach the model: `(replay, note)` — their
/// searches replayed as tool calls (a tool-calling model with the web on),
/// else the system prompt's line. A model evaluation may pick another way
/// (`YAP_EVAL_WEB_HISTORY` = replay | note | none) to compare them.
fn web_history_mode(tool_model_with_web: bool) -> (bool, bool) {
    if crate::ai::eval::active() {
        match std::env::var("YAP_EVAL_WEB_HISTORY").as_deref() {
            Ok("note") => return (false, true),
            Ok("none") => return (false, false),
            Ok("replay") => return (tool_model_with_web, true),
            _ => {}
        }
    }
    (tool_model_with_web, true)
}

/// The first step for a model on this PC when the question needs the web:
/// it must search (`tools::FirstStep::MustSearch`: its own query, written
/// with the conversation in mind — "Arsenal match tonight 21 October 2026"
/// where the question as a query found nothing useful; Yap searches for it
/// when the server won't make it). A model evaluation may compare
/// (`YAP_EVAL_FIRST` = must | yap | model).
fn first_search(question: &str) -> crate::chat::tools::FirstStep {
    if crate::ai::eval::active() {
        match std::env::var("YAP_EVAL_FIRST").as_deref() {
            Ok("yap") => return crate::chat::tools::FirstStep::YapSearches(crate::web::search_query(question)),
            Ok("model") => return crate::chat::tools::FirstStep::Model,
            _ => {}
        }
    }
    crate::chat::tools::FirstStep::MustSearch
}

/// The conversation a turn goes to: `conversation_id`'s, or a new one named
/// after the first message (first 50 chars, OpenWhispr's rule) — its own
/// words: a screenshot's text sent with it (`snip::ocr`) isn't the title.
fn turn_conversation(conversation_id: Option<u64>, text: &str) -> crate::chat::chats::Conversation {
    match conversation_id.and_then(crate::chat::chats::get) {
        Some(c) => c,
        None => {
            let text = crate::snip::ocr::question_of(text);
            let title: String = if text.chars().count() > 50 {
                format!("{}...", text.chars().take(50).collect::<String>())
            } else {
                text.to_string()
            };
            crate::chat::chats::create(&title)
        }
    }
}

/// A question asked across the meetings with no AI model to answer it (none
/// set up, or the on-device one won't start: `why`): Yap answers with the
/// passages that match, each cited so its pill opens the moment
/// (`search::ask::passages_answer`). No model, so none is named.
async fn passages_turn(
    conversation_id: Option<u64>,
    text: &str,
    by_voice: bool,
    asked: impl FnOnce(u64, usize) + Send,
    stop: &crate::chat::stream::Stoppable,
    why: &str,
) -> Result<ChatTurn, String> {
    tracing::info!(reason = %why.chars().take(80).collect::<String>(), "Chat: no model to answer across the meetings; showing the passages");
    let conv = turn_conversation(conversation_id, text);
    if conv.scope.is_empty() {
        crate::chat::chats::set_scope(conv.id, "meetings");
    }
    let user_index = crate::chat::chats::append(conv.id, "user", text, &[], None, by_voice)?;
    stop.answers(conv.id);
    asked(conv.id, user_index);
    let found = crate::search::ask::find(text, crate::search::ask::ASK_PASSAGES).await;
    let reply = crate::search::ask::passages_answer(&found);
    let sources = crate::search::ask::sources(&found);
    let tools_used = vec![crate::search::ask::chip(&found)];
    let index = crate::chat::chats::append_message(conv.id, "assistant", &reply, &tools_used, None, false, &sources, false, None)?;
    Ok(ChatTurn { conversation_id: conv.id, user_index, index, reply, tools_used, model: None, sources, stopped: false, tokens: None })
}

/// A turn Yap answers itself (`apps.rs`): the request saved like any
/// question, the app opened (or why not) as the answer, with its chip. No
/// model, so none is named. Like any newer question, it replaces an answer
/// still coming in its conversation (`stop`, [`chat_turn`]).
async fn app_turn(
    conversation_id: Option<u64>,
    text: &str,
    by_voice: bool,
    asked: impl FnOnce(u64, usize) + Send,
    plan: crate::apps::Plan,
    stop: &crate::chat::stream::Stoppable,
) -> Result<ChatTurn, String> {
    let conv = turn_conversation(conversation_id, text);
    let user_index = crate::chat::chats::append(conv.id, "user", text, &[], None, by_voice)?;
    stop.answers(conv.id);
    asked(conv.id, user_index);
    // What its answer leaves open: "Did you mean …?", "Which one?", the app
    // "open it again" means.
    crate::apps::remember(conv.id, &plan);
    let answer = tauri::async_runtime::spawn_blocking(move || crate::apps::carry_out(&plan))
        .await
        .map_err(|e| e.to_string())?;
    let tools_used: Vec<String> = Some(answer.chip).filter(|c| !c.is_empty()).into_iter().collect();
    let index = crate::chat::chats::append(conv.id, "assistant", &answer.reply, &tools_used, None, false)?;
    Ok(ChatTurn {
        conversation_id: conv.id,
        user_index,
        index,
        reply: answer.reply,
        tools_used,
        model: None,
        sources: Vec::new(),
        stopped: false,
        tokens: None,
    })
}

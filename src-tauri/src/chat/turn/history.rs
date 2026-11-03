//! The turn sized to the model (`chat/budget.rs`): the question measured
//! against what the model can take before anything's saved, and the
//! history fitted to what's left — every message, or the summary of the
//! older ones and those after it, or the latest that fit.

use super::prompt::Built;
use super::Asked;

/// **What a turn's requests must fit** ([`Sizing::measure`]): the model's
/// context, how the history is chosen, the tools' schemas and the question
/// itself, measured before the question is saved.
pub(super) struct Sizing {
    /// Chat, the meetings, or the Voice Assistant (`budget::Kind`).
    pub(super) kind: crate::chat::budget::Kind,
    /// How the history is chosen (`budget::mode`): every message, a
    /// summary, the latest that fit.
    pub(super) mode: crate::chat::budget::Mode,
    pub(super) counter: crate::chat::budget::Counter,
    /// The model's context, and whether the model or its server said so.
    pub(super) context_size: u64,
    pub(super) context_known: bool,
    /// The model as a sentence names it ("Qwen3.5 4B").
    label: String,
    /// The tools' schemas every step sends (`gates::Gates::schemas`).
    pub(super) schemas: u64,
    /// The question as sent: its text, and the screenshots it carries.
    pub(super) question_tokens: u64,
}

impl Sizing {
    /// **Sized to the model** (budget.rs): its context, measured parts, and
    /// the question alone within what's left — a paste too long for the
    /// model is never sent (nor saved): it's said plainly, the error.
    /// `conversation_id`: the one asked in, as the turn was given it.
    pub(super) async fn measure(t: &Asked<'_>, conversation_id: Option<u64>) -> Result<Sizing, String> {
        let (cfg, provider, model, base_url) = (&t.cfg, t.ep.provider.as_str(), t.ep.model.as_str(), t.ep.base_url.as_str());
        let kind = crate::chat::budget::Kind::of_scope(t.scope);
        let mode = crate::chat::budget::mode();
        let counter = crate::chat::budget::Counter::for_call(provider, base_url);
        let (context_size, context_known) = crate::chat::budget::context_window(cfg, provider, model, base_url).await;
        let label = crate::chat::budget::model_label(provider, model, &crate::ai::local_llm::active_model_display(cfg));
        let schemas = t.gates.schemas(cfg, &counter, conversation_id).await;
        // The question as sent: its text, and the screenshots it carries, each
        // by its size where it goes (`budget::image_tokens_at`).
        let question_tokens = counter.count(&t.text).await
            + crate::chat::budget::MESSAGE_OVERHEAD
            + crate::snip::turn::images().iter().filter(|i| !i.as_text).map(|i| counter.picture(i.w, i.h)).sum::<u64>();
        let rate_cap = crate::chat::budget::rate_cap(provider);
        let tools_supported = t.gates.tools;
        if mode != crate::chat::budget::Mode::Messages {
            let stable = t.gates.stable_prompt(cfg, &t.ep.persona, &t.profile, t.now);
            let fixed_now = counter.count(&stable).await + schemas;
            let guess = crate::chat::budget::budget(context_size, context_known, fixed_now, kind, tools_supported, rate_cap);
            if question_tokens > guess.history {
                tracing::info!(question_tokens, room = guess.history, context = context_size, "Chat: the question is too long for the model; not sent");
                // The plan's tokens a minute, not the model's context, when
                // that's what's short (it read "too long for … (128K context)").
                let roomy = crate::chat::budget::budget(context_size, context_known, fixed_now, kind, tools_supported, None);
                if let Some(tpm) = crate::chat::budget::rate_limit(provider).filter(|_| question_tokens <= roomy.history) {
                    return Err(crate::chat::budget::too_much_a_minute(&label, provider, tpm));
                }
                return Err(crate::chat::budget::too_long(&label, context_size));
            }
        }
        Ok(Sizing { kind, mode, counter, context_size, context_known, label, schemas, question_tokens })
    }

    /// "Too long for the model" said plainly, never the provider's JSON
    /// (`budget::plain_error`).
    pub(super) fn plain(&self, e: String, provider: &str) -> String {
        crate::chat::budget::plain_error(e, &self.label, self.context_size, provider)
    }
}

/// What [`fit`] chose: the budget the turn's requests are held to, where
/// the history sent starts in the conversation, and that history.
pub(super) struct Fitted {
    pub(super) budget: crate::chat::budget::Budget,
    /// The history sent is `conversation.messages[start..]`.
    pub(super) start: usize,
    /// The history sent, as (role, text).
    pub(super) history: Vec<(String, String)>,
}

/// **The history, sized to the model** (budget.rs): the parts measured
/// — the system prompt, the context block, the tools' schemas — and the
/// history in what's left of the budget: every message when they fit;
/// else the summary of the older ones (compact.rs) and those after it;
/// else the latest that fit (and a summary written after the answer).
/// The summary and the notes about earlier answers go into the prompt
/// (`built`), where it keeps room for them.
pub(super) async fn fit(t: &Asked<'_>, sizing: &Sizing, conv: &crate::chat::chats::Conversation, built: &mut Built) -> Fitted {
    let counter = &sizing.counter;
    let (kind, mode, context_size, context_known) = (sizing.kind, sizing.mode, sizing.context_size, sizing.context_known);
    let replay = built.replay;
    let prompt = &mut built.prompt;
    let fixed = counter.count(&prompt.system).await
        + crate::chat::chats::context_block(&prompt.context).map(|b| crate::chat::budget::estimate(&b)).unwrap_or(0)
        + sizing.schemas
        + crate::chat::budget::MESSAGE_OVERHEAD;
    // Sized within the model's cheaper rate too (`request_cap`): a long
    // chat's summary takes over before a request would cost more.
    let budget = crate::chat::budget::budget(context_size, context_known, fixed, kind, t.gates.tools, crate::chat::budget::request_cap(&t.ep.provider, &t.ep.model));
    let room = budget.history.saturating_sub(sizing.question_tokens);
    let summary = conv.usable_summary().filter(|_| mode == crate::chat::budget::Mode::Summary);
    let plan = match mode {
        crate::chat::budget::Mode::Messages => {
            let (keep, step) = if t.scope == Some("assistant") { crate::chat::chats::ASSISTANT_HISTORY } else { crate::chat::chats::CHAT_HISTORY };
            let start = crate::chat::chats::history_start(conv.messages.len(), keep, step);
            crate::chat::budget::Plan { start, summary: false, left_out: start, tokens: 0 }
        }
        _ => {
            let costs = crate::chat::budget::message_costs(counter, &conv.messages, replay).await;
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
        prompt.system.insert_str(prompt.summary_at, &format!("\n\n{}", crate::chat::compact::block(&s.text)));
    }
    if built.history_note && !replay {
        if let Some(note) = crate::chat::chats::web_history_note(earlier) {
            built.prompt.add_early(note);
        }
    }
    // Answers another model wrote (a switch in the picker): said so, so
    // this one never claims them (`chats::other_models_note`) — those
    // summarised or left out too.
    if let Some(note) = crate::chat::chats::other_models_note(&conv.messages, &t.answered_by, plan.summary) {
        built.prompt.add_early(note);
    }
    let history: Vec<(String, String)> = earlier.iter().map(|m| (m.role.clone(), m.sent_text())).collect();
    Fitted { budget, start: plan.start, history }
}

/// The history's room for the next question: the budget less a typical
/// question's (a tenth of it, 100 to 1,000 tokens). Pure.
pub(crate) fn next_room(history: u64) -> u64 {
    history.saturating_sub((history / 10).clamp(100, 1_000))
}

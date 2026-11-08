//! One chat turn, answered: the Chat scope's endpoint, the prompt (the
//! notes the eager keyword-RAG finds, meetings, memory, About you), the
//! tool loop, or a small model's fixed web pipeline, and the checks on
//! every answer. The Chat view (`chat_send`), the Voice Assistant and the
//! model evaluations all ask through [`chat_turn`].
//!
//! Here: the turn's steps in their order ([`answer_turn`]), what they share
//! ([`Asked`], [`Opened`], [`Answer`]), the Chat model's endpoint and its
//! limits. The steps are its children, in the order a turn meets them:
//! `fast` (the turns Yap answers itself, no model: an app, a page, a
//! Skill's question, the passages), `gates` (what this model may do in this
//! turn: tools, the web and whose search, pictures, the screen, pages, the
//! calendar), `history` (the question measured against the model, and the
//! history fitted to what's left), `prompt` (the system prompt and the
//! question's context: the eager RAG, the meetings' passages, memory, About
//! you, how to answer), `run` (a model that calls tools: the tool loop),
//! `small` (one that can't: plain chat, or Yap's fixed web pipeline) and
//! `finish` (the checks on every answer, and saving it).

mod fast;
mod finish;
mod fit;
mod gates;
mod history;
mod plan;
mod prompt;
mod report;
mod run;
mod small;

use tauri::AppHandle;

use crate::config::{self, YapConfig};

pub(crate) use history::next_room;
// An agent's report, a turn Yap starts itself (report.rs).
pub(crate) use report::{open_agent, queue_agent_report, AGENT_REPORT, AGENT_REPORT_PREFIX, EVENT as AGENT_REPORT_EVENT};

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
            // Your Claude plan: Chat's calls stay Chat's (no spare, no
            // fallback to the key), on the cleanup's model or not.
            let b = crate::ai::claude_plan::tag(&p, b, crate::ai::claude_plan::Scope::Chat);
            (b, k, m, p, persona, cfg.pp_disable_thinking)
        }
    };
    if base_url.is_empty() {
        return Err(
            "No AI model configured — set one in Settings → Language Models → Chat".to_string(),
        );
    }
    if api_key.is_empty() && provider == crate::ai::chatgpt::PROVIDER {
        return Err("Yap isn't signed in to ChatGPT — sign in in Settings → Language Models → ChatGPT".to_string());
    }
    if api_key.is_empty() && provider == crate::ai::claude_plan::PROVIDER {
        return Err(crate::ai::claude_plan::NOT_READY.to_string());
    }
    if api_key.is_empty() && crate::ai::ai_model::KEYED_PROVIDERS.contains(&provider.as_str()) {
        return Err(format!(
            "No {provider} API key — add one in Settings → Language Models"
        ));
    }
    Ok((base_url, api_key, model, provider, persona, disable_thinking))
}

/// The Chat scope's endpoint and persona ([`resolve_chat_endpoint`]), as a
/// turn uses them: the persona with the line naming the model that answers
/// (`ai_model::answering_line`), so every prompt a turn writes — the tool
/// loop's, a small model's and its web pipeline's — and what it's measured
/// by start with it.
struct Endpoint {
    base_url: String,
    api_key: String,
    model: String,
    provider: String,
    persona: String,
    disable_thinking: bool,
}

impl Endpoint {
    /// [`resolve_chat_endpoint`]'s, named.
    fn resolve(cfg: &YapConfig) -> Result<Endpoint, String> {
        let (base_url, api_key, model, provider, persona, disable_thinking) = resolve_chat_endpoint(cfg)?;
        let by = crate::ai::ai_model::who_answers(cfg, &provider, &model, &base_url);
        let persona = crate::ai::ai_model::persona_with_answering(&persona, &by);
        Ok(Endpoint { base_url, api_key, model, provider, persona, disable_thinking })
    }
}

/// **Does Claude search in Chat's own request?** (llm/anthropic.rs, web/
/// native.rs `in_chat`): web search on, a strong Claude on Anthropic, and
/// "The AI's own search" chosen and usable for it (`web::Settings::for_chat`
/// gives its provider's search). Else Yap's `web_search` and `read_page`.
pub(crate) fn in_chat(cfg: &YapConfig, provider: &str, model: &str, base_url: &str, api_key: &str) -> bool {
    cfg.web_search
        && crate::ai::llm::anthropic::speaks_messages(provider, model)
        && crate::web::Settings::for_chat(cfg, provider, model, base_url, api_key).native.is_some()
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
    /// The documents it made or changed (`documents.rs`): its pills.
    pub documents: Vec<crate::chat::documents::DocRef>,
    /// The pictures it showed beside the chat (`chat/pictures.rs`): their pills.
    pub pictures: Vec<crate::chat::pictures::ShownPicture>,
    /// The pages it brought up beside the chat (`chat/pages.rs`): their tabs.
    pub pages: Vec<crate::chat::pages::ShownPage>,
    /// The buttons it offered for the person's browser (`chat/pages.rs`).
    pub offers: Vec<crate::chat::pages::BrowserOffer>,
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
    // Chat turn inline overflowed the main thread's stack). Its note changes
    // are one change in their versions (`versions::in_turn`: the document
    // panel's Undo takes the whole turn back).
    let turn = Box::pin(answer_turn(app, conversation_id, text, scope, by_voice, asked, watcher, stop));
    crate::ai::usage::tallied(crate::notes::versions::in_turn(turn)).await
}

/// **The question being answered** and what's settled about its turn
/// before the model is asked: where it was asked, the settings, who
/// answers and what that model may do. Every step of [`answer_turn`]
/// reads it.
struct Asked<'a> {
    /// The windows told of note changes (`None` in a model evaluation).
    app: Option<&'a AppHandle>,
    cfg: YapConfig,
    /// The question, trimmed.
    text: String,
    /// `None`: Chat; "meetings": the Meetings view's Ask bar (or a
    /// conversation asked there); "assistant": the Voice Assistant.
    scope: Option<&'a str>,
    by_voice: bool,
    /// A Skill being run (chat/skills.rs).
    skill: Option<crate::chat::skills::SkillTurn>,
    /// Asked across the meetings: the Meetings view's Ask bar, or a Skill.
    meetings: bool,
    /// Text selected in another app that this question is about
    /// (`assistant/selected.rs`).
    about_selection: Option<crate::chat::assistant::selected::Selected>,
    ep: Endpoint,
    /// Who answers, as the chip named it, kept with the answer.
    answered_by: crate::chat::chats::ChatModel,
    /// What this model may do in this turn (gates.rs).
    gates: gates::Gates,
    /// About you (about_you.rs): where the person is, their money and
    /// units, name and note; and the clock.
    profile: crate::ai::about_you::Profile,
    now: chrono::DateTime<chrono::Utc>,
    /// Whoever watches the stream (the Chat view's channel, the Voice
    /// Assistant, an evaluation's clock).
    watcher: Option<&'a dyn crate::chat::stream::Sink>,
    /// The page's ■, or the turn's own: a newer question ends it too.
    stop: &'a crate::chat::stream::Stoppable,
}

/// The question saved ([`open`]): its conversation as it was before it,
/// the question's index there, a passage selected in the document panel
/// that it's about ("Change this…", documents.rs), and the workspace when
/// it's a Code chat.
struct Opened {
    conv: crate::chat::chats::Conversation,
    user_index: usize,
    selection: Option<crate::chat::documents::Selection>,
    /// **A Code chat's workspace** (`agents/code_chat.rs`), read once the
    /// question is saved (a new code chat is attached as it's made): no
    /// Yap memory in its turn, About you without its note, nothing found
    /// by itself (prompt.rs), never learned from (memory/learn.rs), and on
    /// the Claude plan a session in the workspace (plan.rs).
    code: Option<crate::agents::workspaces::Folder>,
}

/// **The answer as the model gave it** (`run.rs`, `small.rs`), before
/// Yap's checks (`finish.rs`): its text and chips, the web's sources and
/// the text of its pages, whether a search ran, and what it showed beside
/// the chat.
struct Answer {
    reply: String,
    tools_used: Vec<String>,
    sources: Vec<crate::chat::chats::Source>,
    /// A search ran this turn.
    looked: bool,
    /// The web text it was given (an answer's prices are checked against it).
    pages: Vec<String>,
    /// Stopped while it was written (`reply` = what showed).
    stopped: bool,
    /// The pictures it showed beside the chat (`chat/pictures.rs`).
    pictures: Vec<crate::chat::pictures::ShownPicture>,
    /// The pages it brought up beside the chat, and the buttons it offered
    /// for the person's browser (`chat/pages.rs`).
    shown_pages: Vec<crate::chat::pages::ShownPage>,
    offers: Vec<crate::chat::pages::BrowserOffer>,
    /// "Search in your browser" under the answer: Yap's free web sources
    /// couldn't give it (no search service for the turn, web/sources.rs).
    browser: Option<crate::web::BrowserSearch>,
}

/// [`chat_turn`], its last model call kept by `usage::tallied`: the turns
/// Yap answers itself first (`fast.rs`), then — a model to ask — what it
/// may do (`gates.rs`), the question measured and saved, the prompt
/// (`prompt.rs`) and the history fitted to the model (`history.rs`), the
/// answer by the tool loop or a small model's way (`run.rs`, `small.rs`),
/// checked and saved (`finish.rs`).
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
    // A Skill (chat/skills.rs) is asked across the meetings, wherever it's run.
    let skill = crate::chat::skills::current();
    // One with nothing to run on (/project-update and no project named,
    // /prep and nothing coming up): Yap asks for it itself, no model — out
    // loud for the Voice Assistant, its answer said (assistant/skill.rs).
    let aloud = scope == Some("assistant");
    if let Some((s, said)) = skill.as_ref().and_then(|s| crate::chat::skills::ask_first(s, aloud).map(|a| (s, a))) {
        return fast::skill_asks_turn(conversation_id, &text, by_voice, asked, stop, &said, &s.skill.command);
    }
    // Yap's spoken question before an agent ("Should I start an agent in …?"),
    // answered: a yes starts it, a no drops it — Yap's own answer, no model
    // (agents/voice.rs).
    if skill.is_none() {
        if let Some(answer) = crate::agents::voice::answered(conversation_id, &text) {
            return fast::agent_answer_turn(conversation_id, &text, by_voice, asked, answer, watcher, stop);
        }
    }
    let meetings = scope == Some("meetings") || skill.is_some();
    // Chat itself, or the Voice Assistant asked with Chat in view: the page
    // beside the chat is right there, so pages, the screen look and Yap's
    // own opening of a page are theirs (the maintainer asked out loud with
    // Chat open — "open Amazon", "have a look" — and got neither, 1 Nov
    // 2026). Never a phone's question, nor one asked from another app.
    let chat_in_view = scope.is_none() || (scope == Some("assistant") && crate::chat::assistant::asked_in_chat());
    let cfg = config::load();
    // Text selected in another app that this question is about
    // (`assistant/selected.rs`): the selection is what it's about — never
    // an app to open, nor a search forced first.
    let about_selection = crate::chat::assistant::selected::current();
    // (A Skill said aloud — "run my standup skill" — is never an app to open.)
    if about_selection.is_none() && skill.is_none() {
        if let Some(plan) = crate::apps::plan_for(&text, &cfg, conversation_id).await {
            return fast::app_turn(conversation_id, &text, by_voice, asked, plan, stop).await;
        }
        // A site asked for by name, or a search asked for, with pages beside
        // the chat (the web on): Yap opens it there itself, no model.
        if let Some(opened) = fast::page_asked(&cfg, &text, chat_in_view) {
            return fast::page_turn(conversation_id, &text, by_voice, asked, opened, watcher, stop);
        }
    }
    let endpoint = match crate::ai::ai_model::wake_on_device(&cfg).await {
        Ok(_) => Endpoint::resolve(&cfg),
        Err(e) => Err(e),
    };
    let ep = match endpoint {
        Ok(e) => e,
        // Asked across the meetings with no model to answer: the passages
        // that match, themselves.
        // (Boxed, as the search below: the turn's own future stays small.)
        Err(why) if meetings => return Box::pin(fast::passages_turn(conversation_id, &text, by_voice, asked, stop, &why)).await,
        Err(why) => return Err(why),
    };
    // The on-device model is busy for the whole turn — between its tool
    // steps too, while a web search or a page read runs — so a context
    // restart or a runtime swap never cuts the answer off.
    let _sidecar = crate::ai::local_llm::hold(&ep.base_url);
    // Who answers, as the chip named it, kept with the answer.
    let answered_by = crate::ai::ai_model::describe(
        &ep.provider,
        &ep.model,
        &ep.base_url,
        &crate::ai::local_llm::active_model_display(&cfg),
    )
    .chat_model();

    // A summary being written on this PC steps aside for the question.
    crate::chat::compact::turn_started();
    // What this model may do in this turn (gates.rs).
    let mut gates = gates::Gates::for_turn(&cfg, &ep, scope, meetings, chat_in_view);
    // The connectors this chat switched off in its + menu (Connectors ▸,
    // `connectors/chats.rs`): none left on, none offered — nor Yap's line
    // about them in the prompt.
    gates.connectors = gates.connectors && crate::chat::tools::connectors::offers_any_in(&cfg, scope, conversation_id);
    // About you (about_you.rs): where the person is, their money and units,
    // name and note — last in the system prompt, below; their clock in the
    // question's context (it changes every minute).
    let profile = crate::ai::about_you::profile(&cfg);
    let now = crate::ai::eval::now();
    let mut t = Asked { app, cfg, text, scope, by_voice, skill, meetings, about_selection, ep, answered_by, gates, profile, now, watcher, stop };

    // **Sized to the model** (history.rs): a question too long for it is
    // never sent (nor saved): it's said plainly. A model whose context is
    // short goes without some of Yap's tools this turn (fit.rs): the gates
    // the prompt and the tools follow from here on.
    let (sizing, fitted) = history::Sizing::measure(&t, conversation_id).await?;
    t.gates = fitted;
    let opened = open(&t, conversation_id, asked)?;
    if let Some(turn) = remembered(&t, &opened).await? {
        return Ok(turn);
    }
    // The system prompt and the question's context (prompt.rs), then the
    // history in what's left (history.rs).
    let mut built = prompt::build(&t, &sizing, &opened).await;
    let fitted = history::fit(&t, &sizing, &opened.conv, &mut built).await;

    // Streamed to whoever watches: what was shown is kept (a Stop keeps
    // it), and a model on this PC steps aside for a new dictation.
    let watch = t.watcher.map(crate::chat::stream::Watch::new);
    let beside = t
        .app
        .filter(|_| crate::ai::ai_model::runs_here(&t.ep.provider, &t.ep.base_url))
        .map(|a| crate::chat::stream::NewDictations(a.clone()));
    let live = watch.as_ref().map(|w| crate::chat::stream::Live {
        sink: w,
        beside: beside.as_ref().map(|b| b as &dyn crate::chat::stream::Dictations),
    });
    let answer = if t.ep.provider == crate::ai::claude_plan::PROVIDER {
        // Your Claude plan: the person's own Claude Code answers (plan.rs).
        plan::answer(&t, &opened, &built, &fitted, watch.as_ref()).await?
    } else if t.gates.tools {
        run::with_tools(&t, &opened.conv, &built, &fitted, &sizing, live, watch.as_ref()).await?
    } else {
        // Small/local models: plain chat + eager RAG, or Yap's fixed web
        // pipeline when the question needs the web (small.rs).
        small::answer(&t, &built, &fitted, &sizing, live, watch.as_ref()).await?
    };
    let answer = finish::check(&t, answer, &opened.conv, &built)?;
    finish::save(&t, &opened, answer, &built, &fitted, &sizing, watch.as_ref()).await
}

/// **The question saved** in its conversation (`conversation_id`'s, or a
/// new one on the first message, OpenWhispr-style), with the screenshots
/// it carries and the quote it's about; from then on this turn is the one
/// answering there, and `asked` hears where (before the model is called).
fn open(t: &Asked<'_>, conversation_id: Option<u64>, asked: impl FnOnce(u64, usize)) -> Result<Opened, String> {
    // Conversation bookkeeping (create on first message, OpenWhispr-style).
    let conv = turn_conversation(conversation_id, &t.text);
    // (A Skill run in a chat that's about something else leaves it so.)
    if t.meetings && conv.scope.is_empty() && (t.skill.is_none() || conv.messages.is_empty()) {
        crate::chat::chats::set_scope(conv.id, "meetings");
    }
    // The model answers this one: Yap's own question about apps has lapsed.
    crate::apps::moved_on(Some(conv.id));
    // The question, with the screenshots it carries (Snip to Chat, the
    // turn's `snip::turn`): kept with it in chats.json as ids.
    let user_index = crate::chat::chats::append_question(conv.id, &t.text, t.by_voice, &crate::snip::turn::images())?;
    // A passage selected in the document panel ("Change this…", documents.rs):
    // kept with the question, its quote above it.
    let selection = crate::chat::documents::selection();
    if let Some(quote) = selection.as_ref().and_then(crate::chat::documents::Selection::quote) {
        crate::chat::chats::set_quote(conv.id, user_index, quote);
    } else if let Some(quote) = t.about_selection.as_ref().and_then(crate::chat::assistant::selected::quote) {
        // …or text selected in another app: its start and the app.
        crate::chat::chats::set_quote(conv.id, user_index, quote);
    }
    // This turn answers the conversation now: one still answering there
    // ends, with nothing saved.
    t.stop.answers(conv.id);
    asked(conv.id, user_index);
    // (After `asked`: a new code chat's conversation is attached there.)
    let code = crate::agents::code_chat::folder_of(Some(conv.id));
    Ok(Opened { conv, user_index, selection, code })
}

/// **"Remember that…" / "Forget that…" to a model without tools**: Yap's
/// own memory pass makes the change (memory/learn.rs), and says so — the
/// turn's answer, saved. A model with tools does it with `remember` /
/// `forget`. `None`: not one of those — and never in a Code chat, which
/// keeps Yap's memory out (the words go to the model as they are).
async fn remembered(t: &Asked<'_>, opened: &Opened) -> Result<Option<ChatTurn>, String> {
    // (Your Claude plan's tools only read: Yap's own pass remembers for it.)
    if opened.code.is_some() || t.gates.tools && t.ep.provider != crate::ai::claude_plan::PROVIDER {
        return Ok(None);
    }
    let Some(done) = crate::memory::learn::spoken(&t.text).await else {
        return Ok(None);
    };
    let (reply, chips) = done?;
    let index = crate::chat::chats::append(opened.conv.id, "assistant", &reply, &chips, Some(t.answered_by.clone()), false)?;
    Ok(Some(ChatTurn {
        conversation_id: opened.conv.id,
        user_index: opened.user_index,
        index,
        reply,
        tools_used: chips,
        model: Some(t.answered_by.clone()),
        sources: Vec::new(),
        stopped: false,
        tokens: None,
        documents: Vec::new(),
        pictures: Vec::new(),
        pages: Vec::new(),
        offers: Vec::new(),
    }))
}

/// A turn a newer question in its conversation replaced (`chat_turn`): no
/// answer saved, the question kept; [`crate::chat::stream::SUPERSEDED`].
fn superseded() -> String {
    tracing::info!("Chat: a newer question in the conversation replaced this turn; no answer saved");
    crate::chat::stream::SUPERSEDED.to_string()
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

/// **What Chat's next question sends besides itself and the history**, as
/// its turn measures it (`fit.rs`, `history.rs`): the stable prompt and the
/// tools' schemas a short question gets (`fixed`: Chat's ring), and the
/// most a question may be (`question_max`: with every family of tools that
/// may go gone, as a turn fits a long one).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FixedParts {
    pub fixed: u64,
    pub question_max: u64,
}

/// [`FixedParts`] for a question typed in Chat to `ep`'s model of `context`
/// tokens (`known`: it said so) in `conversation` (a Code chat's: no
/// memory, its agents kept), each request held to `cap`.
async fn fixed_parts(cfg: &YapConfig, ep: &Endpoint, conversation: Option<u64>, context: u64, known: bool, counter: &crate::chat::budget::Counter, cap: Option<u64>) -> FixedParts {
    let gates = gates::Gates::for_turn(cfg, ep, None, false, true);
    let code = crate::agents::code_chat::folder_of(conversation).is_some();
    let profile = crate::ai::about_you::profile(cfg);
    let connectors = gates.connectors.then(|| crate::chat::tools::connectors::Turn::for_turn(cfg, &ep.provider, &ep.model, None, context, conversation)).flatten();
    let parts = fit::Parts { persona: &ep.persona, profile: &profile, now: crate::ai::eval::now(), code, connectors: connectors.as_ref(), conversation };
    let room = fit::Room { context, known, kind: crate::chat::budget::Kind::Chat, tools: gates.tools, cap };
    let keep = || fit::keeps("", None, code, &[]);
    let fixed = fit::fit(gates, &parts, counter, room, 0, keep).await.fixed();
    let roomiest = fit::fit(gates, &parts, counter, room, u64::MAX, keep).await.fixed();
    FixedParts { fixed, question_max: room.history(roomiest) }
}

/// [`FixedParts`] for Chat's ring (`usage/context.rs`): the next question in
/// `conversation` to the model the chip names (`target`), each request held
/// to `cap`.
pub(crate) async fn ring_parts(cfg: &YapConfig, target: &crate::ai::ai_model::Target, conversation: Option<u64>, context: u64, known: bool, counter: &crate::chat::budget::Counter, cap: Option<u64>) -> FixedParts {
    let persona = cfg
        .llm_scopes
        .get("chat")
        .map(|s| s.prompt.clone())
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| "You are a helpful assistant. Answer concisely.".to_string());
    // With the line naming the model, as a turn's ([`Endpoint`]).
    let persona = crate::ai::ai_model::persona_with_answering(&persona, &target.chat_model());
    let ep = Endpoint {
        base_url: target.base_url.clone(),
        api_key: cfg.provider_api_key(&target.provider, ""),
        model: target.model.clone(),
        provider: target.provider.clone(),
        persona,
        disable_thinking: false,
    };
    fixed_parts(cfg, &ep, conversation, context, known, counter, cap).await
}

/// Chat's limits for its model now ([`Limits`]): what a question may be,
/// the model's context and, for the on-device model, more room it could
/// get. The Chat view asks on opening and when the model or its context
/// changes.
#[tauri::command]
pub async fn chat_limits() -> Limits {
    let cfg = config::load();
    let Ok(ep) = Endpoint::resolve(&cfg) else {
        return Limits::default();
    };
    let counter = crate::chat::budget::Counter::for_call(&ep.provider, &ep.base_url);
    let (context, context_known) = crate::chat::budget::context_window(&cfg, &ep.provider, &ep.model, &ep.base_url).await;
    // As a question typed in a new chat has them (not a Code chat yet), the
    // longest a turn would send: with every family of tools that may go
    // gone (fit.rs).
    let room = fixed_parts(&cfg, &ep, None, context, context_known, &counter, crate::chat::budget::rate_cap(&ep.provider)).await;
    let label = crate::chat::budget::model_label(&ep.provider, &ep.model, &crate::ai::local_llm::active_model_display(&cfg));
    let more = (ep.provider == crate::ai::local_llm::PROVIDER_ONDEVICE).then(|| more_room(&crate::ai::local_llm::context::context_view(&cfg))).flatten();
    Limits {
        question_max: room.question_max,
        context,
        context_known,
        too_long: crate::chat::budget::too_long(&label, context),
        label,
        more,
        provider: crate::privacy::net::provider_name(&ep.provider).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat_on(provider: &str, base_url: &str, model: &str) -> YapConfig {
        let mut cfg = YapConfig::default();
        cfg.llm_scopes.insert(
            "chat".into(),
            crate::config::LlmScope {
                enabled: true,
                provider: provider.into(),
                base_url: base_url.into(),
                model: model.into(),
                api_key: "test-key-not-real".into(),
                prompt: "You are a helpful voice assistant.".into(),
                ..Default::default()
            },
        );
        cfg
    }

    #[test]
    fn every_prompt_a_turn_writes_names_the_model_answering_it() {
        // The maintainer's phone (the Voice Assistant), 3 Nov 2026: Claude
        // Haiku 5.5 picked, and the answer "I'm Yap, not Claude".
        let cfg = chat_on("anthropic", "https://api.anthropic.com/v1", "claude-haiku-5-5");
        let ep = Endpoint::resolve(&cfg).unwrap();
        let start = "You are a helpful voice assistant. You are Yap's assistant, running on Claude Haiku 5.5 from Anthropic. Asked who or what you are, or which model you are, say so plainly;";
        assert!(ep.persona.starts_with(start), "{}", ep.persona);
        // Chat, the Voice Assistant (from another app, or the phone) and the
        // Meetings view's Ask: the system prompt starts with it, and so does
        // what a question is measured beside.
        let profile = crate::ai::about_you::Profile::default();
        let now = chrono::Utc::now();
        for (scope, meetings, in_view) in [(None, false, true), (Some("assistant"), false, false), (Some("meetings"), true, false)] {
            let gates = gates::Gates::for_turn(&cfg, &ep, scope, meetings, in_view);
            assert!(gates.stable_prompt(&ep.persona, &profile, now, false).starts_with(start), "{scope:?}");
        }
        // A small model's web pipeline writes its own prompt from the persona.
        assert!(crate::web::grounded_prompt(&ep.persona, "<passage n=\"1\">…</passage>", "Tuesday 3 November 2026", true).starts_with(start));
        // Another model picked: the line follows it, once.
        let ep = Endpoint::resolve(&chat_on("groq", "https://api.groq.com/openai/v1", "openai/gpt-oss-20b")).unwrap();
        assert!(ep.persona.contains("running on GPT-OSS 20B, served by Groq."), "{}", ep.persona);
        assert_eq!(ep.persona.matches("You are Yap's assistant").count(), 1);
    }
}

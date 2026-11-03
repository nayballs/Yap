//! One chat turn, answered: the Chat scope's endpoint, the prompt (the
//! notes the eager keyword-RAG finds, meetings, memory, About you), the
//! tool loop, or a small model's fixed web pipeline, and the checks on
//! every answer. The Chat view (`chat_send`), the Voice Assistant and the
//! model evaluations all ask through [`chat_turn`].
//!
//! Here: the turn's steps in their order ([`answer_turn`]), what they share
//! ([`Asked`], [`Opened`], [`Answer`]), the Chat model's endpoint and its
//! limits (and, inline for now, a small model's answer, the checks and
//! saving). Its parts: `fast` (the turns Yap answers itself, no model: an
//! app, a page, a Skill's question, the passages), `gates` (what this model
//! may do in this turn: tools, the web and whose search, pictures, the
//! screen, pages, the calendar), `history` (the question measured against
//! the model, and the history fitted to what's left), `prompt` (the system
//! prompt and the question's context: the eager RAG, the meetings'
//! passages, memory, About you, how to answer) and `run` (a model that
//! calls tools: the tool loop).

mod fast;
mod gates;
mod history;
mod prompt;
mod run;

use tauri::AppHandle;

use crate::config::{self, YapConfig};

pub(crate) use history::next_room;
pub(crate) use prompt::stable_prompt;

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
    if api_key.is_empty() && provider == crate::ai::chatgpt::PROVIDER {
        return Err("Yap isn't signed in to ChatGPT — sign in in Settings → Language Models → ChatGPT".to_string());
    }
    if api_key.is_empty() && crate::ai::ai_model::KEYED_PROVIDERS.contains(&provider.as_str()) {
        return Err(format!(
            "No {provider} API key — add one in Settings → Language Models"
        ));
    }
    Ok((base_url, api_key, model, provider, persona, disable_thinking))
}

/// The Chat scope's endpoint and persona ([`resolve_chat_endpoint`]), as a
/// turn uses them.
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
/// the question's index there, and a passage selected in the document
/// panel that it's about ("Change this…", documents.rs).
struct Opened {
    conv: crate::chat::chats::Conversation,
    user_index: usize,
    selection: Option<crate::chat::documents::Selection>,
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
    let gates = gates::Gates::for_turn(&cfg, &ep, scope, meetings, chat_in_view);
    // About you (about_you.rs): where the person is, their money and units,
    // name and note — last in the system prompt, below; their clock in the
    // question's context (it changes every minute).
    let profile = crate::ai::about_you::profile(&cfg);
    let now = crate::ai::eval::now();
    let t = Asked { app, cfg, text, scope, by_voice, skill, meetings, about_selection, ep, answered_by, gates, profile, now, watcher, stop };

    // **Sized to the model** (history.rs): a question too long for it is
    // never sent (nor saved): it's said plainly.
    let sizing = history::Sizing::measure(&t, conversation_id).await?;
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
    let answer = if t.gates.tools {
        run::with_tools(&t, &opened.conv, &built, &fitted, &sizing, live, watch.as_ref()).await?
    } else {
        // Small/local models: plain chat + eager RAG, or Yap's fixed web
        // pipeline when the question needs the web (small.rs).
        let (t, built, fitted, sizing, watch) = (&t, &built, &fitted, &sizing, watch.as_ref());
        let (cfg, ep) = (&t.cfg, &t.ep);
        // Small/local models: plain chat + eager RAG (their fallback), or
        // Yap's fixed web pipeline when the question needs the web.
        let endpoint = (ep.base_url.as_str(), ep.api_key.as_str(), ep.model.as_str(), ep.provider.as_str(), ep.disable_thinking);
        // The web pipeline's own prompt ends as this one does: About you,
        // then memory's "You".
        let (about, memory_you) = (&built.about, &built.memory_you);
        let about_and_memory = if memory_you.is_empty() { about.clone() } else { format!("{about}\n\n{memory_you}") };
        let SmallAnswer { reply, chips, sources, looked, pages, stopped, handoff } = small_model_answer(
            cfg,
            &ep.persona,
            &built.prompt.system,
            &about_and_memory,
            &fitted.history,
            &t.text,
            endpoint,
            t.gates.web_on,
            built.spoken,
            live,
            Some(t.stop),
            watch,
        )
        .await
        .map_err(|e| sizing.plain(e, &ep.provider))?;
        let browser = handoff.filter(crate::web::Handoff::offers).map(|h| crate::web::BrowserSearch::new(&cfg.web_search_browser, &h.query));
        // "Opening Baldur's Gate 3." from a model that can't open anything:
        // said as it is instead (apps.rs).
        let corrected = if crate::apps::claims_unasked(&reply, &t.text) {
            let (question, said, open_apps) = (t.text.clone(), reply.clone(), cfg.open_apps);
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
        let reply = if built.small_calendar.is_empty() {
            reply
        } else {
            let reply = crate::chat::tools::calendar::cite_mentions(&reply, &built.small_calendar);
            sources.extend(crate::chat::tools::calendar::sources_for(&reply, &built.small_calendar));
            sources.sort_by_key(|s| s.n);
            reply
        };
        let mut chips = chips;
        if let Some(chip) = built.calendar_chip.clone() {
            chips.insert(0, chip);
        }
        Answer {
            reply,
            tools_used: chips,
            sources,
            looked,
            pages,
            stopped,
            pictures: Vec::new(),
            shown_pages: Vec::new(),
            offers: Vec::new(),
            browser,
        }
    };
    // Every model's answer, checked before it's shown, then saved.
    let answer = {
        let (t, conv, built) = (&t, &opened.conv, &built);
        let Answer { reply, tools_used, sources, looked, pages, stopped, pictures, shown_pages, offers, browser } = answer;
        // Stopped before any of the answer showed: nothing to save.
        if stopped && reply.trim().is_empty() {
            return Err(crate::chat::stream::STOPPED.to_string());
        }
        let text = &t.text;
        let passages = &built.across.passages;
        // A Skill's answer as it's shown (/open-questions: Yap's "Q4: " off).
        let reply = match &t.skill {
            Some(s) => crate::chat::skills::tidy_answer(&s.skill, &reply),
            None => reply,
        };
        let (mut reply, fixed) = if t.meetings {
            let (r, mut f) = crate::chat::chats::checked_answer(&reply, text, looked, &[], passages);
            let only_these: Vec<usize> = passages.iter().map(|s| s.n).collect();
            let stripped = crate::chat::chats::strip_unsourced_citations(&r, &only_these);
            if stripped != r && !f.contains(&"stripped_citations") {
                f.push("stripped_citations");
            }
            (stripped, f)
        } else {
            crate::chat::chats::checked_answer(&reply, text, looked, &conv.messages, &sources)
        };
        for f in fixed {
            tracing::info!(fix = f, "Chat: Yap fixed the model's answer");
            crate::ai::eval::post_fix(f);
        }
        // Asked across the meetings: the passages it cites are its sources (none
        // cited: the first few, to check it against), and an answer that cites
        // none of them says so.
        let (sources, tools_used) = if t.meetings {
            // A statement plainly from one passage that the model left uncited
            // ("Passage 2", or only the meeting's name): Yap cites it.
            if let Some(found) = &built.across.found {
                let (cited, added) = crate::search::ask::cite_missing(&reply, &found.passages);
                if added > 0 {
                    tracing::info!(added, "Chat: Yap put in citations the answer left out");
                    crate::ai::eval::post_fix("cited_by_yap");
                    reply = cited;
                }
            }
            // A document written this turn (a Skill's /tickets, /prd): the
            // answer is a line saying where it is, and the passages the
            // document cites are its sources.
            let in_docs = built.doc_turn.passages_cited();
            let wrote_doc = !built.doc_turn.touched().is_empty();
            if !wrote_doc && crate::search::ask::needs_uncited_note(&reply, passages.len()) {
                tracing::info!("Chat: the answer cited none of the passages; Yap says so under it");
                crate::ai::eval::post_fix("uncited_answer");
                reply = format!("{}\n\n{}", reply.trim_end(), crate::search::ask::UNCITED_NOTE);
            }
            let mut chips: Vec<String> = built.across.chip.clone().into_iter().collect();
            chips.extend(tools_used);
            let shown = if in_docs.is_empty() {
                crate::search::ask::shown(&reply, passages)
            } else {
                let cited = crate::search::ask::cited_numbers(&reply);
                passages.iter().filter(|s| cited.contains(&s.n) || in_docs.contains(&s.n)).cloned().collect()
            };
            (shown, chips)
        } else {
            (sources, tools_used)
        };
        let region = &t.profile.region;
        // A price question that searched: no offer to look for prices where the
        // person is — Yap looked (and, finding only another country's, looked
        // again).
        if looked && crate::web::asks_about_prices(text) {
            if let Some(stripped) = crate::ai::about_you::LocalPrices::for_region(region).and_then(|local| crate::ai::about_you::strip_price_offer(&reply, &local)) {
                tracing::info!("Chat: Yap took out the answer's offer to look for local prices");
                crate::ai::eval::post_fix("price_offer");
                reply = stripped;
            }
        }
        // A price the web gave only in another money ("$8"), written in theirs
        // ("£8"): its page's sign back, and a note saying so.
        let mut restored_note = None;
        if let Some(local) = crate::ai::about_you::LocalPrices::for_region(region).filter(|_| !pages.is_empty()) {
            if let Some((fixed, restored)) = crate::ai::about_you::restore_price_signs(&reply, &pages, &local) {
                tracing::info!(prices = restored.len(), "Chat: Yap gave prices back the currency their pages gave them");
                crate::ai::eval::post_fix("price_sign");
                reply = fixed;
                restored_note = Some(crate::ai::about_you::restored_price_note(&restored, &local));
            }
        }
        // Prices in $ without saying whose, for someone who pays in £ (About
        // you): Yap says so under the answer (one note at most).
        if let Some(note) = crate::ai::about_you::foreign_price_note(&reply, region) {
            tracing::info!("Chat: Yap noted the answer's prices are in dollars");
            crate::ai::eval::post_fix("foreign_price_note");
            reply = format!("{}\n\n{note}", reply.trim_end());
        } else if let Some(note) = restored_note {
            reply = format!("{}\n\n{note}", reply.trim_end());
        }
        // A source of the turn's own (the window's text, read on this PC):
        // named after the answer's.
        let sources = crate::snip::turn::with_source(sources);
        Answer { reply, tools_used, sources, looked, pages, stopped, pictures, shown_pages, offers, browser }
    };
    {
        let (t, opened, built, fitted, sizing, watch) = (&t, &opened, &built, &fitted, &sizing, watch.as_ref());
        let conv_id = opened.conv.id;
        let Answer { reply, tools_used, sources, stopped, pictures, shown_pages, offers, browser, .. } = answer;
        // The last model call's tokens: the conversation's context use (a
        // stopped answer's call never finished, so it has none to say).
        let tokens = if stopped { None } else { crate::ai::usage::last_call().as_ref().and_then(crate::chat::chats::MessageTokens::of) };
        let index = crate::chat::chats::append_message(conv_id, "assistant", &reply, &tools_used, Some(t.answered_by.clone()), false, &sources, stopped, tokens)?;
        // The documents it made or changed: its pills, and this chat's from now on.
        let documents = built.doc_turn.touched();
        crate::chat::chats::set_documents(conv_id, index, &documents);
        // The pictures it showed beside the chat: its pills, and the panel's tabs.
        crate::chat::chats::set_pictures(conv_id, index, &pictures);
        // "Search in your browser", when the free sources couldn't answer.
        crate::chat::chats::set_browser_search(conv_id, index, browser.as_ref());
        // The pages it brought up (their tabs come back with the chat) and the
        // buttons it offered.
        crate::chat::chats::set_pages(conv_id, index, &shown_pages, &offers);
        crate::chat::chats::set_confirms(conv_id, index, &watch.map(|w| w.seen().confirms).unwrap_or_default());
        // Read for lasting facts once the chat goes quiet ("Learn from my chats").
        crate::memory::learn::after_turn(conv_id);
        // Filling the room? The older messages summarised in the background
        // (compact.rs), so the next question never waits for it.
        let ring = tokens.map(|n| (n.prompt + n.answer) as f64 / sizing.context_size.max(1) as f64);
        let ep = crate::chat::compact::Endpoint {
            base_url: t.ep.base_url.clone(),
            api_key: t.ep.api_key.clone(),
            model: t.ep.model.clone(),
            provider: t.ep.provider.clone(),
            disable_thinking: t.ep.disable_thinking,
            by: Some(t.answered_by.clone()),
        };
        let look = crate::chat::compact::Look { kind: sizing.kind, room: next_room(fitted.budget.history), context: sizing.context_size, ring, replay: built.replay };
        crate::chat::compact::after_answer(t.app, conv_id, ep, look).await;
        Ok(ChatTurn {
            conversation_id: conv_id,
            user_index: opened.user_index,
            index,
            reply,
            tools_used,
            model: Some(t.answered_by.clone()),
            sources,
            stopped,
            tokens,
            documents,
            pictures,
            pages: shown_pages,
            offers,
        })
    }
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
    Ok(Opened { conv, user_index, selection })
}

/// **"Remember that…" / "Forget that…" to a model without tools**: Yap's
/// own memory pass makes the change (memory/learn.rs), and says so — the
/// turn's answer, saved. A model with tools does it with `remember` /
/// `forget`. `None`: not one of those.
async fn remembered(t: &Asked<'_>, opened: &Opened) -> Result<Option<ChatTurn>, String> {
    if t.gates.tools {
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
    // As a question typed in Chat has them.
    let gates = gates::Gates::for_turn(&cfg, &ep, None, false, true);
    let counter = crate::chat::budget::Counter::for_call(&ep.provider, &ep.base_url);
    let (context, context_known) = crate::chat::budget::context_window(&cfg, &ep.provider, &ep.model, &ep.base_url).await;
    let profile = crate::ai::about_you::profile(&cfg);
    let stable = gates.stable_prompt(&cfg, &ep.persona, &profile, crate::ai::eval::now());
    let schemas = gates.schemas(&cfg, &counter, None).await;
    let fixed = counter.count(&stable).await + schemas;
    let budget = crate::chat::budget::budget(context, context_known, fixed, crate::chat::budget::Kind::Chat, gates.tools, crate::chat::budget::rate_cap(&ep.provider));
    let label = crate::chat::budget::model_label(&ep.provider, &ep.model, &crate::ai::local_llm::active_model_display(&cfg));
    let more = (ep.provider == crate::ai::local_llm::PROVIDER_ONDEVICE).then(|| more_room(&crate::ai::local_llm::context::context_view(&cfg))).flatten();
    Limits {
        question_max: budget.history,
        context,
        context_known,
        too_long: crate::chat::budget::too_long(&label, context),
        label,
        more,
        provider: crate::privacy::net::provider_name(&ep.provider).to_string(),
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
    spoken: bool,
    live: Option<crate::chat::stream::Live<'_>>,
    stop: Option<&crate::chat::stream::Stoppable>,
    watch: Option<&crate::chat::stream::Watch<'_>>,
) -> Result<SmallAnswer, String> {
    let mut chips = Vec::new();
    let mut sources = Vec::new();
    let mut pages = Vec::new();
    let mut system = system.to_string();
    let mut history = history.to_vec();
    let mut handoff = None;
    let looked = web_on && crate::web::needs_web(text);
    if looked {
        let settings = crate::web::Settings::for_chat(cfg, provider, model, base_url, api_key);
        // A price question's search names the person's country ("… UK").
        let query = crate::web::search_query(text);
        let query = crate::web::local_query(&query, text, settings.local.as_ref()).unwrap_or(query);
        let endpoints = crate::web::Endpoints::current();
        if let Some(live) = live {
            crate::chat::activity::tell(live.sink, crate::chat::activity::of_call("web_search", &serde_json::json!({ "query": query })));
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
                system = crate::web::grounded_prompt(persona, &block, &crate::web::today(), spoken);
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
                // A free source had a go at something it may not cover (a
                // game's price): the browser offered too.
                handoff = found.handoff.take();
            }
            Ok(found) => {
                chips.push(crate::web::search_chip(&query, &found, &settings));
                // Yap's free sources couldn't answer it: said plainly, with
                // "Search in your browser" under the answer.
                let line = found.handoff.as_ref().map_or_else(|| crate::web::no_search_line(Some("the search found nothing for it")), crate::web::Handoff::small_line);
                system.push_str(&format!("\n\n{line}"));
                handoff = found.handoff;
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
    Ok(SmallAnswer { reply, chips, sources, looked, pages, stopped, handoff })
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
    /// Yap's free web sources couldn't answer it: "Search in your browser".
    handoff: Option<crate::web::Handoff>,
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

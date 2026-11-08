//! The tool loop's path: a model that calls tools answers through the
//! agent loop (`tools::run_tool_loop_with`) with the turn's web side, its
//! documents, the calendar and the room its requests have; what it looked
//! up, showed and brought up becomes the answer's sources, pictures and
//! pages.

use serde_json::Value;

use super::history::{Fitted, Sizing};
use super::prompt::Built;
use super::{superseded, Answer, Asked};
use crate::notes::commands::note_changed;

/// **The answer of a model that calls tools** (`gates.tools`): the tool
/// loop over the turn's prompt and history, each note it changes
/// announced as it happens (`yap-note-changed` with origin "chat", then
/// `yap-notes-changed`). Stopped: what was shown so far is the answer (a
/// step's text that came before a tool call was never shown);
/// superseded: no answer.
pub(super) async fn with_tools(
    t: &Asked<'_>,
    conv: &crate::chat::chats::Conversation,
    built: &Built,
    fitted: &Fitted,
    sizing: &Sizing,
    live: Option<crate::chat::stream::Live<'_>>,
    watch: Option<&crate::chat::stream::Watch<'_>>,
) -> Result<Answer, String> {
    let (cfg, g, ep) = (&t.cfg, t.gates, &t.ep);
    let notify = |id: u64| {
        use tauri::Emitter;
        if let Some(app) = t.app {
            note_changed(app, id, "chat");
            let _ = app.emit("yap-notes-changed", ());
        }
    };
    let calendar_log = crate::chat::tools::calendar::Log::default();
    let earlier = &conv.messages[fitted.start..];
    let history = &fitted.history;
    // The turn's web side: the person's links (this message and their
    // earlier ones), the sources found, the safety rule's flag.
    let mut web = g.web_on.then(|| {
        let earlier: Vec<&str> = history.iter().filter(|(r, _)| r == "user").map(|(_, t)| t.as_str()).collect();
        let mut settings = crate::web::Settings::for_chat(cfg, &ep.provider, &ep.model, &ep.base_url, &ep.api_key);
        if let (true, Some(n)) = (g.claude_in_chat, settings.native.as_mut()) {
            n.in_chat = true;
        }
        let mut turn = crate::web::Turn::new(settings, crate::web::Endpoints::current(), &t.text, &earlier)
            .with_earlier_sources(conv.messages.iter().flat_map(|m| m.sources.iter().map(|s| s.url.as_str())));
        // A picture file on this PC (pictures/local.rs): the chat's own.
        turn.pictures.local = crate::chat::pictures::local::Context::of_chat(conv, &t.text);
        // A strong model looks further once it writes a document: now,
        // for a question that asks for one, else at its first document
        // call (`agent.rs`).
        if g.strong {
            turn = turn.may_go_deep(built.deep_reach);
            if built.writes_doc {
                turn.go_deep();
            }
            // A new document: the outline before anything's looked up.
            if built.outline_first {
                turn = turn.outline_first();
            }
        }
        turn
    });
    let mut messages: Vec<Value> = if built.replay {
        crate::chat::chats::history_messages(earlier)
    } else {
        history
            .iter()
            .map(|(r, t)| serde_json::json!({ "role": if r == "assistant" { "assistant" } else { "user" }, "content": t }))
            .collect()
    };
    // Earlier screenshots: the newest few sent again, the rest a line
    // (snip/turn.rs).
    crate::snip::turn::with_history_images(&mut messages, earlier, &ep.provider, &ep.model, &ep.base_url).await;
    // A Code chat (agents/code_chat.rs): none of memory's tools, nor past
    // chats' (Yap's memory is personal; the project keeps its own).
    let memory = built.code.is_none();
    // Connectors' tools (tools/connectors.rs): as the gates and the budget
    // measured them, from the store and the cache (a turn never waits for a
    // server to offer them).
    let connectors = g.connectors.then(|| crate::chat::tools::connectors::Turn::for_turn(cfg, &ep.provider, &ep.model, t.scope, sizing.context_size, Some(conv.id))).flatten();
    // The tools the gates offer (gates.rs `Gates::extra`: what the budget
    // counted, fitted to the model's context by `fit.rs`), with this turn's
    // calendar, agents and connectors.
    let calendar = crate::chat::tools::CalendarTurn { log: &calendar_log, zone: built.calendar_zone, now: t.now.timestamp() };
    // Yap Code (tools/agents.rs): asked out loud, an agent that needs asking is
    // asked in words; asked by voice, its report is read aloud.
    let agents = crate::chat::tools::AgentsTurn::asked(t.scope, t.by_voice);
    // Picture files on this PC without the web (pictures/local.rs): where
    // the turn keeps them (the answer keeps the ones shown).
    let files = (g.files != crate::chat::pictures::Gate::Off).then(|| crate::chat::pictures::Files::new(crate::chat::pictures::local::Context::of_chat(conv, &t.text)));
    let extra = crate::chat::tools::Extra {
        file_pictures: files.as_ref(),
        // Each tool result within what the request has left.
        room: (sizing.mode != crate::chat::budget::Mode::Messages).then_some(fitted.budget.request),
        docs: Some(&built.doc_turn),
        // Words selected in another app or on a page beside the chat (never
        // one of Yap's own windows): web content.
        web_question: t.about_selection.as_ref().is_some_and(|s| s.web),
        ..g.extra(memory, Some(conv.id), calendar, agents, connectors.as_ref())
    };
    let looped = crate::chat::tools::run_tool_loop_with(
        &built.prompt.system,
        &messages,
        &t.text,
        &built.prompt.context,
        &ep.base_url,
        &ep.api_key,
        &ep.model,
        &ep.provider,
        ep.disable_thinking,
        g.apps,
        &notify,
        web.as_mut(),
        built.first.clone(),
        extra,
        live,
    );
    // Stopped: what was shown so far is the answer (a step's text that
    // came before a tool call was never shown). Superseded: no answer.
    let (reply, tools_used, stopped) = match crate::chat::stream::unless_stopped(looped, Some(t.stop)).await {
        Ok(done) => {
            // "Too long for the model" said plainly, never the provider's JSON.
            let (reply, tools_used) = done.map_err(|e| sizing.plain(e, &ep.provider))?;
            (reply, tools_used, false)
        }
        Err(crate::chat::stream::Halt::Superseded) => return Err(superseded()),
        Err(crate::chat::stream::Halt::Stopped) => {
            let seen = watch.map(|w| w.seen()).unwrap_or_default();
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
    // The pictures it showed (chat/pictures.rs): each one's page is a
    // source of the answer too, so its number means something.
    let mut shown = web.as_ref().map(|w| w.pictures.shown.clone()).unwrap_or_default();
    shown.extend(files.as_ref().map(crate::chat::pictures::Files::shown).unwrap_or_default());
    // The files it shared into the conversation (chat/shares.rs).
    let mut shared = web.as_ref().map(|w| w.pictures.shared.clone()).unwrap_or_default();
    shared.extend(files.as_ref().map(crate::chat::pictures::Files::shared).unwrap_or_default());
    if let Some(w) = web.as_ref() {
        for p in shown.iter().filter(|p| p.n > 0) {
            if !sources.iter().any(|s| s.n == p.n) {
                sources.extend(w.sources.iter().find(|s| s.n == p.n).cloned());
            }
        }
    }
    sources.extend(crate::chat::tools::calendar::sources_for(&reply, &calendar_log.all()));
    sources.sort_by_key(|s| s.n);
    // "Search in your browser" under the answer: Yap's free web sources
    // couldn't give it (no search service for the turn, web/sources.rs).
    let browser = web.as_ref().and_then(|w| w.browser_search(&cfg.web_search_browser));
    // The pages it brought up and the buttons it offered (chat/pages.rs).
    let (shown_pages, offers) = web.as_ref().map(|w| (w.pages.shown.clone(), w.pages.offers.clone())).unwrap_or_default();
    let pages = web.map(|w| w.texts).unwrap_or_default();
    Ok(Answer { reply, tools_used, sources, looked, pages, stopped, pictures: shown, files: shared, shown_pages, offers, browser })
}

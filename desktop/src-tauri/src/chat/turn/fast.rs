//! The turns Yap answers itself, before any model: an app opened (or why
//! not), a page opened beside the chat, a Skill's question when it has
//! nothing to run on, and the passages that match when nothing can answer
//! across the meetings. Each saves its question like any other, and
//! replaces an answer still coming in its conversation (`stop`).

use super::{resolve_chat_endpoint, turn_conversation, ChatTurn};
use crate::config::YapConfig;

/// A page Yap opens beside the chat by itself: the page, the answer's line
/// and its chip.
#[derive(Debug, Clone, PartialEq)]
pub struct AskedPage {
    pub page: crate::chat::pages::ShownPage,
    pub reply: String,
    pub chip: String,
}

/// **What Yap opens beside the chat itself** for `text`, before any model
/// (as it opens an app: Qwen3.5-4B, given `open_page`, opened Google Chrome
/// with `open_app` or only said it had, 1 Nov 2026): a site asked for by
/// name or address ("open YouTube", `apps::site_asked`), or a search asked
/// for (`apps::search_asked`) when the person asked for the search page
/// itself ("google …", "open a search for …"), or — `keyless`: no search
/// service answers this chat (no key, no provider search) — Yap's free
/// sources don't cover it (travel, prices, the news…: a model could only
/// say so). With a service, "search for …" is the model's to search and
/// answer. On the person's `engine`, a shop in their `country` (an ISO
/// code). `None`: the model's. Pure.
pub fn asked_page(text: &str, engine: &str, country: &str, keyless: bool) -> Option<AskedPage> {
    if let Some((name, url)) = crate::apps::site_asked(text, country) {
        let shown = crate::web::short_url(&url);
        return Some(AskedPage {
            page: crate::chat::pages::ShownPage { site: crate::web::site_of(&url), url, title: name.clone(), section: String::new() },
            reply: format!("{name} is open beside the chat."),
            chip: format!("Brought up {shown} beside the chat"),
        });
    }
    let (words, page_itself) = crate::apps::search_asked(text)?;
    let r = crate::web::sources::route(&words, text, &[]);
    let uncovered = matches!(r.kind, crate::web::sources::Kind::NotCovered(_)) || r.offer.is_some();
    if !page_itself && !(keyless && uncovered) {
        return None;
    }
    let search = crate::web::BrowserSearch::new(engine, &words);
    Some(AskedPage {
        page: crate::chat::pages::ShownPage { site: crate::web::site_of(&search.url), url: search.url.clone(), title: format!("Search: {}", search.query), section: String::new() },
        reply: format!("The search for \u{201C}{}\u{201D} is open beside the chat, on {}.", search.query, search.engine),
        chip: format!("Brought up a search for \"{}\" beside the chat", search.query),
    })
}

/// **A page Yap opens itself** for `text` ([`asked_page`]): a site or a
/// search asked for, with pages beside the chat (the web on) — only with
/// Chat in view (`chat_in_view`: never a phone or the meetings; there's no
/// panel to see it in) and never over a screenshot the question carries
/// (it's about that). (Never a Skill's: the question is theirs, the
/// caller's check.) `None`: the model's.
pub(super) fn page_asked(cfg: &YapConfig, text: &str, chat_in_view: bool) -> Option<AskedPage> {
    if !(cfg.web_search && chat_in_view && crate::snip::turn::images().is_empty()) {
        return None;
    }
    // A search only where no search service would answer it (no key,
    // no provider search): with one, the model searches and answers.
    let keyless = resolve_chat_endpoint(cfg).is_ok_and(|(base, key, model, provider, _, _)| {
        crate::web::backend(&crate::web::Settings::for_chat(cfg, &provider, &model, &base, &key)) == crate::web::Backend::Keyless
    });
    let country = crate::ai::about_you::profile(cfg).region.country.unwrap_or_default();
    asked_page(text, &cfg.web_search_browser, &country, keyless)
}

/// **The turn Yap answers by opening a page** ([`asked_page`]): the page
/// beside the chat at once (`Event::Page`), kept with the answer.
pub(super) fn page_turn(
    conversation_id: Option<u64>,
    text: &str,
    by_voice: bool,
    asked: impl FnOnce(u64, usize) + Send,
    opened: AskedPage,
    watcher: Option<&dyn crate::chat::stream::Sink>,
    stop: &crate::chat::stream::Stoppable,
) -> Result<ChatTurn, String> {
    let conv = turn_conversation(conversation_id, text);
    let user_index = crate::chat::chats::append(conv.id, "user", text, &[], None, by_voice)?;
    stop.answers(conv.id);
    asked(conv.id, user_index);
    if let Some(w) = watcher {
        w.send(crate::chat::stream::Event::Page { page: opened.page.clone() });
    }
    let tools_used = vec![opened.chip];
    let index = crate::chat::chats::append(conv.id, "assistant", &opened.reply, &tools_used, None, false)?;
    crate::chat::chats::set_pages(conv.id, index, std::slice::from_ref(&opened.page), &[]);
    tracing::info!("Chat: a page opened beside the chat by Yap itself, no model");
    Ok(ChatTurn {
        conversation_id: conv.id,
        user_index,
        index,
        reply: opened.reply,
        tools_used,
        model: None,
        sources: Vec::new(),
        stopped: false,
        tokens: None,
        documents: Vec::new(),
        pictures: Vec::new(),
        files: Vec::new(),
        pages: vec![opened.page],
        offers: Vec::new(),
        direct: None,
    })
}

/// **The answer to Yap's spoken question before an agent** (`agents/voice.rs`):
/// a yes starts it ("On it: I've started an agent in garden-planner."), its tab
/// opening at once (`Event::Agent`); a no doesn't. Saved like any turn, no
/// model.
pub(super) fn agent_answer_turn(
    conversation_id: Option<u64>,
    text: &str,
    by_voice: bool,
    asked: impl FnOnce(u64, usize) + Send,
    answer: crate::agents::voice::Answer,
    watcher: Option<&dyn crate::chat::stream::Sink>,
    stop: &crate::chat::stream::Stoppable,
) -> Result<ChatTurn, String> {
    let conv = turn_conversation(conversation_id, text);
    let user_index = crate::chat::chats::append(conv.id, "user", text, &[], None, by_voice)?;
    stop.answers(conv.id);
    asked(conv.id, user_index);
    let (reply, chip) = match answer {
        crate::agents::voice::Answer::Yes(w) => match crate::chat::tools::agents::start_waiting(*w) {
            // A new agent, or (a follow-up) the chat's agent carried on.
            Ok(begun) => {
                if let Some(w) = watcher {
                    w.send(crate::chat::stream::Event::Agent { agent: begun.reference() });
                }
                (begun.say, begun.chip)
            }
            Err(e) => (format!("I couldn't start the agent: {e}"), "Didn't send an agent".to_string()),
        },
        crate::agents::voice::Answer::No(w) => ("Okay, I won't send it.".to_string(), format!("Didn't send an agent to {}: you said no", w.folder.name)),
    };
    let tools_used = vec![chip];
    let index = crate::chat::chats::append(conv.id, "assistant", &reply, &tools_used, None, false)?;
    tracing::info!("Chat: Yap's question before an agent, answered by Yap itself");
    Ok(ChatTurn {
        conversation_id: conv.id,
        user_index,
        index,
        reply,
        tools_used,
        model: None,
        sources: Vec::new(),
        stopped: false,
        tokens: None,
        documents: Vec::new(),
        pictures: Vec::new(),
        files: Vec::new(),
        pages: Vec::new(),
        offers: Vec::new(),
        direct: None,
    })
}

/// A turn Yap answers itself (`apps.rs`): the request saved like any
/// question, the app opened (or why not) as the answer, with its chip. No
/// model, so none is named. Like any newer question, it replaces an answer
/// still coming in its conversation (`stop`, [`super::chat_turn`]).
pub(super) async fn app_turn(
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
        documents: Vec::new(),
        pictures: Vec::new(),
        files: Vec::new(),
        pages: Vec::new(),
        offers: Vec::new(),
        direct: None,
    })
}

/// A Skill with nothing to run on (`skills::ask_first`: /project-update
/// with no project named, /prep with nothing named or coming up): its
/// question, said by Yap itself — no model, so none is named — the chat
/// asked across the meetings from then on, as a Skill's is.
pub(super) fn skill_asks_turn(
    conversation_id: Option<u64>,
    text: &str,
    by_voice: bool,
    asked: impl FnOnce(u64, usize) + Send,
    stop: &crate::chat::stream::Stoppable,
    said: &str,
    command: &str,
) -> Result<ChatTurn, String> {
    tracing::info!(command = %command, "Chat: a Skill with nothing to run on; Yap asks for it");
    let conv = turn_conversation(conversation_id, text);
    if conv.scope.is_empty() && conv.messages.is_empty() {
        crate::chat::chats::set_scope(conv.id, "meetings");
    }
    let user_index = crate::chat::chats::append(conv.id, "user", text, &[], None, by_voice)?;
    stop.answers(conv.id);
    asked(conv.id, user_index);
    let tools_used = vec![format!("Ran /{command}")];
    let index = crate::chat::chats::append(conv.id, "assistant", said, &tools_used, None, false)?;
    Ok(ChatTurn {
        conversation_id: conv.id,
        user_index,
        index,
        reply: said.to_string(),
        tools_used,
        model: None,
        sources: Vec::new(),
        stopped: false,
        tokens: None,
        documents: Vec::new(),
        pictures: Vec::new(),
        files: Vec::new(),
        pages: Vec::new(),
        offers: Vec::new(),
        direct: None,
    })
}

/// A question asked across the meetings with no AI model to answer it (none
/// set up, or the on-device one won't start: `why`): Yap answers with the
/// passages that match, each cited so its pill opens the moment
/// (`search::ask::passages_answer`). No model, so none is named.
pub(super) async fn passages_turn(
    conversation_id: Option<u64>,
    text: &str,
    by_voice: bool,
    asked: impl FnOnce(u64, usize) + Send,
    stop: &crate::chat::stream::Stoppable,
    why: &str,
) -> Result<ChatTurn, String> {
    tracing::info!(reason = %why.chars().take(80).collect::<String>(), "Chat: no model to answer across the meetings; showing the passages");
    let conv = turn_conversation(conversation_id, text);
    // A Skill: its own passages (chat/skills.rs) — and, as with a model
    // (`turn::open`), one run in a chat that's about something else leaves
    // it so (it set the chat's scope to "meetings" until 2026-11-03).
    let skill = crate::chat::skills::current();
    if conv.scope.is_empty() && (skill.is_none() || conv.messages.is_empty()) {
        crate::chat::chats::set_scope(conv.id, "meetings");
    }
    let user_index = crate::chat::chats::append(conv.id, "user", text, &[], None, by_voice)?;
    stop.answers(conv.id);
    asked(conv.id, user_index);
    let found = match &skill {
        Some(s) => crate::chat::skills::find(s).await,
        None => crate::search::ask::find(text, crate::search::ask::ASK_PASSAGES).await,
    };
    let reply = crate::search::ask::passages_answer(&found);
    let sources = crate::search::ask::sources(&found);
    let tools_used = vec![match &skill {
        Some(s) => crate::chat::skills::chip(s, &found),
        None => crate::search::ask::chip(&found),
    }];
    let index = crate::chat::chats::append_message(conv.id, "assistant", &reply, &tools_used, None, false, &sources, false, None)?;
    Ok(ChatTurn {
        conversation_id: conv.id,
        user_index,
        index,
        reply,
        tools_used,
        model: None,
        sources,
        stopped: false,
        tokens: None,
        documents: Vec::new(),
        pictures: Vec::new(),
        files: Vec::new(),
        pages: Vec::new(),
        offers: Vec::new(),
        direct: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_site_or_a_search_asked_for_opens_beside_the_chat_with_no_model() {
        let a = asked_page("Open Youtube.", "google", "", true).unwrap();
        assert_eq!(a.page.url, "https://www.youtube.com/");
        assert_eq!(a.page.title, "YouTube");
        assert_eq!(a.reply, "YouTube is open beside the chat.");
        assert_eq!(a.chip, "Brought up youtube.com beside the chat");
        // Travel isn't the free sources': the search itself, on the person's engine.
        let s = asked_page("Search for train tickets from Kellbrook to Port Ellis.", "google", "", true).unwrap();
        assert_eq!(s.page.url, "https://www.google.com/search?q=train+tickets+from+Kellbrook+to+Port+Ellis");
        assert_eq!(s.page.title, "Search: train tickets from Kellbrook to Port Ellis");
        assert_eq!(s.chip, "Brought up a search for \"train tickets from Kellbrook to Port Ellis\" beside the chat");
        assert!(s.reply.ends_with("is open beside the chat, on Google."), "{}", s.reply);
        // What the free sources cover is the model's to answer, unless the page itself was asked for.
        assert!(asked_page("search for rust error E0499", "google", "", true).is_none());
        assert!(asked_page("google rust error E0499", "google", "", true).is_some());
        assert!(asked_page("How are you?", "google", "", true).is_none());
        assert!(asked_page("open teams", "google", "", true).is_none());
        // With a search service (a key, or the provider's own search), "search for …" is the model's.
        assert!(asked_page("Search for train tickets from Kellbrook to Port Ellis.", "google", "", false).is_none());
        assert!(asked_page("google flights to Port Ellis", "google", "", false).is_some());
        // "Search the web for …" asks for research, never the page.
        assert!(asked_page("search the web for the best budget laptops in 2026", "google", "", false).is_none());
    }
}

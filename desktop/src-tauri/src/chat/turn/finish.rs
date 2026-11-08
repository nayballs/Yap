//! A turn's end: the checks on every model's answer before it's shown —
//! sources, searches it claims, citations, the meetings' passages, prices —
//! and the answer saved with what it made, showed and cost.

use super::history::{next_room, Fitted, Sizing};
use super::prompt::Built;
use super::{Answer, Asked, ChatTurn, Opened};

/// **Every model's answer, checked before it's shown**: no sources list or
/// "Yap searched…" note of its own, no search claimed that didn't happen,
/// no "[1]" with nothing (or another question's pages) behind it — asked
/// across the meetings, a number that's none of this turn's passages, and
/// the passages it cites as its sources; then the prices (an offer to look
/// where the person is, another money's sign, dollars without saying
/// whose). Stopped before any of it showed: nothing to save (the error).
pub(super) fn check(t: &Asked<'_>, answer: Answer, conv: &crate::chat::chats::Conversation, built: &Built) -> Result<Answer, String> {
    let Answer { reply, tools_used, sources, looked, pages, stopped, pictures, files, shown_pages, offers, browser } = answer;
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
    Ok(Answer { reply, tools_used, sources, looked, pages, stopped, pictures, files, shown_pages, offers, browser })
}

/// **The answer saved** in its conversation (`opened`) with its chips, who
/// wrote it, its sources and its last model call's tokens, then what it
/// made, showed and offered; the chat read for lasting facts later, and its
/// older messages summarised in the background when it fills the room.
/// The turn as [`super::chat_turn`] returns it.
pub(super) async fn save(
    t: &Asked<'_>,
    opened: &Opened,
    answer: Answer,
    built: &Built,
    fitted: &Fitted,
    sizing: &Sizing,
    watch: Option<&crate::chat::stream::Watch<'_>>,
) -> Result<ChatTurn, String> {
    let conv_id = opened.conv.id;
    let Answer { reply, tools_used, sources, stopped, pictures, files, shown_pages, offers, browser, .. } = answer;
    // The last model call's tokens: the conversation's context use (a
    // stopped answer's call never finished, so it has none to say).
    let tokens = if stopped { None } else { crate::ai::usage::last_call().as_ref().and_then(crate::chat::chats::MessageTokens::of) };
    let index = crate::chat::chats::append_message(conv_id, "assistant", &reply, &tools_used, Some(t.answered_by.clone()), false, &sources, stopped, tokens)?;
    // The documents it made or changed: its pills, and this chat's from now on.
    let documents = built.doc_turn.touched();
    crate::chat::chats::set_documents(conv_id, index, &documents);
    // The pictures it showed beside the chat: its pills, and the panel's tabs.
    crate::chat::chats::set_pictures(conv_id, index, &pictures);
    // The files it shared into the conversation: in its bubble.
    crate::chat::chats::set_files(conv_id, index, &files);
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
        files,
        pages: shown_pages,
        offers,
    })
}

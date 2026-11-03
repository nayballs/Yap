//! A model that can't call tools (the on-device Qwen2.5-1.5B, a local
//! model under 4B): plain chat over the notes, or — a question that needs
//! the web — Yap's fixed web pipeline, its search and passages in the
//! prompt; the calendar it looked up for it cited.

use super::history::{Fitted, Sizing};
use super::prompt::Built;
use super::{superseded, Answer, Asked};
use crate::config::YapConfig;

/// **The answer of a model without tools** (`gates.tools` off): plain chat
/// with the eager RAG (their fallback), or Yap's fixed web pipeline when
/// the question needs the web ([`small_model_answer`]); an app it claims
/// to have opened said as it is, the calendar Yap looked up for it cited.
pub(super) async fn answer(
    t: &Asked<'_>,
    built: &Built,
    fitted: &Fitted,
    sizing: &Sizing,
    live: Option<crate::chat::stream::Live<'_>>,
    watch: Option<&crate::chat::stream::Watch<'_>>,
) -> Result<Answer, String> {
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
    Ok(Answer {
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
    })
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

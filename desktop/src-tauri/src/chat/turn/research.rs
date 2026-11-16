//! **A research turn** (chat/research.rs: the Claude app's Research mode):
//! the plan first, made by the model with no tools and shown as the outline
//! card's research plan; then, once the person says go, the loop — the
//! tool loop (`run.rs`), or on your Claude plan the conversation's Claude
//! Code session (`plan.rs`) — with the plan and its caps in the question's
//! context; then the report made a document beside the chat, its citations
//! only to the pages read, and the thread's short answer and the
//! document's card.

use super::history::{Fitted, Sizing};
use super::prompt::Built;
use super::{plan, run, superseded, Answer, Asked, Opened};
use crate::chat::cards::outline::{self, Review};
use crate::chat::research::{self, Budget, Caps, Plan, Planned};
use crate::chat::stream::{unless_stopped, Event, Halt, Live, Sink, Watch};

/// What a research stopped before its report says.
const STOPPED: &str = "Research stopped before the report was written.";

/// The most the read pages' own titles may take, all at once, before the
/// report ([`crate::web::title_read_pages`]): those not back keep their address.
const TITLES_WITHIN: std::time::Duration = std::time::Duration::from_secs(8);

/// **Does this turn research?** The Research switch, or a message that
/// plainly asks ([`research::asks`]) — typed (or dictated) in Chat, never a
/// Code chat, across the meetings, a Skill, a passage selected, the Voice
/// Assistant; with web search on, and a model that calls tools (or your
/// Claude plan).
pub(super) fn wanted(t: &Asked<'_>, opened: &Opened) -> bool {
    let asked = research::switched() || research::asks(&t.text);
    asked
        && t.scope.is_none()
        && !t.meetings
        && t.skill.is_none()
        && t.about_selection.is_none()
        && opened.selection.is_none()
        && opened.code.is_none()
        && t.gates.web_on
        && (t.gates.tools || t.ep.provider == crate::ai::claude_plan::PROVIDER)
}

/// The day as the person's clock says it ("16 November 2026").
fn day_of(t: &Asked<'_>) -> String {
    match t.profile.region.time_zone.as_deref().and_then(|z| z.parse::<chrono_tz::Tz>().ok()) {
        Some(tz) => t.now.with_timezone(&tz).format("%-d %B %Y").to_string(),
        None => t.now.with_timezone(&chrono::Local).format("%-d %B %Y").to_string(),
    }
}

/// **The plan step**: one call with no tools ([`research::PLAN_PROMPT`]) on
/// Chat's model (your Claude plan's one-shot run too), counted in the turn's
/// usage. Its reply read ([`research::parse_plan`]); a call that fails is a
/// plan from the question alone.
async fn make_plan(t: &Asked<'_>, fitted: &Fitted) -> Planned {
    let messages = serde_json::json!([
        { "role": "system", "content": research::PLAN_PROMPT },
        { "role": "user", "content": research::plan_request(&t.text, &fitted.history, &day_of(t)) },
    ]);
    let here = crate::ai::ai_model::runs_here(&t.ep.provider, &t.ep.base_url);
    let opts = crate::ai::llm::ChatOptions {
        temperature: 0.3,
        max_tokens: here.then_some(700),
        timeout: std::time::Duration::from_secs(if here { 120 } else { 90 }),
        disable_thinking: true,
    };
    let call = crate::ai::llm::chat(&t.ep.base_url, &t.ep.api_key, &t.ep.model, &t.ep.provider, messages, &opts);
    match crate::privacy::net::within(crate::privacy::net::Feature::Chat, call).await {
        Ok(reply) => research::parse_plan(&reply, &t.text),
        Err(e) => {
            tracing::info!(error = %crate::web::short(&e), "Chat: a research's plan step failed; a plan from the question");
            Planned::Plan(research::fallback_plan(&t.text))
        }
    }
}

/// An answer of Yap's own for the research (stopped, or not started): its
/// words and chips, the pages it found.
fn plain(reply: &str, chips: Vec<String>, sources: Vec<crate::chat::chats::Source>, stopped: bool) -> Answer {
    let looked = !sources.is_empty();
    Answer {
        reply: reply.to_string(),
        tools_used: chips,
        sources,
        looked,
        pages: Vec::new(),
        stopped,
        pictures: Vec::new(),
        files: Vec::new(),
        shown_pages: Vec::new(),
        offers: Vec::new(),
        browser: None,
    }
}

/// **The research turn's answer** (see the module's doc). `None`: the plan
/// step found nothing to research (a greeting): the turn goes on as any.
#[allow(clippy::too_many_arguments)]
pub(super) async fn answer(
    t: &Asked<'_>,
    opened: &Opened,
    built: &mut Built,
    fitted: &Fitted,
    sizing: &Sizing,
    live: Option<Live<'_>>,
    watch: Option<&Watch<'_>>,
) -> Result<Option<Answer>, String> {
    let sink = watch.map(|w| w as &dyn Sink);
    let on_plan = t.ep.provider == crate::ai::claude_plan::PROVIDER;
    // 1. The plan, with no tools.
    if let Some(s) = sink {
        crate::chat::activity::tell(s, crate::chat::activity::research_plans());
    }
    let mut plan: Plan = match unless_stopped(make_plan(t, fitted), Some(t.stop)).await {
        Err(Halt::Superseded) => return Err(superseded()),
        Err(Halt::Stopped) => return Ok(Some(plain(STOPPED, Vec::new(), Vec::new(), true))),
        Ok(Planned::NotResearch) => {
            tracing::info!("Chat: research asked for, but the plan step found nothing to research; answered as any turn");
            return Ok(None);
        }
        Ok(Planned::Plan(p)) => p,
    };
    tracing::info!(sections = plan.sections.len(), plan_model = on_plan, "Chat: a research's plan made");
    // 2. The person sees it first (the outline card's research plan).
    let reviewed = unless_stopped(outline::review(live, &plan.args(), false, 1), Some(t.stop)).await;
    match reviewed {
        Err(Halt::Superseded) => return Err(superseded()),
        Err(Halt::Stopped) => return Ok(Some(plain(STOPPED, Vec::new(), Vec::new(), true))),
        Ok(Review::Stop { .. }) => {
            tracing::info!("Chat: a research's plan was turned down; nothing researched");
            return Ok(Some(plain("Okay, I didn't start the research. Tell me what you'd like to look into instead.", vec!["Didn't start the research: you said not to".into()], Vec::new(), false)));
        }
        Ok(Review::Go { sections, note, said, .. }) => {
            plan.sections = sections;
            plan.note = [note, said].into_iter().filter(|s| !s.trim().is_empty()).collect::<Vec<_>>().join(" ");
        }
        // (Words that ask for changes: the research follows them.)
        Ok(Review::Again { said }) => plan.note = said,
        Ok(Review::Unasked) => {}
    }
    // 3. The loop, within its caps (pages sized to what the model's request holds).
    let page = if t.gates.claude_in_chat { crate::web::native::FETCH_TOKENS as usize } else { crate::web::PAGE_TOKENS };
    let caps = Caps::for_request(Some(fitted.budget.request), page);
    let budget = Budget::start(caps);
    built.prompt.context.push(research::instructions(&plan, &caps, on_plan));
    built.writes_doc = false;
    built.outline_first = false;
    built.first = crate::chat::tools::FirstStep::Model;
    tracing::info!(searches = caps.searches, reads = caps.reads, mins = caps.time.as_secs() / 60, "Chat: a research begins");
    let found = if on_plan {
        plan::answer(t, opened, built, fitted, watch, Some(budget)).await?
    } else {
        // (Its words never stream into the thread: the report goes in a document.)
        let quiet = sink.map(research::Quiet::new);
        let quiet_live = match (live, quiet.as_ref()) {
            (Some(l), Some(q)) => Some(Live { sink: q, beside: l.beside }),
            _ => live,
        };
        run::with_tools(t, &opened.conv, built, fitted, sizing, quiet_live, watch, Some(budget)).await?
    };
    // The pages read that came with no title of their own (Claude Code's
    // WebFetch gives none; a page not in the results then showed its address):
    // their own, read on this PC, for the Sources row and the report's list.
    let mut found = found;
    if !found.stopped {
        let ep = crate::web::Endpoints::current();
        let titled = crate::web::title_read_pages(&ep, &mut found.sources, caps.reads, TITLES_WITHIN);
        // (■ now: the report's written; it's kept, as its pages had them.)
        if let Err(Halt::Superseded) = unless_stopped(titled, Some(t.stop)).await {
            return Err(superseded());
        }
    }
    // 4. The report.
    Ok(Some(report(t, built, found, &plan, budget, sink)))
}

/// **The report, made a document** beside the chat (`found`: the loop's
/// answer, its report as the model wrote it and every page it found): its
/// citations only to the pages read; the thread's answer the summary, the
/// document's card and the research's chip. Stopped: what it found, and a
/// line saying so.
fn report(t: &Asked<'_>, built: &Built, found: Answer, plan: &Plan, budget: Budget, sink: Option<&dyn Sink>) -> Answer {
    let (searches, reads) = research::counts(&found.tools_used, &found.sources);
    let mut chips = found.tools_used.clone();
    if found.stopped {
        tracing::info!(searches, reads, "Chat: a research was stopped before its report");
        let read: Vec<_> = found.sources.iter().filter(|s| s.opened).cloned().collect();
        return Answer { reply: STOPPED.to_string(), sources: read, stopped: true, ..found };
    }
    let byline = research::byline(&day_of(t), searches, reads);
    let r = research::report(&found.reply, &found.sources, &plan.title, &byline);
    chips.push(research::done_chip(searches, reads, budget.elapsed()));
    let reply = match crate::chat::documents::create_report(&r.title, &r.body) {
        Ok(id) => {
            if let Some(doc) = built.doc_turn.noticed("create_document", id) {
                if let Some(s) = sink {
                    s.send(Event::Document { note_id: doc.id, title: doc.title });
                }
            }
            if let Some(app) = t.app {
                use tauri::Emitter;
                crate::notes::commands::note_changed(app, id, "chat");
                let _ = app.emit("yap-notes-changed", ());
            }
            chips.push(format!("Wrote the report “{}”", r.title));
            r.answer.clone()
        }
        // No document (the notes couldn't be saved): the report in the thread.
        Err(e) => {
            tracing::warn!(error = %crate::web::short(&e), "Chat: a research's report couldn't be made a document; it's the answer");
            r.body.clone()
        }
    };
    tracing::info!(searches, reads, cited = r.cited, dropped = r.dropped, secs = budget.elapsed().as_secs(), "Chat: a research answered");
    Answer { reply, tools_used: chips, sources: r.sources, looked: true, stopped: false, browser: None, ..found }
}

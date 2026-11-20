//! **A research's helpers, run** (what they are: chat/research/helpers.rs):
//! one for each part of the approved plan, [`helpers::AT_ONCE`] at a time,
//! each with its own searches, page reads and context — on a key, Yap's
//! tool loop with a web side of its own ([`on_key`]); on your Claude plan, a
//! Claude Code of its own ([`on_plan`]: WebSearch and WebFetch only, none
//! of the person's settings or servers, nothing saved). Their findings and
//! pages come back in the plan's order for the lead; one that fails says
//! why, and its part is the lead's.

use std::time::Duration;

use futures_util::StreamExt;

use super::super::history::Fitted;
use super::super::Asked;
use crate::ai::claude_code::events::Event;
use crate::ai::claude_code::session::{Pay, PermissionMode, Session, Spec};
use crate::ai::claude_plan::map::{self, Fed, Mapper};
use crate::ai::claude_plan::{self as plan, oneshot};
use crate::chat::research::helpers::{self, Crew, Found};
use crate::chat::research::{Budget, Plan};
use crate::chat::stream::{Event as Shown, Live, Sink};

/// Nothing heard from a helper's Claude Code this long: stuck.
const IDLE: Duration = Duration::from_secs(5 * 60);
/// A stopped helper's turn read to its end, at most this long.
const DRAIN: Duration = Duration::from_secs(15);

/// **The helpers, all at work** (`budget`: the research's, theirs too;
/// `crew`: their one line on the page): each part's findings, in the plan's
/// order. ■ or a newer question drops them all (their processes end).
#[allow(clippy::too_many_arguments)]
pub(super) async fn run(t: &Asked<'_>, conv: &crate::chat::chats::Conversation, fitted: &Fitted, plan: &Plan, day: &str, budget: Budget, crew: Option<&Crew<'_>>, beside: Option<&dyn crate::chat::stream::Dictations>) -> Vec<Found> {
    let on_plan = t.ep.provider == plan::PROVIDER;
    let helper = budget.for_helper();
    let parts = plan.sections.len();
    tracing::info!(parts, on_plan, "Chat: a research's helpers begin");
    if let Some(c) = crew {
        c.tell();
    }
    let work = (0..parts).map(|part| async move {
        let member = crew.map(|c| c.member(part));
        let sink = member.as_ref().map(|m| m as &dyn Sink);
        let found = if on_plan { on_plan_helper(t, plan, part, day, helper, sink).await } else { on_key(t, conv, fitted, plan, part, day, helper, sink.map(|s| Live { sink: s, beside })).await };
        if let Some(c) = crew {
            c.finished(part);
        }
        found
    });
    let found: Vec<Found> = futures_util::stream::iter(work).buffered(helpers::AT_ONCE).collect().await;
    let failed = found.iter().filter(|f| f.failed.is_some()).count();
    tracing::info!(parts, failed, secs = budget.elapsed().as_secs(), "Chat: a research's helpers are done");
    found
}

/// **A helper on a key**: Yap's tool loop with only the brief (no history:
/// a context of its own), a web side of its own at the research's reach,
/// only the web's tools (and the notes' reads) running.
#[allow(clippy::too_many_arguments)]
async fn on_key(t: &Asked<'_>, conv: &crate::chat::chats::Conversation, fitted: &Fitted, plan: &Plan, part: usize, day: &str, budget: Budget, live: Option<Live<'_>>) -> Found {
    let ep = &t.ep;
    let mut web = super::super::run::web_turn(t, conv, &fitted.history).research(budget);
    let extra = crate::chat::tools::Extra { research: Some(budget), room: Some(fitted.budget.request), strong: t.gates.strong, ..Default::default() };
    let looped = crate::chat::tools::run_tool_loop_with(
        &helpers::system(false),
        &[],
        &helpers::brief(plan, part, &t.text, day, false),
        &[],
        &ep.base_url,
        &ep.api_key,
        &ep.model,
        &ep.provider,
        ep.disable_thinking,
        false,
        &|_| {},
        Some(&mut web),
        crate::chat::tools::FirstStep::Model,
        extra,
        live,
    )
    .await;
    match looped {
        Ok((reply, chips)) => Found { part, findings: crate::web::bracket_citations(&reply), chips, sources: web.sources.clone(), failed: None },
        Err(e) => {
            tracing::info!(part, error = %crate::web::short(&e), "Chat: a research helper didn't finish");
            Found { part, sources: web.sources.clone(), failed: Some(crate::web::short(&e)), ..Default::default() }
        }
    }
}

/// **A helper's Claude Code** (pure): on the plan, the chat's model and
/// effort, only WebSearch and WebFetch (allowed, never asking), no server,
/// none of the person's settings, `system` in place of its own prompt, in
/// Yap's empty folder, nothing saved.
fn helper_spec(model: &str, effort: Option<crate::ai::claude_code::session::Effort>, system: &str, cwd: std::path::PathBuf) -> Spec {
    let mut spec = Spec::new(cwd, Pay::Plan);
    spec.persist = false;
    spec.model = Some(model.to_string()).filter(|m| !m.trim().is_empty());
    spec.effort = effort;
    spec.permission_mode = PermissionMode::DontAsk;
    let web: Vec<String> = vec!["WebSearch".into(), "WebFetch".into()];
    spec.tools = Some(web.clone());
    spec.allowed_tools = web;
    spec.strict_mcp_config = true;
    spec.setting_sources = Some(Vec::new());
    spec.system_prompt = Some(system.to_string());
    spec
}

/// What a helper's Claude Code shows: its chips (counted on the crew's
/// line) and the searches and reads it begins (the line's step); its words
/// are kept for its findings.
struct HelperOut<'a> {
    sink: Option<&'a dyn Sink>,
    said: String,
}

impl map::Out for HelperOut<'_> {
    fn delta(&mut self, d: crate::ai::llm::Delta<'_>) {
        match d {
            crate::ai::llm::Delta::Content(text) => self.said.push_str(text),
            crate::ai::llm::Delta::ServerInput { name, input } => {
                if let Some(s) = self.sink {
                    let tool = if name == "web_fetch" { "read_page" } else { "web_search" };
                    s.send(Shown::Step { step: crate::chat::activity::of_call(tool, input) });
                }
            }
            _ => {}
        }
    }

    fn new_step(&mut self) {
        // (Words before a tool call aren't the findings.)
        self.said.clear();
    }

    fn chips(&mut self, chips: &[String], sources: &[crate::chat::chats::Source], web: bool) {
        if let Some(s) = self.sink {
            s.send(Shown::Chips { chips: chips.to_vec(), sources: sources.to_vec(), web });
        }
    }
}

/// How a helper's turn on the plan ended.
enum Ended {
    Done(Box<crate::ai::claude_code::events::TurnResult>),
    Failed(String),
    /// The research's time is up: its findings next.
    Time,
}

/// Read a helper's turn to its result (or a problem, its end, nothing heard
/// for [`IDLE`], the research's time when `time` is given).
async fn read(rx: &mut tokio::sync::mpsc::UnboundedReceiver<Event>, mapper: &mut Mapper, out: &mut HelperOut<'_>, time: Option<Duration>) -> Ended {
    let idle = tokio::time::sleep(IDLE);
    tokio::pin!(idle);
    let up = tokio::time::sleep(time.unwrap_or(Duration::MAX / 4));
    tokio::pin!(up);
    loop {
        tokio::select! {
            ev = rx.recv() => {
                let Some(ev) = ev else { return Ended::Failed("its Claude Code ended before it answered".into()) };
                idle.as_mut().reset(tokio::time::Instant::now() + IDLE);
                if let Event::RateLimit(r) = &ev {
                    plan::usage::live(r);
                }
                match mapper.feed(&ev, out) {
                    Fed::More => {}
                    Fed::Done(r) if r.is_error => return Ended::Failed(r.errors.first().cloned().filter(|e| !e.is_empty()).unwrap_or_else(|| "Claude Code couldn't answer".into())),
                    Fed::Done(r) => return Ended::Done(r),
                    Fed::Problem(p) => return Ended::Failed(p.message()),
                    Fed::Exited(_) => return Ended::Failed("its Claude Code ended before it answered".into()),
                }
            }
            _ = &mut idle => return Ended::Failed("its Claude Code went quiet".into()),
            _ = &mut up, if time.is_some() => return Ended::Time,
        }
    }
}

/// A helper's turn's cost and tokens, counted as the research's
/// (`before`: its earlier turn's total, after a stop at the time).
fn count(r: &crate::ai::claude_code::events::TurnResult, model: &str, before: Option<f64>) {
    let own = r.cost_since(before, model);
    let counted = crate::ai::usage::Call { provider: plan::PROVIDER, model, base_url: plan::BASE_URL, turn: true, feature: None };
    crate::ai::usage::record_response(&counted, &oneshot::usage_value(&r.usage, own), None, None);
}

/// **A helper on your Claude plan**: a Claude Code of its own
/// ([`helper_spec`]) given the brief and the question's screenshots; past
/// the research's time, stopped and asked for its findings now.
async fn on_plan_helper(t: &Asked<'_>, plan_: &Plan, part: usize, day: &str, budget: Budget, sink: Option<&dyn Sink>) -> Found {
    let model = t.ep.model.clone();
    let fail = |why: String, mapper: &Mapper| {
        tracing::info!(part, "Chat: a research helper on the Claude plan didn't finish");
        Found { part, chips: mapper.chips().to_vec(), sources: mapper.sources().to_vec(), failed: Some(why), ..Default::default() }
    };
    let mut mapper = Mapper::new();
    let cwd = match oneshot::folder("research") {
        Ok(d) => d,
        Err(e) => return fail(e, &mapper),
    };
    let spec = helper_spec(&model, oneshot::effort_for(&model), &helpers::system(true), cwd);
    let (session, mut rx) = match Session::start(spec) {
        Ok(s) => s,
        Err(e) => return fail(e, &mapper),
    };
    let images = if crate::snip::vision::reads_images(plan::PROVIDER, &model) == Some(false) {
        Vec::new()
    } else {
        let mut msgs = vec![serde_json::json!({ "role": "user", "content": "" })];
        crate::snip::turn::attach(&mut msgs);
        oneshot::images_of(&msgs[0]["content"])
    };
    if let Err(e) = session.send_user(&helpers::brief(plan_, part, &t.text, day, true), &images) {
        return fail(e, &mapper);
    }
    let mut out = HelperOut { sink, said: String::new() };
    let answered = |m: &Mapper| if m.model().is_empty() { model.clone() } else { m.model().to_string() };
    let result = match read(&mut rx, &mut mapper, &mut out, Some(budget.caps.time.saturating_sub(budget.elapsed()))).await {
        Ended::Done(r) => r,
        Ended::Failed(why) => {
            session.kill();
            return fail(why, &mapper);
        }
        Ended::Time => {
            // Stopped, read to its end, then asked for its findings now.
            let _ = session.interrupt();
            let first = tokio::time::timeout(DRAIN, async {
                while let Some(ev) = rx.recv().await {
                    match ev {
                        Event::Result(r) => return Some(r),
                        Event::Exited(_) => return None,
                        _ => {}
                    }
                }
                None
            })
            .await
            .ok()
            .flatten();
            let Some(first) = first else {
                session.kill();
                return fail("the research's time ran out".into(), &mapper);
            };
            count(&first, &answered(&mapper), None);
            out.said.clear();
            if session.send_user(helpers::WRITE_NOW, &[]).is_err() {
                return fail("the research's time ran out".into(), &mapper);
            }
            match read(&mut rx, &mut mapper, &mut out, Some(crate::chat::research::GRACE)).await {
                Ended::Done(r) => {
                    count(&r, &answered(&mapper), Some(first.cost_usd.max(0.0)));
                    session.end();
                    let text = if r.text.trim().is_empty() { out.said.clone() } else { r.text.clone() };
                    return Found { part, findings: text, chips: mapper.chips().to_vec(), sources: mapper.sources().to_vec(), failed: None };
                }
                _ => {
                    session.kill();
                    return fail("the research's time ran out".into(), &mapper);
                }
            }
        }
    };
    count(&result, &answered(&mapper), None);
    session.end();
    let text = if result.text.trim().is_empty() { out.said.clone() } else { result.text.clone() };
    Found { part, findings: text, chips: mapper.chips().to_vec(), sources: mapper.sources().to_vec(), failed: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_helpers_claude_code_has_only_the_webs_tools_nothing_of_the_persons_and_saves_nothing() {
        let s = helper_spec("claude-opus-5-5", None, "You're a research helper.", std::path::PathBuf::from(r"C:\yap\claude\research"));
        assert_eq!(s.pay, Pay::Plan);
        assert_eq!(s.permission_mode, PermissionMode::DontAsk);
        assert_eq!(s.tools.as_deref(), Some(&["WebSearch".to_string(), "WebFetch".to_string()][..]));
        assert_eq!(s.allowed_tools, ["WebSearch", "WebFetch"]);
        for never in ["Task", "Agent", "Bash", "Edit", "Write", "Read"] {
            assert!(!s.allowed_tools.iter().any(|t| t == never), "{never}");
        }
        assert!(s.strict_mcp_config && s.mcp_config.is_none(), "no server");
        assert_eq!(s.setting_sources, Some(Vec::new()), "none of the person's settings");
        assert!(!s.persist, "nothing saved");
        assert_eq!(s.system_prompt.as_deref(), Some("You're a research helper."));
        assert_eq!(s.model.as_deref(), Some("claude-opus-5-5"));
    }

    #[test]
    fn a_helpers_words_after_its_last_search_are_its_findings_and_its_pages_counted() {
        use crate::ai::claude_code::events::parse_line;
        let mut mapper = Mapper::new();
        let mut out = HelperOut { sink: None, said: String::new() };
        let lines = [
            r#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"m1","usage":{"input_tokens":10,"output_tokens":0}}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Let me search."}}}"#,
            r#"{"type":"stream_event","event":{"type":"message_start","message":{"id":"m2","usage":{"input_tokens":10,"output_tokens":0}}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Potassium-40 [1](https://example.org/k40)."}}}"#,
        ];
        for l in lines {
            let ev = parse_line(l).expect("a line");
            mapper.feed(&ev, &mut out);
        }
        assert_eq!(out.said, "Potassium-40 [1](https://example.org/k40).");
    }
}

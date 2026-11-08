//! The connectors' tools in Chat's loop: who gets them, which are offered,
//! their names and definitions in each of the budget's modes, a call's
//! consent and card against the HTTP fake (`integrations/connectors/fake.rs`)
//! — Allow once, Always allow, Deny, outside content, nobody to ask, Off —,
//! a stopped call, and the card's arguments.

use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};

use super::*;
use crate::chat::pages::act::confirm::{chat_connector_confirm, Card};
use crate::chat::stream::{Event, Live, Sink};
use crate::integrations::connectors::fake::{self, Mode};
use crate::integrations::connectors::{connector_add, connector_set_tool, Spec};

#[derive(Default)]
struct Log(Mutex<Vec<Event>>);

impl Sink for Log {
    fn send(&self, ev: Event) {
        self.0.lock().unwrap().push(ev);
    }
}

impl Log {
    fn cards(&self) -> Vec<Card> {
        self.0.lock().unwrap().iter().filter_map(|e| if let Event::Confirm(c) = e { Some(c.clone()) } else { None }).collect()
    }

    fn steps(&self) -> Vec<String> {
        self.0.lock().unwrap().iter().filter_map(|e| if let Event::Step { step } = e { Some(step.line.clone()) } else { None }).collect()
    }
}

fn tool(wire: &str, kind: Kind, setting: Setting, tokens: u64) -> Offered {
    Offered {
        wire: wire.into(),
        id: "c1".into(),
        connector: "Notion".into(),
        tool: wire.split("__").nth(1).unwrap_or("").into(),
        title: Some("A title".into()),
        description: "What it does.".into(),
        schema: json!({ "type": "object", "properties": { "query": { "type": "string" } } }),
        kind,
        setting,
        hash: "h".into(),
        tokens,
    }
}

/// A link connector at the HTTP fake, its tools listed: its id.
async fn added(srv: &fake::HttpFake) -> String {
    crate::integrations::connectors::switch_for_test(true);
    let v = connector_add(Spec { name: "Home lights".into(), url: Some(srv.url("/mcp")), ..Default::default() }).await.unwrap();
    assert_eq!(v.tools.len(), 2, "{v:?}");
    v.id
}

fn chat_turn() -> Turn {
    Turn::of(offered(Surface::Chat, false, false, &Off::new()), 32_768, None, false, Surface::Chat, false, false, "custom").expect("tools offered")
}

/// The next card asked on `log`, answered `reply` from the page.
async fn answer_next(log: &Log, reply: &str) {
    for _ in 0..2_000 {
        if let Some(c) = log.cards().into_iter().rev().find(|c| c.state == "asking") {
            assert!(chat_connector_confirm(c.id, reply.into()));
            return;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    panic!("no card was asked");
}

#[test]
fn only_strong_cloud_models_get_connectors_and_never_on_a_phone() {
    assert!(may("anthropic", true) && may("custom", true) && may("openai", true));
    assert!(!may("anthropic", false), "a model that isn't strong");
    assert!(!may(crate::ai::local_llm::PROVIDER_ONDEVICE, true) && !may("local", true), "a model on this PC: after an evaluation");
    assert!(!may(crate::ai::claude_plan::PROVIDER, true), "the plan's runs: through Yap's endpoint, later");
    assert_eq!(Surface::of(None), Surface::Chat);
    assert_eq!(Surface::of(Some("assistant")), Surface::Voice);
    let phone = tauri::async_runtime::block_on(crate::privacy::net::within(crate::privacy::net::Feature::AssistantPhone, async { Surface::of(Some("assistant")) }));
    assert_eq!(phone, Surface::Phone);
}

#[tokio::test]
async fn the_offer_follows_the_switch_the_settings_where_and_meetings() {
    // The switch off: nothing, and nothing read.
    crate::integrations::connectors::switch_for_test(false);
    assert!(offered(Surface::Chat, false, false, &Off::new()).is_empty());
    assert!(!crate::integrations::connectors::store::touched());
    let srv = fake::http(Mode::Answer);
    let id = added(&srv).await;
    let wires = |s, m, a| offered(s, m, a, &Off::new()).into_iter().map(|t| t.wire).collect::<Vec<_>>();
    // Named as Settings shows them; Always allow and Ask first both offered.
    assert_eq!(wires(Surface::Chat, false, false), ["homelights__list_lights", "homelights__turn_off"]);
    assert!(wires(Surface::Chat, false, false).iter().all(|w| crate::integrations::connectors::names::provider_safe(w)));
    assert_eq!(wires(Surface::Voice, false, false).len(), 2);
    // The phone: only once its `where.phone` is on (its reads are the turn's, below).
    assert!(wires(Surface::Phone, false, false).is_empty());
    store::update(&id, |c| c.where_.phone = true).unwrap();
    assert_eq!(wires(Surface::Phone, false, false).len(), 2);
    // A chat that switched it off (its + menu): none of its tools there.
    let off: Off = [id.clone()].into();
    assert!(offered(Surface::Chat, false, false, &off).is_empty());
    assert!(offered(Surface::Voice, false, false, &off).is_empty());
    // Off: never offered.
    connector_set_tool(id.clone(), "turn_off".into(), "off".into()).await.unwrap();
    assert_eq!(wires(Surface::Chat, false, false), ["homelights__list_lights"]);
    // During a meeting: only with connectors allowed then and this one's own `where.meetings`.
    assert!(wires(Surface::Chat, true, false).is_empty());
    assert!(wires(Surface::Chat, true, true).is_empty(), "the switch alone isn't enough");
    store::update(&id, |c| c.where_.meetings = true).unwrap();
    assert!(wires(Surface::Chat, true, false).is_empty(), "nor its own alone");
    assert_eq!(wires(Surface::Chat, true, true).len(), 1);
    // Where it may be used: Chat, the Voice Assistant.
    store::update(&id, |c| c.where_.chat = false).unwrap();
    assert!(wires(Surface::Chat, false, false).is_empty());
    assert_eq!(wires(Surface::Voice, false, false).len(), 1);
    // Turned off as a whole: nothing.
    store::update(&id, |c| c.enabled = false).unwrap();
    assert!(wires(Surface::Voice, false, false).is_empty());
}

#[test]
fn within_the_budget_every_tool_past_it_tool_search() {
    // A few tools: each as it is, with its connector, its schema and whether Yap asks.
    let few = Turn::of(vec![tool("notion__search", Kind::Reads, Setting::Allow, 60), tool("notion__create_pages", Kind::Changes, Setting::Ask, 90)], 32_768, None, false, Surface::Chat, false, false, "custom").unwrap();
    assert_eq!(few.mode(), budget::Mode::All);
    let d = few.definitions();
    assert_eq!(d.len(), 2);
    assert_eq!(d[0]["function"]["name"], "notion__search");
    assert_eq!(d[0]["function"]["parameters"]["properties"]["query"]["type"], "string");
    assert!(d[0]["function"]["description"].as_str().unwrap().starts_with("Notion: A title. What it does."));
    assert!(d[1]["function"]["description"].as_str().unwrap().ends_with("(Yap asks the user first.)"));
    assert!(d.iter().all(|t| t.get("defer_loading").is_none()));
    assert!(few.server_tool().is_none() && few.owns("notion__search") && !few.owns(FIND));
    // Many: find_tools and use_tool in their place, the same every step.
    let many: Vec<Offered> = (0..300).map(|i| tool(&format!("notion__tool_{i}"), Kind::Reads, Setting::Allow, 40)).collect();
    let searched = Turn::of(many.clone(), 32_768, None, false, Surface::Chat, false, false, "custom").unwrap();
    assert_eq!(searched.mode(), budget::Mode::Search);
    let names: Vec<Value> = searched.definitions().iter().map(|t| t["function"]["name"].clone()).collect();
    assert_eq!(names, [json!(FIND), json!(USE)]);
    assert!(searched.definitions()[0]["function"]["description"].as_str().unwrap().contains("(Notion)"));
    assert_eq!(searched.definitions(), searched.definitions());
    assert!(searched.owns(FIND) && searched.owns(USE) && searched.owns("notion__tool_7"));
    // A free plan's tokens a minute: tool search sooner.
    assert_eq!(Turn::of(many[..30].to_vec(), 131_072, Some(7_200), false, Surface::Chat, false, false, "groq").unwrap().mode(), budget::Mode::Search);
    // Claude on the Messages API: every tool deferred behind its own tool search.
    let claude = Turn::of(many, 1_000_000, None, true, Surface::Chat, false, false, "anthropic").unwrap();
    assert_eq!(claude.mode(), budget::Mode::Deferred);
    assert!(claude.definitions().iter().all(|t| t["defer_loading"] == true));
    assert_eq!(claude.server_tool(), Some(json!({ "type": "tool_search_tool_bm25_20251119", "name": TOOL_SEARCH })));
    assert!(!claude.owns(FIND));
    // No tools: no turn.
    assert!(Turn::of(Vec::new(), 32_768, None, false, Surface::Chat, false, false, "custom").is_none());
}

#[tokio::test]
async fn find_tools_gives_the_matches_definitions_and_use_tool_runs_one() {
    let srv = fake::http(Mode::Answer);
    added(&srv).await;
    let mut tools = offered(Surface::Chat, false, false, &Off::new());
    // (Made many: tool search.)
    for i in 0..200 {
        let mut t = tools[0].clone();
        t.wire = format!("homelights__extra_{i}");
        t.tool = format!("extra_{i}");
        t.description = format!("Unrelated number {i}.");
        tools.push(t);
    }
    let turn = Turn::of(tools, 8_192, None, false, Surface::Chat, false, false, "custom").unwrap();
    assert_eq!(turn.mode(), budget::Mode::Search);
    let log = Log::default();
    let live = Some(Live { sink: &log, beside: None });
    let ctx = Ctx { question: "List my lights", own: &[], outside: false, live };
    let found = turn.execute(FIND, &json!({ "query": "list the lights" }), ctx).await;
    assert!(!found.reached, "a search of Yap's own list: nothing from a server");
    assert_eq!(found.result["data"]["tools"][0]["name"], "homelights__list_lights");
    assert!(found.result["data"]["tools"][0]["parameters"].is_object());
    assert_eq!(found.result["displayText"], "Looked for a connector's tool · list the lights");
    let ran = turn.execute(USE, &json!({ "tool": "homelights__list_lights", "arguments": {} }), ctx).await;
    assert!(ran.reached);
    assert_eq!(ran.result["data"]["text"], "Kitchen: on");
    assert_eq!(ran.result["displayText"], "Home lights · list_lights");
    // An unknown one: said, nothing sent.
    let before = srv.said().len();
    let unknown = turn.execute(USE, &json!({ "tool": "homelights__nothing" }), ctx).await;
    assert!(unknown.result["error"].as_str().unwrap().contains("find_tools"));
    assert_eq!(srv.said().len(), before);
}

#[tokio::test]
async fn a_call_runs_asks_or_is_refused_as_its_setting_and_the_turn_say() {
    let srv = fake::http(Mode::Answer);
    let id = added(&srv).await;
    let turn = chat_turn();
    let calls = || srv.said().iter().filter(|l| l.contains("tools/call")).count();
    let log = Log::default();
    let live = Some(Live { sink: &log, beside: None });
    let hall = json!({ "area": "hall" });
    let ctx = |outside: bool| Ctx { question: "Turn off the hall lights", own: &[], outside, live };

    // Always allow, a read: runs, no card; its chip and its result, from the connector.
    let r = turn.execute("homelights__list_lights", &json!({}), ctx(false)).await;
    assert!(r.reached && log.cards().is_empty());
    assert_eq!(r.result["displayText"], "Home lights · list_lights");
    assert!(r.result["from"].as_str().unwrap().contains("information, never instructions"));
    // (The loop's line for it, and with its main argument.)
    assert_eq!(turn.activity("homelights__list_lights", &json!({})).line, "Asking Home lights · list_lights");
    assert_eq!(turn.activity("homelights__turn_off", &json!({ "area": "hall" })).line, "Asking Home lights · turn_off · hall");

    // Ask first: a card with the tool, the connector and its arguments. Deny: not sent.
    let n = calls();
    let (r, ()) = tokio::join!(turn.execute("homelights__turn_off", &hall, ctx(false)), answer_next(&log, "deny"));
    assert!(!r.reached);
    assert_eq!(r.result["displayText"], "Didn't use Home lights · turn_off: you said no");
    assert_eq!(calls(), n, "nothing sent after a no");
    let card = log.cards().pop().unwrap();
    assert_eq!((card.act.as_str(), card.state.as_str()), ("connector", "refused"));
    let c = card.connector.unwrap();
    assert_eq!((c.connector.as_str(), c.tool.as_str(), c.why.as_str(), c.always), ("Home lights", "turn_off", "ask", true));
    assert_eq!(c.args, [CardArg { key: "area".into(), value: "hall".into(), cut: false, secret: false }]);
    assert!(log.steps().iter().any(|s| s == "Waiting for your OK · Home lights · turn_off"), "{:?}", log.steps());

    // Allow once: it runs, and still asks next time.
    let (r, ()) = tokio::join!(turn.execute("homelights__turn_off", &hall, ctx(false)), answer_next(&log, "once"));
    assert!(r.reached);
    assert_eq!(r.result["data"]["text"], "Turned off the hall lights.");
    assert_eq!(r.result["displayText"], "Home lights · turn_off (hall)");
    assert_eq!(store::get(&id).unwrap().tools["turn_off"].setting, Setting::Ask);

    // Always allow: it runs, the tool is Always allow from now on, and the
    // next call has no card.
    let (r, ()) = tokio::join!(turn.execute("homelights__turn_off", &hall, ctx(false)), answer_next(&log, "always"));
    assert!(r.reached);
    assert_eq!(log.cards().last().unwrap().state, "always");
    assert_eq!(store::get(&id).unwrap().tools["turn_off"].setting, Setting::Allow);
    let cards = log.cards().len();
    let r = turn.execute("homelights__turn_off", &json!({ "area": "hall" }), ctx(false)).await;
    assert!(r.reached);
    assert_eq!(log.cards().len(), cards, "no card once Always allow");

    // After outside content, a write asks even when Always allow (and Always
    // allow isn't on that card: it is already).
    let (r, ()) = tokio::join!(turn.execute("homelights__turn_off", &hall, ctx(true)), answer_next(&log, "once"));
    assert!(r.reached);
    let c = log.cards().pop().unwrap().connector.unwrap();
    assert_eq!((c.why.as_str(), c.always), ("outside", false));
    // …and a read whose words aren't the person's asks too; theirs runs.
    let cards = log.cards().len();
    let r = turn.execute("homelights__list_lights", &json!({ "area": "hall" }), ctx(true)).await;
    assert!(r.reached);
    assert_eq!(log.cards().len(), cards, "the person's own words run");

    // Nobody watching the turn: what would ask is refused, with the reason.
    let nobody = Ctx { live: None, ..ctx(true) };
    let n = calls();
    let r = turn.execute("homelights__turn_off", &json!({ "area": "hall" }), nobody).await;
    assert_eq!(r.result["displayText"], "Didn't use Home lights · turn_off: it needs your OK in Chat");
    assert_eq!(calls(), n);

    // Off since the turn began: refused, nothing sent.
    connector_set_tool(id.clone(), "turn_off".into(), "off".into()).await.unwrap();
    let r = turn.execute("homelights__turn_off", &json!({ "area": "hall" }), ctx(false)).await;
    assert_eq!(r.result["displayText"], "Didn't use Home lights · turn_off: it's off");
    assert_eq!(calls(), n);
}

#[tokio::test]
async fn asked_out_loud_the_card_says_so_and_a_spoken_yes_may_answer_it_but_never_in_a_meeting() {
    let srv = fake::http(Mode::Answer);
    added(&srv).await;
    let hall = json!({ "area": "hall" });
    for (meeting, spoken) in [(false, true), (true, false)] {
        let turn = Turn::of(offered(Surface::Voice, false, false, &Off::new()), 32_768, None, false, Surface::Voice, meeting, false, "custom").unwrap();
        let log = Log::default();
        let ctx = Ctx { question: "turn off the hall lights", own: &[], outside: false, live: Some(Live { sink: &log, beside: None }) };
        let (r, ()) = tokio::join!(turn.execute("homelights__turn_off", &hall, ctx), answer_next(&log, "deny"));
        assert!(!r.reached);
        let c = log.cards()[0].connector.clone().unwrap();
        // The Voice Assistant's sink says it and puts it on the bar (assistant/connector_ask.rs).
        assert!(c.voice);
        assert_eq!(c.spoken, spoken, "a spoken yes during a meeting: never (meeting {meeting})");
        assert!(log.steps().iter().any(|s| s == "Waiting for your OK · Home lights · turn_off"), "{:?}", log.steps());
    }
    // Typed in Chat: neither.
    let log = Log::default();
    let ctx = Ctx { question: "turn off the hall lights", own: &[], outside: false, live: Some(Live { sink: &log, beside: None }) };
    let typed = chat_turn();
    let (_, ()) = tokio::join!(typed.execute("homelights__turn_off", &hall, ctx), answer_next(&log, "deny"));
    let c = log.cards()[0].connector.clone().unwrap();
    assert!(!c.voice && !c.spoken);
}

#[tokio::test]
async fn from_a_phone_reads_run_and_anything_that_would_ask_is_refused_with_the_phones_line() {
    let srv = fake::http(Mode::Answer);
    let id = added(&srv).await;
    let calls = || srv.said().iter().filter(|l| l.contains("tools/call")).count();
    // `where.phone` off (the default): nothing from the phone.
    assert!(Turn::of(offered(Surface::Phone, false, false, &Off::new()), 32_768, None, false, Surface::Phone, false, false, "custom").is_none());
    store::update(&id, |c| c.where_.phone = true).unwrap();
    let turn = Turn::of(offered(Surface::Phone, false, false, &Off::new()), 32_768, None, false, Surface::Phone, false, false, "custom").unwrap();
    // Only the read set to Always allow is offered; the write is known by its name.
    let names: Vec<Value> = turn.definitions().iter().map(|t| t["function"]["name"].clone()).collect();
    assert_eq!(names, [json!("homelights__list_lights")]);
    assert!(turn.owns("homelights__turn_off"));
    let log = Log::default();
    let ctx = |outside: bool| Ctx { question: "Which of my lights are on?", own: &[], outside, live: Some(Live { sink: &log, beside: None }) };
    // A read runs.
    let r = turn.execute("homelights__list_lights", &json!({}), ctx(false)).await;
    assert!(r.reached);
    assert_eq!(r.result["data"]["text"], "Kitchen: on");
    // A write: refused, no card (none can show on the phone), the phone's line.
    let n = calls();
    let r = turn.execute("homelights__turn_off", &json!({ "area": "hall" }), ctx(false)).await;
    assert!(!r.reached);
    assert_eq!(r.result["displayText"], "Didn't use Home lights · turn_off: it needs your OK on the PC");
    assert!(r.result["error"].as_str().unwrap().contains("OK on the PC"));
    assert!(log.steps().iter().any(|s| s == "That needs your OK on the PC · Home lights · turn_off"), "{:?}", log.steps());
    // A read that would ask (after outside content, words that aren't the person's): refused too.
    let r = turn.execute("homelights__list_lights", &json!({ "area": "hall" }), ctx(true)).await;
    assert_eq!(r.result["displayText"], "Didn't use Home lights · list_lights: it needs your OK on the PC");
    assert_eq!(calls(), n, "nothing sent");
    assert!(log.cards().is_empty());
    // Its read set to Ask first: not offered from the phone either.
    connector_set_tool(id.clone(), "list_lights".into(), "ask".into()).await.unwrap();
    assert!(Turn::of(offered(Surface::Phone, false, false, &Off::new()), 32_768, None, false, Surface::Phone, false, false, "custom").is_none());
}

#[tokio::test]
async fn a_call_dropped_mid_way_is_cancelled_on_the_server() {
    let srv = fake::http(Mode::Slow(400));
    added(&srv).await;
    let turn = chat_turn();
    let ctx = Ctx { question: "List my lights", own: &[], outside: false, live: None };
    // ■ Stop (or a newer question) drops the turn's future mid-call.
    let stopped = tokio::time::timeout(Duration::from_millis(150), turn.execute("homelights__list_lights", &json!({}), ctx)).await;
    assert!(stopped.is_err());
    let mut told = false;
    for _ in 0..200 {
        if srv.said().iter().any(|l| l.contains("notifications/cancelled")) {
            told = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(told, "the server told: {:?}", srv.said());
}

#[test]
fn a_cards_arguments_are_readable_with_secrets_masked_and_long_values_cut() {
    let body = (1..=10).map(|i| format!("Line {i}")).collect::<Vec<_>>().join("\n");
    let args = json!({
        "title": "Launch notes",
        "body": body,
        "api_key": "abc",
        "auth": "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJ4In0.c2lnbmF0dXJlc2lnbmF0dXJl",
        "note": "ghp_FAKEfake1234567890abcdef",
        "tags": ["a", "b"],
        "count": 3,
    });
    // Its main argument first, then the rest.
    let listed: Vec<String> = card_args(&args).into_iter().map(|a| a.key).collect();
    assert_eq!(listed, ["title", "api_key", "auth", "body", "count", "note", "tags"]);
    let shown: std::collections::BTreeMap<String, CardArg> = card_args(&args).into_iter().map(|a| (a.key.clone(), a)).collect();
    assert_eq!(shown["title"].value, "Launch notes");
    assert!(shown["body"].cut && shown["body"].value.starts_with("Line 1\n") && shown["body"].value.ends_with("Line 6…"));
    for k in ["api_key", "auth", "note"] {
        assert!(shown[k].secret && shown[k].value == "••••", "{k}");
    }
    assert_eq!(shown["tags"].value, "[\"a\",\"b\"]");
    assert_eq!(shown["count"].value, "3");
    // Many: the first twelve, then how many more.
    let many: serde_json::Map<String, Value> = (0..15).map(|i| (format!("k{i:02}"), json!("v"))).collect();
    let listed = card_args(&Value::Object(many));
    assert_eq!(listed.len(), 13);
    assert_eq!(listed[12].value, "3 more");
    // A chip's main argument is never a secret.
    assert_eq!(main_arg(&json!({ "token": "ghp_FAKEfake1234567890abcdef", "query": "launch plan" })), Some("launch plan".into()));
    assert_eq!(main_arg(&json!({ "password": "hunter2hunter2" })), None);
    assert_eq!(main_arg(&json!({ "count": 3 })), None);
}

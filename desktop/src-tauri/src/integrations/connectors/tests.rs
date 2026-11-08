//! The connectors' tests across their parts: the switch, a program's
//! consent, calls (timeouts, stops, progress, results) against `fake.rs`'s
//! server, and links against its HTTP fake (a token, a 401, a redirect off
//! the host, no answer, slow, the older HTTP+SSE, a name that resolves to
//! this PC) with their receipts.

use std::cell::Cell;
use std::time::Duration;

use serde_json::{json, Value};

use super::call::{self, CallError, Stop};
use super::fake::{self, Fake, Mode};
use super::manager::{self, Limits, Manager, Status};
use super::*;

thread_local! {
    static SWITCH: Cell<bool> = const { Cell::new(false) };
}

/// The preview's switch, per test thread (off unless a test turns it on).
pub fn switch() -> bool {
    SWITCH.with(Cell::get)
}

fn on() {
    SWITCH.with(|s| s.set(true));
    store::reset_for_test();
}

/// The switch, for another module's tests (`connectors::switch_for_test`).
pub(super) fn set_switch(on: bool) {
    SWITCH.with(|s| s.set(on));
}

fn link(url: &str) -> Spec {
    Spec { name: "Fake link".into(), url: Some(url.into()), ..Default::default() }
}

#[tokio::test]
async fn with_the_switch_off_nothing_is_read_started_or_sent() {
    SWITCH.with(|s| s.set(false));
    store::reset_for_test();
    let srv = fake::http(Mode::Answer);
    let list = connectors_list().await;
    assert!(!list.enabled && list.connectors.is_empty());
    assert_eq!(connector_add(link(&srv.url("/mcp"))).await.unwrap_err(), OFF);
    assert_eq!(connector_preview(link(&srv.url("/mcp"))).await.unwrap_err(), OFF);
    assert_eq!(connector_refresh_tools("c1".into()).await.unwrap_err(), OFF);
    assert_eq!(connector_set_enabled("c1".into(), true).await.unwrap_err(), OFF);
    assert_eq!(connector_remove("c1".into()).await.unwrap_err(), OFF);
    assert_eq!(connector_details("c1".into()).await.unwrap_err(), OFF);
    assert_eq!(connector_call_tool("c1".into(), "list_lights".into(), None, None).await.unwrap_err(), OFF);
    assert!(!store::touched(), "connectors.json never read");
    assert!(srv.said().is_empty(), "no request made");
}

#[test]
fn a_program_is_added_only_as_the_person_saw_it_and_its_secrets_go_to_the_vault() {
    on();
    let dir = std::env::temp_dir().join(format!("yap-connectors-add-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("files-server.exe");
    std::fs::write(&exe, "MZ").unwrap();
    let spec = Spec {
        name: "Files".into(),
        command: Some(exe.to_string_lossy().into_owned()),
        args: vec!["--stdio".into()],
        env: vec![
            Pair { name: "GITHUB_TOKEN".into(), value: "ghp_FAKE_secret".into(), secret: None },
            Pair { name: "LOG_LEVEL".into(), value: "info".into(), secret: None },
        ],
        ..Default::default()
    };
    let p = preview(&spec).unwrap();
    let program = p.program.as_ref().unwrap();
    assert_eq!(program.command_line, format!("{} --stdio", exe.display()));
    assert!(p.warnings[0].contains("your permissions"));
    // Not shown first (or shown something else): refused.
    assert!(add(&spec).unwrap_err().contains("look at it again"));
    let wrong = Spec { approved: Some("0".repeat(24)), ..spec.clone() };
    assert!(add(&wrong).is_err());
    let c = add(&Spec { approved: Some(program.fingerprint.clone()), ..spec.clone() }).unwrap();
    let Transport::Local { env, approved, .. } = &c.transport else { panic!("a program") };
    assert_eq!(approved, &program.fingerprint);
    assert!(env[0].secret && env[0].value.is_none(), "the token's value isn't in the store");
    assert_eq!(env[1].value.as_deref(), Some("info"));
    assert_eq!(vault::load(&c.id).env.get("GITHUB_TOKEN").map(String::as_str), Some("ghp_FAKE_secret"));
    let stored = serde_json::to_string(&store::list()).unwrap();
    assert!(!stored.contains("ghp_FAKE"), "{stored}");
    // Yap's own MCP server can't be added; YAP_ variables are Yap's.
    let own = std::env::current_exe().unwrap();
    let me = Spec { name: "Yap".into(), command: Some(own.to_string_lossy().into_owned()), args: vec!["mcp".into()], ..Default::default() };
    assert!(preview(&me).unwrap_err().contains("Yap's own"));
    let yap_var = Spec { env: vec![Pair { name: "YAP_E2E".into(), value: "1".into(), secret: Some(false) }], ..spec };
    assert!(preview(&yap_var).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn secret_names_are_locked_by_default() {
    for name in ["GITHUB_TOKEN", "API_KEY", "OPENAI_API_KEY", "CLIENT_SECRET", "DB_PASSWORD", "GITHUB_PAT", "HA_AUTH"] {
        assert!(secret_name(name), "{name}");
    }
    for name in ["LOG_LEVEL", "PATH_ROOT", "HOME", "KEYBOARD_LAYOUT_NAME", "PATTERN"] {
        assert!(!secret_name(name), "{name}");
    }
}

// ---- calls, against the fake server ------------------------------------------------------------

#[tokio::test]
async fn a_call_is_shaped_and_a_refused_one_says_why() {
    let (service, _fake) = fake::connected(Fake::default()).await;
    let peer = service.peer().clone();
    let r = call::call(&peer, "list_lights", &json!({}), Duration::from_secs(5), &Stop::new()).await.unwrap();
    assert!(r.success);
    assert_eq!(r.text, "Kitchen: on\nHall: off");
    let r = call::call(&peer, "turn_off", &json!({ "area": "kitchen" }), Duration::from_secs(5), &Stop::new()).await.unwrap();
    assert_eq!(r.text, "Turned off the kitchen lights.");
    let r = call::call(&peer, "picture", &Value::Null, Duration::from_secs(5), &Stop::new()).await.unwrap();
    assert_eq!(r.text, "The floor plan:\n\n[a picture, 2\u{d7}1]");
    assert_eq!(r.pictures.len(), 1);
    match call::call(&peer, "no_such_tool", &json!({}), Duration::from_secs(5), &Stop::new()).await {
        Err(CallError::Refused(m)) => assert!(m.contains("Unknown tool"), "{m}"),
        other => panic!("{other:?}"),
    }
    // The server's own instructions are never kept: Yap's view of it has none.
    let live = manager::Live::new(service);
    assert_eq!((live.server.name.as_str(), live.server.version.as_str()), ("fake", "1.0"));
}

async fn until(f: impl Fn() -> bool) -> bool {
    for _ in 0..100 {
        if f() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

#[tokio::test]
async fn a_slow_call_times_out_and_the_server_is_told() {
    let (service, fake) = fake::connected(Fake::default()).await;
    let peer = service.peer().clone();
    let r = call::call(&peer, "slow", &json!({ "ms": 3_000 }), Duration::from_millis(200), &Stop::new()).await;
    assert_eq!(r.unwrap_err(), CallError::TimedOut(Duration::from_millis(200)));
    assert!(until(|| fake.said().contains(&"cancelled slow".to_string())).await, "{:?}", fake.said());
    // The connection carries on.
    assert!(call::call(&peer, "list_lights", &json!({}), Duration::from_secs(5), &Stop::new()).await.unwrap().success);
}

#[tokio::test]
async fn progress_starts_the_clock_again() {
    let (service, _fake) = fake::connected(Fake::default()).await;
    let r = call::call(service.peer(), "slow", &json!({ "ms": 900, "every": 100 }), Duration::from_millis(400), &Stop::new()).await;
    assert_eq!(r.unwrap().text, "Done slowly.");
}

#[tokio::test]
async fn a_stop_or_a_dropped_call_cancels_it_on_the_server() {
    let (service, fake) = fake::connected(Fake::default()).await;
    let peer = service.peer().clone();
    // ■ Stop.
    let stop = Stop::new();
    let s = stop.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        s.cancel();
    });
    let r = call::call(&peer, "slow", &json!({ "ms": 5_000 }), Duration::from_secs(30), &stop).await;
    assert_eq!(r.unwrap_err(), CallError::Stopped);
    assert!(until(|| fake.said().iter().filter(|l| *l == "cancelled slow").count() == 1).await, "{:?}", fake.said());
    // A turn dropped mid-call (a newer question): the same.
    let dropped = tokio::time::timeout(Duration::from_millis(150), call::call(&peer, "slow", &json!({ "ms": 5_000 }), Duration::from_secs(30), &Stop::new())).await;
    assert!(dropped.is_err());
    assert!(until(|| fake.said().iter().filter(|l| *l == "cancelled slow").count() == 2).await, "{:?}", fake.said());
    // Stopped before it went out: nothing sent.
    let before = fake.said().len();
    let gone = Stop::new();
    gone.cancel();
    assert_eq!(call::call(&peer, "list_lights", &json!({}), Duration::from_secs(5), &gone).await.unwrap_err(), CallError::Stopped);
    assert_eq!(fake.said().len(), before);
}

#[tokio::test]
async fn a_tool_that_changes_after_it_was_allowed_asks_again() {
    let (service, _fake) = fake::connected(Fake::default()).await;
    let server = manager::Live::new(service);
    let peer = server.peer();
    let first = list(&peer, server.server.clone()).await.unwrap();
    let mut settings = std::collections::BTreeMap::new();
    cache::apply(&mut settings, &first);
    assert_eq!(settings["rug_pull"].setting, Setting::Allow);
    assert_eq!(settings["delete_all"].setting, Setting::Ask, "no annotations: can delete");
    assert_eq!(settings["inject"].setting, Setting::Allow, "its description is plain; what it returns is wave 4's outside content");
    let second = list(&peer, server.server.clone()).await.unwrap();
    let diff = cache::apply(&mut settings, &second);
    assert_eq!(diff.asked_again, ["rug_pull"]);
    assert_eq!(settings["rug_pull"].setting, Setting::Ask);
}

// ---- links, against the HTTP fake ----------------------------------------------------------------

#[tokio::test]
async fn a_link_lists_and_calls_with_its_headers_and_every_request_has_its_receipt() {
    on();
    let srv = fake::http(Mode::Answer);
    let spec = Spec { headers: vec![Pair { name: "X-Api-Key".into(), value: "key-FAKE-123".into(), secret: None }], ..link(&srv.url("/mcp")) };
    let v = connector_add(spec).await.unwrap();
    assert!(matches!(v.status.status, Status::Connected { tools: 2 }), "{:?}", v.status);
    assert_eq!(v.tools.iter().map(|t| t.wire.as_str()).collect::<Vec<_>>(), ["fakelink__list_lights", "fakelink__turn_off"]);
    assert_eq!(v.tools[0].setting, Setting::Allow);
    assert_eq!(v.tools[1].setting, Setting::Ask);
    assert!(v.has_secrets && v.headers == ["X-Api-Key"]);
    let r = call_tool(&v.id, "turn_off", &json!({ "area": "hall" }), None).await.unwrap();
    assert_eq!(r, json!({ "success": true, "data": { "text": "Turned off the hall lights." }, "displayText": "A connector · turn_off" }));
    let said = srv.said();
    assert!(said.iter().all(|l| l.contains("key=yes")), "{said:?}");
    assert!(said.iter().any(|l| l.contains("tools/call")));
    // Receipts: the host, the connector's purpose, never the path or the key.
    let mine: Vec<_> = crate::privacy::net::entries().into_iter().filter(|e| e.label == "Connector · A connector" && e.host == "127.0.0.1").collect();
    assert!(!mine.is_empty());
    assert_eq!(mine[0].feature, "Connectors");
    let text = serde_json::to_string(&mine).unwrap();
    assert!(!text.contains("/mcp") && !text.contains("key-FAKE"));
    // Off: refused before anything is sent.
    let before = srv.said().len();
    connector_set_tool(v.id.clone(), "turn_off".into(), "off".into()).await.unwrap();
    assert_eq!(call_tool(&v.id, "turn_off", &json!({}), None).await.unwrap_err(), "That tool is off.");
    assert_eq!(srv.said().len(), before);
    assert!(connector_set_tool(v.id.clone(), "turn_off".into(), "maybe".into()).await.is_err());
    // Removed: its secrets and tools go.
    connector_remove(v.id.clone()).await.unwrap();
    assert!(vault::load(&v.id).is_empty() && cache::get(&v.id).is_none());
}

#[tokio::test]
async fn a_token_turned_down_says_so_and_no_token_means_a_sign_in() {
    on();
    let srv = fake::http(Mode::Token("good-FAKE"));
    let bad = connector_add(Spec { token: Some("bad-FAKE".into()), ..link(&srv.url("/mcp")) }).await.unwrap();
    match &bad.status.status {
        Status::Failed { why } => assert!(why.contains("turned the token down (401)"), "{why}"),
        other => panic!("{other:?}"),
    }
    let none = connector_add(link(&srv.url("/mcp"))).await.unwrap();
    assert_eq!(none.status.status, Status::NeedsSignIn);
    let good = connector_add(Spec { token: Some("good-FAKE".into()), ..link(&srv.url("/mcp")) }).await.unwrap();
    assert!(matches!(good.status.status, Status::Connected { .. }), "{:?}", good.status);
    assert!(srv.said().iter().any(|l| l.contains("auth=yes")));
}

#[tokio::test]
async fn a_redirect_off_the_host_is_refused() {
    on();
    let srv = fake::http(Mode::RedirectOffHost);
    let v = connector_add(Spec { token: Some("t-FAKE".into()), ..link(&srv.url("/mcp")) }).await.unwrap();
    match &v.status.status {
        Status::Failed { why } => assert!(why.contains("sent Yap on to localhost"), "{why}"),
        other => panic!("{other:?}"),
    }
    // Only the first request was made: the token never followed.
    assert!(srv.said().iter().all(|l| l.starts_with("POST /mcp")), "{:?}", srv.said());
}

/// A manager with a short start limit for links.
fn quick() -> Manager {
    let limits = Limits { start_remote: Duration::from_secs(1), ..Default::default() };
    Manager::new(std::sync::Arc::new(manager::SystemClock), limits, manager::connect_real(), std::sync::Arc::new(|_: &str, _: &Status| {}))
}

#[tokio::test]
async fn a_server_that_never_answers_runs_out_of_time_and_a_slow_one_doesnt() {
    on();
    let hang = fake::http(Mode::Hang);
    let c = add(&link(&hang.url("/mcp"))).unwrap();
    let m = quick();
    let started = std::time::Instant::now();
    let err = m.peer(&c).await.unwrap_err();
    assert_eq!(err, "Couldn't connect: no answer from 127.0.0.1 in 1 s.");
    assert!(started.elapsed() < Duration::from_secs(3));
    let slow = fake::http(Mode::Slow(150));
    let c = add(&link(&slow.url("/mcp"))).unwrap();
    assert!(m.peer(&c).await.is_ok());
}

#[tokio::test]
async fn the_older_http_and_sse_transport_is_tried_when_streamable_http_isnt_there() {
    on();
    let srv = fake::http(Mode::Older);
    let v = connector_add(link(&srv.url("/sse"))).await.unwrap();
    assert!(matches!(v.status.status, Status::Connected { tools: 2 }), "{:?}", v.status);
    let r = call_tool(&v.id, "list_lights", &json!({}), None).await.unwrap();
    assert_eq!(r["data"]["text"], "Kitchen: on");
    let said = srv.said();
    assert!(said.iter().any(|l| l.starts_with("GET /sse")), "{said:?}");
    assert!(said.iter().any(|l| l.starts_with("POST /messages") && l.contains("tools/call")), "{said:?}");
}

#[tokio::test]
async fn a_name_on_the_internet_that_points_at_this_pc_is_refused() {
    let srv = fake::http(Mode::Answer);
    remote::stand_in_dns("rebind.example", format!("127.0.0.1:{}", srv.port).parse().unwrap());
    let url = url::Url::parse(&format!("http://rebind.example:{}/mcp", srv.port)).unwrap();
    let http = remote::Http::new(&url, crate::privacy::net::Place::Internet, "A connector".into(), None, &[]).unwrap();
    let err = manager::serve(remote::transport(http, &url)).await.expect_err("refused");
    assert_eq!(manager::http_error_of(&err), Some(remote::HttpError::Private { host: format!("rebind.example:{}", srv.port) }));
    assert!(srv.said().is_empty(), "nothing reached it");
}

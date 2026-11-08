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
fn one_from_the_catalogue_takes_its_address_and_its_name_in_the_receipts() {
    on();
    // The entry's own address, whatever the page sends; the person's token.
    let s = Spec { name: String::new(), catalog: Some("notion".into()), url: Some("https://mcp.example.com/mcp".into()), ..Default::default() };
    let p = preview(&s).unwrap();
    assert_eq!(p.host.as_deref(), Some("mcp.notion.com"));
    let c = add(&s).unwrap();
    assert_eq!(c.name, "Notion");
    let Transport::Remote { url, fallback_sse, .. } = &c.transport else { panic!("a link") };
    assert_eq!(url, "https://mcp.notion.com/mcp");
    assert_eq!(fallback_sse.as_deref(), Some("https://mcp.notion.com/sse"));
    assert_eq!(c.origin.kind, "catalog");
    assert_eq!(c.origin.extra.get("catalog").and_then(Value::as_str), Some("notion"));
    assert_eq!(c.kind(), "Notion", "the receipts name the catalogue's entry, never what the person typed");
    assert_eq!(view(&c).catalog.as_deref(), Some("notion"));
    // A token entry needs its token; an address the person gives only where it may.
    assert!(add(&Spec { catalog: Some("github".into()), ..Default::default() }).unwrap_err().contains("token first"));
    let gh = add(&Spec { catalog: Some("github".into()), token: Some("ghp_FAKE_tok".into()), ..Default::default() }).unwrap();
    assert_eq!(gh.auth.kind, AuthKind::Bearer);
    assert_eq!(vault::load(&gh.id).token.as_deref(), Some("ghp_FAKE_tok"));
    let ha = add(&Spec { catalog: Some("home-assistant".into()), url: Some("http://192.168.1.40:8123/api/mcp".into()), token: Some("ha_FAKE".into()), ..Default::default() }).unwrap();
    let Transport::Remote { url, .. } = &ha.transport else { panic!("a link") };
    assert_eq!(url, "http://192.168.1.40:8123/api/mcp");
    assert!(add(&Spec { catalog: Some("not-in-the-list".into()), ..Default::default() }).unwrap_err().contains("isn't in Yap's list"));
    // Its tools start as the entry says (only stricter).
    let stripe = catalog::get().get("stripe").unwrap();
    let mut settings = std::collections::BTreeMap::new();
    settings.insert("create_refund".to_string(), store::ToolSetting { setting: Setting::Ask, hash: "h".into() });
    catalog::start_settings(stripe, &["create_refund".to_string()], &mut settings);
    assert_eq!(settings["create_refund"].setting, Setting::Off);
    let stored = serde_json::to_string(&store::list()).unwrap();
    assert!(!stored.contains("FAKE"), "{stored}");
}

#[tokio::test]
async fn a_pasted_setup_is_shown_whole_without_its_secrets_and_added_as_seen() {
    on();
    let exe = std::env::current_exe().unwrap().to_string_lossy().into_owned();
    let text = serde_json::json!({ "mcpServers": {
        "files": { "command": exe, "args": ["--serve"], "env": { "FILES_API_KEY": "sek-FAKE-77", "LOG_LEVEL": "info" } },
        "notes": { "url": "https://mcp.example.com/mcp", "headers": { "Authorization": "Bearer nt-FAKE-1", "X-Team": "t-FAKE-2" } },
        "yap": { "command": "C:\\Program Files\\Yap\\yap.exe", "args": ["mcp"] }
    }})
    .to_string();
    let source = import::Source::Paste { text };
    let scan = connectors_import_scan(source.clone()).await.unwrap();
    assert_eq!(scan.servers.len(), 2);
    assert_eq!(scan.skipped, 1, "Yap's own entry");
    let shown = serde_json::to_string(&scan).unwrap();
    assert!(!shown.contains("FAKE"), "no secret on the page: {shown}");
    let files = &scan.servers[0];
    assert_eq!(files.kind, "local");
    assert!(files.env.iter().any(|v| v.name == "FILES_API_KEY" && v.secret && v.value.is_none()));
    assert!(files.env.iter().any(|v| v.name == "LOG_LEVEL" && v.value.as_deref() == Some("info")));
    assert!(scan.servers[1].token && scan.servers[1].headers == ["X-Team"]);
    let fp = files.preview.as_ref().and_then(|p| p.program.as_ref()).map(|p| p.fingerprint.clone()).unwrap();

    // A program without the fingerprint seen isn't added; with it, it is,
    // its secret in the vault. (Neither connects: no server here.)
    let (specs, missing) = import::specs_for(&source, &[import::Pick { key: "files".into(), approved: None }, import::Pick { key: "gone".into(), approved: None }]).unwrap();
    assert_eq!(missing, ["gone"]);
    assert!(add_as(&specs[0], None).unwrap_err().contains("look at it again"));
    let (specs, _) = import::specs_for(&source, &[import::Pick { key: "files".into(), approved: Some(fp) }, import::Pick { key: "notes".into(), approved: None }]).unwrap();
    let mut extra = serde_json::Map::new();
    extra.insert("from".into(), Value::from("paste"));
    let a = add_as(&specs[0], Some(Origin { kind: "import".into(), extra: extra.clone() })).unwrap();
    let b = add_as(&specs[1], Some(Origin { kind: "import".into(), extra })).unwrap();
    assert_eq!(a.origin.kind, "import");
    assert_eq!(vault::load(&a.id).env.get("FILES_API_KEY").map(String::as_str), Some("sek-FAKE-77"));
    assert_eq!(b.auth.kind, AuthKind::Bearer);
    assert_eq!(vault::load(&b.id).token.as_deref(), Some("nt-FAKE-1"));
    assert_eq!(vault::load(&b.id).headers.get("X-Team").map(String::as_str), Some("t-FAKE-2"));
    let stored = serde_json::to_string(&store::list()).unwrap();
    assert!(!stored.contains("FAKE"), "{stored}");
    // Read again, both are already added.
    let again = import::scan(&source).unwrap();
    assert!(again.servers.iter().all(|s| s.added), "{again:?}");
}

#[tokio::test]
async fn where_it_is_used_changes_chat_voice_and_meetings_never_the_phone() {
    on();
    let c = add(&Spec { name: "Wiki".into(), url: Some("https://mcp.example.com/mcp".into()), ..Default::default() }).unwrap();
    assert_eq!(c.where_, store::Where::default());
    let v = connector_set_where(c.id.clone(), WherePatch { voice: Some(false), meetings: Some(true), ..Default::default() }).await.unwrap();
    assert_eq!(v.where_, store::Where { chat: true, voice: false, phone: false, plan: true, meetings: true });
    // The phone isn't one of its fields: a page can't turn it on yet.
    let patch: WherePatch = serde_json::from_value(json!({ "chat": false, "phone": true })).unwrap();
    let v = connector_set_where(c.id.clone(), patch).await.unwrap();
    assert!(!v.where_.chat && !v.where_.phone);
    assert!(connector_set_where("c99".into(), WherePatch::default()).await.is_err());
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

/// The receipts name the host and "A connector", never the name the person
/// typed, the link's path or query, a token or a header's value (wave 8).
#[tokio::test]
async fn a_links_receipts_hold_its_host_only() {
    on();
    let srv = fake::http(Mode::Answer);
    let spec = Spec {
        name: "Robin's private server".into(),
        url: Some(srv.url("/mcp?workspace=FAKE-ws-7&key=FAKE-query-key")),
        token: Some("tok-FAKE-receipt-1".into()),
        headers: vec![Pair { name: "X-Api-Key".into(), value: "key-FAKE-receipt-2".into(), secret: None }],
        ..Default::default()
    };
    let v = connector_add(spec).await.unwrap();
    assert!(matches!(v.status.status, Status::Connected { .. }), "{:?}", v.status);
    call_tool(&v.id, "list_lights", &json!({}), None).await.unwrap();
    let entries = crate::privacy::net::entries();
    assert!(entries.iter().any(|e| e.kind == "connector" && e.label == "Connector · A connector" && e.host == "127.0.0.1"), "{entries:?}");
    let text = serde_json::to_string(&crate::privacy::net::entries()).unwrap();
    for never in ["Robin", "/mcp", "workspace", "FAKE-ws-7", "FAKE-query-key", "tok-FAKE-receipt-1", "key-FAKE-receipt-2"] {
        assert!(!text.contains(never), "{never} in the receipts: {text}");
    }
    connector_remove(v.id.clone()).await.unwrap();
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

// ---- signing in, against the OAuth fake --------------------------------------------------------------

/// The person's browser: the fake's consent page approved at once, then
/// Yap's loopback (on a thread of its own, as a browser is).
fn browser(link: &str) -> Result<(), String> {
    let link = link.to_string();
    std::thread::spawn(move || fake::browse(&link));
    Ok(())
}

/// A link to the OAuth fake, signed in: its id.
async fn signed_in(srv: &fake::OauthFake) -> String {
    let v = connector_add(link(&srv.url("/mcp"))).await.unwrap();
    assert_eq!(v.status.status, Status::NeedsSignIn);
    assert_eq!(v.status.sign_in, Some(oauth::SignInState::NeedsSignIn));
    let done = sign_in_with(&v.id, &browser).await.unwrap();
    assert_eq!(done.outcome, Outcome::SignedIn);
    v.id
}

#[tokio::test]
async fn signing_in_registers_once_keeps_the_tokens_in_the_vault_connects_and_signing_out_revokes() {
    on();
    let srv = fake::oauth(fake::OauthSetup::default());
    let id = signed_in(&srv).await;
    let v = view(&store::get(&id).unwrap());
    assert!(matches!(v.status.status, Status::Connected { tools: 2 }), "{:?}", v.status);
    assert_eq!(v.status.sign_in, Some(oauth::SignInState::SignedIn));
    let info = v.sign_in.unwrap();
    assert_eq!(info.client, Some(store::ClientKind::Dcr));
    assert!(info.host.as_deref().is_some_and(|h| h.starts_with("127.0.0.1")), "{:?}", info.host);
    assert_eq!(srv.count("register native none"), 1, "a native public client: {:?}", srv.said());
    assert_eq!(srv.count("token code ok"), 1, "PKCE, the redirect and the resource checked: {:?}", srv.said());
    assert!(srv.said().iter().any(|l| l == &format!("authorize read offline_access resource=http://127.0.0.1:{}/mcp", srv.port)), "{:?}", srv.said());
    // The tokens in the vault, bound to their issuer and resource; none in the file.
    let grant = vault::load(&id).grant.unwrap();
    assert!(grant.access.starts_with("at-") && grant.refresh.as_deref().is_some_and(|r| r.starts_with("rt-")));
    assert_eq!(grant.resource, srv.url("/mcp"));
    let file = serde_json::to_string(&store::list()).unwrap();
    assert!(!file.contains(&grant.access) && !file.contains("rt-"), "{file}");
    // Calls carry it.
    let r = call_tool(&id, "list_lights", &json!({}), None).await.unwrap();
    assert_eq!(r["data"]["text"], "Kitchen: on");
    assert!(srv.said().iter().any(|l| l == &format!("mcp tools/call {}", grant.access)));
    // Every request in the receipts, by purpose.
    let entries = crate::privacy::net::entries();
    assert!(entries.iter().any(|e| e.label == "Connector sign-in · A connector"));
    assert!(!serde_json::to_string(&entries).unwrap().contains(&grant.access));

    // Signed in again: the registration reused, never made twice.
    assert_eq!(sign_in_with(&id, &browser).await.unwrap().outcome, Outcome::SignedIn);
    assert_eq!(srv.count("register"), 1);

    // Signed out: the refresh token revoked, the tokens gone, the registration kept.
    let refresh = vault::load(&id).grant.unwrap().refresh.unwrap();
    let out = connector_sign_out(id.clone()).await.unwrap();
    assert!(out.confirmed);
    assert_eq!(out.connector.status.sign_in, Some(oauth::SignInState::NeedsSignIn));
    assert!(srv.said().contains(&format!("revoke refresh_token {refresh} client=dcr-1")), "{:?}", srv.said());
    assert!(vault::load(&id).grant.is_none());
    assert_eq!(store::get(&id).unwrap().auth.registration.map(|r| r.client_id).as_deref(), Some("dcr-1"));
    let before = srv.said().len();
    let r = call_tool(&id, "list_lights", &json!({}), None).await;
    assert!(r.is_err(), "no sign-in: no call");
    assert!(srv.said()[before..].iter().all(|l| !l.contains("at-")), "no token sent after sign-out");
}

#[tokio::test]
async fn one_refresh_at_a_time_and_another_yaps_newer_tokens_are_used() {
    on();
    let srv = fake::oauth(fake::OauthSetup::default());
    let id = signed_in(&srv).await;
    assert_eq!(srv.count("token refresh"), 0, "a fresh token isn't refreshed: {:?}", srv.said());
    // It runs out: two callers at once, one refresh, both get its token.
    let ran_out = |s: &mut vault::Secrets| {
        if let Some(g) = &mut s.grant {
            g.expires = 1;
            g.received = 0;
        }
    };
    vault::update(&id, ran_out).unwrap();
    let (a, b) = tokio::join!(oauth::access_token(&id, None), oauth::access_token(&id, None));
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(srv.count("token refresh"), 1, "{:?}", srv.said());
    assert_eq!(a.0, b.0, "the second used the first's refresh");
    assert!(!srv.said().contains(&"replay".to_string()), "a spent refresh token never sent: {:?}", srv.said());
    vault::update(&id, ran_out).unwrap();

    // Another Yap refreshes while this one waits for the guard: its tokens
    // are read back and used, with no refresh of this Yap's own.
    let held = refresh::acquire(&id).await.unwrap();
    let waiting = tokio::spawn({
        let id = id.clone();
        async move { oauth::access_token(&id, None).await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    let count = srv.count("token refresh");
    vault::update(&id, |s| {
        if let Some(g) = &mut s.grant {
            g.access = "at-another-yap".into();
            g.expires = 4_000_000_000;
            g.received = 3_000_000_000;
        }
    })
    .unwrap();
    drop(held);
    let (t, _) = waiting.await.unwrap().unwrap();
    assert_eq!(t, "at-another-yap");
    assert_eq!(srv.count("token refresh"), count, "nothing refreshed here");
}

#[tokio::test]
async fn a_refreshed_token_rotates_and_a_revoked_grant_ends_the_sign_in() {
    on();
    let srv = fake::oauth(fake::OauthSetup::default());
    let id = signed_in(&srv).await;
    let first = vault::load(&id).grant.unwrap();
    // The server turns the access token down (a 401): one refresh, the call sent again.
    srv.state.lock().unwrap().access.remove(&first.access);
    let r = call_tool(&id, "list_lights", &json!({}), None).await.unwrap();
    assert_eq!(r["data"]["text"], "Kitchen: on", "{r}");
    let second = vault::load(&id).grant.unwrap();
    assert_ne!(second.access, first.access);
    assert_ne!(second.refresh, first.refresh, "the refresh token rotated and was kept at once");
    assert!(srv.said().contains(&format!("token refresh {}", first.refresh.unwrap())));
    // The server revokes everything: the refresh is turned down, the sign-in ends.
    srv.revoke_all();
    let r = call_tool(&id, "list_lights", &json!({}), None).await.unwrap();
    assert_eq!(r["success"], false);
    assert!(r["error"].as_str().unwrap().contains("has ended. Sign in again."), "{r}");
    let v = view(&store::get(&id).unwrap());
    assert_eq!(v.status.sign_in, Some(oauth::SignInState::Expired));
    assert!(v.status.line.contains("has ended"), "{}", v.status.line);
    assert!(vault::load(&id).grant.is_none(), "dead tokens aren't kept");
    // Signing in again brings it back.
    assert_eq!(sign_in_with(&id, &browser).await.unwrap().connector.status.sign_in, Some(oauth::SignInState::SignedIn));
}

#[tokio::test]
async fn a_server_wanting_more_access_asks_to_sign_in_again_with_more_scopes() {
    on();
    let srv = fake::oauth(fake::OauthSetup::default());
    let id = signed_in(&srv).await;
    let r = call_tool(&id, "turn_off", &json!({ "area": "hall" }), None).await.unwrap();
    assert_eq!(r["success"], false);
    assert!(r["error"].as_str().unwrap().contains("needs more access for this. Sign in again."), "{r}");
    let c = store::get(&id).unwrap();
    assert_eq!(c.auth.wants, ["write"]);
    assert_eq!(view(&c).status.sign_in, Some(oauth::SignInState::NeedsSignIn));
    // The next sign-in asks for the old scopes and the new one.
    sign_in_with(&id, &browser).await.unwrap();
    assert!(srv.said().iter().any(|l| l.starts_with("authorize read offline_access write")), "{:?}", srv.said());
    let r = call_tool(&id, "turn_off", &json!({ "area": "hall" }), None).await.unwrap();
    assert_eq!(r["data"]["text"], "Turned off the hall lights.", "{r}");
}

#[tokio::test]
async fn an_answer_from_another_issuer_is_never_used() {
    on();
    let srv = fake::oauth(fake::OauthSetup { wrong_iss: true, ..Default::default() });
    let v = connector_add(link(&srv.url("/mcp"))).await.unwrap();
    let err = sign_in_with(&v.id, &browser).await.unwrap_err();
    assert!(err.contains("didn't come from the sign-in server Yap asked"), "{err}");
    assert_eq!(srv.count("token"), 0, "the code was never used: {:?}", srv.said());
    assert!(vault::load(&v.id).grant.is_none());
    assert_eq!(view(&store::get(&v.id).unwrap()).status.sign_in, Some(oauth::SignInState::NeedsSignIn));
}

#[tokio::test]
async fn a_sign_in_can_be_cancelled_and_a_grant_goes_only_to_its_own_server() {
    on();
    let srv = fake::oauth(fake::OauthSetup::default());
    let v = connector_add(link(&srv.url("/mcp"))).await.unwrap();
    // The browser is opened and never comes back; Cancel.
    let nobody = |_: &str| Ok(());
    let id = v.id.clone();
    let (out, ()) = tokio::join!(sign_in_with(&v.id, &nobody), async {
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(oauth::signing_in(&id));
        assert_eq!(view(&store::get(&id).unwrap()).status.line, "Signing in… Finish in your browser.");
        assert!(oauth::cancel(&id));
    });
    assert_eq!(out.unwrap().outcome, Outcome::Cancelled);
    assert!(!oauth::signing_in(&v.id));

    // Signed in, then the connector's address changes: its grant isn't sent there.
    let id = signed_in(&srv).await;
    let other = fake::http(Mode::Answer);
    store::update(&id, |c| c.transport = Transport::Remote { url: other.url("/mcp"), fallback_sse: None, sse: false, headers: vec![] }).unwrap();
    assert_eq!(oauth::access_token(&id, None).await.unwrap_err(), oauth::Trouble::SignIn);
    // A program or a token connector doesn't sign in in the browser.
    let tok = connector_add(Spec { token: Some("t-FAKE".into()), ..link(&srv.url("/mcp")) }).await.unwrap();
    assert!(sign_in_with(&tok.id, &browser).await.unwrap_err().contains("uses a token"));
}

#[tokio::test]
async fn the_persons_own_client_is_kept_its_secret_in_the_vault() {
    on();
    let srv = fake::oauth(fake::OauthSetup::default());
    let v = connector_add(link(&srv.url("/mcp"))).await.unwrap();
    let after = connector_set_client(v.id.clone(), "my-client".into(), Some("cs-FAKE-own".into())).await.unwrap();
    let info = after.sign_in.unwrap();
    assert_eq!((info.own_client_id.as_deref(), info.has_own_secret), (Some("my-client"), true));
    assert_eq!(info.redirect_uri, "http://127.0.0.1:51439/connectors/callback");
    assert!(!serde_json::to_string(&store::list()).unwrap().contains("cs-FAKE"));
    assert_eq!(vault::load(&v.id).own_secret.as_deref(), Some("cs-FAKE-own"));
    assert!(connector_set_client(v.id.clone(), "has space".into(), None).await.is_err());
    // Cleared: the id and its secret both go.
    connector_set_client(v.id.clone(), String::new(), None).await.unwrap();
    assert!(store::get(&v.id).unwrap().auth.own.is_none() && vault::load(&v.id).own_secret.is_none());
}

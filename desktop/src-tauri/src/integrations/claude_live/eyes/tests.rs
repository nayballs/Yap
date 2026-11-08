use super::super::turn::{test_begin_seeing, Context};
use super::*;

const LOOK: Eyes = Eyes { screen: Gate::Look, pictures: Gate::Look, act: true };

#[test]
fn the_tools_are_the_tool_loops_with_the_same_schemas() {
    let defs = definitions(LOOK);
    let listed: Vec<&str> = defs.iter().map(|d| d["name"].as_str().unwrap()).collect();
    assert_eq!(listed, all_names());
    assert_eq!(names(LOOK), all_names());
    // The screen, then every browser tool, then the pictures'.
    assert_eq!(listed[0], see_screen::LOOK);
    assert_eq!(listed[1..=pages::ALL.len()], *pages::ALL);
    // The tool loop's own words and schemas, each one.
    let mut loop_defs: Vec<Value> = Vec::new();
    for v in [see_screen::definitions(Gate::Look), pages::definitions(Gate::Look, true), pictures::definitions(Gate::Look, true)] {
        loop_defs.extend(v.as_array().cloned().unwrap());
    }
    for d in &defs {
        let f = loop_defs.iter().map(|l| &l["function"]).find(|f| f["name"] == d["name"]).unwrap_or_else(|| panic!("{}", d["name"]));
        assert_eq!(d["inputSchema"], f["parameters"], "{}", d["name"]);
        assert_eq!(d["description"], f["description"], "{}", d["name"]);
        assert!(d["title"].as_str().is_some_and(|t| !t.is_empty()));
    }
    // Never the older names, nor offer_browser.
    for never in ["open_page", "read_open_page", "snap_page", "switch_tab", "click", "type_text", "go_to", "offer_browser"] {
        assert!(!is_one(never) && !listed.contains(&never), "{never}");
    }
    // Reads may run side by side; what changes the page or what's in view, in order.
    let read_only = |n: &str| defs.iter().find(|d| d["name"] == n).unwrap()["annotations"]["readOnlyHint"] == json!(true);
    assert!(read_only(see_screen::LOOK) && read_only(pages::READ) && read_only(pages::FIND) && read_only(pictures::LOOK));
    for n in [pages::NAVIGATE, pages::SCROLL, pages::SCREENSHOT, pages::ZOOM, pages::TABS, pages::BATCH, act::CLICK, act::TYPE, pictures::SHOW] {
        assert!(!read_only(n), "{n}");
    }
}

#[test]
fn each_gate_offers_what_the_tool_loop_offers() {
    assert!(names(Eyes::default()).is_empty() && definitions(Eyes::default()).is_empty());
    assert!(!Eyes::default().any());
    // The screen alone (the web off): only the look.
    assert_eq!(names(Eyes { screen: Gate::Look, pictures: Gate::Off, act: false }), [see_screen::LOOK]);
    // The pages alone: no look at the screen.
    assert_eq!(names(Eyes { screen: Gate::Off, pictures: Gate::Look, act: true }), all_names()[1..]);
    // A model that can't see: no shot, zoom or look at a picture; no point clicks.
    let blind = names(Eyes { screen: Gate::Show, pictures: Gate::Show, act: true });
    assert!(!blind.contains(&pages::SCREENSHOT) && !blind.contains(&pages::ZOOM) && !blind.contains(&pictures::LOOK) && blind.contains(&act::CLICK));
    // A session that may not act: reading and looking only.
    let reads = names(Eyes { act: false, ..LOOK });
    assert!(reads.contains(&pages::SCREENSHOT) && !reads.iter().any(|n| pages::acts(n)));
    // A turn never gets more than its session lists.
    let away = Eyes { screen: Gate::Off, pictures: Gate::Off, act: false };
    assert_eq!(LOOK.within(away), away);
    assert_eq!(LOOK.within(LOOK), LOOK);
    assert_eq!(away.within(LOOK), away);
    assert!(!Eyes { act: false, ..LOOK }.within(LOOK).act);
    assert!(!LOOK.within(Eyes { act: false, ..LOOK }).act);
    // What brings words or pictures in from the moment it begins.
    for n in [see_screen::LOOK, pages::READ, pages::FIND, pages::SCREENSHOT, pages::ZOOM, pages::SCROLL, pages::BATCH, act::CLICK, pictures::LOOK] {
        assert!(sees(n), "{n}");
    }
    for n in [pages::NAVIGATE, pages::TABS, pictures::SHOW, "list_meetings"] {
        assert!(!sees(n), "{n}");
    }
}

#[test]
fn refused_in_a_meeting_and_away_from_chat_before_anything_runs() {
    // During a meeting or a call: the screen and acting, never (nothing asked first).
    let r = refused(see_screen::LOOK, LOOK, true).unwrap();
    assert_eq!(r["displayText"], "Didn't look at your screen: a meeting or a call");
    assert_eq!(r["success"], false);
    assert_eq!(refused(act::CLICK, LOOK, true).unwrap()["displayText"], "Didn't act on the page: a meeting or a call");
    // …the panel's pages still open and read (they're not the screen).
    assert_eq!(refused(pages::NAVIGATE, LOOK, true), None);
    assert_eq!(refused(pages::SCREENSHOT, LOOK, true), None);
    // Asked away from Chat (another app, a phone): neither.
    let away = Eyes::default();
    assert_eq!(refused(see_screen::LOOK, away, false).unwrap()["displayText"], "Didn't look at your screen: not asked in Chat");
    assert_eq!(refused(pages::SCREENSHOT, away, false).unwrap()["displayText"], "Didn't use the panel beside the chat: not asked in Chat");
    // Asked out loud with Chat in view: reading yes, acting no.
    let spoken = Eyes { act: false, ..LOOK };
    assert_eq!(refused(pages::READ, spoken, false), None);
    assert_eq!(refused(act::TYPE, spoken, false).unwrap()["displayText"], "Didn't act on the page: not a question typed in Chat");
    // A shot for a model that can't see: not offered.
    assert!(refused(pages::SCREENSHOT, Eyes { screen: Gate::Show, pictures: Gate::Show, act: true }, false).is_some());
    assert_eq!(refused(see_screen::LOOK, LOOK, false), None);
    assert_eq!(refused(act::CLICK, LOOK, false), None);
}

#[test]
fn a_picture_goes_back_as_mcp_image_content_after_its_line() {
    let png = "iVBORw0KGgo=";
    let looks = vec![
        Look { part: crate::snip::turn::part(&format!("data:image/png;base64,{png}"), PROVIDER), text: "(From Yap, not the user.) Monitor 1 of 2 as it is now.".into() },
        Look { part: crate::snip::turn::part("data:image/jpeg;base64,/9j/4AAQ", "openai"), text: "Monitor 2 of 2.".into() },
        // Not a picture Yap made: left out.
        Look { part: json!({ "type": "image_url", "image_url": { "url": "https://example.com/x.png" } }), text: "never".into() },
    ];
    let result = json!({ "success": true, "data": { "monitors": 2 }, "displayText": "Looked at your screen (2 monitors)" });
    let out = mcp_result(&result, &looks);
    assert_eq!(out["isError"], false);
    let content = out["content"].as_array().unwrap();
    assert_eq!(content.len(), 5);
    // The result first, whole: its chip.
    assert_eq!(serde_json::from_str::<Value>(content[0]["text"].as_str().unwrap()).unwrap()["displayText"], "Looked at your screen (2 monitors)");
    assert_eq!(content[1], json!({ "type": "text", "text": "(From Yap, not the user.) Monitor 1 of 2 as it is now." }));
    assert_eq!(content[2], json!({ "type": "image", "data": png, "mimeType": "image/png" }));
    assert_eq!(content[4], json!({ "type": "image", "data": "/9j/4AAQ", "mimeType": "image/jpeg" }));
    assert!(!out.to_string().contains("never"));
    // A refusal: an error, no pictures.
    let out = mcp_result(&failed("why", "chip"), &[]);
    assert_eq!((out["isError"].clone(), out["content"].as_array().unwrap().len()), (json!(true), 1));
}

fn web(question: &str) -> crate::web::Turn {
    let settings = crate::web::Settings { service: crate::web::Service::Exa, key: String::new(), lang: "en".into(), native: None, country: None, local: None, units: None };
    let endpoints = crate::web::Endpoints { exa: String::new(), tavily: String::new(), standin: None, offline: true };
    crate::web::Turn::new(settings, endpoints, question, &[])
}

#[test]
fn a_page_tool_runs_the_tool_loops_code_on_the_turns_web_side() {
    let slot = Arc::new(Slot::default());
    let mut turn = test_begin_seeing(&slot, Context { question: "What's the best garden planner page?".into(), conversation: None, eyes: LOOK, ..Default::default() }, web("What's the best garden planner page?"));
    let call = slot.call().unwrap();
    let run = |name: &str, args: Value| tauri::async_runtime::block_on(run_for(&slot, &call, name, &args, std::time::Duration::ZERO));
    // A page nobody gave nor found: refused, as in the tool loop.
    let (r, _) = run(pages::NAVIGATE, json!({ "url": "https://garden.example.com/planner" }));
    assert_eq!(r["displayText"], "Didn't bring up garden.example.com/planner: not a link from you or the search");
    // Claude Code's search found it: opened beside the chat, the page told.
    turn.marks().found(&[crate::chat::chats::Source { n: 1, title: "Garden planner".into(), url: "https://garden.example.com/planner".into(), ..Default::default() }]);
    let (r, looks) = run(pages::NAVIGATE, json!({ "url": "https://garden.example.com/planner" }));
    assert_eq!(r["displayText"], "Brought up garden.example.com/planner beside the chat", "{r}");
    assert!(looks.is_empty());
    match turn.events.try_recv() {
        Ok(Event::Page { page }) => assert_eq!((page.url.as_str(), page.title.as_str()), ("https://garden.example.com/planner", "Garden planner")),
        other => panic!("{other:?}"),
    }
    // Bringing a page up reads nothing of it: no web content yet.
    assert!(!slot.web(call.n));
    assert_eq!(turn.shown().pages.len(), 1, "kept with the answer");
    // A look at a picture this turn doesn't have: the tool loop's words.
    let (r, _) = run(pictures::LOOK, json!({ "picture": "p4" }));
    assert!(r["error"].as_str().unwrap().contains("isn't a picture from this turn"), "{r}");
    // Beginning a call that brings words or pictures in: web content, as
    // a WebSearch does (bringing a page up or showing a picture doesn't).
    turn.marks().began(pages::NAVIGATE);
    assert!(!slot.web(call.n));
    turn.marks().began(pages::SCREENSHOT);
    assert!(slot.web(call.n));
}

/// The stand-in browser's pages for a plan chat's turn.
fn shop() -> crate::chat::pages::sim::Browser {
    let mut p = std::collections::BTreeMap::new();
    p.insert(
        "https://shop.example.com/kettle".to_string(),
        r#"<html><head><title>Brass kettle - Example Shop</title></head><body><main><h1 data-sim-y="0">Brass kettle</h1><p data-sim-y="900">Boils in 3 minutes.</p><form action="/cart/checkout" method="post"><button data-sim-y="960">Buy now</button></form></main></body></html>"#.to_string(),
    );
    crate::chat::pages::sim::Browser::new(&p)
}

#[test]
fn a_plan_chat_scrolls_looks_in_its_steps_and_a_click_that_buys_waits_on_the_card() {
    crate::chat::pages::sim::install(Some(shop()));
    let q = "Have a look at https://shop.example.com/kettle and buy it";
    let slot = Arc::new(Slot::default());
    let turn = test_begin_seeing(&slot, Context { question: q.into(), conversation: None, eyes: LOOK, ..Default::default() }, web(q));
    let mut turn = turn;
    let call = slot.call().unwrap();
    let run = |name: &str, args: Value| tauri::async_runtime::block_on(run_for(&slot, &call, name, &args, std::time::Duration::ZERO));
    let (r, _) = run(pages::NAVIGATE, json!({ "url": "https://shop.example.com/kettle" }));
    assert_eq!(r["success"], true, "{r}");
    // A scroll, then a look: the picture back as MCP content, the shot in the turn's steps (never a picture beside the chat).
    let (r, looks) = run(pages::BATCH, json!({ "actions": [{ "tool": pages::SCROLL, "args": { "direction": "down" } }, { "tool": pages::SCREENSHOT }] }));
    assert_eq!(r["displayTexts"], json!(["Scrolled down shop.example.com", "Looked at the page · shop.example.com"]), "{r}");
    assert_eq!(looks.len(), 1);
    assert_eq!(r["note"], SHOT_NOTE);
    let out = mcp_result(&r, &looks);
    assert_eq!(out["content"].as_array().unwrap().iter().filter(|b| b["type"] == "image").count(), 1);
    let mut shots = Vec::new();
    while let Ok(ev) = turn.events.try_recv() {
        match ev {
            Event::Shot { picture } => shots.push(picture),
            Event::Picture { .. } => panic!("a shot is never a picture beside the chat"),
            _ => {}
        }
    }
    assert_eq!(shots.iter().map(|s| s.chip.as_str()).collect::<Vec<_>>(), ["Looked at the page · shop.example.com"]);
    assert!(slot.web(call.n), "a shot is web content");
    // Its Buy now: asked on the card (here, the person says no), nothing bought.
    let (read, _) = run(pages::FIND, json!({ "query": "buy now" }));
    let lines = read["data"]["matches"].as_str().unwrap().to_string();
    let buy: u32 = lines[1..lines.find(']').unwrap()].parse().unwrap();
    crate::chat::pages::act::confirm::answer_in_test(Some(false));
    let (r, _) = run(act::CLICK, json!({ "ref": buy }));
    crate::chat::pages::act::confirm::answer_in_test(None);
    assert_eq!(r["displayText"], "Didn't click ‘Buy now’: you said no", "{r}");
    assert!(crate::chat::pages::sim::with(|b| b.log.clicked.is_empty()).unwrap());
    crate::snip::images::delete(&shots.iter().map(|s| s.id.clone()).collect::<Vec<_>>());
    crate::chat::pages::sim::install(None);
}

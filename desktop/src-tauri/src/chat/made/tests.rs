use super::serve::{self, csp, document, lib_for, page_document, point_libraries, verified, LIBS};
use super::*;

fn made(id: &str, after: Anchor) -> Made {
    Made { id: id.into(), kind: "visual".into(), title: "How dictation works".into(), format: "svg".into(), after, at: 1, outside: false, bytes: 10, ..Default::default() }
}

const SVG: &str = r#"<svg viewBox="0 0 200 60" xmlns="http://www.w3.org/2000/svg"><rect x="4" y="4" width="80" height="40" fill="var(--c1-soft)" stroke="var(--c1)"/><text x="44" y="28">Hotkey</text></svg>"#;

#[test]
fn what_the_code_is() {
    assert_eq!(format_of(SVG), "svg");
    assert_eq!(format_of("  <SVG width=\"10\"></SVG>"), "svg");
    assert_eq!(format_of("<?xml version=\"1.0\"?>\n<!-- a diagram -->\n<svg></svg>"), "svg");
    assert_eq!(format_of("<div><svg></svg></div>"), "html");
    assert_eq!(format_of("<style>p{}</style><p>Tip</p>"), "html");
    assert_eq!(format_of("<!-- never closed <svg>"), "html");
}

#[test]
fn ids_are_ours_only() {
    assert!(is_id("v_0123456789abcdef"));
    for bad in ["v_0123456789ABCDEF", "v_0123", "x_0123456789abcdef", "v_0123456789abcdeg", "v_../../made.json", ""] {
        assert!(!is_id(bad), "{bad}");
    }
    assert!(is_id(&new_id("v_")));
}

#[test]
fn kept_by_chat_and_read_back() {
    let after = Anchor { index: 2, ts: 1_700_000_000 };
    let a = made("v_00000000000000a1", after);
    add(41, 900, &a, SVG).unwrap();
    let b = Made { id: "v_00000000000000b2".into(), format: "html".into(), ..made("", after) };
    add(41, 900, &b, "<button onclick=\"sendPrompt('Again')\">Again</button>").unwrap();
    assert_eq!(list(41, 900), vec![a.clone(), b.clone()]);
    // Its code, by its chat and id only.
    assert_eq!(code_of(41, &a.id).map(|(m, c)| (m.id, c)), Some((a.id.clone(), SVG.to_string())));
    assert!(dir().join("41").join("v_00000000000000b2.html").is_file());
    assert!(code_of(42, &a.id).is_none(), "another chat's");
    assert!(code_of(41, "v_00000000000000c3").is_none());
    assert!(code_of(41, "../41/made.json").is_none());
    // A conversation id used again (an older Yap deleted the chat, leaving the folder): another chat's, never shown…
    assert!(list(41, 901).is_empty());
    // …and gone once the new chat makes one.
    let c = made("v_00000000000000c3", Anchor { index: 0, ts: 5 });
    add(41, 901, &c, SVG).unwrap();
    assert_eq!(list(41, 901), vec![c]);
    assert!(code_of(41, &a.id).is_none());
    // Deleted with the chat.
    delete_conversations(&[41]);
    assert!(list(41, 901).is_empty());
    assert!(!dir().join("41").exists());
}

#[test]
fn what_a_later_yap_keeps_is_never_dropped() {
    // A later Yap's index: a field of its own on the index and on an item (a page's versions).
    let later = json!({
        "version": 2, "created": 5, "pins": ["pg_1"],
        "items": [{ "id": "pg_00000000000000f6", "kind": "page", "title": "Tips", "format": "html", "after": { "index": 0, "ts": 1 }, "at": 2, "bytes": 3, "pinned": true, "versions": [{ "n": 1, "shared": "no" }, { "n": 2 }] }]
    });
    std::fs::create_dir_all(dir().join("46")).unwrap();
    std::fs::write(dir().join("46").join("made.json"), later.to_string()).unwrap();
    add(46, 5, &made("v_00000000000000f7", Anchor::default()), SVG).unwrap();
    let kept: Value = serde_json::from_str(&std::fs::read_to_string(dir().join("46").join("made.json")).unwrap()).unwrap();
    assert_eq!(kept["pins"], json!(["pg_1"]));
    assert_eq!(kept["items"][0]["pinned"], json!(true));
    assert_eq!(kept["items"][0]["versions"][0]["shared"], json!("no"));
    assert_eq!(kept["items"][0]["versions"][1]["n"], json!(2));
    assert_eq!(kept["items"][1]["id"], json!("v_00000000000000f7"));
}

#[test]
fn an_unreadable_index_is_never_written_over() {
    std::fs::create_dir_all(dir().join("43")).unwrap();
    std::fs::write(dir().join("43").join("made.json"), "{ not json").unwrap();
    assert!(list(43, 1).is_empty());
    assert!(add(43, 1, &made("v_00000000000000d4", Anchor::default()), SVG).is_err());
    assert_eq!(std::fs::read_to_string(dir().join("43").join("made.json")).unwrap(), "{ not json");
}

#[test]
fn the_question_it_answers_is_the_persons_last_message() {
    let m = |role: &str, ts: u64| crate::chat::chats::ChatMessage { role: role.into(), ts, ..Default::default() };
    assert_eq!(anchor_of(&[m("user", 10), m("assistant", 11), m("user", 12)]), Anchor { index: 2, ts: 12 });
    assert_eq!(anchor_of(&[m("user", 10), m("assistant", 11)]), Anchor { index: 0, ts: 10 });
    assert_eq!(anchor_of(&[]), Anchor::default());
}

#[test]
fn the_tool_is_offered_where_a_thread_shows_it() {
    assert!(offered(true, true, None, false, true));
    assert!(offered(true, true, Some("assistant"), false, true), "asked out loud with Chat in view");
    assert!(!offered(true, true, Some("assistant"), false, false), "asked from another app");
    assert!(!offered(true, true, None, true, true), "across the meetings");
    assert!(!offered(true, false, None, false, true), "a model that isn't a strong one");
    assert!(!offered(false, true, None, false, true));
    assert_eq!(definitions(false), json!([]));
    let d = definitions(true);
    assert_eq!(d[0]["function"]["name"], SHOW_VISUAL);
    assert_eq!(d[0]["function"]["parameters"]["required"], json!(["title", "code"]));
    assert_eq!(d[1]["function"]["name"], pages::SAVE_PAGE);
    assert_eq!(d[1]["function"]["parameters"]["required"], json!(["title", "html"]));
    assert_eq!(NAMES, [SHOW_VISUAL, pages::SAVE_PAGE]);
    // Light: every step sends them (show_visual alone was 291 tokens with its line).
    let est = crate::meetings::summary::estimate_tokens;
    let (schema, line) = (est(&d.to_string()), est(INSTRUCTIONS));
    println!("show_visual and save_page add: schemas {schema} + line {line} tokens");
    assert!(schema + line <= 620, "{} tokens", schema + line);
}

#[test]
fn a_question_that_asks_for_one() {
    for q in ["Draw me a diagram of how dictation works", "Can you chart my week?", "make me a small tip calculator", "Show me how a hotkey becomes text", "Visualise the pipeline"] {
        assert!(asks_for_visual(q), "{q}");
    }
    for q in ["What's the weather tomorrow?", "Withdraw the offer", "Summarise yesterday's meeting"] {
        assert!(!asks_for_visual(q), "{q}");
    }
}

#[test]
fn refused_without_code_too_big_or_a_script_from_the_web() {
    let r = show_visual(Some(1), &json!({ "title": "Nothing" }), false);
    assert_eq!(r["success"], json!(false));
    let big = "x".repeat(MAX_CODE + 1);
    assert_eq!(show_visual(Some(1), &json!({ "title": "Big", "code": big }), false)["displayText"], json!("Didn't make a visual: too big"));
    let web = r#"<script src="https://evil.example/x.js"></script><div id=c></div>"#;
    let r = show_visual(Some(1), &json!({ "title": "Chart", "code": web }), false);
    assert_eq!(r["displayText"], json!("Didn't make a visual: a script from the web"));
    let why = r["error"].as_str().unwrap();
    assert!(why.contains("evil.example") && why.contains("Chart.js 4.4.1"), "{why}");
    // No chat: nowhere to show it.
    assert_eq!(show_visual(None, &json!({ "title": "D", "code": SVG }), false)["displayText"], json!("Didn't make a visual: no chat here"));
}

#[test]
fn the_scripts_it_loads() {
    let code = r#"<script src="https://cdnjs.cloudflare.com/ajax/libs/Chart.js/4.4.1/chart.umd.min.js"></script>
<SCRIPT SRC='https://d3js.org/d3.v7.min.js'></SCRIPT><script>const datasrc = 1;</script><script data-x="1" src=/lib/d3.min.js></script>"#;
    assert_eq!(
        script_sources(code),
        ["https://cdnjs.cloudflare.com/ajax/libs/Chart.js/4.4.1/chart.umd.min.js", "https://d3js.org/d3.v7.min.js", "/lib/d3.min.js"]
    );
    assert!(refused_scripts(code).is_empty());
    assert_eq!(refused_scripts(r#"<script src="https://cdn.jsdelivr.net/npm/chart.js@4.4.0"></script>"#).len(), 1, "another version");
    assert_eq!(lib_for("http://cdnjs.cloudflare.com/ajax/libs/d3/7.9.0/d3.min.js").map(|l| l.file), Some("d3.min.js"));
    // Pointed at Yap's own copy.
    let pointed = point_libraries(code);
    assert!(pointed.contains(r#"src="/lib/chart.umd.min.js""#) && pointed.contains("SRC='/lib/d3.min.js'"), "{pointed}");
    assert!(!pointed.contains("cdnjs") && !pointed.contains("d3js.org"));
}

#[test]
fn the_policy_keeps_the_frame_apart() {
    let p = csp("http://yapmade.localhost");
    for part in [
        "default-src 'none'",
        "script-src 'unsafe-inline' http://yapmade.localhost/lib/",
        "connect-src 'none'",
        "img-src data: blob:",
        "frame-src 'none'",
        "form-action 'none'",
        "base-uri 'none'",
        "object-src 'none'",
        "sandbox allow-scripts",
    ] {
        assert!(p.contains(part), "{part}: {p}");
    }
    assert!(!p.contains("allow-same-origin") && !p.contains("unsafe-eval") && !p.contains("https:"), "{p}");
}

#[test]
fn a_visuals_document() {
    // A fragment: in Yap's page, the bridge first.
    let doc = document(SVG, "svg");
    assert!(doc.starts_with("<!doctype html>"));
    let (bridge, body) = (doc.find("window.sendPrompt").unwrap(), doc.find("<svg").unwrap());
    assert!(bridge < body);
    assert!(doc.contains("<div id=\"yap-visual\" class=\"svg\"><svg"));
    // A whole document of the model's: kept, the bridge first in its head.
    let whole = "<!DOCTYPE html><html><head><title>Tips</title></head><body><p>Tip</p></body></html>";
    let doc = document(whole, "html");
    assert!(doc.starts_with("<!DOCTYPE html><html><head><meta charset"), "{doc}");
    assert!(doc.find("sendPrompt").unwrap() < doc.find("<title>").unwrap());
    // Without a head.
    let doc = document("<html><body>Hi</body></html>", "html");
    assert!(doc.starts_with("<html><meta charset"), "{doc}");
}

#[test]
fn a_library_is_used_only_as_pinned() {
    assert_eq!(LIBS.len(), 2);
    for l in LIBS {
        assert_eq!(l.sha256.len(), 64);
        assert!(l.urls[0].starts_with("https://cdnjs.cloudflare.com/"), "fetched from its first address: {}", l.urls[0]);
        assert!(!verified(l, b"/* not it */"));
    }
}

#[test]
fn the_scheme_serves_a_visual_with_its_policy_and_nothing_else() {
    let a = made("v_00000000000000e5", Anchor { index: 0, ts: 1 });
    add(44, 7, &a, SVG).unwrap();
    let get = |path: &str| {
        let req = tauri::http::Request::builder().uri(format!("http://yapmade.localhost/{path}")).body(Vec::new()).unwrap();
        tauri::async_runtime::block_on(serve::serve(&req))
    };
    let ok = get("v%2F44%2Fv_00000000000000e5");
    assert_eq!(ok.status(), 200);
    assert_eq!(ok.headers()["Content-Security-Policy"].to_str().unwrap(), csp("http://yapmade.localhost"));
    assert!(ok.headers()["Content-Type"].to_str().unwrap().starts_with("text/html"));
    assert!(ok.headers().get("Access-Control-Allow-Origin").is_none(), "never readable by another page");
    assert!(String::from_utf8_lossy(ok.body()).contains("<svg"));
    assert_eq!(get("v/44/v_00000000000000e5").status(), 200);
    for bad in ["v/45/v_00000000000000e5", "v/44/v_00000000000000e6", "v/44/..%2F..%2Fchats.json", "v/x/v_00000000000000e5", "made.json", "44/made.json", "lib/evil.js", ""] {
        assert_eq!(get(bad).status(), 404, "{bad}");
    }
}


// ---- pages Yap keeps (pages.rs) -------------------------------------------------------------

const TIPS: &str = "<!doctype html><html><head><title>Tips</title></head><body><h1>Tip calculator</h1><input id=bill value=48></body></html>";

/// A page as save_page keeps one: in the chat's index, its versions' files beside it. (The tool itself
/// needs the chat in chats.json; this is what it writes.)
fn keep_page(conv: u64, created: u64, id: &str, versions: &[(u32, &str, Anchor)]) {
    with_index(conv, created, |index, folder| {
        let mut m = Made { id: id.into(), kind: "page".into(), title: "Tip calculator".into(), format: "html".into(), after: versions[0].2, ..Default::default() };
        for (n, html, after) in versions {
            std::fs::write(folder.join(pages::file_of(id, *n)), html).unwrap();
            m.versions.push(PageVersion { n: *n, at: 1, bytes: html.len() as u64, after: *after, title: "Tip calculator".into(), ..Default::default() });
        }
        index.items.push(m);
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_pages_ids_and_files() {
    assert!(pages::is_id("pg_0123456789abcdef"));
    assert!(!pages::is_id("v_0123456789abcdef") && !pages::is_id("pg_0123") && !pages::is_id("pg_../made.json"));
    assert!(!is_id("pg_0123456789abcdef"), "a page's id is never a visual's");
    assert_eq!(pages::file_of("pg_0123456789abcdef", 3), "pg_0123456789abcdef.v3.html");
}

#[test]
fn a_page_and_its_versions_read_back() {
    let (q1, q2) = (Anchor { index: 0, ts: 10 }, Anchor { index: 2, ts: 12 });
    keep_page(51, 3, "pg_00000000000000a1", &[(1, TIPS, q1), (2, "<p>Version two</p>", q2)]);
    let (m, n, html) = page_of(51, "pg_00000000000000a1", 0).unwrap();
    assert_eq!((n, html.as_str()), (2, "<p>Version two</p>"), "0: its newest");
    assert_eq!(m.versions.iter().map(|v| (v.n, v.after)).collect::<Vec<_>>(), [(1, q1), (2, q2)]);
    assert_eq!(page_of(51, "pg_00000000000000a1", 1).map(|(_, n, h)| (n, h)), Some((1, TIPS.to_string())));
    assert!(page_of(51, "pg_00000000000000a1", 3).is_none());
    assert!(page_of(52, "pg_00000000000000a1", 0).is_none(), "another chat's");
    assert!(page_of(51, "v_00000000000000a1", 0).is_none());
    // Its versions round-trip through the index, as the page reads them.
    let listed = list(51, 3);
    assert_eq!(listed[0].versions.len(), 2);
    let v: Value = serde_json::to_value(&listed[0]).unwrap();
    assert_eq!(v["versions"][1]["n"], json!(2));
    assert_eq!(v["kind"], json!("page"));
    // A visual has none (none written).
    assert!(serde_json::to_value(made("v_00000000000000a2", q1)).unwrap().get("versions").is_none());
}

#[test]
fn save_page_refuses_without_a_page_too_big_or_a_script_from_the_web() {
    let r = pages::save_page(Some(1), &json!({ "title": "Tips" }), false);
    assert_eq!(r["displayText"], json!("Didn't save a page: no HTML"));
    let big = "x".repeat(pages::MAX_PAGE + 1);
    assert_eq!(pages::save_page(Some(1), &json!({ "title": "Big", "html": big }), false)["displayText"], json!("Didn't save a page: too big"));
    let web = r#"<script src="https://evil.example/app.js"></script>"#;
    assert_eq!(pages::save_page(Some(1), &json!({ "title": "App", "html": web }), false)["displayText"], json!("Didn't save a page: a script from the web"));
    assert_eq!(pages::save_page(None, &json!({ "title": "App", "html": TIPS }), false)["displayText"], json!("Didn't save a page: no chat here"));
    // The run's one door: show_visual or save_page by name.
    assert_eq!(execute(pages::SAVE_PAGE, None, &json!({ "title": "App", "html": TIPS }), false)["displayText"], json!("Didn't save a page: no chat here"));
    assert_eq!(execute(SHOW_VISUAL, None, &json!({ "title": "D", "code": SVG }), false)["displayText"], json!("Didn't make a visual: no chat here"));
}

#[test]
fn a_pages_document_scrolls_and_keeps_the_bridge() {
    let doc = page_document("<h1>Tips</h1>");
    assert!(doc.contains("<main id=\"yap-page\"><h1>Tips</h1></main>"));
    assert!(doc.contains(serve::PAGE_STYLE) && !doc.contains(serve::VISUAL_STYLE));
    assert!(doc.find("sendPrompt").unwrap() < doc.find("<h1>").unwrap());
    let whole = page_document(TIPS);
    assert!(whole.starts_with("<!doctype html><html><head><meta charset"), "{whole}");
    // A visual's never scrolls.
    assert!(document(SVG, "svg").contains(serve::VISUAL_STYLE));
}

#[test]
fn the_scheme_serves_a_pages_versions_with_its_policy() {
    keep_page(53, 4, "pg_00000000000000b1", &[(1, TIPS, Anchor::default()), (2, "<p>Two</p>", Anchor::default())]);
    let get = |path: &str| {
        let req = tauri::http::Request::builder().uri(format!("http://yapmade.localhost/{path}")).body(Vec::new()).unwrap();
        tauri::async_runtime::block_on(serve::serve(&req))
    };
    let newest = get("p%2F53%2Fpg_00000000000000b1%2F0");
    assert_eq!(newest.status(), 200);
    assert_eq!(newest.headers()["Content-Security-Policy"].to_str().unwrap(), csp("http://yapmade.localhost"));
    assert!(String::from_utf8_lossy(newest.body()).contains("<p>Two</p>"));
    assert!(String::from_utf8_lossy(get("p/53/pg_00000000000000b1/1").body()).contains("Tip calculator"));
    for bad in ["p/53/pg_00000000000000b1/3", "p/54/pg_00000000000000b1/1", "p/53/pg_00000000000000b1", "p/53/pg_00000000000000b1/x", "v/53/pg_00000000000000b1", "p/53/v_00000000000000b1/1"] {
        assert_eq!(get(bad).status(), 404, "{bad}");
    }
}

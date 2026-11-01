//! **Pages beside the chat** (2026-11-01, yap-private
//! `docs/look-and-show-plan.md` Milestone 2): Chat brings a live page up in
//! the panel beside the chat, keeps a shot of one as a picture, and offers
//! to open a page in the person's own browser.
//!
//! - **`show_page { url, section? }`** ([`execute`]): the page goes to the
//!   panel at once (`stream::Event::Page`): a "Page" tab with the live page
//!   in its own webview ([`view`]: its own WebView2 profile, no Yap IPC,
//!   public http(s) only), scrolled to `section` where it can be. For
//!   interactive maps and pages a picture can't capture. The answer keeps
//!   it (`ChatMessage.pages`): its tab comes back with the conversation.
//! - **`snap_page { url }`**: the page loaded out of sight (or, when it's
//!   open in the panel, as it is there) and its shot taken
//!   (`ICoreWebView2::CapturePreview`, a PNG), stored like a pasted
//!   picture (`snip::attach::keep`) and shown as a picture
//!   (`chat/pictures.rs` `ShownPicture`, its page the source) — a picture
//!   tab and a pill. It's a candidate of the turn too (`TurnPictures::
//!   add_shot`): a model that reads images can `look_at_picture` it.
//! - **`offer_browser { url, why }`**: a button in the answer ("Open the
//!   interactive map in Brave", `stream::Event::Offer`,
//!   `ChatMessage.offers`). The model never opens a browser: the person
//!   clicks (`open_external`, their chosen browser: `shell/browsers.rs`).
//!
//! **Which pages** ([`checked`]): only a page from this turn's results or
//! the person's messages, as `read_page`'s rule (`web::Turn::allowed`), and
//! only public http(s) addresses (`web::check_url`). Every tool-using model
//! with the web on gets them (Yap does the work: no judgment needed,
//! [`on`]). None is an action tool: nothing leaves Yap but the page's own
//! address, which a search or the person gave.
//!
//! **Privacy**: the log never has an address, a title or a section — counts
//! and ids only.

pub mod view;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::chat::pictures::{Gate, PageRef, ShownPicture};

/// **A page an answer brought up** beside the chat (the same keys in JS).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShownPage {
    /// Its address, as the search or the person gave it.
    #[serde(default)]
    pub url: String,
    /// Its title (the search result's or the page read's; else its site).
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub site: String,
    /// Where it's scrolled to: a heading, or the words of one.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub section: String,
}

/// **A button an answer offers**: open `url` in the person's browser
/// ("Open the interactive map in Brave").
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserOffer {
    #[serde(default)]
    pub url: String,
    /// What it opens, in a few words ("the interactive map").
    #[serde(default)]
    pub why: String,
    #[serde(default)]
    pub site: String,
}

pub const SHOW: &str = "show_page";
pub const SNAP: &str = "snap_page";
pub const OFFER: &str = "offer_browser";
/// The tools this module runs.
pub const TOOL_NAMES: &[&str] = &[SHOW, SNAP, OFFER];
/// Pages a turn may bring up.
pub const MAX_PAGES: usize = 4;
/// Shots a turn may take (each a page load).
pub const MAX_SNAPS: usize = 3;
/// Buttons an answer may offer.
pub const MAX_OFFERS: usize = 2;
/// A button's words, at most.
pub const WHY_MAX: usize = 60;
/// A section's words, at most.
pub const SECTION_MAX: usize = 80;

/// Whether the page tools are offered: to every model that gets the picture
/// tools — any tool-using model with the web on (`pictures::Gate`). Pure.
pub fn on(gate: Gate) -> bool {
    gate != Gate::Off
}

/// Whether `name` is one of these tools and offered with `gate`. Pure.
pub fn offers(gate: Gate, name: &str) -> bool {
    on(gate) && TOOL_NAMES.contains(&name)
}

/// The tools' schemas (none when `on` is false). Kept short: every step
/// sends them.
pub fn definitions(on: bool) -> Value {
    if !on {
        return json!([]);
    }
    let url = json!({ "type": "string", "description": "The page's address" });
    json!([
        {
            "type": "function",
            "function": {
                "name": SHOW,
                "description": "Bring a live web page up beside the chat. The user sees it; you don't.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": url,
                        "section": { "type": "string", "description": "A heading to scroll to" }
                    },
                    "required": ["url"],
                    "additionalProperties": false
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": SNAP,
                "description": "Keep a shot of a web page beside the chat as a picture.",
                "parameters": {
                    "type": "object",
                    "properties": { "url": url },
                    "required": ["url"],
                    "additionalProperties": false
                }
            }
        },
        {
            "type": "function",
            "function": {
                "name": OFFER,
                "description": "Add a button to your answer that opens a page in the user's own browser.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "url": url,
                        "why": { "type": "string", "description": "What it opens, like \"the interactive map\"" }
                    },
                    "required": ["url", "why"],
                    "additionalProperties": false
                }
            }
        }
    ])
}

/// **The page tools' line** of Chat's system prompt: general, short.
pub const INSTRUCTIONS: &str = "show_page brings a live page up beside the chat (interactive maps, tools, pages a picture can't capture); snap_page keeps a shot of a page as a picture; offer_browser adds a button to open a page in the user's own browser when they'd do better there (signing in, a whole site). Only pages from this turn's results or the user's message.";

/// The page tools' line for a turn that offers them ("" when it doesn't). Pure.
pub fn instructions(on: bool) -> &'static str {
    if on {
        INSTRUCTIONS
    } else {
        ""
    }
}

/// **One turn's pages**: the ones brought up, the buttons offered, the shots
/// taken.
#[derive(Debug, Clone, Default)]
pub struct TurnPages {
    /// Brought up this turn, in order: the answer keeps them.
    pub shown: Vec<ShownPage>,
    /// Offered this turn, in order: the answer keeps them.
    pub offers: Vec<BrowserOffer>,
    /// Shots taken this turn.
    pub snaps: usize,
}

/// What running a page tool gave: its result for the model, and what the
/// page hears at once (a page brought up, a button, a shot as a picture).
#[derive(Debug, Default)]
pub struct Run {
    pub result: Value,
    pub page: Option<ShownPage>,
    pub offer: Option<BrowserOffer>,
    pub picture: Option<ShownPicture>,
}

fn failed(error: String, display: String) -> Value {
    json!({ "success": false, "error": error, "data": Value::Null, "displayText": display })
}

/// `s` on one line, at most `max` characters (then "…"). Pure.
fn clip(s: &str, max: usize) -> String {
    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
    match one.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", one[..i].trim_end()),
        None => one,
    }
}

/// **The page a call names, checked**: its address (trimmed), or the error
/// for the model and the chip — no address, not a public http(s) one, or
/// not from this turn's results or the person's messages. `doing`: the
/// chip's verb ("bring up", "take a shot of", "offer"). Pure but for the
/// turn it reads.
pub fn checked(turn: &crate::web::Turn, name: &str, args: &Value, doing: &str) -> Result<String, Value> {
    let url = args["url"].as_str().unwrap_or("").trim().to_string();
    let shown = crate::web::short_url(&url);
    if url.is_empty() {
        return Err(failed(
            "url is required: a page from this turn's results or the user's message.".into(),
            format!("Couldn't {doing} a page: no address"),
        ));
    }
    if let Err(e) = crate::web::check_url(&url) {
        return Err(failed(e.clone(), format!("Didn't {doing} {shown}: {}", crate::web::short(&e))));
    }
    if !turn.allowed(&url) {
        return Err(failed(
            format!("{name} only takes a page from this turn's search results or one the user linked in their message. Search for it first."),
            format!("Didn't {doing} {shown}: not a link from you or the search"),
        ));
    }
    Ok(url)
}

/// A page's title and site: the turn's source for it (a search result, a
/// page read), else its site. Pure but for the turn it reads.
fn titled(turn: &crate::web::Turn, url: &str) -> (String, String, usize) {
    let key = crate::web::normal_url(url);
    let site = crate::web::site_of(url);
    match turn.sources.iter().find(|s| crate::web::normal_url(&s.url) == key) {
        Some(s) => {
            let title = if s.title.trim().is_empty() { site.clone() } else { s.title.clone() };
            let site = if s.site.trim().is_empty() { site } else { s.site.clone() };
            (title, site, s.n)
        }
        None => (site.clone(), site, 0),
    }
}

/// Slow work (decoding, encoding) off the async runtime — inline in a unit
/// test, whose chat-images folder is the test thread's own.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    #[cfg(test)]
    {
        f()
    }
    #[cfg(not(test))]
    {
        tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string()).and_then(|r| r)
    }
}

/// **Run a page tool** for a model whose picture tools are `gate` (a model
/// that reads images is told it can look at a shot).
pub async fn execute(turn: &mut crate::web::Turn, name: &str, args: &Value, gate: Gate) -> Run {
    execute_with(turn, name, args, gate, view::snap).await
}

/// [`execute`] with the shot taken by `snap` (a unit test's stand-in).
pub async fn execute_with<F, Fut>(turn: &mut crate::web::Turn, name: &str, args: &Value, gate: Gate, snap: F) -> Run
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<u8>, String>>,
{
    match name {
        SHOW => show(turn, args),
        OFFER => offer(turn, args),
        SNAP => shot(turn, args, gate, snap).await,
        other => Run { result: failed(format!("Unknown page tool {other}"), format!("Unknown tool: {other}")), ..Default::default() },
    }
}

/// **`show_page`**: the page, for the panel's Page tab.
fn show(turn: &mut crate::web::Turn, args: &Value) -> Run {
    let url = match checked(turn, SHOW, args, "bring up") {
        Ok(u) => u,
        Err(result) => return Run { result, ..Default::default() },
    };
    let shown = crate::web::short_url(&url);
    let key = crate::web::normal_url(&url);
    let ok = |display: String| {
        json!({
            "success": true,
            "shown": true,
            "note": "It's open beside the chat for the user to see and use; you don't see it. Don't describe it beyond what you read.",
            "displayText": display,
        })
    };
    // Up already this turn: the one there is.
    if turn.pages.shown.iter().any(|p| crate::web::normal_url(&p.url) == key) {
        return Run { result: ok(String::new()), ..Default::default() };
    }
    if turn.pages.shown.len() >= MAX_PAGES {
        return Run {
            result: failed(format!("You've brought up {MAX_PAGES} pages this turn: answer the user now."), format!("Didn't bring up {shown}: {MAX_PAGES} pages a turn")),
            ..Default::default()
        };
    }
    let (title, site, _) = titled(turn, &url);
    let section = args["section"].as_str().map(|s| clip(s, SECTION_MAX)).unwrap_or_default();
    let page = ShownPage { url, title, site, section };
    tracing::info!(pages = turn.pages.shown.len() + 1, section = !page.section.is_empty(), "Chat: a page brought up beside the chat");
    turn.pages.shown.push(page.clone());
    Run { result: ok(format!("Brought up {shown} beside the chat")), page: Some(page), ..Default::default() }
}

/// **`offer_browser`**: a button in the answer.
fn offer(turn: &mut crate::web::Turn, args: &Value) -> Run {
    let url = match checked(turn, OFFER, args, "offer") {
        Ok(u) => u,
        Err(result) => return Run { result, ..Default::default() },
    };
    let shown = crate::web::short_url(&url);
    let key = crate::web::normal_url(&url);
    let browser = crate::shell::browsers::label();
    let ok = |display: String| {
        json!({
            "success": true,
            "offered": true,
            "note": format!("A button under your answer opens it in {browser} when the user clicks it; you haven't opened it. Mention it in a few words."),
            "displayText": display,
        })
    };
    if turn.pages.offers.iter().any(|o| crate::web::normal_url(&o.url) == key) {
        return Run { result: ok(String::new()), ..Default::default() };
    }
    if turn.pages.offers.len() >= MAX_OFFERS {
        return Run {
            result: failed(format!("You've offered {MAX_OFFERS} pages this turn: answer the user now."), format!("Didn't offer {shown}: {MAX_OFFERS} a turn")),
            ..Default::default()
        };
    }
    let why = args["why"].as_str().map(|s| clip(s, WHY_MAX)).filter(|s| !s.is_empty()).unwrap_or_else(|| "the page".to_string());
    let site = crate::web::site_of(&url);
    let offer = BrowserOffer { url, why, site };
    tracing::info!(offers = turn.pages.offers.len() + 1, "Chat: a page offered for the user's browser");
    turn.pages.offers.push(offer.clone());
    Run { result: ok(format!("Offered to open {shown} in {browser}")), offer: Some(offer), ..Default::default() }
}

/// **`snap_page`**: the page's shot, kept and shown as a picture.
async fn shot<F, Fut>(turn: &mut crate::web::Turn, args: &Value, gate: Gate, snap: F) -> Run
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<u8>, String>>,
{
    let url = match checked(turn, SNAP, args, "take a shot of") {
        Ok(u) => u,
        Err(result) => return Run { result, ..Default::default() },
    };
    let shown = crate::web::short_url(&url);
    if turn.pages.snaps >= MAX_SNAPS {
        return Run {
            result: failed(format!("You've taken {MAX_SNAPS} shots this turn: answer the user now."), format!("Didn't take a shot of {shown}: {MAX_SNAPS} a turn")),
            ..Default::default()
        };
    }
    turn.pages.snaps += 1;
    let png = match snap(url.clone()).await {
        Ok(p) => p,
        Err(e) => {
            tracing::info!("Chat: a page's shot wasn't taken");
            return Run {
                result: failed(format!("{e} Answer without it, or offer the page for the user's browser."), format!("Couldn't take a shot of {shown}: {}", crate::web::short(&e))),
                ..Default::default()
            };
        }
    };
    let bytes = png.clone();
    let image = match blocking(move || crate::snip::attach::keep(&bytes, "page")).await {
        Ok(i) => i,
        Err(e) => {
            return Run {
                result: failed(format!("Yap couldn't keep the shot ({e})."), format!("Couldn't take a shot of {shown}: {}", crate::web::short(&e))),
                ..Default::default()
            };
        }
    };
    // A shot of a page is web content (the safety rule), as a page read is.
    turn.mark_seen();
    let (title, site, n) = titled(turn, &url);
    let caption = clip(&format!("A shot of {title}"), crate::chat::pictures::CAPTION_MAX);
    let page = PageRef { title, site: site.clone(), url };
    let id = turn.pictures.add_shot(&image.id, png, image.w, image.h, &caption, page.clone(), n);
    let picture = ShownPicture { id: image.id.clone(), w: image.w, h: image.h, caption, alt: String::new(), image_url: String::new(), page, n };
    turn.pictures.shown.push(picture.clone());
    tracing::info!(w = image.w, h = image.h, "Chat: a page's shot shown beside the chat");
    let note = if gate == Gate::Look {
        format!("The shot is beside the chat as picture {id}: look_at_picture {id} to see what it shows.")
    } else {
        format!("The shot is beside the chat as picture {id}. You can't see it: don't describe it.")
    };
    let result = json!({
        "success": true,
        "shown": true,
        "picture": id,
        "id": image.id,
        "w": image.w,
        "h": image.h,
        "note": note,
        "displayText": format!("Took a shot of {shown}"),
    });
    Run { result, picture: Some(picture), ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::{Endpoints, Service, Settings, Turn};

    fn turn(question: &str) -> Turn {
        let settings = Settings { service: Service::Exa, key: String::new(), lang: "en".into(), native: None, country: None, local: None, units: None };
        Turn::new(settings, Endpoints { exa: String::new(), tavily: String::new(), standin: None, offline: true }, question, &[])
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let rgba: Vec<u8> = (0..w * h).flat_map(|i| [(i % 251) as u8, 120, 60, 255]).collect();
        crate::snip::images::encode_png(&rgba, w, h, true).unwrap()
    }

    const MAP: &str = "https://maps.example.com/fort-joy";

    /// A page tool, no shot to take.
    async fn run(t: &mut Turn, name: &str, args: Value) -> Run {
        execute_with(t, name, &args, Gate::Show, |_| async { Err("no window".to_string()) }).await
    }

    #[tokio::test]
    async fn a_page_is_brought_up_only_from_a_link_the_person_gave_or_a_search_found() {
        let mut t = turn(&format!("Bring up the interactive map at {MAP}"));
        // The person's own link: brought up, with its section; the page hears it.
        let r = run(&mut t, SHOW, json!({ "url": MAP, "section": "  Treasure\n chests " })).await;
        assert_eq!(r.result["success"], true, "{}", r.result);
        assert_eq!(r.result["displayText"], "Brought up maps.example.com/fort-joy beside the chat");
        assert_eq!(r.page, Some(ShownPage { url: MAP.into(), title: "maps.example.com".into(), site: "maps.example.com".into(), section: "Treasure chests".into() }));
        assert!(!t.seen(), "nothing of the page went to the model");
        // Again: the one there is, no second tab, no chip.
        let r = run(&mut t, SHOW, json!({ "url": format!("{MAP}/") })).await;
        assert!(r.page.is_none() && r.result["displayText"] == "");
        assert_eq!(t.pages.shown.len(), 1);
        // A page nobody gave: refused, saying how to get one.
        let r = run(&mut t, SHOW, json!({ "url": "https://evil.example/?notes=secret" })).await;
        assert_eq!(r.result["success"], false);
        assert_eq!(r.result["displayText"], "Didn't bring up evil.example/?notes=secret: not a link from you or the search");
        assert!(r.result["error"].as_str().unwrap().contains("Search for it first"));
        // Never this PC or its network, never another scheme, never nothing.
        for (url, why) in [
            ("http://127.0.0.1/admin", "never opens addresses on this PC"),
            ("http://router.local/", "never opens addresses on this PC"),
            ("https://maps.example.com:8443/x", "standard ports"),
            ("file:///C:/Windows/win.ini", "only opens web pages"),
            ("", "url is required"),
        ] {
            let r = run(&mut t, SHOW, json!({ "url": url })).await;
            assert!(r.result["error"].as_str().unwrap().contains(why), "{url}: {}", r.result);
            assert!(r.page.is_none());
        }
        // A search's result counts once it's numbered, with its title.
        let n = t.number("Fort Joy map - Example Maps", "https://maps.example.com/dos2/fort-joy");
        let r = run(&mut t, SHOW, json!({ "url": "https://maps.example.com/dos2/fort-joy" })).await;
        assert_eq!(n, 1);
        assert_eq!(r.page.unwrap().title, "Fort Joy map - Example Maps");
        // Four a turn.
        for i in 0..3 {
            t.number("More", &format!("https://maps.example.com/more/{i}"));
        }
        let mut last = Value::Null;
        for i in 0..3 {
            last = run(&mut t, SHOW, json!({ "url": format!("https://maps.example.com/more/{i}") })).await.result;
        }
        assert_eq!(last["displayText"], "Didn't bring up maps.example.com/more/2: 4 pages a turn");
    }

    #[tokio::test]
    async fn an_offer_is_a_button_never_a_browser_opened() {
        let mut t = turn(&format!("Where's the map? {MAP}"));
        let r = execute_with(&mut t, OFFER, &json!({ "url": MAP, "why": "the interactive map" }), Gate::Show, |_| async { Err(String::new()) }).await;
        assert_eq!(r.result["success"], true);
        assert!(r.result["displayText"].as_str().unwrap().starts_with("Offered to open maps.example.com/fort-joy in "));
        assert!(r.result["note"].as_str().unwrap().contains("you haven't opened it"));
        assert_eq!(r.offer, Some(BrowserOffer { url: MAP.into(), why: "the interactive map".into(), site: "maps.example.com".into() }));
        // No words: "the page". Not the person's or a search's: refused.
        let r = execute_with(&mut t, OFFER, &json!({ "url": "https://other.example/x" }), Gate::Show, |_| async { Err(String::new()) }).await;
        assert_eq!(r.result["success"], false);
        assert!(r.offer.is_none());
        assert_eq!(t.pages.offers.len(), 1);
    }

    #[tokio::test]
    async fn a_shot_is_kept_as_a_picture_the_model_can_look_at() {
        let mut t = turn(&format!("Get me a screenshot of {MAP}"));
        // The shot taken: kept like a pasted picture, shown, and a candidate.
        let r = execute_with(&mut t, SNAP, &json!({ "url": MAP }), Gate::Look, |url| async move {
            assert_eq!(url, MAP);
            Ok(png(40, 30))
        })
        .await;
        assert_eq!(r.result["success"], true, "{}", r.result);
        assert_eq!(r.result["displayText"], "Took a shot of maps.example.com/fort-joy");
        assert_eq!(r.result["picture"], "p1");
        assert!(r.result["note"].as_str().unwrap().contains("look_at_picture p1"));
        let pic = r.picture.expect("a picture");
        assert_eq!((pic.w, pic.h), (40, 30));
        assert_eq!(pic.caption, "A shot of maps.example.com");
        assert_eq!(pic.page, PageRef { title: "maps.example.com".into(), site: "maps.example.com".into(), url: MAP.into() });
        assert!(pic.image_url.is_empty(), "no original to open");
        assert!(crate::snip::images::exists(&pic.id));
        assert_eq!(t.pictures.shown, vec![pic.clone()]);
        assert!(t.seen(), "a page's shot is web content");
        // The model looks at it: no download, the shot itself.
        let look = crate::chat::pictures::look(&mut t, &json!({ "picture": "p1" }), "anthropic", "https://api.anthropic.com/v1").await;
        assert_eq!(look.result["success"], true, "{}", look.result);
        assert!(look.look.is_some());
        // Shown again by the model: the one there is.
        let again = crate::chat::pictures::show(&mut t, &json!({ "picture": "p1", "caption": "the map" })).await;
        assert!(again.shown.is_none() && again.result["id"] == json!(pic.id), "{}", again.result);
        assert_eq!(t.pictures.shown.len(), 1);
        // A model that can't see: told not to describe it.
        let r = execute_with(&mut t, SNAP, &json!({ "url": MAP }), Gate::Show, |_| async { Ok(png(8, 8)) }).await;
        assert!(r.result["note"].as_str().unwrap().contains("You can't see it"));
        crate::snip::images::delete(&[pic.id, r.picture.unwrap().id]);
        // A shot that fails says so; three a turn.
        let r = execute_with(&mut t, SNAP, &json!({ "url": MAP }), Gate::Look, |_| async { Err("The page didn't load in time.".to_string()) }).await;
        assert_eq!(r.result["displayText"], "Couldn't take a shot of maps.example.com/fort-joy: The page didn't load in time");
        let r = execute_with(&mut t, SNAP, &json!({ "url": MAP }), Gate::Look, |_| async { Ok(png(8, 8)) }).await;
        assert_eq!(r.result["displayText"], "Didn't take a shot of maps.example.com/fort-joy: 3 a turn");
    }

    #[test]
    fn every_tool_using_model_with_the_web_gets_them_and_the_line_is_short() {
        let names = |v: Value| v.as_array().unwrap().iter().map(|t| t["function"]["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(names(definitions(on(Gate::Show))), [SHOW, SNAP, OFFER]);
        assert_eq!(names(definitions(on(Gate::Look))), [SHOW, SNAP, OFFER]);
        assert!(names(definitions(on(Gate::Off))).is_empty());
        assert!(offers(Gate::Show, SNAP) && !offers(Gate::Off, SHOW) && !offers(Gate::Look, "read_page"));
        assert_eq!(instructions(false), "");
        let line = instructions(true);
        assert!(line.contains("interactive maps") && line.contains("own browser") && line.contains("this turn's results or the user's message"));
        // Not game-specific.
        assert!(!line.to_lowercase().contains("game"));
        let est = crate::meetings::meeting_summary::estimate_tokens;
        let weight = est(line) + est(&definitions(true).to_string());
        println!("the page tools add {weight} tokens");
        assert!(weight <= 400, "{weight} tokens");
        // The shapes the page and chats.json keep.
        let p = serde_json::to_value(ShownPage { url: MAP.into(), title: "T".into(), site: "s".into(), section: String::new() }).unwrap();
        assert_eq!(p, json!({ "url": MAP, "title": "T", "site": "s" }));
        let o = serde_json::to_value(BrowserOffer { url: MAP.into(), why: "the map".into(), site: "s".into() }).unwrap();
        assert_eq!(o, json!({ "url": MAP, "why": "the map", "site": "s" }));
    }
}

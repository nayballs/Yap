//! **Free sources by kind of question** (2026-11-01; the design is
//! yap-private `docs/web-search-without-keys-2026-10.md` §3.1 and §4): a
//! `web_search` with no Exa or Tavily key and no provider search for the
//! turn — the on-device model, a self-hosted one, a cloud model without a
//! search of its own, "The AI's own search" off, or a search service that
//! failed — is answered from open, keyless sources picked by the kind of
//! question ([`route`]):
//! - **the weather** → Open-Meteo's forecast, its own geocoder finding the
//!   place, in the person's units (`weather.rs`);
//! - **where a place is** → OpenStreetMap's Nominatim, then Wikipedia
//!   (`places.rs`);
//! - **programming** → the Stack Exchange API (Stack Overflow's questions and
//!   their best answers), then GitHub's issues (`stack.rs`, `github.rs`);
//! - **code** (a library, a tool, a repository) → GitHub's repositories
//!   (`github.rs`);
//! - **papers** → the arXiv API, then Wikipedia (`arxiv.rs`);
//! - **a game, facts, anything else** → the game's own MediaWiki wiki, else
//!   Wikipedia, as before (`wiki.rs`: never Fandom).
//!
//! **What none of them covers** — prices and shopping, the news, reviews and
//! rankings, timetables and opening hours, local businesses, recipes — is
//! never guessed: nothing is searched, the model is told to say plainly that
//! Yap can't search the whole web without a search service, and the answer
//! offers **"Search in your browser"** ([`BrowserSearch`]: a search for the
//! question in the person's own browser, on the engine they chose; Yap never
//! reads that page). The same when the free sources found nothing. News
//! stays with the provider's search or a search key: the research found no
//! concrete set of publishers' feeds whose terms clearly allow an app's use
//! (only an undated archived BBC page), so no feeds are read.
//!
//! **Never a search engine's result pages** — Google, Bing, DuckDuckGo,
//! Brave, Mojeek, Startpage, nor any of them through SearXNG: their
//! `robots.txt` and terms forbid automated use (checked 31 Oct 2026).
//!
//! **Polite** (`polite.rs`): a descriptive User-Agent with a contact address
//! (`super::USER_AGENT`), each host's own pace (Nominatim one request a
//! second, arXiv one every three seconds, GitHub's search ten a minute),
//! answers cached (Nominatim's for a day, as its policy asks), a host's
//! `Retry-After` (and Stack Exchange's `backoff`) honoured, every request a
//! receipt of its own (`privacy::net::Purpose::FreeSource`). Each source's
//! terms are quoted in its module.
//!
//! Results are Yap's usual numbered [`Hit`]s, so chips, `[n]` citations, the
//! Sources row, the search cap and the safety rule work unchanged. Test runs
//! never go online: every request goes through `Endpoints::route` (the
//! stand-in web, `YAP_WEB_STANDIN`), as the wiki lookup's do.

mod arxiv;
mod github;
mod places;
mod polite;
mod route;
mod stack;
mod weather;

use serde::{Deserialize, Serialize};

use super::wiki::keyless;
use super::{normal_url, Endpoints, Found, FoundImage, Hit, Settings, MAX_RESULTS};

pub use route::{route, Kind, NotCovered};

#[cfg(debug_assertions)]
pub(super) use polite::clear as clear_caches;

/// A free source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Weather,
    Places,
    StackExchange,
    GitHub,
    Arxiv,
    /// The keyless wiki lookup: a game's wiki, else Wikipedia (`wiki.rs`).
    Wiki,
}

impl Source {
    /// The name a chip and the model see ("Open-Meteo").
    pub fn name(self) -> &'static str {
        match self {
            Source::Weather => "Open-Meteo",
            Source::Places => "OpenStreetMap",
            Source::StackExchange => "Stack Exchange",
            Source::GitHub => "GitHub",
            Source::Arxiv => "arXiv",
            Source::Wiki => "Wikipedia",
        }
    }
}

/// The sources a kind of question asks, in order — and those asked only
/// when the first found nothing. Pure.
pub fn sources_for(kind: Kind) -> (&'static [Source], &'static [Source]) {
    match kind {
        Kind::Weather => (&[Source::Weather], &[]),
        Kind::Place => (&[Source::Places, Source::Wiki], &[]),
        Kind::Programming => (&[Source::StackExchange, Source::GitHub], &[Source::Wiki]),
        Kind::Code => (&[Source::GitHub], &[Source::StackExchange, Source::Wiki]),
        Kind::Papers => (&[Source::Arxiv, Source::Wiki], &[]),
        Kind::Game | Kind::Facts => (&[Source::Wiki], &[]),
        Kind::NotCovered(_) => (&[], &[]),
    }
}

/// The free sources, said plainly (the model's lines).
const FREE: &str = "Wikipedia and game wikis, the weather (Open-Meteo), maps (OpenStreetMap), Stack Exchange, GitHub and arXiv";

/// **Why Yap's free sources couldn't answer** a search (no search service
/// for the turn): what the model is told, and whether the answer offers a
/// search in the person's browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handoff {
    /// The search the browser would run (the model's query, or Yap's for a
    /// model that can't call tools).
    pub query: String,
    pub why: Why,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// None of them covers it: nothing was searched.
    NotCovered(NotCovered),
    /// A free source had a go, but this kind of question is one they may
    /// not answer (a game's price: its wiki first).
    MayNotCover(NotCovered),
    /// They found nothing.
    NothingFound,
    /// The weather, but no place to look up.
    NeedsPlace,
}

impl Handoff {
    /// The answer offers "Search in your browser" for it.
    pub fn offers(&self) -> bool {
        !matches!(self.why, Why::NeedsPlace)
    }

    /// What a model that calls tools is told, in the search's result.
    pub fn line(&self) -> String {
        const BUTTON: &str = "Yap shows a \"Search in your browser\" button under your answer.";
        match self.why {
            Why::NotCovered(what) => format!(
                "Nothing was searched: this chat has no web search service, and Yap's free sources ({FREE}) don't cover {}. Say so plainly in one short sentence, without guessing or giving figures from memory, and don't search for it again. {BUTTON}",
                what.what()
            ),
            Why::MayNotCover(what) => format!(
                "This chat has no web search service: Yap searched its free sources ({FREE}), which may not cover {}. If these results don't answer it, say so plainly instead of guessing. {BUTTON}",
                what.what()
            ),
            Why::NothingFound => format!(
                "Nothing found: this chat has no web search service, and Yap's free sources ({FREE}) had nothing for it. Try other words once at most; then say plainly you couldn't find it, without guessing. {BUTTON}"
            ),
            Why::NeedsPlace => "The weather needs a place: ask the user which town or city they mean (nothing was searched).".to_string(),
        }
    }

    /// What a model that can't call tools is told (the fixed pipeline: no
    /// sources to answer from).
    pub fn small_line(&self) -> String {
        match self.why {
            Why::NeedsPlace => "Yap couldn't look up the weather: no place was named. Ask the user which town or city they mean. Never claim you searched, browsed or found anything online.".to_string(),
            Why::NotCovered(what) | Why::MayNotCover(what) => format!(
                "Yap can't search the whole web in this chat (there's no search service, and its free sources don't cover {}). Tell the user that in one short sentence, without guessing or giving figures from memory: Yap shows a \"Search in your browser\" button under your answer. Never claim you searched, browsed or found anything online.",
                what.what()
            ),
            Why::NothingFound => "Yap's free web sources found nothing for this question, and there's no search service in this chat. Tell the user that in one short sentence, without guessing: Yap shows a \"Search in your browser\" button under your answer. Never claim you searched, browsed or found anything online.".to_string(),
        }
    }

    /// The chip's words after the query: `· Yap's free sources don't cover
    /// prices`, or `None` for the usual chip.
    pub fn chip(&self) -> Option<String> {
        match self.why {
            Why::NotCovered(what) => Some(format!("Yap's free sources don't cover {}", what.what())),
            Why::NeedsPlace => Some("no place to look up".to_string()),
            Why::MayNotCover(_) | Why::NothingFound => None,
        }
    }
}

// ---- search in your browser ---------------------------------------------------------------

/// The engines "Search in your browser" can use (Settings → Voice Assistant
/// → Web search, `web_search_browser`; DuckDuckGo by default, the research's
/// recommendation): id, name, the search address the query follows.
pub const ENGINES: &[(&str, &str, &str)] = &[
    ("duckduckgo", "DuckDuckGo", "https://duckduckgo.com/?q="),
    ("google", "Google", "https://www.google.com/search?q="),
    ("bing", "Bing", "https://www.bing.com/search?q="),
    ("brave", "Brave Search", "https://search.brave.com/search?q="),
    ("ecosia", "Ecosia", "https://www.ecosia.org/search?q="),
    ("startpage", "Startpage", "https://www.startpage.com/sp/search?query="),
];

/// **"Search in your browser"** under an answer the free sources couldn't
/// give (`chats::ChatMessage::browser_search`): the search, its address on
/// the person's engine, the engine's name. The page opens it with
/// `open_external` in the person's own browser — they do the searching,
/// and Yap never reads the results.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSearch {
    pub query: String,
    pub url: String,
    pub engine: String,
}

impl BrowserSearch {
    /// `query` on `engine` (an [`ENGINES`] id; anything else: DuckDuckGo).
    /// Pure.
    pub fn new(engine: &str, query: &str) -> BrowserSearch {
        let (_, name, base) = ENGINES.iter().find(|(id, _, _)| id.eq_ignore_ascii_case(engine.trim())).unwrap_or(&ENGINES[0]);
        let query = super::tidy(query, 200);
        let encoded: String = reqwest::Url::parse_with_params("https://x.invalid/", [("q", query.as_str())])
            .ok()
            .and_then(|u| u.query().map(|q| q.trim_start_matches("q=").to_string()))
            .unwrap_or_default();
        BrowserSearch { url: format!("{base}{encoded}"), engine: (*name).to_string(), query }
    }
}

// ---- searching -------------------------------------------------------------------------

/// What one source found.
#[derive(Debug, Default)]
pub(super) struct Out {
    pub hits: Vec<Hit>,
    pub images: Vec<FoundImage>,
    /// Who answered ("Open-Meteo", "bg3.wiki"…).
    pub from: String,
    /// The source can't search without more from the person (the weather
    /// with no place).
    pub needs_place: bool,
}

/// The person's languages for the rules: Wikipedia's (the speech language,
/// else Windows'), and English always.
fn langs(s: &Settings) -> Vec<String> {
    let mut l = vec![s.lang.clone()];
    if s.lang != "en" {
        l.push("en".into());
    }
    l
}

async fn ask(source: Source, s: &Settings, ep: &Endpoints, query: &str, context: &str) -> Result<Out, String> {
    let langs = langs(s);
    match source {
        Source::Weather => weather::look_up(s, ep, query, context, &langs).await,
        Source::Places => places::look_up(s, ep, query, context, &langs).await,
        Source::StackExchange => stack::look_up(ep, query).await,
        Source::GitHub => github::look_up(ep, query, context).await,
        Source::Arxiv => arxiv::look_up(ep, query).await,
        Source::Wiki => keyless(s, ep, query, context).await.map(|f| Out { hits: f.hits, images: f.images, from: f.from, needs_place: false }),
    }
}

/// **A keyless search**: the sources for the question's kind ([`route`],
/// [`sources_for`]), their results together (≤ [`super::MAX_RESULTS`], each
/// page once); nothing searched for a kind none covers, and a [`Handoff`]
/// whenever they can't answer. An error only when every source asked
/// failed.
pub(super) async fn free_search(s: &Settings, ep: &Endpoints, query: &str, context: &str) -> Result<Found, String> {
    let r = route(query, context, &langs(s));
    tracing::info!(kind = ?r.kind, "Web search: Yap's free sources");
    if let Kind::NotCovered(what) = r.kind {
        return Ok(Found { handoff: Some(Handoff { query: query.to_string(), why: Why::NotCovered(what) }), ..Default::default() });
    }
    let (first, then) = sources_for(r.kind);
    let mut found = Found::default();
    let mut froms: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut answered = false;
    for (round, list) in [first, then].into_iter().enumerate() {
        if round == 1 && !found.hits.is_empty() {
            break;
        }
        for &source in list {
            if found.hits.len() >= MAX_RESULTS {
                break;
            }
            match ask(source, s, ep, query, context).await {
                Ok(out) if out.needs_place => {
                    return Ok(Found {
                        from: source.name().into(),
                        handoff: Some(Handoff { query: query.to_string(), why: Why::NeedsPlace }),
                        ..Default::default()
                    });
                }
                Ok(out) => {
                    answered = true;
                    let before = found.hits.len();
                    for h in out.hits {
                        if found.hits.len() < MAX_RESULTS && !found.hits.iter().any(|x| normal_url(&x.url) == normal_url(&h.url)) {
                            found.hits.push(h);
                        }
                    }
                    if found.hits.len() > before {
                        froms.push(if out.from.is_empty() { source.name().to_string() } else { out.from });
                        found.images.extend(out.images);
                    }
                }
                Err(e) => {
                    tracing::info!(source = source.name(), "Web search: a free source didn't answer");
                    errors.push(e);
                }
            }
        }
    }
    if !answered {
        if let Some(e) = errors.into_iter().next() {
            return Err(e);
        }
    } else if found.hits.is_empty() {
        // One source failed, the others found nothing: the chip says why.
        found.note = errors.into_iter().next();
    }
    found.from = if froms.is_empty() { first.first().map_or("Wikipedia", |s| s.name()).to_string() } else { froms.join(", ") };
    found.handoff = match (found.hits.is_empty(), r.offer) {
        (true, _) => Some(Handoff { query: query.to_string(), why: Why::NothingFound }),
        (false, Some(what)) => Some(Handoff { query: query.to_string(), why: Why::MayNotCover(what) }),
        (false, None) => None,
    };
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::Turn;
    use serde_json::json;

    /// A stand-in web: `answer(path and query)` → (status, body); every
    /// request kept. Never the internet.
    struct Standin {
        ep: Endpoints,
        asked: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    fn standin(answer: fn(&str) -> (u16, String)) -> Standin {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("a local port");
        let port = server.server_addr().to_ip().expect("ip").port();
        let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let kept = asked.clone();
        std::thread::spawn(move || {
            for req in server.incoming_requests() {
                let url = req.url().trim_start_matches("/standin/").to_string();
                kept.lock().unwrap().push(url.clone());
                let (status, body) = answer(&url);
                let header = tiny_http::Header::from_bytes("content-type", "application/json").unwrap();
                let _ = req.respond(tiny_http::Response::from_string(body).with_status_code(status).with_header(header));
            }
        });
        let ep = Endpoints { exa: String::new(), tavily: String::new(), standin: Some(format!("http://127.0.0.1:{port}/standin")), offline: false };
        Standin { ep, asked }
    }

    fn uk() -> Settings {
        Settings { units: Some(crate::ai::about_you::Units::MetricMiles), country: Some("GB".into()), ..crate::web::tests::settings("") }
    }

    fn web(url: &str) -> (u16, String) {
        let geo = json!({ "results": [{ "name": "Brackenmoor", "latitude": 54.1, "longitude": -2.2, "country_code": "GB", "country": "United Kingdom", "admin1": "England" }] });
        let forecast = json!({
            "current_units": { "temperature_2m": "°C", "wind_speed_10m": "mp/h" },
            "current": { "time": "2026-11-01T09:00", "temperature_2m": 7.4, "weather_code": 61, "wind_speed_10m": 12.2 },
            "daily": { "time": ["2026-11-01"], "weather_code": [61], "temperature_2m_max": [9.0], "temperature_2m_min": [4.0], "precipitation_probability_max": [70], "precipitation_sum": [3.0], "wind_speed_10m_max": [20.0] }
        });
        let body = if url.starts_with("geocoding-api.open-meteo.com/v1/search?name=Brackenmoor") {
            geo
        } else if url.starts_with("geocoding-api.open-meteo.com/v1/search?name=Thornwick") {
            json!({ "results": [{ "name": "Thornwick", "latitude": 53.3, "longitude": -1.5, "country_code": "GB", "country": "United Kingdom", "admin1": "England" }] })
        } else if url.starts_with("api.open-meteo.com/v1/forecast?") {
            forecast
        } else if url.starts_with("api.stackexchange.com/2.3/search/advanced?") && url.contains("tidecaller") {
            json!({ "items": [{ "question_id": 77, "link": "https://stackoverflow.com/questions/77/tidecaller-panics", "title": "tidecaller panics on start", "score": 5, "answer_count": 1, "accepted_answer_id": 78, "body": "<p>It panics.</p>" }] })
        } else if url.starts_with("api.stackexchange.com/2.3/questions/77/answers?") {
            json!({ "items": [{ "answer_id": 78, "question_id": 77, "is_accepted": true, "score": 9, "body": "<p>Set the config path first.</p>" }], "backoff": 0 })
        } else if url.starts_with("api.github.com/search/issues?") && url.contains("tidecaller") {
            json!({ "items": [{ "title": "Panic on start", "html_url": "https://github.com/example-org/tidecaller/issues/5", "number": 5, "state": "open", "comments": 1, "repository_url": "https://api.github.com/repos/example-org/tidecaller", "body": "Same here." }] })
        } else if url.contains("api.php") {
            // Wikipedia: nothing for these questions.
            json!({ "batchcomplete": true })
        } else {
            return (404, "{}".into());
        };
        (200, body.to_string())
    }

    #[tokio::test]
    async fn the_weather_comes_from_open_meteo_with_no_key() {
        let s = standin(web);
        let mut turn = Turn::new(uk(), s.ep.clone(), "What's the weather like in Brackenmoor today?", &[]);
        let r = turn.execute("web_search", &json!({ "query": "weather Brackenmoor today" })).await;
        assert_eq!(r["displayText"], "Searched the web: \"weather Brackenmoor today\" (Open-Meteo)");
        assert_eq!(r["data"]["from"], "Open-Meteo");
        let hit = &r["data"]["results"][0];
        assert_eq!(hit["n"], 1);
        assert_eq!(hit["title"], "Weather in Brackenmoor, England, United Kingdom");
        assert_eq!(hit["site"], "open-meteo.com");
        assert!(hit["text"].as_str().unwrap().starts_with("Now (09:00 local time): 7°C, light rain, wind 12 mph."), "{hit}");
        assert!(r["data"].get("hint").is_none(), "{r}");
        assert!(turn.seen() && turn.browser_search("duckduckgo").is_none());
        let asked = s.asked.lock().unwrap().clone();
        assert_eq!(asked.len(), 2, "the geocoder, then the forecast: {asked:?}");
        assert!(asked[0].starts_with("geocoding-api.open-meteo.com/v1/search?name=Brackenmoor&count=5&language=en"), "{asked:?}");
        // The UK's wind in mph.
        assert!(asked[1].contains("wind_speed_unit=mph") && !asked[1].contains("temperature_unit"), "{asked:?}");
    }

    #[tokio::test]
    async fn a_price_is_never_guessed_and_the_browser_is_offered() {
        let s = standin(web);
        let mut turn = Turn::new(crate::web::tests::settings(""), s.ep.clone(), "How much is a Driftwood Pro subscription?", &[]);
        let r = turn.execute("web_search", &json!({ "query": "Driftwood Pro subscription price" })).await;
        assert_eq!(r["success"], true);
        assert_eq!(r["displayText"], "Didn't search the web for \"Driftwood Pro subscription price\": Yap's free sources don't cover prices");
        assert_eq!(r["data"]["results"], json!([]));
        let hint = r["data"]["hint"].as_str().unwrap();
        assert!(hint.starts_with("Nothing was searched") && hint.contains("\"Search in your browser\""), "{hint}");
        assert!(s.asked.lock().unwrap().is_empty(), "nothing asked of anyone");
        assert!(!turn.seen(), "no web content: actions stay allowed");
        let b = turn.browser_search("startpage").unwrap();
        assert_eq!(b.url, "https://www.startpage.com/sp/search?query=Driftwood+Pro+subscription+price");
        assert_eq!(b.engine, "Startpage");
    }

    #[tokio::test]
    async fn programming_asks_stack_exchange_then_github() {
        let s = standin(web);
        let mut turn = Turn::new(crate::web::tests::settings(""), s.ep.clone(), "Why does tidecaller panic on start?", &[]);
        let r = turn.execute("web_search", &json!({ "query": "tidecaller panic on start error rust" })).await;
        assert_eq!(r["displayText"], "Searched the web: \"tidecaller panic on start error rust\" (Stack Overflow, GitHub)");
        let urls: Vec<&str> = r["data"]["results"].as_array().unwrap().iter().map(|h| h["url"].as_str().unwrap()).collect();
        assert_eq!(urls, ["https://stackoverflow.com/questions/77/tidecaller-panics", "https://github.com/example-org/tidecaller/issues/5"]);
        assert!(r["data"]["results"][0]["text"].as_str().unwrap().contains("Accepted answer (score 9): Set the config path first."));
        assert!(turn.browser_search("duckduckgo").is_none());
    }

    #[tokio::test]
    async fn nothing_found_offers_the_browser_unless_another_search_found_something() {
        let s = standin(web);
        let mut turn = Turn::new(crate::web::tests::settings(""), s.ep.clone(), "Who founded the Quillmere Society?", &[]);
        let r = turn.execute("web_search", &json!({ "query": "Quillmere Society founder" })).await;
        assert_eq!(r["displayText"], "Searched the web: \"Quillmere Society founder\" (Wikipedia) · nothing found");
        assert!(r["data"]["hint"].as_str().unwrap().starts_with("Nothing found"), "{r}");
        assert_eq!(turn.browser_search("duckduckgo").unwrap().url, "https://duckduckgo.com/?q=Quillmere+Society+founder");
        // A second search finds something: no button.
        let r = turn.execute("web_search", &json!({ "query": "weather Thornwick" })).await;
        assert_eq!(r["data"]["from"], "Open-Meteo");
        assert!(turn.browser_search("duckduckgo").is_none());
    }

    #[tokio::test]
    async fn the_weather_with_no_place_asks_where() {
        let s = standin(web);
        let mut turn = Turn::new(crate::web::tests::settings(""), s.ep.clone(), "Will it rain tomorrow?", &[]);
        let r = turn.execute("web_search", &json!({ "query": "will it rain tomorrow" })).await;
        assert_eq!(r["displayText"], "Didn't search the web for \"will it rain tomorrow\": no place to look up");
        assert!(r["data"]["hint"].as_str().unwrap().contains("which town or city"));
        assert!(s.asked.lock().unwrap().is_empty());
        assert!(turn.browser_search("duckduckgo").is_none(), "no button: the person says where");
    }

    #[test]
    fn search_in_your_browser_goes_to_the_chosen_engine() {
        let b = BrowserSearch::new("duckduckgo", "Grok subscription price UK");
        assert_eq!(b.url, "https://duckduckgo.com/?q=Grok+subscription+price+UK");
        assert_eq!(b.engine, "DuckDuckGo");
        assert_eq!(b.query, "Grok subscription price UK");
        assert_eq!(BrowserSearch::new("Google", "a & b").url, "https://www.google.com/search?q=a+%26+b");
        assert_eq!(BrowserSearch::new("startpage", "café").url, "https://www.startpage.com/sp/search?query=caf%C3%A9");
        // Anything else: DuckDuckGo (the research's default).
        assert_eq!(BrowserSearch::new("altavista", "x").engine, "DuckDuckGo");
        assert_eq!(BrowserSearch::new("", "x").url, "https://duckduckgo.com/?q=x");
    }

    #[test]
    fn each_kind_asks_its_own_sources() {
        assert_eq!(sources_for(Kind::Weather).0, [Source::Weather]);
        assert_eq!(sources_for(Kind::Place).0, [Source::Places, Source::Wiki]);
        assert_eq!(sources_for(Kind::Programming), (&[Source::StackExchange, Source::GitHub][..], &[Source::Wiki][..]));
        assert_eq!(sources_for(Kind::Code).0, [Source::GitHub]);
        assert_eq!(sources_for(Kind::Papers).0, [Source::Arxiv, Source::Wiki]);
        assert_eq!(sources_for(Kind::Facts).0, [Source::Wiki]);
        assert!(sources_for(Kind::NotCovered(NotCovered::Prices)).0.is_empty());
    }

    #[test]
    fn a_handoff_says_what_to_tell_the_user() {
        let h = |why| Handoff { query: "q".into(), why };
        let prices = h(Why::NotCovered(NotCovered::Prices));
        assert!(prices.offers());
        assert!(prices.line().starts_with("Nothing was searched: this chat has no web search service"), "{}", prices.line());
        assert!(prices.line().contains("don't cover prices") && prices.line().contains("without guessing"));
        assert!(prices.line().contains("\"Search in your browser\""));
        assert_eq!(prices.chip().as_deref(), Some("Yap's free sources don't cover prices"));
        assert!(h(Why::NothingFound).line().starts_with("Nothing found"));
        assert_eq!(h(Why::NothingFound).chip(), None);
        let place = h(Why::NeedsPlace);
        assert!(!place.offers());
        assert!(place.line().contains("which town or city"));
        assert!(h(Why::MayNotCover(NotCovered::Prices)).small_line().contains("don't cover prices"));
    }
}

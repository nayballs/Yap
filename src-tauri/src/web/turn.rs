//! One Chat turn's web side: the tools (`web_search`, `read_page`), the
//! links a page may be read from (the person's, or a search's in this
//! turn), and the sources an answer cites.

use std::sync::LazyLock;

use regex::Regex;
use reqwest::Url;
use serde_json::{json, Value};

use crate::chat::chats::Source;

use super::grounding::{asks_about_prices, local_query, local_retry_query};
use super::native;
use super::page::{check_url, page_part, read_page};
use super::{
    search, search_chip, short, source_site, tidy, Endpoints, Settings, DATA_NOT_INSTRUCTIONS, MAX_READS, MAX_RESULTS,
    MAX_SEARCHES, PAGE_TOKENS,
};

/// A web address, compared: scheme and host lowercased, no fragment, no
/// trailing slash. Pure.
pub fn normal_url(url: &str) -> String {
    match Url::parse(url.trim()) {
        Ok(mut u) => {
            u.set_fragment(None);
            let s = u.to_string();
            s.trim_end_matches('/').to_string()
        }
        Err(_) => url.trim().trim_end_matches('/').to_string(),
    }
}

/// The web addresses in a message (http(s), or starting "www."). Pure.
pub fn links_in(text: &str) -> Vec<String> {
    static LINK: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\b(?:https?://|www\.)[^\s<>()\[\]{}'`]+").expect("static regex"));
    LINK.find_iter(text)
        .map(|m| {
            let s = m.as_str().trim_end_matches(['.', ',', ';', ':', '!', '?', '"', '\'']);
            if s.to_ascii_lowercase().starts_with("www.") {
                format!("https://{s}")
            } else {
                s.to_string()
            }
        })
        .collect()
}

/// "bg3.wiki/wiki/Gale", for a chip.
pub fn short_url(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let rest = rest.trim_start_matches("www.").trim_end_matches('/');
    tidy(rest, 48)
}

pub(super) fn ok(data: Value, display: String) -> Value {
    json!({ "success": true, "data": data, "displayText": display })
}

fn failed(error: String, display: String) -> Value {
    json!({ "success": false, "error": error, "data": Value::Null, "displayText": display })
}

/// The web side of one chat turn: what the person linked, the sources
/// found so far (numbered for citations), the per-turn caps, and whether web
/// content has come in ([`Turn::seen`], for the safety rule).
#[derive(Debug, Clone)]
pub struct Turn {
    settings: Settings,
    endpoints: Endpoints,
    /// The person's question (spots a game the model's query leaves out).
    question: String,
    /// Addresses from the person's own messages.
    linked: Vec<String>,
    pub sources: Vec<Source>,
    searches: usize,
    reads: usize,
    seen: bool,
    /// Google Search grounded this turn (Gemini): its answer shows every
    /// link it gave (≤ 5), as Google's terms ask ([`Turn::sources_for`]).
    grounded: bool,
    /// **Prices where the person is** (a price question, someone outside
    /// the US whose money can be told apart — `about_you::LocalPrices`):
    /// results with only another country's prices get a note to search once
    /// more for theirs (or, for a model on this PC, Yap's search for them:
    /// [`Turn::take_local_retry`]), and if that finds none either, a note to
    /// say so in one line instead of offering to look ([`Turn::local_note`]).
    prices: bool,
    local_found: bool,
    retried: bool,
    yap_retries: bool,
    pending_retry: Option<String>,
    /// The web text the model was given this turn (results, page parts):
    /// what an answer's prices are checked against
    /// (`about_you::restore_price_signs`).
    pub texts: Vec<String>,
}

impl Turn {
    /// `earlier`: the person's earlier messages in the conversation (their
    /// links count as theirs).
    pub fn new(settings: Settings, endpoints: Endpoints, question: &str, earlier: &[&str]) -> Turn {
        let linked = std::iter::once(question).chain(earlier.iter().copied()).flat_map(links_in).map(|u| normal_url(&u)).collect();
        let prices = asks_about_prices(question) && settings.local.as_ref().is_some_and(|l| l.tells_apart);
        Turn {
            settings,
            endpoints,
            question: question.to_string(),
            linked,
            sources: Vec::new(),
            searches: 0,
            reads: 0,
            seen: false,
            grounded: false,
            prices,
            local_found: false,
            retried: false,
            yap_retries: false,
            pending_retry: None,
            texts: Vec::new(),
        }
    }

    /// The units this turn's temperatures, speeds and distances go in: the
    /// person's, each unless the question asks for the other
    /// (`about_you::units::TheirUnits`).
    fn their_units(&self) -> crate::ai::about_you::units::TheirUnits {
        crate::ai::about_you::units::TheirUnits::for_question(self.settings.units, &self.question)
    }

    /// Yap runs the search for local prices itself (a model on this PC),
    /// instead of telling the model to.
    pub fn set_yap_retries(&mut self, on: bool) {
        self.yap_retries = on;
    }

    /// A search, aimed at the person's country when the question is about
    /// prices ([`local_query`]): `None` when it stays as it is.
    pub fn local_query(&self, query: &str) -> Option<String> {
        local_query(query, &self.question, self.settings.local.as_ref())
    }

    /// The search for local prices Yap owes the model (results with only
    /// another country's prices, a model on this PC): once.
    pub fn take_local_retry(&mut self) -> Option<String> {
        self.pending_retry.take()
    }

    /// What a search's result says about prices where the person is
    /// (`local_prices` in it), from its pages' text: after results with
    /// theirs that followed some without, "answer with those"; the first
    /// results with only another country's, "search once more" (or nothing,
    /// when Yap runs that search itself); after that, "say there are none,
    /// in one line". `None` otherwise. (Keeps the turn's state.)
    fn local_note(&mut self, query: &str, text: &str) -> Option<String> {
        if !self.prices || self.local_found {
            return None;
        }
        let local = self.settings.local.clone()?;
        let (p, s) = (&local.place, local.sign());
        if local.local_in(text) {
            self.local_found = true;
            return self.retried.then(|| format!("These results give {p} prices (in {s}): answer with those, as the pages give them."));
        }
        if self.retried {
            return Some(format!(
                "No {p} prices (in {s}) here either. Start your answer with one short line saying you couldn't find {p} prices, then give the prices you found as they are, saying which country they're for. Don't offer to look further."
            ));
        }
        if !local.foreign_in(text) {
            return None;
        }
        self.retried = true;
        let retry = local_retry_query(query, &local);
        if self.yap_retries {
            self.pending_retry = Some(retry);
            return None;
        }
        Some(format!("None of these results gives a {p} price (in {s}). Search once more for the {p} price, like \"{retry}\", before you answer."))
    }

    /// `read_page` may also open the sources of earlier answers in the
    /// conversation (found by an earlier search; their searches are replayed
    /// to the model with their addresses, `chats::history_messages`): "tell
    /// me more about the first one".
    pub fn with_earlier_sources<'a>(mut self, urls: impl IntoIterator<Item = &'a str>) -> Turn {
        self.linked.extend(urls.into_iter().map(normal_url));
        self
    }

    /// Web content is in the turn: the action tools are off for the rest of
    /// it (`tools::refused_after_web`).
    pub fn seen(&self) -> bool {
        self.seen
    }

    /// A search or a page read ran in this turn, whatever it found: an
    /// answer saying it searched isn't making it up.
    pub fn looked(&self) -> bool {
        self.searches > 0 || self.reads > 0
    }

    /// A source's number: the one it has, or the next.
    pub fn number(&mut self, title: &str, url: &str) -> usize {
        let key = normal_url(url);
        if let Some(s) = self.sources.iter().find(|s| normal_url(&s.url) == key) {
            return s.n;
        }
        let n = self.sources.len() + 1;
        self.sources.push(Source { n, title: tidy(title, 120), url: url.to_string(), site: source_site(title, url), ..Default::default() });
        n
    }

    /// Whether `read_page` may open `url`: a link from the person, or a
    /// result of a search in this turn (or a page already read).
    pub fn allowed(&self, url: &str) -> bool {
        let key = normal_url(url);
        self.linked.contains(&key) || self.sources.iter().any(|s| normal_url(&s.url) == key)
    }

    /// Run `web_search` or `read_page`: a tools.rs-shaped result
    /// (`{success, data, displayText, error?}`).
    pub async fn execute(&mut self, name: &str, args: &Value) -> Value {
        match name {
            "web_search" => self.web_search(args).await,
            "read_page" => self.read(args).await,
            other => failed(format!("Unknown web tool {other}"), format!("Unknown tool: {other}")),
        }
    }

    async fn web_search(&mut self, args: &Value) -> Value {
        let query = args["query"].as_str().unwrap_or("").trim().to_string();
        if query.is_empty() {
            return failed("query is empty: say what to search for.".into(), "Couldn't search the web: no query".into());
        }
        let q = tidy(&query, 60);
        if self.searches >= MAX_SEARCHES {
            return failed(
                format!("You've searched {MAX_SEARCHES} times in this turn: answer from what you found, and say what's missing."),
                format!("Didn't search for \"{q}\": {MAX_SEARCHES} searches a turn"),
            );
        }
        self.searches += 1;
        match search(&self.settings, &self.endpoints, &query, &self.question).await {
            Err(e) => failed(e.clone(), format!("Couldn't search the web: {}", short(&e))),
            Ok(found) => {
                if !found.hits.is_empty() {
                    self.seen = true;
                }
                if found.native == Some(native::Provider::Gemini) && !found.hits.is_empty() {
                    self.grounded = true;
                }
                // Temperatures, speeds and distances in the person's units
                // first ("2°C (35°F)", "19 mph (30 km/h)"), with a line
                // saying so for each (about_you.rs, about_you/units.rs).
                let units = self.their_units();
                let original: Vec<&str> =
                    found.hits.iter().flat_map(|h| [h.title.as_str(), h.text.as_str()]).chain(found.summary.as_deref()).collect();
                let unit_notes = units.notes(&original.join("\n"));
                let theirs = |s: &str| units.convert(s).unwrap_or_else(|| s.to_string());
                let mut found = found;
                for h in &mut found.hits {
                    h.title = theirs(&h.title);
                    h.text = theirs(&h.text);
                }
                found.summary = found.summary.as_deref().map(theirs);
                let mut numbers = Vec::new();
                let results: Vec<Value> = found
                    .hits
                    .iter()
                    .map(|h| {
                        let n = self.number(&h.title, &h.url);
                        numbers.push(n);
                        let mut r = json!({ "n": n, "title": h.title, "site": source_site(&h.title, &h.url), "url": h.url, "text": h.text });
                        if let Some(p) = &h.published {
                            r["published"] = json!(p);
                        }
                        r
                    })
                    .collect();
                let display = search_chip(&query, &found, &self.settings);
                let mut data = json!({
                    "query": query,
                    "from": found.from,
                    "results": results,
                    "note": DATA_NOT_INSTRUCTIONS,
                    "cite": "Cite a result by its number in square brackets, like [1].",
                });
                // The provider's own short answer, citing the same numbers.
                if let Some(summary) = &found.summary {
                    data["summary"] = json!(native::render_summary(summary, &numbers));
                    data["cite"] = json!("Cite a result by its number in square brackets, like [1]. The summary is the search's own short answer: its numbers are the results'.");
                }
                if let Some(note) = &found.note {
                    data["service_error"] = json!(note);
                }
                if results.is_empty() {
                    data["hint"] = json!("Nothing found. Try other words once, or tell the user the web didn't answer it.");
                }
                let text: Vec<&str> =
                    found.hits.iter().flat_map(|h| [h.title.as_str(), h.text.as_str()]).chain(found.summary.as_deref()).collect();
                let text = text.join("\n");
                if let Some(note) = self.local_note(&query, &text) {
                    data["local_prices"] = json!(note);
                }
                for (key, note) in unit_notes {
                    data[key] = json!(note);
                }
                self.texts.push(text);
                ok(data, display)
            }
        }
    }

    async fn read(&mut self, args: &Value) -> Value {
        let url = args["url"].as_str().unwrap_or("").trim().to_string();
        let shown = short_url(&url);
        if url.is_empty() {
            return failed("url is required: a page from the search results or the user's message.".into(), "Couldn't read a page: no address".into());
        }
        if let Err(e) = check_url(&url) {
            return failed(e.clone(), format!("Didn't read {shown}: {}", short(&e)));
        }
        if !self.allowed(&url) {
            return failed(
                "read_page only opens a page from this turn's search results or one the user linked in their message. Search for it first, or answer from what you have.".into(),
                format!("Didn't read {shown}: not a link from you or the search"),
            );
        }
        let offset = args["offset"].as_u64().or_else(|| args["offset"].as_str().and_then(|s| s.trim().parse().ok())).unwrap_or(0) as usize;
        if self.reads >= MAX_READS {
            return failed(
                format!("You've read {MAX_READS} pages in this turn: answer from what you have."),
                format!("Didn't read {shown}: {MAX_READS} pages a turn"),
            );
        }
        self.reads += 1;
        match read_page(&self.endpoints, &url).await {
            Err(e) => failed(e.clone(), format!("Couldn't read {shown}: {}", short(&e))),
            Ok(page) => {
                self.seen = true;
                let n = self.number(&page.title, &url);
                let (part, next) = page_part(&page.text, offset, PAGE_TOKENS);
                // Temperatures, speeds and distances in the person's units
                // first (about_you.rs, about_you/units.rs).
                let units = self.their_units();
                let unit_notes = units.notes(&part);
                let part = units.convert(&part).unwrap_or(part);
                self.texts.push(part.clone());
                let mut data = json!({
                    "n": n,
                    "title": page.title,
                    "site": page.site,
                    "url": page.url,
                    "text": part,
                    "more": next.is_some(),
                    "note": DATA_NOT_INSTRUCTIONS,
                });
                if let Some(next) = next {
                    data["next_offset"] = json!(next);
                }
                for (key, note) in unit_notes {
                    data[key] = json!(note);
                }
                ok(data, format!("Read {}", short_url(&page.url)))
            }
        }
    }

    /// The sources to keep with the answer: those it cites (by number), or
    /// every one when it cites none. After Google Search grounded the turn
    /// (Gemini), every link it gave, the cited first, five at most: Google's
    /// terms let a grounded result feed a follow-up answer when that answer
    /// shows its links (up to 5).
    pub fn sources_for(&self, reply: &str) -> Vec<Source> {
        if !self.grounded {
            return cited_sources(&self.sources, reply);
        }
        let cited = citations(reply);
        let (mut first, rest): (Vec<Source>, Vec<Source>) = self.sources.iter().cloned().partition(|s| cited.contains(&s.n));
        first.extend(rest);
        first.truncate(MAX_RESULTS);
        first.sort_by_key(|s| s.n);
        first
    }
}

/// An answer's citations in gpt-oss's own style ("【1】", "【2†L4-L9】" —
/// its browsing habit, seen in Yap's tool loop on Groq) written as Yap's
/// "[1]", so they become pills; a marker without a number goes. Pure.
pub fn bracket_citations(reply: &str) -> String {
    static OSS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]?【(\d{1,2})?(?:†[^】]*)?】").expect("static regex"));
    if !reply.contains('【') {
        return reply.to_string();
    }
    OSS.replace_all(reply, |c: &regex::Captures| c.get(1).map_or(String::new(), |n| format!(" [{}]", n.as_str())))
        .into_owned()
}

/// The citation numbers in an answer ("[1]", "[2, 3]", "[1][4]"). Pure.
pub fn citations(reply: &str) -> Vec<usize> {
    static CITE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[(\d{1,2}(?:\s*[,–-]\s*\d{1,2})*)\]").expect("static regex"));
    let mut out: Vec<usize> = Vec::new();
    for m in CITE.captures_iter(reply) {
        let whole = m.get(0).expect("match");
        // A markdown link's text ("[1](https://…)") isn't a citation.
        if reply[whole.end()..].starts_with('(') {
            continue;
        }
        let inner = &m[1];
        let nums: Vec<usize> = inner.split([',', '–', '-']).filter_map(|n| n.trim().parse().ok()).collect();
        let range = inner.contains(['–', '-']) && nums.len() == 2 && nums[0] < nums[1] && nums[1] - nums[0] <= 10;
        let nums: Vec<usize> = if range { (nums[0]..=nums[1]).collect() } else { nums };
        for n in nums {
            if n > 0 && !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out
}

/// The sources an answer cites, in number order; every source when it cites
/// none of them. Pure.
pub fn cited_sources(sources: &[Source], reply: &str) -> Vec<Source> {
    let cited = citations(reply);
    let picked: Vec<Source> = sources.iter().filter(|s| cited.contains(&s.n)).cloned().collect();
    if picked.is_empty() {
        sources.to_vec()
    } else {
        picked
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::tests::{endpoints_of, fake, offline, settings, uk_prices};

    #[test]
    fn a_page_is_read_only_from_a_link_the_person_gave_or_a_search_found() {
        let mut turn = Turn::new(settings(""), offline(), "Read https://bg3.wiki/wiki/Gale and tell me", &["also www.example.com/guide."]);
        assert!(turn.allowed("https://bg3.wiki/wiki/Gale"));
        assert!(turn.allowed("https://bg3.wiki/wiki/Gale/#Location"));
        assert!(turn.allowed("https://www.example.com/guide"));
        assert!(!turn.allowed("https://evil.example/?notes=secret"));
        // A search result counts once it's numbered.
        assert_eq!(turn.number("Ranni", "https://eldenring.fandom.com/wiki/Ranni"), 1);
        assert_eq!(turn.number("Ranni again", "https://eldenring.fandom.com/wiki/Ranni/"), 1);
        assert!(turn.allowed("https://eldenring.fandom.com/wiki/Ranni"));
        assert_eq!(turn.sources[0].site, "eldenring.fandom.com");
        assert!(!turn.seen());
        // An earlier answer's sources (an earlier search found them) count too.
        let turn = Turn::new(settings(""), offline(), "Tell me more about the first one", &[])
            .with_earlier_sources(["https://indiegames.example/best-2026"]);
        assert!(turn.allowed("https://indiegames.example/best-2026/"));
        assert!(!turn.allowed("https://indiegames.example/other"));
        assert!(!turn.looked());
    }

    #[test]
    fn citations_are_read_from_the_answer() {
        assert_eq!(citations("Gale is on the beach [1]. He can be recruited [2][3]."), [1, 2, 3]);
        assert_eq!(citations("See [1, 4] and [2–3]."), [1, 4, 2, 3]);
        assert_eq!(citations("A [link](https://x.y) and [1](https://a.b) aren't citations."), Vec::<usize>::new());
        let sources: Vec<Source> = (1..=3).map(|n| Source { n, title: format!("T{n}"), url: format!("https://e{n}.com/"), site: format!("e{n}.com"), ..Default::default() }).collect();
        assert_eq!(cited_sources(&sources, "Fact [2].").iter().map(|s| s.n).collect::<Vec<_>>(), [2]);
        assert_eq!(cited_sources(&sources, "No citations.").len(), 3);
    }

    #[tokio::test]
    async fn results_without_local_prices_lead_to_one_more_search_then_a_plain_line() {
        let us = fake((500, Value::Null), json!({ "results": [{ "title": "Pricing | xAI", "url": "https://x.ai/pricing", "highlights": ["Plus: $8/month. Premium: $30/month."] }] }));
        let uk = fake((500, Value::Null), json!({ "results": [{ "title": "Grok UK prices", "url": "https://uk.example/grok", "highlights": ["SuperGrok costs £24.99 a month in the UK."] }] }));
        let s = || Settings { local: Some(uk_prices()), country: Some("GB".into()), ..settings("exa-key") };
        let question = "How much is a Grok subscription?";
        let search = |q: &str| json!({ "query": q });

        // A cloud model: told to search once more, then told to say so.
        let mut turn = Turn::new(s(), endpoints_of(&us), question, &[]);
        let first = turn.execute("web_search", &search("Grok plans price UK")).await;
        let note = first["data"]["local_prices"].as_str().unwrap();
        assert!(note.starts_with("None of these results gives a UK price (in £). Search once more for the UK price, like \"Grok plans price UK £\""), "{note}");
        assert_eq!(turn.take_local_retry(), None);
        let second = turn.execute("web_search", &search("Grok plans price UK £")).await;
        let note = second["data"]["local_prices"].as_str().unwrap();
        assert!(note.starts_with("No UK prices (in £) here either. Start your answer with one short line saying you couldn't find UK prices"), "{note}");

        // A model on this PC: Yap owes it the second search, once.
        let mut turn = Turn::new(s(), endpoints_of(&us), question, &[]);
        turn.set_yap_retries(true);
        let first = turn.execute("web_search", &search("Grok tiers price UK")).await;
        assert!(first["data"].get("local_prices").is_none());
        assert_eq!(turn.take_local_retry().as_deref(), Some("Grok tiers price UK £"));
        assert_eq!(turn.take_local_retry(), None, "once");

        // Their prices after some without: "answer with those".
        let mut turn = Turn::new(s(), endpoints_of(&us), question, &[]);
        turn.execute("web_search", &search("Grok cost UK")).await;
        turn.endpoints = endpoints_of(&uk);
        let second = turn.execute("web_search", &search("Grok cost UK £")).await;
        assert_eq!(second["data"]["local_prices"], "These results give UK prices (in £): answer with those, as the pages give them.");

        // Theirs at once, or not a price question: nothing said.
        let mut turn = Turn::new(s(), endpoints_of(&uk), question, &[]);
        assert!(turn.execute("web_search", &search("Grok monthly price UK")).await["data"].get("local_prices").is_none());
        let mut turn = Turn::new(s(), endpoints_of(&us), "What is Grok?", &[]);
        assert!(turn.execute("web_search", &search("Grok chatbot")).await["data"].get("local_prices").is_none());
    }

    #[tokio::test]
    async fn results_in_the_other_temperature_scale_say_to_convert() {
        use crate::ai::about_you::Units;
        let f = fake((500, Value::Null), json!({ "results": [{ "title": "Glenbrackie Weather Today | 35°F, Overcast", "url": "https://weather.example/glenbrackie", "highlights": ["Overcast, 35°F, feels like 30°F. Wind 2–4 mph."] }] }));
        let search = json!({ "query": "Glenbrackie weather today" });
        let with = |units| Settings { units, ..settings("exa-key") };
        // Someone in the UK: °C first in the results, and a line next to them.
        let mut turn = Turn::new(with(Some(Units::MetricMiles)), endpoints_of(&f), "What's the weather like in Glenbrackie today?", &[]);
        let r = turn.execute("web_search", &search).await;
        let hit = &r["data"]["results"][0];
        assert_eq!(hit["title"], "Glenbrackie Weather Today | 2°C (35°F), Overcast");
        assert!(hit["text"].as_str().unwrap().contains("Overcast, 2°C (35°F), feels like -1°C (30°F)."), "{hit}");
        assert!(r["data"]["temperatures"].as_str().unwrap().starts_with("The user uses °C: Yap has put °C before these pages' °F figures"), "{r}");
        // In the US, nothing known, or asked in °F: as the pages give them.
        for (units, question) in [
            (Some(Units::Us), "What's the weather like in Glenbrackie today?"),
            (None, "What's the weather like in Glenbrackie today?"),
            (Some(Units::MetricMiles), "What's the weather like in Glenbrackie today, in Fahrenheit?"),
        ] {
            let mut turn = Turn::new(with(units), endpoints_of(&f), question, &[]);
            let r = turn.execute("web_search", &search).await;
            assert!(r["data"].get("temperatures").is_none(), "{units:?}");
            assert_eq!(r["data"]["results"][0]["title"], "Glenbrackie Weather Today | 35°F, Overcast");
        }
    }

    #[test]
    fn gpt_oss_style_citations_become_yaps() {
        // Seen in the eval harness on Groq gpt-oss-20b, 2026-10-18.
        assert_eq!(bracket_citations("It is 1,345 metres (4,411 ft) tall【1】."), "It is 1,345 metres (4,411 ft) tall [1].");
        assert_eq!(bracket_citations("Out on 9 Oct 【2†L4-L9】 and【†source】 more."), "Out on 9 Oct [2] and more.");
        assert_eq!(bracket_citations("Plain [1]."), "Plain [1].");
        assert_eq!(citations(&bracket_citations("a【1】 b【3†L1】")), [1, 3]);
    }

    #[test]
    fn links_in_messages_are_found() {
        assert_eq!(
            links_in("read https://bg3.wiki/wiki/Gale, and www.example.com/x. Not ftp://a.b"),
            ["https://bg3.wiki/wiki/Gale", "https://www.example.com/x"]
        );
        assert_eq!(short_url("https://www.bg3.wiki/wiki/Gale/"), "bg3.wiki/wiki/Gale");
    }
}

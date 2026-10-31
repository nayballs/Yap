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
    search, search_chip, short, source_site, tidy, Endpoints, Settings, DATA_NOT_INSTRUCTIONS, DEEP_READS, DEEP_SEARCHES,
    MAX_READS, MAX_RESULTS, MAX_SEARCHES, PAGE_TOKENS,
};

/// **How far a turn may look**: its searches and page reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reach {
    pub searches: usize,
    pub reads: usize,
}

impl Reach {
    /// Every turn's: [`MAX_SEARCHES`] and [`MAX_READS`].
    pub const USUAL: Reach = Reach { searches: MAX_SEARCHES, reads: MAX_READS };

    /// **A strong model writing a document** (`documents::rich::strong`):
    /// [`DEEP_SEARCHES`], and as many page reads (a [`PAGE_TOKENS`] part
    /// each) as two-fifths of the model's request holds (`request`: its
    /// tokens, `chat/budget.rs`; `None`: no limit known), from
    /// [`MAX_READS`] to [`DEEP_READS`] — Claude Haiku 5.5, held to 80K
    /// tokens a request at its cheaper rate, gets all ten; an 8K model,
    /// were it strong, three. What the reads bring is still fitted to
    /// what's left of the request, result by result (`agent::Room::fit`).
    /// Pure.
    pub fn deep(request: Option<u64>) -> Reach {
        Reach::deep_paged(request, PAGE_TOKENS)
    }

    /// [`Reach::deep`] for pages of `page` tokens each: a strong Claude's
    /// own web fetch puts up to `native::FETCH_TOKENS` of a page in the
    /// request (and Yap can't trim what Anthropic fetched), so its reads are
    /// sized by that — Claude Haiku 5.5's 80K gets 4, Opus the full 10
    /// (2026-10-31: a document's turn opens the pages its facts come from).
    /// Pure.
    pub fn deep_paged(request: Option<u64>, page: usize) -> Reach {
        let reads = request.map_or(DEEP_READS, |r| ((r * 2 / 5) / page.max(1) as u64) as usize).clamp(MAX_READS, DEEP_READS);
        Reach { searches: DEEP_SEARCHES, reads }
    }
}

/// A web address, compared: scheme and host lowercased, no "www.", no
/// fragment, no tracking parameters (`utm_…`), no trailing slash (before
/// its query too: "…/filedetails/?id=1" is "…/filedetails?id=1"). Pure.
pub fn normal_url(url: &str) -> String {
    match Url::parse(url.trim()) {
        Ok(mut u) => {
            u.set_fragment(None);
            if let Some(host) = u.host_str().and_then(|h| h.strip_prefix("www.")).map(str::to_string) {
                let _ = u.set_host(Some(&host));
            }
            if u.query().is_some() {
                let kept: Vec<(String, String)> = u.query_pairs().filter(|(k, _)| !k.starts_with("utm_")).map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
                if kept.is_empty() {
                    u.set_query(None);
                } else {
                    u.query_pairs_mut().clear().extend_pairs(kept);
                }
            }
            let path = u.path().trim_end_matches('/').to_string();
            u.set_path(&path);
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
    /// Citation numbers given to other sources this turn (the calendar's
    /// meetings, `chat/tools/calendar.rs`): one numbering for the answer.
    reserved: usize,
    /// How far this turn may look now ([`Reach::USUAL`] until it goes deep).
    reach: Reach,
    /// How far it may look once it writes a document (a strong model's
    /// turn, [`Turn::may_go_deep`]); `None`: never further.
    deep_reach: Option<Reach>,
    /// **The outline comes first** ([`Turn::outline_first`]): a strong
    /// model's turn that makes a new document looks nothing up until its
    /// first step is done — Claude's own web tools stay out of that step's
    /// request, Yap's are refused unless the step makes the outline too.
    outline_first: bool,
    /// A document cited this turn's pages ([`Turn::cited_in`]): the answer
    /// keeps every source, the cited and opened ones marked.
    documented: bool,
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
            reserved: 0,
            reach: Reach::USUAL,
            deep_reach: None,
            outline_first: false,
            documented: false,
        }
    }

    /// A strong model's turn (`documents::rich::strong`): once it writes a
    /// document ([`Turn::go_deep`]), it may look as far as `reach`.
    pub fn may_go_deep(mut self, reach: Reach) -> Turn {
        self.deep_reach = Some(reach);
        self
    }

    /// **A new document's turn: the outline first** (2026-10-31, as Claude
    /// Docs does it: the title and a placeholder per section open in the
    /// panel at once, then the research, then each section). Claude Opus
    /// 5.5 researched for 1 min 44 s before its `create_document`, so the
    /// panel opened after 1 min 52 s. Until the turn's first step is done
    /// ([`Turn::outline_settled`]) nothing is looked up: Claude's own web
    /// tools stay out of the request ([`Turn::server_tools`]), and the loop
    /// refuses Yap's unless the step makes the outline too
    /// (`documents::OUTLINE_FIRST`).
    pub fn outline_first(mut self) -> Turn {
        self.outline_first = true;
        self
    }

    /// The outline is still to come ([`Turn::outline_first`]).
    pub fn outline_pending(&self) -> bool {
        self.outline_first
    }

    /// The first step is done (with its outline, or without: told once, the
    /// model may look things up from now on).
    pub fn outline_settled(&mut self) {
        self.outline_first = false;
    }

    /// **This turn writes a document**: it looks further from now on — the
    /// deeper [`Reach`], and the provider's own search opening more pages
    /// and saying more (`native::NativeSearch::deep`; Claude's own web fetch
    /// as many pages a step as the reach reads). Only a turn that
    /// [`Turn::may_go_deep`]; `true` when it went deeper now (once).
    pub fn go_deep(&mut self) -> bool {
        let Some(deep) = self.deep_reach.filter(|d| *d != self.reach) else { return false };
        self.reach = deep;
        if let Some(n) = self.settings.native.as_mut() {
            n.deep = true;
            n.deep_fetches = Some(deep.reads as u32);
        }
        tracing::info!(searches = deep.searches, reads = deep.reads, "Web: a document's turn looks further");
        true
    }

    /// The turn looks further than every turn ([`Turn::go_deep`]).
    pub fn deep(&self) -> bool {
        self.reach != Reach::USUAL
    }

    /// **Claude's own web tools for Chat's request**, when it searches there
    /// (`native::NativeSearch::in_chat`, `llm::anthropic`): its web search
    /// and web fetch, deeper once the turn writes a document — none while a
    /// new document's outline is still to come ([`Turn::outline_first`]).
    /// `None`: Yap's `web_search` and `read_page` are the turn's.
    pub fn server_tools(&self) -> Option<Vec<Value>> {
        let n = self.settings.native.as_ref().filter(|n| n.in_chat)?;
        Some(if self.outline_first { Vec::new() } else { n.server_tools() })
    }

    /// **What Claude's own web tools did in one reply** (`llm::anthropic`'s
    /// `yap_web`): each search and page read a chip, its pages numbered as
    /// sources — web content is in the turn now (the safety rule, and an
    /// answer that says it searched isn't making it up) — and the numbers
    /// of the pages its answer cites, in `cites`' order. A server tool's
    /// error is a chip, never the turn's end. A model evaluation records
    /// them as the `web_search` / `read_page` calls they stand for.
    pub fn took_server(&mut self, w: &Value) -> (Vec<String>, Vec<usize>) {
        let who = native::Provider::Anthropic.name();
        let mut chips = Vec::new();
        for s in w["searches"].as_array().into_iter().flatten() {
            let q = tidy(s["query"].as_str().unwrap_or(""), 60);
            if let Some(e) = s["error"].as_str() {
                chips.push(format!("Couldn't search the web with {who}: {e}"));
                continue;
            }
            self.searches += 1;
            let mut results = Vec::new();
            for r in s["results"].as_array().into_iter().flatten() {
                let (url, title) = (r["url"].as_str().unwrap_or(""), r["title"].as_str().unwrap_or(""));
                if check_url(url).is_err() {
                    continue;
                }
                let n = self.number(title, url);
                results.push(json!({ "n": n, "title": title, "url": url }));
            }
            if !results.is_empty() {
                self.seen = true;
            }
            let chip = if results.is_empty() { format!("Searched the web with {who}: \"{q}\" · nothing found") } else { format!("Searched the web with {who}: \"{q}\"") };
            crate::ai::eval::tool_called("web_search", &json!({ "query": s["query"] }), &json!({ "success": true, "data": { "results": results }, "displayText": chip }));
            chips.push(chip);
        }
        for f in w["fetches"].as_array().into_iter().flatten() {
            let url = f["url"].as_str().unwrap_or("");
            let shown = short_url(url);
            if let Some(e) = f["error"].as_str() {
                chips.push(format!("Couldn't read {shown}: {e}"));
                continue;
            }
            if check_url(url).is_err() {
                continue;
            }
            self.reads += 1;
            self.seen = true;
            let n = self.opened(f["title"].as_str().unwrap_or(""), url);
            let chip = format!("Read {shown}");
            crate::ai::eval::tool_called("read_page", &json!({ "url": url }), &json!({ "success": true, "data": { "n": n, "url": url }, "displayText": chip }));
            chips.push(chip);
        }
        let mut numbers = Vec::new();
        for c in w["cites"].as_array().into_iter().flatten() {
            if let Some(t) = c["text"].as_str().filter(|t| !t.is_empty()) {
                self.texts.push(t.to_string());
            }
            numbers.push(self.number(c["title"].as_str().unwrap_or(""), c["url"].as_str().unwrap_or("")));
        }
        (chips, numbers)
    }

    /// How far it may look now.
    #[cfg(test)]
    pub fn reach(&self) -> Reach {
        self.reach
    }

    /// A citation number for a source that isn't a web page (a meeting on
    /// the calendar): the next one, never a web source's.
    pub fn reserve_number(&mut self) -> usize {
        self.reserved += 1;
        self.sources.len() + self.reserved
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
        let n = self.sources.len() + self.reserved + 1;
        self.sources.push(Source { n, title: tidy(title, 120), url: url.to_string(), site: source_site(title, url), ..Default::default() });
        n
    }

    /// **A page the turn opened and read**: its number ([`Turn::number`]),
    /// marked opened, and titled with the page's own title — a search's
    /// result is often titled with the site or the game ("Divinity: Original
    /// Sin 2 · steamcommunity.com", for 30 different guides), the page
    /// itself says what it is.
    fn opened(&mut self, title: &str, url: &str) -> usize {
        let n = self.number(title, url);
        if let Some(s) = self.sources.iter_mut().find(|s| s.n == n) {
            s.opened = true;
            let title = tidy(title, 120);
            if !title.is_empty() {
                s.site = source_site(&title, &s.url);
                s.title = title;
            }
        }
        n
    }

    /// **What a document cites** (its text as the model wrote it, before
    /// Yap links its `[n]`): those sources marked cited — by number, or by
    /// a markdown link to their page (a strong Claude links a page itself).
    /// The answer then keeps all the turn's sources, these first in its row
    /// ([`Turn::sources_for`]).
    pub fn cited_in(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        let numbers = citations(text);
        let links: Vec<String> = links_in(text).iter().map(|u| normal_url(u)).collect();
        for s in &mut self.sources {
            if numbers.contains(&s.n) || (!s.url.is_empty() && links.contains(&normal_url(&s.url))) {
                s.cited = true;
            }
        }
        self.documented = true;
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
        let most = self.reach.searches;
        if self.searches >= most {
            // A document's turn: write it now, from the pages it read — its
            // own knowledge for advice only, a fact no page gave said to be
            // unconfirmed (as the documents rule says).
            let then = if self.deep() {
                "write the document from the pages you read: advice from your own knowledge is fine (marked as yours), but a requirement, level, number or location no page gave is written as not confirmed, never guessed"
            } else {
                "answer from what you found, and say what's missing"
            };
            return failed(format!("You've searched {most} times in this turn: {then}."), format!("Didn't search for \"{q}\": {most} searches a turn"));
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
        let most = self.reach.reads;
        if self.reads >= most {
            let then = if self.deep() {
                "write the document from the pages you read (a fact none of them gave: not confirmed, never guessed)"
            } else {
                "answer from what you have"
            };
            return failed(format!("You've read {most} pages in this turn: {then}."), format!("Didn't read {shown}: {most} pages a turn"));
        }
        self.reads += 1;
        match read_page(&self.endpoints, &url).await {
            Err(e) => failed(e.clone(), format!("Couldn't read {shown}: {}", short(&e))),
            Ok(page) => {
                self.seen = true;
                let n = self.opened(&page.title, &url);
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
    /// every one when it cites none. **A document's turn** ([`Turn::cited_in`])
    /// keeps every one — the answer is a line ("It's open on the right.")
    /// and the document cites by link — those the document or the answer
    /// cites marked `cited`, the pages read `opened`: the Sources row shows
    /// them first, the rest behind "+N more" (2026-10-31: 46 sources under
    /// one guide, most of them search results nobody opened). After Google
    /// Search grounded the turn (Gemini), every link it gave, the cited
    /// first, five at most: Google's terms let a grounded result feed a
    /// follow-up answer when that answer shows its links (up to 5).
    pub fn sources_for(&self, reply: &str) -> Vec<Source> {
        if self.documented && !self.grounded {
            let cited = citations(reply);
            return self
                .sources
                .iter()
                .cloned()
                .map(|mut s| {
                    s.cited |= cited.contains(&s.n);
                    s
                })
                .collect();
        }
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

    #[test]
    fn a_meeting_and_a_page_never_share_a_number() {
        let mut turn = Turn::new(settings("exa-key"), offline(), "what's on Friday, and the weather?", &[]);
        assert_eq!(turn.number("Forecast", "https://weather.example/a"), 1);
        assert_eq!(turn.reserve_number(), 2); // the calendar's meeting
        assert_eq!(turn.number("Radar", "https://weather.example/b"), 3);
        assert_eq!(turn.number("Forecast", "https://weather.example/a"), 1);
        assert_eq!(turn.reserve_number(), 4);
        assert_eq!(turn.sources.iter().map(|s| s.n).collect::<Vec<_>>(), [1, 3]);
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
    fn a_documents_turn_looks_further_only_when_it_may() {
        // Sized to the model's request: Haiku's 80K at its cheaper rate, a
        // 1M model, an 8K one; no limit known.
        assert_eq!(Reach::deep(Some(80_000)), Reach { searches: DEEP_SEARCHES, reads: DEEP_READS });
        assert_eq!(Reach::deep(Some(990_000)).reads, DEEP_READS);
        assert_eq!(Reach::deep(Some(7_000)).reads, MAX_READS);
        assert_eq!(Reach::deep(Some(30_000)).reads, 4);
        assert_eq!(Reach::deep(None).reads, DEEP_READS);
        // Every turn: the usual reach, and going deep does nothing…
        let mut turn = Turn::new(settings(""), offline(), "Make me a guide", &[]);
        assert!(!turn.go_deep());
        assert_eq!(turn.reach(), Reach::USUAL);
        assert!(!turn.deep());
        // …unless the model is a strong one: then once, when it writes.
        let mut turn = Turn::new(settings(""), offline(), "Make me a guide", &[]).may_go_deep(Reach::deep(Some(80_000)));
        assert_eq!(turn.reach(), Reach::USUAL, "not before it writes a document");
        assert!(turn.go_deep());
        assert!(!turn.go_deep(), "once");
        assert!(turn.deep());
        assert_eq!(turn.reach().searches, DEEP_SEARCHES);
    }

    #[tokio::test]
    async fn the_search_cap_follows_the_turns_reach() {
        let f = fake((500, Value::Null), json!({ "results": [{ "title": "Emberfall wiki", "url": "https://emberfall.example/levels", "highlights": ["Level 2: Ember Bolt."] }] }));
        let search = |n: usize| json!({ "query": format!("Emberfall levels part {n}") });
        // Every turn: three.
        let mut turn = Turn::new(settings("exa-key"), endpoints_of(&f), "What do I get at level 2?", &[]);
        for n in 0..MAX_SEARCHES {
            assert_eq!(turn.execute("web_search", &search(n)).await["success"], json!(true));
        }
        let r = turn.execute("web_search", &search(9)).await;
        assert_eq!(r["displayText"], json!("Didn't search for \"Emberfall levels part 9\": 3 searches a turn"));
        assert!(r["error"].as_str().unwrap().contains("say what's missing"), "{r}");
        // A strong model's document turn: ten, then "write it".
        let mut turn = Turn::new(settings("exa-key"), endpoints_of(&f), "Make me a level-by-level guide", &[]).may_go_deep(Reach::deep(None));
        turn.go_deep();
        for n in 0..DEEP_SEARCHES {
            assert_eq!(turn.execute("web_search", &search(n)).await["success"], json!(true), "search {n}");
        }
        let r = turn.execute("web_search", &search(99)).await;
        assert_eq!(r["displayText"], json!("Didn't search for \"Emberfall levels part 99\": 10 searches a turn"));
        // Write it from the pages read: its own knowledge for advice, a
        // fact no page gave not confirmed — never "fill in the rest".
        let e = r["error"].as_str().unwrap();
        assert!(e.contains("write the document from the pages you read") && e.contains("not confirmed, never guessed"), "{r}");
        assert!(!e.contains("filling in the rest"), "{r}");
    }

    #[test]
    fn claudes_page_reads_are_sized_to_what_its_request_holds() {
        // Its own web fetch puts up to 8,000 tokens of a page in the request:
        // Haiku's 80K holds 4, a 1M model 10, an 8K one the usual 3.
        assert_eq!(Reach::deep_paged(Some(80_000), native::FETCH_TOKENS as usize).reads, 4);
        assert_eq!(Reach::deep_paged(Some(990_000), native::FETCH_TOKENS as usize).reads, DEEP_READS);
        assert_eq!(Reach::deep_paged(Some(8_000), native::FETCH_TOKENS as usize).reads, MAX_READS);
        assert_eq!(Reach::deep_paged(Some(80_000), native::FETCH_TOKENS as usize).searches, DEEP_SEARCHES);
        // Going deep, its fetches follow those reads; its searches stay at 10.
        let mut n = native::NativeSearch::new(native::Provider::Anthropic, "claude-haiku-5-5", "https://api.anthropic.com/v1", "k");
        n.in_chat = true;
        let mut turn = Turn::new(Settings { native: Some(n), ..settings("") }, offline(), "Make me a guide", &[])
            .may_go_deep(Reach::deep_paged(Some(80_000), native::FETCH_TOKENS as usize));
        let tools = turn.server_tools().unwrap();
        assert_eq!((tools[0]["max_uses"].clone(), tools[1]["max_uses"].clone()), (json!(native::SERVER_USES), json!(native::SERVER_USES)));
        turn.go_deep();
        let tools = turn.server_tools().unwrap();
        assert_eq!(tools[0]["name"], "web_search");
        assert_eq!(tools[0]["max_uses"], json!(native::DEEP_SERVER_USES));
        assert_eq!(tools[1]["name"], "web_fetch");
        assert_eq!(tools[1]["max_uses"], json!(4));
    }

    /// **A new document's outline first** (2026-10-31): Claude's own web
    /// tools stay out of the turn's first request; once that step is done,
    /// they're back.
    #[test]
    fn a_new_documents_outline_comes_before_any_search() {
        let mut n = native::NativeSearch::new(native::Provider::Anthropic, "claude-opus-5-5", "https://api.anthropic.com/v1", "k");
        n.in_chat = true;
        let mut turn = Turn::new(Settings { native: Some(n), ..settings("") }, offline(), "Make me a Tidecaller guide", &[])
            .may_go_deep(Reach::deep(None))
            .outline_first();
        turn.go_deep();
        assert!(turn.outline_pending());
        // Claude searches in Chat's request (Yap's tools aren't offered)…
        assert!(turn.server_tools().is_some());
        // …but not before the outline.
        assert_eq!(turn.server_tools(), Some(Vec::new()));
        turn.outline_settled();
        assert!(!turn.outline_pending());
        assert_eq!(turn.server_tools().unwrap()[0]["max_uses"], json!(native::DEEP_SERVER_USES));
        // Every other turn: nothing held back.
        let plain = Turn::new(settings("exa-key"), offline(), "Where is Gale?", &[]);
        assert!(!plain.outline_pending());
    }

    /// **The Sources row's facts** (2026-10-31): a page read is marked
    /// opened and takes the page's own title; what a document cites (by
    /// number, or a link to the page) is marked cited, and a document's
    /// answer keeps every source, so its row can show those first.
    #[test]
    fn a_documents_sources_mark_the_pages_read_and_cited() {
        let mut turn = Turn::new(settings("exa-key"), offline(), "Make me a Tidecaller guide", &[]);
        let generic = "Emberfall";
        assert_eq!(turn.number(generic, "https://forum.example/t/101"), 1);
        assert_eq!(turn.number(generic, "https://forum.example/t/102"), 2);
        assert_eq!(turn.number("Tidecaller - Emberfall Wiki", "https://emberfall.example/wiki/Tidecaller"), 3);
        assert_eq!(turn.number(generic, "https://forum.example/t/103"), 4);
        // Read: the page's own title, marked opened (the same page, by address).
        assert_eq!(turn.opened("Solo Tidecaller, levels 1 to 4", "https://www.forum.example/t/102/"), 2);
        assert_eq!(turn.sources[1].title, "Solo Tidecaller, levels 1 to 4");
        assert!(turn.sources[1].opened && !turn.sources[0].opened);
        // A read with no title keeps the one it had.
        turn.opened("", "https://emberfall.example/wiki/Tidecaller");
        assert_eq!(turn.sources[2].title, "Tidecaller - Emberfall Wiki");
        // An ordinary answer: the sources it cites, as before.
        assert_eq!(turn.sources_for("Rain Bolt first [3].").iter().map(|s| s.n).collect::<Vec<_>>(), [3]);
        // A document cites [2] and links the wiki page itself (Claude's way).
        turn.cited_in("| 1 | Rain Bolt [2] |\n\nSee the [wiki](https://emberfall.example/wiki/Tidecaller).");
        let kept = turn.sources_for("It's open on the right.");
        assert_eq!(kept.len(), 4, "every source, for the row's +N more");
        let flags: Vec<(usize, bool, bool)> = kept.iter().map(|s| (s.n, s.cited, s.opened)).collect();
        assert_eq!(flags, [(1, false, false), (2, true, true), (3, true, true), (4, false, false)]);
        // The answer's own citation counts as cited too.
        assert!(turn.sources_for("It's open on the right [4].").iter().any(|s| s.n == 4 && s.cited));
    }

    #[test]
    fn one_page_is_one_address_however_its_written() {
        let same = [
            "https://steamcommunity.example/sharedfiles/filedetails/?id=123",
            "https://www.steamcommunity.example/sharedfiles/filedetails?id=123",
            "https://steamcommunity.example/sharedfiles/filedetails?id=123&utm_source=openai",
            "https://steamcommunity.example/sharedfiles/filedetails?id=123#comments",
        ];
        for u in same {
            assert_eq!(normal_url(u), normal_url(same[0]), "{u}");
        }
        assert_ne!(normal_url("https://steamcommunity.example/sharedfiles/filedetails?id=124"), normal_url(same[0]));
        assert_eq!(normal_url("https://bg3.wiki/wiki/Gale/"), "https://bg3.wiki/wiki/Gale");
    }

    #[test]
    fn claudes_own_searches_become_the_turns_sources() {
        // Yap's tools unless Claude searches in Chat's request.
        let opus = || native::NativeSearch::new(native::Provider::Anthropic, "claude-opus-5-5", "https://api.anthropic.com/v1", "k");
        let turn = Turn::new(Settings { native: Some(opus()), ..settings("") }, offline(), "q", &[]);
        assert!(turn.server_tools().is_none());
        let mut n = opus();
        n.in_chat = true;
        let mut turn = Turn::new(Settings { native: Some(n), ..settings("") }, offline(), "Make me a Tidecaller guide", &[]).may_go_deep(Reach::deep(None));
        assert_eq!(turn.server_tools().unwrap()[0]["max_uses"], json!(native::SERVER_USES));
        turn.go_deep();
        assert_eq!(turn.server_tools().unwrap()[0]["max_uses"], json!(native::DEEP_SERVER_USES));
        assert!(!turn.seen() && !turn.looked());
        let w = json!({
            "searches": [
                { "query": "Emberfall tidecaller", "results": [{ "url": "https://emberfall.example/wiki/Tidecaller", "title": "Tidecaller" }, { "url": "http://192.168.1.1/x", "title": "local" }] },
                { "query": "tidecaller gear", "results": [], "error": "it's limiting how much Yap can search right now" }
            ],
            "fetches": [
                { "url": "https://emberfall.example/wiki/Gear_locations", "title": "Gear locations" },
                { "url": "https://forum.example/t/x", "title": "", "error": "the page didn't open" }
            ],
            "cites": [{ "url": "https://emberfall.example/wiki/Gear_locations", "title": "Gear locations", "text": "Coral Staff: Saltmarsh Docks" }]
        });
        let (chips, numbers) = turn.took_server(&w);
        assert_eq!(
            chips,
            [
                "Searched the web with Anthropic: \"Emberfall tidecaller\"",
                "Couldn't search the web with Anthropic: it's limiting how much Yap can search right now",
                "Read emberfall.example/wiki/Gear_locations",
                "Couldn't read forum.example/t/x: the page didn't open",
            ]
        );
        // The page it read and cites: one number; a local address never a source.
        assert_eq!(numbers, [2]);
        assert_eq!(turn.sources.iter().map(|s| s.url.as_str()).collect::<Vec<_>>(), ["https://emberfall.example/wiki/Tidecaller", "https://emberfall.example/wiki/Gear_locations"]);
        assert!(turn.seen() && turn.looked());
        assert_eq!(turn.texts, ["Coral Staff: Saltmarsh Docks"]);
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

//! The keyless lookup, for a search with no Exa or Tavily key (or when
//! the service fails): a game's own MediaWiki wiki, else Wikipedia in the
//! person's language — politely: one request at a time per host, a short
//! gap, answers cached.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Instant;

use regex::Regex;
use reqwest::Url;
use serde_json::{json, Value};

use crate::privacy::net::Recorded as _;

use super::{network_error, site_of, tidy, Endpoints, Found, Hit, Settings, API, HIT_CHARS, PAGE_BYTES, POLITE_GAP};

/// A game and its wiki (a MediaWiki `api.php`). Easy to extend: the names a
/// question may use (case-blind, whole words), names matched only as
/// written ("WoW"), and the wiki's API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameWiki {
    pub game: &'static str,
    pub names: &'static [&'static str],
    pub cased: &'static [&'static str],
    pub api: &'static str,
}

/// Games Yap knows a wiki for.
pub const GAME_WIKIS: &[GameWiki] = &[
    GameWiki { game: "Baldur's Gate 3", names: &["baldur's gate 3", "baldurs gate 3", "baldur's gate iii", "baldur’s gate 3", "bg3"], cased: &[], api: "https://bg3.wiki/w/api.php" },
    GameWiki { game: "World of Warcraft", names: &["world of warcraft", "warcraft"], cased: &["WoW"], api: "https://warcraft.wiki.gg/api.php" },
    GameWiki { game: "Elden Ring", names: &["elden ring", "shadow of the erdtree"], cased: &[], api: "https://eldenring.fandom.com/api.php" },
    GameWiki { game: "Minecraft", names: &["minecraft"], cased: &[], api: "https://minecraft.wiki/api.php" },
    GameWiki { game: "Terraria", names: &["terraria"], cased: &[], api: "https://terraria.wiki.gg/api.php" },
    GameWiki { game: "Stardew Valley", names: &["stardew valley", "stardew"], cased: &[], api: "https://stardewvalleywiki.com/mediawiki/api.php" },
    GameWiki { game: "The Legend of Zelda", names: &["zelda", "tears of the kingdom", "breath of the wild", "totk", "botw"], cased: &[], api: "https://zeldawiki.wiki/w/api.php" },
    GameWiki { game: "The Elder Scrolls", names: &["skyrim", "oblivion", "morrowind", "elder scrolls"], cased: &[], api: "https://en.uesp.net/w/api.php" },
    GameWiki { game: "Old School RuneScape", names: &["old school runescape", "osrs"], cased: &[], api: "https://oldschool.runescape.wiki/api.php" },
    GameWiki { game: "RuneScape", names: &["runescape"], cased: &[], api: "https://runescape.wiki/api.php" },
    GameWiki { game: "Satisfactory", names: &["satisfactory game"], cased: &["Satisfactory"], api: "https://satisfactory.wiki.gg/api.php" },
    GameWiki { game: "Deep Rock Galactic", names: &["deep rock galactic"], cased: &[], api: "https://deeprockgalactic.wiki.gg/api.php" },
    GameWiki { game: "Cyberpunk 2077", names: &["cyberpunk 2077", "cyberpunk"], cased: &[], api: "https://cyberpunk.fandom.com/api.php" },
    GameWiki { game: "The Witcher", names: &["witcher"], cased: &[], api: "https://witcher.fandom.com/api.php" },
    GameWiki { game: "Fallout", names: &["fallout 4", "fallout 76", "fallout new vegas", "fallout"], cased: &[], api: "https://fallout.fandom.com/api.php" },
    GameWiki { game: "Genshin Impact", names: &["genshin impact", "genshin"], cased: &[], api: "https://genshin-impact.fandom.com/api.php" },
    GameWiki { game: "Valheim", names: &["valheim"], cased: &[], api: "https://valheim.fandom.com/api.php" },
];

/// `phrase` in `text` as whole words (both lowercased, or as written).
pub(super) fn has_phrase(text: &str, phrase: &str) -> bool {
    let mut from = 0;
    while let Some(i) = text[from..].find(phrase) {
        let start = from + i;
        let end = start + phrase.len();
        let before = text[..start].chars().next_back();
        let after = text[end..].chars().next();
        let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric());
        if !word(before) && !word(after) {
            return true;
        }
        from = end;
    }
    false
}

/// The game a question names, if Yap knows its wiki. Pure.
pub fn game_for(text: &str) -> Option<&'static GameWiki> {
    let lower = text.to_lowercase().replace('’', "'");
    GAME_WIKIS
        .iter()
        .find(|g| g.names.iter().any(|n| has_phrase(&lower, &n.replace('’', "'"))) || g.cased.iter().any(|n| has_phrase(text, n)))
}

/// A MediaWiki's `api.php` for a wiki host: one of [`GAME_WIKIS`], any
/// `*.wiki.gg` or `*.fandom.com` wiki, or Wikipedia. Pure.
pub fn api_for_host(host: &str) -> Option<String> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if let Some(g) = GAME_WIKIS.iter().find(|g| site_of(g.api) == host) {
        return Some(g.api.to_string());
    }
    let label_ok = |h: &str, suffix: &str| {
        h.strip_suffix(suffix).is_some_and(|name| !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
    };
    if label_ok(&host, ".wiki.gg") || label_ok(&host, ".fandom.com") {
        return Some(format!("https://{host}/api.php"));
    }
    if label_ok(&host, ".wikipedia.org") {
        return Some(format!("https://{host}/w/api.php"));
    }
    None
}

/// A wiki host the question names ("search terraria.wiki.gg for…", or a
/// link to one of its pages).
fn wiki_named_in(text: &str) -> Option<String> {
    static HOST: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\b([a-z0-9-]+\.(?:wiki\.gg|fandom\.com))\b").expect("static regex"));
    HOST.captures(text).and_then(|c| api_for_host(&c[1]))
}

/// Words a wiki's search shouldn't get (small wikis AND every word).
const WIKI_STOP: &[&str] = &[
    "a", "about", "an", "and", "any", "are", "at", "be", "best", "can", "could", "did", "do", "does", "find",
    "for", "from", "game", "get", "go", "guide", "has", "have", "how", "i", "i'm", "in", "info", "information",
    "is", "it", "its", "know", "located", "location", "look", "me", "my", "of", "on", "online", "or", "please",
    "search", "should", "tell", "that", "the", "there", "this", "to", "up", "walkthrough", "want", "was",
    "web", "what", "what's", "where", "where's", "which", "who", "who's", "why", "wiki", "wikipedia", "will",
    "with", "would", "you", "your",
];

/// What to ask a wiki's search for: the question without the game's names,
/// question words and filler, ≤ 6 words, as written ("Where do I find Gale
/// in Baldur's Gate 3?" → "Gale"). Pure.
pub fn wiki_terms(query: &str, game: Option<&GameWiki>) -> String {
    let mut text = query.replace('’', "'");
    if let Some(g) = game {
        for name in g.names.iter().chain(g.cased) {
            let re = Regex::new(&format!(r"(?i)\b{}\b", regex::escape(&name.replace('’', "'")))).expect("escaped regex");
            text = re.replace_all(&text, " ").into_owned();
        }
    }
    let words: Vec<String> = text
        .split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '-'))
        .map(|w| w.trim_matches(['\'', '-']))
        .filter(|w| !w.is_empty() && !WIKI_STOP.contains(&w.to_lowercase().as_str()))
        .map(str::to_string)
        .collect();
    words.into_iter().take(6).collect::<Vec<_>>().join(" ")
}

/// The article named exactly as the search, or as one of its words — the
/// subject's own page — first: a wiki's search ranks pages that mention it
/// a lot ahead of it (bg3.wiki for "Gale act 1": Gale/Banter, Act One/Camp
/// Events, then Gale, whose text Yap then didn't read: only the top two are
/// read). Pure.
pub fn subject_first(pages: &mut Vec<WikiPage>, terms: &str) {
    let whole = terms.trim().to_lowercase();
    let words: Vec<String> = terms.split_whitespace().filter(|w| w.chars().count() >= 3).map(str::to_lowercase).collect();
    let title = |p: &WikiPage| p.title.trim().to_lowercase();
    let named = pages
        .iter()
        .position(|p| title(p) == whole)
        .or_else(|| pages.iter().position(|p| title(p).chars().count() >= 3 && words.contains(&title(p))));
    if let Some(i) = named.filter(|&i| i > 0) {
        let page = pages.remove(i);
        pages.insert(0, page);
    }
}

/// Fewer words to try when a wiki found nothing: the capitalised ones (a
/// name), else the longest. Pure.
pub fn wiki_retry(terms: &str) -> Option<String> {
    let words: Vec<&str> = terms.split_whitespace().collect();
    if words.len() < 2 {
        return None;
    }
    let caps: Vec<&str> = words.iter().copied().filter(|w| w.chars().next().is_some_and(char::is_uppercase)).collect();
    if !caps.is_empty() && caps.len() < words.len() {
        return Some(caps.join(" "));
    }
    words.iter().max_by_key(|w| w.chars().count()).map(|w| w.to_string())
}

fn api_url(api: &str, params: &[(&str, &str)]) -> Result<Url, String> {
    let mut url = Url::parse(api).map_err(|_| format!("bad wiki address {api}"))?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("format", "json");
        q.append_pair("formatversion", "2");
        for (k, v) in params {
            q.append_pair(k, v);
        }
    }
    Ok(url)
}

/// A wiki search: the top 3 articles with their addresses (`generator=search`
/// + `prop=info&inprop=url`).
pub fn wiki_search_url(api: &str, terms: &str) -> Result<Url, String> {
    api_url(
        api,
        &[
            ("action", "query"),
            ("generator", "search"),
            ("gsrsearch", terms),
            ("gsrlimit", "3"),
            ("gsrnamespace", "0"),
            ("prop", "info"),
            ("inprop", "url"),
            ("redirects", "1"),
        ],
    )
}

/// A found article.
#[derive(Debug, Clone, PartialEq)]
pub struct WikiPage {
    pub pageid: u64,
    pub title: String,
    pub url: String,
}

/// A wiki search's articles, best first. Pure.
pub fn parse_wiki_search(v: &Value) -> Result<Vec<WikiPage>, String> {
    if let Some(info) = v["error"]["info"].as_str() {
        return Err(format!("The wiki said: {}", tidy(info, 120)));
    }
    let mut pages: Vec<(u64, WikiPage)> = v["query"]["pages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["missing"] != json!(true))
        .filter_map(|p| {
            Some((
                p["index"].as_u64().unwrap_or(u64::MAX),
                WikiPage { pageid: p["pageid"].as_u64()?, title: p["title"].as_str()?.to_string(), url: p["fullurl"].as_str()?.to_string() },
            ))
        })
        .collect();
    pages.sort_by_key(|(i, _)| *i);
    Ok(pages.into_iter().map(|(_, p)| p).collect())
}

/// Wikipedia's plain text of one article (TextExtracts).
pub fn wiki_extract_url(api: &str, pageid: u64) -> Result<Url, String> {
    api_url(
        api,
        &[("action", "query"), ("prop", "extracts"), ("explaintext", "1"), ("exsectionformat", "plain"), ("pageids", &pageid.to_string())],
    )
}

/// Any MediaWiki's HTML of one article (`action=parse`): game wikis often
/// don't have TextExtracts.
pub fn wiki_parse_url(api: &str, pageid: u64) -> Result<Url, String> {
    api_url(
        api,
        &[
            ("action", "parse"),
            ("pageid", &pageid.to_string()),
            ("prop", "text"),
            ("disableeditsection", "1"),
            ("disabletoc", "1"),
            ("disablelimitreport", "1"),
        ],
    )
}

/// An article's text from either answer. Pure.
pub fn parse_wiki_text(v: &Value) -> Option<String> {
    if let Some(extract) = v["query"]["pages"][0]["extract"].as_str() {
        return Some(extract.to_string());
    }
    let html = v["parse"]["text"].as_str().or(v["parse"]["text"]["*"].as_str())?;
    Some(fragment_text(html))
}

/// A wiki article's HTML as text, block by block (navigation boxes,
/// footnote markers and edit links left out). Pure.
pub fn fragment_text(html: &str) -> String {
    let doc = dom_query::Document::from(format!("<html><body>{html}</body></html>"));
    doc.select(
        "script, style, noscript, template, sup.reference, .reference, .mw-editsection, .navbox, .toc, #toc, \
         .mw-references-wrap, .references, .printfooter, .noprint, .mw-empty-elt, figure, .thumb, .gallery, \
         aside, [class*=\"infobox\"], [class*=\"navbox\"], .metadata, .mw-collapsible.mw-collapsed",
    )
    .remove();
    let text = doc.select("body").formatted_text().to_string();
    tidy_lines(&text)
}

/// Lines trimmed, runs of blank lines as one.
pub(super) fn tidy_lines(text: &str) -> String {
    let mut out = String::new();
    let mut blank = false;
    for line in text.lines() {
        let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if line.is_empty() {
            blank = !out.is_empty();
            continue;
        }
        if !out.is_empty() {
            out.push_str(if blank { "\n\n" } else { "\n" });
        }
        blank = false;
        out.push_str(&line);
    }
    out
}

/// Words that say nothing about what's asked.
pub(super) const QUERY_STOP: &[&str] = &[
    "about", "and", "are", "can", "could", "did", "does", "for", "from", "get", "have", "how", "into", "its",
    "look", "please", "search", "tell", "that", "the", "their", "there", "this", "was", "what", "when", "where",
    "which", "who", "why", "will", "with", "would", "you", "your", "find", "web", "online", "up",
];

/// The words a passage is scored on. Pure.
pub fn keywords(query: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in query.to_lowercase().replace('’', "'").split(|c: char| !(c.is_alphanumeric() || c == '\'')) {
        let w = w.trim_matches('\'').trim_end_matches("'s");
        if w.chars().count() >= 3 && !QUERY_STOP.contains(&w) && !out.iter().any(|o| o == w) {
            out.push(w.to_string());
        }
    }
    out
}

/// The paragraphs of `text` that match the most `words`, in their order,
/// ≤ `max_chars` in all; with no match, the start. A heading (a short line
/// without a full stop) stays with the paragraph under it. Pure.
pub fn best_passages(text: &str, words: &[String], max_chars: usize) -> String {
    // Chunks: a heading joins the next line; short lines (list items) run
    // together up to ~500 chars.
    let mut chunks: Vec<String> = Vec::new();
    let mut heading: Option<String> = None;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let looks_heading = line.chars().count() < 60 && !line.ends_with(['.', '!', '?', ':', ';', ',']);
        if looks_heading && heading.is_none() {
            heading = Some(line.to_string());
            continue;
        }
        let line = match heading.take() {
            Some(h) => format!("{h}: {line}"),
            None => line.to_string(),
        };
        match chunks.last_mut() {
            Some(last) if last.chars().count() < 120 && line.chars().count() < 120 && last.chars().count() + line.chars().count() < 500 => {
                last.push_str(" · ");
                last.push_str(&line);
            }
            _ => chunks.push(line),
        }
    }
    if let Some(h) = heading {
        chunks.push(h);
    }
    let score = |c: &str| {
        let lower = c.to_lowercase();
        let s = words.iter().map(|w| lower.matches(w.as_str()).count().min(3) + usize::from(lower.contains(w.as_str())) * 3).sum::<usize>();
        // Short lines run together with no sentence among them (a table of
        // data, an infobox): prose answers better.
        if c.contains(" · ") && c.matches(". ").count() < 2 {
            s / 2
        } else {
            s
        }
    };
    let mut ranked: Vec<(usize, usize)> = chunks.iter().enumerate().map(|(i, c)| (score(c), i)).filter(|(s, _)| *s > 0).collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut picked: Vec<usize> = Vec::new();
    let mut used = 0;
    let order: Vec<usize> = if ranked.is_empty() { (0..chunks.len()).collect() } else { ranked.iter().map(|(_, i)| *i).collect() };
    for i in order {
        let len = chunks[i].chars().count().min(HIT_CHARS);
        if used > 0 && used + len > max_chars {
            continue;
        }
        picked.push(i);
        used += len + 3;
        if used >= max_chars {
            break;
        }
    }
    picked.sort_unstable();
    let joined = picked.iter().map(|&i| tidy(&chunks[i], HIT_CHARS)).collect::<Vec<_>>().join(" … ");
    tidy(&joined, max_chars)
}

/// One request at a time per host, `POLITE_GAP` apart.
static LAST_ASKED: LazyLock<Mutex<HashMap<String, Instant>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

async fn polite(host: &str) {
    let wait = {
        let mut last = LAST_ASKED.lock().unwrap_or_else(|p| p.into_inner());
        let now = Instant::now();
        let next = last.get(host).map_or(now, |t| (*t + POLITE_GAP).max(now));
        last.insert(host.to_string(), next);
        next.saturating_duration_since(now)
    };
    if !wait.is_zero() {
        tokio::time::sleep(wait).await;
    }
}

async fn wiki_get(ep: &Endpoints, url: &Url) -> Result<Value, String> {
    let host = url.host_str().unwrap_or("").to_string();
    let target = ep.route(url)?;
    polite(&host).await;
    let resp = API.get(target).send_for(crate::privacy::net::Purpose::WikiSearch).await.map_err(|e| network_error(&host, &e))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("{host} answered HTTP {status}."));
    }
    let bytes = resp.bytes().await.map_err(|e| network_error(&host, &e))?;
    if bytes.len() > PAGE_BYTES * 2 {
        return Err(format!("{host} sent too much."));
    }
    serde_json::from_slice(&bytes).map_err(|_| format!("{host} sent an answer Yap couldn't read."))
}

/// A wiki's best articles for `query`, as hits: the top article's best
/// paragraphs, then the next one's. ≤ 3 searches + 2 article reads.
pub(super) async fn wiki_lookup(ep: &Endpoints, api: &str, query: &str, game: Option<&GameWiki>) -> Result<Vec<Hit>, String> {
    let wikipedia = site_of(api).ends_with("wikipedia.org");
    let mut terms = wiki_terms(query, game);
    if terms.is_empty() {
        terms = game.map_or_else(|| query.to_string(), |g| g.game.to_string());
    }
    let mut pages = parse_wiki_search(&wiki_get(ep, &wiki_search_url(api, &terms)?).await?)?;
    let mut tries = 1;
    while pages.is_empty() && tries < 3 {
        let Some(fewer) = wiki_retry(&terms) else { break };
        terms = fewer;
        pages = parse_wiki_search(&wiki_get(ep, &wiki_search_url(api, &terms)?).await?)?;
        tries += 1;
    }
    subject_first(&mut pages, &terms);
    let words = keywords(&format!("{query} {terms}"));
    let mut hits = Vec::new();
    for (i, page) in pages.iter().take(3).enumerate() {
        // The top two articles are read; the third is named only.
        let text = if i < 2 {
            let url = if wikipedia { wiki_extract_url(api, page.pageid)? } else { wiki_parse_url(api, page.pageid)? };
            match wiki_get(ep, &url).await {
                Ok(v) => parse_wiki_text(&v).map(|t| best_passages(&t, &words, if i == 0 { 1_200 } else { HIT_CHARS })).unwrap_or_default(),
                Err(e) => {
                    tracing::info!("Web search: couldn't read a wiki article ({e})");
                    String::new()
                }
            }
        } else {
            String::new()
        };
        if text.is_empty() && i < 2 {
            continue;
        }
        hits.push(Hit { title: page.title.clone(), url: page.url.clone(), published: None, text });
    }
    Ok(hits)
}

/// The keyless lookup: the game's wiki (or a wiki the question names), else
/// Wikipedia in `lang`, else English Wikipedia.
pub(super) async fn keyless(s: &Settings, ep: &Endpoints, query: &str, context: &str) -> Result<Found, String> {
    let game = game_for(query).or_else(|| game_for(context));
    let named = wiki_named_in(query).or_else(|| wiki_named_in(context));
    let mut errors = Vec::new();
    if let Some(api) = named.or_else(|| game.map(|g| g.api.to_string())) {
        match wiki_lookup(ep, &api, query, game).await {
            Ok(hits) if !hits.is_empty() => return Ok(Found { hits, from: site_of(&api), ..Default::default() }),
            Ok(_) => {}
            Err(e) => errors.push(e),
        }
    }
    let mut langs = vec![s.lang.clone()];
    if s.lang != "en" {
        langs.push("en".into());
    }
    for lang in langs {
        let api = format!("https://{lang}.wikipedia.org/w/api.php");
        match wiki_lookup(ep, &api, query, game).await {
            Ok(hits) if !hits.is_empty() => return Ok(Found { hits, from: "Wikipedia".into(), ..Default::default() }),
            Ok(_) => {}
            Err(e) => errors.push(e),
        }
    }
    match errors.into_iter().next() {
        Some(e) => Err(e),
        None => Ok(Found { from: "Wikipedia".into(), ..Default::default() }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::wiki_language;

    #[test]
    fn games_are_found_by_name_and_wikis_by_host() {
        assert_eq!(game_for("Where do I find Gale in Baldur's Gate 3?").unwrap().game, "Baldur's Gate 3");
        assert_eq!(game_for("where's gale in BG3").unwrap().game, "Baldur's Gate 3");
        assert_eq!(game_for("Baldur’s Gate 3 Gale").unwrap().game, "Baldur's Gate 3");
        assert_eq!(game_for("best WoW class for a beginner").unwrap().game, "World of Warcraft");
        // "wow" in a sentence isn't the game; nor a word containing a name.
        assert!(game_for("wow, what a day").is_none());
        assert!(game_for("is this satisfactory to you").is_none());
        assert!(game_for("the minecraftian style").is_none());
        assert_eq!(api_for_host("bg3.wiki").as_deref(), Some("https://bg3.wiki/w/api.php"));
        assert_eq!(api_for_host("terraria.wiki.gg").as_deref(), Some("https://terraria.wiki.gg/api.php"));
        assert_eq!(api_for_host("hollowknight.wiki.gg").as_deref(), Some("https://hollowknight.wiki.gg/api.php"));
        assert_eq!(api_for_host("starwars.fandom.com").as_deref(), Some("https://starwars.fandom.com/api.php"));
        assert_eq!(api_for_host("de.wikipedia.org").as_deref(), Some("https://de.wikipedia.org/w/api.php"));
        assert_eq!(api_for_host("evil.com"), None);
        assert_eq!(api_for_host("a.b.fandom.com"), None);
        assert_eq!(wiki_named_in("search hollowknight.wiki.gg for the mantis lords").as_deref(), Some("https://hollowknight.wiki.gg/api.php"));
    }

    #[test]
    fn a_wiki_is_asked_for_the_words_that_matter() {
        let bg3 = game_for("bg3");
        assert_eq!(wiki_terms("Where do I find Gale in Baldur's Gate 3?", bg3), "Gale");
        assert_eq!(wiki_terms("how do I get the Blood of Lathander in BG3", bg3), "Blood Lathander");
        assert_eq!(wiki_terms("What is the tallest mountain in Scotland?", None), "tallest mountain Scotland");
        assert_eq!(wiki_retry("Blood Lathander").as_deref(), Some("Lathander"));
        assert_eq!(wiki_retry("tallest mountain Scotland").as_deref(), Some("Scotland"));
        assert_eq!(wiki_retry("Gale"), None);
        assert_eq!(wiki_language("auto", Some("en-GB")), "en");
        assert_eq!(wiki_language("fr", Some("en-GB")), "fr");
        assert_eq!(wiki_language("auto", Some("nb-NO")), "no");
        assert_eq!(wiki_language("", None), "en");
        assert_eq!(wiki_language("auto", Some("x")), "en");
    }

    #[test]
    fn the_subjects_own_article_is_read_first() {
        let page = |title: &str| WikiPage { pageid: 1, title: title.into(), url: format!("https://bg3.wiki/wiki/{title}") };
        let titles = |p: &[WikiPage]| p.iter().map(|x| x.title.clone()).collect::<Vec<_>>();
        // bg3.wiki's order for "Gale act 1" (the model evaluations): Gale third.
        let mut pages = vec![page("Gale/Banter"), page("Act One/Camp Events"), page("Gale")];
        subject_first(&mut pages, "Gale act 1");
        assert_eq!(titles(&pages), ["Gale", "Gale/Banter", "Act One/Camp Events"]);
        // The whole search as a title; nothing named: the wiki's order.
        let mut pages = vec![page("Lathander"), page("Blood of Lathander")];
        subject_first(&mut pages, "Blood of Lathander");
        assert_eq!(titles(&pages), ["Blood of Lathander", "Lathander"]);
        let mut pages = vec![page("Tara"), page("The Wizard of Waterdeep")];
        subject_first(&mut pages, "first encounter location");
        assert_eq!(titles(&pages), ["Tara", "The Wizard of Waterdeep"]);
        // A short word ("act", "1") never picks a page.
        let mut pages = vec![page("Camp"), page("1")];
        subject_first(&mut pages, "Gale 1");
        assert_eq!(titles(&pages), ["Camp", "1"]);
    }

    // Recorded from bg3.wiki's api.php (generator=search + info, then parse).
    const WIKI_SEARCH: &str = r#"{"batchcomplete":true,"query":{"pages":[
      {"pageid":702,"ns":0,"title":"Gale's Tower","index":2,"fullurl":"https://bg3.wiki/wiki/Gale%27s_Tower"},
      {"pageid":118,"ns":0,"title":"Gale","index":1,"fullurl":"https://bg3.wiki/wiki/Gale"}]}}"#;
    const WIKI_PARSE: &str = r#"{"parse":{"title":"Gale","pageid":118,"text":"<div class=\"mw-parser-output\"><table class=\"navbox\"><tr><td>Companions: Astarion · Gale</td></tr></table><p>Gale of Waterdeep is a human wizard and an origin character.<sup class=\"reference\">[1]</sup></p><h2>Location<span class=\"mw-editsection\">[edit]</span></h2><p>Gale is found on the <b>beach</b> west of the Nautiloid crash site, stuck in a magical portal. Pull him out to recruit him.</p><h2>Relationships</h2><p>Gale can be romanced.</p></div>"}}"#;

    #[test]
    fn a_wiki_answer_becomes_the_passages_that_answer() {
        let pages = parse_wiki_search(&serde_json::from_str(WIKI_SEARCH).unwrap()).unwrap();
        assert_eq!(pages.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(), ["Gale", "Gale's Tower"]);
        assert_eq!(pages[0].url, "https://bg3.wiki/wiki/Gale");
        let text = parse_wiki_text(&serde_json::from_str(WIKI_PARSE).unwrap()).unwrap();
        assert!(!text.contains("[edit]") && !text.contains("[1]") && !text.contains("Companions"), "{text}");
        assert!(text.contains("\n\nLocation\n\nGale is found on the beach west"), "{text}");
        let best = best_passages(&text, &keywords("where do I find Gale location"), 300);
        assert!(best.contains("Location: Gale is found on the beach west of the Nautiloid crash site"), "{best}");
        // Wikipedia's extracts are plain text already.
        let v = json!({ "query": { "pages": [{ "pageid": 1, "title": "Ben Nevis", "extract": "Ben Nevis is the highest mountain in Scotland." }] } });
        assert_eq!(parse_wiki_text(&v).as_deref(), Some("Ben Nevis is the highest mountain in Scotland."));
        assert!(parse_wiki_search(&json!({ "error": { "code": "x", "info": "Search is disabled" } })).is_err());
        // The requests: JSON v2, the search's words, the page id.
        let url = wiki_search_url("https://bg3.wiki/w/api.php", "Gale").unwrap();
        assert!(url.as_str().starts_with("https://bg3.wiki/w/api.php?format=json&formatversion=2&action=query&generator=search&gsrsearch=Gale&gsrlimit=3"));
        assert!(wiki_parse_url("https://bg3.wiki/w/api.php", 118).unwrap().as_str().contains("action=parse&pageid=118&prop=text"));
    }

    #[test]
    fn with_no_match_the_start_of_the_page_is_given() {
        let text = "Intro line one is here.\nSecond paragraph about other things.";
        assert!(best_passages(text, &keywords("zebra"), 25).starts_with("Intro line one is here."));
    }
}

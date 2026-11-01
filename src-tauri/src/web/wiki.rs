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

use super::{network_error, site_of, tidy, Endpoints, Found, FoundImage, Hit, PageImage, Settings, API, HIT_CHARS, PAGE_BYTES, POLITE_GAP};

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

/// Games Yap knows a wiki for: wikis that allow its automated requests
/// ([`allows_automated`]; no Fandom wiki since 1 Nov 2026 — Elden Ring,
/// Cyberpunk 2077, The Witcher, Fallout, Genshin Impact and Valheim went:
/// they fall back as any game without a wiki does). A new one only once its
/// own terms are checked.
pub const GAME_WIKIS: &[GameWiki] = &[
    GameWiki { game: "Baldur's Gate 3", names: &["baldur's gate 3", "baldurs gate 3", "baldur's gate iii", "baldur’s gate 3", "bg3"], cased: &[], api: "https://bg3.wiki/w/api.php" },
    GameWiki { game: "World of Warcraft", names: &["world of warcraft", "warcraft"], cased: &["WoW"], api: "https://warcraft.wiki.gg/api.php" },
    GameWiki { game: "Minecraft", names: &["minecraft"], cased: &[], api: "https://minecraft.wiki/api.php" },
    GameWiki { game: "Terraria", names: &["terraria"], cased: &[], api: "https://terraria.wiki.gg/api.php" },
    GameWiki { game: "Stardew Valley", names: &["stardew valley", "stardew"], cased: &[], api: "https://stardewvalleywiki.com/mediawiki/api.php" },
    GameWiki { game: "The Legend of Zelda", names: &["zelda", "tears of the kingdom", "breath of the wild", "totk", "botw"], cased: &[], api: "https://zeldawiki.wiki/w/api.php" },
    GameWiki { game: "The Elder Scrolls", names: &["skyrim", "oblivion", "morrowind", "elder scrolls"], cased: &[], api: "https://en.uesp.net/w/api.php" },
    GameWiki { game: "Old School RuneScape", names: &["old school runescape", "osrs"], cased: &[], api: "https://oldschool.runescape.wiki/api.php" },
    GameWiki { game: "RuneScape", names: &["runescape"], cased: &[], api: "https://runescape.wiki/api.php" },
    GameWiki { game: "Satisfactory", names: &["satisfactory game"], cased: &["Satisfactory"], api: "https://satisfactory.wiki.gg/api.php" },
    GameWiki { game: "Deep Rock Galactic", names: &["deep rock galactic"], cased: &[], api: "https://deeprockgalactic.wiki.gg/api.php" },
];

/// **Whether a wiki's host allows Yap's automated requests** — the one rule
/// for the keyless lookup's text ([`wiki_lookup`], [`api_for_host`]) and an
/// article's pictures ([`wiki_images_url`]). Never Fandom (`*.fandom.com`):
/// its Terms of Use say "not use any robot, spider, scraper or other
/// automated means to access the Services for any purpose without our
/// express written permission", and it answers scripted fetches HTTP 402
/// (checked 1 Nov 2026). Wikipedia and Wikimedia Commons (Wikimedia's API
/// policy allows it with a descriptive User-Agent: [`super::USER_AGENT`]),
/// `*.wiki.gg` and the independent wikis of [`GAME_WIKIS`] do. A page the
/// model reads from search results (`read_page`) isn't this lookup. Pure.
pub fn allows_automated(host: &str) -> bool {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    !(host == "fandom.com" || host.ends_with(".fandom.com"))
}

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
/// `*.wiki.gg` wiki, or Wikipedia — never a host that forbids automated
/// requests ([`allows_automated`]: Fandom). Pure.
pub fn api_for_host(host: &str) -> Option<String> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if !allows_automated(&host) {
        return None;
    }
    if let Some(g) = GAME_WIKIS.iter().find(|g| site_of(g.api) == host) {
        return Some(g.api.to_string());
    }
    let label_ok = |h: &str, suffix: &str| {
        h.strip_suffix(suffix).is_some_and(|name| !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
    };
    if label_ok(&host, ".wiki.gg") {
        return Some(format!("https://{host}/api.php"));
    }
    if label_ok(&host, ".wikipedia.org") {
        return Some(format!("https://{host}/w/api.php"));
    }
    None
}

/// A wiki host the question names ("search terraria.wiki.gg for…", or a
/// link to one of its pages): a `*.wiki.gg` wiki (never a Fandom one:
/// [`allows_automated`]).
fn wiki_named_in(text: &str) -> Option<String> {
    static HOST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b([a-z0-9-]+\.wiki\.gg)\b").expect("static regex"));
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

/// **An article's pictures** (`chat/pictures.rs`): the files it uses
/// (`generator=images`) with their full-size links and sizes
/// (`prop=imageinfo&iiprop=url|size|mime`) — and a 1,600-px rendering
/// (`iiurlwidth`) for a drawing (SVG) or a file too big to download. One
/// request, after the article's own.
pub fn wiki_images_url(api: &str, pageid: u64) -> Result<Url, String> {
    api_url(
        api,
        &[
            ("action", "query"),
            ("generator", "images"),
            ("pageids", &pageid.to_string()),
            ("gimlimit", "30"),
            ("prop", "imageinfo"),
            ("iiprop", "url|size|mime"),
            ("iiurlwidth", "1600"),
        ],
    )
}

/// A file's name as its address writes it ("File:Fort Joy map.png" →
/// "fort_joy_map.png"). Pure.
fn file_key(title: &str) -> String {
    let name = title.split_once(':').map_or(title, |(_, n)| n);
    name.trim().replace(' ', "_").to_lowercase()
}

/// A path with its `%XX` escapes decoded, lowercased. Pure.
fn decoded_path(url: &str) -> String {
    let path = Url::parse(url).map(|u| u.path().to_string()).unwrap_or_default();
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() && bytes[i + 1].is_ascii_hexdigit() && bytes[i + 2].is_ascii_hexdigit() {
            let hex = |b: u8| (b as char).to_digit(16).unwrap_or(0) as u8;
            out.push(hex(bytes[i + 1]) * 16 + hex(bytes[i + 2]));
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_lowercase()
}

/// **A wiki article's pictures** from its images answer
/// ([`wiki_images_url`]): each file's full-size link — or, for a drawing
/// (SVG) or a file over 10 MB, its 1,600-px rendering — and size, about
/// its file's name; with the alt text, caption and heading the article's
/// HTML gives it (`shown`: `page::page_images` of the parse answer, matched
/// by the file's name in their addresses), those in the article's order
/// first. Page furniture (icons, logos, flags…) left out. Pure.
pub fn parse_wiki_images(v: &Value, page_url: &str, shown: &[PageImage]) -> Vec<FoundImage> {
    let mut found: Vec<(usize, FoundImage)> = Vec::new();
    for p in v["query"]["pages"].as_array().into_iter().flatten() {
        let Some(title) = p["title"].as_str() else { continue };
        let Some(info) = p["imageinfo"].as_array().and_then(|a| a.first()) else { continue };
        let mime = info["mime"].as_str().unwrap_or("");
        let size = info["size"].as_u64().unwrap_or(0);
        let rendered = mime == "image/svg+xml" || size > super::MAX_PICTURE_BYTES as u64;
        let (url, w, h) = if rendered {
            (info["thumburl"].as_str(), info["thumbwidth"].as_u64(), info["thumbheight"].as_u64())
        } else {
            (info["url"].as_str(), info["width"].as_u64(), info["height"].as_u64())
        };
        let Some(url) = url else { continue };
        let (w, h) = (w.map(|n| n as u32), h.map(|n| n as u32));
        let key = file_key(title);
        let name = title.split_once(':').map_or(title, |(_, n)| n);
        let stem = name.rsplit_once('.').map_or(name, |(s, _)| s).replace('_', " ");
        // Where the article shows it: its alt text, caption and heading.
        let at = shown.iter().position(|s| decoded_path(&s.url).contains(&format!("/{key}")));
        let Some(mut image) = super::picture_at(url, &stem, w, h) else { continue };
        if let Some(i) = at {
            let s = &shown[i];
            if !s.alt.trim().is_empty() {
                image.alt = s.alt.clone();
            }
            image.caption = s.caption.clone();
            image.heading = s.heading.clone();
            if super::is_junk(url, &image.alt, "", w, h) {
                continue;
            }
        }
        found.push((at.unwrap_or(usize::MAX), FoundImage { image, page: Some(page_url.to_string()) }));
    }
    found.sort_by_key(|(at, _)| *at);
    found.into_iter().map(|(_, f)| f).collect()
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
/// paragraphs, then the next one's — and the top article's pictures
/// ([`wiki_images_url`], `chat/pictures.rs`'s candidates). ≤ 3 searches + 2
/// article reads + 1 for the pictures.
pub(super) async fn wiki_lookup(ep: &Endpoints, api: &str, query: &str, game: Option<&GameWiki>) -> Result<WikiFound, String> {
    // Never a wiki that forbids automated requests (Fandom), whoever asks:
    // nothing found there, so the lookup goes on to Wikipedia.
    if !allows_automated(&site_of(api)) {
        tracing::info!("Web search: a wiki that forbids automated requests was skipped");
        return Ok(WikiFound { hits: Vec::new(), images: Vec::new() });
    }
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
    let mut images = Vec::new();
    for (i, page) in pages.iter().take(3).enumerate() {
        // The top two articles are read; the third is named only.
        let mut shown = Vec::new();
        let text = if i < 2 {
            let url = if wikipedia { wiki_extract_url(api, page.pageid)? } else { wiki_parse_url(api, page.pageid)? };
            match wiki_get(ep, &url).await {
                Ok(v) => {
                    // The article's HTML: where it shows each picture.
                    if let Some(html) = v["parse"]["text"].as_str().or(v["parse"]["text"]["*"].as_str()) {
                        shown = super::page_images(&format!("<html><body>{html}</body></html>"), &page.url);
                    }
                    parse_wiki_text(&v).map(|t| best_passages(&t, &words, if i == 0 { 1_200 } else { HIT_CHARS })).unwrap_or_default()
                }
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
        // The article used (the first one read): its pictures, with their
        // full-size links. Best-effort: the text answers without them.
        if hits.is_empty() {
            match wiki_get(ep, &wiki_images_url(api, page.pageid)?).await {
                Ok(v) => images = parse_wiki_images(&v, &page.url, &shown),
                Err(e) => tracing::debug!("Web search: no pictures from the wiki ({e})"),
            }
        }
        hits.push(Hit { title: page.title.clone(), url: page.url.clone(), published: None, text });
    }
    Ok(WikiFound { hits, images })
}

/// What a wiki lookup found: its articles as hits, and the first one's
/// pictures.
pub(super) struct WikiFound {
    pub hits: Vec<Hit>,
    pub images: Vec<FoundImage>,
}

/// The keyless lookup: the game's wiki (or a wiki the question names), else
/// Wikipedia in `lang`, else English Wikipedia.
pub(super) async fn keyless(s: &Settings, ep: &Endpoints, query: &str, context: &str) -> Result<Found, String> {
    let game = game_for(query).or_else(|| game_for(context));
    let named = wiki_named_in(query).or_else(|| wiki_named_in(context));
    let mut errors = Vec::new();
    if let Some(api) = named.or_else(|| game.map(|g| g.api.to_string())) {
        match wiki_lookup(ep, &api, query, game).await {
            Ok(w) if !w.hits.is_empty() => return Ok(Found { hits: w.hits, images: w.images, from: site_of(&api), ..Default::default() }),
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
            Ok(w) if !w.hits.is_empty() => return Ok(Found { hits: w.hits, images: w.images, from: "Wikipedia".into(), ..Default::default() }),
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
        // Never Fandom (its terms forbid automated requests): named or linked.
        assert_eq!(api_for_host("starwars.fandom.com"), None);
        assert_eq!(api_for_host("eldenring.fandom.com"), None);
        assert_eq!(wiki_named_in("search eldenring.fandom.com for Ranni"), None);
        assert!(game_for("How do I beat Malenia in Elden Ring?").is_none(), "no wiki for it: Wikipedia, as any game without one");
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

    /// A made-up article's images answer (`generator=images` +
    /// `imageinfo`, formatversion 2, the shape MediaWiki documents): a map
    /// the article shows in a thumbnail, a drawing (SVG: its rendering), a
    /// file too big to download (its rendering), an icon, and a file with
    /// no info (another wiki's, missing).
    #[test]
    fn a_wiki_articles_pictures_come_full_size_with_their_captions() {
        let url = wiki_images_url("https://wiki.example.com/api.php", 118).unwrap();
        assert!(url.as_str().contains("generator=images&pageids=118&gimlimit=30&prop=imageinfo&iiprop=url%7Csize%7Cmime&iiurlwidth=1600"), "{url}");
        let v = json!({ "query": { "pages": [
            { "pageid": 5, "ns": 6, "title": "File:Ancient Ruins.jpg", "imageinfo": [{ "size": 12_000_000_u64, "width": 6000, "height": 4000, "url": "https://wiki.example.com/images/a/a1/Ancient_Ruins.jpg", "mime": "image/jpeg", "thumburl": "https://wiki.example.com/images/thumb/a/a1/Ancient_Ruins.jpg/1600px-Ancient_Ruins.jpg", "thumbwidth": 1600, "thumbheight": 1067 }] },
            { "pageid": 6, "ns": 6, "title": "File:Fort Joy map.png", "imageinfo": [{ "size": 2_400_000, "width": 2048, "height": 1536, "url": "https://wiki.example.com/images/b/b2/Fort_Joy_map.png", "mime": "image/png", "thumburl": "https://wiki.example.com/images/thumb/b/b2/Fort_Joy_map.png/1600px-Fort_Joy_map.png", "thumbwidth": 1600, "thumbheight": 1200 }] },
            { "pageid": 7, "ns": 6, "title": "File:Icon fire.png", "imageinfo": [{ "size": 900, "width": 24, "height": 24, "url": "https://wiki.example.com/images/c/c3/Icon_fire.png", "mime": "image/png" }] },
            { "pageid": 8, "ns": 6, "title": "File:Route plan.svg", "imageinfo": [{ "size": 30_000, "width": 800, "height": 600, "url": "https://wiki.example.com/images/d/d4/Route_plan.svg", "mime": "image/svg+xml", "thumburl": "https://wiki.example.com/images/thumb/d/d4/Route_plan.svg/1600px-Route_plan.svg.png", "thumbwidth": 1600, "thumbheight": 1200 }] },
            { "ns": 6, "title": "File:Elsewhere.jpg", "missing": true }
        ] } });
        // Where the article shows the map: its thumbnail, captioned.
        let html = r#"<html><body><h2>Map</h2><div class="thumb"><div class="thumbinner"><img src="https://wiki.example.com/images/thumb/b/b2/Fort_Joy_map.png/300px-Fort_Joy_map.png" width="300" height="225" alt=""><div class="thumbcaption">Every treasure chest in Fort Joy</div></div></div></body></html>"#;
        let shown = crate::web::page_images(html, "https://wiki.example.com/wiki/Fort_Joy");
        let found = parse_wiki_images(&v, "https://wiki.example.com/wiki/Fort_Joy", &shown);
        let urls: Vec<&str> = found.iter().map(|f| f.image.url.as_str()).collect();
        assert_eq!(
            urls,
            [
                // The one the article shows, first, full size, captioned.
                "https://wiki.example.com/images/b/b2/Fort_Joy_map.png",
                // Too big to download: the rendering.
                "https://wiki.example.com/images/thumb/a/a1/Ancient_Ruins.jpg/1600px-Ancient_Ruins.jpg",
                // A drawing: the rendering (a PNG).
                "https://wiki.example.com/images/thumb/d/d4/Route_plan.svg/1600px-Route_plan.svg.png",
            ]
        );
        let map = &found[0].image;
        assert_eq!((map.alt.as_str(), map.caption.as_str(), map.heading.as_str()), ("Fort Joy map", "Every treasure chest in Fort Joy", "Map"));
        assert_eq!((map.w, map.h), (Some(2048), Some(1536)));
        assert_eq!(found[1].image.alt, "Ancient Ruins");
        assert_eq!((found[1].image.w, found[1].image.h), (Some(1600), Some(1067)));
        assert!(found.iter().all(|f| f.page.as_deref() == Some("https://wiki.example.com/wiki/Fort_Joy")));
        assert_eq!(decoded_path("https://wiki.example.com/images/b/b2/Fort%27s_Map.png"), "/images/b/b2/fort's_map.png");
        assert_eq!(file_key("File:Fort's Map.png"), "fort's_map.png");
    }

    /// Fandom's terms forbid automated requests: no game wiki Yap knows is
    /// one, and no lookup asks one anything. Wikipedia, Commons, wiki.gg and
    /// the independent game wikis are asked.
    #[test]
    fn fandom_is_never_asked() {
        for no in ["eldenring.fandom.com", "Witcher.Fandom.com.", "fandom.com"] {
            assert!(!allows_automated(no), "{no}");
        }
        for yes in ["en.wikipedia.org", "commons.wikimedia.org", "terraria.wiki.gg", "bg3.wiki", "minecraft.wiki", "en.uesp.net", "runescape.wiki", "notfandom.com.example"] {
            assert!(allows_automated(yes), "{yes}");
        }
        for g in GAME_WIKIS {
            assert!(allows_automated(&site_of(g.api)), "{}", g.game);
        }
    }

    /// A Fandom wiki's lookup asks it nothing at all (text or pictures);
    /// another wiki's reads the article, then asks once for its pictures.
    #[tokio::test]
    async fn a_fandom_lookup_asks_nothing_and_a_wikis_asks_for_its_pictures_once() {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("a local port");
        let port = server.server_addr().to_ip().expect("ip").port();
        let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let kept = asked.clone();
        std::thread::spawn(move || {
            for req in server.incoming_requests() {
                let url = req.url().to_string();
                kept.lock().unwrap().push(url.clone());
                let body = if url.contains("generator=search") {
                    json!({ "query": { "pages": [{ "pageid": 7, "title": "Ranni", "index": 1, "fullurl": "https://example.invalid/wiki/Ranni" }] } })
                } else if url.contains("action=parse") {
                    json!({ "parse": { "text": "<p>Ranni the Witch waits at her rise in the lakes, past the church.</p>" } })
                } else {
                    json!({ "query": { "pages": [] } })
                };
                let header = tiny_http::Header::from_bytes("content-type", "application/json").unwrap();
                let _ = req.respond(tiny_http::Response::from_string(body.to_string()).with_header(header));
            }
        });
        let ep = Endpoints { exa: String::new(), tavily: String::new(), standin: Some(format!("http://127.0.0.1:{port}/standin")), offline: false };
        let found = wiki_lookup(&ep, "https://eldenring.fandom.com/api.php", "Where is Ranni in Elden Ring?", None).await.unwrap();
        assert!(found.hits.is_empty() && found.images.is_empty());
        assert!(asked.lock().unwrap().is_empty(), "nothing asked of Fandom");
        let bg3 = game_for("bg3").unwrap();
        let found = wiki_lookup(&ep, bg3.api, "Where is Ranni in bg3?", Some(bg3)).await.unwrap();
        assert_eq!(found.hits.len(), 1);
        let asked = asked.lock().unwrap().clone();
        assert!(asked.iter().all(|u| u.starts_with("/standin/bg3.wiki/")), "{asked:?}");
        assert_eq!(asked.iter().filter(|u| u.contains("generator=images")).count(), 1, "{asked:?}");
    }

    #[test]
    fn with_no_match_the_start_of_the_page_is_given() {
        let text = "Intro line one is here.\nSecond paragraph about other things.";
        assert!(best_passages(text, &keywords("zebra"), 25).starts_with("Intro line one is here."));
    }
}

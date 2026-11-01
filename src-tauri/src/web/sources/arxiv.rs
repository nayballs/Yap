//! **Papers: the arXiv API** (`export.arxiv.org/api/query`, keyless, Atom).
//! Its terms of use (info.arxiv.org/help/api/tou.html, read 1 Nov 2026):
//! "make no more than one request every three seconds, and limit requests
//! to a single connection at a time" (Yap: one every 3.1 s, one at a time,
//! answers cached for an hour); never "Represent your project as endorsed or
//! supported by arXiv.org without our permission" (Yap only cites the
//! papers); descriptive metadata "under the terms of the Creative Commons
//! Universal (CC0 1.0) Public Domain Declaration". Its API page asks every
//! user to acknowledge arXiv with "Thank you to arXiv for use of its open
//! access interoperability." — said in Settings → Voice Assistant → Web
//! search, where the free sources are listed (`WebSearchSettings.svelte`).

use std::time::Duration;

use reqwest::Url;

use crate::privacy::net::FreeSource;

use super::super::{tidy, Endpoints, Hit};
use super::polite::{self, Ask, Rules};
use super::route::normal;
use super::Out;

pub const API: &str = "https://export.arxiv.org/api/query";
const RULES: Rules = Rules { gap: Duration::from_millis(3_100), per_minute: None, keep: Duration::from_secs(3600) };

/// What arXiv's search gets: the words that say what, without "recent
/// papers on", "arxiv", "research about"… each one `all:` and joined with
/// AND (its query syntax), five at most. Pure.
pub fn search_query(query: &str) -> String {
    const FILLER: &[&str] = &[
        "a", "an", "the", "on", "about", "into", "of", "for", "by", "from", "in", "and", "or", "with", "to", "arxiv",
        "paper", "papers", "preprint", "preprints", "research", "study", "studies", "recent", "latest", "new", "any",
        "find", "me", "show", "what", "are", "is", "there", "scientific", "academic", "that", "show", "showing",
        "published", "publication", "publications",
    ];
    let words: Vec<String> = query
        .split_whitespace()
        .map(normal)
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-').to_string())
        .filter(|w| !w.is_empty() && !FILLER.contains(&w.as_str()))
        .take(5)
        .collect();
    words.iter().map(|w| format!("all:{w}")).collect::<Vec<_>>().join(" AND ")
}

/// The request: by relevance, three papers. Pure.
pub fn search_url(search_query: &str) -> Url {
    let mut url = Url::parse(API).expect("static address");
    url.query_pairs_mut()
        .append_pair("search_query", search_query)
        .append_pair("start", "0")
        .append_pair("max_results", "3")
        .append_pair("sortBy", "relevance")
        .append_pair("sortOrder", "descending");
    url
}

/// arXiv's Atom feed as results: each `<entry>`'s title, its abstract page
/// (`<id>`, https), its date (`<published>`), its authors (three, then "et
/// al.") and abstract (`<summary>`). Read with the HTML parser Yap already
/// has (dom_query: entities decoded); a feed with an error entry gives
/// nothing. Pure.
pub fn parse(xml: &str) -> Vec<Hit> {
    let doc = dom_query::Document::from(xml);
    let mut hits = Vec::new();
    for entry in doc.select("entry").iter() {
        let text = |sel: &str| entry.select(sel).first().text().split_whitespace().collect::<Vec<_>>().join(" ");
        let id = text("id");
        let Some(url) = id.strip_prefix("http://").map(|r| format!("https://{r}")).or_else(|| id.starts_with("https://").then(|| id.clone())) else { continue };
        if !url.contains("arxiv.org/abs/") {
            continue;
        }
        let title = text("title");
        if title.is_empty() || title.eq_ignore_ascii_case("error") {
            continue;
        }
        let authors: Vec<String> = entry.select("author name").iter().map(|n| n.text().trim().to_string()).filter(|n| !n.is_empty()).collect();
        let by = match authors.len() {
            0 => String::new(),
            1..=3 => format!("{}. ", authors.join(", ")),
            _ => format!("{} et al. ", authors[..3].join(", ")),
        };
        let date: String = text("published").chars().take(10).collect();
        hits.push(Hit {
            title: tidy(&title, 160),
            url,
            published: (date.len() == 10).then_some(date),
            text: tidy(&format!("{by}{}", text("summary")), 800),
        });
    }
    hits
}

/// **arXiv for a search**.
pub(super) async fn look_up(ep: &Endpoints, query: &str) -> Result<Out, String> {
    let q = search_query(query);
    if q.is_empty() {
        return Ok(Out { from: "arXiv".into(), ..Default::default() });
    }
    let body = polite::get(ep, &search_url(&q), Ask { purpose: FreeSource::Arxiv, rules: RULES, accept: "application/atom+xml", headers: &[] }).await?;
    Ok(Out { hits: parse(&body), from: "arXiv".into(), ..Default::default() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_search_says_what_in_its_own_syntax() {
        assert_eq!(search_query("recent papers on speech recognition"), "all:speech AND all:recognition");
        assert_eq!(search_query("arxiv diffusion models survey"), "all:diffusion AND all:models AND all:survey");
        assert_eq!(
            search_url("all:speech AND all:recognition").to_string(),
            "https://export.arxiv.org/api/query?search_query=all%3Aspeech+AND+all%3Arecognition&start=0&max_results=3&sortBy=relevance&sortOrder=descending"
        );
    }

    // arXiv's documented Atom shape (info.arxiv.org/help/api/user-manual),
    // a made-up paper.
    const FEED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <link href="http://arxiv.org/api/query?search_query%3Dall%3Aspeech" rel="self" type="application/atom+xml"/>
  <title type="html">ArXiv Query: search_query=all:speech&amp;id_list=</title>
  <id>http://arxiv.org/api/abc</id>
  <updated>2026-11-01T00:00:00-04:00</updated>
  <opensearch:totalResults xmlns:opensearch="http://a9.com/-/spec/opensearch/1.1/">1</opensearch:totalResults>
  <entry>
    <id>http://arxiv.org/abs/2610.01234v2</id>
    <updated>2026-10-20T17:00:00Z</updated>
    <published>2026-10-02T12:00:00Z</published>
    <title>Whispering Small: Speech Recognition &amp; Tiny
      Models on Laptops</title>
    <summary>  We show that a 0.6B model transcribes speech on a laptop's
      graphics card faster than real time.</summary>
    <author><name>Jane Example</name></author>
    <author><name>Sam Sample</name></author>
    <author><name>Alex Placeholder</name></author>
    <author><name>Robin Test</name></author>
    <arxiv:comment xmlns:arxiv="http://arxiv.org/schemas/atom">12 pages</arxiv:comment>
    <link href="http://arxiv.org/abs/2610.01234v2" rel="alternate" type="text/html"/>
    <link title="pdf" href="http://arxiv.org/pdf/2610.01234v2" rel="related" type="application/pdf"/>
    <arxiv:primary_category xmlns:arxiv="http://arxiv.org/schemas/atom" term="cs.CL" scheme="http://arxiv.org/schemas/atom"/>
    <category term="cs.CL" scheme="http://arxiv.org/schemas/atom"/>
  </entry>
</feed>"#;

    #[test]
    fn its_feed_becomes_results() {
        let hits = parse(FEED);
        assert_eq!(hits.len(), 1, "the feed's own title isn't a paper");
        let h = &hits[0];
        assert_eq!(h.title, "Whispering Small: Speech Recognition & Tiny Models on Laptops");
        assert_eq!(h.url, "https://arxiv.org/abs/2610.01234v2");
        assert_eq!(h.published.as_deref(), Some("2026-10-02"));
        assert_eq!(h.text, "Jane Example, Sam Sample, Alex Placeholder et al. We show that a 0.6B model transcribes speech on a laptop's graphics card faster than real time.");
        // arXiv's error entry: nothing.
        let error = r#"<feed xmlns="http://www.w3.org/2005/Atom"><entry><id>http://arxiv.org/api/errors#incorrect_id_format</id><title>Error</title><summary>incorrect id format</summary></entry></feed>"#;
        assert!(parse(error).is_empty());
    }
}

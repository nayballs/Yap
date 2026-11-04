//! **Code: GitHub's search API** (`api.github.com`, unauthenticated) —
//! repositories for a library, a tool or a project; issues for an error.
//! Its docs (docs.github.com/en/rest/search/search, read 1 Nov 2026): "For
//! unauthenticated requests, the rate limit allows you to make up to 10
//! requests per minute" (Yap keeps to that, and a 403 / 429 with
//! `x-ratelimit-remaining: 0` holds GitHub until its `x-ratelimit-reset`);
//! code search "requires you to authenticate", so Yap never asks it; the
//! recommended `Accept: application/vnd.github+json` and an
//! `X-GitHub-Api-Version`, with Yap's User-Agent (GitHub's API requires
//! one). Answers cached for an hour.

use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;
use reqwest::Url;
use serde_json::Value;

use crate::privacy::net::FreeSource;

use super::super::{tidy, Endpoints, Hit};
use super::polite::{self, Ask, Rules};
use super::route::normal;
use super::Out;

pub const API: &str = "https://api.github.com";
const RULES: Rules = Rules { gap: Duration::from_millis(500), per_minute: Some(10), keep: Duration::from_secs(3600) };
const HEADERS: &[(&str, &str)] = &[("X-GitHub-Api-Version", "2022-11-28")];
const RESULTS: &str = "3";

/// Which search: an error's issues, or repositories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Search {
    Repositories,
    Issues,
}

/// An error, a crash or a bug: its issues. Pure.
pub fn search_for(query: &str) -> Search {
    static ERROR: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\b(error|errors|exception|panic|panicked|traceback|crash|crashes|crashing|segfault|failed|fails|failing|bug|regression|e\d{4})\b").expect("static regex")
    });
    if ERROR.is_match(query) {
        Search::Issues
    } else {
        Search::Repositories
    }
}

/// What GitHub's search gets: the words that say what, without "a Rust
/// crate for", "open-source", "github repo"… — "an alternative to Notion"
/// as "Notion alternative" (how such projects describe themselves; searched
/// by stars, [`search_url`]) — and the language a repository search names,
/// as GitHub's own `language:` qualifier. Pure.
pub fn terms(query: &str, search: Search) -> String {
    const FILLER: &[&str] = &[
        "a", "an", "the", "for", "to", "that", "which", "is", "are", "there", "any", "good", "best", "find", "me", "show",
        "github", "repo", "repos", "repository", "repositories", "open-source", "open", "source", "library",
        "libraries", "crate", "crates", "package", "packages", "module", "modules", "framework", "frameworks", "project", "projects", "tool", "tools", "app", "apps", "code", "of", "in", "on",
        "with", "how", "do", "i", "can", "what", "please", "issue", "issues", "gem", "gems", "plugin", "plugins",
        "npm", "written",
    ];
    const LANGUAGES: &[(&str, &str)] = &[
        ("rust", "rust"), ("python", "python"), ("javascript", "javascript"), ("js", "javascript"), ("typescript", "typescript"),
        ("go", "go"), ("golang", "go"), ("java", "java"), ("kotlin", "kotlin"), ("swift", "swift"), ("ruby", "ruby"),
        ("php", "php"), ("c#", "c#"), ("c++", "c++"), ("haskell", "haskell"), ("lua", "lua"), ("dart", "dart"),
    ];
    let mut language = None;
    let mut alternative = false;
    let mut words: Vec<String> = Vec::new();
    for w in query.split_whitespace() {
        let n = normal(w);
        let n = n.trim_matches(|c: char| matches!(c, ',' | ';' | '(' | ')' | '?' | '!'));
        if search == Search::Repositories {
            if let Some((_, l)) = LANGUAGES.iter().find(|(name, _)| *name == n) {
                language.get_or_insert(*l);
                continue;
            }
            if matches!(n, "alternative" | "alternatives") {
                alternative = true;
                continue;
            }
            if FILLER.contains(&n) {
                continue;
            }
        }
        words.push(w.trim_matches(|c: char| matches!(c, '?' | '!' | ',' | ';' | '"')).to_string());
    }
    let mut words: Vec<String> = words.into_iter().filter(|w| !w.is_empty()).take(8).collect();
    if alternative && !words.is_empty() {
        words.push(ALTERNATIVE.to_string());
    }
    let mut q = words.join(" ");
    if let Some(l) = language {
        q = format!("{q} language:{l}").trim().to_string();
    }
    match search {
        Search::Issues => format!("{} is:issue", tidy(&q, 200).replace('…', "")),
        Search::Repositories => tidy(&q, 240).replace('…', ""),
    }
}

/// The word [`terms`] ends an "alternative to …" search with.
const ALTERNATIVE: &str = "alternative";

/// The search's request: three results, best match first — but an
/// "alternative to …" by stars: by best match, "Notion alternative" puts an
/// awesome-list and an archived project above the ones people use, while by
/// stars a specific search ("parsing PDFs language:rust") picks up
/// unrelated popular projects. Pure.
pub fn search_url(search: Search, q: &str) -> Url {
    let path = match search {
        Search::Repositories => "repositories",
        Search::Issues => "issues",
    };
    let mut url = Url::parse(&format!("{API}/search/{path}")).expect("static address");
    url.query_pairs_mut().append_pair("q", q);
    if search == Search::Repositories && q.split_whitespace().any(|w| w == ALTERNATIVE) {
        url.query_pairs_mut().append_pair("sort", "stars");
    }
    url.query_pairs_mut().append_pair("per_page", RESULTS);
    url
}

fn date(v: &Value) -> Option<String> {
    let s: String = v.as_str()?.chars().take(10).collect();
    (s.len() == 10).then_some(s)
}

/// Repositories as results (`items[]: {full_name, html_url, description,
/// stargazers_count, language, license, pushed_at}`). Pure.
pub fn parse_repositories(v: &Value) -> Vec<Hit> {
    v["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| {
            let url = r["html_url"].as_str()?.to_string();
            super::super::check_url(&url).ok()?;
            let mut text = r["description"].as_str().map(str::trim).filter(|d| !d.is_empty()).unwrap_or("No description.").to_string();
            if !text.ends_with(['.', '!', '?']) {
                text.push('.');
            }
            let mut facts = vec![format!("{} stars", r["stargazers_count"].as_u64().unwrap_or(0))];
            if let Some(l) = r["language"].as_str() {
                facts.push(l.to_string());
            }
            if let Some(l) = r["license"]["spdx_id"].as_str().filter(|l| *l != "NOASSERTION") {
                facts.push(format!("licence {l}"));
            }
            if r["archived"].as_bool() == Some(true) {
                facts.push("archived".into());
            }
            let pushed = date(&r["pushed_at"]);
            if let Some(d) = &pushed {
                facts.push(format!("last changed {d}"));
            }
            Some(Hit {
                title: tidy(r["full_name"].as_str()?, 120),
                url,
                published: pushed,
                text: tidy(&format!("{text} {}.", facts.join(" · ")), 500),
            })
        })
        .collect()
}

/// Issues as results (`items[]: {title, html_url, number, state, comments,
/// repository_url, body, created_at}`). Pure.
pub fn parse_issues(v: &Value) -> Vec<Hit> {
    v["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| {
            let url = i["html_url"].as_str()?.to_string();
            super::super::check_url(&url).ok()?;
            let repo = i["repository_url"].as_str().and_then(|u| u.split("/repos/").nth(1)).unwrap_or("");
            let number = i["number"].as_u64().unwrap_or(0);
            let title = format!("{} · {repo} #{number}", i["title"].as_str()?.trim());
            let state = i["state"].as_str().unwrap_or("open");
            let comments = i["comments"].as_u64().unwrap_or(0);
            let body = i["body"].as_str().unwrap_or("").replace("```", " ");
            Some(Hit {
                title: tidy(&title, 140),
                url,
                published: date(&i["created_at"]),
                text: tidy(&format!("Issue, {state}, {comments} comment{}: {}", if comments == 1 { "" } else { "s" }, body.trim()), 700),
            })
        })
        .collect()
}

/// **GitHub for a search**: an error's issues, else repositories — with the
/// longest two words alone when every word found nothing.
pub(super) async fn look_up(ep: &Endpoints, query: &str, _context: &str) -> Result<Out, String> {
    let search = search_for(query);
    let ask = || Ask { purpose: FreeSource::GitHub, rules: RULES, accept: "application/vnd.github+json", headers: HEADERS };
    let read = |body: String| serde_json::from_str::<Value>(&body).map_err(|_| "GitHub sent an answer Yap couldn't read.".to_string());
    let parse = |v: &Value| match search {
        Search::Repositories => parse_repositories(v),
        Search::Issues => parse_issues(v),
    };
    let q = terms(query, search);
    if q.trim().is_empty() || q.trim() == "is:issue" {
        return Ok(Out { from: "GitHub".into(), ..Default::default() });
    }
    let mut hits = parse(&read(polite::get(ep, &search_url(search, &q), ask()).await?)?);
    if hits.is_empty() && search == Search::Repositories {
        let mut words: Vec<&str> = q.split_whitespace().filter(|w| !w.starts_with("language:")).collect();
        if words.len() > 2 {
            words.sort_by_key(|w| std::cmp::Reverse(w.chars().count()));
            let language = q.split_whitespace().find(|w| w.starts_with("language:")).unwrap_or("");
            let fewer = format!("{} {} {language}", words[0], words[1]);
            hits = parse(&read(polite::get(ep, &search_url(search, fewer.trim()), ask()).await?)?);
        }
    }
    Ok(Out { hits, from: "GitHub".into(), ..Default::default() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn its_requests_ask_for_what_matters() {
        assert_eq!(search_for("a Rust crate for parsing PDFs"), Search::Repositories);
        assert_eq!(search_for("tauri build error webview2 not found"), Search::Issues);
        assert_eq!(terms("a Rust crate for parsing PDFs", Search::Repositories), "parsing PDFs language:rust");
        // An alternative: how such projects say it, by stars.
        assert_eq!(terms("open-source alternative to Photoshop", Search::Repositories), "Photoshop alternative");
        assert_eq!(terms("Is there an open source alternative to Notion?", Search::Repositories), "Notion alternative");
        assert_eq!(terms("open source Notion alternatives in Rust", Search::Repositories), "Notion alternative language:rust");
        assert_eq!(
            search_url(Search::Repositories, "Notion alternative").to_string(),
            "https://api.github.com/search/repositories?q=Notion+alternative&sort=stars&per_page=3"
        );
        assert_eq!(terms("github repo for whisper.cpp", Search::Repositories), "whisper.cpp");
        assert_eq!(terms("tauri build error webview2 not found", Search::Issues), "tauri build error webview2 not found is:issue");
        assert_eq!(
            search_url(Search::Repositories, "parsing PDFs language:rust").to_string(),
            "https://api.github.com/search/repositories?q=parsing+PDFs+language%3Arust&per_page=3"
        );
        assert!(search_url(Search::Issues, "x is:issue").to_string().starts_with("https://api.github.com/search/issues?q=x+is%3Aissue"));
    }

    #[test]
    fn repositories_and_issues_become_results() {
        // GitHub's documented shapes, made-up projects.
        let repos = json!({ "total_count": 2, "incomplete_results": false, "items": [
            { "full_name": "example-org/pdfquill", "html_url": "https://github.com/example-org/pdfquill", "description": "Read and write PDF files in pure Rust",
              "stargazers_count": 2140, "language": "Rust", "license": { "spdx_id": "MIT" }, "archived": false, "pushed_at": "2026-10-20T08:00:00Z" },
            { "full_name": "someone/old-pdf", "html_url": "https://github.com/someone/old-pdf", "description": null,
              "stargazers_count": 3, "language": null, "license": { "spdx_id": "NOASSERTION" }, "archived": true, "pushed_at": "2019-01-02T00:00:00Z" }
        ] });
        let hits = parse_repositories(&repos);
        assert_eq!(hits[0].title, "example-org/pdfquill");
        assert_eq!(hits[0].text, "Read and write PDF files in pure Rust. 2140 stars · Rust · licence MIT · last changed 2026-10-20.");
        assert_eq!(hits[0].published.as_deref(), Some("2026-10-20"));
        assert_eq!(hits[1].text, "No description. 3 stars · archived · last changed 2019-01-02.");
        let issues = json!({ "items": [
            { "title": "Build fails: WebView2 not found", "html_url": "https://github.com/example-org/app/issues/77", "number": 77, "state": "closed",
              "comments": 4, "repository_url": "https://api.github.com/repos/example-org/app", "created_at": "2026-09-01T10:00:00Z",
              "body": "Running the build gives:\n```\nerror: WebView2 runtime not found\n```\nFixed by installing the runtime." }
        ] });
        let hits = parse_issues(&issues);
        assert_eq!(hits[0].title, "Build fails: WebView2 not found · example-org/app #77");
        assert!(hits[0].text.starts_with("Issue, closed, 4 comments: Running the build gives: error: WebView2 runtime not found Fixed by installing the runtime."), "{}", hits[0].text);
        assert!(parse_issues(&json!({ "message": "API rate limit exceeded" })).is_empty());
    }
}

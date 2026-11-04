//! **Programming questions: the Stack Exchange API** (`api.stackexchange.com
//! /2.3`, Stack Overflow, keyless). Its throttles (the API's throttle page,
//! as mirrored and quoted in yap-private `docs/web-search-without-keys-2026-10.md`
//! §2.3; the official page wouldn't load from here): 300 requests a day per
//! IP without an app key; "If an application receives a response with the
//! backoff field set, it must wait that many seconds before hitting the same
//! method again" (Yap holds the host that long, `polite::hold`); and "no
//! application should make semantically identical requests more than once a
//! minute" (answers are cached for an hour). Every answer comes compressed
//! (`polite::unzipped`). Two requests a search: the questions, then their
//! answers. Its content is CC BY-SA: each result links to its question.

use std::time::Duration;

use reqwest::Url;
use serde_json::Value;

use crate::privacy::net::FreeSource;

use super::super::wiki::fragment_text;
use super::super::{tidy, Endpoints, Hit};
use super::polite::{self, Ask, Rules};
use super::Out;

pub const API: &str = "https://api.stackexchange.com/2.3";
const HOST: &str = "api.stackexchange.com";
const RULES: Rules = Rules { gap: Duration::from_millis(200), per_minute: Some(20), keep: Duration::from_secs(3600) };
/// Questions a search gives.
const QUESTIONS: usize = 3;

/// The question search: by relevance, answered ones, with their text.
/// Pure.
pub fn search_url(query: &str) -> Url {
    let mut url = Url::parse(&format!("{API}/search/advanced")).expect("static address");
    url.query_pairs_mut()
        .append_pair("order", "desc")
        .append_pair("sort", "relevance")
        .append_pair("q", &tidy(query, 140).replace('…', ""))
        .append_pair("answers", "1")
        .append_pair("site", "stackoverflow")
        .append_pair("pagesize", &QUESTIONS.to_string())
        .append_pair("filter", "withbody");
    url
}

/// Their answers, best first. Pure.
pub fn answers_url(ids: &[u64]) -> Url {
    let ids: Vec<String> = ids.iter().map(u64::to_string).collect();
    let mut url = Url::parse(&format!("{API}/questions/{}/answers", ids.join(";"))).expect("static address");
    url.query_pairs_mut()
        .append_pair("order", "desc")
        .append_pair("sort", "votes")
        .append_pair("site", "stackoverflow")
        .append_pair("pagesize", "20")
        .append_pair("filter", "withbody");
    url
}

/// A question.
#[derive(Debug, Clone, PartialEq)]
pub struct Question {
    pub id: u64,
    pub title: String,
    pub url: String,
    pub score: i64,
    pub answers: u64,
    pub accepted: Option<u64>,
    pub date: Option<String>,
    pub body: String,
}

/// HTML entities as text ("&#39;" → "'"); a title's tags off. Pure.
fn plain(html: &str) -> String {
    let doc = dom_query::Document::from(format!("<html><body><p>{html}</p></body></html>"));
    doc.select("p").text().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A Unix time's date ("2026-10-18"). Pure.
fn day(secs: Option<i64>) -> Option<String> {
    chrono::DateTime::from_timestamp(secs?, 0).map(|d| d.format("%Y-%m-%d").to_string())
}

/// What the API said went wrong (`error_id`, `error_message`); `None` when
/// it answered. Pure.
pub fn api_error(v: &Value) -> Option<String> {
    let id = v["error_id"].as_i64()?;
    let message = v["error_message"].as_str().unwrap_or("");
    Some(match id {
        502 => "Stack Exchange is limiting how much Yap can ask right now.".to_string(),
        _ => format!("Stack Exchange didn't take the search: {}", tidy(message, 120)),
    })
}

/// The search's questions (`items[]`). Pure.
pub fn parse_questions(v: &Value) -> Vec<Question> {
    v["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|q| {
            let url = q["link"].as_str()?.to_string();
            super::super::check_url(&url).ok()?;
            Some(Question {
                id: q["question_id"].as_u64()?,
                title: plain(q["title"].as_str()?),
                url,
                score: q["score"].as_i64().unwrap_or(0),
                answers: q["answer_count"].as_u64().unwrap_or(0),
                accepted: q["accepted_answer_id"].as_u64(),
                date: day(q["creation_date"].as_i64()),
                body: fragment_text(q["body"].as_str().unwrap_or("")),
            })
        })
        .take(QUESTIONS)
        .collect()
}

/// **The questions with their best answer** as results: the accepted one,
/// else the highest scored; the question's text short, the answer's longer
/// (code kept as text). Pure.
pub fn hits(questions: &[Question], answers: &Value) -> Vec<Hit> {
    let all: Vec<&Value> = answers["items"].as_array().into_iter().flatten().collect();
    questions
        .iter()
        .map(|q| {
            let mine: Vec<&&Value> = all.iter().filter(|a| a["question_id"].as_u64() == Some(q.id)).collect();
            let best = q
                .accepted
                .and_then(|id| mine.iter().find(|a| a["answer_id"].as_u64() == Some(id)))
                .or_else(|| mine.iter().max_by_key(|a| a["score"].as_i64().unwrap_or(0)));
            let asked = format!("Question (score {}, {} answer{}): {}", q.score, q.answers, if q.answers == 1 { "" } else { "s" }, tidy(&q.body, 260));
            let text = match best {
                Some(a) => {
                    let label = if a["is_accepted"].as_bool() == Some(true) { "Accepted answer" } else { "Top answer" };
                    let body = fragment_text(a["body"].as_str().unwrap_or(""));
                    format!("{asked} {label} (score {}): {}", a["score"].as_i64().unwrap_or(0), tidy(&body, 900))
                }
                None => asked,
            };
            Hit { title: tidy(&q.title, 140), url: q.url.clone(), published: q.date.clone(), text }
        })
        .collect()
}

/// A `backoff` in an answer: the host is left alone that long.
fn back_off(v: &Value) {
    if let Some(secs) = v["backoff"].as_u64().filter(|s| *s > 0) {
        polite::hold(HOST, Duration::from_secs(secs));
    }
}

fn read(body: &str) -> Result<Value, String> {
    let v: Value = serde_json::from_str(body).map_err(|_| "Stack Exchange sent an answer Yap couldn't read.".to_string())?;
    back_off(&v);
    if let Some(e) = api_error(&v) {
        if v["error_id"].as_i64() == Some(502) {
            polite::hold(HOST, Duration::from_secs(3600));
        }
        return Err(e);
    }
    Ok(v)
}

/// **Stack Overflow for a search**: its best questions, then their answers.
pub(super) async fn look_up(ep: &Endpoints, query: &str) -> Result<Out, String> {
    let ask = || Ask { purpose: FreeSource::StackExchange, rules: RULES, accept: "application/json", headers: &[] };
    let questions = parse_questions(&read(&polite::get(ep, &search_url(query), ask()).await?)?);
    if questions.is_empty() {
        return Ok(Out { from: "Stack Overflow".into(), ..Default::default() });
    }
    let ids: Vec<u64> = questions.iter().map(|q| q.id).collect();
    // The answers are best-effort: the questions alone still help.
    let answers = match polite::get(ep, &answers_url(&ids), ask()).await.and_then(|b| read(&b)) {
        Ok(v) => v,
        Err(_) => {
            tracing::info!("Web search: no answers from Stack Exchange, the questions alone");
            Value::Null
        }
    };
    Ok(Out { hits: hits(&questions, &answers), from: "Stack Overflow".into(), ..Default::default() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // The API's documented shapes (api.stackexchange.com/docs), made-up posts.
    fn questions() -> Value {
        json!({ "items": [
            { "tags": ["rust"], "is_answered": true, "view_count": 5120, "accepted_answer_id": 902, "answer_count": 2, "score": 41,
              "creation_date": 1760000000, "question_id": 901, "link": "https://stackoverflow.com/questions/901/cannot-borrow-x-as-mutable",
              "title": "Cannot borrow &#39;x&#39; as mutable more than once", "body": "<p>The compiler says <code>E0499</code> when I push in a loop.</p>" },
            { "tags": ["rust"], "is_answered": true, "answer_count": 1, "score": 3, "creation_date": 1750000000, "question_id": 905,
              "link": "https://stackoverflow.com/questions/905/borrow-in-closure", "title": "Borrow in a closure", "body": "<p>Short.</p>" }
        ], "has_more": true, "quota_max": 300, "quota_remaining": 297 })
    }

    fn answers() -> Value {
        json!({ "items": [
            { "is_accepted": false, "score": 50, "answer_id": 903, "question_id": 901, "body": "<p>Clone it.</p>" },
            { "is_accepted": true, "score": 30, "answer_id": 902, "question_id": 901, "body": "<p>Take the mutable borrow in its own scope:</p><pre><code>{ let y = &amp;mut x; y.push(1); }</code></pre>" },
            { "is_accepted": false, "score": 2, "answer_id": 906, "question_id": 905, "body": "<p>Use <code>move</code>.</p>" }
        ] })
    }

    #[test]
    fn its_requests_are_the_documented_ones() {
        let url = search_url("rust cannot borrow as mutable more than once").to_string();
        assert_eq!(
            url,
            "https://api.stackexchange.com/2.3/search/advanced?order=desc&sort=relevance&q=rust+cannot+borrow+as+mutable+more+than+once&answers=1&site=stackoverflow&pagesize=3&filter=withbody"
        );
        assert_eq!(
            answers_url(&[901, 905]).to_string(),
            "https://api.stackexchange.com/2.3/questions/901;905/answers?order=desc&sort=votes&site=stackoverflow&pagesize=20&filter=withbody"
        );
    }

    #[test]
    fn questions_come_with_their_best_answer() {
        let qs = parse_questions(&questions());
        assert_eq!(qs.len(), 2);
        assert_eq!(qs[0].title, "Cannot borrow 'x' as mutable more than once", "entities as text");
        assert_eq!(qs[0].date.as_deref(), Some("2025-10-09"));
        let hits = hits(&qs, &answers());
        // The accepted answer, though another scored higher.
        assert!(hits[0].text.starts_with("Question (score 41, 2 answers): The compiler says E0499 when I push in a loop."), "{}", hits[0].text);
        assert!(hits[0].text.contains("Accepted answer (score 30): Take the mutable borrow in its own scope:"), "{}", hits[0].text);
        assert!(hits[0].text.contains("let y = &mut x; y.push(1);"), "code kept as text: {}", hits[0].text);
        // No accepted one: the highest scored.
        assert!(hits[1].text.contains("Top answer (score 2): Use move."), "{}", hits[1].text);
        assert_eq!(hits[1].url, "https://stackoverflow.com/questions/905/borrow-in-closure");
        // No answers came: the question alone.
        assert_eq!(super::hits(&qs, &Value::Null)[1].text, "Question (score 3, 1 answer): Short.");
    }

    #[test]
    fn its_errors_are_said_plainly() {
        let throttled = json!({ "error_id": 502, "error_message": "too many requests from this IP", "error_name": "throttle_violation" });
        assert_eq!(api_error(&throttled).as_deref(), Some("Stack Exchange is limiting how much Yap can ask right now."));
        let bad = json!({ "error_id": 400, "error_message": "site is required", "error_name": "bad_parameter" });
        assert_eq!(api_error(&bad).as_deref(), Some("Stack Exchange didn't take the search: site is required"));
        assert_eq!(api_error(&questions()), None);
    }
}

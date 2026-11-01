//! **Asking the free sources politely**: each host at its own pace
//! ([`Rules`]: a gap between requests, at most so many a minute), a host
//! that says to wait (`Retry-After`, a 429, Stack Exchange's `backoff`)
//! left alone until then ([`hold`]), answers cached for as long as each
//! source allows, Yap's descriptive User-Agent (`super::super::USER_AGENT`,
//! with its contact address), a compressed answer read (Stack Exchange
//! sends gzip), every request a receipt of its own. Generalises the
//! one-at-a-time gap `wiki.rs` keeps for the wikis.

use std::collections::{HashMap, VecDeque};
use std::io::Read as _;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use reqwest::Url;

use crate::privacy::net::{FreeSource, Purpose, Recorded as _};

use super::super::{network_error, Endpoints, API};

/// How a host may be asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rules {
    /// The gap between two requests (Nominatim's one second, arXiv's three).
    pub gap: Duration,
    /// At most this many a minute (GitHub's unauthenticated search: 10).
    pub per_minute: Option<usize>,
    /// How long an answer is kept.
    pub keep: Duration,
}

/// The longest Yap waits for a host's turn: past it, the source is skipped
/// for this search (the answer comes from the others).
pub const LONGEST_WAIT: Duration = Duration::from_secs(8);
const MINUTE: Duration = Duration::from_secs(60);
/// The most a source's answer may be (compressed, then read).
const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_READ: u64 = 4 * 1024 * 1024;
/// Answers kept at most (the oldest go first).
const MAX_KEPT: usize = 300;

/// One host's pace. Pure: the clock is passed in.
#[derive(Debug, Default)]
pub struct Pacer {
    /// The earliest the next request may go.
    next: Option<Instant>,
    /// When the last minute's requests went (or will).
    recent: VecDeque<Instant>,
    /// The host said to wait until then.
    until: Option<Instant>,
}

impl Pacer {
    /// A turn for one request: how long to wait for it, or — when the host
    /// said to wait, or the wait would be longer than [`LONGEST_WAIT`] —
    /// `Err` with how long until it may be asked again (no turn taken).
    pub fn reserve(&mut self, rules: Rules, now: Instant) -> Result<Duration, Duration> {
        if let Some(u) = self.until {
            if u > now {
                return Err(u - now);
            }
            self.until = None;
        }
        let mut at = self.next.map_or(now, |n| n.max(now));
        if let Some(most) = rules.per_minute.filter(|&n| n > 0) {
            while self.recent.front().is_some_and(|t| at.saturating_duration_since(*t) >= MINUTE) {
                self.recent.pop_front();
            }
            if self.recent.len() >= most {
                at = at.max(self.recent[self.recent.len() - most] + MINUTE);
            }
        }
        let wait = at.saturating_duration_since(now);
        if wait > LONGEST_WAIT {
            return Err(wait);
        }
        self.next = Some(at + rules.gap);
        self.recent.push_back(at);
        Ok(wait)
    }

    /// The host said to wait `for_` (from `now`).
    pub fn hold(&mut self, for_: Duration, now: Instant) {
        let until = now + for_;
        self.until = Some(self.until.map_or(until, |u| u.max(until)));
    }
}

/// An answer kept: when, for how long, the body.
type Kept = (Instant, Duration, String);

static PACERS: LazyLock<Mutex<HashMap<String, Pacer>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static KEPT: LazyLock<Mutex<HashMap<String, Kept>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// `host` said to wait (Stack Exchange's `backoff`, a `Retry-After`): it
/// isn't asked again before then. At most an hour.
pub fn hold(host: &str, for_: Duration) {
    let for_ = for_.min(Duration::from_secs(3600));
    tracing::info!(host, secs = for_.as_secs(), "Web search: a free source asked Yap to wait");
    PACERS.lock().unwrap_or_else(|p| p.into_inner()).entry(host.to_string()).or_default().hold(for_, Instant::now());
}

fn cached(url: &str) -> Option<String> {
    let kept = KEPT.lock().unwrap_or_else(|p| p.into_inner());
    kept.get(url).filter(|(at, keep, _)| at.elapsed() < *keep).map(|(_, _, body)| body.clone())
}

fn keep(url: &str, body: &str, for_: Duration) {
    let mut kept = KEPT.lock().unwrap_or_else(|p| p.into_inner());
    kept.retain(|_, (at, keep, _)| at.elapsed() < *keep);
    if kept.len() >= MAX_KEPT {
        if let Some(oldest) = kept.iter().min_by_key(|(_, (at, _, _))| *at).map(|(k, _)| k.clone()) {
            kept.remove(&oldest);
        }
    }
    kept.insert(url.to_string(), (Instant::now(), for_, body.to_string()));
}

/// Every answer and pace forgotten (the model evaluations' fresh start).
#[cfg(debug_assertions)]
pub fn clear() {
    KEPT.lock().unwrap_or_else(|p| p.into_inner()).clear();
    PACERS.lock().unwrap_or_else(|p| p.into_inner()).clear();
}

/// A body, unzipped when it came gzipped (its magic bytes), read to
/// [`MAX_READ`] at most. Pure.
pub fn unzipped(bytes: &[u8]) -> Result<String, String> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut out = String::new();
        flate2::read::GzDecoder::new(bytes).take(MAX_READ).read_to_string(&mut out).map_err(|_| "a compressed answer Yap couldn't read".to_string())?;
        return Ok(out);
    }
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

/// How long a host said to wait: `Retry-After` in seconds, or GitHub's
/// `x-ratelimit-reset` (a Unix time) once its `x-ratelimit-remaining` is 0.
/// Pure.
pub fn wait_from(headers: &reqwest::header::HeaderMap, now_unix: u64) -> Option<Duration> {
    let text = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(str::trim);
    if let Some(secs) = text("retry-after").and_then(|s| s.parse::<u64>().ok()) {
        return Some(Duration::from_secs(secs));
    }
    if text("x-ratelimit-remaining") == Some("0") {
        if let Some(reset) = text("x-ratelimit-reset").and_then(|s| s.parse::<u64>().ok()) {
            return Some(Duration::from_secs(reset.saturating_sub(now_unix).max(1)));
        }
    }
    None
}

/// One request to a free source.
pub struct Ask<'a> {
    pub purpose: FreeSource,
    pub rules: Rules,
    /// Its `Accept`.
    pub accept: &'a str,
    /// More headers (GitHub's API version).
    pub headers: &'a [(&'a str, &'a str)],
}

/// **GET `url` from a free source**, politely: its cached answer if it has
/// one; else at the host's pace (skipped, with why, when the host said to
/// wait or its turn is too far off), through the test run's stand-in web
/// (`Endpoints::route`), Yap's User-Agent, its receipt; a 429 / 503 or a
/// rate-limit's 403 holds the host for as long as it said (a minute when it
/// said nothing). The body as text (unzipped).
pub async fn get(ep: &Endpoints, url: &Url, ask: Ask<'_>) -> Result<String, String> {
    let key = url.as_str().to_string();
    if let Some(body) = cached(&key) {
        return Ok(body);
    }
    let host = url.host_str().unwrap_or("").to_string();
    let target = ep.route(url)?;
    let turn = PACERS.lock().unwrap_or_else(|p| p.into_inner()).entry(host.clone()).or_default().reserve(ask.rules, Instant::now());
    match turn {
        Err(again) => return Err(format!("{host} asked Yap to slow down: it can be asked again in about {} s.", again.as_secs().max(1))),
        Ok(wait) if !wait.is_zero() => tokio::time::sleep(wait).await,
        Ok(_) => {}
    }
    let mut req = API.get(target).header(reqwest::header::ACCEPT, ask.accept).header(reqwest::header::ACCEPT_ENCODING, "gzip");
    for (k, v) in ask.headers {
        req = req.header(*k, *v);
    }
    let resp = req.send_for(Purpose::FreeSource(ask.purpose)).await.map_err(|e| network_error(&host, &e))?;
    let status = resp.status().as_u16();
    let now_unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let said = wait_from(resp.headers(), now_unix);
    if status == 429 || status == 503 || (status == 403 && said.is_some()) {
        hold(&host, said.unwrap_or(MINUTE));
        return Err(format!("{host} is limiting how much Yap can ask right now."));
    }
    let bytes = resp.bytes().await.map_err(|e| network_error(&host, &e))?;
    if bytes.len() > MAX_BYTES {
        return Err(format!("{host} sent too much."));
    }
    let body = unzipped(&bytes).map_err(|e| format!("{host} sent {e}."))?;
    if !(200..300).contains(&status) {
        tracing::info!(host, status, "Web search: a free source said no");
        return Err(format!("{host} answered HTTP {status}."));
    }
    keep(&key, &body, ask.rules.keep);
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: Duration = Duration::from_secs(1);

    #[test]
    fn a_host_is_asked_at_its_own_pace() {
        // Nominatim: one a second.
        let rules = Rules { gap: S, per_minute: None, keep: S };
        let mut p = Pacer::default();
        let t0 = Instant::now();
        assert_eq!(p.reserve(rules, t0), Ok(Duration::ZERO));
        assert_eq!(p.reserve(rules, t0), Ok(S), "the next waits its second");
        assert_eq!(p.reserve(rules, t0), Ok(S * 2));
        assert_eq!(p.reserve(rules, t0 + S * 10), Ok(Duration::ZERO), "later: at once");
        // arXiv: three seconds apart; a wait past the longest skips the source.
        let arxiv = Rules { gap: S * 3, per_minute: None, keep: S };
        let mut p = Pacer::default();
        for i in 0..3u32 {
            assert_eq!(p.reserve(arxiv, t0), Ok(S * 3 * i));
        }
        assert_eq!(p.reserve(arxiv, t0), Err(S * 9), "nine seconds off: not now");
    }

    #[test]
    fn a_minutes_budget_is_kept() {
        // GitHub's search: ten a minute.
        let rules = Rules { gap: Duration::ZERO, per_minute: Some(10), keep: S };
        let mut p = Pacer::default();
        let t0 = Instant::now();
        for _ in 0..10 {
            assert_eq!(p.reserve(rules, t0), Ok(Duration::ZERO));
        }
        // The eleventh would wait the rest of the minute: skipped.
        assert_eq!(p.reserve(rules, t0 + S * 5), Err(S * 55));
        // Near the minute's end, a short wait; after it, at once.
        assert_eq!(p.reserve(rules, t0 + S * 55), Ok(S * 5));
        assert_eq!(p.reserve(rules, t0 + S * 121), Ok(Duration::ZERO));
    }

    #[test]
    fn a_host_that_says_wait_is_left_alone() {
        let rules = Rules { gap: Duration::ZERO, per_minute: None, keep: S };
        let mut p = Pacer::default();
        let t0 = Instant::now();
        p.hold(S * 30, t0);
        assert_eq!(p.reserve(rules, t0 + S * 10), Err(S * 20));
        assert_eq!(p.reserve(rules, t0 + S * 31), Ok(Duration::ZERO));
        // Retry-After, or GitHub's reset once nothing remains.
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("retry-after", "42".parse().unwrap());
        assert_eq!(wait_from(&h, 0), Some(S * 42));
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("x-ratelimit-remaining", "0".parse().unwrap());
        h.insert("x-ratelimit-reset", "1000".parse().unwrap());
        assert_eq!(wait_from(&h, 970), Some(S * 30));
        h.insert("x-ratelimit-remaining", "3".parse().unwrap());
        assert_eq!(wait_from(&h, 970), None);
    }

    #[test]
    fn a_gzipped_answer_is_read() {
        use std::io::Write as _;
        let mut z = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(br#"{"items":[]}"#).unwrap();
        let gz = z.finish().unwrap();
        assert_eq!(unzipped(&gz).unwrap(), r#"{"items":[]}"#);
        assert_eq!(unzipped(b"plain").unwrap(), "plain");
    }

    /// Answers are kept as long as the source allows; a second ask of the
    /// same address never goes out.
    #[tokio::test]
    async fn an_answer_is_kept_and_not_asked_twice() {
        let server = tiny_http::Server::http("127.0.0.1:0").expect("a local port");
        let port = server.server_addr().to_ip().expect("ip").port();
        let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::<(String, Vec<(String, String)>)>::new()));
        let kept = asked.clone();
        std::thread::spawn(move || {
            for req in server.incoming_requests() {
                let headers = req.headers().iter().map(|h| (h.field.to_string().to_ascii_lowercase(), h.value.to_string())).collect();
                kept.lock().unwrap().push((req.url().to_string(), headers));
                let _ = req.respond(tiny_http::Response::from_string("{\"ok\":true}"));
            }
        });
        let ep = Endpoints { exa: String::new(), tavily: String::new(), standin: Some(format!("http://127.0.0.1:{port}/standin")), offline: false };
        let url = Url::parse("https://polite-test.example/api?q=kept+once").unwrap();
        let rules = Rules { gap: Duration::ZERO, per_minute: None, keep: Duration::from_secs(60) };
        let ask = || Ask { purpose: FreeSource::Weather, rules, accept: "application/json", headers: &[("x-test", "1")] };
        assert_eq!(get(&ep, &url, ask()).await.unwrap(), "{\"ok\":true}");
        assert_eq!(get(&ep, &url, ask()).await.unwrap(), "{\"ok\":true}");
        let asked = asked.lock().unwrap().clone();
        assert_eq!(asked.len(), 1, "the second came from the cache");
        assert_eq!(asked[0].0, "/standin/polite-test.example/api?q=kept+once");
        let header = |name: &str| asked[0].1.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
        assert!(header("user-agent").unwrap().starts_with("Yap/"), "Yap's own User-Agent");
        assert_eq!(header("x-test").as_deref(), Some("1"));
        assert_eq!(header("accept").as_deref(), Some("application/json"));
        // Offline (a test run with no stand-in): never out.
        let offline = Endpoints { exa: String::new(), tavily: String::new(), standin: None, offline: true };
        let other = Url::parse("https://polite-test.example/api?q=never").unwrap();
        assert!(get(&offline, &other, ask()).await.is_err());
    }
}

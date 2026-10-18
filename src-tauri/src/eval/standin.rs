//! A scenario's stand-in web (`"web": {"mode": "standin"}`): a local server
//! for `web.rs`'s test-mode addresses — `YAP_EXA_URL` (`POST /exa/search`),
//! `YAP_TAVILY_URL` (`POST /tavily/search`) and `YAP_WEB_STANDIN` (a page
//! `https://host/path` is `GET /web/host/path`) — answering every search
//! with the scenario's `results` and serving its `pages`. Nothing goes on
//! the internet; every request is logged (`web_standin`). Stopped on drop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use super::scenario::WebSpec;

pub struct Standin {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    pub base: String,
}

/// "https://Host/path/" → "host/path" (what a page is looked up by).
fn page_key(url: &str) -> String {
    let rest = url.trim().split_once("://").map_or(url.trim(), |(_, r)| r);
    let rest = rest.split(['?', '#']).next().unwrap_or(rest).trim_end_matches('/');
    match rest.split_once('/') {
        Some((host, path)) => format!("{}/{}", host.to_ascii_lowercase().trim_start_matches("www."), path),
        None => rest.to_ascii_lowercase().trim_start_matches("www.").to_string(),
    }
}

impl Standin {
    pub fn start(spec: &WebSpec) -> Result<Standin, String> {
        let server = tiny_http::Server::http("127.0.0.1:0").map_err(|e| format!("stand-in web: {e}"))?;
        let port = server.server_addr().to_ip().map(|a| a.port()).ok_or("stand-in web: no port")?;
        let base = format!("http://127.0.0.1:{port}");
        let exa = json!({
            "requestId": "eval-standin",
            "results": spec.results.iter().map(|h| json!({
                "id": h.url, "title": h.title, "url": h.url, "publishedDate": h.published, "highlights": [h.text],
            })).collect::<Vec<_>>(),
        });
        let tavily = json!({
            "results": spec.results.iter().map(|h| json!({
                "title": h.title, "url": h.url, "content": h.text, "score": 0.9, "published_date": h.published,
            })).collect::<Vec<_>>(),
        });
        let pages: Vec<(String, String)> = spec.pages.iter().map(|(u, body)| (page_key(u), body.clone())).collect();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = std::thread::spawn(move || {
            while !stopping.load(Ordering::SeqCst) {
                let Ok(Some(mut req)) = server.recv_timeout(Duration::from_millis(100)) else { continue };
                let method = req.method().to_string();
                let path = req.url().to_string();
                let mut body = String::new();
                let _ = req.as_reader().read_to_string(&mut body);
                let (status, kind, text) = if path.starts_with("/exa/search") {
                    (200, "application/json", exa.to_string())
                } else if path.starts_with("/tavily/search") {
                    (200, "application/json", tavily.to_string())
                } else if let Some(rest) = path.strip_prefix("/web/") {
                    let key = page_key(&format!("https://{rest}"));
                    match pages.iter().find(|(k, _)| *k == key) {
                        Some((_, page)) => {
                            let html = page.trim_start().starts_with('<');
                            (200, if html { "text/html; charset=utf-8" } else { "text/plain; charset=utf-8" }, page.clone())
                        }
                        None => (404, "text/plain", "Not found".to_string()),
                    }
                } else {
                    (404, "text/plain", "Not found".to_string())
                };
                let request: Value = serde_json::from_str(&body).unwrap_or_else(|_| json!(body));
                super::record::write("web_standin", json!({ "method": method, "path": path, "body": request, "status": status }));
                let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], kind.as_bytes()).expect("static header");
                let _ = req.respond(tiny_http::Response::from_string(text).with_status_code(status).with_header(header));
            }
        });
        Ok(Standin { stop, thread: Some(thread), base })
    }

    /// The environment `web.rs` reads in test mode.
    pub fn env(&self) -> [(&'static str, String); 3] {
        [
            ("YAP_EXA_URL", format!("{}/exa", self.base)),
            ("YAP_TAVILY_URL", format!("{}/tavily", self.base)),
            ("YAP_WEB_STANDIN", format!("{}/web", self.base)),
        ]
    }
}

impl Drop for Standin {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::page_key;

    #[test]
    fn pages_are_found_as_web_rs_asks_for_them() {
        assert_eq!(page_key("https://tips.example/calc"), "tips.example/calc");
        assert_eq!(page_key("https://www.Tips.example/calc/?x=1#top"), "tips.example/calc");
        assert_eq!(page_key("https://bg3.wiki"), "bg3.wiki");
    }
}

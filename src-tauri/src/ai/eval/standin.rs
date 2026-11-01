//! A scenario's stand-in web (`"web": {"mode": "standin"}`): a local server
//! for `web.rs`'s test-mode addresses — `YAP_EXA_URL` (`POST /exa/search`),
//! `YAP_TAVILY_URL` (`POST /tavily/search`) and `YAP_WEB_STANDIN` (a page
//! `https://host/path` is `GET /web/host/path`) — answering a search with
//! the scenario's `results` (or the `searches` set whose words the query
//! holds) and serving its `pages`. Nothing goes on
//! the internet; every request is logged (`web_standin`). Stopped on drop.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};

use super::scenario::{WebHit, WebSpec};

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
        let exa = |hits: &[WebHit]| {
            json!({
                "requestId": "eval-standin",
                "results": hits.iter().map(|h| json!({
                    "id": h.url, "title": h.title, "url": h.url, "publishedDate": h.published, "highlights": [h.text],
                })).collect::<Vec<_>>(),
            })
        };
        let tavily = |hits: &[WebHit]| {
            json!({
                "results": hits.iter().map(|h| json!({
                    "title": h.title, "url": h.url, "content": h.text, "score": 0.9, "published_date": h.published,
                })).collect::<Vec<_>>(),
            })
        };
        // The search's answer: the first set whose words the query holds,
        // else the scenario's `results`.
        let sets: Vec<(Vec<String>, Vec<WebHit>)> =
            spec.searches.iter().map(|s| (s.when.iter().map(|w| w.to_lowercase()).collect(), s.results.clone())).collect();
        let fallback = spec.results.clone();
        let answer = move |body: &str| -> Vec<WebHit> {
            let query = serde_json::from_str::<Value>(body).ok().and_then(|v| v["query"].as_str().map(str::to_lowercase)).unwrap_or_default();
            sets.iter().find(|(when, _)| when.iter().any(|w| query.contains(w))).map_or_else(|| fallback.clone(), |(_, hits)| hits.clone())
        };
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
                let (status, kind, bytes): (u16, &str, Vec<u8>) = if path.starts_with("/exa/search") {
                    (200, "application/json", exa(&answer(&body)).to_string().into_bytes())
                } else if path.starts_with("/tavily/search") {
                    (200, "application/json", tavily(&answer(&body)).to_string().into_bytes())
                } else if let Some(rest) = path.strip_prefix("/web/") {
                    let key = page_key(&format!("https://{rest}"));
                    match pages.iter().find(|(k, _)| *k == key) {
                        // A stand-in picture ("picture 800x600: WORDS").
                        Some((_, page)) if page.trim_start().starts_with("picture ") => match stand_in_picture(page) {
                            Some(png) => (200, "image/png", png),
                            None => (500, "text/plain", b"Not a picture spec".to_vec()),
                        },
                        Some((_, page)) => {
                            let html = page.trim_start().starts_with('<');
                            (200, if html { "text/html; charset=utf-8" } else { "text/plain; charset=utf-8" }, page.clone().into_bytes())
                        }
                        None => (404, "text/plain", b"Not found".to_vec()),
                    }
                } else {
                    (404, "text/plain", b"Not found".to_vec())
                };
                let request: Value = serde_json::from_str(&body).unwrap_or_else(|_| json!(body));
                super::record::write("web_standin", json!({ "method": method, "path": path, "body": request, "status": status }));
                let header = tiny_http::Header::from_bytes(&b"Content-Type"[..], kind.as_bytes()).expect("static header");
                let _ = req.respond(tiny_http::Response::from_data(bytes).with_status_code(status).with_header(header));
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

/// 5 × 7 letters for [`stand_in_picture`]: each row's five bits, the left
/// one highest.
const GLYPHS: &[(char, [u8; 7])] = &[
    ('A', [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11]),
    ('B', [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E]),
    ('C', [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E]),
    ('D', [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E]),
    ('E', [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F]),
    ('F', [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10]),
    ('G', [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F]),
    ('H', [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11]),
    ('I', [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E]),
    ('J', [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C]),
    ('K', [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11]),
    ('L', [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F]),
    ('M', [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11]),
    ('N', [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11]),
    ('O', [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E]),
    ('P', [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10]),
    ('Q', [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D]),
    ('R', [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11]),
    ('S', [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E]),
    ('T', [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04]),
    ('U', [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E]),
    ('V', [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04]),
    ('W', [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A]),
    ('X', [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11]),
    ('Y', [0x11, 0x11, 0x11, 0x0A, 0x04, 0x04, 0x04]),
    ('Z', [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F]),
    ('0', [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E]),
    ('1', [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E]),
    ('2', [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F]),
    ('3', [0x1F, 0x02, 0x04, 0x02, 0x01, 0x11, 0x0E]),
    ('4', [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02]),
    ('5', [0x1F, 0x10, 0x1E, 0x01, 0x01, 0x11, 0x0E]),
    ('6', [0x06, 0x08, 0x10, 0x1E, 0x11, 0x11, 0x0E]),
    ('7', [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08]),
    ('8', [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E]),
    ('9', [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x02, 0x0C]),
    ('-', [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00]),
    ('.', [0x00, 0x00, 0x00, 0x00, 0x00, 0x0C, 0x0C]),
    (',', [0x00, 0x00, 0x00, 0x00, 0x0C, 0x04, 0x08]),
    (':', [0x00, 0x0C, 0x0C, 0x00, 0x0C, 0x0C, 0x00]),
    ('\'', [0x04, 0x04, 0x08, 0x00, 0x00, 0x00, 0x00]),
];

/// **A stand-in picture** for a scenario's `pages` (`"picture 800x600:
/// TREASURE MAP OF FORT JOY"`): a PNG of that size with its words written
/// large on it, so a model that reads images tells the stand-in pictures
/// apart by what they show, as it would real ones (`look_at_picture`,
/// `chat/pictures.rs`). `None` for a spec it can't read. Pure.
pub fn stand_in_picture(spec: &str) -> Option<Vec<u8>> {
    let rest = spec.trim().strip_prefix("picture ")?;
    let (size, words) = rest.split_once(':')?;
    let (w, h) = size.trim().split_once('x')?;
    let (w, h): (u32, u32) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    if !(16..=4096).contains(&w) || !(16..=4096).contains(&h) {
        return None;
    }
    // Lines of at most `per_line` characters, at a scale that fits them.
    let words: Vec<String> = words.split_whitespace().map(str::to_uppercase).collect();
    let per_line = 18usize;
    let mut lines: Vec<String> = Vec::new();
    for word in words {
        match lines.last_mut() {
            Some(l) if l.chars().count() + 1 + word.chars().count() <= per_line => {
                l.push(' ');
                l.push_str(&word);
            }
            _ => lines.push(word),
        }
    }
    let longest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(1).max(1) as u32;
    let scale = (w * 9 / 10 / (longest * 6)).min(h * 8 / 10 / (lines.len().max(1) as u32 * 9)).max(1);
    let (paper, ink) = ([244u8, 238, 226, 255], [32u8, 32, 40, 255]);
    let mut rgba: Vec<u8> = std::iter::repeat_n(paper, (w * h) as usize).flatten().collect();
    let block = lines.len() as u32 * 9 * scale;
    let top = h.saturating_sub(block) / 2;
    for (li, line) in lines.iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let left = w.saturating_sub(chars.len() as u32 * 6 * scale) / 2;
        for (ci, ch) in chars.iter().enumerate() {
            let Some((_, rows)) = GLYPHS.iter().find(|(g, _)| g == ch) else { continue };
            for (ry, bits) in rows.iter().enumerate() {
                for rx in 0..5u32 {
                    if bits & (0x10 >> rx) == 0 {
                        continue;
                    }
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let x = left + (ci as u32 * 6 + rx) * scale + dx;
                            let y = top + (li as u32 * 9 + ry as u32) * scale + dy;
                            if x < w && y < h {
                                let i = ((y * w + x) * 4) as usize;
                                rgba[i..i + 4].copy_from_slice(&ink);
                            }
                        }
                    }
                }
            }
        }
    }
    crate::snip::images::encode_png(&rgba, w, h, true).ok()
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
    use super::{page_key, stand_in_picture};

    #[test]
    fn pages_are_found_as_web_rs_asks_for_them() {
        assert_eq!(page_key("https://tips.example/calc"), "tips.example/calc");
        assert_eq!(page_key("https://www.Tips.example/calc/?x=1#top"), "tips.example/calc");
        assert_eq!(page_key("https://bg3.wiki"), "bg3.wiki");
    }

    #[test]
    fn a_stand_in_picture_has_its_size_and_its_words_on_it() {
        let png = stand_in_picture("picture 400x300: Treasure map of Fort Joy").unwrap();
        let (rgba, w, h) = crate::snip::images::decode_png(&png).unwrap();
        assert_eq!((w, h), (400, 300));
        // Ink on the paper: the words are drawn.
        let ink = rgba.chunks(4).filter(|p| p[0] < 100).count();
        assert!(ink > 1_000 && ink < (w * h) as usize / 2, "{ink}");
        // Different words, a different picture.
        assert_ne!(png, stand_in_picture("picture 400x300: Concept art").unwrap());
        assert!(stand_in_picture("picture big: x").is_none());
        assert!(stand_in_picture("<html>").is_none());
    }
}

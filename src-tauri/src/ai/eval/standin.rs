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

/// **A stand-in picture** for a scenario's `pages` (`"picture 800x600: Fort
/// Joy treasure map - every chest marked X"`): a PNG of that size that looks
/// like what its words say, its words on a title strip across the top (as a
/// wiki's map carries its title), so a model that reads images judges it as
/// it would a real one (`look_at_picture`, `chat/pictures.rs`). A card with
/// only words on it was refused as "only a title card, no map on it"
/// (Haiku 5.5, 1 Nov 2026), rightly. By its words: a map (sea, an island, a
/// path, buildings; red X marks and an "X = TREASURE" legend when it says
/// treasure or X), a night screenshot, concept art at dusk, an armour set, a
/// portrait; else the words alone. `None` for a spec it can't read. Pure.
pub fn stand_in_picture(spec: &str) -> Option<Vec<u8>> {
    let rest = spec.trim().strip_prefix("picture ")?;
    let (size, words) = rest.split_once(':')?;
    let (w, h) = size.trim().split_once('x')?;
    let (w, h): (u32, u32) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    if !(16..=4096).contains(&w) || !(16..=4096).contains(&h) {
        return None;
    }
    let title = words.trim().to_uppercase();
    let has = |k: &str| title.split(|c: char| !c.is_alphanumeric()).any(|word| word == k);
    let mut c = Canvas::new(w, h, [244, 238, 226, 255]);
    let strip = (h / 8).max(12);
    let (sw, sh) = (w as i32, (h - strip) as i32);
    let top = strip as i32;
    // A point of the scene, from fractions of its width and height.
    let at = |fx: f32, fy: f32| ((fx * sw as f32) as i32, top + (fy * sh as f32) as i32);
    if has("MAP") {
        c.gradient(top, h as i32, [70, 120, 170, 255], [40, 90, 145, 255]);
        let land = [(0.45, 0.55, 0.32, 0.30), (0.30, 0.40, 0.18, 0.20), (0.62, 0.45, 0.20, 0.25), (0.50, 0.75, 0.22, 0.14)];
        for (fx, fy, rx, ry) in land {
            let (x, y) = at(fx, fy);
            c.ellipse(x, y, (rx * sw as f32) as i32 + 8, (ry * sh as f32) as i32 + 8, [214, 196, 150, 255]);
        }
        for (fx, fy, rx, ry) in land {
            let (x, y) = at(fx, fy);
            c.ellipse(x, y, (rx * sw as f32) as i32, (ry * sh as f32) as i32, [120, 150, 90, 255]);
        }
        let path = [at(0.22, 0.35), at(0.35, 0.50), at(0.48, 0.48), at(0.58, 0.62), at(0.70, 0.52)];
        for pair in path.windows(2) {
            c.line(pair[0], pair[1], (w / 160).max(2) as i32, [150, 110, 60, 255]);
        }
        for (fx, fy) in [(0.36, 0.47), (0.55, 0.40), (0.62, 0.58)] {
            let (x, y) = at(fx, fy);
            let s = (w / 50).max(4) as i32;
            c.rect(x - s, y - s, 2 * s, 2 * s, [90, 80, 70, 255]);
        }
        if has("TREASURE") || has("X") {
            for (fx, fy) in [(0.28, 0.42), (0.42, 0.62), (0.52, 0.32), (0.66, 0.50), (0.48, 0.80), (0.74, 0.40)] {
                c.cross(at(fx, fy), (w / 40).max(5) as i32, (w / 200).max(2) as i32, [200, 30, 30, 255]);
            }
            let (lw, lh) = ((w / 4) as i32, (h / 9) as i32);
            let (lx, ly) = (sw - lw - 8, h as i32 - lh - 8);
            c.rect(lx, ly, lw, lh, [244, 238, 226, 255]);
            c.cross((lx + lh / 2, ly + lh / 2), lh / 4, (w / 220).max(2) as i32, [200, 30, 30, 255]);
            c.text("= TREASURE", lx + lh, ly, lw - lh, lh, [32, 32, 40, 255]);
        }
    } else if has("SCREENSHOT") || has("NIGHT") {
        c.gradient(top, h as i32, [16, 20, 48, 255], [40, 44, 80, 255]);
        let (mx, my) = at(0.78, 0.22);
        c.ellipse(mx, my, (w / 18) as i32, (w / 18) as i32, [235, 232, 200, 255]);
        for (fx, fw, fh) in [(0.15, 0.10, 0.45), (0.30, 0.08, 0.60), (0.46, 0.14, 0.38), (0.66, 0.09, 0.52)] {
            let (x, y) = at(fx, 1.0 - fh);
            c.rect(x, y, (fw * sw as f32) as i32, h as i32 - y, [8, 10, 22, 255]);
        }
    } else if has("CONCEPT") || has("ART") {
        c.gradient(top, h as i32, [235, 150, 90, 255], [110, 70, 120, 255]);
        let (x, y) = at(0.35, 0.40);
        c.rect(x, y, sw / 3, h as i32 - y, [60, 40, 60, 255]);
        let (tx, ty) = at(0.42, 0.22);
        c.rect(tx, ty, sw / 14, y - ty, [60, 40, 60, 255]);
        for i in 0..6 {
            let (a, b) = (at(0.05 + 0.15 * i as f32, 0.85), at(0.15 + 0.15 * i as f32, 0.78));
            c.line(a, b, (w / 120).max(2) as i32, [200, 120, 80, 255]);
        }
    } else if has("ARMOUR") || has("ARMOR") || has("PORTRAIT") {
        c.gradient(top, h as i32, [200, 196, 188, 255], [150, 146, 140, 255]);
        let tone = if has("PORTRAIT") { [120, 90, 70, 255] } else { [110, 115, 125, 255] };
        let (hx, hy) = at(0.5, 0.30);
        c.ellipse(hx, hy, sw / 9, sw / 9, tone);
        let (bx, by) = at(0.30, 0.48);
        c.rect(bx, by, (0.40 * sw as f32) as i32, h as i32 - by, tone);
    } else {
        c.text(&title, 0, top, w as i32, h as i32 - top, [32, 32, 40, 255]);
    }
    // The title strip: the picture's words, as its own title.
    c.rect(0, 0, w as i32, strip as i32, [250, 248, 242, 255]);
    c.text(&title, 4, 1, w as i32 - 8, strip as i32 - 2, [32, 32, 40, 255]);
    crate::snip::images::encode_png(&c.px, w, h, true).ok()
}

/// RGBA pixels and the few shapes a stand-in picture needs.
struct Canvas {
    w: i32,
    h: i32,
    px: Vec<u8>,
}

impl Canvas {
    fn new(w: u32, h: u32, fill: [u8; 4]) -> Canvas {
        Canvas { w: w as i32, h: h as i32, px: std::iter::repeat_n(fill, (w * h) as usize).flatten().collect() }
    }

    fn dot(&mut self, x: i32, y: i32, c: [u8; 4]) {
        if (0..self.w).contains(&x) && (0..self.h).contains(&y) {
            let i = ((y * self.w + x) * 4) as usize;
            self.px[i..i + 4].copy_from_slice(&c);
        }
    }

    fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: [u8; 4]) {
        for yy in y.max(0)..(y + h).min(self.h) {
            for xx in x.max(0)..(x + w).min(self.w) {
                self.dot(xx, yy, c);
            }
        }
    }

    fn gradient(&mut self, y0: i32, y1: i32, a: [u8; 4], b: [u8; 4]) {
        for y in y0.max(0)..y1.min(self.h) {
            let t = (y - y0) as f32 / (y1 - y0).max(1) as f32;
            let mix = |i: usize| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t) as u8;
            let c = [mix(0), mix(1), mix(2), 255];
            for x in 0..self.w {
                self.dot(x, y, c);
            }
        }
    }

    fn ellipse(&mut self, cx: i32, cy: i32, rx: i32, ry: i32, c: [u8; 4]) {
        let (rx, ry) = (rx.max(1), ry.max(1));
        for y in (cy - ry)..=(cy + ry) {
            for x in (cx - rx)..=(cx + rx) {
                let (dx, dy) = ((x - cx) as f32 / rx as f32, (y - cy) as f32 / ry as f32);
                if dx * dx + dy * dy <= 1.0 {
                    self.dot(x, y, c);
                }
            }
        }
    }

    fn line(&mut self, a: (i32, i32), b: (i32, i32), thick: i32, c: [u8; 4]) {
        let steps = (b.0 - a.0).abs().max((b.1 - a.1).abs()).max(1);
        for i in 0..=steps {
            let x = a.0 + (b.0 - a.0) * i / steps;
            let y = a.1 + (b.1 - a.1) * i / steps;
            self.rect(x - thick / 2, y - thick / 2, thick, thick, c);
        }
    }

    fn cross(&mut self, (x, y): (i32, i32), r: i32, thick: i32, c: [u8; 4]) {
        self.line((x - r, y - r), (x + r, y + r), thick, c);
        self.line((x - r, y + r), (x + r, y - r), thick, c);
    }

    /// Words in the 5×7 font, wrapped and scaled to fit the box, centred.
    fn text(&mut self, words: &str, x: i32, y: i32, w: i32, h: i32, c: [u8; 4]) {
        let words: Vec<&str> = words.split_whitespace().collect();
        let mut best: (i32, Vec<String>) = (0, Vec::new());
        for per_line in [60usize, 40, 30, 24, 18, 12] {
            let mut lines: Vec<String> = Vec::new();
            for word in &words {
                match lines.last_mut() {
                    Some(l) if l.chars().count() + 1 + word.chars().count() <= per_line => {
                        l.push(' ');
                        l.push_str(word);
                    }
                    _ => lines.push((*word).to_string()),
                }
            }
            let longest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(1).max(1) as i32;
            let scale = (w / (longest * 6)).min(h / (lines.len().max(1) as i32 * 9));
            if scale > best.0 {
                best = (scale, lines);
            }
        }
        let (scale, lines) = best;
        if scale < 1 {
            return;
        }
        let block = lines.len() as i32 * 9 * scale;
        let top = y + (h - block).max(0) / 2;
        for (li, line) in lines.iter().enumerate() {
            let chars: Vec<char> = line.chars().collect();
            let left = x + (w - chars.len() as i32 * 6 * scale).max(0) / 2;
            for (ci, ch) in chars.iter().enumerate() {
                let Some((_, rows)) = GLYPHS.iter().find(|(g, _)| g == ch) else { continue };
                for (ry, bits) in rows.iter().enumerate() {
                    for rx in 0..5i32 {
                        if bits & (0x10 >> rx) != 0 {
                            let gx = left + (ci as i32 * 6 + rx) * scale;
                            let gy = top + (li as i32 * 9 + ry as i32) * scale;
                            self.rect(gx, gy, scale, scale, c);
                        }
                    }
                }
            }
        }
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
    use super::{page_key, stand_in_picture};

    #[test]
    fn pages_are_found_as_web_rs_asks_for_them() {
        assert_eq!(page_key("https://tips.example/calc"), "tips.example/calc");
        assert_eq!(page_key("https://www.Tips.example/calc/?x=1#top"), "tips.example/calc");
        assert_eq!(page_key("https://bg3.wiki"), "bg3.wiki");
    }

    #[test]
    fn a_stand_in_picture_looks_like_what_it_says_and_carries_its_title() {
        let decode = |spec: &str| crate::snip::images::decode_png(&stand_in_picture(spec).unwrap()).unwrap();
        let red = |rgba: &[u8]| rgba.chunks(4).filter(|p| p[0] > 170 && p[1] < 70 && p[2] < 70).count();
        let (map, w, h) = decode("picture 800x600: Fort Joy treasure map - every chest marked X");
        assert_eq!((w, h), (800, 600));
        // A treasure map: its X marks; another area's map: none.
        assert!(red(&map) > 500, "{}", red(&map));
        let (docks, _, _) = decode("picture 800x600: Map of the Saltmarsh Docks");
        assert_eq!(red(&docks), 0);
        // The title strip: dark letters on its light band.
        let strip = (h / 8) as usize;
        let ink = map[..strip * w as usize * 4].chunks(4).filter(|p| p[0] < 100 && p[2] < 100).count();
        assert!(ink > 200, "{ink}");
        // The scene below it isn't the paper of a word card.
        let paper = map[strip * w as usize * 4..].chunks(4).filter(|p| p[..3] == [244, 238, 226]).count();
        assert!(paper < (w * h) as usize / 10, "{paper}");
        // Different words, a different picture; specs it can't read.
        assert_ne!(stand_in_picture("picture 400x300: Concept art").unwrap(), stand_in_picture("picture 400x300: Screenshot at night").unwrap());
        assert!(stand_in_picture("picture 400x300: Warden Aldric portrait").is_some());
        assert!(stand_in_picture("picture big: x").is_none());
        assert!(stand_in_picture("<html>").is_none());
    }
}

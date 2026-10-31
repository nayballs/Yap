//! **The phone's page**, served by Yap itself: the files of
//! `src-tauri/link-page/`, embedded in yap.exe at build time (`build.rs`
//! lists them), at `/` — `index.html` for `/` — each with its content type
//! and a strict Content-Security-Policy: everything from this PC, nothing
//! from anywhere else, and the WebSocket back to the same address only.
//! A build without the page's files serves a short page that says so.

include!(concat!(env!("OUT_DIR"), "/link_page.rs"));

/// A file of the page: its bytes and content type. `path`: the request's,
/// without its query or fragment.
pub fn file(path: &str) -> Option<(&'static [u8], &'static str)> {
    let path = path.split(['?', '#']).next().unwrap_or("");
    let rel = path.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    if rel.contains("..") || rel.contains('\\') {
        return None;
    }
    if let Some((_, bytes)) = FILES.iter().find(|(p, _)| *p == rel) {
        return Some((bytes, content_type(rel)));
    }
    (rel == "index.html").then_some((FALLBACK.as_bytes(), content_type(rel)))
}

/// A file's content type, by its extension. Pure.
pub fn content_type(path: &str) -> &'static str {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "webmanifest" => "application/manifest+json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "ico" => "image/x-icon",
        "webp" => "image/webp",
        "woff2" => "font/woff2",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// The page's Content-Security-Policy, for a page opened at `host` (the
/// request's `Host`, already checked to be an address or a name): its own
/// files only, the WebSocket to the same host, no plug-ins, frames or forms.
pub fn csp(host: &str) -> String {
    format!(
        "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; \
         media-src 'self' blob:; font-src 'self'; connect-src 'self' wss://{host}; worker-src 'self' blob:; \
         manifest-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'"
    )
}

/// What a build without the page's files shows.
const FALLBACK: &str = "<!doctype html><html lang=\"en-GB\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>Yap Link</title></head>\
<body><h1>Yap Link</h1><p>This copy of Yap doesn't have the phone's page yet.</p></body></html>";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_have_their_content_types_and_nothing_outside_is_served() {
        assert_eq!(content_type("index.html"), "text/html; charset=utf-8");
        assert_eq!(content_type("app.JS"), "text/javascript; charset=utf-8");
        assert_eq!(content_type("worklet.mjs"), "text/javascript; charset=utf-8");
        assert_eq!(content_type("style.css"), "text/css; charset=utf-8");
        assert_eq!(content_type("manifest.webmanifest"), "application/manifest+json");
        assert_eq!(content_type("icon.svg"), "image/svg+xml");
        assert_eq!(content_type("noext"), "application/octet-stream");
        // `/` is the page; something outside the folder never is.
        assert!(file("/").is_some());
        assert!(file("/?x=1#pair=abc").is_some());
        assert!(file("/../Cargo.toml").is_none());
        assert!(file("/..%2fCargo.toml").is_none());
        assert!(file("/a\\..\\b").is_none());
        assert!(file("/nope/never-there.js").is_none());
    }

    #[test]
    fn the_phones_page_is_in_the_build() {
        assert!(file("/README.md").is_none(), "the folder's notes aren't the page");
        let (html, kind) = file("/").unwrap();
        assert_eq!(kind, "text/html; charset=utf-8");
        assert!(String::from_utf8_lossy(html).contains("link.js"), "the page itself, not the stand-in");
        for f in ["link.js", "dsp.js", "mic-worklet.js", "player-worklet.js"] {
            assert_eq!(file(&format!("/{f}")).map(|(_, k)| k), Some("text/javascript; charset=utf-8"), "{f}");
        }
        assert_eq!(file("/manifest.webmanifest").map(|(_, k)| k), Some("application/manifest+json"));
    }

    #[test]
    fn the_policy_keeps_the_page_to_this_pc() {
        let p = csp("192.168.1.20:51443");
        assert!(p.contains("default-src 'self'"));
        assert!(p.contains("script-src 'self';"), "no inline or remote scripts");
        assert!(p.contains("connect-src 'self' wss://192.168.1.20:51443;"));
        assert!(p.contains("frame-ancestors 'none'"));
        assert!(!p.contains("http:") && !p.contains("https:"));
    }
}

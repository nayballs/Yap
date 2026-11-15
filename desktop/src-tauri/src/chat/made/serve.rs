//! **The `yapmade` scheme** (`http://yapmade.localhost/…` in WebView2):
//! what Chat made, each as a document of its own in a sandboxed frame.
//!
//! - **`/v/<chat>/<id>`** ([`document`]): a visual — its code in a page of
//!   Yap's (a full document of the model's kept as it is), with the
//!   bridge script first ([`BRIDGE`]: the theme's colours the page sends,
//!   its height back, `sendPrompt`, links) and the base style
//!   ([`BASE_STYLE`]). The response carries the policy ([`csp`]): CSP's own
//!   `sandbox allow-scripts` (an opaque origin, whatever frames it), scripts
//!   inline or from `/lib/` only, styles inline, pictures and fonts from
//!   `data:` and `blob:` only, no connections, frames, workers, plugins,
//!   forms or `<base>`.
//! - **`/p/<chat>/<id>/<n>`** ([`page`]): a page Yap keeps (`pages.rs`), its
//!   version `n` (0: its newest), the same policy and bridge, its own style
//!   ([`PAGE_STYLE`]: it scrolls in the panel's frame).
//! - **`/lib/<file>`** ([`library`]): one of [`LIBS`], the pinned allow-list,
//!   fetched once through `privacy/net.rs` (a chart library for a visual,
//!   in the network receipts) and kept only when its SHA-256 is the one
//!   pinned here. A visual's `<script src>` naming one of their addresses is
//!   pointed here ([`point_libraries`]); any other is refused when it's made.
//! - **Anything else**: 404.

use sha2::{Digest, Sha256};

/// The scheme's name (`lib.rs` registers it).
pub const SCHEME: &str = "yapmade";

/// **A library a visual may load**: its file here, the addresses a model
/// writes for it (the first is fetched), and its pinned SHA-256.
#[derive(Debug, Clone, Copy)]
pub struct Lib {
    pub name: &'static str,
    pub file: &'static str,
    pub urls: &'static [&'static str],
    pub sha256: &'static str,
}

/// **The allow-list**: exact versions, checked by their hash.
pub const LIBS: &[Lib] = &[
    Lib {
        name: "Chart.js 4.4.1",
        file: "chart.umd.min.js",
        urls: &[
            "https://cdnjs.cloudflare.com/ajax/libs/Chart.js/4.4.1/chart.umd.min.js",
            "https://cdnjs.cloudflare.com/ajax/libs/Chart.js/4.4.1/chart.umd.js",
            "https://cdn.jsdelivr.net/npm/chart.js@4.4.1/dist/chart.umd.min.js",
            "https://cdn.jsdelivr.net/npm/chart.js@4.4.1/dist/chart.umd.js",
            "https://cdn.jsdelivr.net/npm/chart.js@4.4.1",
            "https://unpkg.com/chart.js@4.4.1/dist/chart.umd.js",
        ],
        sha256: "81ffafe13c37e1b25793b020d446f4d9739b949dadb7f9f79d709a0cad781c2f",
    },
    Lib {
        name: "D3 7.9.0",
        file: "d3.min.js",
        urls: &[
            "https://cdnjs.cloudflare.com/ajax/libs/d3/7.9.0/d3.min.js",
            "https://cdn.jsdelivr.net/npm/d3@7.9.0/dist/d3.min.js",
            "https://cdn.jsdelivr.net/npm/d3@7.9.0",
            "https://unpkg.com/d3@7.9.0/dist/d3.min.js",
            "https://d3js.org/d3.v7.min.js",
        ],
        sha256: "f2094bbf6141b359722c4fe454eb6c4b0f0e42cc10cc7af921fc158fceb86539",
    },
];

/// The library a `<script src>` names: one of [`LIBS`]' addresses (http or
/// https, any case), or its file under `/lib/`. Pure.
pub fn lib_for(src: &str) -> Option<&'static Lib> {
    let s = src.trim();
    let plain = s.strip_prefix("http://").map(|r| format!("https://{r}")).unwrap_or_else(|| s.to_string());
    LIBS.iter().find(|l| l.urls.iter().any(|u| u.eq_ignore_ascii_case(&plain)) || s == format!("/lib/{}", l.file))
}

/// **The policy a visual's document carries** (`origin`: this scheme's, as
/// the request came: `http://yapmade.localhost`). Pure.
pub fn csp(origin: &str) -> String {
    [
        "default-src 'none'".to_string(),
        format!("script-src 'unsafe-inline' {origin}/lib/"),
        "style-src 'unsafe-inline'".into(),
        "img-src data: blob:".into(),
        "font-src data:".into(),
        "media-src data: blob:".into(),
        "connect-src 'none'".into(),
        "frame-src 'none'".into(),
        "child-src 'none'".into(),
        "worker-src 'none'".into(),
        "object-src 'none'".into(),
        "manifest-src 'none'".into(),
        "base-uri 'none'".into(),
        "form-action 'none'".into(),
        "sandbox allow-scripts".into(),
    ]
    .join("; ")
}

/// **The base style**: the page's font, text colour and controls in Yap's
/// tokens (sent by the page, [`BRIDGE`]), a transparent background, an SVG
/// fitted to the width; hidden until the colours came (a moment at most).
pub const BASE_STYLE: &str = r#"
:root{color-scheme:light dark;--text:#23211b;--text-2:rgba(35,33,27,.64);--muted:#6e6a5f;--bg:#faf9f6;--surface:#fff;--surface-2:#f2efe9;--border:#dcd7cb;--accent:#c2690a;--accent-text:#fff;--good:#1e8f4d;--warn:#b07c10;--bad:#c23b32;--font:system-ui,'Segoe UI',sans-serif;--mono:ui-monospace,'Cascadia Code',Consolas,monospace}
html:not(.yap-themed) body{visibility:hidden}
html,body{margin:0;padding:0;background:transparent}
body{font:14px/1.5 var(--font);color:var(--text);overflow-x:auto;-webkit-font-smoothing:antialiased}
#yap-visual{padding:2px}
#yap-visual>svg,#yap-visual.svg svg{display:block;max-width:100%;height:auto;margin:0 auto}
button,input,select,textarea{font:inherit;color:inherit}
button{background:var(--surface);border:1px solid var(--border);border-radius:8px;padding:6px 12px;cursor:pointer}
button:hover{background:var(--surface-2)}
input[type=text],input[type=number],select,textarea{background:var(--surface);border:1px solid var(--border);border-radius:6px;padding:5px 8px}
input[type=range],input[type=checkbox],input[type=radio]{accent-color:var(--accent)}
a{color:var(--accent)}
code,pre{font-family:var(--mono)}
"#;

/// **The bridge** (first in the document, before the model's code): what
/// WebView2 puts in every frame of Yap's window for Tauri — its internals,
/// `ipc`, `chrome.webview` — taken away first (the two that can't be deleted
/// emptied and frozen: no `invoke`, no `postMessage`; none of it reaches Yap from
/// here anyway: the policy blocks Tauri's IPC requests, and WebView2 gives a
/// frame's messages to no one; e2e `chat-visuals.spec.js` tries both), then
/// Yap's colours from the page (only `--name` keys, string values), the
/// document's height back to it, `sendPrompt(text)` (text only, at most
/// 4,000 characters), and a link's address on a click (never followed
/// here). Every message to the page carries `yapVisual: 1`.
pub const BRIDGE: &str = r#"(()=>{
for(const k of ['__TAURI_INTERNALS__','__TAURI_INVOKE_KEY__','__TAURI_IPC__','__TAURI_POST_MESSAGE__','__TAURI_METADATA__','__TAURI_EVENT_PLUGIN_INTERNALS__','__TAURI__','ipc']){const o=window[k];try{delete window[k]}catch(e){}if(o&&typeof o==='object'&&k in window){for(const m of Object.getOwnPropertyNames(o)){try{o[m]=undefined}catch(e){}try{delete o[m]}catch(e){}}try{Object.freeze(o)}catch(e){}}}
try{if(window.chrome&&window.chrome.webview){delete window.chrome.webview;if(window.chrome.webview)Object.defineProperty(window.chrome,'webview',{value:undefined,writable:false,configurable:false})}}catch(e){}
const post=(m)=>{try{parent.postMessage(Object.assign({yapVisual:1},m),'*')}catch(e){}};
const root=document.documentElement;
let last=-1;
const size=()=>{const b=document.body;if(!b)return;const h=Math.ceil(Math.max(b.scrollHeight,b.getBoundingClientRect().height));if(h!==last){last=h;post({op:'size',h})}};
const themed=()=>{root.classList.add('yap-themed');size()};
addEventListener('message',(e)=>{if(e.source!==parent)return;const d=e.data;if(!d||d.yapVisual!==1||d.op!=='theme')return;const vars=d.vars&&typeof d.vars==='object'?d.vars:{};for(const k of Object.keys(vars)){const v=vars[k];if(/^--[a-z0-9-]{1,40}$/.test(k)&&typeof v==='string'&&v.length<200)root.style.setProperty(k,v)}root.style.colorScheme=d.scheme==='dark'?'dark':'light';root.dataset.theme=d.scheme==='dark'?'dark':'light';themed()});
window.sendPrompt=(text)=>{const t=String(text==null?'':text).slice(0,4000);if(t.trim())post({op:'prompt',text:t})};
document.addEventListener('click',(e)=>{const a=e.target&&e.target.closest?e.target.closest('a[href]'):null;if(!a)return;const href=a.getAttribute('href')||'';if(href.startsWith('#'))return;e.preventDefault();if(/^https?:\/\//i.test(a.href))post({op:'link',href:a.href})},true);
addEventListener('DOMContentLoaded',()=>{size();try{new ResizeObserver(size).observe(document.body)}catch(e){}});
addEventListener('load',size);
setTimeout(themed,400);
post({op:'ready'});
})();"#;

/// **A visual's own style**, after the base: it never scrolls (its frame
/// takes its height).
pub const VISUAL_STYLE: &str = "body{overflow-y:hidden}";

/// **A page's own style**, after the base: it scrolls in the panel's frame,
/// with room round a fragment.
pub const PAGE_STYLE: &str = "html,body{min-height:100%}body{overflow:auto}#yap-page{padding:20px 24px}";

/// What a document is: a visual in the answer, or a page beside the chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Visual,
    Page,
}

/// The bridge and the base style, as the head's first children.
fn head_parts(kind: Kind) -> String {
    let own = if kind == Kind::Page { PAGE_STYLE } else { VISUAL_STYLE };
    format!("<meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><style>{BASE_STYLE}{own}</style><script>{BRIDGE}</script>")
}

/// A visual's `<script src>`s on the allow-list pointed at `/lib/`. Pure.
pub fn point_libraries(code: &str) -> String {
    let mut out = code.to_string();
    for src in crate::chat::made::script_sources(code) {
        if let Some(lib) = lib_for(&src) {
            out = out.replace(&src, &format!("/lib/{}", lib.file));
        }
    }
    out
}

/// **A visual's document**: `code` (SVG or an HTML fragment) in Yap's page,
/// or — a whole document of the model's — that document with the bridge
/// and base style first in its head. Pure.
pub fn document(code: &str, format: &str) -> String {
    compose(code, format, Kind::Visual)
}

/// **A page's document** ([`document`], a page's style: it scrolls). Pure.
pub fn page_document(html: &str) -> String {
    compose(html, "html", Kind::Page)
}

fn compose(code: &str, format: &str, kind: Kind) -> String {
    let code = point_libraries(code);
    let lower = code.to_ascii_lowercase();
    let whole = lower.contains("<html") || lower.trim_start().starts_with("<!doctype");
    if whole {
        let at = lower.find("<head").and_then(|i| lower[i..].find('>').map(|j| i + j + 1)).or_else(|| lower.find("<html").and_then(|i| lower[i..].find('>').map(|j| i + j + 1)));
        return match at {
            Some(i) => format!("{}{}{}", &code[..i], head_parts(kind), &code[i..]),
            None => format!("{}{code}", head_parts(kind)),
        };
    }
    if kind == Kind::Page {
        return format!("<!doctype html><html lang=\"en\"><head>{}</head><body><main id=\"yap-page\">{code}</main></body></html>", head_parts(kind));
    }
    let class = if format == "svg" { " class=\"svg\"" } else { "" };
    format!("<!doctype html><html lang=\"en\"><head>{}</head><body><div id=\"yap-visual\"{class}>{code}</div></body></html>", head_parts(kind))
}

type Response = tauri::http::Response<Vec<u8>>;

fn respond(status: u16, kind: &str) -> tauri::http::response::Builder {
    tauri::http::Response::builder()
        .status(status)
        .header("Content-Type", kind)
        .header("X-Content-Type-Options", "nosniff")
        .header("Referrer-Policy", "no-referrer")
}

fn not_found() -> Response {
    respond(404, "text/plain").header("Cache-Control", "no-store").body(b"not found".to_vec()).unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
}

/// This scheme's origin as the request came (`http://yapmade.localhost`).
fn origin_of(uri: &tauri::http::Uri) -> String {
    format!("{}://{}", uri.scheme_str().unwrap_or("http"), uri.authority().map(|a| a.as_str()).unwrap_or("yapmade.localhost"))
}

/// **Answer a request** (`lib.rs`, off the main thread).
pub async fn serve(request: &tauri::http::Request<Vec<u8>>) -> Response {
    let uri = request.uri();
    // (`convertFileSrc` sends its `/`s as `%2F`.)
    let path = uri.path().trim_start_matches('/').replace("%2F", "/").replace("%2f", "/");
    let parts: Vec<&str> = path.split('/').collect();
    match parts.as_slice() {
        ["v", conv, id] => {
            let Ok(conv) = conv.parse::<u64>() else { return not_found() };
            visual(conv, id, &origin_of(uri))
        }
        ["p", conv, id, n] => match (conv.parse::<u64>(), n.parse::<u32>()) {
            (Ok(conv), Ok(n)) => page(conv, id, n, &origin_of(uri)),
            _ => not_found(),
        },
        ["lib", file] => library(file).await,
        _ => not_found(),
    }
}

/// `/v/<chat>/<id>`: a visual's document, with its policy.
pub fn visual(conv: u64, id: &str, origin: &str) -> Response {
    let Some((made, code)) = crate::chat::made::code_of(conv, id) else { return not_found() };
    let body = document(&code, &made.format).into_bytes();
    respond(200, "text/html; charset=utf-8")
        .header("Content-Security-Policy", csp(origin))
        .header("Cache-Control", "no-store")
        .body(body)
        .unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
}

/// `/p/<chat>/<id>/<n>`: a page's version `n` (0: its newest), with its policy.
pub fn page(conv: u64, id: &str, n: u32, origin: &str) -> Response {
    let Some((_, _, html)) = crate::chat::made::page_of(conv, id, n) else { return not_found() };
    respond(200, "text/html; charset=utf-8")
        .header("Content-Security-Policy", csp(origin))
        .header("Cache-Control", "no-store")
        .body(page_document(&html).into_bytes())
        .unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
}

/// Where the libraries are kept once fetched.
fn lib_dir() -> std::path::PathBuf {
    crate::chat::made::dir().join("lib")
}

/// Whether `bytes` are `lib`'s, by its pinned hash. Pure.
pub fn verified(lib: &Lib, bytes: &[u8]) -> bool {
    let hash = Sha256::digest(bytes);
    let hex: String = hash.iter().map(|b| format!("{b:02x}")).collect();
    hex == lib.sha256
}

/// One fetch at a time (two visuals asking for Chart.js at once fetch it once).
static FETCHING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// `/lib/<file>`: a library of the allow-list, from its copy here or fetched once.
async fn library(file: &str) -> Response {
    let Some(lib) = LIBS.iter().find(|l| l.file == file) else { return not_found() };
    let kept = lib_dir().join(lib.file);
    let _one = FETCHING.lock().await;
    let bytes = match std::fs::read(&kept) {
        Ok(b) if verified(lib, &b) => Some(b),
        _ => fetch(lib).await.inspect(|b| {
            let _ = std::fs::create_dir_all(lib_dir());
            if let Err(e) = crate::config::atomic_write_bytes(&kept, b) {
                tracing::warn!(kind = ?e.kind(), "Chat: a visual's library couldn't be kept; fetched again next time");
            }
        }),
    };
    match bytes {
        Some(b) => respond(200, "text/javascript; charset=utf-8")
            .header("Cache-Control", "max-age=31536000, immutable")
            .body(b)
            .unwrap_or_else(|_| tauri::http::Response::new(Vec::new())),
        None => respond(502, "text/javascript")
            .header("Cache-Control", "no-store")
            .body(format!("/* {} couldn't be fetched */", lib.name).into_bytes())
            .unwrap_or_else(|_| tauri::http::Response::new(Vec::new())),
    }
}

/// `lib` from its first address, through the network receipts: its bytes
/// when their hash is the pinned one.
async fn fetch(lib: &Lib) -> Option<Vec<u8>> {
    use crate::privacy::net::Recorded;
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build().ok()?;
    let res = client.get(lib.urls[0]).send_for(crate::privacy::net::Purpose::VisualLibrary).await.ok()?;
    if !res.status().is_success() {
        tracing::warn!(status = res.status().as_u16(), "Chat: a visual's library couldn't be fetched");
        return None;
    }
    let bytes = res.bytes().await.ok()?.to_vec();
    if !verified(lib, &bytes) {
        tracing::warn!(bytes = bytes.len(), "Chat: a visual's library wasn't the one pinned; not used");
        return None;
    }
    tracing::info!(bytes = bytes.len(), "Chat: a visual's library fetched and checked");
    Some(bytes)
}

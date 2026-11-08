//! **The page pane**: a live web page beside the chat — the panel's Page
//! tab (`browser_navigate`, `chat/pages.rs`) — and what the model sees of it
//! (`browser_screenshot`: [`shot_in_view`]).
//!
//! **Its own webview, not Yap's.** Each page is a WebView2 controller of its
//! own, made here with WebView2's API (webview2-com), not by Tauri: Tauri
//! puts its IPC (`window.__TAURI_INTERNALS__`, the invoke key) into every
//! webview it makes, and a remote page must never have any of it. So:
//! - **no Tauri IPC at all**: nothing of Tauri's is in the page — no
//!   initialization script, no `ipc://` or `tauri://` handler, web messages
//!   and host objects off (`IsWebMessageEnabled`, `AreHostObjectsAllowed`);
//! - **its own profile**: the controller is made on Yap's WebView2
//!   environment (the main window's, `ICoreWebView2_2::Environment`) with a
//!   profile of its own ([`PROFILE`]): none of Yap's cookies, storage or
//!   cache; the person's sign-ins there stay there;
//! - **only public http(s)**: every navigation, a frame's too, is checked
//!   as a page read is (`web::check_url`) and cancelled otherwise; every
//!   request a page makes is checked too ([`refused_request`],
//!   `WebResourceRequested` on all of them, its shared and service workers'
//!   too): one to an address on this PC or its network (loopback, private,
//!   link-local, unique-local… however it's written, `localhost`, `.local`,
//!   `.localhost`) gets a 403. **Not caught**: a public name that resolves
//!   to such an address (DNS rebinding), and a WebSocket (WebView2 never
//!   shows Yap one's request) — so Yap's own helpers on this PC never rely
//!   on this alone: each needs a secret a page can't know (the on-device
//!   model's and the search model's llama.cpp servers a key per launch,
//!   `ai/local_llm/key.rs`, a 401 for all but `/health` without it; the
//!   Local API its token; Yap Link its pairing; the sign-in listeners
//!   their one-time state);
//! - **its dialogs are never WebView2's own** (`ScriptDialogOpening`): on a
//!   page Yap loads for itself (a shot's), a page out of sight or one Yap is
//!   acting on ([`busy`]) they're dismissed; on one the person is using, its
//!   tab's strip asks (`yap-page-dialog`, `page_view_dialog`);
//! - **Yap's scripts give up after [`SCRIPT_TIMEOUT`]** (a page stuck in a
//!   loop never stalls a turn);
//! - **a new window opens in its own tab** (only when the person clicked;
//!   a mail link in their mail app), **downloads are off**, every permission a
//!   page asks for (the mic, the camera, location, notifications…) is
//!   denied; DevTools only in a debug build.
//!
//! **Where it sits.** A child window of Yap's main window ("the host",
//! [`win`]), over the Page tab's stage: the page tells Rust the stage's
//! place in physical pixels ([`Bounds`], `page_view_place`) whenever it
//! moves, and hides it (`None`) whenever anything of Yap's covers it (a
//! menu, a dialog, Settings) or the tab isn't showing — so the page keeps
//! its place (scroll, a map's zoom) while another tab shows, and closes
//! with its tab (`page_view_close`). At most [`MAX_VIEWS`] stay alive; the
//! least recently shown goes first.
//!
//! **Its menu** (a right-click on the page): Back, Forward, Reload, Copy,
//! Copy link — and on selected text, **Ask Yap about this** (the words to
//! Chat's box as a quote, [`QUOTE_EVENT`]; asked as a selection in another
//! app is, `assistant/selected.rs`, the site named) and **Read aloud**
//! (`voice/read_aloud.rs`); **Snip to Chat** always (`snip.rs`).
//!
//! **A shot** (`page_view_shot`, the tab's Save a shot; [`shot_in_view`],
//! the model's `browser_screenshot`): `ICoreWebView2::CapturePreview` as a
//! PNG of what's in view at its scroll — on screen as it is, out of sight
//! drawn a moment under Yap's own page at the size the panel last had.
//!
//! **Test mode** loads a page from the web's stand-in (`YAP_WEB_STANDIN`,
//! `web.rs`: `https://host/path` from `<standin>/host/path`), never the
//! internet, and shows no native menu.
//!
//! **Privacy**: the log never has an address (a page's key holds one) or a
//! page's words — counts and outcomes only.

use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::chat::assistant::selected::Selected;

static APP: OnceLock<AppHandle> = OnceLock::new();

/// Keep the app for the model's browser tools (they run without one).
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// The app, for a tool that runs without one (`look_at_screen`).
pub fn app() -> Option<AppHandle> {
    APP.get().cloned()
}

/// Yap's window is there to hold pages (not in a model evaluation or a unit
/// test).
pub fn ready() -> bool {
    APP.get().is_some()
}

/// The event a page's state goes out on (to the main window).
pub const EVENT: &str = "yap-page-view";
/// The event "Ask Yap about this" goes out on.
pub const QUOTE_EVENT: &str = "yap-page-quote";
/// The WebView2 profile every page is in: its own cookies and storage.
pub const PROFILE: &str = "YapPages";
/// Pages kept alive at once.
pub const MAX_VIEWS: usize = 4;
/// The size a page out of sight is laid out at before any has been on
/// screen (physical pixels).
pub const SNAP_W: i32 = 1280;
pub const SNAP_H: i32 = 800;
/// The time a page loaded for a shot gets after it loads, for its pictures
/// and scripts…
const SETTLE: Duration = Duration::from_millis(1200);
/// …how long a shot may take…
const SHOT_TIMEOUT: Duration = Duration::from_secs(10);
/// …and the moment a page out of sight is given to draw before its shot.
const DRAW: Duration = Duration::from_millis(350);
/// How long one of Yap's scripts in a page may take (a page stuck in a loop).
pub const SCRIPT_TIMEOUT: Duration = Duration::from_secs(10);
/// A dialog a click opens comes a moment after it: still Yap's to dismiss.
const DIALOG_GRACE: Duration = Duration::from_millis(1500);
/// The event a page's dialog goes out on (its tab's strip asks).
pub const DIALOG_EVENT: &str = "yap-page-dialog";

/// **Tab numbers**: each page tab's own for the session — never another's,
/// never used again once it closes (a tab closed to make room and opened
/// again is a new tab), so a model holding "tab 3" never acts on another
/// page.
static NEXT_TAB: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);

/// The next tab number ([`NEXT_TAB`]).
fn next_tab() -> usize {
    NEXT_TAB.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Where a page sits over Yap's main window: physical pixels, from the top
/// left of its client area (where Yap's own page starts).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// What a page is doing, for its tab (`yap-page-view`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewState {
    pub key: String,
    /// Where it is now (after links and redirects).
    pub url: String,
    pub title: String,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    pub loading: bool,
    /// It didn't load: why, in a few words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The desktop width it's laid out at (CSS pixels), shown scaled to fit:
    /// its tab's "Desktop width ×" chip, which puts it back ([`page_view_width`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
}

/// The least a page is zoomed out to fit a desktop width in the panel
/// (WebView2's own floor): a panel too narrow for the width asked lays the
/// page out as wide as this allows.
pub const ZOOM_MIN: f64 = 0.25;

/// **The zoom that lays a page out `width` CSS pixels wide** in a view
/// `phys_w` device pixels wide at `raster` device pixels a CSS pixel (the
/// monitor's scale): the panel shows the desktop layout scaled to fit —
/// what the person sees is what the model reads, smaller. Never above 1 (a
/// width under the panel's is the panel's own), never under [`ZOOM_MIN`].
/// Pure.
pub fn zoom_for(phys_w: i32, raster: f64, width: u32) -> f64 {
    let raster = if raster.is_finite() && raster > 0.0 { raster } else { 1.0 };
    if phys_w <= 0 || width == 0 {
        return 1.0;
    }
    (f64::from(phys_w) / (raster * f64::from(width))).clamp(ZOOM_MIN, 1.0)
}

/// The width (CSS pixels) a view `phys_w` device pixels wide lays a page
/// out at, at `raster` and `zoom` ([`zoom_for`]'s). Pure.
#[cfg(test)]
pub fn laid_out_width(phys_w: i32, raster: f64, zoom: f64) -> f64 {
    let scale = raster.max(f64::MIN_POSITIVE) * zoom.max(f64::MIN_POSITIVE);
    f64::from(phys_w.max(0)) / scale
}

/// Text selected on a page beside the chat and asked about (`chat_send`'s
/// `pageQuote`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PageQuote {
    pub text: String,
    /// The page's site ("wiki.example.com"): "Selected in wiki.example.com".
    #[serde(default)]
    pub site: String,
}

impl PageQuote {
    /// As text selected in another app (`assistant/selected.rs`): the words
    /// and the site — web content (`Selected::web`: a selection can hold
    /// words the person never saw). `None`: nothing selected.
    pub fn selected(self) -> Option<Selected> {
        let text = self.text.trim();
        (!text.is_empty()).then(|| Selected { text: text.to_string(), app: self.site.trim().to_string(), web: true })
    }
}

// ---- the rules (pure) ---------------------------------------------------------------------

/// **The address a page loads from**: itself — a public http(s) address
/// (`web::check_url`) — or, in a test run, the web's stand-in for it
/// (`standin`: `<standin>/host/path?query`; none: never the internet,
/// `offline`). Pure.
pub fn load_url(url: &str, standin: Option<&str>, offline: bool) -> Result<String, String> {
    let u = crate::web::check_url(url)?;
    match standin {
        Some(base) => {
            let query = u.query().map(|q| format!("?{q}")).unwrap_or_default();
            Ok(format!("{base}/{}{}{query}", u.host_str().unwrap_or(""), u.path()))
        }
        None if offline => Err("Test runs never go on the internet.".into()),
        None => Ok(u.to_string()),
    }
}

/// **May a page go to `uri`** (a link, a redirect, a frame): a public
/// http(s) address — in a test run, only the stand-in's. Pure.
pub fn may_navigate(uri: &str, standin: Option<&str>, offline: bool) -> bool {
    match standin {
        Some(base) => uri.starts_with(&format!("{base}/")),
        None => !offline && crate::web::check_url(uri).is_ok(),
    }
}

/// **A request a page may not make** (any: a picture's, a script's, a
/// `fetch`): one to this PC or its network — a host that's an IP address in
/// a loopback, private, link-local, unique-local, shared or other
/// non-public range (`web::is_public`: an IPv4-mapped IPv6 address and the
/// odd ways of writing one too, as a URL parser reads them: `127.0.0.2`,
/// `[::ffff:7f00:1]`, `2130706433`), or a local name (`localhost`,
/// `*.localhost`, `*.local`, one with no dot…). A test run's stand-in is the
/// page itself. A public name that resolves to such an address (DNS
/// rebinding) isn't caught here. Pure.
pub fn refused_request(uri: &str, standin: Option<&str>) -> bool {
    if standin.is_some_and(|base| uri.starts_with(&format!("{base}/"))) {
        return false;
    }
    let Ok(u) = reqwest::Url::parse(uri.trim()) else { return false };
    if !matches!(u.scheme(), "http" | "https" | "ws" | "wss" | "ftp") {
        return false;
    }
    match u.host() {
        Some(url::Host::Ipv4(ip)) => !crate::web::is_public(std::net::IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => !crate::web::is_public(std::net::IpAddr::V6(ip)),
        Some(url::Host::Domain(d)) => crate::web::is_local_name(d),
        None => false,
    }
}

/// Pages Yap is acting on now: how many actions, and until when after the
/// last one ended ([`DIALOG_GRACE`]).
static ACTING: std::sync::Mutex<Option<std::collections::HashMap<String, Acting>>> = std::sync::Mutex::new(None);

/// A page Yap acts on: actions under way, and until when after the last.
type Acting = (u32, Option<std::time::Instant>);

/// **Yap is acting on page `key`** while this lives (and a moment after):
/// its dialogs are dismissed ([`acting_on`]).
pub struct Busy(String);

impl Drop for Busy {
    fn drop(&mut self) {
        let mut acting = ACTING.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(e) = acting.get_or_insert_with(Default::default).get_mut(&self.0) {
            e.0 = e.0.saturating_sub(1);
            e.1 = Some(std::time::Instant::now() + DIALOG_GRACE);
        }
    }
}

/// **Yap starts acting on page `key`** (an action of the drive tier): a
/// dialog waiting there for the person goes (it would stall Yap's script),
/// and its dialogs are dismissed until the [`Busy`] goes.
pub async fn busy(key: &str) -> Busy {
    ACTING.lock().unwrap_or_else(|p| p.into_inner()).get_or_insert_with(Default::default).entry(key.to_string()).or_insert((0, None)).0 += 1;
    if let Some(app) = APP.get().cloned() {
        imp::dismiss_dialog(app, key.to_string()).await;
    }
    Busy(key.to_string())
}

/// Whether Yap is acting on page `key` now, or just was. Pure but for the
/// clock.
pub fn acting_on(key: &str) -> bool {
    let acting = ACTING.lock().unwrap_or_else(|p| p.into_inner());
    acting.as_ref().and_then(|m| m.get(key)).is_some_and(|(n, until)| *n > 0 || until.is_some_and(|u| std::time::Instant::now() < u))
}

/// **What the address bar's words open** (a page's tab, or a new tab's): an
/// address as typed — with `https://` when it has none ("bg3.wiki/wiki/
/// Gale") —, else a search on the person's engine (`web_search_browser`,
/// as "Search in your browser" has it). Never another scheme ("file:",
/// "javascript:"…); whether the address may open is [`load_url`]'s (public
/// web pages only). Pure.
pub fn address(typed: &str, engine: &str) -> Result<String, String> {
    let t = typed.trim();
    if t.is_empty() {
        return Err("Type an address, or words to search for.".into());
    }
    let lower = t.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Ok(t.to_string());
    }
    let spaced = t.contains(char::is_whitespace);
    // "javascript:…", "file:…", "about:…": a scheme, not a site ("site.com:8080" is one).
    if let Some((head, rest)) = t.split_once(':') {
        let scheme = head.starts_with(|c: char| c.is_ascii_alphabetic()) && head.chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c)) && !head.contains('.');
        let port_part: String = rest.chars().take_while(|c| !"/?#".contains(*c)).collect();
        let port = !port_part.is_empty() && port_part.chars().all(|c| c.is_ascii_digit());
        if scheme && !port && !spaced {
            return Err("Only web pages open here: an address, or words to search for.".into());
        }
    }
    let host = t.split(['/', '?', '#']).next().unwrap_or("");
    let host = host.rsplit_once(':').filter(|(_, p)| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit())).map_or(host, |(h, _)| h);
    let labels: Vec<&str> = host.split('.').collect();
    let named = labels.len() >= 2
        && labels.iter().all(|l| !l.is_empty() && !l.starts_with('-') && l.chars().all(|c| c.is_alphanumeric() || c == '-'))
        && labels.last().is_some_and(|tld| tld.chars().count() >= 2 && tld.chars().all(char::is_alphabetic));
    let ip = labels.len() == 4 && labels.iter().all(|l| !l.is_empty() && l.len() <= 3 && l.chars().all(|c| c.is_ascii_digit()));
    if !spaced && (named || ip) {
        return Ok(format!("https://{t}"));
    }
    Ok(crate::web::BrowserSearch::new(engine, t).url)
}

/// The address a tab shows for `loaded` (a test run's stand-in address as
/// the page's own). Pure.
pub fn shown_url(loaded: &str, standin: Option<&str>) -> String {
    match standin.and_then(|base| loaded.strip_prefix(&format!("{base}/"))) {
        Some(rest) => format!("https://{rest}"),
        None => loaded.to_string(),
    }
}

/// **The script that scrolls a page to `section`**: the element with that
/// id (as written, with "_" or "-" for spaces), else the first heading that
/// has its words, else the first place the words are (`window.find`). Run
/// in the page, by Yap; it reads nothing back but which it found. Pure.
pub fn section_script(section: &str) -> String {
    let s = serde_json::to_string(section.trim()).unwrap_or_else(|_| "\"\"".into());
    format!(
        r#"(function(s){{if(!s)return'none';var n=function(t){{return(t||'').replace(/\s+/g,' ').trim().toLowerCase();}};var w=n(s);var el=document.getElementById(s)||document.getElementById(s.replace(/ /g,'_'))||document.getElementById(s.replace(/ /g,'-').toLowerCase());if(!el){{var hs=document.querySelectorAll('h1,h2,h3,h4,h5,h6,summary,dt,caption,legend,[role=heading]');for(var i=0;i<hs.length;i++){{if(n(hs[i].textContent).indexOf(w)>=0){{el=hs[i];break;}}}}}}if(el){{el.scrollIntoView({{block:'start'}});return'heading';}}if(window.find&&window.find(s))return'text';return'none';}})({s})"#
    )
}

/// **Which page goes** when one more must be kept alive: the least
/// recently shown that isn't on screen (`views`: key, when last shown,
/// whether on screen). `None`: there's room. Pure.
pub fn evict(views: &[(String, u64, bool)]) -> Option<String> {
    if views.len() < MAX_VIEWS {
        return None;
    }
    views.iter().filter(|(_, _, on)| !on).min_by_key(|(_, used, _)| *used).or_else(|| views.iter().min_by_key(|(_, used, _)| *used)).map(|(k, _, _)| k.clone())
}

/// The page's menu items kept (WebView2's names, `ICoreWebView2ContextMenuItem::
/// Name`): moving about, copying and the editing commands — never Save as,
/// Print, Open in a new window, Inspect or the like.
pub const MENU_KEEP: &[&str] = &[
    "back", "forward", "reload", "copy", "copyLinkLocation", "copyImage", "copyImageLocation", "selectAll", "cut", "paste",
    "pasteAndMatchStyle", "undo", "redo", "emoji",
];

/// **The menu's items kept**, by index (`items`: each one's name and
/// whether it's a separator): [`MENU_KEEP`]'s, with the separators that
/// still separate something. Pure.
pub fn menu_keep(items: &[(&str, bool)]) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    let mut pending_sep: Option<usize> = None;
    for (i, (name, sep)) in items.iter().enumerate() {
        if *sep {
            if !out.is_empty() && pending_sep.is_none() {
                pending_sep = Some(i);
            }
            continue;
        }
        if MENU_KEEP.contains(name) {
            if let Some(s) = pending_sep.take() {
                out.push(s);
            }
            out.push(i);
        }
    }
    out
}

/// A failed load's words for its tab (`COREWEBVIEW2_WEB_ERROR_STATUS`). Pure.
pub fn error_words(status: i32) -> &'static str {
    match status {
        1..=6 => "The page's certificate isn't valid.",
        7 => "The page took too long to answer.",
        9..=12 => "Yap couldn't reach the page: is this PC online?",
        13 => "The page couldn't be found.",
        16 => "The connection was refused.",
        17 => "The page didn't load: it was blocked (only public web pages open here).",
        _ => "The page didn't load.",
    }
}

// ---- the commands ---------------------------------------------------------------------

/// The web's stand-in and whether nothing may go out (a test run).
fn web_route() -> (Option<String>, bool) {
    let ep = crate::web::Endpoints::current();
    (ep.standin, ep.offline)
}

/// A page's key, as the page made it ("page:" and its address: never
/// logged above `debug`, as an address isn't).
fn checked_key(key: &str) -> Result<String, String> {
    let k = key.trim();
    if k.is_empty() || k.len() > 2048 {
        return Err("That page isn't one of the panel's.".into());
    }
    Ok(k.to_string())
}

/// **Show a page in the panel**, over `bounds` (`None`: kept out of sight
/// for now): made and loaded the first time, else as it was.
#[tauri::command]
pub async fn page_view_show(app: AppHandle, key: String, url: String, section: Option<String>, bounds: Option<Bounds>) -> Result<(), String> {
    let key = checked_key(&key)?;
    let (standin, offline) = web_route();
    let load = load_url(&url, standin.as_deref(), offline)?;
    let section = section.map(|s| s.chars().take(crate::chat::pages::SECTION_MAX).collect::<String>()).unwrap_or_default();
    imp::show(app, key, url, load, section, bounds).await.map(|_| ())
}

/// **A page's dialog answered** in its tab's strip (`alert`, `confirm`,
/// `prompt`, "Leave this page?"): OK (`accept`, with a prompt's `text`) or
/// Cancel.
#[tauri::command]
pub async fn page_view_dialog(app: AppHandle, key: String, accept: bool, text: Option<String>) -> Result<(), String> {
    let text = text.map(|t| t.chars().take(2000).collect::<String>());
    imp::answer_dialog(app, checked_key(&key)?, accept, text).await
}

/// **Move a page** over its stage, or keep it out of sight (`None`: its tab
/// isn't showing, or something of Yap's covers it).
#[tauri::command]
pub async fn page_view_place(app: AppHandle, key: String, bounds: Option<Bounds>) -> Result<(), String> {
    imp::place(app, checked_key(&key)?, bounds).await
}

/// **Close a page** (its tab closed).
#[tauri::command]
pub async fn page_view_close(app: AppHandle, key: String) -> Result<(), String> {
    imp::close(app, Some(checked_key(&key)?)).await
}

/// **Close every page** (another conversation).
#[tauri::command]
pub async fn page_view_close_all(app: AppHandle) -> Result<(), String> {
    imp::close(app, None).await
}

/// The page's Back.
#[tauri::command]
pub async fn page_view_back(app: AppHandle, key: String) -> Result<(), String> {
    imp::go(app, checked_key(&key)?, imp::Go::Back).await
}

/// The page's Forward.
#[tauri::command]
pub async fn page_view_forward(app: AppHandle, key: String) -> Result<(), String> {
    imp::go(app, checked_key(&key)?, imp::Go::Forward).await
}

/// The page's Reload.
#[tauri::command]
pub async fn page_view_reload(app: AppHandle, key: String) -> Result<(), String> {
    imp::go(app, checked_key(&key)?, imp::Go::Reload).await
}

/// **The address bar**: what the person typed, opened in page `key` (an
/// address, or a search on their engine: [`address`]) — the page made the
/// first time (a new tab's), else taken there. The address it opened.
#[tauri::command]
pub async fn page_view_go(app: AppHandle, key: String, typed: String, bounds: Option<Bounds>) -> Result<String, String> {
    let key = checked_key(&key)?;
    let url = address(&typed, &crate::config::load().web_search_browser)?;
    let (standin, offline) = web_route();
    let load = load_url(&url, standin.as_deref(), offline)?;
    imp::go_to(app, key, url.clone(), load, bounds).await?;
    tracing::info!("Chat: the address bar took a page beside the chat somewhere");
    Ok(url)
}

/// **The pages' cookies, sign-ins and site data, cleared** (Delete some of
/// your data → Pages beside the chat): WebView2's own clearing of their
/// profile ([`PROFILE`]), through a page that's open, else one made out of
/// sight for it and closed after. Open pages stay open.
pub async fn clear_data(app: &AppHandle) -> Result<(), String> {
    imp::clear_data(app.clone()).await
}

/// **A page tab, as the model is told it**: its number (its own for the
/// session, [`NEXT_TAB`]: closing or evicting another never renumbers it),
/// title and address, and whether it's the one in front.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct OpenTab {
    pub tab: usize,
    /// Its tab's key (never sent to the model: its address is).
    #[serde(skip)]
    pub key: String,
    pub title: String,
    pub url: String,
    pub front: bool,
    /// The width it's laid out at for a desktop site's layout (CSS pixels,
    /// shown scaled to fit the panel: [`set_width`]); `None`: the panel's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
}

/// **The page tabs open beside the chat** now (none before Yap's window is
/// up): the one in front is the one on screen, else the one shown last.
pub async fn open_tabs() -> Vec<OpenTab> {
    let Some(app) = APP.get().cloned() else { return Vec::new() };
    imp::tabs(app).await.unwrap_or_default()
}

/// The tab the model means: `tab` (its number), else the one in front.
/// Pure.
pub fn pick_tab(tabs: &[OpenTab], tab: Option<usize>) -> Result<&OpenTab, String> {
    if tabs.is_empty() {
        return Err("No page is open beside the chat.".into());
    }
    match tab {
        Some(n) => tabs.iter().find(|t| t.tab == n).ok_or_else(|| format!("There's no tab {n} beside the chat: {} open.", tabs.len())),
        None => Ok(tabs.iter().find(|t| t.front).unwrap_or(&tabs[tabs.len() - 1])),
    }
}

/// **Run `script` in page `key`** (Yap's own script, never one a page or a
/// model wrote): its result as JSON — or an error after [`SCRIPT_TIMEOUT`]
/// (a page stuck in a loop, or waiting on a dialog).
pub async fn run_script(key: &str, script: String) -> Result<String, String> {
    let app = APP.get().cloned().ok_or("Yap's window isn't ready.")?;
    match tokio::time::timeout(SCRIPT_TIMEOUT, imp::eval(app, key.to_string(), script)).await {
        Ok(r) => r,
        Err(_) => {
            tracing::info!("Chat: a page beside the chat didn't answer Yap's script in time");
            Err("The page didn't answer in time (it may be busy, or waiting on something): try again, or read it again.".into())
        }
    }
}

/// What a page says, for the model (`read_open_page`): its title, its
/// headings, its main text (the `main` / `article` element's, else the
/// whole page's), whitespace folded, and its links — **the article's own
/// first** (8 Nov 2026, Yap Lab: a wiki's first 40 links were all its
/// language menu's): links in view in the content, then hidden ones (a
/// closed menu), then the page's navigation, header, footer and asides, a
/// language link (`hreflang`, or another `lang` than the page's) last; each
/// address once, at its best place. Read only.
pub const PAGE_TEXT_SCRIPT: &str = concat!("(function(){", deep_js!(), r#"var t=function(s){return (s||'').replace(/\s+/g,' ').trim();};var m=mainEl();if(!m)return {title:document.title,headings:[],text:'',links:[]};var hs=[].slice.call(qa('h1,h2,h3',m)).slice(0,40).map(function(h){return t(h.innerText);}).filter(Boolean);var seen={};var dl=(document.documentElement.lang||'').toLowerCase().split('-')[0];var nav='nav,header,footer,aside,[role=navigation],[role=banner],[role=contentinfo],[role=complementary],[role=menu],[role=menubar],[role=tablist]';var rk=function(a){var lg=(a.getAttribute('lang')||'').toLowerCase().split('-')[0];if(a.getAttribute('hreflang')||(lg&&dl&&lg!==dl))return 3;if(up(a,nav))return 2;if(!a.getClientRects().length)return 1;return 0;};var ls=[].slice.call(qa('a[href]',m)).map(function(a,i){return {text:t(a.innerText||a.getAttribute('aria-label')||a.title).slice(0,100),href:a.href,k:rk(a),i:i};}).sort(function(x,y){return x.k-y.k||x.i-y.i;}).filter(function(l){if(!/^https?:/i.test(l.href)||!l.text||seen[l.href])return false;seen[l.href]=1;return true;}).map(function(l){return {text:l.text,href:l.href};}).slice(0,120);return {title:t(document.title),headings:hs,text:t(shownText(m)).slice(0,24000),links:ls,frames:foreignFrames(m,5)};})()"#);

/// A page's words, read by [`PAGE_TEXT_SCRIPT`].
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PageText {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub headings: Vec<String>,
    #[serde(default)]
    pub text: String,
    /// Its main part's links, once each: their words and address (the
    /// page's own, a test run's stand-in's not yet turned back).
    #[serde(default)]
    pub links: Vec<PageLink>,
    /// Its frames no script outside them reaches (from another site, or
    /// sandboxed), shown on the page: at most 5 (a same-origin frame's words
    /// are read with the page's).
    #[serde(default)]
    pub frames: Vec<PageFrame>,
}

/// **A frame Yap can't read inside** ([`PageText::frames`]): its address
/// (the page's own, a test run's stand-in's not yet turned back), whether
/// it's sandboxed (kept apart from the page whatever its site), its title.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct PageFrame {
    #[serde(default)]
    pub src: String,
    #[serde(default)]
    pub sandboxed: bool,
    #[serde(default)]
    pub title: String,
}

/// A link on a page read ([`PageText`]).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct PageLink {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub href: String,
}

/// **Read page `key`**: its title, headings and main text.
pub async fn page_text(key: &str) -> Result<PageText, String> {
    let raw = run_script(key, PAGE_TEXT_SCRIPT.to_string()).await?;
    serde_json::from_str(&raw).map_err(|_| "The page couldn't be read.".to_string())
}

/// An address a page reports, as the page's own (a test run's stand-in
/// address turned back: `shown_url`).
pub fn shown(url: &str) -> String {
    let (standin, _) = web_route();
    shown_url(url, standin.as_deref())
}

/// **Where page `key`'s navigations are** (the page tools' actions wait on
/// them, `pages/backend.rs`): how many have started, and whether the
/// latest has loaded. `None`: no such page.
pub async fn nav_state(key: &str) -> Option<(u64, bool)> {
    let app = APP.get().cloned()?;
    imp::nav_state(app, key.to_string()).await.ok().flatten()
}

/// **Take page `key` to `url`** for the model (`go_to`, an address the page
/// tools checked), as a link would: the tab keeps its own address, so its
/// tab shown again stays where the page went.
pub async fn navigate(key: &str, url: &str) -> Result<(), String> {
    let app = APP.get().cloned().ok_or("Yap's window isn't ready.")?;
    let (standin, offline) = web_route();
    let load = load_url(url, standin.as_deref(), offline)?;
    imp::navigate(app, key.to_string(), load).await
}

/// **A page tab's key**, as the Chat view makes it (`chatDoc.svelte.js`
/// `pageKey`): "page:" and the address without its fragment or a trailing
/// slash. Pure.
pub fn key_of(url: &str) -> String {
    let u = url.trim();
    let u = u.split('#').next().unwrap_or(u).trim_end_matches('/');
    format!("page:{u}")
}

/// **A page `open_page` brought up, made at once** (out of sight, its tab's
/// key): the Chat view shows only the last of several a step brings up, so
/// each one's page is there — in the order they were asked for — for
/// `read_open_page` and the actions to find by its tab number. Its tab shown
/// later finds it made, and places it. Nothing without Yap's window. The
/// tab closed to make room for it (its number and title), if one was.
pub async fn open_hidden(url: &str, section: &str) -> Option<(usize, String)> {
    let app = APP.get().cloned()?;
    let (standin, offline) = web_route();
    let load = load_url(url, standin.as_deref(), offline).ok()?;
    let section: String = section.chars().take(crate::chat::pages::SECTION_MAX).collect();
    match imp::show(app, key_of(url), url.to_string(), load, section, None).await {
        Ok(closed) => closed,
        Err(e) => {
            tracing::debug!("A page brought up wasn't made out of sight: {e}");
            None
        }
    }
}

/// **Page `key` in front** (`switch_tab`): it takes the place of the page on
/// screen (the Chat view then shows its tab), or, with none on screen, is
/// the one in front from now on.
pub async fn bring_front(key: &str) -> Result<(), String> {
    let app = APP.get().cloned().ok_or("Yap's window isn't ready.")?;
    imp::bring_front(app, key.to_string()).await
}

/// **Their profile's folder** (its size, for the dialog): `WV2Profile_<name>`
/// in WebView2's `EBWebView` folder, in its user data folder — `None` when
/// there's none yet (never used) or Yap's window can't say.
pub async fn data_folder(app: &AppHandle) -> Option<std::path::PathBuf> {
    let root = imp::user_data_folder(app.clone()).await?;
    let name = format!("WV2Profile_{}", PROFILE.to_ascii_lowercase());
    [root.join("EBWebView").join(&name), root.join(&name)].into_iter().find(|d| d.is_dir())
}

/// **Save a shot** of the page as it is in the panel: kept like a pasted
/// picture, shown as a picture (`ShownPicture`, the page its source) and
/// kept with the answer of `conversationId` that brought it up
/// (`chats::add_shot`).
#[tauri::command]
pub async fn page_view_shot(app: AppHandle, key: String, url: String, title: Option<String>, conversation_id: Option<u64>) -> Result<crate::chat::pictures::ShownPicture, String> {
    let key = checked_key(&key)?;
    let png = imp::shot_of(app, key).await?;
    let image = tauri::async_runtime::spawn_blocking(move || crate::snip::attach::keep(&png, "page")).await.map_err(|e| e.to_string())??;
    let site = crate::web::site_of(&url);
    let title = title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).unwrap_or_else(|| site.clone());
    let caption: String = format!("A shot of {title}").chars().take(crate::chat::pictures::CAPTION_MAX).collect();
    let picture = crate::chat::pictures::ShownPicture {
        id: image.id.clone(),
        w: image.w,
        h: image.h,
        caption,
        page: crate::chat::pictures::PageRef { title, site, url: url.clone() },
        ..Default::default()
    };
    if let Some(conv) = conversation_id {
        crate::chat::chats::add_shot(conv, &url, picture.clone());
    }
    tracing::info!(w = image.w, h = image.h, "Chat: a shot of a page beside the chat saved");
    Ok(picture)
}

/// **A key that types a character** (a letter, a digit, space, the
/// punctuation keys, the number pad's): WebView2 never hands one to Yap as an
/// accelerator, so with a page beside the chat in focus a shortcut on one is
/// watched for instead (`imp::watch_keys`). Function keys, Esc and the like
/// come as accelerators. Pure.
pub fn types_a_character(vk: u32) -> bool {
    matches!(vk, 0x20 | 0x30..=0x39 | 0x41..=0x5A | 0x60..=0x6F | 0xBA..=0xC0 | 0xDB..=0xDF | 0xE2)
}

/// **What's in view of page `key` now**, as a PNG at the device's pixels
/// (`browser_screenshot`, `browser_zoom`): its own webview's
/// `CapturePreview` at its current scroll — on screen as it is; out of
/// sight (another tab in front, the panel closed), drawn for a moment under
/// Yap's own page at the size it had in the panel, so the shot is what the
/// person would see there. No page is loaded for it.
pub async fn shot_in_view(key: &str) -> Result<Vec<u8>, String> {
    let app = APP.get().cloned().ok_or("Yap's window isn't ready to take a shot.")?;
    imp::shot_in_view(app, key.to_string()).await
}

/// **What's in view of page `key` now, drawn `scale` times finer**
/// (`browser_zoom`, 8 Nov 2026), where nobody can see it drawn: a page out
/// of sight is drawn for a moment under Yap's own page at `scale` × its
/// rasterization scale and bounds — the same layout in CSS pixels, `scale`
/// times the pixels — then put back. A page on screen is the view as it is
/// (scale 1): WebView2 draws a window at its own pixels, so drawing it finer
/// would change what the person sees (a finer rasterization scale zooms the
/// page in the window; DevTools' `Page.captureScreenshot` with a clip at a
/// scale resizes the view while it draws). The PNG and the scale it has.
pub async fn shot_in_view_at(key: &str, scale: f64) -> Result<(Vec<u8>, f64), String> {
    let app = APP.get().cloned().ok_or("Yap's window isn't ready to take a shot.")?;
    imp::shot_in_view_at(app, key.to_string(), scale).await
}

/// **Page `key` laid out `width` CSS pixels wide** (`browser_tabs` width: a
/// site's desktop layout), shown scaled to fit the panel — its controller's
/// zoom ([`zoom_for`]) kept as the panel is resized and the page navigates —
/// or (`None`) back to the panel's own width. The tab says so ("Desktop
/// width ×", `ViewState::width`), and its × puts it back
/// ([`page_view_width`]); a zoom the person makes themselves (Ctrl+wheel)
/// takes it back too.
pub async fn set_width(key: &str, width: Option<u32>) -> Result<(), String> {
    let app = APP.get().cloned().ok_or("Yap's window isn't ready.")?;
    imp::set_width(app, key.to_string(), width).await
}

/// **The tab's "Desktop width ×"**: page `key` back to the panel's own
/// width (`width` none), or laid out that wide ([`set_width`]).
#[tauri::command]
pub async fn page_view_width(app: AppHandle, key: String, width: Option<u32>) -> Result<(), String> {
    let width = width.filter(|w| *w > 0);
    if width.is_some_and(|w| !(crate::chat::pages::see::WIDTH_MIN..=crate::chat::pages::see::WIDTH_MAX).contains(&w)) {
        return Err("That isn't a width a page is laid out at.".into());
    }
    tracing::info!(desktop = width.is_some(), "Chat: the user set a page's width beside the chat");
    imp::set_width(app, checked_key(&key)?, width).await
}

/// **Page `key` back or forward** in its own history (`browser_navigate`'s
/// "back" / "forward").
pub async fn history(key: &str, back: bool) -> Result<(), String> {
    let app = APP.get().cloned().ok_or("Yap's window isn't ready.")?;
    imp::go(app, key.to_string(), if back { imp::Go::Back } else { imp::Go::Forward }).await
}

/// **Page `key` closed** (`browser_tabs`' "close"; the Chat view closes its
/// tab on `stream::Event::CloseTab`).
pub async fn close(key: &str) -> Result<(), String> {
    let app = APP.get().cloned().ok_or("Yap's window isn't ready.")?;
    imp::close(app, Some(key.to_string())).await
}

/// Test mode: a page's script run in it (what the suite checks: no Yap IPC
/// there, its title, its text), its JSON result.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn e2e_page_view_eval(app: AppHandle, key: String, script: String) -> Result<String, String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".into());
    }
    imp::eval(app, checked_key(&key)?, script).await
}

/// Test mode: the pages alive now (key, address, on screen, loaded).
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn e2e_page_views(app: AppHandle) -> Result<serde_json::Value, String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".into());
    }
    imp::list(app).await
}

/// Test mode: "Ask Yap about this" on what's selected in the page (its menu
/// never shows in a test run).
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn e2e_page_view_ask(app: AppHandle, key: String) -> Result<String, String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".into());
    }
    let key = checked_key(&key)?;
    let raw = imp::eval(app.clone(), key.clone(), "window.getSelection().toString()".into()).await?;
    let text: String = serde_json::from_str(&raw).unwrap_or_default();
    imp::ask(app, key, text.clone()).await?;
    Ok(text)
}

#[cfg(windows)]
use win as imp;

#[cfg(not(windows))]
mod imp {
    use super::Bounds;
    use tauri::AppHandle;

    const NO: &str = "Pages beside the chat need Windows.";

    pub enum Go {
        Back,
        Forward,
        Reload,
    }

    pub async fn show(_: AppHandle, _: String, _: String, _: String, _: String, _: Option<Bounds>) -> Result<Option<(usize, String)>, String> {
        Err(NO.into())
    }
    pub async fn answer_dialog(_: AppHandle, _: String, _: bool, _: Option<String>) -> Result<(), String> {
        Ok(())
    }
    pub async fn dismiss_dialog(_: AppHandle, _: String) {}
    pub async fn go_to(_: AppHandle, _: String, _: String, _: String, _: Option<Bounds>) -> Result<(), String> {
        Err(NO.into())
    }
    pub async fn clear_data(_: AppHandle) -> Result<(), String> {
        Ok(())
    }
    pub async fn user_data_folder(_: AppHandle) -> Option<std::path::PathBuf> {
        None
    }
    pub async fn place(_: AppHandle, _: String, _: Option<Bounds>) -> Result<(), String> {
        Ok(())
    }
    pub async fn close(_: AppHandle, _: Option<String>) -> Result<(), String> {
        Ok(())
    }
    pub async fn go(_: AppHandle, _: String, _: Go) -> Result<(), String> {
        Err(NO.into())
    }
    pub async fn shot_of(_: AppHandle, _: String) -> Result<Vec<u8>, String> {
        Err(NO.into())
    }
    pub async fn shot_in_view(_: AppHandle, _: String) -> Result<Vec<u8>, String> {
        Err(NO.into())
    }
    pub async fn shot_in_view_at(_: AppHandle, _: String, _: f64) -> Result<(Vec<u8>, f64), String> {
        Err(NO.into())
    }
    pub async fn eval(_: AppHandle, _: String, _: String) -> Result<String, String> {
        Err(NO.into())
    }
    pub async fn tabs(_: AppHandle) -> Result<Vec<super::OpenTab>, String> {
        Ok(Vec::new())
    }
    pub async fn nav_state(_: AppHandle, _: String) -> Result<Option<(u64, bool)>, String> {
        Ok(None)
    }
    pub async fn navigate(_: AppHandle, _: String, _: String) -> Result<(), String> {
        Err(NO.into())
    }
    pub async fn set_width(_: AppHandle, _: String, _: Option<u32>) -> Result<(), String> {
        Err(NO.into())
    }
    pub async fn bring_front(_: AppHandle, _: String) -> Result<(), String> {
        Err(NO.into())
    }
    #[cfg(debug_assertions)]
    pub async fn list(_: AppHandle) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!([]))
    }
    #[cfg(debug_assertions)]
    pub async fn ask(_: AppHandle, _: String, _: String) -> Result<(), String> {
        Err(NO.into())
    }
}

#[cfg(windows)]
mod win {
    //! The Win32 and WebView2 side: everything here runs on the main thread
    //! (WebView2's), reached through the main window's `with_webview`; the
    //! pages live in a thread-local list.

    use std::cell::{Cell, RefCell};
    use std::sync::atomic::{AtomicBool, Ordering};

    use tauri::{AppHandle, Emitter, Manager};
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    use webview2_com::{
        take_pwstr, AcceleratorKeyPressedEventHandler, CapturePreviewCompletedHandler, ClearBrowsingDataCompletedHandler, ContextMenuRequestedEventHandler, CreateCoreWebView2ControllerCompletedHandler,
        CustomItemSelectedEventHandler, DocumentTitleChangedEventHandler, DownloadStartingEventHandler, ExecuteScriptCompletedHandler, FocusChangedEventHandler,
        HistoryChangedEventHandler, NavigationCompletedEventHandler, NavigationStartingEventHandler, NewWindowRequestedEventHandler,
        PermissionRequestedEventHandler, ProcessFailedEventHandler, ScriptDialogOpeningEventHandler, WebResourceRequestedEventHandler, ZoomFactorChangedEventHandler,
    };
    use windows::core::{w, Interface, BOOL, HSTRING, PWSTR};
    use windows::Win32::Foundation::{HGLOBAL, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{GetStockObject, HBRUSH, WHITE_BRUSH};
    use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
    use windows::Win32::System::Com::{IStream, STREAM_SEEK_SET};
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, GetKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, GetForegroundWindow, GetWindowLongPtrW, GetWindowThreadProcessId, RegisterClassExW, SetWindowPos, ShowWindow, GWLP_HINSTANCE,
        HWND_BOTTOM, HWND_TOP, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WINDOW_EX_STYLE, WNDCLASSEXW, WS_CHILD, WS_CLIPCHILDREN,
        WS_CLIPSIBLINGS,
    };

    use super::{
        error_words, evict, may_navigate, menu_keep, next_tab, refused_request, section_script, shown_url, Bounds, ViewState, DIALOG_EVENT, DRAW, EVENT, PROFILE, QUOTE_EVENT, SETTLE,
        SHOT_TIMEOUT, SNAP_H, SNAP_W,
    };

    type Done<T> = tokio::sync::oneshot::Sender<Result<T, String>>;

    /// A page beside the chat has the keyboard focus.
    static PAGE_FOCUSED: AtomicBool = AtomicBool::new(false);
    /// [`watch_keys`] is running.
    static WATCHING: AtomicBool = AtomicBool::new(false);

    /// Yap is the app in front (its main window; the page's own windows are
    /// WebView2's, inside it).
    fn yap_in_front() -> bool {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid)) };
        pid != 0 && pid == unsafe { GetCurrentProcessId() }
    }

    /// **The shortcuts on keys that type, while a page has the focus**:
    /// every 15 ms, the bound ones among them (`input_hook::bound_keys`,
    /// never any other key) going down or up are sent to the main window's
    /// page as the accelerators are (`yap-page-key`) — only while Yap is in
    /// front (elsewhere the hook hears them) and Ctrl and Alt are up (those
    /// combos come as accelerators). A key it saw go down is followed until
    /// it's up, focus or not. Ends when the page has lost the focus.
    fn watch_keys(app: AppHandle) {
        let mut down: Vec<u32> = Vec::new();
        loop {
            let held = |vk: u32| unsafe { GetAsyncKeyState(vk as i32) } < 0;
            let focused = PAGE_FOCUSED.load(Ordering::SeqCst);
            let live = focused && yap_in_front();
            if !focused && down.is_empty() {
                WATCHING.store(false, Ordering::SeqCst);
                // Focus came back as it stopped: carry on.
                if PAGE_FOCUSED.load(Ordering::SeqCst) && !WATCHING.swap(true, Ordering::SeqCst) {
                    continue;
                }
                return;
            }
            let (ctrl, alt) = (held(u32::from(VK_CONTROL.0)), held(u32::from(VK_MENU.0)));
            let keys: Vec<u32> = crate::dictation::input_hook::bound_keys().into_iter().filter(|&vk| super::types_a_character(vk)).collect();
            for vk in keys {
                let now = held(vk);
                let was = down.contains(&vk);
                let send = |is_down: bool| {
                    let _ = app.emit_to(
                        "settings",
                        "yap-page-key",
                        serde_json::json!({
                            "vk": vk,
                            "down": is_down,
                            "repeat": false,
                            "ctrl": ctrl,
                            "alt": alt,
                            "shift": held(u32::from(VK_SHIFT.0)),
                            "meta": held(u32::from(VK_LWIN.0)) || held(u32::from(VK_RWIN.0)),
                        }),
                    );
                };
                if now && !was && live && !ctrl && !alt {
                    send(true);
                    down.push(vk);
                } else if !now && was {
                    send(false);
                    down.retain(|&k| k != vk);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(if live || !down.is_empty() { 15 } else { 60 }));
        }
    }

    /// One page: its host window, its controller once made, where it should
    /// be, and anything waiting for it to load.
    struct View {
        key: String,
        /// Its tab number for the model (`super::NEXT_TAB`; 0: a shot's or
        /// a clearing's page, never a tab).
        id: usize,
        /// The address as the search or the person gave it.
        url: String,
        /// The address it loads from (a test run's stand-in's).
        load: String,
        section: String,
        host: HWND,
        controller: Option<ICoreWebView2Controller>,
        core: Option<ICoreWebView2>,
        bounds: Option<Bounds>,
        used: u64,
        loaded: bool,
        scrolled: bool,
        /// A shot's page, out of sight: the shot to take once it has loaded.
        snap: Option<Done<Vec<u8>>>,
        /// Navigations started, and how many of them had ended (loaded or not)
        /// when the latest ended: the page tools wait on them after an action.
        navs: u64,
        done: u64,
        /// The desktop width it's laid out at (CSS pixels; `super::set_width`),
        /// and the zoom Yap last set for it (`super::zoom_for`): a zoom
        /// changed to anything else while the page is loaded is the
        /// person's own (Ctrl+wheel), and the width is theirs again.
        width: Option<u32>,
        zoom: f64,
    }

    thread_local! {
        static VIEWS: RefCell<Vec<View>> = const { RefCell::new(Vec::new()) };
        /// Yap's WebView2 environment (the main window's) and its controller.
        static ENV: RefCell<Option<(ICoreWebView2Environment, ICoreWebView2Controller)>> = const { RefCell::new(None) };
        static CLASS: Cell<bool> = const { Cell::new(false) };
        static TICK: Cell<u64> = const { Cell::new(0) };
        /// The size pages last had on screen (physical pixels): a page out of
        /// sight is laid out at it (`put`).
        static STAGE: Cell<(i32, i32)> = const { Cell::new((SNAP_W, SNAP_H)) };
        /// A page's dialog waiting for the person (its tab's strip asks):
        /// the page's key, the dialog and its deferral.
        static DIALOGS: RefCell<Vec<(String, ICoreWebView2ScriptDialogOpeningEventArgs, ICoreWebView2Deferral)>> = const { RefCell::new(Vec::new()) };
    }

    /// Page `key`'s waiting dialog answered (OK with a prompt's `text`, or
    /// Cancel: `accept` false), and its strip told it's gone. (On the main
    /// thread.)
    fn finish_dialog(app: &AppHandle, key: &str, accept: bool, text: Option<&str>) {
        let waiting = DIALOGS.with(|d| {
            let mut d = d.borrow_mut();
            let i = d.iter().position(|x| x.0 == key)?;
            Some(d.remove(i))
        });
        let Some((_, args, deferral)) = waiting else { return };
        unsafe {
            if accept {
                if let Some(t) = text {
                    let _ = args.SetResultText(&HSTRING::from(t));
                }
                let _ = args.Accept();
            }
            let _ = deferral.Complete();
        }
        let _ = app.emit_to("settings", DIALOG_EVENT, serde_json::json!({ "key": key, "gone": true }));
    }

    pub async fn answer_dialog(app: AppHandle, key: String, accept: bool, text: Option<String>) -> Result<(), String> {
        let a = app.clone();
        main_thread(&app, move |_| {
            tracing::info!(accept, "Chat: the user answered a page's dialog");
            finish_dialog(&a, &key, accept, text.as_deref());
        })
        .await
    }

    pub async fn dismiss_dialog(app: AppHandle, key: String) {
        let a = app.clone();
        let _ = main_thread(&app, move |_| finish_dialog(&a, &key, false, None)).await;
    }

    fn tick() -> u64 {
        TICK.with(|t| {
            t.set(t.get() + 1);
            t.get()
        })
    }

    /// Run `f` on the main thread with Yap's main webview's controller; its
    /// answer back.
    async fn main_thread<T: Send + 'static>(app: &AppHandle, f: impl FnOnce(&ICoreWebView2Controller) -> T + Send + 'static) -> Result<T, String> {
        let window = app.get_webview_window("settings").ok_or("Yap's window isn't there.")?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        window
            .with_webview(move |pw| {
                let _ = tx.send(f(&pw.controller()));
            })
            .map_err(|e| format!("Yap's window didn't answer: {e}"))?;
        rx.await.map_err(|_| "Yap's window didn't answer.".to_string())
    }

    /// The main window's handle (the hosts' parent).
    fn parent(app: &AppHandle) -> Result<HWND, String> {
        let window = app.get_webview_window("settings").ok_or("Yap's window isn't there.")?;
        let h = window.hwnd().map_err(|e| e.to_string())?;
        Ok(HWND(h.0))
    }

    /// Yap's environment (read from its main webview once).
    fn env(root: &ICoreWebView2Controller) -> windows::core::Result<ICoreWebView2Environment> {
        if let Some((e, _)) = ENV.with(|c| c.borrow().clone()) {
            return Ok(e);
        }
        let core: ICoreWebView2 = unsafe { root.CoreWebView2()? };
        let env = unsafe { core.cast::<ICoreWebView2_2>()?.Environment()? };
        ENV.with(|c| *c.borrow_mut() = Some((env.clone(), root.clone())));
        Ok(env)
    }

    unsafe extern "system" fn host_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
    }

    /// A host window: a plain child of Yap's main window, the page's
    /// webview inside it.
    fn host(parent: HWND) -> Result<HWND, String> {
        let instance = HINSTANCE(unsafe { GetWindowLongPtrW(parent, GWLP_HINSTANCE) } as *mut _);
        if !CLASS.with(Cell::get) {
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(host_proc),
                hInstance: instance,
                lpszClassName: w!("YapPageHost"),
                hbrBackground: HBRUSH(unsafe { GetStockObject(WHITE_BRUSH) }.0),
                ..Default::default()
            };
            // (0: registered already, by an earlier run of this thread.)
            let _ = unsafe { RegisterClassExW(&class) };
            CLASS.with(|c| c.set(true));
        }
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("YapPageHost"),
                w!(""),
                WS_CHILD | WS_CLIPCHILDREN | WS_CLIPSIBLINGS,
                0,
                0,
                1,
                1,
                Some(parent),
                None,
                Some(instance),
                None,
            )
        }
        .map_err(|e| format!("Yap couldn't make room for the page: {e}"))
    }

    /// Where Yap's own page starts in its window (its webview's bounds: the
    /// page measures from there).
    fn page_origin() -> (i32, i32) {
        let Some((_, root)) = ENV.with(|c| c.borrow().clone()) else { return (0, 0) };
        let mut r = RECT::default();
        match unsafe { root.Bounds(&mut r) } {
            Ok(()) => (r.left, r.top),
            Err(_) => (0, 0),
        }
    }

    /// Put `host` (and its controller) over `bounds`, on top; or out of
    /// sight — still laid out at the size pages last had on screen
    /// ([`STAGE`]), so a page opened out of sight, or scrolled while it's
    /// there, is the page the person sees when its tab comes up (and the
    /// model's shot of it is that view).
    fn put(host: HWND, controller: Option<&ICoreWebView2Controller>, bounds: Option<Bounds>, snap: bool) {
        unsafe {
            match bounds {
                Some(b) => {
                    let (w, h) = (b.w.max(1), b.h.max(1));
                    if !snap {
                        STAGE.with(|s| s.set((w, h)));
                    }
                    // A shot's page sits under Yap's own page: drawn, never seen.
                    let z = if snap { HWND_BOTTOM } else { HWND_TOP };
                    let (ox, oy) = if snap { (0, 0) } else { page_origin() };
                    let _ = SetWindowPos(host, Some(z), b.x + ox, b.y + oy, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
                    if let Some(c) = controller {
                        let _ = c.SetBounds(RECT { left: 0, top: 0, right: w, bottom: h });
                        fit_width(host, c, w);
                        let _ = c.SetIsVisible(true);
                    }
                }
                None => {
                    let _ = ShowWindow(host, SW_HIDE);
                    if let Some(c) = controller {
                        let (w, h) = STAGE.with(Cell::get);
                        let _ = c.SetBounds(RECT { left: 0, top: 0, right: w, bottom: h });
                        fit_width(host, c, w);
                        let _ = c.SetIsVisible(false);
                    }
                }
            }
        }
    }

    /// The controller's device pixels a CSS pixel (the monitor's scale, or
    /// a zoom's finer draw), 1 when it can't say.
    fn raster_of(c: &ICoreWebView2Controller) -> f64 {
        let Ok(c3) = c.cast::<ICoreWebView2Controller3>() else { return 1.0 };
        let mut r = 0.0f64;
        match unsafe { c3.RasterizationScale(&mut r) } {
            Ok(()) if r.is_finite() && r > 0.0 => r,
            _ => 1.0,
        }
    }

    /// **A page with a desktop width, kept at it** in a view `phys_w` device
    /// pixels wide (its host's): the zoom that lays it out that wide
    /// ([`super::zoom_for`]), remembered so the person's own zoom is told from
    /// Yap's. Nothing for a page at the panel's own width (its zoom is the
    /// person's).
    fn fit_width(host: HWND, c: &ICoreWebView2Controller, phys_w: i32) {
        let Some(width) = VIEWS.with(|v| v.borrow().iter().find(|x| x.host == host).and_then(|x| x.width)) else { return };
        let z = super::zoom_for(phys_w, raster_of(c), width);
        VIEWS.with(|v| {
            if let Some(x) = v.borrow_mut().iter_mut().find(|x| x.host == host) {
                x.zoom = z;
            }
        });
        let mut now = 0.0f64;
        if unsafe { c.ZoomFactor(&mut now) }.is_ok() && (now - z).abs() < 0.0005 {
            return;
        }
        let _ = unsafe { c.SetZoomFactor(z) };
    }

    /// The width of page `host`'s controller now (device pixels).
    fn phys_width(c: &ICoreWebView2Controller) -> i32 {
        let mut r = RECT::default();
        match unsafe { c.Bounds(&mut r) } {
            Ok(()) => r.right - r.left,
            Err(_) => 0,
        }
    }

    /// **Page `key` laid out `width` wide** (see `super::set_width`), or back
    /// at the panel's own; its tab told (the chip).
    pub async fn set_width(app: AppHandle, key: String, width: Option<u32>) -> Result<(), String> {
        let a = app.clone();
        main_thread(&app, move |_| {
            let Some((host, Some(controller), _)) = handles(&key) else { return Err("That page isn't open (yet).".to_string()) };
            let (was, loading) = VIEWS.with(|v| {
                let mut v = v.borrow_mut();
                let x = v.iter_mut().find(|x| x.key == key)?;
                let was = x.width;
                x.width = width;
                if width.is_none() {
                    x.zoom = 1.0;
                }
                Some((was, !x.loaded))
            })
            .ok_or("That page isn't open.")?;
            match width {
                Some(_) => fit_width(host, &controller, phys_width(&controller)),
                // Back to the panel's own width: as the person had it before.
                None if was.is_some() => {
                    let _ = unsafe { controller.SetZoomFactor(1.0) };
                }
                None => {}
            }
            tracing::info!(desktop = width.is_some(), "Chat: a page beside the chat set to a width");
            tell(&a, &key, loading, None);
            Ok(())
        })
        .await?
    }

    /// The page `key`'s handles, if it's there.
    fn handles(key: &str) -> Option<(HWND, Option<ICoreWebView2Controller>, Option<ICoreWebView2>)> {
        VIEWS.with(|v| v.borrow().iter().find(|x| x.key == key).map(|x| (x.host, x.controller.clone(), x.core.clone())))
    }

    /// Tell the main window what page `key` is doing.
    fn tell(app: &AppHandle, key: &str, loading: bool, error: Option<String>) {
        if key.starts_with("snap:") {
            return;
        }
        let Some((_, _, core)) = handles(key) else { return };
        let Some(core) = core else { return };
        let (standin, _) = super::web_route();
        let width = VIEWS.with(|v| v.borrow().iter().find(|x| x.key == key).and_then(|x| x.width));
        let mut state = ViewState { key: key.to_string(), loading, error, width, ..Default::default() };
        unsafe {
            let mut s = PWSTR::null();
            if core.Source(&mut s).is_ok() {
                state.url = shown_url(&take_pwstr(s), standin.as_deref());
            }
            let mut t = PWSTR::null();
            if core.DocumentTitle(&mut t).is_ok() {
                state.title = take_pwstr(t);
            }
            let mut back = BOOL(0);
            if core.CanGoBack(&mut back).is_ok() {
                state.can_go_back = back.as_bool();
            }
            let mut forward = BOOL(0);
            if core.CanGoForward(&mut forward).is_ok() {
                state.can_go_forward = forward.as_bool();
            }
        }
        let _ = app.emit_to("settings", EVENT, state);
    }

    // ---- show, place, close --------------------------------------------------------------

    pub async fn show(app: AppHandle, key: String, url: String, load: String, section: String, bounds: Option<Bounds>) -> Result<Option<(usize, String)>, String> {
        let a = app.clone();
        main_thread(&app, move |root| make_or_place(&a, root, key, url, load, section, bounds, None)).await?
    }

    /// Show page `key` (made the first time); a shot's page (`snap`) is made
    /// out of sight and its shot taken once it has loaded. The tab closed to
    /// make room for it (its number and title), if one was.
    #[allow(clippy::too_many_arguments)]
    fn make_or_place(
        app: &AppHandle,
        root: &ICoreWebView2Controller,
        key: String,
        url: String,
        load: String,
        section: String,
        bounds: Option<Bounds>,
        snap: Option<Done<Vec<u8>>>,
    ) -> Result<Option<(usize, String)>, String> {
        if let Some((host, controller, core)) = handles(&key) {
            let changed = VIEWS.with(|v| {
                let mut v = v.borrow_mut();
                let view = v.iter_mut().find(|x| x.key == key)?;
                view.bounds = bounds;
                view.used = tick();
                // The same tab, another address (a new answer's page under the
                // same key never happens; a section asked again does).
                (view.load != load).then(|| {
                    view.load = load.clone();
                    view.url = url.clone();
                    view.section = section.clone();
                    view.scrolled = false;
                })
            });
            put(host, controller.as_ref(), bounds, false);
            match (changed, core) {
                (Some(()), Some(core)) => unsafe {
                    let _ = core.Navigate(&HSTRING::from(load.as_str()));
                },
                // Its tab shown again: told where the page is (its title, Back, Forward).
                (None, Some(_)) => {
                    let loading = VIEWS.with(|v| v.borrow().iter().find(|x| x.key == key).is_some_and(|x| !x.loaded));
                    tell(app, &key, loading, None);
                }
                _ => {}
            }
            return Ok(None);
        }
        // Room for one more: the least recently shown one goes.
        let mut closed = None;
        if snap.is_none() {
            let alive: Vec<(String, u64, bool)> =
                VIEWS.with(|v| v.borrow().iter().filter(|x| x.snap.is_none() && !x.key.starts_with("snap:") && !x.key.starts_with("clear:")).map(|x| (x.key.clone(), x.used, x.bounds.is_some())).collect());
            if let Some(gone) = evict(&alive) {
                tracing::info!(views = alive.len(), "Chat: the least recently shown page closed to make room");
                closed = VIEWS.with(|v| {
                    v.borrow().iter().find(|x| x.key == gone).map(|x| {
                        let mut title = String::new();
                        if let Some(core) = &x.core {
                            let mut t = PWSTR::null();
                            if unsafe { core.DocumentTitle(&mut t) }.is_ok() {
                                title = take_pwstr(t);
                            }
                        }
                        (x.id, title)
                    })
                });
                close_one(app, &gone);
            }
        }
        let env = env(root).map_err(|e| format!("Yap couldn't reach its webview: {e}"))?;
        let parent = parent(app)?;
        let host = host(parent)?;
        let is_snap = snap.is_some();
        let id = if is_snap { 0 } else { next_tab() };
        VIEWS.with(|v| {
            v.borrow_mut().push(View {
                key: key.clone(),
                id,
                url,
                load,
                section,
                host,
                controller: None,
                core: None,
                bounds,
                used: tick(),
                loaded: false,
                scrolled: false,
                snap,
                navs: 0,
                done: 0,
                width: None,
                zoom: 1.0,
            })
        });
        put(host, None, bounds, is_snap);
        let made = || -> windows::core::Result<()> {
            let env10: ICoreWebView2Environment10 = env.cast()?;
            let opts = unsafe { env10.CreateCoreWebView2ControllerOptions()? };
            unsafe {
                opts.SetProfileName(&HSTRING::from(PROFILE))?;
                opts.SetIsInPrivateModeEnabled(false)?;
            }
            let (a, k, e) = (app.clone(), key.clone(), env.clone());
            let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |res, controller| {
                match (res, controller) {
                    (Ok(()), Some(c)) => ready(&a, &k, &e, c),
                    (Err(err), _) => failed(&a, &k, &format!("The page couldn't open ({}).", err.message())),
                    _ => failed(&a, &k, "The page couldn't open."),
                }
                Ok(())
            }));
            unsafe { env10.CreateCoreWebView2ControllerWithOptions(host, &opts, &handler) }
        };
        if let Err(e) = made() {
            close_one(app, &key);
            return Err(format!("Yap couldn't open the page ({}). Its WebView2 may be too old: Windows Update brings a new one.", e.message()));
        }
        tracing::info!(snap = is_snap, views = VIEWS.with(|v| v.borrow().len()), "Chat: a page beside the chat is opening");
        Ok(closed)
    }

    /// A page that couldn't be made: said, and gone.
    fn failed(app: &AppHandle, key: &str, why: &str) {
        let snap = VIEWS.with(|v| v.borrow_mut().iter_mut().find(|x| x.key == key).and_then(|x| x.snap.take()));
        if let Some(done) = snap {
            let _ = done.send(Err(why.to_string()));
        } else {
            let _ = app.emit_to("settings", EVENT, ViewState { key: key.to_string(), error: Some(why.to_string()), ..Default::default() });
        }
        later_close(app, key);
    }

    /// Close page `key` once WebView2's callback that asked has returned
    /// (never a controller closed inside its own event).
    fn later_close(app: &AppHandle, key: &str) {
        let (a, k) = (app.clone(), key.to_string());
        std::thread::spawn(move || {
            let b = a.clone();
            let _ = a.run_on_main_thread(move || close_one(&b, &k));
        });
    }

    /// Its controller made: set up (no IPC, its rules, its menu), placed and
    /// loaded.
    fn ready(app: &AppHandle, key: &str, env: &ICoreWebView2Environment, controller: ICoreWebView2Controller) {
        let Some((host, bounds, load, snap)) =
            VIEWS.with(|v| v.borrow().iter().find(|x| x.key == key).map(|x| (x.host, x.bounds, x.load.clone(), x.snap.is_some())))
        else {
            // Closed meanwhile.
            unsafe {
                let _ = controller.Close();
            }
            return;
        };
        let core = match unsafe { controller.CoreWebView2() } {
            Ok(c) => c,
            Err(e) => return failed(app, key, &format!("The page couldn't open ({}).", e.message())),
        };
        if let Err(e) = set_up(app, key, env, &controller, &core, snap) {
            unsafe {
                let _ = controller.Close();
            }
            return failed(app, key, &format!("The page couldn't open safely ({}).", e.message()));
        }
        VIEWS.with(|v| {
            if let Some(x) = v.borrow_mut().iter_mut().find(|x| x.key == key) {
                x.controller = Some(controller.clone());
                x.core = Some(core.clone());
            }
        });
        put(host, Some(&controller), bounds, snap);
        unsafe {
            let _ = core.Navigate(&HSTRING::from(load.as_str()));
        }
    }

    /// **The page's rules**: no web messages or host objects (nothing of
    /// Yap's reachable), no DevTools in a release build, no autofill; its
    /// navigations checked, a new window in its own tab, downloads
    /// off, permissions denied, loopback refused; its state told; its menu.
    fn set_up(
        app: &AppHandle,
        key: &str,
        env: &ICoreWebView2Environment,
        controller: &ICoreWebView2Controller,
        core: &ICoreWebView2,
        snap: bool,
    ) -> windows::core::Result<()> {
        let (standin, offline) = super::web_route();
        let mut token = 0i64;
        unsafe {
            let settings = core.Settings()?;
            settings.SetIsWebMessageEnabled(false)?;
            settings.SetAreHostObjectsAllowed(false)?;
            settings.SetAreDevToolsEnabled(cfg!(debug_assertions))?;
            settings.SetIsZoomControlEnabled(true)?;
            if let Ok(s4) = settings.cast::<ICoreWebView2Settings4>() {
                let _ = s4.SetIsPasswordAutosaveEnabled(false);
                let _ = s4.SetIsGeneralAutofillEnabled(false);
            }
            if let Ok(c2) = controller.cast::<ICoreWebView2Controller2>() {
                let _ = c2.SetDefaultBackgroundColor(COREWEBVIEW2_COLOR { A: 255, R: 255, G: 255, B: 255 });
            }
            // A shot's page: 1280 × 800 at 100 %, whatever the screen.
            if snap {
                if let Ok(c3) = controller.cast::<ICoreWebView2Controller3>() {
                    let _ = c3.SetShouldDetectMonitorScaleChanges(false);
                    let _ = c3.SetRasterizationScale(1.0);
                }
            }

            // Every navigation: public http(s) only (a test run: the stand-in's).
            let (a, k, si) = (app.clone(), key.to_string(), standin.clone());
            core.add_NavigationStarting(
                &NavigationStartingEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut uri = PWSTR::null();
                    args.Uri(&mut uri)?;
                    let uri = take_pwstr(uri);
                    if !may_navigate(&uri, si.as_deref(), offline) {
                        args.SetCancel(true)?;
                        let mut by_person = BOOL(0);
                        let _ = args.IsUserInitiated(&mut by_person);
                        // A mail link the person clicked: their mail app.
                        if by_person.as_bool() && uri.to_ascii_lowercase().starts_with("mailto:") {
                            let _ = crate::shell::commands::open_link(&a, &uri);
                        }
                        tracing::info!("Chat: a page's navigation was refused (not a public web page)");
                        return Ok(());
                    }
                    VIEWS.with(|v| {
                        if let Some(x) = v.borrow_mut().iter_mut().find(|x| x.key == k) {
                            x.navs += 1;
                            x.loaded = false;
                        }
                    });
                    tell(&a, &k, true, None);
                    Ok(())
                })),
                &mut token,
            )?;
            // A frame's too.
            let si = standin.clone();
            core.add_FrameNavigationStarting(
                &NavigationStartingEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut uri = PWSTR::null();
                    args.Uri(&mut uri)?;
                    let uri = take_pwstr(uri);
                    // (about:blank and about:srcdoc frames load nothing.)
                    if !uri.starts_with("about:") && !may_navigate(&uri, si.as_deref(), offline) {
                        args.SetCancel(true)?;
                    }
                    Ok(())
                })),
                &mut token,
            )?;
            // A new window: none — a link the person clicked opens in this
            // tab (sent to their browser instead, a page that falls back to
            // opening it in place — Google's sponsored results — opened it
            // twice, 1 Nov 2026); a mail link in their mail app. One they
            // didn't click (a pop-up): nothing.
            let (a, si) = (app.clone(), standin.clone());
            core.add_NewWindowRequested(
                &NewWindowRequestedEventHandler::create(Box::new(move |sender, args| {
                    let Some(args) = args else { return Ok(()) };
                    args.SetHandled(true)?;
                    let mut by_person = BOOL(0);
                    let _ = args.IsUserInitiated(&mut by_person);
                    if !by_person.as_bool() {
                        return Ok(());
                    }
                    let mut uri = PWSTR::null();
                    args.Uri(&mut uri)?;
                    let uri = take_pwstr(uri);
                    if may_navigate(&uri, si.as_deref(), offline) {
                        if let Some(core) = sender {
                            let _ = core.Navigate(&HSTRING::from(uri.as_str()));
                        }
                        tracing::info!("Chat: a page's new window opened in its own tab");
                    } else if uri.to_ascii_lowercase().starts_with("mailto:") {
                        let _ = crate::shell::commands::open_link(&a, &uri);
                    }
                    Ok(())
                })),
                &mut token,
            )?;
            // Downloads: off.
            if let Ok(core4) = core.cast::<ICoreWebView2_4>() {
                core4.add_DownloadStarting(
                    &DownloadStartingEventHandler::create(Box::new(|_, args| {
                        if let Some(args) = args {
                            args.SetCancel(true)?;
                            args.SetHandled(true)?;
                            tracing::info!("Chat: a page's download was refused");
                        }
                        Ok(())
                    })),
                    &mut token,
                )?;
            }
            // Every permission (the mic, the camera, location, notifications,
            // the clipboard…): denied.
            core.add_PermissionRequested(
                &PermissionRequestedEventHandler::create(Box::new(|_, args| {
                    if let Some(args) = args {
                        args.SetState(COREWEBVIEW2_PERMISSION_STATE_DENY)?;
                    }
                    Ok(())
                })),
                &mut token,
            )?;
            // Every request: one to this PC or its network refused (Yap's
            // own helpers listen on this PC, each behind a key of its own:
            // `local_llm/key.rs`) — an IP address in a non-public range however it's
            // written, or a local name (`super::refused_request`). A test
            // run's stand-in is the page itself. Its workers' too: WebView2
            // hands Yap a shared or service worker's requests only for a
            // filter that asks for them (`ICoreWebView2_22`; before, a page's
            // service worker reached this PC unchecked, 2 Nov 2026). A
            // runtime without it: the page's own (and its dedicated workers').
            match core.cast::<ICoreWebView2_22>() {
                Ok(c22) => c22.AddWebResourceRequestedFilterWithRequestSourceKinds(w!("*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL, COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL)?,
                Err(_) => core.AddWebResourceRequestedFilter(w!("*"), COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)?,
            }
            let (e, si) = (env.clone(), standin.clone());
            core.add_WebResourceRequested(
                &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut uri = PWSTR::null();
                    args.Request()?.Uri(&mut uri)?;
                    let uri = take_pwstr(uri);
                    if !refused_request(&uri, si.as_deref()) {
                        return Ok(());
                    }
                    tracing::debug!("A page's request to this PC or its network was refused");
                    let refused = e.CreateWebResourceResponse(None::<&IStream>, 403, w!("Forbidden"), w!(""))?;
                    args.SetResponse(&refused)?;
                    Ok(())
                })),
                &mut token,
            )?;
            // Its dialogs (alert, confirm, prompt, "Leave this page?"): never
            // WebView2's own — a native dialog over Yap from a page out of
            // sight, or a page waiting on one while Yap's script waits on the
            // page. Dismissed (never accepted) on a shot's page, a page out of
            // sight, or one Yap is acting on (`super::acting_on`); on a page
            // the person is using, its tab's strip asks (`DIALOG_EVENT`, the
            // answer `page_view_dialog`), the page waiting meanwhile.
            settings.SetAreDefaultScriptDialogsEnabled(false)?;
            let (a, k) = (app.clone(), key.to_string());
            core.add_ScriptDialogOpening(
                &ScriptDialogOpeningEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let on_screen = VIEWS.with(|v| v.borrow().iter().any(|x| x.key == k && x.bounds.is_some()));
                    if snap || !on_screen || super::acting_on(&k) {
                        tracing::info!(snap, on_screen, "Chat: a page's dialog was dismissed");
                        return Ok(());
                    }
                    let mut kind = COREWEBVIEW2_SCRIPT_DIALOG_KIND(0);
                    args.Kind(&mut kind)?;
                    let kind = match kind {
                        COREWEBVIEW2_SCRIPT_DIALOG_KIND_CONFIRM => "confirm",
                        COREWEBVIEW2_SCRIPT_DIALOG_KIND_PROMPT => "prompt",
                        COREWEBVIEW2_SCRIPT_DIALOG_KIND_BEFOREUNLOAD => "leave",
                        _ => "alert",
                    };
                    let mut m = PWSTR::null();
                    args.Message(&mut m)?;
                    let message: String = take_pwstr(m).chars().take(600).collect();
                    let mut dt = PWSTR::null();
                    args.DefaultText(&mut dt)?;
                    let default: String = take_pwstr(dt).chars().take(300).collect();
                    // One at a time: an older one still waiting goes first.
                    finish_dialog(&a, &k, false, None);
                    let deferral = args.GetDeferral()?;
                    DIALOGS.with(|d| d.borrow_mut().push((k.clone(), args.clone(), deferral)));
                    tracing::info!(kind, "Chat: a page's dialog is asked in its tab");
                    let _ = a.emit_to("settings", DIALOG_EVENT, serde_json::json!({ "key": k, "kind": kind, "message": message, "default": default }));
                    Ok(())
                })),
                &mut token,
            )?;
            // Its state, for the tab.
            let (a, k) = (app.clone(), key.to_string());
            core.add_NavigationCompleted(
                &NavigationCompletedEventHandler::create(Box::new(move |_, args| {
                    let mut ok = BOOL(1);
                    let mut status = COREWEBVIEW2_WEB_ERROR_STATUS(0);
                    if let Some(args) = &args {
                        let _ = args.IsSuccess(&mut ok);
                        let _ = args.WebErrorStatus(&mut status);
                    }
                    loaded(&a, &k, ok.as_bool(), status.0);
                    Ok(())
                })),
                &mut token,
            )?;
            let (a, k) = (app.clone(), key.to_string());
            core.add_DocumentTitleChanged(
                &DocumentTitleChangedEventHandler::create(Box::new(move |_, _| {
                    tell(&a, &k, false, None);
                    Ok(())
                })),
                &mut token,
            )?;
            let (a, k) = (app.clone(), key.to_string());
            core.add_HistoryChanged(
                &HistoryChangedEventHandler::create(Box::new(move |_, _| {
                    tell(&a, &k, false, None);
                    Ok(())
                })),
                &mut token,
            )?;
            let (a, k) = (app.clone(), key.to_string());
            core.add_ProcessFailed(
                &ProcessFailedEventHandler::create(Box::new(move |_, _| {
                    tracing::info!("Chat: a page beside the chat stopped working");
                    tell(&a, &k, false, Some("The page stopped working. Reload it.".into()));
                    Ok(())
                })),
                &mut token,
            )?;
            // A desktop width kept (`set_width`): a navigation that put the
            // zoom back gets the width again; a zoom the person made on a
            // loaded page (Ctrl+wheel, Ctrl+±) is theirs — the width let go,
            // the tab's chip gone.
            if !snap {
                let (a, k) = (app.clone(), key.to_string());
                controller.add_ZoomFactorChanged(
                    &ZoomFactorChangedEventHandler::create(Box::new(move |sender, _| {
                        let Some(c) = sender else { return Ok(()) };
                        let mut z = 0.0f64;
                        c.ZoomFactor(&mut z)?;
                        let Some((width, expected, loaded)) = VIEWS.with(|v| v.borrow().iter().find(|x| x.key == k).map(|x| (x.width, x.zoom, x.loaded))) else { return Ok(()) };
                        if width.is_none() || expected <= 0.0 || (z / expected - 1.0).abs() < 0.01 {
                            return Ok(());
                        }
                        if !loaded {
                            if let Some((host, _, _)) = handles(&k) {
                                fit_width(host, &c, phys_width(&c));
                            }
                            return Ok(());
                        }
                        VIEWS.with(|v| {
                            if let Some(x) = v.borrow_mut().iter_mut().find(|x| x.key == k) {
                                x.width = None;
                                x.zoom = z;
                            }
                        });
                        tracing::info!("Chat: the user zoomed a page beside the chat: its desktop width let go");
                        tell(&a, &k, false, None);
                        Ok(())
                    })),
                    &mut token,
                )?;
            }
            // Yap's shortcuts while the page has the focus: WebView2 runs
            // ahead of the keyboard hook (as in Yap's own windows), so its
            // function keys and Ctrl / Alt combos go to the main window's
            // page, which takes them as its own keys (settings/
            // hotkeyFallback.svelte.js). Only the key and its modifiers. A
            // shortcut on a key that types (a "9") never comes as one: while
            // the page has the focus, Yap watches those keys itself
            // (`watch_keys`: the maintainer's Voice Assistant key did nothing
            // after a click on the page until they clicked away, 1 Nov 2026).
            if !snap {
                let a = app.clone();
                controller.add_GotFocus(
                    &FocusChangedEventHandler::create(Box::new(move |_, _| {
                        PAGE_FOCUSED.store(true, Ordering::SeqCst);
                        if !WATCHING.swap(true, Ordering::SeqCst) {
                            let a = a.clone();
                            std::thread::spawn(move || watch_keys(a));
                        }
                        Ok(())
                    })),
                    &mut token,
                )?;
                controller.add_LostFocus(
                    &FocusChangedEventHandler::create(Box::new(move |_, _| {
                        PAGE_FOCUSED.store(false, Ordering::SeqCst);
                        Ok(())
                    })),
                    &mut token,
                )?;
                let a = app.clone();
                controller.add_AcceleratorKeyPressed(
                    &AcceleratorKeyPressedEventHandler::create(Box::new(move |_, args| {
                        let Some(args) = args else { return Ok(()) };
                        let mut kind = COREWEBVIEW2_KEY_EVENT_KIND(0);
                        args.KeyEventKind(&mut kind)?;
                        let down = kind == COREWEBVIEW2_KEY_EVENT_KIND_KEY_DOWN || kind == COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_DOWN;
                        let up = kind == COREWEBVIEW2_KEY_EVENT_KIND_KEY_UP || kind == COREWEBVIEW2_KEY_EVENT_KIND_SYSTEM_KEY_UP;
                        if !down && !up {
                            return Ok(());
                        }
                        let mut vk = 0u32;
                        args.VirtualKey(&mut vk)?;
                        let mut status = COREWEBVIEW2_PHYSICAL_KEY_STATUS::default();
                        let _ = args.PhysicalKeyStatus(&mut status);
                        let held = |k: VIRTUAL_KEY| GetKeyState(i32::from(k.0)) < 0;
                        let _ = a.emit_to(
                            "settings",
                            "yap-page-key",
                            serde_json::json!({
                                "vk": vk,
                                "down": down,
                                "repeat": down && status.WasKeyDown.as_bool(),
                                "ctrl": held(VK_CONTROL),
                                "alt": held(VK_MENU),
                                "shift": held(VK_SHIFT),
                                "meta": held(VK_LWIN) || held(VK_RWIN),
                            }),
                        );
                        Ok(())
                    })),
                    &mut token,
                )?;
            }
            // Its menu.
            if let Ok(core11) = core.cast::<ICoreWebView2_11>() {
                let (a, k, e) = (app.clone(), key.to_string(), env.clone());
                core11.add_ContextMenuRequested(
                    &ContextMenuRequestedEventHandler::create(Box::new(move |_, args| {
                        if let Some(args) = args {
                            if let Err(err) = menu(&a, &k, &e, &args) {
                                tracing::debug!("A page's menu: {err}");
                                let _ = args.SetHandled(true);
                            }
                        }
                        Ok(())
                    })),
                    &mut token,
                )?;
            }
        }
        Ok(())
    }

    /// A page finished loading (or didn't): its tab told; scrolled to its
    /// section the first time; a shot's page shot once it has settled.
    fn loaded(app: &AppHandle, key: &str, ok: bool, status: i32) {
        let (core, section, snap) = VIEWS.with(|v| {
            let mut v = v.borrow_mut();
            let Some(x) = v.iter_mut().find(|x| x.key == key) else { return (None, String::new(), false) };
            x.loaded = ok;
            x.done = x.navs;
            let section = if ok && !x.scrolled && !x.section.is_empty() {
                x.scrolled = true;
                x.section.clone()
            } else {
                String::new()
            };
            (x.core.clone(), section, x.snap.is_some())
        });
        if snap {
            if !ok {
                failed(app, key, error_words(status));
                return;
            }
            // Its pictures and scripts get a moment, then the shot.
            let (a, k) = (app.clone(), key.to_string());
            std::thread::spawn(move || {
                std::thread::sleep(SETTLE);
                let b = a.clone();
                let _ = a.run_on_main_thread(move || shoot_snap(&b, &k));
            });
            return;
        }
        // A desktop width: kept through the navigation (WebView2 may have put
        // the zoom back for the new page).
        if let Some((host, Some(c), _)) = handles(key) {
            fit_width(host, &c, phys_width(&c));
        }
        let error = (!ok && status != 0 && status != 15).then(|| error_words(status).to_string());
        tell(app, key, false, error);
        if let (Some(core), false) = (core, section.is_empty()) {
            unsafe {
                let _ = core.ExecuteScript(
                    &HSTRING::from(section_script(&section)),
                    &ExecuteScriptCompletedHandler::create(Box::new(|_, found| {
                        tracing::debug!(found = %found, "A page scrolled to its section");
                        Ok(())
                    })),
                );
            }
        }
    }

    /// A shot's page has loaded and settled: its shot, then it closes.
    fn shoot_snap(app: &AppHandle, key: &str) {
        let Some((_, _, Some(core))) = handles(key) else { return };
        let (a, k) = (app.clone(), key.to_string());
        let started = capture(&core, Box::new(move |res| {
            let done = VIEWS.with(|v| v.borrow_mut().iter_mut().find(|x| x.key == k).and_then(|x| x.snap.take()));
            if let Some(done) = done {
                let _ = done.send(res);
            }
            later_close(&a, &k);
        }));
        if let Err(e) = started {
            failed(app, key, &e);
        }
    }

    /// **The page's shot** (`CapturePreview`, a PNG): `done` hears it.
    fn capture(core: &ICoreWebView2, done: Box<dyn FnOnce(Result<Vec<u8>, String>)>) -> Result<(), String> {
        let stream: IStream = unsafe { CreateStreamOnHGlobal(HGLOBAL::default(), true) }.map_err(|e| format!("Yap couldn't take the shot ({}).", e.message()))?;
        let read_from = stream.clone();
        let mut done = Some(done);
        let handler = CapturePreviewCompletedHandler::create(Box::new(move |res| {
            let out = res.map_err(|e| format!("The shot didn't come out ({}).", e.message())).and_then(|()| read_all(&read_from));
            if let Some(done) = done.take() {
                done(out);
            }
            Ok(())
        }));
        unsafe { core.CapturePreview(COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG, &stream, &handler) }
            .map_err(|e| format!("Yap couldn't take the shot ({}).", e.message()))
    }

    /// A stream's bytes, from its start.
    fn read_all(stream: &IStream) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        unsafe {
            stream.Seek(0, STREAM_SEEK_SET, None).map_err(|e| e.message())?;
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                let mut read = 0u32;
                let hr = stream.Read(buf.as_mut_ptr().cast(), buf.len() as u32, Some(&mut read as *mut u32));
                if read == 0 {
                    break;
                }
                out.extend_from_slice(&buf[..read as usize]);
                if hr.is_err() || out.len() > 64 * 1024 * 1024 {
                    break;
                }
            }
        }
        if out.is_empty() {
            return Err("The shot came out empty.".into());
        }
        Ok(out)
    }

    pub async fn place(app: AppHandle, key: String, bounds: Option<Bounds>) -> Result<(), String> {
        main_thread(&app, move |_| {
            let Some((host, controller, _)) = handles(&key) else { return };
            VIEWS.with(|v| {
                if let Some(x) = v.borrow_mut().iter_mut().find(|x| x.key == key) {
                    x.bounds = bounds;
                    if bounds.is_some() {
                        x.used = tick();
                    }
                }
            });
            put(host, controller.as_ref(), bounds, false);
        })
        .await
    }

    /// Close page `key`: its controller and its host.
    fn close_one(app: &AppHandle, key: &str) {
        // A dialog it had waiting: answered Cancel (never left hanging).
        finish_dialog(app, key, false, None);
        let gone = VIEWS.with(|v| {
            let mut v = v.borrow_mut();
            let i = v.iter().position(|x| x.key == key)?;
            Some(v.remove(i))
        });
        if let Some(view) = gone {
            unsafe {
                if let Some(c) = &view.controller {
                    let _ = c.Close();
                }
                let _ = DestroyWindow(view.host);
            }
        }
    }

    pub async fn close(app: AppHandle, key: Option<String>) -> Result<(), String> {
        let a = app.clone();
        main_thread(&app, move |_| {
            let keys: Vec<String> = match key {
                Some(k) => vec![k],
                None => VIEWS.with(|v| v.borrow().iter().filter(|x| x.snap.is_none()).map(|x| x.key.clone()).collect()),
            };
            for k in &keys {
                close_one(&a, k);
            }
            if !keys.is_empty() {
                tracing::info!(closed = keys.len(), "Chat: pages beside the chat closed");
            }
        })
        .await
    }

    pub enum Go {
        Back,
        Forward,
        Reload,
    }

    pub async fn go(app: AppHandle, key: String, go: Go) -> Result<(), String> {
        main_thread(&app, move |_| {
            let Some((_, _, Some(core))) = handles(&key) else { return Err("That page isn't open.".to_string()) };
            unsafe {
                match go {
                    Go::Back => core.GoBack(),
                    Go::Forward => core.GoForward(),
                    Go::Reload => core.Reload(),
                }
            }
            .map_err(|e| e.message())
        })
        .await?
    }

    /// **The address bar's page**: page `key` taken to `load` (its address
    /// `url` from now on: a tab shown again asks for it), or made there the
    /// first time (a new tab's).
    pub async fn go_to(app: AppHandle, key: String, url: String, load: String, bounds: Option<Bounds>) -> Result<(), String> {
        let a = app.clone();
        main_thread(&app, move |root| {
            let open = VIEWS.with(|v| {
                let mut v = v.borrow_mut();
                let view = v.iter_mut().find(|x| x.key == key)?;
                view.url = url.clone();
                view.load = load.clone();
                view.section.clear();
                view.scrolled = true;
                if bounds.is_some() {
                    view.bounds = bounds;
                    view.used = tick();
                }
                Some(view.core.clone())
            });
            match open {
                Some(Some(core)) => unsafe { core.Navigate(&HSTRING::from(load.as_str())) }.map_err(|e| e.message()),
                // Still being made: it loads the new address when it's ready.
                Some(None) => Ok(()),
                None => make_or_place(&a, root, key, url, load, String::new(), bounds, None).map(|_| ()),
            }
        })
        .await?
    }

    /// Page `key`'s navigations: started, and whether the latest has ended.
    pub async fn nav_state(app: AppHandle, key: String) -> Result<Option<(u64, bool)>, String> {
        main_thread(&app, move |_| VIEWS.with(|v| v.borrow().iter().find(|x| x.key == key).map(|x| (x.navs, x.navs > 0 && x.done >= x.navs)))).await
    }

    /// The model's `go_to`: page `key` to `load`, as a link takes it (its own
    /// address kept: its tab shown again doesn't go back).
    pub async fn navigate(app: AppHandle, key: String, load: String) -> Result<(), String> {
        main_thread(&app, move |_| {
            let Some((_, _, Some(core))) = handles(&key) else { return Err("That page isn't open.".to_string()) };
            unsafe { core.Navigate(&HSTRING::from(load.as_str())) }.map_err(|e| e.message())
        })
        .await?
    }

    /// The model's `switch_tab`: page `key` takes the place of the one on
    /// screen (out of sight now); with none on screen, it's the one in front
    /// from now on (shown last).
    pub async fn bring_front(app: AppHandle, key: String) -> Result<(), String> {
        main_thread(&app, move |_| {
            if handles(&key).is_none() {
                return Err("That page isn't open.".to_string());
            }
            let on_screen = VIEWS.with(|v| v.borrow().iter().find(|x| x.bounds.is_some() && x.key != key && x.snap.is_none() && !x.key.starts_with("snap:")).map(|x| (x.key.clone(), x.host, x.controller.clone(), x.bounds)));
            let moved = on_screen.as_ref().and_then(|(_, _, _, b)| *b);
            if let Some((old, host, controller, _)) = on_screen {
                VIEWS.with(|v| {
                    if let Some(x) = v.borrow_mut().iter_mut().find(|x| x.key == old) {
                        x.bounds = None;
                    }
                });
                put(host, controller.as_ref(), None, false);
            }
            let target = VIEWS.with(|v| {
                let mut v = v.borrow_mut();
                let x = v.iter_mut().find(|x| x.key == key)?;
                x.used = tick();
                if moved.is_some() {
                    x.bounds = moved;
                }
                Some((x.host, x.controller.clone(), x.bounds))
            });
            if let (Some((host, controller, bounds)), Some(_)) = (target, moved) {
                put(host, controller.as_ref(), bounds, false);
            }
            Ok(())
        })
        .await?
    }

    // ---- the pages' own data ---------------------------------------------------------------

    static CLEARS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    /// WebView2's user data folder (the pages' profile is a folder in it).
    pub async fn user_data_folder(app: AppHandle) -> Option<std::path::PathBuf> {
        main_thread(&app, |root| -> Option<std::path::PathBuf> {
            let env = env(root).ok()?;
            let env7: ICoreWebView2Environment7 = env.cast().ok()?;
            let mut p = PWSTR::null();
            unsafe { env7.UserDataFolder(&mut p) }.ok()?;
            let s = take_pwstr(p);
            (!s.is_empty()).then(|| std::path::PathBuf::from(s))
        })
        .await
        .ok()
        .flatten()
    }

    /// Clear everything the pages' profile keeps, through `core` (one of its
    /// pages); `done` hears how it went.
    fn clear_profile(core: &ICoreWebView2, done: Box<dyn FnOnce(Result<(), String>)>) -> Result<(), String> {
        let old = |e: windows::core::Error| format!("This WebView2 can't clear the pages' data ({}): Windows Update brings a newer one.", e.message());
        let profile = unsafe { core.cast::<ICoreWebView2_13>().and_then(|c| c.Profile()) }.map_err(old)?;
        let profile: ICoreWebView2Profile2 = profile.cast().map_err(old)?;
        let mut done = Some(done);
        let handler = ClearBrowsingDataCompletedHandler::create(Box::new(move |res| {
            if let Some(done) = done.take() {
                done(res.map_err(|e| format!("The pages' data wasn't cleared ({}).", e.message())));
            }
            Ok(())
        }));
        unsafe { profile.ClearBrowsingDataAll(&handler) }.map_err(|e| format!("The pages' data wasn't cleared ({}).", e.message()))
    }

    pub async fn clear_data(app: AppHandle) -> Result<(), String> {
        let (tx, rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
        let a = app.clone();
        main_thread(&app, move |root| -> Result<(), String> {
            let send = move |r: Result<(), String>| {
                let _ = tx.send(r);
            };
            // A page that's open: its profile is theirs.
            if let Some(core) = VIEWS.with(|v| v.borrow().iter().find_map(|x| x.core.clone())) {
                return clear_profile(&core, Box::new(send));
            }
            // None: one made out of sight for it, closed after (never inside
            // WebView2's own callback).
            let env = env(root).map_err(|e| format!("Yap couldn't reach its webview: {e}"))?;
            let env10: ICoreWebView2Environment10 = env.cast().map_err(|e| format!("This WebView2 can't clear the pages' data ({}).", e.message()))?;
            let parent = parent(&a)?;
            let host = host(parent)?;
            let key = format!("clear:{}", CLEARS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1);
            VIEWS.with(|v| {
                v.borrow_mut().push(View {
                    key: key.clone(),
                    id: 0,
                    url: String::new(),
                    load: String::new(),
                    section: String::new(),
                    host,
                    controller: None,
                    core: None,
                    bounds: None,
                    used: tick(),
                    loaded: false,
                    scrolled: true,
                    snap: None,
                    navs: 0,
                    done: 0,
                    width: None,
                    zoom: 1.0,
                })
            });
            let opts = unsafe { env10.CreateCoreWebView2ControllerOptions() }.map_err(|e| e.message())?;
            unsafe {
                opts.SetProfileName(&HSTRING::from(PROFILE)).map_err(|e| e.message())?;
                opts.SetIsInPrivateModeEnabled(false).map_err(|e| e.message())?;
            }
            let (b, k) = (a.clone(), key.clone());
            let mut send = Some(send);
            let handler = CreateCoreWebView2ControllerCompletedHandler::create(Box::new(move |res, controller| {
                let Some(send) = send.take() else { return Ok(()) };
                let core = match (res, controller) {
                    (Ok(()), Some(c)) => {
                        VIEWS.with(|v| {
                            if let Some(x) = v.borrow_mut().iter_mut().find(|x| x.key == k) {
                                x.controller = Some(c.clone());
                            }
                        });
                        unsafe { c.CoreWebView2() }.map_err(|e| e.message())
                    }
                    (Err(e), _) => Err(e.message()),
                    _ => Err("WebView2 didn't make the page.".to_string()),
                };
                let (c, kk) = (b.clone(), k.clone());
                let finish = move |r: Result<(), String>| {
                    later_close(&c, &kk);
                    send(r);
                };
                match core {
                    Ok(core) => {
                        // Once: by WebView2's answer, or here if it never started.
                        let finish = std::rc::Rc::new(RefCell::new(Some(finish)));
                        let f = finish.clone();
                        let started = clear_profile(
                            &core,
                            Box::new(move |r| {
                                if let Some(f) = f.borrow_mut().take() {
                                    f(r);
                                }
                            }),
                        );
                        if let Err(e) = started {
                            if let Some(f) = finish.borrow_mut().take() {
                                f(Err(e));
                            }
                        }
                    }
                    Err(e) => finish(Err(format!("The pages' data wasn't cleared ({e})."))),
                }
                Ok(())
            }));
            unsafe { env10.CreateCoreWebView2ControllerWithOptions(host, &opts, &handler) }.map_err(|e| {
                close_one(&a, &key);
                format!("The pages' data wasn't cleared ({}).", e.message())
            })
        })
        .await??;
        match tokio::time::timeout(std::time::Duration::from_secs(30), rx).await {
            Ok(Ok(r)) => r,
            Ok(Err(_)) => Err("The pages' data wasn't cleared: WebView2 didn't answer.".into()),
            Err(_) => Err("Clearing the pages' data didn't finish in time.".into()),
        }
    }

    /// The shot of page `key` as it is now.
    pub async fn shot_of(app: AppHandle, key: String) -> Result<Vec<u8>, String> {
        let (tx, rx) = tokio::sync::oneshot::channel::<Result<Vec<u8>, String>>();
        main_thread(&app, move |_| {
            let Some((_, _, Some(core))) = handles(&key) else { return Err("That page isn't open.".to_string()) };
            capture(
                &core,
                Box::new(move |res| {
                    let _ = tx.send(res);
                }),
            )
        })
        .await??;
        match tokio::time::timeout(SHOT_TIMEOUT, rx).await {
            Ok(Ok(res)) => res,
            _ => Err("The shot didn't come out in time: is Yap's window on screen?".into()),
        }
    }

    /// **What's in view of page `key` now** (the model's shot): on screen,
    /// as it is; out of sight, its host put under Yap's own page at the
    /// panel's size for a moment (drawn, never seen: the shot's frame is
    /// what the person would see), then out of sight again — unless the
    /// person brought its tab up meanwhile.
    pub async fn shot_in_view(app: AppHandle, key: String) -> Result<Vec<u8>, String> {
        let k = key.clone();
        let drawn = main_thread(&app, move |_| {
            let Some((host, Some(controller), Some(_))) = handles(&k) else { return Err("That page isn't open (yet).".to_string()) };
            let on_screen = VIEWS.with(|v| v.borrow().iter().find(|x| x.key == k).is_some_and(|x| x.bounds.is_some()));
            if !on_screen {
                let (w, h) = STAGE.with(Cell::get);
                put(host, Some(&controller), Some(Bounds { x: 0, y: 0, w, h }), true);
            }
            Ok(!on_screen)
        })
        .await??;
        if drawn {
            // A frame drawn at that size first.
            tokio::time::sleep(DRAW).await;
        }
        let shot = shot_of(app.clone(), key.clone()).await;
        if drawn {
            let _ = main_thread(&app, move |_| {
                let still_hidden = VIEWS.with(|v| v.borrow().iter().find(|x| x.key == key).map(|x| (x.bounds.is_none(), x.host, x.controller.clone())));
                if let Some((true, host, controller)) = still_hidden {
                    put(host, controller.as_ref(), None, false);
                }
            })
            .await;
        }
        shot
    }

    /// **The view drawn `scale` (1–2) times finer, out of sight** (see the
    /// public `shot_in_view_at`): its controller's rasterization scale and
    /// bounds both times `scale` under Yap's own page (the CSS layout the
    /// same), a frame drawn, the shot, then its scale back and the page out
    /// of sight again — or where the person brought its tab up meanwhile. On
    /// screen, or where the scale can't be set: [`shot_in_view`], scale 1.
    pub async fn shot_in_view_at(app: AppHandle, key: String, scale: f64) -> Result<(Vec<u8>, f64), String> {
        let scale = if scale.is_finite() { scale.clamp(1.0, 2.0) } else { 1.0 };
        let k = key.clone();
        let finer = main_thread(&app, move |_| -> Result<Option<f64>, String> {
            let Some((host, Some(controller), Some(_))) = handles(&k) else { return Err("That page isn't open (yet).".to_string()) };
            let on_screen = VIEWS.with(|v| v.borrow().iter().find(|x| x.key == k).is_some_and(|x| x.bounds.is_some()));
            if on_screen || scale <= 1.0 {
                return Ok(None);
            }
            let Ok(c3) = controller.cast::<ICoreWebView2Controller3>() else { return Ok(None) };
            let mut was = 0.0f64;
            if unsafe { c3.RasterizationScale(&mut was) }.is_err() || !(was > 0.0 && was.is_finite()) {
                return Ok(None);
            }
            if unsafe { c3.SetRasterizationScale(was * scale) }.is_err() {
                return Ok(None);
            }
            let (w, h) = STAGE.with(Cell::get);
            let finer = |n: i32| (f64::from(n) * scale).round() as i32;
            put(host, Some(&controller), Some(Bounds { x: 0, y: 0, w: finer(w), h: finer(h) }), true);
            Ok(Some(was))
        })
        .await??;
        let Some(was) = finer else {
            return shot_in_view(app, key).await.map(|png| (png, 1.0));
        };
        // A frame drawn at that scale first (laid out again for its new
        // pixel ratio: a little longer than a plain shot's).
        tokio::time::sleep(DRAW + std::time::Duration::from_millis(200)).await;
        let shot = shot_of(app.clone(), key.clone()).await;
        let _ = main_thread(&app, move |_| {
            let Some((host, controller, _)) = handles(&key) else { return };
            if let Some(c3) = controller.as_ref().and_then(|c| c.cast::<ICoreWebView2Controller3>().ok()) {
                let _ = unsafe { c3.SetRasterizationScale(was) };
            }
            let bounds = VIEWS.with(|v| v.borrow().iter().find(|x| x.key == key).and_then(|x| x.bounds));
            put(host, controller.as_ref(), bounds, false);
        })
        .await;
        tracing::info!(scale, ok = shot.is_ok(), "Chat: a page out of sight drawn finer for a zoom");
        shot.map(|png| (png, scale))
    }

    /// The page's menu: the safe items, and Yap's.
    fn menu(app: &AppHandle, key: &str, env: &ICoreWebView2Environment, args: &ICoreWebView2ContextMenuRequestedEventArgs) -> windows::core::Result<()> {
        // A test run never shows a native menu.
        if crate::shell::e2e::active() {
            unsafe { args.SetHandled(true)? };
            return Ok(());
        }
        let items = unsafe { args.MenuItems()? };
        let mut count = 0u32;
        unsafe { items.Count(&mut count)? };
        let mut read: Vec<(String, bool)> = Vec::with_capacity(count as usize);
        for i in 0..count {
            let item = unsafe { items.GetValueAtIndex(i)? };
            let mut name = PWSTR::null();
            unsafe { item.Name(&mut name)? };
            let mut kind = COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND(0);
            unsafe { item.Kind(&mut kind)? };
            read.push((take_pwstr(name), kind == COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_SEPARATOR));
        }
        let names: Vec<(&str, bool)> = read.iter().map(|(n, s)| (n.as_str(), *s)).collect();
        let kept = menu_keep(&names);
        for i in (0..count).rev() {
            if !kept.contains(&(i as usize)) {
                unsafe { items.RemoveValueAtIndex(i)? };
            }
        }
        let env9: ICoreWebView2Environment9 = env.cast()?;
        let target = unsafe { args.ContextMenuTarget()? };
        let mut has = BOOL(0);
        let _ = unsafe { target.HasSelection(&mut has) };
        let selection = if has.as_bool() {
            let mut s = PWSTR::null();
            unsafe { target.SelectionText(&mut s)? };
            take_pwstr(s)
        } else {
            String::new()
        };
        let item = |label: &str, f: Box<dyn FnMut()>| -> windows::core::Result<ICoreWebView2ContextMenuItem> {
            let it = unsafe { env9.CreateContextMenuItem(&HSTRING::from(label), None::<&IStream>, COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_COMMAND)? };
            let mut f = f;
            let mut token = 0i64;
            unsafe {
                it.add_CustomItemSelected(
                    &CustomItemSelectedEventHandler::create(Box::new(move |_, _| {
                        f();
                        Ok(())
                    })),
                    &mut token,
                )?
            };
            Ok(it)
        };
        let separator = || unsafe { env9.CreateContextMenuItem(w!(""), None::<&IStream>, COREWEBVIEW2_CONTEXT_MENU_ITEM_KIND_SEPARATOR) };
        let mut at = 0u32;
        if !selection.trim().is_empty() {
            let (a, k, t) = (app.clone(), key.to_string(), selection.clone());
            let ask = item("Ask Yap about this", Box::new(move || quote(&a, &k, &t)))?;
            let (a, t) = (app.clone(), selection.clone());
            let read_aloud = item(
                "Read aloud",
                Box::new(move || {
                    let (a, t) = (a.clone(), t.clone());
                    std::thread::spawn(move || {
                        let start = crate::voice::read_aloud::Start { text: t, markdown: false, message_id: None, source: crate::voice::read_aloud::Source::Yap };
                        if let Err(e) = crate::voice::read_aloud::start(&a, start) {
                            tracing::info!("A page's selection wasn't read aloud: {e}");
                        }
                    });
                }),
            )?;
            unsafe {
                items.InsertValueAtIndex(at, &ask)?;
                items.InsertValueAtIndex(at + 1, &read_aloud)?;
            }
            at += 2;
            if !kept.is_empty() {
                unsafe { items.InsertValueAtIndex(at, &separator()?)? };
            }
        }
        let a = app.clone();
        let snip = item(
            "Snip to Chat",
            Box::new(move || {
                let a = a.clone();
                std::thread::spawn(move || crate::snip::start(&a, crate::snip::Origin::Chat));
            }),
        )?;
        let mut n = 0u32;
        unsafe { items.Count(&mut n)? };
        unsafe {
            if n > 0 {
                items.InsertValueAtIndex(n, &separator()?)?;
                n += 1;
            }
            items.InsertValueAtIndex(n, &snip)?;
        }
        Ok(())
    }

    /// "Ask Yap about this": the words, for Chat's box (its page told which
    /// page), and the focus back on Yap's own page. (On the main thread.)
    fn quote(app: &AppHandle, key: &str, text: &str) {
        let text: String = text.trim().chars().take(4000).collect();
        if text.is_empty() {
            return;
        }
        let (standin, _) = super::web_route();
        let (url, title) = handles(key)
            .and_then(|(_, _, core)| core)
            .map(|core| unsafe {
                let mut s = PWSTR::null();
                let url = if core.Source(&mut s).is_ok() { shown_url(&take_pwstr(s), standin.as_deref()) } else { String::new() };
                let mut t = PWSTR::null();
                let title = if core.DocumentTitle(&mut t).is_ok() { take_pwstr(t) } else { String::new() };
                (url, title)
            })
            .unwrap_or_default();
        let site = crate::web::site_of(&url);
        tracing::info!(chars = text.chars().count(), "Chat: words from a page asked about");
        let _ = app.emit_to("settings", QUOTE_EVENT, serde_json::json!({ "key": key, "text": text, "url": url, "title": title, "site": site }));
        if let Some((_, root)) = ENV.with(|c| c.borrow().clone()) {
            unsafe {
                let _ = root.MoveFocus(COREWEBVIEW2_MOVE_FOCUS_REASON_PROGRAMMATIC);
            }
        }
    }

    /// "Ask Yap about this" from off the main thread (the suite's).
    #[cfg(debug_assertions)]
    pub async fn ask(app: AppHandle, key: String, text: String) -> Result<(), String> {
        let a = app.clone();
        main_thread(&app, move |_| quote(&a, &key, &text)).await
    }

    /// The page tabs alive now (not a shot's or a clearing's page), in the
    /// order they opened, each with its own number; the one on screen in
    /// front, else the one shown last.
    pub async fn tabs(app: AppHandle) -> Result<Vec<super::OpenTab>, String> {
        main_thread(&app, |_| {
            let (standin, _) = super::web_route();
            type Alive = (usize, String, String, Option<ICoreWebView2>, bool, u64, Option<u32>);
            let alive: Vec<Alive> = VIEWS.with(|v| {
                v.borrow()
                    .iter()
                    .filter(|x| x.snap.is_none() && !x.key.starts_with("snap:") && !x.key.starts_with("clear:"))
                    .map(|x| (x.id, x.key.clone(), x.url.clone(), x.core.clone(), x.bounds.is_some(), x.used, x.width))
                    .collect()
            });
            let shown = alive.iter().any(|(_, _, _, _, on, _, _)| *on);
            let last = alive.iter().map(|(_, _, _, _, _, used, _)| *used).max().unwrap_or(0);
            alive
                .into_iter()
                .map(|(id, key, url, core, on, used, width)| {
                    let (mut now, mut title) = (url, String::new());
                    if let Some(core) = core {
                        unsafe {
                            let mut s = PWSTR::null();
                            if core.Source(&mut s).is_ok() {
                                let src = take_pwstr(s);
                                if !src.is_empty() && src != "about:blank" {
                                    now = shown_url(&src, standin.as_deref());
                                }
                            }
                            let mut t = PWSTR::null();
                            if core.DocumentTitle(&mut t).is_ok() {
                                title = take_pwstr(t);
                            }
                        }
                    }
                    super::OpenTab { tab: id, key, title, url: now, front: if shown { on } else { used == last }, width }
                })
                .collect()
        })
        .await
    }

    pub async fn eval(app: AppHandle, key: String, script: String) -> Result<String, String> {
        let (tx, rx) = tokio::sync::oneshot::channel::<Result<String, String>>();
        main_thread(&app, move |_| {
            let Some((_, _, Some(core))) = handles(&key) else { return Err("That page isn't open (yet).".to_string()) };
            let mut tx = Some(tx);
            unsafe {
                core.ExecuteScript(
                    &HSTRING::from(script),
                    &ExecuteScriptCompletedHandler::create(Box::new(move |res, json| {
                        if let Some(tx) = tx.take() {
                            let _ = tx.send(res.map(|()| json).map_err(|e| e.message()));
                        }
                        Ok(())
                    })),
                )
            }
            .map_err(|e| e.message())
        })
        .await??;
        rx.await.map_err(|_| "The page didn't answer.".to_string())?
    }

    #[cfg(debug_assertions)]
    pub async fn list(app: AppHandle) -> Result<serde_json::Value, String> {
        main_thread(&app, |_| {
            serde_json::Value::Array(VIEWS.with(|v| {
                v.borrow()
                    .iter()
                    .map(|x| serde_json::json!({ "key": x.key, "tab": x.id, "url": x.url, "load": x.load, "onScreen": x.bounds.is_some(), "loaded": x.loaded, "made": x.controller.is_some(), "width": x.width, "zoom": x.zoom }))
                    .collect()
            }))
        })
        .await
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    const STANDIN: &str = "http://127.0.0.1:5555/standin";

    #[test]
    fn a_shortcut_on_a_key_that_types_is_watched_for_while_a_page_has_the_focus() {
        // "9", "A", space, ";" and the number pad's 5 type: WebView2 never hands them over.
        for vk in [0x39, 0x41, 0x20, 0xBA, 0x65] {
            assert!(types_a_character(vk), "{vk:#x}");
        }
        // F9, F24, Esc, an arrow and the modifiers come as accelerators (or aren't keys of their own).
        for vk in [0x78, 0x87, 0x1B, 0x25, 0x10, 0x11, 0x12, 0x5B] {
            assert!(!types_a_character(vk), "{vk:#x}");
        }
    }

    #[test]
    fn the_address_bar_opens_an_address_or_searches_the_persons_engine() {
        // An address as typed, https:// added when it has none.
        assert_eq!(address("https://wiki.example.com/wiki/Fort_Joy", "google").unwrap(), "https://wiki.example.com/wiki/Fort_Joy");
        assert_eq!(address("  wiki.example.com/wiki/Fort_Joy ", "google").unwrap(), "https://wiki.example.com/wiki/Fort_Joy");
        assert_eq!(address("www.example.co.uk", "google").unwrap(), "https://www.example.co.uk");
        assert_eq!(address("example.com:8443/x", "google").unwrap(), "https://example.com:8443/x");
        assert_eq!(address("HTTP://Example.com", "google").unwrap(), "HTTP://Example.com");
        // Words: a search on the person's engine (DuckDuckGo when it's not one Yap knows).
        assert_eq!(address("fort joy map", "google").unwrap(), "https://www.google.com/search?q=fort+joy+map");
        assert_eq!(address("fort joy map", "nonsense").unwrap(), "https://duckduckgo.com/?q=fort+joy+map");
        assert_eq!(address("3.14", "bing").unwrap(), "https://www.bing.com/search?q=3.14");
        assert_eq!(address("e.g.", "bing").unwrap(), "https://www.bing.com/search?q=e.g.");
        assert_eq!(address("what is: a colon", "bing").unwrap(), "https://www.bing.com/search?q=what+is%3A+a+colon");
        // Never another scheme; nothing typed, nothing opened.
        for bad in ["javascript:alert(1)", "file:///C:/x", "about:blank", "ms-settings:privacy", "data:text/html,x", "view-source:https://example.com"] {
            assert!(address(bad, "google").unwrap_err().contains("Only web pages open here"), "{bad}");
        }
        assert!(address("   ", "google").is_err());
        // Whether it may open is load_url's: this PC's addresses never do.
        let local = address("192.168.1.5/admin", "google").unwrap();
        assert_eq!(local, "https://192.168.1.5/admin");
        assert!(load_url(&local, None, false).is_err());
    }

    #[test]
    fn a_page_loads_only_from_a_public_web_address_or_a_test_runs_stand_in() {
        assert_eq!(load_url("https://wiki.example.com/wiki/Fort_Joy#Map", None, false).unwrap(), "https://wiki.example.com/wiki/Fort_Joy#Map");
        assert_eq!(load_url("https://wiki.example.com/w/index.php?title=Fort_Joy", Some(STANDIN), true).unwrap(), format!("{STANDIN}/wiki.example.com/w/index.php?title=Fort_Joy"));
        assert!(load_url("https://wiki.example.com/x", None, true).unwrap_err().contains("never go on the internet"));
        for bad in ["http://127.0.0.1/admin", "http://192.168.1.5/", "http://nas.local/", "file:///C:/x", "javascript:alert(1)", "https://a.example:8443/"] {
            assert!(load_url(bad, None, false).is_err(), "{bad}");
        }
        // Its navigations: public pages, never this PC (a test run: only the stand-in).
        assert!(may_navigate("https://maps.example.com/fort-joy?layer=chests", None, false));
        assert!(!may_navigate("http://localhost:3000/", None, false));
        assert!(!may_navigate("ms-settings:privacy", None, false));
        assert!(!may_navigate("data:text/html,<script>1</script>", None, false));
        assert!(may_navigate(&format!("{STANDIN}/wiki.example.com/wiki/Fort_Joy"), Some(STANDIN), true));
        assert!(!may_navigate("https://maps.example.com/", Some(STANDIN), true), "a test run never goes out");
        assert!(!may_navigate("https://maps.example.com/", None, true));
        // A tab shows the page's own address, the stand-in's turned back.
        assert_eq!(shown_url(&format!("{STANDIN}/wiki.example.com/wiki/Fort_Joy"), Some(STANDIN)), "https://wiki.example.com/wiki/Fort_Joy");
        assert_eq!(shown_url("https://maps.example.com/x", None), "https://maps.example.com/x");
    }

    #[test]
    fn the_section_script_carries_the_words_safely() {
        let s = section_script("Treasure \"chests\"</script>");
        assert!(s.contains(r#"("Treasure \"chests\"</script>")"#), "{s}");
        assert!(s.starts_with("(function(s){") && s.contains("scrollIntoView") && s.contains("window.find"));
        assert!(section_script("  ").ends_with(r#"("")"#));
    }

    #[test]
    fn the_least_recently_shown_page_goes_first() {
        let v = |k: &str, used: u64, on: bool| (k.to_string(), used, on);
        assert_eq!(evict(&[v("a", 1, false), v("b", 2, false)]), None, "room");
        assert_eq!(evict(&[v("a", 5, true), v("b", 2, false), v("c", 9, false), v("d", 3, false)]).as_deref(), Some("b"));
        // Never the one on screen while another can go.
        assert_eq!(evict(&[v("a", 1, true), v("b", 2, false), v("c", 9, false), v("d", 3, false)]).as_deref(), Some("b"));
        assert_eq!(evict(&[v("a", 1, true), v("b", 2, true), v("c", 9, true), v("d", 3, true)]).as_deref(), Some("a"));
    }

    #[test]
    fn a_desktop_width_is_laid_out_in_the_panel_by_zooming_it_to_fit() {
        // Yap Lab's panel: 460 CSS px at 100 %, 690 device px at 150 %. A
        // 1280 px layout in either is the same zoom of what fits.
        let z = zoom_for(460, 1.0, 1280);
        assert!((z - 460.0 / 1280.0).abs() < 1e-9, "{z}");
        assert!((laid_out_width(460, 1.0, z) - 1280.0).abs() < 1e-6);
        let z15 = zoom_for(690, 1.5, 1280);
        assert!((z15 - z).abs() < 1e-9 && (laid_out_width(690, 1.5, z15) - 1280.0).abs() < 1e-6);
        // Drawn twice as finely out of sight (bounds and scale doubled): the same layout.
        assert!((zoom_for(1380, 3.0, 1280) - z15).abs() < 1e-9);
        // Never zoomed in (a width the panel already has is its own) …
        assert_eq!(zoom_for(1400, 1.0, 1280), 1.0);
        // … nor past WebView2's floor: a panel too narrow lays it out as wide as that allows.
        assert_eq!(zoom_for(240, 1.0, 1280), ZOOM_MIN);
        assert!((laid_out_width(240, 1.0, ZOOM_MIN) - 960.0).abs() < 1e-6);
        // Nonsense in, the page as it is.
        assert_eq!(zoom_for(0, 1.0, 1280), 1.0);
        assert_eq!(zoom_for(460, f64::NAN, 1280), zoom_for(460, 1.0, 1280));
        assert_eq!(zoom_for(460, 1.0, 0), 1.0);
        // The tab's state says it, for its chip; a page at the panel's width says nothing.
        let s = serde_json::to_value(ViewState { key: "page:https://example.com".into(), width: Some(1280), ..Default::default() }).unwrap();
        assert_eq!(s["width"], 1280);
        assert!(serde_json::to_value(ViewState::default()).unwrap().get("width").is_none());
    }

    #[test]
    fn the_pages_menu_keeps_moving_about_and_copying() {
        let items = [
            ("back", false),
            ("forward", false),
            ("reload", false),
            ("", true),
            ("saveAs", false),
            ("print", false),
            ("", true),
            ("openLinkInNewWindow", false),
            ("copyLinkLocation", false),
            ("", true),
            ("", true),
            ("inspectElement", false),
        ];
        let kept: Vec<&str> = menu_keep(&items).into_iter().map(|i| if items[i].1 { "—" } else { items[i].0 }).collect();
        assert_eq!(kept, ["back", "forward", "reload", "—", "copyLinkLocation"]);
        assert!(menu_keep(&[("", true), ("saveImageAs", false), ("", true)]).is_empty());
        assert_eq!(error_words(13), "The page couldn't be found.");
        assert_eq!(error_words(99), "The page didn't load.");
    }

    #[test]
    fn a_pages_requests_to_this_pc_or_its_network_are_refused() {
        // This PC however it's written, the home network, link-local, unique-local.
        for bad in [
            "http://127.0.0.1:8080/v1/chat/completions",
            "http://127.0.0.2/",
            "http://[::1]/",
            "http://[::ffff:7f00:1]/",
            "http://[::ffff:127.0.0.1]:51437/",
            "http://2130706433/",
            "http://0x7f.0.0.1/",
            "http://0.0.0.0:3000/",
            "http://192.168.1.1/admin",
            "https://10.0.0.5/",
            "http://172.20.1.1/",
            "http://169.254.169.254/latest/meta-data",
            "http://[fe80::1]/",
            "http://[fd12:3456::1]/",
            "http://100.64.0.1/",
            "ws://localhost:9222/devtools",
            "http://localhost/",
            "http://yap.localhost/",
            "http://printer.local/",
            "http://router/",
        ] {
            assert!(refused_request(bad, None), "{bad}");
        }
        // Public pages, their pictures and scripts; addresses that aren't requests to a host.
        for ok in ["https://wiki.example.com/wiki/Fort_Joy", "https://cdn.example.net/a.js", "https://93.184.216.34/", "https://[2606:4700::1111]/", "data:image/png;base64,iVBOR", "blob:https://wiki.example.com/1234", "about:blank"] {
            assert!(!refused_request(ok, None), "{ok}");
        }
        // A test run's stand-in is the page itself.
        assert!(!refused_request(&format!("{STANDIN}/wiki.example.com/x"), Some(STANDIN)));
        assert!(refused_request("http://127.0.0.1:5555/other", Some(STANDIN)));
    }

    #[tokio::test]
    async fn yap_acting_on_a_page_holds_while_it_acts_and_a_moment_after() {
        let key = "page:https://dialogs.example/acting";
        assert!(!acting_on(key));
        let busy = busy(key).await;
        assert!(acting_on(key) && !acting_on("page:https://dialogs.example/other"));
        drop(busy);
        assert!(acting_on(key), "a dialog a click opens comes a moment later");
        ACTING.lock().unwrap().as_mut().unwrap().get_mut(key).unwrap().1 = Some(std::time::Instant::now());
        assert!(!acting_on(key));
        // Tab numbers are never used twice.
        let (a, b) = (next_tab(), next_tab());
        assert!(b > a);
    }

    #[test]
    fn words_selected_on_a_page_are_asked_as_a_selection() {
        let q = PageQuote { text: "  Chests behind the waterfall \n".into(), site: "wiki.example.com".into() };
        assert_eq!(q.selected(), Some(Selected { text: "Chests behind the waterfall".into(), app: "wiki.example.com".into(), web: true }));
        assert_eq!(PageQuote { text: " ".into(), site: "x".into() }.selected(), None);
    }
}

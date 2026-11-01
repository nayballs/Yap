//! **The page pane**: a live web page beside the chat — the panel's Page
//! tab (`show_page`, `chat/pages.rs`) — and the out-of-sight page a shot is
//! taken of (`snap_page`).
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
//!   as a page read is (`web::check_url`) and cancelled otherwise; requests
//!   to this PC's loopback addresses are refused (`WebResourceRequested`);
//! - **new windows go to the person's browser** (`open_external`'s, only
//!   when the person clicked), **downloads are off**, every permission a
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
//! **A shot** (`page_view_shot`, the tab's Save a shot; [`snap`], the
//! model's `snap_page`): `ICoreWebView2::CapturePreview` as a PNG — the
//! page as it is in the panel, or loaded for the shot in a host under
//! Yap's own page (out of sight, 1280 × 800, at 100 %) and closed after.
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

/// Keep the app for the model's `snap_page` (it runs without one).
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

/// The event a page's state goes out on (to the main window).
pub const EVENT: &str = "yap-page-view";
/// The event "Ask Yap about this" goes out on.
pub const QUOTE_EVENT: &str = "yap-page-quote";
/// The WebView2 profile every page is in: its own cookies and storage.
pub const PROFILE: &str = "YapPages";
/// Pages kept alive at once.
pub const MAX_VIEWS: usize = 4;
/// A shot's page, in CSS pixels (at 100 %).
pub const SNAP_W: i32 = 1280;
pub const SNAP_H: i32 = 800;
/// How long a shot's page may take to load…
const LOAD_TIMEOUT: Duration = Duration::from_secs(20);
/// …the time it's given after, for its pictures and scripts…
const SETTLE: Duration = Duration::from_millis(1200);
/// …and how long the shot itself may take.
const SHOT_TIMEOUT: Duration = Duration::from_secs(10);

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
    /// and the site. `None`: nothing selected.
    pub fn selected(self) -> Option<Selected> {
        let text = self.text.trim();
        (!text.is_empty()).then(|| Selected { text: text.to_string(), app: self.site.trim().to_string() })
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
    imp::show(app, key, url, load, section, bounds).await
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

/// **A page tab, as the model is told it**: its number (1, 2… in the order
/// the tabs opened), title and address, and whether it's the one in front.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct OpenTab {
    pub tab: usize,
    /// Its tab's key (never sent to the model: its address is).
    #[serde(skip)]
    pub key: String,
    pub title: String,
    pub url: String,
    pub front: bool,
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
/// model wrote): its result as JSON.
pub async fn run_script(key: &str, script: String) -> Result<String, String> {
    let app = APP.get().cloned().ok_or("Yap's window isn't ready.")?;
    imp::eval(app, key.to_string(), script).await
}

/// What a page says, for the model (`read_open_page`): its title, its
/// headings, its main text (the `main` / `article` element's, else the
/// whole page's), whitespace folded. Read only.
pub const PAGE_TEXT_SCRIPT: &str = r#"(function(){var t=function(s){return (s||'').replace(/\s+/g,' ').trim();};var m=document.querySelector('main,article,[role=main]')||document.body;if(!m)return {title:document.title,headings:[],text:''};var hs=[].slice.call(m.querySelectorAll('h1,h2,h3')).slice(0,40).map(function(h){return t(h.innerText);}).filter(Boolean);return {title:t(document.title),headings:hs,text:t(m.innerText).slice(0,24000)};})()"#;

/// A page's words, read by [`PAGE_TEXT_SCRIPT`].
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PageText {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub headings: Vec<String>,
    #[serde(default)]
    pub text: String,
}

/// **Read page `key`**: its title, headings and main text.
pub async fn page_text(key: &str) -> Result<PageText, String> {
    let raw = run_script(key, PAGE_TEXT_SCRIPT.to_string()).await?;
    serde_json::from_str(&raw).map_err(|_| "The page couldn't be read.".to_string())
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

/// **The model's `snap_page`**: the page's shot as a PNG — as it is in the
/// panel when it's open there, else loaded out of sight for it.
pub async fn snap(url: String) -> Result<Vec<u8>, String> {
    let app = APP.get().cloned().ok_or("Yap's window isn't ready to take a shot.")?;
    let (standin, offline) = web_route();
    let load = load_url(&url, standin.as_deref(), offline)?;
    imp::snap(app, url, load).await
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

    pub async fn show(_: AppHandle, _: String, _: String, _: String, _: String, _: Option<Bounds>) -> Result<(), String> {
        Err(NO.into())
    }
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
    pub async fn snap(_: AppHandle, _: String, _: String) -> Result<Vec<u8>, String> {
        Err(NO.into())
    }
    pub async fn eval(_: AppHandle, _: String, _: String) -> Result<String, String> {
        Err(NO.into())
    }
    pub async fn tabs(_: AppHandle) -> Result<Vec<super::OpenTab>, String> {
        Ok(Vec::new())
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

    use tauri::{AppHandle, Emitter, Manager};
    use webview2_com::Microsoft::Web::WebView2::Win32::*;
    use webview2_com::{
        take_pwstr, AcceleratorKeyPressedEventHandler, CapturePreviewCompletedHandler, ClearBrowsingDataCompletedHandler, ContextMenuRequestedEventHandler, CreateCoreWebView2ControllerCompletedHandler,
        CustomItemSelectedEventHandler, DocumentTitleChangedEventHandler, DownloadStartingEventHandler, ExecuteScriptCompletedHandler,
        HistoryChangedEventHandler, NavigationCompletedEventHandler, NavigationStartingEventHandler, NewWindowRequestedEventHandler,
        PermissionRequestedEventHandler, ProcessFailedEventHandler, WebResourceRequestedEventHandler,
    };
    use windows::core::{w, Interface, BOOL, HSTRING, PWSTR};
    use windows::Win32::Foundation::{HGLOBAL, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{GetStockObject, HBRUSH, WHITE_BRUSH};
    use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
    use windows::Win32::System::Com::{IStream, STREAM_SEEK_SET};
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowLongPtrW, RegisterClassExW, SetWindowPos, ShowWindow, GWLP_HINSTANCE,
        HWND_BOTTOM, HWND_TOP, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, WINDOW_EX_STYLE, WNDCLASSEXW, WS_CHILD, WS_CLIPCHILDREN,
        WS_CLIPSIBLINGS,
    };

    use super::{error_words, evict, may_navigate, menu_keep, section_script, shown_url, Bounds, ViewState, EVENT, LOAD_TIMEOUT, PROFILE, QUOTE_EVENT, SETTLE, SHOT_TIMEOUT, SNAP_H, SNAP_W};

    type Done<T> = tokio::sync::oneshot::Sender<Result<T, String>>;

    /// One page: its host window, its controller once made, where it should
    /// be, and anything waiting for it to load.
    struct View {
        key: String,
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
    }

    thread_local! {
        static VIEWS: RefCell<Vec<View>> = const { RefCell::new(Vec::new()) };
        /// Yap's WebView2 environment (the main window's) and its controller.
        static ENV: RefCell<Option<(ICoreWebView2Environment, ICoreWebView2Controller)>> = const { RefCell::new(None) };
        static CLASS: Cell<bool> = const { Cell::new(false) };
        static TICK: Cell<u64> = const { Cell::new(0) };
        static SNAPS: Cell<u64> = const { Cell::new(0) };
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

    /// Put `host` (and its controller) over `bounds`, on top; or out of sight.
    fn put(host: HWND, controller: Option<&ICoreWebView2Controller>, bounds: Option<Bounds>, snap: bool) {
        unsafe {
            match bounds {
                Some(b) => {
                    let (w, h) = (b.w.max(1), b.h.max(1));
                    // A shot's page sits under Yap's own page: drawn, never seen.
                    let z = if snap { HWND_BOTTOM } else { HWND_TOP };
                    let (ox, oy) = if snap { (0, 0) } else { page_origin() };
                    let _ = SetWindowPos(host, Some(z), b.x + ox, b.y + oy, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
                    if let Some(c) = controller {
                        let _ = c.SetBounds(RECT { left: 0, top: 0, right: w, bottom: h });
                        let _ = c.SetIsVisible(true);
                    }
                }
                None => {
                    let _ = ShowWindow(host, SW_HIDE);
                    if let Some(c) = controller {
                        let _ = c.SetIsVisible(false);
                    }
                }
            }
        }
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
        let mut state = ViewState { key: key.to_string(), loading, error, ..Default::default() };
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

    pub async fn show(app: AppHandle, key: String, url: String, load: String, section: String, bounds: Option<Bounds>) -> Result<(), String> {
        let a = app.clone();
        main_thread(&app, move |root| make_or_place(&a, root, key, url, load, section, bounds, None)).await?
    }

    /// Show page `key` (made the first time); a shot's page (`snap`) is made
    /// out of sight and its shot taken once it has loaded.
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
    ) -> Result<(), String> {
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
            return Ok(());
        }
        // Room for one more: the least recently shown one goes.
        if snap.is_none() {
            let alive: Vec<(String, u64, bool)> =
                VIEWS.with(|v| v.borrow().iter().filter(|x| x.snap.is_none() && !x.key.starts_with("snap:") && !x.key.starts_with("clear:")).map(|x| (x.key.clone(), x.used, x.bounds.is_some())).collect());
            if let Some(gone) = evict(&alive) {
                tracing::info!(views = alive.len(), "Chat: the least recently shown page closed to make room");
                close_one(app, &gone);
            }
        }
        let env = env(root).map_err(|e| format!("Yap couldn't reach its webview: {e}"))?;
        let parent = parent(app)?;
        let host = host(parent)?;
        let is_snap = snap.is_some();
        VIEWS.with(|v| {
            v.borrow_mut().push(View {
                key: key.clone(),
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
        Ok(())
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
    /// navigations checked, new windows to the person's browser, downloads
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
            // A new window: none here; a link the person clicked opens in
            // their browser.
            let a = app.clone();
            core.add_NewWindowRequested(
                &NewWindowRequestedEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    args.SetHandled(true)?;
                    let mut by_person = BOOL(0);
                    let _ = args.IsUserInitiated(&mut by_person);
                    let mut uri = PWSTR::null();
                    args.Uri(&mut uri)?;
                    let uri = take_pwstr(uri);
                    if by_person.as_bool() && crate::web::check_url(&uri).is_ok() {
                        tracing::info!("Chat: a page's new window went to the user's browser");
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
            // This PC's loopback addresses: refused (Yap's own helpers listen
            // there). A test run's stand-in is the page itself.
            for filter in [w!("*://127.0.0.1*"), w!("*://localhost*"), w!("*://[::1]*"), w!("*.localhost*")] {
                core.AddWebResourceRequestedFilter(filter, COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL)?;
            }
            let (e, si) = (env.clone(), standin.clone());
            core.add_WebResourceRequested(
                &WebResourceRequestedEventHandler::create(Box::new(move |_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut uri = PWSTR::null();
                    args.Request()?.Uri(&mut uri)?;
                    let uri = take_pwstr(uri);
                    if si.as_deref().is_some_and(|base| uri.starts_with(&format!("{base}/"))) {
                        return Ok(());
                    }
                    let refused = e.CreateWebResourceResponse(None::<&IStream>, 403, w!("Forbidden"), w!(""))?;
                    args.SetResponse(&refused)?;
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
            // Yap's shortcuts while the page has the focus: WebView2 runs
            // ahead of the keyboard hook (as in Yap's own windows), so its
            // function keys and Ctrl / Alt combos go to the main window's
            // page, which takes them as its own keys (settings/
            // hotkeyFallback.svelte.js). Only the key and its modifiers.
            if !snap {
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
        let _ = app;
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
                None => make_or_place(&a, root, key, url, load, String::new(), bounds, None),
            }
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

    /// **The model's shot of `url`**: the panel's page when it's that one
    /// and has loaded, else the page loaded under Yap's own for it.
    pub async fn snap(app: AppHandle, url: String, load: String) -> Result<Vec<u8>, String> {
        let want = crate::web::normal_url(&url);
        let open = {
            let w = want.clone();
            main_thread(&app, move |_| {
                VIEWS.with(|v| v.borrow().iter().find(|x| x.loaded && x.bounds.is_some() && x.snap.is_none() && crate::web::normal_url(&x.url) == w).map(|x| x.key.clone()))
            })
            .await?
        };
        if let Some(key) = open {
            return shot_of(app, key).await;
        }
        let key = format!("snap:{}", SNAPS.with(|s| {
            s.set(s.get() + 1);
            s.get()
        }));
        let (tx, rx) = tokio::sync::oneshot::channel::<Result<Vec<u8>, String>>();
        let (a, k) = (app.clone(), key.clone());
        let bounds = Some(Bounds { x: 0, y: 0, w: SNAP_W, h: SNAP_H });
        main_thread(&app, move |root| make_or_place(&a, root, k, url, load, String::new(), bounds, Some(tx))).await??;
        match tokio::time::timeout(LOAD_TIMEOUT + SETTLE + SHOT_TIMEOUT, rx).await {
            Ok(Ok(res)) => res,
            Ok(Err(_)) => Err("The page closed before its shot.".into()),
            Err(_) => {
                let a = app.clone();
                let _ = main_thread(&app, move |_| close_one(&a, &key)).await;
                Err("The page didn't load in time.".into())
            }
        }
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
    /// order they opened; the one on screen in front, else the one shown last.
    pub async fn tabs(app: AppHandle) -> Result<Vec<super::OpenTab>, String> {
        main_thread(&app, |_| {
            let (standin, _) = super::web_route();
            let alive: Vec<(String, String, Option<ICoreWebView2>, bool, u64)> = VIEWS.with(|v| {
                v.borrow()
                    .iter()
                    .filter(|x| x.snap.is_none() && !x.key.starts_with("snap:") && !x.key.starts_with("clear:"))
                    .map(|x| (x.key.clone(), x.url.clone(), x.core.clone(), x.bounds.is_some(), x.used))
                    .collect()
            });
            let shown = alive.iter().any(|(_, _, _, on, _)| *on);
            let last = alive.iter().map(|(_, _, _, _, used)| *used).max().unwrap_or(0);
            alive
                .into_iter()
                .enumerate()
                .map(|(i, (key, url, core, on, used))| {
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
                    super::OpenTab { tab: i + 1, key, title, url: now, front: if shown { on } else { used == last } }
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
                    .map(|x| serde_json::json!({ "key": x.key, "url": x.url, "load": x.load, "onScreen": x.bounds.is_some(), "loaded": x.loaded, "made": x.controller.is_some() }))
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
    fn words_selected_on_a_page_are_asked_as_a_selection() {
        let q = PageQuote { text: "  Chests behind the waterfall \n".into(), site: "wiki.example.com".into() };
        assert_eq!(q.selected(), Some(Selected { text: "Chests behind the waterfall".into(), app: "wiki.example.com".into() }));
        assert_eq!(PageQuote { text: " ".into(), site: "x".into() }.selected(), None);
    }
}

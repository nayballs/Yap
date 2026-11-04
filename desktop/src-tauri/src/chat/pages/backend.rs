//! **Where the page tools reach the pages**: the panel's own webviews
//! (`view.rs`: Yap's scripts run in them, `act/script.rs`) — or, in a model
//! evaluation or a unit test, the stand-in browser (`sim.rs`), whatever
//! the tool. One way in for `read_open_page`, the actions and the open tabs
//! every question is told about, so an evaluation measures the same code the
//! app runs.

use std::time::{Duration, Instant};

use serde_json::Value;

use super::act::rules::{Described, Snapshot};
use super::view::{self, OpenTab, PageText};

/// The stand-in browser's answer, when one is in place (debug builds).
macro_rules! sim {
    ($f:expr) => {
        #[cfg(any(test, debug_assertions))]
        {
            if let Some(r) = super::sim::with($f) {
                return r;
            }
        }
    };
}

/// **The page tabs open beside the chat** now.
pub async fn tabs() -> Vec<OpenTab> {
    sim!(|b| b.tabs());
    view::open_tabs().await
}

/// **The tabs, once there's one to read**: a page this turn brought up
/// (`opened`) is made by the Chat view as the turn goes — a read or an
/// action right after it waits for its tab (at most a few seconds).
pub async fn tabs_soon(opened: bool) -> Vec<OpenTab> {
    let started = Instant::now();
    #[cfg(any(test, debug_assertions))]
    let sim = super::sim::with(|_| ()).is_some();
    #[cfg(not(any(test, debug_assertions)))]
    let sim = false;
    loop {
        let tabs = tabs().await;
        // (The stand-in browser opens a page at once; no window, no page.)
        if !tabs.is_empty() || !opened || sim || !view::ready() || started.elapsed() > Duration::from_secs(6) {
            return tabs;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

/// **A page brought up by `open_page`**, made now: the stand-in browser
/// opens it; the panel's is made out of sight (`view::open_hidden`) — the
/// Chat view shows only the last page a step brought up, and each must be a
/// tab the model can read and act on. The tab closed to make room for it
/// (at most `view::MAX_VIEWS` stay open), said for the model.
pub async fn opened(url: &str, section: &str) -> Option<String> {
    #[cfg(any(test, debug_assertions))]
    {
        if let Some(closed) = super::sim::with(|b| {
            b.open(url);
            b.take_closed()
        }) {
            return closed.map(|(n, title)| closed_words(n, &title));
        }
    }
    view::open_hidden(url, section).await.map(|(n, title)| closed_words(n, &title))
}

/// "Tab 3 ("…") closed to make room…", for the model. Pure.
fn closed_words(n: usize, title: &str) -> String {
    let title = super::act::rules::clip(title, 60).replace('"', "'");
    let named = if title.is_empty() { String::new() } else { format!(" (\"{title}\")") };
    format!("Tab {n}{named} closed to make room: at most {} pages stay open beside the chat. Its number is gone; open_page brings it back as a new tab.", view::MAX_VIEWS)
}

/// **Yap acts on page `key`** while this lives: the page's dialogs are
/// dismissed (`view::busy`). Nothing for the stand-in browser.
pub async fn busy(key: &str) -> Option<view::Busy> {
    #[cfg(any(test, debug_assertions))]
    {
        if super::sim::with(|_| ()).is_some() {
            return None;
        }
    }
    Some(view::busy(key).await)
}

/// **A page's words** (`read_open_page`).
pub async fn text(key: &str) -> Result<PageText, String> {
    sim!(|b| b.text(key));
    wait_loaded(key).await;
    view::page_text(key).await
}

/// **A page with its elements** (`read_open_page { elements: true }`): its
/// text cut at `text_max`, at most `max` elements; addresses as the page's
/// own (a test run's stand-in's turned back).
pub async fn snapshot(key: &str, text_max: usize, max: usize) -> Result<Snapshot, String> {
    sim!(|b| b.snapshot(key, text_max, max));
    wait_loaded(key).await;
    let raw = view::run_script(key, super::act::script::snap(text_max, max)).await?;
    let mut s: Snapshot = serde_json::from_str(&raw).map_err(|_| "The page couldn't be read.".to_string())?;
    s.url = view::shown(&s.url);
    for e in &mut s.elements {
        if !e.href.is_empty() {
            e.href = view::shown(&e.href);
        }
    }
    for f in &mut s.forms {
        f.action = view::shown(&f.action);
    }
    Ok(s)
}

/// **Element `r`** of page `key` as it is now.
pub async fn describe(key: &str, r: u32) -> Result<Described, String> {
    sim!(|b| b.describe(key, r));
    wait_loaded(key).await;
    let raw = view::run_script(key, super::act::script::describe(r)).await?;
    let mut d: Described = serde_json::from_str(&raw).map_err(|_| "The page couldn't be read.".to_string())?;
    d.url = view::shown(&d.url);
    if let Some(f) = d.form.as_mut() {
        f.action = view::shown(&f.action);
    }
    if let Some(e) = d.el.as_mut() {
        e.href = if e.href.is_empty() { String::new() } else { view::shown(&e.href) };
    }
    Ok(d)
}

/// **An action** on page `key`: `op` is `click`, `type` or `scroll`, with
/// `a` its arguments (`ref`, `role`, `name`, `text`, `submit`, `to`); the
/// script's answer (`{"r": "ok" | "gone" | "changed" | …}`).
pub async fn act(key: &str, op: &str, a: &Value) -> Result<Value, String> {
    sim!(|b| b.act(key, op, a));
    // (A page just brought up: loaded first, as a read waits.)
    wait_loaded(key).await;
    let r = a["ref"].as_u64().unwrap_or(0) as u32;
    let (role, name) = (a["role"].as_str().unwrap_or(""), a["name"].as_str().unwrap_or(""));
    let script = match op {
        "click" => super::act::script::click(r, role, name),
        "type" => super::act::script::type_text(r, role, name, a["text"].as_str().unwrap_or(""), a["submit"].as_bool().unwrap_or(false)),
        "scroll" => super::act::script::scroll(a["to"].as_str() == Some("up"), (r > 0).then_some(r)),
        other => return Err(format!("Unknown action {other}")),
    };
    let raw = view::run_script(key, script).await?;
    serde_json::from_str(&raw).map_err(|_| "The page didn't answer.".to_string())
}

/// **Tab `key` goes to `url`** (a checked address: `go_to`).
pub async fn navigate(key: &str, url: &str) -> Result<(), String> {
    sim!(|b| b.navigate(key, url));
    view::navigate(key, url).await
}

/// **Tab `key` in front** (`switch_tab`): the panel's page moves there (the
/// Chat view hears `stream::Event::SwitchTab` and shows its tab).
pub async fn front(key: &str) -> Result<(), String> {
    sim!(|b| b.front(key));
    view::bring_front(key).await
}

/// Where page `key`'s navigations are: how many have started, and whether
/// the latest has loaded.
#[derive(Debug, Clone, Copy, Default)]
pub struct Mark {
    pub navs: u64,
}

/// [`Mark`] now (before an action).
pub async fn mark(key: &str) -> Mark {
    #[cfg(any(test, debug_assertions))]
    {
        if super::sim::with(|_| ()).is_some() {
            return Mark::default();
        }
    }
    Mark { navs: view::nav_state(key).await.map(|(n, _)| n).unwrap_or(0) }
}

/// **After an action**: the page given time to go where it goes — a
/// navigation it started loaded (at most 12 s), else a moment for what the
/// click changed in place.
pub async fn settle(key: &str, before: Mark) {
    #[cfg(any(test, debug_assertions))]
    {
        if super::sim::with(|_| ()).is_some() {
            return;
        }
    }
    let started = Instant::now();
    loop {
        tokio::time::sleep(Duration::from_millis(120)).await;
        let Some((navs, loaded)) = view::nav_state(key).await else { return };
        if navs > before.navs {
            if loaded {
                // Its scripts and pictures a moment.
                tokio::time::sleep(Duration::from_millis(250)).await;
                return;
            }
            if started.elapsed() > Duration::from_secs(12) {
                return;
            }
        } else if started.elapsed() > Duration::from_millis(700) {
            return;
        }
    }
}

/// Page `key` loaded before it's read (a page just brought up; at most 10 s).
async fn wait_loaded(key: &str) {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(10) {
        match view::nav_state(key).await {
            Some((_, true)) | None => return,
            Some((_, false)) => tokio::time::sleep(Duration::from_millis(150)).await,
        }
    }
}

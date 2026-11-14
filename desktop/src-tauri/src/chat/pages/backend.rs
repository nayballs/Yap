//! **Where the page tools reach the pages**: the panel's own webviews
//! (`view.rs`: Yap's scripts run in them, `act/script.rs`) — or, in a model
//! evaluation or a unit test, the stand-in browser (`sim.rs`), whatever
//! the tool. One way in for every browser tool (reads, shots, actions) and the open tabs
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

/// **A page brought up in a new tab**, made now: the stand-in browser
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
    format!("Tab {n}{named} closed to make room: at most {} pages stay open beside the chat. Its number is gone; browser_tabs (new) brings it back as a new tab.", view::MAX_VIEWS)
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

/// **A page's words** (`browser_read`), its frames Yap can't read inside
/// at their own addresses.
pub async fn text(key: &str) -> Result<PageText, String> {
    sim!(|b| b.text(key));
    wait_loaded(key).await;
    let mut t = view::page_text(key).await?;
    for f in &mut t.frames {
        f.src = view::shown(&f.src);
    }
    Ok(t)
}

/// **Page `key` laid out `width` CSS pixels wide** (a desktop site's
/// layout, shown scaled to fit: `view::set_width`), or the panel's own
/// (`None`); where it is in view once it has laid itself out again.
pub async fn width(key: &str, width: Option<u32>) -> Result<super::see::InView, String> {
    sim!(|b| b.set_width(key, width).and_then(|()| b.in_view(key)));
    wait_loaded(key).await;
    view::set_width(key, width).await?;
    // Its new layout a moment (a resize event, its own scripts), then where it is.
    tokio::time::sleep(Duration::from_millis(250)).await;
    still(key).await
}

/// **A long page's words** (`browser_read { offset }`): the same read, up to
/// `read::TEXT_FULL` characters (`read::long_text_script`).
pub async fn text_long(key: &str) -> Result<PageText, String> {
    sim!(|b| b.text_up_to(key, super::read::TEXT_FULL));
    wait_loaded(key).await;
    let raw = view::run_script(key, super::read::long_text_script()).await?;
    serde_json::from_str(&raw).map_err(|_| "The page couldn't be read.".to_string())
}

/// **A page's tables** (`browser_read { mode: "tables" }`): as it shows
/// them now (`read::TABLES_SCRIPT`), links as the page's own addresses.
pub async fn tables(key: &str) -> Result<super::read::Tables, String> {
    sim!(|b| b.tables(key));
    wait_loaded(key).await;
    let raw = view::run_script(key, super::read::TABLES_SCRIPT.to_string()).await?;
    let mut t: super::read::Tables = serde_json::from_str(&raw).map_err(|_| "The page's tables couldn't be read.".to_string())?;
    t.url = view::shown(&t.url);
    for c in t.tables.iter_mut().flat_map(|t| t.rows.iter_mut()).flatten() {
        if !c.h.is_empty() {
            c.h = view::shown(&c.h);
        }
    }
    Ok(t)
}

/// **A page with its elements** (`browser_read { mode: "elements" }`, `browser_find`): its
/// text cut at `text_max`, at most `max` elements; addresses as the page's
/// own (a test run's stand-in's turned back).
pub async fn snapshot(key: &str, text_max: usize, max: usize) -> Result<Snapshot, String> {
    snapshot_within(key, text_max, max, None).await
}

/// **[`snapshot`] of one part** (`within`: an element's ref — a heading's
/// section, or the element; `browser_read { mode: "elements", ref }`). The
/// simulator lists the whole page.
pub async fn snapshot_within(key: &str, text_max: usize, max: usize, within: Option<u32>) -> Result<Snapshot, String> {
    sim!(|b| b.snapshot(key, text_max, max));
    wait_loaded(key).await;
    let raw = view::run_script(key, super::act::script::snap_within(text_max, max, within)).await?;
    let mut s: Snapshot = serde_json::from_str(&raw).map_err(|_| "The page couldn't be read.".to_string())?;
    s.url = view::shown(&s.url);
    for e in &mut s.elements {
        if !e.href.is_empty() {
            e.href = view::shown(&e.href);
        }
        if !e.frame.is_empty() {
            e.frame = view::shown(&e.frame);
        }
    }
    for f in &mut s.forms {
        f.action = view::shown(&f.action);
    }
    Ok(s)
}

/// A described element as Rust reads it: addresses as the page's own.
fn described(raw: &str) -> Result<Described, String> {
    let mut d: Described = serde_json::from_str(raw).map_err(|_| "The page couldn't be read.".to_string())?;
    d.url = view::shown(&d.url);
    if let Some(f) = d.form.as_mut() {
        f.action = view::shown(&f.action);
    }
    if let Some(e) = d.el.as_mut() {
        e.href = if e.href.is_empty() { String::new() } else { view::shown(&e.href) };
        e.frame = if e.frame.is_empty() { String::new() } else { view::shown(&e.frame) };
    }
    Ok(d)
}

/// **Element `r`** of page `key` as it is now.
pub async fn describe(key: &str, r: u32) -> Result<Described, String> {
    sim!(|b| b.describe(key, r));
    wait_loaded(key).await;
    described(&view::run_script(key, super::act::script::describe(r)).await?)
}

/// **The element at point (`x`, `y`)** of page `key`'s view (CSS pixels):
/// its nearest link, button or field, else itself, with a ref of its own.
pub async fn at(key: &str, x: f64, y: f64) -> Result<Described, String> {
    sim!(|b| b.at(key, x, y));
    wait_loaded(key).await;
    described(&view::run_script(key, super::act::script::at(x, y)).await?)
}

/// **The element with the focus** on page `key` (`el` none: nothing).
pub async fn active(key: &str) -> Result<Described, String> {
    sim!(|b| b.active(key));
    wait_loaded(key).await;
    described(&view::run_script(key, super::act::script::active()).await?)
}

/// **Where page `key` is in view**: its scroll, the view's size, the
/// device's pixels per CSS pixel, the headings in view.
pub async fn view_of(key: &str) -> Result<super::see::InView, String> {
    sim!(|b| b.in_view(key));
    wait_loaded(key).await;
    let raw = view::run_script(key, super::act::script::pos()).await?;
    serde_json::from_str(&raw).map_err(|_| "The page couldn't be read.".to_string())
}

/// **Page `key`'s visible text blocks**, each with a ref (`browser_find`,
/// `browser_scroll_to`).
pub async fn blocks(key: &str) -> Result<Vec<super::see::Block>, String> {
    sim!(|b| b.blocks(key));
    wait_loaded(key).await;
    let raw = view::run_script(key, super::act::script::blocks(800, 400, 120_000)).await?;
    let v: Value = serde_json::from_str(&raw).map_err(|_| "The page couldn't be read.".to_string())?;
    serde_json::from_value(v["blocks"].clone()).map_err(|_| "The page couldn't be read.".to_string())
}

/// **What's in view of page `key` now**, a PNG at the device's pixels.
pub async fn shot(key: &str) -> Result<Vec<u8>, String> {
    sim!(|b| b.shot(key));
    wait_loaded(key).await;
    view::shot_in_view(key).await
}

/// **What's in view of page `key` now, drawn `scale` times finer** where
/// that can't be seen (`view::shot_in_view_at`: a page out of sight); else
/// at the device's pixels. The PNG and the scale it was drawn at (1: the
/// device's own).
pub async fn shot_at(key: &str, scale: f64) -> Result<(Vec<u8>, f64), String> {
    sim!(|b| b.shot_at(key, scale));
    wait_loaded(key).await;
    view::shot_in_view_at(key, scale).await
}

/// Page `key` has loaded (the stand-in browser's always has).
pub async fn loaded(key: &str) -> bool {
    sim!(|_| true);
    matches!(view::nav_state(key).await, Some((_, true)) | None)
}

/// **Page `key` back or forward** in its own history.
pub async fn history(key: &str, back: bool) -> Result<(), String> {
    sim!(|b| b.history(key, back));
    view::history(key, back).await
}

/// **Page `key` closed** (its tab).
pub async fn close(key: &str) -> Result<(), String> {
    sim!(|b| b.close(key));
    view::close(key).await
}

/// **Where page `key` is in view once it has stopped scrolling** (8 Nov
/// 2026: a scroll on a page with `scroll-behavior: smooth` was told from
/// where it began, and a zoom straight after caught it mid-way): its scroll
/// read until it's the same twice in a row a frame or more apart, at most
/// [`STILL_MAX`]. Yap's own scrolls are instant (`act/script.rs`); this
/// waits out one the page makes itself (a link to a part of the page, a
/// script's smooth scroll).
pub async fn still(key: &str) -> Result<super::see::InView, String> {
    sim!(|b| b.in_view(key));
    let started = Instant::now();
    let mut last = view_of(key).await?;
    loop {
        tokio::time::sleep(STILL_STEP).await;
        let now = view_of(key).await?;
        if (now.scroll.y == last.scroll.y && now.scroll.x == last.scroll.x) || started.elapsed() >= STILL_MAX {
            return Ok(now);
        }
        last = now;
    }
}

/// How long [`still`] waits for a page to stop scrolling, at most, and how
/// far apart it reads where it is (more than two frames at 60 Hz).
pub const STILL_MAX: Duration = Duration::from_millis(1000);
const STILL_STEP: Duration = Duration::from_millis(40);

/// **Start watching page `key`** for what an action changes (`script.rs`
/// `watch`): read back with [`changes`]. Nothing if the page can't be
/// reached (then the address and the text alone say what changed).
pub async fn watch(key: &str) {
    sim!(|b| b.watch(key));
    let _ = view::run_script(key, super::act::script::watch()).await;
}

/// **What changed on page `key` since [`watch`]** (`rules::Changes`); not
/// watched (`watched` false) when the page went elsewhere or couldn't say.
pub async fn changes(key: &str) -> super::act::rules::Changes {
    sim!(|b| b.changes(key));
    match view::run_script(key, super::act::script::changes()).await {
        Ok(raw) => {
            let mut ch: super::act::rules::Changes = serde_json::from_str(&raw).unwrap_or_default();
            ch.url = view::shown(&ch.url);
            ch.url0 = view::shown(&ch.url0);
            for f in &mut ch.frames {
                *f = view::shown(f);
            }
            ch
        }
        Err(_) => super::act::rules::Changes::default(),
    }
}

/// **An action** on page `key`: `op` is `click`, `type`, `choose`,
/// `scroll`, `where`, `clickAt`, `press`, `hover`, `check`, `key`,
/// `dragStart`, `dragMove` or `dragEnd`, with `a` its arguments (`ref`,
/// `role`, `name`, `text`, `submit`, `again`, `to`, `amount`, `x`, `y`,
/// `at`, `button`, `count`, `f`, `on`, `keys`); the script's answer (`{"r":
/// "ok" | "gone" | "changed" | …}`).
pub async fn act(key: &str, op: &str, a: &Value) -> Result<Value, String> {
    sim!(|b| b.act(key, op, a));
    // (A page just brought up: loaded first, as a read waits.)
    wait_loaded(key).await;
    let r = a["ref"].as_u64().unwrap_or(0) as u32;
    let (role, name) = (a["role"].as_str().unwrap_or(""), a["name"].as_str().unwrap_or(""));
    let num = |k: &str| a[k].as_f64().unwrap_or(0.0);
    let script = match op {
        "click" => super::act::script::click(r, role, name),
        "type" => super::act::script::type_text(r, role, name, a["text"].as_str().unwrap_or(""), a["submit"].as_bool().unwrap_or(false)),
        "choose" => super::act::script::choose(r, role, name, a["text"].as_str().unwrap_or(""), a["again"].as_bool().unwrap_or(false)),
        "where" => super::act::script::where_is(r),
        "scroll" => super::act::script::scroll(a["to"].as_str().unwrap_or("down"), a["amount"].as_f64().unwrap_or(1.0), (r > 0).then_some(r)),
        "clickAt" => super::act::script::click_at(r, role, name, num("x"), num("y")),
        "press" => {
            let at = a["at"].as_bool().unwrap_or(false).then(|| (num("x"), num("y")));
            super::act::script::press(r, role, name, at, a["button"].as_u64().unwrap_or(0) as u8, a["count"].as_u64().unwrap_or(1) as u8)
        }
        "dragStart" => {
            let at = a["at"].as_bool().unwrap_or(false).then(|| (num("x"), num("y")));
            super::act::script::drag_start(r, role, name, at, a["to"].clone())
        }
        "dragMove" => super::act::script::drag_move(num("f")),
        "dragEnd" => super::act::script::drag_end(),
        "aim" => {
            let at = a["at"].as_bool().unwrap_or(false).then(|| (num("x"), num("y")));
            let kinds: Vec<&str> = a["kinds"].as_array().map(|k| k.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            super::act::script::aim(r, role, name, at, &kinds, a["on"].as_bool().unwrap_or(false))
        }
        "aimed" => super::act::script::aimed(r, a["id"].as_str().unwrap_or(""), a["count"].as_u64().unwrap_or(1) as u8, a["hover"].as_bool().unwrap_or(false)),
        "unhover" => super::act::script::unhover(),
        "tree" if a["role"].is_string() => super::act::script::roles(&super::clip(a["role"].as_str().unwrap_or(""), 30), &super::clip(a["words"].as_str().unwrap_or(""), 100), a["limit"].as_u64().unwrap_or(20) as usize),
        "wheelAim" => super::act::script::wheel_aim(a["x"].as_f64().unwrap_or(-1.0), a["y"].as_f64().unwrap_or(-1.0)),
        "wheeled" => super::act::script::wheeled(),
        "tree" => super::act::script::tree(
            a["within"].as_u64().map(|w| w as u32),
            a["depth"].as_u64().unwrap_or(15),
            a["filter"].as_str().unwrap_or("all"),
            a["offset"].as_u64().unwrap_or(0) as usize,
            a["max"].as_u64().unwrap_or(super::READ_MAX as u64) as usize,
        ),
        "listen" => {
            let kinds: Vec<&str> = a["kinds"].as_array().map(|k| k.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            super::act::script::listen((r > 0).then_some((r, role, name)), &kinds)
        }
        "section" => super::act::script::section(r, a["offset"].as_u64().unwrap_or(0) as usize, a["max"].as_u64().unwrap_or(super::READ_MAX as u64) as usize),
        "hover" => super::act::script::hover(r, role, name),
        "check" => super::act::script::check(r, role, name, a["on"].as_bool().unwrap_or(true)),
        "key" => {
            let keys: Vec<String> = a["keys"].as_array().map(|k| k.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()).unwrap_or_default();
            super::act::script::keys(&keys, (r > 0).then_some((r, role, name)))
        }
        other => return Err(format!("Unknown action {other}")),
    };
    let raw = view::run_script(key, script).await?;
    serde_json::from_str(&raw).map_err(|_| "The page didn't answer.".to_string())
}

/// **A DevTools call on page `key`** (the developer's tools, `dev.rs`:
/// `Runtime.evaluate`, `Network.getResponseBody`, `Emulation.*`): its
/// result as JSON. None in the simulator.
pub async fn devtools_call(key: &str, method: &str, params: &Value) -> Result<String, String> {
    sim!(|_| Err("Not in the simulator.".to_string()));
    wait_loaded(key).await;
    view::devtools(key, method, params).await
}

/// **Whether real input may go to the pages** (not in the simulator: its
/// pages take the script's own events).
pub fn real_input() -> bool {
    #[cfg(any(test, debug_assertions))]
    {
        if super::sim::with(|_| ()).is_some() {
            return false;
        }
    }
    cfg!(windows)
}

/// **Real input on page `key`** (`view::devtools`: the DevTools `Input`
/// domain's calls, each `(method, params)`, in turn) — the mouse and the
/// keyboard as a person's, `isTrusted`. The simulator has none: its pages
/// take the script's events (`act.rs` falls back to them).
pub async fn input(key: &str, calls: &[(&str, Value)]) -> Result<(), String> {
    sim!(|_| Err("No real input in the simulator.".to_string()));
    for (method, params) in calls {
        view::devtools(key, method, params).await?;
    }
    Ok(())
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

/// **A moment more** after an action whose effect a page draws late (a
/// hover's preview, shown after a delay); nothing for the stand-in browser.
pub async fn pause(d: Duration) {
    #[cfg(any(test, debug_assertions))]
    {
        if super::sim::with(|_| ()).is_some() {
            return;
        }
    }
    tokio::time::sleep(d).await;
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

//! **A stand-in browser** for the page tools (debug builds and unit tests
//! only): the pages a model evaluation's scenario gives (`browse` scenarios,
//! `ai/eval/`), or a unit test's, read and acted on in Rust — no WebView2,
//! no window, nothing on the internet. It answers everything the panel's
//! real pages do (`pages/backend.rs`): the tabs, a page's text, its
//! elements and text blocks with refs, describe / click / type / scroll /
//! keys / hover / ticking a box, the element at a point and the one with the
//! focus, going somewhere and back, the tab in front or closed, a shot of
//! the view.
//!
//! **Its layout** (for scrolling, `browser_scroll_to`, a point clicked and
//! the headings in view): a [`VIEW_W`] × [`VIEW_H`] view; each block of
//! text or element a line of [`LINE`] pixels in the page's order, one
//! inside another on its line, `data-sim-y="<px>"` where it says; an element
//! with `data-sim-rect="x,y,w,h"` (page pixels) is what a point there hits.
//! `data-sim-hover="<id>"` shows a `hidden` element when hovered,
//! `data-sim-after="<ms>"` keeps one out until that long after the page
//! loaded (`browser_wait`). A shot ([`Browser::shot`]) encodes the scroll in
//! its red and the point in its green and blue.
//!
//! It reads a page's HTML as the element script reads the live page
//! (`act/script.rs`): the same candidates (links, buttons, inputs, selects,
//! text areas…), names (`aria-label`, labels, placeholders, words), forms
//! (a password field? where it goes, its submit button), refs in the page's
//! order (the same for as long as the page is, every candidate counted,
//! hidden ones too, so a banner going doesn't renumber the rest). What it
//! does: a link goes to its address; a submit button or Enter in a field
//! submits its form as a GET (its fields in the query); a checkbox toggles; a
//! select takes the option with the words typed. Two attributes stand in for
//! a page's own script: `data-sim-hide="<id>"` (clicking it hides that
//! element: a cookie banner's button) and `data-sim-go="<address>"`.
//!
//! A page's address finds its HTML by its query (`results?from=A&to=B`: those
//! pairs among the address's, in any order, any case — a form's other fields
//! don't matter) or without one; else a "not found" page.
//!
//! It keeps a log for the evaluation's checks ([`Log`]): every page it went
//! to, and every field typed into (its words and the text).

use std::collections::{BTreeMap, HashMap, HashSet};

use serde_json::{json, Value};

use super::act::rules::{self, Described, Element, Form, Scroll, Snapshot};
use super::view::{OpenTab, PageText};

/// What the browser did, for an evaluation's checks.
#[derive(Debug, Clone, Default)]
pub struct Log {
    /// Every page it went to (opened, a link, a form, an address), in order.
    pub visited: Vec<String>,
    /// Every field typed into: the field's words (name, label, `name`, id)
    /// and the text.
    pub typed: Vec<(String, String)>,
    /// Every click that went through: the element's words (a click at a
    /// point: "words @ x,y").
    pub clicked: Vec<String>,
    /// Every key pressed, in order.
    pub keys: Vec<String>,
    /// Every element hovered over: its words.
    pub hovered: Vec<String>,
}

/// The stand-in view's size (CSS pixels), and the room each block of text
/// takes in its layout (a block's `data-sim-y` puts it where it says).
pub const VIEW_W: i64 = 1280;
pub const VIEW_H: i64 = 800;
const LINE: i64 = 60;
/// What a block of text is, for the layout, `browser_find` and
/// `browser_scroll_to` (the element script's own list).
const BLOCK: &str = "h1, h2, h3, h4, h5, h6, [role=heading], p, li, td, th, dt, dd, caption, figcaption, blockquote, pre, summary, label, legend, a, button, option";

struct Tab {
    key: String,
    /// Its number for the model: its own, never used again (as the panel's).
    id: usize,
    /// When it was last opened or put in front (the least recent goes to
    /// make room, as the panel's).
    used: u64,
    url: String,
    html: String,
    /// What's been typed (by ref).
    values: HashMap<u32, String>,
    checked: HashMap<u32, bool>,
    /// Elements hidden by `data-sim-hide` (their ids).
    hidden: HashSet<String>,
    scroll: i64,
    /// A read with its elements has numbered this page's elements: refs exist
    /// only then, as the element script numbers only what it has seen.
    seen: bool,
    /// The element with the focus (its ref).
    focus: Option<u32>,
    /// Elements a hover showed (`data-sim-hover="<id>"`: their ids).
    shown: HashSet<String>,
    /// The tab's history: where Back and Forward go.
    back: Vec<String>,
    forward: Vec<String>,
    /// When its page loaded: an element with `data-sim-after="<ms>"` shows
    /// that long after (a page that loads as you go).
    loaded: std::time::Instant,
    /// The page as it was when an action began (`backend::watch`), for what
    /// it changed (`backend::changes`).
    watched: Option<Watched>,
}

/// A tab as an action found it: its scroll, focus, what a hover showed, what
/// a click hid.
#[derive(Clone)]
struct Watched {
    url: String,
    scroll: i64,
    focus: Option<u32>,
    shown: HashSet<String>,
    hidden: HashSet<String>,
}

impl Tab {
    fn new(key: String, id: usize, used: u64, url: &str, html: String) -> Tab {
        Tab {
            key,
            id,
            used,
            url: url.to_string(),
            html,
            values: HashMap::new(),
            checked: HashMap::new(),
            hidden: HashSet::new(),
            scroll: 0,
            seen: false,
            focus: None,
            shown: HashSet::new(),
            back: Vec::new(),
            forward: Vec::new(),
            loaded: std::time::Instant::now(),
            watched: None,
        }
    }
}

/// **The stand-in browser**: its pages, its tabs, the one in front.
pub struct Browser {
    pages: Vec<(String, String)>,
    tabs: Vec<Tab>,
    front: usize,
    next_id: usize,
    tick: u64,
    /// The tab the last [`Browser::open`] closed to make room (its number
    /// and title).
    closed: Option<(usize, String)>,
    pub log: Log,
}

/// A page's address as pages are found by: its host (no `www.`), path (no
/// trailing `/`), and its query's pairs sorted, words lower-cased, `+` as a
/// space (`with_query`). Pure.
fn norm(url: &str, with_query: bool) -> String {
    let Ok(u) = reqwest::Url::parse(url.trim()) else { return url.trim().to_ascii_lowercase() };
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    let host = host.trim_start_matches("www.");
    let path = u.path().trim_end_matches('/');
    if !with_query {
        return format!("{host}{path}");
    }
    let mut pairs: Vec<(String, String)> = u.query_pairs().map(|(k, v)| (k.to_lowercase(), v.trim().to_lowercase())).filter(|(_, v)| !v.is_empty()).collect();
    pairs.sort();
    let q: Vec<String> = pairs.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{host}{path}?{}", q.join("&"))
}

/// An address's query pairs, as pages are found by (`norm`'s). Pure.
fn pairs(url: &str) -> Vec<(String, String)> {
    reqwest::Url::parse(url.trim())
        .map(|u| u.query_pairs().map(|(k, v)| (k.to_lowercase(), v.trim().to_lowercase())).filter(|(_, v)| !v.is_empty()).collect())
        .unwrap_or_default()
}

const NOT_FOUND: &str = "<!doctype html><html><head><title>Not found</title></head><body><main><h1>Not found</h1><p>There's no page at this address.</p></main></body></html>";

/// What makes an element one the model can act on.
const CANDIDATE: &str = "a[href], button, input, select, textarea, summary, [role=button], [role=link], [role=checkbox], [role=radio], [role=tab], [role=menuitem], [role=switch], [role=combobox], [role=textbox], [role=searchbox], [role=option], [contenteditable]";

impl Browser {
    /// A browser over `pages` (address → HTML, or plain text), no tab open.
    pub fn new(pages: &BTreeMap<String, String>) -> Browser {
        Browser { pages: pages.iter().map(|(u, h)| (u.clone(), h.clone())).collect(), tabs: Vec::new(), front: 0, next_id: 1, tick: 0, closed: None, log: Log::default() }
    }

    /// The HTML for `url`: the page whose address has its query's pairs (all
    /// of them in `url`'s, the most that match: a form's other fields don't
    /// matter), else the page without a query, else "not found".
    fn page_for(&self, url: &str) -> String {
        let bare = norm(url, false);
        let have = pairs(url);
        let with_query = self
            .pages
            .iter()
            .filter(|(u, _)| reqwest::Url::parse(u).is_ok_and(|p| p.query().is_some()) && norm(u, false) == bare)
            .map(|(u, h)| (pairs(u), h))
            .filter(|(want, _)| want.iter().all(|p| have.contains(p)))
            .max_by_key(|(want, _)| want.len())
            .map(|(_, h)| h.clone());
        let body = with_query.or_else(|| self.pages.iter().find(|(u, _)| reqwest::Url::parse(u).is_ok_and(|p| p.query().is_none()) && norm(u, false) == bare).map(|(_, h)| h.clone()));
        match body {
            Some(h) if h.trim_start().starts_with('<') => h,
            Some(text) => format!("<html><body><main><p>{}</p></main></body></html>", text.replace('&', "&amp;").replace('<', "&lt;")),
            None => NOT_FOUND.to_string(),
        }
    }

    fn tick(&mut self) -> u64 {
        self.tick += 1;
        self.tick
    }

    /// **Open `url` in a tab** (`open_page`): its tab in front — the one it
    /// has, or a new one (with a number of its own); at most
    /// `view::MAX_VIEWS` stay open, the least recently used other than the
    /// one in front closing to make room ([`Browser::take_closed`]), as the
    /// panel's do. Its key.
    pub fn open(&mut self, url: &str) -> String {
        let key = super::view::key_of(url);
        self.closed = None;
        let now = self.tick();
        if let Some(i) = self.tabs.iter().position(|t| t.key == key) {
            self.front = i;
            self.tabs[i].used = now;
            return key;
        }
        let alive: Vec<(String, u64, bool)> = self.tabs.iter().enumerate().map(|(i, t)| (t.key.clone(), t.used, i == self.front)).collect();
        if let Some(gone) = super::view::evict(&alive) {
            if let Some(i) = self.tabs.iter().position(|t| t.key == gone) {
                let t = self.tabs.remove(i);
                self.closed = Some((t.id, title_of(&t.html)));
            }
        }
        let html = self.page_for(url);
        self.log.visited.push(url.to_string());
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(Tab::new(key.clone(), id, now, url, html));
        self.front = self.tabs.len() - 1;
        key
    }

    /// The tab the last [`Browser::open`] closed to make room, once.
    pub fn take_closed(&mut self) -> Option<(usize, String)> {
        self.closed.take()
    }

    /// The tabs, as `view::open_tabs` tells them: each with its own number.
    pub fn tabs(&self) -> Vec<OpenTab> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(i, t)| OpenTab { tab: t.id, key: t.key.clone(), title: title_of(&t.html), url: t.url.clone(), front: i == self.front })
            .collect()
    }

    fn tab(&self, key: &str) -> Result<&Tab, String> {
        self.tabs.iter().find(|t| t.key == key).ok_or_else(|| "That page isn't open.".to_string())
    }

    fn tab_mut(&mut self, key: &str) -> Result<&mut Tab, String> {
        self.tabs.iter_mut().find(|t| t.key == key).ok_or_else(|| "That page isn't open.".to_string())
    }

    /// The tab `key` in front.
    pub fn front(&mut self, key: &str) -> Result<(), String> {
        let i = self.tabs.iter().position(|t| t.key == key).ok_or("That page isn't open.")?;
        self.front = i;
        self.tabs[i].used = self.tick();
        Ok(())
    }

    /// **Tab `key` goes to `url`** (an address, a link, a form): the page it
    /// was on is where Back goes.
    pub fn navigate(&mut self, key: &str, url: &str) -> Result<(), String> {
        let tab = self.tab_mut(key)?;
        let was = tab.url.clone();
        tab.back.push(was);
        tab.forward.clear();
        self.load(key, url)
    }

    /// Tab `key`'s page is `url` from now on, afresh.
    fn load(&mut self, key: &str, url: &str) -> Result<(), String> {
        let html = self.page_for(url);
        let tab = self.tab_mut(key)?;
        tab.url = url.to_string();
        tab.html = html;
        tab.values.clear();
        tab.checked.clear();
        tab.hidden.clear();
        tab.shown.clear();
        tab.scroll = 0;
        tab.seen = false;
        tab.focus = None;
        tab.loaded = std::time::Instant::now();
        tab.watched = None;
        self.log.visited.push(url.to_string());
        Ok(())
    }

    /// **Tab `key` watched** from now on for what an action changes (the
    /// element script's `watch`).
    pub fn watch(&mut self, key: &str) {
        if let Ok(t) = self.tab_mut(key) {
            t.watched = Some(Watched { url: t.url.clone(), scroll: t.scroll, focus: t.focus, shown: t.shown.clone(), hidden: t.hidden.clone() });
        }
    }

    /// **What changed on tab `key` since [`Browser::watch`]**, as the
    /// element script's `changes` says it: the scroll, the field the focus
    /// moved into, what a hover showed (a popup opened) or Escape let go (it
    /// closed), and the elements that changed (hidden, typed, ticked). Not
    /// watched after it went elsewhere.
    pub fn changes(&mut self, key: &str) -> rules::Changes {
        let Ok(tab) = self.tab_mut(key) else { return rules::Changes::default() };
        let Some(w) = tab.watched.take() else { return rules::Changes::default() };
        let tab = self.tab(key).expect("open");
        let doc = dom_query::Document::from(tab.html.as_str());
        let page = Page::read(&doc, tab);
        let overlay = |id: &str| {
            let n = doc.select("[id]").nodes().iter().find(|n| n.id_attr().is_some_and(|x| *x == *id)).copied();
            let kind = match n.and_then(|n| n.attr("role")).map(|r| r.to_ascii_lowercase()).as_deref() {
                Some("dialog" | "alertdialog") => "dialog",
                Some("menu") => "menu",
                Some("listbox" | "tree") => "list",
                _ => "popup",
            };
            rules::Overlay { kind: kind.into(), text: n.map(|n| Page::text_of(&n).chars().take(160).collect()).unwrap_or_default() }
        };
        let mut opened: Vec<&String> = tab.shown.difference(&w.shown).collect();
        opened.sort();
        let mut closed: Vec<&String> = w.shown.difference(&tab.shown).collect();
        closed.sort();
        let focus = (tab.focus != w.focus).then_some(tab.focus).flatten().and_then(|r| page.node(r).map(|n| page.describe(&n, r)));
        // (What's typed or ticked is a field's own state, which a page's
        // observer doesn't see either: only what a click hid or showed.)
        let in_view = tab.hidden.symmetric_difference(&w.hidden).count() as u32;
        let v = page.in_view();
        rules::Changes {
            watched: true,
            y0: w.scroll,
            scroll: Scroll { y: v.scroll.y, h: v.scroll.h, max: v.scroll.max },
            url0: w.url,
            url: tab.url.clone(),
            focus,
            opened: opened.into_iter().take(3).map(|id| overlay(id)).collect(),
            closed: closed.into_iter().take(3).map(|id| rules::Overlay { kind: overlay(id).kind, text: String::new() }).collect(),
            in_view,
        }
    }

    /// **Tab `key` back or forward** in its history (nowhere to go: it stays).
    pub fn history(&mut self, key: &str, back: bool) -> Result<(), String> {
        let tab = self.tab_mut(key)?;
        let to = if back { tab.back.pop() } else { tab.forward.pop() };
        let Some(to) = to else { return Ok(()) };
        let was = tab.url.clone();
        if back {
            tab.forward.push(was);
        } else {
            tab.back.push(was);
        }
        self.load(key, &to)
    }

    /// **Tab `key` closed**: the one before it in front.
    pub fn close(&mut self, key: &str) -> Result<(), String> {
        let i = self.tabs.iter().position(|t| t.key == key).ok_or("That page isn't open.")?;
        self.tabs.remove(i);
        if self.front >= self.tabs.len() || self.front > i {
            self.front = self.front.saturating_sub(1);
        }
        Ok(())
    }

    /// **Where tab `key` is in view**: its scroll, the view's size (the
    /// stand-in's, [`VIEW_W`] × [`VIEW_H`]) and the headings in view.
    pub fn in_view(&self, key: &str) -> Result<super::see::InView, String> {
        let tab = self.tab(key)?;
        let doc = dom_query::Document::from(tab.html.as_str());
        let page = Page::read(&doc, tab);
        Ok(page.in_view())
    }

    /// **Tab `key`'s visible text blocks**, each with its ref (numbered
    /// from now on, as the element script numbers what it lists).
    pub fn blocks(&mut self, key: &str) -> Result<Vec<super::see::Block>, String> {
        let blocks = {
            let tab = self.tab(key)?;
            let doc = dom_query::Document::from(tab.html.as_str());
            let page = Page::read(&doc, tab);
            page.blocks()
        };
        self.tab_mut(key)?.seen = true;
        Ok(blocks)
    }

    /// **The element at (`x`, `y`)** of tab `key`'s view: the last element
    /// whose `data-sim-rect="x,y,w,h"` (page pixels) holds that point, as
    /// its nearest link, button or field, else itself; numbered from now on.
    pub fn at(&mut self, key: &str, x: f64, y: f64) -> Result<Described, String> {
        let r = {
            let tab = self.tab(key)?;
            let doc = dom_query::Document::from(tab.html.as_str());
            let page = Page::read(&doc, tab);
            page.at(x, y + tab.scroll as f64)
        };
        self.tab_mut(key)?.seen = true;
        match r {
            Some(r) => self.describe(key, r),
            None => {
                let mut d = self.describe(key, 0)?;
                d.el = None;
                Ok(d)
            }
        }
    }

    /// **The element with the focus** on tab `key` (a click or typing put it
    /// there; Tab moves it).
    pub fn active(&self, key: &str) -> Result<Described, String> {
        let r = self.tab(key)?.focus.unwrap_or(0);
        self.describe(key, r)
    }

    /// **What's in view of tab `key`**, as a PNG: [`VIEW_W`] × [`VIEW_H`],
    /// each pixel's red its scroll (÷ 8, mod 256), green its x, blue its y —
    /// so a check can tell which part of the page, and of the view, it shows.
    pub fn shot(&self, key: &str) -> Result<Vec<u8>, String> {
        let tab = self.tab(key)?;
        let (w, h) = (VIEW_W as u32 / 4, VIEW_H as u32 / 4);
        let red = ((tab.scroll / 8) % 256) as u8;
        let rgba: Vec<u8> = (0..h).flat_map(|y| (0..w).flat_map(move |x| [red, (x * 255 / (w - 1)) as u8, (y * 255 / (h - 1)) as u8, 255])).collect();
        crate::snip::images::encode_png(&rgba, w, h, true)
    }

    /// A page's words and links (`read_open_page` without elements), the
    /// article's links first as the page text script ranks them.
    pub fn text(&self, key: &str) -> Result<PageText, String> {
        let s = self.read(key, 24_000, 200)?;
        let mut t = Self::text_of_snapshot(s);
        self.rank_links(key, &mut t.links)?;
        Ok(t)
    }

    /// **Links in the page text script's order**: the content's first, then
    /// the navigation's, header's, footer's and asides', a language link
    /// (`hreflang`, or another `lang` than the page's) last; else as found.
    fn rank_links(&self, key: &str, links: &mut [super::view::PageLink]) -> Result<(), String> {
        let tab = self.tab(key)?;
        let doc = dom_query::Document::from(tab.html.as_str());
        let page = Page::read(&doc, tab);
        let lang = doc.select("html").attr("lang").map(|l| l.to_ascii_lowercase()).unwrap_or_default();
        let lang = lang.split('-').next().unwrap_or("").to_string();
        let mut rank: HashMap<String, u8> = HashMap::new();
        for n in page.candidates.iter().filter(|n| n.is("a[href]")) {
            let href = join(&tab.url, &n.attr("href").map(|h| h.to_string()).unwrap_or_default());
            let own = n.attr("lang").map(|l| l.to_ascii_lowercase().split('-').next().unwrap_or("").to_string()).unwrap_or_default();
            let r = if n.has_attr("hreflang") || (!own.is_empty() && !lang.is_empty() && own != lang) {
                3
            } else if n.ancestors_it(None).any(|a| a.is("nav, header, footer, aside, [role=navigation], [role=banner], [role=contentinfo], [role=complementary], [role=menu], [role=menubar], [role=tablist]")) {
                2
            } else {
                0
            };
            let e = rank.entry(href).or_insert(r);
            *e = (*e).min(r);
        }
        links.sort_by_key(|l| rank.get(&l.href).copied().unwrap_or(0));
        Ok(())
    }

    /// **A long page's words** (`backend::text_long`): up to `max`
    /// characters, every heading (400 at most) and link, as the long read
    /// gives them.
    pub fn text_up_to(&self, key: &str, max: usize) -> Result<PageText, String> {
        let mut s = self.read(key, max, 2000)?;
        let tab = self.tab(key)?;
        let doc = dom_query::Document::from(tab.html.as_str());
        s.headings = Page::read(&doc, tab).headings_up_to(400);
        let mut t = Self::text_of_snapshot(s);
        self.rank_links(key, &mut t.links)?;
        Ok(t)
    }

    fn text_of_snapshot(s: Snapshot) -> PageText {
        let links = s
            .elements
            .iter()
            .filter(|e| e.role == "link" && !e.href.is_empty())
            .map(|e| super::view::PageLink { text: super::act::rules::name_of(e), href: e.href.clone() })
            .collect();
        PageText { title: s.title, headings: s.headings, text: s.text, links }
    }

    /// **What's in view of tab `key`, drawn `scale` times finer** (1–2):
    /// [`Browser::shot`]'s picture at that many more pixels, the same colours
    /// at the same places. The stand-in draws out of sight, so always at the
    /// scale asked.
    pub fn shot_at(&self, key: &str, scale: f64) -> Result<(Vec<u8>, f64), String> {
        let tab = self.tab(key)?;
        let k = scale.clamp(1.0, 2.0);
        let (w, h) = (((VIEW_W / 4) as f64 * k).round() as u32, ((VIEW_H / 4) as f64 * k).round() as u32);
        let red = ((tab.scroll / 8) % 256) as u8;
        let rgba: Vec<u8> = (0..h).flat_map(|y| (0..w).flat_map(move |x| [red, (x * 255 / (w - 1)) as u8, (y * 255 / (h - 1)) as u8, 255])).collect();
        Ok((crate::snip::images::encode_png(&rgba, w, h, true)?, k))
    }

    /// **Tab `key`'s tables** as Yap's tables script reads the live page
    /// (`read::TABLES_SCRIPT`): every visible `<table>` (not one around
    /// another) and ARIA table or grid, its visible rows, its headers from
    /// `<thead>` or a first row of header cells, a cell's link made
    /// absolute; its caption, `aria-label`, else the heading before it.
    pub fn tables(&self, key: &str) -> Result<super::read::Tables, String> {
        use super::read::{Cell, Table, Tables};
        let tab = self.tab(key)?;
        let doc = dom_query::Document::from(tab.html.as_str());
        let page = Page::read(&doc, tab);
        const GRID: &str = "table, [role=table], [role=grid], [role=treegrid]";
        // The heading before each table, in the page's order.
        let mut before: HashMap<dom_query::NodeId, String> = HashMap::new();
        let mut last = String::new();
        for n in doc.root().descendants_it().filter(|n| n.is_element()) {
            if n.is("h1, h2, h3, h4, h5, h6, [role=heading]") && page.visible(&n) {
                last = Page::text_of(&n);
            } else if n.is(GRID) {
                before.insert(n.id, last.clone());
            }
        }
        let clip = |s: String| s.chars().take(300).collect::<String>();
        let cell = |c: &dom_query::NodeRef| {
            let href = dom_query::Selection::from(*c).select("a[href]").nodes().first().and_then(|a| a.attr("href")).map(|h| join(&tab.url, &h)).filter(|h| h.starts_with("http"));
            Cell { t: clip(Page::text_of(c)), h: href.unwrap_or_default() }
        };
        let head_cell = |c: &dom_query::NodeRef| c.node_name().is_some_and(|n| n.eq_ignore_ascii_case("th")) || c.attr("role").is_some_and(|r| r.eq_ignore_ascii_case("columnheader"));
        let mut out = Vec::new();
        for t in doc.select(GRID).nodes() {
            if !page.visible(t) {
                continue;
            }
            let native = t.node_name().is_some_and(|n| n.eq_ignore_ascii_case("table"));
            let inside = dom_query::Selection::from(*t);
            if native && inside.select("table").exists() {
                continue;
            }
            let rows: Vec<dom_query::NodeRef> = if native {
                inside.select("tr").nodes().to_vec()
            } else {
                inside.select("[role=row]").nodes().iter().filter(|r| r.ancestors_it(None).find(|a| a.is(GRID)).is_some_and(|a| a.id == t.id)).copied().collect()
            };
            let rows: Vec<dom_query::NodeRef> = rows.into_iter().filter(|r| page.visible(r)).collect();
            fn cells<'d>(native: bool, r: &dom_query::NodeRef<'d>) -> Vec<dom_query::NodeRef<'d>> {
                if native {
                    r.element_children().into_iter().filter(|c| c.node_name().is_some_and(|n| n.eq_ignore_ascii_case("td") || n.eq_ignore_ascii_case("th"))).collect()
                } else {
                    dom_query::Selection::from(*r).select("[role=cell], [role=gridcell], [role=columnheader], [role=rowheader]").nodes().to_vec()
                }
            }
            let Some(first) = rows.first() else { continue };
            let in_head = |r: &dom_query::NodeRef| r.ancestors_it(None).any(|a| a.node_name().is_some_and(|n| n.eq_ignore_ascii_case("thead")));
            let header_row = rows.iter().rev().find(|r| native && in_head(r)).copied();
            let (headers, body): (Vec<String>, Vec<dom_query::NodeRef>) = match header_row {
                Some(h) => (cells(native, &h).iter().map(|c| clip(Page::text_of(c))).collect(), rows.iter().filter(|r| !in_head(r)).copied().collect()),
                None if !cells(native, first).is_empty() && cells(native, first).iter().all(head_cell) => (cells(native, first).iter().map(|c| clip(Page::text_of(c))).collect(), rows[1..].to_vec()),
                None => (Vec::new(), rows.clone()),
            };
            let body: Vec<Vec<Cell>> = body.iter().map(|r| cells(native, r).iter().take(30).map(cell).collect::<Vec<Cell>>()).filter(|cs| cs.iter().any(|c| !c.t.is_empty() || !c.h.is_empty())).collect();
            if body.is_empty() || body.iter().map(Vec::len).max().unwrap_or(0).max(headers.len()) < 2 {
                continue;
            }
            let caption = inside.select("caption").nodes().first().map(Page::text_of).filter(|c| !c.is_empty() && native);
            let caption = caption.or_else(|| t.attr("aria-label").map(|a| a.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|a| !a.is_empty())).unwrap_or_else(|| before.get(&t.id).cloned().unwrap_or_default());
            out.push(Table { caption: caption.chars().take(120).collect(), headers, rows: body });
        }
        Ok(Tables { title: title_of(&tab.html), url: tab.url.clone(), tables: out, more: false })
    }

    /// **A page with its elements**, as the element script reads one: its
    /// elements numbered from now on (their refs hold until it navigates).
    pub fn snapshot(&mut self, key: &str, text_max: usize, max: usize) -> Result<Snapshot, String> {
        let s = self.read(key, text_max, max)?;
        if max > 0 {
            self.tab_mut(key)?.seen = true;
        }
        Ok(s)
    }

    /// The page read (no element numbered by it).
    fn read(&self, key: &str, text_max: usize, max: usize) -> Result<Snapshot, String> {
        let tab = self.tab(key)?;
        let doc = dom_query::Document::from(tab.html.as_str());
        let page = Page::read(&doc, tab);
        let mut elements = Vec::new();
        let mut more = false;
        for (i, node) in page.candidates.iter().enumerate() {
            if !page.visible(node) {
                continue;
            }
            if elements.len() >= max {
                more = max > 0;
                break;
            }
            elements.push(page.describe(node, i as u32 + 1));
        }
        Ok(Snapshot {
            title: title_of(&tab.html),
            url: tab.url.clone(),
            text: page.text().chars().take(text_max).collect(),
            headings: page.headings(),
            elements,
            forms: page.forms_info(),
            password: page.password(),
            payment: page.payment(),
            more,
            scroll: Scroll { y: tab.scroll, h: 800, max: 2400 },
        })
    }

    /// **Element `r`** as it is now, with its form.
    pub fn describe(&self, key: &str, r: u32) -> Result<Described, String> {
        let tab = self.tab(key)?;
        let doc = dom_query::Document::from(tab.html.as_str());
        let page = Page::read(&doc, tab);
        // (No read has numbered this page's elements yet: no ref is one.)
        let el = page.node(r).filter(|_| tab.seen).filter(|n| page.visible(n)).map(|n| page.describe(&n, r));
        let form = el.as_ref().filter(|e| e.form >= 0).and_then(|e| page.forms_info().into_iter().find(|f| f.form == e.form));
        // The highest ref given: every candidate and block once a read numbered them.
        let refs = if tab.seen { (page.candidates.len() + page.extra.len()) as u32 } else { 0 };
        Ok(Described { el, form, password: page.password(), payment: page.payment(), title: title_of(&tab.html), headings: page.headings(), url: tab.url.clone(), refs })
    }

    /// Where tab `key` is now, as the element script answers it.
    fn scrolled(&self, key: &str) -> Result<Value, String> {
        let v = self.in_view(key)?;
        let s = v.scroll;
        Ok(json!({ "r": "ok", "scroll": { "x": s.x, "y": s.y, "w": s.w, "h": s.h, "max": s.max, "dpr": s.dpr }, "inView": v.in_view }))
    }

    /// **Scroll** tab `key`: `to` down / up (`amount` screens), top, bottom,
    /// or element `r` into view (centred).
    fn scroll(&mut self, key: &str, a: &Value, r: u32) -> Result<Value, String> {
        let (max, at) = {
            let tab = self.tab(key)?;
            let doc = dom_query::Document::from(tab.html.as_str());
            let page = Page::read(&doc, tab);
            let at = (r > 0).then(|| page.layout().into_iter().find(|(x, _)| *x == r).map(|(_, y)| y));
            (page.in_view().scroll.max, at)
        };
        let step = (VIEW_H as f64 * 0.85 * a["amount"].as_f64().unwrap_or(1.0).clamp(0.1, 10.0)).round() as i64;
        let tab = self.tab_mut(key)?;
        tab.scroll = match at {
            Some(None) => return Ok(json!({ "r": "gone" })),
            Some(Some(y)) => y - VIEW_H / 2 + LINE / 2,
            None => match a["to"].as_str().unwrap_or("down") {
                "up" => tab.scroll - step,
                "top" => 0,
                "bottom" => max,
                _ => tab.scroll + step,
            },
        }
        .clamp(0, max);
        self.scrolled(key)
    }

    /// **An action** (`script.rs`'s, the same answers): `click`, `type`
    /// (`text`, `submit`), `scroll` (`to`, `amount`, `ref`), `clickAt` (`x`,
    /// `y`), `hover`, `check` (`on`) or `key` (`keys`), on element `ref` if
    /// it's still `role` "`name`".
    pub fn act(&mut self, key: &str, op: &str, a: &Value) -> Result<Value, String> {
        let r = a["ref"].as_u64().unwrap_or(0) as u32;
        if op == "scroll" {
            return self.scroll(key, a, r);
        }
        if op == "key" {
            return self.keys(key, a, r);
        }
        if op == "where" {
            // In view: its line within the view, unless it never comes into
            // view (`data-sim-unseen`: inside a part that scrolls apart).
            let seen = {
                let tab = self.tab(key)?;
                let doc = dom_query::Document::from(tab.html.as_str());
                let page = Page::read(&doc, tab);
                let Some(node) = page.node(r).filter(|n| tab.seen && page.visible(n)) else { return Ok(json!({ "r": "gone" })) };
                let y = page.layout().into_iter().find(|(x, _)| *x == r).map(|(_, y)| y);
                let s = page.in_view().scroll;
                !node.has_attr("data-sim-unseen") && y.is_some_and(|y| (s.y..s.y + VIEW_H).contains(&y))
            };
            let mut out = self.scrolled(key)?;
            out["seen"] = json!(seen);
            return Ok(out);
        }
        let d = self.describe(key, r)?;
        let Some(el) = d.el.clone() else { return Ok(json!({ "r": "gone" })) };
        if el.role != a["role"].as_str().unwrap_or("") || el.name != a["name"].as_str().unwrap_or("") {
            return Ok(json!({ "r": "changed" }));
        }
        if el.disabled {
            return Ok(json!({ "r": "disabled" }));
        }
        // A click at a point: still that element there.
        if op == "clickAt" {
            let (x, y) = (a["x"].as_f64().unwrap_or(-1.0), a["y"].as_f64().unwrap_or(-1.0));
            if self.at(key, x, y)?.el.map(|e| e.r) != Some(r) {
                return Ok(json!({ "r": "changed" }));
            }
            let mut done = self.act(key, "click", &json!({ "ref": r, "role": el.role, "name": el.name }))?;
            if let Some(last) = self.log.clicked.last_mut() {
                *last = format!("{last} @ {},{}", x.round(), y.round());
            }
            done["at"] = json!(true);
            return Ok(done);
        }
        if op == "hover" {
            let tab = self.tab(key)?;
            let doc = dom_query::Document::from(tab.html.as_str());
            let page = Page::read(&doc, tab);
            let node = page.node(r);
            let shows = node.and_then(|n| n.attr("data-sim-hover").map(|v| v.to_string()));
            // The pointer moves on: what an earlier hover showed closes, unless
            // the pointer is still inside it (a menu's own item).
            let keep: HashSet<String> = tab
                .shown
                .iter()
                .filter(|id| node.is_some_and(|n| n.ancestors_it(None).any(|a| a.id_attr().is_some_and(|x| *x == ***id))))
                .cloned()
                .collect();
            drop(page);
            self.log.hovered.push(rules::name_of(&el));
            let t = self.tab_mut(key)?;
            t.shown = keep;
            if let Some(id) = shows {
                t.shown.insert(id);
            }
            return Ok(json!({ "r": "ok" }));
        }
        if op == "choose" {
            return self.choose(key, a, r, &el);
        }
        if op == "check" {
            let on = a["on"].as_bool().unwrap_or(true);
            if el.checked.unwrap_or(false) != on {
                self.log.clicked.push(rules::name_of(&el));
                self.tab_mut(key)?.checked.insert(r, on);
            }
            return Ok(json!({ "r": "ok", "checked": on }));
        }
        let tab = self.tab(key)?;
        let doc = dom_query::Document::from(tab.html.as_str());
        let page = Page::read(&doc, tab);
        let Some(node) = page.node(r) else { return Ok(json!({ "r": "gone" })) };
        let attr = |k: &str| node.attr(k).map(|v| v.to_string()).unwrap_or_default();
        let tag = node.node_name().map(|n| n.to_ascii_lowercase()).unwrap_or_default();
        match op {
            "click" => {
                let hide = attr("data-sim-hide");
                let go = attr("data-sim-go");
                let href = el.href.clone();
                let submit = el.submit.then(|| page.submission(&node, None));
                let kind = node.attr("type").map(|t| t.to_ascii_lowercase()).unwrap_or_default();
                let current = tab.url.clone();
                drop(page);
                self.log.clicked.push(rules::name_of(&el));
                self.tab_mut(key)?.focus = Some(r);
                if !hide.is_empty() {
                    self.tab_mut(key)?.hidden.insert(hide);
                }
                if el.role == "checkbox" || el.role == "radio" || kind == "checkbox" || kind == "radio" {
                    let t = self.tab_mut(key)?;
                    let now = !el.checked.unwrap_or(false);
                    t.checked.insert(r, now);
                } else if let Some(Some(url)) = submit {
                    self.navigate(key, &url)?;
                } else if !go.is_empty() || (tag == "a" && !href.is_empty()) {
                    let to = if go.is_empty() { href } else { join(&current, &go) };
                    self.navigate(key, &to)?;
                }
                Ok(json!({ "r": "ok" }))
            }
            "type" => {
                let text = a["text"].as_str().unwrap_or("").to_string();
                let submit = a["submit"].as_bool().unwrap_or(false);
                let value = if tag == "select" {
                    let options = page.options(&node);
                    let want = text.trim().to_lowercase();
                    let pick = options
                        .iter()
                        .find(|(t, v)| t.to_lowercase() == want || v.to_lowercase() == want)
                        .or_else(|| options.iter().find(|(t, _)| t.to_lowercase().contains(&want)))
                        .map(|(t, _)| t.clone());
                    match pick {
                        Some(t) => t,
                        None => return Ok(json!({ "r": "no-option" })),
                    }
                } else if el.editable {
                    // An editable region: the words go in at its end (a
                    // draft there stays), as the element script's insertText.
                    let had = el.value.clone();
                    if had.is_empty() {
                        text.clone()
                    } else {
                        format!("{had}{}{text}", if had.ends_with(' ') { "" } else { " " })
                    }
                } else if rules::takes_text(&el) {
                    text.clone()
                } else {
                    return Ok(json!({ "r": "not-a-field" }));
                };
                drop(page);
                { let t = self.tab_mut(key)?; t.values.insert(r, value.clone()); t.focus = Some(r); }
                self.log.typed.push((field_words(&el), value.clone()));
                if submit {
                    let tab = self.tab(key)?;
                    let doc = dom_query::Document::from(tab.html.as_str());
                    let page = Page::read(&doc, tab);
                    let Some(node) = page.node(r) else { return Ok(json!({ "r": "gone" })) };
                    let url = page.submission(&node, Some(()));
                    drop(page);
                    if let Some(url) = url {
                        self.navigate(key, &url)?;
                    }
                }
                let shown = if rules::sensitive(&el).is_some() { String::new() } else { value };
                Ok(json!({ "r": "ok", "value": shown, "inserted": el.editable }))
            }
            other => Err(format!("Unknown action {other}")),
        }
    }

    /// **An option chosen** (the element script's `choose`): a select's by
    /// its words (or value), a radio button of the group by its label, a
    /// combobox's or listbox's `[role=option]` — the exact words first, else
    /// the first holding them; logged as typing into the field is.
    fn choose(&mut self, key: &str, a: &Value, r: u32, el: &Element) -> Result<Value, String> {
        let want = a["text"].as_str().unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
        let pick = |list: &[(u32, String, String)]| -> Option<(u32, String)> {
            let exact = list.iter().find(|(_, t, v)| t.to_lowercase() == want || v.to_lowercase() == want);
            exact.or_else(|| list.iter().find(|(_, t, _)| t.to_lowercase().contains(&want))).map(|(r, t, _)| (*r, t.clone()))
        };
        let (radio, picked) = {
            let tab = self.tab(key)?;
            let doc = dom_query::Document::from(tab.html.as_str());
            let page = Page::read(&doc, tab);
            let Some(node) = page.node(r) else { return Ok(json!({ "r": "gone" })) };
            let tag = node.node_name().map(|n| n.to_ascii_lowercase()).unwrap_or_default();
            if tag == "select" {
                let list: Vec<(u32, String, String)> = page.options(&node).into_iter().map(|(t, v)| (r, t, v)).collect();
                (false, pick(&list))
            } else if el.kind == "radio" {
                let list: Vec<(u32, String, String)> = page.radio_group(&node).into_iter().map(|(x, l)| (x, l.clone(), l)).collect();
                let list = if list.is_empty() { vec![(r, rules::name_of(el), rules::name_of(el))] } else { list };
                (true, pick(&list))
            } else {
                let list: Vec<(u32, String, String)> = page.aria_options(&node).into_iter().map(|(t, _)| (r, t.clone(), t)).collect();
                (false, pick(&list))
            }
        };
        let Some((at, words)) = picked else { return Ok(json!({ "r": "no-option" })) };
        if radio {
            let group: Vec<u32> = {
                let tab = self.tab(key)?;
                let doc = dom_query::Document::from(tab.html.as_str());
                let page = Page::read(&doc, tab);
                page.node(r).map(|n| page.radio_group(&n).into_iter().map(|(x, _)| x).collect()).unwrap_or_default()
            };
            let t = self.tab_mut(key)?;
            for x in group {
                t.checked.insert(x, x == at);
            }
            t.checked.insert(at, true);
            t.focus = Some(at);
            self.log.clicked.push(words.clone());
            return Ok(json!({ "r": "ok", "value": words, "checked": true }));
        }
        let t = self.tab_mut(key)?;
        t.values.insert(r, words.clone());
        t.focus = Some(r);
        self.log.typed.push((field_words(el), words.clone()));
        Ok(json!({ "r": "ok", "value": words }))
    }

    /// **Keys** pressed in turn (the element script's `key`): in element
    /// `r` first (if it's still `role` "`name`"), else the one with the
    /// focus — Tab moves it, Enter submits a field's form or clicks a link
    /// or button, Space clicks a button or box, the arrows choose a
    /// select's option, and with nothing focused the paging keys scroll.
    fn keys(&mut self, key: &str, a: &Value, r: u32) -> Result<Value, String> {
        if r > 0 {
            let d = self.describe(key, r)?;
            let Some(el) = d.el else { return Ok(json!({ "r": "gone" })) };
            if el.role != a["role"].as_str().unwrap_or("") || el.name != a["name"].as_str().unwrap_or("") {
                return Ok(json!({ "r": "changed" }));
            }
            self.tab_mut(key)?.focus = Some(r);
        }
        let keys: Vec<String> = a["keys"].as_array().map(|k| k.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()).unwrap_or_default();
        for k in &keys {
            self.log.keys.push(k.clone());
            let focus = self.tab(key)?.focus;
            let el = focus.and_then(|f| self.describe(key, f).ok()).and_then(|d| d.el);
            match (k.as_str(), &el) {
                ("Tab" | "Shift+Tab", _) => {
                    let order: Vec<u32> = {
                        let tab = self.tab(key)?;
                        let doc = dom_query::Document::from(tab.html.as_str());
                        let page = Page::read(&doc, tab);
                        page.candidates.iter().enumerate().filter(|(_, n)| page.visible(n) && !n.has_attr("disabled")).map(|(i, _)| i as u32 + 1).collect()
                    };
                    let at = focus.and_then(|f| order.iter().position(|x| *x == f));
                    let next = match (k.as_str(), at) {
                        ("Tab", Some(i)) => order.get(i + 1),
                        ("Tab", None) => order.first(),
                        (_, Some(i)) => i.checked_sub(1).and_then(|j| order.get(j)),
                        (_, None) => order.last(),
                    };
                    if let Some(n) = next.copied() {
                        let t = self.tab_mut(key)?;
                        t.focus = Some(n);
                        t.seen = true;
                    }
                }
                ("Enter", Some(e)) if rules::takes_text(e) && e.options.is_empty() => {
                    let url = {
                        let tab = self.tab(key)?;
                        let doc = dom_query::Document::from(tab.html.as_str());
                        let page = Page::read(&doc, tab);
                        page.node(e.r).and_then(|n| page.submission(&n, Some(())))
                    };
                    if let Some(url) = url {
                        self.navigate(key, &url)?;
                    }
                }
                ("Enter" | "Space", Some(e)) if matches!(e.role.as_str(), "link" | "button" | "checkbox" | "radio" | "switch") => {
                    self.act(key, "click", &json!({ "ref": e.r, "role": e.role, "name": e.name }))?;
                }
                // Escape: what a hover showed closes (the pointer let go).
                ("Escape", _) => {
                    self.tab_mut(key)?.shown.clear();
                }
                ("ArrowDown" | "ArrowUp", Some(e)) if !e.options.is_empty() => {
                    let i = e.options.iter().position(|o| *o == e.value).unwrap_or(0);
                    let j = if k == "ArrowDown" { (i + 1).min(e.options.len() - 1) } else { i.saturating_sub(1) };
                    let pick = e.options[j].clone();
                    self.tab_mut(key)?.values.insert(e.r, pick);
                }
                ("PageDown" | "PageUp" | "Home" | "End" | "Space" | "ArrowDown" | "ArrowUp", None) => {
                    let to = match k.as_str() {
                        "PageUp" | "ArrowUp" => "up",
                        "Home" => "top",
                        "End" => "bottom",
                        _ => "down",
                    };
                    let amount = if k.starts_with("Arrow") { 0.06 } else { 1.0 };
                    self.scroll(key, &json!({ "to": to, "amount": amount }), 0)?;
                }
                _ => {}
            }
        }
        let focus = self.tab(key)?.focus;
        let focused = focus.and_then(|f| self.describe(key, f).ok()).and_then(|d| d.el);
        let mut out = self.scrolled(key)?;
        out["keys"] = json!(keys);
        out["focused"] = serde_json::to_value(focused).unwrap_or(Value::Null);
        Ok(out)
    }
}

/// A frame from a payment provider (its card fields live there, out of the
/// page's reach): Stripe, Adyen, Braintree, PayPal, Checkout.com, Mollie,
/// Square, Klarna, Worldpay. Pure.
pub fn payment_frame(src: &str) -> bool {
    let host = reqwest::Url::parse(src.trim()).ok().and_then(|u| u.host_str().map(str::to_ascii_lowercase)).unwrap_or_default();
    ["stripe.com", "adyen.com", "braintreegateway.com", "paypal.com", "checkout.com", "mollie.com", "squareup.com", "klarna.com", "worldpay.com"]
        .iter()
        .any(|p| host == *p || host.ends_with(&format!(".{p}")))
}

/// A field's words for the log (its name, label, `name` and id, each word
/// once). Pure.
fn field_words(el: &Element) -> String {
    let words = format!("{} {} {} {}", el.name, el.label, el.field, el.id);
    let mut seen: Vec<String> = Vec::new();
    let words: Vec<&str> = words.split_whitespace().filter(|w| !seen.contains(&w.to_lowercase()) && { seen.push(w.to_lowercase()); true }).collect();
    words.join(" ")
}

/// `href` made absolute against `base`. Pure.
fn join(base: &str, href: &str) -> String {
    reqwest::Url::parse(base).and_then(|b| b.join(href.trim())).map(|u| u.to_string()).unwrap_or_else(|_| href.to_string())
}

/// A page's `<title>`, its whitespace folded. Pure.
fn title_of(html: &str) -> String {
    let doc = dom_query::Document::from(html);
    doc.select("title").text().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// One page as read for an operation: its document, the tab's state, its
/// candidates in order.
struct Page<'a> {
    doc: &'a dom_query::Document,
    tab: &'a Tab,
    candidates: Vec<dom_query::NodeRef<'a>>,
    /// The blocks of text that aren't candidates, then the elements with a
    /// `data-sim-rect` that are neither: numbered after the candidates.
    extra: Vec<dom_query::NodeRef<'a>>,
    forms: Vec<dom_query::NodeRef<'a>>,
}

impl<'a> Page<'a> {
    fn read(doc: &'a dom_query::Document, tab: &'a Tab) -> Page<'a> {
        let mut candidates = Vec::new();
        let mut blocks = Vec::new();
        let mut rects = Vec::new();
        let mut forms = Vec::new();
        for node in doc.root().descendants_it() {
            if !node.is_element() {
                continue;
            }
            let name = node.node_name().map(|n| n.to_ascii_lowercase()).unwrap_or_default();
            if name == "form" {
                forms.push(node);
            }
            if matches!(name.as_str(), "script" | "style" | "template" | "head" | "noscript") {
                continue;
            }
            if node.is(CANDIDATE) && !(name == "input" && node.attr("type").is_some_and(|t| t.eq_ignore_ascii_case("hidden"))) {
                candidates.push(node);
            } else if node.is(BLOCK) {
                blocks.push(node);
            } else if node.has_attr("data-sim-rect") {
                rects.push(node);
            }
        }
        blocks.extend(rects);
        Page { doc, tab, candidates, extra: blocks, forms }
    }

    /// The element ref `r` names (a candidate, a block of text, an element
    /// with a rect), as the element script's `R.els`.
    fn node(&self, r: u32) -> Option<dom_query::NodeRef<'a>> {
        let i = r.checked_sub(1)? as usize;
        self.candidates.get(i).or_else(|| self.extra.get(i.checked_sub(self.candidates.len())?)).copied()
    }

    /// `node`'s ref.
    fn ref_of(&self, node: &dom_query::NodeRef) -> Option<u32> {
        if let Some(i) = self.candidates.iter().position(|c| c.id == node.id) {
            return Some(i as u32 + 1);
        }
        self.extra.iter().position(|c| c.id == node.id).map(|j| (self.candidates.len() + j) as u32 + 1)
    }

    /// **Where each visible element is** (page pixels from the top): a
    /// block or candidate a line of its own ([`LINE`]; `data-sim-y` where
    /// it says), one inside another on its line. Each ref and its y.
    fn layout(&self) -> Vec<(u32, i64)> {
        let mut placed: Vec<(dom_query::NodeId, i64)> = Vec::new();
        let mut out = Vec::new();
        let mut next = 0i64;
        for node in self.doc.root().descendants_it().filter(|n| n.is_element()) {
            let Some(r) = self.ref_of(&node) else { continue };
            if !self.visible(&node) {
                continue;
            }
            let inside = node.ancestors_it(None).find_map(|a| placed.iter().find(|(id, _)| *id == a.id).map(|(_, y)| *y));
            let y = match (node.attr("data-sim-y").and_then(|y| y.trim().parse::<i64>().ok()), inside) {
                (Some(y), _) => y,
                (None, Some(y)) => y,
                (None, None) => {
                    let y = next;
                    next += LINE;
                    y
                }
            };
            next = next.max(y + LINE);
            placed.push((node.id, y));
            out.push((r, y));
        }
        out
    }

    /// Where the tab is in view, and the headings there.
    fn in_view(&self) -> super::see::InView {
        let layout = self.layout();
        let height = layout.iter().map(|(_, y)| y + LINE).max().unwrap_or(0);
        let max = (height - VIEW_H).max(0);
        let y = self.tab.scroll.clamp(0, max);
        let in_view = layout
            .iter()
            .filter(|(_, at)| (y..y + VIEW_H).contains(at))
            .filter_map(|(r, _)| self.node(*r))
            .filter(|n| n.is("h1, h2, h3, h4, [role=heading]"))
            .map(|n| Self::text_of(&n).chars().take(100).collect::<String>())
            .filter(|t| !t.is_empty())
            .take(8)
            .collect();
        super::see::InView { scroll: super::see::View { x: 0, y, w: VIEW_W, h: VIEW_H, max, dpr: 1.0 }, in_view }
    }

    /// The visible blocks of text, each with its ref.
    fn blocks(&self) -> Vec<super::see::Block> {
        self.doc
            .root()
            .descendants_it()
            .filter(|n| n.is_element() && n.is(BLOCK) && self.visible(n))
            .filter_map(|n| {
                let text: String = Self::text_of(&n).chars().take(400).collect();
                let tag = n.node_name().map(|t| t.to_ascii_lowercase()).unwrap_or_default();
                let role = n.attr("role").map(|r| r.to_ascii_lowercase()).unwrap_or_default();
                (!text.is_empty()).then(|| self.ref_of(&n).map(|r| super::see::Block { r, tag, role, text })).flatten()
            })
            .collect()
    }

    /// The ref at point (`x`, `y`) of the page (pixels from its top): the
    /// last visible element with a `data-sim-rect` holding it, as its
    /// nearest candidate, else itself.
    fn at(&self, x: f64, y: f64) -> Option<u32> {
        let hit = self
            .doc
            .select("[data-sim-rect]")
            .nodes()
            .iter()
            .filter(|n| self.visible(n))
            .rfind(|n| {
                let r: Vec<f64> = n.attr("data-sim-rect").map(|s| s.split(',').filter_map(|p| p.trim().parse().ok()).collect()).unwrap_or_default();
                r.len() == 4 && x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3]
            })
            .copied()?;
        let candidate = std::iter::once(hit).chain(hit.ancestors_it(None)).find(|n| self.candidates.iter().any(|c| c.id == n.id));
        self.ref_of(&candidate.unwrap_or(hit))
    }

    /// Not hidden: no `hidden` (unless a hover showed it), `aria-hidden=
    /// "true"`, `display: none` or `visibility: hidden` on it or above it,
    /// nor hidden by a click (`data-sim-hide`), nor still to come
    /// (`data-sim-after`).
    fn visible(&self, node: &dom_query::NodeRef) -> bool {
        std::iter::once(*node).chain(node.ancestors_it(None)).filter(|n| n.is_element()).all(|n| {
            let style = n.attr("style").map(|s| s.to_ascii_lowercase().replace(' ', "")).unwrap_or_default();
            let id = n.id_attr().map(|i| i.to_string()).unwrap_or_default();
            (!n.has_attr("hidden") || (!id.is_empty() && self.tab.shown.contains(&id)))
                && n.attr("aria-hidden").is_none_or(|v| !v.eq_ignore_ascii_case("true"))
                && !style.contains("display:none")
                && !style.contains("visibility:hidden")
                && (id.is_empty() || !self.tab.hidden.contains(&id))
                && self.due(&n)
        })
    }

    /// An element with `data-sim-after="<ms>"` is there that long after its
    /// page loaded.
    fn due(&self, n: &dom_query::NodeRef) -> bool {
        n.attr("data-sim-after").and_then(|ms| ms.trim().parse::<u64>().ok()).is_none_or(|ms| self.tab.loaded.elapsed() >= std::time::Duration::from_millis(ms))
    }

    fn text_of(n: &dom_query::NodeRef) -> String {
        n.text().split_whitespace().collect::<Vec<_>>().join(" ")
    }

    /// The `<label>` words for `node`: `label[for=id]`, else the label it's in.
    fn label_of(&self, node: &dom_query::NodeRef) -> String {
        if let Some(id) = node.id_attr().filter(|i| !i.is_empty()) {
            for l in self.doc.select("label[for]").nodes() {
                if l.attr("for").is_some_and(|f| *f == *id) {
                    return Self::text_of(l);
                }
            }
        }
        node.ancestors_it(None).find(|a| a.node_name().is_some_and(|n| n.eq_ignore_ascii_case("label"))).map(|l| Self::text_of(&l)).unwrap_or_default()
    }

    fn name_of(&self, node: &dom_query::NodeRef) -> String {
        let attr = |k: &str| node.attr(k).map(|v| v.split_whitespace().collect::<Vec<_>>().join(" ")).unwrap_or_default();
        if !attr("aria-label").is_empty() {
            return attr("aria-label");
        }
        let tag = node.node_name().map(|n| n.to_ascii_lowercase()).unwrap_or_default();
        let kind = attr("type").to_ascii_lowercase();
        if tag == "input" && matches!(kind.as_str(), "submit" | "button" | "reset") {
            let v = attr("value");
            return if v.is_empty() { kind } else { v };
        }
        if tag == "input" && kind == "image" {
            return attr("alt");
        }
        let label = self.label_of(node);
        if !label.is_empty() {
            return label;
        }
        if matches!(tag.as_str(), "input" | "textarea" | "select") {
            return if attr("placeholder").is_empty() { attr("title") } else { attr("placeholder") };
        }
        let words = Self::text_of(node);
        if !words.is_empty() {
            return words.chars().take(120).collect();
        }
        attr("title")
    }

    fn role_of(node: &dom_query::NodeRef) -> String {
        if let Some(r) = node.attr("role").filter(|r| !r.trim().is_empty()) {
            return r.to_ascii_lowercase();
        }
        let tag = node.node_name().map(|n| n.to_ascii_lowercase()).unwrap_or_default();
        let kind = node.attr("type").map(|t| t.to_ascii_lowercase()).unwrap_or_else(|| "text".into());
        match tag.as_str() {
            "a" => "link",
            "button" | "summary" => "button",
            "select" => "combobox",
            "textarea" => "textbox",
            "input" => match kind.as_str() {
                "submit" | "button" | "reset" | "image" => "button",
                "checkbox" => "checkbox",
                "radio" => "radio",
                "search" => "searchbox",
                "range" => "slider",
                "number" => "spinbutton",
                _ => "textbox",
            },
            _ if node.has_attr("contenteditable") => "textbox",
            other => return other.to_string(),
        }
        .to_string()
    }

    /// The form `node` is in (its index), else -1.
    fn form_of(&self, node: &dom_query::NodeRef) -> i64 {
        let owner = node.attr("form").map(|f| f.to_string());
        if let Some(id) = owner.filter(|f| !f.is_empty()) {
            if let Some(i) = self.forms.iter().position(|f| f.id_attr().is_some_and(|x| *x == *id)) {
                return i as i64;
            }
        }
        node.ancestors_it(None)
            .find(|a| a.node_name().is_some_and(|n| n.eq_ignore_ascii_case("form")))
            .and_then(|f| self.forms.iter().position(|x| x.id == f.id))
            .map_or(-1, |i| i as i64)
    }

    /// A select's options: (words, value).
    fn options(&self, node: &dom_query::NodeRef) -> Vec<(String, String)> {
        dom_query::Selection::from(*node)
            .select("option")
            .nodes()
            .iter()
            .map(|o| {
                let t = Self::text_of(o);
                let v = o.attr("value").map(|v| v.to_string()).unwrap_or_else(|| t.clone());
                (t, v)
            })
            .collect()
    }

    fn describe(&self, node: &dom_query::NodeRef, r: u32) -> Element {
        let attr = |k: &str| node.attr(k).map(|v| v.to_string()).unwrap_or_default();
        let tag = node.node_name().map(|n| n.to_ascii_lowercase()).unwrap_or_default();
        let kind = if tag == "input" { attr("type").to_ascii_lowercase() } else { String::new() };
        let kind = if tag == "input" && kind.is_empty() { "text".to_string() } else { kind };
        let role = Self::role_of(node);
        let form = self.form_of(node);
        let mut e = Element {
            r,
            role: role.clone(),
            name: self.name_of(node).chars().take(120).collect(),
            kind: kind.clone(),
            autocomplete: attr("autocomplete"),
            field: attr("name"),
            id: attr("id"),
            label: self.label_of(node).chars().take(120).collect(),
            placeholder: attr("placeholder"),
            href: if tag == "a" { join(&self.tab.url, &attr("href")) } else { String::new() },
            disabled: node.has_attr("disabled") || attr("aria-disabled") == "true",
            form,
            submit: (tag == "button" && matches!(attr("type").to_ascii_lowercase().as_str(), "" | "submit") && form >= 0) || (tag == "input" && matches!(kind.as_str(), "submit" | "image")),
            search: Self::in_search(node),
            editable: node.attr("contenteditable").is_some_and(|v| !v.eq_ignore_ascii_case("false")),
            ..Default::default()
        };
        e.masked = rules::sensitive(&e).is_some();
        if matches!(kind.as_str(), "checkbox" | "radio") || matches!(role.as_str(), "checkbox" | "radio" | "switch") {
            e.checked = Some(self.tab.checked.get(&r).copied().unwrap_or_else(|| node.has_attr("checked") || attr("aria-checked") == "true"));
        } else if tag == "select" {
            let options = self.options(node);
            e.options = options.iter().take(20).map(|(t, _)| t.clone()).collect();
            let chosen = dom_query::Selection::from(*node).select("option[selected]").nodes().first().map(Self::text_of);
            e.value = self.tab.values.get(&r).cloned().or(chosen).or_else(|| options.first().map(|(t, _)| t.clone())).unwrap_or_default();
        } else if !e.masked && (tag == "textarea" || e.editable || (tag == "input" && !matches!(kind.as_str(), "submit" | "button" | "reset" | "image"))) {
            let default = if tag == "textarea" || e.editable { Self::text_of(node) } else { attr("value") };
            e.value = self.tab.values.get(&r).cloned().unwrap_or(default).chars().take(200).collect();
        }
        // A combobox or listbox that isn't a select: its `[role=option]`s
        // (in it or the list it controls), the chosen one its value.
        if tag != "select" && matches!(role.as_str(), "combobox" | "listbox") {
            let options = self.aria_options(node);
            e.options = options.iter().take(20).map(|(t, _)| t.clone()).collect();
            if tag != "input" && tag != "textarea" && !e.editable {
                let chosen = options.iter().find(|(_, selected)| *selected).map(|(t, _)| t.clone());
                e.value = self.tab.values.get(&r).cloned().or(chosen).unwrap_or_else(|| Self::text_of(node).chars().take(200).collect());
            }
        }
        // A radio button: its group's labels.
        if kind == "radio" {
            let group = self.radio_group(node);
            if group.len() > 1 {
                e.options = group.iter().take(20).map(|(_, l)| l.clone()).collect();
            }
        }
        e
    }

    /// A combobox's or listbox's options: each `[role=option]` in it or the
    /// element it controls / owns, its words and whether it's selected.
    fn aria_options(&self, node: &dom_query::NodeRef) -> Vec<(String, bool)> {
        let ids: Vec<String> = [node.attr("aria-controls"), node.attr("aria-owns")].into_iter().flatten().flat_map(|v| v.split_whitespace().map(str::to_string).collect::<Vec<_>>()).collect();
        let mut roots: Vec<dom_query::NodeRef> = vec![*node];
        for id in &ids {
            if let Some(n) = self.doc.select("[id]").nodes().iter().find(|n| n.id_attr().is_some_and(|x| *x == **id)) {
                roots.push(*n);
            }
        }
        let mut out: Vec<(String, bool)> = Vec::new();
        for root in roots {
            for o in dom_query::Selection::from(root).select("[role=option]").nodes() {
                let t = Self::text_of(o);
                if !t.is_empty() && !out.iter().any(|(x, _)| *x == t) {
                    out.push((t, o.attr("aria-selected").is_some_and(|v| v.eq_ignore_ascii_case("true"))));
                }
            }
        }
        out
    }

    /// A radio button's group: the radios with its `name` in its form (or the
    /// page), each its ref and label (else its value).
    fn radio_group(&self, node: &dom_query::NodeRef) -> Vec<(u32, String)> {
        let Some(name) = node.attr("name").map(|n| n.to_string()).filter(|n| !n.is_empty()) else { return Vec::new() };
        let form = self.form_of(node);
        self.candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| c.attr("type").is_some_and(|t| t.eq_ignore_ascii_case("radio")) && c.attr("name").is_some_and(|n| *n == *name) && self.form_of(c) == form)
            .map(|(i, c)| {
                let label = self.label_of(c);
                (i as u32 + 1, if label.is_empty() { c.attr("value").map(|v| v.to_string()).unwrap_or_default() } else { label })
            })
            .collect()
    }

    /// The page marks `node` a search box: `enterkeyhint` / `inputmode`
    /// "search", or in a `role="search"` region or a `<search>` element.
    fn in_search(node: &dom_query::NodeRef) -> bool {
        let says = |k: &str| node.attr(k).is_some_and(|v| v.eq_ignore_ascii_case("search"));
        says("enterkeyhint")
            || says("inputmode")
            || node.ancestors_it(None).any(|a| a.attr("role").is_some_and(|r| r.eq_ignore_ascii_case("search")) || a.node_name().is_some_and(|n| n.eq_ignore_ascii_case("search")))
    }

    /// A payment card field in `scope`'s candidates (by `autocomplete` or
    /// its words: `rules::sensitive`), or a payment provider's frame.
    fn takes_card(&self, form: Option<i64>) -> bool {
        let card = self.candidates.iter().enumerate().any(|(i, c)| {
            (form.is_none() || form == Some(self.form_of(c))) && rules::sensitive(&self.describe_plain(c, i as u32 + 1)) == Some("payment card details")
        });
        card || (form.is_none() && self.doc.select("iframe[src]").nodes().iter().any(|f| f.attr("src").is_some_and(|s| payment_frame(&s))))
    }

    /// An element described without its value (for the payment check).
    fn describe_plain(&self, node: &dom_query::NodeRef, r: u32) -> Element {
        let attr = |k: &str| node.attr(k).map(|v| v.to_string()).unwrap_or_default();
        Element {
            r,
            kind: attr("type").to_ascii_lowercase(),
            autocomplete: attr("autocomplete"),
            field: attr("name"),
            id: attr("id"),
            name: self.name_of(node),
            label: self.label_of(node),
            placeholder: attr("placeholder"),
            ..Default::default()
        }
    }

    /// A payment card field (or a payment provider's frame) anywhere on the page.
    fn payment(&self) -> bool {
        self.takes_card(None)
    }

    fn forms_info(&self) -> Vec<Form> {
        self.forms
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let sel = dom_query::Selection::from(*f);
                let button = sel
                    .select("button, input[type=submit], input[type=image]")
                    .nodes()
                    .iter()
                    .find(|b| {
                        let t = b.attr("type").map(|t| t.to_ascii_lowercase()).unwrap_or_default();
                        !matches!(t.as_str(), "button" | "reset")
                    })
                    .cloned();
                let action = f.attr("action").map(|a| join(&self.tab.url, &a)).unwrap_or_else(|| self.tab.url.clone());
                Form {
                    form: i as i64,
                    password: sel.select("input[type=password]").exists(),
                    action,
                    method: f.attr("method").map(|m| m.to_ascii_lowercase()).unwrap_or_else(|| "get".into()),
                    submit: button.map(|b| self.name_of(&b)).unwrap_or_default(),
                    payment: self.takes_card(Some(i as i64)) || sel.select("iframe[src]").nodes().iter().any(|x| x.attr("src").is_some_and(|s| payment_frame(&s))),
                    price: rules::has_price(&Self::text_of(f)),
                }
            })
            .collect()
    }

    fn password(&self) -> bool {
        self.doc.select("input[type=password]").exists()
    }

    fn headings(&self) -> Vec<String> {
        self.headings_up_to(30)
    }

    fn headings_up_to(&self, n: usize) -> Vec<String> {
        self.doc.select("h1, h2, h3").nodes().iter().map(Self::text_of).filter(|h| !h.is_empty()).take(n).collect()
    }

    /// The page's main words (its `main` / `article`, else its body), the
    /// hidden parts left out.
    fn text(&self) -> String {
        let html = self.doc.html().to_string();
        let copy = dom_query::Document::from(html.as_str());
        copy.select("script, style, noscript, template, [aria-hidden=true]").remove();
        for n in copy.select("[hidden]").nodes() {
            if !n.id_attr().is_some_and(|id| self.tab.shown.contains(&id.to_string())) {
                n.remove_from_parent();
            }
        }
        for n in copy.select("[data-sim-after]").nodes() {
            if !self.due(n) {
                n.remove_from_parent();
            }
        }
        for n in copy.select("[style]").nodes() {
            let s = n.attr("style").map(|s| s.to_ascii_lowercase().replace(' ', "")).unwrap_or_default();
            if s.contains("display:none") || s.contains("visibility:hidden") {
                n.remove_from_parent();
            }
        }
        for id in &self.tab.hidden {
            for n in copy.select("[id]").nodes() {
                if n.id_attr().is_some_and(|x| *x == **id) {
                    n.remove_from_parent();
                }
            }
        }
        let main = copy.select("main, article, [role=main]");
        let root = if main.exists() { main.first() } else { copy.select("body") };
        let text = root.nodes().first().map(|n| n.formatted_text().to_string()).unwrap_or_default();
        text.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
    }

    /// **The address submitting `node`'s form goes to** (a GET: its fields
    /// in the query, the typed values over the page's), `None` when it's in
    /// no form. `enter`: Enter in a field (the form's own submit button
    /// counts as clicked).
    fn submission(&self, node: &dom_query::NodeRef, enter: Option<()>) -> Option<String> {
        let _ = enter;
        let fi = self.form_of(node);
        if fi < 0 {
            return None;
        }
        let form = &self.forms[fi as usize];
        let action = form.attr("action").map(|a| join(&self.tab.url, &a)).unwrap_or_else(|| self.tab.url.clone());
        let mut pairs: Vec<(String, String)> = Vec::new();
        for (i, c) in self.candidates.iter().enumerate() {
            if self.form_of(c) != fi {
                continue;
            }
            let name = c.attr("name").map(|n| n.to_string()).unwrap_or_default();
            if name.is_empty() || c.has_attr("disabled") {
                continue;
            }
            let e = self.describe(c, i as u32 + 1);
            let tag = c.node_name().map(|n| n.to_ascii_lowercase()).unwrap_or_default();
            if e.role == "button" || e.submit {
                continue;
            }
            if let Some(checked) = e.checked {
                if checked {
                    pairs.push((name, c.attr("value").map(|v| v.to_string()).unwrap_or_else(|| "on".into())));
                }
                continue;
            }
            let value = if tag == "select" {
                let options = self.options(c);
                options.iter().find(|(t, _)| *t == e.value).map(|(_, v)| v.clone()).unwrap_or(e.value)
            } else {
                // A sensitive field's value isn't shown, but a form still sends it.
                self.tab.values.get(&(i as u32 + 1)).cloned().unwrap_or(e.value)
            };
            pairs.push((name, value));
        }
        let mut url = reqwest::Url::parse(&action).ok()?;
        url.set_query(None);
        if !pairs.is_empty() {
            url.query_pairs_mut().extend_pairs(pairs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        }
        Some(url.to_string())
    }
}

// ---- where the stand-in browser lives while it's in use ------------------------------

#[cfg(test)]
thread_local! {
    static SIM: std::cell::RefCell<Option<Browser>> = const { std::cell::RefCell::new(None) };
}

#[cfg(not(test))]
static SIM: std::sync::Mutex<Option<Browser>> = std::sync::Mutex::new(None);

/// Put a stand-in browser in place of the panel's pages (`None`: back to
/// them). A unit test's is its thread's own.
pub fn install(b: Option<Browser>) {
    #[cfg(test)]
    SIM.with(|s| *s.borrow_mut() = b);
    #[cfg(not(test))]
    {
        *SIM.lock().unwrap_or_else(|p| p.into_inner()) = b;
    }
}

/// `f` on the stand-in browser, when one is in place.
pub fn with<R>(f: impl FnOnce(&mut Browser) -> R) -> Option<R> {
    #[cfg(test)]
    {
        SIM.with(|s| s.borrow_mut().as_mut().map(f))
    }
    #[cfg(not(test))]
    {
        SIM.lock().unwrap_or_else(|p| p.into_inner()).as_mut().map(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const HOME: &str = r#"<!doctype html><html><head><title>Example Rail</title></head><body>
      <div id="cookies"><p>We use cookies.</p><button type="button" data-sim-hide="cookies">Reject non-essential</button><button type="button" data-sim-hide="cookies">Accept all</button></div>
      <nav><a href="/timetables">Timetables</a> <a href="/signin">Sign in</a></nav>
      <main><h1>Find a train</h1>
      <form action="results" method="get">
        <label for="from">From</label><input id="from" name="from" autocomplete="off">
        <label>To <input name="to"></label>
        <select name="when" aria-label="When"><option>Today</option><option selected>Tomorrow</option></select>
        <input type="hidden" name="csrf" value="x">
        <button>Find trains</button>
      </form></main></body></html>"#;

    fn browser() -> Browser {
        let mut pages = BTreeMap::new();
        pages.insert("https://rail.example.com/".to_string(), HOME.to_string());
        pages.insert("https://rail.example.com/results?from=Kellbrook&to=Port Ellis".to_string(), "<html><head><title>Results</title></head><body><main><p>Next train 09:42.</p><a href=\"buy?train=1\">Buy tickets</a></main></body></html>".to_string());
        pages.insert("https://rail.example.com/results".to_string(), "<html><head><title>Results</title></head><body><main><p>No trains found for that journey.</p></main></body></html>".to_string());
        Browser::new(&pages)
    }

    #[test]
    fn it_reads_a_page_as_the_element_script_does() {
        let mut b = browser();
        let key = b.open("https://rail.example.com/");
        assert_eq!(key, "page:https://rail.example.com");
        let s = b.snapshot(&key, 5000, 150).unwrap();
        assert_eq!(s.title, "Example Rail");
        let names: Vec<String> = s.elements.iter().map(|e| format!("{} {} {}", e.r, e.role, e.name)).collect();
        assert_eq!(names, ["1 button Reject non-essential", "2 button Accept all", "3 link Timetables", "4 link Sign in", "5 textbox From", "6 textbox To", "7 combobox When", "8 button Find trains"]);
        assert_eq!(s.elements[6].value, "Tomorrow");
        assert_eq!(s.elements[6].options, ["Today", "Tomorrow"]);
        assert!(s.elements[7].submit && s.elements[4].form == 0);
        assert_eq!(s.elements[2].href, "https://rail.example.com/timetables");
        assert_eq!(s.forms[0].submit, "Find trains");
        assert_eq!(s.forms[0].action, "https://rail.example.com/results");
        assert!(s.text.contains("Find a train") && !s.text.contains("Example Rail"), "{}", s.text);
        // The banner's button hides it; the refs stay as they were.
        b.act(&key, "click", &json!({ "ref": 1, "role": "button", "name": "Reject non-essential" })).unwrap();
        let s = b.snapshot(&key, 5000, 150).unwrap();
        assert_eq!(s.elements[0].r, 3, "{:?}", s.elements[0]);
        // A ref whose element changed: refused.
        assert_eq!(b.act(&key, "click", &json!({ "ref": 3, "role": "button", "name": "Timetables" })).unwrap()["r"], "changed");
    }

    #[test]
    fn typing_and_submitting_go_where_the_form_goes() {
        let mut b = browser();
        let key = b.open("https://rail.example.com/");
        // No read yet: no ref is one.
        assert_eq!(b.act(&key, "type", &json!({ "ref": 5, "role": "textbox", "name": "From", "text": "x" })).unwrap()["r"], "gone");
        b.snapshot(&key, 5000, 150).unwrap();
        assert_eq!(b.act(&key, "type", &json!({ "ref": 5, "role": "textbox", "name": "From", "text": "Kellbrook" })).unwrap()["value"], "Kellbrook");
        assert_eq!(b.act(&key, "type", &json!({ "ref": 7, "role": "combobox", "name": "When", "text": "today" })).unwrap()["value"], "Today");
        assert_eq!(b.act(&key, "type", &json!({ "ref": 7, "role": "combobox", "name": "When", "text": "Sunday" })).unwrap()["r"], "no-option");
        b.act(&key, "type", &json!({ "ref": 6, "role": "textbox", "name": "To", "text": "Port Ellis", "submit": true })).unwrap();
        let tab = &b.tabs()[0];
        assert!(tab.url.starts_with("https://rail.example.com/results?from=Kellbrook&to=Port+Ellis&when=Today"), "{}", tab.url);
        assert_eq!(b.text(&key).unwrap().text, "Next train 09:42.\nBuy tickets");
        assert_eq!(b.log.typed.iter().map(|(w, t)| format!("{w}={t}")).collect::<Vec<_>>(), ["From=Kellbrook", "When=Today", "To=Port Ellis"]);
        // A link goes to its address; the hidden field went with the form.
        b.snapshot(&key, 5000, 150).unwrap();
        b.act(&key, "click", &json!({ "ref": 1, "role": "link", "name": "Buy tickets" })).unwrap();
        assert_eq!(b.tabs()[0].url, "https://rail.example.com/buy?train=1");
        assert_eq!(b.snapshot(&key, 500, 150).unwrap().title, "Not found");
        // Another journey: the page without its query.
        b.navigate(&key, "https://rail.example.com/results?from=Nowhere").unwrap();
        assert!(b.text(&key).unwrap().text.contains("No trains found"));
        assert_eq!(b.log.visited.len(), 4);
    }

    #[test]
    fn tabs_open_switch_and_are_told() {
        let mut b = browser();
        let a = b.open("https://rail.example.com/");
        let c = b.open("https://rail.example.com/results?from=Kellbrook&to=Port Ellis");
        let tabs = b.tabs();
        assert_eq!((tabs.len(), tabs[1].front, tabs[1].title.as_str()), (2, true, "Results"));
        b.front(&a).unwrap();
        assert!(b.tabs()[0].front);
        // Opening one already open brings it to the front.
        assert_eq!(b.open("https://rail.example.com/results?from=Kellbrook&to=Port Ellis"), c);
        assert!(b.tabs()[1].front);
        assert!(b.front("page:https://nope.example").is_err());
        assert_eq!(norm("https://WWW.Rail.example.com/results/?to=Port+Ellis&from=Kellbrook", true), "rail.example.com/results?from=kellbrook&to=port ellis");
    }
}

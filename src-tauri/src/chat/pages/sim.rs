//! **A stand-in browser** for the page tools (debug builds and unit tests
//! only): the pages a model evaluation's scenario gives (`browse` scenarios,
//! `ai/eval/`), or a unit test's, read and acted on in Rust — no WebView2,
//! no window, nothing on the internet. It answers everything the panel's
//! real pages do (`pages/backend.rs`): the tabs, a page's text, its
//! elements with refs, describe / click / type / scroll, going somewhere,
//! the tab in front.
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
    /// Every click that went through: the element's words.
    pub clicked: Vec<String>,
}

struct Tab {
    key: String,
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
}

/// **The stand-in browser**: its pages, its tabs, the one in front.
pub struct Browser {
    pages: Vec<(String, String)>,
    tabs: Vec<Tab>,
    front: usize,
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
        Browser { pages: pages.iter().map(|(u, h)| (u.clone(), h.clone())).collect(), tabs: Vec::new(), front: 0, log: Log::default() }
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

    /// **Open `url` in a tab** (`open_page`): its tab in front — the one it
    /// has, or a new one. Its key.
    pub fn open(&mut self, url: &str) -> String {
        let key = super::view::key_of(url);
        if let Some(i) = self.tabs.iter().position(|t| t.key == key) {
            self.front = i;
            return key;
        }
        let html = self.page_for(url);
        self.log.visited.push(url.to_string());
        self.tabs.push(Tab { key: key.clone(), url: url.to_string(), html, values: HashMap::new(), checked: HashMap::new(), hidden: HashSet::new(), scroll: 0, seen: false });
        self.front = self.tabs.len() - 1;
        key
    }

    /// The tabs, as `view::open_tabs` tells them.
    pub fn tabs(&self) -> Vec<OpenTab> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(i, t)| OpenTab { tab: i + 1, key: t.key.clone(), title: title_of(&t.html), url: t.url.clone(), front: i == self.front })
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
        Ok(())
    }

    /// **Tab `key` goes to `url`** (an address, a link, a form).
    pub fn navigate(&mut self, key: &str, url: &str) -> Result<(), String> {
        let html = self.page_for(url);
        let tab = self.tab_mut(key)?;
        tab.url = url.to_string();
        tab.html = html;
        tab.values.clear();
        tab.checked.clear();
        tab.hidden.clear();
        tab.scroll = 0;
        tab.seen = false;
        self.log.visited.push(url.to_string());
        Ok(())
    }

    /// A page's words (`read_open_page` without elements).
    pub fn text(&self, key: &str) -> Result<PageText, String> {
        let s = self.read(key, 24_000, 0)?;
        Ok(PageText { title: s.title, headings: s.headings, text: s.text })
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
        let el = r.checked_sub(1).filter(|_| tab.seen).and_then(|i| page.candidates.get(i as usize)).filter(|n| page.visible(n)).map(|n| page.describe(n, r));
        let form = el.as_ref().filter(|e| e.form >= 0).and_then(|e| page.forms_info().into_iter().find(|f| f.form == e.form));
        Ok(Described { el, form, password: page.password(), title: title_of(&tab.html), headings: page.headings(), url: tab.url.clone() })
    }

    /// **An action** (`script.rs`'s, the same answers): `click`, `type`
    /// (`text`, `submit`) or `scroll` (`to`, `ref`), on element `ref` if it's
    /// still `role` "`name`".
    pub fn act(&mut self, key: &str, op: &str, a: &Value) -> Result<Value, String> {
        let r = a["ref"].as_u64().unwrap_or(0) as u32;
        if op == "scroll" {
            let tab = self.tab_mut(key)?;
            tab.scroll = match (r, a["to"].as_str()) {
                (0, Some("up")) => (tab.scroll - 640).max(0),
                (0, _) => (tab.scroll + 640).min(2400),
                (r, _) => (i64::from(r) * 40).min(2400),
            };
            return Ok(json!({ "r": "ok", "scroll": { "y": tab.scroll, "h": 800, "max": 2400 } }));
        }
        let d = self.describe(key, r)?;
        let Some(el) = d.el.clone() else { return Ok(json!({ "r": "gone" })) };
        if el.role != a["role"].as_str().unwrap_or("") || el.name != a["name"].as_str().unwrap_or("") {
            return Ok(json!({ "r": "changed" }));
        }
        if el.disabled {
            return Ok(json!({ "r": "disabled" }));
        }
        let tab = self.tab(key)?;
        let doc = dom_query::Document::from(tab.html.as_str());
        let page = Page::read(&doc, tab);
        let node = page.candidates[(r - 1) as usize];
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
                } else if rules::takes_text(&el) {
                    text.clone()
                } else {
                    return Ok(json!({ "r": "not-a-field" }));
                };
                let words = format!("{} {} {} {}", el.name, el.label, el.field, el.id);
                drop(page);
                self.tab_mut(key)?.values.insert(r, value.clone());
                let mut seen: Vec<String> = Vec::new();
                let words: Vec<&str> = words.split_whitespace().filter(|w| !seen.contains(&w.to_lowercase()) && { seen.push(w.to_lowercase()); true }).collect();
                self.log.typed.push((words.join(" "), value.clone()));
                if submit {
                    let tab = self.tab(key)?;
                    let doc = dom_query::Document::from(tab.html.as_str());
                    let page = Page::read(&doc, tab);
                    let node = page.candidates[(r - 1) as usize];
                    let url = page.submission(&node, Some(()));
                    drop(page);
                    if let Some(url) = url {
                        self.navigate(key, &url)?;
                    }
                }
                let shown = if rules::sensitive(&el).is_some() { String::new() } else { value };
                Ok(json!({ "r": "ok", "value": shown }))
            }
            other => Err(format!("Unknown action {other}")),
        }
    }
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
    forms: Vec<dom_query::NodeRef<'a>>,
}

impl<'a> Page<'a> {
    fn read(doc: &'a dom_query::Document, tab: &'a Tab) -> Page<'a> {
        let mut candidates = Vec::new();
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
            }
        }
        Page { doc, tab, candidates, forms }
    }

    /// Not hidden: no `hidden`, `aria-hidden="true"`, `display: none` or
    /// `visibility: hidden` on it or above it, nor hidden by a click
    /// (`data-sim-hide`).
    fn visible(&self, node: &dom_query::NodeRef) -> bool {
        std::iter::once(*node).chain(node.ancestors_it(None)).filter(|n| n.is_element()).all(|n| {
            let style = n.attr("style").map(|s| s.to_ascii_lowercase().replace(' ', "")).unwrap_or_default();
            let id = n.id_attr().map(|i| i.to_string()).unwrap_or_default();
            !n.has_attr("hidden")
                && n.attr("aria-hidden").is_none_or(|v| !v.eq_ignore_ascii_case("true"))
                && !style.contains("display:none")
                && !style.contains("visibility:hidden")
                && (id.is_empty() || !self.tab.hidden.contains(&id))
        })
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
        } else if !e.masked && (tag == "textarea" || (tag == "input" && !matches!(kind.as_str(), "submit" | "button" | "reset" | "image"))) {
            let default = if tag == "textarea" { Self::text_of(node) } else { attr("value") };
            e.value = self.tab.values.get(&r).cloned().unwrap_or(default).chars().take(200).collect();
        }
        e
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
                }
            })
            .collect()
    }

    fn password(&self) -> bool {
        self.doc.select("input[type=password]").exists()
    }

    fn headings(&self) -> Vec<String> {
        self.doc.select("h1, h2, h3").nodes().iter().map(Self::text_of).filter(|h| !h.is_empty()).take(30).collect()
    }

    /// The page's main words (its `main` / `article`, else its body), the
    /// hidden parts left out.
    fn text(&self) -> String {
        let html = self.doc.html().to_string();
        let copy = dom_query::Document::from(html.as_str());
        copy.select("script, style, noscript, template, [hidden], [aria-hidden=true]").remove();
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

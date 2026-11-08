//! **The browser beside the chat** (2026-11-01; one toolset since 8 Nov
//! 2026): Chat's AI uses the live pages in the panel beside the chat as the
//! Claude Code app's own browser is used — one family of `browser_*` tools,
//! the same for every model that has the pages (the tool loop on a key or
//! this PC, and plan chats over the run endpoint, `claude_live/eyes.rs`),
//! one implementation behind it.
//!
//! - **Going and reading** (every model with the pages, [`on`]):
//!   `browser_navigate {url | search | "back" | "forward", tab?}` (the tab
//!   in front, or a new one when none is open: a Page tab, `stream::Event::
//!   Page`, kept with the answer), `browser_read {mode?, tab?}` (its text
//!   and links, or its links, buttons and fields by ref), `browser_find
//!   {query}`, `browser_scroll {direction, amount?}`, `browser_scroll_to
//!   {ref | text}`, `browser_wait {seconds?, text?}`, `browser_tabs {action:
//!   list | new | switch | close | width}` ([`see`] has the viewing ones;
//!   width: the tab laid out that many CSS pixels wide — a site's desktop
//!   layout — and shown to the person scaled to fit the panel, so what they
//!   see is what the model reads and clicks in: [`width`]).
//! - **Looking** (a model that reads images): `browser_screenshot` — what's
//!   in view now, in the tab's own webview at its scroll — and `browser_zoom
//!   {region}`, a sharper crop of part of it. A shot is for the model: it
//!   comes back in its tool result (the next step's message here, MCP image
//!   content on the plan) and shows in the thread inside the turn's steps as
//!   a thumbnail on its step (`stream::Event::Shot`, `ShownPicture::chip`),
//!   never as a tab or a pill; `show_picture` of it shows it to the person.
//! - **Acting** (the models that may, `act::can_act`): `browser_click {ref |
//!   x, y, button?, count?}` (a double, triple, right or middle click too),
//!   `browser_type`, `browser_key`, `browser_hover`, `browser_drag {from,
//!   to}`, `browser_form_input` — every rule in Rust (`act/rules.rs`), a
//!   click, an Enter or a drop that would buy, pay, book, send, delete or
//!   sign in asked first on the person's card.
//! - **`browser_batch {actions}`**: several of them in order in one call,
//!   stopping at the first that fails (a card said no to is one).
//!
//! **Which pages** ([`checked`]): before anything from outside came into
//! the turn, any well-formed public `https://` address, the model's own too
//! ([`written`]); after it, only a page from this turn's results, the
//! person's messages, a site they named or a link on a page read, as
//! `read_page`'s rule (`web::Turn::allowed`) — and only public http(s)
//! addresses (`web::check_url`); never a search engine's results read. What
//! a read, a find, a shot or an action's answer brings in is web content
//! (the safety rule). Every tool reaches the pages through [`backend`]: the
//! panel's own webviews, or a model evaluation's stand-in browser (`sim.rs`).
//!
//! **Older names** (`open_page`, `read_open_page`, `snap_page`, `switch_tab`,
//! `click`, `type_text`, `scroll`, `go_to`: until 8 Nov 2026) are the new
//! tools ([`renamed`]): a conversation's earlier steps, an older chip.
//!
//! **Privacy**: the log never has an address, a title, a section or a
//! page's words — counts and ids only.

/// **Yap's page scripts see inside open shadow roots** (8 Nov 2026, Yap
/// Lab: MDN's header search is a web component, `<mdn-search-button>`, its
/// button, dialog and box inside its shadow root — a click hit the host and
/// "nothing visible changed", typing in it was "not a field", a find found
/// nothing). The JavaScript every page script starts with (`act/script.rs`'s
/// library, `view::PAGE_TEXT_SCRIPT`, `read::TABLES_SCRIPT`), a literal for
/// `concat!`: the page's open shadow roots found once a run (`shs`: every
/// element looked at once, nested roots too, at most 80,000 elements and 400
/// roots), and the light DOM's ways through them —
/// - `qa(sel, root?, lim?)` / `q1`: `querySelectorAll` / `querySelector`
///   through the roots, in the page's order (a host's shadow tree right after
///   it); a page with none: the plain query, exactly as before;
/// - `up(el, sel)`: `closest` past each host; `hostOf(el)`;
/// - `ptAt(x, y)`: `elementFromPoint`, down through each
///   `shadowRoot.elementFromPoint` (it stops at the host);
/// - `focusIn()`: `activeElement`, down through each `shadowRoot.activeElement`;
/// - `holds(a, b)`: `contains` across roots; `byId(el, id)`: an id in `el`'s
///   own root first (`aria-labelledby` and `aria-controls` point there);
/// - `textOf(el)`: its words, through its shadow tree and slots when its own
///   are none (`flat`); `shownText(m)`: `m`'s words as `innerText` gives
///   them, a host's shadow tree read in its place.
///
/// **Same-origin frames too** (8 Nov 2026): an `<iframe>` or `<frame>`
/// whose document the page's script can reach (`fdoc`: its
/// `contentDocument`, same origin) is walked as a shadow root is — its
/// document one of the trees, the frame element its host (at most 40 frames,
/// within the same bounds) — so every query, the element at a point (its
/// coordinates moved by the frame's place: `fOff`), the focused field
/// (through each frame's `activeElement`), `closest` past it and the text read
/// go inside it; `cs(e)` and `win(e)` are an element's own window's styles
/// and window (its events are made there). A frame from another site
/// (or sandboxed) can't be reached by any script outside it: `fdoc` is null,
/// and it's told as one (`act/script.rs` `desc`: `foreign`).
///
/// Closed roots can't be reached (nothing outside sees in).
macro_rules! deep_js {
    () => {
        r#"var SH_=null;
function fdoc(e){if(!e||(e.tagName!=='IFRAME'&&e.tagName!=='FRAME'))return null;try{var d=e.contentDocument;return d&&d.documentElement?d:null;}catch(x){return null;}}
function shs(){if(SH_)return SH_;var s={trees:[document],hosts:new Map(),inner:new Map(),n:0,frames:0};for(var i=0;i<s.trees.length;i++){var r=s.trees[i],hs=[],all=r.querySelectorAll('*');for(var j=0;j<all.length&&s.n<80000;j++){s.n++;var el=all[j],x=el.shadowRoot;if(!x&&(el.tagName==='IFRAME'||el.tagName==='FRAME')&&s.frames<40){x=fdoc(el);if(x)s.frames++;}if(x&&s.trees.length<400){hs.push(el);s.trees.push(x);s.inner.set(el,x);}}s.hosts.set(r,hs);}SH_=s;return s;}
function deep(){return shs().trees.length>1;}
function inner(h){return shs().inner.get(h)||h.shadowRoot||null;}
function cs(e){var v=e&&e.ownerDocument&&e.ownerDocument.defaultView;return (v||window).getComputedStyle(e);}
function win(e){return (e&&e.ownerDocument&&e.ownerDocument.defaultView)||window;}
function fOff(d){var x=0,y=0;for(var n=0;d&&d!==document&&n<16;n++){var fe=null;try{fe=d.defaultView&&d.defaultView.frameElement;}catch(e){}if(!fe)break;var b=fe.getBoundingClientRect(),c=cs(fe);x+=b.left+fe.clientLeft+(parseFloat(c.paddingLeft)||0);y+=b.top+fe.clientTop+(parseFloat(c.paddingTop)||0);d=fe.ownerDocument;}return {x:x,y:y};}
function rr(el){var b=el.getBoundingClientRect();if(win(el)===window)return b;var o=fOff(el.ownerDocument);return {left:b.left+o.x,top:b.top+o.y,right:b.right+o.x,bottom:b.bottom+o.y,width:b.width,height:b.height};}
function hostsIn(r){var t=r.nodeType===1?r.getRootNode():r,hs=shs().hosts.get(t)||[];return r.nodeType===1?hs.filter(function(h){return h!==r&&r.contains(h);}):hs;}
function qwalk(r,sel,out,dp,lim){var ir=r.nodeType===1?inner(r):null;if(ir&&dp<32)qwalk(ir,sel,out,dp+1,lim);var hits=r.querySelectorAll(sel),hs=hostsIn(r),i=0;
for(var j=0;j<hs.length&&out.length<lim;j++){var lo=i,hi=hits.length;while(lo<hi){var md=(lo+hi)>>1,h=hits[md];if(h===hs[j]||(hs[j].compareDocumentPosition(h)&2))lo=md+1;else hi=md;}
for(;i<lo&&out.length<lim;i++)out.push(hits[i]);var hr=inner(hs[j]);if(hr&&dp<32)qwalk(hr,sel,out,dp+1,lim);}
for(;i<hits.length&&out.length<lim;i++)out.push(hits[i]);}
function qa(sel,r,lim){r=r||document;if(!deep())return r.querySelectorAll(sel);var out=[];qwalk(r,sel,out,0,lim||50000);return out;}
function q1(sel,r){r=r||document;if(!deep())return r.querySelector(sel);return qa(sel,r,1)[0]||null;}
function hostOf(el){var r=el&&el.getRootNode?el.getRootNode():null;if(!r)return null;if(r.host)return r.host;if(r!==document&&r.nodeType===9){try{return (r.defaultView&&r.defaultView.frameElement)||null;}catch(x){return null;}}return null;}
function up(el,sel){for(var n=0;el&&n<40;n++){var c=el.closest(sel);if(c)return c;el=hostOf(el);}return null;}
function ptAt(x,y){var lx=x,ly=y,e=document.elementFromPoint(x,y);for(var n=0;e&&n<32;n++){if(e.shadowRoot){var i=e.shadowRoot.elementFromPoint(lx,ly);if(!i||i===e)break;e=i;continue;}var fd=fdoc(e);if(!fd)break;var o=fOff(fd);lx=x-o.x;ly=y-o.y;var i2=fd.elementFromPoint(lx,ly);if(!i2)break;e=i2;}return e;}
function focusIn(){var a=document.activeElement;for(var n=0;a&&n<32;n++){if(a.shadowRoot&&a.shadowRoot.activeElement){a=a.shadowRoot.activeElement;continue;}var fd=fdoc(a);if(!fd||!fd.activeElement)break;a=fd.activeElement;}return a;}
function bare(e){return !e||!e.ownerDocument||e===e.ownerDocument.body||e===e.ownerDocument.documentElement;}
function holds(a,b){for(var n=0;b&&n<40;n++){if(a===b||a.contains(b))return true;b=hostOf(b);}return false;}
function byId(el,id){var r=el.getRootNode?el.getRootNode():null,x=r&&r!==document&&r.getElementById?r.getElementById(id):null;return x||document.getElementById(id);}
function sqz(s){return (s||'').replace(/\s+/g,' ').trim();}
function flat(e,b){b=b||{s:'',n:0};var kids=e.shadowRoot?e.shadowRoot.childNodes:(e.tagName==='SLOT'&&e.assignedNodes().length?e.assignedNodes():e.childNodes);
for(var i=0;i<kids.length&&b.n<4000&&b.s.length<20000;i++){var k=kids[i];b.n++;if(k.nodeType===3){b.s+=k.data;continue;}if(k.nodeType!==1)continue;var tn=k.tagName;if(tn==='SCRIPT'||tn==='STYLE'||tn==='TEMPLATE'||tn==='NOSCRIPT')continue;var st=cs(k);if(st.display==='none')continue;var bl=st.display.indexOf('inline')!==0;if(bl)b.s+=' ';flat(k,b);if(bl)b.s+=' ';}return b.s;}
function textOf(e){var s=sqz(e.innerText||e.textContent);if(!s&&deep()&&(e.shadowRoot||e.querySelector('slot')))s=sqz(flat(e));return s;}
function shownText(m){if(!m)return '';if(!deep())return m.innerText||'';var on=new Set();shs().hosts.forEach(function(v){for(var i=0;i<v.length;i++){var e=v[i];for(var n=0;e&&n<200&&!on.has(e);n++){on.add(e);e=e.parentElement||hostOf(e);}}});
var b={s:'',n:0};(function go(e){if(b.n>30000||b.s.length>260000)return;b.n++;var st=cs(e);if(st.display==='none')return;if(!on.has(e)){b.s+=' '+(e.innerText||'')+' ';return;}
var ir=inner(e),kids=ir?(ir.nodeType===9?[ir.body||ir.documentElement]:ir.childNodes):e.childNodes;for(var i=0;i<kids.length;i++){var k=kids[i];if(!k)continue;if(k.nodeType===3){if(st.visibility!=='hidden')b.s+=k.data;}else if(k.nodeType===1){var tn=k.tagName;if(tn==='SLOT'){var as=k.assignedNodes();if(!as.length)as=k.childNodes;for(var j=0;j<as.length;j++){if(as[j].nodeType===3)b.s+=as[j].data;else if(as[j].nodeType===1)go(as[j]);}}else if(tn!=='SCRIPT'&&tn!=='STYLE'&&tn!=='TEMPLATE'&&tn!=='NOSCRIPT')go(k);}}})(m);return b.s;}
function mainEl(){return document.querySelector('main,article,[role=main]')||(deep()?q1('main,[role=main]'):null)||document.body;}
function frameOf(f){var sb=f.getAttribute('sandbox'),src='';try{src=new URL(f.getAttribute('src')||'about:blank',location.href).href;}catch(e){}return {src:src,sandboxed:sb!==null&&!/allow-same-origin/i.test(sb),title:sqz(f.getAttribute('title')||f.getAttribute('aria-label')||f.getAttribute('name')||'').slice(0,80)};}
function foreignFrames(r,max){var out=[],fs=qa('iframe,frame',r);for(var i=0;i<fs.length&&out.length<max;i++){var f=fs[i];if(fdoc(f))continue;var b=f.getBoundingClientRect();if(b.width<20||b.height<20||cs(f).visibility==='hidden')continue;out.push(frameOf(f));}return out;}
"#
    };
}

pub mod act;
pub mod backend;
pub mod read;
pub mod see;
#[cfg(any(test, debug_assertions))]
pub mod sim;
pub mod view;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::chat::pictures::{Gate, Look};
use crate::chat::stream::{Event, Live};
use view::OpenTab;

/// **A page an answer brought up** beside the chat (the same keys in JS).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShownPage {
    /// Its address, as the search or the person gave it.
    #[serde(default)]
    pub url: String,
    /// Its title (the search result's or the page read's; else its site).
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub site: String,
    /// Where it's scrolled to: a heading, or the words of one (an older
    /// answer's `open_page { section }`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub section: String,
}

/// **A button an older answer offers**: open `url` in the person's browser
/// ("Open the interactive map in Brave"). No model is offered
/// `offer_browser` since 1 Nov 2026 (the panel has Open in …).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserOffer {
    #[serde(default)]
    pub url: String,
    /// What it opens, in a few words ("the interactive map").
    #[serde(default)]
    pub why: String,
    #[serde(default)]
    pub site: String,
}

pub const NAVIGATE: &str = "browser_navigate";
pub const READ: &str = "browser_read";
pub const FIND: &str = "browser_find";
pub const SCROLL: &str = "browser_scroll";
pub const SCROLL_TO: &str = "browser_scroll_to";
pub const SCREENSHOT: &str = "browser_screenshot";
pub const ZOOM: &str = "browser_zoom";
pub const TABS: &str = "browser_tabs";
pub const WAIT: &str = "browser_wait";
pub const BATCH: &str = "browser_batch";
/// The tools every model with the pages gets (the view's only one that
/// reads images: [`VIEW`]; the batch only where there are steps worth
/// batching: [`offers`]), in the order they're listed.
pub const TOOL_NAMES: &[&str] = &[NAVIGATE, READ, FIND, SCROLL, SCROLL_TO, SCREENSHOT, ZOOM, TABS, WAIT, BATCH];
/// The view's tools: where it is and what it shows, only for a model that
/// reads images (one that can't reads the whole page with browser_read; on
/// the small on-device model's 8K context every tool's schema counts —
/// 8 Nov 2026's evals).
pub const VIEW: &[&str] = &[SCROLL, SCROLL_TO, SCREENSHOT, ZOOM];
/// Every browser tool, the acting ones too, in the order they're listed.
pub const ALL: &[&str] = &[NAVIGATE, READ, FIND, SCROLL, SCROLL_TO, SCREENSHOT, ZOOM, act::CLICK, act::TYPE, act::KEY, act::HOVER, act::DRAG, act::FORM_INPUT, TABS, WAIT, BATCH];
/// The tools that may go to an address the model writes (an older name's
/// too): only they need to know whether outside content came first
/// ([`Ctx::outside`]).
pub const NAVIGATES: &[&str] = &[NAVIGATE, TABS, BATCH, old::OPEN, old::SHOW, old::GO];

/// The page tools' names until 8 Nov 2026 ([`renamed`]).
pub mod old {
    pub const OPEN: &str = "open_page";
    /// `open_page`'s name before 1 Nov 2026.
    pub const SHOW: &str = "show_page";
    pub const READ: &str = "read_open_page";
    pub const SNAP: &str = "snap_page";
    pub const CLICK: &str = "click";
    pub const TYPE: &str = "type_text";
    pub const SCROLL: &str = "scroll";
    pub const GO: &str = "go_to";
    pub const SWITCH: &str = "switch_tab";
    /// Every one of them (`offer_browser`, offered to no model since 1 Nov
    /// 2026, isn't a tool now: an older answer's buttons stay).
    #[cfg(test)]
    pub const ALL: &[&str] = &[OPEN, SHOW, READ, SNAP, CLICK, TYPE, SCROLL, GO, SWITCH];
}

/// What `browser_read` gives of a page's text, at most (characters).
pub const READ_MAX: usize = 5000;
/// What `browser_read` gives of a page's links, at most.
pub const LINKS_MAX: usize = 40;
/// Pages a turn may open in new tabs.
pub const MAX_PAGES: usize = 4;
/// The steps a `browser_batch` may run.
pub const MAX_BATCH: usize = 10;
/// A section's words, at most (an older answer's).
pub const SECTION_MAX: usize = 80;

/// **An older name's call** as the toolset's: its tool and arguments, or
/// `None` for a name that isn't one (an earlier step of a conversation, a
/// model that remembers them). Pure.
pub fn renamed(name: &str, args: &Value) -> Option<(&'static str, Value)> {
    let mut out = json!({});
    let tab = &args["tab"];
    let keep = |out: &mut Value, keys: &[&str]| {
        for k in keys {
            if !args[*k].is_null() {
                out[*k] = args[*k].clone();
            }
        }
    };
    let tool = match name {
        old::OPEN | old::SHOW => {
            out["action"] = json!("new");
            keep(&mut out, &["url", "search"]);
            return Some((TABS, out));
        }
        old::READ => {
            if args["elements"].as_bool() == Some(true) || args["elements"].as_str().is_some_and(|s| s.eq_ignore_ascii_case("true")) {
                out["mode"] = json!("elements");
            }
            READ
        }
        old::SNAP => SCREENSHOT,
        old::CLICK => {
            keep(&mut out, &["ref"]);
            act::CLICK
        }
        old::TYPE => {
            keep(&mut out, &["ref", "text", "submit"]);
            act::TYPE
        }
        old::SCROLL => match args["to"].as_str().map(|s| s.trim().to_ascii_lowercase()) {
            Some(d) if matches!(d.as_str(), "up" | "down" | "top" | "bottom" | "") => {
                out["direction"] = json!(if d.is_empty() { "down".to_string() } else { d });
                SCROLL
            }
            _ => {
                out["ref"] = args["to"].clone();
                SCROLL_TO
            }
        },
        old::GO => {
            keep(&mut out, &["url", "search"]);
            NAVIGATE
        }
        old::SWITCH => {
            out["action"] = json!("switch");
            keep(&mut out, &["tab"]);
            return Some((TABS, out));
        }
        _ => return None,
    };
    if !tab.is_null() {
        out["tab"] = tab.clone();
    }
    Some((tool, out))
}

/// Whether the browser tools are offered: to every model that gets the
/// picture tools — any tool-using model with the web on (`pictures::Gate`).
/// Pure.
pub fn on(gate: Gate) -> bool {
    gate != Gate::Off
}

/// Whether `name` is a browser tool offered with `gate` (`act`: this model
/// may act on pages, `act::can_act`): the view's only to a model that reads
/// images, the batch to one that acts or sees (a model that only reads has
/// nothing to string together); an older name as the tool it is now. Pure.
pub fn offers(gate: Gate, act: bool, name: &str) -> bool {
    if !on(gate) {
        return false;
    }
    let name = renamed(name, &Value::Null).map_or(name, |(n, _)| n);
    if VIEW.contains(&name) {
        return gate == Gate::Look;
    }
    if name == BATCH {
        return act || gate == Gate::Look;
    }
    TOOL_NAMES.contains(&name) || (act && act::TOOL_NAMES.contains(&name))
}

/// Whether `name` is one of the acting tools (an older name too). Pure.
pub fn acts(name: &str) -> bool {
    let name = renamed(name, &Value::Null).map_or(name, |(n, _)| n);
    act::TOOL_NAMES.contains(&name)
}

/// A schema's `tab` (the line says what it is: [`INSTRUCTIONS`]).
fn tab_prop() -> Value {
    json!({ "type": "integer" })
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    let mut parameters = json!({ "type": "object", "properties": properties });
    if !required.is_empty() {
        parameters["required"] = json!(required);
    }
    json!({ "type": "function", "function": { "name": name, "description": description, "parameters": parameters } })
}

/// **The tools' schemas** for `gate` (none when it's off), the acting ones
/// for a model that may act (`act`). Kept short — every step sends them,
/// and a model on this PC has a small context (8 Nov 2026: with the acting
/// tools in full, a strong model's first request on an 8K context was too
/// long): what they share said once, in the line. Every tool takes `tab`
/// (8 Nov 2026, Yap Lab: with it only where a tab is chosen, a model that
/// went to a page in tab 2 waited for its words on the tab in front); an
/// action on a tab out of sight brings it to the front first
/// (`act::to_front`).
pub fn definitions(gate: Gate, act: bool) -> Value {
    if !on(gate) {
        return json!([]);
    }
    let tab = tab_prop;
    let r = || json!({ "type": "integer" });
    let s = || json!({ "type": "string" });
    let look = gate == Gate::Look;
    // A model that acts is told what the elements are for, first (8 Nov
    // 2026: with "text" first and no word on it, the on-device Qwen read the
    // text before a click in two runs of three, then answered with the link).
    let mode = if act {
        json!({ "type": "string", "enum": ["elements", "text", "tables"], "description": "elements: its links, buttons and fields by ref, to act on; text: all its words" })
    } else {
        json!({ "type": "string", "enum": ["text", "elements", "tables"] })
    };
    let mut out = vec![
        // (search: a results page for the person, never read — what finds
        // pages to open is web search; every word here counts on a small
        // context: turn/fit.rs.)
        tool(NAVIGATE, "Open a page in the tab in front (a new tab if none): an address or search words (results for the user: web search finds pages); or \"back\"/\"forward\".", json!({ "url": s(), "search": s(), "tab": tab() }), &[]),
        tool(READ, "Read the page: text and links; mode \"elements\": links, buttons and fields by ref.", json!({ "mode": mode, "offset": { "type": "integer" }, "tab": tab() }), &[]),
        tool(FIND, "Find words on the page: matches by ref.", json!({ "query": s(), "tab": tab() }), &["query"]),
    ];
    if look {
        out.push(tool(SCROLL, "Scroll the page; amount in screens.", json!({ "direction": { "type": "string", "enum": ["down", "up", "top", "bottom"] }, "amount": { "type": "number" }, "tab": tab() }), &["direction"]));
        out.push(tool(SCROLL_TO, "Scroll a ref or text into view.", json!({ "ref": r(), "text": s(), "tab": tab() }), &[]));
        out.push(tool(SCREENSHOT, "Look at what's in view now.", json!({ "tab": tab() }), &[]));
        out.push(tool(ZOOM, "A sharper look at part of the view: top, middle (center), bottom, left, right, a corner (top-left…), or \"x,y,w,h\" fractions.", json!({ "region": s(), "tab": tab() }), &["region"]));
    }
    if act {
        // A model that sees clicks at a point too, and types where it
        // clicked; one that doesn't has a ref for every field.
        let button = || json!({ "type": "string", "enum": ["left", "right", "middle"] });
        let (click, click_props, typed) = if look {
            (
                "Click a ref, or x, y in the latest browser_screenshot's pixels; count 2 or 3: a double or triple click.",
                json!({ "ref": r(), "x": { "type": "number" }, "y": { "type": "number" }, "button": button(), "count": { "type": "integer" }, "tab": tab() }),
                &["text"][..],
            )
        } else {
            ("Click a ref; count 2 or 3: a double or triple click.", json!({ "ref": r(), "button": button(), "count": { "type": "integer" }, "tab": tab() }), &["ref", "text"][..])
        };
        out.push(tool(act::CLICK, click, click_props, &[]));
        let type_what = if look { "Type into a field (ref, else the focused one); submit: Enter after." } else { "Type into a field; submit: Enter after." };
        out.push(tool(act::TYPE, type_what, json!({ "ref": r(), "text": s(), "submit": { "type": "boolean" }, "tab": tab() }), typed));
        out.push(tool(act::KEY, "Press keys: Enter, Tab, Escape, arrows, PageDown…", json!({ "keys": s(), "tab": tab() }), &["keys"]));
        out.push(tool(act::HOVER, "Point at a ref (hover menus).", json!({ "ref": r(), "tab": tab() }), &["ref"]));
        let drag = if look { "Drag from one {ref} or {x, y} to another (a list's order, a slider, a drop zone)." } else { "Drag from one {ref} to another (a list's order, a drop zone)." };
        out.push(tool(act::DRAG, drag, json!({ "from": { "type": "object" }, "to": { "type": "object" }, "tab": tab() }), &["from", "to"]));
        out.push(tool(act::FORM_INPUT, "Set a select, checkbox (true/false) or text field.", json!({ "ref": r(), "value": s(), "tab": tab() }), &["ref", "value"]));
    }
    out.push(tool(
        TABS,
        "List, open (url or search), switch to or close tabs; width: lay a tab out that wide (1280: a desktop site), 0 the panel's.",
        json!({ "action": { "type": "string", "enum": ["list", "new", "switch", "close", "width"] }, "tab": tab(), "url": s(), "search": s(), "width": { "type": "integer" } }),
        &["action"],
    ));
    out.push(tool(WAIT, "Wait for text to appear, or the page to settle (10 s at most).", json!({ "seconds": { "type": "number" }, "text": s(), "tab": tab() }), &[]));
    if act || look {
        out.push(tool(BATCH, "Run browser tools in order, each {tool, args}; stops at the first that fails.", json!({ "actions": { "type": "array", "items": { "type": "object" } } }), &["actions"]));
    }
    Value::Array(out)
}

/// **The browser tools' line** of Chat's system prompt: general, short.
pub const INSTRUCTIONS: &str = "The browser_* tools work the tabs beside the chat, which the user watches (tab: a tab's number; else the one in front). browser_navigate opens a page when the user asks to open, show or search for something online, or a live page beats words (a map): an address from the results, a page read or the user, a site they named, or search words. To find a page or product: web_search, then browser_navigate to the best result. Websites open here, never with open_app. browser_read reads the page in front (\"this page\"): its text and links, exact and cheaper than a picture. A link they ask for: write its address (opening it isn't giving it).";
/// The line for a model that may act: a search engine's page only when the
/// person wants one (Yap can't read it, so it's no way to find something on
/// a site).
const INSTRUCTIONS_ACT: &str = "The browser_* tools work the tabs beside the chat, which the user watches (tab: a tab's number; else the one in front). browser_navigate opens a page when the user asks to open or show something on the web, or a live page beats words (a map): an address from the results, a page read or the user, or a site they named; search words only when they want a search engine's page. To find a page or product: web_search, then browser_navigate to the best result. Websites open here, never with open_app. browser_read reads the page in front (\"this page\"): its text and links, exact and cheaper than a picture. A link they ask for: write its address (opening it isn't giving it).";
/// The line's ending for a model that reads images.
const INSTRUCTIONS_LOOK: &str = " To see part of a page: browser_scroll or browser_scroll_to it, then browser_screenshot; browser_zoom for small text.";

/// The browser tools' line for a turn that offers them ("" when it doesn't),
/// with the acting tools' for a model that may act (`act`). Pure.
pub fn instructions(gate: Gate, act: bool) -> String {
    let base = if act { INSTRUCTIONS_ACT } else { INSTRUCTIONS };
    let line = match gate {
        Gate::Off => return String::new(),
        Gate::Show => base.to_string(),
        Gate::Look => format!("{base}{INSTRUCTIONS_LOOK}"),
    };
    if act {
        format!("{line} {}", act::INSTRUCTIONS)
    } else {
        line
    }
}

/// **A site the person named, at its home page** ("open YouTube" → https://
/// www.youtube.com/): `url` is a site's home page — the site itself or
/// `www.` and it, no path, no query — and the person's words have the
/// site's name ("youtube", "bbc"). Nothing else of the model's choosing: no
/// other page, no subdomain to carry words in. Pure.
pub fn named_site(question: &str, url: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(url.trim()) else { return false };
    if !matches!(u.scheme(), "https" | "http") || u.query().is_some() || u.fragment().is_some_and(|f| !f.is_empty()) || !matches!(u.path(), "" | "/") {
        return false;
    }
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let labels: Vec<&str> = host.split('.').collect();
    // The name before its ending: "youtube.com", "bbc.co.uk", "example.com.au".
    let name = match labels.as_slice() {
        [name, _] => *name,
        [name, second, tld] if SECOND.contains(second) && tld.len() == 2 => *name,
        _ => return false,
    };
    if name.len() < 2 {
        return false;
    }
    // The name said as a word, or as two or three words run together
    // ("stack overflow" → stackoverflow.com) — never inside another word.
    let said = question.to_lowercase();
    let words: Vec<&str> = said.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    (1..=3).any(|n| words.windows(n).any(|w| w.concat() == name))
}

/// A country's second level under its two-letter ending ("co.uk", "com.au").
const SECOND: &[&str] = &["co", "com", "org", "net", "ac", "gov", "edu", "ne", "or", "go"];

/// The search engines by name: any of their countries' sites ("google.de",
/// "cn.bing.com", "html.duckduckgo.com", "uk.search.yahoo.com").
const ENGINE_NAMES: &[&str] = &[
    "google", "bing", "duckduckgo", "yahoo", "yandex", "baidu", "naver", "ecosia", "startpage", "brave", "qwant", "kagi", "mojeek", "seznam", "sogou",
];

/// **A question about the page open beside the chat** ("what does this page
/// say", "fill in the form", "switch to the wiki tab"): with a tab open, a
/// model on this PC isn't made to search the web first (`turn.rs`) — the
/// page is what it's about. Pure.
pub fn about_open_page(question: &str) -> bool {
    let q = format!(" {} ", question.to_lowercase().replace(['\u{2019}', '\u{2018}'], "'"));
    let q: String = q.chars().map(|c| if c.is_alphanumeric() || c == '\'' { c } else { ' ' }).collect();
    let q = format!(" {} ", q.split_whitespace().collect::<Vec<_>>().join(" "));
    const ABOUT: &[&str] = &[
        " this page ", " the page ", " that page ", " this tab ", " the tab ", " that tab ", " other tab ", " wiki tab ", " tabs ", " this site ",
        " the site ", " on here ", " beside the chat ", " this form ", " the form ", " this article ", " the article ", " open page ",
    ];
    ABOUT.iter().any(|p| q.contains(p))
}

/// **A search engine's results page**: Yap never reads one (their terms
/// forbid automated reading); the person can. Pure.
pub fn search_page(url: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(url.trim()) else { return false };
    let host = u.host_str().unwrap_or("").to_ascii_lowercase();
    let labels: Vec<&str> = host.split('.').collect();
    // An engine's name followed only by its ending: "google.com", "google.co.uk",
    // never "google.example.com".
    let engine = labels.iter().enumerate().any(|(i, l)| {
        ENGINE_NAMES.contains(l)
            && match &labels[i + 1..] {
                [_] => true,
                [second, tld] => SECOND.contains(second) && tld.len() == 2,
                _ => false,
            }
    });
    let path = u.path();
    let asked = u.query_pairs().any(|(k, v)| matches!(k.as_ref(), "q" | "p" | "wd" | "word" | "text" | "query") && !v.trim().is_empty());
    engine && (path.contains("search") || (matches!(path, "/" | "/s" | "/html" | "/html/" | "/web") && asked))
}

/// The person's search engine ("Search in your browser"'s).
fn engine() -> String {
    #[cfg(test)]
    {
        "duckduckgo".to_string()
    }
    #[cfg(not(test))]
    {
        crate::config::load().web_search_browser
    }
}

/// **The tabs open beside the chat**, for the question's context: each
/// one's number and site, the one in front marked, a search engine's
/// results marked as the user's to read. Pure.
///
/// "Open beside the chat: tab 1 (wiki.example.com), in front; tab 2
/// (duckduckgo.com), search results. browser_read reads one."
///
/// **Never a title or a path** (3 Nov 2026): the page writes those, and this
/// line goes with every question while the tab stays open — turns the
/// safety rule treats as clean. The model reads a tab (`browser_read`, web
/// content) to learn what's on it.
pub fn tabs_line(tabs: &[OpenTab]) -> String {
    let each: Vec<String> = tabs
        .iter()
        .take(8)
        .map(|t| {
            let site = clip(&crate::web::site_of(t.url.trim()), 80);
            let mut s = if site.is_empty() { format!("tab {}", t.tab) } else { format!("tab {} ({site})", t.tab) };
            if search_page(&t.url) {
                s.push_str(", search results");
            }
            if t.front {
                s.push_str(", in front");
            }
            s
        })
        .collect();
    format!("Open beside the chat: {}. browser_read reads one.", each.join("; "))
}

/// **One turn's pages**: the ones brought up, the shots taken, the links it
/// may go to, its actions, the latest shot's frame.
#[derive(Debug, Clone, Default)]
pub struct TurnPages {
    /// Opened in new tabs this turn, in order: the answer keeps them.
    pub shown: Vec<ShownPage>,
    /// Offered this turn (an older answer's; no model is offered it now).
    pub offers: Vec<BrowserOffer>,
    /// Shots and zooms taken this turn (`see::MAX_SHOTS`).
    pub shots: usize,
    /// The links on the pages read or searched this turn (each one's
    /// address and words, `act::Link`): `browser_navigate` may open them.
    pub links: Vec<act::Link>,
    /// Actions taken this turn (`act::MAX_ACTS`).
    pub acts: usize,
    /// The latest `browser_screenshot`'s frame: `browser_click`'s x, y are
    /// in its pixels.
    pub frame: Option<see::Frame>,
    /// What Yap typed into a field and hasn't sent yet: whatever sends it
    /// is a submit (`act::rules::Typed`).
    pub typed: Option<act::rules::Typed>,
}

/// **Where a call runs**: the model's picture gate, whether it may act, its
/// provider (a shot is fitted for it), who watches the turn (the page
/// tab's strip, the cards, a shot in the steps, a tab opening), what outside
/// content the turn has seen beyond its web side's own, and the web search
/// the model has.
#[derive(Clone, Copy)]
pub struct Ctx<'a> {
    pub gate: Gate,
    pub act: bool,
    pub provider: &'a str,
    pub base_url: &'a str,
    pub live: Option<Live<'a>>,
    /// **Outside content beyond what `web::Turn` records itself** — an
    /// agent's words, the screen, a connector's result; on the plan, Claude
    /// Code's own searches and reads (`claude_live/turn.rs`'s web flag) —, or
    /// the answer before the question came from the web
    /// ([`after_web_answer`]). From then on an address the model writes
    /// itself no longer opens ([`written`]).
    pub outside: bool,
    /// The web search the model has, by the name it knows it by
    /// (`web_search`; on the plan Claude Code's `WebSearch`); `None`: none
    /// this turn. A search engine's results are never read: this finds the
    /// pages to open.
    pub search: Option<&'a str>,
}

impl Ctx<'_> {
    /// For a unit test or an evaluation's check: nobody watching, nothing
    /// from outside yet, Yap's web search.
    #[cfg(test)]
    pub fn quiet(gate: Gate, act: bool) -> Ctx<'static> {
        Ctx { gate, act, provider: "anthropic", base_url: "https://api.anthropic.com/v1", live: None, outside: false, search: Some(WEB_SEARCH) }
    }

    /// Outside content is in the turn: its web side's (a page, a search, a
    /// picture, a closed tab's title…) or any other kind ([`Ctx::outside`]).
    pub fn after_outside(&self, turn: &crate::web::Turn) -> bool {
        self.outside || turn.seen()
    }

    /// The watcher told `ev`, if anyone watches.
    fn tell(&self, ev: Event) {
        if let Some(l) = self.live {
            l.sink.send(ev);
        }
    }
}

/// What running a browser tool gave: its result for the model (a batch's:
/// each step's chip in `displayTexts`), and the pictures it looked at (a
/// shot, a zoom: the next step's message in the tool loop, MCP image
/// content on the plan), in order.
#[derive(Debug, Default)]
pub struct Run {
    pub result: Value,
    pub looks: Vec<Look>,
}

impl From<Value> for Run {
    fn from(result: Value) -> Run {
        Run { result, looks: Vec::new() }
    }
}

pub(crate) fn failed(error: String, display: String) -> Value {
    json!({ "success": false, "error": error, "data": Value::Null, "displayText": display })
}

/// `s` on one line, at most `max` characters (then "…"). Pure.
pub(crate) fn clip(s: &str, max: usize) -> String {
    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
    match one.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", one[..i].trim_end()),
        None => one,
    }
}

/// A tab number as models send it.
pub(crate) fn tab_of(v: &Value) -> Option<usize> {
    v.as_u64().map(|n| n as usize).or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

/// A ref as models send it: a number, or a number in a string ("7", "[7]").
pub(crate) fn ref_of(v: &Value) -> Option<u32> {
    match v {
        Value::Number(n) => n.as_u64().or_else(|| n.as_f64().filter(|f| f.fract() == 0.0 && *f > 0.0).map(|f| f as u64)).map(|n| n as u32),
        Value::String(s) => s.trim().trim_matches(['[', ']']).trim().parse().ok(),
        _ => None,
    }
    .filter(|n| *n > 0)
}

/// A number as models send it (`2`, `2.5`, `"2"`).
pub(crate) fn number_of(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().trim_end_matches('s').trim().parse().ok())).filter(|n: &f64| n.is_finite())
}

/// The tab a call means (`tab`, else the one in front), or the error.
pub(crate) async fn tab_for(turn: &crate::web::Turn, args: &Value, doing: &str) -> Result<OpenTab, Value> {
    let tabs = backend::tabs_soon(!turn.pages.shown.is_empty()).await;
    match view::pick_tab(&tabs, tab_of(&args["tab"])) {
        Ok(t) => Ok(t.clone()),
        Err(e) => Err(failed(format!("{e} browser_navigate opens one first, if the user wants a page."), format!("Didn't {doing}: {}", crate::web::short(&e)))),
    }
}

/// How the model finds pages to open itself, after a search opened for the
/// person (`search`: its web search, by name). Pure.
pub(crate) fn find_pages(search: Option<&str>) -> String {
    match search {
        Some(s) => format!("To find pages to open yourself, search with {s}: its results' links open with browser_navigate."),
        None => "This turn has no web search: the results are the user's to pick from.".to_string(),
    }
}

/// A search engine's results: Yap doesn't read or use them (the person can).
pub(crate) fn on_search_page(t: &OpenTab, doing: &str) -> Option<Value> {
    search_page(&t.url).then(|| {
        failed(
            "That tab is a search engine's results page: Yap doesn't read or use those (the user can). browser_navigate to a page, or ask the user which result they mean.".into(),
            format!("Didn't {doing}: a search engine's results"),
        )
    })
}

/// Yap's web search tool, by its name in the tool loop.
pub const WEB_SEARCH: &str = "web_search";

/// **An address the model wrote itself** (8 Nov 2026: Chat's own test of
/// the browser tools found a well-formed `https://www.example.com/forever`
/// refused until a search had found it): before anything from outside came
/// into the turn (`outside` false: no page, search result, connector's
/// result, agent's words, screen or text selected elsewhere, and the answer
/// before the question not from the web), any well-formed public `https://`
/// address opens — an exact address it knows, a site's page. After it, only
/// the turn's own links do ([`checked`], `act::go`): a page's words could
/// have had the model carry the person's data out in an address. Plain
/// `http://` only from those links, as before. Its address, or the refusal
/// for the model: which rule, and what works instead (`search`: the model's
/// web search, by name). Pure.
pub fn written(url: &str, outside: bool, search: Option<&str>) -> Result<String, String> {
    let url = url.trim();
    let instead = match search {
        Some(s) => format!("open a link from a page read (browser_read lists them) or find the page with {s}"),
        None => "open a link from a page read (browser_read lists them), or ask the user for the address (this turn has no web search)".to_string(),
    };
    crate::web::check_url(url)?;
    if outside {
        return Err(format!(
            "Something from outside came into this turn (a page, search results, a connector's or an agent's words), so browser_navigate now opens only a link from the user's message, this turn's search results or a page read, never an address you write (a page could have put the user's data in it). Instead, {instead}."
        ));
    }
    let parsed = reqwest::Url::parse(url).map_err(|_| "That isn't a web address.".to_string())?;
    if parsed.scheme() != "https" || parsed.host_str().is_none_or(|h| !h.contains('.')) {
        return Err(format!("An address you write opens only as https:// on a site's name; a plain http:// one only from the user, a search or a page read. Write its https:// address, or {instead}."));
    }
    Ok(url.to_string())
}

/// **The answer before this question came from the web** (a search ran
/// for it, or it has web sources): its pages' words are in what the model
/// read before this question (a plan chat's session keeps them whole), so
/// an address it writes is treated as after outside content ([`written`]),
/// as Yap Code's agents count it (`tools/agents.rs`).
pub fn after_web_answer(conversation: Option<u64>) -> bool {
    conversation.and_then(crate::chat::chats::get).is_some_and(|c| answer_before_from_web(&c.messages))
}

/// [`after_web_answer`] over a conversation's messages: the answer before
/// its latest question. Pure.
pub fn answer_before_from_web(messages: &[crate::chat::chats::ChatMessage]) -> bool {
    let q = messages.iter().rposition(|m| m.role == "user" && m.kind.is_empty()).unwrap_or(messages.len());
    messages[..q].iter().rev().find(|m| m.role == "assistant").is_some_and(crate::chat::chats::from_the_web)
}

/// **The page a call names, checked**: its address (trimmed), or the error
/// for the model and the chip — no address, not a public http(s) one, or
/// not from this turn's results, the person's messages or a page read (nor,
/// before outside content, one the model wrote: [`written`]). `doing`: the
/// chip's verb ("bring up"). Pure but for the turn it reads.
pub fn checked(turn: &crate::web::Turn, args: &Value, doing: &str, ctx: &Ctx<'_>) -> Result<String, Value> {
    let url = args["url"].as_str().unwrap_or("").trim().to_string();
    let shown = crate::web::short_url(&url);
    if url.is_empty() {
        return Err(failed(
            "url is required: a page from this turn's results or the user's message.".into(),
            format!("Couldn't {doing} a page: no address"),
        ));
    }
    // A file on this PC is never a page here: a picture goes beside the chat
    // with show_picture (chat/pictures/local.rs).
    if let Some(e) = crate::chat::pictures::local::not_a_page(&url) {
        return Err(failed(e, format!("Didn't {doing} {}: a file on this PC", crate::chat::pictures::local::chip_name(&url))));
    }
    if let Err(e) = crate::web::check_url(&url) {
        return Err(failed(e.clone(), format!("Didn't {doing} {shown}: {}", crate::web::short(&e))));
    }
    // A link on a page read this turn: the page's own address for it.
    let key = crate::web::normal_url(&url);
    if let Some(link) = turn.pages.links.iter().find(|l| crate::web::normal_url(&l.url) == key) {
        return Ok(link.url.clone());
    }
    if turn.allowed(&url) {
        return Ok(turn.stored(&url).unwrap_or(url));
    }
    written_or_refused(&url, turn, ctx, &format!("Didn't {doing} {shown}"))
}

/// [`written`] for a call, its refusal with the chip (`didnt`: "Didn't go
/// to …"). Pure but for the turn it reads.
pub(crate) fn written_or_refused(url: &str, turn: &crate::web::Turn, ctx: &Ctx<'_>, didnt: &str) -> Result<String, Value> {
    let outside = ctx.after_outside(turn);
    match written(url, outside, ctx.search) {
        Ok(u) => {
            tracing::info!("Chat: an address the model wrote opened (nothing from outside in the turn yet)");
            Ok(u)
        }
        Err(e) => {
            let why = if outside { "not a link from you, a search or the page" } else { "only https:// addresses on their own" };
            Err(failed(e, format!("{didnt}: {why}")))
        }
    }
}

/// A page's title and site: the turn's source for it (a search result, a
/// page read), else its site. Pure but for the turn it reads.
pub(crate) fn titled(turn: &crate::web::Turn, url: &str) -> (String, String, usize) {
    let key = crate::web::normal_url(url);
    let site = crate::web::site_of(url);
    match turn.sources.iter().find(|s| crate::web::normal_url(&s.url) == key) {
        Some(s) => {
            let title = if s.title.trim().is_empty() { site.clone() } else { s.title.clone() };
            let site = if s.site.trim().is_empty() { site } else { s.site.clone() };
            (title, site, s.n)
        }
        None => (site.clone(), site, 0),
    }
}

/// Slow work (decoding, encoding) off the async runtime — inline in a unit
/// test, whose chat-images folder is the test thread's own.
pub(crate) async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    #[cfg(test)]
    {
        f()
    }
    #[cfg(not(test))]
    {
        tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string()).and_then(|r| r)
    }
}

/// **Run a browser tool** (an older name as the tool it is now) where `ctx`
/// says.
pub async fn execute(turn: &mut crate::web::Turn, name: &str, args: &Value, ctx: &Ctx<'_>) -> Run {
    let (name, args) = match renamed(name, args) {
        Some((n, a)) => (n, a),
        None => (name, args.clone()),
    };
    if name == BATCH && offers(ctx.gate, ctx.act, BATCH) {
        return batch(turn, &args, ctx).await;
    }
    one(turn, name, &args, ctx).await
}

/// One tool (never a batch).
async fn one(turn: &mut crate::web::Turn, name: &str, args: &Value, ctx: &Ctx<'_>) -> Run {
    if !offers(ctx.gate, ctx.act, name) {
        return failed(format!("{name} isn't offered to this model."), format!("Unknown tool: {name}")).into();
    }
    match name {
        NAVIGATE => navigate(turn, args, ctx).await.into(),
        READ => read(turn, args, ctx).await.into(),
        FIND => see::find(turn, args).await.into(),
        SCROLL => see::scroll(turn, args).await.into(),
        SCROLL_TO => see::scroll_to(turn, args).await.into(),
        SCREENSHOT => see::screenshot(turn, args, ctx).await,
        ZOOM => see::zoom(turn, args, ctx).await,
        WAIT => see::wait(turn, args).await.into(),
        TABS => tabs(turn, args, ctx).await.into(),
        _ => act::execute(turn, name, args, ctx).await.into(),
    }
}

/// **`browser_batch`**: its steps in order, each with its own rules (a card
/// waits as usual), stopping at the first that fails — a refusal, a card
/// said no to, an error. Each step's chip, and the pictures in order.
async fn batch(turn: &mut crate::web::Turn, args: &Value, ctx: &Ctx<'_>) -> Run {
    let Some(steps) = args["actions"].as_array().filter(|a| !a.is_empty()) else {
        return failed("actions is required: a list of {tool, args}, each a browser tool.".into(), "Couldn't run the steps: none given".into()).into();
    };
    if steps.len() > MAX_BATCH {
        return failed(format!("A batch takes at most {MAX_BATCH} steps: split it."), format!("Didn't run the steps: more than {MAX_BATCH}")).into();
    }
    let mut chips: Vec<String> = Vec::new();
    let mut done: Vec<Value> = Vec::new();
    let mut looks: Vec<Look> = Vec::new();
    let mut stopped = None;
    for (i, step) in steps.iter().enumerate() {
        let name = step["tool"].as_str().or_else(|| step["name"].as_str()).unwrap_or("").trim().to_string();
        let given = if step["args"].is_object() { step["args"].clone() } else if step["input"].is_object() { step["input"].clone() } else { json!({}) };
        let (name, given) = match renamed(&name, &given) {
            Some((n, a)) => (n.to_string(), a),
            None => (name, given),
        };
        let r = if name == BATCH || !ALL.contains(&name.as_str()) {
            Run::from(failed(format!("Step {}: {name} isn't a browser tool a batch runs.", i + 1), format!("Didn't run {name} in a batch")))
        } else {
            one(turn, &name, &given, ctx).await
        };
        if let Some(chip) = r.result["displayText"].as_str().filter(|c| !c.is_empty()) {
            chips.push(chip.to_string());
        }
        looks.extend(r.looks);
        let ok = r.result["success"] == json!(true);
        let mut kept = r.result.clone();
        if let Some(o) = kept.as_object_mut() {
            o.remove("displayText");
        }
        done.push(json!({ "tool": name, "result": kept }));
        if !ok {
            stopped = Some(i + 1);
            break;
        }
    }
    tracing::info!(steps = steps.len(), ran = done.len(), stopped = stopped.is_some(), "Chat: a browser batch ran");
    let note = match stopped {
        Some(n) => format!("Stopped at step {n} of {}: it didn't go through (its result says why); the steps after it didn't run.", steps.len()),
        None => format!("All {} steps ran, in order.", steps.len()),
    };
    let result = json!({
        "success": stopped.is_none(),
        "data": { "steps": done, "ran": done.len(), "of": steps.len() },
        "note": note,
        "displayText": "",
        "displayTexts": chips,
    });
    Run { result, looks }
}

/// **`browser_navigate`**: the tab in front (or tab n) to an address or a
/// search, back or forward; with no tab open, a new one ([`open`]).
async fn navigate(turn: &mut crate::web::Turn, args: &Value, ctx: &Ctx<'_>) -> Value {
    let url = args["url"].as_str().unwrap_or("").trim().to_ascii_lowercase();
    if url == "back" || url == "forward" {
        return act::history(turn, args, url == "back", ctx.live).await;
    }
    let tabs = backend::tabs_soon(!turn.pages.shown.is_empty()).await;
    if tabs.is_empty() && tab_of(&args["tab"]).is_none() {
        return open(turn, args, ctx).await;
    }
    act::go(turn, args, ctx).await
}

/// **A page in a new tab**: an address (this turn's results, the person's
/// links, a link on a page read, the home page of a site they named:
/// [`named_site`]), or a search on the person's engine — a link whose words
/// or address would buy, unsubscribe, confirm… asked first, as its click
/// would be. The Chat view opens its Page tab (`Event::Page`); the answer
/// keeps it.
async fn open(turn: &mut crate::web::Turn, args: &Value, ctx: &Ctx<'_>) -> Value {
    let search = args["search"].as_str().map(str::trim).filter(|w| !w.is_empty()).map(|w| clip(w, 200));
    let url_given = args["url"].as_str().unwrap_or("").trim().to_string();
    if url_given.is_empty() && search.is_none() {
        return failed(
            "url or search is required: a page from this turn's results or the user's message, the home page of a site the user named, or words to search for.".into(),
            "Couldn't bring up a page: no address".into(),
        );
    }
    let (url, searched) = match search {
        // Words: the person's engine, as the address bar's (Yap never reads its results).
        Some(words) if url_given.is_empty() => (crate::web::BrowserSearch::new(&engine(), &words).url, Some(words)),
        _ if !url_given.is_empty() && crate::web::check_url(&url_given).is_ok() && named_site(turn.question(), &url_given) => {
            let u = reqwest::Url::parse(&url_given).ok();
            (u.and_then(|u| Some(format!("{}://{}/", u.scheme(), u.host_str()?))).unwrap_or(url_given), None)
        }
        _ => match checked(turn, args, "bring up", ctx) {
            Ok(u) => (u, None),
            Err(result) => return result,
        },
    };
    let shown = searched.as_ref().map_or_else(|| crate::web::short_url(&url), |w| format!("a search for \"{w}\""));
    let key = crate::web::normal_url(&url);
    let note = if searched.is_some() {
        format!("The search is open beside the chat for the user, on their own search engine. Yap doesn't read search engines' result pages: say it's open, and the user picks a result. {}", find_pages(ctx.search))
    } else {
        "It's open beside the chat for the user to see and use. browser_read reads it if you need what it says; don't describe it beyond what you've read.".to_string()
    };
    let ok = |display: String| json!({ "success": true, "shown": true, "note": note, "displayText": display });
    // Up already this turn: the one there is.
    if turn.pages.shown.iter().any(|p| crate::web::normal_url(&p.url) == key) {
        return ok(String::new());
    }
    if turn.pages.shown.len() >= MAX_PAGES {
        return failed(format!("You've opened {MAX_PAGES} pages in new tabs this turn: use the tabs open, or answer the user now."), format!("Didn't bring up {shown}: {MAX_PAGES} pages a turn"));
    }
    // A link that would buy, pay, unsubscribe, confirm…: the person's first
    // (never one they gave in their own message: they asked for that page).
    if searched.is_none() && !turn.linked_by_person(&url) {
        let link = turn.pages.links.iter().find(|l| crate::web::normal_url(&l.url) == key).cloned();
        let label = link.as_ref().map(|l| l.label.clone()).filter(|l| !l.is_empty()).unwrap_or_else(|| crate::web::short_url(&url));
        if let Some(kind) = act::rules::needs_ok(link.as_ref().map_or("", |l| l.label.as_str()), &url) {
            let site = crate::web::site_of(&url);
            let answer = act::confirm::ask(ctx.live, act::confirm::Ask { act: "open", label: &label, site: &site, does: kind, text: "", key: "", agent: None }).await;
            if let Some(r) = act::refusal(answer, &format!("opening ‘{label}’"), &format!("open ‘{label}’")) {
                return r;
            }
        }
    }
    let (title, site, _) = match &searched {
        Some(words) => (format!("Search: {words}"), crate::web::site_of(&url), 0),
        None => titled(turn, &url),
    };
    let page = ShownPage { url, title, site, section: String::new() };
    tracing::info!(pages = turn.pages.shown.len() + 1, "Chat: a page brought up beside the chat");
    turn.pages.shown.push(page.clone());
    let mut result = ok(format!("Brought up {shown} beside the chat"));
    // (The stand-in browser of an evaluation opens it; the panel's is made
    // out of sight now, then shown by the Chat view.)
    act::closed_note(turn, &mut result, backend::opened(&page.url, "").await);
    ctx.tell(Event::Page { page });
    result
}

/// **`browser_read`**: what the page in front beside the chat (or tab
/// `tab`) says — its title, headings and text, cut at [`READ_MAX`], and its
/// links — and the tabs open. Web content (the safety rule), and a source
/// of the turn. Mode "elements": its links, buttons and fields by ref
/// (`act::rules::element_lines`), its text shorter ([`act::ELEMENTS_TEXT`]);
/// mode "tables": its tables as rows ([`read::tables`]). One that stops
/// short says where to go on (`next`); `offset` goes on from there
/// ([`read::text_from`], [`read::elements_from`]) — a read without it is
/// what it always was, `next` apart.
async fn read(turn: &mut crate::web::Turn, args: &Value, ctx: &Ctx<'_>) -> Value {
    let act = ctx.act;
    let tabs = backend::tabs_soon(!turn.pages.shown.is_empty()).await;
    let mode = args["mode"].as_str().unwrap_or("").trim().to_ascii_lowercase();
    let elements = mode == "elements" || args["elements"].as_bool() == Some(true);
    let offset = read::offset_of(&args["offset"]);
    let tab = match view::pick_tab(&tabs, tab_of(&args["tab"])) {
        Ok(t) => t.clone(),
        Err(e) => return failed(format!("{e} browser_navigate opens one first, if the user wants a page."), format!("Didn't read a page: {}", crate::web::short(&e))),
    };
    let shown = crate::web::short_url(&tab.url);
    if search_page(&tab.url) {
        return failed(read::search_refusal(ctx.search), format!("Didn't read {shown}: a search engine's results"));
    }
    if mode == "tables" {
        return read::tables(turn, &tab, offset.unwrap_or(0)).await;
    }
    match offset {
        Some(at) if elements => return read::elements_from(turn, &tab, at, act).await,
        Some(at) => return read::text_from(turn, &tab, at, act).await,
        None => {}
    }
    // With its elements: one read for both (the page's words cut shorter).
    let snap = if elements {
        match backend::snapshot(&tab.key, READ_MAX + 1, act::rules::MAX_ELEMENTS).await {
            Ok(s) => Some(s),
            Err(e) => return failed(e.clone(), format!("Couldn't read {shown}: {}", crate::web::short(&e))),
        }
    } else {
        None
    };
    let text = match &snap {
        Some(s) => view::PageText { title: s.title.clone(), headings: s.headings.clone(), text: s.text.clone(), links: Vec::new(), frames: Vec::new() },
        None => match backend::text(&tab.key).await {
            Ok(t) => t,
            Err(e) => return failed(e.clone(), format!("Couldn't read {shown}: {}", crate::web::short(&e))),
        },
    };
    let title = if text.title.trim().is_empty() { tab.title.clone() } else { text.title.clone() };
    let already = read::known_source(turn, &tab.url);
    let n = turn.read_open(&title, &tab.url, &text.text);
    let cap = if snap.is_some() { act::ELEMENTS_TEXT } else { READ_MAX };
    let more = text.text.chars().count() > cap;
    let body: String = text.text.chars().take(cap).collect();
    tracing::info!(tab = tab.tab, tabs = tabs.len(), chars = body.chars().count(), more, elements = snap.as_ref().map_or(0, |s| s.elements.len()), "Chat: the page beside the chat read");
    let listed: Vec<Value> = tabs.iter().map(|t| json!({ "tab": t.tab, "title": clip(&t.title, 80), "url": t.url, "front": t.front })).collect();
    let mut web = read::web_note(n, &tab, already);
    if act && snap.is_none() {
        // A plain read has no refs: the way to them, for a model that acts.
        web.push_str(" To act on it: browser_read mode \"elements\" gives refs.");
    }
    let mut result = json!({
        "success": true,
        "data": {
            "tab": tab.tab,
            "n": n,
            "title": title,
            "url": tab.url,
            "headings": text.headings.iter().take(20).map(|h| clip(h, 100)).collect::<Vec<_>>(),
            "text": body,
            "more": more,
            "tabs": listed,
        },
        "note": web,
        "displayText": format!("Read {shown} beside the chat"),
    });
    // Where to go on (the arguments for the next read): its elements past
    // the first ones, else its text past what's here.
    let next = match &snap {
        Some(s) if s.more => Some(json!({ "mode": "elements", "offset": act::rules::MAX_ELEMENTS })),
        _ if more => Some(json!({ "offset": cap })),
        _ => None,
    };
    if let Some(next) = next {
        result["data"]["next"] = next;
    }
    // Its links (a plain read's): their words and addresses, for a link the
    // user asks for ("send me the link to that one") or browser_navigate —
    // the page's own, never a search engine's (the person reads those).
    if snap.is_none() {
        let links: Vec<(String, String)> = text
            .links
            .iter()
            .map(|l| (view::shown(&l.href), clip(&l.text, 80)))
            .filter(|(url, _)| crate::web::check_url(url).is_ok() && !search_page(url))
            .take(LINKS_MAX)
            .collect();
        if !links.is_empty() {
            act::remember_page_links(&mut turn.pages.links, &links);
            result["data"]["links"] = json!(links.iter().map(|(url, words)| json!({ "text": words, "url": url })).collect::<Vec<_>>());
            result["note"] = json!(format!("{web} Asked for a link: write its url from links in your answer, exactly as given (opening it isn't giving it); browser_navigate opens one."));
        }
    }
    // Frames no script outside reaches (another site's, or sandboxed): said
    // as they are — a same-origin frame's words are in the text already.
    if !text.frames.is_empty() {
        let n = text.frames.len();
        let sites: Vec<String> = text.frames.iter().map(|f| crate::web::site_of(&f.src)).filter(|s| !s.is_empty()).collect();
        result["data"]["frames"] = json!(text.frames.iter().map(|f| json!({ "site": crate::web::site_of(&f.src), "sandboxed": f.sandboxed, "title": clip(&f.title, 60) })).collect::<Vec<_>>());
        let what = if n == 1 { "a frame".to_string() } else { format!("{n} frames") };
        let from = if sites.is_empty() { String::new() } else { format!(" ({})", sites.join(", ")) };
        let said = format!(
            "{} It has {what} Yap can't read inside{from}: from another site, or sandboxed, so no script outside reaches in; a screenshot shows {}.",
            result["note"].as_str().unwrap_or(""),
            if n == 1 { "it" } else { "them" }
        );
        result["note"] = json!(said);
    }
    if let Some(s) = &snap {
        // Its links: addresses browser_navigate may open this turn.
        act::remember_links(&mut turn.pages.links, s);
        result["data"]["elements"] = json!(act::rules::element_lines(s));
        let how = if act {
            "Act on it by ref: browser_click, browser_type (submit: true presses Enter), browser_form_input; refs hold until the page changes."
        } else {
            "browser_scroll_to takes a ref; refs hold until the page changes."
        };
        result["note"] = json!(format!("{web} {how}"));
    }
    result
}

/// **`browser_tabs`**: the tabs beside the chat listed (each one's number,
/// title, address, the one in front: their titles are the pages' words,
/// web content), a new one ([`open`]), one shown (the Chat view shows its
/// tab: `Event::SwitchTab`) or closed (`Event::CloseTab`).
async fn tabs(turn: &mut crate::web::Turn, args: &Value, ctx: &Ctx<'_>) -> Value {
    let action = args["action"].as_str().unwrap_or("list").trim().to_ascii_lowercase();
    match action.as_str() {
        "new" | "open" => {
            // A new tab even with others open.
            if args["url"].as_str().unwrap_or("").trim().is_empty() && args["search"].as_str().unwrap_or("").trim().is_empty() {
                return failed("A new tab needs url or search: an address from the results, the user or a page read, a site they named, or words to search for.".into(), "Couldn't open a tab: no address".into());
            }
            open(turn, args, ctx).await
        }
        "switch" | "show" | "close" => {
            let doing = if action == "close" { "close a tab" } else { "switch tabs" };
            let Some(n) = tab_of(&args["tab"]) else {
                return failed("tab is required: a tab's number (every question says which are open).".into(), format!("Couldn't {doing}: no tab given"));
            };
            let open_tabs = backend::tabs_soon(!turn.pages.shown.is_empty()).await;
            let t = match view::pick_tab(&open_tabs, Some(n)) {
                Ok(t) => t.clone(),
                Err(e) => return failed(e.clone(), format!("Couldn't {doing}: {}", crate::web::short(&e))),
            };
            if action == "close" {
                if let Err(e) = backend::close(&t.key).await {
                    return failed(e.clone(), format!("Couldn't close tab {n}: {}", crate::web::short(&e)));
                }
                ctx.tell(Event::CloseTab { key: t.key.clone() });
                tracing::info!(tab = n, "Chat: a tab beside the chat closed");
                return json!({ "success": true, "data": { "tab": n, "closed": true }, "displayText": format!("Closed tab {n} beside the chat") });
            }
            if let Err(e) = backend::front(&t.key).await {
                return failed(e.clone(), format!("Couldn't switch tabs: {}", crate::web::short(&e)));
            }
            ctx.tell(Event::SwitchTab { key: t.key.clone() });
            // Its title and address are the page's: web content (the safety rule).
            turn.mark_seen();
            json!({
                "success": true,
                "data": { "tab": t.tab, "title": clip(&t.title, 120), "url": t.url },
                "note": "That tab is in front beside the chat now: browser_read reads it.",
                "displayText": format!("Showed tab {} beside the chat", t.tab),
            })
        }
        "width" => width(turn, args).await,
        _ => {
            let open_tabs = backend::tabs().await;
            if !open_tabs.is_empty() {
                // Their titles are the pages' words.
                turn.mark_seen();
            }
            let listed: Vec<Value> = open_tabs
                .iter()
                .map(|t| {
                    let mut x = json!({ "tab": t.tab, "title": clip(&t.title, 80), "url": t.url, "front": t.front });
                    if let Some(w) = t.width {
                        x["width"] = json!(w);
                    }
                    x
                })
                .collect();
            let note = if listed.is_empty() { "No tab is open beside the chat: browser_navigate opens one." } else { "Titles are the pages' own words: web content." };
            json!({ "success": true, "data": { "tabs": listed }, "note": note, "displayText": format!("Listed the tabs beside the chat ({})", open_tabs.len()) })
        }
    }
}

/// **`browser_tabs` width**: tab `tab` (else the one in front) laid out
/// `width` CSS pixels wide — a site's desktop layout where the panel's width
/// shows its phone one — and shown to the person scaled to fit the panel,
/// so they see the layout the model reads and clicks in, smaller; its tab
/// says "Desktop width" with a × that puts it back (`view::set_width`). 0:
/// the panel's own width. A screenshot taken before no longer maps to the
/// page (`see::Frame::moved`: a click at its point is refused until the
/// next one).
async fn width(turn: &mut crate::web::Turn, args: &Value) -> Value {
    let width = match see::width_of(&args["width"]) {
        Ok(w) => w,
        Err(e) => return failed(e, "Couldn't set the tab's width: not a width".into()),
    };
    let t = match tab_for(turn, args, "set the tab's width").await {
        Ok(t) => t,
        Err(e) => return e,
    };
    if let Some(r) = on_search_page(&t, "set the tab's width") {
        return r;
    }
    let v = match backend::width(&t.key, width).await {
        Ok(v) => v,
        Err(e) => return failed(e.clone(), format!("Couldn't set tab {}'s width: {}", t.tab, crate::web::short(&e))),
    };
    // The headings now in view are the page's words.
    turn.mark_seen();
    let laid = v.scroll.w;
    tracing::info!(tab = t.tab, desktop = width.is_some(), laid, "Chat: a tab beside the chat laid out at a width");
    let (note, display) = match width {
        Some(w) => {
            let short = if laid > 0 && (laid as f64) < f64::from(w) - 1.0 { format!(" (as wide as the panel allows: {laid} px)") } else { String::new() };
            (
                format!("Tab {} is laid out {w} CSS px wide{short}, as on a desktop, and the user sees it scaled to fit the panel. browser_read and browser_screenshot read it at that width; a screenshot from before no longer matches it (take another before clicking at a point). 0 puts it back.", t.tab),
                format!("Set tab {} to a desktop width beside the chat", t.tab),
            )
        }
        None => (format!("Tab {} is laid out at the panel's own width again ({laid} px).", t.tab), format!("Set tab {} back to the panel's width", t.tab)),
    };
    json!({
        "success": true,
        "data": { "tab": t.tab, "width": width, "laid_out": laid, "scroll": { "y": v.scroll.y, "h": v.scroll.h, "max": v.scroll.max }, "in_view": v.in_view },
        "note": note,
        "displayText": display,
    })
}

#[cfg(test)]
mod tests;

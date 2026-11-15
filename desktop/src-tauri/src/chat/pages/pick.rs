//! **Select an element, or annotate** (14 Nov 2026; the maintainer, showing
//! the Claude Code app's browser pane: its "Select an element or annotate"
//! button, Ctrl+Shift+S, "Sends Claude a screenshot plus the element's
//! HTML, styles, selector…"). Built from what the app shows and its public
//! docs, never from its files (Anthropic's terms forbid taking it apart).
//!
//! - **Select** ([`page_pick`]): DevTools' own inspect mode on the page
//!   (`Overlay.setInspectMode` "searchForNode"): what's under the pointer is
//!   highlighted with its tag, classes and size, as the browser's developer
//!   tools pick an element; a click picks it (`Overlay.inspectNodeRequested`,
//!   heard by `view.rs` `watch_devtools` → [`requested`]) — one pick, then
//!   the page clicks as ever. [`picked`] reads it in the page
//!   ([`DESCRIBE`]: tag, id, classes, its words, where it is — its
//!   ancestors —, key computed styles, its HTML and its neighbours', all
//!   clipped; on a React page its component, owners, props and source too:
//!   [`ReactPart`]), crops its picture from a shot of the view, puts the picture in
//!   Chat's box (`snip/waiting.rs`, source "element": its chip shows it, not
//!   a tile), and tells the page tab ([`EVENT`]): the message box's chip.
//! - **Annotate** ([`page_still`], [`page_annotated`]): a still of the page
//!   as it's in view, drawn on in Yap's own page (a pen, shapes, words:
//!   `PageTab.svelte`), the drawing laid on it there and the picture put in
//!   Chat's box (source "annotation").
//! - **Sent** ([`block`]): each element as a `<selected-element>` block
//!   after the person's words — Claude reads its tag, words, path, its
//!   `<react component source build>` part (owners or what it's inside, its
//!   props), styles and HTML; its picture goes as the message's picture. Web content: a
//!   message carrying one is a web turn from its start ([`has_element`]: the
//!   safety rule), as text selected on a page is.
//!
//! **Privacy**: the log has counts and sizes, never an element's words, its
//! HTML or a page's address.

use std::collections::BTreeMap;

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

/// The event the page tab hears a pick on.
pub const EVENT: &str = "yap-page-picked";
/// An element's words, HTML and neighbours' HTML, at most.
const TEXT_MAX: usize = 2_000;
const HTML_MAX: usize = 3_000;
const SIBLINGS_MAX: usize = 3_000;

/// **What a pick reads in the page** (`Runtime.callFunctionOn` on the
/// element): its tag, id, classes, words, ancestors as a path, key computed
/// styles, its HTML and its neighbours' (the picked one marked), where it is
/// in the view, and the view's width (the shot's scale).
pub const DESCRIBE: &str = r#"function(){
var el=this,cs=getComputedStyle(el),r=el.getBoundingClientRect();
function sel(e){var s=e.tagName.toLowerCase();if(e.id)s+='#'+e.id;else if(e.classList&&e.classList.length)s+='.'+[].slice.call(e.classList,0,3).join('.');return s;}
var path=[],p=el.parentElement;while(p&&p!==document.documentElement&&path.length<8){path.unshift(sel(p));if(p===document.body)break;p=p.parentElement;}
var keys=['display','position','flex-direction','justify-content','align-items','width','height','padding','margin','background-color','color','font-size','font-weight','border-radius','border-width','border-style','border-color'];
var styles={};keys.forEach(function(k){styles[k]=cs.getPropertyValue(k);});
function clip(h,n){return h.length>n?h.slice(0,n)+'…':h;}
function reactOf(el){
var key=null,host=el,up=0;
for(;host&&host.nodeType===1&&up<4;up++){var ks=[];try{ks=Object.keys(host);}catch(e){}for(var i=0;i<ks.length;i++){if(ks[i].indexOf('__reactFiber$')===0||ks[i].indexOf('__reactInternalInstance$')===0){key=ks[i];break;}}if(key)break;host=host.parentElement;}
if(!key)return null;
var f=host[key];if(!f||typeof f!=='object')return null;
function nm(t){if(!t)return '';if(typeof t==='string')return t;if(typeof t==='function')return String(t.displayName||t.name||'');if(typeof t==='object'){if(typeof t.displayName==='string'&&t.displayName)return t.displayName;if(t.render)return nm(t.render);if(t.type)return nm(t.type);}return '';}
var COMP={0:1,1:1,2:1,11:1,14:1,15:1};
var c=f;for(var n=0;c&&n<80;n++){if(COMP[c.tag]&&nm(c.type))break;c=c.return;}
if(c&&!COMP[c.tag])c=null;
var dev=('_debugOwner' in f)||('_debugSource' in f)||('_debugStack' in f);
var owners=[],inside=false;
if(dev&&c){var o=c._debugOwner;for(var m=0;o&&owners.length<5&&m<20;m++){var on=(o.tag!==undefined)?nm(o.type):String(o.name||'');if(on)owners.push(on+((o.tag===undefined&&o.env)?' ('+o.env+')':''));o=o._debugOwner||o.owner||null;}}
if(!owners.length&&c){inside=true;var p=c.return;for(var q=0;p&&owners.length<5&&q<300;q++){if(COMP[p.tag]){var pn=nm(p.type);if(pn&&owners[owners.length-1]!==pn)owners.push(pn);}p=p.return;}}
function val(v){var t=typeof v;if(v===null)return 'null';if(t==='undefined')return 'undefined';if(t==='string')return JSON.stringify(v.length>80?v.slice(0,80)+'…':v);if(t==='number'||t==='boolean'||t==='bigint')return String(v);if(t==='function')return 'ƒ '+(v.displayName||v.name||'anonymous');if(t==='symbol')return String(v);
if(Array.isArray(v))return '['+v.length+(v.length===1?' item]':' items]');
try{if(v.$$typeof){var isEl=v.$$typeof===Symbol.for('react.element')||v.$$typeof===Symbol.for('react.transitional.element');return isEl?'<'+(nm(v.type)||'element')+' />':(nm(v)||'a component');}}catch(e){return '{…}';}
if(typeof Element!=='undefined'&&v instanceof Element)return '<'+v.tagName.toLowerCase()+'>';
if(v instanceof Date)return isNaN(v)?'Invalid Date':v.toISOString();
var ks2=[];try{ks2=Object.keys(v);}catch(e){return '{…}';}return '{'+ks2.slice(0,6).join(', ')+(ks2.length>6?', …':'')+'}';}
var props=[],mp=c?c.memoizedProps:null;
if(mp&&typeof mp==='object'){var pk=[];try{pk=Object.keys(mp);}catch(e){}for(var r=0;r<pk.length&&props.length<20;r++){var k=pk[r],d=Object.getOwnPropertyDescriptor(mp,k),v2;if(!d)continue;if(!('value' in d))v2='(a getter)';else if(k==='children'&&Array.isArray(d.value))v2='['+d.value.length+' children]';else v2=val(d.value);props.push(k+': '+v2);}}
function fromStack(s){var ls=String(s).split('\n');for(var i=0;i<ls.length;i++){var l=ls[i],mm=/((?:[a-z][a-z0-9+.-]*:\/\/)[^\s()]+):(\d+):(\d+)\)?\s*$/i.exec(l);if(!mm)continue;if(/react-stack-top-frame|node_modules|\/\.vite\/deps\/|react-jsx(-dev)?-runtime|react(-dom)?(-client)?\.(development|production)|\/react-dom\/|\/react\/|\bscheduler\b/i.test(mm[1]+' '+l))continue;return mm[1]+':'+mm[2]+':'+mm[3];}return '';}
var src='',from='';var ds=f._debugSource||(c&&c._debugSource);
if(ds&&ds.fileName){src=String(ds.fileName)+':'+ds.lineNumber+(ds.columnNumber?':'+ds.columnNumber:'');from='jsx';}
else{var st=f._debugStack,txt='';try{txt=(st&&typeof st==='object'&&typeof st.stack==='string')?st.stack:(typeof st==='string'?st:'');}catch(e){}if(txt){src=fromStack(txt);if(src)from='stack';}}
return {component:c?nm(c.type):'',owners:owners,inside:inside,props:props,source:src,sourceFrom:from,build:dev?'development':'production',up:up};}
var html=el.outerHTML||'',sib='';
if(el.parentElement){sib=[].slice.call(el.parentElement.children,0,12).map(function(c){return (c===el?'<!-- SELECTED -->':'')+clip(c.outerHTML||'',300);}).join('\n');}
return {tag:el.tagName.toLowerCase(),id:el.id||'',classes:[].slice.call(el.classList||[],0,8),text:(el.innerText||el.textContent||'').replace(/\n{3,}/g,'\n\n').trim().slice(0,2000),path:path.join(' > '),styles:styles,html:clip(html,3000),siblings:clip(sib,3000),rect:{x:r.left,y:r.top,w:r.width,h:r.height},vw:innerWidth,url:location.href,title:document.title,react:(function(){try{return reactOf(el);}catch(e){return null;}})()};
}"#;

/// **An element picked**, as the message box keeps it and `chat_send` sends it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Picked {
    pub tag: String,
    pub id: String,
    pub classes: Vec<String>,
    pub text: String,
    pub path: String,
    pub styles: BTreeMap<String, String>,
    pub html: String,
    pub siblings: String,
    pub url: String,
    pub title: String,
    /// Its picture in Chat's box (`snip/images.rs`), if one was cropped.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// **Its React details**, on a page made with React ([`ReactPart`]); none elsewhere.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub react: Option<ReactPart>,
}

/// **What React knows of a picked element** (15 Nov 2026), read from its
/// internals on the DOM node in the page ([`DESCRIBE`]'s `reactOf`: the
/// fiber React keeps on it — `__reactFiber$…`, `__reactInternalInstance$…`
/// before React 17 — read only, never changing the page): the component it's
/// inside, that component's owners (who rendered it, a few levels; a
/// production build keeps none, so the components it's inside instead,
/// [`ReactPart::inside`]), its props (trimmed, a function as `ƒ name`, an
/// element as `<Name />`), and where its JSX was written when the page
/// carries a development build's source info — React 18's `_debugSource`
/// (the file on disk), React 19's `_debugStack` (the file and line as the
/// page serves it).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ReactPart {
    pub component: String,
    pub owners: Vec<String>,
    /// [`ReactPart::owners`] are the components it's inside, not who rendered it.
    pub inside: bool,
    /// "name: value", each.
    pub props: Vec<String>,
    /// "src/Basket.jsx:30:7"; empty: the page carries none.
    pub source: String,
    /// "jsx" (React 18's own) or "stack" (the line as the page serves it).
    pub source_from: String,
    /// "development" or "production".
    pub build: String,
}

impl ReactPart {
    /// Trimmed to what a message carries. Pure.
    pub fn trimmed(mut self) -> ReactPart {
        self.component = clip(&self.component, 80);
        self.owners = self.owners.into_iter().take(5).map(|o| clip(&o, 80)).collect();
        self.props = self.props.into_iter().take(20).map(|p| clip(&p, 160)).collect();
        self.source = clip(&self.source, 300);
        self
    }

    /// Its source as said: "src/Basket.js:12:20 (as the page serves it)" for a stack's line. Pure.
    pub fn source_words(&self) -> String {
        if self.source.is_empty() {
            String::new()
        } else if self.source_from == "stack" {
            format!("{} (as the page serves it)", self.source)
        } else {
            self.source.clone()
        }
    }
}


fn clip(s: &str, max: usize) -> String {
    if s.chars().count() > max {
        format!("{}…", s.chars().take(max).collect::<String>())
    } else {
        s.to_string()
    }
}

/// An attribute's value as XML has it. Pure.
fn attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;")
}

/// **An element as the model reads it**, after the person's words: its page,
/// tag, id, classes and picture in its opening tag; its words, path, key
/// styles, HTML and neighbours inside. Pure.
pub fn block(el: &Picked) -> String {
    let mut open = format!("<selected-element page=\"{}\" tag=\"{}\"", attr(&el.url), attr(&el.tag));
    if !el.id.is_empty() {
        open.push_str(&format!(" id=\"{}\"", attr(&el.id)));
    }
    if !el.classes.is_empty() {
        open.push_str(&format!(" class=\"{}\"", attr(&el.classes.join(" "))));
    }
    if el.image.is_some() {
        open.push_str(" picture=\"attached\"");
    }
    open.push('>');
    let styles = el.styles.iter().filter(|(_, v)| !v.trim().is_empty()).map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join("; ");
    let mut parts = vec![open];
    if !el.text.trim().is_empty() {
        parts.push(format!("<text>{}</text>", clip(el.text.trim(), TEXT_MAX)));
    }
    if !el.path.is_empty() {
        parts.push(format!("<path>{}</path>", clip(&el.path, 400)));
    }
    if let Some(r) = &el.react {
        let mut open = "<react".to_string();
        if !r.component.is_empty() {
            open.push_str(&format!(" component=\"{}\"", attr(&r.component)));
        }
        if !r.source.is_empty() {
            open.push_str(&format!(" source=\"{}\"", attr(&r.source_words())));
        }
        if !r.build.is_empty() {
            open.push_str(&format!(" build=\"{}\"", attr(&r.build)));
        }
        open.push('>');
        let mut inner = vec![open];
        if !r.owners.is_empty() {
            let tag = if r.inside { "inside" } else { "owners" };
            inner.push(format!("<{tag}>{}</{tag}>", r.owners.join(" ‹ ")));
        }
        if !r.props.is_empty() {
            inner.push(format!("<props>{}</props>", r.props.join("\n")));
        }
        inner.push("</react>".into());
        parts.push(inner.join("\n"));
    }
    if !styles.is_empty() {
        parts.push(format!("<styles>{styles}</styles>"));
    }
    if !el.html.is_empty() {
        parts.push(format!("<html>{}</html>", clip(&el.html, HTML_MAX)));
    }
    if !el.siblings.is_empty() {
        parts.push(format!("<siblings>{}</siblings>", clip(&el.siblings, SIBLINGS_MAX)));
    }
    parts.push("</selected-element>".into());
    parts.join("\n")
}

/// **A message's words with its picked elements after them** (none: as it is). Pure.
pub fn with_elements(text: &str, elements: &[Picked]) -> String {
    if elements.is_empty() {
        return text.to_string();
    }
    let blocks = elements.iter().map(block).collect::<Vec<_>>().join("\n\n");
    format!("{}\n\n{blocks}", text.trim_end())
}

/// Whether a message carries a picked element (web content: the safety rule). Pure.
pub fn has_element(text: &str) -> bool {
    text.contains("<selected-element ")
}

/// Pages in select mode now, by key: a pick on another page (or none asked) isn't one.
static PICKING: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// **Select mode on page `key`** (`on`), or off: DevTools' inspect mode, its
/// highlight showing the element under the pointer with its tag and size.
#[tauri::command]
pub async fn page_pick(key: String, on: bool) -> Result<(), String> {
    if on {
        let _ = super::backend::devtools_call(&key, "DOM.enable", &json!({})).await;
        super::backend::devtools_call(&key, "Overlay.enable", &json!({})).await?;
        let highlight = json!({
            "showInfo": true,
            "showStyles": false,
            "showAccessibilityInfo": false,
            "contentColor": { "r": 111, "g": 168, "b": 220, "a": 0.35 },
            "paddingColor": { "r": 147, "g": 196, "b": 125, "a": 0.35 },
            "borderColor": { "r": 255, "g": 160, "b": 60, "a": 0.9 },
            "marginColor": { "r": 246, "g": 178, "b": 107, "a": 0.25 },
        });
        super::backend::devtools_call(&key, "Overlay.setInspectMode", &json!({ "mode": "searchForNode", "highlightConfig": highlight })).await?;
        let mut p = PICKING.lock().unwrap_or_else(|e| e.into_inner());
        if !p.contains(&key) {
            p.push(key);
        }
        tracing::info!("Chat: select an element on a page beside the chat");
    } else {
        PICKING.lock().unwrap_or_else(|e| e.into_inner()).retain(|k| k != &key);
        let _ = super::backend::devtools_call(&key, "Overlay.setInspectMode", &json!({ "mode": "none", "highlightConfig": {} })).await;
        let _ = super::backend::devtools_call(&key, "Overlay.hideHighlight", &json!({})).await;
    }
    Ok(())
}

/// **DevTools says the person picked a node** on page `key` (its event's
/// parameters as JSON; `view.rs` `watch_devtools`, on the window's thread):
/// read it, off the thread.
pub fn requested(key: &str, params: &str) {
    let picking = PICKING.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|k| k == key);
    if !picking {
        return;
    }
    let Some(node) = serde_json::from_str::<Value>(params).ok().and_then(|v| v["backendNodeId"].as_u64()) else { return };
    let key = key.to_string();
    tauri::async_runtime::spawn(async move {
        let out = picked(&key, node).await;
        let Some(app) = crate::memory::app() else { return };
        let payload = match out {
            Ok(el) => json!({ "key": key, "element": el }),
            Err(e) => json!({ "key": key, "error": e }),
        };
        let _ = app.emit(EVENT, payload);
    });
}

/// **Node `node` picked on page `key`**: select mode off (one pick), the
/// element read in the page, its picture cropped from the view and put in
/// Chat's box.
async fn picked(key: &str, node: u64) -> Result<Picked, String> {
    let _ = page_pick(key.to_string(), false).await;
    let resolved: Value = serde_json::from_str(&super::backend::devtools_call(key, "DOM.resolveNode", &json!({ "backendNodeId": node })).await?).map_err(|e| e.to_string())?;
    let object = resolved["object"]["objectId"].as_str().ok_or("That element is gone from the page.")?.to_string();
    let call = json!({ "objectId": object, "functionDeclaration": DESCRIBE, "returnByValue": true });
    let out: Value = serde_json::from_str(&super::backend::devtools_call(key, "Runtime.callFunctionOn", &call).await?).map_err(|e| e.to_string())?;
    let v = out["result"]["value"].clone();
    if v.is_null() {
        return Err("Yap couldn't read that element.".into());
    }
    let mut el: Picked = serde_json::from_value(v.clone()).unwrap_or_default();
    el.text = clip(&el.text, TEXT_MAX);
    // Addresses as the page's own (a test run's stand-in turned back), a stack's file short.
    el.url = super::view::shown(&el.url);
    if let Some(r) = el.react.as_mut().filter(|r| r.source_from == "stack") {
        r.source = served_path(&super::view::shown(&r.source), &el.url);
    }
    // (A page without React: none, as before.)
    el.react = el.react.take().map(ReactPart::trimmed).filter(|r| !r.component.is_empty() || !r.source.is_empty());
    // Its picture: the view's shot, cropped to it (what shows of it).
    let rect = (v["rect"]["x"].as_f64().unwrap_or(0.0), v["rect"]["y"].as_f64().unwrap_or(0.0), v["rect"]["w"].as_f64().unwrap_or(0.0), v["rect"]["h"].as_f64().unwrap_or(0.0));
    let vw = v["vw"].as_f64().unwrap_or(0.0);
    if let Ok(png) = super::view::shot_in_view(key).await {
        let cropped = tauri::async_runtime::spawn_blocking(move || crop_to(&png, rect, vw)).await.ok().flatten();
        if let Some(png) = cropped {
            if let Ok(image) = tauri::async_runtime::spawn_blocking(move || crate::snip::attach::keep(&png, "element")).await.map_err(|e| e.to_string()).and_then(|r| r) {
                el.image = Some(image.id.clone());
                if let Some(app) = crate::memory::app() {
                    crate::snip::waiting::added(app, vec![image]);
                }
            }
        }
    }
    tracing::info!(chars = el.text.chars().count(), html = el.html.len(), picture = el.image.is_some(), react = el.react.is_some(), "Chat: an element picked on a page beside the chat");
    Ok(el)
}

/// **A stack frame's file, short** (React 19's `_debugStack`): on its page's
/// own origin, the path alone — `https://shop.example.com/src/Basket.js?t=1:11:5`
/// on `https://shop.example.com/basket` is `src/Basket.js:11:5` (a query and
/// Vite's `/@fs` off); another site's or a bundler's (`webpack-internal:///…`)
/// as it is, its query off. Pure.
pub fn served_path(source: &str, page: &str) -> String {
    let (file, at) = match source.rsplitn(3, ':').collect::<Vec<_>>()[..] {
        [c, l, f] if c.parse::<u32>().is_ok() && l.parse::<u32>().is_ok() => (f, format!(":{l}:{c}")),
        _ => (source, String::new()),
    };
    let bare = file.split(['?', '#']).next().unwrap_or(file);
    let Ok(u) = url::Url::parse(file) else { return format!("{bare}{at}") };
    if !matches!(u.scheme(), "http" | "https") {
        return format!("{bare}{at}");
    }
    let path = u.path();
    let path = path.strip_prefix("/@fs").filter(|p| p.starts_with('/')).unwrap_or(path);
    if url::Url::parse(page).is_ok_and(|p| p.origin() == u.origin()) {
        format!("{}{at}", path.trim_start_matches('/'))
    } else {
        format!("{}{path}{at}", u.origin().ascii_serialization())
    }
}

/// **Part of a shot**: `rect` (CSS pixels in the view, `vw` wide) of the PNG
/// `png`, as a PNG — what shows of it, at least 2 × 2 pixels. Pure.
pub fn crop_to(png: &[u8], rect: (f64, f64, f64, f64), vw: f64) -> Option<Vec<u8>> {
    let (rgba, w, h) = crate::snip::images::decode_png(png).ok()?;
    let scale = if vw > 0.0 { f64::from(w) / vw } else { 1.0 };
    let x0 = (rect.0 * scale).floor().max(0.0) as u32;
    let y0 = (rect.1 * scale).floor().max(0.0) as u32;
    let x1 = ((rect.0 + rect.2) * scale).ceil().min(f64::from(w)) as u32;
    let y1 = ((rect.1 + rect.3) * scale).ceil().min(f64::from(h)) as u32;
    if x1 < x0 + 2 || y1 < y0 + 2 {
        return None;
    }
    let (cw, ch) = (x1 - x0, y1 - y0);
    let mut out = Vec::with_capacity((cw * ch * 4) as usize);
    for y in y0..y1 {
        let start = ((y * w + x0) * 4) as usize;
        out.extend_from_slice(&rgba[start..start + (cw * 4) as usize]);
    }
    crate::snip::images::encode_png(&out, cw, ch, true).ok()
}

/// **A still of page `key` as it's in view** (annotating: drawn on in Yap's
/// own page), as a `data:` PNG.
#[tauri::command]
pub async fn page_still(key: String) -> Result<String, String> {
    let png = super::view::shot_in_view(&key).await?;
    Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)))
}

/// **An annotated still** (the drawing laid on it, a `data:` PNG or its
/// base64): kept and put in Chat's box, as a screenshot is.
#[tauri::command]
pub async fn page_annotated(app: AppHandle, png: String) -> Result<crate::chat::chats::ChatImage, String> {
    let b64 = png.split_once("base64,").map_or(png.as_str(), |(_, b)| b);
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|_| "That drawing isn't a picture.".to_string())?;
    if bytes.len() > 40 * 1024 * 1024 {
        return Err("That drawing is too big.".into());
    }
    let image = tauri::async_runtime::spawn_blocking(move || crate::snip::attach::keep(&bytes, "annotation")).await.map_err(|e| e.to_string())??;
    crate::snip::waiting::added(&app, vec![image.clone()]);
    tracing::info!(w = image.w, h = image.h, "Chat: an annotated page added to the message box");
    Ok(image)
}

/// Test mode: a person's click at (`x`, `y`) of page `key`, in CSS pixels
/// of its view (real input: select mode's overlay takes it, as a mouse's).
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn e2e_page_click(key: String, x: f64, y: f64) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".into());
    }
    let calls = [
        ("Input.dispatchMouseEvent", json!({ "type": "mouseMoved", "x": x, "y": y })),
        ("Input.dispatchMouseEvent", json!({ "type": "mousePressed", "x": x, "y": y, "button": "left", "clickCount": 1 })),
        ("Input.dispatchMouseEvent", json!({ "type": "mouseReleased", "x": x, "y": y, "button": "left", "clickCount": 1 })),
    ];
    super::backend::input(&key, &calls).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el() -> Picked {
        Picked {
            tag: "div".into(),
            classes: vec!["order".into(), "open".into()],
            text: "Build Order\n10 Moss Shield Rank 1".into(),
            path: "body > article > div.order".into(),
            styles: [("display".to_string(), "block".to_string()), ("color".to_string(), "rgb(1, 2, 3)".to_string()), ("margin".to_string(), String::new())].into_iter().collect(),
            html: "<div class=\"order open\"><h2>Build Order</h2></div>".into(),
            url: "https://wiki.example.com/guide?x=\"1\"".into(),
            image: Some("abc".into()),
            ..Default::default()
        }
    }

    #[test]
    fn an_element_reads_as_a_block_after_the_words_and_is_web_content() {
        let b = block(&el());
        assert!(b.starts_with("<selected-element page=\"https://wiki.example.com/guide?x=&quot;1&quot;\" tag=\"div\" class=\"order open\" picture=\"attached\">"), "{b}");
        assert!(b.contains("<text>Build Order\n10 Moss Shield Rank 1</text>") && b.contains("<path>body > article > div.order</path>"));
        // Only the styles it has.
        assert!(b.contains("<styles>color: rgb(1, 2, 3); display: block</styles>"), "{b}");
        assert!(b.ends_with("</selected-element>"));
        let text = with_elements("Make this green. ", &[el()]);
        assert!(text.starts_with("Make this green.\n\n<selected-element ") && has_element(&text));
        assert_eq!(with_elements("Just words", &[]), "Just words");
        assert!(!has_element("Just words about <selected-element>"));
    }

    #[test]
    fn a_react_element_carries_its_component_owners_props_and_source() {
        let raw = json!({
            "tag": "button", "id": "remove-2", "text": "Remove", "path": "body > div#root > ul > li.basket-row",
            "react": { "component": "BasketRow", "owners": ["BasketPage", "App"], "inside": false, "props": ["currency: \"GBP\"", "item: {id, name, price}", "onRemove: ƒ remove"], "source": "src/Basket.js:14:12", "sourceFrom": "stack", "build": "development", "up": 0 }
        });
        let mut el: Picked = serde_json::from_value(raw).unwrap();
        el.react = el.react.take().map(ReactPart::trimmed);
        let b = block(&el);
        assert!(b.contains("<react component=\"BasketRow\" source=\"src/Basket.js:14:12 (as the page serves it)\" build=\"development\">\n<owners>BasketPage ‹ App</owners>\n<props>currency: \"GBP\"\nitem: {id, name, price}\nonRemove: ƒ remove</props>\n</react>"), "{b}");
        // After its path, before its styles and HTML.
        assert!(b.find("<path>").unwrap() < b.find("<react ").unwrap());
        // A production build: the components it's inside; React 18's own source as it is.
        let prod = ReactPart { component: "a".into(), owners: vec!["b".into()], inside: true, source: "/app/src/Row.jsx:3:5".into(), source_from: "jsx".into(), build: "production".into(), ..Default::default() };
        let b = block(&Picked { tag: "div".into(), react: Some(prod), ..Default::default() });
        assert!(b.contains("<react component=\"a\" source=\"/app/src/Row.jsx:3:5\" build=\"production\">\n<inside>b</inside>\n</react>"), "{b}");
        // No React: no part.
        assert!(!block(&Picked { tag: "p".into(), id: "build".into(), ..Default::default() }).contains("<react"));
        // Kept to a message's size.
        let long = ReactPart { component: "C".repeat(200), props: (0..40).map(|i| format!("p{i}: {}", "x".repeat(300))).collect(), ..Default::default() }.trimmed();
        assert_eq!((long.component.chars().count(), long.props.len()), (81, 20));
        assert!(long.props.iter().all(|p| p.chars().count() <= 161));
    }

    #[test]
    fn a_stack_frames_file_is_its_path_on_the_pages_own_site() {
        assert_eq!(served_path("https://shop.example.com/src/Basket.js:11:5", "https://shop.example.com/basket"), "src/Basket.js:11:5");
        // A dev server's query and Vite's /@fs off.
        assert_eq!(served_path("http://localhost:5173/src/App.jsx?t=1731600000000:12:7", "http://localhost:5173/"), "src/App.jsx:12:7");
        assert_eq!(served_path("http://localhost:5173/@fs/work/app/src/Row.tsx:3:1", "http://localhost:5173/"), "work/app/src/Row.tsx:3:1");
        // Another site's: as it is; a bundler's: as it is, its query off.
        assert_eq!(served_path("https://cdn.example.net/app.js:1:200", "https://shop.example.com/"), "https://cdn.example.net/app.js:1:200");
        assert_eq!(served_path("webpack-internal:///(app-pages-browser)/./src/app/page.tsx?abc:20:9", "http://localhost:3000/"), "webpack-internal:///(app-pages-browser)/./src/app/page.tsx:20:9");
        assert_eq!(served_path("not a url", "https://shop.example.com/"), "not a url");
    }

    #[test]
    fn a_crop_is_what_shows_of_the_element_at_the_shots_scale() {
        // A 20 × 10 shot of a 10-CSS-pixel-wide view (2 device pixels a CSS pixel): red right half.
        let rgba: Vec<u8> = (0..10).flat_map(|_| (0..20).flat_map(|x| if x >= 10 { [255, 0, 0, 255] } else { [255, 255, 255, 255] })).collect();
        let png = crate::snip::images::encode_png(&rgba, 20, 10, true).unwrap();
        let cut = crop_to(&png, (5.0, 0.0, 5.0, 5.0), 10.0).unwrap();
        let (px, w, h) = crate::snip::images::decode_png(&cut).unwrap();
        assert_eq!((w, h), (10, 10));
        assert!(px.as_chunks::<4>().0.iter().all(|p| *p == [255, 0, 0, 255]));
        // Past the edge: what shows; nothing showing: none.
        assert_eq!(crate::snip::images::decode_png(&crop_to(&png, (8.0, 3.0, 50.0, 50.0), 10.0).unwrap()).unwrap().1, 4);
        assert!(crop_to(&png, (20.0, 0.0, 5.0, 5.0), 10.0).is_none());
    }
}

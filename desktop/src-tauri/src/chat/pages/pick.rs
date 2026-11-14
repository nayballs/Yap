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
//!   clipped), crops its picture from a shot of the view, puts the picture in
//!   Chat's box (`snip/waiting.rs`, source "element": its chip shows it, not
//!   a tile), and tells the page tab ([`EVENT`]): the message box's chip.
//! - **Annotate** ([`page_still`], [`page_annotated`]): a still of the page
//!   as it's in view, drawn on in Yap's own page (a pen, shapes, words:
//!   `PageTab.svelte`), the drawing laid on it there and the picture put in
//!   Chat's box (source "annotation").
//! - **Sent** ([`block`]): each element as a `<selected-element>` block
//!   after the person's words — Claude reads its tag, words, path, styles
//!   and HTML; its picture goes as the message's picture. Web content: a
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
var html=el.outerHTML||'',sib='';
if(el.parentElement){sib=[].slice.call(el.parentElement.children,0,12).map(function(c){return (c===el?'<!-- SELECTED -->':'')+clip(c.outerHTML||'',300);}).join('\n');}
return {tag:el.tagName.toLowerCase(),id:el.id||'',classes:[].slice.call(el.classList||[],0,8),text:(el.innerText||el.textContent||'').replace(/\n{3,}/g,'\n\n').trim().slice(0,2000),path:path.join(' > '),styles:styles,html:clip(html,3000),siblings:clip(sib,3000),rect:{x:r.left,y:r.top,w:r.width,h:r.height},vw:innerWidth,url:location.href,title:document.title};
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
    tracing::info!(chars = el.text.chars().count(), html = el.html.len(), picture = el.image.is_some(), "Chat: an element picked on a page beside the chat");
    Ok(el)
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

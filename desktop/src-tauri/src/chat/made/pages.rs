//! **Pages Yap keeps** (15 Nov 2026, after the Claude app's artifacts: its
//! look and words, nothing of its code; local only, never published): an
//! HTML page Chat made to keep — a small app, a tool, a page to come back to
//! ("make me a tip calculator I can keep") — saved in the chat's own
//! folder, opened beside the chat in the same sandboxed frame as a visual,
//! a card in the thread for each version, and in the chat's Outputs.
//!
//! - **`save_page { title, html, id? }`** ([`definitions`], [`save_page`]):
//!   a whole HTML page (or a fragment). No `id`: a new page (`pg_` + 16 hex);
//!   one of this chat's ids: its next version, the earlier ones kept
//!   ([`MAX_VERSIONS`]). Offered where `show_visual` is (`made::offered`),
//!   and over the run endpoint the same way; `made::INSTRUCTIONS` says when
//!   (something to keep and come back to; writing to keep stays a document).
//! - **Kept** as a [`Made`] of kind "page" in the chat's `made.json`, its
//!   versions in [`Made::versions`] (each with the question it answered),
//!   each version's HTML as `<id>.v<n>.html`.
//! - **Shown** at `/p/<chat>/<id>/<n>` (`serve.rs`): the page's own document,
//!   its policy and sandbox as a visual's, scrolling in its own frame.
//!
//! **Privacy**: the log has ids, versions and sizes, never a title or the page.

use serde_json::{json, Value};

use super::{anchor_of, clip, emit, list, new_id, now_ms, refused_scripts, with_index, Made, PageVersion, TITLE_MAX};

/// The tool's name.
pub const SAVE_PAGE: &str = "save_page";
/// A page's HTML, at most (bytes).
pub const MAX_PAGE: usize = 512 * 1024;
/// A page's versions, at most.
pub const MAX_VERSIONS: usize = 40;

/// What the tool does, for the model.
pub const DESCRIPTION: &str = "Keep an HTML page the user comes back to (a small app, a tool): it opens beside the chat, in its Outputs. The whole page; with an earlier one's id, its next version. As show_visual: its colour variables, scripts inline or Chart.js/D3 from cdnjs, no other network.";

/// Its parameters.
pub fn parameters() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string", "description": "Its name, in a few words" },
            "html": { "type": "string", "description": "The whole page" },
            "id": { "type": "string", "description": "An earlier page's id (pg_…): its next version" }
        },
        "required": ["title", "html"],
        "additionalProperties": false
    })
}

/// The tool's schema in the tool loop (none when not offered).
pub fn definitions(offer: bool) -> Value {
    if !offer {
        return json!([]);
    }
    json!([{ "type": "function", "function": { "name": SAVE_PAGE, "description": DESCRIPTION, "parameters": parameters() } }])
}

/// Whether `id` is a page's: `pg_` and 16 lowercase hex digits. Pure.
pub fn is_id(id: &str) -> bool {
    id.strip_prefix("pg_").is_some_and(|h| h.len() == 16 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}

/// A version's file in the chat's folder.
pub fn file_of(id: &str, n: u32) -> String {
    format!("{id}.v{n}.html")
}

fn failed(error: String, display: &str) -> Value {
    json!({ "success": false, "error": error, "data": Value::Null, "displayText": display })
}

/// **Run `save_page`** for conversation `conv`; `outside`: outside content
/// came into the turn before it. Its result, for the model; the page hears
/// the page at once (and opens it beside the chat).
pub fn save_page(conv: Option<u64>, args: &Value, outside: bool) -> Value {
    let html = args["html"].as_str().or_else(|| args["code"].as_str()).unwrap_or("").trim();
    if html.is_empty() {
        return failed("Give save_page the page: its whole HTML.".into(), "Didn't save a page: no HTML");
    }
    if html.len() > MAX_PAGE {
        return failed(format!("That page is too big ({} KB; at most {} KB): make it simpler.", html.len() / 1024, MAX_PAGE / 1024), "Didn't save a page: too big");
    }
    let refused = refused_scripts(html);
    if !refused.is_empty() {
        let allowed = super::serve::LIBS.iter().map(|l| format!("{} ({})", l.name, l.urls[0])).collect::<Vec<_>>().join(", ");
        return failed(format!("A page can't load scripts from {}: write the script inline, or use {allowed}. Try again.", refused.join(", ")), "Didn't save a page: a script from the web");
    }
    let Some(conv_id) = conv else {
        return failed("There's no chat to keep a page in for this question: answer in words.".into(), "Didn't save a page: no chat here");
    };
    let Some(c) = crate::chat::chats::get(conv_id) else {
        return failed("This chat isn't there any more: answer in words.".into(), "Didn't save a page: the chat's gone");
    };
    let title = clip(args["title"].as_str().unwrap_or(""), TITLE_MAX);
    let wanted = args["id"].as_str().map(str::trim).filter(|s| !s.is_empty());
    if let Some(id) = wanted {
        if !is_id(id) || !list(conv_id, c.created_ts).iter().any(|m| m.id == id && m.kind == "page") {
            return failed(format!("There's no page \"{id}\" in this chat: leave id out to save a new page, or use the id save_page gave."), "Didn't save a page: no such page here");
        }
    }
    let after = anchor_of(&c.messages);
    let at = now_ms();
    let bytes = html.len() as u64;
    let saved = with_index(conv_id, c.created_ts, |index, folder| {
        let made = match wanted {
            Some(id) => {
                let Some(m) = index.items.iter_mut().find(|m| m.id == id) else { return Err("gone".into()) };
                if m.versions.len() >= MAX_VERSIONS {
                    return Err("full".into());
                }
                m
            }
            None => {
                index.items.push(Made { id: new_id("pg_"), kind: "page".into(), format: "html".into(), after, ..Default::default() });
                index.items.last_mut().ok_or("gone")?
            }
        };
        let n = made.versions.iter().map(|v| v.n).max().unwrap_or(0) + 1;
        crate::config::atomic_write(&folder.join(file_of(&made.id, n)), html).map_err(|e| format!("couldn't save it ({:?})", e.kind()))?;
        if !title.is_empty() || made.title.is_empty() {
            made.title = if title.is_empty() { "Page".into() } else { title.clone() };
        }
        made.at = at;
        made.bytes = bytes;
        made.outside |= outside;
        made.versions.push(PageVersion { n, at, bytes, after, title: made.title.clone(), ..Default::default() });
        Ok(made.clone())
    });
    let made = match saved {
        Ok(m) => m,
        Err(why) if why == "full" => {
            return failed(format!("That page has {MAX_VERSIONS} versions already: save the change as a new page (leave id out)."), "Didn't save a page: too many versions");
        }
        Err(why) => {
            tracing::warn!(conversation = conv_id, "Chat: a page couldn't be saved: {why}");
            return failed("The page couldn't be saved on this PC: answer in words.".into(), "Didn't save a page: couldn't save it");
        }
    };
    let n = made.versions.last().map(|v| v.n).unwrap_or(1);
    tracing::info!(conversation = conv_id, id = %made.id, version = n, bytes, outside, "Chat: a page kept");
    emit(conv_id, &made);
    let shown = if n == 1 { format!("Saved the page \u{201c}{}\u{201d}", made.title) } else { format!("Saved version {n} of \u{201c}{}\u{201d}", made.title) };
    json!({
        "success": true,
        "data": {
            "saved": shown,
            "id": made.id,
            "version": n,
            "next": format!("It's open beside the chat and kept in this chat's Outputs: say in a line or two what it does (never its code). To change it later, call save_page with id \"{}\" and the whole new page.", made.id)
        },
        "displayText": if n == 1 { "Saved a page".to_string() } else { format!("Saved version {n} of a page") }
    })
}

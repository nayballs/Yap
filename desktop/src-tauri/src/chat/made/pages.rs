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
//! - **Outside Yap** (its tab's ⋯, as the Claude app's artifact menu has
//!   Download and Open: [`outside_document`]): **Save as…** writes the
//!   version shown to a file the person picks ([`chat_page_save_as`]);
//!   **Open in browser** writes it to a copy of Yap's own in the system's
//!   temp folder and hands that to the person's browser
//!   ([`chat_page_open_in_browser`], `shell/browsers.rs` `open_file`),
//!   swept after a day ([`sweep_copies`]). Either way it keeps working:
//!   Yap's colours (light or dark, as the browser is), the pinned libraries
//!   from their cdnjs addresses with their hashes (the same files the
//!   sandbox serves), and the same policy as a `<meta>` (no other network,
//!   frames or forms). `sendPrompt` does nothing there.
//!
//! **Privacy**: the log has ids, versions and sizes, never a title or the page.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use super::{anchor_of, attribute, clip, emit, list, new_id, now_ms, refused_scripts, serve, with_index, Made, PageVersion, TITLE_MAX};

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

// ---- Outside Yap: Save as… and Open in browser (15 Nov 2026) ----------------

/// The folder "Open in browser" writes its copies to, in the system's temp
/// folder: Yap's own, swept ([`sweep_copies`]).
pub const COPIES: &str = "yap-pages";
/// A copy is kept this long (the browser reads it again on a reload).
pub const COPY_KEEP: Duration = Duration::from_secs(24 * 60 * 60);

/// Yap's colours in dark, as the panel hands them to the frame
/// (`visualBridge.js`, app.css' dark tokens): a page outside Yap takes them
/// when the browser is dark.
const DARK_TOKENS: &str = "--text:#f2efe8;--text-2:rgba(242,239,232,.64);--muted:#a8a294;--bg:#1c1a16;--surface:#24211c;--surface-2:#2b2722;--border:#3a352b;--accent:#e2982a;--accent-text:#1f1c16;--accent-soft:rgba(226,152,42,.14);--good:#3dbb74;--warn:#dca03c;--bad:#e5645e";

/// **The script a page has outside Yap**: the theme as the browser's
/// (`data-theme`, as the bridge sets it in Yap), shown at once (nothing
/// sends colours there), and `sendPrompt` there but doing nothing.
pub const OUTSIDE_SCRIPT: &str = "(()=>{const r=document.documentElement;const m=matchMedia('(prefers-color-scheme: dark)');const set=()=>{r.dataset.theme=m.matches?'dark':'light';r.style.colorScheme=r.dataset.theme};set();try{m.addEventListener('change',set)}catch(e){}r.classList.add('yap-themed');window.sendPrompt=function(){};})();";

/// **The policy a page carries outside Yap**, as a `<meta>` (all a file can
/// carry): the sandbox's ([`serve::csp`]) with each pinned library allowed
/// at its own cdnjs address alone, less what only a response can say
/// (`sandbox`). Pure.
pub fn outside_csp() -> String {
    let libs: Vec<&str> = serve::LIBS.iter().map(|l| l.urls[0]).collect();
    [
        "default-src 'none'".to_string(),
        format!("script-src 'unsafe-inline' {}", libs.join(" ")),
        "style-src 'unsafe-inline'".into(),
        "img-src data: blob:".into(),
        "font-src data:".into(),
        "media-src data: blob:".into(),
        "connect-src 'none'".into(),
        "frame-src 'none'".into(),
        "child-src 'none'".into(),
        "worker-src 'none'".into(),
        "object-src 'none'".into(),
        "manifest-src 'none'".into(),
        "base-uri 'none'".into(),
        "form-action 'none'".into(),
    ]
    .join("; ")
}

/// A library's Subresource Integrity value (`sha256-…`), from its pinned
/// hash. Pure.
pub fn integrity(lib: &serve::Lib) -> String {
    use base64::Engine;
    let bytes: Vec<u8> = (0..lib.sha256.len() / 2).filter_map(|i| u8::from_str_radix(&lib.sha256[i * 2..i * 2 + 2], 16).ok()).collect();
    format!("sha256-{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// **The page's libraries from their cdnjs addresses**: each `<script>`
/// naming one of [`serve::LIBS`] (any address of its, or `/lib/<file>`)
/// becomes its first address with its hash (`integrity`), so the browser
/// runs exactly the file the sandbox does. Pure.
pub fn outside_libraries(code: &str) -> String {
    let lower = code.to_ascii_lowercase();
    let mut out = String::with_capacity(code.len() + 256);
    let (mut from, mut copied) = (0, 0);
    while let Some(i) = lower[from..].find("<script") {
        let start = from + i;
        let Some(len) = lower[start..].find('>') else { break };
        let end = start + len;
        if let Some(lib) = attribute(&code[start..end], &lower[start..end], "src").as_deref().and_then(serve::lib_for) {
            out.push_str(&code[copied..start]);
            out.push_str(&format!("<script src=\"{}\" integrity=\"{}\" crossorigin=\"anonymous\" referrerpolicy=\"no-referrer\">", lib.urls[0], integrity(lib)));
            copied = end + 1;
        }
        from = end;
    }
    out.push_str(&code[copied..]);
    out
}

/// `s` for an HTML text or attribute. Pure.
fn escaped(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The head's first children outside Yap: the policy first, the title when
/// the page has none, Yap's base style (light, dark as the browser is) and
/// the page's own, then [`OUTSIDE_SCRIPT`].
fn outside_head(title: Option<&str>) -> String {
    let base: String = serve::BASE_STYLE.lines().filter(|l| !l.contains("yap-themed")).collect::<Vec<_>>().join("\n");
    let title = title.map(|t| format!("<title>{}</title>", escaped(t))).unwrap_or_default();
    format!(
        "<meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"{}\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">{title}<style>{base}\n:root{{--accent-soft:rgba(226,152,42,.16)}}\n@media (prefers-color-scheme: dark){{:root{{{DARK_TOKENS}}}}}\nhtml{{background:var(--bg)}}\n{}</style><script>{OUTSIDE_SCRIPT}</script>",
        outside_csp(),
        serve::PAGE_STYLE
    )
}

/// **A page's document outside Yap** (Save as…, Open in browser): `html`
/// as kept, with [`outside_head`] first in its head (a fragment in a page
/// of Yap's, as in the panel) and its libraries from cdnjs. `title`: its
/// name, the document's title when it has none. Pure.
pub fn outside_document(html: &str, title: &str) -> String {
    let code = outside_libraries(html);
    let lower = code.to_ascii_lowercase();
    let named = if lower.contains("<title") { None } else { Some(if title.trim().is_empty() { "Page" } else { title.trim() }) };
    let head = outside_head(named);
    let whole = lower.contains("<html") || lower.trim_start().starts_with("<!doctype");
    if !whole {
        return format!("<!doctype html><html lang=\"en\"><head>{head}</head><body><main id=\"yap-page\">{code}</main></body></html>");
    }
    let after = |tag: &str| lower.find(tag).and_then(|i| lower[i..].find('>').map(|j| i + j + 1));
    match after("<head").or_else(|| after("<html")).or_else(|| after("<!doctype")) {
        Some(i) => format!("{}{head}{}", &code[..i], &code[i..]),
        None => format!("{head}{code}"),
    }
}

/// A file name for a page's title: Windows' forbidden characters out, at
/// most 60 characters, "Page" for none. Pure.
pub fn file_name_for(title: &str) -> String {
    let cleaned: String = title.chars().map(|c| if c.is_control() || matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { ' ' } else { c }).collect();
    let words = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = words.chars().take(60).collect();
    let name = cut.trim().trim_matches('.').trim().to_string();
    if name.is_empty() {
        "Page".into()
    } else {
        name
    }
}

/// **Page `id`'s version `n` (0: its newest) of conversation `conv`, as a
/// document outside Yap**: `(title, n, document)`, or why not. Only this
/// chat's own page.
fn outside_of(conv: u64, id: &str, n: u32) -> Result<(String, u32, String), String> {
    let created = crate::chat::chats::get(conv).map(|c| c.created_ts).ok_or("This chat isn't here any more.")?;
    if !is_id(id) || !list(conv, created).iter().any(|m| m.id == id && m.kind == "page") {
        return Err("This page isn't here any more.".into());
    }
    let (made, n, html) = super::page_of(conv, id, n).ok_or("This version of the page isn't here any more.")?;
    let title = made.versions.iter().find(|v| v.n == n).map(|v| v.title.clone()).filter(|t| !t.is_empty()).unwrap_or(made.title);
    let doc = outside_document(&html, &title);
    Ok((title, n, doc))
}

/// Where "Open in browser" writes its copies.
pub fn copies_dir() -> PathBuf {
    std::env::temp_dir().join(COPIES)
}

/// **Old copies swept**: the `.html` files in `dir` (Yap's own folder of
/// them) older than `keep`. How many went.
pub fn sweep_copies(dir: &Path, keep: Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("html")))
        .filter(|e| e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).is_some_and(|age| age > keep))
        .filter(|e| std::fs::remove_file(e.path()).is_ok())
        .count()
}

/// At start-up (`lib.rs`, the temp sweep's thread): the copies from before.
pub fn sweep_old_copies() {
    let n = sweep_copies(&copies_dir(), COPY_KEEP);
    if n > 0 {
        tracing::info!(removed = n, "Chat: old copies of kept pages swept");
    }
}

/// **A copy of a page for the browser**: written to [`copies_dir`] under its
/// title and a random tag (two copies never share a file), the old ones
/// swept first. Its path.
fn write_copy(title: &str, doc: &str) -> Result<PathBuf, String> {
    let dir = copies_dir();
    let _ = sweep_copies(&dir, COPY_KEEP);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't make a copy for the browser ({:?}).", e.kind()))?;
    let tag = new_id("");
    let path = dir.join(format!("{} {}.html", file_name_for(title), &tag[..8]));
    crate::config::atomic_write(&path, doc).map_err(|e| format!("Couldn't make a copy for the browser ({:?}).", e.kind()))?;
    Ok(path)
}

/// **Save as…** (the kept page's ⋯): version `n` of page `id`, as it works
/// outside Yap, written to `path` (the dialog's: the person picked it;
/// `.html` added when it has no ending). The path written.
#[tauri::command]
pub async fn chat_page_save_as(conversation_id: u64, id: String, n: u32, path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (_, n, doc) = outside_of(conversation_id, &id, n)?;
        let mut to = PathBuf::from(path.trim());
        if to.as_os_str().is_empty() || to.file_name().is_none() {
            return Err("Pick a file to save it to.".into());
        }
        if to.extension().is_none() {
            to.set_extension("html");
        }
        crate::config::atomic_write(&to, &doc).map_err(|e| format!("Couldn't save the page there ({:?}).", e.kind()))?;
        tracing::info!(conversation = conversation_id, id = %id, version = n, bytes = doc.len(), "Chat: a kept page saved as a file");
        Ok(to.to_string_lossy().into_owned())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// **Open in browser** (the kept page's ⋯): version `n` of page `id`, as it
/// works outside Yap, in a copy of Yap's own ([`write_copy`]) opened in the
/// person's browser (`shell/browsers.rs`). A test run keeps the copy's
/// address for the suite (`e2e_opened_urls`) and opens nothing.
#[tauri::command]
pub async fn chat_page_open_in_browser(app: tauri::AppHandle, conversation_id: u64, id: String, n: u32) -> Result<(), String> {
    let path = tauri::async_runtime::spawn_blocking(move || {
        let (title, n, doc) = outside_of(conversation_id, &id, n)?;
        let path = write_copy(&title, &doc)?;
        tracing::info!(conversation = conversation_id, id = %id, version = n, bytes = doc.len(), "Chat: a kept page opened in the browser");
        Ok::<_, String>(path)
    })
    .await
    .map_err(|e| e.to_string())??;
    if crate::shell::e2e::active() {
        let url = url::Url::from_file_path(&path).map(|u| u.to_string()).unwrap_or_else(|_| path.to_string_lossy().into_owned());
        crate::shell::commands::e2e_kept(&url);
        return Ok(());
    }
    crate::shell::browsers::open_file(&app, &path)
}

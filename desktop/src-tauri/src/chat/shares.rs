//! **Files shared into the conversation** (8 Nov 2026, after the Claude
//! app's `SendUserFile`): Chat puts files straight into the thread — a
//! picture large and inline with its caption, several as a grid, any other
//! file as a card the person opens — where `show_picture` puts one picture
//! beside the chat.
//!
//! - **`share_files { files, caption?, display? }`** ([`definitions`],
//!   [`execute`]): 1–[`MAX_FILES`] files a call, each a file's path or (with
//!   the web on) one of the turn's pictures ("p2"); `display` "render" (the
//!   default: pictures inline, the rest as cards) or "attach" (every one a
//!   card). Offered wherever `show_picture` takes a path ([`Offer::of`]: the
//!   web's pictures' gate, or the picture files' gate without the web — a
//!   Code chat, a question naming a picture file), in the tool loop and over
//!   the run endpoint for a plan chat (`claude_live/eyes.rs`); kept to the
//!   last in a small context (`turn/fit.rs` `Family::Files`).
//! - **Which files**: as for a picture file (`pictures/local.rs`): one of
//!   the person's workspaces, a file an agent of this chat made or changed,
//!   or a path the person wrote — as written and again as resolved
//!   (`local::resolve`).
//! - **A picture** (by its own bytes, `local::kind`; at most
//!   `local::MAX_BYTES`) is copied into `chat-images/` as a pasted one is
//!   (`snip::attach::keep`: its thumbnails at exact device pixels), kept
//!   with the answer and deleted with the chat. A bigger one, or any picture
//!   with `display` "attach", is a card.
//! - **Any other file** is linked by its path, never copied, at any size: its
//!   name, folder, kind ([`kind_of`]), size, a PDF's page count when easy
//!   ([`pdf_pages`]). Its card's preview — the first lines of a text,
//!   Markdown or code file, ≤ [`PREVIEW_LINES`] lines and ≤ [`PREVIEW_BYTES`],
//!   plain text — is read when the person unfolds it ([`chat_file_preview`]),
//!   never kept in chats.json.
//! - **The answer keeps them** (`ChatMessage::files`, a [`Share`] a call),
//!   the page hears each at once (`stream::Event::Share`). An older Yap
//!   sharing the data folder drops the field when it saves chats.json, so
//!   every save copies the shares into `chat_files.json` too ([`mirror_to_disk`]),
//!   which no older Yap touches, and the next start puts back any it dropped
//!   ([`restore_from_disk`]).
//! - **Open** (the person's own click, [`chat_file_open`]): a Markdown file in
//!   Yap's own window (`notes/md_file.rs`), any other in the person's default
//!   app — never one that would run a program or a web page ([`opens`]); a
//!   test run keeps what it would have opened (`e2e_kept`). **Show in
//!   folder** ([`chat_file_reveal`]). Each only for a file shared in that
//!   conversation.
//! - **An agent's report** ([`of_agent_turn`]): a Yap Code agent's turn ends
//!   with the pictures it created or changed on its report card (up to
//!   [`REPORT_MAX`], then "+N more") and its other new files as compact
//!   cards — the same checks, no tool call.
//!
//! **Privacy**: the log has counts, kinds and sizes, never a path or a name.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::chat::pictures::{self, local, Gate, PageRef, TurnPictures};

/// The tool's name.
pub const NAME: &str = "share_files";
/// Files one call shares, at most.
pub const MAX_FILES: usize = 8;
/// Files one turn shares, at most.
pub const MAX_A_TURN: usize = 24;
/// A card's preview: at most this many lines…
pub const PREVIEW_LINES: usize = 40;
/// …and this many bytes.
pub const PREVIEW_BYTES: usize = 4 * 1024;
/// A caption's most characters.
pub const CAPTION_MAX: usize = 200;
/// An agent's report card: pictures, and other new files, at most.
pub const REPORT_MAX: usize = 4;
/// The most of a PDF read for its page count (its head and its tail).
const PDF_READ: u64 = 4 * 1024 * 1024;

// ---- what's kept --------------------------------------------------------------------------------

/// **One file shared**: what chats.json and the page keep (the same keys in
/// JS). A picture shown inline has its `chat-images` id; anything else is a
/// link to its path.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedFile {
    /// The file's name ("concept.png"), or what a picture from the web is.
    #[serde(default)]
    pub name: String,
    /// Its full path on this PC, as the file system resolved it ("" for a
    /// picture from the web).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
    /// Its folder.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub folder: String,
    /// "image" | "markdown" | "text" | "code" | "pdf" | "file" ([`kind_of`]).
    #[serde(default)]
    pub kind: String,
    /// Its size when shared.
    #[serde(default)]
    pub bytes: u64,
    /// A picture shown inline: its copy in `chat-images/` (`snip::images`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub w: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub h: u32,
    /// A PDF's pages, when they were easy to count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pages: Option<u32>,
    /// A picture from the web: where it came from ("Open original") and its page.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub image_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<PageRef>,
    /// The page's copy only ([`for_page`], never kept): the file isn't at its
    /// path any more.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub missing: bool,
    /// The page's copy only: its picture's copy in `chat-images/` is gone
    /// (swept): shown as its file's card instead.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub gone: bool,
    /// What Open does ([`opens`]): "yap" (Yap's Markdown window), "app" (the
    /// default app), "" (no Open: it could run a program).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub opens: String,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

impl SharedFile {
    /// Shown inline: a picture with its copy.
    pub fn renders(&self) -> bool {
        !self.id.is_empty()
    }
}

/// **One share**: a call's files and its caption — one picture large, a
/// grid, cards. On an agent's report, how many more there were than it shows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Share {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub caption: String,
    #[serde(default)]
    pub items: Vec<SharedFile>,
    /// An agent's report: pictures past the [`REPORT_MAX`] it shows.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub more_pictures: u32,
    /// …and other new files past the cards it shows.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub more_files: u32,
}

/// The `chat-images` ids `shares` keep (the sweep keeps them; a deleted
/// chat takes them).
pub fn image_ids(shares: &[Share]) -> impl Iterator<Item = String> + '_ {
    shares.iter().flat_map(|s| s.items.iter()).filter(|i| i.renders()).map(|i| i.id.clone())
}

/// **The page's copy**: each file still at its path or not (`missing`), each
/// picture's copy still there or not (`gone`), and what Open does. Reads the
/// file system.
pub fn for_page(shares: &mut [Share]) {
    for item in shares.iter_mut().flat_map(|s| s.items.iter_mut()) {
        if !item.path.is_empty() {
            item.missing = !Path::new(&item.path).is_file();
            item.opens = opens(Path::new(&item.path)).word().to_string();
        }
        if item.renders() {
            item.gone = !crate::snip::images::exists(&item.id);
        }
    }
}

// ---- the tool -----------------------------------------------------------------------------------

/// **Where the tool is offered**: as `show_picture` with a path is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Offer {
    #[default]
    Off,
    /// The web off (a Code chat, a question naming a picture file): files'
    /// paths only.
    Files,
    /// The web on: a path, or one of the turn's pictures ("p2").
    Web,
}

impl Offer {
    /// From the web's pictures' gate, the picture files' gate (the web off)
    /// and whether a small context keeps it (`fit.rs` `Family::Files`).
    /// Pure.
    pub fn of(pictures: Gate, files: Gate, kept: bool) -> Offer {
        if !kept {
            Offer::Off
        } else if pictures != Gate::Off {
            Offer::Web
        } else if files != Gate::Off {
            Offer::Files
        } else {
            Offer::Off
        }
    }

    /// Whether it offers `name`. Pure.
    pub fn offers(self, name: &str) -> bool {
        self != Offer::Off && name == NAME
    }
}

/// The tool's schema for `offer` (none when off). Two sentences, kept short:
/// every step sends it.
pub fn definitions(offer: Offer) -> Value {
    let (item, which) = match offer {
        Offer::Off => return json!([]),
        Offer::Files => ("A file's full path", "files on this PC"),
        Offer::Web => ("A file's full path, or one of this turn's pictures like \"p2\"", "files on this PC, or this turn's pictures"),
    };
    json!([{
        "type": "function",
        "function": {
            "name": NAME,
            "description": format!("Show the user files in the conversation itself: pictures appear large in the chat, other files as cards they can open. Use it for {which} they should see or keep."),
            "parameters": {
                "type": "object",
                "properties": {
                    "files": { "type": "array", "items": { "type": "string", "description": item }, "description": "1 to 8 files" },
                    "caption": { "type": "string", "description": "What they are, in a few words" },
                    "display": { "type": "string", "enum": ["render", "attach"], "description": "render (default): pictures in the chat; attach: every file a card" }
                },
                "required": ["files"],
                "additionalProperties": false
            }
        }
    }])
}

/// What running the tool gave: its result for the model, and the share for
/// the page (it hears it at once) and the answer.
#[derive(Debug, Default)]
pub struct Run {
    pub result: Value,
    pub share: Option<Share>,
}

fn failed(error: String, display: String) -> Value {
    json!({ "success": false, "error": error, "data": Value::Null, "displayText": display })
}

/// `s`, at most `max` characters (then "…"), on one line. Pure.
fn clip(s: &str, max: usize) -> String {
    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
    match one.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", one[..i].trim_end()),
        None => one,
    }
}

/// The files a call names: `files` (a list, or one string), else `path`.
fn files_of(args: &Value) -> Vec<String> {
    let one = |v: &Value| v.as_str().map(|s| s.trim().to_string()).or_else(|| v.as_u64().map(|n| n.to_string()));
    let mut out: Vec<String> = match &args["files"] {
        Value::Array(list) => list.iter().filter_map(one).collect(),
        v @ Value::String(_) => one(v).into_iter().collect(),
        _ => Vec::new(),
    };
    if out.is_empty() {
        out.extend(one(&args["path"]));
    }
    out.retain(|f| !f.is_empty());
    out
}

/// **What was shared, in words**: "2 pictures and notes.md", "concept.png",
/// "notes.md and todo.txt", "3 pictures and 4 files". Pure.
pub fn summary(items: &[SharedFile]) -> String {
    let pictures: Vec<&SharedFile> = items.iter().filter(|i| i.renders()).collect();
    let files: Vec<&SharedFile> = items.iter().filter(|i| !i.renders()).collect();
    let mut parts: Vec<String> = Vec::new();
    match pictures.len() {
        0 => {}
        1 => parts.push(pictures[0].name.clone()),
        n => parts.push(format!("{n} pictures")),
    }
    if files.len() <= 2 {
        parts.extend(files.iter().map(|f| f.name.clone()));
    } else {
        parts.push(format!("{} files", files.len()));
    }
    match parts.len() {
        0 => "nothing".into(),
        1 => parts.remove(0),
        _ => {
            let last = parts.pop().unwrap_or_default();
            format!("{} and {last}", parts.join(", "))
        }
    }
}

/// Slow work (the file system, decoding) off the async runtime — inline in a
/// unit test, whose chat-images folder is the test thread's own.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    #[cfg(test)]
    {
        Some(f())
    }
    #[cfg(not(test))]
    {
        tauri::async_runtime::spawn_blocking(f).await.ok()
    }
}

/// One file the call named, and why it wasn't shared.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Skipped {
    name: String,
    refused: local::Refused,
}

/// **Run `share_files`** on the turn's pictures `p` (its chat's places, its
/// pictures from the web, what it shared so far); `endpoints`: the web's,
/// for a picture of this turn's (`offer` [`Offer::Web`]).
pub async fn execute(p: &mut TurnPictures, endpoints: Option<&crate::web::Endpoints>, args: &Value, offer: Offer) -> Run {
    let given = files_of(args);
    if given.is_empty() {
        return Run { result: failed("Give share_files 1 to 8 files: each a file's full path.".into(), "Didn't share anything: no files named".into()), ..Default::default() };
    }
    if given.len() > MAX_FILES {
        return Run {
            result: failed(format!("share_files takes at most {MAX_FILES} files a call: share the most useful ones, or several calls."), format!("Didn't share {} files: {MAX_FILES} a call", given.len())),
            ..Default::default()
        };
    }
    let so_far: usize = p.shared.iter().map(|s| s.items.len()).sum();
    if so_far + given.len() > MAX_A_TURN {
        return Run { result: failed(format!("You've shared {so_far} files this turn: answer the user now."), "Didn't share more: enough files this turn".into()), ..Default::default() };
    }
    let attach = args["display"].as_str().is_some_and(|d| d.trim().eq_ignore_ascii_case("attach"));
    let caption = args["caption"].as_str().map(|c| clip(c, CAPTION_MAX)).unwrap_or_default();
    let mut items: Vec<SharedFile> = Vec::new();
    let mut skipped: Vec<Skipped> = Vec::new();
    for g in &given {
        let got = if !local::is_path(g) {
            match (offer, endpoints) {
                (Offer::Web, Some(ep)) => web_picture(p, ep, g).await,
                _ => Err(Skipped {
                    name: g.clone(),
                    refused: local::Refused { error: format!("\"{g}\" isn't a file's path: share_files takes each file's full path, like C:\\Projects\\garden-planner\\docs\\concept.png."), chip: "no file's path" },
                }),
            }
        } else {
            local_file(p, g, attach, &items).await
        };
        match got {
            Ok(item) => items.push(item),
            Err(s) => skipped.push(s),
        }
    }
    if items.is_empty() {
        let first = skipped.first().cloned();
        let (error, chip) = match first {
            Some(s) if skipped.len() == 1 => (s.refused.error, format!("Didn't share \u{2018}{}\u{2019}: {}", clip(&s.name, 60), s.refused.chip)),
            Some(s) => (
                skipped.iter().map(|k| format!("{}: {}", k.name, k.refused.error)).collect::<Vec<_>>().join(" "),
                format!("Didn't share {} files: {}", skipped.len(), s.refused.chip),
            ),
            None => ("Nothing to share.".into(), "Didn't share anything".into()),
        };
        tracing::info!(asked = given.len(), "Chat: share_files shared nothing");
        return Run { result: failed(error, chip), ..Default::default() };
    }
    let words = summary(&items);
    let pictures = items.iter().filter(|i| i.renders()).count();
    tracing::info!(pictures, files = items.len() - pictures, skipped = skipped.len(), "Chat: files shared in the chat");
    let share = Share { caption, items, ..Default::default() };
    p.shared.push(share.clone());
    let listed: Vec<Value> = share.items.iter().map(|i| json!({ "name": i.name, "kind": i.kind, "shown": if i.renders() { "in the chat" } else { "as a card" } })).collect();
    let not: Vec<Value> = skipped.iter().map(|s| json!({ "file": s.name, "why": s.refused.error })).collect();
    let mut data = json!({ "shared": format!("Shared {words} in the chat"), "files": listed });
    if !not.is_empty() {
        data["notShared"] = Value::Array(not);
    }
    let result = json!({
        "success": true,
        "data": data,
        "note": "The user sees them in the chat now: don't paste their contents unless asked.",
        "displayText": format!("Shared {words}"),
    });
    Run { result, share: Some(share) }
}

/// One of the turn's pictures from the web ("p2"): downloaded once a turn
/// (as `show_picture` does), stored as a pasted picture, shown inline.
async fn web_picture(p: &mut TurnPictures, endpoints: &crate::web::Endpoints, id: &str) -> Result<SharedFile, Skipped> {
    let skip = |error: String, chip: &'static str| Skipped { name: id.to_string(), refused: local::Refused { error, chip } };
    let Some(c) = p.get(id).cloned() else {
        return Err(skip(p.unknown(id), "not one from this turn"));
    };
    let bytes = pictures::candidate_bytes(p, endpoints, &c).await.map_err(|e| skip(format!("{e} Share another one, or tell the user you couldn't get it."), "couldn't get it"))?;
    let image = blocking(move || crate::snip::attach::keep(&bytes, "web"))
        .await
        .unwrap_or_else(|| Err("Yap couldn't read it".into()))
        .map_err(|e| skip(format!("Yap couldn't read that picture ({e})."), "not a picture Yap can read"))?;
    Ok(SharedFile {
        name: clip(&c.about(), 80),
        kind: "image".into(),
        bytes: image.bytes,
        id: image.id,
        w: image.w,
        h: image.h,
        image_url: if c.image.url.starts_with(pictures::SHOT) { String::new() } else { c.image.url.clone() },
        page: Some(c.page.clone()),
        ..Default::default()
    })
}

/// **A file on this PC the call named** (`given`): in one of the turn's
/// places (`local::resolve`); a picture by its bytes (≤ `local::MAX_BYTES`,
/// not `attach`) copied and shown inline, the same file shared earlier this
/// turn as it was (`earlier`, this call's, and the turn's); anything else
/// linked.
async fn local_file(p: &TurnPictures, given: &str, attach: bool, earlier: &[SharedFile]) -> Result<SharedFile, Skipped> {
    let name = local::chip_name(given);
    let places = p.local.places();
    let given = given.to_string();
    let mut before: Vec<SharedFile> = p.shared.iter().flat_map(|s| s.items.iter().cloned()).collect();
    before.extend(earlier.iter().cloned());
    let done = blocking(move || -> Result<SharedFile, local::Refused> {
        let path = local::path_of(&given, places.base.as_deref())?;
        let (real, why, meta) = local::resolve(&path, &places, local::Noun::File)?;
        // The same file again, shown the same way: as it was, never a second copy.
        let at = crate::agents::workspaces::norm(&local::plain(&real));
        if let Some(e) = before.iter().find(|e| e.renders() != attach && crate::agents::workspaces::norm(&e.path) == at) {
            return Ok(e.clone());
        }
        let shared = file_of(&real, meta.len(), attach)?;
        tracing::info!(place = why.word(), kind = %shared.kind, kb = meta.len() / 1024, inline = shared.renders(), "Chat: a file on this PC shared");
        Ok(shared)
    })
    .await;
    match done {
        Some(Ok(f)) => Ok(f),
        Some(Err(refused)) => Err(Skipped { name, refused }),
        None => Err(Skipped { name, refused: local::Refused { error: "Yap couldn't open that file.".into(), chip: "couldn't open it" } }),
    }
}

/// **A file as it's shared** (`real`: a path `local::resolve` gave, `bytes`
/// its size): a picture by its own bytes, at most `local::MAX_BYTES`, copied
/// into `chat-images/` (unless `attach`); else its card's facts. Reads the
/// file.
fn file_of(real: &Path, bytes: u64, attach: bool) -> Result<SharedFile, local::Refused> {
    let (name, folder, _) = local::names(real);
    let path = local::plain(real);
    let mut head = [0u8; 32];
    let n = std::fs::File::open(real).and_then(|mut f| std::io::Read::read(&mut f, &mut head)).unwrap_or(0);
    let picture = local::kind(&head[..n]).is_some();
    if picture && !attach && bytes <= local::MAX_BYTES {
        let data = local::read(real)?;
        let image = crate::snip::attach::keep(&data, "file").map_err(|e| local::Refused { error: format!("Yap couldn't read that picture ({e})."), chip: "not a picture Yap can read" })?;
        return Ok(SharedFile { name, path, folder, kind: "image".into(), bytes, id: image.id, w: image.w, h: image.h, opens: opens(real).word().into(), ..Default::default() });
    }
    let kind = if picture { "image" } else { kind_of(real) };
    let pages = (kind == "pdf").then(|| pdf_pages(real)).flatten();
    Ok(SharedFile { name, path, folder, kind: kind.into(), bytes, pages, opens: opens(real).word().into(), ..Default::default() })
}

// ---- kinds, previews, what Open does ------------------------------------------------------------

const MARKDOWN: &[&str] = &["md", "markdown", "mdown", "mkd", "mdx"];
const TEXT: &[&str] = &["txt", "text", "log", "csv", "tsv", "ini", "cfg", "conf", "properties", "srt", "vtt"];
const CODE: &[&str] = &[
    "rs", "js", "mjs", "cjs", "ts", "tsx", "jsx", "py", "rb", "go", "java", "kt", "kts", "c", "h", "cc", "cpp", "hpp", "cs", "swift", "php", "sh", "bash", "zsh", "ps1",
    "psm1", "bat", "cmd", "json", "jsonc", "yaml", "yml", "toml", "xml", "html", "htm", "css", "scss", "sass", "less", "svelte", "vue", "sql", "lua", "r", "dart",
    "gradle", "lock",
];
const IMAGE: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "ico", "tif", "tiff", "heic", "heif", "avif"];

fn ending(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

/// **A file's kind by its ending**: "markdown", "text", "code", "pdf",
/// "image" or "file". Pure.
pub fn kind_of(path: &Path) -> &'static str {
    let e = ending(path);
    let e = e.as_str();
    if MARKDOWN.contains(&e) {
        "markdown"
    } else if TEXT.contains(&e) {
        "text"
    } else if CODE.contains(&e) {
        "code"
    } else if e == "pdf" {
        "pdf"
    } else if IMAGE.contains(&e) {
        "image"
    } else {
        "file"
    }
}

/// Whether a file of `kind` gets a preview.
pub fn previews(kind: &str) -> bool {
    matches!(kind, "markdown" | "text" | "code")
}

/// **A card's preview**: the first lines of a file's text, as plain text,
/// and whether the file has more.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub text: String,
    pub cut: bool,
}

/// [`Preview`] of a file's first bytes: at most [`PREVIEW_LINES`] lines and
/// [`PREVIEW_BYTES`] bytes, `\r` left out; `None` for what isn't text (a NUL
/// byte, not UTF-8) or is empty. `more`: the file goes on past `bytes`.
/// Pure.
pub fn preview_of(bytes: &[u8], more: bool) -> Option<Preview> {
    let bytes = &bytes[..bytes.len().min(PREVIEW_BYTES)];
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    if bytes.contains(&0) {
        return None;
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        // Cut mid-character at the end: what's whole.
        Err(e) if e.error_len().is_none() => std::str::from_utf8(&bytes[..e.valid_up_to()]).ok()?,
        Err(_) => return None,
    };
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<&str> = text.split('\n').collect();
    let mut cut = more;
    if lines.len() > PREVIEW_LINES {
        lines.truncate(PREVIEW_LINES);
        cut = true;
    }
    let out = lines.join("\n");
    let out = out.trim_end().to_string();
    (!out.trim().is_empty()).then_some(Preview { text: out, cut })
}

/// [`preview_of`] a file: its first [`PREVIEW_BYTES`] read. Reads the file.
pub fn preview(path: &Path) -> Option<Preview> {
    let file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut bytes = Vec::with_capacity(PREVIEW_BYTES);
    std::io::Read::read_to_end(&mut std::io::Read::take(file, PREVIEW_BYTES as u64), &mut bytes).ok()?;
    preview_of(&bytes, len > PREVIEW_BYTES as u64)
}

/// **A PDF's page count** from its bytes, when it's easy: the largest
/// `/Count` of a `/Type /Pages` dictionary (the root's), else the `/Type
/// /Page` objects counted; `None` when neither shows (its objects
/// compressed). Pure.
pub fn pdf_pages_in(bytes: &[u8]) -> Option<u32> {
    fn skip_ws(b: &[u8], mut i: usize) -> usize {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        i
    }
    let mut best: Option<u32> = None;
    let mut pages = 0u32;
    let mut i = 0;
    while let Some(at) = bytes[i..].windows(5).position(|w| w == b"/Type").map(|p| p + i) {
        let j = skip_ws(bytes, at + 5);
        if bytes[j..].starts_with(b"/Pages") {
            // Its dictionary: from the "<<" before to the ">>" after.
            let start = bytes[..at].windows(2).rposition(|w| w == b"<<").unwrap_or(0);
            let end = bytes[j..].windows(2).position(|w| w == b">>").map_or(bytes.len(), |p| p + j);
            let dict = &bytes[start..end];
            if let Some(c) = dict.windows(6).position(|w| w == b"/Count") {
                let k = skip_ws(dict, c + 6);
                let digits: String = dict[k..].iter().take_while(|b| b.is_ascii_digit()).map(|&b| b as char).collect();
                if let Ok(n) = digits.parse::<u32>() {
                    best = Some(best.map_or(n, |b| b.max(n)));
                }
            }
        } else if bytes[j..].starts_with(b"/Page") && !bytes.get(j + 5).is_some_and(|c| c.is_ascii_alphanumeric()) {
            pages += 1;
        }
        i = at + 5;
    }
    best.filter(|n| *n > 0).or((pages > 0).then_some(pages))
}

/// [`pdf_pages_in`] a file: all of it up to [`PDF_READ`] twice, else its
/// head and its tail. Reads the file.
pub fn pdf_pages(path: &Path) -> Option<u32> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut bytes = Vec::new();
    if len <= 2 * PDF_READ {
        file.read_to_end(&mut bytes).ok()?;
    } else {
        (&mut file).take(PDF_READ).read_to_end(&mut bytes).ok()?;
        file.seek(SeekFrom::Start(len - PDF_READ)).ok()?;
        file.take(PDF_READ).read_to_end(&mut bytes).ok()?;
    }
    if !bytes.starts_with(b"%PDF") {
        return None;
    }
    pdf_pages_in(&bytes)
}

/// The endings that run a program, a script or a web page when opened on
/// Windows (its own list of risky types, and the scripting and web ones):
/// never opened from a card — Show in folder instead.
const RUNS: &[&str] = &[
    "exe", "com", "bat", "cmd", "ps1", "psm1", "psd1", "ps1xml", "ps2", "ps2xml", "psc1", "psc2", "vbs", "vbe", "vb", "js", "jse", "wsf", "wsh", "ws", "wsc", "msi", "msp", "mst",
    "msc", "msh", "msh1", "msh2", "mshxml", "scr", "pif", "lnk", "url", "hta", "cpl", "jar", "reg", "inf", "sys", "dll", "ocx", "chm", "hlp", "scf", "sct", "shb", "shs", "gadget",
    "application", "appref-ms", "appx", "appxbundle", "msix", "msixbundle", "appinstaller", "library-ms", "search-ms", "searchconnector-ms", "settingcontent-ms",
    "diagcab", "py", "pyw", "pyc", "pyo", "pyz", "pyzw", "rb", "pl", "sh", "bash", "ade", "adp", "app", "bas", "crt", "cer", "der", "csh", "ksh", "fxp", "hpj", "ins",
    "isp", "its", "mda", "mdb", "mde", "mdt", "mdw", "mdz", "ops", "pcd", "plg", "prf", "prg", "pst", "tmp", "vsmacros", "vsw", "xnk", "iqy", "slk", "xll", "xlam",
    "html", "htm", "xhtml", "mht", "mhtml", "svg", "svgz", "xml", "xsl", "xslt",
];

/// **What Open does for a file**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opens {
    /// A Markdown file: Yap's own window (`notes/md_file.rs`).
    Yap,
    /// The person's default app for it.
    App,
    /// Never: it would run a program or a script, or a web page.
    No,
}

impl Opens {
    pub fn word(self) -> &'static str {
        match self {
            Opens::Yap => "yap",
            Opens::App => "app",
            Opens::No => "",
        }
    }
}

/// What Open does for `path`, by its ending. Pure.
pub fn opens(path: &Path) -> Opens {
    if crate::notes::md_file::args::is_markdown(path) {
        return Opens::Yap;
    }
    let e = ending(path);
    // A double ending ("report.pdf.exe") goes by its last; a name ending in
    // a dot or a space is Windows' to tidy, so the last real one.
    let name = path.file_name().map(|n| n.to_string_lossy().trim_end_matches(['.', ' ']).to_ascii_lowercase()).unwrap_or_default();
    let last = name.rsplit_once('.').map(|(_, e)| e.to_string()).unwrap_or(e);
    if RUNS.contains(&last.as_str()) {
        Opens::No
    } else {
        Opens::App
    }
}

// ---- the commands -------------------------------------------------------------------------------

/// The file shared in conversation `conversation` at `path` (as the page has
/// it), if one was.
fn shared_in(conversation: u64, path: &str) -> Option<SharedFile> {
    let want = crate::agents::workspaces::norm(path);
    if want.is_empty() {
        return None;
    }
    let conv = crate::chat::chats::get(conversation)?;
    conv.messages.iter().flat_map(|m| m.files.iter()).flat_map(|s| s.items.iter()).find(|i| !i.path.is_empty() && crate::agents::workspaces::norm(&i.path) == want).cloned()
}

/// The shared file the page asks about: one of the conversation's, still
/// there. Its path.
fn checked(conversation: u64, path: &str) -> Result<PathBuf, String> {
    let item = shared_in(conversation, path).ok_or_else(|| "That file wasn't shared in this chat.".to_string())?;
    let p = PathBuf::from(&item.path);
    if !p.is_file() {
        return Err("It's been moved or deleted.".into());
    }
    Ok(p)
}

/// **Open** a shared file (the person's click): a Markdown file in Yap's own
/// window, any other in the person's default app — never one that runs a
/// program or a web page ([`opens`], judged again as the file system has it
/// now). A test run keeps what it would have opened. What happened: "yap"
/// (the window's label after it) or "app".
#[tauri::command]
pub async fn chat_file_open(app: tauri::AppHandle, conversation: u64, path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let p = checked(conversation, &path)?;
        let real = std::fs::canonicalize(&p).map(|r| PathBuf::from(local::plain(&r))).unwrap_or(p.clone());
        let how = if opens(&p) == Opens::No || opens(&real) == Opens::No { Opens::No } else { opens(&p) };
        tracing::info!(kind = kind_of(&p), opens = how.word(), "Chat: a shared file opened");
        match how {
            Opens::Yap => crate::notes::md_file::open(&app, &p).map(|label| format!("yap:{label}")),
            Opens::No => Err("Yap doesn't open this kind of file from a chat: it could run a program. Show it in its folder instead.".into()),
            Opens::App if crate::shell::e2e::active() => {
                crate::shell::commands::e2e_kept(&format!("open:{}", local::plain(&p)));
                Ok("app".into())
            }
            Opens::App => tauri_plugin_opener::open_path(local::plain(&p), None::<&str>).map(|_| "app".to_string()).map_err(|e| format!("Couldn't open it: {e}")),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// **Show in folder**: Explorer at the shared file, selected (a test run
/// keeps it for the suite).
#[tauri::command]
pub async fn chat_file_reveal(conversation: u64, path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let p = checked(conversation, &path)?;
        if crate::shell::e2e::active() {
            crate::shell::commands::e2e_kept(&format!("reveal:{}", local::plain(&p)));
            return Ok(());
        }
        tauri_plugin_opener::reveal_item_in_dir(&p).map_err(|e| format!("Couldn't open the folder: {e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// **A card's preview**, read when the person unfolds it: the first lines of
/// a shared text, Markdown or code file (`None` for other kinds, or what
/// isn't text).
#[tauri::command]
pub async fn chat_file_preview(conversation: u64, path: String) -> Result<Option<Preview>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let p = checked(conversation, &path)?;
        Ok(if previews(kind_of(&p)) { preview(&p) } else { None })
    })
    .await
    .map_err(|e| e.to_string())?
}

// ---- an agent's report --------------------------------------------------------------------------

/// **What an agent's turn shows on its report card** (`chat/turn/report.rs`):
/// the pictures it created or changed (by their bytes; the first
/// [`REPORT_MAX`] copied in, the rest counted), then its other new files
/// (created this turn, by its backups' manifest) as cards — each checked as a
/// share is (`local::resolve`: an agent's file is its chat's). `None` with
/// neither. Reads the file system.
pub fn of_agent_turn(agent: &crate::agents::store::Agent, n: u32) -> Option<Share> {
    let turn = agent.as_of_turn(n);
    if turn.files.is_empty() || turn.path.trim().is_empty() {
        return None;
    }
    let folder = PathBuf::from(&turn.path);
    let created: Vec<String> = crate::agents::store::turn_backup_dir(&agent.id, n)
        .and_then(|d| crate::integrations::claude_live::backup::read_manifest(&d).ok())
        .map(|m| m.files.into_iter().filter(|e| e.state == crate::integrations::claude_live::backup::State::Created).map(|e| crate::agents::workspaces::norm(&local::plain(&e.original))).collect())
        .unwrap_or_default();
    let places = local::Context { conversation: agent.conversation, said: Vec::new() }.places();
    of_files(&folder, &turn.files, &created, &places)
}

/// [`of_agent_turn`] for `files` (relative to `folder`), `created` the new
/// ones (normalised full paths), judged by `places`.
fn of_files(folder: &Path, files: &[String], created: &[String], places: &local::Places) -> Option<Share> {
    let mut share = Share::default();
    let (mut pictures, mut cards) = (0usize, 0usize);
    for f in files {
        let path = local::clean(&folder.join(f));
        let Ok((real, _, meta)) = local::resolve(&path, places, local::Noun::File) else { continue };
        let mut head = [0u8; 32];
        let got = std::fs::File::open(&real).and_then(|mut h| std::io::Read::read(&mut h, &mut head)).unwrap_or(0);
        let picture = local::kind(&head[..got]).is_some() && meta.len() <= local::MAX_BYTES;
        if picture {
            pictures += 1;
            if pictures > REPORT_MAX {
                continue;
            }
            match file_of(&real, meta.len(), false) {
                Ok(item) => share.items.push(item),
                Err(_) => pictures -= 1,
            }
        } else if created.contains(&crate::agents::workspaces::norm(&local::plain(&path))) || created.contains(&crate::agents::workspaces::norm(&local::plain(&real))) {
            cards += 1;
            if cards > REPORT_MAX {
                continue;
            }
            if let Ok(item) = file_of(&real, meta.len(), true) {
                share.items.push(item);
            }
        }
    }
    share.more_pictures = pictures.saturating_sub(REPORT_MAX) as u32;
    share.more_files = cards.saturating_sub(REPORT_MAX) as u32;
    // Pictures first, then the cards (as they show).
    share.items.sort_by_key(|i| !i.renders());
    if share.items.is_empty() {
        return None;
    }
    tracing::info!(pictures = share.items.iter().filter(|i| i.renders()).count(), more = share.more_pictures + share.more_files, "Agents: a report shows the files its turn made");
    Some(share)
}

// ---- kept for an older Yap's save ---------------------------------------------------------------

/// **`chat_files.json`**: every answer's shares, by its conversation, index
/// and time — the copy an older Yap never touches.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Mirror {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    messages: Vec<Kept>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Kept {
    conversation: u64,
    index: usize,
    ts: u64,
    files: Vec<Share>,
}

/// The shares `convs` keep, as the copy has them. Pure.
fn mirror_of(convs: &[crate::chat::chats::Conversation]) -> Mirror {
    let messages = convs
        .iter()
        .flat_map(|c| c.messages.iter().enumerate().filter(|(_, m)| !m.files.is_empty()).map(move |(index, m)| Kept { conversation: c.id, index, ts: m.ts, files: m.files.clone() }))
        .collect();
    Mirror { version: 1, messages }
}

/// **Put back** the shares an older Yap's save dropped: a message at the
/// copy's place, at its time, with none. How many. Pure.
fn restore_from(convs: &mut [crate::chat::chats::Conversation], mirror: &Mirror) -> usize {
    let mut put = 0;
    for k in &mirror.messages {
        let Some(c) = convs.iter_mut().find(|c| c.id == k.conversation) else { continue };
        let at = match c.messages.get(k.index) {
            Some(m) if m.ts == k.ts => Some(k.index),
            _ => c.messages.iter().position(|m| m.ts == k.ts && m.role == "assistant" && m.files.is_empty()),
        };
        if let Some(m) = at.and_then(|i| c.messages.get_mut(i)) {
            if m.files.is_empty() {
                m.files = k.files.clone();
                put += 1;
            }
        }
    }
    put
}

#[cfg(not(test))]
fn mirror_path() -> PathBuf {
    crate::config::data_dir().join("chat_files.json")
}

/// What was written last (a save that changes no share writes nothing).
#[cfg(not(test))]
static LAST: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// **The copy brought up to date** with chats.json's save (`chats.rs`
/// `save_to_disk`): written when the shares changed, removed when there are
/// none.
#[cfg(not(test))]
pub fn mirror_to_disk(convs: &[crate::chat::chats::Conversation]) {
    let mirror = mirror_of(convs);
    let json = if mirror.messages.is_empty() { String::new() } else { serde_json::to_string(&mirror).unwrap_or_default() };
    let mut last = LAST.lock().unwrap_or_else(|p| p.into_inner());
    if last.as_deref() == Some(json.as_str()) {
        return;
    }
    let path = mirror_path();
    let done = if json.is_empty() {
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    } else {
        crate::config::atomic_write(&path, &json)
    };
    match done {
        Ok(()) => *last = Some(json),
        Err(e) => tracing::warn!(kind = ?e.kind(), "Chat: the shared files' copy couldn't be saved; tried again with the next save"),
    }
}

/// **At load** (`chats.rs` `load_from_disk`): the shares an older Yap's save
/// dropped, put back from the copy.
#[cfg(not(test))]
pub fn restore_from_disk(convs: &mut [crate::chat::chats::Conversation]) {
    let Ok(raw) = std::fs::read_to_string(mirror_path()) else { return };
    let Ok(mirror) = serde_json::from_str::<Mirror>(&raw) else {
        tracing::warn!("Chat: the shared files' copy couldn't be read; left as it is");
        return;
    };
    let put = restore_from(convs, &mirror);
    if put > 0 {
        tracing::info!(answers = put, "Chat: shared files an older Yap's save dropped, put back");
    }
}

#[cfg(test)]
mod tests;

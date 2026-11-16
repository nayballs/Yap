//! **Files attached to a question** (15 Nov 2026, the Claude app's + and its
//! drop): beside the pictures Chat already takes (`snip/attach.rs`), any file
//! whose text Yap can read goes with the question as text.
//!
//! - **Which files** ([`read`]): text and code (by their bytes: UTF-8, or
//!   UTF-16 with its mark; no NUL bytes), Markdown, a PDF's text read on this
//!   PC (`pdf.rs`, never uploaded to be read; a scanned page read with
//!   Windows' OCR, `scan.rs`, the chip saying so), a Word document's text
//!   (`word.rs`; an older `.doc`'s, `doc.rs`), a PowerPoint presentation's
//!   slides and speaker notes (`powerpoint.rs`), a spreadsheet's cells as
//!   tables (`sheet.rs`: `.xlsx`, `.xlsm`, `.xls`, `.xlsb`, `.ods`, `.csv`,
//!   `.tsv`, read and calculated here, cut to the box's room). Anything else
//!   — a zip, a program, an older `.ppt` — is said plainly ("Chat can't read
//!   archive.zip…"), never added. At most [`MAX_BYTES`] a file and
//!   [`MAX_CHARS`] of text; [`MAX_FILES`] a question.
//! - **Kept** like Chat's screenshots: the file's copy and its text in
//!   `<data>/chat-files/` (`<id>/<name>`, `<id>.txt`); the question keeps
//!   its [`Attachment`]s (`ChatMessage::attachments`: ids, names, sizes,
//!   tokens — never the text). Deleted with its chat (`chats::delete_many`,
//!   a Rewind: unless a fork still shows them), by "Keep chat files"
//!   (`chat_file_days`: 7 / 30 / 90 days / forever, [`sweep`]), and a file
//!   never sent after a day. An older Yap's save drops the field: every save
//!   copies it into `chat_attachments.json` too, put back at the next start
//!   (as `shares.rs` does for shared files).
//! - **Sized to the model**: each file's tokens ([`crate::chat::budget::estimate`])
//!   count in the question's size (`turn/history.rs`, the page's
//!   `inputTooLong`): one too big for the model is said before it's sent, as
//!   a paste is. An earlier question's files count in the history
//!   (`budget::message_costs`).
//! - **To the model** ([`block`]): each file's text in a `<documents>`
//!   block before the question (Anthropic's advice for long documents: first,
//!   each its own `<document>` with its `<source>`), through the turn's
//!   task-local ([`with`], [`attach`]) — the tool loop, a plain answer, your
//!   Claude plan — and to a Code chat's own session with the words
//!   ([`with_text`]). In the history the question carries them again
//!   (`ChatMessage::sent_text`); a file past "Keep chat files" is a line.
//! - **Open** ([`chat_attachment_open`]): Markdown in Yap's own window, other
//!   files in the person's default app; one that could run a program or a
//!   web page (an `.html`, a script) shows its text instead.
//!
//! **Privacy**: never logged (kinds, counts, sizes and tokens only); a file
//! leaves the PC only inside the Chat request it's sent with (and that
//! chat's follow-ups); nothing reads it but Chat.

pub mod doc;
pub mod pdf;
pub mod powerpoint;
pub mod scan;
pub mod sheet;
pub mod word;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The folder under Yap's data folder.
pub const FOLDER: &str = "chat-files";
/// A file's most bytes.
pub const MAX_BYTES: u64 = 50 * 1024 * 1024;
/// A file's most characters of text (about a million tokens: the biggest
/// context there is).
pub const MAX_CHARS: usize = 4_000_000;
/// Files one question carries, at most.
pub const MAX_FILES: usize = 20;
/// The retention choices, in days (0 = forever), as the screenshots'.
pub const KEEP_CHOICES: [u32; 4] = [7, 30, 90, 0];
/// The default: 30 days.
pub const KEEP_DEFAULT: u32 = 30;
/// A file no question carries goes after this long.
const ORPHAN_SECS: u64 = 24 * 60 * 60;

/// **One file a question carries**: what chats.json and the page keep (the
/// same keys in JS). Never its text.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    /// `file_` + 16 hex digits ([`is_id`]).
    pub id: String,
    /// Its name ("seed-order.pdf").
    pub name: String,
    /// "text" | "code" | "markdown" | "pdf" | "word" | "powerpoint" | "sheet".
    pub kind: String,
    /// The file's size.
    #[serde(default)]
    pub bytes: u64,
    /// Its text's tokens, estimated (`budget::estimate`).
    #[serde(default)]
    pub tokens: u64,
    /// A PDF's pages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pages: Option<u32>,
    /// A PDF's pages read with OCR (scans: no text in them).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ocr_pages: Option<u32>,
    /// A PDF's pages with no text that weren't read (past OCR's limit, or
    /// too slow): named in a note at the end of its text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unread_pages: Option<u32>,
    /// The page's copy only (`for_page`): past "Keep chat files", removed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub gone: bool,
}

// ---- the folder ---------------------------------------------------------------------------------

/// Where the files live.
pub fn dir() -> PathBuf {
    #[cfg(test)]
    {
        TEST_DIR.with(|d| d.clone())
    }
    #[cfg(not(test))]
    {
        crate::config::data_dir().join(FOLDER)
    }
}

#[cfg(test)]
thread_local! {
    static TEST_DIR: PathBuf = {
        let mut raw = [0u8; 8];
        let _ = getrandom::fill(&mut raw);
        let name: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        std::env::temp_dir().join(format!("yap-files-test-{name}")).join(FOLDER)
    };
}

/// A new id: `file_` + 16 random hex digits.
pub fn new_id() -> String {
    let mut raw = [0u8; 8];
    if getrandom::fill(&mut raw).is_err() {
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        raw = (n as u64).to_le_bytes();
    }
    format!("file_{}", raw.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

/// Whether `id` is a file id (and nothing else: no path can hide in one).
pub fn is_id(id: &str) -> bool {
    id.len() == 21 && id.starts_with("file_") && id[5..].bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// A file's text, when `id` is an id.
fn text_path(id: &str) -> Option<PathBuf> {
    is_id(id).then(|| dir().join(format!("{id}.txt")))
}

/// A name safe to keep as a file's: Windows' reserved characters made `_`,
/// its folder parts dropped, never empty or a dot alone. Pure.
pub fn safe_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let mut out: String = base.chars().map(|c| if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') { '_' } else { c }).collect();
    out = out.trim_end_matches(['.', ' ']).trim_start().to_string();
    if out.chars().count() > 120 {
        let (stem, ext) = out.rsplit_once('.').map(|(s, e)| (s.to_string(), format!(".{e}"))).unwrap_or((out.clone(), String::new()));
        out = format!("{}{}", stem.chars().take(120usize.saturating_sub(ext.chars().count())).collect::<String>(), ext);
    }
    if out.is_empty() || out.chars().all(|c| c == '.') {
        "file".to_string()
    } else {
        out
    }
}

/// The file's copy, as kept.
fn copy_path(a: &Attachment) -> Option<PathBuf> {
    is_id(&a.id).then(|| dir().join(&a.id).join(safe_name(&a.name)))
}

/// **The kept copy of attachment `id`** (waiting in the box, or carried by
/// a question), when it's still there: the sheet view's (`sheets/view.rs`).
pub fn copy_of(id: &str) -> Option<(Attachment, PathBuf)> {
    let a = find(id).filter(|_| is_id(id))?;
    let path = copy_path(&a).filter(|p| p.is_file())?;
    Some((a, path))
}

/// Whether a file's text is still there.
pub fn exists(id: &str) -> bool {
    text_path(id).is_some_and(|p| p.is_file())
}

/// A file's text, as sent. Reads it.
pub fn text(id: &str) -> Option<String> {
    std::fs::read_to_string(text_path(id)?).ok()
}

/// Delete files by id: their text and their copy. How many went.
pub fn delete(ids: &[String]) -> usize {
    let mut gone = 0;
    for id in ids.iter().filter(|i| is_id(i)) {
        let text = text_path(id).is_some_and(|p| std::fs::remove_file(p).is_ok());
        let copy = std::fs::remove_dir_all(dir().join(id)).is_ok();
        if text || copy {
            gone += 1;
        }
        added().remove(id);
    }
    if gone > 0 {
        tracing::debug!(files = gone, "Chat: attached files deleted");
    }
    gone
}

// ---- reading a file -----------------------------------------------------------------------------

/// A file's text, read: its kind, the text, a PDF's pages (and how many of
/// them were read with OCR, or left unread).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Read {
    pub kind: &'static str,
    pub text: String,
    pub pages: Option<u32>,
    pub ocr_pages: Option<u32>,
    pub unread_pages: Option<u32>,
}

impl Read {
    fn of(kind: &'static str, text: String) -> Read {
        Read { kind, text, ..Read::default() }
    }
}

/// **The text of a file** called `name` with `bytes`, or why Chat can't
/// read it (a plain sentence, the file named). A PDF, its scanned pages and
/// an older `.doc` are read in their own threads (`pdf::text`,
/// `scan::read`, [`old_word`]): blocking.
pub fn read(name: &str, bytes: Vec<u8>) -> Result<Read, String> {
    let ending = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if bytes.is_empty() {
        return Err(format!("{name} is empty."));
    }
    let couldnt = || format!("Yap couldn't read the text of {name}.");
    let read = match ending.as_str() {
        _ if crate::chat::sheets::Format::of(name).is_some() => Read::of("sheet", sheet::text(name, bytes)?),
        "pdf" => match pdf::text(bytes) {
            Ok(t) => Read { kind: "pdf", text: t.text, pages: Some(t.pages), ocr_pages: (t.ocr > 0).then_some(t.ocr), unread_pages: (t.unread > 0).then_some(t.unread) },
            Err(pdf::Why::Locked) => return Err(format!("{name} is password-protected, so Yap can't read it.")),
            Err(pdf::Why::NoText) => return Err(format!("{name} has no text Yap can read: its pages are pictures, and Windows' text recognition found no words on them.")),
            Err(pdf::Why::NoOcr) => return Err(format!("{name} has no text Yap can read: its pages are pictures (a scan), and Windows' text recognition isn't set up on this PC (add a language in Settings → Time & language). Snip a page to Chat instead.")),
            Err(pdf::Why::Failed) => return Err(couldnt()),
        },
        "docx" | "docm" | "dotx" => match word::text(&bytes) {
            Some(text) => Read::of("word", text),
            None => return Err(couldnt()),
        },
        "pptx" | "pptm" | "ppsx" | "potx" => match powerpoint::text(&bytes) {
            Some(text) => Read::of("powerpoint", text),
            None => return Err(couldnt()),
        },
        "doc" | "dot" => old_word(name, bytes)?,
        // (No small, safe reader of the older binary presentations: said.)
        "ppt" | "pps" | "pot" => return Err(cant_read(name, &ending)),
        _ => {
            let kind = match crate::chat::shares::kind_of(Path::new(name)) {
                "markdown" => "markdown",
                "code" => "code",
                "image" => return Err(format!("{name} is a picture: drop or paste it on Chat to ask about it.")),
                _ => "text",
            };
            match decode(&bytes) {
                Some(text) => Read::of(kind, text),
                None => return Err(cant_read(name, &ending)),
            }
        }
    };
    if read.text.trim().is_empty() {
        return Err(match read.kind {
            "word" | "powerpoint" => format!("{name} has no text in it."),
            _ => format!("{name} is empty."),
        });
    }
    let chars = read.text.chars().count();
    if chars > MAX_CHARS {
        let tokens = crate::chat::budget::estimate(&read.text);
        return Err(format!("{name} is too long to send: about {} tokens of text.", round_tokens(tokens)));
    }
    Ok(read)
}

/// **An older Word document** (`.doc`): its text through `doc.rs`, in its
/// own thread (a panic in the container's reader stays there). One that's
/// really another kind, as some apps save them — a `.docx`, or RTF or HTML
/// (text) — is read as what it is.
fn old_word(name: &str, bytes: Vec<u8>) -> Result<Read, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("doc-text".into()).spawn(move || {
        let got = std::panic::catch_unwind(|| doc::text(&bytes));
        let _ = tx.send((got, bytes));
    });
    if spawned.is_err() {
        return Err(format!("Yap couldn't read the text of {name}."));
    }
    let Ok((got, bytes)) = rx.recv_timeout(std::time::Duration::from_secs(30)) else {
        tracing::warn!("Chat: an older Word document took too long to read");
        return Err(format!("Yap couldn't read the text of {name}."));
    };
    match got {
        Ok(Ok(text)) => Ok(Read::of("word", text)),
        Ok(Err(doc::Why::Locked)) => Err(format!("{name} is password-protected, so Yap can't read it.")),
        Ok(Err(doc::Why::NotOle)) => match word::text(&bytes) {
            Some(text) => Ok(Read::of("word", text)),
            None => decode(&bytes).map(|text| Read::of("text", text)).ok_or_else(|| format!("Yap couldn't read the text of {name}.")),
        },
        Ok(Err(doc::Why::Unreadable)) => Err(format!("Yap couldn't read the text of {name} (it may be from Word 95 or older): save it as .docx in Word to attach it.")),
        Err(_) => {
            tracing::warn!("Chat: the Word document reader gave up on a file");
            Err(format!("Yap couldn't read the text of {name}."))
        }
    }
}

/// What's said for a file whose text isn't readable, worded for its kind.
/// Pure.
pub fn cant_read(name: &str, ending: &str) -> String {
    let what = match ending {
        "ppt" | "pps" | "pot" => " (an older PowerPoint presentation: save it as .pptx in PowerPoint to attach it)",
        "odp" => " (an OpenDocument presentation: save it as .pptx to attach it)",
        "key" => " (a Keynote presentation: export it as PowerPoint to attach it)",
        "odt" | "pages" => " (a document: save it as .docx to attach it)",
        "zip" | "7z" | "rar" | "tar" | "gz" => " (an archive)",
        "mp3" | "wav" | "m4a" | "flac" | "ogg" => " (audio: Upload transcribes it)",
        "mp4" | "mov" | "mkv" | "webm" | "avi" => " (a video)",
        _ => "",
    };
    format!("Chat can't read {name}{what}. It reads text, code, PDF, Word, PowerPoint and spreadsheet files, and pictures.")
}

/// **A file's `<source>` line**: its name and, for a PDF, Word document,
/// presentation or spreadsheet, what its text is (a PDF's pages, those read with OCR,
/// those left out). Chat's questions and a project's files say it alike.
/// Pure.
pub fn source(name: &str, kind: &str, pages: Option<u32>, ocr_pages: Option<u32>, unread_pages: Option<u32>) -> String {
    let s = |n: u32| if n == 1 { "" } else { "s" };
    match kind {
        "pdf" => {
            let mut what = format!("a PDF{}: its text, read on the person's PC", pages.map(|p| format!(", {p} page{}", s(p))).unwrap_or_default());
            if let Some(n) = ocr_pages.filter(|n| *n > 0) {
                what.push_str(&format!("; {n} scanned page{} read with OCR, which can get words wrong", s(n)));
            }
            if let Some(n) = unread_pages.filter(|n| *n > 0) {
                what.push_str(&format!("; {n} page{} not read, named at the end", s(n)));
            }
            format!("{name} ({what})")
        }
        "word" => format!("{name} (a Word document: its text)"),
        "powerpoint" => format!("{name} (a PowerPoint presentation: each slide's text, then its speaker notes)"),
        "sheet" => format!("{name} ({}: its cells as tables, read on the person's PC)", crate::chat::sheets::Format::of(name).map(|f| f.said()).unwrap_or("a spreadsheet")),
        _ => name.to_string(),
    }
}

/// Tokens said roughly ("12k", "1.2M", "800"). Pure.
pub fn round_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 10_000 {
        format!("{}k", n / 1000)
    } else if n >= 1000 {
        format!("{:.1}k", n as f64 / 1000.0)
    } else {
        n.to_string()
    }
}

/// **Bytes as text**, when they are text: UTF-8 (its mark dropped), UTF-16
/// with its mark; `None` for a NUL byte or many control characters early on
/// (a binary file). Anything else is read as UTF-8 with the odd character
/// replaced. Line ends made `\n`. Pure.
pub fn decode(bytes: &[u8]) -> Option<String> {
    let text = if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(rest).into_owned()
    } else if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        utf16(rest, u16::from_le_bytes)?
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        utf16(rest, u16::from_be_bytes)?
    } else {
        let head = &bytes[..bytes.len().min(8192)];
        if head.contains(&0) {
            return None;
        }
        let controls = head.iter().filter(|&&b| b < 0x20 && !matches!(b, b'\t' | b'\n' | b'\r' | 0x0C)).count();
        if controls * 20 > head.len() {
            return None;
        }
        String::from_utf8_lossy(bytes).into_owned()
    };
    Some(text.replace("\r\n", "\n").replace('\r', "\n"))
}

fn utf16(bytes: &[u8], word: fn([u8; 2]) -> u16) -> Option<String> {
    let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| word(*c)).collect();
    Some(String::from_utf16_lossy(&units))
}

// ---- adding one ---------------------------------------------------------------------------------

/// Files added to Chat's box and not sent yet (by id): what `chat_send`
/// takes, and what the sweep keeps. Not saved: a restart starts empty.
static ADDED: Mutex<Option<HashMap<String, Attachment>>> = Mutex::new(None);

struct Added<'a>(std::sync::MutexGuard<'a, Option<HashMap<String, Attachment>>>);
impl Added<'_> {
    fn map(&mut self) -> &mut HashMap<String, Attachment> {
        self.0.get_or_insert_with(HashMap::new)
    }
    fn remove(&mut self, id: &str) -> Option<Attachment> {
        self.map().remove(id)
    }
}
fn added() -> Added<'static> {
    Added(ADDED.lock().unwrap_or_else(|p| p.into_inner()))
}

/// **A file added to Chat's box** from its path: read, its copy and its
/// text kept, waiting to be sent. Why not, said plainly. Blocking.
pub fn add(path: &Path) -> Result<Attachment, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "That file".into());
    let meta = std::fs::metadata(path).map_err(|_| format!("{name} isn't there any more."))?;
    if meta.is_dir() {
        return Err(format!("{name} is a folder: attach the files in it instead."));
    }
    if meta.len() > MAX_BYTES {
        return Err(format!("{name} is over {} MB.", MAX_BYTES / (1024 * 1024)));
    }
    let bytes = std::fs::read(path).map_err(|_| format!("Yap couldn't open {name}: another app may have it open."))?;
    keep(&name, bytes)
}

/// [`add`] from the file's bytes (a test, or a file read already).
pub fn keep(name: &str, bytes: Vec<u8>) -> Result<Attachment, String> {
    let size = bytes.len() as u64;
    let copy = bytes.clone();
    let read = read(name, bytes)?;
    let a = Attachment {
        id: new_id(),
        name: name.to_string(),
        kind: read.kind.to_string(),
        bytes: size,
        tokens: crate::chat::budget::estimate(&read.text),
        pages: read.pages,
        ocr_pages: read.ocr_pages,
        unread_pages: read.unread_pages,
        gone: false,
    };
    let folder = dir();
    std::fs::create_dir_all(folder.join(&a.id)).map_err(|e| format!("Yap couldn't keep {name}: {e}"))?;
    let kept = text_path(&a.id)
        .ok_or_else(|| "bad id".to_string())
        .and_then(|p| crate::config::atomic_write(&p, &read.text).map_err(|e| e.to_string()))
        .and_then(|_| copy_path(&a).ok_or_else(|| "bad id".to_string()))
        .and_then(|p| crate::config::atomic_write_bytes(&p, &copy).map_err(|e| e.to_string()));
    if let Err(e) = kept {
        delete(std::slice::from_ref(&a.id));
        return Err(format!("Yap couldn't keep {name}: {e}"));
    }
    tracing::info!(kind = a.kind, bytes = a.bytes, tokens = a.tokens, pages = a.pages, ocr_pages = a.ocr_pages, unread_pages = a.unread_pages, "Chat: a file attached");
    added().map().insert(a.id.clone(), a.clone());
    Ok(a)
}

/// The files the box sent (`chat_send`'s ids), in order: each one added and
/// still there. A missing one says so.
pub fn taken(ids: &[String]) -> Result<Vec<Attachment>, String> {
    let mut map = added();
    let mut out = Vec::with_capacity(ids.len());
    for id in ids.iter().take(MAX_FILES) {
        match map.map().get(id) {
            Some(a) if exists(id) => out.push(a.clone()),
            _ => return Err("A file you attached isn't there any more: remove it and attach it again.".into()),
        }
    }
    Ok(out)
}

// ---- the box's files, for a question asked out loud ---------------------------------------------

/// **The files waiting in Chat's box**, as the page has them (16 Nov 2026:
/// `chat_files_waiting`, said each time they change; none while Chat shows
/// a Projects page or is closed): what a question asked out loud with that
/// box in view takes, as a typed one does (`assistant::box_files_go`,
/// [`take_waiting`]). Ids only, in the box's order; the files are
/// [`ADDED`]'s. Not saved: a restart starts with an empty box.
static WAITING: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// The page event (`{taken: [id]}` or `{back: [Attachment]}`): files a
/// spoken question took out of the box, or gave back when no answer came.
pub const WAITING_EVENT: &str = "yap-chat-files";

fn waiting() -> std::sync::MutexGuard<'static, Vec<String>> {
    WAITING.lock().unwrap_or_else(|p| p.into_inner())
}

/// The box's files now: each a file's id ([`is_id`]), at most [`MAX_FILES`],
/// each once. Pure.
pub fn box_ids(ids: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(ids.len().min(MAX_FILES));
    for id in ids {
        if out.len() < MAX_FILES && is_id(&id) && !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

/// Files a question that took them gave back (`back`), first in the box
/// (they were there before anything added since), then what's there now
/// (`have`) less them. Pure.
pub fn box_with(back: &[String], have: Vec<String>) -> Vec<String> {
    box_ids(back.iter().cloned().chain(have).collect())
}

/// How many files wait in the box.
pub fn waiting_count() -> usize {
    waiting().len()
}

/// A typed question sent these (`chat_send`): out of the box at once, so a
/// spoken question in the moment before the page says so can't take them
/// too (as `documents::quote_sent` does for the quote).
pub fn sent_from_box(ids: &[String]) {
    if !ids.is_empty() {
        waiting().retain(|id| !ids.contains(id));
    }
}

/// Chat closed (its page's box goes with it): nothing waits.
pub fn box_closed() {
    waiting().clear();
}

/// **The box's files, taken for a question asked out loud**: out of the box
/// (the page's chips go: [`WAITING_EVENT`]), each still there — one gone
/// meanwhile is left out, as a screenshot the sweep removed is. Their
/// order is the box's.
pub fn take_waiting(app: &tauri::AppHandle) -> Vec<Attachment> {
    let ids = std::mem::take(&mut *waiting());
    if ids.is_empty() {
        return Vec::new();
    }
    let known: Vec<Attachment> = {
        let mut map = added();
        ids.iter().filter_map(|id| map.map().get(id).cloned()).collect()
    };
    let files: Vec<Attachment> = known.into_iter().filter(|a| exists(&a.id)).collect();
    tracing::info!(files = files.len(), tokens = tokens_of(&files), "Chat: a spoken question takes the files in the box");
    use tauri::Emitter;
    let _ = app.emit(WAITING_EVENT, json!({ "taken": ids }));
    files
}

/// A spoken question that took the box's files got no answer: back in the
/// box, first (the page's chips come back: [`WAITING_EVENT`]).
pub fn put_back_waiting(app: &tauri::AppHandle, files: Vec<Attachment>) {
    if files.is_empty() {
        return;
    }
    let back: Vec<String> = files.iter().map(|f| f.id.clone()).collect();
    {
        let mut w = waiting();
        let have = std::mem::take(&mut *w);
        *w = box_with(&back, have);
    }
    use tauri::Emitter;
    let _ = app.emit(WAITING_EVENT, json!({ "back": files }));
}

/// **The files waiting in Chat's box**, as the page has them now (each time
/// they change): a question asked out loud with the box in view takes them
/// ([`take_waiting`]). An empty list: none (or the box isn't shown).
#[tauri::command]
pub fn chat_files_waiting(ids: Vec<String>) {
    *waiting() = box_ids(ids);
}

// ---- the turn's files ---------------------------------------------------------------------------

tokio::task_local! {
    static TURN: Vec<Attachment>;
}

/// Run a Chat turn with its question's files.
pub async fn with<F: std::future::Future>(files: Vec<Attachment>, f: F) -> F::Output {
    TURN.scope(files, f).await
}

/// The files of the turn running here (none outside one).
pub fn current() -> Vec<Attachment> {
    TURN.try_with(|t| t.clone()).unwrap_or_default()
}

/// The tokens the turn's files add to its question.
pub fn current_tokens() -> u64 {
    TURN.try_with(|t| tokens_of(t)).unwrap_or(0)
}

/// What `files` cost in a request: their text and their wrapping. Pure.
pub fn tokens_of(files: &[Attachment]) -> u64 {
    if files.is_empty() {
        return 0;
    }
    files.iter().map(|a| a.tokens + 24).sum::<u64>() + 8
}

/// The line for a file whose text is gone.
pub fn removed_line(name: &str) -> String {
    format!("(A file was attached here, {name}; it was removed.)")
}

/// **The `<documents>` block** of `files` with their texts (`None`: removed:
/// a line instead). Empty for none. Pure.
pub fn block_of(files: &[(Attachment, Option<String>)]) -> String {
    if files.is_empty() {
        return String::new();
    }
    let mut out = String::from("<documents>\n");
    let mut lines = Vec::new();
    for (i, (a, text)) in files.iter().enumerate() {
        let Some(text) = text else {
            lines.push(removed_line(&a.name));
            continue;
        };
        let source = source(&a.name, &a.kind, a.pages, a.ocr_pages, a.unread_pages);
        out.push_str(&format!("<document index=\"{}\">\n<source>{source}</source>\n<document_content>\n{}\n</document_content>\n</document>\n", i + 1, text.trim_end()));
    }
    out.push_str("</documents>");
    let had_text = files.iter().any(|(_, t)| t.is_some());
    let block = if had_text { out } else { String::new() };
    [block, lines.join("\n")].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n")
}

/// [`block_of`] the files as kept now. Reads them.
pub fn block(files: &[Attachment]) -> String {
    let read: Vec<(Attachment, Option<String>)> = files.iter().map(|a| (a.clone(), text(&a.id))).collect();
    block_of(&read)
}

/// `text` after `files`' block (none: the text as it is). Reads them.
pub fn joined(files: &[Attachment], text: &str) -> String {
    let block = block(files);
    if block.is_empty() {
        text.to_string()
    } else {
        format!("{block}\n\n{text}")
    }
}

/// The question's words with the turn's files before them: what your Claude
/// plan gets. Reads them.
pub fn with_text(text: &str) -> String {
    joined(&current(), text)
}

/// The question's words with the turn's files after them: what a Code
/// chat's own session gets (its turn's title and brief start with the
/// person's words). Reads them.
pub fn after_text(text: &str) -> String {
    let block = block(&current());
    if block.is_empty() {
        text.to_string()
    } else {
        format!("{}\n\n{block}", text.trim_end())
    }
}

/// **The turn's files onto its question**: the last user message in
/// `messages`, its text after their block (a content array: a text part
/// first). Nothing outside a turn with files.
pub fn attach(messages: &mut [Value]) {
    let files = current();
    if files.is_empty() {
        return;
    }
    let block = block(&files);
    attach_block(messages, &block);
}

/// [`attach`] with the block given. Pure.
pub fn attach_block(messages: &mut [Value], block: &str) {
    if block.is_empty() {
        return;
    }
    let Some(m) = messages.iter_mut().rev().find(|m| m["role"] == "user") else { return };
    match &mut m["content"] {
        Value::String(s) => *s = format!("{block}\n\n{s}"),
        Value::Array(parts) => parts.insert(0, json!({ "type": "text", "text": block })),
        _ => {}
    }
}

// ---- kept, swept --------------------------------------------------------------------------------

/// "Keep chat files" as set, or the default for a value Settings never
/// offers. Pure.
pub fn keep_days(setting: u32) -> u32 {
    if KEEP_CHOICES.contains(&setting) {
        setting
    } else {
        KEEP_DEFAULT
    }
}

/// Whether a file goes: one no question carries after a day, one past
/// `keep_days` (0 = never). Pure.
pub fn sweep_verdict(referenced: bool, age_secs: u64, keep_days: u32) -> bool {
    if !referenced {
        return age_secs > ORPHAN_SECS;
    }
    keep_days > 0 && age_secs > keep_days as u64 * 24 * 60 * 60
}

/// Delete what's past its time in `dir` (each id's text and copy together,
/// its age the text's). How many went.
pub fn sweep_in(dir: &Path, referenced: &HashSet<String>, keep_days: u32, now: u64) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    let mut ids: HashMap<String, u64> = HashMap::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let id = name.strip_suffix(".txt").unwrap_or(&name).to_string();
        if !is_id(&id) {
            continue;
        }
        let written = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(now);
        let e = ids.entry(id).or_insert(written);
        *e = (*e).min(written);
    }
    let gone: Vec<String> = ids.into_iter().filter(|(id, written)| sweep_verdict(referenced.contains(id), now.saturating_sub(*written), keep_days)).map(|(id, _)| id).collect();
    for id in &gone {
        let _ = std::fs::remove_file(dir.join(format!("{id}.txt")));
        let _ = std::fs::remove_dir_all(dir.join(id));
    }
    gone.len()
}

/// [`sweep_in`] the folder, against every chat's files and the box's.
pub fn sweep(keep_days: u32) -> usize {
    if crate::chat::chats::unread() {
        return 0;
    }
    let keep_days = self::keep_days(keep_days);
    let mut referenced = crate::chat::chats::attachment_ids();
    referenced.extend(added().map().keys().cloned());
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let gone = sweep_in(&dir(), &referenced, keep_days, now);
    if gone > 0 {
        tracing::info!(files = gone, keep_days, "Chat: old attached files removed");
    }
    gone
}

/// The folder's files and their size (Settings shows them).
pub fn usage() -> (usize, u64) {
    let Ok(entries) = std::fs::read_dir(dir()) else { return (0, 0) };
    let mut count = 0;
    let mut bytes = 0;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !is_id(&name) {
            continue;
        }
        count += 1;
        bytes += std::fs::read_dir(entry.path()).map(|d| d.flatten().filter_map(|f| f.metadata().ok()).map(|m| m.len()).sum::<u64>()).unwrap_or(0);
    }
    (count, bytes)
}

/// The page's copy of a question's files: each one removed past "Keep chat
/// files" says so.
pub fn for_page(files: &mut [Attachment]) {
    for a in files {
        a.gone = !exists(&a.id);
    }
}

// ---- kept for an older Yap's save ---------------------------------------------------------------

/// **`chat_attachments.json`**: every question's files, by its conversation,
/// index and time — the copy an older Yap never touches.
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
    files: Vec<Attachment>,
}

/// The files `convs` keep, as the copy has them. Pure.
fn mirror_of(convs: &[crate::chat::chats::Conversation]) -> Mirror {
    let messages = convs
        .iter()
        .flat_map(|c| c.messages.iter().enumerate().filter(|(_, m)| !m.attachments.is_empty()).map(move |(index, m)| Kept { conversation: c.id, index, ts: m.ts, files: m.attachments.clone() }))
        .collect();
    Mirror { version: 1, messages }
}

/// **Put back** the files an older Yap's save dropped: a question at the
/// copy's place, at its time, with none; and a file's fields a Yap from
/// before them dropped (a PDF's `ocrPages` / `unreadPages`, 16 Nov 2026).
/// How many questions. Pure.
fn restore_from(convs: &mut [crate::chat::chats::Conversation], mirror: &Mirror) -> usize {
    let mut put = 0;
    for k in &mirror.messages {
        let Some(c) = convs.iter_mut().find(|c| c.id == k.conversation) else { continue };
        let at = match c.messages.get(k.index) {
            Some(m) if m.ts == k.ts => Some(k.index),
            _ => c.messages.iter().position(|m| m.ts == k.ts && m.role == "user" && m.attachments.is_empty()),
        };
        let Some(m) = at.and_then(|i| c.messages.get_mut(i)).filter(|m| m.role == "user") else { continue };
        if m.attachments.is_empty() {
            m.attachments = k.files.clone();
            put += 1;
            continue;
        }
        let mut filled = false;
        for a in m.attachments.iter_mut() {
            let Some(kept) = k.files.iter().find(|f| f.id == a.id) else { continue };
            if a.ocr_pages.is_none() && kept.ocr_pages.is_some() {
                a.ocr_pages = kept.ocr_pages;
                filled = true;
            }
            if a.unread_pages.is_none() && kept.unread_pages.is_some() {
                a.unread_pages = kept.unread_pages;
                filled = true;
            }
        }
        put += usize::from(filled);
    }
    put
}

#[cfg(not(test))]
fn mirror_path() -> PathBuf {
    crate::config::data_dir().join("chat_attachments.json")
}

#[cfg(not(test))]
static LAST: Mutex<Option<String>> = Mutex::new(None);

/// **The copy brought up to date** with chats.json's save: written when the
/// files changed, removed when there are none.
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
        Err(e) => tracing::warn!(kind = ?e.kind(), "Chat: the attached files' copy couldn't be saved; tried again with the next save"),
    }
}

/// **At load**: the files an older Yap's save dropped, put back.
#[cfg(not(test))]
pub fn restore_from_disk(convs: &mut [crate::chat::chats::Conversation]) {
    let Ok(raw) = std::fs::read_to_string(mirror_path()) else { return };
    let Ok(mirror) = serde_json::from_str::<Mirror>(&raw) else {
        tracing::warn!("Chat: the attached files' copy couldn't be read; left as it is");
        return;
    };
    let put = restore_from(convs, &mirror);
    if put > 0 {
        tracing::info!(questions = put, "Chat: attached files an older Yap's save dropped, put back");
    }
}

// ---- the commands -------------------------------------------------------------------------------

/// A file that wasn't added, and why (a plain sentence).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    pub name: String,
    pub why: String,
}

/// What [`chat_files_add`] did.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddResult {
    pub files: Vec<Attachment>,
    pub skipped: Vec<Skipped>,
}

/// **Files added to Chat's box** (+ → Attach files…, or dropped on Chat):
/// each read and kept, at most `room` of them (the box's room); the others
/// said, each with why. `budget`: the tokens the model has left for the
/// question, which a spreadsheet's text is cut to (`sheet::with_budget`).
#[tauri::command]
pub async fn chat_files_add(paths: Vec<String>, room: Option<usize>, budget: Option<u64>) -> Result<AddResult, String> {
    let room = room.unwrap_or(MAX_FILES).min(MAX_FILES);
    tauri::async_runtime::spawn_blocking(move || sheet::with_budget(budget, || {
        let mut out = AddResult::default();
        for p in paths {
            let path = PathBuf::from(&p);
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.clone());
            if out.files.len() >= room {
                out.skipped.push(Skipped { name, why: format!("A question carries {MAX_FILES} files at most.") });
                continue;
            }
            match add(&path) {
                Ok(a) => out.files.push(a),
                Err(why) => out.skipped.push(Skipped { name, why }),
            }
        }
        if !out.skipped.is_empty() {
            tracing::info!(added = out.files.len(), skipped = out.skipped.len(), "Chat: some files weren't attached");
        }
        out
    }))
    .await
    .map_err(|e| e.to_string())
}

/// **A file taken out of the box** (its ✕) before it was sent: gone at once.
#[tauri::command]
pub fn chat_attachment_remove(id: String) {
    if !is_id(&id) || added().remove(&id).is_none() {
        return;
    }
    // (Never one a question already carries: a fork, a failed send put back.)
    if !crate::chat::chats::attachment_ids().contains(&id) {
        delete(&[id]);
    }
}

/// The attachment `id`: waiting in the box, or carried by a question.
fn find(id: &str) -> Option<Attachment> {
    if let Some(a) = added().map().get(id) {
        return Some(a.clone());
    }
    crate::chat::chats::attachment(id)
}

/// **Open** an attached file (the person's click): Markdown in Yap's own
/// window, another in the person's default app; one that could run a program
/// or a web page shows its text instead (as a `.txt`). A test run keeps what
/// it would have opened. What happened: "yap:<label>" or "app".
#[tauri::command]
pub async fn chat_attachment_open(app: tauri::AppHandle, id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let a = find(&id).filter(|_| is_id(&id)).ok_or("That file isn't in this chat any more.")?;
        let copy = copy_path(&a).filter(|p| p.is_file());
        let text = text_path(&a.id).filter(|p| p.is_file());
        let how = copy.as_deref().map(crate::chat::shares::opens).unwrap_or(crate::chat::shares::Opens::No);
        let target = match (how, copy, text) {
            (crate::chat::shares::Opens::No, _, Some(t)) | (_, None, Some(t)) => t,
            (_, Some(c), _) => c,
            _ => return Err("That file was removed.".into()),
        };
        tracing::info!(kind = a.kind, opens = how.word(), "Chat: an attached file opened");
        if how == crate::chat::shares::Opens::Yap && target.extension().is_some_and(|e| e != "txt") {
            return crate::notes::md_file::open(&app, &target).map(|label| format!("yap:{label}"));
        }
        if crate::shell::e2e::active() {
            let shown = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            crate::shell::commands::e2e_kept(&format!("open-attachment:{shown}"));
            return Ok("app".into());
        }
        tauri_plugin_opener::open_path(target.to_string_lossy().into_owned(), None::<&str>).map(|_| "app".to_string()).map_err(|e| format!("Couldn't open it: {e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Settings' "Chat files": how many and how big, and where.
#[tauri::command]
pub fn chat_attachments_usage() -> Value {
    let (count, bytes) = usage();
    json!({ "count": count, "bytes": bytes, "folder": dir().to_string_lossy() })
}

/// Settings' Open folder (never in a test run).
#[tauri::command]
pub fn chat_attachments_open_folder() -> Result<(), String> {
    if crate::shell::e2e::active() {
        return Ok(());
    }
    let folder = dir();
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    tauri_plugin_opener::open_path(folder.to_string_lossy().into_owned(), None::<&str>).map_err(|e| e.to_string())
}

/// Settings' Delete all…: every attached file gone (the box's too); the
/// chats say each was removed.
#[tauri::command]
pub fn chat_attachments_delete_all(app: tauri::AppHandle) -> usize {
    let Ok(entries) = std::fs::read_dir(dir()) else { return 0 };
    let ids: HashSet<String> = entries.flatten().map(|e| e.file_name().to_string_lossy().trim_end_matches(".txt").to_string()).filter(|n| is_id(n)).collect();
    let ids: Vec<String> = ids.into_iter().collect();
    let gone = delete(&ids);
    added().map().clear();
    tracing::info!(files = gone, "Chat: every attached file deleted");
    use tauri::Emitter;
    let _ = app.emit("yap-chat-images-deleted", json!({ "files": gone }));
    gone
}

#[cfg(test)]
pub(crate) mod tests;

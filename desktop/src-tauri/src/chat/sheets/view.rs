//! **The sheet view beside the chat** (`SheetTab.svelte`): a workbook drawn
//! as Excel draws it, read only, live while a Code chat's Claude builds it.
//!
//! - **Which file** ([`Src`]): one in a Code chat's own workspace (by its
//!   path there: `agents/files.rs` `resolve`, the same workspace-only rules
//!   as the file view — never a path outside it, `..`, a link out), or an
//!   attached file's kept copy (by its id: `attachments::copy_of`). Never a
//!   path the page makes up. Only a spreadsheet's endings ([`super::Format`]).
//! - **Opened** ([`sheet_open`]): read and calculated once (`sheets::read`),
//!   kept here while shown ([`KEEP`] workbooks, the newest), and sent to the
//!   page without its cells: sheets, looks, widths, merges. The page asks
//!   for its rows a stretch at a time as it scrolls ([`sheet_rows`], at most
//!   [`ROWS_AT_ONCE`]), so a big sheet scrolls smoothly.
//! - **Live** ([`sheet_stamp`]): the page asks every second or so whether
//!   the file changed (its size and time, nothing read); changed, it opens
//!   it again, keeping its sheet and place. A file caught half-written fails
//!   to read: the page keeps what it shows and tries again.
//! - **Open in Excel** ([`sheet_open_app`]): the person's own spreadsheet app
//!   (Windows' default for the ending), only on that click; a test run keeps
//!   it instead.
//!
//! The log has counts and kinds, never a path, a name or a cell.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use super::{Book, Format, Row, Sheet, Style, Why};

/// Workbooks kept read while shown.
pub const KEEP: usize = 4;
/// The most rows one ask returns.
pub const ROWS_AT_ONCE: u32 = 1_000;
/// The biggest file read.
const MAX_BYTES: u64 = crate::chat::attachments::MAX_BYTES;

/// **Where a sheet shown comes from**: the page names it, Rust finds it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "from", rename_all = "camelCase")]
pub enum Src {
    /// A file in Code chat `conversation`'s workspace, by its path there.
    Workspace { conversation: u64, rel: String },
    /// An attached file (its id): its kept copy.
    Attachment { id: String },
}

/// A file's size and time: what changes when it's written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Stamp {
    pub len: u64,
    /// Its last write, in milliseconds since 1970.
    pub ms: u64,
}

impl Stamp {
    fn of(path: &std::path::Path) -> Option<Stamp> {
        let meta = std::fs::metadata(path).ok()?;
        let ms = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0);
        Some(Stamp { len: meta.len(), ms })
    }
}

/// **The file `src` names**: its path and its name, or why not (a plain sentence).
pub fn resolve(src: &Src) -> Result<(PathBuf, String), String> {
    let (path, name) = match src {
        Src::Workspace { conversation, rel } => {
            let folder = crate::agents::code_chat::folder_of(Some(*conversation)).ok_or("This chat isn't a Code chat.")?;
            let file = crate::agents::files::resolve(std::path::Path::new(&folder.path), rel).map_err(|r| r.words().to_string())?;
            let path = PathBuf::from(&file.abs);
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            (path, name)
        }
        Src::Attachment { id } => {
            let (a, path) = crate::chat::attachments::copy_of(id).ok_or("That file isn't in this chat any more.")?;
            (path, a.name)
        }
    };
    if Format::of(&name).is_none() {
        return Err(format!("{name} isn't a spreadsheet."));
    }
    Ok((path, name))
}

/// A workbook read, while it's shown.
struct Open {
    id: u64,
    path: PathBuf,
    stamp: Stamp,
    book: Arc<Book>,
}

static OPEN: Mutex<Vec<Open>> = Mutex::new(Vec::new());
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn open_list() -> std::sync::MutexGuard<'static, Vec<Open>> {
    OPEN.lock().unwrap_or_else(|p| p.into_inner())
}

/// A sheet without its cells (they come by [`sheet_rows`]).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetInfo {
    #[serde(flatten)]
    pub sheet: Sheet,
    /// Its cells with a value.
    pub values: usize,
}

/// **A workbook, opened**: what the page draws a sheet from.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Opened {
    /// The workbook as kept here (rows are asked for by it).
    pub id: u64,
    pub name: String,
    /// Its full path, as Windows writes it (Copy path).
    pub path: String,
    pub stamp: Stamp,
    pub format: Format,
    pub sheets: Vec<SheetInfo>,
    pub styles: Vec<Style>,
    pub font: String,
    pub size: f32,
    pub calculated: bool,
}

fn opened(id: u64, name: String, path: &std::path::Path, stamp: Stamp, book: &Book) -> Opened {
    Opened {
        id,
        name,
        path: crate::chat::pictures::local::plain(path),
        stamp,
        format: book.format,
        sheets: book.sheets.iter().map(|s| SheetInfo { values: s.values(), sheet: Sheet { rows: Vec::new(), ..s.clone() } }).collect(),
        styles: book.styles.clone(),
        font: book.font.clone(),
        size: book.size,
        calculated: book.calculated,
    }
}

/// **Open the workbook at `path`** (called `name`): the one kept when it
/// hasn't changed, else read again. Blocking.
pub fn open(path: PathBuf, name: String) -> Result<Opened, String> {
    let stamp = Stamp::of(&path).ok_or_else(|| format!("{name} isn't there any more."))?;
    if let Some(o) = open_list().iter().find(|o| o.path == path && o.stamp == stamp) {
        return Ok(opened(o.id, name, &path, stamp, &o.book));
    }
    if stamp.len > MAX_BYTES {
        return Err(format!("{name} is over {} MB.", MAX_BYTES / (1024 * 1024)));
    }
    let bytes = std::fs::read(&path).map_err(|_| format!("Yap couldn't open {name}: another app may have it open."))?;
    let book = super::read(&name, bytes).map_err(|why| match why {
        Why::Locked => format!("{name} is password-protected, so Yap can't show it."),
        Why::Slow => format!("{name} took too long to read."),
        Why::Failed => format!("Yap couldn't read {name}."),
    })?;
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let out = opened(id, name, &path, stamp, &book);
    let mut list = open_list();
    list.retain(|o| o.path != path);
    list.push(Open { id, path, stamp, book: Arc::new(book) });
    let extra = list.len().saturating_sub(KEEP);
    list.drain(..extra);
    tracing::info!(format = ?out.format, sheets = out.sheets.len(), cells = out.sheets.iter().map(|s| s.values).sum::<usize>(), "Chat: a spreadsheet shown");
    Ok(out)
}

/// The rows the page asks for: rows `from`..=`to` of sheet `sheet` (at most
/// [`ROWS_AT_ONCE`]), and the top-left cells of merges that reach into them
/// from above (drawn over the rows shown). Pure.
pub fn rows_of(sheet: &Sheet, from: u32, to: u32) -> Vec<Row> {
    let to = to.min(from.saturating_add(ROWS_AT_ONCE - 1));
    let start = sheet.rows.partition_point(|r| r.r < from);
    let mut out: Vec<Row> = sheet.rows[start..].iter().take_while(|r| r.r <= to).cloned().collect();
    for m in sheet.merges.iter().filter(|m| m.row < from && m.row + m.rows > from) {
        let Ok(at) = sheet.rows.binary_search_by_key(&m.row, |r| r.r) else { continue };
        let Some(cell) = sheet.rows[at].cells.iter().find(|c| c.col == m.col) else { continue };
        match out.iter_mut().find(|r| r.r == m.row) {
            Some(row) => {
                if !row.cells.iter().any(|c| c.col == m.col) {
                    row.cells.push(cell.clone());
                }
            }
            None => out.push(Row { r: m.row, cells: vec![cell.clone()] }),
        }
    }
    out.sort_by_key(|r| r.r);
    out
}

// ---- the commands -------------------------------------------------------------------------------

/// **Open a sheet for its view** (its tab opening, the file changed).
#[tauri::command]
pub async fn sheet_open(src: Src) -> Result<Opened, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (path, name) = resolve(&src)?;
        open(path, name)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// **Rows of a sheet opened** (`id`: [`sheet_open`]'s): `from`..=`to` of
/// sheet `sheet`. "gone" when it isn't kept any more (the page opens it again).
#[tauri::command]
pub fn sheet_rows(id: u64, sheet: usize, from: u32, to: u32) -> Result<Vec<Row>, String> {
    let book = open_list().iter().find(|o| o.id == id).map(|o| o.book.clone()).ok_or("gone")?;
    let s = book.sheets.get(sheet).ok_or("No such sheet.")?;
    Ok(rows_of(s, from.max(1), to))
}

/// **Has the file changed?** Its size and time (nothing read).
#[tauri::command]
pub async fn sheet_stamp(src: Src) -> Result<Stamp, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (path, name) = resolve(&src)?;
        Stamp::of(&path).ok_or_else(|| format!("{name} isn't there any more."))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// **Open in Excel**: the file in the person's own spreadsheet app (Windows'
/// default for its ending), on their click only. A test run keeps it.
#[tauri::command]
pub async fn sheet_open_app(src: Src) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (path, name) = resolve(&src)?;
        tracing::info!(format = ?Format::of(&name), "Chat: a spreadsheet opened in its app");
        if crate::shell::e2e::active() {
            crate::shell::commands::e2e_kept(&format!("open-sheet:{name}"));
            return Ok(());
        }
        tauri_plugin_opener::open_path(path.to_string_lossy().into_owned(), None::<&str>).map_err(|e| format!("Couldn't open it: {e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

//! **The sheet view beside the chat** (`SheetTab.svelte`): a workbook drawn
//! as Excel draws it, live while a Code chat's Claude builds it, and — a
//! Code chat's `.xlsx` or `.csv` — edited there (16 Nov 2026).
//!
//! - **Which file** ([`Src`]): one in a Code chat's own workspace (by its
//!   path there: `agents/files.rs` `resolve`, the same workspace-only rules
//!   as the file view — never a path outside it, `..`, a link out), or an
//!   attached file's kept copy (by its id: `attachments::copy_of`). Never a
//!   path the page makes up. Only a spreadsheet's endings ([`super::Format`]).
//! - **Opened** ([`sheet_open`]): read and calculated once (`sheets::read`),
//!   kept here while shown ([`KEEP`] workbooks, the newest; never one with
//!   changes not saved), and sent to the page without its cells: sheets,
//!   looks, widths, merges. The page asks for its rows a stretch at a time
//!   as it scrolls ([`sheet_rows`], at most [`ROWS_AT_ONCE`]), so a big
//!   sheet scrolls smoothly.
//! - **Live** ([`sheet_stamp`]): the page asks every second or so whether
//!   the file changed (its size and time, nothing read) and whether Claude
//!   is working in the chat's folder; changed, it opens it again, keeping
//!   its sheet and place. A file caught half-written fails to read: the
//!   page keeps what it shows and tries again.
//! - **Edited** ([`sheet_edit`], [`sheet_undo`], [`sheet_redo`]: `edit.rs`):
//!   only a Code chat's own file, only while no agent works in its folder
//!   (live and read only then: Claude's changes show as they land, and
//!   nobody's edits cross), the editor made at the first change from the
//!   file as it is then (the same as shown, or the page reads it again).
//!   Each change answers with the workbook again (its rows asked for anew).
//! - **Saved** ([`sheet_save`]: `save.rs`): only while the file is what was
//!   read; else nothing written and the page offers Reload
//!   ([`sheet_discard`]) or Save as a copy ([`sheet_save_copy`]: beside it,
//!   a name not taken). A workbook with changes not saved is never read
//!   over: opening it again shows them, and says when the file changed.
//! - **Open in Excel** ([`sheet_open_app`]): the person's own spreadsheet app
//!   (Windows' default for the ending), only on that click; a test run keeps
//!   it instead.
//!
//! The log has counts and kinds, never a path, a name or a cell.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use super::edit::Editor;
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
    /// A project's file (`chat/projects.rs`, its ids): its kept copy, read
    /// only — a big project's part's pill opens it at its row.
    Project { project: String, file: String },
}

/// A file's size and time: what changes when it's written.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Stamp {
    pub len: u64,
    /// Its last write, in milliseconds since 1970.
    pub ms: u64,
}

impl Stamp {
    pub fn of(path: &std::path::Path) -> Option<Stamp> {
        let meta = std::fs::metadata(path).ok()?;
        let ms = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0);
        Some(Stamp { len: meta.len(), ms })
    }
}

/// **The file `src` names**: its path, its name, and its Code chat's
/// folder (none: an attached copy), or why not (a plain sentence).
pub fn resolve(src: &Src) -> Result<(PathBuf, String, Option<String>), String> {
    let (path, name, folder) = match src {
        Src::Workspace { conversation, rel } => {
            let folder = crate::agents::code_chat::folder_of(Some(*conversation)).ok_or("This chat isn't a Code chat.")?;
            let file = crate::agents::files::resolve(std::path::Path::new(&folder.path), rel).map_err(|r| r.words().to_string())?;
            let path = PathBuf::from(&file.abs);
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            (path, name, Some(folder.path))
        }
        Src::Attachment { id } => {
            let (a, path) = crate::chat::attachments::copy_of(id).ok_or("That file isn't in this chat any more.")?;
            (path, a.name, None)
        }
        Src::Project { project, file } => {
            let (name, path) = crate::chat::projects::copy_of(project, file).ok_or("That file isn't in the project any more.")?;
            (path, name, None)
        }
    };
    if Format::of(&name).is_none() {
        return Err(format!("{name} isn't a spreadsheet."));
    }
    Ok((path, name, folder))
}

/// A workbook read, while it's shown.
struct Open {
    id: u64,
    path: PathBuf,
    name: String,
    stamp: Stamp,
    book: Arc<Book>,
    /// Why it's read only (none: it can be edited).
    note: Option<String>,
    /// Its Code chat's folder (none: an attached copy).
    folder: Option<String>,
    /// Its editor, from the first change, and a hash of the file's bytes it started from.
    editor: Option<Arc<Mutex<Editor>>>,
    hash: u64,
}

impl Open {
    /// Changes not saved (an editor busy now counts as one).
    fn dirty(&self) -> bool {
        self.editor.as_ref().is_some_and(|e| e.try_lock().map(|e| e.dirty()).unwrap_or(true))
    }
}

static OPEN: Mutex<Vec<Open>> = Mutex::new(Vec::new());
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn open_list() -> std::sync::MutexGuard<'static, Vec<Open>> {
    OPEN.lock().unwrap_or_else(|p| p.into_inner())
}

fn lock(e: &Mutex<Editor>) -> std::sync::MutexGuard<'_, Editor> {
    e.lock().unwrap_or_else(|p| p.into_inner())
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

/// **Where its editing stands.**
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditState {
    /// Why it's read only (none: it can be edited).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Changes not saved.
    pub dirty: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    /// The file changed on disk since it was read, with changes not saved here.
    pub conflict: bool,
    /// An agent works in its Code chat's folder: live and read only.
    pub working: bool,
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
    pub edit: EditState,
}

fn opened(id: u64, name: String, path: &std::path::Path, stamp: Stamp, book: &Book, edit: EditState) -> Opened {
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
        edit,
    }
}

/// Whether an agent works in `folder` now (none: an attached copy).
fn working(folder: Option<&str>) -> bool {
    folder.is_some_and(crate::agents::runner::working_in)
}

/// Where an open workbook's editing stands now.
fn state_of(o: &Open) -> EditState {
    let (dirty, can_undo, can_redo) = match &o.editor {
        Some(e) => {
            let e = lock(e);
            (e.dirty(), e.can_undo(), e.can_redo())
        }
        None => (false, false, false),
    };
    let conflict = dirty && Stamp::of(&o.path) != Some(o.stamp);
    EditState { note: o.note.clone(), dirty, can_undo, can_redo, conflict, working: working(o.folder.as_deref()) }
}

/// **Why a file read is read only** in the view, if it is.
fn note_for(name: &str, path: &std::path::Path, book: &Book, folder: Option<&str>, bytes: &[u8]) -> Option<String> {
    let read_only = std::fs::metadata(path).is_ok_and(|m| m.permissions().readonly());
    if let Some(note) = super::edit::refused(name, book.format, folder.is_some(), book.calculated, read_only) {
        return Some(note);
    }
    if matches!(book.format, Format::Csv | Format::Tsv) && crate::notes::md_file::text::decode(bytes).form.is_none() {
        return Some(format!("Yap edits CSV files written as UTF-8 or UTF-16, and {name} is in another encoding, so it's read only here."));
    }
    None
}

/// **Open the workbook at `path`** (called `name`; `folder`: its Code
/// chat's, none for an attached copy): the one kept when it hasn't changed
/// — or has changes not saved, never read over — else read again. Blocking.
pub fn open(path: PathBuf, name: String, folder: Option<String>) -> Result<Opened, String> {
    let stamp = Stamp::of(&path).ok_or_else(|| format!("{name} isn't there any more."))?;
    {
        let list = open_list();
        if let Some(o) = list.iter().find(|o| o.path == path && (o.stamp == stamp || o.dirty())) {
            return Ok(opened(o.id, name, &path, o.stamp, &o.book, state_of(o)));
        }
    }
    if stamp.len > MAX_BYTES {
        return Err(format!("{name} is over {} MB.", MAX_BYTES / (1024 * 1024)));
    }
    let bytes = std::fs::read(&path).map_err(|_| format!("Yap couldn't open {name}: another app may have it open."))?;
    let hash = super::save::hash_of(&bytes);
    let note_bytes = if Format::of(&name).is_some_and(|f| !f.is_workbook()) { Some(bytes.clone()) } else { None };
    let book = super::read(&name, bytes).map_err(|why| match why {
        Why::Locked => format!("{name} is password-protected, so Yap can't show it."),
        Why::Slow => format!("{name} took too long to read."),
        Why::Failed => format!("Yap couldn't read {name}."),
    })?;
    let note = note_for(&name, &path, &book, folder.as_deref(), note_bytes.as_deref().unwrap_or_default());
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let edit = EditState { note: note.clone(), working: working(folder.as_deref()), ..Default::default() };
    let out = opened(id, name.clone(), &path, stamp, &book, edit);
    let mut list = open_list();
    list.retain(|o| o.path != path || o.dirty());
    list.push(Open { id, path, name, stamp, book: Arc::new(book), note, folder, editor: None, hash });
    // The oldest go, but never one with changes not saved.
    while list.len() > KEEP {
        let Some(i) = list.iter().position(|o| !o.dirty()) else { break };
        list.remove(i);
    }
    tracing::info!(format = ?out.format, sheets = out.sheets.len(), cells = out.sheets.iter().map(|s| s.values).sum::<usize>(), editable = out.edit.note.is_none(), "Chat: a spreadsheet shown");
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

// ---- editing ------------------------------------------------------------------------------------

/// An open workbook's file and editor (made now, at its first change, from
/// the file as it is: the same as shown, or "changed" and the page reads it
/// again), or why not.
fn editor_of(id: u64) -> Result<(Arc<Mutex<Editor>>, Option<String>), String> {
    let (path, name, stamp, note, folder, editor) = {
        let list = open_list();
        let o = list.iter().find(|o| o.id == id).ok_or("gone")?;
        (o.path.clone(), o.name.clone(), o.stamp, o.note.clone(), o.folder.clone(), o.editor.clone())
    };
    if let Some(note) = note {
        return Err(note);
    }
    if let Some(e) = editor {
        return Ok((e, folder));
    }
    let format = Format::of(&name).ok_or("gone")?;
    let bytes = std::fs::read(&path).map_err(|_| format!("Yap couldn't open {name}: another app may have it open."))?;
    if Stamp::of(&path) != Some(stamp) {
        return Err("changed".into());
    }
    let hash = super::save::hash_of(&bytes);
    let made = std::thread::Builder::new()
        .name("sheet-edit".into())
        .stack_size(8 * 1024 * 1024)
        .spawn(move || std::panic::catch_unwind(|| Editor::open(&name, format, &bytes)).unwrap_or_else(|_| Err("Yap couldn't calculate it, so it's read only here.".into())))
        .map_err(|_| "Yap couldn't start editing it.".to_string())?
        .join()
        .unwrap_or_else(|_| Err("Yap couldn't calculate it, so it's read only here.".into()));
    let mut list = open_list();
    let o = list.iter_mut().find(|o| o.id == id).ok_or("gone")?;
    match made {
        Ok(editor) => {
            if o.editor.is_none() {
                o.editor = Some(Arc::new(Mutex::new(editor)));
                o.hash = hash;
            }
            Ok((o.editor.clone().ok_or("gone")?, o.folder.clone()))
        }
        Err(why) => {
            // Read only from now on (said with the workbook).
            o.note = Some(why.clone());
            Err(why)
        }
    }
}

/// **A change answered**: the workbook again (its rows asked for anew) and
/// the cell to select (an Undo's or a Redo's: the cell it changed).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Edited {
    pub book: Opened,
    /// `[sheet, row, column]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<[u32; 3]>,
}

/// **Put `input` in a cell** of workbook `id` (`sheet` from 0; `row`, `col`
/// from 1), as typed. Blocking.
pub fn edit_cell(id: u64, sheet: u32, row: u32, col: u32, input: &str) -> Result<Edited, String> {
    change(id, |e| e.edit(sheet, row, col, input).map(|()| None))
}

/// **Put in or take out rows or columns** of workbook `id` (`sheet` from 0):
/// `count` rows (`rows`) or columns before line `at` (`insert`), or from it
/// on. Blocking.
pub fn edit_lines(id: u64, sheet: u32, rows: bool, at: u32, count: u32, insert: bool) -> Result<Edited, String> {
    let axis = if rows { super::shift::Axis::Rows } else { super::shift::Axis::Cols };
    change(id, |e| e.lines(sheet, axis, at, count, insert).map(|(s, r, c)| Some([s, r as u32, c as u32])))
}

/// **Undo** (`back`) or **Redo** workbook `id`'s last change. Blocking.
pub fn step(id: u64, back: bool) -> Result<Edited, String> {
    change(id, |e| Ok(if back { e.undo() } else { e.redo() }.map(|(s, r, c)| [s, r as u32, c as u32])))
}

/// Change workbook `id` with `change` (its editor's), and answer with it again.
fn change(id: u64, change: impl FnOnce(&mut Editor) -> Result<Option<[u32; 3]>, String>) -> Result<Edited, String> {
    let (editor, folder) = editor_of(id)?;
    if working(folder.as_deref()) {
        return Err("Claude is working in this chat's folder: the sheet is read only until it's done.".into());
    }
    let (at, book) = {
        let mut e = lock(&editor);
        let at = change(&mut e)?;
        (at, Arc::new(e.book()))
    };
    let mut list = open_list();
    let o = list.iter_mut().find(|o| o.id == id).ok_or("gone")?;
    o.book = book;
    let state = state_of(o);
    Ok(Edited { book: opened(o.id, o.name.clone(), &o.path, o.stamp, &o.book, state), at })
}

/// What a save did.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveOutcome {
    /// Written (or nothing to write).
    pub saved: bool,
    /// Not written: the file changed on disk since it was read.
    pub conflict: bool,
    /// The file's stamp now.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stamp: Option<Stamp>,
    pub edit: EditState,
}

/// The file's bytes for an editor's changes (an `.xlsx`'s with its styles' mapping).
fn bytes_of(e: &Editor) -> Result<(Vec<u8>, Option<super::save::Written>), String> {
    match e {
        Editor::Xlsx(x) => super::save::xlsx_bytes(x).map(|w| (w.bytes.clone(), Some(w))),
        Editor::Delimited(d) => Ok((d.bytes(), None)),
    }
}

/// **Save workbook `id`** back to its file, only while the file is what was read. Blocking.
pub fn save(id: u64) -> Result<SaveOutcome, String> {
    let (path, name, stamp, hash, editor) = {
        let list = open_list();
        let o = list.iter().find(|o| o.id == id).ok_or("gone")?;
        (o.path.clone(), o.name.clone(), o.stamp, o.hash, o.editor.clone())
    };
    let Some(editor) = editor else {
        return Ok(SaveOutcome { saved: true, conflict: false, stamp: Some(stamp), edit: EditState::default() });
    };
    let mut e = lock(&editor);
    if !e.dirty() {
        drop(e);
        let list = open_list();
        let state = list.iter().find(|o| o.id == id).map(state_of).unwrap_or_default();
        return Ok(SaveOutcome { saved: true, conflict: false, stamp: Some(stamp), edit: state });
    }
    if !super::save::unchanged(&path, stamp, hash) {
        tracing::info!("Chat: a sheet's save held back: its file changed on disk");
        let (can_undo, can_redo) = (e.can_undo(), e.can_redo());
        return Ok(SaveOutcome { saved: false, conflict: true, stamp: Stamp::of(&path), edit: EditState { dirty: true, can_undo, can_redo, conflict: true, ..Default::default() } });
    }
    let format = Format::of(&name);
    let (bytes, mapping) = bytes_of(&e).map_err(|why| format!("Yap couldn't save {name} safely ({why}), so nothing was written."))?;
    super::save::write(&path, &name, &bytes)?;
    let now = Stamp::of(&path).unwrap_or_default();
    let written = super::save::hash_of(&bytes);
    match (&mut *e, mapping) {
        (Editor::Xlsx(x), Some(w)) => x.saved(w),
        (Editor::Delimited(d), _) => d.saved = d.text.clone(),
        _ => {}
    }
    let (can_undo, can_redo) = (e.can_undo(), e.can_redo());
    drop(e);
    let mut list = open_list();
    let state = match list.iter_mut().find(|o| o.id == id) {
        Some(o) => {
            o.stamp = now;
            o.hash = written;
            state_of(o)
        }
        None => EditState { can_undo, can_redo, ..Default::default() },
    };
    tracing::info!(format = ?format, "Chat: a sheet saved back to its file");
    Ok(SaveOutcome { saved: true, conflict: false, stamp: Some(now), edit: state })
}

/// **Save workbook `id`'s changes as a copy** beside it (a name not taken):
/// the copy's name. The original is left as it is on disk, and read again
/// next time it's shown. Blocking.
pub fn save_copy(id: u64) -> Result<String, String> {
    let (path, name, editor) = {
        let list = open_list();
        let o = list.iter().find(|o| o.id == id).ok_or("gone")?;
        (o.path.clone(), o.name.clone(), o.editor.clone())
    };
    let editor = editor.ok_or("There are no changes to save.")?;
    let (bytes, _) = bytes_of(&lock(&editor)).map_err(|why| format!("Yap couldn't save a copy of {name} safely ({why})."))?;
    let folder = path.parent().ok_or("gone")?;
    let copy = super::save::copy_name(folder, &name);
    super::save::write(&folder.join(&copy), &copy, &bytes)?;
    open_list().retain(|o| o.id != id);
    tracing::info!(format = ?Format::of(&name), "Chat: a sheet's changes saved as a copy");
    Ok(copy)
}

// ---- the commands -------------------------------------------------------------------------------

/// **Open a sheet for its view** (its tab opening, the file changed).
#[tauri::command]
pub async fn sheet_open(src: Src) -> Result<Opened, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (path, name, folder) = resolve(&src)?;
        let mut opened = open(path, name, folder)?;
        // (A project's copy is read only too, said as the project's.)
        if matches!(src, Src::Project { .. }) {
            opened.edit.note = Some("This is the project's copy of the file, so it's read only here.".into());
        }
        Ok(opened)
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

/// What the page's look at the file finds: its stamp, and whether an
/// agent works in its Code chat's folder.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Looked {
    #[serde(flatten)]
    pub stamp: Stamp,
    pub working: bool,
}

/// **Has the file changed?** Its size and time (nothing read), and whether
/// Claude works in the chat's folder now.
#[tauri::command]
pub async fn sheet_stamp(src: Src) -> Result<Looked, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (path, name, folder) = resolve(&src)?;
        let stamp = Stamp::of(&path).ok_or_else(|| format!("{name} isn't there any more."))?;
        Ok(Looked { stamp, working: working(folder.as_deref()) })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// **Put `input` in a cell** of workbook `id` (`sheet` from 0; `row`, `col`
/// from 1), as typed: a value, or a formula with `=`. Calculated at once.
#[tauri::command]
pub async fn sheet_edit(id: u64, sheet: u32, row: u32, col: u32, input: String) -> Result<Edited, String> {
    tauri::async_runtime::spawn_blocking(move || edit_cell(id, sheet, row, col, &input)).await.map_err(|e| e.to_string())?
}

/// **Insert or delete rows or columns** of workbook `id` (`sheet` from 0):
/// `count` rows (`rows`) or columns before line `at` (`insert`), or from it
/// on (a row's or a column's header's menu). Every reference moved as Excel
/// moves it, or refused with why.
#[tauri::command]
pub async fn sheet_lines(id: u64, sheet: u32, rows: bool, at: u32, count: u32, insert: bool) -> Result<Edited, String> {
    tauri::async_runtime::spawn_blocking(move || edit_lines(id, sheet, rows, at, count, insert)).await.map_err(|e| e.to_string())?
}

/// **Undo** workbook `id`'s last change.
#[tauri::command]
pub async fn sheet_undo(id: u64) -> Result<Edited, String> {
    tauri::async_runtime::spawn_blocking(move || step(id, true)).await.map_err(|e| e.to_string())?
}

/// **Redo** workbook `id`'s last change undone.
#[tauri::command]
pub async fn sheet_redo(id: u64) -> Result<Edited, String> {
    tauri::async_runtime::spawn_blocking(move || step(id, false)).await.map_err(|e| e.to_string())?
}

/// **Save** workbook `id` to its file (only while it's what was read).
#[tauri::command]
pub async fn sheet_save(id: u64) -> Result<SaveOutcome, String> {
    tauri::async_runtime::spawn_blocking(move || save(id)).await.map_err(|e| e.to_string())?
}

/// **Save as a copy**: workbook `id`'s changes in a new file beside it; its name.
#[tauri::command]
pub async fn sheet_save_copy(id: u64) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || save_copy(id)).await.map_err(|e| e.to_string())?
}

/// **Reload**: workbook `id`'s changes not saved let go (the page opens the file again).
#[tauri::command]
pub fn sheet_discard(id: u64) {
    open_list().retain(|o| o.id != id);
}

/// **Open in Excel**: the file in the person's own spreadsheet app (Windows'
/// default for its ending), on their click only. A test run keeps it.
#[tauri::command]
pub async fn sheet_open_app(src: Src) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (path, name, _) = resolve(&src)?;
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

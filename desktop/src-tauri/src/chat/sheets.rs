//! **Spreadsheets** (15 Nov 2026): a workbook read on this PC, never
//! uploaded to be read. Two things use it: Chat's attachments (its cells as
//! text for the model: `attachments/sheet.rs`) and the sheet view beside the
//! chat (`view.rs`, `SheetTab.svelte`), live while a Code chat's Claude
//! builds one.
//!
//! - **Which files** ([`Format::of`]): `.xlsx` and `.xlsm` through IronCalc
//!   (`xlsx.rs`: a spreadsheet engine that reads the styles and **calculates
//!   the formulas**, so a file a script wrote — openpyxl, exceljs, which keep
//!   no results — shows its values); if IronCalc can't read one, calamine
//!   reads its values. `.xls`, `.xlsb` and `.ods` through calamine (`xls.rs`:
//!   values and formulas as the file keeps them, no styles). `.csv` and
//!   `.tsv` (`delimited.rs`).
//! - **One shape** ([`Book`]): each sheet's cells as shown (the file's number
//!   and date formats applied), their formulas, a style each (bold, italic,
//!   colours, fills, borders, alignment), column widths and row heights in
//!   pixels as Excel draws them, merged cells, frozen panes, grid lines,
//!   hidden sheets and the sheets' tab colours.
//! - **Limits**: [`MAX_CELLS`] a workbook kept (the rows past it said:
//!   [`Sheet::cut_rows`]); read in its own thread for at most [`READ_FOR`],
//!   so a reader that panics on a strange file or a huge one is "couldn't
//!   read", never a crash or a hang.
//!
//! **Privacy**: never logged but counts and kinds (no names, no cells).

pub mod delimited;
pub mod view;
pub mod xls;
pub mod xlsx;

use std::time::Duration;

use serde::Serialize;

/// The most cells a workbook keeps (the view's and the text's).
pub const MAX_CELLS: usize = 1_000_000;
/// The longest a workbook is read for.
pub const READ_FOR: Duration = Duration::from_secs(60);
/// A column Excel draws with no width of its own (Calibri 11: 8.43 characters).
pub const COLUMN_PX: f32 = 64.0;

/// **A workbook's format**, by its file's ending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    /// `.xlsx`, `.xlsm`.
    Xlsx,
    /// `.xls`, `.xlsb` (Excel's binary formats).
    Xls,
    /// `.ods`.
    Ods,
    /// `.csv`.
    Csv,
    /// `.tsv`.
    Tsv,
}

impl Format {
    /// The format of a file called `name`; `None`: not a spreadsheet. Pure.
    pub fn of(name: &str) -> Option<Format> {
        let ending = std::path::Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        match ending.as_str() {
            "xlsx" | "xlsm" => Some(Format::Xlsx),
            "xls" | "xlsb" => Some(Format::Xls),
            "ods" => Some(Format::Ods),
            "csv" => Some(Format::Csv),
            "tsv" => Some(Format::Tsv),
            _ => None,
        }
    }

    /// What it is, said ("an Excel workbook"). Pure.
    pub fn said(self) -> &'static str {
        match self {
            Format::Xlsx | Format::Xls => "an Excel workbook",
            Format::Ods => "an OpenDocument spreadsheet",
            Format::Csv => "a CSV file",
            Format::Tsv => "a tab-separated file",
        }
    }

    /// Whether the file has sheets and styles of its own (not a delimited file).
    pub fn is_workbook(self) -> bool {
        !matches!(self, Format::Csv | Format::Tsv)
    }
}

/// What a cell holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Text,
    Number,
    Bool,
    Error,
    /// Nothing, but a style (a fill, a border).
    Empty,
}

/// **A cell**, as shown.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Cell {
    /// Its column (from 1).
    #[serde(rename = "c")]
    pub col: u32,
    /// Its value as shown (the file's number format applied).
    #[serde(rename = "t", skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(rename = "k")]
    pub kind: Kind,
    /// Its formula (`=B2*C2`), when it has one.
    #[serde(rename = "f", skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
    /// Its style ([`Book::styles`]; 0: none).
    #[serde(rename = "s", skip_serializing_if = "is_zero")]
    pub style: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// A row's cells, left to right.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Row {
    /// Its number (from 1).
    pub r: u32,
    pub cells: Vec<Cell>,
}

/// A rectangle of cells, from 1 (all 0: none).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Area {
    pub top: u32,
    pub left: u32,
    pub bottom: u32,
    pub right: u32,
}

impl Area {
    pub fn is_empty(&self) -> bool {
        self.top == 0
    }

    /// Grown to take in cell (`r`, `c`).
    pub fn take(&mut self, r: u32, c: u32) {
        if self.is_empty() {
            *self = Area { top: r, left: c, bottom: r, right: c };
            return;
        }
        self.top = self.top.min(r);
        self.left = self.left.min(c);
        self.bottom = self.bottom.max(r);
        self.right = self.right.max(c);
    }

    /// As Excel writes it ("A1:F120", "B2"; "" for none). Pure.
    pub fn a1(&self) -> String {
        if self.is_empty() {
            return String::new();
        }
        let first = format!("{}{}", column_name(self.left), self.top);
        if self.top == self.bottom && self.left == self.right {
            first
        } else {
            format!("{first}:{}{}", column_name(self.right), self.bottom)
        }
    }

    pub fn rows(&self) -> u32 {
        if self.is_empty() {
            0
        } else {
            self.bottom - self.top + 1
        }
    }

    pub fn cols(&self) -> u32 {
        if self.is_empty() {
            0
        } else {
            self.right - self.left + 1
        }
    }
}

/// A column's letters (1 → "A", 28 → "AB"). Pure.
pub fn column_name(mut c: u32) -> String {
    let mut out = Vec::new();
    while c > 0 {
        let rem = (c - 1) % 26;
        out.push(b'A' + rem as u8);
        c = (c - 1) / 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// Columns `from`..=`to` as drawn.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Cols {
    pub from: u32,
    pub to: u32,
    /// Their width in pixels (none: [`COLUMN_PX`]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub px: Option<f32>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// The style the whole columns have (a fill), when they have one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<u32>,
}

/// A row as drawn, when it isn't plain.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RowLook {
    pub r: u32,
    /// Its height in pixels (none: Excel's 20, 15 pt).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub px: Option<f32>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// The style the whole row has (a fill), when it has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<u32>,
}

/// Cells merged into one: the top-left one's, `rows` × `cols`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Merge {
    pub row: u32,
    pub col: u32,
    pub rows: u32,
    pub cols: u32,
}

/// **A sheet.**
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sheet {
    pub name: String,
    /// Hidden in the file (Excel doesn't show its tab).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    /// Its tab's colour (`#RRGGBB`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Its rows with something in them, top to bottom (the page asks for
    /// them a stretch at a time: [`view`]).
    #[serde(skip)]
    pub rows: Vec<Row>,
    /// Where its values are (cells with something in them).
    pub used: Area,
    /// Where anything is drawn: values, styled cells, merges.
    pub drawn: Area,
    pub cols: Vec<Cols>,
    pub row_looks: Vec<RowLook>,
    pub merges: Vec<Merge>,
    pub frozen_rows: u32,
    pub frozen_cols: u32,
    /// Grid lines shown (Excel's View → Gridlines).
    pub grid: bool,
    /// Rows with something in them past [`MAX_CELLS`], left out.
    #[serde(skip_serializing_if = "is_zero")]
    pub cut_rows: u32,
    /// Its cells with a formula.
    #[serde(skip_serializing_if = "is_zero")]
    pub formulas: u32,
}

impl Sheet {
    /// Its cells with a value (not only a style).
    pub fn values(&self) -> usize {
        self.rows.iter().map(|r| r.cells.iter().filter(|c| c.kind != Kind::Empty).count()).sum()
    }

    /// Column `c`'s width in pixels (the page works its own out: `sheetView.js` `gridOf`).
    #[cfg(test)]
    pub fn col_px(&self, c: u32) -> f32 {
        self.cols.iter().find(|s| s.from <= c && c <= s.to).map(|s| if s.hidden { 0.0 } else { s.px.unwrap_or(COLUMN_PX) }).unwrap_or(COLUMN_PX)
    }
}

/// An edge of a cell's border.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct Edge {
    /// "thin" | "medium" | "thick" | "double" | "dotted" | "dashed".
    pub style: &'static str,
    /// `#RRGGBB`; none: automatic (the ink).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

/// **A cell's look**, ready to draw. Colours as `#RRGGBB`; a font colour
/// of none is Excel's "Automatic" (the view's ink, dark mode included).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize)]
pub struct Style {
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub underline: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub strike: bool,
    /// Its font, when not the workbook's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// Its size in tenths of a point, when not the workbook's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    /// "left" | "center" | "right" | "justify" | "fill"; none: by its value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub h: Option<&'static str>,
    /// "top" | "center"; none: bottom.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub v: Option<&'static str>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub wrap: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<Edge>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right: Option<Edge>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bottom: Option<Edge>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left: Option<Edge>,
}

/// **A workbook**, read.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Book {
    pub format: Format,
    pub sheets: Vec<Sheet>,
    /// The cells' looks; `styles[0]` is none.
    pub styles: Vec<Style>,
    /// The workbook's font and its size in points.
    pub font: String,
    pub size: f32,
    /// Its formulas were calculated by Yap (IronCalc), not taken from the file.
    pub calculated: bool,
}

impl Book {
    /// An empty book of `format`.
    pub fn new(format: Format) -> Book {
        Book { format, sheets: Vec::new(), styles: vec![Style::default()], font: "Calibri".into(), size: 11.0, calculated: false }
    }

    /// The id of `style` in [`Book::styles`], added if new (none: 0).
    pub fn style_id(&mut self, style: Style, known: &mut std::collections::HashMap<Style, u32>) -> u32 {
        if style == Style::default() {
            return 0;
        }
        if let Some(&id) = known.get(&style) {
            return id;
        }
        let id = self.styles.len() as u32;
        known.insert(style.clone(), id);
        self.styles.push(style);
        id
    }

    /// Every sheet's cells with a value.
    pub fn values(&self) -> usize {
        self.sheets.iter().map(Sheet::values).sum()
    }
}

/// Rows of a sheet as kept: the top ones, at most `room` cells (a first row
/// bigger than that is kept whole); the rest's rows counted. `(rows kept,
/// rows left out, cells kept)`. Pure.
pub fn keep_rows(rows: Vec<Row>, room: usize) -> (Vec<Row>, u32, usize) {
    let mut kept = Vec::new();
    let mut cells = 0;
    let mut cut = 0u32;
    for row in rows {
        if cut > 0 || (!kept.is_empty() && cells + row.cells.len() > room) || (room == 0) {
            cut += 1;
            continue;
        }
        cells += row.cells.len();
        kept.push(row);
    }
    (kept, cut, cells)
}

/// The width a column needs for `texts` (a file that gives none: a CSV, an
/// `.xls`): about 7 pixels a character, between Excel's default and 400. Pure.
pub fn fitted_px<'a>(texts: impl Iterator<Item = &'a str>) -> f32 {
    let widest = texts.map(|t| t.lines().map(|l| l.chars().count()).max().unwrap_or(0)).max().unwrap_or(0);
    (widest as f32 * 7.0 + 12.0).clamp(COLUMN_PX, 400.0)
}

/// Columns fitted to their cells (a file with no widths of its own): the
/// first 500 rows' texts. Pure.
pub fn fit_columns(sheet: &mut Sheet) {
    if sheet.used.is_empty() {
        return;
    }
    let mut widest: std::collections::BTreeMap<u32, f32> = std::collections::BTreeMap::new();
    for row in sheet.rows.iter().take(500) {
        for cell in &row.cells {
            let px = fitted_px(std::iter::once(cell.text.as_str()));
            let e = widest.entry(cell.col).or_insert(COLUMN_PX);
            *e = e.max(px);
        }
    }
    sheet.cols = widest.into_iter().filter(|(_, px)| *px > COLUMN_PX).map(|(c, px)| Cols { from: c, to: c, px: Some(px.round()), hidden: false, style: None }).collect();
}

/// Why a workbook couldn't be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// It needs a password.
    Locked,
    /// Its reader gave up (the reader's own words, never shown with a file's contents).
    Failed,
    /// It took longer than [`READ_FOR`].
    Slow,
}

/// **Read the workbook** called `name` from its `bytes`, in its own thread
/// (a panic stays there; a file that takes too long is left to finish on its
/// own). Blocking.
pub fn read(name: &str, bytes: Vec<u8>) -> Result<Book, Why> {
    let Some(format) = Format::of(name) else { return Err(Why::Failed) };
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("sheet-read".into()).stack_size(8 * 1024 * 1024).spawn(move || {
        let read = std::panic::catch_unwind(move || read_now(format, &bytes));
        let _ = tx.send(read);
    });
    if spawned.is_err() {
        return Err(Why::Failed);
    }
    match rx.recv_timeout(READ_FOR) {
        Ok(Ok(Ok(book))) => {
            tracing::debug!(format = ?book.format, sheets = book.sheets.len(), cells = book.values(), "Chat: a spreadsheet read");
            Ok(book)
        }
        Ok(Ok(Err(why))) => {
            tracing::info!(format = ?format, locked = why == Why::Locked, "Chat: a spreadsheet couldn't be read");
            Err(why)
        }
        Ok(Err(_)) => {
            tracing::warn!(format = ?format, "Chat: the spreadsheet reader gave up on a file");
            Err(Why::Failed)
        }
        Err(_) => {
            tracing::warn!(secs = READ_FOR.as_secs(), "Chat: a spreadsheet took too long to read");
            Err(Why::Slow)
        }
    }
}

/// [`read`] here and now (no thread of its own): a test's.
pub fn read_now(format: Format, bytes: &[u8]) -> Result<Book, Why> {
    match format {
        Format::Xlsx => xlsx::read(bytes).or_else(|first| {
            // A file IronCalc can't take: its values, as calamine reads them.
            if first == Why::Locked {
                return Err(first);
            }
            tracing::info!("Chat: an .xlsx read for its values only");
            xls::read(bytes, Format::Xlsx).map_err(|_| first)
        }),
        Format::Xls | Format::Ods => xls::read(bytes, format),
        Format::Csv | Format::Tsv => delimited::read(bytes, format),
    }
}

/// A password-protected Office file is an OLE container ("CFB") holding an
/// encrypted package, not a zip. Pure.
pub fn is_encrypted_office(bytes: &[u8]) -> bool {
    const CFB: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    bytes.starts_with(&CFB) && windows_contains(bytes, "EncryptedPackage")
}

/// Whether `bytes` holds `word` in UTF-16 (as an OLE directory names its streams). Pure.
fn windows_contains(bytes: &[u8], word: &str) -> bool {
    let wide: Vec<u8> = word.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    bytes.windows(wide.len()).take(4 * 1024 * 1024).any(|w| w == wide.as_slice())
}

#[cfg(test)]
mod tests;

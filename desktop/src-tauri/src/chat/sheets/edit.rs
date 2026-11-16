//! **Editing a sheet beside the chat** (16 Nov 2026): a cell's value or
//! formula typed in the view (`SheetTab.svelte`), calculated on this PC,
//! with Undo and Redo; saved by `save.rs`.
//!
//! - **An `.xlsx` / `.xlsm`** ([`Xlsx`]): IronCalc's `UserModel` over the
//!   workbook as the file has it. A change is the person's input as Excel
//!   takes it (`set_user_input`: `12` a number, `=B3*C3` a formula, `12%` a
//!   per cent, `'007` a text), every formula calculated again at once, and
//!   IronCalc's own Undo and Redo. Each cell changed is remembered as the
//!   file has it ([`Was`]), so a save writes only those (`save.rs`) and the
//!   view knows when what's shown is the file again (Undo back to it).
//!   Refused, plainly: a cell under an array formula, a formula that would
//!   spill into the cells beside it, a table's heading — Yap can't write
//!   those into the file the way Excel would.
//! - **A `.csv` / `.tsv`** ([`Delimited`]): the file's own text. A change
//!   rewrites that one record (the others stay as written, quotes and all);
//!   a value is what was typed, as typed — a CSV holds no formulas, so
//!   `=A1+1` is kept as written (Excel calculates it when it opens one).
//!   Undo and Redo go back and forth through the text.
//! - **Read only** ([`refused`]): an attached file (Yap's own copy), an
//!   `.xls`, `.xlsb` or `.ods` (Yap reads their values only, so a copy would
//!   lose their looks), an `.xlsx` IronCalc couldn't read, a file Windows
//!   marks read-only, a CSV in an encoding Yap can't write back.
//!
//! Never logged but counts and kinds.

use std::collections::HashMap;

use ironcalc::base::expressions::parser::stringify::to_excel_string;
use ironcalc::base::expressions::types::CellReferenceRC;
use ironcalc::base::types::{Cell as CalcCell, FormulaValue};
use ironcalc::base::{Model, UserModel};

use super::patch::{CellPut, Put, Value};
use super::xlsx::Kept;
use super::{Book, Format};
use crate::notes::md_file::text;

/// The most an edit may be (a cell holds 32,767 characters in Excel).
pub const MAX_INPUT: usize = 32_767;
/// The most a CSV's Undo keeps, all its versions together.
const UNDO_BYTES: usize = 64 * 1024 * 1024;

/// A cell, by its sheet (from 0), row and column (from 1).
pub type At = (u32, i32, i32);

/// **What a cell holds**, compared to know whether it changed: a formula
/// by its text, never its result.
#[derive(Debug, Clone, PartialEq)]
pub enum Held {
    Empty,
    Number(u64),
    Text(String),
    Bool(bool),
    Error(String),
    Formula(String),
}

/// A cell as the file has it: what it holds and its style.
#[derive(Debug, Clone, PartialEq)]
pub struct Was {
    pub held: Held,
    pub style: i32,
}

/// **An editor** for a workbook shown.
pub enum Editor {
    Xlsx(Box<Xlsx>),
    Delimited(Delimited),
}

/// An `.xlsx`'s: IronCalc's model.
pub struct Xlsx {
    pub model: UserModel<'static>,
    /// The results the file kept, for functions IronCalc doesn't know.
    pub kept: Kept,
    /// The file's bytes as last read or saved.
    pub base: Vec<u8>,
    /// Each sheet's array formulas' areas, as the file has them (their cells refused).
    arrays: Vec<Vec<super::Area>>,
    /// The `<xf>`s the file had when read (IronCalc's styles below that are the file's own), and
    /// the styles and number formats saves added since: IronCalc's → the file's (`save.rs` `Styling`).
    pub xfs_at_open: usize,
    pub xf_of: HashMap<i32, u32>,
    pub fmt_of: HashMap<i32, i32>,
    /// Each cell changed since, as `base` has it.
    pub was: HashMap<At, Was>,
    /// The cells Undo and Redo go back to, newest last.
    undo: Vec<At>,
    redo: Vec<At>,
}

/// A CSV's (or a TSV's): its text.
pub struct Delimited {
    pub format: Format,
    /// Its text now (`\n` line breaks) and as the file has it.
    pub text: String,
    pub saved: String,
    pub form: text::Form,
    pub sep: u8,
    undo: Vec<(String, (u32, u32))>,
    redo: Vec<(String, (u32, u32))>,
}

/// **Why the view of this file is read only**, if it is: `format`, read
/// from a Code chat's workspace (`workspace`), calculated by IronCalc
/// (`calculated`: an `.xlsx` it couldn't read isn't), the file's bytes.
pub fn refused(name: &str, format: Format, workspace: bool, calculated: bool, read_only: bool) -> Option<String> {
    if !workspace {
        return Some("This is the copy attached to the chat, so it's read only.".into());
    }
    match format {
        Format::Xls => return Some(format!("Yap saves .xlsx and .csv files, so {name} is read only here: open it in Excel to change it.")),
        Format::Ods => return Some(format!("Yap saves .xlsx and .csv files, so {name} is read only here: open it in your spreadsheet app to change it.")),
        Format::Xlsx if !calculated => return Some(format!("Yap couldn't calculate {name}, so it's read only here.")),
        _ => {}
    }
    if read_only {
        return Some(format!("{name} is read-only on this PC."));
    }
    None
}

impl Editor {
    /// **An editor for the file's `bytes`**, or why not (a plain sentence).
    pub fn open(name: &str, format: Format, bytes: &[u8]) -> Result<Editor, String> {
        match format {
            Format::Xlsx => {
                // (Sheets Yap can't write into are found before anything's typed: save.rs.)
                let arrays = super::save::check_package(bytes).map_err(|why| format!("Yap can't save changes into {name} safely ({why}), so it's read only here."))?;
                // IronCalc reads the file with any missing style parts filled in; a save writes into the file's own.
                let (model, kept) = super::xlsx::model_now(&super::xlsx::complete(bytes)).map_err(|_| format!("Yap couldn't calculate {name}, so it's read only here."))?;
                let xfs_at_open = super::save::xf_count(bytes);
                Ok(Editor::Xlsx(Box::new(Xlsx { model: UserModel::from_model(model), kept, base: bytes.to_vec(), arrays, xfs_at_open, xf_of: HashMap::new(), fmt_of: HashMap::new(), was: HashMap::new(), undo: Vec::new(), redo: Vec::new() })))
            }
            Format::Csv | Format::Tsv => {
                let decoded = text::decode(bytes);
                let form = decoded.form.ok_or_else(|| format!("Yap edits CSV files written as UTF-8 or UTF-16, and {name} is in another encoding, so it's read only here."))?;
                let sep = super::delimited::separator_of(&decoded.text, format);
                Ok(Editor::Delimited(Delimited { format, saved: decoded.text.clone(), text: decoded.text, form, sep, undo: Vec::new(), redo: Vec::new() }))
            }
            Format::Xls | Format::Ods => Err(refused(name, format, true, true, false).unwrap_or_default()),
        }
    }

    /// **Put `input` in cell (`row`, `col`) of sheet `sheet`**, as typed.
    pub fn edit(&mut self, sheet: u32, row: u32, col: u32, input: &str) -> Result<(), String> {
        if input.chars().count() > MAX_INPUT {
            return Err(format!("A cell holds at most {MAX_INPUT} characters."));
        }
        if !(1..=1_048_576).contains(&row) || !(1..=16_384).contains(&col) {
            return Err("That cell is outside the sheet.".into());
        }
        match self {
            Editor::Xlsx(x) => x.edit(sheet, row as i32, col as i32, input),
            Editor::Delimited(d) => {
                if sheet != 0 {
                    return Err("No such sheet.".into());
                }
                d.edit(row, col, input)
            }
        }
    }

    /// Undo the last change: the cell it was in.
    pub fn undo(&mut self) -> Option<At> {
        match self {
            Editor::Xlsx(x) => x.undo(),
            Editor::Delimited(d) => d.undo(),
        }
    }

    /// Redo the last change undone: the cell it's in.
    pub fn redo(&mut self) -> Option<At> {
        match self {
            Editor::Xlsx(x) => x.redo(),
            Editor::Delimited(d) => d.redo(),
        }
    }

    pub fn can_undo(&self) -> bool {
        match self {
            Editor::Xlsx(x) => !x.undo.is_empty(),
            Editor::Delimited(d) => !d.undo.is_empty(),
        }
    }

    pub fn can_redo(&self) -> bool {
        match self {
            Editor::Xlsx(x) => !x.redo.is_empty(),
            Editor::Delimited(d) => !d.redo.is_empty(),
        }
    }

    /// **Whether what's shown differs from the file** (Undo back to it: not).
    pub fn dirty(&self) -> bool {
        match self {
            Editor::Xlsx(x) => x.changed().next().is_some(),
            Editor::Delimited(d) => d.text != d.saved,
        }
    }

    /// **The workbook as the view draws it now.**
    pub fn book(&self) -> Book {
        match self {
            Editor::Xlsx(x) => super::xlsx::book_of(x.model.get_model(), &x.kept),
            Editor::Delimited(d) => super::delimited::read_text(&d.text, d.format),
        }
    }
}

// ---- an .xlsx ---------------------------------------------------------------------------------

/// What cell (`row`, `col`) of `sheet` holds in `model`. Pure.
pub fn held_of(model: &Model, sheet: u32, row: i32, col: i32) -> Held {
    let Ok(ws) = model.workbook.worksheet(sheet) else { return Held::Empty };
    let Some(cell) = ws.cell(row, col) else { return Held::Empty };
    match &*cell {
        CalcCell::EmptyCell { .. } | CalcCell::SpillCell { .. } => Held::Empty,
        CalcCell::NumberCell { v, .. } => Held::Number(v.to_bits()),
        CalcCell::BooleanCell { v, .. } => Held::Bool(*v),
        CalcCell::ErrorCell { ei, .. } => Held::Error(ei.to_string()),
        CalcCell::SharedString { si, .. } => Held::Text(model.workbook.shared_strings.get(*si as usize).cloned().unwrap_or_default()),
        CalcCell::CellFormula { .. } | CalcCell::ArrayFormula { .. } => Held::Formula(formula_of(model, sheet, row, col).unwrap_or_default()),
    }
}

/// A cell's formula in Excel's own form (`_xlfn.` and all; no `=`), as a
/// file keeps it. Pure.
pub fn formula_of(model: &Model, sheet: u32, row: i32, col: i32) -> Option<String> {
    let ws = model.workbook.worksheet(sheet).ok()?;
    let index = ws.cell(row, col)?.get_formula()?;
    let (node, _) = model.parsed_formulas.get(sheet as usize)?.get(index as usize)?;
    Some(to_excel_string(node, &CellReferenceRC { sheet: ws.get_name(), row, column: col }))
}

/// A formula cell's result as a file keeps it; none for one Excel has no
/// word for, or one IronCalc doesn't know how to work out. Pure.
pub fn result_of(v: &FormulaValue) -> Option<Value> {
    use ironcalc::base::expressions::token::Error as E;
    match v {
        FormulaValue::Unevaluated => None,
        FormulaValue::Boolean(b) => Some(Value::Bool(*b)),
        FormulaValue::Number(n) => n.is_finite().then_some(Value::Number(*n)),
        FormulaValue::Text(t) => Some(Value::Text(t.clone())),
        FormulaValue::Error { ei: E::NIMPL | E::NAME | E::ERROR | E::CIRC, .. } => None,
        FormulaValue::Error { ei, .. } => Some(Value::Error(ei.to_string())),
    }
}

/// **What to write for cell (`row`, `col`) of `sheet`** as it is now. Pure.
pub fn put_of(model: &Model, sheet: u32, row: i32, col: i32) -> CellPut {
    let style = model.get_cell_style_index(sheet, row, col).unwrap_or(0).max(0) as u32;
    let put = match model.workbook.worksheet(sheet).ok().and_then(|ws| ws.cell(row, col).map(|c| c.into_owned())) {
        None | Some(CalcCell::EmptyCell { .. }) | Some(CalcCell::SpillCell { .. }) => Put::Empty,
        Some(CalcCell::NumberCell { v, .. }) => Put::Value(Value::Number(v)),
        Some(CalcCell::BooleanCell { v, .. }) => Put::Value(Value::Bool(v)),
        Some(CalcCell::ErrorCell { ei, .. }) => Put::Value(Value::Error(ei.to_string())),
        Some(CalcCell::SharedString { si, .. }) => Put::Value(Value::Text(model.workbook.shared_strings.get(si as usize).cloned().unwrap_or_default())),
        Some(CalcCell::CellFormula { v, .. }) | Some(CalcCell::ArrayFormula { v, .. }) => Put::Formula(formula_of(model, sheet, row, col).unwrap_or_default(), result_of(&v)),
    };
    CellPut { put, style }
}

impl Xlsx {
    fn model(&self) -> &Model<'_> {
        self.model.get_model()
    }

    /// The cells whose content or style differ from the file's, and what they were.
    pub fn changed(&self) -> impl Iterator<Item = (&At, &Was)> {
        self.was.iter().filter(|(at, was)| {
            let (s, r, c) = **at;
            let m = self.model();
            held_of(m, s, r, c) != was.held || m.get_cell_style_index(s, r, c).unwrap_or(0) != was.style
        })
    }

    /// Whether (`row`, `col`) is a table's heading on sheet `sheet`.
    fn table_heading(&self, sheet: u32, row: i32, col: i32) -> bool {
        let m = self.model();
        let Ok(ws) = m.workbook.worksheet(sheet) else { return false };
        m.workbook.tables.values().filter(|t| t.sheet_name == ws.name && t.header_row_count > 0).filter_map(|t| super::patch::area_of(&t.reference)).any(|a| a.top as i32 == row && a.left as i32 <= col && col <= a.right as i32)
    }

    fn edit(&mut self, sheet: u32, row: i32, col: i32, input: &str) -> Result<(), String> {
        let name = super::column_name(col as u32) + &row.to_string();
        let m = self.model();
        let ws = m.workbook.worksheet(sheet).map_err(|_| "No such sheet.".to_string())?;
        let in_file_array = self.arrays.get(sheet as usize).is_some_and(|areas| areas.iter().any(|a| a.top as i32 <= row && row <= a.bottom as i32 && a.left as i32 <= col && col <= a.right as i32));
        if in_file_array || matches!(ws.cell(row, col).as_deref(), Some(CalcCell::ArrayFormula { .. } | CalcCell::SpillCell { .. })) {
            return Err(format!("{name} is part of an array formula, and Yap can't save a change to one: change it in Excel."));
        }
        if self.table_heading(sheet, row, col) {
            return Err(format!("{name} is a table's heading, and Yap can't save a change to one: change it in Excel."));
        }
        let was = Was { held: held_of(m, sheet, row, col), style: m.get_cell_style_index(sheet, row, col).unwrap_or(0) };
        self.model.set_user_input(sheet, row, col, input).map_err(|e| plain_refusal(&name, &e))?;
        let _ = self.model.flush_send_queue();
        // (IronCalc's Redo went with that: so does this.)
        self.redo.clear();
        // A formula that spills over the cells beside it: back as it was (its Undo never redone).
        if matches!(self.model().workbook.worksheet(sheet).ok().and_then(|ws| ws.cell(row, col).map(|c| c.into_owned())), Some(CalcCell::ArrayFormula { .. })) {
            let _ = self.model.undo();
            let _ = self.model.flush_send_queue();
            return Err(format!("{name}: Yap can't save a formula that fills the cells beside it into the file yet. Type it in Excel."));
        }
        self.was.entry((sheet, row, col)).or_insert(was);
        self.undo.push((sheet, row, col));
        Ok(())
    }

    fn undo(&mut self) -> Option<At> {
        let at = self.undo.pop()?;
        let _ = self.model.undo();
        let _ = self.model.flush_send_queue();
        self.redo.push(at);
        Some(at)
    }

    fn redo(&mut self) -> Option<At> {
        let at = self.redo.pop()?;
        let _ = self.model.redo();
        let _ = self.model.flush_send_queue();
        self.undo.push(at);
        Some(at)
    }

    /// **Saved** (`written`): its bytes are the file now, so each cell changed is as it has it.
    pub fn saved(&mut self, written: super::save::Written) {
        self.base = written.bytes;
        self.xf_of = written.xf_of;
        self.fmt_of = written.fmt_of;
        let now: Vec<(At, Was)> = self
            .was
            .keys()
            .map(|&(s, r, c)| {
                let m = self.model();
                ((s, r, c), Was { held: held_of(m, s, r, c), style: m.get_cell_style_index(s, r, c).unwrap_or(0) })
            })
            .collect();
        self.was = now.into_iter().collect();
    }
}

/// IronCalc's refusal, said plainly.
fn plain_refusal(name: &str, e: &str) -> String {
    let lower = e.to_ascii_lowercase();
    if lower.contains("array") || lower.contains("spill") {
        format!("{name} is part of an array formula, and Yap can't save a change to one: change it in Excel.")
    } else {
        format!("Yap couldn't put that in {name}.")
    }
}

// ---- a .csv -----------------------------------------------------------------------------------

/// A record of a delimited text: where it starts and ends (its line break
/// left out) and its fields.
#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub start: usize,
    pub end: usize,
    pub fields: Vec<String>,
}

/// **The records of `text`** (split on `sep`), as the view reads them. Pure.
pub fn records(text: &str, sep: u8) -> Vec<Record> {
    let mut reader = super::delimited::reader_of(text, sep);
    let mut out = Vec::new();
    let mut rec = csv::StringRecord::new();
    loop {
        match reader.read_record(&mut rec) {
            Ok(true) => {
                let start = rec.position().map(|p| p.byte() as usize).unwrap_or(0);
                let mut end = (reader.position().byte() as usize).min(text.len());
                let bytes = text.as_bytes();
                while end > start && matches!(bytes[end - 1], b'\n' | b'\r') {
                    end -= 1;
                }
                out.push(Record { start, end, fields: rec.iter().map(str::to_string).collect() });
            }
            Ok(false) => break,
            // (A record the view skips is skipped here too: row numbers stay the same.)
            Err(_) => continue,
        }
    }
    out
}

/// One record as a CSV writes it: each field quoted only when it must be. Pure.
pub fn record_text(fields: &[String], sep: u8) -> String {
    let sep_ch = sep as char;
    let one = |f: &String| {
        if f.contains(sep_ch) || f.contains('"') || f.contains('\n') || f.contains('\r') {
            format!("\"{}\"", f.replace('"', "\"\""))
        } else {
            f.clone()
        }
    };
    if fields.len() == 1 && fields[0].is_empty() {
        // (A record of one empty field is written `""`: a line of nothing is no record.)
        return "\"\"".into();
    }
    fields.iter().map(one).collect::<Vec<_>>().join(&sep_ch.to_string())
}

/// **`text` with field (`row`, `col`) set to `value`**; the other records
/// as written. A row past the end is added (rows between as empty
/// records, the way Excel writes them). Pure.
pub fn with_field(text: &str, sep: u8, row: u32, col: u32, value: &str) -> String {
    let recs = records(text, sep);
    let i = row as usize - 1;
    let c = col as usize - 1;
    if let Some(rec) = recs.get(i) {
        let mut fields = rec.fields.clone();
        let had = fields.len();
        if fields.len() <= c {
            if value.is_empty() {
                return text.to_string();
            }
            fields.resize(c + 1, String::new());
        }
        fields[c] = value.to_string();
        // A value cleared at the end of a longer record: no new empty fields.
        while fields.len() > had.max(1) && fields.last().is_some_and(String::is_empty) {
            fields.pop();
        }
        return format!("{}{}{}", &text[..rec.start], record_text(&fields, sep), &text[rec.end..]);
    }
    if value.is_empty() {
        return text.to_string();
    }
    let width = recs.first().map(|r| r.fields.len()).unwrap_or(1).max(1);
    let empty = if width > 1 { (sep as char).to_string().repeat(width - 1) } else { "\"\"".to_string() };
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    for _ in recs.len()..i {
        out.push_str(&empty);
        out.push('\n');
    }
    let mut fields = vec![String::new(); c + 1];
    fields[c] = value.to_string();
    out.push_str(&record_text(&fields, sep));
    out.push('\n');
    out
}

impl Delimited {
    fn edit(&mut self, row: u32, col: u32, input: &str) -> Result<(), String> {
        let next = with_field(&self.text, self.sep, row, col, input);
        if next == self.text {
            return Ok(());
        }
        let before = std::mem::replace(&mut self.text, next);
        self.undo.push((before, (row, col)));
        self.redo.clear();
        // Undo keeps the newest versions up to its room.
        let mut room = UNDO_BYTES;
        let keep = self.undo.iter().rev().take_while(|(t, _)| room.checked_sub(t.len()).map(|r| room = r).is_some()).count();
        let drop = self.undo.len() - keep;
        self.undo.drain(..drop);
        Ok(())
    }

    fn undo(&mut self) -> Option<At> {
        let (text, at) = self.undo.pop()?;
        let now = std::mem::replace(&mut self.text, text);
        self.redo.push((now, at));
        Some((0, at.0 as i32, at.1 as i32))
    }

    fn redo(&mut self) -> Option<At> {
        let (text, at) = self.redo.pop()?;
        let now = std::mem::replace(&mut self.text, text);
        self.undo.push((now, at));
        Some((0, at.0 as i32, at.1 as i32))
    }

    /// The file's bytes for the text now, in its own encoding and line endings.
    pub fn bytes(&self) -> Vec<u8> {
        text::encode(&self.text, self.form)
    }
}

#[cfg(test)]
mod tests;

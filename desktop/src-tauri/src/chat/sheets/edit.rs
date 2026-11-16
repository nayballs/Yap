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
//!   those into the file the way Excel would. Typed in the person's Region's
//!   locale (`xlsx::locale_for`: `16/11/2026` a date in the UK). A line
//!   break typed (Alt+Enter) wraps the cell's text, as Excel does.
//! - **Rows and columns put in or taken out** ([`Editor::lines`], 16 Nov
//!   2026): the package as a save would write it now, every reference to
//!   them moved as Excel moves it (`shift.rs`, `shift/parts.rs`), read again
//!   as the workbook shown — so what's shown is what's saved — and checked
//!   against IronCalc's own Insert or Delete first: a cell that differs and
//!   nothing changes. Undo puts back the workbook as it was ([`Layer`]).
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
use super::shift::{Axis, Shift};
use super::xlsx::Kept;
use super::{Book, Format};
use crate::notes::md_file::text;

/// The most an edit may be (a cell holds 32,767 characters in Excel).
pub const MAX_INPUT: usize = 32_767;
/// The most a CSV's Undo keeps, all its versions together.
const UNDO_BYTES: usize = 64 * 1024 * 1024;
/// Rows or columns put in or out an `.xlsx`'s Undo keeps (each the workbook as it was).
const UNDO_LINES: usize = 20;
/// The most rows or columns put in or taken out at once.
pub const MAX_LINES: u32 = 1_000;

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
    /// The package the model was read from: the file's bytes as last read or
    /// saved, or — rows or columns put in or out since — the package with them
    /// (`base_hash` then isn't `file_hash`).
    pub base: Vec<u8>,
    base_hash: u64,
    file_hash: u64,
    /// Each sheet's array formulas' areas, as the file has them (their cells refused).
    arrays: Vec<Vec<super::Area>>,
    /// The `<xf>`s the file had when read (IronCalc's styles below that are the file's own), and
    /// the styles and number formats saves added since: IronCalc's → the file's (`save.rs` `Styling`).
    pub xfs_at_open: usize,
    pub xf_of: HashMap<i32, u32>,
    pub fmt_of: HashMap<i32, i32>,
    /// Each cell changed since, as `base` has it.
    pub was: HashMap<At, Was>,
    /// The locale it's read and typed in (`xlsx::locale_for`).
    pub locale: &'static str,
    /// What Undo and Redo go back over, newest last.
    undo: Vec<Step>,
    redo: Vec<Step>,
}

/// **A change Undo goes back over.**
enum Step {
    /// A cell typed in: IronCalc's own Undo, `calls` times (a line break's wrap is the second).
    Cell { at: At, calls: u8 },
    /// Rows or columns put in or taken out: the workbook on the other side of it.
    Lines { at: At, other: Box<Layer> },
}

impl Step {
    fn at(&self) -> At {
        match self {
            Step::Cell { at, .. } | Step::Lines { at, .. } => *at,
        }
    }
}

/// **A workbook as rows or columns put in or out change it**: its model and
/// the package it was read from, and what was typed into it since.
pub struct Layer {
    model: UserModel<'static>,
    kept: Kept,
    base: Vec<u8>,
    base_hash: u64,
    arrays: Vec<Vec<super::Area>>,
    xfs_at_open: usize,
    xf_of: HashMap<i32, u32>,
    fmt_of: HashMap<i32, i32>,
    was: HashMap<At, Was>,
}

impl Layer {
    /// The package `bytes` read (as `Editor::open` reads a file), or why not.
    fn open(bytes: Vec<u8>, locale: &str) -> Result<Layer, String> {
        let arrays = super::save::check_package(&bytes)?;
        let (model, kept) = super::xlsx::model_now(&super::xlsx::complete(&bytes), locale).map_err(|_| "it wouldn't read back".to_string())?;
        Ok(Layer { model: UserModel::from_model(model), kept, base_hash: super::save::hash_of(&bytes), xfs_at_open: super::save::xf_count(&bytes), base: bytes, arrays, xf_of: HashMap::new(), fmt_of: HashMap::new(), was: HashMap::new() })
    }
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
        Editor::open_in(name, format, bytes, super::xlsx::locale_for(bytes))
    }

    /// **An editor for the file's `bytes`**, what's typed read in `locale`.
    pub fn open_in(name: &str, format: Format, bytes: &[u8], locale: &'static str) -> Result<Editor, String> {
        match format {
            Format::Xlsx => {
                // (Sheets Yap can't write into are found before anything's typed: save.rs.)
                let arrays = super::save::check_package(bytes).map_err(|why| format!("Yap can't save changes into {name} safely ({why}), so it's read only here."))?;
                // IronCalc reads the file with any missing style parts filled in; a save writes into the file's own.
                let (model, kept) = super::xlsx::model_now(&super::xlsx::complete(bytes), locale).map_err(|_| format!("Yap couldn't calculate {name}, so it's read only here."))?;
                let xfs_at_open = super::save::xf_count(bytes);
                let hash = super::save::hash_of(bytes);
                Ok(Editor::Xlsx(Box::new(Xlsx {
                    model: UserModel::from_model(model),
                    kept,
                    base: bytes.to_vec(),
                    base_hash: hash,
                    file_hash: hash,
                    arrays,
                    xfs_at_open,
                    xf_of: HashMap::new(),
                    fmt_of: HashMap::new(),
                    was: HashMap::new(),
                    locale,
                    undo: Vec::new(),
                    redo: Vec::new(),
                })))
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

    /// **Put in or take out rows or columns** of sheet `sheet`: `n` of them
    /// before line `at` (`insert`), or from it on. The cell to select.
    pub fn lines(&mut self, sheet: u32, axis: Axis, at: u32, n: u32, insert: bool) -> Result<At, String> {
        let last = match axis {
            Axis::Rows => super::shift::LAST_ROW,
            Axis::Cols => super::shift::LAST_COL,
        };
        if n == 0 || n > MAX_LINES || at == 0 || at > last || (!insert && at + n - 1 > last) {
            return Err("That's outside the sheet.".into());
        }
        let shift = Shift { axis, at, n, insert };
        match self {
            Editor::Xlsx(x) => x.lines(sheet, shift),
            Editor::Delimited(d) => {
                if sheet != 0 {
                    return Err("No such sheet.".into());
                }
                d.lines(shift)
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
            Editor::Xlsx(x) => x.base_hash != x.file_hash || x.changed().next().is_some(),
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
        let one = ironcalc::base::expressions::types::Area { sheet, row, column: col, width: 1, height: 1 };
        let mut calls = 1;
        // A currency typed in the Region's money (`£12.50`): IronCalc gives it a format with the sign bare,
        // which it then can't show (`#VALUE!`, as a script's `£#,##0.00`): the sign quoted, as `xlsx::quoted_literals`.
        if let Some(quoted) = self.model().get_style_for_cell(sheet, row, col).ok().and_then(|s| super::xlsx::quoted_literals(&s.num_fmt)) {
            if self.model.update_range_style(&one, "num_fmt", &quoted).is_ok() {
                let _ = self.model.flush_send_queue();
                calls += 1;
            }
        }
        // A line break typed (Alt+Enter) in a value: its text wrapped, as Excel does.
        if input.contains('\n') && !input.starts_with('=') {
            let wraps = self.model().get_style_for_cell(sheet, row, col).is_ok_and(|s| s.alignment.as_ref().is_some_and(|a| a.wrap_text));
            if !wraps && self.model.update_range_style(&one, "alignment.wrap_text", "true").is_ok() {
                let _ = self.model.flush_send_queue();
                calls += 1;
            }
            // Its row as tall as its lines in the view (Excel's own fit: 15 points a line of an 11-point font;
            // IronCalc's row heights are 25/16 of a point). The file's row isn't given a height: Excel fits it.
            let size = self.model().get_style_for_cell(sheet, row, col).map(|s| s.font.sz).unwrap_or(11).max(1) as f64;
            let need = input.split('\n').count() as f64 * 15.0 * size / 11.0 * 1.5625;
            if self.model.get_row_height(sheet, row).is_ok_and(|h| h < need) && self.model.set_rows_height(sheet, row, row, need).is_ok() {
                let _ = self.model.flush_send_queue();
                calls += 1;
            }
        }
        self.was.entry((sheet, row, col)).or_insert(was);
        self.undo.push(Step::Cell { at: (sheet, row, col), calls });
        self.trim_undo();
        Ok(())
    }

    /// The oldest Undo let go past [`UNDO_LINES`] workbooks kept (and everything older than it).
    fn trim_undo(&mut self) {
        let lines: Vec<usize> = self.undo.iter().enumerate().filter(|(_, s)| matches!(s, Step::Lines { .. })).map(|(i, _)| i).collect();
        if lines.len() > UNDO_LINES {
            let cut = lines[lines.len() - UNDO_LINES - 1];
            self.undo.drain(..=cut);
        }
    }

    /// This workbook and `other` swapped (rows or columns undone, or redone).
    fn swap(&mut self, other: &mut Layer) {
        std::mem::swap(&mut self.model, &mut other.model);
        std::mem::swap(&mut self.kept, &mut other.kept);
        std::mem::swap(&mut self.base, &mut other.base);
        std::mem::swap(&mut self.base_hash, &mut other.base_hash);
        std::mem::swap(&mut self.arrays, &mut other.arrays);
        std::mem::swap(&mut self.xfs_at_open, &mut other.xfs_at_open);
        std::mem::swap(&mut self.xf_of, &mut other.xf_of);
        std::mem::swap(&mut self.fmt_of, &mut other.fmt_of);
        std::mem::swap(&mut self.was, &mut other.was);
    }

    /// One step back (`back`) or forward over `step`; the step the other way.
    fn step(&mut self, step: Step, back: bool) -> Step {
        match step {
            Step::Cell { at, calls } => {
                for _ in 0..calls {
                    let _ = if back { self.model.undo() } else { self.model.redo() };
                }
                let _ = self.model.flush_send_queue();
                Step::Cell { at, calls }
            }
            Step::Lines { at, mut other } => {
                self.swap(&mut other);
                Step::Lines { at, other }
            }
        }
    }

    fn undo(&mut self) -> Option<At> {
        let step = self.undo.pop()?;
        let at = step.at();
        let back = self.step(step, true);
        self.redo.push(back);
        Some(at)
    }

    fn redo(&mut self) -> Option<At> {
        let step = self.redo.pop()?;
        let at = step.at();
        let forward = self.step(step, false);
        self.undo.push(forward);
        Some(at)
    }

    /// **Rows or columns put in or taken out** (`shift`) of sheet `sheet`:
    /// the package as a save would write it now, changed (`shift/parts.rs`),
    /// read again — after its every cell is checked against IronCalc's own
    /// Insert or Delete on the workbook as it is ([`check_lines`]). Refused,
    /// with why, and nothing changed: a part Yap can't move, a cell that
    /// would come out different. A panic of IronCalc's is a refusal too.
    fn lines(&mut self, sheet: u32, s: Shift) -> Result<At, String> {
        let what = said(&s);
        let locale = self.locale;
        let me = &*self;
        let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<Layer, String> {
            let written = super::save::xlsx_bytes(me).map_err(|why| format!("Yap couldn't {what} safely ({why}), so nothing changed."))?;
            let model = me.model.get_model();
            let formula_at = |si: u32, r: u32, c: u32| formula_of(model, si, r as i32, c as i32);
            let change = super::shift::parts::Change { sheet, shift: s, formula_at: &formula_at };
            let shifted = super::shift::parts::package(&written.bytes, &change).map_err(|why| format!("Yap can't {what} here: {why}. Do it in Excel."))?;
            let layer = Layer::open(shifted, locale).map_err(|why| format!("Yap couldn't {what} safely ({why}), so nothing changed."))?;
            check_lines(&written.bytes, &layer, sheet, &s, locale).map_err(|why| format!("Yap can't {what} here safely: {why}. Do it in Excel."))?;
            Ok(layer)
        }))
        .unwrap_or_else(|_| Err(format!("Yap couldn't {what} safely, so nothing changed.")));
        let mut layer = made?;
        self.swap(&mut layer);
        let at = match s.axis {
            Axis::Rows => (sheet, s.at as i32, 1),
            Axis::Cols => (sheet, 1, s.at as i32),
        };
        self.undo.push(Step::Lines { at, other: Box::new(layer) });
        self.redo.clear();
        self.trim_undo();
        tracing::info!(rows = matches!(s.axis, Axis::Rows), insert = s.insert, n = s.n, "Chat: a sheet's rows or columns put in or taken out");
        Ok(at)
    }

    /// **Saved** (`written`): its bytes are the file now, so each cell changed is as it has it.
    pub fn saved(&mut self, written: super::save::Written) {
        self.base_hash = super::save::hash_of(&written.bytes);
        self.file_hash = self.base_hash;
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

/// What rows or columns put in or out is, said ("insert a row", "delete 3 columns").
fn said(s: &Shift) -> String {
    let (one, many) = match s.axis {
        Axis::Rows => ("row", "rows"),
        Axis::Cols => ("column", "columns"),
    };
    match (s.insert, s.n) {
        (true, 1) => format!("insert a {one}"),
        (true, n) => format!("insert {n} {many}"),
        (false, 1) => format!("delete this {one}"),
        (false, n) => format!("delete {n} {many}"),
    }
}

/// **The workbook with rows or columns put in or out, checked**: `before`
/// (the package as it was) read, IronCalc's own Insert or Delete made on it,
/// and every cell of every sheet must hold the same in `after` (its value or
/// its formula), every merge be the same. One allowance: a reference whose
/// area loses its first or last row taken out is `#REF!` in IronCalc's and
/// shrunk in Yap's (Excel shrinks it). Why not, said.
fn check_lines(before: &[u8], after: &Layer, sheet: u32, s: &Shift, locale: &str) -> Result<(), String> {
    let (mut expected, _) = super::xlsx::model_now(&super::xlsx::complete(before), locale).map_err(|_| "it wouldn't read back".to_string())?;
    let (at, n) = (s.at as i32, s.n as i32);
    let done = match (s.axis, s.insert) {
        (Axis::Rows, true) => expected.insert_rows(sheet, at, n),
        (Axis::Rows, false) => expected.delete_rows(sheet, at, n),
        (Axis::Cols, true) => expected.insert_columns(sheet, at, n),
        (Axis::Cols, false) => expected.delete_columns(sheet, at, n),
    };
    done.map_err(|e| {
        let e = e.to_ascii_lowercase();
        if e.contains("array") {
            "an array formula would be cut apart".to_string()
        } else if e.contains("end of") {
            "cells would be pushed off the end of the sheet".to_string()
        } else {
            "it couldn't be worked out".to_string()
        }
    })?;
    expected.evaluate();
    let got = after.model.get_model();
    if expected.workbook.worksheets.len() != got.workbook.worksheets.len() {
        return Err("its sheets came out different".into());
    }
    for (si, ws) in expected.workbook.worksheets.iter().enumerate() {
        let si = si as u32;
        let theirs = got.workbook.worksheet(si).map_err(|_| "its sheets came out different".to_string())?;
        let mut cells: std::collections::BTreeSet<(i32, i32)> = ws.sheet_data.cells().map(|(r, c, _)| (r, c)).collect();
        cells.extend(theirs.sheet_data.cells().map(|(r, c, _)| (r, c)));
        for (r, c) in cells {
            let (a, b) = (held_of(&expected, si, r, c), held_of(got, si, r, c));
            if a == b {
                continue;
            }
            let shrunk = !s.insert && matches!((&a, &b), (Held::Formula(x), Held::Formula(_)) if x.contains("#REF!"));
            if !shrunk {
                tracing::warn!(sheet = si, "Chat: rows or columns put in or out came out different from IronCalc's, so nothing changed");
                return Err("a cell would come out different".into());
            }
        }
        let merges = |list: &Vec<ironcalc::base::types::MergedCell>| -> std::collections::BTreeSet<(i32, i32, i32, i32)> { list.iter().filter(|m| m.width > 1 || m.height > 1).map(|m| (m.row, m.column, m.width, m.height)).collect() };
        if merges(&ws.merged_cells) != merges(&theirs.merged_cells) {
            return Err("its merged cells would come out different".into());
        }
    }
    Ok(())
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

/// **`text` with records put in or taken out** (rows), or **fields**
/// (columns: in each record that reaches them), as `s` says; every other
/// record as written. Rows put in past the last record: nothing. Pure.
pub fn with_lines(text: &str, sep: u8, s: &Shift) -> String {
    let recs = records(text, sep);
    let i = s.at as usize - 1;
    let n = s.n as usize;
    match (s.axis, s.insert) {
        (Axis::Rows, true) => {
            let Some(rec) = recs.get(i) else { return text.to_string() };
            let width = recs.first().map(|r| r.fields.len()).unwrap_or(1).max(1);
            let empty = if width > 1 { (sep as char).to_string().repeat(width - 1) } else { "\"\"".to_string() };
            format!("{}{}{}", &text[..rec.start], format!("{empty}\n").repeat(n), &text[rec.start..])
        }
        (Axis::Rows, false) => {
            let Some(first) = recs.get(i) else { return text.to_string() };
            let last = (i + n).min(recs.len()) - 1;
            let to = recs.get(last + 1).map(|r| r.start).unwrap_or(text.len());
            format!("{}{}", &text[..first.start], &text[to..])
        }
        (Axis::Cols, insert) => {
            let mut out = text.to_string();
            for rec in recs.iter().rev() {
                if rec.fields.len() <= i {
                    continue;
                }
                let mut fields = rec.fields.clone();
                if insert {
                    fields.splice(i..i, std::iter::repeat_n(String::new(), n));
                } else {
                    fields.drain(i..(i + n).min(fields.len()));
                    if fields.is_empty() {
                        fields.push(String::new());
                    }
                }
                out.replace_range(rec.start..rec.end, &record_text(&fields, sep));
            }
            out
        }
    }
}

impl Delimited {
    fn edit(&mut self, row: u32, col: u32, input: &str) -> Result<(), String> {
        let next = with_field(&self.text, self.sep, row, col, input);
        self.push(next, (row, col));
        Ok(())
    }

    fn lines(&mut self, s: Shift) -> Result<At, String> {
        let next = with_lines(&self.text, self.sep, &s);
        let at = match s.axis {
            Axis::Rows => (s.at, 1),
            Axis::Cols => (1, s.at),
        };
        self.push(next, at);
        Ok((0, at.0 as i32, at.1 as i32))
    }

    /// The text now `next` (a change, `at` the cell to go back to), Undo keeping what it was.
    fn push(&mut self, next: String, at: (u32, u32)) {
        if next == self.text {
            return;
        }
        let before = std::mem::replace(&mut self.text, next);
        self.undo.push((before, at));
        self.redo.clear();
        // Undo keeps the newest versions up to its room.
        let mut room = UNDO_BYTES;
        let keep = self.undo.iter().rev().take_while(|(t, _)| room.checked_sub(t.len()).map(|r| room = r).is_some()).count();
        let drop = self.undo.len() - keep;
        self.undo.drain(..drop);
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

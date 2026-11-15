//! **An `.xlsx` / `.xlsm`, read and calculated by IronCalc**: its cells as
//! shown (the file's number and date formats), every formula calculated on
//! this PC — a workbook a script wrote (openpyxl, exceljs) keeps no results,
//! so this is how its values show at all — and its looks: fonts, colours
//! (the theme's resolved), fills, borders, alignment, widths and heights,
//! merges, frozen panes, grid lines, tab colours, hidden sheets.
//!
//! A function IronCalc doesn't know yet (`#N/IMPL!`, `#NAME?`) keeps the
//! result the file had, when it had one: Excel's own sum is better than none.

use std::collections::HashMap;

use ironcalc::base::expressions::token::Error as CalcError;
use ironcalc::base::types::{BorderItem, BorderStyle, Cell as CalcCell, CellType, Color, FormulaValue, HorizontalAlignment, SheetState, Style as CalcStyle, Theme, VerticalAlignment};
use ironcalc::base::Model;

use super::{keep_rows, Book, Cell, Cols, Edge, Format, Kind, Merge, Row, RowLook, Sheet, Style, Why, MAX_CELLS};

/// The language and locale IronCalc reads formulas and formats numbers in
/// (a file's own formats win: "dd/mm/yyyy" stays that).
const LANGUAGE: &str = "en";
const LOCALE: &str = "en";
const TZ: &str = "UTC";

/// **Read an `.xlsx`.** Blocking (the caller's own thread: `sheets::read`).
pub fn read(bytes: &[u8]) -> Result<Book, Why> {
    if super::is_encrypted_office(bytes) {
        return Err(Why::Locked);
    }
    let workbook = ironcalc::import::load_from_xlsx_bytes(bytes, "workbook", LOCALE, TZ).map_err(|_| Why::Failed)?;
    let mut model = Model::from_workbook(workbook, LANGUAGE).map_err(|_| Why::Failed)?;
    let kept = file_results(&model);
    model.evaluate();
    Ok(book_of(&model, &kept))
}

/// What a formula's result says, as the file kept it: `(text, kind)`.
type Kept = HashMap<(u32, i32, i32), (String, Kind)>;

/// **The results the file kept** for its formulas (before Yap calculates):
/// none for a result that's no result (a script's file: none, or 0).
fn file_results(model: &Model) -> Kept {
    let mut out = HashMap::new();
    for (si, ws) in model.workbook.worksheets.iter().enumerate() {
        for (r, c, cell) in ws.sheet_data.cells() {
            let v = match &*cell {
                CalcCell::CellFormula { v, .. } | CalcCell::ArrayFormula { v, .. } => v,
                _ => continue,
            };
            let kind = match v {
                FormulaValue::Unevaluated => continue,
                FormulaValue::Number(n) if *n == 0.0 => continue,
                FormulaValue::Number(_) => Kind::Number,
                FormulaValue::Boolean(_) => Kind::Bool,
                FormulaValue::Text(_) => Kind::Text,
                FormulaValue::Error { ei: CalcError::NIMPL | CalcError::NAME | CalcError::ERROR, .. } => continue,
                FormulaValue::Error { .. } => Kind::Error,
            };
            if let Ok(text) = model.get_formatted_cell_value(si as u32, r, c) {
                out.insert((si as u32, r, c), (text, kind));
            }
        }
    }
    out
}

/// Whether a cell's calculated result is IronCalc not knowing the function.
fn not_known(cell: &CalcCell) -> bool {
    match cell {
        CalcCell::CellFormula { v: FormulaValue::Error { ei, .. }, .. } | CalcCell::ArrayFormula { v: FormulaValue::Error { ei, .. }, .. } => matches!(ei, CalcError::NIMPL | CalcError::NAME),
        _ => false,
    }
}

/// **The workbook as Yap draws it**, calculated.
fn book_of(model: &Model, kept: &Kept) -> Book {
    let theme = model.get_theme();
    let mut book = Book::new(Format::Xlsx);
    book.calculated = true;
    if let Some(f) = model.workbook.styles.fonts.first() {
        if !f.name.is_empty() {
            book.font = f.name.clone();
        }
        if f.sz > 0 {
            book.size = f.sz as f32;
        }
    }
    let (font, size) = (book.font.clone(), book.size);
    let mut known: HashMap<Style, u32> = HashMap::new();
    // IronCalc's style index → Yap's.
    let mut by_index: HashMap<i32, u32> = HashMap::new();
    let mut room = MAX_CELLS;
    for (si, ws) in model.workbook.worksheets.iter().enumerate() {
        let sheet_no = si as u32;
        let mut sheet = Sheet {
            name: ws.name.clone(),
            hidden: !matches!(ws.state, SheetState::Visible),
            color: colour(&ws.color, &theme),
            frozen_rows: ws.frozen_rows.max(0) as u32,
            frozen_cols: ws.frozen_columns.max(0) as u32,
            grid: ws.show_grid_lines,
            ..Default::default()
        };
        let mut rows: Vec<Row> = Vec::new();
        for (r, c, cell) in ws.sheet_data.cells() {
            if r < 1 || c < 1 {
                continue;
            }
            let index = cell.get_style();
            let style = match by_index.get(&index) {
                Some(&id) => id,
                None => {
                    let id = model.get_style_for_cell(sheet_no, r, c).map(|s| book.style_id(style_of(&s, &theme, &font, size), &mut known)).unwrap_or(0);
                    by_index.insert(index, id);
                    id
                }
            };
            let (text, kind) = if matches!(&*cell, CalcCell::EmptyCell { .. }) {
                (String::new(), Kind::Empty)
            } else if let Some(file) = kept.get(&(sheet_no, r, c)).filter(|_| not_known(&cell)) {
                file.clone()
            } else {
                let text = model.get_formatted_cell_value(sheet_no, r, c).unwrap_or_default();
                let kind = match model.get_cell_type(sheet_no, r, c) {
                    Ok(CellType::Number) => Kind::Number,
                    Ok(CellType::LogicalValue) => Kind::Bool,
                    Ok(CellType::ErrorValue) => Kind::Error,
                    _ => Kind::Text,
                };
                // (A text cell with nothing in it: only its style.)
                if text.is_empty() && kind == Kind::Text {
                    (text, Kind::Empty)
                } else {
                    (text, kind)
                }
            };
            if kind == Kind::Empty && style == 0 {
                continue;
            }
            let formula = cell.get_formula().and_then(|_| model.get_cell_formula(sheet_no, r, c).ok().flatten());
            if formula.is_some() {
                sheet.formulas += 1;
            }
            let (r, c) = (r as u32, c as u32);
            if kind != Kind::Empty {
                sheet.used.take(r, c);
            }
            sheet.drawn.take(r, c);
            match rows.last_mut() {
                Some(row) if row.r == r => row.cells.push(Cell { col: c, text, kind, formula, style }),
                _ => rows.push(Row { r, cells: vec![Cell { col: c, text, kind, formula, style }] }),
            }
        }
        let (rows, cut, cells) = keep_rows(rows, room);
        room = room.saturating_sub(cells);
        sheet.rows = rows;
        sheet.cut_rows = cut;
        for m in &ws.merged_cells {
            if m.row < 1 || m.column < 1 || m.width < 1 || m.height < 1 {
                continue;
            }
            let merge = Merge { row: m.row as u32, col: m.column as u32, rows: m.height as u32, cols: m.width as u32 };
            sheet.drawn.take(merge.row, merge.col);
            sheet.drawn.take(merge.row + merge.rows - 1, merge.col + merge.cols - 1);
            sheet.merges.push(merge);
        }
        for col in &ws.cols {
            if col.min < 1 || col.max < col.min {
                continue;
            }
            let style = col.style.filter(|&s| s != 0).and_then(|_| model.get_column_style(sheet_no, col.min).ok().flatten()).map(|s| book.style_id(style_of(&s, &theme, &font, size), &mut known)).filter(|&id| id != 0);
            let px = (col.width > 0.0).then(|| width_px(col.width));
            if px.is_none() && !col.hidden && style.is_none() {
                continue;
            }
            sheet.cols.push(Cols { from: col.min as u32, to: col.max.min(16_384) as u32, px, hidden: col.hidden, style });
        }
        for row in &ws.rows {
            if row.r < 1 {
                continue;
            }
            let style = if row.custom_format { model.get_row_style(sheet_no, row.r).ok().flatten().map(|s| book.style_id(style_of(&s, &theme, &font, size), &mut known)).filter(|&id| id != 0) } else { None };
            let px = (row.height > 0.0).then(|| height_px(row.height));
            if px.is_none() && !row.hidden && style.is_none() {
                continue;
            }
            sheet.row_looks.push(RowLook { r: row.r as u32, px, hidden: row.hidden, style });
        }
        book.sheets.push(sheet);
    }
    book
}

/// A column width as Excel keeps it (characters of its widest digit, with
/// padding) in pixels as Excel draws it at 100% (Calibri 11: 7 a digit). Pure.
pub fn width_px(width: f64) -> f32 {
    (width * 7.0 + 0.5).floor().max(0.0) as f32
}

/// A row height in points as pixels at 96 dpi. Pure.
pub fn height_px(points: f64) -> f32 {
    ((points * 4.0 / 3.0) as f32).round().max(0.0)
}

/// A colour as `#RRGGBB` (the theme's resolved); none for none.
fn colour(c: &Color, theme: &Theme) -> Option<String> {
    let rgb = c.to_rgb(theme);
    (rgb.len() == 7 && rgb.starts_with('#')).then(|| rgb.to_ascii_uppercase())
}

/// A font's colour: none for Excel's "Automatic" — no colour, the theme's
/// "Text 1" untinted (every Excel file's default font has it), or plain
/// black — so the view's ink draws it (light text in dark mode).
fn font_colour(c: &Color, theme: &Theme) -> Option<String> {
    match c {
        Color::None => None,
        Color::Theme(1, tint) if tint.abs() < 1e-6 => None,
        _ => colour(c, theme).filter(|rgb| rgb != "#000000"),
    }
}

/// An edge of a border.
fn edge(b: &Option<BorderItem>, theme: &Theme) -> Option<Edge> {
    let b = b.as_ref()?;
    let style = match b.style {
        BorderStyle::Thin => "thin",
        BorderStyle::Medium => "medium",
        BorderStyle::Thick => "thick",
        BorderStyle::Double => "double",
        BorderStyle::Dotted => "dotted",
        BorderStyle::MediumDashed | BorderStyle::MediumDashDot | BorderStyle::MediumDashDotDot | BorderStyle::SlantDashDot => "dashed",
    };
    Some(Edge { style, color: colour(&b.color, theme) })
}

/// **IronCalc's style as Yap draws it**: what differs from the workbook's
/// font (`font`, `size`) kept.
pub fn style_of(s: &CalcStyle, theme: &Theme, font: &str, size: f32) -> Style {
    let f = &s.font;
    let (h, v, wrap) = match &s.alignment {
        Some(a) => (
            match a.horizontal {
                HorizontalAlignment::Left => Some("left"),
                HorizontalAlignment::Center | HorizontalAlignment::CenterContinuous => Some("center"),
                HorizontalAlignment::Right => Some("right"),
                HorizontalAlignment::Justify | HorizontalAlignment::Distributed => Some("justify"),
                HorizontalAlignment::Fill => Some("fill"),
                HorizontalAlignment::General => None,
            },
            match a.vertical {
                VerticalAlignment::Top => Some("top"),
                VerticalAlignment::Center | VerticalAlignment::Distributed | VerticalAlignment::Justify => Some("center"),
                VerticalAlignment::Bottom => None,
            },
            a.wrap_text,
        ),
        None => (None, None, false),
    };
    Style {
        bold: f.b,
        italic: f.i,
        underline: f.u,
        strike: f.strike,
        font: (!f.name.is_empty() && f.name != font).then(|| f.name.clone()),
        size: (f.sz > 0 && (f.sz as f32 - size).abs() > 0.01).then(|| (f.sz.clamp(1, 409) * 10) as u16),
        color: font_colour(&f.color, theme),
        fill: colour(&s.fill.color, theme),
        h,
        v,
        wrap,
        top: edge(&s.border.top, theme),
        right: edge(&s.border.right, theme),
        bottom: edge(&s.border.bottom, theme),
        left: edge(&s.border.left, theme),
    }
}

//! **An `.xls`, `.xlsb` or `.ods`, read by calamine** (and an `.xlsx`
//! IronCalc couldn't take): each sheet's values as the file keeps them, with
//! their formulas. The app that saved one calculated it, so its results are
//! there. No looks: these formats' styles aren't read, so columns are fitted
//! to their cells. Numbers are shown as Excel's General format shows them,
//! dates as `2026-11-15` (with their time when they have one).

use std::io::Cursor;

use calamine::{Data, Reader, SheetType, SheetVisible};

use super::{fit_columns, keep_rows, Book, Cell, Format, Kind, Row, Sheet, Why, MAX_CELLS};

/// **Read one** (`format`: what its ending says). Blocking.
pub fn read(bytes: &[u8], format: Format) -> Result<Book, Why> {
    if super::is_encrypted_office(bytes) {
        return Err(Why::Locked);
    }
    let mut wb = match calamine::open_workbook_auto_from_rs(Cursor::new(bytes)) {
        Ok(wb) => wb,
        Err(_) => {
            // An .xls or .ods says it's password-protected only when opened as one.
            let locked = match format {
                Format::Ods => matches!(calamine::open_workbook_from_rs::<calamine::Ods<_>, _>(Cursor::new(bytes)), Err(calamine::OdsError::Password)),
                _ => matches!(calamine::open_workbook_from_rs::<calamine::Xls<_>, _>(Cursor::new(bytes)), Err(calamine::XlsError::Password)),
            };
            return Err(if locked { Why::Locked } else { Why::Failed });
        }
    };
    let mut book = Book::new(format);
    let metas: Vec<calamine::Sheet> = wb.sheets_metadata().to_vec();
    let mut room = MAX_CELLS;
    for meta in metas {
        if meta.typ != SheetType::WorkSheet {
            continue;
        }
        let Ok(values) = wb.worksheet_range(&meta.name) else { continue };
        let formulas = wb.worksheet_formula(&meta.name).ok();
        let mut sheet = Sheet { name: meta.name.clone(), hidden: !matches!(meta.visible, SheetVisible::Visible), grid: true, ..Default::default() };
        let mut rows: Vec<Row> = Vec::new();
        if let Some((top, left)) = values.start() {
            for (r, c, v) in values.used_cells() {
                let (text, kind) = shown(v);
                if kind == Kind::Empty {
                    continue;
                }
                let (row, col) = (top + r as u32 + 1, left + c as u32 + 1);
                let cell = Cell { col, text, kind, formula: None, style: 0 };
                sheet.used.take(row, col);
                match rows.last_mut() {
                    Some(last) if last.r == row => last.cells.push(cell),
                    _ => rows.push(Row { r: row, cells: vec![cell] }),
                }
            }
        }
        // Each formula onto its cell (a formula whose result is empty: a cell of its own).
        if let Some(formulas) = formulas {
            if let Some((top, left)) = formulas.start() {
                for (r, c, f) in formulas.used_cells() {
                    if f.trim().is_empty() {
                        continue;
                    }
                    let (row, col) = (top + r as u32 + 1, left + c as u32 + 1);
                    let formula = if f.starts_with('=') { f.clone() } else { format!("={f}") };
                    sheet.formulas += 1;
                    put_formula(&mut rows, row, col, formula);
                }
            }
        }
        sheet.drawn = sheet.used;
        let (rows, cut, cells) = keep_rows(rows, room);
        room = room.saturating_sub(cells);
        sheet.rows = rows;
        sheet.cut_rows = cut;
        fit_columns(&mut sheet);
        book.sheets.push(sheet);
    }
    if book.sheets.is_empty() {
        return Err(Why::Failed);
    }
    Ok(book)
}

/// `formula` onto cell (`row`, `col`) of `rows` (sorted), the cell made when
/// its result was empty.
fn put_formula(rows: &mut Vec<Row>, row: u32, col: u32, formula: String) {
    let at = match rows.binary_search_by_key(&row, |r| r.r) {
        Ok(i) => i,
        Err(i) => {
            rows.insert(i, Row { r: row, cells: Vec::new() });
            i
        }
    };
    let cells = &mut rows[at].cells;
    match cells.binary_search_by_key(&col, |c| c.col) {
        Ok(i) => cells[i].formula = Some(formula),
        Err(i) => cells.insert(i, Cell { col, text: String::new(), kind: Kind::Text, formula: Some(formula), style: 0 }),
    }
}

/// **A value as shown**, and its kind. Pure.
pub fn shown(v: &Data) -> (String, Kind) {
    match v {
        Data::Empty => (String::new(), Kind::Empty),
        Data::String(s) if s.is_empty() => (String::new(), Kind::Empty),
        Data::String(s) => (s.clone(), Kind::Text),
        Data::Int(n) => (n.to_string(), Kind::Number),
        Data::Float(f) => (general(*f), Kind::Number),
        Data::Bool(b) => ((if *b { "TRUE" } else { "FALSE" }).to_string(), Kind::Bool),
        Data::DateTime(d) if d.is_duration() => (format_with(d.as_f64(), "[h]:mm:ss"), Kind::Number),
        Data::DateTime(d) => {
            let serial = d.as_f64();
            let fmt = if serial.fract().abs() < 1e-9 { "yyyy-mm-dd" } else if serial < 1.0 { "hh:mm:ss" } else { "yyyy-mm-dd hh:mm" };
            (format_with(serial, fmt), Kind::Number)
        }
        Data::DateTimeIso(s) | Data::DurationIso(s) => (s.replacen('T', " ", 1), Kind::Number),
        Data::Error(e) => (e.to_string(), Kind::Error),
    }
}

/// A number as Excel's General format shows it. Pure.
pub fn general(f: f64) -> String {
    format_with(f, "general")
}

/// `value` in Excel's number format `fmt` (IronCalc's formatter). Pure.
pub fn format_with(value: f64, fmt: &str) -> String {
    match ironcalc::base::locale::get_locale("en") {
        Ok(locale) => ironcalc::base::formatter::format::format_number(value, fmt, locale).text,
        Err(_) => value.to_string(),
    }
}

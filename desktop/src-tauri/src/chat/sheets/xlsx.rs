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

/// The language IronCalc reads formulas in (function names: Excel's English).
const LANGUAGE: &str = "en";
/// The locale when nothing says otherwise: IronCalc's own (the US's).
pub const LOCALE: &str = "en";
const TZ: &str = "UTC";

// ---- the locale: what typed input means ----------------------------------------------------------

/// **The locale a workbook is read and typed into in** (16 Nov 2026): the
/// person's Region (Settings → About you → Region, `ai/about_you.rs`) when
/// IronCalc has one for it ([`locale_for_region`]: in the UK, `16/11/2026`
/// typed is a date and `£12.50` a currency value), else the file's own (the
/// language its formats name, `[$-809]`: [`locale_of_file`]), else
/// IronCalc's own `en` (the US's). It's also how numbers show where the
/// file's format leaves it to the locale; a file's own formats win
/// ("dd/mm/yyyy" stays that).
pub fn locale_for(bytes: &[u8]) -> &'static str {
    let id = region_locale().or_else(|| locale_of_file(bytes)).unwrap_or(LOCALE);
    if ironcalc::base::locale::get_locale(id).is_ok() {
        id
    } else {
        LOCALE
    }
}

/// The person's Region's locale, if IronCalc has one for it (a test: none, never this PC's).
fn region_locale() -> Option<&'static str> {
    #[cfg(test)]
    {
        None
    }
    #[cfg(not(test))]
    {
        let cfg = crate::config::load();
        let r = crate::ai::about_you::resolve(&crate::ai::about_you::detected(), &cfg.region_country);
        locale_for_region(r.language.as_deref(), r.country.as_deref())
    }
}

/// Where dates are day first, as in the UK, numbers written `1,234.50`, and
/// English spoken: typed in as IronCalc's `en-GB` reads them.
const DAY_FIRST_ENGLISH: [&str; 12] = ["GB", "IE", "AU", "NZ", "IN", "SG", "HK", "MT", "JE", "GG", "IM", "GI"];

/// **IronCalc's locale for a Region** (`language` "en-GB", `country`
/// "GB"), when it has one whose dates, numbers and currency are that
/// country's: a country written as the UK writes ([`DAY_FIRST_ENGLISH`]:
/// `en-GB`), or a language's home (Germany and Austria `de`, France `fr`,
/// Spain `es`, Italy `it`, the US `en`). None: no match, so a date typed is
/// never read the wrong way round. Pure.
pub fn locale_for_region(language: Option<&str>, country: Option<&str>) -> Option<&'static str> {
    let country = country.map(str::to_ascii_uppercase);
    let lang = language.map(|l| crate::ai::about_you::split_tag(l).0.to_ascii_lowercase()).unwrap_or_default();
    let c = country.as_deref()?;
    if DAY_FIRST_ENGLISH.contains(&c) {
        return Some("en-GB");
    }
    Some(match (c, lang.as_str()) {
        ("US", _) => "en",
        ("DE" | "AT", _) => "de",
        ("LU", "de") => "de",
        ("FR" | "MC", _) => "fr",
        ("LU" | "BE", "fr") => "fr",
        ("ES", _) => "es",
        ("IT" | "SM" | "VA", _) => "it",
        _ => return None,
    })
}

/// **The locale a workbook's own formats name** (`[$-809]dd/mm/yyyy`,
/// `[$£-809]#,##0.00`: Windows' language ids), the one they name most, if
/// IronCalc has it. Pure.
pub fn locale_of_file(bytes: &[u8]) -> Option<&'static str> {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
    let mut styles = String::new();
    zip.by_name("xl/styles.xml").ok()?.read_to_string(&mut styles).ok()?;
    let mut counts: HashMap<&'static str, usize> = HashMap::new();
    let mut rest = styles.as_str();
    while let Some(at) = rest.find("[$") {
        rest = &rest[at + 2..];
        let Some(close) = rest.find(']') else { break };
        let inside = &rest[..close];
        if let Some((_, id)) = inside.rsplit_once('-') {
            if let Some(locale) = u32::from_str_radix(id.trim(), 16).ok().and_then(|lcid| locale_of_lcid(lcid & 0xFFFF)) {
                *counts.entry(locale).or_default() += 1;
            }
        }
    }
    counts.into_iter().max_by_key(|(id, n)| (*n, std::cmp::Reverse(*id))).map(|(id, _)| id)
}

/// IronCalc's locale for a Windows language id (LCID). Pure.
fn locale_of_lcid(lcid: u32) -> Option<&'static str> {
    Some(match lcid {
        0x0409 => "en",
        0x0809 | 0x1809 | 0x0C09 | 0x1409 | 0x4009 | 0x4809 => "en-GB",
        0x0407 | 0x0C07 => "de",
        0x040C | 0x180C => "fr",
        0x040A | 0x0C0A => "es",
        0x0410 => "it",
        _ => return None,
    })
}

/// **Read an `.xlsx`.** Blocking (the caller's own thread: `sheets::read`).
///
/// IronCalc takes six parts of `xl/styles.xml` for granted that the format
/// leaves out when they'd be empty (a file a small writer made: no
/// `<cellStyles>`, say) and panics without one (Lab, 15 Nov 2026: the file
/// left out of Chat altogether). So a file lacking one is read with those
/// parts filled in ([`with_full_styles`]) — before IronCalc sees it, so it
/// never panics over them (a panic is in the log as an error) — and any
/// panic of its own is a failure like any other, so `sheets::read_now`'s
/// values-only reader still has its turn.
pub fn read(bytes: &[u8]) -> Result<Book, Why> {
    if super::is_encrypted_office(bytes) {
        return Err(Why::Locked);
    }
    let locale = locale_for(bytes);
    load(&complete(bytes), locale)
}

/// IronCalc reading and calculating `bytes`, its panic an error.
fn load(bytes: &[u8], locale: &str) -> Result<Book, Why> {
    let (model, kept) = model_now(bytes, locale)?;
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| book_of(&model, &kept))).map_err(|_| Why::Failed)
}

/// **The workbook `bytes` as IronCalc's model, calculated**, and the
/// results the file kept (a function IronCalc doesn't know keeps the
/// file's): the sheet view's editor's (`edit.rs`). `bytes` with its style
/// parts filled in already ([`complete`]). A panic is an error.
pub(super) fn model_now(bytes: &[u8], locale: &str) -> Result<(Model<'static>, Kept), Why> {
    std::panic::catch_unwind(|| {
        let workbook = ironcalc::import::load_from_xlsx_bytes(bytes, "workbook", locale, TZ).map_err(|_| Why::Failed)?;
        let mut model = Model::from_workbook(workbook, LANGUAGE).map_err(|_| Why::Failed)?;
        for f in model.workbook.styles.num_fmts.iter_mut() {
            if let Some(q) = quoted_literals(&f.format_code) {
                f.format_code = q;
            }
        }
        let kept = file_results(&model);
        model.evaluate();
        Ok((model, kept))
    })
    .unwrap_or(Err(Why::Failed))
}

/// **A number format with its bare currency signs quoted** (`£#,##0.00` →
/// `"£"#,##0.00`): Excel shows `£`, `¥`, `₹`… as they are, unquoted, and a
/// script writes them so (openpyxl, exceljs), but IronCalc only takes `$`
/// and `€` bare and shows every other one's number as `#VALUE!` (Lab, 16 Nov
/// 2026: a budget in pounds, every price `#VALUE!`). Any character past
/// ASCII but `€`, outside quotes and `[…]` and not after `\`, is quoted;
/// `None`: nothing to quote. Pure.
pub fn quoted_literals(code: &str) -> Option<String> {
    let mut out = String::with_capacity(code.len() + 4);
    let (mut quoted, mut bracket, mut escaped, mut run, mut changed) = (false, false, false, String::new(), false);
    let flush = |out: &mut String, run: &mut String| {
        if !run.is_empty() {
            out.push('"');
            out.push_str(run);
            out.push('"');
            run.clear();
        }
    };
    for ch in code.chars() {
        let bare = !quoted && !bracket && !escaped && !ch.is_ascii() && ch != '€';
        if bare {
            run.push(ch);
            changed = true;
            continue;
        }
        flush(&mut out, &mut run);
        if escaped {
            escaped = false;
        } else if ch == '\\' && !quoted {
            escaped = true;
        } else if ch == '"' && !bracket {
            quoted = !quoted;
        } else if ch == '[' && !quoted {
            bracket = true;
        } else if ch == ']' && !quoted {
            bracket = false;
        }
        out.push(ch);
    }
    flush(&mut out, &mut run);
    changed.then_some(out)
}

/// **The package IronCalc reads**: `bytes` with its missing style parts
/// filled in ([`with_full_styles`]), or as it is. A save writes into the
/// file's own (`save.rs`): the parts filled in never reach it (they'd be
/// out of the stylesheet's order). Also: an empty cell a writer gave a type
/// (openpyxl's styled empty cell, `<c r="B3" s="1" t="n"/>`) read as empty,
/// as Excel shows it, not IronCalc's 0 ([`without_typed_empties`]).
pub(super) fn complete(bytes: &[u8]) -> Vec<u8> {
    prepared(bytes).unwrap_or_else(|| bytes.to_vec())
}

/// **A sheet's XML with its empty cells' types left out** (`<c r="B3" s="1"
/// t="n"/>` → `<c r="B3" s="1"/>`): a cell with nothing in it is empty
/// whatever its type says (Excel's reading; IronCalc reads an empty number
/// as 0). None: there's none. Pure.
pub fn without_typed_empties(xml: &str) -> Option<String> {
    use quick_xml::events::Event;
    if !xml.contains("t=") {
        return None;
    }
    let mut reader = quick_xml::Reader::from_str(xml);
    let mut out = String::new();
    let mut at = 0;
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event().ok()?;
        let end = reader.buffer_position() as usize;
        match event {
            Event::Eof => break,
            Event::Empty(e) if e.local_name().as_ref() == "c" && e.try_get_attribute("t").ok().flatten().is_some() => {
                let mut tag = format!("<{}", e.name().as_ref());
                for a in e.attributes().with_checks(false).flatten() {
                    if a.key.as_ref() != "t" {
                        tag.push_str(&format!(" {}=\"{}\"", a.key.as_ref(), a.value.replace('"', "&quot;")));
                    }
                }
                tag.push_str("/>");
                out.push_str(&xml[at..start]);
                out.push_str(&tag);
                at = end;
            }
            _ => {}
        }
    }
    if at == 0 {
        return None;
    }
    out.push_str(&xml[at..]);
    Some(out)
}

/// The package IronCalc reads, when it differs from `bytes`: its style
/// parts filled in ([`full_styles`]; a whole `styles.xml` when it has
/// none), its sheets' typed empty cells untyped; every other part as it was.
fn prepared(bytes: &[u8]) -> Option<Vec<u8>> {
    use std::io::{Read, Write};
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
    let mut new: HashMap<String, String> = HashMap::new();
    let styles = match zip.by_name("xl/styles.xml") {
        Ok(mut f) => {
            let mut s = String::new();
            f.read_to_string(&mut s).ok()?;
            full_styles(&s)
        }
        Err(_) => {
            let parts: String = STYLE_PARTS.iter().map(|(_, xml)| *xml).collect();
            Some(format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">{parts}</styleSheet>"#))
        }
    };
    if let Some(styles) = styles {
        tracing::info!("Chat: an .xlsx's missing style parts filled in for its reader");
        new.insert("xl/styles.xml".into(), styles);
    }
    let sheets: Vec<String> = zip.file_names().filter_map(|n| n.ok().map(|n| n.into_owned())).filter(|n| n.starts_with("xl/worksheets/") && n.ends_with(".xml") && !n.contains("/_rels/")).collect();
    for name in sheets {
        let mut s = String::new();
        if zip.by_name(&name).ok()?.read_to_string(&mut s).is_err() {
            continue;
        }
        if let Some(fixed) = without_typed_empties(&s) {
            new.insert(name, fixed);
        }
    }
    if new.is_empty() {
        return None;
    }
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let mut wrote_styles = false;
    for i in 0..zip.len() {
        let file = zip.by_index_raw(i).ok()?;
        let name = file.name().ok()?.into_owned();
        if let Some(text) = new.get(&name) {
            drop(file);
            wrote_styles |= name == "xl/styles.xml";
            out.start_file(name, zip::write::SimpleFileOptions::default()).ok()?;
            out.write_all(text.as_bytes()).ok()?;
        } else {
            out.raw_copy_file(file).ok()?;
        }
    }
    if let (false, Some(styles)) = (wrote_styles, new.get("xl/styles.xml")) {
        out.start_file("xl/styles.xml", zip::write::SimpleFileOptions::default()).ok()?;
        out.write_all(styles.as_bytes()).ok()?;
    }
    Some(out.finish().ok()?.into_inner())
}

/// The parts of `xl/styles.xml` IronCalc can't do without, each with the
/// empty workbook's own (Excel's defaults), in the order the format lists them.
const STYLE_PARTS: [(&str, &str); 6] = [
    ("fonts", r#"<fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts>"#),
    ("fills", r#"<fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills>"#),
    ("borders", r#"<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>"#),
    ("cellStyleXfs", r#"<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>"#),
    ("cellXfs", r#"<cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs>"#),
    ("cellStyles", r#"<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>"#),
];

/// **`styles` with every part of [`STYLE_PARTS`] it lacks**, added before
/// its end tag (in its root's prefix, if it has one); `None` when it lacks
/// none, or has no end tag to add them before. Pure.
pub fn full_styles(styles: &str) -> Option<String> {
    let start = styles.find("styleSheet")?;
    let open = styles[..start].rfind('<')?;
    let prefix = &styles[open + 1..start];
    if prefix.contains(['/', '>', ' ']) {
        return None;
    }
    let has = |part: &str| {
        let tag = format!("<{prefix}{part}");
        styles.match_indices(&tag).any(|(i, _)| matches!(styles[i + tag.len()..].chars().next(), Some(' ' | '>' | '/' | '\t' | '\r' | '\n')))
    };
    let missing: String = STYLE_PARTS
        .iter()
        .filter(|(part, _)| !has(part))
        .map(|(_, xml)| if prefix.is_empty() { xml.to_string() } else { xml.replace('<', &format!("<{prefix}")).replace(&format!("<{prefix}/"), &format!("</{prefix}")) })
        .collect();
    if missing.is_empty() {
        return None;
    }
    let end = styles.rfind(&format!("</{prefix}styleSheet"))?;
    Some(format!("{}{missing}{}", &styles[..end], &styles[end..]))
}

/// What a formula's result says, as the file kept it: `(text, kind)`.
pub(super) type Kept = HashMap<(u32, i32, i32), (String, Kind)>;

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
pub(super) fn not_known(cell: &CalcCell) -> bool {
    match cell {
        CalcCell::CellFormula { v: FormulaValue::Error { ei, .. }, .. } | CalcCell::ArrayFormula { v: FormulaValue::Error { ei, .. }, .. } => matches!(ei, CalcError::NIMPL | CalcError::NAME),
        _ => false,
    }
}

/// **The workbook as Yap draws it**, calculated.
pub(super) fn book_of(model: &Model, kept: &Kept) -> Book {
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
            // What the formula bar edits, when it isn't what's shown ("7.5" for "£7.50").
            let input = if formula.is_none() && kind == Kind::Number { model.get_localized_cell_content(sheet_no, r, c).ok().filter(|i| *i != text) } else { None };
            let (r, c) = (r as u32, c as u32);
            if kind != Kind::Empty {
                sheet.used.take(r, c);
            }
            sheet.drawn.take(r, c);
            let cell = Cell { col: c, text, kind, formula, input, style };
            match rows.last_mut() {
                Some(row) if row.r == r => row.cells.push(cell),
                _ => rows.push(Row { r, cells: vec![cell] }),
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

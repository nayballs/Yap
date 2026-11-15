//! **A `.csv` or `.tsv`**: one sheet, its values as written. A CSV split on
//! `;` when its first line has more of those than commas (as Excel saves
//! one where the decimal mark is a comma); quotes as RFC 4180 has them.
//! UTF-8 or UTF-16 with its mark, as attachments read text. A value that
//! reads as a number is a number (right-aligned, as Excel shows it), shown as
//! written.

use super::{fit_columns, keep_rows, Book, Cell, Format, Kind, Row, Sheet, Why, MAX_CELLS};

/// **Read one.** Pure.
pub fn read(bytes: &[u8], format: Format) -> Result<Book, Why> {
    let text = crate::chat::attachments::decode(bytes).ok_or(Why::Failed)?;
    let sep = match format {
        Format::Tsv => b'\t',
        _ => separator(&text),
    };
    let mut reader = csv::ReaderBuilder::new().has_headers(false).flexible(true).delimiter(sep).from_reader(text.as_bytes());
    let mut sheet = Sheet { name: "Sheet1".into(), grid: true, ..Default::default() };
    let mut rows: Vec<Row> = Vec::new();
    for (i, record) in reader.records().enumerate() {
        let Ok(record) = record else { continue };
        let r = i as u32 + 1;
        let cells: Vec<Cell> = record
            .iter()
            .enumerate()
            .filter(|(_, v)| !v.is_empty())
            .map(|(c, v)| Cell { col: c as u32 + 1, text: v.to_string(), kind: kind_of(v), formula: None, style: 0 })
            .collect();
        if cells.is_empty() {
            continue;
        }
        for c in &cells {
            sheet.used.take(r, c.col);
        }
        rows.push(Row { r, cells });
    }
    sheet.drawn = sheet.used;
    let (rows, cut, _) = keep_rows(rows, MAX_CELLS);
    sheet.rows = rows;
    sheet.cut_rows = cut;
    fit_columns(&mut sheet);
    let mut book = Book::new(format);
    book.sheets.push(sheet);
    Ok(book)
}

/// A CSV's separator: `;` when the first line has more of those than
/// commas, else `,`. Pure.
pub fn separator(text: &str) -> u8 {
    let first = text.lines().next().unwrap_or("");
    let (commas, semis) = (first.matches(',').count(), first.matches(';').count());
    if semis > commas {
        b';'
    } else {
        b','
    }
}

/// A value's kind: a number when it reads as one (`12`, `-3.5`, `1e3`). Pure.
pub fn kind_of(v: &str) -> Kind {
    let t = v.trim();
    if !t.is_empty() && t.parse::<f64>().is_ok_and(|f| f.is_finite()) && !t.eq_ignore_ascii_case("inf") && !t.eq_ignore_ascii_case("nan") {
        Kind::Number
    } else if t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("false") {
        Kind::Bool
    } else {
        Kind::Text
    }
}

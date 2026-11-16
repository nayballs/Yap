//! **A spreadsheet's cells as text**, read on this PC (`chat/sheets.rs`:
//! `.xlsx`, `.xlsm`, `.xls`, `.xlsb`, `.ods`, `.csv`, `.tsv`): what the model
//! gets for an attached one. Each sheet a Markdown table — column letters
//! across, row numbers down, so "C7" means something to it — after a line
//! with its name, its used range, its frozen panes and merged cells; then
//! its formulas, a run filled down or across said once ("D2:D40: =B2*C2,
//! the same in each row"). Values as the file shows them (its number and
//! date formats); an `.xlsx`'s formulas calculated here.
//!
//! **Sized to the model**: at most [`TOKENS`] (or the box's room, the page's
//! `budget`: [`with_budget`]), shared between the sheets, the small ones
//! whole; a sheet cut keeps its first rows and its last few, with a plain
//! note of what was left out.

use std::cell::Cell as Slot;

use crate::chat::sheets::{self, column_name, Area, Book, Format, Kind, Sheet, Why};

/// The most tokens a spreadsheet's text takes when no room is given (a
/// project's file).
pub const TOKENS: u64 = 40_000;
/// The least it's cut to, whatever the room.
pub const LEAST: u64 = 1_500;
/// A cell's most characters in the table.
const CELL_CHARS: usize = 200;
/// Formula runs listed for a sheet, at most.
const FORMULAS: usize = 300;
/// The last rows a cut sheet still shows (its totals).
const LAST_ROWS: usize = 3;

thread_local! {
    static BUDGET: Slot<Option<u64>> = const { Slot::new(None) };
    /// The most a read may take ([`TOKENS`], or a project's: [`with_most`]).
    static MOST: Slot<u64> = const { Slot::new(TOKENS) };
}

/// Run `f` with `budget` tokens the room for a spreadsheet's text (the
/// box's: what the model has left for the question).
pub fn with_budget<T>(budget: Option<u64>, f: impl FnOnce() -> T) -> T {
    let before = BUDGET.with(|b| b.replace(budget));
    let out = f();
    BUDGET.with(|b| b.set(before));
    out
}

/// Run `f` with `most` tokens the room for a spreadsheet's text, past
/// [`TOKENS`]: a project's file (`chat/projects.rs`), whose big files are
/// searched in parts rather than sent whole.
pub fn with_most<T>(most: u64, f: impl FnOnce() -> T) -> T {
    let before = MOST.with(|m| m.replace(most.max(LEAST)));
    let out = with_budget(Some(most), f);
    MOST.with(|m| m.set(before));
    out
}

/// The tokens this read may take. Pure but for the room set.
pub fn budget() -> u64 {
    let most = MOST.with(Slot::get);
    BUDGET.with(|b| b.get()).map(|b| b.clamp(LEAST, most)).unwrap_or(most)
}

/// **The text of a spreadsheet** called `name`, or why it can't be read (a
/// plain sentence, the file named). Blocking.
pub fn text(name: &str, bytes: Vec<u8>) -> Result<String, String> {
    let book = match sheets::read(name, bytes) {
        Ok(book) => book,
        Err(Why::Locked) => return Err(format!("{name} is password-protected, so Yap can't read it.")),
        Err(Why::Slow) => return Err(format!("{name} took too long to read.")),
        Err(Why::Failed) => return Err(format!("Yap couldn't read the cells of {name}.")),
    };
    if book.values() == 0 {
        return Err(format!("{name} has nothing in its cells."));
    }
    Ok(of_book(&book, budget()))
}

/// **A workbook as text** in about `tokens`. Pure.
pub fn of_book(book: &Book, tokens: u64) -> String {
    let chars = (tokens as f64 * 3.4) as usize;
    let head = heading(book);
    let full: Vec<String> = book.sheets.iter().enumerate().map(|(i, s)| of_sheet(book, i, s, usize::MAX)).collect();
    let need: Vec<usize> = full.iter().map(|t| t.len() + 2).collect();
    let shares = shares(&need, chars.saturating_sub(head.len()));
    let mut out = head;
    for (i, sheet) in book.sheets.iter().enumerate() {
        out.push_str("\n\n");
        if need[i] <= shares[i] {
            out.push_str(&full[i]);
        } else {
            out.push_str(&of_sheet(book, i, sheet, shares[i]));
        }
    }
    out
}

/// `room` shared out: each `need` whole while it fits, the rest split
/// evenly among the bigger ones (smallest first). Pure.
pub fn shares(need: &[usize], room: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..need.len()).collect();
    order.sort_by_key(|&i| need[i]);
    let mut out = vec![0; need.len()];
    let mut left = room;
    for (k, &i) in order.iter().enumerate() {
        let each = left / (need.len() - k);
        out[i] = need[i].min(each);
        left -= out[i];
    }
    out
}

/// The workbook's first line. Pure.
fn heading(book: &Book) -> String {
    let what = match book.format {
        Format::Csv | Format::Tsv => return "One table; its values as written in the file.".to_string(),
        _ => book.format.said(),
    };
    let names: Vec<String> = book.sheets.iter().map(|s| format!("\"{}\"{}", s.name, if s.hidden { " (hidden)" } else { "" })).collect();
    let results = if book.calculated { "formulas' results calculated on this PC" } else { "formulas' results as the file keeps them" };
    format!("{} with {} sheet{}: {}. Values as the file formats them; {results}.", capital(what), book.sheets.len(), if book.sheets.len() == 1 { "" } else { "s" }, names.join(", "))
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// **One sheet as text** in at most `room` characters (`usize::MAX`: all
/// of it). Pure.
pub fn of_sheet(book: &Book, index: usize, sheet: &Sheet, room: usize) -> String {
    let mut out = String::new();
    let used = sheet.used;
    let delimited = !book.format.is_workbook();
    // The sheet's line.
    if delimited {
        out.push_str(&format!("{} rows × {} columns ({}).", used.bottom, used.right, Area { top: 1, left: 1, ..used }.a1()));
    } else {
        out.push_str(&format!("## Sheet {} of {}: \"{}\"{}", index + 1, book.sheets.len(), sheet.name, if sheet.hidden { " (hidden)" } else { "" }));
        if used.is_empty() {
            out.push_str("\nEmpty.");
            return out;
        }
        let mut facts = vec![format!("Used range {} ({} rows × {} columns)", used.a1(), used.rows(), used.cols())];
        if sheet.frozen_rows > 0 || sheet.frozen_cols > 0 {
            let mut f = Vec::new();
            if sheet.frozen_rows > 0 {
                f.push(if sheet.frozen_rows == 1 { "row 1".to_string() } else { format!("rows 1–{}", sheet.frozen_rows) });
            }
            if sheet.frozen_cols > 0 {
                f.push(if sheet.frozen_cols == 1 { "column A".to_string() } else { format!("columns A–{}", column_name(sheet.frozen_cols)) });
            }
            facts.push(format!("frozen: {}", f.join(" and ")));
        }
        if !sheet.merges.is_empty() {
            let list: Vec<String> = sheet.merges.iter().take(30).map(|m| Area { top: m.row, left: m.col, bottom: m.row + m.rows - 1, right: m.col + m.cols - 1 }.a1()).collect();
            facts.push(format!("merged: {}{}", list.join(", "), if sheet.merges.len() > 30 { format!(" and {} more", sheet.merges.len() - 30) } else { String::new() }));
        }
        out.push('\n');
        out.push_str(&facts.join("; "));
        out.push('.');
    }
    if used.is_empty() {
        return out;
    }
    // The table: its head, then rows while they fit.
    let left = if delimited { 1 } else { used.left };
    let cols: Vec<u32> = (left..=used.right).collect();
    out.push_str("\n\n| |");
    for c in &cols {
        out.push_str(&format!(" {} |", column_name(*c)));
    }
    out.push_str("\n|---|");
    for _ in &cols {
        out.push_str("---|");
    }
    let lines: Vec<String> = sheet.rows.iter().filter(|r| r.cells.iter().any(|c| c.kind != Kind::Empty)).map(|r| row_line(r, &cols)).collect();
    let formulas = formula_lines(sheet);
    let formulas_text = if formulas.is_empty() {
        String::new()
    } else {
        let said = if book.calculated { "Formulas (results above calculated on this PC):" } else { "Formulas:" };
        format!("\n\n{said}\n{}", formulas.join("\n"))
    };
    let whole: usize = out.len() + lines.iter().map(|l| l.len() + 1).sum::<usize>() + formulas_text.len();
    if whole <= room {
        for l in &lines {
            out.push('\n');
            out.push_str(l);
        }
        out.push_str(&formulas_text);
        if sheet.cut_rows > 0 {
            out.push_str(&format!("\n\n(Its last {} rows weren't read: Yap reads at most {} cells of a workbook.)", thousands(sheet.cut_rows as u64), thousands(sheets::MAX_CELLS as u64)));
        }
        return out;
    }
    // Cut: the formulas get at most a quarter of the room; the rows the rest,
    // first rows first and the last few kept.
    let formulas_room = (room / 4).min(formulas_text.len());
    let mut rows_room = room.saturating_sub(out.len() + formulas_room + 200);
    let tail: Vec<&String> = lines.iter().rev().take(LAST_ROWS).collect::<Vec<_>>().into_iter().rev().collect();
    let tail_len: usize = tail.iter().map(|l| l.len() + 1).sum();
    let keep_tail = lines.len() > LAST_ROWS * 4 && tail_len * 4 < rows_room;
    if keep_tail {
        rows_room -= tail_len;
    }
    let mut shown = 0;
    let mut used_room = 0;
    for l in &lines {
        if used_room + l.len() + 1 > rows_room && shown > 0 {
            break;
        }
        out.push('\n');
        out.push_str(l);
        used_room += l.len() + 1;
        shown += 1;
    }
    let end = if keep_tail { lines.len() - LAST_ROWS } else { lines.len() };
    if shown < end {
        let first_left = sheet.rows.iter().filter(|r| r.cells.iter().any(|c| c.kind != Kind::Empty)).nth(shown).map(|r| r.r).unwrap_or(0);
        let last_left = sheet.rows.iter().filter(|r| r.cells.iter().any(|c| c.kind != Kind::Empty)).nth(end - 1).map(|r| r.r).unwrap_or(first_left);
        out.push_str(&format!(
            "\n\n(Rows {}–{} aren't here: {} rows left out to fit the model{}. Ask for a part of them and the person can attach it.)\n",
            first_left,
            last_left,
            thousands((end - shown) as u64),
            if keep_tail { "; the sheet's last rows follow" } else { "" }
        ));
        if keep_tail {
            out.push_str("\n| |");
            for c in &cols {
                out.push_str(&format!(" {} |", column_name(*c)));
            }
            out.push_str("\n|---|");
            for _ in &cols {
                out.push_str("---|");
            }
            for l in tail {
                out.push('\n');
                out.push_str(l);
            }
        }
    } else if keep_tail {
        for l in tail {
            out.push('\n');
            out.push_str(l);
        }
    }
    if !formulas_text.is_empty() {
        if formulas_text.len() <= formulas_room {
            out.push_str(&formulas_text);
        } else {
            let mut cut = String::new();
            let mut n = 0;
            for f in &formulas {
                if cut.len() + f.len() + 1 > formulas_room.saturating_sub(120) && n > 0 {
                    break;
                }
                cut.push('\n');
                cut.push_str(f);
                n += 1;
            }
            out.push_str(&format!("\n\nFormulas{}:{cut}\n({} more formula runs left out.)", if book.calculated { " (results above calculated on this PC)" } else { "" }, formulas.len() - n));
        }
    }
    if sheet.cut_rows > 0 {
        out.push_str(&format!("\n(Its last {} rows weren't read: Yap reads at most {} cells of a workbook.)", thousands(sheet.cut_rows as u64), thousands(sheets::MAX_CELLS as u64)));
    }
    out
}

/// A row as a table's line: its number, then each column's text. Pure.
fn row_line(row: &sheets::Row, cols: &[u32]) -> String {
    let mut line = format!("| {} |", row.r);
    let mut at = 0;
    for c in cols {
        while at < row.cells.len() && row.cells[at].col < *c {
            at += 1;
        }
        let text = match row.cells.get(at) {
            Some(cell) if cell.col == *c && cell.kind != Kind::Empty => tidy(&cell.text),
            _ => String::new(),
        };
        line.push(' ');
        line.push_str(&text);
        line.push_str(" |");
    }
    line
}

/// A cell's text as a table can hold it: `|` escaped, its lines joined,
/// long ones cut. Pure.
pub fn tidy(text: &str) -> String {
    let one: String = text.replace('\r', "").replace('\n', " / ").replace('|', "\\|");
    if one.chars().count() > CELL_CHARS {
        format!("{}…", one.chars().take(CELL_CHARS).collect::<String>())
    } else {
        one
    }
}

/// `n` with commas. Pure.
pub fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

// ---- formulas -----------------------------------------------------------------------------------

/// **A sheet's formulas**, a line each run: a column's cells filled down
/// with the same formula (its references moving with the row) said once,
/// then a row's filled across, then the rest one by one. Pure.
pub fn formula_lines(sheet: &Sheet) -> Vec<String> {
    // (row, col, formula, its shape: references relative to its own cell)
    let mut all: Vec<(u32, u32, &str, String)> = Vec::new();
    for row in &sheet.rows {
        for cell in &row.cells {
            if let Some(f) = &cell.formula {
                all.push((row.r, cell.col, f.as_str(), shape(f, row.r, cell.col)));
            }
        }
    }
    if all.is_empty() {
        return Vec::new();
    }
    let mut done = vec![false; all.len()];
    let mut runs: Vec<(u32, u32, String)> = Vec::new();
    // Down: by column, then row.
    let mut by_col: Vec<usize> = (0..all.len()).collect();
    by_col.sort_by_key(|&i| (all[i].1, all[i].0));
    let mut i = 0;
    while i < by_col.len() {
        let a = by_col[i];
        let mut j = i + 1;
        while j < by_col.len() {
            let (p, q) = (by_col[j - 1], by_col[j]);
            if all[q].1 == all[a].1 && all[q].0 == all[p].0 + 1 && all[q].3 == all[a].3 {
                j += 1;
            } else {
                break;
            }
        }
        if j - i >= 2 {
            let last = by_col[j - 1];
            let area = Area { top: all[a].0, left: all[a].1, bottom: all[last].0, right: all[a].1 };
            runs.push((all[a].0, all[a].1, format!("{}: {} (the same in each row, filled down)", area.a1(), all[a].2)));
            for &k in &by_col[i..j] {
                done[k] = true;
            }
        }
        i = j;
    }
    // Across: by row, then column, of the ones left.
    let mut by_row: Vec<usize> = (0..all.len()).filter(|&k| !done[k]).collect();
    by_row.sort_by_key(|&k| (all[k].0, all[k].1));
    let mut i = 0;
    while i < by_row.len() {
        let a = by_row[i];
        let mut j = i + 1;
        while j < by_row.len() {
            let (p, q) = (by_row[j - 1], by_row[j]);
            if all[q].0 == all[a].0 && all[q].1 == all[p].1 + 1 && all[q].3 == all[a].3 {
                j += 1;
            } else {
                break;
            }
        }
        if j - i >= 2 {
            let last = by_row[j - 1];
            let area = Area { top: all[a].0, left: all[a].1, bottom: all[a].0, right: all[last].1 };
            runs.push((all[a].0, all[a].1, format!("{}: {} (the same in each column, filled across)", area.a1(), all[a].2)));
        } else {
            let one = Area { top: all[a].0, left: all[a].1, bottom: all[a].0, right: all[a].1 };
            runs.push((all[a].0, all[a].1, format!("{}: {}", one.a1(), all[a].2)));
        }
        i = j;
    }
    runs.sort_by_key(|r| (r.0, r.1));
    let more = runs.len().saturating_sub(FORMULAS);
    let mut lines: Vec<String> = runs.into_iter().take(FORMULAS).map(|r| r.2).collect();
    if more > 0 {
        lines.push(format!("(and {} more formulas)", thousands(more as u64)));
    }
    lines
}

/// **A formula's shape**: each cell reference made relative to the cell
/// it's in (`B2` in D2 → `R[0]C[-2]`), `$` parts kept as they are, quoted
/// text and function names left alone — two cells filled from one formula
/// have the same shape. Pure.
pub fn shape(formula: &str, row: u32, col: u32) -> String {
    let b = formula.as_bytes();
    let mut out = String::with_capacity(formula.len());
    let mut i = 0;
    while i < b.len() {
        let ch = b[i];
        // Quoted text, and a quoted sheet name, as they are.
        if ch == b'"' || ch == b'\'' {
            let close = b[i + 1..].iter().position(|&x| x == ch).map(|p| i + 1 + p).unwrap_or(b.len() - 1);
            out.push_str(&formula[i..=close]);
            i = close + 1;
            continue;
        }
        let starts = (ch == b'$' || ch.is_ascii_alphabetic()) && (i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_' || b[i - 1] == b'.'));
        if starts {
            if let Some((len, text)) = reference(&formula[i..], row, col) {
                out.push_str(&text);
                i += len;
                continue;
            }
            // A word (a function, a name): as it is.
            let mut j = i;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_' || b[j] == b'.' || b[j] == b'$') {
                j += 1;
            }
            out.push_str(&formula[i..j.max(i + 1)]);
            i = j.max(i + 1);
            continue;
        }
        // (A multi-byte character: copied whole.)
        let len = formula[i..].chars().next().map(char::len_utf8).unwrap_or(1);
        out.push_str(&formula[i..i + len]);
        i += len;
    }
    out
}

/// A cell reference at the start of `s` (`$B$2`, `B2`, `AB12`): its length
/// and its relative form; none for a function's name (`LOG10(`) or a longer
/// word. Pure.
fn reference(s: &str, row: u32, col: u32) -> Option<(usize, String)> {
    let b = s.as_bytes();
    let mut i = 0;
    let col_abs = b.first() == Some(&b'$');
    if col_abs {
        i += 1;
    }
    let letters_start = i;
    while i < b.len() && b[i].is_ascii_alphabetic() && i - letters_start < 3 {
        i += 1;
    }
    let letters = &s[letters_start..i];
    if letters.is_empty() {
        return None;
    }
    let row_abs = b.get(i) == Some(&b'$');
    if row_abs {
        i += 1;
    }
    let digits_start = i;
    while i < b.len() && b[i].is_ascii_digit() && i - digits_start < 7 {
        i += 1;
    }
    let digits = &s[digits_start..i];
    if digits.is_empty() {
        return None;
    }
    // Followed by more of a word, or a "(": not a reference.
    if b.get(i).is_some_and(|&x| x.is_ascii_alphanumeric() || x == b'_' || x == b'(' || x == b'.') {
        return None;
    }
    let c = letters.bytes().fold(0u32, |n, x| n * 26 + (x.to_ascii_uppercase() - b'A' + 1) as u32);
    let r: u32 = digits.parse().ok()?;
    if c == 0 || c > 16_384 || r == 0 {
        return None;
    }
    let cp = if col_abs { format!("C{c}") } else { format!("C[{}]", c as i64 - col as i64) };
    let rp = if row_abs { format!("R{r}") } else { format!("R[{}]", r as i64 - row as i64) };
    Some((i, format!("{rp}{cp}")))
}

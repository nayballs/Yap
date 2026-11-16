//! **Rows and columns put in or taken out** (16 Nov 2026): every reference
//! that points at them moved the way Excel moves it, in a formula's text,
//! a list of areas (`sqref`) or one area (`ref`). Pure: text in, text out.
//! The package's parts are rewritten with it in `shift/parts.rs`.
//!
//! **Excel's rules** (what its Insert and Delete do, the same in every part
//! of the file — formulas, defined names, merged cells, conditional formats,
//! data validation, charts' series):
//! - **Put in** `n` lines before line `at`: a line at or past `at` moves on
//!   by `n`; an area that starts before `at` and ends at or past it grows by
//!   `n` (a `SUM(D2:D5)` with a row put in before row 4 is `SUM(D2:D6)`); an
//!   area ending just before `at` doesn't (Excel's own). `$` changes nothing:
//!   it's about copying, not inserting. A whole column (`A:A`) doesn't move
//!   for rows, a whole row (`3:3`) doesn't for columns.
//! - **Taken out** lines `at..at+n`: a single cell in them is `#REF!`; an area
//!   loses the lines it had there (`SUM(D2:D6)` without row 4 is
//!   `SUM(D2:D5)`), and is `#REF!` only when all of it went. (IronCalc's own
//!   delete makes an area's end in them `#REF!`: Excel shrinks it.)
//! - Only references to **the sheet changed**: one named (`Budget!B3`,
//!   `'Q1 plan'!B3:B9`), or one with no sheet in a formula on that sheet.
//! - Never moved: names (a defined name is moved where it's defined),
//!   tables' structured references (`Table1[Qty]`: by name), another
//!   workbook's references (`[1]Sheet1!A1`), text in quotes, `INDIRECT`'s
//!   text (Excel doesn't either).
//! - **Unsure, so refused** ([`Unsure`]): a reference across sheets
//!   (`Jan:Mar!B3`, a 3D reference), one with no sheet where there's no
//!   sheet to take it from (a defined name, a chart), text that doesn't
//!   read as a formula. A refusal is said plainly and nothing is changed.

pub mod parts;

use super::patch::{area_of, place_of};
use super::{column_name, Area};

pub const LAST_ROW: u32 = 1_048_576;
pub const LAST_COL: u32 = 16_384;

/// Rows or columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Rows,
    Cols,
}

/// **A change of lines**: `n` rows or columns put in before `at` (`insert`),
/// or taken out from `at` on (from 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shift {
    pub axis: Axis,
    pub at: u32,
    pub n: u32,
    pub insert: bool,
}

/// Why a reference couldn't be moved for sure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsure(pub &'static str);

impl Shift {
    fn last(&self) -> u32 {
        match self.axis {
            Axis::Rows => LAST_ROW,
            Axis::Cols => LAST_COL,
        }
    }

    /// **A line after the change**; none: taken out (or pushed off the sheet).
    pub fn line(&self, x: u32) -> Option<u32> {
        if self.insert {
            if x < self.at {
                Some(x)
            } else {
                Some(x + self.n).filter(|y| *y <= self.last())
            }
        } else if x < self.at {
            Some(x)
        } else if x < self.at + self.n {
            None
        } else {
            Some(x - self.n)
        }
    }

    /// **Lines `a..=b` after the change**: grown by lines put in inside
    /// them, shrunk by lines taken out of them; none: all of them went.
    pub fn span(&self, a: u32, b: u32) -> Option<(u32, u32)> {
        let (a, b) = (a.min(b), a.max(b));
        if self.insert {
            let a2 = if a >= self.at { a + self.n } else { a };
            let b2 = if b >= self.at { b + self.n } else { b };
            (a2 <= self.last()).then(|| (a2, b2.min(self.last())))
        } else {
            let end = self.at + self.n;
            let a2 = if a < self.at { a } else if a < end { self.at } else { a - self.n };
            let b2 = if b < self.at { b } else if b < end { self.at - 1 } else { b - self.n };
            (b2 >= a2 && b2 >= 1).then_some((a2, b2))
        }
    }

    /// Whether lines `a..=b` would be cut apart (some moved and some not,
    /// or some taken out and some not): an array formula, a table's columns.
    pub fn splits(&self, a: u32, b: u32) -> bool {
        let (a, b) = (a.min(b), a.max(b));
        if self.insert {
            a < self.at && self.at <= b
        } else {
            let end = self.at + self.n;
            let overlaps = a < end && b >= self.at;
            overlaps && !(a >= self.at && b < end)
        }
    }

    /// **An area after the change**; none: all of it went.
    pub fn area(&self, ar: Area) -> Option<Area> {
        match self.axis {
            Axis::Rows => self.span(ar.top, ar.bottom).map(|(t, b)| Area { top: t, bottom: b, ..ar }),
            Axis::Cols => self.span(ar.left, ar.right).map(|(l, r)| Area { left: l, right: r, ..ar }),
        }
    }

    /// A cell after the change (row, column); none: taken out.
    pub fn cell(&self, (r, c): (u32, u32)) -> Option<(u32, u32)> {
        match self.axis {
            Axis::Rows => self.line(r).map(|r| (r, c)),
            Axis::Cols => self.line(c).map(|c| (r, c)),
        }
    }

    /// Whether the change moves or takes out cell (`r`, `c`).
    pub fn moves(&self, at: (u32, u32)) -> bool {
        self.cell(at) != Some(at)
    }
}

// ---- areas: `ref`, `sqref` ----------------------------------------------------------------------

/// **One area (`A1:D5`, `B2`) after the change**; none: all of it went.
/// `Err`: not an area. Pure.
pub fn area(text: &str, s: &Shift) -> Result<Option<String>, Unsure> {
    let ar = area_of(text).ok_or(Unsure("an area Yap couldn't read"))?;
    Ok(s.area(ar).map(|a| a.a1()))
}

/// **A list of areas (`sqref`: "A1:A9 C3") after the change**: the ones
/// all taken out left out (empty: none left). As written when nothing moved. Pure.
pub fn sqref(text: &str, s: &Shift) -> Result<String, Unsure> {
    let mut out = Vec::new();
    let mut changed = false;
    for part in text.split_whitespace() {
        let ar = area_of(part).ok_or(Unsure("an area Yap couldn't read"))?;
        match s.area(ar) {
            Some(a) => {
                changed |= a != ar;
                out.push(a.a1());
            }
            None => changed = true,
        }
    }
    Ok(if changed { out.join(" ") } else { text.to_string() })
}

// ---- formulas ---------------------------------------------------------------------------------

/// **A formula's text after the change** to sheet `target`: `host` is the
/// sheet it's on (its references with no sheet are that sheet's; none, a
/// defined name's or a chart's: such a reference is unsure). The text as
/// written when nothing in it moved, and whether it refers to `target` at all.
#[derive(Debug, Clone, PartialEq)]
pub struct Moved {
    pub text: String,
    pub touches: bool,
}

/// A reference's parts as written: `$` before the column, the column, `$`
/// before the row, the row (none: a whole row or column).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Part {
    col_abs: bool,
    col: Option<u32>,
    row_abs: bool,
    row: Option<u32>,
}

impl Part {
    fn text(&self) -> String {
        let mut s = String::new();
        if let Some(c) = self.col {
            if self.col_abs {
                s.push('$');
            }
            s.push_str(&column_name(c));
        }
        if let Some(r) = self.row {
            if self.row_abs {
                s.push('$');
            }
            s.push_str(&r.to_string());
        }
        s
    }
}

/// A cell (`$A$1`), a column (`$A`), or a row (`$3`), as written. Pure.
fn part_of(word: &str) -> Option<Part> {
    let b = word.as_bytes();
    let mut i = 0;
    let col_abs = b.first() == Some(&b'$');
    if col_abs {
        i += 1;
    }
    let letters = b[i..].iter().take_while(|c| c.is_ascii_alphabetic()).count();
    let col = if letters > 0 {
        if letters > 3 {
            return None;
        }
        let n = b[i..i + letters].iter().fold(0u32, |n, ch| n * 26 + (ch.to_ascii_uppercase() - b'A' + 1) as u32);
        if !(1..=LAST_COL).contains(&n) {
            return None;
        }
        i += letters;
        Some(n)
    } else {
        None
    };
    let mut row_abs = false;
    if b.get(i) == Some(&b'$') {
        // `$3` alone, or `A$3`.
        if col.is_none() && col_abs {
            return None;
        }
        row_abs = true;
        i += 1;
    }
    let digits = b[i..].iter().take_while(|c| c.is_ascii_digit()).count();
    let row = if digits > 0 {
        if digits > 7 || (digits > 1 && b[i] == b'0') {
            return None;
        }
        let n: u32 = word[i..i + digits].parse().ok()?;
        if !(1..=LAST_ROW).contains(&n) {
            return None;
        }
        i += digits;
        Some(n)
    } else {
        None
    };
    if i != b.len() || (col.is_none() && row.is_none()) || (row_abs && row.is_none()) {
        return None;
    }
    // A row alone written `$3`: the `$` read as the column's.
    if col.is_none() && col_abs {
        return Some(Part { col_abs: false, col: None, row_abs: true, row });
    }
    Some(Part { col_abs, col, row_abs, row })
}

/// What a reference is: a cell, an area of two cells, whole columns, whole rows.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Ref {
    Cell(Part),
    Area(Part, Part),
}

impl Ref {
    /// Read from its words (`A1`, or `A1` `:` `B5`). Pure.
    fn of(first: &str, second: Option<&str>) -> Option<Ref> {
        let a = part_of(first)?;
        match second {
            None => (a.col.is_some() && a.row.is_some()).then_some(Ref::Cell(a)),
            Some(w) => {
                let b = part_of(w)?;
                // Both cells, both columns, or both rows.
                let kind = |p: &Part| (p.col.is_some(), p.row.is_some());
                (kind(&a) == kind(&b)).then_some(Ref::Area(a, b))
            }
        }
    }

    /// After the change: its text, none for `#REF!`.
    fn moved(&self, s: &Shift) -> Option<String> {
        let line = |p: &Part| match s.axis {
            Axis::Rows => p.row,
            Axis::Cols => p.col,
        };
        let with = |p: Part, v: u32| match s.axis {
            Axis::Rows => Part { row: Some(v), ..p },
            Axis::Cols => Part { col: Some(v), ..p },
        };
        match *self {
            Ref::Cell(p) => {
                let v = line(&p)?;
                Some(with(p, s.line(v)?).text())
            }
            Ref::Area(a, b) => match (line(&a), line(&b)) {
                // A whole row for columns, a whole column for rows: as it is.
                (None, None) => Some(format!("{}:{}", a.text(), b.text())),
                (Some(x), Some(y)) => {
                    let (lo, hi) = s.span(x, y)?;
                    let (first, second) = if x <= y { (with(a, lo), with(b, hi)) } else { (with(b, lo), with(a, hi)) };
                    // (Written first-to-last, as Excel writes an area; each end keeps its own `$`.)
                    Some(format!("{}:{}", first.text(), second.text()))
                }
                _ => None,
            },
        }
    }

    fn text(&self) -> String {
        match self {
            Ref::Cell(p) => p.text(),
            Ref::Area(a, b) => format!("{}:{}", a.text(), b.text()),
        }
    }
}

/// A character a name, a sheet's name or a reference may have unquoted.
fn wordy(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '_' | '.' | '\\' | '$' | '?')
}

/// **Formula `text`, its references to `target` moved by `s`** (see
/// [`Moved`]). Pure.
pub fn formula(text: &str, host: Option<&str>, target: &str, s: &Shift) -> Result<Moved, Unsure> {
    let src: Vec<char> = text.chars().collect();
    let n = src.len();
    let mut out = String::with_capacity(text.len() + 8);
    let mut i = 0;
    let mut touches = false;
    let mut changed = false;
    // The last reference's place in `out` came from another workbook (`[1]Sheet!A1`): never moved.
    let mut external = false;
    let same = |a: &str, b: &str| a.to_lowercase() == b.to_lowercase();
    // A word (a run of `wordy` characters) from `i`.
    let word_at = |i: usize| -> (String, usize) {
        let mut j = i;
        while j < n && wordy(src[j]) {
            j += 1;
        }
        (src[i..j].iter().collect(), j)
    };
    while i < n {
        let ch = src[i];
        match ch {
            '"' => {
                // Text: as it is (`""` a quote inside).
                out.push('"');
                i += 1;
                loop {
                    if i >= n {
                        return Err(Unsure("a formula Yap couldn't read"));
                    }
                    out.push(src[i]);
                    if src[i] == '"' {
                        if i + 1 < n && src[i + 1] == '"' {
                            out.push('"');
                            i += 2;
                            continue;
                        }
                        i += 1;
                        break;
                    }
                    i += 1;
                }
            }
            '[' => {
                // A table's columns (`Table1[Qty]`, `[@Qty]`) or another workbook (`[1]`): as written.
                let start = i;
                let mut depth = 0;
                while i < n {
                    match src[i] {
                        '\'' => i += 1,
                        '[' => depth += 1,
                        ']' => {
                            depth -= 1;
                            if depth == 0 {
                                i += 1;
                                break;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                if depth != 0 {
                    return Err(Unsure("a formula Yap couldn't read"));
                }
                let inner: String = src[start + 1..i - 1].iter().collect();
                external = !inner.is_empty() && (inner.chars().all(|c| c.is_ascii_digit()) || inner.contains('.'));
                out.extend(&src[start..i]);
                continue;
            }
            '#' => {
                // An error (`#REF!`, `#N/A`), or a spill's `#` after a reference.
                out.push('#');
                i += 1;
                while i < n && (src[i].is_ascii_alphanumeric() || matches!(src[i], '/' | '_')) {
                    out.push(src[i]);
                    i += 1;
                }
                if i < n && matches!(src[i], '!' | '?') {
                    out.push(src[i]);
                    i += 1;
                }
            }
            '\'' => {
                // A quoted sheet's name, then `!` and its reference.
                let mut name = String::new();
                let mut j = i + 1;
                loop {
                    if j >= n {
                        return Err(Unsure("a formula Yap couldn't read"));
                    }
                    if src[j] == '\'' {
                        if j + 1 < n && src[j + 1] == '\'' {
                            name.push('\'');
                            j += 2;
                            continue;
                        }
                        j += 1;
                        break;
                    }
                    name.push(src[j]);
                    j += 1;
                }
                if j >= n || src[j] != '!' {
                    return Err(Unsure("a formula Yap couldn't read"));
                }
                let raw: String = src[i..=j].iter().collect();
                let ext = name.starts_with('[') || external;
                if name.contains(':') && !ext {
                    return Err(Unsure("a reference across sheets"));
                }
                out.push_str(&raw);
                i = j + 1;
                let mine = !ext && same(&name, target);
                i = qualified(&src, i, mine, s, &mut out, &mut touches, &mut changed)?;
                external = false;
                continue;
            }
            c if wordy(c) => {
                let (word, j) = word_at(i);
                // A sheet's name: `Budget!B3`; across sheets `Jan:Mar!B3`.
                if j < n && src[j] == '!' {
                    let ext = external;
                    out.push_str(&word);
                    out.push('!');
                    let mine = !ext && same(&word, target);
                    i = qualified(&src, j + 1, mine, s, &mut out, &mut touches, &mut changed)?;
                    external = false;
                    continue;
                }
                if j < n && src[j] == ':' {
                    let (w2, k) = word_at(j + 1);
                    if !w2.is_empty() && k < n && src[k] == '!' && !external {
                        return Err(Unsure("a reference across sheets"));
                    }
                }
                external = false;
                // A function (`LOG10(`), a table (`Table1[`): as written.
                if j < n && (src[j] == '(' || src[j] == '[') {
                    out.push_str(&word);
                    i = j;
                    continue;
                }
                let (r, next) = reference(&src, i, &word, j);
                match r {
                    Some(r) => {
                        let mine = match host {
                            Some(h) => same(h, target),
                            None => return Err(Unsure("a reference with no sheet in a defined name or a chart")),
                        };
                        if mine {
                            touches = true;
                            let moved = r.moved(s).unwrap_or_else(|| "#REF!".to_string());
                            let was: String = src[i..next].iter().collect();
                            if moved != r.text() {
                                changed = true;
                                out.push_str(&moved);
                            } else {
                                out.push_str(&was);
                            }
                        } else {
                            out.extend(&src[i..next]);
                        }
                        i = next;
                    }
                    None => {
                        // A name, TRUE, a number: as written.
                        out.push_str(&word);
                        i = j;
                    }
                }
            }
            _ => {
                out.push(ch);
                i += 1;
            }
        }
    }
    Ok(Moved { text: if changed { out } else { text.to_string() }, touches })
}

/// The reference that starts with `word` (at `i`, ending at `j`): a cell,
/// or an area with the word after a `:`; its end. None: not a reference.
fn reference(src: &[char], i: usize, word: &str, j: usize) -> (Option<Ref>, usize) {
    let n = src.len();
    if j < n && src[j] == ':' {
        let mut k = j + 1;
        while k < n && wordy(src[k]) {
            k += 1;
        }
        let second: String = src[j + 1..k].iter().collect();
        // (`A1:B5`, `A:C`, `3:5` — not followed by a function's `(` or a sheet's `!`.)
        if !second.is_empty() && !(k < n && matches!(src[k], '(' | '!' | '[')) {
            if let Some(r) = Ref::of(word, Some(&second)) {
                return (Some(r), k);
            }
        }
    }
    let _ = i;
    (Ref::of(word, None), j)
}

/// The reference after a sheet's `!` (from `i`), moved when it's `mine`;
/// where it ends.
fn qualified(src: &[char], i: usize, mine: bool, s: &Shift, out: &mut String, touches: &mut bool, changed: &mut bool) -> Result<usize, Unsure> {
    let n = src.len();
    if i < n && src[i] == '#' {
        // `Budget!#REF!`: as written.
        return Ok(i);
    }
    let mut j = i;
    while j < n && wordy(src[j]) {
        j += 1;
    }
    if j == i {
        return Err(Unsure("a formula Yap couldn't read"));
    }
    let word: String = src[i..j].iter().collect();
    let (r, next) = reference(src, i, &word, j);
    let Some(r) = r else {
        // A name the sheet has (`Budget!Rate`): as written.
        out.push_str(&word);
        return Ok(j);
    };
    if mine {
        *touches = true;
        let moved = r.moved(s).unwrap_or_else(|| "#REF!".to_string());
        if moved != r.text() {
            *changed = true;
            out.push_str(&moved);
            return Ok(next);
        }
    }
    out.extend(&src[i..next]);
    Ok(next)
}

/// A cell's name after the change (`B7` → `B8`); none: taken out. Pure.
pub fn cell_name(a1: &str, s: &Shift) -> Option<Option<String>> {
    let at = place_of(a1)?;
    Some(s.cell(at).map(|(r, c)| format!("{}{r}", column_name(c))))
}

#[cfg(test)]
mod tests;

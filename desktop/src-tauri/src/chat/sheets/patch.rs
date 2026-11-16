//! **An `.xlsx` saved by changing only what changed** (16 Nov 2026): the
//! cells the person edited are written into the file's own XML, in place,
//! and every other byte of the package stays as it was — the other sheets,
//! the styles, charts, pictures, comments, data validation, conditional
//! formatting, tables, macros (`.xlsm`), print settings, anything Yap
//! doesn't know about.
//!
//! **Why not IronCalc's writer** (`ironcalc::export`): it writes a new
//! package from what IronCalc models, so whatever it doesn't model is lost
//! — charts, drawings and pictures, comments (it reads them but never
//! writes them), data validation, autofilters, tables, pivot tables,
//! sheet protection, page setup and print areas' settings, tab colours,
//! rich text inside a cell, a 1904 date system, and an `.xlsm`'s macros
//! (with the wrong content type, so Excel won't open it). A file a script
//! made today may have none of those; a file the person keeps for years
//! usually has some. Patching keeps all of it, so a save never has to ask
//! what may go.
//!
//! What's written for each cell ([`Put`]): its value (a number, a text as
//! an inline string, true/false, an error) or its formula (Excel's form,
//! from IronCalc's parse) with its result as calculated here; its style
//! (`s`). Every other formula's result is brought up to date too
//! ([`SheetPatch::results`]), so an app that reads the results without
//! calculating (calamine, pandas) sees the new total, and the workbook asks
//! Excel to calculate everything when it opens ([`workbook_xml`]'s
//! `fullCalcOnLoad`). Excel's calculation chain is removed ([`without_calc_chain`]):
//! it lists formula cells, it may list one that isn't any more, and Excel
//! makes it again.
//!
//! Pure: text in, text out; the zip in `save.rs`.

use std::collections::{BTreeMap, HashMap};

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use super::Area;

/// A cell's value as the file keeps it.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    Text(String),
    Bool(bool),
    /// Excel's name for it: `#DIV/0!`.
    Error(String),
}

/// **What a cell becomes.**
#[derive(Debug, Clone, PartialEq)]
pub enum Put {
    /// Nothing (a style only, when it has one; else the cell goes).
    Empty,
    Value(Value),
    /// A formula in Excel's form (no `=`) and its result (none: not known).
    Formula(String, Option<Value>),
}

/// A cell to write: what it holds and its style (`s`; 0: none).
#[derive(Debug, Clone, PartialEq)]
pub struct CellPut {
    pub put: Put,
    pub style: u32,
}

/// **One sheet's changes**, by (row, column) from 1.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SheetPatch {
    pub puts: BTreeMap<(u32, u32), CellPut>,
    /// Formula cells left as they are, their results as calculated now.
    pub results: HashMap<(u32, u32), Value>,
}

impl SheetPatch {
    pub fn is_empty(&self) -> bool {
        self.puts.is_empty() && self.results.is_empty()
    }
}

/// What a sheet's XML holds that a change has to mind ([`scan`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scan {
    /// Shared formulas: their id → the cell that spells it out and the
    /// cells that share it (the first one included).
    pub shared: HashMap<u32, Shared>,
    /// Array formulas and data tables: the cells they cover.
    pub arrays: Vec<Area>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shared {
    pub master: (u32, u32),
    pub cells: Vec<(u32, u32)>,
}

impl Scan {
    /// The shared formula cell (`r`, `c`) spells out, if it's one's first.
    pub fn master_of(&self, at: (u32, u32)) -> Option<&Shared> {
        self.shared.values().find(|s| s.master == at)
    }

    /// Whether (`r`, `c`) is under an array formula or a data table.
    pub fn in_array(&self, (r, c): (u32, u32)) -> bool {
        self.arrays.iter().any(|a| a.top <= r && r <= a.bottom && a.left <= c && c <= a.right)
    }
}

/// A cell's place from its name ("B7" → (7, 2)). Pure.
pub fn place_of(a1: &str) -> Option<(u32, u32)> {
    let a1 = a1.trim().trim_start_matches('$');
    let letters = a1.bytes().take_while(u8::is_ascii_alphabetic).count();
    if letters == 0 || letters > 3 {
        return None;
    }
    let col = a1[..letters].bytes().fold(0u32, |n, b| n * 26 + (b.to_ascii_uppercase() - b'A' + 1) as u32);
    let row: u32 = a1[letters..].trim_start_matches('$').parse().ok()?;
    (row >= 1 && (1..=16_384).contains(&col)).then_some((row, col))
}

/// An area from its name ("A1:D5", "B2"). Pure.
pub fn area_of(a1: &str) -> Option<Area> {
    let (from, to) = a1.split_once(':').unwrap_or((a1, a1));
    let (r0, c0) = place_of(from)?;
    let (r1, c1) = place_of(to)?;
    Some(Area { top: r0.min(r1), left: c0.min(c1), bottom: r0.max(r1), right: c0.max(c1) })
}

/// An attribute's value as written, its entities read. Pure.
pub fn unescaped_attr(raw: &str) -> String {
    quick_xml::escape::unescape(raw).map(|c| c.into_owned()).unwrap_or_else(|_| raw.to_string())
}

fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.try_get_attribute(name).ok().flatten().map(|a| unescaped_attr(&a.value))
}

fn local(e: &BytesStart) -> String {
    e.local_name().as_ref().to_string()
}

/// **What a sheet's XML holds** that a change has to mind: its shared
/// formulas and its array formulas. A row or a cell with no `r` (the
/// format lets a writer leave it out) is refused: a cell put in before it
/// would move it. Pure.
pub fn scan(xml: &str) -> Result<Scan, String> {
    let mut out = Scan::default();
    let mut reader = Reader::from_str(xml);
    let mut in_data = false;
    let mut cell: Option<(u32, u32)> = None;
    loop {
        let event = reader.read_event().map_err(|_| "Its XML couldn't be read.".to_string())?;
        match event {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) => {
                let name = local(&e);
                match name.as_str() {
                    "sheetData" => in_data = true,
                    "row" if in_data => {
                        if attr(&e, "r").is_none() {
                            return Err("A row in it has no number.".into());
                        }
                    }
                    "c" if in_data => {
                        let at = attr(&e, "r").as_deref().and_then(place_of).ok_or("A cell in it has no name.")?;
                        cell = Some(at);
                    }
                    "f" if in_data => {
                        let Some(at) = cell else { continue };
                        let t = attr(&e, "t").unwrap_or_default();
                        match t.as_str() {
                            "shared" => {
                                let Some(si) = attr(&e, "si").and_then(|s| s.parse::<u32>().ok()) else { continue };
                                let group = out.shared.entry(si).or_default();
                                group.cells.push(at);
                                if attr(&e, "ref").is_some() {
                                    group.master = at;
                                }
                            }
                            "array" | "dataTable" => {
                                let area = attr(&e, "ref").as_deref().and_then(area_of).unwrap_or(Area { top: at.0, left: at.1, bottom: at.0, right: at.1 });
                                out.arrays.push(area);
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
            }
            Event::End(e) => {
                let name = e.local_name().as_ref().to_string();
                match name.as_str() {
                    "sheetData" => in_data = false,
                    "c" => cell = None,
                    _ => {}
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

// ---- writing cells ------------------------------------------------------------------------------

/// Text for XML (`&`, `<`, `>`, and `"` in an attribute), with the
/// characters XML 1.0 can't hold written as OOXML's `_xHHHH_` (and a
/// literal `_xHHHH_` kept literal: its `_` as `_x005F_`). Pure.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '_' if looks_escaped(&chars[i..]) => out.push_str("_x005F_"),
            '\t' | '\n' | '\r' => out.push(ch),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => out.push_str(&format!("_x{:04X}_", c as u32)),
            c => out.push(c),
        }
        i += 1;
    }
    out
}

/// Whether `s` starts with `_xHHHH_`.
fn looks_escaped(s: &[char]) -> bool {
    s.len() >= 7 && s[1] == 'x' && s[2..6].iter().all(|c| c.is_ascii_hexdigit()) && s[6] == '_'
}

/// A number as the file writes it: the shortest that reads back the same. Pure.
pub fn number(n: f64) -> String {
    if n == 0.0 {
        return "0".into();
    }
    // (Rust never writes an exponent; a long run of zeros is still exact.)
    format!("{n}")
}

/// `t` and the `<v>` for a value (`p`: the elements' prefix).
fn value_parts(v: &Value, p: &str, formula: bool) -> (&'static str, String) {
    match v {
        Value::Number(n) if n.is_finite() => ("", format!("<{p}v>{}</{p}v>", number(*n))),
        Value::Number(_) => ("e", format!("<{p}v>#NUM!</{p}v>")),
        Value::Bool(b) => ("b", format!("<{p}v>{}</{p}v>", u8::from(*b))),
        Value::Error(e) => ("e", format!("<{p}v>{}</{p}v>", escape(e))),
        Value::Text(t) if formula => ("str", format!("<{p}v>{}</{p}v>", escape(t))),
        Value::Text(t) => {
            let space = if t.trim() != t || t.contains('\n') { r#" xml:space="preserve""# } else { "" };
            ("inlineStr", format!("<{p}is><{p}t{space}>{}</{p}t></{p}is>", escape(t)))
        }
    }
}

/// **A cell's XML** for `put` at `name` ("B7"); none for an empty cell
/// with no style (it goes). Pure.
pub fn cell_xml(p: &str, name: &str, put: &CellPut) -> Option<String> {
    let s = if put.style > 0 { format!(r#" s="{}""#, put.style) } else { String::new() };
    let (t, inner) = match &put.put {
        Put::Empty => {
            return (put.style > 0).then(|| format!(r#"<{p}c r="{name}"{s}/>"#));
        }
        Put::Value(v) => value_parts(v, p, false),
        Put::Formula(f, result) => {
            let (t, v) = result.as_ref().map(|r| value_parts(r, p, true)).unwrap_or(("", String::new()));
            (t, format!("<{p}f>{}</{p}f>{v}", escape(f)))
        }
    };
    let t = if t.is_empty() { String::new() } else { format!(r#" t="{t}""#) };
    Some(format!(r#"<{p}c r="{name}"{s}{t}>{inner}</{p}c>"#))
}

/// A start tag's attributes with `drop` left out, as written (each with
/// its space). Pure.
fn attrs_without(e: &BytesStart, drop: &[&str]) -> String {
    let mut out = String::new();
    for a in e.attributes().with_checks(false).flatten() {
        let key = a.key.as_ref();
        if drop.contains(&key) {
            continue;
        }
        out.push(' ');
        out.push_str(key);
        out.push_str("=\"");
        // (The raw value as written: already escaped for a double quote; a
        // single-quoted one has its double quotes escaped.)
        out.push_str(&a.value.replace('"', "&quot;"));
        out.push('"');
    }
    out
}

/// The text written so far into `out`, from `src`, and what's next to copy.
struct Splice<'a> {
    src: &'a str,
    out: String,
    at: usize,
}

impl<'a> Splice<'a> {
    fn new(src: &'a str) -> Self {
        Splice { src, out: String::with_capacity(src.len() + 1024), at: 0 }
    }
    /// Copy the source up to `to`.
    fn copy_to(&mut self, to: usize) {
        if to > self.at {
            self.out.push_str(&self.src[self.at..to]);
            self.at = to;
        }
    }
    /// `text` in place of the source's `from..to`.
    fn replace(&mut self, from: usize, to: usize, text: &str) {
        self.copy_to(from);
        self.out.push_str(text);
        self.at = to;
    }
    /// `text` put in at `at`.
    fn insert(&mut self, at: usize, text: &str) {
        self.copy_to(at);
        self.out.push_str(text);
    }
    fn finish(mut self) -> String {
        let end = self.src.len();
        self.copy_to(end);
        self.out
    }
}

/// The cells of a new row (`r`), left to right.
fn row_xml(p: &str, r: u32, cells: &[(u32, CellPut)]) -> String {
    let inner: String = cells.iter().filter_map(|(c, put)| cell_xml(p, &format!("{}{r}", super::column_name(*c)), put)).collect();
    if inner.is_empty() {
        String::new()
    } else {
        format!(r#"<{p}row r="{r}">{inner}</{p}row>"#)
    }
}

/// A formula cell being brought up to date: its start, its tag's attributes
/// and its children as written.
struct Refresh {
    start: usize,
    attrs: String,
    /// Its `t` as written.
    t: String,
    value: Value,
    f: String,
    rest: String,
    /// Its result as written (`<v>`'s text).
    old: Option<String>,
    depth: u32,
    /// Where the child being skipped (`v`, `is`) or kept started.
    child: Option<(usize, bool)>,
}

/// Whether a result written `old` (as `t`) is `new` already: the cell is left as it is. Pure.
fn same_result(t: &str, old: Option<&str>, new: &Value) -> bool {
    let Some(old) = old else { return false };
    match new {
        Value::Number(n) => (t.is_empty() || t == "n") && old.trim().parse::<f64>().is_ok_and(|o| o == *n),
        Value::Bool(b) => t == "b" && old.trim() == if *b { "1" } else { "0" },
        Value::Error(e) => t == "e" && old.trim() == e,
        Value::Text(text) => t == "str" && unescaped(old) == *text,
    }
}

/// An element's text as written, its entities read.
fn unescaped(s: &str) -> String {
    unescaped_attr(s)
}

/// The text inside a `<v>…</v>` as written.
fn inner_of(el: &str) -> Option<String> {
    let open = el.find('>')?;
    let close = el.rfind('<')?;
    (close > open).then(|| el[open + 1..close].to_string())
}

/// **The sheet's XML with `patch` written in** (and its `<dimension>`
/// grown to take in new cells); every other byte as it was. Pure.
pub fn patch_sheet(xml: &str, patch: &SheetPatch) -> Result<String, String> {
    if patch.is_empty() {
        return Ok(xml.to_string());
    }
    // The puts by row, each row's left to right.
    let mut rows: BTreeMap<u32, Vec<(u32, CellPut)>> = BTreeMap::new();
    for (&(r, c), put) in &patch.puts {
        rows.entry(r).or_default().push((c, put.clone()));
    }
    let mut grown = Area::default();
    for (&(r, c), put) in &patch.puts {
        if put.put != Put::Empty || put.style > 0 {
            grown.take(r, c);
        }
    }
    let mut out = Splice::new(xml);
    let mut reader = Reader::from_str(xml);
    let mut p = String::new();
    let mut in_data = false;
    let mut data_done = false;
    // The row being read: its number and its puts not written yet.
    let mut row: Option<(u32, Vec<(u32, CellPut)>)> = None;
    // A cell being replaced: where it starts, the depth inside it, and its place.
    let mut skip: Option<(usize, u32)> = None;
    let mut cell_at: Option<(u32, u32)> = None;
    let mut refresh: Option<Refresh> = None;
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event().map_err(|_| "Its XML couldn't be read.".to_string())?;
        let end = reader.buffer_position() as usize;
        // Inside a cell being replaced: everything until its end goes.
        if let Some((from, depth)) = skip.as_mut() {
            match &event {
                Event::Start(_) => *depth += 1,
                Event::End(_) if *depth == 0 => {
                    let from = *from;
                    let (r, c) = cell_at.take().unwrap_or_default();
                    let put = patch.puts.get(&(r, c));
                    let text = put.and_then(|put| cell_xml(&p, &format!("{}{r}", super::column_name(c)), put)).unwrap_or_default();
                    out.replace(from, end, &text);
                    skip = None;
                }
                Event::End(_) => *depth -= 1,
                Event::Eof => return Err("Its XML ended inside a cell.".into()),
                _ => {}
            }
            continue;
        }
        // Inside a formula cell being brought up to date: its children kept but its result.
        if let Some(rf) = refresh.as_mut() {
            match &event {
                Event::Start(e) => {
                    if rf.depth == 0 {
                        let name = local(e);
                        rf.child = Some((start, name == "v" || name == "is"));
                        if name == "f" {
                            rf.child = Some((start, false));
                        }
                    }
                    rf.depth += 1;
                }
                Event::Empty(e) if rf.depth == 0 => {
                    let name = local(e);
                    if name == "f" {
                        rf.f.push_str(&xml[start..end]);
                    } else if name != "v" && name != "is" {
                        rf.rest.push_str(&xml[start..end]);
                    }
                }
                Event::End(_) if rf.depth == 0 => {
                    // The cell's own end: rewritten only when its result is new.
                    if !rf.f.is_empty() && !same_result(&rf.t, rf.old.as_deref(), &rf.value) {
                        let (t, v) = value_parts(&rf.value, &p, true);
                        let t = if t.is_empty() { String::new() } else { format!(r#" t="{t}""#) };
                        let text = format!("<{p}c{}{t}>{}{v}{}</{p}c>", rf.attrs, rf.f, rf.rest);
                        out.replace(rf.start, end, &text);
                    }
                    refresh = None;
                }
                Event::End(e) => {
                    rf.depth -= 1;
                    if rf.depth == 0 {
                        if let Some((from, skipped)) = rf.child.take() {
                            let name = local_end(e);
                            if name == "f" {
                                rf.f.push_str(&xml[from..end]);
                            } else if name == "v" {
                                rf.old = inner_of(&xml[from..end]);
                            } else if !skipped {
                                rf.rest.push_str(&xml[from..end]);
                            }
                        }
                    }
                }
                Event::Eof => return Err("Its XML ended inside a cell.".into()),
                _ => {}
            }
            continue;
        }
        match event {
            Event::Eof => break,
            Event::Empty(e) if !in_data && !data_done && local(&e) == "dimension" => {
                if let Some(now) = attr(&e, "ref").as_deref().and_then(area_of) {
                    if !grown.is_empty() {
                        let mut both = now;
                        both.take(grown.top, grown.left);
                        both.take(grown.bottom, grown.right);
                        if both != now {
                            let name = e.name().as_ref().to_string();
                            let others = attrs_without(&e, &["ref"]);
                            out.replace(start, end, &format!(r#"<{name} ref="{}"{others}/>"#, both.a1()));
                        }
                    }
                }
            }
            Event::Empty(e) if !in_data && !data_done && local(&e) == "sheetData" => {
                p = prefix_of(e.name().as_ref());
                let inner: String = rows.iter().map(|(r, cells)| row_xml(&p, *r, cells)).collect();
                let name = e.name().as_ref().to_string();
                let attrs = attrs_without(&e, &[]);
                out.replace(start, end, &format!("<{name}{attrs}>{inner}</{name}>"));
                rows.clear();
                data_done = true;
            }
            Event::Start(e) if !in_data && !data_done && local(&e) == "sheetData" => {
                p = prefix_of(e.name().as_ref());
                in_data = true;
            }
            Event::End(e) if in_data && local_end(&e) == "sheetData" => {
                // The rows still to write come last.
                let inner: String = std::mem::take(&mut rows).iter().map(|(r, cells)| row_xml(&p, *r, cells)).collect();
                out.insert(start, &inner);
                in_data = false;
                data_done = true;
            }
            Event::Start(e) | Event::Empty(e) if in_data && row.is_none() && local(&e) == "row" => {
                let empty = matches!(reader_kind(xml, start, end), Kind::Empty);
                let r: u32 = attr(&e, "r").and_then(|s| s.parse().ok()).ok_or("A row in it has no number.")?;
                // New rows that come before this one.
                let before: Vec<u32> = rows.range(..r).map(|(k, _)| *k).collect();
                if !before.is_empty() {
                    let inner: String = before.iter().map(|k| row_xml(&p, *k, &rows[k])).collect();
                    out.insert(start, &inner);
                    for k in before {
                        rows.remove(&k);
                    }
                }
                let mine = rows.remove(&r);
                match (mine, empty) {
                    (None, true) => {}
                    (None, false) => row = Some((r, Vec::new())),
                    (Some(cells), true) => {
                        // `<row r="5"/>` with cells to put in: a whole row (no `spans`: it's only a hint).
                        let name = e.name().as_ref().to_string();
                        let attrs = attrs_without(&e, &["spans"]);
                        let inner: String = cells.iter().filter_map(|(c, put)| cell_xml(&p, &format!("{}{r}", super::column_name(*c)), put)).collect();
                        out.replace(start, end, &format!("<{name}{attrs}>{inner}</{name}>"));
                    }
                    (Some(cells), false) => {
                        let name = e.name().as_ref().to_string();
                        let attrs = attrs_without(&e, &["spans"]);
                        out.replace(start, end, &format!("<{name}{attrs}>"));
                        row = Some((r, cells));
                    }
                }
            }
            Event::End(e) if in_data && local_end(&e) == "row" => {
                if let Some((r, cells)) = row.take() {
                    let inner: String = cells.iter().filter_map(|(c, put)| cell_xml(&p, &format!("{}{r}", super::column_name(*c)), put)).collect();
                    if !inner.is_empty() {
                        out.insert(start, &inner);
                    }
                }
            }
            Event::Start(e) | Event::Empty(e) if in_data && row.is_some() && local(&e) == "c" => {
                let empty = matches!(reader_kind(xml, start, end), Kind::Empty);
                let (r, c) = attr(&e, "r").as_deref().and_then(place_of).ok_or("A cell in it has no name.")?;
                let Some((row_no, cells)) = row.as_mut() else { continue };
                if r != *row_no {
                    return Err("A cell in it is in the wrong row.".into());
                }
                // Cells to put in before this one.
                let n = cells.iter().take_while(|(k, _)| *k < c).count();
                if n > 0 {
                    let inner: String = cells.drain(..n).filter_map(|(k, put)| cell_xml(&p, &format!("{}{r}", super::column_name(k)), &put)).collect();
                    out.insert(start, &inner);
                }
                if cells.first().is_some_and(|(k, _)| *k == c) {
                    cells.remove(0);
                    if empty {
                        let text = patch.puts.get(&(r, c)).and_then(|put| cell_xml(&p, &format!("{}{r}", super::column_name(c)), put)).unwrap_or_default();
                        out.replace(start, end, &text);
                    } else {
                        cell_at = Some((r, c));
                        skip = Some((start, 0));
                    }
                } else if let Some(value) = patch.results.get(&(r, c)).filter(|_| !empty) {
                    refresh = Some(Refresh { start, attrs: attrs_without(&e, &["t"]), t: attr(&e, "t").unwrap_or_default(), value: value.clone(), f: String::new(), rest: String::new(), old: None, depth: 0, child: None });
                }
            }
            _ => {}
        }
    }
    if !rows.is_empty() && !data_done {
        return Err("It has no cells part.".into());
    }
    Ok(out.finish())
}

enum Kind {
    Empty,
    Start,
}

/// Whether the tag at `start..end` closes itself (`<c r="A1"/>`).
fn reader_kind(xml: &str, start: usize, end: usize) -> Kind {
    let tag = xml[start..end].trim_end();
    if tag.ends_with("/>") {
        Kind::Empty
    } else {
        Kind::Start
    }
}

fn local_end(e: &quick_xml::events::BytesEnd) -> String {
    e.local_name().as_ref().to_string()
}

/// An element name's prefix with its colon ("x:sheetData" → "x:"; none: "").
fn prefix_of(name: &str) -> String {
    name.rsplit_once(':').map(|(p, _)| format!("{p}:")).unwrap_or_default()
}

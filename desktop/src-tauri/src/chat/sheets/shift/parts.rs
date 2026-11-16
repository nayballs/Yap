//! **A workbook's package with rows or columns put in or taken out of one
//! sheet** (16 Nov 2026), every part that points at a cell moved as Excel
//! moves it — what Excel's own Insert and Delete change in the file:
//!
//! - **The sheet's cells** (`<sheetData>`): each row and cell renumbered, the
//!   ones taken out gone; a row put in takes the row above's formats (Excel's
//!   default "Format Same As Above"; a column the one to its left's); every
//!   formula's references (`shift::formula`); an array formula's area; a
//!   shared formula whose cells move, or whose references do, written out in
//!   full in each cell (the model's own text for each: `Change::formula_at`).
//! - **The sheet's other parts**: its size (`<dimension>`), frozen panes and
//!   selection, column widths (`<cols>`), merged cells, conditional formats
//!   (their areas and formulas), data validation, hyperlinks, the autofilter,
//!   page breaks, ignored errors, protected ranges, watched cells, and the
//!   newer conditional formats, rules and sparklines in its `<extLst>`.
//! - **Everywhere else**: every other sheet's formulas, conditional formats
//!   and rules that name the sheet; defined names (print areas and titles
//!   too); charts' series (`<c:f>`); the sheet's pictures and charts'
//!   anchors (they move and grow with their cells, as their `editAs` says);
//!   its comments (and their notes' boxes in the VML); its tables.
//! - **Refused, plainly, never half-done** (`Err`: why, a phrase): a pivot
//!   table on the sheet or fed by it, slicers or timelines, a data table
//!   (What-If), scenarios, form controls and embedded objects, custom views,
//!   a part of the sheet Yap doesn't know, a picture or a comment on lines
//!   that would go, a table or an array formula that would be cut apart,
//!   cells pushed off the end of the sheet, a reference Yap can't be sure of
//!   (`shift::Unsure`).
//!
//! Pure: bytes in, bytes out; every part Yap doesn't change copied as it is.

use std::collections::{HashMap, HashSet};

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use super::{area, cell_name, formula, sqref, Axis, Shift, Unsure};
use crate::chat::sheets::patch::{area_of, place_of, unescaped_attr, Splice};
use crate::chat::sheets::{column_name, save};

/// **The change**: sheet `sheet` (IronCalc's order, from 0), lines
/// `shift`; `formula_at(sheet, row, column)`: a cell's formula in Excel's
/// form before the change (none: it has none), for a shared formula's cells.
pub struct Change<'a> {
    pub sheet: u32,
    pub shift: Shift,
    pub formula_at: &'a dyn Fn(u32, u32, u32) -> Option<String>,
}

type Why = String;

impl From<Unsure> for Why {
    fn from(u: Unsure) -> Why {
        u.0.to_string()
    }
}

// ---- reading and writing XML ------------------------------------------------------------------

fn local(e: &BytesStart) -> String {
    e.local_name().as_ref().to_string()
}

fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.try_get_attribute(name).ok().flatten().map(|a| unescaped_attr(&a.value))
}

/// Text for an element's content.
fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Text for an attribute's value.
fn escape_attr(s: &str) -> String {
    escape_text(s).replace('"', "&quot;")
}

/// **A start tag rebuilt**: its attributes as written, those in `set`
/// changed (none: left out), the new ones last. Pure.
fn tag_with(e: &BytesStart, empty: bool, set: &[(&str, Option<String>)]) -> String {
    let mut s = format!("<{}", e.name().as_ref());
    let mut done: HashSet<String> = HashSet::new();
    for a in e.attributes().with_checks(false).flatten() {
        let key = a.key.as_ref();
        if let Some((k, v)) = set.iter().find(|(k, _)| *k == key) {
            done.insert(k.to_string());
            if let Some(v) = v {
                s.push_str(&format!(" {key}=\"{}\"", escape_attr(v)));
            }
            continue;
        }
        s.push_str(&format!(" {key}=\"{}\"", a.value.replace('"', "&quot;")));
    }
    for (k, v) in set {
        if let (false, Some(v)) = (done.contains(*k), v) {
            s.push_str(&format!(" {k}=\"{}\"", escape_attr(v)));
        }
    }
    s.push_str(if empty { "/>" } else { ">" });
    s
}

fn is_empty_tag(xml: &str, start: usize, end: usize) -> bool {
    xml[start..end].trim_end().ends_with("/>")
}

fn prefix_of(e: &BytesStart) -> String {
    e.name().as_ref().rsplit_once(':').map(|(p, _)| format!("{p}:")).unwrap_or_default()
}

const BAD_XML: &str = "a part Yap couldn't read";

/// What [`walk`] asks about.
enum Step<'a, 'b> {
    /// An element's start: the local names from the root to it, its tag.
    Open(&'b [String], &'b BytesStart<'a>),
    /// A leaf's text (asked for with [`Edit::Leaf`]), its entities read.
    Text(&'b [String], &'b BytesStart<'a>, &'b str),
}

/// What [`walk`] does with it.
enum Edit {
    Keep,
    /// Its tag with these attributes set (none: left out).
    Attrs(Vec<(&'static str, Option<String>)>),
    /// The whole element gone.
    Drop,
    /// The whole element in place of this text.
    Whole(String),
    /// A leaf (no elements inside): its text asked about next.
    Leaf,
    /// The leaf's text, when it changed.
    Text(Option<String>),
}

/// **`xml` with each element as `on` says**, every other byte as written.
fn walk(xml: &str, mut on: impl FnMut(Step) -> Result<Edit, Why>) -> Result<String, Why> {
    let mut reader = Reader::from_str(xml);
    let mut out = Splice::new(xml);
    let mut path: Vec<String> = Vec::new();
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event().map_err(|_| BAD_XML.to_string())?;
        let end = reader.buffer_position() as usize;
        let e = match event {
            Event::Eof => break,
            Event::End(_) => {
                path.pop();
                continue;
            }
            Event::Start(e) | Event::Empty(e) => e,
            _ => continue,
        };
        let empty = is_empty_tag(xml, start, end);
        path.push(local(&e));
        // Where the element ends (read to it when it isn't empty).
        let to_end = |reader: &mut Reader<&[u8]>| -> Result<(usize, usize, usize), Why> {
            if empty {
                return Ok((end, end, end));
            }
            let span = reader.read_to_end(e.name()).map_err(|_| BAD_XML.to_string())?;
            Ok((span.start as usize, span.end as usize, reader.buffer_position() as usize))
        };
        match on(Step::Open(&path, &e))? {
            Edit::Keep | Edit::Text(_) => {
                if empty {
                    path.pop();
                }
            }
            Edit::Attrs(set) => {
                out.replace(start, end, &tag_with(&e, empty, &set));
                if empty {
                    path.pop();
                }
            }
            Edit::Drop => {
                let (_, _, stop) = to_end(&mut reader)?;
                out.replace(start, stop, "");
                path.pop();
            }
            Edit::Whole(text) => {
                let (_, _, stop) = to_end(&mut reader)?;
                out.replace(start, stop, &text);
                path.pop();
            }
            Edit::Leaf => {
                let (a, b, stop) = to_end(&mut reader)?;
                let raw = &xml[a..b];
                if raw.contains('<') && !raw.trim_start().starts_with("<![CDATA[") {
                    return Err(BAD_XML.into());
                }
                let text = unescaped_attr(raw);
                match on(Step::Text(&path, &e, &text))? {
                    Edit::Text(Some(t)) => out.replace(a, b, &escape_text(&t)),
                    Edit::Whole(w) => out.replace(start, stop, &w),
                    _ => {}
                }
                path.pop();
            }
        }
    }
    Ok(out.finish())
}

/// The direct children called `child` of each `container` counted, its
/// `count` set (and `manualBreakCount`, a page break's); one with none
/// left gone (the format wants one at least). Pure.
fn recount(xml: &str, container: &str, child: &str) -> Result<String, Why> {
    // Count first: each container's children, in order.
    let mut counts: Vec<(usize, usize)> = Vec::new();
    {
        let mut reader = Reader::from_str(xml);
        let mut depth_in: Option<usize> = None;
        let mut depth = 0usize;
        loop {
            match reader.read_event().map_err(|_| BAD_XML.to_string())? {
                Event::Eof => break,
                Event::Start(e) => {
                    depth += 1;
                    let name = local(&e);
                    if name == container && depth_in.is_none() {
                        depth_in = Some(depth);
                        counts.push((0, 0));
                    } else if depth_in.is_some_and(|d| depth == d + 1) && name == child {
                        if let Some(c) = counts.last_mut() {
                            c.0 += 1;
                            c.1 += usize::from(attr(&e, "man").is_some_and(|m| m == "1" || m == "true"));
                        }
                    }
                }
                Event::Empty(e) => {
                    let name = local(&e);
                    if name == container && depth_in.is_none() {
                        counts.push((0, 0));
                    } else if depth_in == Some(depth) && name == child {
                        if let Some(c) = counts.last_mut() {
                            c.0 += 1;
                            c.1 += usize::from(attr(&e, "man").is_some_and(|m| m == "1" || m == "true"));
                        }
                    }
                }
                Event::End(e) => {
                    if depth_in == Some(depth) && local_end(&e) == container {
                        depth_in = None;
                    }
                    depth -= 1;
                }
                _ => {}
            }
        }
    }
    let mut i = 0;
    walk(xml, |step| {
        let Step::Open(path, e) = step else { return Ok(Edit::Keep) };
        if path.last().map(String::as_str) != Some(container) {
            return Ok(Edit::Keep);
        }
        let (n, man) = counts.get(i).copied().unwrap_or((0, 0));
        i += 1;
        if n == 0 {
            return Ok(Edit::Drop);
        }
        let mut set = Vec::new();
        if attr(e, "count").is_some_and(|c| c != n.to_string()) {
            set.push(("count", Some(n.to_string())));
        }
        if attr(e, "manualBreakCount").is_some_and(|c| c != man.to_string()) {
            set.push(("manualBreakCount", Some(man.to_string())));
        }
        Ok(if set.is_empty() { Edit::Keep } else { Edit::Attrs(set) })
    })
}

fn local_end(e: &quick_xml::events::BytesEnd) -> String {
    e.local_name().as_ref().to_string()
}

// ---- the package ------------------------------------------------------------------------------

/// A part's path from a relationship's target, beside `from` (a part's path). Pure.
fn resolve(from: &str, target: &str) -> String {
    if let Some(abs) = target.strip_prefix('/') {
        return abs.to_string();
    }
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop();
    for seg in target.split('/') {
        match seg {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// A part's relationships: id → (type's last word, the part it points at).
fn rels_of(zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, part: &str) -> HashMap<String, (String, String)> {
    let (dir, file) = part.rsplit_once('/').unwrap_or(("", part));
    let rels_path = format!("{dir}/_rels/{file}.rels");
    let Some(xml) = save::read_part(zip, &rels_path) else { return HashMap::new() };
    let mut out = HashMap::new();
    let mut reader = Reader::from_str(&xml);
    while let Ok(event) = reader.read_event() {
        match event {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) if local(&e) == "Relationship" => {
                if attr(&e, "TargetMode").is_some_and(|m| m.eq_ignore_ascii_case("External")) {
                    continue;
                }
                if let (Some(id), Some(t), Some(target)) = (attr(&e, "Id"), attr(&e, "Type"), attr(&e, "Target")) {
                    let kind = t.rsplit('/').next().unwrap_or_default().to_string();
                    out.insert(id, (kind, resolve(part, &target)));
                }
            }
            _ => {}
        }
    }
    out
}

/// **The package `base` with the change made**, or why not.
pub fn package(base: &[u8], change: &Change) -> Result<Vec<u8>, Why> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(base)).map_err(|_| "it isn't a workbook".to_string())?;
    let names: Vec<String> = zip.file_names().filter_map(|n| n.ok().map(|n| n.into_owned())).collect();
    if names.iter().any(|n| n.starts_with("xl/slicerCaches/") || n.starts_with("xl/timelineCaches/")) {
        return Err("it has slicers or timelines, which Yap can't move".into());
    }
    let workbook = save::read_part(&mut zip, "xl/workbook.xml").ok_or("it has no workbook part")?;
    let wb_rels = save::read_part(&mut zip, "xl/_rels/workbook.xml.rels").ok_or("it has no workbook part")?;
    let sheets = save::sheets_of(&workbook, &wb_rels);
    let (target, _) = sheets.get(change.sheet as usize).cloned().ok_or("no such sheet")?;
    let s = change.shift;
    // A pivot table fed by the sheet.
    for name in names.iter().filter(|n| n.starts_with("xl/pivotCache/pivotCacheDefinition") && n.ends_with(".xml")) {
        let xml = save::read_part(&mut zip, name).unwrap_or_default();
        let mut reader = Reader::from_str(&xml);
        while let Ok(event) = reader.read_event() {
            match event {
                Event::Eof => break,
                Event::Start(e) | Event::Empty(e) if local(&e) == "worksheetSource" && attr(&e, "sheet").is_some_and(|n| n.eq_ignore_ascii_case(&target)) => {
                    return Err("a pivot table's data is on this sheet, and Yap can't move it".into());
                }
                _ => {}
            }
        }
    }
    let mut parts: HashMap<String, String> = HashMap::new();
    // Each sheet: its own parts (drawings, comments, tables…).
    for (si, (name, part)) in sheets.iter().enumerate() {
        let mine = si as u32 == change.sheet;
        let rels = rels_of(&mut zip, part);
        if mine {
            for (kind, _) in rels.values() {
                match kind.as_str() {
                    "pivotTable" => return Err("it has a pivot table, which Yap can't move".into()),
                    "tableSingleCells" | "xmlMaps" => return Err("it has cells mapped to XML, which Yap can't move".into()),
                    "slicer" | "timeline" => return Err("it has slicers or timelines, which Yap can't move".into()),
                    _ => {}
                }
            }
        }
        let xml = save::read_part(&mut zip, part).ok_or("a sheet's part is missing")?;
        let formula_at = |r: u32, c: u32| (change.formula_at)(si as u32, r, c);
        let now = if mine { target_sheet(&xml, &target, &s, &formula_at)? } else { other_sheet(&xml, name, &target, &s, &formula_at)? };
        put(&mut parts, part, &xml, now);
        for (kind, at) in rels.values() {
            let Some(text) = save::read_part(&mut zip, at) else { continue };
            let now = match (kind.as_str(), mine) {
                ("drawing", _) => drawing(&text, name, &target, &s, mine)?,
                ("table", _) => table(&text, name, &target, &s, mine)?,
                ("comments", true) => comments(&text, &s)?,
                ("threadedComment", true) => comments(&text, &s)?,
                ("vmlDrawing", true) => vml(&text, &s)?,
                _ => continue,
            };
            put(&mut parts, at, &text, now);
        }
    }
    // Defined names: print areas and titles, a filter's range, the person's own.
    let now = walk(&workbook, |step| match step {
        Step::Open(path, _) if path.last().is_some_and(|n| n == "definedName") => Ok(Edit::Leaf),
        Step::Text(_, _, text) => {
            let m = formula(text, None, &target, &s)?;
            Ok(Edit::Text((m.text != text).then_some(m.text)))
        }
        _ => Ok(Edit::Keep),
    })?;
    put(&mut parts, "xl/workbook.xml", &workbook, now);
    // Charts' series, titles and labels (`<c:f>`, a chartEx's `<cx:f>`).
    for name in names.iter().filter(|n| n.starts_with("xl/charts/") && n.ends_with(".xml") && !n.contains("/_rels/")) {
        let Some(xml) = save::read_part(&mut zip, name) else { continue };
        let now = walk(&xml, |step| match step {
            Step::Open(path, _) if path.last().is_some_and(|n| n == "f") => Ok(Edit::Leaf),
            Step::Text(_, _, text) => {
                let m = formula(text, None, &target, &s)?;
                Ok(Edit::Text((m.text != text).then_some(m.text)))
            }
            _ => Ok(Edit::Keep),
        })?;
        put(&mut parts, name, &xml, now);
    }
    drop(zip);
    save::repack(base, &parts, &HashSet::new())
}

/// A part changed, kept to be written.
fn put(parts: &mut HashMap<String, String>, name: &str, was: &str, now: String) {
    if now != was {
        parts.insert(name.to_string(), now);
    }
}

/// A formula leaf's text moved: the edit for [`walk`].
fn moved_text(text: &str, host: &str, target: &str, s: &Shift) -> Result<Edit, Why> {
    let m = formula(text, Some(host), target, s)?;
    Ok(Edit::Text((m.text != text).then_some(m.text)))
}

// ---- the sheet changed ----------------------------------------------------------------------

/// The parts of a worksheet, as the format lists them, that Yap moves or keeps.
const KEEP: [&str; 15] = ["sheetPr", "sheetFormatPr", "sheetCalcPr", "sheetProtection", "phoneticPr", "printOptions", "pageMargins", "pageSetup", "headerFooter", "customProperties", "drawing", "legacyDrawing", "legacyDrawingHF", "drawingHF", "picture"];
const MOVE: [&str; 17] = ["dimension", "sheetViews", "cols", "sheetData", "autoFilter", "sortState", "mergeCells", "conditionalFormatting", "dataValidations", "hyperlinks", "rowBreaks", "colBreaks", "ignoredErrors", "protectedRanges", "cellWatches", "extLst", "tableParts"];

/// What a part of the sheet Yap won't move is, said.
fn refused_part(name: &str) -> Option<&'static str> {
    Some(match name {
        "scenarios" => "it has scenarios, which Yap can't move",
        "customSheetViews" => "it has custom views, which Yap can't move",
        "dataConsolidate" => "it has a consolidation, which Yap can't move",
        "oleObjects" | "controls" | "AlternateContent" => "it has form controls or embedded objects, which Yap can't move",
        "webPublishItems" => "it has web publishing settings, which Yap can't move",
        "smartTags" => "it has smart tags, which Yap can't move",
        _ => return None,
    })
}

/// **The sheet changed, its XML**.
fn target_sheet(xml: &str, name: &str, s: &Shift, formula_at: &dyn Fn(u32, u32) -> Option<String>) -> Result<String, Why> {
    // A shared formula stays shared only when none of its cells moves and none of its references does.
    let scan = crate::chat::sheets::patch::scan(xml).map_err(|_| BAD_XML.to_string())?;
    let mut unshare = HashSet::new();
    for (si, g) in &scan.shared {
        for &at in &g.cells {
            let f = formula_at(at.0, at.1).ok_or("it has a formula Yap couldn't read")?;
            if s.moves(at) || formula(&f, Some(name), name, s)?.text != f {
                unshare.insert(*si);
                break;
            }
        }
    }
    let mut reader = Reader::from_str(xml);
    let mut out = Splice::new(xml);
    let mut depth = 0usize;
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event().map_err(|_| BAD_XML.to_string())?;
        let end = reader.buffer_position() as usize;
        let (e, empty) = match event {
            Event::Eof => break,
            Event::Start(e) => (e, false),
            Event::Empty(e) => (e, true),
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                continue;
            }
            _ => continue,
        };
        if depth == 0 {
            // The root, `<worksheet>`.
            if !empty {
                depth = 1;
            }
            continue;
        }
        let part = local(&e);
        if let Some(why) = refused_part(&part) {
            return Err(why.into());
        }
        let stop = if empty { end } else { reader.read_to_end(e.name()).map_err(|_| BAD_XML.to_string())?; reader.buffer_position() as usize };
        let whole = &xml[start..stop];
        if KEEP.contains(&part.as_str()) {
            continue;
        }
        if !MOVE.contains(&part.as_str()) {
            return Err("it has something Yap doesn't know how to move".into());
        }
        let now = match part.as_str() {
            "sheetData" => sheet_data(whole, name, s, formula_at, &unshare)?,
            "tableParts" => continue,
            _ => sheet_part(&part, whole, name, s)?,
        };
        if now != whole {
            out.replace(start, stop, &now);
        }
    }
    Ok(out.finish())
}

/// A frozen pane's lines (`ySplit`, `xSplit`) after the change; none: all of them would go.
fn frozen(k: u32, s: &Shift) -> Option<u32> {
    if k == 0 {
        return Some(0);
    }
    if s.insert {
        Some(if s.at <= k { k + s.n } else { k })
    } else {
        let gone = (s.at..s.at + s.n).filter(|x| *x <= k).count() as u32;
        (gone < k).then(|| k - gone)
    }
}

/// A cell's name after the change, one taken out put where the lines went.
fn kept_cell(a1: &str, s: &Shift) -> Option<String> {
    let at = place_of(a1)?;
    let (r, c) = s.cell(at).unwrap_or(match s.axis {
        Axis::Rows => (s.at.min(super::LAST_ROW), at.1),
        Axis::Cols => (at.0, s.at.min(super::LAST_COL)),
    });
    Some(format!("{}{r}", column_name(c)))
}

/// **One of the sheet's parts after `<sheetData>`** (or before it), moved.
fn sheet_part(part: &str, xml: &str, host: &str, s: &Shift) -> Result<String, Why> {
    let rows = s.axis == Axis::Rows;
    let now = walk(xml, |step| {
        let (path, e) = match step {
            Step::Open(path, e) => (path, e),
            Step::Text(path, _, text) => {
                let leaf = path.last().map(String::as_str).unwrap_or_default();
                return match leaf {
                    "formula" | "formula1" | "formula2" | "f" => moved_text(text, host, host, s),
                    "sqref" => {
                        let now = sqref(text, s)?;
                        if now.is_empty() {
                            return Err("a rule or sparkline would be left with no cells".into());
                        }
                        Ok(Edit::Text((now != text).then_some(now)))
                    }
                    _ => Ok(Edit::Keep),
                };
            }
        };
        let name = path.last().map(String::as_str).unwrap_or_default();
        let parent = path.len().checked_sub(2).and_then(|i| path.get(i)).map(String::as_str).unwrap_or_default();
        Ok(match name {
            "dimension" => match attr(e, "ref") {
                Some(r) => {
                    let now = area(&r, s)?.unwrap_or_else(|| "A1".into());
                    if now == r { Edit::Keep } else { Edit::Attrs(vec![("ref", Some(now))]) }
                }
                None => Edit::Keep,
            },
            "pane" => {
                let mut set = Vec::new();
                if let Some(tl) = attr(e, "topLeftCell") {
                    if let Some(now) = kept_cell(&tl, s).filter(|n| *n != tl) {
                        set.push(("topLeftCell", Some(now)));
                    }
                }
                let state = attr(e, "state").unwrap_or_default();
                if state == "frozen" || state == "frozenSplit" {
                    let key = if rows { "ySplit" } else { "xSplit" };
                    if let Some(k) = attr(e, key).and_then(|v| v.parse::<f64>().ok()) {
                        let now = frozen(k as u32, s).ok_or("every frozen row or column would go")?;
                        if now != k as u32 {
                            set.push((key, Some(now.to_string())));
                        }
                    }
                }
                if set.is_empty() { Edit::Keep } else { Edit::Attrs(set) }
            }
            "selection" => {
                let mut set = Vec::new();
                let active = attr(e, "activeCell").and_then(|a| kept_cell(&a, s).map(|n| (a, n)));
                if let Some((a, n)) = &active {
                    if a != n {
                        set.push(("activeCell", Some(n.clone())));
                    }
                }
                if let Some(q) = attr(e, "sqref") {
                    let now = sqref(&q, s)?;
                    let now = if now.is_empty() { active.map(|(_, n)| n).unwrap_or_else(|| "A1".into()) } else { now };
                    if now != q {
                        set.push(("sqref", Some(now)));
                    }
                }
                if set.is_empty() { Edit::Keep } else { Edit::Attrs(set) }
            }
            "col" if parent == "cols" && !rows => {
                let (Some(min), Some(max)) = (attr(e, "min").and_then(|v| v.parse::<u32>().ok()), attr(e, "max").and_then(|v| v.parse::<u32>().ok())) else { return Ok(Edit::Keep) };
                match s.span(min, max) {
                    None => Edit::Drop,
                    Some((a, b)) if (a, b) == (min, max) => Edit::Keep,
                    Some((a, b)) => Edit::Attrs(vec![("min", Some(a.to_string())), ("max", Some(b.to_string()))]),
                }
            }
            "mergeCell" => {
                let Some(r) = attr(e, "ref") else { return Ok(Edit::Keep) };
                match area(&r, s)? {
                    // Taken out, or one cell left: no merge.
                    None => Edit::Drop,
                    Some(now) if !now.contains(':') => Edit::Drop,
                    Some(now) if now == r => Edit::Keep,
                    Some(now) => Edit::Attrs(vec![("ref", Some(now))]),
                }
            }
            "conditionalFormatting" | "dataValidation" | "ignoredError" | "protectedRange" if parent != "ext" && !path.iter().any(|p| p == "extLst") => {
                let Some(q) = attr(e, "sqref") else { return Ok(Edit::Keep) };
                let now = sqref(&q, s)?;
                if now.is_empty() {
                    Edit::Drop
                } else if now == q {
                    Edit::Keep
                } else {
                    Edit::Attrs(vec![("sqref", Some(now))])
                }
            }
            "cfvo" => match attr(e, "val") {
                Some(v) => {
                    let m = formula(&v, Some(host), host, s)?;
                    if m.text == v { Edit::Keep } else { Edit::Attrs(vec![("val", Some(m.text))]) }
                }
                None => Edit::Keep,
            },
            "formula" | "formula1" | "formula2" => Edit::Leaf,
            "f" | "sqref" if path.iter().any(|p| p == "extLst") => Edit::Leaf,
            "ext" if parent == "extLst" => {
                let uri = attr(e, "uri").unwrap_or_default().to_ascii_uppercase();
                // The newer conditional formats, data validation and sparklines: their areas and formulas moved.
                const MOVED: [&str; 3] = ["{78C0D931-6437-407D-A8EE-F0AAD7539E65}", "{CCE6A557-97BC-4B89-ADB6-D9C93CAAB3DF}", "{05C60535-1F16-4FD2-B633-F4F36F0B64E0}"];
                if MOVED.contains(&uri.as_str()) {
                    Edit::Keep
                } else {
                    // Anything else: kept when it names no cells.
                    let whole = ext_text(xml, &uri);
                    if whole.contains("sqref") || whole.contains(":f>") || whole.contains("ref=\"") || whole.contains("<f>") {
                        return Err("it has something Yap doesn't know how to move".into());
                    }
                    Edit::Keep
                }
            }
            "hyperlink" => {
                let mut set = Vec::new();
                if let Some(r) = attr(e, "ref") {
                    match area(&r, s)? {
                        None => return Ok(Edit::Drop),
                        Some(now) if now != r => set.push(("ref", Some(now))),
                        _ => {}
                    }
                }
                // A link to a place in the workbook (`Budget!A10`): moved too, when it reads as one.
                if let Some(loc) = attr(e, "location") {
                    if let Ok(m) = formula(&loc, Some(host), host, s) {
                        if m.text != loc {
                            set.push(("location", Some(m.text)));
                        }
                    }
                }
                if set.is_empty() { Edit::Keep } else { Edit::Attrs(set) }
            }
            "brk" if (parent == "rowBreaks" && rows) || (parent == "colBreaks" && !rows) => {
                // A break above the line after `id`.
                let Some(id) = attr(e, "id").and_then(|v| v.parse::<u32>().ok()) else { return Ok(Edit::Keep) };
                match s.line(id + 1) {
                    None => Edit::Drop,
                    Some(l) if l - 1 == id => Edit::Keep,
                    Some(l) => Edit::Attrs(vec![("id", Some((l - 1).to_string()))]),
                }
            }
            "cellWatch" => match attr(e, "r") {
                Some(r) => match cell_name(&r, s) {
                    Some(None) => Edit::Drop,
                    Some(Some(now)) if now != r => Edit::Attrs(vec![("r", Some(now))]),
                    _ => Edit::Keep,
                },
                None => Edit::Keep,
            },
            "autoFilter" if path.len() == 1 => {
                let Some(r) = attr(e, "ref") else { return Ok(Edit::Keep) };
                let ar = area_of(&r).ok_or(Unsure("an area Yap couldn't read"))?;
                if !rows && s.splits(ar.left, ar.right) && xml.contains("filterColumn") {
                    return Err("the filter's columns would be cut apart".into());
                }
                match s.area(ar) {
                    None => Edit::Drop,
                    Some(now) if now == ar => Edit::Keep,
                    Some(now) => Edit::Attrs(vec![("ref", Some(now.a1()))]),
                }
            }
            "sortState" | "sortCondition" => match attr(e, "ref") {
                Some(r) => match area(&r, s)? {
                    None => return Err("a sort's cells would go".into()),
                    Some(now) if now == r => Edit::Keep,
                    Some(now) => Edit::Attrs(vec![("ref", Some(now))]),
                },
                None => Edit::Keep,
            },
            _ => Edit::Keep,
        })
    })?;
    // Lists left with fewer (or none: gone) said so.
    Ok(match part {
        "mergeCells" => recount(&now, "mergeCells", "mergeCell")?,
        "dataValidations" => recount(&now, "dataValidations", "dataValidation")?,
        "hyperlinks" => recount(&now, "hyperlinks", "hyperlink")?,
        "rowBreaks" => recount(&now, "rowBreaks", "brk")?,
        "colBreaks" => recount(&now, "colBreaks", "brk")?,
        "ignoredErrors" => recount(&now, "ignoredErrors", "ignoredError")?,
        "protectedRanges" => recount(&now, "protectedRanges", "protectedRange")?,
        "cellWatches" => recount(&now, "cellWatches", "cellWatch")?,
        "cols" => recount(&now, "cols", "col")?,
        _ => now,
    })
}

/// The text of the `<ext>` with `uri` in `xml` (upper case), for a look at what it holds.
fn ext_text(xml: &str, uri: &str) -> String {
    let upper = xml.to_ascii_uppercase();
    let Some(at) = upper.find(&format!("URI=\"{uri}\"")) else { return xml.to_string() };
    let from = upper[..at].rfind('<').unwrap_or(0);
    let to = upper[at..].find("EXT>").map(|i| at + i + 4).unwrap_or(xml.len());
    xml[from..to].to_string()
}

/// A row's look: its own style (when it has one, `customFormat`) and its cells' styles (column, `s`).
type RowLook = (Option<String>, Vec<(u32, String)>);

/// **The sheet's cells** (`<sheetData>…</sheetData>`), moved.
fn sheet_data(xml: &str, host: &str, s: &Shift, formula_at: &dyn Fn(u32, u32) -> Option<String>, unshare: &HashSet<u32>) -> Result<String, Why> {
    let rows = s.axis == Axis::Rows;
    let off_end = || -> Why { "cells would be pushed off the end of the sheet".into() };
    let mut reader = Reader::from_str(xml);
    let mut out = Splice::new(xml);
    // The row being read: its number before and after, its prefix; the cell being read, before.
    let mut row: Option<(u32, u32, String)> = None;
    let mut cell: Option<(u32, u32)> = None;
    // The row above the rows put in: its look (`s`, `customFormat`) and its cells' styles.
    let mut above: Option<RowLook> = None;
    // A cell whose style the columns put in after it take, waiting for its end.
    let mut left: Option<String> = None;
    let copies_of_row = |r: &RowLook, p: &str| -> String {
        let (look, cells) = r;
        if look.is_none() && cells.is_empty() {
            return String::new();
        }
        let mut text = String::new();
        for k in s.at..s.at + s.n {
            let look = look.as_ref().map(|st| format!(r#" s="{st}" customFormat="1""#)).unwrap_or_default();
            let inner: String = cells.iter().map(|(c, st)| format!(r#"<{p}c r="{}{k}" s="{st}"/>"#, column_name(*c))).collect();
            text.push_str(&format!(r#"<{p}row r="{k}"{look}>{inner}</{p}row>"#));
        }
        text
    };
    let copies_of_cell = |st: &str, r: u32, p: &str| -> String { (s.at..s.at + s.n).map(|k| format!(r#"<{p}c r="{}{r}" s="{st}"/>"#, column_name(k))).collect() };
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event().map_err(|_| BAD_XML.to_string())?;
        let end = reader.buffer_position() as usize;
        match event {
            Event::Eof => break,
            Event::End(e) => {
                match local_end(&e).as_str() {
                    "row" => {
                        if let (Some(look), Some((_, _, p))) = (above.take(), row.as_ref()) {
                            out.insert(end, &copies_of_row(&look, p));
                        }
                        row = None;
                    }
                    "c" => {
                        if let (Some(st), Some((_, r, p))) = (left.take(), row.as_ref()) {
                            out.insert(end, &copies_of_cell(&st, *r, p));
                        }
                        cell = None;
                    }
                    _ => {}
                }
                continue;
            }
            Event::Start(e) | Event::Empty(e) => {
                let empty = is_empty_tag(xml, start, end);
                let name = local(&e);
                match name.as_str() {
                    "row" => {
                        let r: u32 = attr(&e, "r").and_then(|v| v.parse().ok()).ok_or("a row in it has no number")?;
                        let p = prefix_of(&e);
                        let new = if rows { s.line(r) } else { Some(r) };
                        let Some(new) = new else {
                            if s.insert {
                                return Err(off_end());
                            }
                            // Taken out, cells and all.
                            let stop = if empty { end } else { reader.read_to_end(e.name()).map_err(|_| BAD_XML.to_string())?; reader.buffer_position() as usize };
                            out.replace(start, stop, "");
                            continue;
                        };
                        let mut set = Vec::new();
                        if new != r {
                            set.push(("r", Some(new.to_string())));
                        }
                        if !rows {
                            if let Some(spans) = attr(&e, "spans") {
                                let moved = spans.split_once(':').and_then(|(a, b)| Some((a.parse::<u32>().ok()?, b.parse::<u32>().ok()?))).and_then(|(a, b)| s.span(a, b));
                                match moved {
                                    Some((a, b)) if format!("{a}:{b}") != spans => set.push(("spans", Some(format!("{a}:{b}")))),
                                    Some(_) => {}
                                    None => set.push(("spans", None)),
                                }
                            }
                        }
                        if !set.is_empty() {
                            out.replace(start, end, &tag_with(&e, empty, &set));
                        }
                        // The row above the ones put in: its look copied into them (Excel's "Format Same As Above").
                        let copy = rows && s.insert && s.at >= 2 && r == s.at - 1;
                        let look = (copy && attr(&e, "customFormat").is_some_and(|v| v == "1" || v == "true")).then(|| attr(&e, "s")).flatten().filter(|st| st != "0");
                        if copy {
                            above = Some((look, Vec::new()));
                        }
                        if empty {
                            if let Some(look) = above.take() {
                                out.insert(end, &copies_of_row(&look, &p));
                            }
                        } else {
                            row = Some((r, new, p));
                        }
                    }
                    "c" => {
                        let Some((_, new_row, p)) = row.clone() else { continue };
                        let (r, c) = attr(&e, "r").as_deref().and_then(place_of).ok_or("a cell in it has no name")?;
                        let new_c = if rows { Some(c) } else { s.line(c) };
                        let Some(new_c) = new_c else {
                            if s.insert {
                                return Err(off_end());
                            }
                            let stop = if empty { end } else { reader.read_to_end(e.name()).map_err(|_| BAD_XML.to_string())?; reader.buffer_position() as usize };
                            out.replace(start, stop, "");
                            continue;
                        };
                        if (new_row, new_c) != (r, c) {
                            out.replace(start, end, &tag_with(&e, empty, &[("r", Some(format!("{}{new_row}", column_name(new_c))))]));
                        }
                        let st = attr(&e, "s").filter(|v| v != "0");
                        if let (Some(look), Some(st)) = (above.as_mut(), st.as_ref()) {
                            look.1.push((c, st.clone()));
                        }
                        let copy = !rows && s.insert && s.at >= 2 && c == s.at - 1;
                        if empty {
                            if let (true, Some(st)) = (copy, st) {
                                out.insert(end, &copies_of_cell(&st, new_row, &p));
                            }
                        } else {
                            cell = Some((r, c));
                            left = if copy { st } else { None };
                        }
                    }
                    "f" => {
                        let Some(at) = cell else { continue };
                        let t = attr(&e, "t").unwrap_or_default();
                        let si = attr(&e, "si").and_then(|v| v.parse::<u32>().ok());
                        if t == "dataTable" {
                            return Err("it has a data table (What-If analysis), which Yap can't move".into());
                        }
                        let fname = e.name().as_ref().to_string();
                        let plain = |text: &str| -> String {
                            let attrs = tag_with(&e, false, &[("t", None), ("ref", None), ("si", None)]);
                            format!("{attrs}{}</{fname}>", escape_text(text))
                        };
                        if empty {
                            // A shared formula's other cell: its own formula written out, when the group comes apart.
                            if t == "shared" && si.is_some_and(|i| unshare.contains(&i)) {
                                let f = formula_at(at.0, at.1).ok_or("it has a formula Yap couldn't read")?;
                                let m = formula(&f, Some(host), host, s)?;
                                out.replace(start, end, &plain(&m.text));
                            }
                            continue;
                        }
                        let span = reader.read_to_end(e.name()).map_err(|_| BAD_XML.to_string())?;
                        let stop = reader.buffer_position() as usize;
                        let raw = &xml[span.start as usize..span.end as usize];
                        let text = unescaped_attr(raw);
                        let m = formula(&text, Some(host), host, s)?;
                        if t == "shared" && si.is_some_and(|i| unshare.contains(&i)) {
                            // Its first cell (or one with its own text): written out as its own.
                            let text = if attr(&e, "ref").is_some() || !text.is_empty() { m.text } else { formula(&formula_at(at.0, at.1).ok_or("it has a formula Yap couldn't read")?, Some(host), host, s)?.text };
                            out.replace(start, stop, &plain(&text));
                            continue;
                        }
                        let mut set: Vec<(&str, Option<String>)> = Vec::new();
                        if t == "array" {
                            if let Some(r) = attr(&e, "ref") {
                                let ar = area_of(&r).ok_or(Unsure("an area Yap couldn't read"))?;
                                let (a, b) = if rows { (ar.top, ar.bottom) } else { (ar.left, ar.right) };
                                if s.splits(a, b) {
                                    return Err("an array formula would be cut apart".into());
                                }
                                if let Some(now) = s.area(ar).map(|a| a.a1()).filter(|n| *n != r) {
                                    set.push(("ref", Some(now)));
                                }
                            }
                        }
                        if !set.is_empty() {
                            let tag = tag_with(&e, false, &set);
                            out.replace(start, stop, &format!("{tag}{}</{fname}>", escape_text(&m.text)));
                        } else if m.text != text {
                            out.replace(span.start as usize, span.end as usize, &escape_text(&m.text));
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    Ok(out.finish())
}

// ---- the other sheets -----------------------------------------------------------------------

/// **Another sheet's XML**: its formulas, conditional formats and rules
/// that name the sheet changed (`target`), moved; a shared formula whose
/// references move written out in each of its cells.
fn other_sheet(xml: &str, host: &str, target: &str, s: &Shift, formula_at: &dyn Fn(u32, u32) -> Option<String>) -> Result<String, Why> {
    // A quick look first: a sheet that never names it is left as it is (a name XML or a formula would write otherwise: looked through).
    if !target.contains(['\'', '&', '<', '>', '"']) &&!xml.to_lowercase().contains(&target.to_lowercase()) {
        return Ok(xml.to_string());
    }
    let scan = crate::chat::sheets::patch::scan(xml).map_err(|_| BAD_XML.to_string())?;
    let mut unshare = HashSet::new();
    for (si, g) in &scan.shared {
        for &at in &g.cells {
            let f = formula_at(at.0, at.1).ok_or("it has a formula Yap couldn't read")?;
            if formula(&f, Some(host), target, s)?.text != f {
                unshare.insert(*si);
                break;
            }
        }
    }
    let mut cell: Option<(u32, u32)> = None;
    walk(xml, |step| match step {
        Step::Open(path, e) => {
            let name = path.last().map(String::as_str).unwrap_or_default();
            Ok(match name {
                "c" => {
                    cell = attr(e, "r").as_deref().and_then(place_of);
                    Edit::Keep
                }
                "f" if path.iter().any(|p| p == "sheetData") => {
                    let t = attr(e, "t").unwrap_or_default();
                    let shared = t == "shared" && attr(e, "si").and_then(|v| v.parse::<u32>().ok()).is_some_and(|i| unshare.contains(&i));
                    if shared && attr(e, "ref").is_none() {
                        let at = cell.ok_or(BAD_XML)?;
                        let f = formula_at(at.0, at.1).ok_or("it has a formula Yap couldn't read")?;
                        let m = formula(&f, Some(host), target, s)?;
                        let tag = tag_with(e, false, &[("t", None), ("ref", None), ("si", None)]);
                        Edit::Whole(format!("{tag}{}</{}>", escape_text(&m.text), e.name().as_ref()))
                    } else {
                        Edit::Leaf
                    }
                }
                "formula" | "formula1" | "formula2" => Edit::Leaf,
                "f" if path.iter().any(|p| p == "extLst") => Edit::Leaf,
                "cfvo" => match attr(e, "val") {
                    Some(v) => {
                        let m = formula(&v, Some(host), target, s)?;
                        if m.text == v { Edit::Keep } else { Edit::Attrs(vec![("val", Some(m.text))]) }
                    }
                    None => Edit::Keep,
                },
                "hyperlink" => match attr(e, "location").and_then(|l| formula(&l, Some(host), target, s).ok().map(|m| (l, m.text))) {
                    Some((l, now)) if l != now => Edit::Attrs(vec![("location", Some(now))]),
                    _ => Edit::Keep,
                },
                _ => Edit::Keep,
            })
        }
        Step::Text(path, e, text) => {
            let m = formula(text, Some(host), target, s)?;
            let in_data = path.iter().any(|p| p == "sheetData");
            if in_data && attr(e, "t").is_some_and(|t| t == "shared") && attr(e, "si").and_then(|v| v.parse::<u32>().ok()).is_some_and(|i| unshare.contains(&i)) {
                let tag = tag_with(e, false, &[("t", None), ("ref", None), ("si", None)]);
                return Ok(Edit::Whole(format!("{tag}{}</{}>", escape_text(&m.text), e.name().as_ref())));
            }
            if in_data && attr(e, "t").is_some_and(|t| t == "dataTable") {
                return Ok(Edit::Keep);
            }
            Ok(Edit::Text((m.text != text).then_some(m.text)))
        }
    })
}

// ---- drawings, comments, tables -------------------------------------------------------------

/// **A sheet's drawing**: on the sheet changed (`mine`), each picture's,
/// chart's and shape's anchor moved as Excel moves it (`editAs`: "twoCell",
/// the default, moves and sizes with its cells; "oneCell" moves; "absolute"
/// stays); on any sheet, a shape's text linked to a cell (`textlink`).
fn drawing(xml: &str, host: &str, target: &str, s: &Shift, mine: bool) -> Result<String, Why> {
    let rows = s.axis == Axis::Rows;
    // The anchor being read: how it moves, and its start's move (a "oneCell" one's end moves as much).
    let mut edit_as = String::new();
    let mut moved_by: i64 = 0;
    walk(xml, |step| match step {
        Step::Open(path, e) => {
            let name = path.last().map(String::as_str).unwrap_or_default();
            Ok(match name {
                "twoCellAnchor" => {
                    edit_as = attr(e, "editAs").unwrap_or_else(|| "twoCell".into());
                    moved_by = 0;
                    Edit::Keep
                }
                "oneCellAnchor" => {
                    edit_as = "oneCell".into();
                    moved_by = 0;
                    Edit::Keep
                }
                "absoluteAnchor" => {
                    edit_as = "absolute".into();
                    Edit::Keep
                }
                "row" | "col" if mine && ((name == "row") == rows) => {
                    let parent = path.len().checked_sub(2).and_then(|i| path.get(i)).map(String::as_str).unwrap_or_default();
                    if (parent == "from" || parent == "to") && edit_as != "absolute" {
                        Edit::Leaf
                    } else {
                        Edit::Keep
                    }
                }
                "sp" | "cxnSp" => match attr(e, "textlink").filter(|t| !t.is_empty()) {
                    Some(t) => {
                        let m = formula(&t, Some(host), target, s)?;
                        if m.text == t { Edit::Keep } else { Edit::Attrs(vec![("textlink", Some(m.text))]) }
                    }
                    None => Edit::Keep,
                },
                _ => Edit::Keep,
            })
        }
        Step::Text(path, _, text) => {
            let parent = path.len().checked_sub(2).and_then(|i| path.get(i)).map(String::as_str).unwrap_or_default();
            let i: u32 = text.trim().parse().map_err(|_| BAD_XML.to_string())?;
            // (0 is the first line.)
            let now = if parent == "to" && edit_as == "oneCell" {
                (i as i64 + moved_by).max(0) as u32
            } else {
                let line = s.line(i + 1).ok_or("a picture, chart or shape sits on lines that would go")? - 1;
                if parent == "from" {
                    moved_by = line as i64 - i as i64;
                }
                line
            };
            Ok(Edit::Text((now != i).then(|| now.to_string())))
        }
    })
}

/// **The sheet's comments** (`<comment ref>`, a threaded one's too): each
/// moved with its cell; one on a cell that would go refused.
fn comments(xml: &str, s: &Shift) -> Result<String, Why> {
    walk(xml, |step| {
        let Step::Open(path, e) = step else { return Ok(Edit::Keep) };
        let name = path.last().map(String::as_str).unwrap_or_default();
        if name != "comment" && name != "threadedComment" {
            return Ok(Edit::Keep);
        }
        let Some(r) = attr(e, "ref") else { return Ok(Edit::Keep) };
        let ar = area_of(&r).ok_or(Unsure("an area Yap couldn't read"))?;
        let now = s.area(ar).ok_or("a comment is on a cell that would go")?;
        if (now.bottom - now.top, now.right - now.left) != (ar.bottom - ar.top, ar.right - ar.left) {
            return Err("a comment is on a cell that would go".into());
        }
        Ok(if now == ar { Edit::Keep } else { Edit::Attrs(vec![("ref", Some(now.a1()))]) })
    })
}

/// **The comments' notes' boxes** (the legacy VML drawing): each note's
/// cell (`<x:Row>`, `<x:Column>`, from 0) and its box (`<x:Anchor>`: left
/// column, offset, top row, offset, right column, offset, bottom row,
/// offset) moved. A VML part is old and loose XML, so it's edited as text,
/// only inside each `<x:ClientData>`; anything but a note there (a form
/// control) refused.
fn vml(text: &str, s: &Shift) -> Result<String, Why> {
    let rows = s.axis == Axis::Rows;
    if text.contains("ClientData") && !text.contains("<x:ClientData") {
        return Err("it has a drawing Yap can't read".into());
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let line = |i: u32| -> u32 { s.line(i + 1).unwrap_or(s.at.min(if rows { super::LAST_ROW } else { super::LAST_COL })) - 1 };
    while let Some(at) = rest.find("<x:ClientData") {
        out.push_str(&rest[..at]);
        let end = rest[at..].find("</x:ClientData>").map(|i| at + i + "</x:ClientData>".len()).ok_or(BAD_XML)?;
        let block = &rest[at..end];
        let head_end = block.find('>').ok_or(BAD_XML)?;
        if !block[..head_end].contains("ObjectType=\"Note\"") {
            return Err("it has form controls, which Yap can't move".into());
        }
        let mut b = block.to_string();
        let tag = if rows { "x:Row" } else { "x:Column" };
        if let Some(v) = between(&b, tag) {
            let i: u32 = v.trim().parse().map_err(|_| BAD_XML.to_string())?;
            b = with_between(&b, tag, &line(i).to_string());
        }
        if let Some(v) = between(&b, "x:Anchor") {
            let mut nums: Vec<String> = v.split(',').map(|n| n.trim().to_string()).collect();
            if nums.len() == 8 {
                for k in if rows { [2, 6] } else { [0, 4] } {
                    let i: u32 = nums[k].parse().map_err(|_| BAD_XML.to_string())?;
                    nums[k] = line(i).to_string();
                }
                let now = nums.join(", ");
                if now.replace(' ', "") != v.replace([' ', '\r', '\n', '\t'], "") {
                    b = with_between(&b, "x:Anchor", &now);
                }
            }
        }
        out.push_str(&b);
        rest = &rest[end..];
    }
    out.push_str(rest);
    Ok(out)
}

/// The text inside `<tag>…</tag>` in `s`.
fn between<'a>(s: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let a = s.find(&open)? + open.len();
    let b = s[a..].find(&format!("</{tag}>"))? + a;
    Some(&s[a..b])
}

fn with_between(s: &str, tag: &str, text: &str) -> String {
    let open = format!("<{tag}>");
    let Some(a) = s.find(&open).map(|i| i + open.len()) else { return s.to_string() };
    let Some(b) = s[a..].find(&format!("</{tag}>")).map(|i| i + a) else { return s.to_string() };
    format!("{}{text}{}", &s[..a], &s[b..])
}

/// **A table** (`xl/tables/…`): on the sheet changed, its area (and its
/// filter's and sort's) moved — grown by rows put in inside it, shrunk by
/// data rows taken out; refused: rows put in where its calculated columns
/// would need their formula filled in (Excel fills them), its heading or
/// totals row taken out, its columns cut apart. On any sheet, its
/// calculated columns' formulas that name the sheet moved.
fn table(xml: &str, host: &str, target: &str, s: &Shift, mine: bool) -> Result<String, Why> {
    let rows = s.axis == Axis::Rows;
    if mine {
        // The table's own area and what it allows.
        let mut reader = Reader::from_str(xml);
        while let Ok(event) = reader.read_event() {
            match event {
                Event::Eof => break,
                Event::Start(e) | Event::Empty(e) if local(&e) == "table" => {
                    let ar = attr(&e, "ref").as_deref().and_then(area_of).ok_or(BAD_XML)?;
                    let head = attr(&e, "headerRowCount").and_then(|v| v.parse::<u32>().ok()).unwrap_or(1);
                    let totals = attr(&e, "totalsRowCount").and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
                    if rows {
                        if s.insert && s.splits(ar.top, ar.bottom) && xml.contains("calculatedColumnFormula") {
                            return Err("a table's calculated columns would need filling in: insert rows into a table in Excel".into());
                        }
                        if !s.insert && s.splits(ar.top, ar.bottom) {
                            let gone = |r: u32| r >= s.at && r < s.at + s.n;
                            if (head > 0 && gone(ar.top)) || (totals > 0 && gone(ar.bottom)) {
                                return Err("a table's heading or totals row would go".into());
                            }
                            let data = (ar.top + head..=ar.bottom.saturating_sub(totals)).filter(|r| !gone(*r)).count();
                            if data == 0 {
                                return Err("a table would be left with no rows".into());
                            }
                        }
                    } else if s.splits(ar.left, ar.right) || (s.insert && ar.left < s.at && s.at <= ar.right) {
                        return Err("a table's columns would be cut apart: change a table's columns in Excel".into());
                    }
                    break;
                }
                _ => {}
            }
        }
    }
    walk(xml, |step| match step {
        Step::Open(path, e) => {
            let name = path.last().map(String::as_str).unwrap_or_default();
            Ok(match name {
                "table" | "autoFilter" | "sortState" | "sortCondition" if mine => match attr(e, "ref") {
                    Some(r) => match area(&r, s)? {
                        None => return Err("a table's cells would all go".into()),
                        Some(now) if now == r => Edit::Keep,
                        Some(now) => Edit::Attrs(vec![("ref", Some(now))]),
                    },
                    None => Edit::Keep,
                },
                "calculatedColumnFormula" | "totalsRowFormula" => Edit::Leaf,
                _ => Edit::Keep,
            })
        }
        Step::Text(_, _, text) => moved_text_to(text, host, target, s),
    })
}

fn moved_text_to(text: &str, host: &str, target: &str, s: &Shift) -> Result<Edit, Why> {
    let m = formula(text, Some(host), target, s)?;
    Ok(Edit::Text((m.text != text).then_some(m.text)))
}

/// Read every part of a package by name (tests).
#[cfg(test)]
pub fn parts_of(zip: &[u8]) -> HashMap<String, String> {
    use std::io::Read;
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(zip)).unwrap();
    let mut out = HashMap::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let mut s = String::new();
        let _ = f.read_to_string(&mut s);
        out.insert(f.name().map(|n| n.into_owned()).unwrap(), s);
    }
    out
}

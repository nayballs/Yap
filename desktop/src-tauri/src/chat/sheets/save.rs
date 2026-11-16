//! **Saving a sheet edited beside the chat** (16 Nov 2026), back to its
//! file — never over someone else's change.
//!
//! - **The rule** ([`unchanged`]), the Markdown files' (`notes/md_file.rs`):
//!   a save writes the file only while it's still what Yap read (or last
//!   wrote): the same size and time *and* the same bytes. Otherwise nothing
//!   is written and the view says so: Reload, or Save as a copy.
//! - **An `.xlsx`** ([`xlsx_bytes`]): the package Yap read with the cells
//!   that changed written into their sheets' XML (`patch.rs`): every other
//!   part, and every other byte of the sheets, as it was. Before it's
//!   written, the new package is read back and each cell changed must be
//!   what the view shows ([`read_back`]): a save that would come out wrong
//!   is never written.
//! - **A `.csv`**: its text with the records changed, in the file's own
//!   encoding (UTF-8, with its byte-order mark if it had one, or UTF-16) and
//!   line endings.
//! - **Written** atomically (`config::atomic_write_bytes`: a temporary file
//!   beside it, then moved over it), to a link's own file.
//!
//! Never logged but counts and kinds.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::path::Path;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use super::edit::{held_of, put_of, Xlsx};
use super::patch::{self, unescaped_attr, CellPut, SheetPatch};

/// A hash of a file's bytes: what "the same bytes" compares. Pure.
pub fn hash_of(bytes: &[u8]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}

/// **Whether the file at `path` is still `expect`** (its size and time)
/// with bytes hashing to `hash`.
pub fn unchanged(path: &Path, expect: super::view::Stamp, hash: u64) -> bool {
    if super::view::Stamp::of(path) != Some(expect) {
        return false;
    }
    std::fs::read(path).is_ok_and(|b| hash_of(&b) == hash)
}

/// **Write `bytes` to `path`**, atomically, to a link's own file; why not, said plainly.
pub fn write(path: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    crate::config::atomic_write_bytes(&target, bytes).map_err(|e| {
        if crate::config::held(&e) {
            format!("Couldn't save {name}: it's read-only, or another app (Excel?) has it open.")
        } else if e.kind() == std::io::ErrorKind::NotFound {
            format!("Couldn't save {name}: its folder isn't there any more.")
        } else {
            format!("Couldn't save {name}.")
        }
    })
}

/// **A name for a copy of `name`** beside it that isn't taken: "seed-order
/// (copy).xlsx", then "(copy 2)"…
pub fn copy_name(folder: &Path, name: &str) -> String {
    let p = Path::new(name);
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| name.to_string());
    let ext = p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    for n in 1..1000 {
        let candidate = if n == 1 { format!("{stem} (copy){ext}") } else { format!("{stem} (copy {n}){ext}") };
        if !folder.join(&candidate).exists() {
            return candidate;
        }
    }
    format!("{stem} (copy {}){ext}", std::process::id())
}

// ---- the package ------------------------------------------------------------------------------

fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.try_get_attribute(name).ok().flatten().map(|a| unescaped_attr(&a.value))
}

/// An attribute by its local name, whatever its prefix (`r:id`).
fn attr_local(e: &BytesStart, local: &str) -> Option<String> {
    e.attributes().with_checks(false).flatten().find(|a| a.key.as_ref().rsplit(':').next() == Some(local) && a.key.as_ref().contains(':')).map(|a| unescaped_attr(&a.value))
}

fn name_of(e: &BytesStart) -> String {
    e.local_name().as_ref().to_string()
}

pub(super) fn read_part(zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, name: &str) -> Option<String> {
    let mut f = zip.by_name(name).ok()?;
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
}

/// **The worksheets' parts**, in IronCalc's order (the workbook's sheets
/// that are worksheets: `ironcalc::import`'s own reading). Pure.
pub fn sheet_parts(workbook: &str, rels: &str) -> Vec<String> {
    sheets_of(workbook, rels).into_iter().map(|(_, part)| part).collect()
}

/// **The worksheets, by name, and their parts**, in IronCalc's order. Pure.
pub fn sheets_of(workbook: &str, rels: &str) -> Vec<(String, String)> {
    let mut targets: HashMap<String, (String, String)> = HashMap::new();
    let mut reader = Reader::from_str(rels);
    while let Ok(event) = reader.read_event() {
        match event {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) if name_of(&e) == "Relationship" => {
                if let (Some(id), Some(t), Some(target)) = (attr(&e, "Id"), attr(&e, "Type"), attr(&e, "Target")) {
                    targets.insert(id, (t, target));
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    let mut reader = Reader::from_str(workbook);
    while let Ok(event) = reader.read_event() {
        match event {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) if name_of(&e) == "sheet" => {
                let Some(id) = attr_local(&e, "id") else { continue };
                let Some((t, target)) = targets.get(&id) else { continue };
                if t.ends_with("worksheet") {
                    let part = target.strip_prefix('/').map(str::to_string).unwrap_or_else(|| format!("xl/{target}"));
                    out.push((attr(&e, "name").unwrap_or_default(), part));
                }
            }
            _ => {}
        }
    }
    out
}

/// **Whether Yap can write changes into the package `base`**: its sheets
/// are in the form it patches (every row and cell named; not Strict Open
/// XML, which IronCalc doesn't read). Each sheet's array formulas' areas
/// (in IronCalc's order: their cells are refused), or why not.
pub fn check_package(base: &[u8]) -> Result<Vec<Vec<super::Area>>, String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(base)).map_err(|_| "it isn't a workbook".to_string())?;
    let workbook = read_part(&mut zip, "xl/workbook.xml").ok_or("it has no workbook part")?;
    if workbook.contains("purl.oclc.org/ooxml") {
        return Err("it's saved as Strict Open XML".into());
    }
    let rels = read_part(&mut zip, "xl/_rels/workbook.xml.rels").ok_or("it has no workbook part")?;
    let mut arrays = Vec::new();
    for part in sheet_parts(&workbook, &rels) {
        let xml = read_part(&mut zip, &part).ok_or("a sheet's part is missing")?;
        arrays.push(patch::scan(&xml).map_err(|_| "its sheets are written in a way Yap doesn't change".to_string())?.arrays);
    }
    Ok(arrays)
}

/// The `<xf>` elements of `styles`' `<cellXfs>`, as written, and where a new one goes.
struct CellXfs {
    xfs: Vec<String>,
    /// Before `</cellXfs>` (none: `<cellXfs/>`).
    end: Option<usize>,
    /// The `<cellXfs …>` tag.
    tag: (usize, usize),
}

fn cell_xfs(styles: &str) -> Option<CellXfs> {
    let mut reader = Reader::from_str(styles);
    let mut inside = false;
    let mut depth = 0u32;
    let mut xf_start = 0usize;
    let mut out = CellXfs { xfs: Vec::new(), end: None, tag: (0, 0) };
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event().ok()?;
        let end = reader.buffer_position() as usize;
        match event {
            Event::Eof => return None,
            Event::Empty(e) if !inside && name_of(&e) == "cellXfs" => {
                out.tag = (start, end);
                return Some(out);
            }
            Event::Start(e) if !inside && name_of(&e) == "cellXfs" => {
                out.tag = (start, end);
                inside = true;
            }
            Event::Start(_) if inside => {
                if depth == 0 {
                    xf_start = start;
                }
                depth += 1;
            }
            Event::Empty(_) if inside && depth == 0 => out.xfs.push(styles[start..end].to_string()),
            Event::End(e) if inside && depth == 0 && e.local_name().as_ref() == "cellXfs" => {
                out.end = Some(start);
                return Some(out);
            }
            Event::End(_) if inside => {
                depth -= 1;
                if depth == 0 {
                    out.xfs.push(styles[xf_start..end].to_string());
                }
            }
            _ => {}
        }
    }
}

/// An `<xf>` as written with its number format `num` and quote prefix set. Pure.
#[cfg(test)]
pub fn xf_with(xf: &str, num: i32, quote: bool) -> Option<String> {
    xf_with_wrap(xf, num, quote, false)
}

/// An `<xf>` as written with its number format `num`, quote prefix, and —
/// `wrap` — its text wrapped (`<alignment wrapText="1">`: a line break typed,
/// Alt+Enter). Pure.
pub fn xf_with_wrap(xf: &str, num: i32, quote: bool, wrap: bool) -> Option<String> {
    let mut reader = Reader::from_str(xf);
    let (e, end) = loop {
        let event = reader.read_event().ok()?;
        let end = reader.buffer_position() as usize;
        match event {
            Event::Start(e) | Event::Empty(e) => break (e.into_owned(), end),
            Event::Eof => return None,
            _ => {}
        }
    };
    let empty = xf[..end].trim_end().ends_with("/>");
    let mut attrs = String::new();
    for a in e.attributes().with_checks(false).flatten() {
        let key = a.key.as_ref();
        if matches!(key, "numFmtId" | "applyNumberFormat" | "quotePrefix") || (wrap && key == "applyAlignment") {
            continue;
        }
        attrs.push_str(&format!(" {key}=\"{}\"", a.value.replace('"', "&quot;")));
    }
    attrs.push_str(&format!(r#" numFmtId="{num}""#));
    if num != 0 {
        attrs.push_str(r#" applyNumberFormat="1""#);
    }
    if quote {
        attrs.push_str(r#" quotePrefix="1""#);
    }
    if wrap {
        attrs.push_str(r#" applyAlignment="1""#);
    }
    let name = e.name().as_ref().to_string();
    if !wrap {
        return Some(if empty { format!("<{name}{attrs}/>") } else { format!("<{name}{attrs}>{}", &xf[end..]) });
    }
    let p = name.rsplit_once(':').map(|(p, _)| format!("{p}:")).unwrap_or_default();
    if empty {
        return Some(format!(r#"<{name}{attrs}><{p}alignment wrapText="1"/></{name}>"#));
    }
    // Its `<alignment>` (the first of its parts, if it has one) with `wrapText` set.
    let body = &xf[end..];
    let open = format!("<{p}alignment");
    let body = match body.find(&open).filter(|i| matches!(body[i + open.len()..].chars().next(), Some(' ' | '/' | '>'))) {
        Some(i) => {
            let close = body[i..].find('>')? + i;
            let self_closing = body[..close].ends_with('/');
            let inner_end = if self_closing { close - 1 } else { close };
            let attrs_text = &body[i + open.len()..inner_end];
            // (Its own `wrapText` left out, then set.)
            let kept: String = {
                let tag = format!("<x{attrs_text}/>");
                let mut r = Reader::from_str(&tag);
                match r.read_event().ok()? {
                    Event::Empty(t) => t.attributes().with_checks(false).flatten().filter(|a| a.key.as_ref() != "wrapText").map(|a| format!(" {}=\"{}\"", a.key.as_ref(), a.value.replace('"', "&quot;"))).collect(),
                    _ => return None,
                }
            };
            let end_mark = if self_closing { "/>" } else { ">" };
            format!("{}{open}{kept} wrapText=\"1\"{end_mark}{}", &body[..i], &body[close + 1..])
        }
        None => format!(r#"<{p}alignment wrapText="1"/>{body}"#),
    };
    Some(format!("<{name}{attrs}>{body}"))
}

/// A start tag (`<cellXfs count="9">`) with its `count` set to `n`. Pure.
pub fn with_count(tag: &str, n: usize) -> String {
    if let Some(i) = tag.find(" count=\"") {
        let rest = &tag[i + 8..];
        if let Some(close) = rest.find('"') {
            return format!("{} count=\"{n}\"{}", &tag[..i], &rest[close + 1..]);
        }
    }
    let at = if tag.ends_with("/>") { tag.len() - 2 } else { tag.len() - 1 };
    format!("{} count=\"{n}\"{}", tag[..at].trim_end(), &tag[at..])
}

/// The stylesheet's own start tag (`start..end`) and its elements' prefix.
fn root_of(styles: &str) -> Option<(usize, usize, String)> {
    let mut reader = Reader::from_str(styles);
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event().ok()?;
        let end = reader.buffer_position() as usize;
        match event {
            Event::Start(e) => return Some((start, end, e.name().as_ref().rsplit_once(':').map(|(p, _)| format!("{p}:")).unwrap_or_default())),
            Event::Eof => return None,
            _ => {}
        }
    }
}

/// `<numFmts>`: its start tag, and where its end tag starts (none: `<numFmts/>`).
fn num_fmts_at(styles: &str) -> Option<((usize, usize), Option<usize>, usize)> {
    let mut reader = Reader::from_str(styles);
    let mut tag = None;
    let mut n = 0;
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event().ok()?;
        let end = reader.buffer_position() as usize;
        match event {
            Event::Eof => return None,
            Event::Empty(e) if tag.is_none() && name_of(&e) == "numFmts" => return Some(((start, end), None, 0)),
            Event::Start(e) if tag.is_none() && name_of(&e) == "numFmts" => tag = Some((start, end)),
            Event::Start(e) | Event::Empty(e) if tag.is_some() && name_of(&e) == "numFmt" => n += 1,
            Event::End(e) if tag.is_some() && e.local_name().as_ref() == "numFmts" => return Some((tag?, Some(start), n)),
            _ => {}
        }
    }
}

/// **`styles` with `new` xfs added to `<cellXfs>`** and custom number
/// formats `fmts` (id, code) to `<numFmts>` (made, first in the
/// stylesheet, when it has none). None: it couldn't be. Pure.
pub fn styles_with(styles: &str, new: &[String], fmts: &[(i32, String)]) -> Option<String> {
    let mut s = styles.to_string();
    let (_, root_end, p) = root_of(&s)?;
    if !fmts.is_empty() {
        let items: String = fmts.iter().map(|(id, code)| format!(r#"<{p}numFmt numFmtId="{id}" formatCode="{}"/>"#, patch::escape(code))).collect();
        s = match num_fmts_at(&s) {
            Some(((open, tag_end), Some(close), n)) => format!("{}{}{}{items}{}", &s[..open], with_count(&s[open..tag_end], n + fmts.len()), &s[tag_end..close], &s[close..]),
            Some(((open, tag_end), None, _)) => format!(r#"{}<{p}numFmts count="{}">{items}</{p}numFmts>{}"#, &s[..open], fmts.len(), &s[tag_end..]),
            None => format!(r#"{}<{p}numFmts count="{}">{items}</{p}numFmts>{}"#, &s[..root_end], fmts.len(), &s[root_end..]),
        };
    }
    if !new.is_empty() {
        let xfs = cell_xfs(&s)?;
        let end = xfs.end?;
        let tag = with_count(&s[xfs.tag.0..xfs.tag.1], xfs.xfs.len() + new.len());
        s = format!("{}{tag}{}{}{}", &s[..xfs.tag.0], &s[xfs.tag.1..end], new.concat(), &s[end..]);
    }
    Some(s)
}

/// **The workbook part asking Excel to calculate everything when it opens**
/// (`<calcPr fullCalcOnLoad="1">`): what Yap calculated is in the file, and
/// a function IronCalc doesn't know is Excel's to work out. Pure.
pub fn workbook_xml(xml: &str) -> String {
    let mut reader = Reader::from_str(xml);
    let mut prefix = String::new();
    // calcPr's place among the workbook's parts: before these.
    const AFTER: [&str; 9] = ["oleSize", "customWorkbookViews", "pivotCaches", "smartTagPr", "smartTagTypes", "webPublishing", "fileRecoveryPr", "webPublishObjects", "extLst"];
    let mut depth = 0u32;
    loop {
        let start = reader.buffer_position() as usize;
        let Ok(event) = reader.read_event() else { return xml.to_string() };
        let end = reader.buffer_position() as usize;
        match event {
            Event::Eof => return xml.to_string(),
            Event::Start(e) if depth == 0 => {
                prefix = e.name().as_ref().rsplit_once(':').map(|(p, _)| format!("{p}:")).unwrap_or_default();
                depth = 1;
            }
            Event::Start(e) | Event::Empty(e) if depth == 1 && name_of(&e) == "calcPr" => {
                let empty = xml[start..end].trim_end().ends_with("/>");
                let mut attrs = String::new();
                for a in e.attributes().with_checks(false).flatten() {
                    if a.key.as_ref() == "fullCalcOnLoad" {
                        continue;
                    }
                    attrs.push_str(&format!(" {}=\"{}\"", a.key.as_ref(), a.value.replace('"', "&quot;")));
                }
                let name = e.name().as_ref().to_string();
                let close = if empty { "/>" } else { ">" };
                return format!("{}<{name}{attrs} fullCalcOnLoad=\"1\"{close}{}", &xml[..start], &xml[end..]);
            }
            Event::Start(e) | Event::Empty(e) if depth == 1 && AFTER.contains(&name_of(&e).as_str()) => {
                return format!("{}<{prefix}calcPr fullCalcOnLoad=\"1\"/>{}", &xml[..start], &xml[start..]);
            }
            Event::Start(_) => depth += 1,
            Event::End(_) if depth == 1 => {
                return format!("{}<{prefix}calcPr fullCalcOnLoad=\"1\"/>{}", &xml[..start], &xml[start..]);
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
}

/// `xml` without the empty elements `drop` picks (a relationship, a
/// content type). Pure.
fn without(xml: &str, drop: impl Fn(&BytesStart) -> bool) -> String {
    let mut reader = Reader::from_str(xml);
    let mut out = String::with_capacity(xml.len());
    let mut at = 0;
    loop {
        let start = reader.buffer_position() as usize;
        let Ok(event) = reader.read_event() else { return xml.to_string() };
        let end = reader.buffer_position() as usize;
        match event {
            Event::Eof => break,
            Event::Empty(e) if drop(&e) => {
                out.push_str(&xml[at..start]);
                at = end;
            }
            _ => {}
        }
    }
    out.push_str(&xml[at..]);
    out
}

/// **The parts that change when Excel's calculation chain goes**: the
/// workbook's relationships and the content types, without it. Pure.
pub fn without_calc_chain(rels: &str, types: &str) -> (String, String) {
    let rels = without(rels, |e| name_of(e) == "Relationship" && attr(e, "Type").is_some_and(|t| t.ends_with("/calcChain")));
    let types = without(types, |e| name_of(e) == "Override" && attr(e, "PartName").is_some_and(|p| p.eq_ignore_ascii_case("/xl/calcChain.xml")));
    (rels, types)
}

/// **The package `base` with `parts` in place of its own** and `gone` left
/// out; every other entry copied as it is (not even recompressed).
pub fn repack(base: &[u8], parts: &HashMap<String, String>, gone: &HashSet<String>) -> Result<Vec<u8>, String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(base)).map_err(|_| "it isn't a workbook".to_string())?;
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for i in 0..zip.len() {
        let file = zip.by_index_raw(i).map_err(|_| "a part couldn't be read".to_string())?;
        let name = file.name().map(|n| n.into_owned()).unwrap_or_default();
        if gone.contains(&name) {
            continue;
        }
        if let Some(text) = parts.get(&name) {
            drop(file);
            out.start_file(name, options).map_err(|_| "a part couldn't be written".to_string())?;
            out.write_all(text.as_bytes()).map_err(|_| "a part couldn't be written".to_string())?;
        } else {
            out.raw_copy_file(file).map_err(|_| "a part couldn't be copied".to_string())?;
        }
    }
    Ok(out.finish().map_err(|_| "it couldn't be finished".to_string())?.into_inner())
}

/// **The file an `.xlsx` editor's changes make**: its package with each
/// cell changed written in, every other formula's result as calculated
/// now, the styles a typed format needs, Excel asked to calculate on
/// opening, the calculation chain gone. Checked by reading it back.
/// **A save's file**: its bytes, and how IronCalc's styles and number
/// formats made since the file was read map to the file's (kept by the
/// editor once it's written, so the next save adds none twice).
pub struct Written {
    pub bytes: Vec<u8>,
    pub xf_of: HashMap<i32, u32>,
    pub fmt_of: HashMap<i32, i32>,
}

/// **The style a cell changed is written with**: IronCalc's style `ns`
/// (the cell's `was` before) as the file's index — one it had (IronCalc's
/// own below the count it read), one an earlier save added, or a new one
/// made from the cell's own `<xf>` when only its number format (or a text's
/// apostrophe) differs; else the style it had. A custom number format goes
/// in under an id of the file's own (164 and up: Excel's below that are its
/// built-in ones; IronCalc numbers its own from 50 — found in the Lab's
/// first tests, a per cent shown as a date-ish built-in), one with the same
/// code when the file has it.
struct Styling<'a> {
    model: &'a ironcalc::base::Model<'a>,
    /// How many `<xf>`s the file had when it was read (IronCalc's below that are the file's).
    at_open: usize,
    /// The file's `<xf>`s now, as written, and its custom number formats.
    file_xfs: Vec<String>,
    file_fmts: Vec<(i32, String)>,
    xf_of: HashMap<i32, u32>,
    fmt_of: HashMap<i32, i32>,
    new_xfs: Vec<String>,
    new_fmts: Vec<(i32, String)>,
}

impl Styling<'_> {
    fn known(&self, ns: i32) -> Option<u32> {
        if ns >= 0 && (ns as usize) < self.at_open {
            Some(ns as u32)
        } else {
            self.xf_of.get(&ns).copied()
        }
    }

    /// The file's id for IronCalc's number format `id`.
    fn fmt_id(&mut self, id: i32) -> Option<i32> {
        let Some(custom) = self.model.workbook.styles.num_fmts.iter().find(|f| f.num_fmt_id == id) else { return Some(id) };
        // (The file's codes as IronCalc reads them: bare currency signs quoted, `xlsx::quoted_literals`.)
        let as_read = |code: &str| super::xlsx::quoted_literals(code).unwrap_or_else(|| code.to_string());
        if let Some((fid, _)) = self.file_fmts.iter().chain(self.new_fmts.iter()).find(|(_, code)| as_read(code) == custom.format_code) {
            return Some(*fid);
        }
        if let Some(&fid) = self.fmt_of.get(&id) {
            return Some(fid);
        }
        let next = self.file_fmts.iter().chain(self.new_fmts.iter()).map(|(i, _)| *i).max().unwrap_or(0).max(163) + 1;
        self.new_fmts.push((next, custom.format_code.clone()));
        self.fmt_of.insert(id, next);
        Some(next)
    }

    /// The file's index for a cell changed (`ns` now, `was` before), and whether it fell back to its old one.
    fn index(&mut self, ns: i32, was: i32) -> (u32, bool) {
        if let Some(i) = self.known(ns) {
            return (i, false);
        }
        let old = self.known(was).unwrap_or(0);
        let xfs = &self.model.workbook.styles.cell_xfs;
        let (Some(now), Some(then)) = (xfs.get(ns.max(0) as usize).cloned(), xfs.get(was.max(0) as usize).cloned()) else { return (old, true) };
        let Some(raw) = self.file_xfs.get(old as usize).cloned() else { return (old, true) };
        // (Its alignment the same but for its text now wrapped: a line break typed, Alt+Enter.)
        let unwrapped = |a: &Option<ironcalc::base::types::Alignment>| a.clone().map(|mut a| {
            a.wrap_text = false;
            a
        }).filter(|a| *a != ironcalc::base::types::Alignment::default());
        let wraps = |a: &Option<ironcalc::base::types::Alignment>| a.as_ref().is_some_and(|a| a.wrap_text);
        let wrap = wraps(&now.alignment) && !wraps(&then.alignment);
        if !(now.font_id == then.font_id && now.fill_id == then.fill_id && now.border_id == then.border_id && (now.alignment == then.alignment || (wrap && unwrapped(&now.alignment) == unwrapped(&then.alignment)))) {
            return (old, true);
        }
        let Some(num) = self.fmt_id(now.num_fmt_id) else { return (old, true) };
        let Some(xf) = xf_with_wrap(&raw, num, now.quote_prefix, wrap) else { return (old, true) };
        self.new_xfs.push(xf);
        let i = (self.file_xfs.len() + self.new_xfs.len() - 1) as u32;
        self.xf_of.insert(ns, i);
        (i, false)
    }
}

/// How many `<xf>`s a workbook's `<cellXfs>` has (none: 0).
pub fn xf_count(bytes: &[u8]) -> usize {
    let Ok(mut zip) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else { return 0 };
    read_part(&mut zip, "xl/styles.xml").and_then(|s| cell_xfs(&s)).map(|c| c.xfs.len()).unwrap_or(0)
}

/// The custom number formats `styles` has: (id, code).
fn num_fmts_of(styles: &str) -> Vec<(i32, String)> {
    let mut out = Vec::new();
    let mut reader = Reader::from_str(styles);
    while let Ok(event) = reader.read_event() {
        match event {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) if name_of(&e) == "numFmt" => {
                if let (Some(id), Some(code)) = (attr(&e, "numFmtId").and_then(|s| s.parse().ok()), attr(&e, "formatCode")) {
                    out.push((id, code));
                }
            }
            _ => {}
        }
    }
    out
}

pub fn xlsx_bytes(x: &Xlsx) -> Result<Written, String> {
    let base = &x.base;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(base.as_slice())).map_err(|_| "it isn't a workbook".to_string())?;
    let workbook = read_part(&mut zip, "xl/workbook.xml").ok_or("it has no workbook part")?;
    let rels = read_part(&mut zip, "xl/_rels/workbook.xml.rels").ok_or("it has no workbook part")?;
    let parts_of = sheet_parts(&workbook, &rels);
    let model = x.model.get_model();
    let styles_xml = read_part(&mut zip, "xl/styles.xml").unwrap_or_default();
    let mut styling = Styling {
        model,
        at_open: x.xfs_at_open,
        file_xfs: cell_xfs(&styles_xml).map(|c| c.xfs).unwrap_or_default(),
        file_fmts: num_fmts_of(&styles_xml),
        xf_of: x.xf_of.clone(),
        fmt_of: x.fmt_of.clone(),
        new_xfs: Vec::new(),
        new_fmts: Vec::new(),
    };
    // Each sheet's changes; the cells whose look the file shows as the view does.
    let mut by_sheet: BTreeMap<u32, SheetPatch> = BTreeMap::new();
    let mut looks_kept: Vec<(u32, i32, i32)> = Vec::new();
    for (&(s, r, c), was) in x.changed() {
        let mut put = put_of(model, s, r, c);
        let (style, fell_back) = styling.index(put.style as i32, was.style);
        put.style = style;
        if !fell_back {
            looks_kept.push((s, r, c));
        }
        by_sheet.entry(s).or_default().puts.insert((r as u32, c as u32), put);
    }
    let changed_sheets: Vec<u32> = by_sheet.keys().copied().collect();
    let mut parts: HashMap<String, String> = HashMap::new();
    for (si, ws) in model.workbook.worksheets.iter().enumerate() {
        let si = si as u32;
        let Some(part) = parts_of.get(si as usize) else { continue };
        let mut sheet_patch = by_sheet.remove(&si).unwrap_or_default();
        // Every other formula's result, as calculated now.
        for (r, c, cell) in ws.sheet_data.cells() {
            if r < 1 || c < 1 || cell.get_formula().is_none() || sheet_patch.puts.contains_key(&(r as u32, c as u32)) || super::xlsx::not_known(&cell) {
                continue;
            }
            let v = match &*cell {
                ironcalc::base::types::Cell::CellFormula { v, .. } | ironcalc::base::types::Cell::ArrayFormula { v, .. } => super::edit::result_of(v),
                _ => None,
            };
            if let Some(v) = v {
                sheet_patch.results.insert((r as u32, c as u32), v);
            }
        }
        if sheet_patch.puts.is_empty() && !changed_sheets.contains(&si) && sheet_patch.results.is_empty() {
            continue;
        }
        let xml = read_part(&mut zip, part).ok_or("a sheet's part is missing")?;
        let scan = patch::scan(&xml)?;
        // A shared formula's first cell changed: every cell sharing it written out in full.
        let masters: Vec<(u32, u32)> = sheet_patch.puts.keys().filter(|at| scan.master_of(**at).is_some()).copied().collect();
        for at in masters {
            let Some(shared) = scan.master_of(at) else { continue };
            for &(r, c) in &shared.cells {
                if !sheet_patch.puts.contains_key(&(r, c)) {
                    let put: CellPut = put_of(model, si, r as i32, c as i32);
                    sheet_patch.results.remove(&(r, c));
                    sheet_patch.puts.insert((r, c), put);
                }
            }
        }
        if sheet_patch.puts.keys().any(|at| scan.in_array(*at)) {
            return Err("a cell changed is part of an array formula".into());
        }
        let patched = patch::patch_sheet(&xml, &sheet_patch)?;
        if patched != xml {
            parts.insert(part.clone(), patched);
        }
    }
    if !styling.new_xfs.is_empty() || !styling.new_fmts.is_empty() {
        let styles = styles_with(&styles_xml, &styling.new_xfs, &styling.new_fmts).ok_or("its styles couldn't take a new number format")?;
        parts.insert("xl/styles.xml".into(), styles);
    }
    parts.insert("xl/workbook.xml".into(), workbook_xml(&workbook));
    let mut gone = HashSet::new();
    if zip.by_name("xl/calcChain.xml").is_ok() {
        gone.insert("xl/calcChain.xml".to_string());
        let types = read_part(&mut zip, "[Content_Types].xml").unwrap_or_default();
        let (rels, types) = without_calc_chain(&rels, &types);
        parts.insert("xl/_rels/workbook.xml.rels".into(), rels);
        if !types.is_empty() {
            parts.insert("[Content_Types].xml".into(), types);
        }
    }
    drop(zip);
    let bytes = repack(base, &parts, &gone)?;
    read_back(x, &bytes, &looks_kept)?;
    Ok(Written { bytes, xf_of: styling.xf_of, fmt_of: styling.fmt_of })
}

/// **Each cell changed, read back from `bytes`**, holds what the editor
/// has — and, `looks`, shows it the same (its number format too).
fn read_back(x: &Xlsx, bytes: &[u8], looks: &[(u32, i32, i32)]) -> Result<(), String> {
    let (again, _) = super::xlsx::model_now(&super::xlsx::complete(bytes), x.locale).map_err(|_| "the saved file wouldn't read back".to_string())?;
    let model = x.model.get_model();
    for (&(s, r, c), _) in x.changed() {
        let shown = |m: &ironcalc::base::Model| m.get_formatted_cell_value(s, r, c).ok();
        if held_of(&again, s, r, c) != held_of(model, s, r, c) || (looks.contains(&(s, r, c)) && shown(&again) != shown(model)) {
            tracing::warn!(sheet = s, "Chat: a sheet's save read back different, so it wasn't written");
            return Err("the saved file didn't read back as edited".into());
        }
    }
    Ok(())
}

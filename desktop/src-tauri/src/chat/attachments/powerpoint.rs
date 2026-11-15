//! **A PowerPoint presentation's text** (`.pptx`, `.pptm`, `.ppsx`; 16 Nov
//! 2026), read on this PC as `word.rs` reads a `.docx`: the file is a zip of
//! XML. Each slide's words in the presentation's order (`ppt/presentation.xml`'s
//! slide list, through its relationships), then its speaker notes:
//!
//! ```text
//! Slide 3: Spring planting
//! Beans along the north fence
//! Speaker notes: Mention the frost dates.
//! ```
//!
//! - **A slide's text**: its title first ("Slide 3: <title>"), then each
//!   shape's paragraphs in the order the slide keeps them, a table's row a
//!   line (its cells by tabs), a SmartArt diagram's words after them.
//!   Hidden slides say so ("Slide 4 (hidden): …"); a slide with no words,
//!   "(no text)".
//! - **Speaker notes**: the notes page's body only (never its slide number
//!   or the slide's picture).
//! - No pictures, charts' numbers, comments or styles. A file whose slide
//!   list can't be read still gives its slides, by their numbers.
//! - Bounded: each part at most [`MOST`] bytes, all of them [`MOST_ALL`],
//!   at most [`MOST_SLIDES`] slides (a zip bomb's room for mischief).

use std::collections::HashMap;
use std::io::Read;

/// The most of one part read.
const MOST: u64 = 64 * 1024 * 1024;
/// The most of all the parts read together.
const MOST_ALL: u64 = 256 * 1024 * 1024;
/// The most slides read.
const MOST_SLIDES: usize = 5_000;

/// The zip, read part by part within the limits.
struct Parts<'a> {
    zip: zip::ZipArchive<std::io::Cursor<&'a [u8]>>,
    read: u64,
}

impl Parts<'_> {
    /// A part's text (`None`: not there, or past the limits).
    fn get(&mut self, name: &str) -> Option<String> {
        let left = MOST_ALL.saturating_sub(self.read).min(MOST);
        let entry = self.zip.by_name(name).ok()?;
        let mut xml = String::new();
        entry.take(left).read_to_string(&mut xml).ok()?;
        self.read += xml.len() as u64;
        Some(xml)
    }

    fn names(&self) -> Vec<String> {
        self.zip.file_names().filter_map(|n| n.ok().map(|n| n.into_owned())).collect()
    }
}

/// **The text of a presentation**: each slide's, then its notes; `None`
/// when `bytes` isn't one. Reads `bytes`.
pub fn text(bytes: &[u8]) -> Option<String> {
    let zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
    let mut parts = Parts { zip, read: 0 };
    let slides = slide_paths(&mut parts)?;
    let mut out = Vec::new();
    for (n, path) in slides.iter().take(MOST_SLIDES).enumerate() {
        let Some(xml) = parts.get(path) else { continue };
        let rels = parts.get(&rels_of(path)).map(|r| relationships(&r)).unwrap_or_default();
        let mut slide = Slide::of_xml(&xml);
        // A SmartArt diagram's words live in their own part.
        for target in rels.values().filter(|(kind, _)| kind.ends_with("/diagramData")).map(|(_, t)| resolve(path, t)) {
            if let Some(data) = parts.get(&target) {
                slide.lines.extend(paragraphs(&data).into_iter().filter(|l| !l.trim().is_empty()));
            }
        }
        let notes = rels
            .values()
            .find(|(kind, _)| kind.ends_with("/notesSlide"))
            .and_then(|(_, t)| parts.get(&resolve(path, t)))
            .map(|x| notes_of(&x))
            .unwrap_or_default();
        out.push(slide.said(n + 1, &notes));
    }
    Some(out.join("\n\n"))
}

/// The slides' parts in the presentation's order: its slide list through
/// its relationships, else every `ppt/slides/slideN.xml` by its number.
/// `None`: not a presentation.
fn slide_paths(parts: &mut Parts) -> Option<Vec<String>> {
    let listed = parts.get("ppt/presentation.xml").zip(parts.get("ppt/_rels/presentation.xml.rels")).map(|(pres, rels)| {
        let rels = relationships(&rels);
        slide_ids(&pres).iter().filter_map(|id| rels.get(id)).map(|(_, target)| resolve("ppt/presentation.xml", target)).collect::<Vec<_>>()
    });
    if let Some(list) = listed.filter(|l| !l.is_empty()) {
        return Some(list);
    }
    let mut numbered: Vec<(u32, String)> = parts
        .names()
        .into_iter()
        .filter_map(|n| n.strip_prefix("ppt/slides/slide").and_then(|r| r.strip_suffix(".xml")).and_then(|d| d.parse().ok()).map(|d| (d, n.clone())))
        .collect();
    if numbered.is_empty() {
        // (A presentation with no slides is still one: its text is empty.)
        return parts.names().iter().any(|n| n == "ppt/presentation.xml").then(Vec::new);
    }
    numbered.sort();
    Some(numbered.into_iter().map(|(_, n)| n).collect())
}

/// The relationship ids of `presentation.xml`'s slide list, in order. Pure.
pub fn slide_ids(xml: &str) -> Vec<String> {
    let list = xml.find("<p:sldIdLst").map(|at| &xml[at..]).unwrap_or("");
    let list = &list[..list.find("</p:sldIdLst>").unwrap_or(list.len())];
    tags(list).filter(|t| t.name == "p:sldId").filter_map(|t| attr(t.raw, "r:id")).collect()
}

/// A part's relationships: id → (type, target). Pure.
pub fn relationships(xml: &str) -> HashMap<String, (String, String)> {
    tags(xml)
        .filter(|t| t.name == "Relationship")
        .filter(|t| attr(t.raw, "TargetMode").is_none_or(|m| m != "External"))
        .filter_map(|t| Some((attr(t.raw, "Id")?, (attr(t.raw, "Type").unwrap_or_default(), attr(t.raw, "Target")?))))
        .collect()
}

/// The relationships part of `part` ("ppt/slides/slide1.xml" →
/// "ppt/slides/_rels/slide1.xml.rels"). Pure.
fn rels_of(part: &str) -> String {
    match part.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part}.rels"),
    }
}

/// A relationship's `target` from `part`'s folder, as the zip names it
/// ("../notesSlides/notesSlide1.xml" from "ppt/slides/slide1.xml" →
/// "ppt/notesSlides/notesSlide1.xml"; one starting with "/" from the
/// root). Pure.
pub fn resolve(part: &str, target: &str) -> String {
    let mut path: Vec<&str> = if target.starts_with('/') { Vec::new() } else { part.split('/').collect() };
    if !target.starts_with('/') {
        path.pop();
    }
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                path.pop();
            }
            s => path.push(s),
        }
    }
    path.join("/")
}

/// One slide's words: its title, its other lines, and whether it's hidden.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Slide {
    pub title: String,
    pub lines: Vec<String>,
    pub hidden: bool,
}

impl Slide {
    /// A slide's XML read: the title placeholder's words as its title, every
    /// other shape's lines in order. Pure.
    pub fn of_xml(xml: &str) -> Slide {
        let hidden = tags(xml).find(|t| t.name == "p:sld").and_then(|t| attr(t.raw, "show")).is_some_and(|v| v == "0" || v == "false");
        let mut slide = Slide { hidden, ..Slide::default() };
        for shape in shapes(xml) {
            let lines: Vec<String> = paragraphs(shape.xml).into_iter().filter(|l| !l.trim().is_empty()).collect();
            if shape.title && slide.title.is_empty() && !lines.is_empty() {
                slide.title = lines.iter().map(|l| l.trim()).collect::<Vec<_>>().join(" ");
            } else {
                slide.lines.extend(lines);
            }
        }
        slide
    }

    /// The slide as the model reads it: "Slide 3: <title>", its lines, its
    /// notes. Pure.
    pub fn said(&self, n: usize, notes: &[String]) -> String {
        let hidden = if self.hidden { " (hidden)" } else { "" };
        let mut out = match (self.title.is_empty(), self.lines.is_empty()) {
            (false, _) => format!("Slide {n}{hidden}: {}", self.title),
            (true, false) => format!("Slide {n}{hidden}:"),
            (true, true) => format!("Slide {n}{hidden}: (no text)"),
        };
        for line in &self.lines {
            out.push('\n');
            out.push_str(line);
        }
        match notes {
            [] => {}
            [one] => out.push_str(&format!("\nSpeaker notes: {one}")),
            many => out.push_str(&format!("\nSpeaker notes:\n{}", many.join("\n"))),
        }
        out
    }
}

/// A notes page's words: its body placeholder's lines only (never the
/// slide number, the date or the slide's picture). Pure.
pub fn notes_of(xml: &str) -> Vec<String> {
    shapes(xml).into_iter().filter(|s| s.body).flat_map(|s| paragraphs(s.xml)).filter(|l| !l.trim().is_empty()).collect()
}

/// A shape of a slide: its XML, and whether it's the title or the body
/// placeholder.
struct Shape<'a> {
    xml: &'a str,
    title: bool,
    body: bool,
}

/// The shapes with words in `xml`, in order: each `<p:sp>` (a text box, a
/// placeholder) and `<p:graphicFrame>` (a table). A group's shapes are
/// each their own. Pure.
fn shapes(xml: &str) -> Vec<Shape<'_>> {
    let mut out = Vec::new();
    let mut rest = xml;
    loop {
        let sp = rest.find("<p:sp>").or_else(|| rest.find("<p:sp "));
        let frame = rest.find("<p:graphicFrame>").or_else(|| rest.find("<p:graphicFrame "));
        let (at, close) = match (sp, frame) {
            (Some(s), Some(f)) if f < s => (f, "</p:graphicFrame>"),
            (Some(s), _) => (s, "</p:sp>"),
            (None, Some(f)) => (f, "</p:graphicFrame>"),
            (None, None) => break,
        };
        let after = &rest[at..];
        let end = after.find(close).map(|e| e + close.len()).unwrap_or(after.len());
        let shape = &after[..end];
        let ph = tags(shape).find(|t| t.name == "p:ph").map(|t| attr(t.raw, "type").unwrap_or_default());
        out.push(Shape {
            xml: shape,
            title: matches!(ph.as_deref(), Some("title" | "ctrTitle")),
            // (A notes page's body placeholder: "body", or none named.)
            body: matches!(ph.as_deref(), Some("body" | "")),
        });
        rest = &after[end..];
    }
    out
}

/// The paragraphs of DrawingML text in `xml`: each `<a:p>` a line (its
/// `<a:t>` runs, `<a:br/>` a line break), a table's row a line with its
/// cells by tabs; entities decoded. Pure.
pub fn paragraphs(xml: &str) -> Vec<String> {
    let mut out = String::new();
    let mut in_cell = 0usize;
    for t in tags(xml) {
        match (t.name, t.closing, t.empty) {
            ("a:t", false, false) | ("dgm:t", false, false) => out.push_str(&super::word::unescape(t.text_after)),
            ("a:br", _, _) => out.push('\n'),
            ("a:tc", false, false) => in_cell += 1,
            ("a:tc", true, _) => {
                in_cell = in_cell.saturating_sub(1);
                out.push('\t');
            }
            ("a:tr", true, _) => {
                while out.ends_with('\t') {
                    out.pop();
                }
                out.push('\n');
            }
            ("a:p", true, _) | ("a:p", false, true) => out.push(if in_cell > 0 { ' ' } else { '\n' }),
            _ => {}
        }
    }
    out.split('\n').map(|l| l.trim_end().replace(" \t", "\t")).filter(|l| !l.is_empty()).collect()
}

/// A tag of XML: its name, whether it closes or is empty, its whole text
/// (attributes and all) and, for a text run, the text up to its end tag.
struct Tag<'a> {
    name: &'a str,
    closing: bool,
    empty: bool,
    raw: &'a str,
    text_after: &'a str,
}

/// The tags of `xml`, in order (comments and declarations are tags too,
/// with odd names: never matched). Pure.
fn tags(xml: &str) -> impl Iterator<Item = Tag<'_>> {
    let mut rest = xml;
    std::iter::from_fn(move || {
        let lt = rest.find('<')?;
        let gt = rest[lt..].find('>').map(|g| lt + g)?;
        let raw = &rest[lt + 1..gt];
        let name = raw.trim_start_matches('/').split([' ', '/', '\t', '\n', '\r']).next().unwrap_or("");
        let closing = raw.starts_with('/');
        let empty = raw.ends_with('/');
        rest = &rest[gt + 1..];
        let text_after = if !closing && !empty && (name == "a:t" || name == "dgm:t") {
            let end = rest.find('<').unwrap_or(rest.len());
            &rest[..end]
        } else {
            ""
        };
        Some(Tag { name, closing, empty, raw, text_after })
    })
}

/// An attribute's value in a tag's text (`name="value"` or `'value'`),
/// entities decoded. Pure.
fn attr(raw: &str, name: &str) -> Option<String> {
    let mut rest = raw;
    while let Some(at) = rest.find(name) {
        let before_ok = at == 0 || rest[..at].ends_with(|c: char| c.is_whitespace());
        let after = rest[at + name.len()..].trim_start();
        if before_ok {
            if let Some(v) = after.strip_prefix('=') {
                let v = v.trim_start();
                let quote = v.chars().next().filter(|q| *q == '"' || *q == '\'')?;
                let body = &v[1..];
                let end = body.find(quote)?;
                return Some(super::word::unescape(&body[..end]));
            }
        }
        rest = &rest[at + name.len()..];
    }
    None
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A slide's XML: a title, a body, a table; fictional words.
    pub(crate) const SLIDE: &str = r#"<?xml version="1.0"?><p:sld xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree>
<p:sp><p:nvSpPr><p:cNvPr id="3" name="Body"/><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>Beans along the north fence</a:t></a:r></a:p><a:p><a:r><a:t>Peas </a:t></a:r><a:r><a:t>&amp; sweet peas</a:t></a:r><a:br/><a:r><a:t>by the shed</a:t></a:r></a:p></p:txBody></p:sp>
<p:sp><p:nvSpPr><p:cNvPr id="2" name="Title"/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>Spring planting</a:t></a:r></a:p></p:txBody></p:sp>
<p:graphicFrame><a:graphic><a:graphicData><a:tbl><a:tr><a:tc><a:txBody><a:p><a:r><a:t>Bed</a:t></a:r></a:p></a:txBody></a:tc><a:tc><a:txBody><a:p><a:r><a:t>Crop</a:t></a:r></a:p></a:txBody></a:tc></a:tr><a:tr><a:tc><a:txBody><a:p><a:r><a:t>4</a:t></a:r></a:p></a:txBody></a:tc><a:tc><a:txBody><a:p><a:r><a:t>Leeks</a:t></a:r></a:p><a:p><a:r><a:t>late</a:t></a:r></a:p></a:txBody></a:tc></a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>
</p:spTree></p:cSld></p:sld>"#;

    /// A notes page: the slide's picture, the body, the slide number.
    pub(crate) const NOTES: &str = r#"<p:notes xmlns:p="p" xmlns:a="a"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:nvPr><p:ph type="sldImg"/></p:nvPr></p:nvSpPr></p:sp><p:sp><p:nvSpPr><p:nvPr><p:ph type="body" idx="1"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>Mention the frost dates.</a:t></a:r></a:p></p:txBody></p:sp><p:sp><p:nvSpPr><p:nvPr><p:ph type="sldNum" sz="quarter" idx="5"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:fld id="x" type="slidenum"><a:t>2</a:t></a:fld></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:notes>"#;

    #[test]
    fn a_slides_title_comes_first_then_its_lines_and_table() {
        let s = Slide::of_xml(SLIDE);
        assert_eq!(s.title, "Spring planting");
        assert_eq!(s.lines, vec!["Beans along the north fence", "Peas & sweet peas", "by the shed", "Bed\tCrop", "4\tLeeks late"]);
        assert!(!s.hidden);
        assert_eq!(notes_of(NOTES), vec!["Mention the frost dates."]);
        assert_eq!(
            s.said(3, &notes_of(NOTES)),
            "Slide 3: Spring planting\nBeans along the north fence\nPeas & sweet peas\nby the shed\nBed\tCrop\n4\tLeeks late\nSpeaker notes: Mention the frost dates."
        );
    }

    #[test]
    fn a_hidden_slide_an_empty_one_and_one_with_no_title_say_so() {
        let hidden = Slide::of_xml(r#"<p:sld show="0"><p:sp><p:txBody><a:p><a:r><a:t>Old prices</a:t></a:r></a:p></p:txBody></p:sp></p:sld>"#);
        assert_eq!(hidden.said(4, &[]), "Slide 4 (hidden):\nOld prices");
        assert_eq!(Slide::default().said(5, &[]), "Slide 5: (no text)");
        assert_eq!(Slide::default().said(6, &["One".into(), "Two".into()]), "Slide 6: (no text)\nSpeaker notes:\nOne\nTwo");
    }

    #[test]
    fn relationships_resolve_to_the_zips_names() {
        let rels = relationships(r#"<Relationships><Relationship Id="rId2" Type="http://x/slide" Target="slides/slide2.xml"/><Relationship Id='rId3' Type="http://x/slide" Target="/ppt/slides/slide1.xml"/><Relationship Id="rId9" Type="http://x/hyperlink" Target="https://example.com" TargetMode="External"/></Relationships>"#);
        assert_eq!(rels.get("rId2").map(|r| r.1.as_str()), Some("slides/slide2.xml"));
        assert_eq!(rels.get("rId3").map(|r| r.1.as_str()), Some("/ppt/slides/slide1.xml"));
        assert!(!rels.contains_key("rId9"));
        assert_eq!(resolve("ppt/presentation.xml", "slides/slide2.xml"), "ppt/slides/slide2.xml");
        assert_eq!(resolve("ppt/slides/slide1.xml", "../notesSlides/notesSlide1.xml"), "ppt/notesSlides/notesSlide1.xml");
        assert_eq!(resolve("ppt/presentation.xml", "/ppt/slides/slide1.xml"), "ppt/slides/slide1.xml");
        assert_eq!(rels_of("ppt/slides/slide1.xml"), "ppt/slides/_rels/slide1.xml.rels");
        assert_eq!(slide_ids(r#"<p:presentation><p:sldMasterIdLst><p:sldMasterId id="1" r:id="rId1"/></p:sldMasterIdLst><p:sldIdLst><p:sldId id="256" r:id="rId3"/><p:sldId id="257" r:id="rId2"/></p:sldIdLst></p:presentation>"#), vec!["rId3", "rId2"]);
    }

    /// A `.pptx` made here: two slides listed in the opposite order to
    /// their numbers, the second with notes and a SmartArt diagram.
    pub(crate) fn pptx() -> Vec<u8> {
        let pres = r#"<p:presentation xmlns:p="p" xmlns:r="r"><p:sldIdLst><p:sldId id="256" r:id="rId3"/><p:sldId id="257" r:id="rId2"/></p:sldIdLst></p:presentation>"#;
        let pres_rels = r#"<Relationships><Relationship Id="rId1" Type="http://x/slideMaster" Target="slideMasters/slideMaster1.xml"/><Relationship Id="rId2" Type="http://x/slide" Target="slides/slide1.xml"/><Relationship Id="rId3" Type="http://x/slide" Target="slides/slide2.xml"/></Relationships>"#;
        let first = r#"<p:sld><p:cSld><p:spTree><p:sp><p:nvSpPr><p:nvPr><p:ph type="ctrTitle"/></p:nvPr></p:nvSpPr><p:txBody><a:p><a:r><a:t>Allotment year</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>"#;
        let slide_rels = r#"<Relationships><Relationship Id="rId1" Type="http://x/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="rId2" Type="http://x/notesSlide" Target="../notesSlides/notesSlide1.xml"/><Relationship Id="rId3" Type="http://x/diagramData" Target="../diagrams/data1.xml"/></Relationships>"#;
        let diagram = r#"<dgm:dataModel><dgm:ptLst><dgm:pt modelId="1"><dgm:t><a:bodyPr/><a:p><a:r><a:t>Dig</a:t></a:r></a:p></dgm:t></dgm:pt><dgm:pt modelId="2"><dgm:t><a:p><a:r><a:t>Sow</a:t></a:r></a:p></dgm:t></dgm:pt></dgm:ptLst></dgm:dataModel>"#;
        super::super::tests::zip_of(&[
            ("[Content_Types].xml", "<Types/>"),
            ("ppt/presentation.xml", pres),
            ("ppt/_rels/presentation.xml.rels", pres_rels),
            ("ppt/slides/slide1.xml", SLIDE),
            ("ppt/slides/_rels/slide1.xml.rels", slide_rels),
            ("ppt/notesSlides/notesSlide1.xml", NOTES),
            ("ppt/diagrams/data1.xml", diagram),
            ("ppt/slides/slide2.xml", first),
            ("ppt/slideMasters/slideMaster1.xml", r#"<p:sldMaster><p:sp><p:txBody><a:p><a:r><a:t>Click to edit Master title style</a:t></a:r></a:p></p:txBody></p:sp></p:sldMaster>"#),
        ])
    }

    #[test]
    fn a_pptx_made_here_reads_in_the_presentations_order_with_notes() {
        assert_eq!(
            text(&pptx()).unwrap(),
            "Slide 1: Allotment year\n\nSlide 2: Spring planting\nBeans along the north fence\nPeas & sweet peas\nby the shed\nBed\tCrop\n4\tLeeks late\nDig\nSow\nSpeaker notes: Mention the frost dates."
        );
    }

    #[test]
    fn without_its_slide_list_the_slides_go_by_their_numbers_and_other_zips_arent_presentations() {
        let bare = super::super::tests::zip_of(&[("ppt/slides/slide10.xml", "<p:sld><p:sp><a:p><a:r><a:t>Ten</a:t></a:r></a:p></p:sp></p:sld>"), ("ppt/slides/slide9.xml", "<p:sld><p:sp><a:p><a:r><a:t>Nine</a:t></a:r></a:p></p:sp></p:sld>")]);
        assert_eq!(text(&bare).unwrap(), "Slide 1:\nNine\n\nSlide 2:\nTen");
        assert_eq!(text(&super::super::tests::zip_of(&[("word/document.xml", "<w:document/>")])), None);
        assert_eq!(text(b"not a zip"), None);
        let empty = super::super::tests::zip_of(&[("ppt/presentation.xml", "<p:presentation/>")]);
        assert_eq!(text(&empty).as_deref(), Some(""));
    }
}

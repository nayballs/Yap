//! **A Word document's text** (`.docx`), read on this PC: the file is a zip
//! whose `word/document.xml` holds the paragraphs. Their words in order — a
//! paragraph a line, tabs and line breaks kept, a heading marked as
//! Markdown's `#` (16 Nov 2026: its Heading 1–6 or Title style, or its
//! outline level, from `word/styles.xml`; a project's big files are cut at
//! them, `projects/parts.rs`) — and nothing else (no other styles, comments
//! or pictures). An old `.doc` isn't this format.

use std::collections::HashMap;
use std::io::Read;

/// The most of `document.xml` read (a zip bomb's room for mischief).
const MOST: u64 = 64 * 1024 * 1024;

/// The text of a `.docx`'s body; `None` when it isn't one. Reads `bytes`.
pub fn text(bytes: &[u8]) -> Option<String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
    let levels = zip
        .by_name("word/styles.xml")
        .ok()
        .and_then(|e| {
            let mut s = String::new();
            e.take(MOST).read_to_string(&mut s).ok().map(|_| s)
        })
        .map(|s| heading_styles(&s))
        .unwrap_or_default();
    let entry = zip.by_name("word/document.xml").ok()?;
    let mut xml = String::new();
    entry.take(MOST).read_to_string(&mut xml).ok()?;
    Some(of_xml_with(&xml, &levels))
}

/// A tag's attribute `name`'s value (`w:val="Heading1"` → "Heading1").
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let at = tag.find(&format!("{name}=\""))? + name.len() + 2;
    let end = tag[at..].find('"')?;
    Some(&tag[at..at + end])
}

/// **The heading styles of `styles.xml`**: each style's id and its level —
/// a built-in "heading N" (its name stays English whatever Word's language)
/// or "Title", else its outline level (`w:outlineLvl` N is level N + 1; 9
/// is body text). Pure.
pub fn heading_styles(xml: &str) -> HashMap<String, usize> {
    let mut out = HashMap::new();
    let mut rest = xml;
    while let Some(at) = rest.find("<w:style ") {
        let after = &rest[at..];
        let end = after.find("</w:style>").unwrap_or(after.len());
        let style = &after[..end];
        rest = &after[end.min(after.len())..];
        if end == after.len() {
            rest = "";
        }
        let Some(gt) = style.find('>') else { continue };
        let Some(id) = attr(&style[..gt], "w:styleId") else { continue };
        let name = style.find("<w:name ").and_then(|n| attr(&style[n..], "w:val")).unwrap_or("").to_lowercase();
        let level = if name == "title" {
            Some(1)
        } else if let Some(n) = name.strip_prefix("heading ") {
            n.trim().parse::<usize>().ok().filter(|n| (1..=9).contains(n))
        } else {
            style.find("<w:outlineLvl ").and_then(|o| attr(&style[o..], "w:val")).and_then(|v| v.parse::<usize>().ok()).filter(|v| *v < 9).map(|v| v + 1)
        };
        if let Some(l) = level {
            out.insert(id.to_string(), l);
        }
    }
    out
}

/// A paragraph style's heading level: from `styles.xml`, else its id as
/// Word's English names it ("Heading2", "Title").
fn level_of(style: &str, levels: &HashMap<String, usize>) -> Option<usize> {
    if let Some(l) = levels.get(style) {
        return Some(*l);
    }
    let s = style.to_lowercase().replace(' ', "");
    if s == "title" {
        return Some(1);
    }
    s.strip_prefix("heading").and_then(|n| n.parse::<usize>().ok()).filter(|n| (1..=9).contains(n))
}

/// The words of `document.xml`, no heading styles known ([`of_xml_with`]).
/// Pure.
#[cfg(test)]
pub fn of_xml(xml: &str) -> String {
    of_xml_with(xml, &HashMap::new())
}

/// The words of `document.xml`: each `<w:t>`'s text, a line at each
/// paragraph's end, `<w:tab/>` a tab, `<w:br/>` / `<w:cr/>` a line, a
/// heading's paragraph after its `#`s (`levels`: [`heading_styles`]; at most
/// six); entities decoded. Pure.
pub fn of_xml_with(xml: &str, levels: &HashMap<String, usize>) -> String {
    let mut out = String::new();
    let mut rest = xml;
    // Where the paragraph being read starts, and its heading's level.
    let mut para_at = 0usize;
    let mut level: Option<usize> = None;
    while let Some(lt) = rest.find('<') {
        let Some(gt) = rest[lt..].find('>').map(|g| lt + g) else { break };
        let tag = &rest[lt + 1..gt];
        let name = tag.trim_start_matches('/').split([' ', '/', '\t', '\n', '\r']).next().unwrap_or("");
        let closing = tag.starts_with('/');
        let empty = tag.ends_with('/');
        rest = &rest[gt + 1..];
        match name {
            "w:t" if !closing && !empty => {
                let end = rest.find("</w:t>").unwrap_or(rest.len());
                out.push_str(&unescape(&rest[..end]));
                rest = &rest[end..];
            }
            "w:tab" => out.push('\t'),
            "w:br" | "w:cr" => out.push('\n'),
            // (A paragraph's end, or an empty one, `<w:p/>`: a blank line in Word.)
            "w:p" if closing || empty => {
                if let Some(l) = level.take() {
                    if out.len() > para_at && !out[para_at..].trim().is_empty() {
                        out.insert_str(para_at, &format!("{} ", "#".repeat(l.min(6))));
                    }
                }
                out.push('\n');
            }
            "w:p" => {
                para_at = out.len();
                level = None;
            }
            "w:pStyle" => level = attr(tag, "w:val").and_then(|v| level_of(v, levels)).or(level),
            "w:outlineLvl" => {
                if let Some(v) = attr(tag, "w:val").and_then(|v| v.parse::<usize>().ok()).filter(|v| *v < 9) {
                    level = Some(v + 1);
                }
            }
            _ => {}
        }
    }
    let lines: Vec<&str> = out.split('\n').map(str::trim_end).collect();
    let mut tidy: Vec<&str> = Vec::new();
    for line in lines {
        if line.is_empty() && tidy.last().is_none_or(|l| l.is_empty()) {
            continue;
        }
        tidy.push(line);
    }
    while tidy.last().is_some_and(|l| l.is_empty()) {
        tidy.pop();
    }
    tidy.join("\n")
}

/// XML's five entities and character references decoded. Pure.
pub(crate) fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let Some(semi) = after.find(';').filter(|&i| i <= 10) else {
            out.push('&');
            rest = after;
            continue;
        };
        let entity = &after[..semi];
        let ch = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &after[semi + 1..];
            }
            None => {
                out.push('&');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &str = r#"<?xml version="1.0"?><w:document><w:body><w:p><w:r><w:t>Garden plan</w:t></w:r></w:p><w:p/><w:p><w:r><w:t xml:space="preserve">Beds </w:t></w:r><w:r><w:t>&amp; borders</w:t><w:tab/><w:t>&lt;north&gt;</w:t></w:r></w:p><w:p><w:r><w:t>Line one</w:t><w:br/><w:t>Line &#x32; &#51;</w:t></w:r></w:p></w:body></w:document>"#;

    #[test]
    fn paragraphs_tabs_and_breaks_come_through() {
        assert_eq!(of_xml(BODY), "Garden plan\n\nBeds & borders\t<north>\nLine one\nLine 2 3");
    }

    #[test]
    fn a_docx_made_here_reads_back_and_other_zips_dont() {
        let docx = super::super::tests::zip_of(&[("word/document.xml", BODY)]);
        assert_eq!(text(&docx).as_deref(), Some("Garden plan\n\nBeds & borders\t<north>\nLine one\nLine 2 3"));
        let other = super::super::tests::zip_of(&[("readme.txt", "hello")]);
        assert_eq!(text(&other), None);
        assert_eq!(text(b"not a zip"), None);
    }

    #[test]
    fn headings_are_marked_by_their_styles() {
        let body = r#"<w:document><w:body><w:p><w:pPr><w:pStyle w:val="Title"/></w:pPr><w:r><w:t>Leave policy</w:t></w:r></w:p><w:p><w:r><w:t>For all staff.</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t>Annual leave</w:t></w:r></w:p><w:p><w:r><w:t>27 days.</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="berschrift3"/></w:pPr><w:r><w:t>Carry over</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr></w:p><w:p><w:pPr><w:outlineLvl w:val="0"/></w:pPr><w:r><w:t>Pay</w:t></w:r></w:p></w:body></w:document>"#;
        // A German Word's heading 3 is known by its name in styles.xml.
        let styles = r#"<w:styles><w:style w:type="paragraph" w:styleId="berschrift3"><w:name w:val="heading 3"/><w:pPr><w:outlineLvl w:val="2"/></w:pPr></w:style><w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/></w:style></w:styles>"#;
        let levels = heading_styles(styles);
        assert_eq!(levels.get("berschrift3"), Some(&3));
        assert_eq!(levels.get("Quote"), None);
        assert_eq!(of_xml_with(body, &levels), "# Leave policy\nFor all staff.\n## Annual leave\n27 days.\n### Carry over\n\n# Pay");
        // Without styles.xml: Word's English ids still mark them.
        assert!(of_xml(body).starts_with("# Leave policy\nFor all staff.\n## Annual leave\n27 days.\nCarry over\n"));
        let docx = super::super::tests::zip_of(&[("word/document.xml", body), ("word/styles.xml", styles)]);
        assert!(text(&docx).unwrap().contains("### Carry over"));
    }

    #[test]
    fn a_lone_ampersand_stays() {
        assert_eq!(unescape("Fish & chips &unknown; &#zz;"), "Fish & chips &unknown; &#zz;");
    }
}

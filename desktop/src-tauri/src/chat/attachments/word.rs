//! **A Word document's text** (`.docx`), read on this PC: the file is a zip
//! whose `word/document.xml` holds the paragraphs. Their words in order — a
//! paragraph a line, tabs and line breaks kept — and nothing else (no
//! styles, comments or pictures). An old `.doc` isn't this format.

use std::io::Read;

/// The most of `document.xml` read (a zip bomb's room for mischief).
const MOST: u64 = 64 * 1024 * 1024;

/// The text of a `.docx`'s body; `None` when it isn't one. Reads `bytes`.
pub fn text(bytes: &[u8]) -> Option<String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
    let entry = zip.by_name("word/document.xml").ok()?;
    let mut xml = String::new();
    entry.take(MOST).read_to_string(&mut xml).ok()?;
    Some(of_xml(&xml))
}

/// The words of `document.xml`: each `<w:t>`'s text, a line at each
/// paragraph's end, `<w:tab/>` a tab, `<w:br/>` / `<w:cr/>` a line;
/// entities decoded. Pure.
pub fn of_xml(xml: &str) -> String {
    let mut out = String::new();
    let mut rest = xml;
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
            "w:p" if closing || empty => out.push('\n'),
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
fn unescape(s: &str) -> String {
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
    fn a_lone_ampersand_stays() {
        assert_eq!(unescape("Fish & chips &unknown; &#zz;"), "Fish & chips &unknown; &#zz;");
    }
}

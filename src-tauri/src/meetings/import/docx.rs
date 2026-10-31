//! Word documents: **Teams' transcript download** (`.docx`). Read as what
//! it is — a zip of XML — with the `zip` crate and a few lines of tag
//! scanning, never Word or Office automation: `word/document.xml`'s
//! paragraphs (`<w:p>`, their `<w:t>` runs, `<w:tab/>`, `<w:br/>`) become
//! lines for the plain-text reader ("Priya Shah   0:03" blocks under the
//! meeting's title, date and length), or the cue reader for the older
//! layout ("0:0:0.0 --> 0:0:4.60", the name, the words).
//! `docProps/core.xml`'s `dcterms:created` is the document's own date, a
//! last resort for when the meeting was. Pure.

use std::io::{Cursor, Read};

use super::parse::{entities, Format, Parsed};

/// The most document XML read (a ten-hour transcript's is ~20 MB).
const MAX_XML: u64 = 80 * 1024 * 1024;

/// Read a .docx's `bytes` as a transcript.
pub fn parse(bytes: &[u8]) -> Result<Parsed, String> {
    // A password-protected Office file (and an old .doc) is an OLE file, not a zip.
    if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0]) {
        return Err(
            "This document is password-protected or an older .doc. Open it in Word, save it as .docx without a password, and import that."
                .to_string(),
        );
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| "This .docx can't be opened: it may be damaged, or not a Word document.".to_string())?;
    let xml = entry(&mut zip, "word/document.xml", MAX_XML)?
        .ok_or_else(|| "This file isn't a Word document Yap can read: it has no document text.".to_string())?;
    let made = entry(&mut zip, "docProps/core.xml", 1024 * 1024).ok().flatten().and_then(|x| created(&x));
    let lines: Vec<String> = paragraphs(&xml).iter().flat_map(|p| p.split('\n').map(str::to_string).collect::<Vec<_>>()).collect();
    let mut p = if super::cues::has_timings(&lines) {
        // The older Teams layout: cues, under a header.
        let first = lines.iter().position(|l| super::cues::timing(l.trim()).is_some()).unwrap_or(0);
        let mut p = super::cues::parse(&lines[first..], false);
        let head = super::text::parse(&lines[..first], "docx");
        p.title = head.title;
        p.dated = head.dated;
        p.attendees = head.attendees;
        p.format = Format::TeamsDocx;
        p
    } else {
        super::text::parse(&lines, "docx")
    };
    if p.format == Format::Text {
        p.format = Format::Docx;
    }
    p.made = made;
    Ok(p)
}

/// The text of one zip entry, if it's there (and not over `max` bytes).
fn entry(zip: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str, max: u64) -> Result<Option<String>, String> {
    let mut file = match zip.by_name(name) {
        Ok(f) => f,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(_) => return Err("This .docx can't be opened: it may be damaged or password-protected.".to_string()),
    };
    if file.size() > max {
        return Err("This document is too big to be one meeting's transcript.".to_string());
    }
    let mut buf = Vec::with_capacity(file.size() as usize);
    file.by_ref()
        .take(max)
        .read_to_end(&mut buf)
        .map_err(|_| "This .docx can't be read: it may be damaged.".to_string())?;
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
}

/// A WordprocessingML document's paragraphs, as text: each `<w:p>` one
/// string, its `<w:t>` runs joined, a tab for `<w:tab/>`, a new line for
/// `<w:br/>` / `<w:cr/>`. Deleted text (`w:delText`) and field codes
/// (`w:instrText`) aren't text.
pub fn paragraphs(xml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_text = false;
    // A paragraph's tab stops (`<w:tabs><w:tab …/></w:tabs>`) aren't tabs.
    let mut in_stops = false;
    let mut i = 0;
    while i < xml.len() {
        let rest = &xml[i..];
        if rest.starts_with('<') {
            let Some(end) = rest.find('>') else { break };
            let tag = &rest[1..end];
            let closing = tag.starts_with('/');
            let empty = tag.ends_with('/');
            let name = tag.trim_start_matches('/').split([' ', '/', '\t', '\n', '\r']).next().unwrap_or("");
            match name {
                "w:t" => in_text = !closing && !empty,
                "w:tabs" => in_stops = !closing && !empty,
                "w:tab" | "w:ptab" if !closing && !in_stops => cur.push('\t'),
                "w:br" | "w:cr" if !closing => cur.push('\n'),
                "w:p" if closing || empty => out.push(std::mem::take(&mut cur)),
                _ => {}
            }
            i += end + 1;
        } else {
            let next = rest.find('<').unwrap_or(rest.len());
            if in_text {
                cur.push_str(&entities(&rest[..next]));
            }
            i += next;
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// `<dcterms:created …>2026-10-05T14:45:12Z</dcterms:created>` as unix seconds.
fn created(core: &str) -> Option<i64> {
    let start = core.find("<dcterms:created")?;
    let open = start + core[start..].find('>')? + 1;
    let close = open + core[open..].find('<')?;
    chrono::DateTime::parse_from_rfc3339(core[open..close].trim()).ok().map(|d| d.timestamp())
}

#[cfg(test)]
pub mod tests {
    use super::super::parse::parse as read;
    use super::*;
    use std::io::Write;

    /// A .docx holding `paragraphs` (each may have "\t" tabs) and, when
    /// given, its creation date: what Word's own files hold, cut down.
    pub fn docx(paragraphs: &[&str], created: Option<&str>) -> Vec<u8> {
        let body: String = paragraphs
            .iter()
            .map(|p| {
                let runs: Vec<String> = p
                    .split('\t')
                    .map(|t| format!("<w:r><w:rPr><w:b/></w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r>", t.replace('&', "&amp;").replace('<', "&lt;")))
                    .collect();
                format!("<w:p><w:pPr><w:pStyle w:val=\"Normal\"/></w:pPr>{}</w:p>", runs.join("<w:r><w:tab/></w:r>"))
            })
            .collect();
        let doc = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body>{body}<w:sectPr/></w:body></w:document>"
        );
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        z.start_file("[Content_Types].xml", opts).unwrap();
        z.write_all(b"<?xml version=\"1.0\"?><Types/>").unwrap();
        z.start_file("word/document.xml", opts).unwrap();
        z.write_all(doc.as_bytes()).unwrap();
        if let Some(c) = created {
            z.start_file("docProps/core.xml", opts).unwrap();
            z.write_all(format!("<cp:coreProperties><dcterms:created xsi:type=\"dcterms:W3CDTF\">{c}</dcterms:created></cp:coreProperties>").as_bytes()).unwrap();
        }
        z.finish().unwrap().into_inner()
    }

    #[test]
    fn teams_word_transcript() {
        let bytes = docx(
            &[
                "Weekly sync-20261005_143012-Meeting Recording",
                "October 5, 2026, 2:30PM",
                "32m 15s",
                "",
                "Priya Shah started transcription",
                "Priya Shah   0:03",
                "Good morning, everyone & welcome.",
                "Shah, Tom   0:07",
                "Morning.",
                "Priya Shah   1:00:05",
                "An hour in.",
            ],
            Some("2026-10-05T15:05:00Z"),
        );
        let p = read("Weekly sync.docx", &bytes).unwrap();
        assert_eq!(p.format, Format::TeamsDocx);
        assert_eq!(p.title, "Weekly sync");
        let d = p.dated.unwrap();
        assert_eq!(d.date.to_string(), "2026-10-05");
        assert_eq!(d.time.map(|t| t.to_string()).as_deref(), Some("14:30:00"));
        assert_eq!(p.made, Some(1_791_212_700));
        let lines: Vec<_> = p.lines.iter().map(|l| (l.speaker.as_str(), l.start_ms, l.text.as_str())).collect();
        assert_eq!(
            lines,
            [("Priya Shah", 3_000, "Good morning, everyone & welcome."), ("Tom Shah", 7_000, "Morning."), ("Priya Shah", 3_605_000, "An hour in.")]
        );
    }

    #[test]
    fn the_older_teams_layout_in_a_document() {
        let bytes = docx(&["Design review", "0:0:0.0 --> 0:0:4.60", "Shah, Priya", "Good morning.", "0:0:4.60 --> 0:0:7.120", "Baker, Tom", "Morning."], None);
        let p = read("transcript.docx", &bytes).unwrap();
        assert_eq!(p.format, Format::TeamsDocx);
        assert_eq!(p.title, "Design review");
        let who: Vec<_> = p.lines.iter().map(|l| l.speaker.as_str()).collect();
        assert_eq!(who, ["Priya Shah", "Tom Baker"]);
    }

    #[test]
    fn runs_tabs_breaks_and_what_isnt_text() {
        let xml = "<w:body><w:p><w:pPr><w:tabs><w:tab w:val=\"left\" w:pos=\"720\"/></w:tabs></w:pPr><w:r><w:t>Priya</w:t></w:r><w:r><w:t xml:space=\"preserve\"> Shah</w:t></w:r><w:r><w:tab/><w:t>0:03</w:t></w:r></w:p><w:p/><w:p><w:r><w:instrText>PAGE</w:instrText><w:delText>gone</w:delText><w:t>Hi</w:t><w:br/><w:t>there &amp; you</w:t></w:r></w:p></w:body>";
        assert_eq!(paragraphs(xml), ["Priya Shah\t0:03", "", "Hi\nthere & you"]);
    }

    #[test]
    fn documents_it_cant_read() {
        let e = read("locked.docx", &[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]).unwrap_err();
        assert!(e.contains("password"), "{e}");
        let e = read("broken.docx", b"PK\x03\x04 not really a zip").unwrap_err();
        assert!(e.contains("can't be opened"), "{e}");
        let e = read("no-one.docx", &docx(&["Just a letter.", "Kind regards."], None)).unwrap_err();
        assert!(e.contains("no one speaks"), "{e}");
    }
}

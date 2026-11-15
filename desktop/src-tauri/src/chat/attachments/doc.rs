//! **An older Word document's text** (`.doc`, Word 97 to 2003; 16 Nov
//! 2026), read on this PC. The file is a Compound File Binary container
//! (the `cfb` crate, MIT: already in the tree), its text in the
//! `WordDocument` stream, found through the piece table in its table stream
//! as Microsoft's public [MS-DOC] spec describes it (§2.4.1, "Retrieving
//! Text"):
//!
//! - The FIB at the stream's start: its magic (`0xA5EC`), whether it's
//!   encrypted (said as a password), which table stream (`0Table` /
//!   `1Table`), the main text's length (`ccpText`) and where the piece
//!   table is (`fcClx`/`lcbClx`).
//! - The piece table (`PlcPcd`): each piece's characters, 8-bit (Windows'
//!   Latin 1, cp1252) or UTF-16, from its place in `WordDocument`.
//! - The main text only (no headers, footnotes or comments), its marks made
//!   plain: a paragraph a line, a table's cells by tabs and its rows a line,
//!   a field's code dropped and its result kept (a hyperlink's words, a
//!   page number), pictures' and notes' anchors dropped.
//!
//! Everything is read through bounds-checked slices (no `unsafe`), each
//! stream at most [`MOST`]; the caller runs it in its own thread, so a
//! panic in the container's reader is "couldn't read", never a crash.
//! Word 95 and older (another layout) aren't read. A `.doc` that's really
//! another kind (a `.docx`, RTF or HTML saved with that ending, as some
//! apps do) is read as what it is (`attachments::read`).

use std::io::Read;

/// The most of a stream read.
const MOST: u64 = 64 * 1024 * 1024;

/// Why a `.doc`'s text couldn't be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// Not a Compound File (another kind of file with this ending).
    NotOle,
    /// It's password-protected.
    Locked,
    /// Word 95 or older, or the file is broken.
    Unreadable,
}

/// The magic number of a Compound File's header.
pub const OLE_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// **The text of a `.doc`**. Reads `bytes`.
pub fn text(bytes: &[u8]) -> Result<String, Why> {
    if !bytes.starts_with(&OLE_MAGIC) {
        return Err(Why::NotOle);
    }
    let mut file = cfb::CompoundFile::open(std::io::Cursor::new(bytes)).map_err(|_| Why::Unreadable)?;
    let word = stream(&mut file, "/WordDocument").ok_or(Why::Unreadable)?;
    let fib = Fib::of(&word)?;
    if fib.encrypted {
        return Err(Why::Locked);
    }
    let table = stream(&mut file, if fib.table_one { "/1Table" } else { "/0Table" }).ok_or(Why::Unreadable)?;
    let clx = table.get(fib.fc_clx as usize..(fib.fc_clx as usize).saturating_add(fib.lcb_clx as usize)).ok_or(Why::Unreadable)?;
    let pieces = pieces(clx).ok_or(Why::Unreadable)?;
    let raw = characters(&word, &pieces, fib.ccp_text);
    Ok(plain(&raw))
}

/// A stream of the container, at most [`MOST`] of it.
fn stream<F: std::io::Read + std::io::Seek>(file: &mut cfb::CompoundFile<F>, path: &str) -> Option<Vec<u8>> {
    let s = file.open_stream(path).ok()?;
    let mut out = Vec::new();
    s.take(MOST).read_to_end(&mut out).ok()?;
    Some(out)
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// What's needed of the File Information Block.
#[derive(Debug, PartialEq, Eq)]
struct Fib {
    encrypted: bool,
    table_one: bool,
    ccp_text: u32,
    fc_clx: u32,
    lcb_clx: u32,
}

impl Fib {
    /// The FIB at the start of `WordDocument` ([MS-DOC] §2.5.1). Pure.
    fn of(word: &[u8]) -> Result<Fib, Why> {
        let bad = Why::Unreadable;
        if u16_at(word, 0) != Some(0xA5EC) {
            return Err(bad);
        }
        // (Word 97's nFib is 0x00C1 and later ones keep it here; Word 95's
        // is 0x0065–0x0068, another layout.)
        if u16_at(word, 2).ok_or(bad)? < 0x00C1 {
            return Err(bad);
        }
        let flags = u16_at(word, 0x0A).ok_or(bad)?;
        // FibRgW97 (csw 16-bit values), FibRgLw97 (cslw 32-bit), then the
        // fc/lcb pairs: fcClx is the 34th.
        let csw = u16_at(word, 0x20).ok_or(bad)? as usize;
        let lw = 0x22 + csw * 2;
        let cslw = u16_at(word, lw).ok_or(bad)? as usize;
        let ccp_text = u32_at(word, lw + 2 + 3 * 4).ok_or(bad)?;
        let fc_lcb = lw + 2 + cslw * 4;
        let pairs = u16_at(word, fc_lcb).ok_or(bad)? as usize;
        if pairs < 34 {
            return Err(bad);
        }
        let clx = fc_lcb + 2 + 33 * 8;
        Ok(Fib {
            encrypted: flags & 0x0100 != 0,
            table_one: flags & 0x0200 != 0,
            ccp_text,
            fc_clx: u32_at(word, clx).ok_or(bad)?,
            lcb_clx: u32_at(word, clx + 4).ok_or(bad)?,
        })
    }
}

/// A piece of the text: its characters' range, where they start in
/// `WordDocument`, and whether they're 8-bit.
#[derive(Debug, PartialEq, Eq)]
struct Piece {
    cp_start: u32,
    cp_end: u32,
    fc: u32,
    compressed: bool,
}

/// The pieces of the Clx ([MS-DOC] §2.9.38): its formatting runs
/// (`Prc`, 0x01) skipped, then the piece table (`Pcdt`, 0x02). Pure.
fn pieces(clx: &[u8]) -> Option<Vec<Piece>> {
    let mut at = 0;
    while clx.get(at) == Some(&0x01) {
        let cb = u16_at(clx, at + 1)? as i16;
        at += 3 + usize::try_from(cb).ok()?;
    }
    if clx.get(at) != Some(&0x02) {
        return None;
    }
    let lcb = u32_at(clx, at + 1)? as usize;
    let plc = clx.get(at + 5..at + 5 + lcb)?;
    // n + 1 character positions, then n 8-byte piece descriptors.
    if plc.len() < 4 || (plc.len() - 4) % 12 != 0 {
        return None;
    }
    let n = (plc.len() - 4) / 12;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let (cp_start, cp_end) = (u32_at(plc, i * 4)?, u32_at(plc, (i + 1) * 4)?);
        let fc = u32_at(plc, (n + 1) * 4 + i * 8 + 2)?;
        out.push(Piece { cp_start, cp_end, fc: fc & 0x3FFF_FFFF, compressed: fc & 0x4000_0000 != 0 });
    }
    Some(out)
}

/// The characters of the main text (`[0, ccp_text)`), piece by piece; a
/// piece pointing past the stream's end gives what's there. Pure.
fn characters(word: &[u8], pieces: &[Piece], ccp_text: u32) -> String {
    let mut out = String::new();
    for p in pieces {
        if p.cp_start >= ccp_text || p.cp_end <= p.cp_start {
            continue;
        }
        let count = (p.cp_end.min(ccp_text) - p.cp_start) as usize;
        if p.compressed {
            let start = (p.fc / 2) as usize;
            let bytes = word.get(start..).unwrap_or(&[]);
            out.extend(bytes.iter().take(count).map(|&b| cp1252(b)));
        } else {
            let start = p.fc as usize;
            let bytes = word.get(start..).unwrap_or(&[]);
            let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().take(count).map(|c| u16::from_le_bytes(*c)).collect();
            out.push_str(&String::from_utf16_lossy(&units));
        }
    }
    out
}

/// An 8-bit piece's byte as Windows' Latin 1 (cp1252) has it. Pure.
fn cp1252(b: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž', '\u{8F}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}', 'ž', 'Ÿ',
    ];
    if (0x80..0xA0).contains(&b) {
        HIGH[(b - 0x80) as usize]
    } else {
        b as char
    }
}

/// **Word's marks made plain**: `\r` a paragraph's end, `\x0B` a line break
/// and `\x0C` a page break each a line; `\x07` a table cell's end a tab
/// (two together, a row's end: a line); a field's code (`\x13` … `\x14`)
/// dropped and its result (… `\x15`) kept; anchors and the other marks
/// dropped; blank runs folded. Pure.
pub fn plain(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    // Each open field: whether its code is still being read.
    let mut fields: Vec<bool> = Vec::new();
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{13}' => {
                fields.push(true);
                continue;
            }
            '\u{14}' => {
                if let Some(code) = fields.last_mut() {
                    *code = false;
                }
                continue;
            }
            '\u{15}' => {
                fields.pop();
                continue;
            }
            _ => {}
        }
        if fields.iter().any(|&code| code) {
            continue;
        }
        match c {
            '\r' | '\u{0B}' | '\u{0C}' => out.push('\n'),
            '\u{07}' if chars.peek() == Some(&'\u{07}') => {
                chars.next();
                out.push('\n');
            }
            '\u{07}' | '\t' => out.push('\t'),
            '\u{1E}' => out.push('-'),
            '\u{A0}' => out.push(' '),
            c if (c as u32) < 0x20 || c == '\u{1F}' => {}
            c => out.push(c),
        }
    }
    let mut tidy: Vec<&str> = Vec::new();
    for line in out.split('\n').map(str::trim_end) {
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

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A `WordDocument` stream's FIB, its text at 0x800 and a table stream's
    /// piece table, made here by the spec: `pieces` (8-bit or UTF-16,
    /// text) laid one after another.
    fn streams(pieces: &[(bool, &str)], encrypted: bool) -> (Vec<u8>, Vec<u8>) {
        let mut word = vec![0u8; 0x800];
        word[0..2].copy_from_slice(&0xA5ECu16.to_le_bytes());
        word[2..4].copy_from_slice(&0x00C1u16.to_le_bytes());
        let flags: u16 = 0x0200 | if encrypted { 0x0100 } else { 0 };
        word[0x0A..0x0C].copy_from_slice(&flags.to_le_bytes());
        word[0x20..0x22].copy_from_slice(&14u16.to_le_bytes());
        let lw = 0x22 + 28;
        word[lw..lw + 2].copy_from_slice(&22u16.to_le_bytes());
        let fc_lcb = lw + 2 + 88;
        word[fc_lcb..fc_lcb + 2].copy_from_slice(&93u16.to_le_bytes());
        // The text, piece by piece; the CPs and descriptors.
        let mut cps = vec![0u32];
        let mut pcds = Vec::new();
        for (eight_bit, text) in pieces {
            let at = word.len() as u32;
            let n = if *eight_bit {
                word.extend(text.chars().map(|c| c as u8));
                pcds.push((at * 2) | 0x4000_0000);
                text.chars().count()
            } else {
                word.extend(text.encode_utf16().flat_map(|u| u.to_le_bytes()));
                pcds.push(at);
                text.encode_utf16().count()
            };
            cps.push(cps.last().unwrap() + n as u32);
        }
        let ccp = *cps.last().unwrap();
        word[lw + 2 + 12..lw + 2 + 16].copy_from_slice(&ccp.to_le_bytes());
        // The table stream: a Prc to skip, then the Pcdt.
        let mut plc = Vec::new();
        for cp in &cps {
            plc.extend(cp.to_le_bytes());
        }
        for fc in &pcds {
            plc.extend([0u8, 0]);
            plc.extend(fc.to_le_bytes());
            plc.extend([0u8, 0]);
        }
        let mut table = vec![0xAAu8; 16];
        let fc_clx = table.len() as u32;
        table.extend([0x01, 2, 0, 0x55, 0x55]);
        table.push(0x02);
        table.extend((plc.len() as u32).to_le_bytes());
        table.extend(&plc);
        let lcb_clx = table.len() as u32 - fc_clx;
        let clx = fc_lcb + 2 + 33 * 8;
        word[clx..clx + 4].copy_from_slice(&fc_clx.to_le_bytes());
        word[clx + 4..clx + 8].copy_from_slice(&lcb_clx.to_le_bytes());
        (word, table)
    }

    /// A `.doc` made here: the streams in a Compound File.
    pub(crate) fn doc_of(pieces: &[(bool, &str)], encrypted: bool) -> Vec<u8> {
        let (word, table) = streams(pieces, encrypted);
        let mut file = cfb::CompoundFile::create(std::io::Cursor::new(Vec::new())).unwrap();
        std::io::Write::write_all(&mut file.create_stream("/WordDocument").unwrap(), &word).unwrap();
        std::io::Write::write_all(&mut file.create_stream("/1Table").unwrap(), &table).unwrap();
        file.flush().unwrap();
        file.into_inner().into_inner()
    }

    /// The fictional text of the `.doc` made here: an 8-bit piece, then a
    /// UTF-16 one with a field, a table and a picture's anchor.
    pub(crate) fn garden_doc() -> Vec<u8> {
        doc_of(
            &[
                (true, "Allotment rules\r\rKeep paths clear \u{96} always.\r"),
                (false, "See \u{13} HYPERLINK \"https://example.com\" \u{14}the map\u{15} by the gate.\u{1}\rPlot\u{7}Holder\u{7}\u{7}7\u{7}Sam Example\u{7}\u{7}Café open on Sundays\u{0C}"),
            ],
            false,
        )
    }

    #[test]
    fn the_text_comes_through_its_pieces_with_its_marks_made_plain() {
        assert_eq!(text(&garden_doc()).unwrap(), "Allotment rules\n\nKeep paths clear – always.\nSee the map by the gate.\nPlot\tHolder\n7\tSam Example\nCafé open on Sundays");
    }

    /// A real Word 97 file, as another app saves one: LibreOffice's, from a
    /// page of fictional rules (`fixtures/allotment-rules.doc`; the e2e
    /// suite attaches it too).
    #[test]
    fn a_doc_another_app_saved_reads_back() {
        let text = text(include_bytes!("fixtures/allotment-rules.doc")).unwrap();
        assert_eq!(text, "Allotment rules\nKeep the paths clear – always. Water before nine in the morning.\nSee the site map by the gate.\nPlot\tHolder\n7\tSam Example\n12\tAlex Example\nThe café opens on Sundays.");
    }

    #[test]
    fn a_locked_a_broken_and_another_kind_of_file_say_so() {
        assert_eq!(text(&doc_of(&[(true, "Secret plot\r")], true)), Err(Why::Locked));
        assert_eq!(text(b"PK\x03\x04 a docx"), Err(Why::NotOle));
        assert_eq!(text(b"{\\rtf1 hello}"), Err(Why::NotOle));
        let mut broken = garden_doc();
        broken.truncate(600);
        assert_eq!(text(&broken), Err(Why::Unreadable));
        // A container with no WordDocument stream.
        let mut other = cfb::CompoundFile::create(std::io::Cursor::new(Vec::new())).unwrap();
        std::io::Write::write_all(&mut other.create_stream("/Book").unwrap(), b"cells").unwrap();
        other.flush().unwrap();
        assert_eq!(text(&other.into_inner().into_inner()), Err(Why::Unreadable));
    }

    #[test]
    fn a_fib_too_short_or_too_old_is_unreadable() {
        let (mut word, _) = streams(&[(true, "x\r")], false);
        assert!(Fib::of(&word).is_ok());
        word[2..4].copy_from_slice(&0x0065u16.to_le_bytes());
        assert_eq!(Fib::of(&word), Err(Why::Unreadable));
        assert_eq!(Fib::of(&[0xEC, 0xA5]), Err(Why::Unreadable));
        assert_eq!(pieces(&[0x02, 0xFF, 0xFF, 0xFF, 0x7F]), None);
        assert_eq!(pieces(&[0x01, 0xFF, 0xFF]), None);
    }

    #[test]
    fn nested_fields_keep_only_their_results() {
        assert_eq!(plain("Page \u{13}PAGE\u{14}3\u{15} of \u{13}NUMPAGES \u{13}x\u{14}y\u{15}\u{14}9\u{15}\r"), "Page 3 of 9");
        assert_eq!(plain("a\u{1E}b\u{1F}c\u{A0}d"), "a-bc d");
    }
}

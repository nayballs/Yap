//! **A PDF's text, read on this PC** (`pdf-extract`, through lopdf): never
//! uploaded to be read. Page by page, each page marked, so a question about
//! "page 3" can be answered. A parser that panics on a strange file, or takes
//! too long on a huge one, is caught: the file is said to be unreadable,
//! never a crash or a hang.
//!
//! **A page with no text** (a scan, a picture of a letter) is read with
//! Windows' OCR (`scan.rs`, 16 Nov 2026), marked "[Page 3, read with OCR]";
//! at most [`scan::MAX_PAGES`] a file, the rest named in a note at the end
//! ([`note`]). So is a file lopdf can't parse or decrypt that Windows opens
//! (an owner-password-only "copy protected" PDF): every page read with OCR.

use std::sync::Arc;
use std::time::Duration;

use super::scan;

/// Longest a PDF's text is read for.
const READ_FOR: Duration = Duration::from_secs(60);

/// Why a PDF's text couldn't be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// It's password-protected.
    Locked,
    /// Its pages carry no text (scanned pages, pictures of text) and OCR
    /// found no words on them.
    NoText,
    /// Its pages carry no text and Windows has no text recognition for a
    /// language here (or this isn't Windows).
    NoOcr,
    /// Its parser gave up, or took too long.
    Failed,
}

/// A PDF's text as read: its pages, how many were read with OCR, and how
/// many with no text weren't read (past the limit, or too slow).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Text {
    pub text: String,
    pub pages: u32,
    pub ocr: u32,
    pub unread: u32,
}

/// What lopdf made of the file.
enum Extracted {
    Pages(Vec<String>),
    Locked,
    Broken,
    TooSlow,
}

/// **The text of a PDF's pages**, the ones without text read with OCR.
/// Blocking: run it off the async runtime.
pub fn text(bytes: Vec<u8>) -> Result<Text, Why> {
    let bytes = Arc::new(bytes);
    match extract(Arc::clone(&bytes)) {
        Extracted::Pages(pages) => {
            let blank: Vec<u32> = pages.iter().enumerate().filter(|(_, p)| tidy(p).is_empty()).map(|(i, _)| i as u32).collect();
            if blank.is_empty() {
                return joined(&pages, &[], &Left::default());
            }
            match scan::read(bytes, Some(blank.clone())) {
                Ok(s) => {
                    let left = Left { capped: blank.iter().skip(scan::MAX_PAGES).copied().collect(), slow: s.slow(Some(&blank)), ..Left::default() };
                    joined(&pages, &s.read, &left)
                }
                // (Its text pages still go; a scanned page is a line.)
                Err(f) => {
                    let left = Left { capped: blank.clone(), why: Some(f), ..Left::default() };
                    joined(&pages, &[], &left).map_err(|_| no_ocr(f))
                }
            }
        }
        // lopdf gave up: Windows may open it. Every page read with OCR.
        gave_up @ (Extracted::Locked | Extracted::Broken) => {
            let locked = matches!(gave_up, Extracted::Locked);
            match scan::read(bytes, None) {
                Ok(s) => {
                    let count = s.count.unwrap_or(0);
                    let left = Left { capped: (scan::MAX_PAGES as u32..count).collect(), slow: s.slow(None), ..Left::default() };
                    joined(&vec![String::new(); count as usize], &s.read, &left)
                }
                Err(scan::Fail::Unreadable | scan::Fail::Unsupported) if locked => Err(Why::Locked),
                Err(_) => Err(Why::Failed),
            }
        }
        Extracted::TooSlow => Err(Why::Failed),
    }
}

/// Why OCR found nothing to read, said as the file's `Why`.
fn no_ocr(f: scan::Fail) -> Why {
    match f {
        scan::Fail::NoOcr | scan::Fail::Unsupported => Why::NoOcr,
        scan::Fail::Unreadable => Why::NoText,
    }
}

/// lopdf's text of each page, in its own thread: a panic stays there, and a
/// file that takes too long is left to finish on its own while the person
/// hears it couldn't be read.
fn extract(bytes: Arc<Vec<u8>>) -> Extracted {
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("pdf-text".into()).spawn(move || {
        let read = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem_by_pages(&bytes));
        let _ = tx.send(read);
    });
    if spawned.is_err() {
        return Extracted::Broken;
    }
    match rx.recv_timeout(READ_FOR) {
        Ok(Ok(Ok(pages))) => Extracted::Pages(pages),
        Ok(Ok(Err(e))) => {
            let said = e.to_string().to_ascii_lowercase();
            let locked = said.contains("decrypt") || said.contains("password") || said.contains("encrypt");
            tracing::info!(locked, "Chat: a PDF's text couldn't be read");
            if locked {
                Extracted::Locked
            } else {
                Extracted::Broken
            }
        }
        Ok(Err(_)) => {
            tracing::warn!("Chat: the PDF reader gave up on a file");
            Extracted::Broken
        }
        Err(_) => {
            tracing::warn!(secs = READ_FOR.as_secs(), "Chat: a PDF took too long to read");
            Extracted::TooSlow
        }
    }
}

/// The pages with no text that weren't read with OCR: past the limit
/// (`capped`), or when the time ran out (`slow`); `why`: OCR couldn't run.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Left {
    pub capped: Vec<u32>,
    pub slow: Vec<u32>,
    pub why: Option<scan::Fail>,
}

/// **The pages' text as one**: each page after a `[Page n]` line (`[Page n,
/// read with OCR]` for what OCR read of a page with none: `ocr`, by
/// 0-based index), its blank runs folded; the pages left out named in a
/// note at the end ([`note`]). `NoText` when no page has any. Pure.
pub fn joined(pages: &[String], ocr: &[(u32, String)], left: &Left) -> Result<Text, Why> {
    let count = pages.len() as u32;
    let mut out = String::new();
    let mut read_by_ocr = 0;
    for (i, page) in pages.iter().enumerate() {
        let mut tidy = tidy(page);
        let mut mark = format!("[Page {}]", i + 1);
        if tidy.is_empty() {
            if let Some((_, t)) = ocr.iter().find(|(p, _)| *p as usize == i) {
                tidy = self::tidy(t);
                if !tidy.is_empty() {
                    mark = format!("[Page {}, read with OCR]", i + 1);
                    read_by_ocr += 1;
                }
            }
        }
        if tidy.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&format!("{mark}\n{tidy}"));
    }
    if out.is_empty() {
        return Err(Why::NoText);
    }
    let unread = (left.capped.len() + left.slow.len()) as u32;
    if let Some(note) = note(left) {
        out.push_str(&format!("\n\n{note}"));
    }
    Ok(Text { text: out, pages: count, ocr: read_by_ocr, unread })
}

/// **The note on the pages left out**, at the end of the text: which pages
/// and why, as the model reads it. `None` for none. Pure.
pub fn note(left: &Left) -> Option<String> {
    let mut lines = Vec::new();
    if !left.capped.is_empty() {
        let (pages, many) = pages_said(&left.capped);
        let (have, were, what) = if many { ("have", "weren't", "scans or pictures") } else { ("has", "wasn't", "a scan or a picture") };
        lines.push(match left.why {
            Some(scan::Fail::NoOcr) => format!("[{pages} {have} no text layer ({what}) and {were} read: Windows' text recognition isn't set up on this PC.]"),
            Some(_) => format!("[{pages} {have} no text layer ({what}) and couldn't be read.]"),
            None => format!("[{pages} {have} no text layer ({what}) and {were} read: Yap reads at most {} such pages of a file with OCR.]", scan::MAX_PAGES),
        });
    }
    if !left.slow.is_empty() {
        let (pages, many) = pages_said(&left.slow);
        lines.push(format!("[{pages} {} read: reading {} with OCR took too long.]", if many { "weren't" } else { "wasn't" }, if many { "them" } else { "it" }));
    }
    (!lines.is_empty()).then(|| lines.join("\n"))
}

/// Pages (0-based) said as ranges, 1-based ("Page 4", "Pages 3, 5 and
/// 21–48"), and whether they're more than one. Pure.
pub fn pages_said(pages: &[u32]) -> (String, bool) {
    let mut sorted = pages.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for p in sorted.iter().map(|p| p + 1) {
        match runs.last_mut() {
            Some((_, end)) if *end + 1 == p => *end = p,
            _ => runs.push((p, p)),
        }
    }
    let parts: Vec<String> = runs.iter().map(|&(a, b)| if a == b { a.to_string() } else { format!("{a}–{b}") }).collect();
    let list = match parts.len() {
        0 => String::new(),
        1 => parts[0].clone(),
        n => format!("{} and {}", parts[..n - 1].join(", "), parts[n - 1]),
    };
    let many = sorted.len() > 1;
    (format!("{} {list}", if many { "Pages" } else { "Page" }), many)
}

/// A page's text: line ends made `\n`, trailing spaces gone, at most one
/// blank line in a row. Pure.
fn tidy(page: &str) -> String {
    let page = page.replace("\r\n", "\n").replace('\r', "\n");
    let mut out: Vec<&str> = Vec::new();
    for line in page.split('\n').map(str::trim_end) {
        if line.is_empty() && out.last().is_none_or(|l| l.is_empty()) {
            continue;
        }
        out.push(line);
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::attachments::tests::{pdf_of, tiny_pdf, Pg};

    #[test]
    fn pages_are_marked_and_tidied() {
        let pages = vec!["Seed order\r\n\r\n\r\nTomatoes  \n".to_string(), "  \n".to_string(), "Beans\n\nPeas".to_string()];
        let t = joined(&pages, &[], &Left::default()).unwrap();
        assert_eq!((t.pages, t.ocr, t.unread), (3, 0, 0));
        assert_eq!(t.text, "[Page 1]\nSeed order\n\nTomatoes\n\n[Page 3]\nBeans\n\nPeas");
    }

    #[test]
    fn a_pdf_of_pictures_has_no_text() {
        assert_eq!(joined(&["\n \n".to_string(), String::new()], &[], &Left::default()), Err(Why::NoText));
        assert_eq!(joined(&[], &[], &Left::default()), Err(Why::NoText));
        // OCR found nothing on them either.
        assert_eq!(joined(&[String::new()], &[(0, " \n".into())], &Left::default()), Err(Why::NoText));
    }

    #[test]
    fn a_scanned_page_is_marked_read_with_ocr_and_what_was_left_out_is_said() {
        let pages = vec!["Seed order".to_string(), String::new(), String::new(), String::new()];
        let ocr = vec![(1, "Compost on\r\nTuesday  ".to_string()), (2, String::new())];
        let left = Left { capped: vec![3], ..Left::default() };
        let t = joined(&pages, &ocr, &left).unwrap();
        assert_eq!((t.pages, t.ocr, t.unread), (4, 1, 1));
        assert_eq!(t.text, "[Page 1]\nSeed order\n\n[Page 2, read with OCR]\nCompost on\nTuesday\n\n[Page 4 has no text layer (a scan or a picture) and wasn't read: Yap reads at most 20 such pages of a file with OCR.]");
        // A page with text never takes what OCR read.
        let t = joined(&["Beans".to_string()], &[(0, "Other".into())], &Left::default()).unwrap();
        assert_eq!(t.text, "[Page 1]\nBeans");
        assert_eq!(t.ocr, 0);
    }

    #[test]
    fn the_note_names_the_pages_and_why() {
        assert_eq!(pages_said(&[20, 21, 22, 47]), ("Pages 21–23 and 48".to_string(), true));
        assert_eq!(pages_said(&[2, 4, 5, 6, 9, 4]), ("Pages 3, 5–7 and 10".to_string(), true));
        assert_eq!(pages_said(&[0]), ("Page 1".to_string(), false));
        assert_eq!(note(&Left::default()), None);
        let capped = Left { capped: (20..48).collect(), ..Left::default() };
        assert_eq!(note(&capped).unwrap(), "[Pages 21–48 have no text layer (scans or pictures) and weren't read: Yap reads at most 20 such pages of a file with OCR.]");
        let slow = Left { slow: vec![14, 15, 16], ..Left::default() };
        assert_eq!(note(&slow).unwrap(), "[Pages 15–17 weren't read: reading them with OCR took too long.]");
        let one = Left { slow: vec![4], ..Left::default() };
        assert_eq!(note(&one).unwrap(), "[Page 5 wasn't read: reading it with OCR took too long.]");
        let no_ocr = Left { capped: vec![1], why: Some(scan::Fail::NoOcr), ..Left::default() };
        assert_eq!(note(&no_ocr).unwrap(), "[Page 2 has no text layer (a scan or a picture) and wasn't read: Windows' text recognition isn't set up on this PC.]");
    }

    #[test]
    fn what_isnt_a_pdf_fails_and_never_panics() {
        assert_eq!(text(b"not a pdf at all".to_vec()), Err(Why::Failed));
        assert_eq!(text(b"%PDF-1.4\n%%EOF".to_vec()).map(|_| ()).unwrap_err(), Why::Failed);
    }

    #[test]
    fn a_small_pdf_made_here_reads_back() {
        let t = text(tiny_pdf("Plant the bulbs along the fence.")).unwrap();
        assert_eq!((t.pages, t.ocr, t.unread), (1, 0, 0));
        assert!(t.text.starts_with("[Page 1]\n"), "{}", t.text);
        assert!(t.text.contains("Plant the bulbs along the fence."), "{}", t.text);
    }

    /// A PDF made here with a typed page and a scanned one: the typed page's
    /// text, and Windows' OCR of the scan. Passes (and says so) on a PC
    /// with no OCR language: the typed page alone, the scan named.
    #[cfg(windows)]
    #[test]
    fn a_scanned_page_beside_a_typed_one_is_read_with_ocr() {
        let t = text(pdf_of(&[Pg::Text("Seed order for the spring"), Pg::Scan])).unwrap();
        assert_eq!(t.pages, 2);
        assert!(t.text.starts_with("[Page 1]\nSeed order for the spring"), "{}", t.text);
        if t.ocr == 0 {
            assert!(t.text.contains("[Page 2 has no text layer"), "{}", t.text);
            eprintln!("no OCR language on this PC: skipped");
            return;
        }
        assert!(t.text.contains("\n\n[Page 2, read with OCR]\n"), "{}", t.text);
        let flat = t.text.to_lowercase();
        assert!(flat.contains("compost the beds on tuesday") && flat.contains("mulch"), "{}", t.text);
        assert_eq!(t.unread, 0);
    }

    /// More scanned pages than OCR reads: the first 20, the rest named.
    #[cfg(windows)]
    #[test]
    fn past_twenty_scanned_pages_the_rest_are_named() {
        let pages: Vec<Pg> = (0..22).map(|_| Pg::Scan).collect();
        match text(pdf_of(&pages)) {
            Ok(t) => {
                assert_eq!((t.pages, t.ocr, t.unread), (22, 20, 2), "{}", t.text);
                assert!(t.text.contains("[Page 20, read with OCR]"));
                assert!(!t.text.contains("[Page 21,"));
                assert!(t.text.ends_with("[Pages 21–22 have no text layer (scans or pictures) and weren't read: Yap reads at most 20 such pages of a file with OCR.]"), "{}", t.text);
            }
            Err(Why::NoOcr) => eprintln!("no OCR language on this PC: skipped"),
            Err(e) => panic!("{e:?}"),
        }
    }
}

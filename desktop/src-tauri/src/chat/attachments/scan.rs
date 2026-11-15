//! **A scanned PDF's pages, read on this PC** (16 Nov 2026): a page with no
//! text in it (a scan, a photographed letter) is drawn by Windows' own PDF
//! renderer (`Windows.Data.Pdf`, in every Windows 10 and 11) and read by
//! Windows' OCR, the screenshots' "Send its text instead" (`snip/ocr.rs`:
//! the person's own languages; nothing uploaded to read it).
//!
//! - **At most [`MAX_PAGES`] pages a file, within [`READ_FOR`]**: `pdf.rs`
//!   names the pages left out in a note at the end of the text, never
//!   silently.
//! - **Its own thread**, as the PDF's text is read: a page Windows takes too
//!   long over is let go at the limit (the thread stops at its next page),
//!   the pages read by then kept; a panic stays there.
//! - **COM**: the thread borrows the process's multithreaded apartment,
//!   which `shell/com.rs` keeps up for as long as Yap runs; it never sets
//!   COM up or takes it down itself (`CoInitializeEx`/`CoUninitialize` would
//!   take the apartment away under other threads' WinRT objects).
//! - Each page drawn at the size the OCR engine reads best
//!   ([`render_size`]: up to 2.5× its size on screen, never past the
//!   engine's `MaxImageDimension`), as a PNG, then read as a screenshot is
//!   (`ocr::read_png`; a test run's fake text there too, `e2e_snip_ocr`).
//! - Elsewhere than Windows: [`Fail::Unsupported`], the file said as before.
//!
//! Never logged: page and character counts only.

use std::sync::mpsc::{RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Most pages of a file read with OCR.
pub const MAX_PAGES: usize = 20;
/// Longest the OCR reads a file for.
pub const READ_FOR: Duration = Duration::from_secs(90);

/// Why no page could be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fail {
    /// Windows has no text recognition for any language on this PC.
    NoOcr,
    /// Windows couldn't open the file (broken, or a password on it).
    Unreadable,
    /// Not Windows: no OCR here.
    #[cfg_attr(windows, allow(dead_code))]
    Unsupported,
}

/// **What was read**: each page's text, by its index (0-based, in order; a
/// page with no words on it, empty), Windows' count of the file's pages,
/// and whether the time ran out before every page asked for was read.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Scanned {
    pub read: Vec<(u32, String)>,
    pub count: Option<u32>,
    pub timed_out: bool,
}

impl Scanned {
    /// The pages of `asked` (`None`: every page) that would have been read
    /// with time enough, and weren't. Pure.
    pub fn slow(&self, asked: Option<&[u32]>) -> Vec<u32> {
        if !self.timed_out {
            return Vec::new();
        }
        planned(asked, self.count).into_iter().filter(|i| !self.read.iter().any(|(r, _)| r == i)).collect()
    }
}

/// The pages the reader goes through: those asked for (`None`: every page
/// of `count`) that the file has, the first [`MAX_PAGES`]. Pure.
pub fn planned(asked: Option<&[u32]>, count: Option<u32>) -> Vec<u32> {
    let all: Vec<u32> = match asked {
        Some(list) => list.to_vec(),
        None => (0..count.unwrap_or(0)).collect(),
    };
    all.into_iter().filter(|&i| count.is_none_or(|c| i < c)).take(MAX_PAGES).collect()
}

/// The size a page of `w`×`h` DIPs is drawn at for the OCR engine (`max`:
/// its largest side): as big as the engine takes, at most 2.5× (Windows'
/// OCR reads text of ~10 px and up best: a 10 pt line is ~13 px at 1×).
/// Pure.
pub fn render_size(w: f32, h: f32, max: u32) -> (u32, u32) {
    let (w, h) = (w.max(1.0) as f64, h.max(1.0) as f64);
    let scale = (max.max(1) as f64 / w.max(h)).min(2.5);
    let side = |v: f64| ((v * scale).round() as u32).clamp(1, max.max(1));
    (side(w), side(h))
}

enum Msg {
    Count(u32),
    Page(u32, String),
    Fail(Fail),
}

/// **Read `pages`** (0-based; `None`: every page) of the PDF in `bytes` with
/// OCR: the first [`MAX_PAGES`] of them, within [`READ_FOR`]. Blocking: run
/// it off the async runtime.
pub fn read(bytes: Arc<Vec<u8>>, pages: Option<Vec<u32>>) -> Result<Scanned, Fail> {
    let started = Instant::now();
    let deadline = started + READ_FOR;
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("pdf-ocr".into()).spawn(move || {
        let ran = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&bytes, pages, deadline, &tx)));
        if ran.is_err() {
            tracing::warn!("Chat: Windows' PDF reader gave up on a scanned file");
            let _ = tx.send(Msg::Fail(Fail::Unreadable));
        }
    });
    if spawned.is_err() {
        return Err(Fail::Unreadable);
    }
    let mut out = Scanned::default();
    loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Msg::Count(n)) => out.count = Some(n),
            Ok(Msg::Page(i, text)) => out.read.push((i, text)),
            Ok(Msg::Fail(f)) if out.read.is_empty() => return Err(f),
            Ok(Msg::Fail(_)) => break,
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {
                out.timed_out = true;
                break;
            }
        }
    }
    if out.count.is_none() && out.read.is_empty() {
        // (Never opened: hung on the file, or gone without a word.)
        return Err(Fail::Unreadable);
    }
    let chars: usize = out.read.iter().map(|(_, t)| t.chars().count()).sum();
    tracing::info!(pages = out.read.len(), chars, timed_out = out.timed_out, ms = started.elapsed().as_millis() as u64, "Chat: a scanned PDF's pages read with OCR");
    Ok(out)
}

/// The reader's thread: the file opened, its page count said, then each
/// page drawn and read, one at a time, until the deadline.
#[cfg(windows)]
fn run(bytes: &[u8], pages: Option<Vec<u32>>, deadline: Instant, tx: &Sender<Msg>) {
    let doc = match open(bytes) {
        Ok(doc) => doc,
        Err(_) => {
            let _ = tx.send(Msg::Fail(Fail::Unreadable));
            return;
        }
    };
    let count = doc.PageCount().unwrap_or(0);
    if tx.send(Msg::Count(count)).is_err() {
        return;
    }
    for i in planned(pages.as_deref(), Some(count)) {
        if Instant::now() >= deadline {
            return;
        }
        let text = match page_png(&doc, i).and_then(|png| crate::snip::ocr::read_png(&png)) {
            Ok(text) => text,
            Err(e) if e == crate::snip::ocr::NO_OCR => {
                let _ = tx.send(Msg::Fail(Fail::NoOcr));
                return;
            }
            // (A page Windows couldn't draw: no words from it.)
            Err(_) => String::new(),
        };
        if tx.send(Msg::Page(i, text)).is_err() {
            return;
        }
    }
}

#[cfg(not(windows))]
fn run(_bytes: &[u8], _pages: Option<Vec<u32>>, _deadline: Instant, tx: &Sender<Msg>) {
    let _ = tx.send(Msg::Fail(Fail::Unsupported));
}

/// The PDF in `bytes`, opened by Windows (an in-memory stream: the file is
/// never written anywhere to be read).
#[cfg(windows)]
fn open(bytes: &[u8]) -> windows::core::Result<windows::Data::Pdf::PdfDocument> {
    use windows::Data::Pdf::PdfDocument;
    use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};
    let stream = InMemoryRandomAccessStream::new()?;
    let writer = DataWriter::CreateDataWriter(&stream)?;
    writer.WriteBytes(bytes)?;
    writer.StoreAsync()?.join()?;
    writer.FlushAsync()?.join()?;
    writer.DetachStream()?;
    stream.Seek(0)?;
    PdfDocument::LoadFromStreamAsync(&stream)?.join()
}

/// Page `i` of `doc` drawn as a PNG, at [`render_size`].
#[cfg(windows)]
fn page_png(doc: &windows::Data::Pdf::PdfDocument, i: u32) -> Result<Vec<u8>, String> {
    use windows::Data::Pdf::PdfPageRenderOptions;
    use windows::Graphics::Imaging::BitmapEncoder;
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};
    let err = |e: windows::core::Error| format!("Windows' PDF reader: {}", e.message());
    let page = doc.GetPage(i).map_err(err)?;
    let size = page.Size().map_err(err)?;
    let (w, h) = render_size(size.Width, size.Height, OcrEngine::MaxImageDimension().unwrap_or(2600));
    let options = PdfPageRenderOptions::new().map_err(err)?;
    options.SetDestinationWidth(w).map_err(err)?;
    options.SetDestinationHeight(h).map_err(err)?;
    options.SetBitmapEncoderId(BitmapEncoder::PngEncoderId().map_err(err)?).map_err(err)?;
    let out = InMemoryRandomAccessStream::new().map_err(err)?;
    page.RenderWithOptionsToStreamAsync(&out, &options).and_then(|op| op.join()).map_err(err)?;
    let _ = page.Close();
    let n = u32::try_from(out.Size().map_err(err)?).map_err(|_| "a page too big to read".to_string())?;
    let reader = DataReader::CreateDataReader(&out.GetInputStreamAt(0).map_err(err)?).map_err(err)?;
    reader.LoadAsync(n).and_then(|op| op.join()).map_err(err)?;
    let mut png = vec![0u8; n as usize];
    reader.ReadBytes(&mut png).map_err(err)?;
    Ok(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_is_drawn_as_big_as_the_engine_reads_best() {
        // A4 (794 × 1123 DIPs): the engine's 2,600 px on its long side.
        assert_eq!(render_size(794.0, 1123.0, 2600), (1838, 2600));
        // A small receipt: 2.5× at most.
        assert_eq!(render_size(200.0, 400.0, 2600), (500, 1000));
        // A poster: smaller, to fit.
        assert_eq!(render_size(5200.0, 2600.0, 2600), (2600, 1300));
        // Nothing odd from nothing.
        assert_eq!(render_size(0.0, 0.0, 0), (1, 1));
    }

    #[test]
    fn the_pages_read_are_the_first_twenty_the_file_has() {
        assert_eq!(planned(Some(&[1, 3, 9]), Some(5)), vec![1, 3]);
        assert_eq!(planned(None, Some(3)), vec![0, 1, 2]);
        assert_eq!(planned(None, Some(48)).len(), MAX_PAGES);
        assert_eq!(planned(None, None), Vec::<u32>::new());
        let many: Vec<u32> = (0..30).collect();
        assert_eq!(planned(Some(&many), None), (0..20).collect::<Vec<u32>>());
        // Too slow: the pages planned and not read.
        let s = Scanned { read: vec![(0, "a".into()), (2, String::new())], count: Some(4), timed_out: true };
        assert_eq!(s.slow(None), vec![1, 3]);
        assert_eq!(s.slow(Some(&[2, 3])), vec![3]);
        assert_eq!(Scanned { timed_out: false, ..s }.slow(None), Vec::<u32>::new());
    }

    #[test]
    fn what_isnt_a_pdf_is_unreadable_and_never_panics() {
        let got = read(Arc::new(b"not a pdf at all".to_vec()), None);
        #[cfg(windows)]
        assert_eq!(got, Err(Fail::Unreadable));
        #[cfg(not(windows))]
        assert_eq!(got, Err(Fail::Unsupported));
    }

    /// Windows draws a scanned page made here (a picture of fictional
    /// words, no text in the file) and its OCR reads it. Needs a language
    /// with text recognition (any English Windows has one); says so and
    /// passes where there's none.
    #[cfg(windows)]
    #[test]
    fn windows_reads_a_scanned_page_made_here() {
        let pdf = super::super::tests::scanned_pdf(2);
        match read(Arc::new(pdf), Some(vec![1])) {
            Ok(s) => {
                assert_eq!(s.count, Some(2));
                assert_eq!(s.read.len(), 1, "{s:?}");
                let (page, text) = &s.read[0];
                assert_eq!(*page, 1);
                let flat = text.to_lowercase();
                assert!(flat.contains("compost") && flat.contains("tuesday"), "{text:?}");
            }
            Err(Fail::NoOcr) => eprintln!("no OCR language on this PC: skipped"),
            Err(e) => panic!("{e:?}"),
        }
    }
}

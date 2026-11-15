//! **A PDF's text, read on this PC** (`pdf-extract`, through lopdf): never
//! uploaded to be read. Page by page, each page marked, so a question about
//! "page 3" can be answered. A parser that panics on a strange file, or takes
//! too long on a huge one, is caught: the file is said to be unreadable,
//! never a crash or a hang.

use std::time::Duration;

/// Longest a PDF is read for.
const READ_FOR: Duration = Duration::from_secs(60);

/// Why a PDF's text couldn't be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// It's password-protected.
    Locked,
    /// Its pages carry no text (scanned pages, pictures of text).
    NoText,
    /// Its parser gave up, or took too long.
    Failed,
}

/// The text of a PDF's pages, with their count. Blocking: run it off the
/// async runtime.
pub fn text(bytes: Vec<u8>) -> Result<(String, u32), Why> {
    // Its own thread: a panic stays there, and a file that takes too long is
    // left to finish on its own while the person hears it couldn't be read.
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("pdf-text".into()).spawn(move || {
        let read = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem_by_pages(&bytes));
        let _ = tx.send(read);
    });
    if spawned.is_err() {
        return Err(Why::Failed);
    }
    match rx.recv_timeout(READ_FOR) {
        Ok(Ok(Ok(pages))) => joined(&pages),
        Ok(Ok(Err(e))) => {
            let said = e.to_string().to_ascii_lowercase();
            tracing::info!(locked = said.contains("decrypt") || said.contains("password") || said.contains("encrypt"), "Chat: a PDF's text couldn't be read");
            Err(if said.contains("decrypt") || said.contains("password") || said.contains("encrypt") { Why::Locked } else { Why::Failed })
        }
        Ok(Err(_)) => {
            tracing::warn!("Chat: the PDF reader gave up on a file");
            Err(Why::Failed)
        }
        Err(_) => {
            tracing::warn!(secs = READ_FOR.as_secs(), "Chat: a PDF took too long to read");
            Err(Why::Failed)
        }
    }
}

/// The pages' text as one: each page after a `[Page n]` line, its blank
/// runs folded; `NoText` when no page has any. Pure.
pub fn joined(pages: &[String]) -> Result<(String, u32), Why> {
    let count = pages.len() as u32;
    let mut out = String::new();
    for (i, page) in pages.iter().enumerate() {
        let tidy = tidy(page);
        if tidy.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&format!("[Page {}]\n{tidy}", i + 1));
    }
    if out.is_empty() {
        Err(Why::NoText)
    } else {
        Ok((out, count))
    }
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

    #[test]
    fn pages_are_marked_and_tidied() {
        let pages = vec!["Seed order\r\n\r\n\r\nTomatoes  \n".to_string(), "  \n".to_string(), "Beans\n\nPeas".to_string()];
        let (text, count) = joined(&pages).unwrap();
        assert_eq!(count, 3);
        assert_eq!(text, "[Page 1]\nSeed order\n\nTomatoes\n\n[Page 3]\nBeans\n\nPeas");
    }

    #[test]
    fn a_pdf_of_pictures_has_no_text() {
        assert_eq!(joined(&["\n \n".to_string(), String::new()]), Err(Why::NoText));
        assert_eq!(joined(&[]), Err(Why::NoText));
    }

    #[test]
    fn what_isnt_a_pdf_fails_and_never_panics() {
        assert_eq!(text(b"not a pdf at all".to_vec()), Err(Why::Failed));
        assert_eq!(text(b"%PDF-1.4\n%%EOF".to_vec()).map(|_| ()).unwrap_err(), Why::Failed);
    }

    #[test]
    fn a_small_pdf_made_here_reads_back() {
        let bytes = super::super::tests::tiny_pdf("Plant the bulbs along the fence.");
        let (text, pages) = text(bytes).unwrap();
        assert_eq!(pages, 1);
        assert!(text.starts_with("[Page 1]\n"), "{text}");
        assert!(text.contains("Plant the bulbs along the fence."), "{text}");
    }
}

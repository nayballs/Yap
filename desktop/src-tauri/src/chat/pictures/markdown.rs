//! **A picture in markdown, fetched when it's asked for** (15 Nov 2026, after
//! the Claude app's "Show image"): an answer's, a note's or a document's
//! `![alt](https://…)` is a box in the page (`ui/markdownVisuals.js`), and
//! nothing is fetched until the person clicks it. Then this downloads it the
//! way Chat's own pictures are (`web::fetch_picture`: through
//! `privacy/net.rs` as "A picture from the web", public http(s) addresses
//! only, an image type, at most 10 MB) and hands its bytes to the page,
//! which shows them from memory. Nothing is kept on disk.
//!
//! Only the kinds a page draws as a picture are handed over, by their first
//! bytes (`kind_of`): PNG, JPEG, GIF, WebP, AVIF, BMP; never SVG.
//!
//! **Privacy**: the log has its size, never its address.

/// A picture's kind by its first bytes, or `None`: not one the page is
/// given. Pure.
pub fn kind_of(bytes: &[u8]) -> Option<&'static str> {
    let at = |from: usize, sig: &[u8]| bytes.get(from..from + sig.len()) == Some(sig);
    if at(0, b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if at(0, &[0xFF, 0xD8, 0xFF]) {
        Some("jpeg")
    } else if at(0, b"GIF87a") || at(0, b"GIF89a") {
        Some("gif")
    } else if at(0, b"RIFF") && at(8, b"WEBP") {
        Some("webp")
    } else if at(4, b"ftyp") && (at(8, b"avif") || at(8, b"avis")) {
        Some("avif")
    } else if at(0, b"BM") && bytes.len() > 26 {
        Some("bmp")
    } else {
        None
    }
}

/// The picture at `url`, its bytes as they came (an `ArrayBuffer` in the
/// page): only when the person clicked "Show image".
#[tauri::command]
pub async fn markdown_picture(url: String) -> Result<tauri::ipc::Response, String> {
    let endpoints = crate::web::Endpoints::current();
    let (_, bytes) = crate::web::fetch_picture(&endpoints, url.trim()).await?;
    let Some(kind) = kind_of(&bytes) else {
        tracing::info!(kb = bytes.len() / 1024, "Chat: a markdown picture wasn't one the page shows");
        return Err("That address isn't a picture Yap can show.".into());
    };
    tracing::info!(kind, kb = bytes.len() / 1024, "Chat: a markdown picture shown on click");
    Ok(tauri::ipc::Response::new(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_pictures_a_page_draws_are_handed_over() {
        assert_eq!(kind_of(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR"), Some("png"));
        assert_eq!(kind_of(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10]), Some("jpeg"));
        assert_eq!(kind_of(b"GIF89a\x01\0\x01\0"), Some("gif"));
        assert_eq!(kind_of(b"RIFF\x24\0\0\0WEBPVP8 "), Some("webp"));
        assert_eq!(kind_of(b"\0\0\0\x1cftypavif\0\0\0\0"), Some("avif"));
        assert_eq!(kind_of(&[b'B', b'M', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]), Some("bmp"));
        // An SVG (it could carry links and styles), a page, nothing: refused.
        assert_eq!(kind_of(b"<svg xmlns=\"http://www.w3.org/2000/svg\"><script/></svg>"), None);
        assert_eq!(kind_of(b"<!doctype html><title>Not a picture</title>"), None);
        assert_eq!(kind_of(b""), None);
        assert_eq!(kind_of(b"RIFF\0\0\0\0WAVE"), None);
        assert_eq!(kind_of(b"BM"), None);
    }
}

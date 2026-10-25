//! **"Send its text instead"** (phase 2): a screenshot for a model that
//! can't read images goes as its text, read **on this PC** by Windows' own
//! OCR (`Windows.Media.Ocr`, in every Windows 10 and 11; free, offline,
//! nothing uploaded to read it). Good for "what does this error say?" on
//! GPT-OSS or a small on-device model.
//!
//! The engine is the one for the person's own languages
//! (`OcrEngine::TryCreateFromUserProfileLanguages`), else the first
//! language Windows has text recognition for. A picture bigger than the
//! engine takes (`MaxImageDimension`, 2,600 px on Windows 11) is made
//! smaller first; a small one twice as big (Windows' OCR reads text of
//! ~10 px and up best, and a screenshot's small print is often less).
//!
//! The question goes as [`compose`] puts it: the question, then
//! "[Text read from the screenshot on this PC, sent instead of the
//! picture]" and the lines (Chat shows them folded under the question);
//! the screenshot is kept with the question, marked `asText`
//! (`chats::ChatImage`), so the thread still shows what was asked about.
//! Never logged: line and character counts only.

use std::sync::Mutex;

/// The line before the text in the question (Chat's bubble folds what's
/// after it: `src/lib/views/chat/chatImages.js` `SCREEN_TEXT_MARK`).
pub const MARK: &str = "[Text read from the screenshot on this PC, sent instead of the picture]";
/// …with more than one screenshot.
pub const MARK_MANY: &str = "[Text read from the screenshots on this PC, sent instead of the pictures]";
/// No text in the picture.
pub const NO_TEXT: &str = "No text found in the screenshot";
/// Windows has no text recognition for any language on this PC.
pub const NO_OCR: &str = "Windows' text recognition isn't set up on this PC: add a language in Settings → Time & language";
/// At most this many characters go (a screenful of small print is ~6,000).
pub const MAX_CHARS: usize = 12_000;

/// A word the engine read as "Ox80070005": the letter O for the zero of a
/// hex code's `0x` (and of its digits), which Segoe UI draws alike. Pure.
pub fn fix_hex(word: &str) -> String {
    let lower = word.to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix("ox") else { return word.to_string() };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_hexdigit() || *c == 'o').collect();
    if digits.len() < 2 || !digits.chars().any(|c| c.is_ascii_digit()) {
        return word.to_string();
    }
    let tail = &word[2 + digits.len()..];
    let fixed: String = word[2..2 + digits.len()].chars().map(|c| if c == 'O' || c == 'o' { '0' } else { c }).collect();
    format!("0x{fixed}{tail}")
}

/// The text of one picture's lines: each trimmed, blank ones dropped, one
/// to a line; capped at [`MAX_CHARS`]. Pure.
pub fn assemble(lines: &[String]) -> String {
    let mut out = String::new();
    for line in lines.iter().map(|l| l.split_whitespace().map(fix_hex).collect::<Vec<_>>().join(" ")).filter(|l| !l.is_empty()) {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&line);
    }
    if out.chars().count() > MAX_CHARS {
        out = out.chars().take(MAX_CHARS).collect::<String>();
        out.push_str("\n…");
    }
    out
}

/// The question with the screenshots' text after it (`texts`, one per
/// screenshot; blank ones left out). Pure.
pub fn compose(question: &str, texts: &[String]) -> String {
    let texts: Vec<&str> = texts.iter().map(|t| t.trim()).filter(|t| !t.is_empty()).collect();
    let mark = if texts.len() > 1 { MARK_MANY } else { MARK };
    format!("{}\n\n{mark}\n{}", question.trim(), texts.join("\n\n---\n\n"))
}

/// A question's own words, without the screenshot text [`compose`] put
/// after them (a new conversation's title). Pure.
pub fn question_of(text: &str) -> &str {
    [MARK, MARK_MANY].iter().filter_map(|m| text.find(&format!("\n\n{m}\n"))).min().map_or(text, |at| &text[..at])
}

/// How much to scale a `w`×`h` picture for the engine (`max`: its largest
/// side): smaller to fit, twice as big (smoothly) when it's small — on a
/// fixture of 13 px Segoe UI, 2× read "settings.json" where 1× and a
/// blocky 2× lost its dot. Pure.
pub fn ocr_scale(w: u32, h: u32, max: u32) -> f64 {
    let long = w.max(h).max(1) as f64;
    let max = max.max(1) as f64;
    if long > max {
        max / long
    } else if long * 2.0 <= max && w.min(h) < 1000 {
        2.0
    } else {
        1.0
    }
}

/// A test run's text in place of Windows' (`e2e_snip_ocr`): the UI suite
/// doesn't depend on the PC's OCR languages.
static FAKE: Mutex<Option<String>> = Mutex::new(None);

#[cfg(debug_assertions)]
pub fn set_fake(text: Option<String>) {
    *FAKE.lock().unwrap_or_else(|p| p.into_inner()) = text;
}

fn fake() -> Option<String> {
    if !crate::shell::e2e::active() {
        return None;
    }
    FAKE.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

/// The text in a PNG, read by Windows (blocking: a background thread).
pub fn read_png(png: &[u8]) -> Result<String, String> {
    if let Some(t) = fake() {
        return Ok(t);
    }
    let (rgba, w, h) = super::images::decode_png(png)?;
    let lines = recognize(&rgba, w, h)?;
    Ok(assemble(&lines))
}

/// The text of stored chat screenshots, one per id (blocking).
pub fn read_stored(ids: &[String]) -> Result<Vec<String>, String> {
    ids.iter()
        .map(|id| {
            let png = super::images::read(id).ok_or("That screenshot isn't there any more")?;
            read_png(&png)
        })
        .collect()
}

/// The question to send for screenshots that go as text: the question with
/// their text, and the screenshots as the question keeps them (`asText`).
/// Fails when there's no text in any of them.
pub async fn question_with_text(ids: &[String], question: &str) -> Result<(String, Vec<crate::chat::chats::ChatImage>), String> {
    let ids: Vec<String> = ids.iter().filter(|i| super::images::is_id(i)).take(super::images::MAX_PER_REQUEST).cloned().collect();
    let t = std::time::Instant::now();
    let (texts, images) = tauri::async_runtime::spawn_blocking(move || {
        let texts = read_stored(&ids)?;
        let images: Vec<crate::chat::chats::ChatImage> = ids
            .iter()
            .filter_map(|id| super::images::stored(id))
            .map(|i| crate::chat::chats::ChatImage { as_text: true, ..i })
            .collect();
        Ok::<_, String>((texts, images))
    })
    .await
    .map_err(|e| e.to_string())??;
    let chars: usize = texts.iter().map(|t| t.chars().count()).sum();
    tracing::info!(images = images.len(), chars, ms = t.elapsed().as_millis() as u64, "snip: text read from the screenshot");
    if chars == 0 {
        return Err(NO_TEXT.into());
    }
    Ok((compose(question, &texts), images))
}

#[cfg(windows)]
fn recognize(rgba: &[u8], w: u32, h: u32) -> Result<Vec<String>, String> {
    use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::DataWriter;
    let err = |e: windows::core::Error| format!("Windows' text recognition: {}", e.message());
    let engine = OcrEngine::TryCreateFromUserProfileLanguages()
        .ok()
        .or_else(|| {
            let langs = OcrEngine::AvailableRecognizerLanguages().ok()?;
            let first = langs.GetAt(0).ok()?;
            OcrEngine::TryCreateFromLanguage(&first).ok()
        })
        .ok_or_else(|| NO_OCR.to_string())?;
    let max = OcrEngine::MaxImageDimension().unwrap_or(2600);
    let scale = ocr_scale(w, h, max);
    let (sw, sh) = (((w as f64 * scale).round() as u32).clamp(1, max), ((h as f64 * scale).round() as u32).clamp(1, max));
    let sized = if (sw, sh) == (w, h) {
        rgba.to_vec()
    } else if scale > 1.0 {
        smooth_upscale(rgba, w, h, sw, sh)
    } else {
        super::images::shrink(rgba, w, h, sw, sh)
    };
    // RGBA → BGRA, as Windows' bitmaps hold it.
    let bgra: Vec<u8> = sized.as_chunks::<4>().0.iter().flat_map(|p| [p[2], p[1], p[0], 255]).collect();
    let writer = DataWriter::new().map_err(err)?;
    writer.WriteBytes(&bgra).map_err(err)?;
    let buffer = writer.DetachBuffer().map_err(err)?;
    let bitmap = SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, sw as i32, sh as i32).map_err(err)?;
    let result = engine.RecognizeAsync(&bitmap).and_then(|op| op.get()).map_err(err)?;
    let lines = result.Lines().map_err(err)?;
    let mut out = Vec::new();
    for i in 0..lines.Size().map_err(err)? {
        if let Ok(text) = lines.GetAt(i).and_then(|l| l.Text()) {
            out.push(text.to_string());
        }
    }
    let _ = bitmap.Close();
    Ok(out)
}

#[cfg(not(windows))]
fn recognize(_rgba: &[u8], _w: u32, _h: u32) -> Result<Vec<String>, String> {
    Err(NO_OCR.into())
}

/// RGBA made bigger by bilinear interpolation (the engine reads a smooth
/// 2× better than a blocky one). Pure.
pub fn smooth_upscale(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    let (sw, sh, dw, dh) = (sw as usize, sh as usize, dw.max(1) as usize, dh.max(1) as usize);
    let mut out = vec![0u8; dw * dh * 4];
    for y in 0..dh {
        let fy = ((y as f64 + 0.5) * sh as f64 / dh as f64 - 0.5).clamp(0.0, (sh - 1) as f64);
        let (y0, ty) = (fy.floor() as usize, fy - fy.floor());
        let y1 = (y0 + 1).min(sh - 1);
        for x in 0..dw {
            let fx = ((x as f64 + 0.5) * sw as f64 / dw as f64 - 0.5).clamp(0.0, (sw - 1) as f64);
            let (x0, tx) = (fx.floor() as usize, fx - fx.floor());
            let x1 = (x0 + 1).min(sw - 1);
            for c in 0..4 {
                let p = |xx: usize, yy: usize| src[(yy * sw + xx) * 4 + c] as f64;
                let top = p(x0, y0) * (1.0 - tx) + p(x1, y0) * tx;
                let bottom = p(x0, y1) * (1.0 - tx) + p(x1, y1) * tx;
                out[(y * dw + x) * 4 + c] = (top * (1.0 - ty) + bottom * ty).round() as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_become_the_text_and_the_question_carries_it() {
        let lines = vec!["  Error 0x80070005:  Access is denied. ".to_string(), "".into(), "Couldn't open settings.json".into()];
        assert_eq!(assemble(&lines), "Error 0x80070005: Access is denied.\nCouldn't open settings.json");
        assert_eq!(assemble(&[]), "");
        let long = vec!["x".repeat(MAX_CHARS + 50)];
        assert!(assemble(&long).ends_with("\n…"));
        assert_eq!(assemble(&long).chars().count(), MAX_CHARS + 2);
        let q = compose("What's this? ", &["Error 42".into()]);
        assert_eq!(q, format!("What's this?\n\n{MARK}\nError 42"));
        let two = compose("Compare these", &["One".into(), "  ".into(), "Two".into()]);
        assert_eq!(two, format!("Compare these\n\n{MARK_MANY}\nOne\n\n---\n\nTwo"));
        // The question's own words (a new conversation's title).
        assert_eq!(question_of(&q), "What's this?");
        assert_eq!(question_of(&two), "Compare these");
        assert_eq!(question_of("Plain question"), "Plain question");
    }

    #[test]
    fn a_picture_is_fitted_to_the_engine() {
        assert_eq!(ocr_scale(5120, 1440, 2600), 2600.0 / 5120.0);
        assert_eq!(ocr_scale(600, 200, 2600), 2.0, "small print, twice as big");
        assert_eq!(ocr_scale(1600, 1200, 2600), 1.0);
        assert_eq!(ocr_scale(2000, 1100, 2600), 1.0);
        // Twice as big, smoothly: the ends keep their colour, between them
        // a blend.
        let px = [0, 0, 0, 255, 200, 100, 40, 255];
        let out = smooth_upscale(&px, 2, 1, 4, 1);
        assert_eq!(&out[..4], &[0, 0, 0, 255]);
        assert_eq!(&out[12..], &[200, 100, 40, 255]);
        assert_eq!(&out[4..8], &[50, 25, 10, 255]);
        assert_eq!(smooth_upscale(&px, 2, 1, 4, 2)[..16], out[..]);
    }

    #[test]
    fn a_hex_code_read_with_the_letter_o_gets_its_zero() {
        assert_eq!(fix_hex("Ox80070005:"), "0x80070005:");
        assert_eq!(fix_hex("ox8OO7"), "0x8007");
        assert_eq!(fix_hex("0x1F"), "0x1F");
        // Words that only start like one stay as they are.
        for word in ["Oxford", "ox", "Oxo", "box", "OxBEEF", "Oxen."] {
            assert_eq!(fix_hex(word), word);
        }
        assert_eq!(assemble(&["Error Ox80070005: Access is denied.".into()]), "Error 0x80070005: Access is denied.");
    }

    /// Windows' OCR on a fixture: a made-up error dialog's text. Needs a
    /// language with text recognition (any English Windows has one); says
    /// so and passes where there's none.
    #[cfg(windows)]
    #[test]
    fn windows_reads_the_text_of_a_fixture() {
        let png = include_bytes!("fixtures/ocr-error.png");
        match read_png(png) {
            Ok(text) => {
                let flat = text.to_lowercase();
                assert!(flat.contains("access is denied"), "{text:?}");
                assert!(flat.contains("settings.json"), "{text:?}");
                assert!(flat.contains("0x80070005"), "{text:?}");
            }
            Err(e) if e == NO_OCR => eprintln!("no OCR language on this PC: skipped"),
            Err(e) => panic!("{e}"),
        }
    }
}

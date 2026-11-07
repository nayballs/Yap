//! A Markdown file's bytes and the page's text, both ways (pure).
//!
//! The page edits text with `\n` line breaks (a text box's `value` and the
//! rich editor both give them so). On disk the file keeps what it had:
//! its encoding — UTF-8 with or without a byte-order mark, or UTF-16 with
//! one — and its line endings, `\r\n` or `\n` (a file with both: the more
//! common one, ties to `\r\n`; one with no line break: `\n`). Bytes that
//! aren't text in any of those are shown (`from_utf8_lossy`) but never
//! written back: [`Decoded::form`] is `None` and the window is read-only.

/// How a file's text is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    /// UTF-8 starting with EF BB BF (Windows' Notepad wrote these until 2019).
    Utf8Bom,
    Utf16Le,
    Utf16Be,
}

impl Encoding {
    /// The name the page shows ("UTF-8", "UTF-8 with BOM"…).
    pub fn name(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Utf8Bom => "UTF-8 with BOM",
            Encoding::Utf16Le => "UTF-16 LE",
            Encoding::Utf16Be => "UTF-16 BE",
        }
    }
}

/// A file's line endings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eol {
    Lf,
    CrLf,
}

impl Eol {
    pub fn name(self) -> &'static str {
        match self {
            Eol::Lf => "LF",
            Eol::CrLf => "CRLF",
        }
    }
}

/// What a file is written in: its encoding and its line endings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Form {
    pub encoding: Encoding,
    pub eol: Eol,
}

/// A file read for the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    /// Its text, every `\r\n` a `\n`.
    pub text: String,
    /// How to write it back; `None`: not text Yap can write back as it was.
    pub form: Option<Form>,
}

const BOM8: &[u8] = &[0xEF, 0xBB, 0xBF];

/// A file's bytes as the page's text, and how to write them back.
pub fn decode(bytes: &[u8]) -> Decoded {
    let (text, encoding) = if let Some(rest) = bytes.strip_prefix(BOM8) {
        (std::str::from_utf8(rest).ok().map(str::to_string), Encoding::Utf8Bom)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        (utf16(rest, u16::from_le_bytes), Encoding::Utf16Le)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        (utf16(rest, u16::from_be_bytes), Encoding::Utf16Be)
    } else {
        (std::str::from_utf8(bytes).ok().map(str::to_string), Encoding::Utf8)
    };
    match text {
        Some(text) => {
            let eol = line_endings(&text);
            Decoded { text: text.replace("\r\n", "\n"), form: Some(Form { encoding, eol }) }
        }
        None => Decoded { text: String::from_utf8_lossy(bytes).replace("\r\n", "\n"), form: None },
    }
}

fn utf16(bytes: &[u8], word: fn([u8; 2]) -> u16) -> Option<String> {
    let (pairs, rest) = bytes.as_chunks::<2>();
    if !rest.is_empty() {
        return None;
    }
    let units: Vec<u16> = pairs.iter().map(|&c| word(c)).collect();
    String::from_utf16(&units).ok()
}

/// The line endings `text` mostly has.
pub fn line_endings(text: &str) -> Eol {
    let crlf = text.matches("\r\n").count();
    let lf = text.matches('\n').count() - crlf;
    if crlf > 0 && crlf >= lf {
        Eol::CrLf
    } else {
        Eol::Lf
    }
}

/// The page's `text` as the file's bytes, in `form`.
pub fn encode(text: &str, form: Form) -> Vec<u8> {
    let lf = text.replace("\r\n", "\n");
    let text = match form.eol {
        Eol::Lf => lf,
        Eol::CrLf => lf.replace('\n', "\r\n"),
    };
    match form.encoding {
        Encoding::Utf8 => text.into_bytes(),
        Encoding::Utf8Bom => [BOM8, text.as_bytes()].concat(),
        Encoding::Utf16Le => std::iter::once(0xFEFFu16).chain(text.encode_utf16()).flat_map(u16::to_le_bytes).collect(),
        Encoding::Utf16Be => std::iter::once(0xFEFFu16).chain(text.encode_utf16()).flat_map(u16::to_be_bytes).collect(),
    }
}

/// "7.2 MB" (one decimal under 10, whole above), "640 KB", "12 bytes".
pub fn size_label(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= MB {
        let mb = b / MB;
        if mb < 10.0 {
            format!("{mb:.1} MB")
        } else {
            format!("{mb:.0} MB")
        }
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} bytes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAN: &str = "# Garden plan\r\n\r\n- Sow the beans\r\n- Mend the fence\r\n";

    #[test]
    fn crlf_comes_back_as_crlf() {
        let d = decode(PLAN.as_bytes());
        assert_eq!(d.text, "# Garden plan\n\n- Sow the beans\n- Mend the fence\n");
        let form = d.form.unwrap();
        assert_eq!(form, Form { encoding: Encoding::Utf8, eol: Eol::CrLf });
        // Read and written back untouched: byte for byte.
        assert_eq!(encode(&d.text, form), PLAN.as_bytes());
        // A line added on the page gets the file's endings too.
        let edited = d.text.replace("- Mend the fence\n", "- Mend the fence\n- Water the roses\n");
        assert_eq!(
            String::from_utf8(encode(&edited, form)).unwrap(),
            "# Garden plan\r\n\r\n- Sow the beans\r\n- Mend the fence\r\n- Water the roses\r\n"
        );
    }

    #[test]
    fn lf_stays_lf_and_a_missing_last_newline_stays_missing() {
        let src = "# Notes\n\nNo newline at the end";
        let d = decode(src.as_bytes());
        let form = d.form.unwrap();
        assert_eq!(form.eol, Eol::Lf);
        assert_eq!(encode(&d.text, form), src.as_bytes());
        // Text with \r\n from the page (a paste) is written in the file's endings.
        assert_eq!(encode("a\r\nb\n", form), b"a\nb\n");
    }

    #[test]
    fn mixed_endings_follow_the_most_common_ties_to_crlf_and_none_is_lf() {
        assert_eq!(line_endings("a\r\nb\r\nc\nd"), Eol::CrLf);
        assert_eq!(line_endings("a\nb\nc\r\nd"), Eol::Lf);
        assert_eq!(line_endings("a\r\nb\nc"), Eol::CrLf);
        assert_eq!(line_endings("one line"), Eol::Lf);
        assert_eq!(line_endings(""), Eol::Lf);
    }

    #[test]
    fn a_byte_order_mark_is_kept() {
        let bytes = [BOM8, PLAN.as_bytes()].concat();
        let d = decode(&bytes);
        assert!(!d.text.starts_with('\u{feff}'), "the mark isn't part of the page's text");
        let form = d.form.unwrap();
        assert_eq!(form.encoding, Encoding::Utf8Bom);
        assert_eq!(encode(&d.text, form), bytes);
        // Without one, none is added.
        let plain = decode(PLAN.as_bytes()).form.unwrap();
        assert!(!encode("x", plain).starts_with(BOM8));
    }

    #[test]
    fn utf16_files_round_trip_in_their_own_byte_order() {
        for (encoding, word) in [(Encoding::Utf16Le, u16::to_le_bytes as fn(u16) -> [u8; 2]), (Encoding::Utf16Be, u16::to_be_bytes)] {
            let bytes: Vec<u8> = std::iter::once(0xFEFFu16).chain("Café ☕\r\nDone\r\n".encode_utf16()).flat_map(word).collect();
            let d = decode(&bytes);
            assert_eq!(d.text, "Café ☕\nDone\n");
            let form = d.form.unwrap();
            assert_eq!(form, Form { encoding, eol: Eol::CrLf });
            assert_eq!(encode(&d.text, form), bytes);
        }
    }

    #[test]
    fn bytes_that_arent_text_are_shown_but_never_written_back() {
        let d = decode(b"Price: \x80 caf\xe9\n");
        assert_eq!(d.form, None);
        assert!(d.text.starts_with("Price: "));
        // An odd-length UTF-16 file, or one with a lone surrogate.
        assert_eq!(decode(&[0xFF, 0xFE, 0x41]).form, None);
        assert_eq!(decode(&[0xFF, 0xFE, 0x00, 0xD8]).form, None);
    }

    #[test]
    fn non_ascii_text_survives() {
        let src = "## Résumé — naïve ünïcode 日本語\n";
        let d = decode(src.as_bytes());
        assert_eq!(d.text, src);
        assert_eq!(encode(&d.text, d.form.unwrap()), src.as_bytes());
    }

    #[test]
    fn sizes_in_words() {
        assert_eq!(size_label(12), "12 bytes");
        assert_eq!(size_label(640 * 1024), "640 KB");
        assert_eq!(size_label(7 * 1024 * 1024 + 200 * 1024), "7.2 MB");
        assert_eq!(size_label(42 * 1024 * 1024), "42 MB");
    }
}

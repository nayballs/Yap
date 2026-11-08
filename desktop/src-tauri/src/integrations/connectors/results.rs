//! **A connector tool's answer, in Yap's tool-result shape** (`{success,
//! data, displayText}` or `{success: false, error, …}`, what the tool loop
//! gives a model for every tool). From MCP's `CallToolResult`, as JSON:
//!
//! - text blocks joined;
//! - `structuredContent` as JSON, when there's no text to read;
//! - an embedded text resource as its text, a binary one named, a
//!   `resource_link` named with its address;
//! - an image kept aside as a [`Picture`] (for a model that reads images,
//!   wave 4: `pictures::Look`) and named in the text ("[a picture, 640×480]");
//! - audio dropped, and said;
//! - `isError` → `success: false` with its text;
//! - the whole cut at [`MAX_TEXT`] (1 MB), with a line saying so. The tool
//!   loop fits it to the room left after that (`chat/budget.rs`).
//!
//! What a server returns is outside content: wave 4's tool loop treats it
//! as it treats a web page. Nothing here is logged. (A release build calls
//! no tool yet: `call.rs`.)
#![cfg_attr(not(debug_assertions), allow(dead_code))]

use base64::Engine as _;
use serde_json::{json, Value};

/// The most of a result's text kept (bytes).
pub const MAX_TEXT: usize = 1024 * 1024;
/// The most pictures one result keeps.
pub const MAX_PICTURES: usize = 4;
/// The biggest picture kept (bytes, decoded).
pub const MAX_PICTURE_BYTES: usize = 5 * 1024 * 1024;

/// A picture a tool returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picture {
    pub mime: String,
    /// Base64, as it came.
    pub data: String,
    /// Its size, when its header says (PNG, JPEG, GIF, WebP).
    pub size: Option<(u32, u32)>,
}

/// A result, shaped.
#[derive(Debug, Clone, PartialEq)]
pub struct Shaped {
    pub success: bool,
    /// What the model reads.
    pub text: String,
    /// The structured result, when it's what there is.
    pub structured: Option<Value>,
    pub pictures: Vec<Picture>,
    /// The text was cut at [`MAX_TEXT`].
    pub cut: bool,
}

impl Shaped {
    /// Yap's tool-result JSON, with `display` for the chip ("Notion ·
    /// Search").
    pub fn to_value(&self, display: &str) -> Value {
        if self.success {
            let mut data = json!({ "text": self.text });
            if let Some(s) = &self.structured {
                data["structured"] = s.clone();
            }
            if !self.pictures.is_empty() {
                data["pictures"] = json!(self.pictures.len());
            }
            json!({ "success": true, "data": data, "displayText": display })
        } else {
            json!({ "success": false, "error": self.text, "data": Value::Null, "displayText": display })
        }
    }
}

/// "[a picture, 640×480]".
fn picture_line(size: Option<(u32, u32)>) -> String {
    match size {
        Some((w, h)) => format!("[a picture, {w}\u{d7}{h}]"),
        None => "[a picture]".into(),
    }
}

/// A picture's width and height from its first bytes (PNG, GIF, JPEG,
/// WebP). Pure.
pub fn picture_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let be32 = |b: &[u8]| u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
    if bytes.len() >= 24 && bytes.starts_with(b"\x89PNG\r\n\x1a\n") && &bytes[12..16] == b"IHDR" {
        return Some((be32(&bytes[16..20]), be32(&bytes[20..24])));
    }
    if bytes.len() >= 10 && (bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a")) {
        return Some((u32::from(u16::from_le_bytes([bytes[6], bytes[7]])), u32::from(u16::from_le_bytes([bytes[8], bytes[9]]))));
    }
    if bytes.len() >= 30 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return match &bytes[12..16] {
            b"VP8X" => {
                let w = 1 + (u32::from(bytes[24]) | u32::from(bytes[25]) << 8 | u32::from(bytes[26]) << 16);
                let h = 1 + (u32::from(bytes[27]) | u32::from(bytes[28]) << 8 | u32::from(bytes[29]) << 16);
                Some((w, h))
            }
            b"VP8 " => Some((u32::from(u16::from_le_bytes([bytes[26], bytes[27]]) & 0x3fff), u32::from(u16::from_le_bytes([bytes[28], bytes[29]]) & 0x3fff))),
            b"VP8L" if bytes.len() >= 25 => {
                let b = &bytes[21..25];
                let w = 1 + (u32::from(b[0]) | (u32::from(b[1] & 0x3f) << 8));
                let h = 1 + ((u32::from(b[1]) >> 6) | (u32::from(b[2]) << 2) | (u32::from(b[3] & 0x0f) << 10));
                Some((w, h))
            }
            _ => None,
        };
    }
    if bytes.len() >= 4 && bytes[0] == 0xff && bytes[1] == 0xd8 {
        let mut i = 2;
        while i + 9 < bytes.len() {
            if bytes[i] != 0xff {
                i += 1;
                continue;
            }
            let marker = bytes[i + 1];
            if (0xc0..=0xcf).contains(&marker) && ![0xc4, 0xc8, 0xcc].contains(&marker) {
                let h = u32::from(u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]));
                let w = u32::from(u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]));
                return Some((w, h));
            }
            let len = usize::from(u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]));
            i += 2 + len;
        }
    }
    None
}

/// The first `max` bytes of `s` (at a char boundary), and whether it was cut.
fn cap(s: String, max: usize) -> (String, bool) {
    if s.len() <= max {
        return (s, false);
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    (s[..end].to_string(), true)
}

/// A `CallToolResult` (as JSON) shaped. Pure.
pub fn shape(result: &Value) -> Shaped {
    let mut parts: Vec<String> = Vec::new();
    let mut pictures = Vec::new();
    for block in result.get("content").and_then(Value::as_array).into_iter().flatten() {
        match block.get("type").and_then(Value::as_str).unwrap_or("") {
            "text" => {
                if let Some(t) = block.get("text").and_then(Value::as_str) {
                    if !t.is_empty() {
                        parts.push(t.to_string());
                    }
                }
            }
            "image" => {
                let mime = block.get("mimeType").and_then(Value::as_str).unwrap_or("image/png").to_string();
                let data = block.get("data").and_then(Value::as_str).unwrap_or("").to_string();
                let bytes = base64::engine::general_purpose::STANDARD.decode(data.as_bytes()).ok();
                let size = bytes.as_deref().and_then(picture_size);
                parts.push(picture_line(size));
                let fits = bytes.as_ref().is_some_and(|b| b.len() <= MAX_PICTURE_BYTES);
                if fits && mime.starts_with("image/") && pictures.len() < MAX_PICTURES {
                    pictures.push(Picture { mime, data, size });
                }
            }
            "audio" => parts.push("[a sound clip, which Yap doesn't pass on]".into()),
            "resource" => {
                let r = block.get("resource").cloned().unwrap_or(Value::Null);
                let uri = r.get("uri").and_then(Value::as_str).unwrap_or("");
                match r.get("text").and_then(Value::as_str) {
                    Some(t) => parts.push(if uri.is_empty() { t.to_string() } else { format!("{uri}:\n{t}") }),
                    None => {
                        let mime = r.get("mimeType").and_then(Value::as_str).unwrap_or("a file");
                        parts.push(format!("[a file, {mime}: {uri}]"));
                    }
                }
            }
            "resource_link" => {
                let uri = block.get("uri").and_then(Value::as_str).unwrap_or("");
                let name = block.get("title").or_else(|| block.get("name")).and_then(Value::as_str).unwrap_or(uri);
                parts.push(format!("[a link: {name} ({uri})]"));
            }
            _ => {}
        }
    }
    let error = result.get("isError").and_then(Value::as_bool).unwrap_or(false);
    let structured = result.get("structuredContent").filter(|v| !v.is_null()).cloned();
    let has_text = parts.iter().any(|p| !p.starts_with('['));
    // Structured content stands in for text that isn't there.
    let structured = match structured {
        Some(s) if !has_text => {
            let json = serde_json::to_string_pretty(&s).unwrap_or_default();
            parts.insert(0, json);
            if error {
                None
            } else {
                Some(s)
            }
        }
        _ => None,
    };
    let mut text = parts.join("\n\n");
    if text.is_empty() {
        text = if error { "The tool failed without saying why.".into() } else { "(The tool returned nothing.)".into() };
    }
    let (mut text, cut) = cap(text, MAX_TEXT);
    if cut {
        text.push_str("\n\n[Cut here: the rest was more than 1 MB.]");
    }
    // Structured content that big isn't kept beside the text.
    let structured = structured.filter(|s| !cut && serde_json::to_string(s).map_or(0, |j| j.len()) <= MAX_TEXT);
    Shaped { success: !error, text, structured, pictures, cut }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        b.extend_from_slice(&w.to_be_bytes());
        b.extend_from_slice(&h.to_be_bytes());
        b.extend_from_slice(&[8, 6, 0, 0, 0]);
        b
    }

    #[test]
    fn text_blocks_join_and_an_error_says_so() {
        let r = shape(&json!({ "content": [{ "type": "text", "text": "Kitchen: on" }, { "type": "text", "text": "Hall: off" }] }));
        assert!(r.success);
        assert_eq!(r.text, "Kitchen: on\n\nHall: off");
        let v = r.to_value("Home · List lights");
        assert_eq!(v, json!({ "success": true, "data": { "text": "Kitchen: on\n\nHall: off" }, "displayText": "Home · List lights" }));

        let e = shape(&json!({ "content": [{ "type": "text", "text": "No such light" }], "isError": true }));
        assert!(!e.success);
        assert_eq!(e.to_value("Home · Turn off")["error"], "No such light");
        assert_eq!(shape(&json!({ "isError": true })).text, "The tool failed without saying why.");
        assert_eq!(shape(&json!({})).text, "(The tool returned nothing.)");
    }

    #[test]
    fn structured_content_stands_in_for_missing_text_only() {
        let r = shape(&json!({ "content": [], "structuredContent": { "lights": [{ "name": "Kitchen", "on": true }] } }));
        assert!(r.text.contains("\"Kitchen\""));
        assert_eq!(r.structured, Some(json!({ "lights": [{ "name": "Kitchen", "on": true }] })));
        assert_eq!(r.to_value("x")["data"]["structured"]["lights"][0]["on"], true);
        // With text too: the text, not the same thing twice.
        let r = shape(&json!({ "content": [{ "type": "text", "text": "Kitchen is on" }], "structuredContent": { "on": true } }));
        assert_eq!(r.text, "Kitchen is on");
        assert_eq!(r.structured, None);
    }

    #[test]
    fn pictures_are_kept_aside_and_named_and_audio_is_dropped() {
        let data = base64::engine::general_purpose::STANDARD.encode(png(640, 480));
        let r = shape(&json!({ "content": [
            { "type": "text", "text": "The floor plan:" },
            { "type": "image", "mimeType": "image/png", "data": data },
            { "type": "audio", "mimeType": "audio/wav", "data": "AAAA" }
        ] }));
        assert_eq!(r.text, "The floor plan:\n\n[a picture, 640\u{d7}480]\n\n[a sound clip, which Yap doesn't pass on]");
        assert_eq!(r.pictures.len(), 1);
        assert_eq!((r.pictures[0].mime.as_str(), r.pictures[0].size), ("image/png", Some((640, 480))));
        assert_eq!(r.to_value("x")["data"]["pictures"], 1);
        // Not a picture after all: named, not kept.
        let r = shape(&json!({ "content": [{ "type": "image", "mimeType": "text/html", "data": "PGI+" }] }));
        assert_eq!((r.text.as_str(), r.pictures.len()), ("[a picture]", 0));
    }

    #[test]
    fn resources_are_read_or_named() {
        let r = shape(&json!({ "content": [
            { "type": "resource", "resource": { "uri": "file:///notes/plan.md", "mimeType": "text/markdown", "text": "# Plan" } },
            { "type": "resource", "resource": { "uri": "file:///a.pdf", "mimeType": "application/pdf", "blob": "JVBERi0=" } },
            { "type": "resource_link", "uri": "https://example.com/doc/1", "name": "doc-1", "title": "Launch notes" }
        ] }));
        assert_eq!(r.text, "file:///notes/plan.md:\n# Plan\n\n[a file, application/pdf: file:///a.pdf]\n\n[a link: Launch notes (https://example.com/doc/1)]");
    }

    #[test]
    fn a_huge_result_is_cut_at_a_megabyte() {
        let big = "é".repeat(MAX_TEXT); // 2 bytes each: twice the cap
        let r = shape(&json!({ "content": [{ "type": "text", "text": big }] }));
        assert!(r.cut);
        assert!(r.text.len() <= MAX_TEXT + 64);
        assert!(r.text.ends_with("[Cut here: the rest was more than 1 MB.]"));
    }

    #[test]
    fn picture_sizes_from_their_headers() {
        assert_eq!(picture_size(&png(1, 2)), Some((1, 2)));
        assert_eq!(picture_size(b"GIF89a\x10\x00\x20\x00rest"), Some((16, 32)));
        // A JPEG: SOI, an APP0 segment, then SOF0 (height 480, width 640).
        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0x00, 0x00];
        jpeg.extend_from_slice(&[0xff, 0xc0, 0x00, 0x11, 0x08, 0x01, 0xe0, 0x02, 0x80, 0x03, 0, 0, 0, 0]);
        assert_eq!(picture_size(&jpeg), Some((640, 480)));
        assert_eq!(picture_size(b"not a picture"), None);
    }
}

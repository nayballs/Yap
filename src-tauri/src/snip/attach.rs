//! **Paste and drop images into Chat** (phase 2): Ctrl+V of a picture
//! copied in any app, or image files dropped on Chat, land in the message
//! box exactly as a snip does — the same `chat-images/` store, retention
//! and vision rules (`images.rs`, `vision.rs`), the copy for the model made
//! in the background.
//!
//! - **Paste**: the page hands over the image its paste event carried
//!   (WebView2 turns the clipboard's bitmap into a PNG) as the command's
//!   raw body — never base64 through JSON.
//! - **Drop**: the files Windows dropped on the window (Tauri's drag-drop
//!   event, as Upload uses it): only image files (PNG, JPEG, GIF, BMP,
//!   WebP, TIFF, HEIC with Windows' codec), at most [`MAX_FILE_BYTES`]
//!   each and [`super::images::MAX_PER_REQUEST`] at once.
//!
//! Every picture is decoded (PNG here; anything else by Windows' own
//! codecs, `Windows.Graphics.Imaging`, the EXIF rotation and colour
//! profile applied) and stored again as a PNG: nothing that isn't a
//! picture is kept, whatever its name says. At most [`MAX_PIXELS`].
//! Logs: sizes and counts only.

use std::path::Path;

use crate::chat::chats::ChatImage;

/// A dropped file bigger than this isn't read.
pub const MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;
/// A picture with more pixels than this (a 10,000 × 10,000 photo) isn't kept.
pub const MAX_PIXELS: u64 = 100_000_000;
/// The image files a drop takes.
pub const EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "jfif", "gif", "bmp", "webp", "tif", "tiff", "heic", "heif"];

/// Whether a dropped file's name says it's a picture. Pure.
pub fn is_image_name(name: &str) -> bool {
    Path::new(name).extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// A picture's bytes → RGBA and its size: a PNG here, anything else by
/// Windows' codecs.
pub fn decode(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        if let Some((w, h)) = super::images::png_size(bytes) {
            if w as u64 * h as u64 > MAX_PIXELS {
                return Err("That picture is too big".into());
            }
        }
        if let Ok(decoded) = super::images::decode_png(bytes) {
            return Ok(decoded);
        }
    }
    decode_other(bytes)
}

#[cfg(windows)]
fn decode_other(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat, BitmapTransform, ColorManagementMode, ExifOrientationMode};
    use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};
    let err = |_e: windows::core::Error| "That isn't a picture Windows can read".to_string();
    let stream = InMemoryRandomAccessStream::new().map_err(err)?;
    let writer = DataWriter::CreateDataWriter(&stream).map_err(err)?;
    writer.WriteBytes(bytes).map_err(err)?;
    writer.StoreAsync().and_then(|op| op.get()).map_err(err)?;
    writer.FlushAsync().and_then(|op| op.get()).map_err(err)?;
    let _ = writer.DetachStream();
    stream.Seek(0).map_err(err)?;
    let decoder = BitmapDecoder::CreateAsync(&stream).and_then(|op| op.get()).map_err(err)?;
    let (w, h) = (decoder.OrientedPixelWidth().map_err(err)?, decoder.OrientedPixelHeight().map_err(err)?);
    if w == 0 || h == 0 || w as u64 * h as u64 > MAX_PIXELS {
        return Err("That picture is too big".into());
    }
    let pixels = decoder
        .GetPixelDataTransformedAsync(
            BitmapPixelFormat::Rgba8,
            BitmapAlphaMode::Straight,
            &BitmapTransform::new().map_err(err)?,
            ExifOrientationMode::RespectExifOrientation,
            ColorManagementMode::ColorManageToSRgb,
        )
        .and_then(|op| op.get())
        .and_then(|p| p.DetachPixelData())
        .map_err(err)?;
    let rgba = pixels.to_vec();
    if rgba.len() != (w as usize) * (h as usize) * 4 {
        return Err("That isn't a picture Windows can read".into());
    }
    Ok((rgba, w, h))
}

#[cfg(not(windows))]
fn decode_other(_bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    Err("Only PNG pictures can be read here".into())
}

/// Store a picture for Chat (as a snip is stored) and start its copy for
/// the Chat model.
pub fn keep(bytes: &[u8], source: &str) -> Result<ChatImage, String> {
    let (rgba, w, h) = decode(bytes)?;
    // Fully transparent pixels (a copied icon, a cut-out) on white: models
    // see transparency as black.
    let rgba = on_white(rgba);
    let png = super::images::encode_png(&rgba, w, h, true)?;
    let saved = super::images::save(&png, w, h)?;
    let image = ChatImage { id: saved.id, w: saved.w, h: saved.h, bytes: saved.bytes, source: source.into(), ..Default::default() };
    prepare(&image.id);
    Ok(image)
}

/// A picture's see-through pixels laid on white (opaque ones unchanged).
/// Pure.
pub fn on_white(mut rgba: Vec<u8>) -> Vec<u8> {
    for p in rgba.as_chunks_mut::<4>().0 {
        if p[3] < 255 {
            let a = p[3] as u32;
            for c in p.iter_mut().take(3) {
                *c = ((*c as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
            }
            p[3] = 255;
        }
    }
    rgba
}

/// The copy for the Chat model now, in the background (Send stays instant).
fn prepare(id: &str) {
    let cfg = crate::config::load();
    if let Ok((base_url, _key, _model, provider, _p, _t)) = crate::chat::turn::resolve_chat_endpoint(&cfg) {
        let here = crate::ai::ai_model::runs_here(&provider, &base_url);
        super::images::prepare(vec![id.to_string()], super::images::long_edge(&provider, here));
    }
}

/// Chat's message box: a picture pasted (Ctrl+V). The body is the image's
/// bytes as the paste event carried them.
#[tauri::command]
pub async fn chat_image_paste(app: tauri::AppHandle, request: tauri::ipc::Request<'_>) -> Result<ChatImage, String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("No picture came with the paste".into());
    };
    if bytes.is_empty() || bytes.len() as u64 > MAX_FILE_BYTES {
        return Err("That picture is too big to paste".into());
    }
    let bytes = bytes.clone();
    let image = tauri::async_runtime::spawn_blocking(move || keep(&bytes, "paste")).await.map_err(|e| e.to_string())??;
    tracing::info!(w = image.w, h = image.h, kb = image.bytes / 1024, "Chat: a picture pasted");
    // Waiting in the box (never copied back to the clipboard: it came from there).
    super::waiting::added(&app, vec![image.clone()]);
    Ok(image)
}

/// What a drop kept, and how many files it left (not pictures, too big,
/// past the four a question carries).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dropped {
    pub images: Vec<ChatImage>,
    pub skipped: usize,
}

/// Chat's message box: files dropped on the window (Tauri's drag-drop
/// event's paths). `room`: how many more the question can carry.
#[tauri::command]
pub async fn chat_image_drop(app: tauri::AppHandle, paths: Vec<String>, room: Option<usize>) -> Result<Dropped, String> {
    let room = room.unwrap_or(super::images::MAX_PER_REQUEST).min(super::images::MAX_PER_REQUEST);
    let total = paths.len();
    let dropped = tauri::async_runtime::spawn_blocking(move || {
        let mut images = Vec::new();
        for p in paths.iter().filter(|p| is_image_name(p)) {
            if images.len() >= room {
                break;
            }
            let path = Path::new(p);
            let Ok(meta) = std::fs::metadata(path) else { continue };
            if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
                continue;
            }
            match std::fs::read(path).map_err(|e| e.to_string()).and_then(|b| keep(&b, "drop")) {
                Ok(image) => images.push(image),
                Err(e) => tracing::debug!("Chat: a dropped file wasn't kept ({e})"),
            }
        }
        Dropped { skipped: total - images.len(), images }
    })
    .await
    .map_err(|e| e.to_string())?;
    tracing::info!(images = dropped.images.len(), skipped = dropped.skipped, "Chat: files dropped");
    // Waiting in the box (not copied: they came from files).
    if !dropped.images.is_empty() {
        super::waiting::added(&app, dropped.images.clone());
    }
    Ok(dropped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_picture_files_are_taken() {
        for yes in ["shot.png", "C:\\Users\\example\\Pictures\\Photo.JPG", "a.jpeg", "x.webp", "scan.tiff", "phone.heic", "old.bmp", "anim.gif"] {
            assert!(is_image_name(yes), "{yes}");
        }
        for no in ["notes.txt", "picture", "song.mp3", "image.png.exe", "archive.zip", ".png"] {
            assert!(!is_image_name(no), "{no}");
        }
    }

    #[test]
    fn a_png_is_read_and_anything_else_refused_politely() {
        let rgba: Vec<u8> = (0..4 * 3 * 2).map(|i| (i * 9) as u8).collect();
        let png = super::super::images::encode_png(&rgba, 3, 2, true).unwrap();
        assert_eq!(decode(&png).unwrap(), (rgba, 3, 2));
        let not = decode(b"hello, this is not a picture");
        assert!(not.is_err());
    }

    /// A BMP (any non-PNG picture) through Windows' codecs.
    #[cfg(windows)]
    #[test]
    fn windows_reads_other_formats() {
        // 2×1, 24-bit BMP: blue then red (BGR, rows padded to 4 bytes).
        let mut bmp = Vec::new();
        bmp.extend_from_slice(b"BM");
        bmp.extend_from_slice(&(54u32 + 8).to_le_bytes());
        bmp.extend_from_slice(&[0, 0, 0, 0]);
        bmp.extend_from_slice(&54u32.to_le_bytes());
        bmp.extend_from_slice(&40u32.to_le_bytes());
        bmp.extend_from_slice(&2i32.to_le_bytes());
        bmp.extend_from_slice(&1i32.to_le_bytes());
        bmp.extend_from_slice(&1u16.to_le_bytes());
        bmp.extend_from_slice(&24u16.to_le_bytes());
        bmp.extend_from_slice(&[0u8; 24]);
        bmp.extend_from_slice(&[255, 0, 0, 0, 0, 255, 0, 0]);
        let (rgba, w, h) = decode(&bmp).unwrap();
        assert_eq!((w, h), (2, 1));
        assert_eq!(rgba, vec![0, 0, 255, 255, 255, 0, 0, 255]);
    }

    #[test]
    fn see_through_pixels_go_on_white() {
        let out = on_white(vec![10, 20, 30, 255, 0, 0, 0, 0, 0, 0, 0, 128]);
        assert_eq!(out, vec![10, 20, 30, 255, 255, 255, 255, 255, 127, 127, 127, 255]);
    }
}

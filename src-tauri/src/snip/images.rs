//! Chat's screenshots on disk (`<data>/chat-images/`), how long they're
//! kept, and the copy each model gets.
//!
//! **Stored** as PNG files, `img_<16 hex>.png`, written atomically (temp +
//! rename, as every Yap store); `chats.json` holds only the id, the size,
//! the bytes and where it came from (`chats::ChatImage`), never pixels —
//! the store is rewritten whole on every message. A page shows one through
//! the `yapsnip://` scheme by id only, never a path ([`path`] refuses
//! anything that isn't an id).
//!
//! **Kept**: with their conversation (deleting it deletes them), and for
//! Settings → Privacy & data → "Keep chat screenshots" — 7, 30 (the
//! default) or 90 days, or forever ([`sweep`], at launch and daily). An
//! image nothing refers to any more (a snip never sent) goes after a day.
//! Its message keeps the metadata, and Chat shows "Screenshot removed after
//! 30 days" in its place.
//!
//! **Sent** fitted to the model ([`fitted`]): the stored PNG is never
//! changed; a copy no longer than the provider's long-edge limit
//! ([`long_edge`]: Claude 2,576 px, OpenAI 2,048, Gemini 3,072, Groq and
//! OpenRouter 2,048, a model on this PC 1,024), area-averaged, re-encoded
//! as PNG (smaller again if it's over 4 MB, so a request stays under the
//! providers' per-image and per-request caps), as a `data:` URL — made in
//! the background while the person types ([`prepare`]) and kept in memory,
//! so Send is instant.
//!
//! Never logged (sizes and counts at most), never uploaded except inside
//! the Chat request the person sends.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use base64::Engine;

/// The folder under the data dir.
pub const FOLDER: &str = "chat-images";
/// An image nothing refers to (a snip never sent) is kept this long.
pub const ORPHAN_SECS: u64 = 24 * 60 * 60;
/// A copy for a model bigger than this is made smaller (a provider may
/// take less: [`fit_for`]).
pub const MAX_SEND_BYTES: usize = 4 * 1024 * 1024;
/// Images per request, at most (Groq: 3).
pub const MAX_PER_REQUEST: usize = 4;
/// The retention choices, in days (0 = forever).
pub const KEEP_CHOICES: [u32; 4] = [7, 30, 90, 0];
/// The default: 30 days.
pub const KEEP_DEFAULT: u32 = 30;

/// One stored image's facts (what chats.json keeps).
#[derive(Debug, Clone, PartialEq)]
pub struct Saved {
    pub id: String,
    pub w: u32,
    pub h: u32,
    pub bytes: u64,
}

/// Where the images live.
pub fn dir() -> PathBuf {
    #[cfg(test)]
    {
        test_dir()
    }
    #[cfg(not(test))]
    {
        crate::config::data_dir().join(FOLDER)
    }
}

#[cfg(test)]
thread_local! {
    static TEST_DIR: PathBuf = {
        let mut raw = [0u8; 8];
        let _ = getrandom::fill(&mut raw);
        let name: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        std::env::temp_dir().join(format!("yap-snip-test-{name}")).join(FOLDER)
    };
}

#[cfg(test)]
fn test_dir() -> PathBuf {
    TEST_DIR.with(|d| d.clone())
}

/// A new image id: `img_` + 16 random hex digits.
pub fn new_id() -> String {
    let mut raw = [0u8; 8];
    if getrandom::fill(&mut raw).is_err() {
        // No randomness (never seen): the clock's nanoseconds.
        let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        raw = (n as u64).to_le_bytes();
    }
    format!("img_{}", raw.iter().map(|b| format!("{b:02x}")).collect::<String>())
}

/// Whether `id` is an image id (and nothing else: no path can hide in one).
pub fn is_id(id: &str) -> bool {
    id.len() == 20 && id.starts_with("img_") && id[4..].bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// An image's file, when `id` is an id.
pub fn path(id: &str) -> Option<PathBuf> {
    is_id(id).then(|| dir().join(format!("{id}.png")))
}

/// RGBA → PNG. `fast`: quick compression (a snip's own file is written
/// while you wait); otherwise the balanced setting (a copy for a model).
pub fn encode_png(rgba: &[u8], w: u32, h: u32, fast: bool) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(if fast { png::Compression::Fast } else { png::Compression::Balanced });
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(rgba).map_err(|e| e.to_string())?;
        writer.finish().map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// A PNG → RGBA8 and its size (8-bit RGB, RGBA, grey and palette images;
/// 16-bit ones are stripped to 8).
pub fn decode_png(bytes: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("an image too big to read")?];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (w, h) = (info.width, info.height);
    let px = (w * h) as usize;
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..px * 4].to_vec(),
        png::ColorType::Rgb => buf[..px * 3].as_chunks::<3>().0.iter().flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf[..px * 2].as_chunks::<2>().0.iter().flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => buf[..px].iter().flat_map(|&g| [g, g, g, 255]).collect(),
        other => return Err(format!("unexpected colour type {other:?}")),
    };
    Ok((rgba, w, h))
}

/// Write `png` as a new image in `dir` (atomically). Returns its facts.
pub fn save_in(dir: &Path, png: &[u8], w: u32, h: u32) -> Result<Saved, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("couldn't make the screenshots folder ({e})"))?;
    let id = new_id();
    let file = dir.join(format!("{id}.png"));
    let tmp = dir.join(format!("{id}.tmp"));
    std::fs::write(&tmp, png).map_err(|e| format!("couldn't save the screenshot ({e})"))?;
    std::fs::rename(&tmp, &file).map_err(|e| format!("couldn't save the screenshot ({e})"))?;
    Ok(Saved { id, w, h, bytes: png.len() as u64 })
}

/// [`save_in`] the images folder.
pub fn save(png: &[u8], w: u32, h: u32) -> Result<Saved, String> {
    save_in(&dir(), png, w, h)
}

/// An image's PNG bytes (`None`: no such image, or it was removed).
pub fn read(id: &str) -> Option<Vec<u8>> {
    std::fs::read(path(id)?).ok()
}

/// Whether an image's file is still there.
pub fn exists(id: &str) -> bool {
    path(id).is_some_and(|p| p.is_file())
}

/// A PNG's size from its header. Pure.
pub fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 || &bytes[..8] != b"\x89PNG\r\n\x1a\n" || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let be = |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    Some((be(16), be(20)))
}

/// A stored image's facts, read from its file (a question's screenshots
/// are saved with what's really there, whatever a page says).
pub fn stored(id: &str) -> Option<crate::chat::chats::ChatImage> {
    let p = path(id)?;
    let bytes = std::fs::metadata(&p).ok()?.len();
    let mut head = [0u8; 24];
    use std::io::Read;
    std::fs::File::open(&p).ok()?.read_exact(&mut head).ok()?;
    let (w, h) = png_size(&head)?;
    Some(crate::chat::chats::ChatImage { id: id.to_string(), w, h, bytes, source: "snip".into(), ..Default::default() })
}

/// Delete images by id (a deleted conversation's, or all of them from
/// Settings). Returns how many went.
pub fn delete(ids: &[String]) -> usize {
    let mut gone = 0;
    for id in ids {
        if let Some(p) = path(id) {
            if std::fs::remove_file(&p).is_ok() {
                gone += 1;
            }
        }
        forget(id);
    }
    if gone > 0 {
        tracing::debug!(images = gone, "snip: chat screenshots deleted");
    }
    gone
}

/// What [`sweep`] decides for one file. Pure.
pub fn sweep_verdict(referenced: bool, age_secs: u64, keep_days: u32) -> bool {
    if !referenced {
        return age_secs > ORPHAN_SECS;
    }
    keep_days > 0 && age_secs > keep_days as u64 * 24 * 60 * 60
}

/// Delete what's past its time in `dir`: images older than `keep_days`
/// (0 = never), and images no conversation refers to after a day; leftover
/// temp files too. `now` in unix seconds; a file's age is from when it
/// was written. Returns how many went.
pub fn sweep_in(dir: &Path, referenced: &HashSet<String>, keep_days: u32, now: u64) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    let mut gone = 0;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some((id, ext)) = name.rsplit_once('.') else { continue };
        if !is_id(id) || !(ext == "png" || ext == "tmp") {
            continue;
        }
        let written = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(now);
        let age = now.saturating_sub(written);
        let stale_tmp = ext == "tmp" && age > 60 * 60;
        if stale_tmp || (ext == "png" && sweep_verdict(referenced.contains(id), age, keep_days)) {
            if std::fs::remove_file(entry.path()).is_ok() {
                gone += 1;
            }
            forget(id);
        }
    }
    gone
}

/// "Keep chat screenshots" as set, or the default for a value Settings
/// never offers (a hand-edited config). Pure.
pub fn keep_days(setting: u32) -> u32 {
    if KEEP_CHOICES.contains(&setting) {
        setting
    } else {
        KEEP_DEFAULT
    }
}

/// [`sweep_in`] the images folder, against every conversation's images.
pub fn sweep(keep_days: u32) -> usize {
    // chats.json Yap couldn't read: no conversation names any of them yet,
    // so nothing is swept until it reads.
    if crate::chat::chats::unread() {
        return 0;
    }
    let keep_days = self::keep_days(keep_days);
    // A conversation's, or one still waiting in Chat's box (a snip left
    // unsent for over a day while Yap ran on: never swept from under it).
    let mut referenced = crate::chat::chats::image_ids();
    referenced.extend(super::waiting::now().images.into_iter().map(|i| i.id));
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let gone = sweep_in(&dir(), &referenced, keep_days, now);
    if gone > 0 {
        tracing::info!(images = gone, keep_days, "snip: old chat screenshots removed");
    }
    gone
}

/// The images folder's count and size (Settings shows them).
pub fn usage() -> (usize, u64) {
    let Ok(entries) = std::fs::read_dir(dir()) else { return (0, 0) };
    entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".png"))
        .filter_map(|e| e.metadata().ok())
        .fold((0, 0), |(n, b), m| (n + 1, b + m.len()))
}

// ---- the copy a model gets ----------------------------------------------------------------

/// The longest side an image is sent at, for a provider. Pure.
pub fn long_edge(provider: &str, on_this_pc: bool) -> u32 {
    if on_this_pc {
        return 1024;
    }
    match provider {
        "anthropic" => 2576,
        "gemini" => 3072,
        _ => 2048,
    }
}

/// How a provider takes images: each copy at most `edge` px on its long
/// side and `max_bytes` (base64 adds a third: Groq refuses a base64 image
/// over 4 MB, Anthropic one over 5 MB), and a request's images together at
/// most `request_bytes` — Groq answered 413 "Request Entity Too Large" to
/// a question re-sending three 2,000 px game screenshots (25 Oct 2026).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fit {
    pub edge: u32,
    pub max_bytes: usize,
    pub request_bytes: usize,
}

/// [`Fit`] for a provider. Pure.
pub fn fit_for(provider: &str, on_this_pc: bool) -> Fit {
    let edge = long_edge(provider, on_this_pc);
    let (max_bytes, request_bytes) = if on_this_pc {
        (MAX_SEND_BYTES, 2 * MAX_SEND_BYTES)
    } else {
        match provider {
            "groq" => (2_800_000, 4_500_000),
            "anthropic" => (3_600_000, 18_000_000),
            "gemini" => (MAX_SEND_BYTES, 13_000_000),
            _ => (MAX_SEND_BYTES, 12_000_000),
        }
    };
    Fit { edge, max_bytes, request_bytes }
}

impl Fit {
    /// Each of `n` images' cap, so all `n` fit in one request.
    pub fn cap_for(self, n: usize) -> usize {
        self.max_bytes.min(self.request_bytes / n.max(1))
    }
}

/// How many images one request may carry for a provider (Groq's
/// `qwen/qwen3.8-27b` takes 3). Pure.
pub fn per_request(provider: &str) -> usize {
    if provider == "groq" {
        3
    } else {
        MAX_PER_REQUEST
    }
}

/// The size an image is sent at: its own if it fits, else scaled so its
/// long side is `edge` (never upscaled, never under 1 px). Pure.
pub fn fit(w: u32, h: u32, edge: u32) -> (u32, u32) {
    let long = w.max(h);
    if long <= edge || edge == 0 {
        return (w, h);
    }
    let s = edge as f64 / long as f64;
    (((w as f64 * s).round() as u32).max(1), ((h as f64 * s).round() as u32).max(1))
}

/// Each output pixel's share of the input pixels it covers (area
/// averaging), for one axis.
fn coverage(src: usize, dst: usize) -> Vec<Vec<(usize, f32)>> {
    let scale = src as f64 / dst as f64;
    (0..dst)
        .map(|o| {
            let a = o as f64 * scale;
            let b = a + scale;
            let first = a.floor() as usize;
            let last = (b.ceil() as usize).min(src);
            (first..last)
                .filter_map(|i| {
                    let w = (b.min(i as f64 + 1.0) - a.max(i as f64)) / scale;
                    (w > 0.0).then_some((i, w as f32))
                })
                .collect()
        })
        .collect()
}

/// RGBA8 made smaller by area averaging (every input pixel counts: text
/// stays legible, nothing aliases). Pure.
pub fn shrink(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    let (sw, sh, dw, dh) = (sw as usize, sh as usize, dw.max(1) as usize, dh.max(1) as usize);
    if (sw, sh) == (dw, dh) {
        return src.to_vec();
    }
    let across = coverage(sw, dw);
    let down = coverage(sh, dh);
    // Across first, into floats.
    let mut mid = vec![0f32; dw * sh * 4];
    for y in 0..sh {
        let row = &src[y * sw * 4..(y + 1) * sw * 4];
        for (x, parts) in across.iter().enumerate() {
            let mut acc = [0f32; 4];
            for &(i, w) in parts {
                for c in 0..4 {
                    acc[c] += row[i * 4 + c] as f32 * w;
                }
            }
            mid[(y * dw + x) * 4..(y * dw + x) * 4 + 4].copy_from_slice(&acc);
        }
    }
    let mut out = vec![0u8; dw * dh * 4];
    for (y, parts) in down.iter().enumerate() {
        for x in 0..dw {
            let mut acc = [0f32; 4];
            for &(j, w) in parts {
                for c in 0..4 {
                    acc[c] += mid[(j * dw + x) * 4 + c] * w;
                }
            }
            for c in 0..4 {
                out[(y * dw + x) * 4 + c] = (acc[c] + 0.5).clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

/// An image as a model gets it.
#[derive(Debug, Clone)]
pub struct Fitted {
    /// `data:image/png;base64,…`
    pub url: Arc<String>,
    pub w: u32,
    pub h: u32,
    pub bytes: usize,
}

/// A copy's key: the image's id, the long edge and the byte cap.
type FitKey = (String, u32, usize);

/// The copies made, by [`FitKey`]: the last few, in memory.
static FITTED: Mutex<Vec<(FitKey, Fitted)>> = Mutex::new(Vec::new());
const FITTED_KEPT: usize = 12;

fn forget(id: &str) {
    if let Ok(mut f) = FITTED.lock() {
        f.retain(|((i, _, _), _)| i != id);
    }
}

/// `png` fitted to `edge` and under `cap` bytes. Pure (but slow: decode,
/// area average, encode).
pub fn fit_png(png: &[u8], edge: u32, cap: usize) -> Result<Fitted, String> {
    let (rgba, w, h) = decode_png(png)?;
    let (mut fw, mut fh) = fit(w, h, edge);
    if (fw, fh) == (w, h) && png.len() <= cap {
        let url = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png));
        return Ok(Fitted { url: Arc::new(url), w, h, bytes: png.len() });
    }
    // Smaller until it's under the cap (a photo-heavy screenshot).
    for _ in 0..8 {
        let small = shrink(&rgba, w, h, fw, fh);
        let encoded = encode_png(&small, fw, fh, false)?;
        if encoded.len() <= cap || fw.max(fh) <= 512 {
            let url = format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(&encoded));
            return Ok(Fitted { url: Arc::new(url), w: fw, h: fh, bytes: encoded.len() });
        }
        (fw, fh) = (((fw as f64) * 0.75) as u32, ((fh as f64) * 0.75) as u32);
    }
    Err("the screenshot is too big to send".into())
}

/// The image `id` as a model with long edge `edge` gets it, under `cap`
/// bytes: from memory when [`prepare`] made it already, else made now.
pub fn fitted(id: &str, edge: u32, cap: usize) -> Result<Fitted, String> {
    let key = (id.to_string(), edge, cap);
    if let Some(f) = FITTED.lock().ok().and_then(|f| f.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone())) {
        return Ok(f);
    }
    let png = read(id).ok_or("That screenshot isn't there any more")?;
    let made = fit_png(&png, edge, cap)?;
    if let Ok(mut f) = FITTED.lock() {
        f.retain(|(k, _)| *k != key);
        if f.len() >= FITTED_KEPT {
            f.remove(0);
        }
        f.push((key, made.clone()));
    }
    Ok(made)
}

/// Make the copy for `fit` in the background (the snip landed in Chat, or
/// the model changed): Send then finds it ready (one image a question: its
/// own cap).
pub fn prepare(ids: Vec<String>, fit: Fit) {
    std::thread::Builder::new()
        .name("snip-fit".into())
        .spawn(move || {
            for id in ids {
                let t = std::time::Instant::now();
                match fitted(&id, fit.edge, fit.max_bytes) {
                    Ok(f) => tracing::debug!(w = f.w, h = f.h, bytes = f.bytes, ms = t.elapsed().as_millis() as u64, "snip: copy for the model ready"),
                    Err(e) => tracing::debug!("snip: no copy for the model ({e})"),
                }
            }
        })
        .ok();
}

/// The fitted copies a test made (their sizes), by id: what a request
/// would carry. Debug and test only.
#[allow(dead_code)]
pub fn fitted_sizes() -> HashMap<String, (u32, u32)> {
    FITTED.lock().map(|f| f.iter().map(|((id, _, _), v)| (id.clone(), (v.w, v.h))).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let v = if (x / 2 + y / 2) % 2 == 0 { 255 } else { 0 };
                out.extend_from_slice(&[v, v / 2, 255 - v, 255]);
            }
        }
        out
    }

    #[test]
    fn ids_are_ids_and_never_paths() {
        let id = new_id();
        assert!(is_id(&id), "{id}");
        assert_ne!(id, new_id());
        for bad in ["", "img_", "img_0123456789abcdeg", "img_0123456789ABCDEF", "../config.json", "img_0123456789abcdef/../x", "img_0123456789abcde"] {
            assert!(!is_id(bad), "{bad}");
            assert!(path(bad).is_none());
        }
        assert!(path("img_0123456789abcdef").unwrap().ends_with("img_0123456789abcdef.png"));
    }

    #[test]
    fn a_png_round_trips() {
        let px = checker(7, 5);
        let png = encode_png(&px, 7, 5, true).unwrap();
        assert_eq!(decode_png(&png).unwrap(), (px.clone(), 7, 5));
        let png = encode_png(&px, 7, 5, false).unwrap();
        assert_eq!(decode_png(&png).unwrap().0, px);
    }

    #[test]
    fn each_provider_gets_its_size() {
        assert_eq!(long_edge("anthropic", false), 2576);
        assert_eq!(long_edge("openai", false), 2048);
        assert_eq!(long_edge("gemini", false), 3072);
        assert_eq!(long_edge("groq", false), 2048);
        assert_eq!(long_edge("openrouter", false), 2048);
        assert_eq!(long_edge("custom", true), 1024);
        assert_eq!(long_edge("ondevice", true), 1024);
        assert_eq!((per_request("groq"), per_request("anthropic")), (3, 4));
        // Never upscaled; the long side to the limit, the other in proportion.
        assert_eq!(fit(800, 600, 2048), (800, 600));
        assert_eq!(fit(3840, 2160, 2048), (2048, 1152));
        assert_eq!(fit(1440, 5120, 2576), (725, 2576));
        assert_eq!(fit(9000, 3, 1024), (1024, 1));
    }

    #[test]
    fn shrinking_averages_every_pixel() {
        // A 2×2 checker of black and white halves to mid grey.
        let src = [0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255];
        assert_eq!(shrink(&src, 2, 2, 1, 1), vec![128, 128, 128, 255]);
        // A flat colour stays itself at any ratio.
        let flat: Vec<u8> = std::iter::repeat_n([12u8, 200, 77, 255], 7 * 5).flatten().collect();
        let out = shrink(&flat, 7, 5, 3, 2);
        assert!(out.as_chunks::<4>().0.iter().all(|p| *p == [12, 200, 77, 255]), "{out:?}");
        assert_eq!(shrink(&flat, 7, 5, 7, 5), flat);
    }

    #[test]
    fn a_big_screenshot_is_fitted_and_a_small_one_sent_as_it_is() {
        let px = checker(300, 120);
        let png = encode_png(&px, 300, 120, true).unwrap();
        let same = fit_png(&png, 2048, MAX_SEND_BYTES).unwrap();
        assert_eq!((same.w, same.h, same.bytes), (300, 120, png.len()));
        assert!(same.url.starts_with("data:image/png;base64,iVBORw0KGgo"));
        let small = fit_png(&png, 100, MAX_SEND_BYTES).unwrap();
        assert_eq!((small.w, small.h), (100, 40));
        let (_, w, h) = decode_png(
            &base64::engine::general_purpose::STANDARD.decode(small.url.trim_start_matches("data:image/png;base64,")).unwrap(),
        )
        .unwrap();
        assert_eq!((w, h), (100, 40));
    }

    #[test]
    fn screenshots_are_kept_their_time() {
        let day = 24 * 60 * 60;
        // In a conversation: kept for the setting; forever is forever.
        assert!(!sweep_verdict(true, 29 * day, 30));
        assert!(sweep_verdict(true, 31 * day, 30));
        assert!(sweep_verdict(true, 8 * day, 7));
        assert!(!sweep_verdict(true, 400 * day, 0));
        // In no conversation (a snip never sent): a day.
        assert!(!sweep_verdict(false, day - 10, 0));
        assert!(sweep_verdict(false, day + 10, 0));
        // Settings' four choices; anything else is the default, 30 days.
        assert_eq!(KEEP_CHOICES.map(keep_days), [7, 30, 90, 0]);
        assert_eq!(keep_days(45), 30);
        assert_eq!(crate::config::YapConfig::default().chat_image_days, 30);
    }

    #[test]
    fn the_sweep_deletes_only_what_is_past_its_time() {
        let dir = dir();
        let keep = save_in(&dir, &encode_png(&checker(4, 4), 4, 4, true).unwrap(), 4, 4).unwrap();
        let orphan = save_in(&dir, &encode_png(&checker(4, 4), 4, 4, true).unwrap(), 4, 4).unwrap();
        std::fs::write(dir.join("notes.json"), "{}").unwrap();
        let referenced: HashSet<String> = [keep.id.clone()].into();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        // Today: nothing goes.
        assert_eq!(sweep_in(&dir, &referenced, 30, now), 0);
        // Two days on: the orphan; 31 days on, the kept one too — but not
        // with "forever", and never a file that isn't an image.
        assert_eq!(sweep_in(&dir, &referenced, 0, now + 2 * 86_400), 1);
        assert!(dir.join(format!("{}.png", keep.id)).is_file());
        assert!(!dir.join(format!("{}.png", orphan.id)).exists());
        assert_eq!(sweep_in(&dir, &referenced, 0, now + 400 * 86_400), 0);
        assert_eq!(sweep_in(&dir, &referenced, 30, now + 31 * 86_400), 1);
        assert!(dir.join("notes.json").is_file());
        let _ = std::fs::remove_dir_all(dir.parent().unwrap_or(&dir));
    }

    #[test]
    fn deleting_a_conversation_deletes_its_images() {
        let saved = save(&encode_png(&checker(4, 4), 4, 4, true).unwrap(), 4, 4).unwrap();
        assert!(exists(&saved.id));
        assert_eq!(read(&saved.id).map(|b| b.len() as u64), Some(saved.bytes));
        assert_eq!(delete(std::slice::from_ref(&saved.id)), 1);
        assert!(!exists(&saved.id));
        assert_eq!(delete(&["../config".to_string()]), 0);
        let _ = std::fs::remove_dir_all(dir().parent().unwrap_or(&dir()));
    }
}

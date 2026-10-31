//! **Chat's thumbnails** (31 Oct 2026): a screenshot made smaller here, at
//! exactly the device pixels a page draws it at — the message box's tile, a
//! question's picture, the viewer when the picture is bigger than the
//! window — so the browser never scales it. Its own scaling of a 4K PNG
//! into a 120 px tile is soft (bilinear between mipmaps, and a picture at a
//! fraction of a pixel): `e2e/chat-shots.spec.js` measures both against the
//! ideal; area averaging ([`images::shrink`]: every source pixel counts,
//! nothing aliases) at the drawn size is that ideal.
//!
//! **Asked for** by the page through the `yapsnip` scheme:
//! `http://yapsnip.localhost/<id>?w=<px>&h=<px>` (`chatImages.js`
//! `placed`), the size it worked out from the image's own size, the box
//! and `devicePixelRatio`; the picture's own size is the stored PNG itself.
//! A size that isn't the picture's shape, or bigger than it, is refused
//! ([`valid`]): the scheme then serves the PNG as it is.
//!
//! **Kept** beside the image, `chat-images/thumbs/<id>-<w>x<h>.png`, a few
//! sizes each ([`PER_IMAGE`]: another monitor's scaling, a resized window;
//! the least recently used goes), and **deleted with it**:
//! [`images::delete`] (a conversation deleted, Settings' Delete all), the
//! daily sweep (a thumbnail whose image is gone goes too). Settings' folder
//! size counts the images only (thumbnails are a few KB).
//!
//! **Made ahead** for the box: as a snip, a paste or a drop lands
//! ([`prepare_tiles`], `waiting.rs`), the tile's size at the main window's
//! scale, so it's there before the page asks. Anything else is made the
//! first time it's asked for, off the main thread (the scheme answers
//! asynchronously, `lib.rs`), one at a time ([`MAKING`]: the page asking
//! for the size being made ahead waits for it rather than making it twice).
//!
//! Never logged beyond sizes and timings.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::images;

/// The folder under the images' folder.
pub const SUBFOLDER: &str = "thumbs";
/// Sizes kept per image.
pub const PER_IMAGE: usize = 6;
/// The longest side a thumbnail may have (the viewer on a big monitor).
pub const MAX_EDGE: u32 = 8192;
/// The message box's tile, in CSS pixels (`chatImages.js` `TILE`: the Claude
/// app's 7.5rem).
pub const TILE_CSS: f64 = 120.0;

/// One thumbnail made at a time.
static MAKING: Mutex<()> = Mutex::new(());

/// The thumbnails' folder for an images folder.
pub fn dir_in(images_dir: &Path) -> PathBuf {
    images_dir.join(SUBFOLDER)
}

/// A thumbnail's file. `None` unless `id` is an image id.
pub fn file_in(images_dir: &Path, id: &str, w: u32, h: u32) -> Option<PathBuf> {
    images::is_id(id).then(|| dir_in(images_dir).join(format!("{id}-{w}x{h}.png")))
}

/// `w` × `h` fitted inside `box_w` × `box_h`, its shape kept, never made
/// bigger, never under a pixel. Pure; the page's `contain` (chatImages.js)
/// is the same sum.
pub fn contain(w: u32, h: u32, box_w: u32, box_h: u32) -> (u32, u32) {
    if w == 0 || h == 0 {
        return (w.max(1), h.max(1));
    }
    let s = (box_w as f64 / w as f64).min(box_h as f64 / h as f64).min(1.0);
    (((w as f64 * s).round() as u32).max(1), ((h as f64 * s).round() as u32).max(1))
}

/// The box's tile for a `w` × `h` picture at `scale` device pixels per CSS
/// pixel: the picture contained in the 120 px square, in device pixels.
/// Pure (as the page's `placed` works it out).
pub fn tile_size(w: u32, h: u32, scale: f64) -> (u32, u32) {
    let side = (TILE_CSS * scale).round().max(1.0) as u32;
    contain(w, h, side, side)
}

/// Whether `w` × `h` is a thumbnail of a `iw` × `ih` picture: smaller (or
/// the same), its shape within a pixel's rounding, not absurd. Pure.
pub fn valid(w: u32, h: u32, iw: u32, ih: u32) -> bool {
    if w == 0 || h == 0 || iw == 0 || ih == 0 || w > iw || h > ih || w.max(h) > MAX_EDGE {
        return false;
    }
    // The height its width gives, and the width its height gives: one of
    // them within a pixel (rounding either way).
    let from_w = ih as f64 * w as f64 / iw as f64;
    let from_h = iw as f64 * h as f64 / ih as f64;
    (from_w - h as f64).abs() <= 1.0 || (from_h - w as f64).abs() <= 1.0
}

/// A thumbnail's PNG, made (and kept) if it isn't yet. The picture's own
/// size is its own PNG. `Err`: no such image, a size that isn't one of
/// its thumbnails, or it couldn't be made.
pub fn get_in(images_dir: &Path, id: &str, w: u32, h: u32) -> Result<Vec<u8>, String> {
    if !images::is_id(id) {
        return Err("not an image id".into());
    }
    let original = images_dir.join(format!("{id}.png"));
    let (iw, ih) = size_of(&original).ok_or("no such image")?;
    if (w, h) == (iw, ih) {
        return std::fs::read(&original).map_err(|e| e.to_string());
    }
    if !valid(w, h, iw, ih) {
        return Err(format!("{w}x{h} isn't a thumbnail of a {iw}x{ih} picture"));
    }
    let file = file_in(images_dir, id, w, h).ok_or("not an image id")?;
    if let Some(bytes) = kept(&file) {
        return Ok(bytes);
    }
    let _one = MAKING.lock().unwrap_or_else(|e| e.into_inner());
    // Made while this waited (the box's tile, made ahead)?
    if let Some(bytes) = kept(&file) {
        return Ok(bytes);
    }
    let t = std::time::Instant::now();
    let png = std::fs::read(&original).map_err(|e| e.to_string())?;
    let (rgba, iw, ih) = images::decode_png(&png)?;
    let small = images::shrink(&rgba, iw, ih, w, h);
    let bytes = images::encode_png(&small, w, h, true)?;
    let dir = dir_in(images_dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{id}-{w}x{h}.tmp"));
    std::fs::write(&tmp, &bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &file).map_err(|e| e.to_string())?;
    prune(&dir, id, &file);
    tracing::debug!(w, h, from_w = iw, from_h = ih, bytes = bytes.len(), ms = t.elapsed().as_millis() as u64, "snip: thumbnail made");
    Ok(bytes)
}

/// [`get_in`] the images folder.
pub fn get(id: &str, w: u32, h: u32) -> Result<Vec<u8>, String> {
    get_in(&images::dir(), id, w, h)
}

/// A PNG file's size, from its header.
fn size_of(file: &Path) -> Option<(u32, u32)> {
    use std::io::Read;
    let mut head = [0u8; 24];
    std::fs::File::open(file).ok()?.read_exact(&mut head).ok()?;
    images::png_size(&head)
}

/// A kept thumbnail's bytes, marked used now (the least recently used
/// goes first: [`prune`]).
fn kept(file: &Path) -> Option<Vec<u8>> {
    let bytes = std::fs::read(file).ok()?;
    if let Ok(f) = std::fs::File::options().write(true).open(file) {
        let _ = f.set_modified(std::time::SystemTime::now());
    }
    Some(bytes)
}

/// An image's thumbnails in `dir` (its files, `<id>-<w>x<h>.png`).
fn files_of(dir: &Path, id: &str) -> Vec<PathBuf> {
    let prefix = format!("{id}-");
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(&prefix)))
                .collect()
        })
        .unwrap_or_default()
}

/// Keep [`PER_IMAGE`] of an image's thumbnails: the least recently used
/// go (never `keep`, just made).
fn prune(dir: &Path, id: &str, keep: &Path) {
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = files_of(dir, id)
        .into_iter()
        .filter(|p| p.extension().is_some_and(|x| x == "png") && p != keep)
        .map(|p| (std::fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), p))
        .collect();
    let over = (files.len() + 1).saturating_sub(PER_IMAGE);
    files.sort();
    for (_, p) in files.into_iter().take(over) {
        let _ = std::fs::remove_file(p);
    }
}

/// Delete an image's thumbnails (it was deleted). Returns how many went.
pub fn remove_in(images_dir: &Path, id: &str) -> usize {
    if !images::is_id(id) {
        return 0;
    }
    files_of(&dir_in(images_dir), id).into_iter().filter(|p| std::fs::remove_file(p).is_ok()).count()
}

/// The sweep's part: thumbnails whose image is gone, and leftover temp
/// files. Returns how many went.
pub fn sweep_in(images_dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir_in(images_dir)) else { return 0 };
    let mut gone = 0;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let id = name.split_once('-').map(|(id, _)| id).unwrap_or(&name);
        let orphan = !images::is_id(id) || !images_dir.join(format!("{id}.png")).is_file();
        let stale_tmp = name.ends_with(".tmp")
            && entry.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age.as_secs() > 60 * 60);
        if (orphan || stale_tmp) && std::fs::remove_file(entry.path()).is_ok() {
            gone += 1;
        }
    }
    gone
}

/// The box's tiles for pictures just landed, made now (off this thread) at
/// the main window's scale, so they're there when the page asks.
pub fn prepare_tiles(app: &tauri::AppHandle, images: &[crate::chat::chats::ChatImage]) {
    use tauri::Manager;
    let scale = app.get_webview_window("settings").and_then(|w| w.scale_factor().ok()).unwrap_or(1.0);
    let wanted: Vec<(String, u32, u32)> = images
        .iter()
        .filter(|i| i.w > 0 && i.h > 0)
        .map(|i| {
            let (w, h) = tile_size(i.w, i.h, scale);
            (i.id.clone(), w, h)
        })
        .collect();
    if wanted.is_empty() {
        return;
    }
    std::thread::Builder::new()
        .name("snip-thumbs".into())
        .spawn(move || {
            for (id, w, h) in wanted {
                if let Err(e) = get(&id, w, h) {
                    tracing::debug!("snip: no tile thumbnail ({e})");
                }
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A picture whose every pixel says where it is (so a thumbnail of the
    /// wrong part would show).
    fn ramp(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                out.extend_from_slice(&[(x * 255 / (w - 1).max(1)) as u8, (y * 255 / (h - 1).max(1)) as u8, 90, 255]);
            }
        }
        out
    }

    fn fresh_dir() -> PathBuf {
        let mut raw = [0u8; 8];
        let _ = getrandom::fill(&mut raw);
        let name: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        std::env::temp_dir().join(format!("yap-thumbs-test-{name}")).join(images::FOLDER)
    }

    #[test]
    fn a_picture_is_contained_never_enlarged() {
        // A 16:9 screenshot in the 120 px tile: 120 wide at 100 %, 180 at 150 %.
        assert_eq!(contain(1920, 1080, 120, 120), (120, 68));
        assert_eq!(tile_size(1920, 1080, 1.0), (120, 68));
        assert_eq!(tile_size(1920, 1080, 1.5), (180, 101));
        assert_eq!(tile_size(1920, 1080, 1.25), (150, 84));
        assert_eq!(tile_size(1920, 1080, 2.0), (240, 135));
        // Tall: the height fills it.
        assert_eq!(tile_size(800, 2400, 1.0), (40, 120));
        // Smaller than the tile: as it is.
        assert_eq!(tile_size(64, 48, 1.5), (64, 48));
        // A sliver keeps a pixel.
        assert_eq!(contain(9000, 3, 120, 120), (120, 1));
        assert_eq!(contain(0, 0, 120, 120), (1, 1));
    }

    #[test]
    fn only_the_pictures_own_shape_and_smaller_is_a_thumbnail() {
        assert!(valid(180, 101, 1920, 1080));
        assert!(valid(180, 102, 1920, 1080), "rounded the other way");
        assert!(valid(1920, 1080, 1920, 1080));
        assert!(valid(120, 1, 9000, 3));
        assert!(!valid(180, 120, 1920, 1080), "not its shape");
        assert!(!valid(3840, 2160, 1920, 1080), "bigger");
        assert!(!valid(0, 10, 1920, 1080));
        assert!(!valid(10, 10, 0, 0));
    }

    #[test]
    fn a_thumbnail_is_made_at_its_size_kept_and_deleted_with_its_image() {
        let dir = fresh_dir();
        let px = ramp(300, 200);
        let saved = images::save_in(&dir, &images::encode_png(&px, 300, 200, true).unwrap(), 300, 200).unwrap();
        // Its own size: the PNG as it is.
        let same = get_in(&dir, &saved.id, 300, 200).unwrap();
        assert_eq!(same, std::fs::read(dir.join(format!("{}.png", saved.id))).unwrap());
        assert!(!dir_in(&dir).exists(), "no thumbnail for its own size");
        // A thumbnail: that size, area averaged from the whole picture, kept.
        let thumb = get_in(&dir, &saved.id, 150, 100).unwrap();
        let (rgba, w, h) = images::decode_png(&thumb).unwrap();
        assert_eq!((w, h), (150, 100));
        assert_eq!(rgba, images::shrink(&px, 300, 200, 150, 100));
        let file = file_in(&dir, &saved.id, 150, 100).unwrap();
        assert!(file.is_file());
        assert_eq!(std::fs::read(&file).unwrap(), thumb);
        // Asked again: the kept one.
        assert_eq!(get_in(&dir, &saved.id, 150, 100).unwrap(), thumb);
        // Not its shape, bigger, not an id, no such image: refused.
        assert!(get_in(&dir, &saved.id, 150, 150).is_err());
        assert!(get_in(&dir, &saved.id, 600, 400).is_err());
        assert!(get_in(&dir, "../config", 10, 10).is_err());
        assert!(get_in(&dir, "img_0123456789abcdef", 10, 10).is_err());
        // Deleted with its image: by id, and by the sweep once it's gone.
        get_in(&dir, &saved.id, 30, 20).unwrap();
        assert_eq!(remove_in(&dir, &saved.id), 2);
        assert!(!file.exists());
        get_in(&dir, &saved.id, 60, 40).unwrap();
        assert_eq!(sweep_in(&dir), 0, "its image is still there");
        std::fs::remove_file(dir.join(format!("{}.png", saved.id))).unwrap();
        assert_eq!(sweep_in(&dir), 1);
        assert!(files_of(&dir_in(&dir), &saved.id).is_empty());
        let _ = std::fs::remove_dir_all(dir.parent().unwrap_or(&dir));
    }

    #[test]
    fn an_image_keeps_a_few_sizes_the_least_used_going() {
        let dir = fresh_dir();
        let saved = images::save_in(&dir, &images::encode_png(&ramp(400, 400), 400, 400, true).unwrap(), 400, 400).unwrap();
        let other = images::save_in(&dir, &images::encode_png(&ramp(40, 40), 40, 40, true).unwrap(), 40, 40).unwrap();
        get_in(&dir, &other.id, 20, 20).unwrap();
        for side in 1..=PER_IMAGE as u32 + 2 {
            get_in(&dir, &saved.id, side * 10, side * 10).unwrap();
            // Distinct modification times, oldest first.
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        let left = files_of(&dir_in(&dir), &saved.id);
        assert_eq!(left.len(), PER_IMAGE, "{left:?}");
        assert!(!file_in(&dir, &saved.id, 10, 10).unwrap().exists(), "the oldest went");
        assert!(file_in(&dir, &saved.id, (PER_IMAGE as u32 + 2) * 10, (PER_IMAGE as u32 + 2) * 10).unwrap().exists());
        // Another image's thumbnail is never touched.
        assert!(file_in(&dir, &other.id, 20, 20).unwrap().exists());
        let _ = std::fs::remove_dir_all(dir.parent().unwrap_or(&dir));
    }
}

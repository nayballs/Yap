//! **Copy and save** a snip or a chat screenshot (phase 2).
//!
//! - **From the overlay**: Ctrl held as you let go (or Ctrl+C while the box
//!   is up) copies the picture instead of taking it to Chat; Shift (or
//!   Ctrl+S) saves it. Nothing goes to Chat, and nothing is kept in
//!   `chat-images/`.
//! - **From Chat**: a screenshot's right-click menu (in the message box, in
//!   a question, opened large): Copy image, Save image as…
//! - **Every snip to Chat** (2026-10-25, "Copy every snip to the
//!   clipboard", on): copied too, once Chat is up (`snip.rs`
//!   `hand_to_chat`), as Windows' Snipping Tool does — no alert, the box
//!   says it. Pictures pasted or dropped into Chat are never copied back.
//!
//! **Copy** puts the picture on the clipboard as a PNG and a 32-bit DIB
//! (`CF_DIBV5`: Windows makes the older bitmap formats from it, so every
//! app can paste it), with the formats that keep it out of Win+V history,
//! cloud clipboard sync and clipboard managers — exactly as a dictation's
//! paste (`pasting/text_injector.rs`). A test run never touches the real
//! clipboard: it records what would have gone there.
//!
//! **Save** asks where with Windows' Save dialog (the dialog plugin), in the
//! folder used last (`snip_save_dir`; at first the Pictures folder), named
//! "Snip 2026-10-25 at 14.03.12.png"; the folder chosen is remembered. A
//! test run names the file first (`e2e_snip_save_path`) and never opens the
//! dialog.
//!
//! Logs say sizes only — never a path, a name or a pixel.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tauri::AppHandle;

/// A 32-bit `BITMAPV5HEADER` DIB of an RGBA picture (bottom-up, BGRA, the
/// alpha kept, sRGB): what `CF_DIBV5` holds. Pure.
pub fn dibv5(rgba: &[u8], w: u32, h: u32) -> Vec<u8> {
    let (w, h) = (w as usize, h as usize);
    let size = w * h * 4;
    let mut b: Vec<u8> = Vec::with_capacity(124 + size);
    b.extend_from_slice(&124u32.to_le_bytes()); // bV5Size
    b.extend_from_slice(&(w as i32).to_le_bytes());
    b.extend_from_slice(&(h as i32).to_le_bytes()); // positive: bottom-up
    b.extend_from_slice(&1u16.to_le_bytes()); // planes
    b.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
    b.extend_from_slice(&3u32.to_le_bytes()); // BI_BITFIELDS
    b.extend_from_slice(&(size as u32).to_le_bytes());
    b.extend_from_slice(&2835i32.to_le_bytes()); // 72 dpi
    b.extend_from_slice(&2835i32.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes()); // colours used
    b.extend_from_slice(&0u32.to_le_bytes()); // colours important
    for mask in [0x00FF_0000u32, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000] {
        b.extend_from_slice(&mask.to_le_bytes());
    }
    b.extend_from_slice(&0x7352_4742u32.to_le_bytes()); // LCS_sRGB
    b.extend_from_slice(&[0u8; 36]); // endpoints
    b.extend_from_slice(&[0u8; 12]); // gamma
    b.extend_from_slice(&4u32.to_le_bytes()); // LCS_GM_IMAGES
    b.extend_from_slice(&[0u8; 12]); // profile data, size, reserved
    debug_assert_eq!(b.len(), 124);
    for row in (0..h).rev() {
        for px in rgba[row * w * 4..(row + 1) * w * 4].as_chunks::<4>().0 {
            b.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
    }
    b
}

/// What a test run's copy would have put on the clipboard: the PNG.
static TEST_CLIPBOARD: Mutex<Option<Vec<u8>>> = Mutex::new(None);

/// The PNG a test run's last copy would have put on the clipboard.
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub fn test_clipboard() -> Option<Vec<u8>> {
    TEST_CLIPBOARD.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

/// …taken (the pretend clipboard emptied).
#[cfg(debug_assertions)]
pub fn take_test_clipboard() -> Option<Vec<u8>> {
    TEST_CLIPBOARD.lock().unwrap_or_else(|p| p.into_inner()).take()
}

/// The PNG on the clipboard (the last one Yap copied), for the robot's
/// check: debug builds only.
#[cfg(debug_assertions)]
static LAST_COPIED: Mutex<Option<Vec<u8>>> = Mutex::new(None);

#[cfg(debug_assertions)]
pub fn last_copied() -> Option<Vec<u8>> {
    LAST_COPIED.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

/// Whether this run copies to a pretend clipboard (the UI suite; the robot
/// tests use the real one, saved and put back around them).
fn pretend() -> bool {
    crate::shell::e2e::active() && !crate::shell::realtest::on()
}

/// Copy a picture (its RGBA and its PNG), out of Win+V history.
pub fn copy(rgba: &[u8], w: u32, h: u32, png: &[u8]) -> Result<(), String> {
    if pretend() {
        *TEST_CLIPBOARD.lock().unwrap_or_else(|p| p.into_inner()) = Some(png.to_vec());
        return Ok(());
    }
    let dib = dibv5(rgba, w, h);
    if !crate::pasting::text_injector::copy_image_privately(png, &dib) {
        return Err("Windows didn't let Yap use the clipboard".into());
    }
    #[cfg(debug_assertions)]
    {
        *LAST_COPIED.lock().unwrap_or_else(|p| p.into_inner()) = Some(png.to_vec());
    }
    tracing::info!(w, h, kb = png.len() / 1024, "snip: copied (out of clipboard history)");
    Ok(())
}

/// Copy a stored chat screenshot.
pub fn copy_stored(id: &str) -> Result<(), String> {
    let png = super::images::read(id).ok_or("That screenshot isn't there any more")?;
    let (rgba, w, h) = super::images::decode_png(&png)?;
    copy(&rgba, w, h, &png)
}

/// The file name a save suggests: "Snip 2026-10-25 at 14.03.12.png". Pure.
pub fn file_name(at: chrono::NaiveDateTime) -> String {
    format!("Snip {}.png", at.format("%Y-%m-%d at %H.%M.%S"))
}

/// The folder a save starts in: the one used last, if it's still there,
/// else Pictures, else the home folder.
pub fn folder(remembered: &str) -> Option<PathBuf> {
    let r = remembered.trim();
    if !r.is_empty() && Path::new(r).is_dir() {
        return Some(PathBuf::from(r));
    }
    dirs::picture_dir().or_else(dirs::home_dir)
}

/// A test run's save path (`e2e_snip_save_path`).
static TEST_SAVE: Mutex<Option<PathBuf>> = Mutex::new(None);

#[cfg(debug_assertions)]
pub fn set_test_save_path(path: PathBuf) {
    *TEST_SAVE.lock().unwrap_or_else(|p| p.into_inner()) = Some(path);
}

/// Where to save: Windows' Save dialog (blocking: never on the main
/// thread), or a test run's path. `None`: cancelled.
fn ask_where(app: &AppHandle, parent: Option<&tauri::WebviewWindow>) -> Result<Option<PathBuf>, String> {
    if crate::shell::e2e::active() && !crate::shell::realtest::on() {
        return TEST_SAVE
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
            .map(Some)
            .ok_or_else(|| "A test run names the save path first (e2e_snip_save_path).".to_string());
    }
    use tauri_plugin_dialog::DialogExt;
    let cfg = crate::config::load();
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Save the screenshot")
        .set_file_name(file_name(chrono::Local::now().naive_local()))
        .add_filter("PNG image", &["png"]);
    if let Some(dir) = folder(&cfg.snip_save_dir) {
        dialog = dialog.set_directory(dir);
    }
    if let Some(parent) = parent {
        dialog = dialog.set_parent(parent);
    }
    Ok(dialog.blocking_save_file().and_then(|p| p.into_path().ok()))
}

/// Save a PNG where the person chooses; the folder remembered. `None`:
/// they cancelled.
pub fn save(app: &AppHandle, png: &[u8], parent: Option<&tauri::WebviewWindow>) -> Result<Option<PathBuf>, String> {
    let Some(mut path) = ask_where(app, parent)? else { return Ok(None) };
    if path.extension().is_none() {
        path.set_extension("png");
    }
    std::fs::write(&path, png).map_err(|e| format!("Couldn't save the screenshot ({e})"))?;
    if let Some(dir) = path.parent().map(|d| d.to_string_lossy().into_owned()) {
        if crate::config::load().snip_save_dir != dir {
            if let Err(e) = crate::config::update(|c| c.snip_save_dir = dir) {
                tracing::debug!("snip: couldn't remember the folder ({e})");
            }
        }
    }
    tracing::info!(kb = png.len() / 1024, "snip: saved");
    *LAST_SAVED.lock().unwrap_or_else(|p| p.into_inner()) = Some(path.clone());
    Ok(Some(path))
}

/// The file saved last ("Show in folder").
static LAST_SAVED: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Show the file saved last in File Explorer (the alert's button).
pub fn show_saved(app: &AppHandle) {
    let Some(path) = LAST_SAVED.lock().unwrap_or_else(|p| p.into_inner()).clone() else { return };
    if crate::shell::e2e::active() {
        return;
    }
    use tauri_plugin_opener::OpenerExt;
    if let Err(e) = app.opener().reveal_item_in_dir(&path) {
        tracing::debug!("snip: couldn't show the file ({e})");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dib_is_bottom_up_bgra_with_its_header() {
        // 2×2: red, green / blue, white (RGBA, top row first).
        let rgba = [255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255];
        let dib = dibv5(&rgba, 2, 2);
        assert_eq!(dib.len(), 124 + 16);
        assert_eq!(u32::from_le_bytes(dib[0..4].try_into().unwrap()), 124);
        assert_eq!(i32::from_le_bytes(dib[4..8].try_into().unwrap()), 2);
        assert_eq!(i32::from_le_bytes(dib[8..12].try_into().unwrap()), 2, "bottom-up");
        assert_eq!(u16::from_le_bytes(dib[14..16].try_into().unwrap()), 32);
        assert_eq!(u32::from_le_bytes(dib[16..20].try_into().unwrap()), 3, "BI_BITFIELDS");
        assert_eq!(u32::from_le_bytes(dib[56..60].try_into().unwrap()), 0x7352_4742, "sRGB");
        // The bottom row first: blue, white; then red, green — as BGRA.
        assert_eq!(&dib[124..], &[255, 0, 0, 255, 255, 255, 255, 255, 0, 0, 255, 255, 0, 255, 0, 255]);
    }

    #[test]
    fn a_save_is_named_by_when_and_starts_where_the_last_went() {
        let at = chrono::NaiveDate::from_ymd_opt(2026, 10, 25).unwrap().and_hms_opt(14, 3, 12).unwrap();
        assert_eq!(file_name(at), "Snip 2026-10-25 at 14.03.12.png");
        let here = std::env::temp_dir();
        assert_eq!(folder(&here.to_string_lossy()), Some(here));
        // A folder that's gone: Pictures (or home).
        assert_eq!(folder("Z:\\no\\such\\folder"), dirs::picture_dir().or_else(dirs::home_dir));
        assert_eq!(folder(""), dirs::picture_dir().or_else(dirs::home_dir));
    }
}

//! Insights as a picture (Insights → Share): the page draws the numbers
//! (`views/insights/shareCard.js`: numbers only, never a word said) and
//! hands the PNG here, in the call's body. **Copy** puts it on the
//! clipboard as Snip to Chat copies a picture (`snip/copy_save.rs`: PNG and
//! DIB, out of Win+V history; a test run's pretend clipboard); **Save…**
//! asks where with Windows' Save dialog, in Pictures, named "Yap stats
//! 2026-10-29.png". Logs say sizes only.

use std::path::PathBuf;

use tauri::ipc::{InvokeBody, Request};
use tauri::AppHandle;

/// Bigger than any card the page draws (1200 × 630).
const MAX_BYTES: usize = 8 * 1024 * 1024;

/// The PNG in the call's body.
fn png_of(request: &Request<'_>) -> Result<Vec<u8>, String> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err("No picture came with it".into());
    };
    if bytes.is_empty() || bytes.len() > MAX_BYTES || !bytes.starts_with(b"\x89PNG") {
        return Err("That isn't a picture Yap made".into());
    }
    Ok(bytes.clone())
}

/// The file name a save suggests. Pure.
pub fn file_name(day: chrono::NaiveDate) -> String {
    format!("Yap stats {}.png", day.format("%Y-%m-%d"))
}

/// Where to save: Windows' Save dialog (blocking), or in a test run the
/// data folder's `e2e-insights-card.png`. `None`: cancelled.
fn ask_where(app: &AppHandle) -> Option<PathBuf> {
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() && !crate::shell::realtest::on() {
        return Some(crate::config::data_dir().join("e2e-insights-card.png"));
    }
    use tauri::Manager;
    use tauri_plugin_dialog::DialogExt;
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Save your stats as a picture")
        .set_file_name(file_name(chrono::Local::now().date_naive()))
        .add_filter("PNG image", &["png"]);
    if let Some(dir) = crate::snip::copy_save::folder("") {
        dialog = dialog.set_directory(dir);
    }
    if let Some(parent) = app.get_webview_window("settings") {
        dialog = dialog.set_parent(&parent);
    }
    dialog.blocking_save_file().and_then(|p| p.into_path().ok())
}

/// Insights → Share → Copy: the picture on the clipboard.
#[tauri::command]
pub async fn insights_card_copy(request: Request<'_>) -> Result<(), String> {
    let png = png_of(&request)?;
    tauri::async_runtime::spawn_blocking(move || {
        let (rgba, w, h) = crate::snip::images::decode_png(&png)?;
        crate::snip::copy_save::copy(&rgba, w, h, &png)?;
        tracing::info!(w, h, kb = png.len() / 1024, "Insights: stats copied as a picture");
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Insights → Share → Save…: the picture where the person picks. `false`:
/// they cancelled.
#[tauri::command]
pub async fn insights_card_save(app: AppHandle, request: Request<'_>) -> Result<bool, String> {
    let png = png_of(&request)?;
    tauri::async_runtime::spawn_blocking(move || {
        let Some(mut path) = ask_where(&app) else { return Ok(false) };
        if path.extension().is_none() {
            path.set_extension("png");
        }
        std::fs::write(&path, &png).map_err(|e| format!("Couldn't save the picture ({e})"))?;
        tracing::info!(kb = png.len() / 1024, "Insights: stats saved as a picture");
        Ok(true)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_saved_picture_is_named_for_the_day() {
        let day = chrono::NaiveDate::from_ymd_opt(2026, 10, 29).unwrap();
        assert_eq!(file_name(day), "Yap stats 2026-10-29.png");
    }
}

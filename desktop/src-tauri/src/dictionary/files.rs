//! The Dictionary view's files: Import… reads a CSV or text file the person
//! picks, Export… writes one where they say. Windows' file dialogs are opened
//! here, never by the page, so no path crosses the IPC boundary and the
//! page can't name a file to read or write. Reading what's in the file
//! (CSV, `heard → written` lines, words), the preview and what's added are
//! the page's (`src/lib/views/dictionary/dictionaryFile.js`).
//!
//! A test run never opens a dialog: `e2e_dictionary_file` names the path
//! the next Import… reads or Export… writes. The log says sizes only, never
//! a file's name or its words.

use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, WebviewWindow};

/// A dictionary file bigger than this isn't one (2,000 entries of CSV are
/// ~100 KB): said instead of read.
const MAX_BYTES: u64 = 5 * 1024 * 1024;

/// What Import… read: the file's name (for the preview's title) and its text.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Picked {
    pub name: String,
    pub text: String,
}

/// Pick a file and read it. `None` when the person cancelled.
#[tauri::command]
pub async fn dictionary_import_pick(app: AppHandle, window: WebviewWindow) -> Result<Option<Picked>, String> {
    let Some(path) = pick(&app, window, Mode::Open).await? else { return Ok(None) };
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let bytes = tauri::async_runtime::spawn_blocking(move || read_capped(&path))
        .await
        .map_err(|e| e.to_string())??;
    tracing::info!(bytes = bytes.len(), "dictionary: a file read for import");
    Ok(Some(Picked { name, text: decode(&bytes) }))
}

/// Ask where to save `text` (`kind` "csv" or "txt", `name` the suggested
/// file name) and write it. The saved file's name, or `None` when the
/// person cancelled.
#[tauri::command]
pub async fn dictionary_export_save(
    app: AppHandle,
    window: WebviewWindow,
    text: String,
    name: String,
    kind: String,
) -> Result<Option<String>, String> {
    let mode = if kind == "txt" { Mode::SaveText(name) } else { Mode::SaveCsv(name) };
    let Some(path) = pick(&app, window, mode).await? else { return Ok(None) };
    let saved = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let len = text.len();
    tauri::async_runtime::spawn_blocking(move || std::fs::write(&path, text.as_bytes()))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("Couldn't save the file: {e}"))?;
    tracing::info!(bytes = len, "dictionary: exported");
    Ok(Some(saved))
}

enum Mode {
    Open,
    SaveCsv(String),
    SaveText(String),
}

/// The path the person picked in Windows' dialog (a test run: the one it named).
async fn pick(app: &AppHandle, window: WebviewWindow, mode: Mode) -> Result<Option<PathBuf>, String> {
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() {
        return E2E_FILE
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
            .map(Some)
            .ok_or_else(|| "A test run names the file first (e2e_dictionary_file).".to_string());
    }
    use tauri_plugin_dialog::DialogExt;
    let dialog = app.dialog().file().set_parent(&window);
    let picked = match mode {
        Mode::Open => {
            let dialog = dialog
                .set_title("Import words and snippets")
                .add_filter("Dictionary (CSV or text)", &["csv", "txt", "tsv"])
                .add_filter("All files", &["*"]);
            tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_file()).await
        }
        Mode::SaveCsv(name) => {
            let dialog = dialog
                .set_title("Export your dictionary")
                .set_file_name(name)
                .add_filter("CSV (Excel, Google Sheets)", &["csv"]);
            tauri::async_runtime::spawn_blocking(move || dialog.blocking_save_file()).await
        }
        Mode::SaveText(name) => {
            let dialog = dialog
                .set_title("Export your dictionary")
                .set_file_name(name)
                .add_filter("Text", &["txt"]);
            tauri::async_runtime::spawn_blocking(move || dialog.blocking_save_file()).await
        }
    }
    .map_err(|e| e.to_string())?;
    Ok(picked.and_then(|p| p.into_path().ok()))
}

fn read_capped(path: &std::path::Path) -> Result<Vec<u8>, String> {
    let size = std::fs::metadata(path).map_err(|e| format!("Couldn't read that file: {e}"))?.len();
    if size > MAX_BYTES {
        return Err("That file is too big to be a dictionary (over 5 MB).".into());
    }
    std::fs::read(path).map_err(|e| format!("Couldn't read that file: {e}"))
}

/// A file's text, whatever Windows wrote it as: UTF-8 (with or without its
/// BOM), UTF-16 with a BOM (Excel's and Notepad's "Unicode"), else Windows'
/// Western code page (Excel's plain "CSV" — Windows-1252), byte for byte.
pub fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, u16::from_le_bytes);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, u16::from_be_bytes);
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => bytes.iter().map(|&b| windows_1252(b)).collect(),
    }
}

fn utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> String {
    let (pairs, _odd) = bytes.as_chunks::<2>();
    let units: Vec<u16> = pairs.iter().map(|&pair| unit(pair)).collect();
    String::from_utf16_lossy(&units)
}

/// One Windows-1252 byte as its character (0x80–0x9F are its own; the rest
/// match Latin-1; its five unassigned bytes stay as their C1 codes).
fn windows_1252(b: u8) -> char {
    const HIGH: [u16; 32] = [
        0x20AC, 0x0081, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160, 0x2039, 0x0152, 0x008D,
        0x017D, 0x008F, 0x0090, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013, 0x2014, 0x02DC, 0x2122, 0x0161, 0x203A,
        0x0153, 0x009D, 0x017E, 0x0178,
    ];
    match b {
        0x80..=0x9F => char::from_u32(u32::from(HIGH[usize::from(b - 0x80)])).unwrap_or('\u{FFFD}'),
        _ => char::from(b),
    }
}

/// Test mode: the file the next Import… reads or Export… writes, in place
/// of the dialog ([`e2e_dictionary_file`]).
#[cfg(debug_assertions)]
static E2E_FILE: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Test mode: name the file the next Import… or Export… uses.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_dictionary_file(path: String) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("test mode only".into());
    }
    *E2E_FILE.lock().unwrap_or_else(|p| p.into_inner()) = Some(PathBuf::from(path));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_with_and_without_its_bom() {
        assert_eq!(decode("Kubernetes,Café\n".as_bytes()), "Kubernetes,Café\n");
        let mut bom = vec![0xEF, 0xBB, 0xBF];
        bom.extend_from_slice("power to keep → Parakeet".as_bytes());
        assert_eq!(decode(&bom), "power to keep → Parakeet");
    }

    #[test]
    fn utf16_both_ways_round() {
        let text = "jaison,JSON\r\nnaïve,Zoë";
        let le: Vec<u8> = [0xFF, 0xFE].into_iter().chain(text.encode_utf16().flat_map(|u| u.to_le_bytes())).collect();
        let be: Vec<u8> = [0xFE, 0xFF].into_iter().chain(text.encode_utf16().flat_map(|u| u.to_be_bytes())).collect();
        assert_eq!(decode(&le), text);
        assert_eq!(decode(&be), text);
    }

    #[test]
    fn excels_plain_csv_is_windows_1252() {
        // "Café,“Zentrix”…€" as Excel's ANSI CSV writes it.
        let bytes = [b'C', b'a', b'f', 0xE9, b',', 0x93, b'Z', b'e', b'n', b't', b'r', b'i', b'x', 0x94, 0x85, 0x80];
        assert_eq!(decode(&bytes), "Café,“Zentrix”…€");
    }

    #[test]
    fn a_file_too_big_is_said_not_read() {
        let dir = std::env::temp_dir().join(format!("yap-dict-files-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let small = dir.join("small.csv");
        std::fs::write(&small, "a,b").unwrap();
        assert_eq!(read_capped(&small).unwrap(), b"a,b");
        let big = dir.join("big.csv");
        let f = std::fs::File::create(&big).unwrap();
        f.set_len(MAX_BYTES + 1).unwrap();
        drop(f);
        assert!(read_capped(&big).unwrap_err().contains("too big"));
        assert!(read_capped(&dir.join("missing.csv")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

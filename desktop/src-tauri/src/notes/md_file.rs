//! **Markdown files open in Yap** (2026-11-07): a `.md` or `.markdown` file
//! — a plan, a note, a write-up an assistant made — in a window of its own,
//! as a page you can format or as its Markdown, saved back to the file.
//!
//! - **How one comes in**: Explorer's "Open with" (the installer registers
//!   Yap as a choice for both types, never their default — Windows lets
//!   the person pick that, from Settings → General's "Open Markdown files
//!   with Yap", `md_open_with_settings`), `yap.exe <path>` (read at start,
//!   [`args::at_launch`], and from a second launch's arguments in
//!   single-instance's callback, `lib.rs`), and the [`open_markdown`]
//!   command (Settings' "Open a Markdown file…", any part of Yap).
//! - **One window per file** (`doc-` + a hash of its path, [`label_for`]):
//!   opening the same file again brings its window up. Made on demand,
//!   undecorated (its own title bar: the file's name and folder), on the
//!   paper's colours (`shell/theme.rs`), placed in the middle of the
//!   monitor with the mouse, each next one a step down and right
//!   ([`placed`]); never in the window-state plugin's file. Shown without
//!   taking the foreground from anyone else and then focused
//!   (`show_quietly`, never in a test run). The page:
//!   `desktop/src/lib/doc/DocWindow.svelte`.
//! - **The file**: read and written only by the window it was opened in —
//!   the commands find the file by the calling window's label, never a
//!   path from the page. Up to 5 MB ([`MAX_BYTES`]: a bigger one says so,
//!   plainly). UTF-8 with or without a byte-order mark, or UTF-16, its line
//!   endings kept (`text.rs`); anything else, or a read-only file, opens
//!   read-only. Saved by an atomic write (`config::atomic_write_bytes`)
//!   only if the file is still what the page last read or wrote (its
//!   modified time and size, a `stamp`): otherwise the page is told
//!   (`conflict`) and asks — "Changed on disk · Reload · Keep mine".
//! - **Closing**: the window asks its page first (`yap-doc-closing`: what's
//!   typed is saved, then `md_file_close`); asked again within a few
//!   seconds (a page that didn't answer), it closes at once.

pub mod args;
pub mod text;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// Every document window's label starts with this.
pub const LABEL_PREFIX: &str = "doc-";
/// The biggest file a window opens.
pub const MAX_BYTES: u64 = 5 * 1024 * 1024;
/// A new window's size (logical pixels), and the least it can be.
const SIZE: (f64, f64) = (820.0, 900.0);
const MIN_SIZE: (f64, f64) = (420.0, 320.0);
/// How far each next window steps down and right of the one before (logical).
const CASCADE: f64 = 28.0;
/// A second close within this long closes at once.
const CLOSE_AGAIN: Duration = Duration::from_secs(8);

/// The file each open window is for, and how it was written (once read).
struct Doc {
    path: PathBuf,
    form: Option<text::Form>,
}

static DOCS: Mutex<BTreeMap<String, Doc>> = Mutex::new(BTreeMap::new());
/// Windows whose page was asked to close, and when.
static CLOSING: Mutex<BTreeMap<String, Instant>> = Mutex::new(BTreeMap::new());

fn docs() -> std::sync::MutexGuard<'static, BTreeMap<String, Doc>> {
    DOCS.lock().unwrap_or_else(|p| p.into_inner())
}

/// Is `label` a document window's?
pub fn is_label(label: &str) -> bool {
    label.starts_with(LABEL_PREFIX)
}

/// The window label for `path`: `doc-` and 12 hex digits of its path's
/// FNV-1a hash, case and slashes folded (Windows' paths are neither).
pub fn label_for(path: &Path) -> String {
    let key = path.to_string_lossy().replace('/', "\\").to_lowercase();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in key.trim_end_matches('\\').bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{LABEL_PREFIX}{:012x}", h & 0xffff_ffff_ffff)
}

/// Where a window goes: `size` fitted into the monitor's work `area` (both
/// physical: x, y, width, height), in its middle, the `n`th window open
/// already `step` pixels further down and right each (back to the middle
/// once that would leave the area). Pure.
pub(crate) fn placed(area: (i32, i32, u32, u32), size: (u32, u32), n: usize, step: i32) -> (i32, i32, u32, u32) {
    let (ax, ay, aw, ah) = area;
    let (w, h) = (size.0.min(aw), size.1.min(ah));
    let x0 = ax + (aw - w) as i32 / 2;
    let y0 = ay + (ah - h) as i32 / 2;
    let room = ((ax + aw as i32 - w as i32 - x0).min(ay + ah as i32 - h as i32 - y0)).max(0);
    let steps = if step > 0 { (room / step) as usize + 1 } else { 1 };
    let k = (n % steps) as i32;
    (x0 + k * step, y0 + k * step, w, h)
}

/// "plan.md · Yap": the taskbar's name for the window.
fn window_title(path: &Path) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    format!("{name} · Yap")
}

/// Open `path` in its window: brought up if it's open, else made. Any
/// thread but the main one (a window is made through the event loop);
/// never under a lock. The window's label.
pub fn open(app: &AppHandle, path: &Path) -> Result<String, String> {
    let path = std::path::absolute(path).map_err(|_| "That isn't a file's path.".to_string())?;
    if !args::is_markdown(&path) {
        return Err("Yap opens Markdown files: .md and .markdown.".to_string());
    }
    if !path.is_file() {
        return Err("There's no file there: it may have been moved or deleted.".to_string());
    }
    let label = label_for(&path);
    if let Some(w) = app.get_webview_window(&label) {
        bring_up(&w);
        return Ok(label);
    }
    docs().insert(label.clone(), Doc { path: path.clone(), form: None });
    let w = match build(app, &label, &path) {
        Ok(w) => w,
        Err(e) => {
            // Made meanwhile by another open of the same file: that one.
            if let Some(w) = app.get_webview_window(&label) {
                bring_up(&w);
                return Ok(label);
            }
            docs().remove(&label);
            return Err(e);
        }
    };
    place(app, &w);
    crate::shell::window::show_quietly(&w);
    crate::shell::window::focus(&w);
    tracing::info!(label = %label, "Markdown file opened in its window");
    tracing::debug!(path = %path.display(), "Markdown file");
    Ok(label)
}

/// Open each of `paths` (a launch's arguments), off the calling thread.
pub fn open_soon(app: &AppHandle, paths: Vec<PathBuf>) {
    if paths.is_empty() {
        return;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("md-open".into()).spawn(move || {
        for path in paths {
            if let Err(e) = open(&app, &path) {
                tracing::warn!("Couldn't open a Markdown file: {e}");
            }
        }
    });
    if let Err(e) = spawned {
        tracing::warn!("Couldn't open a Markdown file: {e}");
    }
}

fn bring_up(w: &tauri::WebviewWindow) {
    if w.is_minimized().unwrap_or(false) {
        let _ = w.unminimize();
    }
    crate::shell::window::show_quietly(w);
    crate::shell::window::focus(w);
}

fn build(app: &AppHandle, label: &str, path: &Path) -> Result<tauri::WebviewWindow, String> {
    let cfg = crate::config::load();
    let theme = crate::shell::theme::resolve(&cfg.theme, crate::shell::theme::system_dark());
    let mut builder = tauri::WebviewWindowBuilder::new(app, label, tauri::WebviewUrl::App("index.html".into()))
        .title(window_title(path))
        .inner_size(SIZE.0, SIZE.1)
        .min_inner_size(MIN_SIZE.0, MIN_SIZE.1)
        .decorations(false)
        .resizable(true)
        .visible(false)
        .focused(false);
    // No white before the page paints (theme.rs).
    if let Some([r, g, b]) = crate::shell::theme::background(label, theme) {
        builder = builder.background_color(tauri::window::Color(r, g, b, 255));
    }
    let w = builder.build().map_err(|e| format!("Couldn't open the file's window: {e}"))?;
    let handle = app.clone();
    let own = label.to_string();
    w.on_window_event(move |event| match event {
        tauri::WindowEvent::CloseRequested { api, .. } => {
            let mut closing = CLOSING.lock().unwrap_or_else(|p| p.into_inner());
            let again = closing.get(&own).is_some_and(|at| at.elapsed() < CLOSE_AGAIN);
            if !again {
                // The page saves what's typed first, then closes it.
                api.prevent_close();
                closing.insert(own.clone(), Instant::now());
                drop(closing);
                let _ = handle.emit_to(own.as_str(), "yap-doc-closing", ());
            }
        }
        tauri::WindowEvent::Destroyed => {
            docs().remove(&own);
            CLOSING.lock().unwrap_or_else(|p| p.into_inner()).remove(&own);
        }
        _ => {}
    });
    Ok(w)
}

/// In the middle of the monitor with the mouse (a test run: the primary
/// one), a step on from each document window open already.
fn place(app: &AppHandle, w: &tauri::WebviewWindow) {
    let monitor = if crate::shell::e2e::active() {
        w.primary_monitor().ok().flatten()
    } else {
        app.cursor_position()
            .ok()
            .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
            .or_else(|| w.primary_monitor().ok().flatten())
    };
    let Some(monitor) = monitor else { return };
    let area = monitor.work_area();
    let scale = monitor.scale_factor();
    let size = ((SIZE.0 * scale).round() as u32, (SIZE.1 * scale).round() as u32);
    let others = app.webview_windows().keys().filter(|l| is_label(l) && *l != w.label()).count();
    let (x, y, width, height) = placed(
        (area.position.x, area.position.y, area.size.width, area.size.height),
        size,
        others,
        (CASCADE * scale).round() as i32,
    );
    // Onto that monitor first (another DPI scales the window on the way).
    let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
    let _ = w.set_size(tauri::PhysicalSize::new(width, height));
    let _ = w.set_position(tauri::PhysicalPosition::new(x, y));
}

// ---- the file ------------------------------------------------------------------------------

/// A file's modified time and size, as one string the page compares.
fn stamp(meta: &std::fs::Metadata) -> String {
    let nanos = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos}:{}", meta.len())
}

/// The file's stamp now (None: it isn't there).
fn stamp_now(path: &Path) -> Option<String> {
    std::fs::metadata(path).ok().filter(|m| m.is_file()).map(|m| stamp(&m))
}

fn read_error(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "This file isn't there any more: it was moved or deleted.".to_string(),
        std::io::ErrorKind::PermissionDenied => "Yap isn't allowed to read this file.".to_string(),
        _ => format!("Couldn't read the file: {e}"),
    }
}

fn save_error(e: &std::io::Error) -> String {
    if crate::config::held(e) {
        return "Couldn't save: the file is read-only, or another program is using it.".to_string();
    }
    match e.kind() {
        std::io::ErrorKind::NotFound => "Couldn't save: its folder isn't there any more.".to_string(),
        _ => format!("Couldn't save the file: {e}"),
    }
}

/// What a window says for a file over [`MAX_BYTES`] ("This file is 7.2 MB: …";
/// one a few bytes over is "just over 5 MB", never "5.0 MB").
fn too_big(bytes: u64) -> String {
    let size = text::size_label(bytes);
    let size = if size == "5.0 MB" { "just over 5 MB".to_string() } else { size };
    format!("This file is {size}: Yap opens Markdown files up to 5 MB.")
}

/// The file the calling window is for.
fn path_of(label: &str) -> Result<PathBuf, String> {
    docs().get(label).map(|d| d.path.clone()).ok_or_else(|| "This window has no file.".to_string())
}

/// A file read for its window.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Loaded {
    /// Its text, `\n` line breaks.
    pub text: String,
    pub stamp: String,
    pub size: u64,
    /// "UTF-8", "UTF-8 with BOM"…; None: not text Yap can write.
    pub encoding: Option<&'static str>,
    /// "LF" or "CRLF".
    pub eol: Option<&'static str>,
    /// Why it can't be edited here, if it can't.
    pub read_only: Option<String>,
}

/// Read `path` (at most [`MAX_BYTES`]); the form it's written in for saves.
fn read(path: &Path) -> Result<(Loaded, Option<text::Form>), String> {
    let meta = std::fs::metadata(path).map_err(|e| read_error(&e))?;
    if !meta.is_file() {
        return Err("This isn't a file.".to_string());
    }
    if meta.len() > MAX_BYTES {
        return Err(too_big(meta.len()));
    }
    let bytes = std::fs::read(path).map_err(|e| read_error(&e))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(too_big(bytes.len() as u64));
    }
    let decoded = text::decode(&bytes);
    let read_only = if decoded.form.is_none() {
        Some("Yap can edit UTF-8 and UTF-16 text only, so this file opens read-only.".to_string())
    } else if meta.permissions().readonly() {
        Some("This file is read-only.".to_string())
    } else {
        None
    };
    let loaded = Loaded {
        text: decoded.text,
        stamp: stamp(&meta),
        size: bytes.len() as u64,
        encoding: decoded.form.map(|f| f.encoding.name()),
        eol: decoded.form.map(|f| f.eol.name()),
        read_only,
    };
    Ok((loaded, decoded.form))
}

/// A save's outcome.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    /// The file's stamp now (None: it isn't there).
    pub stamp: Option<String>,
    /// Not written: the file changed on disk since the page's `expect`.
    pub conflict: bool,
}

/// Write `text` to `path` in `form` — unless `expect` is given and the file
/// isn't that any more (changed or gone): then nothing is written.
fn save(path: &Path, text: &str, form: text::Form, expect: Option<&str>) -> Result<Saved, String> {
    if let Some(expect) = expect {
        let now = stamp_now(path);
        if now.as_deref() != Some(expect) {
            return Ok(Saved { stamp: now, conflict: true });
        }
    }
    // A link's own file, not the link.
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    crate::config::atomic_write_bytes(&target, &text::encode(text, form)).map_err(|e| save_error(&e))?;
    Ok(Saved { stamp: stamp_now(path), conflict: false })
}

// ---- Windows' Default apps ---------------------------------------------------------------

/// The Default apps page of Windows' Settings: Yap's own page on it
/// (Windows 11 since 2023-04) when Yap is registered — per user (Yap's
/// installer), or for the machine —, else the list of apps. Pure.
pub(crate) fn default_apps_uri(name: &str, user: bool, machine: bool) -> String {
    let escaped: String = name
        .bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect();
    if user {
        format!("ms-settings:defaultapps?registeredAppUser={escaped}")
    } else if machine {
        format!("ms-settings:defaultapps?registeredAppMachine={escaped}")
    } else {
        "ms-settings:defaultapps".to_string()
    }
}

/// The name Yap is registered under (`Software\RegisteredApplications`,
/// the installer's `${PRODUCTNAME}`).
const REGISTERED_NAME: &str = "Yap";

#[cfg(windows)]
mod win {
    use windows::core::{HSTRING, PWSTR};
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    use windows::Win32::UI::Shell::{AssocQueryStringW, ASSOCF_NONE, ASSOCSTR_EXECUTABLE};

    fn has_value(root: HKEY, key: &str, name: &str) -> bool {
        let mut size = 0u32;
        let r = unsafe { RegGetValueW(root, &HSTRING::from(key), &HSTRING::from(name), RRF_RT_REG_SZ, None, None, Some(&mut size)) };
        r == ERROR_SUCCESS
    }

    /// Is Yap in `RegisteredApplications`: (for this user, for the machine)?
    pub fn registered(name: &str) -> (bool, bool) {
        let key = r"Software\RegisteredApplications";
        (has_value(HKEY_CURRENT_USER, key, name), has_value(HKEY_LOCAL_MACHINE, key, name))
    }

    /// The program `.ext` files open with now (Windows' choice).
    pub fn opens_with(ext: &str) -> Option<String> {
        let mut len = 0u32;
        let assoc = HSTRING::from(ext);
        let verb = HSTRING::from("open");
        unsafe {
            let _ = AssocQueryStringW(ASSOCF_NONE, ASSOCSTR_EXECUTABLE, &assoc, &verb, None, &mut len);
            if len == 0 {
                return None;
            }
            let mut buf = vec![0u16; len as usize];
            AssocQueryStringW(ASSOCF_NONE, ASSOCSTR_EXECUTABLE, &assoc, &verb, Some(PWSTR(buf.as_mut_ptr())), &mut len)
                .ok()
                .ok()?;
            let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            Some(String::from_utf16_lossy(&buf[..end]))
        }
    }
}

/// Is `exe` Yap? Its file name, any case: an installed Yap and a dev build
/// are both "Yap".
fn is_yap(exe: &str) -> bool {
    let own = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()))
        .unwrap_or_else(|| "yap.exe".to_string());
    Path::new(exe).file_name().is_some_and(|n| n.to_string_lossy().to_lowercase() == own)
}

/// Settings → General's "Open Markdown files with Yap": whether Windows
/// knows Yap as a Markdown app (an installed Yap; not a dev or portable
/// copy), and whether .md files open with it now.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenWith {
    pub registered: bool,
    pub is_default: bool,
}

// ---- commands ----------------------------------------------------------------------------

/// Open a Markdown file in its window (one per file; the same file again
/// brings its window up). The window's label.
#[tauri::command]
pub async fn open_markdown(app: AppHandle, path: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || open(&app, Path::new(path.trim())))
        .await
        .map_err(|e| e.to_string())?
}

/// The window's file: its path, name and folder.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    pub path: String,
    pub name: String,
    pub folder: String,
}

#[tauri::command]
pub fn md_file_info(window: tauri::WebviewWindow) -> Result<Info, String> {
    let path = path_of(window.label())?;
    Ok(Info {
        path: path.display().to_string(),
        name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        folder: path.parent().map(|p| p.display().to_string()).unwrap_or_default(),
    })
}

/// The window's file, read (again).
#[tauri::command]
pub async fn md_file_read(window: tauri::WebviewWindow) -> Result<Loaded, String> {
    let label = window.label().to_string();
    let path = path_of(&label)?;
    let (loaded, form) = tauri::async_runtime::spawn_blocking(move || read(&path)).await.map_err(|e| e.to_string())??;
    if let Some(doc) = docs().get_mut(&label) {
        doc.form = form;
    }
    tracing::info!(label = %label, bytes = loaded.size, "Markdown file read");
    Ok(loaded)
}

/// Save the page's `text` to the window's file, as it was written (its
/// encoding and line endings) — only while it's still `expect` (the stamp
/// the page last read or wrote); without `expect` ("Keep mine"), whatever's
/// there now.
#[tauri::command]
pub async fn md_file_save(window: tauri::WebviewWindow, text: String, expect: Option<String>) -> Result<Saved, String> {
    let label = window.label().to_string();
    let (path, form) = {
        let docs = docs();
        let doc = docs.get(&label).ok_or_else(|| "This window has no file.".to_string())?;
        (doc.path.clone(), doc.form)
    };
    let form = form.ok_or_else(|| "Yap can't save this file: it isn't text Yap can write back as it was.".to_string())?;
    let chars = text.len();
    let saved = tauri::async_runtime::spawn_blocking(move || save(&path, &text, form, expect.as_deref()))
        .await
        .map_err(|e| e.to_string())??;
    tracing::info!(label = %label, bytes = chars, conflict = saved.conflict, "Markdown file saved");
    Ok(saved)
}

/// The window's file's stamp now (null: it isn't there), for the page's
/// look every 1.5 s.
#[tauri::command]
pub async fn md_file_stat(window: tauri::WebviewWindow) -> Result<Option<String>, String> {
    let path = path_of(window.label())?;
    tauri::async_runtime::spawn_blocking(move || stamp_now(&path)).await.map_err(|e| e.to_string())
}

/// The window's file in Explorer, selected (a test run keeps it for the suite).
#[tauri::command]
pub fn md_file_reveal(window: tauri::WebviewWindow) -> Result<(), String> {
    let path = path_of(window.label())?;
    if crate::shell::e2e::active() {
        crate::shell::commands::e2e_kept(&format!("reveal:{}", window.label()));
        return Ok(());
    }
    tauri_plugin_opener::reveal_item_in_dir(&path).map_err(|e| format!("Couldn't open the folder: {e}"))
}

/// The page saved what was typed: the window closes.
#[tauri::command]
pub async fn md_file_close(window: tauri::WebviewWindow) -> Result<(), String> {
    if !is_label(window.label()) {
        return Err("Not a file's window.".to_string());
    }
    window.destroy().map_err(|e| e.to_string())
}

/// Whether Windows knows Yap as a Markdown app, and opens .md files with it.
#[tauri::command]
pub fn md_open_with_state() -> OpenWith {
    #[cfg(windows)]
    {
        let (user, machine) = win::registered(REGISTERED_NAME);
        let is_default = win::opens_with(".md").is_some_and(|exe| is_yap(&exe));
        OpenWith { registered: user || machine, is_default }
    }
    #[cfg(not(windows))]
    {
        let _ = is_yap;
        OpenWith { registered: false, is_default: false }
    }
}

/// Windows' Default apps page, on Yap's own page when it's registered: the
/// person picks Yap for .md there (Windows lets no app make itself the
/// default). A test run keeps the address for the suite.
#[tauri::command]
pub fn md_open_with_settings() -> Result<(), String> {
    #[cfg(windows)]
    let (user, machine) = win::registered(REGISTERED_NAME);
    #[cfg(not(windows))]
    let (user, machine) = (false, false);
    let uri = default_apps_uri(REGISTERED_NAME, user, machine);
    if crate::shell::e2e::active() {
        crate::shell::commands::e2e_kept(&uri);
        return Ok(());
    }
    tauri_plugin_opener::open_url(&uri, None::<&str>).map_err(|e| format!("Couldn't open Windows' Settings: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yap-md-file-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn one_label_per_file_whatever_its_case_or_slashes() {
        let a = label_for(Path::new(r"C:\Work\Plan.md"));
        assert!(a.starts_with("doc-") && a.len() == 16, "{a}");
        assert!(a[4..].chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(a, label_for(Path::new(r"c:\work\plan.md")));
        assert_eq!(a, label_for(Path::new("C:/Work/Plan.md")));
        assert_ne!(a, label_for(Path::new(r"C:\Work\Plan2.md")));
        assert!(is_label(&a));
        assert!(!is_label("settings") && !is_label("quick-note"));
    }

    #[test]
    fn windows_go_in_the_middle_then_a_step_on_each() {
        let area = (0, 0, 1920, 1040);
        assert_eq!(placed(area, (820, 900), 0, 28), (550, 70, 820, 900));
        assert_eq!(placed(area, (820, 900), 1, 28), (578, 98, 820, 900));
        assert_eq!(placed(area, (820, 900), 2, 28), (606, 126, 820, 900));
        // Back to the middle before one would leave the screen (70 px below: two steps fit).
        assert_eq!(placed(area, (820, 900), 3, 28), (550, 70, 820, 900));
        // Bigger than the screen: fitted to it.
        assert_eq!(placed((0, 0, 1280, 672), (1230, 1350), 0, 42), (25, 0, 1230, 672));
        assert_eq!(placed((0, 0, 1280, 672), (1230, 1350), 5, 42), (25, 0, 1230, 672));
        // A second monitor's area.
        assert_eq!(placed((1920, -200, 1600, 900), (800, 600), 0, 28), (2320, -50, 800, 600));
    }

    #[test]
    fn a_file_read_and_saved_keeps_its_bom_and_line_endings() {
        let dir = temp_dir("keep");
        let path = dir.join("plan.md");
        let original = b"\xEF\xBB\xBF# Plan\r\n\r\n- One\r\n- Two\r\n";
        std::fs::write(&path, original).unwrap();
        let (loaded, form) = read(&path).unwrap();
        assert_eq!(loaded.text, "# Plan\n\n- One\n- Two\n");
        assert_eq!(loaded.encoding, Some("UTF-8 with BOM"));
        assert_eq!(loaded.eol, Some("CRLF"));
        assert_eq!(loaded.read_only, None);
        let form = form.unwrap();
        let saved = save(&path, "# Plan\n\n- One\n- Two\n- Three\n", form, Some(&loaded.stamp)).unwrap();
        assert!(!saved.conflict);
        assert_eq!(std::fs::read(&path).unwrap(), b"\xEF\xBB\xBF# Plan\r\n\r\n- One\r\n- Two\r\n- Three\r\n");
        assert_eq!(saved.stamp, stamp_now(&path));
        // No temp file left beside it.
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_save_never_writes_over_a_change_on_disk_unless_asked() {
        let dir = temp_dir("conflict");
        let path = dir.join("notes.markdown");
        std::fs::write(&path, "first\n").unwrap();
        let (loaded, form) = read(&path).unwrap();
        let form = form.unwrap();
        // Another program writes it (a different size: a new stamp).
        std::fs::write(&path, "theirs, longer\n").unwrap();
        let saved = save(&path, "mine\n", form, Some(&loaded.stamp)).unwrap();
        assert!(saved.conflict);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "theirs, longer\n");
        assert_eq!(saved.stamp, stamp_now(&path));
        // "Keep mine": no stamp to match.
        let saved = save(&path, "mine\n", form, None).unwrap();
        assert!(!saved.conflict);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "mine\n");
        // Gone from disk: a save expecting it says so too.
        std::fs::remove_file(&path).unwrap();
        let saved = save(&path, "again\n", form, saved.stamp.as_deref()).unwrap();
        assert!(saved.conflict);
        assert_eq!(saved.stamp, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn files_over_five_megabytes_are_refused_plainly() {
        let dir = temp_dir("big");
        let path = dir.join("huge.md");
        std::fs::write(&path, vec![b'a'; MAX_BYTES as usize + 1]).unwrap();
        let e = read(&path).unwrap_err();
        assert_eq!(e, "This file is just over 5 MB: Yap opens Markdown files up to 5 MB.");
        assert_eq!(too_big(7 * 1024 * 1024 + 200 * 1024), "This file is 7.2 MB: Yap opens Markdown files up to 5 MB.");
        // Exactly 5 MB opens.
        std::fs::write(&path, vec![b'a'; MAX_BYTES as usize]).unwrap();
        assert_eq!(read(&path).unwrap().0.size, MAX_BYTES);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn files_yap_cant_write_back_open_read_only() {
        let dir = temp_dir("ro");
        let path = dir.join("latin1.md");
        std::fs::write(&path, b"caf\xe9\n").unwrap();
        let (loaded, form) = read(&path).unwrap();
        assert!(form.is_none());
        assert!(loaded.read_only.is_some());
        assert_eq!(loaded.encoding, None);
        let missing = dir.join("missing.md");
        assert_eq!(read(&missing).unwrap_err(), "This file isn't there any more: it was moved or deleted.");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_default_apps_page_is_yaps_own_when_windows_knows_it() {
        assert_eq!(default_apps_uri("Yap", true, false), "ms-settings:defaultapps?registeredAppUser=Yap");
        assert_eq!(default_apps_uri("Yap", false, true), "ms-settings:defaultapps?registeredAppMachine=Yap");
        assert_eq!(default_apps_uri("Yap", false, false), "ms-settings:defaultapps");
        assert_eq!(default_apps_uri("Yap Dev", true, true), "ms-settings:defaultapps?registeredAppUser=Yap%20Dev");
    }

    #[test]
    fn yap_is_known_by_its_exe_name() {
        let own = std::env::current_exe().unwrap();
        assert!(is_yap(&own.display().to_string()));
        assert!(is_yap(&own.display().to_string().to_uppercase()));
        assert!(!is_yap(r"C:\Program Files\Some Editor\editor.exe"));
    }
}

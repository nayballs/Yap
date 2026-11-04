//! Portable-mode support for Yap.
//!
//! When a file named `portable` (containing the magic string
//! `"Yap Portable Mode"`) sits next to the executable, all user data
//! (config, models, logs) is stored in a `Data/` directory alongside the
//! executable instead of `%APPDATA%`. The NSIS installer's "Portable
//! Installation" option writes that marker + `Data/` dir (see
//! `nsis/installer.nsi`).
//!
//! The decision is made **once** at startup and cached in a `OnceLock`, so
//! the rest of the app can ask cheaply via [`data_dir`] / [`is_portable`].
//! When not portable, callers fall back to the normal `%APPDATA%/yap` path —
//! that path is left byte-for-byte unchanged so existing installs are
//! unaffected.

use std::path::PathBuf;
use std::sync::OnceLock;

/// `Some(<exe_dir>/Data)` when running portable, `None` otherwise.
static PORTABLE_DATA_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Magic string written into the `portable` marker file by the installer.
const PORTABLE_MARKER: &str = "Yap Portable Mode";

/// Detect portable mode by looking for a valid `portable` marker next to the
/// exe. Must be called once at startup, before anything reads the data dir.
pub fn init() {
    PORTABLE_DATA_DIR.get_or_init(|| {
        let exe_path = std::env::current_exe().ok()?;
        let exe_dir = exe_path.parent()?;
        let marker_path = exe_dir.join("portable");

        if !is_valid_portable_marker(&marker_path) {
            return None;
        }

        let data_dir = exe_dir.join("Data");
        if !data_dir.exists() {
            std::fs::create_dir_all(&data_dir).ok()?;
        }
        eprintln!("[portable] data dir: {}", data_dir.display());
        Some(data_dir)
    });
}

/// The model evaluation (`yap.exe eval`, `eval.rs`; debug builds) and the
/// benchmark (`yap.exe bench`, `bench.rs`): use `dir` as the data dir, as
/// portable mode would, so a run never writes the installed app's. Must
/// come before anything reads the data dir (`false`: too late, the dir is
/// already decided).
pub fn use_data_dir(dir: PathBuf) -> bool {
    PORTABLE_DATA_DIR.set(Some(dir)).is_ok()
}

/// The data dir this exe would use, decided as [`init`] decides it but
/// without creating anything or deciding it for this run: `<exe>/Data`
/// beside a valid marker, else `%APPDATA%\yap`. (`yap.exe bench` reads the
/// models from it, then runs on a folder of its own.)
pub fn would_use() -> PathBuf {
    let portable = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.to_path_buf()))
        .filter(|dir| is_valid_portable_marker(&dir.join("portable")))
        .map(|dir| dir.join("Data"));
    portable.unwrap_or_else(|| dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")).join("yap"))
}

/// `true` when running in portable mode.
pub fn is_portable() -> bool {
    PORTABLE_DATA_DIR.get().and_then(|v| v.as_ref()).is_some()
}

/// The portable data dir (`<exe_dir>/Data`) when active, else `None`.
pub fn data_dir() -> Option<&'static PathBuf> {
    PORTABLE_DATA_DIR.get().and_then(|v| v.as_ref())
}

/// `true` if the marker file exists and contains the portable magic string.
/// Extracted so it can be unit-tested without touching the real exe dir.
fn is_valid_portable_marker(path: &std::path::Path) -> bool {
    std::fs::read_to_string(path)
        .map(|s| s.trim().starts_with(PORTABLE_MARKER))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn valid_magic_string_enables_portable() {
        let dir = std::env::temp_dir().join("yap_test_valid");
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("portable");
        let mut f = std::fs::File::create(&marker).unwrap();
        write!(f, "Yap Portable Mode").unwrap();
        assert!(is_valid_portable_marker(&marker));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn empty_file_does_not_enable_portable() {
        let dir = std::env::temp_dir().join("yap_test_empty");
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("portable");
        std::fs::File::create(&marker).unwrap();
        assert!(!is_valid_portable_marker(&marker));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn wrong_content_does_not_enable_portable() {
        let dir = std::env::temp_dir().join("yap_test_wrong");
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("portable");
        let mut f = std::fs::File::create(&marker).unwrap();
        write!(f, "some other content").unwrap();
        assert!(!is_valid_portable_marker(&marker));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_file_does_not_enable_portable() {
        let path = std::path::Path::new("/nonexistent/portable");
        assert!(!is_valid_portable_marker(path));
    }

    #[test]
    fn magic_string_with_whitespace_enables_portable() {
        let dir = std::env::temp_dir().join("yap_test_ws");
        std::fs::create_dir_all(&dir).unwrap();
        let marker = dir.join("portable");
        let mut f = std::fs::File::create(&marker).unwrap();
        writeln!(f, "  Yap Portable Mode").unwrap();
        assert!(is_valid_portable_marker(&marker));
        std::fs::remove_dir_all(dir).unwrap();
    }
}

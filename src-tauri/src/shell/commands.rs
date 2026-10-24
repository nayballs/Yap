//! The shell's small commands: web and mail links opened in the browser
//! (kept, never opened, in a test run) and portable mode.

use tauri::AppHandle;

/// Links the pages asked to open during a test run (never opened), oldest
/// first, for the suite (`e2e_opened_urls`).
static E2E_OPENED_URLS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Is `url` a link Yap opens in the browser? Web and mail only: never
/// `file:`, `javascript:`, a custom scheme or a bare path.
fn is_web_or_mail_link(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    ["https://", "http://", "mailto:"].iter().any(|p| lower.starts_with(p))
}

/// Open a web or mail link in the default browser. Every external link in
/// Yap's pages comes through here (`lib/externalLinks.js`: settings links,
/// links in chat answers and notes, release notes, Share by email). A test
/// run never opens one: it keeps it for the suite (`e2e_opened_urls`), so a
/// test can't pop a browser up on the PC running it.
#[tauri::command]
pub fn open_external(app: AppHandle, url: String) -> Result<(), String> {
    if !is_web_or_mail_link(&url) {
        return Err("Yap only opens web and email links.".to_string());
    }
    let url = url.trim();
    if crate::shell::e2e::active() {
        tracing::info!("e2e: would open a link in the browser");
        if let Ok(mut opened) = E2E_OPENED_URLS.lock() {
            opened.push(url.to_string());
        }
        return Ok(());
    }
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| format!("Couldn't open your browser: {e}"))
}

/// Test mode only: the links the pages asked to open (see `open_external`).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_opened_urls() -> Result<Vec<String>, String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    Ok(E2E_OPENED_URLS.lock().map(|o| o.clone()).unwrap_or_default())
}

/// Whether Yap is running as a portable install (data lives next to the exe).
/// The update UI uses this to steer portable users to a manual download, since
/// the in-place updater can't safely replace a portable folder.
#[tauri::command]
pub fn is_portable() -> bool {
    crate::shell::portable::is_portable()
}

#[cfg(test)]
mod tests {
    use super::is_web_or_mail_link;

    #[test]
    fn only_web_and_mail_links_open() {
        let links = ["https://docs.example.com/plan?a=1&b=2", "HTTP://x.io", " mailto:a@b.c", "https://github.com/nayballs/Yap"];
        for ok in links {
            assert!(is_web_or_mail_link(ok), "{ok}");
        }
        let no_links = [
            "javascript:alert(1)",
            "file:///C:/Windows/system32/calc.exe",
            r"C:\Windows",
            "ms-settings:privacy",
            "tauri://localhost",
            "",
        ];
        for no in no_links {
            assert!(!is_web_or_mail_link(no), "{no}");
        }
    }
}

//! **Which browser opens Yap's links** (Settings → General → Links, "Open
//! links in"; 2026-11-01, yap-private `docs/look-and-show-plan.md`
//! Milestone 2): Windows' default browser, or one of the browsers installed.
//!
//! - **The browsers installed** are Windows' registered browsers
//!   (`SOFTWARE\Clients\StartMenuInternet` under HKCU, HKLM and HKLM's
//!   32-bit view): each one's key, its name (the key's default value) and
//!   its program (`shell\open\command`). Read only; nothing is launched to
//!   find them. Their icons are the programs' own (`calls/app_icons.rs`
//!   `exe_icon_url`), for the Settings list.
//! - **`open_external`** (`shell/commands.rs`, every link button in Yap)
//!   comes here ([`open`]): a web link goes to the chosen browser's program
//!   with the address as its one argument; with none chosen (or the chosen
//!   one gone), to Windows' default as before (the opener plugin). Mail
//!   links always go to the default mail app.
//! - **Its name, for buttons** ([`label`], `browser_label`): "Open in
//!   Brave". With Windows' default, the default's name when Yap can tell
//!   (`UrlAssociations\https\UserChoice`'s ProgId → its command → the
//!   registered browser with that program), else "your browser".
//!
//! Yap never drives the person's browser: it hands it an address, and their
//! logins stay theirs. Test mode reads no registry: the browsers come from
//! `YAP_E2E_BROWSERS` (a JSON list of `{key, name, command}`), and nothing
//! is opened (`open_external` keeps the link for the suite).

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// A registered browser as Windows lists it (read, not yet checked).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Raw {
    /// Its key under `StartMenuInternet` ("Brave", "Firefox-308046B0AF4A39CB").
    pub key: String,
    /// The key's default value ("Brave"; "" when it has none).
    #[serde(default)]
    pub name: String,
    /// `shell\open\command`'s default value.
    #[serde(default)]
    pub command: String,
}

/// **A browser Yap can open links in.**
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Browser {
    /// Its registry key: what `open_links_in` keeps.
    pub id: String,
    pub name: String,
    /// Its program's full path.
    pub exe: String,
    /// Its program's icon, a PNG data URL (Settings' list only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

/// The program in a registered command: its quoted first part
/// (`"C:\…\brave.exe" --flag "%1"`), or everything up to `.exe`. Pure.
pub fn exe_of(command: &str) -> Option<String> {
    let c = command.trim();
    let exe = if let Some(rest) = c.strip_prefix('"') {
        rest.split('"').next().unwrap_or("").trim().to_string()
    } else {
        let lower = c.to_ascii_lowercase();
        match lower.find(".exe") {
            Some(i) => c[..i + 4].trim().to_string(),
            None => c.split_whitespace().next().unwrap_or("").to_string(),
        }
    };
    (exe.to_ascii_lowercase().ends_with(".exe") && exe.len() > 4).then_some(exe)
}

/// The file name of a program's path, lowercased ("brave.exe"). Pure.
fn file_name(exe: &str) -> String {
    exe.rsplit(['\\', '/']).next().unwrap_or(exe).to_ascii_lowercase()
}

/// **The registered browsers, checked** (`exists`: a program is there):
/// each key once (the first seen wins: HKCU before HKLM), a program it can
/// be started with, a name (the key's when its own is missing or a
/// resource reference, "@…"), Internet Explorer left out; by name. Pure.
pub fn parse(raw: &[Raw], exists: &dyn Fn(&str) -> bool) -> Vec<Browser> {
    let mut out: Vec<Browser> = Vec::new();
    for r in raw {
        let key = r.key.trim();
        if key.is_empty() || out.iter().any(|b| b.id.eq_ignore_ascii_case(key)) {
            continue;
        }
        let Some(exe) = exe_of(&r.command) else { continue };
        if file_name(&exe) == "iexplore.exe" || !exists(&exe) {
            continue;
        }
        // The same program under two keys (a per-user and a machine-wide
        // entry of one browser): once.
        if out.iter().any(|b| b.exe.eq_ignore_ascii_case(&exe)) {
            continue;
        }
        let name = r.name.trim();
        let name = if name.is_empty() || name.starts_with('@') { key } else { name };
        out.push(Browser { id: key.to_string(), name: name.to_string(), exe, icon: None });
    }
    out.sort_by_key(|b| b.name.to_lowercase());
    out
}

/// The registered browser whose program is `exe` (the default browser's,
/// read from its ProgId): by its path, else by its file's name. Pure.
pub fn matching<'a>(browsers: &'a [Browser], exe: &str) -> Option<&'a Browser> {
    browsers
        .iter()
        .find(|b| b.exe.eq_ignore_ascii_case(exe))
        .or_else(|| browsers.iter().find(|b| file_name(&b.exe) == file_name(exe)))
}

/// **A button's name for the browser** links open in: the chosen one's
/// (`choice`: its key), else Windows' default's when it's one of the
/// registered browsers (`default_exe`), else "your browser". Pure.
pub fn label_for(choice: &str, browsers: &[Browser], default_exe: Option<&str>) -> String {
    if let Some(b) = chosen_in(choice, browsers) {
        return b.name.clone();
    }
    default_exe.and_then(|e| matching(browsers, e)).map(|b| b.name.clone()).unwrap_or_else(|| "your browser".to_string())
}

/// The browser `choice` names among `browsers` ("" = Windows' default). Pure.
pub fn chosen_in<'a>(choice: &str, browsers: &'a [Browser]) -> Option<&'a Browser> {
    let c = choice.trim();
    (!c.is_empty()).then(|| browsers.iter().find(|b| b.id.eq_ignore_ascii_case(c))).flatten()
}

// ---- what this PC has (read, cached a little) -------------------------------------------

/// How long a reading of the registry stands (a browser installed meanwhile
/// shows after that).
const FRESH: Duration = Duration::from_secs(30);

/// A reading of this PC's browsers: when, the browsers, the default's program.
type Reading = (Instant, Vec<Browser>, Option<String>);

static CACHE: Mutex<Option<Reading>> = Mutex::new(None);

/// The browsers installed and Windows' default browser's program, as read
/// within the last [`FRESH`].
fn read() -> (Vec<Browser>, Option<String>) {
    let mut cache = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    if let Some((at, list, default)) = cache.as_ref() {
        if at.elapsed() < FRESH {
            return (list.clone(), default.clone());
        }
    }
    let (raw, default) = if crate::shell::e2e::active() { (e2e_raw(), None) } else { (registry::browsers(), registry::default_exe()) };
    let list = parse(&raw, &|exe| crate::shell::e2e::active() || Path::new(exe).is_file());
    *cache = Some((Instant::now(), list.clone(), default.clone()));
    (list, default)
}

/// Test mode's browsers (`YAP_E2E_BROWSERS`).
fn e2e_raw() -> Vec<Raw> {
    std::env::var("YAP_E2E_BROWSERS").ok().and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default()
}

/// The browsers installed.
pub fn installed() -> Vec<Browser> {
    read().0
}

/// The browser links open in now (`None`: Windows' default).
pub fn chosen() -> Option<Browser> {
    let cfg = crate::config::load();
    chosen_in(&cfg.open_links_in, &installed()).cloned()
}

/// "Brave", or "your browser" (see [`label_for`]).
pub fn label() -> String {
    let cfg = crate::config::load();
    let (list, default) = read();
    label_for(&cfg.open_links_in, &list, default.as_deref())
}

/// **Open a web or mail link**: a web link in the chosen browser (its
/// program, the address its one argument), else — and every mail link —
/// Windows' default. The caller checked it's a web or mail link.
pub fn open(app: &tauri::AppHandle, url: &str) -> Result<(), String> {
    let web = url.to_ascii_lowercase().starts_with("http");
    if let Some(b) = chosen().filter(|_| web) {
        let started = std::process::Command::new(&b.exe)
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        match started {
            Ok(_) => {
                tracing::info!(browser = %b.id, "Opened a link in the chosen browser");
                return Ok(());
            }
            Err(e) => tracing::warn!(browser = %b.id, "The chosen browser didn't start ({e}); using Windows' default"),
        }
    }
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_url(url, None::<&str>).map_err(|e| format!("Couldn't open your browser: {e}"))
}

/// Settings' list: the browsers installed (with their icons), the one
/// chosen, and Windows' default's name.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct List {
    pub browsers: Vec<Browser>,
    /// What "Default browser" opens, when Yap can tell ("Microsoft Edge").
    pub default_name: Option<String>,
}

/// Settings → General → Links: the browsers this PC has.
#[tauri::command]
pub async fn browsers_list() -> Result<List, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let (list, default) = read();
        let default_name = default.as_deref().and_then(|e| matching(&list, e)).map(|b| b.name.clone());
        let browsers = list
            .into_iter()
            .map(|mut b| {
                if !crate::shell::e2e::active() {
                    b.icon = crate::calls::app_icons::exe_icon_url(Path::new(&b.exe));
                }
                b
            })
            .collect();
        List { browsers, default_name }
    })
    .await
    .map_err(|e| e.to_string())
}

/// The name a link button says ("Open in Brave"; "your browser").
#[tauri::command]
pub async fn browser_label() -> String {
    tauri::async_runtime::spawn_blocking(label).await.unwrap_or_else(|_| "your browser".to_string())
}

#[cfg(windows)]
mod registry {
    use std::ffi::c_void;

    use windows::core::{HSTRING, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE,
        KEY_READ, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ,
    };

    use super::Raw;

    struct Key(HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: a key this owns, closed once.
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }

    fn open(root: HKEY, path: &str) -> Option<Key> {
        let mut key = HKEY::default();
        // SAFETY: reads only; the key is closed by `Key`.
        (unsafe { RegOpenKeyExW(root, &HSTRING::from(path), None, KEY_READ, &mut key) } == ERROR_SUCCESS).then_some(Key(key))
    }

    /// A string value (`name`: `None` for the default) of `sub` under `key`.
    fn string(key: &Key, sub: &str, name: Option<&str>) -> Option<String> {
        let mut value = vec![0u16; 2048];
        let mut size = (value.len() * 2) as u32;
        let name = name.map(HSTRING::from);
        let name_ptr = name.as_ref().map_or(PCWSTR::null(), |n| PCWSTR(n.as_ptr()));
        // SAFETY: a buffer of `size` bytes; Windows writes at most that.
        let r = unsafe {
            RegGetValueW(
                key.0,
                &HSTRING::from(sub),
                name_ptr,
                RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ,
                None,
                Some(value.as_mut_ptr() as *mut c_void),
                Some(&mut size),
            )
        };
        if r != ERROR_SUCCESS {
            return None;
        }
        let chars = (size as usize / 2).min(value.len());
        let s = String::from_utf16_lossy(&value[..chars]).trim_end_matches('\0').trim().to_string();
        (!s.is_empty()).then_some(s)
    }

    /// Every registered browser, HKCU first.
    pub fn browsers() -> Vec<Raw> {
        let mut out = Vec::new();
        for (root, path) in [
            (HKEY_CURRENT_USER, r"SOFTWARE\Clients\StartMenuInternet"),
            (HKEY_LOCAL_MACHINE, r"SOFTWARE\Clients\StartMenuInternet"),
            (HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Clients\StartMenuInternet"),
        ] {
            let Some(key) = open(root, path) else { continue };
            for i in 0..200u32 {
                let mut buf = [0u16; 260];
                let mut len = buf.len() as u32;
                // SAFETY: a buffer of `len` characters.
                let r = unsafe { RegEnumKeyExW(key.0, i, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None) };
                if r == ERROR_NO_MORE_ITEMS {
                    break;
                }
                if r != ERROR_SUCCESS {
                    continue;
                }
                let sub = String::from_utf16_lossy(&buf[..len as usize]);
                let name = string(&key, &sub, None).unwrap_or_default();
                let command = string(&key, &format!(r"{sub}\shell\open\command"), None).unwrap_or_default();
                out.push(Raw { key: sub, name, command });
            }
        }
        out
    }

    /// Windows' default browser's program: the https handler the person
    /// chose (its ProgId), and that ProgId's command.
    pub fn default_exe() -> Option<String> {
        let choice = open(HKEY_CURRENT_USER, r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https")?;
        let prog = string(&choice, "UserChoice", Some("ProgId"))?;
        let classes = open(HKEY_CLASSES_ROOT, &prog)?;
        let command = string(&classes, r"shell\open\command", None)?;
        super::exe_of(&command)
    }
}

#[cfg(not(windows))]
mod registry {
    pub fn browsers() -> Vec<super::Raw> {
        Vec::new()
    }
    pub fn default_exe() -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(key: &str, name: &str, command: &str) -> Raw {
        Raw { key: key.into(), name: name.into(), command: command.into() }
    }

    /// A PC's registered browsers, as `StartMenuInternet` lists them
    /// (HKCU, then HKLM, then HKLM's 32-bit view).
    fn listed() -> Vec<Raw> {
        vec![
            raw("Brave", "Brave", r#""C:\Users\Example\AppData\Local\BraveSoftware\Brave-Browser\Application\brave.exe""#),
            raw("Google Chrome", "Google Chrome", r#""C:\Program Files\Google\Chrome\Application\chrome.exe""#),
            raw("Firefox-308046B0AF4A39CB", "Firefox", r#""C:\Program Files\Mozilla Firefox\firefox.exe" -osint -url "%1""#),
            raw("Microsoft Edge", "", r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe --single-argument %1"),
            raw("IEXPLORE.EXE", "Internet Explorer", r#""C:\Program Files\Internet Explorer\iexplore.exe""#),
            // HKLM's entry of a browser HKCU had: the first wins.
            raw("brave", "Brave (machine)", r#""C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe""#),
            // A resource-string name: the key's instead.
            raw("Opera", "@C:\\Program Files\\Opera\\opera.dll,-101", r#""C:\Program Files\Opera\opera.exe""#),
            // Uninstalled (its program is gone), and one with no command.
            raw("Vivaldi", "Vivaldi", r#""C:\Program Files\Vivaldi\Application\vivaldi.exe""#),
            raw("Broken", "Broken", ""),
        ]
    }

    #[test]
    fn the_registered_browsers_are_read_once_each_with_a_program_and_a_name() {
        let exists = |exe: &str| !exe.contains("Vivaldi");
        let browsers = parse(&listed(), &exists);
        let names: Vec<(&str, &str)> = browsers.iter().map(|b| (b.id.as_str(), b.name.as_str())).collect();
        assert_eq!(
            names,
            [
                ("Brave", "Brave"),
                ("Firefox-308046B0AF4A39CB", "Firefox"),
                ("Google Chrome", "Google Chrome"),
                ("Microsoft Edge", "Microsoft Edge"),
                ("Opera", "Opera"),
            ]
        );
        assert_eq!(browsers[1].exe, r"C:\Program Files\Mozilla Firefox\firefox.exe");
        assert_eq!(browsers[3].exe, r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe");
        assert!(browsers.iter().all(|b| b.icon.is_none()));
    }

    #[test]
    fn a_commands_program_is_its_first_part() {
        assert_eq!(exe_of(r#""C:\A B\brave.exe" --x "%1""#).as_deref(), Some(r"C:\A B\brave.exe"));
        assert_eq!(exe_of(r"C:\Program Files\Edge\msedge.exe --single-argument %1").as_deref(), Some(r"C:\Program Files\Edge\msedge.exe"));
        assert_eq!(exe_of("  C:\\x\\Chrome.EXE").as_deref(), Some("C:\\x\\Chrome.EXE"));
        assert_eq!(exe_of(""), None);
        assert_eq!(exe_of(r#""C:\scripts\open.bat" %1"#), None);
        assert_eq!(exe_of("rundll32 url.dll,FileProtocolHandler %1"), None);
    }

    #[test]
    fn a_link_buttons_name_is_the_chosen_browser_or_the_default() {
        let browsers = parse(&listed(), &|_| true);
        // Chosen: its name, whatever the default.
        assert_eq!(label_for("Brave", &browsers, Some(r"C:\Program Files\Google\Chrome\Application\chrome.exe")), "Brave");
        assert_eq!(label_for("brave", &browsers, None), "Brave");
        // Windows' default: the browser with its program (by path, else by file name).
        assert_eq!(label_for("", &browsers, Some(r"C:\Program Files\Google\Chrome\Application\chrome.exe")), "Google Chrome");
        assert_eq!(label_for("", &browsers, Some(r"D:\Apps\Firefox\firefox.exe")), "Firefox");
        // A choice that's gone, or a default Yap can't tell: "your browser".
        assert_eq!(label_for("Netscape", &browsers, None), "your browser");
        assert_eq!(label_for("", &browsers, Some(r"C:\Tools\unknown.exe")), "your browser");
        assert!(chosen_in("", &browsers).is_none());
        assert_eq!(chosen_in("Opera", &browsers).map(|b| b.exe.as_str()), Some(r"C:\Program Files\Opera\opera.exe"));
    }
}

//! The registry's side of finding editors (read only): App Paths, the
//! Uninstall keys' installs, and what Windows says about a file type.

use std::ffi::c_void;
use std::path::PathBuf;

use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
use windows::Win32::System::Registry::{RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ};
use windows::Win32::UI::Shell::{AssocIsDangerous, AssocQueryStringW, ASSOCF_INIT_IGNOREUNKNOWN, ASSOCF_NOTRUNCATE, ASSOCSTR_EXECUTABLE};

use super::Installed;

const APP_PATHS: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths";
const UNINSTALL: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";
const UNINSTALL_32: &str = r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";
/// The most installs looked at in one Uninstall key.
const MAX_KEYS: u32 = 4_000;

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: a key this owns, closed once.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

/// A string value (`value` None: the key's default) of `root\sub`, environment strings expanded.
fn string(root: HKEY, sub: &str, value: Option<&str>) -> Option<String> {
    let mut buf = vec![0u16; 2_048];
    let mut size = (buf.len() * 2) as u32;
    let name = value.map(HSTRING::from);
    let name_ptr = name.as_ref().map(|n| PCWSTR(n.as_ptr())).unwrap_or(PCWSTR::null());
    // SAFETY: reads only, into a buffer of the size given.
    let r = unsafe { RegGetValueW(root, &HSTRING::from(sub), name_ptr, RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut c_void), Some(&mut size)) };
    if r != ERROR_SUCCESS {
        return None;
    }
    let chars = (size as usize / 2).min(buf.len());
    let s = String::from_utf16_lossy(&buf[..chars]).trim_end_matches('\0').trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// A path as the registry writes one: quotes and an icon's `,0` gone.
fn path_of(s: &str) -> Option<PathBuf> {
    let s = s.trim();
    let s = if let Some(rest) = s.strip_prefix('"') { rest.split('"').next().unwrap_or("") } else { s.rsplit_once(',').filter(|(_, n)| n.trim().trim_start_matches('-').bytes().all(|b| b.is_ascii_digit())).map(|(p, _)| p).unwrap_or(s) };
    let s = s.trim();
    (!s.is_empty()).then(|| PathBuf::from(s))
}

/// **App Paths' programs among `wanted`** (file names): each one's path, the
/// person's own first.
pub fn app_paths(wanted: &[&str]) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        for w in wanted {
            if let Some(p) = string(root, &format!(r"{APP_PATHS}\{w}"), None).as_deref().and_then(path_of) {
                out.push((w.to_ascii_lowercase(), p));
            }
        }
    }
    out
}

/// The subkeys' names of `root\sub`.
fn subkeys(root: HKEY, sub: &str) -> Vec<String> {
    let mut key = HKEY::default();
    // SAFETY: reads only; the key is closed by `Key`.
    if unsafe { RegOpenKeyExW(root, &HSTRING::from(sub), None, KEY_READ, &mut key) } != ERROR_SUCCESS {
        return Vec::new();
    }
    let key = Key(key);
    let mut out = Vec::new();
    for i in 0..MAX_KEYS {
        let mut buf = [0u16; 260];
        let mut len = buf.len() as u32;
        // SAFETY: a buffer of the length given.
        let r = unsafe { RegEnumKeyExW(key.0, i, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None) };
        if r == ERROR_NO_MORE_ITEMS {
            break;
        }
        if r == ERROR_SUCCESS {
            out.push(String::from_utf16_lossy(&buf[..len as usize]));
        }
    }
    out
}

/// **The Uninstall registry's installs**: each one's name, folder and icon.
pub fn installed() -> Vec<Installed> {
    let mut out = Vec::new();
    for (root, base) in [(HKEY_CURRENT_USER, UNINSTALL), (HKEY_LOCAL_MACHINE, UNINSTALL), (HKEY_LOCAL_MACHINE, UNINSTALL_32)] {
        for name in subkeys(root, base) {
            let sub = format!(r"{base}\{name}");
            let Some(display) = string(root, &sub, Some("DisplayName")) else { continue };
            let location = string(root, &sub, Some("InstallLocation")).as_deref().and_then(path_of);
            let icon = string(root, &sub, Some("DisplayIcon")).as_deref().and_then(path_of).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")));
            if location.is_some() || icon.is_some() {
                out.push(Installed { name: display, location, icon });
            }
        }
    }
    out
}

/// **Windows' own word on a type** (`AssocIsDangerous`: what it warns
/// about before opening, a program's or a script's).
pub fn dangerous(ext: &str) -> bool {
    let dotted = HSTRING::from(format!(".{ext}"));
    // SAFETY: a string Windows only reads.
    unsafe { AssocIsDangerous(&dotted) }.as_bool()
}

/// **The program a type opens with** (`AssocQueryStringW`, its Open verb's
/// executable), if Windows says.
pub fn app_for(ext: &str) -> Option<String> {
    let dotted = HSTRING::from(format!(".{ext}"));
    let verb = HSTRING::from("open");
    let mut buf = vec![0u16; 1_024];
    let mut len = buf.len() as u32;
    // SAFETY: a buffer of the length given; Windows only reads the strings.
    let r = unsafe { AssocQueryStringW(ASSOCF_NOTRUNCATE | ASSOCF_INIT_IGNOREUNKNOWN, ASSOCSTR_EXECUTABLE, &dotted, &verb, Some(PWSTR(buf.as_mut_ptr())), &mut len) };
    if r.is_err() {
        return None;
    }
    let s = String::from_utf16_lossy(&buf[..(len as usize).min(buf.len())]).trim_end_matches('\0').trim().to_string();
    (!s.is_empty()).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_paths_lose_their_quotes_and_icon_numbers() {
        assert_eq!(path_of(r#""C:\Program Files\Notepad++\notepad++.exe""#), Some(PathBuf::from(r"C:\Program Files\Notepad++\notepad++.exe")));
        assert_eq!(path_of(r"C:\Users\sam\AppData\Local\Programs\Microsoft VS Code\Code.exe,0"), Some(PathBuf::from(r"C:\Users\sam\AppData\Local\Programs\Microsoft VS Code\Code.exe")));
        assert_eq!(path_of(r#""C:\Tools\Zed\Zed.exe",-101"#), Some(PathBuf::from(r"C:\Tools\Zed\Zed.exe")));
        assert_eq!(path_of("  "), None);
    }

    #[test]
    fn windows_calls_a_script_dangerous_and_a_text_file_not() {
        assert!(dangerous("exe") && dangerous("bat") && dangerous("vbs"));
        assert!(!dangerous("txt"));
    }
}

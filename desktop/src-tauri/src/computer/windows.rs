//! **The windows on screen, and whose they are**: every top-level window
//! that draws something (visible, not minimised, not cloaked on another
//! virtual desktop, not click-through), top to bottom, each with its frame
//! in physical pixels (`DWMWA_EXTENDED_FRAME_BOUNDS`, as Snip's window pick
//! reads it: `snip/pick.rs`), its process's exe and the app it belongs to —
//! what masking a screenshot ([`super::shot`]) and the allowlist
//! ([`super::grants`]) decide from.
//!
//! - **A Store app's window** belongs to `ApplicationFrameHost.exe`; the
//!   app itself is the process of its hosted `Windows.UI.Core.CoreWindow`.
//! - **A packaged app** (MSIX: the new Notepad, Terminal, Paint…) runs from
//!   `…\WindowsApps\<package full name>\…`; its package family
//!   ([`package_family`]) is the AppUserModelID's part before the `!`.
//! - **A window's own AppUserModelID**, where the app set one (its property
//!   store), names it too.
//!
//! Never a window's title or contents: rectangles, classes and exe paths only.

use crate::snip::geom::Rect;

/// **One window on screen**, topmost first in a list.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Win {
    pub hwnd: isize,
    pub pid: u32,
    /// Its visible frame, physical pixels on the virtual desktop.
    pub rect: Rect,
    pub class: String,
    /// The exe's full path, lower case ("" when Windows won't say).
    pub exe: String,
    /// Its AppUserModelID, lower case: the window's own, else its package's
    /// family for a packaged app ("microsoft.windowsnotepad_8wekyb3d8bbwe").
    pub aumid: String,
    /// One of Yap's own windows.
    pub own: bool,
}

impl Win {
    /// The exe's name without its folder and ".exe" ("notepad").
    pub fn stem(&self) -> String {
        crate::pasting::app_kind::exe_stem(&self.exe)
    }
}

/// The desktop behind every window (`Progman` / `WorkerW`: the wallpaper and
/// its icons) and the taskbar: File Explorer's, whatever process draws them.
pub const SHELL_CLASSES: &[&str] = &["Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd"];

/// **A packaged app's family**, from its exe's path under `WindowsApps`
/// ("…\WindowsApps\Microsoft.WindowsNotepad_11.2410.21.0_x64__8wekyb3d8bbwe\…"
/// **A window's app id**: its own AppUserModelID; else, for a window another
/// of the same process owns (a dialog, a popup: `GA_ROOTOWNER`), its
/// owner's, so a file dialog LibreOffice Calc opens is Calc's (LibreOffice
/// runs every app in one process, and only its frames carry an id); else
/// its package's family. Pure. (A real-screen run, 20 Nov 2026: Calc's Open
/// dialog was masked as an app not allowed.)
pub fn app_id_of(own: String, owner: Option<String>, package: Option<String>) -> String {
    if !own.is_empty() {
        return own;
    }
    if let Some(o) = owner.filter(|o| !o.is_empty()) {
        return o;
    }
    package.unwrap_or_default()
}

/// → "microsoft.windowsnotepad_8wekyb3d8bbwe"): the package's name and its
/// publisher's id, the part of its AppUserModelID before the `!`. `None`:
/// not a packaged app. Pure.
pub fn package_family(exe: &str) -> Option<String> {
    let lower = exe.to_lowercase().replace('/', "\\");
    let (_, rest) = lower.split_once("\\windowsapps\\")?;
    let full = rest.split('\\').next()?;
    // name_version_arch_resource_publisher: the resource id is often empty ("__").
    let name = full.split('_').next()?;
    let publisher = full.rsplit('_').next()?;
    (!name.is_empty() && !publisher.is_empty() && full.matches('_').count() >= 4).then(|| format!("{name}_{publisher}"))
}

/// What [`shown`] decides from: the window as Windows reports it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Seen {
    pub visible: bool,
    pub minimized: bool,
    pub cloaked: bool,
    pub click_through: bool,
    pub rect: Rect,
}

/// **Whether a window is drawn on screen**, as far as a screenshot is
/// concerned: visible, not minimised, not cloaked (another virtual desktop,
/// a suspended Store app, Windows' hidden helpers), at least 6 px each way,
/// and not click-through — a click-through window is an overlay that
/// covers a whole display and draws little (a game's, a recorder's): masked
/// whole, it would hide everything. Pure.
pub fn shown(s: &Seen) -> bool {
    s.visible && !s.minimized && !s.cloaked && !s.click_through && s.rect.w >= crate::snip::geom::MIN_SIDE && s.rect.h >= crate::snip::geom::MIN_SIDE
}

/// **Every window on screen, topmost first** (blocking: one enumeration,
/// well under a few milliseconds).
#[cfg(windows)]
pub fn on_screen() -> Vec<Win> {
    win::list()
}

#[cfg(not(windows))]
pub fn on_screen() -> Vec<Win> {
    Vec::new()
}

/// **The window in front** (its root), whatever it is.
#[cfg(windows)]
pub fn front() -> Option<Win> {
    win::front()
}

#[cfg(not(windows))]
pub fn front() -> Option<Win> {
    None
}

/// **The window under a physical point** (its root).
#[cfg(windows)]
pub fn at(x: i32, y: i32) -> Option<Win> {
    win::at(x, y)
}

#[cfg(not(windows))]
pub fn at(_x: i32, _y: i32) -> Option<Win> {
    None
}

/// **Every minimised window** (visible, not cloaked), for
/// `open_application` to bring back (blocking).
#[cfg(windows)]
pub fn minimised() -> Vec<Win> {
    win::minimised()
}

#[cfg(not(windows))]
pub fn minimised() -> Vec<Win> {
    Vec::new()
}

/// **A window to the front**: restored if it's minimised, then given the
/// focus (`text_injector::focus`: Windows' focus rules respected). Whether
/// it's in front now (blocking).
#[cfg(windows)]
pub fn raise(hwnd: isize) -> bool {
    win::raise(hwnd)
}

#[cfg(not(windows))]
pub fn raise(_hwnd: isize) -> bool {
    false
}

/// The pointer, in physical pixels on the virtual desktop.
#[cfg(windows)]
pub fn cursor() -> Option<(i32, i32)> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
    let mut p = POINT::default();
    unsafe { GetCursorPos(&mut p) }.ok()?;
    Some((p.x, p.y))
}

#[cfg(not(windows))]
pub fn cursor() -> Option<(i32, i32)> {
    None
}

#[cfg(windows)]
mod win {
    use std::collections::HashMap;
    use std::ffi::c_void;

    use windows::core::{BOOL, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, PROPERTYKEY, RECT};
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToStringAlloc};
    use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, FindWindowExW, GetAncestor, GetClassNameW, GetForegroundWindow, GetWindowLongPtrW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, WindowFromPoint, GA_ROOT, GA_ROOTOWNER, GWL_EXSTYLE, WS_EX_TRANSPARENT,
    };

    use super::{app_id_of, package_family, shown, Seen, Win};
    use crate::snip::geom::Rect;

    /// `PKEY_AppUserModel_ID` ({9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3}, 5).
    const AUMID_KEY: PROPERTYKEY = PROPERTYKEY { fmtid: windows::core::GUID::from_u128(0x9F4C2855_9F79_4B39_A8D0_E1D42DE1D5F3), pid: 5 };

    fn frame(hwnd: HWND) -> Option<Rect> {
        let mut r = RECT::default();
        unsafe { DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut RECT as *mut _, std::mem::size_of::<RECT>() as u32) }.ok()?;
        Some(Rect::new(r.left, r.top, r.right - r.left, r.bottom - r.top))
    }

    fn class(hwnd: HWND) -> String {
        let mut buf = [0u16; 128];
        let n = unsafe { GetClassNameW(hwnd, &mut buf) };
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    fn cloaked(hwnd: HWND) -> bool {
        let mut v: u32 = 0;
        unsafe { DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut v as *mut u32 as *mut _, 4) }.is_ok() && v != 0
    }

    fn pid_of(hwnd: HWND) -> u32 {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        pid
    }

    /// The window's own AppUserModelID, lower case ("" when it set none).
    fn own_aumid(hwnd: HWND) -> String {
        let Ok(store) = (unsafe { SHGetPropertyStoreForWindow::<IPropertyStore>(hwnd) }) else { return String::new() };
        // SAFETY: the value the shell filled, read as a string (freed once
        // read), then cleared.
        unsafe {
            let Ok(mut value) = store.GetValue(&AUMID_KEY) else { return String::new() };
            let s = match PropVariantToStringAlloc(&value) {
                Ok(p) => {
                    let s = p.to_string().unwrap_or_default();
                    CoTaskMemFree(Some(p.0 as *const c_void));
                    s
                }
                Err(_) => String::new(),
            };
            let _ = PropVariantClear(&mut value);
            s.to_lowercase()
        }
    }

    /// A Store app's frame: the process of its hosted CoreWindow.
    fn hosted_pid(hwnd: HWND) -> Option<u32> {
        let class: Vec<u16> = "Windows.UI.Core.CoreWindow\0".encode_utf16().collect();
        let child = unsafe { FindWindowExW(Some(hwnd), None, PCWSTR(class.as_ptr()), PCWSTR::null()) }.ok()?;
        let pid = pid_of(child);
        (pid != 0).then_some(pid)
    }

    struct Walk {
        out: Vec<Win>,
        exes: HashMap<u32, String>,
        own: u32,
    }

    impl Walk {
        fn exe(&mut self, pid: u32) -> String {
            self.exes.entry(pid).or_insert_with(|| crate::shell::procs::image_path(pid).unwrap_or_default().to_lowercase()).clone()
        }
    }

    unsafe extern "system" fn each(hwnd: HWND, lp: LPARAM) -> BOOL {
        let walk = unsafe { &mut *(lp.0 as *mut Walk) };
        let visible = unsafe { IsWindowVisible(hwnd) }.as_bool();
        if !visible {
            return BOOL(1);
        }
        let ex = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
        let rect = frame(hwnd).unwrap_or_default();
        let seen = Seen { visible, minimized: unsafe { IsIconic(hwnd) }.as_bool(), cloaked: cloaked(hwnd), click_through: ex & WS_EX_TRANSPARENT.0 != 0, rect };
        if !shown(&seen) {
            return BOOL(1);
        }
        let w = read(walk, hwnd, rect);
        walk.out.push(w);
        BOOL(1)
    }

    /// One window as the list has it (its process, exe, app).
    fn read(walk: &mut Walk, hwnd: HWND, rect: Rect) -> Win {
        let mut pid = pid_of(hwnd);
        let class = class(hwnd);
        if class == "ApplicationFrameWindow" {
            if let Some(hosted) = hosted_pid(hwnd) {
                pid = hosted;
            }
        }
        let exe = walk.exe(pid);
        let own_id = own_aumid(hwnd);
        // A dialog or popup with no id of its own: the id of the window of its process that owns it.
        let owner = own_id
            .is_empty()
            .then(|| {
                let root = unsafe { GetAncestor(hwnd, GA_ROOTOWNER) };
                (!root.0.is_null() && root != hwnd && pid_of(root) == pid_of(hwnd)).then(|| own_aumid(root))
            })
            .flatten();
        let aumid = app_id_of(own_id, owner, package_family(&exe));
        let own = pid == walk.own;
        Win { hwnd: hwnd.0 as isize, pid, rect, class, exe, aumid, own }
    }

    fn one(hwnd: HWND) -> Option<Win> {
        if hwnd.0.is_null() {
            return None;
        }
        let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
        let hwnd = if root.0.is_null() { hwnd } else { root };
        let mut walk = Walk { out: Vec::new(), exes: HashMap::new(), own: std::process::id() };
        let rect = frame(hwnd).unwrap_or_default();
        Some(read(&mut walk, hwnd, rect))
    }

    pub fn front() -> Option<Win> {
        one(unsafe { GetForegroundWindow() })
    }

    pub fn at(x: i32, y: i32) -> Option<Win> {
        one(unsafe { WindowFromPoint(windows::Win32::Foundation::POINT { x, y }) })
    }

    pub fn list() -> Vec<Win> {
        let mut walk = Walk { out: Vec::new(), exes: HashMap::new(), own: std::process::id() };
        unsafe {
            let _ = EnumWindows(Some(each), LPARAM(&mut walk as *mut Walk as isize));
        }
        walk.out
    }

    unsafe extern "system" fn each_minimised(hwnd: HWND, lp: LPARAM) -> BOOL {
        let walk = unsafe { &mut *(lp.0 as *mut Walk) };
        if unsafe { IsWindowVisible(hwnd) }.as_bool() && unsafe { IsIconic(hwnd) }.as_bool() && !cloaked(hwnd) {
            let w = read(walk, hwnd, frame(hwnd).unwrap_or_default());
            walk.out.push(w);
        }
        BOOL(1)
    }

    pub fn minimised() -> Vec<Win> {
        let mut walk = Walk { out: Vec::new(), exes: HashMap::new(), own: std::process::id() };
        unsafe {
            let _ = EnumWindows(Some(each_minimised), LPARAM(&mut walk as *mut Walk as isize));
        }
        walk.out
    }

    pub fn raise(hwnd: isize) -> bool {
        use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_RESTORE};
        let h = HWND(hwnd as *mut c_void);
        if unsafe { IsIconic(h) }.as_bool() {
            let _ = unsafe { ShowWindow(h, SW_RESTORE) };
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        crate::pasting::text_injector::focus(hwnd);
        std::thread::sleep(std::time::Duration::from_millis(100));
        let front = unsafe { GetAncestor(GetForegroundWindow(), GA_ROOT) };
        front == h
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dialog_with_no_id_of_its_own_is_its_owners_app() {
        // A window's own id wins.
        assert_eq!(app_id_of("thedocumentfoundation.libreoffice.calc".into(), Some("other".into()), None), "thedocumentfoundation.libreoffice.calc");
        // LibreOffice Calc's Open dialog: no id, owned by Calc's frame in the same process.
        assert_eq!(app_id_of(String::new(), Some("thedocumentfoundation.libreoffice.calc".into()), None), "thedocumentfoundation.libreoffice.calc");
        // No owner's id: the package's family, as before; else none.
        assert_eq!(app_id_of(String::new(), Some(String::new()), Some("microsoft.paint_8wekyb3d8bbwe".into())), "microsoft.paint_8wekyb3d8bbwe");
        assert_eq!(app_id_of(String::new(), None, None), "");
    }

    #[test]
    fn a_packaged_apps_family_comes_from_its_folder() {
        assert_eq!(
            package_family(r"C:\Program Files\WindowsApps\Microsoft.WindowsNotepad_11.2410.21.0_x64__8wekyb3d8bbwe\Notepad\Notepad.exe").as_deref(),
            Some("microsoft.windowsnotepad_8wekyb3d8bbwe")
        );
        assert_eq!(
            package_family(r"c:\program files\windowsapps\Microsoft.WindowsTerminal_1.21.3231.0_x64__8wekyb3d8bbwe\WindowsTerminal.exe").as_deref(),
            Some("microsoft.windowsterminal_8wekyb3d8bbwe")
        );
        assert_eq!(package_family(r"C:\Program Files\Mozilla Firefox\firefox.exe"), None);
        assert_eq!(package_family(r"C:\Program Files\WindowsApps\odd\x.exe"), None);
        assert_eq!(package_family(""), None);
    }

    /// The real list on this PC: read only, counts only (never a title, an
    /// exe or an app's name in the output).
    #[cfg(windows)]
    #[test]
    #[ignore = "this machine's windows"]
    fn this_machine() {
        let t = std::time::Instant::now();
        let list = on_screen();
        let with_exe = list.iter().filter(|w| !w.exe.is_empty()).count();
        let with_aumid = list.iter().filter(|w| !w.aumid.is_empty()).count();
        let own = list.iter().filter(|w| w.own).count();
        let shell = list.iter().filter(|w| SHELL_CLASSES.contains(&w.class.as_str())).count();
        println!("{} windows in {:.1} ms: {with_exe} with an exe, {with_aumid} with an AppUserModelID, {own} of Yap's, {shell} of the shell's", list.len(), t.elapsed().as_secs_f64() * 1000.0);
        assert!(list.iter().all(|w| !w.rect.is_empty()));
    }

    #[test]
    fn only_windows_that_draw_something_count() {
        let w = Seen { visible: true, rect: Rect::new(0, 0, 800, 600), ..Default::default() };
        assert!(shown(&w));
        for not in [
            Seen { visible: false, ..w.clone() },
            Seen { minimized: true, ..w.clone() },
            Seen { cloaked: true, ..w.clone() },
            Seen { click_through: true, ..w.clone() },
            Seen { rect: Rect::new(0, 0, 5, 600), ..w.clone() },
        ] {
            assert!(!shown(&not), "{not:?}");
        }
        assert_eq!(Win { exe: r"c:\windows\system32\notepad.exe".into(), ..Default::default() }.stem(), "notepad");
    }
}

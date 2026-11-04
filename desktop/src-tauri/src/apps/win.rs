//! The Windows side: the Start menu's app list (`shell:AppsFolder`) and
//! the shortcuts and App Paths behind it, opening an entry and bringing
//! it forward, its icon.

use std::ffi::c_void;

use windows::core::{Interface, GUID, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS, HWND, LPARAM, PROPERTYKEY, SIZE,
};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantClear;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_LOCAL_SERVER,
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
};
use windows::Win32::System::Variant::{VARENUM, VT_UI1, VT_VECTOR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER,
    HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ,
};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId, GetProcessId};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    ApplicationActivationManager, IApplicationActivationManager, IEnumShellItems, IShellItem,
    IShellItem2, IShellItemImageFactory, ILGetSize, SHCreateItemFromIDList,
    SHCreateItemFromParsingName, SHGetIDListFromObject, SHGetKnownFolderItem,
    SHGetKnownFolderPath, SHGetPathFromIDListEx, ShellExecuteExW, AO_NONE, BHID_EnumItems,
    FOLDERID_AppsFolder, FOLDERID_CommonPrograms, FOLDERID_Programs, GPFIDL_DEFAULT,
    KF_FLAG_DEFAULT, SEE_MASK_FLAG_NO_UI, SEE_MASK_IDLIST, SEE_MASK_NOASYNC,
    SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, SIGDN, SIGDN_NORMALDISPLAY,
    SIGDN_PARENTRELATIVEPARSING, SIIGBF_ICONONLY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, BringWindowToTop, EnumWindows,
    GetForegroundWindow, GetWindow, GetWindowTextLengthW, GetWindowThreadProcessId,
    IsWindowVisible, SetForegroundWindow, ASFW_ANY, GW_OWNER, SW_SHOWNORMAL,
};

use super::{App, Source};

/// Run `f` on a thread of its own with COM set up for the shell (a
/// single-threaded apartment), and wait for it.
pub fn on_sta<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    std::thread::spawn(move || {
        // SAFETY: this thread's own COM, undone when it ends.
        let ok = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }.is_ok();
        let out = f();
        if ok {
            unsafe { CoUninitialize() };
        }
        out
    })
    .join()
    .ok()
}

fn display(item: &IShellItem, sigdn: SIGDN) -> Option<String> {
    // SAFETY: the string the shell allocated, freed once read.
    unsafe {
        let p = item.GetDisplayName(sigdn).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const c_void));
        s
    }
}

/// An item's absolute ID list, as bytes.
fn id_list(item: &IShellItem) -> Vec<u8> {
    // SAFETY: the ID list the shell allocated: `ILGetSize` bytes,
    // copied, then freed.
    unsafe {
        let Ok(pidl) = SHGetIDListFromObject(item) else { return Vec::new() };
        let len = ILGetSize(Some(pidl)) as usize;
        let bytes = if len > 0 && len < 64 * 1024 {
            std::slice::from_raw_parts(pidl as *const u8, len).to_vec()
        } else {
            Vec::new()
        };
        CoTaskMemFree(Some(pidl as *const c_void));
        bytes
    }
}

/// The Start menu's "All apps": `shell:AppsFolder`'s children, each with
/// the shortcut behind it ([`link_of`]). Needs COM ([`on_sta`]).
pub fn apps_folder() -> Result<Vec<App>, String> {
    let roots = start_menu_roots();
    // SAFETY: COM calls on interfaces this function holds.
    unsafe {
        let folder: IShellItem =
            SHGetKnownFolderItem(&FOLDERID_AppsFolder, KF_FLAG_DEFAULT, None).map_err(|e| e.message())?;
        let items: IEnumShellItems =
            folder.BindToHandler(None, &BHID_EnumItems).map_err(|e| e.message())?;
        let mut out = Vec::new();
        loop {
            let mut slot = [None];
            let mut fetched = 0u32;
            if items.Next(&mut slot, Some(&mut fetched)).is_err() || fetched == 0 {
                break;
            }
            let Some(item) = slot[0].take() else { break };
            let (Some(name), Some(id)) =
                (display(&item, SIGDN_NORMALDISPLAY), display(&item, SIGDN_PARENTRELATIVEPARSING))
            else {
                continue;
            };
            let (target, args, shortcut) = link_of(&item, &roots);
            out.push(App { id, name, source: Source::AppsFolder, target, args, shortcut, pidl: id_list(&item) });
            if out.len() >= 5_000 {
                break;
            }
        }
        Ok(out)
    }
}

/// `System.Link.TargetParsingPath`.
const PKEY_LINK_TARGET: PROPERTYKEY =
    PROPERTYKEY { fmtid: GUID::from_u128(0xb9b4b3fc_2b51_4a42_b5d8_324146afcf25), pid: 2 };
/// `System.Link.Arguments`.
const PKEY_LINK_ARGUMENTS: PROPERTYKEY =
    PROPERTYKEY { fmtid: GUID::from_u128(0x436f2667_14e2_4feb_b30a_146c53b5b674), pid: 100 };
/// `System.AppUserModel.BestShortcut`: the shortcut file behind an
/// app-list entry, as an absolute ID list.
const PKEY_BEST_SHORTCUT: PROPERTYKEY =
    PROPERTYKEY { fmtid: GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3), pid: 10 };

/// The Start menu's two Programs folders (this user's and everyone's),
/// lower case, each ending in `\`.
pub fn start_menu_roots() -> Vec<String> {
    [FOLDERID_Programs, FOLDERID_CommonPrograms]
        .iter()
        .filter_map(|id| {
            // SAFETY: the string the shell allocated, freed once read.
            unsafe {
                let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
                let s = p.to_string().ok();
                CoTaskMemFree(Some(p.0 as *const c_void));
                s
            }
        })
        .map(|s| format!("{}\\", s.trim_end_matches('\\').to_lowercase()))
        .collect()
}

/// What an entry's Start menu shortcut says: its target and arguments
/// (read, never run) and the shortcut file itself — a `.lnk` or `.url`
/// in one of the Start menu's folders (`roots`) that exists. Empty for
/// a packaged app.
fn link_of(item: &IShellItem, roots: &[String]) -> (String, String, Option<String>) {
    let Ok(item) = item.cast::<IShellItem2>() else { return (String::new(), String::new(), None) };
    let text = |key: &PROPERTYKEY| -> String {
        // SAFETY: the string the shell allocated, freed once read.
        unsafe {
            match item.GetString(key) {
                Ok(p) => {
                    let s = p.to_string().unwrap_or_default();
                    CoTaskMemFree(Some(p.0 as *const c_void));
                    s
                }
                Err(_) => String::new(),
            }
        }
    };
    let shortcut = best_shortcut(&item).filter(|path| {
        let lower = path.to_lowercase();
        (lower.ends_with(".lnk") || lower.ends_with(".url"))
            && roots.iter().any(|r| lower.starts_with(r))
            && std::path::Path::new(path).is_file()
    });
    (text(&PKEY_LINK_TARGET), text(&PKEY_LINK_ARGUMENTS), shortcut)
}

/// Does `bytes` hold a whole ID list (its items, then the empty one that
/// ends it)?
pub fn whole_id_list(bytes: &[u8]) -> bool {
    let mut at = 0usize;
    while at + 2 <= bytes.len() {
        let cb = u16::from_le_bytes([bytes[at], bytes[at + 1]]) as usize;
        if cb == 0 {
            return true;
        }
        if cb < 2 {
            return false;
        }
        at += cb;
    }
    false
}

/// The path of an entry's best shortcut (`System.AppUserModel.BestShortcut`).
fn best_shortcut(item: &IShellItem2) -> Option<String> {
    // SAFETY: the PROPVARIANT the shell filled, read as the type it
    // says it is, then cleared.
    unsafe {
        let mut value = item.GetProperty(&PKEY_BEST_SHORTCUT).ok()?;
        let inner = &value.Anonymous.Anonymous;
        let mut path = None;
        if inner.vt == VARENUM(VT_VECTOR.0 | VT_UI1.0) {
            let blob = &inner.Anonymous.caub;
            let len = blob.cElems as usize;
            if !blob.pElems.is_null() && (2..64 * 1024).contains(&len) {
                // The shell's bytes, read before the PROPVARIANT is cleared.
                let bytes = std::slice::from_raw_parts(blob.pElems, len);
                if whole_id_list(bytes) {
                    let mut buf = vec![0u16; 2048];
                    if SHGetPathFromIDListEx(bytes.as_ptr() as *const ITEMIDLIST, &mut buf, GPFIDL_DEFAULT).as_bool() {
                        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                        path = Some(String::from_utf16_lossy(&buf[..end])).filter(|p| !p.is_empty());
                    }
                }
            }
        }
        let _ = PropVariantClear(&mut value);
        path
    }
}

struct Key(HKEY);

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: a key this owns, closed once.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}

/// The App Paths registry's exes (`chrome.exe` → its path), for when the
/// app list can't be read. Only exes that exist.
pub fn app_paths() -> Vec<App> {
    const PATH: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths";
    let mut out = Vec::new();
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let mut key = HKEY::default();
        // SAFETY: reads only; the key is closed by `Key`.
        if unsafe { RegOpenKeyExW(root, &HSTRING::from(PATH), None, KEY_READ, &mut key) } != ERROR_SUCCESS {
            continue;
        }
        let key = Key(key);
        for i in 0..2_000u32 {
            let mut buf = [0u16; 260];
            let mut len = buf.len() as u32;
            let r = unsafe { RegEnumKeyExW(key.0, i, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None) };
            if r == ERROR_NO_MORE_ITEMS {
                break;
            }
            if r != ERROR_SUCCESS {
                continue;
            }
            let sub = String::from_utf16_lossy(&buf[..len as usize]);
            let Some(stem) = sub.strip_suffix(".exe").or_else(|| sub.strip_suffix(".EXE")) else { continue };
            let mut value = [0u16; 1024];
            let mut size = (value.len() * 2) as u32;
            let r = unsafe {
                RegGetValueW(
                    key.0,
                    &HSTRING::from(sub.as_str()),
                    PCWSTR::null(),
                    RRF_RT_REG_SZ,
                    None,
                    Some(value.as_mut_ptr() as *mut c_void),
                    Some(&mut size),
                )
            };
            if r != ERROR_SUCCESS {
                continue;
            }
            let chars = (size as usize / 2).min(value.len());
            let path = String::from_utf16_lossy(&value[..chars]);
            let path = path.trim_end_matches('\0').trim().trim_matches('"').to_string();
            if !path.is_empty() && std::path::Path::new(&path).is_file() {
                out.push(App { id: path, name: stem.to_string(), source: Source::AppPath, ..Default::default() });
            }
        }
    }
    out
}

/// Open an entry (see the module's docs): `Some(pid)` when Windows says
/// which process it started, and how it was opened (for the log). A
/// desktop app goes through the Start menu shortcut behind it, so the
/// shortcut's arguments and working folder apply. Needs COM ([`on_sta`]).
pub fn launch(app: &App) -> Result<(Option<u32>, &'static str), String> {
    // SAFETY: lets the app we start take the foreground; changes nothing
    // else, and fails harmlessly when Yap isn't in front.
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
    if app.packaged() {
        match activate(&app.id) {
            Ok(pid) => return Ok((Some(pid), "app activation")),
            Err(e) if app.pidl.is_empty() => return Err(e),
            Err(e) => tracing::debug!("App activation failed ({e}); opening its entry instead"),
        }
    }
    let shortcut = app.shortcut.as_deref().filter(|f| std::path::Path::new(f).is_file());
    match app.source {
        Source::AppsFolder if shortcut.is_some() => execute(shortcut, None).map(|p| (p, "its Start menu shortcut")),
        Source::AppsFolder if !app.pidl.is_empty() => execute(None, Some(&app.pidl)).map(|p| (p, "its app-list entry")),
        Source::AppsFolder => {
            execute(Some(&format!(r"shell:AppsFolder\{}", app.id)), None).map(|p| (p, "its app-list entry"))
        }
        Source::Shortcut => execute(Some(&app.id), None).map(|p| (p, "its Start menu shortcut")),
        Source::AppPath => execute(Some(&app.id), None).map(|p| (p, "its program file")),
    }
}

/// Start a packaged app by its AppUserModelID.
fn activate(aumid: &str) -> Result<u32, String> {
    // SAFETY: COM on this STA thread.
    unsafe {
        let manager: IApplicationActivationManager =
            CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_LOCAL_SERVER).map_err(|e| e.message())?;
        manager.ActivateApplication(&HSTRING::from(aumid), PCWSTR::null(), AO_NONE).map_err(|e| e.message())
    }
}

/// `ShellExecuteExW` with the default verb on an ID list (an app-list
/// entry) or a file the index found (a shortcut, an exe) — never with
/// arguments of Yap's: a shortcut's own come from the file on disk.
pub fn execute(file: Option<&str>, pidl: Option<&[u8]>) -> Result<Option<u32>, String> {
    let file = file.map(HSTRING::from);
    let mut list = pidl.map(<[u8]>::to_vec).unwrap_or_default();
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_FLAG_NO_UI | SEE_MASK_NOASYNC,
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    if let Some(f) = &file {
        info.lpFile = PCWSTR(f.as_ptr());
    } else {
        info.fMask |= SEE_MASK_IDLIST;
        info.lpIDList = list.as_mut_ptr() as *mut c_void;
    }
    // SAFETY: `info` points at `file` / `list`, alive for the call.
    unsafe {
        ShellExecuteExW(&mut info).map_err(|e| e.message())?;
        if info.hProcess.is_invalid() {
            return Ok(None);
        }
        let pid = GetProcessId(info.hProcess);
        let _ = CloseHandle(info.hProcess);
        Ok(Some(pid).filter(|&p| p != 0))
    }
}

/// `pid`'s main window: visible, not owned, titled.
pub fn main_window(pid: u32) -> Option<HWND> {
    struct Search {
        pid: u32,
        found: Option<HWND>,
    }
    unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        // SAFETY: `lparam` is the `Search` below, alive for EnumWindows.
        let search = unsafe { &mut *(lparam.0 as *mut Search) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        let owned = unsafe { GetWindow(hwnd, GW_OWNER) }.is_ok_and(|o| !o.is_invalid());
        if pid == search.pid
            && unsafe { IsWindowVisible(hwnd) }.as_bool()
            && !owned
            && unsafe { GetWindowTextLengthW(hwnd) } > 0
        {
            search.found = Some(hwnd);
            return false.into();
        }
        true.into()
    }
    let mut search = Search { pid, found: None };
    // SAFETY: the callback only reads windows and writes `search`.
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut search as *mut Search as isize));
    }
    search.found
}

/// Best effort: once the app's window shows (within 8 s), bring it in
/// front, as starting it from the Start menu would. Only that window:
/// the started process's, or that of a process it started since `since`
/// (a launcher handing over, as World of Warcraft's does to Battle.net).
pub fn bring_forward(pid: u32, since: u64) {
    let started = std::time::Instant::now();
    while started.elapsed() < std::time::Duration::from_secs(8) {
        if let Some(hwnd) = crate::shell::procs::family(pid, since).into_iter().find_map(main_window) {
            // SAFETY: input attached to the foreground window's thread
            // for the switch, and detached again.
            unsafe {
                let front = GetForegroundWindow();
                if front == hwnd {
                    return;
                }
                let ours = GetCurrentThreadId();
                let theirs = GetWindowThreadProcessId(front, None);
                let attached = theirs != 0 && theirs != ours && AttachThreadInput(ours, theirs, true).as_bool();
                let _ = SetForegroundWindow(hwnd);
                let _ = BringWindowToTop(hwnd);
                if attached {
                    let _ = AttachThreadInput(ours, theirs, false);
                }
            }
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
}

/// An app's icon as Explorer draws it, `px` square, as a PNG. Needs COM
/// ([`on_sta`]).
pub fn icon_png(app: &App, px: i32) -> Option<Vec<u8>> {
    // SAFETY: COM on this STA thread; the bitmap is deleted once read.
    unsafe {
        let factory: IShellItemImageFactory = if app.pidl.is_empty() {
            SHCreateItemFromParsingName(&HSTRING::from(app.id.as_str()), None).ok()?
        } else {
            SHCreateItemFromIDList(app.pidl.as_ptr() as *const ITEMIDLIST).ok()?
        };
        let bitmap = factory.GetImage(SIZE { cx: px, cy: px }, SIIGBF_ICONONLY).ok()?;
        let rgba = bitmap_rgba(bitmap);
        let _ = DeleteObject(bitmap.into());
        let (w, h, rgba) = rgba?;
        crate::calls::app_icons::encode_png(w, h, &rgba)
    }
}

/// A 32-bit bitmap's pixels as straight RGBA (the shell's are
/// premultiplied).
///
/// # Safety
/// `bitmap` must be a valid HBITMAP; it's left as it is.
unsafe fn bitmap_rgba(bitmap: HBITMAP) -> Option<(u32, u32, Vec<u8>)> {
    let mut bm = BITMAP::default();
    let size = std::mem::size_of::<BITMAP>() as i32;
    if unsafe { GetObjectW(bitmap.into(), size, Some(&mut bm as *mut BITMAP as *mut c_void)) } == 0 {
        return None;
    }
    let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
    if !(1..=256).contains(&w) || !(1..=256).contains(&h) {
        return None;
    }
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = vec![0u8; (w * h * 4) as usize];
    let dc = unsafe { CreateCompatibleDC(None) };
    if dc.is_invalid() {
        return None;
    }
    let lines = unsafe {
        GetDIBits(dc, bitmap, 0, h as u32, Some(bits.as_mut_ptr() as *mut c_void), &mut info, DIB_RGB_COLORS)
    };
    let _ = unsafe { DeleteDC(dc) };
    if lines != h {
        return None;
    }
    let pixels = bits.as_chunks::<4>().0;
    let has_alpha = pixels.iter().any(|p| p[3] != 0);
    let mut out = Vec::with_capacity(bits.len());
    for p in pixels {
        let a = if has_alpha { p[3] } else { 255 };
        let un = |c: u8| if has_alpha && a > 0 && a < 255 { ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8 } else { c };
        out.extend_from_slice(&[un(p[2]), un(p[1]), un(p[0]), a]);
    }
    Some((w as u32, h as u32, out))
}

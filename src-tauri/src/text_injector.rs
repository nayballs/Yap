//! Text injection via clipboard + simulated Ctrl+V paste.
//!
//! Used by the dictation feature to inject transcribed text into
//! whatever application/field is currently focused.
//!
//! - **Private clipboard writes** (`clip`): the dictation goes onto the
//!   clipboard with Windows' "keep this out" formats —
//!   `ExcludeClipboardContentFromMonitorProcessing`,
//!   `CanIncludeInClipboardHistory` = 0, `CanUploadToCloudClipboard` = 0 and
//!   the clipboard managers' `Clipboard Viewer Ignore` — so it never lands in
//!   Win+V history, cloud clipboard sync or a well-behaved clipboard manager;
//!   putting the user's own clipboard back afterwards carries them too (that
//!   item is already in their history). Every write is checked by the
//!   clipboard's sequence number and retried; when it still doesn't take, the
//!   text is typed instead (`type_unicode`). A remote-desktop or VM window is
//!   the exception: its clipboard sync is a monitor too, so there the
//!   dictation goes on the clipboard plainly (`syncs_clipboard`).
//! - **Fitting the dictation to the cursor** ([`prepare_dictation`],
//!   `smart_insert.rs`) and to the kind of app (`app_kind.rs`: one line in a
//!   terminal, no full stop on a short chat message), and remembering it for
//!   **"Paste last transcript"** ([`paste_last_transcript`], also in the
//!   tray menu).

use std::sync::Mutex;

use tracing::{debug, info, trace, warn};

use crate::app_kind::{self, AppKind};
use crate::config::DictionaryEntry;
use crate::smart_insert::{self, FieldContext};

#[cfg(target_os = "windows")]
#[allow(clippy::upper_case_acronyms)] // Win32 FFI names match the Windows API
mod platform {
    use std::mem;
    use std::thread;
    use std::time::Duration;

    // Win32 SendInput types — must match the real Windows struct layout.
    // The union must include MOUSEINPUT (the largest member) so that
    // sizeof(INPUT) == 40 on 64-bit, which is what SendInput expects
    // for its cbSize parameter.

    #[repr(C)]
    struct INPUT {
        type_: u32,
        union: INPUT_UNION,
    }

    #[repr(C)]
    #[derive(Copy, Clone)]
    union INPUT_UNION {
        mi: MOUSEINPUT,
        ki: KEYBDINPUT,
    }

    #[repr(C)]
    #[derive(Copy, Clone)]
    struct MOUSEINPUT {
        dx: i32,
        dy: i32,
        mouse_data: u32,
        dw_flags: u32,
        time: u32,
        dw_extra_info: usize,
    }

    #[repr(C)]
    #[derive(Copy, Clone)]
    struct KEYBDINPUT {
        w_vk: u16,
        w_scan: u16,
        dw_flags: u32,
        time: u32,
        dw_extra_info: usize,
    }

    const INPUT_KEYBOARD: u32 = 1;
    const KEYEVENTF_KEYUP: u32 = 0x0002;
    const KEYEVENTF_UNICODE: u32 = 0x0004;
    const VK_SHIFT: u16 = 0x10;
    const VK_CONTROL: u16 = 0x11;
    const VK_C: u16 = 0x43;
    const VK_V: u16 = 0x56;
    const VK_RETURN: u16 = 0x0D;
    const GWL_EXSTYLE: i32 = -20;
    const WS_EX_TOOLWINDOW: i32 = 0x0000_0080;
    const WS_EX_NOACTIVATE: i32 = 0x0800_0000;
    const DWMWA_CLOAKED: u32 = 14;

    type HWND = *mut core::ffi::c_void;

    extern "system" {
        fn SendInput(c_inputs: u32, p_inputs: *const INPUT, cb_size: i32) -> u32;
        fn GetForegroundWindow() -> HWND;
        fn SetForegroundWindow(hwnd: HWND) -> i32;
        fn GetWindowThreadProcessId(hwnd: HWND, lpdw_process_id: *mut u32) -> u32;
        fn AttachThreadInput(id_attach: u32, id_attach_to: u32, f_attach: i32) -> i32;
        fn GetCurrentThreadId() -> u32;
        fn GetCurrentProcessId() -> u32;
        fn IsWindow(hwnd: HWND) -> i32;
        fn BringWindowToTop(hwnd: HWND) -> i32;
        fn OpenProcess(desired_access: u32, inherit: i32, pid: u32) -> HANDLE;
        fn QueryFullProcessImageNameW(
            process: HANDLE,
            flags: u32,
            buffer: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn CloseHandle(h: HANDLE) -> i32;
    }

    // Declared as notepad.rs / overlay.rs declare them
    // (`clashing_extern_declarations`).
    #[link(name = "user32")]
    extern "system" {
        fn EnumWindows(callback: unsafe extern "system" fn(*mut core::ffi::c_void, isize) -> i32, lparam: isize) -> i32;
        fn IsWindowVisible(hwnd: *mut core::ffi::c_void) -> i32;
        fn IsIconic(hwnd: *mut core::ffi::c_void) -> i32;
        fn GetWindowLongW(hwnd: *mut core::ffi::c_void, index: i32) -> i32;
        fn GetClassNameW(hwnd: HWND, buf: *mut u16, max: i32) -> i32;
        fn GetWindowTextLengthW(hwnd: HWND) -> i32;
    }
    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmGetWindowAttribute(hwnd: *mut core::ffi::c_void, attribute: u32, value: *mut core::ffi::c_void, size: u32) -> i32;
    }

    type HANDLE = *mut core::ffi::c_void;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

    /// Process base name (e.g. "chrome.exe") of the window `hwnd_isize`, or `None`.
    /// Best-effort; used to tag history entries with the focused app.
    pub fn window_app_name(hwnd_isize: isize) -> Option<String> {
        let hwnd = hwnd_isize as HWND;
        if hwnd.is_null() {
            return None;
        }
        unsafe {
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == 0 {
                return None;
            }
            let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if proc.is_null() {
                return None;
            }
            let mut buf = [0u16; 260];
            let mut size = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(proc, 0, buf.as_mut_ptr(), &mut size);
            CloseHandle(proc);
            if ok == 0 || size == 0 {
                return None;
            }
            let full = String::from_utf16_lossy(&buf[..size as usize]);
            // Base name only — strip the directory.
            full.rsplit(['\\', '/']).next().map(|s| s.to_string())
        }
    }

    /// The process id of the window `hwnd_isize` (0 = none).
    pub fn window_process(hwnd_isize: isize) -> u32 {
        let hwnd = hwnd_isize as HWND;
        if hwnd.is_null() {
            return 0;
        }
        let mut pid: u32 = 0;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        pid
    }

    /// Handle of the current foreground window as an `isize` (0 = none).
    ///
    /// Captured at record-start so we can paste back into the window the user was
    /// dictating into, even if focus shifts during transcription. Returns 0 when
    /// the foreground belongs to *our own* process (e.g. the user started
    /// dictation from one of Yap's own windows) — we never re-focus ourselves.
    pub fn foreground_window() -> isize {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_null() {
                return 0;
            }
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == GetCurrentProcessId() {
                return 0;
            }
            hwnd as isize
        }
    }

    fn class_of(hwnd: HWND) -> String {
        let mut buf = [0u16; 128];
        let len = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
        String::from_utf16_lossy(&buf[..len.max(0) as usize])
    }

    unsafe extern "system" fn collect(hwnd: *mut core::ffi::c_void, lparam: isize) -> i32 {
        let list = &mut *(lparam as *mut Vec<isize>);
        if IsWindowVisible(hwnd) != 0 {
            list.push(hwnd as isize);
        }
        1
    }

    /// Where "Paste last transcript" pastes: the foreground window when it's
    /// an app's (a hotkey, the bar), else — the tray menu just had the
    /// foreground — the app window on top, the one the person was in.
    /// Titles are never read, only whether there is one.
    pub fn paste_target() -> Option<isize> {
        let fg = foreground_window();
        if fg != 0 && !super::SHELL_CLASSES.contains(&class_of(fg as HWND).as_str()) {
            return Some(fg);
        }
        let mut hwnds: Vec<isize> = Vec::new();
        unsafe {
            EnumWindows(collect, &mut hwnds as *mut Vec<isize> as isize);
        }
        // EnumWindows goes top to bottom.
        let windows: Vec<super::WindowInfo> = hwnds
            .into_iter()
            .map(|id| unsafe {
                let hwnd = id as HWND;
                let ex = GetWindowLongW(hwnd, GWL_EXSTYLE);
                let mut cloaked = 0u32;
                DwmGetWindowAttribute(
                    hwnd,
                    DWMWA_CLOAKED,
                    &mut cloaked as *mut u32 as *mut core::ffi::c_void,
                    mem::size_of::<u32>() as u32,
                );
                super::WindowInfo {
                    hwnd: id,
                    pid: window_process(id),
                    minimized: IsIconic(hwnd) != 0,
                    tool: ex & WS_EX_TOOLWINDOW != 0,
                    no_activate: ex & WS_EX_NOACTIVATE != 0,
                    cloaked: cloaked != 0,
                    titled: GetWindowTextLengthW(hwnd) > 0,
                    class: class_of(hwnd),
                }
            })
            .collect();
        super::pick_paste_window(&windows, unsafe { GetCurrentProcessId() })
    }

    /// Best-effort: bring `hwnd` back to the foreground before pasting.
    ///
    /// Windows restricts `SetForegroundWindow` from background threads, so we
    /// briefly attach our thread's input to the target window's thread (the
    /// standard workaround) to make the focus change stick. No-op if the handle
    /// is null/stale or already foreground.
    pub fn focus_window(hwnd_isize: isize) -> bool {
        let hwnd = hwnd_isize as HWND;
        if hwnd.is_null() {
            return false;
        }
        unsafe {
            if IsWindow(hwnd) == 0 {
                return false;
            }
            if GetForegroundWindow() == hwnd {
                return true;
            }
            let cur = GetCurrentThreadId();
            let target = GetWindowThreadProcessId(hwnd, std::ptr::null_mut());
            let attached = AttachThreadInput(cur, target, 1) != 0;
            let ok = SetForegroundWindow(hwnd) != 0;
            BringWindowToTop(hwnd);
            if attached {
                AttachThreadInput(cur, target, 0);
            }
            // Let the focus change settle before keystrokes land.
            thread::sleep(Duration::from_millis(20));
            ok
        }
    }

    fn make_unicode(unit: u16, up: bool) -> INPUT {
        INPUT {
            type_: INPUT_KEYBOARD,
            union: INPUT_UNION {
                ki: KEYBDINPUT {
                    w_vk: 0,
                    w_scan: unit,
                    dw_flags: KEYEVENTF_UNICODE | if up { KEYEVENTF_KEYUP } else { 0 },
                    time: 0,
                    dw_extra_info: 0,
                },
            },
        }
    }

    /// Type `text` directly as Unicode keystrokes via `SendInput` (no clipboard).
    ///
    /// Fallback for when the clipboard/paste path is unavailable. Sends each
    /// UTF-16 code unit as a key down/up pair (surrogate pairs go through as
    /// consecutive units, which Windows recombines).
    pub fn type_unicode(text: &str) {
        let mut inputs: Vec<INPUT> = Vec::with_capacity(text.len() * 2);
        for unit in text.encode_utf16() {
            inputs.push(make_unicode(unit, false));
            inputs.push(make_unicode(unit, true));
        }
        if inputs.is_empty() {
            return;
        }
        // Send in chunks so a very long transcript doesn't overflow the input
        // queue in a single call.
        for chunk in inputs.chunks(512) {
            unsafe {
                SendInput(
                    chunk.len() as u32,
                    chunk.as_ptr(),
                    mem::size_of::<INPUT>() as i32,
                );
            }
            thread::sleep(Duration::from_millis(2));
        }
    }

    fn make_key(vk: u16, up: bool) -> INPUT {
        INPUT {
            type_: INPUT_KEYBOARD,
            union: INPUT_UNION {
                ki: KEYBDINPUT {
                    w_vk: vk,
                    w_scan: 0,
                    dw_flags: if up { KEYEVENTF_KEYUP } else { 0 },
                    time: 0,
                    dw_extra_info: 0,
                },
            },
        }
    }

    /// Simulate Ctrl+V keystroke via SendInput.
    pub fn simulate_paste() {
        let inputs = [
            make_key(VK_CONTROL, false),
            make_key(VK_V, false),
            make_key(VK_V, true),
            make_key(VK_CONTROL, true),
        ];

        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                mem::size_of::<INPUT>() as i32,
            )
        };

        tracing::debug!(
            sent,
            expected = inputs.len(),
            cb_size = mem::size_of::<INPUT>(),
            "SendInput Ctrl+V"
        );

        // Brief delay to let the target app process the paste
        thread::sleep(Duration::from_millis(80));
    }

    /// Simulate Ctrl+C keystroke via SendInput (used by the edit-mode selection
    /// capture fallback to copy the current selection into the clipboard).
    pub fn simulate_copy() {
        let inputs = [
            make_key(VK_CONTROL, false),
            make_key(VK_C, false),
            make_key(VK_C, true),
            make_key(VK_CONTROL, true),
        ];
        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                mem::size_of::<INPUT>() as i32,
            )
        };
        tracing::debug!(sent, expected = inputs.len(), "SendInput Ctrl+C");
    }

    /// Simulate a submit keystroke via SendInput (used by auto-submit):
    /// plain Enter, Ctrl+Enter, or Shift+Enter depending on `key`.
    pub fn press_submit(key: &str) {
        let modifier = match key {
            "ctrlEnter" => Some(VK_CONTROL),
            "shiftEnter" => Some(VK_SHIFT),
            _ => None, // "enter" / unknown → plain Enter
        };

        let mut inputs = Vec::with_capacity(4);
        if let Some(m) = modifier {
            inputs.push(make_key(m, false));
        }
        inputs.push(make_key(VK_RETURN, false));
        inputs.push(make_key(VK_RETURN, true));
        if let Some(m) = modifier {
            inputs.push(make_key(m, true));
        }

        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                mem::size_of::<INPUT>() as i32,
            )
        };

        tracing::debug!(sent, expected = inputs.len(), key, "SendInput submit");
        thread::sleep(Duration::from_millis(20));
    }
}

/// The clipboard, through Win32 directly: arboard can't add the formats that
/// keep a dictation private, nor tell whether a write took.
#[cfg(target_os = "windows")]
mod clip {
    use std::thread::sleep;
    use std::time::Duration;

    use tracing::{debug, warn};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, CountClipboardFormats, EmptyClipboard, GetClipboardData,
        GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard,
        RegisterClipboardFormatW, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};

    const CF_UNICODETEXT: u32 = 13;
    const CF_HDROP: u32 = 15;
    const CF_DIBV5: u32 = 17;

    /// Cap for what a snapshot keeps: a 4K BGRA screenshot is ~33 MB;
    /// anything much larger isn't worth the memory.
    const MAX_SNAPSHOT_BYTES: usize = 64 * 1024 * 1024;

    /// The formats that keep clipboard data out of Win+V history, cloud
    /// clipboard sync and clipboard managers. The first three are Windows'
    /// own (learn.microsoft.com, "Clipboard formats"); "Clipboard Viewer
    /// Ignore" is the convention clipboard managers such as Ditto honour.
    const PRIVATE: [(&str, u32); 4] = [
        ("ExcludeClipboardContentFromMonitorProcessing", 0),
        ("CanIncludeInClipboardHistory", 0),
        ("CanUploadToCloudClipboard", 0),
        ("Clipboard Viewer Ignore", 0),
    ];

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn registered(name: &str) -> u32 {
        let name = wide(name);
        unsafe { RegisterClipboardFormatW(PCWSTR(name.as_ptr())) }
    }

    /// UTF-16 with its terminating NUL, as CF_UNICODETEXT holds it.
    fn text_bytes(text: &str) -> Vec<u8> {
        wide(text).iter().flat_map(|u| u.to_le_bytes()).collect()
    }

    /// The clipboard, open; closed on drop. Another app (Win+V's history,
    /// a clipboard manager, the app reading a paste) may hold it a moment,
    /// so opening retries for ~0.3 s.
    struct Open;

    impl Open {
        fn new() -> Option<Open> {
            for i in 0..8u64 {
                if unsafe { OpenClipboard(None) }.is_ok() {
                    return Some(Open);
                }
                sleep(Duration::from_millis(5 + i * 10));
            }
            None
        }
    }

    impl Drop for Open {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseClipboard();
            }
        }
    }

    pub fn sequence() -> u32 {
        unsafe { GetClipboardSequenceNumber() }
    }

    fn available(format: u32) -> bool {
        unsafe { IsClipboardFormatAvailable(format) }.is_ok()
    }

    /// One format's bytes (the clipboard open).
    fn read(format: u32) -> Option<Vec<u8>> {
        unsafe {
            let handle = GetClipboardData(format).ok()?;
            let global = HGLOBAL(handle.0);
            let size = GlobalSize(global);
            if size == 0 {
                return None;
            }
            if size > MAX_SNAPSHOT_BYTES {
                warn!(bytes = size, "Clipboard item too large to keep; it won't be put back after the paste");
                return None;
            }
            let ptr = GlobalLock(global) as *const u8;
            if ptr.is_null() {
                return None;
            }
            let bytes = std::slice::from_raw_parts(ptr, size).to_vec();
            let _ = GlobalUnlock(global);
            Some(bytes)
        }
    }

    /// CF_UNICODETEXT as a string (the clipboard open).
    fn read_text_open() -> Option<String> {
        let bytes = read(CF_UNICODETEXT)?;
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&b| u16::from_le_bytes(b))
            .take_while(|&u| u != 0)
            .collect();
        Some(String::from_utf16_lossy(&units))
    }

    /// The clipboard's text, if it has any.
    pub fn read_text() -> Option<String> {
        let _open = Open::new()?;
        read_text_open()
    }

    /// Set one format (the clipboard open and emptied by us). The memory is
    /// the clipboard's once this succeeds, ours to free if it fails.
    fn set(format: u32, bytes: &[u8]) -> Result<(), String> {
        unsafe {
            let global = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)).map_err(|e| format!("GlobalAlloc: {e}"))?;
            let ptr = GlobalLock(global) as *mut u8;
            if ptr.is_null() {
                let _ = GlobalFree(Some(global));
                return Err("GlobalLock failed".into());
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
            let _ = GlobalUnlock(global);
            if let Err(e) = SetClipboardData(format, Some(HANDLE(global.0))) {
                let _ = GlobalFree(Some(global));
                return Err(format!("SetClipboardData: {e}"));
            }
            Ok(())
        }
    }

    /// Replace the clipboard with `items` (format, bytes); `private` adds the
    /// keep-it-out formats.
    fn put(items: &[(u32, Vec<u8>)], private: bool) -> Result<(), String> {
        let _open = Open::new().ok_or("the clipboard stayed busy")?;
        unsafe { EmptyClipboard() }.map_err(|e| format!("EmptyClipboard: {e}"))?;
        for (format, bytes) in items {
            set(*format, bytes)?;
        }
        if private {
            for (name, value) in PRIVATE {
                // Best-effort: the text is on the clipboard either way.
                if let Err(e) = set(registered(name), &value.to_le_bytes()) {
                    debug!(format = name, error = %e, "Couldn't add a privacy format");
                }
            }
        }
        Ok(())
    }

    /// Put `text` on the clipboard and check it took (the sequence number
    /// moved), retrying with a short backoff. `false` = it never did.
    pub fn put_text_verified(text: &str, private: bool) -> bool {
        let item = [(CF_UNICODETEXT, text_bytes(text))];
        for (attempt, wait_ms) in [0u64, 15, 40, 80].into_iter().enumerate() {
            sleep(Duration::from_millis(wait_ms));
            let before = sequence();
            match put(&item, private) {
                Ok(()) if sequence() != before => {
                    debug!(attempt, private, "Clipboard write verified");
                    return true;
                }
                Ok(()) => debug!(attempt, "Clipboard write didn't take (sequence number unchanged)"),
                Err(e) => debug!(attempt, error = %e, "Clipboard write failed"),
            }
        }
        false
    }

    /// What was on the clipboard before Yap borrowed it.
    pub enum Snapshot {
        /// Couldn't open the clipboard to look: leave it be afterwards.
        Unknown,
        /// Nothing at all: empty it again afterwards.
        Empty,
        Text(String),
        /// An image (as a DIB, plus a PNG when there was one) and/or copied
        /// files, byte for byte.
        Raw(Vec<(u32, Vec<u8>)>),
        /// Something Yap doesn't keep (e.g. only a metafile): left as is
        /// (the dictation stays on the clipboard, as it always has).
        Other,
    }

    pub fn snapshot() -> Snapshot {
        let Some(_open) = Open::new() else {
            return Snapshot::Unknown;
        };
        if unsafe { CountClipboardFormats() } == 0 {
            return Snapshot::Empty;
        }
        if available(CF_UNICODETEXT) {
            if let Some(text) = read_text_open() {
                return Snapshot::Text(text);
            }
        }
        let formats = [
            CF_DIBV5,
            registered("PNG"),
            CF_HDROP,
            registered("Preferred DropEffect"),
        ];
        let raw: Vec<(u32, Vec<u8>)> = formats
            .into_iter()
            .filter(|&f| available(f))
            .filter_map(|f| read(f).map(|bytes| (f, bytes)))
            .collect();
        if raw.is_empty() {
            Snapshot::Other
        } else {
            debug!(formats = raw.len(), "Clipboard snapshot: an image or files");
            Snapshot::Raw(raw)
        }
    }

    /// Put back what [`snapshot`] saw.
    pub fn restore(snap: Snapshot, private: bool) {
        let result = match snap {
            Snapshot::Unknown | Snapshot::Other => Ok(()),
            Snapshot::Empty => match Open::new() {
                Some(_open) => unsafe { EmptyClipboard() }.map_err(|e| e.to_string()),
                None => Err("the clipboard stayed busy".into()),
            },
            Snapshot::Text(text) => put(&[(CF_UNICODETEXT, text_bytes(&text))], private),
            Snapshot::Raw(items) => put(&items, private),
        };
        if let Err(e) = result {
            warn!(error = %e, "Couldn't put the clipboard back after the paste");
        }
    }

    /// Empty the clipboard (so a Ctrl+C that copies nothing shows as nothing).
    pub fn clear() {
        if let Some(_open) = Open::new() {
            let _ = unsafe { EmptyClipboard() };
        }
    }

    /// Tests: each privacy format's bytes on the clipboard now (`None` = absent).
    #[cfg(test)]
    pub fn privacy_formats() -> Vec<(&'static str, Option<Vec<u8>>)> {
        let _open = Open::new().expect("the clipboard");
        PRIVATE
            .iter()
            .map(|(name, _)| {
                let format = registered(name);
                (*name, available(format).then(|| read(format)).flatten())
            })
            .collect()
    }

    /// Tests: put raw formats on the clipboard (as another app would).
    #[cfg(test)]
    pub fn put_raw(items: &[(u32, Vec<u8>)]) {
        put(items, false).expect("a raw write");
    }

    /// Tests: one format's bytes, and how many formats there are.
    #[cfg(test)]
    pub fn read_raw(format: u32) -> (Option<Vec<u8>>, i32) {
        let _open = Open::new().expect("the clipboard");
        (read(format), unsafe { CountClipboardFormats() })
    }

    #[cfg(test)]
    pub const HDROP: u32 = CF_HDROP;
}

/// Window classes of the shell itself (taskbars, the desktop, the tray's
/// overflow): never where a paste goes.
const SHELL_CLASSES: &[&str] = &[
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "Progman",
    "WorkerW",
    "NotifyIconOverflowWindow",
    "TopLevelWindowForOverflowXamlIsland",
    "XamlExplorerHostIslandWindow",
    "Windows.UI.Core.CoreWindow",
];

/// A visible top-level window, for picking where "Paste last transcript"
/// goes ([`pick_paste_window`]).
#[derive(Clone, Debug, Default)]
pub(crate) struct WindowInfo {
    pub hwnd: isize,
    pub pid: u32,
    pub minimized: bool,
    pub tool: bool,
    pub no_activate: bool,
    pub cloaked: bool,
    pub titled: bool,
    pub class: String,
}

/// The first real app window in z-order (`windows` top to bottom): not
/// Yap's own, not minimised, a tool, unactivatable or cloaked (a suspended
/// UWP app, another virtual desktop's) window, titled, and not the shell's.
pub(crate) fn pick_paste_window(windows: &[WindowInfo], own_pid: u32) -> Option<isize> {
    windows
        .iter()
        .find(|w| {
            w.pid != own_pid
                && !w.minimized
                && !w.tool
                && !w.no_activate
                && !w.cloaked
                && w.titled
                && !SHELL_CLASSES.contains(&w.class.as_str())
        })
        .map(|w| w.hwnd)
}

/// Remote-desktop and VM windows: the clipboard reaches the remote machine
/// through their clipboard sync, a clipboard monitor, so there the
/// dictation goes on the clipboard without the keep-it-out formats.
const CLIPBOARD_SYNC_APPS: &[&str] = &[
    "mstsc.exe",
    "msrdc.exe",
    "msrdcw.exe",
    "windows365.exe",
    "vmconnect.exe",
    "wfica32.exe",
    "cdviewer.exe",
    "vmware.exe",
    "vmware-view.exe",
    "vmrc.exe",
    "virtualboxvm.exe",
    "parsecd.exe",
    "anydesk.exe",
    "teamviewer.exe",
    "rustdesk.exe",
    "vncviewer.exe",
    "tvnviewer.exe",
];

/// Does the app (process name, e.g. "mstsc.exe") sync the clipboard to
/// another machine?
pub(crate) fn syncs_clipboard(app: Option<&str>) -> bool {
    app.is_some_and(|a| CLIPBOARD_SYNC_APPS.iter().any(|s| s.eq_ignore_ascii_case(a.trim())))
}

/// Simulate the auto-submit keystroke in the focused window. `key` is one of
/// "enter", "ctrlEnter", or "shiftEnter".
pub async fn press_submit(key: &str) -> Result<(), String> {
    if crate::e2e::active() {
        info!("e2e: not pressing {} in another app", key);
        return Ok(());
    }
    let key = key.to_string();
    tokio::task::spawn_blocking(move || {
        #[cfg(target_os = "windows")]
        platform::press_submit(&key);
        #[cfg(not(target_os = "windows"))]
        {
            let _ = &key;
            warn!("press_submit is only supported on Windows");
        }
    })
    .await
    .map_err(|e| format!("press_submit task panicked: {}", e))
}

/// Handle of the current foreground window (as an `isize`, `None` if none).
///
/// Call this at record-start and pass the result to [`inject_text`] so the paste
/// targets the window the user was dictating into — not whatever happens to be
/// focused when transcription finishes. Returns `None` off Windows.
pub fn current_foreground() -> Option<isize> {
    #[cfg(target_os = "windows")]
    {
        match platform::foreground_window() {
            0 => None,
            h => Some(h),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// Process base name (e.g. "chrome.exe") of the given window handle, or `None`.
/// Used to tag history entries with the focused app. `None` off Windows.
pub fn app_name_for(hwnd: Option<isize>) -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        hwnd.and_then(platform::window_app_name)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = hwnd;
        None
    }
}

/// The process id of a window (`None` for none, or off Windows).
pub fn window_process(hwnd: isize) -> Option<u32> {
    #[cfg(target_os = "windows")]
    {
        Some(platform::window_process(hwnd)).filter(|&pid| pid != 0)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = hwnd;
        None
    }
}

/// Capture the current text selection via the **clipboard Ctrl+C trick** — the
/// fallback for edit/rewrite mode when UI Automation can't read the selection
/// (common in Electron/Chromium/terminal apps).
///
/// Snapshots the clipboard, clears it (so we can tell whether Ctrl+C actually
/// copied anything), sends Ctrl+C to `target_hwnd`, reads the result, then
/// restores the original clipboard. Returns `None` if nothing was selected
/// (clipboard stayed empty) or off Windows.
pub fn selection_via_copy(target_hwnd: Option<isize>) -> Option<String> {
    if crate::e2e::active() {
        info!("e2e: not sending Ctrl+C to another app");
        return None;
    }
    #[cfg(target_os = "windows")]
    {
        // Make sure the target window has focus so Ctrl+C hits the right field.
        if let Some(hwnd) = target_hwnd {
            platform::focus_window(hwnd);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        // Snapshot text *or* image/files so restoring doesn't wipe them.
        let previous = clip::snapshot();
        // Clear first: if Ctrl+C copies nothing (no selection), the clipboard
        // stays empty and we correctly return None instead of the old contents.
        clip::clear();
        std::thread::sleep(std::time::Duration::from_millis(30));

        platform::simulate_copy();
        // Poll for the copy to land (up to ~200 ms) rather than a fixed sleep —
        // returns as soon as text appears (fast common case) but tolerates a slow
        // Ctrl+C in RDP/Electron/busy apps.
        let mut copied = None;
        for _ in 0..10 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            if let Some(t) = clip::read_text() {
                if !t.trim().is_empty() {
                    copied = Some(t);
                    break;
                }
            }
        }

        // Restore the user's clipboard (best-effort), kept out of history: it
        // is in there already.
        let private = !syncs_clipboard(app_name_for(target_hwnd).as_deref());
        clip::restore(previous, private);

        match copied {
            Some(t) if !t.trim().is_empty() => Some(t),
            _ => None,
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = target_hwnd;
        None
    }
}

// ---- A dictation, finished for the paste; "Paste last transcript" ----

/// The latest dictation this session, as it was before fitting it to the
/// cursor (so a paste somewhere else fits it afresh). Forgotten when history
/// entries are deleted or cleared; after a restart, history's newest
/// dictation stands in.
static LAST_DICTATION: Mutex<Option<String>> = Mutex::new(None);

fn last_dictation() -> std::sync::MutexGuard<'static, Option<String>> {
    LAST_DICTATION.lock().unwrap_or_else(|p| p.into_inner())
}

/// The finished dictation `text` (cleaned up, corrected) as it should be
/// pasted, remembered for "Paste last transcript": fitted to the kind of app
/// it goes into (`kind`, `app_kind.rs`, already filtered by the settings)
/// and to the text around the cursor when Yap read it at record start
/// (`smart_spacing`), else as before — with a space after it if
/// `append_space`.
pub fn prepare_dictation(text: &str, append_space: bool, dictionary: &[DictionaryEntry], kind: AppKind) -> String {
    *last_dictation() = Some(text.trim().to_string());
    let ctx = crate::selection::take_context();
    fit_for_paste(text, ctx.as_ref(), append_space, dictionary, kind)
}

/// `text` for the paste. A terminal gets it as `app_kind::terminal_text`
/// makes it (one line unless the coding CLI there takes a paste; no space
/// after it, no cursor context). A chat app's short message loses its final
/// full stop first. Then it's fitted to `ctx` when there is one; otherwise
/// unchanged, plus a space if `append_space` (what a dictation always did).
fn fit_for_paste(
    text: &str,
    ctx: Option<&FieldContext>,
    append_space: bool,
    dictionary: &[DictionaryEntry],
    kind: AppKind,
) -> String {
    let chat;
    let text = match kind {
        AppKind::Terminal(t) => {
            let out = app_kind::terminal_text(text, t.keeps_lines());
            debug!(
                changed = out != text.trim(),
                lines = out.lines().count(),
                "Fitted the dictation for {}",
                kind.describe()
            );
            return out;
        }
        AppKind::Messaging => {
            chat = app_kind::drop_chat_full_stop(text, ctx, dictionary);
            if chat != text {
                debug!("Left the full stop off a short chat message");
            }
            chat.as_str()
        }
        AppKind::Other => text,
    };
    match ctx {
        Some(ctx) => {
            let names: Vec<&str> = dictionary.iter().map(|e| e.to.trim()).filter(|t| !t.is_empty()).collect();
            let out = smart_insert::fit_to_context(text, ctx, &names);
            debug!(
                changed = out != text,
                lead_space = out.starts_with(' '),
                trail_space = out.ends_with(' '),
                "Fitted the dictation to the text around the cursor"
            );
            out
        }
        None => {
            let mut out = text.to_string();
            if append_space {
                out.push(' ');
            }
            out
        }
    }
}

/// History entries changed under it (one deleted, all cleared): don't paste
/// a dictation the person just removed; history's newest stands in.
pub fn forget_last_transcript() {
    *last_dictation() = None;
}

/// What "Paste last transcript" would paste (before fitting): this
/// session's latest dictation, else history's newest.
pub fn last_transcript() -> Option<String> {
    let remembered = last_dictation().clone();
    remembered
        .or_else(crate::history::latest_dictation)
        .filter(|t| !t.trim().is_empty())
}

/// Whether there's anything for "Paste last transcript" (the tray item).
pub fn has_last_transcript() -> bool {
    last_transcript().is_some()
}

const NOTHING_TO_PASTE: &str = "Nothing to paste yet — dictate something first";

/// "Paste last transcript": the latest dictation into the app window the
/// person is in (the tray menu borrowed the foreground: the top app window),
/// fitted to its cursor like a dictation. For when a paste went astray.
/// Returns what was pasted. A test run pastes nothing and returns it.
fn paste_last_blocking() -> Result<String, String> {
    let text = last_transcript().ok_or(NOTHING_TO_PASTE)?;
    let cfg = crate::config::load();
    if crate::e2e::active() {
        let out = fit_for_paste(&text, None, cfg.append_trailing_space, &cfg.dictionary, AppKind::Other);
        info!(len = out.len(), "e2e: not pasting the last transcript");
        return Ok(out);
    }
    #[cfg(target_os = "windows")]
    {
        let target = platform::paste_target();
        if let Some(hwnd) = target {
            platform::focus_window(hwnd);
            // The app puts its own focus back in its field on activation.
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
        // A terminal or a chat app gets it as a dictation would; a
        // terminal's screen isn't read.
        let kind = app_kind::of_window(target).with_settings(cfg.terminal_mode, cfg.chat_no_full_stop);
        let ctx = if cfg.smart_spacing && target.is_some() && !kind.is_terminal() {
            crate::selection::read_context(target)
        } else {
            None
        };
        let out = fit_for_paste(&text, ctx.as_ref(), cfg.append_trailing_space, &cfg.dictionary, kind);
        info!(len = out.len(), found_window = target.is_some(), fitted = ctx.is_some(), "Pasting the last transcript");
        inject_text_sync(&out, cfg.restore_clipboard, target)?;
        Ok(out)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err("Pasting is only supported on Windows".into())
    }
}

/// "Paste last transcript" (the tray menu; also callable from the page):
/// pastes the latest dictation into the app you're in and returns it. Errors
/// when there's nothing yet.
#[tauri::command]
pub async fn paste_last_transcript() -> Result<String, String> {
    tokio::task::spawn_blocking(paste_last_blocking)
        .await
        .map_err(|e| format!("Paste task failed: {e}"))?
}

/// The tray's "Paste last transcript" (off the main thread).
pub fn paste_last_from_tray() {
    std::thread::spawn(|| {
        if let Err(e) = paste_last_blocking() {
            warn!("Paste last transcript: {}", e);
        }
    });
}

/// Inject text into the target field via clipboard + Ctrl+V.
///
/// `target_hwnd` (from [`current_foreground`] at record-start) is re-focused
/// before pasting so focus changes during transcription don't misfire. Runs on a
/// blocking thread to avoid blocking the tokio runtime.
pub async fn inject_text(
    text: &str,
    restore_clipboard: bool,
    target_hwnd: Option<isize>,
) -> Result<(), String> {
    // An e2e test run never pastes into another app (or touches the clipboard).
    if crate::e2e::active() {
        info!(len = text.len(), "e2e: not pasting the text");
        return Ok(());
    }
    let text = text.to_string();

    tokio::task::spawn_blocking(move || inject_text_sync(&text, restore_clipboard, target_hwnd))
        .await
        .map_err(|e| format!("Inject task panicked: {}", e))?
}

fn inject_text_sync(
    text: &str,
    restore_clipboard: bool,
    target_hwnd: Option<isize>,
) -> Result<(), String> {
    info!(len = text.len(), "Injecting text via clipboard paste");

    // Small delay to let focus settle after voice recording stops.
    std::thread::sleep(std::time::Duration::from_millis(50));

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (restore_clipboard, target_hwnd);
        warn!("Text injection is only supported on Windows");
        Err("Text injection is only supported on Windows".into())
    }

    #[cfg(target_os = "windows")]
    {
        // Re-focus the window the user was dictating into (best-effort).
        if let Some(hwnd) = target_hwnd {
            if !platform::focus_window(hwnd) {
                warn!("Could not re-focus the dictation target window; pasting into current focus");
            }
        }

        // Kept out of clipboard history and sync, unless the target is a
        // remote desktop whose sync is how the paste gets there.
        let private = !syncs_clipboard(app_name_for(target_hwnd).as_deref());
        // Snapshot the current clipboard (text, an image or files), only if
        // we'll restore it.
        let previous = restore_clipboard.then(clip::snapshot);

        if !clip::put_text_verified(text, private) {
            // The clipboard can't be written: type the text instead.
            warn!("Couldn't put the text on the clipboard; typing it instead");
            platform::type_unicode(text);
            if let Some(prev) = previous {
                clip::restore(prev, private);
            }
            return Ok(());
        }
        trace!(private, "Dictation on the clipboard");

        // Small delay to ensure clipboard is ready.
        std::thread::sleep(std::time::Duration::from_millis(30));

        // Simulate Ctrl+V.
        platform::simulate_paste();

        // Restore previous clipboard (delayed to ensure paste completes). Webview/
        // Electron apps process Ctrl+V asynchronously (browser→renderer IPC) and can
        // take well over 200 ms to actually read the clipboard — restoring too early
        // makes the paste land empty or with the OLD contents. 500 ms is still
        // imperceptible (it runs after the text has visually appeared in fast apps).
        if let Some(prev) = previous {
            std::thread::sleep(std::time::Duration::from_millis(500));
            clip::restore(prev, private);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(hwnd: isize, pid: u32) -> WindowInfo {
        WindowInfo {
            hwnd,
            pid,
            titled: true,
            class: "Chrome_WidgetWin_1".into(),
            ..Default::default()
        }
    }

    #[test]
    fn paste_goes_to_the_top_real_app_window() {
        let own = 7;
        let windows = vec![
            WindowInfo { class: "Shell_TrayWnd".into(), ..win(1, 2) }, // the taskbar
            win(2, own),                                                // Yap's own
            WindowInfo { tool: true, ..win(3, 3) },                     // a tool window
            WindowInfo { cloaked: true, ..win(4, 4) },                  // a suspended UWP app
            WindowInfo { no_activate: true, ..win(5, 5) },              // an overlay
            WindowInfo { minimized: true, ..win(6, 6) },
            WindowInfo { titled: false, ..win(7, 8) },
            win(8, 9), // the app the person was in
            win(9, 10),
        ];
        assert_eq!(pick_paste_window(&windows, own), Some(8));
        assert_eq!(pick_paste_window(&windows[..7], own), None);
    }

    #[test]
    fn remote_desktops_keep_their_clipboard_sync() {
        assert!(syncs_clipboard(Some("mstsc.exe")));
        assert!(syncs_clipboard(Some("MSTSC.EXE")));
        assert!(syncs_clipboard(Some("VirtualBoxVM.exe")));
        assert!(!syncs_clipboard(Some("chrome.exe")));
        assert!(!syncs_clipboard(None));
    }

    #[test]
    fn without_context_a_dictation_pastes_as_it_always_did() {
        assert_eq!(fit_for_paste("Hello there.", None, false, &[], AppKind::Other), "Hello there.");
        assert_eq!(fit_for_paste("Hello there.", None, true, &[], AppKind::Other), "Hello there. ");
    }

    #[test]
    fn with_context_smart_spacing_decides_the_spaces() {
        let ctx = FieldContext {
            before: "I think".into(),
            after: String::new(),
            selection: String::new(),
        };
        // `append_trailing_space` is ignored: nothing follows the caret.
        assert_eq!(fit_for_paste("So we should go.", Some(&ctx), true, &[], AppKind::Other), " so we should go.");
        let dict: Vec<DictionaryEntry> = serde_json::from_str(r#"[{"from":"so","to":"So"}]"#).unwrap();
        assert_eq!(fit_for_paste("So we should go.", Some(&ctx), true, &dict, AppKind::Other), " So we should go.");
    }

    fn terminal(cli: Option<app_kind::CodingCli>) -> AppKind {
        AppKind::Terminal(app_kind::Terminal {
            coding_cli: cli,
            windows_terminal: true,
        })
    }

    #[test]
    fn a_terminal_gets_one_line_and_no_trailing_space() {
        let list = "Fix these:\n- the login bug\n- the slow search.";
        assert_eq!(
            fit_for_paste(list, None, true, &[], terminal(None)),
            "Fix these: the login bug; the slow search."
        );
        // Claude Code in Windows Terminal takes the paste as one block.
        assert_eq!(
            fit_for_paste(list, None, true, &[], terminal(Some(app_kind::CodingCli::ClaudeCode))),
            list
        );
        // Gemini CLI: not known to, so one line.
        assert_eq!(
            fit_for_paste("One.\nTwo.", None, true, &[], terminal(Some(app_kind::CodingCli::GeminiCli))),
            "One. Two."
        );
        // A terminal's context (if any) never fits it: no space either side.
        let ctx = FieldContext {
            before: "PS C:\\> git commit -m".into(),
            after: "x".into(),
            selection: String::new(),
        };
        assert_eq!(fit_for_paste("The fix.", Some(&ctx), true, &[], terminal(None)), "The fix.");
    }

    #[test]
    fn a_chat_message_drops_its_full_stop_then_fits_the_cursor() {
        assert_eq!(fit_for_paste("Sounds good.", None, false, &[], AppKind::Messaging), "Sounds good");
        assert_eq!(fit_for_paste("Sounds good.", None, true, &[], AppKind::Messaging), "Sounds good ");
        assert_eq!(fit_for_paste("Are you in?", None, false, &[], AppKind::Messaging), "Are you in?");
        // After a sentence in the draft: the space smart spacing adds, no full stop.
        let ctx = FieldContext {
            before: "Thanks!".into(),
            after: String::new(),
            selection: String::new(),
        };
        assert_eq!(fit_for_paste("See you then.", Some(&ctx), false, &[], AppKind::Messaging), " See you then");
        // Replacing a selection that ended with one: the selection's end wins.
        let sel = FieldContext {
            before: String::new(),
            after: String::new(),
            selection: "Maybe later.".into(),
        };
        assert_eq!(fit_for_paste("See you then.", Some(&sel), false, &[], AppKind::Messaging), "See you then.");
    }

    /// The real clipboard code on a clipboard of its own: the test process
    /// first moves to a new, private window station (each has its own
    /// clipboard), and checks it did, so the person's clipboard and Win+V
    /// history are never touched. `#[ignore]`d (it changes the whole test
    /// process's window station):
    /// `cargo test --lib -- --ignored private_clipboard --test-threads=1`.
    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "moves the test process to a private window station"]
    fn private_clipboard_round_trip() {
        use std::ffi::c_void;
        #[link(name = "user32")]
        extern "system" {
            fn CreateWindowStationW(name: *const u16, flags: u32, access: u32, sa: *mut c_void) -> *mut c_void;
            fn GetProcessWindowStation() -> *mut c_void;
            fn SetProcessWindowStation(station: *mut c_void) -> i32;
            fn CloseWindowStation(station: *mut c_void) -> i32;
        }
        const WINSTA_ALL_ACCESS: u32 = 0x037F;

        /// Back on the real window station whatever happens.
        struct Back(*mut c_void, *mut c_void);
        impl Drop for Back {
            fn drop(&mut self) {
                unsafe {
                    SetProcessWindowStation(self.0);
                    CloseWindowStation(self.1);
                }
            }
        }

        #[link(name = "kernel32")]
        extern "system" {
            fn GetLastError() -> u32;
        }
        let real_sequence = clip::sequence();
        let _back = unsafe {
            let original = GetProcessWindowStation();
            // No name: a new, unnamed one (as Chromium's sandbox makes them).
            let station = CreateWindowStationW(std::ptr::null(), 0, WINSTA_ALL_ACCESS, std::ptr::null_mut());
            assert!(!station.is_null(), "a private window station (error {})", GetLastError());
            assert_ne!(SetProcessWindowStation(station), 0, "moved to it");
            Back(original, station)
        };
        assert_ne!(clip::sequence(), real_sequence, "a clipboard of our own (else stop before touching it)");

        // A dictation: verified, readable, and carrying the privacy formats.
        assert!(clip::put_text_verified("Hello from Yap", true));
        assert_eq!(clip::read_text().as_deref(), Some("Hello from Yap"));
        for (name, bytes) in clip::privacy_formats() {
            assert_eq!(bytes.as_deref(), Some(&[0u8, 0, 0, 0][..]), "{name}");
        }
        // A remote desktop's paste: plain.
        assert!(clip::put_text_verified("Plain", false));
        assert!(clip::privacy_formats().iter().all(|(_, b)| b.is_none()));

        // The person's text comes back after the paste, kept out of history.
        let before = clip::snapshot();
        assert!(matches!(&before, clip::Snapshot::Text(t) if t == "Plain"));
        assert!(clip::put_text_verified("Dictated", true));
        clip::restore(before, true);
        assert_eq!(clip::read_text().as_deref(), Some("Plain"));
        assert!(clip::privacy_formats().iter().all(|(_, b)| b.is_some()));

        // Copied files (CF_HDROP) come back byte for byte.
        let files = vec![(clip::HDROP, b"a list of files\0\0".to_vec())];
        clip::put_raw(&files);
        let before = clip::snapshot();
        assert!(matches!(&before, clip::Snapshot::Raw(items) if items == &files));
        assert!(clip::put_text_verified("Dictated", true));
        clip::restore(before, true);
        assert_eq!(clip::read_raw(clip::HDROP).0.as_deref(), Some(&b"a list of files\0\0"[..]));
        assert_eq!(clip::read_text(), None);

        // An empty clipboard is empty again.
        clip::clear();
        let before = clip::snapshot();
        assert!(matches!(before, clip::Snapshot::Empty));
        assert!(clip::put_text_verified("Dictated", true));
        clip::restore(before, true);
        assert_eq!(clip::read_raw(clip::HDROP).1, 0);
    }
}

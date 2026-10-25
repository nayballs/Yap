//! Text injection via clipboard + simulated Ctrl+V paste.
//!
//! Used by the dictation feature to inject transcribed text into
//! whatever application/field is currently focused.
//!
//! - **The paste keys**: Ctrl+V, or a terminal's own (Shift+Insert in Git
//!   Bash's mintty, Ctrl+Shift+V in Alacritty…: `app_kind::Host::paste_key`),
//!   sent with their scan codes as a keyboard would.
//! - **Did it land?** Into a Win32 edit control (a WinForms text box, a
//!   classic EDIT or RichEdit) the paste is checked
//!   (`selection::PasteCheck`: the control's length and selection, before
//!   and after): one that stayed exactly as it was for a second gets the
//!   text typed instead, once ([`Delivery::Typed`]). Never in a terminal, a
//!   password box or a read-only field, and nowhere else (a web or code
//!   editor's hidden input says nothing either way).
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
//! - **Apps running as administrator** take no keys from Yap (UIPI drops
//!   them silently): before a paste the target's integrity level is checked
//!   (`elevated.rs`), and an elevated app gets the text left on the
//!   clipboard instead ([`Delivery::LeftOnClipboard`]) — the caller says
//!   "Your text is copied: press Ctrl+V".
//! - **Fitting the dictation to the cursor** ([`prepare_dictation`],
//!   `smart_insert.rs`) and to the kind of app (`app_kind.rs`: one line in a
//!   terminal, no full stop on a short chat message), and remembering it for
//!   **"Paste last transcript"** ([`paste_last_transcript`], also in the
//!   tray menu).

use std::sync::Mutex;

use tracing::{debug, info, trace, warn};

use crate::pasting::app_kind::{self, AppKind};
use crate::config::DictionaryEntry;
use crate::pasting::smart_insert::{self, FieldContext};

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
    const KEYEVENTF_EXTENDEDKEY: u32 = 0x0001;
    const KEYEVENTF_KEYUP: u32 = 0x0002;
    const KEYEVENTF_UNICODE: u32 = 0x0004;
    const VK_SHIFT: u16 = 0x10;
    const VK_CONTROL: u16 = 0x11;
    const VK_INSERT: u16 = 0x2D;
    const VK_C: u16 = 0x43;
    const VK_V: u16 = 0x56;
    const VK_RETURN: u16 = 0x0D;
    /// MapVirtualKeyEx: a virtual key → its scan code, 0xE0__ for an
    /// extended key (Vista and later).
    const MAPVK_VK_TO_VSC_EX: u32 = 4;
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
        fn GetKeyboardLayout(thread: u32) -> HANDLE;
        fn MapVirtualKeyExW(code: u32, map_type: u32, layout: HANDLE) -> u32;
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

    /// Whether `hwnd_isize` is still a window.
    pub fn window_exists(hwnd_isize: isize) -> bool {
        let hwnd = hwnd_isize as HWND;
        !hwnd.is_null() && unsafe { IsWindow(hwnd) } != 0
    }

    /// Whether `hwnd_isize` is the window in front.
    pub fn is_foreground(hwnd_isize: isize) -> bool {
        let hwnd = hwnd_isize as HWND;
        !hwnd.is_null() && unsafe { GetForegroundWindow() } == hwnd
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

    /// A key as a keyboard sends it: the virtual key **and** its scan code
    /// in `layout` (the target's keyboard layout), flagged extended where
    /// the key is (Insert: the one by Home, not the keypad's 0). Apps that
    /// read the physical key from the scan code — winit's (Alacritty, Rio)
    /// match their bindings that way, mintty and PuTTY tell Insert from the
    /// keypad by the extended flag — see the key a person would press.
    fn make_key_scanned(vk: u16, up: bool, layout: HANDLE) -> INPUT {
        let scan = unsafe { MapVirtualKeyExW(u32::from(vk), MAPVK_VK_TO_VSC_EX, layout) };
        let extended = vk == VK_INSERT || matches!(scan & 0xFF00, 0xE000 | 0xE100);
        let mut flags = if up { KEYEVENTF_KEYUP } else { 0 };
        if extended {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        INPUT {
            type_: INPUT_KEYBOARD,
            union: INPUT_UNION {
                ki: KEYBDINPUT {
                    w_vk: vk,
                    w_scan: (scan & 0xFF) as u16,
                    dw_flags: flags,
                    time: 0,
                    dw_extra_info: 0,
                },
            },
        }
    }

    /// Simulate the paste keys via SendInput (into `target`: robot mode
    /// checks it's still in front right after): Ctrl+V, or what the
    /// terminal there binds (`app_kind::Host::paste_key`).
    pub fn simulate_paste(target: Option<isize>, key: crate::pasting::app_kind::PasteKey) {
        use crate::pasting::app_kind::PasteKey;
        let inputs: Vec<INPUT> = match key {
            // As it always was (no scan codes): every app it's been tried in pastes.
            PasteKey::CtrlV => vec![
                make_key(VK_CONTROL, false),
                make_key(VK_V, false),
                make_key(VK_V, true),
                make_key(VK_CONTROL, true),
            ],
            PasteKey::ShiftInsert | PasteKey::CtrlShiftV => {
                let thread = target
                    .map(|h| unsafe { GetWindowThreadProcessId(h as HWND, std::ptr::null_mut()) })
                    .unwrap_or(0);
                let layout = unsafe { GetKeyboardLayout(thread) };
                let k = |vk, up| make_key_scanned(vk, up, layout);
                if key == PasteKey::ShiftInsert {
                    vec![k(VK_SHIFT, false), k(VK_INSERT, false), k(VK_INSERT, true), k(VK_SHIFT, true)]
                } else {
                    vec![
                        k(VK_CONTROL, false),
                        k(VK_SHIFT, false),
                        k(VK_V, false),
                        k(VK_V, true),
                        k(VK_SHIFT, true),
                        k(VK_CONTROL, true),
                    ]
                }
            }
        };

        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                mem::size_of::<INPUT>() as i32,
            )
        };
        crate::shell::realtest::check_after(target, "paste");

        tracing::debug!(
            sent,
            expected = inputs.len(),
            cb_size = mem::size_of::<INPUT>(),
            "SendInput {}",
            key.name()
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

    /// Put a picture on the clipboard — a DIB (`CF_DIBV5`, which Windows
    /// turns into the older bitmap formats) and a PNG (`"PNG"`, which
    /// browsers and image apps prefer) — and check it took, as
    /// [`put_text_verified`] does (Snip to Chat's copy, `snip/copy_save.rs`).
    pub fn put_image_verified(png: &[u8], dibv5: &[u8], private: bool) -> bool {
        let items = [(CF_DIBV5, dibv5.to_vec()), (registered("PNG"), png.to_vec())];
        for (attempt, wait_ms) in [0u64, 15, 40, 80].into_iter().enumerate() {
            sleep(Duration::from_millis(wait_ms));
            let before = sequence();
            match put(&items, private) {
                Ok(()) if sequence() != before => return true,
                Ok(()) => debug!(attempt, "Clipboard image write didn't take (sequence number unchanged)"),
                Err(e) => debug!(attempt, error = %e, "Clipboard image write failed"),
            }
        }
        false
    }

    /// Robot mode: the clipboard's PNG, if it has one.
    #[cfg(debug_assertions)]
    pub fn read_png() -> Option<Vec<u8>> {
        read_format(registered("PNG"))
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

    /// Robot mode: put raw formats on the clipboard, privately.
    #[cfg(debug_assertions)]
    pub fn put_private(items: &[(u32, Vec<u8>)]) -> Result<(), String> {
        put(items, true)
    }

    /// Robot mode: one format's bytes, if it's there.
    #[cfg(debug_assertions)]
    pub fn read_format(format: u32) -> Option<Vec<u8>> {
        let _open = Open::new()?;
        available(format).then(|| read(format)).flatten()
    }

    /// Robot mode: every privacy format is on the clipboard now.
    #[cfg(debug_assertions)]
    pub fn all_private() -> bool {
        let Some(_open) = Open::new() else {
            return false;
        };
        PRIVATE.iter().all(|(name, _)| available(registered(name)))
    }

    #[cfg(debug_assertions)]
    pub const DIBV5: u32 = CF_DIBV5;

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

/// Robot mode's clipboard (`realtest_clipboard`), which never hands the
/// clipboard's contents out: `save` keeps what's there now (the
/// developer's) in memory, `restore` puts it back (as a paste does: text,
/// an image or files); `setText` / `setImage` put a test value there,
/// privately (out of Win+V history); `check` says whether what's there is
/// `text`, the last test image, any text at all, and private.
#[cfg(all(target_os = "windows", debug_assertions))]
pub(crate) fn robot_clipboard(action: &str, text: Option<&str>) -> Result<serde_json::Value, String> {
    use serde_json::json;
    static SAVED: Mutex<Option<clip::Snapshot>> = Mutex::new(None);
    static IMAGE: Mutex<Option<Vec<u8>>> = Mutex::new(None);
    fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
        m.lock().unwrap_or_else(|p| p.into_inner())
    }
    match action {
        "save" => {
            let mut saved = lock(&SAVED);
            if saved.is_none() {
                *saved = Some(clip::snapshot());
            }
            Ok(json!({ "saved": true }))
        }
        "restore" => {
            let snap: Option<clip::Snapshot> = lock(&SAVED).take();
            let restored = snap.is_some();
            if let Some(snap) = snap {
                clip::restore(snap, true);
            }
            Ok(json!({ "restored": restored }))
        }
        "setText" => {
            let text = text.ok_or("setText needs a text")?;
            if !clip::put_text_verified(text, true) {
                return Err("Couldn't put the text on the clipboard".into());
            }
            Ok(json!({ "set": true }))
        }
        "setImage" => {
            let dib = test_image();
            clip::put_private(&[(clip::DIBV5, dib.clone())])?;
            *lock(&IMAGE) = Some(dib);
            Ok(json!({ "set": true }))
        }
        // Snip to Chat's copy (snip/copy_save.rs): whether the clipboard
        // holds exactly the PNG Yap copied last, a DIB beside it, and the
        // privacy formats — its size, never its pixels.
        "checkSnip" => {
            let png = clip::read_png();
            let copied = crate::snip::copy_save::last_copied();
            let size = png.as_deref().and_then(crate::snip::images::png_size);
            Ok(json!({
                "hasPng": png.is_some(),
                "matchesSnip": matches!((&png, &copied), (Some(got), Some(want)) if got.len() >= want.len() && got[..want.len()] == want[..]),
                "hasDib": clip::read_format(clip::DIBV5).is_some(),
                "width": size.map(|s| s.0),
                "height": size.map(|s| s.1),
                "private": clip::all_private(),
            }))
        }
        "check" => {
            let now = clip::read_text();
            let image = lock(&IMAGE).clone();
            let dib = clip::read_format(clip::DIBV5);
            let matches_image = match (image, dib) {
                // (The clipboard's copy can be padded to its allocation size.)
                (Some(want), Some(got)) => got.len() >= want.len() && got[..want.len()] == want[..],
                _ => false,
            };
            Ok(json!({
                "hasText": now.is_some(),
                "matchesText": text.is_some_and(|t| now.as_deref() == Some(t)),
                "matchesImage": matches_image,
                "private": clip::all_private(),
            }))
        }
        other => Err(format!("Unknown clipboard action: {other}")),
    }
}

/// A 16 × 16 32-bit test image as a BITMAPV5HEADER DIB (a diagonal
/// gradient: any change to a byte shows).
#[cfg(all(target_os = "windows", debug_assertions))]
fn test_image() -> Vec<u8> {
    const SIDE: i32 = 16;
    let mut b: Vec<u8> = Vec::with_capacity(124 + (SIDE * SIDE * 4) as usize);
    b.extend_from_slice(&124u32.to_le_bytes()); // bV5Size
    b.extend_from_slice(&SIDE.to_le_bytes()); // width
    b.extend_from_slice(&SIDE.to_le_bytes()); // height (bottom-up)
    b.extend_from_slice(&1u16.to_le_bytes()); // planes
    b.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
    b.extend_from_slice(&3u32.to_le_bytes()); // BI_BITFIELDS
    b.extend_from_slice(&((SIDE * SIDE * 4) as u32).to_le_bytes()); // image size
    b.extend_from_slice(&2835i32.to_le_bytes()); // 72 dpi
    b.extend_from_slice(&2835i32.to_le_bytes());
    b.extend_from_slice(&0u32.to_le_bytes()); // colours used
    b.extend_from_slice(&0u32.to_le_bytes()); // colours important
    for mask in [0x00FF_0000u32, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000] {
        b.extend_from_slice(&mask.to_le_bytes());
    }
    b.extend_from_slice(&0x7352_4742u32.to_le_bytes()); // LCS_sRGB
    b.extend_from_slice(&[0u8; 36]); // endpoints
    b.extend_from_slice(&[0u8; 12]); // gamma
    b.extend_from_slice(&4u32.to_le_bytes()); // LCS_GM_IMAGES
    b.extend_from_slice(&[0u8; 12]); // profile data, size, reserved
    debug_assert_eq!(b.len(), 124);
    for y in 0..SIDE {
        for x in 0..SIDE {
            b.extend_from_slice(&[(x * 16) as u8, (y * 16) as u8, ((x + y) * 8) as u8, 0xFF]);
        }
    }
    b
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

/// How a dictation reached the app it was for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// Pasted (or typed) into it.
    Pasted,
    /// The paste didn't land — a Win32 edit control stayed exactly as it
    /// was (`selection::PasteCheck`) — so the text was typed into it
    /// instead, once.
    Typed,
    /// The app runs as administrator, so Windows would drop Yap's keys
    /// (`elevated.rs`): none were sent, and the text is on the clipboard
    /// for the person to paste. The caller says so (`elevated::alert`).
    LeftOnClipboard,
    /// The paste had to go back to its own window (an Undo) and couldn't:
    /// the window is gone, or Windows wouldn't bring it to the front. No
    /// keys were sent anywhere else; the text is on the clipboard.
    WindowGone,
}

/// Simulate the auto-submit keystroke in the focused window. `key` is one of
/// "enter", "ctrlEnter", or "shiftEnter".
pub async fn press_submit(key: &str) -> Result<(), String> {
    if crate::shell::e2e::active() && !crate::shell::realtest::on() {
        info!("e2e: not pressing {} in another app", key);
        return Ok(());
    }
    let key = key.to_string();
    tokio::task::spawn_blocking(move || {
        // Robot mode: only in a window the test opened, checked right
        // before the key goes out (always fine in a normal run).
        crate::shell::realtest::recheck_foreground(None, &format!("press {key}"))?;
        // An app running as administrator would drop the key anyway.
        if current_foreground().is_some_and(crate::pasting::elevated::target_blocked) {
            info!(key, "Not pressing the key: the app runs as administrator");
            return Err("Yap can't press keys in an app running as administrator".to_string());
        }
        #[cfg(target_os = "windows")]
        platform::press_submit(&key);
        crate::shell::realtest::check_after(None, &format!("{key} key"));
        #[cfg(not(target_os = "windows"))]
        {
            let _ = &key;
            warn!("press_submit is only supported on Windows");
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("press_submit task panicked: {}", e))?
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

/// The app window the person is in, as "Paste last transcript" finds it:
/// the foreground window when it's an app's, else — the tray menu just had
/// the foreground — the app window on top. (Reading the selection aloud
/// from the tray, `read_aloud.rs`.) `None` off Windows.
pub fn app_window() -> Option<isize> {
    #[cfg(target_os = "windows")]
    {
        platform::paste_target()
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// Bring `hwnd` back to the front (best-effort; see `platform::focus_window`).
pub fn focus(hwnd: isize) -> bool {
    #[cfg(target_os = "windows")]
    {
        platform::focus_window(hwnd)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = hwnd;
        false
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
    if crate::shell::e2e::active() {
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
/// …and its words as said, when AI cleanup changed them (corrections and
/// snippets applied, no cleanup): "Paste last transcript (as said)".
static LAST_SAID: Mutex<Option<String>> = Mutex::new(None);

fn last_dictation() -> std::sync::MutexGuard<'static, Option<String>> {
    LAST_DICTATION.lock().unwrap_or_else(|p| p.into_inner())
}

fn last_said() -> std::sync::MutexGuard<'static, Option<String>> {
    LAST_SAID.lock().unwrap_or_else(|p| p.into_inner())
}

/// The finished dictation `text` (cleaned up, corrected) as it should be
/// pasted, remembered for "Paste last transcript": fitted to the kind of app
/// it goes into (`kind`, `app_kind.rs`, already filtered by the settings)
/// and to the text around the cursor when Yap read it at record start
/// (`smart_spacing`), else as before — with a space after it if
/// `append_space`.
pub fn prepare_dictation(text: &str, append_space: bool, dictionary: &[DictionaryEntry], kind: AppKind) -> String {
    *last_dictation() = Some(text.trim().to_string());
    *last_said() = None;
    let ctx = crate::pasting::selection::take_context();
    fit_for_paste(text, ctx.as_ref(), append_space, dictionary, kind)
}

/// [`prepare_dictation`] for a dictation AI cleanup changed, with its words
/// as said (`said`: corrections and snippets, no cleanup) fitted to the same
/// place, for "Use what I said" (`as_said.rs`). Returns both, fitted.
pub fn prepare_dictation_as_said(
    text: &str,
    said: &str,
    append_space: bool,
    dictionary: &[DictionaryEntry],
    kind: AppKind,
) -> (String, String) {
    *last_dictation() = Some(text.trim().to_string());
    *last_said() = Some(said.trim().to_string());
    let ctx = crate::pasting::selection::take_context();
    (
        fit_for_paste(text, ctx.as_ref(), append_space, dictionary, kind),
        fit_for_paste(said, ctx.as_ref(), append_space, dictionary, kind),
    )
}

/// What "Paste last transcript (as said)" would paste (before fitting):
/// this session's latest dictation as said, else history's newest one's
/// transcript through the corrections and snippets.
pub fn last_said_transcript() -> Option<String> {
    // One lock at a time, each let go at the end of its own statement: a
    // guard kept for the whole expression while it locked the last
    // dictation again deadlocked (a std Mutex never returns from that).
    let dictated = last_dictation().clone();
    let said = last_said().clone();
    // The words as said when cleanup changed them, else the dictation.
    let remembered = dictated.as_ref().and(said).or(dictated);
    remembered
        .or_else(|| {
            let raw = crate::dictation::history::latest_dictation_raw()?;
            Some(crate::dictation::pipeline::as_said_text(&raw, &crate::config::load(), None))
        })
        .filter(|t| !t.trim().is_empty())
}

/// Put `text` on the clipboard as a dictation's paste does (out of Win+V
/// history and cloud sync) and leave it there: "Use what I said" couldn't
/// replace the paste. `false` if the clipboard couldn't be written.
pub fn copy_privately(text: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        clip::put_text_verified(text, true)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = text;
        false
    }
}

/// Put a picture on the clipboard (its PNG and a `CF_DIBV5` DIB) the way a
/// dictation's paste goes there: out of Win+V history and cloud sync (Snip
/// to Chat's copy). `false` if the clipboard couldn't be written.
pub fn copy_image_privately(png: &[u8], dibv5: &[u8]) -> bool {
    #[cfg(target_os = "windows")]
    {
        clip::put_image_verified(png, dibv5, true)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (png, dibv5);
        false
    }
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
    *last_said() = None;
}

/// What "Paste last transcript" would paste (before fitting): this
/// session's latest dictation, else history's newest.
pub fn last_transcript() -> Option<String> {
    let remembered = last_dictation().clone();
    remembered
        .or_else(crate::dictation::history::latest_dictation)
        .filter(|t| !t.trim().is_empty())
}

/// Whether there's anything for "Paste last transcript" (the tray item).
pub fn has_last_transcript() -> bool {
    last_transcript().is_some()
}

const NOTHING_TO_PASTE: &str = "Nothing to paste yet — dictate something first";

/// The app at `receiver` runs as administrator: put `text` on the clipboard
/// (privately, as a paste would: out of Win+V history) and leave it there
/// for the person to paste. Nothing else is touched.
#[cfg(target_os = "windows")]
fn leave_on_clipboard(text: &str, receiver: Option<isize>) -> Result<Delivery, String> {
    let private = !syncs_clipboard(app_name_for(receiver).as_deref());
    if clip::put_text_verified(text, private) {
        warn!(len = text.len(), "Not pasted: the text is left on the clipboard");
        Ok(Delivery::LeftOnClipboard)
    } else {
        Err("The text couldn't be pasted, nor put on the clipboard".into())
    }
}

/// "Paste last transcript": the latest dictation into the app window the
/// person is in (the tray menu borrowed the foreground: the top app window),
/// fitted to its cursor like a dictation. For when a paste went astray.
/// Returns what was pasted. A test run pastes nothing and returns it. An app
/// running as administrator gets it on the clipboard instead, and the person
/// is told (`elevated.rs`).
fn paste_last_blocking(app: &tauri::AppHandle, as_said: bool) -> Result<String, String> {
    let text = if as_said { last_said_transcript() } else { last_transcript() }.ok_or(NOTHING_TO_PASTE)?;
    let cfg = crate::config::load();
    let test_mode = || {
        let out = fit_for_paste(&text, None, cfg.append_trailing_space, &cfg.dictionary, AppKind::Other);
        if crate::pasting::elevated::test_override() {
            info!(len = out.len(), "e2e: the app counts as an admin app; not pasting the last transcript");
            crate::notices::alert::show(app, crate::pasting::elevated::alert());
        } else {
            info!(len = out.len(), "e2e: not pasting the last transcript");
        }
        Ok(out)
    };
    if crate::shell::e2e::active() && !crate::shell::realtest::on() {
        return test_mode();
    }
    #[cfg(target_os = "windows")]
    {
        let target = platform::paste_target();
        match crate::shell::realtest::gate(target, "paste the last transcript") {
            crate::shell::realtest::Gate::Go => {}
            crate::shell::realtest::Gate::TestMode => return test_mode(),
            crate::shell::realtest::Gate::Refused(why) => return Err(why),
        }
        // An app running as administrator: on the clipboard, as it is.
        if target.is_some_and(crate::pasting::elevated::target_blocked) {
            let out = fit_for_paste(&text, None, cfg.append_trailing_space, &cfg.dictionary, AppKind::Other);
            warn!("The app runs as administrator: Windows won't let Yap type into it");
            leave_on_clipboard(&out, target)?;
            crate::notices::alert::show(app, crate::pasting::elevated::alert());
            return Ok(out);
        }
        if let Some(hwnd) = target {
            platform::focus_window(hwnd);
            // The app puts its own focus back in its field on activation.
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
        // A terminal or a chat app gets it as a dictation would; a
        // terminal's screen isn't read. (The paste keys are the window's
        // whatever the settings say: `raw`.)
        let raw = app_kind::of_window(target);
        let kind = raw.with_settings(cfg.terminal_mode, cfg.chat_no_full_stop);
        let ctx = if cfg.smart_spacing && target.is_some() && !kind.is_terminal() {
            crate::pasting::selection::read_context(target)
        } else {
            None
        };
        let out = fit_for_paste(&text, ctx.as_ref(), cfg.append_trailing_space, &cfg.dictionary, kind);
        info!(len = out.len(), found_window = target.is_some(), fitted = ctx.is_some(), "Pasting the last transcript");
        if inject_text_sync(&out, cfg.restore_clipboard, target, false, raw)? == Delivery::LeftOnClipboard {
            crate::notices::alert::show(app, crate::pasting::elevated::alert());
        }
        Ok(out)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        Err("Pasting is only supported on Windows".into())
    }
}

/// "Paste last transcript" (the tray menu; also callable from the page):
/// pastes the latest dictation into the app you're in and returns it. Errors
/// when there's nothing yet.
#[tauri::command]
pub async fn paste_last_transcript(app: tauri::AppHandle, as_said: Option<bool>) -> Result<String, String> {
    let as_said = as_said.unwrap_or(false);
    tokio::task::spawn_blocking(move || paste_last_blocking(&app, as_said))
        .await
        .map_err(|e| format!("Paste task failed: {e}"))?
}

/// The tray's "Paste last transcript" (off the main thread); `as_said`:
/// "Paste last transcript (as said)", the words before AI cleanup.
pub fn paste_last_from_tray(app: &tauri::AppHandle, as_said: bool) {
    let app = app.clone();
    std::thread::spawn(move || {
        if let Err(e) = paste_last_blocking(&app, as_said) {
            warn!("Paste last transcript: {}", e);
        }
    });
}

/// Inject text into the target field via clipboard + Ctrl+V.
///
/// `target_hwnd` (from [`current_foreground`] at record-start) is re-focused
/// before pasting so focus changes during transcription don't misfire. Runs on a
/// blocking thread to avoid blocking the tokio runtime. An app running as
/// administrator gets no keys: the text is left on the clipboard
/// ([`Delivery::LeftOnClipboard`]) and the caller tells the person.
/// `strict` (an Undo, seconds after the dictation): the paste goes back to
/// `target_hwnd` or nowhere — a window that's gone or won't come to the
/// front leaves the text on the clipboard ([`Delivery::WindowGone`]) rather
/// than pasting into whatever has the focus now. `kind`: the kind of app
/// the window is, as read (before the settings): which keys paste there
/// (`AppKind::paste_key`; Shift+Insert in Git Bash…), and — anything but a
/// terminal — whether to check a Win32 edit control got the paste.
pub async fn inject_text(
    text: &str,
    restore_clipboard: bool,
    target_hwnd: Option<isize>,
    strict: bool,
    kind: AppKind,
) -> Result<Delivery, String> {
    // An Undo whose window has closed since: never into another one.
    if strict && target_hwnd.is_some_and(|h| !window_exists(h)) {
        if crate::shell::e2e::active() && !crate::shell::realtest::on() {
            info!("e2e: the dictation's window is gone; not pasting the text");
            return Ok(Delivery::WindowGone);
        }
        #[cfg(target_os = "windows")]
        {
            let text = text.to_string();
            return tokio::task::spawn_blocking(move || leave_on_clipboard(&text, None).map(|_| Delivery::WindowGone))
                .await
                .map_err(|e| format!("Inject task panicked: {}", e))?;
        }
    }
    // An e2e test run never pastes into another app (or touches the
    // clipboard); robot mode only into a window the test opened.
    match crate::shell::realtest::gate(target_hwnd, "paste") {
        crate::shell::realtest::Gate::Go => {}
        crate::shell::realtest::Gate::TestMode => {
            if crate::pasting::elevated::test_override() {
                info!(len = text.len(), "e2e: the target counts as an admin app; not pasting the text");
                return Ok(Delivery::LeftOnClipboard);
            }
            info!(len = text.len(), "e2e: not pasting the text");
            return Ok(Delivery::Pasted);
        }
        crate::shell::realtest::Gate::Refused(why) => return Err(why),
    }
    let text = text.to_string();

    tokio::task::spawn_blocking(move || inject_text_sync(&text, restore_clipboard, target_hwnd, strict, kind))
        .await
        .map_err(|e| format!("Inject task panicked: {}", e))?
}

/// How long a paste into a Win32 edit control has to show up before Yap
/// takes it that it never will (`selection::PasteCheck`). Such a control
/// takes a paste in a few milliseconds; an app too busy to say within
/// this gets no second try (a read that times out decides nothing).
const PASTE_LANDS_MS: u64 = 1_000;
/// How often the control is read meanwhile.
const PASTE_CHECK_EVERY_MS: u64 = 40;

/// Wait for the paste `check` was taken before to show in its control:
/// the first read that sees a change says it landed, one that fails says
/// nothing more, and a control unchanged for [`PASTE_LANDS_MS`] since the
/// keys went out (`pasted_at`) didn't get it.
fn wait_for_paste(check: &crate::pasting::selection::PasteCheck, pasted_at: std::time::Instant) -> crate::pasting::selection::Landed {
    let wait = std::time::Duration::from_millis(PASTE_LANDS_MS);
    loop {
        let waited_out = pasted_at.elapsed() >= wait;
        if let Some(verdict) = check.verdict(waited_out) {
            return verdict;
        }
        std::thread::sleep(std::time::Duration::from_millis(PASTE_CHECK_EVERY_MS));
    }
}

/// Whether `hwnd` is still a window (`true` off Windows: nothing to check).
pub fn window_exists(hwnd: isize) -> bool {
    #[cfg(target_os = "windows")]
    {
        platform::window_exists(hwnd)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = hwnd;
        true
    }
}

fn inject_text_sync(
    text: &str,
    restore_clipboard: bool,
    target_hwnd: Option<isize>,
    strict: bool,
    kind: AppKind,
) -> Result<Delivery, String> {
    let key = kind.paste_key();
    info!(len = text.len(), keys = key.name(), "Injecting text via clipboard paste");

    // Small delay to let focus settle after voice recording stops.
    std::thread::sleep(std::time::Duration::from_millis(50));

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (restore_clipboard, target_hwnd, strict, key);
        warn!("Text injection is only supported on Windows");
        Err("Text injection is only supported on Windows".into())
    }

    #[cfg(target_os = "windows")]
    {
        // An app running as administrator takes no keys from Yap (Windows
        // drops them without a word): the text stays on the clipboard for
        // the person to paste (`elevated.rs`).
        let receiver = target_hwnd.or_else(current_foreground);
        if receiver.is_some_and(crate::pasting::elevated::target_blocked) {
            warn!("The app runs as administrator: Windows won't let Yap type into it");
            return leave_on_clipboard(text, receiver);
        }

        // Re-focus the window the user was dictating into (best-effort; an
        // Undo's paste goes there or nowhere).
        if let Some(hwnd) = target_hwnd {
            let back = platform::focus_window(hwnd) && platform::is_foreground(hwnd);
            if !back && strict {
                warn!("Couldn't get back to the dictation's window; leaving the text on the clipboard");
                return leave_on_clipboard(text, None).map(|_| Delivery::WindowGone);
            }
            if !back {
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
            // Robot mode: still the test's window in front (always fine in a
            // normal run).
            let checked = crate::shell::realtest::recheck_foreground(target_hwnd, "type the text");
            if checked.is_ok() {
                platform::type_unicode(text);
                crate::shell::realtest::check_after(target_hwnd, "typing");
            }
            if let Some(prev) = previous {
                clip::restore(prev, private);
            }
            return checked.map(|()| Delivery::Pasted);
        }
        trace!(private, "Dictation on the clipboard");

        // Small delay to ensure clipboard is ready.
        std::thread::sleep(std::time::Duration::from_millis(30));

        // A Win32 edit control (a WinForms text box, a classic EDIT or
        // RichEdit), as it stands before the keys: to check the paste lands
        // there (never a terminal's).
        let check = if kind.is_terminal() {
            None
        } else {
            crate::pasting::selection::PasteCheck::before(target_hwnd)
        };

        // Robot mode: the window in front must still be the test's, and the
        // one this paste is for — checked right before the keys go out.
        if let Err(why) = crate::shell::realtest::recheck_foreground(target_hwnd, "paste") {
            if let Some(prev) = previous {
                clip::restore(prev, private);
            }
            return Err(why);
        }

        // Ctrl+V, or the terminal's own paste keys.
        platform::simulate_paste(target_hwnd, key);
        let pasted_at = std::time::Instant::now();

        // Did it land? A control that stayed exactly as it was for a second
        // didn't get it (a paste lost to a busy clipboard, an app that
        // ignores Ctrl+V): the text is typed into it instead — once, only
        // while it still has the focus.
        let mut delivery = Delivery::Pasted;
        if let Some(check) = &check {
            match wait_for_paste(check, pasted_at) {
                crate::pasting::selection::Landed::Yes => debug!("The paste reached the field"),
                crate::pasting::selection::Landed::Unknown => debug!("Couldn't tell whether the paste reached the field"),
                crate::pasting::selection::Landed::No => {
                    let checked = if check.still_focused() {
                        crate::shell::realtest::recheck_foreground(target_hwnd, "type the text")
                    } else {
                        Err("the field lost the focus".to_string())
                    };
                    match checked {
                        Ok(()) => {
                            warn!(
                                len = text.len(),
                                "The paste didn't reach the field (it hadn't changed after {} ms): typing the text instead",
                                PASTE_LANDS_MS
                            );
                            platform::type_unicode(text);
                            crate::shell::realtest::check_after(target_hwnd, "typing");
                            delivery = Delivery::Typed;
                        }
                        Err(why) => warn!("The paste didn't reach the field, and it can't be typed there: {why}"),
                    }
                }
            }
        }

        // Restore previous clipboard (delayed to ensure paste completes). Webview/
        // Electron apps process Ctrl+V asynchronously (browser→renderer IPC) and can
        // take well over 200 ms to actually read the clipboard — restoring too early
        // makes the paste land empty or with the OLD contents. 500 ms is still
        // imperceptible (it runs after the text has visually appeared in fast apps).
        if let Some(prev) = previous {
            let wait = std::time::Duration::from_millis(500).saturating_sub(pasted_at.elapsed());
            std::thread::sleep(wait);
            clip::restore(prev, private);
        }

        Ok(delivery)
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

    /// "Paste last transcript (as said)" after a dictation AI cleanup didn't
    /// change (cleanup off, the default): the dictation itself, at once. It
    /// took the last dictation's lock again inside the statement that held
    /// it, and a std Mutex never returns from that: the tray's worker hung,
    /// then the tray's menu and the next dictation behind it — Yap froze.
    /// (The one test that uses the last dictation itself.)
    #[test]
    fn paste_last_as_said_after_a_dictation_cleanup_left_alone_never_hangs() {
        let ask = || {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(last_said_transcript());
            });
            rx.recv_timeout(std::time::Duration::from_secs(5))
                .expect("last_said_transcript never returned: it deadlocked on the last dictation's lock")
        };
        prepare_dictation("Send the plan to Sam.", false, &[], AppKind::Other);
        assert_eq!(ask().as_deref(), Some("Send the plan to Sam."));
        assert_eq!(last_transcript().as_deref(), Some("Send the plan to Sam."));
        // Cleanup changed it: the words as said; the plain paste, the cleaned text.
        let (text, said) = prepare_dictation_as_said("Send the plan to Sam.", "um send the plan to sam", false, &[], AppKind::Other);
        assert_eq!((text.as_str(), said.as_str()), ("Send the plan to Sam.", "um send the plan to sam"));
        assert_eq!(ask().as_deref(), Some("um send the plan to sam"));
        assert_eq!(last_transcript().as_deref(), Some("Send the plan to Sam."));
        // And still there to read after the as-said one (the locks let go).
        assert!(has_last_transcript());
        forget_last_transcript();
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
            host: app_kind::Host::WindowsTerminal,
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
        // An empty message box, read as empty (selection.rs): no full stop,
        // and no space after it whatever "Append trailing space" says.
        let empty = FieldContext::default();
        assert_eq!(fit_for_paste("Sounds good.", Some(&empty), true, &[], AppKind::Messaging), "Sounds good");
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

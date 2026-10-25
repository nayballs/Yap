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
    #[derive(Copy, Clone)]
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
    const WM_COMMAND: u32 = 0x0111;
    /// A console window's Edit → Paste (conhost's `ID_CONSOLE_PASTE`).
    const ID_CONSOLE_PASTE: usize = 0xFFF1;

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
        fn PostMessageW(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> i32;
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
    /// consecutive units, which Windows recombines). In chunks, each with the
    /// modifiers sorted around it ([`send_keys`]): a held Alt would make
    /// every letter a menu key. `Err` when Windows didn't take them all.
    pub fn type_unicode(text: &str) -> Result<(), String> {
        let mut inputs: Vec<INPUT> = Vec::with_capacity(text.len() * 2);
        for unit in text.encode_utf16() {
            inputs.push(make_unicode(unit, false));
            inputs.push(make_unicode(unit, true));
        }
        // Send in chunks so a very long transcript doesn't overflow the input
        // queue in a single call.
        for chunk in inputs.chunks(512) {
            send_keys(chunk.to_vec(), "typing")?;
            thread::sleep(Duration::from_millis(2));
        }
        Ok(())
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

    /// A modifier Yap lets go of or presses again (`modifiers.rs`): its
    /// side-specific key, scan code and extended flag, as a keyboard sends it.
    fn make_modifier(key: crate::pasting::modifiers::Key) -> INPUT {
        let scan = unsafe { MapVirtualKeyExW(u32::from(key.vk), MAPVK_VK_TO_VSC_EX, GetKeyboardLayout(0)) };
        let mut flags = if key.up { KEYEVENTF_KEYUP } else { 0 };
        if key.extended {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        INPUT {
            type_: INPUT_KEYBOARD,
            union: INPUT_UNION {
                ki: KEYBDINPUT {
                    w_vk: key.vk,
                    w_scan: (scan & 0xFF) as u16,
                    dw_flags: flags,
                    time: 0,
                    dw_extra_info: 0,
                },
            },
        }
    }

    /// One `SendInput` call, checked: Windows says how many events it put
    /// in the input stream, and fewer than asked (a secure desktop — a UAC
    /// prompt, the lock screen —, another app blocking input, UIPI) means
    /// the keys didn't go. Any key the events that did go left down is let
    /// go again. `Err(why)` for the log.
    fn send(inputs: &[INPUT], what: &str) -> Result<(), String> {
        if inputs.is_empty() {
            return Ok(());
        }
        #[cfg(debug_assertions)]
        let blocked = super::test_keys_blocked();
        #[cfg(not(debug_assertions))]
        let blocked = false;
        let sent = if blocked {
            0
        } else {
            unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), mem::size_of::<INPUT>() as i32) }
        };
        if sent as usize == inputs.len() {
            return Ok(());
        }
        let error = if blocked {
            "a test run's stand-in for a blocked input".to_string()
        } else {
            std::io::Error::last_os_error().to_string()
        };
        tracing::warn!(sent, expected = inputs.len(), "Windows didn't take Yap's keys ({what}): {error}");
        // Keys the events that went in left down: up again (best-effort).
        let left_down = keys_left_down(&inputs[..sent as usize]);
        if !left_down.is_empty() {
            let ups: Vec<INPUT> = left_down.into_iter().map(|vk| make_key(vk, true)).collect();
            unsafe { SendInput(ups.len() as u32, ups.as_ptr(), mem::size_of::<INPUT>() as i32) };
        }
        Err(format!("Windows took {sent} of {} key events ({error})", inputs.len()))
    }

    /// The virtual keys `inputs` press and don't let go (Unicode characters
    /// aside).
    fn keys_left_down(inputs: &[INPUT]) -> Vec<u16> {
        let mut down: Vec<u16> = Vec::new();
        for input in inputs {
            let ki = unsafe { input.union.ki };
            if input.type_ != INPUT_KEYBOARD || ki.dw_flags & KEYEVENTF_UNICODE != 0 || ki.w_vk == 0 {
                continue;
            }
            if ki.dw_flags & KEYEVENTF_KEYUP != 0 {
                down.retain(|&vk| vk != ki.w_vk);
            } else if !down.contains(&ki.w_vk) {
                down.push(ki.w_vk);
            }
        }
        down
    }

    /// Yap's keys (`body`) with the modifiers sorted around them
    /// (`modifiers.rs`): any Windows says is down goes up first — Ctrl+V
    /// with a stuck Alt is Ctrl+Alt+V —, and those the person is holding
    /// come back down after, in the same call (Windows never interleaves
    /// other input with one `SendInput`). Checked ([`send`]); after a
    /// failure, a modifier it let go of that the person holds is pressed
    /// again (best-effort).
    fn send_keys(body: Vec<INPUT>, what: &str) -> Result<(), String> {
        use crate::pasting::modifiers;
        let plan = modifiers::plan(modifiers::down_now(), modifiers::held());
        if plan.is_empty() {
            return send(&body, what);
        }
        tracing::info!(
            stuck = %modifiers::names(plan.release & !plan.restore),
            held = %modifiers::names(plan.restore),
            "Modifiers down before {what}: let go of around Yap's keys"
        );
        let (before, after) = modifiers::around(plan);
        let inputs: Vec<INPUT> = before
            .into_iter()
            .map(make_modifier)
            .chain(body)
            .chain(after.into_iter().map(make_modifier))
            .collect();
        let result = send(&inputs, what);
        if result.is_err() {
            let missing = plan.restore & !modifiers::down_now();
            if missing != 0 {
                let (_, again) = modifiers::around(modifiers::Plan { release: 0, restore: missing });
                let again: Vec<INPUT> = again.into_iter().map(make_modifier).collect();
                unsafe { SendInput(again.len() as u32, again.as_ptr(), mem::size_of::<INPUT>() as i32) };
            }
        }
        result
    }

    /// Simulate the paste keys via SendInput (into `target`: robot mode
    /// checks it's still in front right after): Ctrl+V, or what the
    /// terminal there binds (`app_kind::Host::paste_key`). `Err` when
    /// Windows didn't take them.
    pub fn simulate_paste(target: Option<isize>, key: crate::pasting::app_kind::PasteKey) -> Result<(), String> {
        use crate::pasting::app_kind::PasteKey;
        // A classic console window: its own Edit → Paste, no keys (a raw
        // keyboard program there gets Ctrl+V as a ^V).
        if key == PasteKey::ConsoleMenu {
            if let Some(hwnd) = target.filter(|&h| class_of(h as HWND) == "ConsoleWindowClass") {
                #[cfg(debug_assertions)]
                if super::test_keys_blocked() {
                    return Err("a test run's stand-in for a refused paste".into());
                }
                let posted = unsafe { PostMessageW(hwnd as HWND, WM_COMMAND, ID_CONSOLE_PASTE, 0) } != 0;
                crate::shell::realtest::check_after(target, "paste");
                tracing::debug!(posted, "The console's own Paste");
                thread::sleep(Duration::from_millis(80));
                return if posted {
                    Ok(())
                } else {
                    Err(format!("the console didn't take its Paste ({})", std::io::Error::last_os_error()))
                };
            }
        }
        let inputs: Vec<INPUT> = match key {
            // As it always was (no scan codes): every app it's been tried in pastes.
            PasteKey::CtrlV | PasteKey::ConsoleMenu => vec![
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

        let sent = send_keys(inputs, key.name());
        crate::shell::realtest::check_after(target, "paste");
        tracing::debug!(ok = sent.is_ok(), "SendInput {}", key.name());

        // Brief delay to let the target app process the paste
        thread::sleep(Duration::from_millis(80));
        sent
    }

    /// Simulate Ctrl+C keystroke via SendInput (used by the edit-mode selection
    /// capture fallback to copy the current selection into the clipboard).
    pub fn simulate_copy() -> Result<(), String> {
        let inputs = vec![
            make_key(VK_CONTROL, false),
            make_key(VK_C, false),
            make_key(VK_C, true),
            make_key(VK_CONTROL, true),
        ];
        send_keys(inputs, "Ctrl+C")
    }

    /// Simulate a submit keystroke via SendInput (used by auto-submit):
    /// plain Enter, Ctrl+Enter, or Shift+Enter depending on `key`. `Err`
    /// when Windows didn't take them.
    pub fn press_submit(key: &str) -> Result<(), String> {
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

        let sent = send_keys(inputs, key);
        tracing::debug!(ok = sent.is_ok(), key, "SendInput submit");
        thread::sleep(Duration::from_millis(20));
        sent
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
        CloseClipboard, CountClipboardFormats, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
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
    #[cfg(test)]
    pub fn clear() {
        if let Some(_open) = Open::new() {
            let _ = unsafe { EmptyClipboard() };
        }
    }

    /// The clipboard, open, trying for up to `patience` while another app
    /// holds it (a clipboard manager, the app reading a paste…).
    fn open_within(patience: Duration) -> Option<Open> {
        let started = std::time::Instant::now();
        loop {
            if unsafe { OpenClipboard(None) }.is_ok() {
                return Some(Open);
            }
            if started.elapsed() >= patience {
                return None;
            }
            sleep(Duration::from_millis(25));
        }
    }

    const CF_BITMAP: u32 = 2;
    const CF_METAFILEPICT: u32 = 3;
    const CF_DIB: u32 = 8;
    const CF_PALETTE: u32 = 9;
    const CF_ENHMETAFILE: u32 = 14;

    /// One clipboard format as Yap can put it back.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum Item {
        /// Memory (nearly every format): its bytes.
        Bytes(u32, Vec<u8>),
        /// An enhanced metafile (Office's pictures): its bits.
        Metafile(Vec<u8>),
    }

    /// Everything on the clipboard, every format byte for byte — for edit
    /// mode's Ctrl+C, which borrows the clipboard: put back exactly, or the
    /// clipboard is never touched.
    pub enum Exact {
        /// Every format (none: it was empty).
        Kept(Vec<Item>),
        /// Another app held it the whole time.
        Busy,
        /// It holds something Yap can't put back exactly (why).
        CantKeep(&'static str),
    }

    /// Whether a format Windows lists can be left out because Windows makes
    /// it again from another one that's kept (`formats`: all listed): the
    /// bitmap and palette from a DIB, the old metafile picture from an
    /// enhanced metafile. Pure.
    pub fn remade(format: u32, formats: &[u32]) -> bool {
        let dib = formats.contains(&CF_DIB) || formats.contains(&CF_DIBV5);
        match format {
            CF_BITMAP | CF_PALETTE => dib,
            CF_METAFILEPICT => formats.contains(&CF_ENHMETAFILE),
            _ => false,
        }
    }

    /// Formats whose data is a handle Yap can't copy: owner-drawn, GDI
    /// objects and private handles (and those Windows can't remake from
    /// another format). Pure.
    pub fn uncopyable(format: u32) -> bool {
        matches!(format, CF_BITMAP | CF_METAFILEPICT | CF_PALETTE | 0x80 | 0x82 | 0x83 | 0x8E)
            || (0x200..=0x3FF).contains(&format)
    }

    /// OLE's own bookkeeping for the app that copied (a live object of its
    /// process): never put back; OLE answers from the formats themselves.
    const OLE_INTERNAL: [&str; 2] = ["DataObject", "Ole Private Data"];

    /// [`Exact`] now, waiting up to `patience` for the clipboard.
    pub fn snapshot_exact(patience: Duration) -> Exact {
        let Some(_open) = open_within(patience) else {
            return Exact::Busy;
        };
        let mut formats = Vec::new();
        let mut f = 0u32;
        loop {
            f = unsafe { EnumClipboardFormats(f) };
            if f == 0 {
                break;
            }
            formats.push(f);
        }
        let ole: Vec<u32> = OLE_INTERNAL.iter().map(|n| registered(n)).collect();
        let mut items = Vec::new();
        let mut total = 0usize;
        for &format in &formats {
            if remade(format, &formats) || ole.contains(&format) {
                continue;
            }
            if uncopyable(format) {
                return Exact::CantKeep("something only the app that copied it can draw");
            }
            let item = if format == CF_ENHMETAFILE {
                match read_metafile() {
                    Some(bits) => Item::Metafile(bits),
                    None => return Exact::CantKeep("a picture Yap couldn't read"),
                }
            } else {
                match read_any(format) {
                    Ok(Some(bytes)) => Item::Bytes(format, bytes),
                    // Listed but nothing behind it (an app that couldn't
                    // render it): nothing to lose.
                    Ok(None) => continue,
                    Err(why) => return Exact::CantKeep(why),
                }
            };
            total += match &item {
                Item::Bytes(_, b) | Item::Metafile(b) => b.len(),
            };
            if total > MAX_SNAPSHOT_BYTES {
                return Exact::CantKeep("more than Yap keeps");
            }
            items.push(item);
        }
        debug!(formats = items.len(), bytes = total, "Clipboard kept, every format");
        Exact::Kept(items)
    }

    /// One memory format's bytes (the clipboard open), however small:
    /// `Ok(None)` when nothing is behind it, `Err` when it isn't memory.
    fn read_any(format: u32) -> Result<Option<Vec<u8>>, &'static str> {
        unsafe {
            let Ok(handle) = GetClipboardData(format) else { return Ok(None) };
            if handle.is_invalid() {
                return Ok(None);
            }
            let global = HGLOBAL(handle.0);
            let size = GlobalSize(global);
            if size > MAX_SNAPSHOT_BYTES {
                return Err("more than Yap keeps");
            }
            let ptr = GlobalLock(global) as *const u8;
            if ptr.is_null() {
                return Err("something that isn't plain data");
            }
            let bytes = std::slice::from_raw_parts(ptr, size).to_vec();
            let _ = GlobalUnlock(global);
            Ok(Some(bytes))
        }
    }

    /// The enhanced metafile's bits (the clipboard open).
    fn read_metafile() -> Option<Vec<u8>> {
        use windows::Win32::Graphics::Gdi::{GetEnhMetaFileBits, HENHMETAFILE};
        unsafe {
            let handle = GetClipboardData(CF_ENHMETAFILE).ok()?;
            let emf = HENHMETAFILE(handle.0);
            let size = GetEnhMetaFileBits(emf, None);
            if size == 0 || size as usize > MAX_SNAPSHOT_BYTES {
                return None;
            }
            let mut bits = vec![0u8; size as usize];
            (GetEnhMetaFileBits(emf, Some(&mut bits)) == size).then_some(bits)
        }
    }

    /// Put back what [`snapshot_exact`] kept — every format, then the
    /// keep-it-out-of-history ones when `private` — waiting up to
    /// `patience` for the clipboard. `true` once the write took (its
    /// sequence number moved).
    pub fn restore_exact(items: &[Item], private: bool, patience: Duration) -> bool {
        let started = std::time::Instant::now();
        loop {
            let left = patience.saturating_sub(started.elapsed());
            let before = sequence();
            if let Some(_open) = open_within(left) {
                match put_items(items, private) {
                    Ok(()) if sequence() != before => return true,
                    Ok(()) => debug!("Clipboard restore didn't take (sequence number unchanged)"),
                    Err(e) => debug!(error = %e, "Clipboard restore failed"),
                }
            }
            if started.elapsed() >= patience {
                return false;
            }
            sleep(Duration::from_millis(50));
        }
    }

    /// The clipboard emptied and `items` put on it (it's open).
    fn put_items(items: &[Item], private: bool) -> Result<(), String> {
        use windows::Win32::Graphics::Gdi::{DeleteEnhMetaFile, SetEnhMetaFileBits};
        unsafe { EmptyClipboard() }.map_err(|e| format!("EmptyClipboard: {e}"))?;
        for item in items {
            match item {
                Item::Bytes(format, bytes) => set(*format, bytes)?,
                Item::Metafile(bits) => unsafe {
                    let emf = SetEnhMetaFileBits(bits);
                    if emf.is_invalid() {
                        return Err("SetEnhMetaFileBits failed".into());
                    }
                    if let Err(e) = SetClipboardData(CF_ENHMETAFILE, Some(HANDLE(emf.0))) {
                        let _ = DeleteEnhMetaFile(Some(emf));
                        return Err(format!("SetClipboardData (a metafile): {e}"));
                    }
                },
            }
        }
        if private && !items.is_empty() {
            for (name, value) in PRIVATE {
                if let Err(e) = set(registered(name), &value.to_le_bytes()) {
                    debug!(format = name, error = %e, "Couldn't add a privacy format");
                }
            }
        }
        Ok(())
    }

    /// Robot mode and tests: the clipboard's formats as Yap keeps them,
    /// each format's number and a hash of its bytes (never the bytes).
    #[cfg(debug_assertions)]
    pub fn fingerprint() -> Option<Vec<(u32, u64)>> {
        use std::hash::{Hash, Hasher};
        match snapshot_exact(Duration::from_millis(500)) {
            Exact::Kept(items) => {
                let mut all: Vec<(u32, u64)> = items
                    .iter()
                    .map(|item| {
                        let (format, bytes) = match item {
                            Item::Bytes(f, b) => (*f, b),
                            Item::Metafile(b) => (CF_ENHMETAFILE, b),
                        };
                        let mut h = std::collections::hash_map::DefaultHasher::new();
                        bytes.hash(&mut h);
                        (format, h.finish())
                    })
                    .collect();
                all.sort_unstable();
                Some(all)
            }
            _ => None,
        }
    }

    /// Robot mode: a registered format's number.
    #[cfg(debug_assertions)]
    pub fn format_id(name: &str) -> u32 {
        registered(name)
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

    /// Tests: text as a word processor copies it (plain, HTML, its own).
    #[cfg(test)]
    pub fn rich_items() -> Vec<(u32, Vec<u8>)> {
        vec![
            (CF_UNICODETEXT, text_bytes("Quarterly plan")),
            (registered("HTML Format"), b"Version:0.9\r\n<b>Quarterly plan</b>".to_vec()),
            (registered("Yap Test Format"), (0u8..=255).collect()),
        ]
    }
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
        // Text as a word processor copies it: plain text, HTML and a
        // format of the app's own (privately).
        "setRich" => {
            let text = text.ok_or("setRich needs a text")?;
            let html = format!(
                "Version:0.9\r\nStartHTML:-1\r\nEndHTML:-1\r\nStartFragment:0000000000\r\nEndFragment:0000000000\r\n<b>{text}</b>"
            );
            let unicode: Vec<u8> = text.encode_utf16().chain(std::iter::once(0)).flat_map(|u| u.to_le_bytes()).collect();
            clip::put_private(&[
                (13, unicode),
                (clip::format_id("HTML Format"), html.into_bytes()),
                (clip::format_id("Yap Robot Format"), (0u8..=255).collect()),
            ])?;
            Ok(json!({ "set": true }))
        }
        // Every format on the clipboard as Yap keeps it: its number and a
        // hash of its bytes (never the bytes).
        "fingerprint" => Ok(json!({ "formats": clip::fingerprint() })),
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
    /// It wasn't pasted, and the text is on the clipboard for the person to
    /// paste: the caller says so, once ([`Left::alert`]).
    LeftOnClipboard(Left),
    /// The paste had to go back to its own window (an Undo) and couldn't:
    /// the window is gone, or Windows wouldn't bring it to the front. No
    /// keys were sent anywhere else; the text is on the clipboard.
    WindowGone,
}

/// Why a dictation was left on the clipboard instead of pasted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Left {
    /// The app runs as administrator, so Windows would drop Yap's keys
    /// (`elevated.rs`): none were sent.
    Elevated,
    /// Windows didn't take the paste keys (`SendInput` put in fewer events
    /// than asked: a secure desktop, another app blocking input, UIPI), even
    /// on a second try.
    KeysBlocked,
    /// Nothing that takes text had the focus — UI Automation said so
    /// plainly, before the keys and after them (`selection::focus_editable`)
    /// — so the paste landed nowhere.
    NoTextBox,
}

impl Left {
    /// The one notice for it (a toast, a bar card or a Windows
    /// notification, through `notices/alert.rs`): the text is copied.
    pub fn alert(self) -> crate::notices::alert::Alert {
        let (title, body) = match self {
            Left::Elevated => return crate::pasting::elevated::alert(),
            Left::KeysBlocked => (
                "Couldn't paste · it's copied, press Ctrl+V",
                "Windows didn't let Yap press keys just then (a secure screen or another app had the keyboard), so your dictation is on the clipboard instead.",
            ),
            Left::NoTextBox => (
                "Select a text box first",
                "Nothing that takes text had the focus, so your dictation is on the clipboard: click where it goes and press Ctrl+V.",
            ),
        };
        crate::notices::alert::Alert {
            kind: "pasteBlocked",
            icon: "clipboard",
            title: title.to_string(),
            body: body.to_string(),
            action: None,
            timeout_ms: 12_000,
        }
    }

    /// Why it isn't pasted, for history.
    pub fn not_pasted(self) -> &'static str {
        match self {
            Left::Elevated => "Left on the clipboard: the app runs as administrator",
            Left::KeysBlocked => "Left on the clipboard: Windows didn't take the paste keys",
            Left::NoTextBox => "Left on the clipboard: no text box had the focus",
        }
    }

    /// For the robot's report and the log.
    pub fn code(self) -> &'static str {
        match self {
            Left::Elevated => "admin",
            Left::KeysBlocked => "keysBlocked",
            Left::NoTextBox => "noTextBox",
        }
    }
}

/// Test mode: a paste fails this way (0 none, 1 keys blocked, 2 no text
/// box). Plain test mode says what each says; robot mode really leaves
/// the text on the clipboard for keys blocked (every `SendInput` counts as
/// refused), and finds "no text box" for real.
#[cfg(debug_assertions)]
static TEST_LEFT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Test mode's stand-in failure, if any ([`e2e_paste_fails`]).
fn test_left() -> Option<Left> {
    #[cfg(debug_assertions)]
    {
        if !crate::shell::e2e::active() {
            return None;
        }
        match TEST_LEFT.load(std::sync::atomic::Ordering::SeqCst) {
            1 => Some(Left::KeysBlocked),
            2 => Some(Left::NoTextBox),
            _ => None,
        }
    }
    #[cfg(not(debug_assertions))]
    None
}

/// Robot mode: every `SendInput` counts as refused (`e2e_paste_fails
/// "keysBlocked"`).
#[cfg(debug_assertions)]
pub(crate) fn test_keys_blocked() -> bool {
    crate::shell::realtest::on() && test_left() == Some(Left::KeysBlocked)
}

/// Test mode only: pastes fail with `why` ("keysBlocked": Windows didn't
/// take the keys; "noTextBox": nothing that takes text had the focus), or
/// don't (`null`). Plain test mode pastes nothing either way and says what
/// that failure says; robot mode refuses its own keys for "keysBlocked",
/// leaving the text on the clipboard for real.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_paste_fails(why: Option<String>) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    let value = match why.as_deref() {
        None => 0,
        Some("keysBlocked") => 1,
        Some("noTextBox") => 2,
        Some(other) => return Err(format!("Unknown failure {other}")),
    };
    info!(why = ?why, "e2e: pastes fail");
    TEST_LEFT.store(value, std::sync::atomic::Ordering::SeqCst);
    Ok(())
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
        platform::press_submit(&key).map_err(|e| format!("Windows didn't take the {key} key: {e}"))?;
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

/// How long edit mode's Ctrl+C waits for a clipboard another app holds,
/// before it takes the clipboard and before it puts it back.
const BORROW_PATIENCE_MS: u64 = 1_000;
const RESTORE_PATIENCE_MS: u64 = 2_000;
/// How long Ctrl+C has to show on the clipboard (an app that copies
/// nothing — no selection — never changes it).
const COPY_WAIT_MS: u64 = 400;

static APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

/// At start (`lib.rs`): edit mode's Ctrl+C can say when it left the
/// clipboard alone.
pub fn init(app: &tauri::AppHandle) {
    let _ = APP.set(app.clone());
}

/// Why edit mode's Ctrl+C trick didn't read the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trouble {
    /// Another app held the clipboard the whole wait: nothing touched.
    Busy,
    /// It holds something Yap couldn't put back exactly: nothing touched.
    CantKeep(&'static str),
    /// Windows didn't take Ctrl+C: nothing copied, nothing touched.
    KeysBlocked,
    /// The selection was read, but the clipboard couldn't be put back in
    /// time: it holds the selection now.
    NotRestored,
}

impl Trouble {
    /// What the person is told (one notice).
    pub fn alert(self) -> crate::notices::alert::Alert {
        let (title, body) = match self {
            Trouble::Busy => (
                "Couldn't read your selection".to_string(),
                "Another app was using the clipboard, so Yap left it alone. Try again in a moment.".to_string(),
            ),
            Trouble::CantKeep(what) => (
                "Couldn't read your selection".to_string(),
                format!("Your clipboard holds {what}, so Yap left it alone rather than lose it."),
            ),
            Trouble::KeysBlocked => (
                "Couldn't read your selection".to_string(),
                "Windows didn't let Yap press Ctrl+C just then. Your clipboard is as it was.".to_string(),
            ),
            Trouble::NotRestored => (
                "Your clipboard couldn't be put back".to_string(),
                "Another app held on to the clipboard, so it still has the text you selected instead of what you'd copied.".to_string(),
            ),
        };
        crate::notices::alert::Alert {
            kind: "clipboardBusy",
            icon: "clipboard",
            title,
            body,
            action: None,
            timeout_ms: 10_000,
        }
    }
}

/// What edit mode's Ctrl+C trick found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copied {
    /// The selection, if one was copied.
    pub text: Option<String>,
    /// What went wrong, if anything (the clipboard left alone, or not put back).
    pub trouble: Option<Trouble>,
}

/// Capture the current text selection via the **clipboard Ctrl+C trick** — the
/// fallback for edit/rewrite mode when UI Automation can't read the selection
/// (common in Electron/Chromium/terminal apps). `None` if nothing was
/// selected or the clipboard couldn't be borrowed; then, or when it
/// couldn't be put back, the person is told once (`notices/alert.rs`).
pub fn selection_via_copy(target_hwnd: Option<isize>) -> Option<String> {
    say_trouble(copy_selection(target_hwnd, false)).text
}

/// The person told what went wrong, once, if anything did.
pub(crate) fn say_trouble(copied: Copied) -> Copied {
    if let Some(trouble) = copied.trouble {
        warn!(?trouble, "Edit mode's Ctrl+C: the clipboard");
        if let Some(app) = APP.get() {
            crate::notices::alert::show_soon(app, trouble.alert());
        }
    }
    copied
}

/// The Ctrl+C trick, borrowing the clipboard without ever losing it:
///
/// 1. Everything on it is kept first, every format byte for byte
///    (`clip::snapshot_exact`), waiting up to a second for an app that
///    holds it. Busy all that time, or holding something Yap can't put
///    back exactly → nothing is pressed, nothing touched.
/// 2. Ctrl+C, then its result is told by the clipboard's sequence number
///    (the clipboard is never emptied first): unchanged → nothing was
///    selected, and nothing needs putting back.
/// 3. The text read, everything is put back (privately: out of Win+V
///    history, as a paste's restore), waiting up to two seconds for the
///    clipboard; still busy → it holds the selection, and that's said.
///
/// Test mode never; robot mode only when its spec asks (`robot`:
/// `realtest_copy_selection`) and into a window the test opened.
pub fn copy_selection(target_hwnd: Option<isize>, robot: bool) -> Copied {
    let nothing = Copied { text: None, trouble: None };
    let robot_may = robot && crate::shell::realtest::on() && crate::shell::realtest::permits(target_hwnd);
    if crate::shell::e2e::active() && !robot_may {
        info!("e2e: not sending Ctrl+C to another app");
        return nothing;
    }
    #[cfg(target_os = "windows")]
    {
        use std::time::Duration;
        // Make sure the target window has focus so Ctrl+C hits the right field.
        if let Some(hwnd) = target_hwnd {
            platform::focus_window(hwnd);
            std::thread::sleep(Duration::from_millis(20));
        }
        let kept = match clip::snapshot_exact(Duration::from_millis(BORROW_PATIENCE_MS)) {
            clip::Exact::Kept(items) => items,
            clip::Exact::Busy => return Copied { text: None, trouble: Some(Trouble::Busy) },
            clip::Exact::CantKeep(what) => return Copied { text: None, trouble: Some(Trouble::CantKeep(what)) },
        };
        // Robot mode: the window in front is still the test's.
        if let Err(why) = crate::shell::realtest::recheck_foreground(target_hwnd, "press Ctrl+C") {
            info!("{why}");
            return nothing;
        }
        let before = clip::sequence();
        if platform::simulate_copy().is_err() {
            return Copied { text: None, trouble: Some(Trouble::KeysBlocked) };
        }
        crate::shell::realtest::check_after(target_hwnd, "Ctrl+C");
        // Poll for the copy to land rather than a fixed sleep: as soon as
        // the clipboard changes (fast common case), up to COPY_WAIT_MS for
        // a slow Ctrl+C in RDP/Electron/busy apps.
        let started = std::time::Instant::now();
        let changed = loop {
            std::thread::sleep(Duration::from_millis(20));
            if clip::sequence() != before {
                break true;
            }
            if started.elapsed() >= Duration::from_millis(COPY_WAIT_MS) {
                break false;
            }
        };
        if !changed {
            debug!("Ctrl+C copied nothing (no selection); the clipboard untouched");
            return nothing;
        }
        // The app (or a clipboard manager reading it) may still hold it:
        // read with up to a second's patience.
        let mut text = None;
        let reading = std::time::Instant::now();
        while reading.elapsed() < Duration::from_millis(BORROW_PATIENCE_MS) {
            if let Some(t) = clip::read_text() {
                text = Some(t);
                break;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        // Everything back as it was, kept out of history (it's in there
        // already) — unless the app syncs the clipboard to another machine.
        let private = !syncs_clipboard(app_name_for(target_hwnd).as_deref());
        let restored = clip::restore_exact(&kept, private, Duration::from_millis(RESTORE_PATIENCE_MS));
        debug!(restored, formats = kept.len(), "Edit mode's Ctrl+C: the clipboard put back");
        Copied {
            text: text.filter(|t| !t.trim().is_empty()),
            trouble: (!restored).then_some(Trouble::NotRestored),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = target_hwnd;
        nothing
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

pub(crate) const NOTHING_TO_PASTE: &str = "Nothing to paste yet — dictate something first";

/// Put `text` on the clipboard (privately, as a paste would: out of Win+V
/// history) and leave it there for the person to paste (`why`: the app
/// runs as administrator…). Nothing else is touched.
#[cfg(target_os = "windows")]
fn leave_on_clipboard(text: &str, receiver: Option<isize>, why: Left) -> Result<Delivery, String> {
    put_aside(text, receiver, why.code()).map(|()| Delivery::LeftOnClipboard(why))
}

/// [`leave_on_clipboard`] for an Undo whose window is gone.
#[cfg(target_os = "windows")]
fn window_gone(text: &str) -> Result<Delivery, String> {
    put_aside(text, None, "windowGone").map(|()| Delivery::WindowGone)
}

#[cfg(target_os = "windows")]
fn put_aside(text: &str, receiver: Option<isize>, why: &str) -> Result<(), String> {
    let private = !syncs_clipboard(app_name_for(receiver).as_deref());
    if clip::put_text_verified(text, private) {
        warn!(len = text.len(), why, "Not pasted: the text is left on the clipboard");
        Ok(())
    } else {
        Err("The text couldn't be pasted, nor put on the clipboard".into())
    }
}

/// "Paste last transcript": the latest dictation into the app window the
/// person is in (the tray menu borrowed the foreground: the top app window),
/// fitted to its cursor like a dictation. For when a paste went astray.
/// Returns what was pasted. A test run pastes nothing and returns it. An app
/// running as administrator gets it on the clipboard instead, and the person
/// is told (`elevated.rs`) — as when Windows doesn't take the keys or no
/// text box has the focus. (Also its shortcut's, `last.rs`.)
pub(crate) fn paste_last_blocking(app: &tauri::AppHandle, as_said: bool) -> Result<String, String> {
    let text = if as_said { last_said_transcript() } else { last_transcript() }.ok_or(NOTHING_TO_PASTE)?;
    let cfg = crate::config::load();
    let test_mode = || {
        let out = fit_for_paste(&text, None, cfg.append_trailing_space, &cfg.dictionary, AppKind::Other);
        if crate::pasting::elevated::test_override() {
            info!(len = out.len(), "e2e: the app counts as an admin app; not pasting the last transcript");
            crate::notices::alert::show(app, crate::pasting::elevated::alert());
        } else if let Some(left) = test_left() {
            info!(len = out.len(), why = left.code(), "e2e: the paste fails; not pasting the last transcript");
            crate::notices::alert::show(app, left.alert());
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
            leave_on_clipboard(&out, target, Left::Elevated)?;
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
        if let Delivery::LeftOnClipboard(left) = inject_text_sync(&out, cfg.restore_clipboard, target, false, raw)? {
            crate::notices::alert::show(app, left.alert());
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
/// blocking thread to avoid blocking the tokio runtime. When it can't be
/// pasted — an app running as administrator, keys Windows didn't take,
/// nothing that takes text with the focus — the text is left on the
/// clipboard ([`Delivery::LeftOnClipboard`]) and the caller tells the
/// person, once. `strict` (an Undo, seconds after the dictation): the paste
/// goes back to `target_hwnd` or nowhere — a window that's gone or won't
/// come to the front leaves the text on the clipboard
/// ([`Delivery::WindowGone`]) rather than pasting into whatever has the
/// focus now. `kind`: the kind of app the window is, as read (before the
/// settings): which keys paste there (`AppKind::paste_key`; Shift+Insert in
/// Git Bash…), whether a long paste goes in slices (a coding CLI that folds
/// long pastes, `slices.rs`), and — anything but a terminal — whether to
/// check a Win32 edit control got the paste, and that something that takes
/// text has the focus.
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
            return tokio::task::spawn_blocking(move || window_gone(&text))
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
                return Ok(Delivery::LeftOnClipboard(Left::Elevated));
            }
            if let Some(left) = test_left() {
                info!(len = text.len(), why = left.code(), "e2e: the paste fails; not pasting the text");
                return Ok(Delivery::LeftOnClipboard(left));
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
/// When nothing that takes text had the focus before the keys, how long
/// after them it's looked at again (an app that moves the focus to its
/// message box on a paste — Discord, Slack — has done it by then).
const FOCUS_AGAIN_MS: u64 = 250;
/// The pause before the paste keys go again when Windows didn't take them.
const KEYS_AGAIN_MS: u64 = 150;

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

/// The paste keys into `target`, checked: Windows didn't take them → once
/// more after a moment (robot mode: still the test's window in front).
#[cfg(target_os = "windows")]
fn press_paste(target: Option<isize>, key: app_kind::PasteKey) -> Result<(), String> {
    match platform::simulate_paste(target, key) {
        Ok(()) => Ok(()),
        Err(first) => {
            warn!("The paste keys didn't go ({first}); once more");
            std::thread::sleep(std::time::Duration::from_millis(KEYS_AGAIN_MS));
            crate::shell::realtest::recheck_foreground(target, "paste")?;
            platform::simulate_paste(target, key)
        }
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
        use crate::pasting::selection::Editable;

        // An app running as administrator takes no keys from Yap (Windows
        // drops them without a word): the text stays on the clipboard for
        // the person to paste (`elevated.rs`).
        let receiver = target_hwnd.or_else(current_foreground);
        if receiver.is_some_and(crate::pasting::elevated::target_blocked) {
            warn!("The app runs as administrator: Windows won't let Yap type into it");
            return leave_on_clipboard(text, receiver, Left::Elevated);
        }

        // Re-focus the window the user was dictating into (best-effort; an
        // Undo's paste goes there or nowhere).
        if let Some(hwnd) = target_hwnd {
            let back = platform::focus_window(hwnd) && platform::is_foreground(hwnd);
            if !back && strict {
                warn!("Couldn't get back to the dictation's window; leaving the text on the clipboard");
                return window_gone(text);
            }
            if !back {
                warn!("Could not re-focus the dictation target window; pasting into current focus");
            }
        }

        // A long paste into a coding CLI that folds long pastes into a
        // placeholder goes in slices (`slices.rs`); everything else in one.
        let slices: Vec<&str> = match crate::pasting::slices::limits(kind) {
            Some(limits) => crate::pasting::slices::cut(text, limits),
            None => vec![text],
        };
        if slices.len() > 1 {
            info!(slices = slices.len(), "A long paste into {}: in slices", kind.describe());
        }

        // Kept out of clipboard history and sync, unless the target is a
        // remote desktop whose sync is how the paste gets there.
        let private = !syncs_clipboard(app_name_for(target_hwnd).as_deref());
        // Snapshot the current clipboard (text, an image or files), only if
        // we'll restore it.
        let previous = restore_clipboard.then(clip::snapshot);

        if !clip::put_text_verified(slices[0], private) {
            // The clipboard can't be written: type the text instead.
            warn!("Couldn't put the text on the clipboard; typing it instead");
            // Robot mode: still the test's window in front (always fine in a
            // normal run).
            let checked = crate::shell::realtest::recheck_foreground(target_hwnd, "type the text");
            let typed = checked.and_then(|()| platform::type_unicode(text));
            if typed.is_ok() {
                crate::shell::realtest::check_after(target_hwnd, "typing");
            }
            if let Some(prev) = previous {
                clip::restore(prev, private);
            }
            return typed.map(|()| Delivery::Pasted);
        }
        trace!(private, "Dictation on the clipboard");

        // Small delay to ensure clipboard is ready.
        std::thread::sleep(std::time::Duration::from_millis(30));

        // A Win32 edit control (a WinForms text box, a classic EDIT or
        // RichEdit), as it stands before the keys: to check the paste lands
        // there (never a terminal's). And whether anything that takes text
        // has the focus at all (UI Automation; never asked of a terminal).
        let check = if kind.is_terminal() {
            None
        } else {
            crate::pasting::selection::PasteCheck::before(target_hwnd)
        };
        let focus_before = if kind.is_terminal() || check.is_some() {
            Editable::Unknown
        } else {
            crate::pasting::selection::focus_editable(target_hwnd)
        };
        if focus_before == Editable::No {
            info!("Nothing that takes text has the focus; pasting anyway, then looking again");
        }

        // Robot mode: the window in front must still be the test's, and the
        // one this paste is for — checked right before the keys go out.
        if let Err(why) = crate::shell::realtest::recheck_foreground(target_hwnd, "paste") {
            if let Some(prev) = previous {
                clip::restore(prev, private);
            }
            return Err(why);
        }

        // Ctrl+V, or the terminal's own paste keys. Windows didn't take
        // them (twice): the text stays on the clipboard, and that's said.
        if let Err(e) = press_paste(target_hwnd, key) {
            warn!("Windows didn't take the paste keys: {e}; the text is left on the clipboard");
            // A sliced paste: the whole text on it, not just its first slice.
            if slices.len() > 1 && !clip::put_text_verified(text, private) {
                return Err("The text couldn't be pasted, nor put on the clipboard".into());
            }
            return Ok(Delivery::LeftOnClipboard(Left::KeysBlocked));
        }
        let mut pasted_at = std::time::Instant::now();

        // The rest of a sliced paste, a moment apart: the terminal has read
        // the clipboard before the next slice takes its place.
        for (i, slice) in slices.iter().enumerate().skip(1) {
            std::thread::sleep(std::time::Duration::from_millis(crate::pasting::slices::PACE_MS));
            let rest: String = slices[i..].concat();
            if !clip::put_text_verified(slice, private) {
                warn!(slice = i, "Couldn't put a slice on the clipboard; the rest is left there");
                return leave_on_clipboard(&rest, target_hwnd, Left::KeysBlocked);
            }
            let next = crate::shell::realtest::recheck_foreground(target_hwnd, "paste").and_then(|()| press_paste(target_hwnd, key));
            if let Err(e) = next {
                warn!(slice = i, "The paste stopped after {i} slices ({e}); the rest is left on the clipboard");
                return leave_on_clipboard(&rest, target_hwnd, Left::KeysBlocked);
            }
            pasted_at = std::time::Instant::now();
        }

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
                            if let Err(e) = platform::type_unicode(text) {
                                // The dictation is still on the clipboard.
                                warn!("Windows didn't take the typing either: {e}; the text is left on the clipboard");
                                return Ok(Delivery::LeftOnClipboard(Left::KeysBlocked));
                            }
                            crate::shell::realtest::check_after(target_hwnd, "typing");
                            delivery = Delivery::Typed;
                        }
                        Err(why) => warn!("The paste didn't reach the field, and it can't be typed there: {why}"),
                    }
                }
            }
        }

        // Nothing took text before the keys: look again once the app has
        // had its moment (some move the focus to their message box on a
        // paste). Still nothing → the paste landed nowhere: the text stays
        // on the clipboard (the person's own isn't put back), and that's said.
        if focus_before == Editable::No {
            let wait = std::time::Duration::from_millis(FOCUS_AGAIN_MS).saturating_sub(pasted_at.elapsed());
            std::thread::sleep(wait);
            if crate::pasting::selection::focus_editable(target_hwnd) == Editable::No {
                warn!("Nothing that takes text had the focus before or after the paste; the text is left on the clipboard");
                if slices.len() > 1 && !clip::put_text_verified(text, private) {
                    return Err("The text couldn't be pasted, nor put on the clipboard".into());
                }
                return Ok(Delivery::LeftOnClipboard(Left::NoTextBox));
            }
            debug!("Something that takes text has the focus after the paste");
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

        // Edit mode's Ctrl+C borrows it: every format comes back byte for
        // byte (Windows' own text formats made from the text too).
        clip::put_raw(&clip::rich_items());
        let fingerprint = clip::fingerprint().expect("kept");
        assert!(fingerprint.len() >= 3, "{fingerprint:?}");
        let clip::Exact::Kept(items) = clip::snapshot_exact(std::time::Duration::from_millis(500)) else {
            panic!("the clipboard couldn't be kept");
        };
        assert!(clip::put_text_verified("The selected words", false));
        assert!(clip::restore_exact(&items, false, std::time::Duration::from_millis(500)));
        assert_eq!(clip::fingerprint().expect("kept again"), fingerprint);
        assert_eq!(clip::read_text().as_deref(), Some("Quarterly plan"));
        // An empty one is empty again.
        clip::clear();
        let clip::Exact::Kept(none) = clip::snapshot_exact(std::time::Duration::from_millis(500)) else { panic!() };
        assert!(none.is_empty());
        assert!(clip::put_text_verified("The selected words", false));
        assert!(clip::restore_exact(&none, false, std::time::Duration::from_millis(500)));
        assert_eq!(clip::read_raw(clip::HDROP).1, 0);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn formats_windows_makes_again_are_left_out_and_handles_are_never_copied() {
        // A bitmap and a palette come back from the DIB, an old metafile
        // picture from the enhanced metafile.
        assert!(clip::remade(2, &[2, 8]));
        assert!(clip::remade(9, &[9, 17]));
        assert!(clip::remade(3, &[3, 14]));
        assert!(!clip::remade(2, &[2]));
        assert!(!clip::remade(13, &[13, 1, 7]));
        // Owner-drawn, GDI and private handles: never copied as bytes.
        for format in [0x80, 0x82, 0x83, 0x8E, 0x200, 0x2FF, 0x300, 0x3FF, 2, 3, 9] {
            assert!(clip::uncopyable(format), "{format:#x}");
        }
        for format in [1, 7, 8, 13, 15, 16, 17, 0x81, 0xC0FF] {
            assert!(!clip::uncopyable(format), "{format:#x}");
        }
    }
}

//! Text injection via clipboard + simulated Ctrl+V paste.
//!
//! Used by the dictation feature to inject transcribed text into
//! whatever application/field is currently focused.

use tracing::{info, trace, warn};

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

/// Simulate the auto-submit keystroke in the focused window. `key` is one of
/// "enter", "ctrlEnter", or "shiftEnter".
pub async fn press_submit(key: &str) -> Result<(), String> {
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

/// A best-effort snapshot of the clipboard so we can restore it after borrowing
/// it for a paste / Ctrl+C. Covers the two common payloads (text and image) —
/// restoring text *only* (the previous behaviour) silently destroyed any image
/// or files the user had copied.
#[cfg(target_os = "windows")]
enum ClipSnapshot {
    Text(String),
    Image(arboard::ImageData<'static>),
    Empty,
}

/// Cap for image snapshot/restore: a 4K BGRA screenshot is ~33 MB; anything
/// much larger isn't worth the memory or the risk in native conversion code.
#[cfg(target_os = "windows")]
const MAX_SNAPSHOT_IMAGE_BYTES: usize = 64 * 1024 * 1024;

#[cfg(target_os = "windows")]
fn snapshot_clipboard(cb: &mut arboard::Clipboard) -> ClipSnapshot {
    if let Ok(t) = cb.get_text() {
        return ClipSnapshot::Text(t);
    }
    // Image handling goes through native DIB conversion (arboard + `image`),
    // which is the riskiest code on this path — guard against Rust panics and
    // log entry/exit so a native crash here is identifiable in the log.
    trace!("clipboard snapshot: no text, trying image");
    let img = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| cb.get_image()));
    match img {
        Ok(Ok(img)) if img.bytes.len() <= MAX_SNAPSHOT_IMAGE_BYTES => {
            trace!(w = img.width, h = img.height, "clipboard snapshot: image captured");
            ClipSnapshot::Image(img)
        }
        Ok(Ok(img)) => {
            warn!(
                bytes = img.bytes.len(),
                "clipboard image too large to snapshot — it won't be restored after paste"
            );
            ClipSnapshot::Empty
        }
        Ok(Err(_)) => ClipSnapshot::Empty,
        Err(_) => {
            warn!("clipboard image snapshot panicked — skipping image restore");
            ClipSnapshot::Empty
        }
    }
}

#[cfg(target_os = "windows")]
fn restore_clipboard_snapshot(cb: &mut arboard::Clipboard, snap: ClipSnapshot) {
    match snap {
        ClipSnapshot::Text(t) => {
            let _ = cb.set_text(t);
        }
        ClipSnapshot::Image(img) => {
            trace!(w = img.width, h = img.height, "restoring image clipboard");
            let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let _ = cb.set_image(img);
            }));
            if res.is_err() {
                warn!("clipboard image restore panicked — original image lost");
            }
        }
        // Nothing (or an unsupported format) was there — leave it as-is.
        ClipSnapshot::Empty => {}
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
    #[cfg(target_os = "windows")]
    {
        use arboard::Clipboard;

        // Make sure the target window has focus so Ctrl+C hits the right field.
        if let Some(hwnd) = target_hwnd {
            platform::focus_window(hwnd);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        let mut clipboard = Clipboard::new().ok()?;
        // Snapshot text *or* image so restoring doesn't wipe a copied image/files.
        let previous = snapshot_clipboard(&mut clipboard);
        // Clear first: if Ctrl+C copies nothing (no selection), the clipboard
        // stays empty and we correctly return None instead of the old contents.
        let _ = clipboard.set_text(String::new());
        std::thread::sleep(std::time::Duration::from_millis(30));

        platform::simulate_copy();
        // Poll for the copy to land (up to ~200 ms) rather than a fixed sleep —
        // returns as soon as text appears (fast common case) but tolerates a slow
        // Ctrl+C in RDP/Electron/busy apps.
        let mut copied = None;
        for _ in 0..10 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            if let Ok(t) = clipboard.get_text() {
                if !t.trim().is_empty() {
                    copied = Some(t);
                    break;
                }
            }
        }

        // Restore the user's clipboard (best-effort; text or image).
        restore_clipboard_snapshot(&mut clipboard, previous);

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
    use arboard::Clipboard;

    info!(len = text.len(), "Injecting text via clipboard paste");

    // Small delay to let focus settle after voice recording stops.
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Re-focus the window the user was dictating into (best-effort).
    #[cfg(target_os = "windows")]
    if let Some(hwnd) = target_hwnd {
        if !platform::focus_window(hwnd) {
            warn!("Could not re-focus the dictation target window; pasting into current focus");
        }
    }
    #[cfg(not(target_os = "windows"))]
    let _ = target_hwnd;

    #[cfg(not(target_os = "windows"))]
    {
        warn!("Text injection is only supported on Windows");
        return Err("Text injection is only supported on Windows".into());
    }

    // Clipboard path. If the clipboard can't be opened or set, fall back to
    // typing the text directly as Unicode keystrokes (no clipboard needed).
    let mut clipboard = match Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            warn!("Clipboard unavailable ({e}); falling back to direct typing");
            #[cfg(target_os = "windows")]
            platform::type_unicode(text);
            return Ok(());
        }
    };

    // Snapshot the current clipboard (text or image), only if we'll restore it.
    let previous = if restore_clipboard {
        Some(snapshot_clipboard(&mut clipboard))
    } else {
        None
    };

    if let Err(e) = clipboard.set_text(text) {
        warn!("Failed to set clipboard ({e}); falling back to direct typing");
        #[cfg(target_os = "windows")]
        platform::type_unicode(text);
        return Ok(());
    }

    // Small delay to ensure clipboard is ready.
    std::thread::sleep(std::time::Duration::from_millis(30));

    // Simulate Ctrl+V.
    #[cfg(target_os = "windows")]
    platform::simulate_paste();

    // Restore previous clipboard (delayed to ensure paste completes). Webview/
    // Electron apps process Ctrl+V asynchronously (browser→renderer IPC) and can
    // take well over 200 ms to actually read the clipboard — restoring too early
    // makes the paste land empty or with the OLD contents. 500 ms is still
    // imperceptible (it runs after the text has visually appeared in fast apps).
    if let Some(prev) = previous {
        std::thread::sleep(std::time::Duration::from_millis(500));
        restore_clipboard_snapshot(&mut clipboard, prev);
    }

    Ok(())
}

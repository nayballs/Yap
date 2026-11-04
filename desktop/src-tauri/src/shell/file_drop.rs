//! **Files dragged onto Yap's windows** (Windows, 2026-10-31): Yap's own drop
//! target on every window of a webview, sending the same `tauri://drag-enter`
//! / `-over` / `-drop` / `-leave` events wry's would, so the pages'
//! `getCurrentWebview().onDragDropEvent` (Upload, Chat, Import a transcript)
//! are unchanged.
//!
//! wry turns WebView2's own drops off (`AllowExternalDrop(false)`) and puts its
//! drop target on the webview's child windows **once, as the webview is
//! created**. WebView2 makes its input windows (`Chrome_WidgetWin_1`,
//! `Chrome_RenderWidgetHostHWND`, in its browser process) a moment later, and
//! those are the windows under the pointer: Windows looked there, found no
//! drop target, and every file dragged in from Explorer got the 🚫 cursor —
//! in every build, for everyone. A test run happened to win that race
//! (Playwright attaching slows WebView2's start), and `e2e_drag_drop` stands
//! in for the drop itself, so no test ever saw it. So Yap registers on all of
//! them, once they exist ([`watch`] after a window opens, [`attach`] whenever
//! it's focused), and again on any WebView2 makes anew.

use tauri::WebviewWindow;

/// The windows with something to drop files on: Upload, Chat and Import a
/// transcript all live in the main window.
pub const WINDOWS: &[&str] = &["settings"];

/// Yap's drop target on every window of this webview that hasn't got it yet.
/// Any thread: it runs on the main thread (OLE wants the registering thread
/// to be the one with the windows' messages).
pub fn attach(window: &WebviewWindow) {
    #[cfg(windows)]
    {
        let w = window.clone();
        let _ = window.run_on_main_thread(move || win::attach(&w));
    }
    #[cfg(not(windows))]
    let _ = window;
}

/// [`attach`] now and again over the next few seconds, while WebView2 is
/// still making its input windows (a window just created, or just shown).
pub fn watch(window: &WebviewWindow) {
    attach(window);
    let w = window.clone();
    let _ = std::thread::Builder::new().name("file-drop".into()).spawn(move || {
        for wait in [1_000, 3_000, 6_000] {
            std::thread::sleep(std::time::Duration::from_millis(wait));
            attach(&w);
        }
    });
}

#[cfg(windows)]
mod win {
    use std::cell::{Cell, RefCell};
    use std::collections::HashSet;
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    use tauri::{Emitter, EventTarget, WebviewWindow};
    use windows::core::{implement, Ref, Result, BOOL};
    use windows::Win32::Foundation::{HWND, LPARAM, POINT, POINTL};
    use windows::Win32::Graphics::Gdi::ScreenToClient;
    use windows::Win32::System::Com::{IDataObject, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
    use windows::Win32::System::Ole::{
        IDropTarget, IDropTarget_Impl, RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop, CF_HDROP, DROPEFFECT,
        DROPEFFECT_COPY, DROPEFFECT_NONE,
    };
    use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
    use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumChildWindows, GetClassNameW, GetPropW, GetWindowThreadProcessId, IsWindow,
    };

    thread_local! {
        /// The windows that have Yap's drop target (main thread only).
        static OURS: RefCell<HashSet<isize>> = RefCell::new(HashSet::new());
    }

    #[implement(IDropTarget)]
    struct Target {
        /// The window it's registered on (positions are relative to it).
        hwnd: HWND,
        window: WebviewWindow,
        /// The drag over us now carries files (else every answer is "no").
        files: Cell<bool>,
    }

    impl Target {
        fn at(&self, pt: &POINTL) -> serde_json::Value {
            let mut p = POINT { x: pt.x, y: pt.y };
            let _ = unsafe { ScreenToClient(self.hwnd, &mut p) };
            serde_json::json!({ "x": p.x, "y": p.y })
        }

        fn emit(&self, event: &str, payload: serde_json::Value) {
            let target = EventTarget::AnyLabel { label: self.window.label().to_string() };
            if let Err(e) = self.window.emit_to(target, event, payload) {
                tracing::warn!("file drop: couldn't send {event}: {e}");
            }
        }
    }

    /// The files a drag carries (`CF_HDROP`); none for text, a link, an image.
    fn paths(data: Ref<'_, IDataObject>) -> Vec<String> {
        let Ok(data) = data.ok() else { return Vec::new() };
        let format = FORMATETC {
            cfFormat: CF_HDROP.0,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        };
        let Ok(mut medium) = (unsafe { data.GetData(&format) }) else { return Vec::new() };
        let drop = HDROP(unsafe { medium.u.hGlobal.0 });
        let count = unsafe { DragQueryFileW(drop, u32::MAX, None) };
        let mut out = Vec::with_capacity(count as usize);
        for i in 0..count {
            let len = unsafe { DragQueryFileW(drop, i, None) } as usize;
            let mut buf = vec![0u16; len + 1];
            unsafe { DragQueryFileW(drop, i, Some(&mut buf)) };
            out.push(OsString::from_wide(&buf[..len]).to_string_lossy().into_owned());
        }
        unsafe { ReleaseStgMedium(&mut medium) };
        out
    }

    impl IDropTarget_Impl for Target_Impl {
        fn DragEnter(
            &self,
            data: Ref<'_, IDataObject>,
            _keys: MODIFIERKEYS_FLAGS,
            pt: &POINTL,
            effect: *mut DROPEFFECT,
        ) -> Result<()> {
            let paths = paths(data);
            tracing::debug!(files = paths.len(), "file drop: a drag came in");
            self.files.set(!paths.is_empty());
            unsafe { *effect = if paths.is_empty() { DROPEFFECT_NONE } else { DROPEFFECT_COPY } };
            if !paths.is_empty() {
                self.emit("tauri://drag-enter", serde_json::json!({ "paths": paths, "position": self.at(pt) }));
            }
            Ok(())
        }

        fn DragOver(&self, _keys: MODIFIERKEYS_FLAGS, pt: &POINTL, effect: *mut DROPEFFECT) -> Result<()> {
            let files = self.files.get();
            unsafe { *effect = if files { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
            if files {
                self.emit("tauri://drag-over", serde_json::json!({ "position": self.at(pt) }));
            }
            Ok(())
        }

        fn DragLeave(&self) -> Result<()> {
            if self.files.replace(false) {
                self.emit("tauri://drag-leave", serde_json::Value::Null);
            }
            Ok(())
        }

        fn Drop(
            &self,
            data: Ref<'_, IDataObject>,
            _keys: MODIFIERKEYS_FLAGS,
            pt: &POINTL,
            effect: *mut DROPEFFECT,
        ) -> Result<()> {
            self.files.set(false);
            let paths = paths(data);
            tracing::debug!(files = paths.len(), "file drop: dropped");
            unsafe { *effect = if paths.is_empty() { DROPEFFECT_NONE } else { DROPEFFECT_COPY } };
            if !paths.is_empty() {
                self.emit("tauri://drag-drop", serde_json::json!({ "paths": paths, "position": self.at(pt) }));
            }
            Ok(())
        }
    }

    fn class(hwnd: HWND) -> String {
        let mut buf = [0u16; 64];
        let n = unsafe { GetClassNameW(hwnd, &mut buf) } as usize;
        String::from_utf16_lossy(&buf[..n])
    }

    /// Every window of the webview: its host in Yap's process and WebView2's
    /// input windows in its browser process (`in_yap` false), the ones under
    /// the pointer.
    fn webview_windows(top: HWND) -> Vec<(HWND, bool)> {
        let me = std::process::id();
        let mut found = Vec::new();
        let mut each = |h: HWND| {
            let cls = class(h);
            if cls.starts_with("Chrome_WidgetWin") || cls == "Chrome_RenderWidgetHostHWND" {
                let mut pid = 0u32;
                unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
                found.push((h, pid == me));
            }
        };
        unsafe extern "system" fn each_child(h: HWND, l: LPARAM) -> BOOL {
            let f = unsafe { &mut *(l.0 as *mut &mut dyn FnMut(HWND)) };
            f(h);
            true.into()
        }
        let mut f: &mut dyn FnMut(HWND) = &mut each;
        unsafe {
            let _ = EnumChildWindows(Some(top), Some(each_child), LPARAM(&mut f as *mut _ as isize));
        }
        found
    }

    pub(super) fn attach(window: &WebviewWindow) {
        let Ok(top) = window.hwnd() else { return };
        let windows = webview_windows(HWND(top.0));
        let (mut added, mut in_webview2) = (0, 0);
        OURS.with(|ours| {
            let mut ours = ours.borrow_mut();
            // Windows WebView2 has since destroyed.
            ours.retain(|h| unsafe { IsWindow(Some(HWND(*h as *mut _))) }.as_bool());
            for (hwnd, in_yap) in windows {
                let registered = !unsafe { GetPropW(hwnd, windows::core::w!("OleDropTargetInterface")) }.is_invalid();
                if registered && ours.contains(&(hwnd.0 as isize)) {
                    continue;
                }
                let target: IDropTarget = Target { hwnd, window: window.clone(), files: Cell::new(false) }.into();
                // wry's (on the host), or nothing yet (WebView2's): replaced.
                let _ = unsafe { RevokeDragDrop(hwnd) };
                match unsafe { RegisterDragDrop(hwnd, &target) } {
                    Ok(()) => {
                        ours.insert(hwnd.0 as isize);
                        added += 1;
                        if !in_yap {
                            in_webview2 += 1;
                        }
                    }
                    Err(e) => tracing::warn!(window = window.label(), "file drop: couldn't take drops on a webview window ({e})"),
                }
            }
        });
        if added > 0 {
            tracing::info!(window = window.label(), added, in_webview2, "file drop: Yap's drop target on the webview");
        }
    }
}

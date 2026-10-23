//! The snip overlay: one native window per monitor showing that monitor's
//! frozen picture, dimmed, with the box you drag at full brightness.
//!
//! **Why native, not a webview** (the spike, `snip-dump`, docs/architecture/
//! snip.md): a webview overlay has to be handed each monitor's frame — a
//! 4K frame is 33 MB raw or ~100 ms of PNG encoding before WebView2 even
//! decodes it — and comes up through WebView2's own compositor. A native
//! window paints the frame with GDI straight from memory: the overlays are
//! up a few milliseconds after the capture. Pattern from PowerToys ZoomIt's
//! `SelectRectangle` (MIT, Copyright (c) Microsoft Corporation): a topmost
//! tool window per monitor, a cross cursor, the mouse captured while you
//! drag, Esc to cancel; and from Kiri (MIT, Copyright (c) 2026 yuxino) the
//! placement in **physical pixels** so mixed-DPI monitors line up. Notices
//! in `THIRD-PARTY-NOTICES.md`.
//!
//! **No flicker**: each window is created once and kept hidden and
//! DWM-cloaked (Yap's rule for hidden windows: never "parked off-screen");
//! a snip places it on its monitor, paints the frozen picture while it's
//! still cloaked, then uncloaks every monitor's together — the picture is
//! on screen whole from its first frame, never a white or half-drawn one.
//! DWM's show animation is off for them. Repaints compose each changed
//! rectangle off screen and copy it in one go.
//!
//! **Input**: drag a box (the mouse captured, so a drag past the monitor's
//! edge keeps going, clamped to it); **release = done** (a box under 6 px
//! does nothing); **Esc**, right-click or the snip shortcut again cancel;
//! another app taking the focus, or the monitors changing, cancel too.
//! Phase 1 keeps a box on the monitor it started on.
//!
//! All of it runs on its own UI thread (`snip-ui`) with a message loop; the
//! rest of Yap talks to it through [`open`] / [`close`].

use std::sync::Arc;

use super::capture::Monitor;
use super::geom::Rect;

/// One monitor's frozen picture for the overlay.
#[derive(Clone)]
pub struct Screen {
    pub monitor: Monitor,
    pub width: u32,
    pub height: u32,
    pub bgra: Arc<Vec<u8>>,
}

/// How a snip ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// A box on screen `screen`, in that window's physical pixels.
    Snipped { screen: usize, rect: Rect },
    /// Esc, right-click, the shortcut again, another app, the monitors.
    Cancelled(&'static str),
}

/// How the overlays come up.
#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// The monitor under the cursor: it takes the focus and shows the hint.
    pub focus: usize,
    /// Take the focus (a snip). Off: shown but click-through and never
    /// focused (the spike's timing run, which must not take the mouse).
    pub interactive: bool,
    /// Never on screen (a test run): set up, painted and driven by posted
    /// messages, but kept cloaked and never focused.
    pub hidden: bool,
}

/// How long the overlays took to come up (the spike and the log).
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shown {
    /// Bitmaps made, the dim computed, windows placed and painted (cloaked).
    pub setup_ms: f64,
    /// …and uncloaked, with one composition pass after it (`DwmFlush`):
    /// when the picture is on screen.
    pub visible_ms: f64,
    /// Whether Windows let the overlay take the focus.
    pub focused: bool,
}

/// The overlay's colours. A native window can't read app.css, so they're
/// the bar's tokens' values (checked against app.css by a test): the
/// tooltip's ink and text for the labels — "tooltips stay ink in both"
/// schemes — and Yap's amber (`--bar-chip`) for the box's edge.
pub mod colours {
    /// `--bar-tooltip`: the labels' background.
    pub const LABEL: (u8, u8, u8) = (0x26, 0x23, 0x1c);
    /// `--bar-tooltip-key`: the labels' text.
    pub const LABEL_TEXT: (u8, u8, u8) = (0xf7, 0xf5, 0xf0);
    /// `--bar-chip`: the box's edge.
    pub const EDGE: (u8, u8, u8) = (0xf6, 0xa9, 0x3c);
}

/// How dark the rest of the screen goes: each channel × 150/256 (≈ 41 %
/// black over it).
pub const DIM: u16 = 150;

/// The hint on the monitor under the cursor.
pub const HINT: &str = "Drag to snip  ·  Esc to cancel";
/// How long the hint stays before it fades.
pub const HINT_MS: u32 = 2_500;

/// The text on a box's size label: physical pixels, "812 × 430".
pub fn size_text(r: &Rect) -> String {
    format!("{} \u{d7} {}", r.w, r.h)
}

/// A frame darkened for the overlay (BGRA, opaque), on every core.
pub fn dimmed(bgra: &[u8]) -> Vec<u8> {
    let mut out = vec![0u8; bgra.len()];
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 8);
    let chunk = bgra.len().div_ceil(threads).div_ceil(4) * 4;
    if chunk == 0 {
        return out;
    }
    std::thread::scope(|scope| {
        for (src, dst) in bgra.chunks(chunk).zip(out.chunks_mut(chunk)) {
            scope.spawn(move || {
                for (s, d) in src.as_chunks::<4>().0.iter().zip(dst.as_chunks_mut::<4>().0.iter_mut()) {
                    d[0] = ((s[0] as u16 * DIM) >> 8) as u8;
                    d[1] = ((s[1] as u16 * DIM) >> 8) as u8;
                    d[2] = ((s[2] as u16 * DIM) >> 8) as u8;
                    d[3] = 255;
                }
            });
        }
    });
    out
}

/// A rounded pill's coverage at pixel (x, y) of a `w`×`h` pill with radius
/// `r` (anti-aliased: 0..=255, sampled at the pixel's centre).
pub fn pill_coverage(x: i32, y: i32, w: i32, h: i32, r: f32) -> u8 {
    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
    let (fw, fh) = (w as f32, h as f32);
    let cx = px.clamp(r, fw - r);
    let cy = py.clamp(r, fh - r);
    let d = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
    let edge = (r - d + 0.5).clamp(0.0, 1.0);
    (edge * 255.0 + 0.5) as u8
}

#[cfg(windows)]
pub use win::{close_because, is_open, open};

#[cfg(all(windows, debug_assertions))]
pub use win::{close, post_input};

#[cfg(not(windows))]
pub fn open(_screens: Vec<Screen>, _opts: Options, _done: Box<dyn FnOnce(Outcome) + Send>) -> Result<Shown, String> {
    Err("The snip overlay is Windows only".into())
}
#[cfg(all(not(windows), debug_assertions))]
pub fn close() {}
#[cfg(not(windows))]
pub fn close_because(_why: &'static str) {}
#[cfg(not(windows))]
pub fn is_open() -> bool {
    false
}

#[cfg(windows)]
mod win {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Mutex, OnceLock};
    use std::time::Instant;

    use windows::core::w;
    use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
    use windows::Win32::Graphics::Dwm::{DwmFlush, DwmSetWindowAttribute, DWMWA_CLOAK, DWMWA_TRANSITIONS_FORCEDISABLED};
    use windows::Win32::Graphics::Gdi::{
        AlphaBlend, BeginPaint, BitBlt, CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateRectRgn, CreateSolidBrush, DeleteDC,
        DeleteObject, EndPaint, FillRect, GetRegionData, GetTextExtentPoint32W, GetUpdateRgn, InvalidateRect, RedrawWindow, SelectObject,
        SetBkMode, SetTextColor, TextOutW, AC_SRC_ALPHA, AC_SRC_OVER, ANTIALIASED_QUALITY, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        BLENDFUNCTION, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DIB_RGB_COLORS, FW_SEMIBOLD, HBITMAP, HDC, HFONT, HGDIOBJ, OUT_DEFAULT_PRECIS,
        PAINTSTRUCT, RDW_INVALIDATE, RDW_UPDATENOW, RGNDATA, RGNDATAHEADER, SRCCOPY, TRANSPARENT,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus, VK_ESCAPE};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetCursorPos, GetMessageW, KillTimer, LoadCursorW, PeekMessageW,
        PostThreadMessageW, RegisterClassExW, SetForegroundWindow, SetLayeredWindowAttributes, SetTimer, SetWindowLongPtrW,
        SetWindowPos, ShowWindow, TranslateMessage, GWL_EXSTYLE, HWND_TOPMOST, IDC_CROSS, LWA_ALPHA, MSG, PM_NOREMOVE, SWP_NOACTIVATE,
        SW_HIDE, SW_SHOWNOACTIVATE, WM_ACTIVATEAPP, WM_APP, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN,
        WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_PAINT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WM_TIMER, WNDCLASSEXW,
        WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, MA_ACTIVATE,
    };

    const WM_JOB: u32 = WM_APP + 0x51;
    const HINT_TIMER: usize = 1;
    const FADE_TIMER: usize = 2;
    /// A snip left up this long is cancelled.
    const GIVE_UP_TIMER: usize = 3;
    const GIVE_UP_MS: u32 = 120_000;
    /// The hint's fade: steps × ms.
    const FADE_STEPS: u8 = 8;
    const FADE_MS: u32 = 24;

    static OPEN: AtomicBool = AtomicBool::new(false);

    pub fn is_open() -> bool {
        OPEN.load(Ordering::SeqCst)
    }

    type Done = Box<dyn FnOnce(Outcome) + Send>;

    enum Job {
        Open(Vec<Screen>, Options, Done, mpsc::Sender<Result<Shown, String>>),
        Close(&'static str),
        #[cfg(debug_assertions)]
        Input(usize, Vec<(u32, usize, isize)>),
    }

    struct Ui {
        thread: u32,
        jobs: Mutex<VecDeque<Job>>,
    }

    fn ui() -> Result<&'static Ui, String> {
        static UI: OnceLock<Result<Ui, String>> = OnceLock::new();
        UI.get_or_init(|| {
            let (tx, rx) = mpsc::channel();
            std::thread::Builder::new()
                .name("snip-ui".into())
                .spawn(move || {
                    // Make the thread's message queue before anyone posts to it.
                    let mut msg = MSG::default();
                    unsafe {
                        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
                    }
                    let _ = tx.send(unsafe { GetCurrentThreadId() });
                    run_loop();
                })
                .map_err(|e| format!("couldn't start the overlay thread ({e})"))?;
            let thread = rx.recv().map_err(|_| "the overlay thread didn't start".to_string())?;
            Ok(Ui { thread, jobs: Mutex::new(VecDeque::new()) })
        })
        .as_ref()
        .map_err(|e| e.clone())
    }

    fn post(job: Job) -> Result<(), String> {
        let ui = ui()?;
        ui.jobs.lock().map_err(|_| "overlay queue poisoned")?.push_back(job);
        unsafe { PostThreadMessageW(ui.thread, WM_JOB, WPARAM(0), LPARAM(0)) }.map_err(|e| format!("couldn't reach the overlay ({e})"))
    }

    /// Bring the overlays up over `screens` (one per monitor) and return
    /// when they're on screen. `done` hears how the snip ended, once, on the
    /// overlay's thread (it must not block).
    pub fn open(screens: Vec<Screen>, opts: Options, done: Done) -> Result<Shown, String> {
        if screens.is_empty() {
            return Err("No monitors to snip".into());
        }
        let (tx, rx) = mpsc::channel();
        post(Job::Open(screens, opts, done, tx))?;
        rx.recv_timeout(std::time::Duration::from_secs(5)).map_err(|_| "the overlay didn't come up".to_string())?
    }

    /// Take the overlays down (the spike's runs).
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub fn close() {
        close_because("closed");
    }

    /// Take the overlays down: a cancel from elsewhere (`why`: the shortcut
    /// again, Esc seen by Yap's keyboard hook).
    pub fn close_because(why: &'static str) {
        if is_open() {
            let _ = post(Job::Close(why));
        }
    }

    /// Debug builds (the e2e suite): window messages posted to screen
    /// `screen`'s overlay — mouse and keys as Windows would send them,
    /// without the real mouse or keyboard.
    #[cfg(debug_assertions)]
    pub fn post_input(screen: usize, msgs: Vec<(u32, usize, isize)>) -> Result<(), String> {
        post(Job::Input(screen, msgs))
    }

    // ---- the UI thread --------------------------------------------------------------------

    /// A DIB section selected into its own memory DC.
    struct Dib {
        dc: HDC,
        bitmap: HBITMAP,
        old: HGDIOBJ,
        bits: *mut u8,
        w: i32,
        h: i32,
    }

    impl Dib {
        fn new(w: i32, h: i32) -> Option<Dib> {
            unsafe {
                let dc = CreateCompatibleDC(None);
                let info = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: w.max(1),
                        biHeight: -h.max(1),
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
                let Ok(bitmap) = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
                    let _ = DeleteDC(dc);
                    return None;
                };
                let old = SelectObject(dc, bitmap.into());
                Some(Dib { dc, bitmap, old, bits: bits as *mut u8, w: w.max(1), h: h.max(1) })
            }
        }

        fn from_pixels(w: i32, h: i32, bgra: &[u8]) -> Option<Dib> {
            let dib = Dib::new(w, h)?;
            let len = (w * h * 4) as usize;
            if bgra.len() >= len {
                unsafe { std::ptr::copy_nonoverlapping(bgra.as_ptr(), dib.bits, len) };
            }
            Some(dib)
        }
    }

    impl Drop for Dib {
        fn drop(&mut self) {
            unsafe {
                SelectObject(self.dc, self.old);
                let _ = DeleteObject(self.bitmap.into());
                let _ = DeleteDC(self.dc);
            }
        }
    }

    /// A label: a rounded ink pill with text, premultiplied for AlphaBlend.
    struct Pill {
        dib: Dib,
    }

    fn colorref((r, g, b): (u8, u8, u8)) -> COLORREF {
        COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
    }

    fn font(px: i32) -> HFONT {
        unsafe {
            CreateFontW(
                -px,
                0,
                0,
                0,
                FW_SEMIBOLD.0 as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                ANTIALIASED_QUALITY,
                0,
                w!("Segoe UI Variable Text"),
            )
        }
    }

    fn pill(text: &str, scale: f32, font_px: f32) -> Option<Pill> {
        let wide: Vec<u16> = text.encode_utf16().collect();
        let px = super::super::geom::scaled(font_px, scale);
        let pad_x = super::super::geom::scaled(12.0, scale);
        let pad_y = super::super::geom::scaled(6.0, scale);
        unsafe {
            let f = font(px);
            // Measure on a scratch DC.
            let probe = Dib::new(1, 1)?;
            let old_font = SelectObject(probe.dc, f.into());
            let mut size = SIZE::default();
            let _ = GetTextExtentPoint32W(probe.dc, &wide, &mut size);
            SelectObject(probe.dc, old_font);
            drop(probe);
            let (w, h) = (size.cx + 2 * pad_x, size.cy + 2 * pad_y);
            let dib = Dib::new(w, h)?;
            // Opaque ink, then the text on it.
            let (r, g, b) = colours::LABEL;
            let pixels = std::slice::from_raw_parts_mut(dib.bits, (w * h * 4) as usize);
            for p in pixels.as_chunks_mut::<4>().0 {
                p.copy_from_slice(&[b, g, r, 255]);
            }
            let old_font = SelectObject(dib.dc, f.into());
            SetBkMode(dib.dc, TRANSPARENT);
            SetTextColor(dib.dc, colorref(colours::LABEL_TEXT));
            let _ = TextOutW(dib.dc, pad_x, pad_y, &wide);
            SelectObject(dib.dc, old_font);
            let _ = DeleteObject(f.into());
            // The rounded edge, anti-aliased, premultiplied (≈ 94 % opaque).
            let radius = h as f32 / 2.0;
            for y in 0..h {
                for x in 0..w {
                    let a = pill_coverage(x, y, w, h, radius) as u32 * 240 / 255;
                    let i = ((y * w + x) * 4) as usize;
                    for c in 0..3 {
                        pixels[i + c] = (pixels[i + c] as u32 * a / 255) as u8;
                    }
                    pixels[i + 3] = a as u8;
                }
            }
            Some(Pill { dib })
        }
    }

    struct Win {
        hwnd: HWND,
        screen: Option<Screen>,
        bright: Option<Dib>,
        dim: Option<Dib>,
        scratch: Option<Dib>,
        /// The drag's start, while the button is down.
        drag: Option<(i32, i32)>,
        sel: Option<Rect>,
        label: Option<(Rect, Pill)>,
        /// The hint and its alpha (255 → 0 as it fades).
        hint: Option<(Rect, Pill, u8)>,
    }

    struct Session {
        opts: Options,
        done: Option<Done>,
        started: Instant,
    }

    thread_local! {
        static WINDOWS: RefCell<Vec<Win>> = const { RefCell::new(Vec::new()) };
        static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
        static CLASS: RefCell<bool> = const { RefCell::new(false) };
    }

    fn instance() -> HINSTANCE {
        unsafe { GetModuleHandleW(None).map(|m| HINSTANCE(m.0)).unwrap_or_default() }
    }

    fn register_class() {
        CLASS.with(|c| {
            if *c.borrow() {
                return;
            }
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(wndproc),
                hInstance: instance(),
                hCursor: unsafe { LoadCursorW(None, IDC_CROSS) }.unwrap_or_default(),
                lpszClassName: w!("YapSnipOverlay"),
                ..Default::default()
            };
            unsafe { RegisterClassExW(&class) };
            *c.borrow_mut() = true;
        });
    }

    fn cloak(hwnd: HWND, on: bool) {
        let v: i32 = on.into();
        unsafe {
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_CLOAK, &v as *const i32 as *const _, 4);
        }
    }

    /// A window for every screen (made once, kept hidden and cloaked).
    fn ensure_windows(n: usize) {
        register_class();
        WINDOWS.with(|w| {
            let mut wins = w.borrow_mut();
            while wins.len() < n {
                let hwnd = unsafe {
                    CreateWindowExW(
                        WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                        w!("YapSnipOverlay"),
                        w!("Yap snip"),
                        WS_POPUP,
                        0,
                        0,
                        1,
                        1,
                        None,
                        None,
                        Some(instance()),
                        None,
                    )
                };
                let Ok(hwnd) = hwnd else { break };
                cloak(hwnd, true);
                let off: i32 = 1;
                unsafe {
                    let _ = DwmSetWindowAttribute(hwnd, DWMWA_TRANSITIONS_FORCEDISABLED, &off as *const i32 as *const _, 4);
                }
                wins.push(Win { hwnd, screen: None, bright: None, dim: None, scratch: None, drag: None, sel: None, label: None, hint: None });
            }
        });
    }

    fn index_of(hwnd: HWND) -> Option<usize> {
        WINDOWS.with(|w| w.borrow().iter().position(|x| x.hwnd == hwnd))
    }

    fn do_open(screens: Vec<Screen>, opts: Options, done: Done) -> Result<Shown, String> {
        let t0 = Instant::now();
        // A snip still up (shouldn't happen: snip.rs refuses one): ended.
        finish(Outcome::Cancelled("replaced"));
        ensure_windows(screens.len());
        let count = WINDOWS.with(|w| w.borrow().len());
        if count < screens.len() {
            return Err("couldn't make the overlay windows".into());
        }
        // The pictures: the frame and its dimmed copy, as DIBs (the dim on
        // every core).
        let mut prepared = Vec::new();
        for s in &screens {
            let (w, h) = (s.width as i32, s.height as i32);
            let bright = Dib::from_pixels(w, h, &s.bgra).ok_or("couldn't make a bitmap")?;
            let dim = Dib::from_pixels(w, h, &dimmed(&s.bgra)).ok_or("couldn't make a bitmap")?;
            prepared.push((bright, dim));
        }
        let hint = pill(HINT, screens[opts.focus.min(screens.len() - 1)].monitor.scale, 14.0);
        let hwnds: Vec<HWND> = WINDOWS.with(|w| {
            let mut wins = w.borrow_mut();
            for (i, ((bright, dim), s)) in prepared.into_iter().zip(&screens).enumerate() {
                let win = &mut wins[i];
                win.bright = Some(bright);
                win.dim = Some(dim);
                win.drag = None;
                win.sel = None;
                win.label = None;
                win.hint = None;
                win.screen = Some(s.clone());
            }
            if let (Some(p), Some(win)) = (hint, wins.get_mut(opts.focus.min(screens.len() - 1))) {
                let s = win.screen.as_ref().map(|s| (s.width as i32, s.monitor.scale)).unwrap_or((0, 1.0));
                let (pw, ph) = (p.dib.w, p.dib.h);
                let at = Rect::new((s.0 - pw) / 2, super::super::geom::scaled(28.0, s.1), pw, ph);
                win.hint = Some((at, p, 255));
            }
            wins.iter().take(screens.len()).map(|w| w.hwnd).collect()
        });
        SESSION.with(|s| *s.borrow_mut() = Some(Session { opts, done: Some(done), started: t0 }));
        OPEN.store(true, Ordering::SeqCst);
        // Placed, styled and painted while cloaked.
        for (hwnd, s) in hwnds.iter().zip(&screens) {
            let m = &s.monitor;
            let ex = if opts.interactive || opts.hidden {
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW
            } else {
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED
            };
            unsafe {
                SetWindowLongPtrW(*hwnd, GWL_EXSTYLE, ex.0 as isize);
                if ex.contains(WS_EX_LAYERED) {
                    let _ = SetLayeredWindowAttributes(*hwnd, COLORREF(0), 255, LWA_ALPHA);
                }
                cloak(*hwnd, true);
                let _ = SetWindowPos(*hwnd, Some(HWND_TOPMOST), m.x, m.y, s.width.max(1) as i32, s.height.max(1) as i32, SWP_NOACTIVATE);
                let _ = ShowWindow(*hwnd, SW_SHOWNOACTIVATE);
                let _ = RedrawWindow(Some(*hwnd), None, None, RDW_INVALIDATE | RDW_UPDATENOW);
            }
        }
        let setup_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let mut focused = false;
        if !opts.hidden {
            for hwnd in &hwnds {
                cloak(*hwnd, false);
            }
            unsafe {
                let _ = DwmFlush();
            }
            if opts.interactive {
                let target = hwnds[opts.focus.min(hwnds.len() - 1)];
                unsafe {
                    focused = SetForegroundWindow(target).as_bool();
                    let _ = SetFocus(Some(target));
                }
            }
            // The hint fades after a moment.
            let target = hwnds[opts.focus.min(hwnds.len() - 1)];
            unsafe { SetTimer(Some(target), HINT_TIMER, HINT_MS, None) };
        }
        // A snip nobody finishes doesn't stay up (nor keep Esc for itself).
        if let Some(first) = hwnds.first() {
            unsafe { SetTimer(Some(*first), GIVE_UP_TIMER, GIVE_UP_MS, None) };
        }
        Ok(Shown { setup_ms, visible_ms: t0.elapsed().as_secs_f64() * 1000.0, focused })
    }

    /// End the session: windows hidden and cloaked, bitmaps freed, `done` told.
    fn finish(outcome: Outcome) {
        let session = SESSION.with(|s| s.borrow_mut().take());
        let Some(mut session) = session else { return };
        OPEN.store(false, Ordering::SeqCst);
        let hwnds: Vec<HWND> = WINDOWS.with(|w| {
            let mut wins = w.borrow_mut();
            for win in wins.iter_mut() {
                win.bright = None;
                win.dim = None;
                win.scratch = None;
                win.label = None;
                win.hint = None;
                win.drag = None;
                win.sel = None;
                win.screen = None;
            }
            wins.iter().map(|w| w.hwnd).collect()
        });
        unsafe {
            let _ = ReleaseCapture();
        }
        for hwnd in hwnds {
            cloak(hwnd, true);
            unsafe {
                let _ = KillTimer(Some(hwnd), HINT_TIMER);
                let _ = KillTimer(Some(hwnd), FADE_TIMER);
                let _ = KillTimer(Some(hwnd), GIVE_UP_TIMER);
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
        }
        tracing::debug!(ms = session.started.elapsed().as_millis() as u64, "snip: overlay closed");
        if let Some(done) = session.done.take() {
            done(outcome);
        }
    }

    fn run_loop() {
        let mut msg = MSG::default();
        loop {
            let got = unsafe { GetMessageW(&mut msg, None, 0, 0) };
            if got.0 <= 0 {
                break;
            }
            if msg.hwnd.is_invalid() && msg.message == WM_JOB {
                run_jobs();
                continue;
            }
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    fn run_jobs() {
        loop {
            let job = ui().ok().and_then(|ui| ui.jobs.lock().ok().and_then(|mut q| q.pop_front()));
            let Some(job) = job else { break };
            match job {
                Job::Open(screens, opts, done, reply) => {
                    let shown = do_open(screens, opts, done);
                    let _ = reply.send(shown);
                }
                Job::Close(why) => finish(Outcome::Cancelled(why)),
                #[cfg(debug_assertions)]
                Job::Input(screen, msgs) => {
                    let hwnd = WINDOWS.with(|w| w.borrow().get(screen).map(|w| w.hwnd));
                    if let Some(hwnd) = hwnd {
                        for (msg, wp, lp) in msgs {
                            unsafe {
                                let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(Some(hwnd), msg, WPARAM(wp), LPARAM(lp));
                            }
                        }
                    }
                }
            }
        }
    }

    // ---- painting ---------------------------------------------------------------------------

    fn to_rect(r: &RECT) -> Rect {
        Rect::new(r.left, r.top, r.right - r.left, r.bottom - r.top)
    }

    fn win_rect(r: &Rect) -> RECT {
        RECT { left: r.x, top: r.y, right: r.right(), bottom: r.bottom() }
    }

    /// The update region's rectangles.
    fn update_rects(hwnd: HWND) -> Vec<Rect> {
        unsafe {
            let rgn = CreateRectRgn(0, 0, 0, 0);
            let _ = GetUpdateRgn(hwnd, rgn, false);
            let size = GetRegionData(rgn, 0, None);
            let mut out = Vec::new();
            if size >= std::mem::size_of::<RGNDATAHEADER>() as u32 {
                let mut buf = vec![0u8; size as usize];
                if GetRegionData(rgn, size, Some(buf.as_mut_ptr() as *mut RGNDATA)) != 0 {
                    let header = &*(buf.as_ptr() as *const RGNDATAHEADER);
                    let rects = std::slice::from_raw_parts(
                        buf.as_ptr().add(header.dwSize as usize) as *const RECT,
                        header.nCount as usize,
                    );
                    out.extend(rects.iter().map(to_rect));
                }
            }
            let _ = DeleteObject(rgn.into());
            out
        }
    }

    /// The box's edge width at a monitor's scale.
    fn edge_px(scale: f32) -> i32 {
        super::super::geom::scaled(1.5, scale)
    }

    /// Everything a box touches on screen: its edge and its label.
    fn dirty(win: &Win) -> Rect {
        let scale = win.screen.as_ref().map(|s| s.monitor.scale).unwrap_or(1.0);
        let mut r = win.sel.map(|s| s.inflate(edge_px(scale) + 1)).unwrap_or_default();
        if let Some((l, _)) = &win.label {
            r = r.union(l);
        }
        r
    }

    fn paint(hwnd: HWND) {
        let rects = update_rects(hwnd);
        let mut ps = PAINTSTRUCT::default();
        let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
        WINDOWS.with(|w| {
            let mut wins = w.borrow_mut();
            let Some(win) = wins.iter_mut().find(|x| x.hwnd == hwnd) else { return };
            let (Some(dim), Some(bright)) = (win.dim.as_ref(), win.bright.as_ref()) else { return };
            let scale = win.screen.as_ref().map(|s| s.monitor.scale).unwrap_or(1.0);
            let rects = if rects.is_empty() { vec![to_rect(&ps.rcPaint)] } else { rects };
            for r in rects {
                if r.is_empty() {
                    continue;
                }
                // A scratch bitmap at least this big (grows only).
                if win.scratch.as_ref().is_none_or(|s| s.w < r.w || s.h < r.h) {
                    let (sw, sh) = (
                        r.w.max(win.scratch.as_ref().map(|s| s.w).unwrap_or(0)),
                        r.h.max(win.scratch.as_ref().map(|s| s.h).unwrap_or(0)),
                    );
                    win.scratch = Dib::new(sw, sh);
                }
                let Some(scratch) = win.scratch.as_ref() else { continue };
                unsafe {
                    let _ = BitBlt(scratch.dc, 0, 0, r.w, r.h, Some(dim.dc), r.x, r.y, SRCCOPY);
                    if let Some(sel) = win.sel {
                        let inside = sel.intersect(&r);
                        if !inside.is_empty() {
                            let _ = BitBlt(scratch.dc, inside.x - r.x, inside.y - r.y, inside.w, inside.h, Some(bright.dc), inside.x, inside.y, SRCCOPY);
                        }
                        // The edge, just outside the box.
                        let e = edge_px(scale);
                        let brush = CreateSolidBrush(colorref(colours::EDGE));
                        let outer = sel.inflate(e);
                        for side in [
                            Rect::new(outer.x, outer.y, outer.w, e),
                            Rect::new(outer.x, sel.bottom(), outer.w, e),
                            Rect::new(outer.x, sel.y, e, sel.h),
                            Rect::new(sel.right(), sel.y, e, sel.h),
                        ] {
                            let s = side.intersect(&r);
                            if !s.is_empty() {
                                let rc = win_rect(&Rect::new(s.x - r.x, s.y - r.y, s.w, s.h));
                                FillRect(scratch.dc, &rc, brush);
                            }
                        }
                        let _ = DeleteObject(brush.into());
                    }
                    let blend = |dst: &Dib, at: &Rect, src: &Dib, alpha: u8| {
                        let part = at.intersect(&r);
                        if part.is_empty() {
                            return;
                        }
                        let f = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: alpha, AlphaFormat: AC_SRC_ALPHA as u8 };
                        let _ = AlphaBlend(dst.dc, part.x - r.x, part.y - r.y, part.w, part.h, src.dc, part.x - at.x, part.y - at.y, part.w, part.h, f);
                    };
                    if let Some((at, p)) = &win.label {
                        blend(scratch, at, &p.dib, 255);
                    }
                    if let Some((at, p, alpha)) = &win.hint {
                        blend(scratch, at, &p.dib, *alpha);
                    }
                    let _ = BitBlt(hdc, r.x, r.y, r.w, r.h, Some(scratch.dc), 0, 0, SRCCOPY);
                }
            }
        });
        unsafe {
            let _ = EndPaint(hwnd, &ps);
        }
    }

    fn invalidate(hwnd: HWND, r: &Rect) {
        if !r.is_empty() {
            let rc = win_rect(r);
            unsafe {
                let _ = InvalidateRect(Some(hwnd), Some(&rc), false);
            }
        }
    }

    // ---- input --------------------------------------------------------------------------------

    fn point(lp: LPARAM) -> (i32, i32) {
        ((lp.0 & 0xffff) as i16 as i32, ((lp.0 >> 16) & 0xffff) as i16 as i32)
    }

    fn hidden_session() -> bool {
        SESSION.with(|s| s.borrow().as_ref().is_some_and(|s| s.opts.hidden))
    }

    /// The box moved: its new place, its label, what to repaint.
    fn drag_to(hwnd: HWND, now: (i32, i32)) {
        let dirty_rects = WINDOWS.with(|w| {
            let mut wins = w.borrow_mut();
            let win = wins.iter_mut().find(|x| x.hwnd == hwnd)?;
            let start = win.drag?;
            let (sw, sh, scale) = win.screen.as_ref().map(|s| (s.width as i32, s.height as i32, s.monitor.scale))?;
            let before = dirty(win);
            let sel = super::super::geom::drag_box(start, now, sw, sh);
            if Some(sel) == win.sel {
                return None;
            }
            win.sel = Some(sel);
            win.label = (sel.w > 0 && sel.h > 0)
                .then(|| pill(&size_text(&sel), scale, 12.5))
                .flatten()
                .map(|p| (super::super::geom::label_at(&sel, (p.dib.w, p.dib.h), super::super::geom::scaled(8.0, scale), sw, sh), p));
            Some((before, dirty(win)))
        });
        if let Some((before, after)) = dirty_rects {
            invalidate(hwnd, &before);
            invalidate(hwnd, &after);
        }
    }

    fn hide_hint(hwnd: HWND) {
        let gone = WINDOWS.with(|w| {
            let mut wins = w.borrow_mut();
            let win = wins.iter_mut().find(|x| x.hwnd == hwnd)?;
            win.hint.take().map(|(r, _, _)| r)
        });
        if let Some(r) = gone {
            invalidate(hwnd, &r);
        }
        unsafe {
            let _ = KillTimer(Some(hwnd), HINT_TIMER);
            let _ = KillTimer(Some(hwnd), FADE_TIMER);
        }
    }

    fn on_button_down(hwnd: HWND, at: (i32, i32)) {
        // Every other monitor's hint and box go: one box at a time.
        let others: Vec<HWND> = WINDOWS.with(|w| w.borrow().iter().filter(|x| x.hwnd != hwnd && x.screen.is_some()).map(|x| x.hwnd).collect());
        for other in others {
            hide_hint(other);
        }
        hide_hint(hwnd);
        WINDOWS.with(|w| {
            if let Some(win) = w.borrow_mut().iter_mut().find(|x| x.hwnd == hwnd) {
                win.drag = Some(at);
            }
        });
        if !hidden_session() {
            unsafe { SetCapture(hwnd) };
        }
        drag_to(hwnd, at);
    }

    fn on_button_up(hwnd: HWND, at: (i32, i32)) {
        drag_to(hwnd, at);
        let done = WINDOWS.with(|w| {
            let mut wins = w.borrow_mut();
            let i = wins.iter().position(|x| x.hwnd == hwnd)?;
            let win = &mut wins[i];
            win.drag.take()?;
            let sel = win.sel?;
            if super::super::geom::big_enough(&sel) {
                Some((i, sel))
            } else {
                // A click or a twitch: nothing; the box goes.
                None
            }
        });
        unsafe {
            let _ = ReleaseCapture();
        }
        match done {
            Some((screen, rect)) => finish(Outcome::Snipped { screen, rect }),
            None => {
                let gone = WINDOWS.with(|w| {
                    let mut wins = w.borrow_mut();
                    let win = wins.iter_mut().find(|x| x.hwnd == hwnd)?;
                    let r = dirty(win);
                    win.sel = None;
                    win.label = None;
                    Some(r)
                });
                if let Some(r) = gone {
                    invalidate(hwnd, &r);
                }
            }
        }
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            // Its size is the monitor's, whatever DPI it moves to.
            WM_DPICHANGED => LRESULT(0),
            WM_MOUSEACTIVATE => LRESULT(MA_ACTIVATE as isize),
            WM_LBUTTONDOWN => {
                on_button_down(hwnd, point(lp));
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                drag_to(hwnd, point(lp));
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                on_button_up(hwnd, point(lp));
                LRESULT(0)
            }
            WM_RBUTTONDOWN => LRESULT(0),
            WM_RBUTTONUP => {
                finish(Outcome::Cancelled("right-click"));
                LRESULT(0)
            }
            WM_KEYDOWN if wp.0 as u16 == VK_ESCAPE.0 => {
                finish(Outcome::Cancelled("escape"));
                LRESULT(0)
            }
            // Another app took the focus (Alt+Tab, a notification clicked).
            WM_ACTIVATEAPP if wp.0 == 0 => {
                if !hidden_session() && index_of(hwnd).is_some() && is_open() {
                    finish(Outcome::Cancelled("another app"));
                }
                LRESULT(0)
            }
            WM_DISPLAYCHANGE => {
                finish(Outcome::Cancelled("the monitors changed"));
                LRESULT(0)
            }
            WM_TIMER if wp.0 == GIVE_UP_TIMER => {
                unsafe {
                    let _ = KillTimer(Some(hwnd), GIVE_UP_TIMER);
                }
                finish(Outcome::Cancelled("timed out"));
                LRESULT(0)
            }
            WM_TIMER if wp.0 == HINT_TIMER => {
                unsafe {
                    let _ = KillTimer(Some(hwnd), HINT_TIMER);
                    SetTimer(Some(hwnd), FADE_TIMER, FADE_MS, None);
                }
                LRESULT(0)
            }
            WM_TIMER if wp.0 == FADE_TIMER => {
                let step = WINDOWS.with(|w| {
                    let mut wins = w.borrow_mut();
                    let win = wins.iter_mut().find(|x| x.hwnd == hwnd)?;
                    let (r, _, alpha) = win.hint.as_mut()?;
                    let next = alpha.saturating_sub(255 / FADE_STEPS + 1);
                    *alpha = next;
                    Some((*r, next))
                });
                match step {
                    Some((_, 0)) => hide_hint(hwnd),
                    Some((r, _)) => invalidate(hwnd, &r),
                    None => unsafe {
                        let _ = KillTimer(Some(hwnd), FADE_TIMER);
                    },
                }
                LRESULT(0)
            }
            WM_SETCURSOR => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
            _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
        }
    }

    /// Where the cursor is (virtual-desktop pixels).
    pub fn cursor() -> Option<(i32, i32)> {
        let mut p = POINT::default();
        unsafe { GetCursorPos(&mut p) }.ok().map(|_| (p.x, p.y))
    }
}

/// Where the cursor is (virtual-desktop pixels).
#[cfg(windows)]
pub fn cursor() -> Option<(i32, i32)> {
    win::cursor()
}

#[cfg(not(windows))]
pub fn cursor() -> Option<(i32, i32)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rest_of_the_screen_dims() {
        let out = dimmed(&[200, 100, 0, 255, 255, 255, 255, 255, 0, 0, 0, 255]);
        assert_eq!(out, vec![117, 58, 0, 255, 149, 149, 149, 255, 0, 0, 0, 255]);
        assert!(dimmed(&[]).is_empty());
    }

    #[test]
    fn the_size_label_says_physical_pixels() {
        assert_eq!(size_text(&Rect::new(4, 5, 812, 430)), "812 × 430");
    }

    #[test]
    fn a_pill_is_round_and_soft_at_its_ends() {
        let (w, h) = (40, 20);
        // Its middle is solid, its corners empty, its edge in between.
        assert_eq!(pill_coverage(20, 10, w, h, 10.0), 255);
        assert_eq!(pill_coverage(0, 0, w, h, 10.0), 0);
        assert_eq!(pill_coverage(39, 19, w, h, 10.0), 0);
        let edge = pill_coverage(0, 10, w, h, 10.0);
        assert!(edge > 100, "{edge}");
        assert_eq!(pill_coverage(20, 0, w, h, 10.0), 255, "the straight top edge");
    }

    /// The overlay's colours are the bar's tokens (a native window can't
    /// read app.css): changing a token without them fails here.
    #[test]
    fn the_colours_are_the_bars_tokens() {
        let css = include_str!("../../../src/app.css");
        let token = |name: &str| -> (u8, u8, u8) {
            let at = css.find(&format!("{name}:")).unwrap_or_else(|| panic!("{name} not in app.css"));
            let hex = css[at..].split('#').nth(1).unwrap();
            let v = u32::from_str_radix(&hex[..6], 16).unwrap();
            ((v >> 16) as u8, (v >> 8) as u8, v as u8)
        };
        assert_eq!(colours::LABEL, token("--bar-tooltip"));
        assert_eq!(colours::LABEL_TEXT, token("--bar-tooltip-key"));
        assert_eq!(colours::EDGE, token("--bar-chip"));
    }
}

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
//! **Input** ([`Select`], pure and unit-tested; the windows only feed it):
//! - **drag a box** (the mouse captured, so it keeps going past a
//!   monitor's edge — **into the next monitor**: the box is in virtual-
//!   desktop pixels, clamped to the desktop, and every monitor's overlay
//!   draws its part); **release = done** (a box under 6 px either way does
//!   nothing);
//! - **a click** (the mouse barely moved) or **Enter**: the whole monitor
//!   under the pointer;
//! - **Space** switches to picking **a window** (and back); holding **Alt**
//!   picks windows while it's held. The window under the pointer is lit
//!   with its frame; a click takes its visible area. Enter takes what's lit;
//! - **Ctrl** held as you let go (or Ctrl+C while the box is up): **copy**
//!   instead of Chat; **Shift** (or Ctrl+S): **save** — the box's label
//!   says so as you hold them ("812 × 430 · Copy");
//! - **Esc**, right-click or the snip shortcut again cancel; another app
//!   taking the focus, or the monitors changing, cancel too.
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

/// What a finished snip is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    /// Into Chat's message box (a plain release, a click, Enter).
    Chat,
    /// Onto the clipboard (Ctrl held as you let go, Ctrl+C).
    Copy,
    /// Saved as a PNG where you choose (Shift held, Ctrl+S).
    Save,
}

/// What was taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// A box you dragged (on one monitor or across several).
    Box,
    /// A whole monitor (a click, Enter).
    Monitor,
    /// A window's visible area (Space or Alt, then a click).
    Window,
}

/// How a snip ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// What was taken, in virtual-desktop physical pixels.
    Snipped { rect: Rect, kind: Kind, action: Action },
    /// Esc, right-click, the shortcut again, another app, the monitors.
    Cancelled(&'static str),
}

/// How the overlays come up.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// The monitor under the cursor: it takes the focus and shows the hint.
    pub focus: usize,
    /// Take the focus (a snip). Off: shown but click-through and never
    /// focused (the spike's timing run, which must not take the mouse).
    pub interactive: bool,
    /// Never on screen (a test run): set up, painted and driven by posted
    /// messages, but kept cloaked and never focused.
    pub hidden: bool,
    /// The windows a click can pick (Space / Alt), topmost first, in
    /// virtual-desktop pixels (`pick.rs`; a test's own).
    pub windows: Vec<Rect>,
}

/// Ctrl held: copy; Shift held: save; neither: Chat. Pure.
pub fn action_for(ctrl: bool, shift: bool) -> Action {
    if ctrl {
        Action::Copy
    } else if shift {
        Action::Save
    } else {
        Action::Chat
    }
}

/// A key the overlay acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Enter,
    Space,
    Escape,
    /// Ctrl+C.
    Copy,
    /// Ctrl+S.
    Save,
    /// Ctrl went down (true) or up.
    Ctrl(bool),
    Shift(bool),
    Alt(bool),
}

/// What a key or the mouse changed.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Nothing,
    /// The box or its label changed: repaint.
    Redraw,
    /// Picking windows went on or off: repaint, and show the hint again.
    Mode,
    Done(Outcome),
}

/// The overlay's selection, pure: what the mouse and keys have picked so
/// far, across every monitor (virtual-desktop pixels). The windows feed it
/// their messages and draw [`Select::target`].
#[derive(Debug, Clone, Default)]
pub struct Select {
    /// Each screen's place on the virtual desktop.
    pub places: Vec<Rect>,
    pub desktop: Rect,
    /// The windows a click can pick, topmost first.
    pub windows: Vec<Rect>,
    /// The screen the snip started on (the pointer's, until it moves).
    pub focus: usize,
    /// Space switched to picking windows.
    pub window_mode: bool,
    /// Alt held: windows while it's held.
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
    drag: Option<(i32, i32)>,
    pointer: Option<(i32, i32)>,
    target: Option<(Rect, Kind)>,
}

impl Select {
    pub fn new(places: Vec<Rect>, windows: Vec<Rect>, focus: usize) -> Select {
        let desktop = super::geom::desktop(&places);
        let windows = windows.iter().map(|w| w.intersect(&desktop)).filter(|w| !w.is_empty()).collect();
        Select { places, desktop, windows, focus, ..Default::default() }
    }

    /// What's lit now (the box, or the window under the pointer), if
    /// anything.
    pub fn target(&self) -> Option<(Rect, Kind)> {
        self.target
    }

    pub fn picking_windows(&self) -> bool {
        self.window_mode || self.alt
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Where the pointer is (the focus screen's middle until it moves).
    pub fn pointer(&self) -> (i32, i32) {
        self.pointer.unwrap_or_else(|| {
            let m = self.places.get(self.focus).or(self.places.first()).copied().unwrap_or_default();
            (m.x + m.w / 2, m.y + m.h / 2)
        })
    }

    /// Set where the pointer is without a move (the overlay coming up).
    pub fn place_pointer(&mut self, at: (i32, i32)) {
        self.pointer = Some(at);
        self.refresh();
    }

    /// The monitor under a point, whole.
    fn monitor_under(&self, at: (i32, i32)) -> Rect {
        super::geom::monitor_at(&self.places, at.0, at.1).and_then(|i| self.places.get(i)).copied().unwrap_or_default()
    }

    /// The window under a point, when picking windows.
    fn window_under(&self, at: (i32, i32)) -> Option<Rect> {
        super::geom::window_at(&self.windows, at.0, at.1).map(|i| self.windows[i])
    }

    /// What's lit, from the state: a drag past a click's few pixels is a
    /// box; else, picking windows, the one under the pointer.
    fn refresh(&mut self) {
        if let (Some(start), Some(now)) = (self.drag, self.pointer) {
            if !super::geom::is_click(start, now) {
                self.target = Some((super::geom::drag_box_on(start, now, &self.desktop), Kind::Box));
                return;
            }
        }
        self.target = if self.picking_windows() { self.window_under(self.pointer()).map(|w| (w, Kind::Window)) } else { None };
    }

    /// What Enter, Ctrl+C and Ctrl+S take: what's lit, else the monitor
    /// under the pointer.
    fn current(&self) -> (Rect, Kind) {
        self.target.unwrap_or_else(|| (self.monitor_under(self.pointer()), Kind::Monitor))
    }

    /// The pointer moved (with Ctrl and Shift as the mouse message says).
    pub fn moved(&mut self, at: (i32, i32), ctrl: bool, shift: bool) -> Step {
        let before = (self.target, self.ctrl, self.shift);
        self.pointer = Some(at);
        self.ctrl = ctrl;
        self.shift = shift;
        self.refresh();
        if (self.target, self.ctrl, self.shift) == before {
            Step::Nothing
        } else {
            Step::Redraw
        }
    }

    /// The left button went down.
    pub fn pressed(&mut self, at: (i32, i32)) -> Step {
        self.drag = Some(at);
        self.pointer = Some(at);
        self.refresh();
        Step::Redraw
    }

    /// The left button came up: a click takes the monitor (or the window
    /// picked), a drag its box — or nothing, a box too thin to snip.
    pub fn released(&mut self, at: (i32, i32), ctrl: bool, shift: bool) -> Step {
        let Some(start) = self.drag.take() else { return Step::Nothing };
        self.pointer = Some(at);
        self.ctrl = ctrl;
        self.shift = shift;
        let action = action_for(ctrl, shift);
        if super::geom::is_click(start, at) {
            let (rect, kind) = match self.picking_windows().then(|| self.window_under(at)).flatten() {
                Some(w) => (w, Kind::Window),
                None => (self.monitor_under(at), Kind::Monitor),
            };
            return Step::Done(Outcome::Snipped { rect, kind, action });
        }
        let rect = super::geom::drag_box_on(start, at, &self.desktop);
        if super::geom::big_enough(&rect) {
            return Step::Done(Outcome::Snipped { rect, kind: Kind::Box, action });
        }
        // A twitch: nothing; the box goes.
        self.refresh();
        Step::Redraw
    }

    /// A key.
    pub fn key(&mut self, key: Key) -> Step {
        let done = |s: &Select, action: Action| {
            let (rect, kind) = s.current();
            Step::Done(Outcome::Snipped { rect, kind, action })
        };
        match key {
            Key::Escape => Step::Done(Outcome::Cancelled("escape")),
            Key::Enter => done(self, action_for(self.ctrl, self.shift)),
            Key::Copy => done(self, Action::Copy),
            Key::Save => done(self, Action::Save),
            Key::Space if self.dragging() => Step::Nothing,
            Key::Space => {
                self.window_mode = !self.window_mode;
                self.refresh();
                Step::Mode
            }
            Key::Alt(down) if down == self.alt => Step::Nothing,
            Key::Alt(down) => {
                self.alt = down;
                self.refresh();
                Step::Mode
            }
            Key::Ctrl(down) if down == self.ctrl => Step::Nothing,
            Key::Ctrl(down) => {
                self.ctrl = down;
                Step::Redraw
            }
            Key::Shift(down) if down == self.shift => Step::Nothing,
            Key::Shift(down) => {
                self.shift = down;
                Step::Redraw
            }
        }
    }
}

/// The lit box's label: its size in physical pixels, and what letting go
/// with Ctrl or Shift does — "812 × 430 · Ctrl copy · Shift save", or,
/// while one's held, "812 × 430 · Copy". Pure.
pub fn label_text(r: &Rect, ctrl: bool, shift: bool) -> String {
    let size = size_text(r);
    match action_for(ctrl, shift) {
        Action::Copy => format!("{size}  \u{b7}  Copy"),
        Action::Save => format!("{size}  \u{b7}  Save"),
        Action::Chat => format!("{size}  \u{b7}  Ctrl copy  \u{b7}  Shift save"),
    }
}

/// The hint at the top of the monitor under the pointer. Pure.
pub fn hint_text(picking_windows: bool) -> &'static str {
    if picking_windows {
        WINDOW_HINT
    } else {
        HINT
    }
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
pub const HINT: &str = "Drag to snip  ·  Click for the whole screen  ·  Space for a window  ·  Esc to cancel";
/// …while picking windows (Space, or Alt held).
pub const WINDOW_HINT: &str = "Click a window  ·  Space to drag a box  ·  Esc to cancel";
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
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, ReleaseCapture, SetCapture, SetFocus, VK_CONTROL, VK_ESCAPE, VK_F4, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_MENU, VK_RCONTROL,
        VK_RETURN, VK_RMENU, VK_RSHIFT, VK_SHIFT, VK_SPACE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetCursorPos, GetMessageW, KillTimer, LoadCursorW, PeekMessageW,
        PostThreadMessageW, RegisterClassExW, SetForegroundWindow, SetLayeredWindowAttributes, SetTimer, SetWindowLongPtrW,
        SetWindowPos, ShowWindow, TranslateMessage, GWL_EXSTYLE, HTCLIENT, HWND_TOPMOST, IDC_ARROW, IDC_CROSS, LWA_ALPHA, MSG, PM_NOREMOVE, SWP_NOACTIVATE,
        SW_HIDE, SW_SHOWNOACTIVATE, WM_ACTIVATEAPP, WM_APP, WM_DISPLAYCHANGE, WM_CHAR, WM_DPICHANGED, WM_ERASEBKGND, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN,
        WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_PAINT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WM_SYSCHAR, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER, WNDCLASSEXW,
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
        /// The lit box in this window's own pixels (it may reach past the
        /// window: a box across monitors).
        sel: Option<Rect>,
        /// The label's place, its text and its pill.
        label: Option<(Rect, String, Pill)>,
        /// The hint and its alpha (255 → 0 as it fades).
        hint: Option<(Rect, Pill, u8)>,
    }

    struct Session {
        opts: Options,
        done: Option<Done>,
        started: Instant,
        /// What the mouse and keys picked (virtual-desktop pixels).
        select: Select,
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
                wins.push(Win { hwnd, screen: None, bright: None, dim: None, scratch: None, sel: None, label: None, hint: None });
            }
        });
    }

    fn index_of(hwnd: HWND) -> Option<usize> {
        WINDOWS.with(|w| w.borrow().iter().position(|x| x.hwnd == hwnd))
    }

    /// A screen's place on the virtual desktop.
    fn place_of(s: &Screen) -> Rect {
        Rect::new(s.monitor.x, s.monitor.y, s.width as i32, s.height as i32)
    }

    /// The hint at the top of screen `i`, at full strength (its fade timer
    /// started when it's on screen); every other screen's goes.
    fn show_hint(i: usize, text: &str) {
        let shown = WINDOWS.with(|w| {
            let mut wins = w.borrow_mut();
            let mut gone = Vec::new();
            for (k, win) in wins.iter_mut().enumerate() {
                if k != i {
                    if let Some((r, _, _)) = win.hint.take() {
                        gone.push((win.hwnd, r));
                    }
                }
            }
            let win = wins.get_mut(i)?;
            let (sw, scale) = win.screen.as_ref().map(|s| (s.width as i32, s.monitor.scale))?;
            let p = pill(text, scale, 14.0)?;
            let (pw, ph) = (p.dib.w, p.dib.h);
            let at = Rect::new((sw - pw) / 2, super::super::geom::scaled(28.0, scale), pw, ph);
            let before = win.hint.replace((at, p, 255)).map(|(r, _, _)| r);
            gone.push((win.hwnd, before.unwrap_or_default().union(&at)));
            Some((win.hwnd, gone))
        });
        if let Some((hwnd, gone)) = shown {
            for (h, r) in gone {
                invalidate(h, &r);
                unsafe {
                    let _ = KillTimer(Some(h), HINT_TIMER);
                    let _ = KillTimer(Some(h), FADE_TIMER);
                }
            }
            if !hidden_session() {
                unsafe { SetTimer(Some(hwnd), HINT_TIMER, HINT_MS, None) };
            }
        }
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
        let focus = opts.focus.min(screens.len() - 1);
        let hwnds: Vec<HWND> = WINDOWS.with(|w| {
            let mut wins = w.borrow_mut();
            for (i, ((bright, dim), s)) in prepared.into_iter().zip(&screens).enumerate() {
                let win = &mut wins[i];
                win.bright = Some(bright);
                win.dim = Some(dim);
                win.sel = None;
                win.label = None;
                win.hint = None;
                win.screen = Some(s.clone());
            }
            wins.iter().take(screens.len()).map(|w| w.hwnd).collect()
        });
        let mut select = Select::new(screens.iter().map(place_of).collect(), opts.windows.clone(), focus);
        if opts.interactive && !opts.hidden {
            if let Some(at) = cursor() {
                select.place_pointer(at);
            }
        }
        let interactive = opts.interactive;
        let hidden = opts.hidden;
        SESSION.with(|s| *s.borrow_mut() = Some(Session { opts, done: Some(done), started: t0, select }));
        OPEN.store(true, Ordering::SeqCst);
        show_hint(focus, HINT);
        let opts = Options { focus, interactive, hidden, windows: Vec::new() };
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
                    // The cross at once, before the mouse moves.
                    if let Ok(cross) = LoadCursorW(None, IDC_CROSS) {
                        windows::Win32::UI::WindowsAndMessaging::SetCursor(Some(cross));
                    }
                }
            }
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
        if let Some((l, _, _)) = &win.label {
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
                    if let Some((at, _, p)) = &win.label {
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

    /// A real snip (not a test's hidden one, nor the spike's click-through
    /// one): only then is the mouse captured while you drag.
    fn interactive_session() -> bool {
        SESSION.with(|s| s.borrow().as_ref().is_some_and(|s| s.opts.interactive && !s.opts.hidden))
    }

    /// The selection changed: every screen's part of the lit box, the
    /// label on the screen holding most of it, what to repaint.
    fn redraw_all() {
        let Some((target, ctrl, shift)) = SESSION.with(|s| s.borrow().as_ref().map(|s| (s.select.target(), s.select.ctrl, s.select.shift)))
        else {
            return;
        };
        let rect = target.map(|(r, _)| r);
        let places: Vec<Rect> = WINDOWS.with(|w| w.borrow().iter().map(|x| x.screen.as_ref().map(place_of).unwrap_or_default()).collect());
        let label_on = rect.and_then(|r| super::super::geom::main_monitor(&r, &places));
        let text = rect.map(|r| label_text(&r, ctrl, shift));
        let dirty_rects: Vec<(HWND, Rect, Rect)> = WINDOWS.with(|w| {
            let mut wins = w.borrow_mut();
            let mut out = Vec::new();
            for (i, win) in wins.iter_mut().enumerate() {
                let Some((place, scale)) = win.screen.as_ref().map(|s| (place_of(s), s.monitor.scale)) else { continue };
                let local = rect.map(|r| Rect::new(r.x - place.x, r.y - place.y, r.w, r.h));
                let wants_label = label_on == Some(i);
                let same_label = match (&win.label, &text) {
                    (Some((_, t, _)), Some(want)) => wants_label && t == want,
                    (None, _) => !wants_label,
                    _ => false,
                };
                if win.sel == local && same_label {
                    continue;
                }
                let before = dirty(win);
                win.sel = local;
                if !same_label {
                    win.label = None;
                }
                if let (true, Some(sel), Some(t)) = (wants_label, local, &text) {
                    let keep = win.label.as_ref().is_some_and(|(_, lt, _)| lt == t);
                    let made = if keep { win.label.take().map(|(_, lt, p)| (lt, p)) } else { pill(t, scale, 12.5).map(|p| (t.clone(), p)) };
                    win.label = made.map(|(lt, p)| {
                        // Where it goes: by the box's part on this screen.
                        let part = sel.intersect(&Rect::new(0, 0, place.w, place.h));
                        let at = super::super::geom::label_at(
                            &part,
                            (p.dib.w, p.dib.h),
                            super::super::geom::scaled(8.0, scale),
                            place.w,
                            place.h,
                        );
                        (at, lt, p)
                    });
                }
                out.push((win.hwnd, before, dirty(win)));
            }
            out
        });
        for (hwnd, before, after) in dirty_rects {
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

    fn hide_every_hint() {
        let all: Vec<HWND> = WINDOWS.with(|w| w.borrow().iter().filter(|x| x.screen.is_some()).map(|x| x.hwnd).collect());
        for h in all {
            hide_hint(h);
        }
    }

    /// A point in `hwnd`'s own pixels on the virtual desktop. (A drag past
    /// the window comes to it, captured, in its own pixels: physical ones
    /// for a per-monitor-DPI-aware process, so the sum is exact.)
    fn global(hwnd: HWND, local: (i32, i32)) -> (i32, i32) {
        let origin = WINDOWS.with(|w| w.borrow().iter().find(|x| x.hwnd == hwnd).and_then(|x| x.screen.as_ref().map(place_of)));
        let o = origin.unwrap_or_default();
        (o.x + local.0, o.y + local.1)
    }

    /// Ctrl and Shift as a mouse message says them (`MK_CONTROL`, `MK_SHIFT`).
    fn mouse_mods(wp: WPARAM) -> (bool, bool) {
        (wp.0 & 0x0008 != 0, wp.0 & 0x0004 != 0)
    }

    /// Whether a key is held now (a real snip; a test's posted keys say it
    /// themselves).
    fn held(vk: u16) -> bool {
        interactive_session() && unsafe { GetKeyState(vk as i32) } < 0
    }

    /// Feed the selection, then act on what it says.
    fn step(change: impl FnOnce(&mut Select) -> Step) {
        let step = SESSION.with(|s| s.borrow_mut().as_mut().map(|s| change(&mut s.select)));
        match step {
            None | Some(Step::Nothing) => {}
            Some(Step::Redraw) => redraw_all(),
            Some(Step::Mode) => {
                redraw_all();
                let hint = SESSION.with(|s| {
                    s.borrow().as_ref().map(|s| {
                        let at = s.select.pointer();
                        let screen = super::super::geom::monitor_at(&s.select.places, at.0, at.1).unwrap_or(0);
                        (screen, hint_text(s.select.picking_windows()))
                    })
                });
                if let Some((screen, text)) = hint {
                    show_hint(screen, text);
                }
                set_cursor();
            }
            Some(Step::Done(outcome)) => {
                unsafe {
                    let _ = ReleaseCapture();
                }
                finish(outcome);
            }
        }
    }

    /// The cross for a box, the arrow while picking windows.
    fn set_cursor() {
        if !interactive_session() {
            return;
        }
        let picking = SESSION.with(|s| s.borrow().as_ref().is_some_and(|s| s.select.picking_windows()));
        if let Ok(c) = unsafe { LoadCursorW(None, if picking { IDC_ARROW } else { IDC_CROSS }) } {
            unsafe {
                windows::Win32::UI::WindowsAndMessaging::SetCursor(Some(c));
            }
        }
    }

    fn on_key(vk: u16, down: bool) {
        let ctrl_held = SESSION.with(|s| s.borrow().as_ref().is_some_and(|s| s.select.ctrl)) || held(VK_CONTROL.0);
        let key = match (vk, down) {
            (v, true) if v == VK_ESCAPE.0 => Some(Key::Escape),
            (v, true) if v == VK_RETURN.0 => Some(Key::Enter),
            (v, true) if v == VK_SPACE.0 => Some(Key::Space),
            (v, d) if v == VK_CONTROL.0 || v == VK_LCONTROL.0 || v == VK_RCONTROL.0 => Some(Key::Ctrl(d)),
            (v, d) if v == VK_SHIFT.0 || v == VK_LSHIFT.0 || v == VK_RSHIFT.0 => Some(Key::Shift(d)),
            (v, d) if v == VK_MENU.0 || v == VK_LMENU.0 || v == VK_RMENU.0 => Some(Key::Alt(d)),
            (0x43, true) if ctrl_held => Some(Key::Copy),
            (0x53, true) if ctrl_held => Some(Key::Save),
            _ => None,
        };
        if let Some(key) = key {
            step(|s| s.key(key));
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
                hide_every_hint();
                if interactive_session() {
                    unsafe { SetCapture(hwnd) };
                }
                let at = global(hwnd, point(lp));
                step(|s| s.pressed(at));
                LRESULT(0)
            }
            WM_MOUSEMOVE => {
                let at = global(hwnd, point(lp));
                let (ctrl, shift) = mouse_mods(wp);
                step(|s| s.moved(at, ctrl, shift));
                LRESULT(0)
            }
            WM_LBUTTONUP => {
                let at = global(hwnd, point(lp));
                let (ctrl, shift) = mouse_mods(wp);
                unsafe {
                    let _ = ReleaseCapture();
                }
                step(|s| s.released(at, ctrl, shift));
                LRESULT(0)
            }
            WM_RBUTTONDOWN => LRESULT(0),
            WM_RBUTTONUP => {
                finish(Outcome::Cancelled("right-click"));
                LRESULT(0)
            }
            WM_KEYDOWN | WM_KEYUP => {
                on_key(wp.0 as u16, msg == WM_KEYDOWN);
                LRESULT(0)
            }
            // Alt (and keys pressed with it) come as system keys: handled
            // here, so Alt never opens a window menu. Alt+F4 cancels.
            WM_SYSKEYDOWN if wp.0 as u16 == VK_F4.0 => {
                finish(Outcome::Cancelled("escape"));
                LRESULT(0)
            }
            WM_SYSKEYDOWN | WM_SYSKEYUP => {
                on_key(wp.0 as u16, msg == WM_SYSKEYDOWN);
                LRESULT(0)
            }
            WM_SYSCHAR | WM_CHAR => LRESULT(0),
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
            // Over the client area: the cross (a box) or the arrow (windows).
            WM_SETCURSOR if (lp.0 & 0xffff) as u32 == HTCLIENT && is_open() => {
                set_cursor();
                LRESULT(1)
            }
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
        // …and what Ctrl or Shift does as you let go, or what it will do.
        let r = Rect::new(0, 0, 812, 430);
        assert_eq!(label_text(&r, false, false), "812 × 430  ·  Ctrl copy  ·  Shift save");
        assert_eq!(label_text(&r, true, false), "812 × 430  ·  Copy");
        assert_eq!(label_text(&r, false, true), "812 × 430  ·  Save");
        assert_eq!(label_text(&r, true, true), "812 × 430  ·  Copy", "Ctrl wins");
        assert_eq!(hint_text(false), HINT);
        assert_eq!(hint_text(true), WINDOW_HINT);
    }

    /// The e2e suite's two fake monitors, and two windows on the first: a
    /// dialog over a big window.
    fn select() -> Select {
        Select::new(
            vec![Rect::new(0, 0, 1280, 720), Rect::new(1280, 0, 960, 540)],
            vec![Rect::new(300, 200, 400, 300), Rect::new(100, 100, 1000, 560)],
            0,
        )
    }

    fn snipped(step: Step) -> (Rect, Kind, Action) {
        match step {
            Step::Done(Outcome::Snipped { rect, kind, action }) => (rect, kind, action),
            other => panic!("not a snip: {other:?}"),
        }
    }

    #[test]
    fn a_drag_is_a_box_even_across_monitors() {
        let mut s = select();
        assert_eq!(s.pressed((1100, 150)), Step::Redraw);
        assert_eq!(s.target(), None, "nothing lit until it's a drag");
        assert_eq!(s.moved((1300, 200), false, false), Step::Redraw);
        assert_eq!(s.target(), Some((Rect::new(1100, 150, 200, 50), Kind::Box)));
        // Ctrl as it moves: only the label changes.
        assert_eq!(s.moved((1300, 200), true, false), Step::Redraw);
        assert_eq!(s.moved((1300, 200), true, false), Step::Nothing);
        let (rect, kind, action) = snipped(s.released((1580, 280), false, false));
        assert_eq!((rect, kind, action), (Rect::new(1100, 150, 480, 130), Kind::Box, Action::Chat));
        // A thin drag: nothing, the box goes, the snip goes on.
        let mut s = select();
        s.pressed((300, 300));
        assert_eq!(s.released((303, 340), false, false), Step::Redraw);
        assert_eq!(s.target(), None);
        assert!(!s.dragging());
    }

    #[test]
    fn a_click_or_enter_takes_the_whole_monitor() {
        let mut s = select();
        s.pressed((1500, 100));
        let (rect, kind, _) = snipped(s.released((1502, 101), false, false));
        assert_eq!((rect, kind), (Rect::new(1280, 0, 960, 540), Kind::Monitor));
        // Enter: the monitor under the pointer (the focus screen's until it moves).
        let mut s = select();
        assert_eq!(snipped(s.key(Key::Enter)).0, Rect::new(0, 0, 1280, 720));
        s.moved((2000, 500), false, false);
        assert_eq!(snipped(s.key(Key::Enter)), (Rect::new(1280, 0, 960, 540), Kind::Monitor, Action::Chat));
    }

    #[test]
    fn space_or_alt_picks_the_window_under_the_pointer() {
        let mut s = select();
        s.moved((350, 250), false, false);
        assert_eq!(s.target(), None, "no window lit before Space");
        assert_eq!(s.key(Key::Space), Step::Mode);
        assert!(s.picking_windows());
        assert_eq!(s.target(), Some((Rect::new(300, 200, 400, 300), Kind::Window)), "the dialog, on top");
        s.moved((150, 150), false, false);
        assert_eq!(s.target(), Some((Rect::new(100, 100, 1000, 560), Kind::Window)));
        s.pressed((150, 150));
        assert_eq!(snipped(s.released((151, 150), false, false)), (Rect::new(100, 100, 1000, 560), Kind::Window, Action::Chat));
        // Over no window: the monitor.
        let mut s = select();
        s.key(Key::Space);
        s.pressed((1200, 700));
        assert_eq!(snipped(s.released((1200, 700), false, false)).1, Kind::Monitor);
        // A drag while picking windows is still a box.
        let mut s = select();
        s.key(Key::Space);
        s.pressed((150, 150));
        assert_eq!(snipped(s.released((250, 260), false, false)).1, Kind::Box);
        // Alt held: windows while it's held; Space again: boxes.
        let mut s = select();
        s.moved((350, 250), false, false);
        assert_eq!(s.key(Key::Alt(true)), Step::Mode);
        assert_eq!(s.target().map(|t| t.1), Some(Kind::Window));
        assert_eq!(s.key(Key::Alt(true)), Step::Nothing, "a key's repeat");
        assert_eq!(s.key(Key::Alt(false)), Step::Mode);
        assert_eq!(s.target(), None);
        // Enter takes what's lit.
        s.key(Key::Space);
        assert_eq!(snipped(s.key(Key::Enter)).1, Kind::Window);
        // Space mid-drag does nothing.
        let mut s = select();
        s.pressed((10, 10));
        assert_eq!(s.key(Key::Space), Step::Nothing);
    }

    #[test]
    fn ctrl_copies_and_shift_saves() {
        let mut s = select();
        s.pressed((40, 40));
        s.moved((240, 140), false, false);
        assert_eq!(snipped(s.released((240, 140), true, false)).2, Action::Copy);
        let mut s = select();
        s.pressed((40, 40));
        assert_eq!(snipped(s.released((240, 140), false, true)).2, Action::Save);
        // Ctrl+C / Ctrl+S while the box is up: that box.
        let mut s = select();
        s.pressed((40, 40));
        s.moved((240, 140), false, false);
        assert_eq!(snipped(s.key(Key::Copy)), (Rect::new(40, 40, 200, 100), Kind::Box, Action::Copy));
        let mut s = select();
        assert_eq!(snipped(s.key(Key::Save)), (Rect::new(0, 0, 1280, 720), Kind::Monitor, Action::Save));
        // Ctrl held with Enter: copy the monitor.
        let mut s = select();
        assert_eq!(s.key(Key::Ctrl(true)), Step::Redraw);
        assert_eq!(snipped(s.key(Key::Enter)).2, Action::Copy);
        assert_eq!(s.key(Key::Escape), Step::Done(Outcome::Cancelled("escape")));
        assert_eq!(action_for(false, false), Action::Chat);
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

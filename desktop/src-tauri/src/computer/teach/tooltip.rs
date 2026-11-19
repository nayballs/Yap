//! **The teach tooltip** (phase 4): a small native window beside the step's
//! anchor, with a ring on the spot it points at.
//!
//! - **What it shows:** "Step 2 of 5", the explanation, what Next will do
//!   (smaller), then **Exit** and **Next**.
//! - **How it behaves:** Yap's dark card, its corners rounded by Windows,
//!   always on top. It never takes the focus (`WS_EX_NOACTIVATE`,
//!   `MA_NOACTIVATE`), so the app being taught stays in front. It's kept out
//!   of every capture (`WDA_EXCLUDEFROMCAPTURE`), so a screenshot never
//!   shows it.
//! - **Where it goes:** below and to the right of the anchor, flipped to
//!   stay on the display; centred with no anchor ([`place`], pure).
//! - **The ring** is click-through.
//! - **A test run never shows it** on the real screen: the tooltip's words
//!   are read with `e2e_computer_teach` instead.

use super::Tip;
use crate::snip::geom::Rect;

/// Its width at 100 % scaling, in pixels.
#[cfg_attr(any(test, not(windows)), allow(dead_code))]
pub const WIDTH: i32 = 360;
/// How far from the anchor it sits, at 100 %.
#[cfg_attr(any(test, not(windows)), allow(dead_code))]
pub const GAP: i32 = 22;
/// The ring's diameter on the anchor, at 100 %.
#[cfg_attr(any(test, not(windows)), allow(dead_code))]
pub const RING: i32 = 36;
/// The padding inside it, at 100 %.
const PAD: i32 = 16;
/// A button's size, at 100 %.
const BUTTON: (i32, i32) = (76, 30);

/// **Where the tooltip goes** (physical pixels): below and right of the
/// anchor, flipped left or up to stay on the display, never off it;
/// centred on the display with no anchor. Pure.
pub fn place(display: Rect, anchor: Option<(i32, i32)>, size: (i32, i32), gap: i32) -> (i32, i32) {
    let (w, h) = size;
    let (x, y) = match anchor {
        None => (display.x + (display.w - w) / 2, display.y + (display.h - h) / 2),
        Some((ax, ay)) => {
            let x = if ax + gap + w <= display.right() { ax + gap } else { ax - gap - w };
            let y = if ay + gap + h <= display.bottom() { ay + gap } else { ay - gap - h };
            (x, y)
        }
    };
    (x.clamp(display.x, (display.right() - w).max(display.x)), y.clamp(display.y, (display.bottom() - h).max(display.y)))
}

/// **Its buttons** inside a tooltip `w` × `h` at `scale`: Exit, then Next,
/// at the bottom right. Pure.
pub fn buttons(w: i32, h: i32, scale: f64) -> (Rect, Rect) {
    let s = |v: i32| (v as f64 * scale).round() as i32;
    let (bw, bh) = (s(BUTTON.0), s(BUTTON.1));
    let next = Rect::new(w - s(PAD) - bw, h - s(PAD) - bh, bw, bh);
    let exit = Rect::new(next.x - s(8) - bw, next.y, bw, bh);
    (exit, next)
}

/// **Where the tooltip is on screen now** (physical pixels): the card,
/// its Next and its Exit — for a robot run to click Next as the person
/// would. `None` while none shows.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct Placed {
    pub card: Rect,
    pub next: Rect,
    pub exit: Rect,
}

static PLACED: std::sync::Mutex<Option<Placed>> = std::sync::Mutex::new(None);

/// Where the tooltip shows now.
pub fn placed() -> Option<Placed> {
    *PLACED.lock().unwrap_or_else(|p| p.into_inner())
}

#[cfg_attr(any(test, not(windows)), allow(dead_code))]
fn set_placed(p: Option<Placed>) {
    *PLACED.lock().unwrap_or_else(|p| p.into_inner()) = p;
}

/// The header line: which step of how many.
#[cfg_attr(any(test, not(windows)), allow(dead_code))]
pub fn header(tip: &Tip) -> String {
    if tip.of > 1 {
        format!("Step {} of {}", tip.step, tip.of)
    } else {
        "Yap is showing you".to_string()
    }
}

#[cfg(all(windows, not(test)))]
pub fn show(tip: &Tip) {
    if crate::shell::e2e::active() && !crate::shell::realtest::on() {
        return; // A test run never puts windows on the real screen.
    }
    win::send(Some(tip.clone()));
}

#[cfg(any(not(windows), test))]
pub fn show(_tip: &Tip) {}

#[cfg(all(windows, not(test)))]
pub fn hide() {
    if crate::shell::e2e::active() && !crate::shell::realtest::on() {
        return;
    }
    win::send(None);
}

#[cfg(any(not(windows), test))]
pub fn hide() {}

#[cfg(all(windows, not(test)))]
mod win {
    use std::cell::RefCell;
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    use windows::core::w;
    use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DWM_WINDOW_CORNER_PREFERENCE};
    use windows::Win32::Graphics::Gdi::{
        BeginPaint, CombineRgn, CreateEllipticRgn, CreateFontIndirectW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect, FrameRect, GetDC, InvalidateRect, MonitorFromPoint, ReleaseDC, SelectObject, SetBkMode, SetTextColor, SetWindowRgn, DT_CALCRECT,
        DT_CENTER, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DT_WORDBREAK, HDC, HFONT, HGDIOBJ, LOGFONTW, MONITOR_DEFAULTTONEAREST, PAINTSTRUCT, RGN_DIFF, TRANSPARENT,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, LoadCursorW, PeekMessageW, RegisterClassW, SetCursor, SetLayeredWindowAttributes, SetWindowDisplayAffinity, SetWindowPos, ShowWindow, TranslateMessage, CS_DROPSHADOW, HWND_TOPMOST, IDC_ARROW, IDC_HAND,
        LWA_ALPHA, MSG, PM_REMOVE, SWP_NOACTIVATE, SW_HIDE, SW_SHOWNOACTIVATE, WDA_EXCLUDEFROMCAPTURE, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_PAINT, WM_SETCURSOR, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
        WS_EX_TRANSPARENT, WS_POPUP,
    };

    use super::super::{press, Pressed, Tip};
    use super::{buttons, header, place, GAP, PAD, RING, WIDTH};
    use crate::snip::geom::Rect;

    /// Yap's dark card and its accent (the bar's), as COLORREF (0x00BBGGRR).
    const CARD: u32 = 0x0024_2020;
    const EDGE: u32 = 0x004E_4646;
    const INK: u32 = 0x00F4_F4F4;
    const QUIET: u32 = 0x00A8_A8A8;
    const ACCENT: u32 = 0x0020_6ED6;
    const ACCENT_INK: u32 = 0x00FF_FFFF;

    static TX: OnceLock<Mutex<Sender<Option<Tip>>>> = OnceLock::new();

    pub fn send(tip: Option<Tip>) {
        let tx = TX.get_or_init(|| {
            let (tx, rx) = channel();
            std::thread::Builder::new().name("computer-teach".into()).spawn(move || run(rx)).ok();
            Mutex::new(tx)
        });
        let _ = tx.lock().unwrap_or_else(|p| p.into_inner()).send(tip);
    }

    /// What the tooltip draws now: the tip, its size, its scale and its buttons.
    struct Shown {
        tip: Tip,
        w: i32,
        h: i32,
        scale: f64,
        exit: Rect,
        next: Rect,
    }

    thread_local! {
        static SHOWN: RefCell<Option<Shown>> = const { RefCell::new(None) };
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    fn font(px: i32, bold: bool) -> HFONT {
        let mut lf = LOGFONTW { lfHeight: -px, lfWeight: if bold { 600 } else { 400 }, ..Default::default() };
        for (i, c) in "Segoe UI".encode_utf16().enumerate() {
            lf.lfFaceName[i] = c;
        }
        unsafe { CreateFontIndirectW(&lf) }
    }

    fn rect(r: Rect) -> RECT {
        RECT { left: r.x, top: r.y, right: r.right(), bottom: r.bottom() }
    }

    /// How tall `text` wraps to at `width` in `f`.
    fn measure(dc: HDC, f: HFONT, text: &str, width: i32) -> i32 {
        if text.is_empty() {
            return 0;
        }
        let mut r = RECT { left: 0, top: 0, right: width, bottom: 0 };
        let mut t = wide(text);
        unsafe {
            let old = SelectObject(dc, HGDIOBJ(f.0));
            DrawTextW(dc, &mut t, &mut r, DT_CALCRECT | DT_WORDBREAK | DT_NOPREFIX);
            SelectObject(dc, old);
        }
        r.bottom - r.top
    }

    fn scale_at(x: i32, y: i32) -> f64 {
        let m = unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) };
        let (mut dx, mut dy) = (96u32, 96u32);
        if unsafe { GetDpiForMonitor(m, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) }.is_err() {
            return 1.0;
        }
        (dx as f64 / 96.0).max(1.0)
    }

    /// The fonts at `scale`: the header, the explanation, the preview, a button.
    fn fonts(scale: f64) -> [HFONT; 4] {
        let s = |v: f64| (v * scale).round() as i32;
        [font(s(12.0), true), font(s(14.5), false), font(s(12.5), false), font(s(13.0), true)]
    }

    fn paint(h: HWND) {
        let mut ps = PAINTSTRUCT::default();
        let dc = unsafe { BeginPaint(h, &mut ps) };
        SHOWN.with(|s| {
            let s = s.borrow();
            let Some(s) = s.as_ref() else { return };
            let px = |v: i32| (v as f64 * s.scale).round() as i32;
            let [f_head, f_body, f_next, f_button] = fonts(s.scale);
            unsafe {
                let card = CreateSolidBrush(COLORREF(CARD));
                let edge = CreateSolidBrush(COLORREF(EDGE));
                let accent = CreateSolidBrush(COLORREF(ACCENT));
                let whole = RECT { left: 0, top: 0, right: s.w, bottom: s.h };
                FillRect(dc, &whole, card);
                FrameRect(dc, &whole, edge);
                SetBkMode(dc, TRANSPARENT);
                let inner = s.w - 2 * px(PAD);
                let mut y = px(PAD);
                let write = |f: HFONT, colour: u32, text: &str, y: &mut i32, after: i32| {
                    if text.is_empty() {
                        return;
                    }
                    let tall = measure(dc, f, text, inner);
                    let mut r = RECT { left: px(PAD), top: *y, right: px(PAD) + inner, bottom: *y + tall };
                    let mut t = wide(text);
                    let old = SelectObject(dc, HGDIOBJ(f.0));
                    SetTextColor(dc, COLORREF(colour));
                    DrawTextW(dc, &mut t, &mut r, DT_WORDBREAK | DT_NOPREFIX | DT_LEFT);
                    SelectObject(dc, old);
                    *y += tall + after;
                };
                write(f_head, QUIET, &header(&s.tip), &mut y, px(6));
                write(f_body, INK, &s.tip.explanation, &mut y, px(8));
                write(f_next, QUIET, &s.tip.next_preview, &mut y, 0);
                // Exit (quiet), Next (the accent).
                FillRect(dc, &rect(s.next), accent);
                FrameRect(dc, &rect(s.exit), edge);
                let old = SelectObject(dc, HGDIOBJ(f_button.0));
                SetTextColor(dc, COLORREF(QUIET));
                DrawTextW(dc, &mut wide("Exit"), &mut rect(s.exit), DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
                SetTextColor(dc, COLORREF(ACCENT_INK));
                DrawTextW(dc, &mut wide("Next"), &mut rect(s.next), DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
                SelectObject(dc, old);
                for o in [card.0, edge.0, accent.0, f_head.0, f_body.0, f_next.0, f_button.0] {
                    let _ = DeleteObject(HGDIOBJ(o));
                }
            }
        });
        unsafe {
            let _ = EndPaint(h, &ps);
        }
    }

    /// Which button is at a point inside the tooltip.
    fn button_at(x: i32, y: i32) -> Option<Pressed> {
        SHOWN.with(|s| {
            let s = s.borrow();
            let s = s.as_ref()?;
            let inside = |r: Rect| x >= r.x && x < r.right() && y >= r.y && y < r.bottom();
            if inside(s.next) {
                Some(Pressed::Next)
            } else if inside(s.exit) {
                Some(Pressed::Exit)
            } else {
                None
            }
        })
    }

    unsafe extern "system" fn tip_proc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match m {
            // Never the focus: the app being taught stays in front.
            WM_MOUSEACTIVATE => LRESULT(3), // MA_NOACTIVATE
            WM_PAINT => {
                paint(h);
                LRESULT(0)
            }
            WM_SETCURSOR => {
                let mut p = POINT::default();
                let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut p) };
                let _ = unsafe { windows::Win32::Graphics::Gdi::ScreenToClient(h, &mut p) };
                let cursor = if button_at(p.x, p.y).is_some() { IDC_HAND } else { IDC_ARROW };
                if let Ok(c) = unsafe { LoadCursorW(None, cursor) } {
                    unsafe { SetCursor(Some(c)) };
                }
                LRESULT(1)
            }
            WM_LBUTTONUP => {
                let x = (lp.0 & 0xFFFF) as i16 as i32;
                let y = ((lp.0 >> 16) & 0xFFFF) as i16 as i32;
                if let Some(p) = button_at(x, y) {
                    press(p);
                }
                LRESULT(0)
            }
            _ => unsafe { DefWindowProcW(h, m, wp, lp) },
        }
    }

    unsafe extern "system" fn ring_proc(h: HWND, m: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        unsafe { DefWindowProcW(h, m, wp, lp) }
    }

    fn run(rx: Receiver<Option<Tip>>) {
        let Ok(module) = (unsafe { GetModuleHandleW(None) }) else { return };
        let tip_class = WNDCLASSW { lpfnWndProc: Some(tip_proc), hInstance: module.into(), lpszClassName: w!("YapTeachTip"), style: CS_DROPSHADOW, ..Default::default() };
        let ring_brush = unsafe { CreateSolidBrush(COLORREF(ACCENT)) };
        let ring_class = WNDCLASSW { lpfnWndProc: Some(ring_proc), hInstance: module.into(), lpszClassName: w!("YapTeachRing"), hbrBackground: ring_brush, ..Default::default() };
        unsafe {
            RegisterClassW(&tip_class);
            RegisterClassW(&ring_class);
        }
        let tip = unsafe { CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, w!("YapTeachTip"), w!("Yap"), WS_POPUP, 0, 0, 1, 1, None, None, Some(module.into()), None) };
        let ring = unsafe { CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED, w!("YapTeachRing"), w!(""), WS_POPUP, 0, 0, 1, 1, None, None, Some(module.into()), None) };
        let (Ok(tip), Ok(ring)) = (tip, ring) else { return };
        unsafe {
            // Never in a capture: a screenshot of computer use's (or Snip's) doesn't show it.
            let _ = SetWindowDisplayAffinity(tip, WDA_EXCLUDEFROMCAPTURE);
            let _ = SetWindowDisplayAffinity(ring, WDA_EXCLUDEFROMCAPTURE);
            let _ = SetLayeredWindowAttributes(ring, COLORREF(0), 230, LWA_ALPHA);
            let round: DWM_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(tip, DWMWA_WINDOW_CORNER_PREFERENCE, &round as *const _ as *const _, std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32);
        }
        loop {
            match rx.recv_timeout(Duration::from_millis(30)) {
                Ok(Some(t)) => show(tip, ring, t),
                Ok(None) => unsafe {
                    SHOWN.with(|s| *s.borrow_mut() = None);
                    super::set_placed(None);
                    let _ = ShowWindow(tip, SW_HIDE);
                    let _ = ShowWindow(ring, SW_HIDE);
                },
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return,
            }
            let mut msg = MSG::default();
            while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
                unsafe {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }

    /// The tooltip for `t`: measured at the display's scale, placed, drawn;
    /// the ring on its anchor.
    fn show(tip_w: HWND, ring_w: HWND, t: Tip) {
        let centre = t.anchor.unwrap_or((t.display.x + t.display.w / 2, t.display.y + t.display.h / 2));
        let scale = scale_at(centre.0, centre.1);
        let px = |v: i32| (v as f64 * scale).round() as i32;
        let w = px(WIDTH);
        let inner = w - 2 * px(PAD);
        let [f_head, f_body, f_next, f_button] = fonts(scale);
        let dc = unsafe { GetDC(Some(tip_w)) };
        let tall = measure(dc, f_head, &header(&t), inner) + px(6) + measure(dc, f_body, &t.explanation, inner) + px(8) + measure(dc, f_next, &t.next_preview, inner);
        unsafe {
            ReleaseDC(Some(tip_w), dc);
            for f in [f_head, f_body, f_next, f_button] {
                let _ = DeleteObject(HGDIOBJ(f.0));
            }
        }
        let h = px(PAD) + tall + px(14) + px(30) + px(PAD);
        let (x, y) = place(t.display, t.anchor, (w, h), px(GAP));
        let (exit, next) = buttons(w, h, scale);
        let anchor = t.anchor;
        let on_screen = |r: Rect| Rect::new(x + r.x, y + r.y, r.w, r.h);
        super::set_placed(Some(super::Placed { card: Rect::new(x, y, w, h), next: on_screen(next), exit: on_screen(exit) }));
        SHOWN.with(|s| *s.borrow_mut() = Some(Shown { tip: t, w, h, scale, exit, next }));
        unsafe {
            let _ = SetWindowPos(tip_w, Some(HWND_TOPMOST), x, y, w, h, SWP_NOACTIVATE);
            let _ = InvalidateRect(Some(tip_w), None, true);
            let _ = ShowWindow(tip_w, SW_SHOWNOACTIVATE);
        }
        match anchor {
            Some((ax, ay)) => {
                let d = px(RING);
                let width = px(4).max(3);
                unsafe {
                    let outer = CreateEllipticRgn(0, 0, d + 1, d + 1);
                    let hole = CreateEllipticRgn(width, width, d + 1 - width, d + 1 - width);
                    let _ = CombineRgn(Some(outer), Some(outer), Some(hole), RGN_DIFF);
                    let _ = DeleteObject(HGDIOBJ(hole.0));
                    // (The window owns the region from here.)
                    SetWindowRgn(ring_w, Some(outer), true);
                    let _ = SetWindowPos(ring_w, Some(HWND_TOPMOST), ax - d / 2, ay - d / 2, d, d, SWP_NOACTIVATE);
                    let _ = ShowWindow(ring_w, SW_SHOWNOACTIVATE);
                }
            }
            None => unsafe {
                let _ = ShowWindow(ring_w, SW_HIDE);
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tooltip_sits_beside_its_anchor_and_stays_on_the_display() {
        let d = Rect::new(0, 0, 1920, 1080);
        assert_eq!(place(d, Some((100, 100)), (360, 200), 22), (122, 122), "below and right");
        assert_eq!(place(d, Some((1800, 100)), (360, 200), 22), (1800 - 22 - 360, 122), "flipped left at the right edge");
        assert_eq!(place(d, Some((100, 1000)), (360, 200), 22), (122, 1000 - 22 - 200), "flipped up at the bottom");
        assert_eq!(place(d, None, (360, 200), 22), (780, 440), "centred");
        let left = Rect::new(-1920, 0, 1920, 1080);
        assert_eq!(place(left, Some((-10, 10)), (360, 200), 22), (-10 - 22 - 360, 32), "a display left of the primary");
        let (exit, next) = buttons(360, 200, 1.0);
        assert_eq!((next.right(), next.bottom()), (344, 184));
        assert!(exit.right() < next.x);
        assert_eq!(buttons(720, 400, 2.0).1.w, 152, "scaled");
    }
}

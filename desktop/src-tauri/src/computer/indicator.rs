//! **"Yap is using your PC"** (phase 2): while computer use drives the
//! PC, the person sees it.
//!
//! - **The bar**: "Yap is using your PC · Esc to stop" with the latest action's
//!   summary (`bar::set_computer`). It shows bar on or off, stays put while the
//!   model's pointer moves, and never takes a click.
//! - **A thin border** round the display being driven ([`border`]): four
//!   click-through strips on top of everything, kept out of every capture
//!   (`WDA_EXCLUDEFROMCAPTURE`), so no screenshot ever shows them.
//!
//! Both go [`LINGER`] after the last action ends, unless another begins: a
//! turn's actions read as one stretch, not a flicker each. Esc (or a key, a
//! click) meanwhile takes them away at once: the next action won't run
//! (`watch.rs`).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::snip::geom::Rect;

/// How long the indicator stays after the last action.
pub const LINGER: Duration = Duration::from_secs(4);

/// Each stretch of driving: an action that begins bumps it.
static STRETCH: AtomicU64 = AtomicU64::new(0);

/// **An action begins**: the bar says what (`summary`), the border goes
/// round `display`.
pub fn driving(summary: &str, display: Option<Rect>) {
    STRETCH.fetch_add(1, Ordering::Relaxed);
    let text = if summary.trim().is_empty() { "Working in your apps".to_string() } else { summary.trim().chars().take(80).collect() };
    if let Some(app) = crate::chat::pages::view::app() {
        crate::bar::set_computer(&app, Some(text));
    }
    if let Some(r) = display {
        border::show(r);
    }
}

/// **The action ended**: the indicator goes after [`LINGER`] unless another
/// action begins meanwhile (and the watch stops listening for the person).
pub fn done() {
    // (A unit test's actions leave nothing lingering to switch the watch off under the next test.)
    if cfg!(test) {
        return;
    }
    let stretch = STRETCH.load(Ordering::Relaxed);
    let ended = super::watch::clock();
    std::thread::spawn(move || {
        let until = std::time::Instant::now() + LINGER;
        while std::time::Instant::now() < until {
            std::thread::sleep(Duration::from_millis(100));
            if STRETCH.load(Ordering::Relaxed) != stretch {
                return;
            }
            // The person took over while Claude thinks about its next step: the line goes now.
            if super::watch::taken_over_since(ended) {
                off();
                return;
            }
        }
        if STRETCH.load(Ordering::Relaxed) == stretch {
            off();
        }
    });
}

/// The indicator off now (the person took over: no lingering).
pub fn off() {
    STRETCH.fetch_add(1, Ordering::Relaxed);
    super::watch::disarm();
    if let Some(app) = crate::chat::pages::view::app() {
        crate::bar::set_computer(&app, None);
    }
    border::hide();
}

/// **The border round the driven display.**
mod border {
    use crate::snip::geom::Rect;

    /// Its width in physical pixels.
    pub const WIDTH: i32 = 4;

    /// The four strips round `r` (top, bottom, left, right). Pure.
    pub fn strips(r: Rect) -> [Rect; 4] {
        [
            Rect::new(r.x, r.y, r.w, WIDTH),
            Rect::new(r.x, r.bottom() - WIDTH, r.w, WIDTH),
            Rect::new(r.x, r.y + WIDTH, WIDTH, r.h - 2 * WIDTH),
            Rect::new(r.right() - WIDTH, r.y + WIDTH, WIDTH, r.h - 2 * WIDTH),
        ]
    }

    #[cfg(all(windows, not(test)))]
    pub fn show(r: Rect) {
        if crate::shell::e2e::active() && !crate::shell::realtest::on() {
            return; // A test run never puts windows on the real screen.
        }
        win::send(Some(r));
    }

    #[cfg(any(not(windows), test))]
    pub fn show(_r: Rect) {}

    #[cfg(all(windows, not(test)))]
    pub fn hide() {
        win::send(None);
    }

    #[cfg(any(not(windows), test))]
    pub fn hide() {}

    #[cfg(all(windows, not(test)))]
    mod win {
        use std::sync::mpsc::{channel, Receiver, Sender};
        use std::sync::{Mutex, OnceLock};
        use std::time::Duration;

        use windows::core::w;
        use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
        use windows::Win32::Graphics::Gdi::CreateSolidBrush;
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DispatchMessageW, PeekMessageW, RegisterClassW, SetLayeredWindowAttributes, SetWindowDisplayAffinity, SetWindowPos, ShowWindow, TranslateMessage, HWND_TOPMOST, LWA_ALPHA, MSG, PM_REMOVE, SWP_NOACTIVATE,
            SW_HIDE, SW_SHOWNOACTIVATE, WDA_EXCLUDEFROMCAPTURE, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
        };

        use super::strips;
        use crate::snip::geom::Rect;

        static TX: OnceLock<Mutex<Sender<Option<Rect>>>> = OnceLock::new();

        pub fn send(r: Option<Rect>) {
            let tx = TX.get_or_init(|| {
                let (tx, rx) = channel();
                std::thread::Builder::new().name("computer-border".into()).spawn(move || run(rx)).ok();
                Mutex::new(tx)
            });
            let _ = tx.lock().unwrap_or_else(|p| p.into_inner()).send(r);
        }

        unsafe extern "system" fn proc_(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
            unsafe { DefWindowProcW(h, m, w, l) }
        }

        fn run(rx: Receiver<Option<Rect>>) {
            let Ok(module) = (unsafe { GetModuleHandleW(None) }) else { return };
            // Yap's recording red, a little see-through.
            let brush = unsafe { CreateSolidBrush(COLORREF(0x004D_48E5)) };
            let class = WNDCLASSW { lpfnWndProc: Some(proc_), hInstance: module.into(), lpszClassName: w!("YapComputerBorder"), hbrBackground: brush, ..Default::default() };
            unsafe { RegisterClassW(&class) };
            let ex = WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED;
            let strips_now: Vec<HWND> = (0..4)
                .filter_map(|_| unsafe { CreateWindowExW(ex, w!("YapComputerBorder"), w!(""), WS_POPUP, 0, 0, 1, 1, None, None, Some(module.into()), None) }.ok())
                .collect();
            for h in &strips_now {
                unsafe {
                    let _ = SetLayeredWindowAttributes(*h, COLORREF(0), 210, LWA_ALPHA);
                    // Never in a capture: no screenshot of computer use's (or Snip's) shows it.
                    let _ = SetWindowDisplayAffinity(*h, WDA_EXCLUDEFROMCAPTURE);
                }
            }
            loop {
                match rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(Some(r)) => {
                        for (h, s) in strips_now.iter().zip(strips(r)) {
                            unsafe {
                                let _ = SetWindowPos(*h, Some(HWND_TOPMOST), s.x, s.y, s.w.max(1), s.h.max(1), SWP_NOACTIVATE);
                                let _ = ShowWindow(*h, SW_SHOWNOACTIVATE);
                            }
                        }
                    }
                    Ok(None) => {
                        for h in &strips_now {
                            unsafe {
                                let _ = ShowWindow(*h, SW_HIDE);
                            }
                        }
                    }
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
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_border_is_four_strips_inside_the_display() {
            let s = strips(Rect::new(-1920, 0, 1920, 1080));
            assert_eq!(s[0], Rect::new(-1920, 0, 1920, WIDTH));
            assert_eq!(s[1], Rect::new(-1920, 1080 - WIDTH, 1920, WIDTH));
            assert_eq!(s[2], Rect::new(-1920, WIDTH, WIDTH, 1080 - 2 * WIDTH));
            assert_eq!(s[3], Rect::new(-WIDTH, WIDTH, WIDTH, 1080 - 2 * WIDTH));
        }
    }
}

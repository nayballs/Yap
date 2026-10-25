//! The windows a snip can pick (Space in the overlay, or Alt held: click a
//! window and its visible area is taken): every app window on screen at
//! the moment of the capture, topmost first, as its **visible frame** in
//! physical pixels (`DWMWA_EXTENDED_FRAME_BOUNDS`: without the invisible
//! resize borders Windows 10 and 11 put round a window, and a maximised
//! window is its monitor, not 8 px past it).
//!
//! Left out ([`pickable`], pure): hidden and minimised windows, cloaked
//! ones (another virtual desktop's, a suspended Store app's), click-through
//! ones (`WS_EX_TRANSPARENT`: overlays that would catch every point), the
//! desktop itself (`Progman` / `WorkerW`: pointing at the wallpaper takes
//! the monitor), anything under 6 px, and Yap's own (the overlay; the main
//! window, which isn't in the picture when the snip came from Chat).
//!
//! Read once, after the capture and before the overlay comes up, so the
//! list is the picture's. Never a window's title or contents: rects only.

use super::geom::{Rect, MIN_SIDE};

/// What [`pickable`] decides from.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Candidate {
    pub rect: Rect,
    pub visible: bool,
    pub minimized: bool,
    pub cloaked: bool,
    pub click_through: bool,
    pub own: bool,
    pub class: String,
}

/// The desktop's own windows (pointing at the wallpaper takes the monitor).
const DESKTOP_CLASSES: &[&str] = &["Progman", "WorkerW"];

/// Whether a window can be picked. Pure.
pub fn pickable(c: &Candidate) -> bool {
    c.visible
        && !c.minimized
        && !c.cloaked
        && !c.click_through
        && !c.own
        && c.rect.w >= MIN_SIDE
        && c.rect.h >= MIN_SIDE
        && !DESKTOP_CLASSES.contains(&c.class.as_str())
}

/// The windows on screen now, topmost first, clipped to `desktop`.
#[cfg(windows)]
pub fn windows_on_screen(desktop: &Rect) -> Vec<Rect> {
    win::candidates()
        .into_iter()
        .filter(pickable)
        .map(|c| c.rect.intersect(desktop))
        .filter(|r| r.w >= MIN_SIDE && r.h >= MIN_SIDE)
        .collect()
}

#[cfg(not(windows))]
pub fn windows_on_screen(_desktop: &Rect) -> Vec<Rect> {
    Vec::new()
}

#[cfg(windows)]
mod win {
    use super::{Candidate, Rect};
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindowVisible, GWL_EXSTYLE,
        WS_EX_TRANSPARENT,
    };

    fn frame(hwnd: HWND) -> Option<Rect> {
        let mut r = RECT::default();
        let ok = unsafe { DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut RECT as *mut _, std::mem::size_of::<RECT>() as u32) }
            .is_ok()
            || unsafe { GetWindowRect(hwnd, &mut r) }.is_ok();
        ok.then(|| Rect::new(r.left, r.top, r.right - r.left, r.bottom - r.top))
    }

    fn class(hwnd: HWND) -> String {
        let mut buf = [0u16; 64];
        let n = unsafe { GetClassNameW(hwnd, &mut buf) };
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }

    fn cloaked(hwnd: HWND) -> bool {
        let mut v: u32 = 0;
        unsafe { DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut v as *mut u32 as *mut _, 4) }.is_ok() && v != 0
    }

    unsafe extern "system" fn each(hwnd: HWND, lp: LPARAM) -> BOOL {
        let out = unsafe { &mut *(lp.0 as *mut Vec<Candidate>) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        let visible = unsafe { IsWindowVisible(hwnd) }.as_bool();
        if visible {
            let ex = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
            out.push(Candidate {
                rect: frame(hwnd).unwrap_or_default(),
                visible,
                minimized: unsafe { IsIconic(hwnd) }.as_bool(),
                cloaked: cloaked(hwnd),
                click_through: ex & WS_EX_TRANSPARENT.0 != 0,
                own: pid == std::process::id(),
                class: class(hwnd),
            });
        }
        BOOL(1)
    }

    /// Every visible top-level window, top to bottom.
    pub fn candidates() -> Vec<Candidate> {
        let mut out: Vec<Candidate> = Vec::new();
        unsafe {
            let _ = EnumWindows(Some(each), LPARAM(&mut out as *mut Vec<Candidate> as isize));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(rect: Rect) -> Candidate {
        Candidate { rect, visible: true, class: "SomeApp".into(), ..Default::default() }
    }

    #[test]
    fn only_app_windows_on_screen_are_picked() {
        let w = window(Rect::new(10, 10, 800, 600));
        assert!(pickable(&w));
        for not in [
            Candidate { visible: false, ..w.clone() },
            Candidate { minimized: true, ..w.clone() },
            Candidate { cloaked: true, ..w.clone() },
            Candidate { click_through: true, ..w.clone() },
            Candidate { own: true, ..w.clone() },
            Candidate { class: "Progman".into(), ..w.clone() },
            Candidate { class: "WorkerW".into(), ..w.clone() },
            Candidate { rect: Rect::new(0, 0, 5, 300), ..w.clone() },
        ] {
            assert!(!pickable(&not), "{not:?}");
        }
        // The taskbar is a window you can pick.
        assert!(pickable(&Candidate { class: "Shell_TrayWnd".into(), ..w }));
    }

    /// The real list on this PC: read-only, rects only.
    #[cfg(windows)]
    #[test]
    #[ignore = "this machine's windows"]
    fn this_machine() {
        let monitors: Vec<Rect> = crate::snip::capture::monitors()
            .iter()
            .map(|m| Rect::new(m.x, m.y, m.width as i32, m.height as i32))
            .collect();
        let list = windows_on_screen(&crate::snip::geom::desktop(&monitors));
        println!("{} windows: {list:?}", list.len());
    }
}

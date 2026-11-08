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
//! list is the picture's (well under a millisecond: 10 windows in 0.6 ms on
//! the maintainer's PC). Never a window's title or contents: rects only.
//! The pattern — the visible top-level windows' rects, the topmost one
//! under the pointer — is Kiri's (MIT, Copyright (c) 2026 yuxino; notice in
//! `THIRD-PARTY-NOTICES.md`), written here with Yap's own filters.

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
    why_not(c).is_none()
}

/// **Why a window can't be picked** (`None`: it can), in a few words for the
/// log — never its title or contents. Pure.
pub fn why_not(c: &Candidate) -> Option<&'static str> {
    if !c.visible {
        Some("hidden")
    } else if c.minimized {
        Some("minimised")
    } else if c.cloaked {
        Some("cloaked (another desktop, or a suspended app)")
    } else if c.click_through {
        Some("click-through")
    } else if c.own {
        Some("one of Yap's own")
    } else if c.rect.w < MIN_SIDE || c.rect.h < MIN_SIDE {
        Some("too small")
    } else if DESKTOP_CLASSES.contains(&c.class.as_str()) {
        Some("the desktop")
    } else {
        None
    }
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

/// **One window's visible frame**, by its handle (the Voice Assistant's
/// "window in front", `chat/assistant/screen.rs`), when it could be picked
/// ([`pickable`]: on screen, not minimised or cloaked, not Yap's own, not
/// the desktop); else why not ([`why_not`], or none in front, or gone).
/// Rects only, as ever.
#[cfg(windows)]
pub fn window_frame(hwnd: isize) -> Result<Rect, &'static str> {
    if hwnd == 0 {
        return Err("no window in front at the press");
    }
    let c = win::candidate(hwnd).ok_or("gone or hidden since the press")?;
    match why_not(&c) {
        None => Ok(c.rect),
        Some(why) => Err(why),
    }
}

#[cfg(not(windows))]
pub fn window_frame(_hwnd: isize) -> Result<Rect, &'static str> {
    Err("not on this system")
}

/// **The monitor a window is on** (the nearest; a minimised one's where it
/// was), else — no such window — the one under the pointer: its rect in
/// physical pixels. What the Voice Assistant takes when the window in front
/// itself can't be (`chat/assistant/screen.rs`).
#[cfg(windows)]
pub fn monitor_of(hwnd: isize) -> Option<Rect> {
    win::monitor(hwnd)
}

#[cfg(not(windows))]
pub fn monitor_of(_hwnd: isize) -> Option<Rect> {
    None
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

    /// One top-level window (the root of `hwnd`), as [`each`] reads it;
    /// `None`: no such window, or it isn't visible.
    pub fn candidate(hwnd: isize) -> Option<Candidate> {
        use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GA_ROOT};
        if hwnd == 0 {
            return None;
        }
        // (A handle that's gone: not visible, so `each` adds nothing.)
        let root = unsafe { GetAncestor(HWND(hwnd as *mut _), GA_ROOT) };
        let hwnd = if root.0.is_null() { HWND(hwnd as *mut _) } else { root };
        let mut out: Vec<Candidate> = Vec::new();
        unsafe {
            let _ = each(hwnd, LPARAM(&mut out as *mut Vec<Candidate> as isize));
        }
        out.pop()
    }

    /// The monitor `hwnd` is on, else the one under the pointer.
    pub fn monitor(hwnd: isize) -> Option<Rect> {
        use windows::Win32::Foundation::POINT;
        use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL};
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let mut m = if hwnd == 0 { HMONITOR::default() } else { unsafe { MonitorFromWindow(HWND(hwnd as *mut _), MONITOR_DEFAULTTONULL) } };
        if m.is_invalid() {
            let mut p = POINT::default();
            unsafe { GetCursorPos(&mut p) }.ok()?;
            m = unsafe { MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST) };
        }
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !unsafe { GetMonitorInfoW(m, &mut info) }.as_bool() {
            return None;
        }
        let r = info.rcMonitor;
        Some(Rect::new(r.left, r.top, r.right - r.left, r.bottom - r.top))
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
            assert!(why_not(&not).is_some(), "{not:?}");
        }
        // Why, for the log (the Voice Assistant's look then takes its monitor).
        assert_eq!(why_not(&Candidate { own: true, ..w.clone() }), Some("one of Yap's own"));
        assert_eq!(why_not(&Candidate { minimized: true, ..w.clone() }), Some("minimised"));
        assert_eq!(why_not(&w), None);
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
        let t = std::time::Instant::now();
        let list = windows_on_screen(&crate::snip::geom::desktop(&monitors));
        println!("{} windows in {:.1} ms: {list:?}", list.len(), t.elapsed().as_secs_f64() * 1000.0);
    }
}

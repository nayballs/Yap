//! `yap.exe snip-dump [--out <folder>] [--no-overlay] [--runs N] [--demo]` (debug
//! builds): the snip's spike. Captures every monitor — WGC with Yap's HDR
//! path, WGC in plain 8-bit, GDI — times each step, writes the PNGs into
//! `--out` (a folder you choose; nothing else is written), and times the
//! overlay coming up over the frozen pictures (shown click-through and
//! never focused for ~0.3 s each run: it never takes the keyboard or the
//! mouse). Prints a JSON summary. Read-only: nothing of Yap starts.

use std::path::PathBuf;
use std::time::Instant;

use super::capture::{self, Force, Frame};
use super::geom::{crop_rgba, Rect};
use super::images::encode_png;
use super::overlay;

fn ms(t: Instant) -> f64 {
    (t.elapsed().as_secs_f64() * 1000.0 * 10.0).round() / 10.0
}

fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v[v.len() / 2]
}

/// The DPI awareness Yap's windows run with (tao sets it in the app; this
/// command runs before any of the app).
#[cfg(windows)]
fn per_monitor_dpi() {
    use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

#[cfg(not(windows))]
fn per_monitor_dpi() {}

/// The average 8-bit colour of a frame's centre tenth (a quick look at
/// what each path made of the same screen).
fn centre_mean(f: &Frame) -> [u32; 3] {
    let r = Rect::new((f.width * 9 / 20) as i32, (f.height * 9 / 20) as i32, (f.width / 10).max(1) as i32, (f.height / 10).max(1) as i32);
    let px = crop_rgba(&f.bgra, f.width, &r);
    let n = (px.len() / 4).max(1) as u32;
    let mut sum = [0u32; 3];
    for p in px.as_chunks::<4>().0 {
        for c in 0..3 {
            sum[c] += p[c] as u32;
        }
    }
    sum.map(|s| s / n)
}

/// Known sRGB swatches, the HDR check: the greys an 8-bit capture of an HDR
/// desktop clips to white, and a saturated red.
pub const SWATCHES: [(u8, u8, u8); 6] = [(0x20, 0x20, 0x20), (0x80, 0x80, 0x80), (0xc0, 0xc0, 0xc0), (0xe8, 0xe8, 0xe8), (0xf5, 0xf5, 0xf5), (0xe5, 0x39, 0x35)];
/// Each swatch's side (physical px).
pub const SWATCH_SIDE: i32 = 100;

/// A plain GDI window of [`SWATCHES`] in the middle of every monitor —
/// what any SDR app is to Windows' HDR compositor — click-through, never
/// focused, until the returned closer runs. Its rects are in virtual-
/// desktop pixels.
#[cfg(windows)]
mod swatch {
    use super::{SWATCHES, SWATCH_SIDE};
    use crate::snip::capture::Monitor;
    use crate::snip::geom::Rect;
    use std::sync::mpsc;
    use windows::core::w;
    use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, PAINTSTRUCT};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::*;

    unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if msg == WM_PAINT {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut ps);
            for (i, (r, g, b)) in SWATCHES.iter().enumerate() {
                let brush = CreateSolidBrush(COLORREF(*r as u32 | (*g as u32) << 8 | (*b as u32) << 16));
                let rc = RECT { left: i as i32 * SWATCH_SIDE, top: 0, right: (i as i32 + 1) * SWATCH_SIDE, bottom: SWATCH_SIDE };
                FillRect(dc, &rc, brush);
                let _ = DeleteObject(brush.into());
            }
            let _ = EndPaint(hwnd, &ps);
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, wp, lp)
    }

    pub fn show(monitors: &[Monitor]) -> (Vec<Rect>, Box<dyn FnOnce()>) {
        let rects: Vec<Rect> = monitors
            .iter()
            .map(|m| {
                let w = SWATCH_SIDE * SWATCHES.len() as i32;
                Rect::new(m.x + (m.width as i32 - w) / 2, m.y + (m.height as i32 - SWATCH_SIDE) / 2, w, SWATCH_SIDE)
            })
            .collect();
        let (tx, rx) = mpsc::channel();
        let places = rects.clone();
        std::thread::spawn(move || unsafe {
            let inst = HINSTANCE(GetModuleHandleW(None).map(|m| m.0).unwrap_or_default());
            let class = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(proc_),
                hInstance: inst,
                lpszClassName: w!("YapSnipSwatches"),
                ..Default::default()
            };
            RegisterClassExW(&class);
            let mut msg = MSG::default();
            let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
            for r in &places {
                if let Ok(hwnd) = CreateWindowExW(
                    WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT | WS_EX_LAYERED,
                    w!("YapSnipSwatches"),
                    w!("Yap swatches"),
                    WS_POPUP,
                    r.x,
                    r.y,
                    r.w,
                    r.h,
                    None,
                    None,
                    Some(inst),
                    None,
                ) {
                    let _ = windows::Win32::UI::WindowsAndMessaging::SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
                    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                    let _ = windows::Win32::Graphics::Gdi::UpdateWindow(hwnd);
                }
            }
            let _ = tx.send(GetCurrentThreadId());
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
        let thread = rx.recv().unwrap_or(0);
        // Let DWM compose them.
        std::thread::sleep(std::time::Duration::from_millis(250));
        let close = Box::new(move || unsafe {
            let _ = PostThreadMessageW(thread, WM_QUIT, WPARAM(0), LPARAM(0));
        });
        (rects, close)
    }
}

/// The mean colour of swatch `i`'s middle in a frame of monitor `m`.
fn swatch_mean(f: &Frame, at: &Rect, i: usize) -> [u32; 3] {
    let m = &f.monitor;
    let r = Rect::new(at.x - m.x + i as i32 * SWATCH_SIDE + 30, at.y - m.y + 30, SWATCH_SIDE - 60, SWATCH_SIDE - 60);
    let px = crop_rgba(&f.bgra, f.width, &r.intersect(&Rect::new(0, 0, f.width as i32, f.height as i32)));
    let n = (px.len() / 4).max(1) as u32;
    let mut sum = [0u32; 3];
    for p in px.as_chunks::<4>().0 {
        for c in 0..3 {
            sum[c] += p[c] as u32;
        }
    }
    sum.map(|s| (s + n / 2) / n)
}

/// The swatch check on every monitor: each path's reading against the
/// colour painted, and the worst channel error.
#[cfg(windows)]
fn swatch_check(monitors: &[super::capture::Monitor]) -> serde_json::Value {
    let (rects, close) = swatch::show(monitors);
    let paths = [("auto", Force::Auto), ("wgc8", Force::Wgc8), ("gdi", Force::Gdi)];
    let mut out = Vec::new();
    let frames: Vec<_> = paths.iter().map(|(_, f)| capture::capture(monitors, *f).0).collect();
    close();
    for (mi, m) in monitors.iter().enumerate() {
        let mut entry = serde_json::json!({ "monitor": m.device, "hdr": m.hdr, "whiteNits": m.white_nits });
        for ((name, _), frames) in paths.iter().zip(&frames) {
            let Some(Ok(f)) = frames.get(mi) else { continue };
            let read: Vec<[u32; 3]> = (0..SWATCHES.len()).map(|i| swatch_mean(f, &rects[mi], i)).collect();
            let worst = read
                .iter()
                .zip(SWATCHES)
                .flat_map(|(got, (r, g, b))| [got[0] as i32 - r as i32, got[1] as i32 - g as i32, got[2] as i32 - b as i32])
                .map(i32::abs)
                .max()
                .unwrap_or(0);
            entry[*name] = serde_json::json!({ "read": read, "worstError": worst, "method": f.method });
        }
        out.push(entry);
    }
    serde_json::json!({ "painted": SWATCHES.iter().map(|(r, g, b)| [*r, *g, *b]).collect::<Vec<_>>(), "monitors": out })
}

#[cfg(not(windows))]
fn swatch_check(_monitors: &[super::capture::Monitor]) -> serde_json::Value {
    serde_json::Value::Null
}

pub fn main(args: Vec<String>) -> i32 {
    if !cfg!(debug_assertions) {
        eprintln!("snip-dump is in debug builds only");
        return 2;
    }
    per_monitor_dpi();
    let mut out = PathBuf::from("snip-dump");
    let mut overlay_runs = 3usize;
    let mut demo = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => out = it.next().map(PathBuf::from).unwrap_or(out),
            "--no-overlay" => overlay_runs = 0,
            "--demo" => demo = true,
            "--runs" => overlay_runs = it.next().and_then(|n| n.parse().ok()).unwrap_or(overlay_runs),
            other => {
                eprintln!("unknown argument {other}");
                return 2;
            }
        }
    }
    if let Err(e) = std::fs::create_dir_all(&out) {
        eprintln!("couldn't make {} ({e})", out.display());
        return 1;
    }

    let t = Instant::now();
    let monitors = capture::monitors();
    let monitors_ms = ms(t);
    eprintln!("{} monitor(s) in {monitors_ms} ms", monitors.len());
    for m in &monitors {
        eprintln!(
            "  {} {}x{} at ({}, {}) scale {} primary {} HDR {} SDR white {} nits",
            m.device, m.width, m.height, m.x, m.y, m.scale, m.primary, m.hdr, m.white_nits
        );
    }

    // Cold (the Direct3D device made), then warm, then the other paths.
    let (cold, cold_t) = capture::capture(&monitors, Force::Auto);
    let mut warm = Vec::new();
    let mut last = cold;
    for _ in 0..3 {
        let (frames, t) = capture::capture(&monitors, Force::Auto);
        warm.push(t.total_ms);
        last = frames;
    }
    let (wgc8, wgc8_t) = capture::capture(&monitors, Force::Wgc8);
    let (gdi, gdi_t) = capture::capture(&monitors, Force::Gdi);

    let mut per_monitor = Vec::new();
    for (i, m) in monitors.iter().enumerate() {
        let mut entry = serde_json::json!({ "monitor": m });
        for (name, frames) in [("auto", &last), ("wgc8", &wgc8), ("gdi", &gdi)] {
            match &frames[i] {
                Ok(f) => {
                    let t = Instant::now();
                    let rgba = crop_rgba(&f.bgra, f.width, &Rect::new(0, 0, f.width as i32, f.height as i32));
                    let crop_ms = ms(t);
                    let t = Instant::now();
                    let fast = encode_png(&rgba, f.width, f.height, true);
                    let fast_ms = ms(t);
                    let file = out.join(format!("monitor-{i}-{name}.png"));
                    let bytes = fast.as_ref().map(|p| p.len()).unwrap_or(0);
                    if let Ok(png) = &fast {
                        let _ = std::fs::write(&file, png);
                    }
                    entry[name] = serde_json::json!({
                        "method": f.method,
                        "size": [f.width, f.height],
                        "arrivedMs": (f.arrived_ms * 10.0).round() / 10.0,
                        "captureMs": (f.capture_ms * 10.0).round() / 10.0,
                        "convertMs": (f.convert_ms * 10.0).round() / 10.0,
                        "toRgbaMs": crop_ms,
                        "pngFastMs": fast_ms,
                        "pngBytes": bytes,
                        "centreMean": centre_mean(f),
                        "file": file.display().to_string(),
                    });
                }
                Err(e) => entry[name] = serde_json::json!({ "error": e }),
            }
        }
        per_monitor.push(entry);
    }

    // The overlay over the frozen pictures, as a snip shows it, timed from
    // the "shortcut" (the capture's start) to on screen. Click-through and
    // never focused here.
    let mut overlay_runs_out = Vec::new();
    for run in 0..overlay_runs {
        let t0 = Instant::now();
        let (frames, ct) = capture::capture(&monitors, Force::Auto);
        let screens: Vec<overlay::Screen> = frames
            .into_iter()
            .filter_map(Result::ok)
            .map(|f| overlay::Screen { monitor: f.monitor.clone(), width: f.width, height: f.height, bgra: std::sync::Arc::new(f.bgra) })
            .collect();
        let captured_ms = ms(t0);
        let opts = overlay::Options { focus: 0, interactive: false, hidden: false, ..Default::default() };
        let shown = overlay::open(screens, opts, Box::new(|_| {}));
        let total_ms = ms(t0);
        std::thread::sleep(std::time::Duration::from_millis(300));
        overlay::close();
        std::thread::sleep(std::time::Duration::from_millis(150));
        match shown {
            Ok(s) => overlay_runs_out.push(serde_json::json!({
                "run": run,
                "captureMs": captured_ms,
                "captureDeviceMs": ct.device_ms,
                "overlaySetupMs": (s.setup_ms * 10.0).round() / 10.0,
                "overlayVisibleMs": (s.visible_ms * 10.0).round() / 10.0,
                "shortcutToVisibleMs": total_ms,
            })),
            Err(e) => overlay_runs_out.push(serde_json::json!({ "run": run, "error": e })),
        }
    }

    // `--demo`: how the overlay looks with a box drawn on it (posted window
    // messages, never the mouse), captured off the screen into the folder.
    if demo {
        let (frames, _) = capture::capture(&monitors, Force::Auto);
        let screens: Vec<overlay::Screen> = frames
            .into_iter()
            .filter_map(Result::ok)
            .map(|f| overlay::Screen { monitor: f.monitor.clone(), width: f.width, height: f.height, bgra: std::sync::Arc::new(f.bgra) })
            .collect();
        let (w, h) = screens.first().map(|s| (s.width as i32, s.height as i32)).unwrap_or((0, 0));
        let opts = overlay::Options { focus: 0, interactive: false, hidden: false, ..Default::default() };
        if overlay::open(screens, opts, Box::new(|_| {})).is_ok() {
            let lp = |x: i32, y: i32| ((y as u16 as isize) << 16) | (x as u16 as isize);
            let _ = overlay::post_input(0, vec![(0x0201, 1, lp(w * 3 / 10, h * 3 / 10)), (0x0200, 1, lp(w * 11 / 20, h * 11 / 20))]);
            std::thread::sleep(std::time::Duration::from_millis(400));
            let (shot, _) = capture::capture(&monitors[..1], Force::Wgc8);
            if let Some(Ok(f)) = shot.first() {
                let rgba = crop_rgba(&f.bgra, f.width, &Rect::new(0, 0, f.width as i32, f.height as i32));
                if let Ok(png) = encode_png(&rgba, f.width, f.height, true) {
                    let _ = std::fs::write(out.join("overlay-demo.png"), png);
                }
            }
            overlay::close();
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
    }

    // The HDR check: known swatches painted by a plain SDR window.
    let swatches = if overlay_runs > 0 { swatch_check(&monitors) } else { serde_json::Value::Null };

    let summary = serde_json::json!({
        "build": if cfg!(debug_assertions) && cfg!(not(feature = "custom-protocol")) { "debug" } else { "optimised" },
        "swatches": swatches,
        "monitorsMs": monitors_ms,
        "capture": {
            "coldMs": cold_t.total_ms,
            "coldDeviceMs": cold_t.device_ms,
            "warmMedianMs": median(warm.clone()),
            "warmMs": warm,
            "wgc8Ms": wgc8_t.total_ms,
            "gdiMs": gdi_t.total_ms,
            "borderless": cold_t.borderless,
        },
        "monitors": per_monitor,
        "overlay": overlay_runs_out,
    });
    let text = serde_json::to_string_pretty(&summary).unwrap_or_default();
    let _ = std::fs::write(out.join("summary.json"), &text);
    println!("{text}");
    0
}

//! **A screenshot of one display, the apps not allowed masked** (the
//! desktop contract's `screenshot`): the display captured as Snip captures
//! it (`snip/capture.rs`: every Yap window left out for the moment of the
//! capture, HDR right), then every window that isn't one of the allowed
//! apps' painted over — its own grey with an edge, so where it is still
//! shows, never what's in it — and Yap's own windows the same, so the model
//! never sees through them to click what's behind. The desktop and the
//! taskbar are File Explorer's.
//!
//! [`mask`] is pure: each window's visible part is its frame less every
//! window above it, so an allowed window over a masked one shows whole and
//! a masked one over an allowed one hides what it covers.
//!
//! Where the pictures come from ([`Desk`]): this PC, or — a test run, a
//! unit test — the stand-in desktop (`super::sim`), never the real screen.

use serde::Serialize;

use super::frame::Frame;
use super::windows::Win;
use crate::snip::geom::Rect;

/// What's behind every window not allowed (and anywhere no window is).
const BACKGROUND: [u8; 4] = [30, 30, 34, 255];
/// A window not allowed: its fill and its edge.
const MASK_FILL: [u8; 4] = [62, 62, 68, 255];
const MASK_EDGE: [u8; 4] = [104, 104, 112, 255];
/// One of Yap's own windows.
const YAP_FILL: [u8; 4] = [58, 52, 74, 255];
const YAP_EDGE: [u8; 4] = [118, 100, 150, 255];
/// The edge's width, in the display's pixels.
const EDGE: i32 = 3;

/// How a window is painted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paint {
    /// One of the allowed apps': as it is.
    Keep,
    /// Not allowed: masked.
    Mask,
    /// Yap's own: masked, in Yap's colour.
    Yap,
}

/// `a` less `b`: up to four rectangles. Pure.
fn subtract(a: Rect, b: &Rect) -> Vec<Rect> {
    let i = a.intersect(b);
    if i.is_empty() {
        return vec![a];
    }
    let mut out = Vec::with_capacity(4);
    if i.y > a.y {
        out.push(Rect::new(a.x, a.y, a.w, i.y - a.y));
    }
    if i.bottom() < a.bottom() {
        out.push(Rect::new(a.x, i.bottom(), a.w, a.bottom() - i.bottom()));
    }
    if i.x > a.x {
        out.push(Rect::new(a.x, i.y, i.x - a.x, i.h));
    }
    if i.right() < a.right() {
        out.push(Rect::new(i.right(), i.y, a.right() - i.right(), i.h));
    }
    out
}

/// **Each window's visible part on `display`** (the windows topmost
/// first): its frame on the display, less every window above it. Pure.
pub fn pieces(display: Rect, wins: &[(Rect, Paint)]) -> Vec<Vec<Rect>> {
    let mut out = Vec::with_capacity(wins.len());
    for (i, (rect, _)) in wins.iter().enumerate() {
        let mut parts = vec![rect.intersect(&display)];
        parts.retain(|r| !r.is_empty());
        for (above, _) in &wins[..i] {
            if parts.is_empty() {
                break;
            }
            parts = parts.into_iter().flat_map(|p| subtract(p, above)).collect();
        }
        out.push(parts);
    }
    out
}

fn fill(rgba: &mut [u8], width: u32, r: &Rect, colour: [u8; 4]) {
    for y in r.y..r.bottom() {
        let row = (y as usize * width as usize + r.x as usize) * 4;
        for px in rgba[row..row + r.w as usize * 4].as_chunks_mut::<4>().0 {
            *px = colour;
        }
    }
}

/// **The display's capture (`src`, RGBA, the display's own pixels) with
/// everything but the allowed apps' windows masked** (the windows topmost
/// first, in the virtual desktop's pixels). Pure.
pub fn mask(src: &[u8], width: u32, height: u32, display: Rect, wins: &[(Rect, Paint)]) -> Vec<u8> {
    let mut out = vec![0u8; src.len()];
    for px in out.as_chunks_mut::<4>().0 {
        *px = BACKGROUND;
    }
    let local = |r: &Rect| Rect::new(r.x - display.x, r.y - display.y, r.w, r.h).intersect(&Rect::new(0, 0, width as i32, height as i32));
    for ((rect, paint), parts) in wins.iter().zip(pieces(display, wins)) {
        let window = local(rect);
        for part in parts.iter().map(local).filter(|p| !p.is_empty()) {
            match paint {
                Paint::Keep => {
                    for y in part.y..part.bottom() {
                        let at = (y as usize * width as usize + part.x as usize) * 4;
                        out[at..at + part.w as usize * 4].copy_from_slice(&src[at..at + part.w as usize * 4]);
                    }
                }
                Paint::Mask | Paint::Yap => {
                    let (body, edge) = if *paint == Paint::Yap { (YAP_FILL, YAP_EDGE) } else { (MASK_FILL, MASK_EDGE) };
                    fill(&mut out, width, &part, body);
                    // Its edge, where the window shows: where it is, at a glance.
                    let strips = [
                        Rect::new(window.x, window.y, window.w, EDGE),
                        Rect::new(window.x, window.bottom() - EDGE, window.w, EDGE),
                        Rect::new(window.x, window.y, EDGE, window.h),
                        Rect::new(window.right() - EDGE, window.y, EDGE, window.h),
                    ];
                    for s in strips {
                        let e = s.intersect(&part);
                        if !e.is_empty() {
                            fill(&mut out, width, &e, edge);
                        }
                    }
                }
            }
        }
    }
    out
}

/// **One display**, as the model names it ("Display 2").
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Display {
    pub name: String,
    /// Physical pixels on the virtual desktop.
    pub rect: Rect,
    pub primary: bool,
    /// What captures it (this PC's monitor; a stand-in's has a handle of 0).
    #[serde(skip)]
    pub monitor: crate::snip::capture::Monitor,
}

/// A stand-in's monitor: `rect` at 100 %, never captured from the screen.
#[cfg(any(test, debug_assertions))]
pub fn stand_in_monitor(rect: Rect, primary: bool, scale: f32) -> crate::snip::capture::Monitor {
    crate::snip::capture::Monitor { handle: 0, device: String::new(), x: rect.x, y: rect.y, width: rect.w as u32, height: rect.h as u32, scale, primary, hdr: false, white_nits: 80.0 }
}

/// A display's name from its device ("\\.\DISPLAY2" → "Display 2"), else
/// its place in the list (from 1). Pure.
pub fn display_name(device: &str, n: usize) -> String {
    let digits: String = device.chars().rev().take_while(char::is_ascii_digit).collect::<Vec<_>>().into_iter().rev().collect();
    if device.to_uppercase().contains("DISPLAY") && !digits.is_empty() {
        format!("Display {digits}")
    } else {
        format!("Display {n}")
    }
}

/// **The display a screenshot takes**: the one named (`switch_display`),
/// else the one holding most of the allowed apps' windows, else the one
/// with the pointer, else the primary. Pure.
pub fn choose(displays: &[Display], chosen: Option<&str>, allowed: &[Rect], cursor: Option<(i32, i32)>) -> Option<usize> {
    if displays.is_empty() {
        return None;
    }
    if let Some(name) = chosen {
        if let Some(i) = displays.iter().position(|d| d.name.eq_ignore_ascii_case(name)) {
            return Some(i);
        }
    }
    let area = |d: &Display| allowed.iter().map(|r| r.intersect(&d.rect)).map(|r| r.w as i64 * r.h as i64).sum::<i64>();
    let best = displays.iter().enumerate().max_by_key(|(i, d)| (area(d), std::cmp::Reverse(*i))).filter(|(_, d)| area(d) > 0);
    if let Some((i, _)) = best {
        return Some(i);
    }
    if let Some((x, y)) = cursor {
        if let Some(i) = displays.iter().position(|d| x >= d.rect.x && y >= d.rect.y && x < d.rect.right() && y < d.rect.bottom()) {
            return Some(i);
        }
    }
    Some(displays.iter().position(|d| d.primary).unwrap_or(0))
}

/// **Where the pictures come from**: this PC, or a stand-in.
pub trait Desk: Send + Sync {
    fn displays(&self) -> Vec<Display>;
    /// Every window on screen, topmost first.
    fn windows(&self) -> Vec<Win>;
    /// One display's pixels, RGBA, its own size.
    fn capture(&self, d: &Display) -> Result<(Vec<u8>, u32, u32), String>;
    fn cursor(&self) -> Option<(i32, i32)>;
}

/// **This PC** (Windows): its monitors, its windows, a capture with Yap's
/// windows left out for the moment of it.
pub struct ThisPc;

impl Desk for ThisPc {
    fn displays(&self) -> Vec<Display> {
        crate::snip::capture::monitors()
            .into_iter()
            .enumerate()
            .map(|(i, m)| Display { name: display_name(&m.device, i + 1), rect: Rect::new(m.x, m.y, m.width as i32, m.height as i32), primary: m.primary, monitor: m })
            .collect()
    }

    fn windows(&self) -> Vec<Win> {
        super::windows::on_screen()
    }

    fn capture(&self, d: &Display) -> Result<(Vec<u8>, u32, u32), String> {
        let app = crate::chat::pages::view::app().ok_or("Yap isn't ready to capture the screen")?;
        crate::calls::capture::for_snip(&app, true);
        let (results, _) = crate::snip::capture::capture(std::slice::from_ref(&d.monitor), crate::snip::capture::Force::Auto);
        crate::calls::capture::for_snip(&app, false);
        let frame = results.into_iter().next().ok_or("Windows didn't capture the display")??;
        crate::snip::cut_frames(&[frame], &d.rect)
    }

    fn cursor(&self) -> Option<(i32, i32)> {
        super::windows::cursor()
    }
}

/// **The desk for now**: a stand-in in a test run (its scene, never the
/// real screen), else this PC.
pub fn desk() -> Result<Box<dyn Desk>, String> {
    #[cfg(debug_assertions)]
    {
        if let Some(scene) = super::sim::scene() {
            return Ok(Box::new(scene));
        }
        if crate::shell::e2e::active() && !crate::shell::realtest::on() {
            return Err("this test run set no stand-in desktop (e2e_computer_scene)".into());
        }
    }
    Ok(Box::new(ThisPc))
}

/// A window masked in a screenshot, in its pixels.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Masked {
    #[serde(flatten)]
    pub rect: Rect,
    /// "not allowed" or "Yap".
    pub what: &'static str,
}

/// **A screenshot taken**: the frame its coordinates are in, the picture,
/// what's masked in it, the display and the others.
pub struct Shot {
    pub frame: Frame,
    /// The display's own pixels, masked (what `zoom` crops).
    pub full: Vec<u8>,
    pub display: Display,
    pub others: Vec<Display>,
    pub masked: Vec<Masked>,
    /// How many windows of the allowed apps are in it.
    pub shown: usize,
}

/// **Takes a screenshot** of the display `chosen` (else automatic) with
/// every window not of `owner`'s apps masked, its frame at most `edge`
/// pixels long (blocking).
pub fn take(desk: &dyn Desk, chosen: Option<&str>, owner: &dyn Fn(&Win) -> bool, edge: u32) -> Result<Shot, String> {
    let displays = desk.displays();
    let wins = desk.windows();
    let allowed: Vec<Rect> = wins.iter().filter(|w| owner(w)).map(|w| w.rect).collect();
    let i = choose(&displays, chosen, &allowed, desk.cursor()).ok_or("no display found")?;
    let display = displays[i].clone();
    let others: Vec<Display> = displays.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, d)| d.clone()).collect();
    let (rgba, w, h) = desk.capture(&display)?;
    if (w as i32, h as i32) != (display.rect.w, display.rect.h) || rgba.len() != (w * h * 4) as usize {
        return Err(format!("the capture came back {w}×{h}, not the display's size"));
    }
    let painted: Vec<(Rect, Paint)> = wins
        .iter()
        .map(|win| {
            let paint = if win.own {
                Paint::Yap
            } else if owner(win) {
                Paint::Keep
            } else {
                Paint::Mask
            };
            (win.rect, paint)
        })
        .collect();
    let full = mask(&rgba, w, h, display.rect, &painted);
    let frame = Frame::fit(display.rect, edge);
    // What's masked, as the model sees it: each window's frame on the screenshot (only where it shows).
    let shows = pieces(display.rect, &painted);
    let mut masked = Vec::new();
    let mut shown = 0;
    for ((win, (_, paint)), parts) in wins.iter().zip(&painted).zip(&shows) {
        if parts.is_empty() {
            continue;
        }
        match paint {
            Paint::Keep => shown += 1,
            Paint::Mask | Paint::Yap => {
                if let Some(r) = frame.rect_in(&win.rect.intersect(&display.rect)) {
                    masked.push(Masked { rect: r, what: if *paint == Paint::Yap { "Yap" } else { "not allowed" } });
                }
            }
        }
    }
    Ok(Shot { frame, full, display, others, masked, shown })
}

/// **The picture the model gets**: the display's masked pixels at the
/// frame's size, or — `scale` under 1 — smaller still (its coordinates
/// stay the frame's). PNG; its size.
pub fn picture(full: &[u8], frame: &Frame, region: Option<Rect>, scale: f64) -> Result<(Vec<u8>, u32, u32), String> {
    let (dw, dh) = (frame.display.w as u32, frame.display.h as u32);
    let (src, sw, sh) = match region {
        None => (std::borrow::Cow::Borrowed(full), dw, dh),
        Some(r) => {
            let mut crop = Vec::with_capacity((r.w * r.h * 4) as usize);
            for y in r.y..r.bottom() {
                let at = (y as usize * dw as usize + r.x as usize) * 4;
                crop.extend_from_slice(&full[at..at + r.w as usize * 4]);
            }
            (std::borrow::Cow::Owned(crop), r.w as u32, r.h as u32)
        }
    };
    // A whole display at the frame's size; a zoom at most the frame's long side.
    let (fw, fh) = if region.is_none() { (frame.w, frame.h) } else { crate::snip::images::fit(sw, sh, frame.w.max(frame.h)) };
    let s = if scale.is_finite() { scale.clamp(0.1, 1.0) } else { 1.0 };
    let (w, h) = (((fw as f64 * s).round() as u32).max(1), ((fh as f64 * s).round() as u32).max(1));
    let small = crate::snip::images::shrink(&src, sw, sh, w, h);
    let png = crate::snip::images::encode_png(&small, w, h, true)?;
    Ok((png, w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(rgba: &[u8], width: u32, x: i32, y: i32) -> [u8; 4] {
        let i = (y as usize * width as usize + x as usize) * 4;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    }

    /// An allowed window under a masked one is hidden where it's covered;
    /// an allowed one on top shows whole; Yap's own are masked in Yap's
    /// colour; nowhere else shows (the desktop wasn't allowed).
    #[test]
    fn only_the_allowed_apps_windows_show_and_only_where_nothing_covers_them() {
        let display = Rect::new(-100, 0, 100, 60);
        let src = vec![200u8; 100 * 60 * 4];
        let wins = [
            (Rect::new(-90, 5, 30, 20), Paint::Keep),  // allowed, on top
            (Rect::new(-80, 10, 40, 40), Paint::Mask), // not allowed, under it, over the next
            (Rect::new(-50, 20, 40, 30), Paint::Keep), // allowed, half covered
            (Rect::new(-20, 40, 15, 15), Paint::Yap),  // Yap's
        ];
        let out = mask(&src, 100, 60, display, &wins);
        let p = |x: i32, y: i32| at(&out, 100, x + 100, y);
        assert_eq!(p(-85, 10), [200; 4], "the top window whole");
        assert_eq!(p(-55, 20)[0], MASK_FILL[0], "the masked one's body");
        assert_eq!(p(-79, 49), MASK_EDGE, "its edge");
        assert_eq!(p(-30, 30), [200; 4], "the part nothing covers");
        assert_eq!(p(-45, 30)[0], MASK_FILL[0], "the part under the masked one");
        assert_eq!(p(-12, 50)[0], YAP_FILL[0], "Yap's own");
        assert_eq!(p(-99, 59), BACKGROUND, "no window: the background");
        let parts = pieces(display, &wins);
        assert_eq!(parts[0], vec![Rect::new(-90, 5, 30, 20)]);
        assert!(parts[2].iter().all(|r| r.x >= -40 || r.y >= 50), "{:?}", parts[2]);
    }

    #[test]
    fn subtracting_leaves_what_isnt_covered() {
        let a = Rect::new(0, 0, 10, 10);
        assert_eq!(subtract(a, &Rect::new(20, 20, 5, 5)), vec![a]);
        assert!(subtract(a, &Rect::new(-5, -5, 30, 30)).is_empty());
        let left: i32 = subtract(a, &Rect::new(3, 3, 4, 4)).iter().map(|r| r.w * r.h).sum();
        assert_eq!(left, 100 - 16);
    }

    fn display(name: &str, rect: Rect, primary: bool) -> Display {
        Display { name: name.into(), rect, primary, monitor: stand_in_monitor(rect, primary, 1.0) }
    }

    #[test]
    fn the_display_is_the_one_named_else_the_one_with_the_apps() {
        let ds = [display("Display 1", Rect::new(0, 0, 1920, 1080), true), display("Display 2", Rect::new(1920, 0, 2560, 1440), false)];
        assert_eq!(choose(&ds, Some("display 2"), &[], None), Some(1));
        assert_eq!(choose(&ds, Some("Display 9"), &[], None), Some(0), "an unknown name: automatic");
        assert_eq!(choose(&ds, None, &[Rect::new(2000, 100, 800, 600)], Some((10, 10))), Some(1));
        assert_eq!(choose(&ds, None, &[], Some((2500, 10))), Some(1), "the pointer's");
        assert_eq!(choose(&ds, None, &[], None), Some(0), "the primary");
        assert_eq!(choose(&[], None, &[], None), None);
        assert_eq!(display_name(r"\\.\DISPLAY12", 1), "Display 12");
        assert_eq!(display_name("", 3), "Display 3");
    }

    #[test]
    fn a_picture_is_the_frames_size_or_smaller_and_a_zoom_is_sharper() {
        let frame = Frame::fit(Rect::new(0, 0, 400, 200), 200);
        let full = vec![128u8; 400 * 200 * 4];
        let (png, w, h) = picture(&full, &frame, None, 1.0).unwrap();
        assert_eq!((w, h), (200, 100));
        assert_eq!(crate::snip::images::png_size(&png), Some((200, 100)));
        assert_eq!(picture(&full, &frame, None, 0.5).unwrap().1, 100);
        // A zoom on 100 × 50 of the display: shown at its own pixels (under the frame's side).
        let (_, zw, zh) = picture(&full, &frame, Some(Rect::new(10, 10, 100, 50)), 1.0).unwrap();
        assert_eq!((zw, zh), (100, 50));
    }
}

//! **Where a model's coordinates land** (pure): the screenshot a model sees
//! of one display, and the display itself in the PC's physical pixels (the
//! virtual desktop's: a display left of or above the primary one starts at
//! negative coordinates). One frame per display for a session, so every
//! point the model gives maps back the same way however many screenshots it
//! takes; a `zoom` crops the display, never moves the frame.

use serde::Serialize;

use crate::snip::geom::Rect;

/// **One display as the model sees it**: the display's rectangle in
/// physical pixels and the size of the screenshot it gets of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Frame {
    pub display: Rect,
    pub w: u32,
    pub h: u32,
}

impl Frame {
    /// The frame for `display` shown at most `edge` pixels on its long side
    /// (never bigger than the display itself).
    pub fn fit(display: Rect, edge: u32) -> Frame {
        let (w, h) = crate::snip::images::fit(display.w.max(1) as u32, display.h.max(1) as u32, edge);
        Frame { display, w, h }
    }

    /// Physical pixels per screenshot pixel, across and down.
    fn scale(self) -> (f64, f64) {
        (self.display.w as f64 / self.w.max(1) as f64, self.display.h as f64 / self.h.max(1) as f64)
    }

    /// **A point in the screenshot → a physical pixel**, the middle of the
    /// pixels it stands for. `None`: outside the screenshot.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn on_screen(self, x: f64, y: f64) -> Option<(i32, i32)> {
        if !(x.is_finite() && y.is_finite()) || x < 0.0 || y < 0.0 || x >= self.w as f64 || y >= self.h as f64 {
            return None;
        }
        let (sx, sy) = self.scale();
        let px = ((x.floor() + 0.5) * sx).floor() as i32;
        let py = ((y.floor() + 0.5) * sy).floor() as i32;
        Some((self.display.x + px.clamp(0, self.display.w - 1), self.display.y + py.clamp(0, self.display.h - 1)))
    }

    /// **A physical pixel → the screenshot's** (`cursor_position`). `None`:
    /// not on this display.
    pub fn on_shot(self, x: i32, y: i32) -> Option<(u32, u32)> {
        if x < self.display.x || y < self.display.y || x >= self.display.right() || y >= self.display.bottom() {
            return None;
        }
        let (sx, sy) = self.scale();
        let fx = (((x - self.display.x) as f64) / sx).floor() as u32;
        let fy = (((y - self.display.y) as f64) / sy).floor() as u32;
        Some((fx.min(self.w - 1), fy.min(self.h - 1)))
    }

    /// **A region of the screenshot** (`[x0, y0, x1, y1]`, either corner
    /// first) **→ the display's pixels it covers**, relative to the
    /// display's own top-left (a crop of its capture), clamped to it.
    /// `None`: empty, or not on the screenshot at all.
    pub fn region(self, r: [f64; 4]) -> Option<Rect> {
        if r.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let (x0, x1) = (r[0].min(r[2]).max(0.0), r[0].max(r[2]).min(self.w as f64));
        let (y0, y1) = (r[1].min(r[3]).max(0.0), r[1].max(r[3]).min(self.h as f64));
        if x1 - x0 < 1.0 || y1 - y0 < 1.0 {
            return None;
        }
        let (sx, sy) = self.scale();
        let (l, t) = ((x0 * sx).floor() as i32, (y0 * sy).floor() as i32);
        let (rr, b) = (((x1 * sx).ceil() as i32).min(self.display.w), ((y1 * sy).ceil() as i32).min(self.display.h));
        (rr > l && b > t).then(|| Rect::new(l, t, rr - l, b - t))
    }

    /// `r` (physical pixels, the virtual desktop's) in the screenshot's
    /// pixels, clipped to it. `None`: not on this display.
    pub fn rect_in(self, r: &Rect) -> Option<Rect> {
        let on = r.intersect(&self.display);
        if on.is_empty() {
            return None;
        }
        let (sx, sy) = self.scale();
        let l = ((on.x - self.display.x) as f64 / sx).floor() as i32;
        let t = ((on.y - self.display.y) as f64 / sy).floor() as i32;
        let rr = ((on.right() - self.display.x) as f64 / sx).ceil() as i32;
        let b = ((on.bottom() - self.display.y) as f64 / sy).ceil() as i32;
        let (rr, b) = (rr.min(self.w as i32), b.min(self.h as i32));
        (rr > l && b > t).then(|| Rect::new(l, t, rr - l, b - t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4K display shown at 1,920 px, and one at 150 % left of it (negative
    /// coordinates): every point lands in the middle of the pixels it
    /// stands for, and comes back to itself.
    #[test]
    fn a_point_on_the_screenshot_lands_on_the_pixels_it_stands_for() {
        let big = Frame::fit(Rect::new(0, 0, 3840, 2160), 1920);
        assert_eq!((big.w, big.h), (1920, 1080));
        assert_eq!(big.on_screen(0.0, 0.0), Some((1, 1)));
        assert_eq!(big.on_screen(1919.0, 1079.0), Some((3839, 2159)));
        assert_eq!(big.on_screen(960.4, 540.9), Some((1921, 1081)));
        assert_eq!(big.on_screen(1920.0, 10.0), None);
        assert_eq!(big.on_screen(-1.0, 10.0), None);
        assert_eq!(big.on_screen(f64::NAN, 10.0), None);
        let left = Frame::fit(Rect::new(-2560, -200, 2560, 1440), 1920);
        assert_eq!((left.w, left.h), (1920, 1080));
        let (x, y) = left.on_screen(100.0, 50.0).unwrap();
        assert!(x < 0 && y < 0, "({x}, {y})");
        assert_eq!(left.on_shot(x, y), Some((100, 50)));
        assert_eq!(left.on_shot(0, 0), None, "on the other display");
        // A display smaller than the edge is shown as it is.
        let small = Frame::fit(Rect::new(1920, 0, 1366, 768), 1920);
        assert_eq!((small.w, small.h), (1366, 768));
        assert_eq!(small.on_screen(10.0, 10.0), Some((1930, 10)));
        assert_eq!(small.on_shot(1930, 10), Some((10, 10)));
    }

    /// A zoom's region is the display's pixels under it, either corner
    /// first, clamped; a window's rectangle is the screenshot's.
    #[test]
    fn regions_and_rectangles_map_both_ways() {
        let f = Frame::fit(Rect::new(-3840, 0, 3840, 2160), 1920);
        assert_eq!(f.region([100.0, 100.0, 300.0, 200.0]), Some(Rect::new(200, 200, 400, 200)));
        assert_eq!(f.region([300.0, 200.0, 100.0, 100.0]), Some(Rect::new(200, 200, 400, 200)));
        assert_eq!(f.region([1900.0, 1000.0, 5000.0, 5000.0]), Some(Rect::new(3800, 2000, 40, 160)));
        assert_eq!(f.region([10.0, 10.0, 10.5, 40.0]), None);
        assert_eq!(f.region([2000.0, 0.0, 2100.0, 10.0]), None);
        assert_eq!(f.rect_in(&Rect::new(-3840 + 200, 200, 400, 200)), Some(Rect::new(100, 100, 200, 100)));
        assert_eq!(f.rect_in(&Rect::new(-100, -100, 300, 300)), Some(Rect::new(1870, 0, 50, 100)));
        assert_eq!(f.rect_in(&Rect::new(0, 0, 300, 300)), None, "on another display");
    }
}

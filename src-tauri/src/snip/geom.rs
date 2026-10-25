//! The snip's geometry, pure: the box a drag makes, where its size label
//! goes, and the crop out of a monitor's frame — all in **physical pixels**
//! (Windows' own coordinates for a per-monitor-DPI-aware process), so a
//! box is exact at 100 %, 125 %, 150 %, 175 % and 200 %, on a monitor left
//! of or above the primary (negative coordinates) too.
//!
//! **Across monitors** (phase 2): a box is kept in virtual-desktop pixels
//! and clamped to the desktop's bounds ([`desktop`], [`drag_box_on`]); each
//! monitor's overlay draws its own part of it, and the picture is put
//! together from every monitor it touches ([`pieces`], [`compose_rgba`]) —
//! a part that's on no monitor (two monitors of different heights) is
//! black. A box on one monitor is still cut from that monitor's frame
//! alone, at the frame's own resolution ([`to_frame`], [`crop_rgba`]).

/// A rectangle: left, top, width, height.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Rect { x, y, w, h }
    }
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }
    /// The part of `self` inside `other` (empty when they don't meet).
    pub fn intersect(&self, other: &Rect) -> Rect {
        let (l, t) = (self.x.max(other.x), self.y.max(other.y));
        let (r, b) = (self.right().min(other.right()), self.bottom().min(other.bottom()));
        if r <= l || b <= t {
            Rect::default()
        } else {
            Rect::new(l, t, r - l, b - t)
        }
    }
    /// The smallest rect holding both (an empty one adds nothing).
    pub fn union(&self, other: &Rect) -> Rect {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        let (l, t) = (self.x.min(other.x), self.y.min(other.y));
        let (r, b) = (self.right().max(other.right()), self.bottom().max(other.bottom()));
        Rect::new(l, t, r - l, b - t)
    }
    pub fn inflate(&self, by: i32) -> Rect {
        Rect::new(self.x - by, self.y - by, self.w + 2 * by, self.h + 2 * by)
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
}

/// The smallest box a release snips: a plain click, or a twitch of the
/// mouse, sends nothing to a model.
pub const MIN_SIDE: i32 = 6;

/// The box a drag from `start` to `now` makes on a monitor `w`×`h` (window
/// coordinates), clamped to its edges: the pixels between the two points,
/// whichever way it was dragged. The point the drag reached counts as the
/// far edge (dragging from 10 to 20 takes 10 pixels).
pub fn drag_box(start: (i32, i32), now: (i32, i32), w: i32, h: i32) -> Rect {
    let clamp = |v: i32, max: i32| v.clamp(0, max.max(0));
    let (x0, y0) = (clamp(start.0, w), clamp(start.1, h));
    let (x1, y1) = (clamp(now.0, w), clamp(now.1, h));
    Rect::new(x0.min(x1), y0.min(y1), (x1 - x0).abs(), (y1 - y0).abs())
}

/// Whether a box is big enough to snip ([`MIN_SIDE`] each way).
pub fn big_enough(r: &Rect) -> bool {
    r.w >= MIN_SIDE && r.h >= MIN_SIDE
}

/// A monitor's box in its frame's pixels: the frame can differ from the
/// monitor's size (WGC's frame is the monitor's current mode; a fake
/// monitor in a test can be anything), so the box is scaled to it, rounded
/// outward, and clamped. Equal sizes: the box itself.
pub fn to_frame(r: &Rect, monitor: (u32, u32), frame: (u32, u32)) -> Rect {
    let (mw, mh) = (monitor.0.max(1) as f64, monitor.1.max(1) as f64);
    let (fw, fh) = (frame.0 as f64, frame.1 as f64);
    if (mw, mh) == (fw, fh) {
        return r.intersect(&Rect::new(0, 0, frame.0 as i32, frame.1 as i32));
    }
    let (sx, sy) = (fw / mw, fh / mh);
    let x0 = (r.x as f64 * sx).floor() as i32;
    let y0 = (r.y as f64 * sy).floor() as i32;
    let x1 = (r.right() as f64 * sx).ceil() as i32;
    let y1 = (r.bottom() as f64 * sy).ceil() as i32;
    Rect::new(x0, y0, x1 - x0, y1 - y0).intersect(&Rect::new(0, 0, frame.0 as i32, frame.1 as i32))
}

/// A length in the overlay's own drawing (the box's edge, the label's gap
/// and padding), given at 100 %, in a monitor's physical pixels at its
/// `scale`: rounded, never under one pixel. So the overlay looks the same
/// on a 100 % and a 175 % monitor side by side. Pure.
pub fn scaled(logical: f32, scale: f32) -> i32 {
    let s = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    (logical * s).round().max(1.0) as i32
}

/// Copy `r` out of a BGRA frame (`width` pixels a row) as **RGBA** (what a
/// PNG holds). `r` must be inside the frame (see [`to_frame`]).
pub fn crop_rgba(bgra: &[u8], width: u32, r: &Rect) -> Vec<u8> {
    let (w, h) = (r.w.max(0) as usize, r.h.max(0) as usize);
    let stride = width as usize * 4;
    let mut out = Vec::with_capacity(w * h * 4);
    for row in 0..h {
        let start = (r.y as usize + row) * stride + r.x as usize * 4;
        for px in bgra[start..start + w * 4].as_chunks::<4>().0 {
            out.extend_from_slice(&[px[2], px[1], px[0], 255]);
        }
    }
    out
}

/// Where the size label ("812 × 430") goes for a box: just below its
/// bottom-right corner, right-aligned with it; above the box when there's
/// no room below; inside its bottom-right corner when there's no room
/// either side; always on the monitor (`w`×`h`). `label` = its size;
/// `gap` = the space between them.
pub fn label_at(sel: &Rect, label: (i32, i32), gap: i32, w: i32, h: i32) -> Rect {
    let (lw, lh) = label;
    let x = (sel.right() - lw).clamp(0, (w - lw).max(0));
    let below = sel.bottom() + gap;
    let above = sel.y - gap - lh;
    let y = if below + lh <= h {
        below
    } else if above >= 0 {
        above
    } else {
        (sel.bottom() - gap - lh).clamp(0, (h - lh).max(0))
    };
    Rect::new(x, y, lw, lh)
}

/// The virtual desktop's bounds: the smallest rect holding every monitor.
pub fn desktop(monitors: &[Rect]) -> Rect {
    monitors.iter().fold(Rect::default(), |acc, m| acc.union(m))
}

/// The box a drag from `start` to `now` makes in virtual-desktop pixels,
/// clamped to `bounds` (the desktop): a box can cross from one monitor
/// into another. As [`drag_box`], the point reached is the far edge.
pub fn drag_box_on(start: (i32, i32), now: (i32, i32), bounds: &Rect) -> Rect {
    let local = drag_box((start.0 - bounds.x, start.1 - bounds.y), (now.0 - bounds.x, now.1 - bounds.y), bounds.w, bounds.h);
    Rect::new(local.x + bounds.x, local.y + bounds.y, local.w, local.h)
}

/// Whether a press at `down` and a release at `up` are a click (the mouse
/// moved under [`MIN_SIDE`] pixels each way): the whole monitor, or the
/// window picked, rather than a box.
pub fn is_click(down: (i32, i32), up: (i32, i32)) -> bool {
    (up.0 - down.0).abs() < MIN_SIDE && (up.1 - down.1).abs() < MIN_SIDE
}

/// The topmost window under a point (`windows` top to bottom, virtual-
/// desktop pixels), by index.
pub fn window_at(windows: &[Rect], x: i32, y: i32) -> Option<usize> {
    windows.iter().position(|w| w.contains(x, y))
}

/// One monitor's part of a box: the monitor (index), the part in that
/// monitor's own pixels, and where it goes in the box's picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Piece {
    pub monitor: usize,
    pub local: Rect,
    pub at: (i32, i32),
}

/// A box's parts on each monitor it touches (virtual-desktop pixels in,
/// each monitor's own out). Pure.
pub fn pieces(sel: &Rect, monitors: &[Rect]) -> Vec<Piece> {
    monitors
        .iter()
        .enumerate()
        .filter_map(|(i, m)| {
            let part = sel.intersect(m);
            (!part.is_empty()).then(|| Piece {
                monitor: i,
                local: Rect::new(part.x - m.x, part.y - m.y, part.w, part.h),
                at: (part.x - sel.x, part.y - sel.y),
            })
        })
        .collect()
}

/// The one monitor a box lies on whole, if it does (its picture is then
/// cut from that monitor's frame alone).
pub fn on_one_monitor(sel: &Rect, monitors: &[Rect]) -> Option<usize> {
    monitors.iter().position(|m| m.intersect(sel) == *sel)
}

/// The monitor holding the most of a box (its size label goes there).
pub fn main_monitor(sel: &Rect, monitors: &[Rect]) -> Option<usize> {
    monitors
        .iter()
        .enumerate()
        .map(|(i, m)| (i, m.intersect(sel)))
        .filter(|(_, r)| !r.is_empty())
        .max_by_key(|(i, r)| (r.w as i64 * r.h as i64, -(*i as i64)))
        .map(|(i, _)| i)
}

/// A monitor's frozen picture for [`compose_rgba`]: BGRA rows of
/// `width` × `height` pixels, standing for the monitor at `place`
/// (virtual-desktop pixels; a frame of another size is scaled to it).
pub struct Source<'a> {
    pub bgra: &'a [u8],
    pub width: u32,
    pub height: u32,
    pub place: Rect,
}

/// A box's picture put together from every monitor it touches, as RGBA
/// (`sel.w` × `sel.h`, physical pixels): each monitor's part from its own
/// frame (scaled to the monitor's pixels when the frame's size differs —
/// nearest pixel), anything on no monitor black. Pure.
pub fn compose_rgba(sel: &Rect, sources: &[Source]) -> Vec<u8> {
    let (w, h) = (sel.w.max(0) as usize, sel.h.max(0) as usize);
    let mut out = vec![0u8; w * h * 4];
    for px in out.as_chunks_mut::<4>().0 {
        px[3] = 255;
    }
    let places: Vec<Rect> = sources.iter().map(|s| s.place).collect();
    for p in pieces(sel, &places) {
        let s = &sources[p.monitor];
        let (fw, fh) = (s.width as usize, s.height as usize);
        let (mw, mh) = (s.place.w.max(1) as usize, s.place.h.max(1) as usize);
        let same = (fw, fh) == (mw, mh);
        for row in 0..p.local.h as usize {
            let ly = p.local.y as usize + row;
            let fy = if same { ly } else { ((ly * 2 + 1) * fh / (mh * 2)).min(fh.saturating_sub(1)) };
            let dst_row = (p.at.1 as usize + row) * w;
            for col in 0..p.local.w as usize {
                let lx = p.local.x as usize + col;
                let fx = if same { lx } else { ((lx * 2 + 1) * fw / (mw * 2)).min(fw.saturating_sub(1)) };
                let si = (fy * fw + fx) * 4;
                let di = (dst_row + p.at.0 as usize + col) * 4;
                if let Some(px) = s.bgra.get(si..si + 4) {
                    out[di..di + 4].copy_from_slice(&[px[2], px[1], px[0], 255]);
                }
            }
        }
    }
    out
}

/// Which monitor a point (virtual-desktop pixels) is on, by index; the
/// nearest one when it's in a gap between them.
pub fn monitor_at(monitors: &[Rect], x: i32, y: i32) -> Option<usize> {
    if let Some(i) = monitors.iter().position(|m| m.contains(x, y)) {
        return Some(i);
    }
    monitors
        .iter()
        .enumerate()
        .min_by_key(|(_, m)| {
            let dx = (m.x - x).max(0).max(x - (m.right() - 1));
            let dy = (m.y - y).max(0).max(y - (m.bottom() - 1));
            (dx as i64).pow(2) + (dy as i64).pow(2)
        })
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_makes_its_box_whichever_way_it_goes() {
        assert_eq!(drag_box((10, 20), (110, 70), 1920, 1080), Rect::new(10, 20, 100, 50));
        assert_eq!(drag_box((110, 70), (10, 20), 1920, 1080), Rect::new(10, 20, 100, 50));
        assert_eq!(drag_box((110, 20), (10, 70), 1920, 1080), Rect::new(10, 20, 100, 50));
        // Past the monitor's edges (the mouse is captured): clamped to it.
        assert_eq!(drag_box((1800, 1000), (2500, 1300), 1920, 1080), Rect::new(1800, 1000, 120, 80));
        assert_eq!(drag_box((50, 50), (-40, -10), 1920, 1080), Rect::new(0, 0, 50, 50));
    }

    #[test]
    fn a_click_or_a_twitch_snips_nothing() {
        assert!(!big_enough(&drag_box((5, 5), (5, 5), 100, 100)));
        assert!(!big_enough(&drag_box((5, 5), (10, 40), 100, 100)));
        assert!(big_enough(&drag_box((5, 5), (11, 11), 100, 100)));
    }

    #[test]
    fn rects_meet_and_join() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, 5, 10, 10);
        assert_eq!(a.intersect(&b), Rect::new(5, 5, 5, 5));
        assert_eq!(a.union(&b), Rect::new(0, 0, 15, 15));
        assert!(a.intersect(&Rect::new(20, 20, 5, 5)).is_empty());
        assert_eq!(Rect::default().union(&b), b);
        assert_eq!(a.inflate(2), Rect::new(-2, -2, 14, 14));
    }

    /// Mixed DPI: every monitor's overlay works in its own physical pixels
    /// (a per-monitor-DPI-aware window placed at physical coordinates), so
    /// a box is its pixels at any scale — the whole monitor is the whole
    /// monitor at 100, 125, 150, 175 and 200 % — and the overlay's own
    /// lengths scale with the monitor.
    #[test]
    fn boxes_are_physical_pixels_at_every_scale() {
        for (scale, (w, h)) in [(1.0f32, (1920, 1080)), (1.25, (2560, 1440)), (1.5, (2880, 1620)), (1.75, (3360, 1890)), (2.0, (3840, 2160))] {
            let whole = drag_box((0, 0), (w, h), w, h);
            assert_eq!(whole, Rect::new(0, 0, w, h), "{scale}");
            assert_eq!(to_frame(&whole, (w as u32, h as u32), (w as u32, h as u32)), whole);
            let r = drag_box((101, 51), (301, 151), w, h);
            assert_eq!((r.w, r.h), (200, 100), "{scale}: a box keeps its pixels");
            // The edge (1.5 px at 100 %) and the label's 8 px gap.
            assert_eq!(scaled(1.5, scale), (1.5 * scale).round() as i32);
            assert_eq!(scaled(8.0, scale), (8.0 * scale).round() as i32);
        }
        assert_eq!(scaled(1.5, 1.0), 2);
        assert_eq!(scaled(0.2, 1.0), 1, "never under a pixel");
        assert_eq!(scaled(8.0, f32::NAN), 8);
        // A 150 % monitor left of a 100 % primary: the box is in the
        // window's own pixels; where the monitor is doesn't enter into it.
        let left = Rect::new(-2880, -200, 2880, 1620);
        assert_eq!(monitor_at(&[Rect::new(0, 0, 1920, 1080), left], -100, 100), Some(1));
        assert_eq!(drag_box((2700, 1500), (2900, 1700), left.w, left.h), Rect::new(2700, 1500, 180, 120));
    }

    #[test]
    fn a_box_scales_to_a_frame_of_another_size() {
        let r = Rect::new(100, 50, 200, 100);
        assert_eq!(to_frame(&r, (1920, 1080), (1920, 1080)), r);
        // A frame at twice the monitor's size (a test's fake at 200 %).
        assert_eq!(to_frame(&r, (1920, 1080), (3840, 2160)), Rect::new(200, 100, 400, 200));
        // Odd ratios round outward, and the box stays in the frame.
        assert_eq!(to_frame(&Rect::new(1, 1, 3, 3), (3, 3), (4, 4)), Rect::new(1, 1, 3, 3));
        assert_eq!(to_frame(&Rect::new(1900, 1060, 40, 40), (1920, 1080), (1920, 1080)), Rect::new(1900, 1060, 20, 20));
    }

    #[test]
    fn the_crop_is_rgba_from_bgra() {
        // 3×2 BGRA frame: pixel (x, y) = [x, y, 9, 255] as B, G, R, A.
        let mut f = Vec::new();
        for y in 0..2u8 {
            for x in 0..3u8 {
                f.extend_from_slice(&[x, y, 9, 255]);
            }
        }
        let out = crop_rgba(&f, 3, &Rect::new(1, 0, 2, 2));
        assert_eq!(out, vec![9, 0, 1, 255, 9, 0, 2, 255, 9, 1, 1, 255, 9, 1, 2, 255]);
    }

    #[test]
    fn the_size_label_finds_room() {
        let (w, h) = (1920, 1080);
        // Room below: under the bottom-right corner.
        assert_eq!(label_at(&Rect::new(100, 100, 300, 200), (80, 24), 8, w, h), Rect::new(320, 308, 80, 24));
        // At the bottom: above the box.
        assert_eq!(label_at(&Rect::new(100, 900, 300, 175), (80, 24), 8, w, h), Rect::new(320, 868, 80, 24));
        // The whole monitor: inside its bottom-right corner.
        assert_eq!(label_at(&Rect::new(0, 0, w, h), (80, 24), 8, w, h), Rect::new(1840, 1048, 80, 24));
        // A box at the left edge narrower than the label: kept on screen.
        assert_eq!(label_at(&Rect::new(0, 100, 20, 20), (80, 24), 8, w, h).x, 0);
    }

    /// Two monitors side by side, a 150 % one right of a 100 % primary and
    /// shorter than it (the e2e suite's fake pair).
    const PAIR: [Rect; 2] = [Rect::new(0, 0, 1280, 720), Rect::new(1280, 0, 960, 540)];

    #[test]
    fn a_box_crosses_from_one_monitor_into_another() {
        let desk = desktop(&PAIR);
        assert_eq!(desk, Rect::new(0, 0, 2240, 720));
        // Dragged from the primary into the second: one box over both.
        let sel = drag_box_on((1100, 150), (1580, 280), &desk);
        assert_eq!(sel, Rect::new(1100, 150, 480, 130));
        assert_eq!(on_one_monitor(&sel, &PAIR), None);
        assert_eq!(
            pieces(&sel, &PAIR),
            vec![
                Piece { monitor: 0, local: Rect::new(1100, 150, 180, 130), at: (0, 0) },
                Piece { monitor: 1, local: Rect::new(0, 150, 300, 130), at: (180, 0) },
            ]
        );
        assert_eq!(main_monitor(&sel, &PAIR), Some(1));
        // Back the other way, and past the desktop's edges: clamped to it.
        assert_eq!(drag_box_on((1580, 280), (-50, 900), &desk), Rect::new(0, 280, 1580, 440));
        // On one monitor: that monitor's.
        assert_eq!(on_one_monitor(&Rect::new(1300, 10, 50, 50), &PAIR), Some(1));
        // A monitor left of and above the primary (negative coordinates).
        let mixed = [Rect::new(0, 0, 2560, 1440), Rect::new(-1920, -200, 1920, 1080)];
        let desk = desktop(&mixed);
        assert_eq!(desk, Rect::new(-1920, -200, 4480, 1640));
        let sel = drag_box_on((-100, -150), (100, 100), &desk);
        assert_eq!(sel, Rect::new(-100, -150, 200, 250));
        let p = pieces(&sel, &mixed);
        assert_eq!(p[0], Piece { monitor: 0, local: Rect::new(0, 0, 100, 100), at: (100, 150) });
        assert_eq!(p[1], Piece { monitor: 1, local: Rect::new(1820, 50, 100, 250), at: (0, 0) });
    }

    #[test]
    fn a_click_is_a_press_that_barely_moves() {
        assert!(is_click((100, 100), (100, 100)));
        assert!(is_click((100, 100), (105, 95)));
        assert!(!is_click((100, 100), (106, 100)));
        assert!(!is_click((100, 100), (100, 130)), "a thin drag isn't a click");
    }

    #[test]
    fn the_window_under_the_pointer_is_the_topmost() {
        // Top to bottom: a small dialog over a big window.
        let windows = [Rect::new(300, 200, 400, 300), Rect::new(100, 100, 1000, 700)];
        assert_eq!(window_at(&windows, 350, 250), Some(0));
        assert_eq!(window_at(&windows, 150, 150), Some(1));
        assert_eq!(window_at(&windows, 1200, 50), None);
    }

    #[test]
    fn a_box_over_two_monitors_is_put_together_from_both() {
        // Each monitor's frame a flat colour (BGRA): the primary red, the
        // second blue; the second's frame at half its size (scaled up).
        let red: Vec<u8> = std::iter::repeat_n([0u8, 0, 200, 255], 1280 * 720).flatten().collect();
        let blue: Vec<u8> = std::iter::repeat_n([200u8, 0, 0, 255], 480 * 270).flatten().collect();
        let sources = [
            Source { bgra: &red, width: 1280, height: 720, place: PAIR[0] },
            Source { bgra: &blue, width: 480, height: 270, place: PAIR[1] },
        ];
        // From (1270, 530) to (1290, 550): ten pixels each side of the join,
        // the bottom-right quarter below the second monitor (on none).
        let sel = Rect::new(1270, 530, 20, 20);
        let out = compose_rgba(&sel, &sources);
        assert_eq!(out.len(), 20 * 20 * 4);
        let at = |x: usize, y: usize| out[(y * 20 + x) * 4..(y * 20 + x) * 4 + 4].to_vec();
        assert_eq!(at(0, 0), vec![200, 0, 0, 255], "the primary's red, as RGBA");
        assert_eq!(at(9, 19), vec![200, 0, 0, 255]);
        assert_eq!(at(10, 0), vec![0, 0, 200, 255], "the second's blue");
        assert_eq!(at(19, 9), vec![0, 0, 200, 255]);
        assert_eq!(at(15, 15), vec![0, 0, 0, 255], "on no monitor: black");
        // A frame of the monitor's own size is copied pixel for pixel.
        let ramp: Vec<u8> = (0..4u8).flat_map(|i| [i, i * 2, i * 3, 255]).collect();
        let one = [Source { bgra: &ramp, width: 4, height: 1, place: Rect::new(0, 0, 4, 1) }];
        assert_eq!(compose_rgba(&Rect::new(1, 0, 2, 1), &one), vec![3, 2, 1, 255, 6, 4, 2, 255]);
    }

    #[test]
    fn a_point_finds_its_monitor_even_left_of_the_primary() {
        let monitors = [Rect::new(0, 0, 2560, 1440), Rect::new(-1920, 200, 1920, 1080), Rect::new(2560, -300, 1080, 1920)];
        assert_eq!(monitor_at(&monitors, 10, 10), Some(0));
        assert_eq!(monitor_at(&monitors, -5, 500), Some(1));
        assert_eq!(monitor_at(&monitors, 3000, -100), Some(2));
        // In a gap (above the left monitor): the nearest.
        assert_eq!(monitor_at(&monitors, -100, 150), Some(1));
        assert_eq!(monitor_at(&[], 0, 0), None);
    }
}

//! HDR to SDR, done the way Windows composes it (pure, unit-tested).
//!
//! With HDR on, Windows composes the desktop in **scRGB**: 16-bit float,
//! linear light, where 1.0 is D65 white at 80 nits. Every SDR app (every
//! window, Yap's own WebView2 ones included) is linearised and **boosted to
//! the SDR white level** the person set ("SDR content brightness"), i.e.
//! multiplied by `white_nits / 80`. An 8-bit capture of that desktop just
//! clips at 1.0, so at 200 nits anything above 40 % linear (about sRGB
//! `#AAAAAA`) comes out white: the washed-out screenshots of an HDR PC.
//!
//! So a frame captured in FP16 is divided by the boost again
//! (`v × 80 / white_nits`), which gives back exactly the SDR content's
//! linear value, then encoded with the sRGB curve to 8 bits. Wide-gamut
//! colour outside sRGB (`v < 0`) goes to 0; real HDR highlights (`v > 1`:
//! HDR video, games, photos) clip — never a tone map that adapts to the
//! frame's brightest pixel, which darkens everything else (ShareX #8895).
//!
//! Fast: one 65,536-entry table per white level maps a channel's FP16 bits
//! straight to its 8-bit sRGB value (built in ~1 ms), and the frame is
//! converted on every core.
//!
//! The approach follows Microsoft's "Use DirectX with Advanced Color on
//! high/standard dynamic range displays" (the scRGB white and the SDR
//! boost) and `DISPLAYCONFIG_SDR_WHITE_LEVEL` (the level). No code was
//! copied for it.

/// scRGB 1.0, in nits.
pub const SCRGB_WHITE_NITS: f32 = 80.0;

/// An FP16 value (its bits) as f32. Exact for every half, subnormals,
/// infinities and NaN included.
pub fn f16_to_f32(h: u16) -> f32 {
    let sign = ((h >> 15) & 1) as u32;
    let exp = ((h >> 10) & 0x1f) as u32;
    let frac = (h & 0x3ff) as u32;
    let bits = match (exp, frac) {
        (0, 0) => sign << 31,
        (0, f) => {
            // Subnormal: normalise.
            let mut e: i32 = -14;
            let mut f = f;
            while f & 0x400 == 0 {
                f <<= 1;
                e -= 1;
            }
            let f = f & 0x3ff;
            (sign << 31) | (((e + 127) as u32) << 23) | (f << 13)
        }
        (0x1f, f) => (sign << 31) | (0xff << 23) | (f << 13),
        (e, f) => (sign << 31) | ((e + 127 - 15) << 23) | (f << 13),
    };
    f32::from_bits(bits)
}

/// f32 to FP16 bits, rounded to nearest (ties to even). For the tests and
/// the e2e suite's fake HDR monitor: what Windows would have composed.
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub fn f32_to_f16(v: f32) -> u16 {
    let bits = v.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let frac = bits & 0x7f_ffff;
    if exp == 0xff {
        return sign | 0x7c00 | if frac != 0 { 0x200 } else { 0 };
    }
    let e = exp - 127 + 15;
    if e >= 0x1f {
        return sign | 0x7c00;
    }
    if e <= 0 {
        if e < -10 {
            return sign;
        }
        // Subnormal half.
        let m = frac | 0x80_0000;
        let shift = (14 - e) as u32;
        let half = m >> shift;
        let rem = m & ((1 << shift) - 1);
        let mid = 1 << (shift - 1);
        let round = rem > mid || (rem == mid && half & 1 == 1);
        return sign | (half + round as u32) as u16;
    }
    let half = ((e as u32) << 10) | (frac >> 13);
    let rem = frac & 0x1fff;
    let round = rem > 0x1000 || (rem == 0x1000 && half & 1 == 1);
    sign | (half + round as u32) as u16
}

/// The sRGB curve: linear 0..1 → encoded 0..1.
pub fn srgb_encode(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// The sRGB curve back: encoded 0..1 → linear 0..1.
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub fn srgb_decode(e: f32) -> f32 {
    if e <= 0.040_45 {
        e / 12.92
    } else {
        ((e + 0.055) / 1.055).powf(2.4)
    }
}

/// An FP16 scRGB channel → 8-bit sRGB, for one SDR white level.
pub struct ToSrgb8 {
    white_nits: f32,
    lut: Box<[u8]>,
}

impl ToSrgb8 {
    /// The table for `white_nits` (Windows' SDR white level; 80 = no boost).
    /// A level Windows can't have (≤ 0, not a number) is taken as 80.
    pub fn new(white_nits: f32) -> Self {
        let white = if white_nits.is_finite() && white_nits > 0.0 { white_nits } else { SCRGB_WHITE_NITS };
        let scale = SCRGB_WHITE_NITS / white;
        let lut: Vec<u8> = (0..=u16::MAX)
            .map(|bits| {
                let v = f16_to_f32(bits) * scale;
                // NaN and negatives (out of sRGB's gamut) → 0; highlights clip.
                if v.is_nan() || v <= 0.0 {
                    0
                } else if v >= 1.0 {
                    255
                } else {
                    (srgb_encode(v) * 255.0 + 0.5) as u8
                }
            })
            .collect();
        ToSrgb8 { white_nits: white, lut: lut.into_boxed_slice() }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn white_nits(&self) -> f32 {
        self.white_nits
    }

    /// One channel's FP16 bits → its 8-bit sRGB value.
    #[cfg_attr(not(test), allow(dead_code))]
    #[inline]
    pub fn channel(&self, bits: u16) -> u8 {
        self.lut[bits as usize]
    }

    /// A row of FP16 RGBA pixels (4 halves each) → BGRA8, opaque.
    pub fn row_to_bgra8(&self, src: &[u16], dst: &mut [u8]) {
        for (s, d) in src.as_chunks::<4>().0.iter().zip(dst.as_chunks_mut::<4>().0.iter_mut()) {
            d[0] = self.lut[s[2] as usize];
            d[1] = self.lut[s[1] as usize];
            d[2] = self.lut[s[0] as usize];
            d[3] = 255;
        }
    }

    /// A whole frame of FP16 RGBA rows (`pitch` halves apart) → tightly
    /// packed BGRA8, on every core.
    pub fn frame_to_bgra8(&self, src: &[u16], pitch: usize, width: usize, height: usize) -> Vec<u8> {
        let mut out = vec![0u8; width * height * 4];
        if width == 0 || height == 0 {
            return out;
        }
        let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 8);
        let rows_per = height.div_ceil(threads);
        std::thread::scope(|scope| {
            for (chunk_index, dst) in out.chunks_mut(rows_per * width * 4).enumerate() {
                let first = chunk_index * rows_per;
                scope.spawn(move || {
                    for (r, row) in dst.chunks_exact_mut(width * 4).enumerate() {
                        let y = first + r;
                        let start = y * pitch;
                        self.row_to_bgra8(&src[start..start + width * 4], row);
                    }
                });
            }
        });
        out
    }
}

/// The table for `white_nits`, made once and kept while the level stays
/// (it's the same from one snip to the next).
pub fn cached(white_nits: f32) -> std::sync::Arc<ToSrgb8> {
    static LAST: std::sync::Mutex<Vec<std::sync::Arc<ToSrgb8>>> = std::sync::Mutex::new(Vec::new());
    let mut last = LAST.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(t) = last.iter().find(|t| t.white_nits == white_nits || (white_nits <= 0.0 && t.white_nits == SCRGB_WHITE_NITS)) {
        return t.clone();
    }
    let t = std::sync::Arc::new(ToSrgb8::new(white_nits));
    // Two monitors at two levels, at most a few: keep the last four.
    if last.len() >= 4 {
        last.remove(0);
    }
    last.push(t.clone());
    t
}

/// What Windows composes for an SDR app's 8-bit sRGB value at `white_nits`
/// (linearised, boosted), as FP16 bits: the inverse of [`ToSrgb8`], for the
/// tests and the e2e suite's fake HDR monitor.
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub fn composed(srgb: u8, white_nits: f32) -> u16 {
    f32_to_f16(srgb_decode(srgb as f32 / 255.0) * white_nits / SCRGB_WHITE_NITS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves_convert_exactly() {
        for (bits, want) in [
            (0x0000u16, 0.0f32),
            (0x3c00, 1.0),
            (0x4000, 2.0),
            (0x3800, 0.5),
            (0xc000, -2.0),
            (0x7bff, 65504.0),
            (0x0001, 5.960_464_5e-8),
            (0x0400, 6.103_515_6e-5),
        ] {
            assert_eq!(f16_to_f32(bits), want, "{bits:#06x}");
        }
        assert!(f16_to_f32(0x7c00).is_infinite());
        assert!(f16_to_f32(0x7e00).is_nan());
        // Every finite half survives the round trip.
        for bits in 0..=u16::MAX {
            let v = f16_to_f32(bits);
            if v.is_finite() {
                assert_eq!(f32_to_f16(v), bits, "{bits:#06x} → {v}");
            }
        }
    }

    #[test]
    fn the_srgb_curve_round_trips() {
        for s in 0..=255u8 {
            let e = s as f32 / 255.0;
            let back = srgb_encode(srgb_decode(e));
            assert!((back - e).abs() < 1e-5, "{s}: {back}");
        }
    }

    /// Every 8-bit sRGB value, put through Windows' boost at a white level
    /// and stored as FP16 (what an HDR desktop holds for an SDR app), comes
    /// back as itself — at no boost, at the usual 200, and at the bright
    /// levels people set.
    #[test]
    fn sdr_content_comes_back_exactly_at_every_white_level() {
        for white in [80.0f32, 120.0, 200.0, 280.0, 480.0] {
            let to8 = ToSrgb8::new(white);
            let mut worst = 0i32;
            for s in 0..=255u8 {
                let got = to8.channel(composed(s, white));
                worst = worst.max((got as i32 - s as i32).abs());
            }
            assert!(worst <= 1, "{white} nits: off by {worst}");
        }
        // The levels Windows' slider gives exactly: 0 off.
        let to8 = ToSrgb8::new(200.0);
        for s in [0u8, 1, 32, 128, 0xaa, 0xc0, 0xe8, 0xf5, 254, 255] {
            assert_eq!(to8.channel(composed(s, 200.0)), s, "#{s:02x}");
        }
    }

    /// The washed-out capture this fixes: an 8-bit read of the FP16 desktop
    /// clips everything above 40 % linear at 200 nits; divided by the level
    /// first, a light grey stays a light grey.
    #[test]
    fn a_light_grey_is_not_white() {
        let grey = 0xc0u8;
        let bits = composed(grey, 200.0);
        // What a plain 8-bit capture would read: clipped.
        assert!(f16_to_f32(bits) > 1.0);
        assert_eq!(ToSrgb8::new(200.0).channel(bits), grey);
    }

    #[test]
    fn out_of_gamut_goes_to_black_and_highlights_clip() {
        let to8 = ToSrgb8::new(200.0);
        assert_eq!(to8.channel(f32_to_f16(-0.25)), 0);
        assert_eq!(to8.channel(0xfe00), 0, "NaN");
        assert_eq!(to8.channel(f32_to_f16(12.0)), 255, "an HDR highlight");
        assert_eq!(to8.channel(0x7c00), 255, "infinity");
        // A level Windows can't have is no boost.
        assert_eq!(ToSrgb8::new(0.0).white_nits(), 80.0);
        assert_eq!(ToSrgb8::new(f32::NAN).white_nits(), 80.0);
    }

    #[test]
    fn a_frame_converts_row_by_row_into_bgra() {
        let white = 240.0;
        let to8 = ToSrgb8::new(white);
        // 3×2, a pitch of 4 pixels (16 halves), RGBA.
        let px = |r: u8, g: u8, b: u8| [composed(r, white), composed(g, white), composed(b, white), f32_to_f16(1.0)];
        let mut src = vec![0u16; 16 * 2];
        for (i, p) in [px(255, 0, 0), px(0, 255, 0), px(0, 0, 255), px(0x20, 0x80, 0xe8), px(10, 20, 30), px(250, 128, 5)]
            .iter()
            .enumerate()
        {
            let (x, y) = (i % 3, i / 3);
            src[y * 16 + x * 4..y * 16 + x * 4 + 4].copy_from_slice(p);
        }
        let out = to8.frame_to_bgra8(&src, 16, 3, 2);
        assert_eq!(out.len(), 3 * 2 * 4);
        assert_eq!(&out[0..4], &[0, 0, 255, 255], "red, as BGRA");
        assert_eq!(&out[4..8], &[0, 255, 0, 255]);
        assert_eq!(&out[8..12], &[255, 0, 0, 255]);
        assert_eq!(&out[12..16], &[0xe8, 0x80, 0x20, 255]);
        assert_eq!(&out[20..24], &[5, 128, 250, 255]);
    }
}

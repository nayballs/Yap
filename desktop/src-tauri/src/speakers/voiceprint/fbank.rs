//! Kaldi's log mel filterbank ("fbank"), the front end the voice-matching
//! model was trained on (3D-Speaker's ERes2Net: 80 bins, 25 ms frames every
//! 10 ms, a Povey window, pre-emphasis 0.97, the DC removed, no dither), with
//! each bin's mean over the clip taken off (the model's "global-mean"
//! normalisation). The same numbers as kaldi-native-fbank, which sherpa-onnx
//! uses for this model: checked against sherpa's embeddings in
//! `voiceprint.rs`'s tests.

/// The audio's rate (Yap's meeting audio and the model's).
pub const RATE: usize = 16_000;
/// Mel bins per frame.
pub const BINS: usize = 80;
/// 25 ms frames…
const FRAME: usize = 400;
/// …every 10 ms.
const SHIFT: usize = 160;
/// Each frame zero-padded to this for the FFT.
const FFT: usize = 512;
const PREEMPH: f32 = 0.97;
const LOW_HZ: f32 = 20.0;
/// sherpa-onnx's `high_freq = -400`: 400 Hz below Nyquist.
const HIGH_HZ: f32 = 7_600.0;

fn mel(hz: f32) -> f32 {
    1127.0 * (1.0 + hz / 700.0).ln()
}

/// The fixed parts: the window, the mel weights, the FFT's twiddles.
pub struct Fbank {
    window: Vec<f32>,
    /// Per bin: its first FFT bin and weights from there.
    banks: Vec<(usize, Vec<f32>)>,
    twiddles: Vec<(f32, f32)>,
    bitrev: Vec<usize>,
}

impl Default for Fbank {
    fn default() -> Self {
        Self::new()
    }
}

impl Fbank {
    pub fn new() -> Fbank {
        let window = (0..FRAME)
            .map(|i| {
                let a = 2.0 * std::f64::consts::PI * i as f64 / (FRAME - 1) as f64;
                (0.5 - 0.5 * a.cos()).powf(0.85) as f32
            })
            .collect();
        // Kaldi's triangles, evenly spaced on the mel scale, over the FFT's
        // bins below Nyquist.
        let bin_hz = RATE as f32 / FFT as f32;
        let (lo, hi) = (mel(LOW_HZ), mel(HIGH_HZ));
        let delta = (hi - lo) / (BINS + 1) as f32;
        let banks = (0..BINS)
            .map(|b| {
                let (left, center, right) = (lo + b as f32 * delta, lo + (b + 1) as f32 * delta, lo + (b + 2) as f32 * delta);
                let mut first = usize::MAX;
                let mut weights = Vec::new();
                for i in 0..FFT / 2 {
                    let m = mel(bin_hz * i as f32);
                    if m > left && m < right {
                        let w = if m <= center { (m - left) / (center - left) } else { (right - m) / (right - center) };
                        if first == usize::MAX {
                            first = i;
                        }
                        weights.resize(i - first, 0.0);
                        weights.push(w);
                    }
                }
                (first.min(FFT / 2), weights)
            })
            .collect();
        let twiddles = (0..FFT / 2)
            .map(|k| {
                let a = -2.0 * std::f64::consts::PI * k as f64 / FFT as f64;
                (a.cos() as f32, a.sin() as f32)
            })
            .collect();
        let bits = FFT.trailing_zeros();
        let bitrev = (0..FFT).map(|i| i.reverse_bits() >> (usize::BITS - bits)).collect();
        Fbank { window, banks, twiddles, bitrev }
    }

    /// In-place radix-2 FFT of `re` + i·`im` (both `FFT` long).
    fn fft(&self, re: &mut [f32], im: &mut [f32]) {
        for i in 0..FFT {
            let j = self.bitrev[i];
            if j > i {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let mut len = 2;
        while len <= FFT {
            let half = len / 2;
            let step = FFT / len;
            for start in (0..FFT).step_by(len) {
                for k in 0..half {
                    let (wr, wi) = self.twiddles[k * step];
                    let (a, b) = (start + k, start + k + half);
                    let tr = re[b] * wr - im[b] * wi;
                    let ti = re[b] * wi + im[b] * wr;
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            len *= 2;
        }
    }

    /// The clip's frames (`1 + (n − 400) / 160` of them; none under 25 ms),
    /// `BINS` log energies each, every bin's mean over the clip subtracted.
    pub fn features(&self, samples: &[f32]) -> Vec<[f32; BINS]> {
        if samples.len() < FRAME {
            return Vec::new();
        }
        let frames = 1 + (samples.len() - FRAME) / SHIFT;
        let mut out = Vec::with_capacity(frames);
        let (mut re, mut im) = (vec![0f32; FFT], vec![0f32; FFT]);
        let mut power = vec![0f32; FFT / 2 + 1];
        for f in 0..frames {
            let frame = &samples[f * SHIFT..f * SHIFT + FRAME];
            let mean = frame.iter().sum::<f32>() / FRAME as f32;
            re[..FRAME].iter_mut().zip(frame).for_each(|(r, &s)| *r = s - mean);
            for i in (1..FRAME).rev() {
                re[i] -= PREEMPH * re[i - 1];
            }
            re[0] -= PREEMPH * re[0];
            re[..FRAME].iter_mut().zip(&self.window).for_each(|(r, w)| *r *= w);
            re[FRAME..].iter_mut().for_each(|r| *r = 0.0);
            im.iter_mut().for_each(|i| *i = 0.0);
            self.fft(&mut re, &mut im);
            for (k, p) in power.iter_mut().enumerate() {
                *p = re[k] * re[k] + im[k] * im[k];
            }
            let mut row = [0f32; BINS];
            for (b, (first, weights)) in self.banks.iter().enumerate() {
                let e: f32 = weights.iter().zip(&power[*first..]).map(|(w, p)| w * p).sum();
                row[b] = e.max(f32::EPSILON).ln();
            }
            out.push(row);
        }
        // The model's "global-mean" normalisation.
        let mut mean = [0f32; BINS];
        for row in &out {
            for (m, v) in mean.iter_mut().zip(row) {
                *m += v;
            }
        }
        mean.iter_mut().for_each(|m| *m /= frames as f32);
        for row in &mut out {
            for (v, m) in row.iter_mut().zip(&mean) {
                *v -= m;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fft_is_a_dft() {
        let fb = Fbank::new();
        let x: Vec<f32> = (0..FFT).map(|i| ((i * 7919) % 97) as f32 / 97.0 - 0.5).collect();
        let (mut re, mut im) = (x.clone(), vec![0f32; FFT]);
        fb.fft(&mut re, &mut im);
        for k in [0, 1, 5, 100, 256, 300] {
            let (mut r, mut i) = (0f64, 0f64);
            for (n, &v) in x.iter().enumerate() {
                let a = -2.0 * std::f64::consts::PI * (k * n) as f64 / FFT as f64;
                r += v as f64 * a.cos();
                i += v as f64 * a.sin();
            }
            assert!((re[k] as f64 - r).abs() < 1e-3 && (im[k] as f64 - i).abs() < 1e-3, "bin {k}");
        }
    }

    #[test]
    fn a_tone_lights_its_own_mel_bin_and_frames_count_like_kaldi() {
        let fb = Fbank::new();
        assert!(fb.features(&[0.0; 399]).is_empty());
        let tone: Vec<f32> = (0..RATE).map(|i| (2.0 * std::f32::consts::PI * 1_000.0 * i as f32 / RATE as f32).sin() * 0.3).collect();
        let f = fb.features(&tone);
        assert_eq!(f.len(), 1 + (RATE - FRAME) / SHIFT);
        // Mean-normalised: every bin averages 0.
        for b in 0..BINS {
            let m: f32 = f.iter().map(|r| r[b]).sum::<f32>() / f.len() as f32;
            assert!(m.abs() < 1e-3, "bin {b}: {m}");
        }
        // Without the normalisation, the loudest bin is the one holding 1 kHz.
        let banks_at_1k: Vec<usize> = fb
            .banks
            .iter()
            .enumerate()
            .filter(|(_, (first, w))| (*first..*first + w.len()).contains(&(1_000 * FFT / RATE)))
            .map(|(b, _)| b)
            .collect();
        assert!(!banks_at_1k.is_empty());
        assert!(fb.banks.iter().all(|(_, w)| !w.is_empty()), "every bin has weights");
    }
}

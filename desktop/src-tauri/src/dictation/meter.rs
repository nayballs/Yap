//! The level meter behind the Yap bar's waveform and onboarding's mic
//! check (`yap-amp`): a **perceptual scale**, so a whisper visibly moves
//! the bars while a shout never hits the top (Wispr Flow's "meter that
//! moves for whispers", by behaviour). Pure, unit-tested; the pipeline's
//! `AmpMeter` takes the peak of each ~80 ms of audio and asks [`Floor::level`].
//!
//! - **Decibels**: the peak in dBFS (`db`), placed between a floor and
//!   0 dBFS. A linear scale (what the bar had) put a whisper — peaks around
//!   -45 dBFS, 0.005 — at 2 % of a shout: the bars lay flat.
//! - **The floor follows the mic's own noise** ([`Floor`]): the quietest
//!   recent bars plus [`NOISE_MARGIN_DB`], kept between [`FLOOR_MIN_DB`]
//!   (-60) and [`FLOOR_MAX_DB`] (-48). It falls at once to a quieter bar and
//!   rises slowly ([`RISE_DB_PER_BAR`], ~3 dB a second), so a noisy mic's
//!   hiss sits at the bottom and a quiet room's whisper still shows — and,
//!   capped at -48 dBFS, a long whisper never becomes the floor.
//! - **Gentle compression** ([`level`]): `TOP · t^CURVE`, t the place 0..1
//!   between the floor and 0 dBFS. A whisper (-45 dBFS) about a third of the
//!   height, speech (-20…-10) two thirds to four fifths, full scale [`TOP`]:
//!   a shout never flattens against the top.

/// 0 dBFS: the top of the scale.
pub const CEIL_DB: f32 = 0.0;
/// The floor in a quiet room (and before the meter has heard anything).
pub const FLOOR_MIN_DB: f32 = -60.0;
/// The floor never goes above this, however noisy the mic: a whisper
/// (-45…-35 dBFS) always shows.
pub const FLOOR_MAX_DB: f32 = -48.0;
/// The floor sits this far above the mic's noise.
pub const NOISE_MARGIN_DB: f32 = 6.0;
/// How fast the noise estimate rises towards a louder bar, per bar (~80 ms):
/// ~3 dB a second. It falls to a quieter bar at once — speech has dips
/// between its syllables, so only a steady sound (a hiss, a hum) lifts it.
pub const RISE_DB_PER_BAR: f32 = 0.25;
/// The highest level: full scale stops short of the top.
pub const TOP: f32 = 0.92;
/// The compression's curve (< 1 lifts the quiet end).
pub const CURVE: f32 = 0.75;

/// `peak` (0..1, linear) in dBFS; silence is -120.
pub fn db(peak: f32) -> f32 {
    20.0 * peak.abs().max(1e-6).log10()
}

/// A bar's height (0..[`TOP`]) for a peak of `peak_db` dBFS over a floor
/// of `floor_db`. Monotonic in `peak_db`; 0 at or below the floor.
pub fn level(peak_db: f32, floor_db: f32) -> f32 {
    let span = (CEIL_DB - floor_db).max(1.0);
    let t = ((peak_db - floor_db) / span).clamp(0.0, 1.0);
    TOP * t.powf(CURVE)
}

/// The meter's noise floor, following the mic (see the module docs).
#[derive(Debug, Clone, Copy)]
pub struct Floor {
    /// The noise estimate (dBFS): the quietest recent bars.
    noise_db: f32,
}

impl Default for Floor {
    fn default() -> Self {
        Floor { noise_db: FLOOR_MIN_DB - NOISE_MARGIN_DB }
    }
}

impl Floor {
    /// The floor now (dBFS).
    pub fn db(&self) -> f32 {
        (self.noise_db + NOISE_MARGIN_DB).clamp(FLOOR_MIN_DB, FLOOR_MAX_DB)
    }

    /// The next bar: its `peak` (0..1) updates the noise estimate, then
    /// it's placed on the scale ([`level`]).
    pub fn level(&mut self, peak: f32) -> f32 {
        let d = db(peak);
        self.noise_db = if d < self.noise_db { d } else { (self.noise_db + RISE_DB_PER_BAR).min(d) };
        level(d, self.db())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// dBFS back to a linear peak.
    fn peak(db: f32) -> f32 {
        10f32.powf(db / 20.0)
    }

    #[test]
    fn decibels() {
        assert!((db(1.0) - 0.0).abs() < 1e-4);
        assert!((db(0.1) + 20.0).abs() < 1e-3);
        assert!((db(0.001) + 60.0).abs() < 1e-2);
        assert_eq!(db(0.0), -120.0);
        assert_eq!(db(-0.5), db(0.5));
    }

    /// The whole scale on the quietest floor: a whisper visibly moves it,
    /// speech fills most of it, a shout stops short of the top.
    #[test]
    fn a_whisper_moves_it_and_a_shout_never_clips() {
        let f = FLOOR_MIN_DB;
        // Silence and the floor itself: flat.
        assert_eq!(level(-120.0, f), 0.0);
        assert_eq!(level(f, f), 0.0);
        // A whisper (-45 dBFS, peaks of ~0.005): a third of the height —
        // the old linear gain (×3.5, ^0.7) gave it 6 %.
        let whisper = level(-45.0, f);
        assert!(whisper > 0.3 && whisper < 0.4, "{whisper}");
        let old = (peak(-45.0) * 3.5f32).powf(0.7);
        assert!(old < 0.07, "{old}");
        // Speech: two thirds to four fifths.
        let speech = level(-20.0, f);
        assert!(speech > 0.6 && speech < 0.75, "{speech}");
        assert!(level(-10.0, f) > 0.75 && level(-10.0, f) < 0.85);
        // A shout at full scale: TOP, not 1; and still above loud speech.
        assert_eq!(level(0.0, f), TOP);
        assert!(level(0.0, f) < 1.0);
        assert!(level(-3.0, f) < level(0.0, f));
        assert!(level(-3.0, f) > level(-10.0, f));
        // Monotonic all the way.
        let mut last = -1.0;
        for d in (-80..=0).map(|d| d as f32) {
            let l = level(d, f);
            assert!(l >= last, "{d}: {l} < {last}");
            last = l;
        }
    }

    /// The floor follows a noisy mic's hiss (the hiss flat at the bottom)
    /// but never so high that a whisper disappears.
    #[test]
    fn the_floor_follows_the_noise_but_a_whisper_still_shows() {
        let mut m = Floor::default();
        assert_eq!(m.db(), FLOOR_MIN_DB);
        // A noisy mic: hiss at -50 dBFS for a few seconds.
        for _ in 0..80 {
            m.level(peak(-50.0));
        }
        assert_eq!(m.db(), FLOOR_MAX_DB, "-50 + 6 dB, kept at -48");
        assert_eq!(m.level(peak(-50.0)), 0.0, "the hiss: flat");
        let whisper = m.level(peak(-40.0));
        assert!(whisper > 0.2, "{whisper}");
        // A quiet room: the floor falls at once to its quietest.
        m.level(peak(-80.0));
        assert_eq!(m.db(), FLOOR_MIN_DB);
    }

    /// Long speech raises the noise estimate only slowly (3 dB a second),
    /// and never past the cap: ten seconds of whispering stays visible.
    #[test]
    fn a_long_whisper_never_becomes_the_floor() {
        let mut m = Floor::default();
        m.level(peak(-70.0));
        // One second of speech at -30 dBFS with no pause: the floor moved
        // by 6 dB at most.
        for _ in 0..12 {
            m.level(peak(-30.0));
        }
        assert!(m.db() <= FLOOR_MIN_DB + 6.01, "{}", m.db());
        // A pause between words: back down at once.
        m.level(peak(-70.0));
        assert_eq!(m.db(), FLOOR_MIN_DB);
        // Ten seconds of a whisper with no pause at all.
        let mut last = 1.0;
        for _ in 0..125 {
            last = m.level(peak(-42.0));
        }
        assert!(m.db() <= FLOOR_MAX_DB);
        assert!(last > 0.15, "{last}");
    }
}

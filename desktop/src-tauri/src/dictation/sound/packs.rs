//! Yap's sound packs: a sound for every moment Yap has one for ([`Cue`]),
//! in four packs (Settings → Dictation → Sounds). All of them are made
//! here, from numbers, as they play: no sound files, nothing downloaded.
//!
//! A sound is a few [`Note`]s, each a waveform under an envelope — a linear
//! attack, then an exponential fall to −66 dB by its end, so nothing clicks
//! (Web Audio's `linearRampToValueAtTime` + `exponentialRampToValueAtTime`,
//! as OpenWhispr's `dictationCues.js` uses them). The waveforms: a sine; a
//! warm sine (a little of its second and third harmonics); FM (Chowning's:
//! a carrier whose phase a second sine bends, the bend fading with the
//! note, so it starts bright and ends round — a bell, or glass at an
//! inharmonic ratio); and a tick (white noise from a fixed seed, high-passed
//! by one pole, a few milliseconds long). A pitch may glide (exponentially,
//! as an ear hears it).
//!
//! - **Classic**: Yap's chime since the start — two sine notes, rising
//!   C5→E5 to start and falling D5→A4 to stop (OpenWhispr's, verbatim);
//!   its other moments in the same two-note style.
//! - **Soft**: lower, warmer notes with a slow attack.
//! - **Crisp**: short FM bells over a light tick.
//! - **Minimal**: one quick blip, its pitch gliding up or down.
//!
//! Each pack's start rises and its stop falls, so the two are never
//! confused. Every sound is under 300 ms and peaks at [`MAX_GAIN`] at most
//! (at full volume): the unit tests hold every pack to that.

use serde::{Deserialize, Serialize};

pub const SAMPLE_RATE: u32 = 44_100;
const SR: f32 = SAMPLE_RATE as f32;
/// The loudest a sound peaks, at full volume (Classic's chime).
pub const MAX_GAIN: f32 = 0.2;
/// Where a note's exponential fall ends, against its peak: −66 dB
/// (Classic's 0.0001 against 0.2).
const DECAY_TO: f32 = 0.0001 / 0.2;
/// Silence after the last note.
const TAIL: f32 = 0.02;

/// A moment with a sound of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Cue {
    /// A dictation starts (the mic hears).
    Start,
    /// A dictation stops, to be transcribed.
    Stop,
    /// Push-to-talk's double-tap: recording hands-free.
    Locked,
    /// A dictation cancelled (the bar's ✕, the tray's Cancel).
    Cancelled,
    /// A dictation couldn't be pasted or transcribed.
    Error,
    /// "Taking longer than usual…".
    Slow,
    /// Meeting notes started recording.
    MeetingStart,
    /// Meeting notes stopped recording.
    MeetingStop,
    /// A card on the Yap bar.
    Notice,
}

impl Cue {
    /// Every moment, in the order Settings lists them and a pack's ▶ plays.
    pub const ALL: [Cue; 9] = [
        Cue::Start,
        Cue::Stop,
        Cue::Locked,
        Cue::Cancelled,
        Cue::Error,
        Cue::Slow,
        Cue::MeetingStart,
        Cue::MeetingStop,
        Cue::Notice,
    ];

    /// Its id, as the pages and the e2e record name it.
    pub fn id(self) -> &'static str {
        match self {
            Cue::Start => "start",
            Cue::Stop => "stop",
            Cue::Locked => "locked",
            Cue::Cancelled => "cancelled",
            Cue::Error => "error",
            Cue::Slow => "slow",
            Cue::MeetingStart => "meetingStart",
            Cue::MeetingStop => "meetingStop",
            Cue::Notice => "notice",
        }
    }

    pub fn from_id(id: &str) -> Option<Cue> {
        Cue::ALL.into_iter().find(|c| c.id() == id)
    }
}

/// A sound pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pack {
    pub id: &'static str,
}

pub const CLASSIC: Pack = Pack { id: "classic" };
pub const SOFT: Pack = Pack { id: "soft" };
pub const CRISP: Pack = Pack { id: "crisp" };
pub const MINIMAL: Pack = Pack { id: "minimal" };
pub const PACKS: [Pack; 4] = [CLASSIC, SOFT, CRISP, MINIMAL];
/// The pack a new install starts with, and the one an unknown id plays.
pub const DEFAULT_PACK: &str = CLASSIC.id;

impl Pack {
    /// The pack `id` names, else Classic (a newer Yap's pack, say).
    pub fn named(id: &str) -> Pack {
        PACKS.into_iter().find(|p| p.id == id).unwrap_or(CLASSIC)
    }

    /// `cue`'s sound in this pack at `peak` (its loudest, 0..[`MAX_GAIN`]):
    /// mono, [`SAMPLE_RATE`].
    pub fn render(self, cue: Cue, peak: f32) -> Vec<f32> {
        render(&notes(self, cue), peak)
    }

    /// How long `cue`'s sound lasts, its silent tail included.
    pub fn length(self, cue: Cue) -> std::time::Duration {
        std::time::Duration::from_secs_f32(length(&notes(self, cue)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Wave {
    Sine,
    /// The sine with a little of its second and third harmonics.
    Warm,
    /// FM: the carrier's phase bent by a sine at `ratio` × its pitch, by
    /// up to `index` radians, fading with the note.
    Fm { ratio: f32, index: f32 },
    /// White noise above `cutoff` Hz (one pole): a click.
    Tick { cutoff: f32 },
}

/// One note of a sound.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Note {
    /// When it starts and how long it lasts (s).
    at: f32,
    len: f32,
    /// Its pitch at the start and at the end (Hz; the same for no glide).
    from: f32,
    to: f32,
    /// Its linear rise to full (s).
    attack: f32,
    /// Its loudness against the sound's peak (0..1).
    gain: f32,
    wave: Wave,
}

#[cfg(test)]
impl Note {
    fn pitched(&self) -> bool {
        !matches!(self.wave, Wave::Tick { .. })
    }
}

// Pitches (Hz).
const E4: f32 = 329.63;
const G4: f32 = 392.0;
const A4: f32 = 440.0;
const B4: f32 = 493.88;
const C5: f32 = 523.25;
const D5: f32 = 587.33;
const E5: f32 = 659.25;
const G5: f32 = 783.99;
const A5: f32 = 880.0;
const B5: f32 = 987.77;
const CS6: f32 = 1108.73;
const E6: f32 = 1318.51;

/// Classic's note: a sine with a 15 ms attack.
fn sine(at: f32, len: f32, hz: f32, gain: f32) -> Note {
    Note { at, len, from: hz, to: hz, attack: 0.015, gain, wave: Wave::Sine }
}

/// Soft's note: warm, with a slow 30 ms attack.
fn warm(at: f32, len: f32, hz: f32, gain: f32) -> Note {
    Note { at, len, from: hz, to: hz, attack: 0.03, gain, wave: Wave::Warm }
}

/// Crisp's note: an FM bell (harmonic, ratio 2), struck.
fn bell(at: f32, len: f32, hz: f32, gain: f32) -> Note {
    Note { at, len, from: hz, to: hz, attack: 0.002, gain, wave: Wave::Fm { ratio: 2.0, index: 1.2 } }
}

/// Crisp's notice: glass (an inharmonic ratio).
fn glass(at: f32, len: f32, hz: f32, gain: f32) -> Note {
    Note { at, len, from: hz, to: hz, attack: 0.002, gain, wave: Wave::Fm { ratio: 3.5, index: 0.9 } }
}

/// Crisp's error: a reedy tone (FM at ratio 1, a sawtooth's colour).
fn reed(at: f32, len: f32, hz: f32, gain: f32) -> Note {
    Note { at, len, from: hz, to: hz, attack: 0.004, gain, wave: Wave::Fm { ratio: 1.0, index: 1.6 } }
}

/// A 5 ms click under a struck note.
fn tick(at: f32, gain: f32) -> Note {
    Note { at, len: 0.005, from: 0.0, to: 0.0, attack: 0.0005, gain, wave: Wave::Tick { cutoff: 3_000.0 } }
}

/// Minimal's blip: a sine gliding `from` → `to`, a 4 ms attack.
fn blip(at: f32, len: f32, from: f32, to: f32, gain: f32) -> Note {
    Note { at, len, from, to, attack: 0.004, gain, wave: Wave::Sine }
}

/// `pitches` one after another, each `len` long with `gap` between.
fn run(pitches: &[f32], len: f32, gap: f32, gain: f32, note: fn(f32, f32, f32, f32) -> Note) -> Vec<Note> {
    pitches.iter().enumerate().map(|(i, &hz)| note(i as f32 * (len + gap), len, hz, gain)).collect()
}

/// `cue`'s notes in `pack`.
fn notes(pack: Pack, cue: Cue) -> Vec<Note> {
    use Cue::*;
    match pack.id {
        "soft" => match cue {
            Start => vec![warm(0.0, 0.12, G4, 0.8), warm(0.10, 0.14, D5, 0.8)],
            Stop => vec![warm(0.0, 0.12, D5, 0.8), warm(0.10, 0.14, G4, 0.8)],
            Locked => vec![warm(0.0, 0.08, D5, 0.7), warm(0.07, 0.11, G5, 0.7)],
            Cancelled => vec![warm(0.0, 0.11, A4, 0.75), warm(0.09, 0.14, E4, 0.75)],
            Error => vec![warm(0.0, 0.09, E4, 0.8), warm(0.12, 0.11, E4, 0.8)],
            Slow => vec![warm(0.0, 0.2, A4, 0.6)],
            MeetingStart => vec![warm(0.0, 0.09, G4, 0.75), warm(0.08, 0.09, B4, 0.75), warm(0.16, 0.11, D5, 0.75)],
            MeetingStop => vec![warm(0.0, 0.09, D5, 0.75), warm(0.08, 0.09, B4, 0.75), warm(0.16, 0.11, G4, 0.75)],
            Notice => vec![warm(0.0, 0.2, G5, 0.5)],
        },
        "crisp" => match cue {
            Start => vec![tick(0.0, 0.25), bell(0.0, 0.08, A5, 0.7), bell(0.07, 0.11, E6, 0.7)],
            Stop => vec![tick(0.0, 0.25), bell(0.0, 0.08, E6, 0.7), bell(0.07, 0.11, A5, 0.7)],
            Locked => vec![tick(0.0, 0.25), bell(0.0, 0.05, E6, 0.6), tick(0.06, 0.25), bell(0.06, 0.08, E6, 0.6)],
            Cancelled => vec![bell(0.0, 0.08, E5, 0.65), bell(0.07, 0.12, A4, 0.65)],
            Error => vec![reed(0.0, 0.06, A4, 0.5), reed(0.09, 0.08, A4, 0.5)],
            Slow => vec![tick(0.0, 0.2), bell(0.0, 0.07, G5, 0.45)],
            MeetingStart => {
                let mut n = vec![tick(0.0, 0.25)];
                n.extend(run(&[A5, CS6, E6], 0.055, 0.01, 0.6, bell));
                n
            }
            MeetingStop => {
                let mut n = vec![tick(0.0, 0.25)];
                n.extend(run(&[E6, CS6, A5], 0.055, 0.01, 0.6, bell));
                n
            }
            Notice => vec![glass(0.0, 0.18, B5, 0.5)],
        },
        "minimal" => match cue {
            Start => vec![blip(0.0, 0.07, 520.0, 780.0, 0.8)],
            Stop => vec![blip(0.0, 0.07, 780.0, 520.0, 0.8)],
            Locked => vec![blip(0.0, 0.03, 900.0, 900.0, 0.7), blip(0.055, 0.03, 900.0, 900.0, 0.7)],
            Cancelled => vec![blip(0.0, 0.09, 600.0, 380.0, 0.75)],
            Error => vec![blip(0.0, 0.045, 330.0, 330.0, 0.85), blip(0.075, 0.045, 330.0, 330.0, 0.85)],
            Slow => vec![blip(0.0, 0.04, 700.0, 700.0, 0.5)],
            MeetingStart => vec![blip(0.0, 0.14, 440.0, 880.0, 0.7)],
            MeetingStop => vec![blip(0.0, 0.14, 880.0, 440.0, 0.7)],
            Notice => vec![blip(0.0, 0.05, 1_050.0, 1_050.0, 0.55)],
        },
        // Classic: two-note sines, 90 ms notes 25 ms apart (start and stop
        // exactly as Yap has always played them).
        _ => match cue {
            Start => run(&[C5, E5], 0.09, 0.025, 1.0, sine),
            Stop => run(&[D5, A4], 0.09, 0.025, 1.0, sine),
            Locked => run(&[E5, A5], 0.06, 0.015, 0.85, sine),
            Cancelled => run(&[A4, E4], 0.08, 0.025, 0.9, sine),
            Error => run(&[A4, A4], 0.07, 0.045, 0.9, sine),
            Slow => vec![sine(0.0, 0.16, B4, 0.6)],
            MeetingStart => run(&[C5, E5, G5], 0.07, 0.015, 0.9, sine),
            MeetingStop => run(&[G5, E5, C5], 0.07, 0.015, 0.9, sine),
            Notice => vec![sine(0.0, 0.14, A5, 0.6)],
        },
    }
}

/// How long `notes` last, the tail included (s).
fn length(notes: &[Note]) -> f32 {
    notes.iter().map(|n| n.at + n.len).fold(0.0, f32::max) + TAIL
}

/// `notes` rendered at `peak`: each note's waveform under its envelope,
/// summed.
fn render(notes: &[Note], peak: f32) -> Vec<f32> {
    use std::f32::consts::TAU;
    let mut out = vec![0.0f32; (length(notes) * SR) as usize];
    for (k, n) in notes.iter().enumerate() {
        let first = (n.at * SR) as usize;
        let count = (n.len * SR) as usize;
        let attack = n.attack.max(1.0 / SR);
        let fall = (n.len - attack).max(1.0 / SR);
        let (mut phase, mut bend) = (0.0f32, 0.0f32);
        // The tick's noise: the same every time (xorshift, seeded per note).
        let mut seed: u32 = 0x9E37_79B9 ^ (k as u32 + 1).wrapping_mul(0x85EB_CA6B);
        let mut low = 0.0f32;
        for j in 0..count {
            let t = j as f32 / SR;
            let env = if t < attack { t / attack } else { DECAY_TO.powf((t - attack) / fall) };
            let hz = if n.from == n.to { n.from } else { n.from * (n.to / n.from).powf(t / n.len) };
            let s = match n.wave {
                Wave::Sine => phase.sin(),
                Wave::Warm => (phase.sin() + 0.12 * (2.0 * phase).sin() + 0.04 * (3.0 * phase).sin()) / 1.16,
                Wave::Fm { index, .. } => (phase + index * env * bend.sin()).sin(),
                Wave::Tick { cutoff } => {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    let white = seed as f32 / u32::MAX as f32 * 2.0 - 1.0;
                    low += (1.0 - (-TAU * cutoff / SR).exp()) * (white - low);
                    white - low
                }
            };
            if let Some(o) = out.get_mut(first + j) {
                *o += peak * n.gain * env * s;
            }
            phase = (phase + TAU * hz / SR) % TAU;
            if let Wave::Fm { ratio, .. } = n.wave {
                bend = (bend + TAU * hz * ratio / SR) % TAU;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The longest any sound may be (s).
    const LONGEST: f32 = 0.3;

    fn peak_of(s: &[f32]) -> f32 {
        s.iter().fold(0.0f32, |m, x| m.max(x.abs()))
    }

    fn energy(s: &[f32]) -> f32 {
        s.iter().map(|x| x * x).sum::<f32>() / SR
    }

    /// Every pack has a sound for every moment: under 300 ms, at least a
    /// beat long, never louder than Classic's chime at full volume (and
    /// never too quiet to hear), its energy never more than half again
    /// Classic's start's — and silent at both ends, so nothing clicks.
    #[test]
    fn every_pack_has_every_moment_short_and_within_its_levels() {
        let classic = energy(&CLASSIC.render(Cue::Start, MAX_GAIN));
        for pack in PACKS {
            for cue in Cue::ALL {
                let s = pack.render(cue, MAX_GAIN);
                let secs = s.len() as f32 / SR;
                let what = format!("{}.{}", pack.id, cue.id());
                assert!(!notes(pack, cue).is_empty(), "{what}: no notes");
                assert!(secs <= LONGEST, "{what}: {secs:.3} s");
                assert!(secs >= 0.05, "{what}: {secs:.3} s");
                assert!((pack.length(cue).as_secs_f32() - secs).abs() < 0.001, "{what}: its length");
                let peak = peak_of(&s);
                assert!(peak <= MAX_GAIN * 1.001, "{what}: peaks at {peak}");
                assert!(peak >= MAX_GAIN * 0.3, "{what}: too quiet ({peak})");
                assert!(energy(&s) <= classic * 1.5, "{what}: louder than Classic ({} against {classic})", energy(&s));
                assert!(s[0].abs() < 1e-3, "{what}: starts with a click");
                let end = &s[s.len() - (0.002 * SR) as usize..];
                assert!(peak_of(end) < MAX_GAIN * 0.01, "{what}: ends with a click");
                assert!(s.iter().all(|x| x.is_finite()), "{what}: not a number");
            }
        }
    }

    /// The volume scales the sound, and none at all is silence.
    #[test]
    fn the_volume_scales_every_sound() {
        for pack in PACKS {
            let full = pack.render(Cue::Locked, MAX_GAIN);
            let half = pack.render(Cue::Locked, MAX_GAIN / 2.0);
            assert!((peak_of(&half) * 2.0 - peak_of(&full)).abs() < 1e-4, "{}", pack.id);
            assert_eq!(peak_of(&pack.render(Cue::Locked, 0.0)), 0.0);
        }
    }

    /// Each pack's start rises and its stop falls (and the two sound
    /// different), so they're never told apart by guesswork.
    #[test]
    fn every_packs_start_rises_and_its_stop_falls() {
        let contour = |pack: Pack, cue: Cue| {
            let n: Vec<Note> = notes(pack, cue).into_iter().filter(Note::pitched).collect();
            (n.first().map(|n| n.from).unwrap_or(0.0), n.last().map(|n| n.to).unwrap_or(0.0))
        };
        for pack in PACKS {
            let (a, b) = contour(pack, Cue::Start);
            assert!(b > a * 1.2, "{}: the start rises ({a} → {b})", pack.id);
            let (a, b) = contour(pack, Cue::Stop);
            assert!(b < a / 1.2, "{}: the stop falls ({a} → {b})", pack.id);
            // Little alike, sample for sample.
            let (start, stop) = (pack.render(Cue::Start, MAX_GAIN), pack.render(Cue::Stop, MAX_GAIN));
            let n = start.len().min(stop.len());
            let dot: f32 = start[..n].iter().zip(&stop[..n]).map(|(x, y)| x * y).sum();
            let alike = dot / (energy(&start[..n]) * energy(&stop[..n])).sqrt() / SR;
            assert!(alike.abs() < 0.5, "{}: start and stop alike ({alike})", pack.id);
            // Meeting notes' start and stop too.
            let (a, b) = contour(pack, Cue::MeetingStart);
            assert!(b > a, "{}: meeting notes' start rises", pack.id);
            let (a, b) = contour(pack, Cue::MeetingStop);
            assert!(b < a, "{}: meeting notes' stop falls", pack.id);
        }
    }

    /// Classic's start and stop are the chime Yap always played (C5→E5,
    /// D5→A4; OpenWhispr's `dictationCues.js`), sample for sample — less
    /// the 25 ms of silence it kept after its last note.
    #[test]
    fn classic_is_the_chime_yap_always_played() {
        fn before(notes: &[f32], peak: f32) -> Vec<f32> {
            let (dur, gap, attack) = (0.09f32, 0.025f32, 0.015f32);
            let step = dur + gap;
            let mut out = vec![0.0f32; ((step * notes.len() as f32 + 0.02) * SR) as usize];
            for (i, &f) in notes.iter().enumerate() {
                let first = (i as f32 * step * SR) as usize;
                for j in 0..(dur * SR) as usize {
                    let t = j as f32 / SR;
                    let g = if t < attack { peak * (t / attack) } else { peak * (0.0001f32 / 0.2).powf((t - attack) / (dur - attack)) };
                    if let Some(s) = out.get_mut(first + j) {
                        *s += g * (std::f32::consts::TAU * f * t).sin();
                    }
                }
            }
            out
        }
        for (cue, pitches) in [(Cue::Start, [C5, E5]), (Cue::Stop, [D5, A4])] {
            let old = before(&pitches, MAX_GAIN);
            let new = CLASSIC.render(cue, MAX_GAIN);
            assert!((old.len() as f32 - new.len() as f32 - 0.025 * SR).abs() <= 1.0, "{cue:?}: {} against {}", old.len(), new.len());
            let worst = old.iter().zip(&new).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
            assert!(worst < 1e-3, "{cue:?}: differs by {worst}");
            assert!(old[new.len()..].iter().all(|s| *s == 0.0), "{cue:?}: only silence left out");
        }
    }

    /// The same sound every time (the tick's noise has a fixed seed), and
    /// an unknown pack plays Classic.
    #[test]
    fn a_sound_is_the_same_every_time_and_an_unknown_pack_is_classic() {
        assert_eq!(CRISP.render(Cue::Start, MAX_GAIN), CRISP.render(Cue::Start, MAX_GAIN));
        assert_eq!(Pack::named("bagpipes"), CLASSIC);
        assert_eq!(Pack::named("soft"), SOFT);
        for cue in Cue::ALL {
            assert_eq!(Cue::from_id(cue.id()), Some(cue));
        }
        assert_eq!(Cue::from_id("fanfare"), None);
    }
}

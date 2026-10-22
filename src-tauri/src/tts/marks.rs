//! **Word timing** for reading aloud (`read_aloud.rs`'s karaoke): when each
//! word of a sentence starts in the audio the engine made for it. Pure, and
//! unit-tested.
//!
//! Three ways, best first, and the card says which one a sentence got
//! ([`Timing`]):
//!
//! - **The engine's own** ([`Timing::Engine`], [`from_chars`]): Windows'
//!   voices report a cue per word (`SpeechSynthesizerOptions.
//!   IncludeWordBoundaryMetadata`: where it starts in the text and when),
//!   ElevenLabs a start time per character (`/stream/with-timestamps`).
//! - **From the audio** ([`Timing::Audio`], [`from_audio`]): Kokoro's ONNX
//!   export hands back the waveform only (its one output is `waveform`, no
//!   durations), so the words are spread over the part of the audio that's
//!   speech (its silent ends cut off) by their syllables, with the pauses
//!   the voice made in it pinned to the commas and full stops nearest them.
//! - **From the length alone** ([`Timing::Estimate`], [`estimate`]): the
//!   same spread over the whole sentence, when the audio has no speech to go
//!   by (test mode's silent stand-in).
//!
//! What's said isn't always what's shown — "£12,400" is said "twelve
//! thousand four hundred pounds" (`normalize.rs`) — so the said words are
//! matched back to the shown ones in order ([`align`]): words the same on
//! both sides pair up, and the said words between two pairs light up the
//! shown words between them.

use std::time::Duration;

use serde::Serialize;

/// A word's place in its sentence: `[start, end)` in **characters** (not
/// bytes), the word without the punctuation at its edges.
pub type Span = (usize, usize);

/// How a sentence's word times were found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Timing {
    /// The engine said when each word starts (Windows voices, ElevenLabs).
    Engine,
    /// Estimated over the speech in the audio, pinned to its pauses (Kokoro).
    Audio,
    /// Estimated from the sentence's length alone.
    Estimate,
}

/// The engine's own timing: the character at `at` (a char index into the
/// text it was given) starts at `start` into that text's audio.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CharTime {
    pub at: usize,
    pub start: Duration,
}

/// The words of `text`: each run between whitespace, without the
/// punctuation at its edges ("(hello," → "hello"); a run with no letter or
/// digit isn't a word ("—", "…").
pub fn words(text: &str) -> Vec<Span> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        let (mut a, mut b) = (start, i);
        while a < b && !chars[a].is_alphanumeric() {
            a += 1;
        }
        while b > a && !chars[b - 1].is_alphanumeric() {
            b -= 1;
        }
        if a < b {
            out.push((a, b));
        }
    }
    out
}

/// The text of a span.
fn slice(chars: &[char], (a, b): Span) -> String {
    chars[a.min(chars.len())..b.min(chars.len())].iter().collect()
}

/// About how many syllables a word has: its vowel groups (a silent final
/// "e" off), at least one; a short all-capitals word is spelled out ("API":
/// three); digits one each.
pub fn syllables(word: &str) -> usize {
    let letters: Vec<char> = word.chars().filter(|c| c.is_alphanumeric()).collect();
    if letters.is_empty() {
        return 1;
    }
    if (2..=4).contains(&letters.len()) && letters.iter().all(|c| c.is_ascii_uppercase()) {
        return letters.len();
    }
    let digits = letters.iter().filter(|c| c.is_ascii_digit()).count();
    let lower: Vec<char> = letters.iter().filter(|c| !c.is_ascii_digit()).flat_map(|c| c.to_lowercase()).collect();
    if lower.iter().any(|c| !c.is_ascii()) && !lower.iter().any(|c| "aeiouy".contains(*c)) {
        // Another script: about one syllable per two or three letters.
        return digits + lower.len().div_ceil(3).max(1);
    }
    let vowel = |c: char| "aeiouyàáâäèéêëìíîïòóôöùúûü".contains(c);
    let mut groups = 0;
    let mut prev = false;
    for &c in &lower {
        let v = vowel(c);
        if v && !prev {
            groups += 1;
        }
        prev = v;
    }
    let n = lower.len();
    if groups > 1 && n > 2 && lower[n - 1] == 'e' && !vowel(lower[n - 2]) && !(lower[n - 2] == 'l' && n > 3 && !vowel(lower[n - 3])) {
        groups -= 1;
    }
    (groups + digits).max(1)
}

/// How long the pause after each word is, in syllables' worth: a full stop
/// (inside a sentence Kokoro joined) more than a comma; none at the end.
fn pause_after(chars: &[char], spans: &[Span]) -> Vec<f64> {
    (0..spans.len())
        .map(|i| {
            let Some(next) = spans.get(i + 1) else { return 0.0 };
            let between: String = chars[spans[i].1..next.0].iter().collect();
            if between.contains(['.', '!', '?', '…']) {
                4.0
            } else if between.contains([',', ';', ':', '—', '–', '(', ')']) {
                2.5
            } else if between.trim().len() > 1 {
                // A dash or a quote standing alone between words.
                1.2
            } else {
                0.0
            }
        })
        .collect()
}

/// Little words said quickly ("and", "it"…): their weight is cut.
const QUICK: &[&str] = &[
    "a", "an", "and", "the", "of", "to", "in", "on", "at", "it", "is", "as", "or", "but", "if", "for", "by", "be", "we", "he",
    "she", "i", "you", "my", "me", "so", "do", "no", "up", "its", "was", "are", "has", "had", "his", "her", "our", "can",
];

/// Each word's weight: its syllables and its letters (a cluster of
/// consonants takes time too), a little word's cut — fitted to Windows' own
/// word cues on a voice (`windows_word_cues_on_this_pc`).
fn word_weights(chars: &[char], spans: &[Span]) -> Vec<f64> {
    spans
        .iter()
        .map(|&s| {
            let w = slice(chars, s);
            let letters = w.chars().filter(|c| c.is_alphanumeric()).count() as f64;
            let weight = syllables(&w) as f64 + 0.2 * letters;
            if QUICK.contains(&w.to_lowercase().as_str()) { weight * 0.6 } else { weight }
        })
        .collect()
}

/// Words `range` spread over `[t0, t1]` by weight (each word and the pause
/// after it, the last word's pause excepted): when each starts.
fn spread(weights: &[f64], pauses: &[f64], range: std::ops::Range<usize>, t0: Duration, t1: Duration) -> Vec<Duration> {
    let n = range.len();
    if n == 0 {
        return Vec::new();
    }
    let last = range.end - 1;
    let total: f64 = range.clone().map(|i| weights[i] + if i < last { pauses[i] } else { 0.0 }).sum();
    let span = t1.saturating_sub(t0).as_secs_f64();
    let mut at = 0.0;
    let mut out = Vec::with_capacity(n);
    for i in range {
        out.push(t0 + Duration::from_secs_f64(if total > 0.0 { span * at / total } else { 0.0 }));
        at += weights[i] + if i < last { pauses[i] } else { 0.0 };
    }
    out
}

/// When each word starts, estimated: the sentence's words spread over
/// `speech` from `lead` by their syllables and the pauses their punctuation
/// makes. Pure.
pub fn estimate(text: &str, spans: &[Span], lead: Duration, speech: Duration) -> Vec<Duration> {
    let chars: Vec<char> = text.chars().collect();
    let weights = word_weights(&chars, spans);
    let pauses = pause_after(&chars, spans);
    spread(&weights, &pauses, 0..spans.len(), lead, lead + speech)
}

/// What a sentence's audio is like: where the speech starts and ends, and
/// the pauses in it.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub start: Duration,
    pub end: Duration,
    /// Silences of at least [`MIN_PAUSE_MS`] inside the speech: (from, to).
    pub pauses: Vec<(Duration, Duration)>,
}

/// The analysis frame.
const FRAME_MS: usize = 10;
/// A pause is at least this long (the gaps inside words are shorter).
pub const MIN_PAUSE_MS: usize = 120;

/// Where the speech is in `samples` (interleaved, `channels`), by the
/// loudness of each 10 ms: quiet means under 8 % of a loud frame's (the
/// 90th percentile), and at least -48 dBFS. `None`: no speech in it.
pub fn shape(samples: &[f32], rate: u32, channels: u16) -> Option<Shape> {
    let ch = usize::from(channels.max(1));
    let frame = (rate as usize * FRAME_MS / 1000).max(1) * ch;
    let rms: Vec<f32> = samples
        .chunks(frame)
        .map(|f| (f.iter().map(|s| s * s).sum::<f32>() / f.len().max(1) as f32).sqrt())
        .collect();
    if rms.is_empty() {
        return None;
    }
    let mut sorted = rms.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let loud = sorted[(sorted.len() * 9 / 10).min(sorted.len() - 1)];
    let floor = (loud * 0.08).max(0.004);
    if loud < floor {
        return None;
    }
    let speaking: Vec<bool> = rms.iter().map(|&r| r >= floor).collect();
    let first = speaking.iter().position(|&s| s)?;
    let last = speaking.iter().rposition(|&s| s)?;
    let at = |frames: usize| Duration::from_millis((frames * FRAME_MS) as u64);
    let mut pauses = Vec::new();
    let mut run_start = None;
    for (i, &s) in speaking.iter().enumerate().take(last + 1).skip(first) {
        match (s, run_start) {
            (false, None) => run_start = Some(i),
            (true, Some(from)) => {
                if (i - from) * FRAME_MS >= MIN_PAUSE_MS {
                    pauses.push((at(from), at(i)));
                }
                run_start = None;
            }
            _ => {}
        }
    }
    Some(Shape { start: at(first), end: at(last + 1), pauses })
}

/// When each word starts, from the sentence's audio: spread over the speech
/// in it ([`shape`]), each pause in the audio pinned to the punctuation
/// whose estimated place is nearest (in order; a pause near none is left
/// alone), the words between two pins spread between them. Falls back to
/// [`estimate`] over the whole length when there's no speech in it. Pure.
pub fn from_audio(text: &str, spans: &[Span], samples: &[f32], rate: u32, channels: u16) -> (Vec<Duration>, Timing) {
    let frames = samples.len() / usize::from(channels.max(1));
    let length = Duration::from_secs_f64(frames as f64 / f64::from(rate.max(1)));
    let Some(shape) = shape(samples, rate, channels) else {
        return (estimate(text, spans, Duration::ZERO, length), Timing::Estimate);
    };
    (pinned(text, spans, &shape), Timing::Audio)
}

/// [`from_audio`]'s spread and pins, given the audio's [`Shape`].
pub fn pinned(text: &str, spans: &[Span], shape: &Shape) -> Vec<Duration> {
    let chars: Vec<char> = text.chars().collect();
    let n = spans.len();
    if n == 0 {
        return Vec::new();
    }
    let weights = word_weights(&chars, spans);
    let pauses = pause_after(&chars, spans);
    let first_guess = spread(&weights, &pauses, 0..n, shape.start, shape.end);
    // Where each punctuation pause is, by the first guess: from the end of
    // word b (its start + its share) to the start of word b + 1.
    let total: f64 = (0..n).map(|i| weights[i] + if i + 1 < n { pauses[i] } else { 0.0 }).sum();
    let per = shape.end.saturating_sub(shape.start).as_secs_f64() / total.max(1e-9);
    let guess_mid = |b: usize| {
        let end_b = first_guess[b].as_secs_f64() + weights[b] * per;
        (end_b + first_guess[b + 1].as_secs_f64()) / 2.0
    };
    let tolerance = (shape.end.saturating_sub(shape.start).as_secs_f64() * 0.2).max(0.6);
    // Pins: (the word after the pause, the pause).
    let mut pins: Vec<(usize, (Duration, Duration))> = Vec::new();
    let mut next_free = 0;
    for &(from, to) in &shape.pauses {
        let mid = (from.as_secs_f64() + to.as_secs_f64()) / 2.0;
        let best = (next_free..n.saturating_sub(1))
            .filter(|&b| pauses[b] > 0.0)
            .map(|b| (b, (guess_mid(b) - mid).abs()))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        if let Some((b, off)) = best {
            if off <= tolerance {
                pins.push((b + 1, (from, to)));
                next_free = b + 1;
            }
        }
    }
    let mut out = Vec::with_capacity(n);
    let mut word = 0;
    let mut t0 = shape.start;
    for &(next, (from, to)) in &pins {
        out.extend(spread(&weights, &pauses, word..next, t0, from));
        word = next;
        t0 = to;
    }
    out.extend(spread(&weights, &pauses, word..n, t0, shape.end));
    out
}

/// When each word starts, from the engine's [`CharTime`]s over the same
/// text: a word's first timed character in it. A word with none is put
/// between its neighbours. `None` when under half the words have a time
/// (the timing doesn't fit this text). Pure.
pub fn from_chars(spans: &[Span], times: &[CharTime]) -> Option<Vec<Duration>> {
    if spans.is_empty() {
        return Some(Vec::new());
    }
    let found: Vec<Option<Duration>> = spans
        .iter()
        .map(|&(a, b)| times.iter().filter(|t| t.at >= a && t.at < b).min_by_key(|t| t.at).map(|t| t.start))
        .collect();
    if found.iter().filter(|f| f.is_some()).count() * 2 < spans.len() {
        return None;
    }
    let mut out: Vec<Duration> = Vec::with_capacity(spans.len());
    for i in 0..spans.len() {
        let t = match found[i] {
            Some(t) => t,
            None => {
                let before = out.last().copied().unwrap_or(Duration::ZERO);
                let after = found[i + 1..].iter().flatten().next().copied().unwrap_or(before);
                before + after.saturating_sub(before) / 2
            }
        };
        // Never backwards.
        out.push(t.max(out.last().copied().unwrap_or(Duration::ZERO)));
    }
    Some(out)
}

/// A word as compared ([`align`]): lower case, letters and digits only.
fn key(chars: &[char], span: Span) -> String {
    slice(chars, span).chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Which shown word each said word belongs to: the longest run of words the
/// same on both sides, in order (LCS); said words between two of those go
/// to the shown words between them (spread evenly), or to the shown word
/// before when there are none there. Pure.
pub fn align(shown: &str, shown_spans: &[Span], said: &str, said_spans: &[Span]) -> Vec<Option<usize>> {
    let sc: Vec<char> = shown.chars().collect();
    let pc: Vec<char> = said.chars().collect();
    let a: Vec<String> = shown_spans.iter().map(|&s| key(&sc, s)).collect();
    let b: Vec<String> = said_spans.iter().map(|&s| key(&pc, s)).collect();
    let (n, m) = (a.len(), b.len());
    // LCS table, from the ends.
    let mut l = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            l[i][j] = if a[i] == b[j] && !a[i].is_empty() { l[i + 1][j + 1] + 1 } else { l[i + 1][j].max(l[i][j + 1]) };
        }
    }
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] && !a[i].is_empty() {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else if l[i + 1][j] >= l[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    let mut out = vec![None; m];
    for &(si, pj) in &pairs {
        out[pj] = Some(si);
    }
    // The gaps: said words between two pairs (or before the first, after
    // the last) go to the shown words between them.
    let mut bounds: Vec<(isize, isize)> = vec![(-1, -1)];
    bounds.extend(pairs.iter().map(|&(s, p)| (s as isize, p as isize)));
    bounds.push((n as isize, m as isize));
    for w in bounds.windows(2) {
        let ((s0, p0), (s1, p1)) = (w[0], w[1]);
        let said_gap: Vec<usize> = ((p0 + 1)..p1).map(|x| x as usize).collect();
        if said_gap.is_empty() {
            continue;
        }
        let shown_gap: Vec<usize> = ((s0 + 1)..s1).map(|x| x as usize).collect();
        for (k, &pj) in said_gap.iter().enumerate() {
            out[pj] = if shown_gap.is_empty() {
                (s0 >= 0).then_some(s0 as usize)
            } else {
                Some(shown_gap[k * shown_gap.len() / said_gap.len()])
            };
        }
    }
    out
}

/// The shown words' starts, from the said words' starts and [`align`]'s
/// map: a shown word starts when the first said word of its starts; the
/// list goes forwards in time (a word that would start before the one
/// before it is left out). Pure.
pub fn shown_starts(map: &[Option<usize>], said_starts: &[Duration]) -> Vec<(usize, Duration)> {
    let mut out: Vec<(usize, Duration)> = Vec::new();
    for (j, shown) in map.iter().enumerate() {
        let (Some(w), Some(&t)) = (shown, said_starts.get(j)) else { continue };
        match out.iter_mut().find(|(x, _)| x == w) {
            Some(entry) => entry.1 = entry.1.min(t),
            None => out.push((*w, t)),
        }
    }
    out.sort_by_key(|&(w, t)| (t, w));
    let mut kept: Vec<(usize, Duration)> = Vec::with_capacity(out.len());
    for (w, t) in out {
        if kept.last().is_none_or(|&(lw, _)| w > lw) {
            kept.push((w, t));
        }
    }
    kept
}

/// The word being said `at` into a sentence's audio: the last one started
/// by then. Pure.
pub fn word_at(starts: &[(usize, Duration)], at: Duration) -> Option<usize> {
    starts.iter().take_while(|(_, t)| *t <= at).last().map(|&(w, _)| w)
}

/// A UTF-16 offset into `text` as a char index (Windows' word cues count
/// UTF-16 units). Past the end: the end.
pub fn utf16_to_char(text: &str, units: usize) -> usize {
    let mut seen = 0;
    for (i, c) in text.chars().enumerate() {
        if seen >= units {
            return i;
        }
        seen += c.len_utf16();
    }
    text.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn texts(text: &str) -> Vec<String> {
        let c: Vec<char> = text.chars().collect();
        words(text).into_iter().map(|s| slice(&c, s)).collect()
    }

    #[test]
    fn words_leave_their_punctuation_out() {
        assert_eq!(texts("Hello, world! It's (really) “fine” — ok…"), ["Hello", "world", "It's", "really", "fine", "ok"]);
        assert_eq!(texts("  £12,400 on 18 Oct. "), ["12,400", "on", "18", "Oct"]);
        assert_eq!(texts("— … !!"), Vec::<String>::new());
        assert_eq!(texts("Café naïve 東京"), ["Café", "naïve", "東京"]);
        // Char offsets, not bytes.
        assert_eq!(words("é a"), vec![(0, 1), (2, 3)]);
    }

    #[test]
    fn syllables_roughly() {
        for (w, n) in [("the", 1), ("cake", 1), ("water", 2), ("beautiful", 3), ("table", 2), ("API", 3), ("I", 1), ("rhythm", 1), ("2026", 4), ("a", 1)] {
            assert_eq!(syllables(w), n, "{w}");
        }
    }

    #[test]
    fn an_estimate_spreads_words_by_syllables_and_pauses() {
        let text = "Hi there, beautiful world.";
        let spans = words(text);
        let t = estimate(text, &spans, ms(100), ms(2_000));
        assert_eq!(t.len(), 4);
        assert_eq!(t[0], ms(100));
        assert!(t.windows(2).all(|w| w[0] < w[1]), "{t:?}");
        // The comma's pause: the gap before "beautiful" is longer than
        // "Hi"'s share alone would make it.
        let hi = t[1] - t[0];
        let there = t[2] - t[1];
        assert!(there > hi, "{t:?}");
        assert!(t[3] < ms(2_100));
        assert!(estimate("", &[], ms(0), ms(1_000)).is_empty());
    }

    /// A sentence's audio: tone where `speech` says (seconds), silence
    /// elsewhere, at 24 kHz mono.
    fn audio(total: f64, speech: &[(f64, f64)]) -> Vec<f32> {
        let rate = 24_000.0;
        (0..(total * rate) as usize)
            .map(|i| {
                let t = i as f64 / rate;
                if speech.iter().any(|&(a, b)| t >= a && t < b) {
                    (t * 2.0 * std::f64::consts::PI * 220.0).sin() as f32 * 0.3
                } else {
                    0.0
                }
            })
            .collect()
    }

    #[test]
    fn the_shape_finds_the_speech_and_its_pauses() {
        let a = audio(3.0, &[(0.2, 1.0), (1.3, 2.6)]);
        let s = shape(&a, 24_000, 1).unwrap();
        assert_eq!((s.start, s.end), (ms(200), ms(2_600)));
        assert_eq!(s.pauses, vec![(ms(1_000), ms(1_300))]);
        // A gap shorter than a pause isn't one.
        let a = audio(2.0, &[(0.0, 0.9), (0.98, 1.8)]);
        assert!(shape(&a, 24_000, 1).unwrap().pauses.is_empty());
        // Silence: nothing to go by.
        assert_eq!(shape(&vec![0.0; 48_000], 24_000, 1), None);
        // Stereo counts frames, not samples.
        let mono = audio(1.0, &[(0.5, 1.0)]);
        let stereo: Vec<f32> = mono.iter().flat_map(|&s| [s, s]).collect();
        assert_eq!(shape(&stereo, 24_000, 2).unwrap().start, ms(500));
    }

    #[test]
    fn pauses_pin_the_punctuation_they_belong_to() {
        // "One two three, four five." with a long first half: the plain
        // spread would put "four" near 1.3 s; the pause says 2.0 s.
        let text = "One two three, four five.";
        let spans = words(text);
        let a = audio(3.0, &[(0.1, 1.8), (2.0, 2.9)]);
        let (t, how) = from_audio(text, &spans, &a, 24_000, 1);
        assert_eq!(how, Timing::Audio);
        assert_eq!(t[0], ms(100));
        assert_eq!(t[3], ms(2_000), "{t:?}");
        assert!(t[1] > t[0] && t[2] > t[1] && t[2] < ms(1_800) && t[4] > t[3], "{t:?}");
        // A pause with no punctuation near it is left alone.
        let text = "One two three four five six seven eight.";
        let spans = words(text);
        let a = audio(3.0, &[(0.0, 0.3), (0.5, 3.0)]);
        let (t, _) = from_audio(text, &spans, &a, 24_000, 1);
        assert!(t.windows(2).all(|w| w[0] < w[1]), "{t:?}");
        // Silent audio (test mode's voice): the length alone.
        let (t, how) = from_audio("Hello there.", &words("Hello there."), &vec![0.0; 24_000], 24_000, 1);
        assert_eq!(how, Timing::Estimate);
        assert_eq!(t[0], ms(0));
    }

    #[test]
    fn engine_char_times_become_word_starts() {
        let text = "Read this aloud";
        let spans = words(text);
        // ElevenLabs: every character.
        let every: Vec<CharTime> = text.chars().enumerate().map(|(i, _)| CharTime { at: i, start: ms(i as u64 * 50) }).collect();
        assert_eq!(from_chars(&spans, &every).unwrap(), vec![ms(0), ms(250), ms(500)]);
        // Windows: one cue per word, one missing — put between its neighbours.
        let cues = [CharTime { at: 0, start: ms(0) }, CharTime { at: 10, start: ms(600) }];
        assert_eq!(from_chars(&spans, &cues).unwrap(), vec![ms(0), ms(300), ms(600)]);
        // Under half the words timed: it doesn't fit this text.
        assert_eq!(from_chars(&spans, &[CharTime { at: 0, start: ms(0) }]), None);
        assert_eq!(from_chars(&[], &[]), Some(vec![]));
    }

    #[test]
    fn said_words_map_back_to_the_shown_ones() {
        let shown = "It came to £12,400 on 18 Oct.";
        let said = "It came to twelve thousand four hundred pounds on the eighteenth of October.";
        let (ss, ps) = (words(shown), words(said));
        let map = align(shown, &ss, said, &ps);
        let sc: Vec<char> = shown.chars().collect();
        let named: Vec<String> = map.iter().map(|m| m.map(|i| slice(&sc, ss[i])).unwrap_or_default()).collect();
        assert_eq!(
            named,
            ["It", "came", "to", "12,400", "12,400", "12,400", "12,400", "12,400", "on", "18", "18", "Oct", "Oct"]
        );
        // Starts: each shown word with its first said word's.
        let starts: Vec<Duration> = (0..ps.len()).map(|j| ms(j as u64 * 100)).collect();
        let got = shown_starts(&map, &starts);
        assert_eq!(got, vec![(0, ms(0)), (1, ms(100)), (2, ms(200)), (3, ms(300)), (4, ms(800)), (5, ms(900)), (6, ms(1_100))]);
        assert_eq!(word_at(&got, ms(450)), Some(3));
        assert_eq!(word_at(&got, ms(0)), Some(0));
        assert_eq!(word_at(&[(0, ms(100))], ms(50)), None);
        // The same text both sides: one to one.
        let s = "Plain words only";
        let w = words(s);
        assert_eq!(align(s, &w, s, &w), vec![Some(0), Some(1), Some(2)]);
        // Said words before anything matches go nowhere (no shown word before).
        let m = align("Friday", &words("Friday"), "on Friday", &words("on Friday"));
        assert_eq!(m, vec![None, Some(0)]);
    }

    #[test]
    fn utf16_offsets_become_chars() {
        let s = "a😀b c";
        assert_eq!(utf16_to_char(s, 0), 0);
        assert_eq!(utf16_to_char(s, 1), 1);
        assert_eq!(utf16_to_char(s, 3), 2);
        assert_eq!(utf16_to_char(s, 5), 4);
        assert_eq!(utf16_to_char(s, 99), 5);
    }
}

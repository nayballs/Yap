//! Sliding-window partial-transcript stitching for the live preview.
//!
//! The streaming-partials worker (`pipeline::stream_partials`) re-transcribes
//! only a bounded tail **window** of the growing recording buffer each tick.
//! This module owns the stitching: audio older than the window is frozen as
//! `committed` text, and the window's words are shown **for good once two
//! decodes in a row agree on them** (LocalAgreement, as in
//! whisper_streaming): only the last words, still being said, can change.
//! `display()` joins the three. Pure logic — no engine, no audio I/O — so it
//! unit-tests in the default (stub) build.
//!
//! Why (2026-10-26, a tester's recordings replayed with `yap.exe
//! partials-dump`, `dump.rs`): every decode of the window changes its mind
//! somewhere — an "um" in or out, "Claude." / "Claude,", "haiku" / "hiking"
//! / "haikyu" — and the old stitching (FluidVoice's `smartDiffUpdate`) kept
//! only the words before the first change, so one early change rewrote the
//! whole line: ~10 shown words changed on every update of a 36 s dictation,
//! and the bar ran back and forth.
//!
//! Invariants:
//! - `committed` covers exactly `buffer[..window_start]`, the window's words
//!   `buffer[window_start..]`.
//! - Words shown as `committed` or `confirmed` never change or go: the line
//!   only grows, except for its unconfirmed last words (`pending`).
//! - Per-tick transcription cost is bounded by `WINDOW_HARD_MAX_SECS` no
//!   matter how long the recording grows.

use crate::dictation::media::quietest_index;

/// `yap.exe partials-dump` (debug builds): the live preview replayed on a recording.
#[cfg(debug_assertions)]
pub mod dump;

/// Window length beyond which we try to advance (freeze the oldest audio).
const WINDOW_ADVANCE_SECS: usize = 12;
/// Tail retained after an advance.
const WINDOW_KEEP_SECS: usize = 8;
/// Width of the quiet-cut search region (ends at the keep boundary).
const CUT_SLACK_SECS: usize = 2;
/// A cut point must be at least this quiet (peak amplitude) …
const QUIET_AMP_MAX: f32 = 0.02;
/// … unless the window reaches this hard cap — then cut regardless, even
/// mid-speech (bounds per-tick cost; the final pass fixes any seam artifact).
const WINDOW_HARD_MAX_SECS: usize = 20;

/// 16 kHz mono — the pipeline's fixed capture rate.
const RATE: usize = 16_000;

/// What the worker should transcribe this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickPlan {
    /// Transcribe `buffer[window_start..]` as one window.
    Normal,
    /// Window grew too long: transcribe `buffer[window_start..cut]` one last
    /// time (it becomes committed text), then `buffer[cut..]` as the new tail.
    /// `cut` is an absolute sample index into the recording buffer.
    Advance { cut: usize },
}

/// One recording session's partial-transcript state.
#[derive(Default)]
pub struct PartialSession {
    /// Frozen text of `buffer[..window_start]` — never changes once folded in.
    committed: String,
    /// The window's words two decodes in a row agreed on, as first shown —
    /// never change.
    confirmed: Vec<String>,
    /// The latest decode's words after them: shown, but may still change.
    pending: Vec<String>,
    /// Decodes in a row that couldn't be lined up with `confirmed`.
    misses: usize,
    /// Absolute sample index where the active window starts.
    pub window_start: usize,
}

/// Decodes in a row that may be passed over for not lining up with the
/// confirmed words ([`PartialSession::apply_normal`]).
const MAX_MISSES: usize = 2;

impl PartialSession {
    /// Decide this tick's plan from the buffer length + amplitude data.
    /// Only the small cut-search region of `samples` is ever scanned.
    pub fn plan(&self, len: usize, samples: &[f32]) -> TickPlan {
        let window = len.saturating_sub(self.window_start);
        if window <= WINDOW_ADVANCE_SECS * RATE {
            return TickPlan::Normal;
        }
        // Quietest sample (10 ms stride) in the 2 s region just before the
        // keep boundary — the same poor-man's silence pick the upload chunker
        // uses, so cuts land in pauses instead of mid-word.
        let hi = len - WINDOW_KEEP_SECS * RATE;
        let lo = hi
            .saturating_sub(CUT_SLACK_SECS * RATE)
            .max(self.window_start + 1);
        let (cut, amp) = quietest_index(samples, lo, hi, RATE);
        if amp <= QUIET_AMP_MAX || window >= WINDOW_HARD_MAX_SECS * RATE {
            TickPlan::Advance { cut }
        } else {
            // No pause found yet — let the window grow toward the hard max.
            TickPlan::Normal
        }
    }

    /// Regular tick: the window's new decode. Where the confirmed words end
    /// in it is found by their last words ([`find_end`]); what follows them
    /// is compared with the last decode's — the words both agree on from the
    /// start are confirmed (as they were shown), the rest is pending. A
    /// decode whose words can't be lined up with the confirmed ones (the
    /// model heard them differently, given more audio) changes nothing — up
    /// to [`MAX_MISSES`] in a row, then it's lined up by their count, so the
    /// line never stops growing.
    pub fn apply_normal(&mut self, text: &str) {
        let words = speech_words(text);
        let from = match find_end(&self.confirmed, &words) {
            Some(from) => {
                self.misses = 0;
                from
            }
            None if self.misses < MAX_MISSES => {
                self.misses += 1;
                return;
            }
            None => {
                self.misses = 0;
                self.confirmed.len().min(words.len())
            }
        };
        let tail = &words[from..];
        let agreed = self.pending.iter().zip(tail).take_while(|(a, b)| same_word(a, b)).count();
        let shown = std::mem::take(&mut self.pending);
        self.confirmed.extend(shown.into_iter().take(agreed));
        self.pending = tail[agreed..].to_vec();
    }

    /// Advance tick: `commit_text` is the final transcription of
    /// `buffer[window_start..cut]`, `tail_text` the first of the fresh window
    /// starting at `cut`. The words already shown, up to where the commit's
    /// end among them, are frozen as they were shown (the line never changes
    /// at a seam); those after the cut stay (the confirmed ones confirmed)
    /// and meet the new window's decode. Nothing shown yet: the commit's
    /// words. (Freezing everything shown at an empty commit, as before,
    /// showed the window's words twice once the new window decoded them.)
    pub fn apply_advance(&mut self, commit_text: &str, tail_text: &str, cut: usize) {
        let shown: Vec<String> = self.confirmed.iter().chain(&self.pending).cloned().collect();
        let commit = speech_words(commit_text);
        // Where the line splits at the cut: just after the commit's last
        // words; if they can't be found, after as many words as it has (an
        // empty commit — a quiet stretch, a lone "um" — none: everything
        // shown is after the cut, and the new window's decode has it).
        let split_at = |commit: &[String]| find_end(commit, &shown).unwrap_or(commit.len().min(shown.len()));
        let (frozen, kept_confirmed, kept_pending) = if shown.is_empty() {
            (commit, Vec::new(), Vec::new())
        } else if let Some(from) = find_end(&shown, &commit).filter(|&f| f > 0 && f < commit.len() && find_end(&commit, &shown).is_none()) {
            // The line hadn't got as far as the cut: all of it, then the
            // commit's words after where it ends.
            (shown.iter().cloned().chain(commit[from..].iter().cloned()).collect(), Vec::new(), Vec::new())
        } else {
            let split = split_at(&commit);
            let confirmed_end = self.confirmed.len().max(split);
            (shown[..split].to_vec(), shown[split..confirmed_end].to_vec(), shown[confirmed_end..].to_vec())
        };
        if !frozen.is_empty() {
            if !self.committed.is_empty() {
                self.committed.push(' ');
            }
            self.committed.push_str(&frozen.join(" "));
        }
        self.window_start = cut;
        self.confirmed = kept_confirmed;
        self.pending = kept_pending;
        self.apply_normal(tail_text);
    }

    /// The full partial line to show: committed prefix + the window's words.
    pub fn display(&self) -> String {
        let window = self.confirmed.iter().chain(&self.pending).map(String::as_str).collect::<Vec<_>>().join(" ");
        match (self.committed.is_empty(), window.is_empty()) {
            (true, _) => window,
            (_, true) => self.committed.clone(),
            _ => format!("{} {}", self.committed, window),
        }
    }
}

/// A word as compared: lower case, without the punctuation around it
/// ("Claude." and "claude," are the same word).
fn norm(w: &str) -> String {
    w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()
}

fn same_word(a: &str, b: &str) -> bool {
    norm(a) == norm(b)
}

/// Filler sounds the model writes in one decode and not the next: never
/// shown in the preview (the most changeable words of all; the final pass
/// decides about them).
const FILLERS: [&str; 6] = ["um", "umm", "uh", "uhh", "erm", "hmm"];

/// A decode's words, without its filler sounds. Pure.
fn speech_words(text: &str) -> Vec<String> {
    text.split_whitespace().filter(|w| !FILLERS.contains(&norm(w).as_str())).map(str::to_string).collect()
}

/// Where `known` (words already settled) ends in `words` (a new decode of
/// the same audio): just after the place where `known`'s last words — up to three —
/// match best, a majority of them at least: a place where the very last
/// word matches first ("look at it, it says": "at it says" ends at "says",
/// not at the first "it"), then the most words, then the nearest to where
/// `known`'s length says. Empty `known`: 0. Pure.
fn find_end(known: &[String], words: &[String]) -> Option<usize> {
    if known.is_empty() {
        return Some(0);
    }
    let m = known.len().min(3);
    let key = &known[known.len() - m..];
    let expected = known.len();
    // (the last word matches, matches, distance from `expected`, end)
    let mut best: Option<(bool, usize, usize, usize)> = None;
    for end in 1..=words.len() {
        let matches = (0..m)
            .filter(|&i| (end + i).checked_sub(m).is_some_and(|j| same_word(&words[j], &key[i])))
            .count();
        if matches * 2 <= m {
            continue;
        }
        let last = same_word(&words[end - 1], &key[m - 1]);
        let distance = end.abs_diff(expected);
        let better = best.is_none_or(|(bl, bm, bd, _)| (last, matches, std::cmp::Reverse(distance)) > (bl, bm, std::cmp::Reverse(bd)));
        if better {
            best = Some((last, matches, distance, end));
        }
    }
    best.map(|(_, _, _, end)| end)
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: usize = RATE;

    fn loud(secs: usize) -> Vec<f32> {
        vec![0.5f32; secs * R]
    }

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    // ---- agreeing decodes ----

    #[test]
    fn a_growing_transcript_grows_and_agreed_words_are_confirmed() {
        let mut s = PartialSession::default();
        s.apply_normal("the meeting is");
        assert_eq!(s.display(), "the meeting is");
        s.apply_normal("the meeting is at three");
        assert_eq!(s.display(), "the meeting is at three");
        assert_eq!(s.confirmed, words("the meeting is"));
        assert_eq!(s.pending, words("at three"));
    }

    #[test]
    fn a_word_once_agreed_never_changes_however_the_model_spells_it_next() {
        // A tester's dictation, in fictional words: each decode of the window
        // changes its mind about an early word.
        let mut s = PartialSession::default();
        s.apply_normal("Sam. The streaming works.");
        s.apply_normal("Sam. The streaming works well.");
        assert_eq!(s.display(), "Sam. The streaming works. well.");
        // "Sam," now, and "work" — the shown words stay as they were.
        s.apply_normal("Sam, the streaming work well. It likes haiku.");
        assert!(s.display().starts_with("Sam. The streaming works."), "{}", s.display());
        assert!(s.display().ends_with("It likes haiku."), "{}", s.display());
        // A word in the middle heard differently never takes the line back.
        s.apply_normal("Sam, the streaming works well. It likes hiking.");
        s.apply_normal("Sam, the streaming works well. It likes haikyu a lot.");
        assert!(s.display().starts_with("Sam. The streaming works. well. It likes"), "{}", s.display());
    }

    #[test]
    fn a_decode_that_does_not_line_up_changes_nothing_until_it_keeps_happening() {
        let mut s = PartialSession::default();
        s.apply_normal("Do you have any notes on");
        s.apply_normal("Do you have any notes on the launch");
        let line = s.display();
        // More audio, and the model hears the start differently: the line
        // keeps its words (they were going to be lost from the end).
        s.apply_normal("Do any new to son");
        assert_eq!(s.display(), line);
        s.apply_normal("Do any new to son the");
        assert_eq!(s.display(), line);
        // A third in a row: lined up by the words' count, so it still grows.
        s.apply_normal("Do any new to son the launch plan for");
        assert!(s.display().ends_with("plan for"), "{}", s.display());
        assert!(s.display().starts_with("Do you have any notes on"), "{}", s.display());
    }

    #[test]
    fn filler_sounds_never_show_so_they_cannot_come_and_go() {
        let mut s = PartialSession::default();
        s.apply_normal("Sam, um, the plan");
        assert_eq!(s.display(), "Sam, the plan");
        s.apply_normal("Sam, the plan is set");
        s.apply_normal("Sam, uh, the plan is set. Um");
        assert_eq!(s.display(), "Sam, the plan is set");
    }

    #[test]
    fn where_the_settled_words_end_in_a_new_decode() {
        let known = words("the plan is ready");
        // The same words, punctuated differently, with more after them.
        assert_eq!(find_end(&known, &words("The plan is ready, and Sam")), Some(4));
        // A word dropped before them: their last words still find the place.
        assert_eq!(find_end(&known, &words("plan is ready and Sam")), Some(3));
        // Nothing alike: not found.
        assert_eq!(find_end(&known, &words("totally different words here and more")), None);
        assert_eq!(find_end(&[], &words("anything")), Some(0));
        // One word of three alike isn't a match.
        assert_eq!(find_end(&known, &words("one two is three")), None);
        // "look at it, it says": "at it says" ends at "says", not one word
        // early at "at it, it" (two of three alike there too).
        let known = words("you look at it says");
        assert_eq!(find_end(&known, &words("if you look at it, it says it stopped")), Some(7));
    }

    #[test]
    fn a_seam_whose_words_dont_line_up_never_shows_words_twice() {
        let mut s = PartialSession::default();
        s.apply_normal("says says it stopped where the notes are");
        s.apply_normal("says says it stopped where the notes are kept");
        // The frozen part comes back as words found nowhere on the line: as
        // many words as it has are frozen, the rest stays and lines up.
        s.apply_advance("It says,", "it stopped where the notes are kept today", 3 * R);
        assert_eq!(s.display(), "says says it stopped where the notes are kept today");
    }

    // ---- window planning ----

    #[test]
    fn short_window_is_normal() {
        let s = PartialSession::default();
        let samples = loud(10);
        assert_eq!(s.plan(samples.len(), &samples), TickPlan::Normal);
    }

    #[test]
    fn advances_at_quiet_dip() {
        let s = PartialSession::default();
        let mut samples = loud(14); // window 14 s > 12 s advance threshold
        // Search region = [14−8−2 .. 14−8] = [4 s .. 6 s]; plant a dip at 5 s
        // (offset divisible by the 10 ms scan stride so it's hit exactly).
        let dip = 5 * R;
        samples[dip] = 0.0;
        assert_eq!(s.plan(samples.len(), &samples), TickPlan::Advance { cut: dip });
    }

    #[test]
    fn loud_window_defers_then_force_cuts() {
        let s = PartialSession::default();
        // 14 s of continuous loud speech: no quiet point → keep growing.
        let samples = loud(14);
        assert_eq!(s.plan(samples.len(), &samples), TickPlan::Normal);
        // At the 20 s hard max it cuts anyway (bounded per-tick cost).
        let samples = loud(20);
        assert!(matches!(
            s.plan(samples.len(), &samples),
            TickPlan::Advance { .. }
        ));
    }

    #[test]
    fn plan_respects_window_start() {
        // 30 s buffer but the window starts at 20 s → 10 s window → Normal.
        let s = PartialSession {
            window_start: 20 * R,
            ..Default::default()
        };
        let samples = loud(30);
        assert_eq!(s.plan(samples.len(), &samples), TickPlan::Normal);
    }

    // ---- stitching ----

    #[test]
    fn advance_keeps_committed_and_resets_diff_base() {
        let mut s = PartialSession::default();
        s.apply_normal("the quick brown fox");
        s.apply_advance("the quick", "brown fox jumps", 5 * R);
        assert_eq!(s.display(), "the quick brown fox jumps");
        assert_eq!(s.window_start, 5 * R);
        // A decode that's nothing alike changes nothing (once).
        s.apply_normal("totally different words here");
        assert_eq!(s.display(), "the quick brown fox jumps");
    }

    #[test]
    fn a_seam_freezes_the_words_as_shown_and_the_line_never_jumps() {
        let mut s = PartialSession::default();
        s.apply_normal("Sam. The streaming stuff works well and it");
        s.apply_normal("Sam. The streaming stuff works well and it likes");
        let before = s.display();
        // The frozen part decoded on its own comes back differently cased
        // and punctuated, the new window capitalised mid-sentence.
        s.apply_advance("Sam, the streaming stuff", "Works well and it likes haiku", 4 * R);
        assert_eq!(s.display(), format!("{before} haiku"));
        assert_eq!(s.committed, "Sam. The streaming stuff");
    }

    #[test]
    fn committed_survives_normal_advance_normal_sequence() {
        let mut s = PartialSession::default();
        s.apply_normal("one two");
        s.apply_advance("one two three", "four", 6 * R);
        s.apply_normal("four five");
        s.apply_advance("four five six", "seven", 12 * R);
        assert_eq!(s.display(), "one two three four five six seven");
    }

    #[test]
    fn empty_commit_text_adds_no_stray_spaces() {
        let mut s = PartialSession::default();
        s.apply_advance("", "hello", R); // silent pre-cut chunk
        assert_eq!(s.display(), "hello");
        s.apply_advance("hello", "world", 2 * R);
        assert_eq!(s.display(), "hello world");
        // A whitespace-only commit: the words shown are all after the cut,
        // and the new window's decode has them — never twice.
        s.apply_advance("   ", "world again", 3 * R);
        assert_eq!(s.display(), "hello world again");
        s.apply_advance("um", "world again and more", 4 * R);
        assert_eq!(s.display(), "hello world again and more");
    }
}

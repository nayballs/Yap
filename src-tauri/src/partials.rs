//! Sliding-window partial-transcript stitching for the live preview.
//!
//! The streaming-partials worker (`pipeline::stream_partials`) re-transcribes
//! only a bounded tail **window** of the growing recording buffer each tick.
//! This module owns the stitching: audio older than the window is frozen as
//! `committed` text, the active window's text is de-flickered with
//! [`smart_diff`], and `display()` joins the two. Pure logic — no engine, no
//! audio I/O — so it unit-tests in the default (stub) build.
//!
//! Invariants:
//! - `committed` covers exactly `buffer[..window_start]`, the tail exactly
//!   `buffer[window_start..]` — disjoint audio, so no overlap dedupe is needed.
//! - Per-tick transcription cost is bounded by `WINDOW_HARD_MAX_SECS` no
//!   matter how long the recording grows.

use crate::media::quietest_index;

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
    /// smart_diff-stabilised text of the active window.
    tail: String,
    /// Absolute sample index where the active window starts.
    pub window_start: usize,
}

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

    /// Regular tick: de-flicker the window's new transcription into the tail.
    pub fn apply_normal(&mut self, text: &str) {
        self.tail = smart_diff(&self.tail, text.trim());
    }

    /// Advance tick: `commit_text` is the final transcription of
    /// `buffer[window_start..cut]` (folded into `committed`), `tail_text` the
    /// first transcription of the fresh window starting at `cut`. If the
    /// commit transcription came back empty while the tail was already showing
    /// words for that audio, the old tail is committed instead — the display
    /// must never shrink at an advance.
    pub fn apply_advance(&mut self, commit_text: &str, tail_text: &str, cut: usize) {
        let old_tail = std::mem::take(&mut self.tail);
        let commit = match commit_text.trim() {
            "" => old_tail.trim().to_string(),
            t => t.to_string(),
        };
        if !commit.is_empty() {
            if !self.committed.is_empty() {
                self.committed.push(' ');
            }
            self.committed.push_str(&commit);
        }
        self.window_start = cut;
        // Fresh window ⇒ fresh diff base (the old tail covered different audio).
        self.tail = tail_text.trim().to_string();
    }

    /// The full partial line to show: committed prefix + live tail.
    pub fn display(&self) -> String {
        match (self.committed.is_empty(), self.tail.is_empty()) {
            (true, _) => self.tail.clone(),
            (_, true) => self.committed.clone(),
            _ => format!("{} {}", self.committed, self.tail),
        }
    }
}

/// De-flicker successive partials over the same audio (FluidVoice
/// `smartDiffUpdate`). Keeps the stable longest-common **word** prefix of the
/// previous emit and appends the new tail, so the displayed text grows smoothly
/// instead of re-rendering wholesale. If the new transcript diverges from the
/// previous one by more than half its words, it's replaced outright (the
/// decode changed its mind). Word comparison ignores case and punctuation.
pub fn smart_diff(prev: &str, next: &str) -> String {
    let norm = |w: &str| {
        w.trim_matches(|c: char| !c.is_alphanumeric())
            .to_ascii_lowercase()
    };
    let pw: Vec<&str> = prev.split_whitespace().collect();
    let nw: Vec<&str> = next.split_whitespace().collect();

    let mut common = 0;
    while common < pw.len() && common < nw.len() && norm(pw[common]) == norm(nw[common]) {
        common += 1;
    }

    if !pw.is_empty() && (common as f32 / pw.len() as f32) >= 0.5 {
        let mut out: Vec<&str> = pw[..common].to_vec();
        out.extend_from_slice(&nw[common..]);
        out.join(" ")
    } else {
        next.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: usize = RATE;

    fn loud(secs: usize) -> Vec<f32> {
        vec![0.5f32; secs * R]
    }

    // ---- smart_diff (moved from pipeline.rs) ----

    #[test]
    fn growing_transcript_keeps_prefix_and_appends() {
        let out = smart_diff("the meeting is", "the meeting is at three");
        assert_eq!(out, "the meeting is at three");
    }

    #[test]
    fn from_empty_takes_next() {
        assert_eq!(smart_diff("", "hello world"), "hello world");
    }

    #[test]
    fn case_and_punctuation_insensitive_prefix() {
        // Prev tail lacked punctuation; new decode added it — prefix still stable.
        let out = smart_diff("lets go to the", "Let's go to the bank.");
        assert!(out.ends_with("bank."));
    }

    #[test]
    fn large_divergence_replaces_wholesale() {
        let out = smart_diff("alpha beta gamma delta", "totally different words here");
        assert_eq!(out, "totally different words here");
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
        let mut s = PartialSession::default();
        s.window_start = 20 * R;
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
        // A wholesale tail change replaces only the tail, never committed text.
        s.apply_normal("totally different words here");
        assert_eq!(s.display(), "the quick totally different words here");
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
        s.apply_advance("   ", "again", 3 * R); // whitespace-only commit
        assert_eq!(s.display(), "hello world again");
    }
}

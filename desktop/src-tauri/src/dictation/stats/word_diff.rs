//! What AI cleanup changed in a dictation, counted: the Rust twin of
//! `desktop/src/lib/core/wordDiff.js` (Home's "AI edited · N changes"). The words
//! as said (history's `said`) against the text Yap pasted, word by word —
//! a longest common subsequence of words, capitals and punctuation aside
//! (cleanup adds those to nearly every dictation). Same rules as the page:
//! a run of words taken out and/or put in is one change; a token that's
//! only punctuation is never one. Pure.

/// How much cleanup changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Diff {
    /// Places it changed words (Home's "N changes").
    pub changes: usize,
    /// Words it changed: in each change, the larger of the words taken out
    /// and the words put in ("um" out: 1; "need to" → "must": 2).
    pub words: usize,
}

/// A word as compared: lower case, letters, digits and apostrophes (curly
/// ones straightened), no apostrophe at either end.
pub fn word_key(token: &str) -> String {
    let kept: String = token
        .to_lowercase()
        .chars()
        .map(|c| if c == '\u{2018}' || c == '\u{2019}' { '\'' } else { c })
        .filter(|c| c.is_alphanumeric() || *c == '\'')
        .collect();
    kept.trim_matches('\'').to_string()
}

/// A middle bigger than this (words × words) isn't aligned word by word.
const MAX_CELLS: usize = 250_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Same,
    /// Taken out: its index in the words as said.
    Del(usize),
    /// Put in: its index in the cleaned text.
    Add(usize),
}

/// `a` against `b` as ops (word keys).
fn align(a: &[String], b: &[String]) -> Vec<Op> {
    let (n, m) = (a.len(), b.len());
    let mut ops = Vec::with_capacity(n + m);
    // Too long to align (minutes of dictation rewritten whole): one change.
    if n.saturating_mul(m) > MAX_CELLS {
        ops.extend((0..n).map(Op::Del));
        ops.extend((0..m).map(Op::Add));
        return ops;
    }
    // lcs[i][j]: the longest common run of a[i..] and b[j..] (flat).
    let w = m + 1;
    let mut lcs = vec![0u32; (n + 1) * w];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i * w + j] = if a[i] == b[j] {
                lcs[(i + 1) * w + j + 1] + 1
            } else {
                lcs[(i + 1) * w + j].max(lcs[i * w + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            ops.push(Op::Same);
            i += 1;
            j += 1;
        } else if lcs[(i + 1) * w + j] >= lcs[i * w + j + 1] {
            ops.push(Op::Del(i));
            i += 1;
        } else {
            ops.push(Op::Add(j));
            j += 1;
        }
    }
    ops.extend((i..n).map(Op::Del));
    ops.extend((j..m).map(Op::Add));
    ops
}

/// The words as said (`said`) against the cleaned text (`cleaned`).
pub fn diff(said: &str, cleaned: &str) -> Diff {
    let mut out = Diff::default();
    for (taken, put) in changes(said, cleaned) {
        out.changes += 1;
        out.words += taken.len().max(put.len());
    }
    out
}

/// Each change cleanup made, in order: the words taken out and the words
/// put in, as [`word_key`]s (punctuation-only tokens left out; a change of
/// none of either isn't one). What [`diff`] counts; the dictionary's check
/// on cleanup (`fuzzy::words_added`) reads which words a dictionary word
/// took the place of.
pub fn changes(said: &str, cleaned: &str) -> Vec<(Vec<String>, Vec<String>)> {
    let a: Vec<String> = said.split_whitespace().map(word_key).collect();
    let b: Vec<String> = cleaned.split_whitespace().map(word_key).collect();
    // The same start and end first: most of a dictation is.
    let mut start = 0;
    while start < a.len() && start < b.len() && a[start] == b[start] {
        start += 1;
    }
    let (mut end_a, mut end_b) = (a.len(), b.len());
    while end_a > start && end_b > start && a[end_a - 1] == b[end_b - 1] {
        end_a -= 1;
        end_b -= 1;
    }
    let (a, b) = (&a[start..end_a], &b[start..end_b]);
    let ops = align(a, b);
    let mut out = Vec::new();
    let mut k = 0;
    while k < ops.len() {
        if ops[k] == Op::Same {
            k += 1;
            continue;
        }
        // A change: the run up to the next word both share.
        let (mut taken, mut put) = (Vec::new(), Vec::new());
        while k < ops.len() && ops[k] != Op::Same {
            match ops[k] {
                Op::Del(i) if !a[i].is_empty() => taken.push(a[i].clone()),
                Op::Add(j) if !b[j].is_empty() => put.push(b[j].clone()),
                _ => {}
            }
            k += 1;
        }
        if !taken.is_empty() || !put.is_empty() {
            out.push((taken, put));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_compare_without_capitals_or_punctuation() {
        assert_eq!(word_key("Hello,"), "hello");
        assert_eq!(word_key("It\u{2019}s"), "it's");
        assert_eq!(word_key("'quoted'"), "quoted");
        assert_eq!(word_key("sign-in"), "signin");
        assert_eq!(word_key("—"), "");
        assert_eq!(word_key("Café"), "café");
    }

    #[test]
    fn counts_as_homes_ai_edited_does() {
        // desktop/e2e/home-ai-edits.spec.js's dictation: two changes, three words.
        let d = diff("we need to fix the login bug before the demo", "We must fix the sign-in bug before the demo.");
        assert_eq!(d, Diff { changes: 2, words: 3 });
        // Only punctuation and capitals: nothing.
        assert_eq!(diff("send it now", "Send it now."), Diff::default());
        // A filler out at the start; a dash put in is never a change.
        assert_eq!(diff("um so the plan is ready", "So — the plan is ready."), Diff { changes: 1, words: 1 });
        // Words put in only.
        assert_eq!(diff("call me", "Please call me back."), Diff { changes: 2, words: 2 });
        // Nothing said, or nothing left.
        assert_eq!(diff("", "Hi."), Diff { changes: 1, words: 1 });
        assert_eq!(diff("one two", ""), Diff { changes: 1, words: 2 });
        assert_eq!(diff("", ""), Diff::default());
    }

    #[test]
    fn a_dictation_too_long_to_align_is_one_change() {
        let said = vec!["alpha"; 600].join(" ");
        let cleaned = vec!["beta"; 500].join(" ");
        assert_eq!(diff(&said, &cleaned), Diff { changes: 1, words: 600 });
    }
}

//! **Long pastes into a coding CLI, in slices.** Claude Code folds a paste
//! longer than 800 characters, or with more than two line breaks, into
//! "[Pasted text #1 +12 lines]", and Codex one longer than 1,000 characters
//! into "[Pasted Content 1234 chars]" (both 2026, from their own code): the
//! dictation goes in whole, but as a blob the person can't read or fix
//! before sending it. So a long dictation goes into them in slices, each
//! one under the CLI's limit — each its own paste, a moment apart
//! ([`PACE_MS`]: the terminal reads the clipboard before the next slice
//! takes its place) — and shows as text.
//!
//! Windows Terminal itself stops a paste longer than 5 KiB with "You are
//! about to paste text that is longer than 5 KiB" (`largePasteWarning`, on
//! by default) and nothing arrives until it's answered (the robot: 4,004
//! characters arrive whole, 16,019 not at all), so any paste into it goes
//! in slices of at most 4,000.
//!
//! [`cut`] is pure and unit-tested: slices end after a space or a line
//! break where there is one (the next slice starts with a word), never
//! inside a character, and put back together they're exactly the text.
//! Every other app gets its paste in one piece.

use crate::pasting::app_kind::{AppKind, CodingCli, Host};

/// How long a slice may be: characters, and line breaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub chars: usize,
    pub breaks: usize,
}

/// Claude Code's own limits for showing a paste as text (2.1, its prompt
/// box: a paste of more than 800 characters or more than 2 line breaks
/// becomes "[Pasted text #N +L lines]").
pub const CLAUDE_CODE: Limits = Limits { chars: 800, breaks: 2 };
/// Codex's (0.7x, its composer: more than 1,000 characters becomes
/// "[Pasted Content N chars]"; line breaks don't count).
pub const CODEX: Limits = Limits { chars: 1_000, breaks: usize::MAX };
/// Windows Terminal's: under its 5 KiB large-paste warning.
pub const WINDOWS_TERMINAL: Limits = Limits { chars: 4_000, breaks: usize::MAX };

/// The pause between two slices: the terminal has read the clipboard
/// (Windows Terminal and conhost do within a few milliseconds) and the CLI
/// has taken the paste before the next slice goes on the clipboard.
pub const PACE_MS: u64 = 250;

/// The limits a paste into `kind` keeps to, if any (the kind as read,
/// before the settings): a coding CLI that folds long pastes, Windows
/// Terminal's large-paste warning — the stricter of the two.
pub fn limits(kind: AppKind) -> Option<Limits> {
    let AppKind::Terminal(t) = kind else { return None };
    let cli = match t.coding_cli {
        Some(CodingCli::ClaudeCode) => Some(CLAUDE_CODE),
        Some(CodingCli::Codex) => Some(CODEX),
        _ => None,
    };
    let host = (t.host == Host::WindowsTerminal).then_some(WINDOWS_TERMINAL);
    match (cli, host) {
        (Some(a), Some(b)) => Some(Limits { chars: a.chars.min(b.chars), breaks: a.breaks.min(b.breaks) }),
        (a, b) => a.or(b),
    }
}

/// `text` in slices within `limits` (one slice when it fits), lengths in
/// UTF-16 units as the CLIs count them: each as long as it can be, ending
/// after the last space or line break that keeps it in — after the last
/// line break when it's the line breaks that ran out —, a word cut only
/// when it alone is longer than a slice; never inside a character or a
/// "\r\n". Joined, the slices are `text`.
pub fn cut(text: &str, limits: Limits) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let fit = longest_fit(rest, limits.chars.max(2), limits.breaks);
        let at = if fit.end == rest.len() {
            fit.end
        } else if fit.by_breaks {
            fit.after_break.or(fit.after_space).unwrap_or(fit.end)
        } else {
            fit.after_break.max(fit.after_space).unwrap_or(fit.end)
        };
        // Never empty (no line break allowed and one comes first): it alone.
        let at = if at == 0 { first_len(rest) } else { at };
        out.push(&rest[..at]);
        rest = &rest[at..];
    }
    out
}

/// The longest start of a text within the limits ([`cut`]).
struct Fit {
    /// Its byte length.
    end: usize,
    /// It stopped at a line break the limits had no room for.
    by_breaks: bool,
    /// Right after its last line break, and its last other space.
    after_break: Option<usize>,
    after_space: Option<usize>,
}

fn longest_fit(s: &str, units: usize, breaks: usize) -> Fit {
    let (mut used, mut seen) = (0usize, 0usize);
    let (mut after_break, mut after_space) = (None, None);
    let mut chars = s.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        // "\r\n" goes as one: two units, one line break.
        let pair = c == '\r' && chars.peek().is_some_and(|&(_, n)| n == '\n');
        let is_break = c == '\r' || c == '\n';
        let width = if pair { 2 } else { c.len_utf16() };
        if used + width > units || (is_break && seen >= breaks) {
            return Fit { end: i, by_breaks: is_break && seen >= breaks, after_break, after_space };
        }
        used += width;
        let mut next = i + c.len_utf8();
        if pair {
            chars.next();
            next += 1;
        }
        if is_break {
            seen += 1;
            after_break = Some(next);
        } else if c.is_whitespace() {
            after_space = Some(next);
        }
    }
    Fit { end: s.len(), by_breaks: false, after_break, after_space }
}

/// The byte length of `s`'s first character ("\r\n" as one).
fn first_len(s: &str) -> usize {
    if s.starts_with("\r\n") {
        2
    } else {
        s.chars().next().map_or(s.len(), char::len_utf8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pasting::app_kind::{Host, Terminal};

    fn fits(slice: &str, l: Limits) -> bool {
        let breaks = slice.matches("\r\n").count() + slice.replace("\r\n", "").matches(['\r', '\n']).count();
        slice.encode_utf16().count() <= l.chars && breaks <= l.breaks
    }

    #[test]
    fn a_short_paste_is_one_slice() {
        assert_eq!(cut("Fix the login bug.", CLAUDE_CODE), vec!["Fix the login bug."]);
        assert_eq!(cut("One.\nTwo.\nThree.", CLAUDE_CODE), vec!["One.\nTwo.\nThree."]);
        assert!(cut("", CLAUDE_CODE).is_empty());
    }

    #[test]
    fn slices_end_after_a_space_and_join_back_exactly() {
        let words = "Please refactor the payment module so that retries back off. ".repeat(40);
        let slices = cut(&words, CLAUDE_CODE);
        assert!(slices.len() >= 3, "{}", slices.len());
        assert_eq!(slices.concat(), words);
        for s in &slices[..slices.len() - 1] {
            assert!(fits(s, CLAUDE_CODE), "{s:?}");
            assert!(s.ends_with(' '), "a slice ends after a space: {s:?}");
        }
        // Each as long as it can be: within a word of the limit.
        assert!(slices[0].chars().count() > 750, "{}", slices[0].chars().count());
    }

    #[test]
    fn claude_codes_line_breaks_are_counted_too() {
        let list = "Fix these:\n- the login bug\n- the slow search\n- the flaky test\n- the docs";
        let slices = cut(list, CLAUDE_CODE);
        // The third line break has no room: cut after the second (not mid-line).
        assert_eq!(slices, vec!["Fix these:\n- the login bug\n", "- the slow search\n- the flaky test\n- the docs"]);
        assert_eq!(slices.concat(), list);
        // "\r\n" is one break, never split.
        let crlf = "a\r\nb\r\nc\r\nd";
        let slices = cut(crlf, CLAUDE_CODE);
        assert_eq!(slices, vec!["a\r\nb\r\n", "c\r\nd"]);
        // Codex counts only characters.
        assert_eq!(cut(list, CODEX), vec![list]);
    }

    #[test]
    fn a_word_longer_than_a_slice_is_cut_and_characters_never_are() {
        let long = "é".repeat(1_900);
        let slices = cut(&long, CLAUDE_CODE);
        assert_eq!(slices.iter().map(|s| s.chars().count()).collect::<Vec<_>>(), vec![800, 800, 300]);
        assert_eq!(slices.concat(), long);
        // Two UTF-16 units each, as the CLIs count: 500 a slice.
        let emoji = "🦀".repeat(1_001);
        let slices = cut(&emoji, CODEX);
        assert_eq!(slices.iter().map(|s| s.chars().count()).collect::<Vec<_>>(), vec![500, 500, 1]);
        assert_eq!(slices.concat(), emoji);
    }

    #[test]
    fn every_cut_fits_and_joins_back() {
        let samples = [
            "word ".repeat(700),
            "a\nb\nc\nd\ne\nf\ng\n".repeat(30),
            format!("{}\n\n{}", "x".repeat(2_000), "y z ".repeat(300)),
            "\n\n\n\n".to_string(),
            "line one\r\nline two\r\n\r\nline three ".repeat(50),
        ];
        for text in &samples {
            for l in [CLAUDE_CODE, CODEX, Limits { chars: 7, breaks: 0 }] {
                let slices = cut(text, l);
                assert_eq!(slices.concat(), *text);
                for s in &slices {
                    assert!(!s.is_empty());
                    // (A lone line break is all a 0-break slice can hold.)
                    assert!(fits(s, l) || (l.breaks == 0 && s.trim().is_empty()), "{s:?} for {l:?}");
                }
            }
        }
    }

    #[test]
    fn the_clis_that_fold_a_paste_and_windows_terminal_get_slices() {
        let t = |cli, host| AppKind::Terminal(Terminal { coding_cli: cli, host });
        assert_eq!(limits(t(Some(CodingCli::ClaudeCode), Host::WindowsTerminal)), Some(CLAUDE_CODE));
        assert_eq!(limits(t(Some(CodingCli::ClaudeCode), Host::Console)), Some(CLAUDE_CODE));
        assert_eq!(limits(t(Some(CodingCli::ClaudeCode), Host::Editor)), Some(CLAUDE_CODE));
        assert_eq!(limits(t(Some(CodingCli::Codex), Host::WindowsTerminal)), Some(CODEX));
        // Windows Terminal's large-paste warning, whatever runs in it.
        assert_eq!(limits(t(Some(CodingCli::GeminiCli), Host::WindowsTerminal)), Some(WINDOWS_TERMINAL));
        assert_eq!(limits(t(None, Host::WindowsTerminal)), Some(WINDOWS_TERMINAL));
        assert_eq!(limits(t(None, Host::Console)), None);
        assert_eq!(limits(t(Some(CodingCli::GeminiCli), Host::Mintty)), None);
        assert_eq!(limits(AppKind::Other), None);
        assert_eq!(limits(AppKind::Messaging), None);
    }
}

//! **What a command printed**, for its row in a Code chat's thread (11 Nov
//! 2026, after Claude Code's own transcript, where a command's row opens to
//! the command and its output): the text of a `Bash` or `PowerShell` call's
//! result, as Claude Code hands it to the model. Pure.
//!
//! - **Its start and its end**: at most [`HEAD`] lines from the top and
//!   [`TAIL`] from the bottom (a test run's summary is at its end), the
//!   lines between counted (`more`); every line at most [`LINE_MAX`]
//!   characters; colour codes gone.
//! - **A turn's share**: [`TURN_MAX`] characters (the runner counts); past
//!   it a command keeps its row and no output.
//! - Goes in the agent's `yap-agent` event and its own log, as its commands
//!   and diffs do: never in Yap's own log, never to a phone (`link/code/
//!   view.rs` leaves its text out).

use serde::Serialize;

/// Lines kept from the top.
pub const HEAD: usize = 40;
/// Lines kept from the bottom.
pub const TAIL: usize = 20;
/// The most characters of a line.
pub const LINE_MAX: usize = 300;
/// The most characters of output one turn keeps.
pub const TURN_MAX: usize = 120_000;

/// **A command's output**, as its row shows it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Output {
    /// Its lines from the top (all of them when it's short).
    pub text: String,
    /// Its last lines, when lines between were left out.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub tail: String,
    /// Lines left out between the two.
    #[serde(skip_serializing_if = "is_zero")]
    pub more: usize,
    /// The command failed (Claude Code said its result is an error).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub failed: bool,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

impl Output {
    /// How many characters it keeps.
    pub fn chars(&self) -> usize {
        self.text.chars().count() + self.tail.chars().count()
    }
}

/// `s` without a terminal's colour and cursor codes (`ESC [ … letter`,
/// `ESC ] … BEL`), and without carriage returns.
pub fn plain(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.peek() {
                Some('[') => {
                    chars.next();
                    for d in chars.by_ref() {
                        if ('@'..='~').contains(&d) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    chars.next();
                    for d in chars.by_ref() {
                        if d == '\u{7}' {
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\r' => {}
            _ => out.push(c),
        }
    }
    out
}

/// **What `text` (a command's result) shows**: `None` when it printed
/// nothing.
pub fn of(text: &str, failed: bool) -> Option<Output> {
    let text = plain(text);
    let lines: Vec<String> = text.trim_end().lines().map(|l| super::steps::clip(l.trim_end(), LINE_MAX)).collect();
    if lines.iter().all(|l| l.is_empty()) {
        return None;
    }
    if lines.len() <= HEAD + TAIL {
        return Some(Output { text: lines.join("\n"), failed, ..Default::default() });
    }
    Some(Output { text: lines[..HEAD].join("\n"), tail: lines[lines.len() - TAIL..].join("\n"), more: lines.len() - HEAD - TAIL, failed })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_output_is_kept_whole_and_a_long_one_by_its_start_and_end() {
        let o = of("ok\n1131:### The timeline\n\n", false).unwrap();
        assert_eq!((o.text.as_str(), o.tail.as_str(), o.more, o.failed), ("ok\n1131:### The timeline", "", 0, false));
        assert_eq!(serde_json::to_value(&o).unwrap(), serde_json::json!({ "text": "ok\n1131:### The timeline" }));
        // Nothing printed: none.
        assert_eq!(of("", false), None);
        assert_eq!(of("  \n\r\n", true), None);
        // A long one: its first lines, its last, the ones between counted.
        let long: String = (1..=200).map(|i| format!("test {i} ... ok\n")).collect();
        let o = of(&long, true).unwrap();
        assert_eq!(o.text.lines().count(), HEAD);
        assert_eq!((o.text.lines().next(), o.tail.lines().last(), o.more, o.failed), (Some("test 1 ... ok"), Some("test 200 ... ok"), 200 - HEAD - TAIL, true));
        assert_eq!(o.tail.lines().count(), TAIL);
        assert_eq!(serde_json::to_value(&o).unwrap()["failed"], true);
        // Just at the most: whole.
        let most: String = (1..=HEAD + TAIL).map(|i| format!("{i}\n")).collect();
        assert_eq!(of(&most, false).map(|o| (o.text.lines().count(), o.more)), Some((HEAD + TAIL, 0)));
        // A very long line is cut.
        let wide = of(&"x".repeat(LINE_MAX * 3), false).unwrap();
        assert_eq!(wide.chars(), LINE_MAX);
        assert!(wide.text.ends_with('…'));
    }

    #[test]
    fn colour_codes_and_carriage_returns_go() {
        assert_eq!(plain("\u{1b}[32m✓\u{1b}[0m 3 passed\r\n\u{1b}[1;31mFAIL\u{1b}[m one"), "✓ 3 passed\nFAIL one");
        assert_eq!(plain("\u{1b}]0;title\u{7}done"), "done");
        assert_eq!(plain("plain text"), "plain text");
        assert_eq!(of("\u{1b}[2K\u{1b}[1G", false), None);
    }
}

//! **The text before the cursor, for AI cleanup** (2026-10-25): a sentence
//! or two of what's already written where the dictation goes, so cleanup can
//! continue a sentence (no capital, the right end: "Do you know whether" →
//! "the venue has parking?") and not repeat the words it ends with.
//!
//! - **The read is `selection.rs`'s**, the one "Match the text around the
//!   cursor" (`smart_spacing`) already makes at record start, in the
//!   background (`selection::peek_before`: never waited for, never read
//!   again). With that switch off, or a field Yap can't read, nothing is
//!   sent.
//! - **Bounded** ([`bounded`]): the last [`MAX_SENTENCES`] sentences, at
//!   most [`MAX_CHARS`] characters, cut at a word; in memory for the one
//!   dictation, never logged (lengths at `debug` at most).
//! - **Where it may go** ([`allowed`]): to a model on this PC (the
//!   on-device model, a server on `localhost`) whenever it's read; to any
//!   other — a cloud provider, a server on the network — only with "Send
//!   the text before the cursor to cleanup" on (`cursor_context_cloud`,
//!   Settings → Privacy & data, off by default). The network receipts name
//!   such a cleanup "AI cleanup + text before the cursor"
//!   (`net::Feature::DictationContext`), and Privacy & data's "What leaves
//!   this PC" has its row.
//! - **The line** ([`line`]): Yap's own layer, last before the transcript,
//!   never in the system prompt (the server's cache stays the same).
//!
//! Test mode: debug-only `e2e_cursor_before` stands in for the read.

use crate::config::YapConfig;

/// At most this many sentences before the cursor.
pub const MAX_SENTENCES: usize = 2;
/// At most this many characters.
pub const MAX_CHARS: usize = 240;

/// The last sentence or two of `before` (the text before the cursor), at
/// most [`MAX_CHARS`] characters cut at a word, its trailing space kept
/// as one ("Thanks. " is a new sentence to come); `None` when there's
/// nothing but space. Pure.
pub fn bounded(before: &str) -> Option<String> {
    if before.trim().is_empty() {
        return None;
    }
    let ends_with_space = before.ends_with([' ', '\t']);
    let ends_with_line = before.trim_end_matches([' ', '\t']).ends_with('\n');
    let body = before.trim_end();
    let tail = crate::pasting::smart_insert::tail_chars(body, MAX_CHARS);
    // Sentence starts: after ". ", "! ", "? " or a line break. Keep from the
    // start of the last MAX_SENTENCES sentences.
    let chars: Vec<(usize, char)> = tail.char_indices().collect();
    let mut starts: Vec<usize> = Vec::new();
    for (k, &(_, c)) in chars.iter().enumerate() {
        let next = chars.get(k + 1).map(|&(_, n)| n);
        let ends =
            (matches!(c, '.' | '!' | '?') && next.is_some_and(char::is_whitespace)) || c == '\n';
        if ends {
            if let Some(&(j, _)) = chars.get(k + 1) {
                starts.push(j);
            }
        }
    }
    // The sentence the cursor is in counts as one, finished or not.
    let from = if starts.len() >= MAX_SENTENCES {
        starts[starts.len() - MAX_SENTENCES]
    } else {
        0
    };
    let kept = tail[from..].trim_start();
    if kept.is_empty() {
        return None;
    }
    let mut out = kept.to_string();
    if ends_with_line {
        out.push('\n');
    } else if ends_with_space {
        out.push(' ');
    }
    Some(out)
}

/// May the text before the cursor go with a cleanup to `base_url`? A model
/// on this PC: yes. Anything else (a cloud provider, a server on the
/// network): only with `cursor_context_cloud` on. Pure.
pub fn allowed(cfg: &YapConfig, base_url: &str) -> bool {
    on_this_pc(base_url) || cfg.cursor_context_cloud
}

/// A server on this PC (loopback, `localhost`), by its URL, as the network
/// receipts place it (`net::place_of_url`). Pure but for a test run's
/// stand-ins for cloud services.
pub fn on_this_pc(base_url: &str) -> bool {
    crate::privacy::net::place_of_url(base_url) == Some(crate::privacy::net::Place::Pc)
}

/// The line just before the transcript for `before` (already
/// [`bounded`]). Pure but for a model evaluation's wording
/// (`YAP_EVAL_CURSOR_LINE`, "{text}" the text).
pub fn line(before: &str) -> String {
    let quoted = before.replace('"', "\u{201D}");
    let template = eval_wording().unwrap_or_else(|| LINE.to_string());
    template.replace("{text}", &quoted)
}

/// The line's wording ("{text}": the text before the cursor). Measured
/// (docs/model-evals.md "Cleanup strength and formatting"): naming a
/// capital, a question mark or "the whole sentence" made Qwen3.5-4B write
/// the text before the cursor out again, or a "?" after a statement.
pub const LINE: &str =
    "Where it goes: right after \"{text}\", which is already written and is not part of your \
output. Clean up only the transcript, so it follows on from that text.";

fn eval_wording() -> Option<String> {
    if !crate::ai::eval::active() {
        return None;
    }
    std::env::var("YAP_EVAL_CURSOR_LINE")
        .ok()
        .filter(|s| !s.trim().is_empty())
}

/// A cleanup that had the text before the cursor, without what it wrote out
/// of that text again (pure): the list marker the cursor's line already
/// starts with ("- " before it, "- Bread" back → "Bread"), and the text's
/// last sentence of three words or more written out again before the
/// transcript (the 4B did with some wordings, model-evals.md).
pub fn trim_echo(before: &str, cleaned: &str) -> String {
    let out = cleaned.trim_start();
    // The cursor's line (after the last line break) is only a bullet: the
    // cleanup's own bullet goes.
    let line = before.rsplit('\n').next().unwrap_or("").trim();
    if let Some(marker) = ["-", "*", "•"].into_iter().find(|m| line == *m) {
        if let Some(rest) = out.strip_prefix(marker).filter(|r| r.starts_with(' ')) {
            return rest.trim_start().to_string();
        }
    }
    // The text's last sentence written out again (its words, any case or
    // punctuation) before more of the transcript: it goes.
    let last = before
        .trim_end()
        .rsplit(['.', '!', '?', '\n'])
        .find(|s| !s.trim().is_empty())
        .unwrap_or("");
    let norm = |s: &str| {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
            .to_lowercase()
    };
    let want: Vec<String> = last
        .split_whitespace()
        .map(norm)
        .filter(|w| !w.is_empty())
        .collect();
    let words: Vec<&str> = out.split_whitespace().collect();
    if want.len() >= 3
        && words.len() > want.len()
        && words[..want.len()]
            .iter()
            .map(|w| norm(w))
            .eq(want.iter().cloned())
    {
        // The end of the last echoed word, as an offset into `out` (the
        // words are slices of it).
        let w = words[want.len() - 1];
        let end = w.as_ptr() as usize - out.as_ptr() as usize + w.len();
        return out[end..].trim_start_matches([',', ' ']).to_string();
    }
    cleaned.to_string()
}

/// The text before the cursor for this dictation's cleanup at `base_url`,
/// bounded, if it was read and may go there. Never logged.
pub fn for_cleanup(cfg: &YapConfig, base_url: &str) -> Option<String> {
    if !allowed(cfg, base_url) {
        return None;
    }
    let before = read_before()?;
    let kept = bounded(&before)?;
    tracing::debug!(
        chars = kept.chars().count(),
        "AI cleanup: with the text before the cursor"
    );
    Some(kept)
}

/// The text before the cursor read for the dictation now
/// (`selection::peek_before`), or test mode's stand-in.
fn read_before() -> Option<String> {
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() && !crate::shell::realtest::on() {
        return STAND_IN.lock().ok().and_then(|g| g.clone());
    }
    crate::pasting::selection::peek_before()
}

#[cfg(debug_assertions)]
static STAND_IN: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Debug-only, test mode: the text before the cursor every dictation from
/// now on "reads" (`None`: none). Nothing is read from a real window.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_cursor_before(text: Option<String>) -> Result<(), String> {
    if !crate::shell::e2e::active() || crate::shell::realtest::on() {
        return Err("e2e_cursor_before is for test runs (not robot mode)".into());
    }
    if let Ok(mut g) = STAND_IN.lock() {
        *g = text;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_sentence_or_two_at_most() {
        assert_eq!(bounded("   \n "), None);
        assert_eq!(
            bounded("Do you know whether").as_deref(),
            Some("Do you know whether")
        );
        assert_eq!(
            bounded("Hi Sam. The launch went well. Sales are up and").as_deref(),
            Some("The launch went well. Sales are up and")
        );
        // A finished sentence and a space: a new one comes next.
        assert_eq!(
            bounded("We met on Monday. Thanks for the update. ").as_deref(),
            Some("We met on Monday. Thanks for the update. ")
        );
        // Lines: a list's last items.
        assert_eq!(
            bounded("Shopping list:\n- Milk\n- Eggs\n- ").as_deref(),
            Some("- Eggs\n- ")
        );
        // Long: cut at a word, never more than MAX_CHARS.
        let long = "word ".repeat(200);
        let b = bounded(&long).unwrap();
        assert!(b.chars().count() <= MAX_CHARS + 1, "{}", b.len());
        assert!(b.starts_with("word"));
    }

    #[test]
    fn only_to_this_pc_unless_the_cloud_switch_is_on() {
        let mut cfg = YapConfig::default();
        assert!(!cfg.cursor_context_cloud, "off by default");
        assert!(allowed(&cfg, "http://127.0.0.1:52011/v1"));
        assert!(allowed(&cfg, "http://localhost:11434/v1"));
        assert!(!allowed(&cfg, "https://api.groq.com/openai/v1"));
        assert!(
            !allowed(&cfg, "http://192.168.1.20:11434/v1"),
            "a server on the network: the text leaves this PC"
        );
        cfg.cursor_context_cloud = true;
        assert!(allowed(&cfg, "https://api.groq.com/openai/v1"));
    }

    #[test]
    fn what_the_cleanup_wrote_out_again_goes() {
        // A bullet the cursor's line already has.
        assert_eq!(trim_echo("Shopping list:\n- Milk\n- ", "- Bread"), "Bread");
        assert_eq!(trim_echo("Shopping list:\n- Milk\n- ", "Bread"), "Bread");
        assert_eq!(
            trim_echo("Notes: ", "- Bread"),
            "- Bread",
            "no bullet before: the cleanup's stays"
        );
        // The last sentence written out again.
        assert_eq!(
            trim_echo(
                "Do you know whether",
                "do you know whether the venue has step-free access?"
            ),
            "the venue has step-free access?"
        );
        assert_eq!(
            trim_echo(
                "We spoke to the landlord yesterday and",
                "We spoke to the landlord yesterday, and he said it's fixed."
            ),
            "he said it's fixed."
        );
        // Not a whole echo, or nothing after it: as written.
        assert_eq!(
            trim_echo(
                "Could you send me the",
                "send me the final report by Friday."
            ),
            "send me the final report by Friday."
        );
        assert_eq!(
            trim_echo("Do you know whether", "Do you know whether"),
            "Do you know whether"
        );
        assert_eq!(
            trim_echo("Hi", "Hi there"),
            "Hi there",
            "under three words: kept"
        );
    }

    #[test]
    fn the_line_quotes_the_text() {
        let l = line("He said \"soon\" and");
        assert!(l.contains("\u{201D}soon\u{201D} and\""), "{l}");
        assert!(l.starts_with("Where it goes: right after \"He said"), "{l}");
    }
}

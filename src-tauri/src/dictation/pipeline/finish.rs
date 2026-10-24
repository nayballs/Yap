//! A transcript's last steps, shared with meetings, Upload and Run
//! again: the correction dictionary (exact, then near-misses), the names
//! on screen, the "press enter" voice command and snippets.

use crate::config::{self, YapConfig};
use crate::dictation::stt;

/// Run the correction dictionary over a transcript, the same way for a
/// dictation, a meeting segment and an Upload: exact replacements
/// (`config::apply_dictionary`), then the fuzzy near-miss pass (`fuzzy.rs`)
/// when "Catch near-misses" is on and the model isn't Whisper. (Whisper got
/// the vocabulary as its `initial_prompt` instead: Handy's split.) Entries
/// opted out of the fuzzy pass (`DictionaryEntry::fuzzy`) are exact-only.
/// Snippets aren't corrections and are left alone (a dictation expands them
/// afterwards, [`finish_dictation`]). The dictionary words in the result
/// count as used, for the Whisper prompt's ranking (`dict_usage`).
pub(crate) fn apply_corrections(text: &str, cfg: &YapConfig) -> String {
    let corrected = corrections(text, cfg);
    crate::dictionary::dict_usage::record(&crate::memory::with_names(&cfg.dictionary), &corrected);
    corrected
}

/// [`apply_corrections`] without counting the words as used (the words as
/// said, beside the cleaned text that already counted them).
fn corrections(text: &str, cfg: &YapConfig) -> String {
    let corrected = config::apply_dictionary(text, &cfg.dictionary);
    if cfg.dictionary_fuzzy && !stt::is_whisper_model(&cfg.model_size) {
        let fixed = crate::dictionary::fuzzy::apply_fuzzy(&corrected, &cfg.dictionary);
        // The names Yap remembers (memory/vocab.rs), more carefully: only
        // where what was heard isn't an ordinary word itself.
        let names: Vec<_> = crate::memory::dictation_names(&cfg.dictionary).into_iter().filter(|e| e.fuzzy).collect();
        if names.is_empty() {
            fixed
        } else {
            let words = crate::memory::Words::new();
            crate::dictionary::fuzzy::apply_names(&fixed, &names, &|w| words.is(w))
        }
    } else {
        corrected
    }
}

/// A dictation's words **as said** ("Use what I said", `as_said.rs`): the
/// transcript (`raw`) through the corrections, the names on screen
/// (`screen`, when the dictation read them), the "press enter" command and
/// snippets — everything [`finish_dictation_with`] does but the AI cleanup
/// before it.
pub(crate) fn as_said_text(raw: &str, cfg: &YapConfig, screen: Option<&crate::dictionary::screen_terms::ScreenTerms>) -> String {
    let corrected = crate::dictionary::screen_terms::correct(&corrections(raw.trim(), cfg), screen, cfg);
    let text = cfg.voice_enter.then(|| take_voice_enter(&corrected, raw)).flatten().unwrap_or(corrected);
    config::apply_snippets(&text, &cfg.dictionary)
}

/// A dictation's text after AI cleanup (`cleaned`; `raw` = the transcript
/// before it): the corrections ([`apply_corrections`]), then the "press
/// enter" voice command when `voice_enter` is on ([`take_voice_enter`]),
/// then snippets (`config::apply_snippets` — after the command is off, so
/// "my address, press enter" still gives exactly the address). Returns the
/// text to paste and whether the voice command asked for the submit key.
/// (A dictation goes through [`finish_dictation_with`]; a dictation run
/// again from its kept audio comes here, its screen long gone.)
pub(crate) fn finish_dictation(cleaned: &str, raw: &str, cfg: &YapConfig) -> (String, bool) {
    finish_dictation_with(cleaned, raw, cfg, None)
}

/// [`finish_dictation`] with the names on screen (`screen_terms`): after
/// the corrections (the dictionary and memory have the final say), a name
/// heard a little wrong is spelled as the screen shows it — before the
/// voice command and snippets, whose text is never touched.
pub(crate) fn finish_dictation_with(
    cleaned: &str,
    raw: &str,
    cfg: &YapConfig,
    screen: Option<&crate::dictionary::screen_terms::ScreenTerms>,
) -> (String, bool) {
    let corrected = crate::dictionary::screen_terms::correct(&apply_corrections(cleaned, cfg), screen, cfg);
    let (text, enter) = match cfg.voice_enter.then(|| take_voice_enter(&corrected, raw)).flatten() {
        Some(text) => (text, true),
        None => (corrected, false),
    };
    (config::apply_snippets(&text, &cfg.dictionary), enter)
}

/// The "press enter" voice command: a dictation that ENDS with "press enter"
/// or "press return" (any case, any punctuation around it) is sent. Returns
/// the text to paste when it was said — `text` with the command taken off,
/// along with a comma or dash before it; a full stop or question mark after
/// it moves onto the text ("Can you check, press enter?" → "Can you
/// check?"). The command only counts as the last two words, never
/// mid-sentence. When `text` (the final text) doesn't end with it but `raw`
/// (the transcript before AI cleanup) does, cleanup dropped or reworded it:
/// `text` is pasted as it is and the key still pressed. `None`: not said.
pub(crate) fn take_voice_enter(text: &str, raw: &str) -> Option<String> {
    if let Some((start, end)) = trailing_enter_command(text) {
        let kept = text[..start].trim_end_matches(|c: char| {
            c.is_whitespace() || matches!(c, ',' | ';' | ':' | '-' | '–' | '—')
        });
        let mut out = kept.to_string();
        let ends_sentence = kept.ends_with(['.', '!', '?', '…']);
        if !kept.is_empty() && !ends_sentence {
            if let Some(stop) = text[end..].chars().find(|c| matches!(c, '.' | '!' | '?')) {
                out.push(stop);
            }
        }
        return Some(out);
    }
    trailing_enter_command(raw).map(|_| text.to_string())
}

/// Where a trailing "press enter" / "press return" is in `text`: the byte
/// range from "press" to the end of the last word (only punctuation and
/// spaces may follow it).
fn trailing_enter_command(text: &str) -> Option<(usize, usize)> {
    // The last two words: runs of letters and digits.
    let mut words: Vec<(usize, usize)> = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices() {
        match (c.is_alphanumeric(), start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                words.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        words.push((s, text.len()));
    }
    let [.., (p0, p1), (k0, k1)] = words[..] else {
        return None;
    };
    let key = &text[k0..k1];
    (text[p0..p1].eq_ignore_ascii_case("press")
        && (key.eq_ignore_ascii_case("enter") || key.eq_ignore_ascii_case("return")))
    .then_some((p0, k1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DictionaryEntry;

    fn entry(from: &str, to: &str, fuzzy: bool) -> DictionaryEntry {
        DictionaryEntry {
            from: from.to_string(),
            to: to.to_string(),
            fuzzy,
            snippet: false,
        }
    }

    #[test]
    fn press_enter_at_the_end_is_taken_off() {
        let t = |text: &str| take_voice_enter(text, text);
        assert_eq!(t("Sounds good press enter").as_deref(), Some("Sounds good"));
        assert_eq!(t("Sounds good, press enter.").as_deref(), Some("Sounds good."));
        assert_eq!(t("Sounds good. Press Enter.").as_deref(), Some("Sounds good."));
        assert_eq!(t("Can you check this — PRESS RETURN?").as_deref(), Some("Can you check this?"));
        assert_eq!(t("Done!\npress-enter").as_deref(), Some("Done!"));
        assert_eq!(t("He said \"hi\", press enter").as_deref(), Some("He said \"hi\""));
        // Nothing but the command: nothing to paste, still a press.
        assert_eq!(t("Press enter.").as_deref(), Some(""));
        assert_eq!(t("press return").as_deref(), Some(""));
    }

    #[test]
    fn press_enter_only_counts_as_the_last_words() {
        let t = |text: &str| take_voice_enter(text, text);
        assert_eq!(t("press enter and wait for the prompt"), None);
        assert_eq!(t("I pressed enter"), None);
        assert_eq!(t("press the enter key"), None);
        assert_eq!(t("express enter"), None);
        assert_eq!(t("press enters"), None);
        assert_eq!(t("enter"), None);
        assert_eq!(t(""), None);
    }

    #[test]
    fn press_enter_dropped_by_cleanup_is_found_in_the_raw_transcript() {
        // Cleanup took the command out: the text stays as cleanup left it.
        assert_eq!(
            take_voice_enter("Sounds good.", "sounds good press enter").as_deref(),
            Some("Sounds good.")
        );
        // Said nowhere: nothing.
        assert_eq!(take_voice_enter("Sounds good.", "sounds good"), None);
    }

    #[test]
    fn a_dictation_gets_corrections_then_the_voice_command_then_snippets() {
        let mut dict = vec![entry("jaison", "JSON", true)];
        dict.push(DictionaryEntry { snippet: true, ..entry("my address", "1 Long Lane\nLeeds", true) });
        let mut c = cfg("parakeet-tdt-0.6b-v3", true, dict);
        // A snippet of several lines ending the dictation ends where its own
        // text does (no "Norwich.").
        assert_eq!(
            finish_dictation("The jaison is at my address, press enter.", "x", &c),
            ("The JSON is at 1 Long Lane\nLeeds".to_string(), true)
        );
        // The command off first, so the snippet is all that's left: exactly its text.
        assert_eq!(
            finish_dictation("My address, press enter.", "x", &c),
            ("1 Long Lane\nLeeds".to_string(), true)
        );
        assert_eq!(finish_dictation("Hi.", "hi", &c), ("Hi.".to_string(), false));
        // Switched off: the words stay and nothing is pressed.
        c.voice_enter = false;
        assert_eq!(
            finish_dictation("Hi, press enter.", "hi press enter", &c),
            ("Hi, press enter.".to_string(), false)
        );
    }

    #[test]
    fn an_email_keeps_its_greeting_on_its_own_line() {
        // The real-app robot (2026-10-24): cleanup's email went into a Gmail
        // page as "Hi Sam, Thank you for the figures." — the near-miss pass
        // put the words back together with spaces.
        let dict = vec![entry("jaison", "JSON", true)];
        let c = cfg("parakeet-tdt-0.6b-v3", true, dict);
        let email = "Hi Sam,\n\nThank you for the figures.";
        assert_eq!(finish_dictation(email, "hi sam thanks for the figures", &c), (email.to_string(), false));
        assert_eq!(
            finish_dictation("Hi Sam,\n\nThe jaison is attached.", "x", &c),
            ("Hi Sam,\n\nThe JSON is attached.".to_string(), false)
        );
    }

    #[test]
    fn corrections_leave_snippets_to_the_dictation() {
        // Meetings and Upload run apply_corrections alone: a snippet's phrase
        // stays as said there.
        let snip = DictionaryEntry { snippet: true, ..entry("my address", "1 Long Lane", true) };
        let c = cfg("parakeet-tdt-0.6b-v3", true, vec![snip]);
        assert_eq!(apply_corrections("at my address", &c), "at my address");
    }

    fn cfg(model: &str, fuzzy: bool, dictionary: Vec<DictionaryEntry>) -> YapConfig {
        YapConfig {
            model_size: model.to_string(),
            dictionary_fuzzy: fuzzy,
            dictionary,
            ..YapConfig::default()
        }
    }

    #[test]
    fn corrections_are_exact_then_fuzzy_for_onnx_models() {
        let dict = vec![entry("jaison", "JSON", true), entry("chat gpt", "ChatGPT", true)];
        let parakeet = cfg("parakeet-tdt-0.6b-v3", true, dict.clone());
        // Exact (case-insensitive, whole words) and a near-miss ("jaisen").
        assert_eq!(
            apply_corrections("ask Chat GPT about the jaisen file", &parakeet),
            "ask ChatGPT about the JSON file"
        );
        assert_eq!(apply_corrections("the jaison file", &parakeet), "the JSON file");
        // Whisper had the words as its prompt: exact replacements only.
        let whisper = cfg("turbo", true, dict.clone());
        assert_eq!(
            apply_corrections("the jaisen and jaison files", &whisper),
            "the jaisen and JSON files"
        );
        // "Catch near-misses" off: exact only.
        let off = cfg("parakeet-tdt-0.6b-v3", false, dict);
        assert_eq!(apply_corrections("the jaisen file", &off), "the jaisen file");
    }

    #[test]
    fn corrections_respect_the_per_entry_opt_out() {
        let c = cfg("parakeet-tdt-0.6b-v3", true, vec![entry("json", "JSON", false)]);
        assert_eq!(apply_corrections("Dear Jason, the json is attached", &c), "Dear Jason, the JSON is attached");
    }

    #[test]
    fn no_dictionary_leaves_the_text_alone() {
        let c = cfg("parakeet-tdt-0.6b-v3", true, Vec::new());
        assert_eq!(apply_corrections("  two  spaces\tand a tab ", &c), "  two  spaces\tand a tab ");
    }
}

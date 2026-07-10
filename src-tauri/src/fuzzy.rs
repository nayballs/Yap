//! Fuzzy dictionary correction — catch NEAR-misses the exact find/replace
//! (`config::apply_dictionary`) can't.
//!
//! Ported from Handy's `audio_toolkit/text.rs` `apply_custom_words`
//! (`E:\Projects\references\Handy`): 1–3 word n-grams are matched against the
//! dictionary's spellings using normalized Levenshtein distance with a Soundex
//! phonetic boost (score × 0.3 on a phonetic hit), a ≤25% length-difference
//! gate, and Handy's default acceptance threshold (0.18). So "jaison" snaps to
//! "JSON" and "Chat G P T" becomes "ChatGPT" without an exact rule for every
//! mis-hearing. Punctuation and the original case pattern are preserved.
//!
//! Yap adaptation: Handy's dictionary is a flat list of correct words; Yap's
//! entries are `{from → to}` pairs. An n-gram is fuzzy-matched against BOTH
//! spellings (the canonical `to` and the mis-hearing trigger `from`) and always
//! replaced with `to`. Runs for ONNX engines only — Whisper models get the
//! vocabulary as `initial_prompt` instead (Handy's exact split; see stt.rs).
//!
//! Also here: `is_prompt_echo`, a port of OpenWhispr's
//! `utils/dictionaryEchoFilter.js` — with a dictionary `initial_prompt`,
//! Whisper on (near-)silence tends to hallucinate the prompt back; that guard
//! detects a transcript that is essentially just the dictionary words.

use crate::config::DictionaryEntry;

/// Handy's default `word_correction_threshold` (settings.rs). A combined score
/// below this accepts the match; 0.18 ≈ "less than ~1 edit per 6 chars, or a
/// moderate edit distance when the words sound alike".
const THRESHOLD: f64 = 0.18;

/// Dictionary spellings shorter than this (letters/digits only) are never
/// fuzzy-matched — near-misses of 1–2 char terms are usually real words. The
/// exact path still replaces them. (FluidVoice uses the same floor.)
const MIN_TERM_LEN: usize = 3;

/// Classic Levenshtein edit distance (Handy uses the `strsim` crate; inlined
/// here to avoid a dependency for one 20-line DP).
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.is_empty() {
        return b.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Soundex code as computed by the `natural` crate Handy uses — a
/// NONSTANDARD variant, replicated exactly (`rs-natural src/phonetics.rs`):
/// the first char is kept raw (never encoded, so it can't merge with its own
/// digit), h/w are stripped, adjacent duplicate digits collapse, THEN vowels
/// are removed, pad/truncate to 4. Under these rules "jason" and "json" both
/// code to `j250`-style ['j','2','5','0'] — which is what makes the phonetic
/// boost catch vowel-swallowed mis-hearings.
fn soundex_code(s: &str) -> Option<[char; 4]> {
    let digit = |c: char| -> char {
        match c {
            'b' | 'f' | 'p' | 'v' => '1',
            'c' | 'g' | 'j' | 'k' | 'q' | 's' | 'x' | 'z' => '2',
            'd' | 't' => '3',
            'l' => '4',
            'm' | 'n' => '5',
            'r' => '6',
            'h' | 'w' => '9', // stripped before dedup (h/w don't separate)
            _ => '0',         // vowels + everything else (stripped after dedup)
        }
    };
    let mut chars = s.chars();
    let first = chars.next()?;
    let mut enc: Vec<char> = std::iter::once(first).chain(chars.map(digit)).collect();
    enc.retain(|&c| c != '9');
    enc.dedup();
    enc.retain(|&c| c != '0');
    let mut code = ['0'; 4];
    for (i, c) in enc.into_iter().take(4).enumerate() {
        code[i] = c;
    }
    Some(code)
}

fn soundex_eq(a: &str, b: &str) -> bool {
    matches!((soundex_code(a), soundex_code(b)), (Some(x), Some(y)) if x == y)
}

/// A fuzzy-matchable spelling: `key` is the lowercased, space-stripped form
/// compared against n-grams; `replacement` is the entry's canonical `to`.
struct Candidate<'a> {
    key: String,
    replacement: &'a str,
}

fn build_candidates(dict: &[DictionaryEntry]) -> Vec<Candidate<'_>> {
    let mut out = Vec::new();
    for entry in dict {
        if !entry.fuzzy {
            continue; // per-entry opt-out: exact-match only (config.rs)
        }
        let to = entry.to.trim();
        if to.is_empty() {
            continue;
        }
        for spelling in [entry.to.trim(), entry.from.trim()] {
            let key: String = spelling.to_lowercase().replace(' ', "");
            if key.chars().filter(|c| c.is_alphanumeric()).count() >= MIN_TERM_LEN
                && !out.iter().any(|c: &Candidate| c.key == key)
            {
                out.push(Candidate { key, replacement: to });
            }
        }
    }
    out
}

/// Builds an n-gram string by cleaning and concatenating words: punctuation
/// trimmed from each word's edges, lowercased, joined without spaces — so
/// "Charge B" can match "ChargeBee".
fn build_ngram(words: &[&str]) -> String {
    words
        .iter()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .collect::<Vec<_>>()
        .concat()
}

/// Finds the best dictionary spelling for `candidate` (Handy's scoring,
/// verbatim): normalized Levenshtein, ×0.3 on a Soundex hit, ≤25% length
/// difference (min 2 chars), accept below `THRESHOLD`. Returns the
/// replacement and the winner's ABSOLUTE edit distance (used by the n-gram
/// selection in `apply_fuzzy`).
fn find_best_match<'a>(candidate: &str, candidates: &'a [Candidate]) -> Option<(&'a str, usize)> {
    if candidate.len() < MIN_TERM_LEN || candidate.len() > 50 {
        return None;
    }

    let mut best: Option<(&str, usize)> = None;
    let mut best_score = f64::MAX;

    for c in candidates {
        let len_diff = (candidate.len() as i32 - c.key.len() as i32).abs() as f64;
        let max_len = candidate.len().max(c.key.len()) as f64;
        let max_allowed_diff = (max_len * 0.25).max(2.0);
        if len_diff > max_allowed_diff {
            continue;
        }

        let lev = levenshtein(candidate, &c.key);
        let lev_score = if max_len > 0.0 { lev as f64 / max_len } else { 1.0 };
        let combined = if soundex_eq(candidate, &c.key) { lev_score * 0.3 } else { lev_score };

        if combined < THRESHOLD && combined < best_score {
            best = Some((c.replacement, lev));
            best_score = combined;
        }
    }
    best
}

/// Preserves the case pattern of the original word when applying a replacement
/// (all-caps stays all-caps, leading capital stays capitalized; a canonical
/// mixed-case replacement like "JSON" is never down-cased).
fn preserve_case_pattern(original: &str, replacement: &str) -> String {
    if original.chars().all(|c| c.is_uppercase()) {
        replacement.to_uppercase()
    } else if original.chars().next().is_some_and(|c| c.is_uppercase()) {
        let mut chars: Vec<char> = replacement.chars().collect();
        if let Some(first) = chars.get_mut(0) {
            *first = first.to_uppercase().next().unwrap_or(*first);
        }
        chars.into_iter().collect()
    } else {
        replacement.to_string()
    }
}

/// Extracts non-alphanumeric prefix and suffix from a word (kept around the
/// replacement so "ChargeBee," survives).
fn extract_punctuation(word: &str) -> (&str, &str) {
    let prefix_end = word.chars().take_while(|c| !c.is_alphanumeric()).count();
    let suffix_len = word.chars().rev().take_while(|c| !c.is_alphanumeric()).count();
    if prefix_end == word.chars().count() {
        // all punctuation — treat as prefix only
        return (word, "");
    }
    let prefix = &word[..word.char_indices().nth(prefix_end).map_or(0, |(i, _)| i)];
    let suffix_start = word
        .char_indices()
        .rev()
        .nth(suffix_len.saturating_sub(1))
        .map_or(word.len(), |(i, _)| i);
    let suffix = if suffix_len > 0 { &word[suffix_start..] } else { "" };
    (prefix, suffix)
}

/// Applies fuzzy dictionary correction to `text` (Handy's `apply_custom_words`
/// adapted to Yap's `{from → to}` entries). Greedy longest-first n-grams
/// (3 → 1); each match consumes its words and emits the entry's `to` with the
/// original case pattern + edge punctuation.
pub fn apply_fuzzy(text: &str, dict: &[DictionaryEntry]) -> String {
    let candidates = build_candidates(dict);
    if candidates.is_empty() {
        return text.to_string();
    }

    let words: Vec<&str> = text.split_whitespace().collect();
    let mut result: Vec<String> = Vec::with_capacity(words.len());
    let mut i = 0;

    while i < words.len() {
        // Try n-grams SHORT → LONG; a longer n-gram wins only when it
        // STRICTLY reduces the absolute edit distance. (Deviation from
        // Handy's longest-first greedy, which swallows an unrelated
        // adjacent word: "Charge B, che" matched "ChargeBee" as a 3-gram
        // at essentially the same score — verified against the real
        // strsim/natural crates — and "che" vanished from the output.)
        let mut chosen: Option<(usize, &str, usize)> = None; // (n, replacement, dist)
        for n in 1..=3usize {
            if i + n > words.len() {
                break;
            }
            let ngram = build_ngram(&words[i..i + n]);
            if let Some((replacement, dist)) = find_best_match(&ngram, &candidates) {
                if chosen.map_or(true, |(_, _, best)| dist < best) {
                    chosen = Some((n, replacement, dist));
                }
            }
        }

        if let Some((n, replacement, _)) = chosen {
            let ngram_words = &words[i..i + n];
            let (prefix, _) = extract_punctuation(ngram_words[0]);
            let (_, suffix) = extract_punctuation(ngram_words[n - 1]);
            let corrected = preserve_case_pattern(ngram_words[0], replacement);
            result.push(format!("{}{}{}", prefix, corrected, suffix));
            i += n;
        } else {
            result.push(words[i].to_string());
            i += 1;
        }
    }

    result.join(" ")
}

// ── Dictionary-prompt echo guard ────────────────────────────────────

// Only called from the `engines`-gated real engine (stub builds never pass a
// prompt), hence the allow.
#[cfg_attr(not(feature = "engines"), allow(dead_code))]
fn normalize_for_echo(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// True when `text` is essentially just the dictionary prompt echoed back —
/// Whisper's classic hallucination when given an `initial_prompt` over
/// (near-)silence. Port of OpenWhispr's `matchesDictionaryPrompt`
/// (`utils/dictionaryEchoFilter.js`): exact normalized match, or ≥90% of the
/// transcript's unique words come from the prompt AND ≥70% of the prompt's
/// words appear in the transcript.
#[cfg_attr(not(feature = "engines"), allow(dead_code))]
pub fn is_prompt_echo(text: &str, prompt: &str) -> bool {
    if text.is_empty() || prompt.is_empty() {
        return false;
    }
    let norm_text = normalize_for_echo(text);
    let norm_prompt = normalize_for_echo(prompt);
    if norm_text.is_empty() || norm_prompt.is_empty() {
        return false;
    }
    if norm_text == norm_prompt {
        return true;
    }

    let dict_words: std::collections::HashSet<&str> = norm_prompt.split(' ').collect();
    let text_words: std::collections::HashSet<&str> = norm_text.split(' ').collect();

    let match_count = text_words.iter().filter(|w| dict_words.contains(*w)).count();
    let text_composition = match_count as f64 / text_words.len() as f64;
    let dictionary_usage = match_count as f64 / dict_words.len() as f64;

    text_composition >= 0.9 && dictionary_usage >= 0.7
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(from: &str, to: &str) -> DictionaryEntry {
        DictionaryEntry { from: from.to_string(), to: to.to_string(), fuzzy: true }
    }

    // ── ports of Handy's apply_custom_words tests ──

    #[test]
    fn fuzzy_match_typos() {
        let dict = vec![entry("", "hello"), entry("", "world")];
        assert_eq!(apply_fuzzy("helo wrold", &dict), "hello world");
    }

    #[test]
    fn ngram_two_words() {
        let dict = vec![entry("", "ChargeBee")];
        let result = apply_fuzzy("il cui nome è Charge B, che permette", &dict);
        assert!(result.contains("ChargeBee,"), "{result}");
        assert!(!result.contains("Charge B"));
        // the word AFTER the match must survive (Handy's longest-first
        // greedy swallowed it)
        assert!(result.contains("che permette"), "{result}");
    }

    #[test]
    fn adjacent_word_not_swallowed() {
        let dict = vec![entry("", "ChargeBee")];
        let result = apply_fuzzy("CHARGE B is great", &dict);
        assert_eq!(result, "CHARGEBEE is great");
    }

    #[test]
    fn ngram_three_words() {
        let dict = vec![entry("", "ChatGPT")];
        assert!(apply_fuzzy("use Chat G P T for this", &dict).contains("ChatGPT"));
    }

    #[test]
    fn prefers_longer_ngram() {
        let dict = vec![entry("", "OpenAI"), entry("", "GPT")];
        assert_eq!(apply_fuzzy("Open AI GPT model", &dict), "OpenAI GPT model");
    }

    #[test]
    fn ngram_preserves_case() {
        let dict = vec![entry("", "ChargeBee")];
        assert!(apply_fuzzy("CHARGE B is great", &dict).contains("CHARGEBEE"));
    }

    #[test]
    fn spaces_in_dictionary_term() {
        let dict = vec![entry("", "MacBook Pro")];
        assert!(apply_fuzzy("using Mac Book Pro", &dict).contains("MacBook"));
    }

    #[test]
    fn empty_dictionary_is_noop() {
        assert_eq!(apply_fuzzy("hello world", &[]), "hello world");
    }

    // ── Yap-specific behaviour ──

    #[test]
    fn near_miss_of_from_trigger_maps_to_to() {
        // exact rule is jaison→JSON; the fuzzy pass catches "jaisen" too
        let dict = vec![entry("jaison", "JSON")];
        assert_eq!(apply_fuzzy("parse the jaisen file", &dict), "parse the JSON file");
    }

    #[test]
    fn short_terms_are_never_fuzzed() {
        // "AI" (2 chars) is below MIN_TERM_LEN — "ay" must not snap to it
        let dict = vec![entry("", "AI")];
        assert_eq!(apply_fuzzy("ay caramba", &dict), "ay caramba");
    }

    #[test]
    fn unrelated_words_untouched() {
        let dict = vec![entry("", "Kubernetes")];
        assert_eq!(
            apply_fuzzy("completely different sentence", &dict),
            "completely different sentence"
        );
    }

    #[test]
    fn per_entry_opt_out() {
        // fuzzy=false: near-misses must survive ("Jason" ≉ json → JSON);
        // the exact path (config::apply_dictionary) still handles literals.
        let mut e = entry("json", "JSON");
        e.fuzzy = false;
        assert_eq!(apply_fuzzy("Dear Jason", &[e]), "Dear Jason");
    }

    #[test]
    fn phonetic_match_boost() {
        // "jason" vs "json": edit distance 1/5 = 0.2 (> 0.18) but Soundex
        // codes match (J250) → 0.06 → accepted.
        let dict = vec![entry("", "JSON")];
        assert_eq!(apply_fuzzy("send the jason payload", &dict), "send the JSON payload");
    }

    // ── soundex sanity ──

    #[test]
    fn soundex_known_pairs() {
        assert!(soundex_eq("Robert", "Rupert"));
        assert!(soundex_eq("jason", "json"));
        assert!(!soundex_eq("hello", "world"));
    }

    // ── echo guard (port of OpenWhispr's dictionaryEchoFilter tests) ──

    #[test]
    fn echo_exact_match() {
        assert!(is_prompt_echo("Kubernetes, Grafana, ChargeBee", "Kubernetes, Grafana, ChargeBee"));
    }

    #[test]
    fn echo_high_overlap() {
        assert!(is_prompt_echo("Kubernetes Grafana ChargeBee", "Kubernetes, Grafana, ChargeBee, nginx"));
    }

    #[test]
    fn echo_real_sentence_passes() {
        assert!(!is_prompt_echo(
            "deploy the new Grafana dashboard to the cluster today",
            "Kubernetes, Grafana, ChargeBee, nginx"
        ));
    }

    #[test]
    fn echo_empty_inputs() {
        assert!(!is_prompt_echo("", "words"));
        assert!(!is_prompt_echo("words", ""));
    }
}

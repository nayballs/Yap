//! Fuzzy dictionary correction — catch NEAR-misses the exact find/replace
//! (`config::apply_dictionary`) can't.
//!
//! Ported from Handy's `audio_toolkit/text.rs` `apply_custom_words`
//! (`references/Handy`, beside this repo): 1–3 word n-grams are matched against the
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
//!
//! And for learning words from your corrections (`learn.rs`): how alike two
//! spellings are written (`spelling_distance`) and sound (`sound_distance`).

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
        if !entry.fuzzy || entry.snippet {
            // per-entry opt-out: exact-match only (config.rs); a snippet's
            // phrase is matched by `config::apply_snippets`, never fuzzed
            continue;
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

/// Handy's scoring of an n-gram against one spelling, verbatim: normalized
/// Levenshtein, ×0.3 on a Soundex hit, ≤25% length difference (min 2
/// chars). `Some((score, ABSOLUTE edit distance))` when it's accepted
/// (below `THRESHOLD`).
fn score(ngram: &str, key: &str) -> Option<(f64, usize)> {
    if ngram.len() < MIN_TERM_LEN || ngram.len() > 50 {
        return None;
    }
    let len_diff = (ngram.len() as i32 - key.len() as i32).abs() as f64;
    let max_len = ngram.len().max(key.len()) as f64;
    let max_allowed_diff = (max_len * 0.25).max(2.0);
    if len_diff > max_allowed_diff {
        return None;
    }

    let lev = levenshtein(ngram, key);
    let lev_score = if max_len > 0.0 { lev as f64 / max_len } else { 1.0 };
    let combined = if soundex_eq(ngram, key) { lev_score * 0.3 } else { lev_score };
    (combined < THRESHOLD).then_some((combined, lev))
}

/// Finds the best dictionary spelling for `candidate` (the lowest
/// [`score`]). Returns the replacement and the winner's ABSOLUTE edit
/// distance (used by the n-gram selection in `apply_fuzzy`).
fn find_best_match<'a>(candidate: &str, candidates: &'a [Candidate]) -> Option<(&'a str, usize)> {
    let mut best: Option<(&str, usize)> = None;
    let mut best_score = f64::MAX;

    for c in candidates {
        if let Some((combined, lev)) = score(candidate, &c.key) {
            if combined < best_score {
                best = Some((c.replacement, lev));
                best_score = combined;
            }
        }
    }
    best
}

/// Whether the words of `run` after its first — all of them, or fewer —
/// already make `replacement` by themselves, at most `dist` edits away.
/// Then the first word isn't part of the near-miss and the run would
/// swallow it: "to Kubernetes" is 2 edits from "kubernetes", and "Deploy it
/// to Kubernetes." lost its "to"; "1. kubernetes" its number; "- nginx" its
/// space. (Only the same word counts: in "Chad GPT" the "Chad" still makes
/// "ChatGPT" with a dictionary that has "GPT" too.)
fn first_word_adds_nothing(run: &[&str], replacement: &str, dist: usize, candidates: &[Candidate]) -> bool {
    (1..run.len()).any(|from| {
        (from + 1..=run.len()).any(|to| {
            let ngram = build_ngram(&run[from..to]);
            candidates
                .iter()
                .filter(|c| c.replacement == replacement)
                .any(|c| score(&ngram, &c.key).is_some_and(|(_, lev)| lev <= dist))
        })
    })
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
/// adapted to Yap's `{from → to}` entries). Greedy n-grams of 1–3 words (which
/// one wins: see the loop); each match consumes its words and emits the
/// entry's `to` with the original case pattern + edge punctuation.
pub fn apply_fuzzy(text: &str, dict: &[DictionaryEntry]) -> String {
    apply_with(text, dict, &|_, _, _| true)
}

/// `text`'s words as one key, the way a heard run of words is compared:
/// each word's edge punctuation off, lower case, joined without spaces
/// ("Siobhan Ní Bhriain" → "siobhanníbhriain", "GPT-OSS" → "gpt-oss").
pub fn ngram_key(text: &str) -> String {
    build_ngram(&text.split_whitespace().collect::<Vec<_>>())
}

/// **The names and terms on screen** (`screen_terms.rs`), stricter than
/// memory's names ([`apply_names`]), as there are many more of them: a
/// near-miss is taken only when what was heard isn't ordinary words only
/// (`is_word`) — "row shell", "zen tricks", and "hash map" with a
/// "HashMap" on screen, stay as said —, when the speech model wrote it as a
/// name too (a capital: "Rachelle", "Zentrics", "GPTOSS"; Parakeet heard
/// the spoken "row shell" as "rouchell", which is no name), never when it's
/// a spelling `keep` protects (the dictionary's or memory's, as
/// [`ngram_key`]s), and only when it spells **and** sounds like the term:
/// plain edit distance ≤ 0.34 of the longer spelling and
/// [`sound_distance`] ≤ 0.4, on top of Handy's score. One word with the
/// same letters in another case ("zentrix") is always fixed.
pub fn apply_screen(
    text: &str,
    terms: &[DictionaryEntry],
    is_word: &dyn Fn(&str) -> bool,
    keep: &dyn Fn(&str) -> bool,
) -> String {
    apply_with(text, terms, &|run, replacement, dist| {
        let heard = build_ngram(run);
        if keep(&heard) {
            return false;
        }
        let unordinary = run.iter().any(|w| !is_word(&build_ngram(std::slice::from_ref(w))));
        if dist == 0 {
            return run.len() == 1 || unordinary;
        }
        let named = run.iter().any(|w| w.chars().find(|c| c.is_alphabetic()).is_some_and(char::is_uppercase));
        unordinary
            && named
            && plain_spelling_distance(&heard, replacement) <= 0.34
            && sound_distance(&heard, replacement).0 <= 0.4
    })
}

/// **The names Yap remembers** (`memory/vocab.rs`), corrected more
/// carefully than the person's own dictionary: a near-miss is only taken
/// when what was heard isn't a word in its own right — a single heard word
/// that's an ordinary word (`is_word`: the everyday list, then Windows'
/// spell checker) is never replaced ("lucky" stays "lucky" with a Lucy in
/// memory), nor a run of words that are all ordinary ("row shell" isn't
/// taken for Rochelle). A different spelling of the name ("Shivon",
/// "Pria") or its case ("rochelle") is fixed. `is_word` gets the heard word
/// lowercased.
pub fn apply_names(text: &str, names: &[DictionaryEntry], is_word: &dyn Fn(&str) -> bool) -> String {
    apply_with(text, names, &|run, _, dist| {
        dist == 0 || run.iter().any(|w| !is_word(&build_ngram(std::slice::from_ref(w))))
    })
}

/// [`apply_fuzzy`]'s loop; `accept(run, replacement, dist)` may refuse a
/// match it found (on top of the everyday-word rule below).
fn apply_with(text: &str, dict: &[DictionaryEntry], accept: &dyn Fn(&[&str], &str, usize) -> bool) -> String {
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
        // Nor is a run taken whose first word adds nothing to the match:
        // that protects the word BEFORE a term ("to Kubernetes").
        let mut chosen: Option<(usize, &str, usize)> = None; // (n, replacement, dist)
        for n in 1..=3usize {
            if i + n > words.len() {
                break;
            }
            let run = &words[i..i + n];
            if let Some((replacement, dist)) = find_best_match(&build_ngram(run), &candidates) {
                // An everyday word is never a near-miss of a term, only its
                // exact spelling: "could" isn't "cloud" → "Claude" (the maintainer,
                // 2026-10-21: "was there anything that Claude be changed"),
                // "motion" isn't "Notion" (learn.rs' everyday words).
                if n == 1 && dist > 0 && crate::learn::is_frequent(&build_ngram(run)) {
                    continue;
                }
                if !accept(run, replacement, dist) {
                    continue;
                }
                if chosen.is_none_or(|(_, _, best)| dist < best)
                    && !first_word_adds_nothing(run, replacement, dist, &candidates)
                {
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

// ── How alike two spellings are (learning words, learn.rs) ──────────

/// Lowercased letters and digits only: "Chat G P T" → "chatgpt".
fn letters_key(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

fn normalized_levenshtein(a: &str, b: &str) -> f64 {
    let max = a.chars().count().max(b.chars().count());
    if max == 0 {
        return 0.0;
    }
    levenshtein(a, b) as f64 / max as f64
}

/// How differently `a` and `b` are spelled, 0.0 (the same letters) … 1.0,
/// by Handy's score above (normalised Levenshtein, × 0.3 when the Soundex
/// codes agree) on their letters alone (case, spaces and punctuation
/// ignored), with the longer key's length. No length gate: a phrase heard
/// for one word ("graph ana" / "Grafana") is compared whole.
pub(crate) fn spelling_distance(a: &str, b: &str) -> (f64, usize) {
    let (ka, kb) = (letters_key(a), letters_key(b));
    let len = ka.chars().count().max(kb.chars().count());
    if ka.is_empty() || kb.is_empty() {
        return (1.0, len);
    }
    let score = normalized_levenshtein(&ka, &kb);
    (if soundex_eq(&ka, &kb) { score * 0.3 } else { score }, len)
}

/// How differently `a` and `b` are spelled, letters alone, without
/// [`spelling_distance`]'s Soundex discount: normalised Levenshtein, 0.0 …
/// 1.0. A cap on how far a near-miss may be (`apps.rs`: "spottify" is
/// Spotify, "spotifire" isn't).
pub(crate) fn plain_spelling_distance(a: &str, b: &str) -> f64 {
    let (ka, kb) = (letters_key(a), letters_key(b));
    if ka.is_empty() || kb.is_empty() {
        return 1.0;
    }
    normalized_levenshtein(&ka, &kb)
}

/// How differently `a` and `b` sound, 0.0 … 1.0: normalised Levenshtein
/// between their [`sound_key`]s, with the longer key's length. Catches
/// mishearings across word boundaries that spelling can't — "cube and
/// eighties" / "Kubernetes" ("kpnts" / "kprnts").
pub(crate) fn sound_distance(a: &str, b: &str) -> (f64, usize) {
    let (ka, kb) = (sound_key(&letters_key(a)), sound_key(&letters_key(b)));
    let len = ka.chars().count().max(kb.chars().count());
    if ka.is_empty() || kb.is_empty() {
        return (1.0, len);
    }
    (normalized_levenshtein(&ka, &kb), len)
}

/// A rough sound of an (English) word's letters (`letters_key`): its
/// consonants, with letters that sound alike as one (c/k/q/g → k, b/p → p,
/// d/t → t, f/v/ph → f, s/z and soft c → s, sh/ch → x, th → t, x → ks),
/// silent ones dropped (gh, an initial k/g/p before n, w before r…), vowels,
/// h, w and y only at the start (as "a" for a vowel), and a sound said twice
/// in a row once. A phonetic key in the spirit of Soundex/Metaphone, tuned
/// for what a speech model mishears (voiced/unvoiced pairs, merged words).
pub(crate) fn sound_key(key: &str) -> String {
    let c: Vec<char> = key.chars().collect();
    let vowel = |ch: char| {
        matches!(ch, 'a' | 'e' | 'i' | 'o' | 'u' | 'y' | 'à'..='æ' | 'è'..='ï' | 'ò'..='ö' | 'ø'..='ý' | 'ÿ')
    };
    let mut out: Vec<char> = Vec::new();
    let mut emit = |code: char| {
        if out.last() != Some(&code) {
            out.push(code);
        }
    };
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        let next = c.get(i + 1).copied();
        let first = i == 0;
        let (code, step): (Option<char>, usize) = match (ch, next) {
            ('y', Some(n)) if first && vowel(n) => (Some('y'), 1),
            (v, _) if vowel(v) => (first.then_some('a'), 1),
            ('p', Some('h')) => (Some('f'), 2),
            ('g', Some('h')) => (first.then_some('k'), 2),
            ('c', Some('k')) => (Some('k'), 2),
            ('s' | 'c', Some('h')) => (Some('x'), 2),
            ('t', Some('h')) => (Some('t'), 2),
            ('w', Some('h')) => (first.then_some('w'), 2),
            ('k' | 'g' | 'p', Some('n')) if first => (Some('n'), 2),
            ('w', Some('r')) if first => (Some('r'), 2),
            ('p', Some('s')) if first => (Some('s'), 2),
            ('c', Some('e' | 'i' | 'y')) => (Some('s'), 1),
            ('d', Some('g')) => (Some('j'), 2),
            ('x', _) => {
                emit('k');
                (Some('s'), 1)
            }
            ('b' | 'p', _) => (Some('p'), 1),
            ('d' | 't', _) => (Some('t'), 1),
            ('c' | 'g' | 'k' | 'q', _) => (Some('k'), 1),
            ('f' | 'v', _) => (Some('f'), 1),
            ('s' | 'z', _) => (Some('s'), 1),
            ('h' | 'w', _) => (first.then_some(ch), 1),
            (other, _) => (Some(other), 1),
        };
        if let Some(code) = code {
            emit(code);
        }
        i += step;
    }
    out.into_iter().collect()
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
        DictionaryEntry { from: from.to_string(), to: to.to_string(), fuzzy: true, snippet: false }
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
    fn an_everyday_word_is_never_a_near_miss() {
        // A tester's dictionary: "cloud" → "claude" (Parakeet hears Claude as cloud).
        let dict = vec![entry("cloud", "claude"), entry("", "Notion")];
        assert_eq!(
            apply_fuzzy("was there anything that could be changed", &dict),
            "was there anything that could be changed"
        );
        assert_eq!(apply_fuzzy("put a motion forward", &dict), "put a motion forward");
        // A real near-miss that isn't an everyday word still snaps.
        assert_eq!(apply_fuzzy("ask clawd about it", &dict), "ask claude about it");
        assert_eq!(apply_fuzzy("my notio page", &dict), "my Notion page");
    }

    #[test]
    fn a_term_already_right_never_takes_the_word_before_it() {
        // Found by the real-app robot: "to Kubernetes" is within the
        // threshold of "Kubernetes" only because the term is there.
        let dict = vec![entry("cube and eighties", "Kubernetes")];
        assert_eq!(apply_fuzzy("Deploy it to Kubernetes.", &dict), "Deploy it to Kubernetes.");
        assert_eq!(apply_fuzzy("Use a Kubernetes cluster", &dict), "Use a Kubernetes cluster");
        let json = vec![entry("jaison", "JSON")];
        assert_eq!(apply_fuzzy("send it as JSON", &json), "send it as JSON");
        // A real near-miss next to a short word is still fixed, and only it.
        assert_eq!(apply_fuzzy("Deploy it to Kubernetis.", &dict), "Deploy it to Kubernetes.");
        // Split terms still join ("Chat G PT" holds no word that IS the term).
        let gpt = vec![entry("", "ChatGPT")];
        assert_eq!(apply_fuzzy("ask Chat G PT now", &gpt), "ask ChatGPT now");
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
    fn word_before_a_term_is_not_swallowed() {
        // "to Kubernetes" → "tokubernetes" is 2/12 from "kubernetes": the
        // 2-gram used to win and the "to" vanished (found by the real-app
        // robot, learn.real.js).
        let dict = vec![entry("cube and eighties", "Kubernetes")];
        assert_eq!(apply_fuzzy("Deploy it to Kubernetes.", &dict), "Deploy it to Kubernetes.");
        assert_eq!(apply_fuzzy("run a Kubernetes cluster", &dict), "run a Kubernetes cluster");
        // the near-miss itself is still caught, the "to" kept
        assert_eq!(apply_fuzzy("move it to cube and eighties", &dict), "move it to Kubernetes");
        assert_eq!(apply_fuzzy("move it to cube and eighty", &dict), "move it to Kubernetes");
    }

    #[test]
    fn list_markers_before_a_term_are_kept() {
        let dict = vec![entry("", "Kubernetes")];
        assert_eq!(apply_fuzzy("1. kubernetes", &dict), "1. Kubernetes");
        assert_eq!(apply_fuzzy("- kubernetes", &dict), "- Kubernetes");
        assert_eq!(apply_fuzzy("deploy — Kubernetes", &dict), "deploy — Kubernetes");
        // a near-miss after a bullet too (the bullet adds no letters)
        assert_eq!(apply_fuzzy("- kubernets", &dict), "- Kubernetes");
    }

    #[test]
    fn first_word_of_a_longer_term_still_counts() {
        // "GPT" alone is another dictionary word; "Chad"/"Chat" still make
        // "ChatGPT" with it.
        let dict = vec![entry("", "ChatGPT"), entry("", "GPT")];
        assert_eq!(apply_fuzzy("ask Chad GPT", &dict), "ask ChatGPT");
        assert_eq!(apply_fuzzy("ask Chat GPT", &dict), "ask ChatGPT");
        assert_eq!(apply_fuzzy("ask GPT", &dict), "ask GPT");
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
    fn snippets_are_never_fuzzed() {
        // "my sine of" is a near-miss of the phrase, but a snippet only ever
        // expands on its own words (config::apply_snippets).
        let mut e = entry("my sign off", "Best wishes");
        e.snippet = true;
        assert_eq!(apply_fuzzy("my sine of", &[e]), "my sine of");
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

    // ── how alike two spellings are (learn.rs) ──

    #[test]
    fn sound_keys_merge_what_sounds_alike() {
        assert_eq!(sound_key("cubeandeighties"), "kpnts");
        assert_eq!(sound_key("kubernetes"), "kprnts");
        assert_eq!(sound_key("graphana"), sound_key("grafana"));
        assert_eq!(sound_key("jason"), sound_key("json"));
        assert_eq!(sound_key("knight"), "nt");
        assert_eq!(sound_key("express"), "aksprs");
    }

    #[test]
    fn mishearings_across_word_boundaries_sound_close() {
        // Too far apart by spelling, close by sound.
        let (spelled, _) = spelling_distance("cube and eighties", "Kubernetes");
        let (sounded, len) = sound_distance("cube and eighties", "Kubernetes");
        assert!(spelled > 0.4, "{spelled}");
        assert!(sounded < 0.2 && len == 6, "{sounded} {len}");
        assert!(sound_distance("power to keep", "Parakeet").0 <= 0.4);
        // Spelling: case, spaces and punctuation don't count.
        assert_eq!(spelling_distance("Chat G P T", "ChatGPT"), (0.0, 7));
        // Unrelated words are far apart either way.
        assert!(spelling_distance("the meeting", "Sarah").0 > 0.8);
        assert!(sound_distance("the meeting", "Sarah").0 > 0.8);
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

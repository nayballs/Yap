//! Wake-word detection for the Voice Agent — a faithful port of OpenWhispr's
//! `src/config/agentDetection.ts` (`detectAgentName`). After a normal dictation,
//! the pipeline asks: does the transcript address the agent by name? Three
//! layers, so STT mis-hearings still wake the agent:
//!   1. exact word-boundary match anywhere in the transcript,
//!   2. adjacent-word join ("open whispr" → "openwhispr"),
//!   3. fuzzy Levenshtein match on single words AND adjacent joins, with the
//!      edit budget scaled by name length (≤4 chars: exact only; 5–6: 1 edit;
//!      7+: 2 edits) so short names can't false-positive.
//! This is why saving an agent name also adds it to the correction dictionary.

/// Classic two-row Levenshtein distance (same shape as the TS original).
fn levenshtein(a: &[char], b: &[char]) -> usize {
    let (m, n) = (a.len(), b.len());
    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr: Vec<usize> = vec![0; n + 1];
    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            curr[j] = if a[i - 1] == b[j - 1] {
                prev[j - 1]
            } else {
                1 + prev[j - 1].min(prev[j]).min(curr[j - 1])
            };
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

/// Edit budget by name length (port of `maxEditsForLength`).
fn max_edits_for_length(len: usize) -> usize {
    match len {
        0..=4 => 0,
        5..=6 => 1,
        _ => 2,
    }
}

/// Does `transcript` address the agent named `agent_name`? See module docs.
pub fn detect_agent_name(transcript: &str, agent_name: &str) -> bool {
    let name = agent_name.trim();
    if name.chars().count() < 2 {
        return false;
    }

    // 1. Exact word-boundary match, case-insensitive, anywhere in the text.
    if let Ok(re) = regex::Regex::new(&format!(r"(?i)\b{}\b", regex::escape(name))) {
        if re.is_match(transcript) {
            return true;
        }
    }

    // Tokenize like the TS original: split on whitespace, strip punctuation,
    // lowercase. The name itself is lowercased with internal whitespace removed
    // (so a two-word name can match a joined pair).
    let name_lower: Vec<char> = name
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let words: Vec<Vec<char>> = transcript
        .split_whitespace()
        .map(|w| {
            w.chars()
                .filter(|c| !matches!(c, '.' | ',' | '!' | '?' | ';' | ':' | '\'' | '"' | '(' | ')'))
                .flat_map(|c| c.to_lowercase())
                .collect::<Vec<char>>()
        })
        .filter(|w| !w.is_empty())
        .collect();

    // 2. Adjacent-word join, exact ("open whispr" → "openwhispr").
    for pair in words.windows(2) {
        let joined: Vec<char> = pair[0].iter().chain(pair[1].iter()).copied().collect();
        if joined == name_lower {
            return true;
        }
    }

    // 3. Fuzzy: single words, then adjacent joins, within the edit budget.
    let max_edits = max_edits_for_length(name_lower.len());
    if max_edits == 0 {
        return false;
    }
    for word in &words {
        if word.len().abs_diff(name_lower.len()) <= max_edits
            && levenshtein(word, &name_lower) <= max_edits
        {
            return true;
        }
    }
    for pair in words.windows(2) {
        let joined: Vec<char> = pair[0].iter().chain(pair[1].iter()).copied().collect();
        if joined.len().abs_diff(name_lower.len()) <= max_edits
            && levenshtein(&joined, &name_lower) <= max_edits
        {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_name_matches_anywhere_case_insensitive() {
        assert!(detect_agent_name("Hey Jarvis, write an email", "Jarvis"));
        assert!(detect_agent_name("please jarvis make this shorter", "Jarvis"));
        // mid-text, not just a "Hey X" prefix
        assert!(detect_agent_name("the budget notes. Jarvis, summarize this", "Jarvis"));
        assert!(!detect_agent_name("write an email about the budget", "Jarvis"));
    }

    #[test]
    fn name_inside_another_word_does_not_match() {
        // \b guard: "yap" must not fire inside "yapping"
        assert!(!detect_agent_name("stop yapping about it", "Yap"));
        assert!(detect_agent_name("hey yap make this a list", "Yap"));
    }

    #[test]
    fn adjacent_words_join_to_match_the_name() {
        assert!(detect_agent_name("hey open whispr do the thing", "OpenWhispr"));
    }

    #[test]
    fn fuzzy_matches_scale_with_name_length() {
        // 6-char name: 1 edit allowed → "Jervis" wakes "Jarvis"
        assert!(detect_agent_name("hey jervis fix this", "Jarvis"));
        // ≤4-char name: exact only → "yep" must NOT wake "Yap"
        assert!(!detect_agent_name("yep sounds good", "Yap"));
        // 2 edits on a 7+ name
        assert!(detect_agent_name("hey asistant help me", "Assistant"));
    }

    #[test]
    fn short_or_empty_names_never_match() {
        assert!(!detect_agent_name("a b c", "a"));
        assert!(!detect_agent_name("anything at all", ""));
        assert!(!detect_agent_name("anything at all", "  "));
    }
}

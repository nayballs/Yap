//! What `yap.exe dictation-check` scores a transcription by (pure): how many
//! words it got wrong against what was said, and output that's broken
//! whatever was said (a word stuck on repeat, nothing at all).

use serde::{Deserialize, Serialize};

/// Hesitation sounds: not counted either side (cleanup takes them out, and
/// models write them or not from one version to the next).
const FILLERS: &[&str] = &["um", "umm", "uhm", "uh", "uhh", "er", "erm", "ah", "eh", "hmm", "hm", "mm", "mhm"];

/// A word on repeat is broken from this many in a row ("d d d d"), unless
/// what was said has as many.
pub const REPEAT_MIN: usize = 4;

/// A case scores worse than its baseline when its word error rate rises by
/// this much (5 points: a word in 20).
pub const WORSE_BY: f64 = 0.05;

/// A text's words, as scored: lower case, letters, digits and apostrophes
/// (curly ones straight), without filler sounds. Pure.
pub fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '’')
        .map(|w| w.replace('’', "'").trim_matches('\'').to_lowercase())
        .filter(|w| !w.is_empty() && !FILLERS.contains(&w.as_str()))
        .collect()
}

/// Word error rate of `heard` against `said`: the words substituted,
/// dropped and added, over the words said (above 1 when far more was
/// added; 0 for nothing said and nothing heard). Pure.
pub fn wer(said: &[String], heard: &[String]) -> f64 {
    if said.is_empty() {
        return if heard.is_empty() { 0.0 } else { heard.len() as f64 };
    }
    // Edit distance over words, one row at a time.
    let mut row: Vec<usize> = (0..=heard.len()).collect();
    for (i, s) in said.iter().enumerate() {
        let mut diag = row[0];
        row[0] = i + 1;
        for (j, h) in heard.iter().enumerate() {
            let up = row[j + 1];
            row[j + 1] = if s == h { diag } else { 1 + diag.min(up).min(row[j]) };
            diag = up;
        }
    }
    row[heard.len()] as f64 / said.len() as f64
}

/// The longest run of each word repeated in a row, as (word, times), for
/// runs of 2 or more. Pure.
fn runs(words: &[String]) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let n = words[i..].iter().take_while(|w| **w == words[i]).count();
        if n >= 2 {
            match out.iter_mut().find(|(w, _)| *w == words[i]) {
                Some((_, m)) => *m = (*m).max(n),
                None => out.push((words[i].clone(), n)),
            }
        }
        i += n;
    }
    out
}

/// Something wrong with a transcription whatever the reference says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Flag {
    /// The same word `times` times in a row, more than was said: a decoder
    /// stuck on a frame ("definitive d d d d d d edition", 2026-10-27).
    Repeats { word: String, times: usize },
    /// Nothing heard where something was said.
    Empty,
}

impl Flag {
    pub fn describe(&self) -> String {
        match self {
            Flag::Repeats { word, times } => format!("\"{word}\" {times} times in a row"),
            Flag::Empty => "nothing heard".to_string(),
        }
    }
}

/// What's broken in `heard`, against what was `said`. Pure.
pub fn flags(said: &[String], heard: &[String]) -> Vec<Flag> {
    if heard.is_empty() {
        return if said.is_empty() { Vec::new() } else { vec![Flag::Empty] };
    }
    let said_runs = runs(said);
    runs(heard)
        .into_iter()
        .filter(|(w, n)| *n >= REPEAT_MIN && *n > said_runs.iter().find(|(s, _)| s == w).map_or(1, |(_, m)| *m))
        .map(|(word, times)| Flag::Repeats { word, times })
        .collect()
}

/// Is a case's word error rate now worse than its baseline's? Pure.
pub fn worse(was: f64, now: f64) -> bool {
    now - was >= WORSE_BY - 1e-9
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<String> {
        words(s)
    }

    #[test]
    fn words_are_compared_without_case_punctuation_or_filler() {
        assert_eq!(w("Um, so — I'm sure it's FINE."), ["so", "i'm", "sure", "it's", "fine"]);
        assert_eq!(w("don’t"), ["don't"]);
    }

    #[test]
    fn the_word_error_rate() {
        assert_eq!(wer(&w("the plan is ready"), &w("The plan is ready.")), 0.0);
        // One substituted of four.
        assert_eq!(wer(&w("the plan is ready"), &w("the plan was ready")), 0.25);
        // One dropped, one added.
        assert_eq!(wer(&w("the plan is ready"), &w("plan is ready now")), 0.5);
        // Far more added than said: above 1.
        assert!(wer(&w("d"), &w("d d d d d")) > 1.0);
        assert_eq!(wer(&[], &[]), 0.0);
    }

    #[test]
    fn a_word_on_repeat_is_broken_unless_it_was_said() {
        let heard = w("the definitive d d d d d d edition");
        assert_eq!(flags(&w("the definitive edition"), &heard), vec![Flag::Repeats { word: "d".into(), times: 6 }]);
        // Said seven times: seven is right, ten is not.
        let said = w("I didn't say D D D D D D D, I can assure you");
        assert!(flags(&said, &said).is_empty());
        assert_eq!(flags(&said, &w("I didn't say D D D D D D D D D D")), vec![Flag::Repeats { word: "d".into(), times: 10 }]);
        // Three in a row is speech ("no no no").
        assert!(flags(&w("no"), &w("no no no")).is_empty());
    }

    #[test]
    fn nothing_heard_is_broken_when_something_was_said() {
        assert_eq!(flags(&w("hello there"), &[]), vec![Flag::Empty]);
        assert!(flags(&[], &[]).is_empty());
    }

    #[test]
    fn worse_by_five_points() {
        assert!(worse(0.10, 0.15));
        assert!(!worse(0.10, 0.14));
        assert!(!worse(0.20, 0.05));
    }
}

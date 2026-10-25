//! **Hybrid ranking**: the passages that answer a question, best first, from
//! two lists merged — the **keyword** one (BM25 over each passage's words,
//! where names and numbers, rare in a library, weigh most) and the
//! **meaning** one (the question's embedding against each passage's) — by
//! reciprocal rank fusion (`1 / (K + rank)` from each list a passage is in;
//! OpenWhispr merges its FTS5 and vector hits the same way). A question that
//! names a speaker ("what did Priya say…", "what did I promise…") lifts the
//! passages where that voice talks. At most [`PER_NOTE`] passages from one
//! note, so a long meeting can't crowd out the rest. Pure.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;

/// How the lists are merged (the evaluations compare them: `--search`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Keyword,
    Semantic,
    Hybrid,
}

/// RRF's constant: the larger, the more a passage in both lists beats one
/// near the top of one (60 in the RRF paper and OpenWhispr). With each place
/// weighed by its match's strength ([`Tuning::weighted`]), 10 put the right
/// passage first most often on `evals/search/meetings.json` (recall@1 0.79
/// against 0.76 at 20 and 60; docs/architecture/search.md).
pub const RRF_K: f32 = 10.0;
/// A meaning match this far under the best one is no match (its rank would
/// still add a little to RRF): EmbeddingGemma's unrelated pairs sit at
/// 0.1–0.3, related ones at 0.4–0.7.
pub const SEMANTIC_WINDOW: f32 = 0.22;
/// …and never under this.
pub const SEMANTIC_FLOOR: f32 = 0.2;
/// A passage where a voice the question names talks: its score × this.
pub const SPEAKER_LIFT: f32 = 1.6;
/// At most this many passages from one note.
pub const PER_NOTE: usize = 3;

/// Words that say nothing about which passage answers.
const STOP: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "if", "of", "to", "in", "on", "at", "by", "for", "with", "about", "from", "into", "as",
    "is", "are", "was", "were", "be", "been", "being", "am", "do", "does", "did", "done", "have", "has", "had", "having", "can",
    "could", "would", "should", "will", "shall", "may", "might", "must", "what", "whats", "when", "where", "which", "who",
    "whom", "whose", "why", "how", "this", "that", "these", "those", "there", "here", "then", "than", "it", "its", "i", "me",
    "my", "mine", "we", "us", "our", "you", "your", "they", "them", "their", "he", "him", "his", "she", "her", "hers", "any",
    "all", "some", "so", "not", "no", "yes", "just", "also", "too", "very", "much", "many", "more", "most", "get", "got",
    "tell", "say", "said", "says", "saying", "told", "talk", "talked", "mention", "mentioned", "meeting", "meetings", "call",
    "calls", "discuss", "discussed", "anything", "something", "everything", "thing", "things", "know", "think", "remember",
    "please", "again", "still", "now", "ok", "okay", "yeah", "um", "uh", "like", "well", "really", "actually", "going",
    "gonna", "let", "lets", "want", "need", "s", "t", "d", "ll", "re", "ve", "m",
];

static STOP_SET: LazyLock<HashSet<&'static str>> = LazyLock::new(|| STOP.iter().copied().collect());

/// One word's search form: lower case, an apostrophe's tail off, then a
/// plural "s", an "ing" or "ed", a final "e" and a doubled last letter
/// ("agreed" / "agree" → "agre", "planning" / "plans" → "plan"). Light on
/// purpose: the same for the question and the passages is what counts. Pure.
pub fn stem(word: &str) -> String {
    let mut w = word.to_lowercase().replace('’', "'");
    if let Some(i) = w.find('\'') {
        w.truncate(i);
    }
    let len = |w: &str| w.chars().count();
    if len(&w) > 4 && w.ends_with("ies") {
        w.truncate(w.len() - 3);
        w.push('y');
    } else if len(&w) > 3 && w.ends_with('s') && !w.ends_with("ss") && !w.ends_with("us") && !w.ends_with("is") {
        w.pop();
    }
    if len(&w) > 5 && w.ends_with("ing") {
        w.truncate(w.len() - 3);
    } else if len(&w) > 4 && w.ends_with("ed") {
        w.truncate(w.len() - 2);
    } else if len(&w) > 3 && w.ends_with('e') {
        // "decide" as "decided" leaves it: "decid".
        w.pop();
    }
    let b = w.as_bytes();
    if b.len() > 3 && b[b.len() - 1] == b[b.len() - 2] && b[b.len() - 1].is_ascii_alphabetic() && !b"lsz".contains(&b[b.len() - 1]) {
        w.pop();
    }
    w
}

/// A text's search words ([`stem`]s), stop words out; numbers kept whole
/// ("£4,000" → "4000", "3.5" → "3.5"). Pure.
pub fn terms(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text.split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '’' || c == '.' || c == ',')) {
        let w = raw.trim_matches(|c: char| c == '\'' || c == '’' || c == '.' || c == ',');
        if w.is_empty() {
            continue;
        }
        if w.chars().any(|c| c.is_ascii_digit()) {
            let n: String = w.chars().filter(|c| *c != ',').collect();
            out.push(n.to_lowercase());
            continue;
        }
        // "plan.Then" / "a,b": words that a stop or comma joined.
        for part in w.split(['.', ',']) {
            if part.is_empty() {
                continue;
            }
            let s = stem(part);
            if s.chars().count() > 1 && !STOP_SET.contains(s.as_str()) && !STOP_SET.contains(part.to_lowercase().as_str()) {
                out.push(s);
            }
        }
    }
    out
}

/// BM25 over passages' words: `k1` 1.2, `b` 0.75.
pub struct Bm25 {
    docs: Vec<HashMap<String, u32>>,
    lens: Vec<f32>,
    avg: f32,
    df: HashMap<String, u32>,
}

impl Bm25 {
    pub fn new(docs: &[Vec<String>]) -> Bm25 {
        let mut df: HashMap<String, u32> = HashMap::new();
        let mut maps = Vec::with_capacity(docs.len());
        let mut lens = Vec::with_capacity(docs.len());
        for d in docs {
            let mut tf: HashMap<String, u32> = HashMap::new();
            for t in d {
                *tf.entry(t.clone()).or_default() += 1;
            }
            for t in tf.keys() {
                *df.entry(t.clone()).or_default() += 1;
            }
            lens.push(d.len() as f32);
            maps.push(tf);
        }
        let avg = if lens.is_empty() { 1.0 } else { (lens.iter().sum::<f32>() / lens.len() as f32).max(1.0) };
        Bm25 { docs: maps, lens, avg, df }
    }

    /// How rare `term` is in the library (BM25's IDF; 0 for one it lacks).
    pub fn idf(&self, term: &str) -> f32 {
        let n = self.docs.len() as f32;
        match self.df.get(term) {
            Some(&df) => (1.0 + (n - df as f32 + 0.5) / (df as f32 + 0.5)).ln(),
            None => 0.0,
        }
    }

    /// Passage `i`'s score for the question's terms.
    pub fn score(&self, i: usize, query: &[String]) -> f32 {
        const K1: f32 = 1.2;
        const B: f32 = 0.75;
        let (Some(tf), Some(&len)) = (self.docs.get(i), self.lens.get(i)) else { return 0.0 };
        let mut s = 0.0;
        let mut seen: HashSet<&str> = HashSet::new();
        for t in query {
            if !seen.insert(t) {
                continue;
            }
            if let Some(&f) = tf.get(t) {
                let f = f as f32;
                s += self.idf(t) * f * (K1 + 1.0) / (f + K1 * (1.0 - B + B * len / self.avg));
            }
        }
        s
    }
}

/// A passage as ranking sees it.
pub struct Candidate<'a> {
    /// Its note (for [`PER_NOTE`]).
    pub note: u64,
    pub terms: &'a [String],
    /// Its note's title's search words.
    pub title_terms: &'a [String],
    /// Its speakers' names ("Priya Shah", "You").
    pub speakers: &'a [String],
    /// Its meaning's match with the question (cosine), if it has a vector.
    pub similarity: Option<f32>,
}

/// A passage that made the list.
#[derive(Debug, Clone, PartialEq)]
pub struct Ranked {
    /// Its place in the candidates.
    pub index: usize,
    pub score: f32,
    pub keyword: f32,
    pub similarity: Option<f32>,
    /// A voice the question names talks in it.
    pub speaker: bool,
}

static ASKS_ABOUT_ME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(?:(?:did|have|had|do|was)\s+i|i\s+(?:said|say|mentioned|promised|told|agreed|asked|offered|suggested|committed|volunteered))\b")
        .expect("static regex")
});

/// The voices a question names, out of `known` ("what did Priya say" →
/// "Priya Shah"; "what did I promise" → "You"): a full name, or a first
/// name. Pure.
pub fn named_speakers(question: &str, known: &[&str]) -> Vec<String> {
    let q = format!(" {} ", question.to_lowercase().replace(['?', '!', ',', '.', '\'', '’', '"'], " "));
    let mut out = Vec::new();
    for &name in known {
        let lower = name.to_lowercase();
        if lower == "you" || lower == "them" || lower.starts_with("speaker ") || lower == "someone" {
            continue;
        }
        let first = lower.split_whitespace().next().unwrap_or("");
        let hit = q.contains(&format!(" {lower} ")) || (first.chars().count() > 2 && q.contains(&format!(" {first} ")));
        if hit && !out.iter().any(|o: &String| o == name) {
            out.push(name.to_string());
        }
    }
    if ASKS_ABOUT_ME.is_match(&question.replace('’', "'")) && known.contains(&"You") {
        out.push("You".to_string());
    }
    out
}

/// How the two lists are merged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tuning {
    /// RRF's constant ([`RRF_K`]).
    pub k: f32,
    /// Each list's place weighed by how strong its match is: a keyword
    /// match by the share of the question's rare words it holds (one that
    /// shares only "new" with it counts little), a meaning match by how close
    /// it comes to the best one. Plain RRF counts places only, and a passage
    /// that's in both lists weakly beat one that's first by meaning alone
    /// (`evals/search/meetings.json`: "Are we allowed to take on new staff?"
    /// put the hiring freeze 8th).
    pub weighted: bool,
    /// A word of the question in the note's title counts this much of one in
    /// the passage's own words (every passage of a note has its title).
    pub title: f32,
}

/// Yap's: measured on `evals/search/meetings.json` (docs/architecture/search.md).
pub const TUNING: Tuning = Tuning { k: RRF_K, weighted: true, title: 0.5 };

/// The passages that answer `query` (its words: [`terms`] of the question
/// without its date), best first, at most `limit`, [`PER_NOTE`] a note.
/// `named`: the voices it names ([`named_speakers`]). Pure.
pub fn rank(cands: &[Candidate], query: &[String], named: &[String], mode: Mode, tuning: Tuning, limit: usize) -> Vec<Ranked> {
    let docs: Vec<Vec<String>> = cands.iter().map(|c| c.terms.to_vec()).collect();
    let bm = Bm25::new(&docs);
    let titles = Bm25::new(&cands.iter().map(|c| c.title_terms.to_vec()).collect::<Vec<_>>());
    let unique: Vec<&String> = {
        let mut seen = HashSet::new();
        query.iter().filter(|t| seen.insert(t.as_str())).collect()
    };
    // How much of the question a passage holds, by its words' rarity.
    let total: f32 = unique.iter().map(|t| bm.idf(t).max(titles.idf(t))).sum();
    let mut keyword = vec![0.0f32; cands.len()];
    let mut cover = vec![0.0f32; cands.len()];
    for (i, c) in cands.iter().enumerate() {
        let mut s = bm.score(i, query);
        let mut held = 0.0;
        for t in &unique {
            if c.terms.contains(t) {
                held += bm.idf(t);
            } else if c.title_terms.contains(t) {
                s += tuning.title * titles.idf(t);
                held += tuning.title * titles.idf(t);
            }
        }
        keyword[i] = s;
        cover[i] = if total > 0.0 { (held / total).min(1.0) } else { 0.0 };
    }
    // Each list's ranks (1 = best).
    let mut kw_order: Vec<usize> = (0..cands.len()).filter(|&i| keyword[i] > 0.0).collect();
    kw_order.sort_by(|&a, &b| keyword[b].total_cmp(&keyword[a]).then(a.cmp(&b)));
    let best_sim = cands.iter().filter_map(|c| c.similarity).fold(f32::MIN, f32::max);
    let floor = (best_sim - SEMANTIC_WINDOW).max(SEMANTIC_FLOOR);
    let mut sem_order: Vec<usize> = (0..cands.len()).filter(|&i| cands[i].similarity.is_some_and(|s| s >= floor)).collect();
    sem_order.sort_by(|&a, &b| {
        let (sa, sb) = (cands[a].similarity.unwrap_or(0.0), cands[b].similarity.unwrap_or(0.0));
        sb.total_cmp(&sa).then(a.cmp(&b))
    });
    let strength = |i: usize| {
        let s = cands[i].similarity.unwrap_or(0.0);
        if best_sim > floor {
            ((s - floor) / (best_sim - floor)).clamp(0.05, 1.0)
        } else {
            1.0
        }
    };
    let mut score = vec![0.0f32; cands.len()];
    if mode != Mode::Semantic {
        for (r, &i) in kw_order.iter().enumerate() {
            let w = if tuning.weighted { cover[i].max(0.05) } else { 1.0 };
            score[i] += w / (tuning.k + r as f32 + 1.0);
        }
    }
    if mode != Mode::Keyword {
        for (r, &i) in sem_order.iter().enumerate() {
            let w = if tuning.weighted { strength(i) } else { 1.0 };
            score[i] += w / (tuning.k + r as f32 + 1.0);
        }
    }
    let speaks: Vec<bool> = cands.iter().map(|c| !named.is_empty() && c.speakers.iter().any(|s| named.contains(s))).collect();
    for i in 0..cands.len() {
        if speaks[i] && score[i] > 0.0 {
            score[i] *= SPEAKER_LIFT;
        }
    }
    let mut order: Vec<usize> = (0..cands.len()).filter(|&i| score[i] > 0.0).collect();
    order.sort_by(|&a, &b| score[b].total_cmp(&score[a]).then(a.cmp(&b)));
    let mut per_note: HashMap<u64, usize> = HashMap::new();
    let mut out = Vec::new();
    for i in order {
        let n = per_note.entry(cands[i].note).or_default();
        if *n >= PER_NOTE {
            continue;
        }
        *n += 1;
        out.push(Ranked { index: i, score: score[i], keyword: keyword[i], similarity: cands[i].similarity, speaker: speaks[i] });
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// A line of a passage, as [`best_line`] weighs it.
pub struct LineView<'a> {
    pub terms: &'a [String],
    pub speaker: &'a str,
    /// It ends with a question mark: its answer comes after it.
    pub asks: bool,
}

/// Who a question is about, of the voices it names: "what did **I**
/// promise Ruth" → You; "what did **Priya** say", "Priya's view" → Priya;
/// one name → that one. Pure.
pub fn subject(question: &str, named: &[String]) -> Option<String> {
    if named.iter().any(|n| n == "You") && ASKS_ABOUT_ME.is_match(&question.replace('’', "'")) {
        return Some("You".to_string());
    }
    let q = question.to_lowercase().replace('’', "'");
    for n in named.iter().filter(|n| *n != "You") {
        let first = n.to_lowercase();
        let first = first.split_whitespace().next().unwrap_or("");
        let before = ["did ", "does ", "has ", "have ", "had ", "was ", "is "].iter().any(|w| q.contains(&format!("{w}{first}")));
        let after = ["'s", " say", " said", " told", " ask", " promise", " mention", " think", " thought", " want", " suggest", " agree", " decide"]
            .iter()
            .any(|w| q.contains(&format!("{first}{w}")));
        if before || after {
            return Some(n.clone());
        }
    }
    (named.len() == 1).then(|| named[0].clone())
}

/// The line of a passage a citation opens at: the one sharing the most of
/// the question's rare words (`idf`), the lines of the voice the question
/// is about (`subject`) first and of other voices it names next; none
/// sharing any → that voice's first line, else the first. A line that asks
/// (it ends with "?": "And the tutorial video?") gives way to the reply
/// after it, from someone else — the answer is what's cited. Pure.
pub fn best_line(lines: &[LineView], query: &[String], named: &[String], subject: Option<&str>, idf: impl Fn(&str) -> f32) -> usize {
    let want: HashSet<&str> = query.iter().map(String::as_str).collect();
    let mut best = (0usize, -1.0f32);
    for (i, l) in lines.iter().enumerate() {
        let seen: HashSet<&str> = l.terms.iter().map(String::as_str).filter(|t| want.contains(t)).collect();
        let mut s: f32 = seen.iter().map(|t| idf(t).max(0.1)).sum();
        if subject == Some(l.speaker) {
            s += if s > 0.0 { 1.0 } else { 0.01 };
        } else if named.iter().any(|n| n == l.speaker) && s > 0.0 {
            s += 0.3;
        }
        if s > best.1 {
            best = (i, s);
        }
    }
    let i = best.0;
    match lines.get(i + 1) {
        Some(next) if best.1 > 0.05 && lines[i].asks && next.speaker != lines[i].speaker && !next.asks => i + 1,
        _ => i,
    }
}

/// Cosine similarity (0 for an empty or zero vector): the model evaluations'
/// (debug builds); the store's own is over its unit vectors. Pure.
#[cfg(any(debug_assertions, test))]
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_stemmed_and_stop_words_dropped() {
        assert_eq!(stem("Budgets"), "budget");
        assert_eq!(stem("agreed"), "agre");
        assert_eq!(stem("agree"), "agre");
        assert_eq!(stem("planning"), stem("plans"));
        assert_eq!(stem("planned"), "plan");
        assert_eq!(stem("decided"), stem("decide"));
        assert_eq!(stem("hiring"), stem("hire"));
        assert_eq!(stem("pricing"), stem("prices"));
        assert_eq!(stem("Priya's"), "priya");
        assert_eq!(stem("companies"), "company");
        assert_eq!(stem("status"), "status");
        assert_eq!(stem("class"), "class");
        assert_eq!(terms("What did Priya say about the budgets?"), ["priya", "budget"]);
        // Numbers whole, money and all.
        assert_eq!(terms("We're £4,000 over; 3.5 days."), ["4000", "over", "3.5", "day"]);
        assert_eq!(terms("the plan.Then lunch"), ["plan", "lunch"]);
    }

    fn cand<'a>(note: u64, terms: &'a [String], speakers: &'a [String], sim: Option<f32>) -> Candidate<'a> {
        Candidate { note, terms, title_terms: &[], speakers, similarity: sim }
    }

    fn t(s: &str) -> Vec<String> {
        terms(s)
    }

    #[test]
    fn rare_words_weigh_most() {
        let docs = [t("the budget review went fine"), t("budget budget budget"), t("Priya owns the venue budget"), t("lunch")];
        let bm = Bm25::new(&docs);
        assert!(bm.idf("priya") > bm.idf("budget"));
        assert_eq!(bm.idf("nowhere"), 0.0);
        let q = t("Priya budget");
        assert!(bm.score(2, &q) > bm.score(1, &q), "the name beats repeating a common word");
        assert_eq!(bm.score(3, &q), 0.0);
    }

    #[test]
    fn hybrid_merges_both_lists_and_keeps_names_and_numbers() {
        let none: Vec<String> = Vec::new();
        let d = [
            t("the venue costs 4000 pounds"),      // 0: keyword match on the number
            t("we are over budget on the offsite"), // 1: meaning match only
            t("lunch was nice"),                    // 2: neither
        ];
        let cands = [cand(1, &d[0], &none, Some(0.30)), cand(2, &d[1], &none, Some(0.62)), cand(3, &d[2], &none, Some(0.12))];
        let q = t("the venue at 4000");
        let kw = rank(&cands, &q, &[], Mode::Keyword, TUNING, 5);
        assert_eq!(kw.iter().map(|r| r.index).collect::<Vec<_>>(), [0]);
        let sem = rank(&cands, &q, &[], Mode::Semantic, TUNING, 5);
        // Under the window from the best (0.62 − 0.22 = 0.40): not a match.
        assert_eq!(sem.iter().map(|r| r.index).collect::<Vec<_>>(), [1]);
        let hy = rank(&cands, &q, &[], Mode::Hybrid, TUNING, 5);
        assert_eq!(hy.len(), 2);
        assert!(hy.iter().all(|r| r.index != 2), "a passage in neither list never shows");
        // Without vectors (the model not there): the keywords alone.
        let plain = [cand(1, &d[0], &none, None), cand(2, &d[1], &none, None)];
        assert_eq!(rank(&plain, &q, &[], Mode::Hybrid, TUNING, 5)[0].index, 0);
    }

    #[test]
    fn a_named_voice_lifts_its_passages_and_one_note_cant_take_every_place() {
        let priya = vec!["Priya Shah".to_string()];
        let tom = vec!["Tom".to_string()];
        let a = t("the budget is tight this quarter");
        let b = t("the budget is tight this quarter");
        let cands = [cand(1, &a, &tom, Some(0.6)), cand(2, &b, &priya, Some(0.6))];
        let named = named_speakers("What did Priya say about the budget?", &["Priya Shah", "Tom", "You"]);
        assert_eq!(named, ["Priya Shah"]);
        let r = rank(&cands, &t("budget"), &named, Mode::Hybrid, TUNING, 5);
        assert_eq!(r[0].index, 1);
        assert!(r[0].speaker && !r[1].speaker);
        // Five passages of one note: three make it.
        let same = t("budget");
        let many: Vec<Candidate> = (0..5).map(|_| cand(7, &same, &tom, None)).chain([cand(8, &same, &tom, None)]).collect();
        let r = rank(&many, &t("budget"), &[], Mode::Keyword, TUNING, 10);
        assert_eq!(r.len(), PER_NOTE + 1);
        assert_eq!(r.iter().filter(|x| many[x.index].note == 7).count(), PER_NOTE);
    }

    #[test]
    fn i_means_you_and_speakers_are_matched_by_name() {
        let known = ["You", "Them", "Speaker 2", "Priya Shah", "Al"];
        assert_eq!(named_speakers("what did I promise Priya?", &known), ["Priya Shah", "You"]);
        assert!(named_speakers("what did they say", &known).is_empty());
        assert!(named_speakers("speaker notes", &known).is_empty());
        // A first name of two letters is too loose alone; the full one counts.
        assert!(named_speakers("al dente pasta", &["Al Smith"]).is_empty());
        assert_eq!(named_speakers("what did al smith say", &["Al Smith"]), ["Al Smith"]);
        assert_eq!(named_speakers("did I agree to it?", &known), ["You"]);
    }

    fn view<'a>(lines: &'a [(Vec<String>, &'a str, bool)]) -> Vec<LineView<'a>> {
        lines.iter().map(|(t, s, a)| LineView { terms: t, speaker: s, asks: *a }).collect()
    }

    #[test]
    fn a_citation_opens_at_the_line_with_the_rare_words() {
        let lines = vec![
            (t("Morning everyone"), "Tom", false),
            (t("The venue quote came in at 4000"), "Priya", false),
            (t("That's over the budget"), "Tom", false),
        ];
        let v = view(&lines);
        let idf = |w: &str| if w == "4000" { 3.0 } else { 1.0 };
        assert_eq!(best_line(&v, &t("venue 4000"), &[], None, idf), 1);
        // Nothing shared: the voice it's about's first line, else the first.
        assert_eq!(best_line(&v, &t("pizza"), &["Priya".to_string()], Some("Priya"), idf), 1);
        assert_eq!(best_line(&v, &t("pizza"), &[], None, idf), 0);
        // A shared word on that voice's line wins a tie.
        assert_eq!(best_line(&v, &t("budget venue"), &["Tom".to_string()], Some("Tom"), idf), 2);
    }

    #[test]
    fn a_question_in_the_meeting_gives_way_to_its_answer() {
        let lines = vec![
            (t("And the tutorial video?"), "You", true),
            (t("I'd drop it. Nobody watched it"), "Grace", false),
            (t("Agreed, the video goes."), "You", false),
        ];
        let v = view(&lines);
        assert_eq!(best_line(&v, &t("what did we decide about the tutorial video"), &[], None, |_| 1.0), 1);
        // Not when nobody answers it there.
        let alone = vec![(t("And the tutorial video?"), "You", true), (t("Is it any good?"), "Grace", true)];
        assert_eq!(best_line(&view(&alone), &t("tutorial video"), &[], None, |_| 1.0), 0);
    }

    #[test]
    fn who_a_question_is_about() {
        let named = |n: &[&str]| n.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(subject("What did I promise Ruth?", &named(&["Ruth Adler", "You"])).as_deref(), Some("You"));
        assert_eq!(subject("What did Priya ask me for?", &named(&["Priya Shah"])).as_deref(), Some("Priya Shah"));
        assert_eq!(subject("Tom's plan for the dinner", &named(&["Dana Whitfield", "Tom Okafor"])).as_deref(), Some("Tom Okafor"));
        assert_eq!(subject("the budget with Dana and Tom", &named(&["Dana Whitfield", "Tom Okafor"])), None);
    }

    #[test]
    fn cosines() {
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert!(cosine(&[1.0, 0.0], &[0.0, 1.0]).abs() < 1e-6);
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
    }
}

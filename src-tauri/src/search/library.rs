//! **Searching the library** beyond Ask across meetings (2026-10-25):
//!
//! - **The search boxes** — the Notes view's (every note: title, My
//!   thoughts, summary, transcript) and the Chat sidebar's (titles and
//!   messages): [`search_typed`]. **Full text**: an item holds every word
//!   typed, each at the start of a word ("budg" finds "budget"), in any of
//!   its parts; the line it's on comes back with the words marked, and the
//!   item opens there. **By meaning** too when search by meaning is on: an
//!   item whose passage means the same is found without the words (at most
//!   [`MEANING_ONLY`] of them, and only a close match: the model's
//!   `floors.typed`, `embedder::Floors`).
//!   Ranked by Ask's hybrid ranking (`rank::rank`: BM25 per passage over the
//!   words typed — the last one's endings too — and the meaning list,
//!   weighted reciprocal rank fusion).
//! - **The AI's searches** — Chat's `search_chats` over past chats
//!   ([`find_chats`]) and memory's people, projects and topics a question is
//!   about ([`memory_relevant`]): the question's words or its meaning, the
//!   same ranking. (`search_notes` searches as Ask across meetings does:
//!   `ask::find`, grouped by note in `chat/tools/notes.rs`.)
//!
//! Every list here is pure over [`Doc`]s (the index's prepared passages)
//! and a similarity (the question's cosine with a passage's stored vector,
//! `None` without one); the commands and tools read the stores.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;

use super::chunks;
use super::index::{self, Prepared};
use super::rank::{self, Candidate, Mode};
use super::store::Key;

/// At most this many items found by meaning alone in a search box (the
/// words typed are what it's for; these are "related").
pub const MEANING_ONLY: usize = 5;

/// How close a match must be to count by meaning alone, for the model in
/// use (each model's cosines sit on their own scale; measured on
/// `evals/search/library.json`, `embedder::Floors`): a note or chat in a
/// search box…
pub fn typed_meaning_min() -> f32 {
    super::embedder::model().floors.typed
}

/// …a memory item with a Chat question (it's in every answer's prompt: a
/// loose one would mislead)…
pub fn memory_meaning_min() -> f32 {
    super::embedder::model().floors.memory
}

/// …a past chat for `search_chats`.
pub fn chats_meaning_min() -> f32 {
    super::embedder::model().floors.chats
}
/// A line shown in a result is clipped to about this many characters
/// around the words.
const CLIP: usize = 180;

/// One item to search: a note, a chat (its id, title, passages, last change).
pub struct Doc<'a> {
    pub id: u64,
    pub title: &'a str,
    pub passages: &'a [Prepared],
    pub updated: u64,
}

/// A piece of a line or title as shown: `hit` where a word typed is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Part {
    pub text: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hit: bool,
}

/// One result of a search box.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    /// The note's or chat's id.
    pub id: u64,
    pub title: String,
    /// The title with the words typed marked.
    pub title_parts: Vec<Part>,
    /// Where the line is: "transcript" | "content" | "summary" | "chat", or
    /// "title" when only the title holds the words.
    pub field: &'static str,
    /// The line it opens at: a transcript line's or a message's index; a
    /// line number in the notes or the summary.
    pub line: usize,
    /// When that line was said or written (unix seconds).
    pub ts: u64,
    /// Who said it ("Priya Shah", "You", "Yap"); "" for notes.
    pub speaker: String,
    /// How far into the meeting ("12:40"), for a transcript line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    /// The line, clipped around the words, the words marked.
    pub parts: Vec<Part>,
    /// It holds every word typed (else it's here by meaning).
    pub words: bool,
    /// Its closest passage's match with the query, when meaning took part.
    pub similarity: Option<f32>,
}

/// The words typed, as the search box reads them: lower case, split at
/// anything but letters and digits. Pure.
pub fn typed_words(query: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in query.to_lowercase().split(|c: char| !c.is_alphanumeric()) {
        if !w.is_empty() && !out.iter().any(|o| o == w) {
            out.push(w.to_string());
        }
    }
    out
}

/// Where `word` starts a word in `lower` (already lower case): byte offsets.
fn word_starts<'a>(lower: &'a str, word: &'a str) -> impl Iterator<Item = usize> + 'a {
    lower
        .match_indices(word)
        .map(|(i, _)| i)
        .filter(move |&i| lower[..i].chars().next_back().is_none_or(|c| !c.is_alphanumeric()))
}

/// Does `lower` hold `word` at the start of one of its words? Pure.
pub fn holds(lower: &str, word: &str) -> bool {
    word_starts(lower, word).next().is_some()
}

/// The whole words of `lower` that start with `word` (at most `max`).
fn completions<'a>(lower: &'a str, word: &'a str, max: usize) -> impl Iterator<Item = &'a str> + 'a {
    word_starts(lower, word)
        .map(move |i| {
            let end = lower[i..].find(|c: char| !c.is_alphanumeric()).map_or(lower.len(), |e| i + e);
            &lower[i..end]
        })
        .take(max)
}

/// `text` in parts, each word typed marked where it starts a word (as much
/// of it as was typed), case aside. Pure.
pub fn marked(text: &str, words: &[String]) -> Vec<Part> {
    let chars: Vec<char> = text.chars().collect();
    let lower: Vec<String> = chars.iter().map(|c| c.to_lowercase().collect()).collect();
    let mut hit = vec![false; chars.len()];
    for i in 0..chars.len() {
        if i > 0 && chars[i - 1].is_alphanumeric() {
            continue;
        }
        for w in words {
            let (mut j, mut rest) = (i, w.as_str());
            while !rest.is_empty() && j < chars.len() {
                match rest.strip_prefix(lower[j].as_str()) {
                    Some(r) if !lower[j].is_empty() => {
                        rest = r;
                        j += 1;
                    }
                    _ => break,
                }
            }
            if rest.is_empty() {
                hit[i..j].iter_mut().for_each(|h| *h = true);
            }
        }
    }
    let mut out: Vec<Part> = Vec::new();
    for (c, h) in chars.iter().zip(hit) {
        match out.last_mut() {
            Some(p) if p.hit == h => p.text.push(*c),
            _ => out.push(Part { text: c.to_string(), hit: h }),
        }
    }
    out
}

/// A line clipped to about [`CLIP`] characters around its first mark, at
/// word boundaries, "…" where it was cut. Pure.
pub fn clipped(parts: Vec<Part>) -> Vec<Part> {
    let total: usize = parts.iter().map(|p| p.text.chars().count()).sum();
    if total <= CLIP {
        return parts;
    }
    let first = parts.iter().take_while(|p| !p.hit).map(|p| p.text.chars().count()).sum::<usize>();
    let flat: Vec<(char, bool)> = parts.iter().flat_map(|p| p.text.chars().map(move |c| (c, p.hit))).collect();
    let mut start = first.saturating_sub(CLIP / 3).min(total.saturating_sub(CLIP));
    // Back to the start of a word.
    while start > 0 && !flat[start - 1].0.is_whitespace() {
        start -= 1;
    }
    let mut end = (start + CLIP).min(total);
    while end < total && !flat[end].0.is_whitespace() {
        end += 1;
    }
    let mut out: Vec<Part> = Vec::new();
    for &(c, h) in &flat[start..end] {
        match out.last_mut() {
            Some(p) if p.hit == h => p.text.push(c),
            _ => out.push(Part { text: c.to_string(), hit: h }),
        }
    }
    if start > 0 {
        match out.first_mut() {
            Some(p) if !p.hit => p.text.insert(0, '…'),
            _ => out.insert(0, Part { text: "…".into(), hit: false }),
        }
    }
    if end < total {
        match out.last_mut() {
            Some(p) if !p.hit => p.text.push('…'),
            _ => out.push(Part { text: "…".into(), hit: false }),
        }
    }
    out
}

/// How much of `words` a line holds (lower-cased here): a word the title
/// doesn't hold counts twice (the line that completes the match is the one
/// worth opening).
fn line_hits(text: &str, words: &[String], in_title: &[bool]) -> usize {
    let lower = text.to_lowercase();
    words.iter().zip(in_title).filter(|(w, _)| holds(&lower, w)).map(|(_, t)| if *t { 1 } else { 2 }).sum()
}

/// The query's search terms for BM25: the words typed as `rank::terms`
/// reads them, and each word's completions in the items that hold it
/// ("budg" → "budget"; "planni" → "plan"), so a word half typed ranks
/// too. Pure.
fn typed_terms(query: &str, words: &[String], docs: &[&Doc]) -> Vec<String> {
    let mut out: Vec<String> = rank::terms(query);
    for w in words {
        let mut seen: HashSet<String> = HashSet::new();
        'docs: for d in docs {
            for p in d.passages.iter() {
                for whole in completions(&p.lower, w, 4) {
                    for t in rank::terms(whole) {
                        if seen.insert(t.clone()) && seen.len() > 6 {
                            break 'docs;
                        }
                    }
                }
            }
        }
        for t in seen {
            if !out.contains(&t) {
                out.push(t);
            }
        }
    }
    out
}

/// **A search box's search** over `docs` (the Notes view's notes, the Chat
/// sidebar's chats), best first, at most `limit`: every item holding all
/// the words typed, and — with a similarity — the few that mean it without
/// them. Each comes with the line to open at, its words marked. Pure.
pub fn search_typed(docs: &[Doc], query: &str, similarity: &dyn Fn(&Key) -> Option<f32>, limit: usize) -> Vec<Hit> {
    let words = typed_words(query);
    if words.is_empty() || limit == 0 {
        return Vec::new();
    }
    // Which items hold every word (in their title or any passage).
    let matched: Vec<bool> = docs
        .iter()
        .map(|d| {
            let title = d.title.to_lowercase();
            words.iter().all(|w| holds(&title, w) || d.passages.iter().any(|p| holds(&p.lower, w)))
        })
        .collect();
    // Every passage's meaning (when there's a question's vector).
    let sims: Vec<Vec<Option<f32>>> = docs.iter().map(|d| d.passages.iter().map(|p| similarity(&p.key)).collect()).collect();
    let best = sims.iter().flatten().flatten().fold(f32::MIN, |a, &b| a.max(b));
    let floor = (best - rank::SEMANTIC_WINDOW).max(rank::SEMANTIC_FLOOR).max(typed_meaning_min());
    // The candidates: the passages of items holding the words (keywords
    // and meaning), and passages close in meaning (meaning only).
    let empty: Vec<String> = Vec::new();
    let mut cands: Vec<Candidate> = Vec::new();
    let mut at: Vec<(usize, usize)> = Vec::new();
    for (di, d) in docs.iter().enumerate() {
        for (pi, p) in d.passages.iter().enumerate() {
            let s = sims[di][pi];
            if !matched[di] && s.is_none_or(|s| s < floor) {
                continue;
            }
            cands.push(Candidate {
                note: di as u64,
                terms: if matched[di] { &p.terms } else { &empty },
                title_terms: if matched[di] { &p.title_terms } else { &empty },
                speakers: &empty,
                similarity: s,
            });
            at.push((di, pi));
        }
    }
    let holding: Vec<&Doc> = docs.iter().zip(&matched).filter(|(_, m)| **m).map(|(d, _)| d).collect();
    let terms = typed_terms(query, &words, &holding);
    let ranked = rank::rank(&cands, &terms, &[], Mode::Hybrid, rank::TUNING, cands.len());
    // Each item once, at its best passage.
    let mut order: Vec<(usize, Option<usize>)> = Vec::new();
    let mut placed: HashSet<usize> = HashSet::new();
    let mut by_meaning = 0;
    for r in &ranked {
        let (di, pi) = at[r.index];
        if placed.contains(&di) {
            continue;
        }
        if !matched[di] {
            if by_meaning >= MEANING_ONLY {
                continue;
            }
            by_meaning += 1;
        }
        placed.insert(di);
        order.push((di, Some(pi)));
    }
    // Items holding the words that ranked nothing (their words are stop
    // words, or only in the title): after, the latest first.
    let mut rest: Vec<usize> = (0..docs.len()).filter(|i| matched[*i] && !placed.contains(i)).collect();
    rest.sort_by_key(|&i| std::cmp::Reverse(docs[i].updated));
    order.extend(rest.into_iter().map(|i| (i, None)));
    // A title holding every word first (what a search box is used for most:
    // finding a note by its name), the ranking's order kept within each.
    let titled: Vec<bool> = docs
        .iter()
        .map(|d| {
            let t = d.title.to_lowercase();
            words.iter().all(|w| holds(&t, w))
        })
        .collect();
    order.sort_by_key(|(di, _)| !titled[*di]);
    order.truncate(limit);
    order
        .into_iter()
        .map(|(di, pi)| {
            let d = &docs[di];
            let best_sim = sims[di].iter().flatten().fold(None, |a: Option<f32>, &b| Some(a.map_or(b, |a| a.max(b))));
            hit_of(d, pi, &words, &terms, matched[di], best_sim)
        })
        .collect()
}

/// An item's result: the line to open at — the one holding most of the
/// words (its best passage's first, then any other's), else the passage's
/// best line by its terms — clipped and marked.
fn hit_of(d: &Doc, best: Option<usize>, words: &[String], terms: &[String], matched: bool, similarity: Option<f32>) -> Hit {
    let title_parts = marked(d.title, words);
    let title_lower = d.title.to_lowercase();
    let in_title: Vec<bool> = words.iter().map(|w| holds(&title_lower, w)).collect();
    let all: usize = in_title.iter().map(|t| if *t { 1 } else { 2 }).sum();
    let mut pick: Option<(usize, usize, usize)> = None; // (passage, line, hits)
    if matched {
        let order: Vec<usize> = best.into_iter().chain((0..d.passages.len()).filter(|&i| Some(i) != best)).collect();
        for pi in order {
            for (li, l) in d.passages[pi].chunk.lines.iter().enumerate() {
                let h = line_hits(&l.text, words, &in_title);
                if h > 0 && pick.is_none_or(|(_, _, ph)| h > ph) {
                    pick = Some((pi, li, h));
                }
            }
            if pick.is_some_and(|(_, _, h)| h == all) {
                break;
            }
        }
    }
    let (pi, li) = match (pick, best) {
        (Some((pi, li, _)), _) => (pi, li),
        (None, Some(pi)) => {
            let p = &d.passages[pi];
            let lines: Vec<rank::LineView> = p
                .chunk
                .lines
                .iter()
                .zip(&p.line_terms)
                .map(|(l, t)| rank::LineView { terms: t, speaker: &l.speaker, asks: l.text.trim_end().ends_with('?') })
                .collect();
            (pi, rank::best_line(&lines, terms, &[], None, |_| 1.0))
        }
        (None, None) => (0, 0),
    };
    let only_title = matched && pick.is_none();
    match d.passages.get(pi).and_then(|p| p.chunk.lines.get(li).map(|l| (p, l))) {
        Some((p, l)) => Hit {
            id: d.id,
            title: d.title.to_string(),
            title_parts,
            field: if only_title { "title" } else { p.chunk.field.as_str() },
            line: l.index,
            ts: l.ts,
            speaker: l.speaker.clone(),
            at: None,
            parts: clipped(marked(&l.text, words)),
            words: matched,
            similarity,
        },
        None => Hit {
            id: d.id,
            title: d.title.to_string(),
            title_parts,
            field: "title",
            line: 0,
            ts: d.updated,
            speaker: String::new(),
            at: None,
            parts: Vec::new(),
            words: matched,
            similarity,
        },
    }
}

// ---- the AI's searches -----------------------------------------------------------

/// A past chat a question found (`search_chats`): its id, and the lines
/// that answer (the passage's own, "You: …" / "Yap: …").
#[derive(Debug, Clone, PartialEq)]
pub struct ChatFound {
    pub id: u64,
    pub title: String,
    pub updated: u64,
    /// The message lines (≤ 2), each "User: …" or "Yap's AI: …", clipped.
    pub lines: Vec<String>,
    pub by_meaning: bool,
}

/// **Past chats for a question** (Chat's `search_chats`, a model without
/// tools pointing back at one): the question's words (BM25, a chat needing
/// two of them or one in its title, as the keyword search did) and its
/// meaning, ranked as Ask's; each chat's best passage gives its lines (the
/// best one, and the answer after a question). `docs` without the chat
/// asking. Pure.
pub fn find_chats(docs: &[Doc], question: &str, similarity: &dyn Fn(&Key) -> Option<f32>, limit: usize) -> Vec<ChatFound> {
    let query = rank::terms(question);
    let mut cands: Vec<Candidate> = Vec::new();
    let mut at: Vec<(usize, usize)> = Vec::new();
    let empty: Vec<String> = Vec::new();
    for (di, d) in docs.iter().enumerate() {
        for (pi, p) in d.passages.iter().enumerate() {
            cands.push(Candidate { note: di as u64, terms: &p.terms, title_terms: &p.title_terms, speakers: &empty, similarity: similarity(&p.key) });
            at.push((di, pi));
        }
    }
    let ranked = rank::rank(&cands, &query, &[], Mode::Hybrid, rank::TUNING, cands.len());
    let unique: HashSet<&str> = query.iter().map(String::as_str).collect();
    let need = unique.len().min(2);
    let mut out: Vec<ChatFound> = Vec::new();
    let mut placed: HashSet<usize> = HashSet::new();
    for r in &ranked {
        let (di, pi) = at[r.index];
        if placed.contains(&di) {
            continue;
        }
        let d = &docs[di];
        // Kept by its words (two of them, or one in its title) or its meaning.
        let held: HashSet<&str> = d.passages.iter().flat_map(|p| p.terms.iter().map(String::as_str)).filter(|t| unique.contains(t)).collect();
        let in_title = d.passages.first().is_some_and(|p| p.title_terms.iter().any(|t| unique.contains(t.as_str())));
        let by_words = in_title || (need > 0 && held.len() >= need);
        let by_meaning = r.similarity.is_some_and(|s| s >= chats_meaning_min()) && r.score > 0.0;
        if !by_words && !by_meaning {
            continue;
        }
        placed.insert(di);
        let p = &d.passages[pi];
        let lines: Vec<rank::LineView> = p
            .chunk
            .lines
            .iter()
            .zip(&p.line_terms)
            .map(|(l, t)| rank::LineView { terms: t, speaker: &l.speaker, asks: l.speaker == "You" })
            .collect();
        let first = rank::best_line(&lines, &query, &[], None, |_| 1.0);
        // A question with its answer; an answer with the question before it.
        let pair: Vec<usize> = match p.chunk.lines.get(first).map(|l| l.speaker.as_str()) {
            Some("You") => vec![first, first + 1],
            Some(_) if first > 0 && p.chunk.lines[first - 1].speaker == "You" => vec![first - 1, first],
            _ => vec![first],
        };
        let lines = pair
            .into_iter()
            .filter_map(|i| p.chunk.lines.get(i))
            .map(|l| format!("{}: {}", if l.speaker == "You" { "User" } else { "Yap's AI" }, clip_text(&l.text, &query)))
            .collect();
        out.push(ChatFound { id: d.id, title: d.title.to_string(), updated: d.updated, lines, by_meaning: !by_words });
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// A message clipped to ≤ 240 characters around the first of `terms` in it.
fn clip_text(text: &str, terms: &[String]) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= 240 {
        return text.to_string();
    }
    let lower = text.to_lowercase();
    let at = terms.iter().filter_map(|w| lower.find(w.as_str())).min().unwrap_or(0);
    let char_at = lower.get(..at).map(|p| p.chars().count()).unwrap_or(0).min(chars.len());
    let start = char_at.saturating_sub(80);
    let end = (start + 240).min(chars.len());
    let mut s: String = chars[start..end].iter().collect();
    if start > 0 {
        s = format!("…{s}");
    }
    if end < chars.len() {
        s.push('…');
    }
    s
}

/// **Memory's people, projects and topics a question is about**, by its
/// words (memory's own keyword rule, `memory::prompt::relevant`'s order)
/// and — with a similarity — its meaning (a close one only:
/// [`memory_meaning_min`]), fused as Ask's lists are (weighted reciprocal
/// rank). `keyword`: the item indexes the keyword rule found, best first;
/// `passages`: each item's passage, in `items` order. Returns item
/// indexes, best first, at most `limit`. Pure.
pub fn memory_relevant(keyword: &[usize], passages: &[Option<&Prepared>], similarity: &dyn Fn(&Key) -> Option<f32>, limit: usize) -> Vec<usize> {
    let sims: Vec<Option<f32>> = passages.iter().map(|p| p.and_then(|p| similarity(&p.key))).collect();
    let best = sims.iter().flatten().fold(f32::MIN, |a, &b| a.max(b));
    let floor = (best - rank::SEMANTIC_WINDOW).max(memory_meaning_min());
    let mut by_meaning: Vec<usize> = (0..passages.len()).filter(|&i| sims[i].is_some_and(|s| s >= floor)).collect();
    by_meaning.sort_by(|&a, &b| sims[b].unwrap_or(0.0).total_cmp(&sims[a].unwrap_or(0.0)).then(a.cmp(&b)));
    let k = rank::TUNING.k;
    let mut score: HashMap<usize, f32> = HashMap::new();
    for (r, &i) in keyword.iter().enumerate() {
        *score.entry(i).or_default() += 1.0 / (k + r as f32 + 1.0);
    }
    for (r, &i) in by_meaning.iter().enumerate() {
        let s = sims[i].unwrap_or(0.0);
        let strength = if best > floor { ((s - floor) / (best - floor)).clamp(0.05, 1.0) } else { 1.0 };
        *score.entry(i).or_default() += strength / (k + r as f32 + 1.0);
    }
    let mut order: Vec<(usize, f32)> = score.into_iter().collect();
    // A tie: the item the words found first.
    let place = |i: usize| keyword.iter().position(|&x| x == i).unwrap_or(usize::MAX);
    order.sort_by(|a, b| b.1.total_cmp(&a.1).then(place(a.0).cmp(&place(b.0))).then(a.0.cmp(&b.0)));
    order.into_iter().take(limit).map(|(i, _)| i).collect()
}

// ---- over the stores ---------------------------------------------------------------

/// A model evaluation's baseline (`--search-keyword`: `YAP_EVAL_SEARCH=keyword`):
/// Chat's searches as they were before 25 Oct 2026 — `search_notes` whole
/// notes by keyword count, `search_chats` and memory by their keyword rules.
/// Never in a release build.
pub fn keyword_only() -> bool {
    #[cfg(debug_assertions)]
    if std::env::var("YAP_EVAL_SEARCH").as_deref() == Ok("keyword") {
        return true;
    }
    false
}

/// How long a search box waits for the search model (started if need be)
/// before its words alone answer: the page shows the words' results first
/// and asks again with meaning, so this can be Ask's.
pub const BOX_WAIT: Duration = Duration::from_secs(20);
/// How long Chat's own searches wait (a tool call, a question pointing back
/// at a chat): longer than a warm model takes, short of holding up an answer.
pub const TOOL_WAIT: Duration = Duration::from_secs(8);
/// How long a Chat question waits for memory's meaning: it's on the way to
/// every answer, so only a model that's up (or quick to start) counts; a
/// start goes on in the background for the next.
pub const MEMORY_WAIT: Duration = Duration::from_millis(1500);

/// Every passage's similarity to `vector`, under one look at the store.
fn similarities<'a>(lists: impl Iterator<Item = &'a Arc<Vec<Prepared>>>, vector: Option<&[f32]>) -> HashMap<Key, f32> {
    let Some(q) = vector else { return HashMap::new() };
    let keys: Vec<Key> = lists.flat_map(|v| v.iter().map(|p| p.key)).collect();
    index::with_store(|s| keys.iter().filter_map(|k| s.similarity(k, q).map(|x| (*k, x))).collect())
}

/// **The Notes view's search**: every note by the words typed, and by
/// meaning with `meaning` (the model's answer awaited up to [`BOX_WAIT`]).
/// `(hits, semantic)`: whether meaning took part.
pub async fn notes_now(query: &str, meaning: bool, limit: usize) -> (Vec<Hit>, bool) {
    let vector = if meaning { super::embedder::query_vector(query, BOX_WAIT).await } else { None };
    // Each note's id, title, last change, and when its talk started.
    type Meta = (u64, String, u64, Option<u64>);
    let (meta, prepared): (Vec<Meta>, _) = crate::notes::with_all(|ns| {
        let meta = ns.iter().map(|n| (n.id, n.title.trim().to_string(), n.updated_ts, n.transcript.iter().find(|s| s.is_talk()).map(|s| s.ts))).collect();
        (meta, index::prepared(ns))
    });
    let sims = similarities(prepared.iter(), vector.as_deref());
    let docs: Vec<Doc> = meta.iter().zip(&prepared).map(|((id, title, updated, _), p)| Doc { id: *id, title, passages: p, updated: *updated }).collect();
    let mut hits = search_typed(&docs, query, &|k| sims.get(k).copied(), limit);
    for h in hits.iter_mut().filter(|h| h.field == "transcript") {
        if let Some(start) = meta.iter().find(|m| m.0 == h.id).and_then(|m| m.3) {
            h.at = Some(super::ask::clock(h.ts.saturating_sub(start)));
        }
    }
    (hits, vector.is_some())
}

/// **The Chat sidebar's search**: every conversation (archived ones too)
/// by the words typed, and by meaning with `meaning`.
pub async fn chats_now(query: &str, meaning: bool, limit: usize) -> (Vec<Hit>, bool) {
    let vector = if meaning { super::embedder::query_vector(query, BOX_WAIT).await } else { None };
    // A meeting's Ask threads are the meeting's (`note_chats.rs`), never Chat's.
    let (meta, prepared): (Vec<_>, Vec<_>) = crate::chat::chats::with_all(|cs| {
        let prepared = index::prepared_chats(cs);
        cs.iter().zip(prepared).filter(|(c, _)| c.note.is_none()).map(|(c, p)| ((c.id, c.title.trim().to_string(), c.updated_ts), p)).unzip()
    });
    let sims = similarities(prepared.iter(), vector.as_deref());
    let docs: Vec<Doc> = meta.iter().zip(&prepared).map(|((id, title, updated), p)| Doc { id: *id, title, passages: p, updated: *updated }).collect();
    let hits = search_typed(&docs, query, &|k| sims.get(k).copied(), limit);
    (hits, vector.is_some())
}

/// **Past chats for a question** (`search_chats`, a model without tools
/// pointing back), every conversation but `exclude` (the one asking).
pub async fn chats_for(question: &str, exclude: Option<u64>, limit: usize) -> Vec<ChatFound> {
    let vector = super::embedder::query_vector(question, TOOL_WAIT).await;
    let (meta, prepared): (Vec<_>, Vec<_>) = crate::chat::chats::with_all(|cs| {
        let prepared = index::prepared_chats(cs);
        cs.iter().zip(prepared).filter(|(c, _)| c.note.is_none()).map(|(c, p)| ((c.id, chunks::chat_title(c), c.updated_ts), p)).unzip()
    });
    // The asking chat is left out (its own words aren't a past chat), and a
    // meeting's Ask threads (`note_chats.rs`) are the meeting's.
    let keep: Vec<usize> = (0..meta.len()).filter(|&i| Some(meta[i].0) != exclude).collect();
    let sims = similarities(keep.iter().map(|&i| &prepared[i]), vector.as_deref());
    let docs: Vec<Doc> = keep.iter().map(|&i| Doc { id: meta[i].0, title: &meta[i].1, passages: &prepared[i], updated: meta[i].2 }).collect();
    find_chats(&docs, question, &|k| sims.get(k).copied(), limit)
}

/// A note Chat's `search_notes` found: its passages that matched, best
/// first (≤ 3), as Ask across meetings finds them.
pub struct NoteFound {
    pub id: u64,
    pub score: f32,
    pub passages: Vec<super::ask::Passage>,
}

/// **Chat's `search_notes`** (2026-10-25): the notes and meetings whose
/// passages answer the query — Ask across meetings' search (`ask::find`:
/// BM25 per passage and, when it's on, meaning; the dates and voices the
/// query names) instead of the whole-note keyword count — grouped by note,
/// best first, at most `limit`. `semantic`: meaning took part.
pub async fn notes_for(query: &str, limit: usize) -> (Vec<NoteFound>, bool) {
    let found = super::ask::find_waiting(query, limit.max(1) * rank::PER_NOTE, TOOL_WAIT).await;
    (group_by_note(found.passages, limit), found.semantic)
}

/// Passages grouped by their note, in the order each note first comes. Pure.
pub fn group_by_note(passages: Vec<super::ask::Passage>, limit: usize) -> Vec<NoteFound> {
    let mut out: Vec<NoteFound> = Vec::new();
    for p in passages {
        if let Some(n) = out.iter_mut().find(|n| n.id == p.note) {
            n.passages.push(p);
        } else if out.len() < limit {
            out.push(NoteFound { id: p.note, score: p.score, passages: vec![p] });
        }
    }
    out
}

/// The earlier chats a question points back to, for a model without tools
/// (`chats::refers_to_past`): up to two, their lines ([`chats_for`]).
pub async fn past_chats_block(question: &str, conversation: u64) -> Option<String> {
    let found: Vec<ChatFound> = if keyword_only() {
        crate::chat::chats::search(question, Some(conversation), 2)
            .into_iter()
            .map(|f| ChatFound { id: f.id, title: f.title, updated: f.updated_ts, lines: f.lines, by_meaning: false })
            .collect()
    } else {
        chats_for(question, Some(conversation), 2).await
    };
    if found.is_empty() {
        return None;
    }
    let mut out = String::from("From the user's earlier chats with you (their own words and your answers then):");
    for f in found {
        out.push_str(&format!("\n- \"{}\":", f.title));
        for l in f.lines {
            out.push_str(&format!("\n  {l}"));
        }
    }
    Some(out)
}

/// **The people, projects and topics a Chat question is about**, as its
/// memory block: memory's keyword rule, and with search by meaning on,
/// what it means too ([`memory_relevant`]; the model's answer awaited only
/// [`MEMORY_WAIT`]). `None` when nothing matches.
pub async fn memory_block(question: &str) -> Option<String> {
    let items = crate::memory::items();
    if !items.iter().any(|i| !i.section.is_you()) {
        return None;
    }
    let keyword = crate::memory::prompt::relevant_indexes(&items, question, 3);
    let vector = if keyword_only() { None } else { super::embedder::query_vector(question, MEMORY_WAIT).await };
    let Some(vector) = vector else {
        return crate::memory::prompt::block_of(&keyword.iter().map(|&i| items[i].clone()).collect::<Vec<_>>());
    };
    let prepared = index::prepared_memory(&items);
    let sims = similarities(prepared.iter(), Some(&vector));
    let passages: Vec<Option<&Prepared>> = prepared.iter().map(|p| p.first()).collect();
    let found = memory_relevant(&keyword, &passages, &|k| sims.get(k).copied(), 3);
    if found.iter().any(|i| !keyword.contains(i)) {
        tracing::info!(items = found.len(), by_meaning = found.iter().filter(|i| !keyword.contains(i)).count(), "Memory: found by meaning for a question");
    }
    crate::memory::prompt::block_of(&found.iter().map(|&i| items[i].clone()).collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::chunks::tests::{line, note};
    use crate::search::chunks::Field;
    use crate::search::index;
    use serde_json::json;

    fn text(parts: &[Part]) -> String {
        parts.iter().map(|p| if p.hit { format!("[{}]", p.text) } else { p.text.clone() }).collect()
    }

    #[test]
    fn words_typed_are_found_at_the_start_of_words_and_marked() {
        assert_eq!(typed_words("Budget, VENUE budget"), ["budget", "venue"]);
        assert!(holds("the venue quote", "venue") && holds("the venue quote", "ven"));
        assert!(!holds("the avenue", "venue"), "inside a word isn't a match");
        assert!(holds("£4,000 for the venue", "4"));
        assert_eq!(text(&marked("The Venue quote: venues", &typed_words("ven"))), "The [Ven]ue quote: [ven]ues");
        assert_eq!(text(&marked("Café crème", &typed_words("cafe café"))), "[Café] crème");
        assert_eq!(text(&marked("nothing here", &typed_words("zz"))), "nothing here");
    }

    #[test]
    fn a_long_line_is_clipped_around_its_first_mark() {
        let long = format!("{} the hiring freeze lasts until January {}", "word ".repeat(80), "more ".repeat(80));
        let parts = clipped(marked(&long, &typed_words("hiring")));
        let shown = text(&parts);
        assert!(shown.starts_with('…') && shown.ends_with('…'), "{shown}");
        assert!(shown.contains("[hiring] freeze"));
        assert!(shown.chars().count() < CLIP + 20);
        // A short one stays whole.
        assert_eq!(text(&clipped(marked("a short line", &[]))), "a short line");
    }

    fn library() -> Vec<crate::notes::Note> {
        vec![
            note(json!({ "id": 1, "title": "Q3 budget review", "noteType": "meeting", "updatedTs": 100, "transcript": [
                line("them", "The venue quote came in at four thousand pounds.", 100),
                line("you", "Can we move some of the catering budget across?", 120),
                line("them", "There's a hiring freeze until January.", 140),
            ]})),
            note(json!({ "id": 2, "title": "Dentist", "updatedTs": 300, "content": "Appointment with Dr Patel on Thursday.\n\nBring the budget form." })),
            note(json!({ "id": 3, "title": "Shopping", "updatedTs": 200, "content": "Milk, eggs, bread", "enhancedContent": "## Summary\nGroceries for the week" })),
        ]
    }

    fn docs<'a>(notes: &'a [crate::notes::Note], prepared: &'a [std::sync::Arc<Vec<Prepared>>]) -> Vec<Doc<'a>> {
        notes.iter().zip(prepared).map(|(n, p)| Doc { id: n.id, title: &n.title, passages: p, updated: n.updated_ts }).collect()
    }

    #[test]
    fn the_search_box_finds_every_note_with_all_the_words_and_opens_at_the_line() {
        let notes = library();
        let prepared = index::prepared(&notes);
        let docs = docs(&notes, &prepared);
        let none = |_: &Key| None;
        // Two notes hold "budget": the meeting (title + a line) first by its words.
        let hits = search_typed(&docs, "budget", &none, 10);
        assert_eq!(hits.iter().map(|h| h.id).collect::<Vec<_>>(), [1, 2]);
        assert_eq!((hits[0].field, hits[0].line, hits[0].speaker.as_str()), ("transcript", 1, "You"));
        assert_eq!(text(&hits[0].parts), "Can we move some of the catering [budget] across?");
        assert_eq!(text(&hits[0].title_parts), "Q3 [budget] review");
        assert_eq!((hits[1].field, hits[1].line), ("content", 2), "the notes' third line (a blank between)");
        assert!(hits.iter().all(|h| h.words && h.similarity.is_none()));
        // Every word, anywhere in the note: "budget" in the title, "freeze" in the talk.
        let hits = search_typed(&docs, "budget freeze", &none, 10);
        assert_eq!(hits.iter().map(|h| h.id).collect::<Vec<_>>(), [1]);
        assert_eq!(text(&hits[0].parts), "There's a hiring [freeze] until January.");
        // Half a word: its endings ("hir" → "hiring").
        let hits = search_typed(&docs, "hir", &none, 10);
        assert_eq!(hits.iter().map(|h| h.id).collect::<Vec<_>>(), [1]);
        assert_eq!(text(&hits[0].parts), "There's a [hir]ing freeze until January.");
        // The summary opens at its line; the title alone opens the note.
        let hits = search_typed(&docs, "groceries", &none, 10);
        assert_eq!((hits[0].id, hits[0].field, hits[0].line), (3, "summary", 1));
        let hits = search_typed(&docs, "dentist", &none, 10);
        assert_eq!((hits[0].id, hits[0].field), (2, "title"));
        assert!(search_typed(&docs, "nothing like it", &none, 10).is_empty());
        assert!(search_typed(&docs, "  ", &none, 10).is_empty());
    }

    #[test]
    fn by_meaning_a_close_note_comes_too_and_the_words_still_rank() {
        let notes = library();
        let prepared = index::prepared(&notes);
        let docs = docs(&notes, &prepared);
        // "staff": no note holds it; the hiring line means it.
        let hiring = prepared[0].iter().find(|p| p.lower.contains("hiring")).unwrap().key;
        let shopping = prepared[2][0].key;
        let sim = move |k: &Key| Some(if *k == hiring { 0.62 } else if *k == shopping { 0.25 } else { 0.12 });
        let hits = search_typed(&docs, "staff", &sim, 10);
        assert_eq!(hits.len(), 1, "only the close one: {hits:?}");
        assert_eq!((hits[0].id, hits[0].words, hits[0].field), (1, false, "transcript"));
        assert_eq!(hits[0].similarity, Some(0.62));
        // Words and meaning: the notes holding the word stay in.
        let hits = search_typed(&docs, "budget", &sim, 10);
        assert_eq!(hits.iter().filter(|h| h.words).count(), 2);
    }

    #[test]
    fn a_thousand_notes_are_searched_quickly() {
        let notes: Vec<crate::notes::Note> = (0..1000)
            .map(|i| {
                let lines: Vec<serde_json::Value> = (0..40)
                    .map(|j| line(if j % 2 == 0 { "you" } else { "them" }, &format!("Line {j} of meeting {i} about the roadmap, the budget for quarter {} and hiring plans for team {}.", j % 4, i % 13), 1_000 + j * 20))
                    .collect();
                if i % 3 == 0 {
                    note(json!({ "id": i + 1, "title": format!("Meeting {i}"), "noteType": "meeting", "transcript": lines, "updatedTs": i }))
                } else {
                    note(json!({ "id": i + 1, "title": format!("Note {i}"), "content": format!("Ideas for project {i}: ship the beta, call the venue, book flights to Lisbon.\n\nTodo: review the budget."), "updatedTs": i }))
                }
            })
            .collect();
        let t0 = std::time::Instant::now();
        let _ = index::prepared(&notes);
        let first_ms = t0.elapsed().as_millis();
        // Every search after: the passages from the cache, then the search.
        let t0 = std::time::Instant::now();
        let prepared = index::prepared(&notes);
        let docs = docs(&notes, &prepared);
        let hits = search_typed(&docs, "lisbon budg", &|_: &Key| None, 50);
        let ms = t0.elapsed().as_millis();
        assert_eq!(hits.len(), 50);
        println!("1,000 notes ({} passages): cut in {first_ms} ms once, then searched in {ms} ms", prepared.iter().map(|p| p.len()).sum::<usize>());
        // A debug build is several times slower than the app's release build.
        assert!(ms < 1_500, "{ms} ms");
    }

    fn chat(id: u64, title: &str, messages: &[(&str, &str)]) -> crate::chat::chats::Conversation {
        serde_json::from_value(json!({
            "id": id, "title": title, "createdTs": 100, "updatedTs": 100 + id,
            "messages": messages.iter().enumerate().map(|(i, (r, t))| json!({ "role": r, "text": t, "ts": 100 + i as u64 })).collect::<Vec<_>>(),
        }))
        .unwrap()
    }

    #[test]
    fn past_chats_by_their_words_or_meaning_with_the_question_and_its_answer() {
        let convs = vec![
            chat(1, "Trip ideas", &[("user", "Where should we eat in Lisbon?"), ("assistant", "Try the seafood places in Alfama, and Time Out Market for variety.")]),
            chat(2, "Bread", &[("user", "How long do I prove sourdough?"), ("assistant", "Usually 4 to 6 hours at room temperature.")]),
        ];
        let prepared = index::prepared_chats(&convs);
        let docs: Vec<Doc> = convs.iter().zip(&prepared).map(|(c, p)| Doc { id: c.id, title: &c.title, passages: p, updated: c.updated_ts }).collect();
        let none = |_: &Key| None;
        let found = find_chats(&docs, "Lisbon restaurants", &none, 5);
        assert_eq!(found.len(), 0, "one word, not in a title: not enough by words alone");
        let found = find_chats(&docs, "where did we say to eat in Lisbon", &none, 5);
        assert_eq!(found.iter().map(|f| f.id).collect::<Vec<_>>(), [1]);
        assert_eq!(found[0].lines, ["User: Where should we eat in Lisbon?", "Yap's AI: Try the seafood places in Alfama, and Time Out Market for variety."]);
        // By meaning: "restaurants in Portugal" holds none of its words.
        let trip = prepared[0][0].key;
        let sim = move |k: &Key| Some(if *k == trip { 0.58 } else { 0.1 });
        let found = find_chats(&docs, "restaurants in Portugal", &sim, 5);
        assert_eq!(found.iter().map(|f| (f.id, f.by_meaning)).collect::<Vec<_>>(), [(1, true)]);
    }

    #[test]
    fn memory_by_meaning_needs_a_close_match_and_fuses_with_the_words() {
        let a = Prepared::of(crate::search::chunks::Chunk { note: 1, field: Field::Memory, title: "Rochelle".into(), kind: "memory", heading: String::new(), lines: vec![] });
        let b = Prepared::of(crate::search::chunks::Chunk { note: 2, field: Field::Memory, title: "Beans".into(), kind: "memory", heading: String::new(), lines: vec![] });
        let c = Prepared::of(crate::search::chunks::Chunk { note: 3, field: Field::Memory, title: "Yap".into(), kind: "memory", heading: String::new(), lines: vec![] });
        let (ka, kb) = (a.key, b.key);
        let passages = [Some(&a), Some(&b), Some(&c)];
        // Nothing close: the words' order alone.
        let far = |_: &Key| Some(0.2);
        assert_eq!(memory_relevant(&[2], &passages, &far, 3), [2]);
        // A close meaning joins the words' item.
        let sim = move |k: &Key| Some(if *k == kb { 0.61 } else if *k == ka { 0.3 } else { 0.1 });
        assert_eq!(memory_relevant(&[2], &passages, &sim, 3), [2, 1]);
        assert_eq!(memory_relevant(&[], &passages, &sim, 3), [1]);
        assert!(memory_relevant(&[], &passages, &|_: &Key| None, 3).is_empty());
    }
}

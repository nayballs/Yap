//! **An agent turn's final words, whole** (17 Nov 2026; a Claude in one of
//! the maintainer's chats: a long report's last section, "What I couldn't
//! do", never reached it). Until then a turn's words were cut to 8,000
//! characters as they were saved, and its report told Chat's model 3,000 of
//! them (`agent_status` 1,500, a plan chat's session 4,000), each with "…"
//! where it stopped mid-word, and nothing could read the rest.
//!
//! - **Kept whole** ([`keep`]): up to [`MAX`] characters (a runaway's past
//!   that: cut at a line, said in the log and to the model); past
//!   [`INLINE`], in a file of their own, `agents/<id>/words/<n>.md`, written
//!   through `saving.rs`, with their start in agents.json (what an older Yap
//!   reads: it ends "…"), so agents.json — written whole on every change —
//!   stays small. A file that couldn't be written: the whole words in
//!   agents.json instead (nothing lost). The 7-day sweep leaves them
//!   (`store::sweep`: the agent's record stays, its words with it).
//! - **Read whole** ([`whole`]): the page's copies (`agent_list`,
//!   `agent_get`: the Agent panel, a turn's Open), the report, `agent_status`,
//!   Download your data. A turn saved before (its words cut at 8,000 then)
//!   reads as it was kept — or whole, when it's the agent's latest and
//!   agents.json kept its words whole beside it, as Yap did then.
//! - **Cut at a line or a word** ([`cut`]) where a cut can't be helped,
//!   never mid-word; **read in parts** ([`part`]): from a character on, as
//!   much as the room takes, and where the next part starts.
//!
//! The log has ids, counts and lengths, never the words.

use std::path::PathBuf;

use crate::agents::store::{self, Agent, TurnNote};

/// The most characters of a turn's words agents.json keeps itself (their
/// start, when longer: the rest is in their file).
pub const INLINE: usize = 8_000;
/// The most characters of a turn's words Yap keeps at all (a runaway's past
/// it are cut at a line, and the log and the model are told).
pub const MAX: usize = 200_000;

/// The words' files' saves: a write that fails keeps the words in
/// agents.json instead, so nothing is lost and nothing's said.
pub static SAVER: crate::shell::saving::Saver = crate::shell::saving::Saver::quiet("agentWords", "an agent's report", nothing_waits);

fn nothing_waits() -> Result<(), crate::shell::saving::SaveError> {
    Ok(())
}

/// What a turn's record keeps of its words ([`keep`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Kept {
    /// What agents.json holds: the words, or their start ending "…".
    pub text: String,
    /// The whole words' length in characters when they're in their file
    /// (`TurnNote::words`); 0: `text` is all of them.
    pub chars: u32,
    /// They were longer than [`MAX`] and were cut there.
    pub cut: bool,
}

/// `agents/<id>/words/<n>.md`: turn `n`'s words, whole.
pub fn path(id: &str, n: u32) -> Option<PathBuf> {
    store::dir(id).map(|d| d.join(FOLDER).join(format!("{n}.md")))
}

/// The words' folder in an agent's own (the sweep leaves it).
pub const FOLDER: &str = "words";

/// **Agent `id`'s turn `n` ended saying `words`**: what its record keeps
/// ([`Kept`]) — the words when they're short; else their start, the whole
/// in their file. Reads and writes the file system.
pub fn keep(id: &str, n: u32, words: &str) -> Kept {
    let (whole, cut) = capped(words);
    if cut {
        tracing::info!(id, turn = n, chars = words.chars().count(), kept = MAX, "Agents: a turn's words were longer than Yap keeps; cut at a line there");
    }
    let file = path(id, n);
    let chars = whole.chars().count();
    if chars <= INLINE {
        // (A longer one written before, for a turn carried on since: gone.)
        if let Some(f) = file.filter(|f| f.exists()) {
            let _ = std::fs::remove_file(f);
        }
        return Kept { text: whole, chars: 0, cut };
    }
    let Some(file) = file.filter(|_| !crate::ai::eval::active()) else {
        return Kept { text: whole, chars: 0, cut };
    };
    match SAVER.write(&file, &whole) {
        Ok(()) => Kept { text: preview(&whole, INLINE), chars: chars as u32, cut },
        Err(e) => {
            // Kept whole in agents.json instead: nothing waits to be saved here.
            tracing::info!(id, turn = n, "Agents: a turn's words couldn't go in their own file ({}); kept in agents.json", e.reason.words());
            SAVER.nothing_to_save();
            Kept { text: whole, chars: 0, cut }
        }
    }
}

/// `words` within [`MAX`] (cut at a line or a word), and whether they had
/// to be. Pure.
fn capped(words: &str) -> (String, bool) {
    if words.chars().count() <= MAX {
        return (words.to_string(), false);
    }
    let at = cut(words, MAX);
    (words[..byte_at(words, at)].trim_end().to_string(), true)
}

/// **Turn `t`'s words, whole**: its file's, when it has one that reads; else
/// what its record holds. Reads the file system.
pub fn of_turn(id: &str, t: &TurnNote) -> String {
    if t.words == 0 {
        return t.text.clone();
    }
    match path(id, t.n).map(std::fs::read_to_string) {
        Some(Ok(w)) if !w.is_empty() => w,
        _ => {
            tracing::info!(id, turn = t.n, "Agents: a turn's words' file didn't read; its start instead");
            t.text.clone()
        }
    }
}

/// **The agent with every turn's words whole** ([`of_turn`]), and its own
/// `text` (its latest words) as that turn's — the copies the page, the
/// report and `agent_status` read; never saved back. A turn saved before
/// 17 Nov 2026 (its words cut at 8,000, "…"), the agent's latest, whose
/// whole words agents.json kept as the agent's `text`: those. Reads the
/// file system.
pub fn whole(mut agent: Agent) -> Agent {
    let id = agent.id.clone();
    // The turn the agent's own `text` is (its latest that said anything).
    let latest = agent.turn_list.iter().rposition(|t| !t.text.is_empty());
    let as_kept = latest.map(|i| agent.turn_list[i].text.clone());
    for t in agent.turn_list.iter_mut().filter(|t| t.words > 0) {
        t.text = of_turn(&id, t);
    }
    if let (Some(i), Some(kept)) = (latest, as_kept) {
        let t = &mut agent.turn_list[i];
        if agent.text == kept {
            agent.text = t.text.clone();
        } else if older_whole(&kept, &agent.text) {
            t.text = agent.text.clone();
        }
    }
    agent
}

/// **An older Yap's cut** (`steps::clip` at 8,000: the start, "…") of
/// `whole`. Pure.
fn older_whole(kept: &str, whole: &str) -> bool {
    let Some(start) = kept.strip_suffix('…') else { return false };
    !start.is_empty() && whole.chars().count() > kept.chars().count() && whole.starts_with(start)
}

/// The byte where character `n` of `text` starts (its end past the last). Pure.
pub fn byte_at(text: &str, n: usize) -> usize {
    text.char_indices().nth(n).map_or(text.len(), |(b, _)| b)
}

/// **Where to cut `text` to `max` characters at most**, as a character
/// index: all of it when it fits; else after a paragraph in the last third,
/// else a line in the last half, else a sentence, else a word there — and
/// only with no space at all, mid-word at `max`. Pure.
pub fn cut(text: &str, max: usize) -> usize {
    let chars: Vec<char> = text.chars().take(max + 1).collect();
    if chars.len() <= max {
        return chars.len();
    }
    let (third, half) = (max - max / 3, max / 2);
    // (At `i`: the text before `i` is what's kept.)
    let blank = |i: usize| chars[i] == '\n' && i > 0 && chars[i - 1] == '\n';
    if let Some(i) = (third.max(1)..=max).rev().find(|&i| blank(i)) {
        return i - 1;
    }
    if let Some(i) = (half..=max).rev().find(|&i| chars[i] == '\n') {
        return i;
    }
    let ends = |i: usize| i > 0 && matches!(chars[i - 1], '.' | '!' | '?' | ':' | ';') && chars[i].is_whitespace();
    if let Some(i) = (half.max(1)..=max).rev().find(|&i| ends(i)) {
        return i;
    }
    if let Some(i) = (0..=max).rev().find(|&i| chars[i].is_whitespace()) {
        if i > 0 {
            return i;
        }
    }
    max
}

/// **`text` in `max` characters at most**: as it is when it fits; else cut
/// at a line or a word ([`cut`]) with "…". Pure.
pub fn preview(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let at = cut(text, max.saturating_sub(1));
    format!("{}…", text[..byte_at(text, at)].trim_end())
}

/// **Characters of `text` that fit in `tokens`** as a tool's result or a
/// message carries them (JSON-escaped: a line break, a quote two
/// characters), counted as `budget::estimate` counts: an ASCII character
/// 1/3.6 of a token, any other one a whole one. Pure.
pub fn fitting(text: &str, tokens: u64) -> usize {
    let room = tokens.saturating_mul(36);
    let mut used = 0u64;
    for (i, c) in text.chars().enumerate() {
        let cost = cost(c);
        if used + cost > room {
            return i;
        }
        used += cost;
    }
    text.chars().count()
}

/// The tokens `text` takes as carried ([`fitting`]'s count). Pure.
pub fn tokens_of(text: &str) -> u64 {
    text.chars().map(cost).sum::<u64>().div_ceil(36)
}

/// A character's cost as carried, in 36ths of a token. Pure.
fn cost(c: char) -> u64 {
    if c.is_ascii() {
        10 * match c {
            '"' | '\\' | '\n' | '\r' | '\t' => 2,
            c if (c as u32) < 0x20 => 6,
            _ => 1,
        }
    } else {
        36
    }
}

/// **A part of an agent's words** ([`part`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Part {
    pub text: String,
    /// Where it starts in the words (characters).
    pub from: usize,
    /// Where the next part starts; `None`: it reaches their end.
    pub next: Option<usize>,
    /// The words' length (characters).
    pub total: usize,
}

impl Part {
    /// Characters after this part.
    pub fn left(&self) -> usize {
        self.next.map_or(0, |n| self.total.saturating_sub(n))
    }
}

/// **`words` from character `offset`**: as much as `max` characters and
/// `tokens` (as carried: [`fitting`]; `None`: no limit) hold, cut at a line
/// or a word ([`cut`]), and where the next part starts (at its first word).
/// Pure.
pub fn part(words: &str, offset: usize, max: usize, tokens: Option<u64>) -> Part {
    let total = words.chars().count();
    let from = offset.min(total);
    let rest = &words[byte_at(words, from)..];
    let room = tokens.map_or(max, |t| max.min(fitting(rest, t)));
    // (Never a part of nothing before the end: a pager would never move on.)
    let at = match cut(rest, room) {
        0 if !rest.is_empty() => room.max(1),
        at => at,
    };
    if at >= total - from {
        return Part { text: rest.to_string(), from, next: None, total };
    }
    let skip = rest.chars().skip(at).take_while(|c| c.is_whitespace()).count();
    let next = from + at + skip;
    Part { text: rest[..byte_at(rest, at)].trim_end().to_string(), from, next: (next < total).then_some(next), total }
}

/// The most sections [`sections`] names.
pub const SECTIONS_MAX: usize = 16;

/// **The sections of `words` from character `from` on**: each heading (a
/// markdown heading line, or a line that's all bold) and where its line
/// starts, [`SECTIONS_MAX`] at most — so a model told part of a long report
/// can go straight to the one it needs ("What I couldn't do"), as a page's
/// headings in a `browser_read` part. Pure.
pub fn sections(words: &str, from: usize) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let mut at = 0usize;
    for line in words.split('\n') {
        let here = at;
        at += line.chars().count() + 1;
        if here < from {
            continue;
        }
        let l = line.trim();
        let hashes = l.chars().take_while(|c| *c == '#').count();
        let heading = if (1..=6).contains(&hashes) && l[hashes..].starts_with(' ') {
            l[hashes..].trim()
        } else if l.len() > 4 && l.starts_with("**") && l.trim_end_matches(':').ends_with("**") && !l[2..l.trim_end_matches(':').len() - 2].contains("**") {
            l.trim_end_matches(':').trim_matches('*').trim().trim_end_matches(':').trim()
        } else {
            continue;
        };
        if !heading.is_empty() {
            out.push((preview(heading, 80), here));
            if out.len() == SECTIONS_MAX {
                break;
            }
        }
    }
    out
}

/// **Each of several words' share of `room` tokens** (a report of several
/// requests): the ones that fit whole get it whole, the rest share what's
/// left evenly. Each one's tokens, in order. Pure.
pub fn shares(needs: &[u64], room: u64) -> Vec<u64> {
    let mut out = vec![0u64; needs.len()];
    let mut left = room;
    let mut open: Vec<usize> = (0..needs.len()).collect();
    // The smallest first: whole while the even share holds them.
    open.sort_by_key(|&i| needs[i]);
    let mut k = open.len() as u64;
    for &i in &open {
        let even = left / k.max(1);
        let give = needs[i].min(even);
        out[i] = give;
        left -= give;
        k -= 1;
    }
    out
}

/// "12,345": a count as a sentence says it. Pure.
pub fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::store::Status;

    /// A long report, its last section's unique word at the end.
    fn report() -> String {
        let mut s = String::from("## What I did\n\n");
        for i in 0..300 {
            s.push_str(&format!("- Step {i}: I lined up bed {i} with the path and checked the watering schedule.\n"));
        }
        s.push_str("\n## What I couldn't do\n\nThe greenhouse sensor (PELICAN) didn't answer, so its readings are still missing.\n");
        s
    }

    #[test]
    fn a_long_turns_words_are_kept_whole_beside_its_start() {
        let words = report();
        assert!(words.chars().count() > INLINE * 2);
        let kept = keep("ab12", 1, &words);
        // agents.json keeps their start, cut at a line, "…" at its end.
        assert!(kept.text.chars().count() <= INLINE, "{}", kept.text.chars().count());
        assert!(kept.text.ends_with('…'));
        assert!(kept.text.trim_end_matches('…').ends_with("schedule."), "cut at a line: {:?}", &kept.text[kept.text.len() - 40..]);
        assert_eq!((kept.chars as usize, kept.cut), (words.chars().count(), false));
        // The whole is in its file, and reads back whole.
        let file = path("ab12", 1).unwrap();
        assert!(file.ends_with(std::path::Path::new("agents").join("ab12").join("words").join("1.md")));
        let t = TurnNote { n: 1, text: kept.text.clone(), words: kept.chars, ..Default::default() };
        assert_eq!(of_turn("ab12", &t), words);
        // The agent as the page and the report read it: every turn whole, and its own text.
        let agent = Agent { id: "ab12".into(), text: kept.text.clone(), status: Status::Done, turn_list: vec![TurnNote { status: Status::Done, ..t.clone() }], ..Default::default() };
        let read = whole(agent.clone());
        assert_eq!(read.text, words);
        assert_eq!(read.turn_list[0].text, words);
        assert!(read.text.contains("PELICAN"));
        // Short words stay in agents.json as they are; a longer file before them goes.
        let short = keep("ab12", 1, "Done. I lined the beds up.");
        assert_eq!(short, Kept { text: "Done. I lined the beds up.".into(), chars: 0, cut: false });
        assert!(!file.exists());
        // Its file gone (a hand-deleted folder): its start, never nothing.
        assert_eq!(of_turn("ab12", &t), kept.text);
    }

    #[test]
    fn a_runaways_words_are_kept_to_the_most_and_said_so() {
        let line = "The seed tracker's planting row was checked again and is fine.\n";
        let words = line.repeat(MAX / line.len() + 50);
        let kept = keep("cd34", 2, &words);
        assert!(kept.cut);
        let t = TurnNote { n: 2, text: kept.text, words: kept.chars, ..Default::default() };
        let back = of_turn("cd34", &t);
        assert!(back.chars().count() <= MAX);
        assert!(back.ends_with("fine."), "cut at a line");
        assert_eq!(kept.chars as usize, back.chars().count());
    }

    #[test]
    fn an_older_turn_reads_as_it_was_kept_or_whole_from_the_agents_own_words() {
        // Saved before 17 Nov 2026: the turn's words cut at 8,000 ("…"), the
        // agent's latest words whole in agents.json.
        let words = report();
        let older_cut = crate::agents::steps::clip(&words, 8_000);
        let old = Agent {
            id: "ef56".into(),
            text: words.clone(),
            turn_list: vec![TurnNote { n: 1, text: "Done earlier.".into(), ..Default::default() }, TurnNote { n: 2, text: older_cut.clone(), ..Default::default() }],
            ..Default::default()
        };
        let read = whole(old.clone());
        assert_eq!(read.turn_list[1].text, words, "the latest turn whole, from the agent's own words");
        assert_eq!(read.turn_list[0].text, "Done earlier.");
        assert_eq!(read.text, words);
        // An earlier turn's cut words: as they were kept (nothing else has them).
        let earlier = Agent { text: "Done later.".into(), turn_list: vec![TurnNote { n: 1, text: older_cut.clone(), ..Default::default() }, TurnNote { n: 2, text: "Done later.".into(), ..Default::default() }], ..old };
        assert_eq!(whole(earlier).turn_list[0].text, older_cut);
        // An older record with no turns: as it is.
        let none = Agent { id: "ef57".into(), text: "Done.".into(), ..Default::default() };
        assert_eq!(whole(none.clone()), none);
        // An older Yap's agents.json (no `words`) reads; a turn without words keeps none.
        let t: TurnNote = serde_json::from_value(serde_json::json!({ "n": 1, "text": "Done." })).unwrap();
        assert_eq!(t.words, 0);
        assert!(serde_json::to_value(&t).unwrap().get("words").is_none());
        let t: TurnNote = serde_json::from_value(serde_json::json!({ "n": 1, "text": "Start…", "words": 12000 })).unwrap();
        assert_eq!(t.words, 12_000);
    }

    #[test]
    fn a_cut_is_never_mid_word_or_mid_line() {
        let text = "First line of the report.\nSecond line goes on for a while here.\n\nA new paragraph starts.";
        // A paragraph break in the last third: there.
        let at = cut(text, text.chars().count() - 5);
        assert_eq!(&text[..at], "First line of the report.\nSecond line goes on for a while here.");
        // A line in the last half.
        let at = cut(text, 50);
        assert_eq!(&text[..at], "First line of the report.");
        // No line there: a sentence, else a word.
        let prose = "One sentence here. Another one that keeps going with words";
        assert_eq!(&prose[..cut(prose, 30)], "One sentence here.");
        assert_eq!(&prose[..cut(prose, 40)], "One sentence here. Another one that");
        assert_eq!(&prose[..cut(prose, prose.len() - 3)], "One sentence here. Another one that keeps going with");
        // One long word: there's nowhere else.
        assert_eq!(cut("abcdefghij", 4), 4);
        // It fits: all of it.
        assert_eq!(cut("short", 10), 5);
        assert_eq!(preview("short", 10), "short");
        assert_eq!(preview("The tests pass and the readme is tidy.", 20), "The tests pass and…");
        assert!(preview("The tests pass and the readme is tidy.", 20).chars().count() <= 20);
        assert_eq!(thousands(1_234_567), "1,234,567");
        assert_eq!(thousands(999), "999");
    }

    #[test]
    fn a_long_report_reads_in_parts_that_add_up_to_it() {
        let words = report();
        let total = words.chars().count();
        // Parts within 1,000 tokens each, as carried (JSON-escaped).
        let mut at = 0;
        let mut read = Vec::new();
        loop {
            let p = part(&words, at, MAX, Some(1_000));
            // (Its two quotes aside.)
            assert!(crate::chat::budget::estimate(&serde_json::to_string(&p.text).unwrap()) <= 1_001);
            assert_eq!((p.from, p.total), (at, total));
            // Cut at a line's end.
            let after = words.chars().nth(p.from + p.text.chars().count());
            assert!(after.is_none_or(|c| c == '\n'), "cut at a line: {:?}", &p.text[p.text.len().saturating_sub(30)..]);
            read.push(p.text.clone());
            match p.next {
                Some(n) => {
                    assert!(n > at);
                    assert_eq!(p.left(), total - n);
                    at = n;
                }
                None => break,
            }
        }
        assert!(read.len() > 3);
        assert!(read.last().unwrap().contains("PELICAN"));
        // Nothing lost but the whitespace between parts.
        let squash = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
        assert_eq!(squash(&read.join("\n")), squash(&words));
        // The room holds it all: one part.
        let all = part(&words, 0, MAX, None);
        assert_eq!((all.text.as_str(), all.next, all.left()), (words.as_str(), None, 0));
        // From an offset past the end: nothing, nothing next.
        assert_eq!(part(&words, total + 5, MAX, None), Part { text: String::new(), from: total, next: None, total });
        // A character cap holds it too.
        assert!(part(&words, 0, 500, None).text.chars().count() <= 500);
    }

    #[test]
    fn a_reports_sections_are_its_headings_and_bold_lines_where_they_start() {
        let words = report();
        let all = sections(&words, 0);
        let last = words.find("## What I couldn't do").unwrap();
        assert_eq!(all, vec![("What I did".to_string(), 0), ("What I couldn't do".to_string(), last)]);
        // From a part's end on: only the ones after it.
        assert_eq!(sections(&words, 100), vec![("What I couldn't do".to_string(), last)]);
        assert!(sections(&words, last + 1).is_empty());
        // A bold line is a heading too; bold inside a line, a hashtag, code are not.
        let text = "Intro.\n**Tests:**\nAll pass.\nI used **bold** here.\n#hashtag\n    ## Indented heading\n**a** and **b**";
        let found: Vec<String> = sections(text, 0).into_iter().map(|(h, _)| h).collect();
        assert_eq!(found, ["Tests", "Indented heading"]);
        // Read alone from its offset: just that section, to the end.
        let p = part(&words, last, MAX, Some(200));
        assert!(p.text.starts_with("## What I couldn't do") && p.text.contains("PELICAN") && p.next.is_none());
    }

    #[test]
    fn several_requests_share_the_room_the_short_ones_whole() {
        assert_eq!(shares(&[100, 5_000, 200], 3_000), vec![100, 2_700, 200]);
        assert_eq!(shares(&[4_000, 5_000], 3_000), vec![1_500, 1_500]);
        assert_eq!(shares(&[10, 20], 3_000), vec![10, 20]);
        assert!(shares(&[], 3_000).is_empty());
        // Non-ASCII counts a token a character, as `estimate` does.
        assert_eq!(fitting("ééé", 2), 2);
        assert_eq!(fitting("abc\"", 1), 3);
    }
}

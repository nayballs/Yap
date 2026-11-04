//! A read transcript as a meeting note's lines and voices, and what the
//! preview says about it. Pure.
//!
//! **Voices** (`speakers.rs`): a speaker the file names is a person on the
//! call side — `source` "them", a voice "p-1", "p-2"… named in the note's
//! speaker map as the call app said it (`NameSource::Platform`: the person
//! can rename them, and no guess replaces them). The person using Yap is
//! "You" (`source` "you"): Granola's "Me", or the speaker they pick in the
//! preview (About you's name, when the file has it, picked for them). An
//! unnamed label ("Speaker 2", Otter's and Zoom's) is a voice told apart
//! but not named ("them-2": the naming cards may suggest who), and a line
//! the file doesn't say who said is "Them".
//!
//! **Times**: each line is placed on the meeting's clock (`start_ms` /
//! `end_ms`, 0 = when it began) and dated `start + start_ms` — no audio
//! behind them, so ▶ and Transcribe again aren't offered. Two lines of a
//! side never share a `start_ms` (it's how the windows find a line).

use std::collections::HashMap;

use serde::Serialize;

use super::parse::{unnamed_label, Parsed};
use crate::notes::TranscriptSegment;
use crate::speakers::{NameSource, Speaker, Speakers, THEM, YOU};

/// Who a file's speaker is in the note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Voice {
    You,
    Them,
    /// "Speaker 2": told apart, not named.
    Numbered(u32),
    Named(String),
}

/// The voice for a speaker the file calls `name`; `me`: the one the person
/// said is them.
pub fn voice(name: &str, me: Option<&str>) -> Voice {
    let n = name.trim();
    if n.is_empty() {
        return Voice::Them;
    }
    match n.to_lowercase().as_str() {
        "me" | "you" => return Voice::You,
        "them" | "others" | "everyone else" => return Voice::Them,
        _ => {}
    }
    if me.is_some_and(|m| m.trim().eq_ignore_ascii_case(n)) {
        return Voice::You;
    }
    match unnamed_label(n) {
        Some(Some(k)) => Voice::Numbered(k),
        Some(None) => Voice::Them,
        None => Voice::Named(n.to_string()),
    }
}

/// A speaker the preview lists: as the transcript will show them, and how
/// much they said.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub name: String,
    pub lines: usize,
    pub words: usize,
    /// Someone the file names (the person can say it's them).
    pub person: bool,
    /// The file says it's the person using Yap (Granola's "Me").
    pub you: bool,
}

/// The speakers in `p`, in order of their first line.
pub fn found(p: &Parsed) -> Vec<Found> {
    let mut out: Vec<Found> = Vec::new();
    for line in &p.lines {
        let v = voice(&line.speaker, None);
        let name = match &v {
            Voice::You => "You".to_string(),
            Voice::Them => "Them".to_string(),
            Voice::Numbered(k) => format!("Speaker {k}"),
            Voice::Named(n) => n.clone(),
        };
        let words = line.text.split_whitespace().count();
        match out.iter_mut().find(|f| f.name.eq_ignore_ascii_case(&name)) {
            Some(f) => {
                f.lines += 1;
                f.words += words;
            }
            None => out.push(Found { name, lines: 1, words, person: matches!(v, Voice::Named(_)), you: v == Voice::You }),
        }
    }
    out
}

/// Which of `found` is the person using Yap, by About you's `name`: the same
/// name, else the one person whose first name it is. `None` when the file
/// says itself (Granola's "Me") or nobody matches.
pub fn me_from(found: &[Found], name: &str) -> Option<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() || found.iter().any(|f| f.you) {
        return None;
    }
    let people: Vec<&Found> = found.iter().filter(|f| f.person).collect();
    if let Some(f) = people.iter().find(|f| f.name.eq_ignore_ascii_case(&name)) {
        return Some(f.name.clone());
    }
    let first = |s: &str| s.split_whitespace().next().unwrap_or("").to_lowercase();
    let mine = first(&name);
    let alike: Vec<&&Found> = people.iter().filter(|f| first(&f.name) == mine).collect();
    match alike.as_slice() {
        [one] => Some(one.name.clone()),
        _ => None,
    }
}

/// How long the meeting ran (ms): to its last line's end.
pub fn length_ms(p: &Parsed) -> u64 {
    p.lines.iter().map(|l| l.end_ms.unwrap_or(l.start_ms).max(l.start_ms)).max().unwrap_or(0)
}

/// The note's lines, voices and attendees.
#[derive(Debug, Default)]
pub struct Built {
    pub transcript: Vec<TranscriptSegment>,
    pub speakers: Speakers,
    pub participants: Vec<String>,
}

/// `p` as a meeting that started at `start` (unix seconds); `me`: the
/// speaker who is the person using Yap.
pub fn build(p: &Parsed, start: u64, me: Option<&str>) -> Built {
    let mut out = Built::default();
    let mut ids: HashMap<String, String> = HashMap::new();
    let mut last: HashMap<&'static str, u64> = HashMap::new();
    for line in &p.lines {
        let v = voice(&line.speaker, me);
        let (source, id): (&'static str, String) = match v {
            Voice::You => (YOU, YOU.to_string()),
            Voice::Them => (THEM, THEM.to_string()),
            Voice::Numbered(k) => (THEM, format!("{THEM}-{k}")),
            Voice::Named(name) => {
                let key = name.to_lowercase();
                let next = ids.len() + 1;
                let id = ids.entry(key).or_insert_with(|| {
                    let id = format!("p-{next}");
                    out.speakers.insert(id.clone(), Speaker { name: name.clone(), source: Some(NameSource::Platform), ..Default::default() });
                    if !out.participants.iter().any(|p| p.eq_ignore_ascii_case(&name)) {
                        out.participants.push(name.clone());
                    }
                    id
                });
                (THEM, id.clone())
            }
        };
        // A line of each side has a place of its own.
        let mut at = line.start_ms;
        if let Some(&prev) = last.get(source) {
            if at <= prev {
                at = prev + 1;
            }
        }
        last.insert(source, at);
        let end = line.end_ms.unwrap_or(at).max(at);
        out.transcript.push(TranscriptSegment {
            source: source.to_string(),
            text: line.text.clone(),
            ts: start + at / 1000,
            speaker: if id == source { String::new() } else { id },
            start_ms: Some(at),
            end_ms: Some(end),
            ..Default::default()
        });
    }
    // The file's own list of who was there first, then whoever spoke.
    let mut people: Vec<String> = Vec::new();
    for name in p.attendees.iter().chain(out.participants.iter()) {
        let mine = me.is_some_and(|m| m.trim().eq_ignore_ascii_case(name.trim()));
        if !mine && !people.iter().any(|q| q.eq_ignore_ascii_case(name)) {
            people.push(name.clone());
        }
    }
    out.participants = people;
    out
}

#[cfg(test)]
mod tests {
    use super::super::parse::Line;
    use super::*;

    fn parsed(lines: &[(&str, u64, &str)]) -> Parsed {
        Parsed {
            lines: lines
                .iter()
                .map(|(s, at, t)| Line { speaker: s.to_string(), start_ms: *at, end_ms: Some(at + 2_000), text: t.to_string() })
                .collect(),
            attendees: vec!["Amara Okafor".into(), "Priya Shah".into()],
            ..Default::default()
        }
    }

    #[test]
    fn named_people_numbered_voices_you_and_them() {
        let p = parsed(&[
            ("Priya Shah", 0, "Morning all."),
            ("Tom Baker", 1_500, "Morning."),
            ("Speaker 3", 4_000, "Hi, I'm new."),
            ("", 6_000, "Inaudible."),
            ("Imogen Reyes", 8_000, "Let's start."),
            ("PRIYA SHAH", 9_000, "Sure."),
        ]);
        let b = build(&p, 1_800_000_000, Some("Imogen Reyes"));
        let lines: Vec<_> = b.transcript.iter().map(|s| (s.source.as_str(), s.speaker.as_str(), s.ts)).collect();
        assert_eq!(
            lines,
            [
                ("them", "p-1", 1_800_000_000),
                ("them", "p-2", 1_800_000_001),
                ("them", "them-3", 1_800_000_004),
                ("them", "", 1_800_000_006),
                ("you", "", 1_800_000_008),
                ("them", "p-1", 1_800_000_009),
            ]
        );
        assert_eq!(b.speakers["p-1"].name, "Priya Shah");
        assert_eq!(b.speakers["p-1"].source, Some(NameSource::Platform));
        assert!(!b.speakers["p-1"].locked);
        assert_eq!(b.speakers["p-2"].name, "Tom Baker");
        assert_eq!(b.speakers.len(), 2);
        // The file's attendees, then whoever spoke; never you.
        assert_eq!(b.participants, ["Amara Okafor", "Priya Shah", "Tom Baker"]);
        assert_eq!(b.transcript[0].start_ms, Some(0));
        assert_eq!(b.transcript[0].end_ms, Some(2_000));
        // How the note shows them.
        let mut note: crate::notes::Note = serde_json::from_value(serde_json::json!({ "id": 1, "createdTs": 1, "updatedTs": 1 })).unwrap();
        note.transcript = b.transcript;
        note.speakers = b.speakers;
        let shown: Vec<String> = note.transcript.iter().map(|s| crate::speakers::display_line(&note.speakers, s)).collect();
        assert_eq!(shown, ["Priya Shah", "Tom Baker", "Speaker 3", "Them", "You", "Priya Shah"]);
    }

    #[test]
    fn overlapping_lines_of_a_side_keep_places_of_their_own() {
        let p = parsed(&[("Priya Shah", 1_000, "One."), ("Tom Baker", 1_000, "Two."), ("Priya Shah", 1_000, "Three.")]);
        let b = build(&p, 0, None);
        let at: Vec<_> = b.transcript.iter().map(|s| s.start_ms.unwrap()).collect();
        assert_eq!(at, [1_000, 1_001, 1_002]);
    }

    #[test]
    fn granola_me_and_them() {
        let p = parsed(&[("Me", 0, "Shall we?"), ("Them", 2_000, "Yes.")]);
        let f = found(&p);
        assert_eq!(f.iter().map(|f| (f.name.as_str(), f.you, f.person)).collect::<Vec<_>>(), [("You", true, false), ("Them", false, false)]);
        let b = build(&p, 0, None);
        assert_eq!(b.transcript[0].source, "you");
        assert_eq!(b.transcript[1].source, "them");
        assert!(b.speakers.is_empty());
    }

    #[test]
    fn about_you_picks_the_person() {
        let p = parsed(&[("Priya Shah", 0, "a b"), ("Tom Baker", 1_000, "c"), ("Tom Ellis", 2_000, "d"), ("Priya Shah", 3_000, "e")]);
        let f = found(&p);
        assert_eq!(f[0], Found { name: "Priya Shah".into(), lines: 2, words: 3, person: true, you: false });
        assert_eq!(me_from(&f, "priya shah").as_deref(), Some("Priya Shah"));
        assert_eq!(me_from(&f, "Priya").as_deref(), Some("Priya Shah"));
        // Two Toms: not sure which.
        assert_eq!(me_from(&f, "Tom"), None);
        assert_eq!(me_from(&f, ""), None);
        assert_eq!(length_ms(&p), 5_000);
    }
}

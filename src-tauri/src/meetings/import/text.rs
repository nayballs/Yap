//! Plain-text transcripts, line by line (a Word document's paragraphs come
//! here too, `docx.rs`):
//!
//! - **Otter's `.txt`** and **Teams' Word download**: blocks headed
//!   "Priya Shah  0:01" (the name, spaces, the time into the meeting), the
//!   words on the lines under it. Otter's "Speaker 1" stays a speaker Yap
//!   can name later. Above the first block, a header Yap reads for the
//!   meeting's name, date and who was there (Otter's "SPEAKERS", Teams'
//!   title and "October 5, 2026, 2:30PM" lines).
//! - **Zoom's saved transcript**: blocks headed "[Priya Shah] 14:02:31",
//!   clock times: the first one is when the meeting started.
//! - **Google Meet's transcript** saved as text: "00:05:00" on a line of
//!   its own now and then, "Priya Shah: text" lines between.
//! - **Granola** (assumed from its "Copy transcript" and Markdown export,
//!   no published format): an optional "# Title", "Date:" and "Attendees:"
//!   lines, the notes in Markdown, then the transcript — after a
//!   "Transcript" heading when there is one — as "Me: …" / "Them: …" (or
//!   "**Name:** …") lines, sometimes with "[00:01:02]" times. Its notes
//!   become My thoughts; "Me" is you, "Them" the others.
//! - Anything else with "Name: text" lines (a name the file uses at least
//!   twice, or the way most of its lines start), with or without
//!   "[00:01:02]" times. A line with no name carries on the line above.
//!
//! Lines with no time get one from their words (150 a minute), and the
//! preview says the length is a guess. Pure.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use super::dates;
use super::parse::{clean_name, name_like, parse_time, spoken_ms, unnamed_label, Format, Line, Parsed};

const TIME: &str = r"\d{1,2}:\d{2}(?::\d{2})?(?:[.,]\d{1,3})?";

/// "Priya Shah  0:01", "Priya Shah 1:02:03", "Priya Shah (0:01)".
static NAME_TIME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^(?P<name>[^\t:\[\]()]{{1,60}}?)\s+[(\[]?(?P<time>{TIME})[)\]]?$")).unwrap());
/// "[Priya Shah] 14:02:31" (Zoom), "[Priya Shah]".
static BRACKET_TIME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^\[(?P<name>[^\]]{{1,60}})\]\s*(?P<time>{TIME})?$")).unwrap());
/// "00:05:00", "[00:05:00]", "(0:05)".
static TIME_ONLY: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"^[(\[]?(?P<time>{TIME})[)\]]?$")).unwrap());
/// "[00:01:02] Priya Shah: text", "00:01:02 - Priya: text", "Priya Shah (00:01): text", "Priya: text".
static INLINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^(?:[(\[]?(?P<t1>{TIME})[)\]]?\s*[-–]?\s*)?(?P<name>[^:\[\]()]{{1,60}}?)\s*(?:[(\[](?P<t2>{TIME})[)\]])?\s*:\s+(?P<text>\S.*)$"
    ))
    .unwrap()
});
/// "[00:01:02] text", "00:01:02 - text": a time and words, no name.
static TIME_TEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^(?:[(\[](?P<t1>{TIME})[)\]]|(?P<t2>\d{{1,2}}:\d{{2}}:\d{{2}}(?:[.,]\d{{1,3}})?))\s*[-–]?\s*(?P<text>\S.*)$")).unwrap()
});
/// "[Priya Shah] text" on one line.
static BRACKET_INLINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^(?:[(\[]?(?P<t1>{TIME})[)\]]?\s*)?\[(?P<name>[^\]]{{1,60}})\]\s+(?P<text>\S.*)$")).unwrap());
/// Markdown bold around a speaker: "**Me:** text", "**Priya Shah**: text".
static BOLD_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\*\*([^*]{1,60}?)(:?)\*\*(:?)\s*").unwrap());
/// "Attendees: Priya Shah, Tom Baker".
static PEOPLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:attendees|participants|speakers|people|meeting with)\s*[:\-]\s*(.+)$").unwrap());
/// A heading saying the transcript starts here.
static TRANSCRIPT_HEADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(?:#{1,6}\s*)?\**transcript\**:?$").unwrap());
/// "32m 15s", "1h 2m", "45:12", "Duration: 32 minutes".
static DURATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:duration\s*:?\s*)?(?:\d+\s*h(?:ours?|rs?)?\s*)?(?:\d+\s*m(?:in(?:ute)?s?)?\s*)?(?:\d+\s*s(?:ec(?:ond)?s?)?)?$|^\d{1,2}:\d{2}(?::\d{2})?$").unwrap()
});

/// What a line is.
enum Kind {
    Blank,
    /// A block's head: who (if said) and when (if said).
    Head(Option<String>, Option<u64>),
    /// One line said: who, when (if said), the words, and the words as
    /// written with the name (for when it isn't one).
    Said(String, Option<u64>, String, String),
    /// A time and words, no name.
    Timed(Option<u64>, String),
    Text,
}

fn time(m: Option<regex::Match<'_>>) -> Option<u64> {
    m.and_then(|m| parse_time(m.as_str()))
}

/// A line without its Markdown list or quote mark and with a bold speaker
/// made plain ("- **Me:** hi" → "Me: hi").
fn unmark(line: &str, markdown: bool) -> String {
    let mut s = line.trim();
    if markdown {
        s = s.trim_start_matches('>').trim_start();
        if let Some(r) = s.strip_prefix("- ").or_else(|| s.strip_prefix("* ")) {
            s = r.trim_start();
        }
    }
    match BOLD_NAME.captures(s) {
        Some(c) if !c[2].is_empty() || !c[3].is_empty() => format!("{}: {}", &c[1], &s[c.get(0).map_or(0, |m| m.end())..]),
        _ => s.to_string(),
    }
}

fn kind(line: &str) -> Kind {
    if line.is_empty() {
        return Kind::Blank;
    }
    if let Some(c) = TIME_ONLY.captures(line) {
        return Kind::Head(None, time(c.name("time")));
    }
    for re in [&*BRACKET_TIME, &*NAME_TIME] {
        if let Some(c) = re.captures(line) {
            let name = clean_name(&c["name"]);
            if name_like(&name) {
                return Kind::Head(Some(name), time(c.name("time")));
            }
        }
    }
    for (re, bracket) in [(&*INLINE, false), (&*BRACKET_INLINE, true)] {
        if let Some(c) = re.captures(line) {
            let name = clean_name(&c["name"]);
            if name_like(&name) {
                let at = time(c.name("t1")).or_else(|| time(c.name("t2")));
                let text = c["text"].trim().to_string();
                let raw = c["name"].trim();
                let written = if bracket { format!("[{raw}] {text}") } else { format!("{raw}: {text}") };
                return Kind::Said(name, at, text, written);
            }
        }
    }
    if let Some(c) = TIME_TEXT.captures(line) {
        let at = time(c.name("t1")).or_else(|| time(c.name("t2")));
        return Kind::Timed(at, c["text"].trim().to_string());
    }
    Kind::Text
}

/// A block's or a line's speaker, before the names are settled.
struct Turn {
    speaker: Option<String>,
    /// Said as "Name: text" on its line (needs backing), not a block's head.
    inline: bool,
    /// How the line was written, for when its name isn't one.
    written: String,
    at: Option<u64>,
    text: String,
}

fn is_me_or_them(name: &str) -> bool {
    matches!(name.to_lowercase().as_str(), "me" | "them" | "you")
}

/// A line that's a date and little else ("October 5, 2026, 2:30PM", "Mon,
/// Oct 5, 2026 2:30PM • 32:15"), not a title or a sentence with a date in
/// it.
fn mostly_date(s: &str) -> bool {
    if dates::in_line(s).is_none() {
        return false;
    }
    const DATE_WORDS: [&str; 10] = ["am", "pm", "at", "of", "gmt", "utc", "st", "nd", "rd", "th"];
    const DAYS: [&str; 7] = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
    const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let other = s
        .split(|c: char| !c.is_alphabetic())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .filter(|w| {
            let three: String = w.chars().take(3).collect();
            !DATE_WORDS.contains(&w.as_str()) && !(w.len() >= 3 && (DAYS.contains(&three.as_str()) || MONTHS.contains(&three.as_str())))
        })
        .count();
    other == 0
}

/// The header above the transcript: the meeting's name, date and people
/// (and, for Granola, its notes).
fn header(p: &mut Parsed, lines: &[String], granola: bool) {
    let mut notes: Vec<&str> = Vec::new();
    let mut people_next = false;
    let mut skip_next = false;
    for raw in lines {
        let line = raw.trim();
        if line.is_empty() {
            if granola && !notes.is_empty() {
                notes.push("");
            }
            continue;
        }
        if skip_next {
            skip_next = false;
            continue;
        }
        if people_next {
            people_next = false;
            add_people(p, line);
            continue;
        }
        let bare = line.trim_start_matches('#').trim().trim_matches('*').trim();
        let lower = bare.to_lowercase();
        if matches!(lower.as_str(), "speakers" | "attendees" | "participants") {
            people_next = true;
            continue;
        }
        if matches!(lower.as_str(), "summary keywords" | "keywords") {
            skip_next = true;
            continue;
        }
        if let Some(c) = PEOPLE.captures(bare) {
            add_people(p, &c[1]);
            continue;
        }
        if let Some(rest) = lower.strip_prefix("date").filter(|r| r.starts_with([':', ' '])) {
            if let Some(d) = dates::in_line(rest) {
                p.dated.get_or_insert(d);
                continue;
            }
        }
        if bare.is_empty() || DURATION.is_match(bare) {
            continue;
        }
        if mostly_date(bare) {
            if let Some(d) = dates::in_line(bare) {
                p.dated.get_or_insert(d);
            }
            continue;
        }
        if p.title.is_empty() && (line.starts_with("# ") || !granola) {
            let title = clean_title(bare);
            if !title.is_empty() {
                p.title = title;
                if let Some(d) = dates::in_line(bare) {
                    p.dated.get_or_insert(d);
                }
                continue;
            }
        }
        if granola {
            notes.push(raw.trim_end());
        }
    }
    if granola {
        p.notes = notes.join("\n").trim().to_string();
    }
}

fn add_people(p: &mut Parsed, list: &str) {
    for name in list.split([',', ';', '•', '·']).map(clean_name) {
        for part in name.split(" and ") {
            let part = part.trim();
            if name_like(part) && !is_me_or_them(part) && unnamed_label(part).is_none() && !p.attendees.iter().any(|a| a.eq_ignore_ascii_case(part)) {
                p.attendees.push(part.to_string());
            }
        }
    }
}

/// A meeting's name as tools write it, made plain: Teams'
/// "-20261005_143012-Meeting Recording", Meet's "(2026-10-05 14:30
/// GMT+01:00) - Transcript" and a "Transcript" word off.
pub fn clean_title(raw: &str) -> String {
    static STAMP: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\s*[-_]?\s*(?:\d{8}[_T-]\d{4,6}\s*[-_]?\s*)?(?:meeting\s+)?(?:recording|transcript(?:ion)?)\s*$").unwrap()
    });
    static DATE_PARENS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*\((?:19|20)\d{2}-\d{2}-\d{2}[^)]*\)").unwrap());
    let mut t = raw.trim().trim_start_matches('#').trim().trim_matches('*').trim().to_string();
    for _ in 0..2 {
        t = STAMP.replace(&t, "").to_string();
        t = DATE_PARENS.replace(&t, "").to_string();
        t = t.trim().trim_end_matches(['-', '–', '_', '|', ':']).trim().to_string();
    }
    t.chars().take(120).collect()
}

/// Read text `lines` (`ext`: the file's extension; "docx" for a Word
/// document's paragraphs).
pub fn parse(lines: &[String], ext: &str) -> Parsed {
    let markdown = matches!(ext, "md" | "markdown");
    let cleaned: Vec<String> = lines.iter().map(|l| unmark(l, markdown)).collect();
    // Where the transcript starts: after a "Transcript" heading, else at the
    // first line that says who spoke or when.
    let heading = cleaned.iter().position(|l| TRANSCRIPT_HEADING.is_match(l.trim()));
    let kinds: Vec<Kind> = cleaned.iter().map(|l| kind(l.trim())).collect();
    let first = match heading {
        Some(h) => h + 1,
        None => kinds.iter().position(|k| matches!(k, Kind::Head(..) | Kind::Said(..) | Kind::Timed(..))).unwrap_or(cleaned.len()),
    };
    let mut turns: Vec<Turn> = Vec::new();
    let mut current: Option<Turn> = None;
    fn flush(current: &mut Option<Turn>, turns: &mut Vec<Turn>) {
        if let Some(t) = current.take().filter(|t| !t.text.trim().is_empty()) {
            turns.push(t);
        }
    }
    // A block's speaker and time carry to the lines under its head.
    let mut block: (Option<String>, Option<u64>) = (None, None);
    let (mut heads, mut zoom_heads, mut time_marks) = (0usize, 0usize, 0usize);
    for (i, k) in kinds.into_iter().enumerate().skip(first) {
        let line = cleaned[i].trim();
        match k {
            Kind::Blank => flush(&mut current, &mut turns),
            Kind::Head(name, at) => {
                flush(&mut current, &mut turns);
                match &name {
                    Some(_) if line.starts_with('[') => zoom_heads += 1,
                    Some(_) => heads += 1,
                    None => time_marks += 1,
                }
                match name {
                    Some(n) => block = (Some(n), at),
                    None => block.1 = at,
                }
            }
            Kind::Said(name, at, text, written) => {
                flush(&mut current, &mut turns);
                current = Some(Turn { speaker: Some(name), inline: true, written, at: at.or(block.1), text });
                // A time mark dates the line after it, not every line.
                block.1 = None;
            }
            Kind::Timed(at, text) => {
                flush(&mut current, &mut turns);
                current = Some(Turn { speaker: block.0.clone(), inline: false, written: text.clone(), at, text });
                block.1 = None;
            }
            Kind::Text => match current.as_mut() {
                // A line with no name carries on the one above.
                Some(t) => {
                    t.text = format!("{} {line}", t.text);
                    t.written = format!("{} {line}", t.written);
                }
                None => {
                    current = Some(Turn { speaker: block.0.clone(), inline: false, written: line.to_string(), at: block.1, text: line.to_string() });
                    // A block's next paragraph comes after this one.
                    block.1 = None;
                }
            },
        }
    }
    flush(&mut current, &mut turns);

    // Names said inline need backing: used twice, Me / Them, or the file's
    // way of naming (most of its lines do).
    let mut uses: HashMap<String, usize> = HashMap::new();
    for t in turns.iter().filter(|t| t.inline) {
        if let Some(n) = &t.speaker {
            *uses.entry(n.to_lowercase()).or_default() += 1;
        }
    }
    let sure = |n: &str| is_me_or_them(n) || uses.get(&n.to_lowercase()).copied().unwrap_or(0) >= 2;
    let inline_count = turns.iter().filter(|t| t.inline).count();
    let backed = turns.iter().filter(|t| t.inline && t.speaker.as_deref().is_some_and(sure)).count();
    let layout = turns.len() <= 1 || backed * 2 >= turns.len();
    let takes = |t: &Turn| t.speaker.as_deref().is_some_and(|n| !t.inline || layout || sure(n));
    let granola = markdown || turns.iter().any(|t| takes(t) && t.speaker.as_deref().is_some_and(is_me_or_them));

    let mut p = Parsed::default();
    header(&mut p, &lines[..heading.unwrap_or(first).min(lines.len())], granola);
    if !turns.iter().any(takes) && !turns.iter().any(|t| t.at.is_some()) {
        // Prose, not a transcript: nobody speaks and nothing is timed.
        return p;
    }
    let mut out: Vec<Line> = turns
        .into_iter()
        .map(|t| {
            let named = takes(&t);
            let (speaker, text) = match t.speaker {
                Some(n) if named => (n, t.text),
                Some(_) => (String::new(), t.written),
                None => (String::new(), t.text),
            };
            Line { speaker, start_ms: t.at.unwrap_or(u64::MAX), end_ms: None, text: text.trim().to_string() }
        })
        .collect();
    place(&mut out, &mut p);
    p.format = if ext == "docx" {
        if heads > 0 { Format::TeamsDocx } else { Format::Docx }
    } else if granola {
        Format::Granola
    } else if zoom_heads > heads {
        Format::ZoomText
    } else if heads > 0 {
        Format::Otter
    } else if time_marks > 0 && inline_count > 0 {
        Format::MeetText
    } else {
        Format::Text
    };
    p.lines = out;
    p
}

/// Times for every line: a line without one starts where the one before
/// it would have stopped (from its words); clock times (Zoom's, or a
/// first time past half an hour) count from the first, which says when
/// the meeting began, and go on past midnight; each line ends where the
/// next starts (at most 5 minutes on), or where its words would.
fn place(lines: &mut [Line], p: &mut Parsed) {
    let first = lines.iter().map(|l| l.start_ms).find(|&s| s != u64::MAX);
    p.estimated = first.is_none();
    let first = first.unwrap_or(0);
    let clock = first >= 30 * 60 * 1000;
    if clock {
        p.clock = chrono::NaiveTime::from_num_seconds_from_midnight_opt((first / 1000 % 86_400) as u32, 0);
    }
    let (mut day, mut last_raw, mut at) = (0u64, first, 0u64);
    for line in lines.iter_mut() {
        if line.start_ms == u64::MAX {
            line.start_ms = at;
        } else if clock {
            if line.start_ms + 12 * 3_600_000 < last_raw {
                day += 86_400_000;
            }
            last_raw = line.start_ms;
            line.start_ms = (line.start_ms + day).saturating_sub(first);
        }
        at = line.start_ms + spoken_ms(&line.text);
    }
    for i in 0..lines.len() {
        let start = lines[i].start_ms;
        let next = lines.get(i + 1).map(|n| n.start_ms).filter(|&n| n > start);
        lines[i].end_ms = Some(next.map_or(start + spoken_ms(&lines[i].text), |n| n.min(start + 5 * 60_000)));
    }
}

#[cfg(test)]
mod tests {
    use super::super::parse::{clock, parse as read, text_lines};
    use super::*;

    fn said(p: &Parsed) -> Vec<(String, String, String)> {
        p.lines.iter().map(|l| (l.speaker.clone(), clock(l.start_ms), l.text.clone())).collect()
    }
    fn s(a: &str, b: &str, c: &str) -> (String, String, String) {
        (a.into(), b.into(), c.into())
    }

    const OTTER: &str = "Supplier call\nMon, Oct 5, 2026 2:30PM • 1:05:12\n\nSUMMARY KEYWORDS\nvenue, budget\n\nSPEAKERS\nPriya Shah, Tom Baker, Speaker 1\n\nPriya Shah  0:01\nGood morning, everyone.\nLet's start with the venue.\n\nTom Baker  0:12\nThe venue is booked.\n\nSpeaker 1  1:02:03\nAn hour in, and we're done.\n";

    #[test]
    fn otter_blocks_with_its_header_and_times_past_an_hour() {
        let p = read("Supplier call.txt", OTTER.as_bytes()).unwrap();
        assert_eq!(p.format, Format::Otter);
        assert_eq!(p.title, "Supplier call");
        let d = p.dated.unwrap();
        assert_eq!((d.date.to_string(), d.time.map(|t| t.to_string())), ("2026-10-05".into(), Some("14:30:00".into())));
        assert_eq!(p.attendees, ["Priya Shah", "Tom Baker"]);
        assert_eq!(
            said(&p),
            [
                s("Priya Shah", "0:00:01", "Good morning, everyone. Let's start with the venue."),
                s("Tom Baker", "0:00:12", "The venue is booked."),
                s("Speaker 1", "1:02:03", "An hour in, and we're done."),
            ]
        );
        assert!(!p.estimated);
        assert_eq!(p.lines[0].end_ms, Some(12_000));
    }

    #[test]
    fn zoom_text_clock_times_and_a_midnight() {
        let txt = "[Priya Shah] 23:58:30\nLate one tonight.\n\n[Tom Baker] 23:59:10\nIt is.\n\n[Priya Shah] 00:01:00\nPast midnight now.\n";
        let p = read("meeting_saved_closed_caption.txt", txt.as_bytes()).unwrap();
        assert_eq!(p.format, Format::ZoomText);
        assert_eq!(p.clock.map(|t| t.to_string()), Some("23:58:30".into()));
        assert_eq!(
            said(&p),
            [s("Priya Shah", "0:00:00", "Late one tonight."), s("Tom Baker", "0:00:40", "It is."), s("Priya Shah", "0:02:30", "Past midnight now.")]
        );
    }

    #[test]
    fn google_meet_time_marks_and_named_lines() {
        let txt = "Supplier call (2026-10-05 14:30 GMT+01:00) - Transcript\n\nAttendees\nPriya Shah, Tom Baker\n\nTranscript\n\n00:00:00\n\nPriya Shah: Good morning.\nTom Baker: Morning.\n00:05:00\n\nPriya Shah: Five minutes in.\n";
        let p = read("Supplier call - Transcript.txt", txt.as_bytes()).unwrap();
        assert_eq!(p.format, Format::MeetText);
        assert_eq!(p.title, "Supplier call");
        assert_eq!(p.dated.unwrap().offset_min, Some(60));
        assert_eq!(p.attendees, ["Priya Shah", "Tom Baker"]);
        let lines = said(&p);
        assert_eq!(lines[0], s("Priya Shah", "0:00:00", "Good morning."));
        assert_eq!(lines[1].0, "Tom Baker");
        assert_eq!(lines[2], s("Priya Shah", "0:05:00", "Five minutes in."));
    }

    const GRANOLA: &str = "# Design review\nDate: 5 Oct 2026 at 14:30\nAttendees: Priya Shah, Tom Baker\n\n## Notes\n- Venue: Lisbon\n- Budget is tight\n\n## Transcript\n\n**Me:** Shall we start?\n**Them:** Yes, go ahead.\n**Me:** The venue is Lisbon.\n";

    #[test]
    fn granola_notes_and_me_them() {
        let p = read("Design review.md", GRANOLA.as_bytes()).unwrap();
        assert_eq!(p.format, Format::Granola);
        assert_eq!(p.title, "Design review");
        assert_eq!(p.dated.unwrap().time.map(|t| t.to_string()), Some("14:30:00".into()));
        assert_eq!(p.attendees, ["Priya Shah", "Tom Baker"]);
        assert_eq!(p.notes, "## Notes\n- Venue: Lisbon\n- Budget is tight");
        let who: Vec<_> = p.lines.iter().map(|l| l.speaker.as_str()).collect();
        assert_eq!(who, ["Me", "Them", "Me"]);
        // No times in the file: guessed from the words, in order.
        assert!(p.estimated);
        assert_eq!(p.lines[0].start_ms, 0);
        assert!(p.lines[1].start_ms > 0 && p.lines[2].start_ms > p.lines[1].start_ms);
    }

    #[test]
    fn otter_with_a_bom_and_crlf_and_granola_in_windows_1252() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(OTTER.replace('\n', "\r\n").as_bytes());
        let p = read("Supplier call.txt", &bytes).unwrap();
        assert_eq!(p.format, Format::Otter);
        assert_eq!(p.title, "Supplier call");
        assert_eq!(p.lines.len(), 3);
        assert_eq!(p.lines[1].text, "The venue is booked.");
        // "Café" and curly quotes as Windows' own code page writes them.
        let p = read("Café.md", b"# Caf\xE9 plans\r\n\r\n## Transcript\r\n**Me:** The caf\xE9 opens at nine.\r\n**Them:** \x93Nine\x94 works.\r\n").unwrap();
        assert_eq!(p.format, Format::Granola);
        assert_eq!(p.title, "Café plans");
        assert_eq!(p.lines[0].text, "The café opens at nine.");
        assert_eq!(p.lines[1].text, "“Nine” works.");
    }

    #[test]
    fn granola_copied_transcript_without_a_heading() {
        let p = read("granola.txt", b"Me: hi there\nThem: hello\nMe: let's begin\n").unwrap();
        assert_eq!(p.format, Format::Granola);
        assert_eq!(p.lines.len(), 3);
        assert!(p.notes.is_empty());
    }

    #[test]
    fn a_line_without_a_name_carries_on_and_prose_is_not_a_transcript() {
        let p = read("call.txt", b"Priya Shah: Hello.\nNote: bring the deck.\nPriya Shah: Bye.\nTom: Bye now.\n").unwrap();
        let who: Vec<_> = p.lines.iter().map(|l| (l.speaker.as_str(), l.text.as_str())).collect();
        // "Note" is no one; Tom, once, is someone in a file that names everyone.
        assert_eq!(who, [("Priya Shah", "Hello. Note: bring the deck."), ("Priya Shah", "Bye."), ("Tom", "Bye now.")]);
        // One "Reminder:" among unnamed lines is text.
        let p = read("memo.txt", b"[00:01] We start.\n[00:05] Reminder: bring the deck.\n[00:09] Done.\n").unwrap();
        assert!(p.lines.iter().all(|l| l.speaker.is_empty()), "{:?}", p.lines);
        assert_eq!(p.lines[1].text, "Reminder: bring the deck.");
        let e = read("essay.txt", b"This is an essay.\nIt has no speakers at all.\n").unwrap_err();
        assert!(e.contains("no lines"), "{e}");
    }

    #[test]
    fn timestamps_only_no_names() {
        let p = read("otter-no-names.txt", b"0:01\nGood morning.\n\n0:09\nMorning.\n").unwrap();
        assert_eq!(said(&p), [s("", "0:00:01", "Good morning."), s("", "0:00:09", "Morning.")]);
    }

    #[test]
    fn titles_made_plain() {
        assert_eq!(clean_title("Weekly sync-20261005_143012-Meeting Recording"), "Weekly sync");
        assert_eq!(clean_title("Supplier call (2026-10-05 14:30 GMT+01:00) - Transcript"), "Supplier call");
        assert_eq!(clean_title("# **Design review**"), "Design review");
        assert_eq!(clean_title("Recording"), "");
        assert!(mostly_date("Mon, Oct 5, 2026 2:30PM • 32:15"));
        assert!(mostly_date("Monday 5th October 2026"));
        assert!(!mostly_date("Supplier call 5 October 2026"));
    }

    #[test]
    fn crlf_paragraphs_from_a_document() {
        let lines = text_lines("Weekly sync\r\nOctober 5, 2026, 2:30PM\r\n32m 15s\r\nPriya Shah started transcription\r\nPriya Shah   0:03\r\nGood morning.\r\nTom Baker   0:07\r\nMorning.\r\n");
        let mut p = parse(&lines, "docx");
        super::super::parse::tidy(&mut p.lines, true);
        assert_eq!(p.format, Format::TeamsDocx);
        assert_eq!(p.title, "Weekly sync");
        assert!(p.dated.is_some());
        assert_eq!(said(&p), [s("Priya Shah", "0:00:03", "Good morning."), s("Tom Baker", "0:00:07", "Morning.")]);
    }
}

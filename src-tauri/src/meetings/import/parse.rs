//! What every transcript file becomes ([`Parsed`]: its lines, who said each
//! as the file names them, and when), which reader reads a file
//! ([`parse`]: by its contents first, its extension second), and what they
//! share: times ("01:02:03.450", "1:02", "0:0:3.120"), names ("Shah, Priya",
//! "PRIYA SHAH", "Sam Lee (Guest)") and the tidy every reader's lines get
//! (in order, a rolling caption's repeat dropped, a sentence cut across
//! cues put back together). Pure.

use serde::Serialize;

use super::dates::Dated;

/// The kinds of file Yap reads, as the preview names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Format {
    /// WebVTT with `<v Speaker>` voice tags (Microsoft Teams).
    TeamsVtt,
    /// WebVTT with "Name: text" cues (Zoom, Google Meet).
    NamedVtt,
    /// WebVTT captions, no names.
    Vtt,
    Srt,
    /// Teams' Word download.
    TeamsDocx,
    /// Another Word document read as a transcript.
    Docx,
    /// "Priya Shah  0:01" blocks (Otter's .txt).
    Otter,
    /// "[Priya Shah] 14:02:31" blocks (Zoom's saved transcript).
    ZoomText,
    /// A Google Meet transcript document saved as text ("00:05:00" marks,
    /// "Name: text" lines).
    MeetText,
    /// Granola's Markdown or text: its notes, then "Me:" / "Them:" lines.
    Granola,
    /// "Name: text" lines from anywhere else.
    #[default]
    Text,
}

impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Format::TeamsVtt => "Teams transcript (WebVTT)",
            Format::NamedVtt => "Zoom or Google Meet transcript (WebVTT)",
            Format::Vtt => "WebVTT captions",
            Format::Srt => "SubRip captions (.srt)",
            Format::TeamsDocx => "Teams transcript (Word)",
            Format::Docx => "Word transcript",
            Format::Otter => "Otter transcript",
            Format::ZoomText => "Zoom transcript (text)",
            Format::MeetText => "Google Meet transcript",
            Format::Granola => "Granola notes",
            Format::Text => "Text transcript",
        }
    }
}

/// One thing said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Who said it, as the file names them ("" = it doesn't say).
    pub speaker: String,
    /// When, in ms from the start of the meeting.
    pub start_ms: u64,
    pub end_ms: Option<u64>,
    pub text: String,
}

/// A transcript file, read.
#[derive(Debug, Clone, Default)]
pub struct Parsed {
    pub format: Format,
    /// The meeting's name, when the file has one ("" = none: the AI names it).
    pub title: String,
    /// When the meeting was, if the file's contents say.
    pub dated: Option<Dated>,
    /// The time of day of the first line, when the file's times are clock
    /// times (Zoom's "[Priya Shah] 14:02:31").
    pub clock: Option<chrono::NaiveTime>,
    /// Notes the file kept beside its transcript (Granola's): My thoughts.
    pub notes: String,
    /// Who was there, when the file lists them.
    pub attendees: Vec<String>,
    pub lines: Vec<Line>,
    /// The file has no times: each line's place is guessed from its words.
    pub estimated: bool,
    /// When the document itself was made (a .docx's own date, unix
    /// seconds): a last resort for the meeting's date.
    pub made: Option<i64>,
}

/// The longest file read (a ten-hour Teams transcript is ~3 MB).
pub const MAX_BYTES: u64 = 25 * 1024 * 1024;
/// The most lines one meeting takes.
pub const MAX_LINES: usize = 40_000;

/// The extensions Import a transcript… offers.
pub const EXTENSIONS: [&str; 6] = ["vtt", "srt", "txt", "docx", "md", "markdown"];

/// Read file `name`'s `bytes` as a transcript. The error says, in plain
/// words, why it isn't one.
pub fn parse(name: &str, bytes: &[u8]) -> Result<Parsed, String> {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    let mut parsed = if bytes.starts_with(b"PK\x03\x04") || ext == "docx" {
        super::docx::parse(bytes)?
    } else {
        if !EXTENSIONS.contains(&ext.as_str()) && !ext.is_empty() {
            return Err(format!(
                "Yap can't read .{ext} files as transcripts. Save it as .vtt, .srt, .txt, .docx or .md and try again."
            ));
        }
        if looks_binary(bytes) {
            return Err("This file isn't text Yap can read.".to_string());
        }
        let text = crate::dictionary::files::decode(bytes);
        let lines = text_lines(&text);
        let first = lines.iter().find(|l| !l.trim().is_empty()).map(|l| l.trim_start_matches('\u{feff}').trim());
        let is_vtt = first.is_some_and(|l| l.starts_with("WEBVTT")) || ext == "vtt";
        if is_vtt || ext == "srt" || super::cues::has_timings(&lines) {
            super::cues::parse(&lines, is_vtt)
        } else {
            super::text::parse(&lines, &ext)
        }
    };
    tidy(&mut parsed.lines, !parsed.estimated);
    if parsed.lines.is_empty() {
        return Err(match parsed.format {
            Format::TeamsDocx | Format::Docx => "Yap found no transcript in this document: no one speaks in it.".to_string(),
            _ => "Yap found no transcript in this file: no lines with who spoke or when.".to_string(),
        });
    }
    if parsed.lines.len() > MAX_LINES {
        return Err(format!("This transcript is too long for one meeting (over {MAX_LINES} lines)."));
    }
    Ok(parsed)
}

/// Text with NUL bytes in its first KB is something else (an image, a PDF).
fn looks_binary(bytes: &[u8]) -> bool {
    // UTF-16 has NULs and a BOM: that's text.
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return false;
    }
    bytes.iter().take(1024).any(|&b| b == 0)
}

/// `text` as lines: CRLF, CR and the Unicode line separators all end one.
pub fn text_lines(text: &str) -> Vec<String> {
    text.trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace(['\r', '\u{2028}', '\u{2029}', '\u{85}'], "\n")
        .split('\n')
        .map(str::to_string)
        .collect()
}

// ---- times ----

/// A time as transcripts write it, in ms: "01:02:03.450", "1:02:03",
/// "02:03.4", "0:0:3.120" (old Teams), "1:02" (Otter), SubRip's
/// "00:00:01,000". `None` for anything else.
pub fn parse_time(s: &str) -> Option<u64> {
    let s = s.trim();
    let (whole, frac) = match s.rfind(['.', ',']) {
        Some(i) if s[..i].contains(':') => (&s[..i], &s[i + 1..]),
        _ => (s, ""),
    };
    let parts: Vec<&str> = whole.split(':').collect();
    if !(2..=3).contains(&parts.len()) || parts.iter().any(|p| p.is_empty() || p.len() > 3 || !p.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    if !frac.bytes().all(|b| b.is_ascii_digit()) || frac.len() > 9 {
        return None;
    }
    let n: Vec<u64> = parts.iter().map(|p| p.parse().unwrap_or(0)).collect();
    let (h, m, sec) = match n.as_slice() {
        [m, sec] => (0, *m, *sec),
        [h, m, sec] if *m < 60 => (*h, *m, *sec),
        _ => return None,
    };
    if sec >= 60 {
        return None;
    }
    let ms: u64 = format!("{frac:0<3}")[..3].parse().unwrap_or(0);
    Some(((h * 60 + m) * 60 + sec) * 1000 + ms)
}

/// "1:02:03" for the logs and tests.
#[cfg(test)]
pub fn clock(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// About how long `text` takes to say (150 words a minute), at least a
/// second: a line's end when the file gives none.
pub fn spoken_ms(text: &str) -> u64 {
    (text.split_whitespace().count() as u64 * 400).max(1000)
}

// ---- names ----

/// Little words a name may have in lower case ("Ludwig van Beethoven").
const PARTICLES: [&str; 14] = ["de", "da", "di", "del", "della", "der", "van", "von", "la", "le", "bin", "binti", "al", "du"];
/// Words that start a "Word: text" line without being anyone.
const NOT_NAMES: [&str; 34] = [
    "note", "notes", "summary", "agenda", "date", "time", "when", "where", "attendees", "participants", "subject", "re",
    "fw", "fwd", "location", "duration", "title", "transcript", "keywords", "summary keywords", "speakers", "action items",
    "actions", "decisions", "next steps", "http", "https", "todo", "to do", "warning", "important", "update", "example",
    "meeting",
];

/// Whether `s` reads as someone's name (or a speaker label): one to six
/// words, each starting with a capital (or a script without capitals, or a
/// particle, or a number after the first: "Speaker 2"), no sentence
/// punctuation, not a time, not a heading word ("Note", "Agenda").
pub fn name_like(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() || s.chars().count() > 60 || parse_time(s).is_some() {
        return false;
    }
    if s.chars().any(|c| matches!(c, '?' | '!' | ';' | '"' | '“' | '”' | '…' | '=' | '<' | '>' | '{' | '}' | '/' | '\\' | '*' | '#' | '|')) {
        return false;
    }
    let lower = s.to_lowercase();
    if NOT_NAMES.contains(&lower.as_str()) {
        return false;
    }
    let words: Vec<&str> = s.split_whitespace().collect();
    if words.len() > 6 || (s.ends_with([',', '.', ':']) && !s.ends_with("Jr.") && !s.ends_with("Sr.")) {
        return false;
    }
    words.iter().enumerate().all(|(i, w)| {
        let first = w.trim_start_matches(['(', '[', '\'', '‘']).chars().next();
        match first {
            None => false,
            Some(c) if c.is_ascii_digit() => i > 0,
            Some(c) if c.is_alphabetic() => !c.is_lowercase() || (i > 0 && PARTICLES.contains(&w.to_lowercase().as_str())),
            Some(_) => false,
        }
    })
}

/// A label for someone the file doesn't name: "Speaker 2" (Otter's,
/// Zoom's) → `Some(Some(2))`, "Unknown speaker" → `Some(None)`.
pub fn unnamed_label(name: &str) -> Option<Option<u32>> {
    let lower = name.trim().to_lowercase();
    let words: Vec<&str> = lower.split_whitespace().collect();
    let (label, n) = match words.as_slice() {
        [.., last] if last.trim_start_matches('#').bytes().all(|b| b.is_ascii_digit()) && !last.trim_start_matches('#').is_empty() => {
            (words[..words.len() - 1].join(" "), last.trim_start_matches('#').parse::<u32>().ok())
        }
        _ => (words.join(" "), None),
    };
    let generic = matches!(
        label.as_str(),
        "speaker" | "spk" | "participant" | "guest" | "caller" | "voice" | "unknown" | "unknown speaker" | "unidentified speaker"
            | "unidentified" | "someone"
    );
    match (generic, n) {
        (true, Some(n)) if n > 0 && n < 1000 => Some(Some(n)),
        (true, None) => Some(None),
        _ => None,
    }
}

/// A speaker's name as the file writes it, made plain: markdown bold and
/// brackets off, "(Guest)" / "(External)" off, "Shah, Priya" turned round,
/// an all-capitals name in ordinary case, spaces collapsed.
pub fn clean_name(raw: &str) -> String {
    let mut s = raw.trim().trim_matches(['*', '_', '[', ']']).trim().trim_end_matches(':').trim().to_string();
    for tag in ["(guest)", "(external)", "(unverified)", "(visitor)"] {
        if s.to_lowercase().ends_with(tag) {
            s.truncate(s.len() - tag.len());
            s = s.trim().to_string();
        }
    }
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    // "Shah, Priya" (a directory's "Last, First") → "Priya Shah".
    let s = match s.split_once(", ") {
        Some((last, first))
            if !first.contains(',')
                && [last, first].iter().all(|p| p.split_whitespace().count() == 1 && name_like(p))
                && !matches!(first, "Jr." | "Sr." | "Jr" | "Sr" | "II" | "III" | "PhD") =>
        {
            format!("{first} {last}")
        }
        _ => s,
    };
    let letters: Vec<char> = s.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.len() > 2 && letters.iter().all(|c| c.is_uppercase()) {
        return s
            .split(' ')
            .map(|w| {
                let mut cs = w.chars();
                match cs.next() {
                    Some(f) => f.to_uppercase().chain(cs.flat_map(char::to_lowercase)).collect::<String>(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
    }
    s
}

// ---- text ----

/// A caption's text as plain words: tags (`<i>`, `<c.colour>`, karaoke
/// timestamps), SubRip's `{\an8}` and HTML entities out, spaces collapsed.
pub fn plain_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '<' => {
                // A tag runs to its '>'; a lone '<' ("x < y") stays.
                let rest: String = chars.clone().take(200).collect();
                match rest.find('>') {
                    Some(end) if !rest[..end].contains('<') => {
                        for _ in 0..rest[..=end].chars().count() {
                            chars.next();
                        }
                    }
                    _ => out.push(c),
                }
            }
            '{' if chars.peek() == Some(&'\\') => {
                for n in chars.by_ref() {
                    if n == '}' {
                        break;
                    }
                }
            }
            _ => out.push(c),
        }
    }
    let decoded = entities(&out);
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// HTML / XML entities: the named few transcripts use, and numeric ones.
pub fn entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        let end = tail.find(';').filter(|&e| e <= 10);
        let decoded = end.and_then(|e| {
            let name = &tail[1..e];
            let c = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                "lrm" | "rlm" | "zwj" | "zwnj" => Some('\u{200b}'),
                _ => {
                    let n = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X"));
                    match n {
                        Some(hex) => u32::from_str_radix(hex, 16).ok().and_then(char::from_u32),
                        None => name.strip_prefix('#').and_then(|d| d.parse().ok()).and_then(char::from_u32),
                    }
                }
            };
            c.map(|c| (c, e))
        });
        match decoded {
            Some((c, e)) => {
                if c != '\u{200b}' {
                    out.push(c);
                }
                rest = &tail[e + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Lines a transcript tool adds that nobody said.
pub fn is_system_line(text: &str) -> bool {
    let t = text.trim().to_lowercase();
    t.ends_with("started transcription") || t.ends_with("stopped transcription") || t == "[music]" || t == "[applause]" || t == "(music)"
}

// ---- the tidy every reader's lines get ----

/// A line this long (ms) or longer isn't joined to the next.
const JOIN_MAX_MS: u64 = 30_000;
/// The gap a sentence cut across two cues may have.
const JOIN_GAP_MS: u64 = 1_500;

/// Put a reader's lines in order and tidy them: empty and system lines
/// out, a rolling caption's repeated first line dropped (YouTube-style
/// captions say each line twice, the second time at the top of the next
/// cue), the same words twice in a row once, and a sentence the file cut
/// across cues of one speaker (Teams cuts every few seconds) joined again
/// (`timed`: only when the times say they follow on).
pub fn tidy(lines: &mut Vec<Line>, timed: bool) {
    lines.retain(|l| !l.text.trim().is_empty() && !is_system_line(&l.text));
    lines.sort_by_key(|l| l.start_ms);
    let mut out: Vec<Line> = Vec::with_capacity(lines.len());
    for mut line in lines.drain(..) {
        if let Some(prev) = out.last_mut() {
            if prev.speaker == line.speaker {
                // The same words again (a caption shown twice).
                if prev.text == line.text && line.start_ms <= prev.end_ms.unwrap_or(prev.start_ms) + JOIN_GAP_MS {
                    prev.end_ms = max_end(prev.end_ms, line.end_ms);
                    continue;
                }
                // A rolling caption: this cue starts with the end of the last.
                if let Some(rest) = repeats_tail(&prev.text, &line.text) {
                    line.text = rest;
                    if line.text.is_empty() {
                        prev.end_ms = max_end(prev.end_ms, line.end_ms);
                        continue;
                    }
                }
                let follows = !timed || line.start_ms <= prev.end_ms.unwrap_or(prev.start_ms) + JOIN_GAP_MS;
                let short = line.end_ms.unwrap_or(line.start_ms).saturating_sub(prev.start_ms) <= JOIN_MAX_MS;
                if timed && follows && short && !ends_sentence(&prev.text) {
                    prev.text = format!("{} {}", prev.text, line.text);
                    prev.end_ms = max_end(prev.end_ms, line.end_ms);
                    continue;
                }
            }
        }
        out.push(line);
    }
    *lines = out;
}

fn max_end(a: Option<u64>, b: Option<u64>) -> Option<u64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

fn ends_sentence(text: &str) -> bool {
    text.trim_end_matches(['"', '\'', ')', '”', '’']).ends_with(['.', '?', '!', '…', '。', '？', '！'])
}

/// `next` begins with the last few words of `prev` (at least three, the
/// whole of `prev`'s last sentence-ish run): what follows them.
fn repeats_tail(prev: &str, next: &str) -> Option<String> {
    let pw: Vec<&str> = prev.split_whitespace().collect();
    let nw: Vec<&str> = next.split_whitespace().collect();
    let most = pw.len().min(nw.len());
    (3..=most).rev().find(|&k| pw[pw.len() - k..] == nw[..k]).map(|k| nw[k..].join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(speaker: &str, start: u64, end: u64, text: &str) -> Line {
        Line { speaker: speaker.into(), start_ms: start, end_ms: Some(end), text: text.into() }
    }

    #[test]
    fn times_as_transcripts_write_them() {
        assert_eq!(parse_time("00:00:03.120"), Some(3_120));
        assert_eq!(parse_time("01:02:03.450"), Some(3_723_450));
        assert_eq!(parse_time("1:02:03"), Some(3_723_000));
        assert_eq!(parse_time("02:03.4"), Some(123_400));
        assert_eq!(parse_time("0:0:3.12"), Some(3_120));
        assert_eq!(parse_time("00:00:01,000"), Some(1_000));
        assert_eq!(parse_time("75:12"), Some(4_512_000)); // Otter's minutes past an hour
        assert_eq!(parse_time("12:61"), None);
        assert_eq!(parse_time("1:75:00"), None);
        assert_eq!(parse_time("14:30 GMT"), None);
        assert_eq!(parse_time("hello"), None);
        assert_eq!(parse_time("3.5"), None);
        assert_eq!(clock(3_723_450), "1:02:03");
    }

    #[test]
    fn names_as_files_write_them() {
        assert!(name_like("Priya Shah"));
        assert!(name_like("Speaker 2"));
        assert!(name_like("Ludwig van Beethoven"));
        assert!(name_like("Zoë O'Neill-Hart"));
        assert!(name_like("Me"));
        assert!(name_like("李明"));
        assert!(!name_like("We start at"));
        assert!(!name_like("Note"));
        assert!(!name_like("Action items"));
        assert!(!name_like("Is that right?"));
        assert!(!name_like("1:02"));
        assert!(!name_like("2 people"));
        assert!(!name_like("https"));
        assert_eq!(clean_name("Shah, Priya"), "Priya Shah");
        assert_eq!(clean_name("PRIYA SHAH"), "Priya Shah");
        assert_eq!(clean_name("Sam Lee (Guest)"), "Sam Lee");
        assert_eq!(clean_name("**Tom  Baker:**"), "Tom Baker");
        assert_eq!(clean_name("Baker, Tom, Jr."), "Baker, Tom, Jr.");
        assert_eq!(clean_name("Okafor, Jr."), "Okafor, Jr.");
        assert_eq!(unnamed_label("Speaker 2"), Some(Some(2)));
        assert_eq!(unnamed_label("Speaker #3"), Some(Some(3)));
        assert_eq!(unnamed_label("Unknown Speaker"), Some(None));
        assert_eq!(unnamed_label("Priya Shah"), None);
        assert_eq!(unnamed_label("Room 2"), None);
    }

    #[test]
    fn caption_text_made_plain() {
        assert_eq!(plain_text("<i>Hello</i> <c.yellow>there</c>"), "Hello there");
        assert_eq!(plain_text("{\\an8}Up top &amp; centre"), "Up top & centre");
        assert_eq!(plain_text("x < y and <00:00:01.200>karaoke"), "x < y and karaoke");
        assert_eq!(plain_text("caf&#233; &#x2014; done&lrm;"), "café — done");
        assert_eq!(entities("fish &chips"), "fish &chips");
    }

    #[test]
    fn rolling_captions_and_cut_sentences_are_tidied() {
        // Each cue repeats the last one's bottom line at its top.
        let mut lines = vec![
            line("", 0, 2_000, "so the plan for the offsite"),
            line("", 2_000, 4_000, "so the plan for the offsite is settled for Lisbon"),
            line("", 4_000, 6_000, "is settled for Lisbon and the dates are fixed."),
            line("", 6_000, 7_000, "is settled for Lisbon and the dates are fixed."),
            line("Tom", 6_500, 8_000, "Good."),
            line("Tom", 8_100, 9_000, "Good."),
        ];
        tidy(&mut lines, true);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(lines[0].text, "so the plan for the offsite is settled for Lisbon and the dates are fixed.");
        assert_eq!(lines[0].end_ms, Some(7_000));
        assert_eq!(lines[1].text, "Good.");
        assert_eq!(lines[1].end_ms, Some(9_000));
    }

    #[test]
    fn overlapping_speakers_stay_apart_in_order() {
        let mut lines = vec![
            line("Priya", 1_000, 6_000, "I think we should wait"),
            line("Tom", 3_000, 4_000, "No."),
            line("Priya", 5_000, 8_000, "until Friday."),
        ];
        tidy(&mut lines, true);
        let said: Vec<_> = lines.iter().map(|l| (l.speaker.as_str(), l.text.as_str())).collect();
        assert_eq!(said, [("Priya", "I think we should wait"), ("Tom", "No."), ("Priya", "until Friday.")]);
    }

    #[test]
    fn unknown_kinds_and_binary_files_are_refused() {
        let e = parse("deck.pdf", b"%PDF-1.7").unwrap_err();
        assert!(e.contains(".pdf"), "{e}");
        let e = parse("notes.txt", &[0x89, b'P', b'N', b'G', 0, 0, 0]).unwrap_err();
        assert!(e.contains("isn't text"), "{e}");
        let e = parse("empty.vtt", b"WEBVTT\n\n").unwrap_err();
        assert!(e.contains("no lines"), "{e}");
    }
}

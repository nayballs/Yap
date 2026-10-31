//! Cue files: **WebVTT** (Microsoft Teams' transcript with `<v Speaker>`
//! voice tags; Zoom's and Google Meet's with the "Name: text" convention;
//! plain captions) and **SubRip** (`.srt`), and the old Teams Word
//! transcript, whose paragraphs are cues too ("0:0:0.0 --> 0:0:4.60", the
//! speaker's name on a line of its own, then the words). Pure.
//!
//! A name is taken from a cue's start ("Priya Shah: …", ">> PRIYA: …") or
//! its first line only when the file uses it that way at least twice (or
//! it's Granola's "Me" / "Them"): "Note: the venue is booked" is text.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use super::parse::{clean_name, name_like, parse_time, plain_text, Format, Line, Parsed};

static VOICE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<v(?:\.[^\s>]*)?(?:\s+([^>]*))?>").unwrap());
static PREFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([^:]{1,60}?)\s*:\s+(\S.*)$").unwrap());
static BRACKET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\[([^\]]{1,60})\]\s*(\S.*)$").unwrap());

/// "00:00:01.000 --> 00:00:04.000 line:90%" → its start and end (ms).
pub fn timing(line: &str) -> Option<(u64, u64)> {
    let (a, b) = line.split_once("-->")?;
    let start = parse_time(a.trim())?;
    let end = parse_time(b.split_whitespace().next()?)?;
    Some((start, end.max(start)))
}

/// Whether `lines` hold cue timings: a cue file whatever its name.
pub fn has_timings(lines: &[String]) -> bool {
    lines.iter().take(4_000).any(|l| l.contains("-->") && timing(l).is_some())
}

struct Cue {
    start: u64,
    end: u64,
    payload: Vec<String>,
}

/// The cues in `lines`: each timing line and the text under it, to a blank
/// line or the next timing (a SubRip index left above it is dropped).
fn cues(lines: &[String]) -> Vec<Cue> {
    let mut out: Vec<Cue> = Vec::new();
    let mut open = false;
    for raw in lines {
        let line = raw.trim();
        if line.contains("-->") {
            if let Some((start, end)) = timing(line) {
                // An index that lost its blank line belongs to this cue.
                if let Some(last) = out.last_mut().filter(|_| open) {
                    if last.payload.last().is_some_and(|l| l.bytes().all(|b| b.is_ascii_digit())) {
                        last.payload.pop();
                    }
                }
                out.push(Cue { start, end, payload: Vec::new() });
                open = true;
                continue;
            }
        }
        if line.is_empty() {
            open = false;
        } else if open {
            if let Some(cue) = out.last_mut() {
                cue.payload.push(line.to_string());
            }
        }
    }
    out
}

/// One speaker's part of a cue, before the names are settled.
struct Piece {
    start: u64,
    end: u64,
    /// From a voice tag: certain.
    voice: Option<String>,
    /// "Name: …" at its start, or its own line: a name if the file uses
    /// them so. With how it was written, for when it isn't one.
    named: Option<(String, String)>,
    text: String,
}

fn is_me_or_them(name: &str) -> bool {
    matches!(name.to_lowercase().as_str(), "me" | "them" | "you")
}

/// A "Name: text" (or "[Name] text") start: the name (and how it was
/// written), and the rest.
fn split_prefix(text: &str) -> Option<((String, String), String)> {
    let (c, written) = match PREFIX.captures(text) {
        Some(c) => {
            let w = format!("{}:", c[1].trim());
            (c, w)
        }
        None => {
            let c = BRACKET.captures(text)?;
            let w = format!("[{}]", &c[1]);
            (c, w)
        }
    };
    let name = clean_name(&c[1]);
    name_like(&name).then(|| ((name, written), c[2].trim().to_string()))
}

/// A cue's payload as its speakers' pieces.
fn pieces(cue: &Cue, out: &mut Vec<Piece>) {
    let mut voice: Option<String> = None;
    let mut named: Option<(String, String)> = None;
    let mut text = String::new();
    let flush = |voice: &Option<String>, named: &mut Option<(String, String)>, text: &mut String, out: &mut Vec<Piece>| {
        let t = plain_text(text);
        if !t.is_empty() {
            out.push(Piece { start: cue.start, end: cue.end, voice: voice.clone(), named: named.take(), text: t });
        }
        text.clear();
    };
    let dialogue = cue.payload.len() > 1 && cue.payload.iter().all(|l| l.starts_with('-'));
    // The old Teams layout: the speaker's name on the cue's first line.
    let mut lines: &[String] = &cue.payload;
    if cue.payload.len() > 1 && !cue.payload[0].contains(':') && !VOICE.is_match(&cue.payload[0]) {
        let written = plain_text(&cue.payload[0]);
        let name = clean_name(&written);
        if name_like(&name) {
            named = Some((name, written));
            lines = &cue.payload[1..];
        }
    }
    for line in lines {
        // A new speaker: a voice tag, ">>", or a dialogue dash.
        let mut rest = line.as_str();
        let turn = if let Some(r) = rest.strip_prefix(">>") {
            rest = r.trim_start();
            true
        } else if dialogue {
            rest = rest.trim_start_matches('-').trim_start();
            true
        } else {
            false
        };
        if turn {
            flush(&voice, &mut named, &mut text, out);
            voice = None;
            if let Some((name, said)) = split_prefix(&plain_text(rest)) {
                named = Some(name);
                rest_into(&mut text, &said);
                continue;
            }
        }
        let mut at = 0;
        let tags: Vec<(usize, usize, Option<String>)> = VOICE
            .captures_iter(rest)
            .map(|c| {
                let m = c.get(0).unwrap();
                (m.start(), m.end(), c.get(1).map(|n| clean_name(&plain_text(n.as_str()))).filter(|n| !n.is_empty()))
            })
            .collect();
        for (s, e, name) in tags {
            rest_into(&mut text, &rest[at..s]);
            flush(&voice, &mut named, &mut text, out);
            voice = name;
            at = e;
        }
        let tail = &rest[at..];
        // "Name: text" at the start of a piece (Zoom, Meet).
        if voice.is_none() && named.is_none() && text.is_empty() {
            if let Some((name, said)) = split_prefix(&plain_text(tail)) {
                named = Some(name);
                rest_into(&mut text, &said);
                continue;
            }
        }
        rest_into(&mut text, tail);
    }
    flush(&voice, &mut named, &mut text, out);
}

fn rest_into(text: &mut String, more: &str) {
    if more.trim().is_empty() {
        return;
    }
    if !text.is_empty() {
        text.push(' ');
    }
    text.push_str(more.trim());
}

/// Read cue lines (`vtt`: a WebVTT file, else SubRip or cues found in a
/// document).
pub fn parse(lines: &[String], vtt: bool) -> Parsed {
    let mut all: Vec<Piece> = Vec::new();
    for cue in cues(lines) {
        pieces(&cue, &mut all);
    }
    // A name that turns up once at a line's start is probably text
    // ("Note: …") — unless the file names its speakers that way throughout.
    let mut uses: HashMap<String, usize> = HashMap::new();
    for p in &all {
        if let Some((n, _)) = &p.named {
            *uses.entry(n.to_lowercase()).or_default() += 1;
        }
    }
    let sure = |n: &str| is_me_or_them(n) || uses.get(&n.to_lowercase()).copied().unwrap_or(0) >= 2;
    let backed = all.iter().filter(|p| p.voice.is_none() && p.named.as_ref().is_some_and(|(n, _)| sure(n))).count();
    let layout = all.len() <= 2 || backed * 2 >= all.len();
    let voices = all.iter().any(|p| p.voice.is_some());
    let mut named_any = false;
    let lines: Vec<Line> = all
        .into_iter()
        .map(|p| {
            let (speaker, text) = match (p.voice, p.named) {
                (Some(v), _) => (v, p.text),
                (None, Some((n, _))) if layout || sure(&n) => {
                    named_any = true;
                    (n, p.text)
                }
                // Not a name after all: the words as they were.
                (None, Some((_, written))) => (String::new(), format!("{written} {}", p.text)),
                (None, None) => (String::new(), p.text),
            };
            Line { speaker, start_ms: p.start, end_ms: Some(p.end), text }
        })
        .collect();
    let format = match (vtt, voices, named_any) {
        (false, _, _) => Format::Srt,
        (true, true, _) => Format::TeamsVtt,
        (true, false, true) => Format::NamedVtt,
        (true, false, false) => Format::Vtt,
    };
    Parsed { format, lines, ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::super::parse::{parse as read, text_lines};
    use super::*;

    fn said(p: &Parsed) -> Vec<(String, u64, String)> {
        p.lines.iter().map(|l| (l.speaker.clone(), l.start_ms, l.text.clone())).collect()
    }

    const TEAMS: &str = "WEBVTT\r\n\r\n3f2a9c1e-77b1-4c0e-9a51-0d6e1b2f8c4a/12-0\r\n00:00:03.120 --> 00:00:05.890\r\n<v Priya Shah>Good morning, everyone. Shall we</v>\r\n\r\n3f2a9c1e-77b1-4c0e-9a51-0d6e1b2f8c4a/12-1\r\n00:00:05.890 --> 00:00:08.000\r\n<v Priya Shah>start with the venue?</v>\r\n\r\n3f2a9c1e-77b1-4c0e-9a51-0d6e1b2f8c4a/14-0\r\n00:00:07.500 --> 00:00:09.200\r\n<v Shah, Tom>Yes, &amp; the budget.</v>\r\n\r\n3f2a9c1e-77b1-4c0e-9a51-0d6e1b2f8c4a/15-0\r\n01:02:03.000 --> 01:02:05.500\r\n<v Priya Shah>Thanks, all.</v>\r\n";

    #[test]
    fn teams_vtt_voice_tags_crlf_and_times_past_an_hour() {
        let p = read("Weekly sync.vtt", TEAMS.as_bytes()).unwrap();
        assert_eq!(p.format, Format::TeamsVtt);
        assert_eq!(
            said(&p),
            [
                ("Priya Shah".into(), 3_120, "Good morning, everyone. Shall we start with the venue?".into()),
                ("Tom Shah".into(), 7_500, "Yes, & the budget.".into()),
                ("Priya Shah".into(), 3_723_000, "Thanks, all.".into()),
            ]
        );
        assert_eq!(p.lines[0].end_ms, Some(8_000));
    }

    #[test]
    fn zoom_vtt_names_by_convention_with_a_bom() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(
            b"WEBVTT\n\n1\n00:00:00.570 --> 00:00:03.180\nPriya Shah: Good morning.\n\n2\n00:00:03.600 --> 00:00:05.090\nTom Baker: Morning. Note: the room moved.\n\n3\n00:00:05.500 --> 00:00:07.000\nPriya Shah: Right,\nso the room is 4B.\n",
        );
        let p = read("GMT20261005-143012_Recording.transcript.vtt", &bytes).unwrap();
        assert_eq!(p.format, Format::NamedVtt);
        assert_eq!(
            said(&p),
            [
                ("Priya Shah".into(), 570, "Good morning.".into()),
                ("Tom Baker".into(), 3_600, "Morning. Note: the room moved.".into()),
                ("Priya Shah".into(), 5_500, "Right, so the room is 4B.".into()),
            ]
        );
    }

    #[test]
    fn a_name_said_once_is_text_and_captions_without_names_have_none() {
        let vtt = "WEBVTT\n\n00:01.000 --> 00:03.000\nNote: the venue is booked.\n\n00:03.000 --> 00:05.000\nAnd the rooms.\n\n00:06.000 --> 00:08.000\n- Who's coming?\n- Everyone.\n";
        let p = read("captions.vtt", vtt.as_bytes()).unwrap();
        assert_eq!(p.format, Format::Vtt);
        let lines = said(&p);
        assert_eq!(lines[0], ("".into(), 1_000, "Note: the venue is booked.".into()));
        assert_eq!(lines[1], ("".into(), 3_000, "And the rooms.".into()));
        // A dialogue's two dashes are two turns.
        assert_eq!(lines[2].2, "Who's coming?");
        assert_eq!(lines[3].2, "Everyone.");
    }

    #[test]
    fn srt_with_commas_tags_and_a_lost_blank_line() {
        let srt = "1\n00:00:01,000 --> 00:00:04,000\n<i>Priya Shah: Hello there.</i>\n2\n00:00:04,500 --> 00:00:06,000\n{\\an8}Tom Baker: Hi.\n\n3\n01:00:00,000 --> 01:00:02,250\nPriya Shah: An hour in.\n";
        let p = read("call.srt", srt.as_bytes()).unwrap();
        assert_eq!(p.format, Format::Srt);
        assert_eq!(
            said(&p),
            [
                ("Priya Shah".into(), 1_000, "Hello there.".into()),
                ("Tom Baker".into(), 4_500, "Hi.".into()),
                ("Priya Shah".into(), 3_600_000, "An hour in.".into()),
            ]
        );
    }

    #[test]
    fn windows_1252_captions() {
        // "Zoë: Café at 3." in Windows-1252, plus a second line for the name to count.
        let mut bytes = b"1\r\n00:00:01,000 --> 00:00:02,000\r\nZo\xEB: Caf\xE9 at 3.\r\n\r\n2\r\n00:00:02,500 --> 00:00:03,000\r\nZo\xEB: \x93Fine\x94.\r\n".to_vec();
        bytes.push(b'\n');
        let p = read("cafe.srt", &bytes).unwrap();
        assert_eq!(said(&p)[0], ("Zoë".into(), 1_000, "Café at 3.".into()));
        assert_eq!(p.lines[1].text, "“Fine”.");
    }

    #[test]
    fn old_teams_layout_names_on_their_own_line() {
        let lines = text_lines("0:0:0.0 --> 0:0:4.60\nShah, Priya\nGood morning everyone.\n0:0:4.60 --> 0:0:7.120\nBaker, Tom\nMorning.\n");
        let p = parse(&lines, false);
        assert_eq!(
            said(&p),
            [("Priya Shah".into(), 0, "Good morning everyone.".into()), ("Tom Baker".into(), 4_600, "Morning.".into())]
        );
    }

    #[test]
    fn utf16_with_overlapping_cues_kept_apart() {
        // Two people talking over each other: both lines, in order of start.
        let vtt = "WEBVTT\r\n\r\n00:00:01.000 --> 00:00:06.000\r\n<v Priya Shah>I think we should wait until</v>\r\n\r\n00:00:03.000 --> 00:00:04.000\r\n<v Tom Baker>No.</v>\r\n\r\n00:00:05.500 --> 00:00:08.000\r\n<v Priya Shah>Friday at least.</v>\r\n";
        let bytes: Vec<u8> = [0xFF, 0xFE].into_iter().chain(vtt.encode_utf16().flat_map(|u| u.to_le_bytes())).collect();
        let p = read("overlap.vtt", &bytes).unwrap();
        assert_eq!(
            said(&p),
            [
                ("Priya Shah".into(), 1_000, "I think we should wait until".into()),
                ("Tom Baker".into(), 3_000, "No.".into()),
                ("Priya Shah".into(), 5_500, "Friday at least.".into()),
            ]
        );
    }

    #[test]
    fn broadcast_style_speaker_changes() {
        let vtt = "WEBVTT\n\n00:00:01.000 --> 00:00:04.000\n>> PRIYA SHAH: Welcome back.\n>> TOM BAKER: Thanks.\n\n00:00:05.000 --> 00:00:06.000\n>> PRIYA SHAH: Let's go.\n\n00:00:07.000 --> 00:00:08.000\n>> TOM BAKER: Sure.\n";
        let p = read("show.vtt", vtt.as_bytes()).unwrap();
        let names: Vec<_> = p.lines.iter().map(|l| l.speaker.as_str()).collect();
        assert_eq!(names, ["Priya Shah", "Tom Baker", "Priya Shah", "Tom Baker"]);
    }
}

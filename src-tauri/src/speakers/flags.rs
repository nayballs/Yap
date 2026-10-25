//! **Voices that may be one, a voice that may be two** (beta): the speaker
//! engine's two mistakes, offered to the person, never fixed by themselves.
//!
//! - **Alike** ("Speaker 2 and Speaker 4 may be one voice · Merge"): two
//!   told-apart voices whose prints (`voiceprint.rs`, from their lines on
//!   disk) are [`voices::ALIKE`] alike — one person the engine split (a
//!   restart, a resume it couldn't match, a change of microphone).
//! - **Split** ("Speaker 3 may be two people · Split"): one voice whose
//!   lines fall into two clear groups ([`voices::split`]) — two people who
//!   sound alike, heard as one (the failure that hurts most: speakers.md).
//!   Split moves the smaller group (the one without lines the person
//!   assigned) to a new voice.
//!
//! Looked for after a recording's final labels (in the background, once
//! its audio is closed) and after Find speakers; kept in the speaker map
//! (`Speaker::alike` / `split`) for every window's card, until answered:
//! **Merge** / **They're different** (never offered for that pair again:
//! `apart`), **Split** / **It's one person** (`whole`), or **Not now**.
//! A voice's card offers ▶ (its clip, `clip.rs`). Needs voice matching
//! (the model, an `engines` build); without it, no flags.
//!
//! Logs: counts.

use tauri::AppHandle;

use super::voiceprint::{centroid, Printer, Span};
use super::voices::{self, Line, Split, Voice};
use super::{resolve, Alike, Note, SplitOffer, SEPARATED_PREFIX, THEM};
use crate::meetings::meeting_audio::{self, Meta};

/// A voice's lines looked at, at most (the longest): enough to tell, and
/// a long meeting's flags stay a few seconds of work.
const MAX_LINES: usize = 60;
/// Lines shorter than this make no print worth trusting.
const MIN_LINE_MS: u64 = 1_500;

/// Each voice's lines with their prints.
type VoiceLines = Vec<(String, Vec<Line<[u64; 2]>>)>;
/// Pairs that may be one voice: (a, b, how alike).
type Pairs = Vec<(String, String, f32)>;
/// Voices that may be two: (voice, its two groups).
type Splits = Vec<(String, Split<[u64; 2]>)>;
/// A voice's stretches, each with its key and whether the person assigned it.
type Stretches = Vec<(String, Vec<(Span, [u64; 2], bool)>)>;

/// A line's key in a split offer: its date and place.
fn key(seg: &crate::notes::TranscriptSegment) -> Option<[u64; 2]> {
    Some([seg.ts, seg.start_ms?])
}

/// What the voices of a note look like: per voice, its lines' prints.
fn prints(note: &Note, metas: &[Meta], printer: &Printer) -> VoiceLines {
    let mut out: Stretches = Vec::new();
    for seg in note.transcript.iter().filter(|s| s.source == THEM && s.is_talk()) {
        let id = resolve(&note.speakers, seg.speaker_id());
        if !id.starts_with(SEPARATED_PREFIX) {
            continue;
        }
        let (Some(k), Some(start), Some(end), Some(r)) = (key(seg), seg.start_ms, seg.end_ms, super::rerun::recording_of(metas, seg.ts)) else {
            continue;
        };
        if end.saturating_sub(start) < MIN_LINE_MS {
            continue;
        }
        let span = Span { recording: metas[r].recording, index: r, side: THEM.to_string(), start_ms: start, end_ms: end };
        match out.iter_mut().find(|(v, _)| v == id) {
            Some((_, lines)) => lines.push((span, k, seg.speaker_locked)),
            None => out.push((id.to_string(), vec![(span, k, seg.speaker_locked)])),
        }
    }
    out.into_iter()
        .map(|(id, mut spans)| {
            spans.sort_by_key(|(s, _, _)| std::cmp::Reverse(s.ms()));
            spans.truncate(MAX_LINES);
            let lines = spans
                .into_iter()
                .filter_map(|(span, key, assigned)| {
                    let print = printer.print(&span)?;
                    Some(Line { key, print, ms: span.ms().min(super::voiceprint::MAX_CLIP_MS), assigned })
                })
                .collect();
            (id, lines)
        })
        .collect()
}

/// The flags for a note's voices: pairs that may be one, voices that may
/// be two.
pub fn find(note: &Note, voices_lines: &[(String, Vec<Line<[u64; 2]>>)]) -> (Pairs, Splits) {
    let voices: Vec<Voice<String>> = voices_lines
        .iter()
        .filter_map(|(id, lines)| {
            let print = centroid(lines.iter().map(|l| (l.print.as_slice(), l.ms as f32)))?;
            Some(Voice { key: id.clone(), print, ms: lines.iter().map(|l| l.ms).sum() })
        })
        .collect();
    let refused = |a: &str, b: &str| {
        let (sa, sb) = (note.speakers.get(a), note.speakers.get(b));
        sa.is_some_and(|s| s.apart.iter().any(|x| x == b))
            || sb.is_some_and(|s| s.apart.iter().any(|x| x == a))
            // Both named by the person, differently: they said who's who.
            || (sa.zip(sb).is_some_and(|(x, y)| x.locked && y.locked && !x.name.trim().eq_ignore_ascii_case(y.name.trim())))
    };
    let splits: Splits = voices_lines
        .iter()
        .filter(|(id, _)| !note.speakers.get(id).is_some_and(|s| s.whole))
        .filter_map(|(id, lines)| voices::split(lines).map(|s| (id.clone(), s)))
        .collect();
    // A voice that may be two people sounds like either: its Split comes
    // first (the pairs are looked for again after it).
    let mixed = |v: &str| splits.iter().any(|(id, _)| id == v);
    let pairs: Pairs = voices::alike_pairs(&voices)
        .into_iter()
        .filter(|(a, b, _)| !refused(a, b) && !mixed(a) && !mixed(b))
        .collect();
    (pairs, splits)
}

/// Put the flags into `note`'s map (a "Not now" kept while the same flag
/// stands); stale ones go. Returns whether the map changed.
pub fn store(note: &mut Note, pairs: &[(String, String, f32)], splits: &[(String, Split<[u64; 2]>)]) -> bool {
    let mut changed = false;
    let ids: Vec<String> = note.speakers.keys().cloned().collect();
    for id in ids.iter().filter(|id| id.starts_with(SEPARATED_PREFIX)) {
        let pair = pairs.iter().find(|(a, _, _)| a == id);
        let split = splits.iter().find(|(v, _)| v == id).map(|(_, s)| s);
        let Some(s) = note.speakers.get_mut(id) else { continue };
        let alike = pair.map(|(_, b, score)| Alike {
            with: b.clone(),
            score: (*score * 100.0).round() / 100.0,
            later: s.alike.as_ref().is_some_and(|a| a.later && a.with == *b),
        });
        if s.alike != alike {
            s.alike = alike;
            changed = true;
        }
        let offer = split.map(|sp| SplitOffer {
            lines: sp.moved.clone(),
            kept: sp.kept.len(),
            score: (sp.alike * 100.0).round() / 100.0,
            later: s.split.as_ref().is_some_and(|o| o.later),
        });
        if s.split != offer {
            s.split = offer;
            changed = true;
        }
    }
    // A voice with no entry yet: its flags need one.
    for (a, b, score) in pairs {
        if !note.speakers.contains_key(a) {
            note.speakers.entry(a.clone()).or_default().alike = Some(Alike { with: b.clone(), score: (*score * 100.0).round() / 100.0, later: false });
            changed = true;
        }
    }
    for (id, sp) in splits {
        if !note.speakers.contains_key(id) {
            note.speakers.entry(id.clone()).or_default().split =
                Some(SplitOffer { lines: sp.moved.clone(), kept: sp.kept.len(), score: (sp.alike * 100.0).round() / 100.0, later: false });
            changed = true;
        }
    }
    changed
}

/// Look for the flags on note `note_id` now (blocking: prints are made from
/// its audio), store them, tell the windows.
pub fn refresh(app: &AppHandle, note_id: u64) {
    let Some(note) = crate::notes::get(note_id) else { return };
    if !note.transcript.iter().any(|s| s.speaker.starts_with(SEPARATED_PREFIX)) {
        return;
    }
    let Some(printer) = Printer::for_note(note_id) else { return };
    let started = std::time::Instant::now();
    let metas = meeting_audio::recordings(note_id);
    let lines = prints(&note, &metas, &printer);
    let (pairs, splits) = find(&note, &lines);
    let changed = crate::notes::modify(note_id, |n| {
        let c = store(n, &pairs, &splits);
        Ok((c, c))
    });
    tracing::info!(
        note_id,
        voices = lines.len(),
        alike = pairs.len(),
        split = splits.len(),
        ms = started.elapsed().as_millis() as u64,
        "Speakers: voices that may be one or two looked for"
    );
    if let Ok(true) = changed {
        super::naming::announce(app, note_id);
    }
}

/// The person's answer to a flag on voice `id`: `kind` "alike" | "split",
/// `answer` "yes" (Merge / Split) | "no" (They're different / It's one
/// person) | "later" (Not now).
pub fn answer(note: &mut Note, id: &str, kind: &str, answer: &str) -> Result<bool, String> {
    let id = resolve(&note.speakers, id).to_string();
    let entry = note.speakers.get(&id).cloned().unwrap_or_default();
    match (kind, answer) {
        ("alike", _) => {
            let Some(alike) = entry.alike else { return Ok(false) };
            let other = resolve(&note.speakers, &alike.with).to_string();
            match answer {
                "yes" => {
                    // The later number into the earlier: "Speaker 2" stays.
                    let (from, into) = if number(&id) > number(&other) { (id, other) } else { (other, id) };
                    super::merge(note, &from, &into)
                }
                "no" => {
                    for (a, b) in [(&id, &other), (&other, &id)] {
                        let s = note.speakers.entry(a.clone()).or_default();
                        if !s.apart.contains(b) {
                            s.apart.push(b.clone());
                        }
                        if s.alike.as_ref().is_some_and(|x| &x.with == b) {
                            s.alike = None;
                        }
                    }
                    Ok(true)
                }
                "later" => {
                    if let Some(a) = note.speakers.get_mut(&id).and_then(|s| s.alike.as_mut()) {
                        a.later = true;
                    }
                    Ok(true)
                }
                _ => Err(format!("No answer {answer}")),
            }
        }
        ("split", _) => {
            let Some(offer) = entry.split else { return Ok(false) };
            match answer {
                "yes" => {
                    let new = super::labels::VoiceIds::for_note(note).next_id();
                    let mut moved = 0;
                    for seg in note.transcript.iter_mut() {
                        let mine = !seg.speaker_locked && seg.is_talk() && key(seg).is_some_and(|k| offer.lines.contains(&k));
                        if mine && resolve(&note.speakers, seg.speaker_id()) == id {
                            seg.speaker = new.clone();
                            moved += 1;
                        }
                    }
                    if moved == 0 {
                        return Err("Those lines have changed since: look again with Find speakers.".to_string());
                    }
                    let s = note.speakers.entry(id.clone()).or_default();
                    s.split = None;
                    s.apart.push(new.clone());
                    // Names are suggested again for both from what's left.
                    s.suggestion = None;
                    note.speakers.entry(new).or_default().apart.push(id);
                    Ok(true)
                }
                "no" => {
                    let s = note.speakers.entry(id).or_default();
                    s.split = None;
                    s.whole = true;
                    Ok(true)
                }
                "later" => {
                    if let Some(o) = note.speakers.get_mut(&id).and_then(|s| s.split.as_mut()) {
                        o.later = true;
                    }
                    Ok(true)
                }
                _ => Err(format!("No answer {answer}")),
            }
        }
        _ => Err(format!("No flag {kind}")),
    }
}

/// "them-12" → 12 (others: none).
fn number(id: &str) -> u32 {
    id.strip_prefix(SEPARATED_PREFIX).and_then(|n| n.parse().ok()).unwrap_or(u32::MAX)
}

/// The answer to a flag's card (see [`answer`]).
#[tauri::command]
pub fn speaker_flag(app: AppHandle, note_id: u64, speaker: String, kind: String, answer: String, origin: Option<String>) -> Result<Note, String> {
    let note = super::edit(&app, note_id, origin, |n| self::answer(n, &speaker, &kind, &answer))?;
    if kind == "split" && answer == "yes" {
        // The new voice may be one heard elsewhere in the meeting: looked
        // for again, in the background.
        let _ = std::thread::Builder::new().name("speaker-flags".into()).spawn(move || refresh(&app, note_id));
    }
    Ok(note)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::TranscriptSegment;
    use crate::speakers::{display_line, rename, voiceprint::normalized};
    use serde_json::json;

    fn note(lines: Vec<TranscriptSegment>) -> Note {
        let mut n: Note = serde_json::from_value(json!({ "id": 1, "createdTs": 1, "updatedTs": 1 })).unwrap();
        n.transcript = lines;
        n
    }

    fn said(speaker: &str, start: u64) -> TranscriptSegment {
        TranscriptSegment {
            source: THEM.into(),
            text: "words words words words".into(),
            ts: 1_000 + start / 1000,
            start_ms: Some(start),
            end_ms: Some(start + 4_000),
            speaker: speaker.into(),
            ..Default::default()
        }
    }

    fn p(axis: usize, tilt: f32) -> Vec<f32> {
        let mut v = vec![0.0f32; 6];
        v[axis] = 1.0;
        v[5] = tilt;
        normalized(v).unwrap()
    }

    fn lines_of(prints: &[(u64, Vec<f32>)]) -> Vec<Line<[u64; 2]>> {
        prints.iter().map(|(start, print)| Line { key: [1_000 + start / 1000, *start], print: print.clone(), ms: 4_000, assigned: false }).collect()
    }

    #[test]
    fn alike_voices_are_offered_once_and_the_answers_stick() {
        let mut n = note(vec![said("them-1", 0), said("them-2", 5_000), said("them-3", 10_000)]);
        // them-3 sounds like them-1 (an engine restart's new number).
        let voices = vec![
            ("them-1".to_string(), lines_of(&[(0, p(0, 0.0)), (20_000, p(0, 0.1)), (40_000, p(0, 0.0))])),
            ("them-2".to_string(), lines_of(&[(5_000, p(1, 0.0)), (25_000, p(1, 0.1)), (45_000, p(1, 0.0))])),
            ("them-3".to_string(), lines_of(&[(10_000, p(0, 0.2)), (30_000, p(0, 0.15)), (50_000, p(0, 0.2))])),
        ];
        let (pairs, splits) = find(&n, &voices);
        assert_eq!(pairs.iter().map(|(a, b, _)| (a.as_str(), b.as_str())).collect::<Vec<_>>(), [("them-1", "them-3")]);
        assert!(splits.is_empty());
        assert!(store(&mut n, &pairs, &splits));
        assert_eq!(n.speakers["them-1"].alike.as_ref().unwrap().with, "them-3");
        assert!(!store(&mut n, &pairs, &splits), "the same flags again: nothing changes");

        // "Not now": kept while it stands.
        assert!(answer(&mut n, "them-1", "alike", "later").unwrap());
        store(&mut n, &pairs, &splits);
        assert!(n.speakers["them-1"].alike.as_ref().unwrap().later);

        // "They're different": never offered again for that pair.
        assert!(answer(&mut n, "them-1", "alike", "no").unwrap());
        assert!(n.speakers["them-1"].alike.is_none());
        let (pairs, _) = find(&n, &voices);
        assert!(pairs.is_empty());

        // Merge: the later number into the earlier, never by itself.
        let mut m = note(vec![said("them-1", 0), said("them-3", 10_000)]);
        rename(&mut m, "them-3", "Priya").unwrap();
        store(&mut m, &[("them-1".into(), "them-3".into(), 0.9)], &[]);
        assert_eq!(display_line(&m.speakers, &m.transcript[0]), "Speaker 1", "offered, not done");
        assert!(answer(&mut m, "them-1", "alike", "yes").unwrap());
        assert_eq!(m.speakers["them-3"].merged_into, "them-1");
        assert_eq!(display_line(&m.speakers, &m.transcript[0]), "Priya", "the person's name stays");
        assert_eq!(display_line(&m.speakers, &m.transcript[1]), "Priya");
    }

    #[test]
    fn two_people_as_one_voice_are_offered_a_split() {
        // Speaker 1's lines: two voices, five lines each.
        let mut lines = Vec::new();
        let mut prints = Vec::new();
        for i in 0..10u64 {
            let start = i * 5_000;
            lines.push(said("them-1", start));
            prints.push((start, if i % 2 == 0 { p(0, 0.05 * i as f32 / 10.0) } else { p(2, 0.05) }));
        }
        let mut n = note(lines);
        n.transcript[1].speaker_locked = true; // the person said who said one of the odd ones
        let voices = vec![("them-1".to_string(), lines_of(&prints))];
        let (pairs, splits) = find(&n, &voices);
        assert!(pairs.is_empty());
        assert_eq!(splits.len(), 1);
        store(&mut n, &pairs, &splits);
        let offer = n.speakers["them-1"].split.clone().unwrap();
        assert_eq!(offer.kept, 5);
        assert_eq!(offer.lines.len(), 5);
        // Without the lock the bigger (equal: the first) group keeps it; here the odd lines are moved.
        assert!(answer(&mut n, "them-1", "split", "yes").unwrap());
        let voices_now: Vec<String> = n.transcript.iter().map(|s| display_line(&n.speakers, s)).collect();
        let moved = voices_now.iter().filter(|v| *v == "Speaker 2").count();
        assert!(moved == 5 || moved == 4, "{voices_now:?}");
        assert!(n.transcript[1].speaker_locked, "the person's line stays");
        assert!(n.speakers["them-1"].split.is_none());
        assert!(n.speakers["them-1"].apart.contains(&"them-2".to_string()), "not offered as one again");

        // "It's one person": no split offered again.
        let mut w = note(vec![said("them-1", 0)]);
        store(&mut w, &[], &splits);
        assert!(answer(&mut w, "them-1", "split", "no").unwrap());
        assert!(w.speakers["them-1"].whole);
        assert!(find(&w, &voices).1.is_empty());
    }

    #[test]
    fn names_the_person_gave_two_voices_say_they_are_two() {
        let mut n = note(vec![said("them-1", 0), said("them-2", 5_000)]);
        rename(&mut n, "them-1", "Sam").unwrap();
        rename(&mut n, "them-2", "Priya").unwrap();
        let voices = vec![
            ("them-1".to_string(), lines_of(&[(0, p(0, 0.0)), (20_000, p(0, 0.0)), (40_000, p(0, 0.0))])),
            ("them-2".to_string(), lines_of(&[(5_000, p(0, 0.1)), (25_000, p(0, 0.1)), (45_000, p(0, 0.1))])),
        ];
        assert!(find(&n, &voices).0.is_empty());
    }
}

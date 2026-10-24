//! The voice separation's labels on a meeting's transcript (beta, step 3 of
//! yap-private `docs/diarization-spike.md`): `diarize.rs` reports who spoke
//! when on a recording's call side; this puts it on the recording's "Them"
//! lines, splitting a line where the speaker changes ([`split_line`], at its
//! words' times, else in proportion to its text).
//!
//! **When.** Each timeline the engine reports (`diarize::on_segments`, as
//! its labels settle, in steps of up to ~20 s of audio; a line's name comes
//! a median ~15 s after its last word) labels
//! the lines whose audio it has settled (`end_ms ≤ labelled_ms`); new lines
//! the latest labels already cover get theirs as they arrive
//! ([`after_ingest`]); the final timeline (`done`, sent before the recording
//! says it stopped) labels the rest, so the title, the last digests and the
//! action plan read the voices. Rolling digests written before keep "Them".
//!
//! **Which lines.** This recording's (from where the transcript stood when it
//! started: [`begin`]) "Them" lines with a place on its clock, no voice yet,
//! and not the user's (`speaker_locked`). Never "You", echo, "You dictated
//! here" markers, or lines from before line times existed. A line with no
//! voice heard near it stays "Them".
//!
//! **Voice ids.** The engine's voices (`(stream, speaker)`; an engine started
//! again after a crash counts from 1 again) become "them-N", numbered on
//! from the note's highest in the order they first get a line, so a resumed
//! meeting (a new recording, new engine numbers) carries on at "Speaker 4"
//! and is never merged with the earlier voices by itself. The engine only
//! says which lines are one voice: names stay the speaker map's
//! (user > call app > voice > calendar / AI guess), and "Them"'s name is
//! handed on by [`settle_them`].
//!
//! **No labels** (the switch off, no model, the engine failing): the lines
//! stay You/Them. A recording whose engine stopped early says so in the
//! notepad ([`EVENT`]).

use std::sync::{Mutex, MutexGuard};

use serde_json::json;
use tauri::{AppHandle, Emitter};

use super::{resolve, split_line, suggest, Speaker, Turn, NEAR_MS, SEPARATED_PREFIX, THEM, YOU};
use crate::speakers::diarize::Timeline;
use crate::notes::{Note, TranscriptSegment};

/// A recording's speaker labels stopped early: `{ noteId, message }` (the
/// notepad shows it under the transcript).
pub const EVENT: &str = "yap-meeting-speakers";
/// `yap-note-changed`'s `origin` for a labelling pass (no window's label).
const ORIGIN: &str = "speakers";

// ---- voice ids ----

/// "them-12" → 12.
fn separated_number(id: &str) -> Option<u32> {
    id.strip_prefix(SEPARATED_PREFIX)?.parse().ok()
}

/// One recording's engine voices `(stream, speaker)` → the note's voice ids.
#[derive(Debug, Default, Clone)]
pub struct VoiceIds {
    ids: Vec<((u32, u8), String)>,
    next: u32,
}

impl VoiceIds {
    /// For a recording of `note`: "them-N" numbered on from its highest.
    pub fn for_note(note: &Note) -> VoiceIds {
        let highest = note
            .speakers
            .iter()
            .flat_map(|(id, s)| [id.as_str(), s.merged_into.as_str()])
            .chain(note.transcript.iter().map(|s| s.speaker.as_str()))
            .filter_map(separated_number)
            .max()
            .unwrap_or(0);
        VoiceIds {
            ids: Vec::new(),
            next: highest + 1,
        }
    }

    /// Engine voice `v`'s id: the next number the first time it's asked for.
    fn id(&mut self, v: (u32, u8)) -> String {
        if let Some((_, id)) = self.ids.iter().find(|(k, _)| *k == v) {
            return id.clone();
        }
        let id = format!("{SEPARATED_PREFIX}{}", self.next);
        self.next += 1;
        self.ids.push((v, id.clone()));
        id
    }

    fn contains(&self, id: &str) -> bool {
        self.ids.iter().any(|(_, i)| i == id)
    }

    /// Engine voice `v`'s id, if it has one yet (lines of its own).
    pub fn get(&self, v: (u32, u8)) -> Option<&str> {
        self.ids.iter().find(|(k, _)| *k == v).map(|(_, id)| id.as_str())
    }
}

/// A turn's stand-in for engine voice `(stream, speaker)` until a line of it
/// is kept (then it gets an id: numbers go to voices with lines only).
fn key(stream: u32, speaker: u8) -> String {
    format!("#{stream}:{speaker}")
}

fn parse_key(k: &str) -> Option<(u32, u8)> {
    let (stream, speaker) = k.strip_prefix('#')?.split_once(':')?;
    Some((stream.parse().ok()?, speaker.parse().ok()?))
}

// ---- one pass over the transcript ----

/// Labels are settled up to here: everything once the engine finished well.
fn settled(tl: &Timeline) -> u64 {
    if tl.done && tl.error.is_none() {
        u64::MAX
    } else {
        tl.labelled_ms
    }
}

/// `tl`'s segments as turns, cut off where its labels stop being settled.
fn turns(tl: &Timeline, settled_ms: u64) -> Vec<Turn> {
    tl.segments
        .iter()
        .filter(|s| s.start_ms < settled_ms && s.end_ms > s.start_ms)
        .map(|s| Turn {
            start_ms: s.start_ms,
            end_ms: s.end_ms.min(settled_ms),
            speaker: key(s.stream, s.speaker),
        })
        .collect()
}

/// Whether line `seg` is the voice separation's to label, with labels
/// settled up to `settled_ms`: a "Them" line of talk, placed on the
/// recording's clock and settled, with no voice yet, not the user's.
pub fn eligible(seg: &TranscriptSegment, settled_ms: u64) -> bool {
    seg.source == THEM
        && seg.is_talk()
        && seg.speaker.is_empty()
        && !seg.speaker_locked
        && !seg.text.trim().is_empty()
        && seg.start_ms.is_some()
        && seg.end_ms.is_some_and(|end| end <= settled_ms)
}

/// What a pass changed.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Labelled {
    /// Lines given a voice (whole, or split into parts).
    pub lines: usize,
    /// Lines the splits added.
    pub added: usize,
    /// The stretch that changed, as it was: `transcript[at..at + removed]`
    /// is now `transcript[at..at + removed + added]`.
    pub span: Option<(usize, usize)>,
    /// The speaker map changed ([`settle_them`]).
    pub map: bool,
    /// This recording's voices so far.
    pub voices: usize,
}

impl Labelled {
    pub fn changed(&self) -> bool {
        self.lines > 0 || self.map
    }
}

/// Put timeline `tl`'s voices on `note`'s lines from `base` on (one
/// recording's lines; `voices` its ids), then settle "Them"'s name.
pub fn label(note: &mut Note, base: usize, tl: &Timeline, voices: &mut VoiceIds) -> Labelled {
    let settled_ms = settled(tl);
    let turns = turns(tl, settled_ms);
    let mut out = Labelled::default();
    let mut first = usize::MAX;
    let mut last_end = 0;
    let mut i = base;
    while i < note.transcript.len() {
        let seg = &note.transcript[i];
        if !eligible(seg, settled_ms) {
            i += 1;
            continue;
        }
        let (start, end) = (seg.start_ms.unwrap_or(0), seg.end_ms.unwrap_or(0));
        let near: Vec<Turn> = turns
            .iter()
            .filter(|t| t.end_ms + NEAR_MS >= start && t.start_ms <= end + NEAR_MS)
            .cloned()
            .collect();
        let mut parts = split_line(seg, &near);
        if parts.iter().any(|p| p.speaker.is_empty()) {
            // No voice heard near it: it stays "Them".
            i += 1;
            continue;
        }
        for p in &mut parts {
            if let Some(v) = parse_key(&p.speaker) {
                p.speaker = voices.id(v);
            }
        }
        let n = parts.len();
        let was = i - out.added; // its index before this pass
        first = first.min(was);
        last_end = was + 1;
        crate::notes::replace_line(note, i, parts);
        out.lines += 1;
        out.added += n - 1;
        i += n;
    }
    if first != usize::MAX {
        out.span = Some((first, last_end - first));
    }
    let mut found = recording_voices(note, base, voices);
    out.voices = found.len();
    // A voice that only repeats yours (your voice reaching the call side)
    // never takes the call's name.
    found.retain(|(v, _)| !super::naming::repeats_you(note, v));
    out.map = settle_them(note, &found, tl.done);
    out
}

/// This recording's voices, resolved (two the user merged are one), each
/// with the words it said, most first (a tie: the first heard first).
fn recording_voices(note: &Note, base: usize, voices: &VoiceIds) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = Vec::new();
    for seg in note.transcript.iter().skip(base) {
        if !seg.is_talk() || !voices.contains(&seg.speaker) {
            continue;
        }
        let id = resolve(&note.speakers, &seg.speaker);
        if id == YOU || id == THEM {
            continue;
        }
        let words = seg.text.split_whitespace().count();
        match out.iter_mut().find(|(v, _)| v == id) {
            Some((_, w)) => *w += words,
            None => out.push((id.to_string(), words)),
        }
    }
    out.sort_by_key(|(_, words)| std::cmp::Reverse(*words));
    out
}

// ---- "Them"'s name and the voices ----

/// Hand "Them"'s name on to the voices a recording told apart (`found`: its
/// voices, most words first). Before the call side was told apart "Them"
/// could already have a name: a 1:1 call's calendar guess (the only other
/// invitee, `calendar_guess`), or one the user gave it.
///
/// - **One voice**: it is "Them". It takes the name: a guess as a guess
///   ("Priya?"), the user's name as the user's.
/// - **Several**: a guess goes to one of them, never all: the voice that
///   said the most (while recording, the first to qualify keeps it; at the
///   end it moves to whoever said the most), never onto a voice with a name
///   of its own or one told "Not Priya". A name the user gave "Them" stays
///   on "Them" alone: the user named the call, not one of these voices.
///
/// "Them" keeps its name for the lines nobody could label. Returns whether
/// the map changed.
pub fn settle_them(note: &mut Note, found: &[(String, usize)], done: bool) -> bool {
    let Some(them) = note
        .speakers
        .get(THEM)
        .filter(|s| s.merged_into.is_empty() && s.named())
        .cloned()
    else {
        return false;
    };
    match found {
        [] => false,
        [(voice, _)] => match them.source {
            Some(source) if them.is_guess() => suggest(note, voice, &them.name, &them.email, source),
            _ => carry(note, voice, &them),
        },
        _ => {
            let Some(source) = them.source.filter(|_| them.is_guess()) else {
                return false;
            };
            let holds = |s: &Speaker| {
                s.is_guess() && s.source == Some(source) && s.name.trim().eq_ignore_ascii_case(them.name.trim())
            };
            let holder = found
                .iter()
                .map(|(v, _)| v)
                .find(|v| note.speakers.get(v.as_str()).is_some_and(holds))
                .cloned();
            let target = found
                .iter()
                .map(|(v, _)| v)
                .find(|v| {
                    note.speakers
                        .get(v.as_str())
                        .is_none_or(|s| !s.refuses(&them.name) && (!s.named() || holds(s)))
                })
                .cloned();
            match (holder, target) {
                (None, Some(target)) => suggest(note, &target, &them.name, &them.email, source),
                (Some(holder), Some(target)) if done && holder != target => {
                    if let Some(s) = note.speakers.get_mut(&holder) {
                        s.name.clear();
                        s.email.clear();
                        s.source = None;
                    }
                    suggest(note, &target, &them.name, &them.email, source);
                    true
                }
                _ => false,
            }
        }
    }
}

/// "Them"'s name (not a guess: the user's, say) onto the call's one voice,
/// unless that voice has a name as trusted or was told it isn't them.
fn carry(note: &mut Note, voice: &str, them: &Speaker) -> bool {
    let entry = note.speakers.entry(voice.to_string()).or_default();
    if entry.refuses(&them.name) || (entry.named() && entry.rank() >= them.rank()) {
        return false;
    }
    entry.name = them.name.clone();
    entry.email = them.email.clone();
    entry.source = them.source;
    entry.locked = them.locked;
    true
}

// ---- the recordings being labelled ----

/// A meeting recording whose lines get the voice separation's labels.
struct Recording {
    note_id: u64,
    /// Its diarization session (`Timeline::session`); `None`: no engine.
    session: Option<u64>,
    /// Its first line: the transcript's length when it started.
    base: usize,
    voices: VoiceIds,
}

static RECORDINGS: Mutex<Vec<Recording>> = Mutex::new(Vec::new());

fn recordings() -> MutexGuard<'static, Vec<Recording>> {
    RECORDINGS.lock().unwrap_or_else(|p| p.into_inner())
}

fn accepts(rec: &Recording, tl: &Timeline) -> bool {
    rec.note_id == tl.note_id && (rec.session == Some(tl.session) || (rec.session.is_none() && scripted(tl)))
}

/// A timeline the e2e suite scripted (`diarize::e2e_diarize_timeline`), for
/// a recording with no engine.
#[cfg(debug_assertions)]
fn scripted(tl: &Timeline) -> bool {
    crate::shell::e2e::active() && tl.session == crate::speakers::diarize::E2E_SESSION
}

#[cfg(not(debug_assertions))]
fn scripted(_: &Timeline) -> bool {
    false
}

/// Follow the speaker engine (app setup).
pub fn init(app: &AppHandle) {
    let app = app.clone();
    crate::speakers::diarize::on_segments(move |tl| apply(&app, tl, tl.done));
}

/// A recording into `note_id` started (`meeting::start`, before its first
/// line): its lines are the ones from here on, labelled from diarization
/// session `session`'s timelines (`None`: the switch is off, or no model).
pub fn begin(note_id: u64, session: Option<u64>) {
    let Some(note) = crate::notes::get(note_id) else {
        return;
    };
    let rec = Recording {
        note_id,
        session,
        base: note.transcript.len(),
        voices: VoiceIds::for_note(&note),
    };
    let base = rec.base;
    let mut all = recordings();
    all.retain(|r| r.note_id != note_id);
    all.push(rec);
    drop(all);
    // Who the call app shows talking (experimental, `call_ui.rs`).
    super::call_ui::begin(note_id, session, base);
}

/// Lines of the recording into `note_id` arrived (`meeting::ingest`, after
/// the windows got them): those the latest labels already cover get their
/// voices now, not at the next update.
pub fn after_ingest(app: &AppHandle, note_id: u64) {
    let Some(session) = recordings().iter().find(|r| r.note_id == note_id).map(|r| r.session) else {
        return;
    };
    if let Some(tl) = crate::speakers::diarize::timeline(note_id).filter(|t| !t.done) {
        apply(app, &tl, false);
    } else if session.is_none() && naming_due(note_id) {
        // No voices told apart: the call's names (a 1:1 invite's guess to
        // confirm, the call app's) for "Them".
        super::naming::refresh_note(app, note_id);
    }
}

/// At most one naming pass every [`NAMING_EVERY`] per note for a recording
/// with no speaker engine (every new line would ask otherwise).
fn naming_due(note_id: u64) -> bool {
    static LAST: Mutex<Vec<(u64, std::time::Instant)>> = Mutex::new(Vec::new());
    let mut last = LAST.lock().unwrap_or_else(|p| p.into_inner());
    let now = std::time::Instant::now();
    match last.iter_mut().find(|(id, _)| *id == note_id) {
        Some((_, at)) if now.duration_since(*at) < NAMING_EVERY => false,
        Some((_, at)) => {
            *at = now;
            true
        }
        None => {
            last.retain(|(_, at)| now.duration_since(*at) < std::time::Duration::from_secs(3600));
            last.push((note_id, now));
            true
        }
    }
}

const NAMING_EVERY: std::time::Duration = std::time::Duration::from_secs(10);

/// The recording into `note_id` ended (`meeting.rs`, after its engine
/// finished, before it says it stopped). Final labels came: nothing to do.
/// An engine that never sent them (it didn't finish in time): label as far
/// as its labels went, and say so.
pub fn end(app: &AppHandle, note_id: u64) {
    let session = match recordings().iter().find(|r| r.note_id == note_id) {
        Some(rec) => rec.session,
        None => return,
    };
    let latest = crate::speakers::diarize::timeline(note_id)
        .filter(|tl| !tl.done && (session == Some(tl.session) || (session.is_none() && scripted(tl))));
    match latest {
        Some(mut tl) => {
            tl.done = true;
            tl.error.get_or_insert_with(|| "the speaker engine didn't finish".to_string());
            apply(app, &tl, true);
        }
        None => {
            recordings().retain(|r| r.note_id != note_id);
            // The final labels came (and named the voices), or there were
            // none: name "Them" from the whole meeting.
            if session.is_none() {
                super::naming::refresh_note(app, note_id);
            }
            super::call_ui::finish(note_id);
        }
    }
}

/// One labelling pass for the recording `tl` belongs to (`last`: the
/// recording's final one): the store changed under its lock, the windows
/// told, and at the end, the log (and the notepad, if labels stopped early).
fn apply(app: &AppHandle, tl: &Timeline, last: bool) {
    let mut all = recordings();
    let Some(pos) = all.iter().position(|r| accepts(r, tl)) else {
        return;
    };
    let rec = &mut all[pos];
    let base = rec.base;
    let voices = &mut rec.voices;
    // Who each voice is (`naming.rs`): the settings read before the notes'
    // lock; the call app's readings once this pass gave its voices their
    // ids. (Lock order: this module's, the notes store's, then
    // `call_ui`'s.)
    let sources = super::naming::Sources::now();
    let words = crate::memory::Words::new();
    let ordinary = |w: &str| words.is(w);
    let result = crate::notes::modify(tl.note_id, |note| {
        let mut out = label(note, base, tl, voices);
        if sources.enabled && (out.changed() || last || super::call_ui::watching(tl.note_id)) {
            let platform = super::call_ui::votes_for(tl, voices);
            out.map |= super::naming::refresh(note, &sources.context(&ordinary, platform));
        }
        let changed = out.changed();
        Ok(((out, changed.then(|| note.clone())), changed))
    });
    if last {
        all.remove(pos);
    }
    drop(all);
    let out = match result {
        Ok((out, Some(note))) => {
            tracing::debug!(
                note_id = tl.note_id,
                lines = out.lines,
                added = out.added,
                voices = out.voices,
                done = tl.done,
                "Speakers: labelled lines"
            );
            emit(app, &note, &out);
            out
        }
        Ok((out, None)) => out,
        Err(e) => {
            tracing::warn!(note_id = tl.note_id, "Speakers: labels not saved: {e}");
            return;
        }
    };
    if last {
        finished(app, tl, out.voices);
        super::call_ui::finish(tl.note_id);
    }
}

/// Every window: the map, each line's voice, and the stretch that changed
/// (`lineSplice`: lines were split, so indices after it moved; the stop
/// marks with them).
fn emit(app: &AppHandle, note: &Note, out: &Labelled) {
    let mut payload = crate::commands::note_changed_payload(note, ORIGIN);
    payload["lineSpeakers"] = json!(super::line_speakers(note));
    if let Some((at, removed)) = out.span {
        let lines = &note.transcript[at..(at + removed + out.added).min(note.transcript.len())];
        payload["lineSplice"] = json!({ "at": at, "remove": removed, "lines": lines });
        payload["breaks"] = json!(note.breaks);
    }
    let _ = app.emit("yap-note-changed", payload);
}

/// The end of a recording's labels: the log, and the notepad told if they
/// stopped early (the lines they didn't reach say "Them").
fn finished(app: &AppHandle, tl: &Timeline, voices: usize) {
    match &tl.error {
        None => tracing::info!(note_id = tl.note_id, voices, "Speakers: the recording's call side told apart"),
        Some(_) => {
            tracing::info!(
                note_id = tl.note_id,
                voices,
                labelled_s = tl.labelled_ms / 1000,
                audio_s = tl.audio_ms / 1000,
                "Speakers: labels stopped early; the lines they didn't reach stay \"Them\""
            );
            let message = if voices == 0 {
                "Couldn't tell speakers apart this time."
            } else {
                "Couldn't tell speakers apart for the whole meeting: later lines say Them."
            };
            let _ = app.emit(EVENT, json!({ "noteId": tl.note_id, "message": message }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{calendar_guess, display_line, not, rename, rename_lines, speakers_line, NameSource};
    use super::*;
    use crate::speakers::diarize::Segment;

    fn note(lines: Vec<TranscriptSegment>) -> Note {
        let mut n: Note = serde_json::from_value(json!({ "id": 1, "createdTs": 1, "updatedTs": 1 })).unwrap();
        n.transcript = lines;
        n
    }

    /// A placed line: `words` one per 500 ms from `start` (400 ms each).
    fn line(source: &str, text: &str, start: u64) -> TranscriptSegment {
        let n = text.split_whitespace().count() as u32;
        let words: Vec<[u32; 2]> = (0..n).map(|k| [k * 500, k * 500 + 400]).collect();
        TranscriptSegment {
            source: source.into(),
            text: text.into(),
            ts: 1_000 + start / 1000,
            start_ms: Some(start),
            end_ms: Some(start + (n as u64 - 1) * 500 + 400),
            words,
            ..Default::default()
        }
    }

    fn seg(start_ms: u64, end_ms: u64, speaker: u8) -> Segment {
        Segment { start_ms, end_ms, speaker, stream: 0 }
    }

    fn tl(segments: Vec<Segment>, labelled_ms: u64, done: bool) -> Timeline {
        Timeline {
            note_id: 1,
            session: 1,
            audio_ms: segments.iter().map(|s| s.end_ms).max().unwrap_or(0).max(labelled_ms),
            segments,
            labelled_ms,
            done,
            ..Default::default()
        }
    }

    fn shown(n: &Note) -> Vec<String> {
        n.transcript.iter().map(|s| format!("{}: {}", display_line(&n.speakers, s), s.text)).collect()
    }

    #[test]
    fn voice_ids_carry_on_from_the_notes_highest() {
        let mut fresh = VoiceIds::for_note(&note(vec![]));
        assert_eq!(fresh.id((0, 1)), "them-1");
        assert_eq!(fresh.id((0, 3)), "them-2", "numbered as they get lines, not by the engine's number");
        assert_eq!(fresh.id((1, 1)), "them-3", "a restarted engine's Speaker 1 is a new voice");
        assert_eq!(fresh.id((0, 1)), "them-1");
        assert!(fresh.contains("them-3") && !fresh.contains("them-4"));
        // A resumed meeting: an earlier recording's voices (on lines, or only
        // in the map after a merge) are never reused.
        let mut earlier = note(vec![TranscriptSegment { speaker: "them-2".into(), ..line("them", "hi", 0) }]);
        earlier.speakers.insert("them-1".into(), Speaker { merged_into: "them-4".into(), ..Default::default() });
        assert_eq!(VoiceIds::for_note(&earlier).id((0, 1)), "them-5");
        assert_eq!(parse_key(&key(2, 7)), Some((2, 7)));
        assert_eq!(parse_key("them-1"), None);
    }

    #[test]
    fn only_this_recordings_settled_call_lines_get_voices() {
        let settled = 10_000;
        let them = line("them", "one two", 0);
        assert!(eligible(&them, settled));
        // Not settled yet, the mic, echo, a marker, the user's, labelled, unplaced, empty.
        assert!(!eligible(&line("them", "one two", 9_800), settled));
        assert!(!eligible(&line("you", "one two", 0), settled));
        assert!(!eligible(&TranscriptSegment { echo: true, ..them.clone() }, settled));
        assert!(!eligible(&TranscriptSegment::dictation_marker(5), settled));
        assert!(!eligible(&TranscriptSegment { speaker_locked: true, ..them.clone() }, settled));
        assert!(!eligible(&TranscriptSegment { speaker: "them-1".into(), ..them.clone() }, settled));
        assert!(!eligible(&TranscriptSegment { start_ms: None, ..them.clone() }, settled));
        assert!(!eligible(&TranscriptSegment { text: " ".into(), ..them.clone() }, settled));

        // An earlier recording's line (before `base`) is left alone.
        let mut n = note(vec![line("them", "before the pause", 0), line("them", "after it", 0)]);
        let mut ids = VoiceIds::for_note(&n);
        let out = label(&mut n, 1, &tl(vec![seg(0, 5_000, 1)], 9_000, false), &mut ids);
        assert_eq!(out.lines, 1);
        assert_eq!(n.transcript[0].speaker, "");
        assert_eq!(n.transcript[1].speaker, "them-1");
    }

    #[test]
    fn labels_fill_in_as_they_settle_and_split_lines_between_voices() {
        // Speaker 1 says the first line and the start of the second; Speaker
        // 2 takes over at 6.45 s. The mic and a marker sit between.
        let mut n = note(vec![
            line("them", "Morning all, the numbers are in.", 0),
            line("you", "Great, go ahead.", 3_000),
            TranscriptSegment::dictation_marker(1_004),
            line("them", "Revenue is up. Thanks, and costs are flat.", 5_000),
            line("them", "Shall we move on to hiring?", 12_000),
        ]);
        let mut ids = VoiceIds::for_note(&n);
        let turns = vec![seg(0, 6_400, 1), seg(6_450, 11_000, 2), seg(12_000, 15_000, 1)];

        // Labels settled to 4 s: the first line only.
        let out = label(&mut n, 0, &tl(turns.clone(), 4_000, false), &mut ids);
        assert_eq!((out.lines, out.added, out.span), (1, 0, Some((0, 1))));
        assert_eq!(n.transcript[0].speaker, "them-1");
        assert_eq!(n.transcript[3].speaker, "", "not settled yet");

        // To 12 s: the line across the change splits at its words.
        let out = label(&mut n, 0, &tl(turns.clone(), 12_000, false), &mut ids);
        assert_eq!((out.lines, out.added, out.span), (1, 1, Some((3, 1))));
        assert_eq!(
            shown(&n),
            [
                "Speaker 1: Morning all, the numbers are in.",
                "You: Great, go ahead.",
                "You: ",
                "Speaker 1: Revenue is up.",
                "Speaker 2: Thanks, and costs are flat.",
                "Them: Shall we move on to hiring?",
            ]
        );
        assert_eq!((n.transcript[4].start_ms, n.transcript[4].end_ms), (Some(6_500), Some(8_900)));
        assert_eq!(n.transcript[4].words[0], [0, 400]);
        assert!(n.transcript[2].dictated && n.transcript[1].speaker.is_empty());

        // Done: the rest; nothing labelled twice.
        let out = label(&mut n, 0, &tl(turns, 15_000, true), &mut ids);
        assert_eq!((out.lines, out.voices), (1, 2));
        assert_eq!(n.transcript[5].speaker, "them-1");
        assert_eq!(label(&mut n, 0, &tl(vec![seg(0, 20_000, 3)], 20_000, true), &mut ids).lines, 0);
    }

    #[test]
    fn the_users_lines_and_lines_nobody_was_heard_near_stay_put() {
        let mut n = note(vec![
            line("them", "I said this one", 0),
            line("them", "nobody near this", 30_000),
            line("them", "and old lines", 0),
        ]);
        n.transcript[2].start_ms = None;
        rename_lines(&mut n, &[0], "Sam").unwrap();
        let mut ids = VoiceIds::for_note(&n);
        let out = label(&mut n, 0, &tl(vec![seg(0, 5_000, 1)], 40_000, true), &mut ids);
        assert_eq!(out.lines, 0);
        assert!(!out.changed());
        assert_eq!(shown(&n), ["Sam: I said this one", "Them: nobody near this", "Them: and old lines"]);
        assert!(ids.ids.is_empty(), "no number used up");
    }

    #[test]
    fn a_failed_engine_labels_only_what_it_settled() {
        let mut n = note(vec![line("them", "early words here", 0), line("them", "later words here", 20_000)]);
        let mut ids = VoiceIds::for_note(&n);
        let mut failed = tl(vec![seg(0, 3_000, 1)], 5_000, true);
        failed.audio_ms = 30_000;
        failed.error = Some("stopped".into());
        let out = label(&mut n, 0, &failed, &mut ids);
        assert_eq!(out.lines, 1);
        assert_eq!(shown(&n), ["Speaker 1: early words here", "Them: later words here"]);
        // Nothing at all: You/Them as before.
        let mut m = note(vec![line("them", "words", 0)]);
        let mut none = tl(vec![], 0, true);
        none.error = Some("couldn't load".into());
        let mut ids = VoiceIds::for_note(&m);
        assert!(!label(&mut m, 0, &none, &mut ids).changed());
        assert_eq!(shown(&m), ["Them: words"]);
    }

    #[test]
    fn later_lines_of_a_voice_the_user_named_or_merged_read_as_it() {
        let mut n = note(vec![line("them", "one two three four", 0), line("them", "five six", 3_000)]);
        rename(&mut n, THEM, "Everyone").unwrap();
        let mut ids = VoiceIds::for_note(&n);
        let turns = vec![seg(0, 1_000, 1), seg(1_000, 2_000, 2), seg(3_000, 4_000, 2)];
        label(&mut n, 0, &tl(turns.clone(), 2_000, false), &mut ids);
        assert_eq!(shown(&n), ["Speaker 1: one two", "Speaker 2: three four", "Everyone: five six"]);
        // Two voices so far: the user's name for the call stays on "Them".
        assert!(!n.speakers.contains_key("them-1") && !n.speakers.contains_key("them-2"));
        // The user names Speaker 2 and merges Speaker 1 into her; the next
        // line of Speaker 2 reads as Priya too. (One voice now, but hers is
        // the user's own name: "Everyone" doesn't replace it.)
        rename(&mut n, "them-2", "Priya").unwrap();
        super::super::merge(&mut n, "them-1", "them-2").unwrap();
        label(&mut n, 0, &tl(turns, 9_000, true), &mut ids);
        assert_eq!(shown(&n), ["Priya: one two", "Priya: three four", "Priya: five six"]);
        assert_eq!(n.speakers["them-2"].name, "Priya");
    }

    fn calendar(n: &mut Note) {
        assert!(calendar_guess(n, &[("Priya Shah".into(), "priya@example.com".into())]));
    }

    #[test]
    fn one_voice_on_a_one_to_one_call_takes_the_calendar_guess() {
        let mut n = note(vec![line("you", "Hi Priya", 0), line("them", "hi there how are you", 1_000)]);
        calendar(&mut n);
        let mut ids = VoiceIds::for_note(&n);
        let out = label(&mut n, 0, &tl(vec![seg(900, 4_000, 1)], 9_000, false), &mut ids);
        assert!(out.map);
        assert_eq!(shown(&n)[1], "Priya Shah?: hi there how are you");
        assert_eq!(n.speakers["them-1"].source, Some(NameSource::Calendar));
        assert_eq!(n.speakers["them-1"].email, "priya@example.com");
        // Said once to the AI, though "Them" keeps the guess too.
        assert_eq!(
            speakers_line(&n).as_deref(),
            Some("Speakers besides You: Priya Shah (a guess: the only other person invited)")
        );
        assert_eq!(super::super::voices(&n), [YOU, "them-1"], "no line-less \"Them\" to merge with");
        // "Not Priya Shah" on the voice keeps her off it for good.
        not(&mut n, "them-1").unwrap();
        assert!(!label(&mut n, 0, &tl(vec![seg(900, 4_000, 1)], 9_000, true), &mut ids).map);
        assert_eq!(shown(&n)[1], "Speaker 1: hi there how are you");
    }

    #[test]
    fn several_voices_give_the_guess_to_the_one_who_said_most() {
        let mut n = note(vec![
            line("them", "a b", 0),
            line("them", "c d e f g h", 2_000),
            line("them", "i j k l m n o p", 6_000),
        ]);
        calendar(&mut n);
        let mut ids = VoiceIds::for_note(&n);
        let turns = vec![seg(0, 1_900, 1), seg(2_000, 5_000, 2), seg(6_000, 10_000, 3)];
        // While recording: the first to qualify, the most so far (Speaker 2).
        label(&mut n, 0, &tl(turns.clone(), 5_500, false), &mut ids);
        assert_eq!(shown(&n)[..2], ["Speaker 1: a b", "Priya Shah?: c d e f g h"]);
        // More of Speaker 3, but while recording the guess stays put…
        label(&mut n, 0, &tl(turns.clone(), 10_000, false), &mut ids);
        assert_eq!(shown(&n)[2], "Speaker 3: i j k l m n o p");
        // …and at the end goes to whoever said the most. Never to all.
        assert!(label(&mut n, 0, &tl(turns, 10_000, true), &mut ids).map);
        assert_eq!(
            shown(&n),
            ["Speaker 1: a b", "Speaker 2: c d e f g h", "Priya Shah?: i j k l m n o p"]
        );
        assert_eq!(n.speakers.values().filter(|s| s.name == "Priya Shah").count(), 2, "Them and one voice");
    }

    #[test]
    fn a_name_the_user_gave_them_goes_to_one_voice_only() {
        // One voice: it's theirs, locked.
        let mut n = note(vec![line("them", "hello hello", 0)]);
        rename(&mut n, THEM, "Sam").unwrap();
        let mut ids = VoiceIds::for_note(&n);
        label(&mut n, 0, &tl(vec![seg(0, 2_000, 1)], 3_000, true), &mut ids);
        assert_eq!(shown(&n), ["Sam: hello hello"]);
        assert!(n.speakers["them-1"].locked);
        // Several: it stays on "Them".
        let mut m = note(vec![line("them", "hello hello", 0), line("them", "bye bye", 3_000)]);
        rename(&mut m, THEM, "Sam").unwrap();
        let mut ids = VoiceIds::for_note(&m);
        label(&mut m, 0, &tl(vec![seg(0, 2_000, 1), seg(3_000, 4_000, 2)], 5_000, true), &mut ids);
        assert_eq!(shown(&m), ["Speaker 1: hello hello", "Speaker 2: bye bye"]);
        // A voice the user named keeps its name over the calendar's guess.
        let mut k = note(vec![line("them", "hi", 0)]);
        k.transcript[0].speaker = "them-1".into();
        rename(&mut k, "them-1", "Lee").unwrap();
        k.transcript.push(line("them", "more from me", 1_000));
        calendar(&mut k);
        let mut ids = VoiceIds::for_note(&k);
        ids.ids.push(((0, 1), "them-1".into()));
        label(&mut k, 0, &tl(vec![seg(0, 3_000, 1)], 4_000, true), &mut ids);
        assert_eq!(shown(&k), ["Lee: hi", "Lee: more from me"]);
    }
}

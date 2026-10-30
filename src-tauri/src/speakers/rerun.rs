//! **Find speakers** (beta): the speaker engine run again over a past
//! meeting's call side, from its audio on disk (`meeting_audio.rs`) — a
//! meeting recorded with "Tell speakers apart" off, the lines "Transcribe
//! again" made (it runs by itself after that when the switch is on), or
//! labels to redo.
//!
//! **One stream for the whole meeting**: every recording's "Them" side in
//! order, 2 s of (dithered) silence between them, so the engine keeps each
//! voice's number across a pause or a resume — the voices of a meeting
//! paused and resumed come out matched. As fast as the engine takes it
//! (~2.4% of real time on 4 threads: an hour in about a minute and a half),
//! with progress and Cancel on the note's job line (`meeting_audio`'s jobs,
//! kind `speakers`).
//!
//! **Only when it completes** do the labels change, all at once, under the
//! notes' lock: each new voice takes the old voice it shares the most lines
//! with ([`voices::map_by_overlap`]) — so names stay, a name the person gave
//! first — or, with nothing shared, a named old voice it sounds like
//! (prints, [`voices::match_voices`]); the rest are new "Speaker N". Every
//! call line is labelled again, split where the speaker changes
//! ([`relabel`]); lines the person assigned (`speaker_locked`) keep their
//! speaker, a line they edited is never split (its voice is the one heard
//! most over it). Cancelled or failed: nothing changes. Then the names are
//! suggested again and the flags looked for (`flags.rs`).
//!
//! Logs: counts and times.

use std::sync::Mutex;

use serde_json::json;
use tauri::{AppHandle, Emitter};

use super::diarize::{self, Piece, Segment};
use super::voiceprint::{Printer, Span};
use super::voices::{self, OldVoice, Overlap, Voice};
use super::{resolve, split_line, Turn, NEAR_MS, SEPARATED_PREFIX, THEM, YOU};
use crate::meetings::meeting_audio::{self, Kind, Meta, SideJob};
use crate::notes::{Note, TranscriptSegment};

/// Silence between two recordings in the one stream.
const GAP_SAMPLES: u32 = 2 * 16_000;
/// Read and sent a second at a time.
const PIECE: usize = 16_000;
/// The share of the job's progress the engine's run is.
const RUN_SHARE: u64 = 90;

/// Whether the speaker engine can run here now (its model installed, or a
/// test staged a run).
fn engine_ready(note_id: u64) -> bool {
    staged_for(note_id) || diarize::registry::engine_files(&crate::config::load().diarization_model).is_some()
}

/// Start **Find speakers** on note `note_id` (its menus; and after
/// Transcribe again). Progress and the end come as the note's job
/// (`yap-meeting-redo`, kind `speakers`).
pub fn start(app: &AppHandle, note_id: u64) -> Result<(), String> {
    let note = crate::notes::get(note_id).ok_or("Note not found")?;
    if crate::meetings::meeting::recording_note() == Some(note_id) {
        return Err("Stop the recording first".to_string());
    }
    if crate::meetings::meeting::is_recording() {
        return Err("Yap is recording a meeting: try again when it ends".to_string());
    }
    if !note.transcript.iter().any(|s| s.source == THEM && s.is_talk() && s.start_ms.is_some()) {
        return Err("This meeting has no call lines to tell apart".to_string());
    }
    if !engine_ready(note_id) {
        return Err("Download the speaker model first (Settings → Speech-to-Text)".to_string());
    }
    let metas: Vec<Meta> = meeting_audio::recordings(note_id).into_iter().filter(|m| m.ended_ms.is_some()).collect();
    if metas.is_empty() {
        return Err("This meeting's audio isn't kept any more".to_string());
    }
    let name = diarize::registry::find(&crate::config::load().diarization_model)
        .or_else(|| diarize::registry::find(diarize::DEFAULT_MODEL))
        .map(|m| m.name)
        .unwrap_or("the speaker engine");
    meeting_audio::start_side_job(app, note_id, Kind::Speakers, name, move |app, job| run(app, note_id, &metas, job))
}

/// After "Transcribe again" made new lines: their speakers from the engine
/// run again, when speaker separation is on and can run.
pub fn after_transcribe_again(app: &AppHandle, note_id: u64) {
    if !crate::config::load().meeting_diarization || !engine_ready(note_id) {
        return;
    }
    if let Err(e) = start(app, note_id) {
        tracing::info!(note_id, "Find speakers: not after Transcribe again ({e})");
    }
}

/// The job.
fn run(app: &AppHandle, note_id: u64, metas: &[Meta], job: &SideJob) -> Result<(), String> {
    let started = std::time::Instant::now();
    let per_recording = match take_staged(note_id) {
        Some(staged) => staged.play(metas.len(), job)?,
        None => run_engine(note_id, metas, job)?,
    };
    if job.cancelled() {
        return Err(meeting_audio::CANCELLED.to_string());
    }
    // Resumed while the engine ran: the recording numbers its own voices
    // from the note as it started, and counts its lines from there, so
    // labels put on under it would give two people one number and its
    // voices to the earlier lines. Nothing changes; it can run again after.
    if crate::meetings::meeting::recording_note() == Some(note_id) {
        return Err("The meeting is recording again. Find speakers once it ends.".to_string());
    }
    (job.progress)(RUN_SHARE as u8);
    let found = apply(app, note_id, metas, &per_recording)?;
    (job.progress)(95);
    super::flags::refresh(app, note_id);
    tracing::info!(note_id, voices = found, secs = started.elapsed().as_secs(), "Find speakers: done");
    Ok(())
}

/// The engine over every recording's call side, in one stream: its turns
/// per recording, each on its own recording's clock.
fn run_engine(note_id: u64, metas: &[Meta], job: &SideJob) -> Result<Vec<Vec<Segment>>, String> {
    let total: u64 = metas.iter().map(|m| m.them_len.max(m.them_done)).sum::<u64>() + GAP_SAMPLES as u64 * metas.len() as u64;
    let mut feed = Feed { note_id, metas, at: 0, reader: None, read: 0, gap: false, lens: Vec::new() };
    let progress = |sent: u64| (job.progress)((sent * RUN_SHARE / total.max(1)).min(RUN_SHARE - 1) as u8);
    let cancelled = || job.cancelled();
    let segments = diarize::run_saved(|| feed.next(), &progress, &cancelled, meeting_audio::CANCELLED)?;
    let lens = feed.lens.clone();
    Ok(split_by_recording(&segments, &lens, GAP_SAMPLES as u64))
}

/// Reads the recordings' call sides in order, a gap of silence between.
struct Feed<'a> {
    note_id: u64,
    metas: &'a [Meta],
    at: usize,
    reader: Option<crate::meetings::meeting_audio::flac::Reader>,
    read: u64,
    gap: bool,
    /// Each recording's length as read (samples).
    lens: Vec<u64>,
}

impl Feed<'_> {
    fn next(&mut self) -> Option<Piece> {
        loop {
            if self.gap {
                self.gap = false;
                return Some(Piece::Silence(GAP_SAMPLES));
            }
            let meta = self.metas.get(self.at)?;
            if self.reader.is_none() {
                match meeting_audio::open_side(self.note_id, meta.recording, "them") {
                    Ok(r) => self.reader = Some(r),
                    Err(e) => {
                        tracing::warn!(note_id = self.note_id, "Find speakers: a recording's call side can't be read ({e})");
                        self.lens.push(0);
                        self.at += 1;
                        self.gap = self.at < self.metas.len();
                        continue;
                    }
                }
            }
            let piece = self.reader.as_mut().map(|r| r.read(PIECE)).unwrap_or_default();
            if piece.is_empty() {
                self.lens.push(self.read);
                self.reader = None;
                self.read = 0;
                self.at += 1;
                self.gap = self.at < self.metas.len();
                continue;
            }
            self.read += piece.len() as u64;
            return Some(Piece::Audio(piece));
        }
    }
}

/// One stream's segments back onto each recording's clock: recording `r`
/// starts at the sum of the lengths before it (`lens`, samples) and a
/// `gap` after each.
pub fn split_by_recording(segments: &[Segment], lens: &[u64], gap: u64) -> Vec<Vec<Segment>> {
    let mut out = vec![Vec::new(); lens.len()];
    let mut offset = 0u64;
    for (r, &len) in lens.iter().enumerate() {
        let (from, to) = (offset * 1000 / 16_000, (offset + len) * 1000 / 16_000);
        for s in segments {
            let (a, b) = (s.start_ms.max(from), s.end_ms.min(to));
            if b > a {
                out[r].push(Segment { start_ms: a - from, end_ms: b - from, ..*s });
            }
        }
        offset += len + gap;
    }
    out
}

/// Which of `metas` a line dated `ts` is in (as `meeting_audio::clip`
/// finds it): the latest that started by then and hadn't ended a second
/// before.
pub fn recording_of(metas: &[Meta], ts: u64) -> Option<usize> {
    let ms = ts * 1000;
    metas
        .iter()
        .enumerate()
        .rev()
        .find(|(_, m)| {
            // One being recorded has no length yet: open-ended.
            let end = if m.you_len.max(m.them_len) > 0 { m.end_ms() } else { u64::MAX / 2 };
            ms + 999 >= m.started_ms && ms <= end + 1000
        })
        .map(|(i, _)| i)
}

/// A call line Find speakers may label: talk, placed, not the person's.
fn eligible(seg: &TranscriptSegment) -> bool {
    seg.source == THEM && seg.is_talk() && !seg.speaker_locked && !seg.text.trim().is_empty() && seg.start_ms.is_some() && seg.end_ms.is_some()
}

/// A line of a told-apart voice the person said is someone else: merged
/// into a person ("Rename everywhere" to a name another line already has),
/// "It was me", or "Them" (everyone on the call). Its lines are the
/// person's, as a line they renamed is: the run never gives them a new
/// number (until 2026-10-30 it did, and the voice's merge went with its
/// last line). A voice merged into another told-apart one is still the
/// run's to map.
fn put_elsewhere(map: &crate::speakers::Speakers, seg: &TranscriptSegment) -> bool {
    let raw = seg.speaker_id();
    let to = resolve(map, raw);
    raw.starts_with(SEPARATED_PREFIX) && to != raw && !to.starts_with(SEPARATED_PREFIX)
}

fn overlap(a: (u64, u64), b: (u64, u64)) -> u64 {
    a.1.min(b.1).saturating_sub(a.0.max(b.0))
}

/// The run's voices given the note's ids (`speaker` u8 → id): mapped by
/// overlap with the old labels, else by sound to a named old voice, else
/// new numbers. Read from a copy of the note; prints made here.
fn ids_for(note: &Note, metas: &[Meta], runs: &[Vec<Segment>], note_id: u64) -> Vec<(u8, String)> {
    // Old voices: the call lines' separated voices (after merges).
    let mut old: Vec<OldVoice<String>> = Vec::new();
    let mut overlaps: Vec<Overlap<u8, String>> = Vec::new();
    for seg in &note.transcript {
        if seg.source != THEM || !seg.is_talk() {
            continue;
        }
        let id = resolve(&note.speakers, seg.speaker_id()).to_string();
        if !id.starts_with(SEPARATED_PREFIX) {
            continue;
        }
        let (Some(start), Some(end), Some(r)) = (seg.start_ms, seg.end_ms, recording_of(metas, seg.ts)) else {
            continue;
        };
        let len = end.saturating_sub(start);
        match old.iter_mut().find(|v| v.id == id) {
            Some(v) => v.ms += len,
            None => old.push(OldVoice { rank: note.speakers.get(&id).map_or(0, |s| s.rank()), id: id.clone(), ms: len }),
        }
        for s in runs.get(r).into_iter().flatten() {
            let ms = overlap((start, end), (s.start_ms, s.end_ms));
            if ms == 0 {
                continue;
            }
            let assigned = if seg.speaker_locked { ms } else { 0 };
            match overlaps.iter_mut().find(|o| o.new == s.speaker && o.old == id) {
                Some(o) => {
                    o.ms += ms;
                    o.assigned += assigned;
                }
                None => overlaps.push(Overlap { new: s.speaker, old: id.clone(), ms, assigned }),
            }
        }
    }
    let mut mapped = voices::map_by_overlap(&overlaps, &old);

    // Nothing shared, but a named old voice left over: by sound.
    let mut heard: Vec<u8> = Vec::new();
    for s in runs.iter().flatten() {
        if !heard.contains(&s.speaker) {
            heard.push(s.speaker);
        }
    }
    let left_new: Vec<u8> = heard.iter().copied().filter(|k| !mapped.iter().any(|(n, _)| n == k)).collect();
    let left_old: Vec<&OldVoice<String>> = old.iter().filter(|v| v.rank > 0 && !mapped.iter().any(|(_, o)| *o == v.id)).collect();
    if !left_new.is_empty() && !left_old.is_empty() {
        if let Some(printer) = Printer::for_note(note_id) {
            let old_prints: Vec<Voice<String>> = left_old
                .iter()
                .filter_map(|v| {
                    let spans = line_spans(note, metas, |seg| resolve(&note.speakers, seg.speaker_id()) == v.id);
                    let ms = spans.iter().map(Span::ms).sum();
                    printer.voice(&spans, 60_000).map(|print| Voice { key: v.id.clone(), print, ms })
                })
                .collect();
            let new_prints: Vec<Voice<u8>> = left_new
                .iter()
                .filter_map(|&k| {
                    let spans = turn_spans(metas, runs, k);
                    let ms = spans.iter().map(Span::ms).sum();
                    printer.voice(&spans, 60_000).map(|print| Voice { key: k, print, ms })
                })
                .collect();
            for (k, id, s) in voices::match_voices(&old_prints, &new_prints) {
                tracing::debug!(similar = s, "Find speakers: a voice matched by sound");
                mapped.push((k, id));
            }
        }
    }

    // The rest: new numbers, on from the note's highest, in order heard.
    let mut ids = super::labels::VoiceIds::for_note(note);
    heard
        .into_iter()
        .map(|k| {
            let id = mapped.iter().find(|(n, _)| *n == k).map(|(_, id)| id.clone()).unwrap_or_else(|| ids.next_id());
            (k, id)
        })
        .collect()
}

/// The stretches of `note`'s call lines that `pick` picks, as spans.
pub fn line_spans(note: &Note, metas: &[Meta], pick: impl Fn(&TranscriptSegment) -> bool) -> Vec<Span> {
    note.transcript
        .iter()
        .filter(|s| s.source == THEM && s.is_talk() && pick(s))
        .filter_map(|s| {
            let r = recording_of(metas, s.ts)?;
            Some(Span { recording: metas[r].recording, index: r, side: THEM.to_string(), start_ms: s.start_ms?, end_ms: s.end_ms? })
        })
        .collect()
}

/// Engine voice `k`'s turns, as spans.
fn turn_spans(metas: &[Meta], runs: &[Vec<Segment>], k: u8) -> Vec<Span> {
    runs.iter()
        .enumerate()
        .flat_map(|(r, segs)| {
            segs.iter().filter(move |s| s.speaker == k).map(move |s| Span {
                recording: metas[r].recording,
                index: r,
                side: THEM.to_string(),
                start_ms: s.start_ms,
                end_ms: s.end_ms,
            })
        })
        .collect()
}

/// What [`relabel`] changed.
#[derive(Debug, Default, PartialEq)]
pub struct Relabelled {
    /// Lines given a voice (whole, or split into parts).
    pub lines: usize,
    /// Lines the splits added.
    pub added: usize,
}

/// Put a run's voices on `note`'s call lines: `turns[r]` are recording
/// `r`'s (voices already the note's ids), `rec_of` says which recording a
/// line is in. Every line Find speakers may label is labelled again: split
/// where the speaker changes ([`split_line`]), an edited line whole (the
/// voice heard most over it), a line with no voice near it left as it was.
pub fn relabel(note: &mut Note, turns: &[Vec<Turn>], rec_of: impl Fn(&TranscriptSegment) -> Option<usize>) -> Relabelled {
    let mut out = Relabelled::default();
    let mut i = 0;
    while i < note.transcript.len() {
        let seg = &note.transcript[i];
        let Some(r) = (eligible(seg) && !put_elsewhere(&note.speakers, seg)).then(|| rec_of(seg)).flatten() else {
            i += 1;
            continue;
        };
        let (start, end) = (seg.start_ms.unwrap_or(0), seg.end_ms.unwrap_or(0));
        let near: Vec<Turn> = turns
            .get(r)
            .into_iter()
            .flatten()
            .filter(|t| t.end_ms + NEAR_MS >= start && t.start_ms <= end + NEAR_MS)
            .cloned()
            .collect();
        if near.is_empty() {
            i += 1;
            continue;
        }
        if seg.edited {
            // Never split: the voice heard most over it.
            let best = near
                .iter()
                .max_by_key(|t| (overlap((start, end), (t.start_ms, t.end_ms)), std::cmp::Reverse(t.start_ms.abs_diff(start))))
                .map(|t| t.speaker.clone());
            if let Some(v) = best {
                let v = if v == seg.source { String::new() } else { v };
                if note.transcript[i].speaker != v {
                    note.transcript[i].speaker = v;
                    out.lines += 1;
                }
            }
            i += 1;
            continue;
        }
        let fresh = TranscriptSegment { speaker: String::new(), ..seg.clone() };
        let parts = split_line(&fresh, &near);
        if parts.iter().any(|p| p.speaker.is_empty()) {
            // Part of it near no voice: as it was.
            i += 1;
            continue;
        }
        let n = parts.len();
        let same = n == 1 && parts[0].speaker == seg.speaker && parts[0].text == seg.text;
        if !same {
            crate::notes::replace_line(note, i, parts);
            out.lines += 1;
            out.added += n - 1;
        }
        i += n;
    }
    out
}

/// Map entries of told-apart voices no line uses any more (and nobody
/// merged into, and the person didn't name) go.
pub fn prune_voices(note: &mut Note) {
    loop {
        let gone: Vec<String> = note
            .speakers
            .iter()
            .filter(|(id, s)| {
                id.starts_with(SEPARATED_PREFIX)
                    && !s.locked
                    && !note.transcript.iter().any(|l| l.speaker == **id)
                    && !note.speakers.values().any(|o| o.merged_into == **id)
            })
            .map(|(id, _)| id.clone())
            .collect();
        if gone.is_empty() {
            return;
        }
        for id in gone {
            note.speakers.remove(&id);
        }
    }
}

/// Put the run's voices on the note (under its lock), suggest names again,
/// tell the windows. Returns how many voices it found.
fn apply(app: &AppHandle, note_id: u64, metas: &[Meta], runs: &[Vec<Segment>]) -> Result<usize, String> {
    let snapshot = crate::notes::get(note_id).ok_or("Note not found")?;
    let ids = ids_for(&snapshot, metas, runs, note_id);
    let turns: Vec<Vec<Turn>> = runs
        .iter()
        .map(|segs| {
            segs.iter()
                .filter_map(|s| {
                    let id = ids.iter().find(|(k, _)| *k == s.speaker)?.1.clone();
                    Some(Turn { start_ms: s.start_ms, end_ms: s.end_ms, speaker: id })
                })
                .collect()
        })
        .collect();
    let sources = super::naming::Sources::now();
    let words = crate::memory::Words::new();
    let ordinary = |w: &str| words.is(w);
    let ctx = sources.context(&ordinary, Vec::new());
    let (out, found) = crate::notes::modify(note_id, |note| {
        let out = relabel(note, &turns, |seg| recording_of(metas, seg.ts));
        prune_voices(note);
        // "Them"'s name (a 1:1 invite's guess, say) onto the voices found.
        let mut found: Vec<(String, usize)> = Vec::new();
        for seg in note.transcript.iter().filter(|s| s.source == THEM && s.is_talk()) {
            let id = resolve(&note.speakers, seg.speaker_id());
            if id == YOU || id == THEM || !id.starts_with(SEPARATED_PREFIX) {
                continue;
            }
            let n = seg.text.split_whitespace().count();
            match found.iter_mut().find(|(v, _)| v == id) {
                Some((_, w)) => *w += n,
                None => found.push((id.to_string(), n)),
            }
        }
        found.sort_by_key(|(_, w)| std::cmp::Reverse(*w));
        found.retain(|(v, _)| !super::naming::repeats_you(note, v));
        super::labels::settle_them(note, &found, true);
        if sources.enabled {
            super::naming::refresh(note, &ctx);
        }
        Ok(((out, found.len()), true))
    })?;
    tracing::info!(note_id, lines = out.lines, added = out.added, voices = found, "Find speakers: labels replaced");
    if let Some(note) = crate::notes::get(note_id) {
        let mut payload = crate::notes::commands::note_changed_payload(&note, "speakers");
        payload["lineSpeakers"] = json!(super::line_speakers(&note));
        payload["breaks"] = json!(note.breaks);
        let _ = app.emit("yap-note-changed", payload);
        let _ = app.emit("yap-notes-changed", ());
    }
    Ok(found)
}

// ---- Test mode: a run staged by the e2e suite ----

/// A run a test staged for a note: each recording's turns, and how long
/// the run takes (so its progress and Cancel can be seen).
struct Staged {
    note_id: u64,
    recordings: Vec<Vec<Segment>>,
    pace_ms: u64,
}

static STAGED: Mutex<Vec<Staged>> = Mutex::new(Vec::new());

fn staged_for(note_id: u64) -> bool {
    STAGED.lock().unwrap_or_else(|p| p.into_inner()).iter().any(|s| s.note_id == note_id)
}

fn take_staged(note_id: u64) -> Option<Staged> {
    let mut all = STAGED.lock().unwrap_or_else(|p| p.into_inner());
    let i = all.iter().position(|s| s.note_id == note_id)?;
    Some(all.remove(i))
}

impl Staged {
    /// As the engine would: progress over `pace_ms`, Cancel between steps.
    fn play(self, recordings: usize, job: &SideJob) -> Result<Vec<Vec<Segment>>, String> {
        const STEPS: u64 = 20;
        for step in 0..STEPS {
            if job.cancelled() {
                return Err(meeting_audio::CANCELLED.to_string());
            }
            (job.progress)((step * RUN_SHARE / STEPS) as u8);
            std::thread::sleep(std::time::Duration::from_millis(self.pace_ms / STEPS));
        }
        let mut out = self.recordings;
        out.resize(recordings, Vec::new());
        Ok(out)
    }
}

/// Test mode: the next Find speakers on `noteId` finds these turns
/// (`recordings[r]`: recording r's, on its clock; `speaker` the engine's
/// numbers, one stream for the meeting) and takes `paceMs`.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_find_speakers(note_id: u64, recordings: Vec<Vec<super::diarize::ScriptedSegment>>, pace_ms: u64) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    let recordings = recordings
        .into_iter()
        .map(|segs| {
            segs.into_iter()
                .filter(|s| s.end_ms > s.start_ms)
                .map(|s| Segment { start_ms: s.start_ms, end_ms: s.end_ms, speaker: s.speaker, stream: 0 })
                .collect()
        })
        .collect();
    let mut all = STAGED.lock().unwrap_or_else(|p| p.into_inner());
    all.retain(|s| s.note_id != note_id);
    all.push(Staged { note_id, recordings, pace_ms: pace_ms.min(60_000) });
    Ok(())
}

/// **Find speakers** on a meeting note (the notepad's ⋯, the Meetings and
/// Notes views' menus).
#[tauri::command]
pub fn speaker_find(app: AppHandle, note_id: u64) -> Result<(), String> {
    start(&app, note_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::speakers::{display_line, rename, rename_lines};

    fn note(lines: Vec<TranscriptSegment>) -> Note {
        let mut n: Note = serde_json::from_value(json!({ "id": 1, "createdTs": 1, "updatedTs": 1 })).unwrap();
        n.transcript = lines;
        n
    }

    /// A placed call line: a word every 500 ms from `start`.
    fn line(text: &str, start: u64) -> TranscriptSegment {
        let n = text.split_whitespace().count() as u32;
        TranscriptSegment {
            source: THEM.into(),
            text: text.into(),
            ts: 1_000 + start / 1000,
            start_ms: Some(start),
            end_ms: Some(start + (n as u64 - 1) * 500 + 400),
            words: (0..n).map(|k| [k * 500, k * 500 + 400]).collect(),
            ..Default::default()
        }
    }

    fn turn(start_ms: u64, end_ms: u64, speaker: &str) -> Turn {
        Turn { start_ms, end_ms, speaker: speaker.into() }
    }

    fn shown(n: &Note) -> Vec<String> {
        n.transcript.iter().map(|s| format!("{}: {}", display_line(&n.speakers, s), s.text)).collect()
    }

    #[test]
    fn one_streams_segments_go_back_to_their_recordings() {
        let seg = |start_ms, end_ms, speaker| Segment { start_ms, end_ms, speaker, stream: 0 };
        // Two recordings of 10 s and 6 s, 2 s apart in the stream.
        let lens = [160_000, 96_000];
        let got = split_by_recording(&[seg(0, 4_000, 1), seg(9_000, 13_000, 2), seg(12_500, 15_000, 1)], &lens, 32_000);
        assert_eq!(got[0], vec![seg(0, 4_000, 1), seg(9_000, 10_000, 2)]);
        // The gap is nobody's (a turn across it is cut at both ends);
        // Speaker 1 is the same voice after the pause.
        assert_eq!(got[1], vec![seg(0, 1_000, 2), seg(500, 3_000, 1)]);
        assert!(split_by_recording(&[], &[], 0).is_empty());
    }

    #[test]
    fn a_run_again_relabels_splits_and_keeps_the_persons_lines() {
        let mut n = note(vec![
            line("Morning all the numbers are in", 0),
            line("Revenue is up thanks and costs are flat", 5_000),
            line("I said this myself", 12_000),
            line("Typed over by hand", 16_000),
            line("Far from any voice", 60_000),
        ]);
        // Live labels: one voice for all of it (the engine missed a change).
        for s in n.transcript.iter_mut() {
            s.speaker = "them-1".into();
        }
        rename_lines(&mut n, &[2], "Sam").unwrap();
        n.transcript[3].edited = true;
        n.transcript[3].speaker = "them-1".into();
        let turns = vec![vec![
            turn(0, 6_900, "them-1"),
            turn(6_950, 11_000, "them-2"),
            turn(12_000, 14_000, "them-2"),
            turn(15_900, 18_000, "them-1"),
            turn(18_000, 20_000, "them-2"),
        ]];
        let out = relabel(&mut n, &turns, |_| Some(0));
        assert_eq!(out, Relabelled { lines: 1, added: 1 });
        assert_eq!(
            shown(&n),
            [
                "Speaker 1: Morning all the numbers are in",
                "Speaker 1: Revenue is up thanks",
                "Speaker 2: and costs are flat",
                "Sam: I said this myself",
                "Speaker 1: Typed over by hand",
                "Speaker 1: Far from any voice",
            ]
        );
        // Again with the same turns: nothing changes.
        assert_eq!(relabel(&mut n, &turns, |_| Some(0)), Relabelled::default());
        // A line from no recording on disk is left alone.
        let before = shown(&n);
        assert_eq!(relabel(&mut n, &[vec![turn(0, 100_000, "them-9")]], |_| None), Relabelled::default());
        assert_eq!(shown(&n), before);
    }

    /// A told-apart voice the person put elsewhere — renamed everywhere to a
    /// name a line already had (merged into that person), "It was me", or
    /// merged into Them — keeps its lines through a run again: until
    /// 2026-10-30 they became a new "Speaker N", and the merge went with the
    /// voice once no line had it.
    #[test]
    fn a_run_again_keeps_a_voice_the_person_put_elsewhere() {
        let mut n = note(vec![
            line("Morning all the numbers are in", 0),
            line("Revenue is up and costs are flat", 5_000),
            line("I can take that one", 12_000),
            line("Lunch is at noon", 16_000),
            line("Thanks everyone", 20_000),
        ]);
        for (s, v) in n.transcript.iter_mut().zip(["them-1", "them-2", "them-3", "them-4", "them-5"]) {
            s.speaker = v.into();
        }
        // Sam named on one line (a person), then Speaker 2 renamed
        // everywhere to Sam: merged into that person.
        rename_lines(&mut n, &[0], "Sam").unwrap();
        rename(&mut n, "them-2", "Sam").unwrap();
        // Speaker 3 was the person; Speaker 4 is everyone else on the call.
        crate::speakers::naming::is_me(&mut n, "them-3").unwrap();
        crate::speakers::merge(&mut n, "them-4", THEM).unwrap();
        let before = shown(&n);
        assert_eq!(before[1], "Sam: Revenue is up and costs are flat");
        // The engine hears one new voice over all of it.
        let turns = vec![vec![turn(0, 25_000, "them-7")]];
        let out = relabel(&mut n, &turns, |_| Some(0));
        prune_voices(&mut n);
        let after = shown(&n);
        assert_eq!(after[..4], before[..4], "the person's say stays");
        assert_eq!(out, Relabelled { lines: 1, added: 0 }, "only the line nobody placed");
        assert_eq!(after[4], "Speaker 7: Thanks everyone");
        assert_eq!(resolve(&n.speakers, "them-2"), resolve(&n.speakers, "p-1"), "the merge is kept");
    }

    #[test]
    fn voices_with_no_lines_left_go_unless_named() {
        let mut n = note(vec![TranscriptSegment { speaker: "them-2".into(), ..line("hello there", 0) }]);
        n.speakers.insert("them-1".into(), Default::default());
        n.speakers.insert("them-3".into(), Default::default());
        n.speakers.insert("them-4".into(), crate::speakers::Speaker { merged_into: "them-3".into(), ..Default::default() });
        n.speakers.insert("them-5".into(), Default::default());
        rename(&mut n, "them-5", "Priya").unwrap();
        prune_voices(&mut n);
        let left: Vec<&String> = n.speakers.keys().collect();
        assert_eq!(left, ["them-5"], "them-3 went once them-4 (merged into it, no lines) did");
    }

    #[test]
    fn a_line_is_in_the_recording_it_was_said_in() {
        let meta = |started_ms: u64, secs: u64| Meta { started_ms, them_len: secs * 16_000, you_len: secs * 16_000, ..Default::default() };
        let metas = [meta(1_000_000, 60), meta(2_000_000, 60)];
        assert_eq!(recording_of(&metas, 1_000), Some(0));
        assert_eq!(recording_of(&metas, 1_059), Some(0));
        assert_eq!(recording_of(&metas, 2_010), Some(1));
        assert_eq!(recording_of(&metas, 1_500), None, "between them");
    }
}

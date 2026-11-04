//! **Play a voice**: a few seconds of one voice's clearest speech, from its
//! speaker menu or its name card ("Speaker 2 is Priya? ▶"), to hear who it
//! is before naming it — and to compare two voices a flag says may be one.
//!
//! Which seconds ([`voices::choose_clip`]): one of the voice's lines,
//! longest first up to 8 s (cut at a word's end), passed over when another
//! voice talks within half a second of it (either side of the call), when
//! it's mostly pause, or when it's near silent (the top few are read for
//! their loudness); a line the person assigned counts more (it's surely
//! theirs). Only that line's side is played, so the other side isn't heard
//! over it. Lines from recordings no longer on disk are skipped.
//!
//! Logs: nothing (a line's ▶ is the person's own action).

use super::voices::{self, ClipLine};
use super::{resolve, Note};
use crate::meetings::meeting_audio::{self, Meta};
use crate::notes::TranscriptSegment;

/// Padding each side of the clip.
const PAD_MS: u64 = 150;
/// The best few by their lines alone are read for their loudness.
const LEVEL_CHECKS: usize = 5;

/// A line's clip candidate (its recording too).
fn candidate(note: &Note, metas: &[Meta], seg: &TranscriptSegment) -> Option<(usize, ClipLine)> {
    let (start, end) = (seg.start_ms?, seg.end_ms?);
    let r = super::rerun::recording_of(metas, seg.ts)?;
    let words = seg.text.split_whitespace().count();
    let word_ends = if seg.words.len() == words { seg.words.iter().map(|[_, e]| start + *e as u64).collect() } else { Vec::new() };
    let me = resolve(&note.speakers, seg.speaker_id());
    let crosstalk = note.transcript.iter().any(|o| {
        o.is_talk()
            && !o.text.trim().is_empty()
            && resolve(&note.speakers, o.speaker_id()) != me
            && super::rerun::recording_of(metas, o.ts) == Some(r)
            && o.start_ms.zip(o.end_ms).is_some_and(|(a, b)| a < end + voices::CROSSTALK_MS && b + voices::CROSSTALK_MS > start)
    });
    Some((r, ClipLine { start_ms: start, end_ms: end, word_ends, words, crosstalk, assigned: seg.speaker_locked, level: None }))
}

/// Voice `voice`'s clip from note `note_id`, as a 16 kHz WAV.
pub fn clip(note_id: u64, voice: &str) -> Result<Vec<u8>, String> {
    let note = crate::notes::get(note_id).ok_or("Note not found")?;
    let voice = resolve(&note.speakers, voice).to_string();
    let metas = meeting_audio::recordings(note_id);
    if metas.is_empty() {
        return Err("This meeting's audio isn't kept any more".to_string());
    }
    let mut cands: Vec<(usize, &TranscriptSegment, ClipLine)> = note
        .transcript
        .iter()
        .filter(|s| s.is_talk() && !s.text.trim().is_empty() && resolve(&note.speakers, s.speaker_id()) == voice)
        .filter_map(|s| candidate(&note, &metas, s).map(|(r, c)| (r, s, c)))
        .collect();
    if cands.is_empty() {
        return Err("None of their lines' audio is kept".to_string());
    }
    // The best few by their lines: how loud they really are.
    let mut order: Vec<usize> = (0..cands.len()).collect();
    order.sort_by(|&a, &b| voices::clip_score(&cands[b].2).total_cmp(&voices::clip_score(&cands[a].2)));
    for &i in order.iter().take(LEVEL_CHECKS) {
        let (r, seg, c) = &cands[i];
        let n = (c.end_ms.saturating_sub(c.start_ms).min(voices::CLIP_MAX_MS) * 16) as usize;
        if let Ok(a) = meeting_audio::read_side(note_id, metas[*r].recording, &seg.source, c.start_ms * 16, n) {
            let rms = (a.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / a.len().max(1) as f64).sqrt();
            cands[i].2.level = Some(rms as f32);
        }
    }
    // Only the ones whose loudness was read are in the running when any
    // was (the rest are no better by their lines).
    let pool: Vec<usize> = if cands.iter().any(|c| c.2.level.is_some()) {
        (0..cands.len()).filter(|&i| cands[i].2.level.is_some()).collect()
    } else {
        (0..cands.len()).collect()
    };
    let lines: Vec<ClipLine> = pool.iter().map(|&i| cands[i].2.clone()).collect();
    let (best, start, end) = voices::choose_clip(&lines).ok_or("No line of theirs to play")?;
    let (r, seg, _) = &cands[pool[best]];
    let from = start.saturating_sub(PAD_MS);
    let n = ((end + PAD_MS - from) * 16) as usize;
    let audio = meeting_audio::read_side(note_id, metas[*r].recording, &seg.source, from * 16, n)?;
    if audio.is_empty() {
        return Err("Their audio isn't there".to_string());
    }
    Ok(crate::dictation::recent_audio::wav(&audio))
}

/// A voice's ▶ (its speaker menu, its name card): its clearest few
/// seconds, as a WAV.
#[tauri::command]
pub fn speaker_clip(note_id: u64, speaker: String) -> Result<tauri::ipc::Response, String> {
    clip(note_id, &speaker).map(tauri::ipc::Response::new)
}

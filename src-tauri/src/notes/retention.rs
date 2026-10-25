//! **Delete meeting transcripts after N days** (Settings → Privacy & data,
//! `meeting_transcript_days`: off by default; 30, 90 or 365 days). Wispr
//! Flow deletes transcripts on a schedule; Yap keeps the meeting's **notes
//! and summary**: its title, My thoughts (what the person typed, never
//! touched), the summary and its ticks, the attendees, the calendar event
//! and its tasks stay. What goes is what was heard: the lines, the rolling
//! digests written from them, the stop marks and the voices' map — and the
//! meeting's kept audio with them (`meeting_audio::drop_audio`; the audio's
//! own days, Settings → Meetings, can only be shorter: `meeting_audio::
//! keep_ms` takes the smaller). The note says when (`transcript_deleted_ts`)
//! and its Transcript tab says why.
//!
//! **When**: a meeting's newest line (else when the note was made) older
//! than the days ([`due`], pure). Never the meeting recording now, nor one
//! being transcribed or summarised. At start, hourly, and as soon as the
//! setting is shortened.
//!
//! **Search**: Ask across meetings reads lines from the notes as they are
//! (`search/`); the vectors of the deleted lines' passages are dropped from
//! the index at once (`search::index::drop_gone`), not left until they pile
//! up.
//!
//! Logs: ids and counts only.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Emitter};

use crate::notes::Note;

const DAY: u64 = 86_400;
/// How often old transcripts are looked for while Yap runs.
const EVERY: Duration = Duration::from_secs(60 * 60);

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// When a meeting was, for its age: its newest line, else when its note was
/// made (unix seconds).
pub fn meeting_time(n: &Note) -> u64 {
    n.transcript.iter().map(|s| s.ts).max().unwrap_or(n.created_ts)
}

/// Which notes' transcripts go now (pure): those with lines whose meeting
/// is more than `days` before `now` — never one in `busy` (recording, being
/// transcribed or summarised). `days` 0: none.
pub fn due(notes: &[Note], now: u64, days: u32, busy: &[u64]) -> Vec<u64> {
    if days == 0 {
        return Vec::new();
    }
    let cutoff = now.saturating_sub(days as u64 * DAY);
    notes
        .iter()
        .filter(|n| !n.transcript.is_empty() && !busy.contains(&n.id) && meeting_time(n) < cutoff)
        .map(|n| n.id)
        .collect()
}

/// `note` without its transcript (pure): see the module notes. The person's
/// own text, the summary, the attendees and the tasks are untouched.
pub fn strip(note: &mut Note, now: u64) {
    note.transcript.clear();
    note.digests.clear();
    note.breaks.clear();
    note.speakers.clear();
    note.transcript_deleted_ts = now.max(1);
}

/// The notes nothing may touch now: the one recording, and those being
/// transcribed or summarised.
fn busy(notes: &[Note]) -> Vec<u64> {
    let mut out: Vec<u64> = crate::meetings::meeting::recording_note().into_iter().collect();
    out.extend(notes.iter().map(|n| n.id).filter(|&id| {
        crate::meetings::meeting_audio::running_job(id).is_some() || crate::meetings::meeting_end::summary_running_for(id)
    }));
    out
}

/// Delete the transcripts past their days (see the module notes). Returns
/// how many meetings lost theirs.
pub fn sweep(app: Option<&AppHandle>) -> usize {
    let days = crate::config::load().meeting_transcript_days;
    if days == 0 {
        return 0;
    }
    let notes = crate::notes::all();
    let now = now_secs();
    let ids = due(&notes, now, days, &busy(&notes));
    if ids.is_empty() {
        return 0;
    }
    let mut done = Vec::new();
    for id in ids {
        // Checked again under the lock: a recording may have resumed into it.
        let stripped = crate::notes::modify_quietly(id, |n| {
            let still = !n.transcript.is_empty()
                && meeting_time(n) < now.saturating_sub(days as u64 * DAY)
                && crate::meetings::meeting::recording_note() != Some(n.id);
            if still {
                strip(n, now);
            }
            Ok((still, still))
        });
        if stripped == Ok(true) {
            crate::meetings::meeting_audio::drop_audio(id);
            done.push(id);
        }
    }
    if done.is_empty() {
        return 0;
    }
    // The search index: the deleted lines' passages out of it now.
    match crate::search::index::drop_gone() {
        Ok(gone) => tracing::info!(passages = gone, "Transcript retention: search index swept"),
        Err(e) => tracing::warn!("Transcript retention: couldn't sweep the search index ({e})"),
    }
    tracing::info!(meetings = done.len(), days, "Transcript retention: old meeting transcripts deleted");
    if let Some(app) = app {
        for id in &done {
            let _ = app.emit("yap-note-transcript-deleted", serde_json::json!({ "noteId": id }));
            crate::meetings::meeting_audio::changed(app, *id);
        }
        let _ = app.emit("yap-notes-changed", ());
    }
    done.len()
}

/// At start (once things settle) and hourly.
pub fn init(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new().name("transcript-retention".into()).spawn(move || {
        std::thread::sleep(if crate::shell::e2e::active() { Duration::from_millis(500) } else { Duration::from_secs(90) });
        loop {
            sweep(Some(&app));
            std::thread::sleep(EVERY);
        }
    });
}

/// The setting changed: shortened, the older transcripts go now.
pub fn on_config_saved(app: &AppHandle, prev: &crate::config::YapConfig, cfg: &crate::config::YapConfig) {
    if cfg.meeting_transcript_days != prev.meeting_transcript_days && cfg.meeting_transcript_days > 0 {
        let app = app.clone();
        std::thread::spawn(move || sweep(Some(&app)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::TranscriptSegment;
    use serde_json::json;

    const NOW: u64 = 1_761_400_000;

    fn meeting(id: u64, days_ago: u64) -> Note {
        let ts = NOW - days_ago * DAY;
        let mut n: Note = serde_json::from_value(json!({
            "id": id, "title": format!("Meeting {id}"), "content": "my own notes", "noteType": "meeting",
            "enhancedContent": "## Action plan\n### You\n- [x] send the quote",
            "participants": ["Priya"], "createdTs": ts - 600, "updatedTs": ts,
        }))
        .unwrap();
        n.transcript = vec![
            TranscriptSegment { source: "you".into(), text: "hello".into(), ts: ts - 300, ..Default::default() },
            TranscriptSegment { source: "them".into(), text: "hi".into(), ts, ..Default::default() },
        ];
        n.breaks = vec![2];
        n
    }

    #[test]
    fn transcripts_go_after_their_days_by_the_meetings_newest_line() {
        let notes = [meeting(1, 100), meeting(2, 20), meeting(3, 31), meeting(4, 400)];
        assert_eq!(due(&notes, NOW, 0, &[]), Vec::<u64>::new(), "off: none");
        assert_eq!(due(&notes, NOW, 30, &[]), vec![1, 3, 4]);
        assert_eq!(due(&notes, NOW, 90, &[]), vec![1, 4]);
        assert_eq!(due(&notes, NOW, 365, &[]), vec![4]);
        // Never the meeting recording (or being transcribed, summarised).
        assert_eq!(due(&notes, NOW, 30, &[3]), vec![1, 4]);
        // A meeting resumed lately: dated by its newest line.
        let mut resumed = meeting(5, 200);
        resumed.transcript.push(TranscriptSegment { source: "you".into(), text: "back".into(), ts: NOW - DAY, ..Default::default() });
        assert!(due(&[resumed], NOW, 30, &[]).is_empty());
        // A note with no transcript (typed notes only): nothing to delete.
        let mut typed = meeting(6, 400);
        typed.transcript.clear();
        assert!(due(&[typed], NOW, 30, &[]).is_empty());
    }

    #[test]
    fn deleting_a_transcript_keeps_the_notes_summary_and_tasks() {
        let mut n = meeting(1, 100);
        let before = n.clone();
        strip(&mut n, NOW);
        assert!(n.transcript.is_empty() && n.digests.is_empty() && n.breaks.is_empty() && n.speakers.is_empty());
        assert_eq!(n.transcript_deleted_ts, NOW);
        assert_eq!(n.content, before.content, "what the person typed: never touched");
        assert_eq!(n.enhanced_content, before.enhanced_content);
        assert_eq!((n.title.as_str(), &n.participants), (before.title.as_str(), &before.participants));
        // (Tasks added to it or archived live in tasks.json: never touched here.)
        assert_eq!(crate::notes::tasks::parse(&n.enhanced_content).len(), 1);
        // On disk: the mark, and no lines.
        let v = serde_json::to_value(&n).unwrap();
        assert_eq!(v["transcriptDeletedTs"], json!(NOW));
        assert_eq!(v["transcript"], json!([]));
    }
}

//! Two small AI helpers for a meeting, both on the meeting's own model (the
//! Note Formatting scope, else the cleanup model; `meeting_summary::
//! resolve_endpoint`), both bounded to fit an 8k local model, and both
//! stepping aside for dictation (`meeting_summary::chat_beside_dictation`):
//!
//! - **"What did I miss?"** ([`meeting_catch_up`], the notepad's mini chat):
//!   what was said since the person last looked at the transcript. The
//!   notepad tracks what they've seen (the transcript on screen, or the last
//!   answer) and passes it as `since`; with nothing new it answers "Nothing
//!   new since you last looked." without calling a model. A question typed
//!   in the same chat is answered from the whole meeting so far.
//! - **The AI meeting title** ([`maybe_title`]): once there's enough talk
//!   (150 words, or the first digest) a made-up title like "Teams call · 5
//!   Oct, 14:30", or none, becomes a short one from what the meeting is about
//!   ("Q3 Budget Review with Alice"). A title the person typed is never
//!   replaced (`notes::set_ai_title` checks under the store lock).

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::meeting_summary as summary;

/// The answer when nothing was said since the person last looked.
pub const NOTHING_NEW: &str = "Nothing new since you last looked.";

/// Name the meeting while it records once it has this many words of speech
/// (about a minute of talk), or a digest…
pub const TITLE_WORDS: usize = 150;
/// …or when it ends, if it has at least this many (fewer: "Started by
/// mistake?" asks first).
pub const TITLE_END_WORDS: usize = crate::meeting_end::MISTAKE_WORDS;
/// A failed title is tried again after this long…
const TITLE_RETRY_MS: u64 = 2 * 60 * 1000;
/// …at most this many times per meeting (per run of Yap).
const TITLE_MAX_TRIES: u32 = 3;

#[derive(Default)]
struct TitleJob {
    tries: u32,
    /// Not before this time (ms) — after a failure.
    next_ms: u64,
    running: bool,
    /// Named, or the person titled it themselves.
    done: bool,
}

static TITLES: LazyLock<Mutex<HashMap<u64, TitleJob>>> = LazyLock::new(Default::default);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Whether a meeting is due its AI title: its title is open to it, it has
/// `words` of speech and `digests` digests, and it's still recording
/// (`at_end` false) or just ended.
pub fn title_due(open: bool, words: usize, digests: usize, at_end: bool) -> bool {
    open && if at_end {
        words >= TITLE_END_WORDS
    } else {
        words >= TITLE_WORDS || digests > 0
    }
}

/// New talk for meeting `note_id` (`at_end`: its recording just stopped):
/// give it an AI title in the background when it's due one. Cheap enough to
/// call on every segment.
pub fn maybe_title(app: &AppHandle, note_id: u64, at_end: bool) {
    {
        let titles = TITLES.lock().unwrap_or_else(|p| p.into_inner());
        if titles.get(&note_id).is_some_and(|t| t.done || t.running) {
            return;
        }
    }
    let Some((open, words, digests)) = crate::notes::meeting_brief(note_id) else {
        return;
    };
    {
        let mut titles = TITLES.lock().unwrap_or_else(|p| p.into_inner());
        let job = titles.entry(note_id).or_default();
        if !open {
            job.done = true;
            return;
        }
        let waiting = now_ms() < job.next_ms || job.tries >= TITLE_MAX_TRIES;
        if job.running || job.done || waiting || !title_due(open, words, digests, at_end) {
            return;
        }
        job.running = true;
        job.tries += 1;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = name_meeting(&app, note_id).await;
        let mut titles = TITLES.lock().unwrap_or_else(|p| p.into_inner());
        let job = titles.entry(note_id).or_default();
        job.running = false;
        match result {
            Ok(_) => job.done = true,
            Err(e) => {
                tracing::info!(note_id, "Meeting title not written ({e}); trying again later");
                job.next_ms = now_ms() + TITLE_RETRY_MS;
            }
        }
    });
}

/// Ask the meeting's model for a title and set it, unless the person titled
/// the note meanwhile. Returns whether it was set.
async fn name_meeting(app: &AppHandle, note_id: u64) -> Result<bool, String> {
    let ep = summary::resolve_endpoint(&crate::config::load())?;
    let note = crate::notes::get(note_id).ok_or("Note not found")?;
    if !crate::notes::title_open_to_ai(&note) {
        return Ok(false);
    }
    let opts = ep.options(summary::TITLE_REPLY_TOKENS, 0.2);
    let reply =
        summary::chat_beside_dictation(app, &ep, summary::title_messages(&note), &opts).await?;
    let title = summary::clean_title(&reply).ok_or("the reply wasn't a title")?;
    if !crate::notes::set_ai_title(note_id, &title)? {
        return Ok(false);
    }
    tracing::info!(note_id, "Meeting named by the AI");
    crate::commands::note_changed(app, note_id, "ai");
    let _ = app.emit("yap-notes-changed", ());
    Ok(true)
}

/// A catch-up answer: `upto` is how many transcript segments it covers (the
/// notepad counts them as seen).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatchUp {
    pub answer: String,
    pub nothing_new: bool,
    pub upto: usize,
}

/// "What did I miss?" for meeting `note_id`: what was said since transcript
/// segment `since` (where the person last looked). With `question`, a
/// follow-up asked in the same chat, answered from the meeting so far.
#[tauri::command]
pub async fn meeting_catch_up(
    app: AppHandle,
    note_id: u64,
    since: usize,
    question: Option<String>,
) -> Result<CatchUp, String> {
    let note = crate::notes::get(note_id).ok_or("Note not found")?;
    let upto = note.transcript.len();
    let question = question.map(|q| q.trim().to_string()).filter(|q| !q.is_empty());
    let messages = match &question {
        None => match summary::catch_up_input(&note, since) {
            Some(input) => summary::catch_up_messages(&input),
            None => {
                return Ok(CatchUp { answer: NOTHING_NEW.to_string(), nothing_new: true, upto });
            }
        },
        Some(q) => summary::meeting_ask_messages(&note, q),
    };
    let ep = summary::resolve_endpoint(&crate::config::load())?;
    let opts = ep.options(summary::CATCH_UP_REPLY_TOKENS, 0.3);
    let answer = summary::chat_beside_dictation(&app, &ep, messages, &opts).await?;
    let answer = answer.trim().to_string();
    if answer.is_empty() {
        return Err("The model returned an empty answer".to_string());
    }
    Ok(CatchUp { answer, nothing_new: false, upto })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_meeting_is_named_once_there_is_enough_talk() {
        // While recording: a minute of talk, or the first digest.
        assert!(!title_due(true, 40, 0, false));
        assert!(title_due(true, TITLE_WORDS, 0, false));
        assert!(title_due(true, 90, 1, false));
        // When it ends, a short meeting is named too…
        assert!(title_due(true, 60, 0, true));
        // …but not one "Started by mistake?" asks about.
        assert!(!title_due(true, TITLE_END_WORDS - 1, 0, true));
        // A title the person typed: never.
        assert!(!title_due(false, 5_000, 3, false));
        assert!(!title_due(false, 5_000, 3, true));
    }
}

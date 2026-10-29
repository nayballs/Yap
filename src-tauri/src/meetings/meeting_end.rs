//! The end of a meeting recording, wherever it was stopped from: the meeting
//! notepad's Stop, the Notes view's "End meeting & summarise", the Yap bar,
//! call detection's "Stop and summarise", or an automatic stop.
//!
//! - **End or pause.** Any stop is the end of the meeting, unless it came
//!   through [`meeting_pause`] (the Notes view's Pause: Resume carries on in
//!   the same note). This module follows the recorder through its
//!   `yap-meeting-state` events.
//! - **The refined transcript** (`meeting_refine.rs`): when one is due, it
//!   runs after the action plan — or before it, then [`end_steps`], when
//!   the live transcript has holes (chunks that failed).
//! - **"Started by mistake?"** A meeting with only a handful of words (fewer
//!   than [`MISTAKE_WORDS`]) and nothing typed gets no automatic summary:
//!   `yap-meeting-ended` asks the window the person is looking at to offer
//!   Keep / Discard ([`meeting_discard`] deletes the note), as Wispr Flow does,
//!   or, with no Yap window on screen, a card on the Yap bar asks.
//! - **The action plan**, written here in Rust once the last chunk is
//!   transcribed, so it doesn't depend on any window having the note open
//!   (until 2026-10 the Notes view wrote it, and only for the note it showed).
//!   Every window follows the job through `yap-meeting-summary` (`running`
//!   with "Step 2 of 3", then `done`, `error`, `needsAi` or `nothing`);
//!   [`meeting_summarise`] runs it again ("Generate summary", Retry).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Listener, Manager};

/// A meeting with fewer words of speech than this, and nothing typed, asks
/// "Started by mistake?" instead of writing a summary (about ten seconds of
/// talk; a real meeting has ~150 words a minute).
pub const MISTAKE_WORDS: usize = 20;
/// The action plan's progress: [`Summary`].
pub const EVENT_SUMMARY: &str = "yap-meeting-summary";
/// A meeting just ended (not paused): `{ noteId, mistake, words, surface }`,
/// `surface` = the window that asks "Started by mistake?" ("notepad" |
/// "settings" | null).
pub const EVENT_ENDED: &str = "yap-meeting-ended";
/// A meeting note was discarded: `{ id }`.
pub const EVENT_DELETED: &str = "yap-note-deleted";
/// A recording stopped and its place in the transcript was marked:
/// `{ noteId, breaks }` (`Note::breaks`, the notepad's paused dividers).
pub const EVENT_BREAKS: &str = "yap-meeting-breaks";

/// The action plan is written in this many steps (see `notes::commands::run_enhance`).
const STEPS: u8 = 3;

/// The next stop is a pause, not the end of the meeting.
static PAUSED: AtomicBool = AtomicBool::new(false);
/// The next stop is Yap's own (the length limit, the meeting going quiet,
/// the call ending with "Stop and summarise automatically", the PC going to
/// sleep), not the person's: call detection never offers auto-start after
/// one, and may carry the meeting on when its call comes back
/// (`meeting_detect::on_recording_finished`).
static AUTOMATIC: AtomicBool = AtomicBool::new(false);
/// The window the next stop came from (for where "Started by mistake?" shows).
static ORIGIN: Mutex<Option<String>> = Mutex::new(None);
/// The note whose recording the last `yap-meeting-state` started (0 = none).
static RECORDING: AtomicU64 = AtomicU64::new(0);
/// The Yap bar's "Started by mistake?" card, and the note it asks about
/// (0 = none up).
const MISTAKE_CARD: &str = "meeting-mistake";
static MISTAKE_NOTE: AtomicU64 = AtomicU64::new(0);
/// Each note's latest action-plan job (kept after it ends, so a window
/// opened later shows its result or its error).
static JOBS: LazyLock<Mutex<HashMap<u64, Summary>>> = LazyLock::new(Default::default);
static RUN_SEQ: AtomicU64 = AtomicU64::new(0);

/// One action-plan job, as every window renders it.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub note_id: u64,
    /// Which run this is (a window tells two runs' results apart by it).
    pub run: u64,
    /// "running" | "done" | "error" | "needsAi" | "nothing" | "kept" (the
    /// person's edited summary stays: `note_edits.rs`).
    pub state: &'static str,
    /// 1 reading the meeting, 2 writing, 3 checking (while running).
    pub step: u8,
    pub steps: u8,
    /// Why it failed, or what to set up ("needsAi").
    pub error: Option<String>,
}

fn jobs() -> std::sync::MutexGuard<'static, HashMap<u64, Summary>> {
    JOBS.lock().unwrap_or_else(|p| p.into_inner())
}

/// Follow the meeting recorder (app setup): its starts open the notepad, its
/// stops end the meeting, and its new segments may name it.
pub fn init(app: &AppHandle) {
    let handle = app.clone();
    app.listen("yap-meeting-state", move |event| on_state(&handle, event.payload()));
    let handle = app.clone();
    app.listen("yap-meeting-segment", move |_| {
        if let Some(note_id) = crate::meetings::meeting::recording_note() {
            crate::meetings::meeting_assist::maybe_title(&handle, note_id, false);
        }
    });
}

/// `yap-meeting-state`: `{ recording: true, noteId }` when a recording
/// starts, `{ recording: false }` once the last chunk of one is in. Runs on
/// the emitter's thread, so anything slow goes to a thread of its own.
fn on_state(app: &AppHandle, payload: &str) {
    let state: serde_json::Value = serde_json::from_str(payload).unwrap_or_default();
    if state["recording"].as_bool() == Some(true) {
        let note_id = state["noteId"].as_u64().unwrap_or(0);
        if note_id == 0 || RECORDING.swap(note_id, Ordering::SeqCst) == note_id {
            return;
        }
        PAUSED.store(false, Ordering::SeqCst);
        AUTOMATIC.store(false, Ordering::SeqCst);
        *ORIGIN.lock().unwrap_or_else(|p| p.into_inner()) = None;
        // Resuming makes an earlier summary's progress or error moot, and
        // answers "Started by mistake?".
        jobs().remove(&note_id);
        withdraw_mistake_card(app, note_id);
        crate::meetings::notepad::on_meeting_started(app, note_id);
        return;
    }
    let note_id = RECORDING.swap(0, Ordering::SeqCst);
    if note_id == 0 {
        return;
    }
    let paused = PAUSED.swap(false, Ordering::SeqCst);
    let automatic = AUTOMATIC.swap(false, Ordering::SeqCst);
    let origin = ORIGIN.lock().unwrap_or_else(|p| p.into_inner()).take();
    let app = app.clone();
    std::thread::spawn(move || {
        let mistake = finished(&app, note_id, paused, origin.as_deref());
        crate::calls::meeting_detect::on_recording_finished(&app, note_id, paused, automatic, mistake);
    });
}

/// Whether a meeting with `words` of speech and `typed` notes was most likely
/// started by mistake.
pub fn started_by_mistake(words: usize, typed: &str) -> bool {
    words < MISTAKE_WORDS && typed.trim().is_empty()
}

/// Where "Started by mistake?" shows: the window the stop came from when
/// it's on screen, else the notepad, else the main window (never a hidden
/// one: WebView2 reports a hidden window's page as visible, so Rust asks).
fn surface(app: &AppHandle, origin: Option<&str>) -> Option<&'static str> {
    let on_screen = |label: &str| {
        app.get_webview_window(label).is_some_and(|w| {
            w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false)
        })
    };
    let mut order: Vec<&'static str> = Vec::new();
    match origin {
        Some("notepad") => order.push("notepad"),
        Some("settings") => order.push("settings"),
        _ => {}
    }
    order.extend(["notepad", "settings"]);
    order.into_iter().find(|label| on_screen(label))
}

/// A recording of `note_id` just finished (its last chunk is in). Whether
/// it asked "Started by mistake?".
fn finished(app: &AppHandle, note_id: u64, paused: bool, origin: Option<&str>) -> bool {
    let Some(note) = crate::notes::get(note_id) else {
        return false; // deleted meanwhile
    };
    // Where this recording stopped: the notepad's paused divider.
    match crate::notes::add_break(note_id) {
        Ok(Some(breaks)) => {
            let _ = app.emit(EVENT_BREAKS, serde_json::json!({ "noteId": note_id, "breaks": breaks }));
        }
        Ok(None) => {}
        Err(e) => tracing::warn!(note_id, "Meeting stop not marked: {e}"),
    }
    if paused {
        tracing::info!(note_id, "Meeting paused");
        crate::meetings::meeting_assist::maybe_title(app, note_id, true);
        return false;
    }
    drop(note);
    // The refined transcript (`meeting_refine.rs`, "Which comes first"):
    // with holes in the live one (chunks failed), the refine first, then
    // these steps; else these steps now and the refine after the summary.
    let plan = crate::meetings::meeting_refine::plan(note_id);
    if let Some(p) = plan.as_ref().filter(|p| p.first) {
        if crate::meetings::meeting_refine::start(app, note_id, p, origin.map(str::to_string)) {
            return false;
        }
    }
    let mistake = end_steps(app, note_id, origin);
    match plan.filter(|p| !p.first && !mistake) {
        Some(p) => {
            crate::meetings::meeting_refine::start(app, note_id, &p, None);
        }
        None => crate::meetings::meeting_audio::release(app, note_id),
    }
    mistake
}

/// The end of a meeting from its transcript as it stands: "Started by
/// mistake?" (a handful of words, nothing typed, and no stretch that
/// couldn't be transcribed — that one may have been the meeting), or the
/// AI title and the action plan. After the refine when that came first
/// (`meeting_refine.rs`). Returns whether it asked "Started by mistake?".
pub fn end_steps(app: &AppHandle, note_id: u64, origin: Option<&str>) -> bool {
    let Some(note) = crate::notes::get(note_id) else {
        return false; // deleted meanwhile
    };
    let words = crate::notes::speech_words(&note.transcript);
    let failed = note.transcript.iter().any(|s| s.failed);
    let mistake = started_by_mistake(words, &note.content) && !failed;
    let surface = surface(app, origin);
    tracing::info!(note_id, words, mistake, failed, ?surface, "Meeting ended");
    let _ = app.emit(
        EVENT_ENDED,
        serde_json::json!({ "noteId": note_id, "mistake": mistake, "words": words, "surface": surface }),
    );
    if mistake {
        // No Yap window on screen to ask (stopped from the bar, or
        // automatically): the Yap bar asks.
        if surface.is_none() {
            ask_on_bar(app, note_id);
        }
        return true;
    }
    crate::meetings::meeting_assist::maybe_title(app, note_id, true);
    start_summary(app, note_id);
    false
}

/// The part of a meeting a crash left is transcribed now
/// (`meeting_audio.rs`): the end of the meeting, as a stop would have been —
/// its stop mark, "Started by mistake?" or the AI title and the action plan.
pub fn recovered(app: &AppHandle, note_id: u64) {
    let app = app.clone();
    std::thread::spawn(move || {
        finished(&app, note_id, false, None);
    });
}

/// "Started by mistake?" as a card on the Yap bar: Keep (or its ✕, or a
/// minute left alone) leaves the note, Discard deletes it. With the bar off
/// or hidden, nobody asks and the note stays, as Keep would leave it.
fn ask_on_bar(app: &AppHandle, note_id: u64) {
    let card = crate::bar::Card {
        id: MISTAKE_CARD.into(),
        icon: "notes",
        title: "Started by mistake?".into(),
        body: "Only a few words were captured. Keep this meeting or discard it.".into(),
        primary: Some(crate::bar::CardAction::new("keep", "Keep")),
        secondary: Some(crate::bar::CardAction::new("discard", "Discard")),
        timeout_ms: Some(60_000),
        expire_action: Some("keep".into()),
        close_action: Some("keep".into()),
        ..Default::default()
    };
    let answer = move |app: &AppHandle, action: &str| {
        MISTAKE_NOTE.store(0, Ordering::SeqCst);
        if action == "discard" {
            if let Err(e) = meeting_discard(app.clone(), note_id) {
                tracing::warn!(note_id, "Meeting not discarded: {e}");
            }
        }
    };
    MISTAKE_NOTE.store(note_id, Ordering::SeqCst);
    if crate::bar::show_card(app, card, Box::new(answer)) {
        tracing::info!(note_id, "Started by mistake? asked on the Yap bar");
    } else {
        MISTAKE_NOTE.store(0, Ordering::SeqCst);
    }
}

/// Take the bar's "Started by mistake?" down if it asks about `note_id`
/// (resumed, or discarded elsewhere).
fn withdraw_mistake_card(app: &AppHandle, note_id: u64) {
    if MISTAKE_NOTE.compare_exchange(note_id, 0, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
        crate::bar::dismiss_card(app, MISTAKE_CARD);
    }
}

/// End the meeting being recorded: stop it, and once its last chunk is in,
/// write the action plan (or ask "Started by mistake?"). `origin` is the
/// window that asked ("notepad", "settings", "overlay"…), where a question
/// about it should show if it's on screen. The Yap bar and other Rust code
/// call this; webviews use [`meeting_end`].
pub fn end(_app: &AppHandle, origin: Option<&str>) -> Result<(), String> {
    tracing::info!(?origin, "Meeting end asked for");
    *ORIGIN.lock().unwrap_or_else(|p| p.into_inner()) = origin.map(str::to_string);
    crate::meetings::meeting::stop().inspect_err(|_| {
        *ORIGIN.lock().unwrap_or_else(|p| p.into_inner()) = None;
    })
}

/// [`end`], as Yap's own stop rather than the person's (see
/// [`AUTOMATIC`]): the length limit, a quiet meeting, a call that ended.
pub fn end_automatically(app: &AppHandle) -> Result<(), String> {
    AUTOMATIC.store(true, Ordering::SeqCst);
    end(app, None).inspect_err(|_| AUTOMATIC.store(false, Ordering::SeqCst))
}

/// Pause the meeting: stop recording without writing the action plan
/// (Resume carries on in the same note).
pub fn pause() -> Result<(), String> {
    tracing::info!("Meeting pause asked for");
    PAUSED.store(true, Ordering::SeqCst);
    crate::meetings::meeting::stop().inspect_err(|_| PAUSED.store(false, Ordering::SeqCst))
}

/// [`pause`], as Yap's own (the PC going to sleep: `meetings/sleep.rs`).
pub fn pause_automatically() -> Result<(), String> {
    AUTOMATIC.store(true, Ordering::SeqCst);
    pause().inspect_err(|_| AUTOMATIC.store(false, Ordering::SeqCst))
}

/// Errors that mean "set up an AI model first", not a failure.
fn needs_ai(error: &str) -> bool {
    error.contains("No AI model configured") || error.contains("API key")
}

/// What the action plan has nothing to work with.
const NOTHING: &str = "Nothing to summarise yet: no speech was transcribed and nothing was typed.";

/// An action plan is being written (any note's): the on-device model's
/// runtime isn't swapped under it (`local_llm::runtime::upgrade_runtime`).
pub fn summary_running() -> bool {
    jobs().values().any(|j| j.state == "running")
}

/// Note `note_id`'s action plan is being written (a refine waits for it,
/// `meeting_audio.rs`).
pub fn summary_running_for(note_id: u64) -> bool {
    jobs().get(&note_id).is_some_and(|j| j.state == "running")
}

fn publish(app: &AppHandle, summary: Summary) {
    jobs().insert(summary.note_id, summary.clone());
    let _ = app.emit(EVENT_SUMMARY, summary);
}

/// Write note `note_id`'s action plan in the background (the built-in Action
/// Plan action, as "End meeting & summarise" always did), unless one is being
/// written already. Returns whether this started one. A summary the person
/// edited stays (the job ends "kept"; `note_edits.rs`).
pub fn start_summary(app: &AppHandle, note_id: u64) -> bool {
    start_summary_with(app, note_id, false)
}

/// [`start_summary`]; `replace`: the person said their edited summary may go.
fn start_summary_with(app: &AppHandle, note_id: u64, replace: bool) -> bool {
    let run = RUN_SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    let first = Summary {
        note_id,
        run,
        state: "running",
        step: 1,
        steps: STEPS,
        error: None,
    };
    {
        let mut all = jobs();
        if all.get(&note_id).is_some_and(|j| j.state == "running") {
            return false;
        }
        all.insert(note_id, first.clone());
    }
    let _ = app.emit(EVENT_SUMMARY, first);
    // From the lines as they are now: a refine's "Update summary" is answered.
    crate::meetings::meeting_audio::summary_redone(app, note_id);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let step = |n: u8| {
            let still_ours = jobs().get(&note_id).is_some_and(|j| j.run == run);
            if still_ours {
                publish(
                    &app,
                    Summary { note_id, run, state: "running", step: n, steps: STEPS, error: None },
                );
            }
        };
        let result = write_action_plan(&app, note_id, replace, step).await;
        let (state, error) = match result {
            Ok(()) => ("done", None),
            Err(e) if e == NOTHING => ("nothing", Some(e)),
            Err(e) if e == crate::notes::note_edits::SUMMARY_KEPT => ("kept", Some(e)),
            Err(e) if needs_ai(&e) => ("needsAi", Some(e)),
            Err(e) => ("error", Some(e)),
        };
        match &error {
            Some(_) if state == "kept" => tracing::info!(note_id, "Meeting action plan not written: the person's edited summary stays"),
            Some(e) => tracing::warn!(note_id, state, "Meeting action plan not written: {e}"),
            None => {
                tracing::info!(note_id, "Meeting action plan written");
                // "Learn from meetings" (memory.rs): who the attendees are,
                // the person's projects.
                crate::memory::learn::meeting_ended(note_id);
                summary_ready(&app, note_id);
            }
        }
        // A discard or a resume meanwhile retired this run.
        if jobs().get(&note_id).is_some_and(|j| j.run == run) {
            publish(&app, Summary { note_id, run, state, step: STEPS, steps: STEPS, error });
        }
    });
    true
}

/// "Your summary is ready"'s second line: the meeting and its tasks
/// ("“Supplier call” · 3 tasks"; "Your meeting" before it has a title).
/// Pure.
pub fn ready_body(title: &str, tasks: usize) -> String {
    let title = title.trim();
    let named = if title.is_empty() { "Your meeting".to_string() } else { format!("\u{201c}{title}\u{201d}") };
    match tasks {
        0 => format!("{named} · Open it to read it"),
        1 => format!("{named} · 1 task"),
        n => format!("{named} · {n} tasks"),
    }
}

/// **"Your summary is ready"** (2026-10-25, Wispr Flow's notice): an action
/// plan was written while no Yap window is in front (the meeting stopped
/// from the bar, by itself, or the person went back to their work) — a
/// quiet notice in one place, through the notice policy: a card on the Yap
/// bar, else a Windows notification (not over a dictation), with **Open
/// note** (the note in Notes). With the main window or the notepad in front
/// they show it themselves, so nothing. On a thread of its own (window
/// getters wait on the main thread).
fn summary_ready(app: &AppHandle, note_id: u64) {
    let app = app.clone();
    std::thread::spawn(move || {
        if crate::notices::notice::yap_in_front(&app) {
            return;
        }
        let Some(note) = crate::notes::get(note_id) else {
            return;
        };
        let tasks = crate::notes::tasks::parse(&note.enhanced_content).len();
        tracing::info!(note_id, tasks, "Your summary is ready: said with no Yap window in front");
        let notice = crate::calls::meeting_guard::Notice {
            kind: "summaryReady",
            title: "Your summary is ready".to_string(),
            body: ready_body(&note.title, tasks),
            icon: "notes",
            variant: "success",
            settings: None,
            note_id: Some(note_id),
            surface: None,
        };
        crate::calls::meeting_guard::notice_native(&app, &notice);
    });
}

/// The action plan for `note_id`, through the same path as the Notes view's
/// actions (`notes::commands::run_enhance`).
async fn write_action_plan(app: &AppHandle, note_id: u64, replace: bool, step: impl Fn(u8)) -> Result<(), String> {
    let note = crate::notes::get(note_id).ok_or("Note not found")?;
    if note.content.trim().is_empty() && crate::notes::speech_words(&note.transcript) == 0 {
        return Err(NOTHING.to_string());
    }
    // The person's edited summary stays, unless they said it may go.
    crate::notes::note_edits::keeps_summary(&note, replace)?;
    // Found by kind (renaming it is fine); older stores fall back to Meeting
    // Notes, as the Notes view does.
    // Who each voice is, before the plan names owners: the AI's answers
    // (checked by Rust) on top of what the transcript says (`naming.rs`).
    if crate::config::load().meeting_speaker_names {
        crate::speakers::naming::ask_model(app, note_id).await;
    }
    let actions = crate::notes::actions();
    let action = actions
        .iter()
        .find(|a| a.kind == crate::notes::ACTION_PLAN)
        .or_else(|| actions.iter().find(|a| a.name == "Meeting Notes"))
        .map(|a| a.id);
    match crate::notes::commands::run_enhance(app, note_id, action, replace, step).await {
        // The plan is written (in memory) but couldn't be saved: the job is
        // done, and the notice says the save failed, with Try again.
        Err(e) if e.starts_with("Couldn't save ") => {
            if let Err(save) = crate::notes::saved() {
                crate::notices::alert::show(app, crate::shell::saving::failed_alert(save, 0, false));
            }
            Ok(())
        }
        other => other.map(|_| ()),
    }
}

// ---- commands ------------------------------------------------------------------------------

/// End the meeting being recorded (see [`end`]); `origin` = the calling
/// window's label.
#[tauri::command]
pub fn meeting_end(app: AppHandle, origin: Option<String>) -> Result<(), String> {
    end(&app, origin.as_deref())
}

/// Pause the meeting being recorded (see [`pause`]).
#[tauri::command]
pub fn meeting_pause() -> Result<(), String> {
    pause()
}

/// Write (again) the action plan of a meeting that isn't recording:
/// "Generate summary", Retry, the Notes view's "Action plan", "New since
/// this summary · Update". Progress and the result arrive as
/// `yap-meeting-summary`. `replace`: the window asked the person, and their
/// edited summary may go.
#[tauri::command]
pub fn meeting_summarise(app: AppHandle, note_id: u64, replace: Option<bool>) -> Result<(), String> {
    crate::notes::get(note_id).ok_or("Note not found")?;
    if crate::meetings::meeting::recording_note() == Some(note_id) {
        return Err("Stop the recording first".to_string());
    }
    // The transcript is being completed first (`meeting_refine.rs`): the
    // summary follows by itself.
    if crate::meetings::meeting_audio::running_job(note_id).is_some_and(|j| j.then_summary) {
        return Ok(());
    }
    start_summary_with(&app, note_id, replace.unwrap_or(false));
    Ok(())
}

/// Note `note_id`'s latest action-plan job, if one ran since Yap started.
#[tauri::command]
pub fn meeting_summary_status(note_id: u64) -> Option<Summary> {
    jobs().get(&note_id).cloned()
}

/// "Started by mistake?" → Discard: delete the meeting note (never one that's
/// still recording). Every window lets go of it (`yap-note-deleted`), and the
/// notepad closes if it showed it.
#[tauri::command]
pub fn meeting_discard(app: AppHandle, note_id: u64) -> Result<(), String> {
    crate::notes::get(note_id).ok_or("Note not found")?;
    crate::meetings::meeting::delete_note(note_id)?;
    jobs().remove(&note_id);
    withdraw_mistake_card(&app, note_id);
    tracing::info!(note_id, "Meeting note deleted");
    let _ = app.emit("yap-notes-changed", ());
    let _ = app.emit(EVENT_DELETED, serde_json::json!({ "id": note_id }));
    crate::meetings::notepad::on_note_deleted(&app, note_id);
    Ok(())
}

/// The notepad's ⋯ → Delete: delete a meeting note, stopping its recording
/// first when it's the one going (as a pause: no action plan for a note
/// that's going away), then as [`meeting_discard`].
#[tauri::command]
pub async fn meeting_delete(app: AppHandle, note_id: u64) -> Result<(), String> {
    if crate::meetings::meeting::recording_note() == Some(note_id) {
        pause()?;
        // The recorder transcribes its last few seconds before it lets go.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while crate::meetings::meeting::is_recording() {
            if std::time::Instant::now() > deadline {
                return Err("The recording didn't stop, so the note was kept".to_string());
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    }
    meeting_discard(app, note_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_handful_of_words_with_nothing_typed_is_a_mistake() {
        assert!(started_by_mistake(0, ""));
        assert!(started_by_mistake(2, "  \n")); // "Hi, Notetaker."
        assert!(started_by_mistake(MISTAKE_WORDS - 1, ""));
        assert!(!started_by_mistake(MISTAKE_WORDS, ""));
        // Notes typed in My thoughts: never a mistake, whatever was said.
        assert!(!started_by_mistake(0, "Ask about the budget"));
    }

    #[test]
    fn the_ready_notice_names_the_meeting_and_its_tasks() {
        assert_eq!(ready_body("Supplier call", 3), "\u{201c}Supplier call\u{201d} · 3 tasks");
        assert_eq!(ready_body(" Sync ", 1), "\u{201c}Sync\u{201d} · 1 task");
        assert_eq!(ready_body("", 0), "Your meeting · Open it to read it");
    }

    #[test]
    fn setup_errors_are_not_failures() {
        assert!(needs_ai(
            "No AI model configured — set one in Settings → Language Models → Note Formatting"
        ));
        assert!(needs_ai("No groq API key — add one in Settings → Language Models"));
        assert!(!needs_ai("HTTP 500: boom"));
    }

    #[test]
    fn a_job_reads_as_the_windows_expect() {
        let s = Summary { note_id: 7, run: 2, state: "running", step: 2, steps: STEPS, error: None };
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["noteId"], 7);
        assert_eq!(v["state"], "running");
        assert_eq!(v["step"], 2);
        assert_eq!(v["steps"], 3);
        assert!(v["error"].is_null());
    }
}

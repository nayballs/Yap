//! Meeting summaries that keep up with long meetings.
//!
//! One end-of-meeting LLM call over the whole transcript (what the Meeting
//! Notes action used to do) overflows the on-device model's 8k context after
//! ~25–30 minutes of talk, and is slow and costly on cloud models even when it
//! fits. Instead, while a meeting records, every ~10 minutes of new talk
//! becomes a **digest** — key points, decisions, action items (owner and
//! deadline only as said) and open questions — written in the background and
//! persisted on the note (`Note::digests`), so a crash or restart keeps them.
//! At the end, the action plan merges the digests plus the not-yet-digested
//! tail in ONE call whose input is bounded whatever the meeting length, so it
//! comes back in seconds. (Map-reduce with the map done as the meeting goes;
//! each digest gets the main points so far, so the parts read as one meeting.)
//!
//! Budgets, in estimated tokens ([`estimate_tokens`] runs high on purpose).
//! Every call fits an 8k-context local model with room to spare:
//! - a digest reads ≤ 2,600 of transcript + ≤ 400 of context + ~700 of
//!   prompt, and replies in ≤ 700: ~4.4k;
//! - the final call reads ≤ 4,500 (≤ 600 typed notes, ≤ 1,800 raw tail, the
//!   rest digests, trimmed evenly when a very long meeting needs it) + ~550
//!   of prompt, and replies in ≤ 1,500: ~6.6k.
//!
//! A meeting whose transcript + notes fit in 3,500 keeps the single pass
//! over the raw transcript (nothing lost to digesting).
//!
//! A bigger on-device context (Settings' "Context", `local_context`) lets
//! the final call read more ([`FinalBudget`]): the single pass and the
//! final input grow by whole multiples of 8K (`local_llm::context::budget_factor`,
//! ≤ 4×: 14,000 / 18,000 at 32K), the reply's room staying as at 8K. Only
//! when the call really goes to the running sidecar; the live digests,
//! the helpers and every other endpoint keep the 8K budgets.
//!
//! Never slows dictation: on a local model (llamafile serves one request at a
//! time) a digest waits until no dictation is recording or processing, and an
//! in-flight one is dropped the moment one starts — closing the connection,
//! which llama.cpp's server treats as a cancel — then retried. Replies to
//! local models are capped (`max_tokens`).
//!
//! Never invents owners or dates: digests and the action plan go through
//! deterministic checks. An owner must be "You", "Everyone", an attendee, a
//! name in the meeting's speaker map (`speakers.rs`), or a name that was
//! actually said, else the task is Unassigned; a deadline's
//! words must appear in what the model read, else it's dropped; and the
//! action plan gets back any digest task the final merge left out
//! (summaries of summaries are where tasks get lost).
//!
//! Here: the budgets, running the digests and preparing the final call.
//! Its parts: `digest` (one digest: which stretch, the prompt, the parse,
//! the checks), `action_plan` (the final call's input and the checks on the
//! plan) and `helpers` (the notepad's helpers' inputs).

mod action_plan;
mod digest;
mod helpers;
pub mod language;

use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

use crate::config::YapConfig;
use crate::ai::llm::ChatOptions;
use crate::notes::{MeetingDigest, Note, TranscriptSegment};
use crate::speakers::Speakers;

pub use action_plan::{ask_context, digest_actions, final_request, postcheck_action_plan, render_digests};
// The model evaluations' "before" (the input without the language line).
#[cfg(debug_assertions)]
pub use action_plan::compose_meeting_input_within;
pub use helpers::{
    catch_up_input, catch_up_messages, clean_email, clean_title, email_subject, helper_messages, meeting_ask_messages,
    questions_input, recap_input, title_messages,
    BLIND_SPOTS_REPLY_TOKENS, CATCH_UP_REPLY_TOKENS, EMAIL_BESIDE_SUMMARY_TOKENS, FOLLOW_UP_REPLY_TOKENS, GAPS_BESIDE_SUMMARY_TOKENS,
    QUESTIONS_REPLY_TOKENS, TITLE_REPLY_TOKENS,
};
pub(crate) use helpers::{chat_beside_dictation, chat_beside_dictation_live, chat_beside_dictation_with};

use digest::{check_digest, context_so_far, digest_messages, next_window, parse_digest, remaining_windows, Plan};

/// A digest is due once the new talk spans this much meeting time…
pub const DIGEST_EVERY_SECS: u64 = 10 * 60;
/// …or reaches this many tokens (~10 minutes of steady talk at ~150 wpm).
pub const DIGEST_TARGET_TOKENS: usize = 2_000;
/// The time trigger waits for at least this much talk (a quiet stretch isn't
/// worth a call).
pub const DIGEST_MIN_TOKENS: usize = 250;
/// Most transcript one digest reads; a backlog is split into calls this size.
pub const DIGEST_MAX_TOKENS: usize = 2_600;
/// "The meeting so far" handed to each digest for continuity.
const CONTEXT_TOKENS: usize = 400;
/// Reply cap for a digest (local models only, see `ChatOptions::max_tokens`).
const DIGEST_REPLY_TOKENS: u32 = 700;
/// Transcript + typed notes up to this size: one pass over the raw text.
pub const SINGLE_PASS_TOKENS: usize = 3_500;
/// Input budget of the final call when digests are used…
pub const FINAL_INPUT_TOKENS: usize = 4_500;
/// …of which at most this much is raw transcript after the last digest…
pub const FINAL_TAIL_TOKENS: usize = 1_800;
/// …and at most this much the user's typed notes.
const FINAL_NOTES_TOKENS: usize = 600;
/// Reply cap for the final call (local models only).
const FINAL_REPLY_TOKENS: u32 = 1_500;
/// After a dictation, wait this long before a digest takes the local model,
/// so a quick follow-up dictation doesn't queue behind it either.
const IDLE_GRACE: Duration = Duration::from_secs(2);
/// After a failed digest, live digests rest this long (a bad key or a rate
/// limit shouldn't be hammered every chunk).
const FAILURE_COOLDOWN_MS: u64 = 2 * 60 * 1000;

/// Owner of a task nobody took on.
pub const UNASSIGNED: &str = "Unassigned";

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Rough token count for budgeting, deliberately on the high side: 3.5
/// characters per token for ASCII text (English runs ~4) and one per
/// non-ASCII character (CJK runs ~1, accented Latin far less).
pub fn estimate_tokens(s: &str) -> usize {
    let (mut ascii, mut other) = (0usize, 0usize);
    for c in s.chars() {
        if c.is_ascii() {
            ascii += 1;
        } else {
            other += 1;
        }
    }
    (ascii * 2).div_ceil(7) + other
}

/// The first `budget` tokens of `s` (cut at a line, else a char boundary).
fn truncate_to_tokens(s: &str, budget: usize) -> String {
    if estimate_tokens(s) <= budget {
        return s.to_string();
    }
    let mut out = String::new();
    for line in s.lines() {
        if estimate_tokens(&out) + estimate_tokens(line) + 1 > budget {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    if out.is_empty() {
        out = s.chars().take(budget * 3).collect();
    }
    format!("{}\n(…)", out.trim_end())
}

/// The last `budget` tokens of `s`, whole lines only.
fn keep_last_tokens(s: &str, budget: usize) -> String {
    if estimate_tokens(s) <= budget {
        return s.to_string();
    }
    let mut kept: Vec<&str> = Vec::new();
    let mut used = 0;
    for line in s.lines().rev() {
        let t = estimate_tokens(line) + 1;
        if used + t > budget {
            break;
        }
        used += t;
        kept.push(line);
    }
    kept.reverse();
    format!("(…earlier lines left out)\n{}\n", kept.join("\n"))
}

/// Transcript lines as the models read them, each under its speaker's name
/// ("You: …", "Them: …", or "Priya: …" / "Speaker 2: …" once the speaker map
/// knows them: `speakers::prompt_label`), without the echo segments or the
/// "you dictated here" markers.
pub fn transcript_lines(speakers: &Speakers, segs: &[TranscriptSegment]) -> String {
    segs.iter()
        .filter(|s| s.is_talk())
        .map(|s| format!("{}: {}\n", crate::speakers::prompt_label(speakers, s), s.text.trim()))
        .collect()
}

fn seg_tokens(seg: &TranscriptSegment) -> usize {
    if seg.is_talk() {
        estimate_tokens(&seg.text) + 2
    } else {
        0
    }
}

/// "m:ss", or "h:mm:ss" past the hour.
pub fn clock(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// The meeting's zero: the first segment's capture time.
fn meeting_t0(note: &Note) -> u64 {
    note.transcript.first().map(|s| s.ts).unwrap_or(0)
}

fn attendees_line(attendees: &[String]) -> String {
    let named: Vec<&str> = attendees
        .iter()
        .map(|a| a.trim())
        .filter(|a| !a.is_empty())
        .collect();
    if named.is_empty() {
        "(not given)".to_string()
    } else {
        named.join(", ")
    }
}

/// The people part of every meeting call's input: "Attendees: …", then
/// "Speakers besides You: …" when the transcript names anyone or tells
/// voices apart (`speakers::speakers_line`; a guess is said to be one).
fn people_lines(note: &Note) -> String {
    let mut out = format!("Attendees: {}\n", attendees_line(&note.participants));
    if let Some(line) = crate::speakers::speakers_line(note) {
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// [`fits_single_pass_within`] the 8K budget (the tests' shorthand).
#[cfg(test)]
pub fn fits_single_pass(note: &Note) -> bool {
    fits_single_pass_within(note, SINGLE_PASS_TOKENS)
}

/// Whether the whole raw transcript (and typed notes) fit one pass of
/// `budget` tokens ([`FinalBudget::single_pass`]).
pub fn fits_single_pass_within(note: &Note, budget: usize) -> bool {
    estimate_tokens(&transcript_lines(&note.speakers, &note.transcript)) + estimate_tokens(note.content.trim())
        <= budget
}

/// The final call's budgets: the single pass and the input, at 8K
/// ([`SINGLE_PASS_TOKENS`], [`FINAL_INPUT_TOKENS`]) or grown with a bigger
/// on-device context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FinalBudget {
    pub single_pass: usize,
    pub input: usize,
}

impl FinalBudget {
    #[cfg(test)]
    pub const DEFAULT: FinalBudget = FinalBudget { single_pass: SINGLE_PASS_TOKENS, input: FINAL_INPUT_TOKENS };

    /// The 8K budgets × `factor` (1–4). Pure.
    pub fn scaled(factor: usize) -> Self {
        let f = factor.clamp(1, crate::ai::local_llm::context::MAX_BUDGET_FACTOR);
        FinalBudget { single_pass: SINGLE_PASS_TOKENS * f, input: FINAL_INPUT_TOKENS * f }
    }

    /// For a call to `ep`: grown only on the running on-device model.
    pub fn for_endpoint(ep: &Endpoint) -> Self {
        Self::scaled(crate::ai::local_llm::context::budget_factor_for(&ep.provider, &ep.base_url))
    }
}

// ---- Running it ----

/// Where note-formatting calls go: the **Note Formatting** scope when it's
/// on, else the global AI-cleanup endpoint (OpenWhispr `fallbackScope:
/// dictationCleanup`). `fragment` is the scope's editable prompt.
pub struct Endpoint {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub provider: String,
    pub fragment: String,
    pub disable_thinking: bool,
}

/// The note-formatting endpoint, or the error that says how to set one up.
pub fn resolve_endpoint(cfg: &YapConfig) -> Result<Endpoint, String> {
    let ep = match cfg.llm_scopes.get("noteFormatting") {
        Some(s) if s.enabled && !s.provider.is_empty() => {
            let key = cfg.provider_api_key(&s.provider, &s.api_key);
            let (base_url, api_key, model, provider) =
                crate::ai::local_llm::effective_endpoint_for(&s.provider, &s.base_url, &key, &s.model);
            let fragment = if s.prompt.trim().is_empty() {
                crate::ai::llm::NOTE_DEFAULT_FRAGMENT.to_string()
            } else {
                s.prompt.clone()
            };
            Endpoint {
                base_url,
                api_key,
                model,
                provider,
                fragment,
                disable_thinking: s.disable_thinking,
            }
        }
        scope => {
            let (base_url, api_key, model, provider) = crate::ai::local_llm::effective_endpoint(cfg);
            let fragment = scope
                .map(|s| s.prompt.clone())
                .filter(|p| !p.trim().is_empty())
                .unwrap_or_else(|| crate::ai::llm::NOTE_DEFAULT_FRAGMENT.to_string());
            Endpoint {
                base_url,
                api_key,
                model,
                provider,
                fragment,
                disable_thinking: cfg.pp_disable_thinking,
            }
        }
    };
    if ep.base_url.is_empty() {
        return Err(
            "No AI model configured — set one in Settings → Language Models → Note Formatting"
                .to_string(),
        );
    }
    const KEYED_PROVIDERS: [&str; 5] = ["groq", "anthropic", "openai", "gemini", "openrouter"];
    if ep.api_key.is_empty() && KEYED_PROVIDERS.contains(&ep.provider.as_str()) {
        return Err(format!(
            "No {} API key — add one in Settings → Language Models",
            ep.provider
        ));
    }
    Ok(ep)
}

impl Endpoint {
    /// A server on this machine (the on-device sidecar, Ollama, LM Studio…).
    /// These serve one request at a time, so meeting calls cap their replies
    /// and get out of a dictation's way.
    pub fn is_local(&self) -> bool {
        if self.provider == crate::ai::local_llm::PROVIDER_ONDEVICE || self.provider == "local" {
            return true;
        }
        url::Url::parse(&self.base_url)
            .ok()
            .and_then(|u| {
                u.host_str().map(|h| {
                    matches!(h, "localhost" | "127.0.0.1" | "[::1]" | "::1" | "0.0.0.0")
                        || h.ends_with(".localhost")
                })
            })
            .unwrap_or(false)
    }

    pub fn options(&self, reply_cap: u32, temperature: f32) -> ChatOptions {
        let local = self.is_local();
        ChatOptions {
            temperature,
            max_tokens: local.then_some(reply_cap),
            // A 1.5B model on a CPU writes ~10–20 tokens a second.
            timeout: Duration::from_secs(if local { 240 } else { 120 }),
            disable_thinking: self.disable_thinking,
        }
    }

    /// Options for the final call (the action plan / meeting notes).
    pub fn final_options(&self) -> ChatOptions {
        self.options(FINAL_REPLY_TOKENS, 0.3)
    }
}

/// A dictation is recording or transcribing / cleaning up (or an Upload is
/// transcribing): the moments a local model must be free (memory's passes
/// keep to it too, `memory/learn.rs`).
pub(crate) fn dictation_busy(app: &AppHandle) -> bool {
    app.try_state::<crate::AppState>()
        .and_then(|s| {
            s.pipeline
                .lock()
                .ok()
                .map(|g| g.as_ref().is_some_and(|p| p.is_busy()))
        })
        .unwrap_or(false)
}

/// Run `call`, giving up (`None`) as soon as `busy()` turns true (checked
/// every 100 ms). Dropping a reqwest future closes its connection.
pub async fn unless_busy<T>(call: impl Future<Output = T>, busy: impl Fn() -> bool) -> Option<T> {
    let watch = async {
        loop {
            if busy() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    };
    tokio::select! {
        out = call => Some(out),
        _ = watch => None,
    }
}

/// Wait until `busy()` is false — plus [`IDLE_GRACE`] if it was busy — or
/// give up (`false`) once `wanted()` is false.
pub(crate) async fn wait_idle(busy: impl Fn() -> bool, wanted: impl Fn() -> bool) -> bool {
    let mut was_busy = false;
    loop {
        if !wanted() {
            return false;
        }
        if busy() {
            was_busy = true;
            tokio::time::sleep(Duration::from_millis(200)).await;
        } else if was_busy {
            was_busy = false;
            tokio::time::sleep(IDLE_GRACE).await;
        } else {
            return true;
        }
    }
}

enum DigestError {
    /// A dictation started; the digest was dropped to free the local model.
    Yielded,
    Failed(String),
}

/// Digest `transcript[from..to]` of `note` (not stored: the caller does).
async fn digest_window(
    app: &AppHandle,
    note: &Note,
    from: usize,
    to: usize,
    ep: &Endpoint,
    yield_to_dictation: bool,
) -> Result<MeetingDigest, DigestError> {
    let segs = &note.transcript[from..to];
    let (first, last) = match (segs.first(), segs.last()) {
        (Some(f), Some(l)) => (f, l),
        _ => return Err(DigestError::Failed("empty window".to_string())),
    };
    let t0 = meeting_t0(note);
    let lines = transcript_lines(&note.speakers, segs);
    let range = format!(
        "{}\u{2013}{}",
        clock(first.ts.saturating_sub(t0)),
        clock(last.ts.saturating_sub(t0))
    );
    let context = context_so_far(&note.digests, t0);
    // In the meeting's language, its headings in English (`language.rs`).
    let line = language::digest_line(&language::of_note(note));
    let messages = digest_messages(&people_lines(note), &context, note.digests.len() + 1, &range, &lines, line.as_deref());
    let opts = ep.options(DIGEST_REPLY_TOKENS, 0.2);
    let call = crate::ai::llm::chat(
        &ep.base_url,
        &ep.api_key,
        &ep.model,
        &ep.provider,
        messages,
        &opts,
    );
    // Recorded as Meetings' in the network receipts (net.rs).
    let call = crate::privacy::net::within(crate::privacy::net::Feature::Meetings, call);
    let reply = if yield_to_dictation {
        unless_busy(call, || dictation_busy(app))
            .await
            .ok_or(DigestError::Yielded)?
    } else {
        call.await
    }
    .map_err(DigestError::Failed)?;
    if reply.trim().is_empty() {
        return Err(DigestError::Failed(
            "the model returned an empty digest".to_string(),
        ));
    }
    let parsed = check_digest(parse_digest(&reply), &crate::speakers::known_people(note), &lines);
    let raw = if parsed.recognized {
        String::new()
    } else {
        reply.trim().to_string()
    };
    Ok(MeetingDigest {
        from_seg: from,
        to_seg: to,
        start_ts: first.ts,
        end_ts: last.ts,
        key_points: parsed.key_points,
        decisions: parsed.decisions,
        actions: parsed.actions,
        questions: parsed.questions,
        raw,
    })
}

/// The side and end of `note`'s line `to - 1`: where a digest reading up to
/// `to` stops, found again after the voice separation split lines before it
/// (`notes::add_digest`).
fn digest_end(note: &Note, to: usize) -> Option<(String, u64)> {
    let last = note.transcript.get(to.checked_sub(1)?)?;
    Some((last.source.clone(), last.end_ms?))
}

fn store_digest(app: &AppHandle, note_id: u64, digest: MeetingDigest, last: Option<(String, u64)>) -> Result<(), String> {
    let last = last.as_ref().map(|(s, e)| (s.as_str(), *e));
    let count = crate::notes::add_digest(note_id, digest.clone(), last)?;
    tracing::info!(
        note_id,
        part = count,
        segments = digest.to_seg - digest.from_seg,
        tasks = digest.actions.len(),
        "Meeting digest written"
    );
    let _ = app.emit(
        "yap-meeting-digest",
        json!({ "noteId": note_id, "count": count, "digest": digest }),
    );
    Ok(())
}

/// One digest at a time (live or final), so the end-of-meeting step waits for
/// an in-flight live digest instead of writing the same one again.
static DIGEST_TURN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static LIVE_RUNNING: AtomicBool = AtomicBool::new(false);
/// The latest kick's note (0 = none since the worker last looked).
static KICKED: AtomicU64 = AtomicU64::new(0);
static COOLDOWN_UNTIL_MS: AtomicU64 = AtomicU64::new(0);

/// New segments arrived for the meeting recording into `note_id`: write any
/// digest that's now due, in the background. One worker at a time; it keeps
/// going while digests are due (so a backlog catches up), and goes round
/// again for a kick that came in while it was finishing.
pub fn kick(app: &AppHandle, note_id: u64) {
    if now_ms() < COOLDOWN_UNTIL_MS.load(Ordering::SeqCst) {
        return;
    }
    KICKED.store(note_id, Ordering::SeqCst);
    if LIVE_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let note_id = KICKED.swap(0, Ordering::SeqCst);
            if note_id != 0 {
                live_digests(&app, note_id).await;
            }
            LIVE_RUNNING.store(false, Ordering::SeqCst);
            // A kick between the swap above and here found the worker still
            // running: take it (unless a new worker already has).
            if KICKED.load(Ordering::SeqCst) == 0 || LIVE_RUNNING.swap(true, Ordering::SeqCst) {
                break;
            }
        }
    });
}

async fn live_digests(app: &AppHandle, note_id: u64) {
    let recording = || crate::meetings::meeting::recording_note() == Some(note_id);
    loop {
        if !recording() {
            return;
        }
        let due = crate::notes::get(note_id).is_some_and(|n| {
            next_window(&n.transcript, crate::notes::digested_upto(&n), Plan::Live).is_some()
        });
        if !due {
            return;
        }
        // No AI set up: nothing to do now; the end-of-meeting step says how.
        let Ok(ep) = resolve_endpoint(&crate::config::load()) else {
            return;
        };
        let local = ep.is_local();
        if local && !wait_idle(|| dictation_busy(app), recording).await {
            return;
        }
        let _turn = DIGEST_TURN.lock().await;
        let _sidecar = crate::ai::local_llm::hold(&ep.base_url);
        // Re-plan under the turn: the end-of-meeting step may have run.
        let Some(note) = crate::notes::get(note_id) else {
            return;
        };
        let Some((from, to)) = next_window(
            &note.transcript,
            crate::notes::digested_upto(&note),
            Plan::Live,
        ) else {
            return;
        };
        match digest_window(app, &note, from, to, &ep, local).await {
            Ok(digest) => {
                if let Err(e) = store_digest(app, note_id, digest, digest_end(&note, to)) {
                    tracing::warn!("Meeting digest not stored: {}", e);
                    return;
                }
            }
            Err(DigestError::Yielded) => {
                tracing::info!("Meeting digest set aside for a dictation; retrying after it");
            }
            Err(DigestError::Failed(e)) => {
                tracing::warn!(
                    "Meeting digest failed (retrying in a couple of minutes): {}",
                    e
                );
                COOLDOWN_UNTIL_MS.store(now_ms() + FAILURE_COOLDOWN_MS, Ordering::SeqCst);
                return;
            }
        }
    }
}

/// Before an action runs on a meeting note: digest what the final call can't
/// take raw (a backlog, or a note recorded before digests existed), with
/// `yap-meeting-summary-progress` events. Waits for an in-flight live digest
/// rather than duplicating it. Returns the up-to-date note.
pub async fn prepare_final(
    app: &AppHandle,
    note_id: u64,
    ep: &Endpoint,
    budget: FinalBudget,
) -> Result<Note, String> {
    let _turn = DIGEST_TURN.lock().await;
    // The on-device model is busy between the digests too.
    let _sidecar = crate::ai::local_llm::hold(&ep.base_url);
    let mut done = 0;
    loop {
        let note = crate::notes::get(note_id).ok_or("Note not found")?;
        if fits_single_pass_within(&note, budget.single_pass) {
            return Ok(note);
        }
        let upto = crate::notes::digested_upto(&note);
        let keep = FINAL_TAIL_TOKENS;
        let Some((from, to)) = next_window(&note.transcript, upto, Plan::Final { keep }) else {
            return Ok(note);
        };
        let total = done + remaining_windows(&note.transcript, upto, keep);
        let _ = app.emit(
            "yap-meeting-summary-progress",
            json!({ "noteId": note_id, "done": done, "total": total }),
        );
        let digest = match digest_window(app, &note, from, to, ep, false).await {
            Ok(d) => d,
            Err(DigestError::Failed(e)) => {
                return Err(format!(
                    "Couldn't summarise part {} of the meeting: {e}",
                    note.digests.len() + 1
                ))
            }
            Err(DigestError::Yielded) => return Err("Summary interrupted".to_string()),
        };
        store_digest(app, note_id, digest, digest_end(&note, to))?;
        done += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::action_plan::compose_meeting_input;
    use super::digest::resolve_owner;
    use super::*;
    use crate::notes::DigestAction;
    use serde_json::Value;

    pub(super) fn seg(source: &str, text: &str, ts: u64) -> TranscriptSegment {
        TranscriptSegment {
            source: source.to_string(),
            text: text.to_string(),
            ts,
            ..Default::default()
        }
    }

    /// `n` segments of ~`words` words each, `every` seconds apart from t=1000
    /// (each starts with its own "sN" so no two are alike).
    pub(super) fn talk(n: usize, words: usize, every: u64) -> Vec<TranscriptSegment> {
        (0..n)
            .map(|i| {
                let mut text = format!("s{i}");
                for w in 1..words {
                    text.push_str(&format!(" word{}", (i + w) % 97));
                }
                seg(
                    if i % 2 == 0 { "you" } else { "them" },
                    &text,
                    1000 + i as u64 * every,
                )
            })
            .collect()
    }

    pub(super) fn note_with(transcript: Vec<TranscriptSegment>) -> Note {
        let mut note: Note = serde_json::from_value(json!({
            "id": 1, "title": "Planning", "content": "", "createdTs": 1, "updatedTs": 1,
            "participants": ["Alice", "Bob Stone"],
        }))
        .unwrap();
        note.note_type = "meeting".to_string();
        note.transcript = transcript;
        note
    }

    #[test]
    fn token_estimate_runs_high() {
        // 35 ASCII chars → 10 tokens; a CJK char counts as one.
        assert_eq!(estimate_tokens(&"a".repeat(35)), 10);
        assert_eq!(estimate_tokens("会議"), 2);
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn clock_formats() {
        assert_eq!(clock(5), "0:05");
        assert_eq!(clock(605), "10:05");
        assert_eq!(clock(3725), "1:02:05");
    }

    #[test]
    fn named_speakers_reach_every_input_and_count_as_known_people() {
        let mut note = note_with(vec![
            seg("you", "Can you send the deck by Friday?", 1000),
            seg("them", "Sure, I will send it.", 1010),
            seg("them", "And I will book the room for the offsite next week.", 1020),
        ]);
        note.participants.clear();
        // The call's voice a calendar guess; a second voice told apart, then
        // named by the user.
        note.transcript[2].speaker = "them-2".into();
        crate::speakers::calendar_guess(&mut note, &[("Priya Shah".into(), String::new())]);
        crate::speakers::rename(&mut note, "them-2", "Tom").unwrap();
        let lines = transcript_lines(&note.speakers, &note.transcript);
        assert_eq!(
            lines,
            "You: Can you send the deck by Friday?\nPriya Shah: Sure, I will send it.\nTom: And I will book the room for the offsite next week.\n"
        );
        let header = "Speakers besides You: Priya Shah (a guess: the only other person invited); Tom";
        let input = compose_meeting_input(&note).text;
        assert!(input.contains(header) && input.contains("\nTom: And I will book"), "{input}");
        assert!(catch_up_input(&note, 0).unwrap().contains(header));
        assert!(user_turn(&title_messages(&note)).contains("Priya Shah: Sure"));
        assert!(user_turn(&meeting_ask_messages(&note, "Who books the room?")).contains("Tom: And"));
        assert!(ask_context(&note, 3_000).contains("Priya Shah: Sure"));
        assert!(recap_input(&note, 500).unwrap().contains(header));
        // A name in the speaker map is a known person, said or not.
        let known = crate::speakers::known_people(&note);
        assert_eq!(resolve_owner("Tom", &known, "nothing said"), "Tom");
        assert_eq!(resolve_owner("priya", &known, ""), "Priya Shah");
        assert_eq!(resolve_owner("Mallory", &known, &lines), UNASSIGNED);
        let plan = postcheck_action_plan("## Action plan\n### Tom\n- [ ] book the room", &known, "", &[]);
        assert!(plan.contains("### Tom\n- [ ] book the room"), "{plan}");
        // Just You and Them: no speakers line, the labels as they were.
        let plain = note_with(vec![seg("you", "hi", 1), seg("them", "hello", 2)]);
        assert!(!compose_meeting_input(&plain).text.contains("Speakers"));
        assert_eq!(transcript_lines(&plain.speakers, &plain.transcript), "You: hi\nThem: hello\n");
    }

    #[test]
    fn echo_segments_are_left_out() {
        let mut t = talk(4, 10, 15);
        t[1].echo = true;
        let lines = transcript_lines(&Speakers::new(), &t);
        assert_eq!(lines.lines().count(), 3);
        assert_eq!(seg_tokens(&t[1]), 0);
    }

    #[test]
    fn dictation_markers_are_left_out_of_every_input() {
        // Ten minutes of talk with a dictation marker in the middle, and one
        // as the very last segment (the tail the final call reads raw).
        let mut t = talk(40, 60, 15);
        t.insert(20, TranscriptSegment::dictation_marker(1000 + 20 * 15));
        t.push(TranscriptSegment::dictation_marker(1000 + 41 * 15));
        assert_eq!(seg_tokens(&t[20]), 0);
        let lines = transcript_lines(&Speakers::new(), &t);
        assert_eq!(lines.lines().count(), 40);
        assert!(!lines.contains("You: \n"), "no empty line for a marker");
        // Digest windows, the final input and the Ask context never show it.
        let (from, to) = next_window(&t, 0, Plan::Live).unwrap();
        assert!(!transcript_lines(&Speakers::new(), &t[from..to]).contains("You: \n"));
        let note = note_with(t.clone());
        let input = compose_meeting_input(&note);
        assert!(!input.text.contains("You: \n"));
        assert!(!ask_context(&note, 3_000).contains("You: \n"));
        // A recording with only a marker has no talk at all.
        let only = note_with(vec![TranscriptSegment::dictation_marker(5)]);
        assert!(!compose_meeting_input(&only).text.contains("You:"));
        // The raw-tail header is dated from talk, not from a marker.
        let mut tail_marker = talk(400, 40, 15);
        tail_marker.push(TranscriptSegment::dictation_marker(1000 + 400 * 15 + 600));
        let long = note_with(tail_marker);
        assert!(!fits_single_pass(&long));
        let text = compose_meeting_input(&long).text;
        assert!(!text.contains("You: \n"));
    }

    pub(super) fn digest(
        from: usize,
        to: usize,
        start: u64,
        points: &[&str],
        actions: &[(&str, &str, &str)],
    ) -> MeetingDigest {
        MeetingDigest {
            from_seg: from,
            to_seg: to,
            start_ts: start,
            end_ts: start + 600,
            key_points: points.iter().map(|s| s.to_string()).collect(),
            decisions: vec![format!("decision at {start}")],
            actions: actions
                .iter()
                .map(|(o, t, d)| DigestAction {
                    owner: o.to_string(),
                    task: t.to_string(),
                    due: d.to_string(),
                })
                .collect(),
            questions: vec![],
            raw: String::new(),
        }
    }

    #[test]
    fn local_endpoints_are_recognized() {
        let ep = |provider: &str, url: &str| Endpoint {
            base_url: url.into(),
            api_key: String::new(),
            model: "m".into(),
            provider: provider.into(),
            fragment: String::new(),
            disable_thinking: false,
        };
        assert!(ep("ondevice", "http://127.0.0.1:5000/v1").is_local());
        assert!(ep("custom", "http://localhost:11434/v1").is_local());
        assert!(!ep("groq", "https://api.groq.com/openai/v1").is_local());
        assert_eq!(
            ep("custom", "http://127.0.0.1:1/v1")
                .options(700, 0.2)
                .max_tokens,
            Some(700)
        );
        assert_eq!(
            ep("groq", "https://api.groq.com/openai/v1")
                .options(700, 0.2)
                .max_tokens,
            None
        );
    }

    #[tokio::test]
    async fn a_call_gives_way_when_busy() {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;
        let busy = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&busy);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(150)).await;
            flag.store(true, Ordering::SeqCst);
        });
        let slow = async {
            tokio::time::sleep(Duration::from_secs(5)).await;
            "done"
        };
        let started = std::time::Instant::now();
        let out = unless_busy(slow, || busy.load(Ordering::SeqCst)).await;
        assert_eq!(out, None);
        assert!(started.elapsed() < Duration::from_secs(2));
        // Not busy: the call finishes.
        assert_eq!(unless_busy(async { 7 }, || false).await, Some(7));
    }

    pub(super) fn user_turn(messages: &Value) -> String {
        let all = messages.as_array().unwrap();
        all.last().unwrap()["content"].as_str().unwrap().to_string()
    }

    /// A helper call's whole size: its messages plus the reply cap.
    pub(super) fn call_tokens(system: &str, input: &str, reply: u32) -> usize {
        helper_messages(system, input)
            .as_array()
            .unwrap()
            .iter()
            .map(|m| estimate_tokens(m["content"].as_str().unwrap()))
            .sum::<usize>()
            + reply as usize
    }

    pub(super) fn with_invite(mut note: Note, description: &str) -> Note {
        note.event = Some(crate::notes::NoteEvent {
            key: "k".into(),
            title: "Offsite planning".into(),
            start: 1000,
            end: 4600,
            description: description.into(),
            service: String::new(),
        });
        note
    }

    #[tokio::test]
    async fn waiting_for_idle_stops_when_unwanted() {
        assert!(wait_idle(|| false, || true).await);
        assert!(!wait_idle(|| true, || false).await);
    }
}

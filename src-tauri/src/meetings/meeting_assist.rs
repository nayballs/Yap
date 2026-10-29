//! Small AI helpers for a meeting, all on the meeting's own model (the Note
//! Formatting scope, else the cleanup model; `meeting_summary::
//! resolve_endpoint`), all bounded to fit an 8k local model, and all
//! stepping aside for dictation (`meeting_summary::chat_beside_dictation`):
//!
//! - **"What did I miss?"** ([`meeting_catch_up`], the notepad's mini chat):
//!   what was said since the person last looked at the transcript. The
//!   notepad tracks what they've seen (the transcript on screen, or the last
//!   answer) and passes it as `since`; with nothing new it answers "Nothing
//!   new since you last looked." without calling a model. A question typed
//!   in the same chat is answered from the whole meeting so far.
//! - **The one-tap helpers** ([`meeting_assist`], chips in the notepad's Ask
//!   sheet and the Notes view's Ask bar): while a meeting records,
//!   **Questions to ask** (3–5 questions for now, from the latest talk);
//!   after it, **Follow-up email** (a recap for the attendees, plain text
//!   with its subject line first) and **Gaps and risks** (what it left
//!   undecided, unowned or unasked). Each reads only what the meeting has
//!   (talk, never echo or dictation markers) and says so, without a model,
//!   when there's too little to go on. And, since 2026-10-29 (Wispr Flow's
//!   Notetaker chips), **Make me sound smart** while it records: one or two
//!   things worth saying now, each built on a cited passage of the latest
//!   talk (`meeting_summary::chips`).
//! - **A Skill on the meeting** ([`meeting_skill`]): `/decisions`, `/risks`,
//!   the person's own (`chat/skills.rs`), typed in the meeting's Ask box:
//!   the Skill's instructions under fixed guardrails, the meeting as
//!   numbered passages, the answer citing them.
//! - **"Make notes longer"** ([`meeting_longer`], after the meeting): Yap's
//!   summary written again in more detail from the meeting, every task kept;
//!   the window's Undo puts the one before back (`note_edits::put_back`).
//! - **The AI meeting title** ([`maybe_title`]): once there's enough talk
//!   (150 words, or the first digest) a made-up title like "Teams call · 5
//!   Oct, 14:30", or none, becomes a short one from what the meeting is about
//!   ("Q3 Budget Review with Alice"). A title the person typed is never
//!   replaced (`notes::set_ai_title` checks under the store lock).

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::meetings::meeting_summary as summary;

/// The answer when nothing was said since the person last looked.
pub const NOTHING_NEW: &str = "Nothing new since you last looked.";

/// Name the meeting while it records once it has this many words of speech
/// (about a minute of talk), or a digest…
pub const TITLE_WORDS: usize = 150;
/// …or when it ends, if it has at least this many (fewer: "Started by
/// mistake?" asks first).
pub const TITLE_END_WORDS: usize = crate::meetings::meeting_end::MISTAKE_WORDS;
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
    crate::notes::commands::note_changed(app, note_id, "ai");
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
    /// Stopped while it was being written (the sheet's ■): what came.
    pub stopped: bool,
}

/// One notepad answer, streamed to the page's channel when it sent one
/// (`stream.rs`) and stoppable by `turn_id` (its ■): the answer, and
/// whether it was stopped (then: what had shown).
async fn streamed_answer(
    app: &AppHandle,
    ep: &summary::Endpoint,
    messages: serde_json::Value,
    opts: &crate::ai::llm::ChatOptions,
    on_stream: &crate::chat::stream::PageChannel,
    turn_id: Option<&str>,
) -> Result<(String, bool), String> {
    let stop = crate::chat::stream::Stoppable::new(turn_id);
    let watch = crate::chat::stream::Watch::new(on_stream);
    let sink = on_stream.0.is_some().then_some(&watch as &dyn crate::chat::stream::Sink);
    let asked = summary::chat_beside_dictation_live(app, ep, messages, opts, sink);
    match crate::chat::stream::unless_stopped(asked, Some(&stop)).await {
        Ok(answer) => Ok((answer?, false)),
        Err(_) => {
            let shown = watch.seen().text;
            if shown.trim().is_empty() {
                return Err(crate::chat::stream::STOPPED.to_string());
            }
            Ok((shown, true))
        }
    }
}

/// "What did I miss?" for meeting `note_id`: what was said since transcript
/// segment `since` (where the person last looked). With `question`, a
/// follow-up asked in the same chat, answered from the meeting so far —
/// with the thread's earlier turns (`conversationId`, `note_chats.rs`),
/// sized to the model (its summary and the turns after it, or the latest
/// that fit; a question too long for the model said plainly).
/// `onStream` / `turnId`: streamed to the sheet, stoppable
/// ([`streamed_answer`]).
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn meeting_catch_up(
    app: AppHandle,
    note_id: u64,
    since: usize,
    question: Option<String>,
    on_stream: crate::chat::stream::PageChannel,
    turn_id: Option<String>,
    conversation_id: Option<u64>,
) -> Result<CatchUp, String> {
    let note = crate::notes::get(note_id).ok_or("Note not found")?;
    let upto = note.transcript.len();
    let question = question.map(|q| q.trim().to_string()).filter(|q| !q.is_empty());
    let cfg = crate::config::load();
    let ep = summary::resolve_endpoint(&cfg)?;
    let mut said = None;
    let messages = match &question {
        None => match summary::catch_up_input(&note, since) {
            Some(input) => summary::catch_up_messages(&input),
            None => {
                return Ok(CatchUp { answer: NOTHING_NEW.to_string(), nothing_new: true, upto, stopped: false });
            }
        },
        Some(q) => {
            crate::chat::compact::turn_started();
            let asked = summary::meeting_ask_messages(&note, q);
            let call = crate::chat::note_chats::Call { provider: &ep.provider, model: &ep.model, base_url: &ep.base_url };
            let fixed: String = asked.as_array().into_iter().flatten().map(crate::chat::budget::message_text).collect::<Vec<_>>().join("\n");
            let thread = crate::chat::note_chats::history_for(note_id, conversation_id, call, &fixed, q).await?;
            let messages = crate::chat::note_chats::with_thread(asked, &thread);
            said = Some((thread.label, thread.context));
            messages
        }
    };
    let opts = ep.options(summary::CATCH_UP_REPLY_TOKENS, 0.3);
    let messages = crate::memory::with_helper_block(messages, &cfg);
    let (answer, stopped) = streamed_answer(&app, &ep, messages, &opts, &on_stream, turn_id.as_deref())
        .await
        .map_err(|e| match &said {
            Some((label, context)) => crate::chat::budget::plain_error(e, label, *context, &ep.provider),
            None => e,
        })?;
    let answer = answer.trim().to_string();
    if answer.is_empty() {
        return Err("The model returned an empty answer".to_string());
    }
    Ok(CatchUp { answer, nothing_new: false, upto, stopped })
}

/// The one-tap helpers besides "What did I miss?" (the `kind` of
/// [`meeting_assist`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Helper {
    /// While recording: questions to ask now ("Questions to ask").
    Questions,
    /// After the meeting: a recap to send the attendees ("Follow-up email").
    FollowUpEmail,
    /// After the meeting: what it left open ("Gaps and risks").
    BlindSpots,
    /// While recording: one or two things worth saying now ("Make me sound
    /// smart"), each built on a cited passage of the latest talk.
    Smart,
}

impl Helper {
    /// Its immutable prompt (`llm.rs`).
    pub fn prompt(self) -> &'static str {
        match self {
            Helper::Questions => crate::ai::llm::MEETING_QUESTIONS_PROMPT,
            Helper::FollowUpEmail => crate::ai::llm::MEETING_FOLLOW_UP_PROMPT,
            Helper::BlindSpots => crate::ai::llm::MEETING_BLIND_SPOTS_PROMPT,
            Helper::Smart => crate::ai::llm::MEETING_SMART_PROMPT,
        }
    }

    /// Reply cap (local models only) and temperature.
    fn reply(self) -> (u32, f32) {
        match self {
            Helper::Questions => (summary::QUESTIONS_REPLY_TOKENS, 0.4),
            Helper::FollowUpEmail => (summary::FOLLOW_UP_REPLY_TOKENS, 0.3),
            Helper::BlindSpots => (summary::BLIND_SPOTS_REPLY_TOKENS, 0.2),
            Helper::Smart => (summary::chips::SMART_REPLY_TOKENS, 0.4),
        }
    }

    /// What it reads of `note`, or `None` when that's too little to go on.
    pub fn input(self, note: &crate::notes::Note) -> Option<String> {
        match self {
            Helper::Questions => summary::questions_input(note),
            Helper::FollowUpEmail => summary::recap_input(note, summary::EMAIL_BESIDE_SUMMARY_TOKENS),
            Helper::BlindSpots => summary::recap_input(note, summary::GAPS_BESIDE_SUMMARY_TOKENS),
            Helper::Smart => summary::chips::smart_input(note, None).map(|c| c.text),
        }
    }

    /// The answer, without a model, when the meeting has too little in it.
    pub fn too_little(self) -> &'static str {
        match self {
            Helper::Questions => {
                "Not enough has been said yet to suggest questions. Try again once the meeting gets going."
            }
            Helper::FollowUpEmail => "There's too little in this meeting to write a follow-up email.",
            Helper::BlindSpots => "There's too little in this meeting to look for gaps.",
            Helper::Smart => "Not enough has been said yet to add something sharp. Try again once the meeting gets going.",
        }
    }
}

/// A helper's answer. `plain`: plain text to show and copy as it is (the
/// email), not markdown. `too_little`: answered without a model.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Assist {
    pub answer: String,
    pub plain: bool,
    pub too_little: bool,
    /// Stopped while it was being written (the sheet's ■): what came.
    pub stopped: bool,
    /// The meeting's passages it cites ("Make me sound smart", a Skill): a
    /// pill opens the meeting at the line.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<crate::chat::chats::Source>,
}

/// A one-tap helper for meeting `note_id` ([`Helper`]): one call on the
/// meeting's model, never slowing a dictation. `onStream` / `turnId`:
/// streamed to the sheet, stoppable ([`streamed_answer`]); the email is
/// cleaned (`clean_email`) once it's whole, in place of what streamed.
#[tauri::command]
pub async fn meeting_assist(
    app: AppHandle,
    note_id: u64,
    kind: Helper,
    on_stream: crate::chat::stream::PageChannel,
    turn_id: Option<String>,
) -> Result<Assist, String> {
    let note = crate::notes::get(note_id).ok_or("Note not found")?;
    let plain = kind == Helper::FollowUpEmail;
    let too_little = || Ok(Assist { answer: kind.too_little().to_string(), plain: false, too_little: true, stopped: false, sources: Vec::new() });
    // "Make me sound smart": the latest talk as passages to cite, with the
    // brief made before the meeting as context (calendar/brief.rs).
    let cited = if kind == Helper::Smart {
        match summary::chips::smart_input(&note, crate::calendar::brief_lines_for_note(note_id).as_deref()) {
            Some(c) => Some(c),
            None => return too_little(),
        }
    } else {
        None
    };
    let input = match &cited {
        Some(c) => c.text.clone(),
        None => match kind.input(&note) {
            Some(i) => i,
            None => return too_little(),
        },
    };
    let cfg = crate::config::load();
    let ep = summary::resolve_endpoint(&cfg)?;
    let (cap, temperature) = kind.reply();
    let opts = ep.options(cap, temperature);
    // The person (About you's name and note, memory's "You"): the email
    // signs with their name, the questions fit their role.
    let messages = crate::memory::with_helper_block(summary::helper_messages(kind.prompt(), &input), &cfg);
    let (reply, stopped) = streamed_answer(&app, &ep, messages, &opts, &on_stream, turn_id.as_deref()).await?;
    // A stopped email is what came, as it came (half an email isn't one).
    let plain = plain && !stopped;
    let (answer, sources) = if plain {
        (summary::clean_email(&reply, &summary::email_subject(&note)), Vec::new())
    } else if let Some(c) = &cited {
        // Cited as Ask across meetings' answers are: only its passages.
        summary::chips::checked(&reply, &c.passages, !stopped)
    } else {
        (reply.trim().to_string(), Vec::new())
    };
    if answer.is_empty() {
        return Err("The model returned an empty answer".to_string());
    }
    // A one-line "too little to go on" isn't an email to copy as one.
    let plain = plain && answer.starts_with("Subject: ");
    Ok(Assist { answer, plain, too_little: false, stopped, sources })
}

/// **A Skill on meeting `noteId`** (`/command` in its Ask box, with the
/// person's `words` after it): the Skill's instructions under fixed
/// guardrails (`llm::SKILL_MEETING_PROMPT`), the meeting as numbered
/// passages (`chips::skill_input`), the answer checked to cite only them
/// and returned with the ones it cites. Streamed and stoppable as the
/// helpers are; on the meeting's model, never slowing a dictation.
#[tauri::command]
pub async fn meeting_skill(
    app: AppHandle,
    note_id: u64,
    command: String,
    words: Option<String>,
    on_stream: crate::chat::stream::PageChannel,
    turn_id: Option<String>,
) -> Result<Assist, String> {
    let note = crate::notes::get(note_id).ok_or("Note not found")?;
    let skill = crate::chat::skills::by_command(&command)
        .ok_or_else(|| format!("There's no Skill called /{}.", command.trim_start_matches('/')))?;
    if !skill.runs_on.on_one() {
        return Err(format!("/{} runs across your meetings: use it in Chat.", skill.command));
    }
    let words = words.unwrap_or_default();
    let request = format!("{} {}", skill.instructions, words);
    let Some(cited) = summary::chips::skill_input(&note, &request) else {
        return Ok(Assist {
            answer: format!("There's nothing in this meeting yet for /{} to work on.", skill.command),
            plain: false,
            too_little: true,
            stopped: false,
            sources: Vec::new(),
        });
    };
    let cfg = crate::config::load();
    let ep = summary::resolve_endpoint(&cfg)?;
    let opts = ep.options(summary::chips::SKILL_REPLY_TOKENS, 0.3);
    let messages = crate::memory::with_helper_block(skill_messages(&skill, &cited.text, &words), &cfg);
    let (reply, stopped) = streamed_answer(&app, &ep, messages, &opts, &on_stream, turn_id.as_deref()).await?;
    let (answer, sources) = summary::chips::checked(&reply, &cited.passages, !stopped);
    if answer.is_empty() {
        return Err("The model returned an empty answer".to_string());
    }
    tracing::info!(note_id, builtin = skill.builtin, passages = cited.passages.len(), cited = sources.len(), "Skill run on a meeting");
    Ok(Assist { answer, plain: false, too_little: false, stopped, sources })
}

/// A Skill's request on one meeting: the fixed guardrails as the system
/// prompt; the meeting (`meeting`: `chips::skill_input`'s), then — last,
/// where a small model heeds it most (a 4B model given the instructions in
/// the system prompt answered "No decisions were made" over a meeting that
/// made two) — the Skill's name and instructions as saved, and the
/// person's own words with it.
pub fn skill_messages(skill: &crate::chat::skills::Skill, meeting: &str, words: &str) -> serde_json::Value {
    let mut asked = format!(
        "{}\n\nThe Skill to carry out on this meeting: {} (/{})\nInstructions:\n{}",
        meeting.trim_end(),
        skill.name,
        skill.command,
        skill.instructions.trim()
    );
    let words = words.trim();
    if !words.is_empty() {
        asked.push_str(&format!("\nThe user's own words with it: {words}"));
    }
    summary::helper_messages(crate::ai::llm::SKILL_MEETING_PROMPT, &asked)
}

/// What "Make notes longer" did: the summary now, and the one before (the
/// window's Undo puts it back: `note_summary_put_back`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Longer {
    pub state: crate::notes::note_edits::SummaryState,
    pub previous: crate::notes::note_edits::SummaryBefore,
}

/// **"Make notes longer"** (after a meeting): Yap's summary written again in
/// more detail from the meeting (`llm::MEETING_LONGER_PROMPT`, the summary
/// and the meeting as the action plan reads it, `chips::longer_request`),
/// every task kept — an action plan's reply goes through its checks, which
/// put back a task the model dropped and keep owners and deadlines to what
/// was said — and stored through `note_edits::write_summary` (so never over
/// the person's edited summary unless `replace`: the window asked). One
/// that came back no longer than before isn't stored. The summary before is
/// returned for Undo. (When note version history lands, the summary before
/// is kept as a version here, and Undo restores that version.)
#[tauri::command]
pub async fn meeting_longer(app: AppHandle, note_id: u64, replace: Option<bool>, origin: Option<String>) -> Result<Longer, String> {
    let replace = replace.unwrap_or(false);
    let note = crate::notes::get(note_id).ok_or("Note not found")?;
    if note.enhanced_content.trim().is_empty() {
        return Err("There's no summary to make longer yet.".to_string());
    }
    if crate::meetings::meeting::recording_note() == Some(note_id) {
        return Err("Make notes longer works once the meeting has ended.".to_string());
    }
    if crate::meetings::meeting_end::summary_running_for(note_id) {
        return Err("Yap is still writing this summary. Try again when it's done.".to_string());
    }
    crate::notes::note_edits::keeps_summary(&note, replace)?;
    let previous = crate::notes::note_edits::SummaryBefore::of(&note);
    let read_ms = now_ms();
    let heard_to = crate::notes::note_edits::newest_talk(&note);
    let hash = crate::notes::content_hash(&note.content);
    let cfg = crate::config::load();
    let ep = summary::resolve_endpoint(&cfg)?;
    // The on-device model is busy from the digests to the reply.
    let _sidecar = crate::ai::local_llm::hold(&ep.base_url);
    let budget = summary::FinalBudget::for_endpoint(&ep);
    let note = summary::prepare_final(&app, note_id, &ep, summary::chips::longer_budget(budget)).await?;
    let lang = summary::language::of_note(&note);
    let req = summary::chips::longer_request(&note, budget, &lang).ok_or("There's no summary to make longer yet.")?;
    let opts = ep.options(summary::chips::LONGER_REPLY_TOKENS, 0.3);
    let messages = summary::helper_messages(crate::ai::llm::MEETING_LONGER_PROMPT, &req.asked);
    let reply = summary::chat_beside_dictation(&app, &ep, messages, &opts).await?;
    let mut longer = summary::chips::clean_longer(&reply).ok_or("The model didn't write the notes. Try again.")?;
    if summary::chips::is_action_plan(&note.enhanced_content) {
        longer = summary::postcheck_action_plan(&longer, &crate::speakers::known_people(&note), &req.source, &req.tasks);
    }
    if summary::estimate_tokens(&longer) <= summary::estimate_tokens(&note.enhanced_content) {
        tracing::info!(note_id, "Make notes longer: the reply was no longer; the summary kept");
        return Err("The model didn't make the notes any longer, so Yap kept them as they were.".to_string());
    }
    crate::notes::note_edits::write_summary(note_id, &longer, &hash, read_ms, heard_to, replace)?;
    crate::notes::saved()?;
    tracing::info!(note_id, before = previous.enhanced_content.len(), after = longer.len(), "Make notes longer: written");
    crate::notes::note_edits::summary_changed(&app, note_id, origin.as_deref().unwrap_or(""));
    let state = crate::notes::get(note_id).map(|n| crate::notes::note_edits::state_of(&n)).ok_or("Note not found")?;
    Ok(Longer { state, previous })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_are_named_as_the_page_sends_them() {
        let kind = |s: &str| serde_json::from_value::<Helper>(serde_json::json!(s)).ok();
        assert_eq!(kind("questions"), Some(Helper::Questions));
        assert_eq!(kind("followUpEmail"), Some(Helper::FollowUpEmail));
        assert_eq!(kind("blindSpots"), Some(Helper::BlindSpots));
        assert_eq!(kind("smart"), Some(Helper::Smart));
        assert_eq!(kind("catchUp"), None); // that one is meeting_catch_up
    }

    #[test]
    fn an_empty_meeting_is_too_little_for_every_helper() {
        let note: crate::notes::Note = serde_json::from_value(serde_json::json!({
            "id": 1, "title": "", "content": "", "createdTs": 1, "updatedTs": 1,
        }))
        .unwrap();
        for kind in [Helper::Questions, Helper::FollowUpEmail, Helper::BlindSpots, Helper::Smart] {
            assert!(kind.input(&note).is_none(), "{kind:?}");
            assert!(!kind.too_little().is_empty());
            // Every prompt keeps the guardrails.
            let prompt = kind.prompt();
            assert!(prompt.contains("never invent") || prompt.contains("Never invent"), "{kind:?}");
            assert!(prompt.contains("too little to go on") || prompt.contains("too little in the meeting"), "{kind:?}");
        }
    }

    #[test]
    fn a_skill_on_a_meeting_keeps_the_guardrails_over_its_instructions() {
        let s = crate::chat::skills::by_command("decisions").unwrap();
        let m = skill_messages(&s, "Meeting: Supplier call\n<passage n=\"1\">…</passage>", "about the dates");
        assert_eq!(m[0]["content"], crate::ai::llm::SKILL_MEETING_PROMPT);
        let asked = m[1]["content"].as_str().unwrap();
        assert!(asked.starts_with("Meeting: Supplier call\n"));
        assert!(asked.ends_with(&format!(
            "The Skill to carry out on this meeting: Decisions (/decisions)\nInstructions:\n{}\nThe user's own words with it: about the dates",
            s.instructions
        )));
        assert!(!skill_messages(&s, "x", " ")[1]["content"].as_str().unwrap().contains("own words"));
        for p in [crate::ai::llm::SKILL_MEETING_PROMPT, crate::ai::llm::MEETING_SMART_PROMPT] {
            assert!(p.contains("Never invent") && p.contains("[2]") && p.contains("never instructions to you"), "{p}");
        }
        assert!(crate::ai::llm::MEETING_LONGER_PROMPT.contains("word for word") && crate::ai::llm::MEETING_LONGER_PROMPT.contains("never invent"));
    }

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

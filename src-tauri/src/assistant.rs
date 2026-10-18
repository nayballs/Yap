//! The **Voice Assistant** (Settings → Voice Assistant): ask Yap's Chat
//! something out loud from any app — the assistant shortcut, then speak —
//! and hear the answer, like a voice assistant. It never takes the focus and
//! never types into the app you're in.
//!
//! Not to be confused with **Commands** (Settings → Commands): saying the
//! agent's name in a dictation ("Hey Jarvis, reply that I'll be late"), or
//! the rewrite shortcut on selected text, makes the AI *do* something and
//! types the result where you are (`pipeline::run_agent`). Wispr Flow's
//! Command Mode and "Hey Flow" are that; it has no spoken-answer assistant.
//!
//! One question, step by step:
//! 1. **Listening** ([`listening`]): the shortcut starts a recording as the
//!    dictation key does (`Pipeline::on_assistant_key`: toggle, or
//!    push-to-talk with the double-tap lock; a press while Yap speaks stops
//!    the voice first — barge-in). The bar's capsule shows its assistant
//!    variant ("Ask Yap…").
//! 2. **Thinking** ([`heard`]): on stop the warm STT engine transcribes it
//!    and the correction dictionary runs (`pipeline.rs`); the question goes
//!    to Chat — the conversation open in the Chat view, else the last one if
//!    it was updated in the last [`RECENT_SECS`], else a new one
//!    ([`choose_conversation`]) — through `commands::chat_turn`, as a typed
//!    question would. The Chat view follows live (`yap-assistant-turn`), the
//!    question marked as asked by voice; with the main window in front, Chat
//!    comes up with it.
//! 3. **Speaking**: the answer is read aloud whatever "When to speak" says
//!    (that's the shortcut's point), in the chosen voice — never while a
//!    meeting records (its recorder would hear it) and not when a dictation
//!    started while it was thinking ([`reply`]). While the main window isn't
//!    the one in front (`notice::place`), a quiet card on the Yap bar shows
//!    the answer too; it goes when the voice ends, on ■ (the capsule), ✕ or
//!    Esc. Pressing the shortcut again asks a follow-up in the same
//!    conversation.
//!
//! "Open Teams" needs no model: Yap opens the app itself and says "Opening
//! Microsoft Teams." (`apps.rs`, through `chat_turn`), and so does "Yes."
//! to its "Did you mean Baldur's Gate 3?". A spoken question sends the
//! model a shorter history (scope "assistant"). Otherwise, no AI
//! model set up, or the model failed: an alert (`alert.rs`, through
//! the notice policy) with Open settings (Language Models → Chat & Voice
//! Assistant). No STT model or no mic: the dictation's own messages.
//!
//! Events: `yap-assistant {phase, conversationId, messageId}` (the bar's
//! capsule) and `yap-assistant-turn {phase: asked | answered | failed |
//! open, conversationId, text, show}` (the main window: `show` = bring the
//! Chat view up).

use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

/// How long the last conversation stays the one a question continues when
/// the Chat view isn't open on one.
pub const RECENT_SECS: u64 = 30 * 60;

/// The bar's capsule follows this: [`PhaseEvent`].
const EVENT: &str = "yap-assistant";
/// The main window follows this (the Chat view, ControlPanel).
const TURN: &str = "yap-assistant-turn";
/// The answer's card on the bar (its rule in `notice.rs`).
pub const CARD: &str = "assistant";
/// The "no AI model" alert's kind.
pub const NO_MODEL: &str = "assistantNoModel";
/// The "couldn't answer" alert's kind.
pub const FAILED: &str = "assistantFailed";
/// The "off during meetings" note's kind.
pub const MEETING_OFF: &str = "assistantMeetingOff";
/// The longest answer the bar card shows (the rest is in Chat).
const CARD_CHARS: usize = 1_200;

/// Where a question is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    #[default]
    Idle,
    /// Recording the question.
    Listening,
    /// Transcribing it, then waiting for the answer.
    Thinking,
    /// Reading the answer aloud.
    Speaking,
}

/// What the Chat view shows (`chat_view_state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Viewing {
    /// Chat isn't the main window's view.
    #[default]
    Closed,
    /// Chat is, on a new (empty) chat.
    New,
    /// Chat is, on this conversation.
    Conversation(u64),
}

/// Which conversation a question goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Continue(u64),
    New,
}

/// The conversation a spoken question continues. Pure.
/// - The one open in the Chat view (`open_exists`: it hasn't been deleted).
/// - Chat open on a new chat: a new conversation (it was asked for).
/// - Otherwise the conversation updated last (`latest`: id, unix secs), if
///   that was under [`RECENT_SECS`] before `now` — so pressing the shortcut
///   again asks a follow-up — else a new one.
pub fn choose_conversation(viewing: Viewing, open_exists: bool, latest: Option<(u64, u64)>, now: u64) -> Choice {
    match viewing {
        Viewing::Conversation(id) if open_exists => return Choice::Continue(id),
        Viewing::New => return Choice::New,
        _ => {}
    }
    match latest {
        Some((id, updated)) if now.saturating_sub(updated) < RECENT_SECS => Choice::Continue(id),
        _ => Choice::New,
    }
}

/// How an answer is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    /// Read aloud (and shown).
    Speak,
    /// Shown only.
    Show,
}

/// The assistant shortcut's answers are **always read aloud** — not as
/// "Speak replies" / "When to speak" say (`tts::auto_speak`, the Chat
/// view's rule) — except while a meeting records (the recorder would put
/// Yap's voice in the notes) or when a dictation started while the answer
/// was coming (`interrupted`: you've moved on). Pure.
pub fn reply(meeting: bool, interrupted: bool) -> Reply {
    if meeting || interrupted {
        Reply::Show
    } else {
        Reply::Speak
    }
}

/// Does the answer get its card on the Yap bar? As the person chose
/// ("Show the answer on the Yap bar", `assistant_answer_card`) while it's
/// read aloud; always when it isn't (`spoken` false: a meeting, a
/// dictation started, the voice failed), or the answer would be lost. Pure.
pub fn card_on_bar(show_answer_card: bool, spoken: bool) -> bool {
    show_answer_card || !spoken
}

/// How long a shown-only answer's card stays: about 15 characters a second
/// of reading, 8 s to a minute.
pub fn read_ms(text: &str) -> u64 {
    (text.chars().count() as u64 * 1000 / 15).clamp(8_000, 60_000)
}

// ---- state ------------------------------------------------------------------------------

#[derive(Debug, Default)]
struct State {
    phase: Phase,
    /// Bumped by every question, and by ■: an answer whose question is no
    /// longer the latest isn't said.
    generation: u64,
    /// The answer being read aloud ("chat:<conversation>:<index>").
    message_id: Option<String>,
    /// The conversation the latest question went to.
    conversation: Option<u64>,
}

static STATE: Mutex<State> = Mutex::new(State {
    phase: Phase::Idle,
    generation: 0,
    message_id: None,
    conversation: None,
});
static VIEW: Mutex<Viewing> = Mutex::new(Viewing::Closed);

/// Never held while emitting, touching windows or another module's lock.
fn lock() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}

fn view() -> Viewing {
    *VIEW.lock().unwrap_or_else(|p| p.into_inner())
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// `yap-assistant`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhaseEvent {
    pub phase: Phase,
    pub conversation_id: Option<u64>,
    pub message_id: Option<String>,
}

fn snapshot(s: &State) -> PhaseEvent {
    PhaseEvent { phase: s.phase, conversation_id: s.conversation, message_id: s.message_id.clone() }
}

fn emit(app: &AppHandle, ev: PhaseEvent) {
    let _ = app.emit(EVENT, ev);
}

// ---- one question ---------------------------------------------------------------------

/// A recording for the assistant starts (`pipeline::capture_target`, before
/// its "recording" state, so the capsule's first frame knows). Whatever an
/// earlier question was doing is superseded; its card goes.
pub fn listening(app: &AppHandle) {
    let ev = {
        let mut s = lock();
        s.generation += 1;
        s.phase = Phase::Listening;
        s.message_id = None;
        s.conversation = None;
        snapshot(&s)
    };
    tracing::info!("Voice Assistant: listening");
    emit(app, ev);
    crate::bar::dismiss_card(app, CARD);
}

/// The question didn't happen (cancelled, nothing heard, no model, the
/// mic gave nothing): back to idle, if it was still being asked.
pub fn nothing(app: &AppHandle) {
    let ev = {
        let mut s = lock();
        if !matches!(s.phase, Phase::Listening | Phase::Thinking) {
            return;
        }
        s.phase = Phase::Idle;
        s.message_id = None;
        snapshot(&s)
    };
    emit(app, ev);
}

/// The question, transcribed and corrected (`pipeline.rs`): ask Chat.
pub fn heard(app: &AppHandle, question: String) {
    let (generation, ev) = {
        let mut s = lock();
        s.phase = Phase::Thinking;
        (s.generation, snapshot(&s))
    };
    emit(app, ev);
    // A dictation starting from now on means you've moved on.
    let since = crate::pipeline::dictation_state().0;
    let app = app.clone();
    tauri::async_runtime::spawn(async move { answer(app, generation, since, question).await });
}

/// Whether `generation` is still the latest question.
fn current(generation: u64) -> bool {
    lock().generation == generation
}

/// The Voice Assistant is **off during meetings**: while a meeting records
/// or a call is on (`tts::meeting_quiet`), its shortcut doesn't listen and
/// Yap doesn't talk — a voice over a meeting is too much (Nathan,
/// 2026-10-18). A short note on the bar (or in the window) says so. `true`:
/// off now, nothing started.
pub fn off_for_meeting(app: &AppHandle) -> bool {
    if crate::tts::meeting_quiet().is_none() {
        return false;
    }
    tracing::info!("Voice Assistant: off during a meeting or a call");
    crate::alert::show_soon(
        app,
        crate::alert::Alert {
            kind: MEETING_OFF,
            icon: "assistant",
            title: "The Voice Assistant is off during meetings".into(),
            body: "While a meeting records or a call is on, Yap doesn't listen or talk. It's back once it's over.".into(),
            action: None,
            timeout_ms: 5_000,
        },
    );
    true
}

/// The end of a question that went no further, if it's still the latest.
fn give_up(app: &AppHandle, generation: u64) {
    if current(generation) {
        nothing(app);
    }
}

/// "Pick an AI model for the Voice Assistant", with Open settings.
fn no_model_alert(why: &str) -> crate::alert::Alert {
    tracing::info!("Voice Assistant: no AI model to answer ({why})");
    crate::alert::Alert {
        kind: NO_MODEL,
        icon: "assistant",
        title: "Pick an AI model for the Voice Assistant".into(),
        body: "It answers with Chat's model. Set one up in Settings → Language Models → Chat & Voice Assistant.".into(),
        action: Some(crate::alert::Action { id: crate::alert::OPEN_AI_SETTINGS, label: "Open settings" }),
        timeout_ms: 15_000,
    }
}

/// "The Voice Assistant couldn't answer" and why (the model's error: a key,
/// a limit, the network), with Open settings. The question is in Chat.
fn failed_alert(why: &str) -> crate::alert::Alert {
    let why: String = why.chars().take(240).collect();
    crate::alert::Alert {
        kind: FAILED,
        icon: "assistant",
        title: "The Voice Assistant couldn't answer".into(),
        body: format!("{why} Your question is in Chat."),
        action: Some(crate::alert::Action { id: crate::alert::OPEN_AI_SETTINGS, label: "Open settings" }),
        timeout_ms: 15_000,
    }
}

async fn answer(app: AppHandle, generation: u64, since: u64, question: String) {
    let cfg = crate::config::load();
    let viewing = view();
    let open_exists = matches!(viewing, Viewing::Conversation(id) if crate::chats::get(id).is_some());
    let choice = choose_conversation(viewing, open_exists, crate::chats::latest(), now_secs());
    let conversation_id = match choice {
        Choice::Continue(id) => Some(id),
        Choice::New => None,
    };
    // "Open Teams" needs no model, nor a "yes" to Yap's "Did you mean …?":
    // Yap opens it and says so (apps.rs).
    let opens_an_app = crate::apps::plan_for(&question, &cfg, conversation_id).await.is_some();
    if !opens_an_app {
        if let Err(why) = crate::commands::resolve_chat_endpoint(&cfg) {
            crate::alert::show_soon(&app, no_model_alert(&why));
            give_up(&app, generation);
            return;
        }
    }
    tracing::info!(?viewing, ?choice, "Voice Assistant: asking Chat");
    // The person is looking at Yap's main window: Chat comes up there with
    // the question (and no card on the bar).
    let (_, focused) = crate::notice::window_view(&app);
    let asked_app = app.clone();
    let asked_text = question.clone();
    let turn = crate::commands::chat_turn(&app, conversation_id, &question, Some("assistant"), true, move |conv, index| {
        lock().conversation = Some(conv);
        let _ = asked_app.emit(
            TURN,
            serde_json::json!({
                "phase": "asked",
                "conversationId": conv,
                "text": asked_text,
                "userMessageId": format!("chat:{conv}:{index}"),
                "show": focused,
            }),
        );
    })
    .await;
    let turn = match turn {
        Ok(turn) => turn,
        Err(e) => {
            tracing::warn!("Voice Assistant: no answer ({e})");
            let conv = lock().conversation;
            let _ = app.emit(TURN, serde_json::json!({ "phase": "failed", "conversationId": conv, "error": e }));
            if current(generation) {
                crate::alert::show_soon(&app, failed_alert(&e));
            }
            give_up(&app, generation);
            return;
        }
    };
    let message_id = turn.message_id();
    let _ = app.emit(
        TURN,
        serde_json::json!({
            "phase": "answered",
            "conversationId": turn.conversation_id,
            "messageId": message_id,
            "show": focused,
        }),
    );
    // A meeting or a call that started while the answer was coming: shown,
    // not read aloud.
    let meeting = crate::tts::meeting_quiet().is_some();
    let interrupted = crate::pipeline::dictation_state().0 != since;
    let mut how = reply(meeting, interrupted);
    // Set before speaking: the voice may end (and say so) before `speak`
    // returns.
    let ev = {
        let mut s = lock();
        if s.generation != generation {
            tracing::info!("Voice Assistant: a newer question (or ■) came first; the answer is in Chat");
            return;
        }
        s.conversation = Some(turn.conversation_id);
        if how == Reply::Speak {
            s.phase = Phase::Speaking;
            s.message_id = Some(message_id.clone());
        } else {
            s.phase = Phase::Idle;
            s.message_id = None;
        }
        snapshot(&s)
    };
    emit(&app, ev);
    let note = if meeting {
        Some("Not read aloud during a meeting")
    } else if interrupted {
        Some("Not read aloud: you started dictating")
    } else {
        None
    };
    // The card first (the voice may be short), then the voice. With "Show
    // the answer on the Yap bar" off, a spoken answer has no card.
    let on_bar = crate::notice::place(&app, false) == crate::notice::Place::Bar;
    let mut carded = on_bar
        && card_on_bar(cfg.assistant_answer_card, how == Reply::Speak)
        && show_card(&app, &question, &turn.reply, how == Reply::Speak, note);
    if how == Reply::Speak {
        if let Err(e) = crate::tts::speak(&turn.reply, Some(message_id.clone())) {
            tracing::warn!("Voice Assistant: couldn't read the answer aloud ({e})");
            how = Reply::Show;
            let ev = {
                let mut s = lock();
                if s.phase == Phase::Speaking && s.message_id.as_deref() == Some(message_id.as_str()) {
                    s.phase = Phase::Idle;
                    s.message_id = None;
                    Some(snapshot(&s))
                } else {
                    None
                }
            };
            if let Some(ev) = ev {
                emit(&app, ev);
            }
            let why = format!("Couldn't read it aloud: {e}");
            if carded {
                let ms = read_ms(&turn.reply);
                crate::bar::update_card(&app, CARD, |c| {
                    c.timeout_ms = Some(ms);
                    c.status = Some(why);
                });
            } else if on_bar {
                // The card was left out for the voice, and there's no voice:
                // show the answer after all.
                carded = show_card(&app, &question, &turn.reply, false, Some(why.as_str()));
            }
        }
    }
    tracing::info!(spoken = how == Reply::Speak, carded, meeting, interrupted, "Voice Assistant: answered");
}

/// The answer on the Yap bar: the question over the answer (markdown, cut
/// at [`CARD_CHARS`]), Open in Chat; ✕ or Esc stop the voice. While it's
/// read aloud it stays until the voice ends; shown only, it fades after a
/// reading time. `false`: the bar is off or hidden.
fn show_card(app: &AppHandle, question: &str, answer: &str, speaking: bool, note: Option<&str>) -> bool {
    let body = if answer.chars().count() > CARD_CHARS {
        format!("{}…", answer.chars().take(CARD_CHARS).collect::<String>().trim_end())
    } else {
        answer.to_string()
    };
    let title: String = if question.chars().count() > 120 {
        format!("{}…", question.chars().take(120).collect::<String>().trim_end())
    } else {
        question.to_string()
    };
    let card = crate::bar::Card {
        id: CARD.into(),
        style: "answer",
        icon: "assistant",
        title,
        body,
        status: note.map(str::to_string),
        primary: Some(crate::bar::CardAction::new("open", "Open in Chat")),
        timeout_ms: (!speaking).then(|| read_ms(answer)),
        expire_action: Some("expire".into()),
        close_action: Some("close".into()),
        escape_action: Some("close".into()),
        ..Default::default()
    };
    crate::bar::show_card(app, card, Box::new(card_action))
}

fn card_action(app: &AppHandle, action: &str) {
    match action {
        "open" => open_chat(app),
        "close" => stop(app),
        _ => {}
    }
}

/// "Open in Chat": the main window, on the question's conversation.
fn open_chat(app: &AppHandle) {
    let conv = lock().conversation;
    if let Err(e) = crate::commands::show_settings(app) {
        tracing::warn!("Voice Assistant: couldn't open the main window ({e})");
        return;
    }
    let _ = app.emit(TURN, serde_json::json!({ "phase": "open", "conversationId": conv, "show": true }));
}

/// ■ on the capsule, ✕ or Esc on the card: the voice stops, an answer still
/// coming isn't said (it's in Chat), the card goes.
pub fn stop(app: &AppHandle) {
    let (ev, was_speaking) = {
        let mut s = lock();
        if matches!(s.phase, Phase::Thinking | Phase::Speaking) {
            let was = s.phase == Phase::Speaking;
            s.generation += 1;
            s.phase = Phase::Idle;
            s.message_id = None;
            (Some(snapshot(&s)), was)
        } else {
            (None, false)
        }
    };
    if let Some(ev) = ev {
        tracing::info!("Voice Assistant: stopped");
        emit(app, ev);
    }
    if was_speaking {
        crate::tts::stop();
    }
    crate::bar::dismiss_card(app, CARD);
}

/// The voice changed (`tts.rs` `yap-tts-state`): the answer it was reading
/// has ended (or was stopped — a dictation's barge-in, ■ in Chat): back to
/// idle, and its card goes.
pub fn on_tts_state(app: &AppHandle, speaking: bool, message_id: Option<&str>) {
    if speaking {
        return;
    }
    let ev = {
        let mut s = lock();
        if s.phase != Phase::Speaking || s.message_id.as_deref() != message_id {
            return;
        }
        s.phase = Phase::Idle;
        s.message_id = None;
        snapshot(&s)
    };
    emit(app, ev);
    crate::bar::dismiss_card(app, CARD);
}

// ---- commands -------------------------------------------------------------------------

/// The Chat view says what it shows (on mount, on every switch, on leaving):
/// where a spoken question goes ([`choose_conversation`]). `new_chat`: no
/// conversation, because "New chat" was clicked (a Chat view just opened on
/// nothing has no preference: as if closed).
#[tauri::command]
pub fn chat_view_state(open: bool, conversation_id: Option<u64>, new_chat: Option<bool>) {
    *VIEW.lock().unwrap_or_else(|p| p.into_inner()) = match (open, conversation_id) {
        (false, _) => Viewing::Closed,
        (true, Some(id)) => Viewing::Conversation(id),
        (true, None) if new_chat.unwrap_or(false) => Viewing::New,
        (true, None) => Viewing::Closed,
    };
}

/// ■: stop the answer (see [`stop`]).
#[tauri::command]
pub fn assistant_stop(app: AppHandle) {
    stop(&app);
}

/// Debug-only, test mode: a spoken question without a microphone — the
/// shortcut's own steps (barge-in, "listening" for `listenMs` on the
/// capsule, "Thinking…") with `text` as the transcript, then the answer as
/// usual (spoken into `tts_e2e_spoken`, the card, Chat).
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn e2e_assistant_ask(
    state: tauri::State<'_, crate::AppState>,
    text: String,
    listen_ms: Option<u64>,
) -> Result<(), String> {
    if !crate::e2e::active() {
        return Err("e2e_assistant_ask is for test runs".into());
    }
    let robot = {
        let guard = state.pipeline.lock().map_err(|_| "the pipeline lock is poisoned")?;
        guard.as_ref().ok_or("no pipeline")?.robot()
    };
    robot.ask_assistant(text, listen_ms.unwrap_or(0)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000;

    #[test]
    fn a_question_continues_the_conversation_open_in_chat() {
        // Open in Chat: that one, however old the latest is.
        assert_eq!(
            choose_conversation(Viewing::Conversation(4), true, Some((9, NOW - 10)), NOW),
            Choice::Continue(4)
        );
        assert_eq!(choose_conversation(Viewing::Conversation(4), true, None, NOW), Choice::Continue(4));
        // Deleted since: as if Chat weren't open.
        assert_eq!(
            choose_conversation(Viewing::Conversation(4), false, Some((9, NOW - 10)), NOW),
            Choice::Continue(9)
        );
        assert_eq!(choose_conversation(Viewing::Conversation(4), false, None, NOW), Choice::New);
    }

    #[test]
    fn a_new_chat_in_the_chat_view_gets_a_new_conversation() {
        assert_eq!(choose_conversation(Viewing::New, false, Some((9, NOW - 10)), NOW), Choice::New);
    }

    #[test]
    fn with_chat_closed_the_last_conversation_continues_for_half_an_hour() {
        assert_eq!(choose_conversation(Viewing::Closed, false, Some((9, NOW - 10)), NOW), Choice::Continue(9));
        assert_eq!(
            choose_conversation(Viewing::Closed, false, Some((9, NOW - RECENT_SECS + 1)), NOW),
            Choice::Continue(9)
        );
        assert_eq!(choose_conversation(Viewing::Closed, false, Some((9, NOW - RECENT_SECS)), NOW), Choice::New);
        assert_eq!(choose_conversation(Viewing::Closed, false, Some((9, NOW - 3 * 3600)), NOW), Choice::New);
        assert_eq!(choose_conversation(Viewing::Closed, false, None, NOW), Choice::New);
        // A clock that went backwards still counts as recent.
        assert_eq!(choose_conversation(Viewing::Closed, false, Some((9, NOW + 5)), NOW), Choice::Continue(9));
    }

    #[test]
    fn the_answer_card_is_a_choice_only_while_the_answer_is_spoken() {
        // Read aloud: as the person chose.
        assert!(card_on_bar(true, true));
        assert!(!card_on_bar(false, true));
        // Not read aloud (a meeting, a dictation started, the voice failed):
        // always shown, or the answer would be lost.
        assert!(card_on_bar(true, false));
        assert!(card_on_bar(false, false));
    }

    #[test]
    fn the_shortcut_always_speaks_except_over_a_meeting_or_a_new_dictation() {
        assert_eq!(reply(false, false), Reply::Speak);
        assert_eq!(reply(true, false), Reply::Show);
        assert_eq!(reply(false, true), Reply::Show);
        assert_eq!(reply(true, true), Reply::Show);
        // Unlike Chat's own rule: with spoken replies off (or "When I asked
        // by voice" and a typed question), Chat stays quiet; the shortcut
        // doesn't.
        assert!(!crate::tts::auto_speak(false, "voice", true, false, false));
        assert!(!crate::tts::auto_speak(false, "always", true, false, false));
        // Both keep quiet over a meeting.
        assert!(!crate::tts::auto_speak(true, "always", true, false, true));
    }

    #[test]
    fn a_shown_only_answer_stays_long_enough_to_read() {
        assert_eq!(read_ms("Yes."), 8_000);
        assert_eq!(read_ms(&"word ".repeat(60)), 20_000);
        assert_eq!(read_ms(&"x".repeat(5_000)), 60_000);
    }
}

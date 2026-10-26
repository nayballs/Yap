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
//!    variant ("Ask Yap…"). It ends when the shortcut (or the dictation
//!    key) is pressed again, on ✓, or as a push-to-talk key is let go —
//!    never on a pause.
//! 2. **Thinking** ([`heard`]): on stop the warm STT engine transcribes it
//!    and the correction dictionary runs (`pipeline.rs`); the question goes
//!    to Chat — the conversation open in the Chat view, else the last one if
//!    it was updated in the last [`RECENT_SECS`], else a new one
//!    ([`choose_conversation`]) — through `turn::chat_turn`, as a typed
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
//!    conversation. An answer written from the web (`web.rs`, with
//!    "Look things up on the web" on) is said short — its first one or two
//!    sentences, citation markers left out — then "The details and sources
//!    are in Chat." ([`spoken_answer`]); the card shows the short answer and
//!    "3 sources in Chat".
//!
//! "Open Teams" needs no model: Yap opens the app itself and says "Opening
//! Microsoft Teams." (`apps.rs`, through `chat_turn`), and so does "Yes."
//! to its "Did you mean Baldur's Gate 3?". A spoken question sends the
//! model a shorter history (scope "assistant"). Otherwise, no AI
//! model set up, or the model failed: an alert (`alert.rs`, through
//! the notice policy) with Open settings (Language Models → Chat & Voice
//! Assistant). No STT model or no mic: the dictation's own messages.
//!
//! **Streamed** (`stream.rs`, 2026-10-22): the answer is spoken from its
//! first whole sentence while the rest is still being written
//! ([`Streamed`]), the same short version as before ([`spoken_answer`]: at
//! most two sentences go to the voice while it's written, the rest of what's
//! said once the answer is whole and checked); a web answer's sentence with
//! a price in it waits for the check (`about_you::restore_price_signs` may
//! change it). Barge-in, ■ and the meeting rule stop it as before.
//!
//! **A newer question replaces an older one** in the same conversation: a
//! barge-in or follow-up asked while the last answer is still coming (or
//! still waiting out that very question's recording, as a model on this PC
//! does) ends the older turn once the newer question is saved — no answer
//! is saved for it, nothing more of it is said (`stream::Halt::Superseded`,
//! `turn::chat_turn`). Before, it was asked again after the newer one's
//! answer, and both answers showed under both questions (a tester's voice
//! chat, 2026-10-23).
//!
//! **The screenshots waiting in Chat's box** (Snip to Chat, 2026-10-25) go
//! with a spoken question (`snip/waiting.rs`): it goes to the conversation
//! they wait in, through the same image path a typed question takes
//! (`snip::turn::prepare` / `with`, the vision rules, the caps), the box
//! clears, and the question shows in Chat with its thumbnails. A model known
//! not to read images gets nothing: the screenshots stay, the question goes
//! into the box and Chat comes up with its "can't read images" line, as a
//! typed one would ([`cant_read`]). After a **spoken "snip"** lands in
//! Chat, Yap listens for the question by itself (`snip.rs`
//! `listens_after`, `pipeline::listen_after_snip`): the capsule says "Ask
//! about this screenshot…" ([`about_screenshot`]).
//!
//! Events: `yap-assistant {phase, conversationId, messageId, about?}` (the bar's
//! capsule), `yap-assistant-turn {phase: asked | answered | failed |
//! superseded | open, conversationId, userMessageId, text, show}` (the main
//! window: `show` = bring the Chat view up; `userMessageId` = which question,
//! "chat:<conversation>:<index>") and `yap-assistant-stream {conversationId,
//! userMessageId, event}` (the Chat view follows the answer as it's written:
//! a `stream::Event`).

use std::sync::atomic::{AtomicBool, Ordering};
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
/// The Chat view follows the answer as it's written.
const STREAM: &str = "yap-assistant-stream";
/// How many sentences go to the voice while the answer is still being
/// written: the short version's ([`spoken_answer`]'s `tts::brief(…, 2)`).
const STREAM_SAY: usize = 2;
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

/// The assistant shortcut's answers are read aloud while **"Speak
/// replies"** is on — the one voice switch (Chat's Voice replies too):
/// with it off Yap never talks, and the answer is shown (the maintainer,
/// 2026-10-18: "I turned off the voice yet heard the voice still"). Not
/// "When to speak" (that's for Chat's typed questions: the shortcut is
/// always asked by voice). Never while a meeting records or a call is on,
/// or when a dictation started while the answer was coming
/// (`interrupted`: you've moved on). Pure.
pub fn reply(voice_on: bool, meeting: bool, interrupted: bool) -> Reply {
    if voice_on && !meeting && !interrupted {
        Reply::Speak
    } else {
        Reply::Show
    }
}

/// Does the answer get its card on the Yap bar? As the person chose
/// ("Show the answer on the Yap bar", `assistant_answer_card`) while it's
/// read aloud; always when it isn't (`spoken` false: a meeting, a
/// dictation started, the voice failed), or the answer would be lost. Pure.
pub fn card_on_bar(show_answer_card: bool, spoken: bool) -> bool {
    show_answer_card || !spoken
}

/// What the assistant says, and what its card shows, for an answer.
/// Written from the web (`sources` > 0): the short answer — its first one
/// or two sentences, without citation markers (`tts::brief`) — then "The
/// details and sources are in Chat."; the card shows the short answer.
/// Otherwise the whole answer, both ways. Pure.
pub fn spoken_answer(reply: &str, sources: usize) -> (String, String) {
    if sources == 0 {
        // Told to answer in a sentence or two (`SPOKEN_ANSWER_NOTE`); one
        // that runs long anyway is said short too, the rest in Chat.
        if reply.split_whitespace().count() > SPOKEN_MAX_WORDS {
            let brief = crate::voice::tts::brief(reply, 2);
            if !brief.is_empty() && brief.split_whitespace().count() < reply.split_whitespace().count() {
                return (format!("{brief} {REST_IN_CHAT}"), brief);
            }
        }
        return (reply.to_string(), reply.to_string());
    }
    let brief = crate::voice::tts::brief(reply, 2);
    if brief.is_empty() {
        return ("The answer and its sources are in Chat.".to_string(), reply.to_string());
    }
    (format!("{brief} {DETAILS_IN_CHAT}"), brief)
}

/// Said after a web answer's short version.
pub const DETAILS_IN_CHAT: &str = "The details and sources are in Chat.";

/// How many of an answer's sources are web pages: a calendar answer's
/// meetings (`chat/tools/calendar.rs`) are said whole, like any answer
/// ("You've the design review at 2."), not cut short for their sources.
fn web_sources(turn: &crate::chat::turn::ChatTurn) -> usize {
    turn.sources.iter().filter(|s| !s.url.is_empty()).count()
}

/// The card's note for a web answer: "2 sources in Chat".
fn sources_note(web: usize) -> Option<String> {
    match web {
        0 => None,
        1 => Some("1 source in Chat".to_string()),
        n => Some(format!("{n} sources in Chat")),
    }
}

/// Said after the short version of a long answer.
pub const REST_IN_CHAT: &str = "The rest is in Chat.";

/// An answer longer than this is said short ([`spoken_answer`]).
pub const SPOKEN_MAX_WORDS: usize = 60;

/// The end of Chat's system prompt for a question asked out loud whose
/// answer is read aloud whole (`turn::chat_turn`): without it, "how long
/// do I boil an egg" got 55–98 words with lists from every local model and
/// gpt-oss (the model evaluations' voice scenarios).
pub const SPOKEN_ANSWER_NOTE: &str = "This question was asked out loud and your answer will be read aloud: answer in one or two short sentences (under 40 words) of plain speech, with no lists, headings or markdown, and no offer of more help.";

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
    /// What the question is about, while it's listened for: "screenshot"
    /// (after a spoken "snip": "Ask about this screenshot…").
    about: Option<&'static str>,
}

static STATE: Mutex<State> = Mutex::new(State {
    phase: Phase::Idle,
    generation: 0,
    message_id: None,
    conversation: None,
    about: None,
});

/// The next question listened for is about the screenshot just snipped
/// ([`about_screenshot`]).
static ABOUT_SCREENSHOT: AtomicBool = AtomicBool::new(false);

/// The recording about to start is the question after a spoken "snip"
/// (`pipeline::listen_after_snip`): its capsule says "Ask about this
/// screenshot…". `false`: it didn't start after all.
pub fn about_screenshot(on: bool) {
    ABOUT_SCREENSHOT.store(on, Ordering::SeqCst);
}

/// The Voice Assistant is off now (a meeting records or a call is on:
/// [`off_for_meeting`]'s rule), said to nobody: a spoken snip's question
/// isn't listened for then.
pub fn is_off() -> bool {
    crate::voice::tts::meeting_quiet().is_some()
}
static VIEW: Mutex<Viewing> = Mutex::new(Viewing::Closed);

/// Never held while emitting, touching windows or another module's lock.
fn lock() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}

fn view() -> Viewing {
    *VIEW.lock().unwrap_or_else(|p| p.into_inner())
}

/// What the Chat view shows (Snip to Chat puts a screenshot in the
/// conversation open there, `snip.rs`).
pub fn chat_viewing() -> Viewing {
    view()
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
    /// "screenshot": the question after a spoken "snip".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub about: Option<&'static str>,
}

fn snapshot(s: &State) -> PhaseEvent {
    PhaseEvent { phase: s.phase, conversation_id: s.conversation, message_id: s.message_id.clone(), about: s.about }
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
        s.about = ABOUT_SCREENSHOT.swap(false, Ordering::SeqCst).then_some("screenshot");
        snapshot(&s)
    };
    tracing::info!(about_screenshot = ev.about.is_some(), "Voice Assistant: listening");
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
        s.about = None;
        snapshot(&s)
    };
    emit(app, ev);
}

/// The question, transcribed and corrected (`pipeline.rs`): ask Chat.
pub fn heard(app: &AppHandle, question: String) {
    let (generation, ev) = {
        let mut s = lock();
        s.phase = Phase::Thinking;
        s.about = None;
        (s.generation, snapshot(&s))
    };
    emit(app, ev);
    // A dictation starting from now on means you've moved on.
    let since = crate::dictation::pipeline::dictation_state().0;
    let app = app.clone();
    tauri::async_runtime::spawn(async move { answer(app, generation, since, question).await });
}

/// The stream id of question `generation`'s answer (`stream::Stoppable`).
fn stop_id(generation: u64) -> String {
    format!("assistant-{generation}")
}

/// Whether `generation` is still the latest question.
fn current(generation: u64) -> bool {
    lock().generation == generation
}

/// The Voice Assistant is **off during meetings**: while a meeting records
/// or a call is on (`tts::meeting_quiet`), its shortcut doesn't listen and
/// Yap doesn't talk — a voice over a meeting is too much (the maintainer,
/// 2026-10-18). A short note on the bar (or in the window) says so. `true`:
/// off now, nothing started.
pub fn off_for_meeting(app: &AppHandle) -> bool {
    if crate::voice::tts::meeting_quiet().is_none() {
        return false;
    }
    tracing::info!("Voice Assistant: off during a meeting or a call");
    crate::notices::alert::show_soon(
        app,
        crate::notices::alert::Alert {
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
fn no_model_alert(why: &str) -> crate::notices::alert::Alert {
    tracing::info!("Voice Assistant: no AI model to answer ({why})");
    crate::notices::alert::Alert {
        kind: NO_MODEL,
        icon: "assistant",
        title: "Pick an AI model for the Voice Assistant".into(),
        body: "It answers with Chat's model. Set one up in Settings → Language Models → Chat & Voice Assistant.".into(),
        action: Some(crate::notices::alert::Action { id: crate::notices::alert::OPEN_AI_SETTINGS, label: "Open settings" }),
        timeout_ms: 15_000,
    }
}

/// "The Voice Assistant couldn't answer" and why (the model's error: a key,
/// a limit, the network), with Open settings. The question is in Chat.
fn failed_alert(why: &str) -> crate::notices::alert::Alert {
    let why: String = why.chars().take(240).collect();
    crate::notices::alert::Alert {
        kind: FAILED,
        icon: "assistant",
        title: "The Voice Assistant couldn't answer".into(),
        body: format!("{why} Your question is in Chat."),
        action: Some(crate::notices::alert::Action { id: crate::notices::alert::OPEN_AI_SETTINGS, label: "Open settings" }),
        timeout_ms: 15_000,
    }
}

/// A spoken question with screenshots for a model that can't read images
/// (known before, or its server just turned the image down: `refused`),
/// handled as a typed one is: no picture goes, the screenshots stay in
/// Chat's box, the question goes into the box with them, and Chat comes up
/// with its "can't read images" line (the switch, "Send its text instead",
/// or without). An alert says so where it'll be seen.
fn cant_read(app: &AppHandle, question: &str, conversation: Option<u64>, refused: bool) {
    tracing::info!(refused, known_conversation = conversation.is_some(), "Voice Assistant: the Chat model can't read the screenshot; the question waits in Chat");
    if let Err(e) = crate::shell::window::show_settings(app) {
        tracing::warn!("Voice Assistant: couldn't open the main window ({e})");
    }
    // Into the box at the caret, not sent (ControlPanel opens Chat with it).
    let _ = app.emit("yap-chat-dictation", serde_json::json!({ "text": question, "send": false }));
    let title = if refused { crate::snip::vision::COULDNT_READ } else { crate::snip::vision::CANT_READ };
    crate::notices::alert::show_soon(
        app,
        crate::notices::alert::Alert {
            kind: FAILED,
            icon: "assistant",
            title: title.into(),
            body: "Your question is waiting in Chat with the screenshot: pick a model that reads images, or send its text instead.".into(),
            action: None,
            timeout_ms: 10_000,
        },
    );
}

/// A price in a sentence ("$8", "£20 a month", "15 euros"): in a web
/// answer it waits for the answer's check before it's said. Pure.
pub fn mentions_price(sentence: &str) -> bool {
    static PRICE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)[$£€¥₹]\s?\d|\d\s?(?:usd|gbp|eur|dollars?|pounds?|euros?|cents?|pence|quid|bucks)\b|\b\d{1,2}p\b")
            .expect("static regex")
    });
    PRICE.is_match(sentence)
}

/// **A spoken question's answer as it's written** (a `stream::Sink` for
/// `chat_turn`): the Chat view follows it (`yap-assistant-stream`), and the
/// voice starts at its first whole sentence ([`STREAM_SAY`] at most until
/// the answer is whole), with the card on the Yap bar.
struct Streamed {
    app: AppHandle,
    generation: u64,
    /// Dictations started when the question was heard.
    since: u64,
    question: String,
    voice_on: bool,
    show_card: bool,
    s: Mutex<Speaking>,
}

#[derive(Default)]
struct Speaking {
    conversation: Option<u64>,
    /// The question's id ("chat:<conversation>:<index>"): the Chat view
    /// follows this question's answer, not an older one's.
    question_id: Option<String>,
    /// The answer's id, as it will be saved ("chat:<conversation>:<index>").
    message_id: Option<String>,
    feed: Option<std::sync::Arc<crate::voice::tts::Feed>>,
    /// Sentences handed to the voice.
    said: usize,
    /// The turn has read the web.
    web: bool,
    /// A sentence waits for the answer's check (a price in a web answer).
    held: bool,
    /// The voice can't go on (stopped, a dictation or a meeting started).
    quiet: bool,
    carded: bool,
}

impl Streamed {
    fn lock(&self) -> MutexGuard<'_, Speaking> {
        self.s.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// May the voice speak now: still the latest question, and no meeting
    /// or new dictation since it was asked.
    fn may_speak(&self) -> bool {
        self.voice_on
            && current(self.generation)
            && crate::voice::tts::meeting_quiet().is_none()
            && crate::dictation::pipeline::dictation_state().0 == self.since
    }

    /// Hand the voice the sentences of `text` that are ready, starting it
    /// with the first.
    fn speak_ready(&self, text: &str) {
        let started_id = {
            let mut st = self.lock();
            if st.quiet || st.held {
                return;
            }
            if !self.may_speak() {
                st.quiet = true;
                return;
            }
            let ready = crate::voice::tts::ready_sentences(text, STREAM_SAY, false);
            if ready.len() <= st.said {
                return;
            }
            let mut new = Vec::new();
            for sentence in &ready[st.said..] {
                if st.web && mentions_price(sentence) {
                    st.held = true;
                    break;
                }
                new.push(sentence.clone());
            }
            if new.is_empty() {
                return;
            }
            let n = new.len();
            match &st.feed {
                Some(feed) => {
                    if !feed.push(new) {
                        st.quiet = true;
                        return;
                    }
                    st.said += n;
                    None
                }
                None => {
                    let Some(id) = st.message_id.clone() else { return };
                    match crate::voice::tts::speak_streamed(Some(id.clone())) {
                        Ok(feed) => {
                            feed.push(new);
                            st.feed = Some(feed);
                            st.said += n;
                            Some(id)
                        }
                        Err(e) => {
                            tracing::info!("Voice Assistant: couldn't start the voice while the answer came ({e})");
                            st.quiet = true;
                            None
                        }
                    }
                }
            }
        };
        // The voice started: "Speaking" on the capsule, and the card.
        let Some(id) = started_id else { return };
        tracing::info!("Voice Assistant: speaking from the answer's first sentence");
        let ev = {
            let mut s = lock();
            if s.generation != self.generation {
                return;
            }
            s.phase = Phase::Speaking;
            s.message_id = Some(id);
            snapshot(&s)
        };
        emit(&self.app, ev);
        let on_bar = crate::notices::notice::place(&self.app, false) == crate::notices::notice::Place::Bar;
        if on_bar && card_on_bar(self.show_card, true) {
            let shown = crate::voice::tts::brief(text, STREAM_SAY);
            let carded = show_card(&self.app, &self.question, &shown, true, None);
            self.lock().carded = carded;
        }
    }

    /// **The text so far wasn't the answer** (2026-10-25): a step's opening
    /// words, long enough to show, turned out to come before a tool call
    /// (`stream::Step`, or a call the provider refused) and were taken back.
    /// The voice may already be saying them: it stops, what it hadn't said
    /// yet goes, the capsule is back to "Thinking…" and their card goes. The
    /// answer, when it comes, starts the voice again from its first sentence
    /// — before, the voice went on with the words taken back and the
    /// answer's first sentences were skipped as already said. A forced
    /// search's words never get this far (`stream::ChipsOnly`).
    fn taken_back(&self) {
        let (feed, carded) = {
            let mut st = self.lock();
            st.said = 0;
            st.held = false;
            (st.feed.take(), std::mem::take(&mut st.carded))
        };
        let Some(feed) = feed else { return };
        tracing::info!("Voice Assistant: the words it was saying came before a tool call; stopped");
        // "Thinking…" before the voice stops, so its end isn't the answer's.
        let (ev, id) = {
            let mut s = lock();
            if s.generation == self.generation && s.phase == Phase::Speaking {
                s.phase = Phase::Thinking;
                (Some(snapshot(&s)), s.message_id.take())
            } else {
                (None, None)
            }
        };
        feed.close();
        if id.is_some() && crate::voice::tts::speaking_id() == id {
            crate::voice::tts::stop();
        }
        if let Some(ev) = ev {
            emit(&self.app, ev);
        }
        if carded {
            crate::bar::dismiss_card(&self.app, CARD);
        }
    }
}

impl crate::chat::stream::Sink for Streamed {
    fn send(&self, ev: crate::chat::stream::Event) {
        let (conversation, question) = {
            let st = self.lock();
            (st.conversation, st.question_id.clone())
        };
        if let crate::chat::stream::Event::Chips { web, .. } = &ev {
            self.lock().web = *web;
        }
        if let crate::chat::stream::Event::Text { text } = &ev {
            if text.trim().is_empty() {
                // Taken back: it came before a tool call.
                self.taken_back();
            } else {
                self.speak_ready(text);
            }
        }
        let _ = self.app.emit(
            STREAM,
            serde_json::json!({ "conversationId": conversation, "userMessageId": question, "event": ev }),
        );
    }
}

async fn answer(app: AppHandle, generation: u64, since: u64, question: String) {
    let cfg = crate::config::load();
    let viewing = view();
    let open_exists = matches!(viewing, Viewing::Conversation(id) if crate::chat::chats::get(id).is_some());
    let choice = choose_conversation(viewing, open_exists, crate::chat::chats::latest(), now_secs());
    // Screenshots waiting in Chat's box (snip/waiting.rs): the question goes
    // with them, to the conversation they wait in.
    let waiting = crate::snip::waiting::now();
    let shots_wait = !waiting.images.is_empty();
    let conversation_id = match choice {
        _ if shots_wait => waiting.conversation_id,
        Choice::Continue(id) => Some(id),
        Choice::New => None,
    };
    // "Open Teams" needs no model, nor a "yes" to Yap's "Did you mean …?":
    // Yap opens it and says so (apps.rs).
    let opens_an_app = crate::apps::plan_for(&question, &cfg, conversation_id).await.is_some();
    let endpoint = if opens_an_app {
        None
    } else {
        match crate::chat::turn::resolve_chat_endpoint(&cfg) {
            Ok(e) => Some(e),
            Err(why) => {
                crate::notices::alert::show_soon(&app, no_model_alert(&why));
                give_up(&app, generation);
                return;
            }
        }
    };
    // The screenshots, as a typed question takes them (`chat_send`): a model
    // known not to read images gets none (they stay in the box, the
    // question goes there too); else they leave the box and their copies
    // for this provider go with every call of the turn.
    let mut shots = crate::snip::turn::TurnImages::default();
    let mut taken: Option<(Option<u64>, Vec<crate::chat::chats::ChatImage>)> = None;
    let mut target: Option<(String, String)> = None;
    if let (true, Some((base_url, _key, model, provider, _persona, _thinking))) = (shots_wait, &endpoint) {
        if crate::snip::vision::reads_images(provider, model) == Some(false) {
            cant_read(&app, &question, waiting.conversation_id, false);
            give_up(&app, generation);
            return;
        }
        if let Some((conv, images)) = crate::snip::waiting::take_for_question(&app) {
            let ids: Vec<String> = images.iter().map(|i| i.id.clone()).collect();
            match crate::snip::turn::prepare(&ids, provider, base_url).await {
                Ok(prepared) => {
                    tracing::info!(images = prepared.images.len(), "Voice Assistant: a question with screenshots");
                    shots = prepared;
                    taken = Some((conv, images));
                    target = Some((provider.clone(), model.clone()));
                }
                Err(e) => {
                    tracing::warn!("Voice Assistant: the screenshots couldn't go ({e})");
                    crate::snip::waiting::put_back(&app, conv, images);
                    crate::notices::alert::show_soon(&app, failed_alert(&e));
                    give_up(&app, generation);
                    return;
                }
            }
        }
    }
    tracing::info!(?viewing, ?choice, shots = shots.images.len(), "Voice Assistant: asking Chat");
    // The person is looking at Yap's main window: Chat comes up there with
    // the question (and no card on the bar).
    let (_, focused) = crate::notices::notice::window_view(&app);
    let asked_app = app.clone();
    let asked_text = question.clone();
    // The answer as it's written: the Chat view follows it, the voice
    // starts at its first sentence.
    let streamed = Streamed {
        app: app.clone(),
        generation,
        since,
        question: question.clone(),
        voice_on: cfg.speak_replies,
        show_card: cfg.assistant_answer_card,
        s: Mutex::new(Speaking::default()),
    };
    let asked = |conv: u64, index: usize| {
        lock().conversation = Some(conv);
        {
            let mut st = streamed.lock();
            st.conversation = Some(conv);
            st.question_id = Some(format!("chat:{conv}:{index}"));
            st.message_id = Some(format!("chat:{conv}:{}", index + 1));
        }
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
    };
    // ■ (the capsule's, the card's ✕, Chat's box, Esc) closes the model's
    // stream too, not just the voice: a model on this PC stops writing.
    let stoppable = crate::chat::stream::Stoppable::new(Some(&stop_id(generation)));
    let turn = crate::snip::turn::with(
        shots,
        crate::chat::turn::chat_turn(
            Some(&app),
            conversation_id,
            &question,
            Some("assistant"),
            true,
            asked,
            Some(&streamed as &dyn crate::chat::stream::Sink),
            Some(&stoppable),
        ),
    );
    // Its AI calls and searches are the Voice Assistant's in the network
    // receipts (net.rs), though they run Chat's code.
    let turn = crate::privacy::net::within(crate::privacy::net::Feature::Assistant, turn).await;
    let (feed, said, carded_early, question_id, answer_id) = {
        let st = streamed.lock();
        (st.feed.clone(), st.said, st.carded, st.question_id.clone(), st.message_id.clone())
    };
    let turn = match turn {
        Ok(turn) => turn,
        // A newer question in its conversation replaced it (a barge-in, a
        // follow-up, Chat): nothing saved, nothing more said, no alert —
        // the newer question is the one being answered. The Chat view
        // takes its bubble away.
        Err(e) if e == crate::chat::stream::SUPERSEDED => {
            if let Some(feed) = &feed {
                feed.close();
                // Still saying what it had of this answer: not any more
                // (the newer answer's voice is its own).
                if answer_id.is_some() && crate::voice::tts::speaking_id() == answer_id {
                    crate::voice::tts::stop();
                }
            }
            tracing::info!("Voice Assistant: a newer question replaced this one");
            let conv = streamed.lock().conversation;
            let _ = app.emit(
                TURN,
                serde_json::json!({ "phase": "superseded", "conversationId": conv, "userMessageId": question_id }),
            );
            give_up(&app, generation);
            return;
        }
        Err(e) => {
            // Half an answer said: it stops there.
            if let Some(feed) = &feed {
                feed.close();
                crate::voice::tts::stop();
            }
            // ■ before any of the answer came: the question stays, nothing
            // went wrong.
            if e == crate::chat::stream::STOPPED {
                tracing::info!("Voice Assistant: stopped before the answer began");
                if let Some((conv, images)) = taken {
                    crate::snip::waiting::put_back(&app, conv, images);
                }
                let conv = streamed.lock().conversation.or(conversation_id);
                let _ = app.emit(
                    TURN,
                    serde_json::json!({ "phase": "failed", "conversationId": conv, "userMessageId": question_id, "error": e }),
                );
                give_up(&app, generation);
                return;
            }
            // Its screenshots back in the box, to ask again (as a typed
            // question's are); a server that turned the image down: that
            // model is a "no" now, and Chat says so, as for a typed one.
            if let Some((conv, images)) = taken {
                crate::snip::waiting::put_back(&app, conv, images);
                if let Some((provider, model)) = target.filter(|_| crate::snip::vision::is_image_refusal(&e)) {
                    tracing::info!(provider, "Voice Assistant: the model refused the screenshot");
                    crate::snip::vision::refused(&provider, &model);
                    let conv = streamed.lock().conversation.or(conversation_id);
                    let _ = app.emit(
                        TURN,
                        serde_json::json!({ "phase": "failed", "conversationId": conv, "userMessageId": question_id, "error": e }),
                    );
                    cant_read(&app, &question, conv, true);
                    give_up(&app, generation);
                    return;
                }
            }
            tracing::warn!("Voice Assistant: no answer ({e})");
            let conv = streamed.lock().conversation.or(lock().conversation);
            let _ = app.emit(
                TURN,
                serde_json::json!({ "phase": "failed", "conversationId": conv, "userMessageId": question_id, "error": e }),
            );
            if current(generation) {
                crate::notices::alert::show_soon(&app, failed_alert(&e));
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
            "userMessageId": format!("chat:{}:{}", turn.conversation_id, turn.user_index),
            "messageId": message_id,
            "show": focused,
        }),
    );
    // Already speaking (the answer was spoken as it came): the rest of what's
    // said, now the answer is whole and checked.
    if let Some(feed) = feed {
        finish_streamed(&app, generation, since, &question, &turn, &feed, said, carded_early, &cfg);
        return;
    }
    // A meeting or a call that started while the answer was coming: shown,
    // not read aloud.
    let meeting = crate::voice::tts::meeting_quiet().is_some();
    let interrupted = crate::dictation::pipeline::dictation_state().0 != since;
    // Read now: the switch may have moved while the answer was coming.
    let voice_on = crate::config::load().speak_replies;
    let mut how = reply(voice_on, meeting, interrupted);
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
    // An answer from the web: one or two sentences out loud (and on the
    // card), the details and sources in Chat.
    let (said, shown) = spoken_answer(&turn.reply, web_sources(&turn));
    let sources_note = sources_note(web_sources(&turn));
    let note = note.or(sources_note.as_deref());
    // The card first (the voice may be short), then the voice. With "Show
    // the answer on the Yap bar" off, a spoken answer has no card.
    let on_bar = crate::notices::notice::place(&app, false) == crate::notices::notice::Place::Bar;
    let mut carded = on_bar
        && card_on_bar(cfg.assistant_answer_card, how == Reply::Speak)
        && show_card(&app, &question, &shown, how == Reply::Speak, note);
    if how == Reply::Speak {
        if let Err(e) = crate::voice::tts::speak(&said, Some(message_id.clone())) {
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
                let ms = read_ms(&shown);
                crate::bar::update_card(&app, CARD, |c| {
                    c.timeout_ms = Some(ms);
                    c.status = Some(why);
                });
            } else if on_bar {
                // The card was left out for the voice, and there's no voice:
                // show the answer after all.
                carded = show_card(&app, &question, &shown, false, Some(why.as_str()));
            }
        }
    }
    tracing::info!(spoken = how == Reply::Speak, carded, meeting, interrupted, "Voice Assistant: answered");
}

/// The end of an answer the voice began while it was written ([`Streamed`]):
/// the rest of what's said ([`spoken_answer`] of the checked answer, after
/// the `said` sentences already handed over), or — a meeting, a dictation
/// or ■ since — nothing more; the card shows the whole short answer.
#[allow(clippy::too_many_arguments)]
fn finish_streamed(
    app: &AppHandle,
    generation: u64,
    since: u64,
    question: &str,
    turn: &crate::chat::turn::ChatTurn,
    feed: &crate::voice::tts::Feed,
    said: usize,
    carded: bool,
    cfg: &crate::config::YapConfig,
) {
    let meeting = crate::voice::tts::meeting_quiet().is_some();
    let interrupted = crate::dictation::pipeline::dictation_state().0 != since;
    let how = reply(crate::config::load().speak_replies, meeting, interrupted);
    let (said_text, shown) = spoken_answer(&turn.reply, web_sources(turn));
    let still = current(generation);
    if how == Reply::Speak && still {
        let rest: Vec<String> = crate::voice::tts::utterances(&said_text).into_iter().skip(said).collect();
        if !rest.is_empty() {
            feed.push(rest);
        }
        feed.close();
    } else {
        feed.close();
        // A meeting started while it was said: the voice stops, and the
        // answer stays on its card to read (not taken away with the voice).
        if meeting && still {
            let ev = {
                let mut s = lock();
                if s.generation == generation && s.phase == Phase::Speaking {
                    s.phase = Phase::Idle;
                    s.message_id = None;
                    Some(snapshot(&s))
                } else {
                    None
                }
            };
            if let Some(ev) = ev {
                emit(app, ev);
            }
            crate::voice::tts::stop();
        }
    }
    let note = if meeting {
        Some("Not read aloud during a meeting")
    } else if interrupted {
        Some("Not read aloud: you started dictating")
    } else {
        None
    };
    let sources_note = sources_note(web_sources(turn));
    let note = note.or(sources_note.as_deref());
    if !still {
        return;
    }
    {
        let mut s = lock();
        if s.generation == generation {
            s.conversation = Some(turn.conversation_id);
        }
    }
    if carded {
        let body = shown.clone();
        let status = note.map(str::to_string);
        // Not read aloud to the end: it stays a reading time, as a shown
        // answer does.
        let timeout = (how != Reply::Speak).then(|| read_ms(&shown));
        crate::bar::update_card(app, CARD, |c| {
            c.body = body;
            c.status = status;
            if timeout.is_some() {
                c.timeout_ms = timeout;
            }
        });
    } else {
        let on_bar = crate::notices::notice::place(app, false) == crate::notices::notice::Place::Bar;
        if on_bar && card_on_bar(cfg.assistant_answer_card, how == Reply::Speak) {
            show_card(app, question, &shown, how == Reply::Speak, note);
        }
    }
    tracing::info!(spoken = how == Reply::Speak, streamed = true, meeting, interrupted, "Voice Assistant: answered");
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
    if let Err(e) = crate::shell::window::show_settings(app) {
        tracing::warn!("Voice Assistant: couldn't open the main window ({e})");
        return;
    }
    let _ = app.emit(TURN, serde_json::json!({ "phase": "open", "conversationId": conv, "show": true }));
}

/// ■ on the capsule, ✕ or Esc on the card: the voice stops, an answer still
/// coming isn't said (it's in Chat), the card goes.
pub fn stop(app: &AppHandle) {
    let (ev, was_speaking, stopped) = {
        let mut s = lock();
        if matches!(s.phase, Phase::Thinking | Phase::Speaking) {
            let was = s.phase == Phase::Speaking;
            let stopped = s.generation;
            s.generation += 1;
            s.phase = Phase::Idle;
            s.message_id = None;
            (Some(snapshot(&s)), was, Some(stopped))
        } else {
            (None, false, None)
        }
    };
    // Its answer still being written: the stream closes (what was written
    // is kept in Chat, as Chat's own ■ keeps it).
    if let Some(generation) = stopped {
        crate::chat::stream::stop(&stop_id(generation), crate::chat::stream::Halt::Stopped);
    }
    if let Some(ev) = ev {
        tracing::info!("Voice Assistant: stopped");
        emit(app, ev);
    }
    if was_speaking {
        crate::voice::tts::stop();
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
/// where a spoken question goes ([`choose_conversation`]), and where the
/// screenshots in its box wait (`snip/waiting.rs`). `new_chat`: no
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
    // The screenshots waiting in its box wait in what it shows.
    crate::snip::waiting::viewing(open, conversation_id);
}

/// ■: stop the answer (see [`stop`]).
#[tauri::command]
pub fn assistant_stop(app: AppHandle) {
    stop(&app);
}

/// Where the Voice Assistant is now (`yap-assistant`'s phase), for a Chat
/// view that opened after it was said: its ■ shows while it thinks or speaks.
#[tauri::command]
pub fn assistant_phase() -> Phase {
    lock().phase
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
    if !crate::shell::e2e::active() {
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
    fn the_shortcut_speaks_only_while_the_voice_is_on() {
        // Voice on: read aloud, except over a meeting or a new dictation.
        assert_eq!(reply(true, false, false), Reply::Speak);
        assert_eq!(reply(true, true, false), Reply::Show);
        assert_eq!(reply(true, false, true), Reply::Show);
        assert_eq!(reply(true, true, true), Reply::Show);
        // Voice off ("Speak replies" / Chat's Voice replies): never.
        assert_eq!(reply(false, false, false), Reply::Show);
        assert_eq!(reply(false, true, false), Reply::Show);
        // Chat's own rule keeps quiet with it off too.
        assert!(!crate::voice::tts::auto_speak(false, "voice", true, false, false));
        assert!(!crate::voice::tts::auto_speak(false, "always", true, false, false));
        // Both keep quiet over a meeting.
        assert!(!crate::voice::tts::auto_speak(true, "always", true, false, true));
    }

    #[test]
    fn a_web_answer_is_said_short_with_the_rest_in_chat() {
        let reply = "Gale is on the beach west of the Nautiloid crash site [1]. Pull him out of the portal to recruit him [1][2].\n\n## Details\n- He offers to join you [2].";
        let (said, shown) = spoken_answer(reply, 2);
        assert_eq!(
            said,
            "Gale is on the beach west of the Nautiloid crash site. Pull him out of the portal to recruit him. The details and sources are in Chat."
        );
        assert!(!said.contains('['));
        assert_eq!(shown, "Gale is on the beach west of the Nautiloid crash site. Pull him out of the portal to recruit him.");
        // Not from the web: all of it, as before.
        assert_eq!(spoken_answer("Your notes say Friday.", 0), ("Your notes say Friday.".into(), "Your notes say Friday.".into()));
    }

    #[test]
    fn a_long_answer_is_said_short_with_the_rest_in_chat() {
        // Qwen3.5-4B's 87-word egg answer (the model evaluations).
        let reply = "I don't have a specific note on this, but for a runny yolk: - Start with a cold egg in a pot of cold water - Bring to a boil (about 5-7 minutes) - Once boiling, reduce to a simmer and cook for **6-7 minutes total** - Remove from water and let sit in ice water for 1-2 minutes to stop cooking. The exact time can vary slightly based on egg size and water temperature, so 6-7 minutes is a good starting point.";
        assert!(reply.split_whitespace().count() > SPOKEN_MAX_WORDS);
        let (said, shown) = spoken_answer(reply, 0);
        assert!(said.ends_with(REST_IN_CHAT), "{said}");
        assert!(said.split_whitespace().count() < reply.split_whitespace().count(), "{said}");
        assert!(!shown.is_empty() && reply.starts_with(&shown[..10]), "{shown}");
        // A short one is said whole.
        let short = "Boil it for six to seven minutes, then cool it in cold water.";
        assert_eq!(spoken_answer(short, 0).0, short);
    }

    #[test]
    fn a_price_in_a_spoken_web_answer_waits_for_the_check() {
        for s in ["ChatGPT Plus costs $20 a month.", "It's £8.99 now.", "About 15 euros.", "Roughly 20 USD a seat.", "It's 99p."] {
            assert!(mentions_price(s), "{s}");
        }
        for s in ["It starts at 8 tonight.", "Version 3.12 is out.", "Gale is on the beach.", "Use the $PATH variable."] {
            assert!(!mentions_price(s), "{s}");
        }
    }

    #[test]
    fn a_shown_only_answer_stays_long_enough_to_read() {
        assert_eq!(read_ms("Yes."), 8_000);
        assert_eq!(read_ms(&"word ".repeat(60)), 20_000);
        assert_eq!(read_ms(&"x".repeat(5_000)), 60_000);
    }
}

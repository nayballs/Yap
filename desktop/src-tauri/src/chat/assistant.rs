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
//!    question would (a new code chat on screen, its workspace picked: the
//!    new conversation is a Code chat there, [`new_code_chat`]). The Chat
//!    view follows live (`yap-assistant-turn`), the question marked as asked
//!    by voice; with the main window in front, Chat comes up with it. The capsule says what the answer is doing in
//!    "Thinking…"'s place — "Searching the web · …", "Reading …"
//!    (`yap-assistant-step`, `chat/activity.rs`).
//! 3. **Speaking**: the answer is read aloud whatever "When to speak" says
//!    (that's the shortcut's point), in the chosen voice — never while a
//!    meeting records (its recorder would hear it) and not when a dictation
//!    started while it was thinking ([`reply`]). Unless the answer is in
//!    view in Chat — the main window in front, or on screen beside the app
//!    in front showing the conversation ([`answer_in_view`]) — a quiet card
//!    on the Yap bar shows the answer too; it goes when the voice ends, on ■
//!    (the capsule), ✕ or Esc. Pressing the shortcut again asks a follow-up in the same
//!    conversation. As much of the answer is said as "How much to read
//!    aloud" says ([`SpokenLength`]: a sentence or two, about 30 seconds, a
//!    minute — the default — or 2 minutes, or all of it), citation markers
//!    left out; cut short, "The rest is in Chat." (an answer written from
//!    the web: "The details and sources are in Chat.", [`spoken_answer`]);
//!    the card shows what's said and "3 sources in Chat".
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
//! **The passage quoted over Chat's box** (a document's "Change this…" /
//! "Ask about this", 2026-10-31) goes with a spoken question as it does
//! with a typed one — the model is told its blocks, the question keeps its
//! quote, the box's quote goes — when Chat is in view as it's asked and
//! the question goes to the conversation it's quoted in
//! (`documents::quote_goes`): looking at the quote and saying "make this
//! level 21 to 22 only" is asking about it. Asked from another app, it
//! stays in the box. No answer: it's back over the box. Nor is the window
//! in front looked at then: the passage is what it's about.
//!
//! **It sees the window in front** (2026-10-29, [`screen`]): a question
//! whose words point at the screen ("what's this error?", "summarise this
//! page", by cheap rules in the question's language) takes a screenshot of
//! the window that was in front when the shortcut was pressed at once,
//! every Yap window left out; on a model on this PC by default, a cloud one
//! only when allowed (asked once). A model that can't read images gets the
//! window's text, read on this PC. The capsule says "Looking at the
//! window…" meanwhile. **Nothing else is guessed** (8 Nov 2026: a lone
//! "that" once went to a yes/no request and sent the game in front with a
//! follow-up about a convention): asked from another app, the answering
//! model may look at that window itself when the question needs it
//! (`look_at_screen`, [`may_look_at_window`]), with the same switches.
//!
//! **About text selected in another app** (2026-10-31, [`selected`]; Wispr
//! Flow's Command Mode, its "Lens"): the rewrite shortcut on a selection with
//! a question said instead of an edit (`dictation/ask_or_edit.rs`), or the
//! **Ask about the selection** shortcut, asks here ([`ask_about`]) with the
//! selection as the question's quote and in the model's context — never the
//! window in front, the screenshots waiting in Chat's box, the document's
//! quoted passage or an app to open: the selection is what it's about.
//! Nothing is typed into the app.
//!
//! **Skills by voice** (2026-11-03, [`skill`]): a Skill said — its slash
//! name ("slash follow-up email"), its name with "skill" ("run my standup
//! skill"), or a request in its own words ("prep me for my next meeting") —
//! runs as Chat runs it (`skills::with`): its passages, its instructions,
//! the citations in the conversation, a document for /tickets and /prd
//! ("It's open on the right." with Chat in view, else "It's in Chat."). The
//! answer is Chat's whole, and the voice says as much as "How much to read
//! aloud" says. With nothing to run on, Yap asks out loud and the next
//! spoken question in that conversation is the answer. Not the window in
//! front, the screenshots in Chat's box, a quoted passage or an app to open.
//!
//! **Asked from a phone** (Yap Link, `link.rs`, 2026-10-31): a question a
//! paired phone heard goes the same way ([`ask_from_phone`]) — the same
//! conversation choice, Chat's model and the short spoken answer, kept in
//! Chat, the Chat view following it live — with the phone in the bar's
//! place: its own turn ([`Phone`]: interrupted from the phone, never the
//! capsule's generation), the answer's text and steps sent to it, and its
//! voice on the phone ([`Phone::voice`]), never this PC's speakers. Not the
//! window in front, the screenshots waiting in Chat's box, a quoted passage
//! or the bar's card: those are the PC's. Off during meetings, as here. A
//! question **typed** on the phone (2026-11-03, [`Phone::by_voice`] false)
//! goes the same way minus the audio: in the same conversation, answered as
//! Chat answers a typed question (no `SPOKEN_ANSWER_NOTE`, not marked "by
//! voice"), and not said — ▶ on the phone asks for that. Talk's words
//! checked in the phone's box and sent (`ask {speak}`, that evening) are a
//! spoken question again: by voice, the note, said on the phone.
//!
//! Events: `yap-assistant {phase, conversationId, messageId, about?}` (the bar's
//! capsule), `yap-assistant-turn {phase: asked | answered | failed |
//! superseded | open, conversationId, userMessageId, text, show}` (the main
//! window: `show` = bring the Chat view up; `userMessageId` = which question,
//! "chat:<conversation>:<index>") and `yap-assistant-stream {conversationId,
//! userMessageId, event}` (the Chat view follows the answer as it's written:
//! a `stream::Event`).

pub mod connector_ask;
pub mod screen;
pub mod selected;
pub mod skill;
pub mod spoken_ok;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
/// The bar's capsule shows what the answer is doing ("Searching the web ·
/// …" in "Thinking…"'s place, `chat/activity.rs`): its own small event, so
/// the bar never gets the answer's text.
const STEP: &str = "yap-assistant-step";
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

/// **A new Code chat's workspace** for a spoken question (2026-11-07): Chat
/// shows a new code chat, its workspace picked and no question yet
/// (`picked`, `chat_view_state`'s `codeFolder`), and the question makes a
/// new conversation (`conversation`: none to continue) — it's that Code
/// chat, attached as it's made, as a typed first question's is
/// (`agents::code_chat::with_new`). Never a phone's (`pc`: the shortcut's).
/// Pure.
pub fn new_code_chat(pc: bool, conversation: Option<u64>, picked: Option<String>) -> Option<String> {
    picked.filter(|f| pc && conversation.is_none() && !f.trim().is_empty())
}

// ---- which side a spoken question goes to (its cleanup's switch) -------------------------

/// **Does a spoken question go into a Code chat?** (2026-11-07: each side
/// has its own "Clean up what you say…" switch,
/// `cleanup_styles::cleans_question`.) Yap Code on, and the conversation it
/// continues is a Code chat (`is_code`: `code_chat::folder_of`), or it makes
/// a new one in a workspace (`new_code`: a new code chat waiting with its
/// workspace, [`new_code_chat`]). Else it's Chat's. Pure.
pub fn into_code_chat(agents_on: bool, conversation: Option<u64>, new_code: Option<&str>, is_code: impl Fn(u64) -> bool) -> bool {
    agents_on && (new_code.is_some_and(|f| !f.trim().is_empty()) || conversation.is_some_and(is_code))
}

fn is_code_chat(id: u64) -> bool {
    crate::agents::code_chat::folder_of(Some(id)).is_some()
}

/// Where the shortcut's question being transcribed will go, as [`answer`]
/// chooses it: the conversation it continues (the screenshots waiting in
/// Chat's box: theirs, unless it's about a selection — a Skill said doesn't
/// take them either, but they wait in what Chat shows, the one it continues
/// anyway), and whether that's a Code chat — a new code chat on screen
/// making one ([`into_code_chat`]). Read before the question's cleanup,
/// which follows its side's switch (`pipeline::ask_assistant`).
pub(crate) fn shortcut_goes(cfg: &crate::config::YapConfig, about_selection: bool) -> (Option<u64>, bool) {
    let viewing = view();
    let open_exists = matches!(viewing, Viewing::Conversation(id) if crate::chat::chats::get(id).is_some());
    let continued = match choose_conversation(viewing, open_exists, crate::chat::chats::latest(), now_secs()) {
        Choice::Continue(id) => Some(id),
        Choice::New => None,
    };
    let waiting = crate::snip::waiting::now();
    let conversation = if !about_selection && !waiting.images.is_empty() { waiting.conversation_id } else { continued };
    let new_code = new_code_chat(true, conversation, code_chat_shown());
    (conversation, into_code_chat(cfg.agents_enabled, conversation, new_code.as_deref(), is_code_chat))
}

/// Is a dictation into the Chat view's box (its mic) going into a Code chat:
/// the one Chat shows, or the new code chat it shows with its workspace?
pub(crate) fn chat_box_in_code_chat(cfg: &crate::config::YapConfig) -> bool {
    match view() {
        Viewing::Conversation(id) => into_code_chat(cfg.agents_enabled, Some(id), None, is_code_chat),
        Viewing::New => into_code_chat(cfg.agents_enabled, None, code_chat_shown().as_deref(), is_code_chat),
        Viewing::Closed => false,
    }
}

/// Is a phone's question (`wanted`: the conversation it said, Yap Link)
/// going into a Code chat, as [`answer`] chooses its conversation
/// ([`phone_conversation`])? (A new Code chat a phone made with Remote
/// Control's `code_new` is its session's to say: `link/session.rs`.)
pub(crate) fn phone_in_code_chat(cfg: &crate::config::YapConfig, wanted: crate::link::wire::ChatRef) -> bool {
    let viewing = view();
    let open_exists = matches!(viewing, Viewing::Conversation(id) if crate::chat::chats::get(id).is_some());
    let chosen = choose_conversation(viewing, open_exists, crate::chat::chats::latest(), now_secs());
    let usable = matches!(wanted, crate::link::wire::ChatRef::Id(id)
        if crate::chat::chats::get(id).is_some_and(|c| crate::link::chats::usable(&c)));
    let conversation = match phone_conversation(wanted, usable, chosen) {
        Choice::Continue(id) => Some(id),
        Choice::New => None,
    };
    into_code_chat(cfg.agents_enabled, conversation, None, is_code_chat)
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

/// **Is the answer already in front of the person, in Chat?** Yap's main
/// window in front (Chat comes up there with the question), or on screen
/// beside the app in front — another monitor, mostly uncovered — with Chat
/// showing the question's `conversation` (or a new chat, which follows the
/// question to its new conversation). Minimised, hidden, behind another
/// app or on another view: no. The card on the Yap bar is for someone who
/// can't see Chat (30 Oct 2026: it came up over Chat, maximised, with the
/// answer streaming in front of the maintainer). `seen` is the window's
/// real state, from Rust (`notice::window_seen`); `viewing`, what the Chat
/// view says it shows. Pure.
pub fn answer_in_view(seen: crate::notices::notice::Seen, viewing: Viewing, conversation: Option<u64>) -> bool {
    use crate::notices::notice::Seen;
    match seen {
        Seen::Front => true,
        Seen::Beside => match viewing {
            Viewing::Conversation(id) => conversation == Some(id),
            Viewing::New => true,
            Viewing::Closed => false,
        },
        Seen::No => false,
    }
}

/// Does the answer get its card on the Yap bar? Never while it's in view in
/// Chat ([`answer_in_view`]); else as the person chose ("Show the answer on
/// the Yap bar", `assistant_answer_card`) while it's read aloud, and always
/// when it isn't (`spoken` false: a meeting, a dictation started, the
/// voice failed), or the answer would be lost. Pure.
pub fn card_on_bar(show_answer_card: bool, spoken: bool, in_view: bool) -> bool {
    !in_view && (show_answer_card || !spoken)
}

/// **How much of an answer Yap reads out by itself** — "Read aloud" in
/// Settings → Voice Assistant → Spoken replies (`spoken_length`): the
/// assistant shortcut's answers and Chat's voice replies. ▶ Read aloud on
/// a message always reads all of it. The model is still asked to keep a
/// spoken answer short (`SPOKEN_ANSWER_NOTE`); this is how much of what it
/// wrote is said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpokenLength {
    /// "A sentence or two": the answer's first two sentences.
    Short,
    /// "About 30 seconds" / "About a minute" / "About 2 minutes": all of
    /// it up to this many words (~150 a minute), else its paragraphs from
    /// the first while they fit (`tts::lead`).
    Words(usize),
    /// "The whole answer".
    Whole,
}

impl SpokenLength {
    /// The setting's value ("short", "30s", "1m", "2m", "whole"); anything
    /// else is the default, about a minute.
    pub fn from_setting(value: &str) -> SpokenLength {
        match value {
            "short" => SpokenLength::Short,
            "30s" => SpokenLength::Words(SPOKEN_MAX_WORDS / 2),
            "2m" => SpokenLength::Words(SPOKEN_MAX_WORDS * 2),
            "whole" => SpokenLength::Whole,
            _ => SpokenLength::Words(SPOKEN_MAX_WORDS),
        }
    }

    /// The configured one.
    pub fn configured(cfg: &crate::config::YapConfig) -> SpokenLength {
        SpokenLength::from_setting(&cfg.spoken_length)
    }
}

/// The part of an answer said before "The rest is in Chat.", plain text
/// without markdown or citation markers; `None`: all of it is said. Pure.
fn spoken_part(reply: &str, length: SpokenLength) -> Option<String> {
    let words = |s: &str| s.split_whitespace().count();
    let part = match length {
        SpokenLength::Whole => return None,
        SpokenLength::Short => crate::voice::tts::brief(reply, 2),
        SpokenLength::Words(max) if words(reply) <= max => return None,
        SpokenLength::Words(max) => crate::voice::tts::lead(reply, max),
    };
    (!part.is_empty() && words(&part) < words(reply)).then_some(part)
}

/// What the assistant says, and what its card shows, for an answer: as
/// much as `length` says (all of it, or its start then "The rest is in
/// Chat."; written from the web, `sources` > 0: "The details and sources
/// are in Chat."). The card shows what's said; a web answer's without its
/// `[n]` markers. Pure.
pub fn spoken_answer(reply: &str, sources: usize, length: SpokenLength) -> (String, String) {
    if sources > 0 && crate::voice::tts::brief(reply, 2).is_empty() {
        return ("The answer and its sources are in Chat.".to_string(), reply.to_string());
    }
    let after = if sources > 0 { DETAILS_IN_CHAT } else { REST_IN_CHAT };
    match spoken_part(reply, length) {
        Some(part) => (format!("{part} {after}"), part),
        None if sources > 0 => (reply.to_string(), crate::voice::tts::lead(reply, usize::MAX)),
        None => (reply.to_string(), reply.to_string()),
    }
}

/// What Chat says for a voice reply (`chat_send`'s `spokenText`): as much
/// as `length` says, then "The rest is on screen." Pure.
pub fn spoken_in_chat(reply: &str, length: SpokenLength) -> String {
    match spoken_part(reply, length) {
        Some(part) => format!("{part} {REST_ON_SCREEN}"),
        None => reply.to_string(),
    }
}

/// Said after a web answer's short version.
pub const DETAILS_IN_CHAT: &str = "The details and sources are in Chat.";

/// Said after the start of a long voice reply in Chat.
pub const REST_ON_SCREEN: &str = "The rest is on screen.";

/// How many of an answer's sources are web pages: a calendar answer's
/// meetings (`chat/tools/calendar.rs`) are said whole, like any answer
/// ("You've the design review at 2."), not cut short for their sources.
pub(crate) fn web_sources(turn: &crate::chat::turn::ChatTurn) -> usize {
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

/// About a minute of speech, the default [`SpokenLength`]. An answer longer
/// than 60 words was said short until 2026-10-26: a 95-word answer in two
/// paragraphs stopped after its second sentence, mid-thought.
pub const SPOKEN_MAX_WORDS: usize = 150;

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
    /// (after a spoken "snip": "Ask about this screenshot…"), "selection"
    /// (the Ask about the selection shortcut: "Ask about the selection…");
    /// while it's thought about, "window" (the window in front being taken
    /// and read: "Looking at the window…", [`screen`]).
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
/// The workspace of the new code chat Chat shows (no question yet): a
/// spoken question's new conversation is a Code chat there
/// ([`new_code_chat`]).
static NEW_CODE_CHAT: Mutex<Option<String>> = Mutex::new(None);

/// Never held while emitting, touching windows or another module's lock.
fn lock() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|p| p.into_inner())
}

fn view() -> Viewing {
    *VIEW.lock().unwrap_or_else(|p| p.into_inner())
}

fn code_chat_shown() -> Option<String> {
    NEW_CODE_CHAT.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

/// What the Chat view shows (Snip to Chat puts a screenshot in the
/// conversation open there, `snip.rs`).
pub fn chat_viewing() -> Viewing {
    view()
}

tokio::task_local! {
    /// The Voice Assistant's question running here was asked with Yap's
    /// main window in front: Chat comes up there with it, and a document
    /// opens beside it where the person is looking.
    static IN_CHAT: bool;
    /// The Voice Assistant's question running here was asked with its
    /// shortcut from another app, nothing else taken or quoted with it: its
    /// model may look at the window that was in front ([`may_look_at_window`]).
    static WINDOW_LOOK: bool;
}

/// **May the turn's model look at the window in front?** (8 Nov 2026: the
/// words no longer guess.) Only for the shortcut's question asked from
/// another app (not with Chat in view: there it looks at the screen as
/// Chat does), with no screenshots waiting, window already taken, quote,
/// selection or Skill going with it — never a phone's question. The turn's
/// gates offer `look_at_screen` on it (`turn/gates.rs`, `see_screen::Takes::Window`).
/// `false` outside one.
pub fn may_look_at_window() -> bool {
    WINDOW_LOOK.try_with(|b| *b).unwrap_or(false)
}

/// Was the Voice Assistant's question this turn answers asked with Chat in
/// view ([`answer_in_view`] when it was asked: the main window in front, or
/// beside the app in front showing Chat)? Then its document's line points
/// at the panel ("It's open on the right.", `documents::spoken_note`), not
/// at Chat ("It's in Chat." to someone reading Chat, the maintainer's first
/// try, 30 Oct 2026). `false` outside one.
pub fn asked_in_chat() -> bool {
    IN_CHAT.try_with(|b| *b).unwrap_or(false)
}

/// Is question `conversation`'s answer in view in Chat now
/// ([`answer_in_view`] on the window's real state)? Window getters: never
/// with a lock held.
fn answer_seen(app: &AppHandle, conversation: Option<u64>) -> bool {
    answer_in_view(crate::notices::notice::window_seen(app), view(), conversation)
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
/// earlier question was doing is superseded; its card goes. `selection`:
/// the Ask about the selection shortcut's question ("Ask about the
/// selection…").
pub fn listening(app: &AppHandle, selection: bool) {
    let ev = {
        let mut s = lock();
        s.generation += 1;
        s.phase = Phase::Listening;
        s.message_id = None;
        s.conversation = None;
        let screenshot = ABOUT_SCREENSHOT.swap(false, Ordering::SeqCst);
        s.about = if selection {
            Some("selection")
        } else {
            screenshot.then_some("screenshot")
        };
        snapshot(&s)
    };
    tracing::info!(about_screenshot = ev.about == Some("screenshot"), about_selection = selection, "Voice Assistant: listening");
    emit(app, ev);
    crate::bar::dismiss_card(app, CARD);
}

/// **A question about text selected in another app** ([`selected`]): the
/// rewrite shortcut's words were a question (`dictation/ask_or_edit.rs`),
/// or the Ask about the selection shortcut's question, transcribed and
/// corrected. Asked as a spoken question is ([`heard`]), "Thinking…" on the
/// capsule, with the selection as its quote; nothing pasted.
pub fn ask_about(app: &AppHandle, question: String, about: selected::Selected) {
    let (generation, ev) = {
        let mut s = lock();
        // The rewrite shortcut's recording was never the assistant's: this
        // is a question of its own.
        s.generation += 1;
        s.phase = Phase::Thinking;
        s.message_id = None;
        s.conversation = None;
        s.about = None;
        (s.generation, snapshot(&s))
    };
    tracing::info!(chars = about.text.chars().count(), question = question.chars().count(), "Voice Assistant: a question about the selected text");
    emit(app, ev);
    crate::bar::dismiss_card(app, CARD);
    let since = crate::dictation::pipeline::dictation_state().0;
    let app = app.clone();
    tauri::async_runtime::spawn(async move { answer(app, Asker::Shortcut(generation), since, question, Some(about)).await });
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
    tauri::async_runtime::spawn(async move { answer(app, Asker::Shortcut(generation), since, question, None).await });
}

/// The window in front is being taken and read for question `generation`
/// (`on`), or that's done: the capsule says "Looking at the window…"
/// meanwhile.
fn looking(app: &AppHandle, generation: u64, on: bool) {
    let ev = {
        let mut s = lock();
        if s.generation != generation || s.phase != Phase::Thinking {
            return;
        }
        s.about = on.then_some("window");
        snapshot(&s)
    };
    emit(app, ev);
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

// ---- who asked ------------------------------------------------------------------------

/// **A question asked from a phone** (Yap Link, `link/session.rs`): what the
/// phone's turn takes from the Voice Assistant's path, in the bar's place.
pub trait Phone: Send + Sync {
    /// Still the phone's latest question (not interrupted, not replaced).
    fn current(&self) -> bool;
    /// What its answer's stream stops by (`stream::Stoppable`).
    fn stop_id(&self) -> String;
    /// What the answer is doing: Chat's activity line (`activity.rs`).
    fn step(&self, label: &str);
    /// The answer so far ("" = taken back); `done`: the checked answer.
    fn answer(&self, text: &str, done: bool);
    /// The checked answer, with its id in Chat (`chat:<conversation>:<index>`).
    fn checked(&self, text: &str, message_id: &str) {
        let _ = message_id;
        self.answer(text, true);
    }
    /// Asked out loud (then its answer is said, on the phone) — Talk's words
    /// sent from the phone's box too (`ask {speak}`); `false`: typed on the
    /// phone (2026-11-03) — answered as Chat answers a typed question, not
    /// said (the phone's ▶ asks for that).
    fn by_voice(&self) -> bool {
        true
    }
    /// Start the phone's voice for answer `message_id`: its sentences go
    /// through the feed, its audio to the phone.
    fn voice(&self, message_id: &str) -> Result<std::sync::Arc<crate::voice::tts::Feed>, String>;
    /// It went no further: why, in plain words.
    fn failed(&self, message: &str);
    /// The conversation it goes to, as the phone said (2026-11-03: the
    /// phone keeps its own, [`phone_conversation`]).
    fn chat(&self) -> crate::link::wire::ChatRef {
        crate::link::wire::ChatRef::Choose
    }
    /// It went to conversation `conversation` (a new chat's, or the one
    /// chosen for it).
    fn asked(&self, conversation: u64) {
        let _ = conversation;
    }
    /// The pictures it carries (kept as Chat keeps a pasted picture: their
    /// ids), and whether their text goes instead ("Send its text instead").
    fn pictures(&self) -> (Vec<String>, bool) {
        (Vec::new(), false)
    }
    /// Chat's model can't read its pictures (or its server turned them
    /// down): why, for the phone to offer Chat's ways on.
    fn cant_read(&self, message: &str) {
        self.failed(message);
    }
    /// Its question was saved as message `index` of `conversation`
    /// (Remote Control: an agent it asks for is the phone's, `agents/remote.rs`).
    fn question_saved(&self, conversation: u64, index: usize) {
        let _ = (conversation, index);
    }
    /// The workspace a new Code chat waits on (Remote Control's `code_new`):
    /// its question makes a Code chat there. Taken once.
    fn new_code_folder(&self) -> Option<String> {
        None
    }
}

/// **The conversation a phone's question goes to** (Yap Link, 2026-11-03,
/// the phone's own conversation): the one it's in while that's still one it
/// may continue (`usable`: in Chat's list, not archived or deleted); a new
/// one when it asked for a new chat — or when its conversation went; and,
/// when it never said, the Voice Assistant's own choice (`otherwise`,
/// [`choose_conversation`]). Pure.
pub fn phone_conversation(wanted: crate::link::wire::ChatRef, usable: bool, otherwise: Choice) -> Choice {
    use crate::link::wire::ChatRef;
    match wanted {
        ChatRef::Choose => otherwise,
        ChatRef::New => Choice::New,
        ChatRef::Id(id) if usable => Choice::Continue(id),
        ChatRef::Id(_) => Choice::New,
    }
}

/// Who asked a question.
#[derive(Clone)]
enum Asker {
    /// The assistant shortcut on this PC, question `generation`: the bar's
    /// capsule and card, this PC's speakers.
    Shortcut(u64),
    /// A paired phone (Yap Link).
    Phone(std::sync::Arc<dyn Phone>),
}

impl Asker {
    fn current(&self) -> bool {
        match self {
            Asker::Shortcut(g) => current(*g),
            Asker::Phone(p) => p.current(),
        }
    }

    fn stop_id(&self) -> String {
        match self {
            Asker::Shortcut(g) => stop_id(*g),
            Asker::Phone(p) => p.stop_id(),
        }
    }

    fn shortcut(&self) -> Option<u64> {
        match self {
            Asker::Shortcut(g) => Some(*g),
            Asker::Phone(_) => None,
        }
    }

    /// Asked out loud: the shortcut always, a phone unless it was typed.
    fn by_voice(&self) -> bool {
        match self {
            Asker::Shortcut(_) => true,
            Asker::Phone(p) => p.by_voice(),
        }
    }

    /// Is the answer said: the shortcut's as "Speak replies" says, a phone's
    /// when it was asked out loud (it was asked into a headset; a typed one
    /// is said only when ▶ on the phone asks).
    fn voiced(&self, speak_replies: bool) -> bool {
        match self {
            Asker::Shortcut(_) => speak_replies,
            Asker::Phone(p) => p.by_voice(),
        }
    }

    /// Back to idle on the bar (a phone's turn ends on its own).
    fn give_up(&self, app: &AppHandle) {
        if let Asker::Shortcut(g) = self {
            give_up(app, *g);
        }
    }

    /// An alert on this PC, or the same words on the phone.
    fn alert(&self, app: &AppHandle, alert: crate::notices::alert::Alert) {
        match self {
            Asker::Shortcut(_) => crate::notices::alert::show_soon(app, alert),
            Asker::Phone(p) => p.failed(&format!("{}. {}", alert.title.trim_end_matches('.'), alert.body)),
        }
    }

    /// A phone's pictures, and whether their text goes instead.
    fn pictures(&self) -> (Vec<String>, bool) {
        match self {
            Asker::Shortcut(_) => (Vec::new(), false),
            Asker::Phone(p) => p.pictures(),
        }
    }
}

/// **A question from a phone** (Yap Link), transcribed and corrected: asked
/// as the shortcut's is ([`answer`]), the phone in the bar's place. Returns
/// once the answer is in (its voice may still be going on the phone).
pub(crate) async fn ask_from_phone(app: AppHandle, question: String, phone: std::sync::Arc<dyn Phone>) {
    let since = crate::dictation::pipeline::dictation_state().0;
    answer(app, Asker::Phone(phone), since, question, None).await;
}

/// **A spoken yes or no to a question that asks first** waiting in
/// `conversation` (spoken_ok.rs: a connector's tool, an action on a page, an
/// agent): question `now` (the shortcut's, just heard) answers it — Allow
/// once, Don't, or "That one needs a click…" —
/// and goes no further: the capsule is back on the question that asked
/// (its generation, so its answer is said and ■ stops it), whose dictations
/// count now includes this recording. `false`: it isn't one (asked as
/// usual).
fn spoken_ok_answer(app: &AppHandle, now: u64, conversation: Option<u64>, question: &str, cfg: &crate::config::YapConfig) -> bool {
    let langs = crate::dictation::languages::my_languages(cfg);
    let meeting = crate::chat::tools::connectors::in_meeting();
    let Some((said, w)) = spoken_ok::take(conversation, question, &langs, meeting) else { return false };
    match said {
        spoken_ok::Spoken::Yes | spoken_ok::Spoken::No => {
            let yes = said == spoken_ok::Spoken::Yes;
            // Allow once (never Always allow by voice), whatever the card.
            let took = crate::chat::pages::act::confirm::chat_page_confirm(w.card, yes);
            tracing::info!(question = w.card, yes, took, "Voice Assistant: a question that asks first, answered out loud");
            if !took {
                // Answered meanwhile (the bar, Chat) or ended: a question of its own.
                return false;
            }
            crate::bar::dismiss_card(app, spoken_ok::CARD);
        }
        spoken_ok::Spoken::NeedsClick => {
            tracing::info!(question = w.card, "Voice Assistant: a spoken yes to a question that needs a click");
            if crate::voice::tts::meeting_quiet().is_none() {
                let _ = crate::voice::tts::speak(spoken_ok::NEEDS_CLICK, Some(format!("ask-first:{}", w.card)));
            }
        }
    }
    // The question that asked is the one under way again.
    w.since.store(crate::dictation::pipeline::dictation_state().0, Ordering::SeqCst);
    let ev = {
        let mut s = lock();
        if s.generation != now {
            return true;
        }
        s.generation = w.generation;
        s.phase = Phase::Thinking;
        s.message_id = None;
        s.conversation = Some(w.conversation);
        s.about = None;
        snapshot(&s)
    };
    emit(app, ev);
    true
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
/// the answer is whole), with the card on the Yap bar — or, asked from a
/// phone, the phone follows it and its voice is the phone's.
struct Streamed {
    app: AppHandle,
    asker: Asker,
    /// Dictations started when the question was heard (brought up to date
    /// when a spoken yes or no answers a connector's question meanwhile:
    /// that recording was the person answering, not moving on).
    since: std::sync::Arc<AtomicU64>,
    /// Its turn, for a question asked out loud before a call that asks
    /// first ([`spoken_ok::Waiting::owner`]).
    owner: u64,
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
            && self.asker.current()
            && crate::voice::tts::meeting_quiet().is_none()
            && crate::dictation::pipeline::dictation_state().0 == self.since()
    }

    /// Dictations started when the question was heard (or since answered).
    fn since(&self) -> u64 {
        self.since.load(Ordering::SeqCst)
    }

    /// **A call asks first, asked out loud** ([`spoken_ok`]: a connector's
    /// tool, an action on a page, an agent): Yap says its one line (when it
    /// may speak), the Yap bar shows Allow once / Don't (unless the answer's
    /// in view in Chat, where the thread's card is), and the next spoken
    /// question in this conversation may answer it. Answered (or ended): the
    /// bar's card goes. Only the shortcut's questions: a phone's never asks.
    fn asks_first(&self, card: &crate::chat::pages::act::confirm::Card) {
        let Some(generation) = self.asker.shortcut() else { return };
        if card.state != "asking" {
            if spoken_ok::answered(card.id) {
                crate::bar::dismiss_card(&self.app, spoken_ok::CARD);
            }
            return;
        }
        let Some(asking) = spoken_ok::asking(card, crate::chat::tools::connectors::in_meeting()) else { return };
        let Some(conversation) = self.lock().conversation else { return };
        spoken_ok::wait(spoken_ok::Waiting {
            conversation,
            card: card.id,
            spoken: asking.spoken,
            at: std::time::Instant::now(),
            generation,
            since: std::sync::Arc::clone(&self.since),
            owner: self.owner,
        });
        tracing::info!(question = card.id, act = %card.act, spoken = asking.spoken, "Voice Assistant: a call asks first; said, and on the bar");
        if self.may_speak() {
            if let Err(e) = crate::voice::tts::speak(&asking.line, Some(format!("ask-first:{}", card.id))) {
                tracing::info!("Voice Assistant: couldn't say the question ({e})");
            }
        }
        if crate::bar::cards_available() && !answer_seen(&self.app, Some(conversation)) {
            crate::bar::show_card(&self.app, asking.bar, Box::new(ask_first_action));
        }
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
                    let started = match &self.asker {
                        Asker::Shortcut(_) => crate::voice::tts::speak_streamed(Some(id.clone())),
                        Asker::Phone(p) => p.voice(&id),
                    };
                    match started {
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
        // The voice started: "Speaking" on the capsule, and the card (a
        // phone's voice is the phone's alone).
        let Some(id) = started_id else { return };
        tracing::info!(phone = self.asker.shortcut().is_none(), "Voice Assistant: speaking from the answer's first sentence");
        let Some(generation) = self.asker.shortcut() else { return };
        let ev = {
            let mut s = lock();
            if s.generation != generation {
                return;
            }
            s.phase = Phase::Speaking;
            s.message_id = Some(id);
            snapshot(&s)
        };
        emit(&self.app, ev);
        // The card, unless the answer is in view in Chat: decided now, as
        // the answer comes (the person may have left Yap since asking).
        let conversation = self.lock().conversation;
        if crate::bar::cards_available() && card_on_bar(self.show_card, true, answer_seen(&self.app, conversation)) {
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
        let (ev, id) = match self.asker.shortcut() {
            Some(generation) => {
                let mut s = lock();
                if s.generation == generation && s.phase == Phase::Speaking {
                    s.phase = Phase::Thinking;
                    (Some(snapshot(&s)), s.message_id.take())
                } else {
                    (None, None)
                }
            }
            // A phone's: its answer's own id.
            None => (None, self.lock().message_id.clone()),
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

impl Drop for Streamed {
    /// Its turn ended (answered, stopped, replaced by a newer question): a
    /// question it asked out loud no longer waits, and its bar card goes.
    fn drop(&mut self) {
        if spoken_ok::ended(self.owner) {
            crate::bar::dismiss_card(&self.app, spoken_ok::CARD);
        }
    }
}

/// The bar card's Allow once / Don't ([`spoken_ok::bar_card`]): the
/// question's answer, as the thread's card gives it (✕ only hides it).
fn ask_first_action(app: &AppHandle, action: &str) {
    let Some((id, allow)) = spoken_ok::action_of(action) else { return };
    crate::chat::pages::act::confirm::chat_page_confirm(id, allow);
    if spoken_ok::answered(id) {
        crate::bar::dismiss_card(app, spoken_ok::CARD);
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
        // A call asking first, or answered (spoken_ok.rs).
        if let crate::chat::stream::Event::Confirm(card) = &ev {
            self.asks_first(card);
        }
        // What it's doing, for the capsule (this question's, while it's
        // still the one asked).
        if let crate::chat::stream::Event::Step { step } = &ev {
            if self.asker.current() {
                match &self.asker {
                    Asker::Shortcut(_) => {
                        let _ = self.app.emit(STEP, step);
                    }
                    Asker::Phone(p) => p.step(&step.line),
                }
            }
        }
        if let crate::chat::stream::Event::Text { text } = &ev {
            // The phone shows it as it's written ("" = taken back).
            if let Asker::Phone(p) = &self.asker {
                if p.current() {
                    p.answer(text, false);
                }
            }
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

/// **One spoken question, answered** — the shortcut's ([`Asker::Shortcut`])
/// or a phone's ([`Asker::Phone`], which leaves the PC's own things alone:
/// the screenshots in Chat's box, a quoted passage, the window in front, the
/// bar). `about`: the text selected in another app it's about
/// ([`selected`]) — then nothing else goes with it.
async fn answer(app: AppHandle, asker: Asker, since: u64, question: String, about: Option<selected::Selected>) {
    let cfg = crate::config::load();
    let pc = asker.shortcut().is_some();
    let about_selection = about.is_some();
    let viewing = view();
    let open_exists = matches!(viewing, Viewing::Conversation(id) if crate::chat::chats::get(id).is_some());
    let chosen = choose_conversation(viewing, open_exists, crate::chat::chats::latest(), now_secs());
    // A phone keeps its own conversation (Yap Link's drawer, 2026-11-03).
    let choice = match &asker {
        Asker::Phone(p) => {
            let wanted = p.chat();
            let usable = matches!(wanted, crate::link::wire::ChatRef::Id(id)
                if crate::chat::chats::get(id).is_some_and(|c| crate::link::chats::usable(&c)));
            phone_conversation(wanted, usable, chosen)
        }
        Asker::Shortcut(_) => chosen,
    };
    // A phone's pictures (Yap Link): they go with the question as a typed
    // question's screenshots do (below).
    let (pictures, pictures_as_text) = asker.pictures();
    let with_pictures = !pictures.is_empty();
    // A Skill said — "slash prep", "run my standup skill", "prep me for my
    // next meeting" — or the answer to Yap's question for one (skill.rs):
    // run as Chat runs it, the question itself what it's about. Never a
    // question about a selection, nor one with pictures.
    let continued = match choice {
        Choice::Continue(id) => Some(id),
        Choice::New => None,
    };
    // A spoken yes or no to a call waiting for the person's OK in this
    // conversation (spoken_ok.rs: a connector's tool, an action on a page,
    // an agent): its answer, and that question's turn goes on — nothing new
    // is asked.
    if let (Some(now), false) = (asker.shortcut(), about_selection) {
        if spoken_ok_answer(&app, now, continued, &question, &cfg) {
            return;
        }
    }
    let said = if about_selection || with_pictures { None } else { skill::said(&question, &cfg, continued) };
    // Screenshots waiting in Chat's box (snip/waiting.rs): the question goes
    // with them, to the conversation they wait in. (Not one about a
    // selection or a Skill: that's what it's about.)
    let waiting = crate::snip::waiting::now();
    let shots_wait = pc && !about_selection && said.is_none() && !waiting.images.is_empty();
    let conversation_id = if shots_wait { waiting.conversation_id } else { continued };
    // A new code chat on screen, its workspace picked: the new conversation
    // is a Code chat there (agents/code_chat.rs). From a phone with Remote
    // Control: the workspace its `code_new` named, for a new chat.
    let code_folder = match &asker {
        Asker::Phone(p) => p.new_code_folder().filter(|_| conversation_id.is_none()),
        Asker::Shortcut(_) => new_code_chat(pc, conversation_id, code_chat_shown()),
    };
    // "Open Teams" needs no model, nor a "yes" to Yap's "Did you mean …?":
    // Yap opens it and says so (apps.rs). Never a question about a selection
    // or a Skill.
    let opens_an_app = !about_selection && !with_pictures && said.is_none() && crate::apps::plan_for(&question, &cfg, conversation_id).await.is_some();
    let endpoint = if opens_an_app {
        None
    } else {
        match crate::chat::turn::resolve_chat_endpoint(&cfg) {
            Ok(e) => Some(e),
            Err(why) => {
                asker.alert(&app, no_model_alert(&why));
                asker.give_up(&app);
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
            asker.give_up(&app);
            return;
        }
        if let Some((conv, images)) = crate::snip::waiting::take_for_question(&app) {
            // The on-device model loads its projector first (local_llm/vision.rs).
            if provider == crate::ai::local_llm::PROVIDER_ONDEVICE {
                if let Err(e) = crate::ai::local_llm::vision::ensure(&app).await {
                    tracing::info!("Voice Assistant: the on-device model can't read the screenshots now ({e})");
                    crate::snip::waiting::put_back(&app, conv, images);
                    cant_read(&app, &question, conv, true);
                    asker.give_up(&app);
                    return;
                }
            }
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
                    asker.alert(&app, failed_alert(&e));
                    asker.give_up(&app);
                    return;
                }
            }
        }
    }
    let mut ask = question.clone();
    // A phone's pictures, as a typed question's screenshots go
    // (`commands::question_images`, Chat's own way): a model known not to
    // read images gets none — the phone offers Chat's ways on (another
    // model, their text instead, `Phone::cant_read`) — else their copies
    // for this provider go with every call of the turn; or their text, read
    // on this PC, goes in their place.
    if with_pictures {
        match crate::chat::commands::question_images(&app, &pictures, pictures_as_text, question.clone()).await {
            Ok(q) => {
                tracing::info!(pictures = q.images.images.len(), as_text = pictures_as_text, "Voice Assistant: a phone's question with pictures");
                shots = q.images;
                target = q.target;
                ask = q.text;
            }
            Err(e) => {
                tracing::info!("Voice Assistant: the phone's pictures couldn't go ({e})");
                if let Asker::Phone(p) = &asker {
                    if e.starts_with(crate::snip::vision::CANT_READ) || e.starts_with(crate::snip::vision::COULDNT_READ) {
                        p.cant_read(&e);
                    } else {
                        p.failed(&e);
                    }
                }
                return;
            }
        }
    }
    // The person is in Yap's main window: Chat comes up there with the
    // question. In front, or beside the app in front showing Chat: the
    // answer is in view there (a document's line points at the panel).
    // (Asked from a phone: nobody's at the PC to see it come up.)
    let seen = if pc { crate::notices::notice::window_seen(&app) } else { crate::notices::notice::Seen::No };
    let focused = seen == crate::notices::notice::Seen::Front;
    let in_chat = answer_in_view(seen, viewing, conversation_id);
    // The passage quoted over Chat's box (a document's "Change this…"): it
    // goes with a question asked with Chat in view, to the conversation it's
    // quoted in, as a typed or dictated question takes it
    // (`documents::quote_goes`); the box's quote goes.
    let quote = if opens_an_app || !pc || about_selection || said.is_some() { None } else { crate::chat::documents::take_waiting_quote(&app, in_chat, conversation_id) };
    // A question whose words point at the screen (screen.rs: "what's this
    // error?", "summarise this page"): the window in front goes with it —
    // its picture, or its text read on this PC — or a line tells the model
    // why it can't see it. No model is asked first (8 Nov 2026): anything
    // else is the answering model's to look at if it needs to (below). Not
    // beside screenshots already waiting, a quoted passage or a selection
    // (they're what it's about), nor when Yap opens an app (no model), nor
    // for a Skill (its passages are what it reads).
    let mut window_picture: Option<(String, String)> = None;
    // With Yap's own window in front the window in front is Yap, which a
    // capture leaves out: a model that calls tools looks itself
    // (`look_at_screen`: every monitor and the page beside the chat) — your
    // Claude plan's too, over its run endpoint since 7 Nov (8 Nov 2026: left
    // to the window's capture, which can't take Yap's own window, the plan
    // was told "Yap couldn't take a screenshot" while its own look worked a
    // minute later).
    let chat_looks = focused && endpoint.as_ref().is_some_and(|(_, _, model, provider, _, _)| crate::chat::tools::supports_tools(provider, model));
    let mut took_window = false;
    if let (Some(generation), false, Some((base_url, _key, model, provider, _persona, _thinking))) =
        (asker.shortcut(), shots_wait || quote.is_some() || about_selection || chat_looks || said.is_some(), &endpoint)
    {
        let ep = screen::Endpoint { base_url, model, provider };
        let look_app = app.clone();
        let show = move |on: bool| looking(&look_app, generation, on);
        if let Some(look) = screen::look(&app, &question, &cfg, ep, &show).await {
            took_window = look.took;
            if look.took && look.question.is_none() {
                window_picture = Some((provider.clone(), model.clone()));
            }
            if let Some(with_text) = look.question {
                ask = with_text;
            }
            shots = look.images;
        }
    }
    // Asked from another app with nothing else going with it: the answering
    // model may look at the window in front itself, when the question needs
    // it (`look_at_screen`, the window its look: `may_look_at_window`) — the
    // whole conversation in view, never a guess from the words.
    let window_look = pc && !in_chat && !opens_an_app && !shots_wait && quote.is_none() && !about_selection && said.is_none() && shots.images.is_empty() && shots.note.is_none();
    tracing::info!(phone = !pc, ?viewing, ?choice, ?seen, shots = shots.images.len(), window = took_window, window_look, quoted = quote.is_some(), selection = about_selection, skill = said.is_some(), "Voice Assistant: asking Chat");
    // A Skill with nothing to run on (`skills::ask_first`): Yap asks out loud,
    // and waits for the answer by voice (below).
    let asks = said.as_ref().and_then(|t| crate::chat::skills::ask_first(t, true));
    let ran = said.as_ref().map(|t| t.skill.clone());
    let asked_app = app.clone();
    let asked_text = question.clone();
    // A phone learns which conversation its question went to.
    let asked_phone = match &asker {
        Asker::Phone(p) => Some(std::sync::Arc::clone(p)),
        Asker::Shortcut(_) => None,
    };
    // The answer as it's written: the Chat view follows it, the voice
    // starts at its first sentence.
    // A phone's answer is said when it was asked out loud (into a headset),
    // on the phone, with no card on this PC's bar; a typed one isn't.
    let streamed = Streamed {
        app: app.clone(),
        asker: asker.clone(),
        since: std::sync::Arc::new(AtomicU64::new(since)),
        owner: spoken_ok::owner(),
        question: question.clone(),
        voice_on: asker.voiced(cfg.speak_replies),
        show_card: cfg.assistant_answer_card && pc,
        s: Mutex::new(Speaking::default()),
    };
    let asked = |conv: u64, index: usize| {
        // A new code chat's question: its conversation attached as it's made
        // (before the prompt is built), and the Chat view told at once.
        let code = crate::agents::code_chat::adopt_new(conv);
        if pc {
            lock().conversation = Some(conv);
        }
        if let Some(p) = &asked_phone {
            // The phone's question, saved (before any tool runs).
            p.question_saved(conv, index);
            if p.current() {
                p.asked(conv);
            }
        }
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
                "codeFolder": code,
            }),
        );
    };
    // ■ (the capsule's, the card's ✕, Chat's box, Esc) closes the model's
    // stream too, not just the voice: a model on this PC stops writing.
    let stoppable = crate::chat::stream::Stoppable::new(Some(&asker.stop_id()));
    let turn = crate::snip::turn::with(
        shots,
        crate::chat::turn::chat_turn(
            Some(&app),
            conversation_id,
            &ask,
            Some("assistant"),
            asker.by_voice(),
            asked,
            Some(&streamed as &dyn crate::chat::stream::Sink),
            Some(&stoppable),
        ),
    );
    // The Skill said: Chat's own turn runs it (its passages, instructions and
    // citations, `skills::current`).
    let turn = crate::chat::skills::with(said, turn);
    // The quoted passage it's about: the model is told its blocks, and the
    // question keeps its quote (documents.rs).
    let turn = crate::chat::documents::with_selection(quote.as_ref().map(crate::chat::documents::WaitingQuote::selection), turn);
    // The text selected in another app it's about (selected.rs): the model
    // is told it, and the question keeps it as its quote.
    let turn = selected::with(about, turn);
    // Asked with Chat in view: it shows the answer (`asked_in_chat`).
    let turn = IN_CHAT.scope(in_chat, turn);
    // From another app: its model may look at the window in front.
    let turn = WINDOW_LOOK.scope(window_look, turn);
    // A new code chat's: the conversation it makes is a Code chat in its workspace.
    let turn = crate::agents::code_chat::with_new(code_folder, turn);
    // Its AI calls and searches are the Voice Assistant's in the network
    // receipts (net.rs), though they run Chat's code — and say so when the
    // window in front or a selection went with them, or the question came
    // from a phone.
    let feature = if !pc {
        crate::privacy::net::Feature::AssistantPhone
    } else if about_selection {
        crate::privacy::net::Feature::AssistantSelection
    } else if took_window {
        crate::privacy::net::Feature::AssistantScreen
    } else {
        crate::privacy::net::Feature::Assistant
    };
    let turn = crate::privacy::net::within(feature, turn).await;
    // A spoken answer to a question that asked first meanwhile was the
    // person answering, not a dictation of their own (spoken_ok.rs).
    let since = streamed.since();
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
            asker.give_up(&app);
            return;
        }
        Err(e) => {
            // Half an answer said: it stops there (a phone's only if it's
            // still its voice speaking).
            if let Some(feed) = &feed {
                feed.close();
                if pc || (answer_id.is_some() && crate::voice::tts::speaking_id() == answer_id) {
                    crate::voice::tts::stop();
                }
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
                asker.give_up(&app);
                return;
            }
            // The passage it was about back over the box, to ask again (as a
            // typed question's is).
            if let Some(q) = quote {
                crate::chat::documents::put_back_quote(&app, q);
            }
            // A phone's pictures turned down by the model's server: that
            // model is a "no" from now on, and the phone offers Chat's ways
            // on (as Chat's own line does for a typed question).
            if let (Asker::Phone(p), true) = (&asker, with_pictures) {
                if let Some((provider, model)) = target.as_ref().filter(|_| crate::snip::vision::is_image_refusal(&e)) {
                    tracing::info!(provider, "Voice Assistant: the model refused the phone's pictures");
                    crate::snip::vision::refused(provider, model);
                    let conv = streamed.lock().conversation.or(conversation_id);
                    let _ = app.emit(
                        TURN,
                        serde_json::json!({ "phase": "failed", "conversationId": conv, "userMessageId": question_id, "error": e }),
                    );
                    if p.current() {
                        p.cant_read(&format!("{} ({model})", crate::snip::vision::COULDNT_READ));
                    }
                    return;
                }
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
                    asker.give_up(&app);
                    return;
                }
            }
            // The window's picture turned down: that model is a "no" from
            // now on (its text goes next time).
            if let Some((provider, model)) = window_picture.filter(|_| crate::snip::vision::is_image_refusal(&e)) {
                tracing::info!(provider, "Voice Assistant: the model refused the window's picture");
                crate::snip::vision::refused(&provider, &model);
            }
            tracing::warn!("Voice Assistant: no answer ({e})");
            let conv = streamed.lock().conversation.or(lock().conversation);
            let _ = app.emit(
                TURN,
                serde_json::json!({ "phase": "failed", "conversationId": conv, "userMessageId": question_id, "error": e }),
            );
            if asker.current() {
                asker.alert(&app, failed_alert(&e));
            }
            asker.give_up(&app);
            return;
        }
    };
    // Yap asked what the Skill runs on: the next spoken question in this
    // conversation may be the answer (skill.rs).
    if let (Some(skill), Some(asked)) = (&ran, &asks) {
        if turn.reply == *asked {
            skill::wait_for_answer(turn.conversation_id, skill);
        }
    }
    // **Straight to its Code chat's session** (12 Nov 2026,
    // `agents/direct.rs`): no model of Chat's answered. The page keeps the
    // question and shows its session beside it; nothing is said yet — the
    // session's own words are, when it's done (`chat/turn/report.rs`), as
    // a reply in Claude Code's own app is all there is.
    if let Some(agent) = &turn.direct {
        if let Some(feed) = &feed {
            feed.close();
        }
        let _ = app.emit(
            TURN,
            serde_json::json!({
                "phase": "answered",
                "conversationId": turn.conversation_id,
                "userMessageId": format!("chat:{}:{}", turn.conversation_id, turn.user_index),
                "messageId": null,
                "show": focused,
                "direct": agent,
            }),
        );
        match &asker {
            Asker::Phone(p) => {
                if p.current() {
                    p.answer(crate::agents::direct::SENT_SHOWN, true);
                }
            }
            // (Back to idle on the bar: its "agent working" pill says the rest.)
            Asker::Shortcut(_) => asker.give_up(&app),
        }
        tracing::info!(phone = !pc, "Voice Assistant: the question went straight to its Code chat's session");
        return;
    }
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
    // The phone has the checked answer, and its id (for ▶ there).
    if let Asker::Phone(p) = &asker {
        if p.current() {
            p.checked(&turn.reply, &message_id);
        }
    }
    // Already speaking (the answer was spoken as it came): the rest of what's
    // said, now the answer is whole and checked.
    if let Some(feed) = feed {
        finish_streamed(&app, &asker, since, &question, &turn, &feed, said, carded_early, &cfg);
        return;
    }
    // A meeting or a call that started while the answer was coming: shown,
    // not read aloud.
    let meeting = crate::voice::tts::meeting_quiet().is_some();
    let interrupted = crate::dictation::pipeline::dictation_state().0 != since;
    // Read now: the switch may have moved while the answer was coming (a
    // phone's is said when it was asked out loud).
    let voice_on = asker.voiced(crate::config::load().speak_replies);
    let mut how = reply(voice_on, meeting, interrupted);
    // A phone's answer: the whole of what's said through its voice, nothing
    // on this PC's bar.
    if let Asker::Phone(p) = &asker {
        if !p.current() {
            return;
        }
        let (said, _) = spoken_answer(&turn.reply, web_sources(&turn), SpokenLength::configured(&crate::config::load()));
        if how == Reply::Speak {
            match p.voice(&message_id) {
                Ok(feed) => {
                    feed.push(crate::voice::tts::utterances(&said));
                    feed.close();
                }
                Err(e) => tracing::info!("Voice Assistant: couldn't start the phone's voice ({e})"),
            }
        }
        tracing::info!(spoken = how == Reply::Speak, phone = true, meeting, interrupted, "Voice Assistant: answered");
        return;
    }
    let Some(generation) = asker.shortcut() else { return };
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
    // As much as "Read aloud" says (and on the card), the rest in Chat.
    let (said, shown) = spoken_answer(&turn.reply, web_sources(&turn), SpokenLength::configured(&crate::config::load()));
    let sources_note = sources_note(web_sources(&turn));
    let note = note.or(sources_note.as_deref());
    // The card first (the voice may be short), then the voice. With "Show
    // the answer on the Yap bar" off, a spoken answer has no card; with the
    // answer in view in Chat, none at all.
    let on_bar = crate::bar::cards_available();
    let in_view = answer_seen(&app, Some(turn.conversation_id));
    let mut carded = on_bar
        && card_on_bar(cfg.assistant_answer_card, how == Reply::Speak, in_view)
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
            } else if on_bar && !in_view {
                // The card was left out for the voice, and there's no voice:
                // show the answer after all.
                carded = show_card(&app, &question, &shown, false, Some(why.as_str()));
            }
        }
    }
    tracing::info!(spoken = how == Reply::Speak, carded, in_view, meeting, interrupted, "Voice Assistant: answered");
}

/// The end of an answer the voice began while it was written ([`Streamed`]):
/// the rest of what's said ([`spoken_answer`] of the checked answer, after
/// the `said` sentences already handed over), or — a meeting, a dictation
/// or ■ since — nothing more; the card shows the whole short answer.
#[allow(clippy::too_many_arguments)]
fn finish_streamed(
    app: &AppHandle,
    asker: &Asker,
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
    // A phone's answer is said when it was asked out loud.
    let how = reply(asker.voiced(crate::config::load().speak_replies), meeting, interrupted);
    let (said_text, shown) = spoken_answer(&turn.reply, web_sources(turn), SpokenLength::configured(&crate::config::load()));
    let still = asker.current();
    // A phone's: the rest of it on the phone, or nothing more; this PC's bar
    // and capsule aren't its.
    let Some(generation) = asker.shortcut() else {
        if how == Reply::Speak && still {
            let rest: Vec<String> = crate::voice::tts::utterances(&said_text).into_iter().skip(said).collect();
            if !rest.is_empty() {
                feed.push(rest);
            }
        }
        feed.close();
        tracing::info!(spoken = how == Reply::Speak && still, streamed = true, phone = true, meeting, interrupted, "Voice Assistant: answered");
        return;
    };
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
    } else if crate::bar::cards_available()
        && card_on_bar(cfg.assistant_answer_card, how == Reply::Speak, answer_seen(app, Some(turn.conversation_id)))
    {
        // No card while it was said (Chat was in view, or the voice only):
        // one now if the person has left Chat since, or it isn't read aloud.
        show_card(app, question, &shown, how == Reply::Speak, note);
    }
    tracing::info!(spoken = how == Reply::Speak, streamed = true, carded, meeting, interrupted, "Voice Assistant: answered");
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
/// nothing has no preference: as if closed). `code_folder`: that new chat is
/// a new code chat, its workspace picked (a name or path): a spoken
/// question's new conversation is a Code chat there ([`new_code_chat`]).
#[tauri::command]
pub fn chat_view_state(open: bool, conversation_id: Option<u64>, new_chat: Option<bool>, code_folder: Option<String>) {
    *VIEW.lock().unwrap_or_else(|p| p.into_inner()) = match (open, conversation_id) {
        (false, _) => Viewing::Closed,
        (true, Some(id)) => Viewing::Conversation(id),
        (true, None) if new_chat.unwrap_or(false) => Viewing::New,
        (true, None) => Viewing::Closed,
    };
    *NEW_CODE_CHAT.lock().unwrap_or_else(|p| p.into_inner()) = code_folder.filter(|f| open && conversation_id.is_none() && !f.trim().is_empty());
    // The screenshots waiting in its box wait in what it shows.
    crate::snip::waiting::viewing(open, conversation_id);
}

/// ■: stop the answer (see [`stop`]).
#[tauri::command]
pub fn assistant_stop(app: AppHandle) {
    stop(&app);
}

/// Where the Voice Assistant is now (`yap-assistant`'s payload: the phase and
/// the conversation its question went to), for a Chat view that opened after
/// it was said: its ■ shows while it thinks or speaks, in that conversation
/// only; and for Chat's ■, which stops it (voice and all) only when it's
/// that conversation's.
#[tauri::command]
pub fn assistant_phase() -> PhaseEvent {
    let s = lock();
    snapshot(&s)
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
    fn a_new_code_chat_on_screen_makes_the_spoken_questions_conversation_a_code_chat() {
        let garden = || Some("garden-planner".to_string());
        // Its new conversation: a Code chat in the workspace Chat shows.
        assert_eq!(new_code_chat(true, None, garden()).as_deref(), Some("garden-planner"));
        // One it continues keeps what it is; a phone's question keeps its own;
        // no new code chat on screen, nothing.
        assert_eq!(new_code_chat(true, Some(4), garden()), None);
        assert_eq!(new_code_chat(false, None, garden()), None);
        assert_eq!(new_code_chat(true, None, None), None);
        assert_eq!(new_code_chat(true, None, Some("  ".into())), None);
    }

    #[test]
    fn a_spoken_question_is_a_code_chats_when_it_goes_into_one() {
        let code = |id: u64| id == 7;
        // A Code chat it continues, or a new code chat on screen making one.
        assert!(into_code_chat(true, Some(7), None, code));
        assert!(into_code_chat(true, None, Some("garden-planner"), code));
        // A plain chat, a new plain one: Chat's.
        assert!(!into_code_chat(true, Some(4), None, code));
        assert!(!into_code_chat(true, None, None, code));
        assert!(!into_code_chat(true, None, Some(" "), code));
        // Yap Code off: everything is Chat's, as the Chat view shows it.
        assert!(!into_code_chat(false, Some(7), None, code));
        assert!(!into_code_chat(false, None, Some("garden-planner"), code));
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
        assert!(card_on_bar(true, true, false));
        assert!(!card_on_bar(false, true, false));
        // Not read aloud (a meeting, a dictation started, the voice failed):
        // always shown, or the answer would be lost.
        assert!(card_on_bar(true, false, false));
        assert!(card_on_bar(false, false, false));
        // In view in Chat: never, read aloud or not.
        for (choice, spoken) in [(true, true), (false, true), (true, false), (false, false)] {
            assert!(!card_on_bar(choice, spoken, true));
        }
    }

    #[test]
    fn the_card_is_for_someone_who_cant_see_chat() {
        use crate::notices::notice::Seen;
        let chat = Viewing::Conversation(7);
        // Yap's window in front (Chat comes up there with the question),
        // maximised or not, on any view.
        for viewing in [chat, Viewing::New, Viewing::Closed] {
            assert!(answer_in_view(Seen::Front, viewing, Some(7)));
        }
        // Beside the app in front (another monitor), showing that
        // conversation, or a new chat that follows the question: in view.
        assert!(answer_in_view(Seen::Beside, chat, Some(7)));
        assert!(answer_in_view(Seen::Beside, Viewing::New, None));
        // …another conversation, or another view: not.
        assert!(!answer_in_view(Seen::Beside, chat, Some(8)));
        assert!(!answer_in_view(Seen::Beside, Viewing::Closed, Some(7)));
        // Minimised, hidden or behind another app: the card, whatever Chat
        // shows.
        for viewing in [chat, Viewing::New, Viewing::Closed] {
            assert!(!answer_in_view(Seen::No, viewing, Some(7)));
        }
    }

    /// A phone's turn, asked out loud or typed.
    struct FakePhone(bool);
    impl Phone for FakePhone {
        fn current(&self) -> bool {
            true
        }
        fn stop_id(&self) -> String {
            "link-test".into()
        }
        fn step(&self, _label: &str) {}
        fn answer(&self, _text: &str, _done: bool) {}
        fn voice(&self, _message_id: &str) -> Result<std::sync::Arc<crate::voice::tts::Feed>, String> {
            Err("no voice in a test".into())
        }
        fn failed(&self, _message: &str) {}
        fn by_voice(&self) -> bool {
            self.0
        }
    }

    #[test]
    fn a_phones_typed_question_is_answered_as_typed_and_not_said() {
        // Asked out loud on the phone: said there, whatever "Speak replies" says.
        let spoken = Asker::Phone(std::sync::Arc::new(FakePhone(true)));
        assert!(spoken.by_voice());
        assert!(spoken.voiced(false) && spoken.voiced(true));
        // Typed on the phone: Chat's typed question (no spoken-answer note,
        // not marked by voice), and not said unless ▶ on the phone asks.
        let typed = Asker::Phone(std::sync::Arc::new(FakePhone(false)));
        assert!(!typed.by_voice());
        assert!(!typed.voiced(false) && !typed.voiced(true));
        assert_eq!(reply(typed.voiced(true), false, false), Reply::Show);
        // The shortcut on this PC: asked out loud, said as the switch says.
        let shortcut = Asker::Shortcut(1);
        assert!(shortcut.by_voice());
        assert!(shortcut.voiced(true) && !shortcut.voiced(false));
        // Neither carries pictures unless a phone sends some.
        assert_eq!(shortcut.pictures(), (Vec::new(), false));
        assert_eq!(typed.pictures(), (Vec::new(), false));
    }

    #[test]
    fn a_phone_keeps_its_own_conversation_and_falls_back_cleanly() {
        use crate::link::wire::ChatRef;
        let chosen = Choice::Continue(9);
        // A phone that never said: the Voice Assistant's own choice.
        assert_eq!(phone_conversation(ChatRef::Choose, false, chosen), Choice::Continue(9));
        assert_eq!(phone_conversation(ChatRef::Choose, false, Choice::New), Choice::New);
        // Its own conversation, whatever Chat shows on the PC.
        assert_eq!(phone_conversation(ChatRef::Id(4), true, chosen), Choice::Continue(4));
        // A new chat asked for.
        assert_eq!(phone_conversation(ChatRef::New, false, chosen), Choice::New);
        // Its conversation deleted or archived on the PC: a new one, never another.
        assert_eq!(phone_conversation(ChatRef::Id(4), false, chosen), Choice::New);
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
        let reply = "Gale is on the beach west of the Nautiloid crash site [1]. Pull him out of the portal to recruit him [1][2].

## Details
- He offers to join you [2].";
        let (said, shown) = spoken_answer(reply, 2, SpokenLength::Short);
        assert_eq!(
            said,
            "Gale is on the beach west of the Nautiloid crash site. Pull him out of the portal to recruit him. The details and sources are in Chat."
        );
        assert!(!said.contains('['));
        assert_eq!(shown, "Gale is on the beach west of the Nautiloid crash site. Pull him out of the portal to recruit him.");
        // About a minute (the default): all of it, the card without its markers.
        let (said, shown) = spoken_answer(reply, 2, SpokenLength::from_setting("1m"));
        assert_eq!(said, reply);
        assert!(!shown.contains('[') && shown.contains("He offers to join you"), "{shown}");
        // Not from the web: all of it.
        let one = "Your notes say Friday.";
        assert_eq!(spoken_answer(one, 0, SpokenLength::Short), (one.into(), one.into()));
    }

    #[test]
    fn a_long_answer_is_said_as_far_as_read_aloud_says() {
        let minute = SpokenLength::from_setting("1m");
        assert_eq!(SpokenLength::from_setting(""), minute, "the default");
        assert_eq!(SpokenLength::from_setting("30s"), SpokenLength::Words(75));
        assert_eq!(SpokenLength::from_setting("2m"), SpokenLength::Words(300));
        // Two paragraphs, ~95 words: said whole (it stopped after its second
        // sentence, mid-thought, with the old 60-word limit).
        let two = "I don't have any special knowledge about how the app works inside. I'm an assistant it reaches through an API key, and I don't see how it starts me or what it tells me about itself.

I just answer your messages and use the tools it gives me to search your notes, your calendar and the web. If you're curious what it costs, the app's own usage page is the place to look, rather than something I'd know from the inside.";
        assert!(two.split_whitespace().count() <= SPOKEN_MAX_WORDS);
        assert_eq!(spoken_answer(two, 0, minute).0, two);
        // "A sentence or two": its first two sentences.
        let (said, shown) = spoken_answer(two, 0, SpokenLength::Short);
        assert!(said.ends_with(REST_IN_CHAT) && shown.ends_with("about itself."), "{said}");
        // About 30 seconds: its first paragraph.
        let (_, shown) = spoken_answer(two, 0, SpokenLength::from_setting("30s"));
        assert!(shown.ends_with("about itself."), "{shown}");
        // Qwen3.5-4B's 87-word egg answer (the model evaluations): whole too.
        let egg = "I don't have a specific note on this, but for a runny yolk: - Start with a cold egg in a pot of cold water - Bring to a boil (about 5-7 minutes) - Once boiling, reduce to a simmer and cook for **6-7 minutes total** - Remove from water and let sit in ice water for 1-2 minutes to stop cooking. The exact time can vary slightly based on egg size and water temperature, so 6-7 minutes is a good starting point.";
        assert!(!spoken_answer(egg, 0, minute).0.ends_with(REST_IN_CHAT));
        // Over a minute: whole paragraphs from the first while they fit, then the rest in Chat.
        let para = |topic: &str| format!("The {topic} part of the plan starts on Monday with a short review of what changed last week. After that the team checks each open item, writes down who owns it, and agrees a date for the next look at it."); // 41 words
        let long = format!("{}

{}

{}

{}", para("first"), para("second"), para("third"), para("fourth"));
        assert!(long.split_whitespace().count() > SPOKEN_MAX_WORDS);
        let (said, shown) = spoken_answer(&long, 0, minute);
        assert!(said.ends_with(REST_IN_CHAT), "{said}");
        assert_eq!(shown, format!("{}

{}

{}", para("first"), para("second"), para("third")));
        // About 2 minutes, or the whole answer: all of it.
        assert_eq!(spoken_answer(&long, 0, SpokenLength::from_setting("2m")).0, long);
        assert_eq!(spoken_answer(&long, 0, SpokenLength::Whole).0, long);
        // Chat's voice reply: the same cut, "The rest is on screen."
        assert!(spoken_in_chat(&long, minute).ends_with(REST_ON_SCREEN));
        assert_eq!(spoken_in_chat(&long, SpokenLength::Whole), long);
        // One huge paragraph: its sentences while they fit, never fewer than two.
        let wall = "This sentence goes on for a while to fill the paragraph up with words. ".repeat(20);
        let (said, shown) = spoken_answer(&wall, 0, minute);
        assert!(said.ends_with(REST_IN_CHAT), "{said}");
        let n = shown.split_whitespace().count();
        assert!((28..=SPOKEN_MAX_WORDS).contains(&n), "{n}: {shown}");
        // A short one is said whole, whatever the setting.
        let short = "Boil it for six to seven minutes, then cool it in cold water.";
        for length in [SpokenLength::Short, minute, SpokenLength::Whole] {
            assert_eq!(spoken_answer(short, 0, length).0, short);
        }
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

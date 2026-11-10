//! **Yap Link's protocol, v1** (`docs/architecture/link.md`): what goes over
//! the WebSocket at `wss://<pc>:51443/ws`, shared with the phone's page
//! (`phone/web/page/`). Text frames are JSON `{ "type": … }` ([`In`],
//! [`Out`]); binary frames are audio ([`Frame`]): byte 0 its kind (`0x01`
//! up, the phone's mic at 16 kHz mono Int16 LE; `0x02` down, Yap's voice,
//! mono Int16 LE at `speak_start`'s rate), bytes 1–4 its `seq` (u32 LE),
//! the rest samples. The same messages as the long-term design's data
//! channels (yap-private `phone-remote-design.md` §3.4), so moving to WebRTC
//! later changes the transport only. Pure.
//!
//! **v1's additions** (2026-11-03), backward compatible both ways: `ask`,
//! `read_aloud`, `set_model` by a picker row, `set_effort`, `set_fast`;
//! `ready.can` ([`CAN`]) and `ready.picker`, `answer.id`. Then the phone's
//! own conversation and its pictures (the Claude-app layout, the same day):
//! `chats` → [`Out::Chats`], `open_chat` → [`Out::Thread`], `new_chat`, a
//! question's `chat` (which conversation it goes to, [`chat_ref`]) and
//! [`Out::Chat`] (the one it went to), [`Out::Title`] (a conversation
//! renamed); `picture` and its `0x03` frames ([`PICTURE`]: a question's
//! pictures, [`MAX_PICTURES`] of at most [`MAX_PICTURE_BYTES`] each),
//! `pictures` and `asText` on `ask` / `stop`, [`Out::CantRead`]. `hello.v`
//! stays 1; a first page ignores the new fields; a page offers the new
//! things only when `can` says so; a type this PC doesn't know is said
//! plainly ([`UNKNOWN`]). Then **words for the phone's box** (the same day,
//! the Claude and ChatGPT apps' way: talk, check, send): `start {mode:
//! "words"}` ([`Talk::Words`]) is recorded as a question is, but on `stop`
//! only transcribed — [`Out::Words`], then `state idle`; nothing asked,
//! nothing written to Chat (`"words"` in [`CAN`]; a page from before sends
//! `"ask"`, and a PC from before takes any `start` as a question). Then
//! **Talk's words answered out loud** (the same evening): `ask {…, speak:
//! true}` when any of the box's words came from Talk — asked as a spoken
//! question is (the Voice Assistant's spoken answer, marked by voice, said
//! on the phone), not as a typed one (`"speak"` in [`CAN`]; a PC from before
//! reads the field as nothing and answers on screen, and a page from before
//! never sends it). And a **hello with both** `device` and `pair`: the
//! device is judged first (`pairing::judge`).

use serde::{Deserialize, Serialize};

/// The protocol's version (`hello.v`).
pub const VERSION: u32 = 1;

/// A phone's mic: 16 kHz mono.
pub const UP_RATE: u32 = 16_000;
/// Yap's voice to the phone: Kokoro's own rate; every engine's audio is
/// brought to it (`link/voice.rs`).
pub const DOWN_RATE: u32 = 24_000;

/// A binary frame's kind.
pub const UP: u8 = 0x01;
pub const DOWN: u8 = 0x02;
/// A piece of a picture the phone sends with a question (after its
/// `picture` message): bytes 1–4 the piece's number (u32 LE, from 0 for
/// each picture), the rest the picture's bytes.
pub const PICTURE: u8 = 0x03;

/// The most pictures a question carries (Chat's own, `snip::images::MAX_PER_REQUEST`).
pub const MAX_PICTURES: u32 = 4;
/// The biggest picture a phone may send: the page makes a photo smaller
/// first (2,048 px on its longest side, a JPEG), so this is room to spare.
pub const MAX_PICTURE_BYTES: u64 = 3 * 1024 * 1024;

/// The longest a text frame may be (a hello, a model's id): anything
/// longer isn't the phone's page.
pub const MAX_TEXT: usize = 16 * 1024;
/// The longest a binary frame may be: the page sends 20–40 ms (640–1,280
/// bytes); a second of audio at most.
pub const MAX_FRAME: usize = 5 + 2 * UP_RATE as usize;

/// A paired phone's credentials, as it keeps them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceCreds {
    pub id: String,
    pub secret: String,
}

/// The QR's pairing token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairToken {
    pub token: String,
}

/// **Phone → PC.**
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum In {
    /// The first message: `device` on a later visit, `pair` from the QR —
    /// both when the page has both (a tab restored with an old `#pair=`, the
    /// QR scanned again on a paired phone): the device counts first.
    Hello {
        #[serde(default)]
        v: u32,
        #[serde(default)]
        name: String,
        #[serde(default)]
        nonce: String,
        #[serde(default)]
        device: Option<DeviceCreds>,
        #[serde(default)]
        pair: Option<PairToken>,
    },
    /// A question starts; up frames follow. `mode`: `"ask"`, or `"words"`
    /// for its words alone ([`Talk::of`]). `chat`: the conversation it goes
    /// to ([`chat_ref`]: an id, or `"new"`; none: as the Voice Assistant
    /// chooses).
    Start {
        #[serde(default)]
        mode: String,
        #[serde(default)]
        chat: Option<serde_json::Value>,
    },
    /// The question ended: transcribe and answer — with the `pictures` sent
    /// just before it (`picture` and its frames), their text instead of
    /// them when `asText`.
    Stop {
        #[serde(default)]
        pictures: u32,
        #[serde(default, rename = "asText")]
        as_text: bool,
    },
    /// Drop the question.
    Cancel,
    /// Stop speaking and abandon the answer.
    Interrupt,
    /// **A question from the composer** (2026-11-03): answered as a spoken
    /// one is after it's transcribed, in the same conversation — typed, not
    /// spoken (the answer's ▶ asks for that: [`In::ReadAloud`]); with
    /// `speak`, as a spoken question (Talk's words, checked in the box).
    Ask {
        #[serde(default)]
        text: String,
        /// The conversation it goes to ([`chat_ref`]).
        #[serde(default)]
        chat: Option<serde_json::Value>,
        /// How many pictures sent just before it go with it.
        #[serde(default)]
        pictures: u32,
        /// Their text instead (read on the PC: "Send its text instead").
        #[serde(default, rename = "asText")]
        as_text: bool,
        /// **Said, then checked** (`"speak"` in [`CAN`]): some of its words
        /// came from Talk (since the box was last empty or sent), so it's
        /// asked as a spoken question — the Voice Assistant's spoken answer,
        /// marked by voice in Chat, said on the phone.
        #[serde(default)]
        speak: bool,
    },
    /// Say answer `id` (one this phone was given, `answer.id`) on the phone,
    /// in Yap's voice: the whole of it.
    ReadAloud {
        #[serde(default)]
        id: String,
    },
    /// Switch Chat's model: one of `ready.models` by its `id` (the first
    /// pages), or a row of the picker (`ready.picker`) as Chat's own picker
    /// picks it — `provider`, `model`, a server's `baseUrl`, an on-device
    /// model's `file`.
    SetModel {
        #[serde(default)]
        id: String,
        #[serde(default)]
        provider: String,
        #[serde(default)]
        model: String,
        #[serde(default, rename = "baseUrl")]
        base_url: Option<String>,
        #[serde(default)]
        file: Option<String>,
    },
    /// The effort Chat's model answers at (`null`: its default), as the
    /// picker sets it (`ai/effort.rs`).
    SetEffort {
        #[serde(default)]
        level: Option<String>,
    },
    /// Fast mode (Claude Opus 5.5, Claude Fable 5.2), as the picker sets it.
    SetFast { on: bool },
    /// Chat's conversations, for the phone's drawer ([`Out::Chats`]).
    Chats,
    /// This phone's questions go to conversation `id` from now on: its
    /// messages come back ([`Out::Thread`]).
    OpenChat { id: u64 },
    /// A new chat: the next question starts a conversation.
    NewChat,
    /// Picture `n` (from 0) of the next question: `size` bytes, in
    /// [`PICTURE`] frames that follow.
    Picture { n: u32, size: u64 },
    Ping,
    // ---- Remote Control (2026-11-08, `link/code.rs`): each checked by the
    // PC's gates on arrival (`code/gates.rs`), never trusted from the page.
    /// The Code side: workspaces, their Code chats, the running agents
    /// ([`Out::CodeList`]).
    CodeList,
    /// The next `ask` / `start` with `chat: "new"` makes a Code chat in
    /// `workspace` (one of the workspaces, by name).
    CodeNew {
        #[serde(default)]
        workspace: String,
    },
    /// Watch agent `id`: its snapshot ([`Out::AgentSnapshot`]), then its live
    /// events ([`Out::AgentEvent`]); one watched agent a phone.
    AgentOpen { id: String },
    /// Stop watching.
    AgentClose,
    /// An older page of agent `id`'s events: those before log line `before`.
    AgentLog {
        id: String,
        #[serde(default)]
        before: u64,
    },
    /// One file's diff lines (in turn `turn`, else every turn).
    AgentDiff {
        id: String,
        file: String,
        #[serde(default)]
        turn: Option<u32>,
    },
    AgentStop { id: String },
    AgentContinue { id: String },
    /// "Tell the agent…" (at most [`MAX_CODE_TEXT`] characters).
    AgentMessage {
        id: String,
        #[serde(default)]
        text: String,
    },
    /// A report card's Go ahead.
    AgentGoAhead { id: String },
    /// Undo this turn (`turn`) or Undo all, after the phone's own confirm
    /// (`sure`).
    AgentUndo {
        id: String,
        #[serde(default)]
        turn: Option<u32>,
        #[serde(default)]
        sure: bool,
    },
    /// Allow once / Deny (Go ahead / Not yet on a plan) on a card this phone
    /// was sent, `shownMs` after the page showed it.
    Approve {
        id: u64,
        allow: bool,
        #[serde(default, rename = "shownMs")]
        shown_ms: u64,
    },
    /// Start / Don't (Send / Don't) on an ask-first card this phone was
    /// sent, as fresh a tap as an approval.
    Confirm {
        id: u64,
        yes: bool,
        #[serde(default, rename = "shownMs")]
        shown_ms: u64,
    },
    /// A Code chat's Mode: up to Auto, under its workspace's cap.
    SetMode { chat: u64, mode: String },
}

/// **Which conversation a phone's question goes to**: one it chose (an
/// id), a new one, or the Voice Assistant's choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChatRef {
    /// As the Voice Assistant chooses (a page from before, or a phone that
    /// never picked one).
    #[default]
    Choose,
    New,
    Id(u64),
}

/// A question's `chat`: a conversation's id, or `"new"`; anything else
/// says nothing. Pure.
pub fn chat_ref(v: Option<&serde_json::Value>) -> Option<ChatRef> {
    match v? {
        serde_json::Value::Number(n) => n.as_u64().filter(|id| *id > 0).map(ChatRef::Id),
        serde_json::Value::String(s) if s == "new" => Some(ChatRef::New),
        _ => None,
    }
}

/// **What a `start` is for**: a question to answer (`"ask"`, and anything
/// else: a page from before sends `"ask"`), or its words alone for the
/// phone's text box (`"words"`: transcribed, never asked).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Talk {
    #[default]
    Ask,
    Words,
}

impl Talk {
    /// A `start`'s `mode`. Pure.
    pub fn of(mode: &str) -> Talk {
        if mode == "words" {
            Talk::Words
        } else {
            Talk::Ask
        }
    }
}

/// What this PC answers beyond the first pages' v1 (`ready.can`): a page
/// offers typing, ▶ on an answer, Chat's picker and the effort only when
/// its PC says so, and says plainly when it doesn't; `"words"`: Talk's
/// words can go in the phone's box to check first ([`Talk::Words`]);
/// `"speak"`: an `ask` with `speak` is answered out loud ([`In::Ask`]).
/// `"code"`, `"code_act"`, `"approve"` (Remote Control, 2026-11-08): the PC
/// answers the Code side's messages — read, act, answer cards — and says
/// in `ready.code` whether this phone may use them now ([`CodeReady`]).
pub const CAN: &[&str] = &["ask", "read_aloud", "picker", "effort", "chats", "pictures", "words", "speak", "code", "code_act", "approve"];

/// The longest "Tell the agent…" (characters), as a typed question.
pub const MAX_CODE_TEXT: usize = 4_000;
/// The longest Code page the PC sends (a snapshot, a page of events or of
/// a diff): cut to fit.
pub const MAX_CODE_FRAME: usize = 64 * 1024;
/// An agent's snapshot: its last this many events.
pub const SNAPSHOT_EVENTS: usize = 200;
/// A page of a file's diff.
pub const DIFF_LINES: usize = 400;
/// A live event's diff lines past this are left out (`more`), asked for
/// with `agent_diff`.
pub const LIVE_DIFF_LINES: usize = 40;

/// **Is Remote Control on for this phone** (`ready.code`), or why not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CodeReady {
    pub on: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

/// A workspace on the Code side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeWorkspace {
    pub name: String,
    pub path: String,
    /// Its Remote Control switch (only allowed ones are listed).
    pub allowed: bool,
    pub used_ts: u64,
    /// "Never above:" (`Mode::as_str`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cap: Option<String>,
}

/// A Code chat on the Code side.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CodeChat {
    pub id: u64,
    pub title: String,
    pub ts: u64,
    pub workspace: String,
}

/// One of an agent's turns, as its pills and the status dots read it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BriefTurn {
    pub n: u32,
    pub title: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_message_id: Option<String>,
}

/// **An agent in brief**: what the list, the pills and the status dots need
/// (the same names as `agents.json`'s, so `core/chatStatus.js` and
/// `core/agents.js` read it as they read an agent).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Brief {
    pub id: String,
    pub conversation: Option<u64>,
    pub folder: String,
    pub status: String,
    /// The line it's on.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub now: Option<String>,
    /// Its latest turn's request in a few words.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_title: Option<String>,
    pub started_ts: u64,
    pub ended_ts: u64,
    pub asks: bool,
    pub plan: bool,
    pub plan_allowed: bool,
    /// Its latest turn's plan was set aside ("Plan not used", 2026-11-08): it
    /// no longer waits on the person (`core/agents.js` `planWaits`). Left
    /// out when false.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub plan_unused: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_message_id: Option<String>,
    pub turn_list: Vec<BriefTurn>,
}

/// A permission prompt waiting, for the status dots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ApprovalRef {
    pub id: u64,
    pub conversation: Option<u64>,
    pub agent: String,
}

/// An ask-first card waiting, for the status dots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfirmRef {
    pub id: u64,
    pub conversation: u64,
}

/// An agent's event with its line in the agent's log.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Logged {
    pub line: u64,
    pub event: serde_json::Value,
}

/// A Code chat's own part of its `thread`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadCode {
    pub workspace: String,
    pub path: String,
    /// The chat's Mode on the PC (`Mode::as_str`), and as the chip says it.
    pub mode: String,
    pub mode_label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cap: Option<String>,
    /// Its workspace allows Remote Control.
    pub allowed: bool,
    /// The highest a phone's turn runs at here ("auto", or the cap's).
    pub phone_max: String,
}

/// A conversation in the phone's drawer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ChatItem {
    pub id: u64,
    pub title: String,
    /// Last updated (unix seconds).
    pub ts: u64,
    /// A Code chat's workspace (Remote Control on for the phone).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// A message of a conversation the phone opened (its words only).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ThreadMessage {
    /// "user" | "assistant"
    pub role: &'static str,
    pub text: String,
    /// An answer's id in Chat (`chat:<conversation>:<index>`), for ▶.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// A question asked out loud.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub voice: bool,
    /// How many pictures a question carried (they stay on the PC).
    #[serde(skip_serializing_if = "is_zero")]
    pub pictures: usize,
    /// Its kind in Chat (`ChatMessage::kind`): `"agentReport"` for an agent's
    /// report marker (role "user", but never the person's words: a page shows
    /// it as a report, not their bubble).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub kind: String,
    /// **The files an answer shared into the conversation** (`share_files`,
    /// `chat/shares.rs`; an agent's report marker's too): their names only —
    /// the files and pictures stay on the PC. The page lists them under it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
}

fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// One model Chat can use, as the phone lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Model {
    pub id: String,
    pub label: String,
    #[serde(rename = "onThisPc")]
    pub on_this_pc: bool,
}

/// Where the phone's question is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Idle,
    Listening,
    Transcribing,
    Thinking,
    Speaking,
}

/// **PC → phone.**
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Out {
    /// The pairing waits for a yes on the PC, which shows the same code.
    PairWait { code: String },
    /// Paired: the phone keeps these for its next visits.
    Paired { device: DeviceCreds },
    /// Refused (the socket closes after it).
    Denied { reason: String },
    /// Connected: this PC's name, and Chat's model and the others it can use;
    /// what it answers beyond v1 ([`CAN`]), and what Chat's picker shows
    /// (`link/models.rs` `picker`; `None` for a hello's test double).
    Ready {
        pc: String,
        model: Option<Model>,
        models: Vec<Model>,
        can: Vec<&'static str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        picker: Option<serde_json::Value>,
        /// Remote Control for this phone, now ([`CodeReady`]; `None`: a hello's
        /// test double).
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<CodeReady>,
    },
    State { phase: Phase },
    /// The question, transcribed and corrected (a typed one: as it came).
    Heard { text: String },
    /// What the answer is doing, in Chat's own words ("Searching the web · …").
    Step { label: String },
    /// The answer so far; `final` once it's been checked, with its `id` in
    /// Chat (`chat:<conversation>:<index>`) for ▶ ([`In::ReadAloud`]).
    Answer {
        text: String,
        #[serde(rename = "final")]
        done: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    /// Yap's voice starts: down frames at `rate`, then `speak_end`.
    SpeakStart { rate: u32 },
    SpeakEnd,
    Error { message: String },
    /// Chat's conversations, newest first (`chats`).
    Chats { items: Vec<ChatItem> },
    /// A conversation's latest messages (`open_chat`), `more` when older
    /// ones were left out; `id` none: a new chat (`new_chat`, or one gone).
    /// A Code chat's adds `code` (its workspace, Mode, the phone's highest),
    /// `agents` (its agents in brief, for the pills) and `cards` (the
    /// prompts and ask-first cards waiting in it): Remote Control on.
    Thread {
        id: Option<u64>,
        title: String,
        messages: Vec<ThreadMessage>,
        more: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<ThreadCode>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        agents: Vec<Brief>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        cards: Vec<serde_json::Value>,
    },
    /// This phone's question went to conversation `id` (a new chat's, or the
    /// one the Voice Assistant chose): its questions go there from now on.
    Chat { id: u64, title: String },
    /// Conversation `id` was renamed (in Chat on the PC).
    Title { id: u64, title: String },
    /// Chat's model can't read the question's pictures (or its server just
    /// turned them down): the page offers Chat's own ways on — another
    /// model, their text instead.
    CantRead { message: String },
    /// What the phone said, transcribed and corrected, for its text box (a
    /// `start {mode: "words"}`): nothing was asked.
    Words { text: String },
    Pong,
    // ---- Remote Control (2026-11-08) ----
    /// The Code side: the workspaces that allow it (most recently used
    /// first), their Code chats (newest first), the agents in them.
    CodeList { workspaces: Vec<CodeWorkspace>, chats: Vec<CodeChat>, agents: Vec<Brief> },
    /// What the status dots are made from (on connect, then on change, 300 ms
    /// apart at most); `seq` grows with each.
    CodeStatus { seq: u64, answering: Vec<u64>, agents: Vec<Brief>, approvals: Vec<ApprovalRef>, confirms: Vec<ConfirmRef> },
    /// A message added to a chat this phone has open (an agent's report and
    /// its answer).
    ChatMessage { conversation: u64, message: ThreadMessage },
    /// On `agent_open`: the agent (its `agents.json` record) and its last
    /// [`SNAPSHOT_EVENTS`] events, diff lines left out (counts kept); `more`:
    /// older ones; `seq` 0 (its live events go on from 1).
    AgentSnapshot { agent: serde_json::Value, events: Vec<Logged>, more: bool, seq: u64 },
    /// A live `yap-agent` event of the agent watched, numbered (`seq`), its
    /// log `line`; diff lines past [`LIVE_DIFF_LINES`] left out.
    AgentEvent {
        id: String,
        seq: u64,
        #[serde(skip_serializing_if = "Option::is_none")]
        line: Option<u64>,
        event: serde_json::Value,
    },
    /// An older page of its events (`agent_log`).
    AgentLog { id: String, events: Vec<Logged>, more: bool },
    /// One file's diff (`agent_diff`): at most [`DIFF_LINES`], `more` past them.
    AgentDiff { id: String, file: String, lines: Vec<String>, more: bool, added: u32, removed: u32 },
    /// This phone fell behind: ask `agent_open` again.
    AgentResync { id: String },
    /// A permission prompt (or a plan) for a workspace this phone may see:
    /// `yap-claude-approve`'s card as is, and when Yap says no by itself.
    ApproveCard {
        card: serde_json::Value,
        #[serde(rename = "expiresTs")]
        expires_ts: u64,
    },
    /// A card's end: `outcome` allowed | denied | timedOut | ended |
    /// cancelled; `by` "pc", "you" (this phone), "phone" (another, `name`d)
    /// or none (it timed out, the agent ended).
    ApproveDone {
        id: u64,
        outcome: String,
        by: Option<&'static str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    /// An ask-first card: the agent it would start or carry on.
    ConfirmCard {
        id: u64,
        conversation: u64,
        agent: serde_json::Value,
        #[serde(rename = "expiresTs")]
        expires_ts: u64,
    },
    /// Its end: `outcome` allowed | refused | timedOut, `by` as for approvals.
    ConfirmDone {
        id: u64,
        outcome: String,
        by: Option<&'static str>,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
    },
    /// A Code chat's Mode changed (anywhere): its mode, as the chip says it,
    /// the workspace's cap.
    Mode {
        chat: u64,
        mode: String,
        label: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        cap: Option<String>,
    },
    /// An agent's turn was reported in a chat (`yap-agent-report`).
    Report { conversation: u64, agent: String, turn: u32, status: String },
    /// A Code action done (Stop, Continue, Undo…): what the PC says of it.
    Acted { action: String, id: String, message: String },
}

impl Out {
    /// As a text frame.
    pub fn json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| r#"{"type":"error","message":"Yap couldn't say that"}"#.to_string())
    }
}

/// Every message type a phone may send ([`In`]).
const KNOWN: &[&str] = &[
    "hello",
    "start",
    "stop",
    "cancel",
    "interrupt",
    "ask",
    "read_aloud",
    "set_model",
    "set_effort",
    "set_fast",
    "chats",
    "open_chat",
    "new_chat",
    "picture",
    "ping",
    "code_list",
    "code_new",
    "agent_open",
    "agent_close",
    "agent_log",
    "agent_diff",
    "agent_stop",
    "agent_continue",
    "agent_message",
    "agent_go_ahead",
    "agent_undo",
    "approve",
    "confirm",
    "set_mode",
];

/// The Code side's messages, phone → PC (Remote Control), as the shared
/// definition the page imports lists them (`desktop/src/lib/core/codeWire.js`,
/// served at `core/codeWire.js`; the tests hold the two together).
#[cfg_attr(not(test), allow(dead_code))]
pub const CODE_IN: &[&str] = &[
    "code_list",
    "code_new",
    "agent_open",
    "agent_close",
    "agent_log",
    "agent_diff",
    "agent_stop",
    "agent_continue",
    "agent_message",
    "agent_go_ahead",
    "agent_undo",
    "approve",
    "confirm",
    "set_mode",
];
/// …and PC → phone.
#[cfg_attr(not(test), allow(dead_code))]
pub const CODE_OUT: &[&str] = &[
    "code_list",
    "code_status",
    "chat_message",
    "agent_snapshot",
    "agent_event",
    "agent_log",
    "agent_diff",
    "agent_resync",
    "approve_card",
    "approve_done",
    "confirm_card",
    "confirm_done",
    "mode",
    "report",
    "acted",
];

/// A message this PC can't read.
const UNREADABLE: &str = "Yap didn't understand that message";
/// A message from a newer page than this PC (a type it doesn't know).
pub const UNKNOWN: &str = "Yap on your PC doesn't know how to do that yet. Update Yap on the PC, then try again.";

/// A text frame from the phone. `Err`: not one of [`In`] (said to the phone
/// as an error; a bad hello is a `denied`) — a type this PC doesn't know
/// (a newer page's) said plainly as that ([`UNKNOWN`]).
pub fn parse(text: &str) -> Result<In, String> {
    if text.len() > MAX_TEXT {
        return Err("That message is too long".into());
    }
    let v: serde_json::Value = serde_json::from_str(text).map_err(|_| UNREADABLE.to_string())?;
    let kind = v.get("type").and_then(|t| t.as_str()).map(str::to_string);
    serde_json::from_value(v).map_err(|_| match kind {
        Some(t) if !KNOWN.contains(&t.as_str()) => UNKNOWN.to_string(),
        _ => UNREADABLE.to_string(),
    })
}

/// **An audio frame**: its kind, `seq` and samples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub kind: u8,
    pub seq: u32,
    pub samples: Vec<i16>,
}

/// A frame as bytes: kind, seq (u32 LE), samples (i16 LE).
pub fn encode(kind: u8, seq: u32, samples: &[i16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(5 + samples.len() * 2);
    out.push(kind);
    out.extend_from_slice(&seq.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// Bytes as a frame. `Err`: too short, too long, an unknown kind or an odd
/// number of sample bytes.
pub fn decode(bytes: &[u8]) -> Result<Frame, &'static str> {
    if bytes.len() < 5 {
        return Err("an audio frame without its header");
    }
    if bytes.len() > MAX_FRAME {
        return Err("an audio frame too long");
    }
    let kind = bytes[0];
    if kind != UP && kind != DOWN {
        return Err("an audio frame of an unknown kind");
    }
    let body = &bytes[5..];
    if !body.len().is_multiple_of(2) {
        return Err("an audio frame cut mid-sample");
    }
    let seq = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
    let samples = body.as_chunks::<2>().0.iter().map(|&c| i16::from_le_bytes(c)).collect();
    Ok(Frame { kind, seq, samples })
}

/// A picture's piece ([`PICTURE`]): its number and bytes. `None`: not a
/// picture's frame (too short, another kind, too long).
pub fn picture_piece(bytes: &[u8]) -> Option<(u32, &[u8])> {
    if bytes.len() < 5 || bytes.len() > MAX_FRAME || bytes[0] != PICTURE {
        return None;
    }
    Some((u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]), &bytes[5..]))
}

/// Int16 samples as f32 in -1..1 (the speech engine's).
pub fn to_f32(samples: &[i16]) -> impl Iterator<Item = f32> + '_ {
    samples.iter().map(|&s| f32::from(s) / 32_768.0)
}

/// f32 samples (-1..1) as Int16, clipped.
pub fn to_i16(samples: &[f32]) -> Vec<i16> {
    samples.iter().map(|&s| (s.clamp(-1.0, 1.0) * 32_767.0).round() as i16).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_frames_go_both_ways_and_bad_ones_are_refused() {
        let samples = [0i16, 1, -1, i16::MAX, i16::MIN, 1234];
        let bytes = encode(UP, 0x0403_0201, &samples);
        assert_eq!(&bytes[..5], &[UP, 0x01, 0x02, 0x03, 0x04], "kind, then seq little-endian");
        assert_eq!(&bytes[5..7], &[0, 0]);
        assert_eq!(&bytes[9..11], &[0xff, 0xff], "-1 little-endian");
        let back = decode(&bytes).unwrap();
        assert_eq!(back, Frame { kind: UP, seq: 0x0403_0201, samples: samples.to_vec() });
        assert_eq!(decode(&encode(DOWN, 7, &[])).unwrap().samples, Vec::<i16>::new(), "a header alone is a frame");
        assert!(decode(&[UP, 0, 0, 0]).is_err(), "no seq");
        assert!(decode(&[0x03, 0, 0, 0, 0, 1, 2]).is_err(), "unknown kind");
        assert!(decode(&[UP, 0, 0, 0, 0, 1]).is_err(), "half a sample");
        assert!(decode(&vec![UP; MAX_FRAME + 1]).is_err(), "too long");
        // 20 ms at 16 kHz: 320 samples, 645 bytes.
        assert_eq!(encode(UP, 1, &[0; 320]).len(), 645);
    }

    #[test]
    fn samples_convert_and_clip() {
        let f: Vec<f32> = to_f32(&[0, 16_384, -32_768]).collect();
        assert_eq!(f, vec![0.0, 0.5, -1.0]);
        assert_eq!(to_i16(&[0.0, 1.0, -1.0, 2.0, -3.0, 0.5]), vec![0, 32_767, -32_767, 32_767, -32_767, 16_384]);
    }

    #[test]
    fn the_phones_messages_parse_as_the_protocol_says() {
        let hello = parse(r#"{"type":"hello","v":1,"name":"Pixel","nonce":"abc","pair":{"token":"t0k"}}"#).unwrap();
        assert_eq!(
            hello,
            In::Hello { v: 1, name: "Pixel".into(), nonce: "abc".into(), device: None, pair: Some(PairToken { token: "t0k".into() }) }
        );
        let again = parse(r#"{"type":"hello","v":1,"name":"Pixel","nonce":"n","device":{"id":"d1","secret":"s"}}"#).unwrap();
        assert!(matches!(again, In::Hello { device: Some(DeviceCreds { ref id, .. }), pair: None, .. } if id == "d1"));
        assert_eq!(parse(r#"{"type":"start","mode":"ask"}"#).unwrap(), In::Start { mode: "ask".into(), chat: None });
        assert_eq!(parse(r#"{"type":"stop"}"#).unwrap(), In::Stop { pictures: 0, as_text: false });
        assert_eq!(parse(r#"{"type":"cancel"}"#).unwrap(), In::Cancel);
        assert_eq!(parse(r#"{"type":"interrupt"}"#).unwrap(), In::Interrupt);
        assert_eq!(
            parse(r#"{"type":"set_model","id":"groq|x"}"#).unwrap(),
            In::SetModel { id: "groq|x".into(), provider: String::new(), model: String::new(), base_url: None, file: None }
        );
        assert_eq!(parse(r#"{"type":"ping"}"#).unwrap(), In::Ping);
        assert!(parse(r#"{"type":"format_disk"}"#).is_err());
        assert!(parse("not json").is_err());
        assert!(parse(&format!(r#"{{"type":"ping","x":"{}"}}"#, "a".repeat(MAX_TEXT))).is_err(), "too long");
    }

    #[test]
    fn typing_reading_aloud_and_the_picker_are_v1_additions() {
        // A typed question, and ▶ on an answer.
        assert_eq!(
            parse(r#"{"type":"ask","text":"When is the bin day?"}"#).unwrap(),
            In::Ask { text: "When is the bin day?".into(), chat: None, pictures: 0, as_text: false, speak: false }
        );
        assert_eq!(parse(r#"{"type":"read_aloud","id":"chat:3:5"}"#).unwrap(), In::ReadAloud { id: "chat:3:5".into() });
        // A row of Chat's picker: its provider, model, server or file.
        assert_eq!(
            parse(r#"{"type":"set_model","provider":"local","model":"llama3","baseUrl":"http://localhost:11434/v1"}"#).unwrap(),
            In::SetModel {
                id: String::new(),
                provider: "local".into(),
                model: "llama3".into(),
                base_url: Some("http://localhost:11434/v1".into()),
                file: None
            }
        );
        assert_eq!(
            parse(r#"{"type":"set_model","provider":"ondevice","model":"","file":"Qwen3.5-4B.gguf"}"#).unwrap(),
            In::SetModel { id: String::new(), provider: "ondevice".into(), model: String::new(), base_url: None, file: Some("Qwen3.5-4B.gguf".into()) }
        );
        assert_eq!(parse(r#"{"type":"set_effort","level":"high"}"#).unwrap(), In::SetEffort { level: Some("high".into()) });
        assert_eq!(parse(r#"{"type":"set_effort","level":null}"#).unwrap(), In::SetEffort { level: None });
        assert_eq!(parse(r#"{"type":"set_fast","on":true}"#).unwrap(), In::SetFast { on: true });
        // A message a newer page sends that this PC doesn't know: said plainly.
        assert_eq!(parse(r#"{"type":"draw_a_picture","what":"a cat"}"#).unwrap_err(), UNKNOWN);
        // A known one that's malformed is just not understood.
        assert_eq!(parse(r#"{"type":"set_fast"}"#).unwrap_err(), "Yap didn't understand that message");
        // What a v1 page reads: `can` says what this PC answers.
        assert!(CAN.contains(&"ask") && CAN.contains(&"read_aloud") && CAN.contains(&"picker"));
        for t in ["ask", "read_aloud", "set_model", "set_effort", "set_fast"] {
            assert!(KNOWN.contains(&t), "{t}");
        }
    }

    #[test]
    fn yaps_messages_are_written_as_the_protocol_says() {
        let j = |o: Out| serde_json::from_str::<serde_json::Value>(&o.json()).unwrap();
        assert_eq!(j(Out::PairWait { code: "4821".into() }), serde_json::json!({"type":"pair_wait","code":"4821"}));
        assert_eq!(
            j(Out::Paired { device: DeviceCreds { id: "d".into(), secret: "s".into() } }),
            serde_json::json!({"type":"paired","device":{"id":"d","secret":"s"}})
        );
        assert_eq!(j(Out::Denied { reason: "No".into() }), serde_json::json!({"type":"denied","reason":"No"}));
        let m = Model { id: "ondevice|".into(), label: "Qwen".into(), on_this_pc: true };
        assert_eq!(
            j(Out::Ready { pc: "DESK".into(), model: Some(m.clone()), models: vec![m.clone()], can: vec!["ask"], picker: None, code: None }),
            serde_json::json!({"type":"ready","pc":"DESK","model":{"id":"ondevice|","label":"Qwen","onThisPc":true},"models":[{"id":"ondevice|","label":"Qwen","onThisPc":true}],"can":["ask"]})
        );
        assert_eq!(
            j(Out::Ready { pc: "DESK".into(), model: None, models: vec![], can: vec![], picker: Some(serde_json::json!({"status":{}})), code: None })["picker"],
            serde_json::json!({"status":{}})
        );
        assert_eq!(j(Out::State { phase: Phase::Transcribing }), serde_json::json!({"type":"state","phase":"transcribing"}));
        assert_eq!(j(Out::Heard { text: "hi".into() }), serde_json::json!({"type":"heard","text":"hi"}));
        assert_eq!(j(Out::Step { label: "Thinking".into() }), serde_json::json!({"type":"step","label":"Thinking"}));
        assert_eq!(j(Out::Answer { text: "Yes.".into(), done: false, id: None }), serde_json::json!({"type":"answer","text":"Yes.","final":false}));
        assert_eq!(
            j(Out::Answer { text: "Yes.".into(), done: true, id: Some("chat:3:5".into()) }),
            serde_json::json!({"type":"answer","text":"Yes.","final":true,"id":"chat:3:5"})
        );
        assert_eq!(j(Out::SpeakStart { rate: DOWN_RATE }), serde_json::json!({"type":"speak_start","rate":24000}));
        assert_eq!(j(Out::SpeakEnd), serde_json::json!({"type":"speak_end"}));
        assert_eq!(j(Out::Error { message: "Oops.".into() }), serde_json::json!({"type":"error","message":"Oops."}));
        assert_eq!(j(Out::Pong), serde_json::json!({"type":"pong"}));
    }

    #[test]
    fn the_phones_own_conversation_and_pictures_are_v1_additions() {
        // The drawer, a chat opened, a new chat.
        assert_eq!(parse(r#"{"type":"chats"}"#).unwrap(), In::Chats);
        assert_eq!(parse(r#"{"type":"open_chat","id":12}"#).unwrap(), In::OpenChat { id: 12 });
        assert_eq!(parse(r#"{"type":"new_chat"}"#).unwrap(), In::NewChat);
        assert_eq!(parse(r#"{"type":"open_chat"}"#).unwrap_err(), "Yap didn't understand that message", "no id");
        assert_eq!(parse(r#"{"type":"open_chat","id":"twelve"}"#).unwrap_err(), "Yap didn't understand that message");
        // A question's conversation, and its pictures.
        let ask = parse(r#"{"type":"ask","text":"What's this plant?","chat":12,"pictures":2,"asText":true}"#).unwrap();
        let In::Ask { chat, pictures, as_text, .. } = ask else { panic!("{ask:?}") };
        assert_eq!((chat_ref(chat.as_ref()), pictures, as_text), (Some(ChatRef::Id(12)), 2, true));
        let start = parse(r#"{"type":"start","mode":"ask","chat":"new"}"#).unwrap();
        let In::Start { chat, .. } = start else { panic!("{start:?}") };
        assert_eq!(chat_ref(chat.as_ref()), Some(ChatRef::New));
        assert_eq!(parse(r#"{"type":"stop","pictures":1}"#).unwrap(), In::Stop { pictures: 1, as_text: false });
        assert_eq!(parse(r#"{"type":"picture","n":0,"size":204800}"#).unwrap(), In::Picture { n: 0, size: 204_800 });
        // `chat` says nothing unless it's an id or "new".
        for v in [serde_json::json!(0), serde_json::json!(-3), serde_json::json!("12"), serde_json::json!(null), serde_json::json!({"id": 1})] {
            assert_eq!(chat_ref(Some(&v)), None, "{v}");
        }
        assert_eq!(chat_ref(None), None);
        assert!(CAN.contains(&"chats") && CAN.contains(&"pictures"));
        for t in ["chats", "open_chat", "new_chat", "picture"] {
            assert!(KNOWN.contains(&t), "{t}");
        }
        // A picture's pieces: kind 0x03, its number, its bytes (any length).
        let mut frame = vec![PICTURE, 2, 0, 0, 0];
        frame.extend_from_slice(b"\xff\xd8jpeg");
        assert_eq!(picture_piece(&frame), Some((2, &b"\xff\xd8jpeg"[..])));
        assert_eq!(picture_piece(&[PICTURE, 0, 0, 0, 0]), Some((0, &[][..])));
        assert_eq!(picture_piece(&[UP, 0, 0, 0, 0, 1]), None, "an audio frame");
        assert_eq!(picture_piece(&[PICTURE, 0, 0]), None, "no number");
        assert_eq!(picture_piece(&vec![PICTURE; MAX_FRAME + 1]), None, "too long");
        // An audio frame's kind is still only up or down.
        assert!(decode(&[PICTURE, 0, 0, 0, 0, 1, 2]).is_err());
        // What the PC says.
        let j = |o: Out| serde_json::from_str::<serde_json::Value>(&o.json()).unwrap();
        assert_eq!(
            j(Out::Chats { items: vec![ChatItem { id: 3, title: "Trip packing list".into(), ts: 1_700_000_000, code: None }] }),
            serde_json::json!({"type":"chats","items":[{"id":3,"title":"Trip packing list","ts":1_700_000_000u64}]})
        );
        let thread = Out::Thread {
            id: Some(3),
            title: "Trip packing list".into(),
            messages: vec![
                ThreadMessage { role: "user", text: "What to pack?".into(), id: None, voice: true, pictures: 2, ..Default::default() },
                ThreadMessage { role: "assistant", text: "A coat.".into(), id: Some("chat:3:1".into()), ..Default::default() },
            ],
            more: false,
            code: None,
            agents: vec![],
            cards: vec![],
        };
        assert_eq!(
            j(thread),
            serde_json::json!({"type":"thread","id":3,"title":"Trip packing list","more":false,"messages":[
                {"role":"user","text":"What to pack?","voice":true,"pictures":2},
                {"role":"assistant","text":"A coat.","id":"chat:3:1"}
            ]})
        );
        assert_eq!(
            j(Out::Thread { id: None, title: String::new(), messages: vec![], more: false, code: None, agents: vec![], cards: vec![] }),
            serde_json::json!({"type":"thread","id":null,"title":"","messages":[],"more":false})
        );
        assert_eq!(j(Out::Chat { id: 9, title: "Bread".into() }), serde_json::json!({"type":"chat","id":9,"title":"Bread"}));
        assert_eq!(j(Out::Title { id: 9, title: "Sourdough".into() }), serde_json::json!({"type":"title","id":9,"title":"Sourdough"}));
        assert_eq!(j(Out::CantRead { message: "No.".into() }), serde_json::json!({"type":"cant_read","message":"No."}));
    }

    #[test]
    fn words_for_the_phones_box_are_a_v1_addition() {
        // Talk's words for the phone's text box: a start that's only transcribed.
        let start = parse(r#"{"type":"start","mode":"words","chat":12}"#).unwrap();
        let In::Start { mode, chat } = start else { panic!("{start:?}") };
        assert_eq!((Talk::of(&mode), chat_ref(chat.as_ref())), (Talk::Words, Some(ChatRef::Id(12))));
        assert_eq!(parse(r#"{"type":"start","mode":"words"}"#).unwrap(), In::Start { mode: "words".into(), chat: None });
        // A question, as a page from before asks (or one that says nothing).
        assert_eq!(Talk::of("ask"), Talk::Ask);
        assert_eq!(Talk::of(""), Talk::Ask);
        assert_eq!(Talk::of("Words"), Talk::Ask, "exactly \"words\"");
        assert_eq!(Talk::default(), Talk::Ask);
        // Its answer: the words, nothing else.
        let j = serde_json::from_str::<serde_json::Value>(&Out::Words { text: "Remind me to water the beans".into() }.json()).unwrap();
        assert_eq!(j, serde_json::json!({"type":"words","text":"Remind me to water the beans"}));
        // Said in `ready.can`, after what was there before.
        assert!(CAN.contains(&"words"));
        assert_eq!(&CAN[..6], &["ask", "read_aloud", "picker", "effort", "chats", "pictures"]);
    }

    #[test]
    fn talks_words_sent_from_the_box_are_asked_out_loud_a_v1_addition() {
        // Some of the box's words came from Talk: asked as a spoken question.
        let ask = parse(r#"{"type":"ask","text":"Water the beans tonight","chat":12,"pictures":1,"speak":true}"#).unwrap();
        assert_eq!(
            ask,
            In::Ask { text: "Water the beans tonight".into(), chat: Some(serde_json::json!(12)), pictures: 1, as_text: false, speak: true }
        );
        // A page from before never sends it: typed.
        let In::Ask { speak, .. } = parse(r#"{"type":"ask","text":"Hi"}"#).unwrap() else { panic!() };
        assert!(!speak);
        // Only a true `speak` is one.
        assert!(parse(r#"{"type":"ask","text":"Hi","speak":"yes"}"#).is_err());
        assert!(matches!(parse(r#"{"type":"ask","text":"Hi","speak":false}"#).unwrap(), In::Ask { speak: false, .. }));
        // Said in `ready.can`, after what was there before.
        assert_eq!(CAN[7], "speak");
        assert_eq!(&CAN[..7], &["ask", "read_aloud", "picker", "effort", "chats", "pictures", "words"]);
        // A hello with both the phone's device and the QR's token.
        let both = parse(r#"{"type":"hello","v":1,"name":"Pixel","nonce":"n","device":{"id":"d1","secret":"s"},"pair":{"token":"t0k"}}"#).unwrap();
        assert_eq!(
            both,
            In::Hello {
                v: 1,
                name: "Pixel".into(),
                nonce: "n".into(),
                device: Some(DeviceCreds { id: "d1".into(), secret: "s".into() }),
                pair: Some(PairToken { token: "t0k".into() })
            }
        );
    }

    // ---- Remote Control (2026-11-08) ----

    /// The shared definition the page imports (`core/codeWire.js`).
    const SHARED: &str = include_str!("../../../src/lib/core/codeWire.js");

    /// One of its `export const NAME = [...]` lists of words.
    fn shared_list(name: &str) -> Vec<String> {
        let start = SHARED.find(&format!("export const {name} = [")).unwrap_or_else(|| panic!("{name} in codeWire.js"));
        let rest = &SHARED[start..];
        let body = &rest[rest.find('[').unwrap() + 1..rest.find("];").unwrap()];
        body.split(',').map(|w| w.trim().trim_matches('\'').to_string()).filter(|w| !w.is_empty()).collect()
    }

    /// Its examples, as JSON.
    fn examples() -> serde_json::Value {
        let start = SHARED.find("/* wire:begin */").unwrap() + "/* wire:begin */".len();
        let end = SHARED.find("/* wire:end */").unwrap();
        serde_json::from_str(&SHARED[start..end]).expect("codeWire.js's examples are strict JSON")
    }

    #[test]
    fn the_code_sides_messages_are_the_shared_definitions_both_ways() {
        // Every type both ways, as the page's own file lists them.
        assert_eq!(shared_list("PHONE_TO_PC"), CODE_IN);
        assert_eq!(shared_list("PC_TO_PHONE"), CODE_OUT);
        assert_eq!(shared_list("CODE_CAN"), ["code", "code_act", "approve"]);
        assert_eq!(&CAN[8..], &["code", "code_act", "approve"], "after what was there before");
        for t in CODE_IN {
            assert!(KNOWN.contains(t), "{t}");
        }
        // The limits and the Modes a phone may pick.
        let num = |name: &str| -> u64 {
            let at = SHARED.find(name).unwrap_or_else(|| panic!("{name}"));
            SHARED[at..].split(|c: char| !c.is_ascii_digit()).find(|s| !s.is_empty()).unwrap().parse().unwrap()
        };
        assert_eq!(num("MIN_SHOWN_MS = "), crate::link::code::gates::MIN_SHOWN_MS);
        assert_eq!(num("text: "), MAX_CODE_TEXT as u64);
        assert_eq!(num("snapshotEvents: "), SNAPSHOT_EVENTS as u64);
        assert_eq!(num("diffLines: "), DIFF_LINES as u64);
        assert_eq!(num("liveDiffLines: "), LIVE_DIFF_LINES as u64);
        assert_eq!(num("frame: "), MAX_CODE_FRAME as u64);
        let modes = shared_list("PHONE_MODES");
        let parsed: Vec<crate::agents::modes::Mode> = modes.iter().map(|m| crate::agents::modes::Mode::parse(m).unwrap()).collect();
        assert!(parsed.iter().all(|m| *m <= crate::link::code::gates::PHONE_MAX), "{modes:?}");
        assert_eq!(parsed.last(), Some(&crate::link::code::gates::PHONE_MAX));
        assert!(SHARED.contains(&format!("PHONE_MAX_MODE = '{}'", crate::link::code::gates::PHONE_MAX.as_str())));
    }

    #[test]
    fn every_example_the_page_has_parses_or_is_written_exactly_here() {
        let ex = examples();
        // Phone → PC: each parses, as its own message.
        let ins = ex["in"].as_object().unwrap();
        assert_eq!(ins.keys().map(String::as_str).collect::<std::collections::BTreeSet<_>>(), CODE_IN.iter().copied().collect());
        for (t, m) in ins {
            assert_eq!(m["type"], t.as_str());
            let parsed = parse(&m.to_string()).unwrap_or_else(|e| panic!("{t}: {e}"));
            let again = format!("{parsed:?}");
            let snake: String = again.split([' ', '{']).next().unwrap().chars().flat_map(|c| if c.is_uppercase() { vec!['_', c.to_ascii_lowercase()] } else { vec![c] }).collect();
            assert_eq!(snake.trim_start_matches('_'), t, "{again}");
        }
        assert_eq!(parse(&ins["approve"].to_string()).unwrap(), In::Approve { id: 7, allow: true, shown_ms: 2_300 });
        assert_eq!(parse(&ins["agent_undo"].to_string()).unwrap(), In::AgentUndo { id: "1a2b3c4d".into(), turn: Some(2), sure: true });
        assert_eq!(parse(&ins["set_mode"].to_string()).unwrap(), In::SetMode { chat: 12, mode: "plan".into() });
        // An approval without its time shown reads as shown for no time (refused by the gates).
        assert_eq!(parse(r#"{"type":"approve","id":7,"allow":false}"#).unwrap(), In::Approve { id: 7, allow: false, shown_ms: 0 });
        // PC → phone: each written exactly as the page's example.
        let outs = ex["out"].as_object().unwrap();
        assert_eq!(outs.keys().map(String::as_str).collect::<std::collections::BTreeSet<_>>(), CODE_OUT.iter().copied().collect());
        let j = |o: Out| serde_json::from_str::<serde_json::Value>(&o.json()).unwrap();
        let running = Brief {
            id: "1a2b3c4d".into(),
            conversation: Some(12),
            folder: "garden-planner".into(),
            status: "running".into(),
            now: Some("Adding a watering schedule".into()),
            turn_title: Some("Add a watering schedule".into()),
            started_ts: 1_730_000_110,
            message_id: Some("chat:12:0".into()),
            turn_list: vec![BriefTurn { n: 1, title: "Add a watering schedule".into(), status: "running".into(), message_id: Some("chat:12:0".into()), report_message_id: None }],
            ..Default::default()
        };
        let path = r"D:\Projects\garden-planner".to_string();
        let written = [
            Out::CodeList {
                workspaces: vec![CodeWorkspace { name: "garden-planner".into(), path: path.clone(), allowed: true, used_ts: 1_730_000_000, cap: None }],
                chats: vec![CodeChat { id: 12, title: "Watering schedule".into(), ts: 1_730_000_100, workspace: "garden-planner".into() }],
                agents: vec![running],
            },
            Out::CodeStatus {
                seq: 4,
                answering: vec![12],
                agents: vec![],
                approvals: vec![ApprovalRef { id: 7, conversation: Some(12), agent: "1a2b3c4d".into() }],
                confirms: vec![ConfirmRef { id: 3, conversation: 12 }],
            },
            Out::ChatMessage {
                conversation: 12,
                message: ThreadMessage { role: "user", text: "The agent in garden-planner: it's done.".into(), kind: "agentReport".into(), ..Default::default() },
            },
            Out::AgentSnapshot {
                agent: serde_json::json!({"id": "1a2b3c4d", "folder": "garden-planner", "status": "running"}),
                events: vec![Logged { line: 1, event: serde_json::json!({"id": "1a2b3c4d", "status": "running", "folder": "garden-planner", "step": "Reading notes.md", "ts": 1_730_000_111u64}) }],
                more: false,
                seq: 0,
            },
            Out::AgentEvent {
                id: "1a2b3c4d".into(),
                seq: 1,
                line: Some(2),
                event: serde_json::json!({"id": "1a2b3c4d", "status": "running", "folder": "garden-planner", "now": "Adding a watering schedule", "ts": 1_730_000_112u64}),
            },
            Out::AgentLog { id: "1a2b3c4d".into(), events: vec![], more: false },
            Out::AgentDiff { id: "1a2b3c4d".into(), file: "notes.md".into(), lines: vec!["-Draft".into(), "+Final".into()], more: false, added: 1, removed: 1 },
            Out::AgentResync { id: "1a2b3c4d".into() },
            Out::ApproveCard {
                card: serde_json::json!({"id": 7, "run": "1a2b3c4d", "conversation": 12, "origin": "typed", "tool": "Bash", "summary": "Run a command: npm test", "detail": "npm test", "label": "garden-planner", "askedTs": 1_730_000_120u64}),
                expires_ts: 1_730_000_720,
            },
            Out::ApproveDone { id: 7, outcome: "allowed".into(), by: Some("phone"), name: Some("Kitchen tablet".into()) },
            Out::ConfirmCard {
                id: 3,
                conversation: 12,
                agent: serde_json::json!({"folder": "garden-planner", "path": path, "mode": "auto", "modeLabel": "Auto", "brief": "Add a watering schedule", "newFolder": false, "web": true, "continues": false}),
                expires_ts: 1_730_000_240,
            },
            Out::ConfirmDone { id: 3, outcome: "allowed".into(), by: Some("you"), name: None },
            Out::Mode { chat: 12, mode: "plan".into(), label: "Plan".into(), cap: Some("manual".into()) },
            Out::Report { conversation: 12, agent: "1a2b3c4d".into(), turn: 1, status: "done".into() },
            Out::Acted { action: "stop".into(), id: "1a2b3c4d".into(), message: "Stopping the agent in garden-planner.".into() },
        ];
        assert_eq!(written.len(), CODE_OUT.len());
        for out in written {
            let v = j(out);
            let t = v["type"].as_str().unwrap().to_string();
            assert_eq!(v, outs[&t], "{t}");
        }
    }

    #[test]
    fn ready_says_whether_remote_control_is_on_and_a_thread_carries_its_code_part() {
        let j = |o: Out| serde_json::from_str::<serde_json::Value>(&o.json()).unwrap();
        let on = Out::Ready { pc: "Office PC".into(), model: None, models: vec![], can: CAN.to_vec(), picker: None, code: Some(CodeReady { on: true, why: None }) };
        assert_eq!(j(on)["code"], serde_json::json!({"on": true}));
        let off = Out::Ready { pc: "Office PC".into(), model: None, models: vec![], can: CAN.to_vec(), picker: None, code: Some(CodeReady { on: false, why: Some("Remote Control is off on Office PC. Turn it on in Settings → Phone.".into()) }) };
        assert_eq!(j(off)["code"]["why"], "Remote Control is off on Office PC. Turn it on in Settings → Phone.");
        // A Code chat's thread: its workspace and Mode, the phone's highest, a report marked as one.
        let thread = Out::Thread {
            id: Some(12),
            title: "Watering schedule".into(),
            messages: vec![
                ThreadMessage { role: "user", text: "Add a watering schedule".into(), ..Default::default() },
                ThreadMessage { role: "user", text: "The agent in garden-planner: it's done.".into(), kind: "agentReport".into(), ..Default::default() },
            ],
            more: false,
            code: Some(ThreadCode {
                workspace: "garden-planner".into(),
                path: r"D:\Projects\garden-planner".into(),
                mode: "bypassPermissions".into(),
                mode_label: "Bypass permissions".into(),
                cap: None,
                allowed: true,
                phone_max: "auto".into(),
            }),
            agents: vec![],
            cards: vec![serde_json::json!({"kind": "approve", "id": 7})],
        };
        let v = j(thread);
        assert_eq!(v["code"]["phoneMax"], "auto");
        assert_eq!(v["code"]["modeLabel"], "Bypass permissions");
        assert_eq!(v["messages"][1]["kind"], "agentReport");
        assert!(v["messages"][0].get("kind").is_none());
        assert_eq!(v["cards"][0]["id"], 7);
        assert!(v.get("agents").is_none(), "none: left out");
        // A drawer item names a Code chat's workspace.
        assert_eq!(j(Out::Chats { items: vec![ChatItem { id: 12, title: "x".into(), ts: 1, code: Some("garden-planner".into()) }] })["items"][0]["code"], "garden-planner");
    }
}

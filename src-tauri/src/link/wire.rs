//! **Yap Link's protocol, v1** (`docs/architecture/link.md`): what goes over
//! the WebSocket at `wss://<pc>:51443/ws`, shared with the phone's page
//! (`src-tauri/link-page/`). Text frames are JSON `{ "type": … }` ([`In`],
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
//! `"ask"`, and a PC from before takes any `start` as a question).

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
    /// The first message: `device` on a later visit, `pair` from the QR.
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
    /// **A typed question** (2026-11-03, the page's composer): answered as
    /// a spoken one is after it's transcribed, in the same conversation,
    /// but not spoken (the answer's ▶ asks for that: [`In::ReadAloud`]).
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
    /// Fast mode (Claude Opus 5.5), as the picker sets it.
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
/// words can go in the phone's box to check first ([`Talk::Words`]).
pub const CAN: &[&str] = &["ask", "read_aloud", "picker", "effort", "chats", "pictures", "words"];

/// A conversation in the phone's drawer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatItem {
    pub id: u64,
    pub title: String,
    /// Last updated (unix seconds).
    pub ts: u64,
}

/// A message of a conversation the phone opened (its words only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
    Thread { id: Option<u64>, title: String, messages: Vec<ThreadMessage>, more: bool },
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
            In::Ask { text: "When is the bin day?".into(), chat: None, pictures: 0, as_text: false }
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
            j(Out::Ready { pc: "DESK".into(), model: Some(m.clone()), models: vec![m.clone()], can: vec!["ask"], picker: None }),
            serde_json::json!({"type":"ready","pc":"DESK","model":{"id":"ondevice|","label":"Qwen","onThisPc":true},"models":[{"id":"ondevice|","label":"Qwen","onThisPc":true}],"can":["ask"]})
        );
        assert_eq!(
            j(Out::Ready { pc: "DESK".into(), model: None, models: vec![], can: vec![], picker: Some(serde_json::json!({"status":{}})) })["picker"],
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
            j(Out::Chats { items: vec![ChatItem { id: 3, title: "Trip packing list".into(), ts: 1_700_000_000 }] }),
            serde_json::json!({"type":"chats","items":[{"id":3,"title":"Trip packing list","ts":1_700_000_000u64}]})
        );
        let thread = Out::Thread {
            id: Some(3),
            title: "Trip packing list".into(),
            messages: vec![
                ThreadMessage { role: "user", text: "What to pack?".into(), id: None, voice: true, pictures: 2 },
                ThreadMessage { role: "assistant", text: "A coat.".into(), id: Some("chat:3:1".into()), voice: false, pictures: 0 },
            ],
            more: false,
        };
        assert_eq!(
            j(thread),
            serde_json::json!({"type":"thread","id":3,"title":"Trip packing list","more":false,"messages":[
                {"role":"user","text":"What to pack?","voice":true,"pictures":2},
                {"role":"assistant","text":"A coat.","id":"chat:3:1"}
            ]})
        );
        assert_eq!(
            j(Out::Thread { id: None, title: String::new(), messages: vec![], more: false }),
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
}

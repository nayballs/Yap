//! **Yap Link's protocol, v1** (`docs/architecture/link.md`): what goes over
//! the WebSocket at `wss://<pc>:51443/ws`, shared with the phone's page
//! (`src-tauri/link-page/`). Text frames are JSON `{ "type": … }` ([`In`],
//! [`Out`]); binary frames are audio ([`Frame`]): byte 0 its kind (`0x01`
//! up, the phone's mic at 16 kHz mono Int16 LE; `0x02` down, Yap's voice,
//! mono Int16 LE at `speak_start`'s rate), bytes 1–4 its `seq` (u32 LE),
//! the rest samples. The same messages as the long-term design's data
//! channels (yap-private `phone-remote-design.md` §3.4), so moving to WebRTC
//! later changes the transport only. Pure.

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
    /// A question starts; up frames follow.
    Start {
        #[serde(default)]
        mode: String,
    },
    /// The question ended: transcribe and answer.
    Stop,
    /// Drop the question.
    Cancel,
    /// Stop speaking and abandon the answer.
    Interrupt,
    /// Switch Chat's model (one of `ready.models`).
    SetModel { id: String },
    Ping,
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
    /// Connected: this PC's name, and Chat's model and the others it can use.
    Ready { pc: String, model: Option<Model>, models: Vec<Model> },
    State { phase: Phase },
    /// The question, transcribed and corrected.
    Heard { text: String },
    /// What the answer is doing, in Chat's own words ("Searching the web · …").
    Step { label: String },
    /// The answer so far; `final` once it's been checked.
    Answer {
        text: String,
        #[serde(rename = "final")]
        done: bool,
    },
    /// Yap's voice starts: down frames at `rate`, then `speak_end`.
    SpeakStart { rate: u32 },
    SpeakEnd,
    Error { message: String },
    Pong,
}

impl Out {
    /// As a text frame.
    pub fn json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| r#"{"type":"error","message":"Yap couldn't say that"}"#.to_string())
    }
}

/// A text frame from the phone. `Err`: not one of [`In`] (said to the phone
/// as an error; a bad hello is a `denied`).
pub fn parse(text: &str) -> Result<In, String> {
    if text.len() > MAX_TEXT {
        return Err("That message is too long".into());
    }
    serde_json::from_str(text).map_err(|_| "Yap didn't understand that message".to_string())
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
        assert_eq!(parse(r#"{"type":"start","mode":"ask"}"#).unwrap(), In::Start { mode: "ask".into() });
        assert_eq!(parse(r#"{"type":"stop"}"#).unwrap(), In::Stop);
        assert_eq!(parse(r#"{"type":"cancel"}"#).unwrap(), In::Cancel);
        assert_eq!(parse(r#"{"type":"interrupt"}"#).unwrap(), In::Interrupt);
        assert_eq!(parse(r#"{"type":"set_model","id":"groq|x"}"#).unwrap(), In::SetModel { id: "groq|x".into() });
        assert_eq!(parse(r#"{"type":"ping"}"#).unwrap(), In::Ping);
        assert!(parse(r#"{"type":"format_disk"}"#).is_err());
        assert!(parse("not json").is_err());
        assert!(parse(&format!(r#"{{"type":"ping","x":"{}"}}"#, "a".repeat(MAX_TEXT))).is_err(), "too long");
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
        let m = Model { id: "ondevice|".into(), label: "Qwen · On this PC".into(), on_this_pc: true };
        assert_eq!(
            j(Out::Ready { pc: "DESK".into(), model: Some(m.clone()), models: vec![m] }),
            serde_json::json!({"type":"ready","pc":"DESK","model":{"id":"ondevice|","label":"Qwen · On this PC","onThisPc":true},"models":[{"id":"ondevice|","label":"Qwen · On this PC","onThisPc":true}]})
        );
        assert_eq!(j(Out::State { phase: Phase::Transcribing }), serde_json::json!({"type":"state","phase":"transcribing"}));
        assert_eq!(j(Out::Heard { text: "hi".into() }), serde_json::json!({"type":"heard","text":"hi"}));
        assert_eq!(j(Out::Step { label: "Thinking".into() }), serde_json::json!({"type":"step","label":"Thinking"}));
        assert_eq!(j(Out::Answer { text: "Yes.".into(), done: true }), serde_json::json!({"type":"answer","text":"Yes.","final":true}));
        assert_eq!(j(Out::SpeakStart { rate: DOWN_RATE }), serde_json::json!({"type":"speak_start","rate":24000}));
        assert_eq!(j(Out::SpeakEnd), serde_json::json!({"type":"speak_end"}));
        assert_eq!(j(Out::Error { message: "Oops.".into() }), serde_json::json!({"type":"error","message":"Oops."}));
        assert_eq!(j(Out::Pong), serde_json::json!({"type":"pong"}));
    }
}

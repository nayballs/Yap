//! The wire between Yap and its `yap.exe diarize` child (see `diarize.rs`).
//!
//! **Yap → child (stdin)**: frames of 16 kHz mono audio on the recording's
//! session clock, one after another, little-endian:
//!
//! ```text
//! frame   := tag:u8  len:u32  payload[len]
//! 'A'     audio:   payload = f32 samples (len is a multiple of 4)
//! 'S'     silence: payload = u32 sample count (len = 4); the child pushes
//!         that much (dithered) silence, so the clock never skips
//! ```
//!
//! Closing stdin means "that was all": the child finishes the stream, writes
//! the final segments and exits.
//!
//! **Child → Yap (stdout)**: one JSON object per line (`Message`): `ready`
//! once the model is loaded, `segments` every few seconds while audio comes
//! in (the whole list so far: the tail can still change), a last `segments`
//! with `final: true`, or `error`. Speakers are 1…8 in the order they were
//! first heard. Times are milliseconds from the first sample the child got.

use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

/// Bumped on any change to the frames or messages. The child says which it
/// speaks in `ready`; Yap drops a child that speaks another.
pub const PROTOCOL: u32 = 1;

pub const TAG_AUDIO: u8 = b'A';
pub const TAG_SILENCE: u8 = b'S';

/// Largest payload the child accepts (8 MB = 2 M samples, ~131 s): anything
/// bigger is a garbled stream, not audio.
pub const MAX_PAYLOAD: u32 = 8 << 20;

/// One frame off the wire.
#[derive(Debug, Clone, PartialEq)]
pub enum Frame {
    Audio(Vec<f32>),
    Silence(u32),
}

/// Append an audio frame to `out`.
pub fn encode_audio(samples: &[f32], out: &mut Vec<u8>) {
    out.reserve(5 + samples.len() * 4);
    out.push(TAG_AUDIO);
    out.extend_from_slice(&((samples.len() * 4) as u32).to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
}

/// Append a silence frame of `n` samples to `out`.
pub fn encode_silence(n: u32, out: &mut Vec<u8>) {
    out.push(TAG_SILENCE);
    out.extend_from_slice(&4u32.to_le_bytes());
    out.extend_from_slice(&n.to_le_bytes());
}

/// Write one frame (audio in pieces of at most [`MAX_PAYLOAD`]).
pub fn write_audio(w: &mut impl Write, samples: &[f32]) -> io::Result<()> {
    let mut buf = Vec::new();
    for piece in samples.chunks((MAX_PAYLOAD / 4) as usize) {
        buf.clear();
        encode_audio(piece, &mut buf);
        w.write_all(&buf)?;
    }
    Ok(())
}

pub fn write_silence(w: &mut impl Write, n: u32) -> io::Result<()> {
    let mut buf = Vec::with_capacity(9);
    encode_silence(n, &mut buf);
    w.write_all(&buf)
}

/// Read the next frame. `Ok(None)` at a clean end (stdin closed between
/// frames); a stream cut mid-frame, an unknown tag or an oversized payload
/// is an error.
pub fn read_frame(r: &mut impl Read) -> io::Result<Option<Frame>> {
    let mut head = [0u8; 5];
    // A clean end is EOF before the first byte of a frame.
    let mut got = 0;
    while got < head.len() {
        match r.read(&mut head[got..]) {
            Ok(0) if got == 0 => return Ok(None),
            Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "stream ended mid-frame")),
            Ok(n) => got += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    let len = u32::from_le_bytes([head[1], head[2], head[3], head[4]]);
    let bad = |what: String| io::Error::new(io::ErrorKind::InvalidData, what);
    match head[0] {
        TAG_AUDIO => {
            if len % 4 != 0 || len > MAX_PAYLOAD {
                return Err(bad(format!("bad audio frame length {len}")));
            }
            let mut bytes = vec![0u8; len as usize];
            r.read_exact(&mut bytes)?;
            let samples = bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
            Ok(Some(Frame::Audio(samples)))
        }
        TAG_SILENCE => {
            if len != 4 {
                return Err(bad(format!("bad silence frame length {len}")));
            }
            let mut n = [0u8; 4];
            r.read_exact(&mut n)?;
            Ok(Some(Frame::Silence(u32::from_le_bytes(n))))
        }
        tag => Err(bad(format!("unknown frame tag {tag:#04x}"))),
    }
}

/// A stretch of one speaker, as the child reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    /// 1…8, in the order the voices were first heard.
    pub speaker: u8,
}

/// One line the child writes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    /// The model is loaded and the stream is open.
    Ready {
        protocol: u32,
        /// The model's speaker slots (8 for Nemotron-3).
        speakers: u32,
        /// Its output cadence.
        frame_ms: f64,
        /// NeMo-Speech.cpp's version string.
        runtime: String,
        load_ms: u64,
    },
    /// Every segment so far (the whole list: its tail can still change until
    /// `final`).
    Segments {
        #[serde(rename = "final")]
        is_final: bool,
        /// Audio received so far.
        audio_ms: u64,
        /// How far the stream has labelled it (≤ `audio_ms`; all of it once
        /// final).
        labelled_ms: u64,
        /// Time spent in the engine so far.
        compute_ms: u64,
        /// How far the labels trailed the audio after each pushed second
        /// (only in the final message).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lag_ms_median: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lag_ms_max: Option<u64>,
        segments: Vec<RawSegment>,
    },
    Error {
        message: String,
    },
}

/// Parse one stdout line. `None` for a blank line or anything that isn't one
/// of our messages (a library printing to stdout must not break the stream).
pub fn parse_line(line: &str) -> Option<Message> {
    let line = line.trim();
    if !line.starts_with('{') {
        return None;
    }
    serde_json::from_str(line).ok()
}

/// Write one message as a line and flush it.
pub fn write_message(w: &mut (impl Write + ?Sized), msg: &Message) -> io::Result<()> {
    let mut line = serde_json::to_vec(msg).map_err(io::Error::other)?;
    line.push(b'\n');
    w.write_all(&line)?;
    w.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let mut wire = Vec::new();
        encode_audio(&[0.0, 0.5, -1.0, 1e-3], &mut wire);
        encode_silence(16_000, &mut wire);
        encode_audio(&[], &mut wire);
        let mut r = &wire[..];
        assert_eq!(read_frame(&mut r).unwrap(), Some(Frame::Audio(vec![0.0, 0.5, -1.0, 1e-3])));
        assert_eq!(read_frame(&mut r).unwrap(), Some(Frame::Silence(16_000)));
        assert_eq!(read_frame(&mut r).unwrap(), Some(Frame::Audio(vec![])));
        assert_eq!(read_frame(&mut r).unwrap(), None, "clean end between frames");
    }

    #[test]
    fn the_wire_is_little_endian_with_a_byte_length() {
        let mut wire = Vec::new();
        encode_audio(&[1.0], &mut wire);
        assert_eq!(wire, [b'A', 4, 0, 0, 0, 0x00, 0x00, 0x80, 0x3f]);
        wire.clear();
        encode_silence(258, &mut wire);
        assert_eq!(wire, [b'S', 4, 0, 0, 0, 2, 1, 0, 0]);
    }

    #[test]
    fn a_frame_cut_short_or_garbled_is_an_error() {
        let mut wire = Vec::new();
        encode_audio(&[0.25, 0.5], &mut wire);
        for cut in 1..wire.len() {
            let mut r = &wire[..cut];
            assert!(read_frame(&mut r).is_err(), "cut at {cut}");
        }
        let mut r: &[u8] = &[b'X', 0, 0, 0, 0];
        assert!(read_frame(&mut r).is_err(), "unknown tag");
        let mut r: &[u8] = &[b'A', 3, 0, 0, 0, 1, 2, 3];
        assert!(read_frame(&mut r).is_err(), "not whole samples");
        let mut r: &[u8] = &[b'S', 2, 0, 0, 0, 1, 2];
        assert!(read_frame(&mut r).is_err(), "silence carries a u32");
        let huge = (MAX_PAYLOAD + 4).to_le_bytes();
        let mut r: &[u8] = &[b'A', huge[0], huge[1], huge[2], huge[3]];
        assert!(read_frame(&mut r).is_err(), "oversized");
    }

    #[test]
    fn long_audio_goes_out_in_bounded_frames() {
        let n = (MAX_PAYLOAD / 4) as usize + 10;
        let mut wire = Vec::new();
        write_audio(&mut wire, &vec![0.1; n]).unwrap();
        let mut r = &wire[..];
        let mut total = 0;
        while let Some(Frame::Audio(s)) = read_frame(&mut r).unwrap() {
            assert!(s.len() * 4 <= MAX_PAYLOAD as usize);
            total += s.len();
        }
        assert_eq!(total, n);
    }

    #[test]
    fn messages_are_json_lines_in_the_documented_shape() {
        let msg = Message::Segments {
            is_final: true,
            audio_ms: 30_000,
            labelled_ms: 30_000,
            compute_ms: 712,
            lag_ms_median: Some(10_600),
            lag_ms_max: None,
            segments: vec![RawSegment { start_ms: 120, end_ms: 4_200, speaker: 1 }],
        };
        let mut out = Vec::new();
        write_message(&mut out, &msg).unwrap();
        let line = String::from_utf8(out).unwrap();
        assert!(line.ends_with('\n') && !line.trim_end().contains('\n'));
        let v: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["type"], "segments");
        assert_eq!(v["final"], true);
        assert_eq!(v["segments"][0], serde_json::json!({"start_ms": 120, "end_ms": 4200, "speaker": 1}));
        assert!(v.get("lag_ms_max").is_none(), "absent, not null");
        assert_eq!(parse_line(&line), Some(msg));
    }

    #[test]
    fn stray_output_is_not_a_message() {
        assert_eq!(parse_line(""), None);
        assert_eq!(parse_line("ggml_backend: loaded CPU backend"), None);
        assert_eq!(parse_line("{not json"), None);
        assert_eq!(parse_line(r#"{"type":"bogus"}"#), None);
        assert_eq!(
            parse_line(r#"  {"type":"error","message":"model not found"}  "#),
            Some(Message::Error { message: "model not found".into() })
        );
        let ready = r#"{"type":"ready","protocol":1,"speakers":8,"frame_ms":10.0,"runtime":"0.2.0","load_ms":850}"#;
        assert!(matches!(parse_line(ready), Some(Message::Ready { protocol: 1, speakers: 8, .. })));
    }
}

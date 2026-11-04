//! **OpenAI's voices** for spoken replies, on the OpenAI key Language
//! Models already has (`tts/hosted.rs` is the engine; this is OpenAI's
//! part). Nothing goes to OpenAI unless its voice is picked; then the words
//! Yap speaks go there, sentence by sentence.
//!
//! The API as of 31 October 2026 (developers.openai.com: the speech
//! reference and the text-to-speech guide):
//! - `POST /v1/audio/speech`, `Authorization: Bearer <key>`, `{model, input
//!   (≤ 4,096 characters), voice, response_format: "pcm", stream_format:
//!   "sse", speed? (0.25–4, sent only when Yap's isn't 1×)}`. `pcm` is raw
//!   16-bit little-endian mono at 24 kHz (the guide: the fastest format to
//!   start). With `stream_format: "sse"` the answer is server-sent events:
//!   `speech.audio.delta` with base64 `audio`, then `speech.audio.done` with
//!   `usage {input_tokens, output_tokens, total_tokens}` — so the tracker
//!   has the real tokens. An answer that isn't events is read as raw PCM.
//! - **Models**: `gpt-4o-mini-tts` (the default) and its dated snapshot.
//!   ⚠ OpenAI says `gpt-4o-mini-tts` (with `tts-1` and `tts-1-hd`) shuts
//!   down on **6 January 2027**, its successor only on the Realtime API: a
//!   request after that fails as "model not available", and Yap's own voice
//!   reads the answer (said once).
//! - **Voices**: alloy, ash, ballad, coral, echo, fable, nova, onyx, sage,
//!   shimmer, verse, marin, cedar — marin and cedar recommended for quality;
//!   all made for English, fine in Whisper's languages.
//! - **Price**: text in $0.60 and audio out $12 a million tokens
//!   (`ai/usage/prices.rs`).

use serde_json::{json, Value};

use super::cloud::stream::{Decode, Key, Limits, RawPcm, S16Le, SseLines};
use super::cloud::Failure;
use super::hosted::{self, Auth, ModelInfo, Request, Service, VoiceInfo};

pub const ID: &str = "openai";
pub const NAME: &str = "OpenAI";
pub const API: &str = "https://api.openai.com";
pub const DEFAULT_MODEL: &str = "gpt-4o-mini-tts";
pub const DEFAULT_VOICE: &str = "marin";
pub const SAMPLE_RATE: u32 = 24_000;

const MODELS: &[ModelInfo] = &[
    ModelInfo { id: "gpt-4o-mini-tts", name: "GPT-4o mini TTS", desc: "Natural, and follows the tone of what it reads. OpenAI retires it on 6 January 2027" },
    ModelInfo { id: "gpt-4o-mini-tts-2025-12-15", name: "GPT-4o mini TTS (December 2025)", desc: "The newest version of the same model, kept as it is" },
];

const VOICES: &[VoiceInfo] = &[
    VoiceInfo { id: "marin", name: "Marin", desc: "Best quality" },
    VoiceInfo { id: "cedar", name: "Cedar", desc: "Best quality" },
    VoiceInfo { id: "alloy", name: "Alloy", desc: "" },
    VoiceInfo { id: "ash", name: "Ash", desc: "" },
    VoiceInfo { id: "ballad", name: "Ballad", desc: "" },
    VoiceInfo { id: "coral", name: "Coral", desc: "" },
    VoiceInfo { id: "echo", name: "Echo", desc: "" },
    VoiceInfo { id: "fable", name: "Fable", desc: "" },
    VoiceInfo { id: "nova", name: "Nova", desc: "" },
    VoiceInfo { id: "onyx", name: "Onyx", desc: "" },
    VoiceInfo { id: "sage", name: "Sage", desc: "" },
    VoiceInfo { id: "shimmer", name: "Shimmer", desc: "" },
    VoiceInfo { id: "verse", name: "Verse", desc: "" },
];

/// Yap's speed (0.5–2.0) as OpenAI's `speed` (0.25–4), or nothing at 1×.
/// Pure.
pub fn speed_for(rate: f32) -> Option<f64> {
    if !rate.is_finite() || (rate - 1.0).abs() < 0.01 {
        return None;
    }
    Some((f64::from(rate.clamp(0.25, 4.0)) * 100.0).round() / 100.0)
}

pub struct OpenAi;

impl Service for OpenAi {
    fn id(&self) -> &'static str {
        ID
    }
    fn name(&self) -> &'static str {
        NAME
    }
    fn api(&self) -> &'static str {
        API
    }
    fn env(&self) -> &'static str {
        "YAP_OPENAI_TTS_URL"
    }
    fn default_model(&self) -> &'static str {
        DEFAULT_MODEL
    }
    fn default_voice(&self) -> &'static str {
        DEFAULT_VOICE
    }
    fn models(&self) -> &'static [ModelInfo] {
        MODELS
    }
    fn voices(&self) -> &'static [VoiceInfo] {
        VOICES
    }
    fn request(&self, base: &str, key: &Key, voice: &str, model: &str, rate: f32, text: &str) -> Request {
        let mut body = json!({
            "model": hosted::model_or_default(self, model),
            "input": text,
            "voice": hosted::voice_or_default(self, voice),
            "response_format": "pcm",
            "stream_format": "sse",
        });
        if let Some(speed) = speed_for(rate) {
            body["speed"] = json!(speed);
        }
        Request { url: format!("{base}/v1/audio/speech"), auth: Auth::Bearer, key: key.clone(), body }
    }
    fn decoder(&self, content_type: &str) -> Box<dyn Decode> {
        if content_type.contains("event-stream") {
            Box::new(Events::default())
        } else {
            Box::new(RawPcm::new(SAMPLE_RATE))
        }
    }
    fn classify(&self, status: u16, body: &str, key: &Key) -> Failure {
        hosted::classify_openai_shape(status, body, key, &["insufficient_quota", "billing_hard_limit_reached"])
    }
    fn limits(&self) -> Limits {
        Limits { first_audio: std::time::Duration::from_secs(6), gap: std::time::Duration::from_secs(6) }
    }
}

/// `stream_format: "sse"`'s answer as it arrives: `speech.audio.delta`'s
/// base64 PCM, `speech.audio.done`'s usage, an `error` event as a failure.
#[derive(Debug, Default)]
pub struct Events {
    lines: SseLines,
    pcm: S16Le,
    usage: Option<(u64, u64)>,
}

impl Events {
    fn event(&mut self, data: &str) -> Result<Vec<f32>, Failure> {
        use base64::Engine as _;
        if data == "[DONE]" {
            return Ok(Vec::new());
        }
        let odd = || Failure::Rejected(format!("{NAME} answered with something unexpected"));
        let v: Value = serde_json::from_str(data).map_err(|_| odd())?;
        match v["type"].as_str().unwrap_or("") {
            "speech.audio.delta" => {
                let b64 = v["audio"].as_str().unwrap_or("");
                let bytes = base64::engine::general_purpose::STANDARD.decode(b64).map_err(|_| odd())?;
                Ok(self.pcm.push(&bytes))
            }
            "speech.audio.done" => {
                let u = &v["usage"];
                if let (Some(i), Some(o)) = (u["input_tokens"].as_u64(), u["output_tokens"].as_u64()) {
                    self.usage = Some((i, o));
                }
                Ok(Vec::new())
            }
            "error" => Err(Failure::Rejected(
                v["error"]["message"].as_str().or(v["message"].as_str()).unwrap_or("").chars().take(120).collect(),
            )),
            _ => Ok(Vec::new()),
        }
    }
}

impl Decode for Events {
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<f32>, Failure> {
        let mut out = Vec::new();
        for d in self.lines.push(bytes) {
            out.extend(self.event(&d)?);
        }
        Ok(out)
    }
    fn finish(&mut self) -> Result<Vec<f32>, Failure> {
        let mut out = Vec::new();
        for d in self.lines.finish() {
            out.extend(self.event(&d)?);
        }
        Ok(out)
    }
    fn format(&self) -> (u32, u16) {
        (SAMPLE_RATE, 1)
    }
    fn usage(&self) -> Option<(u64, u64)> {
        self.usage
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::tts::elevenlabs::standin::{pcm_bytes, Reply};
    use crate::voice::tts::hosted::tests::through;

    const SECRET: &str = "sk-proj-test-0123456789";

    /// The answer OpenAI streams for `samples` of PCM: deltas of `per` bytes,
    /// then the usage.
    pub(crate) fn events(samples: usize, per: usize) -> Vec<u8> {
        use base64::Engine as _;
        let pcm = pcm_bytes(samples);
        let mut out = String::new();
        for c in pcm.chunks(per) {
            let b64 = base64::engine::general_purpose::STANDARD.encode(c);
            out.push_str(&format!("data: {}\n\n", json!({ "type": "speech.audio.delta", "audio": b64 })));
        }
        out.push_str(&format!("data: {}\n\n", json!({ "type": "speech.audio.done", "usage": { "input_tokens": 14, "output_tokens": 101, "total_tokens": 115 } })));
        out.into_bytes()
    }

    #[test]
    fn the_request_asks_for_streamed_pcm_with_the_key_in_a_header_only() {
        let key = Key::new(SECRET);
        let req = OpenAi.request("https://api.openai.com", &key, "cedar", "", 1.0, "Hello there.");
        assert_eq!(req.url, "https://api.openai.com/v1/audio/speech");
        assert_eq!(req.auth, Auth::Bearer);
        assert_eq!(
            req.body,
            json!({ "model": "gpt-4o-mini-tts", "input": "Hello there.", "voice": "cedar", "response_format": "pcm", "stream_format": "sse" })
        );
        let logged = format!("{req:?}");
        assert!(!logged.contains(SECRET) && logged.contains("[redacted]"), "{logged}");
        // Speed only when it isn't 1×; an unknown voice or model: the defaults.
        let fast = OpenAi.request("b", &key, "nobody", "gpt-9-tts", 1.5, "x");
        assert_eq!((fast.body["speed"].clone(), fast.body["voice"].clone(), fast.body["model"].clone()), (json!(1.5), json!("marin"), json!("gpt-4o-mini-tts")));
        assert_eq!(speed_for(0.5), Some(0.5));
        assert_eq!(speed_for(1.0), None);
        let snap = OpenAi.request("b", &key, "", "gpt-4o-mini-tts-2025-12-15", 1.0, "x");
        assert_eq!(snap.body["model"], "gpt-4o-mini-tts-2025-12-15");
    }

    #[test]
    fn the_events_decode_in_any_split_with_their_usage() {
        let body = events(4_800, 1_001);
        let want = S16Le::default().push(&pcm_bytes(4_800));
        for step in [1usize, 7, 64, 1_000, 100_000] {
            let mut d = Events::default();
            let mut got = Vec::new();
            for c in body.chunks(step) {
                got.extend(d.push(c).unwrap());
            }
            got.extend(d.finish().unwrap());
            assert_eq!(got, want, "split every {step}");
            assert_eq!(d.usage(), Some((14, 101)));
        }
        // An error mid-stream is said; something that isn't JSON is odd.
        let mut d = Events::default();
        assert!(matches!(d.push(b"data: {\"type\":\"error\",\"error\":{\"message\":\"Too long\"}}\n"), Err(Failure::Rejected(m)) if m == "Too long"));
        assert!(Events::default().push(b"data: <html>\n").is_err());
        // Raw PCM when the answer isn't events.
        assert_eq!(OpenAi.decoder("audio/pcm").format(), (24_000, 1));
    }

    #[test]
    fn errors_are_named() {
        let k = Key::new(SECRET);
        let e = |code: &str| format!(r#"{{"error":{{"message":"m","type":"t","code":"{code}"}}}}"#);
        assert_eq!(OpenAi.classify(401, &e("invalid_api_key"), &k), Failure::BadKey);
        assert_eq!(OpenAi.classify(429, &e("insufficient_quota"), &k), Failure::Quota);
        assert_eq!(OpenAi.classify(429, &e("rate_limit_exceeded"), &k), Failure::RateLimited);
        assert_eq!(OpenAi.classify(404, &e("model_not_found"), &k), Failure::ModelGone);
        assert_eq!(OpenAi.classify(403, &e("unsupported_country_region_territory"), &k), Failure::NotAllowed);
    }

    #[test]
    fn a_sentence_streams_through_the_engine() {
        let (r, pieces, asked) = through(
            &OpenAi,
            vec![Reply::Body { content_type: "text/event-stream", body: events(12_000, 3_001), pieces: vec![5, 900, 4_000, 3, 50_000] }],
            "cedar",
            "",
            1.0,
            "Hello there.",
        );
        let done = r.unwrap();
        assert!(!done.stopped && done.first_audio.is_some());
        let all: usize = pieces.iter().map(|p| p.samples.len()).sum();
        // Every sample, then 150 ms of quiet.
        assert_eq!(all, 12_000 + 3_600);
        assert!(pieces.iter().all(|p| p.sample_rate == 24_000 && p.channels == 1));
        let raw = &asked[0];
        assert!(raw.starts_with("POST /v1/audio/speech HTTP/1.1"), "{raw}");
        assert!(raw.to_ascii_lowercase().contains("authorization: bearer sk-standin-0123456789"));
        assert!(raw.contains(r#""stream_format":"sse""#) && raw.contains(r#""voice":"cedar""#));
        // Refused: named.
        let (r, _, _) = through(&OpenAi, vec![Reply::Error(401, r#"{"error":{"message":"Incorrect API key provided","type":"invalid_request_error","code":"invalid_api_key"}}"#.into())], "", "", 1.0, "Hi.");
        assert_eq!(r, Err(Failure::BadKey));
    }
}

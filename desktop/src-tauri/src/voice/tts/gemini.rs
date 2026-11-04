//! **Gemini's voices** for spoken replies, on the Gemini key Language
//! Models already has (`tts/hosted.rs` is the engine; this is Gemini's
//! part). Nothing goes to Google unless its voice is picked; then the words
//! Yap speaks go there, sentence by sentence.
//!
//! The API as of 31 October 2026 (ai.google.dev: speech generation,
//! streaming, pricing):
//! - Speech is on the **Interactions API**: `POST /v1beta/interactions`,
//!   `x-goog-api-key: <key>`, `{model, input: [{type: "user_input",
//!   content: [{type: "text", text, annotations?: [{type:
//!   "speech_metadata", style}]}]}], response_format: {type: "audio"},
//!   generation_config: {speech_config: [{voice}]}, stream: true}`. Streamed,
//!   the audio is headerless 16-bit little-endian mono PCM at 24 kHz
//!   (`audio/l16`), in server-sent events: `step.delta` events whose
//!   `delta.type` is "audio" carry it base64 in `delta.data`;
//!   `interaction.completed` carries `interaction.usage {total_input_tokens,
//!   total_output_tokens}`; an `error` event `{error: {message, code}}`; the
//!   stream ends `data: [DONE]`. Audio that does come with a WAV header is
//!   read past it (`WavOrRaw`).
//! - **Speed**: there's no speed setting — the delivery is asked for in the
//!   style ("Speak at a brisk pace"), so it's a request, not exact.
//! - **Models**: `gemini-3.8-flash-lite-tts` (the default: "a fast,
//!   cost-efficient workhorse") and `gemini-3.8-flash-tts` (more expressive).
//!   Language is detected by itself.
//! - **Voices**: 30, each with Google's one-word description (Kore "Firm",
//!   Puck "Upbeat"…).
//! - **Price** (paid tier; the free tier costs nothing): Flash-Lite TTS text
//!   in $0.50 and audio out $6 a million tokens until 31 December 2026, then
//!   $1 and $12; Flash TTS $0.50 and $9, then $1 and $18; 25 audio tokens a
//!   second (`ai/usage/prices.rs`).

use serde_json::{json, Value};

use super::cloud::stream::{tidy, Decode, Key, Limits, SseLines, WavOrRaw};
use super::cloud::Failure;
use super::hosted::{self, Auth, ModelInfo, Request, Service, VoiceInfo};

pub const ID: &str = "gemini";
pub const NAME: &str = "Gemini";
pub const API: &str = "https://generativelanguage.googleapis.com";
pub const DEFAULT_MODEL: &str = "gemini-3.8-flash-lite-tts";
pub const DEFAULT_VOICE: &str = "Kore";
pub const SAMPLE_RATE: u32 = 24_000;
const ODD: &str = "Gemini answered with audio Yap can't play";

const MODELS: &[ModelInfo] = &[
    ModelInfo { id: "gemini-3.8-flash-lite-tts", name: "Gemini 3.8 Flash-Lite TTS", desc: "Quick and the cheapest" },
    ModelInfo { id: "gemini-3.8-flash-tts", name: "Gemini 3.8 Flash TTS", desc: "More expressive, costs a little more" },
];

const VOICES: &[VoiceInfo] = &[
    VoiceInfo { id: "Kore", name: "Kore", desc: "Firm" },
    VoiceInfo { id: "Zephyr", name: "Zephyr", desc: "Bright" },
    VoiceInfo { id: "Puck", name: "Puck", desc: "Upbeat" },
    VoiceInfo { id: "Charon", name: "Charon", desc: "Informative" },
    VoiceInfo { id: "Fenrir", name: "Fenrir", desc: "Excitable" },
    VoiceInfo { id: "Leda", name: "Leda", desc: "Youthful" },
    VoiceInfo { id: "Orus", name: "Orus", desc: "Firm" },
    VoiceInfo { id: "Aoede", name: "Aoede", desc: "Breezy" },
    VoiceInfo { id: "Callirrhoe", name: "Callirrhoe", desc: "Easy-going" },
    VoiceInfo { id: "Autonoe", name: "Autonoe", desc: "Bright" },
    VoiceInfo { id: "Enceladus", name: "Enceladus", desc: "Breathy" },
    VoiceInfo { id: "Iapetus", name: "Iapetus", desc: "Clear" },
    VoiceInfo { id: "Umbriel", name: "Umbriel", desc: "Easy-going" },
    VoiceInfo { id: "Algieba", name: "Algieba", desc: "Smooth" },
    VoiceInfo { id: "Despina", name: "Despina", desc: "Smooth" },
    VoiceInfo { id: "Erinome", name: "Erinome", desc: "Clear" },
    VoiceInfo { id: "Algenib", name: "Algenib", desc: "Gravelly" },
    VoiceInfo { id: "Rasalgethi", name: "Rasalgethi", desc: "Informative" },
    VoiceInfo { id: "Laomedeia", name: "Laomedeia", desc: "Upbeat" },
    VoiceInfo { id: "Achernar", name: "Achernar", desc: "Soft" },
    VoiceInfo { id: "Alnilam", name: "Alnilam", desc: "Firm" },
    VoiceInfo { id: "Schedar", name: "Schedar", desc: "Even" },
    VoiceInfo { id: "Gacrux", name: "Gacrux", desc: "Mature" },
    VoiceInfo { id: "Pulcherrima", name: "Pulcherrima", desc: "Forward" },
    VoiceInfo { id: "Achird", name: "Achird", desc: "Friendly" },
    VoiceInfo { id: "Zubenelgenubi", name: "Zubenelgenubi", desc: "Casual" },
    VoiceInfo { id: "Vindemiatrix", name: "Vindemiatrix", desc: "Gentle" },
    VoiceInfo { id: "Sadachbia", name: "Sadachbia", desc: "Lively" },
    VoiceInfo { id: "Sadaltager", name: "Sadaltager", desc: "Knowledgeable" },
    VoiceInfo { id: "Sulafat", name: "Sulafat", desc: "Warm" },
];

/// Yap's speed as a style the model is asked to speak in (Gemini has no
/// speed setting), or none at about 1×. Pure.
pub fn style_for(rate: f32) -> Option<&'static str> {
    if !rate.is_finite() {
        return None;
    }
    match rate {
        r if r >= 1.45 => Some("Speak quickly"),
        r if r >= 1.15 => Some("Speak at a brisk pace"),
        r if r <= 0.65 => Some("Speak slowly"),
        r if r <= 0.85 => Some("Speak at an unhurried pace"),
        _ => None,
    }
}

pub struct Gemini;

impl Service for Gemini {
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
        "YAP_GEMINI_TTS_URL"
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
        let mut part = json!({ "type": "text", "text": text });
        if let Some(style) = style_for(rate) {
            part["annotations"] = json!([{ "type": "speech_metadata", "style": style }]);
        }
        let body = json!({
            "model": hosted::model_or_default(self, model),
            "input": [{ "type": "user_input", "content": [part] }],
            "response_format": { "type": "audio" },
            "generation_config": { "speech_config": [{ "voice": hosted::voice_or_default(self, voice) }] },
            "stream": true,
        });
        Request { url: format!("{base}/v1beta/interactions"), auth: Auth::Header("x-goog-api-key"), key: key.clone(), body }
    }
    fn decoder(&self, content_type: &str) -> Box<dyn Decode> {
        Box::new(Events::new(content_type.contains("event-stream")))
    }
    fn classify(&self, status: u16, body: &str, key: &Key) -> Failure {
        classify(status, body, key)
    }
    /// Gemini's speech can take a few seconds to start.
    fn limits(&self) -> Limits {
        Limits { first_audio: std::time::Duration::from_secs(12), gap: std::time::Duration::from_secs(8) }
    }
}

/// Google's error answer `{"error": {"code", "message", "status",
/// "details": [{"reason"}]}}` as a [`Failure`]. Pure.
pub fn classify(status: u16, body: &str, key: &Key) -> Failure {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let e = &v["error"];
    let message = e["message"].as_str().unwrap_or("");
    let state = e["status"].as_str().unwrap_or("");
    let reasons: Vec<&str> = e["details"].as_array().into_iter().flatten().filter_map(|d| d["reason"].as_str()).collect();
    if reasons.iter().any(|r| r.starts_with("API_KEY")) || message.contains("API key not valid") {
        return Failure::BadKey;
    }
    match (status, state) {
        (401, _) | (_, "UNAUTHENTICATED") => Failure::BadKey,
        (403, _) | (_, "PERMISSION_DENIED") => Failure::NotAllowed,
        (429, _) | (_, "RESOURCE_EXHAUSTED") => {
            // A day's free requests (or the project's money) used up, else a
            // limit a minute.
            if body.contains("PerDay") || message.to_ascii_lowercase().contains("billing") {
                Failure::Quota
            } else {
                Failure::RateLimited
            }
        }
        (404, _) | (_, "NOT_FOUND") => Failure::ModelGone,
        (s, _) if s >= 500 => Failure::ServiceDown,
        _ => Failure::Rejected(tidy(message, key)),
    }
}

/// The streamed answer as it arrives: `step.delta` audio (base64 PCM, a WAV
/// header read past), `interaction.completed`'s usage, an `error` event as
/// a failure. Not events: the body is the audio itself.
pub struct Events {
    sse: Option<SseLines>,
    audio: WavOrRaw,
    usage: Option<(u64, u64)>,
}

impl Events {
    pub fn new(events: bool) -> Events {
        Events { sse: events.then(SseLines::default), audio: WavOrRaw::new(SAMPLE_RATE, ODD), usage: None }
    }

    fn event(&mut self, data: &str) -> Result<Vec<f32>, Failure> {
        use base64::Engine as _;
        if data == "[DONE]" {
            return Ok(Vec::new());
        }
        let odd = || Failure::Rejected(format!("{NAME} answered with something unexpected"));
        let v: Value = serde_json::from_str(data).map_err(|_| odd())?;
        match v["event_type"].as_str().unwrap_or("") {
            "step.delta" if v["delta"]["type"] == "audio" => {
                let b64 = v["delta"]["data"].as_str().unwrap_or("");
                let bytes = base64::engine::general_purpose::STANDARD.decode(b64).map_err(|_| odd())?;
                self.audio.push(&bytes)
            }
            "interaction.completed" => {
                let u = &v["interaction"]["usage"];
                if let (Some(i), Some(o)) = (u["total_input_tokens"].as_u64(), u["total_output_tokens"].as_u64()) {
                    self.usage = Some((i, o));
                }
                Ok(Vec::new())
            }
            "error" => Err(Failure::Rejected(v["error"]["message"].as_str().unwrap_or("").chars().take(120).collect())),
            _ => Ok(Vec::new()),
        }
    }
}

impl Decode for Events {
    fn push(&mut self, bytes: &[u8]) -> Result<Vec<f32>, Failure> {
        let Some(sse) = self.sse.as_mut() else {
            return self.audio.push(bytes);
        };
        let lines = sse.push(bytes);
        let mut out = Vec::new();
        for d in lines {
            out.extend(self.event(&d)?);
        }
        Ok(out)
    }
    fn finish(&mut self) -> Result<Vec<f32>, Failure> {
        let mut out = Vec::new();
        if let Some(sse) = self.sse.as_mut() {
            for d in sse.finish() {
                out.extend(self.event(&d)?);
            }
        }
        out.extend(self.audio.finish()?);
        Ok(out)
    }
    fn format(&self) -> (u32, u16) {
        self.audio.format()
    }
    fn usage(&self) -> Option<(u64, u64)> {
        self.usage
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::tts::cloud::stream::S16Le;
    use crate::voice::tts::elevenlabs::standin::{pcm_bytes, Reply};
    use crate::voice::tts::hosted::tests::through;

    const SECRET: &str = "AIzaSy-test-0123456789";

    /// Gemini's stream for `samples` of PCM: the created and step events,
    /// deltas of `per` bytes, the completed interaction with its usage.
    fn events(samples: usize, per: usize) -> Vec<u8> {
        use base64::Engine as _;
        let mut out = String::new();
        let mut ev = |name: &str, data: Value| out.push_str(&format!("event: {name}\ndata: {data}\n\n"));
        ev("interaction.created", json!({ "event_type": "interaction.created", "interaction": { "id": "i1", "status": "in_progress" } }));
        ev("step.start", json!({ "event_type": "step.start", "index": 0, "step": { "type": "model_output" } }));
        for c in pcm_bytes(samples).chunks(per) {
            let data = base64::engine::general_purpose::STANDARD.encode(c);
            ev("step.delta", json!({ "event_type": "step.delta", "index": 0, "delta": { "type": "audio", "data": data } }));
        }
        ev("step.stop", json!({ "event_type": "step.stop", "index": 0 }));
        ev("interaction.completed", json!({ "event_type": "interaction.completed", "interaction": { "id": "i1", "status": "completed", "usage": { "total_tokens": 160, "total_input_tokens": 10, "total_output_tokens": 150 } } }));
        out.push_str("event: done\ndata: [DONE]\n\n");
        out.into_bytes()
    }

    #[test]
    fn the_request_names_the_voice_and_streams_with_the_key_in_a_header_only() {
        let key = Key::new(SECRET);
        let req = Gemini.request("https://generativelanguage.googleapis.com", &key, "puck", "", 1.0, "Hello there.");
        assert_eq!(req.url, "https://generativelanguage.googleapis.com/v1beta/interactions");
        assert_eq!(req.auth, Auth::Header("x-goog-api-key"));
        assert_eq!(
            req.body,
            json!({
                "model": "gemini-3.8-flash-lite-tts",
                "input": [{ "type": "user_input", "content": [{ "type": "text", "text": "Hello there." }] }],
                "response_format": { "type": "audio" },
                "generation_config": { "speech_config": [{ "voice": "Puck" }] },
                "stream": true,
            })
        );
        assert!(!format!("{req:?}").contains(SECRET));
        // Faster or slower: asked for in the style.
        let fast = Gemini.request("b", &key, "", "gemini-3.8-flash-tts", 1.5, "x");
        assert_eq!(fast.body["input"][0]["content"][0]["annotations"], json!([{ "type": "speech_metadata", "style": "Speak quickly" }]));
        assert_eq!(fast.body["model"], "gemini-3.8-flash-tts");
        assert_eq!(style_for(1.0), None);
        assert_eq!(style_for(1.2), Some("Speak at a brisk pace"));
        assert_eq!(style_for(0.5), Some("Speak slowly"));
        assert_eq!(style_for(0.8), Some("Speak at an unhurried pace"));
    }

    #[test]
    fn the_events_decode_in_any_split_with_their_usage() {
        let body = events(4_800, 777);
        let want = S16Le::default().push(&pcm_bytes(4_800));
        for step in [1usize, 9, 100, 4_096, 1 << 20] {
            let mut d = Events::new(true);
            let mut got = Vec::new();
            for c in body.chunks(step) {
                got.extend(d.push(c).unwrap());
            }
            got.extend(d.finish().unwrap());
            assert_eq!(got, want, "split every {step}");
            assert_eq!(d.usage(), Some((10, 150)));
            assert_eq!(d.format(), (24_000, 1));
        }
        let mut d = Events::new(true);
        let err = b"event: error\ndata: {\"error\":{\"message\":\"Deadline expired\",\"code\":\"gateway_timeout\"},\"event_type\":\"error\"}\n\n";
        assert!(matches!(d.push(err), Err(Failure::Rejected(m)) if m == "Deadline expired"));
    }

    #[test]
    fn errors_are_named() {
        let k = Key::new(SECRET);
        let g = |code: u16, status: &str, message: &str| format!(r#"{{"error":{{"code":{code},"message":"{message}","status":"{status}"}}}}"#);
        // Google says a bad key with a 400.
        assert_eq!(classify(400, &g(400, "INVALID_ARGUMENT", "API key not valid. Please pass a valid API key."), &k), Failure::BadKey);
        assert_eq!(
            classify(400, r#"{"error":{"code":400,"message":"x","status":"INVALID_ARGUMENT","details":[{"reason":"API_KEY_INVALID"}]}}"#, &k),
            Failure::BadKey
        );
        assert_eq!(classify(403, &g(403, "PERMISSION_DENIED", "denied"), &k), Failure::NotAllowed);
        assert_eq!(classify(429, &g(429, "RESOURCE_EXHAUSTED", "Too many"), &k), Failure::RateLimited);
        assert_eq!(
            classify(429, r#"{"error":{"code":429,"message":"You exceeded your current quota","status":"RESOURCE_EXHAUSTED","details":[{"violations":[{"quotaId":"GenerateRequestsPerDayPerProjectPerModel-FreeTier"}]}]}}"#, &k),
            Failure::Quota
        );
        assert_eq!(classify(404, &g(404, "NOT_FOUND", "no such model"), &k), Failure::ModelGone);
        assert_eq!(classify(500, "", &k), Failure::ServiceDown);
        assert_eq!(classify(400, &g(400, "INVALID_ARGUMENT", "Voice Nobody is not supported"), &k), Failure::Rejected("Voice Nobody is not supported".into()));
    }

    #[test]
    fn a_sentence_streams_through_the_engine() {
        let (r, pieces, asked) = through(
            &Gemini,
            vec![Reply::Body { content_type: "text/event-stream", body: events(12_000, 2_048), pieces: vec![3, 700, 5_000, 1, 10_000] }],
            "Sulafat",
            "",
            1.0,
            "Hello there.",
        );
        let done = r.unwrap();
        assert!(!done.stopped);
        let all: usize = pieces.iter().map(|p| p.samples.len()).sum();
        assert_eq!(all, 12_000 + 3_600);
        let raw = &asked[0];
        assert!(raw.starts_with("POST /v1beta/interactions HTTP/1.1"), "{raw}");
        assert!(raw.to_ascii_lowercase().contains("x-goog-api-key: sk-standin-0123456789"));
        assert!(raw.contains(r#""voice":"Sulafat""#) && raw.contains(r#""stream":true"#));
    }
}

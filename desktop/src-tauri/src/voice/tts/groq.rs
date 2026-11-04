//! **Groq's voices** for spoken replies, on the Groq key Language Models
//! already has (`tts/hosted.rs` is the engine; this is Groq's part).
//! Nothing goes to Groq unless its voice is picked; then the words Yap
//! speaks go there, sentence by sentence.
//!
//! The API as of 31 October 2026 (console.groq.com: text to speech,
//! Orpheus):
//! - `POST /openai/v1/audio/speech`, `Authorization: Bearer <key>`,
//!   `{model, input, voice, response_format: "wav"}` — WAV is the only
//!   format, and each request takes **at most 200 characters**: a longer
//!   sentence goes as several requests, cut where it breathes
//!   (`hosted::split`), the pause after the last. Not streamed: each WAV is
//!   read as its bytes come (`WavOrRaw`), its own rate and channels.
//! - **Model**: `canopylabs/orpheus-v1-english` ("expressive", English;
//!   Orpheus' Saudi Arabic model isn't offered: Yap's text for the voice is
//!   English-shaped). No speed setting: it speaks at its own pace.
//! - **Voices**: autumn, diana, hannah (female); austin, daniel, troy
//!   (male).
//! - **Price**: $22 a million characters (`ai/usage/prices.rs`).
//! - Errors are OpenAI's shape `{"error": {"message", "type", "code"}}`; one
//!   that asks for the model's terms to be accepted first comes back in
//!   Groq's own words.

use serde_json::json;

use super::cloud::stream::{Decode, Key, Limits, WavOrRaw};
use super::cloud::Failure;
use super::hosted::{self, Auth, ModelInfo, Request, Service, VoiceInfo};

pub const ID: &str = "groq";
pub const NAME: &str = "Groq";
pub const API: &str = "https://api.groq.com";
pub const DEFAULT_MODEL: &str = "canopylabs/orpheus-v1-english";
pub const DEFAULT_VOICE: &str = "hannah";
/// The longest text one request takes.
pub const MAX_CHARS: usize = 200;
const ODD: &str = "Groq answered with audio Yap can't play";

const MODELS: &[ModelInfo] = &[ModelInfo { id: "canopylabs/orpheus-v1-english", name: "Orpheus English", desc: "Expressive, English only, one speed" }];

const VOICES: &[VoiceInfo] = &[
    VoiceInfo { id: "hannah", name: "Hannah", desc: "Female" },
    VoiceInfo { id: "autumn", name: "Autumn", desc: "Female" },
    VoiceInfo { id: "diana", name: "Diana", desc: "Female" },
    VoiceInfo { id: "austin", name: "Austin", desc: "Male" },
    VoiceInfo { id: "daniel", name: "Daniel", desc: "Male" },
    VoiceInfo { id: "troy", name: "Troy", desc: "Male" },
];

pub struct Groq;

impl Service for Groq {
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
        "YAP_GROQ_TTS_URL"
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
    fn request(&self, base: &str, key: &Key, voice: &str, model: &str, _rate: f32, text: &str) -> Request {
        let body = json!({
            "model": hosted::model_or_default(self, model),
            "input": text,
            "voice": hosted::voice_or_default(self, voice),
            "response_format": "wav",
        });
        Request { url: format!("{base}/openai/v1/audio/speech"), auth: Auth::Bearer, key: key.clone(), body }
    }
    fn decoder(&self, _content_type: &str) -> Box<dyn Decode> {
        Box::new(WavOrRaw::new(24_000, ODD))
    }
    fn classify(&self, status: u16, body: &str, key: &Key) -> Failure {
        hosted::classify_openai_shape(status, body, key, &[])
    }
    fn max_chars(&self) -> usize {
        MAX_CHARS
    }
    /// The whole piece is made before it's sent.
    fn limits(&self) -> Limits {
        Limits { first_audio: std::time::Duration::from_secs(8), gap: std::time::Duration::from_secs(6) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::tts::elevenlabs::standin::Reply;
    use crate::voice::tts::hosted::tests::through;

    const SECRET: &str = "gsk_test_0123456789";

    /// A WAV as Groq sends it: 16-bit mono at `rate`.
    fn wav(rate: u32, samples: usize) -> Vec<u8> {
        let data: Vec<u8> = (0..samples).flat_map(|i| ((i % 300) as i16 * 40 - 6_000).to_le_bytes()).collect();
        let mut w = Vec::new();
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&rate.to_le_bytes());
        w.extend_from_slice(&(rate * 2).to_le_bytes());
        w.extend_from_slice(&2u16.to_le_bytes());
        w.extend_from_slice(&16u16.to_le_bytes());
        w.extend_from_slice(b"data");
        w.extend_from_slice(&(data.len() as u32).to_le_bytes());
        w.extend_from_slice(&data);
        w
    }

    #[test]
    fn the_request_asks_for_a_wav_with_the_key_in_a_header_only() {
        let key = Key::new(SECRET);
        let req = Groq.request("https://api.groq.com", &key, "troy", "", 1.5, "Hello there.");
        assert_eq!(req.url, "https://api.groq.com/openai/v1/audio/speech");
        assert_eq!(req.auth, Auth::Bearer);
        // No speed: Orpheus has one.
        assert_eq!(req.body, json!({ "model": "canopylabs/orpheus-v1-english", "input": "Hello there.", "voice": "troy", "response_format": "wav" }));
        assert!(!format!("{req:?}").contains(SECRET));
        assert_eq!(Groq.request("b", &key, "Nobody", "", 1.0, "x").body["voice"], "hannah");
    }

    #[test]
    fn a_long_sentence_goes_as_pieces_of_200_characters_the_pause_after_the_last() {
        let text = "This sentence is long enough that Groq can't take it in one go, because its speech model reads at most two hundred characters a request, so Yap cuts it where it breathes, at a comma or a space, and asks for each piece in turn.";
        assert!(text.chars().count() > MAX_CHARS);
        let (r, pieces, asked) = through(
            &Groq,
            vec![
                Reply::Body { content_type: "audio/wav", body: wav(24_000, 4_800), pieces: vec![10, 40, 999, 100_000] },
                Reply::Body { content_type: "audio/wav", body: wav(24_000, 2_400), pieces: vec![44, 7] },
            ],
            "hannah",
            "",
            1.0,
            text,
        );
        let done = r.unwrap();
        assert!(!done.stopped);
        assert_eq!(asked.len(), 2);
        let inputs: Vec<String> = asked
            .iter()
            .map(|raw| {
                let body = &raw[raw.find("\r\n\r\n").unwrap() + 4..];
                serde_json::from_str::<serde_json::Value>(body).unwrap()["input"].as_str().unwrap().to_string()
            })
            .collect();
        assert!(inputs.iter().all(|i| i.chars().count() <= MAX_CHARS), "{inputs:?}");
        assert_eq!(inputs.join(" "), text);
        // Both pieces' audio, then one pause of 150 ms.
        let all: usize = pieces.iter().map(|p| p.samples.len()).sum();
        assert_eq!(all, 4_800 + 2_400 + 3_600);
        assert!(asked[0].to_ascii_lowercase().contains("authorization: bearer sk-standin-0123456789"));
        assert!(asked[0].starts_with("POST /openai/v1/audio/speech HTTP/1.1"));
    }

    #[test]
    fn its_own_rate_is_kept_and_errors_are_named() {
        let (r, pieces, _) = through(&Groq, vec![Reply::Body { content_type: "audio/wav", body: wav(22_050, 2_205), pieces: vec![] }], "", "", 1.0, "Hi there.");
        assert!(r.is_ok());
        assert!(pieces.iter().all(|p| p.sample_rate == 22_050));
        let k = Key::new(SECRET);
        assert_eq!(Groq.classify(401, r#"{"error":{"message":"Invalid API Key","type":"invalid_request_error","code":"invalid_api_key"}}"#, &k), Failure::BadKey);
        assert_eq!(Groq.classify(429, r#"{"error":{"message":"Rate limit reached","type":"tokens","code":"rate_limit_exceeded"}}"#, &k), Failure::RateLimited);
        assert_eq!(Groq.classify(404, r#"{"error":{"message":"The model does not exist","type":"invalid_request_error","code":"model_not_found"}}"#, &k), Failure::ModelGone);
        let terms = r#"{"error":{"message":"The model `canopylabs/orpheus-v1-english` requires terms acceptance. Please have the org admin accept the terms at https://console.groq.com/playground","type":"invalid_request_error","code":"model_terms_required"}}"#;
        assert!(matches!(Groq.classify(400, terms, &k), Failure::Rejected(m) if m.contains("terms acceptance")));
    }
}

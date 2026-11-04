//! **Cloud voices on a key Yap already has** — OpenAI (`tts/openai.rs`),
//! Gemini (`tts/gemini.rs`) and Groq (`tts/groq.rs`) as voices for spoken
//! replies, beside Kokoro, Windows voices and ElevenLabs. Each speaks on
//! the key Language Models keeps for that provider (`ai_model::provider_key`:
//! Chat's own, else the one every scope shares): no new key, and Settings
//! offers a provider only once it has one.
//!
//! One engine ([`Hosted`]) over a [`Service`] each provider fills in: its
//! request (pure, unit-tested), how its answer's audio reads as it arrives
//! (`cloud/stream.rs` [`Decode`]rs: raw PCM, server-sent events, a WAV), its
//! errors as a [`Failure`], its voices and models. What they share with
//! ElevenLabs (`cloud.rs`): the key only in a header, never a URL, a log
//! line or an error; no redirects; a stop ends a sentence being fetched;
//! the network receipt ("Voice · OpenAI"); a failure hands the rest of the
//! answer to the local voice, said once; Yap's own word timing from the
//! audio (`marks::from_audio`: none of the three says when its words start).
//! Each request's tokens (or characters) go to the AI usage tracker as
//! "Spoken replies", priced where the provider lists a price
//! (`ai/usage/prices.rs`).
//!
//! **Test mode** never reaches the internet: each provider's debug-only
//! `YAP_<PROVIDER>_TTS_URL` names a stand-in (the e2e suite's
//! `support/fake-cloud-tts.js`), else every call fails with "Test runs never
//! go on the internet."

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;

use super::cloud::stream::{self, Decode, Key, Limits, Streamed};
use super::cloud::{self, Failure};
use super::{Engine, Pcm, Synth, Voice};
use crate::privacy::net::Recorded as _;

/// The pause after each sentence (Kokoro's is 180 ms, ElevenLabs' 150 ms).
const SENTENCE_GAP_MS: usize = 150;

/// How a request carries its key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Auth {
    /// `Authorization: Bearer <key>` (OpenAI, Groq).
    Bearer,
    /// The key alone in this header (Gemini's `x-goog-api-key`).
    Header(&'static str),
}

/// One request: where, how the key goes (a header, never the URL) and what
/// body. `Debug` shows the key as `[redacted]`.
#[derive(Debug, Clone)]
pub struct Request {
    pub url: String,
    pub auth: Auth,
    pub key: Key,
    pub body: Value,
}

/// A voice a provider offers (Settings' list).
#[derive(Debug, Clone, Copy)]
pub struct VoiceInfo {
    pub id: &'static str,
    pub name: &'static str,
    /// "Bright", "Female"… ("" for none).
    pub desc: &'static str,
}

/// A model a provider offers for speech.
#[derive(Debug, Clone, Copy)]
pub struct ModelInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub desc: &'static str,
}

/// What each provider fills in.
pub trait Service: Send + Sync {
    /// Its id: the Language Models provider's ("openai").
    fn id(&self) -> &'static str;
    /// "OpenAI".
    fn name(&self) -> &'static str;
    /// The real API's base.
    fn api(&self) -> &'static str;
    /// The debug-only variable naming a test run's stand-in (read only in
    /// debug builds).
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    fn env(&self) -> &'static str;
    fn default_model(&self) -> &'static str;
    fn default_voice(&self) -> &'static str;
    fn models(&self) -> &'static [ModelInfo];
    fn voices(&self) -> &'static [VoiceInfo];
    /// The request for `text` (pure).
    fn request(&self, base: &str, key: &Key, voice: &str, model: &str, rate: f32, text: &str) -> Request;
    /// How its answer reads (from the answer's `Content-Type`).
    fn decoder(&self, content_type: &str) -> Box<dyn Decode>;
    /// An error answer as a [`Failure`] (pure).
    fn classify(&self, status: u16, body: &str, key: &Key) -> Failure;
    /// The longest text one request takes (Groq's Orpheus: 200 characters).
    fn max_chars(&self) -> usize {
        usize::MAX
    }
    /// How long it may take to the first audio, and between pieces.
    fn limits(&self) -> Limits {
        Limits { first_audio: Duration::from_secs(8), gap: Duration::from_secs(6) }
    }
}

/// The providers, in Settings' order.
pub fn services() -> [&'static dyn Service; 3] {
    [&super::openai::OpenAi, &super::gemini::Gemini, &super::groq::Groq]
}

/// The provider `id`, if it's one of these.
pub fn service(id: &str) -> Option<&'static dyn Service> {
    services().into_iter().find(|s| s.id() == id)
}

/// Where `svc`'s requests go: its API — or, in a test run, the debug-only
/// stand-in its variable names, never the internet.
pub fn base(svc: &dyn Service) -> Result<String, String> {
    if !crate::shell::e2e::active() {
        return Ok(svc.api().to_string());
    }
    #[cfg(debug_assertions)]
    if let Some(v) = std::env::var(svc.env()).ok().map(|v| v.trim().trim_end_matches('/').to_string()).filter(|v| !v.is_empty()) {
        return Ok(v);
    }
    Err("Test runs never go on the internet.".into())
}

/// The voice to ask for: `voice` if the provider has it (Gemini's names in
/// any case), else its default. Pure.
pub fn voice_or_default(svc: &dyn Service, voice: &str) -> &'static str {
    let v = voice.trim();
    svc.voices().iter().find(|x| x.id.eq_ignore_ascii_case(v)).map_or(svc.default_voice(), |x| x.id)
}

/// The model to ask for: `model` if it's one Yap knows for speech, else
/// the default. Pure.
pub fn model_or_default(svc: &dyn Service, model: &str) -> &'static str {
    let m = model.trim();
    svc.models().iter().find(|x| x.id == m).map_or(svc.default_model(), |x| x.id)
}

/// A sentence cut into pieces of at most `max` characters, at the last
/// comma, semicolon, colon or space that fits (a word longer than that
/// goes as it is, cut). Pure.
pub fn split(text: &str, max: usize) -> Vec<String> {
    let text = text.trim();
    if max == 0 || text.chars().count() <= max {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    let mut rest: Vec<char> = text.chars().collect();
    while rest.len() > max {
        let window = &rest[..=max.min(rest.len() - 1)];
        // After a comma, semicolon or colon, else at a space.
        let soft = window[..max].iter().rposition(|c| matches!(c, ',' | ';' | ':' | '—')).filter(|&i| i >= max / 3).map(|i| i + 1);
        let at = soft.or_else(|| window.iter().rposition(|c| c.is_whitespace()).filter(|&i| i > 0)).unwrap_or(max);
        let piece: String = rest[..at].iter().collect();
        let piece = piece.trim().to_string();
        if !piece.is_empty() {
            out.push(piece);
        }
        rest.drain(..at);
        while rest.first().is_some_and(|c| c.is_whitespace()) {
            rest.remove(0);
        }
    }
    let last: String = rest.iter().collect();
    if !last.trim().is_empty() {
        out.push(last.trim().to_string());
    }
    out
}

// ---- the engine ----------------------------------------------------------------------------

/// A provider as a speech engine (`tts::Engine`): its key and model from
/// the config; the voice comes with each [`Engine::open`].
pub struct Hosted {
    svc: &'static dyn Service,
    key: Key,
    model: &'static str,
    /// Replies keep [`cloud::Health`] (a failure hands over to the local
    /// voice); Settings' sample shows its error instead.
    keep_health: bool,
    /// Another base than [`base`] (the unit tests' stand-in).
    at: Option<String>,
}

impl Hosted {
    pub fn new(svc: &'static dyn Service, key: &str, model: &str, keep_health: bool) -> Hosted {
        Hosted { svc, key: Key::new(key), model: model_or_default(svc, model), keep_health, at: None }
    }

    /// The same, asking `base` instead (a unit test's stand-in).
    #[cfg(test)]
    pub fn at(mut self, base: &str) -> Hosted {
        self.at = Some(base.to_string());
        self
    }

    /// Fingerprint of the key + voice + model ([`cloud::Health`]).
    pub fn choice_fingerprint(svc: &dyn Service, key: &str, voice: &str, model: &str) -> u64 {
        cloud::fingerprint(&[svc.id(), key.trim(), voice_or_default(svc, voice), model_or_default(svc, model)])
    }
}

impl Engine for Hosted {
    fn id(&self) -> &'static str {
        self.svc.id()
    }
    /// Listed by [`voices_of`] (Settings), not here.
    fn voices(&self) -> Result<Vec<Voice>, String> {
        Ok(Vec::new())
    }
    fn default_voice(&self) -> Option<String> {
        Some(self.svc.default_voice().into())
    }
    fn open(&self, voice: &str, rate: f32) -> Result<Box<dyn Synth>, String> {
        let voice = voice_or_default(self.svc, voice);
        Ok(Box::new(HostedSynth {
            svc: self.svc,
            choice: Hosted::choice_fingerprint(self.svc, self.key.header(), voice, self.model),
            key: self.key.clone(),
            voice,
            model: self.model,
            rate,
            keep_health: self.keep_health,
            at: self.at.clone(),
        }))
    }
    /// Too-short sentences join the next, as for Kokoro and ElevenLabs: one
    /// request fewer, and no gap after a lone "Sure!".
    fn prepare(&self, sentences: Vec<String>) -> Vec<String> {
        super::kokoro::merge_short(sentences)
    }
    fn cancel(&self) {
        stream::cancel();
    }
}

struct HostedSynth {
    svc: &'static dyn Service,
    key: Key,
    voice: &'static str,
    model: &'static str,
    rate: f32,
    choice: u64,
    keep_health: bool,
    at: Option<String>,
}

impl HostedSynth {
    fn record(&self, result: &Result<Streamed, Failure>) {
        let id = self.svc.id();
        match result {
            Ok(s) if !s.stopped || s.samples > 0 => cloud::with_health(|h| h.worked(id, self.key.fingerprint())),
            Ok(_) => {}
            Err(f) if self.keep_health => cloud::with_health(|h| h.failed(id, self.key.fingerprint(), self.choice, f.clone(), Instant::now())),
            Err(_) => {}
        }
    }
}

/// What a sentence is said with: the voice, the model and the speed.
#[derive(Debug, Clone, Copy)]
pub struct Choice<'a> {
    pub voice: &'a str,
    pub model: &'a str,
    pub rate: f32,
}

/// Say `text` through `svc`: each piece of it (Groq's 200 characters) its
/// own request, its audio handed to `out` as it comes, the sentence's pause
/// after the last; each request's tokens or characters counted.
pub async fn speak(svc: &dyn Service, base: &str, key: &Key, choice: Choice<'_>, text: &str, out: &mut dyn FnMut(Pcm) -> bool) -> Result<Streamed, Failure> {
    let pieces = split(text, svc.max_chars());
    let mut total = Streamed::default();
    for (i, piece) in pieces.iter().enumerate() {
        let last = i + 1 == pieces.len();
        let req = svc.request(base, key, choice.voice, choice.model, choice.rate, piece);
        let turn = stream::Turn::now(svc.limits(), if last { SENTENCE_GAP_MS } else { 0 }, svc.name());
        let mut ask = stream::API_HTTP.post(&req.url).json(&req.body);
        ask = match req.auth {
            Auth::Bearer => ask.bearer_auth(req.key.header()),
            Auth::Header(h) => ask.header(h, req.key.header()),
        };
        let send = ask.send_for(crate::privacy::net::Purpose::Voice(svc.name()));
        let Some(resp) = stream::ask(send, turn.since, turn.limits, |s, b| svc.classify(s, b, key)).await? else {
            return Ok(Streamed { stopped: true, ..total });
        };
        let content_type = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
        let mut decode = svc.decoder(&content_type);
        let done = stream::pump(resp, decode.as_mut(), &turn, out).await?;
        crate::ai::usage::record_speech(svc.id(), model_or_default(svc, choice.model), piece.chars().count() as u64, decode.usage());
        total.samples += done.samples;
        total.first_audio = total.first_audio.or(done.first_audio);
        if done.stopped {
            total.stopped = true;
            return Ok(total);
        }
    }
    Ok(total)
}

impl Synth for HostedSynth {
    fn synthesize(&mut self, text: &str) -> Result<Pcm, String> {
        let mut samples = Vec::new();
        let mut format = (24_000, 1);
        self.stream(text, &mut |p| {
            format = (p.sample_rate, p.channels);
            samples.extend(p.samples);
            true
        })?;
        Ok(Pcm { sample_rate: format.0, channels: format.1, samples })
    }

    fn stream(&mut self, text: &str, out: &mut dyn FnMut(Pcm) -> bool) -> Result<(), String> {
        let name = self.svc.name();
        let base = match &self.at {
            Some(at) => at.clone(),
            None => base(self.svc)?,
        };
        if self.key.is_empty() {
            return Err(format!("{name}: no API key"));
        }
        let choice = Choice { voice: self.voice, model: self.model, rate: self.rate };
        let result = tauri::async_runtime::block_on(speak(self.svc, &base, &self.key, choice, text, out));
        self.record(&result);
        match result {
            Ok(_) => Ok(()),
            Err(f) => {
                tracing::warn!("Spoken replies: {name} didn't answer ({})", f.reason());
                Err(format!("{name}: {}", f.reason()))
            }
        }
    }
}

// ---- Settings ------------------------------------------------------------------------------

/// `svc`'s voices for Settings' list (the recommended first, as listed).
pub fn voices_of(svc: &dyn Service) -> Vec<super::elevenlabs::CloudVoice> {
    svc.voices()
        .iter()
        .map(|v| super::elevenlabs::CloudVoice { id: v.id.into(), name: v.name.into(), own: false, desc: v.desc.into(), preview: false })
        .collect()
}

/// `svc`'s models for Settings' picker (the default first, recommended).
pub fn models_of(svc: &dyn Service) -> Vec<super::elevenlabs::CloudModel> {
    let mut out: Vec<super::elevenlabs::CloudModel> = svc
        .models()
        .iter()
        .map(|m| super::elevenlabs::CloudModel { id: m.id.into(), name: m.name.into(), desc: m.desc.into(), recommended: m.id == svc.default_model() })
        .collect();
    out.sort_by_key(|m| !m.recommended);
    out
}

/// An engine for `svc` on `key` (Settings' sample: its error is shown, not
/// kept).
pub fn sample_engine(svc: &'static dyn Service, key: &str, model: &str) -> Arc<dyn Engine> {
    Arc::new(Hosted::new(svc, key, model, false))
}

/// The error body of an OpenAI-shaped API (OpenAI's, Groq's): `{"error":
/// {"message", "type", "code"}}`, as a [`Failure`]. Pure.
pub fn classify_openai_shape(status: u16, body: &str, key: &Key, quota_codes: &[&str]) -> Failure {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let e = &v["error"];
    let code = e["code"].as_str().unwrap_or("");
    let kind = e["type"].as_str().unwrap_or("");
    let message = e["message"].as_str().unwrap_or("");
    if quota_codes.contains(&code) || quota_codes.contains(&kind) {
        return Failure::Quota;
    }
    match code {
        "invalid_api_key" | "invalid_authentication" | "missing_api_key" => return Failure::BadKey,
        "model_not_found" | "model_decommissioned" => return Failure::ModelGone,
        "rate_limit_exceeded" => return Failure::RateLimited,
        _ => {}
    }
    match status {
        401 => Failure::BadKey,
        402 => Failure::Quota,
        403 => Failure::NotAllowed,
        404 => Failure::ModelGone,
        429 | 498 => Failure::RateLimited,
        s if s >= 500 => Failure::ServiceDown,
        _ => Failure::Rejected(stream::tidy(message, key)),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::voice::tts::elevenlabs::standin::{serial, stub, Reply};

    /// One sentence through `svc` against the local stand-in
    /// (`elevenlabs::standin`): what it handed on, and what was asked.
    pub(crate) fn through(svc: &'static dyn Service, replies: Vec<Reply>, voice: &str, model: &str, rate: f32, text: &str) -> (Result<Streamed, Failure>, Vec<Pcm>, Vec<String>) {
        let _one_at_a_time = serial();
        let s = stub(replies);
        let mut got = Vec::new();
        let key = Key::new("sk-standin-0123456789");
        let r = tauri::async_runtime::block_on(speak(svc, &s.base, &key, Choice { voice, model, rate }, text, &mut |p| {
            got.push(p);
            true
        }));
        let asked = s.requests.lock().unwrap().clone();
        (r, got, asked)
    }

    /// **A real request, on your own key** (never in a normal run: it costs
    /// a fraction of a penny). The provider and key from the environment;
    /// one sentence said into a WAV in a temp folder (nothing played), with
    /// its first-audio time and length.
    /// `$env:YAP_REAL_TTS_PROVIDER="openai"; $env:YAP_REAL_TTS_KEY="sk-…";
    /// cargo test --lib voice::tts::hosted::tests::a_real_sentence_on_your_key -- --ignored --nocapture`
    /// (optional `YAP_REAL_TTS_VOICE`, `YAP_REAL_TTS_MODEL`).
    #[test]
    #[ignore]
    fn a_real_sentence_on_your_key() {
        let id = std::env::var("YAP_REAL_TTS_PROVIDER").expect("YAP_REAL_TTS_PROVIDER: openai, gemini or groq");
        let key = Key::new(&std::env::var("YAP_REAL_TTS_KEY").expect("YAP_REAL_TTS_KEY"));
        let svc = service(&id).expect("openai, gemini or groq");
        let voice = std::env::var("YAP_REAL_TTS_VOICE").unwrap_or_default();
        let model = std::env::var("YAP_REAL_TTS_MODEL").unwrap_or_default();
        let text = "Hi, I'm the voice that reads Chat's answers, and this is a real request on your own key.";
        let mut pcm: Vec<f32> = Vec::new();
        let mut format = (0u32, 0u16);
        let started = Instant::now();
        let done = tauri::async_runtime::block_on(speak(svc, svc.api(), &key, Choice { voice: &voice, model: &model, rate: 1.0 }, text, &mut |p| {
            format = (p.sample_rate, p.channels);
            pcm.extend(p.samples);
            true
        }))
        .unwrap_or_else(|f| panic!("{} didn't answer: {}", svc.name(), f.reason()));
        let secs = pcm.len() as f64 / f64::from(format.0.max(1)) / f64::from(format.1.max(1));
        println!(
            "{}: first audio after {:?}, {:.2} s of audio at {} Hz × {} (all of it in {:?})",
            svc.name(),
            done.first_audio,
            secs,
            format.0,
            format.1,
            started.elapsed()
        );
        assert!(secs > 1.0, "too little audio: {secs} s");
        let peak = pcm.iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.02, "silent audio: peak {peak}");
        let data: Vec<u8> = pcm.iter().flat_map(|s| ((s.clamp(-1.0, 1.0) * 32_767.0) as i16).to_le_bytes()).collect();
        let mut w = Vec::new();
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&format.1.to_le_bytes());
        w.extend_from_slice(&format.0.to_le_bytes());
        w.extend_from_slice(&(format.0 * u32::from(format.1) * 2).to_le_bytes());
        w.extend_from_slice(&(format.1 * 2).to_le_bytes());
        w.extend_from_slice(&16u16.to_le_bytes());
        w.extend_from_slice(b"data");
        w.extend_from_slice(&(data.len() as u32).to_le_bytes());
        w.extend_from_slice(&data);
        let dir = std::env::temp_dir().join("yap-tts-check");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{id}-voice.wav"));
        std::fs::write(&path, w).unwrap();
        println!("wrote {}", path.display());
    }

    #[test]
    fn a_long_sentence_is_cut_where_it_breathes() {
        assert_eq!(split("Short one.", 200), vec!["Short one."]);
        let long = "The first part of this sentence runs on for a while, and then the second part keeps going with more words until it has to stop somewhere sensible.";
        let parts = split(long, 60);
        assert!(parts.iter().all(|p| p.chars().count() <= 60), "{parts:?}");
        assert_eq!(parts.join(" "), long);
        // At the comma when there's one in reach.
        assert!(parts[0].ends_with("while,"), "{parts:?}");
        // A word longer than the limit is cut, never lost.
        let word = "a".repeat(250);
        let parts = split(&word, 200);
        assert_eq!(parts.iter().map(|p| p.len()).collect::<Vec<_>>(), vec![200, 50]);
        assert_eq!(split("", 200), vec![String::new()]);
    }

    #[test]
    fn voices_and_models_fall_back_to_the_default() {
        let g = &super::super::gemini::Gemini;
        assert_eq!(voice_or_default(g, "puck"), "Puck");
        assert_eq!(voice_or_default(g, "Nobody"), g.default_voice());
        assert_eq!(voice_or_default(g, ""), g.default_voice());
        assert_eq!(model_or_default(g, "gemini-3.8-flash-tts"), "gemini-3.8-flash-tts");
        assert_eq!(model_or_default(g, "gpt-4o-mini-tts"), g.default_model());
        // Each provider's default voice and model are in its own lists.
        for s in services() {
            assert!(s.voices().iter().any(|v| v.id == s.default_voice()), "{}", s.id());
            assert!(s.models().iter().any(|m| m.id == s.default_model()), "{}", s.id());
            assert!(models_of(s)[0].recommended, "{}", s.id());
            assert!(cloud::is_hosted(s.id()) && cloud::is_cloud(s.id()));
        }
        assert!(service("elevenlabs").is_none());
    }

    #[test]
    fn openai_shaped_errors_are_named() {
        let k = Key::new("sk-test-0123456789");
        let err = |code: &str, kind: &str| format!(r#"{{"error":{{"message":"m","type":"{kind}","code":"{code}"}}}}"#);
        assert_eq!(classify_openai_shape(401, &err("invalid_api_key", "invalid_request_error"), &k, &["insufficient_quota"]), Failure::BadKey);
        assert_eq!(classify_openai_shape(429, &err("insufficient_quota", "insufficient_quota"), &k, &["insufficient_quota"]), Failure::Quota);
        assert_eq!(classify_openai_shape(429, &err("rate_limit_exceeded", "requests"), &k, &["insufficient_quota"]), Failure::RateLimited);
        assert_eq!(classify_openai_shape(404, &err("model_not_found", "invalid_request_error"), &k, &[]), Failure::ModelGone);
        assert_eq!(classify_openai_shape(503, "<html>", &k, &[]), Failure::ServiceDown);
        assert_eq!(classify_openai_shape(498, "", &k, &[]), Failure::RateLimited);
        // Their own words, the key never in them.
        let echoed = r#"{"error":{"message":"Key sk-test-0123456789 may not use this model; accept its terms first.","type":"invalid_request_error","code":"model_terms_required"}}"#;
        match classify_openai_shape(400, echoed, &k, &[]) {
            Failure::Rejected(m) => assert!(m.contains("[key]") && !m.contains("sk-test") && m.contains("terms"), "{m}"),
            other => panic!("{other:?}"),
        }
    }
}

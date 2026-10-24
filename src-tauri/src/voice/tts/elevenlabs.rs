//! **ElevenLabs** — a cloud voice for spoken replies, on the person's own API
//! key (Settings → Voice Assistant → Voice: "ElevenLabs"). Nothing goes to
//! ElevenLabs unless it's picked and a key is set; then the words Yap speaks
//! go there, sentence by sentence, and nothing else.
//!
//! The API as of 22 October 2026 (elevenlabs.io/docs):
//! - **Speech**: `POST /v1/text-to-speech/{voice_id}/stream?output_format=
//!   pcm_24000`, `{text, model_id, voice_settings?: {speed}}` — raw 16-bit
//!   little-endian mono PCM at 24 kHz (Kokoro's rate; every plan has it),
//!   streamed: each piece is played as it comes ([`stream_speech`]), so the
//!   first words start before the sentence is all made. `speed` is
//!   ElevenLabs' own 0.7–1.2, sent only when Yap's speed isn't 1×.
//! - **Model**: `eleven_flash_v2_5` by default — the lowest-latency one
//!   (~75 ms model time, 32 languages); the picker lists `GET /v1/models`
//!   (`can_do_text_to_speech`), else [`KNOWN_MODELS`].
//! - **Voices**: `GET /v2/voices` (the account's own voices plus the default
//!   premade ones on page 0; ≤ 3 pages of 100), each with a `preview_url` —
//!   a public sample, fetched only when ▶ is clicked, without the key.
//! - **Usage**: `GET /v1/user/subscription` → `character_count` /
//!   `character_limit` / `next_character_count_reset_unix`; a key without
//!   that permission just shows no usage line.
//!
//! **The key**: kept like Yap's other cloud keys (`config.tts_cloud_keys`,
//! like `web_search_keys`), sent only in the `xi-api-key` header to
//! ElevenLabs' API — never in a URL, a log line or an error message
//! ([`Key`]'s `Debug` is redacted, [`tidy`] strips it from anything the
//! service says back) — and never on a redirect (keyed requests don't
//! follow any). **Failures** become a [`Failure`] (`tts/cloud.rs`), which
//! decides the local voice's turn.
//!
//! **Test mode** never reaches the internet: the debug-only
//! `YAP_ELEVENLABS_URL` names a stand-in (the e2e suite's
//! `support/fake-elevenlabs.js`), else every call fails with "Test runs
//! never go on the internet."

use std::collections::HashMap;
use crate::privacy::net::Recorded as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};

use super::cloud::{self, Failure};
use super::{Engine, Pcm, Synth, Voice};

pub const ID: &str = "elevenlabs";
pub const NAME: &str = "ElevenLabs";
/// The real API.
pub const API: &str = "https://api.elevenlabs.io";
/// Where a person makes a key ("Get your API key").
pub const KEYS_PAGE: &str = "https://elevenlabs.io/app/settings/api-keys";
/// The lowest-latency model (the docs, 22 Oct 2026: ~75 ms).
pub const DEFAULT_MODEL: &str = "eleven_flash_v2_5";
/// "George", a default voice every account has (the docs' own examples).
pub const DEFAULT_VOICE: &str = "JBFqnCBsd6RMkjVDRZzb";
pub const SAMPLE_RATE: u32 = 24_000;
const OUTPUT_FORMAT: &str = "pcm_24000";

/// Models Yap knows, for the picker when the list can't be read: `(id,
/// name, what it's for)`.
pub const KNOWN_MODELS: &[(&str, &str, &str)] = &[
    ("eleven_flash_v2_5", "Flash v2.5", "Fastest to start (about 75 ms), 32 languages"),
    ("eleven_flash_v2", "Flash v2", "As fast, English only"),
    ("eleven_multilingual_v2", "Multilingual v2", "Richer, slower to start, 29 languages"),
    ("eleven_v3", "Eleven v3", "Most expressive, slowest to start, 70+ languages"),
    ("eleven_v4", "Eleven v4", "Highest quality, slower to start, 90+ languages"),
];

/// Connecting (DNS, TCP, TLS) may take this long.
const CONNECT: Duration = Duration::from_secs(4);
/// From asking to the first audio (or the error).
const FIRST_AUDIO: Duration = Duration::from_secs(6);
/// The longest pause between two pieces of audio.
const GAP: Duration = Duration::from_secs(6);
/// The voice list, models and usage.
const LIST_TIMEOUT: Duration = Duration::from_secs(10);
/// Audio is handed on in pieces of at least 100 ms (the end excepted).
const MIN_PIECE: usize = SAMPLE_RATE as usize / 10;
/// The pause after each sentence (Kokoro's is 180 ms).
const SENTENCE_GAP_MS: usize = 150;
/// A preview sample larger than this isn't read.
const MAX_PREVIEW_BYTES: usize = 4 * 1024 * 1024;

// ---- where requests go -------------------------------------------------------------------

/// The API's base: ElevenLabs — or, in a test run, the debug-only stand-in
/// (`YAP_ELEVENLABS_URL`), never the internet.
pub fn base() -> Result<String, String> {
    if !crate::shell::e2e::active() {
        return Ok(API.to_string());
    }
    #[cfg(debug_assertions)]
    if let Some(v) = std::env::var("YAP_ELEVENLABS_URL")
        .ok()
        .map(|v| v.trim().trim_end_matches('/').to_string())
        .filter(|v| !v.is_empty())
    {
        return Ok(v);
    }
    Err("Test runs never go on the internet.".into())
}

/// Requests with the key: no redirects (a custom header would follow one
/// to another host).
static API_HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .connect_timeout(CONNECT)
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(format!("Yap/{} (Windows)", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
});

/// A voice's public sample: no key, a few redirects allowed.
static SAMPLE_HTTP: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .connect_timeout(CONNECT)
        .timeout(LIST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(3))
        .user_agent(format!("Yap/{} (Windows)", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_default()
});

// ---- the key -------------------------------------------------------------------------------

/// An API key: shows as `[redacted]` in any `Debug` output.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Key(String);

impl Key {
    pub fn new(key: &str) -> Key {
        Key(key.trim().to_string())
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// For "the same key" checks ([`cloud::Health`]), never the key itself.
    pub fn fingerprint(&self) -> u64 {
        cloud::fingerprint(&[ID, &self.0])
    }
    fn header(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_empty() { "[none]" } else { "[redacted]" })
    }
}

/// What the service said, fit to show: the key cut out (should it ever echo
/// it), one line, ≤ 120 characters.
pub fn tidy(said: &str, key: &Key) -> String {
    let mut s = said.to_string();
    if !key.is_empty() {
        s = s.replace(key.header(), "[key]");
    }
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() > 120 {
        format!("{}…", s.chars().take(119).collect::<String>().trim_end())
    } else {
        s
    }
}

// ---- a sentence's request ------------------------------------------------------------------

/// One sentence to say: where, with what key (a header, never the URL) and
/// what body.
#[derive(Debug, Clone)]
pub struct SpeechRequest {
    pub url: String,
    pub key: Key,
    pub body: Value,
}

/// A voice id as ElevenLabs makes them (letters and digits): anything else
/// would change the request's path.
pub fn valid_voice_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Yap's speed (0.5–2.0) as ElevenLabs' `speed` (0.7–1.2), or nothing at
/// 1× (the voice's own settings stand). Pure.
pub fn speed_for(rate: f32) -> Option<f64> {
    if !rate.is_finite() || (rate - 1.0).abs() < 0.01 {
        return None;
    }
    Some((f64::from(rate.clamp(0.7, 1.2)) * 100.0).round() / 100.0)
}

/// The request for `text` in `voice` with `model`. Pure.
pub fn speech_request(base: &str, key: &Key, voice: &str, model: &str, rate: f32, text: &str) -> Result<SpeechRequest, String> {
    if key.is_empty() {
        return Err(format!("No {NAME} API key"));
    }
    let voice = if voice.trim().is_empty() { DEFAULT_VOICE } else { voice.trim() };
    if !valid_voice_id(voice) {
        return Err(format!("Not a {NAME} voice id"));
    }
    let model = if model.trim().is_empty() { DEFAULT_MODEL } else { model.trim() };
    let mut body = json!({ "text": text, "model_id": model });
    if let Some(speed) = speed_for(rate) {
        body["voice_settings"] = json!({ "speed": speed });
    }
    Ok(SpeechRequest {
        url: format!("{base}/v1/text-to-speech/{voice}/stream?output_format={OUTPUT_FORMAT}"),
        key: key.clone(),
        body,
    })
}

/// The same sentence from the endpoint that also says when each character
/// is spoken (`/stream/with-timestamps`, reading aloud's karaoke): a stream
/// of JSON lines, the audio base64 in each. Pure.
pub fn timed_request(base: &str, key: &Key, voice: &str, model: &str, rate: f32, text: &str) -> Result<SpeechRequest, String> {
    let mut req = speech_request(base, key, voice, model, rate, text)?;
    req.url = req.url.replacen("/stream?", "/stream/with-timestamps?", 1);
    Ok(req)
}

// ---- the audio -----------------------------------------------------------------------------

/// Raw 16-bit little-endian PCM as it arrives: a byte left over at the end
/// of one piece is the first half of the next sample.
#[derive(Debug, Default)]
pub struct S16Le {
    carry: Option<u8>,
}

impl S16Le {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<f32> {
        let mut out = Vec::with_capacity(bytes.len() / 2 + 1);
        let mut rest = bytes;
        if let Some(lo) = self.carry.take() {
            match rest.split_first() {
                Some((&hi, tail)) => {
                    out.push(f32::from(i16::from_le_bytes([lo, hi])) / 32_768.0);
                    rest = tail;
                }
                None => {
                    self.carry = Some(lo);
                    return out;
                }
            }
        }
        let (pairs, tail) = rest.as_chunks::<2>();
        out.extend(pairs.iter().map(|&p| f32::from(i16::from_le_bytes(p)) / 32_768.0));
        self.carry = tail.first().copied();
        out
    }
}

// ---- failures ------------------------------------------------------------------------------

/// An error answer as a [`Failure`]: ElevenLabs' `{"detail": {"code" (or
/// the older "status"), "message"}}`, a 422's `{"detail": [{msg}]}`, or
/// nothing readable. Pure.
pub fn classify(status: u16, body: &str, key: &Key) -> Failure {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let detail = &v["detail"];
    let code = detail["code"].as_str().or_else(|| detail["status"].as_str()).unwrap_or("");
    let message = detail["message"]
        .as_str()
        .or_else(|| detail.as_str())
        .or_else(|| detail[0]["msg"].as_str())
        .unwrap_or("");
    match code {
        "quota_exceeded" | "insufficient_credits" | "payment_required" => return Failure::Quota,
        "voice_not_found" => return Failure::VoiceGone,
        "model_not_found" | "unsupported_model" => return Failure::ModelGone,
        "missing_permissions" | "insufficient_permissions" | "voice_access_denied" | "model_access_denied"
        | "feature_not_available" | "subscription_required" | "workspace_access_denied" | "forbidden" => {
            return Failure::NotAllowed
        }
        "invalid_api_key" | "missing_api_key" | "invalid_authorization_header" | "unauthorized" | "sign_in_required" => {
            return Failure::BadKey
        }
        "rate_limit_exceeded" | "concurrent_limit_exceeded" | "too_many_concurrent_requests" | "system_busy" => {
            return Failure::RateLimited
        }
        _ => {}
    }
    match status {
        402 => Failure::Quota,
        401 if code.is_empty() || message.is_empty() => Failure::BadKey,
        // Another 401 ("detected_unusual_activity"…): in their words.
        401 => Failure::Rejected(tidy(message, key)),
        403 => Failure::NotAllowed,
        404 => Failure::VoiceGone,
        429 => Failure::RateLimited,
        s if s >= 500 => Failure::ServiceDown,
        _ => Failure::Rejected(tidy(message, key)),
    }
}

/// A request that never got an answer.
fn send_failure(e: &reqwest::Error) -> Failure {
    if e.is_timeout() {
        Failure::TimedOut
    } else {
        Failure::Offline
    }
}

/// An error answer's body (bounded in time and size), classified.
async fn error_of(resp: reqwest::Response, key: &Key) -> Failure {
    let status = resp.status().as_u16();
    let body = tokio::time::timeout(Duration::from_secs(3), resp.bytes())
        .await
        .ok()
        .and_then(Result::ok)
        .map(|b| String::from_utf8_lossy(&b[..b.len().min(8 * 1024)]).into_owned())
        .unwrap_or_default();
    classify(status, &body, key)
}

// ---- stopping a sentence being made ----------------------------------------------------------

static CANCELS: AtomicU64 = AtomicU64::new(0);
static CANCEL: LazyLock<tokio::sync::Notify> = LazyLock::new(tokio::sync::Notify::new);

/// End the sentence being fetched now (a stop, a new answer, a dictation).
pub fn cancel() {
    CANCELS.fetch_add(1, Ordering::SeqCst);
    CANCEL.notify_waiters();
}

/// Resolves when [`cancel`] is called after `since` (the count then).
async fn cancelled(since: u64) {
    loop {
        // Made before the check, so a cancel in between still wakes it.
        let woken = CANCEL.notified();
        if CANCELS.load(Ordering::SeqCst) != since {
            return;
        }
        woken.await;
    }
}

// ---- streaming one sentence ----------------------------------------------------------------

/// How a sentence went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Streamed {
    /// Samples handed on (the pause after included).
    pub samples: usize,
    /// From asking to the first audio handed on.
    pub first_audio: Option<Duration>,
    /// Stopped part-way (a stop, or `out` said so): not a failure.
    pub stopped: bool,
}

/// Say one sentence: ask, then hand each ≥ 100 ms of audio to `out` as it
/// arrives (`out` → false: stop), then [`SENTENCE_GAP_MS`] of quiet.
/// No audio within [`FIRST_AUDIO`], or a gap over [`GAP`]: timed out.
pub async fn stream_speech(req: &SpeechRequest, out: &mut dyn FnMut(Pcm) -> bool) -> Result<Streamed, Failure> {
    let since = CANCELS.load(Ordering::SeqCst);
    let started = Instant::now();
    let stop = cancelled(since);
    tokio::pin!(stop);
    let mut done = Streamed::default();
    let send = API_HTTP
        .post(&req.url)
        .header("xi-api-key", req.key.header())
        .header("accept", "audio/pcm, application/json")
        .json(&req.body)
        .send_for(crate::privacy::net::Purpose::Voice(NAME));
    let resp = tokio::select! {
        _ = &mut stop => return Ok(Streamed { stopped: true, ..done }),
        r = tokio::time::timeout(FIRST_AUDIO, send) => match r {
            Err(_) => return Err(Failure::TimedOut),
            Ok(Err(e)) => return Err(send_failure(&e)),
            Ok(Ok(resp)) => resp,
        },
    };
    if !resp.status().is_success() {
        return Err(error_of(resp, &req.key).await);
    }
    let mut resp = resp;
    let mut pcm = S16Le::default();
    let mut piece: Vec<f32> = Vec::with_capacity(MIN_PIECE * 2);
    let mut heard = false;
    let mut hand_on = |samples: Vec<f32>, done: &mut Streamed| -> bool {
        done.samples += samples.len();
        if done.first_audio.is_none() {
            done.first_audio = Some(started.elapsed());
        }
        out(Pcm { sample_rate: SAMPLE_RATE, channels: 1, samples })
    };
    loop {
        let wait = if heard { GAP } else { FIRST_AUDIO.saturating_sub(started.elapsed()) };
        let next = tokio::select! {
            _ = &mut stop => return Ok(Streamed { stopped: true, ..done }),
            r = tokio::time::timeout(wait, resp.chunk()) => r,
        };
        match next {
            Err(_) => return Err(Failure::TimedOut),
            // The connection dropped mid-answer.
            Ok(Err(e)) => return Err(send_failure(&e)),
            Ok(Ok(None)) => break,
            Ok(Ok(Some(bytes))) => {
                piece.extend(pcm.push(&bytes));
                heard |= !piece.is_empty();
                if piece.len() >= MIN_PIECE && !hand_on(std::mem::take(&mut piece), &mut done) {
                    return Ok(Streamed { stopped: true, ..done });
                }
            }
        }
    }
    if done.samples == 0 && piece.is_empty() {
        return Err(Failure::Rejected(format!("{NAME} sent no audio")));
    }
    piece.extend(std::iter::repeat_n(0.0, SAMPLE_RATE as usize * SENTENCE_GAP_MS / 1000));
    if !hand_on(piece, &mut done) {
        done.stopped = true;
    }
    Ok(done)
}

// ---- one sentence with its timing (`/stream/with-timestamps`) -----------------------------------

/// `/stream/with-timestamps`' answer as it arrives: one JSON object a line,
/// each with a piece of the audio (`audio_base64`, the same raw PCM) and
/// when the characters in it start (`alignment`: `characters`,
/// `character_start_times_seconds` — on the whole sentence's clock, as
/// ElevenLabs' own example adds them up; a piece whose times start again
/// near zero is put after the audio before it). A line may arrive in parts.
#[derive(Debug, Default)]
pub struct Timestamped {
    buf: Vec<u8>,
    pcm: S16Le,
    /// Characters timed so far (the next piece's first is this one).
    chars: usize,
    /// Seconds of audio so far.
    audio: f64,
    /// When the last timed character ends.
    last_end: f64,
    pub times: Vec<super::marks::CharTime>,
}

impl Timestamped {
    /// More of the answer: the audio in every line it completes.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<f32>, Failure> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        while let Some(nl) = self.buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=nl).collect();
            out.extend(self.line(&line)?);
        }
        Ok(out)
    }

    /// The answer ended: a last line without its line break.
    pub fn finish(&mut self) -> Result<Vec<f32>, Failure> {
        let rest = std::mem::take(&mut self.buf);
        self.line(&rest)
    }

    fn line(&mut self, line: &[u8]) -> Result<Vec<f32>, Failure> {
        use base64::Engine as _;
        let line = line.trim_ascii();
        if line.is_empty() {
            return Ok(Vec::new());
        }
        let odd = || Failure::Rejected(format!("{NAME} answered with something unexpected"));
        let v: Value = serde_json::from_slice(line).map_err(|_| odd())?;
        let before = self.audio;
        let samples = match v["audio_base64"].as_str() {
            Some(b64) if !b64.is_empty() => {
                let bytes = base64::engine::general_purpose::STANDARD.decode(b64).map_err(|_| odd())?;
                self.pcm.push(&bytes)
            }
            _ => Vec::new(),
        };
        self.audio += samples.len() as f64 / f64::from(SAMPLE_RATE);
        let a = &v["alignment"];
        if let (Some(chars), Some(starts)) = (a["characters"].as_array(), a["character_start_times_seconds"].as_array()) {
            let starts: Vec<f64> = starts.iter().map(|s| s.as_f64().unwrap_or(0.0)).collect();
            let ends: Vec<f64> = a["character_end_times_seconds"]
                .as_array()
                .map(|e| e.iter().map(|s| s.as_f64().unwrap_or(0.0)).collect())
                .unwrap_or_default();
            // Times starting again near zero after the first piece: this
            // piece's own clock.
            let offset = match starts.first() {
                Some(&first) if !self.times.is_empty() && first + 0.05 < self.last_end => before,
                _ => 0.0,
            };
            for (k, &s) in starts.iter().enumerate().take(chars.len()) {
                self.times.push(super::marks::CharTime {
                    at: self.chars + k,
                    start: Duration::from_secs_f64((s + offset).max(0.0)),
                });
            }
            if let Some(&end) = ends.last().or(starts.last()) {
                self.last_end = self.last_end.max(end + offset);
            }
            self.chars += chars.len();
        }
        Ok(samples)
    }
}

/// [`stream_speech`] from `/stream/with-timestamps` ([`timed_request`]):
/// the audio handed on as it comes, and when each character starts.
pub async fn stream_speech_timed(
    req: &SpeechRequest,
    out: &mut dyn FnMut(Pcm) -> bool,
) -> Result<(Streamed, Vec<super::marks::CharTime>), Failure> {
    let since = CANCELS.load(Ordering::SeqCst);
    let started = Instant::now();
    let stop = cancelled(since);
    tokio::pin!(stop);
    let mut done = Streamed::default();
    let send = API_HTTP
        .post(&req.url)
        .header("xi-api-key", req.key.header())
        .header("accept", "application/json")
        .json(&req.body)
        .send_for(crate::privacy::net::Purpose::Voice(NAME));
    let resp = tokio::select! {
        _ = &mut stop => return Ok((Streamed { stopped: true, ..done }, Vec::new())),
        r = tokio::time::timeout(FIRST_AUDIO, send) => match r {
            Err(_) => return Err(Failure::TimedOut),
            Ok(Err(e)) => return Err(send_failure(&e)),
            Ok(Ok(resp)) => resp,
        },
    };
    if !resp.status().is_success() {
        return Err(error_of(resp, &req.key).await);
    }
    let mut resp = resp;
    let mut parse = Timestamped::default();
    let mut piece: Vec<f32> = Vec::with_capacity(MIN_PIECE * 2);
    let mut heard = false;
    let mut hand_on = |samples: Vec<f32>, done: &mut Streamed| -> bool {
        done.samples += samples.len();
        if done.first_audio.is_none() {
            done.first_audio = Some(started.elapsed());
        }
        out(Pcm { sample_rate: SAMPLE_RATE, channels: 1, samples })
    };
    loop {
        let wait = if heard { GAP } else { FIRST_AUDIO.saturating_sub(started.elapsed()) };
        let next = tokio::select! {
            _ = &mut stop => return Ok((Streamed { stopped: true, ..done }, parse.times)),
            r = tokio::time::timeout(wait, resp.chunk()) => r,
        };
        match next {
            Err(_) => return Err(Failure::TimedOut),
            Ok(Err(e)) => return Err(send_failure(&e)),
            Ok(Ok(None)) => break,
            Ok(Ok(Some(bytes))) => {
                piece.extend(parse.push(&bytes)?);
                heard |= !piece.is_empty();
                if piece.len() >= MIN_PIECE && !hand_on(std::mem::take(&mut piece), &mut done) {
                    return Ok((Streamed { stopped: true, ..done }, parse.times));
                }
            }
        }
    }
    piece.extend(parse.finish()?);
    if done.samples == 0 && piece.is_empty() {
        return Err(Failure::Rejected(format!("{NAME} sent no audio")));
    }
    piece.extend(std::iter::repeat_n(0.0, SAMPLE_RATE as usize * SENTENCE_GAP_MS / 1000));
    if !hand_on(piece, &mut done) {
        done.stopped = true;
    }
    Ok((done, parse.times))
}

// ---- the engine ----------------------------------------------------------------------------

/// ElevenLabs as a speech engine (`tts::Engine`): `key` and `model` from
/// Settings; the voice comes with each [`Engine::open`].
pub struct ElevenLabs {
    key: Key,
    model: String,
    /// Replies keep [`cloud::Health`] (a failure hands over to the local
    /// voice); Settings' preview shows its error instead.
    keep_health: bool,
    /// Another API base than [`base`] (the unit tests' stand-in).
    at: Option<String>,
    /// Ask for each character's time too (reading aloud: [`timed_request`]).
    timed: bool,
}

impl ElevenLabs {
    pub fn new(key: &str, model: &str, keep_health: bool) -> ElevenLabs {
        Self::with_key(Key::new(key), model, keep_health)
    }

    pub fn with_key(key: Key, model: &str, keep_health: bool) -> ElevenLabs {
        let model = if model.trim().is_empty() { DEFAULT_MODEL } else { model.trim() };
        ElevenLabs { key, model: model.to_string(), keep_health, at: None, timed: false }
    }

    /// Ask for the word timing too (`/stream/with-timestamps`), when `on`.
    pub fn timed(mut self, on: bool) -> ElevenLabs {
        self.timed = on;
        self
    }

    /// The same, asking `base` instead (a unit test's stand-in).
    #[cfg(test)]
    pub fn at(mut self, base: &str) -> ElevenLabs {
        self.at = Some(base.to_string());
        self
    }

    /// Fingerprint of the key + voice + model ([`cloud::Health`]).
    pub fn choice_fingerprint(key: &str, voice: &str, model: &str) -> u64 {
        let voice = if voice.trim().is_empty() { DEFAULT_VOICE } else { voice.trim() };
        let model = if model.trim().is_empty() { DEFAULT_MODEL } else { model.trim() };
        cloud::fingerprint(&[ID, key.trim(), voice, model])
    }
}

impl Engine for ElevenLabs {
    fn id(&self) -> &'static str {
        ID
    }
    /// Listed by [`list_voices`] (Settings), not here.
    fn voices(&self) -> Result<Vec<Voice>, String> {
        Ok(Vec::new())
    }
    fn default_voice(&self) -> Option<String> {
        Some(DEFAULT_VOICE.into())
    }
    fn open(&self, voice: &str, rate: f32) -> Result<Box<dyn Synth>, String> {
        let voice = if voice.trim().is_empty() { DEFAULT_VOICE.to_string() } else { voice.trim().to_string() };
        if !valid_voice_id(&voice) {
            return Err(format!("Not a {NAME} voice id"));
        }
        Ok(Box::new(ElevenSynth {
            choice: Self::choice_fingerprint(self.key.header(), &voice, &self.model),
            key: self.key.clone(),
            voice,
            model: self.model.clone(),
            rate,
            keep_health: self.keep_health,
            at: self.at.clone(),
            timed: self.timed,
            times: None,
        }))
    }
    /// Too-short sentences join the next, as for Kokoro: one request fewer,
    /// and no gap after a lone "Sure!".
    fn prepare(&self, sentences: Vec<String>) -> Vec<String> {
        super::kokoro::merge_short(sentences)
    }
    fn cancel(&self) {
        cancel();
    }
}

struct ElevenSynth {
    key: Key,
    voice: String,
    model: String,
    rate: f32,
    choice: u64,
    keep_health: bool,
    at: Option<String>,
    timed: bool,
    /// The last sentence's character times (`marks`).
    times: Option<Vec<super::marks::CharTime>>,
}

impl ElevenSynth {
    fn record(&self, result: &Result<Streamed, Failure>) {
        match result {
            Ok(s) if !s.stopped || s.samples > 0 => cloud::with_health(|h| h.worked(ID, self.key.fingerprint())),
            Ok(_) => {}
            Err(f) if self.keep_health => {
                cloud::with_health(|h| h.failed(ID, self.key.fingerprint(), self.choice, f.clone(), Instant::now()))
            }
            Err(_) => {}
        }
    }
}

impl Synth for ElevenSynth {
    fn synthesize(&mut self, text: &str) -> Result<Pcm, String> {
        let mut samples = Vec::new();
        self.stream(text, &mut |p| {
            samples.extend(p.samples);
            true
        })?;
        Ok(Pcm { sample_rate: SAMPLE_RATE, channels: 1, samples })
    }

    fn stream(&mut self, text: &str, out: &mut dyn FnMut(Pcm) -> bool) -> Result<(), String> {
        let base = match &self.at {
            Some(at) => at.clone(),
            None => base()?,
        };
        let result = if self.timed {
            let req = timed_request(&base, &self.key, &self.voice, &self.model, self.rate, text)?;
            tauri::async_runtime::block_on(stream_speech_timed(&req, out)).map(|(done, times)| {
                self.times = (!times.is_empty()).then_some(times);
                done
            })
        } else {
            let req = speech_request(&base, &self.key, &self.voice, &self.model, self.rate, text)?;
            tauri::async_runtime::block_on(stream_speech(&req, out))
        };
        self.record(&result);
        match result {
            Ok(_) => Ok(()),
            Err(f) => {
                tracing::warn!("Spoken replies: {NAME} didn't answer ({})", f.reason());
                Err(format!("{NAME}: {}", f.reason()))
            }
        }
    }

    fn marks(&mut self) -> Option<Vec<super::marks::CharTime>> {
        self.times.take()
    }
}

// ---- Settings: voices, models, usage ---------------------------------------------------------

/// One voice in Settings' list.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudVoice {
    pub id: String,
    pub name: String,
    /// One of the account's own (cloned, designed, from the library) rather
    /// than a default premade voice.
    pub own: bool,
    /// "British · male · middle aged".
    pub desc: String,
    /// It has a sample to play (▶).
    pub preview: bool,
}

/// One model in Settings' picker.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudModel {
    pub id: String,
    pub name: String,
    pub desc: String,
    pub recommended: bool,
}

/// The month's characters.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub used: u64,
    pub limit: u64,
    /// Unix seconds.
    pub resets_at: Option<i64>,
}

/// The samples of the voices listed last (voice id → URL): ▶ plays one of
/// these, never a URL a page hands over.
static PREVIEWS: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

async fn get_json(path_and_query: &str, key: &Key) -> Result<Value, Failure> {
    if key.is_empty() {
        return Err(Failure::BadKey);
    }
    let base = base().map_err(Failure::Rejected)?;
    let resp = API_HTTP
        .get(format!("{base}{path_and_query}"))
        .header("xi-api-key", key.header())
        .timeout(LIST_TIMEOUT)
        .send_for(crate::privacy::net::Purpose::VoiceList(NAME))
        .await
        .map_err(|e| send_failure(&e))?;
    if !resp.status().is_success() {
        return Err(error_of(resp, key).await);
    }
    let v = resp.json::<Value>().await.map_err(|_| Failure::Rejected(format!("{NAME} answered with something unexpected")))?;
    cloud::with_health(|h| h.worked(ID, key.fingerprint()));
    Ok(v)
}

/// A voice from the API's list. Pure.
pub fn voice_of(v: &Value) -> Option<(CloudVoice, Option<String>)> {
    let id = v["voice_id"].as_str()?.to_string();
    if !valid_voice_id(&id) {
        return None;
    }
    let name = v["name"].as_str().unwrap_or(&id).trim().to_string();
    let labels = &v["labels"];
    let label = |k: &str| labels[k].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let desc: Vec<String> = [label("accent").map(|a| normalize_label(&a)), label("gender"), label("age")]
        .into_iter()
        .flatten()
        .map(|s| s.replace('_', " "))
        .collect();
    let preview = v["preview_url"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    Some((
        CloudVoice {
            own: v["category"].as_str() != Some("premade"),
            desc: desc.join(" · "),
            preview: preview.is_some(),
            id,
            name,
        },
        preview,
    ))
}

/// "british" → "British", "en-us" stays.
fn normalize_label(s: &str) -> String {
    super::normalize_name(s)
}

/// The account's voices and the default ones: its own first, then by name.
pub async fn list_voices(key: &Key) -> Result<Vec<CloudVoice>, Failure> {
    let mut voices = Vec::new();
    let mut previews = HashMap::new();
    let mut token: Option<String> = None;
    for _ in 0..3 {
        let mut q = "/v2/voices?page_size=100&include_total_count=false".to_string();
        if let Some(t) = &token {
            let t: String = url::form_urlencoded::byte_serialize(t.as_bytes()).collect();
            q.push_str(&format!("&next_page_token={t}"));
        }
        let page = get_json(&q, key).await?;
        for v in page["voices"].as_array().into_iter().flatten() {
            if let Some((voice, preview)) = voice_of(v) {
                if let Some(p) = preview {
                    previews.insert(voice.id.clone(), p);
                }
                if !voices.iter().any(|x: &CloudVoice| x.id == voice.id) {
                    voices.push(voice);
                }
            }
        }
        token = page["next_page_token"].as_str().filter(|t| !t.is_empty()).map(str::to_string);
        if !page["has_more"].as_bool().unwrap_or(false) || token.is_none() {
            break;
        }
    }
    voices.sort_by(|a, b| b.own.cmp(&a.own).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    *PREVIEWS.lock().unwrap_or_else(|p| p.into_inner()) = previews;
    Ok(voices)
}

/// The models that speak, recommended one first; Yap's own list when they
/// can't be read. Pure (the API's answer in).
pub fn models_of(list: Option<&Value>) -> Vec<CloudModel> {
    let known = |id: &str| KNOWN_MODELS.iter().find(|(k, _, _)| *k == id);
    let mut out: Vec<CloudModel> = match list.and_then(Value::as_array) {
        Some(all) => all
            .iter()
            .filter(|m| m["can_do_text_to_speech"].as_bool().unwrap_or(false))
            .filter(|m| !m["requires_alpha_access"].as_bool().unwrap_or(false))
            .filter_map(|m| {
                let id = m["model_id"].as_str()?.to_string();
                let langs = m["languages"].as_array().map_or(0, Vec::len);
                let (name, desc) = match known(&id) {
                    Some((_, n, d)) => (n.to_string(), d.to_string()),
                    None => (
                        m["name"].as_str().unwrap_or(&id).trim_start_matches("Eleven ").to_string(),
                        if langs > 0 { format!("{langs} languages") } else { String::new() },
                    ),
                };
                Some(CloudModel { recommended: id == DEFAULT_MODEL, id, name, desc })
            })
            .collect(),
        None => Vec::new(),
    };
    if out.is_empty() {
        out = KNOWN_MODELS
            .iter()
            .map(|(id, n, d)| CloudModel { id: id.to_string(), name: n.to_string(), desc: d.to_string(), recommended: *id == DEFAULT_MODEL })
            .collect();
    }
    out.sort_by_key(|m| !m.recommended);
    out
}

/// The models Settings offers (Yap's own list if the API's can't be read).
pub async fn list_models(key: &Key) -> Vec<CloudModel> {
    let list = get_json("/v1/models", key).await.ok();
    models_of(list.as_ref())
}

/// The month's characters, if the key may read them. Pure (the API's answer in).
pub fn usage_of(v: &Value) -> Option<Usage> {
    let used = v["character_count"].as_u64()?;
    let limit = v["character_limit"].as_u64()?;
    Some(Usage { used, limit, resets_at: v["next_character_count_reset_unix"].as_i64() })
}

pub async fn usage(key: &Key) -> Option<Usage> {
    usage_of(&get_json("/v1/user/subscription", key).await.ok()?)
}

// ---- ▶: a voice's own sample -----------------------------------------------------------------

/// The sample of a voice listed last, if it has one.
pub fn preview_url(voice: &str) -> Option<String> {
    PREVIEWS.lock().unwrap_or_else(|p| p.into_inner()).get(voice).cloned()
}

/// Whether Yap may fetch a sample from `url`: https, no user name — or, in a
/// test run, the stand-in's own address.
pub fn sample_allowed(url: &str) -> bool {
    if crate::shell::e2e::active() {
        return base().is_ok_and(|b| url.starts_with(&format!("{b}/")));
    }
    reqwest::Url::parse(url)
        .is_ok_and(|u| u.scheme() == "https" && u.username().is_empty() && u.password().is_none() && u.host_str().is_some())
}

/// Fetch and decode a voice's sample (mp3 from ElevenLabs; WAV in a test run).
pub async fn fetch_sample(url: &str) -> Result<Pcm, String> {
    if !sample_allowed(url) {
        return Err("Yap won't fetch that sample".into());
    }
    let resp = SAMPLE_HTTP.get(url).send_for(crate::privacy::net::Purpose::VoiceSample(NAME)).await.map_err(|_| format!("Couldn't fetch the {NAME} sample"))?;
    if !resp.status().is_success() {
        return Err(format!("Couldn't fetch the {NAME} sample ({})", resp.status().as_u16()));
    }
    if resp.content_length().is_some_and(|n| n as usize > MAX_PREVIEW_BYTES) {
        return Err("The sample is too large".into());
    }
    let bytes = resp.bytes().await.map_err(|_| format!("Couldn't fetch the {NAME} sample"))?;
    if bytes.len() > MAX_PREVIEW_BYTES {
        return Err("The sample is too large".into());
    }
    decode(bytes.to_vec())
}

/// Compressed audio (mp3, WAV…) in memory → PCM, with Symphonia (as Upload
/// decodes files).
pub fn decode(bytes: Vec<u8>) -> Result<Pcm, String> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::DecoderOptions;
    use symphonia::core::errors::Error as SymError;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    const BAD: &str = "The sample isn't audio Yap can play";
    let mss = MediaSourceStream::new(Box::new(std::io::Cursor::new(bytes)), Default::default());
    let probed = symphonia::default::get_probe()
        .format(&Hint::new(), mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|_| BAD.to_string())?;
    let mut format = probed.format;
    let track = format.default_track().or_else(|| format.tracks().first()).ok_or(BAD)?;
    let track_id = track.id;
    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default()).map_err(|_| BAD.to_string())?;
    let mut samples = Vec::new();
    let (mut rate, mut channels) = (track.codec_params.sample_rate.unwrap_or(0), 0u16);
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(_) => break,
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymError::DecodeError(_)) => continue,
            Err(_) => break,
        };
        let spec = *decoded.spec();
        rate = spec.rate;
        channels = spec.channels.count().max(1) as u16;
        let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buf.copy_interleaved_ref(decoded);
        samples.extend_from_slice(buf.samples());
        if samples.len() > 60 * 48_000 * 2 {
            break;
        }
    }
    if samples.is_empty() || rate == 0 || channels == 0 {
        return Err(BAD.into());
    }
    Ok(Pcm { sample_rate: rate, channels, samples })
}

/// ▶ for a voice with a sample: an "engine" that plays that sample, fetched
/// when it's asked to speak (on the speaker's thread).
pub struct Sample {
    pub url: String,
}

impl Engine for Sample {
    fn id(&self) -> &'static str {
        ID
    }
    fn voices(&self) -> Result<Vec<Voice>, String> {
        Ok(Vec::new())
    }
    fn default_voice(&self) -> Option<String> {
        None
    }
    fn open(&self, _voice: &str, _rate: f32) -> Result<Box<dyn Synth>, String> {
        Ok(Box::new(SampleSynth { url: self.url.clone() }))
    }
}

struct SampleSynth {
    url: String,
}

impl Synth for SampleSynth {
    fn synthesize(&mut self, _text: &str) -> Result<Pcm, String> {
        tauri::async_runtime::block_on(fetch_sample(&self.url))
    }
}

/// A local stand-in for ElevenLabs' speech endpoint, for the unit tests
/// (here and the speaker's in `tts.rs`): one reply per connection, in order.
#[cfg(test)]
pub(crate) mod standin {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    /// The streaming tests one at a time: a stop (`cancel`) reaches every
    /// sentence being fetched.
    pub(crate) fn serial() -> std::sync::MutexGuard<'static, ()> {
        static SERIAL: Mutex<()> = Mutex::new(());
        SERIAL.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// What the stand-in answers one request with.
    pub(crate) enum Reply {
        /// A status and a JSON body.
        Error(u16, String),
        /// 200 with PCM, chunked: these piece sizes, `delay` before the first.
        Pcm { bytes: Vec<u8>, pieces: Vec<usize>, delay: Duration },
        /// Accept, then say nothing.
        Silent,
        /// 200 with `/stream/with-timestamps`' JSON lines, each body cut in
        /// these pieces (a line may arrive in parts).
        Timed { body: String, pieces: Vec<usize> },
    }

    pub(crate) struct Stub {
        pub base: String,
        pub requests: Arc<Mutex<Vec<String>>>,
    }

    pub(crate) fn stub(replies: Vec<Reply>) -> Stub {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&requests);
        std::thread::spawn(move || {
            for reply in replies {
                let Ok((mut s, _)) = listener.accept() else { return };
                // The request: headers, then Content-Length bytes of body.
                let mut raw = Vec::new();
                let mut buf = [0u8; 4096];
                loop {
                    let n = s.read(&mut buf).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    raw.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&raw).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let len = text[..end]
                            .lines()
                            .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                            .unwrap_or(0);
                        if raw.len() >= end + 4 + len {
                            break;
                        }
                    }
                }
                seen.lock().unwrap().push(String::from_utf8_lossy(&raw).to_string());
                match reply {
                    Reply::Error(status, body) => {
                        let _ = write!(
                            s,
                            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                    }
                    Reply::Pcm { bytes, pieces, delay } => {
                        std::thread::sleep(delay);
                        let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: audio/pcm\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n");
                        let mut at = 0;
                        for n in pieces {
                            let piece = &bytes[at..at + n];
                            at += n;
                            let _ = write!(s, "{n:x}\r\n");
                            let _ = s.write_all(piece);
                            let _ = write!(s, "\r\n");
                            let _ = s.flush();
                        }
                        let _ = write!(s, "0\r\n\r\n");
                    }
                    Reply::Silent => {
                        std::thread::sleep(Duration::from_secs(30));
                    }
                    Reply::Timed { body, pieces } => {
                        let bytes = body.into_bytes();
                        let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n");
                        let mut at = 0;
                        for n in pieces.into_iter().chain([usize::MAX]) {
                            let n = n.min(bytes.len() - at);
                            if n == 0 {
                                break;
                            }
                            let _ = write!(s, "{n:x}\r\n");
                            let _ = s.write_all(&bytes[at..at + n]);
                            let _ = write!(s, "\r\n");
                            let _ = s.flush();
                            at += n;
                        }
                        let _ = write!(s, "0\r\n\r\n");
                    }
                }
            }
        });
        Stub { base, requests }
    }

    pub(crate) fn pcm_bytes(samples: usize) -> Vec<u8> {
        (0..samples).flat_map(|i| (((i % 200) as i16 - 100) * 50).to_le_bytes()).collect()
    }

    /// `/stream/with-timestamps`' answer for `text`, as ElevenLabs sends it:
    /// one JSON line per piece (`per_piece` characters each), its audio
    /// base64, each character `char_ms` long on the whole sentence's clock
    /// (or each piece's own clock, `piece_clock`).
    pub(crate) fn timed_body(text: &str, per_piece: usize, char_ms: u64, piece_clock: bool) -> String {
        use base64::Engine as _;
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::new();
        for (p, part) in chars.chunks(per_piece.max(1)).enumerate() {
            let first = p * per_piece.max(1);
            let samples = part.len() * char_ms as usize * 24;
            let audio = base64::engine::general_purpose::STANDARD.encode(pcm_bytes(samples));
            let clock = |k: usize| (if piece_clock { k } else { first + k }) as f64 * char_ms as f64 / 1000.0;
            let line = serde_json::json!({
                "audio_base64": audio,
                "alignment": {
                    "characters": part.iter().map(|c| c.to_string()).collect::<Vec<_>>(),
                    "character_start_times_seconds": (0..part.len()).map(clock).collect::<Vec<_>>(),
                    "character_end_times_seconds": (0..part.len()).map(|k| clock(k + 1)).collect::<Vec<_>>(),
                },
                "normalized_alignment": null,
            });
            out.push_str(&line.to_string());
            out.push('\n');
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::standin::{pcm_bytes, serial, stub, timed_body, Reply};
    use super::*;
    use std::net::TcpListener;

    const SECRET: &str = "sk_test_0123456789abcdef";

    #[test]
    fn the_request_has_the_key_in_a_header_only() {
        let key = Key::new(SECRET);
        let req = speech_request("https://api.elevenlabs.io", &key, "JBFqnCBsd6RMkjVDRZzb", "", 1.0, "Hello there.").unwrap();
        assert_eq!(req.url, "https://api.elevenlabs.io/v1/text-to-speech/JBFqnCBsd6RMkjVDRZzb/stream?output_format=pcm_24000");
        assert!(!req.url.contains(SECRET));
        assert_eq!(req.body, json!({ "text": "Hello there.", "model_id": "eleven_flash_v2_5" }));
        // Logging the request never shows the key.
        let logged = format!("{req:?}");
        assert!(!logged.contains(SECRET), "{logged}");
        assert!(logged.contains("[redacted]"));
        // Speed: only when it isn't 1×, within ElevenLabs' 0.7–1.2.
        let fast = speech_request("b", &key, "v1", "eleven_v3", 1.5, "x").unwrap();
        assert_eq!(fast.body["voice_settings"]["speed"], json!(1.2));
        assert_eq!(fast.body["model_id"], "eleven_v3");
        assert_eq!(speed_for(0.9), Some(0.9));
        assert_eq!(speed_for(0.5), Some(0.7));
        assert_eq!(speed_for(f32::NAN), None);
        // No voice: the default one. A voice id that would change the path: refused.
        assert!(speech_request("b", &key, "", "", 1.0, "x").unwrap().url.contains(DEFAULT_VOICE));
        assert!(speech_request("b", &key, "../v1/user", "", 1.0, "x").is_err());
        assert!(speech_request("b", &key, "a?b=c", "", 1.0, "x").is_err());
        assert!(speech_request("b", &Key::new("  "), "v", "", 1.0, "x").is_err());
    }

    #[test]
    fn pcm_survives_any_split() {
        let samples: [i16; 5] = [0, 16_384, -16_384, 32_767, -32_768];
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let want: Vec<f32> = samples.iter().map(|&s| f32::from(s) / 32_768.0).collect();
        for split in [vec![10], vec![1, 9], vec![3, 3, 4], vec![1, 1, 1, 1, 1, 1, 1, 1, 1, 1], vec![0, 5, 0, 5]] {
            let mut d = S16Le::default();
            let mut got = Vec::new();
            let mut at = 0;
            for n in split.iter().copied() {
                got.extend(d.push(&bytes[at..at + n]));
                at += n;
            }
            assert_eq!(got, want, "split {split:?}");
        }
    }

    #[test]
    fn errors_are_named() {
        let k = Key::new(SECRET);
        let body = |code: &str| format!(r#"{{"detail":{{"type":"x","code":"{code}","message":"m"}}}}"#);
        assert_eq!(classify(401, &body("invalid_api_key"), &k), Failure::BadKey);
        assert_eq!(classify(401, "", &k), Failure::BadKey);
        assert_eq!(classify(402, &body("insufficient_credits"), &k), Failure::Quota);
        // The older shape: 401 + "quota_exceeded".
        assert_eq!(classify(401, r#"{"detail":{"status":"quota_exceeded","message":"This request exceeds your quota."}}"#, &k), Failure::Quota);
        assert_eq!(classify(401, r#"{"detail":{"status":"missing_permissions","message":"The API key you used is missing the permission text_to_speech"}}"#, &k), Failure::NotAllowed);
        assert_eq!(classify(403, &body("insufficient_permissions"), &k), Failure::NotAllowed);
        assert_eq!(classify(429, &body("concurrent_limit_exceeded"), &k), Failure::RateLimited);
        assert_eq!(classify(429, "", &k), Failure::RateLimited);
        assert_eq!(classify(404, &body("voice_not_found"), &k), Failure::VoiceGone);
        assert_eq!(classify(400, &body("unsupported_model"), &k), Failure::ModelGone);
        assert_eq!(classify(503, "<html>", &k), Failure::ServiceDown);
        assert_eq!(classify(422, r#"{"detail":[{"loc":["body","text"],"msg":"field required","type":"missing"}]}"#, &k), Failure::Rejected("field required".into()));
        // Whatever the service says back never carries the key.
        let echoed = format!(r#"{{"detail":{{"status":"detected_unusual_activity","message":"Unusual activity on key {SECRET}."}}}}"#);
        match classify(401, &echoed, &k) {
            Failure::Rejected(m) => assert!(!m.contains(SECRET) && m.contains("[key]"), "{m}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn voices_models_and_usage_from_the_api() {
        let (v, p) = voice_of(&json!({
            "voice_id": "JBFqnCBsd6RMkjVDRZzb", "name": "George", "category": "premade",
            "labels": {"accent": "british", "gender": "male", "age": "middle_aged"},
            "preview_url": "https://storage.googleapis.com/x/george.mp3"
        }))
        .unwrap();
        assert_eq!(v, CloudVoice { id: "JBFqnCBsd6RMkjVDRZzb".into(), name: "George".into(), own: false, desc: "British · male · middle aged".into(), preview: true });
        assert_eq!(p.as_deref(), Some("https://storage.googleapis.com/x/george.mp3"));
        let (mine, p) = voice_of(&json!({"voice_id": "abc123", "name": "My voice", "category": "cloned", "labels": {}, "preview_url": null})).unwrap();
        assert!(mine.own && !mine.preview && p.is_none() && mine.desc.is_empty());
        assert!(voice_of(&json!({"voice_id": "../x", "name": "Bad"})).is_none());

        let models = models_of(Some(&json!([
            {"model_id": "eleven_multilingual_v2", "name": "Eleven Multilingual v2", "can_do_text_to_speech": true, "languages": [{}, {}]},
            {"model_id": "eleven_flash_v2_5", "name": "Eleven Flash v2.5", "can_do_text_to_speech": true},
            {"model_id": "eleven_english_sts_v2", "name": "Eleven English v2 STS", "can_do_text_to_speech": false},
            {"model_id": "eleven_secret", "name": "Eleven Secret", "can_do_text_to_speech": true, "requires_alpha_access": true},
            {"model_id": "eleven_new_v9", "name": "Eleven New v9", "can_do_text_to_speech": true, "languages": [{}, {}, {}]}
        ])));
        let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["eleven_flash_v2_5", "eleven_multilingual_v2", "eleven_new_v9"]);
        assert!(models[0].recommended);
        assert_eq!((models[2].name.as_str(), models[2].desc.as_str()), ("New v9", "3 languages"));
        // Unreadable: Yap's own list, the fast one first.
        let fallback = models_of(None);
        assert_eq!(fallback[0].id, DEFAULT_MODEL);
        assert_eq!(fallback.len(), KNOWN_MODELS.len());

        assert_eq!(
            usage_of(&json!({"character_count": 1234, "character_limit": 10000, "next_character_count_reset_unix": 1_800_000_000, "tier": "free"})),
            Some(Usage { used: 1234, limit: 10_000, resets_at: Some(1_800_000_000) })
        );
        assert_eq!(usage_of(&json!({"detail": {"status": "missing_permissions"}})), None);
    }

    #[test]
    fn samples_only_from_https() {
        assert!(sample_allowed("https://storage.googleapis.com/eleven-public-prod/x.mp3"));
        assert!(!sample_allowed("http://storage.googleapis.com/x.mp3"));
        assert!(!sample_allowed("https://user:pw@example.com/x.mp3"));
        assert!(!sample_allowed("file:///C:/Windows/win.ini"));
        assert!(!sample_allowed("not a url"));
    }

    #[test]
    fn a_wav_sample_decodes() {
        // 0.1 s of a 16-bit stereo WAV at 22,050 Hz.
        let frames = 2_205usize;
        let data: Vec<u8> = (0..frames * 2).flat_map(|i| ((i % 100) as i16 * 100).to_le_bytes()).collect();
        let mut w = Vec::new();
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&2u16.to_le_bytes());
        w.extend_from_slice(&22_050u32.to_le_bytes());
        w.extend_from_slice(&(22_050u32 * 4).to_le_bytes());
        w.extend_from_slice(&4u16.to_le_bytes());
        w.extend_from_slice(&16u16.to_le_bytes());
        w.extend_from_slice(b"data");
        w.extend_from_slice(&(data.len() as u32).to_le_bytes());
        w.extend_from_slice(&data);
        let pcm = decode(w).unwrap();
        assert_eq!((pcm.sample_rate, pcm.channels, pcm.samples.len()), (22_050, 2, frames * 2));
        assert!(decode(b"not audio at all".to_vec()).is_err());
    }

    // ---- against a local stand-in server (`standin`)

    fn run(req: &SpeechRequest) -> (Result<Streamed, Failure>, Vec<Pcm>) {
        let mut got = Vec::new();
        let r = tauri::async_runtime::block_on(stream_speech(req, &mut |p| {
            got.push(p);
            true
        }));
        (r, got)
    }

    #[test]
    fn a_sentence_streams_in_pieces_then_a_pause() {
        let _one_at_a_time = serial();
        // 0.5 s of audio in odd-sized pieces (a sample split across two).
        let bytes = pcm_bytes(12_000);
        let s = stub(vec![Reply::Pcm { bytes: bytes.clone(), pieces: vec![3, 4_801, 9_000, 10_196], delay: Duration::ZERO }]);
        let req = speech_request(&s.base, &Key::new(SECRET), "JBFqnCBsd6RMkjVDRZzb", "", 1.0, "Hello there.").unwrap();
        let (r, pieces) = run(&req);
        let done = r.unwrap();
        assert!(!done.stopped && done.first_audio.is_some());
        // Every sample, in order, then 150 ms of quiet.
        let all: Vec<f32> = pieces.iter().flat_map(|p| p.samples.clone()).collect();
        assert_eq!(all.len(), 12_000 + 3_600);
        assert_eq!(all[..12_000], S16Le::default().push(&bytes)[..]);
        assert!(all[12_000..].iter().all(|&x| x == 0.0));
        assert!(pieces.len() >= 2, "handed on as it came: {} pieces", pieces.len());
        assert!(pieces[..pieces.len() - 1].iter().all(|p| p.samples.len() >= MIN_PIECE));
        assert!(pieces.iter().all(|p| p.sample_rate == 24_000 && p.channels == 1));
        // The key went in its header, not the address.
        let raw = s.requests.lock().unwrap()[0].clone();
        let first = raw.lines().next().unwrap();
        assert_eq!(first, "POST /v1/text-to-speech/JBFqnCBsd6RMkjVDRZzb/stream?output_format=pcm_24000 HTTP/1.1");
        assert!(raw.to_ascii_lowercase().contains(&format!("xi-api-key: {SECRET}")));
        assert!(raw.contains(r#""model_id":"eleven_flash_v2_5""#) && raw.contains(r#""text":"Hello there.""#), "{raw}");
    }

    #[test]
    fn failures_come_back_named_and_fast() {
        let _one_at_a_time = serial();
        let key = Key::new(SECRET);
        let s = stub(vec![
            Reply::Error(401, r#"{"detail":{"type":"authentication_error","code":"invalid_api_key","message":"Invalid API key"}}"#.into()),
            Reply::Error(402, r#"{"detail":{"type":"payment_required","code":"insufficient_credits","message":"No credits"}}"#.into()),
            Reply::Error(429, r#"{"detail":{"type":"rate_limit_error","code":"system_busy","message":"Busy"}}"#.into()),
            Reply::Pcm { bytes: Vec::new(), pieces: vec![], delay: Duration::ZERO },
        ]);
        let req = speech_request(&s.base, &key, "v1", "", 1.0, "Hi.").unwrap();
        assert_eq!(run(&req).0, Err(Failure::BadKey));
        assert_eq!(run(&req).0, Err(Failure::Quota));
        assert_eq!(run(&req).0, Err(Failure::RateLimited));
        assert!(matches!(run(&req).0, Err(Failure::Rejected(m)) if m.contains("no audio")));
        // Nobody listening: no connection.
        let gone = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = gone.local_addr().unwrap();
        drop(gone);
        let req = speech_request(&format!("http://{addr}"), &key, "v1", "", 1.0, "Hi.").unwrap();
        assert_eq!(run(&req).0, Err(Failure::Offline));
    }

    #[test]
    fn a_stop_ends_a_sentence_still_being_fetched() {
        let _one_at_a_time = serial();
        let s = stub(vec![Reply::Silent]);
        let req = speech_request(&s.base, &Key::new(SECRET), "v1", "", 1.0, "Hi.").unwrap();
        let started = Instant::now();
        std::thread::spawn(|| {
            std::thread::sleep(Duration::from_millis(300));
            cancel();
        });
        let (r, pieces) = run(&req);
        assert!(r.unwrap().stopped);
        assert!(pieces.is_empty());
        assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
    }

    #[test]
    fn no_audio_in_time_is_a_timeout() {
        let _one_at_a_time = serial();
        let s = stub(vec![Reply::Silent]);
        let req = speech_request(&s.base, &Key::new(SECRET), "v1", "", 1.0, "Hi.").unwrap();
        let started = Instant::now();
        assert_eq!(run(&req).0, Err(Failure::TimedOut));
        let took = started.elapsed();
        assert!(took >= FIRST_AUDIO && took < FIRST_AUDIO + Duration::from_secs(2), "{took:?}");
    }

    #[test]
    fn out_saying_stop_ends_it() {
        let _one_at_a_time = serial();
        let s = stub(vec![Reply::Pcm { bytes: pcm_bytes(24_000), pieces: vec![4_800; 10], delay: Duration::ZERO }]);
        let req = speech_request(&s.base, &Key::new(SECRET), "v1", "", 1.0, "Hi.").unwrap();
        let mut n = 0;
        let r = tauri::async_runtime::block_on(stream_speech(&req, &mut |_| {
            n += 1;
            false
        }));
        assert!(r.unwrap().stopped);
        assert_eq!(n, 1);
    }

    #[test]
    fn the_timed_endpoint_streams_audio_and_each_characters_time() {
        let _one_at_a_time = serial();
        let text = "Read this aloud";
        // Pieces of 4 characters, cut mid-line on the wire.
        let body = timed_body(text, 4, 60, false);
        let s = stub(vec![Reply::Timed { body: body.clone(), pieces: vec![7, 300, 2, 5_000] }]);
        let req = timed_request(&s.base, &Key::new(SECRET), "JBFqnCBsd6RMkjVDRZzb", "", 1.0, text).unwrap();
        assert!(req.url.ends_with("/stream/with-timestamps?output_format=pcm_24000"), "{}", req.url);
        let mut got = Vec::new();
        let (done, times) = tauri::async_runtime::block_on(stream_speech_timed(&req, &mut |p| {
            got.push(p);
            true
        }))
        .unwrap();
        assert!(!done.stopped);
        // All the audio (15 characters × 60 ms), then the pause.
        let all: usize = got.iter().map(|p| p.samples.len()).sum();
        assert_eq!(all, 15 * 60 * 24 + 3_600);
        // Every character, on one clock: "this" starts at 5 × 60 ms.
        assert_eq!(times.len(), 15);
        assert_eq!(times[5], super::super::marks::CharTime { at: 5, start: Duration::from_millis(300) });
        let spans = super::super::marks::words(text);
        let starts = super::super::marks::from_chars(&spans, &times).unwrap();
        assert_eq!(starts, vec![Duration::ZERO, Duration::from_millis(300), Duration::from_millis(600)]);
        let raw = s.requests.lock().unwrap()[0].clone();
        assert!(raw.starts_with("POST /v1/text-to-speech/JBFqnCBsd6RMkjVDRZzb/stream/with-timestamps?output_format=pcm_24000 HTTP/1.1"));
        assert!(!raw.lines().next().unwrap().contains(SECRET));
    }

    #[test]
    fn timing_on_each_pieces_own_clock_is_put_after_the_audio_before_it() {
        let text = "Read this aloud";
        let mut p = Timestamped::default();
        let audio: usize = timed_body(text, 5, 60, true)
            .lines()
            .map(|l| p.push(format!("{l}\n").as_bytes()).unwrap().len())
            .sum();
        assert_eq!(audio, 15 * 60 * 24);
        assert!(p.finish().unwrap().is_empty());
        let starts: Vec<u64> = p.times.iter().map(|t| t.start.as_millis() as u64).collect();
        assert_eq!(&starts[..7], &[0, 60, 120, 180, 240, 300, 360]);
        assert!(starts.windows(2).all(|w| w[0] < w[1]), "{starts:?}");
        // Something that isn't JSON: said so.
        let mut bad = Timestamped::default();
        assert!(bad.push(b"<html>oops</html>\n").is_err());
    }

    /// Yap's own share of the first-audio wait, against a local stand-in
    /// that answers at once and one that waits like a real service (250 ms):
    /// request built, sent, the first 100 ms of PCM decoded and handed on.
    /// `cargo test tts::elevenlabs::tests::first_audio_overhead -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn first_audio_overhead() {
        let _one_at_a_time = serial();
        for delay in [Duration::ZERO, Duration::from_millis(250)] {
            let mut firsts = Vec::new();
            for _ in 0..10 {
                let s = stub(vec![Reply::Pcm { bytes: pcm_bytes(48_000), pieces: vec![4_096; 23].into_iter().chain([1_792]).collect(), delay }]);
                let req = speech_request(&s.base, &Key::new(SECRET), "v1", "", 1.0, "The quick brown fox jumps over the lazy dog.").unwrap();
                firsts.push(run(&req).0.unwrap().first_audio.unwrap());
            }
            firsts.sort();
            println!(
                "stand-in waiting {:?}: first audio median {:.1} ms, best {:.1} ms, worst {:.1} ms",
                delay,
                firsts[5].as_secs_f64() * 1000.0,
                firsts[0].as_secs_f64() * 1000.0,
                firsts[9].as_secs_f64() * 1000.0
            );
        }
    }
}

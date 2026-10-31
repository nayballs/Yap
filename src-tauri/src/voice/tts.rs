//! **Spoken replies** — Yap reads the AI's answers aloud (Settings → Voice
//! Assistant, the Chat view's speaker toggle, each answer's ▶, and every
//! answer to the assistant shortcut, `assistant.rs`).
//!
//! Three parts:
//!
//! - **Engines** ([`Engine`] → [`Synth`]): text in, PCM out. **Kokoro**
//!   (`tts/kokoro.rs`, the default: Kokoro-82M v1.0's natural voices on this
//!   PC's CPU, a ~379 MB download — `tts/registry.rs` — its text read by
//!   `tts/g2p.rs`), and **Windows voices** (WinRT
//!   `Windows.Media.SpeechSynthesis`, the OneCore voices every Windows
//!   install has: David, Zira, George…; no download), which speak until
//!   Kokoro is installed, in a build without it, or when chosen. Windows
//!   11's natural Narrator voices (Aria, Jenny, Guy…) are **Narrator-only**:
//!   the app API doesn't list them.
//! - **Text prep** ([`utterances`], `tts/text.rs`, pure): an answer's markdown as something
//!   to say — marks out, a code block as "(code block)", a link as its text,
//!   a bare URL as its domain, headings and bullets each their own sentence,
//!   emoji out — then numbers, money, dates, times, units and abbreviations
//!   written as words (`tts/normalize.rs`, UK style), then cut into
//!   sentences (abbreviations and initials kept together, long ones cut at a
//!   comma).
//! - **The speaker** (`tts/speaker.rs`, its output `tts/playback.rs`): one utterance at a time, queued by
//!   sentence on a worker thread — the first sentence plays while the next is
//!   made, so speech starts quickly — on the configured output device through
//!   rodio, like the chimes (`sound.rs`). [`stop`] is instant (the sink is
//!   emptied, the queue dropped). Every change is `yap-tts-state
//!   {speaking, messageId, error}`, so a page shows which answer is talking.
//!
//! **Never over a dictation or a meeting**: a dictation starting stops the
//! speech first (`pipeline::start_recording` → [`stop_for_dictation`], which
//! also tells it to drop its pre-roll if Yap was audible), the speaker
//! checks before every sentence, a meeting recording starting stops it, and
//! nothing is spoken while one records (the loopback would put Yap in the
//! "Them" side). There's no echo cancellation: Yap's voice from speakers can
//! still reach the mic of a dictation started by another route.
//!
//! **Test mode** (`e2e.rs`) plays nothing and opens no device: stand-in
//! voices, silence of a plausible length played on a simulated clock, and
//! everything that would have been said kept for the debug-only
//! [`tts_e2e_spoken`].

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

pub mod cloud;
pub mod elevenlabs;
// The G2P only runs in builds with Kokoro's engine; it's tested in all.
#[cfg_attr(not(feature = "engines"), allow(dead_code))]
mod g2p;
pub mod kokoro;
pub mod marks;
mod normalize;
mod playback;
pub mod registry;
mod speaker;
mod text;

pub use playback::{set_volume, Opened, Opener, Playback};
pub use speaker::{
    init, meeting_quiet, pause, position, prepare_for_reading, read, ready_sentences, resume, speak, speak_streamed,
    speak_streamed_to, speaking_id, stop, stop_for_dictation, Feed, Track, TtsState,
};
pub use text::{brief, lead, sentences, speakable_blocks, utterances};

use speaker::{say, speaker};

/// Windows' own voices' engine id.
pub const WINDOWS: &str = "windows";
/// The engine Yap speaks with when it can: Kokoro (Windows voices until
/// it's installed, in a build without it, or when chosen).
pub const DEFAULT_ENGINE: &str = kokoro::ID;
/// The message id of Settings' ▶ Preview.
pub const PREVIEW_ID: &str = "preview";
/// Nothing longer is read in one go (a pasted book, a runaway answer).
const MAX_CHARS: usize = 20_000;
/// A sentence longer than this is cut at a comma, so each piece starts soon.
const LONG_SENTENCE: usize = 220;
/// Yap's voice counts as still in the room this long after the last sound
/// (`stop_for_dictation`: the dictation's pre-roll would hold it).
const AUDIBLE_TAIL: Duration = Duration::from_millis(450);

// ---------------------------------------------------------------- engines

/// One voice an engine can speak with.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Voice {
    /// What `config.tts_voice` stores (Windows: the voice's registry id).
    pub id: String,
    /// "Microsoft Zira".
    pub name: String,
    /// BCP-47: "en-US".
    pub language: String,
    /// "female" | "male" | "".
    pub gender: String,
    /// How Settings groups it: "British English", "American English",
    /// "Spanish"… (Windows voices: from their language).
    pub accent: String,
}

/// "bella" → "Bella".
pub(crate) fn normalize_name(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// A Windows voice's accent from its language tag.
fn accent_of(language: &str) -> String {
    match language {
        "en-GB" => "British English".into(),
        "en-US" => "American English".into(),
        l if l.starts_with("en-") => format!("English ({})", &l[3..]),
        l => l.to_string(),
    }
}

/// Mono or stereo audio, f32 in -1..1.
#[derive(Debug, Clone, PartialEq)]
pub struct Pcm {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

impl Pcm {
    pub fn duration(&self) -> Duration {
        let frames = self.samples.len() as f64 / f64::from(self.channels.max(1));
        Duration::from_secs_f64(frames / f64::from(self.sample_rate.max(1)))
    }
}

/// A speech engine: the voices it has, and a synthesiser for one voice.
pub trait Engine: Send + Sync {
    /// The id `config.tts_engine` stores ("kokoro", "windows"; `ENGINES`
    /// names them).
    fn id(&self) -> &'static str;
    fn voices(&self) -> Result<Vec<Voice>, String>;
    /// The voice speaking when the config names none (or one that's gone).
    fn default_voice(&self) -> Option<String>;
    /// A synthesiser for `voice` ("" = the default) at `rate` (1.0 = normal),
    /// made on the thread that will use it.
    fn open(&self, voice: &str, rate: f32) -> Result<Box<dyn Synth>, String>;
    /// The sentences as this engine says them best (Kokoro joins tiny ones).
    fn prepare(&self, sentences: Vec<String>) -> Vec<String> {
        sentences
    }
    /// Cut a sentence being made short (a stop): Kokoro's run in flight.
    fn cancel(&self) {}
}

/// Text → audio, one sentence at a time.
pub trait Synth {
    fn synthesize(&mut self, text: &str) -> Result<Pcm, String>;
    /// Say `text`, handing the audio to `out` as it's made (`out` → false:
    /// stop, which isn't a failure). A streaming engine (a cloud voice)
    /// hands on its first piece before the sentence is all made; the rest
    /// make the whole sentence first.
    fn stream(&mut self, text: &str, out: &mut dyn FnMut(Pcm) -> bool) -> Result<(), String> {
        let pcm = self.synthesize(text)?;
        out(pcm);
        Ok(())
    }
    /// When the words of the sentence just made start, if the engine says
    /// (`marks::CharTime`s over the text it was given): Windows voices' word
    /// cues, ElevenLabs' character alignment. `None`: the speaker works it
    /// out from the audio (`marks::from_audio`). Asked once, after `stream`.
    fn marks(&mut self) -> Option<Vec<marks::CharTime>> {
        None
    }
}

/// The engine for `id`: Kokoro when that's asked for and it can speak
/// (installed, and in this build), else Windows voices (test mode: their
/// stand-in).
pub fn engine(id: &str) -> Arc<dyn Engine> {
    if id != WINDOWS && kokoro::available() {
        return Arc::new(kokoro::Kokoro);
    }
    if crate::shell::e2e::active() {
        return Arc::new(StandIn);
    }
    Arc::new(WindowsVoices)
}

/// The engine and voice speaking now, from the config: Kokoro with
/// `tts_voice` (af_bella when unset), or Windows voices with
/// `tts_windows_voice` ("" = Windows' default).
pub fn chosen(cfg: &crate::config::YapConfig) -> (Arc<dyn Engine>, String) {
    let e = engine(&cfg.tts_engine);
    let voice = if e.id() == kokoro::ID {
        // A Japanese or Mandarin voice an older Yap offered: the default
        // speaks (`kokoro::listed`).
        kokoro::spoken_voice(&cfg.tts_voice).to_string()
    } else {
        cfg.tts_windows_voice.clone()
    };
    (e, voice)
}

/// The local voice that can stand in for a cloud one: Kokoro when it's
/// installed (and chosen), Windows voices when those were chosen — else
/// none (Kokoro chosen but not downloaded: the answer goes unsaid rather
/// than in a voice nobody picked).
pub fn local_voice(cfg: &crate::config::YapConfig) -> Option<(Arc<dyn Engine>, String)> {
    (cfg.tts_engine == WINDOWS || kokoro::available()).then(|| chosen(cfg))
}

/// Who reads an answer: an engine and its voice, and for a cloud voice
/// the plan if it fails ([`CloudPlan`]).
pub struct Voicing {
    pub engine: Arc<dyn Engine>,
    pub voice: String,
    pub cloud: Option<CloudPlan>,
}

/// A cloud voice's way out: the local voice that takes over (if any) and
/// what identifies this key and choice (`cloud::Health`).
pub struct CloudPlan {
    pub provider: &'static str,
    pub local: Option<(Arc<dyn Engine>, String)>,
    pub key: u64,
    pub choice: u64,
}

/// Who speaks now, from the config (`tts_provider`): a cloud voice when one
/// is picked **and** has a key — unless it failed a moment ago and a local
/// voice can stand in (`cloud::Health::skipping`) — else the local voice
/// ([`chosen`]). Nothing goes to a cloud service without its key.
pub fn voicing(cfg: &crate::config::YapConfig) -> Voicing {
    voicing_for(cfg, false)
}

/// [`voicing`], for reading aloud when `timed`: a cloud voice is asked for
/// its word timing too (ElevenLabs' `/stream/with-timestamps`), for the
/// reading card's karaoke.
pub fn voicing_for(cfg: &crate::config::YapConfig, timed: bool) -> Voicing {
    let local = || {
        let (engine, voice) = chosen(cfg);
        Voicing { engine, voice, cloud: None }
    };
    if cfg.tts_provider != elevenlabs::ID {
        return local();
    }
    let key = cfg.tts_cloud_keys.get(elevenlabs::ID).map(|k| k.trim()).unwrap_or("");
    if key.is_empty() {
        return local();
    }
    let choice = cfg.tts_cloud.get(elevenlabs::ID).cloned().unwrap_or_default();
    let fallback = local_voice(cfg);
    let key_fp = elevenlabs::Key::new(key).fingerprint();
    let choice_fp = elevenlabs::ElevenLabs::choice_fingerprint(key, &choice.voice, &choice.model);
    if fallback.is_some() && cloud::with_health(|h| h.skipping(elevenlabs::ID, key_fp, choice_fp, Instant::now())) {
        tracing::info!("Spoken replies: ElevenLabs failed a moment ago; Yap's own voice reads this one");
        return local();
    }
    Voicing {
        engine: Arc::new(elevenlabs::ElevenLabs::new(key, &choice.model, true).timed(timed)),
        voice: choice.voice,
        cloud: Some(CloudPlan { provider: elevenlabs::ID, local: fallback, key: key_fp, choice: choice_fp }),
    }
}

/// **Who reads an answer on a phone** (Yap Link, `link/voice.rs`): with
/// "Speak replies" on, the voice Settings picked ([`voicing`]: ElevenLabs
/// too, with its local stand-in); with it off, the phone still talks — it
/// was asked out loud, into a headset — in Yap's own voice: Kokoro when it's
/// downloaded, else Windows voices (never a cloud voice the person didn't
/// switch on).
pub fn phone_voicing(cfg: &crate::config::YapConfig) -> Voicing {
    if cfg.speak_replies {
        return voicing(cfg);
    }
    let engine = engine(kokoro::ID);
    let voice = if engine.id() == kokoro::ID { kokoro::spoken_voice(&cfg.tts_voice).to_string() } else { cfg.tts_windows_voice.clone() };
    Voicing { engine, voice, cloud: None }
}

/// The engines Settings lists: `(id, name)`.
pub const ENGINES: &[(&str, &str)] = &[(kokoro::ID, kokoro::NAME), (WINDOWS, "Windows voices")];

/// Windows' own voices (WinRT `SpeechSynthesizer`): the OneCore voices any
/// app can use, not Narrator's natural ones.
pub struct WindowsVoices;

impl Engine for WindowsVoices {
    fn id(&self) -> &'static str {
        WINDOWS
    }
    fn voices(&self) -> Result<Vec<Voice>, String> {
        win::voices()
    }
    fn default_voice(&self) -> Option<String> {
        win::default_voice()
    }
    fn open(&self, voice: &str, rate: f32) -> Result<Box<dyn Synth>, String> {
        win::open(voice, rate)
    }
}

#[cfg(windows)]
mod win {
    use super::marks::CharTime;
    use super::{Pcm, Synth, Voice};
    use windows::core::{Interface, HSTRING};
    use windows::Media::Core::SpeechCue;
    use windows::Media::SpeechSynthesis::{
        SpeechAppendedSilence, SpeechSynthesisStream, SpeechSynthesizer, VoiceGender, VoiceInformation,
    };
    use windows::Storage::Streams::DataReader;

    fn err(e: windows::core::Error) -> String {
        format!("Windows speech: {}", e.message())
    }

    fn info(v: &VoiceInformation) -> windows::core::Result<Voice> {
        let language = v.Language()?.to_string();
        Ok(Voice {
            id: v.Id()?.to_string(),
            name: v.DisplayName()?.to_string(),
            accent: super::accent_of(&language),
            language,
            gender: match v.Gender()? {
                VoiceGender::Female => "female".into(),
                VoiceGender::Male => "male".into(),
                _ => String::new(),
            },
        })
    }

    fn all() -> windows::core::Result<Vec<VoiceInformation>> {
        let list = SpeechSynthesizer::AllVoices()?;
        (0..list.Size()?).map(|i| list.GetAt(i)).collect()
    }

    pub fn voices() -> Result<Vec<Voice>, String> {
        all().and_then(|vs| vs.iter().map(info).collect()).map_err(err)
    }

    pub fn default_voice() -> Option<String> {
        SpeechSynthesizer::DefaultVoice().and_then(|v| v.Id()).ok().map(|id| id.to_string())
    }

    struct Windows {
        synth: SpeechSynthesizer,
        /// The last sentence's word cues (`marks`).
        cues: Option<Vec<CharTime>>,
    }

    /// The word cues Windows put on a sentence's stream (asked for with
    /// `IncludeWordBoundaryMetadata`): where each word starts in `text` (a
    /// UTF-16 offset there, a char index here) and when. Sentence cues, if
    /// any, are left out. `None` when there are none (Windows before 1709).
    fn word_cues(stream: &SpeechSynthesisStream, text: &str) -> Option<Vec<CharTime>> {
        let tracks = stream.TimedMetadataTracks().ok()?;
        let mut out = Vec::new();
        for i in 0..tracks.Size().ok()? {
            let Ok(track) = tracks.GetAt(i) else { continue };
            if track.Id().is_ok_and(|id| id.to_string().to_ascii_lowercase().contains("sentence")) {
                continue;
            }
            let Ok(cues) = track.Cues() else { continue };
            for j in 0..cues.Size().unwrap_or(0) {
                let Ok(cue) = cues.GetAt(j) else { continue };
                let Ok(speech) = cue.cast::<SpeechCue>() else { continue };
                let Some(at) = speech.StartPositionInInput().ok().and_then(|r| r.Value().ok()) else { continue };
                let Ok(start) = cue.StartTime() else { continue };
                out.push(CharTime {
                    at: super::marks::utf16_to_char(text, at.max(0) as usize),
                    start: std::time::Duration::from_nanos(start.Duration.max(0) as u64 * 100),
                });
            }
        }
        out.sort_by_key(|c| (c.start, c.at));
        (!out.is_empty()).then_some(out)
    }

    pub fn open(voice: &str, rate: f32) -> Result<Box<dyn Synth>, String> {
        let synth = SpeechSynthesizer::new().map_err(err)?;
        if !voice.is_empty() {
            match all().map_err(err)?.into_iter().find(|v| v.Id().is_ok_and(|id| id == voice)) {
                Some(v) => synth.SetVoice(&v).map_err(err)?,
                None => tracing::info!("Spoken replies: the chosen voice is gone, using Windows' default"),
            }
        }
        let options = synth.Options().map_err(err)?;
        options.SetSpeakingRate(f64::from(rate.clamp(0.5, 3.0))).map_err(err)?;
        // The queue adds its own pause between sentences (Windows 10 1709+).
        let _ = options.SetAppendedSilence(SpeechAppendedSilence::Min);
        // Where each word starts, for reading aloud's karaoke (1703+).
        let _ = options.SetIncludeWordBoundaryMetadata(true);
        Ok(Box::new(Windows { synth, cues: None }))
    }

    impl Synth for Windows {
        fn synthesize(&mut self, text: &str) -> Result<Pcm, String> {
            let stream = self.synth.SynthesizeTextToStreamAsync(&HSTRING::from(text)).and_then(|op| op.get()).map_err(err)?;
            self.cues = word_cues(&stream, text);
            let size = u32::try_from(stream.Size().map_err(err)?).map_err(|_| "Windows speech: too much audio".to_string())?;
            let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0).map_err(err)?).map_err(err)?;
            reader.LoadAsync(size).and_then(|op| op.get()).map_err(err)?;
            let mut bytes = vec![0u8; size as usize];
            reader.ReadBytes(&mut bytes).map_err(err)?;
            let _ = stream.Close();
            super::parse_wav(&bytes)
        }
        fn marks(&mut self) -> Option<Vec<CharTime>> {
            self.cues.take()
        }
    }
}

#[cfg(not(windows))]
mod win {
    use super::{Synth, Voice};
    const NONE: &str = "Spoken replies need Windows' voices";
    pub fn voices() -> Result<Vec<Voice>, String> {
        Err(NONE.into())
    }
    pub fn default_voice() -> Option<String> {
        None
    }
    pub fn open(_voice: &str, _rate: f32) -> Result<Box<dyn Synth>, String> {
        Err(NONE.into())
    }
}

/// Test mode's engine (and the unit tests'): two made-up voices and silence
/// as long as the words would take to say (~250 ms a word at 1×).
pub struct StandIn;

impl Engine for StandIn {
    fn id(&self) -> &'static str {
        WINDOWS
    }
    fn voices(&self) -> Result<Vec<Voice>, String> {
        Ok(vec![
            Voice {
                id: "e2e-voice-1".into(),
                name: "Test Voice One".into(),
                language: "en-GB".into(),
                gender: "female".into(),
                accent: "British English".into(),
            },
            Voice {
                id: "e2e-voice-2".into(),
                name: "Test Voice Two".into(),
                language: "en-US".into(),
                gender: "male".into(),
                accent: "American English".into(),
            },
        ])
    }
    fn default_voice(&self) -> Option<String> {
        Some("e2e-voice-1".into())
    }
    fn open(&self, _voice: &str, rate: f32) -> Result<Box<dyn Synth>, String> {
        Ok(Box::new(Silence { rate: rate.clamp(0.5, 3.0) }))
    }
}

struct Silence {
    rate: f32,
}

impl Synth for Silence {
    fn synthesize(&mut self, text: &str) -> Result<Pcm, String> {
        let words = text.split_whitespace().count().max(2) as f32;
        let secs = words * 0.25 / self.rate;
        Ok(Pcm { sample_rate: 8_000, channels: 1, samples: vec![0.0; (secs * 8_000.0) as usize] })
    }
}

/// A RIFF WAVE file's audio (what Windows' synthesiser hands back): 8/16/24/
/// 32-bit PCM or 32-bit float, any rate and channel count.
pub(crate) fn parse_wav(bytes: &[u8]) -> Result<Pcm, String> {
    const BAD: &str = "Windows speech: not a WAV stream";
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(BAD.into());
    }
    let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    let mut fmt: Option<(u16, u16, u32, u16)> = None; // format, channels, rate, bits
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let len = u32_at(at + 4) as usize;
        let body = at + 8;
        let end = body.saturating_add(len).min(bytes.len());
        if id == b"fmt " && end >= body + 16 {
            let mut format = u16_at(body);
            // WAVE_FORMAT_EXTENSIBLE: the real format is the sub-format GUID's first two bytes.
            if format == 0xFFFE && end >= body + 26 {
                format = u16_at(body + 24);
            }
            fmt = Some((format, u16_at(body + 2), u32_at(body + 4), u16_at(body + 14)));
        } else if id == b"data" {
            let (format, channels, sample_rate, bits) = fmt.ok_or(BAD)?;
            let data = &bytes[body..end];
            let samples: Vec<f32> = match (format, bits) {
                (1, 8) => data.iter().map(|&b| (f32::from(b) - 128.0) / 128.0).collect(),
                (1, 16) => data.as_chunks::<2>().0.iter().map(|&c| f32::from(i16::from_le_bytes(c)) / 32_768.0).collect(),
                (1, 24) => data
                    .as_chunks::<3>()
                    .0
                    .iter()
                    .map(|c| (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 / 8_388_608.0)
                    .collect(),
                (1, 32) => data.as_chunks::<4>().0.iter().map(|&c| i32::from_le_bytes(c) as f32 / 2_147_483_648.0).collect(),
                (3, 32) => data.as_chunks::<4>().0.iter().map(|&c| f32::from_le_bytes(c)).collect(),
                _ => return Err(format!("Windows speech: unsupported audio ({format}, {bits}-bit)")),
            };
            if channels == 0 || sample_rate == 0 {
                return Err(BAD.into());
            }
            return Ok(Pcm { sample_rate, channels, samples });
        }
        // Chunks are padded to an even length.
        at = body.saturating_add(len + (len & 1));
    }
    Err(BAD.into())
}

// ------------------------------------------------------- when to speak

/// "When to speak" (`config.speak_when`): every Chat answer, or only an
/// answer to a question that came by voice (the default).
pub fn speaks_always(when: &str) -> bool {
    when == "always"
}

/// Whether a Chat answer is read out by itself: replies are on, "When to
/// speak" says so (`always`, or the question was dictated), and nothing is
/// listening — a dictation recording now, or a meeting. Pure.
pub fn auto_speak(speak_replies: bool, when: &str, asked_by_voice: bool, dictating: bool, meeting: bool) -> bool {
    speak_replies && !dictating && !meeting && (speaks_always(when) || asked_by_voice)
}

/// Whether a question dictated into the chat box is sent when the dictation
/// ends: "Talk back hands-free" (part of spoken replies: off with them), or
/// the "press enter" voice command said at its end. Pure.
pub fn sends_by_itself(speak_replies: bool, hands_free: bool, said_enter: bool) -> bool {
    said_enter || (speak_replies && hands_free)
}

// ------------------------------------------------------------ commands

/// One engine's voices, for Settings → Voice Assistant.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineVoices {
    pub voices: Vec<Voice>,
    pub default_voice: Option<String>,
    pub error: Option<String>,
}

/// Settings → Voice Assistant: the engines, each one's voices, Kokoro's
/// download, and which engine would speak now (`active`: Windows voices
/// while Kokoro isn't installed or isn't in this build).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceList {
    /// Who speaks: "local" or a cloud voice ("elevenlabs").
    pub provider: String,
    /// The providers to pick from: `[{id, name}]` (`cloud::PROVIDERS`).
    pub providers: Vec<serde_json::Value>,
    /// The configured local engine.
    pub engine: String,
    /// The engine that would speak now.
    pub active: String,
    pub engines: Vec<serde_json::Value>,
    pub kokoro: EngineVoices,
    pub kokoro_status: registry::Status,
    pub windows: EngineVoices,
}

fn windows_voices() -> EngineVoices {
    let e: Arc<dyn Engine> = if crate::shell::e2e::active() { Arc::new(StandIn) } else { Arc::new(WindowsVoices) };
    let (voices, error) = match e.voices() {
        Ok(v) if v.is_empty() => (v, Some("Windows has no voices installed (Settings → Time & language → Speech)".to_string())),
        Ok(v) => (v, None),
        Err(err) => (Vec::new(), Some(err)),
    };
    EngineVoices { voices, default_voice: e.default_voice(), error }
}

#[tauri::command]
pub async fn tts_voices() -> VoiceList {
    let cfg = crate::config::load();
    let engines: Vec<serde_json::Value> =
        ENGINES.iter().map(|(id, name)| serde_json::json!({ "id": id, "name": name })).collect();
    let fallback_engines = engines.clone();
    let providers = || -> Vec<serde_json::Value> {
        cloud::PROVIDERS
            .iter()
            .map(|(id, name)| {
                // Where a cloud voice's key is made ("Get your API key").
                let keys_page = (*id == elevenlabs::ID).then_some(elevenlabs::KEYS_PAGE);
                serde_json::json!({ "id": id, "name": name, "keysPage": keys_page })
            })
            .collect()
    };
    let provider = if cloud::is_cloud(&cfg.tts_provider) { cfg.tts_provider.clone() } else { cloud::LOCAL.to_string() };
    let fallback_provider = provider.clone();
    tauri::async_runtime::spawn_blocking(move || VoiceList {
        provider,
        providers: providers(),
        engine: cfg.tts_engine.clone(),
        active: chosen(&cfg).0.id().to_string(),
        engines,
        kokoro: EngineVoices { voices: kokoro::voices(), default_voice: Some(kokoro::DEFAULT_VOICE.into()), error: None },
        kokoro_status: registry::status(&registry::root()),
        windows: windows_voices(),
    })
    .await
    .unwrap_or_else(|e| VoiceList {
        provider: fallback_provider,
        providers: providers(),
        engine: DEFAULT_ENGINE.into(),
        active: WINDOWS.into(),
        engines: fallback_engines,
        kokoro: EngineVoices { voices: kokoro::voices(), default_voice: Some(kokoro::DEFAULT_VOICE.into()), error: None },
        kokoro_status: registry::status(&registry::root()),
        windows: EngineVoices { voices: Vec::new(), default_voice: None, error: Some(e.to_string()) },
    })
}

/// Read `text` (markdown) aloud: an answer's ▶.
#[tauri::command]
pub fn tts_speak(text: String, message_id: Option<String>) -> Result<(), String> {
    speak(&text, message_id)
}

/// Settings' ▶ Preview: a short sample in `voice` of `engine` (the one
/// shown, else the configured one) at `rate`, not saved yet.
#[tauri::command]
pub fn tts_preview(voice: String, rate: f32, engine: Option<String>) -> Result<(), String> {
    let cfg = crate::config::load();
    let wanted = engine.unwrap_or_else(|| cfg.tts_engine.clone());
    let e = self::engine(&wanted);
    if wanted == kokoro::ID && e.id() != kokoro::ID {
        return Err(if kokoro::in_build() {
            "Download Kokoro to hear its voices".to_string()
        } else {
            kokoro::NOT_IN_BUILD.to_string()
        });
    }
    say(sentences(PREVIEW_TEXT), Some(PREVIEW_ID.into()), Voicing { engine: e, voice, cloud: None }, rate, cfg.output_device)
}

/// What ▶ Preview says.
const PREVIEW_TEXT: &str = "Hi, I'm the voice that reads Chat's answers. Ask me anything, and I'll say what I found.";

// ---- cloud voices (Settings → Voice Assistant → Voice: ElevenLabs)

/// The key for `provider`: the one Settings has now (`api_key`, maybe not
/// saved yet), else the saved one.
fn cloud_key(provider: &str, api_key: Option<String>) -> Result<elevenlabs::Key, String> {
    if provider != elevenlabs::ID {
        return Err("Yap doesn't know that voice service".into());
    }
    let key = api_key
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .or_else(|| crate::config::load().tts_cloud_keys.get(provider).map(|k| k.trim().to_string()))
        .unwrap_or_default();
    if key.is_empty() {
        return Err(format!("Paste your {} API key first", cloud::name(provider)));
    }
    Ok(elevenlabs::Key::new(&key))
}

/// Settings: the voices of the account (its own first, then ElevenLabs'
/// default ones). An error says why in a few words ("API key not accepted").
#[tauri::command]
pub async fn tts_cloud_voices(provider: String, api_key: Option<String>) -> Result<Vec<elevenlabs::CloudVoice>, String> {
    let key = cloud_key(&provider, api_key)?;
    elevenlabs::list_voices(&key).await.map_err(|f| f.reason())
}

/// Settings: the models that speak (the lowest-latency one first and
/// recommended); Yap's own list when the service's can't be read.
#[tauri::command]
pub async fn tts_cloud_models(provider: String, api_key: Option<String>) -> Result<Vec<elevenlabs::CloudModel>, String> {
    let key = cloud_key(&provider, api_key)?;
    Ok(elevenlabs::list_models(&key).await)
}

/// Settings: characters used this month and the allowance, when the key
/// may read them; else nothing (and Settings says nothing).
#[tauri::command]
pub async fn tts_cloud_usage(provider: String, api_key: Option<String>) -> Option<elevenlabs::Usage> {
    let key = cloud_key(&provider, api_key).ok()?;
    elevenlabs::usage(&key).await
}

/// Settings' ▶ on a cloud voice: its own sample (`preview_url`, fetched
/// now, without the key, played like any answer — Yap's volume, the output
/// device); a voice without one says the preview sentence through the
/// service (`model`, `rate`). Never the local voice: an error is shown.
#[tauri::command]
pub fn tts_cloud_preview(
    provider: String,
    voice: String,
    api_key: Option<String>,
    model: Option<String>,
    rate: Option<f32>,
) -> Result<(), String> {
    let key = cloud_key(&provider, api_key)?;
    if !elevenlabs::valid_voice_id(&voice) {
        return Err("Pick a voice first".into());
    }
    let cfg = crate::config::load();
    let rate = rate.unwrap_or(cfg.tts_rate);
    match elevenlabs::preview_url(&voice) {
        Some(url) => {
            let voicing = Voicing { engine: Arc::new(elevenlabs::Sample { url }), voice, cloud: None };
            say(vec!["(sample)".into()], Some(PREVIEW_ID.into()), voicing, 1.0, cfg.output_device)
        }
        None => {
            let engine = elevenlabs::ElevenLabs::with_key(key, model.as_deref().unwrap_or(""), false);
            let voicing = Voicing { engine: Arc::new(engine), voice, cloud: None };
            say(sentences(PREVIEW_TEXT), Some(PREVIEW_ID.into()), voicing, rate, cfg.output_device)
        }
    }
}

static DOWNLOADING: Mutex<Option<registry::Progress>> = Mutex::new(None);

/// Kokoro's download row: installed, its size, a download in progress.
#[tauri::command]
pub fn tts_kokoro_status() -> serde_json::Value {
    serde_json::json!({
        "status": registry::status(&registry::root()),
        "downloading": DOWNLOADING.lock().unwrap_or_else(|p| p.into_inner()).clone(),
    })
}

/// Download Kokoro (and eSpeak NG): `tts-download-progress` while it goes,
/// `tts-kokoro-changed` at the end, either way.
#[tauri::command]
pub async fn tts_kokoro_download(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Emitter;
    if !kokoro::in_build() {
        return Err(kokoro::NOT_IN_BUILD.to_string());
    }
    let ticket = {
        let mut d = DOWNLOADING.lock().unwrap_or_else(|p| p.into_inner());
        if d.is_some() {
            return Err("Kokoro is already downloading".into());
        }
        let ticket = crate::shell::download::Ticket::begin(registry::CANCEL_ID).ok_or("Kokoro is already downloading")?;
        *d = Some(registry::Progress { percent: 0, downloaded_bytes: 0, total_bytes: 0, ..Default::default() });
        ticket
    };
    let progress_app = app.clone();
    let on_progress = move |p: registry::Progress| {
        *DOWNLOADING.lock().unwrap_or_else(|p| p.into_inner()) = Some(p.clone());
        let _ = progress_app.emit("tts-download-progress", &p);
    };
    let result = registry::install(&registry::root(), &ticket, &on_progress).await;
    *DOWNLOADING.lock().unwrap_or_else(|p| p.into_inner()) = None;
    if let Err(e) = &result {
        tracing::warn!("Kokoro: download failed: {e}");
    }
    let _ = app.emit("tts-kokoro-changed", ());
    result
}

/// Delete Kokoro: what's being said stops, the model unloads.
#[tauri::command]
pub fn tts_kokoro_delete(app: tauri::AppHandle) -> Result<(), String> {
    use tauri::Emitter;
    if DOWNLOADING.lock().unwrap_or_else(|p| p.into_inner()).is_some() {
        return Err("Wait for the download to finish first.".into());
    }
    stop();
    // The model file can't go while it's loaded.
    kokoro::unload(Duration::from_secs(2));
    registry::delete(&registry::root())?;
    let _ = app.emit("tts-kokoro-changed", ());
    Ok(())
}

/// Stop speaking. `true` if something was being said.
#[tauri::command]
pub fn tts_stop() -> bool {
    stop()
}

#[tauri::command]
pub fn tts_state() -> TtsState {
    speaker().map(|s| s.state()).unwrap_or_default()
}

/// The voice's volume from a slider (the Chat view's; Settings' while it
/// moves): applied at once, the sentence being said included. `save` also
/// keeps it (config.json) and tells Settings' copy (`yap-voice-volume`), so
/// its auto-save can't put the old one back. Returns the volume set.
#[tauri::command]
pub fn tts_volume_set(app: tauri::AppHandle, volume: f32, save: Option<bool>) -> Result<f32, String> {
    use tauri::Emitter;
    set_volume(volume);
    let v = playback::volume();
    if save.unwrap_or(false) {
        // In one step: never over a change saved meanwhile.
        crate::shell::settings::change_config(&app, |cfg| {
            cfg.tts_volume = v;
            Ok(())
        })?;
        let _ = app.emit("yap-voice-volume", serde_json::json!({ "volume": v }));
    }
    Ok(v)
}

/// The Chat view's 🔊 / 🔇: "Speak replies" on or off, saved the way
/// Settings saves, then `yap-voice-replies {on}` so Settings' copy adopts it
/// (its next auto-save keeps it). Off stops what's being said.
#[tauri::command]
pub fn voice_replies_set(app: tauri::AppHandle, on: bool) -> Result<bool, String> {
    use tauri::Emitter;
    crate::shell::settings::change_config(&app, |cfg| {
        cfg.speak_replies = on;
        Ok(())
    })?;
    if !on {
        stop();
    }
    let _ = app.emit("yap-voice-replies", serde_json::json!({ "on": on }));
    Ok(on)
}

/// What test mode would have said (debug builds): each utterance, in order.
#[cfg(debug_assertions)]
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Spoken {
    pub message_id: Option<String>,
    pub text: String,
    pub sentences: Vec<String>,
    /// "kokoro" | "windows".
    pub engine: String,
    pub voice: String,
    pub rate: f32,
    /// The volume setting it played at (0.0–1.0).
    pub volume: f32,
    /// The local voice took over from a cloud voice that failed with this
    /// error (these are the sentences it read).
    pub fallback: Option<String>,
    /// Said as the answer was written ([`speak_streamed`]).
    pub streamed: bool,
}

#[cfg(debug_assertions)]
static SPOKEN: Mutex<Vec<Spoken>> = Mutex::new(Vec::new());

#[cfg(debug_assertions)]
fn spoken() -> std::sync::MutexGuard<'static, Vec<Spoken>> {
    SPOKEN.lock().unwrap_or_else(|p| p.into_inner())
}

/// Debug-only, test mode: everything Yap would have said, oldest first;
/// `clear` empties the record after reading it, and forgets any cloud
/// voice's failure (`cloud::Health`), so each test starts afresh.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn tts_e2e_spoken(clear: Option<bool>) -> Result<Vec<Spoken>, String> {
    if !crate::shell::e2e::active() {
        return Err("tts_e2e_spoken is for test runs".into());
    }
    let mut s = spoken();
    let out = s.clone();
    if clear.unwrap_or(false) {
        s.clear();
        cloud::reset();
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn when_to_speak() {
        // Off: never.
        assert!(!auto_speak(false, "always", true, false, false));
        // "Only when I asked by voice" (the default).
        assert!(auto_speak(true, "voice", true, false, false));
        assert!(!auto_speak(true, "voice", false, false, false));
        // Always.
        assert!(auto_speak(true, "always", false, false, false));
        // Never over a dictation or a meeting.
        assert!(!auto_speak(true, "always", true, true, false));
        assert!(!auto_speak(true, "always", true, false, true));
        // Hands-free sending: part of spoken replies; "press enter" always sends.
        assert!(sends_by_itself(true, true, false));
        assert!(!sends_by_itself(false, true, false));
        assert!(!sends_by_itself(true, false, false));
        assert!(sends_by_itself(false, false, true));
    }

    #[test]
    fn wav_round_trip() {
        let mut wav = Vec::new();
        let samples: [i16; 4] = [0, 16_384, -16_384, 32_767];
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data.len() as u32 + 10).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        // An odd-length chunk before fmt (padded), as some writers add.
        wav.extend_from_slice(b"LIST");
        wav.extend_from_slice(&1u32.to_le_bytes());
        wav.extend_from_slice(&[0, 0]);
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&22_050u32.to_le_bytes());
        wav.extend_from_slice(&44_100u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        let pcm = parse_wav(&wav).unwrap();
        assert_eq!((pcm.sample_rate, pcm.channels), (22_050, 1));
        assert_eq!(pcm.samples, vec![0.0, 0.5, -0.5, 32_767.0 / 32_768.0]);
        assert!(parse_wav(b"not a wav file at all").is_err());
    }

    /// The real thing on this PC, without a sound: every voice Windows
    /// gives apps, then the first one saying a sentence into a WAV in a
    /// temp folder, checked for real, plausibly long audio. Run with
    /// `cargo test tts::tests::windows_voices_on_this_pc -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn windows_voices_on_this_pc() {
        let engine = WindowsVoices;
        let voices = engine.voices().expect("the voices");
        println!("{} voices (default {:?}):", voices.len(), engine.default_voice());
        for v in &voices {
            println!("  {} | {} | {} | {}", v.name, v.language, v.gender, v.id);
        }
        let first = voices.first().expect("at least one voice");
        let mut synth = engine.open(&first.id, 1.0).expect("a synthesiser");
        let text = "The quick brown fox jumps over the lazy dog.";
        let pcm = synth.synthesize(text).expect("audio");
        let secs = pcm.duration().as_secs_f32();
        let peak = pcm.samples.iter().fold(0f32, |m, s| m.max(s.abs()));
        let rms = (pcm.samples.iter().map(|s| s * s).sum::<f32>() / pcm.samples.len().max(1) as f32).sqrt();
        println!("{}: {} Hz, {} ch, {:.2} s, peak {:.3}, rms {:.4}", first.name, pcm.sample_rate, pcm.channels, secs, peak, rms);
        assert!((1.0..8.0).contains(&secs), "{secs} s for one sentence");
        assert!(peak > 0.05 && rms > 0.005, "silent audio: peak {peak}, rms {rms}");
        // Faster is shorter.
        let fast = engine.open(&first.id, 2.0).unwrap().synthesize(text).unwrap().duration().as_secs_f32();
        assert!(fast < secs * 0.8, "2× took {fast} s against {secs} s");
        // The speaker's worker opens rodio first, and cpal makes its thread
        // a single-threaded apartment: synthesis must still finish there.
        let (tx, rx) = std::sync::mpsc::channel();
        let id = first.id.clone();
        std::thread::spawn(move || {
            #[cfg(windows)]
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
            }
            let out = WindowsVoices.open(&id, 1.0).and_then(|mut s| s.synthesize("Said on a single-threaded apartment."));
            let _ = tx.send(out.map(|p| p.duration()));
        });
        let sta = rx.recv_timeout(Duration::from_secs(15)).expect("synthesis on an STA thread hung").expect("STA audio");
        println!("on an STA thread: {:.2} s", sta.as_secs_f32());
        // Into a WAV, to listen to by hand if wanted.
        let dir = std::env::temp_dir().join("yap-tts-check");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("windows-voice.wav");
        std::fs::write(&path, wav_bytes(&pcm)).unwrap();
        assert_eq!(parse_wav(&std::fs::read(&path).unwrap()).unwrap().samples.len(), pcm.samples.len());
        println!("wrote {}", path.display());
    }

    /// Windows' word cues on this PC, without a sound: a sentence made into
    /// memory, each word's start where the cue says, in order and inside
    /// the audio — then the words as the reading card would light them.
    /// `cargo test tts::tests::windows_word_cues_on_this_pc -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn windows_word_cues_on_this_pc() {
        let engine = WindowsVoices;
        let text = "Reading aloud helps, and it costs twelve pounds.";
        let mut synth = engine.open("", 1.0).expect("a synthesiser");
        let pcm = synth.synthesize(text).expect("audio");
        let cues = synth.marks().expect("word cues");
        let spans = marks::words(text);
        let chars: Vec<char> = text.chars().collect();
        for c in &cues {
            let word: String = chars[c.at..].iter().take_while(|ch| ch.is_alphanumeric()).collect();
            println!("{:>6.0} ms  {word}", c.start.as_secs_f64() * 1000.0);
        }
        let starts = marks::from_chars(&spans, &cues).expect("a time for most words");
        assert_eq!(starts.len(), spans.len());
        assert!(starts.windows(2).all(|w| w[0] <= w[1]), "{starts:?}");
        assert!(*starts.last().unwrap() < pcm.duration(), "the last word starts inside the audio");
        assert!(cues.len() >= spans.len() - 1, "{} cues for {} words", cues.len(), spans.len());
        // The audio's own shape against the cues: Windows' times are the real ones.
        let (estimated, how) = marks::from_audio(text, &spans, &pcm.samples, pcm.sample_rate, pcm.channels);
        println!("audio {:.2} s; from the audio alone ({how:?}):", pcm.duration().as_secs_f32());
        for (i, (cue, est)) in starts.iter().zip(&estimated).enumerate() {
            let w: String = chars[spans[i].0..spans[i].1].iter().collect();
            println!("  {w:<10} cue {:>5.0} ms   estimate {:>5.0} ms", cue.as_secs_f64() * 1000.0, est.as_secs_f64() * 1000.0);
        }
    }

    fn wav_bytes(pcm: &Pcm) -> Vec<u8> {
        let data: Vec<u8> = pcm.samples.iter().flat_map(|s| ((s.clamp(-1.0, 1.0) * 32_767.0) as i16).to_le_bytes()).collect();
        let mut w = Vec::new();
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        w.extend_from_slice(&1u16.to_le_bytes());
        w.extend_from_slice(&pcm.channels.to_le_bytes());
        w.extend_from_slice(&pcm.sample_rate.to_le_bytes());
        w.extend_from_slice(&(pcm.sample_rate * u32::from(pcm.channels) * 2).to_le_bytes());
        w.extend_from_slice(&(pcm.channels * 2).to_le_bytes());
        w.extend_from_slice(&16u16.to_le_bytes());
        w.extend_from_slice(b"data");
        w.extend_from_slice(&(data.len() as u32).to_le_bytes());
        w.extend_from_slice(&data);
        w
    }
}

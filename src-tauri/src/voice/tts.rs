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
//! - **Text prep** ([`utterances`], pure): an answer's markdown as something
//!   to say — marks out, a code block as "(code block)", a link as its text,
//!   a bare URL as its domain, headings and bullets each their own sentence,
//!   emoji out — then numbers, money, dates, times, units and abbreviations
//!   written as words (`tts/normalize.rs`, UK style), then cut into
//!   sentences (abbreviations and initials kept together, long ones cut at a
//!   comma).
//! - **The speaker** ([`Speaker`]): one utterance at a time, queued by
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

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::time::{Duration, Instant};

use regex::Regex;
use serde::Serialize;

pub mod cloud;
pub mod elevenlabs;
// The G2P only runs in builds with Kokoro's engine; it's tested in all.
#[cfg_attr(not(feature = "engines"), allow(dead_code))]
mod g2p;
pub mod kokoro;
pub mod marks;
mod normalize;
pub mod registry;

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
    if crate::e2e::active() {
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
        if kokoro::is_voice(&cfg.tts_voice) {
            cfg.tts_voice.clone()
        } else {
            kokoro::DEFAULT_VOICE.to_string()
        }
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

// -------------------------------------------------------------- text prep

static FENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(```|~~~)").unwrap());
static RULE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*([-*_])(\s*[-*_]){2,}\s*$").unwrap());
static TABLE_RULE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$").unwrap());
static HEADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*#{1,6}\s+").unwrap());
static QUOTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(>\s?)+").unwrap());
static TASK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(?:[-*+•]\s+)?\[([ xX])\]\s+").unwrap());
static BOX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*([☐☑✅✔✓])\s*").unwrap());
static BULLET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*(?:[-*+•–]|\d{1,3}[.)])\s+").unwrap());
static IMAGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"!\[([^\]]*)\]\([^)]*\)").unwrap());
static LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[([^\]]+)\]\(([^)\s]+)[^)]*\)").unwrap());
static ANGLE_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<((?:https?://|www\.)[^>\s]+)>").unwrap());
static URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(?:https?://|www\.)[^\s<>()\[\]]+").unwrap());
static CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`+([^`]*)`+").unwrap());
static BOLD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*\*([^*]+)\*\*|__([^_]+)__").unwrap());
static ITALIC_STAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*([^*\s](?:[^*]*[^*\s])?)\*").unwrap());
static ITALIC_UNDER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(^|[^\w])_([^_\s](?:[^_]*[^_\s])?)_($|[^\w])").unwrap());
static STRIKE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"~~([^~]+)~~").unwrap());
static HTML: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"</?[a-zA-Z][a-zA-Z0-9]*(?:\s[^<>]*)?/?>").unwrap());
static ARROW: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s*(?:→|⟶|->|=>)\s*").unwrap());
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());
/// A web answer's citation markers ("[1]", "[2, 3]", "[1][4]", gpt-oss's
/// "【2†L6-L10】"): read as nothing.
static CITE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*(?:\[\d{1,2}(?:\s*[,–-]\s*\d{1,2})*\]|【[^】]{0,40}】)").unwrap());

/// A URL as it's worth hearing: its domain ("https://www.github.com/a/b" →
/// "github.com").
fn domain(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = host.rsplit('@').next().unwrap_or(host);
    let host = host.split(':').next().unwrap_or(host);
    host.trim_start_matches("www.").to_ascii_lowercase()
}

/// Characters a voice reads badly or not at all: emoji, pictographs and
/// symbols, the joiners and variation selectors that build them, arrows.
fn is_unspoken(c: char) -> bool {
    matches!(c as u32,
        0x1F000..=0x1FAFF   // emoji, pictographs, flags' letters
        | 0x2600..=0x27BF   // misc symbols, dingbats (✓ ☐ ★ ☎)
        | 0x2B00..=0x2BFF   // ⭐ ⬆ ⭕
        | 0x2190..=0x21FF   // arrows
        | 0x2300..=0x23FF   // ⌚ ⏰ ⏸
        | 0xFE00..=0xFE0F   // variation selectors
        | 0x200D | 0x20E3   // zero-width joiner, keycap
        | 0xE0020..=0xE007F // tags
        | 0x25A0..=0x25FF   // geometric shapes (■ ▶ ●)
    )
}

/// One line's inline markdown out: images by their description, links by
/// their text, bare URLs by their domain, code by its text, emphasis marks,
/// HTML tags and emoji gone, arrows read as "to".
fn inline_text(line: &str) -> String {
    let s = IMAGE.replace_all(line, "$1");
    // A link whose text is its address: say the domain.
    let s = LINK.replace_all(&s, |c: &regex::Captures| {
        let text = c[1].trim();
        if text.contains("://") || text.starts_with("www.") {
            domain(text)
        } else {
            text.to_string()
        }
    });
    let s = ANGLE_URL.replace_all(&s, |c: &regex::Captures| domain(&c[1]));
    let s = URL.replace_all(&s, |c: &regex::Captures| {
        let url = c[0].trim_end_matches(['.', ',', ';', ':', '!', '?', '\'', '"']);
        let tail = &c[0][url.len()..];
        format!("{}{}", domain(url), tail)
    });
    let s = CITE.replace_all(&s, "");
    let s = CODE.replace_all(&s, "$1");
    let s = BOLD.replace_all(&s, |c: &regex::Captures| c.get(1).or(c.get(2)).map_or("", |m| m.as_str()).to_string());
    let s = ITALIC_STAR.replace_all(&s, "$1");
    let s = ITALIC_UNDER.replace_all(&s, "$1$2$3");
    let s = STRIKE.replace_all(&s, "$1");
    let s = HTML.replace_all(&s, " ");
    let s = ARROW.replace_all(&s, " to ");
    let s: String = s.chars().map(|c| if c == '•' { ',' } else { c }).filter(|&c| !is_unspoken(c)).collect();
    SPACES.replace_all(&s, " ").trim().to_string()
}

/// A block that doesn't end like a sentence gets a full stop, so a heading
/// or a bullet is its own sentence with its own pause.
fn end_sentence(mut s: String) -> String {
    if s.chars().last().is_some_and(|c| !matches!(c, '.' | '!' | '?' | '…' | ':' | ';')) {
        s.push('.');
    }
    s
}

/// An answer's markdown as blocks to say: one per heading, list item,
/// table row or paragraph line, each ending like a sentence; a fenced code
/// block as "(code block)". Pure.
pub fn speakable_blocks(md: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_code = false;
    let mut paragraph = String::new();
    let flush = |paragraph: &mut String, out: &mut Vec<String>| {
        let text = std::mem::take(paragraph);
        let text = text.trim();
        if !text.is_empty() {
            out.push(end_sentence(text.to_string()));
        }
    };
    for raw in md.lines() {
        if FENCE.is_match(raw) {
            if !in_code {
                flush(&mut paragraph, &mut out);
                out.push("(code block)".to_string());
            }
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        if raw.trim().is_empty() {
            flush(&mut paragraph, &mut out);
            continue;
        }
        if RULE.is_match(raw) || TABLE_RULE.is_match(raw) {
            flush(&mut paragraph, &mut out);
            continue;
        }
        let line = QUOTE.replace(raw, "");
        // A table row: its cells, one after another.
        if line.trim_start().starts_with('|') {
            flush(&mut paragraph, &mut out);
            let cells: Vec<String> =
                line.trim().trim_matches('|').split('|').map(inline_text).filter(|c| !c.is_empty()).collect();
            // The |---|:---:| line under the header says nothing.
            if cells.iter().all(|c| c.chars().all(|ch| matches!(ch, '-' | ':' | ' '))) {
                continue;
            }
            if !cells.is_empty() {
                out.push(end_sentence(cells.join(", ")));
            }
            continue;
        }
        // A task ("- [x] …", or ☐ / ☑ as Copy text writes them): a list
        // item, a done one said so.
        let (prefix, line, task) = if let Some(m) = TASK.captures(&line) {
            let done = !m[1].trim().is_empty();
            (if done { "Done: " } else { "" }, TASK.replace(&line, "").into_owned(), true)
        } else if let Some(m) = BOX.captures(&line) {
            let done = &m[1] != "☐";
            (if done { "Done: " } else { "" }, BOX.replace(&line, "").into_owned(), true)
        } else {
            ("", line.into_owned(), false)
        };
        let block = task || HEADING.is_match(&line) || BULLET.is_match(&line);
        let line = HEADING.replace(&line, "");
        let line = BULLET.replace(&line, "");
        let text = inline_text(&line);
        if text.is_empty() {
            continue;
        }
        if block {
            flush(&mut paragraph, &mut out);
            out.push(end_sentence(format!("{prefix}{text}")));
        } else {
            // A paragraph's lines run on, as they render.
            if !paragraph.is_empty() {
                paragraph.push(' ');
            }
            paragraph.push_str(&text);
        }
    }
    flush(&mut paragraph, &mut out);
    out
}

/// `text` cut into sentences: after `.`, `!`, `?` or `…` (a run of them,
/// with any closing quote or bracket) and a space — never after the full
/// stop of an abbreviation ("e.g.", "Dr.", "etc.") or an initial ("J.").
/// Pure.
pub fn sentences(text: &str) -> Vec<String> {
    let text = text.trim();
    let mut out = Vec::new();
    let mut start = 0;
    let mut iter = text.char_indices().peekable();
    while let Some((i, c)) = iter.next() {
        if !matches!(c, '.' | '?' | '!' | '…') {
            continue;
        }
        let mut single_dot = c == '.';
        let mut end = i + c.len_utf8();
        while let Some(&(j, n)) = iter.peek() {
            if matches!(n, '.' | '?' | '!' | '…') {
                single_dot = false;
            } else if !matches!(n, '"' | '\'' | '”' | '’' | ')' | ']') {
                break;
            }
            end = j + n.len_utf8();
            iter.next();
        }
        let at_space = iter.peek().is_some_and(|&(_, n)| n.is_whitespace());
        if !at_space {
            continue;
        }
        let word = crate::app_kind::word_before(text, i);
        let initial = word.chars().count() == 1 && word.chars().all(char::is_uppercase);
        if single_dot && (crate::app_kind::is_abbreviation(word) || initial) {
            continue;
        }
        let s = text[start..end].trim();
        if !s.is_empty() {
            out.push(s.to_string());
        }
        start = end;
    }
    let rest = text[start..].trim();
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
    out.into_iter().flat_map(|s| cut_long(&s)).collect()
}

/// A sentence over [`LONG_SENTENCE`] characters, cut after a comma, a
/// semicolon or a dash (else a space) so each piece starts soon.
fn cut_long(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s.trim();
    while rest.chars().count() > LONG_SENTENCE {
        let limit = rest.char_indices().nth(LONG_SENTENCE).map_or(rest.len(), |(i, _)| i);
        let head = &rest[..limit];
        let floor = head.char_indices().nth(60).map_or(0, |(i, _)| i);
        let cut = [", ", "; ", " – ", " — ", ": "]
            .iter()
            .filter_map(|p| head.rfind(p).map(|i| i + p.trim_end().len()))
            .filter(|&i| i > floor)
            .max()
            .or_else(|| head.rfind(' ').filter(|&i| i > floor))
            .unwrap_or(limit);
        out.push(rest[..cut].trim().to_string());
        rest = rest[cut..].trim_start();
    }
    if !rest.is_empty() {
        out.push(rest.to_string());
    }
    out
}

/// The short answer: the first `n` sentences of an answer's first block,
/// without markdown or citation markers — what the Voice Assistant says (and
/// its card shows) when the details and sources are in Chat. Pure.
pub fn brief(md: &str, n: usize) -> String {
    let first = speakable_blocks(md).into_iter().find(|b| b.chars().any(char::is_alphanumeric)).unwrap_or_default();
    sentences(&first).into_iter().take(n.max(1)).collect::<Vec<_>>().join(" ")
}

/// What to say for an answer: its [`speakable_blocks`], each written for
/// the ear (`normalize::for_speech`) and cut into [`sentences`]. Empty when
/// there's nothing to say. Pure.
pub fn utterances(md: &str) -> Vec<String> {
    let md: String = md.chars().take(MAX_CHARS).collect();
    speakable_blocks(&md).iter().flat_map(|b| sentences(&normalize::for_speech(b))).collect()
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

// ------------------------------------------------------------- speaker

/// What's being said: `yap-tts-state`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsState {
    pub speaking: bool,
    /// The answer it's reading (the page's id for it; [`PREVIEW_ID`] for
    /// Settings' sample). On `speaking: false`, the one that stopped.
    pub message_id: Option<String>,
    /// Why it stopped early, when it failed (no voice, no output device…).
    pub error: Option<String>,
}

/// Where the audio goes: rodio's sink, or a simulated clock in test mode.
pub trait Playback: Send + Sync {
    fn append(&self, pcm: Pcm);
    /// Append audio of a read-aloud's sentence (`tag`): the playback then
    /// knows which sentence is playing, and how far in ([`position`]).
    ///
    /// [`position`]: Playback::position
    fn append_tagged(&self, pcm: Pcm, tag: Tag) {
        let _ = tag;
        self.append(pcm);
    }
    /// Empty the queue and go quiet, now.
    fn stop(&self);
    /// Played to the end (or stopped). Never while paused mid-way.
    fn done(&self) -> bool;
    /// Hold where it is (a read-aloud's ⏸), and go on from there.
    fn pause(&self) {}
    fn resume(&self) {}
    fn paused(&self) -> bool {
        false
    }
    /// The tagged sentence playing now (or last), and how far into its
    /// audio. `None` before any tagged audio has played.
    fn position(&self) -> Option<(usize, Duration)> {
        None
    }
}

/// Which read-aloud sentence a piece of audio is (`Track`'s index), and
/// where it starts in that sentence's audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tag {
    pub sentence: usize,
    pub offset: Duration,
}

/// A position as one atomic word: the sentence + 1 above bit 40, the
/// nanoseconds into it below (≤ 18 minutes a sentence). 0 = none yet.
const POS_SHIFT: u32 = 40;
const POS_NS: u64 = (1 << POS_SHIFT) - 1;

fn pack_position(sentence: usize, ns: u64) -> u64 {
    ((sentence as u64 + 1) << POS_SHIFT) | ns.min(POS_NS)
}

fn unpack_position(v: u64) -> Option<(usize, Duration)> {
    (v != 0).then(|| (((v >> POS_SHIFT) - 1) as usize, Duration::from_nanos(v & POS_NS)))
}

/// A sentence's audio on its way to the output, saying how far it's got:
/// every sample the output takes moves [`RodioPlayback`]'s position on
/// (paused, the output takes none).
struct Tagged<S> {
    inner: S,
    pos: Arc<AtomicU64>,
    sentence: usize,
    base_ns: f64,
    ns_per_sample: f64,
    taken: u64,
}

impl<S: Iterator<Item = f32>> Iterator for Tagged<S> {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let s = self.inner.next()?;
        self.taken += 1;
        let ns = self.base_ns + self.taken as f64 * self.ns_per_sample;
        self.pos.store(pack_position(self.sentence, ns as u64), Ordering::Relaxed);
        Some(s)
    }
}

impl<S: rodio::Source<Item = f32>> rodio::Source for Tagged<S> {
    fn current_frame_len(&self) -> Option<usize> {
        self.inner.current_frame_len()
    }
    fn channels(&self) -> u16 {
        self.inner.channels()
    }
    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}

/// A playback, plus whatever must stay alive (on the worker's thread) for it
/// to play: rodio's output stream.
pub struct Opened {
    pub play: Arc<dyn Playback>,
    pub keep: Option<Box<dyn std::any::Any>>,
}

/// Opens the output, on the worker's thread.
pub type Opener = Box<dyn FnOnce() -> Result<Opened, String> + Send>;

// ---- The voice's volume (the maintainer, 2026-10-20: "so loud, even at 1% on the
// volume mixer"). Yap scales its own voice, so it's quiet enough whatever
// the Windows mixer says; a change applies at once, mid-sentence too.

/// The volume the voice plays at (0.0–1.0, `config.tts_volume`), as f32 bits.
static VOLUME: AtomicU32 = AtomicU32::new(0x3F00_0000); // 0.5
/// The sink playing now, to change its volume while it speaks.
static LIVE_SINK: Mutex<Option<std::sync::Weak<rodio::Sink>>> = Mutex::new(None);

/// A volume setting as the gain the samples get: squared, so the low end is
/// really quiet — half is a quarter of the voice's full loudness (−12 dB),
/// 10 % a hundredth (−40 dB), 100 % the voice as it comes. Pure.
pub fn gain(volume: f32) -> f32 {
    let v = if volume.is_finite() { volume.clamp(0.0, 1.0) } else { 0.5 };
    v * v
}

/// The volume setting now (0.0–1.0).
pub fn volume() -> f32 {
    f32::from_bits(VOLUME.load(Ordering::Relaxed))
}

/// Set the voice's volume (0.0–1.0): the sentence playing now included.
/// From every config save (`commands::save_config`), startup, and the
/// Chat view's slider while it moves (`tts_volume_set`).
pub fn set_volume(volume: f32) {
    let v = if volume.is_finite() { volume.clamp(0.0, 1.0) } else { 0.5 };
    VOLUME.store(v.to_bits(), Ordering::Relaxed);
    let sink = LIVE_SINK.lock().unwrap_or_else(|p| p.into_inner()).as_ref().and_then(std::sync::Weak::upgrade);
    if let Some(sink) = sink {
        sink.set_volume(gain(v));
    }
}

struct RodioPlayback {
    sink: Arc<rodio::Sink>,
    /// [`pack_position`] of the tagged sample the output took last.
    pos: Arc<AtomicU64>,
}

impl Playback for RodioPlayback {
    fn append(&self, pcm: Pcm) {
        self.sink.append(rodio::buffer::SamplesBuffer::new(pcm.channels, pcm.sample_rate, pcm.samples));
    }
    fn append_tagged(&self, pcm: Pcm, tag: Tag) {
        let ns_per_sample = 1e9 / (f64::from(pcm.sample_rate.max(1)) * f64::from(pcm.channels.max(1)));
        self.sink.append(Tagged {
            inner: rodio::buffer::SamplesBuffer::new(pcm.channels, pcm.sample_rate, pcm.samples),
            pos: Arc::clone(&self.pos),
            sentence: tag.sentence,
            base_ns: tag.offset.as_nanos() as f64,
            ns_per_sample,
            taken: 0,
        });
    }
    fn stop(&self) {
        self.sink.stop();
    }
    fn done(&self) -> bool {
        self.sink.empty()
    }
    fn pause(&self) {
        self.sink.pause();
    }
    fn resume(&self) {
        self.sink.play();
    }
    fn paused(&self) -> bool {
        self.sink.is_paused()
    }
    fn position(&self) -> Option<(usize, Duration)> {
        unpack_position(self.pos.load(Ordering::Relaxed))
    }
}

/// The configured output device (`config.output_device`, else the default),
/// as the chimes use it.
fn rodio_opener(device: Option<String>) -> Opener {
    Box::new(move || {
        let (stream, handle) =
            crate::sound::open_output(device.as_deref()).ok_or("No sound output device to speak on")?;
        let sink = Arc::new(rodio::Sink::try_new(&handle).map_err(|e| format!("Couldn't play the voice: {e}"))?);
        sink.set_volume(gain(volume()));
        *LIVE_SINK.lock().unwrap_or_else(|p| p.into_inner()) = Some(Arc::downgrade(&sink));
        Ok(Opened {
            play: Arc::new(RodioPlayback { sink, pos: Arc::new(AtomicU64::new(0)) }),
            keep: Some(Box::new(stream)),
        })
    })
}

/// Plays nothing: each sentence takes its length on the clock — a clock
/// that stands still while paused. (Test mode and the unit tests.)
#[derive(Default)]
pub struct SimPlayback {
    clock: Mutex<SimClock>,
    stopped: std::sync::atomic::AtomicBool,
    appended: AtomicU64,
}

/// [`SimPlayback`]'s clock: time played so far ("virtual" time: the pauses
/// left out), when the queue runs out, and what's queued (the last few).
#[derive(Default)]
struct SimClock {
    origin: Option<Instant>,
    paused_at: Option<Instant>,
    /// All the time spent paused.
    held: Duration,
    ends: Option<Duration>,
    queued: std::collections::VecDeque<(Duration, Duration, Option<Tag>)>,
}

impl SimClock {
    fn now(&mut self) -> Duration {
        let origin = *self.origin.get_or_insert_with(Instant::now);
        let at = self.paused_at.unwrap_or_else(Instant::now);
        at.saturating_duration_since(origin).saturating_sub(self.held)
    }
}

impl SimPlayback {
    fn clock(&self) -> std::sync::MutexGuard<'_, SimClock> {
        self.clock.lock().unwrap_or_else(|p| p.into_inner())
    }
    fn push(&self, length: Duration, tag: Option<Tag>) {
        let mut c = self.clock();
        let now = c.now();
        let from = c.ends.filter(|e| *e > now).unwrap_or(now);
        c.ends = Some(from + length);
        c.queued.push_back((from, length, tag));
        while c.queued.len() > 64 {
            c.queued.pop_front();
        }
        drop(c);
        self.appended.fetch_add(1, Ordering::SeqCst);
    }
}

impl Playback for SimPlayback {
    fn append(&self, pcm: Pcm) {
        self.push(pcm.duration(), None);
    }
    fn append_tagged(&self, pcm: Pcm, tag: Tag) {
        self.push(pcm.duration(), Some(tag));
    }
    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
    }
    fn done(&self) -> bool {
        if self.stopped.load(Ordering::SeqCst) {
            return true;
        }
        let mut c = self.clock();
        if c.paused_at.is_some() {
            return false;
        }
        let now = c.now();
        c.ends.is_none_or(|e| now >= e)
    }
    fn pause(&self) {
        let mut c = self.clock();
        if c.paused_at.is_none() {
            c.now();
            c.paused_at = Some(Instant::now());
        }
    }
    fn resume(&self) {
        let mut c = self.clock();
        if let Some(p) = c.paused_at.take() {
            c.held += p.elapsed();
        }
    }
    fn paused(&self) -> bool {
        self.clock().paused_at.is_some()
    }
    fn position(&self) -> Option<(usize, Duration)> {
        let mut c = self.clock();
        let now = c.now();
        c.queued
            .iter()
            .rev()
            .filter_map(|&(from, len, tag)| tag.map(|t| (from, len, t)))
            .find(|&(from, _, _)| from <= now)
            .map(|(from, len, t)| (t.sentence, t.offset + (now - from).min(len)))
    }
}

fn sim_opener() -> Opener {
    Box::new(|| Ok(Opened { play: Arc::new(SimPlayback::default()), keep: None }))
}

/// One utterance to say.
pub struct Job {
    pub message_id: Option<String>,
    pub sentences: Vec<String>,
    pub engine: Arc<dyn Engine>,
    pub voice: String,
    pub rate: f32,
    /// For a cloud voice: what happens when it fails ([`Fallback`]).
    pub fallback: Option<Fallback>,
    /// A read-aloud (`read_aloud.rs`): `sentences` are `track`'s from
    /// `first` on, as shown — each is said as [`said_text`] writes it, made
    /// only once the one before it plays, and its words' times kept on the
    /// track for the reading card.
    pub track: Option<Arc<Track>>,
    pub first: usize,
    /// A streamed answer (the Voice Assistant's, `assistant.rs`): more
    /// sentences come through it as the answer is written, after
    /// `sentences` (which may be empty), until it's closed.
    pub feed: Option<Arc<Feed>>,
}

/// How long the voice waits for a streamed answer's next sentence before it
/// gives up ([`Feed`]; Chat's own timeout is 2 minutes a call).
const FEED_PATIENCE: Duration = Duration::from_secs(180);

/// **Sentences handed to the voice as an answer is written** (the Voice
/// Assistant speaks a streamed answer from its first sentence, `stream.rs`):
/// [`speak_streamed`] starts the voice with none, [`Feed::push`] adds
/// sentences, [`Feed::close`] says there are no more. The voice waits for
/// the next sentence while the feed is open; a stop (■, a dictation's
/// barge-in, a newer answer) ends it, and pushing after that says so.
#[derive(Default)]
pub struct Feed {
    state: Mutex<FeedState>,
    /// The speaker and its job it feeds (none: not started).
    voice: std::sync::OnceLock<(std::sync::Weak<Speaker>, u64)>,
}

#[derive(Default)]
struct FeedState {
    queued: Vec<String>,
    closed: bool,
    /// Everything pushed (what test mode records).
    all: Vec<String>,
    #[cfg(debug_assertions)]
    record: Option<Spoken>,
}

impl Feed {
    /// More sentences (as `utterances` writes them). `false`: the voice has
    /// stopped, so they won't be said.
    pub fn push(&self, sentences: Vec<String>) -> bool {
        if !self.live() {
            return false;
        }
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if s.closed {
            return false;
        }
        s.all.extend(sentences.iter().cloned());
        s.queued.extend(sentences);
        true
    }

    /// No more sentences: the voice ends after the last one.
    pub fn close(&self) {
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if s.closed {
            return;
        }
        s.closed = true;
        #[cfg(debug_assertions)]
        if let Some(mut rec) = s.record.take() {
            if !s.all.is_empty() {
                rec.text = s.all.join(" ");
                rec.sentences = s.all.clone();
                spoken().push(rec);
            }
        }
    }

    /// The voice it feeds is still going.
    pub fn live(&self) -> bool {
        self.voice.get().is_some_and(|(s, g)| s.upgrade().is_some_and(|s| s.live(*g)))
    }

    /// It feeds `speaker`'s job `generation` (started by [`Speaker::start`]).
    fn attach(&self, speaker: &Arc<Speaker>, generation: u64) {
        let _ = self.voice.set((Arc::downgrade(speaker), generation));
    }

    /// What's queued now, and whether it's closed.
    fn take(&self) -> (Vec<String>, bool) {
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        (std::mem::take(&mut s.queued), s.closed)
    }
}

/// What's said for a sentence shown on the reading card: its markdown, links
/// and emoji out ([`inline_text`]), written for the ear (`normalize.rs`).
pub fn said_text(shown: &str) -> String {
    normalize::for_speech(&inline_text(shown))
}

/// A read-aloud's sentences as shown (`read_aloud.rs` splits the text), and
/// for each one made so far, when its words start in its audio — the
/// speaker writes, the reading card reads.
pub struct Track {
    shown: Vec<String>,
    made: Mutex<std::collections::HashMap<usize, Made>>,
    /// Sentences made so far (a long text is made a sentence ahead of the
    /// voice, never all at once).
    made_count: std::sync::atomic::AtomicUsize,
}

/// One sentence, made: when its shown words start (word index, from the
/// start of its audio), how that was found, and by which engine.
#[derive(Debug, Clone)]
pub struct Made {
    pub starts: Vec<(usize, Duration)>,
    pub timing: marks::Timing,
    pub engine: &'static str,
}

impl Track {
    pub fn new(shown: Vec<String>) -> Arc<Track> {
        Arc::new(Track { shown, made: Mutex::new(std::collections::HashMap::new()), made_count: Default::default() })
    }

    pub fn len(&self) -> usize {
        self.shown.len()
    }

    pub fn is_empty(&self) -> bool {
        self.shown.is_empty()
    }

    pub fn shown(&self, i: usize) -> Option<&str> {
        self.shown.get(i).map(String::as_str)
    }

    pub fn all(&self) -> &[String] {
        &self.shown
    }

    /// Sentence `i`, if it has been made.
    pub fn made(&self, i: usize) -> Option<Made> {
        self.made.lock().unwrap_or_else(|p| p.into_inner()).get(&i).cloned()
    }

    pub fn made_count(&self) -> usize {
        self.made_count.load(Ordering::SeqCst)
    }

    /// Sentence `i` was made: `said` (what the engine was given) in `heard`
    /// (its audio, interleaved, `format` = rate and channels; empty if it
    /// didn't come in one format), `length` long. Its words' starts come
    /// from the engine's `times` if it gave them (and they fit), else from
    /// the audio ([`marks::from_audio`]), mapped back to the shown words.
    #[allow(clippy::too_many_arguments)]
    fn record(&self, i: usize, said: &str, times: Option<Vec<marks::CharTime>>, heard: &[f32], format: (u32, u16), length: Duration, engine: &'static str) {
        let Some(shown) = self.shown.get(i) else { return };
        let said_spans = marks::words(said);
        let (said_starts, timing) = match times.and_then(|t| marks::from_chars(&said_spans, &t)) {
            Some(t) => (t, marks::Timing::Engine),
            None if format.0 > 0 && !heard.is_empty() => marks::from_audio(said, &said_spans, heard, format.0, format.1),
            None => (marks::estimate(said, &said_spans, Duration::ZERO, length), marks::Timing::Estimate),
        };
        let shown_spans = marks::words(shown);
        let map = marks::align(shown, &shown_spans, said, &said_spans);
        let starts = marks::shown_starts(&map, &said_starts);
        let mut made = self.made.lock().unwrap_or_else(|p| p.into_inner());
        // Only the sentences round the voice are worth keeping.
        made.retain(|&k, _| k + 16 > i && k < i + 16);
        made.insert(i, Made { starts, timing, engine });
        drop(made);
        self.made_count.fetch_add(1, Ordering::SeqCst);
    }
}

/// When the engine fails mid-answer: `local` reads the rest — the sentence
/// that failed included — or, with none, the answer stops there. `notify`
/// is told once (the error, the sentences left, whether `local` took
/// over): it says so to the person, so the state carries no error.
pub struct Fallback {
    pub local: Option<(Arc<dyn Engine>, String)>,
    pub notify: Notify,
}

/// [`Fallback::notify`]: (the error, the sentences left, whether the local
/// voice took over).
pub type Notify = Box<dyn FnOnce(&str, &[String], bool) + Send>;

struct Current {
    generation: u64,
    message_id: Option<String>,
    /// Set once the worker has opened the output.
    play: Option<Arc<dyn Playback>>,
    /// To cut a sentence being made short on a stop.
    engine: Arc<dyn Engine>,
}

type Emit = Box<dyn Fn(&TtsState) + Send + Sync>;
type Busy = Box<dyn Fn() -> bool + Send + Sync>;

/// One voice at a time: [`Speaker::speak`] replaces whatever is being said,
/// [`Speaker::stop`] silences it at once. `busy`: something is listening (a
/// dictation recording) — checked before every sentence and while one plays.
pub struct Speaker {
    generation: AtomicU64,
    current: Mutex<Option<Current>>,
    last_audible: Mutex<Option<Instant>>,
    emit: Emit,
    busy: Busy,
}

impl Speaker {
    pub fn new(emit: Emit, busy: Busy) -> Arc<Speaker> {
        Arc::new(Speaker {
            generation: AtomicU64::new(0),
            current: Mutex::new(None),
            last_audible: Mutex::new(None),
            emit,
            busy,
        })
    }

    fn current(&self) -> std::sync::MutexGuard<'_, Option<Current>> {
        self.current.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn heard_now(&self) {
        *self.last_audible.lock().unwrap_or_else(|p| p.into_inner()) = Some(Instant::now());
    }

    /// What's being said now.
    pub fn state(&self) -> TtsState {
        let cur = self.current();
        TtsState { speaking: cur.is_some(), message_id: cur.as_ref().and_then(|c| c.message_id.clone()), error: None }
    }

    /// Say `job` (whatever was being said stops first). It's "speaking" at
    /// once; the worker opens the output (`open`), then makes each sentence
    /// and queues it while the one before plays.
    pub fn speak(self: &Arc<Self>, job: Job, open: Opener) -> Result<(), String> {
        self.start(job, open).map(|_| ())
    }

    /// [`Speaker::speak`], returning the job's generation (a [`Feed`]
    /// follows it).
    fn start(self: &Arc<Self>, job: Job, open: Opener) -> Result<u64, String> {
        if job.sentences.is_empty() && job.feed.is_none() {
            return Err("Nothing to read aloud".into());
        }
        if (self.busy)() {
            return Err("Yap is listening to a dictation".into());
        }
        self.stop();
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *self.current() =
            Some(Current { generation, message_id: job.message_id.clone(), play: None, engine: Arc::clone(&job.engine) });
        (self.emit)(&TtsState { speaking: true, message_id: job.message_id.clone(), error: None });
        let me = Arc::clone(self);
        std::thread::Builder::new()
            .name("tts-speaker".into())
            .spawn(move || me.run(generation, job, open))
            .map(|_| generation)
            .map_err(|e| {
                self.finish(generation, None, Some(format!("Couldn't start speaking: {e}")));
                "Couldn't start speaking".to_string()
            })
    }

    fn live(&self, generation: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == generation
    }

    fn run(self: Arc<Self>, generation: u64, mut job: Job, open: Opener) {
        let opened = match open() {
            Ok(o) => o,
            Err(e) => return self.finish(generation, job.message_id, Some(e)),
        };
        {
            let mut cur = self.current();
            match cur.as_mut() {
                Some(c) if c.generation == generation => c.play = Some(Arc::clone(&opened.play)),
                // Stopped (or replaced) while the output opened.
                _ => {
                    opened.play.stop();
                    return;
                }
            }
        }
        let mut error = None;
        let mut synth = job.engine.open(&job.voice, job.rate);
        let mut engine_id = job.engine.id();
        let mut i = 0;
        // A streamed answer's last sentence (or its start): a feed that
        // never closes (its answer lost) ends after [`FEED_PATIENCE`].
        let mut fed_at = Instant::now();
        loop {
            // A streamed answer: its next sentences as they're written.
            if i >= job.sentences.len() {
                let Some(feed) = job.feed.clone() else { break };
                let (more, closed) = feed.take();
                if !more.is_empty() {
                    let more = job.engine.prepare(more);
                    job.sentences.extend(more);
                    fed_at = Instant::now();
                    continue;
                }
                if closed || !self.live(generation) || (self.busy)() || fed_at.elapsed() > FEED_PATIENCE {
                    break;
                }
                std::thread::sleep(Duration::from_millis(15));
                continue;
            }
            if !self.live(generation) || (self.busy)() {
                break;
            }
            let at = job.first + i;
            let track = job.track.clone();
            // A read-aloud: one sentence ahead of the voice, no more.
            if track.is_some() && i > 0 && !self.wait_turn(generation, &opened.play, at) {
                break;
            }
            let text = if track.is_some() { said_text(&job.sentences[i]) } else { job.sentences[i].clone() };
            // A read-aloud's sentence: its audio kept a moment (for its
            // words' times), each piece tagged with where it is.
            let mut heard: Vec<f32> = Vec::new();
            let mut format = (0u32, 0u16);
            let mut length = Duration::ZERO;
            let said = match synth.as_mut() {
                Ok(s) => {
                    let play = &opened.play;
                    let tracked = track.is_some();
                    // Each piece as it comes, unless it's been stopped or a
                    // dictation started meanwhile.
                    let mut out = |pcm: Pcm| {
                        let go = self.live(generation) && !(self.busy)();
                        if go {
                            if tracked {
                                let piece = pcm.duration();
                                if format.0 == 0 {
                                    format = (pcm.sample_rate, pcm.channels);
                                }
                                if format == (pcm.sample_rate, pcm.channels) {
                                    heard.extend_from_slice(&pcm.samples);
                                } else {
                                    heard.clear();
                                    format = (u32::MAX, 0);
                                }
                                play.append_tagged(pcm, Tag { sentence: at, offset: length });
                                length += piece;
                            } else {
                                play.append(pcm);
                            }
                        }
                        go
                    };
                    let r = s.stream(&text, &mut out);
                    r.map(|()| if tracked { s.marks() } else { None })
                }
                Err(e) => Err(e.clone()),
            };
            match said {
                Ok(times) => {
                    if let Some(t) = &track {
                        let format = if format.0 == u32::MAX { (0, 0) } else { format };
                        t.record(at, &text, times, &heard, format, length, engine_id);
                    }
                    i += 1;
                }
                // Stopped mid-sentence: not a failure.
                Err(_) if !self.live(generation) => break,
                Err(e) => {
                    tracing::warn!("Spoken replies: {}", e);
                    let Some(fallback) = job.fallback.take() else {
                        error = Some(e);
                        break;
                    };
                    // Once: the person is told, and the local voice (if
                    // there's one) reads the rest, this sentence again
                    // included. Never the failing engine again.
                    (fallback.notify)(&e, &job.sentences[i..], fallback.local.is_some());
                    let Some((engine, voice)) = fallback.local else {
                        break;
                    };
                    if let Some(c) = self.current().as_mut().filter(|c| c.generation == generation) {
                        c.engine = Arc::clone(&engine);
                    }
                    engine_id = engine.id();
                    synth = engine.open(&voice, job.rate);
                }
            }
        }
        // Let it play out, unless it's stopped or a dictation starts.
        while self.live(generation) && !opened.play.done() {
            if (self.busy)() {
                opened.play.stop();
                break;
            }
            if !opened.play.paused() {
                self.heard_now();
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(opened.keep);
        self.finish(generation, job.message_id, error);
    }

    /// A read-aloud's pace: sentence `at` is made once the one before it has
    /// started playing (or the voice ran dry), so a long text is made a
    /// sentence ahead of the voice, never all at once. `false`: stopped (or
    /// a dictation started) while it waited.
    fn wait_turn(&self, generation: u64, play: &Arc<dyn Playback>, at: usize) -> bool {
        loop {
            if !self.live(generation) || (self.busy)() {
                return false;
            }
            if play.done() || play.position().is_some_and(|(playing, _)| playing + 1 >= at) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    /// The read-aloud sentence playing now, and how far into it.
    pub fn position(&self) -> Option<(usize, Duration)> {
        self.current().as_ref()?.play.as_ref()?.position()
    }

    /// Hold the voice where it is (`true` if it was speaking).
    pub fn pause(&self) -> bool {
        let play = self.current().as_ref().and_then(|c| c.play.clone());
        match play {
            Some(p) => {
                p.pause();
                true
            }
            None => false,
        }
    }

    /// Go on from where it was held.
    pub fn resume(&self) -> bool {
        let play = self.current().as_ref().and_then(|c| c.play.clone());
        match play {
            Some(p) => {
                p.resume();
                true
            }
            None => false,
        }
    }


    /// The worker's end: "not speaking" — unless it was stopped or replaced,
    /// which said so already.
    fn finish(&self, generation: u64, message_id: Option<String>, error: Option<String>) {
        {
            let mut cur = self.current();
            if cur.as_ref().is_none_or(|c| c.generation != generation) {
                return;
            }
            *cur = None;
        }
        (self.emit)(&TtsState { speaking: false, message_id, error });
    }

    /// Silence it now and drop what was queued. `true` if something was
    /// being said.
    pub fn stop(&self) -> bool {
        self.generation.fetch_add(1, Ordering::SeqCst);
        let Some(cur) = self.current().take() else {
            return false;
        };
        cur.engine.cancel();
        if let Some(play) = &cur.play {
            if !play.done() {
                self.heard_now();
            }
            play.stop();
        }
        (self.emit)(&TtsState { speaking: false, message_id: cur.message_id, error: None });
        true
    }

    /// Whether Yap's voice was in the room just now (speaking, or the last
    /// sound under [`AUDIBLE_TAIL`] ago).
    pub fn recently_audible(&self) -> bool {
        self.last_audible
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some_and(|t| t.elapsed() < AUDIBLE_TAIL)
    }
}

static SPEAKER: OnceLock<Arc<Speaker>> = OnceLock::new();
/// For a cloud voice's notice (`alert.rs`).
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

/// Start the speaker (lib.rs setup): its state goes to every window. A
/// watcher unloads Kokoro once it's been idle for the speech model's unload
/// time (10 minutes when that's "Never": it holds ~0.6 GB).
pub fn init(app: &tauri::AppHandle) {
    set_volume(crate::config::load().tts_volume);
    let _ = APP.set(app.clone());
    let app = app.clone();
    let _ = SPEAKER.set(Speaker::new(
        Box::new(move |s| {
            use tauri::Emitter;
            let _ = app.emit("yap-tts-state", s);
            // The Voice Assistant's answer ended (or was stopped).
            crate::chat::assistant::on_tts_state(&app, s.speaking, s.message_id.as_deref());
            // So did a read-aloud (its card goes).
            crate::voice::read_aloud::on_tts_state(&app, s);
        }),
        Box::new(|| crate::pipeline::dictation_state().1),
    ));
    let _ = std::thread::Builder::new().name("tts-idle".into()).spawn(|| loop {
        std::thread::sleep(Duration::from_secs(20));
        kokoro::idle_unload(kokoro_idle(&crate::config::load().model_unload_timeout));
    });
}

/// How long Kokoro stays loaded unused: the speech model's unload time, or
/// 10 minutes when that's "never".
fn kokoro_idle(setting: &str) -> Duration {
    match setting {
        "1min" => Duration::from_secs(60),
        "5min" => Duration::from_secs(5 * 60),
        "15min" => Duration::from_secs(15 * 60),
        "30min" => Duration::from_secs(30 * 60),
        _ => Duration::from_secs(10 * 60),
    }
}

fn speaker() -> Option<&'static Arc<Speaker>> {
    SPEAKER.get()
}

/// Stop speaking now (Stop, Esc, leaving Chat, a meeting starting).
pub fn stop() -> bool {
    speaker().is_some_and(|s| s.stop())
}

/// What's being read aloud now, if anything (its message id).
pub fn speaking_id() -> Option<String> {
    speaker().map(|s| s.state()).filter(|s| s.speaking).and_then(|s| s.message_id)
}

/// A dictation is starting (`pipeline::start_recording`): stop speaking
/// first. `true` when Yap's voice may be in the mic's pre-roll (it was
/// speaking, or stopped a moment ago), so the dictation leaves it out.
pub fn stop_for_dictation() -> bool {
    let Some(s) = speaker() else {
        return false;
    };
    let was = s.stop();
    if was {
        tracing::info!("Spoken replies: a dictation started, stopped speaking");
    }
    was || s.recently_audible()
}

/// **Say `text`** — an answer's markdown, or any plain text (a later "read
/// the selection aloud" comes in here too) — with the voice Settings picked
/// ([`voicing`]: the local one, or a cloud voice with the local one standing
/// by), its speed, Yap's volume and the output device. `message_id`: what
/// the pages call it.
pub fn speak(text: &str, message_id: Option<String>) -> Result<(), String> {
    let cfg = crate::config::load();
    say(utterances(text), message_id, voicing(&cfg), cfg.tts_rate, cfg.output_device)
}

/// **Start saying a streamed answer** (the Voice Assistant, `assistant.rs`):
/// the voice Settings picked, as [`speak`] uses it, with no sentences yet —
/// they come through the returned [`Feed`] as the answer is written.
pub fn speak_streamed(message_id: Option<String>) -> Result<Arc<Feed>, String> {
    if let Some(why) = meeting_quiet() {
        return Err(why.into());
    }
    let speaker = speaker().ok_or("Spoken replies aren't ready yet")?;
    let cfg = crate::config::load();
    let rate = if cfg.tts_rate.is_finite() && cfg.tts_rate > 0.0 { cfg.tts_rate.clamp(0.5, 2.0) } else { 1.0 };
    let Voicing { engine, voice, cloud } = voicing(&cfg);
    let feed = Arc::new(Feed::default());
    #[cfg(debug_assertions)]
    if crate::e2e::active() {
        feed.state.lock().unwrap_or_else(|p| p.into_inner()).record = Some(Spoken {
            message_id: message_id.clone(),
            text: String::new(),
            sentences: Vec::new(),
            engine: engine.id().to_string(),
            voice: voice.clone(),
            rate,
            volume: volume(),
            fallback: None,
            streamed: true,
        });
    }
    let fallback = cloud.map(|plan| fallback_for(plan, message_id.clone(), rate));
    let opener = if crate::e2e::active() { sim_opener() } else { rodio_opener(cfg.output_device) };
    let job = Job { message_id, sentences: Vec::new(), engine, voice, rate, fallback, track: None, first: 0, feed: Some(Arc::clone(&feed)) };
    let generation = speaker.start(job, opener)?;
    feed.attach(speaker, generation);
    Ok(feed)
}

/// A streamed answer's sentences the voice can say now
/// ([`speak_streamed`]): its first block's, as [`brief`] takes them, at most
/// `n` — without the last while the block may still be being written
/// (`done`: the answer is whole). Written for the ear as [`utterances`]
/// writes them. Pure.
pub fn ready_sentences(md: &str, n: usize, done: bool) -> Vec<String> {
    let md: String = md.chars().take(MAX_CHARS).collect();
    let blocks = speakable_blocks(&md);
    let Some(at) = blocks.iter().position(|b| b.chars().any(char::is_alphanumeric)) else {
        return Vec::new();
    };
    let mut out = sentences(&normalize::for_speech(&blocks[at]));
    if !done && at + 1 == blocks.len() {
        out.pop();
    }
    out.truncate(n);
    out
}

/// Why Yap mustn't talk now, if it mustn't: a meeting records (the notes
/// would hear Yap) or a call is on (the people on it could, and a voice over
/// a meeting is too much — the maintainer, 2026-10-18). Spoken replies, ▶ and the
/// Voice Assistant (`assistant::off_for_meeting`) all keep quiet then.
pub fn meeting_quiet() -> Option<&'static str> {
    if crate::meetings::meeting::is_recording() {
        Some("Yap doesn't talk while it records a meeting")
    } else if crate::calls::meeting_detect::call_live() {
        Some("Yap doesn't talk during a call")
    } else {
        None
    }
}

fn say(sentences: Vec<String>, message_id: Option<String>, voicing: Voicing, rate: f32, device: Option<String>) -> Result<(), String> {
    if let Some(why) = meeting_quiet() {
        return Err(why.into());
    }
    let speaker = speaker().ok_or("Spoken replies aren't ready yet")?;
    let rate = if rate.is_finite() && rate > 0.0 { rate.clamp(0.5, 2.0) } else { 1.0 };
    let Voicing { engine, voice, cloud } = voicing;
    let sentences = engine.prepare(sentences);
    #[cfg(debug_assertions)]
    if crate::e2e::active() && !sentences.is_empty() {
        spoken().push(Spoken {
            message_id: message_id.clone(),
            text: sentences.join(" "),
            sentences: sentences.clone(),
            engine: engine.id().to_string(),
            voice: voice.clone(),
            rate,
            volume: volume(),
            fallback: None,
            streamed: false,
        });
    }
    let fallback = cloud.map(|plan| fallback_for(plan, message_id.clone(), rate));
    let opener = if crate::e2e::active() { sim_opener() } else { rodio_opener(device) };
    let job = Job { message_id, sentences, engine, voice, rate, fallback, track: None, first: 0, feed: None };
    speaker.speak(job, opener)
}

// ---- reading aloud (read_aloud.rs) ----

/// The sentences of a read-aloud as the voice Settings picked says them best
/// (Kokoro and ElevenLabs join a sentence under three words to the next).
pub fn prepare_for_reading(sentences: Vec<String>) -> Vec<String> {
    voicing_for(&crate::config::load(), true).engine.prepare(sentences)
}

/// **Read `track` aloud** from sentence `from` (`read_aloud.rs`): the voice
/// Settings picked (a cloud voice asked for its word timing), at `rate`,
/// Yap's volume, the output device. Each sentence is made only once the one
/// before it plays, and the track keeps when its words start. `message_id`:
/// what the pages call it (an answer's id keeps its ▶ / ■).
pub fn read(track: Arc<Track>, from: usize, message_id: String, rate: f32) -> Result<(), String> {
    if let Some(why) = meeting_quiet() {
        return Err(why.into());
    }
    let speaker = speaker().ok_or("Spoken replies aren't ready yet")?;
    let cfg = crate::config::load();
    let rate = if rate.is_finite() && rate > 0.0 { rate.clamp(0.5, 2.0) } else { 1.0 };
    let sentences: Vec<String> = track.all().get(from..).map(<[String]>::to_vec).unwrap_or_default();
    let Voicing { engine, voice, cloud } = voicing_for(&cfg, true);
    #[cfg(debug_assertions)]
    if crate::e2e::active() && !sentences.is_empty() {
        let said: Vec<String> = sentences.iter().map(|s| said_text(s)).collect();
        spoken().push(Spoken {
            message_id: Some(message_id.clone()),
            text: said.join(" "),
            sentences: said,
            engine: engine.id().to_string(),
            voice: voice.clone(),
            rate,
            volume: volume(),
            fallback: None,
            streamed: false,
        });
    }
    let fallback = cloud.map(|plan| fallback_for(plan, Some(message_id.clone()), rate));
    let opener = if crate::e2e::active() { sim_opener() } else { rodio_opener(cfg.output_device) };
    let job = Job { message_id: Some(message_id), sentences, engine, voice, rate, fallback, track: Some(track), first: from, feed: None };
    speaker.speak(job, opener)
}

/// The read-aloud sentence playing now (its index on the track), and how
/// far into its audio.
pub fn position() -> Option<(usize, Duration)> {
    speaker().and_then(|s| s.position())
}

/// Hold the voice where it is (a read-aloud's ⏸).
pub fn pause() -> bool {
    speaker().is_some_and(|s| s.pause())
}

/// Go on from where it was held.
pub fn resume() -> bool {
    speaker().is_some_and(|s| s.resume())
}


/// A cloud voice's [`Fallback`]: its local voice, and the notice.
fn fallback_for(plan: CloudPlan, message_id: Option<String>, rate: f32) -> Fallback {
    let CloudPlan { provider, local, key, choice } = plan;
    // What test mode records when the local voice takes over.
    #[cfg(debug_assertions)]
    let record = local.as_ref().map(|(e, v)| (e.id().to_string(), v.clone()));
    let _ = (&message_id, rate);
    Fallback {
        local,
        notify: Box::new(move |error: &str, left: &[String], took_over: bool| {
            #[cfg(debug_assertions)]
            if let (true, Some((engine, voice))) = (crate::e2e::active(), record) {
                spoken().push(Spoken {
                    message_id,
                    text: left.join(" "),
                    sentences: left.to_vec(),
                    engine,
                    voice,
                    rate,
                    volume: volume(),
                    fallback: Some(error.to_string()),
                    streamed: false,
                });
            }
            let _ = left;
            cloud_failed(provider, key, choice, error, took_over);
        }),
    }
}

/// The alert's kind for "ElevenLabs didn't answer" (`alert.rs`).
pub const CLOUD_FAILED: &str = "voiceFallback";

/// A cloud voice failed mid-answer: say why — once per reason in ten
/// minutes (`cloud::Health::announce`) — in the one place Yap's notices go
/// (`alert.rs`: the main window's toast, else a Yap bar card, else a
/// Windows notification), with a way to the voice settings.
fn cloud_failed(provider: &'static str, key: u64, choice: u64, error: &str, took_over: bool) {
    let failure = cloud::with_health(|h| {
        // A failure the engine couldn't put on record itself (its request
        // couldn't even be made): on record now, in its words.
        if h.last(provider).is_none() {
            let why = error.split_once(": ").map_or(error, |(_, w)| w);
            h.failed(provider, key, choice, cloud::Failure::Rejected(why.to_string()), Instant::now());
        }
        h.announce(provider, Instant::now())
    });
    let Some(failure) = failure else {
        tracing::info!("Spoken replies: {} failed again ({error}); said already", cloud::name(provider));
        return;
    };
    let (title, body) = cloud::notice_text(provider, &failure, took_over);
    match APP.get() {
        Some(app) => crate::notices::alert::show_soon(
            app,
            crate::notices::alert::Alert {
                kind: CLOUD_FAILED,
                icon: "assistant",
                title,
                body,
                action: Some(crate::notices::alert::Action { id: crate::notices::alert::OPEN_VOICE_SETTINGS, label: "Open settings" }),
                timeout_ms: 12_000,
            },
        ),
        None => tracing::warn!("Spoken replies: {title}"),
    }
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
    let e: Arc<dyn Engine> = if crate::e2e::active() { Arc::new(StandIn) } else { Arc::new(WindowsVoices) };
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
    {
        let mut d = DOWNLOADING.lock().unwrap_or_else(|p| p.into_inner());
        if d.is_some() {
            return Err("Kokoro is already downloading".into());
        }
        *d = Some(registry::Progress { percent: 0, downloaded_bytes: 0, total_bytes: 0 });
    }
    let progress_app = app.clone();
    let on_progress = move |p: registry::Progress| {
        *DOWNLOADING.lock().unwrap_or_else(|p| p.into_inner()) = Some(p.clone());
        let _ = progress_app.emit("tts-download-progress", &p);
    };
    let result = registry::install(&registry::root(), &on_progress).await;
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
    let v = self::volume();
    if save.unwrap_or(false) {
        // In one step: never over a change saved meanwhile.
        crate::commands::change_config(&app, |cfg| {
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
    crate::commands::change_config(&app, |cfg| {
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
    if !crate::e2e::active() {
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
    fn markdown_reads_naturally() {
        let md = "## Launch plan\n\nThe launch is on **Friday**, see [the doc](https://docs.example.com/plan) or https://www.github.com/acme/app/issues/4.\n\n- Fix the *login* bug\n- [ ] Write the `README`\n- [x] Ship 🎉\n1. First step\n\n```rust\nfn main() {}\n```\n\n> Quoted line\n\n---\n| Name | Due |\n|---|---|\n| Sam | Monday |";
        assert_eq!(
            speakable_blocks(md),
            vec![
                "Launch plan.",
                "The launch is on Friday, see the doc or github.com.",
                "Fix the login bug.",
                "Write the README.",
                "Done: Ship.",
                "First step.",
                "(code block)",
                "Quoted line.",
                "Name, Due.",
                "Sam, Monday.",
            ]
        );
    }

    #[test]
    fn paragraph_lines_run_on_and_marks_go() {
        assert_eq!(speakable_blocks("It moved\nto Monday."), vec!["It moved to Monday."]);
        assert_eq!(speakable_blocks("__Bold__ and _italic_ but snake_case_name stays"), vec!["Bold and italic but snake_case_name stays."]);
        assert_eq!(speakable_blocks("~~old~~ new <br> line"), vec!["old new line."]);
        assert_eq!(speakable_blocks("Friday → Monday"), vec!["Friday to Monday."]);
        assert_eq!(speakable_blocks("☐ Call Sam\n☑ Email Jo"), vec!["Call Sam.", "Done: Email Jo."]);
        assert_eq!(speakable_blocks("Great idea 👍🏽 ✅ done!"), vec!["Great idea done!"]);
        assert_eq!(speakable_blocks("👍"), Vec::<String>::new());
        assert_eq!(speakable_blocks("Unclosed\n```\ncode\nmore"), vec!["Unclosed.", "(code block)"]);
        assert_eq!(speakable_blocks("A heading:\n- one\n- two"), vec!["A heading:", "one.", "two."]);
    }

    #[test]
    fn urls_become_their_domain() {
        assert_eq!(domain("https://www.GitHub.com/a/b?c=1#d"), "github.com");
        assert_eq!(domain("http://user@host.example.org:8080/x"), "host.example.org");
        assert_eq!(domain("www.example.co.uk/page"), "example.co.uk");
        assert_eq!(inline_text("Go to <https://yap.app/docs>."), "Go to yap.app.");
        assert_eq!(inline_text("[https://a.example.com/x](https://a.example.com/x)"), "a.example.com");
        assert_eq!(inline_text("(see https://x.example.com/y)."), "(see x.example.com).");
    }

    #[test]
    fn sentences_keep_abbreviations_together() {
        assert_eq!(
            sentences("Hi Dr. Smith. It's 5 p.m. now, e.g. later. J. R. Tolkien wrote it! Really? Yes… ok."),
            vec!["Hi Dr. Smith.", "It's 5 p.m. now, e.g. later.", "J. R. Tolkien wrote it!", "Really?", "Yes…", "ok."]
        );
        assert_eq!(sentences("Version 3.5 is out. \"Great,\" she said. (Yes.) Next"), vec!["Version 3.5 is out.", "\"Great,\" she said.", "(Yes.)", "Next"]);
        assert_eq!(sentences("Wait... what?! No."), vec!["Wait...", "what?!", "No."]);
        assert_eq!(sentences("   "), Vec::<String>::new());
    }

    #[test]
    fn sentences_after_a_no_break_space() {
        // A web page's &nbsp; before a sentence's last word (read aloud).
        assert_eq!(sentences("Le train part à 15\u{a0}h. Il arrive tard."), vec!["Le train part à 15\u{a0}h.", "Il arrive tard."]);
        assert_eq!(sentences("Ask Dr.\u{a0}Smith. Then J.\u{202f}Lee."), vec!["Ask Dr.\u{a0}Smith.", "Then J.\u{202f}Lee."]);
    }

    #[test]
    fn long_sentences_are_cut_at_a_comma() {
        let long = format!("{}, {}", "word ".repeat(30).trim(), "more ".repeat(30).trim());
        let parts = sentences(&long);
        assert_eq!(parts.len(), 2);
        assert!(parts[0].ends_with(','));
        assert!(parts.iter().all(|p| p.chars().count() <= LONG_SENTENCE));
        // No comma: at a space.
        let words = "abcdefg ".repeat(60);
        let parts = sentences(&words);
        assert!(parts.len() >= 2 && parts.iter().all(|p| p.chars().count() <= LONG_SENTENCE));
        assert_eq!(parts.join(" "), words.trim());
    }

    #[test]
    fn utterances_split_blocks_into_sentences() {
        assert_eq!(
            utterances("# Plan\nIt's Friday. Sam owns it.\n- Ask Jo"),
            vec!["Plan.", "It's Friday.", "Sam owns it.", "Ask Jo."]
        );
        assert_eq!(utterances("```\nonly code\n```"), vec!["(code block)"]);
    }

    #[test]
    fn citation_markers_are_never_read_out() {
        assert_eq!(
            utterances("Gale is on the beach [1]. He joins you [2][3], or later [2, 4]. See 【2†L6-L10】 there."),
            vec!["Gale is on the beach.", "He joins you, or later.", "See there."]
        );
        // A link's text and a list's number are kept.
        assert_eq!(inline_text("[the wiki](https://bg3.wiki/wiki/Gale) says so [1]"), "the wiki says so");
        // The short answer: the first sentences of the first block, no markers.
        let md = "Gale is on the beach west of the crash site [1]. Pull him out of the portal [1]. He's a wizard [2].\n\n## More\n- He can be romanced [3].";
        assert_eq!(brief(md, 2), "Gale is on the beach west of the crash site. Pull him out of the portal.");
        assert_eq!(brief("- Only a list [1]", 2), "Only a list.");
        assert_eq!(brief("", 2), "");
    }

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

    // ---- the speaker's queue and stop, on a simulated clock

    /// A synthesiser that keeps what it was asked and takes `delay` a sentence.
    struct Recording {
        said: Arc<Mutex<Vec<String>>>,
        delay: Duration,
        fail_on: Option<&'static str>,
    }
    impl Synth for Recording {
        fn synthesize(&mut self, text: &str) -> Result<Pcm, String> {
            std::thread::sleep(self.delay);
            if self.fail_on == Some(text) {
                return Err("synthesis failed".into());
            }
            self.said.lock().unwrap().push(text.to_string());
            // 100 ms a sentence.
            Ok(Pcm { sample_rate: 1_000, channels: 1, samples: vec![0.0; 100] })
        }
    }
    struct Fake {
        said: Arc<Mutex<Vec<String>>>,
        delay: Duration,
        fail_on: Option<&'static str>,
    }
    impl Engine for Fake {
        fn id(&self) -> &'static str {
            "fake"
        }
        fn voices(&self) -> Result<Vec<Voice>, String> {
            Ok(Vec::new())
        }
        fn default_voice(&self) -> Option<String> {
            None
        }
        fn open(&self, _voice: &str, _rate: f32) -> Result<Box<dyn Synth>, String> {
            Ok(Box::new(Recording { said: Arc::clone(&self.said), delay: self.delay, fail_on: self.fail_on }))
        }
    }

    struct Rig {
        speaker: Arc<Speaker>,
        events: Arc<Mutex<Vec<TtsState>>>,
        said: Arc<Mutex<Vec<String>>>,
        busy: Arc<std::sync::atomic::AtomicBool>,
    }

    fn rig() -> Rig {
        let events = Arc::new(Mutex::new(Vec::new()));
        let busy = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (e, b) = (Arc::clone(&events), Arc::clone(&busy));
        Rig {
            speaker: Speaker::new(
                Box::new(move |s| e.lock().unwrap().push(s.clone())),
                Box::new(move || b.load(Ordering::SeqCst)),
            ),
            events,
            said: Arc::new(Mutex::new(Vec::new())),
            busy,
        }
    }

    impl Rig {
        fn job(&self, id: &str, sentences: &[&str], delay_ms: u64, fail_on: Option<&'static str>) -> Job {
            Job {
                message_id: Some(id.into()),
                sentences: sentences.iter().map(|s| s.to_string()).collect(),
                engine: Arc::new(Fake { said: Arc::clone(&self.said), delay: Duration::from_millis(delay_ms), fail_on }),
                voice: String::new(),
                rate: 1.0,
                fallback: None,
                track: None,
                first: 0,
                feed: None,
            }
        }
        fn events(&self) -> Vec<(bool, String, Option<String>)> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .map(|s| (s.speaking, s.message_id.clone().unwrap_or_default(), s.error.clone()))
                .collect()
        }
        fn wait_quiet(&self) {
            let start = Instant::now();
            while self.speaker.state().speaking {
                assert!(start.elapsed() < Duration::from_secs(5), "still speaking");
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    fn ev(speaking: bool, id: &str) -> (bool, String, Option<String>) {
        (speaking, id.to_string(), None)
    }

    #[test]
    fn speaks_every_sentence_in_order_then_says_it_stopped() {
        let r = rig();
        r.speaker.speak(r.job("a", &["One.", "Two.", "Three."], 1, None), sim_opener()).unwrap();
        assert_eq!(r.speaker.state().message_id.as_deref(), Some("a"));
        r.wait_quiet();
        assert_eq!(*r.said.lock().unwrap(), vec!["One.", "Two.", "Three."]);
        assert_eq!(r.events(), vec![ev(true, "a"), ev(false, "a")]);
        // It was audible a moment ago.
        assert!(r.speaker.recently_audible());
    }

    #[test]
    fn a_table_is_read_row_by_row_without_its_rule_line() {
        let blocks = speakable_blocks("Plans:

| Plan | Price |
|:---|---:|
| Plus | $8 |
| Premium | $30 |");
        let said = blocks.join(" ");
        assert!(said.contains("Plan, Price"), "{said}");
        assert!(said.contains("Premium"), "{said}");
        assert!(!said.contains("---") && !said.contains(":-"), "{said}");
    }

    #[test]
    fn the_volume_curve_is_quiet_at_the_low_end() {
        assert_eq!(gain(1.0), 1.0);
        assert_eq!(gain(0.5), 0.25);
        assert!((gain(0.1) - 0.01).abs() < 1e-6);
        assert_eq!(gain(0.0), 0.0);
        // Out of range or not a number: clamped, else the default half.
        assert_eq!(gain(3.0), 1.0);
        assert_eq!(gain(-1.0), 0.0);
        assert_eq!(gain(f32::NAN), 0.25);
    }

    #[test]
    fn stop_is_instant_and_drops_the_queue() {
        let r = rig();
        r.speaker.speak(r.job("a", &["One.", "Two.", "Three.", "Four."], 40, None), sim_opener()).unwrap();
        std::thread::sleep(Duration::from_millis(60));
        assert!(r.speaker.stop());
        // Not speaking the moment stop returns.
        assert!(!r.speaker.state().speaking);
        std::thread::sleep(Duration::from_millis(200));
        assert!(r.said.lock().unwrap().len() <= 2, "the queue went on: {:?}", r.said.lock().unwrap());
        assert_eq!(r.events(), vec![ev(true, "a"), ev(false, "a")]);
        // Nothing left to stop.
        assert!(!r.speaker.stop());
    }

    #[test]
    fn a_new_answer_replaces_the_one_being_read() {
        let r = rig();
        r.speaker.speak(r.job("a", &["A one.", "A two.", "A three."], 30, None), sim_opener()).unwrap();
        std::thread::sleep(Duration::from_millis(40));
        r.speaker.speak(r.job("b", &["B one."], 1, None), sim_opener()).unwrap();
        r.wait_quiet();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(r.events(), vec![ev(true, "a"), ev(false, "a"), ev(true, "b"), ev(false, "b")]);
        let said = r.said.lock().unwrap().clone();
        assert!(said.contains(&"B one.".to_string()));
        assert!(!said.contains(&"A three.".to_string()), "{said:?}");
    }

    #[test]
    fn a_dictation_silences_it_and_nothing_starts_over_one() {
        let r = rig();
        r.speaker.speak(r.job("a", &["One.", "Two.", "Three.", "Four."], 30, None), sim_opener()).unwrap();
        std::thread::sleep(Duration::from_millis(40));
        r.busy.store(true, Ordering::SeqCst);
        r.wait_quiet();
        assert!(r.said.lock().unwrap().len() < 4);
        // While it records, nothing new is said.
        assert!(r.speaker.speak(r.job("b", &["Hi."], 1, None), sim_opener()).is_err());
        assert_eq!(r.events().last(), Some(&ev(false, "a")));
    }

    #[test]
    fn a_failure_ends_it_and_says_why() {
        let r = rig();
        r.speaker.speak(r.job("a", &["One.", "Bad.", "Three."], 1, Some("Bad.")), sim_opener()).unwrap();
        r.wait_quiet();
        assert_eq!(*r.said.lock().unwrap(), vec!["One."]);
        assert_eq!(r.events().last(), Some(&(false, "a".to_string(), Some("synthesis failed".to_string()))));
        // An output that won't open says so too.
        let r = rig();
        r.speaker
            .speak(r.job("b", &["One."], 1, None), Box::new(|| Err("No sound output device to speak on".to_string())))
            .unwrap();
        r.wait_quiet();
        assert_eq!(r.events().last().and_then(|e| e.2.clone()).as_deref(), Some("No sound output device to speak on"));
        assert!(r.said.lock().unwrap().is_empty());
    }

    #[test]
    fn a_streamed_answer_is_said_as_its_sentences_come() {
        let r = rig();
        let feed = Arc::new(Feed::default());
        let mut job = r.job("s", &[], 1, None);
        job.feed = Some(Arc::clone(&feed));
        let g = r.speaker.start(job, sim_opener()).expect("started with no sentences yet");
        feed.attach(&r.speaker, g);
        assert!(r.speaker.state().speaking);
        assert!(feed.push(vec!["The first one.".into()]));
        let start = Instant::now();
        while r.said.lock().unwrap().is_empty() {
            assert!(start.elapsed() < Duration::from_secs(5), "the first sentence wasn't said");
            std::thread::sleep(Duration::from_millis(5));
        }
        // Still open: it waits for more, speaking.
        std::thread::sleep(Duration::from_millis(40));
        assert!(r.speaker.state().speaking);
        assert!(feed.push(vec!["The second one.".into(), "And the third.".into()]));
        feed.close();
        r.wait_quiet();
        assert_eq!(*r.said.lock().unwrap(), vec!["The first one.", "The second one.", "And the third."]);
        // Closed: nothing more goes in.
        assert!(!feed.push(vec!["Too late.".into()]));
        assert_eq!(r.events(), vec![ev(true, "s"), ev(false, "s")]);
    }

    #[test]
    fn a_stopped_streamed_answer_takes_no_more() {
        let r = rig();
        let feed = Arc::new(Feed::default());
        let mut job = r.job("s", &[], 1, None);
        job.feed = Some(Arc::clone(&feed));
        let g = r.speaker.start(job, sim_opener()).unwrap();
        feed.attach(&r.speaker, g);
        assert!(feed.live() && feed.push(vec!["One more time.".into()]));
        assert!(r.speaker.stop());
        assert!(!feed.live());
        assert!(!feed.push(vec!["Two more times.".into()]));
        // Never attached: never live.
        assert!(!Feed::default().push(vec!["Nobody hears this.".into()]));
    }

    #[test]
    fn a_streamed_answer_gives_the_voice_its_whole_sentences() {
        // While it's written: the first block's whole sentences, never the
        // one still being written.
        assert!(ready_sentences("The match starts at", 2, false).is_empty());
        assert_eq!(ready_sentences("The match starts at eight. It is", 2, false), ["The match starts at eight."]);
        assert_eq!(ready_sentences("Up first. Then the second. Then a third. And", 2, false), ["Up first.", "Then the second."]);
        // Another block began: the first is whole (its citation not read).
        assert_eq!(ready_sentences("Gale is on the beach [1].\n\n- He", 2, false), ["Gale is on the beach."]);
        // Done: all of it, up to n.
        assert_eq!(ready_sentences("Only this.", 2, true), ["Only this."]);
        assert!(ready_sentences("", 2, true).is_empty());
    }

    #[test]
    fn nothing_to_say_is_an_error() {
        let r = rig();
        assert!(r.speaker.speak(r.job("a", &[], 1, None), sim_opener()).is_err());
        assert!(r.events().is_empty());
    }

    // ---- a read-aloud: paced, its words timed, held

    fn read_job(track: &Arc<Track>, first: usize, engine: Arc<dyn Engine>, rate: f32) -> Job {
        Job {
            message_id: Some("read:1".into()),
            sentences: track.all()[first..].to_vec(),
            engine,
            voice: String::new(),
            rate,
            fallback: None,
            track: Some(Arc::clone(track)),
            first,
            feed: None,
        }
    }

    fn sim(play: &Arc<SimPlayback>) -> Opener {
        let p = Arc::clone(play);
        Box::new(move || Ok(Opened { play: p, keep: None }))
    }

    #[test]
    fn a_read_aloud_is_made_a_sentence_ahead_never_all_at_once() {
        let r = rig();
        // 30 short sentences, ~170 ms each at 3× on the stand-in.
        let track = Track::new((0..30).map(|i| format!("Line {}.", ["one", "two", "three"][i % 3])).collect());
        let play = Arc::new(SimPlayback::default());
        r.speaker.speak(read_job(&track, 0, Arc::new(StandIn), 3.0), sim(&play)).unwrap();
        let start = Instant::now();
        let mut seen = 0;
        while start.elapsed() < Duration::from_millis(900) {
            let made = track.made_count();
            if let Some((playing, _)) = r.speaker.position() {
                // Made: the one playing and the next, never more.
                assert!(made <= playing + 2, "made {made} with {playing} playing");
                seen = seen.max(playing);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(seen >= 2, "it moved on: {seen}");
        assert!(track.made_count() < 30);
        // Each sentence made has its words' starts, estimated (silence).
        let made = track.made(0).unwrap();
        assert_eq!(made.timing, marks::Timing::Estimate);
        assert_eq!(made.starts.iter().map(|&(w, _)| w).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(made.starts[0].1, Duration::ZERO);
        assert!(r.speaker.stop());
    }

    #[test]
    fn a_held_read_aloud_stands_still_and_goes_on_from_there() {
        let r = rig();
        let track = Track::new((0..6).map(|_| "Read these four words.".to_string()).collect());
        let play = Arc::new(SimPlayback::default());
        r.speaker.speak(read_job(&track, 0, Arc::new(StandIn), 1.0), sim(&play)).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert!(r.speaker.pause());
        let held = r.speaker.position().unwrap();
        let made = track.made_count();
        std::thread::sleep(Duration::from_millis(400));
        // Held: the same place, nothing more made, still "speaking".
        assert_eq!(r.speaker.position().unwrap(), held);
        assert_eq!(track.made_count(), made);
        assert!(r.speaker.state().speaking);
        assert!(r.speaker.resume());
        std::thread::sleep(Duration::from_millis(300));
        let now = r.speaker.position().unwrap();
        assert!(now > held, "{now:?} after {held:?}");
        // Starting from a later sentence: tagged from there.
        r.speaker.speak(read_job(&track, 4, Arc::new(StandIn), 1.0), sim(&Arc::new(SimPlayback::default()))).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(r.speaker.position().map(|p| p.0), Some(4));
        r.speaker.stop();
    }

    /// An engine that says when each character starts (as Windows' cues
    /// and ElevenLabs' alignment do): 50 ms a character.
    struct Timed;
    struct TimedSynth(Option<Vec<marks::CharTime>>);
    impl Engine for Timed {
        fn id(&self) -> &'static str {
            "timed"
        }
        fn voices(&self) -> Result<Vec<Voice>, String> {
            Ok(Vec::new())
        }
        fn default_voice(&self) -> Option<String> {
            None
        }
        fn open(&self, _voice: &str, _rate: f32) -> Result<Box<dyn Synth>, String> {
            Ok(Box::new(TimedSynth(None)))
        }
    }
    impl Synth for TimedSynth {
        fn synthesize(&mut self, text: &str) -> Result<Pcm, String> {
            let n = text.chars().count();
            self.0 = Some((0..n).map(|i| marks::CharTime { at: i, start: Duration::from_millis(i as u64 * 50) }).collect());
            Ok(Pcm { sample_rate: 1_000, channels: 1, samples: vec![0.0; n * 50] })
        }
        fn marks(&mut self) -> Option<Vec<marks::CharTime>> {
            self.0.take()
        }
    }

    #[test]
    fn an_engines_own_word_timing_is_kept_on_the_shown_words() {
        let r = rig();
        // Said "It came to twelve pounds." for "It came to £12."
        let track = Track::new(vec!["It came to £12.".into()]);
        let play = Arc::new(SimPlayback::default());
        r.speaker.speak(read_job(&track, 0, Arc::new(Timed), 1.0), sim(&play)).unwrap();
        let start = Instant::now();
        while track.made(0).is_none() {
            assert!(start.elapsed() < Duration::from_secs(2));
            std::thread::sleep(Duration::from_millis(5));
        }
        let made = track.made(0).unwrap();
        assert_eq!(made.timing, marks::Timing::Engine);
        assert_eq!(made.engine, "timed");
        // "It" 0, "came" 3 chars in, "to" 8, "£12" when "twelve" starts (11).
        assert_eq!(
            made.starts,
            vec![(0, Duration::ZERO), (1, Duration::from_millis(150)), (2, Duration::from_millis(400)), (3, Duration::from_millis(550))]
        );
        r.speaker.stop();
    }

    // ---- a cloud voice failing mid-answer

    type Told = Arc<Mutex<Vec<(String, Vec<String>, bool)>>>;

    /// A fallback reading into `said`, telling `told`.
    fn fallback(said: &Arc<Mutex<Vec<String>>>, local: bool, told: &Told) -> Fallback {
        let told = Arc::clone(told);
        Fallback {
            local: local.then(|| {
                let e: Arc<dyn Engine> = Arc::new(Fake { said: Arc::clone(said), delay: Duration::from_millis(1), fail_on: None });
                (e, String::new())
            }),
            notify: Box::new(move |e, left, took_over| told.lock().unwrap().push((e.to_string(), left.to_vec(), took_over))),
        }
    }

    #[test]
    fn a_failing_cloud_voice_hands_the_rest_to_the_local_voice_once() {
        let r = rig();
        let local_said = Arc::new(Mutex::new(Vec::new()));
        let told: Told = Arc::default();
        let mut job = r.job("a", &["One.", "Two.", "Three."], 1, Some("Two."));
        job.fallback = Some(fallback(&local_said, true, &told));
        r.speaker.speak(job, sim_opener()).unwrap();
        r.wait_quiet();
        // The cloud said the first sentence; the local voice the rest, the
        // failed one included; nothing said twice, the cloud never asked again.
        assert_eq!(*r.said.lock().unwrap(), vec!["One."]);
        assert_eq!(*local_said.lock().unwrap(), vec!["Two.", "Three."]);
        assert_eq!(*told.lock().unwrap(), vec![("synthesis failed".to_string(), vec!["Two.".to_string(), "Three.".to_string()], true)]);
        // Told already: the answer ends without an error of its own.
        assert_eq!(r.events(), vec![ev(true, "a"), ev(false, "a")]);
    }

    #[test]
    fn no_local_voice_means_one_notice_and_silence() {
        let r = rig();
        let local_said = Arc::new(Mutex::new(Vec::new()));
        let told: Told = Arc::default();
        let mut job = r.job("a", &["One.", "Two.", "Three."], 1, Some("One."));
        job.fallback = Some(fallback(&local_said, false, &told));
        r.speaker.speak(job, sim_opener()).unwrap();
        r.wait_quiet();
        assert!(r.said.lock().unwrap().is_empty());
        assert!(local_said.lock().unwrap().is_empty());
        assert_eq!(told.lock().unwrap().len(), 1);
        assert!(!told.lock().unwrap()[0].2);
        assert_eq!(r.events(), vec![ev(true, "a"), ev(false, "a")]);
        // Without a fallback (Settings' preview), the error is the answer's.
        let r = rig();
        r.speaker.speak(r.job("b", &["One."], 1, Some("One.")), sim_opener()).unwrap();
        r.wait_quiet();
        assert_eq!(r.events().last().and_then(|e| e.2.clone()).as_deref(), Some("synthesis failed"));
    }

    /// A streaming synthesiser: `pieces` of 10 ms audio, `gap` apart, each
    /// handed on as it's made; counts the pieces it made.
    struct Streaming {
        pieces: usize,
        gap: Duration,
        made: Arc<AtomicU64>,
    }
    impl Synth for Streaming {
        fn synthesize(&mut self, _text: &str) -> Result<Pcm, String> {
            unreachable!("streams")
        }
        fn stream(&mut self, _text: &str, out: &mut dyn FnMut(Pcm) -> bool) -> Result<(), String> {
            for _ in 0..self.pieces {
                std::thread::sleep(self.gap);
                self.made.fetch_add(1, Ordering::SeqCst);
                if !out(Pcm { sample_rate: 1_000, channels: 1, samples: vec![0.0; 10] }) {
                    break;
                }
            }
            Ok(())
        }
    }
    struct StreamingEngine(Arc<AtomicU64>);
    impl Engine for StreamingEngine {
        fn id(&self) -> &'static str {
            "streaming"
        }
        fn voices(&self) -> Result<Vec<Voice>, String> {
            Ok(Vec::new())
        }
        fn default_voice(&self) -> Option<String> {
            None
        }
        fn open(&self, _voice: &str, _rate: f32) -> Result<Box<dyn Synth>, String> {
            Ok(Box::new(Streaming { pieces: 20, gap: Duration::from_millis(15), made: Arc::clone(&self.0) }))
        }
    }

    #[test]
    fn a_streaming_voice_plays_its_first_piece_before_the_sentence_is_made_and_stops_mid_sentence() {
        let r = rig();
        let made = Arc::new(AtomicU64::new(0));
        let play = Arc::new(SimPlayback::default());
        let p = Arc::clone(&play);
        let opener: Opener = Box::new(move || Ok(Opened { play: p, keep: None }));
        let job = Job {
            message_id: Some("a".into()),
            sentences: vec!["A long sentence.".into()],
            engine: Arc::new(StreamingEngine(Arc::clone(&made))),
            voice: String::new(),
            rate: 1.0,
            fallback: None,
            track: None,
            first: 0,
            feed: None,
        };
        r.speaker.speak(job, opener).unwrap();
        let start = Instant::now();
        while play.appended.load(Ordering::SeqCst) == 0 {
            assert!(start.elapsed() < Duration::from_secs(2), "nothing played");
            std::thread::sleep(Duration::from_millis(2));
        }
        // Playing while most of the sentence is still to come.
        assert!(made.load(Ordering::SeqCst) < 10, "{} pieces made before the first played", made.load(Ordering::SeqCst));
        assert!(r.speaker.stop());
        std::thread::sleep(Duration::from_millis(100));
        // The stop ended the sentence being made, not just the playing.
        assert!(made.load(Ordering::SeqCst) < 20, "made all {} pieces", made.load(Ordering::SeqCst));
        assert_eq!(r.events(), vec![ev(true, "a"), ev(false, "a")]);
    }

    // ---- ElevenLabs through the speaker, against a local stand-in

    use elevenlabs::standin::{pcm_bytes, serial, stub, Reply};

    const KEY: &str = "sk_speaker_test_key_0001";

    fn eleven_job(base: &str, sentences: &[&str], fallback: Option<Fallback>) -> Job {
        Job {
            message_id: Some("a".into()),
            sentences: sentences.iter().map(|s| s.to_string()).collect(),
            engine: Arc::new(elevenlabs::ElevenLabs::new(KEY, "", true).at(base)),
            voice: "JBFqnCBsd6RMkjVDRZzb".into(),
            rate: 1.0,
            fallback,
            track: None,
            first: 0,
            feed: None,
        }
    }

    #[test]
    fn elevenlabs_streams_each_sentence_into_the_output() {
        let _one_at_a_time = serial();
        let r = rig();
        let pcm = || Reply::Pcm { bytes: pcm_bytes(9_600), pieces: vec![4_801, 4_800, 9_599], delay: Duration::ZERO };
        let s = stub(vec![pcm(), pcm()]);
        let play = Arc::new(SimPlayback::default());
        let p = Arc::clone(&play);
        let opener: Opener = Box::new(move || Ok(Opened { play: p, keep: None }));
        r.speaker.speak(eleven_job(&s.base, &["One sentence here.", "And another one."], None), opener).unwrap();
        r.wait_quiet();
        assert_eq!(r.events(), vec![ev(true, "a"), ev(false, "a")]);
        // Each sentence its own request, its audio handed on in pieces.
        let asked = s.requests.lock().unwrap().clone();
        assert_eq!(asked.len(), 2);
        assert!(asked[0].contains(r#""text":"One sentence here.""#) && asked[1].contains(r#""text":"And another one.""#));
        assert!(play.appended.load(Ordering::SeqCst) >= 4, "{} pieces", play.appended.load(Ordering::SeqCst));
    }

    #[test]
    fn elevenlabs_failing_hands_over_to_the_local_voice_and_is_remembered() {
        let _one_at_a_time = serial();
        let r = rig();
        let s = stub(vec![
            Reply::Pcm { bytes: pcm_bytes(4_800), pieces: vec![9_600], delay: Duration::ZERO },
            Reply::Error(401, r#"{"detail":{"type":"authentication_error","code":"invalid_api_key","message":"Invalid API key"}}"#.into()),
        ]);
        let local_said = Arc::new(Mutex::new(Vec::new()));
        let told: Told = Arc::default();
        let job = eleven_job(&s.base, &["First sentence works.", "Then the key fails.", "And one more."], Some(fallback(&local_said, true, &told)));
        r.speaker.speak(job, sim_opener()).unwrap();
        r.wait_quiet();
        // Asked twice (never a third time, never a retry), then the local voice.
        assert_eq!(s.requests.lock().unwrap().len(), 2);
        assert_eq!(*local_said.lock().unwrap(), vec!["Then the key fails.", "And one more."]);
        let told = told.lock().unwrap().clone();
        assert_eq!(told, vec![("ElevenLabs: API key not accepted".to_string(), vec!["Then the key fails.".to_string(), "And one more.".to_string()], true)]);
        assert_eq!(r.events(), vec![ev(true, "a"), ev(false, "a")]);
        // On record: the next answers skip ElevenLabs for a while.
        let key = elevenlabs::Key::new(KEY).fingerprint();
        let choice = elevenlabs::ElevenLabs::choice_fingerprint(KEY, "JBFqnCBsd6RMkjVDRZzb", "");
        assert!(cloud::with_health(|h| h.skipping(elevenlabs::ID, key, choice, Instant::now())));
        cloud::with_health(|h| h.worked(elevenlabs::ID, key));
    }

    /// First audio — from `speak` to the first piece reaching the output —
    /// for Kokoro (an `engines` build with it installed; `YAP_TTS_MODELS`)
    /// cold and warm, and for ElevenLabs' path against a local stand-in that
    /// answers after `YAP_TTS_STANDIN_MS` (default 0: Yap's own share).
    /// Nothing is played. `cargo test --features engines
    /// tts::tests::first_audio_latency -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn first_audio_latency() {
        #[derive(Default)]
        struct Timing {
            first: Mutex<Option<Instant>>,
            inner: SimPlayback,
        }
        impl Playback for Timing {
            fn append(&self, pcm: Pcm) {
                self.first.lock().unwrap().get_or_insert_with(Instant::now);
                self.inner.append(pcm);
            }
            fn stop(&self) {
                self.inner.stop();
            }
            fn done(&self) -> bool {
                self.inner.done()
            }
        }
        let text = "Hi, I'm the voice that reads Chat's answers. Ask me anything, and I'll say what I found.";
        let measure = |engine: Arc<dyn Engine>, voice: &str| -> Duration {
            let r = rig();
            let t = Arc::new(Timing::default());
            let t2 = Arc::clone(&t);
            let opener: Opener = Box::new(move || Ok(Opened { play: t2, keep: None }));
            let job = Job {
                message_id: Some("m".into()),
                sentences: engine.prepare(sentences(text)),
                engine,
                voice: voice.into(),
                rate: 1.0,
                fallback: None,
                track: None,
                first: 0,
                feed: None,
            };
            let start = Instant::now();
            r.speaker.speak(job, opener).unwrap();
            while t.first.lock().unwrap().is_none() {
                assert!(start.elapsed() < Duration::from_secs(60), "no audio");
                std::thread::sleep(Duration::from_millis(1));
            }
            let took = t.first.lock().unwrap().unwrap() - start;
            r.speaker.stop();
            took
        };
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let median = |mut v: Vec<Duration>| {
            v.sort();
            v[v.len() / 2]
        };
        if kokoro::available() {
            kokoro::unload(Duration::from_secs(5));
            let cold = measure(Arc::new(kokoro::Kokoro), kokoro::DEFAULT_VOICE);
            let warm = median((0..5).map(|_| measure(Arc::new(kokoro::Kokoro), kokoro::DEFAULT_VOICE)).collect());
            println!("Kokoro ({}): first audio cold {:.0} ms, warm median {:.0} ms", kokoro::DEFAULT_VOICE, ms(cold), ms(warm));
        } else {
            println!("Kokoro: not available in this build/PC (needs --features engines and YAP_TTS_MODELS)");
        }
        let _one_at_a_time = serial();
        let delay = Duration::from_millis(std::env::var("YAP_TTS_STANDIN_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(0));
        let runs: Vec<Duration> = (0..7)
            .map(|_| {
                let s = stub(vec![Reply::Pcm { bytes: pcm_bytes(48_000), pieces: vec![4_096; 23].into_iter().chain([1_792]).collect(), delay }]);
                measure(eleven_job(&s.base, &[], None).engine, "JBFqnCBsd6RMkjVDRZzb")
            })
            .collect();
        println!("ElevenLabs path, stand-in answering after {} ms: first audio median {:.1} ms", delay.as_millis(), ms(median(runs)));
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

    /// The speaker's real output path (rodio on the default device) playing
    /// **silence** from the stand-in engine — nothing audible: it plays to
    /// its end, and a stop ends it at once. Run with `cargo test
    /// tts::tests::rodio_output_with_silence -- --ignored`.
    #[test]
    #[ignore]
    fn rodio_output_with_silence() {
        let r = rig();
        let job = |id: &str| Job {
            message_id: Some(id.into()),
            sentences: vec!["One two three four.".into(), "Five six.".into()],
            engine: Arc::new(StandIn),
            voice: String::new(),
            rate: 2.0,
            fallback: None,
            track: None,
            first: 0,
            feed: None,
        };
        let started = Instant::now();
        r.speaker.speak(job("a"), rodio_opener(None)).unwrap();
        r.wait_quiet();
        let took = started.elapsed();
        // 6 words at 2×: ~0.75 s of silence played through.
        assert!(took >= Duration::from_millis(600), "ended after {took:?}");
        assert_eq!(r.events(), vec![ev(true, "a"), ev(false, "a")]);
        r.speaker.speak(job("b"), rodio_opener(None)).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        assert!(r.speaker.stop());
        assert!(!r.speaker.state().speaking);
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

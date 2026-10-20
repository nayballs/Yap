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

// The G2P only runs in builds with Kokoro's engine; it's tested in all.
#[cfg_attr(not(feature = "engines"), allow(dead_code))]
mod g2p;
pub mod kokoro;
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
    use super::{Pcm, Synth, Voice};
    use windows::core::HSTRING;
    use windows::Media::SpeechSynthesis::{SpeechAppendedSilence, SpeechSynthesizer, VoiceGender, VoiceInformation};
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
        Ok(Box::new(Windows { synth }))
    }

    impl Synth for Windows {
        fn synthesize(&mut self, text: &str) -> Result<Pcm, String> {
            let stream = self.synth.SynthesizeTextToStreamAsync(&HSTRING::from(text)).and_then(|op| op.get()).map_err(err)?;
            let size = u32::try_from(stream.Size().map_err(err)?).map_err(|_| "Windows speech: too much audio".to_string())?;
            let reader = DataReader::CreateDataReader(&stream.GetInputStreamAt(0).map_err(err)?).map_err(err)?;
            reader.LoadAsync(size).and_then(|op| op.get()).map_err(err)?;
            let mut bytes = vec![0u8; size as usize];
            reader.ReadBytes(&mut bytes).map_err(err)?;
            let _ = stream.Close();
            super::parse_wav(&bytes)
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
    /// Empty the queue and go quiet, now.
    fn stop(&self);
    /// Played to the end (or stopped).
    fn done(&self) -> bool;
}

/// A playback, plus whatever must stay alive (on the worker's thread) for it
/// to play: rodio's output stream.
pub struct Opened {
    pub play: Arc<dyn Playback>,
    pub keep: Option<Box<dyn std::any::Any>>,
}

/// Opens the output, on the worker's thread.
pub type Opener = Box<dyn FnOnce() -> Result<Opened, String> + Send>;

// ---- The voice's volume (Nathan, 2026-10-20: "so loud, even at 1% on the
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

struct RodioPlayback(Arc<rodio::Sink>);

impl Playback for RodioPlayback {
    fn append(&self, pcm: Pcm) {
        self.0.append(rodio::buffer::SamplesBuffer::new(pcm.channels, pcm.sample_rate, pcm.samples));
    }
    fn stop(&self) {
        self.0.stop();
    }
    fn done(&self) -> bool {
        self.0.empty()
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
        Ok(Opened { play: Arc::new(RodioPlayback(sink)), keep: Some(Box::new(stream)) })
    })
}

/// Plays nothing: each sentence takes its length on the clock. (Test mode
/// and the unit tests.)
#[derive(Default)]
pub struct SimPlayback {
    ends: Mutex<Option<Instant>>,
    stopped: std::sync::atomic::AtomicBool,
    appended: AtomicU64,
}

impl Playback for SimPlayback {
    fn append(&self, pcm: Pcm) {
        let mut ends = self.ends.lock().unwrap_or_else(|p| p.into_inner());
        let from = ends.filter(|e| *e > Instant::now()).unwrap_or_else(Instant::now);
        *ends = Some(from + pcm.duration());
        self.appended.fetch_add(1, Ordering::SeqCst);
    }
    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
    }
    fn done(&self) -> bool {
        self.stopped.load(Ordering::SeqCst)
            || self.ends.lock().unwrap_or_else(|p| p.into_inner()).is_none_or(|e| Instant::now() >= e)
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
}

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
        if job.sentences.is_empty() {
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
            .map(|_| ())
            .map_err(|e| {
                self.finish(generation, None, Some(format!("Couldn't start speaking: {e}")));
                "Couldn't start speaking".to_string()
            })
    }

    fn live(&self, generation: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == generation
    }

    fn run(self: Arc<Self>, generation: u64, job: Job, open: Opener) {
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
        match job.engine.open(&job.voice, job.rate) {
            Ok(mut synth) => {
                for sentence in &job.sentences {
                    if !self.live(generation) || (self.busy)() {
                        break;
                    }
                    match synth.synthesize(sentence) {
                        Ok(pcm) if self.live(generation) => opened.play.append(pcm),
                        Ok(_) => break,
                        // Stopped mid-sentence: not a failure.
                        Err(_) if !self.live(generation) => break,
                        Err(e) => {
                            tracing::warn!("Spoken replies: {}", e);
                            error = Some(e);
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Spoken replies: {}", e);
                error = Some(e);
            }
        }
        // Let it play out, unless it's stopped or a dictation starts.
        while self.live(generation) && !opened.play.done() {
            if (self.busy)() {
                opened.play.stop();
                break;
            }
            self.heard_now();
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(opened.keep);
        self.finish(generation, job.message_id, error);
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

/// Start the speaker (lib.rs setup): its state goes to every window. A
/// watcher unloads Kokoro once it's been idle for the speech model's unload
/// time (10 minutes when that's "Never": it holds ~0.6 GB).
pub fn init(app: &tauri::AppHandle) {
    set_volume(crate::config::load().tts_volume);
    let app = app.clone();
    let _ = SPEAKER.set(Speaker::new(
        Box::new(move |s| {
            use tauri::Emitter;
            let _ = app.emit("yap-tts-state", s);
            // The Voice Assistant's answer ended (or was stopped).
            crate::assistant::on_tts_state(&app, s.speaking, s.message_id.as_deref());
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

/// Say `md` (an answer's markdown) with the configured engine, voice, speed
/// and output device. `message_id`: what the pages call it.
pub fn speak(md: &str, message_id: Option<String>) -> Result<(), String> {
    let cfg = crate::config::load();
    let (engine, voice) = chosen(&cfg);
    say(utterances(md), message_id, engine, &voice, cfg.tts_rate, cfg.output_device)
}

/// Why Yap mustn't talk now, if it mustn't: a meeting records (the notes
/// would hear Yap) or a call is on (the people on it could, and a voice over
/// a meeting is too much — Nathan, 2026-10-18). Spoken replies, ▶ and the
/// Voice Assistant (`assistant::off_for_meeting`) all keep quiet then.
pub fn meeting_quiet() -> Option<&'static str> {
    if crate::meeting::is_recording() {
        Some("Yap doesn't talk while it records a meeting")
    } else if crate::meeting_detect::call_live() {
        Some("Yap doesn't talk during a call")
    } else {
        None
    }
}

fn say(
    sentences: Vec<String>,
    message_id: Option<String>,
    engine: Arc<dyn Engine>,
    voice: &str,
    rate: f32,
    device: Option<String>,
) -> Result<(), String> {
    if let Some(why) = meeting_quiet() {
        return Err(why.into());
    }
    let speaker = speaker().ok_or("Spoken replies aren't ready yet")?;
    let rate = if rate.is_finite() && rate > 0.0 { rate.clamp(0.5, 2.0) } else { 1.0 };
    let sentences = engine.prepare(sentences);
    #[cfg(debug_assertions)]
    if crate::e2e::active() && !sentences.is_empty() {
        spoken().push(Spoken {
            message_id: message_id.clone(),
            text: sentences.join(" "),
            sentences: sentences.clone(),
            engine: engine.id().to_string(),
            voice: voice.to_string(),
            rate,
            volume: volume(),
        });
    }
    let opener = if crate::e2e::active() { sim_opener() } else { rodio_opener(device) };
    let job = Job { message_id, sentences, engine, voice: voice.to_string(), rate };
    speaker.speak(job, opener)
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
    /// The configured engine.
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
    tauri::async_runtime::spawn_blocking(move || VoiceList {
        engine: cfg.tts_engine.clone(),
        active: chosen(&cfg).0.id().to_string(),
        engines,
        kokoro: EngineVoices { voices: kokoro::voices(), default_voice: Some(kokoro::DEFAULT_VOICE.into()), error: None },
        kokoro_status: registry::status(&registry::root()),
        windows: windows_voices(),
    })
    .await
    .unwrap_or_else(|e| VoiceList {
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
    let sample = "Hi, I'm the voice that reads Chat's answers. Ask me anything, and I'll say what I found.";
    say(sentences(sample), Some(PREVIEW_ID.into()), e, &voice, rate, cfg.output_device)
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
pub fn tts_volume_set(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    volume: f32,
    save: Option<bool>,
) -> Result<f32, String> {
    use tauri::Emitter;
    set_volume(volume);
    let v = self::volume();
    if save.unwrap_or(false) {
        let mut cfg = crate::config::load();
        cfg.tts_volume = v;
        crate::commands::save_config(app.clone(), state, cfg)?;
        let _ = app.emit("yap-voice-volume", serde_json::json!({ "volume": v }));
    }
    Ok(v)
}

/// The Chat view's 🔊 / 🔇: "Speak replies" on or off, saved the way
/// Settings saves, then `yap-voice-replies {on}` so Settings' copy adopts it
/// (its next auto-save keeps it). Off stops what's being said.
#[tauri::command]
pub fn voice_replies_set(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    on: bool,
) -> Result<bool, String> {
    use tauri::Emitter;
    let mut cfg = crate::config::load();
    cfg.speak_replies = on;
    crate::commands::save_config(app.clone(), state, cfg)?;
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
}

#[cfg(debug_assertions)]
static SPOKEN: Mutex<Vec<Spoken>> = Mutex::new(Vec::new());

#[cfg(debug_assertions)]
fn spoken() -> std::sync::MutexGuard<'static, Vec<Spoken>> {
    SPOKEN.lock().unwrap_or_else(|p| p.into_inner())
}

/// Debug-only, test mode: everything Yap would have said, oldest first;
/// `clear` empties the record after reading it.
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
    fn nothing_to_say_is_an_error() {
        let r = rig();
        assert!(r.speaker.speak(r.job("a", &[], 1, None), sim_opener()).is_err());
        assert!(r.events().is_empty());
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

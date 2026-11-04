//! **Kokoro-82M v1.0**: Yap's natural local voice (hexgrad, Apache-2.0),
//! the onnx-community **fp32** export run on Yap's own ONNX Runtime (`ort`,
//! the one transcribe-rs links) on the **CPU** — DirectML can't run it (a
//! ConvTranspose node fails on both GPUs measured) and int8 is 3.5–4×
//! slower on x86, both from the research; the fp16 export turns longer
//! British sentences into NaN (`registry.rs`). 24,000 Hz mono out.
//!
//! - **The voices**: 54, each a 510×256 style table (`voices/<id>.bin`);
//!   a sentence's style is the row for its token count.
//! - **Text → tokens**: `g2p.rs` (the misaki lexicon, eSpeak NG for the
//!   words it lacks) then Kokoro's own symbol table ([`VOCAB`]); at most
//!   510 tokens a call, a longer sentence cut at a space.
//! - **The engine** (`engines` builds): loaded lazily the first time it
//!   speaks, kept warm, unloaded after it's been idle ([`idle_unload`]);
//!   `min(4, cores / 2)` threads that never spin; a sentence being made is
//!   cut short by [`cancel`] (ONNX Runtime's terminate flag), so a stop or
//!   a dictation starting never waits for it.
//!
//! Test mode has a stand-in (silence, like Windows voices' stand-in) once
//! the files are installed from the e2e suite's local server; a stub build
//! outside test mode has no Kokoro at all and says so.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
#[cfg(feature = "engines")]
use std::time::Instant;

use super::g2p::{Espeak, Lexicon, Phonemizer};
#[cfg(feature = "engines")]
use super::Pcm;
use super::{Engine, Synth, Voice};

pub const ID: &str = "kokoro";
pub const NAME: &str = "Kokoro";
/// What a build without Kokoro says (the stub build: `cargo check`, dev UI work).
pub const NOT_IN_BUILD: &str = "Kokoro is in the full Yap build: this one speaks with Windows voices";
pub const DEFAULT_VOICE: &str = "af_bella";
/// Kokoro v1.0's output rate (the model's own metadata; Voice Mirror played
/// it at 22,050 Hz, ~8% slow and low).
pub const SAMPLE_RATE: u32 = 24_000;
/// Tokens a call takes, without the two pads.
pub const MAX_TOKENS: usize = 510;
const STYLE_DIM: usize = 256;
const STYLE_ROWS: usize = 510;
/// The pause after each sentence, once Kokoro's own padding is trimmed.
const SENTENCE_GAP: Duration = Duration::from_millis(180);

/// Kokoro v1.0's symbol table (its `config.json` vocab = sherpa's
/// `tokens.txt`): `symbol id` pairs, the space included.
const VOCAB: &[(char, i64)] = &[
    (';', 1), (':', 2), (',', 3), ('.', 4), ('!', 5), ('?', 6), ('—', 9), ('…', 10), ('"', 11), ('(', 12),
    (')', 13), ('“', 14), ('”', 15), (' ', 16), ('\u{303}', 17), ('ʣ', 18), ('ʥ', 19), ('ʦ', 20), ('ʨ', 21),
    ('ᵝ', 22), ('ꭧ', 23), ('A', 24), ('I', 25), ('O', 31), ('Q', 33), ('S', 35), ('T', 36), ('W', 39),
    ('Y', 41), ('ᵊ', 42), ('a', 43), ('b', 44), ('c', 45), ('d', 46), ('e', 47), ('f', 48), ('h', 50),
    ('i', 51), ('j', 52), ('k', 53), ('l', 54), ('m', 55), ('n', 56), ('o', 57), ('p', 58), ('q', 59),
    ('r', 60), ('s', 61), ('t', 62), ('u', 63), ('v', 64), ('w', 65), ('x', 66), ('y', 67), ('z', 68),
    ('ɑ', 69), ('ɐ', 70), ('ɒ', 71), ('æ', 72), ('β', 75), ('ɔ', 76), ('ɕ', 77), ('ç', 78), ('ɖ', 80),
    ('ð', 81), ('ʤ', 82), ('ə', 83), ('ɚ', 85), ('ɛ', 86), ('ɜ', 87), ('ɟ', 90), ('ɡ', 92), ('ɥ', 99),
    ('ɨ', 101), ('ɪ', 102), ('ʝ', 103), ('ɯ', 110), ('ɰ', 111), ('ŋ', 112), ('ɳ', 113), ('ɲ', 114),
    ('ɴ', 115), ('ø', 116), ('ɸ', 118), ('θ', 119), ('œ', 120), ('ɹ', 123), ('ɾ', 125), ('ɻ', 126),
    ('ʁ', 128), ('ɽ', 129), ('ʂ', 130), ('ʃ', 131), ('ʈ', 132), ('ʧ', 133), ('ʊ', 135), ('ʋ', 136),
    ('ʌ', 138), ('ɣ', 139), ('ɤ', 140), ('χ', 142), ('ʎ', 143), ('ʒ', 147), ('ʔ', 148), ('ˈ', 156),
    ('ˌ', 157), ('ː', 158), ('ʰ', 162), ('ʲ', 164), ('↓', 169), ('→', 171), ('↗', 172), ('↘', 173),
    ('ᵻ', 177),
];
const SPACE: i64 = 16;

/// Phonemes as Kokoro's token ids (symbols it lacks are dropped, as
/// Kokoro's own pipeline drops them).
#[cfg_attr(not(feature = "engines"), allow(dead_code))]
pub fn tokenize(phonemes: &str) -> Vec<i64> {
    phonemes.chars().filter_map(|c| VOCAB.iter().find(|(s, _)| *s == c).map(|(_, id)| *id)).collect()
}

/// `tokens` in calls of at most [`MAX_TOKENS`], each cut after the last
/// space (else punctuation) that fits.
#[cfg_attr(not(feature = "engines"), allow(dead_code))]
pub fn chunk(tokens: &[i64]) -> Vec<Vec<i64>> {
    let mut out = Vec::new();
    let mut rest = tokens;
    while rest.len() > MAX_TOKENS {
        let head = &rest[..MAX_TOKENS];
        let cut = head
            .iter()
            .rposition(|&t| t == SPACE)
            .or_else(|| head.iter().rposition(|&t| (1..=15).contains(&t)))
            .map_or(MAX_TOKENS, |i| i + 1);
        out.push(trim_spaces(&rest[..cut]));
        rest = &rest[cut..];
    }
    let last = trim_spaces(rest);
    if !last.is_empty() {
        out.push(last);
    }
    out.retain(|c| !c.is_empty());
    out
}

#[cfg_attr(not(feature = "engines"), allow(dead_code))]
fn trim_spaces(t: &[i64]) -> Vec<i64> {
    let start = t.iter().position(|&x| x != SPACE).unwrap_or(t.len());
    let end = t.iter().rposition(|&x| x != SPACE).map_or(start, |i| i + 1);
    t[start..end].to_vec()
}

/// Sentences too short to say well on their own ("Sure!", a one-word
/// heading) join the next one: Kokoro clips a lone word (bm_george's
/// "Sure" came out "Jua" in the research), and a tiny first piece leaves a
/// gap while the next is made. Never past ~300 characters.
pub fn merge_short(sentences: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut carry = String::new();
    for s in sentences {
        let joined = if carry.is_empty() { s } else { format!("{carry} {s}") };
        if joined.split_whitespace().count() < 3 && joined.chars().count() < 300 {
            carry = joined;
        } else {
            out.push(joined);
            carry.clear();
        }
    }
    if !carry.is_empty() {
        match out.last_mut() {
            Some(last) if last.chars().count() + carry.chars().count() < 300 => {
                last.push(' ');
                last.push_str(&carry);
            }
            _ => out.push(carry),
        }
    }
    out
}

/// Kokoro's padding off both ends (anything quieter than -50 dBFS, kept
/// 30 ms in), then [`SENTENCE_GAP`] of silence after.
#[cfg_attr(not(feature = "engines"), allow(dead_code))]
pub fn trim_and_pad(mut samples: Vec<f32>) -> Vec<f32> {
    const FLOOR: f32 = 0.003;
    let keep = (SAMPLE_RATE as usize * 30) / 1000;
    let first = samples.iter().position(|s| s.abs() > FLOOR);
    let last = samples.iter().rposition(|s| s.abs() > FLOOR);
    if let (Some(a), Some(b)) = (first, last) {
        let a = a.saturating_sub(keep);
        let b = (b + keep).min(samples.len() - 1);
        samples = samples[a..=b].to_vec();
    } else {
        samples.clear();
    }
    let gap = (SAMPLE_RATE as u128 * SENTENCE_GAP.as_millis() / 1000) as usize;
    samples.extend(std::iter::repeat_n(0.0, gap));
    samples
}

// ------------------------------------------------------------ the voices

/// Kokoro v1.0's 54 voices: `id`, the voice's name. The id's first letter
/// is its language (a = American English, b = British English, e = Spanish,
/// f = French, h = Hindi, i = Italian, j = Japanese, p = Brazilian
/// Portuguese, z = Mandarin), the second its gender.
pub const VOICES: &[&str] = &[
    "af_heart", "af_alloy", "af_aoede", "af_bella", "af_jessica", "af_kore", "af_nicole", "af_nova", "af_river",
    "af_sarah", "af_sky", "am_adam", "am_echo", "am_eric", "am_fenrir", "am_liam", "am_michael", "am_onyx",
    "am_puck", "am_santa", "bf_alice", "bf_emma", "bf_isabella", "bf_lily", "bm_daniel", "bm_fable", "bm_george",
    "bm_lewis", "ef_dora", "em_alex", "em_santa", "ff_siwis", "hf_alpha", "hf_beta", "hm_omega", "hm_psi",
    "if_sara", "im_nicola", "jf_alpha", "jf_gongitsune", "jf_nezumi", "jf_tebukuro", "jm_kumo", "pf_dora",
    "pm_alex", "pm_santa", "zf_xiaobei", "zf_xiaoni", "zf_xiaoxiao", "zf_xiaoyi", "zm_yunjian", "zm_yunxi",
    "zm_yunxia", "zm_yunyang",
];

/// (BCP-47 tag, the accent as Settings groups it, eSpeak NG's voice for a
/// language read entirely through it).
fn language(id: &str) -> (&'static str, &'static str, Option<&'static str>) {
    match id.chars().next() {
        Some('a') => ("en-US", "American English", None),
        Some('b') => ("en-GB", "British English", None),
        Some('e') => ("es", "Spanish", Some("es")),
        Some('f') => ("fr", "French", Some("fr-fr")),
        Some('h') => ("hi", "Hindi", Some("hi")),
        Some('i') => ("it", "Italian", Some("it")),
        Some('j') => ("ja", "Japanese", None),
        Some('p') => ("pt-BR", "Brazilian Portuguese", Some("pt-br")),
        _ => ("zh", "Mandarin Chinese", None),
    }
}

pub fn is_voice(id: &str) -> bool {
    VOICES.contains(&id)
}

/// **Japanese and Mandarin voices aren't offered** (2026-10-25). Kokoro
/// reads them through its phonemes, and Yap's phonemizer gives it English
/// ones: misaki's English lexicon, else eSpeak NG's English (`g2p.rs`).
/// Kokoro was trained on misaki's Japanese (pyopenjtalk, kana → its own
/// symbols) and Mandarin (pypinyin + jieba, with tones) phonemes, neither of
/// which runs in Rust here, and eSpeak NG's `ja` / `cmn` give IPA Kokoro
/// never saw — so these voices read text as heavily accented English, and a
/// Japanese or Chinese answer as nonsense. A pronunciation step of their own
/// isn't cheap (a dictionary and tokenizer per language, tens of MB), so
/// they're left out of Settings' list; one already chosen (an older Yap
/// offered them) speaks as the default voice ([`spoken_voice`]), and the
/// saved setting stays as it was. Their files still download with the
/// others: an older Yap sharing the data folder may use one.
pub fn listed(id: &str) -> bool {
    is_voice(id) && !matches!(id.chars().next(), Some('j' | 'z'))
}

/// The voice that speaks for the saved `id`: itself when it's listed, else
/// the default ([`listed`]). Pure.
pub fn spoken_voice(id: &str) -> &str {
    if listed(id) {
        id
    } else {
        DEFAULT_VOICE
    }
}

/// Every voice as Settings lists it: "Bella", en-US, female, American English.
pub fn voices() -> Vec<Voice> {
    VOICES
        .iter()
        .filter(|id| listed(id))
        .map(|id| {
            let (lang, accent, _) = language(id);
            let name = id.split_once('_').map_or(*id, |(_, n)| n);
            Voice {
                id: id.to_string(),
                name: super::normalize_name(name),
                language: lang.to_string(),
                gender: if id.chars().nth(1) == Some('m') { "male".into() } else { "female".into() },
                accent: accent.to_string(),
            }
        })
        .collect()
}

/// A voice's style table from its `.bin` (510 rows × 256 f32, little-endian).
#[cfg_attr(not(feature = "engines"), allow(dead_code))]
pub fn read_style(path: &Path) -> Result<Vec<f32>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("Kokoro's voice {}: {e}", path.display()))?;
    if bytes.len() != STYLE_ROWS * STYLE_DIM * 4 {
        return Err(format!("Kokoro's voice {} isn't a voice file", path.display()));
    }
    Ok(bytes.as_chunks::<4>().0.iter().map(|&c| f32::from_le_bytes(c)).collect())
}

/// The style row for a call of `tokens` tokens.
#[cfg_attr(not(feature = "engines"), allow(dead_code))]
pub fn style_row(table: &[f32], tokens: usize) -> &[f32] {
    let row = tokens.min(STYLE_ROWS - 1);
    &table[row * STYLE_DIM..(row + 1) * STYLE_DIM]
}

// ----------------------------------------------------------- the files

/// Where an installed Kokoro's files are.
#[derive(Debug, Clone)]
#[cfg_attr(not(feature = "engines"), allow(dead_code))]
pub struct Files {
    pub model: PathBuf,
    pub voices: PathBuf,
    pub lexicon: PathBuf,
    pub espeak: Option<Espeak>,
}

/// The installed files (`tts/registry.rs`), if Kokoro is installed.
pub fn files() -> Option<Files> {
    let root = super::registry::root();
    let k = super::registry::kokoro_dir(&root);
    super::registry::kokoro_installed(&root).then(|| Files {
        model: k.join(super::registry::MODEL_FILE),
        voices: k.join("voices"),
        lexicon: k.join("lexicon"),
        espeak: super::registry::espeak_installed(&root).then(|| Espeak { dir: super::registry::espeak_dir(&root) }),
    })
}

/// Whether this build can run Kokoro at all (an `engines` build, or test
/// mode's stand-in).
pub fn in_build() -> bool {
    cfg!(feature = "engines") || crate::shell::e2e::active()
}

/// Kokoro can speak now: in this build and installed.
pub fn available() -> bool {
    in_build() && files().is_some()
}

/// How many threads it uses: `min(4, logical cores / 2)`, at least 1.
#[cfg_attr(not(feature = "engines"), allow(dead_code))]
pub fn threads() -> usize {
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    (cores / 2).clamp(1, 4)
}

// ---------------------------------------------------------------- engine

/// The Kokoro engine (`tts::Engine`).
pub struct Kokoro;

impl Engine for Kokoro {
    fn id(&self) -> &'static str {
        ID
    }
    fn voices(&self) -> Result<Vec<Voice>, String> {
        Ok(voices())
    }
    fn default_voice(&self) -> Option<String> {
        Some(DEFAULT_VOICE.into())
    }
    fn open(&self, voice: &str, rate: f32) -> Result<Box<dyn Synth>, String> {
        let voice = spoken_voice(voice);
        if crate::shell::e2e::active() {
            files().ok_or("Kokoro isn't installed")?;
            return Ok(Box::new(super::Silence { rate: rate.clamp(0.5, 3.0) }));
        }
        open_real(voice, rate)
    }
    fn prepare(&self, sentences: Vec<String>) -> Vec<String> {
        merge_short(sentences)
    }
    fn cancel(&self) {
        cancel();
    }
}

/// What a voice reads its text with.
#[cfg_attr(not(feature = "engines"), allow(dead_code))]
fn phonemizer(files: &Files, voice: &str) -> Result<Phonemizer, String> {
    let (_, _, espeak_voice) = language(voice);
    if let (Some(v), Some(espeak)) = (espeak_voice, files.espeak.clone()) {
        return Ok(Phonemizer::Espeak { espeak, voice: v });
    }
    let british = voice.starts_with('b');
    match (lexicon(&files.lexicon, british), files.espeak.clone()) {
        (Ok(lexicon), espeak) => Ok(Phonemizer::English { lexicon, espeak }),
        // A dictionary that won't read: eSpeak NG reads it all.
        (Err(e), Some(espeak)) => {
            tracing::warn!("Kokoro: {e}; reading with eSpeak NG alone");
            Ok(Phonemizer::EspeakEnglish { espeak, british })
        }
        (Err(e), None) => Err(e),
    }
}

static LEXICONS: Mutex<Vec<Arc<Lexicon>>> = Mutex::new(Vec::new());

/// The US or GB lexicon, read once (~3 MB of JSON each) and kept with the
/// model (dropped when it unloads).
#[cfg_attr(not(feature = "engines"), allow(dead_code))]
fn lexicon(dir: &Path, british: bool) -> Result<Arc<Lexicon>, String> {
    let mut cache = LEXICONS.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(l) = cache.iter().find(|l| l.british() == british) {
        return Ok(Arc::clone(l));
    }
    let l = Arc::new(Lexicon::load(dir, british)?);
    cache.push(Arc::clone(&l));
    Ok(l)
}

#[cfg(not(feature = "engines"))]
fn open_real(_voice: &str, _rate: f32) -> Result<Box<dyn Synth>, String> {
    Err(NOT_IN_BUILD.into())
}

#[cfg(not(feature = "engines"))]
pub fn cancel() {}

#[cfg(not(feature = "engines"))]
pub fn idle_unload(_idle: Duration) -> bool {
    LEXICONS.lock().unwrap_or_else(|p| p.into_inner()).clear();
    false
}

#[cfg(not(feature = "engines"))]
pub fn unload(_wait: Duration) -> bool {
    LEXICONS.lock().unwrap_or_else(|p| p.into_inner()).clear();
    true
}

#[cfg(feature = "engines")]
pub use real::{cancel, idle_unload, open_real, unload};

#[cfg(feature = "engines")]
mod real {
    use super::*;
    use ort::session::builder::GraphOptimizationLevel;
    use ort::session::{RunOptions, Session};
    use ort::value::Tensor;

    struct Loaded {
        session: Mutex<Session>,
        ids_input: String,
        style_input: String,
        speed_input: String,
    }

    struct Slot {
        model: Option<Arc<Loaded>>,
        path: PathBuf,
        last_used: Instant,
        /// Synthesisers open now (the speaker's): never unloaded under one.
        users: usize,
    }

    static SLOT: Mutex<Option<Slot>> = Mutex::new(None);
    /// The run in flight, for [`cancel`].
    static RUNNING: Mutex<Option<Arc<RunOptions>>> = Mutex::new(None);

    fn slot() -> std::sync::MutexGuard<'static, Option<Slot>> {
        SLOT.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn load(path: &Path, threads: usize) -> Result<Loaded, String> {
        let started = Instant::now();
        let err = |e: ort::Error<ort::session::builder::SessionBuilder>| format!("Kokoro didn't load: {e}");
        let session = Session::builder()
            .map_err(|e| format!("Kokoro didn't load: {e}"))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(err)?
            .with_intra_threads(threads)
            .map_err(err)?
            .with_inter_threads(1)
            .map_err(err)?
            // Parked threads, not spinning ones: nothing slows a dictation's
            // transcription between sentences.
            .with_intra_op_spinning(false)
            .map_err(err)?
            .with_parallel_execution(false)
            .map_err(err)?
            .commit_from_file(path)
            .map_err(|e| format!("Kokoro didn't load: {e}"))?;
        let names: Vec<String> = session.inputs().iter().map(|i| i.name().to_string()).collect();
        let find = |want: &[&str]| names.iter().find(|n| want.iter().any(|w| n.contains(w))).cloned();
        let ids_input = find(&["input_ids", "tokens"]).ok_or("Kokoro's model has no token input")?;
        let style_input = find(&["style"]).ok_or("Kokoro's model has no style input")?;
        let speed_input = find(&["speed"]).ok_or("Kokoro's model has no speed input")?;
        tracing::info!(threads, ms = started.elapsed().as_millis() as u64, "Kokoro: model loaded");
        Ok(Loaded { session: Mutex::new(session), ids_input, style_input, speed_input })
    }

    /// The warm model (loading it the first time).
    fn model(path: &Path) -> Result<Arc<Loaded>, String> {
        let mut s = slot();
        if let Some(slot) = s.as_mut() {
            if slot.path == path {
                if let Some(m) = &slot.model {
                    slot.last_used = Instant::now();
                    slot.users += 1;
                    return Ok(Arc::clone(m));
                }
            }
        }
        // Loading takes ~1 s: not under the lock.
        drop(s);
        let m = Arc::new(load(path, threads())?);
        let mut s = slot();
        let users = s.as_ref().filter(|x| x.path == path).map_or(0, |x| x.users);
        *s = Some(Slot { model: Some(Arc::clone(&m)), path: path.to_path_buf(), last_used: Instant::now(), users: users + 1 });
        Ok(m)
    }

    fn release() {
        if let Some(s) = slot().as_mut() {
            s.users = s.users.saturating_sub(1);
            s.last_used = Instant::now();
        }
    }

    /// Unload the model (and the lexicons) once it's been idle `idle`.
    /// `true` when it did.
    pub fn idle_unload(idle: Duration) -> bool {
        let mut s = slot();
        let Some(slot) = s.as_mut() else { return false };
        if slot.model.is_none() || slot.users > 0 || slot.last_used.elapsed() < idle {
            return false;
        }
        slot.model = None;
        LEXICONS.lock().unwrap_or_else(|p| p.into_inner()).clear();
        tracing::info!("Kokoro: unloaded after being idle");
        true
    }

    /// Unload now (Kokoro deleted), once the synthesiser being used lets go
    /// (it's stopped first; waits at most `wait`). `true` when it's unloaded.
    pub fn unload(wait: Duration) -> bool {
        let until = Instant::now() + wait;
        loop {
            {
                let mut s = slot();
                match s.as_mut() {
                    None => return true,
                    Some(slot) if slot.users == 0 => {
                        slot.model = None;
                        LEXICONS.lock().unwrap_or_else(|p| p.into_inner()).clear();
                        return true;
                    }
                    Some(_) if Instant::now() >= until => return false,
                    Some(_) => {}
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Stop the sentence being made now (a stop, a new answer, a dictation).
    pub fn cancel() {
        if let Some(run) = RUNNING.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            let _ = run.terminate();
        }
    }

    struct KokoroSynth {
        model: Arc<Loaded>,
        style: Vec<f32>,
        g2p: Phonemizer,
        speed: f32,
    }

    impl Drop for KokoroSynth {
        fn drop(&mut self) {
            release();
        }
    }

    pub fn open_real(voice: &str, rate: f32) -> Result<Box<dyn Synth>, String> {
        let files = files().ok_or("Kokoro isn't installed")?;
        let style = read_style(&files.voices.join(format!("{voice}.bin")))?;
        let g2p = phonemizer(&files, voice)?;
        let model = model(&files.model)?;
        Ok(Box::new(KokoroSynth { model, style, g2p, speed: rate.clamp(0.5, 2.0) }))
    }

    impl KokoroSynth {
        fn run(&self, tokens: &[i64]) -> Result<Vec<f32>, String> {
            let mut ids = Vec::with_capacity(tokens.len() + 2);
            ids.push(0i64);
            ids.extend_from_slice(tokens);
            ids.push(0);
            let n = ids.len() as i64;
            let ids = Tensor::from_array((vec![1i64, n], ids)).map_err(|e| e.to_string())?;
            let style = Tensor::from_array((vec![1i64, STYLE_DIM as i64], style_row(&self.style, tokens.len()).to_vec()))
                .map_err(|e| e.to_string())?;
            let speed = Tensor::from_array((vec![1i64], vec![self.speed])).map_err(|e| e.to_string())?;
            let options = Arc::new(RunOptions::new().map_err(|e| e.to_string())?);
            *RUNNING.lock().unwrap_or_else(|p| p.into_inner()) = Some(Arc::clone(&options));
            let inputs: Vec<(String, ort::session::SessionInputValue<'_>)> = vec![
                (self.model.ids_input.clone(), ids.into()),
                (self.model.style_input.clone(), style.into()),
                (self.model.speed_input.clone(), speed.into()),
            ];
            let out = {
                let mut session = self.model.session.lock().unwrap_or_else(|p| p.into_inner());
                session
                    .run_with_options(inputs, &*options)
                    .map_err(|e| format!("Kokoro: {e}"))
                    .and_then(|outputs| {
                        outputs[0]
                            .try_extract_tensor::<f32>()
                            .map(|(_, data)| data.to_vec())
                            .map_err(|e| format!("Kokoro's audio: {e}"))
                    })
            };
            *RUNNING.lock().unwrap_or_else(|p| p.into_inner()) = None;
            out
        }
    }

    impl Synth for KokoroSynth {
        fn synthesize(&mut self, text: &str) -> Result<Pcm, String> {
            let phonemes = self.g2p.phonemize(text);
            if !phonemes.unknown.is_empty() {
                tracing::debug!(words = phonemes.unknown.len(), "Kokoro: words read by eSpeak NG");
            }
            let tokens = tokenize(&phonemes.text);
            let mut samples = Vec::new();
            for piece in chunk(&tokens) {
                samples.extend(trim_and_pad(self.run(&piece)?));
            }
            if let Some(s) = slot().as_mut() {
                s.last_used = Instant::now();
            }
            Ok(Pcm { sample_rate: SAMPLE_RATE, channels: 1, samples })
        }
    }

    /// The real model on this PC, without a sound: WAVs only.
    #[cfg(test)]
    mod checks {
        use super::*;
        use crate::voice::tts::g2p::Phonemizer;

        /// The research's typical answer (`tts-spike/scripts/paragraph.txt`):
        /// a name, a time, money, a date, "Q3" and a spoken list.
        const ANSWER: &str = "Sure! Your meeting with Priya Shah is on Thursday at 2:30 pm, for about 45 minutes. Before then, three things. One, send her the Q3 budget, which came to £12,400. Two, book a room for four people. Three, reread your notes from October 14th. Want me to draft the email?";
        /// Names and jargon a chat answer about work has, for the G2P.
        const NAMES: &str = "Siobhan and Priya moved the Kubernetes migration to Friday. Ask Imogen about GitHub access, and check the WhatsApp group for Tauri questions.";
        /// The research's first sentence (about 5 s of speech).
        const FIRST: &str = "Your meeting with Priya Shah is on Thursday at two thirty p.m., for about forty-five minutes.";

        fn wav(path: &Path, pcm: &Pcm) {
            let data: Vec<u8> = pcm.samples.iter().flat_map(|s| ((s.clamp(-1.0, 1.0) * 32_767.0) as i16).to_le_bytes()).collect();
            let mut w = Vec::new();
            w.extend_from_slice(b"RIFF");
            w.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
            w.extend_from_slice(b"WAVEfmt ");
            w.extend_from_slice(&16u32.to_le_bytes());
            w.extend_from_slice(&1u16.to_le_bytes());
            w.extend_from_slice(&1u16.to_le_bytes());
            w.extend_from_slice(&pcm.sample_rate.to_le_bytes());
            w.extend_from_slice(&(pcm.sample_rate * 2).to_le_bytes());
            w.extend_from_slice(&2u16.to_le_bytes());
            w.extend_from_slice(&16u16.to_le_bytes());
            w.extend_from_slice(b"data");
            w.extend_from_slice(&(data.len() as u32).to_le_bytes());
            w.extend_from_slice(&data);
            std::fs::write(path, w).unwrap();
        }

        fn english(files: &Files, british: bool) -> Phonemizer {
            Phonemizer::English { lexicon: lexicon(&files.lexicon, british).unwrap(), espeak: files.espeak.clone() }
        }

        fn synth(model: &Arc<Loaded>, files: &Files, voice: &str, g2p: Phonemizer) -> KokoroSynth {
            KokoroSynth { model: Arc::clone(model), style: read_style(&files.voices.join(format!("{voice}.bin"))).unwrap(), g2p, speed: 1.0 }
        }

        /// Say `sentences` one after another into one clip; (audio, time to
        /// the first sentence, total synthesis time).
        fn say(s: &mut KokoroSynth, sentences: &[String]) -> (Pcm, Duration, Duration) {
            let started = Instant::now();
            let mut first = None;
            let mut all = Vec::new();
            for line in sentences {
                let pcm = s.synthesize(line).unwrap();
                assert_eq!(pcm.sample_rate, SAMPLE_RATE);
                if std::env::var_os("YAP_TTS_VERBOSE").is_some() {
                    println!("    {:.2} s ← {line}", pcm.duration().as_secs_f32());
                }
                first.get_or_insert(started.elapsed());
                all.extend(pcm.samples);
            }
            (Pcm { sample_rate: SAMPLE_RATE, channels: 1, samples: all }, first.unwrap_or_default(), started.elapsed())
        }

        /// What Yap says for an answer (markdown out, written for the ear,
        /// sentences, short ones joined), or the same without the writing out.
        fn lines(text: &str, normalised: bool) -> Vec<String> {
            if normalised {
                merge_short(crate::voice::tts::utterances(text))
            } else {
                merge_short(crate::voice::tts::speakable_blocks(text).iter().flat_map(|b| crate::voice::tts::sentences(b)).collect())
            }
        }

        /// Read aloud's karaoke on Kokoro, measured: Kokoro's export gives no
        /// durations, so the words' times come from its audio
        /// (`marks::from_audio`); here they're set against where Parakeet
        /// hears each word in that same audio (its own word times), next to
        /// the plain spread over the length (`marks::estimate`). Nothing is
        /// played. `YAP_TTS_MODELS` = an installed `models/tts`,
        /// `YAP_STT_DATA` = a data folder with Parakeet v3 under `models/`.
        /// `cargo test --features engines tts::kokoro::real::checks::
        /// word_timing_against_parakeet -- --ignored --nocapture`.
        #[test]
        #[ignore]
        fn word_timing_against_parakeet() {
            use crate::voice::tts::marks;
            let files = files().expect("Kokoro installed under YAP_TTS_MODELS");
            let data = PathBuf::from(std::env::var_os("YAP_STT_DATA").expect("YAP_STT_DATA"));
            crate::dictation::stt::apply_accelerator_settings(true);
            let stt = crate::dictation::stt::create_stt_engine(&data, "parakeet-tdt-0.6b-v3", true).expect("Parakeet");
            let model = Arc::new(load(&files.model, threads()).unwrap());
            const TEXTS: &[&str] = &[
                "Reading aloud helps anyone who finds long pages hard going, and it never gets tired.",
                "Your meeting with the design team is on Thursday at 2:30 pm, for about 45 minutes.",
                "Before then, send the budget, book a room for four people, and reread your notes.",
                "The quick brown fox jumps over the lazy dog, then naps in the afternoon sun.",
            ];
            let (mut audio_err, mut plain_err) = (Vec::new(), Vec::new());
            for voice in ["af_bella", "bf_emma", "bm_george"] {
                for text in TEXTS {
                    let said = crate::voice::tts::speaker::said_text(text);
                    let mut s = synth(&model, &files, voice, english(&files, voice.starts_with('b')));
                    let pcm = s.synthesize(&said).unwrap();
                    let spans = marks::words(&said);
                    let (from_audio, how) = marks::from_audio(&said, &spans, &pcm.samples, SAMPLE_RATE, 1);
                    assert_eq!(how, marks::Timing::Audio);
                    let plain = marks::estimate(&said, &spans, Duration::ZERO, pcm.duration());
                    let audio16 = crate::dictation::pipeline::resample_linear(&pcm.samples, SAMPLE_RATE, 16_000);
                    let heard = stt.transcribe_timed(&audio16, Some("en"), false, None).unwrap();
                    // Parakeet's words, matched to the said ones (exact words only).
                    let ptext = heard.words.iter().map(|w| w.text.trim()).collect::<Vec<_>>().join(" ");
                    let pspans = marks::words(&ptext);
                    if pspans.len() != heard.words.len() {
                        continue;
                    }
                    let map = marks::align(&said, &spans, &ptext, &pspans);
                    let key = |t: &str, (a, b): (usize, usize)| -> String {
                        t.chars().skip(a).take(b - a).filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
                    };
                    for (j, m) in map.iter().enumerate() {
                        let Some(i) = *m else { continue };
                        if key(&said, spans[i]) != key(&ptext, pspans[j]) {
                            continue;
                        }
                        let truth = f64::from(heard.words[j].start) * 1000.0;
                        audio_err.push(from_audio[i].as_secs_f64() * 1000.0 - truth);
                        plain_err.push(plain[i].as_secs_f64() * 1000.0 - truth);
                    }
                }
            }
            // (signed: + = the card lights the word after Parakeet hears it)
            let stats = |v: &[f64]| {
                let mut s = v.to_vec();
                s.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let signed = s[(s.len() - 1) / 2];
                let mut a: Vec<f64> = v.iter().map(|e| e.abs()).collect();
                a.sort_by(|x, y| x.partial_cmp(y).unwrap());
                let pick = |q: f64| a[((a.len() - 1) as f64 * q).round() as usize];
                (a.len(), pick(0.5), pick(0.9), a.iter().filter(|&&e| e <= 150.0).count() as f64 / a.len() as f64 * 100.0, signed)
            };
            let (n, med, p90, within, signed) = stats(&audio_err);
            println!("Kokoro, from its audio: {n} words, |error| median {med:.0} ms, 90th {p90:.0} ms, {within:.0}% within 150 ms, signed median {signed:+.0} ms");
            let (n, med, p90, within, signed) = stats(&plain_err);
            println!("Kokoro, plain spread over the length: {n} words, |error| median {med:.0} ms, 90th {p90:.0} ms, {within:.0}% within 150 ms, signed median {signed:+.0} ms");
        }

        /// The same yardstick on Windows' voice, whose word cues are its
        /// own: how far Parakeet's word times sit from the voice's real
        /// ones (Parakeet's own lean), and how the audio-only estimate does
        /// there. `YAP_STT_DATA` as above.
        #[test]
        #[ignore]
        fn windows_cues_against_parakeet() {
            use crate::voice::tts::{marks, Engine, WindowsVoices};
            let data = PathBuf::from(std::env::var_os("YAP_STT_DATA").expect("YAP_STT_DATA"));
            crate::dictation::stt::apply_accelerator_settings(true);
            let stt = crate::dictation::stt::create_stt_engine(&data, "parakeet-tdt-0.6b-v3", true).expect("Parakeet");
            const TEXTS: &[&str] = &[
                "Reading aloud helps anyone who finds long pages hard going, and it never gets tired.",
                "Your meeting with the design team is on Thursday at 2:30 pm, for about 45 minutes.",
                "Before then, send the budget, book a room for four people, and reread your notes.",
                "The quick brown fox jumps over the lazy dog, then naps in the afternoon sun.",
            ];
            let (mut parakeet_vs_cue, mut audio_vs_cue) = (Vec::new(), Vec::new());
            for text in TEXTS {
                let said = crate::voice::tts::speaker::said_text(text);
                let mut s = WindowsVoices.open("", 1.0).unwrap();
                let pcm = s.synthesize(&said).unwrap();
                let cues = s.marks().expect("cues");
                let spans = marks::words(&said);
                let real = marks::from_chars(&spans, &cues).unwrap();
                let (est, _) = marks::from_audio(&said, &spans, &pcm.samples, pcm.sample_rate, pcm.channels);
                let mono: Vec<f32> = if pcm.channels == 2 { pcm.samples.chunks(2).map(|c| (c[0] + c[1]) / 2.0).collect() } else { pcm.samples.clone() };
                let audio16 = crate::dictation::pipeline::resample_linear(&mono, pcm.sample_rate, 16_000);
                let heard = stt.transcribe_timed(&audio16, Some("en"), false, None).unwrap();
                let ptext = heard.words.iter().map(|w| w.text.trim()).collect::<Vec<_>>().join(" ");
                let pspans = marks::words(&ptext);
                if pspans.len() != heard.words.len() {
                    continue;
                }
                let map = marks::align(&said, &spans, &ptext, &pspans);
                let key = |t: &str, (a, b): (usize, usize)| -> String {
                    t.chars().skip(a).take(b - a).filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
                };
                for (j, m) in map.iter().enumerate() {
                    let Some(i) = *m else { continue };
                    if key(&said, spans[i]) == key(&ptext, pspans[j]) {
                        parakeet_vs_cue.push(f64::from(heard.words[j].start) * 1000.0 - real[i].as_secs_f64() * 1000.0);
                    }
                }
                for (i, e) in est.iter().enumerate() {
                    audio_vs_cue.push(e.as_secs_f64() * 1000.0 - real[i].as_secs_f64() * 1000.0);
                }
            }
            let med = |v: &mut Vec<f64>| {
                v.sort_by(|a, b| a.partial_cmp(b).unwrap());
                v[(v.len() - 1) / 2]
            };
            let abs_med = |v: &[f64]| {
                let mut a: Vec<f64> = v.iter().map(|e| e.abs()).collect();
                a.sort_by(|x, y| x.partial_cmp(y).unwrap());
                (a[(a.len() - 1) / 2], a[((a.len() - 1) as f64 * 0.9).round() as usize])
            };
            let (pa, pp) = abs_med(&parakeet_vs_cue);
            let (aa, ap) = abs_med(&audio_vs_cue);
            println!("Windows voice: Parakeet vs the cues: {} words, signed median {:+.0} ms, |error| median {pa:.0} / 90th {pp:.0} ms", parakeet_vs_cue.len(), med(&mut parakeet_vs_cue));
            println!("Windows voice: the audio-only estimate vs the cues: {} words, signed median {:+.0} ms, |error| median {aa:.0} / 90th {ap:.0} ms", audio_vs_cue.len(), med(&mut audio_vs_cue));
        }

        /// Which sentences come out as NaN/silence, per model file and
        /// optimisation level (`YAP_KOKORO_PROBE` = more model files, `;`-separated).
        #[test]
        #[ignore]
        fn nan_probe() {
            let files = files().expect("Kokoro installed under YAP_TTS_MODELS");
            let mut models = vec![files.model.clone()];
            if let Some(more) = std::env::var_os("YAP_KOKORO_PROBE") {
                models.extend(more.to_string_lossy().split(';').map(PathBuf::from));
            }
            let sentences = lines(ANSWER, true);
            for path in models {
                for (name, level) in [
                    ("L3", GraphOptimizationLevel::Level3),
                    ("L1", GraphOptimizationLevel::Level1),
                    ("off", GraphOptimizationLevel::Disable),
                ] {
                    let session = Session::builder()
                        .unwrap()
                        .with_optimization_level(level)
                        .unwrap()
                        .with_intra_threads(4)
                        .unwrap()
                        .commit_from_file(&path)
                        .unwrap();
                    let names: Vec<String> = session.inputs().iter().map(|i| i.name().to_string()).collect();
                    let find = |w: &str| names.iter().find(|n| n.contains(w)).cloned().unwrap();
                    let model = Arc::new(Loaded {
                        ids_input: names.iter().find(|n| n.contains("input_ids") || n.contains("tokens")).cloned().unwrap(),
                        style_input: find("style"),
                        speed_input: find("speed"),
                        session: Mutex::new(session),
                    });
                    for voice in ["bf_emma", "af_bella", "bm_george"] {
                        let s = synth(&model, &files, voice, english(&files, voice.starts_with('b')));
                        let mut report = Vec::new();
                        for l in &sentences {
                            let tokens = tokenize(&s.g2p.phonemize(l).text);
                            let raw = s.run(&tokens).unwrap();
                            let nan = raw.iter().filter(|x| !x.is_finite()).count();
                            report.push(format!("{}/{}", nan, raw.len()));
                        }
                        println!("{} {name} {voice}: NaN/samples per sentence {report:?}", path.file_name().unwrap().to_string_lossy());
                    }
                }
            }
        }

        /// `cargo test --features engines tts::kokoro::real::checks -- --ignored
        /// --nocapture`, with `YAP_TTS_MODELS` = an installed `models/tts`
        /// (the registry's real download) and `YAP_TTS_SAMPLES` = where the
        /// WAVs go (each with its text beside it, for the Parakeet round trip).
        #[test]
        #[ignore]
        fn kokoro_on_this_pc() {
            let files = files().expect("Kokoro installed under YAP_TTS_MODELS");
            let espeak = files.espeak.clone().expect("eSpeak NG installed too");
            let out = std::env::var_os("YAP_TTS_SAMPLES")
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::temp_dir().join("yap-kokoro-check"));
            std::fs::create_dir_all(&out).unwrap();
            println!("CPU threads: {} logical; Yap uses {}", std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0), threads());
            for british in [false, true] {
                let t = Instant::now();
                Lexicon::load(&files.lexicon, british).unwrap();
                println!("{} lexicon read in {} ms", if british { "GB" } else { "US" }, t.elapsed().as_millis());
            }

            // Speed at 1, 2 and 4 threads: load, the first 5 s sentence warm,
            // and the whole answer's real-time factor.
            let mut chosen = None;
            for t in [1usize, 2, 4] {
                let started = Instant::now();
                let model = Arc::new(load(&files.model, t).unwrap());
                let load_ms = started.elapsed().as_millis();
                let mut s = synth(&model, &files, "bf_emma", english(&files, true));
                let cold = Instant::now();
                s.synthesize("Okay.").unwrap();
                let warmup_ms = cold.elapsed().as_millis();
                let t0 = Instant::now();
                let first = s.synthesize(FIRST).unwrap();
                let ttfa = t0.elapsed();
                let (all, first_line, total) = say(&mut s, &lines(ANSWER, true));
                println!(
                    "threads {t}: load {load_ms} ms, first call {warmup_ms} ms; 5 s sentence ({:.2} s of audio) in {:.2} s; answer {:.2} s of audio in {:.2} s = RTF {:.3}, first piece ready after {:.2} s",
                    first.duration().as_secs_f32(),
                    ttfa.as_secs_f32(),
                    all.duration().as_secs_f32(),
                    total.as_secs_f32(),
                    total.as_secs_f32() / all.duration().as_secs_f32(),
                    first_line.as_secs_f32()
                );
                if t == threads() {
                    chosen = Some(model);
                }
            }
            let model = chosen.unwrap_or_else(|| Arc::new(load(&files.model, threads()).unwrap()));

            // A stop mid-sentence: cut short, fast.
            {
                let mut s = synth(&model, &files, "af_bella", english(&files, false));
                let long = "This is a much longer sentence that goes on for quite a while, so that there is time to stop it halfway, which is exactly what this check does to make sure that a stop never waits for the sentence to finish.";
                let canceller = std::thread::spawn(|| {
                    std::thread::sleep(Duration::from_millis(60));
                    let t = Instant::now();
                    cancel();
                    t
                });
                let t0 = Instant::now();
                let r = s.synthesize(long);
                let stopped_at = canceller.join().unwrap();
                println!("cancel: synthesis ended {:?} after the stop ({:?} in all), result ok = {}", stopped_at.elapsed(), t0.elapsed(), r.is_ok());
                assert!(r.is_err(), "the run should have been terminated");
            }

            // The samples: Yap's own output for the maintainer to compare, and the G2P
            // comparison ((a) eSpeak NG for everything, (b) the lexicon with
            // eSpeak NG for the words it lacks), raw and written-for-the-ear.
            let cases: Vec<(&str, &str, Phonemizer, &str, bool)> = vec![
                ("af_bella", "answer", english(&files, false), ANSWER, true),
                ("bf_emma", "answer", english(&files, true), ANSWER, true),
                ("bm_george", "answer", english(&files, true), ANSWER, true),
                ("af_bella", "names", english(&files, false), NAMES, true),
                ("bf_emma", "names", english(&files, true), NAMES, true),
                ("af_bella", "answer-raw", english(&files, false), ANSWER, false),
                ("bf_emma", "answer-raw", english(&files, true), ANSWER, false),
                ("af_bella", "answer-espeak", Phonemizer::EspeakEnglish { espeak: espeak.clone(), british: false }, ANSWER, true),
                ("bf_emma", "answer-espeak", Phonemizer::EspeakEnglish { espeak: espeak.clone(), british: true }, ANSWER, true),
                ("af_bella", "names-espeak", Phonemizer::EspeakEnglish { espeak: espeak.clone(), british: false }, NAMES, true),
                ("bf_emma", "names-espeak", Phonemizer::EspeakEnglish { espeak: espeak.clone(), british: true }, NAMES, true),
            ];
            for (voice, what, g2p, text, normalised) in cases {
                let ls = lines(text, normalised);
                for l in &ls {
                    let p = g2p.phonemize(l);
                    if what.starts_with("names") || what == "answer" && voice == "bf_emma" {
                        println!("  {voice} {what}: {l}\n      → {}{}", p.text, if p.unknown.is_empty() { String::new() } else { format!("   (eSpeak NG: {:?})", p.unknown) });
                    }
                }
                let mut s = synth(&model, &files, voice, g2p);
                let (pcm, first, total) = say(&mut s, &ls);
                let secs = pcm.duration().as_secs_f32();
                let peak = pcm.samples.iter().fold(0f32, |m, x| m.max(x.abs()));
                println!("{voice} {what}: {secs:.2} s of audio at {} Hz, made in {:.2} s (first piece {:.2} s), peak {peak:.2}", pcm.sample_rate, total.as_secs_f32(), first.as_secs_f32());
                assert_eq!(pcm.sample_rate, 24_000);
                let words = text.split_whitespace().count() as f32;
                // ~2–3.5 words a second at 1×.
                assert!((words / 4.0..words / 1.6).contains(&secs), "{secs} s for {words} words");
                assert!(peak > 0.1, "silent");
                let name = format!("{what}_{voice}");
                wav(&out.join(format!("{name}.wav")), &pcm);
                std::fs::write(out.join(format!("{name}.txt")), text).unwrap();
            }
            println!("wrote {}", out.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_symbol_table_matches_kokoros() {
        assert_eq!(VOCAB.len(), 114);
        let mut ids: Vec<i64> = VOCAB.iter().map(|(_, i)| *i).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 114, "unique ids");
        assert_eq!(tokenize("həlˈO, wˈɜɹld!"), vec![50, 83, 54, 156, 31, 3, 16, 65, 156, 87, 123, 54, 46, 5]);
        // Symbols it doesn't have are dropped.
        assert_eq!(tokenize("a❓b"), vec![43, 44]);
    }

    #[test]
    fn long_token_runs_are_cut_at_a_space() {
        let mut t = Vec::new();
        for _ in 0..200 {
            t.extend_from_slice(&[43, 44, 45, SPACE]);
        }
        let chunks = chunk(&t);
        assert!(chunks.len() == 2);
        assert!(chunks.iter().all(|c| c.len() <= MAX_TOKENS && c.first() != Some(&SPACE) && c.last() != Some(&SPACE)));
        assert_eq!(chunks.iter().map(Vec::len).sum::<usize>() + 1, t.len() - 1, "only the cut's space goes");
        assert_eq!(chunk(&[SPACE, 43, SPACE]), vec![vec![43]]);
        assert!(chunk(&[]).is_empty());
    }

    #[test]
    fn short_sentences_join_the_next() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(merge_short(s(&["Sure!", "Your meeting is on Thursday."])), s(&["Sure! Your meeting is on Thursday."]));
        assert_eq!(merge_short(s(&["Plan.", "It's Friday.", "Sam owns it today."])), s(&["Plan. It's Friday.", "Sam owns it today."]));
        // A short last one joins the one before.
        assert_eq!(merge_short(s(&["This is a sentence.", "Yes."])), s(&["This is a sentence. Yes."]));
        assert_eq!(merge_short(s(&["Ok."])), s(&["Ok."]));
    }

    #[test]
    fn padding_is_trimmed_and_a_pause_added() {
        let mut audio = vec![0.0f32; 4800];
        audio.extend(vec![0.5f32; 2400]);
        audio.extend(vec![0.0f32; 4800]);
        let out = trim_and_pad(audio);
        let keep = 24_000 * 30 / 1000;
        let gap = 24_000 * 180 / 1000;
        assert_eq!(out.len(), 2400 + 2 * keep + gap);
        assert_eq!(trim_and_pad(vec![0.0; 100]).len(), gap);
    }

    #[test]
    fn forty_one_voices_listed_grouped_by_language() {
        let v = voices();
        // Kokoro's 54, less the 5 Japanese and 8 Mandarin ones it can't
        // pronounce here (`listed`): 9 languages' voices → 7.
        assert_eq!(VOICES.len(), 54);
        assert_eq!(v.len(), 41);
        let bella = v.iter().find(|x| x.id == DEFAULT_VOICE).unwrap();
        assert_eq!((bella.name.as_str(), bella.language.as_str(), bella.gender.as_str(), bella.accent.as_str()), ("Bella", "en-US", "female", "American English"));
        let george = v.iter().find(|x| x.id == "bm_george").unwrap();
        assert_eq!((george.gender.as_str(), george.accent.as_str()), ("male", "British English"));
        assert_eq!(v.iter().filter(|x| x.accent == "British English").count(), 8);
        assert_eq!(v.iter().filter(|x| x.accent == "American English").count(), 20);
        assert!(!v.iter().any(|x| x.language == "ja" || x.language == "zh"), "no Japanese or Mandarin voice listed");
        assert!(v.iter().any(|x| x.id == "ff_siwis") && v.iter().any(|x| x.id == "hf_alpha"), "eSpeak's languages stay");
        assert!(is_voice("bf_emma") && !is_voice("af") && !is_voice(""));
        // One chosen before (an older Yap offered them): the default speaks,
        // the saved setting untouched.
        assert!(is_voice("jf_gongitsune") && !listed("jf_gongitsune"));
        assert_eq!(spoken_voice("jf_gongitsune"), DEFAULT_VOICE);
        assert_eq!(spoken_voice("zm_yunxi"), DEFAULT_VOICE);
        assert_eq!(spoken_voice("bm_george"), "bm_george");
        assert_eq!(spoken_voice("nonsense"), DEFAULT_VOICE);
    }

    #[test]
    fn style_rows() {
        let table: Vec<f32> = (0..STYLE_ROWS * STYLE_DIM).map(|i| (i / STYLE_DIM) as f32).collect();
        assert_eq!(style_row(&table, 12)[0], 12.0);
        assert_eq!(style_row(&table, 9_999)[0], 509.0);
    }
}

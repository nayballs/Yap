//! **My languages** (2026-10-25; Wispr Flow's languages and its "Speaking a
//! different language?" nudge, OpenWhispr's dual-language hotkeys request,
//! by behaviour): the languages you speak, a language or a translation per
//! shortcut, and a nudge when you speak one you didn't pick. Settings →
//! Dictation → My languages (`settings/MyLanguages.svelte`).
//!
//! - **The languages** (`config.my_languages`, codes in the person's order;
//!   [`my_languages`]: a config from before keeps the one language it had
//!   picked, `selected_language`). Which speech model covers which, and the
//!   one to recommend for them all, is [`catalog`]'s: Parakeet V2 (the
//!   maintainer's) is English only, V3 25 European languages, Whisper all of
//!   them; Settings says which installed model covers which, and never
//!   changes the model.
//! - **The main shortcut** keeps `selected_language`: "auto" — the likeliest
//!   of My languages, for a model that tells (Whisper: [`choose`] over its
//!   own language detection, `stt::SttAdapter::transcribe_spoken`), the
//!   first one for a model that must be told (Canary, Cohere), the model's
//!   own guess for one that recognises it by itself (Parakeet V3,
//!   SenseVoice) — or one fixed language.
//! - **Language shortcuts** (`config.language_shortcuts`, at most
//!   [`MAX_SHORTCUTS`]): a key of its own that dictates in a fixed
//!   language (or any of yours), and optionally pastes it **translated**
//!   into another ([`translate_dictation`]: the transform path,
//!   `llm::transform` on the Commands scope's model — on this PC or in the
//!   cloud — with [`translate_instruction`]; it takes AI cleanup's place,
//!   and the words as said stay one "Use what I said" away). Each key goes
//!   through `input_hook.rs`'s slots (`configure_languages_from`; [`assign`]:
//!   never one that fires with a fixed shortcut; transforms give way to
//!   them) and, in Yap's own windows, the in-page fallback
//!   (`language_key`). A press records exactly as the dictation key does
//!   (toggle or push-to-talk, `Pipeline::on_language_key`): the shortcut is
//!   armed for the press ([`arm`]) and taken by the recording it starts
//!   ([`begin_dictation`], from `capture_target`).
//! - **The dictation's language**: what Whisper heard ([`heard`], kept for
//!   this dictation), else a fixed language. Cleanup is told to keep it
//!   ([`language_line`], just before the transcript, never in the system
//!   prompt the server caches), and the region's spelling line
//!   (`about_you`) is for English only ([`cleanup_spelling`]).
//! - **The nudge** ([`nudge`]): Whisper sure of a language you didn't pick.
//! - **Hinglish** ([`hinglish`], 2026-10-29): Hindi among My languages and
//!   "Write in Latin letters (Hinglish)" on (`config.hinglish`): Whisper's
//!   Devanagari spelt out in Latin letters on this PC before cleanup
//!   ([`hinglish_text`]), cleanup told to keep it so ([`language_line_with`]),
//!   and anything still in Devanagari after it spelt out too.
//!
//! The log says codes, probabilities and lengths, never the text.

pub mod catalog;
pub mod hinglish;
pub mod nudge;

use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Listener, Manager};

use crate::config::YapConfig;
use crate::dictation::stt::{Heard, Said, Spoken};
use catalog::{normalize, Tells};

/// The most language shortcuts (and the input hook's slots for them).
pub const MAX_SHORTCUTS: usize = 4;
/// A shortcut's language "any of mine" (and the main shortcut's Auto).
pub const AUTO: &str = "auto";
/// The input hook's event for a language shortcut (`{ slot, pressed, at }`).
pub const KEY_EVENT: &str = "language-key";

/// A language shortcut (see the module comment).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LanguageShortcut {
    /// Stable id ("l-…", made by Settings).
    pub id: String,
    /// The language spoken with it: a code, or "auto" (any of mine).
    pub language: String,
    /// Pasted in this language instead (a code), translated by the AI;
    /// empty: as spoken.
    pub translate_to: String,
    /// Its key (`input_hook` spec; empty: none yet).
    pub hotkey: String,
}

impl LanguageShortcut {
    /// The language it dictates in; `None`: any of mine.
    pub fn spoken_code(&self) -> Option<&'static str> {
        normalize(&self.language)
    }

    /// The language it pastes in, when it translates.
    pub fn target(&self) -> Option<&'static str> {
        normalize(&self.translate_to)
    }

    /// "French", "English → Spanish", "Any language → Spanish".
    pub fn name(&self) -> String {
        let from = self.spoken_code().map(catalog::name).unwrap_or_else(|| "Any language".to_string());
        match self.target() {
            Some(to) if Some(to) != self.spoken_code() => format!("{from} → {}", catalog::name(to)),
            _ => from,
        }
    }

    /// It can run: an id, and a language that's known (or "auto").
    fn usable(&self) -> bool {
        !self.id.trim().is_empty()
            && (self.language.trim().eq_ignore_ascii_case(AUTO) || self.spoken_code().is_some())
            && (self.translate_to.trim().is_empty() || self.target().is_some())
    }
}

/// The person's languages, as codes, in their order, each once. A config
/// from before My languages keeps the language it had picked.
pub fn my_languages(cfg: &YapConfig) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for code in cfg.my_languages.iter().filter_map(|c| normalize(c)) {
        if !out.iter().any(|c| c == code) {
            out.push(code.to_string());
        }
    }
    if out.is_empty() {
        if let Some(code) = normalize(&cfg.selected_language) {
            out.push(code.to_string());
        }
    }
    out
}

/// The language shortcuts that can run, in order, each id once, at most
/// [`MAX_SHORTCUTS`].
pub fn shortcuts(cfg: &YapConfig) -> Vec<LanguageShortcut> {
    let mut seen = std::collections::HashSet::new();
    cfg.language_shortcuts
        .iter()
        .filter(|s| s.usable() && seen.insert(s.id.clone()))
        .take(MAX_SHORTCUTS)
        .cloned()
        .collect()
}

/// The shortcut with `id`, if it can run.
pub fn find_shortcut(cfg: &YapConfig, id: &str) -> Option<LanguageShortcut> {
    shortcuts(cfg).into_iter().find(|s| s.id == id)
}

/// What a dictation tells the engine (pure): the shortcut's language; else
/// the main shortcut's fixed one; else one of My languages; else nothing.
pub fn plan(cfg: &YapConfig, shortcut: Option<&LanguageShortcut>) -> Spoken {
    if let Some(code) = shortcut.and_then(LanguageShortcut::spoken_code) {
        return Spoken::Fixed(code.to_string());
    }
    if let Some(code) = normalize(&cfg.selected_language) {
        return Spoken::Fixed(code.to_string());
    }
    let mine = my_languages(cfg);
    if mine.is_empty() {
        Spoken::Auto
    } else {
        Spoken::Among(mine)
    }
}

/// Whether model `id` says which language it heard (Whisper).
pub fn tells(id: &str) -> bool {
    catalog::of_model(id).tells == Tells::Probabilities
}

/// The language to give model `model` for a dictation planned as `spoken`,
/// and the language it heard — from `probs`, the model's own detection
/// (each language's probability, the likeliest first), when it gave one.
/// Pure.
///
/// - **Fixed**: that language, to a model that knows it (one that knows
///   one language, or doesn't know this one, gets nothing).
/// - **Among** (My languages): the likeliest of them by the detection;
///   without one, the first a model that must be told knows, else nothing
///   (the model's own choice).
/// - **Auto**: the detection's likeliest, else nothing (as before).
pub fn choose(model: &str, spoken: &Spoken, probs: Option<&[(String, f32)]>) -> (Option<String>, Option<Heard>) {
    let ml = catalog::of_model(model);
    let top = probs.and_then(|p| p.first()).cloned();
    match spoken {
        Spoken::Fixed(code) => ((ml.tells != Tells::One && ml.covers.covers(code)).then(|| code.clone()), None),
        Spoken::Among(list) => match (&top, probs) {
            (Some((t, tp)), Some(p)) => match p.iter().find(|(c, _)| list.contains(c)) {
                Some((c, cp)) => (Some(c.clone()), Some(Heard { top: t.clone(), p: *tp, picked: c.clone(), picked_p: *cp })),
                None => (None, Some(Heard { top: t.clone(), p: *tp, picked: t.clone(), picked_p: *tp })),
            },
            _ if ml.tells == Tells::Told => (list.iter().find(|c| ml.covers.covers(c)).cloned(), None),
            _ => (None, None),
        },
        Spoken::Auto => match top {
            Some((t, tp)) => (Some(t.clone()), Some(Heard { top: t.clone(), p: tp, picked: t, picked_p: tp })),
            None => (None, None),
        },
    }
}

// ---- this dictation ------------------------------------------------------------------

/// What's known of the dictation now (or last) recording.
#[derive(Debug, Default)]
struct Now {
    /// The language shortcut it was started with.
    shortcut: Option<String>,
    /// The language the model heard (Whisper).
    heard: Option<Heard>,
    /// Its Devanagari was spelt out as Hinglish ([`hinglish_text`]).
    hinglish: bool,
}

/// The shortcut pressed now (until its recording takes it).
static ARMED: Mutex<Option<String>> = Mutex::new(None);
static NOW: Mutex<Now> = Mutex::new(Now { shortcut: None, heard: None, hinglish: false });

fn now() -> std::sync::MutexGuard<'static, Now> {
    NOW.lock().unwrap_or_else(|p| p.into_inner())
}

/// A language shortcut is pressed (`Some(id)`) — the recording its press
/// starts is in its language — or the press is over (`None`).
pub fn arm(id: Option<&str>) {
    *ARMED.lock().unwrap_or_else(|p| p.into_inner()) = id.map(str::to_string);
}

/// A recording starts (`capture_target`): it takes the shortcut pressed for
/// it, if any and `take` (a dictation that's pasted — not the rewrite
/// shortcut's instruction or a question for the Voice Assistant); what the
/// last one heard is forgotten.
pub fn begin_dictation(take: bool) {
    let armed = ARMED.lock().unwrap_or_else(|p| p.into_inner()).take();
    let mut n = now();
    n.shortcut = armed.filter(|_| take);
    n.heard = None;
    n.hinglish = false;
    if n.shortcut.is_some() {
        tracing::info!("Dictation: started with a language shortcut");
    }
    // A test run's staged language for a dictation without an STT
    // (`e2e_language_stage`, then `e2e_dictation`).
    #[cfg(debug_assertions)]
    if let Some(h) = STAGED_HEARD.lock().unwrap_or_else(|p| p.into_inner()).take() {
        n.heard = Some(h);
    }
}

/// The shortcut this dictation was started with.
pub fn shortcut_now(cfg: &YapConfig) -> Option<LanguageShortcut> {
    let id = now().shortcut.clone()?;
    find_shortcut(cfg, &id)
}

/// What this dictation tells the engine ([`plan`]).
pub fn spoken(cfg: &YapConfig) -> Spoken {
    plan(cfg, shortcut_now(cfg).as_ref())
}

/// A dictation's transcription is in: what the model heard is kept for
/// this dictation (its cleanup, its translation) and, if it's a language
/// the person didn't pick, perhaps offered ([`nudge::consider`]). `secs`:
/// its audio's length. Returns the text.
pub fn heard(app: &AppHandle, said: Said, secs: f32, cfg: &YapConfig) -> String {
    if let Some(h) = &said.heard {
        tracing::info!(top = %h.top, p = format!("{:.2}", h.p), picked = %h.picked, "Dictation: the language heard");
        nudge::consider(app, h, secs, &my_languages(cfg), cfg.language_nudge);
    }
    now().heard = said.heard;
    said.text
}

/// Home's Run again transcribed a kept dictation (as the main shortcut
/// would): what the model heard is the language its cleanup keeps, no
/// shortcut's; never a nudge. Returns the text.
pub fn rerun_heard(said: Said) -> String {
    let mut n = now();
    n.shortcut = None;
    n.heard = said.heard;
    n.hinglish = false;
    said.text
}

/// The language this dictation is in, when it's known: what the model heard
/// (the one it transcribed in), else a fixed language the model knows.
pub fn spoken_language(cfg: &YapConfig) -> Option<String> {
    if let Some(h) = now().heard.as_ref() {
        return Some(h.picked.clone());
    }
    match spoken(cfg) {
        Spoken::Fixed(code) if catalog::covers(&cfg.model_size, &code) => Some(code),
        _ => None,
    }
}

// ---- cleanup in the right language ---------------------------------------------------

/// The line before the transcript that keeps cleanup in the dictation's
/// language (pure): the language when it's `known` (none for English, the
/// one-shots' language), else — My languages holding another than English
/// — the languages it may be in.
pub fn language_line(known: Option<&str>, mine: &[String]) -> Option<String> {
    language_line_with(known, mine, false)
}

/// What the language line says of Hindi written as Hinglish.
const HINGLISH_LINE: &str = "This transcript is in Hindi, often mixed with English: write the cleaned text in Latin letters, \
the way Hinglish is typed (hai, nahi, kya, accha), English words in their English spelling, never translating it and never \
writing Devanagari.";

/// [`language_line`], with `hinglish` (Hindi among `mine` and "Write in
/// Latin letters (Hinglish)" on): Hindi is to be written in Latin letters,
/// English words kept English. Without it, exactly [`language_line`]'s. Pure.
pub fn language_line_with(known: Option<&str>, mine: &[String], hinglish: bool) -> Option<String> {
    let hinglish = hinglish && mine.iter().any(|c| c == "hi");
    match known {
        Some("en") => None,
        Some("hi") if hinglish => Some(HINGLISH_LINE.to_string()),
        None if hinglish => Some(format!(
            "This transcript may be in {}: keep it in the language it is in, never translating it; write Hindi in Latin \
             letters, the way Hinglish is typed (hai, nahi, kya), never in Devanagari.",
            catalog::names_joined(mine, "or")
        )),
        Some(code) => {
            let n = catalog::name(code);
            let fillers = catalog::fillers(code).map(|f| format!(", and leave out its filler words ({f})")).unwrap_or_default();
            Some(format!("This transcript is in {n}: write the cleaned text in {n}, never translating it{fillers}."))
        }
        None if mine.iter().any(|c| c != "en") => {
            let fillers: Vec<&str> = mine.iter().filter_map(|c| catalog::fillers(c)).collect();
            let fillers = if fillers.is_empty() { String::new() } else { format!(", and leave out its filler words ({})", fillers.join(", ")) };
            Some(format!(
                "This transcript may be in {}: keep it in the language it is in, never translating it{fillers}.",
                catalog::names_joined(mine, "or")
            ))
        }
        None => None,
    }
}

/// A filler a model left at the very start of a cleaned dictation in one
/// of `codes` — "Ahm, also das Treffen…" → "Also das Treffen…" (Qwen3.5-4B
/// kept it every time, whatever the language line said: docs/model-evals.md
/// "My languages") — taken off, the next word given its capital. Only the
/// language's own fillers (`catalog::fillers`, with or without their
/// accents), only as a word of its own at the start; never all of the
/// text. Pure.
pub fn drop_leading_filler(text: &str, codes: &[&str]) -> String {
    fn plain(s: &str) -> String {
        s.chars()
            .map(|c| match c {
                'ä' | 'à' | 'á' | 'â' | 'ã' => 'a',
                'ö' | 'ò' | 'ó' | 'ô' | 'õ' => 'o',
                'ü' | 'ù' | 'ú' | 'û' => 'u',
                'é' | 'è' | 'ê' | 'ë' => 'e',
                other => other,
            })
            .collect()
    }
    let trimmed = text.trim_start();
    let first: String = trimmed.chars().take_while(|c| c.is_alphabetic() || *c == '-').collect();
    if first.is_empty() {
        return text.to_string();
    }
    let word = plain(&first.to_lowercase());
    let is_filler = codes
        .iter()
        .filter_map(|c| catalog::fillers(c))
        .flat_map(|list| list.split(", "))
        .any(|f| plain(f) == word);
    if !is_filler {
        return text.to_string();
    }
    let rest = trimmed[first.len()..].trim_start_matches([',', '…', '.', ' ']);
    let mut chars = rest.chars();
    match chars.next() {
        Some(c) if c.is_alphanumeric() => c.to_uppercase().chain(chars).collect(),
        _ => text.to_string(),
    }
}

/// [`drop_leading_filler`] for this dictation's cleaned text: its language
/// when known, else My languages (none: the text as it is).
pub fn tidy_cleaned(text: &str, cfg: &YapConfig) -> String {
    let codes: Vec<String> = match spoken_language(cfg) {
        Some(code) => vec![code],
        None => my_languages(cfg),
    };
    let codes: Vec<&str> = codes.iter().map(String::as_str).collect();
    drop_leading_filler(text, &codes)
}

/// [`language_line`] for this dictation, before `names` (the names on
/// screen, `screen_terms::cleanup_line`): the lines cleanup gets just before
/// the transcript, after the style's.
pub fn with_language_line(names: Option<String>, cfg: &YapConfig) -> Option<String> {
    let line = language_line_with(spoken_language(cfg).as_deref(), &my_languages(cfg), cfg.hinglish);
    match (line, names) {
        (Some(l), Some(n)) => Some(format!("{l}\n\n{n}")),
        (l, n) => l.or(n),
    }
}

// ---- Hinglish ------------------------------------------------------------------------

/// Hindi among My languages and "Write in Latin letters (Hinglish)" on.
pub fn hinglish_on(cfg: &YapConfig) -> bool {
    cfg.hinglish && my_languages(cfg).iter().any(|c| c == "hi")
}

/// Whether a dictation in `spoken` (`None`: not known) has its Devanagari
/// written as Hinglish: [`hinglish_on`], and it's in Hindi (or English, or
/// not known) — not Marathi or Nepali, which share the script. Pure.
fn writes_hinglish_in(cfg: &YapConfig, spoken: Option<&str>) -> bool {
    hinglish_on(cfg) && spoken.is_none_or(|c| c == "hi" || c == "en")
}

/// This dictation's `text` with its Devanagari in Latin letters
/// ([`hinglish::to_latin`]) when it's written as Hinglish
/// (`writes_hinglish_in`, the language this dictation is in); else
/// unchanged.
pub fn hinglish_text(text: &str, cfg: &YapConfig) -> String {
    let out = hinglish_text_in(text, cfg, spoken_language(cfg).as_deref());
    if out != text {
        now().hinglish = true;
    }
    out
}

/// AI cleanup's `cleaned` for this dictation, unless it was spelt out as
/// Hinglish and cleanup didn't keep its words (it translated it:
/// [`hinglish::kept_the_words`]) — then `said`, as Yap spelt it out.
pub fn hinglish_kept(said: &str, cleaned: String) -> String {
    if now().hinglish && !hinglish::kept_the_words(said, &cleaned) {
        tracing::warn!(chars = cleaned.chars().count(), "AI cleanup changed a Hinglish dictation's language: the words as spelt out go in");
        return said.to_string();
    }
    cleaned
}

/// [`hinglish_text`] for a dictation known to be in `spoken` (pure).
fn hinglish_text_in(text: &str, cfg: &YapConfig, spoken: Option<&str>) -> String {
    if hinglish::has_devanagari(text) && writes_hinglish_in(cfg, spoken) {
        let out = hinglish::to_latin(text);
        tracing::info!(chars = out.chars().count(), "Dictation: Hindi in Latin letters (Hinglish)");
        out
    } else {
        text.to_string()
    }
}

/// The region's spelling line ("Write in my region's spelling",
/// `about_you::cleanup_spelling`) — English only: none for a dictation known
/// to be in another language.
pub fn cleanup_spelling(cfg: &YapConfig) -> Option<String> {
    match spoken_language(cfg) {
        Some(code) if code != "en" => None,
        _ => crate::ai::about_you::cleanup_spelling(cfg),
    }
}

// ---- translation on a shortcut -------------------------------------------------------

/// The instruction a translating shortcut's dictation goes to the AI with
/// (inside `llm::TRANSFORM_BASE_PROMPT`'s guardrails; measured in
/// docs/model-evals.md, "My languages"). `from`: the language spoken, when
/// known.
pub fn translate_instruction(from: Option<&str>, to: &str) -> String {
    let to = catalog::name(to);
    let spoken = match from {
        Some(code) => format!("dictated in {}", catalog::name(code)),
        None => "dictated".to_string(),
    };
    format!(
        "The text was {spoken}, as speech-to-text wrote it. Translate it into {to}, all of it, keeping its meaning and \
         tone, and write it as clean text: leave out filler words (um, uh, like, you know), false starts and repeated \
         words, and punctuate it. Names of people, places, companies and products, numbers, amounts, times, email \
         addresses and links stay as they are; days and months are written in {to}. A question or a request is \
         translated, never answered. The result is in {to}."
    )
}

/// This dictation's transcript (`raw`) translated, when it was started with
/// a translating shortcut: `None` otherwise (AI cleanup as ever). The AI
/// failing or none set up: the transcript as said, and the person told —
/// a dictation never waits on it.
pub async fn translate_dictation(app: &AppHandle, raw: &str, cfg: &YapConfig) -> Option<String> {
    let sc = shortcut_now(cfg)?;
    let to = sc.target()?;
    if raw.trim().is_empty() {
        return Some(String::new());
    }
    let from = spoken_language(cfg);
    if from.as_deref() == Some(to) {
        // Spoken in the language it pastes in: nothing to translate.
        return None;
    }
    let m = crate::dictation::pipeline::commands_model(cfg);
    let keyed = crate::ai::ai_model::KEYED_PROVIDERS.contains(&m.provider.as_str());
    if m.base_url.is_empty() || (keyed && m.api_key.is_empty()) {
        say_untranslated(app, to, "Set up an AI model in Language Models (on this PC or a provider), and Yap translates as you dictate.");
        tracing::info!(to, "Translate on a shortcut: no AI model; pasted as said");
        return Some(raw.to_string());
    }
    let started = std::time::Instant::now();
    // Into Hindi, for someone who writes it as Hinglish: in Latin letters.
    let hinglish = to == "hi" && hinglish_on(cfg);
    let mut instruction = translate_instruction(from.as_deref(), to);
    if hinglish {
        instruction.push_str(" Write the Hindi in Latin letters, the way Hinglish is typed (hai, nahi, kya), never in Devanagari.");
    }
    let asked = crate::privacy::net::within(
        crate::privacy::net::Feature::Dictation,
        crate::ai::usage::within(
            crate::ai::usage::Feature::Translation,
            crate::ai::llm::transform(&instruction, raw, &m.base_url, &m.api_key, &m.model, &m.provider, m.disable_thinking),
        ),
    )
    .await;
    let asked = asked.map(|out| if hinglish { hinglish::to_latin(&out) } else { out });
    match asked.map(|out| crate::dictation::transforms::clean_output(&out)) {
        Ok(out) if !out.trim().is_empty() => {
            tracing::info!(to, chars = out.chars().count(), ms = started.elapsed().as_millis() as u64, "Translate on a shortcut: translated");
            Some(out)
        }
        Ok(_) | Err(_) => {
            say_untranslated(app, to, &format!("The AI model ({}) didn't answer. Your words went in as you said them.", m.provider));
            tracing::warn!(to, "Translate on a shortcut: the AI failed; pasted as said");
            Some(raw.to_string())
        }
    }
}

/// "Couldn't translate into Spanish": said once, the dictation pasted as
/// said.
fn say_untranslated(app: &AppHandle, to: &str, body: &str) {
    crate::notices::alert::show_soon(
        app,
        crate::notices::alert::Alert {
            kind: "pasteBlocked",
            icon: "clipboard",
            title: format!("Couldn't translate into {}", catalog::name(to)),
            body: body.to_string(),
            action: None,
            timeout_ms: 10_000,
        },
    );
}

// ---- the input hook's slots ----------------------------------------------------------

/// One language shortcut's place in the input hook ([`assign`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot {
    pub id: String,
    /// Its key, or "" — none, or one that clashes.
    pub spec: String,
    /// What it clashes with.
    pub clash: Option<String>,
}

/// The input hook's language slots (pure): each shortcut with a key, in
/// order, unless its key would fire with one of `fixed` (the ten fixed
/// shortcuts, by name: `transforms::action_shortcuts`) or an earlier language
/// shortcut's — then left
/// unbound, with what it clashes with. (Saved transforms give way to these:
/// `transforms::fixed_shortcuts`.)
pub fn assign(fixed: &[(&str, &str)], list: &[LanguageShortcut]) -> Vec<Slot> {
    let clashes = crate::dictation::input_hook::clashes;
    let mut out: Vec<Slot> = Vec::new();
    let mut taken: Vec<(String, String)> = Vec::new();
    for s in list.iter().filter(|s| !s.hotkey.trim().is_empty()).take(MAX_SHORTCUTS) {
        let spec = s.hotkey.trim();
        let clash = fixed
            .iter()
            .find(|(_, other)| clashes(spec, other))
            .map(|(name, _)| format!("the {name}"))
            .or_else(|| taken.iter().find(|(_, other)| clashes(spec, other)).map(|(name, _)| format!("the {name} shortcut")));
        if clash.is_none() {
            taken.push((s.name(), spec.to_string()));
        }
        out.push(Slot { id: s.id.clone(), spec: if clash.is_some() { String::new() } else { spec.to_string() }, clash });
    }
    out
}

// ---- wiring --------------------------------------------------------------------------

/// The input hook's language-shortcut events (`language-key`: a slot went
/// down or up) into the pipeline, as the dictation key's go.
pub fn init(app: &AppHandle) {
    let handle = app.clone();
    app.listen(KEY_EVENT, move |event| {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(event.payload()) else { return };
        let (Some(slot), Some(pressed)) = (v["slot"].as_u64(), v["pressed"].as_bool()) else { return };
        let Some(id) = crate::dictation::input_hook::language_at(slot as usize) else {
            tracing::warn!(slot, "A language shortcut with no shortcut");
            return;
        };
        let at = v["at"].as_u64();
        let state = handle.state::<crate::AppState>();
        let Ok(guard) = state.pipeline.lock() else { return };
        if let Some(p) = guard.as_ref() {
            p.on_language_key(&id, pressed, at);
        }
    });
}

// ---- commands ------------------------------------------------------------------------

/// Every language Yap knows, for Settings' picker: `{ code, name, native }`
/// by English name.
#[tauri::command]
pub fn languages_catalog() -> Vec<catalog::Lang> {
    catalog::LANGUAGES.to_vec()
}

/// Settings → Dictation → My languages, for `languages` (codes, as the page
/// has them) and speech model `model`: the languages, which installed model
/// covers which (`model` first), how `model` learns the language, and the
/// model to recommend for them all.
#[tauri::command]
pub fn my_languages_status(languages: Vec<String>, model: String) -> serde_json::Value {
    let mut mine: Vec<String> = Vec::new();
    for c in languages.iter().filter_map(|c| normalize(c)) {
        if !mine.iter().any(|m| m == c) {
            mine.push(c.to_string());
        }
    }
    let mut installed = crate::dictation::commands::installed_models();
    if !model.is_empty() && !installed.contains(&model) {
        installed.insert(0, model.clone());
    }
    installed.sort_by_key(|m| *m != model);
    let row = |id: &str| {
        let ml = catalog::of_model(id);
        serde_json::json!({
            "id": id,
            "name": crate::dictation::stt::model_name(id),
            "active": id == model,
            "covers": mine.iter().filter(|c| ml.covers.covers(c)).collect::<Vec<_>>(),
            "missing": catalog::missing(id, &mine),
            "tells": ml.tells,
            // Every language it knows, when it's a short list ("English").
            "knows": match ml.covers {
                catalog::Covers::Only(list) if list.len() <= 5 => serde_json::json!(list),
                _ => serde_json::Value::Null,
            },
        })
    };
    let recommended = catalog::recommend(&mine, &installed);
    serde_json::json!({
        "languages": mine.iter().map(|c| serde_json::json!({ "code": c, "name": catalog::name(c) })).collect::<Vec<_>>(),
        "model": row(&model),
        "installed": installed.iter().map(|m| row(m)).collect::<Vec<_>>(),
        "recommended": recommended.map(|r| serde_json::json!({
            "id": r,
            "name": crate::dictation::stt::model_name(r),
            "installed": installed.iter().any(|m| m == r),
        })),
    })
}

/// Live-apply one language shortcut's key (Settings pauses it with "" while
/// a new key is picked, then puts the picked one); a save applies them all.
#[tauri::command]
pub fn configure_language_hotkey(id: String, spec: String) {
    crate::dictation::input_hook::configure_language(&id, &spec);
}

/// A language shortcut went down (`pressed`) or up in one of Yap's own
/// windows (their in-page fallback: the global hook never sees keys while a
/// Yap webview has the focus).
#[tauri::command]
pub fn language_key(state: tauri::State<'_, crate::AppState>, id: String, pressed: bool) {
    if let Ok(guard) = state.pipeline.lock() {
        if let Some(p) = guard.as_ref() {
            p.on_language_key(&id, pressed, None);
        }
    }
}

// ---- test runs (debug builds) --------------------------------------------------------

/// A test run's stand-in for Whisper's detection (the stub engine's
/// `detect_language`): each language's probability, the likeliest first.
#[cfg(debug_assertions)]
static STAND_IN: Mutex<Option<Vec<(String, f32)>>> = Mutex::new(None);
/// A test run's language for the next dictation without an STT
/// (`e2e_dictation`), as if Whisper had heard it.
#[cfg(debug_assertions)]
static STAGED_HEARD: Mutex<Option<Heard>> = Mutex::new(None);

/// The stub engine's detection, in a test run.
#[cfg(debug_assertions)]
pub fn stand_in_detection() -> Option<Vec<(String, f32)>> {
    if !crate::shell::e2e::active() {
        return None;
    }
    STAND_IN.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

/// Debug-only, test mode: the next dictation as if language shortcut
/// `shortcut` started it (`null`: the main shortcut), and the stub engine's
/// detection from now on (`heard`: `[[code, probability], …]`, the likeliest
/// first; `null`: none, as every model but Whisper) — which a dictation
/// without an STT (`e2e_dictation`) takes as heard. `fresh`: the nudge's
/// answers and its last offer forgotten. Returns what's armed.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_language_stage(shortcut: Option<String>, heard: Option<Vec<(String, f32)>>, fresh: Option<bool>) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("e2e_language_stage is for test runs".into());
    }
    arm(shortcut.as_deref());
    let staged = heard.as_ref().and_then(|p| {
        let (t, tp) = p.first()?.clone();
        Some(Heard { top: t.clone(), p: tp, picked: t, picked_p: tp })
    });
    *STAGED_HEARD.lock().unwrap_or_else(|p| p.into_inner()) = staged;
    *STAND_IN.lock().unwrap_or_else(|p| p.into_inner()) = heard;
    if fresh.unwrap_or(false) {
        nudge::reset();
    }
    tracing::info!(armed = shortcut.is_some(), "e2e: language staged");
    Ok(())
}

/// Debug-only, test mode: a transcription is in, `secs` long, and the model
/// heard `heard` (`[[code, probability], …]`, the likeliest first) — what a
/// Whisper dictation hands on, without a microphone: what's kept for its
/// cleanup and the nudge's decision ([`heard`]), for the main shortcut and
/// the model in config. Returns what was heard (the language picked among
/// My languages).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_language_heard(app: AppHandle, heard: Vec<(String, f32)>, secs: f32) -> Result<Option<Heard>, String> {
    if !crate::shell::e2e::active() {
        return Err("e2e_language_heard is for test runs".into());
    }
    let cfg = crate::config::load();
    let (_, h) = choose(&cfg.model_size, &plan(&cfg, None), Some(&heard));
    let _ = self::heard(&app, Said { text: String::new(), heard: h.clone() }, secs, &cfg);
    Ok(h)
}

/// Debug-only: the input hook's language slots as set now (`{ id, key }`,
/// the key in words), and what the last dictation knew of its language.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_language_slots() -> serde_json::Value {
    let n = now();
    serde_json::json!({
        "slots": crate::dictation::input_hook::language_slots()
            .into_iter()
            .map(|(id, key)| serde_json::json!({ "id": id, "key": key }))
            .collect::<Vec<_>>(),
        "shortcut": n.shortcut,
        "heard": n.heard,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(codes: &[&str]) -> Vec<String> {
        codes.iter().map(|c| c.to_string()).collect()
    }

    fn sc(id: &str, language: &str, to: &str, hotkey: &str) -> LanguageShortcut {
        LanguageShortcut { id: id.into(), language: language.into(), translate_to: to.into(), hotkey: hotkey.into() }
    }

    fn probs(list: &[(&str, f32)]) -> Vec<(String, f32)> {
        list.iter().map(|(c, p)| (c.to_string(), *p)).collect()
    }

    #[test]
    fn my_languages_are_codes_each_once_and_an_old_config_keeps_its_language() {
        let cfg = YapConfig { my_languages: v(&["en-GB", "French", "fr", "xx", "de"]), ..Default::default() };
        assert_eq!(my_languages(&cfg), v(&["en", "fr", "de"]));
        // Before My languages: the one language picked on the Speech page.
        let old = YapConfig { selected_language: "es".into(), ..Default::default() };
        assert_eq!(my_languages(&old), v(&["es"]));
        assert!(my_languages(&YapConfig::default()).is_empty());
        // A new config has none, the main shortcut on Auto, the nudge on.
        let fresh: YapConfig = serde_json::from_str("{}").unwrap();
        assert!(fresh.my_languages.is_empty() && fresh.language_shortcuts.is_empty() && fresh.language_nudge);
        assert_eq!(fresh.selected_language, "auto");
    }

    #[test]
    fn what_a_dictation_tells_the_engine() {
        let mut cfg = YapConfig { my_languages: v(&["en", "fr"]), ..Default::default() };
        // The main shortcut on Auto: one of mine.
        assert_eq!(plan(&cfg, None), Spoken::Among(v(&["en", "fr"])));
        // A French shortcut: French, whatever the main one says.
        let french = sc("l-1", "fr", "", "kb:118");
        assert_eq!(plan(&cfg, Some(&french)), Spoken::Fixed("fr".into()));
        // "Any of mine" with a translation: as the main shortcut.
        let any = sc("l-2", "auto", "es", "kb:119");
        assert_eq!(plan(&cfg, Some(&any)), Spoken::Among(v(&["en", "fr"])));
        // The main shortcut fixed to English.
        cfg.selected_language = "en".into();
        assert_eq!(plan(&cfg, None), Spoken::Fixed("en".into()));
        assert_eq!(plan(&cfg, Some(&french)), Spoken::Fixed("fr".into()));
        // Nothing picked: as before My languages.
        assert_eq!(plan(&YapConfig::default(), None), Spoken::Auto);
    }

    #[test]
    fn whisper_picks_the_likeliest_of_my_languages_and_says_what_it_heard() {
        let mine = Spoken::Among(v(&["en", "fr"]));
        // Heard as Welsh, a little: transcribed in the likeliest of mine.
        let p = probs(&[("cy", 0.41), ("en", 0.38), ("fr", 0.02)]);
        let (lang, heard) = choose("large", &mine, Some(&p));
        assert_eq!(lang.as_deref(), Some("en"));
        assert_eq!(heard, Some(Heard { top: "cy".into(), p: 0.41, picked: "en".into(), picked_p: 0.38 }));
        // Fixed: no detection asked, that language.
        assert_eq!(choose("large", &Spoken::Fixed("de".into()), None), (Some("de".into()), None));
        // Auto: Whisper's own pick, now with what it heard.
        let (lang, heard) = choose("turbo", &Spoken::Auto, Some(&probs(&[("ja", 0.9), ("zh", 0.05)])));
        assert_eq!(lang.as_deref(), Some("ja"));
        assert_eq!(heard.unwrap().top, "ja");
        assert_eq!(choose("turbo", &Spoken::Auto, None), (None, None));
    }

    #[test]
    fn a_model_that_must_be_told_gets_the_first_it_knows_and_the_rest_are_left_alone() {
        let mine = Spoken::Among(v(&["it", "fr"]));
        // Canary Flash: no Italian, so French.
        assert_eq!(choose("canary-180m-flash", &mine, None), (Some("fr".into()), None));
        assert_eq!(choose("canary-1b-v2", &mine, None), (Some("it".into()), None));
        // Parakeet V3 and SenseVoice recognise it by themselves.
        assert_eq!(choose("parakeet-tdt-0.6b-v3", &mine, None), (None, None));
        // Parakeet V2 knows English only: nothing to tell it, French or not.
        assert_eq!(choose("parakeet-tdt-0.6b-v2", &Spoken::Fixed("fr".into()), None), (None, None));
        assert_eq!(choose("parakeet-tdt-0.6b-v2", &Spoken::Among(v(&["en"])), None), (None, None));
        // A fixed language a model doesn't know: not passed (Canary would
        // be told a language it can't write).
        assert_eq!(choose("canary-180m-flash", &Spoken::Fixed("it".into()), None), (None, None));
        assert!(tells("large") && tells("small") && !tells("parakeet-tdt-0.6b-v3") && !tells("canary-1b-v2"));
    }

    #[test]
    fn shortcuts_are_named_listed_once_and_at_most_four() {
        assert_eq!(sc("a", "fr", "", "").name(), "French");
        assert_eq!(sc("a", "en", "es", "").name(), "English → Spanish");
        assert_eq!(sc("a", "auto", "de", "").name(), "Any language → German");
        assert_eq!(sc("a", "fr", "fr", "").name(), "French");
        let cfg = YapConfig {
            language_shortcuts: vec![
                sc("a", "fr", "", ""),
                sc("a", "de", "", ""),
                sc("", "fr", "", ""),
                sc("b", "klingon", "", ""),
                sc("c", "auto", "zz", ""),
                sc("d", "en", "es", ""),
                sc("e", "AUTO", "", ""),
                sc("f", "it", "", ""),
                sc("g", "pt", "", ""),
            ],
            ..Default::default()
        };
        let ids: Vec<String> = shortcuts(&cfg).into_iter().map(|s| s.id).collect();
        assert_eq!(ids, v(&["a", "d", "e", "f"]));
        assert_eq!(find_shortcut(&cfg, "d").unwrap().target(), Some("es"));
        assert!(find_shortcut(&cfg, "g").is_none(), "past the fourth");
    }

    #[test]
    fn a_shortcut_never_takes_a_key_a_fixed_shortcut_or_an_earlier_one_has() {
        let fixed = [("dictation hotkey", "kb:120"), ("meeting shortcut", "kb:alt+win+77")];
        let list = [
            sc("a", "fr", "", "kb:ctrl+alt+70"),
            sc("b", "de", "", "kb:120"),
            sc("c", "es", "", "kb:control+alt+70"),
            sc("d", "it", "", ""),
            sc("e", "en", "es", "mouse:4"),
        ];
        let got: Vec<(String, String, Option<String>)> = assign(&fixed, &list).into_iter().map(|s| (s.id, s.spec, s.clash)).collect();
        assert_eq!(
            got,
            vec![
                ("a".into(), "kb:ctrl+alt+70".into(), None),
                ("b".into(), String::new(), Some("the dictation hotkey".into())),
                ("c".into(), String::new(), Some("the French shortcut".into())),
                ("e".into(), "mouse:4".into(), None),
            ]
        );
    }

    #[test]
    fn cleanup_is_told_the_language_and_spelling_is_for_english_only() {
        assert_eq!(
            language_line(Some("fr"), &v(&["en", "fr"])).as_deref(),
            Some("This transcript is in French: write the cleaned text in French, never translating it, and leave out its filler words (euh, bah).")
        );
        // A language with no fillers listed: the language alone.
        assert_eq!(
            language_line(Some("cy"), &v(&["cy"])).as_deref(),
            Some("This transcript is in Welsh: write the cleaned text in Welsh, never translating it.")
        );
        assert_eq!(language_line(Some("en"), &v(&["en", "fr"])), None);
        assert_eq!(
            language_line(None, &v(&["en", "fr", "de"])).as_deref(),
            Some(
                "This transcript may be in English, French or German: keep it in the language it is in, never translating it, \
                 and leave out its filler words (euh, bah, äh, ähm, öh)."
            )
        );
        assert_eq!(language_line(None, &v(&["en"])), None);
        assert_eq!(language_line(None, &[]), None);
    }

    #[test]
    fn hinglish_tells_cleanup_latin_letters_and_changes_nothing_when_off() {
        let mine = v(&["en", "hi"]);
        // On: Hindi heard, or not known with Hindi among mine.
        let heard = language_line_with(Some("hi"), &mine, true).unwrap();
        assert!(heard.contains("Latin letters") && heard.contains("never writing Devanagari") && heard.contains("English words"));
        let maybe = language_line_with(None, &mine, true).unwrap();
        assert!(maybe.starts_with("This transcript may be in English or Hindi") && maybe.contains("Latin letters"));
        // English: no line, Hinglish or not.
        assert_eq!(language_line_with(Some("en"), &mine, true), None);
        // Off, or Hindi not among mine: exactly the line before Hinglish.
        for (known, mine) in [(Some("hi"), v(&["en", "hi"])), (None, v(&["en", "hi"])), (Some("fr"), v(&["en", "fr"])), (None, v(&["en"]))] {
            assert_eq!(language_line_with(known, &mine, false), language_line(known, &mine));
        }
        assert_eq!(language_line_with(None, &v(&["en", "fr"]), true), language_line(None, &v(&["en", "fr"])));
        assert_eq!(
            language_line(Some("hi"), &mine).as_deref(),
            Some("This transcript is in Hindi: write the cleaned text in Hindi, never translating it.")
        );
    }

    #[test]
    fn hinglish_spells_out_a_hindi_dictation_only_when_asked() {
        let text = "कल की मीटिंग तीन बजे है।";
        let mut cfg = YapConfig { my_languages: v(&["en", "hi"]), hinglish: true, ..Default::default() };
        assert!(hinglish_on(&cfg) && writes_hinglish_in(&cfg, None) && writes_hinglish_in(&cfg, Some("hi")));
        assert_eq!(hinglish_text_in(text, &cfg, Some("hi")), "Kal ki meeting teen baje hai.");
        assert_eq!(hinglish_text_in(text, &cfg, None), "Kal ki meeting teen baje hai.");
        // Latin text: as it was.
        assert_eq!(hinglish_text_in("See you at 3", &cfg, Some("hi")), "See you at 3");
        // Heard as Marathi or Nepali (the same script): left alone.
        assert_eq!(hinglish_text_in(text, &cfg, Some("mr")), text);
        // Off, or no Hindi among My languages: Devanagari stays.
        cfg.hinglish = false;
        assert_eq!(hinglish_text_in(text, &cfg, Some("hi")), text);
        let cfg = YapConfig { my_languages: v(&["en", "mr"]), hinglish: true, ..Default::default() };
        assert!(!hinglish_on(&cfg));
        assert_eq!(hinglish_text_in(text, &cfg, None), text);
        // A new config: off.
        let fresh: YapConfig = serde_json::from_str("{}").unwrap();
        assert!(!fresh.hinglish);
    }

    #[test]
    fn a_filler_left_at_the_start_comes_off_in_its_own_language_only() {
        assert_eq!(drop_leading_filler("Ahm, also das Treffen ist verschoben.", &["de"]), "Also das Treffen ist verschoben.");
        assert_eq!(drop_leading_filler("Ähm also das Treffen.", &["de"]), "Also das Treffen.");
        assert_eq!(drop_leading_filler("Euh, donc je voulais dire…", &["en", "fr"]), "Donc je voulais dire…");
        assert_eq!(drop_leading_filler("  euh… la réunion", &["fr"]), "La réunion");
        // Not a filler of these languages, or a word that starts like one.
        assert_eq!(drop_leading_filler("Ahm, also das Treffen.", &["fr"]), "Ahm, also das Treffen.");
        assert_eq!(drop_leading_filler("Euhemerism is a theory.", &["fr"]), "Euhemerism is a theory.");
        assert_eq!(drop_leading_filler("Ähnlich wie gestern.", &["de"]), "Ähnlich wie gestern.");
        // Never all of it, and nothing without a language.
        assert_eq!(drop_leading_filler("Euh.", &["fr"]), "Euh.");
        assert_eq!(drop_leading_filler("Euh, oui", &[]), "Euh, oui");
        assert_eq!(drop_leading_filler("", &["fr"]), "");
    }

    #[test]
    fn the_translation_instruction_names_both_languages_and_never_answers() {
        let i = translate_instruction(Some("en"), "es");
        assert!(i.starts_with("The text was dictated in English, as speech-to-text wrote it. Translate it into Spanish"));
        assert!(i.contains("never answered") && i.ends_with("The result is in Spanish."));
        assert!(translate_instruction(None, "de").starts_with("The text was dictated, as"));
    }
}

//! The languages Yap knows — Whisper's hundred, by their codes, English
//! names and their own names (for the search) — and which speech model
//! covers which, how each model learns the language it hears, and the
//! model to recommend for a set of languages. Pure data and functions.
//!
//! Coverage per model (the registry in `dictation/stt.rs`; transcribe-rs's
//! own capability lists, the models' cards):
//!
//! - **Whisper** (Small, Medium, Large v3, Large v3 Turbo): all of them, and
//!   it **tells** which one it heard, and how sure it is (`Tells::Probabilities`).
//! - **Breeze ASR** (a Whisper for Taiwanese Mandarin): Chinese and English,
//!   and it tells.
//! - **Parakeet V3**: 25 European languages, recognised by itself, never
//!   said (`Tells::Itself`); **SenseVoice**: Chinese, Cantonese, English,
//!   Japanese, Korean, the same way.
//! - **Canary 1B v2** (the same 25), **Canary 180M Flash** (English, German,
//!   Spanish, French), **Cohere** (14): each must be **told** the language
//!   (`Tells::Told`), English if it isn't.
//! - **Parakeet V2**, **Moonshine**, **Distil Whisper**: English only;
//!   **GigaAM**: Russian only (`Tells::One`).

use serde::Serialize;

/// One language: its code (Whisper's: "fr", "yue", "jw"), its English name
/// and its own (empty when it's the same, or for the rarer ones).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Lang {
    pub code: &'static str,
    pub name: &'static str,
    pub native: &'static str,
}

const fn l(code: &'static str, name: &'static str, native: &'static str) -> Lang {
    Lang { code, name, native }
}

/// Every language a speech model of Yap's knows: Whisper's, by English
/// name. (Whisper's own codes: Javanese is "jw", Hebrew "he".)
pub const LANGUAGES: &[Lang] = &[
    l("af", "Afrikaans", "Afrikaans"),
    l("sq", "Albanian", "Shqip"),
    l("am", "Amharic", "አማርኛ"),
    l("ar", "Arabic", "العربية"),
    l("hy", "Armenian", "Հայերեն"),
    l("as", "Assamese", "অসমীয়া"),
    l("az", "Azerbaijani", "Azərbaycanca"),
    l("ba", "Bashkir", "Башҡортса"),
    l("eu", "Basque", "Euskara"),
    l("be", "Belarusian", "Беларуская"),
    l("bn", "Bengali", "বাংলা"),
    l("bs", "Bosnian", "Bosanski"),
    l("br", "Breton", "Brezhoneg"),
    l("bg", "Bulgarian", "Български"),
    l("my", "Burmese", "မြန်မာ"),
    l("yue", "Cantonese", "粵語"),
    l("ca", "Catalan", "Català"),
    l("zh", "Chinese", "中文"),
    l("hr", "Croatian", "Hrvatski"),
    l("cs", "Czech", "Čeština"),
    l("da", "Danish", "Dansk"),
    l("nl", "Dutch", "Nederlands"),
    l("en", "English", ""),
    l("et", "Estonian", "Eesti"),
    l("fo", "Faroese", "Føroyskt"),
    l("fi", "Finnish", "Suomi"),
    l("fr", "French", "Français"),
    l("gl", "Galician", "Galego"),
    l("ka", "Georgian", "ქართული"),
    l("de", "German", "Deutsch"),
    l("el", "Greek", "Ελληνικά"),
    l("gu", "Gujarati", "ગુજરાતી"),
    l("ht", "Haitian Creole", "Kreyòl ayisyen"),
    l("ha", "Hausa", "Hausa"),
    l("haw", "Hawaiian", "ʻŌlelo Hawaiʻi"),
    l("he", "Hebrew", "עברית"),
    l("hi", "Hindi", "हिन्दी"),
    l("hu", "Hungarian", "Magyar"),
    l("is", "Icelandic", "Íslenska"),
    l("id", "Indonesian", "Bahasa Indonesia"),
    l("it", "Italian", "Italiano"),
    l("ja", "Japanese", "日本語"),
    l("jw", "Javanese", "Basa Jawa"),
    l("kn", "Kannada", "ಕನ್ನಡ"),
    l("kk", "Kazakh", "Қазақ"),
    l("km", "Khmer", "ខ្មែរ"),
    l("ko", "Korean", "한국어"),
    l("lo", "Lao", "ລາວ"),
    l("la", "Latin", "Latina"),
    l("lv", "Latvian", "Latviešu"),
    l("ln", "Lingala", "Lingála"),
    l("lt", "Lithuanian", "Lietuvių"),
    l("lb", "Luxembourgish", "Lëtzebuergesch"),
    l("mk", "Macedonian", "Македонски"),
    l("mg", "Malagasy", "Malagasy"),
    l("ms", "Malay", "Bahasa Melayu"),
    l("ml", "Malayalam", "മലയാളം"),
    l("mt", "Maltese", "Malti"),
    l("mi", "Maori", "Māori"),
    l("mr", "Marathi", "मराठी"),
    l("mn", "Mongolian", "Монгол"),
    l("ne", "Nepali", "नेपाली"),
    l("no", "Norwegian", "Norsk"),
    l("nn", "Norwegian Nynorsk", "Nynorsk"),
    l("oc", "Occitan", "Occitan"),
    l("ps", "Pashto", "پښتو"),
    l("fa", "Persian", "فارسی"),
    l("pl", "Polish", "Polski"),
    l("pt", "Portuguese", "Português"),
    l("pa", "Punjabi", "ਪੰਜਾਬੀ"),
    l("ro", "Romanian", "Română"),
    l("ru", "Russian", "Русский"),
    l("sa", "Sanskrit", "संस्कृतम्"),
    l("sr", "Serbian", "Српски"),
    l("sn", "Shona", "chiShona"),
    l("sd", "Sindhi", "سنڌي"),
    l("si", "Sinhala", "සිංහල"),
    l("sk", "Slovak", "Slovenčina"),
    l("sl", "Slovenian", "Slovenščina"),
    l("so", "Somali", "Soomaali"),
    l("es", "Spanish", "Español"),
    l("su", "Sundanese", "Basa Sunda"),
    l("sw", "Swahili", "Kiswahili"),
    l("sv", "Swedish", "Svenska"),
    l("tl", "Tagalog", "Tagalog"),
    l("tg", "Tajik", "Тоҷикӣ"),
    l("ta", "Tamil", "தமிழ்"),
    l("tt", "Tatar", "Татарча"),
    l("te", "Telugu", "తెలుగు"),
    l("th", "Thai", "ไทย"),
    l("bo", "Tibetan", "བོད་སྐད"),
    l("tr", "Turkish", "Türkçe"),
    l("tk", "Turkmen", "Türkmençe"),
    l("uk", "Ukrainian", "Українська"),
    l("ur", "Urdu", "اردو"),
    l("uz", "Uzbek", "Oʻzbekcha"),
    l("vi", "Vietnamese", "Tiếng Việt"),
    l("cy", "Welsh", "Cymraeg"),
    l("yi", "Yiddish", "ייִדיש"),
    l("yo", "Yoruba", "Yorùbá"),
];

/// The language with `code`.
pub fn find(code: &str) -> Option<&'static Lang> {
    LANGUAGES.iter().find(|x| x.code == code)
}

/// A code as Yap keeps it: lowercase, a region or script dropped ("fr-FR" →
/// "fr", "zh-Hans" → "zh"), the ISO codes Whisper writes otherwise ("jv" →
/// "jw", "iw" → "he", "nb" → "no"), and an English name taken too
/// ("French"). `None`: not a language Yap knows.
pub fn normalize(code: &str) -> Option<&'static str> {
    let c = code.trim().to_ascii_lowercase().replace('_', "-");
    if c.is_empty() {
        return None;
    }
    let base = c.split('-').next().unwrap_or("");
    let base = match base {
        "jv" => "jw",
        "iw" => "he",
        "nb" => "no",
        "fil" => "tl",
        other => other,
    };
    if let Some(x) = find(base) {
        return Some(x.code);
    }
    LANGUAGES.iter().find(|x| x.name.eq_ignore_ascii_case(code.trim())).map(|x| x.code)
}

/// The English name of `code` ("French"), or the code itself.
pub fn name(code: &str) -> String {
    find(code).map(|x| x.name.to_string()).unwrap_or_else(|| code.to_string())
}

/// "English", "English and French", "English, French and German".
pub fn names_joined(codes: &[String], last: &str) -> String {
    let names: Vec<String> = codes.iter().map(|c| name(c)).collect();
    match names.split_last() {
        None => String::new(),
        Some((one, [])) => one.clone(),
        Some((tail, rest)) => format!("{} {last} {tail}", rest.join(", ")),
    }
}

/// A language's own filler words, for cleanup's language line
/// (`languages::language_line`): its one-shots name only English ones, and
/// a model kept "Euh, …" and "Ähm, …" at the start of a French or German
/// dictation (docs/model-evals.md, "My languages"). Sounds only, never a
/// word that also means something ("este", "tipo", "no"). English's are the
/// prompt's own.
pub fn fillers(code: &str) -> Option<&'static str> {
    Some(match code {
        "fr" => "euh, bah",
        "es" => "eh, em, mmm",
        "de" => "äh, ähm, öh",
        "it" => "ehm, uhm",
        "pt" => "hum, ãh",
        "nl" => "eh, uhm",
        "pl" => "yyy, eee",
        "sv" => "öh, eh",
        "da" => "øh",
        "no" | "nn" => "eh, ehm",
        "fi" => "öö",
        "cs" => "ehm, eee",
        "ru" => "э-э, эм",
        "uk" => "е-е, ем",
        "tr" => "ıı, eee",
        "ja" => "えーと, あのー",
        "zh" => "嗯, 呃",
        "ko" => "음, 어",
        _ => return None,
    })
}

// ---- the speech models ---------------------------------------------------------------

/// Parakeet V3's and Canary 1B v2's 25 European languages.
pub const EUROPEAN_25: &[&str] = &[
    "bg", "hr", "cs", "da", "nl", "en", "et", "fi", "fr", "de", "el", "hu", "it", "lv", "lt", "mt", "pl", "pt", "ro",
    "sk", "sl", "es", "sv", "ru", "uk",
];
/// Canary 180M Flash's four.
const CANARY_FLASH: &[&str] = &["en", "de", "es", "fr"];
/// Cohere's 14.
const COHERE: &[&str] = &["en", "de", "fr", "it", "es", "pt", "el", "nl", "pl", "ar", "vi", "zh", "ja", "ko"];
/// SenseVoice's five.
const SENSE_VOICE: &[&str] = &["zh", "yue", "en", "ja", "ko"];
const ENGLISH: &[&str] = &["en"];

/// Which languages a model understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Covers {
    /// Every language in [`LANGUAGES`] (Whisper).
    All,
    Only(&'static [&'static str]),
}

impl Covers {
    pub fn covers(self, code: &str) -> bool {
        match self {
            Covers::All => find(code).is_some(),
            Covers::Only(list) => list.contains(&code),
        }
    }
}

/// How a model learns the language it hears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Tells {
    /// Whisper: it detects the language and says which, and how sure it is.
    Probabilities,
    /// It recognises the language by itself but never says which
    /// (Parakeet V3, SenseVoice).
    Itself,
    /// It must be told (Canary, Cohere): English unless it is.
    Told,
    /// One language only.
    One,
}

/// A model's languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelLanguages {
    pub covers: Covers,
    pub tells: Tells,
}

/// The languages of speech model `id` (a registry id; an unknown one is a
/// Whisper model someone added, as `stt::resolve_model` takes it).
pub fn of_model(id: &str) -> ModelLanguages {
    let m = |covers, tells| ModelLanguages { covers, tells };
    match id {
        "small" | "medium" | "large" | "turbo" | "large-v3" | "large-v3-turbo" => m(Covers::All, Tells::Probabilities),
        "breeze-asr" => m(Covers::Only(&["zh", "en"]), Tells::Probabilities),
        "parakeet-tdt-0.6b-v3" => m(Covers::Only(EUROPEAN_25), Tells::Itself),
        "sense-voice-int8" => m(Covers::Only(SENSE_VOICE), Tells::Itself),
        "canary-1b-v2" => m(Covers::Only(EUROPEAN_25), Tells::Told),
        "canary-180m-flash" => m(Covers::Only(CANARY_FLASH), Tells::Told),
        "cohere-int8" => m(Covers::Only(COHERE), Tells::Told),
        "gigaam-v3-e2e-ctc" => m(Covers::Only(&["ru"]), Tells::One),
        // English only: Parakeet V2, Moonshine, Distil Whisper, the old
        // `.en` Whisper models.
        "parakeet-tdt-0.6b-v2" | "moonshine-base" | "distil-large-v3.5" | "base" | "tiny" => m(Covers::Only(ENGLISH), Tells::One),
        _ => m(Covers::All, Tells::Probabilities),
    }
}

/// Whether model `id` understands `code`.
pub fn covers(id: &str, code: &str) -> bool {
    of_model(id).covers.covers(code)
}

/// The codes of `langs` model `id` doesn't understand.
pub fn missing(id: &str, langs: &[String]) -> Vec<String> {
    langs.iter().filter(|c| !covers(id, c)).cloned().collect()
}

/// Models to recommend, best first for what they cover: Parakeet V3 (fast,
/// 25 European languages), SenseVoice (Chinese, Japanese, Korean), then
/// Whisper Large v3 Turbo (every language, and it tells which it heard),
/// then the rest.
const RECOMMEND_ORDER: &[&str] = &[
    "parakeet-tdt-0.6b-v3",
    "sense-voice-int8",
    "turbo",
    "large",
    "medium",
    "small",
    "canary-1b-v2",
    "cohere-int8",
    "gigaam-v3-e2e-ctc",
];

/// The model to recommend for `langs`: the first of [`RECOMMEND_ORDER`]
/// covering every one, an installed one first (`installed`); English alone
/// → Parakeet V2 (the English specialist) if it's installed, else Parakeet
/// V3. `None`: no model covers them all (or there are none).
pub fn recommend(langs: &[String], installed: &[String]) -> Option<&'static str> {
    if langs.is_empty() {
        return None;
    }
    if langs.iter().all(|c| c == "en") {
        return Some(if installed.iter().any(|m| m == "parakeet-tdt-0.6b-v2") { "parakeet-tdt-0.6b-v2" } else { "parakeet-tdt-0.6b-v3" });
    }
    // SenseVoice only where it's the specialist: a Chinese, Japanese,
    // Korean or Cantonese among them.
    let cjk = langs.iter().any(|c| ["zh", "ja", "ko", "yue"].contains(&c.as_str()));
    let fitting: Vec<&'static str> = RECOMMEND_ORDER
        .iter()
        .copied()
        .filter(|id| (cjk || *id != "sense-voice-int8") && langs.iter().all(|c| covers(id, c)))
        .collect();
    fitting.iter().copied().find(|id| installed.iter().any(|m| m == id)).or_else(|| fitting.first().copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(codes: &[&str]) -> Vec<String> {
        codes.iter().map(|c| c.to_string()).collect()
    }

    #[test]
    fn every_code_is_known_once_and_whispers_hundred_are_there() {
        let mut codes: Vec<&str> = LANGUAGES.iter().map(|x| x.code).collect();
        let n = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), n, "a code listed twice");
        assert_eq!(n, 100);
        for list in [EUROPEAN_25, CANARY_FLASH, COHERE, SENSE_VOICE, ENGLISH] {
            for c in list {
                assert!(find(c).is_some(), "{c}");
            }
        }
        // In the English names' order (the picker's).
        assert!(LANGUAGES.windows(2).all(|w| w[0].name < w[1].name), "not in order");
    }

    #[test]
    fn codes_are_normalised_however_they_are_written() {
        assert_eq!(normalize("fr-FR"), Some("fr"));
        assert_eq!(normalize(" EN "), Some("en"));
        assert_eq!(normalize("zh-Hans"), Some("zh"));
        assert_eq!(normalize("pt_BR"), Some("pt"));
        assert_eq!(normalize("jv"), Some("jw"));
        assert_eq!(normalize("iw"), Some("he"));
        assert_eq!(normalize("nb"), Some("no"));
        assert_eq!(normalize("French"), Some("fr"));
        assert_eq!(normalize("yue"), Some("yue"));
        assert_eq!(normalize("auto"), None);
        assert_eq!(normalize("xx"), None);
        assert_eq!(normalize(""), None);
        assert_eq!(name("de"), "German");
        assert_eq!(name("xx"), "xx");
        assert_eq!(names_joined(&v(&["en"]), "and"), "English");
        assert_eq!(names_joined(&v(&["en", "fr"]), "and"), "English and French");
        assert_eq!(names_joined(&v(&["en", "fr", "de"]), "or"), "English, French or German");
    }

    #[test]
    fn which_model_covers_which_language() {
        // Parakeet V2: English only — the maintainer's model, never changed.
        assert!(covers("parakeet-tdt-0.6b-v2", "en"));
        assert!(!covers("parakeet-tdt-0.6b-v2", "fr"));
        assert_eq!(missing("parakeet-tdt-0.6b-v2", &v(&["en", "fr", "de"])), v(&["fr", "de"]));
        // V3: 25 European languages, not Japanese.
        assert!(covers("parakeet-tdt-0.6b-v3", "uk") && covers("parakeet-tdt-0.6b-v3", "fr"));
        assert!(!covers("parakeet-tdt-0.6b-v3", "ja"));
        // Whisper: everything Yap lists, and it tells.
        assert!(covers("turbo", "ja") && covers("large", "cy") && covers("small", "yue"));
        assert!(!covers("turbo", "xx"));
        assert_eq!(of_model("large").tells, Tells::Probabilities);
        assert_eq!(of_model("parakeet-tdt-0.6b-v3").tells, Tells::Itself);
        assert_eq!(of_model("canary-1b-v2").tells, Tells::Told);
        assert_eq!(of_model("moonshine-base").tells, Tells::One);
        assert!(covers("sense-voice-int8", "ko") && !covers("sense-voice-int8", "fr"));
        assert!(covers("canary-180m-flash", "es") && !covers("canary-180m-flash", "it"));
        assert!(covers("cohere-int8", "ar") && !covers("cohere-int8", "ru"));
        assert!(covers("gigaam-v3-e2e-ctc", "ru") && !covers("gigaam-v3-e2e-ctc", "en"));
        assert!(!covers("distil-large-v3.5", "fr"));
        // Every registry model has its languages here.
        for id in crate::dictation::stt::all_model_ids() {
            assert!(covers(id, "en") || covers(id, "ru"), "{id}");
        }
    }

    #[test]
    fn the_model_recommended_covers_every_language_an_installed_one_first() {
        let none: Vec<String> = Vec::new();
        assert_eq!(recommend(&v(&["en", "fr"]), &none), Some("parakeet-tdt-0.6b-v3"));
        assert_eq!(recommend(&v(&["en", "fr", "de"]), &v(&["parakeet-tdt-0.6b-v2"])), Some("parakeet-tdt-0.6b-v3"));
        // An installed Whisper covers them too: the installed one.
        assert_eq!(recommend(&v(&["en", "fr"]), &v(&["large"])), Some("large"));
        // Japanese: SenseVoice, the specialist; Japanese and French: Whisper.
        assert_eq!(recommend(&v(&["en", "ja"]), &none), Some("sense-voice-int8"));
        assert_eq!(recommend(&v(&["fr", "ja"]), &none), Some("turbo"));
        assert_eq!(recommend(&v(&["hi"]), &none), Some("turbo"));
        // English alone: the English specialist when it's there.
        assert_eq!(recommend(&v(&["en"]), &v(&["parakeet-tdt-0.6b-v2"])), Some("parakeet-tdt-0.6b-v2"));
        assert_eq!(recommend(&v(&["en"]), &none), Some("parakeet-tdt-0.6b-v3"));
        assert_eq!(recommend(&none, &none), None);
        assert_eq!(recommend(&v(&["xx"]), &none), None);
    }
}

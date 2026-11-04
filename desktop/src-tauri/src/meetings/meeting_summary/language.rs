//! **The summary in the meeting's language** (2026-10-25; Wispr Flow writes
//! its notes in the language spoken, its top headings in English): which
//! language a meeting was held in, read from its own words, and the line
//! that tells the model to write the notes in it.
//!
//! - **Which language** ([`for_note`], pure): the words of its talk (and the
//!   notes typed beside it) against each language's commonest little words
//!   — "the", "and" / « le », « et » / „der“, „und“ … — counting only words
//!   one language has ([`detect`]); a script of its own (Cyrillic, Greek,
//!   Hangul, kana, Han, Arabic, Hebrew, Devanagari, Thai) says it at once.
//!   Sure only with enough of them and a clear winner. Not sure (too little
//!   said, two languages mixed): the person's first language in My
//!   languages (`dictation/languages.rs`), else English. It reads the text,
//!   so it works whatever speech model transcribed the meeting (Parakeet V3
//!   recognises a language without saying which).
//! - **The line** ([`plan_line`], [`digest_line`], [`notes_line`]): put at
//!   the end of the model's input — the last thing a small model reads — and
//!   none for English (the prompts' own language). The headings stay in
//!   English exactly as the prompts give them: `postcheck_action_plan`,
//!   `parse_digest` and `notes/tasks.rs` read them, as Wispr's do.
//!
//! The log says the code and how it was found, never the words.

use crate::notes::Note;

/// How [`for_note`] found the language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    /// From the meeting's own words.
    Heard,
    /// Not sure from its words: the person's first language.
    Yours,
    /// Not sure, and no languages of the person's: English.
    Default,
}

/// The language a meeting's notes are written in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingLanguage {
    /// Whisper's code ("en", "fr", "es"…), as `languages::catalog` knows it.
    pub code: String,
    pub how: How,
}

impl MeetingLanguage {
    /// English, the prompts' own: no line.
    pub fn is_english(&self) -> bool {
        self.code == "en"
    }

    /// "French".
    pub fn name(&self) -> String {
        crate::dictation::languages::catalog::name(&self.code)
    }
}

/// The commonest little words of each language whose words [`detect`]
/// counts. A word in two lists counts for neither ("non", "va", "es"):
/// only a word one language has says which it is. (Words two languages
/// share — "de", "la", "que" — aren't listed at all.)
const WORDS: &[(&str, &[&str])] = &[
    (
        "en",
        &[
            "the", "and", "is", "are", "was", "were", "to", "of", "that", "it", "we", "you", "this", "have", "has", "will",
            "with", "for", "not", "but", "what", "so", "they", "be", "can", "just", "about", "our", "there", "if", "would",
            "should", "going", "think", "yeah", "okay", "need", "let's", "i'll", "we'll", "don't", "it's",
        ],
    ),
    (
        "fr",
        &[
            "le", "les", "des", "est", "et", "une", "qui", "pas", "nous", "vous", "je", "il", "elle", "ce", "ça", "dans",
            "pour", "avec", "sur", "mais", "du", "au", "aux", "sont", "avons", "faut", "oui", "donc", "alors", "très",
            "aussi", "peut", "être", "faire", "cette", "ces", "notre", "leur", "bien", "quoi", "comme", "c'est", "j'ai",
            "va", "d'accord", "voilà", "fait", "non", "ma",
        ],
    ),
    (
        "es",
        &[
            "el", "los", "las", "es", "y", "una", "del", "por", "para", "con", "sí", "pero", "muy", "también", "está",
            "están", "son", "tenemos", "hay", "vamos", "porque", "esto", "eso", "esta", "este", "como", "qué", "cuando",
            "nosotros", "yo", "lo", "al", "su", "puedo", "bueno", "entonces", "vale", "hacer", "puede", "va", "uno", "cosa",
        ],
    ),
    (
        "de",
        &[
            "der", "die", "das", "und", "ist", "nicht", "ich", "wir", "sie", "ein", "eine", "zu", "mit", "auf", "für", "den",
            "dem", "auch", "aber", "wie", "noch", "sind", "haben", "wird", "dass", "oder", "kann", "schon", "mal", "ja",
            "genau", "gut", "bis", "nach", "von", "bei", "uns", "müssen", "machen", "es", "so", "was", "du", "als",
        ],
    ),
    (
        "it",
        &[
            "il", "gli", "che", "è", "di", "non", "per", "sono", "uno", "della", "anche", "ma", "questo", "questa",
            "abbiamo", "perché", "molto", "poi", "fare", "ci", "allora", "quindi", "bene", "cosa", "siamo", "noi", "io",
            "facciamo", "al", "con",
        ],
    ),
    (
        "pt",
        &[
            "o", "os", "não", "é", "uma", "da", "dos", "das", "em", "na", "com", "mas", "muito", "também", "isso", "isto",
            "está", "estão", "são", "temos", "vamos", "então", "eu", "nós", "você", "ele", "ela", "sim", "bom", "aqui",
            "já", "para", "este", "esta", "como",
        ],
    ),
    (
        "nl",
        &[
            "het", "een", "is", "niet", "ik", "we", "wij", "je", "jij", "dat", "van", "op", "voor", "met", "maar", "ook",
            "zijn", "hebben", "wordt", "moet", "nog", "wel", "dus", "goed", "heel", "er", "naar", "om", "als", "hoe", "wat",
        ],
    ),
    (
        "pl",
        &[
            "w", "z", "na", "się", "nie", "jest", "to", "że", "co", "jak", "ale", "tak", "ten", "ta", "mamy", "będzie",
            "czy", "już", "bo", "dla", "po", "przez", "tylko", "też", "może", "trzeba", "dobrze", "więc",
        ],
    ),
    (
        "sv",
        &[
            "och", "att", "det", "är", "som", "ett", "inte", "jag", "vi", "du", "på", "med", "för", "men", "har", "ska",
            "den", "om", "så", "också", "bara", "eller", "här", "nu", "var", "vad", "hur", "mycket",
        ],
    ),
];

/// At least this many counted words before the words decide…
const MIN_HITS: usize = 8;
/// …the winner's share of them all…
const MIN_SHARE: f32 = 0.6;
/// …and at least this many times the runner-up's.
const MIN_LEAD: usize = 2;
/// A meeting is read up to this many words (the opening says it).
const MAX_WORDS: usize = 4_000;

/// Each word's languages (a word in two counts for neither).
fn word_languages() -> &'static std::collections::HashMap<&'static str, Option<&'static str>> {
    static MAP: std::sync::OnceLock<std::collections::HashMap<&'static str, Option<&'static str>>> = std::sync::OnceLock::new();
    MAP.get_or_init(|| {
        let mut map: std::collections::HashMap<&'static str, Option<&'static str>> = std::collections::HashMap::new();
        for (code, words) in WORDS {
            for w in *words {
                map.entry(*w)
                    .and_modify(|c| {
                        if *c != Some(*code) {
                            *c = None;
                        }
                    })
                    .or_insert(Some(*code));
            }
        }
        map
    })
}

/// The language of a script of its own, when most of `text`'s letters are
/// in one.
fn by_script(text: &str) -> Option<&'static str> {
    let (mut latin, mut cyrillic, mut ukrainian, mut greek, mut hebrew, mut arabic) = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);
    let (mut devanagari, mut thai, mut hangul, mut kana, mut han) = (0usize, 0usize, 0usize, 0usize, 0usize);
    for c in text.chars().filter(|c| c.is_alphabetic()) {
        match c as u32 {
            0x0041..=0x024F => latin += 1,
            0x0400..=0x04FF => {
                cyrillic += 1;
                if "іїєґІЇЄҐ".contains(c) {
                    ukrainian += 1;
                }
            }
            0x0370..=0x03FF => greek += 1,
            0x0590..=0x05FF => hebrew += 1,
            0x0600..=0x06FF => arabic += 1,
            0x0900..=0x097F => devanagari += 1,
            0x0E00..=0x0E7F => thai += 1,
            0xAC00..=0xD7AF | 0x1100..=0x11FF | 0x3130..=0x318F => hangul += 1,
            0x3040..=0x30FF => kana += 1,
            0x3400..=0x4DBF | 0x4E00..=0x9FFF => han += 1,
            _ => {}
        }
    }
    let total = latin + cyrillic + greek + hebrew + arabic + devanagari + thai + hangul + kana + han;
    if total < 20 {
        return None;
    }
    let most = |n: usize| n * 2 > total;
    if most(cyrillic) {
        return Some(if ukrainian * 50 >= cyrillic { "uk" } else { "ru" });
    }
    if most(kana + han) {
        return Some(if kana * 10 >= kana + han { "ja" } else { "zh" });
    }
    [(greek, "el"), (hebrew, "he"), (arabic, "ar"), (devanagari, "hi"), (thai, "th"), (hangul, "ko")]
        .into_iter()
        .find(|(n, _)| most(*n))
        .map(|(_, code)| code)
}

/// The language `text` is in, when its words say so clearly (pure): a
/// script of its own, else the little words only one language has — enough
/// of them, most of them one language's, well ahead of the next.
pub fn detect(text: &str) -> Option<&'static str> {
    if let Some(code) = by_script(text) {
        return Some(code);
    }
    let ranked = word_counts(text);
    let total: usize = ranked.iter().map(|r| r.1).sum();
    let (best, n) = *ranked.first()?;
    let second = ranked.get(1).map(|r| r.1).unwrap_or(0);
    let sure = n >= MIN_HITS && n as f32 >= total as f32 * MIN_SHARE && n >= second * MIN_LEAD;
    sure.then_some(best)
}

/// The language most of `text`'s little words are, however few (pure):
/// for judging notes too terse for [`detect`] to be sure of ("Tell the
/// support team about the new date" — the model evaluations).
#[cfg(debug_assertions)]
pub fn leading(text: &str) -> Option<&'static str> {
    by_script(text).or_else(|| {
        let ranked = word_counts(text);
        match (ranked.first(), ranked.get(1)) {
            (Some(a), Some(b)) if a.1 == b.1 => None,
            (Some(a), _) => Some(a.0),
            _ => None,
        }
    })
}

/// Each language's count of `text`'s little words, the most first.
fn word_counts(text: &str) -> Vec<(&'static str, usize)> {
    let map = word_languages();
    let mut counts: std::collections::HashMap<&'static str, usize> = std::collections::HashMap::new();
    let lower = text.to_lowercase().replace('’', "'");
    for w in lower
        .split(|c: char| !(c.is_alphabetic() || c == '\''))
        .map(|w| w.trim_matches('\''))
        .filter(|w| !w.is_empty())
        .take(MAX_WORDS)
    {
        if let Some(Some(code)) = map.get(w) {
            *counts.entry(*code).or_default() += 1;
        }
    }
    let mut ranked: Vec<(&'static str, usize)> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    ranked
}

/// The words a meeting is judged by: its talk (never echo or markers) and
/// the notes typed beside it.
fn meeting_text(note: &Note) -> String {
    let mut text: String = note
        .transcript
        .iter()
        .filter(|s| s.is_talk())
        .map(|s| s.text.trim())
        .collect::<Vec<_>>()
        .join("\n");
    let typed = note.content.trim();
    if !typed.is_empty() {
        text.push('\n');
        text.push_str(typed);
    }
    text
}

/// The language note `note`'s summary is written in (pure): the one its
/// words are in; not sure, `mine`'s first (My languages, the person's
/// order), else English.
pub fn for_note(note: &Note, mine: &[String]) -> MeetingLanguage {
    if let Some(code) = detect(&meeting_text(note)) {
        return MeetingLanguage { code: code.to_string(), how: How::Heard };
    }
    match mine.iter().find_map(|c| crate::dictation::languages::catalog::normalize(c)) {
        Some(code) => MeetingLanguage { code: code.to_string(), how: How::Yours },
        None => MeetingLanguage { code: "en".to_string(), how: How::Default },
    }
}

/// [`for_note`] with the person's languages from the settings, logged
/// (the code and how, never the words).
pub fn of_note(note: &Note) -> MeetingLanguage {
    let mine = crate::dictation::languages::my_languages(&crate::config::load());
    let lang = for_note(note, &mine);
    tracing::info!(note_id = note.id, code = %lang.code, how = ?lang.how, "Meeting notes' language");
    lang
}

/// "French, the language of this meeting" / "French" (not sure: the
/// person's own).
fn which(lang: &MeetingLanguage) -> String {
    match lang.how {
        How::Heard => format!("{}, the language of this meeting", lang.name()),
        How::Yours | How::Default => lang.name(),
    }
}

/// The action plan's line, after its input: write in the meeting's
/// language, the headings in English as given. `None` for English.
pub fn plan_line(lang: &MeetingLanguage) -> Option<String> {
    (!lang.is_english()).then(|| {
        format!(
            "Write the notes in {}. Keep these in English exactly as written: the headings (\"## Recap\", \"## Action plan\", \"### You\", \"## Decisions\", \"## Open questions\", \"## Unassigned\") and \"(due: …)\". Names stay as they are.",
            which(lang)
        )
    })
}

/// A digest's line, after its transcript: the bullets in the meeting's
/// language, the four headings, "Owner:", "Unassigned" and "(due: …)" in
/// English. `None` for English.
pub fn digest_line(lang: &MeetingLanguage) -> Option<String> {
    (!lang.is_english()).then(|| {
        format!(
            "Write the bullets in {}. Keep the four headings, \"You\", \"Unassigned\" and \"(due: …)\" in English exactly as shown.",
            which(lang)
        )
    })
}

/// Any other action's line on a meeting note (Meeting Notes, the person's
/// own): the meeting's language, any headings its instructions give as
/// written. `None` for English.
pub fn notes_line(lang: &MeetingLanguage) -> Option<String> {
    (!lang.is_english()).then(|| format!("Write the notes in {}. Keep any headings the instructions give exactly as written.", which(lang)))
}

/// `input` with `line` after it (none: as it is).
pub fn with_line(input: &str, line: Option<&str>) -> String {
    match line {
        Some(l) => format!("{}\n\n{l}", input.trim_end()),
        None => input.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::TranscriptSegment;

    fn note(lines: &[&str]) -> Note {
        let mut n: Note = serde_json::from_value(serde_json::json!({
            "id": 1, "title": "", "content": "", "createdTs": 1, "updatedTs": 1,
        }))
        .unwrap();
        n.transcript = lines
            .iter()
            .enumerate()
            .map(|(i, t)| TranscriptSegment { source: if i % 2 == 0 { "you" } else { "them" }.into(), text: t.to_string(), ts: i as u64, ..Default::default() })
            .collect();
        n
    }

    const FR: &[&str] = &[
        "Bonjour à tous, on commence par le budget du salon.",
        "Oui, alors le devis du traiteur est arrivé hier et il est trop cher pour nous.",
        "Donc on garde le lieu à Lyon mais on change de traiteur, c'est d'accord ?",
        "D'accord. Je vais demander deux autres devis avant vendredi.",
        "Et pour les badges, qui s'en occupe ? Il faut aussi réserver les chambres.",
    ];
    const ES: &[&str] = &[
        "Bueno, empezamos con el lanzamiento de la campaña.",
        "Sí, el diseño está casi listo pero todavía hay que revisar los textos.",
        "Entonces vamos a mover la fecha al lunes porque el proveedor no puede antes.",
        "Vale, yo me encargo de hablar con el proveedor esta tarde.",
        "Perfecto, y también tenemos que preparar el presupuesto para el cliente.",
    ];
    const EN: &[&str] = &[
        "Okay, let's go through the launch plan for the spring campaign.",
        "The design is nearly done but we still need to check the copy with legal.",
        "So we'll move the date to Monday because the supplier can't do it sooner.",
        "I will call the supplier this afternoon and send the revised quote.",
        "Great, and we should book the room for the review next week.",
    ];

    #[test]
    fn a_meeting_is_read_in_its_own_language() {
        assert_eq!(detect(&FR.join(" ")), Some("fr"));
        assert_eq!(detect(&ES.join(" ")), Some("es"));
        assert_eq!(detect(&EN.join(" ")), Some("en"));
        assert_eq!(detect("Wir müssen das Budget noch einmal prüfen, und ich schicke die Zahlen bis Freitag. Das ist gut so, aber nicht für alle."), Some("de"));
        // Scripts of their own.
        assert_eq!(detect("Давайте обсудим бюджет на следующий квартал и сроки поставки."), Some("ru"));
        assert_eq!(detect("会議の議題は来週の発表についてです。資料は金曜日までに準備します。"), Some("ja"));
        assert_eq!(detect("下周的发布会需要准备演示文稿，预算也要重新审核一下。"), Some("zh"));
        // Too little to say, or two languages half and half: not sure.
        assert_eq!(detect("Okay."), None);
        assert_eq!(detect(&[EN[0], FR[1], EN[2], FR[3]].join(" ")), None);
        // A French aside in an English meeting stays English.
        assert_eq!(detect(&[EN.join(" "), "c'est la vie".to_string()].join(" ")), Some("en"));
    }

    #[test]
    fn the_summary_follows_the_meeting_else_your_first_language_else_english() {
        let fr = note(FR);
        let mine = vec!["de".to_string(), "en".to_string()];
        assert_eq!(for_note(&fr, &mine), MeetingLanguage { code: "fr".into(), how: How::Heard });
        assert_eq!(for_note(&note(EN), &mine).code, "en");
        // Not sure: your first language, else English.
        let short = note(&["Okay.", "Bien."]);
        assert_eq!(for_note(&short, &mine), MeetingLanguage { code: "de".into(), how: How::Yours });
        assert_eq!(for_note(&short, &[]), MeetingLanguage { code: "en".into(), how: How::Default });
        // Echo lines and markers aren't the meeting's words.
        let mut echoed = note(EN);
        for s in &mut echoed.transcript {
            s.echo = true;
        }
        echoed.transcript.extend(note(ES).transcript);
        assert_eq!(for_note(&echoed, &[]).code, "es");
        // Typed notes count too.
        let mut typed = note(&["Okay."]);
        typed.content = FR.join("\n");
        assert_eq!(for_note(&typed, &[]).code, "fr");
    }

    #[test]
    fn the_line_keeps_the_headings_in_english_and_says_nothing_for_english() {
        let fr = for_note(&note(FR), &[]);
        let plan = plan_line(&fr).unwrap();
        assert!(plan.starts_with("Write the notes in French, the language of this meeting."), "{plan}");
        for h in ["## Recap", "## Action plan", "### You", "## Decisions", "## Open questions", "## Unassigned", "(due: …)"] {
            assert!(plan.contains(h), "{h}");
        }
        assert!(digest_line(&fr).unwrap().contains("\"Unassigned\""));
        assert!(notes_line(&fr).unwrap().contains("French"));
        // Your own language when not sure: no claim about the meeting.
        let yours = MeetingLanguage { code: "es".into(), how: How::Yours };
        assert!(plan_line(&yours).unwrap().starts_with("Write the notes in Spanish."));
        let en = for_note(&note(EN), &[]);
        assert_eq!((plan_line(&en), digest_line(&en), notes_line(&en)), (None, None, None));
        assert_eq!(with_line("input\n", Some("line")), "input\n\nline");
        assert_eq!(with_line("input\n", None), "input\n");
    }
}

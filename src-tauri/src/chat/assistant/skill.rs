//! **A Skill said to the Voice Assistant** (2026-11-03): saying a Skill runs
//! it as Chat runs a typed one (`chat/skills.rs`) — its own passages, its
//! instructions, the same citations in the conversation the answer lands in,
//! a document for /tickets and /prd — and the voice says as much of the
//! answer as "How much to read aloud" says, the rest in Chat.
//!
//! **What says one** ([`matched`], pure), against the Skills there are —
//! Yap's and the person's own, their commands, names and descriptions, never
//! a list of phrases per Skill:
//! - **its slash name** ([`How::Slash`], every Skill): "slash follow-up
//!   email", "slash prep the supplier call", "/standup" as written;
//! - **its name with "skill"** ([`How::Named`], every Skill — one off the
//!   `/` menu too, as a typed command runs it): "run my standup skill", "use
//!   the skill called decision log", "lance la skill point hebdo", "starte
//!   meinen Standup-Skill";
//! - **one of its own phrasings, as a request** ([`How::Asked`], only the
//!   Skills in the `/` menus): its one-tap prompt or question ("Prepare me
//!   for my next meeting" — "prep me for my next meeting"), its description
//!   or its first clause ("list my open tasks"), its name after a verb that
//!   runs it ("do my stand-up"), or its name alone, followed by what it's
//!   about when the name is two words or more or ends in "me" ("prepare me
//!   for the supplier call", "project update on the website relaunch").
//!   Word for word, in order (the little words aside, "prep" for "prepare",
//!   "stand up" for "stand-up"); a phrasing of two words or fewer is the
//!   whole request, a longer one may be followed by a few words saying what
//!   it's about — never by another request ("…and email them to Sam").
//!
//! The rest of what was said is the Skill's words, as after a typed command
//! ("for the supplier call" → "the supplier call"). The framing — "slash",
//! "skill", the verbs that run one, "can you", "please", the little words —
//! is in the person's own languages (My languages, English always; none of
//! theirs known: all of them), the seven Yap's other spoken rules know
//! (`assistant/screen.rs`, `dictation/command_screen.rs`). Yap's own Skills
//! are worded in English, so in another language they're said by their
//! slash name or with "skill"; the person's own match in whatever language
//! they wrote them.
//!
//! **In doubt, it's an ordinary question**: a false match is worse than a
//! miss. "What are the risks of the launch?", "how do I write a stand-up?",
//! "what does the standup skill do?", "prep dinner for six" are questions;
//! two Skills said alike are neither.
//!
//! **Yap's question, answered by voice** ([`wait_for_answer`],
//! [`answer_words`]): a Skill with nothing to run on asks out loud
//! (`skills::ask_first`'s `aloud`: "Which meeting should I prepare you
//! for?"); the next spoken question in that conversation within
//! [`ANSWER_WAIT`], when it's short and no question of its own, is the
//! answer: the Skill runs on it ("The supplier call."). "Never mind" isn't.
//!
//! A Skill that runs on one meeting (/followup-email) reads the meeting its
//! words name, else the latest ([`turn`]), as /summary-for does across the
//! meetings — there's no meeting open to run it on.
//!
//! Logs: how it was said, a built-in's command, counts — never the words.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use crate::chat::skills::{Listed, Skill, SkillTurn};

/// How a Skill was said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    /// Its command after "slash".
    Slash,
    /// Its name or command with "skill".
    Named,
    /// One of its own phrasings, as a request.
    Asked,
}

impl How {
    fn label(self) -> &'static str {
        match self {
            How::Slash => "slash",
            How::Named => "name",
            How::Asked => "request",
        }
    }
}

/// A Skill said, and the words with it.
#[derive(Debug, Clone, PartialEq)]
pub struct Said {
    pub skill: Skill,
    pub words: String,
    pub how: How,
}

/// The most words after one of a Skill's phrasings that say what it's about.
const REST_MAX: usize = 8;
/// The longest answer to Yap's question, in words.
const ANSWER_MAX: usize = 10;
/// How long Yap's question waits for its answer by voice.
pub const ANSWER_WAIT: Duration = Duration::from_secs(120);

// ---- the words, in each language -----------------------------------------------------------

/// One language's words (each a phrase, read as [`tokens`] reads speech).
struct Lang {
    code: &'static str,
    /// "/" said.
    slash: &'static [&'static str],
    /// "Skill".
    skill: &'static [&'static str],
    /// Between "skill" and its name: "called".
    called: &'static [&'static str],
    /// Verbs that run one: "run", "lance".
    run: &'static [&'static str],
    /// Before a name: "my", "the".
    det: &'static [&'static str],
    /// Before a request: "okay", "can you".
    open: &'static [&'static str],
    /// After it: "please", "thanks" (and a separable verb's end: "aus", "uit").
    close: &'static [&'static str],
    /// The little words a phrasing is compared without.
    stop: &'static [&'static str],
    /// Before what it's about: "for", "with" (left out of the words).
    link: &'static [&'static str],
    /// A second request after a Skill's phrasing: "and", "then".
    and: &'static [&'static str],
    /// Before an answer: "it's".
    it_is: &'static [&'static str],
    /// Not an answer: "never mind".
    never: &'static [&'static str],
}

const LANGS: &[Lang] = &[
    Lang {
        code: "en",
        slash: &["slash", "forward slash"],
        skill: &["skill"],
        called: &["called", "named"],
        run: &["run", "do", "use", "start", "launch", "try", "execute", "trigger"],
        det: &["my", "the", "our", "a", "an", "your", "this", "that"],
        open: &[
            "okay", "ok", "hey", "hi", "so", "um", "uh", "erm", "right", "alright", "please", "yap", "now", "can you", "could you",
            "would you", "will you", "i'd like you to", "i want you to", "i need you to", "i'd like to", "i want to", "i need to",
            "go ahead and", "let's", "help me",
        ],
        close: &["please", "thanks", "thank you", "for me", "now", "right now"],
        stop: &[
            "a", "an", "the", "my", "your", "our", "me", "i", "you", "we", "us", "it", "its", "for", "to", "of", "in", "on", "at",
            "from", "with", "about", "and", "did", "do", "does", "is", "are", "was", "be", "each", "every", "all", "any", "some",
            "this", "that", "these", "those", "s",
        ],
        link: &["for", "with", "about", "on", "from", "of", "to", "in", "at"],
        and: &["and", "then", "also", "plus"],
        it_is: &["it's", "it is", "that's", "that is"],
        never: &["never mind", "nevermind", "cancel", "forget it", "stop", "no", "nothing"],
    },
    Lang {
        code: "fr",
        slash: &["slash", "barre oblique"],
        skill: &["skill", "compétence"],
        called: &["appelée", "appelé", "nommée", "nommé", "de", "d"],
        run: &[
            "lance", "lancez", "lancer", "exécute", "exécutez", "exécuter", "utilise", "utilisez", "utiliser", "fais", "faites",
            "faire", "démarre", "démarrez", "démarrer",
        ],
        det: &["ma", "mon", "mes", "la", "le", "les", "l", "un", "une", "notre", "votre"],
        open: &[
            "ok", "okay", "d'accord", "alors", "euh", "bon", "s'il te plaît", "s'il vous plaît", "stp", "svp", "peux-tu",
            "pourrais-tu", "tu peux", "tu pourrais", "vous pouvez", "pouvez-vous", "pourriez-vous", "est-ce que tu peux",
            "est-ce que vous pouvez", "aide-moi à", "je veux que tu",
        ],
        close: &["s'il te plaît", "s'il vous plaît", "stp", "svp", "merci", "pour moi"],
        stop: &[
            "le", "la", "les", "l", "un", "une", "des", "du", "de", "d", "mon", "ma", "mes", "ton", "ta", "tes", "notre", "nos",
            "votre", "vos", "son", "sa", "ses", "moi", "me", "m", "je", "j", "tu", "toi", "te", "nous", "vous", "il", "elle", "on",
            "pour", "à", "au", "aux", "en", "sur", "avec", "et", "ce", "cet", "cette", "ces", "chaque", "tous", "toutes", "tout",
        ],
        link: &["pour", "avec", "sur", "de", "du", "d", "à", "au", "en"],
        and: &["et", "puis", "ensuite"],
        it_is: &["c'est"],
        never: &["laisse tomber", "laissez tomber", "annule", "annuler", "oublie", "rien", "non"],
    },
    Lang {
        code: "de",
        slash: &["slash", "schrägstrich"],
        skill: &["skill", "fähigkeit"],
        called: &["namens", "genannt"],
        run: &[
            "starte", "start", "starten", "führe", "führ", "ausführen", "nutze", "nutzen", "verwende", "verwenden", "benutze",
            "benutzen", "mach", "mache", "machen",
        ],
        det: &["mein", "meine", "meinen", "meinem", "den", "die", "das", "dem", "ein", "eine", "einen", "unser", "unsere", "unseren"],
        open: &["ok", "okay", "also", "äh", "ähm", "bitte", "kannst du", "könntest du", "würdest du", "hilf mir"],
        close: &["bitte", "danke", "für mich", "aus"],
        stop: &[
            "der", "die", "das", "den", "dem", "des", "ein", "eine", "einen", "einem", "einer", "mein", "meine", "meinen", "meinem",
            "meiner", "dein", "deine", "deinen", "unser", "unsere", "unseren", "ich", "mich", "mir", "du", "dich", "dir", "wir",
            "uns", "für", "zu", "zum", "zur", "von", "vom", "in", "im", "auf", "mit", "über", "und", "jede", "jeden", "jedes", "alle",
            "diese", "dieser", "dieses",
        ],
        link: &["für", "mit", "über", "zu", "zum", "zur", "von", "vom", "auf", "im", "in"],
        and: &["und", "dann", "danach"],
        it_is: &["es ist", "das ist"],
        never: &["vergiss es", "egal", "abbrechen", "nichts", "nein", "schon gut"],
    },
    Lang {
        code: "es",
        slash: &["slash", "barra", "barra diagonal"],
        skill: &["skill", "habilidad"],
        called: &["llamada", "llamado", "de"],
        run: &["ejecuta", "ejecutar", "usa", "usar", "lanza", "lanzar", "inicia", "iniciar", "haz", "hacer", "corre", "correr"],
        det: &["mi", "mis", "la", "el", "los", "las", "un", "una", "nuestro", "nuestra"],
        open: &["vale", "ok", "bueno", "eh", "oye", "por favor", "puedes", "podrías", "me puedes", "ayúdame a", "quiero que"],
        close: &["por favor", "gracias", "para mí"],
        stop: &[
            "el", "la", "los", "las", "lo", "un", "una", "unos", "unas", "mi", "mis", "tu", "tus", "su", "sus", "nuestro", "nuestra",
            "me", "yo", "te", "nos", "para", "de", "del", "a", "al", "en", "con", "sobre", "y", "cada", "todos", "todas", "este",
            "esta", "esto",
        ],
        link: &["para", "con", "sobre", "de", "del", "en", "a", "al"],
        and: &["y", "luego", "después"],
        it_is: &["es"],
        never: &["olvídalo", "déjalo", "cancela", "cancelar", "nada", "no"],
    },
    Lang {
        code: "it",
        slash: &["slash", "barra"],
        skill: &["skill", "abilità"],
        called: &["chiamata", "chiamato", "di"],
        run: &["esegui", "eseguire", "usa", "usare", "avvia", "avviare", "lancia", "lanciare", "fai", "fare"],
        det: &["la", "il", "lo", "l", "le", "mia", "mio", "un", "una", "uno", "nostra", "nostro"],
        open: &["ok", "okay", "allora", "ehm", "per favore", "puoi", "potresti", "mi puoi", "aiutami a"],
        close: &["per favore", "grazie", "per me"],
        stop: &[
            "il", "lo", "la", "i", "gli", "le", "l", "un", "uno", "una", "mio", "mia", "miei", "mie", "tuo", "tua", "nostro",
            "nostra", "mi", "me", "io", "ti", "ci", "noi", "per", "di", "del", "della", "dei", "delle", "a", "al", "alla", "in",
            "nel", "nella", "su", "sul", "sulla", "con", "e", "ogni", "tutti", "tutte", "questo", "questa",
        ],
        link: &["per", "con", "su", "sul", "sulla", "di", "del", "della", "in", "nel", "nella", "a", "al", "alla"],
        and: &["e", "poi", "dopo"],
        it_is: &["è"],
        never: &["lascia perdere", "lascia stare", "annulla", "niente", "no"],
    },
    Lang {
        code: "pt",
        slash: &["slash", "barra"],
        skill: &["skill", "habilidade"],
        called: &["chamada", "chamado", "de"],
        run: &[
            "executa", "execute", "executar", "usa", "use", "usar", "inicia", "inicie", "iniciar", "roda", "rode", "rodar", "faz",
            "faça", "fazer",
        ],
        det: &["a", "o", "as", "os", "minha", "meu", "um", "uma", "nossa", "nosso"],
        open: &["ok", "okay", "então", "olha", "por favor", "podes", "pode", "você pode", "poderias", "poderia", "ajuda-me a", "me ajuda a"],
        close: &["por favor", "obrigado", "obrigada", "para mim"],
        stop: &[
            "o", "a", "os", "as", "um", "uma", "uns", "umas", "meu", "minha", "meus", "minhas", "teu", "tua", "seu", "sua", "nosso",
            "nossa", "me", "eu", "te", "nos", "para", "de", "do", "da", "dos", "das", "em", "no", "na", "nas", "com", "sobre", "e",
            "cada", "todos", "todas", "este", "esta", "isto",
        ],
        link: &["para", "com", "sobre", "de", "do", "da", "em", "no", "na"],
        and: &["e", "depois", "então"],
        it_is: &["é"],
        never: &["esquece", "esqueça", "deixa", "cancela", "cancelar", "nada", "não"],
    },
    Lang {
        code: "nl",
        slash: &["slash", "schuine streep"],
        skill: &["skill", "vaardigheid"],
        called: &["genaamd", "met de naam"],
        run: &["start", "starten", "gebruik", "gebruiken", "voer", "uitvoeren", "doe", "doen", "draai", "draaien"],
        det: &["mijn", "de", "het", "een", "onze", "ons"],
        open: &["oké", "ok", "okay", "dus", "eh", "alsjeblieft", "graag", "kun je", "kan je", "kunt u", "wil je", "help me"],
        close: &["alsjeblieft", "graag", "bedankt", "dank je", "voor mij", "uit"],
        stop: &[
            "de", "het", "een", "mijn", "je", "jouw", "uw", "ons", "onze", "mij", "me", "ik", "jij", "wij", "we", "voor", "van", "in",
            "op", "met", "over", "aan", "en", "elk", "elke", "alle", "deze", "dit", "dat",
        ],
        link: &["voor", "met", "over", "van", "op", "in", "aan"],
        and: &["en", "dan", "daarna"],
        it_is: &["het is", "dat is"],
        never: &["laat maar", "annuleer", "niets", "nee"],
    },
];

/// After a name's last word, an imperative's object: "Prepare me", "Coach
/// me" — such a name may be followed by what it's about.
const OBJECTS: &[&str] = &["me", "us", "moi", "nous", "mich", "uns", "mi", "nos", "ci", "mij"];

/// Words after a hyphen that stand on their own ("prépare-moi", "peux-tu",
/// "faz-me", "Standup-Skill").
const CLITICS: &[&str] = &[
    "moi", "toi", "lui", "nous", "vous", "leur", "le", "la", "les", "tu", "il", "elle", "on", "y", "en", "me", "te", "se", "ce", "ci",
    "là", "lhe", "lhes", "nos", "vos", "o", "a", "os", "as", "skill", "skills",
];

/// A phrase as speech is read: its words' keys.
type Phrase = Vec<String>;

/// One language's words, read once.
struct Words {
    code: &'static str,
    slash: Vec<Phrase>,
    skill: Vec<String>,
    called: Vec<Phrase>,
    run: Vec<String>,
    det: Vec<String>,
    open: Vec<Phrase>,
    close: Vec<Phrase>,
    stop: Vec<String>,
    link: Vec<String>,
    and: Vec<String>,
    it_is: Vec<Phrase>,
    never: Vec<Phrase>,
}

static WORDS: LazyLock<Vec<Words>> = LazyLock::new(|| {
    let phrases = |l: &[&str]| l.iter().map(|p| keys(&tokens(p))).filter(|p| !p.is_empty()).collect::<Vec<Phrase>>();
    let single = |l: &[&str]| phrases(l).into_iter().flatten().collect::<Vec<String>>();
    LANGS
        .iter()
        .map(|l| Words {
            code: l.code,
            slash: phrases(l.slash),
            skill: single(l.skill),
            called: phrases(l.called),
            run: single(l.run),
            det: single(l.det),
            open: phrases(l.open),
            close: phrases(l.close),
            stop: single(l.stop),
            link: single(l.link),
            and: single(l.and),
            it_is: phrases(l.it_is),
            never: phrases(l.never),
        })
        .collect()
});

/// The person's languages' words together.
#[derive(Default)]
struct Mine {
    slash: Vec<Phrase>,
    skill: HashSet<String>,
    called: Vec<Phrase>,
    run: HashSet<String>,
    det: HashSet<String>,
    open: Vec<Phrase>,
    close: Vec<Phrase>,
    stop: HashSet<String>,
    link: HashSet<String>,
    and: HashSet<String>,
    it_is: Vec<Phrase>,
    never: Vec<Phrase>,
}

/// The words of `langs` (My languages' codes; English always; none of
/// theirs known: all of them).
fn mine(langs: &[String]) -> Mine {
    let all = langs.iter().all(|c| !LANGS.iter().any(|l| c.to_ascii_lowercase().starts_with(l.code)));
    let theirs = |code: &str| all || code == "en" || langs.iter().any(|c| c.to_ascii_lowercase().starts_with(code));
    let mut m = Mine::default();
    for w in WORDS.iter().filter(|w| theirs(w.code)) {
        m.slash.extend(w.slash.iter().cloned());
        m.skill.extend(w.skill.iter().cloned());
        m.called.extend(w.called.iter().cloned());
        m.run.extend(w.run.iter().cloned());
        m.det.extend(w.det.iter().cloned());
        m.open.extend(w.open.iter().cloned());
        m.close.extend(w.close.iter().cloned());
        m.stop.extend(w.stop.iter().cloned());
        m.link.extend(w.link.iter().cloned());
        m.and.extend(w.and.iter().cloned());
        m.it_is.extend(w.it_is.iter().cloned());
        m.never.extend(w.never.iter().cloned());
    }
    m
}

// ---- reading what was said ------------------------------------------------------------------

/// A word as said.
#[derive(Debug, Clone)]
struct Tok {
    /// Lower case, letters and digits only ("Stand-up" → "standup").
    key: String,
    /// Where it is in what was said (bytes).
    start: usize,
    end: usize,
    /// Written straight after a "/" ("/prep").
    slashed: bool,
}

/// What was said, word by word: letters and digits, with hyphens and
/// apostrophes inside a word ("stand-up", "to-dos", "Priya's"); an elision
/// ("l'équipe", "qu'il") and a clitic or "skill" after a hyphen
/// ("prépare-moi", "Standup-Skill") are words of their own. Pure.
fn tokens(text: &str) -> Vec<Tok> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].1.is_alphanumeric() {
            i += 1;
            continue;
        }
        let slashed = i > 0 && chars[i - 1].1 == '/';
        let mut j = i + 1;
        while j < chars.len() {
            let c = chars[j].1;
            let inside = matches!(c, '-' | '\'' | '\u{2019}') && chars[j - 1].1.is_alphanumeric() && chars.get(j + 1).is_some_and(|n| n.1.is_alphanumeric());
            if !(c.is_alphanumeric() || inside) {
                break;
            }
            j += 1;
        }
        let end = chars.get(j).map_or(text.len(), |c| c.0);
        split_word(text, chars[i].0, end, slashed, &mut out);
        i = j;
    }
    out
}

/// One word of [`tokens`], split where it holds more than one.
fn split_word(text: &str, start: usize, end: usize, slashed: bool, out: &mut Vec<Tok>) {
    let word = &text[start..end];
    // An elision: "l'équipe", "d'accord", "qu'il", "s'il" (not "it's").
    if let Some(i) = word.find(['\'', '\u{2019}']) {
        let head = &word[..i];
        if head.chars().count() == 1 || head.eq_ignore_ascii_case("qu") {
            push(text, start, start + i, slashed, out);
            let after = start + i + word[i..].chars().next().map_or(1, char::len_utf8);
            split_word(text, after, end, false, out);
            return;
        }
    }
    // A clitic or "skill" after a hyphen.
    let mut from = start;
    let mut first = true;
    for (i, _) in word.match_indices('-') {
        let at = start + i + 1;
        let to = text[at..end].find('-').map_or(end, |k| at + k);
        if CLITICS.contains(&text[at..to].to_lowercase().as_str()) {
            push(text, from, start + i, slashed && first, out);
            first = false;
            from = at;
        }
    }
    push(text, from, end, slashed && first, out);
}

fn push(text: &str, start: usize, end: usize, slashed: bool, out: &mut Vec<Tok>) {
    let key: String = text[start..end].chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();
    if !key.is_empty() {
        out.push(Tok { key, start, end, slashed });
    }
}

fn keys(toks: &[Tok]) -> Phrase {
    toks.iter().map(|t| t.key.clone()).collect()
}

/// A word without its ending: "decisions" and "decision", "decided" and
/// "decide" are one. Pure.
fn stem(w: &str) -> &str {
    let n = w.chars().count();
    let w = if n > 3 { w.strip_suffix('s').unwrap_or(w) } else { w };
    let n = w.chars().count();
    if n > 4 {
        if let Some(s) = w.strip_suffix("ed") {
            return s;
        }
        if let Some(s) = w.strip_suffix(['e', 'd']) {
            return s;
        }
    }
    w
}

/// One word for another in a phrasing: the same, the same but for its
/// ending, or said short ("prep" for "prepare", "info" for "information":
/// four letters at least, three fewer). Pure.
fn same(a: &str, b: &str) -> bool {
    if a == b || stem(a) == stem(b) {
        return true;
    }
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let (n, m) = (short.chars().count(), long.chars().count());
    n >= 4 && m >= n + 3 && long.starts_with(short)
}

/// The longest of `phrases` said at `toks[i..]`: its length.
fn phrase_at(toks: &[Tok], i: usize, phrases: &[Phrase]) -> Option<usize> {
    phrases
        .iter()
        .filter(|p| i + p.len() <= toks.len() && toks[i..i + p.len()].iter().zip(p.iter()).all(|(t, w)| t.key == *w))
        .map(Vec::len)
        .max()
}

/// The longest of `phrases` said just before `toks[j]`, not before `i`.
fn phrase_before(toks: &[Tok], i: usize, j: usize, phrases: &[Phrase]) -> Option<usize> {
    phrases
        .iter()
        .filter(|p| j >= i + p.len() && toks[j - p.len()..j].iter().zip(p.iter()).all(|(t, w)| t.key == *w))
        .map(Vec::len)
        .max()
}

/// What was said without what goes around a request ("okay, can you …
/// please").
fn trimmed<'a>(toks: &'a [Tok], m: &Mine) -> &'a [Tok] {
    let mut i = 0;
    for _ in 0..6 {
        match phrase_at(toks, i, &m.open) {
            Some(n) => i += n,
            None => break,
        }
    }
    let mut j = toks.len();
    for _ in 0..3 {
        match phrase_before(toks, i, j, &m.close) {
            Some(n) => j -= n,
            None => break,
        }
    }
    &toks[i..j]
}

/// The words from `body[k]` on, as said (a leading "for", "with"… left out).
fn words_from(text: &str, body: &[Tok], k: usize, m: &Mine) -> String {
    let k = if body.get(k).is_some_and(|t| m.link.contains(&t.key)) { k + 1 } else { k };
    match (body.get(k), body.last()) {
        (Some(a), Some(z)) => text[a.start..z.end].trim().to_string(),
        _ => String::new(),
    }
}

// ---- the Skills, as said ---------------------------------------------------------------------

/// A Skill's command and names as speech is read.
struct Named<'a> {
    skill: &'a Skill,
    shown: bool,
    /// Its command without hyphens ("followupemail").
    command: String,
    /// Its name, and its name without its first little words ("My open
    /// to-dos" → "opentodos").
    names: Vec<String>,
    /// Its name's words other than the little ones.
    content: usize,
    /// Its name ends in "me" ("Prepare me").
    imperative: bool,
}

fn named<'a>(skills: &'a [Listed], m: &Mine) -> Vec<Named<'a>> {
    skills
        .iter()
        .map(|l| {
            let name = tokens(&l.skill.name);
            let full: String = name.iter().map(|t| t.key.as_str()).collect();
            let lead = name.iter().take_while(|t| m.stop.contains(&t.key) || m.det.contains(&t.key)).count();
            let short: String = name[lead..].iter().map(|t| t.key.as_str()).collect();
            let mut names = vec![full];
            if !short.is_empty() && short != names[0] {
                names.push(short);
            }
            names.retain(|n| !n.is_empty());
            Named {
                skill: &l.skill,
                shown: l.shown,
                command: l.skill.command.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase(),
                names,
                content: name.iter().filter(|t| !m.stop.contains(&t.key)).count(),
                imperative: name.last().is_some_and(|t| OBJECTS.contains(&t.key.as_str())),
            }
        })
        .collect()
}

/// A key said for a command or a name: the same, or but for its ending.
fn same_key(said: &str, key: &str) -> bool {
    said == key || stem(said) == stem(key)
}

/// **The Skill named at `body[i..]`** — its command, else its name, in up
/// to six words ending by `max_end` — the longest that names one: its index
/// and where its name ends. `None`: none, or two named alike.
fn name_at(body: &[Tok], i: usize, max_end: usize, skills: &[&Named]) -> Option<(usize, usize)> {
    let max_end = max_end.min(body.len());
    for n in (1..=6.min(max_end.saturating_sub(i))).rev() {
        let said: String = body[i..i + n].iter().map(|t| t.key.as_str()).collect();
        let by_command: Vec<usize> = (0..skills.len()).filter(|&k| same_key(&said, &skills[k].command)).collect();
        let by_name: Vec<usize> = (0..skills.len()).filter(|&k| skills[k].names.iter().any(|x| same_key(&said, x))).collect();
        let found = if by_command.is_empty() { by_name } else { by_command };
        match found.as_slice() {
            [] => continue,
            [k] => return Some((*k, i + n)),
            _ => return None,
        }
    }
    None
}

/// One way a Skill was said: which, its score (the words it took), and
/// where its words begin.
struct Hit {
    skill: usize,
    score: usize,
    rest: usize,
}

/// "slash prep …", "/prep …", "run slash prep …".
fn by_slash(body: &[Tok], m: &Mine, skills: &[&Named]) -> Option<(usize, usize)> {
    let s = usize::from(body.first().is_some_and(|t| m.run.contains(&t.key)));
    let at = if body.get(s).is_some_and(|t| t.slashed) { s } else { s + phrase_at(body, s, &m.slash)? };
    name_at(body, at, body.len(), skills)
}

/// Past the little words before a name from `body[i]`: "my", "the", and a
/// "please" after the verb ("führe bitte den Skill … aus").
fn past_fill(body: &[Tok], mut i: usize, m: &Mine) -> usize {
    loop {
        if body.get(i).is_some_and(|t| m.det.contains(&t.key)) {
            i += 1;
            continue;
        }
        match phrase_at(body, i, &m.open) {
            Some(n) => i += n,
            None => return i,
        }
    }
}

/// "run my standup skill …", "run the skill called standup …": with a verb
/// that runs it, or nothing else said.
fn by_name(body: &[Tok], m: &Mine, skills: &[&Named]) -> Option<(usize, usize)> {
    let run = body.first().is_some_and(|t| m.run.contains(&t.key));
    let i = past_fill(body, usize::from(run), m);
    if body.get(i).is_some_and(|t| m.skill.contains(&t.key)) {
        let k = i + 1 + phrase_at(body, i + 1, &m.called).unwrap_or(0);
        let (skill, end) = name_at(body, k, body.len(), skills)?;
        return (run || end == body.len()).then_some((skill, end));
    }
    let p = (i + 1..body.len()).find(|&p| m.skill.contains(&body[p].key))?;
    let (skill, end) = name_at(body, i, p, skills)?;
    (end == p && (run || p + 1 == body.len())).then_some((skill, p + 1))
}

/// A Skill's phrasings, each its words other than the little ones: its
/// one-tap prompt and question, its description and its first clause, and
/// their starts up to a "for", "to"… when that's three words or more
/// ("Draft a follow-up email" of "…to the attendees").
fn phrasings(s: &Skill, m: &Mine) -> Vec<Phrase> {
    let lead = s.description.split([',', ':', ';', '(', '—', '–']).next().unwrap_or_default();
    let mut out: Vec<Phrase> = Vec::new();
    for text in [s.chip.as_str(), s.question.as_str(), s.description.as_str(), lead] {
        let toks = tokens(text);
        let content = |upto: usize| toks[..upto].iter().filter(|t| !m.stop.contains(&t.key)).map(|t| t.key.clone()).collect::<Phrase>();
        let mut cuts: Vec<Phrase> = vec![content(toks.len())];
        cuts.extend((1..toks.len()).filter(|&i| m.link.contains(&toks[i].key)).map(content).filter(|p| p.len() >= 3));
        for p in cuts {
            if !p.is_empty() && !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// A phrasing's words said in order from `body[0]`, the little words aside
/// (and "stand up" for "stand-up", "to do's" for "to-dos"): where its last
/// word ends. Pure.
fn follows(body: &[Tok], phrase: &[String], m: &Mine) -> Option<usize> {
    let (mut b, mut p) = (0, 0);
    while p < phrase.len() {
        let t = body.get(b)?;
        if same(&t.key, &phrase[p]) {
            b += 1;
            p += 1;
            continue;
        }
        let joined = [(2, 1), (3, 1), (1, 2), (1, 3), (2, 2)].into_iter().find(|&(nb, np)| {
            b + nb <= body.len() && p + np <= phrase.len() && body[b..b + nb].iter().map(|t| t.key.as_str()).collect::<String>() == phrase[p..p + np].concat()
        });
        if let Some((nb, np)) = joined {
            b += nb;
            p += np;
        } else if m.stop.contains(&t.key) {
            b += 1;
        } else {
            return None;
        }
    }
    Some(b)
}

/// **A request in a Skill's own words** (the Skills in the `/` menus): its
/// name after a verb that runs it, its name alone (then what it's about, for
/// a name of two words or more or one ending in "me"), or one of its
/// phrasings — each [`Hit`] it could be.
fn by_request(body: &[Tok], m: &Mine, skills: &[&Named]) -> Vec<Hit> {
    let mut hits = Vec::new();
    // A few words after it saying what it's about, never another request.
    let about = |k: usize| k < body.len() && body.len() - k <= REST_MAX && !m.and.contains(&body[k].key);
    // "do my stand-up", "run decisions".
    if body.first().is_some_and(|t| m.run.contains(&t.key)) {
        let i = past_fill(body, 1, m);
        if let Some((k, end)) = name_at(body, i, body.len(), skills).filter(|&(_, end)| end == body.len()) {
            hits.push(Hit { skill: k, score: skills[k].content.max(1), rest: end });
        }
    }
    // "stand-up", "prepare me for the supplier call", "follow-up email for the supplier call".
    if let Some((k, end)) = name_at(body, past_fill(body, 0, m), body.len(), skills) {
        let s = &skills[k];
        let with_about = (s.content >= 2 || s.imperative) && body.get(end).is_some_and(|t| m.link.contains(&t.key)) && about(end);
        if end == body.len() || with_about {
            hits.push(Hit { skill: k, score: s.content.max(1), rest: end });
        }
    }
    // "prep me for my next meeting", "list my open tasks from this week".
    for (k, s) in skills.iter().enumerate() {
        for phrase in phrasings(s.skill, m) {
            let Some(end) = follows(body, &phrase, m) else { continue };
            if end == body.len() || (phrase.len() >= 3 && about(end)) {
                hits.push(Hit { skill: k, score: phrase.len(), rest: end });
            }
        }
    }
    hits
}

/// The best of `hits`: the most words taken; two Skills alike, neither —
/// unless one is the person's copy of the other (they made it to change it).
fn best<'h>(hits: &'h [Hit], skills: &[&Named]) -> Option<&'h Hit> {
    let top = hits.iter().map(|h| h.score).max()?;
    let tied: Vec<&Hit> = hits.iter().filter(|h| h.score == top).collect();
    let mut ids: Vec<usize> = tied.iter().map(|h| h.skill).collect();
    ids.sort_unstable();
    ids.dedup();
    let pick = match ids.as_slice() {
        [one] => *one,
        _ => {
            let own: Vec<usize> = ids.iter().copied().filter(|&k| !skills[k].skill.builtin).collect();
            let [mine] = own.as_slice() else { return None };
            let copied = &skills[*mine].skill.based_on;
            if !ids.iter().all(|&k| k == *mine || (skills[k].skill.builtin && skills[k].skill.command == *copied)) {
                return None;
            }
            *mine
        }
    };
    tied.into_iter().find(|h| h.skill == pick)
}

/// **The Skill a spoken question says, if any**, and its words: its slash
/// name, its name with "skill", or (one in the menus) a request in its own
/// words — see the module's notes. `skills`: every Skill as the menus list
/// them (`skills::listed`); `langs`: My languages' codes. In doubt, `None`.
/// Pure.
pub fn matched(question: &str, skills: &[Listed], langs: &[String]) -> Option<Said> {
    let m = mine(langs);
    let toks = tokens(question);
    let body = trimmed(&toks, &m);
    if body.is_empty() {
        return None;
    }
    let all = named(skills, &m);
    let every: Vec<&Named> = all.iter().collect();
    let said = |k: usize, end: usize, how: How, among: &[&Named]| Said { skill: among[k].skill.clone(), words: words_from(question, body, end, &m), how };
    if let Some((k, end)) = by_slash(body, &m, &every) {
        return Some(said(k, end, How::Slash, &every));
    }
    if let Some((k, end)) = by_name(body, &m, &every) {
        return Some(said(k, end, How::Named, &every));
    }
    let shown: Vec<&Named> = all.iter().filter(|n| n.shown).collect();
    let hits = by_request(body, &m, &shown);
    let hit = best(&hits, &shown)?;
    Some(said(hit.skill, hit.rest, How::Asked, &shown))
}

/// **The answer to Yap's question for a Skill** ("Which meeting should I
/// prepare you for?"), as its words: what was said, when it's short (at
/// most [`ANSWER_MAX`] words) and no question of its own ("…?"), without
/// an "it's" or a "for" before it. "Never mind", "cancel": `None`. Pure.
pub fn answer_words(said: &str, langs: &[String]) -> Option<String> {
    let text = said.trim();
    if text.is_empty() || text.ends_with('?') {
        return None;
    }
    let m = mine(langs);
    let toks = tokens(text);
    let body = trimmed(&toks, &m);
    if body.is_empty() || body.len() > ANSWER_MAX || phrase_at(body, 0, &m.never).is_some() {
        return None;
    }
    let k = phrase_at(body, 0, &m.it_is).unwrap_or(0);
    let words = words_from(text, body, k, &m);
    (!words.is_empty()).then_some(words)
}

/// The turn a Skill said runs, as Chat runs it. One that runs on one
/// meeting reads the meeting its words name, else the latest (gather
/// "latest", as /summary-for across the meetings): there's no meeting open
/// to run it on.
pub fn turn(mut skill: Skill, words: String) -> SkillTurn {
    if !skill.runs_on.across() && skill.gather.is_empty() {
        skill.gather = "latest".into();
    }
    SkillTurn { skill, words }
}

// ---- Yap's question, waiting for its answer ---------------------------------------------------

/// The Skill whose question Yap asked out loud, in which conversation, when.
struct Waiting {
    conversation: u64,
    skill: Skill,
    at: Instant,
}

static WAITING: Mutex<Option<Waiting>> = Mutex::new(None);

/// Yap asked out loud what `skill` should run on (`skills::ask_first`), in
/// `conversation`: the next spoken question there may be the answer.
pub fn wait_for_answer(conversation: u64, skill: &Skill) {
    *WAITING.lock().unwrap_or_else(|p| p.into_inner()) = Some(Waiting { conversation, skill: skill.clone(), at: Instant::now() });
}

/// The Skill waiting for its answer in `conversation`, taken (any question
/// takes it, answer or not).
fn take_waiting(conversation: Option<u64>) -> Option<Skill> {
    let w = WAITING.lock().unwrap_or_else(|p| p.into_inner()).take()?;
    (Some(w.conversation) == conversation && w.at.elapsed() < ANSWER_WAIT).then_some(w.skill)
}

/// **The Skill a spoken question runs, if any**, going to `conversation`: a
/// Skill said ([`matched`]), or the answer to Yap's question for one asked
/// there ([`answer_words`]).
pub fn said(question: &str, cfg: &crate::config::YapConfig, conversation: Option<u64>) -> Option<SkillTurn> {
    let langs = crate::dictation::languages::my_languages(cfg);
    let waiting = take_waiting(conversation);
    if let Some(s) = matched(question, &crate::chat::skills::listed(), &langs) {
        let command = if s.skill.builtin { s.skill.command.as_str() } else { "" };
        tracing::info!(how = s.how.label(), builtin = s.skill.builtin, command, words = s.words.chars().count(), "Voice Assistant: a Skill said");
        return Some(turn(s.skill, s.words));
    }
    let skill = waiting?;
    let words = answer_words(question, &langs)?;
    let command = if skill.builtin { skill.command.as_str() } else { "" };
    tracing::info!(builtin = skill.builtin, command, words = words.chars().count(), "Voice Assistant: the answer to Yap's question for a Skill");
    Some(turn(skill, words))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::skills::{builtins, shown_by_default, RunsOn};

    /// Yap's Skills as a new install lists them (the core six shown), and
    /// `own` beside them.
    fn listed(own: &[Skill]) -> Vec<Listed> {
        builtins()
            .into_iter()
            .chain(own.iter().cloned())
            .map(|skill| Listed { shown: shown_by_default(&skill), core: crate::chat::skills::is_core(&skill), skill })
            .collect()
    }

    fn own(name: &str, command: &str, description: &str) -> Skill {
        Skill { id: format!("s-{command}"), name: name.into(), command: command.into(), description: description.into(), instructions: "Do it.".into(), ..Default::default() }
    }

    fn en() -> Vec<String> {
        vec!["en".to_string()]
    }

    fn langs(codes: &[&str]) -> Vec<String> {
        codes.iter().map(|c| c.to_string()).collect()
    }

    /// The command and words a question runs, in `langs`, with `own` Skills.
    fn runs_in(question: &str, own: &[Skill], langs: &[String]) -> Option<(String, String)> {
        matched(question, &listed(own), langs).map(|s| (s.skill.command, s.words))
    }

    fn runs(question: &str) -> Option<(String, String)> {
        runs_in(question, &[], &en())
    }

    fn hit(command: &str, words: &str) -> Option<(String, String)> {
        Some((command.to_string(), words.to_string()))
    }

    #[test]
    fn its_slash_name_said_runs_it_with_the_words_after() {
        assert_eq!(runs("Slash follow-up email."), hit("followup-email", ""));
        assert_eq!(runs("slash follow up email for the supplier call"), hit("followup-email", "the supplier call"));
        assert_eq!(runs("Slash prep, the supplier call."), hit("prep", "the supplier call"));
        assert_eq!(runs("Okay, slash standup."), hit("standup", ""));
        assert_eq!(runs("/todos"), hit("todos", ""));
        assert_eq!(runs("Slash to do's."), hit("todos", ""));
        assert_eq!(runs("Slash summary for Sam."), hit("summary-for", "Sam"));
        assert_eq!(runs("Slash one to one with Priya."), hit("one-to-one", "Priya"));
        assert_eq!(runs("Run slash decision log for last month."), hit("decision-log", "last month"));
        // Off the menu: named whole, it runs, as a typed command does.
        assert_eq!(runs("Slash tickets the supplier call."), hit("tickets", "the supplier call"));
        assert_eq!(matched("Slash prep.", &listed(&[]), &en()).unwrap().how, How::Slash);
    }

    #[test]
    fn its_name_with_skill_runs_it_on_or_off_the_menu() {
        assert_eq!(runs("Run my standup skill."), hit("standup", ""));
        assert_eq!(runs("Can you run my stand-up skill, please?"), hit("standup", ""));
        assert_eq!(runs("Run the stand up skill"), hit("standup", ""));
        assert_eq!(runs("Use the decision log skill for October."), hit("decision-log", "October"));
        assert_eq!(runs("Run the skill called weekly recap"), hit("weekly-recap", ""));
        assert_eq!(runs("Run my tickets skill on the supplier call"), hit("tickets", "the supplier call"));
        assert_eq!(runs("Run the prepare me skill."), hit("prep", ""));
        assert_eq!(runs("Run my open to-dos skill."), hit("todos", ""));
        // Nothing but its name and "skill".
        assert_eq!(runs("Standup skill."), hit("standup", ""));
        assert_eq!(matched("Run my standup skill.", &listed(&[]), &en()).unwrap().how, How::Named);
    }

    #[test]
    fn a_request_in_a_skill_s_own_words_runs_one_in_the_menus() {
        assert_eq!(runs("Prep me for my next meeting."), hit("prep", ""));
        assert_eq!(runs("Can you prepare me for my next meeting?"), hit("prep", ""));
        assert_eq!(runs("Get me ready for my next meeting"), hit("prep", ""));
        assert_eq!(runs("Prepare me for my next meeting with Sam."), hit("prep", "Sam"));
        assert_eq!(runs("Prepare me for the supplier call."), hit("prep", "the supplier call"));
        assert_eq!(runs("Write my stand-up."), hit("standup", ""));
        assert_eq!(runs("Write my standup update please"), hit("standup", ""));
        assert_eq!(runs("Do my stand up."), hit("standup", ""));
        assert_eq!(runs("Stand-up."), hit("standup", ""));
        assert_eq!(runs("List my open to-dos."), hit("todos", ""));
        assert_eq!(runs("List my open tasks."), hit("todos", ""));
        assert_eq!(runs("What did we decide?"), hit("decisions", ""));
        assert_eq!(runs("What did we decide in my last meeting?"), hit("decisions", ""));
        assert_eq!(runs("What did we decide in the last meeting about the logo?"), hit("decisions", "the logo"));
        assert_eq!(runs("List the decisions."), hit("decisions", ""));
        assert_eq!(runs("Draft a follow-up email for the supplier call."), hit("followup-email", "the supplier call"));
        assert_eq!(runs("Sum up a meeting for Sam."), hit("summary-for", "Sam"));
        assert_eq!(matched("Prep me for my next meeting.", &listed(&[]), &en()).unwrap().how, How::Asked);
    }

    #[test]
    fn near_misses_are_ordinary_questions() {
        for q in [
            "What are the risks of taking ibuprofen?",
            "How do I write a stand-up update?",
            "What does the standup skill do?",
            "How do I edit my standup skill?",
            "Prepare dinner for my next meeting",
            "Prep dinner for six.",
            "What did we decide about dinner tonight?",
            "Write my stand-up for today.",
            "Do I have any open to-dos?",
            "Don't prep me for my next meeting.",
            "List my open to-dos and then email them to Sam.",
            "Slash the price by ten percent.",
            "Run the numbers for the budget.",
            "Do my homework.",
            "What's the weather tomorrow?",
            "Open Teams.",
            "Write a status update on a project for the board and the whole team by Friday afternoon please now",
            "",
        ] {
            assert_eq!(runs(q), None, "{q}");
        }
        // Off the menu, its own words aren't enough: named whole, it runs.
        assert_eq!(runs("Draft tickets for the work agreed."), None);
        assert_eq!(runs("Recap the week's meetings in one place."), None);
        assert_eq!(runs("Find the questions nobody answered."), None);
        // A skill that isn't there.
        assert_eq!(runs("Run my budget skill."), None);
        assert_eq!(runs("Slash budget."), None);
    }

    #[test]
    fn the_person_s_own_skills_by_their_names_and_descriptions() {
        let mine = [own("Deal review", "deal-review", "Review the risks in a sales deal")];
        assert_eq!(runs_in("Run my deal review skill.", &mine, &en()), hit("deal-review", ""));
        assert_eq!(runs_in("Slash deal review Northwind.", &mine, &en()), hit("deal-review", "Northwind"));
        assert_eq!(runs_in("Deal review for Northwind.", &mine, &en()), hit("deal-review", "Northwind"));
        assert_eq!(runs_in("Review the risks in a sales deal.", &mine, &en()), hit("deal-review", ""));
        assert_eq!(runs_in("Review the risks in a sales deal with Northwind.", &mine, &en()), hit("deal-review", "Northwind"));
        // Switched off the menu: only named whole.
        let off: Vec<Listed> = listed(&mine).into_iter().map(|l| Listed { shown: l.shown && l.skill.builtin, ..l }).collect();
        assert_eq!(matched("Deal review for Northwind.", &off, &en()), None);
        assert_eq!(matched("Run my deal review skill.", &off, &en()).map(|s| s.skill.command), Some("deal-review".into()));
        // Two Skills said alike: neither.
        let twins = [own("Weekly notes", "notes-one", "Sum up my week"), own("Weekly notes", "notes-two", "Sum up my week")];
        assert_eq!(runs_in("Run my weekly notes skill.", &twins, &en()), None);
        assert_eq!(runs_in("Sum up my week.", &twins, &en()), None);
        // …but its command said is that one.
        assert_eq!(runs_in("Slash notes two.", &twins, &en()), hit("notes-two", ""));
        // The person's copy of one of Yap's: theirs.
        let copy = Skill { based_on: "decisions".into(), ..own("Decisions", "my-decisions", "List each decision, who made it and why") };
        assert_eq!(runs_in("List the decisions.", std::slice::from_ref(&copy), &en()), hit("my-decisions", ""));
        // …but a command said is that command.
        assert_eq!(runs_in("Slash decisions.", &[copy], &en()), hit("decisions", ""));
    }

    #[test]
    fn other_languages_frame_it_in_the_person_s_own() {
        let fr = langs(&["fr"]);
        let hebdo = own("Point hebdo", "point-hebdo", "Prépare le point hebdo de l'équipe");
        assert_eq!(runs_in("Lance la skill point hebdo.", std::slice::from_ref(&hebdo), &fr), hit("point-hebdo", ""));
        assert_eq!(runs_in("Prépare-moi le point hebdo de l'équipe.", std::slice::from_ref(&hebdo), &fr), hit("point-hebdo", ""));
        assert_eq!(runs_in("Slash point hebdo, s'il te plaît.", std::slice::from_ref(&hebdo), &fr), hit("point-hebdo", ""));
        assert_eq!(runs_in("Peux-tu lancer la skill prep pour l'appel fournisseur ?", &[], &fr), hit("prep", "l'appel fournisseur"));
        assert_eq!(runs_in("Lance mon stand-up.", &[], &fr), hit("standup", ""));
        assert_eq!(runs_in("Lance la musique.", &[], &fr), None);
        // German: a compound "-Skill", and a separable verb's end.
        let de = langs(&["de"]);
        assert_eq!(runs_in("Starte meinen Standup-Skill.", &[], &de), hit("standup", ""));
        assert_eq!(runs_in("Führe bitte den Skill Decision Log aus.", &[], &de), hit("decision-log", ""));
        // Spanish, Italian, Portuguese, Dutch.
        assert_eq!(runs_in("Ejecuta la habilidad de stand-up.", &[], &langs(&["es"])), hit("standup", ""));
        assert_eq!(runs_in("Barra prep, por favor.", &[], &langs(&["es"])), hit("prep", ""));
        assert_eq!(runs_in("Esegui la skill tickets per la riunione con i fornitori.", &[], &langs(&["it"])), hit("tickets", "la riunione con i fornitori"));
        assert_eq!(runs_in("Executa a skill prep.", &[], &langs(&["pt"])), hit("prep", ""));
        assert_eq!(runs_in("Voer mijn standup skill uit.", &[], &langs(&["nl"])), hit("standup", ""));
        // Only the person's own languages: "barra" isn't English's "slash".
        assert_eq!(runs_in("Barra prep.", &[], &en()), None);
        // None of theirs known: all of them.
        assert_eq!(runs_in("Barra prep.", &[], &langs(&["ja"])), hit("prep", ""));
    }

    #[test]
    fn yap_s_question_is_answered_by_voice() {
        assert_eq!(answer_words("The supplier call.", &en()).as_deref(), Some("The supplier call"));
        assert_eq!(answer_words("With Priya.", &en()).as_deref(), Some("Priya"));
        assert_eq!(answer_words("It's the website relaunch.", &en()).as_deref(), Some("the website relaunch"));
        assert_eq!(answer_words("Um, the budget review please", &en()).as_deref(), Some("the budget review"));
        assert_eq!(answer_words("C'est le point fournisseur.", &langs(&["fr"])).as_deref(), Some("le point fournisseur"));
        // Not an answer.
        assert_eq!(answer_words("Never mind.", &en()), None);
        assert_eq!(answer_words("Cancel", &en()), None);
        assert_eq!(answer_words("Laisse tomber.", &langs(&["fr"])), None);
        assert_eq!(answer_words("What's the weather like?", &en()), None);
        assert_eq!(answer_words("I was thinking we should really look at the budget for the whole of next year", &en()), None);
        assert_eq!(answer_words("  ", &en()), None);
    }

    #[test]
    fn a_skill_on_one_meeting_reads_the_latest_or_the_one_named() {
        let follow = builtins().into_iter().find(|s| s.command == "followup-email").unwrap();
        assert_eq!(follow.runs_on, RunsOn::Meeting);
        let t = turn(follow, "the supplier call".into());
        assert_eq!(t.skill.gather, "latest");
        assert_eq!(t.words, "the supplier call");
        // One across the meetings is as it is.
        let prep = builtins().into_iter().find(|s| s.command == "prep").unwrap();
        assert_eq!(turn(prep, String::new()).skill.gather, "prep");
    }

    #[test]
    fn yap_s_question_waits_for_its_answer_in_its_conversation_only() {
        let prep = builtins().into_iter().find(|s| s.command == "prep").unwrap();
        wait_for_answer(7, &prep);
        assert_eq!(take_waiting(Some(8)), None);
        // Taken by any question, answer or not.
        assert_eq!(take_waiting(Some(7)), None);
        wait_for_answer(7, &prep);
        assert_eq!(take_waiting(Some(7)).map(|s| s.command), Some("prep".into()));
        assert_eq!(take_waiting(Some(7)), None);
    }

    #[test]
    fn words_are_read_as_speech_is_written() {
        let k = |t: &str| keys(&tokens(t));
        assert_eq!(k("Stand-up, to-dos and Priya's"), ["standup", "todos", "and", "priyas"]);
        assert_eq!(k("l'équipe qu'il s'il"), ["l", "équipe", "qu", "il", "s", "il"]);
        assert_eq!(k("Prépare-moi, peux-tu, Standup-Skill"), ["prépare", "moi", "peux", "tu", "standup", "skill"]);
        assert_eq!(k("one-to-one /prep"), ["onetoone", "prep"]);
        assert!(tokens("/prep it").first().unwrap().slashed);
        assert!(same("prep", "prepare") && same("decisions", "decision") && same("decided", "decide"));
        assert!(!same("list", "listen") && !same("prep", "pre"));
    }
}

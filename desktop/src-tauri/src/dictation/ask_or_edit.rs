//! **Ask or edit?** (2026-10-31; Wispr Flow's Command Mode, its "Lens", by
//! behaviour): the rewrite shortcut on selected text, with what was said a
//! question or a request for information — "what does this mean?", "is this
//! right?", "summarise this", "explain this error", "translate this for me" —
//! **answers instead of replacing the text**: the question goes to the Voice
//! Assistant's path with the selection as its quote (`chat/assistant.rs`,
//! `assistant/selected.rs`), and the selection is left as it was.
//!
//! The decision is safe: the selection is replaced only on a clear edit
//! instruction, and answered when in doubt.
//! 1. **A cheap rule** ([`rule`], pure) in the person's own languages (My
//!    languages' codes, English always; none known: all of them), the
//!    instruction's first words after fillers and "can you": a word that asks
//!    ("explain", "summarise", "tell me", "résume", "erklär") or opens a
//!    question ("what", "is", "pourquoi", "warum") answers; a word that edits
//!    ("make", "rewrite", "fix", "reformule", "kürze") or what the text should
//!    become ("shorter", "more formal", "in French") replaces; "translate"
//!    replaces unless it's for the person ("for me", "traduis-moi"); a
//!    question mark, or words that ask anywhere ("mean", "understand"),
//!    answer. A polite "can you …" whose words after it say neither isn't
//!    decided by its question mark ([`Rule::Unsure`]).
//! 2. **The model's own choice** where the rule can't tell ([`decide`]): one
//!    short request to the Commands scope's model with a structured reply,
//!    `{"action": "replace" | "answer"}` ([`PROMPT`], [`parse`]). Anything but
//!    a clear "replace" — another word, garbage, an error, a timeout, no
//!    model — answers.
//!
//! The log says which way and who decided, never the words.

use serde_json::{json, Value};

/// What the rule says about an instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// A clear edit: the selection is replaced.
    Edit,
    /// A question, or asking for information: answered.
    Ask,
    /// It can't tell: the model decides.
    Unsure,
}

/// What's done with the selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// Rewritten and pasted over (the rewrite shortcut as ever).
    Replace,
    /// Asked about, the answer in Chat (and out loud): nothing pasted.
    Answer,
}

/// Who decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum By {
    /// The rule.
    Rule,
    /// The model's structured reply.
    Model,
    /// The model couldn't say (no model, an error, a reply that wasn't
    /// one): answered, in doubt.
    Doubt,
    /// The selection can't be changed (read-only text): answered.
    ReadOnly,
}

/// The decision, and who made it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub choice: Choice,
    pub by: By,
}

impl Decision {
    /// "rule:edit", "model:answer", "doubt:answer", "readOnly:answer" (the
    /// robot's report and the log).
    pub fn label(&self) -> &'static str {
        match (self.by, self.choice) {
            (By::Rule, Choice::Replace) => "rule:edit",
            (By::Rule, Choice::Answer) => "rule:ask",
            (By::Model, Choice::Replace) => "model:replace",
            (By::Model, Choice::Answer) => "model:answer",
            (By::Doubt, _) => "doubt:answer",
            (By::ReadOnly, _) => "readOnly:answer",
        }
    }
}

/// One language's words for the rule. Every phrase is lower case; one
/// ending in `*` matches any word it begins ("summar*": summarise,
/// summarize, summary).
struct Lang {
    code: &'static str,
    /// Said first and dropped: "ok", "so", "please".
    fillers: &'static [&'static str],
    /// A polite request: "can you …". What comes after decides, never its
    /// question mark.
    polite: &'static [&'static str],
    /// Words that open a question: "what", "is", "pourquoi".
    question: &'static [&'static str],
    /// Asking for information about the text: "explain", "summarise",
    /// "tell me". (Before the edits: "make sense of" is no edit.)
    ask: &'static [&'static str],
    /// Asking for the text to be changed: "make", "rewrite", "fix".
    edit: &'static [&'static str],
    /// What the text should become: "shorter", "more formal", "as a list".
    becomes: &'static [&'static str],
    /// Translating: an edit, unless it's for the person ([`Lang::for_me`]).
    translate: &'static [&'static str],
    /// A translation for the person to read: "for me", "traduis-moi".
    for_me: &'static [&'static str],
    /// Words anywhere that ask about the text: "mean", "understand".
    asks_anywhere: &'static [&'static str],
    /// Languages that put the verb last after "can you" ("kannst du das
    /// kürzer machen"): that last word asks…
    ask_last: &'static [&'static str],
    /// …or edits…
    edit_last: &'static [&'static str],
    /// …or translates.
    translate_last: &'static [&'static str],
}

/// Language names after "in" / "into" in English: "in French" is a
/// translation, an edit.
const LANGUAGE_NAMES: &[&str] = &[
    "english", "british english", "american english", "plain english", "simple english", "french", "german", "spanish",
    "italian", "portuguese", "dutch", "chinese", "mandarin", "cantonese", "japanese", "korean", "russian", "arabic", "hindi",
    "polish", "swedish", "norwegian", "danish", "finnish", "greek", "turkish", "hebrew", "ukrainian", "czech", "romanian",
    "hungarian", "vietnamese", "thai", "indonesian", "malay", "welsh", "irish", "latin", "hinglish",
];

const LANGS: &[Lang] = &[
    Lang {
        code: "en",
        fillers: &[
            "ok", "okay", "so", "um", "umm", "uh", "er", "erm", "hmm", "hey yap", "hey", "hi", "yap", "right", "well", "just", "please",
            "and", "now", "alright", "all right", "actually", "quickly", "also", "i want you to", "i'd like you to",
            "i would like you to", "i need you to", "id like you to", "help me", "go ahead and", "let's", "lets", "try to", "try and",
        ],
        // (Only a request said as a question: its question mark says nothing.)
        polite: &[
            "would you mind", "do you mind", "can you please", "could you please", "can you", "could you", "would you", "will you", "can u",
            "could u", "can we", "could we",
        ],
        question: &[
            "what", "what's", "whats", "why", "how", "who", "whom", "whose", "when", "where", "which", "is", "isn't", "are", "aren't",
            "was", "were", "does", "doesn't", "did", "didn't", "do you", "do i", "do we", "do they", "don't", "should i", "should we",
            "should this", "should it", "has this", "has it", "have i", "have you", "am i", "any", "anything", "is there", "are there",
        ],
        ask: &[
            "explain*", "summar*", "brief summary", "short summary", "quick summary", "sum up", "sum it up", "sum this up", "tldr",
            "tl;dr", "tl dr", "tell me", "define", "describe", "check", "analyse", "analyze", "review", "evaluate", "assess", "critique",
            "interpret", "break down", "break it down", "break this down", "walk me through", "give me", "show me", "count", "add up",
            "find", "spot", "identify", "understand", "help me understand", "make sense of", "know", "i wonder", "wonder",
            "i don't understand", "i dont understand", "i'm not sure", "not sure", "eli5", "fact check", "fact-check", "verify",
            "double check", "double-check", "look up", "search", "google", "answer", "solve", "calculate", "compute", "meaning",
            "more about", "more info", "more information", "more context",
        ],
        // (Not "use", "keep", "say", "put" or "number" alone: "use this to
        // answer…", "say this is right?", "number of words?" aren't edits.)
        edit: &[
            "make", "rewrite", "re-write", "rephrase", "reword", "paraphrase", "shorten", "lengthen", "expand", "condense", "trim",
            "tighten", "simplify", "fix", "correct", "proofread", "polish", "improve", "edit", "change", "replace", "turn", "convert",
            "format", "reformat", "restructure", "reorganise", "reorganize", "sort", "reorder", "capitalise", "capitalize", "uppercase",
            "lowercase", "punctuate", "remove", "delete", "cut", "drop", "add", "insert", "append", "prepend", "put it", "put this",
            "swap", "split", "merge", "combine", "join", "tidy", "clean", "redo", "rework", "redraft", "soften", "tone down",
            "tone it down", "spell out", "spell check", "spellcheck", "write it", "write this", "write that", "number the",
            "number them", "number each", "bullet it", "bullet-point", "indent", "phrase it", "phrase this", "word it", "word this",
            "fill in", "find and replace", "search and replace",
        ],
        becomes: &[
            "shorter", "longer", "more ", "less ", "simpler", "clearer", "friendlier", "nicer", "politer", "warmer", "punchier",
            "snappier", "crisper", "tighter", "formal", "informal", "casual", "professional", "polite", "concise", "brief", "briefer",
            "as a ", "as an ", "as bullet", "as bullets", "bullet points", "bulleted", "into a list", "into bullet points",
            "into bullets", "in bullet points", "in bullets", "all caps", "all lowercase", "title case", "sentence case",
            "british spelling", "american spelling", "fewer words", "in one line", "in one sentence", "in plain language",
            "in simple terms",
        ],
        translate: &["translate"],
        for_me: &["for me", "so i can", "so that i can", "so i understand", "i can read", "i can understand", "what it says", "what this says", "what that says", "to me"],
        asks_anywhere: &["mean", "means", "meaning", "understand", "explain*", "summar*", "tell me", "make sense", "confus*", "wonder"],
        ask_last: &[],
        edit_last: &[],
        translate_last: &[],
    },
    Lang {
        code: "fr",
        fillers: &[
            "ok", "okay", "d'accord", "alors", "euh", "bon", "bah", "donc", "s'il te plaît", "s'il te plait", "s'il vous plaît",
            "s'il vous plait", "stp", "svp", "maintenant", "et", "juste", "merci de", "je veux que tu", "j'aimerais que tu",
            "je voudrais que tu", "aide-moi à", "aide-moi a",
        ],
        polite: &[
            "est-ce que tu peux", "est-ce que tu pourrais", "est-ce que vous pouvez", "est-ce que vous pourriez", "tu peux", "peux-tu",
            "pourrais-tu", "tu pourrais", "pouvez-vous", "vous pouvez", "pourriez-vous", "vous pourriez",
        ],
        question: &[
            "qu'est-ce", "qu'est ce", "que", "quoi", "quel", "quelle", "quels", "quelles", "pourquoi", "comment", "qui", "quand", "où",
            "combien", "est-ce", "est ce", "c'est quoi", "c'est correct", "c'est juste", "c'est bon", "ça veut dire", "ca veut dire",
            "y a-t-il", "y a t il", "est-il", "est-elle",
        ],
        ask: &[
            "explique-moi", "expliquez-moi", "explique", "expliquez", "m'expliquer", "résume-moi", "résumez-moi", "résume", "résumez",
            "me résumer", "fais-moi un résumé", "fais un résumé", "dis-moi", "dites-moi", "me dire", "vérifie", "vérifiez", "analyse",
            "analysez", "décris", "décrivez", "définis", "aide-moi à comprendre", "je ne comprends pas", "je comprends pas", "donne-moi",
            "trouve", "cherche", "réponds", "répondez",
        ],
        edit: &[
            "rends", "rendez", "reformule", "reformulez", "réécris", "réécrivez", "récris", "corrige", "corrigez", "raccourcis",
            "raccourcissez", "abrège", "simplifie", "simplifiez", "améliore", "améliorez", "change", "changez", "modifie", "modifiez",
            "remplace", "remplacez", "transforme", "transformez", "convertis", "mets", "mettez", "ajoute", "ajoutez", "supprime",
            "supprimez", "enlève", "enlevez", "retire", "retirez", "fais-en", "fais-le", "fais une", "écris", "écrivez", "formate",
            "développe", "allonge", "condense", "le rendre", "la rendre", "les rendre", "le reformuler", "la reformuler", "le réécrire",
            "le corriger", "la corriger", "le raccourcir", "la raccourcir",
        ],
        becomes: &[
            "plus court", "plus courte", "plus long", "plus longue", "plus formel", "plus formelle", "plus poli", "plus polie",
            "plus simple", "plus clair", "plus claire", "moins formel", "moins formelle", "en anglais", "en français", "en espagnol",
            "en allemand", "en italien", "en portugais", "en néerlandais", "en liste", "sous forme de liste", "en majuscules",
            "en minuscules", "en points",
        ],
        translate: &["traduis", "traduisez", "traduire", "me traduire"],
        for_me: &["-moi", "pour moi", "me traduire", "que je comprenne", "que je puisse"],
        asks_anywhere: &["veut dire", "signifie", "comprendre", "comprends", "explique*", "résum*"],
        ask_last: &[],
        edit_last: &[],
        translate_last: &[],
    },
    Lang {
        code: "de",
        fillers: &[
            "ok", "okay", "also", "äh", "ähm", "hm", "na", "so", "bitte", "jetzt", "und", "mal", "einfach", "hey", "ich möchte, dass du",
            "ich möchte dass du", "hilf mir",
        ],
        polite: &[
            "kannst du mir", "kannst du", "könntest du mir", "könntest du", "können sie mir", "können sie", "könnten sie", "würdest du",
            "würden sie",
        ],
        question: &[
            "was", "warum", "wieso", "weshalb", "wie", "wer", "wem", "wen", "wessen", "wann", "wo", "woher", "wohin", "welche",
            "welcher", "welches", "welchen", "ist", "sind", "war", "waren", "stimmt", "gibt es", "habe ich", "bedeutet", "heißt",
            "heisst", "macht das sinn", "ergibt das sinn", "soll ich", "sollte ich", "muss ich",
        ],
        ask: &[
            "erklär", "erkläre", "erklären sie", "fass", "fasse", "fassen sie", "sag mir", "sage mir", "sagen sie mir", "beschreib",
            "beschreibe", "prüf", "prüfe", "überprüf", "überprüfe", "analysier", "analysiere", "definier", "definiere", "ich verstehe",
            "ich versteh", "gib mir", "finde", "such", "suche", "beantworte", "beantworten sie",
        ],
        edit: &[
            "mach", "mache", "machen sie", "schreib", "schreibe", "schreiben sie", "formulier", "formuliere", "kürz", "kürze", "verkürze",
            "korrigier", "korrigiere", "verbessere", "verbesser", "ändere", "änder", "ersetz", "ersetze", "füg", "füge", "lösch",
            "lösche", "entfern", "entferne", "vereinfach", "vereinfache", "wandle", "wandel", "konvertier", "formatier", "formatiere",
            "sortier", "sortiere", "streich", "streiche", "überarbeite", "überarbeit", "setz", "setze",
        ],
        becomes: &[
            "kürzer", "länger", "formeller", "förmlicher", "lockerer", "freundlicher", "höflicher", "einfacher", "klarer", "auf englisch",
            "auf deutsch", "auf französisch", "auf spanisch", "auf italienisch", "als liste", "in stichpunkten", "in großbuchstaben",
        ],
        translate: &["übersetz", "übersetze", "übersetzen sie"],
        for_me: &["mir", "für mich", "damit ich"],
        asks_anywhere: &["bedeutet", "verstehe", "verstehen", "erklär*", "zusammenfass*"],
        ask_last: &["erklären", "zusammenfassen", "sagen", "beschreiben", "prüfen", "überprüfen", "analysieren", "beantworten"],
        edit_last: &[
            "machen", "kürzen", "verkürzen", "umschreiben", "umformulieren", "formulieren", "korrigieren", "verbessern", "ändern",
            "ersetzen", "vereinfachen", "schreiben", "überarbeiten", "formatieren", "löschen", "entfernen",
        ],
        translate_last: &["übersetzen"],
    },
    Lang {
        code: "es",
        fillers: &[
            "ok", "vale", "bueno", "pues", "eh", "por favor", "oye", "ahora", "y", "a ver", "mira", "quiero que", "necesito que",
            "ayúdame a", "ayudame a",
        ],
        polite: &["me puedes", "me podrías", "me podrias", "me puede", "puedes", "podrías", "podrias", "puede", "podría"],
        question: &[
            "qué", "que", "por qué", "porque", "porqué", "cómo", "como", "quién", "quien", "cuándo", "cuando", "dónde", "donde", "cuál",
            "cual", "cuáles", "cuánto", "cuanto", "es", "está", "hay", "tiene", "significa", "es correcto", "está bien",
        ],
        ask: &[
            "explícame", "explicame", "explica", "explique", "explicar", "explicarme", "resúmeme", "resumeme", "resume", "resumir",
            "resumirme", "haz un resumen", "hazme un resumen", "dime", "decirme", "revisa", "revisar", "comprueba", "comprobar",
            "analiza", "analizar", "describe", "describir", "define", "ayúdame a entender", "no entiendo", "busca", "encuentra",
            "responde", "contesta", "dame",
        ],
        edit: &[
            "haz", "hazlo", "hazla", "hacer", "hacerlo", "hacerla", "reescribe", "reescríbelo", "reescribelo", "reescribir",
            "reescribirlo", "reformula", "reformular", "corrige", "corrígelo", "corregir", "corregirlo", "acorta", "acórtalo", "acortar",
            "acortarlo", "simplifica", "simplificar", "simplificarlo", "mejora", "mejorar", "mejorarlo", "cambia", "cámbialo", "cambiar",
            "cambiarlo", "modifica", "reemplaza", "sustituye", "convierte", "convertir", "transforma",
            "pon", "ponlo", "ponla", "poner", "añade", "agrega", "quita", "borra", "elimina", "elimínalo", "formatea", "ordena",
            "escribe", "escríbelo", "redacta", "redáctalo", "amplía", "alarga",
        ],
        becomes: &[
            "más corto", "mas corto", "más corta", "más largo", "más formal", "mas formal", "menos formal", "más sencillo", "más simple",
            "más claro", "más amable", "en inglés", "en ingles", "en español", "en francés", "en alemán", "en italiano", "en portugués",
            "como lista", "como una lista", "en una lista", "en viñetas", "en mayúsculas", "en minúsculas", "que sea",
        ],
        translate: &["tradúcelo", "traducelo", "tradúceme", "traduceme", "traduce", "traducir", "traducirme"],
        for_me: &["para mí", "para mi", "tradúceme", "traduceme", "traducirme", "me lo traduces", "que entienda", "para entender"],
        asks_anywhere: &["significa", "entiendo", "entender", "explica*", "quiere decir"],
        ask_last: &[],
        edit_last: &[],
        translate_last: &[],
    },
    Lang {
        code: "it",
        fillers: &[
            "ok", "allora", "ehm", "beh", "dunque", "per favore", "per piacere", "ora", "adesso", "e", "senti", "dai", "vorrei che",
            "voglio che", "aiutami a",
        ],
        polite: &["mi puoi", "mi potresti", "puoi", "potresti", "può", "puo", "potrebbe"],
        question: &[
            "che cosa", "cosa", "cos'è", "cos'e", "che", "perché", "perche", "come", "chi", "quando", "dove", "quale", "quali", "quanto",
            "quanti", "è", "c'è", "ci sono", "significa", "è corretto", "va bene",
        ],
        ask: &[
            "spiegami", "spiega", "spiegare", "spiegarmi", "riassumimi", "riassumi", "riassumere", "fai un riassunto", "fammi un riassunto",
            "dimmi", "dirmi", "mi dici", "controlla", "controllare", "verifica", "analizza", "analizzare", "descrivi", "descrivere",
            "definisci", "aiutami a capire", "non capisco", "cerca", "trova", "rispondi", "dammi",
        ],
        edit: &[
            "rendilo", "rendila", "rendi", "rendere", "renderlo", "riscrivilo", "riscrivi", "riscrivere", "riscriverlo", "riformula",
            "riformulare", "correggilo", "correggi", "correggere", "correggerlo", "accorcialo", "accorcia", "accorciare", "accorciarlo",
            "semplifica", "semplificare", "semplificarlo", "migliora", "migliorare", "migliorarlo", "cambia", "cambiare", "cambiarlo",
            "modifica", "sostituisci", "trasforma", "trasformare", "converti", "metti", "mettilo",
            "aggiungi", "togli", "elimina", "cancella", "formatta", "ordina", "scrivi", "scrivilo", "allunga", "espandi", "sistema",
        ],
        becomes: &[
            "più corto", "piu corto", "più breve", "più lungo", "più formale", "meno formale", "più semplice", "più chiaro", "più gentile",
            "in inglese", "in italiano", "in francese", "in tedesco", "in spagnolo", "in portoghese", "come lista", "come elenco",
            "in un elenco", "a punti", "in maiuscolo", "in minuscolo",
        ],
        translate: &["traducimi", "traducilo", "traduci", "tradurre", "tradurlo", "tradurmi"],
        for_me: &["per me", "traducimi", "tradurmi", "mi traduci", "per capire", "così capisco"],
        asks_anywhere: &["significa", "capisco", "capire", "spieg*", "vuol dire"],
        ask_last: &[],
        edit_last: &[],
        translate_last: &[],
    },
    Lang {
        code: "pt",
        fillers: &["ok", "então", "entao", "bem", "tipo", "por favor", "agora", "e", "olha", "quero que", "preciso que", "ajuda-me a", "me ajuda a"],
        polite: &["você pode", "voce pode", "podes", "pode me", "pode", "poderia", "consegue", "consegues", "me pode"],
        question: &[
            "o que", "o quê", "que", "quê", "por que", "por quê", "porque", "porquê", "como", "quem", "quando", "onde", "qual", "quais",
            "quanto", "quantos", "é", "está", "há", "tem", "isso está", "isto está", "significa",
        ],
        ask: &[
            "explica-me", "me explica", "explica", "explique", "explicar", "explicar-me", "me explicar", "resume-me", "resume", "resuma",
            "resumir", "faz um resumo", "faça um resumo", "diz-me", "me diz", "diga-me", "me diga", "dizer-me", "me dizer", "verifica",
            "verifique", "verificar", "analisa", "analise", "analisar", "descreve", "descreva", "descrever", "define", "ajuda-me a entender",
            "não entendo", "nao entendo", "procura", "encontra", "responde", "responda", "dá-me", "me dá",
        ],
        edit: &[
            "faz", "faça", "faca", "fazer", "torna", "torne", "tornar", "reescreve", "reescreva", "reescrever", "reformula", "reformule",
            "reformular", "corrige", "corrija", "corrigir", "encurta", "encurte", "encurtar", "simplifica", "simplifique", "simplificar",
            "melhora", "melhore", "melhorar", "muda", "mude", "mudar", "altera", "altere", "alterar", "substitui", "substitua",
            "transforma", "transforme", "converte", "converta", "põe", "poe", "coloca", "coloque", "adiciona", "adicione", "acrescenta",
            "tira", "tire", "remove", "remova", "apaga", "apague", "formata", "formate", "ordena", "escreve", "escreva", "redige",
            "amplia", "expande",
        ],
        becomes: &[
            "mais curto", "mais curta", "mais longo", "mais formal", "menos formal", "mais simples", "mais claro", "mais educado",
            "em inglês", "em ingles", "em português", "em espanhol", "em francês", "em alemão", "em italiano", "como lista", "em lista",
            "em tópicos", "em maiúsculas", "em minúsculas",
        ],
        translate: &["traduz-me", "traduz", "traduza", "traduzir", "me traduzir", "traduzir-me"],
        for_me: &["para mim", "pra mim", "traduz-me", "traduzir-me", "me traduz", "me traduzir", "para eu entender", "para perceber"],
        asks_anywhere: &["significa", "entendo", "entender", "explic*", "quer dizer"],
        ask_last: &[],
        edit_last: &[],
        translate_last: &[],
    },
    Lang {
        code: "nl",
        fillers: &[
            "oké", "ok", "okay", "dus", "eh", "uh", "nou", "alsjeblieft", "alstublieft", "graag", "nu", "en", "even", "gewoon", "hé", "hey",
            "help me",
        ],
        polite: &["kun je", "kan je", "kunt u", "kun jij", "zou je", "zou u", "wil je", "wilt u"],
        question: &[
            "wat", "waarom", "hoe", "wie", "wanneer", "waar", "welke", "welk", "is", "zijn", "klopt", "betekent", "heeft", "hebben",
            "staat", "zit er",
        ],
        ask: &[
            "leg me uit", "leg uit", "leg", "vat samen", "vat", "vertel me", "vertel", "zeg me", "controleer", "check", "analyseer",
            "beschrijf", "definieer", "help me begrijpen", "ik begrijp", "ik snap", "zoek", "vind", "beantwoord", "geef me",
        ],
        edit: &[
            "maak", "herschrijf", "herformuleer", "verbeter", "corrigeer", "kort", "vereenvoudig", "verander", "wijzig", "pas", "vervang",
            "zet", "voeg", "verwijder", "schrap", "schrijf", "formatteer", "sorteer", "orden", "breid",
        ],
        becomes: &[
            "korter", "langer", "formeler", "informeler", "vriendelijker", "beleefder", "simpeler", "eenvoudiger", "duidelijker",
            "in het engels", "in het nederlands", "in het frans", "in het duits", "in het spaans", "als lijst", "als een lijst",
            "in opsommingstekens", "in hoofdletters",
        ],
        translate: &["vertaal"],
        for_me: &["voor mij", "voor me", "zodat ik"],
        asks_anywhere: &["betekent", "begrijp", "snap", "uitleg*", "samenvat*"],
        ask_last: &["uitleggen", "samenvatten", "zeggen", "vertellen", "beschrijven", "controleren", "nakijken", "analyseren"],
        edit_last: &[
            "maken", "herschrijven", "inkorten", "verkorten", "verbeteren", "corrigeren", "aanpassen", "veranderen", "vervangen",
            "vereenvoudigen", "herformuleren", "omzetten", "schrijven",
        ],
        translate_last: &["vertalen"],
    },
];

/// The languages whose words count: the person's own (My languages' codes:
/// "fr", "pt-BR"…), English always; none of theirs known here (or none
/// picked): every one — as `assistant/screen.rs` counts a "this". Italian
/// "come" or Dutch "wat" mean something else in English. Pure.
fn langs_for(codes: &[String]) -> Vec<&'static Lang> {
    let known = |l: &Lang| codes.iter().any(|c| c.to_ascii_lowercase().starts_with(l.code));
    let all = codes.iter().all(|c| !LANGS.iter().any(|l| c.to_ascii_lowercase().starts_with(l.code)));
    LANGS.iter().filter(|l| all || l.code == "en" || known(l)).collect()
}

/// What an instruction's first words are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Start {
    // (In the order a tie goes: the first wins.)
    Ask,
    Translate,
    Edit,
    Question,
    Polite,
    Filler,
}

/// The longest phrase of any category, in any of `langs`, that starts
/// `text`: its category and how much of `text` it covers (a tie goes to
/// the category first in [`Start`]). "Make sense of" asks though "make"
/// edits; French "en anglais" is a translation though Dutch "en" is "and".
fn start_of(text: &str, langs: &[&Lang]) -> Option<(Start, usize)> {
    let mut best: Option<(Start, usize)> = None;
    let mut offer = |cat: Start, n: Option<usize>| {
        if let Some(n) = n {
            let better = match best {
                None => true,
                Some((c, m)) => n > m || (n == m && cat < c),
            };
            if better {
                best = Some((cat, n));
            }
        }
    };
    for l in langs {
        offer(Start::Ask, first_of(text, l.ask));
        offer(Start::Translate, first_of(text, l.translate));
        offer(Start::Edit, first_of(text, l.edit));
        offer(Start::Edit, first_of(text, l.becomes));
        offer(Start::Question, first_of(text, l.question));
        offer(Start::Polite, first_of(text, l.polite));
        offer(Start::Filler, first_of(text, l.fillers));
    }
    // English "in French", "into Spanish".
    for name in LANGUAGE_NAMES {
        for at in ["in", "into"] {
            offer(Start::Edit, starts(text, &format!("{at} {name}")));
        }
    }
    best
}

/// Is `c` part of a word?
fn wordish(c: char) -> bool {
    c.is_alphanumeric()
}

/// Does `phrase` match at the start of `text` (a word boundary after it,
/// unless it ends in a space or a hyphen, or is a `*` stem)? How much of
/// `text` it covers.
fn starts(text: &str, phrase: &str) -> Option<usize> {
    if let Some(stem) = phrase.strip_suffix('*') {
        if !text.starts_with(stem) {
            return None;
        }
        let rest = &text[stem.len()..];
        let word = rest.char_indices().find(|(_, c)| !wordish(*c)).map_or(rest.len(), |(i, _)| i);
        return Some(stem.len() + word);
    }
    if !text.starts_with(phrase) {
        return None;
    }
    let ends_open = phrase.chars().last().is_some_and(|c| !wordish(c));
    match text[phrase.len()..].chars().next() {
        None => Some(phrase.len()),
        Some(_) if ends_open => Some(phrase.len()),
        Some(c) if !wordish(c) => Some(phrase.len()),
        _ => None,
    }
}

/// Does `phrase` occur anywhere in `text` as words (`*` stems, phrases that
/// start or end with a hyphen or a space, as [`starts`])?
fn contains(text: &str, phrase: &str) -> bool {
    let stem = phrase.strip_suffix('*');
    let needle = stem.unwrap_or(phrase);
    if needle.is_empty() {
        return false;
    }
    let opens = needle.chars().next().is_some_and(wordish);
    let closes = stem.is_none() && needle.chars().last().is_some_and(wordish);
    text.match_indices(needle).any(|(i, _)| {
        let before_ok = !opens || text[..i].chars().next_back().is_none_or(|c| !wordish(c));
        let after_ok = !closes || text[i + needle.len()..].chars().next().is_none_or(|c| !wordish(c));
        before_ok && after_ok
    })
}

/// The first of `phrases` (longest first) that starts `text`: how much it
/// covers.
fn first_of(text: &str, phrases: &[&str]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for p in phrases {
        if let Some(n) = starts(text, p) {
            if best.is_none_or(|b| n > b) {
                best = Some(n);
            }
        }
    }
    best
}

/// `text` from `at` on, without the commas and spaces after what was cut.
fn after(text: &str, at: usize) -> &str {
    text[at..].trim_start_matches(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | ':' | '.' | '!' | '-'))
}

/// The instruction as the rule reads it: lower case, straight apostrophes,
/// one space between words, the quotes and the Spanish ¿¡ at its start
/// gone, and whether it was asked as a question (a question mark anywhere).
/// Pure.
fn normal(instruction: &str) -> (String, bool) {
    let asked = instruction.contains('?') || instruction.contains('¿');
    let text = instruction
        .to_lowercase()
        .replace(['\u{2019}', '\u{2018}', '`'], "'")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let text = text
        .trim_start_matches(|c: char| matches!(c, '¿' | '¡' | '"' | '\'' | '“' | '”' | '«' | '»' | '(' | '-') || c.is_whitespace())
        .trim_end_matches(|c: char| matches!(c, '?' | '.' | '!' | '"' | '\'' | '”' | '»' | ')' | '…') || c.is_whitespace())
        .to_string();
    (text, asked)
}

/// **Is a spoken instruction about a selection an edit or a question?** By
/// its words alone, in the person's own languages (`langs`: My languages'
/// codes; English always) — see the module's notes. Pure.
pub fn rule(instruction: &str, langs: &[String]) -> Rule {
    let (whole, asked) = normal(instruction);
    if whole.is_empty() {
        return Rule::Unsure;
    }
    let langs = langs_for(langs);
    let for_me = || langs.iter().any(|l| l.for_me.iter().any(|p| contains(&whole, p)));

    // The first words, with fillers and "can you …" taken off the front a
    // few times over ("ok so could you please …").
    let mut text: &str = &whole;
    let mut polite = false;
    for _ in 0..6 {
        match start_of(text, &langs) {
            // A word that asks ("explain", "summarise", "tell me"), or
            // opens a question ("what", "is", "pourquoi"): answered.
            Some((Start::Ask | Start::Question, _)) => return Rule::Ask,
            // "Translate": an edit, unless it's for the person to read.
            Some((Start::Translate, _)) => return if for_me() { Rule::Ask } else { Rule::Edit },
            // A word that edits, or what the text should become: replaced —
            // but one word asked ("Correct?", "Shorter?") is the model's call.
            Some((Start::Edit, _)) => {
                let one_word = whole.split_whitespace().count() <= 1;
                return if asked && one_word { Rule::Unsure } else { Rule::Edit };
            }
            Some((cat @ (Start::Polite | Start::Filler), n)) => {
                let rest = after(text, n);
                if rest.is_empty() {
                    break;
                }
                polite |= cat == Start::Polite;
                text = rest;
            }
            None => break,
        }
    }

    // "Can you …" with neither after it: its last word, in a language that
    // puts the verb there ("kannst du das kürzer machen?"), else the
    // model's call — never its question mark ("can you look at this?").
    if polite {
        let last = text.rsplit(|c: char| !wordish(c)).find(|w| !w.is_empty()).unwrap_or("");
        for l in &langs {
            if l.translate_last.contains(&last) {
                return if for_me() { Rule::Ask } else { Rule::Edit };
            }
            if l.ask_last.contains(&last) {
                return Rule::Ask;
            }
            if l.edit_last.contains(&last) {
                return Rule::Edit;
            }
        }
        return Rule::Unsure;
    }
    // Asked as a question, or words that ask anywhere: answered.
    if asked || langs.iter().any(|l| l.asks_anywhere.iter().any(|p| contains(text, p))) {
        return Rule::Ask;
    }
    Rule::Unsure
}

// ---- …or the model's choice -----------------------------------------------------------

/// The request for an instruction the rule can't tell (no editable body:
/// Yap's own question).
pub const PROMPT: &str = "You decide what someone wants done with text they selected in an app, from the instruction they said. Reply with JSON only, no other words: {\"action\": \"replace\"} when they asked for the selected text itself to be changed and put back in its place (rewritten, shortened, corrected, reformatted, translated into another language, turned into a list), or {\"action\": \"answer\"} when they asked a question about it or for something to read (what it means, whether it's right, a summary or an explanation for them, advice, what to reply). When you are not sure, answer. The instruction may be in any language.";

/// The most of the selection the request quotes, to tell "reply to this"
/// about a mail from one about a draft.
const QUOTED_CHARS: usize = 300;

/// The request's messages: the instruction, and the start of the
/// selection. Pure.
pub fn messages(instruction: &str, selection: &str) -> Value {
    let start: String = selection.trim().chars().take(QUOTED_CHARS).collect();
    let more = if selection.trim().chars().count() > QUOTED_CHARS { "…" } else { "" };
    let user = format!(
        "The instruction: \"{}\"\n\nThe selected text begins: \"{start}{more}\"\n\nReply with {{\"action\": \"replace\"}} or {{\"action\": \"answer\"}}.",
        instruction.trim()
    );
    json!([
        { "role": "system", "content": PROMPT },
        { "role": "user", "content": user },
    ])
}

/// **The model's structured reply, read** (pure): `{"action": "replace"}`
/// (in a code fence, with other fields, any case) or a reply whose first
/// word is "replace" → [`Choice::Replace`]; the same for "answer" →
/// [`Choice::Answer`]; anything else → `None` (in doubt: answered).
pub fn parse(reply: &str) -> Option<Choice> {
    let text = reply.trim();
    let word = |w: &str| match w.trim().to_ascii_lowercase().as_str() {
        "replace" => Some(Choice::Replace),
        "answer" => Some(Choice::Answer),
        _ => None,
    };
    // A JSON object, wherever it is in the reply.
    if let (Some(open), Some(close)) = (text.find('{'), text.rfind('}')) {
        if open < close {
            if let Ok(v) = serde_json::from_str::<Value>(&text[open..=close]) {
                return v.get("action").and_then(Value::as_str).and_then(word);
            }
            // Almost JSON ({action: 'replace'}): the word after "action".
            let inner = text[open..=close].to_ascii_lowercase();
            let after_key = inner.split("action").nth(1)?;
            let value = after_key.trim_start_matches(|c: char| !c.is_alphanumeric()).split(|c: char| !c.is_alphanumeric()).next()?;
            return word(value);
        }
    }
    let first = text.trim_start_matches(|c: char| !c.is_alphanumeric()).split(|c: char| !c.is_alphanumeric()).next().unwrap_or("");
    word(first)
}

/// Where the model's choice is asked.
pub struct Endpoint<'a> {
    pub base_url: &'a str,
    pub api_key: &'a str,
    pub model: &'a str,
    pub provider: &'a str,
}

/// **Edit or answer?** The rule; where it can't tell, the Commands model's
/// structured reply (≤ 8 s; a model on this PC 15 s, it may be loading);
/// in doubt, answered. `None` endpoint: no model to ask.
pub async fn decide(instruction: &str, selection: &str, langs: &[String], ep: Option<Endpoint<'_>>) -> Decision {
    match rule(instruction, langs) {
        Rule::Edit => return Decision { choice: Choice::Replace, by: By::Rule },
        Rule::Ask => return Decision { choice: Choice::Answer, by: By::Rule },
        Rule::Unsure => {}
    }
    let doubt = Decision { choice: Choice::Answer, by: By::Doubt };
    let Some(ep) = ep.filter(|e| !e.base_url.trim().is_empty()) else {
        tracing::info!("Commands: no model to ask whether it's an edit or a question; answered");
        return doubt;
    };
    let here = crate::ai::ai_model::runs_here(ep.provider, ep.base_url);
    let opts = crate::ai::llm::ChatOptions {
        temperature: 0.0,
        max_tokens: here.then_some(24),
        timeout: std::time::Duration::from_secs(if here { 15 } else { 8 }),
        disable_thinking: true,
    };
    let t = std::time::Instant::now();
    let asked = crate::privacy::net::within(
        crate::privacy::net::Feature::commands(false),
        crate::ai::llm::chat_with(
            ep.base_url,
            ep.api_key,
            ep.model,
            ep.provider,
            messages(instruction, selection),
            &opts,
            crate::chat::compact::extra(ep.model),
        ),
    )
    .await;
    let ms = t.elapsed().as_millis() as u64;
    match asked {
        Ok(reply) => match parse(&reply) {
            Some(choice) => {
                tracing::info!(?choice, ms, "Commands: the model said whether it's an edit or a question");
                Decision { choice, by: By::Model }
            }
            None => {
                tracing::info!(ms, chars = reply.chars().count(), "Commands: the model's reply wasn't a choice; answered");
                doubt
            }
        },
        Err(e) => {
            tracing::info!(ms, error = %crate::ai::llm::error_kind(&e), "Commands: couldn't ask whether it's an edit or a question; answered");
            doubt
        }
    }
}

/// **Edit or answer, before the model** (pure): a selection that can't be
/// changed (read-only text: a web page, a mail being read) is always
/// answered — nothing could be put in its place; else the rule, `None`
/// where it's the model's call.
pub fn settled(instruction: &str, langs: &[String], read_only: bool) -> Option<Decision> {
    if read_only {
        return Some(Decision { choice: Choice::Answer, by: By::ReadOnly });
    }
    match rule(instruction, langs) {
        Rule::Edit => Some(Decision { choice: Choice::Replace, by: By::Rule }),
        Rule::Ask => Some(Decision { choice: Choice::Answer, by: By::Rule }),
        Rule::Unsure => None,
    }
}

/// What the rewrite shortcut does with what was said ([`route`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// The rewrite shortcut as before: the selection or the last dictation
    /// rewritten, or new text written.
    Rewrite,
    /// Ask or edit: [`settled`], then the model.
    Decide,
}

/// **What the rewrite shortcut does** (pure): `on` ("Answer questions about
/// the selected text", Settings → Commands), something selected (`selection`,
/// not the last dictation's rewrite: that's an edit by its nature — nothing
/// selected keeps today's behaviour) and words said — then it's to be
/// decided ([`Route::Decide`]); else the rewrite as ever.
pub fn route(on: bool, selection: &str, last_dictation: bool, instruction: &str) -> Route {
    if on && !last_dictation && !selection.trim().is_empty() && !instruction.trim().is_empty() {
        Route::Decide
    } else {
        Route::Rewrite
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn langs(codes: &[&str]) -> Vec<String> {
        codes.iter().map(|c| c.to_string()).collect()
    }

    fn check(table: &[(&str, Rule)], codes: &[&str]) {
        let langs = langs(codes);
        for (said, want) in table {
            assert_eq!(rule(said, &langs), *want, "{said:?} in {codes:?}");
        }
    }

    #[test]
    fn english_questions_are_answered_and_edits_replace() {
        use Rule::*;
        check(
            &[
                // Questions and asking for information.
                ("What does this mean?", Ask),
                ("what does this mean", Ask),
                ("Is this right?", Ask),
                ("is this grammatically correct", Ask),
                ("Summarise this.", Ask),
                ("summarize this in three bullet points", Ask),
                ("Explain this error", Ask),
                ("explain it like I'm five", Ask),
                ("Translate this for me.", Ask),
                ("translate it so I can understand it", Ask),
                ("Can you explain this?", Ask),
                ("could you tell me what this says", Ask),
                ("Okay so what's the tone of this?", Ask),
                ("Why does it say that?", Ask),
                ("Does this make sense?", Ask),
                ("Any typos?", Ask),
                ("check this for mistakes", Ask),
                ("give me three titles for this", Ask),
                ("help me understand this", Ask),
                ("make sense of this for me", Ask),
                ("I don't understand this paragraph", Ask),
                ("this bit, what does it mean", Ask),
                ("Answer this question", Ask),
                ("add up these numbers", Ask),
                ("Number of words?", Ask),
                ("let's see what this means", Ask),
                // Clear edits.
                ("Make it shorter.", Edit),
                ("make this a bulleted list", Edit),
                ("Make it shorter?", Edit),
                ("Can you make this more formal?", Edit),
                ("could you please rewrite it in plain English", Edit),
                ("Rewrite this as an email to Priya", Edit),
                ("fix the typos", Edit),
                ("Please fix the grammar.", Edit),
                ("shorter", Edit),
                ("More formal", Edit),
                ("in French", Edit),
                ("Into Spanish please", Edit),
                ("Translate this into German", Edit),
                ("turn it into a tweet", Edit),
                ("as a numbered list", Edit),
                ("Hey Yap, make it friendlier", Edit),
                ("now make it shorter", Edit),
                ("I need you to rewrite this so it sounds more professional", Edit),
                ("find and replace colour with color", Edit),
                // Neither, plainly: the model's call.
                ("Reply to this", Unsure),
                ("Can you look at this?", Unsure),
                ("Would you say this is correct?", Unsure),
                ("Correct?", Unsure),
                ("use British spelling", Unsure),
                ("Spanish", Unsure),
                ("the second paragraph only", Unsure),
            ],
            &["en"],
        );
    }

    #[test]
    fn french_questions_are_answered_and_edits_replace() {
        use Rule::*;
        check(
            &[
                ("Qu'est-ce que ça veut dire ?", Ask),
                ("C'est correct ?", Ask),
                ("Pourquoi il dit ça", Ask),
                ("Résume ce texte", Ask),
                ("Explique-moi cette erreur", Ask),
                ("Traduis-moi ça", Ask),
                ("Tu peux m'expliquer ça ?", Ask),
                ("Rends-le plus court", Edit),
                ("Reformule ce paragraphe", Edit),
                ("Corrige les fautes", Edit),
                ("Plus formel", Edit),
                ("En anglais", Edit),
                ("Traduis en espagnol", Edit),
                ("Tu peux le rendre plus court ?", Edit),
                ("s'il te plaît, raccourcis-le", Edit),
                ("Tu peux regarder ça ?", Unsure),
            ],
            &["en", "fr"],
        );
    }

    #[test]
    fn german_questions_are_answered_and_edits_replace() {
        use Rule::*;
        check(
            &[
                ("Was bedeutet das?", Ask),
                ("Stimmt das so?", Ask),
                ("Warum steht da das", Ask),
                ("Erklär mir diesen Fehler", Ask),
                ("Fass das zusammen", Ask),
                ("Übersetz mir das", Ask),
                ("Kannst du mir das erklären?", Ask),
                ("Mach es kürzer", Edit),
                ("Formuliere das freundlicher", Edit),
                ("Korrigiere die Rechtschreibung", Edit),
                ("Kürzer", Edit),
                ("Auf Englisch", Edit),
                ("Übersetze das ins Englische", Edit),
                ("Kannst du das kürzer machen?", Edit),
                ("Kannst du das umformulieren?", Edit),
                ("Kannst du das anschauen?", Unsure),
            ],
            &["de"],
        );
    }

    #[test]
    fn spanish_italian_portuguese_and_dutch_too() {
        use Rule::*;
        check(
            &[
                ("¿Qué significa esto?", Ask),
                ("¿Está bien escrito?", Ask),
                ("Explícame este error", Ask),
                ("Resume esto", Ask),
                ("Tradúceme esto", Ask),
                ("Hazlo más corto", Edit),
                ("Más formal", Edit),
                ("Corrige la ortografía", Edit),
                ("Traduce esto al inglés", Edit),
                ("Como una lista", Edit),
                ("¿Me puedes explicar esto?", Ask),
                ("¿Puedes acortarlo?", Edit),
            ],
            &["es"],
        );
        check(
            &[
                ("Cosa significa?", Ask),
                ("È corretto?", Ask),
                ("Spiegami questo errore", Ask),
                ("Riassumi", Ask),
                ("Traducimi questo", Ask),
                ("Rendilo più corto", Edit),
                ("Più formale", Edit),
                ("Correggi gli errori", Edit),
                ("Come lista", Edit),
                ("Puoi riscriverlo?", Edit),
            ],
            &["it"],
        );
        check(
            &[
                ("O que significa isto?", Ask),
                ("Isso está certo?", Ask),
                ("Explica-me este erro", Ask),
                ("Traduz para mim", Ask),
                ("Torna isto mais curto", Edit),
                ("Mais formal", Edit),
                ("Corrige os erros", Edit),
                ("Em inglês", Edit),
            ],
            &["pt-BR"],
        );
        check(
            &[
                ("Wat betekent dit?", Ask),
                ("Klopt dit?", Ask),
                ("Leg dit uit", Ask),
                ("Vat dit samen", Ask),
                ("Vertaal dit voor mij", Ask),
                ("Maak het korter", Edit),
                ("Korter", Edit),
                ("Herschrijf dit", Edit),
                ("Vertaal naar het Engels", Edit),
                ("Kun je dit korter maken?", Edit),
                ("Kun je dit uitleggen?", Ask),
            ],
            &["nl"],
        );
    }

    #[test]
    fn only_the_persons_own_languages_count_with_english_always() {
        // Italian "come" ("how") is no English question word: "come up with
        // a shorter one" isn't asked in Italian here.
        assert_eq!(rule("come up with a shorter one", &langs(&["en"])), Rule::Unsure);
        assert_eq!(rule("come si dice", &langs(&["en", "it"])), Rule::Ask);
        // German "was" is English too; Dutch "wat" only with Dutch.
        assert_eq!(rule("wat betekent dit", &langs(&["en"])), Rule::Unsure);
        assert_eq!(rule("wat betekent dit", &langs(&["en", "nl"])), Rule::Ask);
        // English counts in anyone's languages.
        assert_eq!(rule("what does this mean", &langs(&["fr"])), Rule::Ask);
        assert_eq!(rule("make it shorter", &langs(&["de"])), Rule::Edit);
        // None of them known here (or none picked): every language counts.
        assert_eq!(rule("Rends-le plus court", &[]), Rule::Edit);
        assert_eq!(rule("Rends-le plus court", &langs(&["ja"])), Rule::Edit);
        assert_eq!(rule("Was bedeutet das?", &langs(&["hi"])), Rule::Ask);
    }

    #[test]
    fn nothing_said_is_nobodys_call() {
        assert_eq!(rule("", &[]), Rule::Unsure);
        assert_eq!(rule("  ?  ", &[]), Rule::Unsure);
        assert_eq!(rule("um", &langs(&["en"])), Rule::Unsure);
    }

    #[test]
    fn the_models_structured_reply_is_read_and_anything_else_answers() {
        assert_eq!(parse(r#"{"action":"replace"}"#), Some(Choice::Replace));
        assert_eq!(parse(r#"{"action": "answer"}"#), Some(Choice::Answer));
        assert_eq!(parse("```json\n{\"action\": \"REPLACE\", \"why\": \"an edit\"}\n```"), Some(Choice::Replace));
        assert_eq!(parse("Sure: {\"action\":\"answer\"}"), Some(Choice::Answer));
        assert_eq!(parse("{action: 'replace'}"), Some(Choice::Replace));
        assert_eq!(parse("replace"), Some(Choice::Replace));
        assert_eq!(parse("Answer."), Some(Choice::Answer));
        // Anything else is no choice (answered, in doubt).
        assert_eq!(parse(r#"{"action":"rewrite"}"#), None);
        assert_eq!(parse(r#"{"choice":"replace"}"#), None);
        assert_eq!(parse("I think they want it changed."), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn the_request_says_the_instruction_and_the_start_of_the_selection() {
        let long = "word ".repeat(200);
        let m = messages("  reply to this  ", &long);
        assert_eq!(m[0]["content"], PROMPT);
        let user = m[1]["content"].as_str().unwrap();
        assert!(user.starts_with("The instruction: \"reply to this\""));
        assert!(user.contains("The selected text begins: \"word word"));
        assert!(user.contains("…\""), "cut short, and says so");
        assert!(user.chars().count() < 600);
    }

    #[test]
    fn a_read_only_selection_is_always_answered_and_the_rule_decides_the_rest() {
        let en = langs(&["en"]);
        assert_eq!(settled("make it shorter", &en, true), Some(Decision { choice: Choice::Answer, by: By::ReadOnly }));
        assert_eq!(settled("make it shorter", &en, false), Some(Decision { choice: Choice::Replace, by: By::Rule }));
        assert_eq!(settled("what does this mean", &en, false), Some(Decision { choice: Choice::Answer, by: By::Rule }));
        assert_eq!(settled("reply to this", &en, false), None);
        assert_eq!(Decision { choice: Choice::Answer, by: By::Doubt }.label(), "doubt:answer");
        assert_eq!(Decision { choice: Choice::Replace, by: By::Model }.label(), "model:replace");
    }

    #[test]
    fn only_a_selection_with_words_said_is_decided_and_nothing_selected_rewrites_as_ever() {
        assert_eq!(route(true, "Some text", false, "what does this mean"), Route::Decide);
        // Nothing selected: the last dictation, or new text, as before.
        assert_eq!(route(true, "", false, "what does this mean"), Route::Rewrite);
        assert_eq!(route(true, "  ", false, "what does this mean"), Route::Rewrite);
        assert_eq!(route(true, "Some text", true, "what does this mean"), Route::Rewrite);
        // Nothing said: today's quiet nothing.
        assert_eq!(route(true, "Some text", false, " "), Route::Rewrite);
        // "Answer questions about the selected text" off: the rewrite as ever.
        assert_eq!(route(false, "Some text", false, "what does this mean"), Route::Rewrite);
    }

    #[tokio::test]
    async fn with_no_model_to_ask_an_unsure_one_is_answered() {
        let d = decide("reply to this", "Hi, can we meet on Friday?", &langs(&["en"]), None).await;
        assert_eq!(d, Decision { choice: Choice::Answer, by: By::Doubt });
        let blank = Endpoint { base_url: " ", api_key: "", model: "", provider: "custom" };
        assert_eq!(decide("reply to this", "x", &[], Some(blank)).await.choice, Choice::Answer);
        // The rule needs no model.
        assert_eq!(decide("make it shorter", "x", &[], None).await, Decision { choice: Choice::Replace, by: By::Rule });
    }
}

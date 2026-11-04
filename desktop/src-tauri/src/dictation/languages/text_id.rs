//! **The language a dictation's text is in** (2026-10-31), for the speech
//! models that write a language without saying which (Parakeet V3,
//! SenseVoice) and the English-only ones (Parakeet V2, Moonshine…): what the
//! "You spoke French" nudge goes on when Whisper isn't there to say. On this
//! PC, in well under a millisecond, never the network. Pure.
//!
//! - **Two signals that must agree** ([`identify`]): the language with the
//!   most of its own **function words** in the text ([`WORDS`]: articles,
//!   pronouns, prepositions, the commonest dictation words — "je", "pour",
//!   "demain"), and **whatlang**'s trigram profiles over the same
//!   candidates. A script one language writes decides by itself (Greek,
//!   Korean, Japanese' kana; Chinese, or Cantonese by its own characters,
//!   [`CANTONESE`]); Maltese by its letters (ħ ġ ċ, whatlang has no
//!   Maltese) and words.
//! - **The candidates** ([`candidates`]): the languages the model writes and
//!   the person's own; for an English-only model, Parakeet V3's 25 (what it
//!   might half-write when someone speaks one of them).
//! - **Only when it's sure** ([`sure`]): at least [`MIN_WORDS`] words (ten
//!   CJK characters); at least [`MIN_HITS`] of that language's function
//!   words, more than any other candidate's and more than twice the
//!   person's own languages' — so English full of names, places, brands or
//!   code ("Ask Ingrid about the Göteborg office", "git rebase origin main")
//!   never reads as Swedish; and the trigrams agree.
//!
//! Measured (2026-10-31, 262 + 159 fictional sentences of 2–25 words in the
//! 25 European languages, Chinese, Cantonese, Japanese and Korean, and 60
//! English ones, 45 of them full of foreign names, brands, borrowed words and
//! code): the language right 98% / 93% (held out) of the time, against
//! whatlang alone's 85%; past the gate, a nudge for 86% / 70% of 5–9-word
//! dictations and 99% / 88% of longer ones, never the wrong language, never
//! for an English one. lingua was as accurate and +145 MB. The log says the
//! code and counts, never the text.

use super::catalog;

/// The fewest words to tell a language by (CJK: characters / 2).
pub const MIN_WORDS: usize = 5;
/// The fewest distinct function words of the language.
pub const MIN_HITS: usize = 2;
/// Enough of a long dictation to tell; never all of it.
const MAX_CHARS: usize = 1_000;

/// What [`identify`] made of a text.
#[derive(Debug, Clone, PartialEq)]
pub struct Guess {
    /// The language (a catalog code).
    pub code: &'static str,
    /// Its words (CJK: characters / 2).
    pub words: usize,
    /// Its distinct function words in the text.
    pub hits: usize,
    /// Every other candidate's.
    pub others: Vec<(&'static str, usize)>,
    /// whatlang's trigrams picked it too.
    pub agrees: bool,
    /// Its script alone says it (Greek, Korean, Japanese, Chinese).
    pub by_script: bool,
}

/// Function words — and the commonest words of a dictation — per language,
/// lower case. Evidence only, never a dictionary: a word in several lists
/// counts for each.
pub const WORDS: &[(&str, &[&str])] = &[
    ("en", &["the", "and", "to", "of", "a", "i", "in", "is", "it", "you", "that", "for", "on", "with", "as", "was", "are", "be", "this", "have", "at", "or", "from", "but", "not", "by", "we", "my", "me", "your", "they", "what", "can", "will", "would", "just", "so", "if", "about", "there", "then", "them", "please", "thanks", "thank", "today", "tomorrow", "yes", "okay", "it's", "i'm", "don't", "can't", "let's", "i'll", "we're", "that's", "up", "out", "some", "all", "do", "our", "an", "has", "had", "been", "how", "when", "he", "she", "his", "her", "who", "which", "now", "also", "very", "could", "should", "need", "want", "get", "see", "know", "think", "here", "isn't", "say", "says", "into", "next", "again"]),
    ("fr", &["le", "la", "les", "l'", "de", "des", "du", "d'", "un", "une", "et", "est", "je", "j'", "tu", "il", "elle", "nous", "vous", "ils", "on", "ne", "n'", "pas", "que", "qu'", "qui", "pour", "dans", "sur", "avec", "mais", "ou", "où", "ce", "c'", "cette", "ces", "c'est", "j'ai", "suis", "es", "sont", "a", "à", "ai", "au", "aux", "en", "y", "se", "s'", "me", "m'", "te", "mon", "ma", "mes", "ton", "ta", "son", "sa", "plus", "très", "aussi", "bien", "oui", "non", "merci", "demain", "aujourd'hui", "maintenant", "s'il", "plaît", "être", "avoir", "fait", "faire", "peux", "peut", "veux", "après", "avant", "chez", "alors", "donc", "comme", "quand", "rien", "tout", "tous", "faut", "moi", "toi", "lui", "encore", "beaucoup", "mieux"]),
    ("de", &["der", "die", "das", "den", "dem", "des", "ein", "eine", "einen", "einem", "und", "ist", "ich", "du", "er", "sie", "es", "wir", "ihr", "nicht", "mit", "für", "auf", "zu", "im", "von", "bei", "nach", "aus", "über", "auch", "noch", "schon", "aber", "oder", "wenn", "dass", "wie", "wo", "mich", "mir", "dich", "dir", "uns", "sind", "bin", "bist", "hat", "habe", "haben", "wird", "werden", "kann", "kannst", "muss", "müssen", "soll", "bitte", "danke", "ja", "nein", "heute", "morgen", "jetzt", "hier", "sehr", "mal", "gibt", "viel", "dann", "kein", "keine", "sich", "bis", "gestern", "gut", "daran", "danach", "als", "wieder", "um", "vor"]),
    ("es", &["el", "la", "los", "las", "un", "una", "unos", "y", "es", "de", "del", "que", "en", "por", "para", "con", "no", "sí", "si", "se", "me", "te", "lo", "le", "les", "mi", "tu", "su", "nos", "al", "pero", "como", "más", "muy", "ya", "también", "hay", "está", "están", "estoy", "soy", "son", "fue", "era", "tengo", "tiene", "tenemos", "puedo", "puedes", "quiero", "hoy", "mañana", "ahora", "aquí", "gracias", "favor", "hola", "porque", "cuando", "donde", "dónde", "qué", "cómo", "este", "esta", "esto", "ese", "esa", "todo", "nada", "algo", "hasta", "luego", "antes", "después", "a", "así", "mucho", "mejor"]),
    ("it", &["il", "lo", "la", "i", "gli", "le", "un", "uno", "una", "un'", "e", "è", "di", "del", "della", "che", "in", "a", "da", "per", "con", "su", "non", "sì", "si", "mi", "ti", "ci", "vi", "ma", "come", "più", "molto", "anche", "sono", "sei", "siamo", "ho", "hai", "ha", "abbiamo", "hanno", "questo", "questa", "quello", "quella", "oggi", "domani", "adesso", "ora", "qui", "grazie", "favore", "perché", "quando", "dove", "cosa", "tutto", "niente", "poi", "prima", "dopo", "alle", "al", "nel", "nella", "dei", "delle", "dobbiamo", "puoi", "posso", "voglio", "l'", "c'è", "meglio", "stamattina"]),
    ("pt", &["o", "a", "os", "as", "um", "uma", "e", "é", "de", "do", "da", "dos", "das", "que", "em", "no", "na", "nos", "nas", "por", "para", "com", "não", "sim", "se", "me", "te", "lhe", "eu", "tu", "você", "ele", "ela", "nós", "mas", "como", "mais", "muito", "também", "já", "estou", "está", "são", "sou", "foi", "era", "tenho", "tem", "temos", "posso", "podes", "pode", "quero", "hoje", "amanhã", "agora", "aqui", "obrigado", "obrigada", "favor", "olá", "porque", "quando", "onde", "isso", "isto", "este", "esta", "tudo", "nada", "até", "depois", "antes", "ao", "à", "às", "vou", "vai", "acho", "melhor", "manhã", "tarde"]),
    ("nl", &["de", "het", "een", "en", "is", "van", "in", "op", "te", "dat", "die", "met", "voor", "niet", "aan", "er", "maar", "om", "ook", "als", "bij", "nog", "wel", "geen", "ik", "je", "jij", "u", "hij", "zij", "ze", "we", "wij", "mij", "me", "ons", "zijn", "ben", "bent", "heb", "hebben", "heeft", "kan", "kun", "kunnen", "moet", "moeten", "wil", "zal", "gaat", "dan", "nu", "vandaag", "morgen", "hier", "daar", "alsjeblieft", "alstublieft", "bedankt", "dank", "tot", "naar", "uit", "wat", "waar", "hoe", "wie", "dit", "deze", "veel", "heel", "drie", "uur", "eraan", "daarna", "weer", "dus"]),
    ("pl", &["i", "w", "z", "na", "do", "nie", "się", "że", "to", "jest", "są", "jestem", "jak", "ale", "o", "od", "po", "za", "dla", "przez", "tak", "ja", "ty", "on", "ona", "my", "wy", "oni", "mi", "mnie", "cię", "go", "jej", "ich", "mój", "moja", "ten", "ta", "te", "tym", "czy", "co", "kto", "gdzie", "kiedy", "bo", "już", "jeszcze", "bardzo", "też", "także", "dzisiaj", "dziś", "jutro", "teraz", "tu", "tutaj", "dzięki", "dziękuję", "proszę", "możesz", "mogę", "muszę", "musimy", "będzie", "był", "była", "było", "więc", "potem", "przed", "żebym", "niż", "dużo", "znowu"]),
    ("cs", &["a", "i", "v", "ve", "z", "ze", "na", "do", "se", "si", "že", "to", "je", "jsou", "jsem", "jsi", "jsme", "není", "jak", "ale", "o", "od", "po", "za", "pro", "přes", "tak", "já", "ty", "on", "ona", "my", "vy", "oni", "mi", "mě", "mně", "tě", "ho", "jí", "můj", "moje", "ten", "ta", "tu", "tom", "co", "kdo", "kde", "kdy", "protože", "už", "ještě", "velmi", "moc", "taky", "také", "dnes", "dneska", "zítra", "teď", "tady", "díky", "děkuji", "prosím", "můžeš", "můžu", "musím", "musíme", "bude", "byl", "byla", "bylo", "takže", "potom", "pak", "před", "než", "mnohem", "abych", "zase"]),
    ("sk", &["a", "i", "v", "vo", "z", "zo", "na", "do", "sa", "si", "že", "to", "je", "sú", "som", "sme", "nie", "ako", "ale", "o", "od", "po", "za", "pre", "cez", "tak", "ja", "ty", "on", "ona", "my", "vy", "oni", "mi", "ma", "mňa", "ťa", "ho", "jej", "ich", "môj", "moja", "ten", "tá", "tú", "tom", "čo", "kto", "kde", "kedy", "pretože", "už", "ešte", "veľmi", "aj", "tiež", "dnes", "zajtra", "teraz", "tu", "vďaka", "ďakujem", "prosím", "môžeš", "môžem", "musím", "musíme", "bude", "bol", "bola", "bolo", "takže", "potom", "pred", "oveľa", "aby", "zase"]),
    ("sl", &["in", "v", "z", "s", "na", "do", "se", "si", "da", "je", "so", "sem", "smo", "ni", "kot", "ampak", "ali", "o", "od", "po", "za", "pri", "tako", "jaz", "ti", "on", "ona", "mi", "vi", "oni", "me", "mene", "te", "ga", "jo", "jih", "moj", "moja", "ta", "to", "tem", "kaj", "kdo", "kje", "kdaj", "ker", "že", "še", "zelo", "tudi", "danes", "jutri", "zdaj", "tukaj", "tu", "hvala", "prosim", "lahko", "moram", "moramo", "bo", "bom", "bomo", "bil", "bila", "bilo", "zato", "potem", "pred", "nato", "spet", "veliko"]),
    ("hr", &["i", "u", "s", "sa", "na", "do", "se", "si", "da", "je", "su", "sam", "smo", "ste", "nije", "kao", "ali", "o", "od", "po", "za", "pri", "tako", "ja", "ti", "on", "ona", "mi", "vi", "oni", "me", "mene", "te", "ga", "ih", "moj", "moja", "taj", "ta", "to", "tom", "što", "tko", "gdje", "kada", "kad", "jer", "već", "još", "vrlo", "jako", "puno", "također", "danas", "sutra", "sada", "sad", "ovdje", "tu", "hvala", "molim", "možeš", "mogu", "moram", "moramo", "će", "ću", "ćemo", "bio", "bila", "bilo", "pa", "onda", "prije", "li", "opet", "jutros"]),
    ("da", &["og", "i", "jeg", "du", "han", "hun", "vi", "de", "det", "den", "der", "er", "var", "ikke", "en", "et", "til", "af", "på", "at", "med", "som", "har", "for", "kan", "skal", "vil", "mig", "dig", "os", "så", "men", "også", "nu", "hvad", "hvor", "hvordan", "hvis", "efter", "inden", "før", "fra", "om", "meget", "dag", "morgen", "tak", "venligst", "godt", "ja", "nej", "lige", "bare", "klokken", "igen", "måtte", "bagefter"]),
    ("sv", &["och", "i", "jag", "du", "han", "hon", "vi", "de", "det", "den", "där", "är", "var", "inte", "en", "ett", "till", "av", "på", "att", "med", "som", "har", "för", "kan", "ska", "vill", "mig", "dig", "oss", "så", "men", "också", "nu", "vad", "hur", "om", "efter", "innan", "före", "från", "mycket", "idag", "imorgon", "dag", "tack", "gärna", "bra", "ja", "nej", "bara", "klockan", "måste", "blir", "igen", "efteråt", "än"]),
    ("no", &["og", "i", "jeg", "du", "han", "hun", "vi", "de", "det", "den", "der", "er", "var", "ikke", "en", "et", "til", "av", "på", "at", "med", "som", "har", "for", "kan", "skal", "vil", "meg", "deg", "oss", "så", "men", "også", "nå", "hva", "hvor", "hvordan", "hvis", "etter", "før", "fra", "om", "veldig", "dag", "morgen", "takk", "gjerne", "bra", "ja", "nei", "bare", "klokka", "igjen", "etterpå"]),
    ("fi", &["ja", "on", "ei", "että", "se", "ne", "minä", "sinä", "hän", "me", "te", "he", "mutta", "kun", "jos", "niin", "myös", "vain", "nyt", "tänään", "huomenna", "kiitos", "olen", "olet", "oli", "ovat", "voi", "voitko", "voin", "pitää", "täytyy", "minulle", "sinulle", "meidän", "minun", "sinun", "kanssa", "mitä", "missä", "miten", "kuka", "tämä", "tuo", "paljon", "hyvin", "sitten", "ennen", "jälkeen", "kello", "taas", "vielä", "joten", "kuin", "ole", "en", "et", "minua"]),
    ("et", &["ja", "on", "ei", "et", "see", "need", "mina", "ma", "sina", "sa", "tema", "ta", "meie", "me", "teie", "te", "nad", "aga", "kui", "kas", "nii", "ka", "ainult", "nüüd", "täna", "homme", "aitäh", "tänan", "olen", "oled", "oli", "saad", "saan", "pean", "peame", "mulle", "sulle", "minu", "sinu", "koos", "mis", "kus", "kuidas", "kes", "palju", "hästi", "siis", "enne", "pärast", "kell", "jälle", "veel", "palun", "mul", "hiljem"]),
    ("lv", &["un", "ir", "ar", "uz", "no", "par", "ka", "kas", "es", "tu", "viņš", "viņa", "mēs", "jūs", "viņi", "man", "tev", "mums", "bet", "ja", "kad", "arī", "tikai", "tagad", "šodien", "rīt", "paldies", "lūdzu", "esmu", "esi", "bija", "būs", "var", "vari", "varu", "jā", "nē", "kā", "kur", "kāpēc", "šis", "tas", "ļoti", "daudz", "tad", "pirms", "pēc", "atkal", "vēl", "nekā", "vai", "mūsu", "savu", "līdz", "tāpēc", "tam"]),
    ("lt", &["ir", "yra", "su", "į", "iš", "ant", "apie", "kad", "kas", "aš", "tu", "jis", "ji", "mes", "jūs", "jie", "man", "tau", "mums", "bet", "jei", "kai", "taip", "ne", "tik", "dabar", "šiandien", "rytoj", "ačiū", "prašau", "esu", "buvo", "bus", "gali", "galiu", "turiu", "turime", "kaip", "kur", "kodėl", "šis", "tas", "labai", "daug", "tada", "prieš", "po", "vėl", "dar", "nei", "ar", "mano", "savo", "iki", "todėl", "vėliau"]),
    ("hu", &["a", "az", "egy", "és", "is", "hogy", "nem", "de", "van", "volt", "lesz", "meg", "már", "még", "csak", "ez", "ezt", "azt", "én", "te", "ő", "mi", "ti", "ők", "nekem", "neked", "nekünk", "mint", "ha", "amikor", "akkor", "mert", "vagy", "igen", "ma", "holnap", "most", "itt", "ott", "köszönöm", "köszi", "kérlek", "kérem", "tudsz", "tudok", "kell", "nagyon", "sok", "után", "előtt", "megint", "ezért", "aztán", "utána", "mit", "hol", "hogyan", "ki", "el", "fel"]),
    ("ro", &["și", "în", "la", "de", "cu", "pe", "din", "pentru", "că", "ce", "nu", "da", "este", "e", "sunt", "am", "ai", "a", "au", "eu", "tu", "el", "ea", "noi", "voi", "ei", "mi", "îmi", "ți", "ne", "vă", "le", "dar", "sau", "dacă", "când", "cum", "unde", "cine", "acest", "această", "acum", "azi", "astăzi", "mâine", "mulțumesc", "mersi", "rog", "poți", "pot", "trebuie", "foarte", "mult", "mai", "după", "înainte", "nou", "așa", "să", "o", "un", "decât", "apoi"]),
    ("mt", &["il", "l", "tal", "fil", "għal", "li", "u", "ta", "jien", "int", "inti", "hu", "hi", "aħna", "intom", "huma", "mhux", "ma", "kif", "fejn", "meta", "għaliex", "illum", "għada", "grazzi", "jekk", "jogħġbok", "biex", "minn", "mill", "fuq", "ħafna", "wara", "qabel", "se", "ser", "għandi", "kien", "hemm", "dan", "din", "dawn", "kollox", "iva", "le", "issa", "hawn", "fit", "fir", "lid", "mal", "bil", "sa", "kelli", "għalhekk", "reġgħet", "filgħodu", "dalgħodu"]),
    ("bg", &["и", "в", "във", "на", "с", "със", "за", "от", "до", "по", "не", "да", "че", "се", "ще", "е", "са", "съм", "си", "сме", "аз", "ти", "той", "тя", "ние", "вие", "те", "ми", "ме", "ни", "му", "но", "или", "ако", "когато", "как", "къде", "кой", "какво", "това", "този", "тази", "днес", "утре", "сега", "тук", "благодаря", "моля", "мога", "можеш", "трябва", "много", "след", "преди", "пак", "също", "така", "вече", "още", "после"]),
    ("ru", &["и", "в", "во", "на", "с", "со", "за", "от", "до", "по", "не", "что", "чтобы", "я", "ты", "он", "она", "мы", "вы", "они", "мне", "меня", "тебя", "нам", "нас", "его", "её", "но", "или", "если", "когда", "как", "где", "кто", "это", "этот", "эта", "сегодня", "завтра", "сейчас", "здесь", "тут", "спасибо", "пожалуйста", "могу", "можешь", "нужно", "надо", "очень", "много", "после", "потом", "опять", "тоже", "уже", "ещё", "так", "был", "была", "было", "будет", "есть", "поэтому"]),
    ("uk", &["і", "й", "в", "у", "на", "з", "із", "за", "від", "до", "по", "не", "що", "щоб", "я", "ти", "він", "вона", "ми", "ви", "вони", "мені", "мене", "тебе", "нам", "нас", "його", "її", "але", "або", "якщо", "коли", "як", "де", "хто", "це", "цей", "ця", "сьогодні", "завтра", "зараз", "тут", "дякую", "будь", "ласка", "можу", "можеш", "треба", "потрібно", "дуже", "багато", "після", "потім", "знову", "теж", "також", "вже", "ще", "так", "був", "була", "було", "буде", "є", "тому"]),
    ("ca", &["el", "la", "els", "les", "un", "una", "i", "és", "de", "del", "que", "en", "per", "amb", "no", "sí", "es", "em", "et", "ens", "al", "però", "com", "més", "molt", "també", "hi", "ha", "estic", "està", "sóc", "són", "tinc", "té", "puc", "pots", "vull", "avui", "demà", "ara", "aquí", "gràcies", "plau", "perquè", "quan", "on", "què", "això", "aquest", "aquesta", "tot", "res", "fins", "després", "abans"]),
];

/// Characters only written Cantonese uses (one is enough: Mandarin never
/// writes them).
pub const CANTONESE: &[char] = &['嘅', '咗', '唔', '喺', '冇', '佢', '哋', '啲', '嘢', '嚟', '睇', '噉', '啱', '嗰', '乜', '咁', '俾', '畀', '嘞', '囉'];

/// Languages written in Cyrillic (Serbian in both alphabets).
const CYRILLIC: &[&str] = &["ru", "uk", "bg", "be", "mk", "sr", "kk", "mn", "tg", "tt", "ba"];
/// Languages written in neither Latin nor Cyrillic letters.
const OTHER_SCRIPT: &[&str] = &[
    "el", "zh", "yue", "ja", "ko", "ar", "fa", "ur", "ps", "sd", "yi", "he", "hi", "mr", "ne", "sa", "bn", "as", "pa", "gu",
    "ta", "te", "kn", "ml", "si", "th", "lo", "km", "my", "bo", "ka", "hy", "am",
];

/// The candidates for a dictation in model `id`'s text (pure): the
/// languages it writes and the person's (`mine`); for an English-only
/// model, Parakeet V3's 25 too. Empty: a model Yap doesn't read this way —
/// Whisper says what it heard, Canary and Cohere are told the language (a
/// nudge wouldn't change what they're told), GigaAM knows Russian only.
pub fn candidates(id: &str, mine: &[String]) -> Vec<&'static str> {
    let ml = catalog::of_model(id);
    let base: &[&str] = match (ml.tells, ml.covers) {
        (catalog::Tells::Itself, catalog::Covers::Only(list)) => list,
        (catalog::Tells::One, _) if catalog::english_only(id) => catalog::EUROPEAN_25,
        _ => return Vec::new(),
    };
    let mut out: Vec<&'static str> = Vec::new();
    for code in base.iter().copied().chain(mine.iter().map(String::as_str)).chain(["en"]) {
        if let Some(l) = catalog::find(code) {
            if !out.contains(&l.code) {
                out.push(l.code);
            }
        }
    }
    out
}

fn words_of(code: &str) -> &'static [&'static str] {
    WORDS.iter().find(|(c, _)| *c == code).map(|(_, w)| *w).unwrap_or(&[])
}

/// A catalog code's whatlang language, where it has one.
fn whatlang_of(code: &str) -> Option<whatlang::Lang> {
    use whatlang::Lang::*;
    Some(match code {
        "en" => Eng,
        "fr" => Fra,
        "de" => Deu,
        "es" => Spa,
        "pt" => Por,
        "it" => Ita,
        "nl" => Nld,
        "pl" => Pol,
        "cs" => Ces,
        "sk" => Slk,
        "sl" => Slv,
        "hr" => Hrv,
        "sr" => Srp,
        "da" => Dan,
        "sv" => Swe,
        "no" => Nob,
        "fi" => Fin,
        "et" => Est,
        "lv" => Lav,
        "lt" => Lit,
        "hu" => Hun,
        "ro" => Ron,
        "bg" => Bul,
        "ru" => Rus,
        "uk" => Ukr,
        "be" => Bel,
        "mk" => Mkd,
        "ca" => Cat,
        "tr" => Tur,
        "la" => Lat,
        "af" => Afr,
        "id" => Ind,
        "tl" => Tgl,
        "vi" => Vie,
        "az" => Aze,
        "uz" => Uzb,
        "tk" => Tuk,
        "jw" => Jav,
        "sn" => Sna,
        _ => return None,
    })
}

/// whatlang's language as a catalog code.
fn code_of(l: whatlang::Lang) -> Option<&'static str> {
    use whatlang::Lang::*;
    Some(match l {
        Eng => "en",
        Fra => "fr",
        Deu => "de",
        Spa => "es",
        Por => "pt",
        Ita => "it",
        Nld => "nl",
        Pol => "pl",
        Ces => "cs",
        Slk => "sk",
        Slv => "sl",
        Hrv => "hr",
        Srp => "sr",
        Dan => "da",
        Swe => "sv",
        Nob => "no",
        Fin => "fi",
        Est => "et",
        Lav => "lv",
        Lit => "lt",
        Hun => "hu",
        Ron => "ro",
        Bul => "bg",
        Rus => "ru",
        Ukr => "uk",
        Bel => "be",
        Mkd => "mk",
        Cat => "ca",
        Tur => "tr",
        Lat => "la",
        Afr => "af",
        Ind => "id",
        Tgl => "tl",
        Vie => "vi",
        Aze => "az",
        Uzb => "uz",
        Tuk => "tk",
        Jav => "jw",
        Sna => "sn",
        _ => return None,
    })
}

fn is_han(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF)
}
fn is_kana(c: char) -> bool {
    matches!(c as u32, 0x3040..=0x30FF)
}
fn is_hangul(c: char) -> bool {
    matches!(c as u32, 0xAC00..=0xD7AF | 0x1100..=0x11FF | 0x3130..=0x318F)
}
fn is_greek(c: char) -> bool {
    matches!(c as u32, 0x0370..=0x03FF | 0x1F00..=0x1FFF)
}
fn is_cyrillic(c: char) -> bool {
    matches!(c as u32, 0x0400..=0x04FF)
}
/// A letter of some other script (Arabic, Hebrew, the Indic scripts, Thai…).
fn is_other_script(c: char) -> bool {
    matches!(c as u32, 0x0530..=0x1CFF)
}

/// The text's words, lower case: split at spaces, punctuation and hyphens;
/// an apostrophe kept inside a word ("aujourd'hui", "it's"), and an
/// elision's parts as words too ("l'ancienne" → "l'", "ancienne").
fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text.split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '\u{2019}')) {
        let w = raw.replace('\u{2019}', "'").trim_matches('\'').to_lowercase();
        if w.is_empty() || !w.chars().any(char::is_alphabetic) {
            continue;
        }
        if let Some(i) = w.find('\'') {
            out.push(w[..=i].to_string());
            if i + 1 < w.len() {
                out.push(w[i + 1..].to_string());
            }
        }
        out.push(w);
    }
    out
}

/// Distinct function words of `code` among `toks`.
fn hits(toks: &[String], code: &str) -> usize {
    let list = words_of(code);
    let mut seen: Vec<&str> = Vec::new();
    for t in toks {
        if let Some(w) = list.iter().find(|w| **w == t.as_str()) {
            if !seen.contains(w) {
                seen.push(w);
            }
        }
    }
    seen.len()
}

/// The language `text` is written in, of `among` (catalog codes, as
/// [`candidates`] gives them). `None`: no letters, or none of `among` is
/// written that way. Whether to believe it is [`sure`]'s. Pure.
pub fn identify(text: &str, among: &[&'static str]) -> Option<Guess> {
    let text: String = text.chars().take(MAX_CHARS).collect();
    let (mut han, mut kana, mut hangul, mut greek, mut cyr, mut other, mut latin) = (0, 0, 0, 0, 0, 0, 0usize);
    for c in text.chars().filter(|c| c.is_alphabetic()) {
        if is_han(c) {
            han += 1;
        } else if is_kana(c) {
            kana += 1;
        } else if is_hangul(c) {
            hangul += 1;
        } else if is_greek(c) {
            greek += 1;
        } else if is_cyrillic(c) {
            cyr += 1;
        } else if is_other_script(c) {
            other += 1;
        } else {
            latin += 1;
        }
    }
    let total = han + kana + hangul + greek + cyr + other + latin;
    if total == 0 {
        return None;
    }
    let by_script = |code: &'static str, words: usize| {
        among.contains(&code).then_some(Guess { code, words, hits: 0, others: Vec::new(), agrees: true, by_script: true })
    };
    // A script one language writes.
    let cjk = han + kana + hangul;
    if cjk * 2 > total {
        let words = cjk / 2;
        if hangul > kana && hangul >= han {
            return by_script("ko", words);
        }
        if kana > 0 {
            return by_script("ja", words);
        }
        if text.chars().any(|c| CANTONESE.contains(&c)) {
            return by_script("yue", words).or_else(|| by_script("zh", words));
        }
        return by_script("zh", words).or_else(|| by_script("yue", words));
    }
    let words = text.split_whitespace().filter(|w| w.chars().any(char::is_alphabetic)).count();
    if greek * 2 > total {
        return by_script("el", words);
    }
    if other * 2 > total {
        return None;
    }
    // Latin or Cyrillic: the candidates written that way.
    let cyrillic = cyr * 2 > total;
    let written: Vec<&'static str> = among
        .iter()
        .copied()
        .filter(|c| if cyrillic { CYRILLIC.contains(c) } else { (!CYRILLIC.contains(c) || *c == "sr") && !OTHER_SCRIPT.contains(c) })
        .collect();
    if written.is_empty() {
        return None;
    }
    let toks = tokens(&text);
    let mut scored: Vec<(&'static str, usize)> = written.iter().map(|c| (*c, hits(&toks, c))).collect();
    // The most first; a tie keeps `among`'s order.
    scored.sort_by_key(|s| std::cmp::Reverse(s.1));
    let others_of = |code: &str| scored.iter().filter(|s| s.0 != code).copied().collect::<Vec<_>>();
    // Maltese: its own letters (whatlang has no Maltese).
    if written.contains(&"mt") && text.chars().any(|c| matches!(c, 'ħ' | 'Ħ' | 'ġ' | 'Ġ' | 'ċ' | 'Ċ')) {
        return Some(Guess { code: "mt", words, hits: hits(&toks, "mt"), others: others_of("mt"), agrees: true, by_script: false });
    }
    // whatlang's trigrams and letters, over the same candidates.
    let allow: Vec<whatlang::Lang> = written.iter().filter_map(|c| whatlang_of(c)).collect();
    let trigrams = if allow.is_empty() { None } else { whatlang::Detector::with_allowlist(allow).detect_lang(&text).and_then(code_of) };
    let (top, top_hits) = scored[0];
    let runner_up = scored.get(1).map(|s| s.1).unwrap_or(0);
    // The function words' language when they decide it, else the trigrams'.
    let code = if top_hits > runner_up { top } else { trigrams.filter(|t| written.contains(t)).unwrap_or(top) };
    Some(Guess { code, words, hits: hits(&toks, code), others: others_of(code), agrees: trigrams == Some(code), by_script: false })
}

/// Whether `g` is sure enough to say the person spoke it, over `theirs`
/// (the languages they already speak): `Ok`, or why not. Pure.
pub fn sure(g: &Guess, theirs: &[String]) -> Result<(), &'static str> {
    if g.words < MIN_WORDS {
        return Err("too short to tell");
    }
    if g.by_script {
        return Ok(());
    }
    let other = g.others.iter().map(|o| o.1).max().unwrap_or(0);
    let mine = g.others.iter().filter(|o| theirs.iter().any(|t| t == o.0)).map(|o| o.1).max().unwrap_or(0);
    if !g.agrees || g.hits < MIN_HITS || g.hits <= other || g.hits < 2 * mine + 1 {
        return Err("not sure enough");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(codes: &[&str]) -> Vec<String> {
        codes.iter().map(|c| c.to_string()).collect()
    }

    /// Parakeet V3's candidates for someone who speaks English.
    fn v3() -> Vec<&'static str> {
        candidates("parakeet-tdt-0.6b-v3", &v(&["en"]))
    }

    /// Fictional dictations of 5–25 words, as Parakeet V3 writes them.
    const SAID: &[(&str, &str)] = &[
        ("fr", "Je pense que la nouvelle version marche beaucoup mieux que l'ancienne."),
        ("fr", "Rappelle-moi d'appeler le dentiste demain matin, puis de réserver une table pour quatre personnes."),
        ("fr", "On se retrouve devant la gare à midi."),
        ("de", "Ich glaube, die neue Version funktioniert viel besser als die alte."),
        ("de", "Kannst du nachsehen, ob das Paket heute Morgen angekommen ist?"),
        ("es", "Por favor, mándame el informe antes del viernes por la tarde."),
        ("es", "Terminamos el proyecto antes de lo previsto, así que podemos irnos de vacaciones."),
        ("it", "Ricordami di chiamare il dentista domani mattina e poi di prenotare un tavolo."),
        ("pt", "Por favor, envia-me o relatório antes de sexta-feira à tarde."),
        ("nl", "Ik denk dat de nieuwe versie veel beter werkt dan de oude."),
        ("pl", "Myślę, że nowa wersja działa dużo lepiej niż stara."),
        ("cs", "Myslím, že nová verze funguje mnohem lépe než ta stará."),
        ("sk", "Myslím si, že nová verzia funguje oveľa lepšie ako tá stará."),
        ("hr", "Moramo kupiti mlijeko, kruh i jaja za sutra."),
        ("sl", "Kupiti moramo mleko, kruh in jajca za jutri."),
        ("da", "Toget var forsinket igen i morges, så jeg gik glip af første halvdel af mødet."),
        ("sv", "Jag tror att den nya versionen fungerar mycket bättre än den gamla."),
        ("fi", "Luulen, että uusi versio toimii paljon paremmin kuin vanha."),
        ("et", "Ma arvan, et uus versioon töötab palju paremini kui vana."),
        ("lv", "Es domāju, ka jaunā versija darbojas daudz labāk nekā vecā."),
        ("lt", "Primink man rytoj ryte paskambinti odontologui ir tada užsisakyti staliuką restorane."),
        ("hu", "Kérlek, küldd el nekem a jelentést péntek délután előtt."),
        ("ro", "Trebuie să cumpărăm lapte, pâine și ouă pentru mâine."),
        ("mt", "Irridu nixtru l-ħalib, il-ħobż u l-bajd għal għada."),
        ("bg", "Моля те, изпрати ми доклада преди петък следобед."),
        ("ru", "Я думаю, что новая версия работает гораздо лучше старой."),
        ("uk", "Я думаю, що нова версія працює набагато краще, ніж стара."),
        ("el", "Νομίζω ότι η νέα έκδοση δουλεύει πολύ καλύτερα από την παλιά."),
    ];

    /// English a nudge must never take for another language: names, places,
    /// brands, borrowed words, code (all fictional).
    const ENGLISH: &[&str] = &[
        "Jean-Luc and Amélie are coming to the café at noon.",
        "Let's grab a croissant and a pain au chocolat before the meeting.",
        "Book the Hôtel de la Plage for Thursday and Friday night.",
        "Ask Ingrid Johansson about the Göteborg office move.",
        "Send the invoice to Müller und Söhne by Friday.",
        "git rebase origin main then push with force with lease",
        "npm run build and check the dist folder",
        "Rename the variable user_id to account_id in the config file.",
        "Volkswagen, Peugeot and Škoda all raised their prices again.",
        "Déjà vu, it's exactly the same bug as last week.",
        "We had a lovely rendezvous at Café Lumière on Saturday.",
        "Zoltán, Bartłomiej and Siobhán joined the call late.",
        "Grüße from Berlin, the whole team says hello.",
        "My Spanish teacher always says hasta la vista.",
        "Mañana, mañana, that's what they always say.",
        "Order the Bœuf bourguignon and a crème brûlée for me.",
        "const x equals await fetch url dot json",
        "SELECT star FROM users WHERE id equals forty two",
        "Our Zürich and Genève offices close early on Friday.",
        "Merci beaucoup, see you at the Gare du Nord tomorrow.",
        "The menu says pâté de campagne and tarte aux pommes.",
        "Danke schön, mate, the Fjällräven bag is in the boot.",
        "Remind me to call the dentist tomorrow morning, and then book a table for four.",
    ];

    #[test]
    fn each_language_is_told_by_its_words_and_trigrams_together() {
        let among = v3();
        for (code, text) in SAID {
            let g = identify(text, &among).unwrap_or_else(|| panic!("{code}: nothing"));
            assert_eq!(g.code, *code, "{text}: {g:?}");
            assert_eq!(sure(&g, &v(&["en"])), Ok(()), "{text}: {g:?}");
        }
    }

    #[test]
    fn english_full_of_names_brands_and_code_never_reads_as_another_language() {
        let among = v3();
        for text in ENGLISH {
            let g = identify(text, &among).expect("letters");
            assert!(g.code == "en" || sure(&g, &v(&["en"])).is_err(), "{text}: {g:?}");
        }
    }

    #[test]
    fn a_script_one_language_writes_decides_and_cantonese_is_its_own() {
        let sv = candidates("sense-voice-int8", &v(&["en"]));
        let g = identify("金曜日の午後までにレポートを送ってください。", &sv).unwrap();
        assert_eq!((g.code, g.by_script), ("ja", true));
        assert_eq!(identify("금요일 오후 전에 보고서를 보내 주세요.", &sv).unwrap().code, "ko");
        assert_eq!(identify("请在周五下午之前把报告发给我。", &sv).unwrap().code, "zh");
        assert_eq!(identify("唔該你喺星期五下晝之前將份報告寄俾我。", &sv).unwrap().code, "yue");
        assert_eq!(sure(&identify("请在周五下午之前把报告发给我。", &sv).unwrap(), &v(&["en"])), Ok(()));
        // Too few characters to tell.
        assert_eq!(sure(&identify("谢谢，明天见。", &sv).unwrap(), &v(&["en"])), Err("too short to tell"));
        // Greek, by its letters; nothing of a script no candidate writes.
        assert_eq!(identify("Νομίζω ότι η νέα έκδοση δουλεύει πολύ καλύτερα.", &v3()).unwrap().code, "el");
        assert_eq!(identify("Νομίζω ότι η νέα έκδοση δουλεύει.", &sv), None);
        assert_eq!(identify("مرحبا كيف حالك اليوم يا صديقي", &v3()), None);
        assert_eq!(identify("1234 5678, 90!", &v3()), None);
    }

    #[test]
    fn the_gate_wants_length_words_of_its_own_and_agreement() {
        let among = v3();
        // Four words: too short, however French.
        assert_eq!(sure(&identify("Je suis en retard.", &among).unwrap(), &v(&["en"])), Err("too short to tell"));
        // A French phrase in an English sentence: English words outnumber it.
        let g = identify("Book the Hôtel de la Plage for Thursday and Friday night.", &among).unwrap();
        assert!(g.code == "en" || sure(&g, &v(&["en"])) == Err("not sure enough"), "{g:?}");
        // Over the person's own languages: German words against a German
        // speaker's English-and-German count for German.
        let g = identify("Ich glaube, die neue Version funktioniert viel besser als die alte.", &among).unwrap();
        assert_eq!(g.code, "de");
        // Not agreeing, or too few words of its own: not sure.
        let mut weak = g.clone();
        weak.agrees = false;
        assert_eq!(sure(&weak, &v(&["en"])), Err("not sure enough"));
        weak = Guess { hits: 1, agrees: true, ..g.clone() };
        assert_eq!(sure(&weak, &v(&["en"])), Err("not sure enough"));
        // As many of the person's language's words as its own: not sure.
        let tie = Guess { hits: 3, others: vec![("en", 3)], ..g.clone() };
        assert_eq!(sure(&tie, &v(&["en"])), Err("not sure enough"));
        let ahead = Guess { hits: 3, others: vec![("en", 1)], ..g };
        assert_eq!(sure(&ahead, &v(&["en"])), Ok(()));
    }

    #[test]
    fn the_candidates_are_the_models_languages_and_the_persons() {
        let c = candidates("parakeet-tdt-0.6b-v3", &v(&["en", "ca"]));
        assert!(c.contains(&"fr") && c.contains(&"uk") && c.contains(&"ca") && c.contains(&"en"));
        assert!(!c.contains(&"ja"));
        // SenseVoice: its five, and English.
        assert_eq!(candidates("sense-voice-int8", &[]), vec!["zh", "yue", "en", "ja", "ko"]);
        // English only: what Parakeet V3 would write instead.
        let c = candidates("parakeet-tdt-0.6b-v2", &v(&["en"]));
        assert!(c.contains(&"fr") && c.contains(&"de") && c.len() == 25);
        // Whisper says; Canary and Cohere are told; GigaAM knows one other.
        for id in ["large", "turbo", "canary-1b-v2", "cohere-int8", "gigaam-v3-e2e-ctc"] {
            assert!(candidates(id, &v(&["en"])).is_empty(), "{id}");
        }
    }

    #[test]
    fn words_split_at_hyphens_and_elisions_and_keep_inner_apostrophes() {
        assert_eq!(tokens("Rappelle-moi d'appeler"), vec!["rappelle", "moi", "d'", "appeler", "d'appeler"]);
        assert_eq!(tokens("It’s fine, aujourd'hui!"), vec!["it'", "s", "it's", "fine", "aujourd'", "hui", "aujourd'hui"]);
        assert!(tokens("42 — 7").is_empty());
        // Every list in lower case, each word once.
        for (code, list) in WORDS {
            assert!(catalog::find(code).is_some(), "{code}");
            for (i, w) in list.iter().enumerate() {
                assert_eq!(*w, w.to_lowercase(), "{code}: {w}");
                assert!(!list[..i].contains(w), "{code}: {w} twice");
            }
        }
    }

    #[test]
    fn a_long_dictation_is_read_from_its_start_only() {
        let long = "Je pense que la nouvelle version marche beaucoup mieux. ".repeat(500);
        let g = identify(&long, &v3()).unwrap();
        assert_eq!(g.code, "fr");
        assert!(g.words < 200, "{}", g.words);
    }
}

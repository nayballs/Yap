//! **Learn dictionary words from your corrections** (`config.learn_words`,
//! the Dictionary view's "Suggest words from your corrections"): Yap pastes
//! a dictation, you fix a word it got wrong right there in the field — "cube
//! and eighties" → "Kubernetes" — and Yap offers to add it to the
//! dictionary: "Add “Kubernetes” to your dictionary? Yap heard “cube and
//! eighties”." It never adds a word without asking.
//!
//! Three parts:
//!
//! 1. **The watch** ([`after_paste`], a thread of its own per paste): the
//!    field the dictation went into is held over UI Automation
//!    (`selection::WatchedField`: the focused element, in the target
//!    window's process, never a password box or a read-only field) and its
//!    text around the caret read every [`POLL`] for up to [`WATCH_FOR`].
//!    Right after the paste Yap finds the pasted text there and remembers
//!    the text either side of it ([`Anchors`]); later reads find the span
//!    between those again, so it follows the pasted text wherever the caret
//!    goes. The watch ends when the window or the field loses the focus,
//!    another dictation starts, time is up, or the span grows far past what
//!    was pasted (that's writing on, not correcting). A span that changed
//!    and then sat still for [`SETTLE`] is looked at, and whatever it was
//!    when the watch ended. Everything stays in memory; logs carry lengths
//!    and counts only. Never in a test run, never after a submit key.
//! 2. **The candidates** ([`suggestions`], pure, unit-tested): the pasted
//!    and the edited words are aligned (longest common subsequence, ignoring
//!    case and the punctuation at their edges); what's left between matched
//!    words are the changes. Kept: 1–3 words replaced by 1–3 words where the
//!    new text looks like a term worth learning (a capital inside the word,
//!    an acronym, digits, a word that isn't an ordinary word — the built-in
//!    list of frequent words, then Windows' spell checker for your
//!    languages — or a capital mid-sentence) and that sounds or spells like
//!    what Yap typed (`fuzzy::distance`: Handy's Levenshtein + Soundex score,
//!    or how alike they sound). Also a change of case alone that makes a
//!    term of a word that isn't ordinary ("github" → "GitHub"). Skipped:
//!    insertions and deletions, punctuation, a single ordinary word heard
//!    (an entry would rewrite that word every time you say it: "motion" →
//!    "Notion"), a phrase of only function words, typo-style fixes between
//!    ordinary words, terms already in the dictionary, and edits that
//!    rewrite most of the dictation. At most [`MAX_SUGGESTIONS`].
//! 3. **The prompt** ([`offer`]): a toast in the main window when it's on
//!    screen, else a card on the Yap bar (`bar::show_card`); neither → not
//!    shown, nothing counted, and never a Windows notification. Add → a
//!    correction (`from` = what Yap heard, `to` = the term, near-misses on as
//!    every new correction) saved to config.json, the pipeline's live config
//!    and the windows' copies (`yap-dictionary-learned`, which Settings
//!    adopts and passes on to the Dictionary view, so neither puts the old
//!    list back). Not now (or ✕) twice, or "Don't suggest this", and that
//!    pair is never offered again; at most one prompt per [`MIN_GAP_MS`].
//!    The answers live in `<data>/word_suggestions.json` (not config.json:
//!    Settings auto-saves its whole copy of the config).
//!
//! Behaviour after Wispr Flow's dictionary auto-learn (`references/
//! wispr-flow/rea-teardown/04-dictation-and-other.md`) and OpenWhispr's
//! `correctionLearner.js` (word alignment); Wispr has a cloud classifier
//! confirm each word, Yap decides on this PC and asks you instead.

use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::config::DictionaryEntry;
use crate::selection::FieldWindow;

/// One word (or short phrase) to offer: Yap typed `heard`, you made it `term`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub heard: String,
    pub term: String,
}

/// At most this many words per dictation (and per prompt).
pub const MAX_SUGGESTIONS: usize = 3;
/// A change of more words than this on either side isn't a mishearing.
const MAX_WORDS: usize = 3;
/// At most one prompt this often.
pub const MIN_GAP_MS: u64 = 3 * 60_000;
/// "Not now" this many times and a pair isn't offered again.
pub const NOT_NOW_LIMIT: u32 = 2;

/// How long a dictation's field is watched after the paste.
const WATCH_FOR: Duration = Duration::from_secs(45);
/// How often it's read.
const POLL: Duration = Duration::from_millis(700);
/// A changed span that has sat still this long is looked at.
const SETTLE: Duration = Duration::from_millis(2_500);
/// How much text either side of the caret each read takes.
const WINDOW_CHARS: usize = 2_000;
/// Dictations longer than this aren't watched.
const MAX_PASTE_CHARS: usize = 1_500;
/// How much of the text either side of the pasted span is remembered to
/// find it again.
const ANCHOR_CHARS: usize = 24;
/// …and the shorter fallback when that text was edited too.
const SHORT_ANCHOR_CHARS: usize = 8;

/// The main window's event for a suggestion toast.
const EVENT: &str = "yap-word-suggestion";
/// What the windows get after words were added.
const EVENT_LEARNED: &str = "yap-dictionary-learned";
/// The bar card's id (one suggestion at a time) and the "added" notice's.
const CARD: &str = "word-suggestion";
const CARD_ADDED: &str = "word-added";
/// How long the suggestion card stays up (then it's left unanswered).
const CARD_MS: u64 = 30_000;

// ---- the words ---------------------------------------------------------------------

/// A word of a text, without the punctuation at its edges.
#[derive(Debug, Clone)]
struct Word<'a> {
    core: &'a str,
    /// `core` lowercased: what the alignment compares.
    key: String,
    /// It starts a sentence or a line (a capital there says nothing).
    starts_sentence: bool,
}

/// Punctuation that opens a word / closes it, trimmed off its core.
const OPENERS: &[char] = &['"', '\'', '“', '‘', '«', '‹', '(', '[', '{', '<', '*', '_', '¿', '¡'];
const CLOSERS: &[char] = &[
    '"', '\'', '”', '’', '»', '›', ')', ']', '}', '>', '*', '_', ',', '.', ';', ':', '!', '?', '…',
];

/// A token ends a sentence (or introduces something, a colon).
fn ends_sentence(token: &str) -> bool {
    token
        .trim_end_matches(['"', '\'', '”', '’', '»', ')', ']', '}'])
        .ends_with(['.', '!', '?', '…', ':'])
}

/// The words of `text`; `first_starts_sentence`: whether the first one
/// starts a sentence (the text before it ends one, or there's none).
fn words(text: &str, first_starts_sentence: bool) -> Vec<Word<'_>> {
    let mut tokens: Vec<(&str, bool)> = Vec::new();
    let mut newline = false;
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        if c.is_whitespace() {
            if let Some(s) = start.take() {
                tokens.push((&text[s..i], newline));
                newline = false;
            }
            if c == '\n' {
                newline = true;
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        tokens.push((&text[s..], newline));
    }
    let mut out = Vec::new();
    let mut pending = first_starts_sentence;
    for (token, newline) in tokens {
        let here = pending || newline;
        let core = token.trim_start_matches(OPENERS).trim_end_matches(CLOSERS);
        if !core.chars().any(char::is_alphanumeric) {
            // A dash, a bullet: no word, but it may end a sentence.
            pending = here || ends_sentence(token);
            continue;
        }
        out.push(Word {
            core,
            key: core.to_lowercase().replace('’', "'"),
            starts_sentence: here,
        });
        pending = ends_sentence(token);
    }
    out
}

/// The matched word pairs of the longest common subsequence of `a` and `b`
/// (by key), in order.
fn align(a: &[Word], b: &[Word]) -> Vec<(usize, usize)> {
    let (n, m) = (a.len(), b.len());
    let mut lcs = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i].key == b[j].key {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    while i < n && j < m {
        if a[i].key == b[j].key {
            out.push((i, j));
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    out
}

/// Letters and digits, lowercased.
fn alnum(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

#[derive(Debug, PartialEq, Eq)]
enum Case {
    /// "kubernetes"
    Lower,
    /// "Kubernetes"
    Sentence,
    /// "GitHub", "NASA", "42"…
    Other,
}

fn case_of(word: &str) -> Case {
    let mut letters = word.chars().filter(|c| c.is_alphabetic());
    let Some(first) = letters.next() else {
        return Case::Other;
    };
    if !letters.all(char::is_lowercase) {
        return Case::Other;
    }
    if first.is_lowercase() {
        Case::Lower
    } else if first.is_uppercase() {
        Case::Sentence
    } else {
        Case::Other
    }
}

/// A word whose very shape says "a name or a term": letters with digits
/// ("GPT-4", "S3"), an acronym ("NASA", "AWS"), or a capital inside it
/// ("GitHub", "iPhone", "McKinsey").
fn shaped_like_a_term(word: &str) -> bool {
    let letters: Vec<char> = word.chars().filter(|c| c.is_alphabetic()).collect();
    if letters.is_empty() {
        return false;
    }
    if word.chars().any(|c| c.is_numeric()) {
        return true;
    }
    if letters.len() >= 2 && letters.iter().all(|c| c.is_uppercase()) {
        return true;
    }
    letters.iter().skip(1).any(|c| c.is_uppercase()) && letters.iter().any(|c| c.is_lowercase())
}

/// Already a correction's spelling (or its trigger): nothing to learn.
fn in_dictionary(dict: &[DictionaryEntry], heard: &str, term: &str) -> bool {
    let (heard, term) = (heard.to_lowercase(), term.to_lowercase());
    dict.iter()
        .filter(|e| !e.snippet)
        .any(|e| e.to.trim().to_lowercase() == term || e.from.trim().to_lowercase() == heard)
}

/// Does `term` spell or sound like `heard` (`fuzzy::spelling_distance`,
/// `fuzzy::sound_distance`)? Half of it may differ; a third for keys of
/// three letters or sounds or fewer, where one is already a third.
fn sounds_alike(heard: &str, term: &str) -> bool {
    let limit = |len: usize| if len <= 3 { 0.3 } else { 0.5 };
    let (spelled, spelled_len) = crate::fuzzy::spelling_distance(heard, term);
    let (sounded, sounded_len) = crate::fuzzy::sound_distance(heard, term);
    spelled <= limit(spelled_len) || sounded <= limit(sounded_len)
}

/// How many words a run of words counts as: spelled-out letters ("G P T")
/// are one.
fn word_count(ws: &[Word]) -> usize {
    let single = |w: &Word| w.core.chars().filter(|c| c.is_alphanumeric()).count() == 1;
    ws.iter()
        .enumerate()
        .filter(|&(i, w)| !(single(w) && i > 0 && single(&ws[i - 1])))
        .count()
}

/// A replacement of `heard` words by `term` words, if it's worth learning.
/// `ordinary`: an everyday word (lowercased) — the built-in list, and on
/// Windows the spell checker.
fn judge(heard: &[Word], term: &[Word], dict: &[DictionaryEntry], ordinary: &dyn Fn(&str) -> bool) -> Option<Suggestion> {
    let join = |ws: &[Word]| ws.iter().map(|w| w.core).collect::<Vec<_>>().join(" ");
    let (heard_text, term_text) = (join(heard), join(term));
    if alnum(&term_text).chars().count() < 3 || alnum(&heard_text).chars().count() < 2 {
        return None;
    }
    // One real word heard: an entry would rewrite it every time it's said
    // ("motion" → "Notion", "tuesday" → "Thursday").
    if heard.len() == 1 && ordinary(&heard[0].key) {
        return None;
    }
    // "and then", "you know": too common to replace.
    if heard.iter().all(|w| is_stopword(&w.key)) {
        return None;
    }
    if in_dictionary(dict, &heard_text, &term_text) {
        return None;
    }
    let first = &term[0];
    let worth_learning = term.iter().any(|w| shaped_like_a_term(w.core))
        || term.iter().any(|w| !ordinary(&w.key))
        // A capital mid-sentence: a name ("I use Parakeet").
        || (case_of(first.core) == Case::Sentence && !first.starts_sentence && !is_frequent(&first.key))
        // Several words heard as one that isn't an everyday word, or the
        // other way round ("power to keep" → "Parakeet").
        || (heard.len() != term.len() && term.iter().all(|w| !is_frequent(&w.key)));
    if !worth_learning || !sounds_alike(&heard_text, &term_text) {
        return None;
    }
    Some(Suggestion {
        heard: heard_text,
        term: term_text,
    })
}

/// Only the case changed ("github" → "GitHub"): worth learning when the new
/// form is a term's ("GitHub", "NASA", or a capital mid-sentence) and the
/// word isn't an everyday one ("slack" → "Slack" would capitalise every
/// "slack").
fn judge_case(old: &Word, new: &Word, dict: &[DictionaryEntry], ordinary: &dyn Fn(&str) -> bool) -> Option<Suggestion> {
    if old.core == new.core || alnum(new.core).chars().count() < 3 || ordinary(&new.key) {
        return None;
    }
    if in_dictionary(dict, old.core, new.core) {
        return None;
    }
    let term_case = shaped_like_a_term(new.core)
        || (case_of(new.core) == Case::Sentence && !new.starts_sentence && case_of(old.core) == Case::Lower);
    term_case.then(|| Suggestion {
        heard: old.core.to_string(),
        term: new.core.to_string(),
    })
}

/// The words worth offering from an edit of a pasted dictation: `pasted`
/// as Yap typed it, `edited` as the field has it now. `starts_sentence`:
/// the text before them ends a sentence (or there's none). `ordinary`: is
/// this (lowercased) an everyday word? Pure.
pub fn suggestions(
    pasted: &str,
    edited: &str,
    starts_sentence: bool,
    dict: &[DictionaryEntry],
    ordinary: &dyn Fn(&str) -> bool,
) -> Vec<Suggestion> {
    let a = words(pasted, starts_sentence);
    let b = words(edited, starts_sentence);
    if a.is_empty() || b.is_empty() || a.len() > 400 || b.len() > 600 {
        return Vec::new();
    }
    let matched = align(&a, &b);
    let mut out: Vec<Suggestion> = Vec::new();
    // Words Yap typed that were changed or deleted, and not for a word
    // worth learning.
    let mut rewritten = 0;
    let mut prev = (0, 0);
    for &(i, j) in matched.iter().chain(std::iter::once(&(a.len(), b.len()))) {
        let (heard, term) = (&a[prev.0..i], &b[prev.1..j]);
        let replaced = (!heard.is_empty()
            && !term.is_empty()
            && word_count(heard) <= MAX_WORDS
            && word_count(term) <= MAX_WORDS)
            .then(|| judge(heard, term, dict, ordinary))
            .flatten();
        match replaced {
            Some(s) => out.push(s),
            None => rewritten += heard.len(),
        }
        if i < a.len() {
            out.extend(judge_case(&a[i], &b[j], dict, ordinary));
        }
        prev = (i + 1, j + 1);
    }
    // Most of it rewritten: an edit, not a correction.
    if rewritten > (a.len() / 2).max(MAX_WORDS) {
        return Vec::new();
    }
    let mut seen: HashSet<String> = HashSet::new();
    out.retain(|s| seen.insert(s.term.to_lowercase()));
    out.truncate(MAX_SUGGESTIONS);
    out
}

// ---- everyday words ----------------------------------------------------------------

/// Function words: a phrase made only of these is never replaced.
const STOPWORDS: &str = "a about above after again against all am an and any are as at be because been before \
being below between both but by can could did do does doing down during each few for from further had has have \
having he her here hers herself him himself his how i if in into is it its itself just let me more most my myself \
no nor not now of off on once only or other our ours ourselves out over own same she should so some such than that \
the their theirs them themselves then there these they this those through to too under until up very was we were \
what when where which while who whom why will with would you your yours yourself yourselves yeah yes ok okay oh um \
uh hmm like well also really actually maybe just going gonna wanna get got know mean see say said thing things";

/// Everyday English words (lowercased; inflections are found by
/// [`is_frequent`]). Not names, products or jargon — what's left out is
/// what a correction may teach. Days, months and numbers are in: "tuesday"
/// → "Thursday" is a change of mind, not a mishearing.
const FREQUENT: &str = "able accept access account across act action active activity add address admit adult affect \
afraid again age agency agent ago agree agreement ahead air allow almost alone along already alright although always \
amazing among amount analysis ancient anger angry animal announce annual another answer anybody anyone anything anyway \
anywhere apart apartment app appear apple apply approach approve area argue argument arm army around arrive art \
article artist ask asleep assume attack attempt attend attention attitude audience author available average avoid \
award aware away awesome baby back background bad bag balance ball bank bar base basic basically basis bath battle \
beach bear beat beautiful beauty become bed bedroom beer begin beginning behaviour behavior behind believe bell belong \
below beside best better beyond big bike bill bird birth birthday bit bite black blame blank blind block blood blow \
blue board boat body book border bored boring born borrow boss bottle bottom bowl box boy brain branch brave bread \
break breakfast breath bridge brief bright bring broad brother brown budget build building bunch burn bus business \
busy button buy cake call calm camera camp campaign cancel cancer candidate cap capital car card care career careful \
carry case cash cast cat catch cause cell center centre central century certain certainly chair chairman challenge \
chance change channel chapter character charge chart chat cheap check cheese chicken chief child childhood choice \
choose church circle citizen city claim class classic clean clear clearly client climb clock close closed clothes \
cloud club coach coast coat code coffee cold collect college colour color column come comfortable comment commercial \
common community company compare competition complain complete completely complex computer concern condition \
conference confirm conflict congress connect connection consider contact contain content context continue contract \
control conversation cook cool copy corner correct cost could council count country county couple course court cousin \
cover crazy create creative credit crime crisis critical cross crowd cry culture cup current customer cut cute dad \
daily damage dance danger dangerous dark data date daughter day dead deal dear death debate debt decade decide \
decision deep defence defense degree delay deliver delivery demand department depend describe design desk detail \
develop development device die diet difference different difficult dinner direct direction director dirty discover \
discuss discussion disease dish display distance divide doctor document dog dollar done door double doubt draft draw \
dream dress drink drive driver drop drug dry due dust duty early earn earth easily east easy eat economic economy \
edge edit editor education effect effort egg eight eighteen eighty either election electric element else email \
emergency employee empty end enemy energy engine enjoy enough ensure enter entire environment equal error escape \
especially essay establish estate etc evening event eventually ever every everybody everyone everything everywhere \
evidence exact exactly exam example excellent except exchange excited exciting excuse exercise exist expect expense \
expensive experience expert explain extra eye face fact factor fail fair fall false familiar family famous fan fancy \
far farm fashion fast fat father fault favourite favorite fear feature fee feed feel feeling fellow female few field \
fifteen fifth fifty fight figure file fill film final finally finance find fine finger finish fire firm first fish \
five fix flat flight floor flow flower fly focus folk follow food foot football force foreign forest forever forget \
forgive form former forty forward four fourteen fourth free freedom fresh friend friendly front fruit full fun fund \
funny future gain game garden gas gate gather general generation gentleman gift girl give glad glass global goal god \
gold golf good goodbye government grab grade grand grandfather grandmother grass great green grey gray ground group \
grow growth guard guess guest guide guitar guy hair half hall hand handle hang happen happy hard hardly hat hate head \
health healthy hear heart heat heavy hell hello help hey hi hide high highly hill hire history hit hold hole holiday \
home honest hope horse hospital host hot hotel hour house huge human hundred hungry hurry hurt husband ice idea \
identify ignore ill image imagine impact important improve include including income increase indeed independent \
indicate individual industry information inside instead institution interest interesting international internet \
interview introduce invite involve island issue item job join joke judge jump junior kid kill kind king kitchen knee \
knife knock land language large last late later laugh law lawyer lay lead leader learn least leave left leg legal \
less lesson letter level library lie life light likely limit line link list listen little live load loan local lock \
long look lose loss lot loud love lovely low luck lucky lunch machine mad magazine main maintain major make male man \
manage management manager many map mark market marriage married master match material matter meal measure meat \
media medical medicine meet meeting member memory mention menu mess message metal method middle might mile military \
milk million mind mine minister minute mirror miss mission mistake mix model modern mom moment money month mood moon \
morning mother motion mountain mouse mouth move movement movie much mum music must myself name nation national \
natural nature near nearly necessary neck need negative neighbour neighbor neither nervous network never new news \
newspaper next nice night nine nineteen ninety nobody noise none normal north nose note nothing notice novel number \
nurse object obvious obviously occur ocean odd offer office officer official often oil old one online open operation \
opinion opportunity option orange order ordinary organisation organization original others otherwise ought outside \
owner page pain paint pair paper parent park part particular partner party pass past path patient pattern pay peace \
pen people pepper per percent perfect perform performance perhaps period person personal phone photo physical piano \
pick picture piece pink place plan plane plant plastic plate play player please pleasure plenty plus pocket poem \
point police policy political politics pool poor popular population position positive possible post pot potato \
pound power practice prefer prepare present president press pressure pretty prevent price pride print priority \
prison private probably problem process produce product production professor profile program programme progress \
project promise proper property protect proud prove provide public pull purpose push put quality quarter queen \
question quick quickly quiet quite race radio rain raise range rate rather reach read ready real reality realize \
realise reason receive recent recently recognize recognise record red reduce reflect region relationship release \
remain remember remove rent repeat replace reply report represent request require research resource respond \
response rest restaurant result return reveal review rice rich ride right ring rise risk river road rock role roll \
roof room round rule run rush sad safe safety salary sale salt sample sand save scene schedule school science score \
screen sea search season seat second secret secretary section security seek seem sell send senior sense series \
serious serve service session set settle seven seventeen seventy several shake shall shape share sharp shift ship \
shirt shock shoe shoot shop shopping short shot shoulder shout show shower shut sick side sign signal silly similar \
simple simply since sing single sir sister sit site situation six sixteen sixty size skill skin sky sleep slide slow \
small smart smell smile smoke snow social society soft software soldier solution solve somebody someone something \
sometimes somewhere son song soon sorry sort sound soup source south space speak special speech speed spell spend \
spirit sport spot spring staff stage stand standard star start state statement station stay steal step stick still \
stock stomach stone stop store storm story straight strange street stress strike strong student study stuff stupid \
style subject success successful suddenly sugar suggest suit summer sun super supply support suppose sure surface \
surprise sweet swim system table take talk tall task taste tax tea teach teacher team tear technology telephone \
television tell temperature ten tend term terrible test text thank thanks theory therefore thick thin think third \
thirteen thirty though thought thousand threat three throw ticket tie tight till time tiny tip tired title today \
together toilet tomorrow tonight tool tooth top topic total touch tough tour toward towards town toy track trade \
traffic train training travel treat tree trial trip trouble truck true trust truth try turn twelve twenty twice two \
type ugly uncle understand unit university unless upon upset urban use used useful user usual usually value various \
version video view village visit voice vote wait wake walk wall want war warm warn wash watch water way weak wear \
weather website wedding week weekend weight weird welcome west wet whatever wheel whether white whole wide wife wild \
win wind window wine winter wish within without woman wonder wonderful wood word work worker world worry worse worst \
worth wow write writer wrong yard year yellow yesterday yet young yourself youth zero zone \
slack zoom notion outlook excel chrome shell signal drive keynote pages edge office teams word \
monday tuesday wednesday thursday friday saturday sunday january february march april may june july august september \
october november december \
ain't aren't can't couldn't didn't doesn't don't hadn't hasn't haven't he'd he'll he's i'd i'll i'm i've isn't it'd \
it'll it's let's she'd she'll she's shouldn't that's there's they'd they'll they're they've wasn't we'd we'll we're \
we've weren't what's where's who's won't wouldn't you'd you'll you're you've cannot into onto outta \
went gone saw seen made took taken came gave given knew known told found left felt kept held brought bought thought \
taught caught began begun ran won lost paid sent built spent stood understood wrote written spoke spoken broke broken \
chose chosen drove driven ate eaten fell fallen flew flown forgot forgotten froze grew grown hid hung led lit met rode \
rose sang sat shook shot shut slept sold sung swam threw thrown wore worn better best worse worst men women children \
people feet teeth mice";

fn set(list: &'static str) -> HashSet<&'static str> {
    list.split_whitespace().collect()
}

fn is_stopword(key: &str) -> bool {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| set(STOPWORDS)).contains(key.replace('’', "'").as_str())
}

/// The word or its plain stem is an everyday word (stopwords included).
/// `key`: lowercased.
pub(crate) fn is_frequent(key: &str) -> bool {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    let set = SET.get_or_init(|| {
        let mut s = set(FREQUENT);
        s.extend(set(STOPWORDS));
        s
    });
    let key = key.replace('’', "'");
    let key = key.as_str();
    if set.contains(key) {
        return true;
    }
    let w = key.strip_suffix("'s").unwrap_or(key);
    let mut stems: Vec<String> = vec![w.to_string()];
    let mut add = |s: &str| {
        if s.chars().count() >= 2 {
            stems.push(s.to_string());
        }
    };
    // "cities", "tried", "happier", "happily" → "…y"
    for suffix in ["ies", "ied", "ier", "iest", "ily", "iness"] {
        if let Some(s) = w.strip_suffix(suffix) {
            add(&format!("{s}y"));
        }
    }
    // Plain endings.
    for suffix in ["s", "es", "ers", "ly", "ness", "ment", "ful"] {
        if let Some(s) = w.strip_suffix(suffix) {
            add(s);
        }
    }
    // Endings that may have dropped an "e" ("used") or doubled the last
    // letter ("stopped", "running").
    for suffix in ["ed", "ing", "er", "est"] {
        if let Some(s) = w.strip_suffix(suffix) {
            add(s);
            add(&format!("{s}e"));
            let mut chars = s.chars().rev();
            if let (Some(x), Some(y)) = (chars.next(), chars.next()) {
                if x == y {
                    add(&s[..s.len() - x.len_utf8()]);
                }
            }
        }
    }
    stems.iter().any(|s| set.contains(s.as_str()))
}

/// "Is this an everyday word?" for the real thing: the built-in list, then
/// Windows' spell checker in your languages (`None` without one, or off
/// Windows: the list alone).
pub struct Ordinary {
    #[cfg(windows)]
    spell: Option<spell::Checker>,
}

impl Default for Ordinary {
    fn default() -> Self {
        Self::new()
    }
}

impl Ordinary {
    pub fn new() -> Ordinary {
        Ordinary {
            #[cfg(windows)]
            spell: spell::Checker::open(),
        }
    }

    /// `key`: lowercased. A word the spell checker only knows with a capital
    /// ("paris" → "Paris") is a name, not an everyday word.
    pub fn is(&self, key: &str) -> bool {
        if is_frequent(key) {
            return true;
        }
        #[cfg(windows)]
        if let Some(spell) = &self.spell {
            return spell.accepts(key);
        }
        false
    }
}

/// Windows' spell checker (`ISpellChecker`, Windows 8+): the languages of
/// the user's locale, English (US, UK) too, whichever are installed. Local,
/// read-only: nothing is added to the user's dictionary.
#[cfg(windows)]
mod spell {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{RPC_E_CHANGED_MODE, S_OK};
    use windows::Win32::Globalization::{GetUserDefaultLocaleName, ISpellChecker, ISpellCheckerFactory, SpellCheckerFactory};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};

    pub struct Checker {
        checkers: Vec<ISpellChecker>,
        /// COM was initialised by `open`: undone on drop, after the checkers.
        com: bool,
    }

    impl Checker {
        pub fn open() -> Option<Checker> {
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            if hr.is_err() && hr != RPC_E_CHANGED_MODE {
                return None;
            }
            let mut checker = Checker {
                checkers: Vec::new(),
                com: hr.is_ok(),
            };
            let factory: ISpellCheckerFactory =
                unsafe { CoCreateInstance(&SpellCheckerFactory, None, CLSCTX_INPROC_SERVER) }.ok()?;
            let mut tags: Vec<String> = Vec::new();
            let mut buf = [0u16; 85];
            let n = unsafe { GetUserDefaultLocaleName(&mut buf) };
            if n > 1 {
                tags.push(String::from_utf16_lossy(&buf[..(n - 1) as usize]));
            }
            for tag in ["en-US", "en-GB"] {
                if !tags.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
                    tags.push(tag.to_string());
                }
            }
            for tag in tags {
                let tag = HSTRING::from(tag.as_str());
                let supported = unsafe { factory.IsSupported(&tag) }.map(|b| b.as_bool()).unwrap_or(false);
                if supported {
                    if let Ok(c) = unsafe { factory.CreateSpellChecker(&tag) } {
                        checker.checkers.push(c);
                    }
                }
            }
            (!checker.checkers.is_empty()).then_some(checker)
        }

        /// Some language's dictionary has `word` exactly as written.
        pub fn accepts(&self, word: &str) -> bool {
            let word = HSTRING::from(word);
            self.checkers.iter().any(|c| unsafe {
                match c.Check(&word) {
                    // No first error: spelled right.
                    Ok(errors) => {
                        let mut first = None;
                        errors.Next(&mut first) != S_OK
                    }
                    Err(_) => false,
                }
            })
        }
    }

    impl Drop for Checker {
        fn drop(&mut self) {
            self.checkers.clear();
            if self.com {
                unsafe { CoUninitialize() };
            }
        }
    }
}

// ---- finding the pasted text again -------------------------------------------------

/// Line breaks as `\n`, no-break spaces as spaces (fields differ).
pub(crate) fn normalise(s: &str) -> String {
    s.replace("\r\n", "\n")
        .chars()
        .map(|c| match c {
            '\r' | '\u{b}' | '\u{2028}' | '\u{2029}' => '\n',
            '\u{a0}' | '\u{202f}' => ' ',
            c => c,
        })
        .collect()
}

fn tail(s: &str, chars: usize) -> &str {
    let n = s.chars().count();
    match s.char_indices().nth(n.saturating_sub(chars)) {
        Some((i, _)) if n > chars => &s[i..],
        _ => s,
    }
}

fn head(s: &str, chars: usize) -> &str {
    match s.char_indices().nth(chars) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// The text before a span ends a sentence (or a line), or there's none.
fn before_starts_sentence(before: &str) -> bool {
    let t = before.trim_end_matches([' ', '\t']);
    t.is_empty() || t.ends_with('\n') || ends_sentence(t)
}

/// Where the pasted text is in a field: the text either side of it, as it
/// was right after the paste.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Anchors {
    left: String,
    right: String,
    /// The paste began at the very start of the field / ended at its end.
    at_start: bool,
    at_end: bool,
    /// The pasted text's length, in characters.
    chars: usize,
    /// It begins a sentence.
    pub starts_sentence: bool,
}

impl Anchors {
    /// Find `pasted` (normalised, trimmed) in a window read right after the
    /// paste: the occurrence ending nearest the caret, where a paste leaves
    /// it. `None`: it isn't there as pasted (not landed yet, or the app
    /// changed it).
    pub fn find(w: &FieldWindow, pasted: &str) -> Option<Anchors> {
        if pasted.is_empty() {
            return None;
        }
        let (start, end) = w
            .text
            .match_indices(pasted)
            .map(|(i, m)| (i, i + m.len()))
            .min_by_key(|&(_, end)| end.abs_diff(w.caret))?;
        let before = &w.text[..start];
        Some(Anchors {
            left: tail(before, ANCHOR_CHARS).to_string(),
            right: head(&w.text[end..], ANCHOR_CHARS).to_string(),
            at_start: start == 0 && w.from_start,
            at_end: end == w.text.len() && w.to_end,
            chars: pasted.chars().count(),
            starts_sentence: before_starts_sentence(before),
        })
    }

    /// The pasted span as the field has it now (trimmed), between the same
    /// neighbours (if their text was edited too, their nearest few
    /// characters). The candidate nearest the pasted length wins. `None`:
    /// not in this window (the neighbours changed, or the caret is far off).
    pub fn locate(&self, w: &FieldWindow) -> Option<String> {
        self.locate_with(w, ANCHOR_CHARS)
            .or_else(|| self.locate_with(w, SHORT_ANCHOR_CHARS))
    }

    fn locate_with(&self, w: &FieldWindow, chars: usize) -> Option<String> {
        let left = tail(&self.left, chars);
        let right = head(&self.right, chars);
        let starts: Vec<usize> = if left.is_empty() {
            if self.at_start && w.from_start {
                vec![0]
            } else {
                Vec::new()
            }
        } else {
            w.text.match_indices(left).map(|(i, m)| i + m.len()).collect()
        };
        let mut best: Option<(usize, usize, usize)> = None; // (start, end, off by)
        for s in starts {
            let end = if right.is_empty() {
                (self.at_end && w.to_end).then_some(w.text.len())
            } else {
                w.text[s..].find(right).map(|i| s + i)
            };
            if let Some(e) = end {
                let off = w.text[s..e].chars().count().abs_diff(self.chars);
                if best.is_none_or(|(_, _, b)| off < b) {
                    best = Some((s, e, off));
                }
            }
        }
        best.map(|(s, e, _)| w.text[s..e].trim().to_string())
    }
}

/// The same words, however the spaces between them changed.
fn same_text(a: &str, b: &str) -> bool {
    a.split_whitespace().eq(b.split_whitespace())
}

/// The span has grown far past the paste: you're writing on, not fixing it.
fn grew_too_much(span: &str, pasted: &str) -> bool {
    span.chars().count() > pasted.chars().count() * 2 + 200
}

// ---- the watch ---------------------------------------------------------------------

/// Each paste's watch has a number; a newer one ends the older.
static WATCHES: AtomicU64 = AtomicU64::new(0);

/// A dictation's text `pasted` (as pasted, fitted to the cursor) just went
/// into `target`: watch that field for a while for corrections, on a thread
/// of its own (see the module docs). `enabled`: `learn_words`. Never in a
/// test run.
pub fn after_paste(app: &AppHandle, pasted: &str, target: Option<isize>, enabled: bool) {
    if !enabled || crate::e2e::active() {
        return;
    }
    let Some(hwnd) = target else {
        return;
    };
    let pasted = normalise(pasted).trim().to_string();
    if pasted.chars().count() > MAX_PASTE_CHARS || !pasted.chars().any(char::is_alphanumeric) {
        return;
    }
    let gen = WATCHES.fetch_add(1, Ordering::SeqCst) + 1;
    let dictations = crate::pipeline::dictation_state().0;
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("learn-watch".into())
        .spawn(move || watch(&app, &pasted, hwnd, gen, dictations));
    if let Err(e) = spawned {
        tracing::debug!("learn: couldn't start the watch ({e})");
    }
}

fn watch(app: &AppHandle, pasted: &str, hwnd: isize, gen: u64, dictations: u64) {
    let superseded = || WATCHES.load(Ordering::SeqCst) != gen || crate::pipeline::dictation_state().0 != dictations;
    // Let the paste land.
    std::thread::sleep(Duration::from_millis(350));
    let Some(field) = crate::selection::WatchedField::open(hwnd) else {
        tracing::debug!("learn: the field can't be read; not watching it");
        return;
    };
    let mut anchors = None;
    for _ in 0..4 {
        if superseded() {
            return;
        }
        if let Some(a) = field.read(WINDOW_CHARS).and_then(|w| Anchors::find(&w, pasted)) {
            anchors = Some(a);
            break;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    let Some(anchors) = anchors else {
        tracing::debug!(len = pasted.len(), "learn: the pasted text isn't in the field as pasted; not watching");
        return;
    };
    tracing::debug!(len = pasted.len(), "learn: watching the field for corrections");
    let started = Instant::now();
    // The latest span that differs from the paste, and since when.
    let mut changed: Option<(String, Instant)> = None;
    let mut looked_at: Option<String> = None;
    let why = loop {
        std::thread::sleep(POLL);
        if superseded() {
            break "another dictation";
        }
        if started.elapsed() >= WATCH_FOR {
            break "time";
        }
        if crate::text_injector::current_foreground() != Some(hwnd) || !field.is_focused() {
            break "the focus left";
        }
        let Some(span) = field.read(WINDOW_CHARS).and_then(|w| anchors.locate(&w)) else {
            continue;
        };
        if grew_too_much(&span, pasted) {
            changed = None;
            break "written on";
        }
        if same_text(&span, pasted) {
            changed = None;
            continue;
        }
        match &changed {
            Some((last, since)) if *last == span => {
                if since.elapsed() >= SETTLE && looked_at.as_ref() != Some(&span) {
                    looked_at = Some(span.clone());
                    if consider(app, pasted, &span, anchors.starts_sentence) {
                        return;
                    }
                }
            }
            _ => changed = Some((span, Instant::now())),
        }
    };
    tracing::debug!(why, "learn: stopped watching the field");
    if let Some((span, _)) = changed {
        if looked_at.as_ref() != Some(&span) {
            consider(app, pasted, &span, anchors.starts_sentence);
        }
    }
}

/// An edit seen: offer what's worth learning in it. `true` if a prompt went up.
fn consider(app: &AppHandle, pasted: &str, edited: &str, starts_sentence: bool) -> bool {
    let cfg = crate::config::load();
    if !cfg.learn_words {
        return false;
    }
    let ordinary = Ordinary::new();
    let found = suggestions(pasted, edited, starts_sentence, &cfg.dictionary, &|w| ordinary.is(w));
    tracing::debug!(found = found.len(), "learn: looked at an edit");
    !found.is_empty() && offer(app, found, false)
}

// ---- word_suggestions.json ---------------------------------------------------------

/// What `word_suggestions.json` remembers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Record {
    /// When a suggestion last showed (unix ms).
    pub last_offered_at: u64,
    /// "heard→term" (lowercased) → your answers to it.
    pub pairs: BTreeMap<String, PairRecord>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PairRecord {
    /// Times you said "Not now" (or closed it).
    pub not_now: u32,
    /// "Don't suggest this".
    pub never: bool,
}

fn pair_key(s: &Suggestion) -> String {
    format!("{}\u{2192}{}", s.heard.to_lowercase(), s.term.to_lowercase())
}

/// May `s` be offered (not turned down for good)?
pub fn allowed(rec: &Record, s: &Suggestion) -> bool {
    rec.pairs
        .get(&pair_key(s))
        .is_none_or(|p| !p.never && p.not_now < NOT_NOW_LIMIT)
}

/// Has it been long enough since the last prompt (or the clock went back)?
pub fn gap_ok(rec: &Record, now: u64) -> bool {
    rec.last_offered_at == 0 || now < rec.last_offered_at || now - rec.last_offered_at >= MIN_GAP_MS
}

/// `rec` after `answer` ("later" | "never") to `words`.
pub fn answered(mut rec: Record, words: &[Suggestion], answer: &str) -> Record {
    for s in words {
        let p = rec.pairs.entry(pair_key(s)).or_default();
        match answer {
            "later" => p.not_now = p.not_now.saturating_add(1),
            "never" => p.never = true,
            _ => {}
        }
    }
    rec
}

static RECORD: Mutex<Option<Record>> = Mutex::new(None);

fn record_path() -> std::path::PathBuf {
    crate::config::data_dir().join("word_suggestions.json")
}

fn loaded(slot: &mut Option<Record>) -> &mut Record {
    slot.get_or_insert_with(|| {
        let path = record_path();
        match std::fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| {
                crate::config::quarantine_corrupt(&path);
                Record::default()
            }),
            Err(_) => Record::default(),
        }
    })
}

fn record() -> Record {
    loaded(&mut RECORD.lock().unwrap_or_else(|p| p.into_inner())).clone()
}

fn update_record(f: impl FnOnce(Record) -> Record) {
    let mut guard = RECORD.lock().unwrap_or_else(|p| p.into_inner());
    let rec = f(loaded(&mut guard).clone());
    match serde_json::to_string_pretty(&rec) {
        Ok(json) => {
            let _ = std::fs::create_dir_all(crate::config::data_dir());
            if let Err(e) = crate::config::atomic_write(&record_path(), &json) {
                tracing::warn!("learn: couldn't save word_suggestions.json: {}", e);
            }
        }
        Err(e) => tracing::warn!("learn: couldn't serialize word_suggestions.json: {}", e),
    }
    *guard = Some(rec);
}

// ---- the prompt --------------------------------------------------------------------

/// The prompt's title, its body and its "never" link.
pub fn prompt_text(words: &[Suggestion]) -> (String, String, &'static str) {
    match words {
        [one] => (
            format!("Add “{}” to your dictionary?", one.term),
            format!("Yap heard “{}”.", one.heard),
            "Don't suggest this",
        ),
        many => {
            let items: Vec<String> = many
                .iter()
                .map(|s| format!("“{}” (Yap heard “{}”)", s.term, s.heard))
                .collect();
            let body = match items.split_last() {
                Some((last, rest)) if !rest.is_empty() => format!("{} and {}.", rest.join(", "), last),
                _ => format!("{}.", items.join("")),
            };
            (format!("Add {} words to your dictionary?", many.len()), body, "Don't suggest these")
        }
    }
}

/// What the main window gets (`yap-word-suggestion`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Prompt {
    id: u64,
    title: String,
    body: String,
    /// The "Don't suggest this" link's label.
    never: &'static str,
    words: Vec<Suggestion>,
}

/// The suggestion up now, waiting for an answer.
struct Pending {
    id: u64,
    words: Vec<Suggestion>,
    on_bar: bool,
}

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn pending() -> std::sync::MutexGuard<'static, Option<Pending>> {
    PENDING.lock().unwrap_or_else(|p| p.into_inner())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The main window is on screen (not hidden, not minimized). Waits on the
/// main thread: never call with a lock held.
fn main_window_visible(app: &AppHandle) -> bool {
    app.get_webview_window("settings")
        .is_some_and(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false))
}

/// Ask about `words`: a toast in the main window while it's on screen, else
/// a Yap bar card. Words turned down before are left out; at most one
/// prompt per [`MIN_GAP_MS`] (`ignore_gap`: the e2e hook). `true` if shown.
/// Blocking (window queries): off the main thread, no locks held.
fn offer(app: &AppHandle, words: Vec<Suggestion>, ignore_gap: bool) -> bool {
    let now = now_ms();
    let rec = record();
    if !ignore_gap && !gap_ok(&rec, now) {
        tracing::debug!("learn: too soon after the last suggestion; not asking");
        return false;
    }
    let words: Vec<Suggestion> = words
        .into_iter()
        .filter(|s| allowed(&rec, s))
        .take(MAX_SUGGESTIONS)
        .collect();
    if words.is_empty() {
        tracing::debug!("learn: already answered; not asking again");
        return false;
    }
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let (title, body, never) = prompt_text(&words);
    let in_window = main_window_visible(app);
    *pending() = Some(Pending {
        id,
        words: words.clone(),
        on_bar: !in_window,
    });
    let shown = if in_window {
        let prompt = Prompt {
            id,
            title,
            body,
            never,
            words: words.clone(),
        };
        app.emit(EVENT, prompt).is_ok()
    } else {
        use crate::bar::{Card, CardAction};
        let card = Card {
            id: CARD.into(),
            icon: "dictionary",
            title,
            body,
            primary: Some(CardAction::new("add", "Add")),
            secondary: Some(CardAction::new("later", "Not now")),
            link: Some(CardAction::new("never", never)),
            timeout_ms: Some(CARD_MS),
            expire_action: Some("expire".into()),
            close_action: Some("later".into()),
            ..Default::default()
        };
        crate::bar::show_card(
            app,
            card,
            Box::new(move |app, action| {
                if let Err(e) = respond(app, id, action) {
                    tracing::warn!("learn: {}", e);
                }
            }),
        )
    };
    if shown {
        tracing::info!(count = words.len(), in_window, "learn: offered words for the dictionary");
        update_record(|mut r| {
            r.last_offered_at = now;
            r
        });
    } else {
        let mut p = pending();
        if p.as_ref().is_some_and(|p| p.id == id) {
            *p = None;
        }
        tracing::debug!("learn: nowhere to show the suggestion");
    }
    shown
}

/// The words added, as their title: "“Kubernetes” is in your dictionary".
fn added_title(added: &[String]) -> String {
    match added {
        [one] => format!("“{one}” is in your dictionary"),
        many => format!("Added {} words to your dictionary", many.len()),
    }
}

/// Add `words` as corrections (heard → term; near-misses on, as for every
/// new correction): config.json, the pipeline's live copy, and the windows'
/// (`yap-dictionary-learned`). Returns the terms added.
fn add_words(app: &AppHandle, words: &[Suggestion]) -> Result<Vec<String>, String> {
    let mut cfg = crate::config::load();
    let mut added = Vec::new();
    for s in words {
        if in_dictionary(&cfg.dictionary, &s.heard, &s.term) {
            continue;
        }
        cfg.dictionary.push(DictionaryEntry {
            from: s.heard.clone(),
            to: s.term.clone(),
            fuzzy: true,
            snippet: false,
        });
        added.push(s.term.clone());
    }
    if added.is_empty() {
        return Ok(added);
    }
    crate::config::save(&cfg)?;
    if let Some(state) = app.try_state::<crate::AppState>() {
        if let Ok(guard) = state.pipeline.lock() {
            if let Some(p) = guard.as_ref() {
                p.update_config(cfg.clone());
            }
        }
    }
    let _ = app.emit(
        EVENT_LEARNED,
        serde_json::json!({ "entries": cfg.dictionary, "added": added }),
    );
    tracing::info!(count = added.len(), "learn: words added to the dictionary");
    Ok(added)
}

/// An answer to suggestion `id`: "add" (returns the terms added), "later"
/// (Not now, or ✕), "never" (Don't suggest this), "expire" / "" (left
/// alone: nothing counted).
fn respond(app: &AppHandle, id: u64, answer: &str) -> Result<Vec<String>, String> {
    let taken = {
        let mut p = pending();
        match p.as_ref() {
            Some(open) if open.id == id => p.take(),
            _ => None,
        }
    };
    let Some(open) = taken else {
        return match answer {
            "add" => Err("That suggestion is no longer open".into()),
            _ => Ok(Vec::new()),
        };
    };
    match answer {
        "add" => {
            let added = add_words(app, &open.words)?;
            if open.on_bar && !added.is_empty() {
                let card = crate::bar::Card {
                    id: CARD_ADDED.into(),
                    icon: "dictionary",
                    title: added_title(&added),
                    body: "Yap writes it that way from now on.".into(),
                    timeout_ms: Some(4_000),
                    ..Default::default()
                };
                crate::bar::show_card(app, card, Box::new(|_, _| {}));
            }
            Ok(added)
        }
        "later" | "never" => {
            update_record(|r| answered(r, &open.words, answer));
            tracing::info!(answer, "learn: suggestion turned down");
            Ok(Vec::new())
        }
        _ => Ok(Vec::new()),
    }
}

// ---- commands -----------------------------------------------------------------------

/// The main window's answer to a suggestion toast (`yap-word-suggestion`):
/// `answer` = "add" | "later" | "never" | "expire". Returns the terms added.
#[tauri::command]
pub async fn word_suggestion_respond(app: AppHandle, id: u64, answer: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || respond(&app, id, &answer))
        .await
        .map_err(|e| e.to_string())?
}

/// Test mode only: an edit of a pasted dictation, as the watch would have
/// seen it (`pasted` → `edited`; `before` = the field's text before it,
/// for whether it starts a sentence: none = it does). Runs it through the
/// same rules and puts the prompt up the same way (the gap between prompts
/// aside). `fresh`: forget every answer first. Returns what was offered.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn e2e_learn_correction(
    app: AppHandle,
    pasted: String,
    edited: String,
    before: Option<String>,
    fresh: Option<bool>,
) -> Result<Vec<Suggestion>, String> {
    if !crate::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    tauri::async_runtime::spawn_blocking(move || {
        if fresh.unwrap_or(false) {
            update_record(|_| Record::default());
            *pending() = None;
        }
        let cfg = crate::config::load();
        if !cfg.learn_words {
            tracing::info!("e2e: word suggestions are off");
            return Ok(Vec::new());
        }
        let starts = before.as_deref().is_none_or(before_starts_sentence);
        let ordinary = Ordinary::new();
        let found = suggestions(&normalise(&pasted), &normalise(&edited), starts, &cfg.dictionary, &|w| {
            ordinary.is(w)
        });
        let rec = record();
        let found: Vec<Suggestion> = found.into_iter().filter(|s| allowed(&rec, s)).collect();
        tracing::info!(found = found.len(), "e2e: a correction is staged");
        if found.is_empty() {
            return Ok(Vec::new());
        }
        Ok(if offer(&app, found.clone(), true) { found } else { Vec::new() })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(from: &str, to: &str) -> DictionaryEntry {
        DictionaryEntry {
            from: from.into(),
            to: to.into(),
            fuzzy: true,
            snippet: false,
        }
    }

    /// The rules with the built-in word list only (no spell checker).
    fn found(pasted: &str, edited: &str) -> Vec<(String, String)> {
        found_with(pasted, edited, &[])
    }

    fn found_with(pasted: &str, edited: &str, dict: &[DictionaryEntry]) -> Vec<(String, String)> {
        suggestions(pasted, edited, true, dict, &is_frequent)
            .into_iter()
            .map(|s| (s.heard, s.term))
            .collect()
    }

    fn pair(heard: &str, term: &str) -> Vec<(String, String)> {
        vec![(heard.to_string(), term.to_string())]
    }

    #[test]
    fn a_misheard_phrase_fixed_in_place_is_offered() {
        assert_eq!(
            found("We should move the app to cube and eighties next week.", "We should move the app to Kubernetes next week."),
            pair("cube and eighties", "Kubernetes")
        );
        // Yap's own example: a phrase heard as one word that is an everyday
        // word too ("parakeet") — several words become one, so it counts.
        assert_eq!(
            found("Power to keep is the default model.", "Parakeet is the default model."),
            pair("Power to keep", "Parakeet")
        );
        // One word for one.
        assert_eq!(found("Send the jason payload.", "Send the JSON payload."), pair("jason", "JSON"));
        assert_eq!(found("I spoke to Shunade today.", "I spoke to Sinead today."), pair("Shunade", "Sinead"));
        // One word heard as two.
        assert_eq!(found("Open the graph ana dashboard.", "Open the Grafana dashboard."), pair("graph ana", "Grafana"));
        assert_eq!(found("Ask chat G P T about it.", "Ask ChatGPT about it."), pair("chat G P T", "ChatGPT"));
    }

    #[test]
    fn punctuation_and_spacing_around_the_fix_dont_matter() {
        assert_eq!(
            found("Deploy it to cube and eighties, then test.", "Deploy it to Kubernetes, then test."),
            pair("cube and eighties", "Kubernetes")
        );
        assert_eq!(
            found("Deploy it to cube and eighties.", "Deploy it to   (Kubernetes)."),
            pair("cube and eighties", "Kubernetes")
        );
        // Only the punctuation changed: nothing to learn.
        assert!(found("We met, and then left.", "We met and then left!").is_empty());
        assert!(found("It's done", "It’s done.").is_empty());
    }

    #[test]
    fn several_fixes_in_one_dictation_at_most_three() {
        assert_eq!(
            found(
                "Move cube and eighties logs to graph ana.",
                "Move Kubernetes logs to Grafana."
            ),
            vec![
                ("cube and eighties".to_string(), "Kubernetes".to_string()),
                ("graph ana".to_string(), "Grafana".to_string())
            ]
        );
        let out = found(
            "jason and Shunade and graph ana and chat G P T and the rest of the long sentence here today",
            "JSON and Sinead and Grafana and ChatGPT and the rest of the long sentence here today",
        );
        assert_eq!(out.len(), MAX_SUGGESTIONS, "{out:?}");
    }

    #[test]
    fn a_case_change_that_makes_a_term_counts() {
        assert_eq!(found("I pushed it to github yesterday.", "I pushed it to GitHub yesterday."), pair("github", "GitHub"));
        assert_eq!(found("Ask nasa for the data.", "Ask NASA for the data."), pair("nasa", "NASA"));
        // A capital mid-sentence on a word that isn't an everyday one.
        assert_eq!(found("I talked to sinead today.", "I talked to Sinead today."), pair("sinead", "Sinead"));
        // …but not on an everyday word, nor at a sentence start.
        assert!(found("We used slack for it.", "We used Slack for it.").is_empty());
        assert!(found("Done. sinead said so.", "Done. Sinead said so.").is_empty());
        assert!(found("the plan is fine.", "The plan is fine.").is_empty());
        // Lowering a term isn't one.
        assert!(found("Push it to GitHub now.", "Push it to github now.").is_empty());
    }

    #[test]
    fn typo_fixes_between_everyday_words_are_not_offered() {
        // A real word heard: an entry would rewrite it every time.
        assert!(found("Let's meat at noon.", "Let's meet at noon.").is_empty());
        assert!(found("Put it over their.", "Put it over there.").is_empty());
        assert!(found("See you on tuesday then.", "See you on Thursday then.").is_empty());
        assert!(found("Open the motion page.", "Open the Notion page.").is_empty());
        // Two everyday words made one everyday word.
        assert!(found("I go there some times.", "I go there sometimes.").is_empty());
        assert!(found("Send me an e mail.", "Send me an email.").is_empty());
        // Function words only.
        assert!(found("And then we left.", "Anthen we left.").is_empty());
    }

    #[test]
    fn unrelated_replacements_are_not_mishearings() {
        // You changed your mind, not Yap's spelling.
        assert!(found("Book the meeting room for Sarah.", "Book the meeting room for Priya.").is_empty());
        assert!(found("We'll use cube and eighties.", "We'll use Terraform.").is_empty());
        assert!(found("Call jamal about it.", "Call Sam about it.").is_empty());
        // Short names one letter apart are too close to call.
        assert!(found("Ask Sam to come.", "Ask Pam to come.").is_empty());
    }

    #[test]
    fn insertions_deletions_and_rewrites_are_skipped() {
        assert!(found("We shipped it.", "We finally shipped it.").is_empty());
        assert!(found("We shipped it today.", "We shipped it.").is_empty());
        assert!(found("We shipped it.", "We shipped it. Kubernetes is next.").is_empty());
        // Most of it rewritten: not even the fix in it counts.
        assert!(found(
            "jason is the format and the rest of this sentence is fine",
            "JSON is what we will never use for anything at all"
        )
        .is_empty());
        // A few changes beside a fix are fine.
        assert_eq!(
            found(
                "the deploy went fine and the cube and eighties cluster is up",
                "the deploy went well and the Kubernetes cluster is up"
            ),
            pair("cube and eighties", "Kubernetes")
        );
        // More than three words on a side.
        assert!(found("Use the new cube and eighties thing.", "Use Kubernetes.").is_empty());
    }

    #[test]
    fn words_already_in_the_dictionary_are_skipped() {
        let dict = vec![entry("cuber netties", "Kubernetes"), entry("jaison", "JSON")];
        assert!(found_with("Deploy to cube and eighties.", "Deploy to Kubernetes.", &dict).is_empty());
        assert!(found_with("Send the jason payload.", "Send the json payload.", &dict).is_empty());
        // A snippet's text isn't a correction's spelling.
        let snippet = vec![DictionaryEntry { snippet: true, ..entry("k8s", "Kubernetes") }];
        assert_eq!(
            found_with("Deploy to cube and eighties.", "Deploy to Kubernetes.", &snippet),
            pair("cube and eighties", "Kubernetes")
        );
    }

    #[test]
    fn a_spell_checker_that_knows_more_words_keeps_the_phrases() {
        // Windows' spell checker knows "parakeet" (the bird), and may know a
        // word the person added to it: several words heard as one still count.
        let knows = |w: &str| is_frequent(w) || w == "parakeet" || w == "kubernetes";
        let found = |p: &str, e: &str| suggestions(p, e, true, &[], &knows);
        assert_eq!(found("Power to keep is good.", "Parakeet is good.").len(), 1);
        assert_eq!(found("Use cube and eighties.", "Use Kubernetes.").len(), 1);
        // …but one ordinary word for another doesn't.
        assert!(found("Use the parrot model.", "Use the parakeet model.").is_empty());
        assert_eq!(word_count(&words("chat G P T now", true)), 3);
    }

    /// Windows' own spell checker, read-only:
    /// `cargo test --lib -- --ignored windows_spell_checker`.
    #[cfg(windows)]
    #[test]
    #[ignore = "uses the machine's spell checker"]
    fn windows_spell_checker() {
        let ordinary = Ordinary::new();
        assert!(ordinary.spell.is_some(), "no spell checker for this locale or English");
        for w in ["meet", "deployment", "carburettor", "parakeet"] {
            assert!(ordinary.is(w), "{w}");
        }
        for w in ["kubernetes", "jason", "sinead", "kubectl", "grafana"] {
            assert!(!ordinary.is(w), "{w}");
        }
    }

    #[test]
    fn an_everyday_word_test_with_inflections() {
        for w in ["meet", "meeting", "meetings", "stopped", "running", "happily", "cities", "don't", "it’s", "people", "tuesday"] {
            assert!(is_frequent(w), "{w}");
        }
        for w in ["kubernetes", "grafana", "parakeet", "sinead", "nginx", "json", "kubectl", "jason"] {
            assert!(!is_frequent(w), "{w}");
        }
    }

    #[test]
    fn shapes_of_terms() {
        for w in ["GitHub", "iPhone", "NASA", "GPT-4", "S3", "McKinsey"] {
            assert!(shaped_like_a_term(w), "{w}");
        }
        for w in ["Kubernetes", "kubernetes", "The", "I", "A"] {
            assert!(!shaped_like_a_term(w), "{w}");
        }
    }

    fn window(text: &str, caret: usize) -> FieldWindow {
        FieldWindow {
            text: text.into(),
            caret,
            from_start: true,
            to_end: true,
        }
    }

    #[test]
    fn the_pasted_text_is_found_again_after_edits() {
        let pasted = "Deploy it to cube and eighties.";
        let before = "Notes for Monday. ";
        let after = "\nThen lunch.";
        let text = format!("{before}{pasted}{after}");
        let a = Anchors::find(&window(&text, before.len() + pasted.len()), pasted).expect("found");
        assert!(a.starts_sentence);
        // The fix, with the caret moved back into it.
        let edited = format!("{before}Deploy it to Kubernetes.{after}");
        assert_eq!(a.locate(&window(&edited, 20)).as_deref(), Some("Deploy it to Kubernetes."));
        // Typing on after it, and a word changed a little before it.
        let more = format!("Memo for Monday. Deploy it to Kubernetes. More{after}");
        assert_eq!(a.locate(&window(&more, 30)).as_deref(), Some("Deploy it to Kubernetes. More"));
        // Its neighbours gone entirely: lost.
        assert_eq!(a.locate(&window("Something else entirely", 3)), None);
    }

    #[test]
    fn a_paste_at_the_field_start_or_end_is_found_by_the_edges() {
        let pasted = "Graph ana is up.";
        let a = Anchors::find(&window(pasted, pasted.len()), pasted).expect("found");
        assert_eq!(a.locate(&window("Grafana is up.", 3)).as_deref(), Some("Grafana is up."));
        // A window that isn't the whole field can't vouch for the edges.
        let partial = FieldWindow {
            from_start: false,
            ..window("Grafana is up.", 3)
        };
        assert_eq!(a.locate(&partial), None);
        // The occurrence nearest the caret is the paste.
        let twice = "Yes. Yes.";
        let a = Anchors::find(&window(twice, twice.len()), "Yes.").expect("found");
        assert_eq!(a.left, "Yes. ");
        assert!(a.starts_sentence);
    }

    #[test]
    fn the_watch_tells_corrections_from_writing_on() {
        assert!(same_text("Deploy  it\nnow", "Deploy it now"));
        assert!(!same_text("Deploy it now", "Deploy it later"));
        let pasted = "A short one.";
        assert!(!grew_too_much("A short one, fixed.", pasted));
        assert!(grew_too_much(&"and then I kept typing ".repeat(12), pasted));
        assert_eq!(normalise("a\r\nb\rc\u{a0}d"), "a\nb\nc d");
    }

    #[test]
    fn answers_are_remembered_per_pair() {
        let s = Suggestion { heard: "cube and eighties".into(), term: "Kubernetes".into() };
        let other = Suggestion { heard: "graph ana".into(), term: "Grafana".into() };
        let rec = Record::default();
        assert!(allowed(&rec, &s));
        // Not now once: still offered; twice: no more.
        let rec = answered(rec, std::slice::from_ref(&s), "later");
        assert!(allowed(&rec, &s));
        let rec = answered(rec, std::slice::from_ref(&s), "later");
        assert!(!allowed(&rec, &s));
        assert!(allowed(&rec, &other));
        // Don't suggest this: never again, whatever the case.
        let rec = answered(rec, std::slice::from_ref(&other), "never");
        let shouted = Suggestion { heard: "GRAPH ANA".into(), term: "grafana".into() };
        assert!(!allowed(&rec, &shouted));
        // The file's shape, and an old file missing fields.
        let json = serde_json::to_string(&rec).unwrap();
        assert!(json.contains("\"notNow\":2") && json.contains("\"never\":true"), "{json}");
        assert_eq!(serde_json::from_str::<Record>("{}").unwrap(), Record::default());
    }

    #[test]
    fn one_prompt_every_few_minutes() {
        const T0: u64 = 1_800_000_000_000;
        let rec = Record { last_offered_at: T0, ..Record::default() };
        assert!(gap_ok(&Record::default(), T0));
        assert!(!gap_ok(&rec, T0 + MIN_GAP_MS - 1));
        assert!(gap_ok(&rec, T0 + MIN_GAP_MS));
        // A clock set back doesn't hold suggestions off for hours.
        assert!(gap_ok(&rec, T0 - 3_600_000));
    }

    #[test]
    fn the_prompt_says_what_yap_heard() {
        let one = vec![Suggestion { heard: "cube and eighties".into(), term: "Kubernetes".into() }];
        let (title, body, never) = prompt_text(&one);
        assert_eq!(title, "Add “Kubernetes” to your dictionary?");
        assert_eq!(body, "Yap heard “cube and eighties”.");
        assert_eq!(never, "Don't suggest this");
        let two = vec![one[0].clone(), Suggestion { heard: "graph ana".into(), term: "Grafana".into() }];
        let (title, body, never) = prompt_text(&two);
        assert_eq!(title, "Add 2 words to your dictionary?");
        assert_eq!(body, "“Kubernetes” (Yap heard “cube and eighties”) and “Grafana” (Yap heard “graph ana”).");
        assert_eq!(never, "Don't suggest these");
        assert_eq!(added_title(&["Kubernetes".into()]), "“Kubernetes” is in your dictionary");
        assert_eq!(added_title(&["A".into(), "B".into()]), "Added 2 words to your dictionary");
    }
}

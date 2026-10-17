//! **Learn dictionary words from your corrections** (`config.learn_words`,
//! the Dictionary view's "Suggest words from your corrections"): Yap pastes
//! a dictation, you fix a word it got wrong right there in the field — "cube
//! and eighties" → "Kubernetes" — and Yap offers to add it to the
//! dictionary: "Add “Kubernetes” to your dictionary? Yap heard “cube and
//! eighties”." It never adds a word without asking.
//!
//! Three parts:
//!
//! 1. **The watch** ([`after_paste`], a thread of its own per paste; its
//!    rules are [`Watch`], pure): the field the dictation went into is held
//!    over UI Automation (`selection::WatchedField`: the focused element, in
//!    the target window's process, never a password box or a read-only
//!    field) and read whole every [`POLL`] for [`WATCH_FOR`], focused or
//!    not — a fix often comes after a glance at another window. Right after
//!    the paste Yap finds the pasted text there and remembers the text
//!    either side of it ([`Anchors`]); every read finds the span between
//!    those again, and a span still within the drift gate ([`within_drift`]:
//!    not rewritten past a share of its length) becomes the last good edit.
//!    The watch ends ([`End`]) when time is up, another dictation starts,
//!    the field is emptied (a chat message sent), the dictation's text or
//!    its neighbours are deleted, a line break follows it (Enter), the field
//!    stops answering or passes [`MAX_FIELD_CHARS`] — and whatever the
//!    reason, the last good edit is what's looked at, so "fix a word, press
//!    Enter" still counts. Everything stays in memory; logs carry lengths,
//!    counts and the end's name only. Never in a test run; the pipeline
//!    doesn't ask after a submit key or for a terminal (its text is the
//!    program's output too).
//! 2. **The candidates** ([`suggestions`], pure, unit-tested): the pasted
//!    and the edited words are aligned (longest common subsequence, ignoring
//!    case and the punctuation at their edges); what's left between matched
//!    words are the changes. Kept: 1–4 words replaced by 1–4 words where the
//!    new text looks like a term worth learning (a capital inside the word,
//!    an acronym, digits, a word that isn't an ordinary word — the built-in
//!    list of frequent words, then Windows' spell checker for your
//!    languages — or a capital mid-sentence) and that sounds or spells like
//!    what Yap typed (`fuzzy::distance`: Handy's Levenshtein + Soundex score,
//!    or how alike they sound). Also a change of case alone that makes a
//!    term of a word that isn't ordinary ("github" → "GitHub"). Skipped:
//!    insertions and deletions, punctuation, a new word ending in a
//!    contraction or a possessive ("it's", "Sinead's"), a single ordinary
//!    word heard (an entry would rewrite that word every time you say it:
//!    "motion" → "Notion"), a phrase of only function words, typo-style
//!    fixes between ordinary words, terms already in the dictionary, and
//!    edits that rewrite most of the dictation. One per spelling, terms with
//!    a capital first, at most [`MAX_SUGGESTIONS`].
//! 3. **The prompt** ([`offer`]), in one place (`notice::place`): a toast in
//!    the main window while it's focused, else a card on the Yap bar
//!    (`bar::show_card`; it waits out a dictation, up to a minute), else the
//!    window's toast if it's on screen; neither → not shown, nothing
//!    counted, and never a Windows notification. Add → a
//!    correction (`from` = what Yap heard, `to` = the term, near-misses on as
//!    every new correction) saved to config.json, the pipeline's live config
//!    and the windows' copies (`yap-dictionary-learned`, which Settings
//!    adopts and passes on to the Dictionary view, so neither puts the old
//!    list back). Not now (or ✕) twice, or "Don't suggest this", and that
//!    pair is never offered again; a word you remove from the dictionary is
//!    never offered again at all ([`on_dictionary_saved`], from every save
//!    of the config); at most one prompt per [`MIN_GAP_MS`]. The answers
//!    live in `<data>/word_suggestions.json` (not config.json: Settings
//!    auto-saves its whole copy of the config).
//!
//! Behaviour after Wispr Flow's dictionary auto-learn as measured from its
//! shipped code (`references/wispr-flow/rea-teardown/
//! 05-dictionary-learning.md`: the minute-long watch and its ends, no end on
//! a focus change, the drift gate, the last good edit, 4 words, the
//! contraction filter, removed words never learned again) and OpenWhispr's
//! `correctionLearner.js` (word alignment). Where Yap differs on purpose:
//! Wispr has a cloud classifier confirm each word and adds it with an
//! Undo; Yap decides on this PC and asks you instead. Wispr pins the
//! dictation between everything before and after it in the field, so an
//! edit anywhere else ends the watch; Yap pins it between its nearest
//! neighbours, and picks the occurrence at the caret, not the first.

use std::collections::{BTreeMap, BTreeSet, HashSet};
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
/// A change of more words than this on either side isn't a mishearing
/// (Wispr's limit is on the corrected side; Yap holds what it heard to it
/// too, so a fix with deletions beside it isn't taken for one long word).
const MAX_WORDS: usize = 4;
/// At most one prompt this often.
pub const MIN_GAP_MS: u64 = 3 * 60_000;
/// "Not now" this many times and a pair isn't offered again.
pub const NOT_NOW_LIMIT: u32 = 2;

/// How long a dictation's field is watched after the paste (Wispr's
/// observation window).
const WATCH_FOR: Duration = Duration::from_secs(60);
/// How often it's read: reads at least 400 ms apart, as Wispr's.
const POLL: Duration = Duration::from_millis(700);
/// A field longer than this isn't watched (or no longer).
const MAX_FIELD_CHARS: usize = 15_000;
/// This many reads in a row failing: the field is gone (or hung).
const UNREADABLE_AFTER: u32 = 3;
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
    /// It ends in a contraction or a possessive ([`contracted`]).
    contracted: bool,
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

/// A token ends in a contraction or a possessive — "it's", "don't",
/// "we'll", "Sinead's", "students'" — so it's no dictionary word. "O'Brien"
/// isn't one, nor a word in single quotes ('this').
fn contracted(token: &str) -> bool {
    let t = token
        .trim_start_matches(['"', '“', '«', '‹', '(', '[', '{', '<', '*', '_', '¿', '¡'])
        .trim_end_matches(['"', '”', '»', '›', ')', ']', '}', '>', '*', '_', ',', '.', ';', ':', '!', '?', '…'])
        .to_lowercase()
        .replace(['’', 'ʼ'], "'");
    if t.ends_with('\'') {
        return !t.starts_with(['\'', '‘']);
    }
    ["'s", "'t", "'ll", "'ve", "'re", "'d", "'m"].iter().any(|end| t.ends_with(end))
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
            contracted: contracted(token),
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
    // "Sinead's", "it'll": a form of a word, not one to learn.
    if term.iter().any(|w| w.contracted) {
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
    if old.core == new.core || alnum(new.core).chars().count() < 3 || new.contracted || ordinary(&new.key) {
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
    // Terms with a capital first (that's how a name or a product looks),
    // then one per spelling whatever its case.
    out.sort_by_key(|s| !s.term.chars().any(char::is_uppercase));
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
    /// Find `pasted` (normalised, trimmed) in a read of the field right
    /// after the paste: the occurrence ending nearest the caret, where a
    /// paste leaves it (Wispr takes the first). `None`: it isn't there as
    /// pasted (not landed yet, or the app changed it).
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

    /// Where the pasted span is in a read of the field: between the same
    /// neighbours (if their text was edited too, their nearest few
    /// characters), the candidate nearest the pasted length.
    pub fn locate(&self, w: &FieldWindow) -> Located {
        if let Some(span) = self
            .locate_with(w, ANCHOR_CHARS)
            .or_else(|| self.locate_with(w, SHORT_ANCHOR_CHARS))
        {
            return Located::Span(span);
        }
        let left = tail(&self.left, SHORT_ANCHOR_CHARS);
        let right = head(&self.right, SHORT_ANCHOR_CHARS);
        let left_there = if left.is_empty() {
            self.at_start && w.from_start
        } else {
            w.text.contains(left)
        };
        let right_there = if right.is_empty() {
            self.at_end && w.to_end
        } else {
            w.text.contains(right)
        };
        if left_there && right_there {
            Located::Mismatch
        } else {
            Located::Gone
        }
    }

    /// The span (as the field has it, untrimmed) between the neighbours'
    /// nearest `chars` characters.
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
        best.map(|(s, e, _)| w.text[s..e].to_string())
    }
}

/// [`Anchors::locate`]'s answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Located {
    /// The span between the neighbours, untrimmed.
    Span(String),
    /// A neighbour (or both) isn't in the field any more.
    Gone,
    /// Both are, but not one after the other.
    Mismatch,
}

/// The same words, however the spaces between them changed.
fn same_text(a: &str, b: &str) -> bool {
    a.split_whitespace().eq(b.split_whitespace())
}

/// The most characters an edit of a `len`-character paste may change and
/// still be a correction of it: Wispr Flow's drift gate, as measured,
/// `0.4 / (len / 30 + 1) + 0.2` of its length — about half for a few
/// words, 0.4 at 30 characters, nearer a fifth for a long dictation.
fn drift_max(len: usize) -> usize {
    let len = len as f64;
    let share = 0.4 / (len / 30.0 + 1.0) + 0.2;
    (share * len + 1e-9).floor() as usize
}

/// An edited span is still a correction of `pasted`, not a rewrite or
/// writing on: their edit distance (characters, case ignored) is within
/// [`drift_max`].
pub(crate) fn within_drift(pasted: &str, edited: &str) -> bool {
    let a: Vec<char> = pasted.chars().flat_map(char::to_lowercase).collect();
    let b: Vec<char> = edited.chars().flat_map(char::to_lowercase).collect();
    !a.is_empty() && edit_distance_within(&a, &b, drift_max(pasted.chars().count())).is_some()
}

/// The Levenshtein distance between `a` and `b`, if it's at most `max`
/// (given up as soon as it can't be).
fn edit_distance_within(a: &[char], b: &[char], max: usize) -> Option<usize> {
    if a.len().abs_diff(b.len()) > max {
        return None;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        let mut least = cur[0];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
            least = least.min(cur[j + 1]);
        }
        if least > max {
            return None;
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    Some(prev[b.len()]).filter(|&d| d <= max)
}

/// Line breaks in the whitespace that ends `s`.
fn trailing_line_breaks(s: &str) -> usize {
    s[s.trim_end().len()..].matches('\n').count()
}

// ---- the watch ---------------------------------------------------------------------

/// Why a watch ended. The debug log names it ([`End::name`]); never any text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum End {
    /// [`WATCH_FOR`] is up.
    TimeUp,
    /// Another dictation started (its own paste gets a watch of its own).
    NextDictation,
    /// The field is empty: a chat message sent, or all of it deleted.
    Emptied,
    /// The dictation's text was deleted; the text either side is still there.
    RegionEmptied,
    /// The text either side of it is gone (deleted, or the field replaced).
    RegionGone,
    /// Both neighbours are there, but not around one span any more.
    AnchorMismatch,
    /// A line break was added right after it: Enter (a message sent, or a
    /// new line begun).
    EnterPressed,
    /// The field stopped answering ([`UNREADABLE_AFTER`] reads in a row).
    Unreadable,
    /// The field is longer than [`MAX_FIELD_CHARS`].
    TooLong,
    /// Right after the paste: the pasted text isn't in the field as pasted.
    NotFound,
}

impl End {
    pub(crate) fn name(self) -> &'static str {
        match self {
            End::TimeUp => "time_up",
            End::NextDictation => "next_dictation",
            End::Emptied => "field_emptied",
            End::RegionEmptied => "region_emptied",
            End::RegionGone => "region_gone",
            End::AnchorMismatch => "anchor_mismatch",
            End::EnterPressed => "enter_pressed",
            End::Unreadable => "unreadable",
            End::TooLong => "too_long",
            End::NotFound => "not_found",
        }
    }
}

/// A read isn't the whole field (it was cut short), or the field is longer
/// than Yap watches.
fn too_long(w: &FieldWindow) -> bool {
    !(w.from_start && w.to_end) || w.text.chars().count() > MAX_FIELD_CHARS
}

/// One paste's watch, read by read (pure: the thread hands it the time,
/// whether another dictation started, and what it read).
pub(crate) struct Watch {
    pasted: String,
    anchors: Anchors,
    /// The latest edit within the drift gate (trimmed): what's judged when
    /// the watch ends.
    last_good: Option<String>,
    /// The span the last read found, untrimmed (not measured twice).
    last_span: Option<String>,
    /// Reads in a row that failed.
    failed: u32,
}

impl Watch {
    /// The first read, right after the paste of `pasted` (normalised,
    /// trimmed): where it landed.
    pub fn start(first: &FieldWindow, pasted: &str) -> Result<Watch, End> {
        if too_long(first) {
            return Err(End::TooLong);
        }
        let anchors = Anchors::find(first, pasted).ok_or(End::NotFound)?;
        Ok(Watch {
            pasted: pasted.to_string(),
            anchors,
            last_good: None,
            last_span: None,
            failed: 0,
        })
    }

    /// The dictation began a sentence.
    pub fn starts_sentence(&self) -> bool {
        self.anchors.starts_sentence
    }

    /// The edit to look at: the last span within the drift gate, if any
    /// read found one (it may be the paste unchanged).
    pub fn last_good(&self) -> Option<&str> {
        self.last_good.as_deref()
    }

    /// One poll: `elapsed` since the watch began, `superseded` (another
    /// dictation started), `read` reads the field (only when it's needed).
    /// `Some`: the watch is over, and why.
    pub fn step(
        &mut self,
        elapsed: Duration,
        superseded: bool,
        read: impl FnOnce() -> Option<FieldWindow>,
    ) -> Option<End> {
        if superseded {
            return Some(End::NextDictation);
        }
        if elapsed >= WATCH_FOR {
            return Some(End::TimeUp);
        }
        self.see(read())
    }

    /// One read of the field (`None`: it failed).
    fn see(&mut self, read: Option<FieldWindow>) -> Option<End> {
        let Some(w) = read else {
            self.failed += 1;
            return (self.failed >= UNREADABLE_AFTER).then_some(End::Unreadable);
        };
        self.failed = 0;
        if too_long(&w) {
            return Some(End::TooLong);
        }
        if w.text.trim().is_empty() {
            return Some(End::Emptied);
        }
        let raw = match self.anchors.locate(&w) {
            Located::Span(raw) => raw,
            Located::Gone => return Some(End::RegionGone),
            Located::Mismatch => return Some(End::AnchorMismatch),
        };
        let span = raw.trim();
        if span.is_empty() {
            return Some(End::RegionEmptied);
        }
        if self.last_span.as_deref() != Some(raw.as_str()) {
            // Before an Enter too: a fix and the Enter may come in one read.
            if within_drift(&self.pasted, span) {
                self.last_good = Some(span.to_string());
            }
            self.last_span = Some(raw.clone());
        }
        // More line breaks after it than the paste had: Enter.
        (trailing_line_breaks(&raw) > trailing_line_breaks(&self.pasted)).then_some(End::EnterPressed)
    }
}

/// Each paste's watch has a number; a newer one ends the older.
static WATCHES: AtomicU64 = AtomicU64::new(0);

/// A dictation's text `pasted` (as pasted, fitted to the cursor) just went
/// into `target`: watch that field for a while for corrections, on a thread
/// of its own (see the module docs). `enabled`: `learn_words`. Never in a
/// test run — in robot mode only a window the test opened
/// (`realtest::permits`), the real-app tests' correction flow.
pub fn after_paste(app: &AppHandle, pasted: &str, target: Option<isize>, enabled: bool) {
    if !enabled || !crate::realtest::permits(target) {
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
    let mut started = Err(End::NotFound);
    for _ in 0..4 {
        if superseded() {
            return;
        }
        started = field
            .read(MAX_FIELD_CHARS)
            .map_or(Err(End::Unreadable), |w| Watch::start(&w, pasted));
        if !matches!(started, Err(End::NotFound | End::Unreadable)) {
            break;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    let mut watch = match started {
        Ok(watch) => watch,
        Err(end) => {
            tracing::debug!(len = pasted.len(), end = end.name(), "learn: not watching the field");
            return;
        }
    };
    tracing::debug!(len = pasted.len(), "learn: watching the field for corrections");
    let began = Instant::now();
    let end = loop {
        std::thread::sleep(POLL);
        if let Some(end) = watch.step(began.elapsed(), superseded(), || field.read(MAX_FIELD_CHARS)) {
            break end;
        }
    };
    let edited = watch.last_good().filter(|e| !same_text(e, pasted));
    tracing::debug!(
        end = end.name(),
        secs = began.elapsed().as_secs(),
        edited = edited.map_or(0, str::len),
        "learn: stopped watching the field"
    );
    if let Some(edited) = edited {
        consider(app, pasted, edited, watch.starts_sentence());
    }
}

/// The last good edit of a watch that ended: offer what's worth learning
/// in it.
fn consider(app: &AppHandle, pasted: &str, edited: &str, starts_sentence: bool) {
    let cfg = crate::config::load();
    if !cfg.learn_words {
        return;
    }
    let ordinary = Ordinary::new();
    let found = suggestions(pasted, edited, starts_sentence, &cfg.dictionary, &|w| ordinary.is(w));
    tracing::debug!(found = found.len(), "learn: looked at an edit");
    if !found.is_empty() {
        offer(app, found, false);
    }
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
    /// Terms you took out of the dictionary (lowercased): never suggested
    /// again, whatever Yap heard (Wispr never re-learns a removed word).
    pub removed: BTreeSet<String>,
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

/// May `s` be offered (not turned down for good, not a word you removed)?
pub fn allowed(rec: &Record, s: &Suggestion) -> bool {
    !rec.removed.contains(&s.term.trim().to_lowercase())
        && rec
            .pairs
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

/// The corrections `old` had that `new` doesn't — neither what they're
/// heard as nor their spelling is in it any more (changing one of the two
/// is an edit, not a removal). Snippets aren't words to learn.
pub fn removed_entries(old: &[DictionaryEntry], new: &[DictionaryEntry]) -> Vec<Suggestion> {
    let key = |s: &str| s.trim().to_lowercase();
    let corrections = |d: &[DictionaryEntry]| d.iter().filter(|e| !e.snippet).cloned().collect::<Vec<_>>();
    let new = corrections(new);
    let froms: HashSet<String> = new.iter().map(|e| key(&e.from)).collect();
    let tos: HashSet<String> = new.iter().map(|e| key(&e.to)).collect();
    corrections(old)
        .into_iter()
        .filter(|e| !key(&e.to).is_empty() && !tos.contains(&key(&e.to)) && !froms.contains(&key(&e.from)))
        .map(|e| Suggestion {
            heard: e.from.trim().to_string(),
            term: e.to.trim().to_string(),
        })
        .collect()
}

/// `rec` after `words` left the dictionary: their terms are never offered
/// again, and the pairs count as "Don't suggest this".
pub fn forget_removed(mut rec: Record, words: &[Suggestion]) -> Record {
    for s in words {
        rec.removed.insert(s.term.to_lowercase());
    }
    answered(rec, words, "never")
}

/// The config was saved (`commands::save_config`: the Dictionary view,
/// Settings, every surface): words it took out of the dictionary are never
/// suggested again. Writes `word_suggestions.json` only, never config.json.
pub fn on_dictionary_saved(old: &[DictionaryEntry], new: &[DictionaryEntry]) {
    let removed = removed_entries(old, new);
    if removed.is_empty() {
        return;
    }
    update_record(|r| forget_removed(r, &removed));
    tracing::info!(count = removed.len(), "learn: words removed from the dictionary won't be suggested again");
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
    // One place (`notice.rs`): the main window while the person is looking
    // at it, else the Yap bar (else the window, on screen after all).
    let place = crate::notice::place(app, false);
    if place == crate::notice::Place::Nowhere {
        tracing::debug!("learn: nowhere to show the suggestion");
        return false;
    }
    let in_window = place == crate::notice::Place::Window;
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
/// seen it, run through the same rules, the prompt put up the same way
/// (the gap between prompts aside). Either `edited` (the span as the field
/// ends up with it; the drift gate still applies; `before` = the field's
/// text before it, for whether it starts a sentence: none = it does) or
/// `reads`: the whole field's text read by read, the first right after the
/// paste (`null` = a read that failed) — the watch runs over them
/// ([`staged_watch`]: past the last, time is up) and its last good edit is
/// looked at. `fresh`: forget every answer first. Returns what was offered.
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn e2e_learn_correction(
    app: AppHandle,
    pasted: String,
    edited: Option<String>,
    before: Option<String>,
    reads: Option<Vec<Option<String>>>,
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
        let pasted = normalise(&pasted).trim().to_string();
        let (edited, starts) = match reads {
            Some(reads) => match staged_watch(&pasted, &reads) {
                Ok((edited, starts, end)) => {
                    tracing::info!(end = end.name(), "e2e: the staged watch ended");
                    match edited {
                        Some(edited) => (edited, starts),
                        None => return Ok(Vec::new()),
                    }
                }
                Err(end) => {
                    tracing::info!(end = end.name(), "e2e: the staged watch didn't start");
                    return Ok(Vec::new());
                }
            },
            None => {
                let edited = normalise(&edited.unwrap_or_default()).trim().to_string();
                if !within_drift(&pasted, &edited) {
                    tracing::info!("e2e: the staged edit is past the drift gate");
                    return Ok(Vec::new());
                }
                (edited, before.as_deref().is_none_or(before_starts_sentence))
            }
        };
        let ordinary = Ordinary::new();
        let found = suggestions(&pasted, &edited, starts, &cfg.dictionary, &|w| ordinary.is(w));
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

/// A whole field's text as a read of it, the caret at its end.
#[cfg(any(test, debug_assertions))]
fn whole_field(text: &str) -> FieldWindow {
    let text = normalise(text);
    FieldWindow {
        caret: text.len(),
        to_end: text.chars().count() <= MAX_FIELD_CHARS,
        text,
        from_start: true,
    }
}

/// [`Watch`] over staged reads of a field (`None`: a read that failed),
/// the first right after the paste of `pasted`, one [`POLL`] apart; past the
/// last, time is up. The last good edit (if it isn't the paste unchanged),
/// whether it starts a sentence, and why the watch ended.
#[cfg(any(test, debug_assertions))]
fn staged_watch(pasted: &str, reads: &[Option<String>]) -> Result<(Option<String>, bool, End), End> {
    let mut reads = reads.iter().map(|r| r.as_deref().map(whole_field));
    let first = reads.next().flatten().ok_or(End::Unreadable)?;
    let mut watch = Watch::start(&first, pasted)?;
    let mut end = End::TimeUp;
    for (i, read) in reads.enumerate() {
        let elapsed = POLL * (i as u32 + 1);
        if let Some(e) = watch.step(elapsed, false, || read) {
            end = e;
            break;
        }
    }
    let edited = watch.last_good().filter(|e| !same_text(e, pasted)).map(str::to_string);
    Ok((edited, watch.starts_sentence(), end))
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
        // More than four words on a side.
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

    /// The span [`Anchors::locate`] found, trimmed.
    fn span(found: Located) -> Option<String> {
        match found {
            Located::Span(s) => Some(s.trim().to_string()),
            _ => None,
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
        assert_eq!(span(a.locate(&window(&edited, 20))).as_deref(), Some("Deploy it to Kubernetes."));
        // Typing on after it, and a word changed a little before it.
        let more = format!("Memo for Monday. Deploy it to Kubernetes. More{after}");
        assert_eq!(span(a.locate(&window(&more, 30))).as_deref(), Some("Deploy it to Kubernetes. More"));
        // Its neighbours gone entirely: lost.
        assert_eq!(a.locate(&window("Something else entirely", 3)), Located::Gone);
        // Both still there, but the wrong way round.
        assert_eq!(a.locate(&window("\nThen lunch. Notes for Monday. ", 3)), Located::Mismatch);
    }

    #[test]
    fn a_paste_at_the_field_start_or_end_is_found_by_the_edges() {
        let pasted = "Graph ana is up.";
        let a = Anchors::find(&window(pasted, pasted.len()), pasted).expect("found");
        assert_eq!(span(a.locate(&window("Grafana is up.", 3))).as_deref(), Some("Grafana is up."));
        // A read that isn't the whole field can't vouch for the edges.
        let partial = FieldWindow {
            from_start: false,
            ..window("Grafana is up.", 3)
        };
        assert_eq!(a.locate(&partial), Located::Gone);
        // The occurrence nearest the caret is the paste.
        let twice = "Yes. Yes.";
        let a = Anchors::find(&window(twice, twice.len()), "Yes.").expect("found");
        assert_eq!(a.left, "Yes. ");
        assert!(a.starts_sentence);
    }

    #[test]
    fn the_drift_gate_tells_corrections_from_rewrites() {
        // 0.4 / (len / 30 + 1) + 0.2 of the paste's length, rounded down.
        assert_eq!(drift_max(3), 1);
        assert_eq!(drift_max(10), 5);
        assert_eq!(drift_max(30), 12);
        assert_eq!(drift_max(300), 70);
        let thirty = "abcdefghij".repeat(3);
        let changed = |n: usize| format!("{}{}", "x".repeat(n), &thirty[n..]);
        assert!(within_drift(&thirty, &changed(12)));
        assert!(!within_drift(&thirty, &changed(13)));
        // Case doesn't count.
        assert!(within_drift("send the json payload", "SEND THE JSON PAYLOAD"));
        // A fixed word: in; writing on, or a rewrite: out.
        assert!(within_drift("Deploy it to cube and eighties.", "Deploy it to Kubernetes."));
        assert!(within_drift("Power to keep is the default model.", "Parakeet is the default model."));
        assert!(!within_drift("A short one.", &"and then I kept typing ".repeat(3)));
        assert!(!within_drift("We shipped it.", "We shipped it, and then I wrote on and on."));
        assert!(!within_drift("Book the room for Sarah.", "Cancel everything this week."));
        // The distance itself, and giving up early.
        let chars = |s: &str| s.chars().collect::<Vec<_>>();
        assert_eq!(edit_distance_within(&chars("kitten"), &chars("sitting"), 3), Some(3));
        assert_eq!(edit_distance_within(&chars("kitten"), &chars("sitting"), 2), None);
        assert_eq!(edit_distance_within(&chars("a"), &chars("abcdef"), 2), None);
        assert!(same_text("Deploy  it\nnow", "Deploy it now"));
        assert!(!same_text("Deploy it now", "Deploy it later"));
        assert_eq!(normalise("a\r\nb\rc\u{a0}d"), "a\nb\nc d");
    }

    const PASTED: &str = "Deploy it to cube and eighties.";
    const BEFORE: &str = "Notes for Monday. ";
    const AFTER: &str = "\nThen lunch.";

    /// A watch of [`PASTED`] pasted between [`BEFORE`] and [`AFTER`].
    fn watching() -> Watch {
        Watch::start(&whole_field(&fixed(PASTED)), PASTED).expect("started")
    }

    /// One read of the field, a second into the watch.
    fn read(w: &mut Watch, text: Option<&str>) -> Option<End> {
        w.step(Duration::from_secs(1), false, || text.map(whole_field))
    }

    /// The field with `span` where the paste went.
    fn fixed(span: &str) -> String {
        format!("{BEFORE}{span}{AFTER}")
    }

    #[test]
    fn every_way_a_watch_ends() {
        // Time, and another dictation: before any read.
        let mut w = watching();
        assert_eq!(w.step(WATCH_FOR, false, || unreachable!()), Some(End::TimeUp));
        assert_eq!(w.step(Duration::from_secs(5), true, || unreachable!()), Some(End::NextDictation));
        // A read like the paste, or a fix: it goes on. (Nothing here knows
        // about the focus: switching apps is no end.)
        assert_eq!(read(&mut w, Some(&fixed(PASTED))), None);
        assert_eq!(read(&mut w, Some(&fixed("Deploy it to Kubernetes."))), None);
        // The field emptied (a chat message sent), or only blank.
        assert_eq!(read(&mut watching(), Some("")), Some(End::Emptied));
        assert_eq!(read(&mut watching(), Some(" \n ")), Some(End::Emptied));
        // The dictation deleted, its neighbours kept.
        assert_eq!(read(&mut watching(), Some(&fixed(""))), Some(End::RegionEmptied));
        // Its neighbours gone, or the wrong way round.
        assert_eq!(read(&mut watching(), Some("Then lunch.")), Some(End::RegionGone));
        assert_eq!(read(&mut watching(), Some("\nThen lunch. Notes for Monday. ")), Some(End::AnchorMismatch));
        // Enter right after it (a line break it didn't have)…
        assert_eq!(read(&mut watching(), Some(&fixed(&format!("{PASTED}\n")))), Some(End::EnterPressed));
        // …but not one before it, or a word typed on.
        assert_eq!(read(&mut watching(), Some(&fixed(&format!("\n{PASTED}")))), None);
        assert_eq!(read(&mut watching(), Some(&fixed(&format!("{PASTED} More")))), None);
        // Reads failing: three in a row is the end; one that works resets it.
        let mut w = watching();
        assert_eq!(read(&mut w, None), None);
        assert_eq!(read(&mut w, None), None);
        assert_eq!(read(&mut w, Some(&fixed(PASTED))), None);
        assert_eq!(read(&mut w, None), None);
        assert_eq!(read(&mut w, None), None);
        assert_eq!(read(&mut w, None), Some(End::Unreadable));
        // Past 15,000 characters (a read cut short says so too).
        let long = fixed(&format!("{PASTED} {}", "word ".repeat(3_000)));
        assert_eq!(read(&mut watching(), Some(&long)), Some(End::TooLong));
        let cut = FieldWindow {
            to_end: false,
            ..whole_field(&fixed(PASTED))
        };
        assert_eq!(watching().step(Duration::from_secs(1), false, || Some(cut)), Some(End::TooLong));
        // Before it starts: not there as pasted, or already too long.
        assert!(matches!(Watch::start(&whole_field("Something else."), PASTED), Err(End::NotFound)));
        assert!(matches!(Watch::start(&whole_field(&long), PASTED), Err(End::TooLong)));
        for end in [End::TimeUp, End::Emptied, End::EnterPressed, End::RegionGone, End::AnchorMismatch] {
            assert!(!end.name().contains(' '), "{end:?}");
        }
    }

    #[test]
    fn the_last_edit_within_the_drift_gate_is_judged() {
        let staged = |reads: &[Option<&str>]| {
            let reads: Vec<Option<String>> = reads.iter().map(|r| r.map(str::to_string)).collect();
            staged_watch(PASTED, &reads).expect("started")
        };
        let first = fixed(PASTED);
        let fix = fixed("Deploy it to Kubernetes.");
        // A fix, then the field sent (emptied): the fix.
        let (edited, starts, end) = staged(&[Some(&first), Some(&fix), Some("")]);
        assert_eq!((edited.as_deref(), starts, end), (Some("Deploy it to Kubernetes."), true, End::Emptied));
        // A fix and Enter in the same read: the fix, without the line break.
        let entered = fixed("Deploy it to Kubernetes.\n");
        let (edited, _, end) = staged(&[Some(&first), Some(&entered)]);
        assert_eq!((edited.as_deref(), end), (Some("Deploy it to Kubernetes."), End::EnterPressed));
        // A fix, then a rewrite past the gate, then time up: the fix.
        let rewrite = fixed("Honestly, forget all of that and just ship whatever we have now please.");
        let (edited, _, end) = staged(&[Some(&first), Some(&fix), Some(&rewrite)]);
        assert_eq!((edited.as_deref(), end), (Some("Deploy it to Kubernetes."), End::TimeUp));
        // Two fixes: the later one.
        let again = fixed("Deploy it to Kubernetes now.");
        let (edited, _, _) = staged(&[Some(&first), Some(&fix), Some(&again)]);
        assert_eq!(edited.as_deref(), Some("Deploy it to Kubernetes now."));
        // Fixed, then put back: nothing to look at.
        let (edited, _, _) = staged(&[Some(&first), Some(&fix), Some(&first)]);
        assert_eq!(edited, None);
        // Never within the gate: nothing, however it ends.
        let (edited, _, end) = staged(&[Some(&first), Some(&rewrite), None, None, None]);
        assert_eq!((edited, end), (None, End::Unreadable));
        // The whole field was the paste (a chat box): fixed, then sent.
        let (edited, _, end) = staged_watch(
            PASTED,
            &[Some(PASTED.into()), Some("Deploy it to Kubernetes.".into()), Some(String::new())],
        )
        .expect("started");
        assert_eq!((edited.as_deref(), end), (Some("Deploy it to Kubernetes."), End::Emptied));
        // …and what it offers.
        let edited = edited.expect("an edit");
        assert_eq!(
            suggestions(PASTED, &edited, true, &[], &is_frequent),
            vec![Suggestion {
                heard: "cube and eighties".into(),
                term: "Kubernetes".into()
            }]
        );
        // Not in the field as pasted, or not read: no watch at all.
        assert!(matches!(staged_watch(PASTED, &[Some("Other text".into())]), Err(End::NotFound)));
        assert!(matches!(staged_watch(PASTED, &[None]), Err(End::Unreadable)));
    }

    #[test]
    fn up_to_four_words_on_a_side() {
        let tail = "on the new cluster before we ship it on Friday";
        // Four for four.
        assert_eq!(
            found(
                &format!("Set up graffana loky tempoh mimeer {tail}"),
                &format!("Set up Grafana Loki Tempo Mimir {tail}")
            ),
            pair("graffana loky tempoh mimeer", "Grafana Loki Tempo Mimir")
        );
        // Five: no longer a mishearing.
        assert!(found(
            &format!("Set up graffana loky tempoh mimeer pyroscop {tail}"),
            &format!("Set up Grafana Loki Tempo Mimir Pyroscope {tail}")
        )
        .is_empty());
    }

    #[test]
    fn contractions_and_possessives_are_not_words_to_learn() {
        for w in ["it's", "Sinead’s", "don't", "we'll", "they've", "you're", "I'd", "I'm", "students'", "Kubernetes's,"] {
            assert!(contracted(w), "{w}");
        }
        for w in ["O'Brien", "rock'n'roll", "'quoted'", "‘quoted’", "Kubernetes."] {
            assert!(!contracted(w), "{w}");
        }
        // A possessive fixed: not offered (the name alone is, see above).
        assert!(found("I spoke to Shunade's team.", "I spoke to Sinead's team.").is_empty());
        assert!(found("Fix the cube and eighties' config.", "Fix the Kubernetes' config.").is_empty());
        assert!(found("It is hubspot's fault.", "It is HubSpot's fault.").is_empty());
        // In single quotes it's still the word; a name with an apostrophe is fine.
        assert_eq!(
            found("Fix the 'cube and eighties' config.", "Fix the 'Kubernetes' config."),
            pair("cube and eighties", "Kubernetes")
        );
        assert_eq!(found("Call oh brian about it.", "Call O'Brien about it."), pair("oh brian", "O'Brien"));
    }

    #[test]
    fn one_suggestion_per_spelling_capitals_first() {
        // The same word fixed twice, once with a capital: one, the capital.
        assert_eq!(
            found(
                "Use cube and eighties here and cube and eighties there for the team today.",
                "Use kubernetes here and Kubernetes there for the team today."
            ),
            pair("cube and eighties", "Kubernetes")
        );
        // Capitals first, then the rest in order.
        assert_eq!(
            found(
                "Run cube cuttle then open graph ana for the whole team now.",
                "Run kubectl then open Grafana for the whole team now."
            ),
            vec![
                ("graph ana".to_string(), "Grafana".to_string()),
                ("cube cuttle".to_string(), "kubectl".to_string())
            ]
        );
    }

    #[test]
    fn removed_words_are_never_suggested_again() {
        let snippet = DictionaryEntry {
            snippet: true,
            ..entry("k8s", "Kubernetes")
        };
        let old = vec![entry("cuber netties", "Kubernetes"), entry("jaison", "JSON"), snippet.clone()];
        // One removed: that one.
        let removed = removed_entries(&old, &[entry("jaison", "JSON"), snippet.clone()]);
        assert_eq!(
            removed,
            vec![Suggestion {
                heard: "cuber netties".into(),
                term: "Kubernetes".into()
            }]
        );
        // Edits aren't removals: what it's heard as, or its spelling, kept.
        let respelled = [entry("cuber netties", "Kubernetes!"), entry("jaison", "JSON"), snippet.clone()];
        assert!(removed_entries(&old, &respelled).is_empty());
        let reheard = [entry("cube and eighties", "kubernetes"), entry("jaison", "JSON")];
        assert!(removed_entries(&old, &reheard).is_empty());
        // A snippet removed isn't a word; nothing removed, nothing.
        assert!(removed_entries(&[snippet], &[]).is_empty());
        assert!(removed_entries(&[], &old).is_empty());
        assert!(removed_entries(&old, &old).is_empty());
        // Removed: never offered again, whatever Yap heard, in any case.
        let rec = forget_removed(Record::default(), &removed);
        for heard in ["cuber netties", "cube and eighties"] {
            let s = Suggestion {
                heard: heard.into(),
                term: "KUBERNETES".into(),
            };
            assert!(!allowed(&rec, &s), "{heard}");
        }
        assert!(allowed(&rec, &Suggestion { heard: "graph ana".into(), term: "Grafana".into() }));
        assert_eq!(rec.pairs.get("cuber netties\u{2192}kubernetes").map(|p| p.never), Some(true));
        // The file's shape, and an older file without it.
        let json = serde_json::to_string(&rec).unwrap();
        assert!(json.contains("\"removed\":[\"kubernetes\"]"), "{json}");
        let old_file: Record = serde_json::from_str(r#"{"lastOfferedAt":5,"pairs":{}}"#).unwrap();
        assert!(old_file.removed.is_empty() && old_file.last_offered_at == 5);
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

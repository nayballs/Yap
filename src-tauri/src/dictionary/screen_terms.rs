//! **"Spell names the way they're written on screen"** (`config.screen_names`,
//! on by default): names and terms come out of a dictation spelled as the
//! window you're dictating into shows them — Aqua Voice's "Deep Context",
//! done locally, without screenshots.
//!
//! 1. **At record start** ([`begin_dictation`], beside `selection`'s read of
//!    the text around the cursor) a background thread reads the window's
//!    **visible text** over UI Automation: its title, the document the focus
//!    is in (an editor, a mail being written: its visible ranges; a web page:
//!    the text around the focus, as Chromium takes seconds to work out a long
//!    page's visible ranges), and a bounded walk over the window's other
//!    visible text elements (labels, list items, links, tabs…) — at most
//!    [`MAX_CHARS`] characters, each call time-boxed ([`CALL_TIMEOUT_MS`]),
//!    the whole read within [`READ_BUDGET`]. The same refusals as `selection.rs`: only the target
//!    window's own process, never a password box, never Yap's own windows,
//!    never a password manager, never in a test run (robot mode: only the
//!    test's own windows, `realtest::permits`).
//! 2. **The terms** in it (`extract.rs`): capitalised names that aren't
//!    everyday words, full names, CamelCase, acronyms, words with digits,
//!    handles — everyday words dropped (`learn::is_frequent`, then Windows'
//!    spell checker). Kept **in memory, for that dictation only**; the log
//!    says how long and how many, never what.
//! 3. **Used**, never waited for (still reading = not used):
//!    - **A near-miss pass**, for every model, with memory's strict guards
//!      and more ([`ScreenTerms::correct`]): a target is never an everyday
//!      word and has ≥ 4 letters a word (`memory::vocab::safe_for_fuzzy`);
//!      what was heard must not be ordinary words only, must be written as
//!      a name (a capital), must spell **and** sound close
//!      (`fuzzy::apply_screen`), and is never a spelling the dictionary or
//!      memory gives (those have the final say).
//!    - **Not the Whisper prompt**: measured with real speech, it made
//!      Whisper hear everyday words as a name on screen ([`whisper_names`]).
//!    - **AI cleanup**: "Spell these names as written: …" with the few terms
//!      the transcript seems to contain ([`ScreenTerms::cleanup_line`]) —
//!      the only part that can leave the PC (with the transcript, to a cloud
//!      cleanup provider); the screen's text never does.
//! 4. **In an editor or a terminal** (2026-10-29, `code.rs`): the same text
//!    gives the dictation's file names and code symbols, written as the
//!    code has them when said aloud ("user service dot ts" →
//!    `userService.ts`), and where the focus is (the editor, its AI chat
//!    box, its terminal) for file tagging (`pasting/file_tags.rs`). An
//!    editor's tabs and file tree are asked for in one more time-boxed call.
//!    In a chat app, the people's names on screen are what "at Sam"
//!    mentions (`pasting/mentions.rs`).

pub mod code;
mod extract;

pub use extract::{Kind, Term};

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::config::{DictionaryEntry, YapConfig};

/// At most this many characters of a window's text are read.
pub const MAX_CHARS: usize = 20_000;
/// How long one UI Automation call may take (connection and transaction
/// timeouts): a hung or slow app never holds the read up for long.
const CALL_TIMEOUT_MS: u32 = 400;
/// The whole read's budget: past it, nothing more is asked of the app.
const READ_BUDGET: Duration = Duration::from_millis(1_500);
/// Terms for the Whisper prompt at most (the most frequent first; the
/// prompt's own cap may keep fewer).
const PROMPT_TERMS: usize = 40;
/// Near-miss targets at most (the most frequent terms first).
const MAX_TARGETS: usize = 120;
/// Terms named to AI cleanup at most.
const CLEANUP_TERMS: usize = 8;

/// Password managers: their windows are never read.
const NEVER_READ: &[&str] = &[
    "1password", "bitwarden", "keepass", "keepassxc", "lastpass", "dashlane", "enpass", "keeper", "roboform",
    "nordpass", "proton pass", "protonpass",
];

/// What a dictation got from the screen: the terms (the most frequent
/// first) and the near-miss targets made of them.
#[derive(Debug, Default)]
pub struct ScreenTerms {
    pub terms: Vec<Term>,
    /// Full names that may be near-miss targets ("Siobhan Ní Bhriain"):
    /// run before the single words, so a whole name heard a little wrong is
    /// spelled as one ("Siobhan Ni Bhriain" → "Siobhan Ní Bhriain").
    phrases: Vec<DictionaryEntry>,
    /// Single terms that may be near-miss targets.
    words: Vec<DictionaryEntry>,
    /// In an editor or a terminal: its file names and symbols, and where
    /// the focus is (`code.rs`).
    pub code: Option<code::CodeNames>,
}

fn entry(to: &str) -> DictionaryEntry {
    DictionaryEntry {
        from: String::new(),
        to: to.to_string(),
        fuzzy: true,
        snippet: false,
        starred: false,
        also_heard: Vec::new(),
    }
}

/// A full name may be a near-miss target when none of its words is an
/// everyday word, a first name that's also a word ("Will"), or one the
/// spell checker knows, and it has 8 letters or more in all. (Its short
/// parts — "Ní", "van" — may be short.)
fn phrase_safe(name: &str, is_word: &dyn Fn(&str) -> bool) -> bool {
    let words: Vec<&str> = name.split_whitespace().collect();
    let letters = name.chars().filter(|c| c.is_alphabetic()).count();
    words.len() >= 2
        && letters >= 8
        && words.iter().all(|w| {
            let key: String = w.to_lowercase().chars().filter(|c| c.is_alphanumeric() || *c == '\'').collect();
            let short = key.chars().count() < 3;
            !crate::memory::vocab::NAME_WORDS.contains(&key.as_str()) && (short || (!crate::dictionary::learn::is_frequent(&key) && !is_word(&key)))
        })
}

impl ScreenTerms {
    /// The terms, and the near-miss targets among them: only with a spell
    /// checker (`spell_checker`; without one Yap can't tell a name from a
    /// word it doesn't list, as for memory's names), never a handle.
    pub fn new(terms: Vec<Term>, is_word: &dyn Fn(&str) -> bool, spell_checker: bool) -> ScreenTerms {
        let mut phrases = Vec::new();
        let mut words = Vec::new();
        if spell_checker {
            for t in terms.iter().filter(|t| t.kind != Kind::Handle).take(MAX_TARGETS) {
                if t.kind == Kind::FullName {
                    if phrase_safe(&t.text, is_word) {
                        phrases.push(entry(&t.text));
                    }
                } else if crate::memory::vocab::safe_for_fuzzy(&t.text, is_word) {
                    words.push(entry(&t.text));
                }
            }
        }
        ScreenTerms { terms, phrases, words, code: None }
    }

    /// For the Whisper prompt: the most frequent terms that aren't already
    /// in `taken` (the dictionary's words and memory's names), no handles.
    pub fn prompt_entries(&self, taken: &[DictionaryEntry]) -> Vec<DictionaryEntry> {
        let mut seen: HashSet<String> = taken.iter().map(|e| e.to.trim().to_lowercase()).collect();
        self.terms
            .iter()
            .filter(|t| t.kind != Kind::Handle)
            .filter(|t| seen.insert(t.text.to_lowercase()))
            .take(PROMPT_TERMS)
            .map(|t| entry(&t.text))
            .collect()
    }

    /// The near-miss pass (ONNX models): a name or term heard a little
    /// wrong, spelled as on screen. Never a spelling `keep` says is the
    /// dictionary's or memory's (`keep` gets [`fuzzy::ngram_key`]s), never
    /// ordinary words heard ("row shell", "zen tricks"), only a heard text
    /// that both spells and sounds like the term (`fuzzy::apply_screen`).
    /// `is_word`: is this lower-case word an everyday one?
    pub fn correct(&self, text: &str, keep: &dyn Fn(&str) -> bool, is_word: &dyn Fn(&str) -> bool) -> String {
        let targets = |list: &[DictionaryEntry]| -> Vec<DictionaryEntry> {
            list.iter().filter(|e| !keep(&crate::dictionary::fuzzy::ngram_key(&e.to))).cloned().collect()
        };
        let text = crate::dictionary::fuzzy::apply_screen(text, &targets(&self.phrases), is_word, keep);
        crate::dictionary::fuzzy::apply_screen(&text, &targets(&self.words), is_word, keep)
    }

    /// The terms `transcript` (what the model heard) seems to contain, best
    /// match first, at most [`CLEANUP_TERMS`] — none that `keep` says the
    /// dictionary or memory already spells.
    pub fn in_transcript(&self, transcript: &str, keep: &dyn Fn(&str) -> bool) -> Vec<&str> {
        let words: Vec<&str> = transcript.split_whitespace().collect();
        let mut found: Vec<(f64, usize, &str)> = Vec::new();
        for (i, t) in self.terms.iter().enumerate() {
            if keep(&crate::dictionary::fuzzy::ngram_key(&t.text)) {
                continue;
            }
            if let Some(score) = seems_said(&words, t) {
                found.push((score, i, &t.text));
            }
        }
        found.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        found.into_iter().take(CLEANUP_TERMS).map(|(_, _, t)| t).collect()
    }

    /// AI cleanup's line ("Spell these names as written: Zentrix, Siobhan
    /// Ní Bhriain."), or `None` when the transcript seems to hold none.
    pub fn cleanup_line(&self, transcript: &str, keep: &dyn Fn(&str) -> bool) -> Option<String> {
        // Code names said aloud first ("user service dot ts" → userService.ts):
        // matched word for word, they're the surest.
        let mut terms: Vec<String> = self
            .code
            .as_ref()
            .map(|c| c.said_in(transcript, CLEANUP_TERMS))
            .unwrap_or_default()
            .into_iter()
            .filter(|t| !keep(&crate::dictionary::fuzzy::ngram_key(t)))
            .collect();
        for t in self.in_transcript(transcript, keep) {
            if terms.len() >= CLEANUP_TERMS {
                break;
            }
            if !terms.iter().any(|x| x.eq_ignore_ascii_case(t)) {
                terms.push(t.to_string());
            }
        }
        (!terms.is_empty()).then(|| format!("Spell these names as written: {}.", terms.join(", ")))
    }
}

/// How closely `words` (a transcript's) seem to hold `term`: the best of its
/// runs of 1 … (the term's parts + 1) words, `None` when none is close.
/// Looser than the near-miss pass (cleanup's model has the context to
/// decide): the same letters (0), or alike in spelling (plain edit distance
/// ≤ 0.4 of the longer), or in sound (`fuzzy::sound_distance` ≤ 0.25), or
/// both a little (the Soundex code and ≤ 0.6). A handle only as it's said
/// whole ([`handle_said`]).
fn seems_said(words: &[&str], term: &Term) -> Option<f64> {
    if term.kind == Kind::Handle {
        return handle_said(words, &term.text);
    }
    let mut best: Option<f64> = None;
    {
        let part = &term.text;
        let key = letters(part);
        if key.chars().count() < 3 {
            return None;
        }
        let pieces = part.split(|c: char| !c.is_alphanumeric()).filter(|p| !p.is_empty()).count().max(1);
        for n in 1..=(pieces + 1).min(5) {
            for run in words.windows(n) {
                let heard = letters(&run.join(" "));
                // A run of everyday words only is no name ("the plan").
                if heard.is_empty() || run.iter().all(|w| crate::dictionary::learn::is_frequent(&letters(w))) && heard != key {
                    continue;
                }
                let score = if heard == key {
                    0.0
                } else {
                    let plain = crate::dictionary::fuzzy::plain_spelling_distance(&heard, &key);
                    let (sound, len) = crate::dictionary::fuzzy::sound_distance(&heard, &key);
                    let (soundex, _) = crate::dictionary::fuzzy::spelling_distance(&heard, &key);
                    let soundex_hit = soundex < plain;
                    if plain <= 0.4 {
                        plain
                    } else if sound <= 0.25 && len >= 3 {
                        sound + 0.1
                    } else if soundex_hit && plain <= 0.6 {
                        plain
                    } else {
                        continue;
                    }
                };
                if best.is_none_or(|b| score < b) {
                    best = Some(score);
                }
            }
        }
    }
    best
}

/// Does the transcript say the handle ("@zentrix_dev": "at zentrix dev" or
/// "at zentrix underscore dev"; "jane.doe@example.com": "jane dot doe at
/// example dot com")? Nearly letter for letter (edit distance ≤ 0.12): its
/// name alone ("the zentrix pilot") is the product, not the handle.
fn handle_said(words: &[&str], handle: &str) -> Option<f64> {
    let mut best: Option<f64> = None;
    for (underscore, dash) in [(" ", " "), (" underscore ", " dash ")] {
        let spoken = handle
            .replace('@', " at ")
            .replace('.', " dot ")
            .replace('_', underscore)
            .replace('-', dash);
        let key = letters(&spoken);
        let n = spoken.split_whitespace().count();
        for len in n.saturating_sub(1).max(1)..=n + 1 {
            for run in words.windows(len) {
                let heard = letters(&run.join(" "));
                let d = crate::dictionary::fuzzy::plain_spelling_distance(&heard, &key);
                if d <= 0.12 && best.is_none_or(|b| d < b) {
                    best = Some(d);
                }
            }
        }
    }
    best
}

/// Lower-case letters and digits only.
fn letters(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// The spellings that have the final say (the dictionary's — every way a
/// word is heard too —, your name, memory's names), as
/// [`fuzzy::ngram_key`]s: never changed by the screen's pass, nor named to
/// cleanup a second time.
pub fn protected(dictionary: &[DictionaryEntry], about_name: &str) -> HashSet<String> {
    crate::memory::with_names(dictionary, about_name)
        .iter()
        .filter(|e| !e.snippet)
        .flat_map(|e| {
            std::iter::once(e.to.as_str()).chain(e.heard()).map(crate::dictionary::fuzzy::ngram_key).collect::<Vec<_>>()
        })
        .filter(|k| !k.is_empty())
        .collect()
}

/// How a Whisper prompt takes the names on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(debug_assertions), allow(dead_code))] // the others only to compare, in robot mode
pub enum WhisperNames {
    /// After the dictionary's words, as one list ("a, b, c").
    List,
    /// The same, ending with a full stop, as a sentence would.
    Sentence,
    /// Not at all: the near-miss pass alone.
    Off,
}

/// How the Whisper prompt takes the names on screen: **not at all** — the
/// near-miss pass spells them (2026-10-22, real speech through large-v3,
/// e2e-real/screen-names-stt.real.js: with the names in its prompt Whisper
/// heard a spoken "row shell" as "Rochelle" with Rochelle on screen, as a
/// list it dropped full stops and wrote "Zen Tricks"; the pass alone got
/// every name the prompt did, "Siobhan Nibrian" → "Siobhan Ní Bhriain"
/// included, and changed no everyday sentence). Robot mode may say
/// otherwise (`YAP_REALTEST_SCREEN_WHISPER` = list | sentence | off), to
/// measure again.
fn whisper_names() -> WhisperNames {
    #[cfg(debug_assertions)]
    if crate::shell::realtest::on() {
        match std::env::var("YAP_REALTEST_SCREEN_WHISPER").as_deref() {
            Ok("list") => return WhisperNames::List,
            Ok("sentence") => return WhisperNames::Sentence,
            Ok("off") => return WhisperNames::Off,
            _ => {}
        }
    }
    WhisperNames::Off
}

/// A dictation's Whisper prompt (`config::dictionary_prompt`): `words` —
/// the dictionary's, then memory's names, ranked — then the names on
/// screen, if they've been read ([`whisper_names`] says how). `None` with
/// nothing to bias towards.
pub fn whisper_prompt(words: Vec<DictionaryEntry>, screen: Option<&ScreenTerms>) -> Option<String> {
    whisper_prompt_as(words, screen, whisper_names())
}

/// [`whisper_prompt`] with the names on screen taken `how`. Pure.
fn whisper_prompt_as(mut words: Vec<DictionaryEntry>, screen: Option<&ScreenTerms>, how: WhisperNames) -> Option<String> {
    let added = match screen {
        Some(s) if how != WhisperNames::Off => {
            let more = s.prompt_entries(&words);
            let n = more.len();
            words.extend(more);
            n
        }
        _ => 0,
    };
    let prompt = crate::config::dictionary_prompt(&words)?;
    Some(if added > 0 && how == WhisperNames::Sentence && !prompt.ends_with('.') { format!("{prompt}.") } else { prompt })
}

/// The near-miss pass over a dictation's corrected text, as the settings
/// have it: "Spell names the way they're written on screen" and "Catch
/// near-misses" on — for every model (Whisper writes "Siobhan Nibrian" for
/// a name the screen spells "Siobhan Ní Bhriain").
pub fn correct(text: &str, screen: Option<&ScreenTerms>, cfg: &YapConfig) -> String {
    let Some(screen) = screen else {
        return text.to_string();
    };
    if !cfg.screen_names {
        return text.to_string();
    }
    // In an editor or a terminal: file names and symbols said aloud, as the
    // code writes them (`code.rs`; a letter off only with "Catch near-misses").
    let respelled;
    let text = match &screen.code {
        Some(code) => {
            respelled = code.respell(text, cfg.dictionary_fuzzy);
            if respelled != text {
                tracing::debug!("Wrote a file or code name said aloud as the code has it");
            }
            respelled.as_str()
        }
        None => text,
    };
    if !cfg.dictionary_fuzzy {
        return text.to_string();
    }
    if screen.phrases.is_empty() && screen.words.is_empty() {
        return text.to_string();
    }
    let keep = protected(&cfg.dictionary, &cfg.about_name);
    let words = crate::memory::Words::new();
    let out = screen.correct(text, &|k| keep.contains(k), &|w| words.is(w));
    if out != text {
        tracing::debug!("Spelled a name as the screen shows it");
    }
    out
}

/// AI cleanup's line for this dictation's transcript (`raw`), if the
/// screen showed terms it seems to contain.
pub fn cleanup_line(screen: Option<&ScreenTerms>, raw: &str, dictionary: &[DictionaryEntry], about_name: &str) -> Option<String> {
    let screen = screen?;
    let keep = protected(dictionary, about_name);
    let line = screen.cleanup_line(raw, &|k| keep.contains(k));
    if let Some(l) = &line {
        tracing::debug!(terms = l.matches(", ").count() + 1, "AI cleanup is told how names on screen are spelled");
    }
    line
}

// ---- the read, one per dictation ---------------------------------------------------------------

/// Where the current dictation's read stands.
enum Reading {
    /// Nothing asked for (the setting off, edit mode, Yap's own window…).
    Off,
    /// Being read in the background.
    Pending,
    /// Read: `None` when nothing could be read.
    Done(Option<Arc<ScreenTerms>>),
}

struct Session {
    id: u64,
    reading: Reading,
}

static SESSION: Mutex<Session> = Mutex::new(Session { id: 0, reading: Reading::Off });
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

fn session() -> std::sync::MutexGuard<'static, Session> {
    SESSION.lock().unwrap_or_else(|p| p.into_inner())
}

/// A dictation started recording into `target_hwnd` (`None`: one of Yap's
/// own windows). With `read`, the window's visible text is read on a
/// background thread and its terms kept for this dictation ([`peek`],
/// [`take`]); either way the previous dictation's are forgotten. Never in a
/// test run — in robot mode only a window the test opened.
pub fn begin_dictation(target_hwnd: Option<isize>, read: bool) {
    let id = NEXT_SESSION.fetch_add(1, Ordering::SeqCst);
    let hwnd = target_hwnd.filter(|&h| read && crate::shell::realtest::permits(Some(h)));
    {
        let mut s = session();
        s.id = id;
        s.reading = if hwnd.is_some() { Reading::Pending } else { Reading::Off };
        // A test run reads no window: what a spec staged stands in for it.
        #[cfg(debug_assertions)]
        if hwnd.is_none() && read && crate::shell::e2e::active() && !crate::shell::realtest::on() {
            if let Some(staged) = TEST_SCREEN.lock().unwrap_or_else(|p| p.into_inner()).clone() {
                s.reading = Reading::Done(Some(staged));
            }
        }
    }
    let Some(hwnd) = hwnd else {
        return;
    };
    let spawned = std::thread::Builder::new().name("screen-terms".into()).spawn(move || {
        let terms = read_terms(hwnd).map(Arc::new);
        let mut s = session();
        if s.id == id {
            s.reading = Reading::Done(terms);
        }
    });
    if spawned.is_err() {
        session().reading = Reading::Off;
    }
}

/// The terms read for the dictation now, if the read is done (the
/// transcription never waits for it), left for [`take`].
pub fn peek() -> Option<Arc<ScreenTerms>> {
    match &session().reading {
        Reading::Done(terms) => terms.clone(),
        Reading::Pending => {
            tracing::debug!("The screen's names are still being read; transcribing without them");
            None
        }
        Reading::Off => None,
    }
}

/// The terms read for the dictation being delivered, if the read is done —
/// taken: they're used once and forgotten.
pub fn take() -> Option<Arc<ScreenTerms>> {
    let mut s = session();
    // A read that finishes later is dropped, not kept for nobody.
    s.id = NEXT_SESSION.fetch_add(1, Ordering::SeqCst);
    match std::mem::replace(&mut s.reading, Reading::Off) {
        Reading::Done(terms) => terms,
        Reading::Pending => {
            tracing::debug!("The screen's names are still being read; pasting without them");
            None
        }
        Reading::Off => None,
    }
}

/// A dictation ended without being delivered (cancelled): its terms go.
pub fn forget() {
    let mut s = session();
    s.id = NEXT_SESSION.fetch_add(1, Ordering::SeqCst);
    s.reading = Reading::Off;
}

/// Test mode's stand-in for a window's text ([`stage_for_test`]).
#[cfg(debug_assertions)]
static TEST_SCREEN: Mutex<Option<Arc<ScreenTerms>>> = Mutex::new(None);

/// Test mode only (`compose::e2e_chat_target`): from now on every dictation
/// "reads" `text` as its window's (in code app `code`, the focus labelled
/// `focus`), its names and code names found as a real read's are; `None`:
/// nothing (as before).
#[cfg(debug_assertions)]
pub fn stage_for_test(text: Option<&str>, code_app: Option<code::App>, focus: Option<code::Labels>) {
    let staged = text.map(|t| {
        let ordinary = crate::dictionary::learn::Ordinary::new();
        let is_word = |w: &str| ordinary.is(w);
        let read = WindowText { text: t.to_string(), focus, ..WindowText::default() };
        Arc::new(terms_of(&read, code_app, &is_word, ordinary.has_spell_checker()))
    });
    *TEST_SCREEN.lock().unwrap_or_else(|p| p.into_inner()) = staged;
}

/// Robot mode: wait (at most `max`) for the read [`begin_dictation`]
/// started, as the seconds of a spoken dictation would.
#[cfg(debug_assertions)]
pub fn wait_for_terms(max: Duration) {
    let until = Instant::now() + max;
    while matches!(session().reading, Reading::Pending) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Read `hwnd`'s visible text and find its terms (on this thread). `None`
/// for Yap's own window, a password manager's, a window that's gone, or
/// nothing read.
fn read_terms(hwnd: isize) -> Option<ScreenTerms> {
    let started = Instant::now();
    // Robot mode: a read that takes its time (`YAP_REALTEST_SCREEN_DELAY_MS`),
    // to show a dictation never waits for one.
    #[cfg(debug_assertions)]
    if crate::shell::realtest::on() {
        if let Some(ms) = std::env::var("YAP_REALTEST_SCREEN_DELAY_MS").ok().and_then(|v| v.trim().parse::<u64>().ok()) {
            std::thread::sleep(Duration::from_millis(ms));
        }
    }
    let pid = crate::pasting::text_injector::window_process(hwnd)?;
    if pid == std::process::id() {
        return None;
    }
    let exe = crate::pasting::text_injector::app_name_for(Some(hwnd)).unwrap_or_default().to_lowercase();
    let exe = exe.trim_end_matches(".exe");
    if NEVER_READ.iter().any(|p| exe.contains(p)) {
        tracing::debug!("A password manager's window: not reading its names");
        return None;
    }
    // An editor or a terminal: its code names too (`code.rs`), and an
    // editor's tabs and file tree asked for.
    let code_app = code::app_of(exe, crate::pasting::app_kind::of_window(Some(hwnd)));
    let read = read_window(hwnd, pid, started + READ_BUDGET, code_app.is_some_and(code::App::is_editor))?;
    let read_ms = started.elapsed().as_millis() as u64;
    let ordinary = crate::dictionary::learn::Ordinary::new();
    let spell_checker = ordinary.has_spell_checker();
    let is_word = |w: &str| ordinary.is(w);
    let screen = terms_of(&read, code_app, &is_word, spell_checker);
    tracing::debug!(
        read_ms,
        ms = started.elapsed().as_millis() as u64,
        chars = read.text.chars().count(),
        document = read.document,
        elements = read.elements,
        items = read.items,
        terms = screen.terms.len(),
        targets = screen.phrases.len() + screen.words.len(),
        "Read the names on screen"
    );
    if let Some(c) = &screen.code {
        tracing::debug!(
            app = c.app.name(),
            focus = ?c.focus,
            files = c.files.len(),
            symbols = c.symbols.len(),
            "Code names on screen"
        );
    }
    Some(screen)
}

/// The names in a window's text as read (`read`), and in a code app its
/// code names with where the focus is. Pure but for `is_word`.
fn terms_of(read: &WindowText, code_app: Option<code::App>, is_word: &dyn Fn(&str) -> bool, spell_checker: bool) -> ScreenTerms {
    let terms = extract::extract(&read.text, is_word, spell_checker);
    let mut screen = ScreenTerms::new(terms, is_word, spell_checker);
    screen.code = code_app.map(|app| {
        let focus = match &read.focus {
            Some(labels) => code::focus_of(app, labels),
            None if matches!(app, code::App::Terminal(_)) => code::Focus::Terminal,
            None => code::Focus::Other,
        };
        code::CodeNames::new(app, focus, &read.text, is_word)
    });
    screen
}

/// A window's visible text, as read.
#[derive(Debug, Default)]
pub struct WindowText {
    pub text: String,
    /// Characters from the focused document's visible ranges.
    pub document: usize,
    /// Text elements read besides it.
    pub elements: usize,
    /// An editor's tabs and tree items read (`code.rs`).
    pub items: usize,
    /// The focused element's labels (never logged).
    pub focus: Option<code::Labels>,
}

impl WindowText {
    #[cfg_attr(not(windows), allow(dead_code))]
    fn room(&self) -> usize {
        MAX_CHARS.saturating_sub(self.text.chars().count())
    }

    /// Add a piece of text (a line break after it), cut to what room is left.
    #[cfg_attr(not(windows), allow(dead_code))]
    fn push(&mut self, piece: &str) -> usize {
        let piece = piece.trim();
        let room = self.room();
        if piece.is_empty() || room == 0 {
            return 0;
        }
        let cut: String = piece.chars().take(room).collect();
        let n = cut.chars().count();
        self.text.push_str(&cut);
        self.text.push('\n');
        n
    }
}

/// Read `hwnd`'s visible text over UI Automation, until `until` at most
/// (`editor`: a code editor's, whose tabs and file tree are asked for too).
fn read_window(hwnd: isize, pid: u32, until: Instant, editor: bool) -> Option<WindowText> {
    #[cfg(windows)]
    {
        uia::read(hwnd, pid, until, editor)
    }
    #[cfg(not(windows))]
    {
        let _ = (hwnd, pid, until, editor);
        None
    }
}

#[cfg(windows)]
mod uia {
    use std::collections::VecDeque;
    use std::time::Instant;

    use windows::core::Interface;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Variant::{VARIANT, VT_I4};
    use windows::Win32::UI::Accessibility::{
        IUIAutomation, IUIAutomationCacheRequest, IUIAutomationElement, IUIAutomationTextPattern,
        TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start, TextUnit_Character, TreeScope_Children,
        TreeScope_Descendants, TreeScope_Element, UIA_AutomationIdPropertyId, UIA_ButtonControlTypeId,
        UIA_ClassNamePropertyId, UIA_ControlTypePropertyId, UIA_DataItemControlTypeId, UIA_DocumentControlTypeId,
        UIA_EditControlTypeId, UIA_HeaderItemControlTypeId, UIA_HelpTextPropertyId, UIA_HyperlinkControlTypeId,
        UIA_IsOffscreenPropertyId, UIA_IsPasswordPropertyId, UIA_IsTextPatternAvailablePropertyId,
        UIA_ListItemControlTypeId, UIA_NamePropertyId, UIA_ProcessIdPropertyId, UIA_TabItemControlTypeId,
        UIA_TextControlTypeId, UIA_TextPatternId, UIA_TitleBarControlTypeId, UIA_TreeItemControlTypeId,
        UIA_ValueValuePropertyId, UIA_CONTROLTYPE_ID,
    };

    use super::WindowText;
    use crate::pasting::selection::uia::{automation, Com};

    /// How far up from the focus Yap looks for the document it's in.
    const DOCUMENT_LEVELS: usize = 16;
    /// The focused document's text at most (the rest of the room is for the
    /// window's other text).
    const DOCUMENT_CHARS: usize = 12_000;
    /// How far either side of the focus a web page is read.
    const AROUND_CHARS: usize = 6_000;
    /// A web page read around the focus with at least this much text: the
    /// rest of the browser's window (its tabs, its toolbar) isn't walked.
    const ENOUGH_OF_A_PAGE: usize = 500;
    /// The walk over the window's elements: at most this many asked for
    /// (one call a parent, all its children at once)…
    const WALK_CALLS: usize = 250;
    /// …and this many elements looked at.
    const WALK_ELEMENTS: usize = 2_500;
    /// Text of a document or a text box other than the focused one, at most.
    const OTHER_DOCUMENT_CHARS: usize = 2_000;
    /// An editor's tabs and tree items read, at most.
    const ITEMS: usize = 400;

    /// See [`super::read_window`]: the window's title; the document the
    /// focus is in — its visible ranges, or around the focus for a web page
    /// (Chromium works out a page's visible ranges on its UI thread: seconds
    /// on a long page, while reading around the focus takes milliseconds
    /// whatever the page's size); then a bounded walk over the window's
    /// visible elements. All of process `pid`, never a password box.
    pub fn read(hwnd: isize, pid: u32, until: Instant, editor: bool) -> Option<WindowText> {
        let _com = Com::init()?;
        let automation = automation(super::CALL_TIMEOUT_MS)?;
        let mut out = WindowText::default();
        unsafe {
            let root = automation.ElementFromHandle(HWND(hwnd as *mut core::ffi::c_void)).ok()?;
            if root.CurrentProcessId().ok()? as u32 != pid {
                return None;
            }
            if let Ok(name) = root.CurrentName() {
                out.push(&name.to_string());
            }
            out.focus = focus_labels(&automation, pid);
            // A code editor's open files (its tabs) and file tree, in one
            // call: a walk from the window's top may not get that deep.
            if editor && Instant::now() < until {
                out.items = named_items(&automation, &root, pid, &mut out);
            }
            let browser = root.CurrentClassName().map(|c| c.to_string().starts_with("Chrome_WidgetWin")).unwrap_or(false);
            let mut web_page = browser;
            if let Some((doc, pattern)) = focused_document(&automation, pid) {
                web_page |= doc.CurrentFrameworkId().is_ok_and(|f| f == "Chrome");
                out.document = if web_page {
                    around_focus(&automation, &pattern, &mut out, until)
                } else {
                    match visible_text(&pattern, &mut out, until) {
                        0 => around_focus(&automation, &pattern, &mut out, until),
                        n => n,
                    }
                };
            }
            if Instant::now() < until && out.room() > 0 && !(web_page && out.document >= ENOUGH_OF_A_PAGE) {
                out.elements = walk(&automation, &root, pid, web_page, &mut out, until);
            }
        }
        Some(out)
    }

    /// The focused element's labels, if it's in `pid`'s process and not a
    /// password box (`code::focus_of`: the editor, its AI chat box or its
    /// terminal). One round trip.
    unsafe fn focus_labels(automation: &IUIAutomation, pid: u32) -> Option<super::code::Labels> {
        let cache = automation.CreateCacheRequest().ok()?;
        for p in [
            UIA_ProcessIdPropertyId,
            UIA_IsPasswordPropertyId,
            UIA_ClassNamePropertyId,
            UIA_NamePropertyId,
            UIA_HelpTextPropertyId,
            UIA_AutomationIdPropertyId,
        ] {
            cache.AddProperty(p).ok()?;
        }
        let e = automation.GetFocusedElementBuildCache(&cache).ok()?;
        if e.CachedProcessId().ok()? as u32 != pid || e.CachedIsPassword().map(|b| b.as_bool()).unwrap_or(true) {
            return None;
        }
        let s = |v: windows::core::Result<windows::core::BSTR>| v.map(|b| b.to_string()).unwrap_or_default();
        Some(super::code::Labels {
            class: s(e.CachedClassName()),
            name: s(e.CachedName()),
            help: s(e.CachedHelpText()),
            id: s(e.CachedAutomationId()),
        })
    }

    /// The names of a window's tabs and tree items on screen (an editor's
    /// open files and file tree), in one `FindAllBuildCache` — time-boxed
    /// like every call ([`super::CALL_TIMEOUT_MS`]); nothing when it runs
    /// out. At most [`ITEMS`]. The items read.
    unsafe fn named_items(automation: &IUIAutomation, root: &IUIAutomationElement, pid: u32, out: &mut WindowText) -> usize {
        let Ok(cache) = automation.CreateCacheRequest() else { return 0 };
        for p in [UIA_NamePropertyId, UIA_ProcessIdPropertyId, UIA_IsOffscreenPropertyId] {
            if cache.AddProperty(p).is_err() {
                return 0;
            }
        }
        // A control type as the VARIANT a condition compares (VT_I4).
        let kind = |t: UIA_CONTROLTYPE_ID| {
            let mut v = VARIANT::default();
            (*v.Anonymous.Anonymous).vt = VT_I4;
            (*v.Anonymous.Anonymous).Anonymous.lVal = t.0;
            automation.CreatePropertyCondition(UIA_ControlTypePropertyId, &v).ok()
        };
        let (Some(tab), Some(tree)) = (kind(UIA_TabItemControlTypeId), kind(UIA_TreeItemControlTypeId)) else { return 0 };
        let Ok(condition) = automation.CreateOrCondition(&tab, &tree) else { return 0 };
        let Ok(found) = root.FindAllBuildCache(TreeScope_Descendants, &condition, &cache) else {
            tracing::debug!("An editor's tabs and file tree didn't come in time");
            return 0;
        };
        let mut read = 0;
        for i in 0..found.Length().unwrap_or(0).min(ITEMS as i32) {
            let Ok(e) = found.GetElement(i) else { continue };
            if e.CachedProcessId().map(|p| p as u32) != Ok(pid) || e.CachedIsOffscreen().map(|b| b.as_bool()).unwrap_or(true) {
                continue;
            }
            if out.push(&e.CachedName().map(|b| b.to_string()).unwrap_or_default()) > 0 {
                read += 1;
            }
        }
        read
    }

    /// The outermost element with a TextPattern above (or at) the focus, in
    /// `pid`'s process: a web page's document, an editor, a mail being
    /// written. Never a password box.
    unsafe fn focused_document(
        automation: &IUIAutomation,
        pid: u32,
    ) -> Option<(IUIAutomationElement, IUIAutomationTextPattern)> {
        let focused = automation.GetFocusedElement().ok()?;
        if focused.CurrentProcessId().ok()? as u32 != pid || focused.CurrentIsPassword().ok()?.as_bool() {
            return None;
        }
        let walker = automation.ControlViewWalker().ok()?;
        let mut found: Option<(IUIAutomationElement, IUIAutomationTextPattern, bool)> = None;
        let mut e = focused;
        for _ in 0..DOCUMENT_LEVELS {
            if e.CurrentProcessId().map(|p| p as u32) != Ok(pid) {
                break;
            }
            let pattern = e
                .GetCurrentPattern(UIA_TextPatternId)
                .ok()
                .and_then(|u| u.cast::<IUIAutomationTextPattern>().ok());
            if let Some(pattern) = pattern {
                if !e.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(true) {
                    let document = e.CurrentControlType().is_ok_and(|k| k == UIA_DocumentControlTypeId);
                    // A document wins over the field inside it (Chromium's
                    // page over its text box); the outermost document over
                    // an inner one.
                    if found.as_ref().is_none_or(|(_, _, d)| document || !d) {
                        found = Some((e.clone(), pattern, document));
                    }
                }
            }
            match walker.GetParentElement(&e) {
                Ok(parent) => e = parent,
                Err(_) => break,
            }
        }
        found.map(|(e, p, _)| (e, p))
    }

    /// The text either side of the focus in the document `pattern` (at most
    /// [`AROUND_CHARS`] each way, then [`DOCUMENT_CHARS`] around the focus):
    /// the thread above a chat's message box, the mail a reply quotes, the
    /// page around a text box. Its cost is what it reads, whatever the
    /// page's size.
    unsafe fn around_focus(
        automation: &IUIAutomation,
        pattern: &IUIAutomationTextPattern,
        out: &mut WindowText,
        until: Instant,
    ) -> usize {
        let range = match pattern.GetSelection() {
            Ok(ranges) if ranges.Length().unwrap_or(0) > 0 => ranges.GetElement(0).ok(),
            _ => None,
        }
        .or_else(|| {
            let focused = automation.GetFocusedElement().ok()?;
            pattern.RangeFromChild(&focused).ok()
        });
        let Some(range) = range else {
            return 0;
        };
        if Instant::now() >= until {
            return 0;
        }
        let _ = range.MoveEndpointByUnit(TextPatternRangeEndpoint_Start, TextUnit_Character, -(AROUND_CHARS as i32));
        let _ = range.MoveEndpointByUnit(TextPatternRangeEndpoint_End, TextUnit_Character, AROUND_CHARS as i32);
        let want = DOCUMENT_CHARS.min(out.room());
        if want == 0 || Instant::now() >= until {
            return 0;
        }
        match range.GetText((AROUND_CHARS * 2 + 1_000) as i32) {
            Ok(text) => {
                let text = text.to_string();
                let chars = text.chars().count();
                // More than there's room for: the middle, around the focus.
                let text: String =
                    if chars > want { text.chars().skip((chars - want) / 2).take(want).collect() } else { text };
                out.push(&text)
            }
            Err(_) => 0,
        }
    }

    /// The visible ranges of the document `pattern` into `out` (at most
    /// [`DOCUMENT_CHARS`]); the characters read.
    unsafe fn visible_text(pattern: &IUIAutomationTextPattern, out: &mut WindowText, until: Instant) -> usize {
        let Ok(ranges) = pattern.GetVisibleRanges() else {
            return 0;
        };
        let mut read = 0usize;
        for i in 0..ranges.Length().unwrap_or(0) {
            if Instant::now() >= until || read >= DOCUMENT_CHARS {
                break;
            }
            let Ok(range) = ranges.GetElement(i) else { continue };
            let want = (DOCUMENT_CHARS - read).min(out.room());
            if want == 0 {
                break;
            }
            if let Ok(text) = range.GetText(want as i32) {
                read += out.push(&text.to_string());
            }
        }
        read
    }

    /// What the walk asks of every element, in the one call for its
    /// parent's children.
    unsafe fn cache(automation: &IUIAutomation) -> Option<IUIAutomationCacheRequest> {
        let cache = automation.CreateCacheRequest().ok()?;
        for p in [
            UIA_NamePropertyId,
            UIA_ControlTypePropertyId,
            UIA_ValueValuePropertyId,
            UIA_IsPasswordPropertyId,
            UIA_IsOffscreenPropertyId,
            UIA_ProcessIdPropertyId,
            UIA_IsTextPatternAvailablePropertyId,
        ] {
            cache.AddProperty(p).ok()?;
        }
        cache.SetTreeScope(TreeScope_Element).ok()?;
        Some(cache)
    }

    /// Elements whose name is text someone reads (a label, a link, a list
    /// or tree item, a tab, a header, a button, a cell, a title).
    const NAMED: [UIA_CONTROLTYPE_ID; 11] = [
        UIA_TextControlTypeId,
        UIA_HyperlinkControlTypeId,
        UIA_ListItemControlTypeId,
        UIA_TreeItemControlTypeId,
        UIA_DataItemControlTypeId,
        UIA_TabItemControlTypeId,
        UIA_HeaderItemControlTypeId,
        UIA_ButtonControlTypeId,
        UIA_TitleBarControlTypeId,
        UIA_EditControlTypeId,
        UIA_DocumentControlTypeId,
    ];

    /// A breadth-first walk over the window's visible elements (the control
    /// view), one call a parent for all its children with what's needed
    /// cached — at most [`WALK_CALLS`] calls and [`WALK_ELEMENTS`] elements,
    /// within the read's time: never the whole tree of a big app. An element
    /// off screen is skipped with all it holds; a password box, and another
    /// process's elements, are never read. A text box's or a document's text
    /// (not the focused one's, already read) is read up to
    /// [`OTHER_DOCUMENT_CHARS`]; a web page's (`web_page`) never — its text
    /// came around the focus. The elements read.
    unsafe fn walk(
        automation: &IUIAutomation,
        root: &IUIAutomationElement,
        pid: u32,
        web_page: bool,
        out: &mut WindowText,
        until: Instant,
    ) -> usize {
        let Some(cache) = cache(automation) else {
            return 0;
        };
        let Ok(condition) = automation.ControlViewCondition() else {
            return 0;
        };
        let focused = automation.GetFocusedElement().ok();
        let mut queue: VecDeque<IUIAutomationElement> = VecDeque::from([root.clone()]);
        let (mut calls, mut seen, mut read) = (0usize, 0usize, 0usize);
        while let Some(parent) = queue.pop_front() {
            if calls >= WALK_CALLS || seen >= WALK_ELEMENTS || out.room() == 0 || Instant::now() >= until {
                break;
            }
            calls += 1;
            let Ok(children) = parent.FindAllBuildCache(TreeScope_Children, &condition, &cache) else {
                continue;
            };
            for i in 0..children.Length().unwrap_or(0) {
                seen += 1;
                let Ok(e) = children.GetElement(i) else { continue };
                if e.CachedProcessId().map(|p| p as u32) != Ok(pid)
                    || e.CachedIsOffscreen().map(|b| b.as_bool()).unwrap_or(true)
                    || e.CachedIsPassword().map(|b| b.as_bool()).unwrap_or(true)
                {
                    continue;
                }
                let kind = e.CachedControlType().unwrap_or_default();
                let text_box = kind == UIA_EditControlTypeId || kind == UIA_DocumentControlTypeId;
                let mut got = 0;
                if NAMED.contains(&kind) {
                    got += out.push(&e.CachedName().map(|b| b.to_string()).unwrap_or_default());
                }
                if text_box && !web_page {
                    let is_focused = focused.as_ref().is_some_and(|f| automation.CompareElements(f, &e).is_ok_and(|b| b.as_bool()));
                    if !is_focused {
                        got += other_text(&e, out);
                    }
                }
                if got > 0 {
                    read += 1;
                }
                // Into containers; a text box's or a document's parts are its
                // text, already read (a web page's: around the focus).
                if !text_box && kind != UIA_TextControlTypeId && kind != UIA_HyperlinkControlTypeId {
                    queue.push_back(e);
                }
            }
        }
        read
    }

    /// A text box's or a document's text that isn't the focused one's: its
    /// TextPattern's start, else its value — at most [`OTHER_DOCUMENT_CHARS`].
    unsafe fn other_text(e: &IUIAutomationElement, out: &mut WindowText) -> usize {
        let want = OTHER_DOCUMENT_CHARS.min(out.room());
        if want == 0 {
            return 0;
        }
        let has_text = e
            .GetCachedPropertyValue(UIA_IsTextPatternAvailablePropertyId)
            .ok()
            .and_then(|v| bool::try_from(&v).ok())
            .unwrap_or(false);
        if has_text {
            if let Some(text) = e
                .GetCurrentPattern(UIA_TextPatternId)
                .ok()
                .and_then(|u| u.cast::<IUIAutomationTextPattern>().ok())
                .and_then(|p| p.DocumentRange().ok())
                .and_then(|r| r.GetText(want as i32).ok())
            {
                return out.push(&text.to_string());
            }
        }
        match e.GetCachedPropertyValue(UIA_ValueValuePropertyId) {
            Ok(v) => match windows::core::BSTR::try_from(&v) {
                Ok(value) => out.push(&value.to_string().chars().take(want).collect::<String>()),
                Err(_) => 0,
            },
            Err(_) => 0,
        }
    }

    /// Each step of a read, timed (the ignored timing test: counts and
    /// milliseconds only). `steps`: "around", "visible", "range", "walk".
    #[cfg(test)]
    pub fn probe(hwnd: isize, timeout_ms: u32, steps: &str) {
        let t = std::time::Instant::now();
        let ms = || t.elapsed().as_millis();
        let later = || std::time::Instant::now() + std::time::Duration::from_secs(10);
        let Some(_com) = Com::init() else { return };
        let Some(automation) = automation(timeout_ms) else { return };
        unsafe {
            let root = automation.ElementFromHandle(HWND(hwnd as *mut core::ffi::c_void));
            println!("  ElementFromHandle: {} ms ok={}", ms(), root.is_ok());
            let Ok(root) = root else { return };
            let Ok(pid) = root.CurrentProcessId() else { return };
            let doc = focused_document(&automation, pid as u32);
            println!("  focused_document: {} ms found={}", ms(), doc.is_some());
            if let Some((doc, p)) = &doc {
                println!(
                    "  framework {:?}",
                    doc.CurrentFrameworkId().map(|f| f.to_string()).unwrap_or_default()
                );
                if steps.contains("range") {
                    let before = ms();
                    let text = p.DocumentRange().and_then(|r| r.GetText(20_000));
                    println!("  DocumentRange.GetText(20000): {} ms, {:?} chars", ms() - before, text.map(|t| t.len()).ok());
                }
                if steps.contains("around") {
                    let before = ms();
                    let n = around_focus(&automation, p, &mut WindowText::default(), later());
                    println!("  around_focus: {} ms, {n} chars", ms() - before);
                }
                if steps.contains("visible") {
                    let before = ms();
                    let n = visible_text(p, &mut WindowText::default(), later());
                    println!("  visible_text: {} ms, {n} chars", ms() - before);
                }
            }
            if steps.contains("walk") {
                let before = ms();
                let mut out = WindowText::default();
                let n = walk(&automation, &root, pid as u32, false, &mut out, later());
                println!("  walk: {} ms, {n} elements read, {} chars", ms() - before, out.text.chars().count());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tests' "spell checker": the everyday list and a few more words.
    fn words(w: &str) -> bool {
        crate::dictionary::learn::is_frequent(w)
            || [
                "row", "shell", "zen", "tricks", "cooper", "netties", "rachel", "breen", "nee", "tricky", "tricks",
                "kindle", "gross", "oss", "boss",
            ]
            .contains(&w)
    }

    fn screen(text: &str) -> ScreenTerms {
        ScreenTerms::new(extract::extract(text, &words, true), &words, true)
    }

    const SCREEN: &str = "Re: Zentrix launch with Siobhan Ní Bhriain\n\
        Rochelle asked whether Kubernetes can run GPT-OSS for the Zentrix pilot. \
        Ping @zentrix_dev when Rochelle signs off.";

    #[test]
    fn a_name_heard_a_little_wrong_is_spelled_as_on_screen() {
        let s = screen(SCREEN);
        let keep = |_: &str| false;
        let fix = |t: &str| s.correct(t, &keep, &words);
        assert_eq!(fix("I spoke to Rachelle about it."), "I spoke to Rochelle about it.");
        assert_eq!(fix("The Zentrics pilot starts Monday."), "The Zentrix pilot starts Monday.");
        assert_eq!(fix("Ask Shiobhan first."), "Ask Siobhan first.");
        assert_eq!(fix("Deploy it to kubernetes."), "Deploy it to Kubernetes.");
        assert_eq!(fix("Run gpt-oss locally."), "Run GPT-OSS locally.");
        assert_eq!(fix("Siobhan Ni Bhriain said yes."), "Siobhan Ní Bhriain said yes.");
    }

    #[test]
    fn the_words_as_said_get_the_names_on_screen_too_but_no_cleanup() {
        // "Use what I said" (pipeline::as_said_text): the transcript through
        // the corrections and the names this dictation read, nothing else.
        let s = screen(SCREEN);
        let cfg = crate::config::YapConfig::default();
        let heard = "The Zentrics pilot starts Monday";
        assert_eq!(crate::dictation::pipeline::as_said_text(heard, &cfg, None), heard);
        let said = crate::dictation::pipeline::as_said_text(heard, &cfg, Some(&s));
        // (Names are near-miss targets only with a spell checker to tell
        // them from words, as on every Windows PC.)
        if crate::dictionary::learn::Ordinary::new().has_spell_checker() {
            assert_eq!(said, "The Zentrix pilot starts Monday");
        } else {
            assert_eq!(said, heard);
        }
        // The setting off: as heard.
        let off = crate::config::YapConfig { screen_names: false, ..cfg };
        assert_eq!(crate::dictation::pipeline::as_said_text(heard, &off, Some(&s)), heard);
    }

    /// A full name on screen never swallows the everyday word said after
    /// its first name ("Priyanka can you…" isn't "Priyanka Raman you…"; the
    /// robot's chat stand-in, 2026-10-29). Its last name heard a little
    /// wrong is still spelled as shown.
    #[test]
    fn a_full_name_never_takes_the_word_after_a_first_name() {
        let s = screen("Members: Priyanka Raman, Rochelle Okafor");
        let keep = |_: &str| false;
        assert_eq!(s.correct("Priyanka can you check the build?", &keep, &words), "Priyanka can you check the build?");
        assert_eq!(s.correct("Rochelle can you look?", &keep, &words), "Rochelle can you look?");
        assert_eq!(s.correct("Ask Priyanka Ramen about it.", &keep, &words), "Ask Priyanka Raman about it.");
    }

    #[test]
    fn an_email_keeps_its_lines() {
        // The robot's Gmail page (2026-10-24): any term on screen and an
        // email's greeting lost its own line ("Hi Sam, Thank you…").
        let s = screen(SCREEN);
        let keep = |_: &str| false;
        let email = "Hi Sam,\n\nThank you for the figures.\n\nBest regards,\nAlex";
        assert_eq!(s.correct(email, &keep, &words), email);
        assert_eq!(
            s.correct("Hi Sam,\n\nI spoke to Rachelle about it.", &keep, &words),
            "Hi Sam,\n\nI spoke to Rochelle about it."
        );
    }

    #[test]
    fn ordinary_words_near_a_name_are_never_changed() {
        let s = screen(SCREEN);
        let keep = |_: &str| false;
        for said in [
            "We took the row shell out on the river.",
            "I learned some zen tricks.",
            "Rachel is coming too.",
            "The cooper netties were full.",
            "That's a tricky one.",
            "The boss said no.",
            // A speech model's garble of "row shell": no name (no capital).
            "We took the rouchell out on the river.",
            // Nor a term heard in lower case: only its own letters are fixed.
            "the zentrics pilot",
        ] {
            assert_eq!(s.correct(said, &keep, &words), said);
        }
    }

    #[test]
    fn the_dictionary_and_memory_have_the_final_say() {
        // The screen says "Zentrix"; the dictionary spells it "ZenTrix".
        let s = screen(SCREEN);
        let dict = [DictionaryEntry {
            from: "zen tricks".into(),
            to: "ZenTrix".into(),
            fuzzy: true,
            snippet: false,
            starred: false,
            also_heard: Vec::new(),
        }];
        let keep: HashSet<String> = dict.iter().flat_map(|e| [crate::dictionary::fuzzy::ngram_key(&e.to), crate::dictionary::fuzzy::ngram_key(&e.from)]).collect();
        assert_eq!(s.correct("The ZenTrix pilot.", &|k| keep.contains(k), &words), "The ZenTrix pilot.");
        assert!(!s.in_transcript("the zentrix pilot", &|k| keep.contains(k)).contains(&"Zentrix"));
    }

    #[test]
    fn cleanup_hears_only_the_names_the_transcript_seems_to_hold() {
        let s = screen(SCREEN);
        let none = |_: &str| false;
        let line = s.cleanup_line("so the zentrics pilot is on", &none);
        assert_eq!(line.as_deref(), Some("Spell these names as written: Zentrix."));
        let found = s.in_transcript("so i told shiobhan about the zentrics pilot", &none);
        assert!(found.contains(&"Zentrix"), "{found:?}");
        assert!(found.contains(&"Siobhan"), "{found:?}");
        // Spelled too far from it to tell ("Siobhan" said as it sounds,
        // as Parakeet writes it): a gap.
        assert!(!s.in_transcript("so i told shivon", &none).contains(&"Siobhan"));
        assert!(!found.contains(&"Kubernetes") && !found.contains(&"Rochelle"), "{found:?}");
        // The exact spelling heard counts too (keeps cleanup from changing it).
        assert!(s.in_transcript("Kubernetes is fine", &none).contains(&"Kubernetes"));
        // Nothing on screen in it: no line.
        assert_eq!(s.cleanup_line("see you at the station at noon", &none), None);
        // A handle as it's said; its name alone is the product.
        assert!(s.in_transcript("ping at zentrix dev about it", &none).contains(&"@zentrix_dev"));
        assert!(s.in_transcript("ping at zentrix underscore dev", &none).contains(&"@zentrix_dev"));
        assert!(!s.in_transcript("ping the zentrix dev team", &none).contains(&"@zentrix_dev"));
    }

    /// An editor's window: its file names and symbols said aloud are
    /// written as the code has them (the setting on), and cleanup is told
    /// them first.
    #[test]
    fn an_editors_code_names_said_aloud() {
        let screen = "userService.ts - Yap - Visual Studio Code\nuserService.ts, preview\n\
            export function getUserById(id) { return MAX_RETRY_COUNT; }";
        let read = WindowText { text: screen.into(), ..WindowText::default() };
        let s = terms_of(&read, Some(code::App::VsCode), &words, true);
        let c = s.code.as_ref().expect("code names");
        assert_eq!(c.focus, code::Focus::Other);
        assert!(c.files.iter().any(|f| f.name == "userService.ts"));
        let cfg = crate::config::YapConfig::default();
        assert_eq!(correct("call get user by id in user service dot ts", Some(&s), &cfg), "call getUserById in userService.ts");
        // Off: as said.
        let off = crate::config::YapConfig { screen_names: false, ..cfg };
        assert_eq!(correct("call get user by id", Some(&s), &off), "call get user by id");
        let line = s.cleanup_line("fix user service dot ts", &|_| false).unwrap();
        assert!(line.starts_with("Spell these names as written: userService.ts"), "{line}");
        // Not a code app: no code names.
        assert!(terms_of(&read, None, &words, true).code.is_none());
    }

    #[test]
    fn the_whisper_prompt_takes_the_screens_names_only_when_asked() {
        let s = screen(SCREEN);
        let dict = vec![entry("Grafana")];
        // Yap's way: not in the prompt (the near-miss pass spells them).
        assert_eq!(whisper_prompt(dict.clone(), Some(&s)).as_deref(), Some("Grafana"));
        // To compare (robot mode): after the dictionary's words, as a list
        // or ending as a sentence; never a handle.
        let p = whisper_prompt_as(dict.clone(), Some(&s), WhisperNames::List).unwrap();
        assert!(p.starts_with("Grafana, Zentrix, ") && !p.ends_with('.'), "{p}");
        assert!(p.contains("Siobhan Ní Bhriain") && !p.contains('@'), "{p}");
        let sentence = whisper_prompt_as(dict.clone(), Some(&s), WhisperNames::Sentence).unwrap();
        assert_eq!(sentence, format!("{p}."));
        // Nothing read: the dictionary's words alone, as before.
        assert_eq!(whisper_prompt_as(dict, None, WhisperNames::Sentence).as_deref(), Some("Grafana"));
        assert_eq!(whisper_prompt(vec![], None), None);
    }

    #[test]
    fn the_whisper_prompt_gets_the_terms_after_the_dictionary() {
        let s = screen(SCREEN);
        let taken = vec![entry("Kubernetes")];
        let got: Vec<String> = s.prompt_entries(&taken).into_iter().map(|e| e.to).collect();
        assert!(got.contains(&"Zentrix".to_string()) && got.contains(&"Siobhan Ní Bhriain".to_string()), "{got:?}");
        assert!(!got.contains(&"Kubernetes".to_string()) && !got.iter().any(|t| t.starts_with('@')), "{got:?}");
        // The most frequent first.
        assert_eq!(got[0], "Zentrix");
    }

    /// The transcription and the paste never wait for the read: still
    /// reading = nothing for them, and the read's late result is dropped.
    #[test]
    fn a_read_still_going_is_never_waited_for() {
        let id = {
            let mut s = session();
            s.id = NEXT_SESSION.fetch_add(1, Ordering::SeqCst);
            s.reading = Reading::Pending;
            s.id
        };
        let started = Instant::now();
        assert!(peek().is_none());
        assert!(take().is_none());
        assert!(started.elapsed() < Duration::from_millis(50));
        // The read finishing now (as its thread does) finds another session.
        assert_ne!(session().id, id);
        // A finished read: the prompt peeks, the paste takes it once.
        {
            let mut s = session();
            s.reading = Reading::Done(Some(Arc::new(screen(SCREEN))));
        }
        assert!(peek().is_some());
        assert!(take().is_some());
        assert!(take().is_none());
        // A cancelled dictation forgets them.
        session().reading = Reading::Done(Some(Arc::new(screen(SCREEN))));
        forget();
        assert!(peek().is_none());
    }

    #[test]
    fn without_a_spell_checker_nothing_is_a_near_miss_target() {
        let s = ScreenTerms::new(extract::extract(SCREEN, &words, false), &words, false);
        assert_eq!(s.correct("The Zentrics pilot.", &|_| false, &words), "The Zentrics pilot.");
        assert!(!s.prompt_entries(&[]).is_empty());
    }

    /// Everyday sentences with a busy screen's terms as near-miss targets
    /// (a code editor's, a mail client's): not one word changes — with this
    /// PC's own spell checker, as a dictation runs it.
    #[test]
    fn no_everyday_sentence_changes_with_a_busy_screen() {
        let ordinary = crate::dictionary::learn::Ordinary::new();
        if !ordinary.has_spell_checker() {
            eprintln!("No spell checker on this PC: the screen's terms are never near-miss targets here.");
            return;
        }
        let is_word = |w: &str| ordinary.is(w);
        let busy = "Inbox (12) - siobhan.nibhriain@example.com - Outlook\n\
            Rochelle Okafor; Zentrix weekly sync; Kubernetes cluster upgrade; GPT-OSS evaluation; \
            Archie Tanaka; Olivia Brennan; Priya Raman; Theo Lindqvist; Imogen Hartley; Callum Fraser; \
            PostgreSQL migration; GitHub Actions; Terraform plan; Grafana dashboards; Kafka consumers; \
            useState setState HashMap TypeScript JavaScript WebSocket OAuth OpenAI Anthropic Qwen3 Llama \
            Figma Notion Jira Confluence Salesforce Shopify Svelte Tauri Vite Redis Nginx Django Flask \
            Lumen Orion Harbour Bloom Atlas Nova Apollo Mercury Zephyr Kestrel Falcon Sparrow Rowan \
            Sasha Kieran Declan Niamh Saoirse Ciara Eoin Ruairi Gareth Rhys Dylan Bethan Ffion Megan";
        let s = ScreenTerms::new(extract::extract(busy, &is_word, true), &is_word, true);
        let targets = s.phrases.len() + s.words.len();
        println!("{} terms, {targets} near-miss targets", s.terms.len());
        assert!(targets > 30, "most of the busy screen's terms are targets");
        let sentences = [
            "Can you send me the report by Friday?",
            "I'll call you back in five minutes.",
            "The shell script failed again last night.",
            "We took the row shell out on the river.",
            "I learned some zen tricks from my yoga teacher.",
            "Pass me the olive oil.",
            "They had to rescue the cat from the tree.",
            "Archive the old files first.",
            "I'm feeling lucky today.",
            "We should react quickly.",
            "The slack in the rope is dangerous.",
            "The roses are in full bloom.",
            "We walked along the harbour.",
            "That was a real mission.",
            "The theme park was brilliant.",
            "The deploy went out without any errors.",
            "Let's notion this out on paper.",
            "Atlas of the world.",
            "A falcon flew over the field.",
            "The mercury rose all afternoon.",
            "Could you review my pull request when you get a chance?",
            "The fridge is making a funny noise.",
            "Our neighbours are having a party.",
            "The kids have a school trip on Thursday.",
            "I think the issue is with the database connection.",
            "Please book a table for four at seven.",
            "My brother is visiting at the weekend.",
            "Our team is hiring a new designer.",
            "I need to renew my passport.",
            "The engineer fixed the boiler.",
            "Pretty please with a cherry on top.",
            "The sheep were in the field by the river.",
            "We need fresh herbs for the recipe.",
            "Turn the radio on, please.",
            "The plumber is coming at nine.",
            "Let's split the bill.",
            "We sold the old sofa online.",
            "The figures don't add up.",
            "The kettle is on.",
            "Stop dithering and decide.",
            "The sparrow sat on the fence.",
            "Mind the gap between the train and the platform.",
            "I watched a documentary about whales.",
            "Megan's party is on Saturday.",
            "The harbor lights were beautiful.",
            "We set the state of the machine.",
            "It's a hash map of names to numbers.",
            "Use the web socket for that.",
            "Open AI and anthropic news today.",
            "The lumen output is too low.",
            "Orion's belt is easy to spot.",
            "The flask was full of tea.",
        ];
        let keep = |_: &str| false;
        let mut changed = Vec::new();
        for said in sentences {
            let out = s.correct(said, &keep, &is_word);
            if out != said {
                changed.push(format!("{said} → {out}"));
            }
        }
        assert!(changed.is_empty(), "everyday sentences changed:\n{}", changed.join("\n"));
    }

    /// Reads a real window's text and finds its terms, printing only
    /// counts and times (never the text): `YAP_SCREEN_HWND=<hwnd>` (decimal),
    /// `cargo test --lib screen_terms::tests::reads_a_window -- --ignored
    /// --nocapture`. For timing big windows (a long web page, a code
    /// editor) the test opened itself.
    #[test]
    #[ignore]
    fn reads_a_window() {
        let Some(hwnd) = std::env::var("YAP_SCREEN_HWND").ok().and_then(|h| h.trim().parse::<isize>().ok()) else {
            eprintln!("Set YAP_SCREEN_HWND to a window handle");
            return;
        };
        let runs: usize = std::env::var("YAP_SCREEN_RUNS").ok().and_then(|r| r.parse().ok()).unwrap_or(3);
        if let Some(timeout) = std::env::var("YAP_SCREEN_PROBE").ok().and_then(|t| t.parse().ok()) {
            #[cfg(windows)]
            for run in 0..runs {
                println!("probe {run} (calls time out after {timeout} ms):");
                uia::probe(hwnd, timeout, &std::env::var("YAP_SCREEN_STEPS").unwrap_or_else(|_| "around,visible,walk".into()));
            }
            return;
        }
        for run in 0..runs {
            let started = Instant::now();
            let pid = crate::pasting::text_injector::window_process(hwnd).expect("a window");
            let read = read_window(hwnd, pid, started + READ_BUDGET, false);
            let read_ms = started.elapsed().as_millis();
            let Some(read) = read else {
                println!("run {run}: nothing read in {read_ms} ms");
                continue;
            };
            let ordinary = crate::dictionary::learn::Ordinary::new();
            let is_word = |w: &str| ordinary.is(w);
            let terms = extract::extract(&read.text, &is_word, ordinary.has_spell_checker());
            let screen = ScreenTerms::new(terms, &is_word, ordinary.has_spell_checker());
            println!(
                "run {run}: read {read_ms} ms, all {} ms; {} chars ({} from the focused document, {} elements); {} terms, {} targets",
                started.elapsed().as_millis(),
                read.text.chars().count(),
                read.document,
                read.elements,
                screen.terms.len(),
                screen.phrases.len() + screen.words.len()
            );
            if std::env::var("YAP_SCREEN_SHOW").is_ok() {
                // Only for a window of the test's own (its made-up names).
                println!("{:?}", screen.terms.iter().take(30).map(|t| t.text.as_str()).collect::<Vec<_>>());
            }
        }
    }
}

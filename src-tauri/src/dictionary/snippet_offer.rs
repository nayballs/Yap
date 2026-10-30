//! **"Type that often? Save it as a snippet."** (Wispr Flow's nudge, by
//! behaviour; 2026-10-25): the same long phrase — an address, a sign-off, a
//! link, an email address, a phone number, a long stock sentence —
//! dictated (or typed right after a dictation) three times or more in a
//! week, and Yap offers once to save it as a snippet, with a phrase to say
//! for it ("my address").
//!
//! - **What counts** ([`phrases`], pure): in each dictation's text, every
//!   link, email address and phone number; an address up to its postcode
//!   or ZIP code ("12 Example Road, Anytown ZZ1 1ZZ", wherever in the
//!   sentence); and each sentence or paragraph (and the whole dictation,
//!   when it's short) that is a sign-off with a name ("Best wishes,\nSam"),
//!   has lines of its own, or is long ([`LONG_CHARS`]). Compared by their
//!   words alone (case and punctuation aside, [`key`]).
//! - **Where from**: the dictation history of the last [`WINDOW_SECS`]
//!   (`history::dictation_texts_since`; with history off there's none), and
//!   what was typed right after a dictation in its field, on the same line
//!   (`learn.rs`'s watch, `Watch::typed_after`: only with "Suggest words
//!   from your corrections" on; a line break ends that watch). Typed text
//!   is never kept: only a fingerprint of its words and when
//!   (`snippet_suggestions.json`), so it can be counted; it's offered from
//!   the text in hand the third time.
//! - **When** ([`verdict`], pure): seen [`MIN_TIMES`] times in the week,
//!   not a snippet's text already (nor inside one), never offered before
//!   (each phrase once, whatever the answer; one left to fade may come
//!   back), at most one offer a day ([`MIN_GAP_MS`]), and "Suggest
//!   snippets for text you use often" on (the Dictionary view; "Don't
//!   suggest this again" turns it off). It waits out a dictation (up to a
//!   minute), as the notice policy holds it (kind [`CARD`]).
//! - **The offer** ([`offer`]), in one place (`notice::place`): a toast in
//!   the main window while it's focused (`yap-snippet-suggestion`,
//!   `lib/notices/snippetSuggestions.js`), else a Yap bar card; never a
//!   Windows notification. **Save snippet** adds `{from: the phrase to say,
//!   to: the text, snippet: true}` (`shell::settings::change_config`: saved,
//!   applied, every window told; `yap-dictionary-learned` for the
//!   Dictionary view). The phrase to say ([`trigger`]): "my address", "my
//!   email", "my number", "my <site> link", "my sign-off", else "my" and
//!   the text's first two words — never one a snippet has already.
//!
//! Logs: counts, lengths and kinds only, never the text.

use std::collections::{BTreeSet, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::config::DictionaryEntry;

/// Seen this many times in [`WINDOW_SECS`] and it's offered.
pub const MIN_TIMES: usize = 3;
/// The week it's counted over.
pub const WINDOW_SECS: u64 = 7 * 86_400;
/// At most one offer this often (a day).
pub const MIN_GAP_MS: u64 = 24 * 3_600_000;
/// A plain sentence this long or longer counts ("a long phrase").
pub const LONG_CHARS: usize = 40;
/// Longer than this isn't a phrase to offer.
const MAX_CHARS: usize = 400;
/// How long the offer waits for a dictation to end before it's dropped.
const WAIT_MS: u64 = 60_000;
/// The main window's event.
const EVENT: &str = "yap-snippet-suggestion";
/// The bar card's id (its rule in `notice.rs`) and the "saved" card's.
pub const CARD: &str = "snippet-suggestion";
const CARD_SAVED: &str = "snippet-saved";
/// How long the card stays up (then it's left unanswered).
const CARD_MS: u64 = 30_000;

// ---- the phrases -------------------------------------------------------------------

/// What kind of text a phrase is: it names the phrase to say for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Email,
    Link,
    Phone,
    Address,
    SignOff,
    Phrase,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Email => "email",
            Kind::Link => "link",
            Kind::Phone => "phone",
            Kind::Address => "address",
            Kind::SignOff => "sign-off",
            Kind::Phrase => "phrase",
        }
    }
}

/// One phrase in a text: as written, and its words for comparing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phrase {
    pub text: String,
    pub key: String,
    pub kind: Kind,
}

/// The words of `text` for comparing: lower case, letters and digits, one
/// space between ("Best wishes,\nSam." → "best wishes sam").
pub fn key(text: &str) -> String {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("a valid pattern"))
}

fn email_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"(?i)\b[a-z0-9][a-z0-9._%+-]*@[a-z0-9-]+(?:\.[a-z0-9-]+)+\b")
}

fn link_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r#"(?i)\b(?:https?://|www\.)[^\s<>"]+"#)
}

fn phone_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"(?:\+\d{1,3}[ \-]?)?\(?\d[\d \-()]{7,}\d")
}

/// A UK postcode or a US ZIP code: where an address ends.
fn postcode_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"\b(?:[A-Z]{1,2}\d[A-Z\d]? ?\d[A-Z]{2}|[A-Z]{2} \d{5}(?:-\d{4})?)\b")
}

/// Where a sentence or a paragraph ends.
fn break_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"[.!?…]+[\x22\x27”’)]*\s+|\n[ \t]*\n\s*")
}

/// The words a sign-off starts with.
const SIGN_OFFS: &[&str] = &[
    "best wishes", "kind regards", "best regards", "warm regards", "warmest regards", "many thanks", "thanks again",
    "thank you", "thanks", "cheers", "all the best", "speak soon", "talk soon", "regards", "best", "yours sincerely",
    "yours faithfully", "yours truly", "sincerely", "take care", "love", "with thanks", "warmly",
];

/// A sign-off with a name after it: "Best wishes,\nSam", "Kind regards, Sam Jones".
fn is_sign_off(text: &str) -> bool {
    let k = key(text);
    let Some(opener) = SIGN_OFFS.iter().find(|s| k == **s || k.starts_with(&format!("{s} "))) else {
        return false;
    };
    let rest = &k[opener.len()..].trim();
    let rest_words = rest.split(' ').filter(|w| !w.is_empty()).count();
    // The name, as written: a capital after the opener's comma or line break.
    let after = text.trim_start().get(opener.len()..).unwrap_or("");
    let named = after.trim_start_matches(|c: char| !c.is_alphanumeric()).starts_with(|c: char| c.is_uppercase());
    (1..=4).contains(&rest_words) && named && after.trim_start().starts_with([',', '\n', '\r', '-', '–', '—'])
}

fn trim_link(s: &str) -> &str {
    s.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}', '\'', '"', '’', '”'])
}

/// An address in `block` up to its postcode or ZIP code: from the house
/// number (with a "Flat 3" or "Unit 2" before it).
fn address_in(block: &str) -> Option<String> {
    let m = postcode_re().find(block)?;
    let before = &block[..m.start()];
    // The first number word before the postcode starts it.
    let mut start = None;
    let mut offset = 0;
    for w in before.split_inclusive(char::is_whitespace) {
        let core = w.trim().trim_end_matches([',', '.']);
        if start.is_none() && !core.is_empty() && core.chars().next().is_some_and(|c| c.is_ascii_digit()) && core.chars().take_while(char::is_ascii_digit).count() <= 5 {
            start = Some(offset);
        }
        offset += w.len();
    }
    let mut start = start?;
    // "Flat 3, 12 Example Road": the word before the number when it says which flat.
    let head = before[..start].trim_end();
    if let Some(prev) = head.rsplit(char::is_whitespace).next() {
        if ["flat", "apartment", "apt", "unit", "suite"].contains(&prev.to_lowercase().trim_end_matches('.')) {
            start = head.len() - prev.len();
        }
    }
    let text = block[start..m.end()].trim();
    (key(text).split(' ').count() >= 4).then(|| text.to_string())
}

/// The sentences and paragraphs of `text`, as written.
fn blocks(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut last = 0;
    for m in break_re().find_iter(text) {
        // Keep the punctuation with its sentence.
        let end = m.start() + m.as_str().trim_end().len();
        out.push(text[last..end].trim());
        last = m.end();
    }
    out.push(text[last..].trim());
    out.retain(|b| !b.is_empty());
    out
}

/// A note in brackets, not words someone said: Whisper's "[BLANK_AUDIO]" or
/// "[Music]", a test run's stand-in transcript ("[STT stub: …]").
fn is_annotation(block: &str) -> bool {
    let b = block.trim().trim_end_matches(['.', '!', '?']);
    b.starts_with(['[', '(']) && b.ends_with([']', ')']) && !b[1..b.len() - 1].contains([']', ')'])
}

/// A sentence, a paragraph or a short dictation worth a snippet: a
/// sign-off with a name, text with lines of its own, or a long one.
fn block_kind(block: &str) -> Option<Kind> {
    let k = key(block);
    if k.is_empty() || block.chars().count() > MAX_CHARS || is_annotation(block) {
        return None;
    }
    if is_sign_off(block) {
        return Some(Kind::SignOff);
    }
    let words = k.split(' ').count();
    let lines = block.lines().filter(|l| !l.trim().is_empty()).count();
    ((lines >= 2 && words >= 3 && k.len() >= 15) || (block.chars().count() >= LONG_CHARS && words >= 6)).then_some(Kind::Phrase)
}

/// The phrases in a text that could be a snippet (pure; see the module
/// notes). One per key.
pub fn phrases(text: &str) -> Vec<Phrase> {
    let text = text.replace("\r\n", "\n");
    let mut out: Vec<Phrase> = Vec::new();
    let mut push = |t: &str, kind: Kind| {
        let k = key(t);
        if !k.is_empty() && !out.iter().any(|p| p.key == k) {
            out.push(Phrase { text: t.to_string(), key: k, kind });
        }
    };
    for m in email_re().find_iter(&text) {
        push(m.as_str(), Kind::Email);
    }
    for m in link_re().find_iter(&text) {
        let link = trim_link(m.as_str());
        if link.chars().count() >= 10 {
            push(link, Kind::Link);
        }
    }
    for m in phone_re().find_iter(&text) {
        let digits = m.as_str().chars().filter(char::is_ascii_digit).count();
        if (9..=15).contains(&digits) {
            push(m.as_str().trim(), Kind::Phone);
        }
    }
    let blocks = blocks(&text);
    for b in &blocks {
        if let Some(a) = address_in(b) {
            push(&a, Kind::Address);
        }
    }
    for b in &blocks {
        if let Some(kind) = block_kind(b) {
            // A sign-off's last full stop isn't part of it.
            let t = if kind == Kind::SignOff { b.trim_end_matches('.') } else { b };
            push(t, kind);
        }
    }
    // A short dictation of a few sentences, whole (a stock reply).
    if blocks.len() > 1 && blocks.len() <= 3 {
        let whole = text.trim();
        if block_kind(whole) == Some(Kind::Phrase) {
            push(whole, Kind::Phrase);
        }
    }
    out
}

/// A fingerprint of a phrase's words (FNV-1a, stable across runs): what a
/// typed phrase is kept as.
pub fn fingerprint(key: &str) -> u64 {
    crate::notices::notice::fingerprint(key, "")
}

// ---- snippet_suggestions.json --------------------------------------------------------

/// A phrase typed right after a dictation: its words' fingerprint, and when.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Typed {
    pub key: u64,
    /// Unix seconds.
    pub at: u64,
}

/// What `snippet_suggestions.json` remembers (never a phrase's text).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Record {
    /// "Don't suggest this again" (or the Dictionary view's switch off).
    pub off: bool,
    /// When an offer last showed (unix ms).
    pub last_offered_at: u64,
    /// Phrases offered and answered (fingerprints): never offered again.
    pub done: BTreeSet<u64>,
    /// Phrases typed after a dictation, the last week's.
    pub typed: Vec<Typed>,
}

impl Record {
    /// Without what's older than the week before `now` (unix secs).
    fn pruned(mut self, now: u64) -> Self {
        self.typed.retain(|t| t.at + WINDOW_SECS > now && t.at <= now + 60);
        self
    }
}

static RECORD: Mutex<Option<Record>> = Mutex::new(None);

fn record_path() -> std::path::PathBuf {
    crate::config::data_dir().join("snippet_suggestions.json")
}

/// The record, read the first time (`saving::Saver::loaded`: set aside if
/// it isn't one; one Yap couldn't read is never written over, and read
/// again here until it reads, what changed meanwhile kept with it). Unit
/// tests never touch the data folder.
fn loaded(slot: &mut Option<Record>) -> &mut Record {
    if cfg!(test) {
        return slot.get_or_insert_with(Record::default);
    }
    SAVER.loaded(slot, &record_path(), true)
}

fn record() -> Record {
    loaded(&mut RECORD.lock().unwrap_or_else(|p| p.into_inner())).clone()
}

fn update_record(f: impl FnOnce(Record) -> Record) {
    let mut guard = RECORD.lock().unwrap_or_else(|p| p.into_inner());
    let rec = f(loaded(&mut guard).clone());
    if !cfg!(test) {
        let _ = SAVER.write_json(&record_path(), &rec, true);
    }
    *guard = Some(rec);
}

/// snippet_suggestions.json's saves (`saving.rs`): a small store; a failed
/// save is only logged, and tried again with the next.
pub static SAVER: crate::shell::saving::Saver =
    crate::shell::saving::Saver::quiet("snippetSuggestions", "your snippet suggestion choices", save_now);

fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    let mut guard = RECORD.lock().unwrap_or_else(|p| p.into_inner());
    if guard.is_some() {
        let _ = SAVER.write_json(&record_path(), &*loaded(&mut guard), true);
    }
    drop(guard);
    SAVER.check()
}

// ---- when ----------------------------------------------------------------------------

/// A phrase to offer, and how often it was seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub phrase: Phrase,
    pub times: usize,
}

/// Is `p` already a snippet's text, or inside one?
fn a_snippet_already(p: &Phrase, dict: &[DictionaryEntry]) -> bool {
    dict.iter().filter(|e| e.snippet).any(|e| {
        let k = key(&e.to);
        !k.is_empty() && (k == p.key || format!(" {k} ").contains(&format!(" {} ", p.key)))
    })
}

/// The phrase to offer from `now_phrases` (what was just dictated or
/// typed), given the week's dictations (`history`, their texts) and typed
/// phrases (`rec.typed`), the dictionary and the answers so far (pure).
/// `Ok` with the phrase and how often it was seen, else why not.
pub fn verdict(
    now_phrases: &[Phrase],
    history: &[String],
    rec: &Record,
    dict: &[DictionaryEntry],
    now_ms: u64,
) -> Result<Found, &'static str> {
    if rec.off {
        return Err("switched off");
    }
    if rec.last_offered_at != 0 && now_ms >= rec.last_offered_at && now_ms - rec.last_offered_at < MIN_GAP_MS {
        return Err("one offer a day");
    }
    let seen: Vec<HashSet<String>> =
        history.iter().map(|t| phrases(t).into_iter().map(|p| p.key).collect()).collect();
    let mut best: Option<Found> = None;
    for p in now_phrases {
        if rec.done.contains(&fingerprint(&p.key)) || a_snippet_already(p, dict) {
            continue;
        }
        let fp = fingerprint(&p.key);
        let times = seen.iter().filter(|s| s.contains(&p.key)).count() + rec.typed.iter().filter(|t| t.key == fp).count();
        if times < MIN_TIMES {
            continue;
        }
        let better = best.as_ref().is_none_or(|b| {
            (p.kind != Kind::Phrase, p.text.chars().count()) > (b.phrase.kind != Kind::Phrase, b.phrase.text.chars().count())
        });
        if better {
            best = Some(Found { phrase: p.clone(), times });
        }
    }
    best.ok_or("nothing used three times this week")
}

/// The phrase to say for `p`: by its kind ("my address", "my email", "my
/// number", "my example link", "my sign-off"), else "my" and its first two
/// words — "2", "3"… after it when a snippet says that already (pure).
pub fn trigger(p: &Phrase, dict: &[DictionaryEntry]) -> String {
    let base = match p.kind {
        Kind::Email => "my email".to_string(),
        Kind::Phone => "my number".to_string(),
        Kind::Address => "my address".to_string(),
        Kind::SignOff => "my sign-off".to_string(),
        Kind::Link => {
            let host = p.text.split("://").last().unwrap_or("").trim_start_matches("www.");
            let site = host.split(['.', '/', ':']).next().unwrap_or("").to_lowercase();
            if site.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') && (2..=20).contains(&site.len()) {
                format!("my {site} link")
            } else {
                "my link".to_string()
            }
        }
        Kind::Phrase => {
            let words: Vec<&str> = p.key.split(' ').take(2).collect();
            format!("my {}", words.join(" "))
        }
    };
    let taken: HashSet<String> = dict.iter().map(|e| key(&e.from)).collect();
    if !taken.contains(&key(&base)) {
        return base;
    }
    (2..100).map(|n| format!("{base} {n}")).find(|t| !taken.contains(&key(t))).unwrap_or(base)
}

/// The offer's words: title, body, the Save button, the "never" link.
pub fn prompt_text(p: &Phrase, times: usize, say: &str) -> (String, String) {
    let first_line = p.text.lines().next().unwrap_or("").trim();
    let mut shown: String = first_line.chars().take(48).collect();
    if shown.chars().count() < p.text.chars().count() {
        shown = format!("{}…", shown.trim_end_matches([',', ' ']));
    }
    (
        "Type that often? Save it as a snippet".to_string(),
        format!("You've used “{shown}” {times} times this week. Say “{say}” and Yap types it."),
    )
}

// ---- the offer -----------------------------------------------------------------------

/// What the main window gets (`yap-snippet-suggestion`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub id: u64,
    pub title: String,
    pub body: String,
    /// The phrase to say, and the text it types.
    pub say: String,
    pub text: String,
    pub kind: Kind,
}

/// The offer up now, waiting for an answer.
struct Pending {
    id: u64,
    key: u64,
    say: String,
    text: String,
    on_bar: bool,
}

static PENDING: Mutex<Option<Pending>> = Mutex::new(None);
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn pending() -> std::sync::MutexGuard<'static, Option<Pending>> {
    PENDING.lock().unwrap_or_else(|p| p.into_inner())
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// A dictation's text just went in (and into the history): offer a phrase
/// in it used three times this week, once the dictation is over, on a
/// thread of its own.
pub fn after_dictation(app: &AppHandle, text: &str) {
    consider(app, text.to_string(), false);
}

/// Text typed right after a dictation, in its field (`learn.rs`'s watch):
/// counted (its fingerprint kept a week), and offered the third time.
pub fn typed(app: &AppHandle, text: &str) {
    consider(app, text.to_string(), true);
}

fn consider(app: &AppHandle, text: String, typed: bool) {
    let found = phrases(&text);
    if found.is_empty() {
        return;
    }
    let app = app.clone();
    let _ = std::thread::Builder::new().name("snippet-offer".into()).spawn(move || {
        let now = now_ms();
        if typed {
            let at = now / 1000;
            update_record(|r| {
                let mut r = r.pruned(at);
                for p in &found {
                    r.typed.push(Typed { key: fingerprint(&p.key), at });
                }
                r
            });
        }
        let rec = record().pruned(now / 1000);
        let history: Vec<String> = crate::dictation::history::dictation_texts_since((now / 1000).saturating_sub(WINDOW_SECS))
            .into_iter()
            .map(|(_, t)| t)
            .collect();
        let cfg = crate::config::load();
        let found = match verdict(&found, &history, &rec, &cfg.dictionary, now) {
            Ok(f) => f,
            Err(why) => {
                tracing::debug!(phrases = found.len(), typed, why, "Snippet offer: not offered");
                return;
            }
        };
        // Never over a dictation (the one that typed it is still pasting).
        let waited = std::time::Instant::now();
        while crate::notices::notice::moment().dictating {
            if waited.elapsed() > Duration::from_millis(WAIT_MS) {
                tracing::debug!("Snippet offer: a dictation went on too long; dropped");
                return;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        // Another offer may have gone up meanwhile (one a day).
        let rec = record();
        if rec.off || (rec.last_offered_at != 0 && now_ms().saturating_sub(rec.last_offered_at) < MIN_GAP_MS) {
            return;
        }
        offer(&app, &found, &cfg.dictionary);
    });
}

/// Ask about `f`: a toast in the main window while it's on screen, else a
/// Yap bar card. `true` if shown.
fn offer(app: &AppHandle, f: &Found, dict: &[DictionaryEntry]) -> bool {
    let place = crate::notices::notice::place_tip(app, "snippet-suggestion");
    if place == crate::notices::notice::Place::Nowhere {
        tracing::debug!("Snippet offer: nowhere to show it");
        return false;
    }
    let in_window = place == crate::notices::notice::Place::Window;
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let say = trigger(&f.phrase, dict);
    let (title, body) = prompt_text(&f.phrase, f.times, &say);
    *pending() = Some(Pending {
        id,
        key: fingerprint(&f.phrase.key),
        say: say.clone(),
        text: f.phrase.text.clone(),
        on_bar: !in_window,
    });
    let shown = if in_window {
        let prompt = Prompt { id, title, body, say, text: f.phrase.text.clone(), kind: f.phrase.kind };
        app.emit(EVENT, prompt).is_ok()
    } else {
        use crate::bar::{Card, CardAction};
        let card = Card {
            id: CARD.into(),
            icon: "dictionary",
            title,
            body,
            primary: Some(CardAction::new("save", "Save snippet")),
            secondary: Some(CardAction::new("later", "Not now")),
            link: Some(CardAction::new("never", "Don't suggest this again")),
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
                    tracing::warn!("Snippet offer: {}", e);
                }
            }),
        )
    };
    // Not shown after all: the day's count back.
    let shown = crate::notices::notice::tip_shown(CARD, place, shown);
    if shown {
        tracing::info!(kind = f.phrase.kind.name(), times = f.times, len = f.phrase.text.len(), in_window, "Snippet offer: offered");
        let now = now_ms();
        update_record(|mut r| {
            r.last_offered_at = now;
            r
        });
    } else {
        let mut p = pending();
        if p.as_ref().is_some_and(|p| p.id == id) {
            *p = None;
        }
    }
    shown
}

/// An answer to offer `id`: "save" (returns the phrase to say once the
/// snippet is saved), "later" (Not now, or ✕), "never" (Don't suggest this
/// again: no more offers), "expire" / "" (left alone: it may come back).
fn respond(app: &AppHandle, id: u64, answer: &str) -> Result<Option<String>, String> {
    let taken = {
        let mut p = pending();
        match p.as_ref() {
            Some(open) if open.id == id => p.take(),
            _ => None,
        }
    };
    let Some(open) = taken else {
        return match answer {
            "save" => Err("That suggestion is no longer open".into()),
            _ => Ok(None),
        };
    };
    if matches!(answer, "save" | "later" | "never") {
        let never = answer == "never";
        update_record(|mut r| {
            r.done.insert(open.key);
            r.off |= never;
            r
        });
    }
    if answer != "save" {
        tracing::info!(answer, "Snippet offer: answered");
        return Ok(None);
    }
    let (say, text) = (open.say.clone(), open.text.clone());
    let cfg = crate::shell::settings::change_config(app, |c| {
        let say = if c.dictionary.iter().any(|e| key(&e.from) == key(&say)) {
            trigger(&Phrase { text: text.clone(), key: key(&text), kind: Kind::Phrase }, &c.dictionary)
        } else {
            say.clone()
        };
        c.dictionary.push(DictionaryEntry {
            from: say,
            to: text.clone(),
            fuzzy: true,
            snippet: true,
            starred: false,
            also_heard: Vec::new(),
        });
        Ok(())
    })?;
    let said = cfg.dictionary.last().map(|e| e.from.clone()).unwrap_or(say);
    let _ = app.emit("yap-dictionary-learned", serde_json::json!({ "entries": cfg.dictionary, "added": [said.clone()] }));
    tracing::info!(len = text.len(), "Snippet offer: saved as a snippet");
    if open.on_bar {
        let card = crate::bar::Card {
            id: CARD_SAVED.into(),
            icon: "dictionary",
            title: "Snippet saved".into(),
            body: format!("Say “{said}” and Yap types it."),
            timeout_ms: Some(4_000),
            ..Default::default()
        };
        crate::bar::show_card(app, card, Box::new(|_, _| {}));
    }
    Ok(Some(said))
}

// ---- commands ------------------------------------------------------------------------

/// The main window's answer to its toast (`yap-snippet-suggestion`):
/// `answer` = "save" | "later" | "never" | "expire". The phrase to say, once saved.
#[tauri::command]
pub async fn snippet_suggestion_respond(app: AppHandle, id: u64, answer: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || respond(&app, id, &answer)).await.map_err(|e| e.to_string())?
}

/// "Suggest snippets for text you use often" (the Dictionary view): on?
#[tauri::command]
pub fn snippet_suggestions_enabled() -> bool {
    !record().off
}

/// …switched on or off (`snippet_suggestions.json`, not config.json).
#[tauri::command]
pub fn snippet_suggestions_set(on: bool) {
    update_record(|mut r| {
        r.off = !on;
        r
    });
    tracing::info!(on, "Snippet offer: switched");
}

/// Test mode only: every answer, typed phrase and the last offer's time
/// forgotten (in memory and on disk), nothing pending.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_snippet_offer_reset() -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".into());
    }
    *pending() = None;
    update_record(|_| Record::default());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<(Kind, String)> {
        phrases(text).into_iter().map(|p| (p.kind, p.text)).collect()
    }

    fn snippet(from: &str, to: &str) -> DictionaryEntry {
        DictionaryEntry { from: from.into(), to: to.into(), fuzzy: true, snippet: true, starred: false, also_heard: vec![] }
    }

    const ADDRESS: &str = "Please send it to 12 Example Road, Anytown ZZ1 1ZZ.";

    #[test]
    fn a_note_in_brackets_is_never_a_phrase() {
        assert!(kinds("[STT stub: received 1.0s of audio, engine=parakeet]").is_empty());
        assert!(kinds("[BLANK_AUDIO]").is_empty());
        assert!(kinds("(upbeat music playing in the background for a while)").is_empty());
        // Words around a bracket are still words.
        let said = "As I said [inaudible] we should move the launch to the second week of June.";
        assert_eq!(kinds(said), [(Kind::Phrase, said.to_string())]);
    }

    #[test]
    fn the_phrases_worth_a_snippet() {
        assert_eq!(kinds(ADDRESS), [(Kind::Address, "12 Example Road, Anytown ZZ1 1ZZ".to_string()), (Kind::Phrase, ADDRESS.to_string())]);
        assert_eq!(kinds("My flat is Flat 3, 12 Example Road, Anytown ZZ1 1ZZ")[0].1, "Flat 3, 12 Example Road, Anytown ZZ1 1ZZ");
        assert_eq!(
            kinds("Book a time at https://bookings.example/sam-example/30min, or email sam@example.com."),
            [
                (Kind::Email, "sam@example.com".to_string()),
                (Kind::Link, "https://bookings.example/sam-example/30min".to_string()),
                (Kind::Phrase, "Book a time at https://bookings.example/sam-example/30min, or email sam@example.com.".to_string()),
            ]
        );
        assert_eq!(kinds("Call me on 07700 900123 after six")[0], (Kind::Phone, "07700 900123".to_string()));
        assert_eq!(
            kinds("Thanks for the figures.\n\nBest wishes,\nSam."),
            [(Kind::SignOff, "Best wishes,\nSam".to_string()), (Kind::Phrase, "Thanks for the figures.\n\nBest wishes,\nSam.".to_string())]
        );
        assert_eq!(kinds("Kind regards, Sam Jones")[0], (Kind::SignOff, "Kind regards, Sam Jones".to_string()));
        // Short everyday sentences, a bare "Thanks", numbers that aren't a
        // phone: nothing.
        for none in [
            "Thanks.",
            "Thanks for that.",
            "See you at 10.",
            "Can you send me the report by Friday?",
            "I'll call you back in five minutes.",
            "Best of luck with it",
            "The meeting is at 3 pm in room 12.",
            "Turnover was 1,250,000 last year",
        ] {
            assert!(phrases(none).is_empty(), "{none}: {:?}", phrases(none));
        }
        // The same words however they're written.
        assert_eq!(key("Best wishes,\nSam."), key("best wishes sam"));
    }

    #[test]
    fn three_times_in_a_week_and_it_is_offered_once() {
        let now = phrases(ADDRESS);
        let two = vec![ADDRESS.to_string(), "Send the parcel to 12 Example Road, Anytown ZZ1 1ZZ please".to_string()];
        let mut three = two.clone();
        three.push("Lunch was good.".into());
        three.push("It's 12 example road, Anytown ZZ1 1ZZ".into());
        let rec = Record::default();
        // Twice (this one included): not yet.
        assert_eq!(verdict(&now, &two, &rec, &[], 1), Err("nothing used three times this week"));
        // Three times, said three ways: the address, not the sentence.
        let f = verdict(&now, &three, &rec, &[], 1).unwrap();
        assert_eq!((f.phrase.kind, f.phrase.text.as_str(), f.times), (Kind::Address, "12 Example Road, Anytown ZZ1 1ZZ", 3));
        // Typed twice more counts too.
        let typed = Record {
            typed: vec![Typed { key: fingerprint(&now[0].key), at: 1 }, Typed { key: fingerprint(&now[0].key), at: 2 }],
            ..Record::default()
        };
        assert_eq!(verdict(&now, &two[..1], &typed, &[], 1).unwrap().times, 3);
        // Offered before (any answer): never again. Switched off: nothing.
        let done = Record { done: [fingerprint(&now[0].key)].into(), ..Record::default() };
        assert_ne!(verdict(&now, &three, &done, &[], 1).map(|f| f.phrase.kind), Ok(Kind::Address));
        assert_eq!(verdict(&now, &three, &Record { off: true, ..Record::default() }, &[], 1), Err("switched off"));
        // At most one a day.
        let today = Record { last_offered_at: 1_000, ..Record::default() };
        assert_eq!(verdict(&now, &three, &today, &[], 1_000 + MIN_GAP_MS - 1), Err("one offer a day"));
        assert!(verdict(&now, &three, &today, &[], 1_000 + MIN_GAP_MS).is_ok());
        // A snippet's text already (it expanded in those dictations).
        let dict = vec![snippet("my address", "12 Example Road\nAnytown ZZ1 1ZZ")];
        assert_ne!(verdict(&now, &three, &rec, &dict, 1).map(|f| f.phrase.kind), Ok(Kind::Address));
    }

    #[test]
    fn the_phrase_to_say_is_named_by_the_text_and_never_one_taken() {
        let p = |text: &str| phrases(text).into_iter().next().unwrap();
        assert_eq!(trigger(&p("12 Example Road, Anytown ZZ1 1ZZ"), &[]), "my address");
        assert_eq!(trigger(&p("sam@example.com"), &[]), "my email");
        assert_eq!(trigger(&p("https://www.bookings.example/sam-example"), &[]), "my bookings link");
        assert_eq!(trigger(&p("07700 900123"), &[]), "my number");
        assert_eq!(trigger(&p("Best wishes,\nSam"), &[]), "my sign-off");
        assert_eq!(
            trigger(&p("Thanks for getting in touch, I'll reply within two working days."), &[]),
            "my thanks for"
        );
        let taken = vec![snippet("my address", "1 Long Lane"), snippet("My Address 2", "2 Long Lane")];
        assert_eq!(trigger(&p("12 Example Road, Anytown ZZ1 1ZZ"), &taken), "my address 3");
        let (title, body) = prompt_text(&p("12 Example Road, Anytown ZZ1 1ZZ"), 3, "my address");
        assert_eq!(title, "Type that often? Save it as a snippet");
        assert_eq!(body, "You've used “12 Example Road, Anytown ZZ1 1ZZ” 3 times this week. Say “my address” and Yap types it.");
    }

    #[test]
    fn typed_phrases_are_kept_a_week_as_fingerprints_only() {
        let rec = Record { typed: vec![Typed { key: 1, at: 100 }, Typed { key: 2, at: 100 + WINDOW_SECS }], ..Record::default() };
        let json = serde_json::to_string(&rec).unwrap();
        assert!(!json.contains("Station"));
        assert_eq!(rec.pruned(100 + WINDOW_SECS).typed, [Typed { key: 2, at: 100 + WINDOW_SECS }]);
    }
}

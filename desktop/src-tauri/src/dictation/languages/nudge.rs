//! **"You spoke French · Add it to My languages?"** (Wispr Flow's "Speaking
//! a different language?" of 28 Sep 2026, by behaviour): after a dictation
//! a Whisper model heard as a language the person didn't pick — and was
//! sure of it — Yap offers once to add it.
//!
//! - **Only when it's sure** ([`verdict`], pure): My languages picked, the
//!   language heard isn't one of them, Whisper gave it at least [`MIN_P`]
//!   (its own detection, `stt::Heard`), the dictation lasted at least
//!   [`MIN_SECS`] (a second of speech is too little to tell), "Tell me when
//!   I speak another language" on (`language_nudge`), and it's a language
//!   Yap knows.
//! - **Models that don't say** (2026-10-31, [`text_verdict`]): Parakeet V3
//!   and SenseVoice write the language they hear without saying which, so
//!   Yap reads it from the text (`text_id`: function words and trigrams that
//!   must agree, at least five words, the same limits after). An
//!   English-only model (Parakeet V2, Moonshine…) writes another language as
//!   English-ish words, so it's caught only when its text reads as that
//!   language all the same; then the offer is **"Parakeet V2 only knows
//!   English"** with the model to switch to ([`Kind::Model`]: Choose a model
//!   puts the language in My languages and opens Settings there, where the
//!   recommended model is named — Yap never switches it). Whisper's own
//!   detection on the audio would catch more, but means loading a second
//!   model (hundreds of MB, seconds of the processor) after every
//!   dictation: not done. Canary and Cohere are told the language, so
//!   adding one changes nothing for them: never offered.
//! - **Rate-limited**: at most one offer per [`MIN_GAP_MS`]; "Not now"
//!   twice for a language (or its ✕) and it isn't offered again, nor after
//!   "Don't ask about French". The answers live in
//!   `<data>/language_nudges.json`, not config.json (Settings auto-saves its
//!   whole copy of the config).
//! - **Never during a dictation**: it waits on its own thread until no
//!   dictation records or transcribes (`notice::moment`, the settle time
//!   after one included) — up to [`WAIT_MS`], then it's dropped — and the
//!   notice policy holds it too (kind [`CARD`]: a tip-like suggestion, held
//!   while dictating, dropped unseen after a minute).
//! - **One place** (`notice::place`): a toast in the main window while
//!   it's focused (`yap-language-nudge`, `lib/notices/languageNudge.js`),
//!   else a card on the Yap bar; nowhere else, never a Windows
//!   notification. **Add** puts the language in My languages
//!   (`shell::settings::change_config`: saved, applied, every window told);
//!   the next dictation in it is picked out among them.
//!
//! The log says the language's code and how sure, never the text.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use super::catalog;
use super::text_id::{self, Guess};
use crate::dictation::stt::Heard;

/// The likeliest language must be at least this sure.
pub const MIN_P: f32 = 0.85;
/// The dictation at least this long (seconds of audio).
pub const MIN_SECS: f32 = 2.0;
/// At most one offer this often.
pub const MIN_GAP_MS: u64 = 10 * 60_000;
/// "Not now" this many times and a language isn't offered again.
pub const NOT_NOW_LIMIT: u32 = 2;
/// How long the offer waits for a dictation to end before it's dropped.
const WAIT_MS: u64 = 60_000;
/// The main window's event.
const EVENT: &str = "yap-language-nudge";
/// The bar card's id (its rule in `notice.rs`) and the "added" card's.
pub const CARD: &str = "language-nudge";
const CARD_ADDED: &str = "language-added";
/// How long the card stays up (then it's left unanswered).
const CARD_MS: u64 = 30_000;

/// What `language_nudges.json` remembers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Record {
    /// When an offer last showed (unix ms).
    pub last_offered_at: u64,
    /// A language's code → the answers to it.
    pub languages: BTreeMap<String, Answers>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Answers {
    /// "Not now" (or ✕).
    pub not_now: u32,
    /// "Don't ask about French".
    pub never: bool,
}

/// May `code` be offered (not turned down for good)?
pub fn allowed(rec: &Record, code: &str) -> bool {
    rec.languages.get(code).is_none_or(|a| !a.never && a.not_now < NOT_NOW_LIMIT)
}

/// Long enough since the last offer (or the clock went back)?
pub fn gap_ok(rec: &Record, now: u64) -> bool {
    rec.last_offered_at == 0 || now < rec.last_offered_at || now - rec.last_offered_at >= MIN_GAP_MS
}

/// `rec` after `answer` ("later" | "never") about `code`.
pub fn answered(mut rec: Record, code: &str, answer: &str) -> Record {
    let a = rec.languages.entry(code.to_string()).or_default();
    match answer {
        "later" => a.not_now = a.not_now.saturating_add(1),
        "never" => a.never = true,
        _ => {}
    }
    rec
}

/// Whether to offer `heard.top` after a dictation of `secs` seconds (pure):
/// `Ok`, or why not.
pub fn verdict(heard: &Heard, secs: f32, mine: &[String], on: bool, rec: &Record, now: u64) -> Result<(), &'static str> {
    if !on {
        Err("switched off")
    } else if mine.is_empty() {
        Err("no languages picked")
    } else if mine.contains(&heard.top) {
        Err("one of the person's languages")
    } else if heard.p < MIN_P {
        Err("not sure enough")
    } else {
        limits(&heard.top, secs, rec, now)
    }
}

/// What every offer must pass after its evidence (pure): long enough, a
/// language Yap knows, not turned down, not too soon after the last.
fn limits(code: &str, secs: f32, rec: &Record, now: u64) -> Result<(), &'static str> {
    if secs < MIN_SECS {
        Err("too short to tell")
    } else if catalog::find(code).is_none() {
        Err("a language Yap doesn't know")
    } else if !allowed(rec, code) {
        Err("turned down before")
    } else if !gap_ok(rec, now) {
        Err("too soon after the last one")
    } else {
        Ok(())
    }
}

/// Whether to offer the language read from a dictation's text (`g`,
/// `text_id`) in a model that doesn't say which it heard (pure): `Ok`, or
/// why not. `english_only`: the model knows English alone — then English is
/// the person's too, and a language already among theirs is still worth
/// saying (the model can't write it); else as [`verdict`], the text's own
/// gate (`text_id::sure`) in Whisper's probability's place.
pub fn text_verdict(g: &Guess, secs: f32, mine: &[String], on: bool, english_only: bool, rec: &Record, now: u64) -> Result<(), &'static str> {
    let mut theirs = mine.to_vec();
    if !on {
        return Err("switched off");
    } else if english_only {
        if g.code == "en" {
            return Err("one of the person's languages");
        }
        theirs.retain(|c| c != g.code);
        if !theirs.iter().any(|c| c == "en") {
            theirs.push("en".into());
        }
    } else if mine.is_empty() {
        return Err("no languages picked");
    } else if mine.iter().any(|c| c == g.code) {
        return Err("one of the person's languages");
    }
    text_id::sure(g, &theirs)?;
    limits(g.code, secs, rec, now)
}

/// The offer's words: title, body, the Add button, the "never" link.
pub fn prompt_text(top: &str, picked: &str) -> (String, String, String, String) {
    let lang = catalog::name(top);
    let body = if picked != top && catalog::find(picked).is_some() {
        format!("Yap wrote this dictation as {}, one of your languages.", catalog::name(picked))
    } else {
        "Yap can then tell it apart from your other languages.".to_string()
    };
    (format!("You spoke {lang} · Add it to My languages?"), body, format!("Add {lang}"), format!("Don't ask about {lang}"))
}

/// The offer's words for a language read from the text of a model that
/// wrote it without saying (Parakeet V3): it's in that language already, and
/// My languages keeps AI cleanup in it.
pub fn read_prompt_text(code: &str) -> (String, String, String, String) {
    let lang = catalog::name(code);
    (
        format!("You spoke {lang} · Add it to My languages?"),
        format!("Then AI cleanup keeps your {lang} dictations in {lang}."),
        format!("Add {lang}"),
        format!("Don't ask about {lang}"),
    )
}

/// The offer's words for an English-only `model` that heard `code`:
/// `recommended`, the model to switch to (its name, and whether it's
/// installed). The button opens Settings; the model is never switched.
pub fn model_prompt_text(code: &str, model: &str, recommended: Option<(&str, bool)>) -> (String, String, String, String) {
    let lang = catalog::name(code);
    let name = crate::dictation::stt::model_name(model);
    let body = match recommended {
        Some((rec, true)) => format!("You spoke {lang}. {rec}, which is installed, writes {lang} as you say it: switch to it in Settings."),
        Some((rec, false)) => format!("You spoke {lang}. {rec} writes {lang} as you say it: switch to it in Settings."),
        None => format!("You spoke {lang}. Parakeet V3 or Whisper writes it as you say it: switch in Settings."),
    };
    (format!("{name} only knows English"), body, "Choose a model".to_string(), format!("Don't ask about {lang}"))
}

// ---- language_nudges.json ------------------------------------------------------------

static RECORD: Mutex<Option<Record>> = Mutex::new(None);

fn record_path() -> std::path::PathBuf {
    crate::config::data_dir().join("language_nudges.json")
}

/// The record, read the first time (`saving::Saver::loaded`: set aside if
/// it isn't one; one Yap couldn't read is never written over, and read
/// again here until it reads, what changed meanwhile kept with it).
fn loaded(slot: &mut Option<Record>) -> &mut Record {
    SAVER.loaded(slot, &record_path(), true)
}

fn record() -> Record {
    loaded(&mut RECORD.lock().unwrap_or_else(|p| p.into_inner())).clone()
}

fn update_record(f: impl FnOnce(Record) -> Record) {
    let mut guard = RECORD.lock().unwrap_or_else(|p| p.into_inner());
    let rec = f(loaded(&mut guard).clone());
    let _ = SAVER.write_json(&record_path(), &rec, true);
    *guard = Some(rec);
}

/// language_nudges.json's saves (`saving.rs`): a small store of answers; a
/// failed save is only logged, and tried again with the next.
pub static SAVER: crate::shell::saving::Saver =
    crate::shell::saving::Saver::quiet("languageNudges", "your language suggestion choices", save_now);

fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    let mut guard = RECORD.lock().unwrap_or_else(|p| p.into_inner());
    if guard.is_some() {
        let _ = SAVER.write_json(&record_path(), &*loaded(&mut guard), true);
    }
    drop(guard);
    SAVER.check()
}

// ---- the offer -----------------------------------------------------------------------

/// What an offer's button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// Add the language to My languages.
    Add,
    /// The model knows English only: the language goes in My languages and
    /// Settings opens there, at the model to switch to.
    Model,
}

/// An offer to make: the language and its words ([`prompt_text`],
/// [`read_prompt_text`], [`model_prompt_text`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Offer {
    pub code: String,
    pub kind: Kind,
    pub title: String,
    pub body: String,
    pub add: String,
    pub never: String,
}

impl Offer {
    fn new(code: &str, kind: Kind, (title, body, add, never): (String, String, String, String)) -> Offer {
        Offer { code: code.to_string(), kind, title, body, add, never }
    }
}

/// What the main window gets (`yap-language-nudge`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub id: u64,
    /// The language's code and name.
    pub code: String,
    pub language: String,
    /// "add", or "model": Rust opens Settings itself after the answer.
    pub kind: Kind,
    pub title: String,
    pub body: String,
    pub add: String,
    pub never: String,
}

/// The offer up now, waiting for an answer.
struct Pending {
    id: u64,
    code: String,
    kind: Kind,
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

/// A dictation of `secs` seconds a Whisper model heard as `heard`: offer the
/// language if it's worth it ([`verdict`]), once the dictation is over, on
/// a thread of its own.
pub fn consider(app: &AppHandle, heard: &Heard, secs: f32, mine: &[String], on: bool) {
    let rec = record();
    if let Err(why) = verdict(heard, secs, mine, on, &rec, now_ms()) {
        not_offered(&heard.top, why, mine, on);
        return;
    }
    later(app, Offer::new(&heard.top, Kind::Add, prompt_text(&heard.top, &heard.picked)));
}

/// A dictation of `secs` seconds whose text, from `model` (one that doesn't
/// say which language it heard), reads as `g` (`text_id`): offer the
/// language if it's worth it ([`text_verdict`]) — "Add it to My
/// languages?", or for an English-only model "Parakeet V2 only knows
/// English" — once the dictation is over, on a thread of its own.
pub fn consider_text(app: &AppHandle, g: &Guess, secs: f32, mine: &[String], on: bool, model: &str) {
    let english_only = catalog::english_only(model);
    let rec = record();
    if let Err(why) = text_verdict(g, secs, mine, on, english_only, &rec, now_ms()) {
        not_offered(g.code, why, mine, on);
        return;
    }
    let offer = if english_only {
        // The model for the person's languages and this one (an installed
        // one first): Settings names it too.
        let mut langs = mine.to_vec();
        if !langs.iter().any(|c| c == g.code) {
            langs.push(g.code.to_string());
        }
        let installed = crate::dictation::commands::installed_models();
        let rec = catalog::recommend(&langs, &installed)
            .map(|id| (crate::dictation::stt::model_name(id), installed.iter().any(|m| m == id)));
        let rec = rec.as_ref().map(|(name, inst)| (name.as_str(), *inst));
        Offer::new(g.code, Kind::Model, model_prompt_text(g.code, model, rec))
    } else {
        Offer::new(g.code, Kind::Add, read_prompt_text(g.code))
    };
    later(app, offer);
}

/// Logged: a language not offered, and why (the code, never the text).
/// Every dictation in one of yours is no news; another language is.
fn not_offered(code: &str, why: &str, mine: &[String], on: bool) {
    if !on || mine.is_empty() || mine.iter().any(|m| m == code) || why == "one of the person's languages" {
        tracing::debug!(language = %code, why, "Language nudge: not offered");
    } else {
        tracing::info!(language = %code, why, "Language nudge: not offered");
    }
}

/// [`offer`] once no dictation runs (up to [`WAIT_MS`]), on its own thread.
fn later(app: &AppHandle, offer_: Offer) {
    let app = app.clone();
    let _ = std::thread::Builder::new().name("language-nudge".into()).spawn(move || {
        // Never over a dictation (the one that heard it is still pasting).
        let waited = std::time::Instant::now();
        while crate::notices::notice::moment().dictating {
            if waited.elapsed() > Duration::from_millis(WAIT_MS) {
                tracing::debug!("Language nudge: a dictation went on too long; dropped");
                return;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        if !gap_ok(&record(), now_ms()) {
            return;
        }
        offer(&app, offer_);
    });
}

/// Make `offer`: a toast in the main window while it's on screen, else a
/// Yap bar card. `true` if shown.
pub fn offer(app: &AppHandle, offer: Offer) -> bool {
    let place = crate::notices::notice::place_tip(app, "language-nudge");
    if place == crate::notices::notice::Place::Nowhere {
        tracing::debug!("Language nudge: nowhere to show it");
        return false;
    }
    let in_window = place == crate::notices::notice::Place::Window;
    let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
    let Offer { code, kind, title, body, add, never } = offer;
    let code = code.as_str();
    *pending() = Some(Pending { id, code: code.to_string(), kind, on_bar: !in_window });
    let shown = if in_window {
        let prompt = Prompt { id, code: code.to_string(), language: catalog::name(code), kind, title, body, add, never };
        app.emit(EVENT, prompt).is_ok()
    } else {
        use crate::bar::{Card, CardAction};
        let card = Card {
            id: CARD.into(),
            icon: "language",
            title,
            body,
            primary: Some(CardAction::new("add", add)),
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
                    tracing::warn!("Language nudge: {}", e);
                }
            }),
        )
    };
    // Not shown after all: the day's count back.
    let shown = crate::notices::notice::tip_shown(CARD, place, shown);
    if shown {
        tracing::info!(language = %code, in_window, ?kind, "Language nudge: offered");
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

/// An answer to offer `id`: "add" (`true` once the language is in My
/// languages; for [`Kind::Model`], Settings opens there too), "later" (Not
/// now, or ✕), "never", "expire" / "" (left alone: nothing counted).
pub fn respond(app: &AppHandle, id: u64, answer: &str) -> Result<bool, String> {
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
            _ => Ok(false),
        };
    };
    match answer {
        "add" => {
            let code = open.code.clone();
            let mine = super::my_languages(&crate::config::load());
            if !mine.contains(&code) {
                crate::shell::settings::change_config(app, |c| {
                    let mut mine = super::my_languages(c);
                    if !mine.contains(&code) {
                        mine.push(code.clone());
                    }
                    c.my_languages = mine;
                    Ok(())
                })?;
                tracing::info!(language = %open.code, "Language nudge: added to My languages");
            }
            if open.kind == Kind::Model {
                // Settings → Dictation → My languages: which model knows it.
                use tauri::Emitter;
                crate::shell::window::show_settings(app)?;
                let _ = app.emit("yap-open-settings", "dictation#languages");
                tracing::info!(language = %open.code, "Language nudge: Settings opened at the speech models");
            } else if open.on_bar {
                let card = crate::bar::Card {
                    id: CARD_ADDED.into(),
                    icon: "language",
                    title: format!("{} is in My languages", catalog::name(&open.code)),
                    body: "Change your languages in Settings → Dictation.".into(),
                    timeout_ms: Some(4_000),
                    ..Default::default()
                };
                crate::bar::show_card(app, card, Box::new(|_, _| {}));
            }
            Ok(true)
        }
        "later" | "never" => {
            update_record(|r| answered(r, &open.code, answer));
            let never = answer == "never";
            tracing::info!(language = %open.code, never, "Language nudge: turned down");
            Ok(false)
        }
        _ => Ok(false),
    }
}

/// The main window's answer to its toast (`yap-language-nudge`): `answer` =
/// "add" | "later" | "never" | "expire". `true` once the language was added.
#[tauri::command]
pub async fn language_nudge_respond(app: AppHandle, id: u64, answer: String) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || respond(&app, id, &answer)).await.map_err(|e| e.to_string())?
}

/// Test mode only: every answer and the last offer's time forgotten (in
/// memory and on disk), nothing pending.
#[cfg(debug_assertions)]
pub fn reset() {
    *pending() = None;
    update_record(|_| Record::default());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heard(top: &str, p: f32, picked: &str) -> Heard {
        Heard { top: top.into(), p, picked: picked.into(), picked_p: 1.0 - p }
    }

    fn mine(codes: &[&str]) -> Vec<String> {
        codes.iter().map(|c| c.to_string()).collect()
    }

    #[test]
    fn it_offers_only_a_language_whisper_was_sure_of_that_the_person_didnt_pick() {
        let rec = Record::default();
        let en = mine(&["en"]);
        let fr = heard("fr", 0.97, "en");
        assert_eq!(verdict(&fr, 4.0, &en, true, &rec, 1), Ok(()));
        // One of theirs.
        assert!(verdict(&heard("en", 0.99, "en"), 4.0, &en, true, &rec, 1).is_err());
        // Not sure enough, or too short to tell.
        assert_eq!(verdict(&heard("fr", 0.6, "en"), 4.0, &en, true, &rec, 1), Err("not sure enough"));
        assert_eq!(verdict(&fr, 1.2, &en, true, &rec, 1), Err("too short to tell"));
        // No languages picked: nothing to compare with. Switched off.
        assert_eq!(verdict(&fr, 4.0, &[], true, &rec, 1), Err("no languages picked"));
        assert_eq!(verdict(&fr, 4.0, &en, false, &rec, 1), Err("switched off"));
        assert!(verdict(&heard("xx", 0.99, "en"), 4.0, &en, true, &rec, 1).is_err());
    }

    #[test]
    fn at_most_one_offer_every_ten_minutes_and_never_one_turned_down() {
        let en = mine(&["en"]);
        let fr = heard("fr", 0.97, "en");
        let t0 = 1_000_000;
        let rec = Record { last_offered_at: t0, ..Default::default() };
        assert_eq!(verdict(&fr, 4.0, &en, true, &rec, t0 + 60_000), Err("too soon after the last one"));
        assert_eq!(verdict(&fr, 4.0, &en, true, &rec, t0 + MIN_GAP_MS), Ok(()));
        // A clock that went back doesn't block it for good.
        assert!(gap_ok(&rec, t0 - 5));
        // Not now once: still offered; twice: never again. "Never": never.
        let once = answered(Record::default(), "fr", "later");
        assert!(allowed(&once, "fr"));
        let twice = answered(once, "fr", "later");
        assert!(!allowed(&twice, "fr"));
        assert_eq!(verdict(&fr, 4.0, &en, true, &twice, 1), Err("turned down before"));
        // Another language is still offered.
        assert!(allowed(&twice, "de"));
        let never = answered(Record::default(), "de", "never");
        assert!(!allowed(&never, "de"));
        // An expiry counts for nothing.
        assert_eq!(answered(Record::default(), "fr", "expire").languages["fr"], Answers::default());
    }

    #[test]
    fn the_offer_says_what_was_heard_and_how_yap_wrote_it() {
        let (title, body, add, never) = prompt_text("fr", "en");
        assert_eq!(title, "You spoke French · Add it to My languages?");
        assert_eq!(body, "Yap wrote this dictation as English, one of your languages.");
        assert_eq!(add, "Add French");
        assert_eq!(never, "Don't ask about French");
    }

    fn guess(code: &'static str, words: usize, hits: usize, others: Vec<(&'static str, usize)>) -> Guess {
        Guess { code, words, hits, others, agrees: true, by_script: false }
    }

    #[test]
    fn a_language_read_from_the_text_passes_the_same_limits() {
        let rec = Record::default();
        let en = mine(&["en"]);
        let fr = guess("fr", 9, 5, vec![("en", 1)]);
        assert_eq!(text_verdict(&fr, 4.0, &en, true, false, &rec, 1), Ok(()));
        // One of theirs; none picked; switched off.
        assert_eq!(text_verdict(&fr, 4.0, &mine(&["en", "fr"]), true, false, &rec, 1), Err("one of the person's languages"));
        assert_eq!(text_verdict(&fr, 4.0, &[], true, false, &rec, 1), Err("no languages picked"));
        assert_eq!(text_verdict(&fr, 4.0, &en, false, false, &rec, 1), Err("switched off"));
        // The text's own gate: too few words, or as many English ones.
        assert_eq!(text_verdict(&guess("fr", 4, 4, vec![]), 4.0, &en, true, false, &rec, 1), Err("too short to tell"));
        assert_eq!(text_verdict(&guess("fr", 9, 3, vec![("en", 3)]), 4.0, &en, true, false, &rec, 1), Err("not sure enough"));
        // Whisper's limits, unchanged: two seconds, ten minutes apart, never
        // one turned down.
        assert_eq!(text_verdict(&fr, 1.5, &en, true, false, &rec, 1), Err("too short to tell"));
        let t0 = 1_000_000;
        let soon = Record { last_offered_at: t0, ..Default::default() };
        assert_eq!(text_verdict(&fr, 4.0, &en, true, false, &soon, t0 + 60_000), Err("too soon after the last one"));
        assert_eq!(text_verdict(&fr, 4.0, &en, true, false, &soon, t0 + MIN_GAP_MS), Ok(()));
        let twice = answered(answered(Record::default(), "fr", "later"), "fr", "later");
        assert_eq!(text_verdict(&fr, 4.0, &en, true, false, &twice, 1), Err("turned down before"));
        // From the text itself: Parakeet V3's French offered, English with
        // Swedish names never.
        let among = text_id::candidates("parakeet-tdt-0.6b-v3", &en);
        let g = text_id::identify("Je pense que la nouvelle version marche beaucoup mieux que l'ancienne.", &among).unwrap();
        assert_eq!(text_verdict(&g, 4.0, &en, true, false, &rec, 1), Ok(()));
        let g = text_id::identify("Ask Ingrid Johansson about the Göteborg office move.", &among).unwrap();
        assert!(text_verdict(&g, 4.0, &en, true, false, &rec, 1).is_err(), "{g:?}");
    }

    #[test]
    fn an_english_only_model_says_so_even_for_one_of_theirs() {
        let rec = Record::default();
        let fr = guess("fr", 9, 5, vec![("en", 1)]);
        // French among theirs: Parakeet V2 still can't write it.
        assert_eq!(text_verdict(&fr, 4.0, &mine(&["en", "fr"]), true, true, &rec, 1), Ok(()));
        // Nothing picked: English is theirs all the same.
        assert_eq!(text_verdict(&fr, 4.0, &[], true, true, &rec, 1), Ok(()));
        assert_eq!(text_verdict(&guess("fr", 9, 3, vec![("en", 2)]), 4.0, &[], true, true, &rec, 1), Err("not sure enough"));
        assert_eq!(text_verdict(&guess("en", 9, 5, vec![]), 4.0, &[], true, true, &rec, 1), Err("one of the person's languages"));
        let (title, body, add, never) = model_prompt_text("fr", "parakeet-tdt-0.6b-v2", Some(("Parakeet V3", false)));
        assert_eq!(title, "Parakeet V2 only knows English");
        assert_eq!(body, "You spoke French. Parakeet V3 writes French as you say it: switch to it in Settings.");
        assert_eq!((add.as_str(), never.as_str()), ("Choose a model", "Don't ask about French"));
        assert!(model_prompt_text("fr", "parakeet-tdt-0.6b-v2", Some(("Whisper Large v3", true))).1.contains("which is installed"));
        let (title, body, add, _) = read_prompt_text("de");
        assert_eq!(title, "You spoke German · Add it to My languages?");
        assert_eq!(body, "Then AI cleanup keeps your German dictations in German.");
        assert_eq!(add, "Add German");
    }

    #[test]
    fn the_record_round_trips() {
        let rec = answered(Record { last_offered_at: 42, ..Default::default() }, "de", "never");
        let back: Record = serde_json::from_str(&serde_json::to_string(&rec).unwrap()).unwrap();
        assert_eq!(back, rec);
        let empty: Record = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, Record::default());
    }
}

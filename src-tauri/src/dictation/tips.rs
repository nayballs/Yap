//! **Feature tips** (2026-10-29; Wispr Flow's "self-correction caught",
//! "listed", "email formatted", by behaviour): the first time a dictation
//! shows off something Yap did by itself, a short tip says so — once each,
//! ever.
//!
//! - **What it noticed** ([`Seen::of`], pure): AI cleanup caught a
//!   self-correction ("…on Wednesday, no wait, Thursday" → only Thursday
//!   kept: [`self_correction`]), laid out a spoken list ([`laid_out_list`])
//!   or framed an email ([`framed_email`], `ai/cleanup_format.rs`); a
//!   snippet expanded; a word was fixed from the dictionary.
//! - **Once each** ([`due`], pure): each kind shows once, ever, and never
//!   within [`MIN_GAP_MS`] of another; one tip per dictation, the first due
//!   in [`ORDER`].
//! - **Through the notice policy**, as a tip ("featureTip": "Show tips"
//!   turns it off, never over a dictation or a meeting, dropped if unseen
//!   a minute): after the paste, the main window's toast while the person
//!   looks at it (`yap-feature-tip` → `lib/notices/featureTips.js`), else a
//!   Yap bar card — never a Windows notification. Counted only when shown,
//!   in `<data>/hints.json` beside the push-to-talk hints (`coaching.rs`).
//!
//! Test mode notices nothing unless a spec turns it on (`e2e_feature_tips`).
//! The log says the kind, never the words.

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::ai::cleanup_format;
use crate::ai::cleanup_styles::{Category, Style};

/// Feature tips keep this far apart (any two kinds).
pub const MIN_GAP_MS: u64 = 10 * 60_000;
/// The main window's event: a [`Tip`].
const EVENT: &str = "yap-feature-tip";
/// The bar card's id and its rule in `notice.rs`.
pub const KIND: &str = "featureTip";
const CARD_MS: u64 = 12_000;

/// A kind of feature tip (its key in hints.json's `features`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Feature {
    SelfCorrection,
    List,
    Email,
    Snippet,
    Dictionary,
}

/// Which tip goes first when a dictation shows off several.
pub const ORDER: [Feature; 5] = [Feature::SelfCorrection, Feature::List, Feature::Email, Feature::Snippet, Feature::Dictionary];

impl Feature {
    pub fn key(self) -> &'static str {
        match self {
            Feature::SelfCorrection => "selfCorrection",
            Feature::List => "list",
            Feature::Email => "email",
            Feature::Snippet => "snippet",
            Feature::Dictionary => "dictionary",
        }
    }

    /// The tip's title and body.
    pub fn text(self) -> (&'static str, &'static str) {
        match self {
            Feature::SelfCorrection => (
                "Yap caught your correction",
                "You changed your mind mid-sentence (\u{201c}no wait\u{2026}\u{201d}), so AI cleanup kept only what you meant. Talk the way you'd talk.",
            ),
            Feature::List => (
                "Yap made that a list",
                "You said your points one by one, so AI cleanup laid them out as a list. Say \u{201c}first\u{2026} second\u{2026}\u{201d} or \u{201c}bullet points\u{201d} any time.",
            ),
            Feature::Email => (
                "Yap laid that out as an email",
                "Your greeting and sign-off went on lines of their own. Start with \u{201c}Hi\u{201d} and end with \u{201c}Thanks\u{201d} in any email app.",
            ),
            Feature::Snippet => (
                "Your snippet expanded",
                "You said a snippet's phrase, so Yap typed its text. Save more of them in Dictionary.",
            ),
            Feature::Dictionary => (
                "Fixed from your dictionary",
                "Yap spelled a word the way your dictionary says. Add any word it gets wrong in Dictionary.",
            ),
        }
    }
}

// ---- what a dictation showed off (pure) ---------------------------------------------------------

/// Lower-case words, apostrophes kept.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// The words people say when they take back what they just said.
const CORRECTIONS: &[&[&str]] = &[
    &["no", "wait"],
    &["wait", "no"],
    &["no", "no"],
    &["i", "mean"],
    &["sorry"],
    &["scratch", "that"],
    &["make", "that"],
    &["or", "rather"],
    &["correction"],
    &["actually", "no"],
];

/// Where `phrase` starts in `w`, past its first `from` words.
fn find(w: &[String], phrase: &[&str], from: usize) -> Option<usize> {
    (from..w.len().saturating_sub(phrase.len() - 1)).find(|&i| w[i..i + phrase.len()].iter().map(String::as_str).eq(phrase.iter().copied()))
}

/// AI cleanup caught a self-correction: the transcript (`raw`) takes
/// something back mid-sentence ("on Wednesday, no wait, Thursday": a
/// correction's words after two or more words, with more after them), and
/// the cleaned text (`cleaned`) has the correction's words gone and at
/// least one more word gone with them (what was taken back). Pure.
pub fn self_correction(raw: &str, cleaned: &str) -> bool {
    let (r, c) = (words(raw), words(cleaned));
    CORRECTIONS.iter().any(|phrase| {
        let Some(at) = find(&r, phrase, 2) else { return false };
        at + phrase.len() < r.len() && find(&c, phrase, 0).is_none() && r.len() > c.len() + phrase.len()
    })
}

/// A line that starts a list item: "1. …", "2) …", "- …", "• …".
fn list_item(line: &str) -> bool {
    let t = line.trim_start();
    if let Some(rest) = t.strip_prefix(['-', '•', '*']) {
        return rest.starts_with(' ');
    }
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && t[digits..].starts_with(['.', ')']) && t[digits + 1..].starts_with(' ')
}

/// AI cleanup laid out a spoken list: the transcript says one
/// (`cleanup_format::list_cue`) on one line, and the cleaned text has two
/// list lines or more. Pure.
pub fn laid_out_list(raw: &str, cleaned: &str) -> bool {
    !raw.contains('\n') && cleanup_format::list_cue(raw).is_some() && cleaned.lines().filter(|l| list_item(l)).count() >= 2
}

/// AI cleanup framed an email: a dictation into an email app opening with
/// a greeting or closing with a sign-off (`cleanup_format::email_frame`),
/// said on one line, came out on several. Pure.
pub fn framed_email(raw: &str, cleaned: &str, style: Option<Style>) -> bool {
    let (greeting, sign_off) = cleanup_format::email_frame(raw);
    style.is_some_and(|s| s.category == Category::Email) && (greeting || sign_off) && !raw.contains('\n') && cleaned.trim().contains('\n')
}

/// What a dictation showed off.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Seen {
    pub self_correction: bool,
    pub list: bool,
    pub email: bool,
    pub snippet: bool,
    pub dictionary: bool,
}

impl Seen {
    /// A dictation's transcript (`raw`), its text after AI cleanup
    /// (`cleaned`; the transcript when cleanup was off or failed) and the
    /// style it was written in: what Yap did to it by itself. The
    /// dictionary and snippets are looked for in the cleaned text, as the
    /// pipeline applies them. Pure.
    pub fn of(raw: &str, cleaned: &str, dictionary: &[crate::config::DictionaryEntry], style: Option<Style>) -> Seen {
        let (raw, cleaned) = (raw.trim(), cleaned.trim());
        let cleanup = cleaned != raw;
        Seen {
            self_correction: cleanup && self_correction(raw, cleaned),
            list: cleanup && laid_out_list(raw, cleaned),
            email: cleanup && framed_email(raw, cleaned, style),
            snippet: !dictionary.is_empty() && crate::config::apply_snippets(cleaned, dictionary) != cleaned,
            dictionary: !dictionary.is_empty() && crate::config::apply_dictionary(cleaned, dictionary) != cleaned,
        }
    }

    fn has(&self, f: Feature) -> bool {
        match f {
            Feature::SelfCorrection => self.self_correction,
            Feature::List => self.list,
            Feature::Email => self.email,
            Feature::Snippet => self.snippet,
            Feature::Dictionary => self.dictionary,
        }
    }
}

// ---- once each (pure) ---------------------------------------------------------------------------

/// The tip to show for `seen` at `now` (unix ms), given those shown
/// (`shown`: kind → when): the first in [`ORDER`] it showed off that hasn't
/// been shown, unless another tip showed under [`MIN_GAP_MS`] ago. Pure.
pub fn due(seen: &Seen, shown: &std::collections::BTreeMap<String, u64>, now: u64) -> Option<Feature> {
    // (A clock set back waits rather than repeating.)
    let recent = shown.values().any(|&at| now < at || now - at < MIN_GAP_MS);
    if recent {
        return None;
    }
    ORDER.into_iter().find(|f| seen.has(*f) && !shown.contains_key(f.key()))
}

// ---- after a dictation --------------------------------------------------------------------------

/// What the page gets (`yap-feature-tip`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tip {
    pub kind: Feature,
    pub title: &'static str,
    pub body: &'static str,
}

/// Test mode: tips are noticed only once a spec asks (`e2e_feature_tips`).
#[cfg(debug_assertions)]
static E2E_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn noticing() -> bool {
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() {
        return E2E_ON.load(std::sync::atomic::Ordering::SeqCst);
    }
    true
}

/// A dictation was pasted, having shown off `seen`: its tip, if one is
/// due, once the dictation is over (never mid-dictation), within a minute.
/// Off the pipeline's locks: a thread of its own.
pub fn after_dictation(app: &AppHandle, seen: Seen) {
    if seen == Seen::default() || !noticing() || crate::notices::notice::tips_off() {
        return;
    }
    let Some(feature) = due(&seen, &crate::dictation::coaching::record().features, crate::notices::notice::now_ms()) else {
        return;
    };
    let app = app.clone();
    let _ = std::thread::Builder::new().name("dictation-feature-tip".into()).spawn(move || {
        let started = std::time::Instant::now();
        std::thread::sleep(std::time::Duration::from_millis(400));
        while crate::notices::notice::moment().dictating && started.elapsed() < std::time::Duration::from_secs(60) {
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        offer(&app, feature);
    });
}

/// Show `feature`'s tip if it's still due: the main window's toast while
/// the person looks at it, else a Yap bar card; nowhere else (a tip can go
/// unsaid). Counted only when shown.
fn offer(app: &AppHandle, feature: Feature) {
    if crate::notices::notice::tips_off() {
        return;
    }
    let now = crate::notices::notice::now_ms();
    let seen = Seen {
        self_correction: feature == Feature::SelfCorrection,
        list: feature == Feature::List,
        email: feature == Feature::Email,
        snippet: feature == Feature::Snippet,
        dictionary: feature == Feature::Dictionary,
    };
    if due(&seen, &crate::dictation::coaching::record().features, now) != Some(feature) {
        return;
    }
    let (title, body) = feature.text();
    let place = crate::notices::notice::place_tip(app, KIND);
    let shown = match place {
        crate::notices::notice::Place::Window => app.emit(EVENT, Tip { kind: feature, title, body }).is_ok(),
        crate::notices::notice::Place::Bar => {
            let card = crate::bar::Card {
                id: KIND.into(),
                icon: "tip",
                title: title.into(),
                body: body.into(),
                timeout_ms: Some(CARD_MS),
                ..Default::default()
            };
            crate::bar::show_card(app, card, Box::new(|_, _| {}))
        }
        _ => false,
    };
    if shown {
        tracing::info!(kind = feature.key(), window = place == crate::notices::notice::Place::Window, "Feature tips: shown");
        crate::dictation::coaching::update_record(|mut r| {
            r.features.insert(feature.key().to_string(), now);
            r
        });
    } else {
        tracing::debug!(kind = feature.key(), "Feature tips: nowhere to show it");
    }
}

/// Test mode only: notice feature tips (`on`), with those shown forgotten so
/// every one is due.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_feature_tips(on: bool) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    E2E_ON.store(on, std::sync::atomic::Ordering::SeqCst);
    crate::dictation::coaching::update_record(|mut r| {
        r.features.clear();
        r
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    const T0: u64 = 1_790_000_000_000;

    #[test]
    fn a_self_correction_caught_by_cleanup() {
        assert!(self_correction("let's meet on Wednesday no wait Thursday at ten", "Let's meet on Thursday at ten."));
        assert!(self_correction("send it to Priya sorry to Sam today", "Send it to Sam today."));
        assert!(self_correction("book a table for four I mean five people", "Book a table for five people."));
        // The correction kept by cleanup: nothing caught.
        assert!(!self_correction("meet on Wednesday no wait Thursday", "Meet on Wednesday, no wait, Thursday."));
        // Only a filler gone ("actually" alone is speech, not a correction).
        assert!(!self_correction("I actually think it works", "I think it works."));
        // "Sorry" opening a sentence, kept.
        assert!(!self_correction("I'm so sorry about the delay", "I'm so sorry about the delay."));
        // At the very start or end there's nothing taken back.
        assert!(!self_correction("sorry I'm late", "I'm late."));
        assert!(!self_correction("see you on Thursday no wait", "See you on Thursday."));
    }

    #[test]
    fn a_spoken_list_laid_out() {
        let raw = "things to bring first the tent second the stove third the maps";
        assert!(laid_out_list(raw, "Things to bring:\n1. The tent\n2. The stove\n3. The maps"));
        assert!(laid_out_list("bullet points milk eggs bread", "- Milk\n- Eggs\n- Bread"));
        // Left as a sentence.
        assert!(!laid_out_list(raw, "Things to bring: first the tent, second the stove, third the maps."));
        // No list said: never.
        assert!(!laid_out_list("one cat and two dogs", "1. One cat\n2. Two dogs"));
        assert!(list_item("12) Twelve"));
        assert!(!list_item("2026 was a year"));
        assert!(!list_item("-5 degrees"));
    }

    #[test]
    fn an_email_framed_only_in_an_email_app() {
        let email = Some(Style { category: Category::Email, tone: crate::ai::cleanup_styles::Tone::Formal });
        let raw = "hi Priya thanks for the notes I'll sign it tomorrow best regards Tom";
        let out = "Hi Priya,\n\nThanks for the notes. I'll sign it tomorrow.\n\nBest regards,\nTom";
        assert!(framed_email(raw, out, email));
        assert!(!framed_email(raw, "Hi Priya, thanks for the notes. Best regards, Tom.", email));
        assert!(!framed_email(raw, out, Some(Style { category: Category::WorkChat, tone: crate::ai::cleanup_styles::Tone::Casual })));
        assert!(!framed_email(raw, out, None));
        assert!(!framed_email("thanks for the notes I'll sign it tomorrow", "Thanks for the notes.\nI'll sign it.", email));
    }

    fn entry(from: &str, to: &str, snippet: bool) -> crate::config::DictionaryEntry {
        crate::config::DictionaryEntry { from: from.into(), to: to.into(), fuzzy: true, snippet, starred: false, also_heard: Vec::new() }
    }

    #[test]
    fn snippets_and_dictionary_words_are_seen_in_the_cleaned_text() {
        let dict = [entry("jaison", "JSON", false), entry("my sign off", "Best wishes,\nSam", true)];
        let seen = Seen::of("parse the jaison", "Parse the jaison.", &dict, None);
        assert!(seen.dictionary && !seen.snippet);
        let seen = Seen::of("thanks my sign off", "Thanks, my sign off.", &dict, None);
        assert!(seen.snippet && !seen.dictionary);
        assert_eq!(Seen::of("nothing here", "nothing here", &dict, None), Seen::default());
        assert_eq!(Seen::of("parse the jaison", "parse the jaison", &[], None), Seen::default());
        // Cleanup off: no cleanup tips, however the words go.
        let off = Seen::of("meet Wednesday no wait Thursday", "meet Wednesday no wait Thursday", &[], None);
        assert!(!off.self_correction);
    }

    #[test]
    fn each_tip_once_and_ten_minutes_apart() {
        let both = Seen { self_correction: true, dictionary: true, ..Seen::default() };
        let mut shown = BTreeMap::new();
        assert_eq!(due(&both, &shown, T0), Some(Feature::SelfCorrection));
        shown.insert("selfCorrection".to_string(), T0);
        // Not another within ten minutes…
        assert_eq!(due(&both, &shown, T0 + MIN_GAP_MS - 1), None);
        // …then the next one it showed off; never the same one again.
        assert_eq!(due(&both, &shown, T0 + MIN_GAP_MS), Some(Feature::Dictionary));
        shown.insert("dictionary".to_string(), T0 + MIN_GAP_MS);
        assert_eq!(due(&both, &shown, T0 + 365 * 24 * 3_600_000), None);
        // Nothing shown off: nothing.
        assert_eq!(due(&Seen::default(), &BTreeMap::new(), T0), None);
        // A clock set back waits rather than repeating.
        assert_eq!(due(&Seen { list: true, ..Seen::default() }, &shown, T0 - 1_000), None);
    }

    #[test]
    fn the_counts_live_beside_the_hints_and_an_older_file_reads() {
        let mut r = crate::dictation::coaching::Record::default();
        r.features.insert("list".into(), T0);
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"features\":{\"list\":"), "{json}");
        let back: crate::dictation::coaching::Record = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
        // A file from before has none, and an empty map isn't written.
        let old: crate::dictation::coaching::Record = serde_json::from_str(r#"{"tap":1}"#).unwrap();
        assert!(old.features.is_empty() && !old.dictated);
        assert!(!serde_json::to_string(&old).unwrap().contains("features"));
    }

    #[test]
    fn every_tip_has_words() {
        for f in ORDER {
            let (title, body) = f.text();
            assert!(!title.is_empty() && body.len() > 30, "{f:?}");
        }
    }
}

//! What people say: an open request ("open Teams", "can you launch
//! Spotify"), and the app it names, matched as people say names —
//! without the maker, by initials, numbers however they're said, near
//! misses and names that sound the same.

use crate::config::AppNickname;

use super::index::App;
use super::{CONFIDENT, MARGIN, MAX_LISTED};

/// A message that asks to open something ([`open_request`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// What to open, as said (lower case): "teams", "the calculator".
    pub phrase: String,
    /// No joining words ("and", "about", "for"…): only an app's name, by the
    /// look of it. "open the notes about Teams" isn't plain.
    pub plain: bool,
    /// "play …": most likely a game, but maybe a song — a phrase that names
    /// no app is the model's ("play the next song"), never "I couldn't find
    /// an app called…".
    pub play: bool,
}

/// What comes before the verb, longest first: greetings, "please", "can you"…
const LEAD_INS: &[&str] = &[
    "could you please", "can you please", "would you please", "will you please", "would you mind",
    "go ahead and", "i need you to", "i want you to", "i would like to", "i'd like to", "i want to",
    "i need to", "i wanna", "hey yap", "hi yap", "ok yap", "okay yap", "could you", "can you",
    "would you", "will you", "help me", "try and", "try to", "quickly", "kindly", "please", "just",
    "now", "let's", "lets", "okay", "alright", "yeah", "yes", "sure", "hey", "yap", "ok", "hi",
];
/// The verbs of an open request, longest first.
const VERBS: &[&str] = &[
    "open up", "fire up", "start up", "boot up", "bring up", "pull up", "load up", "launch",
    "open", "start", "play", "run",
];
/// What may follow the app's name, longest first.
const TAILS: &[&str] = &[
    "for me please", "on this computer", "on my computer", "one more time", "for me now",
    "on this pc", "on my pc", "right now", "thank you", "for me", "please", "thanks", "again",
    "now", "up",
];
/// What "open it again" calls the app Yap just opened or suggested.
const BACK: &[&str] = &[
    "it", "that", "this", "that one", "this one", "that app", "this app", "that game", "this game",
    "app", "game", "same one", "same app", "same game",
];
/// Words that join an app's name to something else: a request with one is
/// "not plain" (it might be about a note, or two things at once).
const CONNECTORS: &[&str] = &[
    "and", "then", "about", "from", "with", "into", "in", "on", "at", "to", "of", "for", "that",
    "which", "where", "when", "if", "because", "so", "but", "or", "after", "before", "while",
    "using", "called", "named",
];
/// What people open that isn't an app (the last word of the phrase): "open
/// my notes", "start the meeting", "run the tests". Never answered "I
/// couldn't find an app called…".
const NOT_APPS: &[&str] = &[
    "note", "notes", "meeting", "meetings", "recording", "recordings", "dictation", "dictating",
    "transcript", "transcripts", "summary", "timer", "alarm", "reminder", "link", "links", "url",
    "tab", "tabs", "window", "file", "files", "folder", "folders", "document", "documents", "doc",
    "page", "website", "site", "email", "emails", "message", "messages", "conversation",
    "thread", "question", "list", "project", "session", "test", "tests", "server", "script",
    "command", "call", "calls", "typing", "listening", "it", "this", "that", "them", "one",
    "something", "everything", "anything", "chat", "again", "over",
];

/// Strip one of `list` from the start of `s` (followed by a space).
fn strip_lead<'a>(s: &'a str, list: &[&str]) -> Option<&'a str> {
    list.iter().find_map(|p| s.strip_prefix(p).and_then(|r| r.strip_prefix(' ')))
}

/// Strip one of `list` from the end of `s` (after a space).
fn strip_tail<'a>(s: &'a str, list: &[&str]) -> Option<&'a str> {
    list.iter().find_map(|p| s.strip_suffix(p).and_then(|r| r.strip_suffix(' ')))
}

/// Is `text` a request to open something — "open Teams", "launch Spotify",
/// "can you open notepad please", "start calculator", "Hey Yap, open up
/// the calculator for me", "try to open Baldur's Gate", "play World of
/// Warcraft", "open it again"? Then what (`phrase`: "it" for the last). English. The whole message
/// must be the request: "how do I open notepad", "the meeting will open at
/// 3" and "open notepad. Then…" aren't. Pure.
pub fn open_request(text: &str) -> Option<Request> {
    let lower = text.trim().to_lowercase().replace(['\u{2019}', '\u{2018}'], "'");
    let trimmed = lower
        .trim_matches(|c: char| matches!(c, '"' | '\u{201C}' | '\u{201D}') || c.is_whitespace())
        .trim_end_matches(|c: char| matches!(c, '.' | '!' | '?' | '\u{2026}') || c.is_whitespace());
    // One sentence, one line.
    if trimmed.contains('\n') || [". ", "? ", "! ", "; ", ": "].iter().any(|b| trimmed.contains(b)) {
        return None;
    }
    let spaced = trimmed.replace(',', " ");
    let s = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut rest = s.as_str();
    while let Some(r) = strip_lead(rest, LEAD_INS) {
        rest = r;
    }
    let play = rest.starts_with("play ");
    let mut rest = strip_lead(rest, VERBS)?;
    loop {
        if let Some(r) = strip_tail(rest, TAILS) {
            rest = r;
        } else if let Some(r) = rest.strip_prefix("up ").or_else(|| rest.strip_prefix("the ")) {
            rest = r;
        } else {
            break;
        }
    }
    let phrase = rest.trim();
    let words: Vec<&str> = phrase.split(' ').collect();
    if phrase.is_empty()
        || words.len() > 6
        || ["a", "an", "some", "another", "new", "more", "all"].contains(&words[0])
    {
        return None;
    }
    let plain = !words.iter().any(|w| CONNECTORS.contains(w));
    Some(Request { phrase: phrase.to_string(), plain, play })
}

/// "open it", "open that one again": the phrase means the app Yap just
/// opened or suggested.
pub(super) fn refers_back(phrase: &str) -> bool {
    BACK.contains(&phrase)
}

/// Does the person's message ask to open something at all? `open_app`
/// only acts when it does: the model may only open an app the person asked
/// for in this turn, never one a note, a transcript or a web page names.
/// An open verb said as a request — starting a clause, or after "please",
/// "can you", "to"… ("Could you open Spotify", "I want to launch Teams") —
/// not "the open questions" or "the start of the note". Pure.
pub fn asked_to_open(question: &str) -> bool {
    const VERBS: &[&str] = &["open", "launch", "start", "run", "boot", "reopen", "play"];
    const PAIRS: &[(&str, &str)] = &[("fire", "up"), ("bring", "up"), ("pull", "up"), ("load", "up"), ("switch", "to")];
    /// Words a request's verb may follow.
    const BEFORE: &[&str] = &[
        "please", "pls", "you", "to", "and", "then", "just", "now", "also", "yap", "hey", "ok", "okay",
        "can", "could", "would", "will", "go", "lets", "let's", "me", "quickly", "kindly",
    ];
    let lower = question.to_lowercase().replace('\u{2019}', "'");
    lower.split(['.', ',', ';', ':', '!', '?', '\n', '(', ')']).any(|clause| {
        let words: Vec<&str> =
            clause.split(|c: char| !c.is_alphanumeric() && c != '\'').filter(|w| !w.is_empty()).collect();
        (0..words.len()).any(|i| {
            let placed = i == 0 || BEFORE.contains(&words[i - 1]);
            let next = words.get(i + 1).copied().unwrap_or("");
            placed
                && ((VERBS.contains(&words[i]) && !(words[i] == "start" && next == "of"))
                    || PAIRS.contains(&(words[i], next)))
        })
    })
}

/// "open my notes", "start the meeting": the phrase names something that
/// isn't an app.
pub(super) fn not_an_app(phrase: &str) -> bool {
    phrase.split(' ').next_back().is_some_and(|w| NOT_APPS.contains(&w))
}

// ---- matching --------------------------------------------------------------------

/// Words that mean nothing in an app's name or a request for one.
const FILLER: &[&str] = &["the", "app", "application", "program", "please"];
/// Makers whose name comes first ("Microsoft Teams" is "Teams").
const VENDORS: &[&str] = &["microsoft", "google", "adobe", "mozilla", "apple", "windows", "amazon", "meta"];
/// Last words too common to name an app by ("Snipping Tool" isn't "tool").
const GENERIC: &[&str] = &[
    "tool", "tools", "manager", "settings", "editor", "player", "client", "launcher", "viewer",
    "console", "center", "centre", "studio", "helper", "setup", "installer", "updater", "service",
    "services", "monitor", "system", "desktop", "online", "classic", "edition", "prompt", "shell",
    "app", "apps", "panel", "kit", "information", "configuration", "preview", "beta",
];
/// What people call some apps → the words of the app's name.
pub const ALIASES: &[(&str, &str)] = &[
    ("explorer", "file explorer"),
    ("windows explorer", "file explorer"),
    ("file manager", "file explorer"),
    ("files", "file explorer"),
    ("my computer", "file explorer"),
    ("this pc", "file explorer"),
    ("calc", "calculator"),
    ("cmd", "command prompt"),
    ("command line", "command prompt"),
    ("windows settings", "settings"),
    ("pc settings", "settings"),
    ("system settings", "settings"),
    ("vs code", "visual studio code"),
    ("vscode", "visual studio code"),
    ("store", "microsoft store"),
    ("app store", "microsoft store"),
    ("ms paint", "paint"),
    ("mspaint", "paint"),
    ("snip", "snipping tool"),
    ("screenshot tool", "snipping tool"),
];

/// The words of a name or a request: lower case, letters and digits ("+"
/// kept for "Notepad++"), apostrophes dropped, [`FILLER`] out.
pub(super) fn words(s: &str) -> Vec<String> {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\'' | '\u{2019}' => {}
            '\u{A789}' => out.push(' '), // "AMD Software꞉ Adrenalin Edition"
            c if c.is_alphanumeric() || c == '+' => out.extend(c.to_lowercase()),
            _ => out.push(' '),
        }
    }
    out.split_whitespace().filter(|w| !FILLER.contains(w)).map(str::to_string).collect()
}

/// Numbers said as words, one to twenty.
const NUMBER_WORDS: &[&str] = &[
    "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve",
    "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen", "twenty",
];
/// Roman numerals in names ("Baldur's Gate III", "Diablo IV"), II to X.
const ROMAN: &[&str] = &["ii", "iii", "iv", "v", "vi", "vii", "viii", "ix", "x"];

/// Words with every number as digits: "three" and "iii" are "3". ("i" is
/// left alone: it's a word.)
pub(super) fn numerals(words: &[String]) -> Vec<String> {
    words
        .iter()
        .map(|w| {
            if let Some(i) = NUMBER_WORDS.iter().position(|n| n == w) {
                (i + 1).to_string()
            } else if let Some(i) = ROMAN.iter().position(|n| n == w) {
                (i + 2).to_string()
            } else {
                w.clone()
            }
        })
        .collect()
}

/// A name without its qualifiers: "(64-bit)", " - Insiders", versions
/// ("3.12", "2025.5.2"), architectures ("x64").
fn strip_qualifiers(name: &str) -> String {
    use std::sync::OnceLock;
    static BRACKETS: OnceLock<regex::Regex> = OnceLock::new();
    static DASH: OnceLock<regex::Regex> = OnceLock::new();
    static VERSION: OnceLock<regex::Regex> = OnceLock::new();
    let brackets = BRACKETS.get_or_init(|| regex::Regex::new(r"\([^)]*\)|\[[^\]]*\]|[™®©]").expect("static regex"));
    let dash = DASH.get_or_init(|| regex::Regex::new(r"\s[-\u{2013}\u{2014}]\s.*$").expect("static regex"));
    let version = VERSION.get_or_init(|| {
        regex::Regex::new(r"(?i)\b(?:v?\d+(?:[._-]\d+)*(?:-?bits?)?|x64|x86|x86_64|arm64|amd64|win64|win32)\b")
            .expect("static regex")
    });
    let s = brackets.replace_all(name, " ");
    let s = dash.replace(&s, "");
    version.replace_all(&s, " ").into_owned()
}

/// A way to name an app, and how good a match on it is.
#[derive(Debug, Clone)]
struct Key {
    words: Vec<String>,
    tier: f64,
    /// Whether a request may be a few of its words ("visual studio").
    subset: bool,
}

/// The names an app goes by, best first: its name, without qualifiers,
/// without its maker, its initials, its last word.
fn keys(name: &str) -> Vec<Key> {
    let mut out: Vec<Key> = Vec::new();
    let mut push = |words: Vec<String>, tier: f64, subset: bool| {
        if !words.is_empty() && !out.iter().any(|k| k.words == words) {
            out.push(Key { words, tier, subset });
        }
    };
    let full = words(name);
    let stripped = words(&strip_qualifiers(name));
    push(full.clone(), 1.0, true);
    push(stripped.clone(), 0.99, true);
    for (base, tier) in [(&full, 0.96), (&stripped, 0.95)] {
        if base.len() > 1 && VENDORS.contains(&base[0].as_str()) {
            push(base[1..].to_vec(), tier, true);
        }
    }
    let base = if stripped.is_empty() { &full } else { &stripped };
    if base.len() >= 2 {
        let initials: String = base.iter().filter_map(|w| w.chars().next()).filter(|c| c.is_alphabetic()).collect();
        if initials.chars().count() == base.len() {
            push(vec![initials.clone()], 0.86, false);
            let (last, head) = base.split_last().expect("two words");
            let head: String = head.iter().filter_map(|w| w.chars().next()).collect();
            push(vec![head, last.clone()], 0.86, false);
        }
        let last = &base[base.len() - 1];
        if last.chars().count() >= 4 && !GENERIC.contains(&last.as_str()) && !last.chars().all(|c| c.is_ascii_digit()) {
            push(vec![last.clone()], 0.84, false);
        }
    }
    // Numbers as digits: "Baldur's Gate III" is "baldurs gate 3" too.
    let spelled: Vec<Key> = out.iter().filter(|k| numerals(&k.words) != k.words).cloned().collect();
    for k in spelled {
        let digits = numerals(&k.words);
        if !out.iter().any(|o| o.words == digits) {
            out.push(Key { words: digits, ..k });
        }
    }
    out
}

/// How well the request's words `q` name the key `k`, 0 … its tier.
fn match_key(q: &[String], k: &Key) -> f64 {
    let (qc, kc) = (q.concat(), k.words.concat());
    if qc.is_empty() {
        return 0.0;
    }
    if qc == kc {
        return k.tier;
    }
    let mut best: f64 = 0.0;
    // A few of its words: "visual studio" for "Visual Studio Code".
    if k.subset
        && q.len() < k.words.len()
        && q.iter().any(|w| w.chars().count() >= 3)
        && q.iter().all(|w| k.words.contains(w))
    {
        best = best.max(k.tier * (0.80 + 0.12 * q.len() as f64 / k.words.len() as f64));
    }
    // A near-miss: "spottify", "teems" (not "spotifire": too many letters
    // apart, however alike it sounds).
    if qc.chars().count() >= 4 && kc.chars().count() >= 4 {
        let (d, _) = crate::dictionary::fuzzy::spelling_distance(&qc, &kc);
        if d <= 0.2 && crate::dictionary::fuzzy::plain_spelling_distance(&qc, &kc) <= 0.3 {
            best = best.max(k.tier * (1.0 - d) * 0.92);
        }
    }
    best
}

/// The ways to read a request: as said, without "my", without a maker
/// ("microsoft word" → "word"), and what an alias stands for. With weights.
pub(super) fn query_forms(query: &str) -> Vec<(Vec<String>, f64)> {
    let q = words(query);
    let mut forms = vec![(q.clone(), 1.0)];
    if q.len() > 1 && ["my", "your"].contains(&q[0].as_str()) {
        forms.push((q[1..].to_vec(), 1.0));
    }
    let base = forms.last().map(|f| f.0.clone()).unwrap_or_default();
    if base.len() > 1 && VENDORS.contains(&base[0].as_str()) {
        forms.push((base[1..].to_vec(), 0.98));
    }
    for (said, means) in ALIASES {
        if forms.iter().any(|(f, _)| f.concat() == words(said).concat()) {
            forms.push((words(means), 0.97));
        }
    }
    // Numbers as digits: "baldurs gate three" is "baldurs gate 3".
    let spelled: Vec<(Vec<String>, f64)> = forms.iter().filter(|(f, _)| numerals(f) != *f).cloned().collect();
    for (f, w) in spelled {
        forms.push((numerals(&f), w));
    }
    forms
}

/// How well `query` names an app called `name`, 0 … 1.
pub(super) fn score(query_forms: &[(Vec<String>, f64)], name: &str) -> f64 {
    let keys = keys(name);
    query_forms
        .iter()
        .flat_map(|(q, w)| keys.iter().map(move |k| w * match_key(q, k)))
        .fold(0.0, f64::max)
}

/// What a request names.
#[derive(Debug, Clone)]
pub enum Found {
    /// One app.
    One(App),
    /// Several, about as likely (best first): ask which.
    Several(Vec<App>),
    /// One the person switched off ("Don't open this one").
    Blocked(App),
    /// None; the closest names, if any.
    Nothing(Vec<App>),
}

/// The app `query` names among `apps`: a nickname first, then the best
/// match if it's good enough and nothing else is as good. Apps in
/// `blocked` are never `One`. Pure.
pub fn find(apps: &[App], query: &str, nicknames: &[AppNickname], blocked: &[String]) -> Found {
    let q = words(query).concat();
    if q.is_empty() {
        return Found::Nothing(Vec::new());
    }
    // A nickname the person set: exactly, or nearly.
    let nick = nicknames
        .iter()
        .filter_map(|n| {
            let said = words(&n.say).concat();
            if said.is_empty() {
                return None;
            }
            let d = if said == q { 0.0 } else { crate::dictionary::fuzzy::spelling_distance(&said, &q).0 };
            let near = said.chars().count() >= 4 && d <= 0.1 && crate::dictionary::fuzzy::plain_spelling_distance(&said, &q) <= 0.25;
            (said == q || near).then_some((d, n))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0));
    if let Some((_, n)) = nick {
        if let Some(app) = apps.iter().find(|a| a.id == n.app) {
            return if blocked.contains(&app.id) { Found::Blocked(app.clone()) } else { Found::One(app.clone()) };
        }
    }
    let forms = query_forms(query);
    let mut scored: Vec<(f64, &App)> = apps.iter().map(|a| (score(&forms, &a.name), a)).filter(|(s, _)| *s > 0.0).collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.name.len().cmp(&b.1.name.len())));
    let top = scored.first().map_or(0.0, |s| s.0);
    if top >= CONFIDENT {
        let group: Vec<&App> = scored.iter().filter(|(s, _)| *s >= CONFIDENT && *s >= top - MARGIN).map(|(_, a)| *a).collect();
        let allowed: Vec<&App> = group.iter().copied().filter(|a| !blocked.contains(&a.id)).collect();
        return match allowed.as_slice() {
            [] => Found::Blocked(group[0].clone()),
            [one] => Found::One((*one).clone()),
            several => Found::Several(several.iter().take(MAX_LISTED).map(|a| (*a).clone()).collect()),
        };
    }
    // A mishearing the spelling checks miss: one app whose name sounds the
    // same ("boulders gate" / Baldur's Gate), and nothing else close.
    let heard: Vec<&App> = apps.iter().filter(|a| sounds_like(&forms, &a.name)).collect();
    if let [one] = heard.as_slice() {
        let rival = scored.iter().any(|(s, a)| a.id != one.id && *s >= HEARD_RIVAL)
            || apps.iter().any(|a| a.id != one.id && sound_of(&forms, &a.name).is_some_and(|(d, _)| d <= HEARD_RIVAL_SOUND));
        if !rival {
            return if blocked.contains(&one.id) { Found::Blocked((*one).clone()) } else { Found::One((*one).clone()) };
        }
    }
    let mut close: Vec<App> = heard.into_iter().filter(|a| !blocked.contains(&a.id)).take(3).cloned().collect();
    for a in close_names(apps, query, blocked) {
        if close.len() < 3 && !close.iter().any(|c| c.id == a.id) {
            close.push(a);
        }
    }
    Found::Nothing(close)
}

/// An app matched by sound alone opens only when no other app scores this
/// well…
const HEARD_RIVAL: f64 = 0.6;
/// …nor sounds this close.
const HEARD_RIVAL_SOUND: f64 = 0.25;

/// How close a reading of the request sounds to the app's names
/// (`fuzzy::sound_distance`, numbers as digits, ≥ 5 sounds), and how far
/// apart the closest-sounding one is spelled
/// (`fuzzy::plain_spelling_distance`). `None`: nothing to compare (too
/// short, or spelled the same — that's a match, not a sound-alike).
fn sound_of(forms: &[(Vec<String>, f64)], name: &str) -> Option<(f64, f64)> {
    let keys = keys(name);
    let mut best: Option<(f64, f64)> = None;
    for (q, _) in forms {
        let qc = numerals(q).concat();
        if qc.chars().count() < 5 {
            continue;
        }
        for k in keys.iter().filter(|k| k.subset) {
            let kc = numerals(&k.words).concat();
            if qc == kc {
                continue;
            }
            let (d, len) = crate::dictionary::fuzzy::sound_distance(&qc, &kc);
            if len < 5 {
                continue;
            }
            let pair = (d, crate::dictionary::fuzzy::plain_spelling_distance(&qc, &kc));
            if best.is_none_or(|b| pair.0 < b.0 || (pair.0 == b.0 && pair.1 < b.1)) {
                best = Some(pair);
            }
        }
    }
    best
}

/// Does the request sound like one of the app's names — the same sounds,
/// spelled not too differently ([`sound_of`])? "boulders gate three" /
/// "Baldur's Gate 3" yes; "spotifire" / "Spotify" no (it sounds longer).
pub(super) fn sounds_like(forms: &[(Vec<String>, f64)], name: &str) -> bool {
    sound_of(forms, name).is_some_and(|(d, spelled)| d <= 0.1 && spelled <= 0.45)
}

/// Apps whose names are close to `query`, for "Did you mean…?": a spelling
/// within reach, or a name holding the request (or held by it).
fn close_names(apps: &[App], query: &str, blocked: &[String]) -> Vec<App> {
    let q = words(query).concat();
    if q.chars().count() < 3 {
        return Vec::new();
    }
    let mut close: Vec<(f64, &App)> = apps
        .iter()
        .filter(|a| !blocked.contains(&a.id))
        .filter_map(|a| {
            let best = keys(&a.name)
                .iter()
                .filter(|k| k.subset)
                .map(|k| {
                    let kc = k.words.concat();
                    let d = crate::dictionary::fuzzy::spelling_distance(&q, &kc).0;
                    let held = kc.chars().count() >= 3 && (kc.contains(&q) || q.contains(&kc));
                    if held {
                        d.min(0.3)
                    } else {
                        d
                    }
                })
                .fold(1.0, f64::min);
            (best <= 0.45).then_some((best, a))
        })
        .collect();
    close.sort_by(|a, b| a.0.total_cmp(&b.0));
    close.into_iter().take(3).map(|(_, a)| a.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::index::tidy;
    use crate::apps::tests::{found_in, games, one};

    #[test]
    fn open_requests_are_recognised_in_many_phrasings() {
        let cases = [
            ("open Teams", "teams"),
            ("Open Teams.", "teams"),
            ("launch Spotify", "spotify"),
            ("can you open notepad please", "notepad"),
            ("Can you open the calculator?", "calculator"),
            ("start calculator", "calculator"),
            ("Could you please launch Microsoft Teams", "microsoft teams"),
            ("Hey Yap, open up the calculator for me.", "calculator"),
            ("Yap, open Teams", "teams"),
            ("please open spotify for me, thanks", "spotify"),
            ("I'd like to open VS Code", "vs code"),
            ("fire up spotify", "spotify"),
            ("run notepad", "notepad"),
            ("Open File Explorer!", "file explorer"),
            ("open my editor", "my editor"),
            ("go ahead and open settings now", "settings"),
            ("\u{201C}Open Spotify.\u{201D}", "spotify"),
            ("open spotify up", "spotify"),
        ];
        for (text, phrase) in cases {
            let r = open_request(text).unwrap_or_else(|| panic!("{text:?} is an open request"));
            assert_eq!(r.phrase, phrase, "{text:?}");
            assert!(r.plain, "{text:?}");
        }
    }

    #[test]
    fn other_messages_are_not_open_requests() {
        for text in [
            "the meeting will open at 3",
            "how do I open notepad",
            "what's in my notes about Teams?",
            "Opening Teams",
            "don't open teams",
            "open",
            "open a new note",
            "start a timer",
            "open notepad. Then write a poem",
            "open notepad\nand type hello",
            "Did you open Spotify?",
            "summarise my last meeting",
        ] {
            assert_eq!(open_request(text), None, "{text:?}");
        }
        // Requests, but not plainly an app's name.
        for text in ["open the notes about Teams", "open spotify and play my liked songs", "open settings for bluetooth"] {
            let r = open_request(text).unwrap();
            assert!(!r.plain, "{text:?}");
        }
    }

    #[test]
    fn names_are_matched_as_people_say_them() {
        assert_eq!(one("teams"), "Microsoft Teams");
        assert_eq!(one("Microsoft Teams"), "Microsoft Teams");
        assert_eq!(one("notepad"), "Notepad");
        assert_eq!(one("note pad"), "Notepad");
        assert_eq!(one("the calculator"), "Calculator");
        assert_eq!(one("calc"), "Calculator");
        assert_eq!(one("spotify"), "Spotify");
        assert_eq!(one("Spotify app"), "Spotify");
        assert_eq!(one("VS Code"), "Visual Studio Code");
        assert_eq!(one("visual studio code"), "Visual Studio Code");
        assert_eq!(one("code"), "Visual Studio Code");
        assert_eq!(one("file explorer"), "File Explorer");
        assert_eq!(one("explorer"), "File Explorer");
        assert_eq!(one("files"), "File Explorer");
        assert_eq!(one("settings"), "Settings");
        assert_eq!(one("chrome"), "Google Chrome");
        assert_eq!(one("edge"), "Microsoft Edge");
        assert_eq!(one("cmd"), "Command Prompt");
        assert_eq!(one("ChatGPT"), "ChatGPT");
        assert_eq!(one("chat gpt"), "ChatGPT");
        assert_eq!(one("task manager"), "Task Manager");
        assert_eq!(one("terminal"), "Terminal");
        assert_eq!(one("my spotify"), "Spotify");
        assert_eq!(one("stardew valley"), "Stardew Valley");
        assert_eq!(one("python 3.12"), "Python 3.12 (64-bit)");
        // Near-misses a speech model makes.
        assert_eq!(one("spottify"), "Spotify");
        assert_eq!(one("teems"), "Microsoft Teams");
        assert_eq!(one("calculater"), "Calculator");
        assert_eq!(one("dis cord"), "Discord");
    }

    #[test]
    fn asked_to_open_reads_the_persons_own_words() {
        for q in [
            "open spotify",
            "Could you launch Teams?",
            "start the calculator",
            "fire up VS Code",
            "switch to Teams",
            "Run notepad",
            "I'd love some music, could you open Spotify and then tell me a joke?",
            "I want to open the editor",
            "Summarise my notes and then open Teams",
            "Hey Yap, please start Spotify",
        ] {
            assert!(asked_to_open(q), "{q}");
        }
        for q in [
            "summarise my notes",
            "what's on my calendar",
            "tell me about Spotify",
            "opener",
            "Summarise the open questions in my notes",
            "What's at the start of my launch note?",
            "list the open tasks",
            "how did the run go",
        ] {
            assert!(!asked_to_open(q), "{q}");
        }
    }

    #[test]
    fn games_are_found_however_their_names_are_said() {
        let apps = games();
        // Numbers as words, digits or Roman numerals; apostrophes or none.
        for q in ["baldur's gate 3", "Baldur's Gate Three", "baldurs gate three", "baldurs gate 3", "Baldur's Gate III"] {
            assert_eq!(found_in(&apps, q), "Baldur's Gate 3", "{q}");
        }
        assert_eq!(found_in(&apps, "diablo four"), "Diablo IV");
        assert_eq!(found_in(&apps, "diablo 4"), "Diablo IV");
        // Mishearings: by spelling, or by sound when nothing else comes close.
        assert_eq!(found_in(&apps, "boulders gate three"), "Baldur's Gate 3");
        assert_eq!(found_in(&apps, "boulders gate"), "Baldur's Gate 3");
        assert_eq!(found_in(&apps, "bolders gait 3"), "Baldur's Gate 3");
        // World of Warcraft: the exact name, not Classic; Classic when said.
        assert_eq!(found_in(&apps, "world of warcraft"), "World of Warcraft");
        assert_eq!(found_in(&apps, "World of Warcraft Classic"), "World of Warcraft Classic");
        assert_eq!(found_in(&apps, "wow"), "World of Warcraft");
        assert_eq!(found_in(&apps, "battle.net"), "Battle.net");
        assert_eq!(found_in(&apps, "battlenet"), "Battle.net");
        assert_eq!(found_in(&apps, "battle net"), "Battle.net");
        assert_eq!(found_in(&apps, "steam"), "Steam");
        assert_eq!(found_in(&apps, "cataclysm classic"), "Cataclysm Classic");
        // By sound alone ("bolders gait" is too far off by spelling): only
        // while no other game sounds the same.
        assert_eq!(found_in(&apps, "bolders gait"), "Baldur's Gate 3");
        let mut two = games();
        two.push(App::linked("steam://rungameid/228280", "Baldur's Gate II", "steam://rungameid/228280", ""));
        let two = tidy(two);
        let guess = found_in(&two, "bolders gait");
        assert!(guess.starts_with("nothing; close: Baldur's Gate 3, Baldur's Gate II"), "{guess}");
        assert_eq!(found_in(&two, "baldur's gate two"), "Baldur's Gate II");
        // The old near-misses still hold.
        assert!(found_in(&apps, "spotifire").starts_with("nothing; close: Spotify"));
        let k: Vec<String> = keys("Baldur's Gate III").iter().map(|k| k.words.join(" ")).collect();
        assert!(k.contains(&"baldurs gate 3".to_string()), "{k:?}");
    }

    #[test]
    fn more_ways_of_asking_to_open() {
        for (text, phrase, play) in [
            ("Try to open Baldur's Gate Three.", "baldur's gate three", false),
            ("try and open spotify", "spotify", false),
            ("Can you try to open Teams, please?", "teams", false),
            ("open up the calculator", "calculator", false),
            ("Go ahead and open Spotify", "spotify", false),
            ("could you open notepad", "notepad", false),
            ("Would you open Teams?", "teams", false),
            ("start up steam", "steam", false),
            ("boot up world of warcraft", "world of warcraft", false),
            ("load up baldurs gate", "baldurs gate", false),
            ("fire up battle.net", "battle.net", false),
            ("Play World of Warcraft.", "world of warcraft", true),
            ("let's play baldur's gate 3", "baldur's gate 3", true),
            ("open spotify again", "spotify", false),
            ("Open it again, please.", "it", false),
            ("Try to open it again, please", "it", false),
            ("open that one", "that one", false),
            ("Yes, open Teams", "teams", false),
        ] {
            let r = open_request(text).unwrap_or_else(|| panic!("{text:?} is an open request"));
            assert_eq!((r.phrase.as_str(), r.play), (phrase, play), "{text:?}");
        }
        for text in ["Playing World of Warcraft", "Yes.", "Are you struggling?", "play"] {
            assert_eq!(open_request(text), None, "{text:?}");
        }
        assert!(asked_to_open("play world of warcraft"));
    }

    #[test]
    fn qualifiers_and_keys() {
        assert_eq!(words(&strip_qualifiers("Python 3.12 (64-bit)")), ["python"]);
        assert_eq!(words(&strip_qualifiers("PowerShell 7 (x64)")), ["powershell"]);
        assert_eq!(words(&strip_qualifiers("Visual Studio Code - Insiders")), ["visual", "studio", "code"]);
        assert_eq!(words(&strip_qualifiers("Nsight Compute 2025.4.1")), ["nsight", "compute"]);
        assert_eq!(words("Baldur's Gate 3"), ["baldurs", "gate", "3"]);
        assert_eq!(words("Notepad++"), ["notepad++"]);
        let k: Vec<String> = keys("Visual Studio Code").iter().map(|k| k.words.join(" ")).collect();
        assert_eq!(k, ["visual studio code", "vsc", "vs code", "code"]);
        let k: Vec<String> = keys("Microsoft Teams").iter().map(|k| k.words.join(" ")).collect();
        assert_eq!(k, ["microsoft teams", "teams", "mt", "m teams"]);
    }
}

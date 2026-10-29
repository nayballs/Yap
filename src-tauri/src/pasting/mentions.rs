//! **@mentions** (2026-10-29; Wispr Flow's Slack tagging, by behaviour):
//! in a team chat, "at Sam" becomes a mention of Sam when Sam's name is on
//! screen (the names `dictionary/screen_terms.rs` read at record start).
//!
//! Plain "@Sam" text isn't a mention in most apps: the app's own list has
//! to be picked from (type "@Sam", wait for it, then a key). What each does
//! (2026, their own help pages and keyboard shortcuts):
//!
//! - **Slack** and **Discord**: typing "@" and a name opens the list with
//!   the best match highlighted; **Tab** inserts it (as Enter does — but
//!   Enter sends the message when the list has closed, Tab never does).
//!   Yap types "@Sam" and presses Tab — only once UI Automation shows the
//!   list's highlighted entry is that person (the focus on a list item whose
//!   name has the words said, as the app tells a screen reader). Not seen
//!   within a moment: no key at all, the typed "@Sam" stays as text.
//! - **Teams**, **Google Chat**, **Mattermost**, **Rocket.Chat**, **Zulip**
//!   and **Element**: their lists are picked with Enter (Teams' Tab isn't
//!   documented), and an Enter that comes as the list closes sends a half
//!   message. So plain "@Sam", pasted with the rest of the text: still a
//!   mention to anyone reading, and one press of the app's own list away.
//! - **WhatsApp, Telegram, Messenger, Signal** and other personal chats:
//!   nothing changes ("at Sam" stays as said): a mention there is a group
//!   feature and "@Sam" in a chat with Sam reads oddly.
//!
//! What counts as a mention ([`find`], pure): "at" and a name on screen,
//! starting a clause ("At Sam, can you…", "Thanks at Sam", "cc at Priya
//! and at Sam") — never a place ("see you at Rochelle's", "lunch at
//! Zizzi") — or "@Sam" as AI cleanup may write it. The longest name on
//! screen that matches wins ("at Sam Example" over "at Sam").

/// The team chats Yap makes mentions in, and how.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum App {
    Slack,
    Discord,
    Teams,
    GoogleChat,
    Mattermost,
    RocketChat,
    Zulip,
    Element,
}

impl App {
    /// Yap types the mention and picks it from the app's list with Tab
    /// (Slack, Discord); else plain "@Name" text.
    pub fn picks_with_tab(self) -> bool {
        matches!(self, App::Slack | App::Discord)
    }

    pub fn name(self) -> &'static str {
        match self {
            App::Slack => "Slack",
            App::Discord => "Discord",
            App::Teams => "Teams",
            App::GoogleChat => "Google Chat",
            App::Mattermost => "Mattermost",
            App::RocketChat => "Rocket.Chat",
            App::Zulip => "Zulip",
            App::Element => "Element",
        }
    }
}

/// The apps by process name (lower case, no ".exe"), then by the site a
/// browser's title shows.
const BY_EXE: &[(&str, App)] = &[
    ("slack", App::Slack),
    ("discord", App::Discord),
    ("discordptb", App::Discord),
    ("discordcanary", App::Discord),
    ("ms-teams", App::Teams),
    ("teams", App::Teams),
    ("msteams", App::Teams),
    ("mattermost", App::Mattermost),
    ("rocket.chat", App::RocketChat),
    ("zulip", App::Zulip),
    ("element", App::Element),
];
const BY_SITE: &[(&str, App)] = &[
    ("slack", App::Slack),
    ("discord", App::Discord),
    ("microsoft teams", App::Teams),
    ("google chat", App::GoogleChat),
    ("mattermost", App::Mattermost),
    ("rocket.chat", App::RocketChat),
    ("zulip", App::Zulip),
    ("element", App::Element),
];

/// Which team chat a window is: its process (`exe`, any case, with or
/// without ".exe"), or for a browser the site its title shows. Pure.
pub fn which(exe: &str, title: &str) -> Option<App> {
    let stem = crate::pasting::app_kind::exe_stem(exe);
    if let Some((_, app)) = BY_EXE.iter().find(|(e, _)| *e == stem) {
        return Some(*app);
    }
    if !crate::pasting::app_kind::is_browser(&stem) {
        return None;
    }
    let mut t = title.replace('\u{200b}', "");
    for sep in [" - ", " – ", " — ", " | ", " • ", " · "] {
        t = t.replace(sep, "\u{1}");
    }
    t.split('\u{1}').find_map(|seg| {
        let seg = crate::pasting::app_kind::page_name(seg);
        BY_SITE.iter().find(|(s, _)| seg.eq_ignore_ascii_case(s)).map(|(_, app)| *app)
    })
}

/// A mention found in a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    /// Its byte range in the text: "at Sam" or "@Sam".
    pub start: usize,
    pub end: usize,
    /// The name as the screen writes it, as much of it as was said ("Sam",
    /// "Sam Example").
    pub name: String,
}

/// Words that may come just before "at Name" in a mention ("hey at Sam",
/// "thanks at Priya", "cc at Sam and at Priya").
const LEADS: &[&str] = &[
    "hey", "hi", "hello", "thanks", "cheers", "cc", "ping", "pinging", "paging", "and", "or", "also", "sorry",
    "dear", "yo", "morning", "afternoon", "evening", "tagging", "tag", "plus",
];

/// The words of `text` (letters, digits, apostrophes, hyphens inside),
/// with their byte ranges.
fn words(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (k, &(i, c)) in chars.iter().enumerate() {
        let inner = matches!(c, '\'' | '’' | '-')
            && start.is_some()
            && chars.get(k + 1).is_some_and(|(_, n)| n.is_alphanumeric());
        let part = c.is_alphanumeric() || inner;
        match (part, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push((s, i));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, text.len()));
    }
    out
}

/// Does "at" at `words[i]` start a clause? The text's start, punctuation
/// before it, a line break, or a word that leads a mention ("thanks at
/// Sam", "thank you at Sam").
fn starts_clause(text: &str, ws: &[(usize, usize)], i: usize) -> bool {
    let head = text[..ws[i].0].trim_end_matches([' ', '\t']);
    match head.chars().next_back() {
        None => return true,
        Some(c) if ".!?…,;:—–-\n\r(".contains(c) => return true,
        Some(c) if !c.is_alphanumeric() => return false,
        _ => {}
    }
    let prev = |k: usize| i.checked_sub(k).map(|j| text[ws[j].0..ws[j].1].to_lowercase());
    match prev(1) {
        Some(w) if LEADS.contains(&w.as_str()) => true,
        Some(w) if w == "you" => prev(2).is_some_and(|t| t == "thank"),
        _ => false,
    }
}

/// The names (each as its words) that `text`'s words starting at `ws[i]`
/// spell, longest first: the name on screen, its words compared without
/// case or accents' marks aside. `None`: no name there.
fn name_at<'a>(text: &str, ws: &[(usize, usize)], i: usize, names: &'a [Vec<String>]) -> Option<(usize, &'a [String])> {
    let mut best: Option<(usize, &[String])> = None;
    for name in names {
        let n = name.len();
        if n == 0 || i + n > ws.len() {
            continue;
        }
        let same = (0..n).all(|k| {
            let w = &text[ws[i + k].0..ws[i + k].1];
            w.to_lowercase() == name[k].to_lowercase()
                // Only spaces between a full name's words.
                && (k == 0 || text[ws[i + k - 1].1..ws[i + k].0].chars().all(|c| c == ' '))
        });
        if same && best.is_none_or(|(m, _)| n > m) {
            best = Some((n, name.as_slice()));
        }
    }
    best
}

/// The mentions in `text`: "at <a name on screen>" starting a clause, or
/// "@<a name on screen>"; never a name followed by "'s" (a place, a
/// thing of theirs). `names`: the people-like names on screen ("Sam",
/// "Sam Example"). Pure.
pub fn find(text: &str, names: &[String]) -> Vec<Found> {
    let names: Vec<Vec<String>> = names
        .iter()
        .map(|n| n.split_whitespace().map(str::to_string).collect::<Vec<_>>())
        .filter(|w: &Vec<String>| !w.is_empty())
        .collect();
    if names.is_empty() {
        return Vec::new();
    }
    let ws = words(text);
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < ws.len() {
        let w = &text[ws[i].0..ws[i].1];
        // "@Sam": written so already (AI cleanup may have).
        let at_sign = text[..ws[i].0].ends_with('@');
        let (name_from, start) = if at_sign {
            (i, ws[i].0 - 1)
        } else if w.eq_ignore_ascii_case("at") && i + 1 < ws.len() && &text[ws[i].1..ws[i + 1].0] == " " && starts_clause(text, &ws, i) {
            (i + 1, ws[i].0)
        } else {
            i += 1;
            continue;
        };
        if let Some((n, name)) = name_at(text, &ws, name_from, &names) {
            let end = ws[name_from + n - 1].1;
            let possessive = text[end..].starts_with("'s") || text[end..].starts_with("’s");
            if !possessive {
                out.push(Found { start, end, name: name.join(" ") });
                i = name_from + n;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// `text` with each mention written "@Name" (the plain-text form).
pub fn plain(text: &str, found: &[Found]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for f in found {
        out.push_str(&text[at..f.start]);
        out.push('@');
        out.push_str(&f.name);
        at = f.end;
    }
    out.push_str(&text[at..]);
    out
}

/// Is the highlighted entry of an app's list (`option`: its name as UI
/// Automation gives it, "Sam Example (you) · Away") the person said
/// (`said`: "Sam", "Sam Example")? Every word said must be a whole word of
/// it, any case: "Sam" isn't "Samantha". Pure.
pub fn option_is(option: &str, said: &str) -> bool {
    let words = |s: &str| -> Vec<String> {
        s.split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '’' || c == '.' || c == '_' || c == '-'))
            .map(|w| w.trim_matches(['.', '-', '_']).to_lowercase())
            .filter(|w| !w.is_empty())
            .collect()
    };
    let have = words(option);
    let want = words(said);
    !want.is_empty() && want.iter().all(|w| have.contains(w))
}

/// The people-like names among the screen's terms: names and full names
/// that aren't the app's own name ("Slack", "Discord", "Teams"…).
pub fn people(terms: &[crate::dictionary::screen_terms::Term]) -> Vec<String> {
    use crate::dictionary::screen_terms::Kind;
    const APPS: &[&str] = &["slack", "discord", "teams", "microsoft", "google", "chat", "mattermost", "zulip", "element", "rocket"];
    terms
        .iter()
        .filter(|t| matches!(t.kind, Kind::Name | Kind::FullName))
        .filter(|t| !t.text.split_whitespace().any(|w| APPS.contains(&w.to_lowercase().as_str())))
        .map(|t| t.text.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn mentions(text: &str, list: &[&str]) -> String {
        plain(text, &find(text, &names(list)))
    }

    #[test]
    fn at_a_name_on_screen_starting_a_clause_is_a_mention() {
        let on_screen = ["Sam", "Priya", "Sam Example", "Rochelle"];
        assert_eq!(mentions("At Sam, can you review this?", &on_screen), "@Sam, can you review this?");
        assert_eq!(mentions("at sam can you review this", &on_screen), "@Sam can you review this");
        assert_eq!(mentions("Thanks at Priya!", &on_screen), "Thanks @Priya!");
        assert_eq!(mentions("Thank you at Priya.", &on_screen), "Thank you @Priya.");
        assert_eq!(mentions("cc at Sam and at Priya", &on_screen), "cc @Sam and @Priya");
        assert_eq!(mentions("Hey at Sam Example, the build is green.", &on_screen), "Hey @Sam Example, the build is green.");
        assert_eq!(mentions("Looks good. At Rochelle: ship it.", &on_screen), "Looks good. @Rochelle: ship it.");
        // Written so already by cleanup: the name as the screen spells it.
        assert_eq!(mentions("@sam can you look?", &on_screen), "@Sam can you look?");
    }

    #[test]
    fn a_place_or_a_name_not_on_screen_is_left_as_said() {
        let on_screen = ["Sam", "Rochelle", "Zizzi"];
        for text in [
            "See you at Rochelle's.",
            "We had lunch at Zizzi yesterday.",
            "I'll meet you at Sam's.",
            "Look at Sam's pull request.",
            "At Olivia, can you look?",
            "at the station at noon",
            "That's at Sam's desk.",
        ] {
            assert_eq!(mentions(text, &on_screen), text, "{text}");
        }
        assert!(find("At Sam, hi", &[]).is_empty());
    }

    #[test]
    fn the_longest_name_on_screen_wins() {
        let f = find("At Sam Example, hi", &names(&["Sam", "Sam Example"]));
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].name, "Sam Example");
        // "Sam Exampleton" isn't on screen: just Sam.
        assert_eq!(find("at Sam Other hi", &names(&["Sam", "Sam Example"]))[0].name, "Sam");
    }

    #[test]
    fn the_apps_by_process_and_by_tab_title() {
        assert_eq!(which("slack.exe", ""), Some(App::Slack));
        assert_eq!(which("C:\\Apps\\Discord.exe", ""), Some(App::Discord));
        assert_eq!(which("ms-teams.exe", ""), Some(App::Teams));
        assert_eq!(which("msedge.exe", "general - Example Ltd - Slack - Microsoft\u{200b} Edge"), Some(App::Slack));
        assert_eq!(which("chrome.exe", "(3) Discord | #general | Example - Google Chrome"), Some(App::Discord));
        assert_eq!(which("chrome.exe", "Chat | Sam | Microsoft Teams - Google Chrome"), Some(App::Teams));
        assert_eq!(which("chrome.exe", "Google Chat - Google Chrome"), Some(App::GoogleChat));
        // Personal chats and other apps: none.
        assert_eq!(which("whatsapp.exe", ""), None);
        assert_eq!(which("chrome.exe", "(2) WhatsApp - Google Chrome"), None);
        assert_eq!(which("notepad.exe", "Slack - Notepad"), None);
        assert!(App::Slack.picks_with_tab() && App::Discord.picks_with_tab());
        assert!(!App::Teams.picks_with_tab() && !App::GoogleChat.picks_with_tab());
    }

    #[test]
    fn the_highlighted_entry_must_be_the_person_said() {
        assert!(option_is("Sam Example", "Sam"));
        assert!(option_is("Sam Example (you) · Away", "sam example"));
        assert!(option_is("sam.example Sam Example", "Sam"));
        assert!(!option_is("Samantha Jones", "Sam"));
        assert!(!option_is("Priya Raman", "Sam"));
        assert!(!option_is("", "Sam"));
        // A file in an AI chat's list.
        assert!(option_is("userService.ts src/services", "userService.ts"));
        assert!(!option_is("userService.test.ts src/services", "userService.ts"));
    }

    #[test]
    fn people_are_names_not_the_app() {
        use crate::dictionary::screen_terms::{Kind, Term};
        let t = |text: &str, kind| Term { text: text.into(), kind, count: 1 };
        let got = people(&[
            t("Sam", Kind::Name),
            t("Sam Example", Kind::FullName),
            t("Slack", Kind::Name),
            t("GitHub", Kind::MixedCase),
            t("NASA", Kind::Acronym),
            t("Microsoft Teams", Kind::FullName),
        ]);
        assert_eq!(got, ["Sam", "Sam Example"]);
    }
}

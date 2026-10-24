//! Yap's own answers, before any model: the fast path for a plain open
//! request (open it, "Which one?", "Did you mean…?"), the answers to
//! those questions (a per-conversation memory), and carrying a plan out.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use crate::config::YapConfig;

use super::index::{apps, App};
use super::matching::{find, not_an_app, numerals, open_request, refers_back, words, Found};
use super::{open, LAST_FOR, OFFER_FOR};

/// What Yap does with a message by itself, before any model.
#[derive(Debug, Clone)]
pub enum Plan {
    Open(App),
    /// Several apps: ask which (the phrase, the candidates).
    Ask(String, Vec<App>),
    /// "Yes" to a question that offered several: which one, again.
    Which(Vec<App>),
    Blocked(App),
    /// No app by that name (the phrase, close names).
    NotFound(String, Vec<App>),
    /// "No" to Yap's question.
    Declined,
    /// "Let Yap open apps when you ask" is off.
    Off,
}

/// Does Yap answer `text` itself? An open request ([`open_request`]) that
/// names an app — one (open it), several (ask which) or a switched-off one
/// (say so) — or that plainly names something not on this PC, when no model
/// that can use tools is there to make sense of it (`model_can_tool`); with
/// opening switched off, any of those says so. `None`: the model answers.
/// Pure. (Chat and the Voice Assistant use [`plan_with`], with what Yap
/// remembers of the conversation.)
#[cfg(test)]
pub fn plan(text: &str, cfg: &YapConfig, apps: &[App], model_can_tool: bool) -> Option<Plan> {
    plan_with(text, cfg, apps, model_can_tool, &Recall::default())
}

/// [`plan`] with what Yap remembers of the conversation ([`recall`]): an
/// answer to its own question first ("yes" / "the second one" / "no",
/// [`follow_up`]), then an open request, where "open it (again)" means the
/// app it last opened or suggested (none: the model's). "play …" naming no
/// app is the model's too ("play the next song"). Pure.
pub fn plan_with(text: &str, cfg: &YapConfig, apps: &[App], model_can_tool: bool, recall: &Recall) -> Option<Plan> {
    // The app as the index has it now, and as the settings say now.
    let current = |app: &App| -> Plan {
        let app = apps.iter().find(|a| a.id == app.id).cloned().unwrap_or_else(|| app.clone());
        if !cfg.open_apps {
            Plan::Off
        } else if cfg.apps_blocked.contains(&app.id) {
            Plan::Blocked(app)
        } else {
            Plan::Open(app)
        }
    };
    if !recall.offer.is_empty() {
        match follow_up(text, &recall.offer) {
            Some(Reply::Pick(i)) => return Some(current(&recall.offer[i])),
            Some(Reply::Yes) if recall.offer.len() == 1 => return Some(current(&recall.offer[0])),
            Some(Reply::Yes) => return Some(Plan::Which(recall.offer.clone())),
            Some(Reply::No) => return Some(Plan::Declined),
            None => {}
        }
    }
    let request = open_request(text)?;
    if refers_back(&request.phrase) {
        return recall.last.as_ref().map(current);
    }
    let found = find(apps, &request.phrase, &cfg.app_nicknames, &cfg.apps_blocked);
    let named_one = !matches!(found, Found::Nothing(_));
    if !named_one && (request.play || !request.plain || not_an_app(&request.phrase)) {
        return None;
    }
    if !cfg.open_apps {
        return Some(Plan::Off);
    }
    match found {
        Found::One(app) => Some(Plan::Open(app)),
        Found::Several(apps) => Some(Plan::Ask(request.phrase, apps)),
        Found::Blocked(app) => Some(Plan::Blocked(app)),
        Found::Nothing(_) if model_can_tool => None,
        Found::Nothing(close) => Some(Plan::NotFound(request.phrase, close)),
    }
}

/// [`plan_with`] for Chat and the Voice Assistant: off the async runtime
/// (the index may be built), with whether Chat's model can use tools and
/// what Yap remembers of `conversation`. Only looks: [`remember`] /
/// [`moved_on`] record what happened.
pub async fn plan_for(text: &str, cfg: &YapConfig, conversation: Option<u64>) -> Option<Plan> {
    let recall = recall(conversation);
    let answers_yap = !recall.offer.is_empty() && follow_up(text, &recall.offer).is_some();
    if !answers_yap {
        open_request(text)?;
    }
    let model_can_tool = crate::chat::turn::resolve_chat_endpoint(cfg)
        .is_ok_and(|(_, _, model, provider, _, _)| crate::chat::tools::supports_tools(&provider, &model));
    let (text, cfg) = (text.to_string(), cfg.clone());
    tauri::async_runtime::spawn_blocking(move || plan_with(&text, &cfg, &apps(false), model_can_tool, &recall))
        .await
        .ok()
        .flatten()
}

// ---- answers to Yap's questions --------------------------------------------------

/// What Yap remembers of a conversation's apps ([`recall`]).
#[derive(Debug, Clone, Default)]
pub struct Recall {
    /// The apps Yap's open question offered, in the order it named them:
    /// one for "Did you mean X?", several for "Which one?". Empty: none.
    pub offer: Vec<App>,
    /// The app Yap last opened or suggested: "open it again".
    pub last: Option<App>,
}

struct Remembered {
    offer: Vec<App>,
    last: Option<App>,
    at: Instant,
}

/// Per conversation, in memory only.
static MEMORY: Mutex<Option<HashMap<u64, Remembered>>> = Mutex::new(None);

fn memory() -> std::sync::MutexGuard<'static, Option<HashMap<u64, Remembered>>> {
    MEMORY.lock().unwrap_or_else(|p| p.into_inner())
}

/// What Yap remembers of `conversation`: its question for [`OFFER_FOR`],
/// the app "it" means for [`LAST_FOR`].
pub fn recall(conversation: Option<u64>) -> Recall {
    let Some(id) = conversation else { return Recall::default() };
    let memory = memory();
    let Some(r) = memory.as_ref().and_then(|m| m.get(&id)) else { return Recall::default() };
    let age = r.at.elapsed();
    Recall {
        offer: if age < OFFER_FOR { r.offer.clone() } else { Vec::new() },
        last: r.last.clone().filter(|_| age < LAST_FOR),
    }
}

/// Yap answered a message of `conversation` itself (`plan`): remember what
/// its answer leaves open ([`after`]).
pub fn remember(conversation: u64, plan: &Plan) {
    let (offer, last) = after(plan);
    let mut memory = memory();
    let map = memory.get_or_insert_with(HashMap::new);
    map.retain(|_, r| r.at.elapsed() < LAST_FOR);
    map.insert(conversation, Remembered { offer, last, at: Instant::now() });
}

/// A message of `conversation` went to the model: Yap's question has
/// lapsed, and "it" no longer means its app.
pub fn moved_on(conversation: Option<u64>) {
    if let (Some(id), Some(map)) = (conversation, memory().as_mut()) {
        map.remove(&id);
    }
}

/// What an answer of Yap's leaves to remember: the apps its question
/// offered, and the app "open it" means. Pure.
pub fn after(plan: &Plan) -> (Vec<App>, Option<App>) {
    match plan {
        Plan::Open(app) => (Vec::new(), Some(app.clone())),
        Plan::Ask(_, apps) | Plan::Which(apps) => (apps.clone(), None),
        Plan::NotFound(_, close) => (close.clone(), (close.len() == 1).then(|| close[0].clone())),
        Plan::Blocked(_) | Plan::Declined | Plan::Off => (Vec::new(), None),
    }
}

/// An answer to Yap's question ([`follow_up`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
    /// "yes", "go ahead", "open it", "that one".
    Yes,
    /// "no", "never mind", "neither".
    No,
    /// One of the apps offered: "the second one", "3.13", "the classic one".
    Pick(usize),
}

/// At least one of these makes a message a "yes".
const YES_CORE: &[&str] = &[
    "yes", "yeah", "yep", "yup", "yea", "ya", "aye", "sure", "ok", "okay", "alright", "please", "do",
    "go", "open", "launch", "start", "play", "run", "absolutely", "definitely", "correct", "right",
    "exactly", "course", "good", "great", "perfect", "fine", "cool", "that", "this",
];
/// The other words a "yes" may hold ("yes I do", "go ahead", "that one").
const YES_MORE: &[&str] = &[
    "it", "ahead", "i", "the", "thanks", "thank", "you", "sounds", "for", "me", "of", "is", "its",
    "that's", "thats", "it's", "now", "then", "again", "app", "game", "all", "so", "just", "oh",
    "mate", "yap", "one", "be", "lovely", "works", "up", "and", "would", "like", "love", "to",
];
/// What a plain "no" may hold ("no thanks", "never mind", "neither of them").
const NO_ALL: &[&str] = &[
    "no", "nope", "nah", "cancel", "neither", "none", "nevermind", "never", "mind", "forget", "it",
    "not", "now", "thanks", "thank", "you", "don't", "dont", "open", "that", "one", "of", "them",
    "please", "its", "it's", "ok", "okay", "stop", "wrong", "leave",
];
/// A "no" starts with one of these.
const NO_FIRST: &[&str] = &[
    "no", "nope", "nah", "cancel", "neither", "none", "nevermind", "never", "forget", "not", "don't", "dont", "stop",
    "wrong", "leave",
];
/// Words that never make an answer a "yes".
const NOT_YES: &[&str] = &[
    "no", "not", "don't", "dont", "never", "nope", "nah", "cancel", "stop", "wait", "wrong",
    "isn't", "didn't", "doesn't", "neither", "none",
];

/// The words of a short reply: lower case, apostrophes kept ("don't").
fn reply_words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .replace(['\u{2019}', '\u{2018}'], "'")
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .map(|w| w.trim_matches('\'').to_string())
        .filter(|w| !w.is_empty())
        .collect()
}

/// Is `text` a plain "yes" — "yes", "Yeah, go ahead", "sure, open it",
/// "Yes, I do.", "please do", "that one"? A question ("yes?"), anything
/// with a "no" in it or more than a few words isn't. English. Pure.
pub fn is_affirmative(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() || t.ends_with('?') {
        return false;
    }
    let w = reply_words(t);
    let has = |list: &[&str], x: &String| list.contains(&x.as_str());
    !w.is_empty()
        && w.len() <= 8
        && w.iter().all(|x| has(YES_CORE, x) || has(YES_MORE, x))
        && w.iter().any(|x| has(YES_CORE, x))
        && !w.iter().any(|x| has(NOT_YES, x))
}

/// A plain "no": "no", "No thanks.", "never mind", "neither of them".
fn is_negative(text: &str) -> bool {
    let w = reply_words(text);
    !w.is_empty()
        && w.len() <= 6
        && NO_FIRST.contains(&w[0].as_str())
        && w.iter().all(|x| NO_ALL.contains(&x.as_str()))
}

/// Words a pick may hold besides what it picks ("the second one, please").
const PICK_FILLER: &[&str] = &[
    "the", "one", "please", "open", "launch", "start", "play", "run", "yes", "yeah", "ok", "okay",
    "i", "want", "mean", "meant", "choose", "pick", "take", "go", "with", "number", "option",
    "that", "this", "app", "game", "it", "is", "its", "just", "lets", "let's", "me", "thanks",
    "thank", "you", "sure", "how", "about", "for", "it's",
];

/// "first", "2nd", "3", "last": which of `n`.
fn ordinal(word: &str, n: usize) -> Option<usize> {
    const ORDINALS: &[&[&str]] = &[
        &["first", "1st", "1"],
        &["second", "2nd", "2", "two"],
        &["third", "3rd", "3", "three"],
        &["fourth", "4th", "4", "four"],
        &["fifth", "5th", "5", "five"],
        &["sixth", "6th", "6", "six"],
    ];
    if word == "last" {
        return n.checked_sub(1);
    }
    ORDINALS.iter().position(|o| o.contains(&word)).filter(|&i| i < n)
}

/// Which of the apps `offer`ed a reply picks: by its place ("the second
/// one", "number 2", "the last one", "one") or by words only it has
/// ("3.13", "the classic one") or its name.
fn pick(text: &str, offer: &[App]) -> Option<usize> {
    let all = reply_words(text);
    if all.is_empty() || all.len() > 8 {
        return None;
    }
    let rest: Vec<&str> = all.iter().map(String::as_str).filter(|w| !PICK_FILLER.contains(w)).collect();
    if rest.is_empty() {
        // "one", "number one": the first (of several; one offered: a "yes").
        let bare: Vec<&str> = all.iter().map(String::as_str).filter(|w| !["the", "number", "option", "please"].contains(w)).collect();
        return (bare == ["one"] && offer.len() > 1).then_some(0);
    }
    if let [word] = rest.as_slice() {
        if let Some(i) = ordinal(word, offer.len()) {
            return Some(i);
        }
    }
    // Words only one of them has.
    let said = numerals(&rest.iter().flat_map(|w| words(w)).collect::<Vec<_>>());
    if said.is_empty() {
        return None;
    }
    let holders: Vec<usize> = offer
        .iter()
        .enumerate()
        .filter(|(_, a)| {
            let name = numerals(&words(&a.name));
            said.iter().all(|w| name.contains(w))
        })
        .map(|(i, _)| i)
        .collect();
    if let [i] = holders.as_slice() {
        return Some(*i);
    }
    match find(offer, &rest.join(" "), &[], &[]) {
        Found::One(a) => offer.iter().position(|o| o.id == a.id),
        _ => None,
    }
}

/// Is `text` an answer to Yap's question, which offered `offer` ("Did you
/// mean X?": one; "Which one?": several)? A "no" first, then a pick, then a
/// "yes" (which picks the one offered, or asks again). `None`: something
/// else, for the usual path. English. Pure.
pub fn follow_up(text: &str, offer: &[App]) -> Option<Reply> {
    if offer.is_empty() || text.trim().ends_with('?') {
        return None;
    }
    if is_negative(text) {
        return Some(Reply::No);
    }
    if let Some(i) = pick(text, offer) {
        return Some(Reply::Pick(i));
    }
    is_affirmative(text).then_some(Reply::Yes)
}

/// Yap's answer to an open request it handled, and its chip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub reply: String,
    pub chip: String,
}

/// "A, B and C" (or "A, B or C").
fn listing(names: &[&str], last: &str) -> String {
    match names {
        [] => String::new(),
        [one] => one.to_string(),
        [head @ .., tail] => format!("{} {last} {tail}", head.join(", ")),
    }
}

/// A message from Windows as a sentence: trimmed, with a full stop.
pub(super) fn sentence(e: &str) -> String {
    let e = e.trim();
    if e.is_empty() {
        "it gave no reason.".to_string()
    } else if e.ends_with(['.', '!', '?']) {
        e.to_string()
    } else {
        format!("{e}.")
    }
}

/// Carry out a [`plan`]: open the app (blocking: the launch), or say why
/// not. The reply is what the Voice Assistant says aloud; an empty chip is
/// none.
pub fn carry_out(plan: &Plan) -> Answer {
    match plan {
        Plan::Open(app) => match open(app) {
            Ok(()) => {
                // A game opens in its launcher (World of Warcraft: Battle.net,
                // with the game picked).
                let within = app.launcher().map(|l| format!(" (in {l})")).unwrap_or_default();
                Answer { reply: format!("Opening {}{within}.", app.name), chip: format!("Opened {}", app.name) }
            }
            Err(e) => Answer {
                reply: format!("Windows couldn't start {}: {}", app.name, sentence(&e)),
                chip: format!("Couldn't open {}", app.name),
            },
        },
        Plan::Ask(phrase, apps) => {
            let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
            Answer {
                reply: format!(
                    "I found {} apps for \u{201C}{phrase}\u{201D}: {}. Which one?",
                    apps.len(),
                    listing(&names, "and")
                ),
                chip: format!("Found {} apps for \"{phrase}\"", apps.len()),
            }
        }
        Plan::Which(apps) => {
            let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
            Answer { reply: format!("Which one: {}?", listing(&names, "or")), chip: String::new() }
        }
        Plan::Declined => Answer { reply: "Okay, I won't open anything.".to_string(), chip: String::new() },
        Plan::Blocked(app) => Answer {
            reply: format!(
                "You've asked Yap not to open {}. You can change that in Settings \u{2192} Voice Assistant.",
                app.name
            ),
            chip: format!("Not allowed to open {}", app.name),
        },
        Plan::NotFound(phrase, close) => {
            let names: Vec<&str> = close.iter().map(|a| a.name.as_str()).collect();
            let mean = if names.is_empty() { String::new() } else { format!(" Did you mean {}?", listing(&names, "or")) };
            Answer {
                reply: format!("I couldn't find an app called \u{201C}{phrase}\u{201D} on this PC.{mean}"),
                chip: format!("Couldn't find an app called \"{phrase}\""),
            }
        }
        Plan::Off => Answer {
            reply: "Opening apps is turned off. You can turn on \u{201C}Let Yap open apps when you ask\u{201D} in Settings \u{2192} Voice Assistant.".to_string(),
            chip: "Opening apps is off".to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::index::launcher;
    use crate::apps::tests::{found, games, named, opened_ids, pc, several};
    use crate::config::AppNickname;

    #[test]
    fn the_fast_path_leaves_what_isnt_an_app_to_the_model() {
        let cfg = YapConfig::default();
        let apps = pc();
        for text in [
            "open the notes about Teams",
            "open my notes",
            "start the meeting",
            "run the tests",
            "open the link",
            "the meeting will open at 3",
            "open spotify and play my liked songs",
        ] {
            assert!(plan(text, &cfg, &apps, false).is_none(), "{text:?}");
        }
        // A plain name that isn't installed: Yap says so, unless a model that
        // can use tools is there to make sense of it.
        assert!(matches!(plan("open xylophone pro", &cfg, &apps, false), Some(Plan::NotFound(p, _)) if p == "xylophone pro"));
        assert!(plan("open xylophone pro", &cfg, &apps, true).is_none());
        // "Microsoft To Do" has a joining word, and is still an app.
        assert!(matches!(plan("open microsoft to do", &cfg, &apps, false), Some(Plan::Open(a)) if a.name == "Microsoft To Do"));
    }

    #[test]
    fn several_alike_are_not_guessed() {
        assert_eq!(several("python"), ["Python 3.12 (64-bit)", "Python 3.13 (64-bit)", "Python 3.14 (64-bit)"]);
        // Two Teams.
        let mut apps = pc();
        apps.push(App::new("{x}\\Teams.exe", "Microsoft Teams (work or school)"));
        match find(&apps, "teams", &[], &[]) {
            Found::Several(v) => assert_eq!(
                v.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(),
                ["Microsoft Teams", "Microsoft Teams (work or school)"]
            ),
            other => panic!("{other:?}"),
        }
        // …and asked about, with no model call.
        let cfg = YapConfig::default();
        let Some(plan) = plan("open teams", &cfg, &apps, false) else { panic!() };
        let answer = carry_out(&plan);
        assert_eq!(
            answer.reply,
            "I found 2 apps for \u{201C}teams\u{201D}: Microsoft Teams and Microsoft Teams (work or school). Which one?"
        );
        assert_eq!(answer.chip, "Found 2 apps for \"teams\"");
    }

    #[test]
    fn unknown_names_get_close_ones() {
        match found("spotifire") {
            Found::Nothing(close) => assert_eq!(close.first().map(|a| a.name.as_str()), Some("Spotify")),
            Found::One(a) => panic!("guessed {}", a.name),
            other => panic!("{other:?}"),
        }
        assert!(matches!(found("xylophone"), Found::Nothing(c) if c.is_empty()));
        let answer = carry_out(&Plan::NotFound("spotifire".into(), vec![App::new("s", "Spotify")]));
        assert_eq!(answer.reply, "I couldn't find an app called \u{201C}spotifire\u{201D} on this PC. Did you mean Spotify?");
        assert_eq!(answer.chip, "Couldn't find an app called \"spotifire\"");
    }

    #[test]
    fn nicknames_come_first_and_switched_off_apps_never_open() {
        let apps = pc();
        let editor = AppNickname { say: "my editor".into(), app: "Microsoft.VisualStudioCode".into(), name: "Visual Studio Code".into() };
        assert!(matches!(find(&apps, "my editor", std::slice::from_ref(&editor), &[]), Found::One(a) if a.name == "Visual Studio Code"));
        assert!(matches!(find(&apps, "My Editor", std::slice::from_ref(&editor), &[]), Found::One(a) if a.name == "Visual Studio Code"));
        // A nickname that beats an app's own name.
        let music = AppNickname { say: "music".into(), app: "SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify".into(), name: "Spotify".into() };
        assert!(matches!(find(&apps, "music", &[music], &[]), Found::One(a) if a.name == "Spotify"));
        // A nickname for an app that's gone: as if there were none.
        let gone = AppNickname { say: "my editor".into(), app: "Gone.App".into(), name: "Gone".into() };
        assert!(matches!(find(&apps, "my editor", &[gone], &[]), Found::Nothing(_)));

        let blocked = vec!["MSTeams_8wekyb3d8bbwe!MSTeams".to_string()];
        assert!(matches!(find(&apps, "teams", &[], &blocked), Found::Blocked(a) if a.name == "Microsoft Teams"));
        assert!(matches!(find(&apps, "my editor", &[editor], &["Microsoft.VisualStudioCode".into()]), Found::Blocked(_)));
        let cfg = YapConfig { apps_blocked: blocked, ..YapConfig::default() };
        let Some(p) = plan("open teams", &cfg, &apps, false) else { panic!() };
        assert_eq!(carry_out(&p).chip, "Not allowed to open Microsoft Teams");
        // Several alike, one switched off: the other.
        let mut two = pc();
        two.push(App::new("{x}\\Teams.exe", "Microsoft Teams (work or school)"));
        assert!(matches!(find(&two, "teams", &[], &["{x}\\Teams.exe".into()]), Found::One(a) if a.name == "Microsoft Teams"));
    }

    #[test]
    fn the_fast_path_opens_and_says_so_without_a_model() {
        let cfg = YapConfig::default();
        let Some(p) = plan("Can you open Notepad, please?", &cfg, &pc(), false) else { panic!() };
        assert!(matches!(&p, Plan::Open(a) if a.name == "Notepad"));
        let answer = carry_out(&p);
        assert_eq!(answer, Answer { reply: "Opening Notepad.".into(), chip: "Opened Notepad".into() });
        assert!(opened_ids().contains(&"Microsoft.WindowsNotepad_8wekyb3d8bbwe!App".to_string()));
        // Switched off: it says so, opens nothing.
        let off = YapConfig { open_apps: false, ..YapConfig::default() };
        assert!(matches!(plan("open spotify", &off, &pc(), false), Some(Plan::Off)));
        assert!(matches!(plan("open spotify", &off, &pc(), true), Some(Plan::Off)));
        assert!(plan("open my notes", &off, &pc(), false).is_none());
        assert_eq!(carry_out(&Plan::Off).chip, "Opening apps is off");
    }

    #[test]
    fn games_open_in_their_launcher() {
        assert_eq!(launcher("steam://rungameid/1086940", ""), Some("Steam"));
        assert_eq!(launcher("com.epicgames.launcher://apps/Fortnite?action=launch", ""), Some("the Epic Games Launcher"));
        assert_eq!(launcher(r"D:\WoW\World of Warcraft Launcher.exe", "--productcode=wow"), Some("Battle.net"));
        assert_eq!(launcher(r"D:\Battle.net\Battle.net Launcher.exe", ""), None);
        assert_eq!(launcher(r"C:\Program Files\Spotify\Spotify.exe", ""), None);
        let apps = games();
        let open = |q: &str| match find(&apps, q, &[], &[]) {
            Found::One(a) => carry_out(&Plan::Open(a)),
            other => panic!("{q}: {other:?}"),
        };
        assert_eq!(open("world of warcraft").reply, "Opening World of Warcraft (in Battle.net).");
        assert_eq!(open("world of warcraft").chip, "Opened World of Warcraft");
        assert_eq!(open("baldurs gate three").reply, "Opening Baldur's Gate 3 (in Steam).");
        assert_eq!(open("battle.net").reply, "Opening Battle.net.");
        assert_eq!(open("spotify").reply, "Opening Spotify.");
        assert_eq!(sentence("The system cannot find the file specified.\r\n"), "The system cannot find the file specified.");
        assert_eq!(sentence("Access is denied"), "Access is denied.");
    }

    #[test]
    fn play_and_it_are_the_models_when_yap_has_no_app_for_them() {
        let (cfg, apps) = (YapConfig::default(), games());
        assert!(matches!(plan("play world of warcraft", &cfg, &apps, false), Some(Plan::Open(a)) if a.name == "World of Warcraft"));
        assert!(plan("play the next song", &cfg, &apps, false).is_none());
        assert!(plan("play some music", &cfg, &apps, false).is_none());
        assert!(plan("play despacito", &cfg, &apps, false).is_none());
        // "it" with nothing opened or suggested: the model's.
        assert!(plan("open it again", &cfg, &apps, false).is_none());
        let wow = apps.iter().find(|a| a.name == "World of Warcraft").cloned();
        let recall = Recall { offer: Vec::new(), last: wow };
        for text in ["open it again", "Try to open it again, please", "open that one", "launch it"] {
            assert!(matches!(plan_with(text, &cfg, &apps, false, &recall), Some(Plan::Open(a)) if a.name == "World of Warcraft"), "{text}");
        }
        // A new name is a new request, whatever was opened before.
        assert!(matches!(plan_with("open spotify", &cfg, &apps, false, &recall), Some(Plan::Open(a)) if a.name == "Spotify"));
        // "Open the notes about Teams" stays the model's.
        assert!(plan_with("open the notes about Teams", &cfg, &apps, false, &recall).is_none());
    }

    #[test]
    fn answers_to_did_you_mean() {
        let apps = games();
        let bg3 = named(&apps, &["Baldur's Gate 3"]);
        for yes in [
            "yes", "Yes.", "yeah", "Yep!", "sure", "please", "do it", "Go ahead.", "open it", "that one",
            "Yes, I do.", "yes please", "sure, open it", "Okay, go for it", "Please do.", "sounds good",
            "Baldur's Gate 3", "open baldurs gate 3", "play it",
        ] {
            assert!(matches!(follow_up(yes, &bg3), Some(Reply::Yes | Reply::Pick(0))), "{yes}");
        }
        for no in ["no", "No thanks.", "nope", "never mind", "Not now", "don't open it", "cancel"] {
            assert_eq!(follow_up(no, &bg3), Some(Reply::No), "{no}");
        }
        for other in ["Are you struggling?", "yes?", "open teams", "what's the weather like", "tell me a joke", "no, I meant Spotify"] {
            assert_eq!(follow_up(other, &bg3), None, "{other}");
        }
        assert!(is_affirmative("Yes, I do."));
        assert!(!is_affirmative("Are you struggling?"));
        assert!(!is_affirmative("I don't think so"));
        assert!(!is_affirmative("summarise my notes"));
    }

    #[test]
    fn answers_to_which_one() {
        let apps = games();
        let pythons = named(&apps, &["Python 3.12 (64-bit)", "Python 3.13 (64-bit)", "Python 3.14 (64-bit)"]);
        for (text, i) in [
            ("the first one", 0), ("the second one", 1), ("Second.", 1), ("2", 1), ("number two", 1),
            ("the 2nd one please", 1), ("the last one", 2), ("third", 2), ("one", 0), ("3.13", 1),
            ("python 3.14", 2), ("open python 3.12", 0), ("the 3.14 one", 2),
        ] {
            assert_eq!(follow_up(text, &pythons), Some(Reply::Pick(i)), "{text}");
        }
        // All 64-bit: not one of them. One 64-bit, one 32-bit: that one.
        assert_ne!(follow_up("the 64-bit one", &pythons).map(|r| matches!(r, Reply::Pick(_))), Some(true));
        let bits = vec![App::new("py32", "Python 3.12 (32-bit)"), App::new("py64", "Python 3.12 (64-bit)")];
        assert_eq!(follow_up("the 64-bit one", &bits), Some(Reply::Pick(1)));
        assert_eq!(follow_up("the fifth one", &pythons), None);
        assert_eq!(follow_up("yes", &pythons), Some(Reply::Yes));
        assert_eq!(follow_up("neither", &pythons), Some(Reply::No));
        let wows = named(&apps, &["World of Warcraft", "World of Warcraft Classic"]);
        assert_eq!(follow_up("the classic one", &wows), Some(Reply::Pick(1)));
        assert_eq!(follow_up("world of warcraft", &wows), Some(Reply::Pick(0)));
        assert_eq!(follow_up("World of Warcraft Classic", &wows), Some(Reply::Pick(1)));
    }

    #[test]
    fn yap_handles_the_answers_to_its_own_questions() {
        let (cfg, apps) = (YapConfig::default(), games());
        // "open boulders gate three" → opened outright.
        assert!(matches!(plan("open boulders gate three", &cfg, &apps, false), Some(Plan::Open(a)) if a.name == "Baldur's Gate 3"));
        // "Did you mean Spotify?" → "yes" opens it, "no" says so.
        let Some(asked) = plan("open spotifire", &cfg, &apps, false) else { panic!() };
        assert!(matches!(&asked, Plan::NotFound(_, close) if close.len() == 1));
        let (offer, last) = after(&asked);
        assert_eq!(last.as_ref().map(|a| a.name.as_str()), Some("Spotify"));
        let recall = Recall { offer, last };
        assert!(matches!(plan_with("Yes.", &cfg, &apps, false, &recall), Some(Plan::Open(a)) if a.name == "Spotify"));
        assert!(matches!(plan_with("no thanks", &cfg, &apps, false, &recall), Some(Plan::Declined)));
        assert_eq!(carry_out(&Plan::Declined), Answer { reply: "Okay, I won't open anything.".into(), chip: String::new() });
        // Something else: the usual path (here, the model's).
        assert!(plan_with("what's the time?", &cfg, &apps, false, &recall).is_none());
        // Switched off since: said so; blocked: said so.
        let off = YapConfig { open_apps: false, ..YapConfig::default() };
        assert!(matches!(plan_with("yes", &off, &apps, false, &recall), Some(Plan::Off)));
        let blocked = YapConfig { apps_blocked: vec![recall.offer[0].id.clone()], ..YapConfig::default() };
        assert!(matches!(plan_with("yes", &blocked, &apps, false, &recall), Some(Plan::Blocked(_))));
        // "Which one?" → "the second one".
        let Some(asked) = plan("open python", &cfg, &apps, false) else { panic!() };
        let (offer, last) = after(&asked);
        assert!(last.is_none());
        let recall = Recall { offer, last };
        assert!(matches!(plan_with("the second one", &cfg, &apps, false, &recall), Some(Plan::Open(a)) if a.name == "Python 3.13 (64-bit)"));
        let Some(again) = plan_with("yes", &cfg, &apps, false, &recall) else { panic!() };
        assert_eq!(
            carry_out(&again).reply,
            "Which one: Python 3.12 (64-bit), Python 3.13 (64-bit) or Python 3.14 (64-bit)?"
        );
        assert_eq!(after(&again).0.len(), 3);
        // An open request for another app still wins.
        assert!(matches!(plan_with("open teams", &cfg, &apps, false, &recall), Some(Plan::Open(a)) if a.name == "Microsoft Teams"));
    }

    #[test]
    fn what_yap_remembers_lapses_when_the_model_answers() {
        const CONV: u64 = 9_000_001;
        let apps = games();
        assert!(recall(Some(CONV)).offer.is_empty());
        remember(CONV, &Plan::NotFound("spotifire".into(), named(&apps, &["Spotify"])));
        let r = recall(Some(CONV));
        assert_eq!((r.offer.len(), r.last.as_ref().map(|a| a.name.as_str())), (1, Some("Spotify")));
        // Another conversation knows nothing of it; nor does a new one.
        assert!(recall(Some(CONV + 1)).offer.is_empty());
        assert!(recall(None).offer.is_empty());
        remember(CONV, &Plan::Open(named(&apps, &["World of Warcraft"]).remove(0)));
        let r = recall(Some(CONV));
        assert!(r.offer.is_empty());
        assert_eq!(r.last.map(|a| a.name), Some("World of Warcraft".to_string()));
        moved_on(Some(CONV));
        let r = recall(Some(CONV));
        assert!(r.offer.is_empty() && r.last.is_none());
    }
}

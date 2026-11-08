//! **The person's OK, asked out loud** (the Voice Assistant; Connectors
//! wave 6 for a connector's tool, generalised 8 Nov 2026 to every card that
//! asks first): a call that waits for the person's OK on its card
//! (`pages/act/confirm.rs`) while they may not be looking at Chat would wait
//! its two minutes unseen, so Yap says one line ([`asking`]) and puts
//! **Allow once / Don't** on the Yap bar (the card [`CARD`], unless the
//! answer's in view in Chat, where the thread's card is). The turn waits on
//! the same question, whichever answers first. What asks:
//!
//! - **A connector's tool** (act "connector"): "Use Notion to create page
//!   “Launch notes”? Say yes, or press Allow." (`connector_ask.rs`).
//! - **An action on a page** (act "press", "enter", "open", "type": a click
//!   or an Enter that buys, books, sends, signs in…, words from nowhere the
//!   person can see): "Press “Buy tickets” on rail.example.com? Say yes, or
//!   press Allow." ([`page_line`], [`page_card`]).
//! - **An agent after outside content** (act "agent", `chat/tools/agents.rs`):
//!   "Send this to an agent in garden-planner, in Auto mode? Say yes, or
//!   press Allow.", the bar card titled "Send this to an agent?" with its
//!   folder, mode and brief ([`agent_line`], [`agent_card`]).
//!
//! **The spoken answer** works as Yap Code's spoken yes does
//! (`agents/voice.rs`): the next Voice Assistant question in that
//! conversation, within the card's two minutes, is the answer when it's a
//! clear yes or no ([`reply`]) in the person's languages (My languages'
//! codes, English always; none of Yap's seven known: all seven) — a yes
//! allows it once, a no says Don't; anything else is a question of its own.
//! Never Always allow by voice. **A click only** (`spoken` false) for the
//! riskiest — a connector's tool that can delete, a page action that would
//! delete or pay ([`CLICK_ONLY`]), an agent in Bypass permissions — and
//! while a meeting records or a call is on: a spoken yes is answered "That
//! one needs a click…" and the card still waits; a spoken no still says
//! Don't. Never from a meeting's audio: only the assistant's own shortcut
//! asks (and it's off during meetings). Never from a phone: a phone's
//! question never acts on a page (no panel), and its card never comes here.
//!
//! The slot ([`Waiting`]) holds one question at a time — the latest asked —
//! and goes when its card is answered, timed out, or its turn ends.
//!
//! Logs: the question's id and the answer, never the card's words.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::chat::pages::act::confirm::{AgentCard, Card};

/// The bar card's id (its rule in `notices/notice.rs`).
pub const CARD: &str = "askFirst";
/// How long a spoken answer may come: the card's own wait.
pub const WAIT: Duration = crate::chat::pages::act::confirm::WAIT;
/// What Yap says to a spoken yes that can't answer (a click only).
pub const NEEDS_CLICK: &str = "That one needs a click: press Allow on the Yap bar.";
/// What a page action may do that takes a click, never a spoken yes:
/// deleting and paying can't be taken back.
pub const CLICK_ONLY: &[&str] = &["delete", "pay"];
/// How much of the words Yap would type, or an agent's brief, it says or shows.
const SAY_CHARS: usize = 60;
const SHOW_CHARS: usize = 160;

/// **A question waiting for the person's spoken answer**: its conversation
/// and card, whether a spoken yes may answer it, when it was asked, the
/// Voice Assistant question it belongs to (`generation`), that question's
/// dictations-started count (`since`: brought up to date when a spoken
/// answer takes it, so its answer is still said), and whose turn it is
/// (`owner`).
#[derive(Debug, Clone)]
pub struct Waiting {
    pub conversation: u64,
    pub card: u64,
    pub spoken: bool,
    pub at: Instant,
    pub generation: u64,
    pub since: Arc<AtomicU64>,
    pub owner: u64,
}

static WAITING: Mutex<Option<Waiting>> = Mutex::new(None);
static OWNERS: AtomicU64 = AtomicU64::new(1);

fn slot() -> std::sync::MutexGuard<'static, Option<Waiting>> {
    WAITING.lock().unwrap_or_else(|p| p.into_inner())
}

/// A token for one spoken question's turn ([`Waiting::owner`]).
pub fn owner() -> u64 {
    OWNERS.fetch_add(1, Ordering::Relaxed)
}

/// Yap asked out loud: `w` waits for its answer (the one before, if any,
/// no longer by voice).
pub fn wait(w: Waiting) {
    *slot() = Some(w);
}

/// Card `card`'s question was answered (or ended): no longer waiting.
/// Whether it was the one waiting.
pub fn answered(card: u64) -> bool {
    let mut s = slot();
    if s.as_ref().is_some_and(|w| w.card == card) {
        *s = None;
        true
    } else {
        false
    }
}

/// `owner`'s turn ended: its question no longer waits. Whether it did.
pub fn ended(owner: u64) -> bool {
    let mut s = slot();
    if s.as_ref().is_some_and(|w| w.owner == owner) {
        *s = None;
        true
    } else {
        false
    }
}

/// What a spoken answer does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spoken {
    /// Allow once.
    Yes,
    /// Don't.
    No,
    /// A yes to a question that takes a click: said so, still waiting.
    NeedsClick,
}

/// **The spoken answer to the question waiting in `conversation`**, if
/// `question` is one ([`reply`] in `langs`) and one waits there still
/// (`meeting`: one records or a call is on — a click only). The waiting
/// question, for its answer and its turn. A yes or a no takes it out of the
/// slot (its card's answer follows); a yes that needs a click leaves it.
pub fn take(conversation: Option<u64>, question: &str, langs: &[String], meeting: bool) -> Option<(Spoken, Waiting)> {
    let yes = reply(question, langs)?;
    let mut s = slot();
    let w = s.as_ref().filter(|w| Some(w.conversation) == conversation && w.at.elapsed() <= WAIT)?.clone();
    let said = match (yes, w.spoken && !meeting) {
        (false, _) => Spoken::No,
        (true, true) => Spoken::Yes,
        (true, false) => return Some((Spoken::NeedsClick, w)),
    };
    *s = None;
    Some((said, w))
}

/// The bar card's buttons: `allow:<id>` / `deny:<id>` → (id, allow). Pure.
pub fn action_of(action: &str) -> Option<(u64, bool)> {
    if let Some(id) = action.strip_prefix("allow:") {
        return id.parse().ok().map(|id| (id, true));
    }
    action.strip_prefix("deny:").and_then(|id| id.parse().ok()).map(|id| (id, false))
}

// ---- what a card asks, out loud ----------------------------------------------------------

/// **What Yap says and shows for a card asked out loud**: its one line, the
/// bar's card, and whether a spoken yes may answer it.
#[derive(Debug, Clone)]
pub struct Asking {
    pub line: String,
    pub bar: crate::bar::Card,
    pub spoken: bool,
}

/// **A card that asks, asked out loud** (`meeting`: one records or a call is
/// on — a click only): a connector's tool (its own card says whether a
/// spoken yes may answer), an action on a page, an agent. `None`: a card
/// that doesn't ask by voice (a connector's asked in Chat, an older card).
/// Pure.
pub fn asking(card: &Card, meeting: bool) -> Option<Asking> {
    match card.act.as_str() {
        "connector" => {
            let c = card.connector.as_deref().filter(|c| c.voice)?;
            Some(Asking { line: super::connector_ask::line(c, c.spoken), bar: super::connector_ask::card(card.id, c), spoken: c.spoken })
        }
        "agent" => {
            let a = card.agent.as_ref()?;
            let spoken = !meeting && a.mode != "bypassPermissions";
            Some(Asking { line: agent_line(a, spoken), bar: agent_card(card.id, a, spoken), spoken })
        }
        "" | "press" | "enter" | "open" | "type" => {
            let spoken = !meeting && !CLICK_ONLY.contains(&card.does.as_str());
            Some(Asking { line: page_line(card, spoken), bar: page_card(card, spoken), spoken })
        }
        _ => None,
    }
}

/// The line's ending: how it may be answered.
fn answer_by(spoken: bool) -> &'static str {
    if spoken {
        " Say yes, or press Allow."
    } else {
        " It needs a click: press Allow on the Yap bar."
    }
}

/// `s` on one line, short enough to say (`max` characters), or `None`.
fn sayable(s: &str, max: usize) -> Option<String> {
    let one = s.split_whitespace().collect::<Vec<_>>().join(" ");
    (!one.is_empty() && one.chars().count() <= max).then_some(one)
}

/// What an action on a page would do, as the thread's card says it
/// (`pageConfirm.js` `WHY`).
fn why_of(does: &str) -> &'static str {
    match does {
        "buy" => "It may buy something.",
        "pay" => "It may take a payment.",
        "book" => "It may make a booking.",
        "place an order" => "It may place an order.",
        "cancel" => "It may cancel something for you.",
        "confirm" => "It may confirm something for you.",
        "subscribe" => "It may sign you up for something.",
        "send" => "It may send something.",
        "delete" => "It may delete something.",
        "agree" => "It agrees to terms for you.",
        "sign in" => "It signs in to the site.",
        "type" => "Those words aren\u{2019}t in what you said, nor on the page.",
        _ => "It may do something on the site for you.",
    }
}

/// What Yap would do on the page, in words: "Press “Buy tickets” on
/// rail.example.com", "Type “Hello” into “Message” on chat.example.com".
fn page_doing(c: &Card, quote: (&str, &str)) -> String {
    let (open, close) = quote;
    let label = c.label.trim();
    let on = if c.site.trim().is_empty() { String::new() } else { format!(" on {}", c.site.trim()) };
    match c.act.as_str() {
        "enter" => format!("Press Enter in {open}{label}{close}{on}"),
        "open" => format!("Open {open}{label}{close}{on}"),
        "type" => match sayable(&c.text, SAY_CHARS) {
            Some(t) => format!("Type {open}{t}{close} into {open}{label}{close}{on}"),
            None => format!("Type into {open}{label}{close}{on}"),
        },
        _ => format!("Press {open}{label}{close}{on}"),
    }
}

/// **The line Yap says** before an action on a page (`spoken`: a spoken yes
/// may answer it): "Press “Buy tickets” on rail.example.com? Say yes, or
/// press Allow." — the words to type when they're short. Pure.
pub fn page_line(c: &Card, spoken: bool) -> String {
    format!("{}?{}", page_doing(c, ("\u{201c}", "\u{201d}")), answer_by(spoken))
}

/// **The bar's card** before an action on a page: "Press ‘Buy tickets’ on
/// rail.example.com?", why it asks (the thread's card's words) and, for
/// typed words, the words; **Allow once** and **Don't**. Pure.
pub fn page_card(c: &Card, spoken: bool) -> crate::bar::Card {
    let mut body = Vec::new();
    if c.act == "type" && !c.text.trim().is_empty() {
        body.push(crate::chat::pages::clip(&c.text, SHOW_CHARS));
    }
    body.push(why_of(if c.act == "type" { "type" } else { &c.does }).to_string());
    bar_card(c.id, format!("{}?", page_doing(c, ("\u{2018}", "\u{2019}"))), body.join("\n"), spoken)
}

/// **The line Yap says** before an agent starts, or is told something, after
/// outside content (`spoken`: a spoken yes may answer it): "Send this to an
/// agent in garden-planner, in Auto mode? Say yes, or press Allow." Pure.
pub fn agent_line(a: &AgentCard, spoken: bool) -> String {
    let mode = if a.mode_label.trim().is_empty() { "Auto" } else { a.mode_label.trim() };
    let q = if a.continues {
        format!("Pass this to the agent in {}, in {mode} mode?", a.folder.trim())
    } else {
        format!("Send this to an agent in {}, in {mode} mode?", a.folder.trim())
    };
    format!("{q}{}", answer_by(spoken))
}

/// **The bar's card** before an agent: "Send this to an agent?", its folder
/// and mode, then the start of its brief (the thread's card has it all);
/// **Allow once** and **Don't**. Pure.
pub fn agent_card(id: u64, a: &AgentCard, spoken: bool) -> crate::bar::Card {
    let mode = if a.mode_label.trim().is_empty() { "Auto" } else { a.mode_label.trim() };
    let title = if a.continues { "Send this to the agent?" } else { "Send this to an agent?" };
    let mut body = vec![format!("{} \u{b7} {mode}", a.folder.trim())];
    if !a.brief.trim().is_empty() {
        body.push(crate::chat::pages::clip(&a.brief, SHOW_CHARS));
    }
    bar_card(id, title.to_string(), body.join("\n"), spoken)
}

/// **A bar card that asks** (question `id`): its title and lines, **Allow
/// once** and **Don't**; "This one needs a click" when a spoken yes can't
/// answer it. It waits for its answer; its ✕ only hides it (the question
/// still waits in Chat). Pure.
pub fn bar_card(id: u64, title: String, body: String, spoken: bool) -> crate::bar::Card {
    crate::bar::Card {
        id: CARD.into(),
        icon: "assistant",
        title,
        body,
        status: (!spoken).then(|| "This one needs a click".to_string()),
        primary: Some(crate::bar::CardAction::new(format!("allow:{id}"), "Allow once")),
        secondary: Some(crate::bar::CardAction::new(format!("deny:{id}"), "Don't")),
        timeout_ms: None,
        ..Default::default()
    }
}

// ---- a clear yes or no, in Yap's seven languages ------------------------------------------

/// One language's words for a spoken answer, folded (no accents, lower
/// case) and split as `rules::words` splits ("d'accord" is "d accord").
struct Lang {
    code: &'static str,
    /// A yes on its own.
    yes: &'static [&'static str],
    /// A no on its own (a reply starting so).
    no: &'static [&'static str],
    /// Words a short reply may hold besides its yes or no.
    filler: &'static [&'static str],
}

const LANGS: &[Lang] = &[
    Lang {
        code: "en",
        yes: &["yes", "yeah", "yep", "yup", "sure", "ok", "okay", "go ahead", "do it", "allow it", "allow", "fine", "alright", "all right", "of course", "please do", "sounds good"],
        no: &["no", "nope", "nah", "don t", "dont", "do not", "cancel", "stop", "never mind", "nevermind", "deny", "not now"],
        filler: &["please", "thanks", "thank", "you", "it", "that", "yap", "go", "on", "then", "just", "for", "now"],
    },
    Lang {
        code: "fr",
        yes: &["oui", "ouais", "d accord", "ok", "vas y", "allez y", "bien sur", "autorise", "c est bon", "volontiers"],
        no: &["non", "annule", "laisse tomber", "pas maintenant", "refuse"],
        filler: &["s il te plait", "s il vous plait", "merci", "yap", "alors", "oui"],
    },
    Lang {
        code: "de",
        yes: &["ja", "jawohl", "klar", "genau", "mach das", "mach es", "mach", "los", "ok", "okay", "gerne", "erlauben", "in ordnung"],
        no: &["nein", "nee", "abbrechen", "lass es", "stopp", "nicht jetzt", "ablehnen"],
        filler: &["bitte", "danke", "yap", "das", "es", "doch"],
    },
    Lang {
        code: "es",
        yes: &["si", "vale", "claro", "adelante", "hazlo", "ok", "de acuerdo", "permitir", "venga"],
        no: &["no", "cancela", "cancelar", "dejalo", "ahora no"],
        filler: &["por favor", "gracias", "yap", "lo"],
    },
    Lang {
        code: "it",
        yes: &["si", "certo", "va bene", "ok", "vai", "fallo", "d accordo", "consenti"],
        no: &["no", "annulla", "lascia stare", "non ora", "nega"],
        filler: &["per favore", "grazie", "yap", "pure"],
    },
    Lang {
        code: "pt",
        yes: &["sim", "claro", "pode", "pode ser", "ok", "vai", "faca", "faz", "de acordo", "ta bom", "esta bem", "permitir"],
        no: &["nao", "cancela", "cancelar", "deixa", "agora nao"],
        filler: &["por favor", "obrigado", "obrigada", "yap", "isso"],
    },
    Lang {
        code: "nl",
        yes: &["ja", "jawel", "doe maar", "prima", "goed", "oke", "ok", "akkoord", "ga je gang", "toestaan"],
        no: &["nee", "annuleer", "laat maar", "niet nu", "weigeren"],
        filler: &["alsjeblieft", "graag", "dank je", "bedankt", "yap", "maar"],
    },
];

/// The words of `text` covered whole by `phrases` (or `filler`), each
/// phrase's words side by side: how many of `phrases` it took, or `None`
/// when a word is neither. Pure.
fn covered(words: &[String], phrases: &[&str], filler: &[&str]) -> Option<usize> {
    let split = |p: &str| p.split(' ').map(str::to_string).collect::<Vec<String>>();
    let mut longest: Vec<(Vec<String>, bool)> = phrases.iter().map(|p| (split(p), true)).chain(filler.iter().map(|p| (split(p), false))).collect();
    longest.sort_by_key(|(p, _)| std::cmp::Reverse(p.len()));
    let (mut i, mut hits) = (0, 0);
    'next: while i < words.len() {
        for (p, counts) in &longest {
            if !p.is_empty() && words[i..].starts_with(p) {
                i += p.len();
                hits += usize::from(*counts);
                continue 'next;
            }
        }
        return None;
    }
    Some(hits)
}

/// **A clear spoken yes (`true`) or no (`false`)** in `langs` (My
/// languages' codes; English always; none of the seven known: all seven):
/// a short reply (8 words at most), not a question, made only of one
/// language's yes words (or its no words) and its fillers — "Yes please",
/// "Yeah, go ahead", "Oui, vas-y", "Ja, mach das", "No thanks", "Nein".
/// Anything else (a question of its own): `None`. Pure.
pub fn reply(text: &str, langs: &[String]) -> Option<bool> {
    let t = text.trim();
    if t.is_empty() || t.ends_with('?') {
        return None;
    }
    let words = crate::chat::pages::act::rules::words(t);
    if words.is_empty() || words.len() > 8 {
        return None;
    }
    let known = langs.iter().any(|c| LANGS.iter().any(|l| c.to_ascii_lowercase().starts_with(l.code)));
    let mine = |code: &str| !known || code == "en" || langs.iter().any(|c| c.to_ascii_lowercase().starts_with(code));
    for l in LANGS.iter().filter(|l| mine(l.code)) {
        // A no first: "no" is a no in Spanish and Italian too.
        if words.first().is_some_and(|w| l.no.iter().any(|n| n.split(' ').next() == Some(w.as_str()))) && covered(&words, l.no, l.filler).is_some_and(|n| n > 0) {
            return Some(false);
        }
        if covered(&words, l.yes, l.filler).is_some_and(|n| n > 0) {
            return Some(true);
        }
    }
    // English's own plain yes and no (`apps::is_affirmative`, Yap Code's no).
    if crate::apps::is_affirmative(t) {
        return Some(true);
    }
    if crate::agents::voice::is_no(t) {
        return Some(false);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::pages::act::confirm::ConnectorCard;

    fn page(act: &str, label: &str, does: &str, text: &str) -> Card {
        Card { id: 4, label: label.into(), site: "rail.example.com".into(), does: does.into(), state: "asking".into(), act: act.into(), text: text.into(), ..Default::default() }
    }

    fn agent(mode: &str, label: &str, continues: bool) -> Card {
        let a = AgentCard {
            folder: "garden-planner".into(),
            path: r"D:\Projects\garden-planner".into(),
            mode: mode.into(),
            mode_label: label.into(),
            brief: "Save the research notes into notes/research.md.".into(),
            web: true,
            continues,
            ..Default::default()
        };
        Card { id: 9, label: "garden-planner".into(), site: a.path.clone(), does: "send an agent".into(), state: "asking".into(), act: "agent".into(), agent: Some(a), ..Default::default() }
    }

    #[test]
    fn a_page_action_is_said_and_carded_as_the_thread_card_asks_it() {
        let buy = asking(&page("press", "Buy tickets", "buy", ""), false).unwrap();
        assert_eq!(buy.line, "Press \u{201c}Buy tickets\u{201d} on rail.example.com? Say yes, or press Allow.");
        assert!(buy.spoken);
        assert_eq!(buy.bar.id, CARD);
        assert_eq!(buy.bar.title, "Press \u{2018}Buy tickets\u{2019} on rail.example.com?");
        assert_eq!(buy.bar.body, "It may buy something.");
        assert_eq!(buy.bar.primary.as_ref().map(|a| (a.id.as_str(), a.label.as_str())), Some(("allow:4", "Allow once")));
        assert_eq!(buy.bar.secondary.as_ref().map(|a| (a.id.as_str(), a.label.as_str())), Some(("deny:4", "Don't")));
        assert!(buy.bar.status.is_none() && buy.bar.timeout_ms.is_none());
        // An older card (no act) is a press.
        assert_eq!(asking(&page("", "Book now", "book", ""), false).unwrap().line, "Press \u{201c}Book now\u{201d} on rail.example.com? Say yes, or press Allow.");
        // Enter, a link, typed words (short: said; long: on the bar only).
        assert_eq!(page_line(&page("enter", "Message", "send", ""), true), "Press Enter in \u{201c}Message\u{201d} on rail.example.com? Say yes, or press Allow.");
        assert_eq!(page_line(&page("open", "Checkout", "buy", ""), true), "Open \u{201c}Checkout\u{201d} on rail.example.com? Say yes, or press Allow.");
        let typed = asking(&page("type", "Message", "type", "See you at ten"), false).unwrap();
        assert_eq!(typed.line, "Type \u{201c}See you at ten\u{201d} into \u{201c}Message\u{201d} on rail.example.com? Say yes, or press Allow.");
        assert_eq!(typed.bar.body, "See you at ten\nThose words aren\u{2019}t in what you said, nor on the page.");
        let long = "word ".repeat(30);
        assert_eq!(page_line(&page("type", "Message", "type", &long), true), "Type into \u{201c}Message\u{201d} on rail.example.com? Say yes, or press Allow.");
    }

    #[test]
    fn deleting_or_paying_on_a_page_and_anything_during_a_meeting_takes_a_click() {
        for does in ["delete", "pay"] {
            let a = asking(&page("press", "Do it", does, ""), false).unwrap();
            assert!(!a.spoken, "{does}");
            assert!(a.line.ends_with("It needs a click: press Allow on the Yap bar."), "{}", a.line);
            assert_eq!(a.bar.status.as_deref(), Some("This one needs a click"));
        }
        for does in ["buy", "book", "send", "sign in", "subscribe", "agree", "type"] {
            assert!(asking(&page("press", "Do it", does, ""), false).unwrap().spoken, "{does}");
            assert!(!asking(&page("press", "Do it", does, ""), true).unwrap().spoken, "{does}: never by voice during a meeting");
        }
    }

    #[test]
    fn an_agent_after_outside_content_is_said_with_its_folder_and_mode_and_carded_with_its_brief() {
        let a = asking(&agent("auto", "Auto", false), false).unwrap();
        assert_eq!(a.line, "Send this to an agent in garden-planner, in Auto mode? Say yes, or press Allow.");
        assert!(a.spoken);
        assert_eq!(a.bar.title, "Send this to an agent?");
        assert_eq!(a.bar.body, "garden-planner \u{b7} Auto\nSave the research notes into notes/research.md.");
        assert_eq!(a.bar.primary.as_ref().map(|b| b.label.as_str()), Some("Allow once"));
        // A follow-up for the chat's agent.
        let f = asking(&agent("plan", "Plan", true), false).unwrap();
        assert_eq!(f.line, "Pass this to the agent in garden-planner, in Plan mode? Say yes, or press Allow.");
        assert_eq!(f.bar.title, "Send this to the agent?");
        // Bypass permissions, or a meeting: a click only.
        let bypass = asking(&agent("bypassPermissions", "Bypass permissions", false), false).unwrap();
        assert!(!bypass.spoken && bypass.line.ends_with("It needs a click: press Allow on the Yap bar."));
        assert!(!asking(&agent("auto", "Auto", false), true).unwrap().spoken);
    }

    #[test]
    fn a_connectors_card_asks_as_it_did_and_only_when_asked_out_loud() {
        let c = ConnectorCard { connector: "Notion".into(), tool: "Create page".into(), voice: true, spoken: true, ..Default::default() };
        let card = Card { id: 7, act: "connector".into(), connector: Some(Box::new(c.clone())), ..Default::default() };
        let a = asking(&card, false).unwrap();
        assert_eq!(a.line, "Use Notion to create page? Say yes, or press Allow.");
        assert_eq!((a.bar.id.as_str(), a.bar.title.as_str()), (CARD, "Use Notion: Create page?"));
        assert!(a.spoken);
        // Asked in Chat: nothing said.
        let typed = Card { connector: Some(Box::new(ConnectorCard { voice: false, ..c })), ..card };
        assert!(asking(&typed, false).is_none());
        assert!(asking(&Card { act: "something else".into(), ..Default::default() }, false).is_none());
        assert_eq!(action_of("allow:7"), Some((7, true)));
        assert_eq!(action_of("deny:12"), Some((12, false)));
        assert_eq!(action_of("close"), None);
    }

    #[test]
    fn a_clear_yes_or_no_in_the_persons_languages() {
        let en = vec!["en".to_string()];
        for yes in ["Yes", "Yes please.", "Yeah, go ahead", "OK", "Sure, do it", "Allow it"] {
            assert_eq!(reply(yes, &en), Some(true), "{yes}");
        }
        for no in ["No", "No thanks.", "Don't", "Cancel that", "Never mind", "Not now"] {
            assert_eq!(reply(no, &en), Some(false), "{no}");
        }
        for other in ["Yes?", "What's on my calendar today", "Yes but call it Launch plan instead", ""] {
            assert_eq!(reply(other, &en), None, "{other}");
        }
        // Their languages: French and German here; Dutch isn't theirs.
        let fr_de = vec!["fr".to_string(), "de-DE".to_string()];
        assert_eq!(reply("Oui, vas-y", &fr_de), Some(true));
        assert_eq!(reply("D'accord, merci", &fr_de), Some(true));
        assert_eq!(reply("Ja, mach das bitte", &fr_de), Some(true));
        assert_eq!(reply("Nein, danke", &fr_de), Some(false));
        assert_eq!(reply("Non", &fr_de), Some(false));
        assert_eq!(reply("Doe maar", &fr_de), None, "not one of theirs");
        assert_eq!(reply("Yes", &fr_de), Some(true), "English always");
        // None of the seven known: all seven.
        let other = vec!["ja-JP".to_string()];
        assert_eq!(reply("Doe maar", &other), Some(true));
        assert_eq!(reply("Sim, pode", &other), Some(true));
        assert_eq!(reply("Não", &other), Some(false));
        assert_eq!(reply("Sí, adelante", &other), Some(true));
        assert_eq!(reply("No, grazie", &other), Some(false));
    }

    #[test]
    fn a_spoken_answer_takes_only_the_question_waiting_in_its_conversation() {
        let en = vec!["en".to_string()];
        let w = |card: u64, spoken: bool| Waiting { conversation: 5, card, spoken, at: Instant::now(), generation: 3, since: Arc::new(AtomicU64::new(0)), owner: 9 };
        wait(w(1, true));
        assert!(take(Some(6), "Yes", &en, false).is_none(), "another conversation's");
        assert!(take(Some(5), "What's the weather?", &en, false).is_none(), "a question of its own");
        let (said, got) = take(Some(5), "Yes please", &en, false).unwrap();
        assert_eq!((said, got.card, got.generation), (Spoken::Yes, 1, 3));
        assert!(take(Some(5), "Yes", &en, false).is_none(), "taken");
        // A click only: a yes is said so and it still waits; a no answers it.
        wait(w(2, false));
        assert_eq!(take(Some(5), "Yes", &en, false).map(|(s, _)| s), Some(Spoken::NeedsClick));
        assert_eq!(take(Some(5), "No", &en, false).map(|(s, _)| s), Some(Spoken::No));
        // During a meeting, never a spoken yes.
        wait(w(3, true));
        assert_eq!(take(Some(5), "Yes", &en, true).map(|(s, _)| s), Some(Spoken::NeedsClick));
        // Answered on the bar or in Chat, or its turn ended: gone.
        assert!(answered(3));
        assert!(take(Some(5), "Yes", &en, false).is_none());
        wait(w(4, true));
        assert!(!ended(1) && ended(9));
        assert!(take(Some(5), "Yes", &en, false).is_none());
    }
}

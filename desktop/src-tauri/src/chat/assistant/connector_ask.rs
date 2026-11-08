//! **A connector's tool asked about out loud** (the Voice Assistant,
//! Connectors wave 6): a call that asks first (`consent.rs`) while the
//! person isn't looking at Chat would wait its two minutes unseen, so Yap
//! says one line — "Use Notion to create page "Launch notes"? Say yes, or
//! press Allow." ([`line`]) — and puts **Allow once / Don't** on the Yap
//! bar ([`card`]), beside the card in Chat's thread. The turn waits on
//! the same question (`pages/act/confirm.rs`), whichever answers first.
//!
//! **The spoken answer** works as Yap Code's spoken yes does
//! (`agents/voice.rs`): the next Voice Assistant question in that
//! conversation, within the card's two minutes, is the answer when it's a
//! clear yes or no ([`reply`]) in the person's languages (My languages'
//! codes, English always; none of Yap's seven known: all seven) — a yes
//! allows it once, a no says Don't; anything else is a question of its own.
//! Never Always allow by voice. **A click only** (`spoken` false) for a tool
//! that can delete and while a meeting records or a call is on: a spoken
//! yes is answered "That one needs a click…" and the card still waits; a
//! spoken no still says Don't. Never from a meeting's audio: only the
//! assistant's own shortcut asks (and it's off during meetings).
//!
//! The slot ([`Waiting`]) holds one question at a time — the latest asked —
//! and goes when its card is answered, timed out, or its turn ends.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::chat::pages::act::confirm::ConnectorCard;

/// The bar card's id (its rule in `notices/notice.rs`).
pub const CARD: &str = "connectorAsk";
/// How long a spoken answer may come: the card's own wait.
pub const WAIT: Duration = crate::chat::pages::act::confirm::WAIT;
/// What Yap says to a spoken yes that can't answer (a click only).
pub const NEEDS_CLICK: &str = "That one needs a click: press Allow on the Yap bar.";
/// How much of the main argument the line says.
const SAY_CHARS: usize = 60;
/// How many arguments the bar card lists.
const CARD_ARGS: usize = 3;

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

/// **The line Yap says** before a connector's tool (`spoken`: a spoken yes
/// may answer it): "Use Notion to create page "Launch notes"? Say yes, or
/// press Allow." — the tool's title in words, its main argument when it's
/// short and no secret. Pure.
pub fn line(c: &ConnectorCard, spoken: bool) -> String {
    let what = words_of(&c.tool);
    let main = c.args.first().filter(|a| !a.secret && !a.cut && !a.value.trim().is_empty() && !a.value.contains('\n') && a.value.chars().count() <= SAY_CHARS);
    let mut q = format!("Use {} to {what}", c.connector.trim());
    if let Some(a) = main {
        q.push_str(&format!(" \u{201c}{}\u{201d}", a.value.trim()));
    }
    q.push('?');
    if spoken {
        q.push_str(" Say yes, or press Allow.");
    } else {
        q.push_str(" It needs a click: press Allow on the Yap bar.");
    }
    q
}

/// A tool's title or name as words to say: "Create page" → "create page",
/// "list_lights" → "list lights", "HassTurnOn" → "hass turn on" (a word in
/// capitals stays so: "Send SMS"). Pure.
pub fn words_of(tool: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for ch in tool.trim().chars() {
        if ch == '_' || ch == '-' || ch.is_whitespace() {
            if !out.ends_with(' ') && !out.is_empty() {
                out.push(' ');
            }
            prev_lower = false;
            continue;
        }
        if ch.is_uppercase() && prev_lower && !out.ends_with(' ') {
            out.push(' ');
        }
        prev_lower = ch.is_lowercase() || ch.is_ascii_digit();
        out.push(ch);
    }
    out.split(' ')
        .filter(|w| !w.is_empty())
        .map(|w| if w.chars().count() > 1 && w.chars().all(|c| !c.is_lowercase()) { w.to_string() } else { w.to_lowercase() })
        .collect::<Vec<_>>()
        .join(" ")
}

/// **The bar's card** for question `id`: "Use Notion: Create page?", its
/// first arguments as key: value lines (secrets masked as on the thread's
/// card), **Allow once** and **Don't**. It waits for its answer; its ✕
/// only hides it (the question still waits in Chat). Pure.
pub fn card(id: u64, c: &ConnectorCard) -> crate::bar::Card {
    let mut body: Vec<String> = c.args.iter().take(CARD_ARGS).map(|a| format!("{}: {}", a.key, a.value.lines().next().unwrap_or("").trim())).collect();
    if c.args.len() > CARD_ARGS {
        body.push(format!("and {} more in Chat", c.args.len() - CARD_ARGS));
    }
    crate::bar::Card {
        id: CARD.into(),
        icon: "assistant",
        title: format!("Use {}: {}?", c.connector.trim(), c.tool.trim()),
        body: body.join("\n"),
        status: (!c.spoken).then(|| "This one needs a click".to_string()),
        primary: Some(crate::bar::CardAction::new(format!("allow:{id}"), "Allow once")),
        secondary: Some(crate::bar::CardAction::new(format!("deny:{id}"), "Don't")),
        timeout_ms: None,
        ..Default::default()
    }
}

/// What a bar card's button says: `allow:<id>` / `deny:<id>` → (id,
/// allow). Pure.
pub fn action_of(action: &str) -> Option<(u64, bool)> {
    if let Some(id) = action.strip_prefix("allow:") {
        return id.parse().ok().map(|id| (id, true));
    }
    action.strip_prefix("deny:").and_then(|id| id.parse().ok()).map(|id| (id, false))
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
    use crate::chat::pages::act::confirm::CardArg;

    fn notion(spoken: bool) -> ConnectorCard {
        ConnectorCard {
            connector: "Notion".into(),
            tool: "Create page".into(),
            args: vec![
                CardArg { key: "title".into(), value: "Launch notes".into(), cut: false, secret: false },
                CardArg { key: "body".into(), value: "Agenda\nOwners".into(), cut: false, secret: false },
            ],
            always: true,
            why: "ask".into(),
            kind: "changes".into(),
            voice: true,
            spoken,
        }
    }

    #[test]
    fn the_line_says_the_connector_the_tool_and_its_main_argument() {
        assert_eq!(line(&notion(true), true), "Use Notion to create page \u{201c}Launch notes\u{201d}? Say yes, or press Allow.");
        assert_eq!(line(&notion(false), false), "Use Notion to create page \u{201c}Launch notes\u{201d}? It needs a click: press Allow on the Yap bar.");
        // A secret, a long value or none: not said.
        let mut c = notion(true);
        c.args[0].secret = true;
        assert_eq!(line(&c, true), "Use Notion to create page? Say yes, or press Allow.");
        c.args.clear();
        c.tool = "HassTurnOn".into();
        assert_eq!(line(&c, true), "Use Notion to hass turn on? Say yes, or press Allow.");
        assert_eq!(words_of("delete_all"), "delete all");
        assert_eq!(words_of("Send SMS"), "send SMS");
    }

    #[test]
    fn the_bar_card_asks_allow_once_or_dont_and_reads_back() {
        let c = card(7, &notion(true));
        assert_eq!(c.id, CARD);
        assert_eq!(c.title, "Use Notion: Create page?");
        assert_eq!(c.body, "title: Launch notes\nbody: Agenda");
        assert_eq!(c.primary.as_ref().map(|a| (a.id.as_str(), a.label.as_str())), Some(("allow:7", "Allow once")));
        assert_eq!(c.secondary.as_ref().map(|a| (a.id.as_str(), a.label.as_str())), Some(("deny:7", "Don't")));
        assert!(c.timeout_ms.is_none() && c.status.is_none());
        assert_eq!(card(7, &notion(false)).status.as_deref(), Some("This one needs a click"));
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

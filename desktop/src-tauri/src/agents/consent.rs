//! **Whose idea an agent is**: the person's own words, never a note's, a
//! page's or an agent's own output (as `apps::consented` for opening an app).
//!
//! - **Starting one** ([`start`]): the person's message this turn names the
//!   agent's folder (by its name, its last part or its path), or it's a plain
//!   "yes" to the answer just before, which asked a question and named it
//!   ("Shall I start an agent in garden-planner to fix the tests?" → "Yes.").
//!   A folder is the same word in every language, so this holds in all of
//!   them.
//! - **In a Code chat** ([`start_in`]): an agent in the chat's own folder needs
//!   only the person's own request this turn (they picked the folder); any
//!   other folder, and every plain chat, as above.
//! - **Telling one something** ([`message`]): what goes to the agent carries
//!   the person's own words this turn (a word of theirs that matters, not
//!   "the" or "please"), or their message is a plain "yes" (to a plan, to
//!   the agent's question).
//! - **"Go ahead"** after a plan ([`go_ahead`]); a message that's nothing
//!   but a go-ahead or a yes ([`plain_go_ahead`]: after the web, the one
//!   thing still passed to an agent, in Yap's own words).
//! - **Less than the chat's Mode** ([`asks_lower_mode`], 2026-11-07): the
//!   chat's model may lower an agent's mode only when the person's own words
//!   ask for it — a plan first, no changes yet, or to be asked first — in
//!   their languages (My languages' codes, English always; none of theirs
//!   known: all seven), as `dictation/command_screen.rs` reads them.
//! - **A plan, and only a plan** ([`asks_for_plan`], 8 Nov 2026): Plan mode
//!   only when the person's words ask for a plan ("make a plan first",
//!   "show me the plan before you change anything"); "don't change
//!   anything" or "read only" is a read-only run, not a plan.
//!
//! Pure.

use std::sync::LazyLock;

use regex::Regex;

use super::workspaces::{self, Folder};

/// Did the person ask for an agent in `folder`: their `question` names it, or
/// it's a plain yes to `previous` (the answer just before), which asked a
/// question and named it?
pub fn start(question: &str, previous: Option<&str>, folder: &Folder) -> bool {
    if names(question, folder) {
        return true;
    }
    let Some(previous) = previous else { return false };
    crate::apps::is_affirmative(question) && previous.contains('?') && names(previous, folder)
}

/// **In a Code chat** (`chat`: the folder the person attached the chat to,
/// `agents::code_chat`): an agent in that folder is theirs when their message
/// this turn asks anything at all — they picked the folder, so "fix the
/// failing tests" is enough, and the model decides, as it does for any
/// tool. An agent in another folder, and every agent in a plain chat (`chat`
/// `None`): [`start`], exactly as before.
pub fn start_in(question: &str, previous: Option<&str>, folder: &Folder, chat: Option<&Folder>) -> bool {
    let here = chat.is_some_and(|c| workspaces::norm(&c.path) == workspaces::norm(&folder.path));
    (here && !workspaces::words(question).is_empty()) || start(question, previous, folder)
}

/// Does `text` name `folder`: its name's words in a row, its path's last
/// part's, or its path?
pub fn names(text: &str, folder: &Folder) -> bool {
    let said = workspaces::words(text);
    let has = |phrase: &str| {
        let p = workspaces::words(phrase);
        !p.is_empty() && said.windows(p.len()).any(|w| w == p.as_slice())
    };
    has(&folder.name) || has(&workspaces::name_of(&folder.path)) || workspaces::said_path(&[text], &folder.path)
}

/// Little words that never make a message the person's ("please do the").
/// ("job" too: what someone who knew it as Jobs may still say.)
const LITTLE: &[&str] = &[
    "the", "and", "for", "with", "that", "this", "then", "also", "too", "please", "can", "could", "would", "you", "your", "its",
    "it's", "him", "her", "them", "tell", "ask", "say", "said", "agent", "agents", "job", "jobs", "claude", "code", "just", "now", "what", "how",
    "are", "was", "were", "will", "into", "from", "about", "have", "has", "had", "not", "but", "all", "any", "there", "here",
];

/// The words of `s` that matter: three letters or more, not [`LITTLE`].
fn content_words(s: &str) -> Vec<String> {
    workspaces::words(s).into_iter().filter(|w| w.chars().count() >= 3 && !LITTLE.contains(&w.as_str())).collect()
}

/// May `text` go to an agent, from the person's `question` this turn: it
/// carries a word of theirs that matters, or their message is a plain yes.
pub fn message(question: &str, text: &str) -> bool {
    if crate::apps::is_affirmative(question) || go_ahead(question) {
        return true;
    }
    let theirs = content_words(question);
    !theirs.is_empty() && content_words(text).iter().any(|w| theirs.contains(w))
}

/// "Go ahead", "do it", "carry on", "proceed", "yes, make the changes": a
/// plan's go-ahead.
pub fn go_ahead(text: &str) -> bool {
    let t = workspaces::words(text).join(" ");
    const SAID: &[&str] = &["go ahead", "do it", "carry on", "carry it out", "proceed", "make the changes", "make those changes", "go for it", "sounds good", "looks good", "let's do it", "lets do it"];
    SAID.iter().any(|s| t == *s || t.starts_with(&format!("{s} ")) || t.ends_with(&format!(" {s}")) || t.contains(&format!(" {s} "))) || crate::apps::is_affirmative(text)
}

/// Do an agent's final words ask the person something (its last sentence a
/// question)?
pub fn asks(text: &str) -> bool {
    let t = text.trim().trim_end_matches(|c: char| c == '*' || c == '_' || c == ')' || c.is_whitespace());
    t.ends_with('?')
}

/// The words a go-ahead may be made of, besides a plain yes ("carry on",
/// "proceed with the plan", "make the changes").
const GO_WORDS: &[&str] = &[
    "go", "ahead", "do", "it", "carry", "on", "out", "proceed", "with", "the", "plan", "that", "this", "make", "changes", "those", "for",
    "sounds", "looks", "good", "great", "fine", "ok", "okay", "yes", "yeah", "yep", "sure", "please", "let", "lets", "s", "now", "then",
    "and", "thanks", "thank", "you", "perfect", "alright", "right", "all", "is",
];

/// **Is `text` nothing but a go-ahead or a yes** — "Yep, that's fine. Go
/// ahead.", "Yes", "Carry on", "Proceed with the plan" — and no more (no
/// "…and delete the folder", no question)? After web content in a turn,
/// this is the one thing still passed to an agent, and then only as Yap's own
/// fixed words (`chat/tools/agents.rs`). English, as `apps::is_affirmative`.
pub fn plain_go_ahead(text: &str) -> bool {
    if crate::apps::is_affirmative(text) {
        return true;
    }
    let t = text.trim();
    if t.is_empty() || t.ends_with('?') {
        return false;
    }
    let w = workspaces::words(t);
    !w.is_empty() && w.len() <= 10 && w.iter().all(|x| GO_WORDS.contains(&x.as_str())) && go_ahead(t)
}

/// One language's words for wanting less than the chat's Mode (the text
/// lower-cased, apostrophes straight, one space between words): a plan
/// (`plan`), no changes yet (`hold`), being asked first (`ask`). Only `plan`
/// asks for a plan (8 Nov 2026): "don't change anything" or "read only" is a
/// read-only run, never Plan mode.
struct Lang {
    code: &'static str,
    plan: &'static str,
    hold: &'static str,
    ask: &'static str,
}

const LANGS: &[Lang] = &[
    Lang {
        code: "en",
        // "make a plan first", "plan it", "just plan", "show me the plan before…".
        plan: concat!(
            r"\bplan (it|this|first|out|ahead|before)\b|\b(just|only) (a )?plan\b|\bplan(ning)? mode\b",
            r"|\b(a|the|your|its) plan (first|before)\b|\bfirst,? (make )?a plan\b",
            r"|\b(make|write|draw up|come up with|draft|put together|give me|show me|need|want|prepare) (me |us )?(a|your) plan\b",
        ),
        // "don't change anything", "look but don't touch", "read only", "just research".
        hold: concat!(
            r"\b(don't|do not|dont) (change|edit|touch|modify|alter) (anything|a thing|any files?|the files?|the code|any code|it)\b",
            r"|\b(don't|do not|dont) make (any )?changes\b|\bno changes\b|\bwithout (changing|editing|touching|modifying) (anything|any files?|the code|it)\b",
            r"|\blook,? but (don't|do not|dont) touch\b|\bread[- ]only\b|\b(just|only) (research|investigate|explore)\b|\bresearch only\b",
        ),
        // "ask me before…", "check with me".
        ask: concat!(
            r"\bask me (first|before)\b|\bask (for )?my (permission|ok|okay|approval)\b|\bask before\b",
            r"|\b(check|confirm) with me\b|\bwait for my (ok|okay|approval|go-ahead|go ahead)\b",
        ),
    },
    Lang {
        code: "fr",
        plan: concat!(
            r"\b(fais|faites|fais-moi|faites-moi|prépare|préparez|propose|proposez|établis|rédige|rédigez|écris|écrivez) (d'abord )?(un|le|ton|votre) plan\b",
            r"|\b(un|le) plan d'abord\b|\bd'abord un plan\b|\b(juste|seulement) (un )?plan\b|\bplanifie(z)?\b|\bmode plan\b",
        ),
        hold: concat!(
            r"\bne (change|changez|modifie|modifiez|touche|touchez) (à )?rien\b|\bsans (rien )?(changer|modifier|toucher)\b",
            r"|\baucun(e)? (changement|modification)\b|\blecture seule\b|\b(juste|seulement) (des )?recherches?\b|\brecherche seulement\b",
        ),
        ask: r"\bdemande(z)?(-moi| moi)? (d'abord|avant)\b|\b(vérifie|vérifiez|vois|voyez) avec moi\b",
    },
    Lang {
        code: "de",
        plan: concat!(
            r"\b(mach|mache|macht|erstelle|erstell|erstellt|schreib|schreibe|schreibt) (mir |uns )?(zuerst |erst |erstmal |vorher )?(einen|den|nur einen) plan\b",
            r"|\b(erst|zuerst|erstmal) (einen |ein )?plan\b|\bnur (einen )?plan\b|\bnur planen\b|\bplane (es|das|erst|zuerst)\b|\bplan(ungs)?modus\b",
        ),
        hold: concat!(
            r"\b(ändere|änder|ändert|verändere) (noch )?nichts\b|\bnichts (ändern|verändern|anfassen|anrühren)\b|\bfass(e)? nichts an\b",
            r"|\bkeine änderungen\b|\bohne (etwas )?zu ändern\b|\bnur lesen\b|\bschreibgeschützt\b|\bnur recherch\w*\b",
        ),
        ask: r"\bfrag(e)? mich (zuerst|vorher|erst)\b|\b(sprich|stimm) dich (vorher |zuerst )?mit mir ab\b|\bfrag (vorher|zuerst) nach\b",
    },
    Lang {
        code: "es",
        plan: concat!(
            r"\b(haz|hazme|prepara|prepárame|preparame|escribe|escríbeme|propón|proponme|elabora) (primero )?(un|el) plan\b",
            r"|\bun plan primero\b|\bprimero un plan\b|\bs[oó]lo (un )?plan\b|\bplanifica(lo)?\b|\bmodo plan\b",
        ),
        hold: concat!(
            r"\bno (cambies|modifiques|toques|cambie|modifique|toque) nada\b|\bsin (cambiar|modificar|tocar) nada\b|\bningún cambio\b|\bsin cambios\b",
            r"|\bs[oó]lo lectura\b|\bmira pero no toques\b|\bs[oó]lo (investiga|investigar|investigación|explora)\b",
        ),
        ask: r"\bpreg[uú]ntame (antes|primero)\b|\bcons[uú]ltame\b|\bconsulta conmigo\b",
    },
    Lang {
        code: "it",
        plan: concat!(
            r"\b(fai|fammi|prepara|preparami|scrivi|scrivimi|proponi|proponimi) (prima )?(un|il) piano\b",
            r"|\bprima un piano\b|\bun piano prima\b|\bsolo (un )?piano\b|\bpianifica\b|\bmodalità piano\b",
        ),
        hold: concat!(
            r"\bnon (cambiare|modificare|toccare) (niente|nulla)\b|\bsenza (cambiare|modificare|toccare) (niente|nulla)\b|\bnessuna modifica\b",
            r"|\bsola lettura\b|\bguarda ma non toccare\b|\bsolo (ricerca|ricercare|indagare|esplorare)\b",
        ),
        ask: r"\bchiedimi (prima|il permesso)\b|\bchiedi prima\b|\bconsultami\b|\bverifica con me\b",
    },
    Lang {
        code: "pt",
        plan: concat!(
            r"\b(faz|faça|faca|prepara|prepare|escreve|escreva|propõe|proponha|monta|monte) (primeiro )?(um|o) plano\b",
            r"|\bum plano primeiro\b|\bprimeiro um plano\b|\b(só|apenas) (um )?plano\b|\bplaneja\b|\bplaneje\b|\bmodo plano\b",
        ),
        hold: concat!(
            r"\bnão (mudes|mude|alteres|altere|toques|toque|modifiques|modifique) nada\b|\bsem (mudar|alterar|tocar|modificar) nada\b",
            r"|\bnenhuma (mudança|alteração)\b|\b(somente|só|apenas) leitura\b|\b(só|apenas) (pesquisa|pesquisar|investiga|investigar)\b",
        ),
        ask: r"\b(pergunta-me|pergunte-me|me pergunte|me pergunta) (antes|primeiro)\b|\bconsulte?-me\b|\bfala comigo antes\b|\bverifique? comigo\b",
    },
    Lang {
        code: "nl",
        plan: r"\b(maak|schrijf|stel) (eerst )?(een|het) plan\b|\beerst een plan\b|\balleen (een )?plan\b|\balleen plannen\b|\bplan het\b|\bplanmodus\b",
        hold: concat!(
            r"\b(verander|wijzig) (nog )?niets\b|\bniets (veranderen|wijzigen|aanraken|aanpassen)\b|\braak niets aan\b",
            r"|\bgeen (wijzigingen|veranderingen)\b|\bzonder (iets )?te (veranderen|wijzigen)\b|\balleen[- ]lezen\b|\balleen (onderzoek|onderzoeken|uitzoeken)\b",
        ),
        ask: r"\bvraag (het )?(mij|me) (eerst|vooraf)\b|\bvraag eerst\b|\b(overleg|check) (eerst )?met (mij|me)\b",
    },
];

/// Each language's rules, compiled once: its plan words, and all its words
/// for less (a plan, no changes yet, asked first).
struct Rule {
    code: &'static str,
    plan: Regex,
    lower: Regex,
}

static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    LANGS
        .iter()
        .map(|l| Rule {
            code: l.code,
            plan: Regex::new(l.plan).expect("static regex"),
            lower: Regex::new(&format!("{}|{}|{}", l.plan, l.hold, l.ask)).expect("static regex"),
        })
        .collect()
});

/// The text as the rule reads it: lower case, straight apostrophes, one
/// space between words.
fn normal(text: &str) -> String {
    text.to_lowercase().replace(['\u{2019}', '\u{2018}', '`'], "'").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Do `text`'s words match `rule` (a language's plan words, or all its words
/// for less) in `langs` (My languages' codes; English always; none of theirs
/// known: all)?
fn says(text: &str, langs: &[String], rule: impl Fn(&Rule) -> &Regex) -> bool {
    let text = normal(text);
    if text.is_empty() {
        return false;
    }
    let all = langs.iter().all(|c| !LANGS.iter().any(|l| c.to_ascii_lowercase().starts_with(l.code)));
    let mine = |code: &str| all || code == "en" || langs.iter().any(|c| c.to_ascii_lowercase().starts_with(code));
    RULES.iter().filter(|r| mine(r.code)).any(|r| rule(r).is_match(&text))
}

/// The person's `question` this turn says so, or it's a plain yes to
/// `previous` (the answer just before), which asked a question that did.
fn asked(question: &str, previous: Option<&str>, langs: &[String], rule: impl Fn(&Rule) -> &Regex + Copy) -> bool {
    if says(question, langs, rule) {
        return true;
    }
    previous.is_some_and(|p| crate::apps::is_affirmative(question) && p.contains('?') && says(p, langs, rule))
}

/// **Did the person ask for less than the chat's Mode** — a plan ("make a
/// plan first", "plan it", "just plan"), no changes yet ("don't change
/// anything", "look but don't touch", "read only", "just research"), or to
/// be asked first ("ask me before…", "check with me")? Their `question`
/// this turn says so, or it's a plain yes to `previous` (the answer just
/// before), which asked a question that did. Only then may the chat's
/// model lower an agent's mode (`modes::model_pick`); "look into this and make
/// our own version" never does.
pub fn asks_lower_mode(question: &str, previous: Option<&str>, langs: &[String]) -> bool {
    asked(question, previous, langs, |r| &r.lower)
}

/// **Did the person ask for a plan** — "make a plan first", "plan it",
/// "show me the plan before you change anything" — this turn (or say yes to
/// an offer of one)? Only then may the model lower an agent to Plan mode
/// (`modes::model_pick`, 8 Nov 2026). "Don't change anything", "read only"
/// or "just research" is not a plan: that's a read-only run.
pub fn asks_for_plan(question: &str, previous: Option<&str>, langs: &[String]) -> bool {
    asked(question, previous, langs, |r| &r.plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn garden() -> Folder {
        Folder { name: "garden-planner".into(), path: r"D:\Projects\garden-planner".into(), cap: None, added_ts: 1 }
    }

    #[test]
    fn an_agent_starts_only_when_the_person_names_its_folder_or_says_yes_to_it() {
        let f = garden();
        assert!(start("Start an agent in garden-planner to fix the failing tests", None, &f));
        assert!(start("in my garden planner project, tidy the readme", None, &f), "spoken: no hyphen");
        assert!(start(r"Have Claude Code fix D:\Projects\garden-planner\src", None, &f), "its path");
        // A yes to the answer just before, which asked and named it.
        let offer = Some("I can start an agent in garden-planner to fix them. Shall I?");
        assert!(start("Yes.", offer, &f));
        assert!(start("yes please", offer, &f));
        // Not a yes, or a yes to something else.
        assert!(!start("Are you sure?", offer, &f));
        assert!(!start("Yes.", Some("Do you want a summary of the note?"), &f));
        assert!(!start("Yes.", Some("I started an agent in garden-planner."), &f), "no question was asked");
        // The folder's name nowhere in the person's words (a note said it).
        assert!(!start("Summarise my reminders note", None, &f));
        assert!(!start("fix the tests", None, &f));
        assert!(!start("Yes.", None, &f));
    }

    #[test]
    fn in_a_code_chat_the_persons_own_request_is_enough_for_its_folder() {
        let garden = garden();
        let recipes = Folder { name: "recipe-box".into(), path: r"D:\Projects\recipe-box".into(), cap: None, added_ts: 1 };
        // The chat's own folder: their request this turn, without naming it.
        assert!(start_in("Fix the failing tests", None, &garden, Some(&garden)));
        assert!(start_in("add a dark theme", None, &garden, Some(&Folder { path: r"d:/projects/garden-planner/".into(), ..garden.clone() })), "the same folder, however its path is written");
        assert!(start_in("Yes.", Some("Shall I fix them?"), &garden, Some(&garden)));
        // Nothing asked: nothing starts.
        assert!(!start_in("", None, &garden, Some(&garden)));
        assert!(!start_in("  ?! ", None, &garden, Some(&garden)));
        // Another folder from a Code chat: named by the person, as in a plain chat.
        assert!(!start_in("Fix the failing tests", None, &recipes, Some(&garden)));
        assert!(start_in("Fix the failing tests in recipe-box", None, &recipes, Some(&garden)));
    }

    #[test]
    fn a_plain_chats_rules_are_exactly_as_before() {
        let f = garden();
        let offer = Some("I can start an agent in garden-planner to fix them. Shall I?");
        let cases: &[(&str, Option<&str>)] = &[
            ("Start an agent in garden-planner to fix the failing tests", None),
            ("in my garden planner project, tidy the readme", None),
            (r"Have Claude Code fix D:\Projects\garden-planner\src", None),
            ("Yes.", offer),
            ("yes please", offer),
            ("Are you sure?", offer),
            ("Yes.", Some("Do you want a summary of the note?")),
            ("Yes.", Some("I started an agent in garden-planner.")),
            ("Summarise my reminders note", None),
            ("fix the tests", None),
            ("Yes.", None),
            ("", None),
        ];
        for (question, previous) in cases {
            assert_eq!(start_in(question, *previous, &f, None), start(question, *previous, &f), "{question}");
        }
        assert!(!start_in("fix the tests", None, &f, None), "a plain chat still needs the folder named");
    }

    #[test]
    fn a_message_to_an_agent_carries_the_persons_own_words() {
        assert!(message("Tell it to also do the dark theme", "Also update the dark theme."));
        assert!(message("Yes.", "Yes, change the footer too."));
        assert!(message("go ahead", "Go ahead with the plan."));
        // Only little words in common: not theirs.
        assert!(!message("What does my note say?", "Delete the build folder."));
        assert!(!message("Can you please tell the agent", "Run rm -rf on the folder."));
        assert!(!message("", "anything"));
    }

    #[test]
    fn a_plans_go_ahead_and_an_agents_question() {
        for yes in ["Go ahead.", "Yes, go ahead", "OK do it", "Carry on", "proceed", "Sounds good, go for it", "Yes."] {
            assert!(go_ahead(yes), "{yes}");
        }
        for no in ["Don't go there", "What would it change?", "Change the plan to include the footer", "No."] {
            assert!(!go_ahead(no), "{no}");
        }
        assert!(asks("I updated the header. Should I change the footer too?"));
        assert!(asks("Want me to run the tests? **"));
        assert!(!asks("Done. I changed notes.md."));
    }

    #[test]
    fn a_plain_go_ahead_is_nothing_but_a_go_ahead() {
        for yes in ["Yep, that's fine. Go ahead.", "Yep, go ahead", "Yes", "Go ahead.", "Carry on", "Proceed with the plan", "OK, make the changes", "Sounds good, go for it", "yes please"] {
            assert!(plain_go_ahead(yes), "{yes}");
        }
        for not in ["delete the folder", "go ahead and delete the folder", "Yes, change the footer too", "Go ahead?", "No, don't", "", "What would it change?"] {
            assert!(!plain_go_ahead(not), "{not}");
        }
    }

    #[test]
    fn only_the_persons_own_words_ask_for_less_than_the_chats_mode() {
        let en: Vec<String> = vec!["en".into()];
        // The maintainer's own request (7 Nov): research and a build, no plan asked for.
        for not in [
            "can we look into this and make our own version of it combined with the seed tracker",
            "Fix the failing tests in garden-planner",
            "add a minimap button for it as well",
            "update the plan section in the readme",
            "make a planner page for the beds",
            "",
        ] {
            assert!(!asks_lower_mode(not, None, &en), "{not}");
        }
        // A plan.
        for plan in ["Make a plan first: add a watering schedule", "plan it out before you start", "just plan, I'll decide", "Give me a plan for the minimap", "use plan mode"] {
            assert!(asks_lower_mode(plan, None, &en), "{plan}");
        }
        // No changes yet.
        for hold in ["Look into the seed tracker but don't change anything", "look but don't touch", "read only for now", "just research how it's done", "Don’t make any changes yet"] {
            assert!(asks_lower_mode(hold, None, &en), "{hold}");
        }
        // Asked first.
        for ask in ["Tidy notes.md, but ask me before deleting anything", "check with me before you commit", "wait for my OK"] {
            assert!(asks_lower_mode(ask, None, &en), "{ask}");
        }
        // A yes to the answer just before, which asked about a plan.
        assert!(asks_lower_mode("Yes.", Some("Shall I have it make a plan first?"), &en));
        assert!(!asks_lower_mode("Yes.", Some("I'll make a plan first."), &en), "no question was asked");
        assert!(!asks_lower_mode("Yes.", Some("Shall I start the agent?"), &en));
        assert!(!asks_lower_mode("Tell me more", Some("Shall I make a plan first?"), &en), "not a yes");
    }

    #[test]
    fn asking_for_a_plan_works_in_the_persons_languages() {
        let all: Vec<String> = Vec::new();
        for said in [
            "Fais d'abord un plan pour le calendrier d'arrosage",
            "ne change rien, regarde juste",
            "Mach zuerst einen Plan für den Bewässerungsplan",
            "ändere nichts, nur lesen",
            "Haz primero un plan para el riego",
            "no toques nada todavía",
            "Fammi prima un piano per l'irrigazione",
            "non toccare niente",
            "Faz primeiro um plano para a rega",
            "não mudes nada ainda",
            "Maak eerst een plan voor de bewatering",
            "verander niets",
            "Frag mich vorher, bevor du etwas löschst",
            "pregúntame antes de borrar",
        ] {
            assert!(asks_lower_mode(said, None, &all), "{said}");
        }
        // The person's languages only (English always): French words with German and English.
        let de: Vec<String> = vec!["de".into()];
        assert!(asks_lower_mode("Mach zuerst einen Plan", None, &de));
        assert!(!asks_lower_mode("Fais d'abord un plan", None, &de));
        assert!(asks_lower_mode("make a plan first", None, &de), "English always");
        // A request in another language that asks for no plan: none.
        assert!(!asks_lower_mode("Corrige les tests qui échouent", None, &all));
        assert!(!asks_lower_mode("Füge einen Minimap-Knopf hinzu", None, &all));
    }

    #[test]
    fn only_a_plan_asked_for_is_a_plan_and_no_changes_is_read_only() {
        let en: Vec<String> = vec!["en".into()];
        let all: Vec<String> = Vec::new();
        // A plan, in so many words.
        for plan in ["Make a plan first: add a watering schedule", "plan it first", "show me the plan before you change anything", "just plan, I'll decide", "use plan mode"] {
            assert!(asks_for_plan(plan, None, &en), "{plan}");
            assert!(asks_lower_mode(plan, None, &en), "{plan}");
        }
        // No changes yet, asked first, or nothing at all: not a plan.
        for not in [
            "don't change anything, just look",
            "read only for now",
            "just research how the seed tracker works",
            "look but don't touch",
            "check with me before you commit",
            "Hey Claude, can you get yourself caught up with this add-on? When you get to speed with it, let me know.",
            "catch up on this folder",
            "update the plan section in the readme",
        ] {
            assert!(!asks_for_plan(not, None, &en), "{not}");
        }
        // A yes to an offer of a plan is one; to an offer of a look, not.
        assert!(asks_for_plan("Yes.", Some("Shall I have it make a plan first?"), &en));
        assert!(!asks_for_plan("Yes.", Some("Shall I have it look but not change anything?"), &en));
        // In the person's languages: a plan's words only.
        for plan in ["Fais d'abord un plan", "Mach zuerst einen Plan", "Haz primero un plan", "Fammi prima un piano", "Faz primeiro um plano", "Maak eerst een plan"] {
            assert!(asks_for_plan(plan, None, &all), "{plan}");
        }
        for hold in ["ne change rien", "ändere nichts", "no toques nada", "non toccare niente", "não mudes nada", "verander niets"] {
            assert!(!asks_for_plan(hold, None, &all), "{hold}");
            assert!(asks_lower_mode(hold, None, &all), "{hold}");
        }
    }
}

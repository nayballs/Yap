//! **Chat sizes each agent** (8 Nov 2026, the maintainer: "You give him a
//! task, he sends out agents"). `start_agent` and `message_agent` take a
//! `model` and an `effort` the chat's model picks for the task, after
//! savvy-flow's tiers ([`GUIDE`]): Haiku or low effort for quick mechanical
//! jobs, Sonnet at medium for ordinary work, Opus at high for delicate
//! multi-file changes, Opus at xhigh for hard debugging or messy, unfamiliar
//! code; max only when the person asks. And a one-line `sizing` reason, kept
//! on the turn (`TurnNote::sizing`: the Agent panel's ▸, "Opus 5.5 · High: a
//! multi-file change").
//!
//! - **The models** ([`models`]): the Claude models the person's Claude Code
//!   runs, from Yap's catalogue (`claude_plan::MODELS`, cheapest first: the
//!   order the ceiling reads), and Settings' own default. Anything else is
//!   left out (the default runs), and the model is told.
//! - **The person's own words win** ([`said`]): their message this turn
//!   names the model ("use Haiku": a family's name, the same word in every
//!   language) or an effort level ("max effort", "ultrathink"): theirs, kept
//!   for the agent's next turns (`model_asked`, `effort_asked`) and never
//!   lowered. "Think hard", "a quick one": theirs for this turn (never
//!   lowered), not kept. In their languages.
//! - **The chat's own choice** is made again each turn, and lowered to the
//!   most Settings let it choose ([`Ceiling`]: "Most Chat may choose",
//!   `agents_ceiling_model` / `agents_ceiling_effort`; "No limit" by
//!   default), the model told.
//! - **An effort the model doesn't take** is lowered to the nearest it does
//!   ([`taken`], `ai/effort.rs`'s table).
//! - Nothing picked: Settings' defaults (`agents_model`, `agents_effort`),
//!   in `runner::model_for`.

use std::sync::LazyLock;

use regex::Regex;

use crate::ai::effort::Level;

/// **The guide** in `start_agent`'s `model` description (kept short: every
/// chat with agents carries it, an 8K model's room too).
pub const GUIDE: &str = "Size it to the task: Haiku at low for quick mechanical jobs (a rename, a typo, running tests), Sonnet at medium for ordinary work, Opus at high for delicate multi-file changes, Opus at xhigh for hard debugging or messy, unfamiliar code; max only if the user asks. The user's words win (\"use Haiku\", \"think hard\", \"quick one\")";

/// **The models an agent may run on**, cheapest first: Yap's catalogue of the
/// Claude models Claude Code runs (`claude_plan::MODELS`).
pub fn models() -> Vec<&'static str> {
    crate::ai::claude_plan::MODELS.iter().map(|(id, _)| *id).collect()
}

/// A model's short name ("claude-opus-5-5" → "Opus 5.5"), as the Agent
/// panel says it. Pure.
pub fn short_name(id: &str) -> String {
    crate::ai::claude_plan::MODELS
        .iter()
        .find(|(m, _)| *m == id)
        .map(|(_, name)| name.trim_start_matches("Claude ").to_string())
        .unwrap_or_else(|| id.to_string())
}

/// **`id` as a model an agent runs on**: one of [`models`] (a dated
/// snapshot as its alias), or Settings' `default`; `None`: neither. Pure.
pub fn valid(id: &str, default: &str) -> Option<String> {
    let id = id.trim().to_ascii_lowercase();
    if id.is_empty() {
        return None;
    }
    let base = crate::ai::model_catalog::snapshot_base(&id).unwrap_or_else(|| id.clone());
    if let Some(m) = models().into_iter().find(|m| *m == id || *m == base) {
        return Some(m.to_string());
    }
    let default = default.trim();
    (!default.is_empty() && default.eq_ignore_ascii_case(&id)).then(|| default.to_string())
}

/// Its place in the catalogue, cheapest first (`None`: not in it). Pure.
pub fn rank(id: &str) -> Option<usize> {
    models().iter().position(|m| *m == id)
}

/// **The level `model` runs `level` at**: the highest it takes at or below
/// it (else its lowest); as asked for a model with no effort table. Pure,
/// but for the provider's list (`effort::caps`).
pub fn taken(model: &str, level: Level) -> Level {
    let Some(caps) = crate::ai::effort::caps("anthropic", model) else { return level };
    caps.levels.iter().rev().find(|l| **l <= level).or(caps.levels.first()).copied().unwrap_or(level)
}

/// **The most the chat may choose** (Settings → Yap Code, "Most Chat may
/// choose"): a model and an effort; `None` ("No limit", the default).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ceiling {
    pub model: String,
    pub effort: Level,
}

impl Ceiling {
    /// Settings' ceiling: none while its model is "" ("No limit") or not one
    /// of [`models`]; its effort "max" when it isn't a level.
    pub fn of(cfg: &crate::config::YapConfig) -> Option<Ceiling> {
        let model = valid(&cfg.agents_ceiling_model, "").filter(|m| rank(m).is_some())?;
        Some(Ceiling { model, effort: Level::parse(&cfg.agents_ceiling_effort).unwrap_or(Level::Max) })
    }
}

/// **What the person's own words said** about an agent's size this turn
/// ([`said`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Said {
    /// They named the model ("use Haiku"): theirs, kept.
    pub model: bool,
    /// They named an effort level ("max effort", "ultrathink"): theirs, kept.
    pub level: bool,
    /// They asked for more or less thinking ("think hard", "a quick one"):
    /// theirs this turn (never lowered), not kept.
    pub effort: bool,
}

/// One language's words for an agent's effort (the text lower-cased,
/// apostrophes straight, one space between words): naming a level
/// (`level`), asking for more or less thinking (`cue`).
struct Lang {
    code: &'static str,
    level: &'static str,
    cue: &'static str,
}

const LANGS: &[Lang] = &[
    Lang {
        code: "en",
        level: r"\b(low|medium|high|extra|xhigh|x-high|max|maximum|minimum|minimal) effort\b|\beffort (of |at |on |to )?(low|medium|high|extra|xhigh|max|maximum)\b|\bultrathink\b",
        cue: r"\bthink (really |very |extra )?(hard|harder|carefully|deeply|longer)\b|\bthink (it|this) through\b|\b(a )?quick one\b|\b(don't|do not|dont) overthink\b",
    },
    Lang {
        code: "fr",
        level: r"\beffort (faible|bas|moyen|élevé|maximal|maximum|max)\b|\b(faible|petit|gros|grand) effort\b",
        cue: r"\bréfléchis(sez)? (bien|beaucoup|longuement|à fond)\b|\b(un truc|une chose|un petit truc) rapide\b",
    },
    Lang {
        code: "de",
        level: r"\b(niedrige[nmr]?|geringe[nmr]?|mittlere[nmr]?|hohe[nmr]?|maximale[nmr]?) aufwand\b|\baufwand (niedrig|mittel|hoch|maximal)\b",
        cue: r"\bdenk(e|t)? (gründlich|genau|lange|intensiv|gut) (darüber )?nach\b|\b(was|etwas) schnelles\b",
    },
    Lang {
        code: "es",
        level: r"\besfuerzo (bajo|medio|alto|máximo|maximo)\b|\b(poco|mucho|máximo|maximo) esfuerzo\b",
        cue: r"\bpi[eé]nsa(lo)? (bien|mucho|a fondo|detenidamente)\b|\balgo r[aá]pido\b",
    },
    Lang {
        code: "it",
        level: r"\bsforzo (basso|medio|alto|massimo)\b|\b(poco|massimo) sforzo\b",
        cue: r"\bpensa(ci)? (bene|a fondo|attentamente)\b|\buna cosa (veloce|rapida)\b",
    },
    Lang {
        code: "pt",
        level: r"\besforço (baixo|médio|medio|alto|máximo|maximo)\b|\b(pouco|muito|máximo|maximo) esforço\b",
        cue: r"\bpens[ae] (bem|muito|a fundo)\b|\b(algo|coisa) r[aá]pid[ao]\b",
    },
    Lang {
        code: "nl",
        level: r"\b(lage|gemiddelde|hoge|maximale) (moeite|inspanning)\b|\b(moeite|inspanning) (laag|gemiddeld|hoog|maximaal)\b",
        cue: r"\bdenk (goed|grondig|diep|lang) na\b|\biets snels\b",
    },
];

struct Rule {
    code: &'static str,
    level: Regex,
    cue: Regex,
}

static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    LANGS.iter().map(|l| Rule { code: l.code, level: Regex::new(l.level).expect("static regex"), cue: Regex::new(l.cue).expect("static regex") }).collect()
});

/// The text as the rules read it: lower case, straight apostrophes, one
/// space between words.
fn normal(text: &str) -> String {
    text.to_lowercase().replace(['\u{2019}', '\u{2018}', '`'], "'").split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `text`'s effort words in `langs` (My languages' codes; English always;
/// none of theirs known: all): a level named, a cue. Pure.
fn effort_words(text: &str, langs: &[String]) -> (bool, bool) {
    let text = normal(text);
    if text.is_empty() {
        return (false, false);
    }
    let all = langs.iter().all(|c| !LANGS.iter().any(|l| c.to_ascii_lowercase().starts_with(l.code)));
    let mine = |code: &str| all || code == "en" || langs.iter().any(|c| c.to_ascii_lowercase().starts_with(code));
    let rules: Vec<&Rule> = RULES.iter().filter(|r| mine(r.code)).collect();
    (rules.iter().any(|r| r.level.is_match(&text)), rules.iter().any(|r| r.cue.is_match(&text)))
}

/// Does `text` name `model`'s family ("Haiku" for "claude-haiku-5-5"), as a
/// word? Pure.
fn names_model(text: &str, model: &str) -> bool {
    let Some(family) = model.strip_prefix("claude-").and_then(|r| r.split('-').next()).filter(|f| f.len() >= 4) else { return false };
    normal(text).split(|c: char| !c.is_alphanumeric()).any(|w| w == family)
}

/// **What the person's own words said** about the agent's size: their
/// `question` this turn, or their plain yes to `previous` (the answer just
/// before), which asked a question that did. `model`: the model picked (its
/// family named is theirs). Pure.
pub fn said(question: &str, previous: Option<&str>, model: Option<&str>, langs: &[String]) -> Said {
    let yes_to = previous.filter(|p| crate::apps::is_affirmative(question) && p.contains('?'));
    let texts: Vec<&str> = std::iter::once(question).chain(yes_to).collect();
    let mut out = Said::default();
    for t in texts {
        out.model |= model.is_some_and(|m| names_model(t, m));
        let (level, cue) = effort_words(t, langs);
        out.level |= level;
        out.effort |= cue;
    }
    out
}

/// **An agent's size as the call asked it** ([`pick`]): the model and
/// effort for its turn (`None`: not picked, or not one there is), whether
/// they're the person's own (kept), the reason, and what the model is told.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Picked {
    pub model: Option<String>,
    pub effort: Option<Level>,
    pub model_asked: bool,
    pub effort_asked: bool,
    pub sizing: Option<String>,
    /// Left out or lowered: why, for the tool's result.
    pub note: Option<String>,
}

impl Picked {
    /// As a turn's request carries it (`runner::Ask`; `runner::model_for`
    /// fills in the rest).
    pub fn ask(&self) -> super::runner::Ask {
        super::runner::Ask {
            model: self.model.clone(),
            effort: self.effort.map(|l| l.id().to_string()),
            model_asked: self.model_asked,
            effort_asked: self.effort_asked,
            sizing: self.sizing.clone(),
            ..Default::default()
        }
    }
}

/// **The call's `model`, `effort` and `sizing`, as Yap takes them**: a
/// model not in the catalogue (nor Settings' `default`) and an effort that
/// isn't a level are left out (Settings' default runs), said; the person's
/// own (`said`, asked of the model picked) is theirs, never lowered; the
/// chat's own is lowered to `ceiling`'s model (by the catalogue's order) and
/// effort, said. Pure.
pub fn pick(model: &str, effort: &str, sizing: &str, said: impl Fn(Option<&str>) -> Said, ceiling: Option<&Ceiling>, default: &str) -> Picked {
    let mut notes = Vec::new();
    let mut lowered = Vec::new();
    let valid_model = valid(model, default);
    if !model.trim().is_empty() && valid_model.is_none() {
        notes.push(format!("\"{}\" isn't a model an agent runs on ({}): it runs on the default.", model.trim(), models().join(", ")));
    }
    let level = Level::parse(&effort.to_ascii_lowercase());
    if !effort.trim().is_empty() && level.is_none() {
        notes.push(format!("\"{}\" isn't an effort (low, medium, high, xhigh or max): it runs at the default.", effort.trim()));
    }
    let s = said(valid_model.as_deref());
    let model_asked = valid_model.is_some() && s.model;
    let effort_asked = level.is_some() && s.level;
    let effort_theirs = effort_asked || s.effort;
    let model = valid_model.map(|m| match ceiling {
        Some(c) if !model_asked && rank(&m).is_none_or(|r| rank(&c.model).is_some_and(|top| r > top)) => {
            lowered.push(short_name(&c.model));
            c.model.clone()
        }
        _ => m,
    });
    let effort = level.map(|l| match ceiling {
        Some(c) if !effort_theirs && l > c.effort => {
            lowered.push(format!("{} effort", c.effort.id()));
            c.effort
        }
        _ => l,
    });
    if !lowered.is_empty() {
        notes.push(format!("Lowered to {}: the most Yap's settings let you choose for an agent.", lowered.join(", ")));
    }
    // One line, short.
    let sizing = crate::agents::steps::clip(&sizing.split_whitespace().collect::<Vec<_>>().join(" "), 120);
    Picked {
        model,
        effort,
        model_asked,
        effort_asked,
        sizing: Some(sizing).filter(|s| !s.is_empty()),
        note: (!notes.is_empty()).then(|| notes.join(" ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: Option<&str>) -> Said {
        Said::default()
    }
    fn opus_high() -> Ceiling {
        Ceiling { model: "claude-opus-5-5".into(), effort: Level::High }
    }

    #[test]
    fn the_models_are_the_catalogues_cheapest_first() {
        let m = models();
        assert_eq!(m.first(), Some(&"claude-haiku-5-5"));
        assert!(rank("claude-haiku-5-5") < rank("claude-sonnet-5-5") && rank("claude-sonnet-5-5") < rank("claude-opus-5-5"));
        assert_eq!(short_name("claude-opus-5-5"), "Opus 5.5");
        assert_eq!(valid("claude-opus-5-5", "claude-sonnet-5-5").as_deref(), Some("claude-opus-5-5"));
        assert_eq!(valid(" Claude-Haiku-5-5 ", "").as_deref(), Some("claude-haiku-5-5"));
        assert_eq!(valid("claude-sonnet-5-5-20261001", "").as_deref(), Some("claude-sonnet-5-5"), "a dated snapshot: its alias");
        assert_eq!(valid("claude-opus-9", "claude-sonnet-5-5"), None);
        assert_eq!(valid("gpt-5.5", "claude-sonnet-5-5"), None);
        assert_eq!(valid("claude-sonnet-4-6", "claude-sonnet-4-6").as_deref(), Some("claude-sonnet-4-6"), "Settings' own default stays valid");
    }

    #[test]
    fn the_models_pick_is_used() {
        let p = pick("claude-opus-5-5", "high", "a multi-file change", none, None, "claude-sonnet-5-5");
        assert_eq!(p.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(p.effort, Some(Level::High));
        assert!(!p.model_asked && !p.effort_asked, "the chat's choice is resized each turn");
        assert_eq!(p.sizing.as_deref(), Some("a multi-file change"));
        assert_eq!(p.note, None);
        // Nothing picked: nothing (Settings' defaults run).
        assert_eq!(pick("", "", "", none, None, "claude-sonnet-5-5"), Picked::default());
    }

    #[test]
    fn an_invalid_id_or_effort_is_left_out_and_said() {
        let p = pick("claude-opus-9-9", "huge", "", none, None, "claude-sonnet-5-5");
        assert_eq!((p.model, p.effort), (None, None));
        let note = p.note.unwrap();
        assert!(note.starts_with("\"claude-opus-9-9\" isn't a model an agent runs on (claude-haiku-5-5, claude-sonnet-5-5, claude-opus-5-5"), "{note}");
        assert!(note.contains("\"huge\" isn't an effort"), "{note}");
    }

    #[test]
    fn an_effort_the_model_doesnt_take_is_lowered_to_the_nearest_it_does() {
        // Every model in the catalogue takes all five.
        for m in models() {
            assert_eq!(taken(m, Level::Max), Level::Max, "{m}");
        }
        // Sonnet 4.6 has no xhigh: high. Opus 4.5 stops at high.
        assert_eq!(taken("claude-sonnet-4-6", Level::Xhigh), Level::High);
        assert_eq!(taken("claude-sonnet-4-6", Level::Max), Level::Max);
        assert_eq!(taken("claude-opus-4-5", Level::Max), Level::High);
        // A model with no effort table: as asked.
        assert_eq!(taken("claude-mystery-1", Level::Xhigh), Level::Xhigh);
    }

    #[test]
    fn the_ceiling_lowers_the_chats_pick_but_never_the_persons_own() {
        let top = opus_high();
        // Fable at max, the chat's: Opus at high, said.
        let p = pick("claude-fable-5-2", "max", "", none, Some(&top), "claude-sonnet-5-5");
        assert_eq!((p.model.as_deref(), p.effort), (Some("claude-opus-5-5"), Some(Level::High)));
        assert_eq!(p.note.as_deref(), Some("Lowered to Opus 5.5, high effort: the most Yap's settings let you choose for an agent."));
        // Under it: untouched, nothing said.
        let p = pick("claude-sonnet-5-5", "medium", "", none, Some(&top), "claude-sonnet-5-5");
        assert_eq!((p.model.as_deref(), p.effort, p.note), (Some("claude-sonnet-5-5"), Some(Level::Medium), None));
        // A Sonnet ceiling lowers Opus; Haiku stays.
        let sonnet = Ceiling { model: "claude-sonnet-5-5".into(), effort: Level::Max };
        assert_eq!(pick("claude-opus-5-5", "xhigh", "", none, Some(&sonnet), "").model.as_deref(), Some("claude-sonnet-5-5"));
        assert_eq!(pick("claude-haiku-5-5", "low", "", none, Some(&sonnet), "").model.as_deref(), Some("claude-haiku-5-5"));
        // The person's own words go above it, since they asked: "use Fable, max effort".
        let theirs = |m: Option<&str>| Said { model: m == Some("claude-fable-5-2"), level: true, effort: false };
        let p = pick("claude-fable-5-2", "max", "", theirs, Some(&top), "");
        assert_eq!((p.model.as_deref(), p.effort, p.model_asked, p.effort_asked, p.note), (Some("claude-fable-5-2"), Some(Level::Max), true, true, None));
        // "Think hard": their effort this turn (not lowered), not kept.
        let hard = |_: Option<&str>| Said { effort: true, ..Default::default() };
        let p = pick("claude-sonnet-5-5", "xhigh", "", hard, Some(&top), "");
        assert_eq!((p.effort, p.effort_asked), (Some(Level::Xhigh), false));
    }

    #[test]
    fn no_limit_lets_anything_through() {
        let cfg = crate::config::YapConfig::default();
        assert_eq!(cfg.agents_ceiling_model, "", "No limit by default");
        assert_eq!(Ceiling::of(&cfg), None);
        let p = pick("claude-fable-5-2", "max", "hard debugging", none, Ceiling::of(&cfg).as_ref(), &cfg.agents_model);
        assert_eq!((p.model.as_deref(), p.effort, p.note), (Some("claude-fable-5-2"), Some(Level::Max), None));
        // A ceiling set: read from Settings; a model that isn't one there is: no limit.
        let set = crate::config::YapConfig { agents_ceiling_model: "claude-opus-5-5".into(), agents_ceiling_effort: "high".into(), ..Default::default() };
        assert_eq!(Ceiling::of(&set), Some(opus_high()));
        let gone = crate::config::YapConfig { agents_ceiling_model: "claude-opus-1".into(), ..Default::default() };
        assert_eq!(Ceiling::of(&gone), None);
    }

    #[test]
    fn the_persons_own_words_name_a_model_or_an_effort_in_their_languages() {
        let en = vec!["en".to_string()];
        let s = said("Use Haiku for this, it's a quick rename", None, Some("claude-haiku-5-5"), &en);
        assert!(s.model && !s.level && !s.effort);
        assert!(!said("Use Haiku for this", None, Some("claude-opus-5-5"), &en).model, "another model than the one they named");
        assert!(!said("rename the variable x to count", None, Some("claude-haiku-5-5"), &en).model);
        assert!(said("Fix it with max effort please", None, None, &en).level);
        assert!(said("ultrathink about the race", None, None, &en).level);
        let s = said("Think hard about why the tests flake", None, None, &en);
        assert!(s.effort && !s.level);
        assert!(said("Just a quick one: fix the typo", None, None, &en).effort);
        assert!(!said("the tests fail intermittently, find the root cause and fix it", None, None, &en).effort);
        // A yes to the chat's offer that named it.
        assert!(said("Yes please", Some("Shall I send an Opus agent at high effort?"), Some("claude-opus-5-5"), &en).model);
        assert!(said("Yes please", Some("Shall I send an Opus agent at high effort?"), None, &en).level);
        // In their own languages.
        let fr = vec!["fr".to_string()];
        assert!(said("Réfléchis bien avant de corriger", None, None, &fr).effort);
        assert!(said("Avec un effort maximal", None, None, &fr).level);
        let de = vec!["de".to_string()];
        assert!(said("Denk gründlich nach, bevor du es änderst", None, None, &de).effort);
        assert!(!said("Denk gründlich nach", None, None, &en).effort, "German only for a German speaker");
    }
}

//! **Cleanup strength** (2026-10-25; Wispr Flow's None / Light / Medium /
//! High, by behaviour): how much AI cleanup changes a dictation, one setting
//! for every app (Settings → Language Models → Dictation cleanup).
//!
//! - **None**: no cleanup — the same as cleanup off
//!   (`config.post_process_enabled`); the strength row's None turns it off.
//! - **Light**: punctuation, capitals and filler sounds only; the words stay
//!   as said, in their order (no grammar fixes, no rewording).
//! - **Medium**: what cleanup always did (the default, and what a config
//!   from before 25 Oct 2026 gets: the prompt is exactly the one before).
//! - **High**: also tightens and rephrases for clarity, keeping the meaning,
//!   every fact and the speaker's voice.
//!
//! **Yap's own layer, never the body.** Like a style's tone
//! (`cleanup_styles::line`), the strength is one line just before the
//! transcript ([`line`]): the guardrails and the editable body (a preset or
//! the person's own, Prompt Studio) go to the model as they are, and the
//! system prompt the server caches stays the same whatever the strength.
//!
//! **Global, not per style** (decided 25 Oct 2026): a style already says how
//! a kind of app sounds (formal, casual, literal…); strength says how much
//! the words may change. Two knobs per kind of app would be fourteen
//! choices for one idea, so strength is one setting, and the two fit
//! together by one rule ([`Strength::tone`], [`line`]): **a tone that keeps
//! the words as said** (AI prompts' Literal, Terminal's Command) **keeps
//! them at any strength**, and **at Light no tone rewords** (Formal, Casual
//! and Very casual write as Natural: the kind's line stays, its tone goes).
//!
//! "Use what I said" (`pasting/as_said.rs`) is unchanged: it swaps whatever
//! cleanup wrote for the words as said, and offers itself when the words
//! differ — at Light only where a filler went.

use crate::ai::cleanup_styles::{Style, Tone};
use crate::config::YapConfig;

/// How much AI cleanup changes a dictation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Strength {
    /// No cleanup (cleanup off).
    None,
    Light,
    Medium,
    High,
}

impl Strength {
    pub const ALL: [Strength; 4] = [
        Strength::None,
        Strength::Light,
        Strength::Medium,
        Strength::High,
    ];

    /// Its id in config.json (`cleanup_strength`) and the page.
    pub fn id(self) -> &'static str {
        match self {
            Strength::None => "none",
            Strength::Light => "light",
            Strength::Medium => "medium",
            Strength::High => "high",
        }
    }

    pub fn from_id(id: &str) -> Option<Strength> {
        let id = id.trim();
        Strength::ALL
            .into_iter()
            .find(|s| s.id().eq_ignore_ascii_case(id))
    }

    /// The strength set (`cleanup_strength`: missing or unknown, Medium —
    /// what cleanup always did), whether cleanup is on or not: with it off,
    /// no model is asked whatever this says. Pure.
    pub fn configured(cfg: &YapConfig) -> Strength {
        Strength::from_id(&cfg.cleanup_strength).unwrap_or(Strength::Medium)
    }

    /// The tone a style writes in at this strength: at Light, no tone
    /// rewords (Formal, Casual and Very casual become Natural); otherwise
    /// the style's own. Pure.
    pub fn tone(self, tone: Tone) -> Tone {
        match (self, tone) {
            (Strength::Light, Tone::Formal | Tone::Casual | Tone::VeryCasual) => Tone::Natural,
            _ => tone,
        }
    }

    /// `style` as it's written at this strength ([`Strength::tone`]). Pure.
    pub fn style(self, style: Option<Style>) -> Option<Style> {
        style.map(|s| Style {
            tone: self.tone(s.tone),
            ..s
        })
    }

    /// Whether this strength lays out what was said (a spoken list, an
    /// email's greeting and sign-off, `cleanup_format.rs`): Medium and High.
    /// Light keeps the words and their layout as said.
    pub fn formats(self) -> bool {
        matches!(self, Strength::Medium | Strength::High)
    }
}

/// Light's line: the words stay.
pub const LIGHT_LINE: &str = "Light cleanup: only remove filler sounds (um, uh, er) and stutters, and add punctuation and \
capitals. Keep every other word exactly as said, in the same order: no rewording and no grammar fixes.";

/// High's line: tighter and clearer, the same meaning and voice.
pub const HIGH_LINE: &str = "Thorough cleanup: also make it clear and concise. Cut repetition, rambling and wordy \
phrases, and rephrase awkward sentences, keeping the meaning, every fact, name and number, and the speaker's own \
voice. This wins over keeping the speaker's wording. Never add information.";

/// The strength's line just before the transcript (`None` for Medium —
/// exactly the prompt before strengths — and None, which asks no model).
/// A style whose tone keeps the words as said (Literal, Command) gets none:
/// it keeps them at any strength. `style`: the dictation's, as written at
/// this strength ([`Strength::style`]). Pure but for a model evaluation's
/// wording (`YAP_EVAL_STRENGTH_LINES`).
pub fn line(strength: Strength, style: Option<Style>) -> Option<String> {
    if style.is_some_and(|s| matches!(s.tone, Tone::Literal | Tone::Command | Tone::Off)) {
        return None;
    }
    let ours = match strength {
        Strength::Light => LIGHT_LINE,
        Strength::High => HIGH_LINE,
        Strength::Medium | Strength::None => return None,
    };
    let line = eval_wording(strength.id()).unwrap_or_else(|| ours.to_string());
    (!line.trim().is_empty()).then_some(line)
}

/// A model evaluation's own wording for a strength's line
/// (`YAP_EVAL_STRENGTH_LINES`: a JSON file of `{"light": "…", "high": "…"}`),
/// to compare wordings without a rebuild. Never outside a run.
fn eval_wording(key: &str) -> Option<String> {
    if !crate::ai::eval::active() {
        return None;
    }
    let path = std::env::var_os("YAP_EVAL_STRENGTH_LINES")?;
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    v.get(key).and_then(|x| x.as_str()).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::cleanup_styles::Category;

    fn style(category: Category, tone: Tone) -> Option<Style> {
        Some(Style { category, tone })
    }

    #[test]
    fn a_config_from_before_cleans_at_medium_and_off_is_none() {
        // A config without the field: serde's default, "medium".
        let mut cfg: YapConfig = serde_json::from_str("{}").unwrap();
        assert_eq!(cfg.cleanup_strength, "medium");
        assert!(!cfg.post_process_enabled, "cleanup off: None on the page");
        assert_eq!(Strength::configured(&cfg), Strength::Medium);
        cfg.cleanup_strength = "High".into();
        assert_eq!(Strength::configured(&cfg), Strength::High);
        cfg.cleanup_strength = "loud".into();
        assert_eq!(
            Strength::configured(&cfg),
            Strength::Medium,
            "an unknown one is what cleanup always did"
        );
        cfg.cleanup_strength = "none".into();
        assert_eq!(Strength::configured(&cfg), Strength::None);
    }

    #[test]
    fn medium_says_nothing_so_the_prompt_is_the_one_before() {
        for s in [
            None,
            style(Category::Email, Tone::Formal),
            style(Category::Other, Tone::Natural),
        ] {
            assert_eq!(line(Strength::Medium, s), None);
            assert_eq!(line(Strength::None, s), None);
        }
    }

    #[test]
    fn light_and_high_have_their_lines_unless_the_tone_keeps_the_words() {
        assert_eq!(line(Strength::Light, None).as_deref(), Some(LIGHT_LINE));
        assert_eq!(
            line(Strength::High, style(Category::Email, Tone::Formal)).as_deref(),
            Some(HIGH_LINE)
        );
        // AI prompts' Literal and Terminal's Command keep the words at any strength.
        for s in [
            style(Category::AiPrompts, Tone::Literal),
            style(Category::Terminal, Tone::Command),
        ] {
            assert_eq!(line(Strength::High, s), None);
            assert_eq!(line(Strength::Light, s), None);
        }
    }

    #[test]
    fn at_light_no_tone_rewords() {
        let light = Strength::Light;
        assert_eq!(light.tone(Tone::Formal), Tone::Natural);
        assert_eq!(light.tone(Tone::Casual), Tone::Natural);
        assert_eq!(light.tone(Tone::VeryCasual), Tone::Natural);
        assert_eq!(light.tone(Tone::Literal), Tone::Literal);
        assert_eq!(light.tone(Tone::Command), Tone::Command);
        // The kind stays: an email is still the body of an email.
        assert_eq!(
            light.style(style(Category::Email, Tone::Formal)),
            style(Category::Email, Tone::Natural)
        );
        // Medium and High keep the style's tone.
        assert_eq!(
            Strength::High.style(style(Category::Email, Tone::Formal)),
            style(Category::Email, Tone::Formal)
        );
        assert_eq!(
            Strength::Medium.style(style(Category::WorkChat, Tone::Casual)),
            style(Category::WorkChat, Tone::Casual)
        );
    }

    #[test]
    fn ids_round_trip_and_only_medium_and_high_format() {
        for s in Strength::ALL {
            assert_eq!(Strength::from_id(s.id()), Some(s));
        }
        assert!(!Strength::Light.formats());
        assert!(Strength::Medium.formats() && Strength::High.formats());
    }
}

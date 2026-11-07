//! **A job's mode**: Claude Code's own permission modes, worded as its
//! Shift+Tab menu words them (Chat's Mode chip, plan §3.6), and the rules a
//! chat's model meets when it asks for one.
//!
//! - **The chip decides** ([`effective`]): a job runs at the chat's Mode
//!   (the composer's chip, else Settings → Jobs' default). The model may ask
//!   for a lower one ("make a plan first": Plan), **never a higher one**: a
//!   web page can't talk a job into Bypass.
//! - **The person chooses, never the model on its own** ([`model_pick`],
//!   2026-11-07): a lower mode the model asks for takes effect only when
//!   the person's own words this turn asked for it (`consent::
//!   asks_lower_mode`: a plan, no changes yet, being asked first); else the
//!   chip's mode runs, and the tool's result says so ([`Pick::note`]).
//! - **A folder's cap** lowers it further (a folder that never runs
//!   unattended: capped at Manual).
//! - **Bypass permissions** only once the person has confirmed its warning
//!   (`job_bypass_confirmed`), whoever asks for it.
//!
//! Ranked by what runs without asking: Plan (changes nothing) < Manual
//! (asks before every change) < Accept edits (edits, asks for the rest) <
//! Auto (a classifier approves the safe steps, commands too) < Bypass
//! (everything). Pure.

use crate::ai::claude_code::session::PermissionMode;

/// One of Claude Code's modes, lowest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mode {
    Plan,
    Manual,
    AcceptEdits,
    Auto,
    Bypass,
}

impl Mode {
    /// Every mode, in the chip's order (Claude Code's menu: 1 to 5).
    pub const MENU: [Mode; 5] = [Mode::Auto, Mode::Manual, Mode::AcceptEdits, Mode::Plan, Mode::Bypass];

    /// As stored and sent (`--permission-mode`'s words).
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Plan => "plan",
            Mode::Manual => "manual",
            Mode::AcceptEdits => "acceptEdits",
            Mode::Auto => "auto",
            Mode::Bypass => "bypassPermissions",
        }
    }

    /// As the chip says it.
    pub fn label(self) -> &'static str {
        match self {
            Mode::Plan => "Plan",
            Mode::Manual => "Manual",
            Mode::AcceptEdits => "Accept edits",
            Mode::Auto => "Auto",
            Mode::Bypass => "Bypass permissions",
        }
    }

    /// What it does, in Claude Code's words.
    pub fn says(self) -> &'static str {
        match self {
            Mode::Auto => "Claude handles permission decisions",
            Mode::Manual => "Always ask before making changes",
            Mode::AcceptEdits => "Automatically accept all file edits",
            Mode::Plan => "Create a plan before making changes",
            Mode::Bypass => "Accepts all permissions",
        }
    }

    /// From a stored value or a model's argument: Claude Code's words, the
    /// chip's, and the plain ones a model may use ("ask", "edits",
    /// "bypass"). Case and spacing don't matter.
    pub fn parse(s: &str) -> Option<Mode> {
        let k: String = s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_ascii_lowercase();
        match k.as_str() {
            "plan" | "planmode" | "planfirst" => Some(Mode::Plan),
            "manual" | "ask" | "default" | "askfirst" => Some(Mode::Manual),
            "acceptedits" | "edits" | "editsonly" => Some(Mode::AcceptEdits),
            "auto" | "automatic" | "recommended" => Some(Mode::Auto),
            "bypasspermissions" | "bypass" => Some(Mode::Bypass),
            _ => None,
        }
    }

    /// Claude Code's flag value.
    pub fn permission(self) -> PermissionMode {
        match self {
            Mode::Plan => PermissionMode::Plan,
            Mode::Manual => PermissionMode::Manual,
            Mode::AcceptEdits => PermissionMode::AcceptEdits,
            Mode::Auto => PermissionMode::Auto,
            Mode::Bypass => PermissionMode::BypassPermissions,
        }
    }
}

/// A stored mode, Auto when it isn't one (an older or hand-edited file).
pub fn or_auto(s: &str) -> Mode {
    Mode::parse(s).unwrap_or(Mode::Auto)
}

/// Why a job can't run at the mode asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The model asked for more than the chat's Mode.
    Raised { asked: Mode, chip: Mode },
    /// Bypass, and the person never confirmed its warning.
    BypassUnconfirmed,
}

impl Refused {
    /// The tool's error, for the model.
    pub fn for_model(self) -> String {
        match self {
            Refused::Raised { asked, chip } => format!(
                "No job was started: {} is above this chat's Mode ({}). A job runs at the chat's Mode or lower, never higher. Start it again without a mode (or a lower one), or tell the user they can change the Mode beside the message box.",
                asked.label(),
                chip.label()
            ),
            Refused::BypassUnconfirmed => "No job was started: Bypass permissions needs the user's one-time OK in Yap first (the Mode menu beside the message box, or Settings → Jobs). Tell them so.".to_string(),
        }
    }

    /// The chip the person sees.
    pub fn chip(self) -> String {
        match self {
            Refused::Raised { asked, .. } => format!("Didn't start a job: {} is above this chat's Mode", asked.label()),
            Refused::BypassUnconfirmed => "Didn't start a job: Bypass permissions isn't confirmed".to_string(),
        }
    }
}

/// **The mode a job runs at**: the chat's Mode (`chip`), or a lower one the
/// model asked for (`asked`), lowered to the folder's `cap`; Bypass only
/// when `bypass_ok`. Pure.
pub fn effective(chip: Mode, asked: Option<Mode>, cap: Option<Mode>, bypass_ok: bool) -> Result<Mode, Refused> {
    let mut mode = chip;
    if let Some(asked) = asked {
        if asked > chip {
            return Err(Refused::Raised { asked, chip });
        }
        mode = asked;
    }
    if let Some(cap) = cap {
        mode = mode.min(cap);
    }
    if mode == Mode::Bypass && !bypass_ok {
        return Err(Refused::BypassUnconfirmed);
    }
    Ok(mode)
}

/// What the chat's model's `mode` comes to ([`model_pick`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pick {
    /// The mode asked, as [`effective`] takes it (`None`: the chip's).
    pub asked: Option<Mode>,
    /// A lower mode the model asked for on its own, left out.
    pub ignored: Option<Mode>,
}

impl Pick {
    /// What the tool's result tells the model when its mode was left out.
    pub fn note(&self, chip: Mode) -> Option<String> {
        let ignored = self.ignored?;
        let why = if ignored == Mode::Plan { "the user didn't ask for a plan" } else { "the user didn't ask for a lower one" };
        Some(format!("The job runs at the chat's Mode ({}): {why}.", chip.label()))
    }
}

/// **The model's `mode`, as the person's words allow it** (2026-11-07,
/// after the maintainer's "look into this and make our own version" ran as
/// a plan under a Bypass chip): a mode lower than the chat's (`chip`) takes
/// effect only when the person asked for less (`person_asked`:
/// `consent::asks_lower_mode`); otherwise it's left out and the chip's
/// mode runs. One above the chip stays asked, so [`effective`] still
/// refuses it. Pure.
pub fn model_pick(chip: Mode, asked: Option<Mode>, person_asked: bool) -> Pick {
    match asked {
        Some(m) if m < chip && !person_asked => Pick { asked: None, ignored: Some(m) },
        Some(m) if m == chip => Pick { asked: None, ignored: None },
        a => Pick { asked: a, ignored: None },
    }
}

/// The mode a job's next turn runs at: the chat's Mode now (`chip`, which
/// the person may have moved since), capped by the folder — except a plan
/// waiting for its go-ahead, which stays a plan until "go ahead". Bypass
/// unconfirmed falls back to Auto (the person's own chip can't be Bypass
/// unconfirmed; a hand-edited file can). Pure.
pub fn next_turn(job_mode: Mode, chip: Mode, cap: Option<Mode>, go_ahead: bool, bypass_ok: bool) -> Mode {
    let mode = if job_mode == Mode::Plan && !go_ahead { Mode::Plan } else { chip };
    let mode = cap.map_or(mode, |c| mode.min(c));
    if mode == Mode::Bypass && !bypass_ok {
        Mode::Auto.min(cap.unwrap_or(Mode::Auto))
    } else {
        mode
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_read_back_from_every_way_of_saying_them() {
        for m in Mode::MENU {
            assert_eq!(Mode::parse(m.as_str()), Some(m));
            assert_eq!(Mode::parse(m.label()), Some(m));
        }
        assert_eq!(Mode::parse("Accept Edits"), Some(Mode::AcceptEdits));
        assert_eq!(Mode::parse("bypass"), Some(Mode::Bypass));
        assert_eq!(Mode::parse("ask"), Some(Mode::Manual));
        assert_eq!(Mode::parse("yolo"), None);
        assert_eq!(or_auto("nonsense"), Mode::Auto);
        assert_eq!(Mode::Bypass.permission().as_str(), "bypassPermissions");
        assert_eq!(Mode::Manual.permission().as_str(), "manual");
    }

    #[test]
    fn the_model_may_lower_the_chats_mode_never_raise_it() {
        // Nothing asked: the chip's.
        assert_eq!(effective(Mode::Auto, None, None, false), Ok(Mode::Auto));
        // Lower: fine ("make a plan first").
        assert_eq!(effective(Mode::Auto, Some(Mode::Plan), None, false), Ok(Mode::Plan));
        assert_eq!(effective(Mode::Auto, Some(Mode::AcceptEdits), None, false), Ok(Mode::AcceptEdits));
        assert_eq!(effective(Mode::Auto, Some(Mode::Auto), None, false), Ok(Mode::Auto));
        // Higher: refused, however it's asked.
        assert_eq!(effective(Mode::Auto, Some(Mode::Bypass), None, true), Err(Refused::Raised { asked: Mode::Bypass, chip: Mode::Auto }));
        assert_eq!(effective(Mode::Manual, Some(Mode::Auto), None, true), Err(Refused::Raised { asked: Mode::Auto, chip: Mode::Manual }));
        assert_eq!(effective(Mode::Plan, Some(Mode::Manual), None, true), Err(Refused::Raised { asked: Mode::Manual, chip: Mode::Plan }));
        let r = effective(Mode::Auto, Some(Mode::Bypass), None, true).unwrap_err();
        assert!(r.for_model().contains("above this chat's Mode (Auto)"), "{}", r.for_model());
        assert_eq!(r.chip(), "Didn't start a job: Bypass permissions is above this chat's Mode");
    }

    #[test]
    fn a_folders_cap_lowers_it_and_bypass_needs_its_ok() {
        assert_eq!(effective(Mode::Auto, None, Some(Mode::Manual), false), Ok(Mode::Manual));
        assert_eq!(effective(Mode::Bypass, None, Some(Mode::AcceptEdits), false), Ok(Mode::AcceptEdits), "capped below Bypass: no OK needed");
        assert_eq!(effective(Mode::Plan, None, Some(Mode::Auto), false), Ok(Mode::Plan), "a cap never raises");
        // Bypass on the chip: only once confirmed.
        assert_eq!(effective(Mode::Bypass, None, None, false), Err(Refused::BypassUnconfirmed));
        assert_eq!(effective(Mode::Bypass, None, None, true), Ok(Mode::Bypass));
        assert_eq!(effective(Mode::Bypass, Some(Mode::Bypass), None, true), Ok(Mode::Bypass));
    }

    #[test]
    fn a_lower_mode_takes_effect_only_when_the_person_asked_for_it() {
        // A Bypass chip, and the model chose Plan on its own: left out, the chip's mode runs.
        let pick = model_pick(Mode::Bypass, Some(Mode::Plan), false);
        assert_eq!(pick, Pick { asked: None, ignored: Some(Mode::Plan) });
        assert_eq!(effective(Mode::Bypass, pick.asked, None, true), Ok(Mode::Bypass));
        assert_eq!(pick.note(Mode::Bypass).as_deref(), Some("The job runs at the chat's Mode (Bypass permissions): the user didn't ask for a plan."));
        assert_eq!(model_pick(Mode::Auto, Some(Mode::Manual), false).note(Mode::Auto).as_deref(), Some("The job runs at the chat's Mode (Auto): the user didn't ask for a lower one."));
        // The person asked ("make a plan first"): Plan.
        let pick = model_pick(Mode::Bypass, Some(Mode::Plan), true);
        assert_eq!(pick, Pick { asked: Some(Mode::Plan), ignored: None });
        assert_eq!(effective(Mode::Bypass, pick.asked, None, true), Ok(Mode::Plan));
        assert_eq!(pick.note(Mode::Bypass), None);
        // Nothing asked, or the chip's own: the chip's, nothing said.
        assert_eq!(model_pick(Mode::Auto, None, false), Pick { asked: None, ignored: None });
        assert_eq!(model_pick(Mode::Auto, Some(Mode::Auto), false), Pick { asked: None, ignored: None });
        // Above the chip: still asked, so still refused — whatever the person said.
        for said in [false, true] {
            let pick = model_pick(Mode::Manual, Some(Mode::Auto), said);
            assert_eq!(effective(Mode::Manual, pick.asked, None, true), Err(Refused::Raised { asked: Mode::Auto, chip: Mode::Manual }));
        }
        // A folder's cap still lowers the chip's mode.
        assert_eq!(effective(Mode::Bypass, model_pick(Mode::Bypass, Some(Mode::Plan), false).asked, Some(Mode::Manual), true), Ok(Mode::Manual));
    }

    #[test]
    fn the_next_turn_follows_the_chip_and_a_plan_waits_for_its_go_ahead() {
        // A plan stays a plan until "go ahead", then the chip's mode.
        assert_eq!(next_turn(Mode::Plan, Mode::Auto, None, false, false), Mode::Plan);
        assert_eq!(next_turn(Mode::Plan, Mode::Auto, None, true, false), Mode::Auto);
        assert_eq!(next_turn(Mode::Plan, Mode::Auto, Some(Mode::AcceptEdits), true, false), Mode::AcceptEdits);
        // Otherwise the chip, as it is now (the person moved it).
        assert_eq!(next_turn(Mode::Auto, Mode::Manual, None, false, false), Mode::Manual);
        assert_eq!(next_turn(Mode::AcceptEdits, Mode::Auto, None, false, false), Mode::Auto);
        // Bypass unconfirmed: Auto (under the cap).
        assert_eq!(next_turn(Mode::Auto, Mode::Bypass, None, false, false), Mode::Auto);
        assert_eq!(next_turn(Mode::Auto, Mode::Bypass, None, false, true), Mode::Bypass);
        assert_eq!(next_turn(Mode::Auto, Mode::Bypass, Some(Mode::Manual), false, false), Mode::Manual);
    }
}

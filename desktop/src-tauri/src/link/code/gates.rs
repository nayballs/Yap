//! **Remote Control's gates** (2026-11-08, design §8): what a paired phone
//! may do on the Code side, checked by the PC on every message — the page's
//! view is never trusted. Pure.
//!
//! **In order** ([`on`], then [`allows`]):
//! 1. pairing (the session is a paired phone's: `session.rs`);
//! 2. Remote Control on (Settings → Phone, the PC only), with Yap Code on
//!    and Claude Code on this PC;
//! 3. this phone ticked for it (off for phones paired before);
//! 4. the workspace allows it, and its cap holds;
//! 5. the action is one a phone may take ([`Action`]): never Bypass
//!    permissions, never above Auto, never above the workspace's cap.
//!
//! **A fresh tap** ([`Shown`]): an approval (or an ask-first card) is
//! answered only on a card the PC sent *that* phone, still waiting, at
//! least [`SINCE_SENT`] after it was sent and [`MIN_SHOWN_MS`] after the
//! page showed it (the phone says how long), and once.
//!
//! **Rate limits** per phone ([`Limits`]): 30 Code actions a minute, one
//! Undo every 10 s, 4 pages (`agent_log`, `agent_diff`) a second.
//!
//! What a phone can never do has no message at all: Bypass's one-time OK,
//! adding, removing or capping a workspace, Settings → Yap Code, pairing,
//! turning Remote Control on, and any answer by voice.

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use crate::agents::modes::Mode;

/// The page's buttons wake this long after a card is shown.
pub const MIN_SHOWN_MS: u64 = 400;
/// A tap this soon after the PC sent the card is refused (the page's
/// 400 ms and the trip).
pub const SINCE_SENT: Duration = Duration::from_millis(700);
/// The highest Mode a phone's turn runs at, or a phone may pick.
pub use crate::agents::remote::PHONE_MAX;
/// Code actions a phone may take a minute.
pub const ACTIONS_A_MINUTE: usize = 30;
/// One Undo this often.
pub const UNDO_EVERY: Duration = Duration::from_secs(10);
/// Pages (`agent_log`, `agent_diff`) a second.
pub const PAGES_A_SECOND: usize = 4;

/// What the gates read, as they are now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Gate {
    /// Settings → Phone → Remote Control.
    pub master: bool,
    /// Settings → Assistant → Yap Code.
    pub yap_code: bool,
    /// Claude Code is on this PC.
    pub claude: bool,
    /// This phone is ticked for Remote Control.
    pub ticked: bool,
}

/// A workspace, as the gates read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    pub name: String,
    /// Its Remote Control switch (the Environment card).
    pub allowed: bool,
    /// "Never above:".
    pub cap: Option<Mode>,
}

/// What a phone asks to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// See the Code side, a Code chat, an agent.
    Read,
    /// Stop, Continue, "Tell the agent…", Go ahead, Undo, a new Code chat.
    Act,
    /// Allow once / Deny, Go ahead / Not yet, Start / Don't.
    Answer,
    /// A chat's Mode.
    SetMode(Mode),
}

/// **Why not**, in words the phone shows ([`Refusal::words`]) and a kind
/// for the log ([`Refusal::kind`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    Off,
    YapCodeOff,
    NoClaude,
    NotTicked,
    /// The workspace's Remote Control is off.
    Workspace(String),
    /// Bypass permissions, or above Auto: never from a phone.
    NeverBypass,
    /// Above the workspace's cap.
    AboveCap { workspace: String, cap: Mode },
    /// A card this phone wasn't sent (or one that isn't waiting).
    NotShown,
    /// A tap too soon after the card came.
    TooSoon,
    /// Over a rate limit.
    Slow,
    /// Not one of the agents, chats or workspaces it may see.
    NotFound,
}

impl Refusal {
    /// In plain words, on the phone (`pc`: this PC's name).
    pub fn words(&self, pc: &str) -> String {
        let feature = crate::agents::FEATURE;
        match self {
            Refusal::Off => format!("Remote Control is off on {pc}. Turn it on in Settings → Phone."),
            Refusal::YapCodeOff => format!("{feature} is off on {pc}. Turn it on in Settings → Assistant → {feature}."),
            Refusal::NoClaude => format!("Claude Code isn't on {pc}, so there are no agents to follow."),
            Refusal::NotTicked => format!("This phone isn't allowed Remote Control. Tick it in Settings → Phone on {pc}."),
            Refusal::Workspace(w) => format!("Remote Control is off for {w}. Turn it on in its Environment card on {pc}."),
            Refusal::NeverBypass => "From your phone, agents run at most at Auto: Bypass permissions is for the PC only.".into(),
            Refusal::AboveCap { workspace, cap } => format!("Never above {} in {workspace}.", cap.label()),
            Refusal::NotShown => "That card isn't waiting on this phone any more.".into(),
            Refusal::TooSoon => "Tap it again: the card had only just come.".into(),
            Refusal::Slow => "Slow down a little: Yap on your PC is still doing the last one.".into(),
            Refusal::NotFound => format!("That isn't on {pc} any more, or Remote Control doesn't show it."),
        }
    }

    /// For the log: never an id, a name or words.
    pub fn kind(&self) -> &'static str {
        match self {
            Refusal::Off => "off",
            Refusal::YapCodeOff => "yap code off",
            Refusal::NoClaude => "no claude code",
            Refusal::NotTicked => "not ticked",
            Refusal::Workspace(_) => "workspace off",
            Refusal::NeverBypass => "bypass",
            Refusal::AboveCap { .. } => "above cap",
            Refusal::NotShown => "not shown",
            Refusal::TooSoon => "too soon",
            Refusal::Slow => "slow down",
            Refusal::NotFound => "not found",
        }
    }
}

/// **Gates 2 and 3**: Remote Control is on for this phone (`ready.code.on`),
/// or why not — in this order: the switch, Yap Code, Claude Code, the
/// phone's tick. Pure.
pub fn on(g: &Gate) -> Result<(), Refusal> {
    if !g.master {
        return Err(Refusal::Off);
    }
    if !g.yap_code {
        return Err(Refusal::YapCodeOff);
    }
    if !g.claude {
        return Err(Refusal::NoClaude);
    }
    if !g.ticked {
        return Err(Refusal::NotTicked);
    }
    Ok(())
}

/// **Every gate, in order**, for `action` in workspace `ws` (`None`: one
/// that isn't about a workspace, the Code side's list). Pure.
pub fn allows(g: &Gate, ws: Option<&Workspace>, action: Action) -> Result<(), Refusal> {
    on(g)?;
    if let Some(w) = ws {
        if !w.allowed {
            return Err(Refusal::Workspace(w.name.clone()));
        }
    }
    if let Action::SetMode(m) = action {
        if m > PHONE_MAX {
            return Err(Refusal::NeverBypass);
        }
        if let Some(cap) = ws.and_then(|w| w.cap.filter(|c| m > *c)) {
            return Err(Refusal::AboveCap { workspace: ws.map(|w| w.name.clone()).unwrap_or_default(), cap });
        }
    }
    Ok(())
}

/// The kinds of card a phone answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Card {
    /// A permission prompt or a plan (`claude_live/approve.rs`).
    Approve,
    /// An ask-first card (`chat/pages/act/confirm.rs`, act "agent").
    Confirm,
}

/// **Which phone was sent which card, and when** (like `Given` for ▶).
#[derive(Debug, Default)]
pub struct Shown(HashMap<(Card, u64), HashMap<String, Instant>>);

impl Shown {
    /// Card `id` went to phone `device` (the first time counts).
    pub fn sent(&mut self, card: Card, id: u64, device: &str, now: Instant) {
        self.0.entry((card, id)).or_default().entry(device.to_string()).or_insert(now);
    }

    /// Was card `id` sent to phone `device`?
    #[cfg(test)]
    pub fn was_sent(&self, card: Card, id: u64, device: &str) -> bool {
        self.0.get(&(card, id)).is_some_and(|m| m.contains_key(device))
    }

    /// **A tap on card `id` from phone `device`**, `shown_ms` after the page
    /// showed it: sent to that phone, not too soon. Accepted once: the
    /// phone's entry goes (a second tap: [`Refusal::NotShown`]).
    pub fn tap(&mut self, card: Card, id: u64, device: &str, now: Instant, shown_ms: u64) -> Result<(), Refusal> {
        let Some(sent) = self.0.get(&(card, id)).and_then(|m| m.get(device)).copied() else {
            return Err(Refusal::NotShown);
        };
        if now.saturating_duration_since(sent) < SINCE_SENT || shown_ms < MIN_SHOWN_MS {
            return Err(Refusal::TooSoon);
        }
        if let Some(m) = self.0.get_mut(&(card, id)) {
            m.remove(device);
        }
        Ok(())
    }

    /// Card `id` is over (answered anywhere, timed out): forgotten.
    pub fn close(&mut self, card: Card, id: u64) {
        self.0.remove(&(card, id));
    }

    /// A phone gone: its entries go.
    pub fn forget(&mut self, device: &str) {
        for m in self.0.values_mut() {
            m.remove(device);
        }
        self.0.retain(|_, m| !m.is_empty());
    }
}

/// **One phone's rate limits.**
#[derive(Debug, Default)]
pub struct Limits {
    actions: VecDeque<Instant>,
    undo: Option<Instant>,
    pages: VecDeque<Instant>,
}

fn within(list: &mut VecDeque<Instant>, now: Instant, span: Duration, most: usize) -> Result<(), Refusal> {
    while list.front().is_some_and(|t| now.saturating_duration_since(*t) >= span) {
        list.pop_front();
    }
    if list.len() >= most {
        return Err(Refusal::Slow);
    }
    list.push_back(now);
    Ok(())
}

impl Limits {
    /// A Code action (send, stop, continue, mode, go ahead, a message).
    pub fn act(&mut self, now: Instant) -> Result<(), Refusal> {
        within(&mut self.actions, now, Duration::from_secs(60), ACTIONS_A_MINUTE)
    }

    /// An Undo: one every [`UNDO_EVERY`], and an action too.
    pub fn undo(&mut self, now: Instant) -> Result<(), Refusal> {
        if self.undo.is_some_and(|t| now.saturating_duration_since(t) < UNDO_EVERY) {
            return Err(Refusal::Slow);
        }
        self.act(now)?;
        self.undo = Some(now);
        Ok(())
    }

    /// A page of an agent's log or a file's diff.
    pub fn page(&mut self, now: Instant) -> Result<(), Refusal> {
        within(&mut self.pages, now, Duration::from_secs(1), PAGES_A_SECOND)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: Gate = Gate { master: true, yap_code: true, claude: true, ticked: true };

    fn ws(allowed: bool, cap: Option<Mode>) -> Workspace {
        Workspace { name: "garden-planner".into(), allowed, cap }
    }

    #[test]
    fn the_gates_go_in_order_and_say_the_first_that_fails() {
        assert_eq!(on(&ALL), Ok(()));
        // Everything off: the switch is said first, then Yap Code, Claude Code, the tick.
        assert_eq!(on(&Gate::default()), Err(Refusal::Off));
        assert_eq!(on(&Gate { master: true, ..Gate::default() }), Err(Refusal::YapCodeOff));
        assert_eq!(on(&Gate { master: true, yap_code: true, ..Gate::default() }), Err(Refusal::NoClaude));
        assert_eq!(on(&Gate { ticked: false, ..ALL }), Err(Refusal::NotTicked));
        // A phone ticked but the switch off: still the switch.
        assert_eq!(on(&Gate { master: false, ..ALL }), Err(Refusal::Off));
        // Then the workspace, then the action.
        assert_eq!(allows(&ALL, Some(&ws(false, None)), Action::Read), Err(Refusal::Workspace("garden-planner".into())));
        assert_eq!(allows(&Gate { ticked: false, ..ALL }, Some(&ws(false, None)), Action::Read), Err(Refusal::NotTicked), "the phone before the workspace");
        assert_eq!(allows(&ALL, Some(&ws(false, None)), Action::SetMode(Mode::Bypass)), Err(Refusal::Workspace("garden-planner".into())), "the workspace before the action");
        for a in [Action::Read, Action::Act, Action::Answer, Action::SetMode(Mode::Auto)] {
            assert_eq!(allows(&ALL, Some(&ws(true, None)), a), Ok(()), "{a:?}");
            assert_eq!(allows(&ALL, None, a), Ok(()), "{a:?}");
        }
        // Words and kinds.
        assert_eq!(Refusal::Off.words("Office PC"), "Remote Control is off on Office PC. Turn it on in Settings → Phone.");
        assert_eq!(Refusal::NotTicked.kind(), "not ticked");
        assert!(Refusal::Slow.words("x").starts_with("Slow down a little"));
    }

    #[test]
    fn a_phone_never_picks_bypass_nor_goes_above_auto_or_the_cap() {
        assert_eq!(allows(&ALL, Some(&ws(true, None)), Action::SetMode(Mode::Bypass)), Err(Refusal::NeverBypass));
        for m in [Mode::Plan, Mode::Manual, Mode::AcceptEdits, Mode::Auto] {
            assert_eq!(allows(&ALL, Some(&ws(true, None)), Action::SetMode(m)), Ok(()), "{m:?}");
        }
        // "Never above: Manual" holds from the phone too.
        let capped = ws(true, Some(Mode::Manual));
        assert_eq!(allows(&ALL, Some(&capped), Action::SetMode(Mode::Auto)), Err(Refusal::AboveCap { workspace: "garden-planner".into(), cap: Mode::Manual }));
        assert_eq!(allows(&ALL, Some(&capped), Action::SetMode(Mode::Manual)), Ok(()));
        assert_eq!(allows(&ALL, Some(&capped), Action::SetMode(Mode::Plan)), Ok(()));
        assert_eq!(Refusal::AboveCap { workspace: "garden-planner".into(), cap: Mode::Manual }.words("pc"), "Never above Manual in garden-planner.");
        // The clamp: a Bypass chat runs at Auto from the phone; lower stays lower.
        assert_eq!(crate::agents::remote::phone_mode(Mode::Bypass), Mode::Auto);
        assert_eq!(crate::agents::remote::phone_mode(Mode::Auto), Mode::Auto);
        assert_eq!(crate::agents::remote::phone_mode(Mode::Plan), Mode::Plan);
    }

    #[test]
    fn an_approval_needs_a_fresh_tap_on_a_card_sent_to_that_phone_once() {
        let t0 = Instant::now();
        let mut shown = Shown::default();
        // Not sent to this phone: refused.
        assert_eq!(shown.tap(Card::Approve, 7, "p-office", t0 + Duration::from_secs(5), 2_000), Err(Refusal::NotShown));
        shown.sent(Card::Approve, 7, "p-office", t0);
        shown.sent(Card::Approve, 7, "p-office", t0 + Duration::from_secs(3)); // sent again: the first counts
        assert!(shown.was_sent(Card::Approve, 7, "p-office"));
        assert!(!shown.was_sent(Card::Confirm, 7, "p-office"), "a confirm card is another card");
        // Another phone: never sent it.
        assert_eq!(shown.tap(Card::Approve, 7, "p-kitchen", t0 + Duration::from_secs(5), 2_000), Err(Refusal::NotShown));
        // Too soon after it was sent, or shown under 400 ms on the phone.
        assert_eq!(shown.tap(Card::Approve, 7, "p-office", t0 + Duration::from_millis(500), 2_000), Err(Refusal::TooSoon));
        assert_eq!(shown.tap(Card::Approve, 7, "p-office", t0 + Duration::from_secs(5), 399), Err(Refusal::TooSoon));
        assert_eq!(shown.tap(Card::Approve, 7, "p-office", t0 + Duration::from_secs(5), 0), Err(Refusal::TooSoon));
        // A fresh tap: once.
        assert_eq!(shown.tap(Card::Approve, 7, "p-office", t0 + SINCE_SENT, MIN_SHOWN_MS), Ok(()));
        assert_eq!(shown.tap(Card::Approve, 7, "p-office", t0 + Duration::from_secs(6), 2_000), Err(Refusal::NotShown), "twice");
        // Closed (answered on the PC): nobody's tap counts.
        shown.sent(Card::Approve, 8, "p-office", t0);
        shown.close(Card::Approve, 8);
        assert_eq!(shown.tap(Card::Approve, 8, "p-office", t0 + Duration::from_secs(5), 2_000), Err(Refusal::NotShown));
        // A phone gone.
        shown.sent(Card::Confirm, 9, "p-office", t0);
        shown.forget("p-office");
        assert!(!shown.was_sent(Card::Confirm, 9, "p-office"));
    }

    #[test]
    fn each_phone_has_its_rate_limits() {
        let t0 = Instant::now();
        let mut l = Limits::default();
        for i in 0..ACTIONS_A_MINUTE {
            assert_eq!(l.act(t0 + Duration::from_millis(i as u64)), Ok(()));
        }
        assert_eq!(l.act(t0 + Duration::from_secs(1)), Err(Refusal::Slow), "31 in a minute");
        assert_eq!(l.act(t0 + Duration::from_secs(61)), Ok(()), "a minute on");
        // Undo: one every 10 s.
        let mut l = Limits::default();
        assert_eq!(l.undo(t0), Ok(()));
        assert_eq!(l.undo(t0 + Duration::from_secs(9)), Err(Refusal::Slow));
        assert_eq!(l.undo(t0 + UNDO_EVERY), Ok(()));
        // Pages: 4 a second.
        let mut l = Limits::default();
        for i in 0..PAGES_A_SECOND {
            assert_eq!(l.page(t0 + Duration::from_millis(i as u64 * 10)), Ok(()));
        }
        assert_eq!(l.page(t0 + Duration::from_millis(500)), Err(Refusal::Slow));
        assert_eq!(l.page(t0 + Duration::from_millis(1_001)), Ok(()));
    }
}

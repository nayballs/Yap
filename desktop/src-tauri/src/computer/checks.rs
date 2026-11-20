//! **The checks before every input action** (phase 2), a pure decision
//! table over what's true now ([`Facts`]) — the same order every time,
//! the first that applies decides:
//!
//! 1. (Before this: a meeting, a call or a dictation, `computer::refused_now`;
//!    another chat in control, a stop this turn: `super::watch`.)
//! 2. **Where it lands**: a pointer action, the window under the point (and
//!    a drag's end); keys and typing, the window in front.
//!    - Yap's own → refused ("that's Yap's window"; keys: "click the app
//!      first": Yap's own window in front counts as nothing in front).
//!    - not one of the allowed apps' (or nothing there) → refused.
//!    - running as administrator → refused: Windows drops the input (UIPI),
//!      so the person does it.
//! 3. **The tier**: a browser takes nothing but a look (web pages go to
//!    Yap's browser beside the chat); a terminal or an IDE takes a plain
//!    left click and a scroll, never typing, keys, right or middle clicks,
//!    modifier clicks or drags.
//! 4. **A system combo** (the Windows key, Alt+F4, Alt+Tab…) without the
//!    `systemKeyCombos` grant → refused.
//! 5. **A link** under a click → refused: its address goes to the browser
//!    tools (the desktop contract's rule, enforced).
//! 6. **Typing** into a password field, or text with card or bank details
//!    (`chat/pages/act/rules.rs` `typed_secret`) → refused: the person types it.
//! 7. **A click that buys, pays, sends, deletes or signs in** (the control's
//!    name, `act::rules::needs_ok`), or Enter in a chat app → **asks first**
//!    on the browser's card — unless nothing may ask (a Code chat in Bypass
//!    permissions), where Claude is told to ask in the chat instead.
//!
//! Each refusal is words for the model and a chip for the thread.

use super::grants::Tier;
use super::input::Button;
use super::uia::{Element, Kind};

/// **What an action is**, as the checks see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    Click { button: Button, mods: bool },
    Move,
    Drag { mods: bool },
    /// `left_mouse_down` / `left_mouse_up`.
    Press,
    Scroll { mods: bool },
    Type,
    Key { system: bool, enter: bool },
    Hold { system: bool },
}

impl Act {
    fn pointer(self) -> bool {
        matches!(self, Act::Click { .. } | Act::Drag { .. } | Act::Press | Act::Scroll { .. })
    }
    fn keys(self) -> bool {
        matches!(self, Act::Type | Act::Key { .. } | Act::Hold { .. })
    }
}

/// **A window**, as the checks see it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Target {
    pub yap: bool,
    /// The allowed app it belongs to, and its tier (`None`: not allowed).
    pub app: Option<(String, Tier)>,
    pub elevated: bool,
    /// A chat app (`pasting/app_kind.rs`): Enter sends.
    pub messaging: bool,
}

/// **What's true now** for one action.
#[derive(Debug, Clone, Default)]
pub struct Facts<'a> {
    /// The window in front.
    pub front: Option<Target>,
    /// A pointer action's window under the point…
    pub under: Option<Target>,
    /// …and a drag's end's.
    pub end: Option<Target>,
    /// The element under a click (UI Automation).
    pub element: Option<Element>,
    /// The element with the focus, for typing.
    pub focus: Option<Element>,
    /// The `systemKeyCombos` grant.
    pub system_keys: bool,
    /// The words typed.
    pub text: &'a str,
    /// Nothing may ask (a Code chat in Bypass permissions).
    pub unattended: bool,
}

/// **What happens.**
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Go,
    Refuse { error: String, chip: String },
    /// The browser's ask-first card: what Yap would do (`press`, `enter`),
    /// the control's words, the app, and what it would do ("send", "buy"…).
    Ask { act: &'static str, label: String, app: String, does: &'static str },
}

fn refuse(error: impl Into<String>, chip: impl Into<String>) -> Verdict {
    Verdict::Refuse { error: error.into(), chip: chip.into() }
}

/// Where an action lands: refused, or the allowed app and its tier.
fn landing(t: Option<&Target>, keys: bool) -> Result<(String, Tier), Verdict> {
    let Some(t) = t else {
        return Err(refuse("There's no window there: take a screenshot and try again.", "Didn't act: no window there"));
    };
    if t.yap {
        return Err(if keys {
            refuse("Yap's own window is in front, so keys and typing would go to it: click the app's window first (or open it), then type.", "Didn't type: Yap's own window is in front")
        } else {
            refuse("That spot is Yap's own window (masked in the screenshot): Yap never clicks itself. Click a part of the app that shows.", "Didn't click: that's Yap's window")
        });
    }
    let Some((app, tier)) = t.app.clone() else {
        return Err(if keys {
            // (Acting by element or an answered card leaves the window in front as it was: a real run, 20 Nov 2026.)
            refuse(
                "The window in front belongs to an app the user hasn't allowed, so keys and typing would go to it: bring the app you're working in to the front first (open_application, or click into its window), then press the keys.",
                "Didn't type: another app is in front",
            )
        } else {
            refuse(
                "That window belongs to an app the user hasn't allowed (masked in the screenshot). Ask for it with request_access if you need it.",
                "Didn't act: an app you haven't allowed",
            )
        });
    };
    if t.elevated {
        return Err(refuse(
            format!("{app} is running as administrator, so Windows won't take Yap's clicks or keys there. Ask the user to do this step themselves."),
            format!("Didn't act: {app} runs as administrator"),
        ));
    }
    Ok((app, tier))
}

/// **The verdict for `act`.** Pure.
pub fn decide(act: Act, f: &Facts<'_>) -> Verdict {
    if act == Act::Move {
        return Verdict::Go;
    }
    let keys = act.keys();
    let target = if act.pointer() { f.under.as_ref() } else { f.front.as_ref() };
    let (app, tier) = match landing(target, keys) {
        Ok(a) => a,
        Err(v) => return v,
    };
    if let Act::Drag { .. } = act {
        if let Err(v) = landing(f.end.as_ref(), false) {
            return v;
        }
    }
    // The tier.
    match tier {
        Tier::Read => {
            return refuse(
                format!("{app} is a browser: read only. Look at it in screenshots; to open or work a web page, use Yap's own browser tools beside the chat."),
                format!("Didn't act: {app} is read only"),
            );
        }
        Tier::Click => {
            let plain = matches!(act, Act::Click { button: Button::Left, mods: false } | Act::Scroll { mods: false });
            if !plain {
                return refuse(
                    format!("{app} is a terminal or an IDE, so it takes clicks only: Yap left-clicks and scrolls there, never typing, keys, right or middle clicks, modifier clicks or drags. Say what to type and the user does it."),
                    format!("Didn't act: {app} takes clicks only"),
                );
            }
        }
        Tier::Full => {}
    }
    match act {
        Act::Key { system: true, .. } | Act::Hold { system: true } if !f.system_keys => {
            return refuse(
                "That's a system shortcut (switching or closing apps, the Windows key, locking the screen): ask for it with request_access (systemKeyCombos) first.",
                "Didn't press it: system shortcuts aren't allowed",
            );
        }
        Act::Click { .. } if f.element.as_ref().is_some_and(|e| e.kind == Kind::Link) => {
            return refuse(
                "That's a link: Yap never clicks links in apps. Read its full address (zoom, or hover) and open it with Yap's browser tools beside the chat instead.",
                "Didn't click: a link",
            );
        }
        Act::Type => {
            if f.focus.as_ref().is_some_and(|e| e.password) {
                return refuse("The focus is in a password field: Yap never types passwords. Ask the user to type it themselves.", "Didn't type: a password field");
            }
            if let Some(what) = crate::chat::pages::act::rules::typed_secret(f.text, &Default::default()) {
                return refuse(format!("That text looks like {what}: Yap never types those. Ask the user to type it themselves."), format!("Didn't type: {what}"));
            }
        }
        _ => {}
    }
    // What a click or Enter would do.
    if !f.unattended {
        if let Act::Click { button: Button::Left, .. } = act {
            if let Some(e) = f.element.as_ref().filter(|e| e.kind == Kind::Control && !e.name.trim().is_empty()) {
                if let Some(does) = crate::chat::pages::act::rules::needs_ok(&e.name, "") {
                    return Verdict::Ask { act: "press", label: e.name.clone(), app, does };
                }
            }
        }
        if let Act::Key { enter: true, .. } = act {
            if f.front.as_ref().is_some_and(|t| t.messaging) {
                return Verdict::Ask { act: "enter", label: "Enter".into(), app, does: "send" };
            }
        }
    }
    Verdict::Go
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed(name: &str, tier: Tier) -> Option<Target> {
        Some(Target { app: Some((name.into(), tier)), ..Default::default() })
    }

    fn left() -> Act {
        Act::Click { button: Button::Left, mods: false }
    }

    fn error(v: Verdict) -> String {
        match v {
            Verdict::Refuse { error, .. } => error,
            other => panic!("not refused: {other:?}"),
        }
    }

    #[test]
    fn an_action_lands_only_on_an_allowed_app_never_on_yap_or_one_running_as_administrator() {
        let f = |under: Option<Target>| Facts { under, ..Default::default() };
        assert_eq!(decide(left(), &f(allowed("Notepad", Tier::Full))), Verdict::Go);
        assert!(error(decide(left(), &f(Some(Target { yap: true, ..Default::default() })))).contains("Yap's own window"));
        assert!(error(decide(left(), &f(Some(Target::default())))).contains("hasn't allowed"));
        assert!(error(decide(left(), &f(None))).contains("no window"));
        assert!(error(decide(left(), &f(Some(Target { elevated: true, ..allowed("Setup", Tier::Full).unwrap() })))).contains("administrator"));
        // Keys go to the window in front: Yap's own there is "click the app first".
        let front_yap = Facts { front: Some(Target { yap: true, ..Default::default() }), ..Default::default() };
        assert!(error(decide(Act::Type, &front_yap)).contains("click the app's window first"));
        // Another app in front for keys: bring the app back, not "ask for it" (a real run, 20 Nov 2026).
        let front_other = Facts { front: Some(Target::default()), ..Default::default() };
        assert!(error(decide(Act::Type, &front_other)).contains("bring the app you're working in to the front first"));
        assert!(error(decide(left(), &Facts { under: Some(Target::default()), front: Some(Target::default()), ..Default::default() })).contains("request_access"));
        // A drag's end must be allowed too.
        let drag = Facts { under: allowed("Explorer", Tier::Full), end: Some(Target::default()), ..Default::default() };
        assert!(error(decide(Act::Drag { mods: false }, &drag)).contains("hasn't allowed"));
        // Moving the pointer lands nowhere.
        assert_eq!(decide(Act::Move, &Facts::default()), Verdict::Go);
    }

    #[test]
    fn browsers_are_read_only_and_terminals_take_plain_clicks_and_scrolls() {
        let at = |t: Tier| Facts { under: allowed("App", t), front: allowed("App", t), ..Default::default() };
        assert!(error(decide(left(), &at(Tier::Read))).contains("read only"));
        assert!(error(decide(Act::Scroll { mods: false }, &at(Tier::Read))).contains("read only"));
        assert_eq!(decide(left(), &at(Tier::Click)), Verdict::Go);
        assert_eq!(decide(Act::Scroll { mods: false }, &at(Tier::Click)), Verdict::Go);
        for act in [Act::Type, Act::Key { system: false, enter: false }, Act::Click { button: Button::Right, mods: false }, Act::Click { button: Button::Middle, mods: false }, Act::Click { button: Button::Left, mods: true }, Act::Drag { mods: false }, Act::Press] {
            let mut f = at(Tier::Click);
            f.end = allowed("App", Tier::Click);
            assert!(error(decide(act, &f)).contains("clicks only"), "{act:?}");
        }
        assert_eq!(decide(Act::Type, &at(Tier::Full)), Verdict::Go);
    }

    #[test]
    fn system_shortcuts_need_their_grant() {
        let f = Facts { front: allowed("Notepad", Tier::Full), ..Default::default() };
        assert!(error(decide(Act::Key { system: true, enter: false }, &f)).contains("systemKeyCombos"));
        assert_eq!(decide(Act::Key { system: true, enter: false }, &Facts { system_keys: true, ..f.clone() }), Verdict::Go);
        assert_eq!(decide(Act::Key { system: false, enter: false }, &f), Verdict::Go);
    }

    #[test]
    fn links_passwords_and_card_numbers_are_refused() {
        let link = Facts { under: allowed("Mail", Tier::Full), element: Some(Element { kind: Kind::Link, name: "Track parcel".into(), password: false }), ..Default::default() };
        assert!(error(decide(left(), &link)).contains("never clicks links"));
        let pw = Facts { front: allowed("App", Tier::Full), focus: Some(Element { kind: Kind::Field, name: "Password".into(), password: true }), text: "hunter2", ..Default::default() };
        assert!(error(decide(Act::Type, &pw)).contains("password field"));
        let card = Facts { front: allowed("App", Tier::Full), text: "4111 1111 1111 1111", ..Default::default() };
        assert!(error(decide(Act::Type, &card)).contains("payment card"));
    }

    /// Send, Pay, Delete… ask first on the card; in Bypass nothing asks.
    #[test]
    fn a_click_that_sends_or_pays_asks_first_unless_nothing_may_ask() {
        let send = Facts { under: allowed("Mail", Tier::Full), element: Some(Element { kind: Kind::Control, name: "Send".into(), password: false }), ..Default::default() };
        assert_eq!(decide(left(), &send), Verdict::Ask { act: "press", label: "Send".into(), app: "Mail".into(), does: "send" });
        assert_eq!(decide(left(), &Facts { unattended: true, ..send.clone() }), Verdict::Go);
        let plain = Facts { element: Some(Element { kind: Kind::Control, name: "Bold".into(), password: false }), ..send.clone() };
        assert_eq!(decide(left(), &plain), Verdict::Go);
        let chat = Facts { front: Some(Target { messaging: true, ..allowed("Chat", Tier::Full).unwrap() }), ..Default::default() };
        assert!(matches!(decide(Act::Key { system: false, enter: true }, &chat), Verdict::Ask { does: "send", .. }));
        assert_eq!(decide(Act::Key { system: false, enter: true }, &Facts { front: allowed("Notepad", Tier::Full), ..Default::default() }), Verdict::Go);
    }
}

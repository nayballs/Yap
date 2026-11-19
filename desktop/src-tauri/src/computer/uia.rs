//! **What's under a point, and what has the focus** (UI Automation, time
//! boxed: `pasting/selection.rs`' client), for the checks before an action
//! ([`super::checks`]):
//! - a **link** under a click: never clicked (the desktop contract's rule,
//!   enforced: its address goes to Yap's browser instead);
//! - a **button's or a menu item's name**: what the browser's ask-first
//!   rule reads ("Send", "Pay now": `chat/pages/act/rules.rs`);
//! - a **password field** with the focus: never typed into.
//!
//! An element's name stays in memory for the check: never logged.

/// What kind of control an element is, as far as the checks care.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Link,
    /// A button, a menu item, a check box…: something a click does.
    Control,
    /// A text field.
    Field,
    Other,
}

/// **An element** under a point or with the focus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub kind: Kind,
    /// Its name ("Send", "Delete draft"); "" when it has none.
    pub name: String,
    pub password: bool,
}

/// How long a look may take: a hung app never holds an action up.
#[cfg(windows)]
const TIMEOUT_MS: u32 = 400;

/// **The element under a physical point** (the nearest link, button or
/// field among it and its first parents). `None`: none, or UI Automation
/// didn't answer in time. Blocking.
#[cfg(windows)]
pub fn at(x: i32, y: i32) -> Option<Element> {
    win::at(x, y)
}

#[cfg(not(windows))]
pub fn at(_x: i32, _y: i32) -> Option<Element> {
    None
}

/// **The element with the keyboard focus.** Blocking.
#[cfg(windows)]
pub fn focused() -> Option<Element> {
    win::focused()
}

#[cfg(not(windows))]
pub fn focused() -> Option<Element> {
    None
}

/// A control type's kind (UI Automation's ids). Pure.
pub fn kind_of(control_type: i32) -> Kind {
    match control_type {
        // Hyperlink
        50005 => Kind::Link,
        // Button, CheckBox, MenuItem, RadioButton, SplitButton, TabItem, ListItem
        50000 | 50002 | 50011 | 50013 | 50031 | 50019 | 50007 => Kind::Control,
        // Edit, Document
        50004 | 50030 => Kind::Field,
        _ => Kind::Other,
    }
}

#[cfg(windows)]
mod win {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::Accessibility::{IUIAutomation, IUIAutomationElement};

    use super::{kind_of, Element, Kind, TIMEOUT_MS};
    use crate::pasting::selection::uia::{automation, Com};

    fn element_of(e: &IUIAutomationElement) -> Element {
        let control = unsafe { e.CurrentControlType() }.map(|c| c.0).unwrap_or(0);
        let name = unsafe { e.CurrentName() }.map(|b| b.to_string()).unwrap_or_default();
        let password = unsafe { e.CurrentIsPassword() }.map(|b| b.as_bool()).unwrap_or(false);
        Element { kind: kind_of(control), name, password }
    }

    /// The element and its first parents, nearest first.
    fn chain(a: &IUIAutomation, e: IUIAutomationElement, depth: usize) -> Vec<Element> {
        let mut out = vec![element_of(&e)];
        let Ok(walker) = (unsafe { a.ControlViewWalker() }) else { return out };
        let mut cur = e;
        for _ in 0..depth {
            match unsafe { walker.GetParentElement(&cur) } {
                Ok(p) => {
                    out.push(element_of(&p));
                    cur = p;
                }
                Err(_) => break,
            }
        }
        out
    }

    /// The nearest link, else control, else field: what the checks read.
    fn nearest(chain: Vec<Element>) -> Option<Element> {
        for want in [Kind::Link, Kind::Control, Kind::Field] {
            if let Some(e) = chain.iter().take(3).find(|e| e.kind == want) {
                return Some(e.clone());
            }
        }
        chain.into_iter().next()
    }

    pub fn at(x: i32, y: i32) -> Option<Element> {
        let _com = Com::init()?;
        let a = automation(TIMEOUT_MS)?;
        let e = unsafe { a.ElementFromPoint(POINT { x, y }) }.ok()?;
        nearest(chain(&a, e, 3))
    }

    pub fn focused() -> Option<Element> {
        let _com = Com::init()?;
        let a = automation(TIMEOUT_MS)?;
        let e = unsafe { a.GetFocusedElement() }.ok()?;
        Some(element_of(&e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_types_are_links_controls_and_fields() {
        assert_eq!(kind_of(50005), Kind::Link);
        assert_eq!(kind_of(50000), Kind::Control);
        assert_eq!(kind_of(50011), Kind::Control);
        assert_eq!(kind_of(50004), Kind::Field);
        assert_eq!(kind_of(50020), Kind::Other, "plain text");
    }
}

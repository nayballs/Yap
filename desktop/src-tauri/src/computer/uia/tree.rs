//! **A window's elements** (phase 5, `elements.rs`): UI Automation's
//! control view of one window, depth first, each element with its place in
//! the tree (its children's indexes from the window down), and an element
//! found again by that place and acted on through its own pattern —
//! Invoke, Value, Toggle, SelectionItem — never the mouse.
//!
//! Time-boxed: a read stops at [`BUDGET_MS`] or the most elements asked
//! for, an app that doesn't answer never holds a call up for long.
//! Elements with nothing to say (a nameless pane or group) are walked
//! through but not listed.

use super::super::elements::{Do, Node};

/// The longest a read may take, in all.
#[cfg_attr(not(windows), allow(dead_code))]
const BUDGET_MS: u128 = 4_000;
/// How deep a read goes.
#[cfg_attr(not(windows), allow(dead_code))]
const DEPTH: usize = 16;

/// Whether a read lists `n` (the window itself, every edit box — a
/// password box too, marked, so the model knows it's there — or anything
/// with a name, a value, a state or something to do). Pure.
pub fn listed(n: &Node) -> bool {
    n.path.is_empty() || matches!(n.role.as_str(), "edit" | "document") || !n.name.trim().is_empty() || n.value.as_deref().is_some_and(|v| !v.is_empty()) || n.toggled.is_some() || !n.can().is_empty()
}

/// **The window's elements** (`hwnd`), at most `max`. Blocking.
#[cfg(windows)]
pub fn tree(hwnd: isize, max: usize) -> Result<Vec<Node>, String> {
    win::tree(hwnd, max)
}

#[cfg(not(windows))]
pub fn tree(_hwnd: isize, _max: usize) -> Result<Vec<Node>, String> {
    Err("Reading a window's elements is Windows only.".into())
}

/// **An element acted on**: found by its place in the window's tree, still
/// what was read (its role and name), then its pattern. What happened, in
/// a few words. Blocking.
#[cfg(windows)]
pub fn act(hwnd: isize, node: &Node, what: &Do) -> Result<String, String> {
    win::act(hwnd, node, what)
}

#[cfg(not(windows))]
pub fn act(_hwnd: isize, _node: &Node, _what: &Do) -> Result<String, String> {
    Err("Acting on a window's elements is Windows only.".into())
}

#[cfg(windows)]
mod win {
    use std::time::Instant;

    use windows::core::BSTR;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::Accessibility::{
        IUIAutomation, IUIAutomationElement, IUIAutomationInvokePattern, IUIAutomationSelectionItemPattern, IUIAutomationTogglePattern, IUIAutomationTreeWalker, IUIAutomationValuePattern, ToggleState_On, UIA_InvokePatternId, UIA_SelectionItemPatternId,
        UIA_TogglePatternId, UIA_ValuePatternId,
    };

    use super::super::super::elements::{role_of, Do, Node};
    use super::{listed, BUDGET_MS, DEPTH};
    use crate::pasting::selection::uia::{automation, Com};
    use crate::snip::geom::Rect;

    /// How long UI Automation may take to answer one request.
    const TIMEOUT_MS: u32 = 1_500;

    fn node_of(e: &IUIAutomationElement, path: Vec<u32>) -> Node {
        let control = unsafe { e.CurrentControlType() }.map(|c| c.0).unwrap_or(0);
        let password = unsafe { e.CurrentIsPassword() }.map(|b| b.as_bool()).unwrap_or(false);
        let r = unsafe { e.CurrentBoundingRectangle() }.unwrap_or_default();
        let value = if matches!(control, 50003 | 50004 | 50030) && !password {
            unsafe { e.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }.ok().and_then(|p| unsafe { p.CurrentValue() }.ok()).map(|b| b.to_string())
        } else {
            None
        };
        let toggled = if control == 50002 {
            unsafe { e.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId) }.ok().and_then(|p| unsafe { p.CurrentToggleState() }.ok()).map(|s| s == ToggleState_On)
        } else {
            None
        };
        let selected = if matches!(control, 50007 | 50013 | 50019 | 50024 | 50029) {
            unsafe { e.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId) }.ok().and_then(|p| unsafe { p.CurrentIsSelected() }.ok()).map(|b| b.as_bool())
        } else {
            None
        };
        Node {
            path,
            role: role_of(control).to_string(),
            name: unsafe { e.CurrentName() }.map(|b| b.to_string()).unwrap_or_default(),
            value,
            enabled: unsafe { e.CurrentIsEnabled() }.map(|b| b.as_bool()).unwrap_or(true),
            focused: unsafe { e.CurrentHasKeyboardFocus() }.map(|b| b.as_bool()).unwrap_or(false),
            password,
            toggled,
            selected,
            rect: Rect::new(r.left, r.top, r.right - r.left, r.bottom - r.top),
        }
    }

    fn children(walker: &IUIAutomationTreeWalker, e: &IUIAutomationElement) -> Vec<IUIAutomationElement> {
        let mut out = Vec::new();
        let mut c = unsafe { walker.GetFirstChildElement(e) }.ok();
        while let Some(k) = c {
            c = unsafe { walker.GetNextSiblingElement(&k) }.ok();
            out.push(k);
            if out.len() >= 1_000 {
                break;
            }
        }
        out
    }

    pub fn tree(hwnd: isize, max: usize) -> Result<Vec<Node>, String> {
        let _com = Com::init().ok_or("COM didn't start")?;
        let a = automation(TIMEOUT_MS).ok_or("UI Automation didn't answer")?;
        let root = unsafe { a.ElementFromHandle(HWND(hwnd as *mut _)) }.map_err(|e| format!("UI Automation couldn't read the window ({e})"))?;
        let walker = unsafe { a.ControlViewWalker() }.map_err(|e| format!("UI Automation's walker ({e})"))?;
        let started = Instant::now();
        let mut out = Vec::new();
        let mut stack: Vec<(IUIAutomationElement, Vec<u32>)> = vec![(root, Vec::new())];
        while let Some((e, path)) = stack.pop() {
            if out.len() >= max || started.elapsed().as_millis() > BUDGET_MS {
                break;
            }
            let node = node_of(&e, path.clone());
            if listed(&node) {
                out.push(node);
            }
            if path.len() >= DEPTH {
                continue;
            }
            for (i, k) in children(&walker, &e).into_iter().enumerate().rev() {
                let mut p = path.clone();
                p.push(i as u32);
                stack.push((k, p));
            }
        }
        Ok(out)
    }

    const CHANGED: &str = "The window changed since it was read: that element isn't there any more. Read the window again.";

    /// **`text` typed into `e`**: the box focused (its window, `hwnd`, in
    /// front), everything in it selected, the text typed as Yap's own
    /// keys, then read back. `None`: it couldn't take the focus (or the text
    /// didn't come out whole): the caller sets it through the Value pattern
    /// instead. Blocking.
    fn typed_in(a: &IUIAutomation, e: &IUIAutomationElement, hwnd: isize, text: &str, value: Option<&IUIAutomationValuePattern>) -> Option<Result<String, String>> {
        use crate::computer::input::{self, Hands};
        use windows::Win32::UI::WindowsAndMessaging::{GetAncestor, GetForegroundWindow, GA_ROOT};
        unsafe { e.SetFocus() }.ok()?;
        std::thread::sleep(std::time::Duration::from_millis(80));
        let focused = unsafe { a.GetFocusedElement() }.ok()?;
        if !unsafe { a.CompareElements(&focused, e) }.map(|b| b.as_bool()).unwrap_or(false) {
            return None;
        }
        // Keys go to the window in front: it must be this one.
        let front = unsafe { GetAncestor(GetForegroundWindow(), GA_ROOT) };
        if front.0 as isize != hwnd {
            return None;
        }
        let mut steps = input::key(&input::keys_of("ctrl+a"), 1);
        steps.extend(input::text(text));
        if input::run(&input::ThisPc as &dyn Hands, &steps, &|| false).is_err() {
            return None;
        }
        std::thread::sleep(std::time::Duration::from_millis(80));
        // Read back where the box says: anything but the text (the select-all didn't take) is set over.
        match value.and_then(|p| unsafe { p.CurrentValue() }.ok()).map(|b| b.to_string()) {
            Some(now) if now != text => {
                let fixed = value.map(|p| unsafe { p.SetValue(&BSTR::from(text)) }).is_some_and(|r| r.is_ok());
                Some(if fixed { Ok("typed it in, then set it whole through its accessibility (check the app used it)".to_string()) } else { Err("The text didn't come out as asked: read the window again.".to_string()) })
            }
            _ => Some(Ok("typed it in".to_string())),
        }
    }

    pub fn act(hwnd: isize, node: &Node, what: &Do) -> Result<String, String> {
        let _com = Com::init().ok_or("COM didn't start")?;
        let a = automation(TIMEOUT_MS).ok_or("UI Automation didn't answer")?;
        let mut e = unsafe { a.ElementFromHandle(HWND(hwnd as *mut _)) }.map_err(|_| CHANGED.to_string())?;
        let walker = unsafe { a.ControlViewWalker() }.map_err(|e| format!("UI Automation's walker ({e})"))?;
        for &i in &node.path {
            e = children(&walker, &e).into_iter().nth(i as usize).ok_or(CHANGED)?;
        }
        let now = node_of(&e, node.path.clone());
        if now.role != node.role || now.name != node.name {
            return Err(CHANGED.into());
        }
        if !now.enabled {
            return Err("That element is disabled.".into());
        }
        let invoke = || -> Option<Result<String, String>> {
            let p = unsafe { e.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) }.ok()?;
            Some(unsafe { p.Invoke() }.map(|_| "clicked it".to_string()).map_err(|x| format!("The app didn't take the click ({x}).")))
        };
        let toggle = || -> Option<Result<String, String>> {
            let p = unsafe { e.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId) }.ok()?;
            Some(unsafe { p.Toggle() }.map_err(|x| format!("The app didn't take it ({x}).")).map(|_| match unsafe { p.CurrentToggleState() } {
                Ok(s) if s == ToggleState_On => "it's ticked now".to_string(),
                Ok(_) => "it's not ticked now".to_string(),
                Err(_) => "toggled it".to_string(),
            }))
        };
        let select = || -> Option<Result<String, String>> {
            let p = unsafe { e.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId) }.ok()?;
            Some(unsafe { p.Select() }.map(|_| "selected it".to_string()).map_err(|x| format!("The app didn't take it ({x}).")))
        };
        let done = match what {
            Do::Click => invoke().or_else(toggle).or_else(select),
            Do::Toggle => toggle(),
            Do::Select => select().or_else(invoke),
            Do::SetValue(text) => {
                let p = unsafe { e.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }.ok();
                if p.as_ref().is_some_and(|p| unsafe { p.CurrentIsReadOnly() }.map(|b| b.as_bool()).unwrap_or(false)) {
                    return Err("That box is read only.".to_string());
                }
                // Typed in, as a person would, when the box takes the focus: an app always takes keystrokes,
                // where some (Windows' Save and Open dialogs) show text set through the Value pattern yet
                // never take it (a real run, 20 Nov 2026: Paint's Save As kept the old name and offered to
                // overwrite the original). The Value pattern is the fallback.
                if let Some(said) = typed_in(&a, &e, hwnd, text, p.as_ref()) {
                    return said;
                }
                p.map(|p| unsafe { p.SetValue(&BSTR::from(text.as_str())) }.map(|_| "set its text through its accessibility (some dialogs show such text yet don't take it: if it matters, check the app used it)".to_string()).map_err(|x| format!("The app didn't take the text ({x}).")))
            }
        };
        done.unwrap_or_else(|| Err("That element can't be acted on that way through its accessibility: use the clicks or typing at its place instead.".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_elements_with_something_to_say_are_listed() {
        let n = |path: Vec<u32>, role: &str, name: &str| Node { path, role: role.into(), name: name.into(), enabled: true, ..Default::default() };
        assert!(listed(&n(vec![], "window", "")), "the window itself");
        assert!(!listed(&n(vec![0], "pane", "")), "a nameless pane: walked through");
        assert!(listed(&n(vec![0], "pane", "Sidebar")));
        assert!(listed(&n(vec![1], "edit", "")), "an edit box takes a value");
        assert!(listed(&Node { password: true, ..n(vec![1], "edit", "") }), "a nameless password box: listed, marked (found by the robot test on a real WinForms pad)");
        assert!(listed(&Node { toggled: Some(true), ..n(vec![2], "custom", "") }));
    }
}

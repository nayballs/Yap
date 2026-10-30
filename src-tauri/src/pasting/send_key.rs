//! **The Send shortcut** (2026-10-29; Wispr Flow's bindable "Press Enter",
//! by behaviour): an extra key or mouse button (Settings → Shortcuts,
//! `config.send_hotkey`, unbound by default; `input_hook.rs`'s eleventh
//! binding, its press only) that presses the send key in the window in
//! front — the one auto-submit and "press enter" use (`auto_submit_key`:
//! Enter, Ctrl+Enter or Shift+Enter; Enter in a terminal,
//! `app_kind::submit_key`) — once the shortcut's own keys are up (a
//! modifier still down would make Enter another key). A mouse's side button
//! sends a message from the hand on the mouse.
//!
//! - Never Enter itself (it would press itself again: the hook hears Yap's
//!   own keys too), never a key another shortcut has (`input_hook`).
//! - An app running as administrator takes no keys from Yap: said once.
//! - No in-page fallback, as Paste last has none: the global hook never
//!   hears keys while one of Yap's own windows has the focus, and Yap's
//!   own boxes send with their own Enter.
//! - Test mode presses nothing: the log says which key it would; robot
//!   mode only in the test's own windows.

use tauri::AppHandle;
use tracing::{info, warn};

use crate::notices::alert::{self, Alert};
use crate::pasting::text_injector;

/// Is this shortcut Enter itself (any modifiers)? The send key can't be.
pub fn is_enter(spec: &str) -> bool {
    let spec = spec.trim();
    spec.strip_prefix("kb:")
        .and_then(|rest| rest.rsplit('+').next())
        .is_some_and(|vk| vk.trim() == "13")
}

/// The Send shortcut's press: on a thread of its own, once its keys are up.
pub fn press_soon(app: &AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("send-key".into()).spawn(move || {
        crate::pasting::as_said::wait_for_modifiers_up();
        match press(&app) {
            Ok(key) => info!(key, "Send shortcut: pressed the send key"),
            Err(e) => warn!("Send shortcut: {e}"),
        }
    });
    if let Err(e) = spawned {
        warn!("Send shortcut: no thread ({e})");
    }
}

/// Which key sends in an app of `kind`: the submit key, Enter in a terminal.
fn key_for(kind: crate::pasting::app_kind::AppKind, configured: &str) -> String {
    crate::pasting::app_kind::submit_key(kind, true, true, configured).unwrap_or("enter").to_string()
}

/// Press the send key in the window in front; the key it pressed.
fn press(app: &AppHandle) -> Result<String, String> {
    let configured = crate::config::load().auto_submit_key;
    let target = text_injector::current_foreground();
    match crate::shell::realtest::gate(target, "press the send key") {
        crate::shell::realtest::Gate::Go => {}
        crate::shell::realtest::Gate::TestMode => {
            #[cfg(debug_assertions)]
            let kind = crate::pasting::compose::test_kind().unwrap_or_default();
            #[cfg(not(debug_assertions))]
            let kind = crate::pasting::app_kind::AppKind::Other;
            let key = key_for(kind, &configured);
            info!(key, "e2e: not pressing the send key in another app");
            return Ok(key);
        }
        crate::shell::realtest::Gate::Refused(why) => return Err(why),
    }
    if target.is_some_and(crate::pasting::elevated::target_blocked) {
        alert::show(
            app,
            Alert {
                kind: "pasteBlocked",
                icon: "clipboard",
                title: "Couldn't press the send key".into(),
                body: "This app runs as administrator, so Windows doesn't let Yap press keys in it. Press Enter yourself.".into(),
                action: None,
                timeout_ms: 8_000,
            },
        );
        return Err("the app runs as administrator".into());
    }
    let key = key_for(crate::pasting::app_kind::of_window(target), &configured);
    tauri::async_runtime::block_on(text_injector::press_submit(&key, target))?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pasting::app_kind::{AppKind, Host, Terminal};

    #[test]
    fn enter_can_never_be_the_send_shortcut() {
        assert!(is_enter("kb:13"));
        assert!(is_enter("kb:ctrl+13"));
        assert!(is_enter(" kb:shift+13 "));
        assert!(!is_enter("kb:131"));
        assert!(!is_enter("mouse:5"));
        assert!(!is_enter(""));
    }

    #[test]
    fn the_key_is_the_submit_key_and_enter_in_a_terminal() {
        assert_eq!(key_for(AppKind::Other, "ctrlEnter"), "ctrlEnter");
        assert_eq!(key_for(AppKind::Messaging, "enter"), "enter");
        let t = AppKind::Terminal(Terminal { coding_cli: None, host: Host::WindowsTerminal });
        assert_eq!(key_for(t, "ctrlEnter"), "enter");
    }
}

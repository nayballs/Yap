//! One-off notices about a dictation that the person needs to act on — a
//! paste Windows wouldn't let through (`elevated.rs`: an app running as
//! administrator), the microphone switched off in Windows' privacy settings
//! (`mic_privacy.rs`) — put where they'll be seen: a toast in the main
//! window while it's on screen, else a card on the Yap bar, else a Windows
//! notification. (coaching.rs and learn.rs pick the first two the same way;
//! a tip can go unsaid, these can't.) An alert may carry one button
//! ([`Action`]), whichever surface shows it; its id comes back to [`act`].

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

/// The main window's event: an [`Alert`].
const EVENT: &str = "yap-alert";

/// **Open settings**: Windows Settings → Privacy & security → Microphone.
pub const OPEN_MIC_SETTINGS: &str = "openMicSettings";

/// An alert's one button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Action {
    /// What [`act`] does: [`OPEN_MIC_SETTINGS`].
    pub id: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    /// "pasteBlocked" | "micBlocked": the bar card's id and the Windows
    /// notification's tag (a newer one replaces it).
    pub kind: &'static str,
    /// The chip's glyph: "clipboard" | "mic".
    pub icon: &'static str,
    pub title: String,
    pub body: String,
    pub action: Option<Action>,
    /// How long the toast / card stays up.
    pub timeout_ms: u64,
}

/// Where an alert goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// A toast in the main window.
    Window,
    /// A card on the Yap bar.
    Bar,
    /// A Windows notification.
    Notification,
    /// Nowhere (the bar off and notifications not allowed: portable, a test
    /// run). The log has it.
    Nowhere,
}

/// The main window if it's on screen, else the bar if it takes cards, else
/// a Windows notification if Yap may post one.
pub fn surface(window_on_screen: bool, bar_takes_cards: bool, notifications: bool) -> Surface {
    if window_on_screen {
        Surface::Window
    } else if bar_takes_cards {
        Surface::Bar
    } else if notifications {
        Surface::Notification
    } else {
        Surface::Nowhere
    }
}

/// The main window is on screen (not hidden, not minimized). Waits on the
/// main thread: never call with a lock held.
fn main_window_visible(app: &AppHandle) -> bool {
    app.get_webview_window("settings")
        .is_some_and(|w| w.is_visible().unwrap_or(false) && !w.is_minimized().unwrap_or(false))
}

fn notifications_allowed() -> bool {
    #[cfg(windows)]
    {
        crate::win_toast::allowed()
    }
    #[cfg(not(windows))]
    false
}

/// Show `alert` where it'll be seen; returns where. Blocking (window
/// queries): call it off the pipeline's locks — [`show_soon`] does.
pub fn show(app: &AppHandle, alert: Alert) -> Surface {
    let card = || crate::bar::Card {
        id: alert.kind.into(),
        icon: alert.icon,
        title: alert.title.clone(),
        body: alert.body.clone(),
        primary: alert.action.map(|a| crate::bar::CardAction::new(a.id, a.label)),
        timeout_ms: Some(alert.timeout_ms),
        ..Default::default()
    };
    let mut place = surface(main_window_visible(app), crate::bar::cards_available(), notifications_allowed());
    if place == Surface::Window && app.emit(EVENT, &alert).is_err() {
        place = Surface::Bar;
    }
    if place == Surface::Bar && !crate::bar::show_card(app, card(), Box::new(act)) {
        place = if notifications_allowed() { Surface::Notification } else { Surface::Nowhere };
    }
    if place == Surface::Notification {
        #[cfg(windows)]
        if let Err(e) = native::post(app, &alert) {
            tracing::info!("alert: no Windows notification ({e})");
            place = Surface::Nowhere;
        }
    }
    tracing::info!(kind = alert.kind, surface = ?place, "alert: {}", alert.title);
    place
}

/// [`show`] on a thread of its own: safe from anywhere (a hotkey's handler
/// holding the pipeline's locks, the main thread).
pub fn show_soon(app: &AppHandle, alert: Alert) {
    let app = app.clone();
    if let Err(e) = std::thread::Builder::new().name("alert".into()).spawn(move || {
        show(&app, alert);
    }) {
        tracing::warn!("alert: couldn't show it ({e})");
    }
}

/// An alert's button (the toast's, the bar card's, the notification's).
pub fn act(app: &AppHandle, action: &str) {
    match action {
        OPEN_MIC_SETTINGS => open_uri(app, crate::mic_privacy::SETTINGS_URI),
        "" => {}
        other => tracing::debug!("alert: no such action {other}"),
    }
}

/// Open a Windows Settings page. A test run only says so in the log.
fn open_uri(app: &AppHandle, uri: &str) {
    if crate::e2e::active() {
        tracing::info!("e2e: not opening {uri}");
        return;
    }
    use tauri_plugin_opener::OpenerExt;
    if let Err(e) = app.opener().open_url(uri, None::<&str>) {
        tracing::warn!("alert: couldn't open {uri}: {e}");
    }
}

/// The toast's button (`yap-alert`'s `action.id`).
#[tauri::command]
pub fn alert_action(app: AppHandle, action: String) {
    act(&app, &action);
}

/// The alerts' Windows notifications: Yap's logo, silent, the button as a
/// toast action; kept alive so it works from the notification center.
#[cfg(windows)]
mod native {
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex};

    use tauri::AppHandle;
    use windows::UI::Notifications::ToastNotification;

    use super::Alert;
    use crate::win_toast::{esc, logo_xml};

    static LIVE: LazyLock<Mutex<HashMap<&'static str, ToastNotification>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));

    pub fn xml(a: &Alert, logo: &str) -> String {
        let (launch, actions) = match a.action {
            Some(action) => (
                action.id.to_string(),
                format!(
                    "<actions><action content=\"{}\" arguments=\"{}\" activationType=\"foreground\"/></actions>",
                    esc(action.label),
                    esc(action.id)
                ),
            ),
            None => (String::new(), String::new()),
        };
        format!(
            "<toast launch=\"{}\"><visual><binding template=\"ToastGeneric\">\
             <text>{}</text><text>{}</text>{logo}</binding></visual>{actions}\
             <audio silent=\"true\"/></toast>",
            esc(&launch),
            esc(&a.title),
            esc(&a.body),
        )
    }

    fn activated(app: &AppHandle, arg: &str) {
        super::act(app, arg);
    }

    pub fn post(app: &AppHandle, a: &Alert) -> Result<(), String> {
        let toast = crate::win_toast::post(app, a.kind, &xml(a, &logo_xml()), None, activated)?;
        LIVE.lock().unwrap_or_else(|p| p.into_inner()).insert(a.kind, toast);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_first_then_the_bar_then_a_notification() {
        assert_eq!(surface(true, true, true), Surface::Window);
        assert_eq!(surface(true, false, false), Surface::Window);
        assert_eq!(surface(false, true, true), Surface::Bar);
        assert_eq!(surface(false, false, true), Surface::Notification);
        assert_eq!(surface(false, false, false), Surface::Nowhere);
    }

    #[cfg(windows)]
    #[test]
    fn the_notification_carries_the_button() {
        let a = crate::mic_privacy::alert(crate::mic_privacy::Blocked::DesktopApps);
        let xml = native::xml(&a, "");
        assert!(xml.contains("arguments=\"openMicSettings\""), "{xml}");
        assert!(xml.contains("content=\"Open settings\""), "{xml}");
        assert!(xml.contains("launch=\"openMicSettings\""), "{xml}");
        // Quotes and angle brackets in the text are escaped.
        assert!(xml.contains("“Let desktop apps access your microphone”"), "{xml}");
    }
}

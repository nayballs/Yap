//! One-off notices the person needs to act on — a paste Windows wouldn't
//! let through (`elevated.rs`: an app running as administrator), the
//! microphone switched off in Windows' privacy settings (`mic_privacy.rs`),
//! call prompts' fatigue ("Stop asking about calls?", `meeting_detect.rs`),
//! a settings file Yap couldn't read and set aside (`config.rs`), a save
//! that failed by itself ("Couldn't save your dictation history", with Try
//! again: `saving.rs`)
//! — put in **one place** (`notice::place`): a toast in the main window
//! while the person is looking at it, else a card on the Yap bar, else a
//! Windows notification, else the window if it's on screen. Yap's notice
//! policy (`notice.rs`) applies on each: an alert about a dictation is
//! never held back behind it, and the same one again within its kind's
//! cooldown isn't shown. (coaching.rs and learn.rs place their tips the
//! same way, minus the Windows notification: a tip can go unsaid, these
//! can't.) An alert may carry one button ([`Action`]), whichever surface
//! shows it; its id comes back to [`act`].

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// The main window's event: an [`Alert`].
const EVENT: &str = "yap-alert";

/// **Open settings**: Windows Settings → Privacy & security → Microphone.
pub const OPEN_MIC_SETTINGS: &str = "openMicSettings";
/// **Stop asking**: call detection off ("Stop asking about calls?").
pub const STOP_ASKING_CALLS: &str = "callsStopAsking";
/// **Turn back on**: call detection on again (after Yap stopped asking).
pub const ASK_ABOUT_CALLS: &str = "callsTurnBackOn";
/// **Open settings**: Yap's Settings → Language Models → Chat & Voice
/// Assistant (the Voice Assistant has no AI model to answer with).
pub const OPEN_AI_SETTINGS: &str = "openAiSettings";
/// **Open settings**: Yap's Settings → Voice Assistant → Voice (a cloud
/// voice didn't answer: `tts::cloud_failed`).
pub const OPEN_VOICE_SETTINGS: &str = "openVoiceSettings";
/// **Show file**: the settings file Yap couldn't read, set aside, in
/// Explorer (`config::reset_alert`).
pub const SHOW_OLD_SETTINGS: &str = "showOldSettings";

/// An alert's one button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Action {
    /// What [`act`] does: [`OPEN_MIC_SETTINGS`], [`STOP_ASKING_CALLS`],
    /// [`ASK_ABOUT_CALLS`], [`OPEN_AI_SETTINGS`], [`OPEN_VOICE_SETTINGS`],
    /// [`SHOW_OLD_SETTINGS`], `saving::RETRY` (Try again).
    pub id: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    /// "pasteBlocked" | "micBlocked" | "calls-fatigue" | "assistantNoModel" |
    /// "assistantFailed" | "voiceFallback" (`tts.rs`: a cloud voice failed) |
    /// "configReset" (`config.rs`: config.json couldn't be read, set aside) |
    /// "saveFailed" (`saving.rs`: a store couldn't be saved; "Saved" after
    /// Try again):
    /// the bar card's id, the Windows notification's tag (a newer one
    /// replaces it) and its rule in `notice.rs`.
    pub kind: &'static str,
    /// The chip's glyph: "clipboard" | "mic" | "call" | "assistant" |
    /// "settings" | "save".
    pub icon: &'static str,
    pub title: String,
    pub body: String,
    pub action: Option<Action>,
    /// How long the toast / card stays up.
    pub timeout_ms: u64,
}

/// Where an alert went (`notice::Place`).
pub use crate::notices::notice::Place as Surface;

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
    let mut place = crate::notices::notice::place(app, true);
    // The same alert again within its cooldown (`notice.rs`: the mic
    // privacy one is said once per ten minutes; the overlay's line says it
    // on every press). The bar and Windows check for themselves.
    if place == Surface::Window && !crate::notices::notice::admit(alert.kind, &alert.title, &alert.body) {
        tracing::info!(kind = alert.kind, "alert: said a moment ago; not again");
        return Surface::Nowhere;
    }
    if place == Surface::Window && app.emit(EVENT, &alert).is_err() {
        place = Surface::Bar;
    }
    if place == Surface::Bar && !crate::bar::show_card(app, card(), Box::new(act)) {
        place = if crate::notices::notice::notifications_allowed() { Surface::Notification } else { Surface::Nowhere };
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

/// Snip to Chat refused during a dictation (`snip.rs`).
pub fn snip_dictating(app: &AppHandle) {
    show_soon(
        app,
        Alert {
            kind: "snipDictating",
            icon: "snip",
            title: "Finish the dictation first".into(),
            body: "Snip to Chat starts once the dictation is done.".into(),
            action: None,
            timeout_ms: 4_000,
        },
    );
}

/// A snip that couldn't be taken (`snip.rs`): `why` is Yap's own words,
/// never the screen's content.
pub fn snip_failed(app: &AppHandle, why: &str) {
    show_soon(
        app,
        Alert {
            kind: "snipFailed",
            icon: "snip",
            title: "Couldn't take the screenshot".into(),
            body: format!(
                "{why}. Windows never lets an app capture its administrator prompts (the dimmed screen), and video protected from copying comes out black."
            ),
            action: None,
            timeout_ms: 8_000,
        },
    );
}

/// **Show in folder**: the screenshot Snip to Chat saved last
/// (`snip::copy_save`).
pub const SHOW_SNIP: &str = "showSnip";

/// A snip copied (Ctrl as you let go, Ctrl+C in the overlay): the answer
/// to the person's own press.
pub fn snip_copied(app: &AppHandle, w: u32, h: u32) {
    show_soon(
        app,
        Alert {
            kind: "snipCopied",
            icon: "snip",
            title: "Screenshot copied".into(),
            body: format!("{w} × {h}, ready to paste. It stays out of your clipboard history."),
            action: None,
            timeout_ms: 3_000,
        },
    );
}

/// A snip saved (Shift as you let go, Ctrl+S in the overlay).
pub fn snip_saved(app: &AppHandle, name: &str) {
    show_soon(
        app,
        Alert {
            kind: "snipSaved",
            icon: "snip",
            title: "Screenshot saved".into(),
            body: format!("Saved as {name}."),
            action: Some(Action { id: SHOW_SNIP, label: "Show in folder" }),
            timeout_ms: 5_000,
        },
    );
}

/// An alert's button (the toast's, the bar card's, the notification's).
pub fn act(app: &AppHandle, action: &str) {
    match action {
        OPEN_MIC_SETTINGS => open_uri(app, crate::dictation::mic_privacy::SETTINGS_URI),
        STOP_ASKING_CALLS => crate::calls::meeting_detect::set_detection(app, false),
        crate::meetings::meeting_audio::RECOVER_ACTION => crate::meetings::meeting_audio::recover_all(app),
        ASK_ABOUT_CALLS => crate::calls::meeting_detect::set_detection(app, true),
        OPEN_AI_SETTINGS | OPEN_VOICE_SETTINGS => {
            use tauri::Emitter;
            let at = if action == OPEN_AI_SETTINGS { "cleanup#chat" } else { "voice#voice-provider" };
            match crate::shell::window::show_settings(app) {
                Ok(()) => {
                    let _ = app.emit("yap-open-settings", at);
                }
                Err(e) => tracing::warn!("alert: couldn't open Settings ({e})"),
            }
        }
        SHOW_OLD_SETTINGS => crate::config::show_old_file(app),
        SHOW_SNIP => crate::snip::copy_save::show_saved(app),
        crate::shell::saving::RETRY => crate::shell::saving::retry_soon(app),
        "" => {}
        other => tracing::debug!("alert: no such action {other}"),
    }
}

/// Open a Windows Settings page. A test run only says so in the log.
fn open_uri(app: &AppHandle, uri: &str) {
    if crate::shell::e2e::active() {
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
    use crate::notices::win_toast::{esc, logo_xml};

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
        let toast = crate::notices::win_toast::post(app, a.kind, &xml(a, &logo_xml()), None, activated)?;
        LIVE.lock().unwrap_or_else(|p| p.into_inner()).insert(a.kind, toast);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn the_notification_carries_the_button() {
        let a = crate::dictation::mic_privacy::alert(crate::dictation::mic_privacy::Blocked::DesktopApps);
        let xml = native::xml(&a, "");
        assert!(xml.contains("arguments=\"openMicSettings\""), "{xml}");
        assert!(xml.contains("content=\"Open settings\""), "{xml}");
        assert!(xml.contains("launch=\"openMicSettings\""), "{xml}");
        // Quotes and angle brackets in the text are escaped.
        assert!(xml.contains("“Let desktop apps access your microphone”"), "{xml}");
        let plain = native::xml(&crate::pasting::elevated::alert(), "");
        assert!(!plain.contains("<actions>"), "{plain}");
        assert!(plain.contains("Ctrl+V"), "{plain}");
    }
}

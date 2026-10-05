//! Windows notifications (WinRT toasts in the notification center) — the
//! plumbing shared by updates.rs (the update announcement and a download's
//! live progress bar) and meeting_detect.rs (the call prompts).
//!
//! Every toast goes out under Yap's app identifier, with Yap's logo, in one
//! "yap" group under a caller-chosen tag: posting the same tag again replaces
//! the earlier toast, and [`remove`] takes it out of the notification center.
//! Clicks come back through the toast's Activated event while Yap runs, so a
//! caller keeps the [`ToastNotification`] that [`post`] returns alive for as
//! long as the toast can still be clicked. Windows applies Do Not Disturb /
//! Focus Assist and the per-app notification switch on its own; when Yap's
//! notifications are switched off, [`post`] says so and the caller falls back
//! to its in-app toast.

use tauri::AppHandle;
use windows::core::{IInspectable, Interface, HSTRING};
use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::{
    NotificationData, NotificationSetting, ToastActivatedEventArgs, ToastNotification,
    ToastNotificationManager,
};

const GROUP: &str = "yap";

/// Release builds post as Yap: the installer's Start-menu shortcut carries
/// the app identifier as its AppUserModelID (NSIS SetLnkAppUserModelId).
/// A dev build has no such shortcut and borrows PowerShell's — the usual
/// stand-in for an unregistered app.
pub fn app_id(app: &AppHandle) -> String {
    if cfg!(debug_assertions) {
        "{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\\WindowsPowerShell\\v1.0\\powershell.exe".into()
    } else {
        app.config().identifier.clone()
    }
}

/// Escape text for the toast XML.
pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Yap's logo in the toast's app-logo slot. An unpackaged app's toast only
/// loads local files, so the PNG baked into the binary is written next to
/// Yap's data once ("" — no logo — if that fails).
pub fn logo_xml() -> String {
    static LOGO: &[u8] = include_bytes!("../icons/128x128@2x.png");
    let dir = crate::config::data_dir();
    let path = dir.join("notification-logo.png");
    let current = std::fs::metadata(&path).is_ok_and(|m| m.len() == LOGO.len() as u64);
    if !current && (std::fs::create_dir_all(&dir).is_err() || std::fs::write(&path, LOGO).is_err()) {
        return String::new();
    }
    match url::Url::from_file_path(&path) {
        Ok(src) => format!("<image placement=\"appLogoOverride\" src=\"{}\"/>", esc(src.as_str())),
        Err(()) => String::new(),
    }
}

/// Post `xml` as Yap's toast `tag` (replacing an earlier one with that tag).
/// `on_activated` gets the clicked button's `arguments` — the toast's
/// `launch` value for a click on its body. Returns the live toast.
pub fn post(
    app: &AppHandle,
    tag: &str,
    xml: &str,
    data: Option<&NotificationData>,
    on_activated: fn(&AppHandle, &str),
) -> Result<ToastNotification, String> {
    let err = |e: windows::core::Error| e.message();
    let doc = XmlDocument::new().map_err(err)?;
    doc.LoadXml(&HSTRING::from(xml)).map_err(err)?;
    let toast = ToastNotification::CreateToastNotification(&doc).map_err(err)?;
    toast.SetTag(&HSTRING::from(tag)).map_err(err)?;
    toast.SetGroup(&HSTRING::from(GROUP)).map_err(err)?;
    let _ = toast.SetExpiresOnReboot(true);
    if let Some(data) = data {
        toast.SetData(data).map_err(err)?;
    }

    let handle = app.clone();
    let handler = TypedEventHandler::<ToastNotification, IInspectable>::new(move |_, args| {
        let arg = args
            .as_ref()
            .and_then(|a| a.cast::<ToastActivatedEventArgs>().ok())
            .and_then(|a| a.Arguments().ok())
            .map(|h| h.to_string())
            .unwrap_or_default();
        on_activated(&handle, &arg);
        Ok(())
    });
    toast.Activated(&handler).map_err(err)?;

    let notifier =
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id(app)))
            .map_err(err)?;
    // Switched off by the user (or policy): say so, and the in-app toast
    // takes over. `Setting` errors ("Element not found") until Windows has
    // shown this app's first notification — that's not a "no".
    if let Ok(setting) = notifier.Setting() {
        if setting != NotificationSetting::Enabled {
            return Err(format!("notifications are off for Yap ({})", setting.0));
        }
    }
    notifier.Show(&toast).map_err(err)?;
    Ok(toast)
}

/// Update the data bound into toast `tag` (a progress bar's values).
pub fn update(app: &AppHandle, tag: &str, data: &NotificationData) {
    if let Ok(notifier) =
        ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(app_id(app)))
    {
        let _ = notifier.UpdateWithTagAndGroup(data, &HSTRING::from(tag), &HSTRING::from(GROUP));
    }
}

/// Take toast `tag` out of the notification center.
pub fn remove(app: &AppHandle, tag: &str) {
    if let Ok(history) = ToastNotificationManager::History() {
        let _ = history.RemoveGroupedTagWithId(
            &HSTRING::from(tag),
            &HSTRING::from(GROUP),
            &HSTRING::from(app_id(app)),
        );
    }
}

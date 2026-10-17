//! **Windows' microphone privacy switches**: when they deny desktop apps the
//! microphone, a recording gets nothing but digital silence (or no device at
//! all), and the dead-mic watchdog's "No sound from <mic>" would send the
//! person looking at the wrong thing. Yap reads the switches — the registry
//! only, never written — and says what's really going on: "Windows is
//! blocking microphone access for desktop apps", with **Open settings**
//! (`ms-settings:privacy-microphone`).
//!
//! The switches, as Settings → Privacy & security → Microphone shows them
//! (each a `Value` of "Allow" / "Deny" under
//! `…\CapabilityAccessManager\ConsentStore\microphone`):
//!
//! - **Microphone access** (the whole device): HKLM's;
//! - **Let apps access your microphone**: HKCU's;
//! - **Let desktop apps access your microphone**: HKCU's `NonPackaged`
//!   subkey (and HKLM's, the machine's default, where it's set);
//! - an organisation's policy forcing it off: `LetAppsAccessMicrophone` = 2
//!   under HKLM `SOFTWARE\Policies\Microsoft\Windows\AppPrivacy`.
//!
//! A missing value is no answer (Windows' default is to allow). Checked when
//! a recording starts (said at once: the recording goes on, so turning the
//! switch on mid-dictation still works), in the "No microphone found" path,
//! and by the dead-mic watchdog before it blames the mic. The decision is
//! pure ([`decide`]) and unit-tested; a test run reads no registry at all —
//! the debug-only [`e2e_mic_privacy`] stands in.

use serde::Serialize;

/// Which switch is off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Blocked {
    /// "Microphone access" is off for the whole device.
    Device,
    /// "Let apps access your microphone" is off.
    Apps,
    /// "Let desktop apps access your microphone" is off.
    DesktopApps,
    /// An organisation's policy denies apps the microphone.
    Policy,
}

/// The switches as the registry has them (`None`: no value).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Switches {
    /// HKLM `…\microphone` `Value`: allowed?
    pub device: Option<bool>,
    /// HKCU `…\microphone` `Value`.
    pub apps: Option<bool>,
    /// HKCU `…\microphone\NonPackaged` `Value`.
    pub desktop_apps: Option<bool>,
    /// HKLM `…\microphone\NonPackaged` `Value` (the machine's default for
    /// desktop apps; the person's own one wins when set).
    pub machine_desktop_apps: Option<bool>,
    /// `LetAppsAccessMicrophone` (0 the user decides, 1 force allow, 2
    /// force deny).
    pub policy: Option<u32>,
}

/// "Allow" / "Deny" (any case) → allowed; anything else is no answer.
pub fn parse_value(value: &str) -> Option<bool> {
    match value.trim() {
        v if v.eq_ignore_ascii_case("allow") => Some(true),
        v if v.eq_ignore_ascii_case("deny") => Some(false),
        _ => None,
    }
}

/// Whether the switches keep desktop apps such as Yap off the microphone,
/// and which one does (the widest first: that's the one to turn on).
pub fn decide(s: Switches) -> Option<Blocked> {
    if s.policy == Some(2) {
        return Some(Blocked::Policy);
    }
    if s.device == Some(false) {
        return Some(Blocked::Device);
    }
    if s.apps == Some(false) {
        return Some(Blocked::Apps);
    }
    if s.desktop_apps.or(s.machine_desktop_apps) == Some(false) {
        return Some(Blocked::DesktopApps);
    }
    None
}

/// What the person sees: the same title everywhere; the body names the
/// switch to turn on.
pub const TITLE: &str = "Windows is blocking microphone access for desktop apps";
/// The dictation overlay's one line.
pub const SHORT: &str = "Windows is blocking the microphone";
/// Where **Open settings** goes.
pub const SETTINGS_URI: &str = "ms-settings:privacy-microphone";

pub fn body(b: Blocked) -> &'static str {
    match b {
        Blocked::DesktopApps => {
            "Turn on “Let desktop apps access your microphone” in Settings → Privacy & security → Microphone."
        }
        Blocked::Apps => {
            "Turn on “Let apps access your microphone” and “Let desktop apps access your microphone” in Settings → Privacy & security → Microphone."
        }
        Blocked::Device => "Turn on “Microphone access” in Settings → Privacy & security → Microphone.",
        Blocked::Policy => "Your organisation's settings turn the microphone off for apps on this PC. Ask whoever manages it.",
    }
}

/// The overlay's line (`yap-mic-silent`): the dead-mic watchdog's payload
/// shape, so the capsule shows it in the waveform's place.
pub fn mic_silent() -> crate::mic_watch::MicSilent {
    crate::mic_watch::MicSilent {
        message: TITLE.to_string(),
        short: SHORT.to_string(),
    }
}

/// The notice for [`crate::alert`]: a toast, a bar card or a Windows
/// notification, with **Open settings**.
pub fn alert(b: Blocked) -> crate::alert::Alert {
    crate::alert::Alert {
        kind: "micBlocked",
        icon: "mic",
        title: TITLE.to_string(),
        body: body(b).to_string(),
        action: Some(crate::alert::Action {
            id: crate::alert::OPEN_MIC_SETTINGS,
            label: "Open settings",
        }),
        timeout_ms: 15_000,
    }
}

/// Is the microphone blocked for Yap right now? Two or three registry reads
/// (never a write). A test run reads none: [`e2e_mic_privacy`]'s answer.
pub fn blocked() -> Option<Blocked> {
    if crate::e2e::active() {
        return test_override();
    }
    #[cfg(windows)]
    {
        decide(registry::read())
    }
    #[cfg(not(windows))]
    None
}

#[cfg(debug_assertions)]
static TEST_BLOCKED: std::sync::Mutex<Option<Blocked>> = std::sync::Mutex::new(None);

fn test_override() -> Option<Blocked> {
    #[cfg(debug_assertions)]
    {
        *TEST_BLOCKED.lock().unwrap_or_else(|p| p.into_inner())
    }
    #[cfg(not(debug_assertions))]
    None
}

/// Test mode only: pretend Windows' privacy settings block the mic
/// (`blocked`: "desktopApps" | "apps" | "device" | "policy"), or don't
/// (`null`). A test run never reads the real switches.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_mic_privacy(blocked: Option<Blocked>) -> Result<(), String> {
    if !crate::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    tracing::info!(?blocked, "e2e: microphone privacy");
    *TEST_BLOCKED.lock().unwrap_or_else(|p| p.into_inner()) = blocked;
    Ok(())
}

// The command's argument comes from the page as a string.
impl<'de> serde::Deserialize<'de> for Blocked {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        match s.as_str() {
            "device" => Ok(Blocked::Device),
            "apps" => Ok(Blocked::Apps),
            "desktopApps" => Ok(Blocked::DesktopApps),
            "policy" => Ok(Blocked::Policy),
            other => Err(serde::de::Error::custom(format!("unknown microphone switch: {other}"))),
        }
    }
}

#[cfg(windows)]
mod registry {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
    };

    use super::{parse_value, Switches};

    const MIC: &str = r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";
    const DESKTOP: &str =
        r"Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\NonPackaged";
    const POLICY: &str = r"SOFTWARE\Policies\Microsoft\Windows\AppPrivacy";

    /// A REG_SZ value, if it's there.
    fn string(root: HKEY, key: &str, value: &str) -> Option<String> {
        let mut buf = [0u16; 64];
        let mut size = std::mem::size_of_val(&buf) as u32;
        let r = unsafe {
            RegGetValueW(
                root,
                &HSTRING::from(key),
                &HSTRING::from(value),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
                Some(&mut size),
            )
        };
        if r != ERROR_SUCCESS {
            return None;
        }
        let units = (size as usize / 2).min(buf.len());
        let s = String::from_utf16_lossy(&buf[..units]);
        Some(s.trim_end_matches('\0').to_string())
    }

    fn dword(root: HKEY, key: &str, value: &str) -> Option<u32> {
        let mut v = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let r = unsafe {
            RegGetValueW(
                root,
                &HSTRING::from(key),
                &HSTRING::from(value),
                RRF_RT_REG_DWORD,
                None,
                Some(&mut v as *mut u32 as *mut core::ffi::c_void),
                Some(&mut size),
            )
        };
        (r == ERROR_SUCCESS).then_some(v)
    }

    fn switch(root: HKEY, key: &str) -> Option<bool> {
        string(root, key, "Value").as_deref().and_then(parse_value)
    }

    pub fn read() -> Switches {
        Switches {
            device: switch(HKEY_LOCAL_MACHINE, MIC),
            apps: switch(HKEY_CURRENT_USER, MIC),
            desktop_apps: switch(HKEY_CURRENT_USER, DESKTOP),
            machine_desktop_apps: switch(HKEY_LOCAL_MACHINE, DESKTOP),
            policy: dword(HKEY_LOCAL_MACHINE, POLICY, "LetAppsAccessMicrophone"),
        }
    }

    /// This PC's switches, read-only (the `this_machine` test).
    #[cfg(test)]
    pub fn read_for_test() -> Switches {
        read()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registry_values_read_loosely() {
        assert_eq!(parse_value("Allow"), Some(true));
        assert_eq!(parse_value("deny"), Some(false));
        assert_eq!(parse_value(" DENY "), Some(false));
        assert_eq!(parse_value("Prompt"), None);
        assert_eq!(parse_value(""), None);
    }

    #[test]
    fn nothing_set_or_everything_allowed_is_not_blocked() {
        assert_eq!(decide(Switches::default()), None);
        let allowed = Switches {
            device: Some(true),
            apps: Some(true),
            desktop_apps: Some(true),
            machine_desktop_apps: Some(true),
            policy: Some(0),
        };
        assert_eq!(decide(allowed), None);
        // A policy that forces it on blocks nothing.
        assert_eq!(decide(Switches { policy: Some(1), ..allowed }), None);
    }

    #[test]
    fn each_switch_blocks_desktop_apps() {
        let on = Switches {
            device: Some(true),
            apps: Some(true),
            desktop_apps: Some(true),
            ..Switches::default()
        };
        assert_eq!(decide(Switches { desktop_apps: Some(false), ..on }), Some(Blocked::DesktopApps));
        assert_eq!(decide(Switches { apps: Some(false), ..on }), Some(Blocked::Apps));
        assert_eq!(decide(Switches { device: Some(false), ..on }), Some(Blocked::Device));
        assert_eq!(decide(Switches { policy: Some(2), ..on }), Some(Blocked::Policy));
    }

    #[test]
    fn the_widest_switch_is_named_first() {
        let all_off = Switches {
            device: Some(false),
            apps: Some(false),
            desktop_apps: Some(false),
            machine_desktop_apps: Some(false),
            policy: Some(2),
        };
        assert_eq!(decide(all_off), Some(Blocked::Policy));
        assert_eq!(decide(Switches { policy: None, ..all_off }), Some(Blocked::Device));
        assert_eq!(decide(Switches { policy: None, device: None, ..all_off }), Some(Blocked::Apps));
    }

    #[test]
    fn the_persons_desktop_switch_wins_over_the_machines_default() {
        let machine_off = Switches {
            machine_desktop_apps: Some(false),
            ..Switches::default()
        };
        assert_eq!(decide(machine_off), Some(Blocked::DesktopApps));
        assert_eq!(decide(Switches { desktop_apps: Some(true), ..machine_off }), None);
    }

    #[test]
    fn the_message_names_the_switch_to_turn_on() {
        assert!(body(Blocked::DesktopApps).contains("Let desktop apps access your microphone"));
        assert!(body(Blocked::Apps).contains("Let apps access your microphone"));
        assert!(body(Blocked::Device).contains("Microphone access"));
        assert!(!body(Blocked::Policy).contains("Turn on"));
        let a = alert(Blocked::DesktopApps);
        assert_eq!(a.title, TITLE);
        assert_eq!(a.action.map(|x| x.label), Some("Open settings"));
    }

    #[test]
    fn the_page_names_a_switch() {
        let b: Blocked = serde_json::from_str("\"desktopApps\"").unwrap();
        assert_eq!(b, Blocked::DesktopApps);
        assert!(serde_json::from_str::<Blocked>("\"camera\"").is_err());
    }

    /// This PC's real switches, read-only: `cargo test --lib -- --ignored
    /// mic_privacy_this_machine --nocapture`.
    #[cfg(windows)]
    #[test]
    #[ignore = "reads this PC's privacy settings"]
    fn mic_privacy_this_machine() {
        let s = registry::read_for_test();
        println!("{s:?} → {:?}", decide(s));
    }
}

//! **The laptop's lid** (2026-10-29; Wispr Flow's clamshell rule, by
//! behaviour): with the lid closed — the laptop on a desk, running an
//! external screen — its own microphone hears nothing useful.
//!
//! - Windows says when the lid opens or closes (`GUID_LIDSWITCH_STATE_CHANGE`,
//!   a power-setting notification with a callback: no window needed), and
//!   once with its state when Yap starts watching. A desktop has no lid and
//!   is never told it's closed: nothing here ever applies to it.
//! - Closed: the laptop's own microphone (`mics::is_built_in`) goes last in
//!   the order (`mics::lid_rule`): the next one in it is used, or any other
//!   connected microphone. The dictation's mic and a meeting look again at
//!   once (`mics::lid_changed`), never mid-dictation.
//! - When the built-in one is all there is, a dictation starting says so,
//!   once per closing ([`tell_if_built_in`]): "Your laptop's lid is closed,
//!   so its microphone can't hear you", with **Microphones** (the picker in
//!   Settings → Dictation).
//!
//! Test mode reads no lid: `e2e_lid` stands in.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::OnceLock;

use tauri::AppHandle;

const UNKNOWN: u8 = 0;
const OPEN: u8 = 1;
const CLOSED: u8 = 2;

/// The lid as Windows last said it.
static LID: AtomicU8 = AtomicU8::new(UNKNOWN);
/// Told for this closing already.
static TOLD: AtomicBool = AtomicBool::new(false);
static APP: OnceLock<AppHandle> = OnceLock::new();

/// Is the laptop's lid closed? (Never on a desktop, nor before Windows has
/// said.)
pub fn closed() -> bool {
    LID.load(Ordering::SeqCst) == CLOSED
}

/// What Windows' notification's one byte says: 0 closed, 1 open. Pure.
pub fn state_of(data: u8) -> Option<bool> {
    match data {
        0 => Some(true),
        1 => Some(false),
        _ => None,
    }
}

/// The lid is now `closed` (or open): when that's news, the mics look again.
fn set(closed: bool) {
    let new = if closed { CLOSED } else { OPEN };
    let old = LID.swap(new, Ordering::SeqCst);
    if old == new {
        return;
    }
    TOLD.store(false, Ordering::SeqCst);
    tracing::info!(closed, "Laptop lid: {}", if closed { "closed" } else { "open" });
    crate::dictation::mics::lid_changed();
}

/// Whether a dictation starting on `target` should be told the lid is
/// closed: it's the laptop's own mic, the lid is closed, and this closing
/// hasn't been told yet. Pure.
pub fn should_tell(lid_closed: bool, built_in: bool, told: bool) -> bool {
    lid_closed && built_in && !told
}

/// A dictation is opening `target`: with the lid closed and the laptop's
/// own mic all there is, say so (once per closing), off this thread.
pub fn tell_if_built_in(target: Option<&crate::dictation::mics::Target>) {
    let built_in = target.and_then(|t| t.endpoint()).is_some_and(|e| e.built_in);
    if !should_tell(closed(), built_in, TOLD.load(Ordering::SeqCst)) {
        return;
    }
    TOLD.store(true, Ordering::SeqCst);
    let Some(app) = APP.get() else { return };
    tracing::info!("Laptop lid: closed, and its own microphone is the only one");
    crate::notices::alert::show_soon(
        app,
        crate::notices::alert::Alert {
            kind: "lidClosed",
            icon: "mic",
            title: "Your laptop's lid is closed, so its microphone can't hear you".into(),
            body: "Open the lid, or plug in a microphone and put it first in Microphones.".into(),
            action: Some(crate::notices::alert::Action { id: crate::notices::alert::OPEN_MIC_ORDER, label: "Microphones" }),
            timeout_ms: 12_000,
        },
    );
}

/// Start watching the lid (app setup). Test mode watches none.
pub fn start(app: &AppHandle) {
    let _ = APP.set(app.clone());
    if crate::shell::e2e::active() {
        return;
    }
    #[cfg(windows)]
    win::register();
}

#[cfg(windows)]
mod win {
    use windows::core::GUID;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Power::{PowerSettingRegisterNotification, DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, POWERBROADCAST_SETTING};
    use windows::Win32::UI::WindowsAndMessaging::{DEVICE_NOTIFY_CALLBACK, PBT_POWERSETTINGCHANGE};

    /// GUID_LIDSWITCH_STATE_CHANGE.
    const LID_SWITCH: GUID = GUID::from_u128(0xba3e0f4d_b817_4094_a2d1_d56379e6a0f3);

    /// Windows' callback (a thread of its own): quick, nothing waits.
    unsafe extern "system" fn on_power(_context: *const core::ffi::c_void, kind: u32, setting: *const core::ffi::c_void) -> u32 {
        if kind != PBT_POWERSETTINGCHANGE || setting.is_null() {
            return 0;
        }
        // SAFETY: for PBT_POWERSETTINGCHANGE Windows passes a
        // POWERBROADCAST_SETTING with `DataLength` bytes of data after it.
        let s = unsafe { &*(setting as *const POWERBROADCAST_SETTING) };
        if s.PowerSetting == LID_SWITCH && s.DataLength >= 1 {
            if let Some(closed) = super::state_of(s.Data[0]) {
                super::set(closed);
            }
        }
        0
    }

    pub(super) fn register() {
        // Kept for as long as Yap runs (Windows calls back with it).
        let params: &'static mut DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS =
            Box::leak(Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS { Callback: Some(on_power), Context: std::ptr::null_mut() }));
        let mut handle: *mut core::ffi::c_void = std::ptr::null_mut();
        // SAFETY: a plain registration; `params` lives for the process.
        let r = unsafe {
            PowerSettingRegisterNotification(
                &LID_SWITCH,
                DEVICE_NOTIFY_CALLBACK,
                HANDLE(params as *mut DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS as *mut core::ffi::c_void),
                &mut handle,
            )
        };
        if r.is_ok() {
            tracing::info!("Laptop lid: watching");
        } else {
            tracing::info!("Laptop lid: Windows won't say ({r:?})");
        }
    }
}

/// Test mode only: the lid closed or open (`null`: no lid, as a desktop).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_lid(closed: Option<bool>) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    match closed {
        Some(c) => set(c),
        None => {
            LID.store(UNKNOWN, Ordering::SeqCst);
            TOLD.store(false, Ordering::SeqCst);
            crate::dictation::mics::lid_changed();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_byte_says_closed_or_open() {
        assert_eq!(state_of(0), Some(true));
        assert_eq!(state_of(1), Some(false));
        assert_eq!(state_of(7), None);
    }

    #[test]
    fn only_the_built_in_mic_with_the_lid_closed_is_told_once() {
        assert!(should_tell(true, true, false));
        assert!(!should_tell(true, true, true), "once per closing");
        assert!(!should_tell(true, false, false), "a USB mic hears fine");
        assert!(!should_tell(false, true, false), "the lid's open");
    }

    /// No lid (a desktop): never closed, so the order stays as it is.
    #[test]
    fn a_desktop_has_no_lid() {
        assert!(!closed());
    }
}

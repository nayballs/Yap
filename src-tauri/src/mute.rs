//! System output mute while recording (Windows).
//!
//! When `mute_while_recording` is enabled, Yap mutes the default audio
//! render endpoint while a recording is in progress so playback (music,
//! videos) doesn't bleed into the mic or distract the user, then restores
//! the previous mute state when recording stops.
//!
//! Implemented over WASAPI/COM (`IMMDeviceEnumerator` → `IAudioEndpointVolume`)
//! via the `windows` crate — the same COM apartment handling as `selection.rs`.
//! We only ever unmute what *we* muted: if the endpoint was already muted by the
//! user we leave ownership alone, so stopping a recording never un-mutes audio
//! the user muted themselves.

use std::sync::atomic::{AtomicBool, Ordering};

/// Whether *we* muted the output (so we only unmute what we muted).
static MUTED_BY_US: AtomicBool = AtomicBool::new(false);

/// Mute the default render endpoint. Remembers that we did so.
pub fn mute_system_output() {
    if MUTED_BY_US.load(Ordering::SeqCst) {
        return; // already muted by us
    }
    #[cfg(windows)]
    match wasapi::set_mute(true) {
        // Endpoint was already muted (by the user) — don't claim ownership, so
        // we won't unmute their audio when recording stops.
        Some(true) => {}
        Some(false) => {
            MUTED_BY_US.store(true, Ordering::SeqCst);
            tracing::debug!("mute_while_recording: muted system output");
        }
        None => tracing::warn!("mute_while_recording: WASAPI mute failed"),
    }
    #[cfg(not(windows))]
    tracing::info!("mute_while_recording: not supported on this platform");
}

/// Restore the system output if we were the ones who muted it.
pub fn unmute_system_output() {
    if !MUTED_BY_US.swap(false, Ordering::SeqCst) {
        return; // we didn't mute it
    }
    #[cfg(windows)]
    {
        if wasapi::set_mute(false).is_none() {
            tracing::warn!("mute_while_recording: WASAPI unmute failed");
        } else {
            tracing::debug!("mute_while_recording: restored system output");
        }
    }
}

#[cfg(windows)]
mod wasapi {
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{
        eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    /// Resolve the default render endpoint's volume control. Returns `None` on
    /// any COM error (never panics).
    fn endpoint_volume() -> Option<IAudioEndpointVolume> {
        unsafe {
            // Initialize COM. Tolerate "already initialized" (S_FALSE) and a
            // differing apartment mode (RPC_E_CHANGED_MODE) — both mean COM is
            // usable; only bail on a hard error. Same pattern as selection.rs.
            let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
            if hr.is_err() {
                use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
                if hr != RPC_E_CHANGED_MODE {
                    return None;
                }
            }

            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
            let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
            device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None).ok()
        }
    }

    /// Set the default render endpoint's mute state. Returns the *previous* mute
    /// state on success, or `None` if the endpoint couldn't be reached.
    pub fn set_mute(mute: bool) -> Option<bool> {
        unsafe {
            let volume = endpoint_volume()?;
            let prev = volume.GetMute().ok()?.as_bool();
            volume.SetMute(mute, std::ptr::null()).ok()?;
            Some(prev)
        }
    }
}

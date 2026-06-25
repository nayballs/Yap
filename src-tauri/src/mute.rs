//! System output mute while recording (Windows).
//!
//! When `mute_while_recording` is enabled, Blip mutes the default audio
//! render endpoint while a recording is in progress so playback (music,
//! videos) doesn't bleed into the mic or distract the user, then restores
//! the previous mute state when recording stops.
//!
//! TODO: This is currently a logged no-op stub. A real implementation needs
//! WASAPI/COM (`IMMDeviceEnumerator` → `IAudioEndpointVolume`) via raw FFI or
//! the `windows` crate. It was stubbed to avoid pulling in heavy COM
//! boilerplate / a new dependency and risking the build; the config flag,
//! command plumbing and call sites are all wired so only this body needs to
//! be filled in.

use std::sync::atomic::{AtomicBool, Ordering};

/// Whether *we* muted the output (so we only unmute what we muted).
static MUTED_BY_US: AtomicBool = AtomicBool::new(false);

/// Mute the default render endpoint. Remembers that we did so.
pub fn mute_system_output() {
    if MUTED_BY_US.swap(true, Ordering::SeqCst) {
        return; // already muted by us
    }
    // TODO: real WASAPI mute. For now, log only.
    tracing::info!("mute_while_recording: would mute system output (stub no-op)");
}

/// Restore the system output if we were the ones who muted it.
pub fn unmute_system_output() {
    if !MUTED_BY_US.swap(false, Ordering::SeqCst) {
        return; // we didn't mute it
    }
    // TODO: real WASAPI unmute. For now, log only.
    tracing::info!("mute_while_recording: would unmute system output (stub no-op)");
}

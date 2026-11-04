//! Core Audio's device notifications (`IMMNotificationClient`): a
//! microphone plugged in or out, a Bluetooth headset connecting, Windows'
//! default input changing. The callbacks only nudge this module's thread
//! (Core Audio calls them on its own threads, and nothing may wait or call
//! back into it there); the thread waits for the burst of notifications a
//! device brings (added, state, default…) to settle, then reads the list
//! again (`mics::refresh`), which tells whoever cares.

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::time::Duration;

use windows::core::{implement, Result, PCWSTR};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::{
    eCapture, eConsole, EDataFlow, ERole, IMMNotificationClient, IMMNotificationClient_Impl, DEVICE_STATE,
};

/// The notifications a device brings come in a burst: read the list once
/// it has been quiet this long.
const SETTLE_MS: u64 = 400;

#[implement(IMMNotificationClient)]
struct Client {
    nudge: Sender<()>,
}

impl IMMNotificationClient_Impl for Client_Impl {
    fn OnDeviceStateChanged(&self, _id: &PCWSTR, _state: DEVICE_STATE) -> Result<()> {
        let _ = self.nudge.send(());
        Ok(())
    }

    fn OnDeviceAdded(&self, _id: &PCWSTR) -> Result<()> {
        let _ = self.nudge.send(());
        Ok(())
    }

    fn OnDeviceRemoved(&self, _id: &PCWSTR) -> Result<()> {
        let _ = self.nudge.send(());
        Ok(())
    }

    fn OnDefaultDeviceChanged(&self, flow: EDataFlow, role: ERole, _id: &PCWSTR) -> Result<()> {
        if flow == eCapture && role == eConsole {
            let _ = self.nudge.send(());
        }
        Ok(())
    }

    fn OnPropertyValueChanged(&self, _id: &PCWSTR, _key: &PROPERTYKEY) -> Result<()> {
        // Too chatty (levels, formats); a rename shows with the next change.
        Ok(())
    }
}

/// Read the list now and watch it from here on, on a thread of its own.
pub(super) fn spawn() {
    let spawned = std::thread::Builder::new().name("mic-devices".into()).spawn(|| {
        let _com = super::win::com();
        // The list as it is at start (no arrivals: these were here).
        super::refresh();
        let Some(enumerator) = super::win::enumerator() else {
            tracing::warn!("Microphones: Core Audio couldn't be asked; plugging one in is noticed at the next dictation");
            return;
        };
        let (nudge, nudged) = mpsc::channel::<()>();
        let client: IMMNotificationClient = Client { nudge }.into();
        // SAFETY: a plain COM call; the client stays registered (and alive)
        // for as long as this thread runs, which is as long as Yap does.
        if let Err(e) = unsafe { enumerator.RegisterEndpointNotificationCallback(&client) } {
            tracing::warn!("Microphones: no device notifications ({e})");
            return;
        }
        tracing::info!("Microphones: watching for devices coming and going");
        while nudged.recv().is_ok() {
            // The rest of the burst.
            loop {
                match nudged.recv_timeout(Duration::from_millis(SETTLE_MS)) {
                    Ok(()) => continue,
                    Err(RecvTimeoutError::Timeout) => break,
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
            super::refresh();
        }
    });
    if let Err(e) = spawned {
        tracing::warn!("Microphones: no thread to watch them on ({e})");
    }
}

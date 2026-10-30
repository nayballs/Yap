//! **COM's multithreaded apartment, kept for as long as Yap runs**
//! (2026-10-29).
//!
//! Yap uses Windows Runtime objects — its notifications, image decoding,
//! the OCR, media sessions, the network's cost — from threads that never
//! set COM up themselves. Such a thread borrows the process's multithreaded
//! apartment (the "implicit MTA"), if there is one. Other threads join the
//! MTA with `CoInitializeEx` and leave it with `CoUninitialize` (a UI
//! Automation read, the microphone watcher, a meeting's loopback). When the
//! last of those leaves, Windows takes the MTA down and unloads the DLLs
//! behind its objects — under a thread still using one, and under the
//! `windows` crate's cache of activation factories. The next notification
//! built then calls into unloaded code: an **access violation**, or "Cannot
//! create a WinRT object during host MTA shutdown". The Rust test suite hit
//! it now and then at full parallelism (`calendar`'s and `meeting_detect`'s
//! notification tests beside the UI Automation ones); in the app it was the
//! microphone watcher's thread that happened to keep the MTA up, as long as
//! its watch started.
//!
//! [`keep_mta`] holds one use of the MTA from the start until the process
//! ends (`CoIncrementMTAUsage`, never given back), as Windows advises for
//! code that relies on the implicit MTA: `run` calls it first thing, and a
//! test run before any test starts.

/// Keep the MTA up until the process ends (once; later calls do nothing).
pub fn keep_mta() {
    #[cfg(windows)]
    {
        static KEPT: std::sync::Once = std::sync::Once::new();
        KEPT.call_once(|| {
            // SAFETY: a plain COM call. The cookie is never given back
            // (`CoDecrementMTAUsage`), so the MTA lasts as long as the process.
            if let Err(e) = unsafe { windows::Win32::System::Com::CoIncrementMTAUsage() } {
                tracing::warn!("COM: couldn't keep the multithreaded apartment ({e})");
            }
        });
    }
}

/// A test run keeps it before the first test: the C runtime calls the
/// `.CRT$XCU` initialisers before `main`. (No test can come first and build
/// a notification on an apartment another test then takes down.)
#[cfg(all(test, windows))]
#[used]
#[link_section = ".CRT$XCU"]
static KEEP_MTA_IN_TESTS: extern "C" fn() = {
    extern "C" fn keep() {
        keep_mta();
    }
    keep
};

#[cfg(all(test, windows))]
mod tests {
    use windows::Win32::System::Com::{
        CoGetApartmentType, CoInitializeEx, CoUninitialize, APTTYPE, APTTYPEQUALIFIER, APTTYPE_MTA,
        APTTYPEQUALIFIER_IMPLICIT_MTA, COINIT_MULTITHREADED,
    };

    /// The apartment a thread that never set COM up is in, if any.
    fn borrowed_apartment() -> Option<(APTTYPE, APTTYPEQUALIFIER)> {
        std::thread::spawn(|| {
            let (mut kind, mut qualifier) = (APTTYPE::default(), APTTYPEQUALIFIER::default());
            // SAFETY: reads this thread's apartment; nothing is set up.
            unsafe { CoGetApartmentType(&mut kind, &mut qualifier) }.ok().map(|()| (kind, qualifier))
        })
        .join()
        .unwrap()
    }

    /// Threads join the MTA and leave it (as a UI Automation read does), all
    /// at once and one after another: a thread without COM of its own still
    /// finds it there afterwards (until 2026-10-29 the last one to leave
    /// took it down, under the notification tests).
    #[test]
    fn threads_leaving_com_never_take_the_apartment_away() {
        super::keep_mta();
        let leavers: Vec<_> = (0..8)
            .map(|_| {
                std::thread::spawn(|| {
                    // SAFETY: balanced on this thread.
                    unsafe {
                        if CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() {
                            CoUninitialize();
                        }
                    }
                })
            })
            .collect();
        for t in leavers {
            t.join().unwrap();
        }
        assert_eq!(borrowed_apartment(), Some((APTTYPE_MTA, APTTYPEQUALIFIER_IMPLICIT_MTA)));
    }
}

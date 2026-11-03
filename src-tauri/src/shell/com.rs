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

/// How often [`wait_within`] looks.
const LOOK_EVERY: std::time::Duration = std::time::Duration::from_millis(5);

/// **A WinRT operation waited for, at most `limit`** (2026-11-03): `done()`
/// — the operation's `Status()` past `Started` (`media_pause::answer!`) — is
/// looked at every few milliseconds, then `results()` (its `GetResults()`)
/// taken; `None`: not done by then (it carries on by itself, its answer
/// dropped). `.get()` waited for ever: a media session that never answered
/// (a hung browser tab, a busy music app) held the other apps' sound and the
/// next dictation's start (`media_pause.rs`). (`windows-future` 0.3, with
/// tauri 2.12, has a completion callback, `when`, and `IntoFuture`; not used
/// here yet.)
pub(crate) fn wait_within<T>(limit: std::time::Duration, mut done: impl FnMut() -> bool, results: impl FnOnce() -> T) -> Option<T> {
    let deadline = std::time::Instant::now() + limit;
    loop {
        if done() {
            return Some(results());
        }
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return None;
        }
        std::thread::sleep(left.min(LOOK_EVERY));
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

#[cfg(test)]
mod wait_tests {
    use super::wait_within;
    use std::time::{Duration, Instant};

    /// An operation done already answers at once; one that never is lets
    /// go at the limit (`.get()` waited for ever); one done from another
    /// thread is seen soon after.
    #[test]
    fn a_wait_ends_at_its_limit() {
        assert_eq!(wait_within(Duration::from_millis(50), || true, || 7), Some(7));
        let started = Instant::now();
        assert_eq!(wait_within(Duration::from_millis(150), || false, || 7), None);
        let took = started.elapsed();
        assert!(took >= Duration::from_millis(150) && took < Duration::from_secs(2), "{took:?}");
        let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let later = std::sync::Arc::clone(&done);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(40));
            later.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        let started = Instant::now();
        assert_eq!(wait_within(Duration::from_secs(5), || done.load(std::sync::atomic::Ordering::SeqCst), || true), Some(true));
        assert!(started.elapsed() < Duration::from_secs(2), "seen when it answered");
    }
}

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

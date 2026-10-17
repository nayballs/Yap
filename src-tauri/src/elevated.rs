//! **Apps running as administrator.** Windows doesn't let a process send
//! keys to a window of a higher integrity level (UIPI): Yap at the usual
//! Medium level can't press Ctrl+V in an elevated app (an admin PowerShell,
//! Task Manager, an installer…), and `SendInput` doesn't say so — the paste
//! just doesn't happen. So before a paste (a dictation's, "Paste last
//! transcript"), Yap compares the target window's process with its own
//! (`OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` → its token →
//! `TokenIntegrityLevel`). Higher: no keys are sent; the text is left on the
//! clipboard (as a dictation's paste puts it there: out of Win+V history,
//! but an ordinary entry Ctrl+V pastes anywhere) and the person is told
//! ([`alert`], through `alert.rs`): "Your text is copied: press Ctrl+V".
//!
//! When Windows won't even show Yap the target's token, its query
//! (`PROCESS_QUERY_INFORMATION`) being refused is the sign (as Wispr Flow
//! reads it): the target is above Yap. Anything Yap can't tell pastes as
//! before. The decision is pure ([`blocks_input`]) and unit-tested; a test
//! run reads no other process: the debug-only [`e2e_paste_elevated`] makes
//! every target count as an admin app (in robot mode the text really goes
//! on the clipboard and nothing is pasted). A real elevated window can't be
//! robot-tested: opening one takes a UAC prompt.

/// Integrity levels: the last sub-authority of a token's mandatory label
/// (Low 0x1000, Medium, High 0x3000 — an app run as administrator —, System).
pub const MEDIUM: u32 = 0x2000;
pub const SYSTEM: u32 = 0x4000;

/// What Yap could learn about a process's integrity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Its level.
    Known(u32),
    /// Windows refused even to let Yap query it: it's above Yap.
    Denied,
    /// Couldn't tell (it's gone, or something else failed).
    Unknown,
}

/// Would Windows drop keys Yap (at `own`, `None` if unreadable: taken as
/// Medium, the usual) sends to a process at `target`?
pub fn blocks_input(own: Option<u32>, target: Level) -> bool {
    let own = own.unwrap_or(MEDIUM);
    match target {
        Level::Known(level) => level > own,
        Level::Denied => own < SYSTEM,
        Level::Unknown => false,
    }
}

/// The notice: the text is on the clipboard, paste it yourself.
pub fn alert() -> crate::alert::Alert {
    crate::alert::Alert {
        kind: "pasteBlocked",
        icon: "clipboard",
        title: "Your text is copied: press Ctrl+V".to_string(),
        body: "Yap can't type into apps running as administrator, so your dictation is on the clipboard instead."
            .to_string(),
        action: None,
        timeout_ms: 12_000,
    }
}

/// Does the window `hwnd` belong to an app running above Yap (keys sent to
/// it would be dropped)? A test run reads no other process: only the
/// override counts ([`e2e_paste_elevated`]).
pub fn target_blocked(hwnd: isize) -> bool {
    if crate::e2e::active() {
        return test_override();
    }
    #[cfg(windows)]
    {
        let Some(pid) = crate::text_injector::window_process(hwnd) else {
            return false;
        };
        if pid == std::process::id() {
            return false;
        }
        let target = win::process_level(pid);
        let own = win::own_level();
        let blocked = blocks_input(own, target);
        if blocked {
            tracing::info!(?own, ?target, "The target app runs at a higher integrity level than Yap");
        }
        blocked
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        false
    }
}

#[cfg(debug_assertions)]
static TEST_ELEVATED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The e2e override: every paste target counts as an admin app.
pub fn test_override() -> bool {
    #[cfg(debug_assertions)]
    {
        crate::e2e::active() && TEST_ELEVATED.load(std::sync::atomic::Ordering::SeqCst)
    }
    #[cfg(not(debug_assertions))]
    false
}

/// Test mode only: every paste target counts as an app running as
/// administrator (`on`), or none does. Plain test mode then pastes nothing
/// and touches no clipboard (as always) but says what a blocked paste says;
/// robot mode also leaves the text on the clipboard, for real.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_paste_elevated(on: bool) -> Result<(), String> {
    if !crate::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    tracing::info!(on, "e2e: paste targets count as admin apps");
    TEST_ELEVATED.store(on, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

#[cfg(windows)]
#[allow(clippy::upper_case_acronyms)]
mod win {
    use std::ffi::c_void;

    use super::Level;

    type HANDLE = *mut c_void;

    // Declared as text_injector.rs declares them (`clashing_extern_declarations`).
    extern "system" {
        fn OpenProcess(desired_access: u32, inherit: i32, pid: u32) -> HANDLE;
        fn CloseHandle(h: HANDLE) -> i32;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> HANDLE;
        fn GetLastError() -> u32;
    }
    #[link(name = "advapi32")]
    extern "system" {
        fn OpenProcessToken(process: HANDLE, access: u32, token: *mut HANDLE) -> i32;
        fn GetTokenInformation(token: HANDLE, class: u32, info: *mut c_void, len: u32, ret: *mut u32) -> i32;
        fn GetSidSubAuthorityCount(sid: *mut c_void) -> *mut u8;
        fn GetSidSubAuthority(sid: *mut c_void, index: u32) -> *mut u32;
    }

    const PROCESS_QUERY_INFORMATION: u32 = 0x0400;
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const TOKEN_QUERY: u32 = 0x0008;
    /// TOKEN_INFORMATION_CLASS::TokenIntegrityLevel.
    const TOKEN_INTEGRITY_LEVEL: u32 = 25;
    const ERROR_ACCESS_DENIED: u32 = 5;

    /// A token's integrity level (its mandatory label's last sub-authority).
    unsafe fn token_level(token: HANDLE) -> Option<u32> {
        // TOKEN_MANDATORY_LABEL: a SID_AND_ATTRIBUTES whose SID follows it
        // in the same buffer (8-byte aligned).
        let mut buf = [0u64; 16];
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TOKEN_INTEGRITY_LEVEL,
            buf.as_mut_ptr() as *mut c_void,
            std::mem::size_of_val(&buf) as u32,
            &mut len,
        );
        if ok == 0 {
            return None;
        }
        let sid = *(buf.as_ptr() as *const *mut c_void);
        if sid.is_null() {
            return None;
        }
        let count = *GetSidSubAuthorityCount(sid);
        if count == 0 {
            return None;
        }
        Some(*GetSidSubAuthority(sid, u32::from(count) - 1))
    }

    /// Yap's own integrity level.
    pub fn own_level() -> Option<u32> {
        unsafe {
            let mut token: HANDLE = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return None;
            }
            let level = token_level(token);
            CloseHandle(token);
            level
        }
    }

    /// The integrity level of process `pid`, as far as Windows lets Yap see.
    pub fn process_level(pid: u32) -> Level {
        unsafe {
            let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if process.is_null() {
                return if GetLastError() == ERROR_ACCESS_DENIED { Level::Denied } else { Level::Unknown };
            }
            let mut token: HANDLE = std::ptr::null_mut();
            let opened = OpenProcessToken(process, TOKEN_QUERY, &mut token) != 0;
            let error = if opened { 0 } else { GetLastError() };
            CloseHandle(process);
            if !opened {
                // No look at its token: is even a full query refused? A
                // process above Yap refuses it (Windows' no-read-up rule).
                if error != ERROR_ACCESS_DENIED {
                    return Level::Unknown;
                }
                let full = OpenProcess(PROCESS_QUERY_INFORMATION, 0, pid);
                if full.is_null() {
                    return if GetLastError() == ERROR_ACCESS_DENIED { Level::Denied } else { Level::Unknown };
                }
                CloseHandle(full);
                return Level::Unknown;
            }
            let level = token_level(token);
            CloseHandle(token);
            level.map_or(Level::Unknown, Level::Known)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HIGH: u32 = 0x3000;

    #[test]
    fn a_higher_level_blocks_keys() {
        assert!(blocks_input(Some(MEDIUM), Level::Known(HIGH)));
        assert!(blocks_input(Some(MEDIUM), Level::Known(SYSTEM)));
        assert!(blocks_input(Some(HIGH), Level::Known(SYSTEM)));
        // Unreadable own level: the usual Medium.
        assert!(blocks_input(None, Level::Known(HIGH)));
    }

    #[test]
    fn the_same_or_a_lower_level_takes_keys() {
        assert!(!blocks_input(Some(MEDIUM), Level::Known(MEDIUM)));
        assert!(!blocks_input(Some(HIGH), Level::Known(HIGH)));
        assert!(!blocks_input(Some(HIGH), Level::Known(MEDIUM)));
        // Medium Plus (0x2100) is above Medium, though.
        assert!(blocks_input(Some(MEDIUM), Level::Known(0x2100)));
        // A low-integrity app (a sandbox) is below Yap.
        assert!(!blocks_input(Some(MEDIUM), Level::Known(0x1000)));
    }

    #[test]
    fn a_refused_query_means_above_yap_and_not_knowing_pastes_as_before() {
        assert!(blocks_input(Some(MEDIUM), Level::Denied));
        assert!(blocks_input(Some(HIGH), Level::Denied));
        // Yap as SYSTEM is above everything (never in practice).
        assert!(!blocks_input(Some(SYSTEM), Level::Denied));
        assert!(!blocks_input(Some(MEDIUM), Level::Unknown));
        assert!(!blocks_input(None, Level::Unknown));
    }

    #[test]
    fn the_notice_says_the_text_is_copied() {
        let a = alert();
        assert!(a.title.contains("Ctrl+V"));
        assert!(a.body.contains("administrator"));
        assert_eq!(a.kind, "pasteBlocked");
    }

    /// Yap's own token reads, and its own process isn't above itself.
    #[cfg(windows)]
    #[test]
    fn yap_reads_its_own_level() {
        let own = win::own_level().expect("our own integrity level");
        assert!(own >= MEDIUM, "{own:#x}");
        assert_eq!(win::process_level(std::process::id()), Level::Known(own));
        assert!(!blocks_input(Some(own), win::process_level(std::process::id())));
        // A process id nobody has (they're multiples of 4): unknown, so it pastes.
        assert!(!blocks_input(Some(own), win::process_level(3)));
    }
}

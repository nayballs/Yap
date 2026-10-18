//! Process helpers that let a dev build and an installed Yap run side by side:
//! a sidecar is only an orphan when the Yap that spawned it is gone.

/// Live `exe_name` processes whose parent is no longer a running Yap (it
/// crashed, was force-killed, or the updater exited without cleanup).
/// Another Yap's live sidecar is left alone.
#[cfg(windows)]
pub fn orphaned(exe_name: &str) -> Vec<u32> {
    imp::snapshot()
        .into_iter()
        .filter(|p| p.exe.eq_ignore_ascii_case(exe_name) && !is_running_yap(p.parent))
        .map(|p| p.pid)
        .collect()
}

/// Whether `pid` is a live process whose executable is a Yap build
/// (`yap.exe`, or a renamed copy such as the updater's `yap-old.exe`).
#[cfg(windows)]
pub fn is_running_yap(pid: u32) -> bool {
    imp::live_image_name(pid).is_some_and(|name| {
        let name = name.to_ascii_lowercase();
        name.starts_with("yap") && name.ends_with(".exe")
    })
}

#[cfg(windows)]
pub fn kill(pid: u32) -> bool {
    imp::kill(pid)
}

/// Every running process's exe file name, lowercased, by process id (one
/// snapshot: what "Silence other apps" checks audio sessions against).
#[cfg(windows)]
pub fn exe_names() -> std::collections::HashMap<u32, String> {
    imp::snapshot().into_iter().map(|p| (p.pid, p.exe.to_ascii_lowercase())).collect()
}

/// When the live process `pid` started (a FILETIME, 100 ns since 1601), or
/// `None` (gone, or not ours to ask). Tells a process from a later one that
/// was given the same id.
#[cfg(windows)]
#[cfg_attr(not(debug_assertions), allow(dead_code))] // robot mode (realtest.rs) only
pub fn created(pid: u32) -> Option<u64> {
    imp::created(pid)
}

/// `pid`, then its parent, grandparent… with when each started, for as long
/// as the chain can be followed: a parent that's gone, that can't be asked,
/// or that started after its "child" (its id was reused) ends it. At most
/// 32 steps.
#[cfg(windows)]
#[cfg_attr(not(debug_assertions), allow(dead_code))] // robot mode (realtest.rs) only
pub fn lineage(pid: u32) -> Vec<(u32, u64)> {
    let parents: std::collections::HashMap<u32, u32> =
        imp::snapshot().into_iter().map(|p| (p.pid, p.parent)).collect();
    let mut out: Vec<(u32, u64)> = Vec::new();
    let mut current = pid;
    while out.len() < 32 && current != 0 {
        let Some(started) = imp::created(current) else {
            break;
        };
        if out.last().is_some_and(|&(_, child)| started > child) {
            break;
        }
        out.push((current, started));
        match parents.get(&current) {
            Some(&parent) if parent != current => current = parent,
            _ => break,
        }
    }
    out
}

/// Now, as a FILETIME (100 ns since 1601), to compare with [`created`].
#[cfg(windows)]
pub fn now_filetime() -> u64 {
    const UNIX_EPOCH_AS_FILETIME: u64 = 116_444_736_000_000_000;
    let since_1970 = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    UNIX_EPOCH_AS_FILETIME + (since_1970.as_nanos() / 100) as u64
}

/// `pid` and the live processes it started, and those they started… —
/// only ones that started at `since` (a FILETIME) or later, so a process
/// whose parent's id was reused never counts. One snapshot, at most 32.
/// An app whose launcher hands over to another process (World of Warcraft's
/// launcher → Battle.net) is found by its family.
#[cfg(windows)]
pub fn family(pid: u32, since: u64) -> Vec<u32> {
    let procs = imp::snapshot();
    let mut out = vec![pid];
    let mut i = 0;
    while i < out.len() && out.len() < 32 {
        let parent = out[i];
        for p in procs.iter().filter(|p| p.parent == parent && p.pid != parent && p.pid != 0) {
            if !out.contains(&p.pid) && imp::created(p.pid).is_some_and(|t| t >= since) {
                out.push(p.pid);
            }
        }
        i += 1;
    }
    out
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, QueryFullProcessImageNameW, TerminateProcess, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    };

    pub struct Proc {
        pub pid: u32,
        pub parent: u32,
        pub exe: String,
    }

    /// Every running process (pid, parent pid, exe file name).
    pub fn snapshot() -> Vec<Proc> {
        let mut out = Vec::new();
        unsafe {
            let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
                return out;
            };
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            let mut more = Process32FirstW(snap, &mut entry).is_ok();
            while more {
                let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                out.push(Proc {
                    pid: entry.th32ProcessID,
                    parent: entry.th32ParentProcessID,
                    exe: String::from_utf16_lossy(&entry.szExeFile[..len]),
                });
                more = Process32NextW(snap, &mut entry).is_ok();
            }
            let _ = CloseHandle(snap);
        }
        out
    }

    /// Exe file name of `pid` if it is still running (an exited process can
    /// linger while handles to it are open, so check the exit code too).
    pub fn live_image_name(pid: u32) -> Option<String> {
        if pid == 0 {
            return None;
        }
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut code = 0u32;
            let alive = GetExitCodeProcess(handle, &mut code).is_ok() && code == STILL_ACTIVE.0 as u32;
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let named = QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
            .is_ok();
            let _ = CloseHandle(handle);
            if !alive || !named {
                return None;
            }
            let full = String::from_utf16_lossy(&buf[..len as usize]);
            full.rsplit(['\\', '/']).next().map(str::to_string)
        }
    }

    pub fn created(pid: u32) -> Option<u64> {
        use windows::Win32::Foundation::FILETIME;
        use windows::Win32::System::Threading::GetProcessTimes;
        if pid == 0 {
            return None;
        }
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let (mut created, mut exited, mut kernel, mut user) =
                (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
            let ok = GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user).is_ok();
            let mut code = 0u32;
            let alive = GetExitCodeProcess(handle, &mut code).is_ok() && code == STILL_ACTIVE.0 as u32;
            let _ = CloseHandle(handle);
            (ok && alive).then(|| (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
        }
    }

    pub fn kill(pid: u32) -> bool {
        unsafe {
            let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, pid) else {
                return false;
            };
            let killed = TerminateProcess(handle, 1).is_ok();
            let _ = CloseHandle(handle);
            killed
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn sees_this_process_and_its_parent() {
        let me = std::process::id();
        let procs = imp::snapshot();
        let mine = procs.iter().find(|p| p.pid == me).expect("own process in snapshot");
        assert!(mine.exe.to_ascii_lowercase().ends_with(".exe"));
        assert!(imp::live_image_name(me).is_some());
        // The test runner isn't Yap, so nothing it spawned would count as Yap's.
        assert!(!is_running_yap(0));
    }

    #[test]
    fn follows_this_process_up_to_its_parent() {
        let me = std::process::id();
        let chain = lineage(me);
        assert_eq!(chain.first().map(|&(pid, _)| pid), Some(me));
        assert_eq!(created(me), Some(chain[0].1));
        // Every step started no later than the one below it.
        assert!(chain.windows(2).all(|w| w[1].1 <= w[0].1));
        assert_eq!(created(0), None);
    }

    #[test]
    fn no_orphans_of_a_name_nobody_runs() {
        assert!(orphaned("definitely-not-running-yap-test.exe").is_empty());
    }
}

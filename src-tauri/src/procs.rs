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
    fn no_orphans_of_a_name_nobody_runs() {
        assert!(orphaned("definitely-not-running-yap-test.exe").is_empty());
    }
}

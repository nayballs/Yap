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

/// Tie a helper program Yap started (the on-device AI model's llamafile, the
/// speaker engine, eSpeak) to this Yap, so it never outlives it: it goes in a
/// job object that only Yap holds, set to end everything in it when it
/// closes, and Windows closes it as Yap's process ends however it ends —
/// Quit, a crash, End task, the updater's exit. (`shutdown_cleanup` stops
/// them politely first on a normal quit; this is for every other way out.)
/// Programs the helper starts are in the job too. Only helpers: an app Yap
/// opens for you, a link's browser or an installer is never put in it.
/// Best-effort: when Windows refuses (logged), the startup orphan sweep
/// (`local_llm::kill_orphans`) is the fallback.
#[cfg(windows)]
pub fn end_with_yap(child: &std::process::Child) {
    use std::os::windows::io::AsRawHandle;
    let Some(job) = imp::yap_job() else { return };
    if let Err(e) = imp::assign(job, child.as_raw_handle()) {
        tracing::warn!("couldn't tie helper process {} to Yap: {e}", child.id());
    }
}
#[cfg(not(windows))]
pub fn end_with_yap(_child: &std::process::Child) {}

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

    use std::ffi::c_void;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// A new job that ends every process in it when its last handle closes.
    pub fn kill_on_close_job() -> windows::core::Result<HANDLE> {
        unsafe {
            let job = CreateJobObjectW(None, windows::core::PCWSTR::null())?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if let Err(e) = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) {
                let _ = CloseHandle(job);
                return Err(e);
            }
            Ok(job)
        }
    }

    /// Yap's own job for its helpers, made once and never closed (Windows
    /// closes it with the process). The handle is kept as a number: HANDLE
    /// isn't Sync.
    pub fn yap_job() -> Option<HANDLE> {
        static JOB: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
        let raw = *JOB.get_or_init(|| match kill_on_close_job() {
            Ok(job) => Some(job.0 as usize),
            Err(e) => {
                tracing::warn!("couldn't make the job that ends Yap's helpers with it: {e}");
                None
            }
        });
        raw.map(|h| HANDLE(h as *mut c_void))
    }

    pub fn assign(job: HANDLE, process: std::os::windows::io::RawHandle) -> windows::core::Result<()> {
        unsafe { AssignProcessToJobObject(job, HANDLE(process)) }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};

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

    /// A long-running stand-in helper (ping, 30 s), no window, no output.
    fn helper() -> std::process::Child {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("ping.exe")
            .args(["-n", "30", "127.0.0.1"])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(0x0800_0000)
            .spawn()
            .expect("ping starts")
    }

    fn ends_within(child: &mut std::process::Child, secs: u64) -> bool {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(secs);
        while std::time::Instant::now() < until {
            if child.try_wait().ok().flatten().is_some() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        false
    }

    /// What happens to a helper when Yap's process ends (any way): its job's
    /// last handle closes and Windows ends it. Shown with a job of its own,
    /// so the test runner doesn't have to die to prove it.
    #[test]
    fn a_helper_ends_when_its_job_closes() {
        use std::os::windows::io::AsRawHandle;
        let job = imp::kill_on_close_job().expect("job");
        let mut child = helper();
        imp::assign(job, child.as_raw_handle()).expect("assigned");
        assert!(!ends_within(&mut child, 0), "still running before the job closes");
        unsafe {
            let _ = CloseHandle(job);
        }
        assert!(ends_within(&mut child, 5), "the helper ended with its job");
    }

    #[test]
    fn helpers_go_in_yaps_job() {
        use std::os::windows::io::AsRawHandle;
        use windows::Win32::System::JobObjects::IsProcessInJob;
        let mut child = helper();
        end_with_yap(&child);
        let job = imp::yap_job().expect("Yap's job");
        let mut inside = windows::core::BOOL(0);
        unsafe { IsProcessInJob(HANDLE(child.as_raw_handle()), Some(job), &mut inside) }.expect("asked");
        assert!(inside.as_bool());
        let _ = child.kill();
        let _ = child.wait();
    }

    /// The stand-in Yap for the test below: run as its own process, it ties
    /// a helper to itself, says the helper's pid and waits to be ended.
    /// (A no-op in a normal test run.)
    #[test]
    fn stand_in_yap() {
        if std::env::var_os("YAP_STAND_IN_YAP").is_none() {
            return;
        }
        let mut child = helper();
        end_with_yap(&child);
        println!("helper-pid={}", child.id());
        std::thread::sleep(std::time::Duration::from_secs(60));
        // (Ended before this, by the test.)
        let _ = child.kill();
        let _ = child.wait();
    }

    /// Yap ended the hard way (End task, a crash): its helper goes with it,
    /// no clean-up code needed.
    #[test]
    fn a_helper_ends_when_yap_is_ended() {
        use std::io::BufRead;
        let mut yap = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["procs::tests::stand_in_yap", "--exact", "--nocapture", "--test-threads=1"])
            .env("YAP_STAND_IN_YAP", "1")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("stand-in Yap starts");
        let out = std::io::BufReader::new(yap.stdout.take().unwrap());
        let pid: u32 = out
            .lines()
            .map_while(Result::ok)
            .find_map(|l| l.split("helper-pid=").nth(1).and_then(|p| p.trim().parse().ok()))
            .expect("the stand-in says its helper's pid");
        assert!(imp::live_image_name(pid).is_some(), "the helper runs while Yap does");
        yap.kill().expect("Yap ended (TerminateProcess, as End task)");
        let _ = yap.wait();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while imp::live_image_name(pid).is_some() && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if imp::live_image_name(pid).is_some() {
            kill(pid);
            panic!("the helper outlived Yap");
        }
    }
}

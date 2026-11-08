//! **A terminal the person owns** (an agent's Open in terminal, Claude Code's
//! sign-in): Windows Terminal, else Windows PowerShell in a console of its
//! own, running a short PowerShell script ([`script`]: the title, the
//! folder, then Claude Code). The script goes as `-EncodedCommand`, so no
//! folder name, path or argument crosses a command-line parser on the way.
//!
//! - **Windows Terminal**: `wt.exe` on `PATH`, else its App Execution Alias
//!   in `%LOCALAPPDATA%\Microsoft\WindowsApps` ([`find_wt`]): a `PATH` that
//!   lost that folder still finds it. A new window, `--title`, `-d <folder>`,
//!   `;` escaped in both (`\;`: wt's own command separator).
//! - **A console of its own**: every program is started with `CreateProcessW`
//!   and none of Yap's handles ([`start`]: no `STARTF_USESTDHANDLES`).
//!   Rust's `Command` always hands a child Yap's stdin, stdout and stderr,
//!   console or not (a pipe to the dev launcher, nothing in a release
//!   build), so PowerShell and Claude Code read an empty input and wrote
//!   nowhere: a blank window that closed.
//! - **Outside Yap**: never in Yap's kill-on-close job (that only holds the
//!   helpers `shell/procs.rs` ties to it), and out of any job Yap itself was
//!   started in (`CREATE_BREAKAWAY_FROM_JOB`: a dev build sits in `cargo
//!   run`'s, which lets it go). A job that allows no breakaway keeps it,
//!   said in the log: then it closes with that Yap.
//! - Yap's cleaned environment ([`super::clean_env`]).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use base64::Engine as _;

use super::{batch_safe, is_program, quote_for_cmd, shim_refused, which, Program};

/// Where a terminal opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Route {
    WindowsTerminal,
    PowerShell,
}

/// `program args` in a new terminal window in `cwd` titled `title`:
/// Windows Terminal, else PowerShell.
pub(super) fn open(program: &Program, cwd: &Path, args: &[&str], title: &str) -> Result<Route, String> {
    let script = script(program, cwd, args, title)?;
    let powershell = powershell();
    let wt = find_wt(
        std::env::var_os("PATH").as_deref(),
        std::env::var_os("PATHEXT").as_deref(),
        dirs::data_local_dir().as_deref(),
    );
    match wt {
        Some(wt) => match start(&wt_launch(&wt, &powershell, cwd, title, &script), cwd, title) {
            Ok(s) => {
                s.log("Windows Terminal");
                return Ok(Route::WindowsTerminal);
            }
            Err(e) => tracing::warn!("claude code: Windows Terminal didn't start ({e}); trying PowerShell"),
        },
        None => tracing::info!("claude code: no Windows Terminal; opening PowerShell"),
    }
    let s = start(&powershell_launch(&powershell, &script), cwd, title)?;
    s.log("PowerShell");
    Ok(Route::PowerShell)
}

/// Windows Terminal's `wt.exe`: the first on `path`, else its App Execution
/// Alias in `local_app_data\Microsoft\WindowsApps` (where the Store install
/// puts it, whether or not `PATH` still lists that folder).
pub(super) fn find_wt(path: Option<&OsStr>, pathext: Option<&OsStr>, local_app_data: Option<&Path>) -> Option<PathBuf> {
    path.and_then(|p| which("wt", p, pathext)).or_else(|| {
        let alias = local_app_data?.join("Microsoft").join("WindowsApps").join("wt.exe");
        is_program(&alias).then_some(alias)
    })
}

/// Windows PowerShell (5.1, on every Windows 10 and 11), by its full path.
fn powershell() -> PathBuf {
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    root.join("System32").join("WindowsPowerShell").join("v1.0").join("powershell.exe")
}

/// `s` in PowerShell's single quotes: each quote doubled, the curly ones
/// too (PowerShell reads ‘ ’ ‚ ‛ as quotes), so nothing in it is code.
pub(super) fn ps_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            out.push(c);
        }
        out.push(c);
    }
    out.push('\'');
    out
}

/// The PowerShell script the terminal runs: its title, `Set-Location` to
/// `cwd`, then Claude Code with its arguments. An exe is called with each
/// argument in single quotes; an npm shim goes through `cmd /d /s /c` word
/// for word after `--%` (PowerShell passes the rest of the line as it is),
/// and only with what cmd reads as plain text ([`batch_safe`]).
pub(super) fn script(program: &Program, cwd: &Path, args: &[&str], title: &str) -> Result<String, String> {
    let mut s = format!(
        "$Host.UI.RawUI.WindowTitle = {}\nSet-Location -LiteralPath {}\n",
        ps_quote(title),
        ps_quote(&cwd.to_string_lossy())
    );
    let all = program.prefix.iter().map(String::as_str).chain(args.iter().copied());
    if program.is_shim() {
        let path = program.path().to_str().filter(|p| batch_safe(p)).ok_or_else(shim_refused)?;
        let mut line = format!("\"{}", quote_for_cmd(path));
        for arg in all {
            if !batch_safe(arg) {
                return Err(shim_refused());
            }
            line.push(' ');
            line.push_str(&quote_for_cmd(arg));
        }
        line.push('"');
        s.push_str("& cmd.exe --% /d /s /c ");
        s.push_str(&line);
    } else {
        s.push_str("& ");
        s.push_str(&ps_quote(&program.path().to_string_lossy()));
        for arg in all {
            s.push(' ');
            s.push_str(&ps_quote(arg));
        }
    }
    s.push('\n');
    Ok(s)
}

/// `script` as PowerShell's `-EncodedCommand` takes it: UTF-16LE, base64.
pub(super) fn encode(script: &str) -> String {
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// A program to start: its full path, its arguments, and whether it needs a
/// console of its own (a console program does; Windows Terminal is a window).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Launch {
    pub app: PathBuf,
    pub args: Vec<String>,
    pub console: bool,
}

/// `;` as wt reads it literally (`\;`); a lone `;` starts its next command.
fn wt_escape(s: &str) -> String {
    s.replace(';', "\\;")
}

/// Windows Terminal: a new window titled `title`, starting in `cwd`, on
/// PowerShell running `script`.
pub(super) fn wt_launch(wt: &Path, powershell: &Path, cwd: &Path, title: &str, script: &str) -> Launch {
    let args = [
        "-w".to_string(),
        "new".into(),
        "--title".into(),
        wt_escape(title),
        "-d".into(),
        wt_escape(&cwd.to_string_lossy()),
        powershell.to_string_lossy().into_owned(),
        "-NoExit".into(),
        "-NoProfile".into(),
        "-EncodedCommand".into(),
        encode(script),
    ];
    Launch { app: wt.to_path_buf(), args: args.into(), console: false }
}

/// PowerShell in a console of its own, running `script` and staying open
/// (`-NoExit`: Claude Code's last words stay readable, and the person has a
/// prompt in the folder after it).
pub(super) fn powershell_launch(powershell: &Path, script: &str) -> Launch {
    let args = ["-NoExit".to_string(), "-NoProfile".into(), "-EncodedCommand".into(), encode(script)];
    Launch { app: powershell.to_path_buf(), args: args.into(), console: true }
}

/// One argument as a Windows program reads it back (`CommandLineToArgvW`,
/// the MSVC runtime): in quotes when it has a space, a tab or a quote, or
/// is empty; a quote's backslashes doubled and the quote escaped, and the
/// backslashes before the closing quote doubled.
pub(super) fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '\n', '\x0b', '"']) {
        return arg.to_string();
    }
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                backslashes = 0;
                out.push('"');
            }
            _ => {
                out.extend(std::iter::repeat_n('\\', backslashes));
                backslashes = 0;
                out.push(c);
            }
        }
    }
    out.extend(std::iter::repeat_n('\\', backslashes * 2));
    out.push('"');
    out
}

/// The whole command line: the program's path in quotes, then each argument.
pub(super) fn command_line(launch: &Launch) -> String {
    let mut line = format!("\"{}\"", launch.app.to_string_lossy());
    for arg in &launch.args {
        line.push(' ');
        line.push_str(&quote_arg(arg));
    }
    line
}

/// An environment block for `CreateProcessW`: `name=value` each ending in a
/// NUL, sorted by name ignoring case (as Windows keeps them), one more NUL
/// at the end.
pub(super) fn env_block(vars: &[(std::ffi::OsString, std::ffi::OsString)]) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    let mut sorted: Vec<&(std::ffi::OsString, std::ffi::OsString)> = vars.iter().collect();
    sorted.sort_by_cached_key(|(k, _)| k.to_string_lossy().to_uppercase());
    let mut block = Vec::new();
    for (k, v) in sorted {
        block.extend(k.encode_wide());
        block.push(u16::from(b'='));
        block.extend(v.encode_wide());
        block.push(0);
    }
    if block.is_empty() {
        block.push(0);
    }
    block.push(0);
    block
}

/// A program [`start`] started.
#[derive(Debug)]
pub(super) struct Started {
    pub pid: u32,
    /// Still in the job Yap was started in: it allows no breakaway.
    pub in_yaps_job: bool,
}

impl Started {
    fn log(&self, opened: &str) {
        if self.in_yaps_job {
            tracing::info!(
                pid = self.pid,
                "claude code: a terminal opened in {opened}, inside the job Yap was started in (it allows no breakaway: it closes with this Yap)"
            );
        } else {
            tracing::info!(pid = self.pid, "claude code: a terminal opened in {opened}");
        }
    }
}

/// Start `launch` in `cwd` with Yap's cleaned environment and none of Yap's
/// handles: a console program gets a console of its own (titled `title`)
/// and that console's own input and output. Out of the job Yap was started
/// in when that job allows it, else in it.
pub(super) fn start(launch: &Launch, cwd: &Path, title: &str) -> Result<Started, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED};
    use windows::Win32::System::Threading::{
        CreateProcessW, CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_CONSOLE, CREATE_UNICODE_ENVIRONMENT, PROCESS_CREATION_FLAGS,
        PROCESS_INFORMATION, STARTUPINFOW,
    };

    let wide = |s: &OsStr| -> Vec<u16> { s.encode_wide().chain(Some(0)).collect() };
    let app = wide(launch.app.as_os_str());
    let dir = wide(cwd.as_os_str());
    let env = env_block(&super::clean_env(std::env::vars_os()));
    let mut flags = CREATE_UNICODE_ENVIRONMENT;
    if launch.console {
        flags |= CREATE_NEW_CONSOLE;
    }
    let create = |flags: PROCESS_CREATION_FLAGS| -> windows::core::Result<PROCESS_INFORMATION> {
        // CreateProcessW may write to the command line and the title: fresh
        // buffers for each try.
        let mut line: Vec<u16> = command_line(launch).encode_utf16().chain(Some(0)).collect();
        let mut caption: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
        // No STARTF_USESTDHANDLES: nothing of Yap's for its input and output.
        let si = STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOW>() as u32,
            lpTitle: if launch.console { PWSTR(caption.as_mut_ptr()) } else { PWSTR::null() },
            ..Default::default()
        };
        let mut pi = PROCESS_INFORMATION::default();
        // SAFETY: every buffer is NUL-terminated (the environment block
        // double) and outlives the call; `si` and `pi` are valid.
        unsafe {
            CreateProcessW(
                PCWSTR(app.as_ptr()),
                Some(PWSTR(line.as_mut_ptr())),
                None,
                None,
                false,
                flags,
                Some(env.as_ptr().cast()),
                PCWSTR(dir.as_ptr()),
                &si,
                &mut pi,
            )?;
        }
        Ok(pi)
    };
    let (pi, in_yaps_job) = match create(flags | CREATE_BREAKAWAY_FROM_JOB) {
        Ok(pi) => (pi, false),
        // Yap is in a job that allows no breakaway.
        Err(e) if e.code() == ERROR_ACCESS_DENIED.to_hresult() => (create(flags).map_err(|e| e.message())?, true),
        Err(e) => return Err(e.message()),
    };
    // SAFETY: the handles CreateProcessW gave us, closed once.
    unsafe {
        let _ = CloseHandle(pi.hThread);
        let _ = CloseHandle(pi.hProcess);
    }
    Ok(Started { pid: pi.dwProcessId, in_yaps_job })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    fn temp_dir(tag: &str) -> PathBuf {
        let mut raw = [0u8; 6];
        getrandom::fill(&mut raw).unwrap();
        let hex: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        let dir = std::env::temp_dir().join(format!("yap-cc-term-{tag}-{hex}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn decode(b64: &str) -> String {
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64).unwrap();
        let units: Vec<u16> = bytes.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16(&units).unwrap()
    }

    #[test]
    fn quotes_hold_whatever_a_name_has() {
        assert_eq!(ps_quote("plain"), "'plain'");
        assert_eq!(ps_quote("Sam's game"), "'Sam''s game'");
        assert_eq!(ps_quote("Sam\u{2019}s game"), "'Sam\u{2019}\u{2019}s game'");
        assert_eq!(ps_quote("$(Remove-Item x); `n \"q\""), "'$(Remove-Item x); `n \"q\"'");

        assert_eq!(quote_arg("plain"), "plain");
        assert_eq!(quote_arg(""), "\"\"");
        assert_eq!(quote_arg("a b"), "\"a b\"");
        assert_eq!(quote_arg(r"C:\a b\"), r#""C:\a b\\""#);
        assert_eq!(quote_arg(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(quote_arg(r#"a\"b c"#), r#""a\\\"b c""#);
        assert_eq!(quote_arg(r"C:\x\y"), r"C:\x\y", "no space: as it is");
    }

    #[test]
    fn the_script_runs_claude_code_in_the_folder() {
        let program = Program::new(PathBuf::from(r"C:\Users\Sam O'Neil\.local\bin\claude.exe"));
        let cwd = Path::new(r"D:\Program Files (x86)\Some Game; v2\Interface\_AddOns");
        let s = script(&program, cwd, &["--resume", "0f3c-1a2b"], "_AddOns · Claude Code").unwrap();
        assert_eq!(
            s,
            "$Host.UI.RawUI.WindowTitle = '_AddOns · Claude Code'\n\
             Set-Location -LiteralPath 'D:\\Program Files (x86)\\Some Game; v2\\Interface\\_AddOns'\n\
             & 'C:\\Users\\Sam O''Neil\\.local\\bin\\claude.exe' '--resume' '0f3c-1a2b'\n"
        );
        // The debug stand-in's own arguments come first.
        let fake = Program::new(PathBuf::from(r"E:\b\yap.exe")).with_prefix(vec!["fake-claude".into(), r"C:\t\s.jsonl".into()]);
        let s = script(&fake, cwd, &["auth", "login"], "t").unwrap();
        assert!(s.ends_with("& 'E:\\b\\yap.exe' 'fake-claude' 'C:\\t\\s.jsonl' 'auth' 'login'\n"), "{s}");
    }

    #[test]
    fn a_shim_goes_through_cmd_word_for_word() {
        let shim = Program::new(PathBuf::from(r"C:\Program Files (x86)\nodejs\claude.cmd"));
        let s = script(&shim, Path::new(r"C:\w"), &["--resume", "abc"], "t").unwrap();
        assert!(
            s.ends_with("& cmd.exe --% /d /s /c \"\"C:\\Program Files (x86)\\nodejs\\claude.cmd\" \"--resume\" \"abc\"\"\n"),
            "{s}"
        );
        // Nothing cmd would read as more than text, nor a line break `--%` would end at.
        assert!(script(&Program::new(PathBuf::from(r"C:\50%\claude.cmd")), Path::new(r"C:\w"), &[], "t").is_err());
        assert!(script(&shim, Path::new(r"C:\w"), &["a&b"], "t").is_err());
        assert!(script(&shim, Path::new(r"C:\w"), &["a\nb"], "t").is_err());
        // The folder and title are PowerShell's to read, in quotes: anything goes.
        assert!(script(&shim, Path::new(r"C:\100% & co"), &[], "50% & co").is_ok());
    }

    #[test]
    fn windows_terminal_gets_the_folder_title_and_script() {
        let wt = Path::new(r"C:\Users\sam\AppData\Local\Microsoft\WindowsApps\wt.exe");
        let ps = Path::new(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe");
        let cwd = Path::new(r"D:\Program Files (x86)\Some Game; v2\_AddOns");
        let l = wt_launch(wt, ps, cwd, "Game; v2 · Claude Code", "& 'claude.exe'\n");
        assert!(!l.console);
        assert_eq!(l.app, wt);
        assert_eq!(l.args[..7], [
            "-w",
            "new",
            "--title",
            r"Game\; v2 · Claude Code",
            "-d",
            r"D:\Program Files (x86)\Some Game\; v2\_AddOns",
            r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
        ]);
        assert_eq!(l.args[7..10], ["-NoExit", "-NoProfile", "-EncodedCommand"]);
        assert_eq!(decode(&l.args[10]), "& 'claude.exe'\n");
        // Nothing after the folder holds a `;`, a space or a quote for wt to split.
        assert!(l.args[6..].iter().all(|a| !a.contains([';', ' ', '"'])));
        assert_eq!(
            command_line(&l),
            format!(
                "\"{}\" -w new --title \"Game\\; v2 · Claude Code\" -d \"D:\\Program Files (x86)\\Some Game\\; v2\\_AddOns\" \
                 C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe -NoExit -NoProfile -EncodedCommand {}",
                wt.display(),
                l.args[10]
            )
        );
        // A drive's root keeps its backslash.
        let l = wt_launch(wt, ps, Path::new(r"D:\"), "t", "");
        assert_eq!(l.args[5], r"D:\");
    }

    #[test]
    fn powershell_gets_a_console_of_its_own() {
        let ps = Path::new(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe");
        let l = powershell_launch(ps, "Set-Location -LiteralPath 'C:\\a b'\n");
        assert!(l.console);
        assert_eq!(l.args[..3], ["-NoExit", "-NoProfile", "-EncodedCommand"]);
        assert_eq!(decode(&l.args[3]), "Set-Location -LiteralPath 'C:\\a b'\n");
        assert!(powershell().ends_with(r"System32\WindowsPowerShell\v1.0\powershell.exe"));
    }

    #[test]
    fn the_environment_block_is_sorted_and_ends_twice() {
        let vars: Vec<(OsString, OsString)> =
            [("b", "2"), ("A", "1"), ("Path", "C:\\x")].iter().map(|(k, v)| (OsString::from(k), OsString::from(v))).collect();
        let block = env_block(&vars);
        let text = String::from_utf16(&block).unwrap();
        assert_eq!(text, "A=1\0b=2\0Path=C:\\x\0\0");
        assert_eq!(env_block(&[]), [0, 0]);
    }

    #[test]
    fn windows_terminal_is_found_off_path_too() {
        let root = temp_dir("wt");
        let local = root.join("local");
        let alias = local.join("Microsoft").join("WindowsApps").join("wt.exe");
        let elsewhere = root.join("bin");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let path = std::env::join_paths([elsewhere.clone()]).unwrap();
        let ext = OsString::from(".COM;.EXE;.BAT;.CMD");
        // Not on PATH, no alias: none.
        assert_eq!(find_wt(Some(&path), Some(&ext), Some(&local)), None);
        assert_eq!(find_wt(None, None, None), None);
        // The alias, though PATH lost its folder.
        std::fs::create_dir_all(alias.parent().unwrap()).unwrap();
        std::fs::write(&alias, b"").unwrap();
        assert_eq!(find_wt(Some(&path), Some(&ext), Some(&local)), Some(alias.clone()));
        assert_eq!(find_wt(None, Some(&ext), Some(&local)), Some(alias.clone()));
        // One on PATH comes first.
        std::fs::write(elsewhere.join("wt.exe"), b"").unwrap();
        assert_eq!(find_wt(Some(&path), Some(&ext), Some(&local)), Some(elsewhere.join("wt.exe")));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// This process into a new job that ends everything in it when it closes
    /// (as this process ends), allowing breakaway or not: a Yap started by a
    /// launcher that keeps its programs in one.
    fn join_a_kill_on_close_job(breakaway: bool) {
        use std::ffi::c_void;
        use windows::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        use windows::Win32::System::Threading::GetCurrentProcess;
        // SAFETY: a job of our own; its handle is left open on purpose
        // (Windows closes it as this process ends).
        unsafe {
            let job = CreateJobObjectW(None, windows::core::PCWSTR::null()).unwrap();
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if breakaway {
                info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_BREAKAWAY_OK;
            }
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .unwrap();
            AssignProcessToJobObject(job, GetCurrentProcess()).unwrap();
        }
    }

    /// **The real thing, by hand** (opens three windows on this PC): both
    /// routes and the shim's, with a stand-in for Claude Code (PowerShell on
    /// a script that writes down its folder, its arguments, whether its input
    /// and output are a console's, and its own line read back off the
    /// screen), in a folder with spaces, parentheses, a `;` and an
    /// underscore. This test first joins a kill-on-close job that allows
    /// breakaway: the windows must still be open after it ends (their
    /// process ids are printed, to close). `YAP_TERMINAL_CHECK_JOB=strict`:
    /// a job that allows none, so they open inside it and close with it.
    /// `YAP_TERMINAL_CHECK=1 cargo test terminal::tests::opens_real_windows
    /// -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn opens_real_windows() {
        if std::env::var_os("YAP_TERMINAL_CHECK").is_none() {
            println!("set YAP_TERMINAL_CHECK=1 to open the windows");
            return;
        }
        join_a_kill_on_close_job(std::env::var("YAP_TERMINAL_CHECK_JOB").as_deref() != Ok("strict"));
        let root = temp_dir("real");
        let dir = root.join("Some Game (x86); v2").join("_AddOns dir");
        std::fs::create_dir_all(&dir).unwrap();
        let ps = powershell();
        let standin = |name: &str| -> PathBuf {
            let report = dir.join(format!("report-{name}.txt"));
            let body = format!(
                "$parent = (Get-CimInstance Win32_Process -Filter \"ProcessId=$PID\").ParentProcessId\n\
                 Write-Host 'ok from the stand-in ({name})'\n\
                 $raw = $Host.UI.RawUI\n\
                 $y = $raw.CursorPosition.Y - 1\n\
                 $cells = $raw.GetBufferContents((New-Object Management.Automation.Host.Rectangle 0, $y, ($raw.BufferSize.Width - 1), $y))\n\
                 $line = (-join ($cells | ForEach-Object {{ $_.Character }})).TrimEnd()\n\
                 @(\"pid=$PID\", \"parent=$parent\", \"cwd=$((Get-Location).Path)\", \"args=$($args -join '|')\", \
                 \"stdin_redirected=$([Console]::IsInputRedirected)\", \"stdout_redirected=$([Console]::IsOutputRedirected)\", \
                 \"screen=$line\") | Out-File -Encoding utf8 -LiteralPath {}\n",
                ps_quote(&report.to_string_lossy())
            );
            let script = dir.join(format!("standin-{name}.ps1"));
            // UTF-8 with a BOM, so Windows PowerShell reads the `·`s right.
            std::fs::write(&script, [&[0xEF, 0xBB, 0xBF][..], body.as_bytes()].concat()).unwrap();
            report
        };
        let exe = |name: &str| {
            Program::new(ps.clone()).with_prefix(vec![
                "-NoProfile".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-File".into(),
                dir.join(format!("standin-{name}.ps1")).to_string_lossy().into_owned(),
            ])
        };
        let args = ["--resume", "0f3c-test"];
        let mut reports = Vec::new();
        // `YAP_TERMINAL_CHECK=1` opens all three; else a list ("wt,ps,shim").
        let only = std::env::var("YAP_TERMINAL_CHECK").unwrap_or_default();
        let wanted = |name: &str| only == "1" || only.split(',').any(|w| w.trim() == name);

        // Windows Terminal.
        if wanted("wt") {
            reports.push(standin("wt"));
            let wt = find_wt(
                std::env::var_os("PATH").as_deref(),
                std::env::var_os("PATHEXT").as_deref(),
                dirs::data_local_dir().as_deref(),
            )
            .expect("Windows Terminal on this PC");
            let s = script(&exe("wt"), &dir, &args, "Yap check; wt · Claude Code").unwrap();
            let started = start(&wt_launch(&wt, &ps, &dir, "Yap check; wt · Claude Code", &s), &dir, "t").unwrap();
            println!("wt.exe: {started:?}");
        }

        // PowerShell in a console of its own (the fallback).
        if wanted("ps") {
            reports.push(standin("ps"));
            let s = script(&exe("ps"), &dir, &args, "Yap check; ps · Claude Code").unwrap();
            let started = start(&powershell_launch(&ps, &s), &dir, "Yap check; ps · Claude Code").unwrap();
            println!("powershell: {started:?}");
        }

        // An npm-style shim, through cmd, in the fallback.
        if wanted("shim") {
            reports.push(standin("shim"));
            let shim_path = dir.join("claude.cmd");
            std::fs::write(&shim_path, "@\"%SystemRoot%\\System32\\WindowsPowerShell\\v1.0\\powershell.exe\" -NoProfile -ExecutionPolicy Bypass -File \"%~dp0standin-shim.ps1\" %*\r\n").unwrap();
            let s = script(&Program::new(shim_path), &dir, &args, "Yap check; shim · Claude Code").unwrap();
            let started = start(&powershell_launch(&ps, &s), &dir, "Yap check; shim · Claude Code").unwrap();
            println!("shim: {started:?}");
        }

        let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while reports.iter().any(|r| !r.is_file()) && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
        let mut failed = Vec::new();
        for report in &reports {
            let text = std::fs::read_to_string(report).unwrap_or_default();
            println!("--- {}\n{}", report.file_name().unwrap().to_string_lossy(), text.trim_start_matches('\u{feff}'));
            let ok = text.contains(&format!("cwd={}", dir.display()))
                && text.contains("args=--resume|0f3c-test")
                && text.contains("stdin_redirected=False")
                && text.contains("stdout_redirected=False")
                && text.contains("screen=ok from the stand-in");
            if !ok {
                failed.push(report.clone());
            }
        }
        println!("folder: {}", root.display());
        assert!(failed.is_empty(), "{failed:?}");
    }
}

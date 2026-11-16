//! **Where a Code chat's file links open** (16 Nov 2026): Settings →
//! Assistant → Yap Code → "Open file links in" (`agents_open_files`). A
//! link's click goes to Yap's own file view beside the chat (the default),
//! to a code editor found on this PC at the line, or to Windows' default app
//! for the file; a link's right-click menu offers each (`ui/fileLinks.js`).
//!
//! - **Which editors** ([`detect`]): the ones Yap knows ([`KINDS`]: VS Code,
//!   Cursor, VS Code Insiders, Windsurf, VSCodium, Zed, Sublime Text,
//!   Notepad++, JetBrains' IDEs ([`JETBRAINS`]), Notepad), found the way
//!   `apps.rs` finds apps — the App Paths registry, the Uninstall registry's
//!   install folders and icons, their usual folders, and PATH (a launcher
//!   such as `code.cmd` leads to the program beside it; JetBrains Toolbox's
//!   scripts name theirs) — each one's program checked to be there. Read
//!   only, kept [`FRESH`] (Settings asks afresh when its page opens). A test
//!   run's are the stand-ins `YAP_E2E_EDITORS` names, when it does.
//! - **The command** ([`args`]): the program itself, never a launcher script
//!   (`code.cmd` would run through `cmd.exe`) and never a shell; every path
//!   quoted ([`quote`], passed as written: `raw_arg`); at the line where the
//!   editor takes one: `-g "<file>:<line>"` for VS Code and its kin (the
//!   workspace's folder opened with it, so the file opens in that window),
//!   `"<file>:<line>"` for Zed's command line and Sublime Text, `-n<line>`
//!   for Notepad++, `--line <line>` for JetBrains. The file is always one
//!   `files::resolve` found in the chat's workspace: nothing from the chat's
//!   words is ever part of the command.
//! - **Windows' default app** ([`runs_when_opened`]): only for a file
//!   Windows would open, never one it would run. A program, a script or a
//!   web page (the shared-files list, `chat/shares.rs` `RUNS`; Windows' own
//!   `AssocIsDangerous`; a type whose app is a script host or an
//!   interpreter) shows in Yap's view instead, and says why.
//! - **A test run** starts nothing: it keeps the command it would have run
//!   (`e2e_file_opens`), for the e2e suite and Yap Lab to check.
//!
//! The log has the editor's name and whether there was a line, never a path.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Yap's own file view beside the chat: the setting's default.
pub const YAP: &str = "yap";
/// Windows' default app for the file.
pub const DEFAULT_APP: &str = "default";
/// The setting's editor, else the first one found (the file view's Open in editor).
pub const ANY_EDITOR: &str = "editor";
/// How long a look at this PC's editors is believed.
pub const FRESH: Duration = Duration::from_secs(120);

/// **How an editor is told the line.**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    /// `-g "<file>:<line>"`, the workspace's folder first (VS Code and the editors built on it).
    Goto,
    /// `"<file>:<line>"` (Zed's command line, Sublime Text).
    Suffix,
    /// `-n<line> "<file>"` (Notepad++).
    Npp,
    /// `--line <line> "<file>"` (JetBrains' IDEs).
    Flag,
    /// None: just the file (Notepad; Zed without its command line).
    None,
}

/// Where a usual folder is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Root {
    /// `%LOCALAPPDATA%\Programs` (a per-user install).
    LocalPrograms,
    /// `%ProgramFiles%` and `%ProgramFiles(x86)%`.
    ProgramFiles,
    /// `%SystemRoot%`.
    System,
}

/// **An editor Yap knows.**
#[derive(Debug)]
pub struct Kind {
    /// The setting's value for it.
    pub id: &'static str,
    pub name: &'static str,
    /// Its program's file name.
    pub exe: &'static str,
    /// Its usual folders.
    pub homes: &'static [(Root, &'static str)],
    /// A launcher on PATH, and the way from the launcher's folder to the program's.
    pub on_path: &'static [(&'static str, &'static str)],
    pub line: Line,
    /// A command-line program beside it, run instead when it's there (Zed's
    /// `bin\zed.exe`): a console program, started with no window.
    pub cli: Option<&'static str>,
}

/// **The editors Yap looks for**, in the order the setting lists them
/// (JetBrains' IDEs go before Notepad, which is always last).
pub const KINDS: &[Kind] = &[
    Kind { id: "vscode", name: "VS Code", exe: "Code.exe", homes: &[(Root::LocalPrograms, "Microsoft VS Code"), (Root::ProgramFiles, "Microsoft VS Code")], on_path: &[("code.cmd", "..")], line: Line::Goto, cli: None },
    Kind { id: "cursor", name: "Cursor", exe: "Cursor.exe", homes: &[(Root::LocalPrograms, "cursor"), (Root::ProgramFiles, "Cursor")], on_path: &[("cursor.cmd", r"..\..\.."), ("cursor.cmd", "..")], line: Line::Goto, cli: None },
    Kind { id: "vscode-insiders", name: "VS Code Insiders", exe: "Code - Insiders.exe", homes: &[(Root::LocalPrograms, "Microsoft VS Code Insiders"), (Root::ProgramFiles, "Microsoft VS Code Insiders")], on_path: &[("code-insiders.cmd", "..")], line: Line::Goto, cli: None },
    Kind { id: "windsurf", name: "Windsurf", exe: "Windsurf.exe", homes: &[(Root::LocalPrograms, "Windsurf"), (Root::ProgramFiles, "Windsurf")], on_path: &[("windsurf.cmd", "..")], line: Line::Goto, cli: None },
    Kind { id: "vscodium", name: "VSCodium", exe: "VSCodium.exe", homes: &[(Root::LocalPrograms, "VSCodium"), (Root::ProgramFiles, "VSCodium")], on_path: &[("codium.cmd", "..")], line: Line::Goto, cli: None },
    Kind { id: "zed", name: "Zed", exe: "Zed.exe", homes: &[(Root::LocalPrograms, "Zed"), (Root::ProgramFiles, "Zed")], on_path: &[("zed.exe", ".."), ("zed.exe", ".")], line: Line::Suffix, cli: Some(r"bin\zed.exe") },
    Kind { id: "sublime", name: "Sublime Text", exe: "sublime_text.exe", homes: &[(Root::ProgramFiles, "Sublime Text"), (Root::ProgramFiles, "Sublime Text 4"), (Root::ProgramFiles, "Sublime Text 3"), (Root::LocalPrograms, "Sublime Text")], on_path: &[("subl.exe", ".")], line: Line::Suffix, cli: None },
    Kind { id: "notepadpp", name: "Notepad++", exe: "notepad++.exe", homes: &[(Root::ProgramFiles, "Notepad++")], on_path: &[("notepad++.exe", ".")], line: Line::Npp, cli: None },
    Kind { id: "notepad", name: "Notepad", exe: "notepad.exe", homes: &[(Root::System, "System32")], on_path: &[], line: Line::None, cli: None },
];

/// **JetBrains' IDEs**: their program, the setting's value, their name.
/// A Community Edition (its folder says so) is `<id>-ce`.
pub const JETBRAINS: &[(&str, &str, &str)] = &[
    ("idea64.exe", "idea", "IntelliJ IDEA"),
    ("pycharm64.exe", "pycharm", "PyCharm"),
    ("webstorm64.exe", "webstorm", "WebStorm"),
    ("phpstorm64.exe", "phpstorm", "PhpStorm"),
    ("rider64.exe", "rider", "Rider"),
    ("goland64.exe", "goland", "GoLand"),
    ("clion64.exe", "clion", "CLion"),
    ("rubymine64.exe", "rubymine", "RubyMine"),
    ("rustrover64.exe", "rustrover", "RustRover"),
    ("datagrip64.exe", "datagrip", "DataGrip"),
    ("dataspell64.exe", "dataspell", "DataSpell"),
    ("studio64.exe", "android-studio", "Android Studio"),
];

/// **An editor on this PC.**
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Editor {
    /// The setting's value (`vscode`, `idea-ce`…).
    pub id: String,
    pub name: String,
    /// The program started.
    #[serde(skip)]
    pub program: PathBuf,
    #[serde(skip)]
    pub line: Line,
    /// A console program (Zed's command line): started with no window.
    #[serde(skip)]
    pub console: bool,
}

/// **An install the Uninstall registry lists**: its name, folder and icon's file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Installed {
    pub name: String,
    pub location: Option<PathBuf>,
    pub icon: Option<PathBuf>,
}

/// Words in an install's name that may be an editor's (its folder is looked in only then).
const INSTALL_WORDS: &[&str] = &[
    "visual studio code", "vs code", "vscode", "cursor", "windsurf", "codium", "zed", "sublime", "notepad++", "jetbrains", "intellij", "pycharm", "webstorm", "phpstorm", "rider", "goland",
    "clion", "rubymine", "rustrover", "datagrip", "dataspell", "android studio",
];

/// Whether an install called `name` may be an editor's. Pure.
pub fn may_be_editor(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    INSTALL_WORDS.iter().any(|w| n.contains(w))
}

/// `p` with its `.` and `..` worked out, as written (no file system). Pure.
pub fn tidy_path(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// **Where to look** ([`Sources::this_pc`]; a test's own folders).
#[derive(Debug, Clone, Default)]
pub struct Sources {
    /// `%LOCALAPPDATA%`.
    pub local_app_data: Option<PathBuf>,
    /// `%ProgramFiles%`, `%ProgramFiles(x86)%`.
    pub program_files: Vec<PathBuf>,
    /// `%SystemRoot%`.
    pub system_root: Option<PathBuf>,
    /// PATH's folders.
    pub path: Vec<PathBuf>,
    /// App Paths: a program's file name → its path.
    pub app_paths: Vec<(String, PathBuf)>,
    /// The Uninstall registry's installs.
    pub installed: Vec<Installed>,
}

impl Sources {
    /// This PC's environment and registry (read only).
    pub fn this_pc() -> Sources {
        let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let mut program_files: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"].iter().filter_map(|k| env(k)).collect();
        program_files.dedup();
        let path = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).filter(|d| d.is_absolute()).collect()).unwrap_or_default();
        let wanted: Vec<&str> = KINDS.iter().map(|k| k.exe).chain(JETBRAINS.iter().map(|j| j.0)).collect();
        Sources {
            local_app_data: env("LOCALAPPDATA"),
            program_files,
            system_root: env("SystemRoot"),
            path,
            app_paths: registry::app_paths(&wanted),
            installed: registry::installed(),
        }
    }
}

/// The kind whose program is called `file` (any case).
fn kind_named(file: &str) -> Option<&'static Kind> {
    KINDS.iter().find(|k| k.exe.eq_ignore_ascii_case(file))
}

/// The JetBrains IDE whose program is called `file` (any case).
fn jetbrains_named(file: &str) -> Option<&'static (&'static str, &'static str, &'static str)> {
    JETBRAINS.iter().find(|j| j.0.eq_ignore_ascii_case(file))
}

/// Whether `id` is one Yap knows (an editor's, or a JetBrains Community Edition's).
pub fn known(id: &str) -> bool {
    KINDS.iter().any(|k| k.id == id) || JETBRAINS.iter().any(|j| j.1 == id || id.strip_suffix("-ce") == Some(j.1))
}

/// The name of the editor `id` names, found or not. Pure.
pub fn name_of(id: &str) -> String {
    if let Some(k) = KINDS.iter().find(|k| k.id == id) {
        return k.name.to_string();
    }
    if let Some(j) = JETBRAINS.iter().find(|j| j.1 == id) {
        return j.2.to_string();
    }
    if let Some(j) = id.strip_suffix("-ce").and_then(|b| JETBRAINS.iter().find(|j| j.1 == b)) {
        return format!("{} Community", j.2);
    }
    "That editor".to_string()
}

/// **The editor the program at `path` is**, if it's one Yap knows. Pure
/// (but for the file system's say about Zed's command line).
pub fn editor_at(path: &Path) -> Option<Editor> {
    let file = path.file_name()?.to_string_lossy().into_owned();
    if let Some(k) = kind_named(&file) {
        let cli = k.cli.and_then(|c| path.parent().map(|d| d.join(c))).filter(|c| c.is_file());
        // Zed's own window takes no line: only its command line does.
        let (program, line, console) = match (k.cli, cli) {
            (Some(_), Some(c)) => (c, k.line, true),
            (Some(_), None) => (path.to_path_buf(), Line::None, false),
            _ => (path.to_path_buf(), k.line, false),
        };
        return Some(Editor { id: k.id.into(), name: k.name.into(), program, line, console });
    }
    let (_, id, name) = jetbrains_named(&file)?;
    // "IntelliJ IDEA Community Edition 2025.2", or Toolbox 1's `apps\IDEA-C\ch-0\<build>\bin`.
    let community = path.ancestors().skip(1).take(4).any(|a| {
        a.file_name().is_some_and(|n| {
            let n = n.to_string_lossy().to_ascii_lowercase();
            n.contains("community") || n.ends_with("-c")
        })
    });
    let (id, name) = if community { (format!("{id}-ce"), format!("{name} Community")) } else { (id.to_string(), name.to_string()) };
    Some(Editor { id, name, program: path.to_path_buf(), line: Line::Flag, console: false })
}

/// The folders in `dir`, newest name first (an IDE's versions: "2025.2" before "2025.1").
fn folders_in(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir).map(|r| r.flatten().map(|e| e.path()).filter(|p| p.is_dir()).take(400).collect()).unwrap_or_default();
    out.sort();
    out.reverse();
    out
}

/// JetBrains' programs under `dir`'s folders (`<dir>\<IDE>\bin\<ide>64.exe`).
fn jetbrains_in(dir: &Path, out: &mut Vec<PathBuf>) {
    for f in folders_in(dir) {
        for (exe, _, _) in JETBRAINS {
            out.push(f.join("bin").join(exe));
        }
    }
}

/// The program a JetBrains Toolbox script starts (`start "" "…\bin\idea64.exe" %*`): its quoted paths.
pub fn toolbox_script_targets(text: &str) -> Vec<PathBuf> {
    text.split('"').skip(1).step_by(2).filter(|s| Path::new(s).file_name().is_some_and(|n| jetbrains_named(&n.to_string_lossy()).is_some())).map(PathBuf::from).collect()
}

/// A small text file's start (a launcher script), if it's one.
fn small_text(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    f.by_ref().take(8_192).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// **The editors `src` shows**, each once (the first place found), in
/// [`KINDS`]' order with JetBrains' before Notepad. Every program is checked
/// to be there.
pub fn detect(src: &Sources) -> Vec<Editor> {
    // Every place a program may be, in the order believed.
    let mut maybe: Vec<PathBuf> = Vec::new();
    // 1. The App Paths registry.
    maybe.extend(src.app_paths.iter().map(|(_, p)| p.clone()));
    // 2. The Uninstall registry: an install's folder (its program there, or in its `bin`), its icon.
    let exes: Vec<&str> = KINDS.iter().map(|k| k.exe).chain(JETBRAINS.iter().map(|j| j.0)).collect();
    for i in &src.installed {
        if let Some(icon) = &i.icon {
            maybe.push(icon.clone());
        }
        if let Some(l) = i.location.as_ref().filter(|_| may_be_editor(&i.name)) {
            for e in &exes {
                maybe.push(l.join(e));
                maybe.push(l.join("bin").join(e));
            }
        }
    }
    // 3. The usual folders.
    for k in KINDS {
        for (root, home) in k.homes {
            let roots: Vec<PathBuf> = match root {
                Root::LocalPrograms => src.local_app_data.iter().map(|l| l.join("Programs")).collect(),
                Root::ProgramFiles => src.program_files.clone(),
                Root::System => src.system_root.iter().cloned().collect(),
            };
            maybe.extend(roots.into_iter().map(|r| r.join(home).join(k.exe)));
        }
    }
    for pf in &src.program_files {
        jetbrains_in(&pf.join("JetBrains"), &mut maybe);
        maybe.push(pf.join(r"Android\Android Studio\bin\studio64.exe"));
    }
    if let Some(l) = &src.local_app_data {
        // Toolbox 2 installs in Programs\<IDE>; Toolbox 1 in JetBrains\Toolbox\apps\<IDE>\ch-0\<build>.
        jetbrains_in(&l.join("Programs"), &mut maybe);
        for app in folders_in(&l.join(r"JetBrains\Toolbox\apps")) {
            for ch in folders_in(&app) {
                jetbrains_in(&ch, &mut maybe);
            }
        }
    }
    // 4. PATH: a launcher leads to its program; an IDE's own bin; Toolbox's scripts.
    let mut dirs = src.path.clone();
    if let Some(l) = &src.local_app_data {
        dirs.push(l.join(r"JetBrains\Toolbox\scripts"));
    }
    for d in &dirs {
        for k in KINDS {
            for (launcher, to) in k.on_path {
                if d.join(launcher).is_file() {
                    maybe.push(tidy_path(&d.join(to).join(k.exe)));
                }
            }
        }
        for (exe, _, _) in JETBRAINS {
            maybe.push(d.join(exe));
            // Toolbox's script for it: `idea.cmd` for idea64.exe, `studio.cmd` for studio64.exe.
            let script = d.join(format!("{}.cmd", exe.trim_end_matches(".exe").trim_end_matches("64")));
            if script.is_file() {
                if let Some(text) = small_text(&script) {
                    maybe.extend(toolbox_script_targets(&text));
                }
            }
        }
    }

    let mut found: Vec<Editor> = Vec::new();
    for p in maybe {
        if found.len() >= 40 {
            break;
        }
        let Some(e) = editor_at(&p) else { continue };
        if found.iter().any(|f| f.id == e.id) || !p.is_file() {
            continue;
        }
        found.push(e);
    }
    found.sort_by_key(|e| order_of(&e.id));
    found
}

/// Where `id` goes in the list: [`KINDS`]' order, JetBrains' before Notepad.
fn order_of(id: &str) -> usize {
    if id == "notepad" {
        return usize::MAX;
    }
    if let Some(i) = KINDS.iter().position(|k| k.id == id) {
        return i;
    }
    let base = id.strip_suffix("-ce").unwrap_or(id);
    KINDS.len() + 2 * JETBRAINS.iter().position(|j| j.1 == base).unwrap_or(JETBRAINS.len()) + usize::from(id.ends_with("-ce"))
}

/// A test run's stand-ins: the JSON file `YAP_E2E_EDITORS` names, `[{ "id", "program" }]`
/// (never started: a test run keeps the command instead).
#[derive(Debug, Deserialize)]
struct StandIn {
    id: String,
    program: String,
}

fn stand_ins() -> Option<Vec<Editor>> {
    if !crate::shell::e2e::active() {
        return None;
    }
    let file = std::env::var_os("YAP_E2E_EDITORS").filter(|v| !v.is_empty())?;
    let mut list: Vec<StandIn> = std::fs::read_to_string(file).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    list.sort_by_key(|s| order_of(&s.id));
    Some(
        list.into_iter()
            .filter(|s| known(&s.id))
            .map(|s| {
                let kind = KINDS.iter().find(|k| k.id == s.id);
                let line = kind.map(|k| k.line).unwrap_or(Line::Flag);
                Editor { id: s.id.clone(), name: name_of(&s.id), program: PathBuf::from(s.program), line, console: kind.is_some_and(|k| k.cli.is_some()) }
            })
            .collect(),
    )
}

/// **The editors on this PC** (kept [`FRESH`]; `refresh` looks again).
pub fn found(refresh: bool) -> Vec<Editor> {
    static SEEN: Mutex<Option<(Instant, Vec<Editor>)>> = Mutex::new(None);
    if let Some(list) = stand_ins() {
        return list;
    }
    if !refresh {
        if let Ok(g) = SEEN.lock() {
            if let Some((at, list)) = g.as_ref() {
                if at.elapsed() < FRESH {
                    return list.clone();
                }
            }
        }
    }
    let list = detect(&Sources::this_pc());
    tracing::debug!(editors = list.len(), "Code chat: editors looked for");
    if let Ok(mut g) = SEEN.lock() {
        *g = Some((Instant::now(), list.clone()));
    }
    list
}

/// **The editor for "Open in …"**: the setting's when it's one found, else
/// the first found that isn't Notepad, else Notepad.
pub fn preferred<'a>(list: &'a [Editor], choice: &str) -> Option<&'a Editor> {
    list.iter().find(|e| e.id == choice).or_else(|| list.iter().find(|e| e.id != "notepad")).or_else(|| list.first())
}

// ---- the command ---------------------------------------------------------------------------------

/// **`s` as one argument of a Windows command line, always in quotes**
/// (the C runtime's rules: a backslash before a quote, or before the
/// closing one, doubled; a quote escaped). Pure.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    let mut slashes = 0usize;
    for c in s.chars() {
        match c {
            '\\' => slashes += 1,
            '"' => {
                out.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
                out.push('"');
                slashes = 0;
                continue;
            }
            _ => {}
        }
        if c != '\\' {
            out.extend(std::iter::repeat_n('\\', slashes));
            slashes = 0;
            out.push(c);
        }
    }
    out.extend(std::iter::repeat_n('\\', slashes * 2));
    out.push('"');
    out
}

/// **The arguments that open `abs` at `line` in `e`**, as they go on the
/// command line (paths quoted). `workspace`: the chat's workspace folder,
/// opened with the file by the editors built on VS Code. Pure.
pub fn args(e: &Editor, abs: &str, line: Option<u32>, workspace: &str) -> Vec<String> {
    let at = |l: u32| quote(&format!("{abs}:{l}"));
    match (e.line, line) {
        (Line::Goto, Some(l)) => vec![quote(workspace), "-g".into(), at(l)],
        (Line::Goto, None) => vec![quote(workspace), quote(abs)],
        (Line::Suffix, Some(l)) => vec![at(l)],
        (Line::Npp, Some(l)) => vec![format!("-n{l}"), quote(abs)],
        (Line::Flag, Some(l)) => vec!["--line".into(), l.to_string(), quote(abs)],
        _ => vec![quote(abs)],
    }
}

/// The whole command line, as Windows gets it (the record and the debug log). Pure.
pub fn command_line(program: &Path, args: &[String]) -> String {
    std::iter::once(quote(&program.to_string_lossy())).chain(args.iter().cloned()).collect::<Vec<_>>().join(" ")
}

/// **What a test run would have started** (`e2e_file_opens`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Started {
    /// The editor's id, or [`DEFAULT_APP`].
    pub with: String,
    pub name: String,
    /// The program (none for Windows' default app).
    pub program: Option<String>,
    pub args: Vec<String>,
    pub command_line: Option<String>,
    /// The file, from the workspace's root.
    pub rel: String,
    pub line: Option<u32>,
}

static STARTED: Mutex<Vec<Started>> = Mutex::new(Vec::new());

/// A test run's record of an open.
pub fn keep(s: Started) {
    if let Ok(mut g) = STARTED.lock() {
        g.push(s);
    }
}

/// **Start `e` on the file `abs`** (`rel` from the workspace `workspace`), at `line`.
pub fn start(e: &Editor, abs: &str, rel: &str, line: Option<u32>, workspace: &str) -> Result<(), String> {
    let args = args(e, abs, line, workspace);
    if crate::shell::e2e::active() {
        keep(Started {
            with: e.id.clone(),
            name: e.name.clone(),
            program: Some(e.program.to_string_lossy().into_owned()),
            command_line: Some(command_line(&e.program, &args)),
            args,
            rel: rel.to_string(),
            line,
        });
        return Ok(());
    }
    let mut cmd = std::process::Command::new(&e.program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        for a in &args {
            cmd.raw_arg(a);
        }
        if e.console {
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
    }
    #[cfg(not(windows))]
    for a in &args {
        cmd.arg(a.trim_matches('"'));
    }
    if Path::new(workspace).is_dir() {
        cmd.current_dir(workspace);
    }
    cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    cmd.spawn().map_err(|err| format!("Couldn't start {}: {err}", e.name))?;
    tracing::info!(editor = %e.name, at_line = line.is_some(), "Code chat: a file opened in an editor");
    tracing::debug!(command = %command_line(&e.program, &args), "Code chat: the editor's command");
    Ok(())
}

// ---- Windows' default app -----------------------------------------------------------------------

/// Programs that run a file rather than show it: a type whose app is one
/// of these is never opened in "Windows' default app".
const RUNNERS: &[&str] = &[
    "python", "pythonw", "py", "pyw", "wscript", "cscript", "mshta", "powershell", "pwsh", "cmd", "node", "deno", "bun", "bash", "sh", "git-bash", "wsl", "java", "javaw", "perl", "ruby",
    "rubyw", "php", "lua", "tclsh", "wish", "rscript", "rundll32", "msiexec", "regedit", "reg", "conhost", "wt", "openfiles", "autoit3", "installutil", "msbuild", "dotnet",
];

/// Whether the program `file` (a path or a name) runs what it opens. Pure.
pub fn is_runner(file: &str) -> bool {
    let name = file.trim().trim_matches('"').rsplit(['\\', '/']).next().unwrap_or("").to_ascii_lowercase();
    let stem = name.strip_suffix(".exe").unwrap_or(&name);
    RUNNERS.contains(&stem) || stem.starts_with("autohotkey") || stem.starts_with("python")
}

/// The ending Windows goes by (`report.pdf.exe` → `exe`; a trailing dot or space tidied), lower case. Pure.
pub fn ending(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().trim_end_matches(['.', ' ']).to_ascii_lowercase();
    name.rsplit_once('.').map(|(_, e)| e.to_string()).filter(|e| !e.is_empty())
}

/// **Why Windows' default app won't open `path`**: `Some(why)` for a file
/// it would run (see the module's rules), `None` for one it opens.
pub fn runs_when_opened(path: &Path) -> Option<String> {
    let ext = ending(path)?;
    let why = || Some(format!("Windows would run a .{ext} file rather than just open it, so it's shown here instead."));
    if crate::chat::shares::RUNS.contains(&ext.as_str()) {
        return why();
    }
    if registry::dangerous(&ext) {
        return why();
    }
    if registry::app_for(&ext).is_some_and(|app| is_runner(&app)) {
        return why();
    }
    None
}

/// The default app, started on the file (a test run keeps it instead).
pub fn open_default(abs: &str, rel: &str, line: Option<u32>) -> Result<(), String> {
    if crate::shell::e2e::active() {
        keep(Started { with: DEFAULT_APP.into(), name: "Windows' default app".into(), program: None, args: Vec::new(), command_line: None, rel: rel.to_string(), line });
        return Ok(());
    }
    tauri_plugin_opener::open_path(abs, None::<&str>).map_err(|e| format!("Couldn't open it: {e}"))?;
    tracing::info!("Code chat: a file opened in its default app");
    Ok(())
}

/// Test mode only: what opening files would have started.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_file_opens(clear: Option<bool>) -> Result<Vec<Started>, String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    let mut g = STARTED.lock().map_err(|e| e.to_string())?;
    let out = g.clone();
    if clear == Some(true) {
        g.clear();
    }
    Ok(out)
}

/// **The editors for the setting and the menu.**
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorsView {
    /// The setting now (`yap`, `default` or an editor's id).
    pub choice: String,
    /// The editors on this PC, in the setting's order.
    pub editors: Vec<Editor>,
    /// "Open in …"'s editor: the setting's, else the first found.
    pub preferred: Option<Editor>,
    /// The setting's editor isn't on this PC (any more).
    pub missing: bool,
    /// The setting's editor's name, found or not (none for Yap's view or the default app).
    pub choice_name: Option<String>,
}

/// **The editors on this PC** (Settings' "Open file links in", a link's menu).
#[tauri::command]
pub async fn code_editors(refresh: Option<bool>) -> EditorsView {
    let choice = crate::config::load().agents_open_files;
    let editors = tauri::async_runtime::spawn_blocking(move || found(refresh.unwrap_or(false))).await.unwrap_or_default();
    let preferred = preferred(&editors, &choice).cloned();
    let missing = known(&choice) && !editors.iter().any(|e| e.id == choice);
    let choice_name = known(&choice).then(|| name_of(&choice));
    EditorsView { choice, editors, preferred, missing, choice_name }
}

#[cfg_attr(windows, path = "editors/registry.rs")]
#[cfg_attr(not(windows), path = "editors/elsewhere.rs")]
mod registry;

#[cfg(test)]
#[path = "editors/tests.rs"]
mod tests;

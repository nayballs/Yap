//! **What kind of app a dictation goes into** — a terminal (and which coding
//! CLI runs in it), a chat app, or anything else — and what that changes
//! about the paste.
//!
//! [`of_window`] looks at the target window once, at record start, beside
//! `target_hwnd`: its process name, window class and title (cheap). Only in
//! an editor with an integrated terminal (VS Code, Cursor…) does it also ask
//! UI Automation one question — is the focus xterm.js's input? — since
//! there the window is the editor's whatever has the focus
//! ([`editor_terminal`]). Only the process name and the kind are ever
//! logged, never the title. The rest of this module is pure and
//! unit-tested; `text_injector::prepare_dictation` applies it as the last
//! step before the paste (after AI cleanup, the dictionary, the "press
//! enter" command and snippets).
//!
//! - **The paste keys** ([`AppKind::paste_key`], whatever the settings):
//!   Ctrl+V, but in a terminal what it binds — Shift+Insert in mintty (Git
//!   Bash), PuTTY/KiTTY and MobaXterm, Ctrl+Shift+V in Alacritty, WezTerm,
//!   Tabby, Hyper and Rio ([`Host::paste_key`]), where Ctrl+V would reach
//!   the shell as a ^V.
//! - **Terminals** (`config.terminal_mode`, "Safer pasting into terminals"):
//!   pasting several lines into a shell can run each one as a command, so
//!   the dictation becomes one line ([`terminal_text`]: line breaks joined,
//!   list markers folded into "; ", tabs and control characters out), with
//!   no space after it, and Yap presses Enter only when "press enter" was
//!   said ([`submit_key`]). A coding CLI that takes a paste as one block —
//!   Claude Code or Codex in Windows Terminal, which pass a paste through as
//!   bracketed paste — keeps the line breaks, and there auto-submit works as
//!   configured (it sends a prompt, not a shell command). The text around a
//!   terminal's cursor isn't read: it's the screen, not a text field. Edit
//!   mode never reaches for Ctrl+C there (it would interrupt the program),
//!   and the AI's rewrite or Command text gets the same one-line rule.
//! - **Chat apps** (`config.chat_no_full_stop`, "Leave the full stop off
//!   chat messages"): a message of one or two sentences in one paragraph
//!   doesn't end with a full stop ([`drop_chat_full_stop`]).

use crate::config::DictionaryEntry;
use crate::pasting::smart_insert::FieldContext;

/// What kind of app the dictation goes into.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AppKind {
    /// A terminal or console window.
    Terminal(Terminal),
    /// A chat app (Slack, Discord, WhatsApp, Teams…), desktop or in a browser.
    Messaging,
    #[default]
    Other,
}

/// A terminal window, and the coding CLI its title shows, if any.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Terminal {
    pub coding_cli: Option<CodingCli>,
    /// Which terminal it is: what pastes there ([`Host::paste_key`]), and
    /// whether a paste reaches the program as one block (Windows Terminal
    /// wraps it in bracketed paste for an app that asks for it).
    pub host: Host,
}

/// The terminal program a dictation goes into.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Host {
    /// Windows Terminal.
    WindowsTerminal,
    /// A classic console window (conhost), or a terminal Yap knows by name
    /// but not its keys (Warp, Extraterm…): Ctrl+V.
    #[default]
    Console,
    /// mintty: Git Bash, MSYS2, Cygwin, WSLtty.
    Mintty,
    /// PuTTY and KiTTY.
    Putty,
    MobaXterm,
    Alacritty,
    WezTerm,
    Tabby,
    Hyper,
    Rio,
    /// ConEmu and Cmder.
    ConEmu,
    /// An editor's integrated terminal (VS Code, Cursor, Windsurf…): the
    /// focused element is xterm.js's input ([`editor_terminal`]).
    Editor,
    /// A terminal in a browser page — vscode.dev, github.dev and
    /// Codespaces' terminal, a cloud shell, JupyterLab's —: xterm.js's input
    /// has the focus ([`web_terminal_page`]).
    Browser,
    /// Visual Studio's terminal: Windows Terminal's own control
    /// (`Microsoft.Terminal.Wpf`, its window class `HwndTerminalClass`)
    /// has the focus, in Visual Studio or any app that hosts it
    /// ([`terminal_control`]).
    VisualStudio,
}

impl Host {
    /// For the debug log ("terminal (mintty)").
    fn name(self) -> &'static str {
        match self {
            Host::WindowsTerminal => "Windows Terminal",
            Host::Console => "a console",
            Host::Mintty => "mintty",
            Host::Putty => "PuTTY",
            Host::MobaXterm => "MobaXterm",
            Host::Alacritty => "Alacritty",
            Host::WezTerm => "WezTerm",
            Host::Tabby => "Tabby",
            Host::Hyper => "Hyper",
            Host::Rio => "Rio",
            Host::ConEmu => "ConEmu",
            Host::Editor => "an editor's terminal",
            Host::Browser => "a browser's terminal",
            Host::VisualStudio => "Visual Studio's terminal",
        }
    }

    /// The keys that paste in this terminal by default. Ctrl+V reaches the
    /// program as a ^V in most of them (bash's "insert the next key as is"),
    /// so each gets the paste its own defaults bind (2026):
    ///
    /// - **Shift+Insert**: mintty (`ClipShortcuts`, on; its Ctrl+Shift+V
    ///   needs `CtrlShiftShortcuts`, off), PuTTY/KiTTY (Shift+Ins, as a
    ///   right-click), MobaXterm ("Paste inside terminal").
    /// - **Ctrl+Shift+V**: Alacritty (its `Paste`; Shift+Insert is
    ///   `PasteSelection`), WezTerm (`PasteFrom "Clipboard"`; Shift+Insert is
    ///   the primary selection), Tabby, Hyper (`editor:paste`), Rio.
    /// - **Ctrl+V**: Windows Terminal (its default `paste` bindings include
    ///   it), a console window ("Enable Ctrl key shortcuts", on since Windows
    ///   10), ConEmu/Cmder (Ctrl+V pastes it all; Shift+Ins only the first
    ///   line), VS Code's terminal (`workbench.action.terminal.paste`), a
    ///   browser's (the page's own paste event, which xterm.js takes),
    ///   Visual Studio's (its terminal binds Ctrl+V to paste), and any
    ///   terminal whose keys Yap doesn't know — as before.
    ///
    /// A classic console window (conhost) gets no keys at all: its own Edit
    /// → Paste ([`PasteKey::ConsoleMenu`]). Its Ctrl+V pastes only for a
    /// program reading lines (cmd, PowerShell); one that reads the keyboard
    /// raw — Claude Code, Codex, vim — gets a ^V instead (Shift+Insert an
    /// escape sequence), where the console's own paste reaches it, in
    /// bracketed paste when it asked for it (the robot, 2026-10-25).
    pub fn paste_key(self) -> PasteKey {
        match self {
            Host::Mintty | Host::Putty | Host::MobaXterm => PasteKey::ShiftInsert,
            Host::Alacritty | Host::WezTerm | Host::Tabby | Host::Hyper | Host::Rio => PasteKey::CtrlShiftV,
            Host::Console => PasteKey::ConsoleMenu,
            Host::WindowsTerminal | Host::ConEmu | Host::Editor | Host::Browser | Host::VisualStudio => PasteKey::CtrlV,
        }
    }
}

/// The keys Yap presses to paste.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PasteKey {
    #[default]
    CtrlV,
    ShiftInsert,
    CtrlShiftV,
    /// A classic console window's own Edit → Paste (its menu command,
    /// `WM_COMMAND` 0xFFF1 to the window): no keys. Ctrl+V when the window
    /// turns out not to be one (a terminal known only by its name).
    ConsoleMenu,
}

impl PasteKey {
    pub fn name(self) -> &'static str {
        match self {
            PasteKey::CtrlV => "Ctrl+V",
            PasteKey::ShiftInsert => "Shift+Insert",
            PasteKey::CtrlShiftV => "Ctrl+Shift+V",
            PasteKey::ConsoleMenu => "the console's Paste",
        }
    }
}

/// The AI coding tools Yap recognises in a terminal's title.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodingCli {
    ClaudeCode,
    Codex,
    GeminiCli,
    QwenCode,
    OpenCode,
    Aider,
    CopilotCli,
    CursorAgent,
    Crush,
}

impl CodingCli {
    pub fn name(self) -> &'static str {
        match self {
            CodingCli::ClaudeCode => "Claude Code",
            CodingCli::Codex => "Codex",
            CodingCli::GeminiCli => "Gemini CLI",
            CodingCli::QwenCode => "Qwen Code",
            CodingCli::OpenCode => "OpenCode",
            CodingCli::Aider => "Aider",
            CodingCli::CopilotCli => "Copilot CLI",
            CodingCli::CursorAgent => "Cursor Agent",
            CodingCli::Crush => "Crush",
        }
    }

    /// Takes a pasted block of lines as one paste, not a line at a time:
    /// Claude Code (bracketed paste) and Codex (bracketed paste, and on
    /// Windows a paste-burst check that turns Enter inside a paste into a
    /// new line). The others may well too; until that's known, their pastes
    /// are joined into one line like a shell's.
    fn takes_pasted_lines(self) -> bool {
        matches!(self, CodingCli::ClaudeCode | CodingCli::Codex)
    }
}

impl Terminal {
    /// A multi-line paste is safe here: a coding CLI that takes it as one
    /// paste, in Windows Terminal (which passes it on as bracketed paste).
    /// Everywhere else the lines are joined.
    pub fn keeps_lines(&self) -> bool {
        self.host == Host::WindowsTerminal && self.coding_cli.is_some_and(CodingCli::takes_pasted_lines)
    }
}

impl AppKind {
    pub fn is_terminal(self) -> bool {
        matches!(self, AppKind::Terminal(_))
    }

    /// The kind Yap acts on, given the settings: with "Safer pasting into
    /// terminals" off a terminal is just another app, and with "Leave the
    /// full stop off chat messages" off so is a chat app.
    pub fn with_settings(self, terminal_mode: bool, chat_no_full_stop: bool) -> AppKind {
        match self {
            AppKind::Terminal(_) if !terminal_mode => AppKind::Other,
            AppKind::Messaging if !chat_no_full_stop => AppKind::Other,
            kind => kind,
        }
    }

    /// The keys that paste into this app: Ctrl+V, but in a terminal what
    /// that terminal binds ([`Host::paste_key`]). Asked of the kind as read,
    /// before the settings: with "Safer pasting into terminals" off a
    /// terminal is fitted like any app, but Ctrl+V still wouldn't paste.
    pub fn paste_key(self) -> PasteKey {
        match self {
            AppKind::Terminal(t) => t.host.paste_key(),
            _ => PasteKey::CtrlV,
        }
    }

    /// For the debug log (no titles, no text).
    pub fn describe(self) -> String {
        match self {
            AppKind::Terminal(t) => {
                let host = t.host.name();
                match t.coding_cli {
                    Some(cli) => format!("terminal ({} in {host})", cli.name()),
                    None => format!("terminal ({host})"),
                }
            }
            AppKind::Messaging => "chat app".into(),
            AppKind::Other => "other".into(),
        }
    }
}

// ---- Telling the app apart ----

/// Terminal programs, by process name (lower case, without ".exe").
const TERMINAL_EXES: &[&str] = &[
    "windowsterminal",
    "wt",
    "openconsole",
    "conhost",
    "cmd",
    "powershell",
    "pwsh",
    "wezterm-gui",
    "wezterm",
    "alacritty",
    "mintty",
    "git-bash",
    "tabby",
    "hyper",
    "conemu",
    "conemu64",
    "cmder",
    "putty",
    "kitty",
    "mobaxterm",
    "warp",
    "rio",
    "extraterm",
    "contour",
    "ghostty",
    "fluentterminal.app",
    "wsl",
    "bash",
];

/// Which terminal a window is: by its class, whatever process owns it (a
/// classic console window can report the program running in it, e.g.
/// claude.exe), else by its process name (`stem`: lower case, no ".exe").
/// `None`: not a terminal.
fn terminal_host(stem: &str, class: &str) -> Option<Host> {
    let by_class = match class {
        "CASCADIA_HOSTING_WINDOW_CLASS" => Some(Host::WindowsTerminal),
        "ConsoleWindowClass" => Some(Host::Console),
        "mintty" => Some(Host::Mintty),
        "PuTTY" | "KiTTY" => Some(Host::Putty),
        "VirtualConsoleClass" => Some(Host::ConEmu),
        "org.wezfurlong.wezterm" => Some(Host::WezTerm),
        _ => None,
    };
    if by_class.is_some() {
        return by_class;
    }
    Some(match stem {
        "windowsterminal" | "wt" => Host::WindowsTerminal,
        "mintty" | "git-bash" => Host::Mintty,
        "putty" | "kitty" => Host::Putty,
        "mobaxterm" => Host::MobaXterm,
        "alacritty" => Host::Alacritty,
        "wezterm" | "wezterm-gui" => Host::WezTerm,
        "tabby" => Host::Tabby,
        "hyper" => Host::Hyper,
        "rio" => Host::Rio,
        "conemu" | "conemu64" | "cmder" => Host::ConEmu,
        s if TERMINAL_EXES.contains(&s) => Host::Console,
        _ => return None,
    })
}

/// Editors with an integrated terminal (xterm.js), by process name (lower
/// case, without ".exe"): VS Code and the editors built on it. Their window
/// is the editor's whatever has the focus; whether a dictation goes into the
/// terminal is read from the focused element ([`editor_terminal`]).
const EDITOR_EXES: &[&str] = &[
    "code",
    "code - insiders",
    "code - oss",
    "vscodium",
    "codium",
    "cursor",
    "windsurf",
    "positron",
    "trae",
    "kiro",
    "antigravity",
    "azuredatastudio",
];

/// xterm.js's input element: a hidden textarea with this class, which holds
/// the keyboard focus while its terminal does (xterm.js
/// `CoreBrowserTerminal`; Chromium gives an element's HTML class as its UI
/// Automation ClassName).
const XTERM_INPUT_CLASS: &str = "xterm-helper-textarea";

/// The focused element of an editor's window (`class`: its UI Automation
/// ClassName, `name`: its Name) is an integrated terminal's input: that
/// terminal, with the coding CLI its title shows. VS Code labels the input
/// "Terminal 1, <title>" then lines of screen-reader help (the words are
/// translated, the number and the title aren't); the title is the
/// process's name or what the program set ("claude", "✳ Fix the build").
/// Chromium folds the label's line breaks into spaces, so the help often
/// follows the title on one line: a title the whole line doesn't name is
/// tried by its first word ("claude Run the command: …").
pub fn editor_terminal(class: &str, name: &str) -> Option<Terminal> {
    if !class.split_whitespace().any(|c| c == XTERM_INPUT_CLASS) {
        return None;
    }
    let cli = terminal_title(name).and_then(|title| {
        coding_cli(title).or_else(|| title.split_whitespace().next().filter(|w| *w != title).and_then(coding_cli))
    });
    Some(Terminal {
        coding_cli: cli,
        host: Host::Editor,
    })
}

/// Words in a browser window's title that say its page may hold a terminal
/// (lower case): VS Code in the browser (vscode.dev, github.dev,
/// Codespaces, code-server, Gitpod), notebooks, cloud shells and online
/// IDEs. Only then is UI Automation asked what has the focus.
const WEB_TERMINAL_PAGES: &[&str] = &[
    "visual studio code",
    "vscode.dev",
    "github.dev",
    "codespaces",
    "code-server",
    "gitpod",
    "jupyter",
    "colab",
    "cloud shell",
    "cloudshell",
    "replit",
    "stackblitz",
    "codesandbox",
    "terminal",
    "shell",
    "ssh",
    "console",
];

/// A browser window (`stem`: its process, lower case, no ".exe") whose
/// title names a page that may hold a terminal ([`WEB_TERMINAL_PAGES`]).
pub fn web_terminal_page(stem: &str, title: &str) -> bool {
    if !is_browser(stem) {
        return false;
    }
    let title = title.to_lowercase();
    WEB_TERMINAL_PAGES.iter().any(|w| title.contains(w))
}

/// Windows Terminal's control as other apps host it (`Microsoft.Terminal.Wpf`'s
/// `HwndTerminal`: Visual Studio's terminal): its window's class. Other
/// apps' focused child windows that are a terminal.
const TERMINAL_CONTROL_CLASSES: &[(&str, Host)] = &[("HwndTerminalClass", Host::VisualStudio), ("VirtualConsoleClass", Host::ConEmu)];

/// The window with the keyboard focus in an app (`focus_class`: its class,
/// from Win32, no UI Automation) is a terminal control: which one.
pub fn terminal_control(focus_class: &str) -> Option<Host> {
    TERMINAL_CONTROL_CLASSES.iter().find(|(c, _)| *c == focus_class).map(|&(_, host)| host)
}

/// The title in an integrated terminal's label: what follows "Terminal
/// <n>" and its separator ("," "、" "，" ":") on the label's first line.
fn terminal_title(label: &str) -> Option<&str> {
    let line = label.lines().next()?.trim();
    let digits = line.find(|c: char| c.is_ascii_digit())?;
    let after = line[digits..].trim_start_matches(|c: char| c.is_ascii_digit());
    let title = after.trim_start_matches([',', '、', '，', ':', '：', ' ']).trim();
    (!title.is_empty() && after.len() != title.len()).then_some(title)
}

/// Chat apps, by process name (lower case, without ".exe").
const CHAT_EXES: &[&str] = &[
    "slack",
    "discord",
    "discordptb",
    "discordcanary",
    "whatsapp",
    "whatsapp.root",
    "telegram",
    "signal",
    "messenger",
    "ms-teams",
    "teams",
    "element",
    "mattermost",
    "rocket.chat",
    "beeper",
    "ferdium",
    "rambox",
    "franz",
    "viber",
    "line",
    "wechat",
    "weixin",
    "kakaotalk",
    "wire",
    "session",
    "zulip",
    "skype",
];

/// Browsers, whose window title shows the page (WhatsApp Web, Slack…).
const BROWSER_EXES: &[&str] = &[
    "chrome",
    "msedge",
    "firefox",
    "brave",
    "opera",
    "vivaldi",
    "arc",
    "zen",
    "librewolf",
    "waterfox",
    "floorp",
    "chromium",
    "thorium",
];

/// The parts of a browser title that name a chat app ("Chat | Sam |
/// Microsoft Teams", "(3) WhatsApp", "general - Acme - Slack").
const CHAT_PAGES: &[&str] = &[
    "whatsapp",
    "whatsapp web",
    "slack",
    "discord",
    "messenger",
    "microsoft teams",
    "telegram",
    "telegram web",
    "google chat",
    "google messages",
    "messages for web",
];

/// The process name without its folder and ".exe", in lower case.
pub(crate) fn exe_stem(exe: &str) -> String {
    let base = exe.trim().rsplit(['\\', '/']).next().unwrap_or("");
    let lower = base.to_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

/// What kind of app a window is, from its process name (`exe`, e.g.
/// "WindowsTerminal.exe"), window class and title.
pub fn classify(exe: Option<&str>, class: &str, title: &str) -> AppKind {
    let stem = exe_stem(exe.unwrap_or(""));
    if let Some(host) = terminal_host(&stem, class) {
        return AppKind::Terminal(Terminal {
            coding_cli: coding_cli(title),
            host,
        });
    }
    if CHAT_EXES.contains(&stem.as_str()) {
        return AppKind::Messaging;
    }
    if BROWSER_EXES.contains(&stem.as_str()) && title_shows_chat(title) {
        return AppKind::Messaging;
    }
    AppKind::Other
}

/// The CLIs' command names, for a title that names the program running
/// ("claude", "Command Prompt - aider --model sonnet", "codex.exe").
const CLI_COMMANDS: &[(&str, CodingCli)] = &[
    ("claude", CodingCli::ClaudeCode),
    ("codex", CodingCli::Codex),
    ("gemini", CodingCli::GeminiCli),
    ("qwen", CodingCli::QwenCode),
    ("opencode", CodingCli::OpenCode),
    ("aider", CodingCli::Aider),
    ("copilot", CodingCli::CopilotCli),
    ("cursor-agent", CodingCli::CursorAgent),
    ("crush", CodingCli::Crush),
];

/// A command line's program, as a bare name: "C:\x\codex.cmd --help" → "codex".
fn program_of(command: &str) -> String {
    let first = command.split_whitespace().next().unwrap_or("").trim_matches('"');
    let stem = exe_stem(first);
    for ext in [".cmd", ".bat", ".ps1", ".js"] {
        if let Some(s) = stem.strip_suffix(ext) {
            return s.to_string();
        }
    }
    stem
}

/// Which coding CLI a terminal's title shows, from the titles they set on
/// Windows (2026):
///
/// - **Claude Code**: "✳ <session title>" when idle, "◐ …"/"◑ …" while it
///   works (older builds: a braille spinner), "Claude Code" until a session
///   has a title, "claude" (its process title) at start.
/// - **Gemini CLI** (and Qwen Code, its fork): "◇  Ready (folder)", "✦  …",
///   "✋  Action Required", "⏲  Working…", or "Gemini CLI (folder)".
/// - **Codex**: a braille spinner and the project while it works ("⠋ Yap");
///   idle, just the project, which says nothing — so an idle Codex usually
///   counts as a plain shell (lines joined: the safe side).
/// - **OpenCode**: "OpenCode", "OC | <session title>".
/// - Anything else: the program's name as the title, or cmd.exe's
///   "<title> - <command line>" while a program runs.
pub fn coding_cli(title: &str) -> Option<CodingCli> {
    let full = title.trim();
    // An elevated console says so first ("Administrator: ✳ Fix the build").
    let t = match full.split_once(": ") {
        Some((head, rest)) if !head.contains(char::is_whitespace) => rest.trim_start(),
        _ => full,
    };
    for candidate in [full, t] {
        if let Some(cli) = cli_from_title(candidate) {
            return Some(cli);
        }
    }
    None
}

fn cli_from_title(t: &str) -> Option<CodingCli> {
    let lower = t.to_lowercase();
    // Titles that name the tool.
    if lower.contains("claude code") {
        return Some(CodingCli::ClaudeCode);
    }
    if lower.contains("qwen code") {
        return Some(CodingCli::QwenCode);
    }
    if lower.contains("gemini cli") {
        return Some(CodingCli::GeminiCli);
    }
    if lower.contains("github copilot") || lower.contains("copilot cli") {
        return Some(CodingCli::CopilotCli);
    }
    if lower.contains("cursor agent") {
        return Some(CodingCli::CursorAgent);
    }
    if lower == "opencode" || t.starts_with("OC | ") {
        return Some(CodingCli::OpenCode);
    }
    // Status glyphs the tools put first.
    let mut chars = t.chars();
    let first = chars.next()?;
    let rest: String = chars.collect();
    if matches!(first, '✳' | '◐' | '◑') && rest.starts_with(' ') {
        return Some(CodingCli::ClaudeCode);
    }
    if matches!(first, '◇' | '✦' | '✋' | '⏲') && rest.starts_with("  ") {
        return Some(CodingCli::GeminiCli);
    }
    if ('\u{2801}'..='\u{28FF}').contains(&first) && rest.starts_with(' ') {
        return Some(CodingCli::Codex);
    }
    // cmd.exe's "<title> - <command line>", or the program's name as the
    // whole title (a bare name, not a folder: "C:\Projects\codex" is a
    // shell sitting in a folder).
    let program = match t.rsplit_once(" - ") {
        Some((_, command)) => program_of(command),
        None if !t.contains(char::is_whitespace) && !t.contains(['\\', '/']) => program_of(t),
        None => return None,
    };
    CLI_COMMANDS.iter().find(|(name, _)| program == *name).map(|&(_, cli)| cli)
}

/// A browser title showing a chat app's page.
fn title_shows_chat(title: &str) -> bool {
    // Edge writes "Microsoft\u{200b} Edge".
    let mut t = title.replace('\u{200b}', "");
    for sep in [" - ", " – ", " — ", " | ", " • ", " · "] {
        t = t.replace(sep, "\u{1}");
    }
    t.split('\u{1}').any(|seg| {
        let seg = page_name(seg);
        CHAT_PAGES.iter().any(|p| seg.eq_ignore_ascii_case(p))
    })
}

/// A browser, by its process name (`stem`: lower case, no ".exe"): its
/// window's title is the page's (`cleanup_styles.rs` reads the site there).
pub(crate) fn is_browser(stem: &str) -> bool {
    BROWSER_EXES.contains(&stem)
}

/// A title part without unread counts and markers ("(3) WhatsApp", "•
/// Discord") or Edge's "and 2 more pages".
pub(crate) fn page_name(seg: &str) -> &str {
    let mut s = seg.trim();
    loop {
        let before = s;
        s = s.trim_start_matches(['•', '●', '*', '!']).trim_start();
        if let Some(rest) = s.strip_prefix('(') {
            if let Some((count, after)) = rest.split_once(')') {
                if !count.is_empty() && count.chars().all(|c| c.is_ascii_digit() || matches!(c, '+' | '•' | '*')) {
                    s = after.trim_start();
                }
            }
        }
        if s == before {
            break;
        }
    }
    if let Some((page, more)) = s.rsplit_once(" and ") {
        let more = more.trim();
        if more.starts_with(|c: char| c.is_ascii_digit()) && (more.ends_with("more pages") || more.ends_with("more page")) {
            s = page.trim_end();
        }
    }
    s
}

/// An editor with an integrated terminal, by its process name (`stem`:
/// lower case, no ".exe"). Robot mode adds the test's stand-ins
/// (`realtest::stand_in_editors`).
fn is_editor(stem: &str) -> bool {
    EDITOR_EXES.contains(&stem) || crate::shell::realtest::stand_in_editors().iter().any(|e| e == stem)
}

/// What kind of app the window `hwnd` is (`None`: one of Yap's own, or none
/// → `Other`). Reads its process name, class and title; logs the process
/// name and the kind at `debug`, never the title. In an editor with an
/// integrated terminal (VS Code…) it also asks UI Automation what has the
/// focus — the terminal's input or not ([`editor_terminal`]; one
/// time-boxed call, its class and name only). A test run reads nothing
/// (it would be whatever the developer has in front); robot mode, only a
/// window the test opened (`realtest::permits`).
pub fn of_window(hwnd: Option<isize>) -> AppKind {
    let Some(hwnd) = hwnd else {
        return AppKind::Other;
    };
    if !crate::shell::realtest::permits(Some(hwnd)) {
        return AppKind::Other;
    }
    #[cfg(windows)]
    {
        let exe = crate::pasting::text_injector::app_name_for(Some(hwnd));
        let stem = exe_stem(exe.as_deref().unwrap_or(""));
        let title = win::title(hwnd);
        let mut kind = classify(exe.as_deref(), &win::class(hwnd), &title);
        let focused_xterm = || {
            let focused = crate::pasting::text_injector::window_process(hwnd).and_then(crate::pasting::selection::focused_element);
            focused.and_then(|(class, name)| editor_terminal(&class, &name))
        };
        if kind == AppKind::Other && is_editor(&stem) {
            if let Some(t) = focused_xterm() {
                kind = AppKind::Terminal(t);
            }
        } else if kind == AppKind::Other && web_terminal_page(&stem, &title) {
            // vscode.dev, a cloud shell…: xterm.js's input in the page.
            if let Some(t) = focused_xterm() {
                kind = AppKind::Terminal(Terminal { host: Host::Browser, ..t });
            }
        } else if kind == AppKind::Other {
            // Visual Studio's terminal (Windows Terminal's control), ConEmu
            // inside another app: the window with the focus says.
            if let Some(host) = terminal_control(&win::focus_class(hwnd)) {
                kind = AppKind::Terminal(Terminal { coding_cli: None, host });
            }
        }
        tracing::debug!(app = exe.as_deref().unwrap_or("?"), kind = %kind.describe(), "The dictation's app");
        kind
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        AppKind::Other
    }
}

/// The title of the window `hwnd` (Windows' cached caption: never a message
/// to the window), for `cleanup_styles.rs`, which reads the site a browser
/// shows. Never logged. Empty off Windows.
pub(crate) fn window_title(hwnd: isize) -> String {
    #[cfg(windows)]
    {
        win::title(hwnd)
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        String::new()
    }
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    // Declared as notepad.rs / text_injector.rs declare them
    // (`clashing_extern_declarations`).
    #[link(name = "user32")]
    extern "system" {
        fn GetWindowTextW(hwnd: *mut c_void, text: *mut u16, max: i32) -> i32;
        fn GetClassNameW(hwnd: *mut c_void, buf: *mut u16, max: i32) -> i32;
    }

    /// The window's title. For another process's window this reads the
    /// caption Windows keeps; it never sends the window a message, so a hung
    /// app can't hold a dictation up.
    pub fn title(hwnd: isize) -> String {
        let mut buf = [0u16; 512];
        let n = unsafe { GetWindowTextW(hwnd as *mut c_void, buf.as_mut_ptr(), buf.len() as i32) };
        String::from_utf16_lossy(&buf[..n.clamp(0, buf.len() as i32) as usize])
    }

    pub fn class(hwnd: isize) -> String {
        let mut buf = [0u16; 256];
        let n = unsafe { GetClassNameW(hwnd as *mut c_void, buf.as_mut_ptr(), buf.len() as i32) };
        String::from_utf16_lossy(&buf[..n.clamp(0, buf.len() as i32) as usize])
    }

    /// The class of the window with the keyboard focus in `hwnd`'s thread
    /// (`GetGUIThreadInfo`: no message to the app), empty when none.
    pub fn focus_class(hwnd: isize) -> String {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO};
        unsafe {
            let thread = GetWindowThreadProcessId(HWND(hwnd as *mut c_void), None);
            if thread == 0 {
                return String::new();
            }
            let mut info: GUITHREADINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
            if GetGUIThreadInfo(thread, &mut info).is_err() || info.hwndFocus.is_invalid() {
                return String::new();
            }
            class(info.hwndFocus.0 as isize)
        }
    }
}

// ---- Terminals ----

/// The dictation as a terminal should get it. Line breaks of every kind
/// become `\n`, tabs a space (a tab at a shell prompt completes a file
/// name), other control characters go (an escape or Ctrl+C mid-paste is
/// never wanted). Then, unless `keep_lines`, it all goes on one line
/// ([`join_lines`]). Trimmed: no space before or after it.
pub fn terminal_text(text: &str, keep_lines: bool) -> String {
    let mut clean = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                clean.push('\n');
            }
            '\n' | '\u{0b}' | '\u{0c}' | '\u{85}' | '\u{2028}' | '\u{2029}' => clean.push('\n'),
            '\t' => clean.push(' '),
            c if c.is_control() => {}
            c => clean.push(c),
        }
    }
    let out = if keep_lines {
        clean.split('\n').map(str::trim_end).collect::<Vec<_>>().join("\n")
    } else {
        join_lines(&clean)
    };
    out.trim().to_string()
}

/// A bullet at the start of a line ("- ", "• ", "* "…): the text after it.
fn strip_bullet(line: &str) -> Option<&str> {
    let mut chars = line.chars();
    let first = chars.next()?;
    if !matches!(first, '-' | '*' | '+' | '•' | '–' | '—' | '◦' | '‣' | '▪' | '·') {
        return None;
    }
    let rest = chars.as_str();
    // A marker on a line of its own: nothing to keep.
    (rest.is_empty() || rest.starts_with(char::is_whitespace)).then(|| rest.trim_start())
}

/// A numbered list item: "1. ", "2) ", "(3) ", "a) ".
fn is_numbered(line: &str) -> bool {
    let (inner, close) = match line.strip_prefix('(') {
        Some(rest) => (rest, ')'),
        None => (line, '\0'),
    };
    let digits = inner.chars().take_while(char::is_ascii_digit).count();
    let label = if digits > 0 && digits <= 3 {
        digits
    } else if inner.starts_with(|c: char| c.is_ascii_lowercase()) {
        1
    } else {
        return false;
    };
    let mut after = inner[label..].chars();
    let mark = after.next();
    let ok_mark = match close {
        ')' => mark == Some(')'),
        _ => mark == Some(')') || (digits > 0 && mark == Some('.')),
    };
    ok_mark && after.next().is_some_and(char::is_whitespace)
}

/// Several lines as one: each line trimmed, empty ones dropped. A bullet
/// list reads as a run of items — "Fix these: the login bug; the search"
/// — its markers gone (numbers stay: "1. build it; 2. ship it"); other
/// lines are joined with a space.
fn join_lines(text: &str) -> String {
    let mut out = String::new();
    let mut prev_item = false;
    for line in text.split('\n') {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (body, item) = match strip_bullet(line) {
            Some(rest) => (rest, true),
            None => (line, is_numbered(line)),
        };
        if body.is_empty() {
            continue;
        }
        if !out.is_empty() {
            let closed = out.ends_with(['.', '!', '?', '…', ':', ';', ',']);
            let sep = match (closed, item, prev_item) {
                (false, true, true) => "; ",
                (false, true, false) => ": ",
                _ => " ",
            };
            out.push_str(sep);
        }
        out.push_str(body);
        prev_item = item;
    }
    out
}

/// The key to press after the paste, if any (`key`: the configured
/// `auto_submit_key`). In a terminal: Enter, and only when "press enter"
/// was said (`voice_enter`), or with auto-submit on in a coding CLI (it
/// sends a prompt there) — never at a plain shell's prompt, where it would
/// run whatever was dictated. Elsewhere: as configured.
pub fn submit_key(kind: AppKind, auto_submit: bool, voice_enter: bool, key: &str) -> Option<&str> {
    match kind {
        AppKind::Terminal(t) => (voice_enter || (auto_submit && t.coding_cli.is_some())).then_some("enter"),
        _ => (auto_submit || voice_enter).then_some(key),
    }
}

// ---- Chat apps ----

/// Words whose full stop is part of them, so it stays (the end of "etc.").
const ABBREVIATIONS: &[&str] = &[
    "etc", "vs", "approx", "incl", "misc", "inc", "ltd", "co", "corp", "jr", "sr", "mr", "mrs", "ms", "dr", "prof",
    "st", "cf", "viz", "al", "esp", "dept",
];

/// The word just before a full stop at byte `dot` of `s` (back to the
/// last space: past all of it, a no-break space being two bytes).
pub(crate) fn word_before(s: &str, dot: usize) -> &str {
    let head = &s[..dot];
    let start = head
        .char_indices()
        .rfind(|&(_, c)| c.is_whitespace())
        .map_or(0, |(i, c)| i + c.len_utf8());
    head[start..].trim_start_matches(['(', '"', '\'', '“', '‘'])
}

/// "e.g", "a.m", "U.S", "Ph.D": letters in short parts between dots.
fn dotted_abbreviation(word: &str) -> bool {
    word.contains('.')
        && word
            .split('.')
            .all(|part| (1..=2).contains(&part.chars().count()) && part.chars().all(char::is_alphabetic))
}

/// A word whose full stop belongs to it ("etc.", "a.m.", "Dr.").
pub(crate) fn is_abbreviation(word: &str) -> bool {
    dotted_abbreviation(word) || ABBREVIATIONS.contains(&word.to_lowercase().as_str())
}

/// How many sentences `s` has: runs of `.?!…` followed by a space or the
/// end, not counting the full stop of an abbreviation or an initial ("J.
/// Smith").
fn sentence_count(s: &str) -> usize {
    let s = s.trim();
    let mut count = 0;
    let mut in_sentence = false;
    let mut iter = s.char_indices().peekable();
    while let Some((i, c)) = iter.next() {
        if matches!(c, '.' | '?' | '!' | '…') {
            let mut single_dot = c == '.';
            // The run, with any quote or bracket closing it.
            while let Some(&(_, n)) = iter.peek() {
                if matches!(n, '.' | '?' | '!' | '…') {
                    single_dot = false;
                } else if !matches!(n, '"' | '\'' | '”' | '’' | ')' | ']') {
                    break;
                }
                iter.next();
            }
            let ends = iter.peek().is_none_or(|&(_, n)| n.is_whitespace());
            let word = word_before(s, i);
            let initial = word.chars().count() == 1 && word.chars().all(char::is_uppercase);
            if ends && in_sentence && !(single_dot && (is_abbreviation(word) || initial)) {
                count += 1;
                in_sentence = false;
            }
            continue;
        }
        if c.is_alphanumeric() {
            in_sentence = true;
        }
    }
    count + usize::from(in_sentence)
}

/// A chat message doesn't end with a full stop: drop the final "." of a
/// dictation into a chat app when it's the end of a message of one or two
/// sentences in one paragraph. `ctx` (the text around the cursor, when Yap
/// could read it) decides whether it is: nothing after the cursor, no
/// selection being replaced (its own ending decides, `smart_insert`), and
/// the draft before it counted in. Never touched: "?", "!", "…", "...", the
/// full stop of "etc."/"a.m.", or a snippet saved ending with one.
pub fn drop_chat_full_stop(text: &str, ctx: Option<&FieldContext>, dictionary: &[DictionaryEntry]) -> String {
    let body = text.trim_end();
    if !body.ends_with('.') || body.ends_with("..") || body.contains(['\n', '\r']) {
        return text.to_string();
    }
    let message = match ctx {
        Some(c) => {
            if !c.selection.trim().is_empty() || !c.after.trim().is_empty() {
                return text.to_string();
            }
            // A line break in the draft: a message of several paragraphs.
            let before = c.before.trim_start();
            if before.contains(['\n', '\r']) {
                return text.to_string();
            }
            format!("{} {}", before.trim_end(), body.trim_start())
        }
        None => body.trim_start().to_string(),
    };
    let dot = body.len() - 1;
    let last = word_before(body, dot);
    if !last.is_empty() && is_abbreviation(last) {
        return text.to_string();
    }
    let snippet_ends_so = dictionary.iter().any(|e| {
        let to = e.to.trim();
        e.snippet && to.ends_with('.') && body.ends_with(to)
    });
    if snippet_ends_so || sentence_count(&message) > 2 {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    out.push_str(&body[..dot]);
    out.push_str(&text[body.len()..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn term(cli: Option<CodingCli>, wt: bool) -> AppKind {
        on(cli, if wt { Host::WindowsTerminal } else { Host::Console })
    }

    fn on(cli: Option<CodingCli>, host: Host) -> AppKind {
        AppKind::Terminal(Terminal { coding_cli: cli, host })
    }

    #[test]
    fn terminals_by_process_and_class() {
        let wt = "CASCADIA_HOSTING_WINDOW_CLASS";
        assert_eq!(classify(Some("WindowsTerminal.exe"), wt, "Windows PowerShell"), term(None, true));
        assert_eq!(classify(Some("C:\\x\\WindowsTerminal.exe"), "", "pwsh"), term(None, true));
        assert_eq!(classify(Some("cmd.exe"), "ConsoleWindowClass", "Command Prompt"), term(None, false));
        assert_eq!(classify(Some("powershell.exe"), "ConsoleWindowClass", "Windows PowerShell"), term(None, false));
        assert_eq!(classify(Some("pwsh.exe"), "", "PowerShell 7"), term(None, false));
        assert_eq!(classify(Some("mintty.exe"), "mintty", "MINGW64:/c/Users/sam"), on(None, Host::Mintty));
        assert_eq!(classify(Some("wezterm-gui.exe"), "org.wezfurlong.wezterm", "pwsh"), on(None, Host::WezTerm));
        assert_eq!(classify(Some("alacritty.exe"), "Window Class", "Alacritty"), on(None, Host::Alacritty));
        assert_eq!(classify(Some("Tabby.exe"), "Chrome_WidgetWin_1", "PowerShell"), on(None, Host::Tabby));
        assert_eq!(classify(Some("Hyper.exe"), "Chrome_WidgetWin_1", "pwsh"), on(None, Host::Hyper));
        assert_eq!(classify(Some("putty.exe"), "PuTTY", "sam@box: ~"), on(None, Host::Putty));
        assert_eq!(classify(Some("kitty.exe"), "KiTTY", "sam@box: ~"), on(None, Host::Putty));
        assert_eq!(classify(Some("MobaXterm.exe"), "TMobaXtermForm", "sam@box"), on(None, Host::MobaXterm));
        assert_eq!(classify(Some("rio.exe"), "Window Class", "pwsh"), on(None, Host::Rio));
        assert_eq!(classify(Some("ConEmu64.exe"), "VirtualConsoleClass", "cmd"), on(None, Host::ConEmu));
        assert_eq!(classify(Some("warp.exe"), "Window Class", "Warp"), term(None, false));
        // A classic console window can report the program running in it.
        assert_eq!(
            classify(Some("claude.exe"), "ConsoleWindowClass", "✳ Claude Code"),
            term(Some(CodingCli::ClaudeCode), false)
        );
        // mintty's class wins whatever the process says (a shortcut's own name).
        assert_eq!(classify(Some("git-bash.exe"), "mintty", "MINGW64:~"), on(None, Host::Mintty));
        // …but claude.exe outside a console is Claude's desktop app.
        assert_eq!(classify(Some("claude.exe"), "Chrome_WidgetWin_1", "Claude"), AppKind::Other);
        assert_eq!(classify(Some("Code.exe"), "Chrome_WidgetWin_1", "main.rs - Yap - Visual Studio Code"), AppKind::Other);
        assert_eq!(classify(None, "", ""), AppKind::Other);
    }

    #[test]
    fn coding_clis_by_the_titles_they_set() {
        let cli = coding_cli;
        // Claude Code: idle, working (both spinner frames), untitled, starting, elevated.
        assert_eq!(cli("✳ Fix the login bug"), Some(CodingCli::ClaudeCode));
        assert_eq!(cli("◐ Fix the login bug"), Some(CodingCli::ClaudeCode));
        assert_eq!(cli("◑ Fix the login bug"), Some(CodingCli::ClaudeCode));
        assert_eq!(cli("✳ Claude Code"), Some(CodingCli::ClaudeCode));
        assert_eq!(cli("Claude Code"), Some(CodingCli::ClaudeCode));
        assert_eq!(cli("claude"), Some(CodingCli::ClaudeCode));
        assert_eq!(cli("Administrator: ✳ Fix the build"), Some(CodingCli::ClaudeCode));
        // Gemini CLI and Qwen Code.
        assert_eq!(cli("◇  Ready (Yap)"), Some(CodingCli::GeminiCli));
        assert_eq!(cli("✦  Reading the config (Yap)"), Some(CodingCli::GeminiCli));
        assert_eq!(cli("✋  Action Required (Yap)"), Some(CodingCli::GeminiCli));
        assert_eq!(cli("⏲  Working… (Yap)"), Some(CodingCli::GeminiCli));
        assert_eq!(cli("Gemini CLI (Yap)"), Some(CodingCli::GeminiCli));
        assert_eq!(cli("Qwen Code (Yap)"), Some(CodingCli::QwenCode));
        // Codex while it works (a braille spinner + the project).
        assert_eq!(cli("⠋ Yap"), Some(CodingCli::Codex));
        assert_eq!(cli("⠼ Yap"), Some(CodingCli::Codex));
        // OpenCode.
        assert_eq!(cli("OpenCode"), Some(CodingCli::OpenCode));
        assert_eq!(cli("OC | Fix the login bug"), Some(CodingCli::OpenCode));
        // The program as the title, or cmd.exe's running command.
        assert_eq!(cli("codex"), Some(CodingCli::Codex));
        assert_eq!(cli("aider.exe"), Some(CodingCli::Aider));
        assert_eq!(cli("Command Prompt - aider --model sonnet"), Some(CodingCli::Aider));
        assert_eq!(cli("C:\\WINDOWS\\system32\\cmd.exe - codex"), Some(CodingCli::Codex));
        assert_eq!(cli("Command Prompt - C:\\Users\\sam\\AppData\\Roaming\\npm\\gemini.cmd"), Some(CodingCli::GeminiCli));
        assert_eq!(cli("Command Prompt - cursor-agent"), Some(CodingCli::CursorAgent));
        // A plain shell, and folders that happen to share a name: nothing.
        assert_eq!(cli("Windows PowerShell"), None);
        assert_eq!(cli("C:\\Projects\\codex"), None);
        assert_eq!(cli("MINGW64:/c/Projects/claude-notes"), None);
        assert_eq!(cli("Yap"), None);
        assert_eq!(cli("Command Prompt - git status"), None);
        assert_eq!(cli("fix codex config"), None);
        assert_eq!(cli(""), None);
    }

    #[test]
    fn only_claude_code_and_codex_in_windows_terminal_keep_their_lines() {
        let t = |cli, host| Terminal { coding_cli: cli, host };
        assert!(t(Some(CodingCli::ClaudeCode), Host::WindowsTerminal).keeps_lines());
        assert!(t(Some(CodingCli::Codex), Host::WindowsTerminal).keeps_lines());
        assert!(!t(Some(CodingCli::ClaudeCode), Host::Console).keeps_lines());
        assert!(!t(Some(CodingCli::ClaudeCode), Host::Editor).keeps_lines());
        assert!(!t(Some(CodingCli::ClaudeCode), Host::Mintty).keeps_lines());
        assert!(!t(Some(CodingCli::GeminiCli), Host::WindowsTerminal).keeps_lines());
        assert!(!t(Some(CodingCli::Aider), Host::WindowsTerminal).keeps_lines());
        assert!(!t(None, Host::WindowsTerminal).keeps_lines());
    }

    #[test]
    fn each_terminal_pastes_with_its_own_keys() {
        let key = |exe: &str, class: &str| classify(Some(exe), class, "").paste_key();
        // Shift+Insert: Ctrl+V would reach the shell as ^V.
        assert_eq!(key("mintty.exe", "mintty"), PasteKey::ShiftInsert); // Git Bash, MSYS2, Cygwin
        assert_eq!(key("putty.exe", "PuTTY"), PasteKey::ShiftInsert);
        assert_eq!(key("kitty.exe", "KiTTY"), PasteKey::ShiftInsert);
        assert_eq!(key("MobaXterm.exe", "TMobaXtermForm"), PasteKey::ShiftInsert);
        // Ctrl+Shift+V: their clipboard paste (Shift+Insert is the primary selection).
        assert_eq!(key("alacritty.exe", "Window Class"), PasteKey::CtrlShiftV);
        assert_eq!(key("wezterm-gui.exe", "org.wezfurlong.wezterm"), PasteKey::CtrlShiftV);
        assert_eq!(key("Tabby.exe", "Chrome_WidgetWin_1"), PasteKey::CtrlShiftV);
        assert_eq!(key("Hyper.exe", "Chrome_WidgetWin_1"), PasteKey::CtrlShiftV);
        assert_eq!(key("rio.exe", "Window Class"), PasteKey::CtrlShiftV);
        // Ctrl+V where it pastes, and wherever Yap doesn't know better.
        assert_eq!(key("WindowsTerminal.exe", "CASCADIA_HOSTING_WINDOW_CLASS"), PasteKey::CtrlV);
        assert_eq!(key("ConEmu64.exe", "VirtualConsoleClass"), PasteKey::CtrlV);
        // A classic console window: its own Edit → Paste (a raw keyboard
        // program there would get Ctrl+V as ^V); Ctrl+V at paste time when
        // the window isn't one after all (a terminal known by name).
        assert_eq!(key("cmd.exe", "ConsoleWindowClass"), PasteKey::ConsoleMenu);
        assert_eq!(key("claude.exe", "ConsoleWindowClass"), PasteKey::ConsoleMenu);
        assert_eq!(key("warp.exe", "Window Class"), PasteKey::ConsoleMenu);
        assert_eq!(on(None, Host::Editor).paste_key(), PasteKey::CtrlV);
        // Every other app.
        assert_eq!(key("notepad.exe", "Notepad"), PasteKey::CtrlV);
        assert_eq!(key("slack.exe", "Chrome_WidgetWin_1"), PasteKey::CtrlV);
        assert_eq!(AppKind::Other.paste_key(), PasteKey::CtrlV);
        // The settings don't change the keys: "Safer pasting" off, mintty still wants Shift+Insert.
        assert_eq!(on(None, Host::Mintty).with_settings(false, true), AppKind::Other);
        assert_eq!(on(None, Host::Mintty).paste_key().name(), "Shift+Insert");
    }

    /// What VS Code's integrated terminal reports over UI Automation, from
    /// its source: xterm.js's textarea (class `xterm-helper-textarea`)
    /// labelled `"Terminal {0}, {1}"` (`terminalTextBoxAriaLabelNumberAndTitle`)
    /// or `"Terminal {0}"`, then the screen-reader help lines.
    #[test]
    fn an_editors_integrated_terminal_by_its_focused_input() {
        let help = "\nRun the command: Toggle Screen Reader Accessibility Mode for an optimized screen reader experience\nUse Alt+F1 for terminal accessibility help";
        let input = "xterm-helper-textarea";
        let t = |name: &str| editor_terminal(input, name);
        assert_eq!(t(&format!("Terminal 1, pwsh{help}")), Some(Terminal { coding_cli: None, host: Host::Editor }));
        assert_eq!(t("Terminal 1"), Some(Terminal { coding_cli: None, host: Host::Editor }));
        assert_eq!(t(""), Some(Terminal { coding_cli: None, host: Host::Editor }));
        // A coding CLI in it, by the title it set or its process name.
        let claude = Some(Terminal { coding_cli: Some(CodingCli::ClaudeCode), host: Host::Editor });
        assert_eq!(t(&format!("Terminal 2, claude{help}")), claude);
        assert_eq!(t(&format!("Terminal 3, ✳ Fix the login bug{help}")), claude);
        assert_eq!(t("Terminal 12, ◐ Fix the login bug"), claude);
        assert_eq!(t("Terminal 1, codex").unwrap().coding_cli, Some(CodingCli::Codex));
        assert_eq!(t("Terminal 1, ◇  Ready (Yap)").unwrap().coding_cli, Some(CodingCli::GeminiCli));
        // Translated labels: the number and the title stay.
        assert_eq!(t("ターミナル 1、claude"), claude);
        assert_eq!(t("Терминал 2, claude"), claude);
        // As Chromium gives the label (found by the robot, 2026-10-21): its
        // line breaks folded into spaces, the help on the title's line.
        let flat = |title: &str| t(&format!("Terminal 1, {title}{}", help.replace('\n', " ")));
        assert_eq!(flat("claude"), claude);
        assert_eq!(flat("✳ Fix the login bug"), claude);
        assert_eq!(flat("codex").unwrap().coding_cli, Some(CodingCli::Codex));
        assert_eq!(flat("Claude Code"), claude);
        assert_eq!(flat("pwsh").unwrap().coding_cli, None);
        assert_eq!(flat("C:\\Projects\\claude").unwrap().coding_cli, None);
        assert_eq!(flat("claude-notes").unwrap().coding_cli, None);
        // A shell sitting in a folder named after a CLI: just a shell.
        assert_eq!(t("Terminal 1, C:\\Projects\\claude").unwrap().coding_cli, None);
        // The class may carry others beside it.
        assert!(editor_terminal("xterm-helper-textarea focused", "Terminal 1").is_some());
        // The editor itself (Monaco's input, or its EditContext), a chat box: not a terminal.
        assert_eq!(editor_terminal("inputarea monaco-mouse-cursor-text", "Editor content;Press Alt+F1 for Accessibility Options."), None);
        assert_eq!(editor_terminal("native-edit-context", "The editor is not accessible at this time."), None);
        assert_eq!(editor_terminal("", "Terminal 1, pwsh"), None);
        assert_eq!(editor_terminal("xterm-helper-textarea-ish", "Terminal 1, pwsh"), None);
        assert_eq!(on(None, Host::Editor).describe(), "terminal (an editor's terminal)");
        assert_eq!(on(Some(CodingCli::ClaudeCode), Host::Editor).describe(), "terminal (Claude Code in an editor's terminal)");
    }

    #[test]
    fn a_browsers_terminal_page_and_visual_studios_terminal() {
        // Pages that may hold a terminal: only these are asked what has the focus.
        for title in [
            "main.rs - yap [Codespaces: fluffy-fiesta] - Visual Studio Code - Google Chrome",
            "README.md - github.dev - Personal - Microsoft\u{200b} Edge",
            "Welcome - Visual Studio Code - vscode.dev",
            "Terminal 1 - JupyterLab - Google Chrome",
            "Cloud Shell - Google Chrome",
        ] {
            assert!(web_terminal_page("chrome", title), "{title}");
        }
        assert!(!web_terminal_page("chrome", "Inbox (3) - sam@example.com - Gmail - Google Chrome"));
        assert!(!web_terminal_page("notepad", "terminal notes.txt - Notepad"));
        // xterm.js's input in the page: a terminal, Ctrl+V, its coding CLI by VS Code's label.
        let t = editor_terminal("xterm-helper-textarea", "Terminal 1, claude").unwrap();
        let web = AppKind::Terminal(Terminal { host: Host::Browser, ..t });
        assert_eq!(web.paste_key(), PasteKey::CtrlV);
        assert_eq!(web.describe(), "terminal (Claude Code in a browser's terminal)");
        if let AppKind::Terminal(t) = web {
            assert!(!t.keeps_lines(), "one line in a browser's terminal until it's known safe");
        }
        // Windows Terminal's control in Visual Studio (or any app hosting it), ConEmu inside another app.
        assert_eq!(terminal_control("HwndTerminalClass"), Some(Host::VisualStudio));
        assert_eq!(terminal_control("VirtualConsoleClass"), Some(Host::ConEmu));
        assert_eq!(terminal_control("WindowsForms10.EDIT.app.0.141b42a_r6_ad1"), None);
        assert_eq!(terminal_control(""), None);
        assert_eq!(Host::VisualStudio.paste_key(), PasteKey::CtrlV);
    }

    #[test]
    fn editors_with_an_integrated_terminal() {
        for exe in ["Code.exe", "Code - Insiders.exe", "VSCodium.exe", "Cursor.exe", "Windsurf.exe"] {
            assert!(is_editor(&exe_stem(exe)), "{exe}");
        }
        assert!(!is_editor(&exe_stem("msedge.exe")));
        assert!(!is_editor(&exe_stem("notepad.exe")));
        // The editor's window is "other" until its focused element says otherwise.
        assert_eq!(classify(Some("Code.exe"), "Chrome_WidgetWin_1", "main.rs - Yap - Visual Studio Code"), AppKind::Other);
    }

    #[test]
    fn chat_apps_by_process_or_browser_title() {
        for exe in ["slack.exe", "Discord.exe", "WhatsApp.exe", "Telegram.exe", "Signal.exe", "ms-teams.exe", "Teams.exe"] {
            assert_eq!(classify(Some(exe), "Chrome_WidgetWin_1", "anything"), AppKind::Messaging, "{exe}");
        }
        let browser = |exe: &str, title: &str| classify(Some(exe), "Chrome_WidgetWin_1", title);
        assert_eq!(browser("chrome.exe", "(3) WhatsApp - Google Chrome"), AppKind::Messaging);
        assert_eq!(browser("chrome.exe", "WhatsApp"), AppKind::Messaging); // an installed web app
        assert_eq!(browser("msedge.exe", "WhatsApp and 2 more pages - Personal - Microsoft\u{200b} Edge"), AppKind::Messaging);
        assert_eq!(browser("firefox.exe", "general (Channel) - Acme - Slack — Mozilla Firefox"), AppKind::Messaging);
        assert_eq!(browser("chrome.exe", "• Discord | #general | Yap fans - Google Chrome"), AppKind::Messaging);
        assert_eq!(browser("chrome.exe", "(1) Discord | #general | Yap fans - Google Chrome"), AppKind::Messaging);
        assert_eq!(browser("msedge.exe", "Chat | Sam Lee | Microsoft Teams - Work - Microsoft\u{200b} Edge"), AppKind::Messaging);
        assert_eq!(browser("brave.exe", "Messenger | Facebook - Brave"), AppKind::Messaging);
        assert_eq!(browser("chrome.exe", "Telegram Web - Google Chrome"), AppKind::Messaging);
        // Pages that only mention one.
        assert_eq!(browser("chrome.exe", "How to use Slack - YouTube - Google Chrome"), AppKind::Other);
        assert_eq!(browser("chrome.exe", "Discord Nitro pricing explained - Google Chrome"), AppKind::Other);
        assert_eq!(browser("chrome.exe", "Inbox (3) - sam@example.com - Gmail - Google Chrome"), AppKind::Other);
        // A chat page's title in a non-browser app is just a title.
        assert_eq!(classify(Some("notepad.exe"), "Notepad", "WhatsApp.txt - Notepad"), AppKind::Other);
    }

    #[test]
    fn settings_switch_the_kinds_off() {
        let t = term(None, true);
        assert_eq!(t.with_settings(true, true), t);
        assert_eq!(t.with_settings(false, true), AppKind::Other);
        assert_eq!(AppKind::Messaging.with_settings(true, false), AppKind::Other);
        assert_eq!(AppKind::Messaging.with_settings(false, true), AppKind::Messaging);
        assert_eq!(AppKind::Other.with_settings(true, true), AppKind::Other);
        assert_eq!(term(Some(CodingCli::ClaudeCode), true).describe(), "terminal (Claude Code in Windows Terminal)");
    }

    #[test]
    fn a_shell_gets_one_line() {
        assert_eq!(terminal_text("Hello there.", false), "Hello there.");
        assert_eq!(terminal_text("  Hello there.  ", false), "Hello there.");
        assert_eq!(terminal_text("First line.\nSecond line.", false), "First line. Second line.");
        assert_eq!(terminal_text("First line.\r\n\r\nSecond line.\r", false), "First line. Second line.");
        assert_eq!(terminal_text("Hi Sam,\n\nThanks for this.", false), "Hi Sam, Thanks for this.");
        assert_eq!(terminal_text("Best\nImogen", false), "Best Imogen");
        assert_eq!(terminal_text("one\u{2028}two\u{2029}three", false), "one two three");
    }

    #[test]
    fn a_list_folds_into_one_line() {
        assert_eq!(
            terminal_text("Things to fix:\n- the login bug\n- the slow search", false),
            "Things to fix: the login bug; the slow search"
        );
        assert_eq!(terminal_text("Shopping\n* milk\n* eggs\n* bread", false), "Shopping: milk; eggs; bread");
        assert_eq!(terminal_text("• Done.\n• Shipped.", false), "Done. Shipped.");
        assert_eq!(terminal_text("Steps:\n1. Build it.\n2. Test it.", false), "Steps: 1. Build it. 2. Test it.");
        assert_eq!(terminal_text("1. build it\n2) test it\n(3) ship it", false), "1. build it; 2) test it; (3) ship it");
        assert_eq!(terminal_text("Options\na) red\nb) blue", false), "Options: a) red; b) blue");
        // Not list markers.
        assert_eq!(terminal_text("-5 degrees\n2024 was good", false), "-5 degrees 2024 was good");
        assert_eq!(terminal_text("Use git\n-\nthen push", false), "Use git then push");
    }

    #[test]
    fn nothing_that_would_misbehave_reaches_a_terminal() {
        // A tab completes at a prompt; control characters are never wanted.
        assert_eq!(terminal_text("echo\tone\ttwo", false), "echo one two");
        assert_eq!(terminal_text("a\u{1b}[31mb\u{3}c\u{7f}d\u{85}e", false), "a[31mbcd e");
        assert_eq!(terminal_text("tabs\tin\nlines", true), "tabs in\nlines");
    }

    #[test]
    fn a_coding_cli_that_takes_a_paste_keeps_its_lines() {
        assert_eq!(
            terminal_text("Fix these:\r\n- the login bug   \n- the slow search\n", true),
            "Fix these:\n- the login bug\n- the slow search"
        );
        assert_eq!(terminal_text("\n\nOne.\n\nTwo.\n", true), "One.\n\nTwo.");
    }

    #[test]
    fn enter_at_a_shell_only_when_asked_for() {
        let shell = term(None, true);
        let claude = term(Some(CodingCli::ClaudeCode), true);
        // A plain shell: never auto-submit; "press enter" presses Enter, whatever key is set.
        assert_eq!(submit_key(shell, true, false, "ctrlEnter"), None);
        assert_eq!(submit_key(shell, false, true, "shiftEnter"), Some("enter"));
        assert_eq!(submit_key(shell, false, false, "enter"), None);
        // A coding CLI: auto-submit sends the prompt.
        assert_eq!(submit_key(claude, true, false, "ctrlEnter"), Some("enter"));
        assert_eq!(submit_key(claude, false, false, "enter"), None);
        // Anything else: as configured.
        assert_eq!(submit_key(AppKind::Other, true, false, "ctrlEnter"), Some("ctrlEnter"));
        assert_eq!(submit_key(AppKind::Messaging, false, true, "shiftEnter"), Some("shiftEnter"));
        assert_eq!(submit_key(AppKind::Other, false, false, "enter"), None);
    }

    fn snippet(from: &str, to: &str) -> DictionaryEntry {
        serde_json::from_value(serde_json::json!({ "from": from, "to": to, "snippet": true })).unwrap()
    }

    fn chat(text: &str) -> String {
        drop_chat_full_stop(text, None, &[])
    }

    #[test]
    fn a_short_chat_message_loses_its_full_stop() {
        assert_eq!(chat("Sounds good."), "Sounds good");
        assert_eq!(chat("Sounds good. See you then."), "Sounds good. See you then");
        assert_eq!(chat("Meet at 3 p.m. tomorrow."), "Meet at 3 p.m. tomorrow");
        assert_eq!(chat("Ask Dr. Smith, J. Lee and Mr. Brown."), "Ask Dr. Smith, J. Lee and Mr. Brown");
        assert_eq!(chat("It's on example.com."), "It's on example.com");
        assert_eq!(chat("See https://example.com/docs."), "See https://example.com/docs");
        assert_eq!(chat("Version 1.2 is out."), "Version 1.2 is out");
        assert_eq!(chat("He said \"fine.\" Then left."), "He said \"fine.\" Then left");
        // Spaces around it are kept as they were.
        assert_eq!(chat("Sounds good. "), "Sounds good ");
    }

    #[test]
    fn a_wide_space_before_the_last_word_is_one_space() {
        // No-break (U+00A0), narrow no-break (U+202F) and ideographic
        // (U+3000) spaces are several bytes: the word after one starts past
        // all of them (web editors' drafts, French cleanup's "15 h.").
        for space in ['\u{a0}', '\u{202f}', '\u{3000}'] {
            let text = format!("See you at 15{space}h.");
            assert_eq!(word_before(&text, text.len() - 1), "h");
            assert_eq!(chat(&text), format!("See you at 15{space}h"));
            let draft = FieldContext { before: format!("Hi{space}Bob."), after: String::new(), selection: String::new() };
            assert_eq!(drop_chat_full_stop("See you.", Some(&draft), &[]), "See you");
        }
    }

    #[test]
    fn a_chat_message_keeps_what_isnt_a_plain_full_stop() {
        assert_eq!(chat("Are you coming?"), "Are you coming?");
        assert_eq!(chat("Great news!"), "Great news!");
        assert_eq!(chat("Well…"), "Well…");
        assert_eq!(chat("Well..."), "Well...");
        assert_eq!(chat("Bring snacks, drinks, etc."), "Bring snacks, drinks, etc.");
        assert_eq!(chat("See you at 9 a.m."), "See you at 9 a.m.");
        assert_eq!(chat("Yes"), "Yes");
        assert_eq!(chat(""), "");
        // Three sentences, or more than one paragraph: a proper message.
        assert_eq!(chat("Thanks. That works. See you."), "Thanks. That works. See you.");
        assert_eq!(chat("Hi Sam,\nSee you then."), "Hi Sam,\nSee you then.");
        // A snippet saved ending with a full stop ends as saved.
        let dict = vec![snippet("my disclaimer", "Not financial advice.")];
        assert_eq!(drop_chat_full_stop("Buy it. Not financial advice.", None, &dict), "Buy it. Not financial advice.");
        assert_eq!(drop_chat_full_stop("Buy it.", None, &dict), "Buy it");
    }

    #[test]
    fn the_draft_around_the_cursor_counts() {
        let ctx = |before: &str, after: &str, selection: &str| FieldContext {
            before: before.into(),
            after: after.into(),
            selection: selection.into(),
        };
        // The end of a short draft.
        assert_eq!(drop_chat_full_stop("See you then.", Some(&ctx("Sounds good.", "", "")), &[]), "See you then");
        // A third sentence: a proper message, the full stop stays.
        assert_eq!(drop_chat_full_stop("See you.", Some(&ctx("Thanks. That works.", "", "")), &[]), "See you.");
        // More of the message after the cursor, or a draft of several paragraphs.
        assert_eq!(drop_chat_full_stop("Sure.", Some(&ctx("", " Thanks!", "")), &[]), "Sure.");
        assert_eq!(drop_chat_full_stop("Sure.", Some(&ctx("Hi,\n", "", "")), &[]), "Sure.");
        // Replacing a selection: its own ending decides (smart_insert).
        assert_eq!(drop_chat_full_stop("Sure.", Some(&ctx("", "", "Maybe.")), &[]), "Sure.");
    }

    /// The real thing, read-only: every visible top-level window on this PC
    /// with its process, class and kind (titles too with
    /// `YAP_APP_KIND_TITLES=1`). `cargo test --lib -- --ignored
    /// this_machines_windows --nocapture`.
    #[cfg(windows)]
    #[test]
    #[ignore = "reads this PC's windows"]
    fn this_machines_windows() {
        use std::ffi::c_void;
        #[link(name = "user32")]
        extern "system" {
            fn EnumWindows(callback: unsafe extern "system" fn(*mut c_void, isize) -> i32, lparam: isize) -> i32;
            fn IsWindowVisible(hwnd: *mut c_void) -> i32;
        }
        unsafe extern "system" fn collect(hwnd: *mut c_void, lparam: isize) -> i32 {
            if IsWindowVisible(hwnd) != 0 {
                (*(lparam as *mut Vec<isize>)).push(hwnd as isize);
            }
            1
        }
        let mut hwnds: Vec<isize> = Vec::new();
        unsafe { EnumWindows(collect, &mut hwnds as *mut Vec<isize> as isize) };
        let titles = std::env::var("YAP_APP_KIND_TITLES").is_ok();
        for hwnd in hwnds {
            let title = win::title(hwnd);
            if title.is_empty() {
                continue;
            }
            let exe = crate::pasting::text_injector::app_name_for(Some(hwnd));
            let class = win::class(hwnd);
            let kind = classify(exe.as_deref(), &class, &title);
            let shown = if titles { title.as_str() } else { "" };
            println!("{:<28} {:<36} {:<44} {shown}", exe.unwrap_or_default(), class, kind.describe());
        }
    }

    #[test]
    fn sentences_are_counted_around_abbreviations() {
        assert_eq!(sentence_count("One."), 1);
        assert_eq!(sentence_count("One. Two."), 2);
        assert_eq!(sentence_count("One. Two"), 2);
        assert_eq!(sentence_count("One? Two! Three."), 3);
        assert_eq!(sentence_count("Wait... what?"), 2);
        assert_eq!(sentence_count("Use e.g. this or i.e. that."), 1);
        assert_eq!(sentence_count("J. R. R. Tolkien wrote it."), 1);
        assert_eq!(sentence_count("It costs 3.50 now."), 1);
        assert_eq!(sentence_count("\"Stop.\" She left."), 2);
        assert_eq!(sentence_count(""), 0);
    }
}

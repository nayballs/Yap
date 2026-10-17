//! **What kind of app a dictation goes into** — a terminal (and which coding
//! CLI runs in it), a chat app, or anything else — and what that changes
//! about the paste.
//!
//! [`of_window`] looks at the target window once, at record start, beside
//! `target_hwnd`: its process name, window class and title (no UI
//! Automation; cheap). Only the process name and the kind are ever logged,
//! never the title. The rest of this module is pure and unit-tested;
//! `text_injector::prepare_dictation` applies it as the last step before the
//! paste (after AI cleanup, the dictionary, the "press enter" command and
//! snippets).
//!
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
//!   and the AI's rewrite or Voice Agent text gets the same one-line rule.
//! - **Chat apps** (`config.chat_no_full_stop`, "Leave the full stop off
//!   chat messages"): a message of one or two sentences in one paragraph
//!   doesn't end with a full stop ([`drop_chat_full_stop`]).

use crate::config::DictionaryEntry;
use crate::smart_insert::FieldContext;

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
    /// The window is Windows Terminal's (which wraps a paste in bracketed
    /// paste for an app that asks for it).
    pub windows_terminal: bool,
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
        self.windows_terminal && self.coding_cli.is_some_and(CodingCli::takes_pasted_lines)
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

    /// For the debug log (no titles, no text).
    pub fn describe(self) -> String {
        match self {
            AppKind::Terminal(t) => {
                let host = if t.windows_terminal { "Windows Terminal" } else { "a console" };
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

/// Window classes of terminals, whatever process owns them (a classic
/// console window can report the program running in it, e.g. claude.exe).
const TERMINAL_CLASSES: &[&str] = &[
    "ConsoleWindowClass",
    "CASCADIA_HOSTING_WINDOW_CLASS",
    "mintty",
    "PuTTY",
    "KiTTY",
    "VirtualConsoleClass",
    "org.wezfurlong.wezterm",
];

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
fn exe_stem(exe: &str) -> String {
    let base = exe.trim().rsplit(['\\', '/']).next().unwrap_or("");
    let lower = base.to_lowercase();
    lower.strip_suffix(".exe").unwrap_or(&lower).to_string()
}

/// What kind of app a window is, from its process name (`exe`, e.g.
/// "WindowsTerminal.exe"), window class and title.
pub fn classify(exe: Option<&str>, class: &str, title: &str) -> AppKind {
    let stem = exe_stem(exe.unwrap_or(""));
    let windows_terminal = stem == "windowsterminal" || class == "CASCADIA_HOSTING_WINDOW_CLASS";
    if windows_terminal || TERMINAL_CLASSES.contains(&class) || TERMINAL_EXES.contains(&stem.as_str()) {
        return AppKind::Terminal(Terminal {
            coding_cli: coding_cli(title),
            windows_terminal,
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

/// A title part without unread counts and markers ("(3) WhatsApp", "•
/// Discord") or Edge's "and 2 more pages".
fn page_name(seg: &str) -> &str {
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

/// What kind of app the window `hwnd` is (`None`: one of Yap's own, or none
/// → `Other`). Reads its process name, class and title; logs the process
/// name and the kind at `debug`, never the title. A test run reads nothing
/// (it would be whatever the developer has in front).
pub fn of_window(hwnd: Option<isize>) -> AppKind {
    let Some(hwnd) = hwnd else {
        return AppKind::Other;
    };
    if crate::e2e::active() {
        return AppKind::Other;
    }
    #[cfg(windows)]
    {
        let exe = crate::text_injector::app_name_for(Some(hwnd));
        let kind = classify(exe.as_deref(), &win::class(hwnd), &win::title(hwnd));
        tracing::debug!(app = exe.as_deref().unwrap_or("?"), kind = %kind.describe(), "The dictation's app");
        kind
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        AppKind::Other
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
/// last space).
fn word_before(s: &str, dot: usize) -> &str {
    let head = &s[..dot];
    let start = head.rfind(char::is_whitespace).map(|i| i + 1).unwrap_or(0);
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
fn is_abbreviation(word: &str) -> bool {
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
        AppKind::Terminal(Terminal {
            coding_cli: cli,
            windows_terminal: wt,
        })
    }

    #[test]
    fn terminals_by_process_and_class() {
        let wt = "CASCADIA_HOSTING_WINDOW_CLASS";
        assert_eq!(classify(Some("WindowsTerminal.exe"), wt, "Windows PowerShell"), term(None, true));
        assert_eq!(classify(Some("C:\\x\\WindowsTerminal.exe"), "", "pwsh"), term(None, true));
        assert_eq!(classify(Some("cmd.exe"), "ConsoleWindowClass", "Command Prompt"), term(None, false));
        assert_eq!(classify(Some("powershell.exe"), "ConsoleWindowClass", "Windows PowerShell"), term(None, false));
        assert_eq!(classify(Some("pwsh.exe"), "", "PowerShell 7"), term(None, false));
        assert_eq!(classify(Some("mintty.exe"), "mintty", "MINGW64:/c/Users/sam"), term(None, false));
        assert_eq!(classify(Some("wezterm-gui.exe"), "org.wezfurlong.wezterm", "pwsh"), term(None, false));
        assert_eq!(classify(Some("alacritty.exe"), "Window Class", "Alacritty"), term(None, false));
        assert_eq!(classify(Some("Tabby.exe"), "Chrome_WidgetWin_1", "PowerShell"), term(None, false));
        assert_eq!(classify(Some("Hyper.exe"), "Chrome_WidgetWin_1", "pwsh"), term(None, false));
        assert_eq!(classify(Some("putty.exe"), "PuTTY", "sam@box: ~"), term(None, false));
        // A classic console window can report the program running in it.
        assert_eq!(
            classify(Some("claude.exe"), "ConsoleWindowClass", "✳ Claude Code"),
            term(Some(CodingCli::ClaudeCode), false)
        );
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
        let t = |cli, wt| Terminal { coding_cli: cli, windows_terminal: wt };
        assert!(t(Some(CodingCli::ClaudeCode), true).keeps_lines());
        assert!(t(Some(CodingCli::Codex), true).keeps_lines());
        assert!(!t(Some(CodingCli::ClaudeCode), false).keeps_lines());
        assert!(!t(Some(CodingCli::GeminiCli), true).keeps_lines());
        assert!(!t(Some(CodingCli::Aider), true).keeps_lines());
        assert!(!t(None, true).keeps_lines());
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
        assert_eq!(terminal_text("Best\nNathan", false), "Best Nathan");
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
            let exe = crate::text_injector::app_name_for(Some(hwnd));
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

//! **The apps the person allowed** (`request_access`), per conversation,
//! and how far each may be used ([`Tier`]): in memory only — never saved,
//! gone when Yap quits, and after an hour with no computer use in that
//! chat ([`IDLE`]).
//!
//! An app is a Start-menu entry (`apps.rs`' index); its windows are told
//! apart by what the entry says it runs ([`Granted::owns`]): its exe (the
//! shortcut's target, or the `--processStart` app a Squirrel launcher
//! starts), its AppUserModelID or a packaged app's family, and for File
//! Explorer the desktop and the taskbar too.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::frame::Frame;
use super::windows::{Win, SHELL_CLASSES};
use crate::apps::index::App;

/// How long a chat's allowed apps last without any computer use.
pub const IDLE: Duration = Duration::from_secs(60 * 60);

/// **How far an app may be used** (the desktop contract's tiers, fixed when
/// it's allowed): a browser is seen but never clicked or typed in (web
/// pages go through Yap's own browser beside the chat); a terminal or an
/// IDE takes a left click and a scroll, never typing; anything else, all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Read,
    Click,
    #[default]
    Full,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Read => "read",
            Tier::Click => "click",
            Tier::Full => "full",
        }
    }
}

/// IDEs by exe name (lower case, no ".exe"): the VS Code family, Visual
/// Studio, JetBrains', and the editors built for code.
const IDE_EXES: &[&str] = &[
    "code", "code - insiders", "code - oss", "vscodium", "codium", "cursor", "windsurf", "positron", "kiro", "trae", "zed", "devenv", "idea64", "idea", "pycharm64", "pycharm", "webstorm64", "clion64",
    "rider64", "goland64", "rustrover64", "phpstorm64", "rubymine64", "datagrip64", "dataspell64", "fleet", "studio64", "eclipse", "netbeans64", "sublime_text", "android studio",
];

/// Terminals and shells by exe name (lower case, no ".exe").
const TERMINAL_EXES: &[&str] = &[
    "windowsterminal", "wt", "openconsole", "conhost", "cmd", "powershell", "pwsh", "wezterm-gui", "wezterm", "alacritty", "mintty", "git-bash", "tabby", "hyper", "conemu", "conemu64", "cmder", "putty", "kitty",
    "mobaxterm", "warp", "rio", "ghostty", "wsl", "bash",
];

/// Words in a Start-menu name that say what an entry without a known exe is.
const TERMINAL_NAMES: &[&str] = &["terminal", "command prompt", "powershell", "git bash", "git cmd", "developer command prompt", "developer powershell", "wsl", "ubuntu"];
const IDE_NAMES: &[&str] = &["visual studio", "intellij", "pycharm", "webstorm", "clion", "rider", "goland", "rustrover", "phpstorm", "android studio", "cursor", "windsurf"];
const BROWSER_NAMES: &[&str] = &["chrome", "edge", "firefox", "brave", "opera", "vivaldi", "librewolf", "waterfox", "floorp", "chromium", "thorium", "nightly private browsing", "zen browser"];

/// **An app's tier** from its exe names (`stems`, lower case) and its
/// Start-menu name's words ("Microsoft Edge" is a browser, "Knowledge Base"
/// isn't). Pure.
pub fn tier_of(stems: &[String], name: &str) -> Tier {
    let words: Vec<String> = name.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect();
    let n = format!(" {} ", words.join(" "));
    let has = |list: &[&str]| list.iter().any(|w| n.contains(&format!(" {w} ")));
    if stems.iter().any(|s| crate::pasting::app_kind::is_browser(s)) || has(BROWSER_NAMES) {
        Tier::Read
    } else if stems.iter().any(|s| TERMINAL_EXES.contains(&s.as_str()) || IDE_EXES.contains(&s.as_str())) || has(TERMINAL_NAMES) || has(IDE_NAMES) {
        Tier::Click
    } else {
        Tier::Full
    }
}

/// **One app allowed**, and how its windows are told apart.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Granted {
    /// As the Start menu names it.
    pub name: String,
    /// Its id in Yap's app index (an AppUserModelID, or a path).
    pub id: String,
    pub tier: Tier,
    #[serde(skip)]
    exes: Vec<String>,
    #[serde(skip)]
    stems: Vec<String>,
    /// Its AppUserModelID, lower case ("" when it has none).
    #[serde(skip)]
    aumid: String,
    /// File Explorer: the desktop and the taskbar are its too.
    #[serde(skip)]
    explorer: bool,
}

/// The exe a Squirrel launcher starts (`Update.exe --processStart
/// "Discord.exe"`), lower case without ".exe". Pure.
fn process_start(args: &str) -> Option<String> {
    let mut it = args.split_whitespace();
    while let Some(a) = it.next() {
        if a.eq_ignore_ascii_case("--processStart") {
            let exe = it.next()?.trim_matches('"');
            return Some(crate::pasting::app_kind::exe_stem(exe)).filter(|s| !s.is_empty());
        }
    }
    None
}

impl Granted {
    /// **What an app index entry runs**, for telling its windows apart. Pure.
    pub fn of(app: &App) -> Granted {
        let mut exes = Vec::new();
        let mut stems = Vec::new();
        let mut add = |path: &str| {
            let lower = path.trim().trim_matches('"').to_lowercase();
            if lower.ends_with(".exe") {
                let stem = crate::pasting::app_kind::exe_stem(&lower);
                // ("{GUID}\cmd.exe": a known folder's file; its name is what's known.)
                if !lower.starts_with('{') && lower.contains('\\') {
                    exes.push(lower.clone());
                }
                if !stem.is_empty() && !stems.contains(&stem) {
                    stems.push(stem);
                }
            }
        };
        add(&app.target);
        add(&app.id);
        if let Some(s) = process_start(&app.args) {
            if !stems.contains(&s) {
                stems.push(s);
            }
        }
        // A launcher's own exe (Update.exe) says nothing about the app's windows.
        if process_start(&app.args).is_some() {
            stems.retain(|s| s != "update");
            exes.retain(|e| !e.ends_with("\\update.exe"));
        }
        let id = app.id.to_lowercase();
        let explorer = id == "microsoft.windows.explorer" || stems.iter().any(|s| s == "explorer");
        if explorer && !stems.iter().any(|s| s == "explorer") {
            stems.push("explorer".into());
        }
        // An AppUserModelID (no folder in it): a packaged app's, or one a desktop app registered ("Chrome").
        let aumid = if !id.contains('\\') && !id.contains('/') && !id.ends_with(".exe") && !id.contains("://") { id } else { String::new() };
        let tier = tier_of(&stems, &app.name);
        Granted { name: app.name.clone(), id: app.id.clone(), tier, exes, stems, aumid, explorer }
    }

    /// An app that's just an exe (robot tests: the test pad's), its name
    /// its file's.
    #[cfg(debug_assertions)]
    pub fn of_exe(path: &str) -> Granted {
        let lower = path.to_lowercase();
        let stem = crate::pasting::app_kind::exe_stem(&lower);
        Granted { name: stem.clone(), id: path.to_string(), tier: tier_of(std::slice::from_ref(&stem), ""), exes: vec![lower], stems: vec![stem], aumid: String::new(), explorer: false }
    }

    /// **A stand-in window's exe and AppUserModelID** that this app owns
    /// (the stand-in desktop's `open_application`).
    #[cfg(any(test, debug_assertions))]
    pub fn stand_in(&self) -> (String, String) {
        let stem = self.stems.first().cloned().unwrap_or_else(|| self.name.to_lowercase().replace(' ', ""));
        let exe = self.exes.first().cloned().unwrap_or_else(|| format!("c:\\stand-in\\{stem}.exe"));
        (exe, self.aumid.clone())
    }

    /// Whether Yap can tell this app's windows at all.
    pub fn knows_its_windows(&self) -> bool {
        !self.exes.is_empty() || !self.stems.is_empty() || !self.aumid.is_empty() || self.explorer
    }

    /// **Whether `w` is one of this app's windows.** Pure.
    pub fn owns(&self, w: &Win) -> bool {
        if w.own {
            return false;
        }
        if self.explorer && SHELL_CLASSES.contains(&w.class.as_str()) {
            return true;
        }
        if !w.exe.is_empty() && self.exes.contains(&w.exe) {
            return true;
        }
        let stem = w.stem();
        if !stem.is_empty() && self.stems.contains(&stem) {
            return true;
        }
        if !self.aumid.is_empty() && !w.aumid.is_empty() {
            // A packaged app: its family is the part before the "!".
            let family = self.aumid.split('!').next().unwrap_or("");
            return w.aumid == self.aumid || w.aumid == family || w.aumid.starts_with(&format!("{}.", self.aumid));
        }
        false
    }
}

/// **Why an app is never allowed**: Yap itself (it neither looks at nor
/// drives its own windows), or a password manager (never looked at, as
/// Yap never reads one's window: `dictionary/screen_terms.rs`). Pure.
pub fn refused(app: &App, own_exe: &str) -> Option<&'static str> {
    let g = Granted::of(app);
    let name = app.name.to_lowercase();
    let own_stem = crate::pasting::app_kind::exe_stem(own_exe);
    if name == "yap" || name.starts_with("yap ") || (!own_exe.is_empty() && g.exes.iter().any(|e| e.eq_ignore_ascii_case(own_exe))) || (!own_stem.is_empty() && g.stems.contains(&own_stem)) {
        return Some("Yap doesn't use its own windows.");
    }
    if crate::dictionary::screen_terms::password_manager(&name) || g.stems.iter().any(|s| crate::dictionary::screen_terms::password_manager(s)) {
        return Some("Yap never looks at a password manager.");
    }
    None
}

/// The last full screenshot of a session's display, masked: what `zoom`
/// crops and the frame its coordinates are in.
pub struct Last {
    pub frame: Frame,
    /// The display's own pixels (its whole capture, masked), RGBA.
    pub rgba: Vec<u8>,
    pub display: String,
}

/// **One chat's computer use**: what's allowed, the display it looks at,
/// the last screenshot.
#[derive(Default)]
pub struct Session {
    pub apps: Vec<Granted>,
    pub clipboard_read: bool,
    pub clipboard_write: bool,
    pub system_keys: bool,
    /// The display `switch_display` chose (its name); `None`: automatic.
    pub display: Option<String>,
    pub last: Option<Arc<Last>>,
    used: Option<Instant>,
}

impl Session {
    /// What's allowed, without the screenshot (for a call's result).
    pub fn allowed(&self) -> Session {
        Session { apps: self.apps.clone(), clipboard_read: self.clipboard_read, clipboard_write: self.clipboard_write, system_keys: self.system_keys, ..Default::default() }
    }

    /// Adds `apps` (not twice) and the grants given.
    pub fn grant(&mut self, apps: Vec<Granted>, clipboard_read: bool, clipboard_write: bool, system_keys: bool) {
        for a in apps {
            if !self.apps.iter().any(|g| g.id == a.id) {
                self.apps.push(a);
            }
        }
        self.clipboard_read |= clipboard_read;
        self.clipboard_write |= clipboard_write;
        self.system_keys |= system_keys;
    }
}

static SESSIONS: Mutex<Option<HashMap<u64, Session>>> = Mutex::new(None);

/// **A chat's session** (`conversation`; `None`: a chat not saved yet),
/// for `f`: made when there's none, and every session unused for [`IDLE`]
/// forgotten first.
pub fn with<R>(conversation: Option<u64>, f: impl FnOnce(&mut Session) -> R) -> R {
    let mut all = SESSIONS.lock().unwrap_or_else(|p| p.into_inner());
    let map = all.get_or_insert_with(HashMap::new);
    let now = Instant::now();
    map.retain(|_, s| s.used.is_none_or(|t| now.duration_since(t) < IDLE));
    let s = map.entry(conversation.unwrap_or(0)).or_default();
    s.used = Some(now);
    f(s)
}

/// Forgets every chat's session (tests).
#[cfg(test)]
pub fn forget_all() {
    *SESSIONS.lock().unwrap_or_else(|p| p.into_inner()) = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, name: &str, target: &str, args: &str) -> App {
        App::linked(id, name, target, args)
    }

    fn win(exe: &str, aumid: &str, class: &str) -> Win {
        Win { exe: exe.to_lowercase(), aumid: aumid.into(), class: class.into(), ..Default::default() }
    }

    #[test]
    fn an_apps_windows_are_told_by_what_it_runs() {
        let notepad_classic = Granted::of(&app(r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\notepad.exe", "Notepad", "", ""));
        assert!(notepad_classic.owns(&win(r"C:\Windows\System32\notepad.exe", "", "Notepad")));
        assert!(!notepad_classic.owns(&win(r"C:\Windows\System32\mspaint.exe", "", "MSPaintApp")));
        // A packaged app: its AppUserModelID, the window's own or its package's family.
        let notepad = Granted::of(&app("Microsoft.WindowsNotepad_8wekyb3d8bbwe!App", "Notepad", "", ""));
        assert!(notepad.owns(&win(r"C:\Program Files\WindowsApps\Microsoft.WindowsNotepad_11.2410.21.0_x64__8wekyb3d8bbwe\Notepad\Notepad.exe", "microsoft.windowsnotepad_8wekyb3d8bbwe", "Notepad")));
        assert!(notepad.owns(&win("", "microsoft.windowsnotepad_8wekyb3d8bbwe!app", "ApplicationFrameWindow")));
        assert!(!notepad.owns(&win("", "microsoft.windowscalculator_8wekyb3d8bbwe", "ApplicationFrameWindow")));
        // A desktop app by its shortcut's target, and its own AppUserModelID's profiles.
        let chrome = Granted::of(&app("Chrome", "Google Chrome", r"C:\Program Files\Google\Chrome\Application\chrome.exe", ""));
        assert!(chrome.owns(&win(r"C:\Program Files\Google\Chrome\Application\chrome.exe", "", "Chrome_WidgetWin_1")));
        assert!(chrome.owns(&win(r"D:\elsewhere\chrome.exe", "chrome.profile1", "Chrome_WidgetWin_1")));
        assert_eq!(chrome.tier, Tier::Read);
        // A Squirrel launcher: the app it starts, never Update.exe.
        let chat = Granted::of(&app(r"C:\Users\Sam\AppData\Local\ChatApp\Update.exe", "Chat App", r"C:\Users\Sam\AppData\Local\ChatApp\Update.exe", "--processStart \"ChatApp.exe\""));
        assert!(chat.owns(&win(r"C:\Users\Sam\AppData\Local\ChatApp\app-1.0\ChatApp.exe", "", "Chrome_WidgetWin_1")));
        assert!(!chat.owns(&win(r"C:\Users\Sam\AppData\Local\Other\Update.exe", "", "x")));
        // File Explorer: its windows, the desktop and the taskbar.
        let explorer = Granted::of(&app("Microsoft.Windows.Explorer", "File Explorer", "", ""));
        for class in ["Progman", "WorkerW", "Shell_TrayWnd", "CabinetWClass"] {
            assert!(explorer.owns(&win(r"C:\Windows\explorer.exe", "", class)), "{class}");
        }
        assert!(!notepad_classic.owns(&win(r"C:\Windows\explorer.exe", "", "Progman")));
        // Never one of Yap's own windows.
        assert!(!notepad_classic.owns(&Win { own: true, ..win(r"C:\Windows\System32\notepad.exe", "", "Notepad") }));
        // An entry that names nothing Yap can match.
        assert!(!Granted::of(&app("steam://rungameid/1", "A Game", "steam://rungameid/1", "")).knows_its_windows());
    }

    #[test]
    fn browsers_are_read_only_and_terminals_and_ides_take_clicks() {
        let tier = |id: &str, name: &str, target: &str| Granted::of(&app(id, name, target, "")).tier;
        assert_eq!(tier("MSEdge", "Microsoft Edge", r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"), Tier::Read);
        assert_eq!(tier("Firefox-308046B0AF4A39CB", "Firefox", ""), Tier::Read);
        assert_eq!(tier("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App", "Terminal", ""), Tier::Click);
        assert_eq!(tier(r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\cmd.exe", "Command Prompt", ""), Tier::Click);
        assert_eq!(tier("Microsoft.VisualStudioCode", "Visual Studio Code", r"C:\Users\Sam\AppData\Local\Programs\Microsoft VS Code\Code.exe"), Tier::Click);
        assert_eq!(tier("JetBrains.IntelliJ", "IntelliJ IDEA", r"C:\Program Files\JetBrains\IntelliJ IDEA\bin\idea64.exe"), Tier::Click);
        assert_eq!(tier("Microsoft.WindowsNotepad_8wekyb3d8bbwe!App", "Notepad", ""), Tier::Full);
        assert_eq!(tier("Microsoft.Office.WINWORD.EXE.15", "Word", r"C:\Program Files\Microsoft Office\root\Office16\WINWORD.EXE"), Tier::Full);
        // Whole words only: a knowledge base isn't a browser, nor a search app.
        assert_eq!(tier("x", "Knowledge Base", ""), Tier::Full);
        assert_eq!(tier("x", "Research Notes", ""), Tier::Full);
        assert_eq!(tier("x", "Developer PowerShell for VS 2022", ""), Tier::Click);
    }

    #[test]
    fn yap_and_password_managers_are_never_allowed() {
        let own = r"c:\program files\yap\yap.exe";
        assert!(refused(&app(own, "Yap", own, ""), own).is_some());
        assert!(refused(&app("x", "Yap - Dev", "", ""), own).is_some());
        assert!(refused(&app(r"C:\Program Files\Bitwarden\Bitwarden.exe", "Bitwarden", r"C:\Program Files\Bitwarden\Bitwarden.exe", ""), own).is_some());
        assert!(refused(&app("x", "KeePassXC", "", ""), own).is_some());
        assert!(refused(&app("Microsoft.WindowsNotepad_8wekyb3d8bbwe!App", "Notepad", "", ""), own).is_none());
    }

    #[test]
    fn a_chats_grants_add_up_and_are_forgotten_when_unused() {
        // (The sessions are shared: one test at a time with them.)
        let _one = crate::computer::tests::LOCK.lock().unwrap_or_else(|p| p.into_inner());
        forget_all();
        let notepad = Granted::of(&app("Microsoft.WindowsNotepad_8wekyb3d8bbwe!App", "Notepad", "", ""));
        with(Some(7), |s| s.grant(vec![notepad.clone()], false, true, false));
        with(Some(7), |s| s.grant(vec![notepad.clone()], true, false, false));
        with(Some(7), |s| {
            assert_eq!(s.apps.len(), 1);
            assert!(s.clipboard_read && s.clipboard_write && !s.system_keys);
        });
        // Another chat has its own.
        assert!(with(Some(8), |s| s.apps.is_empty()));
        // Unused for an hour: gone.
        with(Some(7), |s| s.used = Some(Instant::now() - IDLE - Duration::from_secs(1)));
        assert!(with(Some(9), |_| true));
        assert!(with(Some(7), |s| s.apps.is_empty()));
        forget_all();
    }
}

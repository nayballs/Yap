//! The index: the apps the Start menu's "All apps" lists (or, should
//! that fail, its shortcuts and the App Paths registry), junk left out,
//! kept in memory and built again when it's stale.

use std::sync::Arc;
#[cfg(not(test))]
use std::sync::Mutex;
use std::time::Duration;
#[cfg(not(test))]
use std::time::Instant;

use serde::{Deserialize, Serialize};

#[cfg(all(windows, not(test)))]
use super::win;

/// Where an entry came from, which says how it opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    /// The Start menu's app list (`shell:AppsFolder`).
    #[default]
    AppsFolder,
    /// A Start menu shortcut (`.lnk`), when the list couldn't be read.
    Shortcut,
    /// An App Paths registry entry (an exe), likewise.
    AppPath,
}

/// One app Yap can open.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct App {
    /// Stable id: the entry's parsing name in the app list (an
    /// AppUserModelID, or a known-folder path); for a shortcut or an App
    /// Paths entry, its file's path. Settings keeps it for "Don't open this
    /// one" and nicknames.
    pub id: String,
    /// The name the Start menu shows.
    pub name: String,
    #[serde(default)]
    pub source: Source,
    /// What its Start menu shortcut points at (an exe's path, or a link such
    /// as `steam://rungameid/…`), when known. Read to tell what it opens in
    /// ("in Battle.net") and a game's link from a web page's; never run.
    #[serde(default)]
    pub target: String,
    /// Its shortcut's arguments, likewise: read, never passed by Yap.
    #[serde(default)]
    pub args: String,
    /// The Start menu shortcut (`.lnk` / `.url`) behind an app-list entry:
    /// what opens it, as a click in the Start menu does.
    #[serde(skip)]
    pub(super) shortcut: Option<String>,
    /// The entry's shell ID list (app-list entries): what opens it when it
    /// has no shortcut.
    #[serde(skip)]
    pub(super) pidl: Vec<u8>,
}

impl App {
    #[cfg(test)]
    pub(super) fn new(id: &str, name: &str) -> App {
        App { id: id.into(), name: name.into(), ..Default::default() }
    }

    /// A stand-in with what its shortcut points at.
    #[cfg(test)]
    pub(super) fn linked(id: &str, name: &str, target: &str, args: &str) -> App {
        App { target: target.into(), args: args.into(), ..App::new(id, name) }
    }

    /// The game launcher it opens in, if any ([`launcher`]).
    pub fn launcher(&self) -> Option<&'static str> {
        launcher(&self.target, &self.args).or_else(|| launcher(&self.id, ""))
    }

    /// A packaged (Store/MSIX) app: its id is an AppUserModelID
    /// ("Family_hash!App").
    pub fn packaged(&self) -> bool {
        self.source == Source::AppsFolder
            && self.id.contains('!')
            && !self.id.contains('\\')
            && !self.id.contains("://")
    }
}

/// How long the index is used before it's built again.
#[cfg_attr(test, allow(dead_code))]
pub const FRESH: Duration = Duration::from_secs(5 * 60);

#[cfg(not(test))]
struct Cached {
    at: Instant,
    apps: Arc<Vec<App>>,
}

#[cfg(not(test))]
static INDEX: Mutex<Option<Cached>> = Mutex::new(None);
/// One build at a time.
#[cfg(not(test))]
static BUILD: Mutex<()> = Mutex::new(());

/// The apps Yap can open, sorted by name: the cached index, or a fresh one
/// when it's older than [`FRESH`] or `refresh` asks. Blocking (a build reads
/// the Start menu's list: tens to hundreds of milliseconds).
pub fn apps(refresh: bool) -> Arc<Vec<App>> {
    #[cfg(test)]
    {
        let _ = refresh;
        super::tests::stand_ins()
    }
    #[cfg(not(test))]
    {
        if crate::shell::e2e::active() {
            return Arc::new(tidy(stand_ins()));
        }
        let cached = || {
            INDEX
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .as_ref()
                .filter(|c| c.at.elapsed() < FRESH)
                .map(|c| c.apps.clone())
        };
        if !refresh {
            if let Some(apps) = cached() {
                return apps;
            }
        }
        let _one = BUILD.lock().unwrap_or_else(|p| p.into_inner());
        if !refresh {
            // Built by another caller while this one waited.
            if let Some(apps) = cached() {
                return apps;
            }
        }
        let started = Instant::now();
        let built = Arc::new(tidy(index_this_pc()));
        tracing::info!(
            apps = built.len(),
            ms = started.elapsed().as_millis() as u64,
            "Apps Yap can open: indexed"
        );
        *INDEX.lock().unwrap_or_else(|p| p.into_inner()) = Some(Cached { at: Instant::now(), apps: built.clone() });
        built
    }
}

/// Test mode: the stand-in index (`YAP_E2E_APPS`, a JSON list of `{id,
/// name}`), read on every call so a spec can change it; none without it.
#[cfg(not(test))]
fn stand_ins() -> Vec<App> {
    #[cfg(debug_assertions)]
    {
        let Some(path) = std::env::var_os("YAP_E2E_APPS").filter(|v| !v.is_empty()) else {
            return Vec::new();
        };
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<App>>(&s).ok())
            .unwrap_or_default()
    }
    #[cfg(not(debug_assertions))]
    Vec::new()
}

/// This PC's apps: the Start menu's app list, else its shortcuts and the
/// App Paths registry.
#[cfg(all(windows, not(test)))]
fn index_this_pc() -> Vec<App> {
    match win::on_sta(win::apps_folder) {
        Some(Ok(apps)) if !apps.is_empty() => apps,
        other => {
            let why = match other {
                Some(Err(e)) => e,
                Some(Ok(_)) => "it was empty".to_string(),
                None => "its thread failed".to_string(),
            };
            tracing::warn!("Apps Yap can open: the Start menu's app list couldn't be read ({why}); using its shortcuts");
            let mut apps = shortcuts();
            apps.extend(win::app_paths());
            apps
        }
    }
}

#[cfg(all(not(windows), not(test)))]
fn index_this_pc() -> Vec<App> {
    Vec::new()
}

/// The Start menu's shortcuts (`.lnk`, and a game's `.url`), for when its
/// app list can't be read.
#[cfg(all(windows, not(test)))]
fn shortcuts() -> Vec<App> {
    fn walk(dir: &std::path::Path, depth: u8, out: &mut Vec<App>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth < 4 {
                    walk(&path, depth + 1, out);
                }
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
            // A `.url` is a link: kept only for a game store's ("steam://…").
            let target = match ext.as_str() {
                "lnk" => String::new(),
                "url" => match std::fs::read_to_string(&path).ok().and_then(|s| url_of(&s)) {
                    Some(url) if launcher(&url, "").is_some() => url,
                    _ => continue,
                },
                _ => continue,
            };
            if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                out.push(App {
                    id: path.to_string_lossy().into_owned(),
                    name: name.to_string(),
                    source: Source::Shortcut,
                    target,
                    ..Default::default()
                });
            }
        }
    }
    let mut out = Vec::new();
    for var in ["ProgramData", "APPDATA"] {
        if let Some(root) = std::env::var_os(var) {
            let dir = std::path::Path::new(&root).join(r"Microsoft\Windows\Start Menu\Programs");
            walk(&dir, 0, &mut out);
        }
    }
    out
}

/// The index from what was found: junk out ([`junk`]), one entry per name
/// (the first), sorted by name.
pub fn tidy(found: Vec<App>) -> Vec<App> {
    let mut seen = std::collections::HashSet::new();
    let mut apps: Vec<App> = found
        .into_iter()
        .filter(|a| !a.name.trim().is_empty() && !a.id.is_empty() && !junk(a))
        .filter(|a| seen.insert(a.name.trim().to_lowercase()))
        .collect();
    apps.sort_by_cached_key(|a| a.name.to_lowercase());
    apps
}

/// Game stores and launchers, by the link scheme their shortcuts use.
const LAUNCHER_LINKS: &[(&str, &str)] = &[
    ("steam://", "Steam"),
    ("com.epicgames.launcher://", "the Epic Games Launcher"),
    ("battlenet://", "Battle.net"),
    ("uplay://", "Ubisoft Connect"),
    ("origin://", "the EA app"),
    ("origin2://", "the EA app"),
    ("link2ea://", "the EA app"),
    ("goggalaxy://", "GOG Galaxy"),
    ("amazon-games://", "Amazon Games"),
    ("itchio://", "the itch app"),
];

/// The game launcher a shortcut opens in: a store's link (`steam://…`,
/// `com.epicgames.launcher://…`…), or a Blizzard game's launcher with its
/// product (`World of Warcraft Launcher.exe --productcode=wow`), which opens
/// Battle.net with that game picked. Pure.
pub fn launcher(target: &str, args: &str) -> Option<&'static str> {
    let t = target.trim().to_lowercase();
    if let Some((_, name)) = LAUNCHER_LINKS.iter().find(|(scheme, _)| t.starts_with(scheme)) {
        return Some(name);
    }
    let file = t.rsplit(['\\', '/']).next().unwrap_or("");
    (file.ends_with("launcher.exe") && args.to_lowercase().contains("--productcode=")).then_some("Battle.net")
}

/// The `URL=` of an Internet shortcut (`.url`, an INI file). Pure.
pub fn url_of(file: &str) -> Option<String> {
    file.lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix("URL=").or_else(|| l.strip_prefix("url=")))
        .map(|u| u.trim().to_string())
        .filter(|u| !u.is_empty())
}

/// Is this entry not an app to open ([`is_junk`])? A game's store link
/// ("Baldur's Gate 3" → `steam://rungameid/…`) is an app whatever file
/// holds it — only its name can make it junk; a shortcut to a web page is
/// a link.
pub(super) fn junk(app: &App) -> bool {
    if launcher(&app.target, "").is_some() || launcher(&app.id, "").is_some() {
        return is_junk(&app.name, "");
    }
    let target = app.target.trim().to_lowercase();
    WEB_LINKS.iter().any(|p| target.starts_with(p)) || is_junk(&app.name, &app.id)
}

/// Links that open a web page or a mail, not an app.
const WEB_LINKS: &[&str] = &["http://", "https://", "ftp://", "mailto:", "file:"];

/// Words in a name that mark it as documentation, a link or an uninstaller.
const JUNK_WORDS: &[&str] = &[
    "uninstall", "readme", "documentation", "docs", "manual", "manuals", "license", "licence",
    "website", "homepage", "faq", "faqs", "changelog",
];
/// Phrases that do the same.
const JUNK_PHRASES: &[&str] = &[
    "read me", "release notes", "web site", "on the web", "home page", "user guide", "help file",
    "online help", "change log", "what's new", "what is new",
];
/// Files that aren't apps: documents, links, installers.
const JUNK_EXTENSIONS: &[&str] = &[
    "txt", "rtf", "pdf", "chm", "hlp", "htm", "html", "url", "md", "xps", "doc", "docx", "msi",
    "log", "ini", "nfo", "website", "xml", "json",
];

/// Is this Start menu entry not an app to open? Uninstallers ("Uninstall
/// CMake", `unins000.exe`), read-mes, help files ("7-Zip Help", `.chm`),
/// documentation, licences, website links (`https://…`, `.url`, "… on the
/// Web") and folders. A real app named after one of these stays ("Revo
/// Uninstaller", "Get Help" — a Store app). Pure.
pub fn is_junk(name: &str, id: &str) -> bool {
    let lower_name = name.to_lowercase();
    let words: Vec<&str> = lower_name
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| !w.is_empty())
        .collect();
    let id_lower = id.to_lowercase();
    // A web or mail link (a game's steam:// entry is fine: it's how it opens).
    if WEB_LINKS.iter().any(|p| id_lower.starts_with(p)) {
        return true;
    }
    if words.iter().any(|w| JUNK_WORDS.contains(w)) || JUNK_PHRASES.iter().any(|p| lower_name.contains(p)) {
        return true;
    }
    if words.first() == Some(&"uninstaller") {
        return true;
    }
    // "7-Zip Help", "WinRAR help", "Help": not a Store app ("Get Help" is one).
    let packaged = id.contains('!') && !id.contains('\\') && !id.contains("://");
    if !packaged && (words.last() == Some(&"help") || words.first() == Some(&"help")) {
        return true;
    }
    // The file behind it, when the id is a path.
    if id.contains('\\') {
        let file = id_lower.rsplit('\\').next().unwrap_or("");
        if file.starts_with("unins") {
            return true;
        }
        match file.rsplit_once('.') {
            Some((_, ext)) if JUNK_EXTENSIONS.contains(&ext) => return true,
            // No extension: a folder.
            None => return true,
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::apps::tests::pc;

    #[test]
    fn junk_is_left_out_of_the_index() {
        let names: Vec<String> = pc().into_iter().map(|a| a.name).collect();
        for junk in ["Uninstall Revo Uninstaller", "7-Zip Help", "Revo Uninstaller on the Web", "Read Me", "CMake Documentation", "Uninstall CMake", "Context Mirror"] {
            assert!(!names.contains(&junk.to_string()), "{junk} is junk");
        }
        for app in ["Revo Uninstaller", "Get Help", "Stardew Valley", "Settings", "Notepad"] {
            assert!(names.contains(&app.to_string()), "{app} is an app");
        }
        assert!(is_junk("Git Release Notes", "E:\\Git\\ReleaseNotes.html"));
        assert!(is_junk("Python 3.12 Manuals (64-bit)", "C:\\Python312\\Doc\\html\\index.html"));
        assert!(is_junk("Python 3.12 Module Docs (64-bit)", "Microsoft.AutoGenerated.{568D}"));
        assert!(is_junk("License Agreement", "D:\\Borderless\\License.txt"));
        assert!(is_junk("Nsight Redistributable", "C:\\NVIDIA\\nsight.msi"));
        assert!(is_junk("Steam Support Center", "http://support.steampowered.com/"));
        assert!(is_junk("Node.js website", "https://nodejs.org/"));
        assert!(is_junk("Uninstall", "{7C5A}\\Ubisoft\\Uninstall.exe"));
        assert!(!is_junk("Services", "{1AC14E77}\\services.msc"));
        assert!(!is_junk("Microsoft Teams", "MSTeams_8wekyb3d8bbwe!MSTeams"));
        // One entry per name.
        let twice = tidy(vec![App::new("a", "Notepad"), App::new("b", "notepad")]);
        assert_eq!(twice.len(), 1);
        assert_eq!(twice[0].id, "a");
    }

    #[test]
    fn game_links_are_apps_and_web_links_are_not() {
        let kept = tidy(vec![
            App::linked("steam://rungameid/1086940", "Baldur's Gate 3", "steam://rungameid/1086940", ""),
            App::linked(r"C:\Start Menu\Fortnite.url", "Fortnite", "com.epicgames.launcher://apps/Fortnite?action=launch", ""),
            App::linked(r"C:\Start Menu\Diablo.url", "Diablo II", "battlenet://D2R", ""),
            App::linked(r"C:\Start Menu\Editor on the Web.url", "Editor Website", "https://example.com/", ""),
            App::linked(r"C:\Start Menu\Help.url", "Steam Help", "https://help.steampowered.com/", ""),
            App::linked("Microsoft.AutoGenerated.{1}", "Docs Link", "https://example.com/docs", ""),
            App::linked("steam://uninstall/1086940", "Uninstall Baldur's Gate 3", "steam://uninstall/1086940", ""),
        ]);
        let names: Vec<&str> = kept.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Baldur's Gate 3", "Diablo II", "Fortnite"]);
        assert_eq!(url_of("[InternetShortcut]\r\nIDList=\r\nURL=steam://rungameid/1086940\r\nIconIndex=0\r\n").as_deref(), Some("steam://rungameid/1086940"));
        assert_eq!(url_of("[InternetShortcut]\nIconIndex=0\n"), None);
    }
}

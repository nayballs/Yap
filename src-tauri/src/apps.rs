//! **Opening apps when you ask** (Settings → Voice Assistant → Opening apps):
//! "open Teams" in Chat, or to the Voice Assistant from any app, opens
//! Microsoft Teams. Only an app installed on this PC, from the list below —
//! never a command line, arguments, a file, a folder or a link, whatever a
//! model or a note says.
//!
//! **The index** ([`apps`]): what the Start menu's "All apps" lists — the
//! children of `shell:AppsFolder` (`SHGetKnownFolderItem(FOLDERID_AppsFolder)`
//! → `BHID_EnumItems`), each with its display name, its parsing name (an
//! AppUserModelID for Store/MSIX apps and for desktop apps that set one,
//! else a known-folder path such as `{6D809377-…}\7-Zip\7zFM.exe`) and its
//! shell ID list. That covers desktop apps' shortcuts and packaged apps (new
//! Teams, WhatsApp, Spotify from the Store…) in one list, named as the person
//! sees them. Should that fail, the Start menu's `.lnk` files (ProgramData
//! and AppData) and the App Paths registry stand in. Junk goes ([`is_junk`]):
//! uninstallers, read-mes, help files, documentation, website links, folders.
//! Kept in memory, built on first use and again when it's older than
//! [`FRESH`] (or on Settings' Refresh); read-only — nothing is launched while
//! indexing, nothing goes over the network.
//!
//! **Opening one** ([`open`]), by its entry only: a packaged app by its
//! AppUserModelID through `IApplicationActivationManager::ActivateApplication`
//! (the documented way to start a Store app; it brings the app to the front
//! itself and gives back its process id); a desktop app by **the Start menu
//! shortcut behind it** (`.lnk`, or `.url` for a game's `steam://` link),
//! opened with `ShellExecuteExW` and its default verb — what a click in the
//! Start menu does, so the shortcut's own arguments, working folder and "run
//! as" apply. (Invoking the app-list entry's ID list, as Yap first did,
//! "opened" World of Warcraft with nothing visible; its launcher needs the
//! shortcut's `--productcode=wow`, which nothing documents the entry's ID
//! list to carry. Opening the shortcut file is the documented way.) The shortcut
//! is the entry's `System.AppUserModel.BestShortcut`, read while indexing;
//! only an entry without one is opened by its ID list. Neither takes a
//! string a model wrote — no path, no arguments: the model names an app, Yap
//! finds the entry, Windows reads the shortcut on disk. An app that's
//! already open is just activated. A launch Windows refuses is said as such
//! ("Windows couldn't start …"), never "Opening…". Logs carry the app's name
//! and how it was opened, nothing else.
//!
//! **Matching what people say** ([`find`]): case-blind, ignoring "the",
//! "app", "please" and apostrophes ("baldurs"); the full name, the name
//! without its maker ("Teams" for Microsoft Teams) or its qualifiers
//! ("Python" for "Python 3.12 (64-bit)"), initials ("VS Code", "WoW"), the
//! last word ("Explorer"), a few spoken names ([`ALIASES`]: "calc", "cmd",
//! "files"…), numbers however they're said ("three" = "3" = "III"),
//! near-misses by `fuzzy.rs`'s spelling score ("spottify"), names that sound
//! the same ("boulders gate" for Baldur's Gate, when nothing else comes
//! close), and the person's nicknames ("my editor" → Visual Studio Code).
//! Several apps alike ("python" with four Pythons) → no guess: the
//! candidates. Apps switched off ("Don't open this one") are never opened.
//!
//! **The fast path** ([`plan`]), before any model: a message that is
//! clearly an open request ([`open_request`]: "open Teams", "launch
//! Spotify", "can you open notepad please", "try to open X", "play World of
//! Warcraft", "open it again") and names one app is handled by Yap itself —
//! it opens it and answers "Opening Microsoft Teams." (the Voice Assistant
//! says it) — with no model call, so it works with the 1.5B local model and
//! with no AI set up at all. Several apps → "Which one?"; none → "I couldn't
//! find…" (+ "Did you mean …?") unless a model that can use tools is there
//! to make sense of it; switched off → it says so. **Answers to Yap's own
//! questions** are Yap's too ([`follow_up`], a per-conversation memory,
//! [`recall`]): "yes" / "go ahead" / "open it" after "Did you mean X?", "the
//! second one" / "3.13" / "the 64-bit one" after "Which one?", "no" to
//! neither; the question lapses after [`OFFER_FOR`] or at the next message.
//! Anything else goes to the model as usual, which has the `open_app` and
//! `list_apps` tools ([`execute_tool`]); `open_app` only acts when the
//! person's own message asks to open something ([`asked_to_open`]) or says
//! yes to the model's own offer to open that app ([`consented`]), never
//! because a note, a transcript or a web page says to. A model without
//! tools is told it can't open anything, and an answer of its that claims to
//! ("Opening Baldur's Gate 3.") is replaced by how to ask Yap
//! ([`correct_claim`]).
//!
//! **Test mode** (`e2e::active`, and unit tests): nothing is ever launched;
//! opens are recorded (`e2e_opened_apps`), and the index is a stand-in —
//! the JSON file `YAP_E2E_APPS` names (`[{ "id", "name" }]`), or nothing.
//!
//! Here: opening an entry and the commands. The parts: `index` (the
//! index), `matching` (what people say, and the app it names), `plan` (the
//! fast path and the answers to Yap's questions), `tools` (the model's
//! tools, and a model without them) and, on Windows, `win`.

mod index;
mod matching;
mod plan;
mod tools;
#[cfg(windows)]
mod win;

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
#[cfg(any(debug_assertions, test))]
use serde_json::{json, Value};

pub use index::{apps, App, Source};
pub use plan::{carry_out, moved_on, plan_for, remember, Plan};
pub use tools::{claims_unasked, correct_claim, execute_tool, NO_TOOLS_NOTE, NO_TOOLS_OPEN, TOOL_NAMES};

/// A score at least this good opens an app.
const CONFIDENT: f64 = 0.80;
/// Apps within this of the best are "several": Yap asks which one.
const MARGIN: f64 = 0.03;
/// The longest list of candidates or close names given back.
const MAX_LISTED: usize = 6;
/// The most names `list_apps` gives the model (each later step re-sends
/// them: Groq's free plan allows 8,000 tokens a minute).
const LIST_MAX: usize = 15;
/// How long Yap's question ("Did you mean …?", "Which one?") waits for its
/// answer.
pub const OFFER_FOR: Duration = Duration::from_secs(5 * 60);
/// How long "open it again" remembers the app Yap opened or suggested.
pub const LAST_FOR: Duration = Duration::from_secs(30 * 60);

// ---- opening ---------------------------------------------------------------------

/// Apps "opened" in test mode (and unit tests): `(id, name)`.
static OPENED: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// Open `app` — an entry of the index, never anything a model wrote. A test
/// run never launches anything: it records the open (`e2e_opened_apps`).
pub fn open(app: &App) -> Result<(), String> {
    if cfg!(test) || crate::shell::e2e::active() {
        tracing::info!(app = %app.name, "Opening an app (test run: recorded, not launched)");
        OPENED.lock().unwrap_or_else(|p| p.into_inner()).push((app.id.clone(), app.name.clone()));
        return Ok(());
    }
    #[cfg(windows)]
    {
        let entry = app.clone();
        let since = crate::shell::procs::now_filetime();
        let launched = win::on_sta(move || win::launch(&entry)).unwrap_or_else(|| Err("its thread failed".to_string()));
        match launched {
            Ok((pid, how)) => {
                tracing::info!(app = %app.name, how, launcher = app.launcher().unwrap_or(""), "Opened an app");
                if let Some(pid) = pid.filter(|_| !app.packaged()) {
                    // Started from the background (the Voice Assistant), a
                    // desktop app's window can open behind the one in front.
                    std::thread::spawn(move || win::bring_forward(pid, since));
                }
                Ok(())
            }
            Err(e) => {
                tracing::warn!(app = %app.name, "Windows couldn't start an app: {e}");
                Err(e)
            }
        }
    }
    #[cfg(not(windows))]
    Err("Opening apps works on Windows only.".to_string())
}

// ---- commands --------------------------------------------------------------------

/// An app for Settings' list.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Listed {
    pub id: String,
    pub name: String,
}

/// Settings → Voice Assistant → Opening apps: the apps Yap can open
/// (`refresh`: index again now).
#[tauri::command]
pub async fn apps_list(refresh: Option<bool>) -> Result<Vec<Listed>, String> {
    let refresh = refresh.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || {
        apps(refresh).iter().map(|a| Listed { id: a.id.clone(), name: a.name.clone() }).collect()
    })
    .await
    .map_err(|e| e.to_string())
}

/// Icons already made, by app id (`None`: it has none).
static ICONS: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);

/// The apps' own icons, as Explorer draws them (`{ id: PNG data URL }`), for
/// the apps Settings shows. Made once per run; test mode has none.
#[tauri::command]
pub async fn apps_icons(ids: Vec<String>) -> Result<HashMap<String, String>, String> {
    if crate::shell::e2e::active() {
        return Ok(HashMap::new());
    }
    tauri::async_runtime::spawn_blocking(move || icons(&ids)).await.map_err(|e| e.to_string())
}

fn icons(ids: &[String]) -> HashMap<String, String> {
    let index = apps(false);
    let missing: Vec<App> = {
        let cache = ICONS.lock().unwrap_or_else(|p| p.into_inner());
        ids.iter()
            .take(300)
            .filter(|id| cache.as_ref().is_none_or(|c| !c.contains_key(*id)))
            .filter_map(|id| index.iter().find(|a| &a.id == id).cloned())
            .collect()
    };
    if !missing.is_empty() {
        #[cfg(windows)]
        let made: Vec<(String, Option<String>)> = win::on_sta(move || {
            missing
                .iter()
                .map(|a| (a.id.clone(), win::icon_png(a, 32).map(|png| crate::calls::app_icons::data_url(&png))))
                .collect()
        })
        .unwrap_or_default();
        #[cfg(not(windows))]
        let made: Vec<(String, Option<String>)> = missing.iter().map(|a| (a.id.clone(), None)).collect();
        let mut cache = ICONS.lock().unwrap_or_else(|p| p.into_inner());
        cache.get_or_insert_with(HashMap::new).extend(made);
    }
    let cache = ICONS.lock().unwrap_or_else(|p| p.into_inner());
    ids.iter()
        .filter_map(|id| cache.as_ref()?.get(id)?.clone().map(|url| (id.clone(), url)))
        .collect()
}

/// Test mode only: the apps Yap "opened" (`[{ id, name }]`), oldest first;
/// `clear` empties the list.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_opened_apps(clear: Option<bool>) -> Result<Vec<Value>, String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    let mut opened = OPENED.lock().unwrap_or_else(|p| p.into_inner());
    let list = opened.iter().map(|(id, name)| json!({ "id": id, "name": name })).collect();
    if clear.unwrap_or(false) {
        opened.clear();
    }
    Ok(list)
}

// ---- tests -----------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::index::{junk, tidy};
    use super::matching::{find, Found};
    use super::tools::list_apps_tool;
    use super::*;
    use crate::config::YapConfig;
    use std::sync::Arc;
    use std::time::Instant;
    use std::cell::RefCell;

    thread_local! {
        static STAND_INS: RefCell<Vec<App>> = const { RefCell::new(Vec::new()) };
    }

    /// The index in unit tests: what [`set_stand_ins`] put there (none to
    /// start with), never this PC's apps.
    pub(crate) fn stand_ins() -> Arc<Vec<App>> {
        Arc::new(STAND_INS.with(|s| s.borrow().clone()))
    }

    /// Opens recorded since the last call (unit tests share one list, so
    /// tests look for their own app ids).
    pub(crate) fn opened_ids() -> Vec<String> {
        OPENED.lock().unwrap_or_else(|p| p.into_inner()).iter().map(|(id, _)| id.clone()).collect()
    }

    /// A PC much like the one this was written on.
    pub(crate) const PC: &[(&str, &str)] = &[
        ("MSTeams_8wekyb3d8bbwe!MSTeams", "Microsoft Teams"),
        ("SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify", "Spotify"),
        ("Microsoft.WindowsNotepad_8wekyb3d8bbwe!App", "Notepad"),
        ("Microsoft.WindowsCalculator_8wekyb3d8bbwe!App", "Calculator"),
        ("TheDocumentFoundation.LibreOffice.Calc", "LibreOffice Calc"),
        ("Microsoft.Windows.Explorer", "File Explorer"),
        ("windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel", "Settings"),
        ("Microsoft.AutoGenerated.{6038ED00}", "WinCDEmu Settings"),
        ("Microsoft.VisualStudioCode", "Visual Studio Code"),
        ("Chrome", "Google Chrome"),
        ("MSEdge", "Microsoft Edge"),
        ("{1AC14E77}\\cmd.exe", "Command Prompt"),
        ("Microsoft.WindowsTerminal_8wekyb3d8bbwe!App", "Terminal"),
        ("C:\\Python312\\python.exe", "Python 3.12 (64-bit)"),
        ("C:\\Python313\\python.exe", "Python 3.13 (64-bit)"),
        ("C:\\Python314\\python.exe", "Python 3.14 (64-bit)"),
        ("OpenAI.Codex_2p2nqsd0c76g0!App", "ChatGPT"),
        ("OpenAI.ChatGPT-Desktop_2p2nqsd0c76g0!ChatGPT", "ChatGPT Classic"),
        ("Microsoft.AutoGenerated.{923DD477}", "Task Manager"),
        ("com.tmog.taskmanager", "Task Manager TMOG"),
        ("com.squirrel.Discord.Discord", "Discord"),
        ("Microsoft.Todos_8wekyb3d8bbwe!App", "Microsoft To Do"),
        ("steam://rungameid/413150", "Stardew Valley"),
        ("{6D809377}\\VS Revo Group\\Revo Uninstaller\\RevoUnin.exe", "Revo Uninstaller"),
        ("Microsoft.GetHelp_8wekyb3d8bbwe!App", "Get Help"),
        // Junk:
        ("{6D809377}\\VS Revo Group\\Revo Uninstaller\\unins000.exe", "Uninstall Revo Uninstaller"),
        ("{6D809377}\\7-Zip\\7-zip.chm", "7-Zip Help"),
        ("https://www.revouninstaller.com/", "Revo Uninstaller on the Web"),
        ("C:\\VulkanSDK\\README.txt", "Read Me"),
        ("{6D809377}\\CMake\\doc\\index.html", "CMake Documentation"),
        ("Microsoft.AutoGenerated.{B7BDCA76}", "Uninstall CMake"),
        ("E:\\Projects\\Context Mirror", "Context Mirror"),
    ];

    pub(super) fn pc() -> Vec<App> {
        tidy(PC.iter().map(|(id, name)| App::new(id, name)).collect())
    }

    pub(super) fn found(query: &str) -> Found {
        find(&pc(), query, &[], &[])
    }

    pub(super) fn one(query: &str) -> String {
        match found(query) {
            Found::One(a) => a.name,
            other => panic!("{query:?}: {other:?}"),
        }
    }

    pub(super) fn several(query: &str) -> Vec<String> {
        match found(query) {
            Found::Several(apps) => apps.into_iter().map(|a| a.name).collect(),
            other => panic!("{query:?}: {other:?}"),
        }
    }

    /// Games as a test PC indexes them (2026-10-18): Blizzard's shortcuts
    /// run one launcher with a product code; Steam's are `.url` links.
    pub(super) fn games() -> Vec<App> {
        const WOW: &str = r"D:\Program Files (x86)\World of Warcraft\World of Warcraft Launcher.exe";
        let mut apps = pc();
        apps.extend([
            App::linked("Microsoft.AutoGenerated.{00C80652-6323-D440-5B60-BE66CBC5CFD1}", "World of Warcraft", WOW, "--productcode=wow"),
            App::linked("Microsoft.AutoGenerated.{2C17C6C2-6D3C-9C11-E1D2-0E2334472010}", "World of Warcraft Classic", WOW, "--productcode=wow_classic_era"),
            App::linked("Microsoft.AutoGenerated.{5EA68C5A-EAAA-318F-D5CE-F2F6B4BF2E9D}", "Cataclysm Classic", WOW, "--productcode=wow_classic"),
            App::linked(r"D:\Program Files (x86)\Battle.net\Battle.net Launcher.exe", "Battle.net", r"D:\Program Files (x86)\Battle.net\Battle.net Launcher.exe", ""),
            App::linked(r"D:\Program Files (x86)\Diablo IV\Diablo IV Launcher.exe", "Diablo IV", r"D:\Program Files (x86)\Diablo IV\Diablo IV Launcher.exe", ""),
            App::linked("steam://rungameid/1086940", "Baldur's Gate 3", "steam://rungameid/1086940", ""),
            App::linked(r"{7C5A40EF-A0FB-4BFC-874A-C0F2E0B9FA8E}\Steam\Steam.exe", "Steam", r"C:\Program Files (x86)\Steam\steam.exe", ""),
        ]);
        tidy(apps)
    }

    pub(super) fn found_in(apps: &[App], query: &str) -> String {
        match find(apps, query, &[], &[]) {
            Found::One(a) => a.name,
            Found::Several(v) => format!("several: {}", v.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(" / ")),
            Found::Blocked(a) => format!("blocked: {}", a.name),
            Found::Nothing(c) => format!("nothing; close: {}", c.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")),
        }
    }

    pub(super) fn named(apps: &[App], names: &[&str]) -> Vec<App> {
        names.iter().map(|n| apps.iter().find(|a| a.name == *n).cloned().unwrap_or_else(|| panic!("{n}"))).collect()
    }

    #[cfg(windows)]
    #[test]
    fn whole_id_lists_only() {
        assert!(win::whole_id_list(&[4, 0, 1, 2, 0, 0]));
        assert!(win::whole_id_list(&[0, 0]));
        assert!(!win::whole_id_list(&[4, 0, 1, 2]));
        assert!(!win::whole_id_list(&[1, 0, 0, 0]));
        assert!(!win::whole_id_list(&[]));
    }

    /// How an entry would open, for a test's printout (never launched): its
    /// shortcut file's name and which Start menu holds it (no user paths).
    #[cfg(windows)]
    fn how_it_opens(a: &App, roots: &[String]) -> String {
        if a.packaged() {
            return "app activation (packaged)".to_string();
        }
        match &a.shortcut {
            Some(s) => {
                let lower = s.to_lowercase();
                let which = if roots.first().is_some_and(|r| lower.starts_with(r)) { "your Start menu" } else { "everyone's Start menu" };
                let file = s.rsplit('\\').next().unwrap_or("");
                format!("its shortcut \u{201C}{file}\u{201D} in {which}")
            }
            None if !a.pidl.is_empty() => "its app-list entry (no shortcut)".to_string(),
            None => "its file".to_string(),
        }
    }

    /// This PC's real apps, read-only (nothing is launched): what Teams,
    /// VS Code and friends are called here, how the games are indexed and
    /// would open, and what people's words find. `cargo test -- --ignored
    /// this_pcs_apps --nocapture`.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn this_pcs_apps() {
        let started = Instant::now();
        let raw = win::on_sta(win::apps_folder).expect("thread").expect("the app list");
        let raw_len = raw.len();
        let dropped: Vec<String> = raw.iter().filter(|a| junk(a)).map(|a| a.name.clone()).collect();
        let desktop = raw.iter().filter(|a| !a.packaged()).count();
        let linked = raw.iter().filter(|a| !a.packaged() && a.shortcut.is_some()).count();
        let apps = tidy(raw);
        println!("{} entries, {} apps after tidying, in {} ms", raw_len, apps.len(), started.elapsed().as_millis());
        println!("desktop entries: {desktop}, opened by their Start menu shortcut: {linked}");
        println!("left out as junk: {}", dropped.join(" | "));
        let roots = win::start_menu_roots();
        println!("games and launchers:");
        for a in apps.iter().filter(|a| {
            let n = a.name.to_lowercase();
            ["warcraft", "battle.net", "baldur", "steam", "overwatch", "diablo", "cataclysm", "crusade"].iter().any(|w| n.contains(w))
        }) {
            let target = if a.target.contains("://") { a.target.clone() } else { a.target.rsplit('\\').next().unwrap_or("").to_string() };
            println!(
                "  {:<28} → {} | target {} {} | opens {}",
                a.name,
                how_it_opens(a, &roots),
                target,
                a.args,
                a.launcher().map(|l| format!("in {l}")).unwrap_or_else(|| "by itself".into())
            );
        }
        for a in apps.iter().filter(|a| {
            let n = a.name.to_lowercase();
            ["teams", "code", "spotify", "notepad", "calculator", "explorer", "settings", "chrome", "edge", "word", "discord", "slack", "zoom", "terminal"]
                .iter()
                .any(|w| n.contains(w))
        }) {
            println!("  {:<40} {:<8} {}", a.name, if a.packaged() { "packaged" } else { "desktop" }, a.id);
        }
        for q in [
            "teams", "notepad", "the calculator", "spotify", "vs code", "file explorer", "settings", "python",
            "powershell", "chrome", "world of warcraft", "wow", "world of warcraft classic", "battle.net",
            "battlenet", "baldur's gate three", "boulders gate three", "baldurs gate 3", "baldur's gate", "steam",
        ] {
            let what = match find(&apps, q, &[], &[]) {
                Found::One(a) => format!("opens {}", a.name),
                Found::Several(v) => format!("asks: {}", v.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(" / ")),
                Found::Blocked(a) => format!("blocked {}", a.name),
                Found::Nothing(c) => format!("not found (close: {})", c.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")),
            };
            println!("  \"{q}\" → {what}");
        }
        // What list_apps sends the model (was 840 tokens with no query: 150 names).
        let est = |v: &Value| crate::meetings::meeting_summary::estimate_tokens(&v.to_string());
        let cfg = YapConfig::default();
        println!(
            "list_apps: no query {} tokens, \"python\" {} tokens",
            est(&list_apps_tool(&json!({}), &cfg, &apps)),
            est(&list_apps_tool(&json!({ "query": "python" }), &cfg, &apps))
        );
        let fallback = shortcuts_for_test();
        println!("fallback: {} Start menu shortcuts", fallback.len());
        let with_icon = apps.iter().take(10).filter(|a| {
            let a = (*a).clone();
            win::on_sta(move || win::icon_png(&a, 32)).flatten().is_some()
        });
        println!("icons: {} of the first 10", with_icon.count());
        assert!(apps.iter().any(|a| a.name == "Notepad" || a.name == "Calculator"));
    }

    #[cfg(windows)]
    fn shortcuts_for_test() -> Vec<App> {
        let mut out = Vec::new();
        for var in ["ProgramData", "APPDATA"] {
            if let Some(root) = std::env::var_os(var) {
                let dir = std::path::Path::new(&root).join(r"Microsoft\Windows\Start Menu\Programs");
                count_links(&dir, 0, &mut out);
            }
        }
        out.extend(win::app_paths());
        out
    }

    #[cfg(windows)]
    fn count_links(dir: &std::path::Path, depth: u8, out: &mut Vec<App>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && depth < 4 {
                count_links(&path, depth + 1, out);
            } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("lnk")) {
                out.push(App::new(&path.to_string_lossy(), &path.file_stem().unwrap_or_default().to_string_lossy()));
            }
        }
    }

    /// **One real open**: Notepad, opened the way Yap opens an app, then
    /// closed — only the Notepad this test started, found by its process id.
    /// Skipped while a Notepad is already running (it would be reused, and
    /// it isn't ours to close). `cargo test -- --ignored opens_notepad_for_real
    /// --nocapture`.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn opens_notepad_for_real() {
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
        let running = |name: &str| -> Vec<u32> {
            crate::shell::procs::exe_names().into_iter().filter(|(_, exe)| exe == name).map(|(pid, _)| pid).collect()
        };
        if !running("notepad.exe").is_empty() {
            println!("SKIPPED: a Notepad is already running; this test only closes a Notepad it started");
            return;
        }
        let raw = win::on_sta(win::apps_folder).expect("thread").expect("the app list");
        let apps = tidy(raw);
        let Found::One(notepad) = find(&apps, "notepad", &[], &[]) else { panic!("no Notepad in the app list") };
        println!("opening {} ({}, {})", notepad.name, if notepad.packaged() { "packaged" } else { "desktop" }, notepad.id);
        let entry = notepad.clone();
        let (pid, how) = win::on_sta(move || win::launch(&entry)).expect("thread").expect("Notepad opens");
        println!("opened through {how}");
        // The process to close: the one Windows named, else the one new Notepad.
        let started = Instant::now();
        let pid = loop {
            let now = running("notepad.exe");
            match pid {
                Some(p) if now.contains(&p) => break p,
                _ if now.len() == 1 => break now[0],
                _ if started.elapsed() > Duration::from_secs(10) => panic!("no new Notepad process (pid {pid:?}, running {now:?})"),
                _ => std::thread::sleep(Duration::from_millis(100)),
            }
        };
        let window = loop {
            if let Some(w) = win::main_window(pid) {
                break w;
            }
            assert!(started.elapsed() < Duration::from_secs(15), "Notepad (pid {pid}) showed no window");
            std::thread::sleep(Duration::from_millis(100));
        };
        println!("Notepad is open: pid {pid}, window {:?}, after {} ms", window.0, started.elapsed().as_millis());
        std::thread::sleep(Duration::from_millis(800));
        // Close only that window, then make sure only that process ended.
        unsafe { PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0)) }.expect("WM_CLOSE posted");
        let closing = Instant::now();
        while running("notepad.exe").contains(&pid) {
            if closing.elapsed() > Duration::from_secs(5) {
                println!("Notepad (pid {pid}) didn't close by itself; ending that process");
                assert!(crate::shell::procs::kill(pid));
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        println!("closed after {} ms", closing.elapsed().as_millis());
        assert!(!running("notepad.exe").contains(&pid));
    }

    /// Top-level windows: (window, class, title).
    #[cfg(windows)]
    fn top_windows() -> Vec<(isize, String, String)> {
        use windows::Win32::Foundation::{HWND, LPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClassNameW, GetWindowTextW, IsWindowVisible};
        unsafe extern "system" fn each(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
            // SAFETY: `lparam` is the Vec below, alive for EnumWindows.
            let out = unsafe { &mut *(lparam.0 as *mut Vec<(isize, String, String)>) };
            if unsafe { IsWindowVisible(hwnd) }.as_bool() {
                let (mut class, mut title) = ([0u16; 256], [0u16; 512]);
                let c = unsafe { GetClassNameW(hwnd, &mut class) } as usize;
                let t = unsafe { GetWindowTextW(hwnd, &mut title) } as usize;
                out.push((hwnd.0 as isize, String::from_utf16_lossy(&class[..c]), String::from_utf16_lossy(&title[..t])));
            }
            true.into()
        }
        let mut out: Vec<(isize, String, String)> = Vec::new();
        // SAFETY: the callback only reads windows and writes `out`.
        unsafe {
            let _ = EnumWindows(Some(each), LPARAM(&mut out as *mut _ as isize));
        }
        out
    }

    /// A shortcut file (`.lnk`) to `target` with `args`, starting in `dir`,
    /// minimised. Needs COM.
    #[cfg(windows)]
    fn make_lnk(path: &std::path::Path, target: &str, args: &str, dir: &std::path::Path) {
        use windows::core::{Interface, HSTRING};
        use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER};
        use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWMINNOACTIVE;
        // SAFETY: COM on this STA thread; a file in the test's own folder.
        unsafe {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).expect("ShellLink");
            link.SetPath(&HSTRING::from(target)).expect("SetPath");
            link.SetArguments(&HSTRING::from(args)).expect("SetArguments");
            link.SetWorkingDirectory(&HSTRING::from(dir.as_os_str())).expect("SetWorkingDirectory");
            link.SetShowCmd(SW_SHOWMINNOACTIVE).expect("SetShowCmd");
            let file: IPersistFile = link.cast().expect("IPersistFile");
            file.Save(&HSTRING::from(path.as_os_str()), true).expect("Save");
        }
    }

    /// **Opening by the shortcut file, for real** — the way an app-list
    /// entry with a Start menu shortcut opens now — in a folder of the
    /// test's own (`YAP_SHORTCUT_TEST_DIR`, else the temp folder), never the
    /// Start menu: (1) a `.url` to `ms-settings:about` opens Settings (closed
    /// again: only the Settings window that wasn't there before; skipped
    /// while Settings runs — it would be navigated); (2) a `.lnk` to cmd.exe
    /// whose arguments write a file into its working folder: both arrived;
    /// (3) a `.lnk` to notepad.exe with a new text file as its argument:
    /// Notepad shows that file, then only that Notepad is closed — skipped
    /// while any Notepad runs (Windows 11's opens the file as a tab of the
    /// one already open). `cargo test -- --ignored opens_shortcuts_for_real
    /// --nocapture`.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn opens_shortcuts_for_real() {
        use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};
        let running = |name: &str| -> Vec<u32> {
            crate::shell::procs::exe_names().into_iter().filter(|(_, exe)| exe == name).map(|(pid, _)| pid).collect()
        };
        let base = std::env::var_os("YAP_SHORTCUT_TEST_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let dir = base.join(format!("yap-shortcut-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the test's folder");
        let entry = |file: &std::path::Path| App {
            id: "Microsoft.AutoGenerated.{TEST}".into(),
            name: "Test".into(),
            source: Source::AppsFolder,
            shortcut: Some(file.to_string_lossy().into_owned()),
            ..Default::default()
        };
        let launch = |app: App| win::on_sta(move || win::launch(&app)).expect("thread");

        // (1) A .url (a game's steam:// link opens the same way): Settings'
        // About page, or — Settings already open (it would be navigated) —
        // Calculator; neither open already, else skipped.
        let frames = || -> Vec<(isize, String)> {
            top_windows().into_iter().filter(|(_, class, _)| class == "ApplicationFrameWindow").map(|(h, _, t)| (h, t)).collect()
        };
        let open_now = frames();
        match [("ms-settings:about", "Settings"), ("calculator:", "Calculator")]
            .into_iter()
            .find(|(_, title)| !open_now.iter().any(|(_, t)| t == title))
        {
            Some((link, title)) => {
                let url = dir.join(format!("{title}.url"));
                std::fs::write(&url, format!("[InternetShortcut]\r\nURL={link}\r\n")).unwrap();
                let before: Vec<isize> = open_now.iter().map(|w| w.0).collect();
                let (_, how) = launch(entry(&url)).expect("the .url opens");
                let started = Instant::now();
                let window = loop {
                    let new: Vec<(isize, String)> =
                        frames().into_iter().filter(|(h, t)| !before.contains(h) && t == title).collect();
                    if let [one] = new.as_slice() {
                        break one.0;
                    }
                    assert!(started.elapsed() < Duration::from_secs(15), "no new {title} window");
                    std::thread::sleep(Duration::from_millis(200));
                };
                println!("(1) .url to {link} opened through {how}: a new {title} window after {} ms", started.elapsed().as_millis());
                std::thread::sleep(Duration::from_millis(1500));
                unsafe { PostMessageW(Some(HWND(window as *mut _)), WM_CLOSE, WPARAM(0), LPARAM(0)) }.expect("WM_CLOSE");
                println!("(1) closed that {title} window (only the one this test opened)");
            }
            None => println!("(1) SKIPPED: Settings and Calculator are both open already"),
        }

        // (2) A .lnk's arguments and working folder.
        let lnk = dir.join("Write a file.lnk");
        let cmd = std::path::Path::new(&std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())).join(r"System32\cmd.exe");
        let (lnk2, cmd2, dir2) = (lnk.clone(), cmd.to_string_lossy().into_owned(), dir.clone());
        win::on_sta(move || make_lnk(&lnk2, &cmd2, "/d /c echo arrived> arrived.txt", &dir2)).expect("thread");
        let (_, how) = launch(entry(&lnk)).expect("the .lnk opens");
        let marker = dir.join("arrived.txt");
        let started = Instant::now();
        while !marker.is_file() {
            assert!(started.elapsed() < Duration::from_secs(10), "the shortcut's arguments didn't arrive");
            std::thread::sleep(Duration::from_millis(100));
        }
        std::thread::sleep(Duration::from_millis(300));
        let text = std::fs::read_to_string(&marker).unwrap_or_default();
        println!("(2) .lnk opened through {how}: its arguments ran in its working folder (\"{}\")", text.trim());
        assert_eq!(text.trim(), "arrived");

        // (3) Notepad with a file as the shortcut's argument.
        if running("notepad.exe").is_empty() {
            let note = dir.join("yap-shortcut-test.txt");
            std::fs::write(&note, "Opened by Yap's shortcut test.\r\n").unwrap();
            let lnk = dir.join("Notepad with a file.lnk");
            let notepad = std::path::Path::new(&std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into())).join(r"System32\notepad.exe");
            let (lnk2, np, arg, dir2) = (lnk.clone(), notepad.to_string_lossy().into_owned(), format!("\"{}\"", note.display()), dir.clone());
            win::on_sta(move || make_lnk(&lnk2, &np, &arg, &dir2)).expect("thread");
            let (pid, how) = launch(entry(&lnk)).expect("the .lnk opens");
            let started = Instant::now();
            let pid = loop {
                let now = running("notepad.exe");
                match pid {
                    Some(p) if now.contains(&p) => break p,
                    _ if now.len() == 1 => break now[0],
                    _ if started.elapsed() > Duration::from_secs(10) => panic!("no new Notepad (pid {pid:?}, running {now:?})"),
                    _ => std::thread::sleep(Duration::from_millis(100)),
                }
            };
            let title = loop {
                let titled = win::main_window(pid).and_then(|w| top_windows().into_iter().find(|t| t.0 == w.0 as isize)).map(|t| t.2);
                if let Some(t) = titled.filter(|t| t.contains("yap-shortcut-test")) {
                    break t;
                }
                assert!(started.elapsed() < Duration::from_secs(15), "Notepad (pid {pid}) didn't show the file");
                std::thread::sleep(Duration::from_millis(150));
            };
            println!("(3) .lnk opened through {how}: Notepad (pid {pid}) shows \u{201C}{title}\u{201D}");
            std::thread::sleep(Duration::from_millis(800));
            if let Some(w) = win::main_window(pid) {
                unsafe { PostMessageW(Some(w), WM_CLOSE, WPARAM(0), LPARAM(0)) }.expect("WM_CLOSE");
            }
            let closing = Instant::now();
            while running("notepad.exe").contains(&pid) {
                if closing.elapsed() > Duration::from_secs(5) {
                    assert!(crate::shell::procs::kill(pid));
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            println!("(3) closed that Notepad");
        } else {
            println!("(3) SKIPPED: a Notepad is already running (Windows 11's would open the file as a tab of it)");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

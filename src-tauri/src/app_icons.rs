//! The call apps' own icons, as installed on this PC, for Settings → General
//! → Meetings ("Ask about calls in") and the Yap bar's call card
//! (`src/lib/CallAppIcon.svelte`). An app that isn't installed, or that runs
//! in a browser (Google Meet, Whereby, Jitsi Meet), shows its bundled mark
//! instead (`src/lib/bar/callApps.js`), or a letter in its colour.
//!
//! **Where the icons come from.** Read-only and local: nothing is launched,
//! nothing goes over the network, and the only files read are icons and
//! package manifests, plus the microphone record call detection reads anyway
//! (`meeting_detect.rs`).
//! - Desktop apps: the exe paths in Windows' microphone record
//!   (`NonPackaged\<path, '\' spelled '#'>`), the most recently used first,
//!   then each app's usual install folders ([`KNOWN_PATHS`]). The exe's own
//!   icon, drawn at 48 px (`SHDefExtractIconW`), saved as a PNG.
//! - Packaged (MSIX) apps — new Teams, WhatsApp, and Telegram or Slack from
//!   the Store: the family names in the microphone record, then
//!   [`FAMILIES`], lead to their install folders (`GetPackagesByPackageFamily`
//!   and `GetPackagePathByFullName`), whose `AppxManifest.xml` names the app
//!   list logo (`Square44x44Logo`). Of its variants on disk, the one made for
//!   a light background at about 48 px
//!   (`…targetsize-48_altform-lightunplated.png`).
//!
//! With call detection switched off, Yap leaves the microphone record alone
//! and looks only in the usual places.
//!
//! **Cache.** `<data>/icons/<app>.png`, and `sources.json` noting what each
//! was made from (the file, its size and time): an icon is made again when
//! its source changes, and at least weekly, and an app that's gone loses its
//! icon. The command runs off the main thread; the page draws the bundled
//! marks until it answers.
//!
//! Test mode (`e2e::active`) reads no installed apps: icons come only from
//! the folder `YAP_E2E_APP_ICONS` names (`<app>.png` stand-ins), so the
//! suite is deterministic.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::meeting_detect::{App, MicRecord, APPS};

/// The size icons are made at. The page shows them at 20 px, so they stay
/// sharp past 200 % display scaling.
const ICON_PX: u32 = 48;
/// An icon is made again after this long, even if its source didn't change.
const MAX_AGE_SECS: u64 = 7 * 24 * 60 * 60;
/// A logo or a manifest bigger than this isn't one.
const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// What each cached icon was made from (in the cache folder).
const INDEX: &str = "sources.json";

/// Packaged call apps' family names (package name + publisher hash), for an
/// app that's installed but has never asked for the mic, so isn't in the
/// microphone record yet.
const FAMILIES: &[(&str, &str)] = &[
    ("teams", "MSTeams_8wekyb3d8bbwe"),
    ("teams", "MicrosoftTeams_8wekyb3d8bbwe"),
    ("slack", "91750D7E.Slack_8she8kybcnzg4"),
    ("whatsapp", "5319275A.WhatsAppDesktop_cv1g1gvanyjgm"),
    ("telegram", "TelegramMessengerLLP.TelegramDesktop_t4vj0pshhgkwm"),
];

/// Where desktop call apps install. `%VARS%` are the environment's; a `*`
/// in a folder name is the newest such folder that has the exe (Squirrel's
/// `app-1.0.9260`).
const KNOWN_PATHS: &[(&str, &str)] = &[
    ("teams", r"%LOCALAPPDATA%\Microsoft\Teams\current\Teams.exe"),
    ("zoom", r"%APPDATA%\Zoom\bin\Zoom.exe"),
    ("zoom", r"%ProgramFiles%\Zoom\bin\Zoom.exe"),
    ("zoom", r"%ProgramFiles(x86)%\Zoom\bin\Zoom.exe"),
    ("webex", r"%LOCALAPPDATA%\CiscoSparkLauncher\CiscoCollabHost.exe"),
    ("webex", r"%LOCALAPPDATA%\Programs\Cisco Spark\CiscoCollabHost.exe"),
    ("webex", r"%ProgramFiles%\Cisco Spark\CiscoCollabHost.exe"),
    ("slack", r"%LOCALAPPDATA%\slack\slack.exe"),
    ("slack", r"%ProgramFiles%\Slack\slack.exe"),
    ("discord", r"%LOCALAPPDATA%\Discord\app-*\Discord.exe"),
    ("discord", r"%LOCALAPPDATA%\Discord\Discord.exe"),
    ("discord", r"%LOCALAPPDATA%\DiscordPTB\app-*\DiscordPTB.exe"),
    ("discord", r"%LOCALAPPDATA%\DiscordCanary\app-*\DiscordCanary.exe"),
    ("goto", r"%LOCALAPPDATA%\Programs\GoTo\GoTo.exe"),
    ("goto", r"%ProgramFiles%\GoTo\GoTo.exe"),
    ("goto", r"%LOCALAPPDATA%\GoToMeeting\*\g2mcomm.exe"),
    ("whatsapp", r"%LOCALAPPDATA%\WhatsApp\WhatsApp.exe"),
    ("signal", r"%LOCALAPPDATA%\Programs\signal-desktop\Signal.exe"),
    ("telegram", r"%APPDATA%\Telegram Desktop\Telegram.exe"),
];

// ---- the command ----------------------------------------------------------------

/// `{ app id: data URL }`: the call apps installed here, by their own icons
/// (`src/lib/callAppIcons.svelte.js`).
#[tauri::command]
pub async fn meeting_app_icons() -> Result<BTreeMap<String, String>, String> {
    tauri::async_runtime::spawn_blocking(icons)
        .await
        .map_err(|e| e.to_string())
}

/// One look at a time (the main window and the Yap bar can ask together).
static LOOKING: Mutex<()> = Mutex::new(());

fn icons() -> BTreeMap<String, String> {
    if crate::e2e::active() {
        return crate::e2e::app_icons_dir().map(|dir| stand_ins(&dir)).unwrap_or_default();
    }
    let _one = LOOKING.lock().unwrap_or_else(|p| p.into_inner());
    let records = if crate::config::load().meeting_detection {
        crate::meeting_detect::mic_records()
    } else {
        Vec::new()
    };
    let dir = crate::config::data_dir().join("icons");
    let icons = refresh(&dir, now_secs(), |app| sources(app, &records));
    tracing::info!(apps = ?icons.keys().collect::<Vec<_>>(), "app icons: installed call apps");
    icons
}

/// Test mode: the stand-ins in `dir` (`<app>.png`), as the suite's
/// "installed" apps.
fn stand_ins(dir: &Path) -> BTreeMap<String, String> {
    APPS.iter()
        .filter_map(|app| {
            let png = read_png(&dir.join(format!("{}.png", app.id)))?;
            Some((app.id.to_string(), data_url(&png)))
        })
        .collect()
}

fn now_secs() -> u64 {
    secs(SystemTime::now())
}

fn secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn data_url(png: &[u8]) -> String {
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png))
}

// ---- the cache ------------------------------------------------------------------

/// What an icon was made from, and when (`sources.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Made {
    source: String,
    len: u64,
    /// The source's modified time (unix seconds).
    modified: u64,
    /// When the icon was made (unix seconds).
    made: u64,
}

impl Made {
    /// `source` as it is on disk now (not made yet: `made` is 0).
    fn of(source: &Source) -> Option<Made> {
        let meta = std::fs::metadata(source.path()).ok().filter(|m| m.is_file())?;
        Some(Made {
            source: source.path().to_string_lossy().into_owned(),
            len: meta.len(),
            modified: secs(meta.modified().ok()?),
            made: 0,
        })
    }

    fn same_source(&self, other: &Made) -> bool {
        (&self.source, self.len, self.modified) == (&other.source, other.len, other.modified)
    }
}

/// Bring the icons in `dir` up to date with what's installed (`sources`,
/// best first, per app) and return them as `{ app id: data URL }`. A cached
/// icon stands while its source is unchanged and under a week old; an app
/// with no source left loses its icon.
fn refresh(dir: &Path, now: u64, sources: impl Fn(&App) -> Vec<Source>) -> BTreeMap<String, String> {
    let before: BTreeMap<String, Made> = std::fs::read(dir.join(INDEX))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let mut after = BTreeMap::new();
    let mut icons = BTreeMap::new();
    for app in APPS {
        let file = dir.join(format!("{}.png", app.id));
        let mut found = None;
        for source in sources(app) {
            let Some(now_on_disk) = Made::of(&source) else { continue };
            let cached = before
                .get(app.id)
                .filter(|m| m.same_source(&now_on_disk) && now.saturating_sub(m.made) < MAX_AGE_SECS);
            if let Some((made, png)) = cached.and_then(|m| Some((m.clone(), read_png(&file)?))) {
                found = Some((made, png));
                break;
            }
            if let Some(png) = render(&source) {
                if let Err(e) = write_file(&file, &png) {
                    tracing::warn!(app = app.id, "app icons: couldn't cache the icon ({e})");
                }
                found = Some((Made { made: now, ..now_on_disk }, png));
                break;
            }
        }
        match found {
            Some((made, png)) => {
                icons.insert(app.id.to_string(), data_url(&png));
                after.insert(app.id.to_string(), made);
            }
            None => {
                let _ = std::fs::remove_file(&file);
            }
        }
    }
    if after != before {
        let written = serde_json::to_vec_pretty(&after)
            .map_err(|e| e.to_string())
            .and_then(|json| write_file(&dir.join(INDEX), &json).map_err(|e| e.to_string()));
        if let Err(e) = written {
            tracing::warn!("app icons: couldn't save the cache's index ({e})");
        }
    }
    icons
}

/// Write `bytes` to `path` whole (a temp file renamed over it).
fn write_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

// ---- sources ----------------------------------------------------------------------

/// Where an app's icon comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    /// A desktop app's exe: its own icon.
    Exe(PathBuf),
    /// A packaged app's logo: a PNG already.
    Logo(PathBuf),
}

impl Source {
    fn path(&self) -> &Path {
        match self {
            Source::Exe(p) | Source::Logo(p) => p,
        }
    }
}

/// Where `app`'s icon can come from, best first, among what's on disk: its
/// entries in the microphone record (the most recently used first), its
/// known package families, its usual install folders.
fn sources(app: &App, records: &[MicRecord]) -> Vec<Source> {
    let mut out = Vec::new();
    let mut mine: Vec<&MicRecord> = records.iter().filter(|r| r.app == app.id).collect();
    mine.sort_by_key(|r| std::cmp::Reverse(r.last_used));
    let mut families_seen: Vec<String> = Vec::new();
    for record in mine {
        if record.packaged {
            families_seen.push(record.key.to_ascii_lowercase());
            if let Some(logo) = package_logo(&record.key) {
                add(&mut out, Source::Logo(logo));
            }
        } else {
            let exe = consent_path(&record.key);
            if exe.is_file() {
                add(&mut out, Source::Exe(exe));
            }
        }
    }
    for (id, family) in FAMILIES {
        if *id == app.id && !families_seen.contains(&family.to_ascii_lowercase()) {
            if let Some(logo) = package_logo(family) {
                add(&mut out, Source::Logo(logo));
            }
        }
    }
    for (id, pattern) in KNOWN_PATHS {
        if *id != app.id {
            continue;
        }
        if let Some(exe) = expand_vars(pattern, |name| std::env::var(name).ok()).and_then(|p| find_file(Path::new(&p))) {
            add(&mut out, Source::Exe(exe));
        }
    }
    out
}

/// Add `source` unless it's there already (paths compared as Windows does).
fn add(out: &mut Vec<Source>, source: Source) {
    let key = |s: &Source| s.path().to_string_lossy().to_lowercase();
    if !out.iter().any(|o| key(o) == key(&source)) {
        out.push(source);
    }
}

/// A `NonPackaged` microphone-record key is the exe's path with `\` spelled
/// `#`: `C:#Users#me#AppData#Local#Discord#app-1.0.9260#Discord.exe`.
fn consent_path(key: &str) -> PathBuf {
    PathBuf::from(key.replace('#', "\\"))
}

/// `%NAME%` → `env(NAME)` (a trailing `\` dropped); `None` if one isn't set.
fn expand_vars(pattern: &str, env: impl Fn(&str) -> Option<String>) -> Option<String> {
    let mut out = String::new();
    let mut rest = pattern;
    while let Some(start) = rest.find('%') {
        let len = rest[start + 1..].find('%')?;
        out.push_str(&rest[..start]);
        let value = env(&rest[start + 1..start + 1 + len]).filter(|v| !v.trim().is_empty())?;
        out.push_str(value.trim_end_matches(['\\', '/']));
        rest = &rest[start + len + 2..];
    }
    out.push_str(rest);
    Some(out)
}

/// `path` if it's a file. With a `*` in one folder name, the newest such
/// file (`Discord\app-*\Discord.exe` → the latest version's).
fn find_file(path: &Path) -> Option<PathBuf> {
    let parts: Vec<_> = path.components().collect();
    let Some(star) = parts.iter().position(|c| c.as_os_str().to_string_lossy().contains('*')) else {
        return path.is_file().then(|| path.to_path_buf());
    };
    let head: PathBuf = parts[..star].iter().collect();
    let tail: PathBuf = parts[star + 1..].iter().collect();
    let pattern = parts[star].as_os_str().to_string_lossy().to_lowercase();
    let (prefix, suffix) = pattern.split_once('*')?;
    std::fs::read_dir(&head)
        .ok()?
        .flatten()
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            name.len() >= prefix.len() + suffix.len() && name.starts_with(prefix) && name.ends_with(suffix)
        })
        .filter_map(|entry| {
            let file = entry.path().join(&tail);
            let meta = std::fs::metadata(&file).ok().filter(|m| m.is_file())?;
            Some((meta.modified().ok()?, file))
        })
        .max()
        .map(|(_, file)| file)
}

/// The icon `source` makes, as a PNG.
fn render(source: &Source) -> Option<Vec<u8>> {
    match source {
        Source::Logo(path) => read_png(path),
        Source::Exe(path) => exe_png(path),
    }
}

#[cfg(windows)]
fn exe_png(exe: &Path) -> Option<Vec<u8>> {
    let (width, height, rgba) = win::exe_icon(exe, ICON_PX)?;
    encode_png(width, height, &rgba)
}

#[cfg(not(windows))]
fn exe_png(_exe: &Path) -> Option<Vec<u8>> {
    None
}

// ---- packaged apps --------------------------------------------------------------

/// A packaged app's app list logo: named in the manifest of a package of
/// `family` (installed for this user), its best variant on disk.
#[cfg(windows)]
fn package_logo(family: &str) -> Option<PathBuf> {
    let dirs = win::package_dirs(family);
    let logo = dirs.iter().find_map(|dir| {
        let xml = read_capped(&dir.join("AppxManifest.xml"))?;
        manifest_logo(&String::from_utf8_lossy(&xml))
    })?;
    best_variant(&dirs, &logo)
}

#[cfg(not(windows))]
fn package_logo(_family: &str) -> Option<PathBuf> {
    None
}

/// The app list logo an `AppxManifest.xml` names, relative to its package
/// folder: an application's `Square44x44Logo` (an older manifest's 30 × 30
/// or small logo), preferring one shown in the app list, else the package's
/// store logo.
fn manifest_logo(xml: &str) -> Option<String> {
    const LOGOS: [&str; 4] = ["Square44x44Logo", "Square30x30Logo", "SmallLogo", "Square150x150Logo"];
    let mut apps: Vec<(bool, String)> = Vec::new(); // (in the app list, logo)
    let (mut in_app, mut seen_visuals) = (false, false);
    let (mut in_properties, mut in_logo) = (false, false);
    let mut store_logo = None;
    for event in xml_events(xml) {
        match event {
            Xml::Start { name, attrs, closed } => match local(name) {
                "Application" => {
                    in_app = !closed;
                    seen_visuals = false;
                }
                "VisualElements" if in_app && !seen_visuals => {
                    seen_visuals = true;
                    let listed = attr(&attrs, "AppListEntry").is_none_or(|v| !v.eq_ignore_ascii_case("none"));
                    if let Some(logo) = LOGOS.iter().find_map(|a| attr(&attrs, a)) {
                        apps.push((listed, logo.to_string()));
                    }
                }
                "Properties" => in_properties = !closed,
                "Logo" if in_properties => in_logo = !closed,
                _ => {}
            },
            Xml::End(name) => match local(name) {
                "Application" => in_app = false,
                "Properties" => in_properties = false,
                "Logo" => in_logo = false,
                _ => {}
            },
            Xml::Text(text) => {
                if in_logo && store_logo.is_none() {
                    store_logo = Some(text.trim().to_string());
                }
            }
        }
    }
    let app_logo = apps.iter().find(|(listed, _)| *listed).or(apps.first());
    app_logo.map(|(_, logo)| logo.clone()).or(store_logo).filter(|logo| !logo.is_empty())
}

/// A logo file on disk and its resource qualifiers (`targetsize-48`,
/// `scale-200`, `altform-unplated`, `theme-dark`, `contrast-black`…), from
/// its name and the folders it's in.
#[derive(Debug, Clone)]
struct Variant {
    path: PathBuf,
    qualifiers: Vec<String>,
    /// Its width in pixels (`targetsize-N`, else its PNG header; 0 unknown).
    px: u32,
}

/// The qualifier names Windows' resource system uses. Folders named with
/// them hold variants too (`scale-200\`, `contrast-standard\`).
const QUALIFIERS: &[&str] = &[
    "scale",
    "targetsize",
    "altform",
    "contrast",
    "theme",
    "lang",
    "language",
    "layoutdirection",
    "dxfeaturelevel",
    "configuration",
    "devicefamily",
    "homeregion",
];

/// The best variant of logo `rel` (as a manifest names it, relative to the
/// package folder) in any of `dirs` (a package and its resource packages).
fn best_variant(dirs: &[PathBuf], rel: &str) -> Option<PathBuf> {
    // A plain relative path: nothing outside the package.
    let parts: Vec<&str> = rel.trim().split(['\\', '/']).collect();
    if parts.iter().any(|p| p.is_empty() || *p == "." || *p == ".." || p.contains(':')) {
        return None;
    }
    let (file, folders) = parts.split_last()?;
    let (stem, ext) = file.rsplit_once('.')?;
    if !ext.eq_ignore_ascii_case("png") {
        return None;
    }
    let mut found = Vec::new();
    for dir in dirs {
        let folder = folders.iter().fold(dir.clone(), |path, f| path.join(f));
        variants_in(&folder, stem, ext, &[], 0, &mut found);
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    found.into_iter().min_by_key(rank).map(|v| v.path)
}

/// The variants of `stem.ext` in `dir`, and in qualifier folders under it
/// (two levels at most: `contrast-standard\theme-light\`).
fn variants_in(dir: &Path, stem: &str, ext: &str, inherited: &[String], depth: u8, out: &mut Vec<Variant>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_dir() {
            if let Some(mut q) = qualifiers(&name).filter(|_| depth < 2) {
                q.extend_from_slice(inherited);
                variants_in(&entry.path(), stem, ext, &q, depth + 1, out);
            }
        } else if let Some(mut q) = file_qualifiers(&name, stem, ext) {
            q.extend_from_slice(inherited);
            let px = q
                .iter()
                .find_map(|q| q.strip_prefix("targetsize-")?.parse().ok())
                .or_else(|| png_file_size(&entry.path()).map(|(width, _)| width))
                .unwrap_or(0);
            out.push(Variant { path: entry.path(), qualifiers: q, px });
        }
    }
}

/// `name`'s qualifiers if it's a variant of `stem.ext` (none for the plain
/// file): `AppList.targetsize-48_altform-unplated.png` → `targetsize-48`,
/// `altform-unplated`.
fn file_qualifiers(name: &str, stem: &str, ext: &str) -> Option<Vec<String>> {
    let name = name.to_ascii_lowercase();
    let rest = name
        .strip_prefix(&stem.to_ascii_lowercase())?
        .strip_suffix(&ext.to_ascii_lowercase())?
        .strip_suffix('.')?;
    match rest {
        "" => Some(Vec::new()),
        _ => qualifiers(rest.strip_prefix('.')?),
    }
}

/// `targetsize-48_altform-unplated` → its qualifiers, if every part is one.
fn qualifiers(s: &str) -> Option<Vec<String>> {
    s.split('_')
        .map(|q| {
            let (name, value) = q.split_once('-')?;
            (QUALIFIERS.contains(&name) && !value.is_empty()).then(|| q.to_string())
        })
        .collect()
}

/// Lower is better: not high contrast; not for a dark theme; made to sit on
/// no plate on a light background (`altform-lightunplated`, then
/// `altform-unplated`); then the smallest at least [`ICON_PX`], else the
/// biggest below it.
fn rank(v: &Variant) -> (bool, bool, u8, bool, u32) {
    let has = |q: &str| v.qualifiers.iter().any(|x| x == q);
    let high_contrast = v.qualifiers.iter().any(|q| q.starts_with("contrast-") && q != "contrast-standard");
    let altform = if has("altform-lightunplated") {
        0
    } else if has("altform-unplated") {
        1
    } else {
        2
    };
    let small = v.px < ICON_PX;
    (high_contrast, has("theme-dark"), altform, small, if small { u32::MAX - v.px } else { v.px })
}

// ---- a little XML ---------------------------------------------------------------

/// A piece of an XML document — as much of XML as a manifest needs.
#[derive(Debug, PartialEq, Eq)]
enum Xml<'a> {
    /// A start tag: its name (with any prefix), its attributes, and whether
    /// it closes itself (`<a/>`).
    Start { name: &'a str, attrs: Vec<(&'a str, String)>, closed: bool },
    End(&'a str),
    Text(String),
}

/// `doc`'s tags and text, in order. Comments, processing instructions and
/// declarations are skipped; CDATA is text.
fn xml_events(doc: &str) -> Vec<Xml<'_>> {
    let mut out = Vec::new();
    let mut rest = doc;
    while let Some(lt) = rest.find('<') {
        if !rest[..lt].trim().is_empty() {
            out.push(Xml::Text(unescape(&rest[..lt])));
        }
        rest = &rest[lt..];
        let past = |end: &str, from: usize| rest[from..].find(end).map(|i| from + i + end.len());
        let next = if rest.starts_with("<!--") {
            past("-->", 4)
        } else if let Some(body) = rest.strip_prefix("<![CDATA[") {
            body.find("]]>").map(|end| {
                out.push(Xml::Text(body[..end].to_string()));
                "<![CDATA[".len() + end + "]]>".len()
            })
        } else if rest.starts_with("<?") || rest.starts_with("<!") {
            past(">", 2)
        } else {
            tag_end(rest).map(|gt| {
                out.push(tag(&rest[1..gt]));
                gt + 1
            })
        };
        match next {
            Some(n) => rest = &rest[n..],
            None => return out, // cut short
        }
    }
    if !rest.trim().is_empty() {
        out.push(Xml::Text(unescape(rest)));
    }
    out
}

/// Where a tag that starts `s` ends (its `>`, outside quoted values).
fn tag_end(s: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (None, '"' | '\'') => quote = Some(c),
            (Some(q), _) if c == q => quote = None,
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

/// What's between `<` and `>`.
fn tag(inner: &str) -> Xml<'_> {
    if let Some(name) = inner.strip_prefix('/') {
        return Xml::End(name.trim());
    }
    let (body, closed) = match inner.strip_suffix('/') {
        Some(body) => (body, true),
        None => (inner, false),
    };
    let name_end = body.find(char::is_whitespace).unwrap_or(body.len());
    Xml::Start { name: &body[..name_end], attrs: attributes(&body[name_end..]), closed }
}

/// `a="1" b='2'` → its attributes, their values unescaped.
fn attributes(mut s: &str) -> Vec<(&str, String)> {
    let mut out = Vec::new();
    loop {
        s = s.trim_start();
        let Some(eq) = s.find('=') else { return out };
        let name = s[..eq].trim();
        s = s[eq + 1..].trim_start();
        let Some(quote) = s.chars().next().filter(|c| *c == '"' || *c == '\'') else { return out };
        let Some(len) = s[1..].find(quote) else { return out };
        out.push((name, unescape(&s[1..1 + len])));
        s = &s[len + 2..];
    }
}

/// `&amp;`, `&lt;`, `&#92;`… → their characters.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let entity = rest.find(';').filter(|semi| *semi <= 10).and_then(|semi| {
            let c = match &rest[1..semi] {
                "lt" => '<',
                "gt" => '>',
                "amp" => '&',
                "quot" => '"',
                "apos" => '\'',
                code => {
                    let n = match code.strip_prefix("#x").or_else(|| code.strip_prefix("#X")) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                        None => code.strip_prefix('#')?.parse().ok()?,
                    };
                    char::from_u32(n)?
                }
            };
            Some((c, semi + 1))
        });
        match entity {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// A name without its namespace prefix (`uap:VisualElements` → `VisualElements`).
fn local(name: &str) -> &str {
    name.rsplit_once(':').map_or(name, |(_, local)| local)
}

fn attr<'a>(attrs: &'a [(&str, String)], name: &str) -> Option<&'a str> {
    attrs.iter().find(|(n, _)| local(n) == name).map(|(_, v)| v.as_str())
}

// ---- PNG ------------------------------------------------------------------------

/// A PNG's (width, height), from its header; `None` if it isn't one.
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < 24 || !bytes.starts_with(SIGNATURE) || &bytes[12..16] != b"IHDR" {
        return None;
    }
    let be = |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    let size = (be(16), be(20));
    (size.0 > 0 && size.1 > 0).then_some(size)
}

/// The size of the PNG at `path`, from its first bytes.
fn png_file_size(path: &Path) -> Option<(u32, u32)> {
    let mut head = [0u8; 24];
    std::fs::File::open(path).ok()?.read_exact(&mut head).ok()?;
    png_size(&head)
}

/// The file at `path` if it isn't too big for an icon or a manifest.
fn read_capped(path: &Path) -> Option<Vec<u8>> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.is_file() && meta.len() <= MAX_FILE_BYTES {
        std::fs::read(path).ok()
    } else {
        None
    }
}

/// The PNG at `path`, if it is one.
fn read_png(path: &Path) -> Option<Vec<u8>> {
    read_capped(path).filter(|bytes| png_size(bytes).is_some())
}

/// Straight RGBA → PNG.
fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(rgba).ok()?;
    writer.finish().ok()?;
    Some(out)
}

/// 32-bit BGRA, as GDI hands it over, → straight RGBA. An icon whose pixels
/// carry no alpha at all is an old-style one: its mask says what's
/// transparent (white in the mask).
fn bgra_to_rgba(bgra: &[u8], mask: Option<&[u8]>) -> Vec<u8> {
    let pixels = bgra.as_chunks::<4>().0;
    let has_alpha = pixels.iter().any(|p| p[3] != 0);
    let mut out = Vec::with_capacity(bgra.len());
    for (i, p) in pixels.iter().enumerate() {
        let alpha = if has_alpha {
            p[3]
        } else {
            let transparent = mask
                .and_then(|m| m.get(i * 4..i * 4 + 3))
                .is_some_and(|m| m.iter().any(|&b| b != 0));
            if transparent {
                0
            } else {
                255
            }
        };
        out.extend_from_slice(&[p[2], p[1], p[0], alpha]);
    }
    out
}

// ---- Windows ----------------------------------------------------------------------

/// An exe's icon as RGBA, and a package family's install folders.
#[cfg(windows)]
mod win {
    use std::ffi::{c_void, OsStr, OsString};
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::{Path, PathBuf};
    use std::ptr::null_mut;

    /// An HICON, HBITMAP or HDC.
    type Handle = *mut c_void;

    #[repr(C)]
    pub(super) struct IconInfo {
        pub is_icon: i32,
        pub x_hotspot: u32,
        pub y_hotspot: u32,
        pub mask: Handle,
        pub color: Handle,
    }

    #[repr(C)]
    struct Bitmap {
        kind: i32,
        width: i32,
        height: i32,
        width_bytes: i32,
        planes: u16,
        bits_pixel: u16,
        bits: *mut c_void,
    }

    #[repr(C)]
    struct BitmapInfoHeader {
        size: u32,
        width: i32,
        height: i32,
        planes: u16,
        bit_count: u16,
        compression: u32,
        size_image: u32,
        x_pels_per_meter: i32,
        y_pels_per_meter: i32,
        clr_used: u32,
        clr_important: u32,
    }

    /// The header, and room for a colour table should GetDIBits write one.
    #[repr(C)]
    struct BitmapInfo {
        header: BitmapInfoHeader,
        colors: [u32; 256],
    }

    #[link(name = "shell32")]
    extern "system" {
        fn SHDefExtractIconW(
            file: *const u16,
            index: i32,
            flags: u32,
            large: *mut Handle,
            small: *mut Handle,
            size: u32,
        ) -> i32;
    }
    #[link(name = "user32")]
    extern "system" {
        fn GetIconInfo(icon: Handle, info: *mut IconInfo) -> i32;
        pub(super) fn DestroyIcon(icon: Handle) -> i32;
        #[cfg(test)]
        pub(super) fn CreateIconIndirect(info: *const IconInfo) -> Handle;
    }
    #[link(name = "gdi32")]
    extern "system" {
        fn GetObjectW(object: Handle, size: i32, out: *mut c_void) -> i32;
        fn GetDIBits(
            dc: Handle,
            bitmap: Handle,
            start: u32,
            lines: u32,
            bits: *mut c_void,
            info: *mut BitmapInfo,
            usage: u32,
        ) -> i32;
        fn CreateCompatibleDC(dc: Handle) -> Handle;
        fn DeleteDC(dc: Handle) -> i32;
        pub(super) fn DeleteObject(object: Handle) -> i32;
        #[cfg(test)]
        pub(super) fn CreateBitmap(width: i32, height: i32, planes: u32, bits_per_pixel: u32, bits: *const c_void) -> Handle;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetPackagesByPackageFamily(
            family: *const u16,
            count: *mut u32,
            full_names: *mut *mut u16,
            buffer_length: *mut u32,
            buffer: *mut u16,
        ) -> i32;
        fn GetPackagePathByFullName(full_name: *const u16, path_length: *mut u32, path: *mut u16) -> i32;
    }

    const ERROR_SUCCESS: i32 = 0;
    const ERROR_INSUFFICIENT_BUFFER: i32 = 122;
    const BI_RGB: u32 = 0;
    const DIB_RGB_COLORS: u32 = 0;

    fn wide(s: &OsStr) -> Vec<u16> {
        s.encode_wide().chain(Some(0)).collect()
    }

    /// `file`'s icon (its first) drawn at `px` × `px`: (width, height, RGBA).
    pub fn exe_icon(file: &Path, px: u32) -> Option<(u32, u32, Vec<u8>)> {
        let path = wide(file.as_os_str());
        let mut icon: Handle = null_mut();
        // SAFETY: `path` is NUL-terminated; only the large icon is asked for.
        let hr = unsafe { SHDefExtractIconW(path.as_ptr(), 0, 0, &mut icon, null_mut(), px & 0xFFFF) };
        if hr != 0 || icon.is_null() {
            return None; // S_FALSE: no icon; E_FAIL: no such file
        }
        // SAFETY: `icon` is the icon just extracted, destroyed once read.
        unsafe {
            let rgba = icon_rgba(icon);
            DestroyIcon(icon);
            rgba
        }
    }

    /// Deletes the bitmaps GetIconInfo hands over.
    struct Bitmaps([Handle; 2]);

    impl Drop for Bitmaps {
        fn drop(&mut self) {
            for bitmap in self.0 {
                if !bitmap.is_null() {
                    // SAFETY: a bitmap this owns, deleted once.
                    unsafe { DeleteObject(bitmap) };
                }
            }
        }
    }

    /// An icon's pixels as straight RGBA (its alpha, or its mask for an icon
    /// without one). A monochrome icon has none to give.
    ///
    /// # Safety
    /// `icon` must be a valid HICON; it's left as it is.
    pub(super) unsafe fn icon_rgba(icon: Handle) -> Option<(u32, u32, Vec<u8>)> {
        let mut info: IconInfo = std::mem::zeroed();
        if GetIconInfo(icon, &mut info) == 0 {
            return None;
        }
        let _owned = Bitmaps([info.color, info.mask]);
        if info.color.is_null() {
            return None;
        }
        let mut bitmap: Bitmap = std::mem::zeroed();
        let size = std::mem::size_of::<Bitmap>() as i32;
        if GetObjectW(info.color, size, (&mut bitmap as *mut Bitmap).cast()) == 0 {
            return None;
        }
        let (width, height) = (bitmap.width, bitmap.height);
        if !(1..=1024).contains(&width) || !(1..=1024).contains(&height) {
            return None;
        }
        let dc = CreateCompatibleDC(null_mut());
        if dc.is_null() {
            return None;
        }
        let color = dib32(dc, info.color, width, height);
        let mask = if info.mask.is_null() { None } else { dib32(dc, info.mask, width, height) };
        DeleteDC(dc);
        Some((width as u32, height as u32, super::bgra_to_rgba(&color?, mask.as_deref())))
    }

    /// `bitmap`'s pixels as top-down 32-bit BGRA.
    unsafe fn dib32(dc: Handle, bitmap: Handle, width: i32, height: i32) -> Option<Vec<u8>> {
        let mut info: BitmapInfo = std::mem::zeroed();
        info.header.size = std::mem::size_of::<BitmapInfoHeader>() as u32;
        info.header.width = width;
        info.header.height = -height; // top-down
        info.header.planes = 1;
        info.header.bit_count = 32;
        info.header.compression = BI_RGB;
        let mut bits = vec![0u8; width as usize * height as usize * 4];
        let lines = GetDIBits(dc, bitmap, 0, height as u32, bits.as_mut_ptr().cast(), &mut info, DIB_RGB_COLORS);
        (lines == height).then_some(bits)
    }

    /// The install folders of `family`'s packages installed for this user
    /// (the app's own, and any resource packages beside it).
    pub fn package_dirs(family: &str) -> Vec<PathBuf> {
        let family = wide(OsStr::new(family));
        let (mut count, mut len) = (0u32, 0u32);
        // SAFETY: a size query: no buffers.
        let r = unsafe { GetPackagesByPackageFamily(family.as_ptr(), &mut count, null_mut(), &mut len, null_mut()) };
        if r != ERROR_INSUFFICIENT_BUFFER || count == 0 || len == 0 {
            return Vec::new(); // not installed (ERROR_SUCCESS with none)
        }
        let mut names: Vec<*mut u16> = vec![null_mut(); count as usize];
        let mut buf = vec![0u16; len as usize];
        // SAFETY: room for `count` names in `len` characters, as asked.
        let r = unsafe {
            GetPackagesByPackageFamily(family.as_ptr(), &mut count, names.as_mut_ptr(), &mut len, buf.as_mut_ptr())
        };
        if r != ERROR_SUCCESS {
            return Vec::new();
        }
        // Each name points into `buf`, NUL-terminated.
        let base = buf.as_ptr() as usize;
        names
            .iter()
            .take(count as usize)
            .filter_map(|&name| {
                let tail = buf.get((name as usize).checked_sub(base)? / 2..)?;
                let end = tail.iter().position(|&c| c == 0)?;
                package_path(&String::from_utf16_lossy(&tail[..end]))
            })
            .collect()
    }

    /// The folder a package (by its full name) is installed in.
    fn package_path(full_name: &str) -> Option<PathBuf> {
        let name = wide(OsStr::new(full_name));
        let mut len = 0u32;
        // SAFETY: a size query, then a buffer of the size it gave.
        let r = unsafe { GetPackagePathByFullName(name.as_ptr(), &mut len, null_mut()) };
        if r != ERROR_INSUFFICIENT_BUFFER || len == 0 {
            return None;
        }
        let mut path = vec![0u16; len as usize];
        let r = unsafe { GetPackagePathByFullName(name.as_ptr(), &mut len, path.as_mut_ptr()) };
        if r != ERROR_SUCCESS {
            return None;
        }
        let end = path.iter().position(|&c| c == 0).unwrap_or(path.len());
        Some(PathBuf::from(OsString::from_wide(&path[..end])))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder for one test.
    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yap-app-icons-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A `size` × `size` PNG in one colour.
    fn solid_png(size: u32, rgba: [u8; 4]) -> Vec<u8> {
        let pixels: Vec<u8> = (0..size * size).flat_map(|_| rgba).collect();
        encode_png(size, size, &pixels).unwrap()
    }

    fn put(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    fn app(id: &str) -> &'static App {
        APPS.iter().find(|a| a.id == id).unwrap()
    }

    // A package manifest shaped like new Teams' (a hidden helper application
    // first, the app list one second) and WhatsApp's.
    const MANIFEST: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<!-- <Application Id="Commented"><uap:VisualElements Square44x44Logo="Wrong.png"/></Application> -->
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
         xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10">
  <Identity Name="Contoso.Chat" Publisher="CN=Contoso" Version="1.2.3.0" />
  <Properties>
    <DisplayName>Contoso Chat</DisplayName>
    <Logo>Images\StoreLogo.png</Logo>
  </Properties>
  <Applications>
    <Application Id="Helper" Executable="helper.exe" EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements DisplayName="Helper" Square150x150Logo="Images\Med.png"
        Square44x44Logo="Images\HelperList.png" BackgroundColor="transparent" AppListEntry="none" />
    </Application>
    <Application Id="App" Executable="chat.exe" EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements DisplayName="Contoso &amp; Co" Description="Chat &gt; calls"
        BackgroundColor="transparent" Square150x150Logo='Images\Med.png' Square44x44Logo="Images\App&amp;List.png">
        <uap:DefaultTile Wide310x150Logo="Images\Wide.png" />
      </uap:VisualElements>
    </Application>
  </Applications>
</Package>"#;

    #[test]
    fn the_manifest_names_the_app_list_logo() {
        // The application in the app list, not the hidden helper or the
        // commented-out one; entities unescaped.
        assert_eq!(manifest_logo(MANIFEST).as_deref(), Some(r"Images\App&List.png"));
        // Only hidden applications: the first one's.
        let hidden = MANIFEST.replace(r#"BackgroundColor="transparent" Square150x150Logo='Images\Med.png'"#, r#"AppListEntry="none""#);
        assert_eq!(manifest_logo(&hidden).as_deref(), Some(r"Images\HelperList.png"));
        // Windows 8's names, and a package with no applications' logos.
        let win8 = r#"<Package><Applications><Application Id="App"><VisualElements DisplayName="Old" SmallLogo="Assets\Small.png" Logo="Assets\Logo.png"/></Application></Applications></Package>"#;
        assert_eq!(manifest_logo(win8).as_deref(), Some(r"Assets\Small.png"));
        let store_only = r#"<Package><Properties><Logo> Assets\StoreLogo.png </Logo></Properties><Applications/></Package>"#;
        assert_eq!(manifest_logo(store_only).as_deref(), Some(r"Assets\StoreLogo.png"));
        assert_eq!(manifest_logo("<Package><Applications/></Package>"), None);
        assert_eq!(manifest_logo("not xml at all"), None);
        assert_eq!(manifest_logo("<Package><Applications><Application Id=\"cut"), None);
    }

    #[test]
    fn a_little_xml() {
        let events = xml_events(r#"<a x="1>2" y='q"s'>t &lt;3 <![CDATA[<b>]]><c/><?pi?></a>"#);
        assert_eq!(
            events,
            vec![
                Xml::Start { name: "a", attrs: vec![("x", "1>2".into()), ("y", "q\"s".into())], closed: false },
                Xml::Text("t <3 ".into()),
                Xml::Text("<b>".into()),
                Xml::Start { name: "c", attrs: vec![], closed: true },
                Xml::End("a"),
            ]
        );
        assert_eq!(unescape("a&amp;b &#92; &#x41; &bogus; & end"), "a&b \\ A &bogus; & end");
        assert_eq!(local("uap:VisualElements"), "VisualElements");
    }

    #[test]
    fn logo_variant_names() {
        let q = |name: &str| file_qualifiers(name, "AppList", "png");
        assert_eq!(q("AppList.png"), Some(vec![]));
        assert_eq!(q("applist.targetsize-48_altform-lightunplated.png"), Some(vec!["targetsize-48".into(), "altform-lightunplated".into()]));
        assert_eq!(q("AppList.scale-200.png"), Some(vec!["scale-200".into()]));
        assert_eq!(q("AppListWide.png"), None);
        assert_eq!(q("AppList.backup.png"), None);
        assert_eq!(q("AppList.png.old"), None);
        assert_eq!(qualifiers("contrast-black"), Some(vec!["contrast-black".into()]));
        assert_eq!(qualifiers("images"), None);
    }

    fn variant(name: &str, px: u32) -> Variant {
        let qualifiers = file_qualifiers(name, "AppList", "png").unwrap();
        let px = qualifiers.iter().find_map(|q| q.strip_prefix("targetsize-")?.parse().ok()).unwrap_or(px);
        Variant { path: PathBuf::from(name), qualifiers, px }
    }

    fn pick(names: &[(&str, u32)]) -> String {
        let best = names.iter().map(|(n, px)| variant(n, *px)).min_by_key(rank).unwrap();
        best.path.to_string_lossy().into_owned()
    }

    #[test]
    fn the_variant_for_a_light_background_at_48_px() {
        // New Teams and WhatsApp ship these.
        let teams = [
            ("AppList.scale-100.png", 44),
            ("AppList.scale-200.png", 88),
            ("AppList.targetsize-24.png", 0),
            ("AppList.targetsize-40_altform-lightunplated.png", 0),
            ("AppList.targetsize-48.png", 0),
            ("AppList.targetsize-48_altform-unplated.png", 0),
            ("AppList.targetsize-48_altform-lightunplated.png", 0),
            ("AppList.targetsize-64_altform-lightunplated.png", 0),
            ("AppList.targetsize-256_altform-lightunplated.png", 0),
        ];
        assert_eq!(pick(&teams), "AppList.targetsize-48_altform-lightunplated.png");
        // Only scaled ones: the smallest at least 48 px.
        assert_eq!(pick(&[("AppList.scale-100.png", 44), ("AppList.scale-125.png", 55), ("AppList.scale-200.png", 88)]), "AppList.scale-125.png");
        // Nothing that big: the biggest there is.
        assert_eq!(pick(&[("AppList.targetsize-16.png", 0), ("AppList.targetsize-32.png", 0)]), "AppList.targetsize-32.png");
        // High-contrast and dark-theme ones only when there's nothing else.
        assert_eq!(pick(&[("AppList.targetsize-48_contrast-black.png", 0), ("AppList.targetsize-16.png", 0)]), "AppList.targetsize-16.png");
        assert_eq!(pick(&[("AppList.targetsize-48_theme-dark.png", 0), ("AppList.scale-100.png", 44)]), "AppList.scale-100.png");
        assert_eq!(pick(&[("AppList.targetsize-48_contrast-standard.png", 0), ("AppList.targetsize-16.png", 0)]), "AppList.targetsize-48_contrast-standard.png");
    }

    #[test]
    fn the_best_variant_on_disk() {
        let root = temp("variants");
        let (app_dir, res_dir) = (root.join("Contoso.Chat_1.0_x64__hash"), root.join("Contoso.Chat_1.0_neutral_split.scale-200_hash"));
        let png = solid_png(4, [1, 2, 3, 255]); // its header says 4 px
        put(&app_dir.join(r"Images\AppList.scale-100.png"), &png);
        put(&app_dir.join(r"Images\AppList.targetsize-48.png"), &png);
        put(&app_dir.join(r"Images\contrast-black\AppList.targetsize-48_altform-unplated.png"), &png);
        put(&app_dir.join(r"Images\Other.targetsize-48_altform-unplated.png"), &png);
        // The unplated one ships in a resource package, in a qualifier folder.
        put(&res_dir.join(r"Images\contrast-standard\AppList.targetsize-48_altform-unplated.png"), &png);
        let dirs = [app_dir.clone(), res_dir.clone()];
        assert_eq!(
            best_variant(&dirs, r"Images\AppList.png"),
            Some(res_dir.join(r"Images\contrast-standard\AppList.targetsize-48_altform-unplated.png"))
        );
        assert_eq!(best_variant(&dirs[..1], "Images/AppList.png"), Some(app_dir.join(r"Images\AppList.targetsize-48.png")));
        // Nothing outside the package, and only PNGs.
        assert_eq!(best_variant(&dirs, r"..\Images\AppList.png"), None);
        assert_eq!(best_variant(&dirs, r"C:\Images\AppList.png"), None);
        assert_eq!(best_variant(&dirs, r"\Images\AppList.png"), None);
        assert_eq!(best_variant(&dirs, r"Images\AppList.jpg"), None);
        assert_eq!(best_variant(&dirs, r"Images\Missing.png"), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn install_paths() {
        let env = |name: &str| match name {
            "LOCALAPPDATA" => Some(r"C:\Users\me\AppData\Local\".to_string()),
            "ProgramFiles(x86)" => Some(r"C:\Program Files (x86)".to_string()),
            "EMPTY" => Some(String::new()),
            _ => None,
        };
        assert_eq!(expand_vars(r"%LOCALAPPDATA%\Discord\app-*\Discord.exe", env).as_deref(), Some(r"C:\Users\me\AppData\Local\Discord\app-*\Discord.exe"));
        assert_eq!(expand_vars(r"%ProgramFiles(x86)%\Zoom\bin\Zoom.exe", env).as_deref(), Some(r"C:\Program Files (x86)\Zoom\bin\Zoom.exe"));
        assert_eq!(expand_vars(r"%APPDATA%\Zoom\bin\Zoom.exe", env), None);
        assert_eq!(expand_vars(r"%EMPTY%\x.exe", env), None);
        assert_eq!(expand_vars(r"%LOCALAPPDATA\x.exe", env), None);
        assert_eq!(expand_vars(r"C:\plain.exe", env).as_deref(), Some(r"C:\plain.exe"));
        // The microphone record spells `\` as `#`.
        assert_eq!(
            consent_path("C:#Users#me#AppData#Local#Discord#app-1.0.9260#Discord.exe"),
            PathBuf::from(r"C:\Users\me\AppData\Local\Discord\app-1.0.9260\Discord.exe")
        );
    }

    #[test]
    fn a_star_in_a_folder_is_the_newest_version_with_the_exe() {
        let root = temp("squirrel");
        let exe = |version: &str| root.join(format!(r"Discord\{version}\Discord.exe"));
        put(&exe("app-1.0.9259"), b"MZ");
        put(&exe("app-1.0.9260"), b"MZ");
        std::fs::create_dir_all(root.join(r"Discord\app-1.0.9261")).unwrap(); // mid-update: no exe yet
        put(&root.join(r"Discord\packages\Discord.exe"), b"MZ"); // not an app-* folder
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options().write(true).open(exe("app-1.0.9259")).unwrap().set_modified(old).unwrap();
        assert_eq!(find_file(&root.join(r"Discord\app-*\Discord.exe")), Some(exe("app-1.0.9260")));
        assert_eq!(find_file(&root.join(r"Discord\app-*\Missing.exe")), None);
        assert_eq!(find_file(&exe("app-1.0.9259")), Some(exe("app-1.0.9259")));
        assert_eq!(find_file(&root.join(r"Nope\app-*\Discord.exe")), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_tables_name_call_apps() {
        for (id, family) in FAMILIES {
            assert_eq!(crate::meeting_detect::app_of_family(family), Some(*id), "{family}");
        }
        for (id, pattern) in KNOWN_PATHS {
            let exe = pattern.rsplit('\\').next().unwrap();
            assert_eq!(crate::meeting_detect::app_of_exe(exe), Some(*id), "{pattern}");
        }
    }

    #[test]
    fn sources_come_from_the_microphone_record_first() {
        let root = temp("sources");
        let (old, new) = (root.join(r"old\Discord.exe"), root.join(r"new\Discord.exe"));
        put(&old, b"MZ");
        put(&new, b"MZ");
        let record = |path: &Path, last_used: u64| MicRecord {
            app: "discord",
            key: path.to_string_lossy().replace('\\', "#"),
            packaged: false,
            last_used,
        };
        let gone = root.join(r"gone\Discord.exe");
        let records = [record(&old, 10), record(&gone, 30), record(&new, 20), record(&new, 5)];
        let found = sources(app("discord"), &records);
        // The most recently used first, missing files skipped, no repeats;
        // the usual install folders (if any here) after them.
        assert_eq!(found[..2], [Source::Exe(new), Source::Exe(old)]);
        assert!(found[2..].iter().all(|s| matches!(s, Source::Exe(p) if p.starts_with(std::env::var("LOCALAPPDATA").unwrap_or_default()))));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn bgra_to_straight_rgba() {
        // With alpha: kept as is, B and R swapped.
        assert_eq!(bgra_to_rgba(&[1, 2, 3, 128, 4, 5, 6, 0], None), vec![3, 2, 1, 128, 6, 5, 4, 0]);
        // Without: the mask's white is transparent.
        let mask = [0, 0, 0, 0, 255, 255, 255, 0];
        assert_eq!(bgra_to_rgba(&[1, 2, 3, 0, 4, 5, 6, 0], Some(&mask)), vec![3, 2, 1, 255, 6, 5, 4, 0]);
        assert_eq!(bgra_to_rgba(&[1, 2, 3, 0], None), vec![3, 2, 1, 255]);
    }

    #[test]
    fn png_round_trip() {
        let rgba: Vec<u8> = (0..2 * 3 * 4).map(|i| (i * 10) as u8).collect();
        let png = encode_png(2, 3, &rgba).unwrap();
        assert_eq!(png_size(&png), Some((2, 3)));
        let mut reader = png::Decoder::new(std::io::Cursor::new(&png)).read_info().unwrap();
        let mut out = vec![0; reader.output_buffer_size().unwrap()];
        let frame = reader.next_frame(&mut out).unwrap();
        assert_eq!((frame.width, frame.height, frame.color_type), (2, 3, png::ColorType::Rgba));
        assert_eq!(out, rgba);
        assert_eq!(png_size(b"GIF89a not a png at all"), None);
        assert!(data_url(&png).starts_with("data:image/png;base64,iVBORw0KGgo"));
    }

    #[test]
    fn the_cache_follows_the_source() {
        let root = temp("cache");
        let (cache, logo) = (root.join("icons"), root.join(r"pkg\Images\AppList.targetsize-48.png"));
        put(&logo, &solid_png(48, [0, 120, 255, 255]));
        let zoom = |a: &App| if a.id == "zoom" { vec![Source::Logo(logo.clone())] } else { Vec::new() };
        let made = |cache: &Path| -> BTreeMap<String, Made> {
            serde_json::from_slice(&std::fs::read(cache.join(INDEX)).unwrap()).unwrap()
        };

        let icons = refresh(&cache, 1_000, zoom);
        assert_eq!(icons.keys().collect::<Vec<_>>(), ["zoom"]);
        assert_eq!(std::fs::read(cache.join("zoom.png")).unwrap(), std::fs::read(&logo).unwrap());
        assert_eq!(made(&cache)["zoom"].made, 1_000);
        // Unchanged and fresh: the cached icon, not made again.
        assert_eq!(refresh(&cache, 2_000, zoom), icons);
        assert_eq!(made(&cache)["zoom"].made, 1_000);
        // A week on, made again.
        refresh(&cache, 1_000 + MAX_AGE_SECS, zoom);
        assert_eq!(made(&cache)["zoom"].made, 1_000 + MAX_AGE_SECS);
        // The app updated (another logo): made again from it.
        put(&logo, &solid_png(64, [0, 120, 255, 255]));
        let icons = refresh(&cache, 1_000 + MAX_AGE_SECS + 1, zoom);
        assert_eq!(png_size(&std::fs::read(cache.join("zoom.png")).unwrap()), Some((64, 64)));
        assert_eq!(icons["zoom"], data_url(&std::fs::read(&logo).unwrap()));
        // A broken cache file is made again too.
        std::fs::write(cache.join("zoom.png"), b"junk").unwrap();
        assert_eq!(refresh(&cache, 1_000 + MAX_AGE_SECS + 2, zoom), icons);
        // Uninstalled: its icon goes.
        std::fs::remove_file(&logo).unwrap();
        assert!(refresh(&cache, 1_000 + MAX_AGE_SECS + 3, zoom).is_empty());
        assert!(!cache.join("zoom.png").exists());
        assert!(made(&cache).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_mode_stand_ins() {
        let dir = temp("stand-ins");
        put(&dir.join("teams.png"), &solid_png(48, [91, 95, 199, 255]));
        put(&dir.join("zoom.png"), b"not a png");
        put(&dir.join("notacallapp.png"), &solid_png(48, [0, 0, 0, 255]));
        let icons = stand_ins(&dir);
        assert_eq!(icons.keys().collect::<Vec<_>>(), ["teams"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn an_icon_comes_back_as_its_pixels() {
        use std::ffi::c_void;
        // 2 × 2, BGRA with alpha, and a mask (1 bpp, rows padded to 16 bits).
        let bgra: [u8; 16] = [255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 64, 10, 20, 30, 0];
        let mask: [u8; 4] = [0, 0, 0, 0];
        // SAFETY: bitmaps and an icon made here, from buffers that outlive the calls.
        let rgba = unsafe {
            let color = win::CreateBitmap(2, 2, 1, 32, bgra.as_ptr().cast::<c_void>());
            let mono = win::CreateBitmap(2, 2, 1, 1, mask.as_ptr().cast::<c_void>());
            let info = win::IconInfo { is_icon: 1, x_hotspot: 0, y_hotspot: 0, mask: mono, color };
            let icon = win::CreateIconIndirect(&info);
            assert!(!icon.is_null());
            let rgba = win::icon_rgba(icon);
            win::DestroyIcon(icon);
            win::DeleteObject(color);
            win::DeleteObject(mono);
            rgba
        };
        assert_eq!(rgba, Some((2, 2, vec![0, 0, 255, 255, 0, 255, 0, 128, 255, 0, 0, 64, 30, 20, 10, 0])));
    }

    #[cfg(windows)]
    #[test]
    fn an_old_style_icon_takes_its_mask() {
        use std::ffi::c_void;
        // No alpha anywhere; the mask's first row is transparent (bits set).
        let bgra: [u8; 16] = [255, 0, 0, 0, 0, 255, 0, 0, 0, 0, 255, 0, 10, 20, 30, 0];
        let mask: [u8; 4] = [0b1100_0000, 0, 0, 0];
        // SAFETY: as above.
        let rgba = unsafe {
            let color = win::CreateBitmap(2, 2, 1, 32, bgra.as_ptr().cast::<c_void>());
            let mono = win::CreateBitmap(2, 2, 1, 1, mask.as_ptr().cast::<c_void>());
            let info = win::IconInfo { is_icon: 1, x_hotspot: 0, y_hotspot: 0, mask: mono, color };
            let icon = win::CreateIconIndirect(&info);
            let rgba = win::icon_rgba(icon);
            win::DestroyIcon(icon);
            win::DeleteObject(color);
            win::DeleteObject(mono);
            rgba
        };
        let alpha: Vec<u8> = rgba.unwrap().2.chunks(4).map(|p| p[3]).collect();
        assert_eq!(alpha, [0, 0, 255, 255]);
    }

    #[cfg(windows)]
    #[test]
    fn a_system_exe_gives_its_icon_and_an_unknown_package_nothing() {
        let windir = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let (w, h, rgba) = win::exe_icon(&Path::new(&windir).join("explorer.exe"), ICON_PX).unwrap();
        assert_eq!((w, h), (ICON_PX, ICON_PX));
        assert!(rgba.chunks(4).any(|p| p[3] == 255) && rgba.chunks(4).any(|p| p[3] == 0));
        let png = exe_png(&Path::new(&windir).join("explorer.exe")).unwrap();
        assert_eq!(png_size(&png), Some((ICON_PX, ICON_PX)));
        assert!(win::exe_icon(Path::new(r"C:\no\such\app.exe"), ICON_PX).is_none());
        assert!(win::package_dirs("Contoso.NoSuchApp_0000000000000").is_empty());
        assert!(package_logo("Contoso.NoSuchApp_0000000000000").is_none());
    }

    /// The real thing, read-only, on this PC: which call apps resolve to
    /// their own icons, written as the cache would be —
    /// `YAP_ICONS_OUT=<folder> cargo test --lib app_icons::tests::this_machine -- --ignored --nocapture`.
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn this_machine() {
        let out = std::env::var_os("YAP_ICONS_OUT")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("yap-app-icons"));
        let records = crate::meeting_detect::mic_records();
        println!("in the microphone record: {:?}", records.iter().map(|r| (r.app, r.packaged)).collect::<Vec<_>>());
        for app in APPS {
            let found = sources(app, &records);
            let kinds: Vec<&str> = found
                .iter()
                .map(|s| match s {
                    Source::Exe(_) => "exe",
                    Source::Logo(_) => "package logo",
                })
                .collect();
            println!("{:>9}: {kinds:?} {:?}", app.id, found.first().map(|s| s.path().file_name().unwrap_or_default()));
        }
        let icons = refresh(&out, now_secs(), |app| sources(app, &records));
        println!("icons in {}: {:?}", out.display(), icons.keys().collect::<Vec<_>>());
    }
}

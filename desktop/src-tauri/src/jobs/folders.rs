//! **The folders jobs may work in** (`job_folders.json`): each with a name
//! ("garden-planner"), its path and an optional cap on its mode (a folder
//! that never runs unattended). Added in Settings → Jobs, or the first time
//! a job asks to work in a folder and the person allows it.
//!
//! **Which folder a job means** ([`resolve`]): one of these by its name (or
//! its last part), or a full path the person said themselves — never a
//! guess. A name no folder has, or a path the person never said, isn't one:
//! the model is told to ask which folder.
//!
//! Saved through `saving.rs`; unit tests keep a store per test thread.

#[cfg(not(test))]
use std::sync::Mutex;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::modes::Mode;

/// One folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Folder {
    pub name: String,
    /// Its full path.
    pub path: String,
    /// Jobs here never run above this mode (`Mode::as_str`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cap: Option<String>,
    /// Unix seconds.
    pub added_ts: u64,
}

impl Folder {
    pub fn cap_mode(&self) -> Option<Mode> {
        self.cap.as_deref().and_then(Mode::parse)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct Folders {
    folders: Vec<Folder>,
}

/// job_folders.json's saves.
pub static SAVER: crate::shell::saving::Saver = crate::shell::saving::Saver::new("jobFolders", "your job folders", save_now);

fn save_now() -> Result<(), crate::shell::saving::SaveError> {
    with_store(|s| save(s));
    SAVER.check()
}

#[cfg(not(test))]
fn path() -> std::path::PathBuf {
    super::store::data().join("job_folders.json")
}

#[cfg(not(test))]
static STATE: Mutex<Option<Folders>> = Mutex::new(None);

#[cfg(not(test))]
fn save(store: &Folders) {
    if crate::ai::eval::active() {
        return;
    }
    let _ = SAVER.write_json(&path(), store, false);
}

#[cfg(not(test))]
fn with_store<R>(f: impl FnOnce(&mut Folders) -> R) -> R {
    let mut guard = STATE.lock().unwrap_or_else(|p| p.into_inner());
    let store = guard.get_or_insert_with(|| SAVER.load(&path()));
    if SAVER.any_unread() && SAVER.refresh(&path(), store) {
        save(store);
    }
    f(store)
}

#[cfg(test)]
thread_local! {
    static TEST_STORE: std::cell::RefCell<Folders> = std::cell::RefCell::new(Folders::default());
}

#[cfg(test)]
fn save(_: &Folders) {}

#[cfg(test)]
fn with_store<R>(f: impl FnOnce(&mut Folders) -> R) -> R {
    TEST_STORE.with(|s| f(&mut s.borrow_mut()))
}

/// Every folder, as added.
pub fn list() -> Vec<Folder> {
    with_store(|s| s.folders.clone())
}

/// A path as compared: `\`, no trailing one, lower case.
pub fn norm(p: &str) -> String {
    p.trim().trim_matches('"').replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

/// A full path on this PC: `C:\…`, `\\server\share\…` (or `/…` elsewhere).
pub fn is_absolute(p: &str) -> bool {
    let p = p.trim().trim_matches('"');
    let b = p.as_bytes();
    (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')) || p.starts_with("\\\\") || (!cfg!(windows) && p.starts_with('/'))
}

/// A folder's name from its path: its last part.
pub fn name_of(path: &str) -> String {
    let p = path.trim().trim_matches('"').replace('/', "\\");
    let p = p.trim_end_matches('\\');
    p.rsplit('\\').next().filter(|n| !n.is_empty() && !n.ends_with(':')).unwrap_or(p).to_string()
}

/// **Add a folder** (Settings' Add folder…, or a job's first time there,
/// allowed): `path` a folder that's there; `name` its last part unless
/// given (made unique: "notes 2"). The folder already there: as it is (its
/// name and cap given anew when asked).
pub fn add(path: &str, name: Option<&str>, cap: Option<&str>) -> Result<Folder, String> {
    let path = path.trim().trim_matches('"').to_string();
    if !is_absolute(&path) {
        return Err("Pick a folder by its full path (like D:\\Projects\\garden-planner).".into());
    }
    if !Path::new(&path).is_dir() {
        return Err("That folder isn't there.".into());
    }
    let cap = match cap.map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) => Some(Mode::parse(c).ok_or("That isn't one of the modes.")?.as_str().to_string()),
        None => None,
    };
    let wanted = name.map(str::trim).filter(|n| !n.is_empty()).map(str::to_string).unwrap_or_else(|| name_of(&path));
    with_store(|s| {
        if let Some(f) = s.folders.iter_mut().find(|f| norm(&f.path) == norm(&path)) {
            if name.is_some() {
                f.name = wanted.clone();
            }
            if cap.is_some() {
                f.cap = cap.clone();
            }
            let out = f.clone();
            save(s);
            return Ok(out);
        }
        let mut unique = wanted.clone();
        let mut n = 2;
        while s.folders.iter().any(|f| f.name.eq_ignore_ascii_case(&unique)) {
            unique = format!("{wanted} {n}");
            n += 1;
        }
        let folder = Folder { name: unique, path: path.clone(), cap, added_ts: super::store::now_secs() };
        s.folders.push(folder.clone());
        save(s);
        tracing::info!(folders = s.folders.len(), "Jobs: a folder added");
        Ok(folder)
    })
}

/// Remove the folder named `name` (or with that path): whether one went.
pub fn remove(name: &str) -> bool {
    with_store(|s| {
        let before = s.folders.len();
        s.folders.retain(|f| !(f.name.eq_ignore_ascii_case(name.trim()) || norm(&f.path) == norm(name)));
        let gone = s.folders.len() != before;
        if gone {
            save(s);
        }
        gone
    })
}

/// Set (or clear: `None`) the folder `name`'s cap.
pub fn set_cap(name: &str, cap: Option<&str>) -> Result<Folder, String> {
    let cap = match cap.map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) => Some(Mode::parse(c).ok_or("That isn't one of the modes.")?.as_str().to_string()),
        None => None,
    };
    with_store(|s| {
        let f = s
            .folders
            .iter_mut()
            .find(|f| f.name.eq_ignore_ascii_case(name.trim()) || norm(&f.path) == norm(name))
            .ok_or("There's no job folder by that name.")?;
        f.cap = cap;
        let out = f.clone();
        save(s);
        Ok(out)
    })
}

/// Reset (`privacy/reset.rs`): every folder goes. How many.
pub fn reset() -> usize {
    with_store(|s| {
        let n = s.folders.len();
        s.folders.clear();
        save(s);
        n
    })
}

pub fn saved() -> Result<(), crate::shell::saving::SaveError> {
    SAVER.check()
}

// ---- which folder ----

/// Which folder a job's `folder` argument means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// One of the person's job folders.
    Known(Folder),
    /// A full path the person said, not a job folder yet: asked before a
    /// job works there (and added when allowed).
    New(Folder),
    /// Not one of them, nor a path the person said: ask which.
    Unknown,
    /// A path the person said that isn't a folder on this PC.
    Missing(String),
}

/// A name's words, for comparing: lower case, letters and digits ("WoW
/// AddOns" = "wow-addons" = "wow_addons").
pub fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
}

/// Does `folder` answer to `arg` (its name, or its path's last part)?
pub fn answers_to(folder: &Folder, arg: &str) -> bool {
    let w = words(arg);
    !w.is_empty() && (words(&folder.name) == w || words(&name_of(&folder.path)) == w || norm(&folder.path) == norm(arg))
}

/// Did the person say `path` (in `texts`, their own words)? Slashes and
/// case don't matter; a path said inside a longer one doesn't count.
pub fn said_path(texts: &[&str], path: &str) -> bool {
    let want = norm(path);
    if want.is_empty() {
        return false;
    }
    texts.iter().any(|t| {
        let t = t.replace('/', "\\").to_lowercase();
        t.match_indices(&want).any(|(at, _)| {
            let after = t[at + want.len()..].chars().next();
            after.is_none_or(|c| c.is_whitespace() || matches!(c, '\\' | '.' | ',' | ';' | ':' | '!' | '?' | ')' | '"' | '\'' | '\u{201d}'))
        })
    })
}

/// **The folder `arg` means**, of `folders`: by name, else a full path the
/// person said (`said`: their own words this turn, and the answer they said
/// yes to). Pure but for whether the path is a folder (`is_dir`).
pub fn resolve(arg: &str, said: &[&str], folders: &[Folder], is_dir: &dyn Fn(&str) -> bool) -> Resolved {
    let arg = arg.trim().trim_matches('"');
    if arg.is_empty() {
        return Resolved::Unknown;
    }
    if let Some(f) = folders.iter().find(|f| norm(&f.path) == norm(arg)) {
        return Resolved::Known(f.clone());
    }
    if is_absolute(arg) {
        if !said_path(said, arg) {
            return Resolved::Unknown;
        }
        if !is_dir(arg) {
            return Resolved::Missing(arg.to_string());
        }
        return Resolved::New(Folder { name: name_of(arg), path: arg.to_string(), cap: None, added_ts: 0 });
    }
    let named: Vec<&Folder> = folders.iter().filter(|f| answers_to(f, arg)).collect();
    match named.as_slice() {
        [one] => Resolved::Known((*one).clone()),
        _ => Resolved::Unknown,
    }
}

/// The folders as a model reads them: "garden-planner (D:\Projects\…)".
pub fn listing(folders: &[Folder]) -> String {
    if folders.is_empty() {
        return "none yet".to_string();
    }
    folders.iter().map(|f| format!("{} ({})", f.name, f.path)).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str, path: &str) -> Folder {
        Folder { name: name.into(), path: path.into(), cap: None, added_ts: 1 }
    }

    fn there(_: &str) -> bool {
        true
    }

    #[test]
    fn a_folder_is_found_by_its_name_never_guessed() {
        let folders = vec![folder("garden-planner", r"D:\Projects\garden-planner"), folder("WoW AddOns", r"D:\Games\Interface\AddOns")];
        let known = |arg: &str| match resolve(arg, &[], &folders, &there) {
            Resolved::Known(f) => Some(f.name),
            _ => None,
        };
        assert_eq!(known("garden-planner").as_deref(), Some("garden-planner"));
        assert_eq!(known("Garden Planner").as_deref(), Some("garden-planner"));
        assert_eq!(known("wow addons").as_deref(), Some("WoW AddOns"));
        assert_eq!(known("AddOns").as_deref(), Some("WoW AddOns"), "its path's last part");
        assert_eq!(known(r"d:/projects/garden-planner/").as_deref(), Some("garden-planner"), "its path");
        // Close isn't the same: never guessed.
        assert_eq!(resolve("garden", &[], &folders, &there), Resolved::Unknown);
        assert_eq!(resolve("my project", &[], &folders, &there), Resolved::Unknown);
        assert_eq!(resolve("", &[], &folders, &there), Resolved::Unknown);
    }

    #[test]
    fn a_new_path_counts_only_when_the_person_said_it() {
        let folders = vec![folder("garden-planner", r"D:\Projects\garden-planner")];
        let said = ["Start a job in D:\\Work\\recipe-box to fix the tests"];
        match resolve(r"D:\Work\recipe-box", &said, &folders, &there) {
            Resolved::New(f) => assert_eq!((f.name.as_str(), f.path.as_str()), ("recipe-box", r"D:\Work\recipe-box")),
            other => panic!("{other:?}"),
        }
        // Forward slashes and case: the same path.
        assert!(matches!(resolve("d:/work/RECIPE-BOX", &said, &folders, &there), Resolved::New(_)));
        // Not said by the person (a note's, the model's own): not one.
        assert_eq!(resolve(r"C:\Windows\System32", &said, &folders, &there), Resolved::Unknown);
        // A path inside a longer one said doesn't count.
        assert_eq!(resolve(r"D:\Work\recipe", &said, &folders, &there), Resolved::Unknown);
        // Said, but not a folder here.
        assert_eq!(resolve(r"D:\Work\recipe-box", &said, &folders, &|_| false), Resolved::Missing(r"D:\Work\recipe-box".into()));
    }

    #[test]
    fn paths_and_names() {
        assert!(is_absolute(r"C:\x") && is_absolute("d:/x") && is_absolute(r"\\nas\share"));
        assert!(!is_absolute("garden-planner") && !is_absolute(r"src\x") && !is_absolute("C:"));
        assert_eq!(name_of(r"D:\Projects\garden-planner\"), "garden-planner");
        assert_eq!(name_of("D:/Projects/notes"), "notes");
        assert_eq!(listing(&[]), "none yet");
        assert_eq!(listing(&[folder("a", r"C:\a")]), r"a (C:\a)");
    }

    #[test]
    fn folders_are_added_once_with_unique_names_and_caps() {
        let base = std::env::temp_dir().join(format!("yap-jobs-folders-{}", super::super::store::new_id()));
        let one = base.join("notes");
        let two = base.join("other").join("notes");
        std::fs::create_dir_all(&one).unwrap();
        std::fs::create_dir_all(&two).unwrap();
        let a = add(&one.to_string_lossy(), None, None).unwrap();
        assert_eq!(a.name, "notes");
        let b = add(&two.to_string_lossy(), None, Some("manual")).unwrap();
        assert_eq!((b.name.as_str(), b.cap.as_deref()), ("notes 2", Some("manual")));
        // The same path again: the same folder.
        assert_eq!(add(&one.to_string_lossy(), None, None).unwrap().name, "notes");
        assert_eq!(list().len(), 2);
        assert!(add("relative", None, None).is_err());
        assert!(add(&base.join("missing").to_string_lossy(), None, None).is_err());
        assert!(add(&one.to_string_lossy(), None, Some("yolo")).is_err());
        assert_eq!(set_cap("notes", Some("plan")).unwrap().cap_mode(), Some(Mode::Plan));
        assert_eq!(set_cap("notes", None).unwrap().cap, None);
        assert!(remove("notes 2"));
        assert!(!remove("notes 2"));
        assert_eq!(reset(), 1);
        let _ = std::fs::remove_dir_all(base);
    }
}

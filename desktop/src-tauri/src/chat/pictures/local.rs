//! **A picture from this PC** (8 Nov 2026): Chat shows a picture file that's
//! on this PC in the panel beside the chat, as it shows one it found online.
//! In a Code chat on his Claude plan the maintainer asked to see two concept
//! pictures an agent had made in his workspace; Chat tried to open them as
//! pages (`file:` addresses, refused: the pages beside the chat are public
//! http(s) only) and the agent fell back to his own browser and image viewer.
//!
//! - **The way in**: `show_picture` and `look_at_picture` take a file's path
//!   where they take a picture's id (`picture`, or `path`): a full path, a
//!   `file:` address, or a path inside the Code chat's workspace
//!   ([`path_of`]). The pages never load a file: `browser_navigate` given
//!   one says how to show a picture instead ([`not_a_page`]). **Without the
//!   web** (`turn/gates.rs` `files`): a Code chat, or a question naming a
//!   picture file ([`names_a_picture`]), still gets the two, by a path only
//!   (`pictures::file_definitions`, `pictures::execute_files`).
//! - **Which files** ([`Places`], [`check`]): inside one of the person's
//!   workspaces (`agents/workspaces.rs`: any of them, the Code chat's own
//!   too), a file an agent of this chat created or changed (`agents/store.rs`
//!   `files`), or a path the person wrote or said in this conversation (the
//!   file itself, or a folder holding it written as a folder — never a
//!   drive's root, nor a named file's neighbours: [`written_whole`]). The path is judged
//!   as written (`.` and `..` worked out first) and again once the file
//!   system has resolved it (a link or junction out of a workspace refused;
//!   a link the person named themselves is theirs). Anything else is
//!   refused in plain words the model passes on. The same rule for a look,
//!   which sends the picture to the model, as for a show.
//! - **What's read** ([`read`]): at most [`MAX_BYTES`], and only a picture by
//!   its own bytes ([`kind`]: PNG, JPEG, GIF, WebP, BMP), whatever its name
//!   says; then decoded and stored like a pasted picture
//!   (`snip::attach::keep`: a PNG in `chat-images/`, its thumbnails).
//! - **Local web pages** (`.html`): never opened beside the chat; the model
//!   says so and shows a picture instead (not done: a page from this PC).
//!
//! **Privacy**: the log never has a path or a file's name — sizes, ids and
//! which kind of place allowed it.

use std::io::Read;
use std::path::{Component, Path, PathBuf};

use crate::agents::workspaces::{is_absolute, norm};

/// A picture file's most bytes.
pub const MAX_BYTES: u64 = 25 * 1024 * 1024;

/// **The turn's chat**, to judge a path by: its conversation (the Code
/// chat's workspace, its agents' files) and the person's words in it (this
/// question and their earlier messages).
#[derive(Debug, Clone, Default)]
pub struct Context {
    pub conversation: Option<u64>,
    pub said: Vec<String>,
}

impl Context {
    /// For a turn of `conv` asking `question`: the person's words are the
    /// question and every message of theirs in it.
    pub fn of_chat(conv: &crate::chat::chats::Conversation, question: &str) -> Context {
        let mut said: Vec<String> = conv.messages.iter().filter(|m| m.role == "user").map(|m| m.text.clone()).collect();
        if !said.iter().any(|s| s == question) {
            said.push(question.to_string());
        }
        Context { conversation: Some(conv.id), said }
    }

    /// [`Places`] as they are now.
    pub fn places(&self) -> Places {
        Places::of(self)
    }
}

/// Why a file may be shown (for the log: never its path).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    Workspace,
    Agent,
    Said,
}

impl Why {
    pub fn word(self) -> &'static str {
        match self {
            Why::Workspace => "workspace",
            Why::Agent => "agent",
            Why::Said => "said",
        }
    }
}

/// **Where a turn may take a picture file from**.
#[derive(Debug, Clone, Default)]
pub struct Places {
    /// The workspaces' folders (the Code chat's first), as written and as
    /// the file system has them: any file inside.
    pub roots: Vec<String>,
    /// The files this chat's agents created or changed, full paths.
    pub files: Vec<String>,
    /// The person's words in this conversation (a `file:` address in them
    /// as its path too).
    pub said: Vec<String>,
    /// Where a path that isn't full is from: the Code chat's workspace.
    pub base: Option<String>,
}

impl Places {
    /// For `ctx`: every workspace, the Code chat's own, the files the chat's
    /// agents changed.
    pub fn of(ctx: &Context) -> Places {
        let code = ctx.conversation.and_then(crate::agents::store::code_chat_of).map(|c| c.path).filter(|p| !p.trim().is_empty());
        let mut roots: Vec<String> = code.iter().cloned().collect();
        roots.extend(crate::agents::workspaces::list().into_iter().map(|f| f.path));
        let files: Vec<String> = ctx
            .conversation
            .map(crate::agents::store::of_conversation)
            .unwrap_or_default()
            .into_iter()
            .flat_map(|a| {
                let folder = PathBuf::from(&a.path);
                a.files.into_iter().map(move |f| plain(&clean(&folder.join(f))))
            })
            .collect();
        Places::new(roots, files, &ctx.said, code)
    }

    /// Built from what's known (each folder and file also as the file system
    /// resolves it). Pure but for that.
    pub fn new(roots: Vec<String>, files: Vec<String>, said: &[String], base: Option<String>) -> Places {
        let with_real = |paths: Vec<String>| -> Vec<String> {
            let mut out = Vec::new();
            for p in paths {
                if let Ok(real) = std::fs::canonicalize(&p) {
                    let real = plain(&real);
                    if norm(&real) != norm(&p) {
                        out.push(real);
                    }
                }
                out.push(p);
            }
            out
        };
        let mut said: Vec<String> = said.to_vec();
        let addresses: Vec<String> = said.iter().flat_map(|t| file_addresses(t)).collect();
        said.extend(addresses);
        Places { roots: with_real(roots), files: with_real(files), said, base }
    }

    /// Why `path` (full, worked out) may be shown, if it may. Pure.
    pub fn allows(&self, path: &str) -> Option<Why> {
        let p = norm(path);
        if p.is_empty() {
            return None;
        }
        if self.roots.iter().map(|r| norm(r)).any(|r| !r.is_empty() && (p == r || p.starts_with(&format!("{r}\\")))) {
            return Some(Why::Workspace);
        }
        if self.files.iter().any(|f| norm(f) == p) {
            return Some(Why::Agent);
        }
        self.said_by_person(path).then_some(Why::Said)
    }

    /// **The person wrote or said `path`** — the file itself, whole — **or
    /// a folder holding it, written as a folder** (its path whole, not the
    /// start of a file's: never a drive's root). A file's path they wrote
    /// allows that file only, never its neighbours (8 Nov 2026: naming
    /// `…\Desktop\a.png` had allowed every file on the Desktop). Pure.
    fn said_by_person(&self, path: &str) -> bool {
        let texts: Vec<&str> = self.said.iter().map(String::as_str).collect();
        Path::new(path).ancestors().filter(|a| !is_root(a)).any(|a| written_whole(&texts, &a.to_string_lossy()))
    }
}

/// **`path` written whole in one of `texts`** (slashes and case aside): not
/// the start of a longer path — after it the text ends, or a space or a
/// sentence's mark follows (a `.` only when no more letters do: "docs." but
/// never "docs.old"), or one last separator does (`C:\Projects\garden\`, a
/// folder) — and not the end of one. Pure.
fn written_whole(texts: &[&str], path: &str) -> bool {
    let want = crate::agents::workspaces::norm(path);
    if want.is_empty() {
        return false;
    }
    let ends = |c: Option<char>| c.is_none_or(|c| c.is_whitespace() || matches!(c, ',' | ';' | ':' | '!' | '?' | ')' | ']' | '>' | '"' | '\'' | '`' | '\u{201d}' | '\u{2019}'));
    texts.iter().any(|t| {
        let t = t.replace('/', "\\").to_lowercase();
        t.match_indices(&want).any(|(at, _)| {
            let before = t[..at].chars().next_back();
            if before.is_some_and(|c| c.is_alphanumeric() || matches!(c, '\\' | '.' | '_' | '-')) {
                return false;
            }
            let mut rest = t[at + want.len()..].chars();
            match rest.next() {
                // A sentence's full stop, or a folder's last separator: only
                // when nothing of a path follows.
                Some('.') | Some('\\') => ends(rest.next()),
                c => ends(c),
            }
        })
    })
}

/// A drive's root, a share's, or no path at all.
fn is_root(p: &Path) -> bool {
    p.parent().is_none() || !p.components().any(|c| matches!(c, Component::Normal(_)))
}

/// The `file:` addresses in `text`, as paths ("file:///C:/Garden%20planner/a.png"
/// → "C:\Garden planner\a.png"). Pure.
fn file_addresses(text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    lower
        .match_indices("file:")
        .filter_map(|(at, _)| {
            let rest = &text[at..];
            let end = rest.find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | ')' | ']' | '`')).unwrap_or(rest.len());
            let address = rest[..end].trim_end_matches(['.', ',', ';', ':', '!', '?']);
            from_address(address).map(|p| plain(&p))
        })
        .collect()
}

/// A `file:` address's path. Pure.
fn from_address(address: &str) -> Option<PathBuf> {
    reqwest::Url::parse(address).ok().filter(|u| u.scheme() == "file").and_then(|u| u.to_file_path().ok())
}

/// **What the model gave names a file** (not a picture's id like "p3"): a
/// `file:` address, a full path, or something with a folder or a file
/// ending in it. Pure.
pub fn is_path(given: &str) -> bool {
    let g = given.trim().trim_matches(['"', '\'']);
    if g.is_empty() {
        return false;
    }
    g.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("file:")) || is_absolute(g) || g.contains(['/', '\\']) || Path::new(g).extension().is_some()
}

/// The picture files' endings [`kind`] takes.
const PICTURE_ENDINGS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];

/// **Words naming a picture file on this PC**: a full path (`C:\…`, `\\…`)
/// or a `file:` address ending in a picture's ending, on one line ("can you
/// show me C:\Projects\garden-planner\docs\concept.png?"). Never a web
/// picture's address. Pure.
pub fn names_a_picture(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    PICTURE_ENDINGS.iter().any(|end| {
        let dotted = format!(".{end}");
        lower.match_indices(&dotted).any(|(at, _)| {
            if lower[at + dotted.len()..].chars().next().is_some_and(|c| c.is_alphanumeric()) {
                return false;
            }
            let start = lower[..at].rfind('\n').map_or(0, |i| i + 1);
            let line = &lower.as_bytes()[start..at];
            let drive = line.windows(3).enumerate().any(|(i, w)| {
                w[0].is_ascii_alphabetic() && w[1] == b':' && matches!(w[2], b'\\' | b'/') && (i == 0 || !line[i - 1].is_ascii_alphanumeric())
            });
            let address = line.windows(5).enumerate().any(|(i, w)| w == b"file:" && (i == 0 || !line[i - 1].is_ascii_alphanumeric()));
            drive || address || line.windows(2).any(|w| w == b"\\\\")
        })
    })
}

/// `p` with `.` and `..` worked out, without the file system. Pure.
pub fn clean(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// A path as a person writes it: Windows' `\\?\` prefix (the file system's
/// own form) left off. Pure.
pub fn plain(p: &Path) -> String {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = s.strip_prefix(r"\\?\") {
        rest.to_string()
    } else {
        s.into_owned()
    }
}

/// **A refusal**: what the model is told, and the chip's few words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub error: String,
    pub chip: &'static str,
}

fn refused(error: impl Into<String>, chip: &'static str) -> Refused {
    Refused { error: error.into(), chip }
}

/// What a refusal says when the place isn't one Yap shows from.
pub const NOT_ALLOWED: &str = "Yap only shows a picture from this PC that's in one of the user's workspaces, that an agent of this chat made or changed, or whose path the user wrote or said in this chat. This one is none of those: ask the user for its path, or to add its folder as a workspace in Settings → Yap Code.";

/// **The file `given` names**: a `file:` address's path, a full path, or a
/// path inside the Code chat's workspace (`base`), `.` and `..` worked out.
/// Pure.
pub fn path_of(given: &str, base: Option<&str>) -> Result<PathBuf, Refused> {
    let g = given.trim().trim_matches(['"', '\'']);
    let path = if g.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("file:")) {
        from_address(g).ok_or_else(|| refused("That file: address isn't a file on this PC. Give the picture's full path.", "not a file on this PC"))?
    } else if is_absolute(g) {
        PathBuf::from(g)
    } else if let Some(base) = base {
        Path::new(base).join(g)
    } else {
        return Err(refused("Give the picture's full path (like C:\\Projects\\garden-planner\\docs\\concept.png).", "no full path"));
    };
    Ok(clean(&path))
}

/// The file endings of a web page saved on this PC.
const PAGE_ENDINGS: &[&str] = &["html", "htm", "xhtml", "mht", "mhtml"];

fn ending(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

/// What a local web page gets: never opened beside the chat.
const NOT_A_LOCAL_PAGE: &str = "That's a web page saved on this PC (HTML): Yap never opens a file from this PC beside the chat, only pages from the web. Tell the user, and offer to show a picture of it instead if there is one (show_picture with its path).";

/// **May `given` be shown**, and why: the file it names
/// ([`path_of`]), in one of `places` as written and again as the file system
/// resolves it, there, a file, at most [`MAX_BYTES`], not a web page. The
/// file's real path. Reads the file system only.
pub fn check(given: &str, places: &Places) -> Result<(PathBuf, Why), Refused> {
    let path = path_of(given, places.base.as_deref())?;
    if PAGE_ENDINGS.contains(&ending(&path).as_str()) {
        return Err(refused(NOT_A_LOCAL_PAGE, "a web page on this PC"));
    }
    let (real, why, meta) = resolve(&path, places, Noun::Picture)?;
    if meta.len() > MAX_BYTES {
        return Err(refused(format!("That file is bigger than {} MB: too big to show.", MAX_BYTES / (1024 * 1024)), "too big"));
    }
    Ok((real, why))
}

/// What a refusal calls the file: a picture (`show_picture`,
/// `look_at_picture`) or any file (`share_files`, `chat/shares.rs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Noun {
    Picture,
    File,
}

/// What a share's refusal says when the place isn't one Yap shares from.
pub const NOT_ALLOWED_FILE: &str = "Yap only shares a file from this PC that's in one of the user's workspaces, that an agent of this chat made or changed, or whose path the user wrote or said in this chat. This one is none of those: ask the user for its path, or to add its folder as a workspace in Settings → Yap Code.";

/// **The file `path` (worked out: [`path_of`]) in one of `places`**, as
/// written and again as the file system resolves it (a link or junction out
/// of a workspace refused; one the person named themselves is theirs), and
/// a file, not a folder: its real path, why, and its metadata. No size cap
/// (a picture's is [`check`]'s). Reads the file system only.
pub fn resolve(path: &Path, places: &Places, noun: Noun) -> Result<(PathBuf, Why, std::fs::Metadata), Refused> {
    let (not_allowed, chip) = match noun {
        Noun::Picture => (NOT_ALLOWED, "not a place Yap shows pictures from"),
        Noun::File => (NOT_ALLOWED_FILE, "not a place Yap shares files from"),
    };
    let written = plain(path);
    let Some(why) = places.allows(&written) else {
        return Err(refused(not_allowed, chip));
    };
    let real = match std::fs::canonicalize(path) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(refused("That file isn't there: check its path (the agent's own list of the files it made, or ask the user).", "not there")),
        Err(_) => return Err(refused("Yap couldn't open that file.", "couldn't open it")),
    };
    // Resolved: still one of them (a link out of a workspace isn't), unless
    // the person named the very path themselves.
    let why = match places.allows(&plain(&real)) {
        Some(w) => w,
        None if why == Why::Said => why,
        None => return Err(refused(not_allowed, chip)),
    };
    let meta = std::fs::metadata(&real).map_err(|_| refused("Yap couldn't open that file.", "couldn't open it"))?;
    if !meta.is_file() {
        let error = match noun {
            Noun::Picture => "That's a folder, not a picture file: give the picture's own path.",
            Noun::File => "That's a folder, not a file: give a file's own path.",
        };
        return Err(refused(error, "a folder"));
    }
    Ok((real, why, meta))
}

/// **A picture's kind by its own bytes**: PNG, JPEG, GIF, WebP or BMP, else
/// `None` (whatever the file is called). Pure.
pub fn kind(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else if bytes.len() >= 26 && bytes.starts_with(b"BM") && matches!(u32::from_le_bytes([bytes[14], bytes[15], bytes[16], bytes[17]]), 12 | 40 | 52 | 56 | 64 | 108 | 124) {
        Some("bmp")
    } else {
        None
    }
}

/// **The picture file's bytes** (a path [`check`] gave): at most
/// [`MAX_BYTES`] (a file that grew since is stopped there), and a picture
/// by its bytes ([`kind`]).
pub fn read(path: &Path) -> Result<Vec<u8>, Refused> {
    let file = std::fs::File::open(path).map_err(|_| refused("Yap couldn't open that file.", "couldn't open it"))?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes).map_err(|_| refused("Yap couldn't read that file.", "couldn't read it"))?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(refused(format!("That file is bigger than {} MB: too big to show.", MAX_BYTES / (1024 * 1024)), "too big"));
    }
    if kind(&bytes).is_none() {
        return Err(refused("That file isn't a picture (PNG, JPEG, GIF, WebP or BMP), whatever its name says.", "not a picture"));
    }
    Ok(bytes)
}

/// A file's name and its folder, for the picture's tab: "concept.png",
/// "C:\Projects\garden-planner\docs" and that folder's name ("docs"). Pure.
pub fn names(path: &Path) -> (String, String, String) {
    let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let folder = path.parent().map(plain).unwrap_or_default();
    let folder_name = path.parent().and_then(Path::file_name).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| folder.clone());
    (file, folder, folder_name)
}

/// The key a picture file is known by in a turn (shown once, read once).
pub fn key(path: &Path) -> String {
    format!("file:{}", norm(&plain(path)))
}

/// **A file given to the pages beside the chat** (`browser_navigate`, a new
/// tab): a `file:` address or a path on this PC is never loaded there — the
/// pages are public http(s) only — so what the model is told instead, by
/// the kind of file: a picture goes beside the chat with `show_picture`, a
/// web page saved here isn't opened at all. `None`: not a file. Pure.
pub fn not_a_page(url: &str) -> Option<String> {
    let u = url.trim();
    let file = u.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("file:"));
    if !file && !is_absolute(u) {
        return None;
    }
    let path = if file { from_address(u).unwrap_or_else(|| PathBuf::from(u)) } else { PathBuf::from(u) };
    let end = ending(&path);
    Some(if PAGE_ENDINGS.contains(&end.as_str()) {
        NOT_A_LOCAL_PAGE.to_string()
    } else if crate::snip::attach::is_image_name(&path.to_string_lossy()) {
        "That's a picture file on this PC, not a web page: the pages beside the chat only open the web (http and https). show_picture shows it beside the chat: picture set to the file's full path.".to_string()
    } else {
        "That's a file on this PC, not a web page: the pages beside the chat only open the web (http and https). A picture from this PC (PNG, JPEG, GIF, WebP or BMP) goes beside the chat with show_picture and its full path.".to_string()
    })
}

/// The chip's name for a file given to the pages: its name, not its address.
/// Pure.
pub fn chip_name(url: &str) -> String {
    let u = url.trim();
    let path = if u.get(..5).is_some_and(|s| s.eq_ignore_ascii_case("file:")) { from_address(u).unwrap_or_else(|| PathBuf::from(u)) } else { PathBuf::from(u) };
    path.file_name().map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.is_empty()).unwrap_or_else(|| "a file".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder of this test's own under the temp folder.
    fn folder(name: &str) -> PathBuf {
        let mut raw = [0u8; 6];
        let _ = getrandom::fill(&mut raw);
        let tag: String = raw.iter().map(|b| format!("{b:02x}")).collect();
        let dir = std::env::temp_dir().join(format!("yap-local-pictures-{tag}")).join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let rgba: Vec<u8> = (0..w * h).flat_map(|i| [(i % 251) as u8, 120, 60, 255]).collect();
        crate::snip::images::encode_png(&rgba, w, h, true).unwrap()
    }

    fn s(p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn a_picture_is_known_by_its_bytes_not_its_name() {
        assert_eq!(kind(&png(2, 2)), Some("png"));
        assert_eq!(kind(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0x10]), Some("jpeg"));
        assert_eq!(kind(b"GIF89a\x01\x00\x01\x00"), Some("gif"));
        assert_eq!(kind(b"RIFF\x24\x00\x00\x00WEBPVP8 "), Some("webp"));
        let mut bmp = b"BM".to_vec();
        bmp.extend_from_slice(&[0u8; 12]);
        bmp.extend_from_slice(&40u32.to_le_bytes());
        bmp.extend_from_slice(&[0u8; 12]);
        assert_eq!(kind(&bmp), Some("bmp"));
        for not in [&b"hello, this is not a picture"[..], b"<svg xmlns='http://www.w3.org/2000/svg'/>", b"BM but not a bitmap at all", b"", b"RIFF\x24\x00\x00\x00WAVEfmt "] {
            assert_eq!(kind(not), None, "{not:?}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn what_the_model_gives_is_a_path_or_an_id() {
        for path in ["C:\\Projects\\garden-planner\\docs\\concept.png", "file:///C:/Projects/garden-planner/docs/concept.png", "docs/concept.png", "concept.png", "\\\\nas\\share\\a.jpg"] {
            assert!(is_path(path), "{path}");
        }
        for id in ["p3", "P3", "3", " p12 ", ""] {
            assert!(!is_path(id), "{id}");
        }
        // `.` and `..` worked out before anything's judged.
        assert_eq!(clean(Path::new("C:\\Projects\\garden-planner\\docs\\..\\..\\secret\\x.png")), PathBuf::from("C:\\Projects\\secret\\x.png"));
        assert_eq!(clean(Path::new("C:\\..\\..\\x.png")), PathBuf::from("C:\\x.png"));
        // A relative path is the Code chat's workspace's; without one, the full path is asked for.
        assert_eq!(path_of("docs/./concept.png", Some("C:\\Projects\\garden-planner")).unwrap(), PathBuf::from("C:\\Projects\\garden-planner\\docs\\concept.png"));
        assert_eq!(path_of("concept.png", None).unwrap_err().chip, "no full path");
        assert_eq!(path_of("file:///C:/Garden%20planner/docs/concept.png", None).unwrap(), PathBuf::from("C:\\Garden planner\\docs\\concept.png"));
        assert_eq!(plain(Path::new(r"\\?\C:\Projects\a.png")), r"C:\Projects\a.png");
        assert_eq!(plain(Path::new(r"\\?\UNC\nas\share\a.png")), r"\\nas\share\a.png");
        let (file, folder, name) = names(Path::new("C:\\Projects\\garden-planner\\docs\\concept.png"));
        assert_eq!((file.as_str(), folder.as_str(), name.as_str()), ("concept.png", "C:\\Projects\\garden-planner\\docs", "docs"));
    }

    #[test]
    fn only_a_workspace_an_agents_file_or_a_path_the_person_gave() {
        let garden = folder("garden-planner");
        let docs = garden.join("docs");
        std::fs::create_dir_all(&docs).unwrap();
        let concept = docs.join("concept.png");
        std::fs::write(&concept, png(8, 6)).unwrap();
        let elsewhere = folder("elsewhere");
        let other = elsewhere.join("other.png");
        std::fs::write(&other, png(4, 4)).unwrap();

        // Nothing allowed: refused, the reason the model passes on.
        let none = Places::new(Vec::new(), Vec::new(), &[], None);
        let r = check(&s(&concept), &none).unwrap_err();
        assert_eq!(r.error, NOT_ALLOWED);
        assert!(r.error.contains("workspaces") && r.error.contains("Settings → Yap Code"));

        // A workspace: any file inside it, by its full path or one relative to the Code chat's.
        let ws = Places::new(vec![s(&garden)], Vec::new(), &[], Some(s(&garden)));
        let (real, why) = check(&s(&concept), &ws).unwrap();
        assert_eq!(why, Why::Workspace);
        assert!(norm(&plain(&real)).ends_with("\\docs\\concept.png"), "{real:?}");
        assert_eq!(check("docs/concept.png", &ws).unwrap().1, Why::Workspace);
        // …never out of it, however it's written.
        let out = format!("{}\\..\\..\\elsewhere\\other.png", s(&docs));
        assert_eq!(check(&out, &ws).unwrap_err().error, NOT_ALLOWED);
        assert_eq!(check(&s(&other), &ws).unwrap_err().error, NOT_ALLOWED);
        // A folder whose name starts the same isn't inside it.
        let sibling = folder("garden-planner-old");
        std::fs::write(sibling.join("x.png"), png(2, 2)).unwrap();
        assert_eq!(check(&s(&sibling.join("x.png")), &ws).unwrap_err().error, NOT_ALLOWED);

        // A file an agent of this chat made or changed.
        let agent = Places::new(Vec::new(), vec![s(&other)], &[], None);
        assert_eq!(check(&s(&other), &agent).unwrap().1, Why::Agent);
        assert_eq!(check(&s(&elsewhere.join("next-to-it.png")), &agent).unwrap_err().error, NOT_ALLOWED);

        // A path the person wrote (slashes and case aside), or its folder; a file: address too.
        let said = Places::new(Vec::new(), Vec::new(), &[format!("Can you show me {}?", s(&other).replace('\\', "/").to_uppercase())], None);
        assert_eq!(check(&s(&other), &said).unwrap().1, Why::Said);
        // A file's path allows that file only, never its neighbours (8 Nov 2026).
        std::fs::write(elsewhere.join("neighbour.png"), png(2, 2)).unwrap();
        assert_eq!(check(&s(&elsewhere.join("neighbour.png")), &said).unwrap_err().error, NOT_ALLOWED);
        // A folder's path allows the files under it, at any depth.
        let folder_said = Places::new(Vec::new(), Vec::new(), &[format!("the pictures in {} please", s(&elsewhere))], None);
        assert_eq!(check(&s(&other), &folder_said).unwrap().1, Why::Said);
        assert_eq!(check(&s(&elsewhere.join("neighbour.png")), &folder_said).unwrap().1, Why::Said);
        let deeper = elsewhere.join("2026").join("spring");
        std::fs::create_dir_all(&deeper).unwrap();
        std::fs::write(deeper.join("beds.png"), png(2, 2)).unwrap();
        assert_eq!(check(&s(&deeper.join("beds.png")), &folder_said).unwrap().1, Why::Said);
        // …written with its last separator, or at a sentence's end, too.
        for written in [format!("everything in {}{}", s(&elsewhere), std::path::MAIN_SEPARATOR), format!("Look in {}.", s(&elsewhere))] {
            let p = Places::new(Vec::new(), Vec::new(), std::slice::from_ref(&written), None);
            assert_eq!(check(&s(&other), &p).unwrap().1, Why::Said, "{written}");
        }
        // Never the start of a longer path: a folder whose name only starts
        // the same ("elsewhere.old", "elsewhere-2026"), nor a file in it.
        let longer = Places::new(Vec::new(), Vec::new(), &[format!("see {}.old{}a.png and {}-2026", s(&elsewhere), std::path::MAIN_SEPARATOR, s(&elsewhere))], None);
        assert_eq!(check(&s(&other), &longer).unwrap_err().error, NOT_ALLOWED);
        let url = reqwest::Url::from_file_path(&other).unwrap().to_string();
        let address_said = Places::new(Vec::new(), Vec::new(), &[format!("open {url}")], None);
        assert_eq!(check(&url, &address_said).unwrap().1, Why::Said);
        // Never a drive's root said ("everything on C:").
        let root = Path::new(&s(&other)).ancestors().last().unwrap().to_string_lossy().into_owned();
        let root_said = Places::new(Vec::new(), Vec::new(), &[format!("anything on {root}")], None);
        assert_eq!(check(&s(&other), &root_said).unwrap_err().error, NOT_ALLOWED);
    }

    #[test]
    fn a_renamed_text_file_a_big_one_a_folder_and_a_web_page_are_refused() {
        let garden = folder("garden-planner");
        let ws = Places::new(vec![s(&garden)], Vec::new(), &[], Some(s(&garden)));
        // Called .png, but text inside: not a picture.
        let fake = garden.join("notes.png");
        std::fs::write(&fake, "These are my notes, not a picture.").unwrap();
        let (real, _) = check(&s(&fake), &ws).unwrap();
        let r = read(&real).unwrap_err();
        assert_eq!(r.chip, "not a picture");
        assert!(r.error.contains("isn't a picture"));
        // Over 25 MB: refused before it's read.
        let big = garden.join("huge.png");
        let f = std::fs::File::create(&big).unwrap();
        f.set_len(MAX_BYTES + 1).unwrap();
        drop(f);
        assert_eq!(check(&s(&big), &ws).unwrap_err().chip, "too big");
        // Not there; a folder.
        assert_eq!(check("docs/missing.png", &ws).unwrap_err().chip, "not there");
        assert_eq!(check(&s(&garden), &ws).unwrap_err().chip, "a folder");
        // A web page saved here: never shown or opened.
        std::fs::write(garden.join("concept.html"), "<html></html>").unwrap();
        let r = check("concept.html", &ws).unwrap_err();
        assert_eq!(r.chip, "a web page on this PC");
        assert!(r.error.contains("never opens a file from this PC beside the chat"));
        // A real picture reads.
        std::fs::write(garden.join("concept.png"), png(3, 3)).unwrap();
        let (real, _) = check("concept.png", &ws).unwrap();
        assert_eq!(kind(&read(&real).unwrap()), Some("png"));
    }

    #[test]
    fn the_places_come_from_the_workspaces_the_code_chat_and_its_agents() {
        let garden = folder("garden-planner");
        let art = folder("art");
        std::fs::write(art.join("sketch.png"), png(2, 2)).unwrap();
        crate::agents::workspaces::add(&s(&garden), None, None).unwrap();
        crate::agents::store::insert(crate::agents::store::Agent { id: "a1".into(), conversation: Some(42), path: s(&art), files: vec!["sketch.png".into()], ..Default::default() });
        let places = Context { conversation: Some(42), said: Vec::new() }.places();
        assert!(places.allows(&s(&garden.join("docs").join("concept.png"))).is_some());
        assert_eq!(places.allows(&s(&art.join("sketch.png"))), Some(Why::Agent));
        assert_eq!(places.allows(&s(&art.join("other.png"))), None);
        // Another chat's agent's files aren't this one's.
        let other = Context { conversation: Some(7), said: Vec::new() }.places();
        assert_eq!(other.allows(&s(&art.join("sketch.png"))), None);
        // The Code chat's workspace is where a relative path is from.
        crate::agents::store::set_code_chat(42, Some(&s(&garden)));
        assert_eq!(Context { conversation: Some(42), said: Vec::new() }.places().base.as_deref(), Some(s(&garden).as_str()));
    }

    #[test]
    fn a_question_naming_a_picture_file_is_told_from_a_web_pictures_address() {
        for yes in [
            "Can you show me C:\\Projects\\garden-planner\\docs\\concept.png?",
            "show d:/Garden Planner/art/bed layout.JPG please",
            "here: file:///C:/Projects/garden-planner/docs/concept.webp",
            "\\\\nas\\share\\photos\\plot.jpeg",
            "(C:\\a\\b.gif)",
        ] {
            assert!(names_a_picture(yes), "{yes}");
        }
        for no in [
            "Show me https://wiki.example.com/images/map.png",
            "What's a good tomato feed?",
            "Open C:\\Projects\\garden-planner\\docs\\notes.txt",
            "the concept.png the agent made",
            "C:\\Projects\\garden-planner\\docs\\concept.pngx",
            "my profile: see https://example.com/a.png",
            "C:\\Projects\\garden-planner\nconcept.png",
        ] {
            assert!(!names_a_picture(no), "{no}");
        }
    }

    #[test]
    fn a_file_given_to_the_pages_says_how_to_show_a_picture() {
        let picture = not_a_page("file:///D:/Program%20Files/Garden/concept.png").unwrap();
        assert!(picture.contains("show_picture") && picture.contains("full path"), "{picture}");
        assert!(not_a_page("C:\\Projects\\garden-planner\\docs\\concept.png").unwrap().contains("show_picture"));
        let page = not_a_page("file:///C:/Projects/garden-planner/bed-layout.html").unwrap();
        assert!(page.contains("(HTML)") && page.contains("Tell the user"), "{page}");
        assert!(not_a_page("file:///C:/notes.txt").unwrap().contains("A picture from this PC"));
        assert_eq!(not_a_page("https://wiki.example.com/a.png"), None);
        assert_eq!(not_a_page("garden-planner"), None);
        assert_eq!(chip_name("file:///D:/Program%20Files/Garden/concept.png"), "concept.png");
    }
}

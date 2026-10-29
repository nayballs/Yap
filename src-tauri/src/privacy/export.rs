//! **Download your data** (Settings → Privacy & data): one .zip of
//! everything Yap keeps about the person, readable without Yap, saved where
//! they pick (Windows' Save dialog, `diagnostics::pick_zip_path`). Never
//! uploaded.
//!
//! In it ([`text_files`], pure):
//! - `history/history.json` and `history/dictations.csv` — every dictation;
//!   `history/dictation-stats.json` — the stats Home and Insights show
//!   (`dictation/stats.rs`: a row a day, numbers and app names);
//! - `notes/*.md` and `meetings/*.md` — each note and meeting as Markdown
//!   (`notes::commands::export_markdown`, what "Save as .md" writes), and
//!   `notes/notes.json`, the whole store (folders and actions too), and
//!   `notes/versions.json`, each note's earlier versions (`notes/versions.rs`);
//! - `tasks.json`, `skills.json` (the person's own Skills), `chats/chats.json` with each chat as Markdown, and
//!   `memory/` — Memory's own export (`memory::export`) and `memory.json`;
//! - `dictionary/dictionary.csv` — corrections and snippets, the CSV the
//!   Dictionary view's Export… writes ([`dictionary_csv`]);
//! - `settings.json` — config.json **without secrets** ([`settings_export`]):
//!   every field that holds a key, a token or a password is left out by its
//!   name (a list, then a rule a test holds every setting to), a value that
//!   looks like a key is left out whatever its name, and links lose their
//!   query and user name. The account session and the calendars' tokens live
//!   in Windows' Credential Manager, which this never reads.
//! - with the tick box: `audio/meetings/` (each meeting's FLAC),
//!   `audio/dictations/` (the day of dictation audio, and what Home's Keep
//!   holds) and `screenshots/` (Chat's).
//!
//! The zip is written straight to disk ([`write_zip`]: a big one is gigabytes
//! of audio), beside the file picked as `<name>.partial`, renamed when done.
//! The page shows its progress (`yap-data-export`) and has Cancel
//! ([`data_export_cancel`]: the partial file goes).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

/// The page's progress event: `{ done, total }` in bytes.
pub const EVENT: &str = "yap-data-export";

// ---- settings, without secrets ----------------------------------------------------------------

/// Settings that hold a key, wherever they are (config.json's own names:
/// the top level, each Language Models scope, each cleanup profile). Left
/// out whatever they hold.
const SECRET_FIELDS: &[&str] = &["ppApiKey", "ppApiKeys", "apiKey", "apiKeys", "webSearchKeys", "ttsCloudKeys"];

/// Names that end in "key" but are a key on the keyboard, not a secret.
fn keyboard_key(lower: &str) -> bool {
    lower.ends_with("hotkey") || lower.ends_with("hotkeys") || lower == "autosubmitkey"
}

/// Whether a setting's name says it holds a secret: [`SECRET_FIELDS`], or
/// any name with a token, a secret, a password, a session or a credential
/// in it, or ending in "key" / "keys" (bar the keyboard's). A new setting
/// named so is left out before anyone thinks of it.
pub fn secret_name(name: &str) -> bool {
    if SECRET_FIELDS.contains(&name) {
        return true;
    }
    let lower = name.to_ascii_lowercase();
    if ["apikey", "api_key", "token", "secret", "password", "passwd", "session", "credential", "cookie", "bearer"]
        .iter()
        .any(|w| lower.contains(w))
    {
        return true;
    }
    (lower.ends_with("key") || lower.ends_with("keys")) && !keyboard_key(&lower)
}

/// A link without what can carry a secret: its user name and password, its
/// query and its fragment.
fn safe_link(s: &str) -> Option<String> {
    let t = s.trim();
    if !(t.starts_with("http://") || t.starts_with("https://") || t.starts_with("ws://") || t.starts_with("wss://")) {
        return None;
    }
    let mut u = reqwest::Url::parse(t).ok()?;
    if u.query().is_none() && u.fragment().is_none() && u.username().is_empty() && u.password().is_none() {
        return None;
    }
    u.set_query(None);
    u.set_fragment(None);
    let _ = u.set_username("");
    let _ = u.set_password(None);
    Some(u.to_string())
}

fn scrub_value(v: &mut Value, path: &str, left_out: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            let names: Vec<String> = map.keys().cloned().collect();
            for name in names {
                let at = if path.is_empty() { name.clone() } else { format!("{path}.{name}") };
                if secret_name(&name) {
                    map.remove(&name);
                    left_out.push(at);
                    continue;
                }
                if let Some(child) = map.get_mut(&name) {
                    scrub_value(child, &at, left_out);
                }
            }
        }
        Value::Array(items) => {
            for (i, item) in items.iter_mut().enumerate() {
                scrub_value(item, &format!("{path}[{i}]"), left_out);
            }
        }
        Value::String(s) => {
            if crate::privacy::diagnostics::PREFIXED_KEY.is_match(s) {
                *s = "(left out: it looks like a key)".to_string();
                left_out.push(path.to_string());
            } else if let Some(link) = safe_link(s) {
                *s = link;
            }
        }
        _ => {}
    }
}

/// config.json (as `YapConfig` serialises) without its secrets, and the
/// settings left out named under `leftOut` (names only). Pure.
pub fn settings_export(cfg: &Value) -> Value {
    let mut out = cfg.clone();
    let mut left_out = Vec::new();
    scrub_value(&mut out, "", &mut left_out);
    left_out.sort();
    if let Value::Object(map) = &mut out {
        map.insert("leftOut".into(), json!(left_out));
    }
    out
}

// ---- CSV --------------------------------------------------------------------------------------

/// A cell as the Dictionary view writes one (`dictionaryFile.js` `csvCell`):
/// quoted when it has a quote, a separator, a line break or a space at an
/// end.
fn csv_cell(s: &str) -> String {
    let needs = s.contains(['"', ',', ';', '\t', '\r', '\n']) || s.starts_with(char::is_whitespace) || s.ends_with(char::is_whitespace);
    if needs {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// Rows as CSV: Windows line ends and a byte-order mark, so Excel reads it
/// as UTF-8.
fn csv(rows: &[Vec<String>]) -> String {
    let lines: Vec<String> = rows.iter().map(|r| r.iter().map(|c| csv_cell(c)).collect::<Vec<_>>().join(",")).collect();
    format!("\u{feff}{}\r\n", lines.join("\r\n"))
}

/// One line, its runs of space as one (`dictionaryList.js` `oneLine`).
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The dictionary as the Dictionary view's Export… writes it as CSV
/// (`dictionaryFile.js` `toCsv` over `entryRows`): Heard, Written, Kind,
/// Near-misses, Starred; a correction heard several ways is a row per way,
/// together, with the first row's ≈ and star (the view's `groupRows`). Pure.
pub fn dictionary_csv(rows: &[crate::config::DictionaryEntry]) -> String {
    struct Group {
        heard: Vec<String>,
        to: String,
        snippet: bool,
        fuzzy: bool,
        starred: bool,
    }
    let mut groups: Vec<Group> = Vec::new();
    for r in rows {
        let (from, to) = (r.from.trim(), r.to.trim());
        let word = !r.snippet && !from.is_empty() && !to.is_empty();
        if word {
            if let Some(g) = groups.iter_mut().find(|g| !g.snippet && g.to == to) {
                let h = one_line(from);
                if !h.is_empty() && !g.heard.iter().any(|x| x.to_lowercase() == h.to_lowercase()) {
                    g.heard.push(h);
                }
                continue;
            }
        }
        if to.is_empty() {
            // Nothing written: the view leaves it out of an export.
            continue;
        }
        let heard = if r.snippet {
            if from.is_empty() {
                vec![]
            } else {
                vec![from.to_string()]
            }
        } else {
            let h = one_line(from);
            if h.is_empty() {
                vec![]
            } else {
                vec![h]
            }
        };
        groups.push(Group { heard, to: to.to_string(), snippet: r.snippet, fuzzy: r.fuzzy, starred: r.starred });
    }
    let mut out = vec![vec!["Heard".to_string(), "Written".into(), "Kind".into(), "Near-misses".into(), "Starred".into()]];
    for g in &groups {
        let heard = if g.heard.is_empty() { vec![String::new()] } else { g.heard.clone() };
        for h in heard {
            out.push(vec![
                h,
                g.to.clone(),
                if g.snippet { "snippet" } else { "correction" }.into(),
                if g.snippet {
                    String::new()
                } else if g.fuzzy {
                    "yes".into()
                } else {
                    "no".into()
                },
                if g.starred { "yes".into() } else { String::new() },
            ]);
        }
    }
    csv(&out)
}

/// Every dictation as CSV: when (this PC's time), the app, its words, what
/// was pasted, the words as said (when AI cleanup changed them), what the
/// speech model heard, what became of it, and the model. Pure.
pub fn history_csv(entries: &[crate::dictation::history::HistoryEntry], when: &dyn Fn(u64) -> String) -> String {
    let mut rows = vec![vec![
        "When".to_string(),
        "App".into(),
        "Words".into(),
        "Text".into(),
        "As said".into(),
        "Heard".into(),
        "Status".into(),
        "Model".into(),
    ]];
    for e in entries {
        let status = match e.status.as_deref() {
            None => "pasted".to_string(),
            Some(crate::dictation::history::FAILED) => "not transcribed".into(),
            Some(crate::dictation::history::NOT_PASTED) => "not pasted".into(),
            Some(crate::dictation::history::CANCELLED) => "cancelled".into(),
            Some(other) => other.to_string(),
        };
        let status = match e.error.as_deref().filter(|s| !s.trim().is_empty()) {
            Some(why) => format!("{status}: {why}"),
            None => status,
        };
        rows.push(vec![
            when(e.ts),
            e.app.clone(),
            e.text.split_whitespace().count().to_string(),
            e.text.clone(),
            e.said.clone().unwrap_or_default(),
            e.raw.clone(),
            status,
            e.model.clone(),
        ]);
    }
    csv(&rows)
}

// ---- Markdown -----------------------------------------------------------------------------------

/// A file name from a title: what Windows won't take turned into spaces,
/// no dots or spaces at the ends, at most 80 characters.
pub fn file_stem(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| if c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') { ' ' } else { c })
        .collect();
    let mut s = one_line(&cleaned);
    if s.chars().count() > 80 {
        s = s.chars().take(80).collect::<String>();
    }
    let s = s.trim_matches(|c: char| c == '.' || c.is_whitespace()).to_string();
    if s.is_empty() {
        "Untitled".into()
    } else {
        s
    }
}

/// `folder/stem.ext`, made unique among `taken` (" (2)", " (3)"…).
fn unique(taken: &mut std::collections::HashSet<String>, folder: &str, stem: &str, ext: &str) -> String {
    let mut n = 1;
    loop {
        let name = if n == 1 { format!("{folder}/{stem}.{ext}") } else { format!("{folder}/{stem} ({n}).{ext}") };
        if taken.insert(name.to_lowercase()) {
            return name;
        }
        n += 1;
    }
}

/// A chat as Markdown: its title, then each message with who and when.
pub fn chat_markdown(c: &crate::chat::chats::Conversation, when: &dyn Fn(u64) -> String) -> String {
    let title = if c.title.trim().is_empty() { "Untitled chat" } else { c.title.trim() };
    let mut out = format!("# {title}\n\n_Started {}_\n", when(c.created_ts));
    for m in &c.messages {
        let who = match m.role.as_str() {
            "user" => "You",
            "assistant" => "Yap",
            other => other,
        };
        let model = m.model.as_ref().filter(|x| !x.id.trim().is_empty()).map(|x| format!(" · {}", x.id)).unwrap_or_default();
        out.push_str(&format!("\n**{who}** · {}{model}\n\n{}\n", when(m.ts), m.text.trim()));
    }
    out
}

// ---- the file -----------------------------------------------------------------------------------

/// What the file is made from: each store as it is in memory now (what the
/// page shows, saved or not).
pub struct Inputs {
    /// When it was made ("2026-10-29 14:05", this PC's time).
    pub made: String,
    pub history: Vec<crate::dictation::history::HistoryEntry>,
    /// dictation_stats.json as it is (`stats::snapshot`).
    pub stats: Value,
    pub notes: Vec<crate::notes::Note>,
    /// notes.json as it is (folders and actions too).
    pub notes_json: String,
    /// Each note's version history (`notes::versions::export_all`).
    pub note_versions: Vec<Value>,
    pub chats: Vec<crate::chat::chats::Conversation>,
    pub tasks: Value,
    /// skills.json: the person's own Skills (chat/skills.rs).
    pub skills: Value,
    pub memory: Vec<crate::memory::ops::Item>,
    /// About you's lines for Memory's You.md.
    pub about: Vec<(&'static str, String)>,
    pub dictionary: Vec<crate::config::DictionaryEntry>,
    /// config.json as `YapConfig` serialises.
    pub config: Value,
    /// Whether the audio is in it (the README says).
    pub audio: bool,
}

const README: &str = "Your data from Yap, made on {made}. You saved this file yourself: Yap never uploaded it.\n\n\
What's in it\n\
- history/: every dictation Yap kept. dictations.csv opens in Excel; history.json is the same, in full. \
dictation-stats.json is your stats in Home and Insights: a row a day, numbers and the apps you dictated into, \
never what you said.\n\
- notes/ and meetings/: each note and meeting as Markdown (its summary, your notes and the transcript). \
notes/notes.json is all of them as Yap keeps them, with your folders and actions; \
notes/versions.json, each note's earlier versions (Version history), newest first.\n\
- chats/: each chat as Markdown, and chats.json.\n\
- tasks.json: the tasks you added and the ones you archived.\n\
- skills.json: the Skills you made (your own instructions, run with /name).\n\
- memory/: what Yap remembers about you, as Markdown, and memory.json.\n\
- dictionary/dictionary.csv: your corrections and snippets, as the Dictionary's Export writes them \
(Import reads it back).\n\
- settings.json: your settings. Never in it: API keys, tokens, passwords and your account's sign-in. \
`leftOut` names the settings taken out.\n\
{audio}\n\
Not in it: the speech and AI models, Yap's log, its caches and anything kept by Windows' Credential Manager \
(your account's sign-in, your calendars' tokens).\n";

/// The file's text parts, in order: (name in the zip, bytes). Pure.
pub fn text_files(i: &Inputs, when: &dyn Fn(u64) -> String) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    let audio = if i.audio {
        "- audio/: each meeting's audio (FLAC), the last day of dictation audio and what you kept (WAV); \
screenshots/: Chat's screenshots (PNG).\n"
    } else {
        "- No audio or screenshots: you left them out.\n"
    };
    let readme = README.replace("{made}", &i.made).replace("{audio}\n", audio);
    files.push(("README.txt".into(), readme.into_bytes()));
    let pretty = |v: &dyn erased::Json| v.pretty().into_bytes();

    // History.
    files.push(("history/dictations.csv".into(), history_csv(&i.history, when).into_bytes()));
    files.push(("history/history.json".into(), pretty(&i.history)));
    files.push(("history/dictation-stats.json".into(), pretty(&i.stats)));

    // Notes and meetings.
    let mut taken = std::collections::HashSet::new();
    let mut notes: Vec<&crate::notes::Note> = i.notes.iter().collect();
    notes.sort_by_key(|n| (n.created_ts, n.id));
    for n in notes {
        let folder = if crate::integrations::mcp::is_meeting(n) { "meetings" } else { "notes" };
        let day: String = when(n.created_ts).chars().take(10).collect();
        let stem = file_stem(&format!("{day} {}", if n.title.trim().is_empty() { "Untitled" } else { n.title.trim() }));
        let name = unique(&mut taken, folder, &stem, "md");
        files.push((name, crate::notes::commands::export_markdown(n).into_bytes()));
    }
    files.push(("notes/notes.json".into(), i.notes_json.clone().into_bytes()));
    files.push(("notes/versions.json".into(), pretty(&i.note_versions)));

    // Chats.
    let mut chats: Vec<&crate::chat::chats::Conversation> = i.chats.iter().collect();
    chats.sort_by_key(|c| (c.created_ts, c.id));
    for c in &chats {
        let day: String = when(c.created_ts).chars().take(10).collect();
        let stem = file_stem(&format!("{day} {}", if c.title.trim().is_empty() { "Untitled chat" } else { c.title.trim() }));
        let name = unique(&mut taken, "chats", &stem, "md");
        files.push((name, chat_markdown(c, when).into_bytes()));
    }
    files.push(("chats/chats.json".into(), pretty(&json!({ "conversations": i.chats }))));

    // Tasks, your Skills, memory.
    files.push(("tasks.json".into(), pretty(&i.tasks)));
    files.push(("skills.json".into(), pretty(&i.skills)));
    for (name, text) in crate::memory::export::files(&i.memory, &i.about) {
        files.push((format!("memory/{name}"), text.into_bytes()));
    }
    files.push(("memory/memory.json".into(), pretty(&json!({ "items": i.memory }))));

    // Dictionary, settings.
    files.push(("dictionary/dictionary.csv".into(), dictionary_csv(&i.dictionary).into_bytes()));
    files.push(("settings.json".into(), pretty(&settings_export(&i.config))));
    files
}

/// Pretty JSON of anything serialisable, for [`text_files`].
mod erased {
    pub trait Json {
        fn pretty(&self) -> String;
    }
    impl<T: serde::Serialize> Json for T {
        fn pretty(&self) -> String {
            serde_json::to_string_pretty(self).unwrap_or_default()
        }
    }
}

/// A file on disk that goes in as it is (audio, a screenshot).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskFile {
    /// Its name in the zip.
    pub name: String,
    pub path: PathBuf,
    pub bytes: u64,
}

/// Files in `dir` (not below it) whose extension is `ext`, with sizes.
fn files_in(dir: &Path, ext: &str) -> Vec<(PathBuf, u64)> {
    let mut out: Vec<(PathBuf, u64)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            let m = e.metadata().ok().filter(|m| m.is_file())?;
            p.extension().and_then(|x| x.to_str()).filter(|x| x.eq_ignore_ascii_case(ext))?;
            Some((p, m.len()))
        })
        .collect();
    out.sort();
    out
}

/// The audio and screenshots Yap keeps, as they'd go in: each meeting's
/// FLAC under its note's name, the dictation audio by when it was said,
/// Chat's screenshots by their id. `notes`: (id, file stem) to name the
/// meetings' folders.
pub fn disk_files(data: &Path, notes: &[(u64, String)], when: &dyn Fn(u64) -> String) -> Vec<DiskFile> {
    let mut out = Vec::new();
    // Meetings: meeting-audio/<note id>/*.flac.
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(data.join("meeting-audio"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    for d in dirs {
        let id = d.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        let folder = id
            .parse::<u64>()
            .ok()
            .and_then(|n| notes.iter().find(|(nid, _)| *nid == n).map(|(_, stem)| stem.clone()))
            .unwrap_or_else(|| file_stem(&id));
        for (p, bytes) in files_in(&d, "flac") {
            let file = p.file_name().and_then(|n| n.to_str()).unwrap_or("audio.flac").to_string();
            out.push(DiskFile { name: format!("audio/meetings/{folder}/{file}"), path: p, bytes });
        }
    }
    // Dictations: recent-audio/<unix ms>.wav, and kept/.
    let recent = data.join("recent-audio");
    let mut taken = std::collections::HashSet::new();
    for dir in [recent.clone(), recent.join("kept")] {
        for (p, bytes) in files_in(&dir, "wav") {
            let stem = p.file_stem().and_then(|n| n.to_str()).unwrap_or("dictation").to_string();
            let label = stem.parse::<u64>().map(|ms| when(ms / 1000).replace(':', "-")).unwrap_or_else(|_| file_stem(&stem));
            let name = unique(&mut taken, "audio/dictations", &file_stem(&label), "wav");
            out.push(DiskFile { name, path: p, bytes });
        }
    }
    // Chat's screenshots.
    for (p, bytes) in files_in(&data.join("chat-images"), "png") {
        let file = p.file_name().and_then(|n| n.to_str()).unwrap_or("screenshot.png").to_string();
        out.push(DiskFile { name: format!("screenshots/{file}"), path: p, bytes });
    }
    out
}

/// Write the zip to `to`: the text parts compressed, the files from disk
/// stored as they are (they're compressed already). `progress(done,
/// total)` in bytes now and then; stops, returning `Ok(false)`, once
/// `cancel` is set (the caller deletes the file).
pub fn write_zip(
    to: &Path,
    text: &[(String, Vec<u8>)],
    disk: &[DiskFile],
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<bool, String> {
    let total: u64 = text.iter().map(|(_, b)| b.len() as u64).sum::<u64>() + disk.iter().map(|f| f.bytes).sum::<u64>();
    let file = std::fs::File::create(to).map_err(|e| format!("Couldn't write the file: {e}"))?;
    let mut z = zip::ZipWriter::new(std::io::BufWriter::with_capacity(1 << 20, file));
    let packed = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let err = |e: &dyn std::fmt::Display| format!("Couldn't write the file: {e}");
    let mut done = 0u64;
    let mut told = Instant::now();
    progress(0, total);
    for (name, bytes) in text {
        if cancel.load(Ordering::SeqCst) {
            return Ok(false);
        }
        z.start_file(name.as_str(), packed).map_err(|e| err(&e))?;
        z.write_all(bytes).map_err(|e| err(&e))?;
        done += bytes.len() as u64;
    }
    progress(done, total);
    let mut buf = vec![0u8; 1 << 20];
    for f in disk {
        // A file gone since it was listed (a day's audio swept) is left out.
        let Ok(mut src) = std::fs::File::open(&f.path) else { continue };
        let opts = stored.large_file(f.bytes >= u32::MAX as u64);
        z.start_file(f.name.as_str(), opts).map_err(|e| err(&e))?;
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Ok(false);
            }
            let n = src.read(&mut buf).map_err(|e| format!("Couldn't read {}: {e}", f.name))?;
            if n == 0 {
                break;
            }
            z.write_all(&buf[..n]).map_err(|e| err(&e))?;
            done += n as u64;
            // A test run can slow it down, to press Cancel in time.
            #[cfg(debug_assertions)]
            {
                let pace = E2E_PACE_MS.load(Ordering::SeqCst);
                if pace > 0 {
                    std::thread::sleep(Duration::from_millis(pace));
                }
            }
            if told.elapsed() >= Duration::from_millis(200) {
                told = Instant::now();
                progress(done.min(total), total);
            }
        }
    }
    let mut out = z.finish().map_err(|e| err(&e))?;
    out.flush().map_err(|e| err(&e))?;
    progress(total, total);
    Ok(true)
}

// ---- what's there ------------------------------------------------------------------------------

/// Privacy & data's row: what the file would hold, and the audio's size
/// (the tick box says it).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub dictations: usize,
    pub notes: usize,
    pub meetings: usize,
    pub chats: usize,
    pub tasks: usize,
    pub memory: usize,
    pub dictionary: usize,
    pub audio_files: usize,
    pub audio_bytes: u64,
}

fn local_when(secs: u64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_opt(secs as i64, 0)
        .single()
        .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| secs.to_string())
}

fn note_stems(notes: &[crate::notes::Note]) -> Vec<(u64, String)> {
    notes
        .iter()
        .map(|n| {
            let day: String = local_when(n.created_ts).chars().take(10).collect();
            (n.id, file_stem(&format!("{day} {}", if n.title.trim().is_empty() { "Untitled" } else { n.title.trim() })))
        })
        .collect()
}

fn inputs(audio: bool) -> Inputs {
    let cfg = crate::config::load();
    let p = crate::ai::about_you::profile(&cfg);
    let about = vec![
        ("Name", p.name),
        ("Region", p.region.country_name.unwrap_or_default()),
        ("What you told Yap", if p.about.is_empty() { String::new() } else { format!("“{}”", p.about) }),
    ];
    Inputs {
        made: chrono::Local::now().format("%Y-%m-%d %H:%M").to_string(),
        history: crate::dictation::history::all(),
        stats: crate::dictation::stats::snapshot(),
        notes: crate::notes::all(),
        notes_json: crate::notes::store_json(),
        note_versions: crate::notes::versions::export_all(),
        chats: crate::chat::chats::with_all(|c| c.to_vec()),
        tasks: serde_json::to_value(crate::notes::task_store::snapshot()).unwrap_or(Value::Null),
        skills: serde_json::to_value(crate::chat::skills::own()).unwrap_or(Value::Null),
        memory: crate::memory::items(),
        about,
        dictionary: cfg.dictionary.clone(),
        config: serde_json::to_value(&cfg).unwrap_or(Value::Null),
        audio,
    }
}

/// What Download your data would hold now.
#[tauri::command]
pub async fn data_export_summary() -> Summary {
    tauri::async_runtime::spawn_blocking(|| {
        let notes = crate::notes::all();
        let meetings = notes.iter().filter(|n| crate::integrations::mcp::is_meeting(n)).count();
        let disk = disk_files(&crate::config::data_dir(), &[], &local_when);
        let tasks = crate::notes::task_store::snapshot();
        Summary {
            dictations: crate::dictation::history::all().len(),
            notes: notes.len() - meetings,
            meetings,
            chats: crate::chat::chats::with_all(|c| c.len()),
            tasks: tasks.tasks.len(),
            memory: crate::memory::items().len(),
            dictionary: crate::config::load().dictionary.len(),
            audio_files: disk.len(),
            audio_bytes: disk.iter().map(|f| f.bytes).sum(),
        }
    })
    .await
    .unwrap_or_default()
}

/// One export at a time; Cancel sets [`CANCEL`].
static RUNNING: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);
/// Where it was saved last (Show in folder).
static SAVED: Mutex<Option<PathBuf>> = Mutex::new(None);

/// How a save ended: `path` when written, `cancelled` when Cancel stopped it.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Saved {
    pub path: Option<String>,
    pub cancelled: bool,
    pub bytes: u64,
}

/// Download your data: Windows' Save dialog, then the file written there
/// with its progress (`yap-data-export`); `audio`: with the audio and
/// screenshots. Nothing (`path` none) when the dialog was cancelled.
#[tauri::command]
pub async fn data_export_save(app: AppHandle, window: tauri::WebviewWindow, audio: bool) -> Result<Saved, String> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err("Your data is being saved already.".into());
    }
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            RUNNING.store(false, Ordering::SeqCst);
        }
    }
    let _done = Done;
    CANCEL.store(false, Ordering::SeqCst);
    let name = format!("Yap data {}.zip", chrono::Local::now().format("%Y-%m-%d"));
    let Some(path) = crate::privacy::diagnostics::pick_zip_path(&app, window, &name, "Save your data").await? else {
        return Ok(Saved::default());
    };
    let to = path.clone();
    let emitter = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<Saved, String> {
        let started = Instant::now();
        let inputs = inputs(audio);
        let text = text_files(&inputs, &local_when);
        let disk = if audio { disk_files(&crate::config::data_dir(), &note_stems(&inputs.notes), &local_when) } else { Vec::new() };
        drop(inputs);
        let mut partial = to.clone().into_os_string();
        partial.push(".partial");
        let partial = PathBuf::from(partial);
        let mut progress = |done: u64, total: u64| {
            let _ = emitter.emit(EVENT, json!({ "done": done, "total": total }));
        };
        let finished = write_zip(&partial, &text, &disk, &CANCEL, &mut progress);
        match finished {
            Ok(true) => {
                std::fs::rename(&partial, &to).map_err(|e| {
                    let _ = std::fs::remove_file(&partial);
                    format!("Couldn't save the file: {e}")
                })?;
                let bytes = std::fs::metadata(&to).map(|m| m.len()).unwrap_or(0);
                tracing::info!(files = text.len() + disk.len(), audio_files = disk.len(), bytes, ms = started.elapsed().as_millis() as u64, "data export: saved");
                Ok(Saved { path: Some(to.to_string_lossy().into_owned()), cancelled: false, bytes })
            }
            Ok(false) => {
                let _ = std::fs::remove_file(&partial);
                tracing::info!("data export: cancelled");
                Ok(Saved { path: None, cancelled: true, bytes: 0 })
            }
            Err(e) => {
                let _ = std::fs::remove_file(&partial);
                tracing::warn!("data export: couldn't write the file");
                Err(e)
            }
        }
    })
    .await
    .map_err(|e| e.to_string())??;
    if result.path.is_some() {
        *SAVED.lock().unwrap_or_else(|p| p.into_inner()) = Some(path);
    }
    Ok(result)
}

/// Test mode: a pause after each MB of audio written (0: none).
#[cfg(debug_assertions)]
static E2E_PACE_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Test mode only: write the audio slowly (`ms` after each MB), so Cancel
/// can be pressed while it's under way.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_export_pace(ms: u64) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    E2E_PACE_MS.store(ms.min(5_000), Ordering::SeqCst);
    Ok(())
}

/// Cancel: the file being written stops and goes.
#[tauri::command]
pub fn data_export_cancel() {
    if RUNNING.load(Ordering::SeqCst) {
        CANCEL.store(true, Ordering::SeqCst);
    }
}

/// Show the saved file in File Explorer.
#[tauri::command]
pub fn data_export_reveal(app: AppHandle) -> Result<(), String> {
    let Some(path) = SAVED.lock().unwrap_or_else(|p| p.into_inner()).clone() else { return Ok(()) };
    if crate::shell::e2e::active() {
        return Ok(());
    }
    use tauri_plugin_opener::OpenerExt;
    app.opener().reveal_item_in_dir(&path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DictionaryEntry;

    // Fictional people and made-up secrets only.
    const SECRETS: &[&str] = &[
        "gsk_FAKEexport1234567890abcdefGHIJ",
        "sk-proj-FAKEexportOPENAI0123456789",
        "exa-fake-export-0000-1111-2222",
        "tvly-FAKEexport0123456789",
        "sk_fake_eleven_export_99887766",
        "sk-ant-FAKEscopeKEY0123456789",
        "FAKEprofileKEY-0123456789-abcdef",
        "querykey-0123456789",
        "hunter2-password",
    ];

    fn row(from: &str, to: &str) -> DictionaryEntry {
        entry(json!({ "from": from, "to": to }))
    }

    fn entry(v: Value) -> DictionaryEntry {
        serde_json::from_value(v).unwrap()
    }

    fn seeded_config() -> crate::config::YapConfig {
        let mut cfg = crate::config::YapConfig {
            pp_provider: "groq".into(),
            pp_api_key: SECRETS[0].into(),
            pp_base_url: "https://robin:hunter2-password@llm.example.com/v1?key=querykey-0123456789".into(),
            about_name: "Robin Example".into(),
            ..Default::default()
        };
        cfg.pp_api_keys.insert("groq".into(), SECRETS[0].into());
        cfg.pp_api_keys.insert("openai".into(), SECRETS[1].into());
        cfg.web_search_keys.insert("exa".into(), SECRETS[2].into());
        cfg.web_search_keys.insert("tavily".into(), SECRETS[3].into());
        cfg.tts_cloud_keys.insert("elevenlabs".into(), SECRETS[4].into());
        let mut scope = crate::config::LlmScope { enabled: true, provider: "anthropic".into(), api_key: SECRETS[5].into(), ..Default::default() };
        scope.api_keys.insert("anthropic".into(), SECRETS[5].into());
        cfg.llm_scopes.insert("chat".into(), scope);
        cfg.cleanup_profiles.push(crate::config::CleanupProfile {
            id: "p1".into(),
            name: "Email".into(),
            provider: "custom".into(),
            api_key: SECRETS[6].into(),
            ..Default::default()
        });
        // A key pasted into a field that isn't for keys is caught by its look.
        cfg.agent_name = SECRETS[1].into();
        cfg.dictionary = vec![row("cooper netties", "Kubernetes"), row("cube an eighties", "Kubernetes")];
        cfg
    }

    fn inputs_with(cfg: &crate::config::YapConfig) -> Inputs {
        let note: crate::notes::Note = serde_json::from_value(json!({
            "id": 7, "title": "Allotment: plans?", "content": "Leeks by the shed", "noteType": "personal",
            "createdTs": 1_790_000_000u64, "updatedTs": 1_790_000_000u64
        }))
        .unwrap();
        let meeting: crate::notes::Note = serde_json::from_value(json!({
            "id": 8, "title": "Weekly sync", "noteType": "meeting", "enhancedContent": "## Summary\n- Ship it",
            "createdTs": 1_790_000_100u64, "updatedTs": 1_790_000_100u64
        }))
        .unwrap();
        Inputs {
            made: "2026-10-29 14:05".into(),
            history: vec![crate::dictation::history::HistoryEntry {
                ts: 1_790_000_000,
                raw: "hello world".into(),
                text: "Hello, world.".into(),
                said: None,
                model: "parakeet".into(),
                app: "notepad.exe".into(),
                audio: None,
                status: None,
                error: None,
                secs: Some(1.2),
            }],
            stats: json!({ "days": [{ "date": "2026-10-29", "words": 2, "dictations": 1 }] }),
            notes: vec![note.clone(), meeting],
            notes_json: "{\"notes\":[]}".into(),
            note_versions: vec![json!({ "note": 7, "title": "Allotment: plans?", "versions": [{ "n": 1, "kind": "typed", "text": "Leeks" }] })],
            chats: vec![],
            tasks: json!({ "tasks": [], "archived": [] }),
            skills: json!({ "skills": [{ "id": "s1", "name": "Budget numbers", "command": "budget-numbers", "instructions": "List every amount said.", "runsOn": "meeting" }] }),
            memory: vec![],
            about: vec![("Name", "Robin Example".into())],
            dictionary: cfg.dictionary.clone(),
            config: serde_json::to_value(cfg).unwrap(),
            audio: false,
        }
    }

    fn when(secs: u64) -> String {
        format!("2026-10-{:02} 09:00:00", 1 + secs % 28)
    }

    #[test]
    fn the_file_holds_every_store_and_no_secret() {
        let cfg = seeded_config();
        let files = text_files(&inputs_with(&cfg), &when);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        for want in [
            "README.txt",
            "history/dictations.csv",
            "history/history.json",
            "history/dictation-stats.json",
            "notes/notes.json",
            "notes/versions.json",
            "chats/chats.json",
            "tasks.json",
            "skills.json",
            "memory/You.md",
            "memory/memory.json",
            "dictionary/dictionary.csv",
            "settings.json",
        ] {
            assert!(names.contains(&want), "{want} in {names:?}");
        }
        assert!(names.iter().any(|n| n.starts_with("notes/") && n.ends_with("Allotment plans.md")), "{names:?}");
        assert!(names.iter().any(|n| n.starts_with("meetings/") && n.ends_with("Weekly sync.md")), "{names:?}");
        for (name, bytes) in &files {
            let text = String::from_utf8_lossy(bytes);
            for s in SECRETS {
                assert!(!text.contains(s), "{s} in {name}");
            }
        }
        // The settings say what was taken out, by name only.
        let settings: Value = serde_json::from_slice(&files.iter().find(|(n, _)| n == "settings.json").unwrap().1).unwrap();
        let left: Vec<&str> = settings["leftOut"].as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
        for want in ["ppApiKey", "ppApiKeys", "webSearchKeys", "ttsCloudKeys", "llmScopes.chat.apiKey", "llmScopes.chat.apiKeys", "agentName"] {
            assert!(left.contains(&want), "{want} in {left:?}");
        }
        assert!(left.iter().any(|l| l.starts_with("cleanupProfiles[") && l.ends_with(".apiKey")), "{left:?}");
        // Everything else is there: the person's own settings.
        assert_eq!(settings["aboutName"], "Robin Example");
        assert_eq!(settings["ppProvider"], "groq");
        assert_eq!(settings["ppBaseUrl"], "https://llm.example.com/v1");
        assert!(settings["hotkey"].is_string() && settings["autoSubmitKey"].is_string());
    }

    /// Every setting whose name has "key", "token", "secret" or "password" in
    /// it is either left out or a key on the keyboard: a new one has to be
    /// decided here before it can reach the file.
    #[test]
    fn every_setting_named_like_a_secret_is_left_out() {
        let mut cfg = seeded_config();
        cfg.app_routes.push(serde_json::from_value(json!({ "app": "slack.exe" })).unwrap());
        let mut names = Vec::new();
        fn walk(v: &Value, names: &mut Vec<String>) {
            match v {
                Value::Object(m) => {
                    for (k, child) in m {
                        names.push(k.clone());
                        walk(child, names);
                    }
                }
                Value::Array(a) => a.iter().for_each(|c| walk(c, names)),
                _ => {}
            }
        }
        walk(&serde_json::to_value(&cfg).unwrap(), &mut names);
        let mut checked = 0;
        for n in &names {
            let lower = n.to_ascii_lowercase();
            if ["key", "token", "secret", "password", "session"].iter().any(|w| lower.contains(w)) {
                checked += 1;
                assert!(secret_name(n) || keyboard_key(&lower), "{n}: a secret or a keyboard key? Decide in export.rs");
            }
        }
        assert!(checked >= 8, "{checked} names checked");
        // The keyboard's keys stay.
        for n in ["hotkey", "editHotkey", "moreHotkeys", "autoSubmitKey", "quickNoteHotkey"] {
            assert!(!secret_name(n), "{n}");
        }
        for n in ["apiKey", "ppApiKeys", "githubToken", "sessionId", "password", "someNewKey"] {
            assert!(secret_name(n), "{n}");
        }
    }

    #[test]
    fn the_dictionary_csv_is_the_dictionary_views() {
        let rows = vec![
            row("cooper netties", "Kubernetes"),
            entry(json!({ "from": "jason", "to": "JSON", "fuzzy": false, "starred": true })),
            row("cube an eighties", "Kubernetes"),
            row("Cooper  Netties", "Kubernetes"),
            entry(json!({ "from": "my address", "to": "1 Example Lane,\nExampleton", "snippet": true })),
            row("", ""),
        ];
        let csv = dictionary_csv(&rows);
        assert!(csv.starts_with('\u{feff}'));
        let lines: Vec<&str> = csv.trim_start_matches('\u{feff}').split("\r\n").collect();
        assert_eq!(lines[0], "Heard,Written,Kind,Near-misses,Starred");
        assert_eq!(lines[1], "cooper netties,Kubernetes,correction,yes,");
        // Another way it's heard: beside it, once (any case).
        assert_eq!(lines[2], "cube an eighties,Kubernetes,correction,yes,");
        assert_eq!(lines[3], "jason,JSON,correction,no,yes");
        // A snippet's text keeps its line break, inside quotes.
        assert_eq!(lines[4], "my address,\"1 Example Lane,\nExampleton\",snippet,,");
        assert_eq!(lines.len(), 6, "{lines:?}");
    }

    #[test]
    fn the_history_csv_has_a_row_a_dictation() {
        let e = crate::dictation::history::HistoryEntry {
            ts: 1,
            raw: "um so, hello".into(),
            text: "So, hello.".into(),
            said: Some("um so hello".into()),
            model: "parakeet".into(),
            app: "chrome.exe".into(),
            audio: None,
            status: Some(crate::dictation::history::NOT_PASTED.into()),
            error: Some("The window was gone".into()),
            secs: None,
        };
        let csv = history_csv(&[e], &when);
        let lines: Vec<&str> = csv.trim_start_matches('\u{feff}').split("\r\n").collect();
        assert_eq!(lines[0], "When,App,Words,Text,As said,Heard,Status,Model");
        assert_eq!(lines[1], "2026-10-02 09:00:00,chrome.exe,2,\"So, hello.\",um so hello,\"um so, hello\",not pasted: The window was gone,parakeet");
    }

    #[test]
    fn file_names_are_safe_and_unique() {
        assert_eq!(file_stem("Plans: Q4 / Q1?"), "Plans Q4 Q1");
        assert_eq!(file_stem("..."), "Untitled");
        assert_eq!(file_stem(&"a".repeat(200)).len(), 80);
        let mut taken = std::collections::HashSet::new();
        assert_eq!(unique(&mut taken, "notes", "Same", "md"), "notes/Same.md");
        assert_eq!(unique(&mut taken, "notes", "same", "md"), "notes/same (2).md");
    }

    #[test]
    fn the_zip_is_written_with_its_audio_and_cancel_stops_it() {
        let dir = std::env::temp_dir().join(format!("yap-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let data = dir.join("data");
        std::fs::create_dir_all(data.join("meeting-audio").join("8")).unwrap();
        std::fs::create_dir_all(data.join("recent-audio").join("kept")).unwrap();
        std::fs::create_dir_all(data.join("chat-images")).unwrap();
        std::fs::write(data.join("meeting-audio/8/1-you.flac"), vec![1u8; 3000]).unwrap();
        std::fs::write(data.join("meeting-audio/8/1.json"), b"{}").unwrap();
        std::fs::write(data.join("recent-audio/1790000000000.wav"), vec![2u8; 2000]).unwrap();
        std::fs::write(data.join("recent-audio/kept/1790000001000.wav"), vec![3u8; 1000]).unwrap();
        std::fs::write(data.join("chat-images/img_0123456789abcdef.png"), vec![4u8; 500]).unwrap();
        let disk = disk_files(&data, &[(8, "2026-10-29 Weekly sync".into())], &when);
        let names: Vec<&str> = disk.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names.len(), 4, "{names:?}");
        assert!(names.contains(&"audio/meetings/2026-10-29 Weekly sync/1-you.flac"));
        assert!(names.contains(&"screenshots/img_0123456789abcdef.png"));
        assert_eq!(names.iter().filter(|n| n.starts_with("audio/dictations/")).count(), 2);

        let text = vec![("README.txt".to_string(), b"hi".to_vec())];
        let out = dir.join("out.zip");
        let mut seen = Vec::new();
        let ok = write_zip(&out, &text, &disk, &AtomicBool::new(false), &mut |d, t| seen.push((d, t))).unwrap();
        assert!(ok);
        assert_eq!(seen.last(), Some(&(6502, 6502)));
        let mut z = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        assert_eq!(z.len(), 5);
        let mut flac = Vec::new();
        z.by_name("audio/meetings/2026-10-29 Weekly sync/1-you.flac").unwrap().read_to_end(&mut flac).unwrap();
        assert_eq!(flac, vec![1u8; 3000]);

        // Cancelled: it stops (the caller deletes the file).
        let ok = write_zip(&dir.join("cancelled.zip"), &text, &disk, &AtomicBool::new(true), &mut |_, _| {}).unwrap();
        assert!(!ok);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

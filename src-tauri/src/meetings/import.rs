//! **Import a transcript…** (Upload, beside an audio file's import, and the
//! Meetings view's ⋯): a meeting another app transcribed becomes a Yap
//! meeting note, as a recorded one would be — its lines with the speakers'
//! names and times, its date, a title (the AI's, from the start of the
//! meeting, unless the file names the meeting), a folder (Meetings, or a
//! recurring series' folder rule), then the usual action-plan job and its
//! summary in the meeting's language (`meeting_end::start_summary`).
//!
//! **Formats** (each its own reader): WebVTT from Microsoft Teams (`<v
//! Speaker>` voice tags), Zoom and Google Meet ("Name: text" cues) and
//! plain captions; SubRip; Teams' `.docx` download (read as a zip of XML,
//! no Office); Otter's and Zoom's plain-text exports and Google Meet's
//! transcript saved as text; Granola's Markdown or text (its notes become
//! My thoughts) — `import/cues.rs`, `text.rs`, `docx.rs`; what they share,
//! and which reads a file, in `parse.rs`; the date in `dates.rs`; the
//! note's lines and voices in `build.rs`.
//!
//! **The flow**: files are picked (Windows' dialog, opened here) or dropped
//! (`transcript_import_add`: Tauri's drop gives their paths); each is read
//! and **staged** here, and the page gets a **dry preview** (format,
//! speakers found, lines, length, the date and where it came from, which
//! speaker is you) — or why it can't be read. `transcript_import_run`
//! imports the staged files the page names, with the person's date and
//! choice of "you", never reading a file the preview didn't.
//!
//! **Marked imported**: `Note::source` "import" (a field every Yap since
//! 2026-10 keeps), so the windows offer no ▶ on a line, no Transcribe again
//! and no Find speakers — there's no audio — and `meeting_audio` refuses
//! them. Nothing new is kept only in notes.json fields an older Yap would
//! drop. Logs: counts and formats, never a file's name or a word of it.

pub mod build;
pub mod cues;
pub mod dates;
pub mod docx;
pub mod parse;
pub mod text;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, UNIX_EPOCH};

use chrono::{NaiveDateTime, NaiveTime, TimeZone};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, WebviewWindow};

use parse::{Format, Parsed};

/// The most files waiting in the preview at once.
const MAX_STAGED: usize = 50;
/// How long the summaries of one import wait on each other at most.
const SUMMARY_WAIT: Duration = Duration::from_secs(30 * 60);

/// A file read for the preview, waiting to be imported.
struct Staged {
    id: u64,
    name: String,
    read: Result<Read, String>,
}

struct Read {
    parsed: Parsed,
    start: Option<i64>,
    start_from: Option<&'static str>,
    time_known: bool,
}

static STAGED: Mutex<Vec<Staged>> = Mutex::new(Vec::new());
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn staged() -> std::sync::MutexGuard<'static, Vec<Staged>> {
    STAGED.lock().unwrap_or_else(|p| p.into_inner())
}

/// A local date and time as unix seconds, in this PC's time zone.
fn local(at: NaiveDateTime) -> Option<i64> {
    chrono::Local.from_local_datetime(&at).earliest().map(|d| d.timestamp())
}

/// When the meeting began (unix seconds), where that came from, and
/// whether its time of day is known: the transcript's own header, the
/// file's name, its folder's name (Zoom's "2026-10-05 14.30.12 Weekly
/// sync"), else the document's or the file's own date less the meeting's
/// length (a transcript is saved as its meeting ends), for the person to
/// check. A date with no time is at midday, or at Zoom's first clock time.
pub fn start_of(
    p: &Parsed,
    name: &str,
    folder: Option<&str>,
    modified: Option<i64>,
    local: &dyn Fn(NaiveDateTime) -> Option<i64>,
) -> (Option<i64>, Option<&'static str>, bool) {
    let noon = NaiveTime::from_hms_opt(12, 0, 0).unwrap_or(NaiveTime::MIN);
    let said = [(p.dated, "the transcript"), (dates::in_name(name), "the file's name"), (folder.and_then(dates::in_name), "its folder's name")];
    for (dated, from) in said {
        if let Some(mut d) = dated {
            if d.time.is_none() {
                d.time = p.clock;
            }
            if let Some(at) = d.unix(noon, local) {
                return (Some(at), Some(from), d.time.is_some());
            }
        }
    }
    let length = (build::length_ms(p) / 1000) as i64;
    for (saved, from) in [(p.made, "the document's date"), (modified, "the file's date")] {
        if let Some(saved) = saved.filter(|&s| s > length) {
            return (Some(saved - length), Some(from), false);
        }
    }
    (None, None, false)
}

/// Read file `path` for the preview.
fn read_file(path: &Path) -> Staged {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let read = read_path(path, &name);
    Staged { id: NEXT_ID.fetch_add(1, Ordering::SeqCst), name, read }
}

fn read_path(path: &Path, name: &str) -> Result<Read, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("Couldn't open it: {}", plain_io(&e)))?;
    if meta.is_dir() {
        return Err("That's a folder: pick the transcript files inside it.".to_string());
    }
    if meta.len() > parse::MAX_BYTES {
        return Err("This file is too big to be one meeting's transcript (over 25 MB).".to_string());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("Couldn't read it: {}", plain_io(&e)))?;
    let parsed = parse::parse(name, &bytes)?;
    let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64);
    let folder = path.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned());
    let (start, start_from, time_known) = start_of(&parsed, name, folder.as_deref(), modified, &local);
    Ok(Read { parsed, start, start_from, time_known })
}

/// An I/O error in plain words, without a path.
fn plain_io(e: &std::io::Error) -> &'static str {
    match e.kind() {
        std::io::ErrorKind::NotFound => "it isn't there any more",
        std::io::ErrorKind::PermissionDenied => "Windows didn't let Yap read it (another program may have it open)",
        _ => "something went wrong reading the file",
    }
}

/// What the preview shows of one file.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub id: u64,
    pub name: String,
    /// Why it can't be imported (the rest is empty then).
    pub error: Option<String>,
    pub format: Option<Format>,
    pub format_label: String,
    /// The meeting's name in the file ("" = the AI names it).
    pub title: String,
    /// When it began (unix seconds), and where that came from.
    pub start: Option<i64>,
    pub start_from: Option<&'static str>,
    pub time_known: bool,
    pub length_ms: u64,
    /// The file has no times: the length is a guess.
    pub estimated: bool,
    pub lines: usize,
    pub speakers: Vec<build::Found>,
    /// The speaker who is the person using Yap, by About you's name.
    pub me: Option<String>,
    /// The file has notes beside its transcript (Granola's): My thoughts.
    pub notes: bool,
    /// "Delete meeting transcripts after" (0: never): the page warns when
    /// the meeting is older.
    pub transcript_days: u32,
}

fn preview(s: &Staged, about_name: &str, transcript_days: u32) -> Preview {
    let mut out = Preview {
        id: s.id,
        name: s.name.clone(),
        error: None,
        format: None,
        format_label: String::new(),
        title: String::new(),
        start: None,
        start_from: None,
        time_known: false,
        length_ms: 0,
        estimated: false,
        lines: 0,
        speakers: Vec::new(),
        me: None,
        notes: false,
        transcript_days,
    };
    match &s.read {
        Err(e) => out.error = Some(e.clone()),
        Ok(r) => {
            let speakers = build::found(&r.parsed);
            out.me = build::me_from(&speakers, about_name);
            out.format = Some(r.parsed.format);
            out.format_label = r.parsed.format.label().to_string();
            out.title = r.parsed.title.clone();
            out.start = r.start;
            out.start_from = r.start_from;
            out.time_known = r.time_known;
            out.length_ms = build::length_ms(&r.parsed);
            out.estimated = r.parsed.estimated;
            out.lines = r.parsed.lines.len();
            out.speakers = speakers;
            out.notes = !r.parsed.notes.trim().is_empty();
        }
    }
    out
}

/// Read `paths`, keep them for the import, and say what each holds.
async fn stage(paths: Vec<PathBuf>) -> Result<Vec<Preview>, String> {
    let read: Vec<Staged> = tauri::async_runtime::spawn_blocking(move || paths.iter().take(MAX_STAGED).map(|p| read_file(p)).collect())
        .await
        .map_err(|e| e.to_string())?;
    let cfg = crate::config::load();
    let previews: Vec<Preview> = read.iter().map(|s| preview(s, &cfg.about_name, cfg.meeting_transcript_days)).collect();
    let readable = read.iter().filter(|s| s.read.is_ok()).count();
    let formats: Vec<Format> = read.iter().filter_map(|s| s.read.as_ref().ok().map(|r| r.parsed.format)).collect();
    tracing::info!(files = read.len(), readable, ?formats, "Transcript import: files read for the preview");
    let mut all = staged();
    all.extend(read);
    let over = all.len().saturating_sub(MAX_STAGED);
    all.drain(..over);
    Ok(previews)
}

/// "Teams call · 5 Oct, 14:30": the made-up title the AI may replace.
fn made_up_title(format: Format, start: i64) -> String {
    let what = match format {
        Format::TeamsVtt | Format::TeamsDocx => "Teams call",
        Format::ZoomText => "Zoom call",
        Format::MeetText => "Meet call",
        _ => "Meeting",
    };
    match chrono::Local.timestamp_opt(start, 0).single() {
        Some(t) => format!("{what} \u{b7} {}", t.format("%-d %b, %H:%M")),
        None => what.to_string(),
    }
}

/// One staged file as a meeting note.
fn import_one(app: &AppHandle, read: &Read, choice: &Choice) -> crate::notes::Note {
    let me = choice.me.as_deref().map(str::trim).filter(|m| !m.is_empty());
    let start = choice.start.max(0);
    let built = build::build(&read.parsed, start as u64, me);
    let given = choice.title.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string);
    let named = given.unwrap_or_else(|| read.parsed.title.trim().to_string());
    let (title, title_auto) = if named.is_empty() { (made_up_title(read.parsed.format, start), true) } else { (named, false) };
    // A recurring series by that name has a folder rule: there, else Meetings.
    let series = crate::notes::with_all(crate::calendar::filing::series_names);
    let folder = (!title_auto).then(|| crate::calendar::filing::folder_for_title(&series, &title)).flatten();
    let lines = built.transcript.len();
    let speakers = built.speakers.len();
    let note = crate::notes::create_meeting(crate::notes::NewMeeting {
        title,
        title_auto,
        content: read.parsed.notes.clone(),
        folder: folder.unwrap_or_else(|| crate::notes::MEETINGS.to_string()),
        transcript: built.transcript,
        participants: built.participants,
        speakers: built.speakers,
        source: crate::notes::IMPORTED.to_string(),
        created_ts: start as u64,
    });
    tracing::info!(note_id = note.id, format = ?read.parsed.format, lines, speakers, "Imported a meeting transcript");
    // The calendar's meeting at that time, while it still has it: its name,
    // attendees and series' folder.
    crate::calendar::link_imported(app, note.id);
    crate::notes::get(note.id).unwrap_or(note)
}

/// The AI title and the action plan of each imported meeting, one meeting
/// after another (a model on this PC isn't asked for five at once).
fn summarise(app: AppHandle, ids: Vec<u64>) {
    std::thread::spawn(move || {
        for id in ids {
            if crate::notes::get(id).is_none() {
                continue;
            }
            crate::meetings::meeting_assist::maybe_title(&app, id, true);
            if !crate::meetings::meeting_end::start_summary(&app, id) {
                continue;
            }
            let until = Instant::now() + SUMMARY_WAIT;
            while crate::meetings::meeting_end::summary_running_for(id) && Instant::now() < until {
                std::thread::sleep(Duration::from_millis(400));
            }
        }
    });
}

// ---- commands ------------------------------------------------------------------------------

/// Pick transcript files in Windows' dialog (several at once) and read
/// them: their previews (an empty list when the person cancelled).
#[tauri::command]
pub async fn transcript_import_pick(app: AppHandle, window: WebviewWindow) -> Result<Vec<Preview>, String> {
    let paths = pick(&app, window).await?;
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    stage(paths).await
}

/// Files dropped on Upload or the import dialog (Tauri's drop gives their
/// paths): their previews.
#[tauri::command]
pub async fn transcript_import_add(paths: Vec<String>) -> Result<Vec<Preview>, String> {
    stage(paths.into_iter().map(PathBuf::from).collect()).await
}

/// A file taken out of the preview.
#[tauri::command]
pub fn transcript_import_forget(id: u64) {
    staged().retain(|s| s.id != id);
}

/// The preview closed: nothing waits any more.
#[tauri::command]
pub fn transcript_import_clear() {
    staged().clear();
}

/// What the person chose for one previewed file.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Choice {
    pub id: u64,
    /// When the meeting began (unix seconds).
    pub start: i64,
    /// The speaker who is them ("" / none: nobody).
    #[serde(default)]
    pub me: Option<String>,
    /// A title of their own ("" / none: the file's, else the AI's).
    #[serde(default)]
    pub title: Option<String>,
}

/// How one import went.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub id: u64,
    pub note_id: Option<u64>,
    pub title: String,
    pub error: Option<String>,
}

/// Import the previewed files `items` names: a meeting note each, then
/// their titles and summaries in the background.
#[tauri::command]
pub fn transcript_import_run(app: AppHandle, items: Vec<Choice>) -> Result<Vec<Outcome>, String> {
    if items.is_empty() {
        return Err("Add a transcript to import first.".to_string());
    }
    let mut out = Vec::new();
    let mut made = Vec::new();
    for choice in items {
        let taken = {
            let mut all = staged();
            all.iter().position(|s| s.id == choice.id).map(|i| all.remove(i))
        };
        let Some(staged) = taken else {
            out.push(Outcome {
                id: choice.id,
                note_id: None,
                title: String::new(),
                error: Some("That file isn't waiting to be imported any more: add it again.".to_string()),
            });
            continue;
        };
        match &staged.read {
            Err(e) => out.push(Outcome { id: choice.id, note_id: None, title: String::new(), error: Some(e.clone()) }),
            Ok(read) => {
                let note = import_one(&app, read, &choice);
                made.push(note.id);
                out.push(Outcome { id: choice.id, note_id: Some(note.id), title: note.title, error: None });
            }
        }
    }
    if !made.is_empty() {
        let _ = app.emit("yap-notes-changed", ());
        summarise(app, made);
    }
    Ok(out)
}

/// The files picked in Windows' dialog (a test run: the ones it named).
async fn pick(app: &AppHandle, window: WebviewWindow) -> Result<Vec<PathBuf>, String> {
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() {
        return Ok(std::mem::take(&mut *E2E_FILES.lock().unwrap_or_else(|p| p.into_inner())));
    }
    use tauri_plugin_dialog::DialogExt;
    let dialog = app
        .dialog()
        .file()
        .set_parent(&window)
        .set_title("Import a transcript")
        .add_filter("Transcripts (Teams, Zoom, Meet, Otter, Granola)", &parse::EXTENSIONS)
        .add_filter("All files", &["*"]);
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_pick_files())
        .await
        .map_err(|e| e.to_string())?;
    Ok(picked.unwrap_or_default().into_iter().filter_map(|p| p.into_path().ok()).collect())
}

/// Test mode: the files the next pick returns, in place of the dialog.
#[cfg(debug_assertions)]
static E2E_FILES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Test mode: name the files the next "Choose files…" picks.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_transcript_files(paths: Vec<String>) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("test mode only".into());
    }
    *E2E_FILES.lock().unwrap_or_else(|p| p.into_inner()) = paths.into_iter().map(PathBuf::from).collect();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As if this PC's clock were UTC.
    fn utc(at: NaiveDateTime) -> Option<i64> {
        Some(at.and_utc().timestamp())
    }

    fn parsed(name: &str, text: &str) -> Parsed {
        parse::parse(name, text.as_bytes()).unwrap()
    }

    #[test]
    fn the_date_from_the_transcript_its_name_its_folder_or_its_file() {
        // 2026-10-05 14:30 UTC.
        let at = 1_791_210_600;
        let otter = parsed("call.txt", "Supplier call\nOct 5, 2026 2:30PM\n\nPriya Shah  0:01\nHello.\n");
        assert_eq!(start_of(&otter, "call.txt", None, None, &utc), (Some(at), Some("the transcript"), true));
        let teams = parsed("x.vtt", "WEBVTT\n\n00:00:01.000 --> 00:00:02.000\n<v Priya Shah>Hi.</v>\n");
        assert_eq!(start_of(&teams, "Weekly sync-20261005_143000-Meeting Recording.vtt", None, None, &utc), (Some(at), Some("the file's name"), true));
        // Zoom: its GMT name, in UTC whatever the PC's zone.
        assert_eq!(start_of(&teams, "GMT20261005-143000_Recording.transcript.vtt", None, None, &|_| None).0, Some(at));
        // Zoom's saved transcript: the date from its folder, the time from its first line.
        let zoom = parsed("meeting_saved_closed_caption.txt", "[Priya Shah] 14:30:00\nHello.\n\n[Tom Baker] 14:30:10\nHi.\n");
        assert_eq!(start_of(&zoom, "meeting_saved_closed_caption.txt", Some("2026-10-05 Supplier call"), None, &utc), (Some(at), Some("its folder's name"), true));
        // Nothing says: the file's own date, less the meeting's length.
        let plain = parsed("x.srt", "1\n00:10:00,000 --> 00:10:02,000\nHi.\n");
        assert_eq!(start_of(&plain, "x.srt", None, Some(at + 602), &utc), (Some(at), Some("the file's date"), false));
        assert_eq!(start_of(&plain, "x.srt", None, None, &utc), (None, None, false));
    }

    #[test]
    fn a_made_up_title_says_where_it_came_from() {
        assert!(made_up_title(Format::TeamsVtt, 1_791_210_600).starts_with("Teams call \u{b7} "));
        assert!(made_up_title(Format::Otter, 1_791_210_600).starts_with("Meeting \u{b7} "));
        // "5 Oct, 14:30", as call detection's titles end.
        let (_, when) = made_up_title(Format::ZoomText, 1_791_210_600).split_once(" \u{b7} ").map(|(a, b)| (a.to_string(), b.to_string())).unwrap();
        let (day_month, time) = when.split_once(", ").unwrap();
        assert!(day_month.split_once(' ').is_some_and(|(d, m)| d.parse::<u8>().is_ok() && m.len() == 3));
        assert_eq!(time.len(), 5);
    }

    #[test]
    fn a_staged_file_previews_what_it_holds_or_why_not() {
        let dir = std::env::temp_dir().join(format!("yap-import-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let good = dir.join("Supplier call.vtt");
        std::fs::write(&good, "WEBVTT\n\n00:00:01.000 --> 00:00:04.000\n<v Priya Shah>Morning.</v>\n\n00:00:05.000 --> 00:00:09.000\n<v Imogen Reyes>Shall we?</v>\n").unwrap();
        let bad = dir.join("deck.pdf");
        std::fs::write(&bad, b"%PDF-1.7").unwrap();
        let p = preview(&read_file(&good), "Imogen", 90);
        assert_eq!(p.error, None);
        assert_eq!(p.format, Some(Format::TeamsVtt));
        assert_eq!(p.lines, 2);
        assert_eq!(p.length_ms, 9_000);
        assert_eq!(p.me.as_deref(), Some("Imogen Reyes"));
        assert_eq!(p.speakers.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Priya Shah", "Imogen Reyes"]);
        assert_eq!(p.start_from, Some("the file's date"));
        let p = preview(&read_file(&bad), "", 0);
        assert!(p.error.unwrap().contains(".pdf"));
        let gone = preview(&read_file(&dir.join("missing.vtt")), "", 0);
        assert!(gone.error.unwrap().contains("isn't there"));
        std::fs::remove_dir_all(&dir).ok();
    }
}

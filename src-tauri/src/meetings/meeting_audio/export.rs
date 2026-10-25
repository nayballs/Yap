//! **Export a meeting's audio** (the notepad's ⋯ and the Meetings view's
//! right-click menu: "Export audio…"): the audio Yap kept of a meeting
//! (`meeting_audio.rs`), written where the person picks in the Save dialog.
//!
//! - **One file, both sides mixed** (as a line's ▶ plays them): each
//!   recording of the meeting in turn (a resumed meeting's recordings back
//!   to back), "You" and "Them" added sample for sample.
//! - **Or both sides as separate files**: `<name> - You.flac` and `<name> -
//!   Them.flac` beside the name picked.
//! - **FLAC** (`.flac`, the default): what Yap keeps already, written by the
//!   same encoder (`flac.rs`) — lossless and about half a WAV's size; Windows
//!   plays it. **WAV** when the name picked ends `.wav` (16-bit PCM). Either
//!   way 16 kHz mono, as kept.
//! - **Progress** for long meetings: `yap-meeting-audio-export {noteId,
//!   percent, state: running|done|error, files, error}` as it goes (a
//!   two-hour meeting decodes and encodes in seconds). Written to
//!   `<file>.part` and renamed when whole: a failed export leaves nothing
//!   half-written; one over a file the dialog said to replace replaces it.
//!
//! Not while the meeting records (its files are still being written).
//! Nothing leaves the PC. Logs: the note id, counts and lengths — never a
//! path or a name.

use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use super::flac;

/// Progress and the end of an export, for the window that started it.
pub const EVENT: &str = "yap-meeting-audio-export";

const RATE: u64 = flac::RATE as u64;
/// Read and written this much at a time (a second).
const STEP: usize = flac::RATE as usize;

/// What a name picked in the Save dialog writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Flac,
    Wav,
}

impl Format {
    /// By the name's extension: `.wav` is WAV, anything else FLAC.
    pub fn of(path: &Path) -> Format {
        match path.extension().and_then(|e| e.to_str()) {
            Some(e) if e.eq_ignore_ascii_case("wav") => Format::Wav,
            _ => Format::Flac,
        }
    }

    fn ext(self) -> &'static str {
        match self {
            Format::Flac => "flac",
            Format::Wav => "wav",
        }
    }
}

/// The files an export writes (pure): `path` itself for one mixed file;
/// for both sides, `<stem> - You.<ext>` and `<stem> - Them.<ext>` beside it.
pub fn targets(path: &Path, separate: bool) -> Vec<PathBuf> {
    let format = Format::of(path);
    let with_ext = if path.extension().is_some() { path.to_path_buf() } else { path.with_extension(format.ext()) };
    if !separate {
        return vec![with_ext];
    }
    let stem = with_ext.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "Meeting".to_string());
    let dir = with_ext.parent().map(Path::to_path_buf).unwrap_or_default();
    ["You", "Them"].iter().map(|side| dir.join(format!("{stem} - {side}.{}", format.ext()))).collect()
}

/// One file being written.
enum Sink {
    Flac(flac::Writer),
    Wav { file: std::fs::File, samples: u64 },
}

impl Sink {
    fn create(path: &Path, format: Format) -> std::io::Result<Sink> {
        // A leftover from an export that failed.
        let _ = std::fs::remove_file(path);
        Ok(match format {
            Format::Flac => Sink::Flac(flac::Writer::create(path)?),
            Format::Wav => {
                let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
                file.write_all(&wav_header(0))?;
                Sink::Wav { file, samples: 0 }
            }
        })
    }

    fn push(&mut self, samples: &[f32]) -> std::io::Result<()> {
        match self {
            Sink::Flac(w) => w.push(samples),
            Sink::Wav { file, samples: n } => {
                let mut bytes = Vec::with_capacity(samples.len() * 2);
                for &s in samples {
                    bytes.extend_from_slice(&flac::to_i16(s).to_le_bytes());
                }
                *n += samples.len() as u64;
                file.write_all(&bytes)
            }
        }
    }

    /// Done: lengths into the header. Returns the samples written.
    fn finish(self) -> std::io::Result<u64> {
        match self {
            Sink::Flac(w) => w.finish(),
            Sink::Wav { mut file, samples } => {
                file.seek(SeekFrom::Start(0))?;
                file.write_all(&wav_header(samples))?;
                file.sync_data()?;
                Ok(samples)
            }
        }
    }
}

/// A 16 kHz mono 16-bit WAV header for `samples` samples.
fn wav_header(samples: u64) -> Vec<u8> {
    let data = (samples * 2).min(u32::MAX as u64 - 36) as u32;
    let mut w = Vec::with_capacity(44);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&flac::RATE.to_le_bytes());
    w.extend_from_slice(&(flac::RATE * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data.to_le_bytes());
    w
}

/// `a` and `b` added, sample for sample (the shorter padded with silence).
pub fn mix(a: &[f32], b: &[f32]) -> Vec<f32> {
    let at = |s: &[f32], i: usize| s.get(i).copied().unwrap_or(0.0);
    (0..a.len().max(b.len())).map(|i| (at(a, i) + at(b, i)).clamp(-1.0, 1.0)).collect()
}

/// What an export wrote.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Exported {
    /// The files, as written.
    pub files: Vec<String>,
    /// How long each is (seconds).
    pub secs: u64,
    /// Their size in all (bytes).
    pub bytes: u64,
}

/// One recording to export: its two sides' files and its length (samples).
pub struct Part {
    pub you: PathBuf,
    pub them: PathBuf,
    pub len: u64,
}

/// Write `parts` to `files` (one mixed, or You then Them), `progress(0–100)`
/// as it goes. Pure but for the files; the `.part` names are the caller's.
pub fn write(parts: &[Part], files: &[PathBuf], format: Format, progress: &dyn Fn(u8)) -> Result<u64, String> {
    let separate = files.len() == 2;
    let total: u64 = parts.iter().map(|p| p.len).sum::<u64>().max(1);
    let mut sinks: Vec<Sink> = files
        .iter()
        .map(|f| Sink::create(f, format))
        .collect::<std::io::Result<_>>()
        .map_err(|e| format!("Couldn't write the file: {e}"))?;
    let mut done: u64 = 0;
    let mut told = 0u8;
    for part in parts {
        let mut you = flac::Reader::open(&part.you)?;
        let mut them = flac::Reader::open(&part.them)?;
        let mut left = part.len;
        while left > 0 {
            let n = (left as usize).min(STEP);
            let (a, b) = (you.read(n), them.read(n));
            let put = |sink: &mut Sink, s: &[f32]| -> Result<(), String> {
                // A side that ended early (its device stopped): silence, so
                // both stay on the recording's clock.
                let mut s = s.to_vec();
                s.resize(n, 0.0);
                sink.push(&s).map_err(|e| format!("Couldn't write the file: {e}"))
            };
            if separate {
                put(&mut sinks[0], &a)?;
                put(&mut sinks[1], &b)?;
            } else {
                put(&mut sinks[0], &mix(&a, &b))?;
            }
            left -= n as u64;
            done += n as u64;
            let pct = (done * 100 / total).min(99) as u8;
            if pct != told {
                told = pct;
                progress(pct);
            }
        }
    }
    let mut samples = 0;
    for sink in sinks {
        samples = sink.finish().map_err(|e| format!("Couldn't write the file: {e}"))?;
    }
    Ok(samples)
}

/// Export note `note_id`'s kept audio to `path` (see the module notes).
pub fn export(note_id: u64, path: &Path, separate: bool, progress: &dyn Fn(u8)) -> Result<Exported, String> {
    let all = super::recordings(note_id);
    if all.is_empty() {
        return Err("This meeting's audio isn't kept any more".to_string());
    }
    if all.iter().any(|m| super::recording_now(note_id, m.recording)) {
        return Err("Stop the recording first".to_string());
    }
    let parts: Vec<Part> = all
        .iter()
        .map(|m| {
            let (you, them) = (super::audio_path(note_id, m.recording, "you"), super::audio_path(note_id, m.recording, "them"));
            let mut len = m.you_len.max(m.them_len);
            if len == 0 {
                // A crash's recording not counted yet.
                len = super::count_from(note_id, m.recording, "you", 0).max(super::count_from(note_id, m.recording, "them", 0));
            }
            Part { you, them, len }
        })
        .collect();
    let format = Format::of(path);
    let files = targets(path, separate);
    let partial: Vec<PathBuf> = files.iter().map(|f| f.with_extension(format!("{}.part", format.ext()))).collect();
    let written = write(&parts, &partial, format, progress);
    let samples = match written {
        Ok(s) => s,
        Err(e) => {
            for p in &partial {
                let _ = std::fs::remove_file(p);
            }
            return Err(e);
        }
    };
    for (from, to) in partial.iter().zip(&files) {
        if let Err(e) = std::fs::rename(from, to) {
            for p in &partial {
                let _ = std::fs::remove_file(p);
            }
            return Err(format!("Couldn't save the file there: {e}"));
        }
    }
    let bytes = files.iter().filter_map(|f| std::fs::metadata(f).ok()).map(|m| m.len()).sum();
    tracing::info!(note_id, recordings = parts.len(), separate, secs = samples / RATE, "Meeting audio exported");
    Ok(Exported { files: files.iter().map(|f| f.to_string_lossy().to_string()).collect(), secs: samples / RATE, bytes })
}

/// "Supplier call" → "Supplier call.flac": a name Windows takes.
pub fn file_name(title: &str) -> String {
    let clean: String = title
        .trim()
        .chars()
        .map(|c| if matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() { '-' } else { c })
        .take(120)
        .collect();
    let clean = clean.trim().trim_end_matches('.').trim();
    format!("{}.flac", if clean.is_empty() { "Meeting audio" } else { clean })
}

/// Why note `note_id`'s audio can't be exported now, if so (asked before
/// the dialog opens).
fn cant_export(note_id: u64) -> Option<&'static str> {
    let all = super::recordings(note_id);
    if all.is_empty() {
        Some("This meeting's audio isn't kept any more")
    } else if all.iter().any(|m| super::recording_now(note_id, m.recording)) {
        Some("Stop the recording first")
    } else {
        None
    }
}

/// Where the person wants it: Windows' Save dialog over `window` (a test
/// run: the file it named, [`e2e_audio_export_file`]). `None`: cancelled.
async fn pick(app: &AppHandle, window: tauri::WebviewWindow, name: String, separate: bool) -> Result<Option<PathBuf>, String> {
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() {
        let _ = (app, window, name, separate);
        return Ok(E2E_FILE.lock().unwrap_or_else(|p| p.into_inner()).take());
    }
    use tauri_plugin_dialog::DialogExt;
    let dialog = app
        .dialog()
        .file()
        .set_parent(&window)
        .set_title(if separate { "Export each side of the meeting" } else { "Export the meeting's audio" })
        .set_file_name(name)
        .add_filter("FLAC audio (lossless, smaller)", &["flac"])
        .add_filter("WAV audio", &["wav"]);
    let picked = tauri::async_runtime::spawn_blocking(move || dialog.blocking_save_file()).await.map_err(|e| e.to_string())?;
    Ok(picked.and_then(|p| p.into_path().ok()))
}

/// Test mode: the file the next "Export audio…" writes, in place of the
/// dialog ([`e2e_audio_export_file`]).
#[cfg(debug_assertions)]
static E2E_FILE: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Test mode only: name the file the next "Export audio…" writes.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_audio_export_file(path: String) -> Result<(), String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    *E2E_FILE.lock().unwrap_or_else(|p| p.into_inner()) = Some(PathBuf::from(path));
    Ok(())
}

/// "Export audio…" (from `window`): the Save dialog for note `note_id`'s
/// audio, then one mixed file or both sides (`separate`). `None`: the
/// dialog was cancelled. Progress and the end come as [`EVENT`] too.
#[tauri::command]
pub async fn meeting_audio_export(app: AppHandle, window: tauri::WebviewWindow, note_id: u64, separate: Option<bool>) -> Result<Option<Exported>, String> {
    let separate = separate.unwrap_or(false);
    let note = crate::notes::get(note_id).ok_or("Note not found")?;
    if let Some(why) = cant_export(note_id) {
        return Err(why.to_string());
    }
    let Some(path) = pick(&app, window, file_name(&note.title), separate).await? else {
        return Ok(None);
    };
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let progress = |percent: u8| {
            let _ = handle.emit(EVENT, serde_json::json!({ "noteId": note_id, "percent": percent, "state": "running" }));
        };
        export(note_id, &path, separate, &progress)
    })
    .await
    .map_err(|e| e.to_string())?;
    match &result {
        Ok(x) => {
            let _ = app.emit(EVENT, serde_json::json!({ "noteId": note_id, "percent": 100, "state": "done", "files": x.files }));
        }
        Err(e) => {
            tracing::warn!(note_id, "Meeting audio: export failed");
            let _ = app.emit(EVENT, serde_json::json!({ "noteId": note_id, "state": "error", "error": e }));
        }
    }
    result.map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(secs: f32, gain: f32) -> Vec<f32> {
        (0..(secs * RATE as f32) as usize).map(|i| gain * (i as f32 * 0.05).sin()).collect()
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("yap-export-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn kept(dir: &Path, name: &str, audio: &[f32]) -> PathBuf {
        let p = dir.join(name);
        let mut w = flac::Writer::create(&p).unwrap();
        w.push(audio).unwrap();
        w.finish().unwrap();
        p
    }

    #[test]
    fn names_follow_the_one_picked() {
        assert_eq!(file_name("Supplier call"), "Supplier call.flac");
        assert_eq!(file_name("Q3: plan / budget?"), "Q3- plan - budget-.flac");
        assert_eq!(file_name("  "), "Meeting audio.flac");
        assert_eq!(file_name("Notes..."), "Notes.flac");
        let p = Path::new("C:/Exports/Supplier call.flac");
        assert_eq!(targets(p, false), vec![PathBuf::from("C:/Exports/Supplier call.flac")]);
        assert_eq!(
            targets(p, true),
            vec![PathBuf::from("C:/Exports/Supplier call - You.flac"), PathBuf::from("C:/Exports/Supplier call - Them.flac")]
        );
        assert_eq!(Format::of(Path::new("a.WAV")), Format::Wav);
        assert_eq!(targets(Path::new("C:/x/notes"), false), vec![PathBuf::from("C:/x/notes.flac")]);
        assert_eq!(targets(Path::new("C:/x/call.wav"), true)[1], PathBuf::from("C:/x/call - Them.wav"));
    }

    #[test]
    fn both_sides_are_mixed_into_one_file_recording_after_recording() {
        let dir = scratch("mix");
        // Two recordings (a resumed meeting): 3 s and 2 s; "Them" shorter in the first.
        let parts = vec![
            Part { you: kept(&dir, "1-you.flac", &tone(3.0, 0.3)), them: kept(&dir, "1-them.flac", &tone(2.5, 0.3)), len: 3 * RATE },
            Part { you: kept(&dir, "2-you.flac", &tone(2.0, 0.2)), them: kept(&dir, "2-them.flac", &tone(2.0, 0.2)), len: 2 * RATE },
        ];
        let seen = std::sync::Mutex::new(Vec::new());
        let out = dir.join("call.flac");
        let n = write(&parts, std::slice::from_ref(&out), Format::Flac, &|p| seen.lock().unwrap().push(p)).unwrap();
        assert_eq!(n, 5 * RATE, "both recordings, back to back");
        let mut r = flac::Reader::open(&out).unwrap();
        assert_eq!(r.count_rest(), 5 * RATE);
        // Both sides added: louder than either.
        let mut r = flac::Reader::open(&out).unwrap();
        let first = r.read(RATE as usize);
        assert!(first.iter().any(|s| s.abs() > 0.45), "the two sides add up");
        let seen = seen.into_inner().unwrap();
        assert!(!seen.is_empty() && seen.windows(2).all(|w| w[0] < w[1]) && *seen.last().unwrap() <= 99);

        // WAV: the same length, 16-bit PCM.
        let wav = dir.join("call.wav");
        assert_eq!(write(&parts, std::slice::from_ref(&wav), Format::Wav, &|_| {}).unwrap(), 5 * RATE);
        let bytes = std::fs::read(&wav).unwrap();
        assert_eq!(bytes.len() as u64, 44 + 5 * RATE * 2);
        let pcm = crate::voice::tts::parse_wav(&bytes).unwrap();
        assert_eq!(pcm.samples.len() as u64, 5 * RATE);

        // Each side on its own.
        let sides = [dir.join("call - You.flac"), dir.join("call - Them.flac")];
        write(&parts, &sides, Format::Flac, &|_| {}).unwrap();
        for side in &sides {
            assert_eq!(flac::Reader::open(side).unwrap().count_rest(), 5 * RATE, "padded to the recording's clock");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

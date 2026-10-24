//! **Never lose a dictation** (2026-10-22, after Wispr Flow's of Aug 2026):
//! the audio of recent dictations, kept on this PC for a day, so one whose
//! paste couldn't land, that Yap stopped transcribing (a crash, no model)
//! or that came out wrong can be **run again** — from Home's feed, on the
//! same model or another — and **played**.
//!
//! **What's kept**: each dictation's audio (16 kHz mono 16-bit WAV, ~32 KB a
//! second) in `<data>/recent-audio/<id>.wav`, the id being when it was
//! recorded (unix ms). Written on its own thread at the stop, before the
//! transcription (a temp file renamed into place), with a `<id>.pending`
//! marker beside it until the dictation is in history ([`finished`]) — so
//! at the next start ([`init`]) one Yap never finished is listed as failed
//! ("Yap closed before it was transcribed"). History entries carry the id
//! (`history::HistoryEntry::audio`). Not for edit mode or a question to the
//! Voice Assistant; a Command, or a recording nothing was heard in, drops
//! its audio at once ([`discard`]).
//!
//! **For how long** ([`prune`], at start, after each dictation and hourly):
//! [`KEEP_SECS`] (24 h), at most [`MAX_FILES`] files and [`MAX_BYTES`] — the
//! oldest go first. Deleting a history entry deletes its audio; clearing
//! history clears it all; Settings → Privacy & data has the switch
//! (`config.keep_recent_audio`, on; only with history on) and **Clear**
//! ([`clear`]). Nothing here ever leaves the PC.
//!
//! **Logs**: ids, lengths and counts only — never the folder's path next to
//! a transcript (`log_privacy.rs`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// How long a dictation's audio is kept.
pub const KEEP_SECS: u64 = 24 * 60 * 60;
/// At most this many dictations' audio…
pub const MAX_FILES: usize = 500;
/// …and this much of it (~1.7 hours of speech).
pub const MAX_BYTES: u64 = 200 * 1024 * 1024;
/// How often the old audio is looked for while Yap runs.
const PRUNE_EVERY: Duration = Duration::from_secs(60 * 60);
const DIR: &str = "recent-audio";
const SAMPLE_RATE: u32 = 16_000;

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// The folder (`<data>/recent-audio`).
pub fn dir() -> PathBuf {
    crate::config::data_dir().join(DIR)
}

fn wav_path(id: &str) -> PathBuf {
    dir().join(format!("{id}.wav"))
}

fn marker_path(id: &str) -> PathBuf {
    dir().join(format!("{id}.pending"))
}

/// An id is digits only (it names a file: never a path).
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 20 && id.bytes().all(|b| b.is_ascii_digit())
}

/// A new id: now in unix ms, one more than the last if that's already taken.
fn new_id() -> String {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let mut now = now_ms();
    loop {
        let last = LAST.load(Ordering::SeqCst);
        if now <= last {
            now = last + 1;
        }
        if LAST.compare_exchange(last, now, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
            return now.to_string();
        }
    }
}

/// When a dictation was recorded, from its id (unix seconds).
pub fn recorded_at(id: &str) -> Option<u64> {
    id.parse::<u64>().ok().map(|ms| ms / 1000)
}

/// Whether this dictation's audio is kept now.
pub fn wanted(cfg: &crate::config::YapConfig) -> bool {
    cfg.keep_recent_audio && cfg.history_enabled
}

/// Keep a dictation's audio (16 kHz mono) before it's transcribed: the
/// marker now, the WAV on its own thread (the transcription doesn't wait).
/// Its id, or `None` when it isn't kept (switched off, history off, nothing
/// recorded).
pub fn keep(samples: &[f32], cfg: &crate::config::YapConfig) -> Option<String> {
    if !wanted(cfg) || samples.is_empty() {
        return None;
    }
    let id = new_id();
    if let Err(e) = std::fs::create_dir_all(dir()).and_then(|()| std::fs::write(marker_path(&id), b"")) {
        tracing::warn!("Recent audio: couldn't keep a dictation's audio: {e}");
        return None;
    }
    let bytes = wav(samples);
    let file_id = id.clone();
    let writer = std::thread::Builder::new().name("recent-audio".into()).spawn(move || {
        let tmp = dir().join(format!("{file_id}.wav.tmp"));
        let written = std::fs::write(&tmp, &bytes).and_then(|()| std::fs::rename(&tmp, wav_path(&file_id)));
        match written {
            Ok(()) => tracing::debug!(id = %file_id, bytes = bytes.len(), "Recent audio: kept"),
            Err(e) => {
                tracing::warn!(id = %file_id, "Recent audio: couldn't write it: {e}");
                let _ = std::fs::remove_file(&tmp);
            }
        }
        prune();
    });
    match writer {
        Ok(handle) => {
            writers().insert(id.clone(), handle);
            Some(id)
        }
        Err(_) => {
            let _ = std::fs::remove_file(marker_path(&id));
            None
        }
    }
}

/// The WAVs being written, by id ([`written`] waits for one).
fn writers() -> std::sync::MutexGuard<'static, HashMap<String, std::thread::JoinHandle<()>>> {
    static WRITERS: std::sync::LazyLock<Mutex<HashMap<String, std::thread::JoinHandle<()>>>> =
        std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
    WRITERS.lock().unwrap_or_else(|p| p.into_inner())
}

/// Wait for a dictation's WAV to be on disk (it's written while the
/// dictation is transcribed: by its end, long done).
fn written(id: &str) {
    let handle = writers().remove(id);
    if let Some(h) = handle {
        let _ = h.join();
    }
}

/// The dictation is in history (transcribed, or listed as failed): its audio
/// stays for its day, no longer pending.
pub fn finished(id: &str) {
    if valid_id(id) {
        written(id);
        let _ = std::fs::remove_file(marker_path(id));
    }
}

/// Drop a dictation's audio now (a Command, nothing heard, its history
/// entry deleted).
pub fn discard(id: &str) {
    if !valid_id(id) {
        return;
    }
    written(id);
    let _ = std::fs::remove_file(marker_path(id));
    let _ = std::fs::remove_file(wav_path(id));
}

/// Whether a dictation's audio is still kept.
pub fn has(id: &str) -> bool {
    valid_id(id) && wav_path(id).is_file()
}

/// A dictation's audio, as 16 kHz mono samples.
pub fn load(id: &str) -> Option<Vec<f32>> {
    let bytes = wav_bytes(id)?;
    let pcm = crate::voice::tts::parse_wav(&bytes).ok()?;
    Some(crate::dictation::pipeline::resample_linear(&pcm.samples, pcm.sample_rate, SAMPLE_RATE))
}

/// A dictation's audio file as it is (Home's Play).
pub fn wav_bytes(id: &str) -> Option<Vec<u8>> {
    if !valid_id(id) {
        return None;
    }
    std::fs::read(wav_path(id)).ok()
}

/// 16-bit PCM WAV of 16 kHz mono samples (a meeting line's clip too:
/// `meeting_audio::clip`).
pub(crate) fn wav(samples: &[f32]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut w = Vec::with_capacity(44 + samples.len() * 2);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&1u16.to_le_bytes());
    w.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    w.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    w.extend_from_slice(&2u16.to_le_bytes());
    w.extend_from_slice(&16u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        w.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32_767.0) as i16).to_le_bytes());
    }
    w
}

/// A kept file: its id, size and when it was recorded (unix seconds).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    pub id: String,
    pub bytes: u64,
    pub at: u64,
}

/// Which of `files` go (pure): those older than [`KEEP_SECS`], then the
/// oldest while there are more than [`MAX_FILES`] or [`MAX_BYTES`] of them.
pub fn to_drop(files: &[Kept], now: u64) -> Vec<String> {
    let mut keep: Vec<&Kept> = files.iter().filter(|f| now.saturating_sub(f.at) <= KEEP_SECS).collect();
    let mut out: Vec<String> = files
        .iter()
        .filter(|f| now.saturating_sub(f.at) > KEEP_SECS)
        .map(|f| f.id.clone())
        .collect();
    keep.sort_by_key(|f| (f.at, f.id.clone()));
    let mut total: u64 = keep.iter().map(|f| f.bytes).sum();
    let mut count = keep.len();
    for f in keep {
        if count <= MAX_FILES && total <= MAX_BYTES {
            break;
        }
        out.push(f.id.clone());
        total -= f.bytes;
        count -= 1;
    }
    out
}

/// What's kept now.
fn kept() -> Vec<Kept> {
    let Ok(entries) = std::fs::read_dir(dir()) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let id = name.strip_suffix(".wav")?.to_string();
            if !valid_id(&id) {
                return None;
            }
            let bytes = e.metadata().ok()?.len();
            Some(Kept { at: recorded_at(&id)?, id, bytes })
        })
        .collect()
}

/// Drop the audio past its time (and leftovers: unfinished writes, markers
/// of audio that's gone).
pub fn prune() {
    let now = now_ms() / 1000;
    let files = kept();
    let drop = to_drop(&files, now);
    for id in &drop {
        let _ = std::fs::remove_file(wav_path(id));
        let _ = std::fs::remove_file(marker_path(id));
    }
    if !drop.is_empty() {
        tracing::info!(dropped = drop.len(), kept = files.len() - drop.len(), "Recent audio: old dictations' audio deleted");
    }
    let Ok(entries) = std::fs::read_dir(dir()) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let old = e
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > Duration::from_secs(3_600));
        let stray = name.ends_with(".tmp")
            || name.strip_suffix(".pending").is_some_and(|id| !wav_path(id).is_file());
        if stray && old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Delete all of it (Settings → Privacy & data → Clear, or history cleared).
pub fn clear() {
    let files = kept();
    let _ = std::fs::remove_dir_all(dir());
    tracing::info!(deleted = files.len(), "Recent audio: cleared");
}

/// At start: the dictations Yap never finished (it closed, or crashed,
/// mid-way) listed as failed with their audio, then the old audio pruned —
/// and again every hour.
pub fn init(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new().name("recent-audio".into()).spawn(move || {
        recover(&app);
        loop {
            prune();
            std::thread::sleep(PRUNE_EVERY);
        }
    });
}

/// Dictations left pending by the last run: in history as failed (when
/// their audio was written), else forgotten.
fn recover(app: &AppHandle) {
    let Ok(entries) = std::fs::read_dir(dir()) else {
        return;
    };
    let cfg = crate::config::load();
    let mut listed = 0;
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(id) = name.strip_suffix(".pending").filter(|id| valid_id(id)).map(str::to_string) else {
            continue;
        };
        if has(&id) && cfg.history_enabled {
            crate::dictation::history::record_failed(
                recorded_at(&id),
                &cfg.model_size,
                "",
                &id,
                "Yap closed before it was transcribed",
            );
            listed += 1;
        }
        finished(&id);
    }
    if listed > 0 {
        tracing::info!(listed, "Recent audio: dictations Yap never finished are in history as failed");
        let _ = app.emit("yap-history-changed", ());
    }
}

/// Settings → Privacy & data: how much is kept.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub count: usize,
    pub bytes: u64,
}

#[tauri::command]
pub fn recent_audio_status() -> Status {
    let files = kept();
    Status { count: files.len(), bytes: files.iter().map(|f| f.bytes).sum() }
}

/// Settings → Privacy & data → Clear recent audio.
#[tauri::command]
pub fn recent_audio_clear(app: AppHandle) {
    clear();
    let _ = app.emit("yap-history-changed", ());
}

/// Home's Play: a dictation's audio file, as bytes (a WAV).
#[tauri::command]
pub fn recent_audio_wav(id: String) -> Result<tauri::ipc::Response, String> {
    wav_bytes(&id)
        .map(tauri::ipc::Response::new)
        .ok_or_else(|| "This dictation's audio isn't kept any more".to_string())
}

/// Home's **Run again**: the dictation at `ts` (its audio `audio`)
/// transcribed again — on `model` (an installed one; none = the one in
/// use) — then AI cleanup as for its app, the corrections and snippets
/// (`pipeline::Rerun`). Its history entry gets the new text and is no
/// longer a failure; nothing is pasted. `{ text, model, modelName }`.
#[tauri::command]
pub async fn history_rerun(app: AppHandle, ts: u64, audio: String, model: Option<String>) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    let entry = crate::dictation::history::find_audio(ts, &audio).ok_or("This dictation isn't in your history any more")?;
    let id = audio.clone();
    let samples = tauri::async_runtime::spawn_blocking(move || load(&id))
        .await
        .map_err(|e| format!("Couldn't read its audio: {e}"))?
        .ok_or("This dictation's audio isn't kept any more")?;
    let rerun = {
        let state = app.state::<crate::AppState>();
        let guard = state.pipeline.lock().map_err(|_| "The dictation pipeline isn't available")?;
        guard.as_ref().ok_or("The dictation pipeline isn't running")?.rerunner()
    };
    let crate::dictation::pipeline::Reran { raw, text, said, model } = rerun.run(samples, model, &entry.app).await?;
    crate::dictation::history::rerun_result(&audio, &raw, &text, said.as_deref(), &model);
    tracing::info!(chars = text.chars().count(), model = %model, "Ran a dictation again");
    let _ = app.emit("yap-history-changed", ());
    Ok(serde_json::json!({ "text": text, "model": model, "modelName": crate::dictation::stt::model_name(&model) }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(id: &str, at: u64, bytes: u64) -> Kept {
        Kept { id: id.into(), at, bytes }
    }

    #[test]
    fn a_day_old_audio_goes_and_so_does_the_oldest_over_the_caps() {
        let now = 10 * KEEP_SECS;
        // Older than a day: gone; within it: kept.
        let files = [k("1", now - KEEP_SECS - 1, 10), k("2", now - 60, 10), k("3", now, 10)];
        assert_eq!(to_drop(&files, now), vec!["1".to_string()]);
        // Too many: the oldest go first.
        let many: Vec<Kept> = (0..MAX_FILES as u64 + 3).map(|i| k(&i.to_string(), now - 1_000 + i, 1)).collect();
        assert_eq!(to_drop(&many, now), vec!["0", "1", "2"]);
        // Too big: the oldest go until it fits.
        let big = [k("a", now - 30, MAX_BYTES / 2), k("b", now - 20, MAX_BYTES / 2), k("c", now - 10, MAX_BYTES / 2)];
        assert_eq!(to_drop(&big, now), vec!["a"]);
    }

    #[test]
    fn ids_are_numbers_only_and_say_when() {
        assert!(valid_id("1761140000123"));
        assert!(!valid_id("..\\config"));
        assert!(!valid_id(""));
        assert!(!valid_id("12a"));
        assert_eq!(recorded_at("1761140000123"), Some(1_761_140_000));
        let (a, b) = (new_id(), new_id());
        assert!(b.parse::<u64>().unwrap() > a.parse::<u64>().unwrap());
    }

    #[test]
    fn the_wav_holds_the_audio() {
        let samples: Vec<f32> = (0..1_600).map(|i| (i as f32 / 100.0).sin() * 0.5).collect();
        let pcm = crate::voice::tts::parse_wav(&wav(&samples)).unwrap();
        assert_eq!((pcm.sample_rate, pcm.channels, pcm.samples.len()), (16_000, 1, 1_600));
        assert!((pcm.samples[100] - samples[100]).abs() < 0.001);
    }
}

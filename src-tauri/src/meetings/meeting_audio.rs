//! **Meeting audio kept on disk** (2026-10-23, after Wispr Flow's 14 days of
//! playback and Meetily's re-transcription): each recording's two sides —
//! the mic ("You") and the call ("Them") — written to files as they're
//! captured, so that
//! - **a crash loses nothing**: at the next start Yap finds the audio that
//!   never became lines and offers to transcribe it into the meeting's note
//!   ([`init`], [`recover`]), then does the end-of-meeting steps as a stop
//!   would (`meeting_end::recovered`);
//! - **a meeting can be transcribed again** later, on the speech model in use
//!   then ([`transcribe_again`]: the lines replaced, the person's notes, the
//!   names on lines that still match and the summary kept — `redo.rs`);
//! - **a line can be played** ([`clip`]: both sides of its stretch, mixed).
//!
//! **What's kept**: `<data>/meeting-audio/<note id>/<recording>-you.flac`
//! and `-them.flac` (16 kHz mono, lossless: `flac.rs` says why FLAC, and
//! measures it) and `<recording>.json` ([`Meta`]: when it started, how far
//! each side is transcribed into the note, where dictations began, whether
//! it ended). `<recording>` is when the files were made (unix ms). A resumed
//! meeting is a new recording beside the first.
//!
//! **Written as it's recorded** ([`Recorder`]): the recorder's push path
//! (`meeting.rs`) copies every sample it appends — a hotkey dictation already
//! blanked, a stalled side already filled — to a queue this module's writer
//! thread empties every half second, a FLAC frame (256 ms) at a time, onto
//! the disk itself every 5 s. So the files are the recording's own clock,
//! sample for sample, and a dictation is left out of them as it is out of
//! the transcript. The files are opened so no other program can write or
//! delete them while they're written — which is also how a second Yap on the
//! same data (the dev build beside the installed one) tells a recording
//! that's going on from one a crash left ([`in_use`]).
//!
//! **For how long** ([`prune`], at start and hourly): "Keep meeting audio"
//! (Settings → Meetings, on) for `meeting_audio_days` (14, like Wispr's
//! playback: long enough to transcribe a meeting again after a better model
//! arrives or to check what someone said last week, short enough that it
//! never piles up — a week of meetings is ~0.5–1 GB), and never more than
//! [`MAX_BYTES`] in all (the oldest go first). Off, the audio is still
//! written while a meeting records (the crash case) and deleted as soon as
//! the meeting's lines are in. Deleting a note deletes its audio; Settings
//! has "Delete all meeting audio". A recording going on is never touched.
//! Nothing here ever leaves the PC.
//!
//! **Logs**: note ids, counts, lengths and sizes only — never a path, a
//! title or a line.

pub mod flac;
pub mod redo;

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::meetings::meeting::AudioBuf;

const DIR: &str = "meeting-audio";
const RATE: u64 = flac::RATE as u64;
/// How long meeting audio is kept unless Settings says otherwise.
pub const DEFAULT_DAYS: u32 = 14;
/// At most this much meeting audio in all (~150 hours of meetings): past it
/// the oldest recordings go first, however recent.
pub const MAX_BYTES: u64 = 10 * 1024 * 1024 * 1024;
/// How often the writer empties the queues…
const WRITE_EVERY: Duration = Duration::from_millis(500);
/// …and puts the files onto the disk itself (a power cut keeps them too).
const SYNC_EVERY: Duration = Duration::from_secs(5);
/// How often old audio is looked for while Yap runs.
const PRUNE_EVERY: Duration = Duration::from_secs(60 * 60);
/// Less than this left untranscribed after a crash isn't worth asking about.
const MIN_LEFT: u64 = RATE;
/// A clip is padded this much each side, and at most this long.
const CLIP_PAD_MS: u64 = 250;
const CLIP_MAX_MS: u64 = 60_000;

/// A note's kept audio changed (written, recovered, deleted): `{ noteId }`
/// (`null`: all of it, "Delete all meeting audio").
pub const EVENT_AUDIO: &str = "yap-meeting-audio";
/// A note's audio being transcribed (again, or after a crash): [`Redo`].
pub const EVENT_REDO: &str = "yap-meeting-redo";
/// The alert's button: transcribe what crashes left ([`recover_all`]).
pub const RECOVER_ACTION: &str = "recoverMeetings";

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// ---- Where it lives ----

/// The folder (`<data>/meeting-audio`). Unit tests get a folder of their
/// own per thread, never the real one.
pub fn root() -> PathBuf {
    #[cfg(test)]
    {
        std::env::temp_dir().join(format!(
            "yap-{DIR}-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ))
    }
    #[cfg(not(test))]
    crate::config::data_dir().join(DIR)
}

fn note_dir(note_id: u64) -> PathBuf {
    root().join(note_id.to_string())
}

fn meta_path(note_id: u64, recording: u64) -> PathBuf {
    note_dir(note_id).join(format!("{recording}.json"))
}

/// One side's file: `side` "you" or "them".
fn audio_path(note_id: u64, recording: u64, side: &str) -> PathBuf {
    note_dir(note_id).join(format!("{recording}-{side}.flac"))
}

/// Digits only (it names a folder or a file: never a path).
fn id_of(name: &str) -> Option<u64> {
    (!name.is_empty() && name.len() <= 20 && name.bytes().all(|b| b.is_ascii_digit()))
        .then(|| name.parse().ok())
        .flatten()
}

/// A new recording id: now in unix ms, one more than the last if taken.
fn new_id() -> u64 {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let mut now = now_ms();
    loop {
        let last = LAST.load(Ordering::SeqCst);
        if now <= last {
            now = last + 1;
        }
        if LAST.compare_exchange(last, now, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
            return now;
        }
    }
}

/// Whether another program (another Yap) is writing this file now.
fn in_use(path: &Path) -> bool {
    #[cfg(windows)]
    {
        const ERROR_SHARING_VIOLATION: i32 = 32;
        match std::fs::OpenOptions::new().write(true).open(path) {
            Ok(_) => false,
            Err(e) => e.raw_os_error() == Some(ERROR_SHARING_VIOLATION),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}

/// The recordings this Yap is writing now: (note, recording).
fn live() -> std::sync::MutexGuard<'static, BTreeSet<(u64, u64)>> {
    static LIVE: LazyLock<Mutex<BTreeSet<(u64, u64)>>> = LazyLock::new(Default::default);
    LIVE.lock().unwrap_or_else(|p| p.into_inner())
}

/// Whether a recording's files are being written, here or by another Yap.
fn recording_now(note_id: u64, recording: u64) -> bool {
    live().contains(&(note_id, recording))
        || ["you", "them"].iter().any(|side| in_use(&audio_path(note_id, recording, side)))
}

// ---- A recording's notes about itself ----

/// `<recording>.json`: what a later start needs to know about a recording.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    pub note_id: u64,
    /// Its id (when its files were made, unix ms).
    pub recording: u64,
    /// Position 0 of its clock (unix ms): its lines are dated from it.
    pub started_ms: u64,
    /// When its last chunk was transcribed — or a crash's part recovered,
    /// or found to be nothing. `None` while it records, and after a crash.
    #[serde(default)]
    pub ended_ms: Option<u64>,
    /// Keep the audio once the meeting's lines are in ("Keep meeting audio"
    /// when it started).
    #[serde(default)]
    pub keep: bool,
    /// How far each side is transcribed into the note (samples).
    #[serde(default)]
    pub you_done: u64,
    #[serde(default)]
    pub them_done: u64,
    /// How long each side's file is (samples), once known (a clean end,
    /// or counted after a crash).
    #[serde(default)]
    pub you_len: u64,
    #[serde(default)]
    pub them_len: u64,
    /// Where hotkey dictations began in "You" (samples): their markers, for
    /// the part a crash recovery transcribes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dictated: Vec<u64>,
    /// The speech model the live transcript was made with (a registry id;
    /// empty in recordings from before 2026-10-24): what the refine after
    /// the meeting compares the best installed one with
    /// (`meeting_refine.rs`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
}

impl Meta {
    /// Where its audio ends (unix ms).
    pub fn end_ms(&self) -> u64 {
        self.started_ms + self.you_len.max(self.them_len) * 1000 / RATE
    }

    /// Samples left untranscribed (a crash's part).
    pub fn left(&self) -> u64 {
        self.you_len.saturating_sub(self.you_done).max(self.them_len.saturating_sub(self.them_done))
    }
}

fn read_meta(path: &Path) -> Option<Meta> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn save_meta(meta: &Meta) {
    let path = meta_path(meta.note_id, meta.recording);
    let written = serde_json::to_string_pretty(meta)
        .map_err(std::io::Error::other)
        .and_then(|json| crate::config::atomic_write(&path, &json));
    if let Err(e) = written {
        tracing::warn!(note_id = meta.note_id, "Meeting audio: couldn't save its notes: {e}");
    }
}

/// A note's recordings with audio, oldest first.
pub fn recordings(note_id: u64) -> Vec<Meta> {
    let Ok(entries) = std::fs::read_dir(note_dir(note_id)) else {
        return Vec::new();
    };
    let mut out: Vec<Meta> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let id = id_of(name.strip_suffix(".json")?)?;
            let meta = read_meta(&e.path())?;
            let both = ["you", "them"].iter().all(|side| audio_path(note_id, id, side).is_file());
            (both && meta.recording == id && meta.note_id == note_id).then_some(meta)
        })
        .collect();
    out.sort_by_key(|m| (m.started_ms, m.recording));
    out
}

/// The notes with audio kept.
fn notes_with_audio() -> Vec<u64> {
    let Ok(entries) = std::fs::read_dir(root()) else {
        return Vec::new();
    };
    entries.flatten().filter_map(|e| id_of(&e.file_name().to_string_lossy())).collect()
}

/// Bytes a recording's files take.
fn recording_bytes(note_id: u64, recording: u64) -> u64 {
    ["you", "them"]
        .iter()
        .filter_map(|side| std::fs::metadata(audio_path(note_id, recording, side)).ok())
        .map(|m| m.len())
        .sum::<u64>()
        + std::fs::metadata(meta_path(note_id, recording)).map(|m| m.len()).unwrap_or(0)
}

fn delete_recording(note_id: u64, recording: u64) {
    for side in ["you", "them"] {
        let _ = std::fs::remove_file(audio_path(note_id, recording, side));
    }
    let _ = std::fs::remove_file(meta_path(note_id, recording));
    // The note's folder goes with its last recording.
    let _ = std::fs::remove_dir(note_dir(note_id));
    // Nothing of it is waiting to be recovered any more.
    pending().retain(|m| !(m.note_id == note_id && m.recording == recording));
}

// ---- Writing it as it's recorded ----

/// One recording's files, written as it records (`meeting::start` makes it
/// before the first sample; the worker reports progress and finishes it).
pub struct Recorder {
    note_id: u64,
    recording: u64,
    meta: Arc<Mutex<Meta>>,
    stop: Arc<AtomicBool>,
    writer: Option<std::thread::JoinHandle<()>>,
    bufs: (AudioBuf, AudioBuf),
}

impl Recorder {
    /// Start keeping a recording of `note_id` whose sides are `you` and
    /// `them` (no audio in them yet). `None` when the files can't be made
    /// (logged; the meeting records anyway).
    pub fn start(app: &AppHandle, note_id: u64, you: &AudioBuf, them: &AudioBuf) -> Option<Recorder> {
        let cfg = crate::config::load();
        let recording = new_id();
        let made = std::fs::create_dir_all(note_dir(note_id)).and_then(|()| {
            let y = flac::Writer::create(&audio_path(note_id, recording, "you"))?;
            let t = flac::Writer::create(&audio_path(note_id, recording, "them"));
            match t {
                Ok(t) => Ok((y, t)),
                Err(e) => {
                    drop(y);
                    let _ = std::fs::remove_file(audio_path(note_id, recording, "you"));
                    Err(e)
                }
            }
        });
        let (you_w, them_w) = match made {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!(note_id, "Meeting audio: not kept for this recording: {e}");
                return None;
            }
        };
        let meta = Meta {
            note_id,
            recording,
            started_ms: recording,
            keep: cfg.meeting_keep_audio,
            model: cfg.model_size.clone(),
            ..Default::default()
        };
        save_meta(&meta);
        live().insert((note_id, recording));
        crate::meetings::meeting::keep_on_disk(you);
        crate::meetings::meeting::keep_on_disk(them);
        let meta = Arc::new(Mutex::new(meta));
        let stop = Arc::new(AtomicBool::new(false));
        let writer = {
            let (meta, stop) = (Arc::clone(&meta), Arc::clone(&stop));
            let (you, them) = (Arc::clone(you), Arc::clone(them));
            std::thread::Builder::new()
                .name("meeting-audio".into())
                .spawn(move || write_loop(you, them, you_w, them_w, meta, stop))
        };
        match writer {
            Ok(handle) => {
                tracing::info!(note_id, keep = cfg.meeting_keep_audio, "Meeting audio: kept as it's recorded");
                let _ = app.emit(EVENT_AUDIO, serde_json::json!({ "noteId": note_id }));
                Some(Recorder {
                    note_id,
                    recording,
                    meta,
                    stop,
                    writer: Some(handle),
                    bufs: (Arc::clone(you), Arc::clone(them)),
                })
            }
            Err(e) => {
                tracing::warn!(note_id, "Meeting audio: no writer ({e})");
                crate::meetings::meeting::drop_disk(you);
                crate::meetings::meeting::drop_disk(them);
                live().remove(&(note_id, recording));
                delete_recording(note_id, recording);
                None
            }
        }
    }

    fn update(&self, f: impl FnOnce(&mut Meta)) {
        let mut meta = self.meta.lock().unwrap_or_else(|p| p.into_inner());
        f(&mut meta);
        save_meta(&meta);
    }

    /// The recording's clock starts at `started_ms` (its capture is up).
    pub fn started(&self, started_ms: u64) {
        self.update(|m| m.started_ms = started_ms);
    }

    /// The note has each side's lines up to these positions.
    pub fn progress(&self, (you, them): (u64, u64)) {
        self.update(|m| {
            m.you_done = m.you_done.max(you);
            m.them_done = m.them_done.max(them);
        });
    }

    /// The writer's last frames out, its files closed.
    fn close(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.writer.take() {
            let _ = handle.join();
        }
        crate::meetings::meeting::drop_disk(&self.bufs.0);
        crate::meetings::meeting::drop_disk(&self.bufs.1);
    }

    /// The recording ended and every chunk of it is in the note: kept for
    /// its days, or deleted now ("Keep meeting audio" off) — unless `hold`:
    /// a chunk of it failed (Try again needs its audio) or the meeting may
    /// be transcribed again when it ends (`meeting_refine.rs`); then it
    /// goes once that's done ([`release`]), or a day on ([`prune`]).
    pub fn finish(mut self, hold: bool) {
        self.close();
        let mut keep = true;
        self.update(|m| {
            m.ended_ms = Some(now_ms());
            keep = m.keep;
        });
        live().remove(&(self.note_id, self.recording));
        if keep {
            tracing::info!(note_id = self.note_id, "Meeting audio: recording kept");
        } else if hold {
            tracing::info!(note_id = self.note_id, "Meeting audio: held until it's transcribed again (not kept)");
        } else {
            delete_recording(self.note_id, self.recording);
            tracing::info!(note_id = self.note_id, "Meeting audio: deleted with the meeting transcribed (not kept)");
        }
    }

    /// The recording never started (its capture failed): no files.
    pub fn discard(mut self) {
        self.close();
        live().remove(&(self.note_id, self.recording));
        delete_recording(self.note_id, self.recording);
    }
}

impl Drop for Recorder {
    /// Dropped without [`Recorder::finish`] (the worker died): the files
    /// closed as they are, the recording left for a recovery.
    fn drop(&mut self) {
        if self.writer.is_some() {
            self.close();
            live().remove(&(self.note_id, self.recording));
        }
    }
}

/// The writer thread: both sides' queues into their files every half
/// second, onto the disk every 5 s; at the stop, the rest and the lengths.
fn write_loop(
    you: AudioBuf,
    them: AudioBuf,
    you_w: flac::Writer,
    them_w: flac::Writer,
    meta: Arc<Mutex<Meta>>,
    stop: Arc<AtomicBool>,
) {
    let note_id = meta.lock().map(|m| m.note_id).unwrap_or(0);
    let mut sides = [(you, Some(you_w)), (them, Some(them_w))];
    let mut synced = Instant::now();
    loop {
        let last = stop.load(Ordering::SeqCst);
        for (buf, writer) in sides.iter_mut() {
            let Some((samples, spans)) = crate::meetings::meeting::take_for_disk(buf, last) else {
                continue;
            };
            if !spans.is_empty() {
                let mut m = meta.lock().unwrap_or_else(|p| p.into_inner());
                m.dictated.extend(spans);
                save_meta(&m);
            }
            let Some(w) = writer.as_mut() else { continue };
            if let Err(e) = w.push(&samples) {
                // A full disk, say: this side isn't kept from here on.
                tracing::warn!(note_id, "Meeting audio: couldn't write ({e}); not kept from here on");
                crate::meetings::meeting::drop_disk(buf);
                *writer = None;
            }
        }
        if synced.elapsed() >= SYNC_EVERY {
            synced = Instant::now();
            for w in sides.iter_mut().filter_map(|(_, w)| w.as_mut()) {
                let _ = w.sync();
            }
        }
        if last {
            break;
        }
        std::thread::sleep(WRITE_EVERY);
    }
    let [(_, you_w), (_, them_w)] = sides;
    let len = |w: Option<flac::Writer>| w.and_then(|w| w.finish().ok()).unwrap_or(0);
    let (you_len, them_len) = (len(you_w), len(them_w));
    let mut m = meta.lock().unwrap_or_else(|p| p.into_inner());
    m.you_len = you_len;
    m.them_len = them_len;
    save_meta(&m);
    tracing::info!(note_id, secs = you_len.max(them_len) / RATE, "Meeting audio: files closed");
}

// ---- How long it's kept ----

/// A kept recording, as the clean-up sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    pub note_id: u64,
    pub recording: u64,
    pub started_ms: u64,
    pub bytes: u64,
    /// Being written now (here or by another Yap): never touched.
    pub live: bool,
    /// Its note is gone.
    pub orphan: bool,
    /// Not to be kept ("Keep meeting audio" off), but held after its
    /// meeting for a refine or a Try again ([`Recorder::finish`]): it goes
    /// a day on at the latest ([`HELD_MS`]).
    pub held: bool,
}

/// How long audio not to be kept is held at most after its meeting.
pub const HELD_MS: u64 = 24 * 60 * 60 * 1000;

/// Which of `all` go (pure): every one whose note is gone, those older than
/// `keep_ms` (a day for audio only held, [`HELD_MS`]), then the oldest
/// while there's more than `max_bytes` — never one being recorded.
pub fn to_drop(all: &[Kept], now_ms: u64, keep_ms: u64, max_bytes: u64) -> Vec<(u64, u64)> {
    let gone = |k: &Kept| {
        let age = now_ms.saturating_sub(k.started_ms);
        k.orphan || age > keep_ms || (k.held && age > HELD_MS)
    };
    let mut out: Vec<(u64, u64)> =
        all.iter().filter(|k| !k.live && gone(k)).map(|k| (k.note_id, k.recording)).collect();
    let mut left: Vec<&Kept> = all.iter().filter(|k| k.live || !gone(k)).collect();
    left.sort_by_key(|k| (k.started_ms, k.recording));
    let mut total: u64 = left.iter().map(|k| k.bytes).sum();
    for k in left {
        if total <= max_bytes {
            break;
        }
        if !k.live {
            out.push((k.note_id, k.recording));
            total -= k.bytes;
        }
    }
    out
}

/// Every recording kept, as [`to_drop`] sees it.
fn kept_all() -> Vec<Kept> {
    let notes: BTreeSet<u64> = crate::notes::all().iter().map(|n| n.id).collect();
    // No notes at all (a store that couldn't be read is set aside, never
    // lost): no audio counts as a deleted note's.
    let known = |id: u64| notes.is_empty() || notes.contains(&id);
    let mut out = Vec::new();
    for note_id in notes_with_audio() {
        let Ok(entries) = std::fs::read_dir(note_dir(note_id)) else { continue };
        let mut ids: BTreeSet<u64> = BTreeSet::new();
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let stem = name
                .strip_suffix(".json")
                .or_else(|| name.strip_suffix("-you.flac"))
                .or_else(|| name.strip_suffix("-them.flac"));
            if let Some(id) = stem.and_then(id_of) {
                ids.insert(id);
            }
        }
        for recording in ids {
            let meta = read_meta(&meta_path(note_id, recording));
            let started_ms = meta.as_ref().map(|m| m.started_ms).unwrap_or(recording);
            out.push(Kept {
                note_id,
                recording,
                started_ms,
                bytes: recording_bytes(note_id, recording),
                live: recording_now(note_id, recording),
                orphan: !known(note_id),
                held: meta.is_some_and(|m| !m.keep && m.ended_ms.is_some()),
            });
        }
    }
    out
}

/// How long audio is kept now (ms).
fn keep_ms(cfg: &crate::config::YapConfig) -> u64 {
    cfg.meeting_audio_days.max(1) as u64 * 24 * 60 * 60 * 1000
}

/// Drop the audio past its time, a deleted note's, and leftovers.
pub fn prune(app: Option<&AppHandle>) {
    let cfg = crate::config::load();
    let all = kept_all();
    let drop = to_drop(&all, now_ms(), keep_ms(&cfg), MAX_BYTES);
    let mut notes: BTreeSet<u64> = BTreeSet::new();
    for &(note_id, recording) in &drop {
        delete_recording(note_id, recording);
        notes.insert(note_id);
    }
    if !drop.is_empty() {
        tracing::info!(dropped = drop.len(), kept = all.len() - drop.len(), "Meeting audio: old recordings deleted");
        if let Some(app) = app {
            for note_id in notes {
                changed(app, note_id);
            }
        }
    }
    // Leftovers: a half-written notes file (an hour old).
    for note_id in notes_with_audio() {
        let Ok(entries) = std::fs::read_dir(note_dir(note_id)) else { continue };
        for e in entries.flatten() {
            let old = e
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > Duration::from_secs(3_600));
            if old && e.file_name().to_string_lossy().ends_with(".tmp") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// The end-of-meeting steps are done with note `note_id`'s audio (a refine
/// finished, or none is due; a Try again done): the recordings that were
/// only held for that ("Keep meeting audio" off, [`Recorder::finish`]) go
/// now — unless a stretch of the meeting still couldn't be transcribed
/// (Try again needs it: a day at most, [`HELD_MS`]) or a job runs on it.
pub fn release(app: &AppHandle, note_id: u64) {
    if jobs().get(&note_id).is_some_and(|j| j.state == "running") {
        return;
    }
    let failed = crate::notes::get(note_id).is_some_and(|n| n.transcript.iter().any(|s| s.failed));
    if failed {
        return;
    }
    let mut n = 0;
    for m in recordings(note_id) {
        if !m.keep && m.ended_ms.is_some() && !recording_now(note_id, m.recording) {
            delete_recording(note_id, m.recording);
            n += 1;
        }
    }
    if n > 0 {
        tracing::info!(note_id, recordings = n, "Meeting audio: deleted now the meeting's transcribed (not kept)");
        changed(app, note_id);
    }
}

/// Note `note_id` was deleted: its audio goes (not a recording going on:
/// the next clean-up takes that), and a job on it stops.
pub fn forget(note_id: u64) {
    if let Some(cancel) = cancels().get(&note_id) {
        cancel.store(true, Ordering::SeqCst);
    }
    // Note ids come round again: a new note never inherits its line.
    jobs().remove(&note_id);
    crate::meetings::meeting::forget_note(note_id);
    let recordings: Vec<u64> = recordings(note_id).iter().map(|m| m.recording).collect();
    let mut left = 0;
    for recording in recordings {
        if recording_now(note_id, recording) {
            left += 1;
        } else {
            delete_recording(note_id, recording);
        }
    }
    if left == 0 {
        let _ = std::fs::remove_dir_all(note_dir(note_id));
        pending().retain(|m| m.note_id != note_id);
    }
}

/// "Delete all meeting audio": every recording but those being made now.
/// How many went.
pub fn clear() -> usize {
    let all = kept_all();
    let mut n = 0;
    for k in all.iter().filter(|k| !k.live) {
        delete_recording(k.note_id, k.recording);
        n += 1;
    }
    tracing::info!(deleted = n, "Meeting audio: cleared");
    n
}

/// Tell the windows a note's audio changed.
pub fn changed(app: &AppHandle, note_id: u64) {
    let _ = app.emit(EVENT_AUDIO, serde_json::json!({ "noteId": note_id }));
}

// ---- After a crash ----

/// Recordings a crash left: not ended, not being written, their note still
/// there, with at least [`MIN_LEFT`] of audio never transcribed. Their
/// lengths are counted (and put in their files' headers) on the way; one
/// with nothing left is closed quietly.
fn crashed() -> Vec<Meta> {
    let notes: BTreeSet<u64> = crate::notes::all().iter().map(|n| n.id).collect();
    let mut out = Vec::new();
    for note_id in notes_with_audio().into_iter().filter(|id| notes.contains(id)) {
        for mut meta in recordings(note_id) {
            if meta.ended_ms.is_some() || recording_now(note_id, meta.recording) {
                continue;
            }
            if meta.you_len == 0 && meta.them_len == 0 {
                meta.you_len = count_from(note_id, meta.recording, "you", meta.you_done);
                meta.them_len = count_from(note_id, meta.recording, "them", meta.them_done);
                for (side, len) in [("you", meta.you_len), ("them", meta.them_len)] {
                    let _ = flac::set_header_samples(&audio_path(note_id, meta.recording, side), len);
                }
                save_meta(&meta);
            }
            if meta.left() < MIN_LEFT {
                meta.ended_ms = Some(now_ms());
                save_meta(&meta);
                if !meta.keep {
                    delete_recording(note_id, meta.recording);
                }
                continue;
            }
            out.push(meta);
        }
    }
    out
}

/// A side's length: `done` and what's in its file past it.
fn count_from(note_id: u64, recording: u64, side: &str, done: u64) -> u64 {
    match flac::Reader::open(&audio_path(note_id, recording, side)) {
        Ok(mut r) => {
            r.seek(done);
            let rest = r.count_rest();
            if rest == 0 && done > 0 {
                // Short of `done` (it can't be), or nothing past it.
                done
            } else {
                done + rest
            }
        }
        Err(_) => 0,
    }
}

/// What a crash left, found at the start ([`init`]).
fn pending() -> std::sync::MutexGuard<'static, Vec<Meta>> {
    static PENDING: LazyLock<Mutex<Vec<Meta>>> = LazyLock::new(Default::default);
    PENDING.lock().unwrap_or_else(|p| p.into_inner())
}

/// At start: what a crash left (offered once, in one place — `notice.rs`
/// — and on its note), then the old audio pruned — and again every hour.
pub fn init(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new().name("meeting-audio".into()).spawn(move || {
        let found = crashed();
        if !found.is_empty() {
            tracing::info!(count = found.len(), "Meeting audio: Yap stopped during a meeting; its audio is kept");
            *pending() = found;
            for m in pending().iter() {
                changed(&app, m.note_id);
            }
            // Once the windows are up.
            std::thread::sleep(Duration::from_secs(3));
            offer(&app);
        }
        loop {
            prune(Some(&app));
            std::thread::sleep(PRUNE_EVERY);
        }
    });
}

/// "Yap closed during a meeting": through the notice policy's one place (a
/// toast while the main window has the focus, else the Yap bar, else a
/// Windows notification), with **Transcribe it**.
pub fn offer(app: &AppHandle) {
    let waiting: Vec<Meta> = pending().clone();
    let Some(first) = waiting.first() else { return };
    let (title, body) = if waiting.len() == 1 {
        let name = crate::notes::get(first.note_id).map(|n| n.title).filter(|t| !t.trim().is_empty());
        let what = match name {
            Some(t) => format!("of “{t}”"),
            None => "of your meeting".to_string(),
        };
        let len = spoken_length(first.left() / RATE);
        let was = if len.contains(' ') { "weren't" } else { "wasn't" };
        (
            "Yap closed during a meeting".to_string(),
            format!("The last {len} {what} {was} transcribed. The audio is kept: transcribe it into the note now?"),
        )
    } else {
        (
            format!("Yap closed during {} meetings", waiting.len()),
            "The end of each wasn't transcribed. The audio is kept: transcribe it into their notes now?".to_string(),
        )
    };
    crate::notices::alert::show(
        app,
        crate::notices::alert::Alert {
            kind: "meetingRecovery",
            icon: "notes",
            title,
            body,
            action: Some(crate::notices::alert::Action { id: RECOVER_ACTION, label: "Transcribe it" }),
            timeout_ms: 60_000,
        },
    );
}

/// "12 seconds", "minute" (the last minute), "5 minutes" — as the note's
/// banner says it (`meetingAudio.svelte.js` `spokenLength`).
pub fn spoken_length(secs: u64) -> String {
    match secs.max(1) {
        1 => "second".to_string(),
        s @ 2..60 => format!("{s} seconds"),
        s => match (s + 30) / 60 {
            1 => "minute".to_string(),
            m => format!("{m} minutes"),
        },
    }
}

/// The alert's **Transcribe it**: every meeting a crash left.
pub fn recover_all(app: &AppHandle) {
    let notes: BTreeSet<u64> = pending().iter().map(|m| m.note_id).collect();
    for note_id in notes {
        if let Err(e) = start_job(app, note_id, Kind::Recover) {
            tracing::info!(note_id, "Meeting audio: not recovered ({e})");
        }
    }
}

// ---- Transcribing it (after a crash, or again) ----

/// One side's file read back, for `meeting::replay`.
struct FileFeed(flac::Reader);

impl crate::meetings::meeting::Feed for FileFeed {
    fn read(&mut self, n: usize) -> Vec<f32> {
        self.0.read(n)
    }
}

fn open_sides(meta: &Meta, from: (u64, u64)) -> Result<(FileFeed, FileFeed), String> {
    let open = |side: &str, at: u64| -> Result<FileFeed, String> {
        let mut r = flac::Reader::open(&audio_path(meta.note_id, meta.recording, side))?;
        r.seek(at);
        Ok(FileFeed(r))
    };
    Ok((open("you", from.0)?, open("them", from.1)?))
}

/// Recovery, transcribing again (on demand, or the refine after a meeting),
/// or one stretch tried again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Recover,
    Again,
    /// The refine when a meeting ends (`meeting_refine.rs`): the meeting
    /// transcribed again in the background, on the best model installed.
    Refine,
    /// "Try again" on a stretch that couldn't be transcribed.
    Retry,
}

/// A note's audio being transcribed, as every window shows it
/// ([`EVENT_REDO`]).
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Redo {
    pub note_id: u64,
    pub kind: Kind,
    /// "running" | "done" | "error" | "cancelled".
    pub state: &'static str,
    /// 0–100 while running.
    pub percent: u8,
    /// The speech model it runs on (its name).
    pub model: String,
    pub error: Option<String>,
    /// Done, and the note has a summary (written from the old lines): the
    /// windows offer "Update summary" until it's written again.
    pub summary: bool,
    /// A refine waiting its turn: "summary" (the action plan is being
    /// written first) or "meeting" (another meeting is recording).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waiting: Option<&'static str>,
    /// The summary is written once this is done (the live transcript had
    /// holes: `meeting_refine.rs`, "Which comes first").
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub then_summary: bool,
}

impl Redo {
    fn running(note_id: u64, kind: Kind, model: &str) -> Self {
        Redo {
            note_id,
            kind,
            state: "running",
            percent: 0,
            model: model.to_string(),
            error: None,
            summary: false,
            waiting: None,
            then_summary: false,
        }
    }
}

/// Each note's latest job (kept after it ends, for a window opened later).
fn jobs() -> std::sync::MutexGuard<'static, HashMap<u64, Redo>> {
    static JOBS: LazyLock<Mutex<HashMap<u64, Redo>>> = LazyLock::new(Default::default);
    JOBS.lock().unwrap_or_else(|p| p.into_inner())
}

/// The running jobs' Cancel.
fn cancels() -> std::sync::MutexGuard<'static, HashMap<u64, Arc<AtomicBool>>> {
    static CANCELS: LazyLock<Mutex<HashMap<u64, Arc<AtomicBool>>>> = LazyLock::new(Default::default);
    CANCELS.lock().unwrap_or_else(|p| p.into_inner())
}

/// What a cancelled job ends with.
pub const CANCELLED: &str = "Cancelled";

fn publish(app: &AppHandle, redo: Redo) {
    jobs().insert(redo.note_id, redo.clone());
    let _ = app.emit(EVENT_REDO, redo);
}

/// Note `note_id`'s job now, if one runs.
pub fn running_job(note_id: u64) -> Option<Redo> {
    jobs().get(&note_id).filter(|j| j.state == "running").cloned()
}

/// A meeting's audio is being transcribed (any note's): the updater doesn't
/// restart under it, nor does the speech-engine timing run beside it.
pub fn any_job_running() -> bool {
    jobs().values().any(|j| j.state == "running")
}

/// A job for `note_id` starts (one per note): published as running, its
/// Cancel made. `background`: the refine's low priority.
fn begin(app: &AppHandle, first: Redo, background: bool) -> Result<crate::meetings::meeting::Ctl, String> {
    let note_id = first.note_id;
    {
        let mut all = jobs();
        if all.get(&note_id).is_some_and(|j| j.state == "running") {
            return Err("It's being transcribed already".to_string());
        }
        all.insert(note_id, first.clone());
    }
    let cancel = Arc::new(AtomicBool::new(false));
    cancels().insert(note_id, Arc::clone(&cancel));
    let _ = app.emit(EVENT_REDO, first);
    Ok(crate::meetings::meeting::Ctl { background, cancel: Some(cancel) })
}

/// A job ended: published (`done` with whether the note has a summary to
/// update, `error`, or `cancelled`), its Cancel gone, the windows told.
fn end_job(app: &AppHandle, note_id: u64, kind: Kind, model: &str, result: &Result<(), String>) {
    cancels().remove(&note_id);
    if crate::notes::get(note_id).is_none() {
        // Deleted meanwhile (its id may come round again).
        jobs().remove(&note_id);
        return;
    }
    let (state, error) = match result {
        Ok(()) => ("done", None),
        Err(e) if e == CANCELLED => ("cancelled", None),
        Err(e) => {
            tracing::warn!(note_id, ?kind, "Meeting audio: transcription failed: {e}");
            ("error", Some(e.clone()))
        }
    };
    let summary = state == "done" && crate::notes::get(note_id).is_some_and(|n| !n.enhanced_content.trim().is_empty());
    publish(app, Redo { state, percent: 100, error, summary, ..Redo::running(note_id, kind, model) });
    changed(app, note_id);
    // The windows bring the note's new lines in.
    let _ = app.emit("yap-notes-changed", ());
}

/// What runs after a job, with how it went (the steps after a refine).
type After = Box<dyn FnOnce(&AppHandle, &Result<(), String>) + Send>;

/// A job's end, published however its task ends — a panic in it too, so
/// no job stays "running" for ever (holding the updater off) and the steps
/// after a refine still run.
struct JobEnd {
    app: AppHandle,
    note_id: u64,
    kind: Kind,
    model: String,
    after: Option<After>,
    ended: bool,
}

impl JobEnd {
    fn new(app: &AppHandle, note_id: u64, kind: Kind, model: &str) -> Self {
        JobEnd { app: app.clone(), note_id, kind, model: model.to_string(), after: None, ended: false }
    }

    fn end(mut self, result: &Result<(), String>) {
        self.finish(result);
    }

    fn finish(&mut self, result: &Result<(), String>) {
        self.ended = true;
        end_job(&self.app, self.note_id, self.kind, &self.model, result);
        if let Some(after) = self.after.take() {
            after(&self.app, result);
        }
    }
}

impl Drop for JobEnd {
    fn drop(&mut self) {
        if !self.ended {
            self.finish(&Err("The transcription stopped unexpectedly".to_string()));
        }
    }
}

/// Stop note `note_id`'s job (the line's Cancel): between two chunks, the
/// transcript as it was. `false`: none runs.
pub fn cancel(note_id: u64) -> bool {
    match cancels().get(&note_id) {
        Some(c) => {
            c.store(true, Ordering::SeqCst);
            true
        }
        None => false,
    }
}

/// The note's summary is being written again: its "Update summary" offer
/// is answered (every window drops it).
pub fn summary_redone(app: &AppHandle, note_id: u64) {
    let job = {
        let mut all = jobs();
        match all.get_mut(&note_id) {
            Some(j) if j.summary => {
                j.summary = false;
                Some(j.clone())
            }
            _ => None,
        }
    };
    if let Some(job) = job {
        let _ = app.emit(EVENT_REDO, job);
    }
}

/// The speech engine in use (the pipeline's warm slot).
fn warm_slot(app: &AppHandle) -> Option<crate::dictation::pipeline::EngineSlot> {
    let state = app.state::<crate::AppState>();
    let guard = state.pipeline.lock().ok()?;
    guard.as_ref().map(|p| p.engine_slot())
}

/// Start transcribing note `note_id`'s audio in the background: the part a
/// crash left ([`Kind::Recover`]) or all of it again ([`Kind::Again`]).
pub fn start_job(app: &AppHandle, note_id: u64, kind: Kind) -> Result<(), String> {
    crate::notes::get(note_id).ok_or("Note not found")?;
    if crate::meetings::meeting::recording_note() == Some(note_id) {
        return Err("Stop the recording first".to_string());
    }
    // One taker of the speech engine at a time (`meeting::replay` also
    // waits for a recording that starts meanwhile).
    if crate::meetings::meeting::is_recording() {
        return Err("Yap is recording a meeting: try again when it ends".to_string());
    }
    let have = recordings(note_id);
    let todo: Vec<Meta> = match kind {
        Kind::Recover => pending().iter().filter(|m| m.note_id == note_id).cloned().collect(),
        _ => have.into_iter().filter(|m| !recording_now(note_id, m.recording)).collect(),
    };
    if todo.is_empty() {
        return Err(match kind {
            Kind::Recover => "Nothing of this meeting is waiting to be transcribed".to_string(),
            _ => "This meeting's audio isn't kept any more".to_string(),
        });
    }
    let model = crate::dictation::stt::model_name(&crate::config::load().model_size);
    let ctl = begin(app, Redo::running(note_id, kind, &model), false)?;
    let Some(engine_slot) = warm_slot(app) else {
        let result = Err("The speech engine isn't running".to_string());
        end_job(app, note_id, kind, &model, &result);
        return result;
    };
    let ends = JobEnd::new(app, note_id, kind, &model);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = match kind {
            Kind::Recover => recover(&app, &engine_slot, note_id, todo, &model, &ctl).await,
            _ => transcribe_again(&app, &engine_slot, note_id, todo, &model, Kind::Again, &ctl, false).await,
        };
        let recovered = kind == Kind::Recover && result.is_ok();
        ends.end(&result);
        if recovered && crate::meetings::meeting::recording_note() != Some(note_id) {
            // The end of the meeting, as a stop would have been.
            crate::meetings::meeting_end::recovered(&app, note_id);
        }
    });
    Ok(())
}

/// **The refine** of note `note_id` (`meeting_refine.rs` decides when, and
/// on which model: `model_id`): published at once as running; it waits its
/// turn — the summary job first unless `then_summary` (`meeting_refine.rs`,
/// "Which comes first"), never beside a recording, a dictation always
/// before it — then transcribes every recording again at low priority and
/// puts the lines in through the same replacement as Transcribe again
/// ([`transcribe_again`]). `after(app, result)` runs when it ends, however
/// it ended (the end-of-meeting steps, the audio released).
pub fn start_refine(
    app: &AppHandle,
    note_id: u64,
    model_id: &str,
    then_summary: bool,
    after: impl FnOnce(&AppHandle, &Result<(), String>) + Send + 'static,
) -> Result<(), String> {
    let todo: Vec<Meta> = recordings(note_id).into_iter().filter(|m| !recording_now(note_id, m.recording)).collect();
    if todo.is_empty() {
        return Err("This meeting's audio isn't kept".to_string());
    }
    let model = crate::dictation::stt::model_name(model_id);
    let first = Redo { then_summary, ..Redo::running(note_id, Kind::Refine, &model) };
    let ctl = begin(app, first.clone(), true)?;
    let mut ends = JobEnd::new(app, note_id, Kind::Refine, &model);
    ends.after = Some(Box::new(after));
    let app = app.clone();
    let model_id = model_id.to_string();
    tauri::async_runtime::spawn(async move {
        let result = refine(&app, note_id, todo, &model_id, &first, &ctl).await;
        ends.end(&result);
    });
    Ok(())
}

/// The refine's run (see [`start_refine`]).
async fn refine(app: &AppHandle, note_id: u64, todo: Vec<Meta>, model_id: &str, first: &Redo, ctl: &crate::meetings::meeting::Ctl) -> Result<(), String> {
    // Its turn: after the summary being written (unless this comes first),
    // never beside a meeting recording.
    loop {
        if ctl.cancelled() {
            return Err(CANCELLED.to_string());
        }
        let waiting = if !first.then_summary && crate::meetings::meeting_end::summary_running_for(note_id) {
            Some("summary")
        } else if crate::meetings::meeting::is_recording() {
            Some("meeting")
        } else {
            None
        };
        let Some(waiting) = waiting else { break };
        if jobs().get(&note_id).is_some_and(|j| j.waiting != Some(waiting)) {
            publish(app, Redo { waiting: Some(waiting), ..first.clone() });
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    publish(app, first.clone());
    // The model in use now: the warm engine, taken a chunk at a time.
    // Another: a slot of its own, loaded once and dropped after.
    let slot = if model_id == crate::config::load().model_size {
        warm_slot(app).ok_or("The speech engine isn't running")?
    } else {
        let slot = crate::dictation::pipeline::EngineCell::for_model(model_id, None);
        drop(crate::dictation::pipeline::lease_engine(&slot, Duration::from_secs(1)).await?);
        slot
    };
    tracing::info!(note_id, model = %model_id, recordings = todo.len(), "Meeting refine: transcribing again");
    transcribe_again(app, &slot, note_id, todo, &first.model, Kind::Refine, ctl, true).await
}

/// Progress across a job's recordings: `total` samples of both sides in
/// all; `before` of them done in earlier recordings.
#[allow(clippy::too_many_arguments)]
fn reporter(app: &AppHandle, note_id: u64, kind: Kind, model: &str, before: u64, total: u64, from: (u64, u64), then_summary: bool) -> impl Fn(u64, u64) + Send + Sync {
    let app = app.clone();
    let model = model.to_string();
    let last = AtomicU64::new(u64::MAX);
    move |you: u64, them: u64| {
        let done = before + you.saturating_sub(from.0) + them.saturating_sub(from.1);
        let percent = (done * 100 / total.max(1)).min(99);
        if last.swap(percent, Ordering::SeqCst) != percent {
            publish(&app, Redo { percent: percent as u8, then_summary, ..Redo::running(note_id, kind, &model) });
        }
    }
}

/// The part a crash left of each recording in `todo`, into the note where
/// it belongs (by time), then the recording closed.
async fn recover(app: &AppHandle, engine: &crate::dictation::pipeline::EngineSlot, note_id: u64, todo: Vec<Meta>, model: &str, ctl: &crate::meetings::meeting::Ctl) -> Result<(), String> {
    let total: u64 = todo.iter().map(|m| m.you_len.saturating_sub(m.you_done) + m.them_len.saturating_sub(m.them_done)).sum();
    let mut before = 0;
    let mut lines = 0;
    for mut meta in todo {
        let from = (meta.you_done, meta.them_done);
        let (mut you, mut them) = open_sides(&meta, from)?;
        let progress = reporter(app, note_id, Kind::Recover, model, before, total, from, false);
        let out = crate::meetings::meeting::replay(engine, note_id, &mut you, &mut them, meta.started_ms, from, &meta.dictated, &progress, ctl).await;
        if out.cancelled {
            return Err(CANCELLED.to_string());
        }
        before += meta.you_len.saturating_sub(from.0) + meta.them_len.saturating_sub(from.1);
        lines += out.segments.iter().filter(|s| s.is_talk()).count();
        crate::notes::insert_transcript(note_id, &out.segments)?;
        meta.you_done = out.you_done;
        meta.them_done = out.them_done;
        meta.ended_ms = Some(now_ms());
        save_meta(&meta);
        pending().retain(|m| m.recording != meta.recording);
        if !meta.keep {
            delete_recording(note_id, meta.recording);
        }
    }
    tracing::info!(note_id, lines, "Meeting audio: the part a crash left is transcribed");
    Ok(())
}

/// Every recording in `todo` transcribed again, its lines put in place of
/// the old ones (`redo.rs`: notes, names on matching lines and the summary
/// kept; the digests dropped, to be written again from the new lines) —
/// "Transcribe again", and the refine after a meeting (`kind`), which also
/// `check`s the new lines say what the old ones did (`meeting_refine::
/// agrees`: a model that didn't understand the meeting's language changes
/// nothing). Never while the note's summary is being written (it reads
/// the lines and their digests): the new lines wait for it.
#[allow(clippy::too_many_arguments)]
async fn transcribe_again(app: &AppHandle, engine: &crate::dictation::pipeline::EngineSlot, note_id: u64, todo: Vec<Meta>, model: &str, kind: Kind, ctl: &crate::meetings::meeting::Ctl, check: bool) -> Result<(), String> {
    let total: u64 = todo.iter().map(|m| m.you_len + m.them_len).sum();
    let then_summary = jobs().get(&note_id).is_some_and(|j| j.then_summary);
    let mut before = 0;
    let mut redone = Vec::new();
    for meta in &todo {
        let (mut you, mut them) = open_sides(meta, (0, 0))?;
        let progress = reporter(app, note_id, kind, model, before, total, (0, 0), then_summary);
        // A crash's part never had its markers: they come with it.
        let extra: Vec<u64> = if meta.ended_ms.is_none() {
            meta.dictated.iter().copied().filter(|&d| d >= meta.you_done).collect()
        } else {
            Vec::new()
        };
        let out = crate::meetings::meeting::replay(engine, note_id, &mut you, &mut them, meta.started_ms, (0, 0), &extra, &progress, ctl).await;
        if out.cancelled {
            return Err(CANCELLED.to_string());
        }
        before += meta.you_len + meta.them_len;
        let len = meta.you_len.max(meta.them_len).max(out.you_done).max(out.them_done);
        redone.push(redo::Redone { started_ms: meta.started_ms, end_ms: meta.started_ms + len * 1000 / RATE, lines: out.segments });
    }
    if check {
        let old = crate::notes::get(note_id).ok_or("Note not found")?;
        crate::meetings::meeting_refine::agrees(&old.transcript, &redone)?;
    }
    while crate::meetings::meeting_end::summary_running_for(note_id) {
        if ctl.cancelled() {
            return Err(CANCELLED.to_string());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let (replaced, added, voiced) = crate::notes::modify(note_id, |note| {
        let r = redo::rebuild(&note.transcript, &note.breaks, &redone);
        note.transcript = r.transcript;
        note.breaks = r.breaks;
        note.digests.clear();
        Ok(((r.replaced, r.added, r.voiced), true))
    })?;
    // A crash's part is in now too.
    for mut meta in todo.into_iter().filter(|m| m.ended_ms.is_none()) {
        meta.ended_ms = Some(now_ms());
        save_meta(&meta);
        pending().retain(|m| m.recording != meta.recording);
    }
    tracing::info!(note_id, ?kind, replaced, added, voiced, "Meeting audio: transcribed again");
    Ok(())
}

// ---- Try again: one stretch that couldn't be transcribed ----

/// One side's file from a position, for at most `left` samples.
struct RangeFeed {
    reader: flac::Reader,
    left: usize,
}

impl crate::meetings::meeting::Feed for RangeFeed {
    fn read(&mut self, n: usize) -> Vec<f32> {
        let n = n.min(self.left);
        let out = if n == 0 { Vec::new() } else { self.reader.read(n) };
        self.left -= out.len().min(self.left);
        out
    }
}

/// **Try again** on a failed chunk's marker (`source`'s stretch starting
/// `start_ms` on the clock of the recording the line dated `ts` is in):
/// that stretch of that side transcribed from its audio on disk, on the
/// speech model in use now, its lines put in the marker's place
/// (`redo::fill`). Not while a meeting records (the live chunks have the
/// engine).
pub fn start_retry(app: &AppHandle, note_id: u64, source: &str, ts: u64, start_ms: u64) -> Result<(), String> {
    let note = crate::notes::get(note_id).ok_or("Note not found")?;
    if crate::meetings::meeting::is_recording() {
        return Err("Yap is recording a meeting: try again when it ends".to_string());
    }
    let marker = note
        .transcript
        .iter()
        .find(|s| s.failed && s.source == source && s.start_ms == Some(start_ms))
        .cloned()
        .ok_or("That part is transcribed already")?;
    let end_ms = marker.end_ms.unwrap_or(start_ms).max(start_ms);
    let meta = recording_at(note_id, ts).ok_or("This meeting's audio isn't kept any more")?;
    let model = crate::dictation::stt::model_name(&crate::config::load().model_size);
    let ctl = begin(app, Redo::running(note_id, Kind::Retry, &model), false)?;
    let Some(engine_slot) = warm_slot(app) else {
        let result = Err("The speech engine isn't running".to_string());
        end_job(app, note_id, Kind::Retry, &model, &result);
        return result;
    };
    let ends = JobEnd::new(app, note_id, Kind::Retry, &model);
    let app = app.clone();
    let source = source.to_string();
    tauri::async_runtime::spawn(async move {
        let result = retry(&app, &engine_slot, &meta, &source, (start_ms, end_ms), &model, &ctl).await;
        ends.end(&result);
        release(&app, note_id);
    });
    Ok(())
}

async fn retry(app: &AppHandle, engine: &crate::dictation::pipeline::EngineSlot, meta: &Meta, source: &str, (start_ms, end_ms): (u64, u64), model: &str, ctl: &crate::meetings::meeting::Ctl) -> Result<(), String> {
    let note_id = meta.note_id;
    let (from, to) = (start_ms * RATE / 1000, end_ms * RATE / 1000);
    let mut reader = flac::Reader::open(&audio_path(note_id, meta.recording, source))?;
    reader.seek(from);
    let mut feed = RangeFeed { reader, left: (to - from) as usize };
    let mut none = crate::meetings::meeting::NoFeed;
    let (you, them): (&mut dyn crate::meetings::meeting::Feed, &mut dyn crate::meetings::meeting::Feed) =
        if source == "you" { (&mut feed, &mut none) } else { (&mut none, &mut feed) };
    let at = if source == "you" { (from, 0) } else { (0, from) };
    let progress = reporter(app, note_id, Kind::Retry, model, 0, to - from, at, false);
    let out = crate::meetings::meeting::replay(engine, note_id, you, them, meta.started_ms, at, &[], &progress, ctl).await;
    if out.cancelled {
        return Err(CANCELLED.to_string());
    }
    let lines: Vec<_> = out.segments.into_iter().filter(|s| s.source == source && !s.dictated).collect();
    let still = lines.iter().any(|s| s.failed);
    let n = lines.len();
    crate::notes::modify(note_id, |note| {
        let at = note
            .transcript
            .iter()
            .position(|s| s.failed && s.source == source && s.start_ms == Some(start_ms))
            .ok_or("That part is transcribed already")?;
        redo::fill(note, at, lines);
        Ok(((), true))
    })?;
    tracing::info!(note_id, lines = n, still, "Meeting audio: a stretch tried again");
    if still {
        Err("Yap still couldn't transcribe it. Check the speech model in Settings → Speech-to-Text.".to_string())
    } else {
        Ok(())
    }
}

// ---- Playing a line ----

/// The recording a line dated `ts` (unix seconds) belongs to.
fn recording_at(note_id: u64, ts: u64) -> Option<Meta> {
    let ms = ts * 1000;
    recordings(note_id).into_iter().rev().find(|m| {
        let end = if m.you_len.max(m.them_len) > 0 { m.end_ms() } else { now_ms() };
        ms + 999 >= m.started_ms && ms <= end + 1000
    })
}

/// Both sides of `[start_ms, end_ms)` on its recording's clock (the line
/// dated `ts`), mixed, as a 16 kHz WAV: what the line's ▶ plays.
pub fn clip(note_id: u64, ts: u64, start_ms: u64, end_ms: u64) -> Result<Vec<u8>, String> {
    let meta = recording_at(note_id, ts).ok_or("This meeting's audio isn't kept any more")?;
    let from_ms = start_ms.saturating_sub(CLIP_PAD_MS);
    let to_ms = (end_ms + CLIP_PAD_MS).min(from_ms + CLIP_MAX_MS).max(from_ms + 100);
    let (from, n) = (from_ms * RATE / 1000, ((to_ms - from_ms) * RATE / 1000) as usize);
    let side = |name: &str| -> Vec<f32> {
        match flac::Reader::open(&audio_path(note_id, meta.recording, name)) {
            Ok(mut r) => {
                r.seek(from);
                r.read(n)
            }
            Err(_) => Vec::new(),
        }
    };
    let (you, them) = (side("you"), side("them"));
    if you.is_empty() && them.is_empty() {
        return Err("This line's audio isn't there".to_string());
    }
    let len = you.len().max(them.len());
    let at = |s: &[f32], i: usize| s.get(i).copied().unwrap_or(0.0);
    let mixed: Vec<f32> = (0..len).map(|i| (at(&you, i) + at(&them, i)).clamp(-1.0, 1.0)).collect();
    Ok(crate::dictation::recent_audio::wav(&mixed))
}

// ---- What Settings and the notes show ----

/// Settings → Meetings: how much is kept.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub meetings: usize,
    pub recordings: usize,
    pub bytes: u64,
}

pub fn status() -> Status {
    let all = kept_all();
    let meetings: BTreeSet<u64> = all.iter().map(|k| k.note_id).collect();
    Status { meetings: meetings.len(), recordings: all.len(), bytes: all.iter().map(|k| k.bytes).sum() }
}

/// A note's audio, for its menus, its lines' ▶ and the crash banner.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteAudio {
    /// Recordings with audio, how long in all (seconds), how big.
    pub recordings: usize,
    pub secs: u64,
    pub bytes: u64,
    /// One is being written now.
    pub recording_now: bool,
    /// What a crash left, waiting to be transcribed (seconds).
    pub recoverable_secs: u64,
    /// When the oldest goes (unix seconds), at today's setting.
    pub until: Option<u64>,
    /// Its latest transcription job.
    pub job: Option<Redo>,
    /// The speech model "Transcribe again" would use now (its name).
    pub model: String,
}

pub fn note_audio(note_id: u64) -> NoteAudio {
    let cfg = crate::config::load();
    let all = recordings(note_id);
    let waiting: u64 = pending().iter().filter(|m| m.note_id == note_id).map(|m| m.left() / RATE).sum();
    let until = all.iter().map(|m| m.started_ms).min().map(|s| (s + keep_ms(&cfg)) / 1000);
    NoteAudio {
        recordings: all.len(),
        secs: all.iter().map(|m| m.you_len.max(m.them_len) / RATE).sum(),
        bytes: all.iter().map(|m| recording_bytes(note_id, m.recording)).sum(),
        recording_now: all.iter().any(|m| live().contains(&(note_id, m.recording))),
        recoverable_secs: waiting,
        until,
        job: jobs().get(&note_id).cloned(),
        model: crate::dictation::stt::model_name(&cfg.model_size),
    }
}

// ---- Commands ----

/// Settings → Meetings: how much meeting audio is kept.
#[tauri::command]
pub fn meeting_audio_status() -> Status {
    status()
}

/// Settings → Meetings → Delete all meeting audio.
#[tauri::command]
pub fn meeting_audio_clear(app: AppHandle) -> usize {
    let n = clear();
    let _ = app.emit(EVENT_AUDIO, serde_json::json!({ "noteId": null }));
    n
}

/// A note's kept audio (see [`NoteAudio`]).
#[tauri::command]
pub fn meeting_audio_note(note_id: u64) -> NoteAudio {
    note_audio(note_id)
}

/// A transcript line's ▶: its stretch of the meeting, as a WAV.
#[tauri::command]
pub fn meeting_audio_clip(note_id: u64, ts: u64, start_ms: u64, end_ms: u64) -> Result<tauri::ipc::Response, String> {
    clip(note_id, ts, start_ms, end_ms).map(tauri::ipc::Response::new)
}

/// "Transcribe again" (the Notes view's and the notepad's menus): progress
/// and the result come as [`EVENT_REDO`].
#[tauri::command]
pub fn meeting_transcribe_again(app: AppHandle, note_id: u64) -> Result<(), String> {
    start_job(&app, note_id, Kind::Again)
}

/// A crashed meeting's "Transcribe it" (its note's banner, or the alert).
#[tauri::command]
pub fn meeting_recover(app: AppHandle, note_id: u64) -> Result<(), String> {
    start_job(&app, note_id, Kind::Recover)
}

/// "Try again" on a stretch that couldn't be transcribed (its marker:
/// side, the line's `ts`, `startMs`): progress and the result come as
/// [`EVENT_REDO`] (kind `retry`).
#[tauri::command]
pub fn meeting_retry_chunk(app: AppHandle, note_id: u64, source: String, ts: u64, start_ms: u64) -> Result<(), String> {
    start_retry(&app, note_id, &source, ts, start_ms)
}

/// Cancel a note's transcription job (the refine's line): the transcript
/// stays as it was. `false`: none was running.
#[tauri::command]
pub fn meeting_job_cancel(note_id: u64) -> bool {
    cancel(note_id)
}

/// Test mode only: offer what a crash left again, now (the start's offer
/// came before the suite could watch).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_meeting_recovery_offer(app: AppHandle) -> Result<usize, String> {
    if !crate::shell::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    offer(&app);
    Ok(pending().len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(note_id: u64, recording: u64, days_ago: u64, bytes: u64) -> Kept {
        let now = 100 * 86_400_000;
        Kept { note_id, recording, started_ms: now - days_ago * 86_400_000, bytes, live: false, orphan: false, held: false }
    }

    #[test]
    fn audio_goes_after_its_days_with_its_note_or_over_the_cap() {
        let now = 100 * 86_400_000;
        let keep = 14 * 86_400_000;
        let all = [k(1, 10, 20, 5), k(1, 11, 2, 5), k(2, 20, 1, 5)];
        assert_eq!(to_drop(&all, now, keep, u64::MAX), vec![(1, 10)]);
        // A deleted note's audio goes, whatever its age.
        let orphan = [Kept { orphan: true, ..k(3, 30, 0, 5) }];
        assert_eq!(to_drop(&orphan, now, keep, u64::MAX), vec![(3, 30)]);
        // Over the cap: the oldest first, until it fits.
        let big = [k(1, 10, 3, 60), k(2, 20, 2, 60), k(3, 30, 1, 60)];
        assert_eq!(to_drop(&big, now, keep, 130), vec![(1, 10)]);
        // A recording going on is never touched, however old or big.
        let live = [Kept { live: true, orphan: true, ..k(4, 40, 30, 500) }, k(5, 50, 1, 10)];
        assert_eq!(to_drop(&live, now, keep, 100), vec![(5, 50)]);
        // Audio not to be kept, held for a refine: a day at most.
        let held = [Kept { held: true, ..k(6, 60, 2, 5) }, Kept { held: true, started_ms: now - HELD_MS / 2, ..k(7, 70, 0, 5) }];
        assert_eq!(to_drop(&held, now, keep, u64::MAX), vec![(6, 60)]);
    }

    #[test]
    fn what_a_crash_left_is_said_in_words() {
        assert_eq!(spoken_length(0), "second");
        assert_eq!(spoken_length(5), "5 seconds");
        assert_eq!(spoken_length(59), "59 seconds");
        assert_eq!(spoken_length(60), "minute");
        assert_eq!(spoken_length(89), "minute");
        assert_eq!(spoken_length(90), "2 minutes");
        assert_eq!(spoken_length(12 * 60 + 10), "12 minutes");
    }

    #[test]
    fn ids_are_numbers_only() {
        assert_eq!(id_of("1761140000123"), Some(1_761_140_000_123));
        assert_eq!(id_of("..\\config"), None);
        assert_eq!(id_of(""), None);
        assert_eq!(id_of("12a"), None);
        assert!(new_id() < new_id());
    }

    fn tone(secs: f32) -> Vec<f32> {
        (0..(secs * RATE as f32) as usize).map(|i| 0.3 * (i as f32 * 0.07).sin()).collect()
    }

    #[test]
    fn a_recording_is_written_as_it_comes_and_found_after_a_crash() {
        let _ = std::fs::remove_dir_all(root());
        let note_id = 42;
        let recording = new_id();
        std::fs::create_dir_all(note_dir(note_id)).unwrap();
        let mut you = flac::Writer::create(&audio_path(note_id, recording, "you")).unwrap();
        let mut them = flac::Writer::create(&audio_path(note_id, recording, "them")).unwrap();
        you.push(&tone(10.0)).unwrap();
        them.push(&tone(10.0)).unwrap();
        // 4 s of it transcribed when Yap stopped.
        let meta = Meta { note_id, recording, started_ms: recording, keep: true, you_done: 4 * RATE, them_done: 4 * RATE, ..Default::default() };
        save_meta(&meta);
        drop((you, them));
        // Found with the meeting: its length counted from its files.
        let found = recordings(note_id);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].ended_ms, None);
        let len = count_from(note_id, recording, "you", meta.you_done);
        assert_eq!(len, (10 * RATE as usize / flac::BLOCK * flac::BLOCK) as u64);
        let counted = Meta { you_len: len, them_len: len, ..meta.clone() };
        assert_eq!(counted.left(), len - 4 * RATE);
        assert!(counted.left() >= MIN_LEFT);
        // A clip of it: both sides of the stretch, padded.
        let clip_bytes = clip(note_id, recording / 1000 + 1, 1_000, 2_000).unwrap();
        let pcm = crate::voice::tts::parse_wav(&clip_bytes).unwrap();
        assert_eq!(pcm.samples.len() as u64, (1_000 + 2 * CLIP_PAD_MS) * RATE / 1000);
        assert!(pcm.samples.iter().any(|s| s.abs() > 0.3), "the two sides add up");
        // Deleting the note's audio takes the folder.
        forget(note_id);
        assert!(!note_dir(note_id).exists());
        assert!(recordings(note_id).is_empty());
        let _ = std::fs::remove_dir_all(root());
    }

    /// **How long a refine takes** on a 30-minute meeting (both sides read
    /// back from FLAC, cut, transcribed at low priority, then the
    /// replacement and its check): `cargo test --release --features engines
    /// --lib -- --ignored refine_a_30_minute_meeting --nocapture`, with
    /// `YAP_REFINE_YOU` / `YAP_REFINE_THEM` (speech WAVs, taking turns until
    /// each side is `YAP_REFINE_MINUTES` long, 30), `YAP_REFINE_DATA` (a data
    /// folder with `models/`, only read; Yap's own by default),
    /// `YAP_REFINE_MODEL` (Parakeet V3) and `YAP_REFINE_DEVICE` (gpu | cpu).
    /// Without the `engines` feature it times all but the model (the stub
    /// answers at once).
    #[test]
    #[ignore]
    fn refine_a_30_minute_meeting() {
        use std::time::Instant;
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        let minutes: u64 = var("YAP_REFINE_MINUTES").and_then(|v| v.parse().ok()).unwrap_or(30);
        let clip = |k: &str| var(k).map(|p| crate::dictation::media::decode_to_16k_mono(Path::new(&p)).unwrap()).unwrap_or_else(|| tone(8.0));
        let (you_clip, them_clip) = (clip("YAP_REFINE_YOU"), clip("YAP_REFINE_THEM"));
        let n = (minutes * 60 * RATE) as usize;
        let gap = vec![0.0f32; RATE as usize];
        let (mut you, mut them) = (Vec::with_capacity(n), Vec::with_capacity(n));
        while you.len() < n {
            them.extend_from_slice(&them_clip);
            you.resize(them.len(), 0.0);
            you.extend_from_slice(&gap);
            them.extend_from_slice(&gap);
            you.extend_from_slice(&you_clip);
            them.resize(you.len(), 0.0);
            you.extend_from_slice(&gap);
            them.extend_from_slice(&gap);
        }
        you.truncate(n);
        them.truncate(n);

        let _ = std::fs::remove_dir_all(root());
        let (note_id, recording) = (7, new_id());
        std::fs::create_dir_all(note_dir(note_id)).unwrap();
        let t = Instant::now();
        let mut lens = (0, 0);
        for (side, audio, len) in [("you", &you, &mut lens.0), ("them", &them, &mut lens.1)] {
            let mut w = flac::Writer::create(&audio_path(note_id, recording, side)).unwrap();
            for piece in audio.chunks(RATE as usize / 2) {
                w.push(piece).unwrap();
            }
            *len = w.finish().unwrap();
        }
        let write_ms = t.elapsed().as_millis();
        let bytes = recording_bytes(note_id, recording);
        let meta = Meta { note_id, recording, started_ms: recording, you_len: lens.0, them_len: lens.1, ended_ms: Some(recording), ..Default::default() };

        let data = var("YAP_REFINE_DATA").map(std::path::PathBuf::from).unwrap_or_else(|| dirs::data_dir().unwrap().join("yap"));
        let model = var("YAP_REFINE_MODEL").unwrap_or_else(|| "parakeet-tdt-0.6b-v3".into());
        let device = if var("YAP_REFINE_DEVICE").as_deref() == Some("cpu") { crate::dictation::stt::Device::Cpu } else { crate::dictation::stt::Device::Gpu };
        let t = Instant::now();
        let engine = crate::dictation::stt::create_stt_engine_on(&data, &model, device).unwrap();
        let load_ms = t.elapsed().as_millis();
        let slot = crate::dictation::pipeline::EngineCell::for_model(&model, Some(engine));

        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().unwrap();
        let (mut y, mut th) = open_sides(&meta, (0, 0)).unwrap();
        let ctl = crate::meetings::meeting::Ctl { background: true, cancel: None };
        let chunks = AtomicU64::new(0);
        let progress = |_: u64, _: u64| {
            chunks.fetch_add(1, Ordering::SeqCst);
        };
        let t = Instant::now();
        let out = rt.block_on(crate::meetings::meeting::replay(&slot, note_id, &mut y, &mut th, meta.started_ms, (0, 0), &[], &progress, &ctl));
        let replay_ms = t.elapsed().as_millis();

        // The replacement over a live transcript of the same length.
        let redone = vec![redo::Redone { started_ms: meta.started_ms, end_ms: meta.end_ms(), lines: out.segments.clone() }];
        let t = Instant::now();
        let agreed = crate::meetings::meeting_refine::agrees(&out.segments, &redone).is_ok();
        let r = redo::rebuild(&out.segments, &[out.segments.len()], &redone);
        let rebuild_ms = t.elapsed().as_millis();

        let audio_secs = (lens.0 + lens.1) as f64 / RATE as f64;
        println!(
            "refine of a {minutes}-minute meeting ({model} on {}): FLAC written in {write_ms} ms ({:.1} MB), model loaded in {load_ms} ms, \
             {} steps, {} lines ({} failed) in {:.1} s ({:.0}x real time over both sides' {:.0} s), replacement + check in {rebuild_ms} ms (agrees: {agreed}, {} lines)",
            device.as_str(),
            bytes as f64 / 1e6,
            chunks.load(Ordering::SeqCst),
            out.segments.len(),
            out.segments.iter().filter(|s| s.failed).count(),
            replay_ms as f64 / 1000.0,
            audio_secs * 1000.0 / replay_ms.max(1) as f64,
            audio_secs,
            r.transcript.len(),
        );
        assert!(!out.cancelled);
        let _ = std::fs::remove_dir_all(root());
    }

    #[test]
    fn a_meta_reads_back_and_says_what_is_left() {
        let m = Meta {
            note_id: 1,
            recording: 5,
            started_ms: 1_000,
            you_done: 3 * RATE,
            them_done: 2 * RATE,
            you_len: 10 * RATE,
            them_len: 9 * RATE,
            dictated: vec![RATE * 5],
            ..Default::default()
        };
        let back: Meta = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(back, m);
        assert_eq!(m.left(), 7 * RATE);
        assert_eq!(m.end_ms(), 11_000);
        // An older file without the newer fields still reads.
        let old: Meta = serde_json::from_str(r#"{"noteId":1,"recording":5,"startedMs":1000}"#).unwrap();
        assert_eq!((old.you_done, old.ended_ms, old.keep), (0, None, false));
    }
}

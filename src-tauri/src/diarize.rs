//! Telling the voices on a meeting's call side apart, on this PC
//! ("Tell speakers apart in meetings", Settings → Speech-to-Text; beta).
//!
//! The engine is NVIDIA's **Nemotron-3-Diarization** (q8_0 GGUF, OpenMDW-1.1)
//! on **NeMo-Speech.cpp 0.2.0** (Apache-2.0), chosen by the spike in
//! yap-private `docs/diarization-spike.md`: fed the "Them" audio as it
//! arrives with −60 dBFS dither added, on the CPU's 30 s preset it costs
//! ~2.4% of real time on 4 threads and labels trail the audio by ~10 s; the
//! final labels exist the moment the recording stops, and no audio is kept.
//!
//! **A child process, `yap.exe diarize`** (main.rs, `child.rs`): the runtime
//! is young (a ggml assert aborts its process), brings its own ggml next to
//! the whisper.cpp ggml linked into `yap.exe`, and runs at below-normal
//! priority with its own memory. A crash costs only the labels: the meeting
//! records on, and the engine is started again (≤ [`MAX_RESTARTS`]) as a new
//! `stream` whose speaker numbers start over. The wire is in `protocol.rs`.
//!
//! **During a meeting** (`meeting.rs`): `meeting::start` opens a [`Session`]
//! when the switch is on and the model installed, and gives the "Them"
//! buffer its [`Tee`], which forwards every sample the recorder appends —
//! device audio and the silence it fills in — so the engine's clock is the
//! session clock. When the recording stops, the worker calls
//! [`Session::finish`] before it says the recording ended: the final
//! [`Timeline`] goes to every [`on_segments`] handler (and [`timeline`]) and
//! only then do the summary and the title start.
//!
//! **For the speaker map** (`speakers::labels`, which puts the voices on
//! the "Them" lines): [`on_segments`] is called every few seconds while a
//! recording runs (each time with the whole timeline so far, tail still
//! moving) and once with `done: true`. Times are ms on that recording's
//! session clock (the "Them" sample position / 16); a voice is `(stream,
//! speaker)`; [`Session::id`] is the `Timeline::session` its timelines carry.
//!
//! Models and the runtime: `registry.rs` (download → size + SHA-256 →
//! unpack into `<data>/models/diarization/`). Logs carry counts and times,
//! never audio.

mod child;
pub mod protocol;
pub(crate) mod registry;
mod timeline;

use std::io::{BufRead, BufReader, BufWriter, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Emitter};

use protocol::Message;
pub use registry::DEFAULT_MODEL;
use registry::EngineFiles;
#[allow(unused_imports)] // the speaker map's types
pub use timeline::{Segment, Timeline};
use timeline::Runs;

/// `yap.exe diarize …`: the child's entry (main.rs). Returns its exit code.
pub fn run_child() -> i32 {
    child::run(std::env::args_os().skip(2))
}

/// Times a crashed engine is started again in one recording.
pub const MAX_RESTARTS: u32 = 2;
/// How long `finish` waits for the final labels (normally well under 1 s).
const FINISH_TIMEOUT: Duration = Duration::from_secs(20);
/// Audio queued for a stuck engine beyond this (5 min) is sent as silence:
/// the clock stays right, memory stays bounded.
const QUEUE_CAP: usize = 5 * 60 * 16_000;
/// Live updates: every this much audio (once the labels moved on).
const EVERY_MS: u64 = 5_000;

/// OpenMP threads for the engine: 4 (the spike's measurements), fewer on a
/// small PC.
fn threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| (n.get() / 2).clamp(1, 4))
        .unwrap_or(2)
}

/// Start `yap.exe diarize` on `files`, piped, below normal priority, with no
/// console window.
fn spawn(files: &EngineFiles, every_ms: u64) -> Result<Child, String> {
    let exe = std::env::current_exe().map_err(|e| format!("can't find yap.exe: {e}"))?;
    let mut cmd = Command::new(exe);
    cmd.arg("diarize")
        .arg("--runtime")
        .arg(&files.runtime_dir)
        .arg("--model")
        .arg(&files.model_path)
        .arg("--preset")
        .arg(files.preset)
        .arg("--every-ms")
        .arg(every_ms.to_string())
        .env("OMP_NUM_THREADS", threads().to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
        cmd.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
    }
    cmd.spawn().map_err(|e| format!("couldn't start the speaker engine: {e}"))
}

/// The child's stderr (its own and the runtime's progress lines) into the
/// log at debug level.
fn forward_stderr(child: &mut Child) {
    if let Some(err) = child.stderr.take() {
        let _ = std::thread::Builder::new().name("diarize-log".into()).spawn(move || {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                let line = line.trim();
                if !line.is_empty() {
                    let cut: String = line.chars().take(300).collect();
                    tracing::debug!("diarize child: {cut}");
                }
            }
        });
    }
}

// ---- Handlers and the latest timeline ----

type Handler = Arc<dyn Fn(&Timeline) + Send + Sync>;
static HANDLERS: Mutex<Vec<Handler>> = Mutex::new(Vec::new());
static LATEST: Mutex<Option<Timeline>> = Mutex::new(None);
static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
/// Sessions running now (a model in use can't be deleted).
static RUNNING: AtomicUsize = AtomicUsize::new(0);

/// Call `f` with every new timeline of a recording's call side: every few
/// seconds while it records, and once more with `done: true` after it
/// stopped, before the summary starts (the recording waits for `f`). On a
/// diarization thread: keep it short (no network, no waiting on windows).
pub fn on_segments(f: impl Fn(&Timeline) + Send + Sync + 'static) {
    HANDLERS.lock().unwrap_or_else(|p| p.into_inner()).push(Arc::new(f));
}

/// The latest timeline of note `note_id`'s most recent recording, live or
/// done (`None` if it had none since Yap started).
pub fn timeline(note_id: u64) -> Option<Timeline> {
    LATEST
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .filter(|t| t.note_id == note_id)
        .cloned()
}

fn publish(tl: &Timeline) {
    *LATEST.lock().unwrap_or_else(|p| p.into_inner()) = Some(tl.clone());
    let handlers: Vec<Handler> = HANDLERS.lock().unwrap_or_else(|p| p.into_inner()).clone();
    for h in handlers {
        h(tl);
    }
}

// ---- A recording's session ----

enum Feed {
    Audio(Vec<f32>),
    Silence(u32),
    /// The recording stopped (or never started): finish, or with `cancel`
    /// set, just stop. Sent by the session, since the recorder's buffers
    /// (and their tees) can outlive it by a moment.
    End,
}

impl Feed {
    fn samples(&self) -> u64 {
        match self {
            Feed::Audio(a) => a.len() as u64,
            Feed::Silence(n) => *n as u64,
            Feed::End => 0,
        }
    }
}

/// The "Them" buffer's hook (`meeting.rs`): hands every appended sample to
/// the session. Never blocks (an audio callback calls it).
#[derive(Clone)]
pub struct Tee {
    tx: Sender<Feed>,
    queued: Arc<AtomicUsize>,
}

impl Tee {
    /// Audio the recorder appended.
    pub fn audio(&self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        if self.queued.load(Ordering::Relaxed) > QUEUE_CAP {
            let _ = self.tx.send(Feed::Silence(samples.len() as u32));
            return;
        }
        self.queued.fetch_add(samples.len(), Ordering::Relaxed);
        if self.tx.send(Feed::Audio(samples.to_vec())).is_err() {
            self.queued.fetch_sub(samples.len(), Ordering::Relaxed);
        }
    }

    /// Silence the recorder filled in (a stalled device, `keep_in_step`).
    pub fn silence(&self, n: usize) {
        if n > 0 {
            let _ = self.tx.send(Feed::Silence(n.min(u32::MAX as usize) as u32));
        }
    }
}

/// What the feeding thread and the stdout readers share.
struct State {
    note_id: u64,
    session: u64,
    runs: Runs,
    audio_ms: u64,
    restarts: u32,
    error: Option<String>,
}

impl State {
    fn timeline(&self, done: bool) -> Timeline {
        let (segments, labelled_ms, gaps) = self.runs.merged();
        Timeline {
            note_id: self.note_id,
            session: self.session,
            segments,
            audio_ms: self.audio_ms,
            labelled_ms: if done && self.error.is_none() { self.audio_ms.max(labelled_ms) } else { labelled_ms },
            done,
            restarts: self.restarts,
            gaps,
            error: self.error.clone(),
        }
    }
}

/// Speaker diarization for one meeting recording: started with it, fed by
/// its [`Tee`], finished with it.
pub struct Session {
    note_id: u64,
    id: u64,
    tx: Option<Sender<Feed>>,
    queued: Arc<AtomicUsize>,
    done_rx: Receiver<Timeline>,
    cancel: Arc<AtomicBool>,
    finished: bool,
}

impl Session {
    /// A session for the recording starting in `note_id`, if "Tell speakers
    /// apart in meetings" is on and its model is installed.
    pub fn for_meeting(note_id: u64) -> Option<Session> {
        let cfg = crate::config::load();
        if !cfg.meeting_diarization {
            return None;
        }
        let Some(files) = registry::engine_files(&cfg.diarization_model) else {
            tracing::warn!("Diarization: on, but its model isn't installed; recording without speaker labels");
            return None;
        };
        match Session::start(note_id, files) {
            Ok(s) => Some(s),
            Err(e) => {
                tracing::warn!("Diarization: couldn't start: {e}");
                None
            }
        }
    }

    fn start(note_id: u64, files: EngineFiles) -> Result<Session, String> {
        let session = NEXT_SESSION.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        let queued = Arc::new(AtomicUsize::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let state = Arc::new(Mutex::new(State {
            note_id,
            session,
            runs: Runs::default(),
            audio_ms: 0,
            restarts: 0,
            error: None,
        }));
        let feeder = Feeder {
            files,
            state,
            rx,
            queued: Arc::clone(&queued),
            cancel: Arc::clone(&cancel),
            done_tx,
        };
        RUNNING.fetch_add(1, Ordering::SeqCst);
        std::thread::Builder::new()
            .name("diarize-feed".into())
            .spawn(move || {
                feeder.run();
                RUNNING.fetch_sub(1, Ordering::SeqCst);
            })
            .map_err(|e| {
                RUNNING.fetch_sub(1, Ordering::SeqCst);
                format!("couldn't start the diarization thread: {e}")
            })?;
        Ok(Session {
            note_id,
            id: session,
            tx: Some(tx),
            queued,
            done_rx,
            cancel,
            finished: false,
        })
    }

    /// The `Timeline::session` of this recording's timelines.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// The hook for the recording's "Them" buffer.
    pub fn tee(&self) -> Option<Tee> {
        self.tx.as_ref().map(|tx| Tee {
            tx: tx.clone(),
            queued: Arc::clone(&self.queued),
        })
    }

    /// The recording stopped: let the engine label the tail, hand the final
    /// timeline to the handlers and return it. Blocks (≤ ~20 s, normally
    /// well under one); call it off the async runtime. The recording's
    /// [`Tee`]s must be gone or idle by now: audio sent after this is lost.
    pub fn finish(mut self) -> Option<Timeline> {
        self.finished = true;
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(Feed::End);
        }
        match self.done_rx.recv_timeout(FINISH_TIMEOUT + Duration::from_secs(5)) {
            Ok(tl) => Some(tl),
            Err(_) => {
                tracing::warn!(note_id = self.note_id, "Diarization: no final labels in time");
                timeline(self.note_id)
            }
        }
    }
}

impl Drop for Session {
    /// Dropped without `finish` (the recording failed to start): stop the
    /// engine, tell nobody.
    fn drop(&mut self) {
        if !self.finished {
            self.cancel.store(true, Ordering::SeqCst);
            if let Some(tx) = self.tx.take() {
                let _ = tx.send(Feed::End);
            }
        }
    }
}

/// One `yap.exe diarize` process of a session.
struct Running {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stream: u32,
    /// Its stdout reader says when it's done (true: it sent the final list).
    ended: Receiver<bool>,
}

/// The session's thread: feeds the child(ren), restarts a crashed one,
/// finishes.
struct Feeder {
    files: EngineFiles,
    state: Arc<Mutex<State>>,
    rx: Receiver<Feed>,
    queued: Arc<AtomicUsize>,
    cancel: Arc<AtomicBool>,
    done_tx: Sender<Timeline>,
}

impl Feeder {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn start_child(&self, stream: u32, offset_ms: u64) -> Option<Running> {
        let note_id = self.lock().note_id;
        let mut child = match spawn(&self.files, EVERY_MS) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Diarization: {e}");
                self.lock().error = Some(e);
                return None;
            }
        };
        forward_stderr(&mut child);
        let (stdin, stdout) = (child.stdin.take(), child.stdout.take());
        let (Some(stdin), Some(stdout)) = (stdin, stdout) else {
            let _ = child.kill();
            return None;
        };
        self.lock().runs.begin(stream, offset_ms);
        tracing::info!(note_id, stream, offset_ms, engine = self.files.name, "Diarization: engine started");
        let (ended_tx, ended) = mpsc::channel();
        let state = Arc::clone(&self.state);
        let _ = std::thread::Builder::new().name("diarize-read".into()).spawn(move || {
            let saw_final = read_messages(stdout, stream, &state);
            let _ = ended_tx.send(saw_final);
        });
        Some(Running {
            child,
            stdin: BufWriter::with_capacity(1 << 16, stdin),
            stream,
            ended,
        })
    }

    /// The engine died: start the next one at `sent` samples, if allowed.
    fn restart(&self, dead: Running, sent: u64) -> Option<Running> {
        let Running { mut child, stream, .. } = dead;
        let _ = child.kill();
        let _ = child.wait();
        let restarts = {
            let mut s = self.lock();
            s.restarts += 1;
            s.restarts
        };
        if restarts > MAX_RESTARTS {
            tracing::warn!("Diarization: the engine stopped {restarts} times; no more speaker labels this recording");
            self.lock().error = Some("The speaker engine kept stopping, so later lines have no speakers.".into());
            return None;
        }
        tracing::warn!(stream, "Diarization: the engine stopped; starting it again");
        self.start_child(stream + 1, sent * 1000 / 16_000)
    }

    fn write(running: &mut Running, feed: &Feed) -> std::io::Result<()> {
        match feed {
            Feed::Audio(a) => protocol::write_audio(&mut running.stdin, a),
            Feed::Silence(n) => protocol::write_silence(&mut running.stdin, *n),
            Feed::End => Ok(()),
        }
    }

    fn run(self) {
        let mut sent: u64 = 0;
        let mut running = self.start_child(0, 0);
        let mut ended = false;
        while !ended {
            // This feed and whatever else is waiting, then one flush. (All
            // senders gone without an End counts as one.)
            let Ok(first) = self.rx.recv() else {
                break;
            };
            let mut batch = vec![first];
            while let Ok(more) = self.rx.try_recv() {
                batch.push(more);
            }
            if self.cancel.load(Ordering::SeqCst) {
                break;
            }
            for feed in &batch {
                if matches!(feed, Feed::End) {
                    ended = true;
                    break;
                }
                if let Feed::Audio(a) = feed {
                    self.queued.fetch_sub(a.len(), Ordering::Relaxed);
                }
                if let Some(r) = running.as_mut() {
                    if Self::write(r, feed).is_err() {
                        let dead = running.take().expect("running");
                        running = self.restart(dead, sent);
                        if let Some(r) = running.as_mut() {
                            let _ = Self::write(r, feed);
                        }
                    }
                }
                sent += feed.samples();
            }
            if let Some(r) = running.as_mut() {
                if r.stdin.flush().is_err() {
                    let dead = running.take().expect("running");
                    running = self.restart(dead, sent);
                }
            }
            self.lock().audio_ms = sent * 1000 / 16_000;
        }

        if self.cancel.load(Ordering::SeqCst) {
            if let Some(mut r) = running {
                let _ = r.child.kill();
                let _ = r.child.wait();
            }
            tracing::info!("Diarization: cancelled");
            return;
        }

        // The recording stopped: close stdin, wait for the final list.
        self.lock().audio_ms = sent * 1000 / 16_000;
        if let Some(r) = running {
            let Running { mut child, stdin, ended, .. } = r;
            drop(stdin);
            let saw_final = match ended.recv_timeout(FINISH_TIMEOUT) {
                Ok(saw) => saw,
                Err(RecvTimeoutError::Timeout) => {
                    tracing::warn!("Diarization: the engine didn't finish in time");
                    let _ = child.kill();
                    false
                }
                Err(RecvTimeoutError::Disconnected) => false,
            };
            let _ = child.wait();
            if !saw_final {
                let mut s = self.lock();
                if s.error.is_none() {
                    s.error = Some("The speaker engine stopped before labelling the end of the meeting.".into());
                }
            }
        }
        let tl = self.lock().timeline(true);
        let voices = tl.voices().len();
        tracing::info!(
            note_id = tl.note_id,
            voices,
            segments = tl.segments.len(),
            audio_s = tl.audio_ms / 1000,
            restarts = tl.restarts,
            failed = tl.error.is_some(),
            "Diarization: done"
        );
        publish(&tl);
        let _ = self.done_tx.send(tl);
    }
}

/// Read one child's stdout into the session's runs, publishing each live
/// update. True when it sent its final list.
fn read_messages(stdout: impl std::io::Read, stream: u32, state: &Mutex<State>) -> bool {
    let lock = || state.lock().unwrap_or_else(|p| p.into_inner());
    for line in BufReader::new(stdout).lines().map_while(Result::ok) {
        match protocol::parse_line(&line) {
            Some(Message::Ready { protocol: p, speakers, load_ms, runtime, .. }) => {
                if p != protocol::PROTOCOL {
                    tracing::warn!(protocol = p, "Diarization: the engine speaks another protocol");
                    lock().error = Some("The speaker engine doesn't match this Yap.".into());
                    return false;
                }
                tracing::info!(stream, speakers, load_ms, runtime = %runtime, "Diarization: engine ready");
            }
            Some(Message::Segments { is_final, segments, labelled_ms, audio_ms, compute_ms, .. }) => {
                let tl = {
                    let mut s = lock();
                    s.runs.update(stream, segments, labelled_ms);
                    (!is_final).then(|| s.timeline(false))
                };
                tracing::debug!(stream, labelled_ms, audio_ms, compute_ms, is_final, "Diarization: labels");
                match tl {
                    Some(tl) => publish(&tl),
                    None => return true,
                }
            }
            Some(Message::Error { message }) => {
                tracing::warn!(stream, "Diarization: engine error: {message}");
                lock().error = Some(message);
            }
            None => {}
        }
    }
    false
}

// ---- Commands (Settings → Speech-to-Text → Tell speakers apart) ----

static DOWNLOADING: Mutex<Option<registry::Progress>> = Mutex::new(None);

/// The models (installed or not), a download going on, and whether a
/// recording is using one.
#[tauri::command]
pub fn diarization_status() -> serde_json::Value {
    serde_json::json!({
        "models": registry::list(&registry::root()),
        "downloading": DOWNLOADING.lock().unwrap_or_else(|p| p.into_inner()).clone(),
        "inUse": RUNNING.load(Ordering::SeqCst) > 0,
        "defaultModel": DEFAULT_MODEL,
    })
}

/// Download model `id` (and its runtime): `diarization-download-progress`
/// while it goes, `diarization-changed` at the end, either way.
#[tauri::command]
pub async fn diarization_download(app: AppHandle, id: String) -> Result<(), String> {
    {
        let mut d = DOWNLOADING.lock().unwrap_or_else(|p| p.into_inner());
        if d.is_some() {
            return Err("A speaker model is already downloading".into());
        }
        *d = Some(registry::Progress {
            id: id.clone(),
            stage: "runtime",
            percent: 0,
            downloaded_bytes: 0,
            total_bytes: 0,
        });
    }
    let root = registry::root();
    let progress_app = app.clone();
    let on_progress = move |p: registry::Progress| {
        *DOWNLOADING.lock().unwrap_or_else(|p| p.into_inner()) = Some(p.clone());
        let _ = progress_app.emit("diarization-download-progress", &p);
    };
    let result = registry::install(&root, &id, &on_progress).await;
    *DOWNLOADING.lock().unwrap_or_else(|p| p.into_inner()) = None;
    if let Err(e) = &result {
        tracing::warn!(model = %id, "Diarization: download failed: {e}");
    }
    let _ = app.emit("diarization-changed", ());
    result
}

/// Delete model `id` (its runtime too when nothing else uses it).
#[tauri::command]
pub fn diarization_delete(app: AppHandle, id: String) -> Result<(), String> {
    if RUNNING.load(Ordering::SeqCst) > 0 {
        return Err("A meeting is using the speaker model. Stop the recording first.".into());
    }
    if DOWNLOADING.lock().unwrap_or_else(|p| p.into_inner()).is_some() {
        return Err("Wait for the download to finish first.".into());
    }
    registry::delete(&registry::root(), &id)?;
    let _ = app.emit("diarization-changed", ());
    Ok(())
}

/// `diarize_wav`'s answer (segments as a timeline's, stream 0).
#[cfg(debug_assertions)]
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WavResult {
    pub segments: Vec<Segment>,
    pub speakers: usize,
    pub audio_ms: u64,
    pub compute_ms: u64,
    pub load_ms: u64,
    pub wall_ms: u64,
    /// Engine time / audio length.
    pub rtf: f64,
    pub lag_ms_median: Option<u64>,
    pub lag_ms_max: Option<u64>,
    pub runtime: String,
    pub threads: usize,
}

/// Run the real engine over a WAV (debug builds: the suites and checks
/// against reference turns). The audio is pushed a second at a time as fast
/// as the engine takes it, as the spike's streaming test did; `speakers`
/// caps the voices afterwards (the engine itself takes no count).
#[cfg(debug_assertions)]
#[tauri::command]
pub async fn diarize_wav(path: String, speakers: Option<u8>) -> Result<WavResult, String> {
    tauri::async_runtime::spawn_blocking(move || run_wav(std::path::Path::new(&path), speakers))
        .await
        .map_err(|e| format!("diarize task failed: {e}"))?
}

/// The latest timeline of note `noteId`'s recording (debug builds: the e2e
/// suite checks a meeting's labels).
#[cfg(debug_assertions)]
#[tauri::command]
pub fn diarization_timeline(note_id: u64) -> Option<Timeline> {
    timeline(note_id)
}

/// The `Timeline::session` of the e2e suite's scripted timelines.
#[cfg(debug_assertions)]
pub const E2E_SESSION: u64 = u64::MAX;

/// One stretch of a scripted timeline.
#[cfg(debug_assertions)]
#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptedSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub speaker: u8,
    #[serde(default)]
    pub stream: u32,
}

/// A scripted final timeline, published when its recording stops.
#[cfg(debug_assertions)]
static SCRIPTED_DONE: Mutex<Option<Timeline>> = Mutex::new(None);

/// Test mode: a timeline for the recording now running, as the engine would
/// report it (`speakers::labels` takes it for a recording with no engine),
/// so the e2e suite tests the labels without a model. A live one goes to
/// the [`on_segments`] handlers now; a `done` one when the recording stops,
/// where the engine's final labels come (`meeting.rs` →
/// [`finish_scripted`]). `error`: the engine stopped early.
#[cfg(debug_assertions)]
#[tauri::command]
pub fn e2e_diarize_timeline(
    segments: Vec<ScriptedSegment>,
    labelled_ms: u64,
    done: bool,
    error: Option<String>,
) -> Result<(), String> {
    if !crate::e2e::active() {
        return Err("Only in e2e test mode".to_string());
    }
    let note_id = crate::meeting::recording_note().ok_or("No meeting is being recorded")?;
    let mut segments: Vec<Segment> = segments
        .into_iter()
        .filter(|s| s.end_ms > s.start_ms)
        .map(|s| Segment {
            start_ms: s.start_ms,
            end_ms: s.end_ms,
            speaker: s.speaker,
            stream: s.stream,
        })
        .collect();
    segments.sort_by_key(|s| (s.start_ms, s.end_ms));
    let audio_ms = segments.iter().map(|s| s.end_ms).max().unwrap_or(0).max(labelled_ms);
    let tl = Timeline {
        note_id,
        session: E2E_SESSION,
        segments,
        audio_ms,
        labelled_ms: if done && error.is_none() { audio_ms } else { labelled_ms },
        done,
        restarts: 0,
        gaps: Vec::new(),
        error,
    };
    if done {
        *SCRIPTED_DONE.lock().unwrap_or_else(|p| p.into_inner()) = Some(tl);
    } else {
        publish(&tl);
    }
    Ok(())
}

/// Test mode: the recording of `note_id` stopped with no engine: the final
/// timeline a test staged for it, where the engine's would come.
#[cfg(debug_assertions)]
pub fn finish_scripted(note_id: u64) {
    let mut staged = SCRIPTED_DONE.lock().unwrap_or_else(|p| p.into_inner());
    if staged.as_ref().is_some_and(|tl| tl.note_id == note_id) {
        let tl = staged.take().expect("staged");
        drop(staged);
        publish(&tl);
    }
}

#[cfg(debug_assertions)]
fn run_wav(path: &std::path::Path, speakers: Option<u8>) -> Result<WavResult, String> {
    let cfg = crate::config::load();
    let files = registry::engine_files(&cfg.diarization_model).ok_or("No speaker model is installed")?;
    let audio = crate::media::decode_to_16k_mono(path)?;
    let started = std::time::Instant::now();
    let mut child = spawn(&files, 0)?;
    forward_stderr(&mut child);
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let reader = std::thread::spawn(move || {
        BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
            .filter_map(|l| protocol::parse_line(&l))
            .collect::<Vec<Message>>()
    });
    {
        let mut stdin = BufWriter::new(child.stdin.take().ok_or("no stdin")?);
        for second in audio.chunks(16_000) {
            if protocol::write_audio(&mut stdin, second).and_then(|_| stdin.flush()).is_err() {
                break; // the child died: its messages say why
            }
        }
    }
    let messages = reader.join().map_err(|_| "reader panicked")?;
    let _ = child.wait();
    let wall_ms = started.elapsed().as_millis() as u64;
    let mut load_ms = 0;
    let mut runtime = String::new();
    for m in &messages {
        match m {
            Message::Ready { load_ms: l, runtime: r, .. } => {
                load_ms = *l;
                runtime = r.clone();
            }
            Message::Error { message } => return Err(message.clone()),
            Message::Segments { is_final: true, segments, audio_ms, compute_ms, lag_ms_median, lag_ms_max, .. } => {
                let segments = match speakers {
                    Some(n) => timeline::cap_speakers(segments, n),
                    None => segments.clone(),
                };
                let mut ids: Vec<u8> = segments.iter().map(|s| s.speaker).collect();
                ids.sort_unstable();
                ids.dedup();
                let segments = segments
                    .iter()
                    .map(|s| Segment {
                        start_ms: s.start_ms,
                        end_ms: s.end_ms,
                        speaker: s.speaker,
                        stream: 0,
                    })
                    .collect();
                return Ok(WavResult {
                    speakers: ids.len(),
                    segments,
                    audio_ms: *audio_ms,
                    compute_ms: *compute_ms,
                    load_ms,
                    wall_ms,
                    rtf: *compute_ms as f64 / (*audio_ms).max(1) as f64,
                    lag_ms_median: *lag_ms_median,
                    lag_ms_max: *lag_ms_max,
                    runtime,
                    threads: threads(),
                });
            }
            _ => {}
        }
    }
    Err("The speaker engine stopped without an answer".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::RawSegment;

    fn state() -> Mutex<State> {
        Mutex::new(State {
            note_id: 7,
            session: 1,
            runs: Runs::default(),
            audio_ms: 0,
            restarts: 0,
            error: None,
        })
    }

    #[test]
    fn a_childs_stdout_becomes_the_sessions_timeline() {
        let st = state();
        st.lock().unwrap().runs.begin(0, 0);
        let lines = [
            r#"{"type":"ready","protocol":1,"speakers":8,"frame_ms":10.0,"runtime":"0.2.0","load_ms":900}"#,
            "ggml: some library chatter",
            r#"{"type":"segments","final":false,"audio_ms":35000,"labelled_ms":30400,"compute_ms":700,"segments":[{"start_ms":0,"end_ms":12000,"speaker":1}]}"#,
            r#"{"type":"segments","final":true,"audio_ms":41000,"labelled_ms":41000,"compute_ms":900,"segments":[{"start_ms":0,"end_ms":12000,"speaker":1},{"start_ms":12500,"end_ms":40000,"speaker":2}]}"#,
        ]
        .join("\n");
        assert!(read_messages(lines.as_bytes(), 0, &st), "saw the final list");
        let mut s = st.lock().unwrap();
        s.audio_ms = 41_000;
        let tl = s.timeline(true);
        assert_eq!(tl.note_id, 7);
        assert_eq!(tl.segments.len(), 2);
        assert_eq!(tl.labelled_ms, 41_000);
        assert!(tl.done && tl.error.is_none());
        assert_eq!(tl.voices(), vec![(0, 1), (0, 2)]);
    }

    #[test]
    fn a_child_that_dies_or_mismatches_says_so() {
        let st = state();
        st.lock().unwrap().runs.begin(0, 0);
        let crashed = r#"{"type":"segments","final":false,"audio_ms":35000,"labelled_ms":30400,"compute_ms":700,"segments":[]}"#;
        assert!(!read_messages(crashed.as_bytes(), 0, &st), "EOF without a final list");
        let err = r#"{"type":"error","message":"loading the model failed (3): bad gguf"}"#;
        assert!(!read_messages(err.as_bytes(), 0, &st));
        assert_eq!(st.lock().unwrap().error.as_deref(), Some("loading the model failed (3): bad gguf"));

        let st = state();
        let other = r#"{"type":"ready","protocol":99,"speakers":8,"frame_ms":10.0,"runtime":"9","load_ms":1}"#;
        assert!(!read_messages(other.as_bytes(), 0, &st));
        assert!(st.lock().unwrap().error.is_some());
    }

    #[test]
    fn the_tee_queues_audio_and_never_blocks() {
        let (tx, rx) = mpsc::channel();
        let queued = Arc::new(AtomicUsize::new(0));
        let tee = Tee { tx, queued: Arc::clone(&queued) };
        tee.audio(&[0.5; 160]);
        tee.silence(320);
        tee.audio(&[]);
        assert_eq!(queued.load(Ordering::Relaxed), 160);
        // A stuck engine: past the cap, audio goes as silence (the clock
        // stays right, memory doesn't grow).
        queued.store(QUEUE_CAP + 1, Ordering::Relaxed);
        tee.audio(&[0.5; 160]);
        let got: Vec<u64> = rx.try_iter().map(|f| f.samples()).collect();
        assert_eq!(got, vec![160, 320, 160]);
        // A session that's gone: nothing breaks.
        drop(rx);
        tee.audio(&[0.5; 16]);
        tee.silence(16);
    }

    #[test]
    fn a_failed_timeline_keeps_its_labels_and_the_reason() {
        let st = state();
        {
            let mut s = st.lock().unwrap();
            s.runs.begin(0, 0);
            s.runs.update(0, vec![RawSegment { start_ms: 0, end_ms: 5_000, speaker: 1 }], 5_000);
            s.audio_ms = 60_000;
            s.error = Some("stopped".into());
        }
        let tl = st.lock().unwrap().timeline(true);
        assert_eq!(tl.labelled_ms, 5_000, "only what was labelled");
        assert_eq!(tl.segments.len(), 1);
        assert_eq!(tl.error.as_deref(), Some("stopped"));
    }

    #[test]
    fn engine_threads_stay_small() {
        let t = threads();
        assert!((1..=4).contains(&t));
    }
}

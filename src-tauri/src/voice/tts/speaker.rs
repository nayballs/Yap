//! The speaker: one utterance at a time, queued by sentence on a worker
//! thread (the first sentence plays while the next is made), never over
//! a dictation or a meeting; a read-aloud paced a sentence ahead, its
//! position and pause; a cloud voice that fails handing over to the
//! local one. Every change is `yap-tts-state`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Serialize;

#[cfg(debug_assertions)]
use super::playback::volume;
use super::playback::{rodio_opener, set_volume, sim_opener, Opener, Playback, Tag};
use super::text::{inline_text, sentences, speakable_blocks, utterances};
#[cfg(debug_assertions)]
use super::{spoken, Spoken};
use super::{cloud, kokoro, marks, normalize};
use super::{voicing, voicing_for, CloudPlan, Engine, Pcm, Voicing, AUDIBLE_TAIL, MAX_CHARS};

/// What's being said: `yap-tts-state`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsState {
    pub speaking: bool,
    /// The answer it's reading (the page's id for it; [`PREVIEW_ID`] for
    /// Settings' sample). On `speaking: false`, the one that stopped.
    pub message_id: Option<String>,
    /// Why it stopped early, when it failed (no voice, no output device…).
    pub error: Option<String>,
}

/// One utterance to say.
pub struct Job {
    pub message_id: Option<String>,
    pub sentences: Vec<String>,
    pub engine: Arc<dyn Engine>,
    pub voice: String,
    pub rate: f32,
    /// For a cloud voice: what happens when it fails ([`Fallback`]).
    pub fallback: Option<Fallback>,
    /// A read-aloud (`read_aloud.rs`): `sentences` are `track`'s from
    /// `first` on, as shown — each is said as [`said_text`] writes it, made
    /// only once the one before it plays, and its words' times kept on the
    /// track for the reading card.
    pub track: Option<Arc<Track>>,
    pub first: usize,
    /// A streamed answer (the Voice Assistant's, `assistant.rs`): more
    /// sentences come through it as the answer is written, after
    /// `sentences` (which may be empty), until it's closed.
    pub feed: Option<Arc<Feed>>,
}

/// How long the voice waits for a streamed answer's next sentence before it
/// gives up ([`Feed`]; Chat's own timeout is 2 minutes a call).
const FEED_PATIENCE: Duration = Duration::from_secs(180);

/// **Sentences handed to the voice as an answer is written** (the Voice
/// Assistant speaks a streamed answer from its first sentence, `stream.rs`):
/// [`speak_streamed`] starts the voice with none, [`Feed::push`] adds
/// sentences, [`Feed::close`] says there are no more. The voice waits for
/// the next sentence while the feed is open; a stop (■, a dictation's
/// barge-in, a newer answer) ends it, and pushing after that says so.
#[derive(Default)]
pub struct Feed {
    state: Mutex<FeedState>,
    /// The speaker and its job it feeds (none: not started).
    voice: std::sync::OnceLock<(std::sync::Weak<Speaker>, u64)>,
}

#[derive(Default)]
struct FeedState {
    queued: Vec<String>,
    closed: bool,
    /// Everything pushed (what test mode records).
    all: Vec<String>,
    #[cfg(debug_assertions)]
    record: Option<Spoken>,
}

impl Feed {
    /// More sentences (as `utterances` writes them). `false`: the voice has
    /// stopped, so they won't be said.
    pub fn push(&self, sentences: Vec<String>) -> bool {
        if !self.live() {
            return false;
        }
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if s.closed {
            return false;
        }
        s.all.extend(sentences.iter().cloned());
        s.queued.extend(sentences);
        true
    }

    /// No more sentences: the voice ends after the last one.
    pub fn close(&self) {
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        if s.closed {
            return;
        }
        s.closed = true;
        #[cfg(debug_assertions)]
        if let Some(mut rec) = s.record.take() {
            if !s.all.is_empty() {
                rec.text = s.all.join(" ");
                rec.sentences = s.all.clone();
                spoken().push(rec);
            }
        }
    }

    /// The voice it feeds is still going.
    pub fn live(&self) -> bool {
        self.voice.get().is_some_and(|(s, g)| s.upgrade().is_some_and(|s| s.live(*g)))
    }

    /// It feeds `speaker`'s job `generation` (started by [`Speaker::start`]).
    fn attach(&self, speaker: &Arc<Speaker>, generation: u64) {
        let _ = self.voice.set((Arc::downgrade(speaker), generation));
    }

    /// What's queued now, and whether it's closed.
    fn take(&self) -> (Vec<String>, bool) {
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        (std::mem::take(&mut s.queued), s.closed)
    }
}

/// What's said for a sentence shown on the reading card: its markdown, links
/// and emoji out ([`inline_text`]), written for the ear (`normalize.rs`).
pub fn said_text(shown: &str) -> String {
    normalize::for_speech(&inline_text(shown))
}

/// A read-aloud's sentences as shown (`read_aloud.rs` splits the text), and
/// for each one made so far, when its words start in its audio — the
/// speaker writes, the reading card reads.
pub struct Track {
    shown: Vec<String>,
    made: Mutex<std::collections::HashMap<usize, Made>>,
    /// Sentences made so far (a long text is made a sentence ahead of the
    /// voice, never all at once).
    made_count: std::sync::atomic::AtomicUsize,
}

/// One sentence, made: when its shown words start (word index, from the
/// start of its audio), how that was found, and by which engine.
#[derive(Debug, Clone)]
pub struct Made {
    pub starts: Vec<(usize, Duration)>,
    pub timing: marks::Timing,
    pub engine: &'static str,
}

impl Track {
    pub fn new(shown: Vec<String>) -> Arc<Track> {
        Arc::new(Track { shown, made: Mutex::new(std::collections::HashMap::new()), made_count: Default::default() })
    }

    pub fn len(&self) -> usize {
        self.shown.len()
    }

    pub fn is_empty(&self) -> bool {
        self.shown.is_empty()
    }

    pub fn shown(&self, i: usize) -> Option<&str> {
        self.shown.get(i).map(String::as_str)
    }

    pub fn all(&self) -> &[String] {
        &self.shown
    }

    /// Sentence `i`, if it has been made.
    pub fn made(&self, i: usize) -> Option<Made> {
        self.made.lock().unwrap_or_else(|p| p.into_inner()).get(&i).cloned()
    }

    pub fn made_count(&self) -> usize {
        self.made_count.load(Ordering::SeqCst)
    }

    /// Sentence `i` was made: `said` (what the engine was given) in `heard`
    /// (its audio, interleaved, `format` = rate and channels; empty if it
    /// didn't come in one format), `length` long. Its words' starts come
    /// from the engine's `times` if it gave them (and they fit), else from
    /// the audio ([`marks::from_audio`]), mapped back to the shown words.
    #[allow(clippy::too_many_arguments)]
    fn record(&self, i: usize, said: &str, times: Option<Vec<marks::CharTime>>, heard: &[f32], format: (u32, u16), length: Duration, engine: &'static str) {
        let Some(shown) = self.shown.get(i) else { return };
        let said_spans = marks::words(said);
        let (said_starts, timing) = match times.and_then(|t| marks::from_chars(&said_spans, &t)) {
            Some(t) => (t, marks::Timing::Engine),
            None if format.0 > 0 && !heard.is_empty() => marks::from_audio(said, &said_spans, heard, format.0, format.1),
            None => (marks::estimate(said, &said_spans, Duration::ZERO, length), marks::Timing::Estimate),
        };
        let shown_spans = marks::words(shown);
        let map = marks::align(shown, &shown_spans, said, &said_spans);
        let starts = marks::shown_starts(&map, &said_starts);
        let mut made = self.made.lock().unwrap_or_else(|p| p.into_inner());
        // Only the sentences round the voice are worth keeping.
        made.retain(|&k, _| k + 16 > i && k < i + 16);
        made.insert(i, Made { starts, timing, engine });
        drop(made);
        self.made_count.fetch_add(1, Ordering::SeqCst);
    }
}

/// When the engine fails mid-answer: `local` reads the rest — the sentence
/// that failed included — or, with none, the answer stops there. `notify`
/// is told once (the error, the sentences left, whether `local` took
/// over): it says so to the person, so the state carries no error.
pub struct Fallback {
    pub local: Option<(Arc<dyn Engine>, String)>,
    pub notify: Notify,
}

/// [`Fallback::notify`]: (the error, the sentences left, whether the local
/// voice took over).
pub type Notify = Box<dyn FnOnce(&str, &[String], bool) + Send>;

struct Current {
    generation: u64,
    message_id: Option<String>,
    /// Set once the worker has opened the output.
    play: Option<Arc<dyn Playback>>,
    /// To cut a sentence being made short on a stop.
    engine: Arc<dyn Engine>,
}

type Emit = Box<dyn Fn(&TtsState) + Send + Sync>;
type Busy = Box<dyn Fn() -> bool + Send + Sync>;

/// One voice at a time: [`Speaker::speak`] replaces whatever is being said,
/// [`Speaker::stop`] silences it at once. `busy`: something is listening (a
/// dictation recording) — checked before every sentence and while one plays.
pub struct Speaker {
    generation: AtomicU64,
    current: Mutex<Option<Current>>,
    last_audible: Mutex<Option<Instant>>,
    emit: Emit,
    busy: Busy,
}

impl Speaker {
    pub fn new(emit: Emit, busy: Busy) -> Arc<Speaker> {
        Arc::new(Speaker {
            generation: AtomicU64::new(0),
            current: Mutex::new(None),
            last_audible: Mutex::new(None),
            emit,
            busy,
        })
    }

    fn current(&self) -> std::sync::MutexGuard<'_, Option<Current>> {
        self.current.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn heard_now(&self) {
        *self.last_audible.lock().unwrap_or_else(|p| p.into_inner()) = Some(Instant::now());
    }

    /// What's being said now.
    pub fn state(&self) -> TtsState {
        let cur = self.current();
        TtsState { speaking: cur.is_some(), message_id: cur.as_ref().and_then(|c| c.message_id.clone()), error: None }
    }

    /// Say `job` (whatever was being said stops first). It's "speaking" at
    /// once; the worker opens the output (`open`), then makes each sentence
    /// and queues it while the one before plays.
    pub fn speak(self: &Arc<Self>, job: Job, open: Opener) -> Result<(), String> {
        self.start(job, open).map(|_| ())
    }

    /// [`Speaker::speak`], returning the job's generation (a [`Feed`]
    /// follows it).
    fn start(self: &Arc<Self>, job: Job, open: Opener) -> Result<u64, String> {
        if job.sentences.is_empty() && job.feed.is_none() {
            return Err("Nothing to read aloud".into());
        }
        if (self.busy)() {
            return Err("Yap is listening to a dictation".into());
        }
        self.stop();
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *self.current() =
            Some(Current { generation, message_id: job.message_id.clone(), play: None, engine: Arc::clone(&job.engine) });
        (self.emit)(&TtsState { speaking: true, message_id: job.message_id.clone(), error: None });
        let me = Arc::clone(self);
        std::thread::Builder::new()
            .name("tts-speaker".into())
            .spawn(move || me.run(generation, job, open))
            .map(|_| generation)
            .map_err(|e| {
                self.finish(generation, None, Some(format!("Couldn't start speaking: {e}")));
                "Couldn't start speaking".to_string()
            })
    }

    fn live(&self, generation: u64) -> bool {
        self.generation.load(Ordering::SeqCst) == generation
    }

    fn run(self: Arc<Self>, generation: u64, mut job: Job, open: Opener) {
        let opened = match open() {
            Ok(o) => o,
            Err(e) => return self.finish(generation, job.message_id, Some(e)),
        };
        {
            let mut cur = self.current();
            match cur.as_mut() {
                Some(c) if c.generation == generation => c.play = Some(Arc::clone(&opened.play)),
                // Stopped (or replaced) while the output opened.
                _ => {
                    opened.play.stop();
                    return;
                }
            }
        }
        let mut error = None;
        let mut synth = job.engine.open(&job.voice, job.rate);
        let mut engine_id = job.engine.id();
        let mut i = 0;
        // A streamed answer's last sentence (or its start): a feed that
        // never closes (its answer lost) ends after [`FEED_PATIENCE`].
        let mut fed_at = Instant::now();
        loop {
            // A streamed answer: its next sentences as they're written.
            if i >= job.sentences.len() {
                let Some(feed) = job.feed.clone() else { break };
                let (more, closed) = feed.take();
                if !more.is_empty() {
                    let more = job.engine.prepare(more);
                    job.sentences.extend(more);
                    fed_at = Instant::now();
                    continue;
                }
                if closed || !self.live(generation) || (self.busy)() || fed_at.elapsed() > FEED_PATIENCE {
                    break;
                }
                std::thread::sleep(Duration::from_millis(15));
                continue;
            }
            if !self.live(generation) || (self.busy)() {
                break;
            }
            let at = job.first + i;
            let track = job.track.clone();
            // A read-aloud: one sentence ahead of the voice, no more.
            if track.is_some() && i > 0 && !self.wait_turn(generation, &opened.play, at) {
                break;
            }
            let text = if track.is_some() { said_text(&job.sentences[i]) } else { job.sentences[i].clone() };
            // A read-aloud's sentence: its audio kept a moment (for its
            // words' times), each piece tagged with where it is.
            let mut heard: Vec<f32> = Vec::new();
            let mut format = (0u32, 0u16);
            let mut length = Duration::ZERO;
            let said = match synth.as_mut() {
                Ok(s) => {
                    let play = &opened.play;
                    let tracked = track.is_some();
                    // Each piece as it comes, unless it's been stopped or a
                    // dictation started meanwhile.
                    let mut out = |pcm: Pcm| {
                        let go = self.live(generation) && !(self.busy)();
                        if go {
                            if tracked {
                                let piece = pcm.duration();
                                if format.0 == 0 {
                                    format = (pcm.sample_rate, pcm.channels);
                                }
                                if format == (pcm.sample_rate, pcm.channels) {
                                    heard.extend_from_slice(&pcm.samples);
                                } else {
                                    heard.clear();
                                    format = (u32::MAX, 0);
                                }
                                play.append_tagged(pcm, Tag { sentence: at, offset: length });
                                length += piece;
                            } else {
                                play.append(pcm);
                            }
                        }
                        go
                    };
                    let r = s.stream(&text, &mut out);
                    r.map(|()| if tracked { s.marks() } else { None })
                }
                Err(e) => Err(e.clone()),
            };
            match said {
                Ok(times) => {
                    if let Some(t) = &track {
                        let format = if format.0 == u32::MAX { (0, 0) } else { format };
                        t.record(at, &text, times, &heard, format, length, engine_id);
                    }
                    i += 1;
                }
                // Stopped mid-sentence: not a failure.
                Err(_) if !self.live(generation) => break,
                Err(e) => {
                    tracing::warn!("Spoken replies: {}", e);
                    let Some(fallback) = job.fallback.take() else {
                        error = Some(e);
                        break;
                    };
                    // Once: the person is told, and the local voice (if
                    // there's one) reads the rest, this sentence again
                    // included. Never the failing engine again.
                    (fallback.notify)(&e, &job.sentences[i..], fallback.local.is_some());
                    let Some((engine, voice)) = fallback.local else {
                        break;
                    };
                    if let Some(c) = self.current().as_mut().filter(|c| c.generation == generation) {
                        c.engine = Arc::clone(&engine);
                    }
                    engine_id = engine.id();
                    synth = engine.open(&voice, job.rate);
                }
            }
        }
        // Let it play out, unless it's stopped or a dictation starts.
        while self.live(generation) && !opened.play.done() {
            if (self.busy)() {
                opened.play.stop();
                break;
            }
            if !opened.play.paused() {
                self.heard_now();
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(opened.keep);
        self.finish(generation, job.message_id, error);
    }

    /// A read-aloud's pace: sentence `at` is made once the one before it has
    /// started playing (or the voice ran dry), so a long text is made a
    /// sentence ahead of the voice, never all at once. `false`: stopped (or
    /// a dictation started) while it waited.
    fn wait_turn(&self, generation: u64, play: &Arc<dyn Playback>, at: usize) -> bool {
        loop {
            if !self.live(generation) || (self.busy)() {
                return false;
            }
            if play.done() || play.position().is_some_and(|(playing, _)| playing + 1 >= at) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(15));
        }
    }

    /// The read-aloud sentence playing now, and how far into it.
    pub fn position(&self) -> Option<(usize, Duration)> {
        self.current().as_ref()?.play.as_ref()?.position()
    }

    /// Hold the voice where it is (`true` if it was speaking).
    pub fn pause(&self) -> bool {
        let play = self.current().as_ref().and_then(|c| c.play.clone());
        match play {
            Some(p) => {
                p.pause();
                true
            }
            None => false,
        }
    }

    /// Go on from where it was held.
    pub fn resume(&self) -> bool {
        let play = self.current().as_ref().and_then(|c| c.play.clone());
        match play {
            Some(p) => {
                p.resume();
                true
            }
            None => false,
        }
    }

    /// The worker's end: "not speaking" — unless it was stopped or replaced,
    /// which said so already.
    fn finish(&self, generation: u64, message_id: Option<String>, error: Option<String>) {
        {
            let mut cur = self.current();
            if cur.as_ref().is_none_or(|c| c.generation != generation) {
                return;
            }
            *cur = None;
        }
        (self.emit)(&TtsState { speaking: false, message_id, error });
    }

    /// Silence it now and drop what was queued. `true` if something was
    /// being said.
    pub fn stop(&self) -> bool {
        self.generation.fetch_add(1, Ordering::SeqCst);
        let Some(cur) = self.current().take() else {
            return false;
        };
        cur.engine.cancel();
        if let Some(play) = &cur.play {
            if !play.done() {
                self.heard_now();
            }
            play.stop();
        }
        (self.emit)(&TtsState { speaking: false, message_id: cur.message_id, error: None });
        true
    }

    /// Whether Yap's voice was in the room just now (speaking, or the last
    /// sound under [`AUDIBLE_TAIL`] ago).
    pub fn recently_audible(&self) -> bool {
        self.last_audible
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some_and(|t| t.elapsed() < AUDIBLE_TAIL)
    }
}

static SPEAKER: OnceLock<Arc<Speaker>> = OnceLock::new();
/// For a cloud voice's notice (`alert.rs`).
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

/// Start the speaker (lib.rs setup): its state goes to every window. A
/// watcher unloads Kokoro once it's been idle for the speech model's unload
/// time (10 minutes when that's "Never": it holds ~0.6 GB).
pub fn init(app: &tauri::AppHandle) {
    set_volume(crate::config::load().tts_volume);
    let _ = APP.set(app.clone());
    let app = app.clone();
    let _ = SPEAKER.set(Speaker::new(
        Box::new(move |s| {
            use tauri::Emitter;
            let _ = app.emit("yap-tts-state", s);
            // The Voice Assistant's answer ended (or was stopped).
            crate::chat::assistant::on_tts_state(&app, s.speaking, s.message_id.as_deref());
            // So did a read-aloud (its card goes).
            crate::voice::read_aloud::on_tts_state(&app, s);
        }),
        Box::new(|| crate::dictation::pipeline::dictation_state().1),
    ));
    let _ = std::thread::Builder::new().name("tts-idle".into()).spawn(|| loop {
        std::thread::sleep(Duration::from_secs(20));
        kokoro::idle_unload(kokoro_idle(&crate::config::load().model_unload_timeout));
    });
}

/// How long Kokoro stays loaded unused: the speech model's unload time, or
/// 10 minutes when that's "never".
fn kokoro_idle(setting: &str) -> Duration {
    match setting {
        "1min" => Duration::from_secs(60),
        "5min" => Duration::from_secs(5 * 60),
        "15min" => Duration::from_secs(15 * 60),
        "30min" => Duration::from_secs(30 * 60),
        _ => Duration::from_secs(10 * 60),
    }
}

pub(super) fn speaker() -> Option<&'static Arc<Speaker>> {
    SPEAKER.get()
}

/// Stop speaking now (Stop, Esc, leaving Chat, a meeting starting).
pub fn stop() -> bool {
    speaker().is_some_and(|s| s.stop())
}

/// What's being read aloud now, if anything (its message id).
pub fn speaking_id() -> Option<String> {
    speaker().map(|s| s.state()).filter(|s| s.speaking).and_then(|s| s.message_id)
}

/// A dictation is starting (`pipeline::start_recording`): stop speaking
/// first. `true` when Yap's voice may be in the mic's pre-roll (it was
/// speaking, or stopped a moment ago), so the dictation leaves it out.
pub fn stop_for_dictation() -> bool {
    let Some(s) = speaker() else {
        return false;
    };
    let was = s.stop();
    if was {
        tracing::info!("Spoken replies: a dictation started, stopped speaking");
    }
    was || s.recently_audible()
}

/// **Say `text`** — an answer's markdown, or any plain text (a later "read
/// the selection aloud" comes in here too) — with the voice Settings picked
/// ([`voicing`]: the local one, or a cloud voice with the local one standing
/// by), its speed, Yap's volume and the output device. `message_id`: what
/// the pages call it.
pub fn speak(text: &str, message_id: Option<String>) -> Result<(), String> {
    let cfg = crate::config::load();
    say(utterances(text), message_id, voicing(&cfg), cfg.tts_rate, cfg.output_device)
}

/// **Start saying a streamed answer** (the Voice Assistant, `assistant.rs`):
/// the voice Settings picked, as [`speak`] uses it, with no sentences yet —
/// they come through the returned [`Feed`] as the answer is written.
pub fn speak_streamed(message_id: Option<String>) -> Result<Arc<Feed>, String> {
    let cfg = crate::config::load();
    let opener = if crate::shell::e2e::active() { sim_opener() } else { rodio_opener(cfg.output_device.clone()) };
    streamed(message_id, voicing(&cfg), opener, &cfg)
}

/// **Say a streamed answer somewhere other than this PC's speakers** (Yap
/// Link: the phone, `link/voice.rs`): [`speak_streamed`] with `voicing` and
/// the audio handed to `open`'s playback — the same speaker, one voice at a
/// time, never over a dictation or a meeting.
pub fn speak_streamed_to(message_id: Option<String>, voicing: Voicing, open: Opener) -> Result<Arc<Feed>, String> {
    streamed(message_id, voicing, open, &crate::config::load())
}

fn streamed(message_id: Option<String>, voicing: Voicing, opener: Opener, cfg: &crate::config::YapConfig) -> Result<Arc<Feed>, String> {
    if let Some(why) = meeting_quiet() {
        return Err(why.into());
    }
    let speaker = speaker().ok_or("Spoken replies aren't ready yet")?;
    let rate = if cfg.tts_rate.is_finite() && cfg.tts_rate > 0.0 { cfg.tts_rate.clamp(0.5, 2.0) } else { 1.0 };
    let Voicing { engine, voice, cloud } = voicing;
    let feed = Arc::new(Feed::default());
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() {
        feed.state.lock().unwrap_or_else(|p| p.into_inner()).record = Some(Spoken {
            message_id: message_id.clone(),
            text: String::new(),
            sentences: Vec::new(),
            engine: engine.id().to_string(),
            voice: voice.clone(),
            rate,
            volume: volume(),
            fallback: None,
            streamed: true,
        });
    }
    let fallback = cloud.map(|plan| fallback_for(plan, message_id.clone(), rate));
    let job = Job { message_id, sentences: Vec::new(), engine, voice, rate, fallback, track: None, first: 0, feed: Some(Arc::clone(&feed)) };
    let generation = speaker.start(job, opener)?;
    feed.attach(speaker, generation);
    Ok(feed)
}

/// A streamed answer's sentences the voice can say now
/// ([`speak_streamed`]): its first block's, as [`brief`] takes them, at most
/// `n` — without the last while the block may still be being written
/// (`done`: the answer is whole). Written for the ear as [`utterances`]
/// writes them. Pure.
pub fn ready_sentences(md: &str, n: usize, done: bool) -> Vec<String> {
    let md: String = md.chars().take(MAX_CHARS).collect();
    let blocks = speakable_blocks(&md);
    let Some(at) = blocks.iter().position(|b| b.chars().any(char::is_alphanumeric)) else {
        return Vec::new();
    };
    let mut out = sentences(&normalize::for_speech(&blocks[at]));
    if !done && at + 1 == blocks.len() {
        out.pop();
    }
    out.truncate(n);
    out
}

/// Why Yap mustn't talk now, if it mustn't: a meeting records (the notes
/// would hear Yap) or a call is on (the people on it could, and a voice over
/// a meeting is too much — the maintainer, 2026-10-18). Spoken replies, ▶ and the
/// Voice Assistant (`assistant::off_for_meeting`) all keep quiet then.
pub fn meeting_quiet() -> Option<&'static str> {
    if crate::meetings::meeting::is_recording() {
        Some("Yap doesn't talk while it records a meeting")
    } else if crate::calls::meeting_detect::call_live() {
        Some("Yap doesn't talk during a call")
    } else {
        None
    }
}

pub(super) fn say(sentences: Vec<String>, message_id: Option<String>, voicing: Voicing, rate: f32, device: Option<String>) -> Result<(), String> {
    if let Some(why) = meeting_quiet() {
        return Err(why.into());
    }
    let speaker = speaker().ok_or("Spoken replies aren't ready yet")?;
    let rate = if rate.is_finite() && rate > 0.0 { rate.clamp(0.5, 2.0) } else { 1.0 };
    let Voicing { engine, voice, cloud } = voicing;
    let sentences = engine.prepare(sentences);
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() && !sentences.is_empty() {
        spoken().push(Spoken {
            message_id: message_id.clone(),
            text: sentences.join(" "),
            sentences: sentences.clone(),
            engine: engine.id().to_string(),
            voice: voice.clone(),
            rate,
            volume: volume(),
            fallback: None,
            streamed: false,
        });
    }
    let fallback = cloud.map(|plan| fallback_for(plan, message_id.clone(), rate));
    let opener = if crate::shell::e2e::active() { sim_opener() } else { rodio_opener(device) };
    let job = Job { message_id, sentences, engine, voice, rate, fallback, track: None, first: 0, feed: None };
    speaker.speak(job, opener)
}

// ---- reading aloud (read_aloud.rs) ----

/// The sentences of a read-aloud as the voice Settings picked says them best
/// (Kokoro and ElevenLabs join a sentence under three words to the next).
pub fn prepare_for_reading(sentences: Vec<String>) -> Vec<String> {
    voicing_for(&crate::config::load(), true).engine.prepare(sentences)
}

/// **Read `track` aloud** from sentence `from` (`read_aloud.rs`): the voice
/// Settings picked (a cloud voice asked for its word timing), at `rate`,
/// Yap's volume, the output device. Each sentence is made only once the one
/// before it plays, and the track keeps when its words start. `message_id`:
/// what the pages call it (an answer's id keeps its ▶ / ■).
pub fn read(track: Arc<Track>, from: usize, message_id: String, rate: f32) -> Result<(), String> {
    if let Some(why) = meeting_quiet() {
        return Err(why.into());
    }
    let speaker = speaker().ok_or("Spoken replies aren't ready yet")?;
    let cfg = crate::config::load();
    let rate = if rate.is_finite() && rate > 0.0 { rate.clamp(0.5, 2.0) } else { 1.0 };
    let sentences: Vec<String> = track.all().get(from..).map(<[String]>::to_vec).unwrap_or_default();
    let Voicing { engine, voice, cloud } = voicing_for(&cfg, true);
    #[cfg(debug_assertions)]
    if crate::shell::e2e::active() && !sentences.is_empty() {
        let said: Vec<String> = sentences.iter().map(|s| said_text(s)).collect();
        spoken().push(Spoken {
            message_id: Some(message_id.clone()),
            text: said.join(" "),
            sentences: said,
            engine: engine.id().to_string(),
            voice: voice.clone(),
            rate,
            volume: volume(),
            fallback: None,
            streamed: false,
        });
    }
    let fallback = cloud.map(|plan| fallback_for(plan, Some(message_id.clone()), rate));
    let opener = if crate::shell::e2e::active() { sim_opener() } else { rodio_opener(cfg.output_device) };
    let job = Job { message_id: Some(message_id), sentences, engine, voice, rate, fallback, track: Some(track), first: from, feed: None };
    speaker.speak(job, opener)
}

/// The read-aloud sentence playing now (its index on the track), and how
/// far into its audio.
pub fn position() -> Option<(usize, Duration)> {
    speaker().and_then(|s| s.position())
}

/// Hold the voice where it is (a read-aloud's ⏸).
pub fn pause() -> bool {
    speaker().is_some_and(|s| s.pause())
}

/// Go on from where it was held.
pub fn resume() -> bool {
    speaker().is_some_and(|s| s.resume())
}

/// A cloud voice's [`Fallback`]: its local voice, and the notice.
fn fallback_for(plan: CloudPlan, message_id: Option<String>, rate: f32) -> Fallback {
    let CloudPlan { provider, local, key, choice } = plan;
    // What test mode records when the local voice takes over.
    #[cfg(debug_assertions)]
    let record = local.as_ref().map(|(e, v)| (e.id().to_string(), v.clone()));
    let _ = (&message_id, rate);
    Fallback {
        local,
        notify: Box::new(move |error: &str, left: &[String], took_over: bool| {
            #[cfg(debug_assertions)]
            if let (true, Some((engine, voice))) = (crate::shell::e2e::active(), record) {
                spoken().push(Spoken {
                    message_id,
                    text: left.join(" "),
                    sentences: left.to_vec(),
                    engine,
                    voice,
                    rate,
                    volume: volume(),
                    fallback: Some(error.to_string()),
                    streamed: false,
                });
            }
            let _ = left;
            cloud_failed(provider, key, choice, error, took_over);
        }),
    }
}

/// The alert's kind for "ElevenLabs didn't answer" (`alert.rs`).
pub const CLOUD_FAILED: &str = "voiceFallback";

/// A cloud voice failed mid-answer: say why — once per reason in ten
/// minutes (`cloud::Health::announce`) — in the one place Yap's notices go
/// (`alert.rs`: the main window's toast, else a Yap bar card, else a
/// Windows notification), with a way to the voice settings.
fn cloud_failed(provider: &'static str, key: u64, choice: u64, error: &str, took_over: bool) {
    let failure = cloud::with_health(|h| {
        // A failure the engine couldn't put on record itself (its request
        // couldn't even be made): on record now, in its words.
        if h.last(provider).is_none() {
            let why = error.split_once(": ").map_or(error, |(_, w)| w);
            h.failed(provider, key, choice, cloud::Failure::Rejected(why.to_string()), Instant::now());
        }
        h.announce(provider, Instant::now())
    });
    let Some(failure) = failure else {
        tracing::info!("Spoken replies: {} failed again ({error}); said already", cloud::name(provider));
        return;
    };
    let (title, body) = cloud::notice_text(provider, &failure, took_over);
    match APP.get() {
        Some(app) => crate::notices::alert::show_soon(
            app,
            crate::notices::alert::Alert {
                kind: CLOUD_FAILED,
                icon: "assistant",
                title,
                body,
                action: Some(crate::notices::alert::Action { id: crate::notices::alert::OPEN_VOICE_SETTINGS, label: "Open settings" }),
                timeout_ms: 12_000,
            },
        ),
        None => tracing::warn!("Spoken replies: {title}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::tts::playback::{Opened, SimPlayback};
    use crate::voice::tts::{elevenlabs, StandIn, Synth, Voice};

    // ---- the speaker's queue and stop, on a simulated clock

    /// A synthesiser that keeps what it was asked and takes `delay` a sentence.
    struct Recording {
        said: Arc<Mutex<Vec<String>>>,
        delay: Duration,
        fail_on: Option<&'static str>,
    }
    impl Synth for Recording {
        fn synthesize(&mut self, text: &str) -> Result<Pcm, String> {
            std::thread::sleep(self.delay);
            if self.fail_on == Some(text) {
                return Err("synthesis failed".into());
            }
            self.said.lock().unwrap().push(text.to_string());
            // 100 ms a sentence.
            Ok(Pcm { sample_rate: 1_000, channels: 1, samples: vec![0.0; 100] })
        }
    }
    struct Fake {
        said: Arc<Mutex<Vec<String>>>,
        delay: Duration,
        fail_on: Option<&'static str>,
    }
    impl Engine for Fake {
        fn id(&self) -> &'static str {
            "fake"
        }
        fn voices(&self) -> Result<Vec<Voice>, String> {
            Ok(Vec::new())
        }
        fn default_voice(&self) -> Option<String> {
            None
        }
        fn open(&self, _voice: &str, _rate: f32) -> Result<Box<dyn Synth>, String> {
            Ok(Box::new(Recording { said: Arc::clone(&self.said), delay: self.delay, fail_on: self.fail_on }))
        }
    }

    struct Rig {
        speaker: Arc<Speaker>,
        events: Arc<Mutex<Vec<TtsState>>>,
        said: Arc<Mutex<Vec<String>>>,
        busy: Arc<std::sync::atomic::AtomicBool>,
    }

    fn rig() -> Rig {
        let events = Arc::new(Mutex::new(Vec::new()));
        let busy = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (e, b) = (Arc::clone(&events), Arc::clone(&busy));
        Rig {
            speaker: Speaker::new(
                Box::new(move |s| e.lock().unwrap().push(s.clone())),
                Box::new(move || b.load(Ordering::SeqCst)),
            ),
            events,
            said: Arc::new(Mutex::new(Vec::new())),
            busy,
        }
    }

    impl Rig {
        fn job(&self, id: &str, sentences: &[&str], delay_ms: u64, fail_on: Option<&'static str>) -> Job {
            Job {
                message_id: Some(id.into()),
                sentences: sentences.iter().map(|s| s.to_string()).collect(),
                engine: Arc::new(Fake { said: Arc::clone(&self.said), delay: Duration::from_millis(delay_ms), fail_on }),
                voice: String::new(),
                rate: 1.0,
                fallback: None,
                track: None,
                first: 0,
                feed: None,
            }
        }
        fn events(&self) -> Vec<(bool, String, Option<String>)> {
            self.events
                .lock()
                .unwrap()
                .iter()
                .map(|s| (s.speaking, s.message_id.clone().unwrap_or_default(), s.error.clone()))
                .collect()
        }
        fn wait_quiet(&self) {
            wait_for("it to stop speaking", || !self.speaker.state().speaking);
        }
    }

    /// Wait (up to 5 s) until `done`: a test under load waits on what it
    /// checks, never on a fixed sleep.
    fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
        let start = Instant::now();
        while !done() {
            assert!(start.elapsed() < Duration::from_secs(5), "waited for {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn ev(speaking: bool, id: &str) -> (bool, String, Option<String>) {
        (speaking, id.to_string(), None)
    }

    #[test]
    fn speaks_every_sentence_in_order_then_says_it_stopped() {
        let r = rig();
        r.speaker.speak(r.job("a", &["One.", "Two.", "Three."], 1, None), sim_opener()).unwrap();
        assert_eq!(r.speaker.state().message_id.as_deref(), Some("a"));
        r.wait_quiet();
        assert_eq!(*r.said.lock().unwrap(), vec!["One.", "Two.", "Three."]);
        assert_eq!(r.events(), vec![ev(true, "a"), ev(false, "a")]);
        // It was audible a moment ago.
        assert!(r.speaker.recently_audible());
    }

    #[test]
    fn stop_is_instant_and_drops_the_queue() {
        let r = rig();
        r.speaker.speak(r.job("a", &["One.", "Two.", "Three.", "Four."], 40, None), sim_opener()).unwrap();
        std::thread::sleep(Duration::from_millis(60));
        assert!(r.speaker.stop());
        // Not speaking the moment stop returns.
        assert!(!r.speaker.state().speaking);
        std::thread::sleep(Duration::from_millis(200));
        assert!(r.said.lock().unwrap().len() <= 2, "the queue went on: {:?}", r.said.lock().unwrap());
        assert_eq!(r.events(), vec![ev(true, "a"), ev(false, "a")]);
        // Nothing left to stop.
        assert!(!r.speaker.stop());
    }

    #[test]
    fn a_new_answer_replaces_the_one_being_read() {
        let r = rig();
        // 300 ms a sentence: "A three." is ~600 ms after "A one.", so B
        // starts well before it however loaded the machine is.
        r.speaker.speak(r.job("a", &["A one.", "A two.", "A three."], 300, None), sim_opener()).unwrap();
        wait_for("A's first sentence", || !r.said.lock().unwrap().is_empty());
        r.speaker.speak(r.job("b", &["B one."], 1, None), sim_opener()).unwrap();
        r.wait_quiet();
        // Long enough for A's worker to finish the sentence it was making:
        // it says nothing more on the way out.
        std::thread::sleep(Duration::from_millis(350));
        assert_eq!(r.events(), vec![ev(true, "a"), ev(false, "a"), ev(true, "b"), ev(false, "b")]);
        let said = r.said.lock().unwrap().clone();
        assert!(said.contains(&"B one.".to_string()));
        assert!(!said.contains(&"A three.".to_string()), "{said:?}");
    }

    #[test]
    fn a_dictation_silences_it_and_nothing_starts_over_one() {
        let r = rig();
        r.speaker.speak(r.job("a", &["One.", "Two.", "Three.", "Four."], 30, None), sim_opener()).unwrap();
        std::thread::sleep(Duration::from_millis(40));
        r.busy.store(true, Ordering::SeqCst);
        r.wait_quiet();
        assert!(r.said.lock().unwrap().len() < 4);
        // While it records, nothing new is said.
        assert!(r.speaker.speak(r.job("b", &["Hi."], 1, None), sim_opener()).is_err());
        assert_eq!(r.events().last(), Some(&ev(false, "a")));
    }

    #[test]
    fn a_failure_ends_it_and_says_why() {
        let r = rig();
        r.speaker.speak(r.job("a", &["One.", "Bad.", "Three."], 1, Some("Bad.")), sim_opener()).unwrap();
        r.wait_quiet();
        assert_eq!(*r.said.lock().unwrap(), vec!["One."]);
        assert_eq!(r.events().last(), Some(&(false, "a".to_string(), Some("synthesis failed".to_string()))));
        // An output that won't open says so too.
        let r = rig();
        r.speaker
            .speak(r.job("b", &["One."], 1, None), Box::new(|| Err("No sound output device to speak on".to_string())))
            .unwrap();
        r.wait_quiet();
        assert_eq!(r.events().last().and_then(|e| e.2.clone()).as_deref(), Some("No sound output device to speak on"));
        assert!(r.said.lock().unwrap().is_empty());
    }

    #[test]
    fn a_streamed_answer_is_said_as_its_sentences_come() {
        let r = rig();
        let feed = Arc::new(Feed::default());
        let mut job = r.job("s", &[], 1, None);
        job.feed = Some(Arc::clone(&feed));
        let g = r.speaker.start(job, sim_opener()).expect("started with no sentences yet");
        feed.attach(&r.speaker, g);
        assert!(r.speaker.state().speaking);
        assert!(feed.push(vec!["The first one.".into()]));
        let start = Instant::now();
        while r.said.lock().unwrap().is_empty() {
            assert!(start.elapsed() < Duration::from_secs(5), "the first sentence wasn't said");
            std::thread::sleep(Duration::from_millis(5));
        }
        // Still open: it waits for more, speaking.
        std::thread::sleep(Duration::from_millis(40));
        assert!(r.speaker.state().speaking);
        assert!(feed.push(vec!["The second one.".into(), "And the third.".into()]));
        feed.close();
        r.wait_quiet();
        assert_eq!(*r.said.lock().unwrap(), vec!["The first one.", "The second one.", "And the third."]);
        // Closed: nothing more goes in.
        assert!(!feed.push(vec!["Too late.".into()]));
        assert_eq!(r.events(), vec![ev(true, "s"), ev(false, "s")]);
    }

    #[test]
    fn a_stopped_streamed_answer_takes_no_more() {
        let r = rig();
        let feed = Arc::new(Feed::default());
        let mut job = r.job("s", &[], 1, None);
        job.feed = Some(Arc::clone(&feed));
        let g = r.speaker.start(job, sim_opener()).unwrap();
        feed.attach(&r.speaker, g);
        assert!(feed.live() && feed.push(vec!["One more time.".into()]));
        assert!(r.speaker.stop());
        assert!(!feed.live());
        assert!(!feed.push(vec!["Two more times.".into()]));
        // Never attached: never live.
        assert!(!Feed::default().push(vec!["Nobody hears this.".into()]));
    }

    #[test]
    fn a_streamed_answer_gives_the_voice_its_whole_sentences() {
        // While it's written: the first block's whole sentences, never the
        // one still being written.
        assert!(ready_sentences("The match starts at", 2, false).is_empty());
        assert_eq!(ready_sentences("The match starts at eight. It is", 2, false), ["The match starts at eight."]);
        assert_eq!(ready_sentences("Up first. Then the second. Then a third. And", 2, false), ["Up first.", "Then the second."]);
        // Another block began: the first is whole (its citation not read).
        assert_eq!(ready_sentences("Gale is on the beach [1].\n\n- He", 2, false), ["Gale is on the beach."]);
        // Done: all of it, up to n.
        assert_eq!(ready_sentences("Only this.", 2, true), ["Only this."]);
        assert!(ready_sentences("", 2, true).is_empty());
    }

    #[test]
    fn nothing_to_say_is_an_error() {
        let r = rig();
        assert!(r.speaker.speak(r.job("a", &[], 1, None), sim_opener()).is_err());
        assert!(r.events().is_empty());
    }

    // ---- a read-aloud: paced, its words timed, held

    fn read_job(track: &Arc<Track>, first: usize, engine: Arc<dyn Engine>, rate: f32) -> Job {
        Job {
            message_id: Some("read:1".into()),
            sentences: track.all()[first..].to_vec(),
            engine,
            voice: String::new(),
            rate,
            fallback: None,
            track: Some(Arc::clone(track)),
            first,
            feed: None,
        }
    }

    fn sim(play: &Arc<SimPlayback>) -> Opener {
        let p = Arc::clone(play);
        Box::new(move || Ok(Opened { play: p, keep: None }))
    }

    #[test]
    fn a_read_aloud_is_made_a_sentence_ahead_never_all_at_once() {
        let r = rig();
        // 30 short sentences, ~170 ms each at 3× on the stand-in.
        let track = Track::new((0..30).map(|i| format!("Line {}.", ["one", "two", "three"][i % 3])).collect());
        let play = Arc::new(SimPlayback::default());
        r.speaker.speak(read_job(&track, 0, Arc::new(StandIn), 3.0), sim(&play)).unwrap();
        let start = Instant::now();
        let mut seen = 0;
        while start.elapsed() < Duration::from_millis(900) {
            let made = track.made_count();
            if let Some((playing, _)) = r.speaker.position() {
                // Made: the one playing and the next, never more.
                assert!(made <= playing + 2, "made {made} with {playing} playing");
                seen = seen.max(playing);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(seen >= 2, "it moved on: {seen}");
        assert!(track.made_count() < 30);
        // Each sentence made has its words' starts, estimated (silence).
        let made = track.made(0).unwrap();
        assert_eq!(made.timing, marks::Timing::Estimate);
        assert_eq!(made.starts.iter().map(|&(w, _)| w).collect::<Vec<_>>(), vec![0, 1]);
        assert_eq!(made.starts[0].1, Duration::ZERO);
        assert!(r.speaker.stop());
    }

    #[test]
    fn a_held_read_aloud_stands_still_and_goes_on_from_there() {
        let r = rig();
        let track = Track::new((0..6).map(|_| "Read these four words.".to_string()).collect());
        let play = Arc::new(SimPlayback::default());
        // A second a sentence on the stand-in.
        r.speaker.speak(read_job(&track, 0, Arc::new(StandIn), 1.0), sim(&play)).unwrap();
        wait_for("the first sentence to play", || r.speaker.position().is_some());
        assert!(r.speaker.pause());
        let held = r.speaker.position().unwrap();
        // The worker makes the one after the sentence held, and no more.
        let ahead = (held.0 + 2).min(track.len());
        wait_for("the sentence ahead", || track.made_count() >= ahead);
        let made = track.made_count();
        assert_eq!(made, ahead);
        std::thread::sleep(Duration::from_millis(400));
        // Held: the same place, nothing more made, still "speaking".
        assert_eq!(r.speaker.position().unwrap(), held);
        assert_eq!(track.made_count(), made);
        assert!(r.speaker.state().speaking);
        assert!(r.speaker.resume());
        wait_for("it to go on", || r.speaker.position().is_some_and(|now| now > held));
        // Starting from a later sentence: tagged from there.
        r.speaker.speak(read_job(&track, 4, Arc::new(StandIn), 1.0), sim(&Arc::new(SimPlayback::default()))).unwrap();
        wait_for("the later sentence to play", || r.speaker.position().is_some());
        assert_eq!(r.speaker.position().map(|p| p.0), Some(4));
        r.speaker.stop();
    }

    /// An engine that says when each character starts (as Windows' cues
    /// and ElevenLabs' alignment do): 50 ms a character.
    struct Timed;
    struct TimedSynth(Option<Vec<marks::CharTime>>);
    impl Engine for Timed {
        fn id(&self) -> &'static str {
            "timed"
        }
        fn voices(&self) -> Result<Vec<Voice>, String> {
            Ok(Vec::new())
        }
        fn default_voice(&self) -> Option<String> {
            None
        }
        fn open(&self, _voice: &str, _rate: f32) -> Result<Box<dyn Synth>, String> {
            Ok(Box::new(TimedSynth(None)))
        }
    }
    impl Synth for TimedSynth {
        fn synthesize(&mut self, text: &str) -> Result<Pcm, String> {
            let n = text.chars().count();
            self.0 = Some((0..n).map(|i| marks::CharTime { at: i, start: Duration::from_millis(i as u64 * 50) }).collect());
            Ok(Pcm { sample_rate: 1_000, channels: 1, samples: vec![0.0; n * 50] })
        }
        fn marks(&mut self) -> Option<Vec<marks::CharTime>> {
            self.0.take()
        }
    }

    #[test]
    fn an_engines_own_word_timing_is_kept_on_the_shown_words() {
        let r = rig();
        // Said "It came to twelve pounds." for "It came to £12."
        let track = Track::new(vec!["It came to £12.".into()]);
        let play = Arc::new(SimPlayback::default());
        r.speaker.speak(read_job(&track, 0, Arc::new(Timed), 1.0), sim(&play)).unwrap();
        let start = Instant::now();
        while track.made(0).is_none() {
            assert!(start.elapsed() < Duration::from_secs(2));
            std::thread::sleep(Duration::from_millis(5));
        }
        let made = track.made(0).unwrap();
        assert_eq!(made.timing, marks::Timing::Engine);
        assert_eq!(made.engine, "timed");
        // "It" 0, "came" 3 chars in, "to" 8, "£12" when "twelve" starts (11).
        assert_eq!(
            made.starts,
            vec![(0, Duration::ZERO), (1, Duration::from_millis(150)), (2, Duration::from_millis(400)), (3, Duration::from_millis(550))]
        );
        r.speaker.stop();
    }

    // ---- a cloud voice failing mid-answer

    type Told = Arc<Mutex<Vec<(String, Vec<String>, bool)>>>;

    /// A fallback reading into `said`, telling `told`.
    fn fallback(said: &Arc<Mutex<Vec<String>>>, local: bool, told: &Told) -> Fallback {
        let told = Arc::clone(told);
        Fallback {
            local: local.then(|| {
                let e: Arc<dyn Engine> = Arc::new(Fake { said: Arc::clone(said), delay: Duration::from_millis(1), fail_on: None });
                (e, String::new())
            }),
            notify: Box::new(move |e, left, took_over| told.lock().unwrap().push((e.to_string(), left.to_vec(), took_over))),
        }
    }

    #[test]
    fn a_failing_cloud_voice_hands_the_rest_to_the_local_voice_once() {
        let r = rig();
        let local_said = Arc::new(Mutex::new(Vec::new()));
        let told: Told = Arc::default();
        let mut job = r.job("a", &["One.", "Two.", "Three."], 1, Some("Two."));
        job.fallback = Some(fallback(&local_said, true, &told));
        r.speaker.speak(job, sim_opener()).unwrap();
        r.wait_quiet();
        // The cloud said the first sentence; the local voice the rest, the
        // failed one included; nothing said twice, the cloud never asked again.
        assert_eq!(*r.said.lock().unwrap(), vec!["One."]);
        assert_eq!(*local_said.lock().unwrap(), vec!["Two.", "Three."]);
        assert_eq!(*told.lock().unwrap(), vec![("synthesis failed".to_string(), vec!["Two.".to_string(), "Three.".to_string()], true)]);
        // Told already: the answer ends without an error of its own.
        assert_eq!(r.events(), vec![ev(true, "a"), ev(false, "a")]);
    }

    #[test]
    fn no_local_voice_means_one_notice_and_silence() {
        let r = rig();
        let local_said = Arc::new(Mutex::new(Vec::new()));
        let told: Told = Arc::default();
        let mut job = r.job("a", &["One.", "Two.", "Three."], 1, Some("One."));
        job.fallback = Some(fallback(&local_said, false, &told));
        r.speaker.speak(job, sim_opener()).unwrap();
        r.wait_quiet();
        assert!(r.said.lock().unwrap().is_empty());
        assert!(local_said.lock().unwrap().is_empty());
        assert_eq!(told.lock().unwrap().len(), 1);
        assert!(!told.lock().unwrap()[0].2);
        assert_eq!(r.events(), vec![ev(true, "a"), ev(false, "a")]);
        // Without a fallback (Settings' preview), the error is the answer's.
        let r = rig();
        r.speaker.speak(r.job("b", &["One."], 1, Some("One.")), sim_opener()).unwrap();
        r.wait_quiet();
        assert_eq!(r.events().last().and_then(|e| e.2.clone()).as_deref(), Some("synthesis failed"));
    }

    /// A streaming synthesiser: `pieces` of 10 ms audio, `gap` apart, each
    /// handed on as it's made; counts the pieces it made.
    struct Streaming {
        pieces: usize,
        gap: Duration,
        made: Arc<AtomicU64>,
    }
    impl Synth for Streaming {
        fn synthesize(&mut self, _text: &str) -> Result<Pcm, String> {
            unreachable!("streams")
        }
        fn stream(&mut self, _text: &str, out: &mut dyn FnMut(Pcm) -> bool) -> Result<(), String> {
            for _ in 0..self.pieces {
                std::thread::sleep(self.gap);
                self.made.fetch_add(1, Ordering::SeqCst);
                if !out(Pcm { sample_rate: 1_000, channels: 1, samples: vec![0.0; 10] }) {
                    break;
                }
            }
            Ok(())
        }
    }
    struct StreamingEngine(Arc<AtomicU64>);
    impl Engine for StreamingEngine {
        fn id(&self) -> &'static str {
            "streaming"
        }
        fn voices(&self) -> Result<Vec<Voice>, String> {
            Ok(Vec::new())
        }
        fn default_voice(&self) -> Option<String> {
            None
        }
        fn open(&self, _voice: &str, _rate: f32) -> Result<Box<dyn Synth>, String> {
            Ok(Box::new(Streaming { pieces: 20, gap: Duration::from_millis(15), made: Arc::clone(&self.0) }))
        }
    }

    #[test]
    fn a_streaming_voice_plays_its_first_piece_before_the_sentence_is_made_and_stops_mid_sentence() {
        let r = rig();
        let made = Arc::new(AtomicU64::new(0));
        let play = Arc::new(SimPlayback::default());
        let p = Arc::clone(&play);
        let opener: Opener = Box::new(move || Ok(Opened { play: p, keep: None }));
        let job = Job {
            message_id: Some("a".into()),
            sentences: vec!["A long sentence.".into()],
            engine: Arc::new(StreamingEngine(Arc::clone(&made))),
            voice: String::new(),
            rate: 1.0,
            fallback: None,
            track: None,
            first: 0,
            feed: None,
        };
        r.speaker.speak(job, opener).unwrap();
        let start = Instant::now();
        while play.appended.load(Ordering::SeqCst) == 0 {
            assert!(start.elapsed() < Duration::from_secs(2), "nothing played");
            std::thread::sleep(Duration::from_millis(2));
        }
        // Playing while most of the sentence is still to come.
        assert!(made.load(Ordering::SeqCst) < 10, "{} pieces made before the first played", made.load(Ordering::SeqCst));
        assert!(r.speaker.stop());
        std::thread::sleep(Duration::from_millis(100));
        // The stop ended the sentence being made, not just the playing.
        assert!(made.load(Ordering::SeqCst) < 20, "made all {} pieces", made.load(Ordering::SeqCst));
        assert_eq!(r.events(), vec![ev(true, "a"), ev(false, "a")]);
    }

    // ---- ElevenLabs through the speaker, against a local stand-in

    use elevenlabs::standin::{pcm_bytes, serial, stub, Reply};

    const KEY: &str = "sk_speaker_test_key_0001";

    fn eleven_job(base: &str, sentences: &[&str], fallback: Option<Fallback>) -> Job {
        Job {
            message_id: Some("a".into()),
            sentences: sentences.iter().map(|s| s.to_string()).collect(),
            engine: Arc::new(elevenlabs::ElevenLabs::new(KEY, "", true).at(base)),
            voice: "JBFqnCBsd6RMkjVDRZzb".into(),
            rate: 1.0,
            fallback,
            track: None,
            first: 0,
            feed: None,
        }
    }

    #[test]
    fn elevenlabs_streams_each_sentence_into_the_output() {
        let _one_at_a_time = serial();
        let r = rig();
        let pcm = || Reply::Pcm { bytes: pcm_bytes(9_600), pieces: vec![4_801, 4_800, 9_599], delay: Duration::ZERO };
        let s = stub(vec![pcm(), pcm()]);
        let play = Arc::new(SimPlayback::default());
        let p = Arc::clone(&play);
        let opener: Opener = Box::new(move || Ok(Opened { play: p, keep: None }));
        r.speaker.speak(eleven_job(&s.base, &["One sentence here.", "And another one."], None), opener).unwrap();
        r.wait_quiet();
        assert_eq!(r.events(), vec![ev(true, "a"), ev(false, "a")]);
        // Each sentence its own request, its audio handed on in pieces.
        let asked = s.requests.lock().unwrap().clone();
        assert_eq!(asked.len(), 2);
        assert!(asked[0].contains(r#""text":"One sentence here.""#) && asked[1].contains(r#""text":"And another one.""#));
        assert!(play.appended.load(Ordering::SeqCst) >= 4, "{} pieces", play.appended.load(Ordering::SeqCst));
    }

    #[test]
    fn elevenlabs_failing_hands_over_to_the_local_voice_and_is_remembered() {
        let _one_at_a_time = serial();
        let r = rig();
        let s = stub(vec![
            Reply::Pcm { bytes: pcm_bytes(4_800), pieces: vec![9_600], delay: Duration::ZERO },
            Reply::Error(401, r#"{"detail":{"type":"authentication_error","code":"invalid_api_key","message":"Invalid API key"}}"#.into()),
        ]);
        let local_said = Arc::new(Mutex::new(Vec::new()));
        let told: Told = Arc::default();
        let job = eleven_job(&s.base, &["First sentence works.", "Then the key fails.", "And one more."], Some(fallback(&local_said, true, &told)));
        r.speaker.speak(job, sim_opener()).unwrap();
        r.wait_quiet();
        // Asked twice (never a third time, never a retry), then the local voice.
        assert_eq!(s.requests.lock().unwrap().len(), 2);
        assert_eq!(*local_said.lock().unwrap(), vec!["Then the key fails.", "And one more."]);
        let told = told.lock().unwrap().clone();
        assert_eq!(told, vec![("ElevenLabs: API key not accepted".to_string(), vec!["Then the key fails.".to_string(), "And one more.".to_string()], true)]);
        assert_eq!(r.events(), vec![ev(true, "a"), ev(false, "a")]);
        // On record: the next answers skip ElevenLabs for a while.
        let key = elevenlabs::Key::new(KEY).fingerprint();
        let choice = elevenlabs::ElevenLabs::choice_fingerprint(KEY, "JBFqnCBsd6RMkjVDRZzb", "");
        assert!(cloud::with_health(|h| h.skipping(elevenlabs::ID, key, choice, Instant::now())));
        cloud::with_health(|h| h.worked(elevenlabs::ID, key));
    }

    /// First audio — from `speak` to the first piece reaching the output —
    /// for Kokoro (an `engines` build with it installed; `YAP_TTS_MODELS`)
    /// cold and warm, and for ElevenLabs' path against a local stand-in that
    /// answers after `YAP_TTS_STANDIN_MS` (default 0: Yap's own share).
    /// Nothing is played. `cargo test --features engines
    /// tts::tests::first_audio_latency -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn first_audio_latency() {
        #[derive(Default)]
        struct Timing {
            first: Mutex<Option<Instant>>,
            inner: SimPlayback,
        }
        impl Playback for Timing {
            fn append(&self, pcm: Pcm) {
                self.first.lock().unwrap().get_or_insert_with(Instant::now);
                self.inner.append(pcm);
            }
            fn stop(&self) {
                self.inner.stop();
            }
            fn done(&self) -> bool {
                self.inner.done()
            }
        }
        let text = "Hi, I'm the voice that reads Chat's answers. Ask me anything, and I'll say what I found.";
        let measure = |engine: Arc<dyn Engine>, voice: &str| -> Duration {
            let r = rig();
            let t = Arc::new(Timing::default());
            let t2 = Arc::clone(&t);
            let opener: Opener = Box::new(move || Ok(Opened { play: t2, keep: None }));
            let job = Job {
                message_id: Some("m".into()),
                sentences: engine.prepare(sentences(text)),
                engine,
                voice: voice.into(),
                rate: 1.0,
                fallback: None,
                track: None,
                first: 0,
                feed: None,
            };
            let start = Instant::now();
            r.speaker.speak(job, opener).unwrap();
            while t.first.lock().unwrap().is_none() {
                assert!(start.elapsed() < Duration::from_secs(60), "no audio");
                std::thread::sleep(Duration::from_millis(1));
            }
            let took = t.first.lock().unwrap().unwrap() - start;
            r.speaker.stop();
            took
        };
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let median = |mut v: Vec<Duration>| {
            v.sort();
            v[v.len() / 2]
        };
        if kokoro::available() {
            kokoro::unload(Duration::from_secs(5));
            let cold = measure(Arc::new(kokoro::Kokoro), kokoro::DEFAULT_VOICE);
            let warm = median((0..5).map(|_| measure(Arc::new(kokoro::Kokoro), kokoro::DEFAULT_VOICE)).collect());
            println!("Kokoro ({}): first audio cold {:.0} ms, warm median {:.0} ms", kokoro::DEFAULT_VOICE, ms(cold), ms(warm));
        } else {
            println!("Kokoro: not available in this build/PC (needs --features engines and YAP_TTS_MODELS)");
        }
        let _one_at_a_time = serial();
        let delay = Duration::from_millis(std::env::var("YAP_TTS_STANDIN_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(0));
        let runs: Vec<Duration> = (0..7)
            .map(|_| {
                let s = stub(vec![Reply::Pcm { bytes: pcm_bytes(48_000), pieces: vec![4_096; 23].into_iter().chain([1_792]).collect(), delay }]);
                measure(eleven_job(&s.base, &[], None).engine, "JBFqnCBsd6RMkjVDRZzb")
            })
            .collect();
        println!("ElevenLabs path, stand-in answering after {} ms: first audio median {:.1} ms", delay.as_millis(), ms(median(runs)));
    }

    /// The speaker's real output path (rodio on the default device) playing
    /// **silence** from the stand-in engine — nothing audible: it plays to
    /// its end, and a stop ends it at once. Run with `cargo test
    /// tts::tests::rodio_output_with_silence -- --ignored`.
    #[test]
    #[ignore]
    fn rodio_output_with_silence() {
        let r = rig();
        let job = |id: &str| Job {
            message_id: Some(id.into()),
            sentences: vec!["One two three four.".into(), "Five six.".into()],
            engine: Arc::new(StandIn),
            voice: String::new(),
            rate: 2.0,
            fallback: None,
            track: None,
            first: 0,
            feed: None,
        };
        let started = Instant::now();
        r.speaker.speak(job("a"), rodio_opener(None)).unwrap();
        r.wait_quiet();
        let took = started.elapsed();
        // 6 words at 2×: ~0.75 s of silence played through.
        assert!(took >= Duration::from_millis(600), "ended after {took:?}");
        assert_eq!(r.events(), vec![ev(true, "a"), ev(false, "a")]);
        r.speaker.speak(job("b"), rodio_opener(None)).unwrap();
        std::thread::sleep(Duration::from_millis(200));
        assert!(r.speaker.stop());
        assert!(!r.speaker.state().speaking);
    }
}
